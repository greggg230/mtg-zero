//! The support report: which cards the engine plays and what blocks the rest
//! (ENGINE.md 3). The staging tool for porting cards: features that block
//! the most cards on their own come first.

use crate::db::CardDb;
use std::collections::BTreeMap;
use std::fmt::Write;

pub struct FeatureCount {
    pub feature: String,
    /// Cards this feature blocks (possibly among others).
    pub cards: usize,
    /// Cards this feature alone blocks (supporting it unlocks them).
    pub alone: usize,
}

pub struct SupportReport {
    pub supported: Vec<String>,
    pub blocked: Vec<(String, Vec<String>)>,
}

impl SupportReport {
    pub fn from_db(db: &CardDb) -> SupportReport {
        SupportReport {
            supported: db.supported().map(str::to_string).collect(),
            blocked: db.unsupported().iter().map(|(n, b)| (n.clone(), b.clone())).collect(),
        }
    }

    pub fn total(&self) -> usize {
        self.supported.len() + self.blocked.len()
    }

    /// Features by the number of cards they block alone, then by cards.
    pub fn features(&self) -> Vec<FeatureCount> {
        let mut m: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for (_, bs) in &self.blocked {
            for f in bs {
                let e = m.entry(f.as_str()).or_default();
                e.0 += 1;
                if bs.len() == 1 {
                    e.1 += 1;
                }
            }
        }
        let mut v: Vec<FeatureCount> = m
            .into_iter()
            .map(|(f, (cards, alone))| FeatureCount { feature: f.to_string(), cards, alone })
            .collect();
        v.sort_by(|a, b| b.alone.cmp(&a.alone).then(b.cards.cmp(&a.cards)).then(a.feature.cmp(&b.feature)));
        v
    }

    /// Cards blocked by exactly `n` features.
    pub fn blocked_by(&self, n: usize) -> Vec<&(String, Vec<String>)> {
        self.blocked.iter().filter(|(_, b)| b.len() == n).collect()
    }

    /// Greedy staging: repeatedly add the feature that completes the most
    /// cards (ties: the feature blocking the most cards). Returns (feature,
    /// cards completed by it, cumulative supported cards) per step.
    pub fn greedy(&self, steps: usize) -> Vec<(String, usize, usize)> {
        let mut done: Vec<&str> = Vec::new();
        let mut open: Vec<&Vec<String>> = self.blocked.iter().map(|(_, b)| b).collect();
        let mut supported = self.supported.len();
        let mut out = Vec::new();
        for _ in 0..steps {
            let mut best: Option<(&str, usize, usize)> = None;
            let mut cand: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for b in &open {
                let missing: Vec<&str> =
                    b.iter().map(|s| s.as_str()).filter(|f| !done.contains(f)).collect();
                for f in &missing {
                    let e = cand.entry(f).or_default();
                    e.1 += 1;
                    if missing.len() == 1 {
                        e.0 += 1;
                    }
                }
            }
            for (f, (alone, cards)) in cand {
                if best.is_none_or(|(_, a, c)| (alone, cards) > (a, c)) {
                    best = Some((f, alone, cards));
                }
            }
            let Some((f, alone, _)) = best else { break };
            done.push(f);
            open.retain(|b| !b.iter().all(|x| done.contains(&x.as_str())));
            supported += alone;
            out.push((f.to_string(), alone, supported));
        }
        out
    }

    pub fn render(&self, top: usize) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "supported {} / {} cards", self.supported.len(), self.total());
        let _ = writeln!(s, "  {}", self.supported.join(", "));
        let _ = writeln!(s, "blocked by one feature: {} cards", self.blocked_by(1).len());
        let _ = writeln!(s, "{:<64} {:>6} {:>6}", "feature", "alone", "cards");
        for f in self.features().into_iter().take(top) {
            let _ = writeln!(s, "{:<64} {:>6} {:>6}", f.feature, f.alone, f.cards);
        }
        s
    }
}

/// What makes a card a threat (a way to win games), for the threat view of
/// the support report. Planeswalkers are not counted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Threat {
    Creature,
    Vehicle,
    Equipment,
    /// A land that becomes a creature.
    Manland,
    /// Makes creature tokens.
    Tokens,
    /// Damage or life loss that can reach a player.
    Burn,
}

/// Classify every card of an IR document (raw JSON, so blocked cards are
/// classified too) as a threat, or not.
pub fn threat_kinds(ir_json: &str) -> BTreeMap<String, Threat> {
    let doc: serde_json::Value = serde_json::from_str(ir_json).expect("IR JSON");
    let mut out = BTreeMap::new();
    for card in doc["cards"].as_array().expect("cards") {
        let name = card["name"].as_str().expect("name").to_string();
        if let Some(t) = threat_of(&card["printing"]) {
            out.insert(name, t);
        }
    }
    out
}

fn threat_of(p: &serde_json::Value) -> Option<Threat> {
    let has = |key: &str, v: &str| p[key].as_array().is_some_and(|a| a.iter().any(|x| x == v));
    if has("card_types", "Planeswalker") {
        return None;
    }
    if has("card_types", "Creature") {
        return Some(Threat::Creature);
    }
    if has("subtypes", "Vehicle") {
        return Some(Threat::Vehicle);
    }
    if has("subtypes", "Equipment") {
        return Some(Threat::Equipment);
    }
    let mut tags = Vec::new();
    collect_tags(p, &mut tags);
    if has("card_types", "Land") && tags.iter().any(|t| t.contains("BecomeLandCreature")) {
        return Some(Threat::Manland);
    }
    let creature_token = |v: &serde_json::Value| {
        let mut found = false;
        walk(v, &mut |x| {
            if x["$t"] == "CardPrinting" && x["card_types"].as_array().is_some_and(|a| a.iter().any(|t| t == "Creature")) {
                found = true;
            }
        });
        found
    };
    if creature_token(&p["spell_effect"])
        || creature_token(&p["triggered_abilities"])
        || creature_token(&p["activated_abilities"])
        || tags.iter().any(|t| t.starts_with("CreateTokens") || t == "Mobilize" || t == "Amass")
    {
        return Some(Threat::Tokens);
    }
    const BURN: &[&str] = &[
        "DealDamageAnyTarget",
        "DealDamageEachOpponent",
        "EachOpponentLosesLife",
        "DealXDamage",
        "DealDamageThenChainCopy",
        "TargetOpponentLosesLife",
    ];
    if tags.iter().any(|t| BURN.contains(&t.as_str())) {
        return Some(Threat::Burn);
    }
    None
}

fn walk(v: &serde_json::Value, f: &mut impl FnMut(&serde_json::Value)) {
    f(v);
    match v {
        serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, f)),
        serde_json::Value::Object(m) => m.values().for_each(|x| walk(x, f)),
        _ => {}
    }
}

fn collect_tags(v: &serde_json::Value, out: &mut Vec<String>) {
    walk(v, &mut |x| {
        if let Some(t) = x["$t"].as_str() {
            out.push(t.to_string());
        }
    });
}

/// One feature in the threat view: threats it blocks alone, threats it
/// blocks with others, and those threats.
pub struct ThreatFeature {
    pub feature: String,
    pub alone: usize,
    pub threats: usize,
    pub cards: Vec<String>,
}

impl SupportReport {
    /// The threat view: features by the number of blocked threats they
    /// block alone, then by threats blocked (ENGINE.md 16).
    pub fn threat_features(&self, kinds: &BTreeMap<String, Threat>) -> Vec<ThreatFeature> {
        let mut m: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
        for (name, bs) in &self.blocked {
            if !kinds.contains_key(name) {
                continue;
            }
            for f in bs {
                let e = m.entry(f.as_str()).or_default();
                if bs.len() == 1 {
                    e.0 += 1;
                }
                e.1.push(name.clone());
            }
        }
        let mut v: Vec<ThreatFeature> = m
            .into_iter()
            .map(|(f, (alone, cards))| ThreatFeature { feature: f.to_string(), alone, threats: cards.len(), cards })
            .collect();
        v.sort_by(|a, b| b.alone.cmp(&a.alone).then(b.threats.cmp(&a.threats)).then(a.feature.cmp(&b.feature)));
        v
    }

    /// Greedy staging over threats only: the feature completing the most
    /// threats next (ties: the one blocking the most threats). Returns
    /// (feature, threats completed, the threats).
    pub fn threat_greedy(&self, kinds: &BTreeMap<String, Threat>, steps: usize) -> Vec<(String, usize, Vec<String>)> {
        let mut open: Vec<(&String, &Vec<String>)> =
            self.blocked.iter().filter(|(n, _)| kinds.contains_key(n)).map(|(n, b)| (n, b)).collect();
        let mut done: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for _ in 0..steps {
            let mut cand: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for (_, b) in &open {
                let missing: Vec<&str> = b.iter().map(|s| s.as_str()).filter(|f| !done.contains(f)).collect();
                for f in &missing {
                    let e = cand.entry(f).or_default();
                    e.1 += 1;
                    if missing.len() == 1 {
                        e.0 += 1;
                    }
                }
            }
            let mut best: Option<(&str, usize, usize)> = None;
            for (f, (alone, n)) in cand {
                if best.is_none_or(|(_, a, c)| (alone, n) > (a, c)) {
                    best = Some((f, alone, n));
                }
            }
            let Some((f, _, _)) = best else { break };
            done.push(f);
            let (fin, rest): (Vec<_>, Vec<_>) =
                open.into_iter().partition(|(_, b)| b.iter().all(|x| done.contains(&x.as_str())));
            open = rest;
            out.push((f.to_string(), fin.len(), fin.into_iter().map(|(n, _)| n.clone()).collect()));
        }
        out
    }
}
