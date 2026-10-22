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
