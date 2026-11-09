//! A spectator's view of a game (zero-core `export_state`, its docs/replays.md): the replay
//! recorder stores it at every step and the replay viewer (delverai.greggg230.com) draws the
//! board from it. Everything is shown, both hands included (a spectator, not a seat), with the
//! pending decision's options in words and the tail of the game log (spec `"log": true`).
//!
//! Read only: nothing in a game depends on it (behaviour digests unchanged).

use super::MtgAz;
use crate::db::{types, AbilitySlot, Counter, Kw};
use crate::decision::{Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, StackKind, Step};
use crate::ids::{CardIdx, Target, Zone};
use serde_json::{json, Value};

/// Bump when the shape changes (the viewer checks it).
pub const VIEW_VERSION: u32 = 1;

/// Log lines carried by each exported state (the viewer rebuilds the whole log from the
/// consecutive states' tails and `log_n`).
const LOG_TAIL: usize = 40;

const KWS: [(Kw, &str); 17] = [
    (Kw::Flying, "flying"),
    (Kw::Reach, "reach"),
    (Kw::FirstStrike, "first strike"),
    (Kw::DoubleStrike, "double strike"),
    (Kw::Deathtouch, "deathtouch"),
    (Kw::Trample, "trample"),
    (Kw::Lifelink, "lifelink"),
    (Kw::Vigilance, "vigilance"),
    (Kw::Haste, "haste"),
    (Kw::Defender, "defender"),
    (Kw::Menace, "menace"),
    (Kw::Flash, "flash"),
    (Kw::Hexproof, "hexproof"),
    (Kw::Shroud, "shroud"),
    (Kw::Indestructible, "indestructible"),
    (Kw::Infect, "infect"),
    (Kw::Shadow, "shadow"),
];

const TYPES: [(u16, &str); 9] = [
    (types::LAND, "land"),
    (types::CREATURE, "creature"),
    (types::ARTIFACT, "artifact"),
    (types::ENCHANTMENT, "enchantment"),
    (types::PLANESWALKER, "planeswalker"),
    (types::BATTLE, "battle"),
    (types::INSTANT, "instant"),
    (types::SORCERY, "sorcery"),
    (types::KINDRED, "kindred"),
];

const COLORS: [&str; 6] = ["white", "blue", "black", "red", "green", "colorless"];

fn step_name(s: Step) -> &'static str {
    match s {
        Step::Setup => "setup",
        Step::Untap => "untap",
        Step::Upkeep => "upkeep",
        Step::Draw => "draw",
        Step::Main1 => "main1",
        Step::BeginCombat => "begin_combat",
        Step::DeclareAttackers => "declare_attackers",
        Step::DeclareBlockers => "declare_blockers",
        Step::FirstStrikeDamage => "first_strike_damage",
        Step::CombatDamage => "combat_damage",
        Step::EndCombat => "end_combat",
        Step::Main2 => "main2",
        Step::End => "end",
        Step::Cleanup => "cleanup",
    }
}

fn counter_name(g: &Game, k: Counter) -> String {
    match k {
        Counter::P1P1 => "+1/+1".into(),
        Counter::M1M1 => "-1/-1".into(),
        Counter::Kw(kw) => KWS.iter().find(|(x, _)| *x == kw).map_or("keyword", |(_, n)| n).into(),
        Counter::Stun => "stun".into(),
        Counter::Named(id) => g.db().subtype_name(id).to_string(),
        Counter::Loyalty => "loyalty".into(),
        Counter::Lore => "lore".into(),
    }
}

impl Game {
    fn view_types(&self, c: CardIdx) -> Vec<&'static str> {
        let t = self.types_of(c);
        TYPES.iter().filter(|(b, _)| t & b != 0).map(|(_, n)| *n).collect()
    }

    /// A card in a hand, graveyard, exile or the command zone.
    fn view_card(&self, c: CardIdx) -> Value {
        let o = self.obj(c);
        let d = self.def_of(c);
        let mut v = json!({"id": c.0, "name": self.name(c), "owner": o.owner});
        if o.zone == Zone::FaceDown {
            v["face_down"] = json!(true);
        }
        if d.token {
            v["token"] = json!(true);
        }
        v
    }

    fn view_permanent(&self, c: CardIdx) -> Value {
        let o = self.obj(c);
        let d = self.def_of(c);
        let mut v = json!({
            "id": c.0,
            "name": self.name(c),
            "owner": o.owner,
            "controller": o.controller,
            "types": self.view_types(c),
            "tapped": o.tapped,
        });
        if d.token {
            v["token"] = json!(true);
        }
        if self.is_creature(c) {
            let (p, t) = self.pt(c);
            v["pt"] = json!([p, t]);
            if self.is_sick(c) && !self.has_kw(c, Kw::Haste) {
                v["sick"] = json!(true);
            }
        }
        if o.damage > 0 {
            v["damage"] = json!(o.damage);
        }
        let counters: serde_json::Map<String, Value> =
            self.counters_of(c).into_iter().map(|(k, n)| (counter_name(self, k), json!(n))).collect();
        if !counters.is_empty() {
            v["counters"] = Value::Object(counters);
        }
        let kws = self.keywords_of(c);
        let kw: Vec<&str> = KWS.iter().filter(|(k, _)| kws.has(*k)).map(|(_, n)| *n).collect();
        if !kw.is_empty() {
            v["kw"] = json!(kw);
        }
        if o.attacking {
            v["attacking"] = json!(true);
            if let Some(pw) = o.attacks {
                v["attacks"] = json!(pw.idx.0);
            }
            if o.blocked {
                v["blocked"] = json!(true);
            }
        }
        if let Some(a) = o.blocking {
            v["blocking"] = json!(a.0);
        }
        if let Some(h) = o.attached {
            if self.live(h) {
                v["attached"] = json!(h.idx.0);
            }
        }
        v
    }

    fn view_target(&self, t: Target) -> Value {
        match t {
            Target::Player(p) => json!({"player": p}),
            Target::Obj(r) => json!({"id": r.idx.0, "name": self.name(r.idx), "live": self.live(r)}),
            Target::Stack(id) => json!({"stack": id, "name": self.describe_target(t)}),
        }
    }

    fn view_stack(&self) -> Vec<Value> {
        self.stack
            .iter()
            .map(|s| {
                let mut v = match s.kind {
                    StackKind::Spell { card } => json!({"stack": s.id, "id": card.0, "name": self.name(card), "spell": true}),
                    StackKind::Ability { ability, source } => json!({
                        "stack": s.id,
                        "id": source.idx.0,
                        "name": self.db().def(ability.def).name,
                        "ability": ability_kind(ability.slot),
                    }),
                };
                v["controller"] = json!(s.controller);
                if !s.targets.is_empty() {
                    v["targets"] = s.targets.iter().map(|&(_, t)| self.view_target(t)).collect();
                }
                if s.x > 0 {
                    v["x"] = json!(s.x);
                }
                if !s.modes.is_empty() {
                    v["modes"] = json!(s.modes.iter().map(|m| m + 1).collect::<Vec<_>>());
                }
                v
            })
            .collect()
    }

    fn view_player(&self, p: u8) -> Value {
        let pl = self.player(p);
        let exile: Vec<Value> = self
            .exile
            .iter()
            .chain(self.face_down.iter())
            .filter(|&&c| self.obj(c).owner == p)
            .map(|&c| self.view_card(c))
            .collect();
        let mut pool = String::new();
        for (i, &n) in pl.pool.amt.iter().enumerate() {
            for _ in 0..n.min(20) {
                pool.push(crate::mana::LETTERS[i]);
            }
        }
        let mut v = json!({
            "id": p,
            "life": pl.life,
            "library": pl.library.len(),
            "hand": pl.hand.iter().map(|&c| self.view_card(c)).collect::<Vec<_>>(),
            "graveyard": pl.graveyard.iter().map(|&c| self.view_card(c)).collect::<Vec<_>>(),
            "exile": exile,
            "battlefield": self.permanents_of(p).map(|c| self.view_permanent(c)).collect::<Vec<_>>(),
            "lands_played": pl.lands_played,
            "mulligans": pl.mulligans,
        });
        if !pool.is_empty() {
            v["pool"] = json!(pool);
        }
        if pl.poison > 0 {
            v["poison"] = json!(pl.poison);
        }
        if pl.energy > 0 {
            v["energy"] = json!(pl.energy);
        }
        if pl.lost {
            v["lost"] = json!(true);
        }
        v
    }

    /// The asking task's kind name (context for yes / no questions).
    fn asking_task(&self) -> Option<String> {
        let t = format!("{:?}", self.agenda.last()?);
        let end = t.find(|ch: char| !ch.is_alphanumeric()).unwrap_or(t.len());
        Some(t[..end].to_string())
    }

    /// The card a choice is about, for the viewer to point at.
    fn choice_card(&self, c: Choice) -> Option<CardIdx> {
        match c {
            Choice::Card(i)
            | Choice::Play(i)
            | Choice::Cast(i)
            | Choice::Ability(i, _)
            | Choice::CastFace(i)
            | Choice::PlayFace(i)
            | Choice::Faces(i, _) => Some(i),
            Choice::Target(Target::Obj(r)) => Some(r.idx),
            _ => None,
        }
    }

    fn other_face_name(&self, i: CardIdx) -> &'static str {
        let d = self.def_of(i);
        match d.back.or(d.adventure) {
            Some(b) => &self.db().def(b).name,
            None => self.name(i),
        }
    }

    /// An option in words, for the decision `kind` (the pending one; the position lab's forced
    /// labels, `lab.rs`).
    pub(crate) fn option_text(&self, kind: DecisionKind, c: Choice) -> String {
        use DecisionKind as K;
        let nm = |i: CardIdx| self.name(i).to_string();
        let tgt = |t: Target| match t {
            Target::Player(p) => format!("P{}", p + 1),
            _ => self.describe_target(t).split('#').next().unwrap_or("").to_string(),
        };
        match (kind, c) {
            (K::Priority, Choice::Pass) => "Pass priority".into(),
            (K::Priority, Choice::Play(i)) => format!("Play {}", nm(i)),
            (K::Priority, Choice::Cast(i)) => format!("Cast {}", nm(i)),
            (K::Priority, Choice::CastFace(i)) => format!("Cast {} ({})", self.other_face_name(i), nm(i)),
            (K::Priority, Choice::PlayFace(i)) => format!("Play {} ({})", self.other_face_name(i), nm(i)),
            (K::Priority, Choice::Faces(i, _)) => format!("Cast or play {}", nm(i)),
            (K::Priority, Choice::Companion) => "Put the companion into hand".into(),
            (K::Priority, Choice::FromGraveyard) => "Cast or play from the graveyard".into(),
            (K::ChooseGraveyardCard, Choice::Cast(i)) => format!("Cast {}", nm(i)),
            (K::ChooseGraveyardCard, Choice::Play(i)) => format!("Play {}", nm(i)),
            (K::ChooseGraveyardCard, Choice::CastFace(i)) => format!("Cast {} ({})", self.other_face_name(i), nm(i)),
            (K::ChooseGraveyardCard, Choice::PlayFace(i)) => format!("Play {} ({})", self.other_face_name(i), nm(i)),
            (K::ChooseGraveyardCard, Choice::Faces(i, _)) => format!("Cast or play {}", nm(i)),
            (_, Choice::Ability(i, slot)) => {
                let d = self.def_of(i);
                let ad = match slot {
                    AbilitySlot::Activated(k) => d.activated.get(k as usize),
                    _ => None,
                };
                let loyalty = ad.and_then(|a| a.loyalty);
                // A mana ability that taps its permanent: at priority, floating
                // mana in response; in a cost's choice, before the permanent is
                // sacrificed or returned (ENGINE.md 86).
                let taps_for_mana = ad.is_some_and(|a| a.mana().is_some() && a.cost.tap);
                match (kind, loyalty) {
                    (K::PayMana, _) => format!("Tap {} for mana", nm(i)),
                    (K::Sacrifice, _) => format!("Tap {} for mana, then sacrifice it", nm(i)),
                    (K::BounceCost, _) => format!("Tap {} for mana, then return it to hand", nm(i)),
                    (K::Priority, _) if taps_for_mana => format!("Tap {} for mana", nm(i)),
                    (_, Some(l)) => format!("Activate {} ({}{})", nm(i), if l > 0 { "+" } else { "" }, l),
                    _ => format!("Activate {}", nm(i)),
                }
            }
            (K::Mulligan, Choice::Pass) => "Keep".into(),
            (K::Mulligan, Choice::Yes) => "Mulligan".into(),
            (K::MulliganBottom, Choice::Card(i)) => format!("Put {} on the bottom", nm(i)),
            (K::Target | K::NewTarget, Choice::Target(t)) => format!("Target {}", tgt(t)),
            (K::Target, Choice::Pass) => "No more targets".into(),
            (K::NewTarget, Choice::Pass) => "Keep the target".into(),
            (K::PayMana, Choice::Card(i)) => format!("Tap {} for mana", nm(i)),
            (K::DeclareAttackers, Choice::Card(i)) => format!("Attack with {}", nm(i)),
            (K::DeclareAttackers, Choice::Pass) => "No (more) attackers".into(),
            (K::AttackTarget, Choice::Target(t)) => format!("Attack {}", tgt(t)),
            (K::DeclareBlockers, _) => {
                let blocker = match self.agenda.last() {
                    Some(Task::DeclareBlockers { i }) => self.potential_blockers().get(*i as usize).map(|&b| nm(b)),
                    _ => None,
                };
                let b = blocker.unwrap_or_else(|| "The next blocker".into());
                match c {
                    Choice::Card(a) => format!("{b} blocks {}", nm(a)),
                    _ => format!("{b} doesn't block"),
                }
            }
            (K::AssignDamage, Choice::Number(n)) => {
                let to = match self.agenda.last() {
                    Some(Task::AssignDamage { att, j, .. }) => self.blockers_of(*att).get(*j as usize).map(|&b| nm(b)),
                    _ => None,
                };
                format!("{n} damage to {}", to.unwrap_or_else(|| "the blocker".into()))
            }
            (K::YesNo, Choice::Yes) => "Yes".into(),
            (K::YesNo, Choice::Pass) => "No".into(),
            (K::ChooseHandCard, Choice::Card(i)) => format!("Choose {}", nm(i)),
            (K::Discard | K::DiscardCost, Choice::Card(i)) => format!("Discard {}", nm(i)),
            (K::Sacrifice, Choice::Card(i)) => format!("Sacrifice {}", nm(i)),
            (K::ExileCost, Choice::Card(i)) => format!("Exile {}", nm(i)),
            (K::BounceCost, Choice::Card(i)) => format!("Return {} to hand", nm(i)),
            (K::LegendRule, Choice::Card(i)) => format!("Keep {}", nm(i)),
            (K::ChoosePermanent | K::ChooseGraveyardCard | K::ChooseExiled, Choice::Card(i)) => format!("Choose {}", nm(i)),
            (K::ChoosePermanent | K::ChooseGraveyardCard | K::ChooseExiled, Choice::Pass) => "No more".into(),
            (K::OrderTriggers, Choice::Trigger(k)) => {
                let p = self.to_move();
                let src = self
                    .waiting_triggers()
                    .iter()
                    .filter(|t| Some(t.controller) == p)
                    .nth(k as usize)
                    .map(|t| self.db().def(t.source_def).name.clone());
                format!("Put the {} trigger on the stack first", src.unwrap_or_else(|| format!("#{}", k + 1)))
            }
            (K::SearchLibrary, Choice::Def(d)) => format!("Find {}", self.db().def(d).name),
            (K::SearchLibrary, Choice::Pass) => "Find nothing".into(),
            (K::PutOnTop, Choice::Def(d)) => format!("{} on top", self.db().def(d).name),
            (K::PutOnBottom, Choice::Def(d)) => format!("{} on the bottom", self.db().def(d).name),
            (K::PutInGraveyard, Choice::Def(d)) => format!("{} into the graveyard", self.db().def(d).name),
            (K::PutInHand, Choice::Def(d)) => format!("{} into hand", self.db().def(d).name),
            (K::PutOntoBattlefield, Choice::Def(d)) => format!("{} onto the battlefield", self.db().def(d).name),
            (K::PutOnBottom | K::PutInGraveyard | K::PutInHand | K::PutOntoBattlefield, Choice::Pass) => "Done".into(),
            (K::PutOnTop, Choice::Card(i)) => format!("{} on top", nm(i)),
            (K::PutOnBottom, Choice::Card(i)) => format!("{} on the bottom", nm(i)),
            (K::PutInGraveyard, Choice::Card(i)) => format!("{} into the graveyard", nm(i)),
            (K::PutInHand, Choice::Card(i)) => format!("{} into hand", nm(i)),
            (K::PutOntoBattlefield, Choice::Card(i)) => format!("{} onto the battlefield", nm(i)),
            (K::ChooseDiscard, Choice::Def(d)) => format!("They discard {}", self.db().def(d).name),
            (K::NameCard, Choice::Def(d)) => format!("Name {}", self.db().def(d).name),
            (K::ChooseMode, Choice::Number(n)) => format!("Mode {}", n + 1),
            (K::ChooseMode, Choice::Pass) => "No more modes".into(),
            (K::ChooseColor, Choice::Number(n)) => COLORS.get(n as usize).map_or(format!("Colour {n}"), |s| s.to_string()),
            (K::ChooseAltCost, Choice::Number(0)) => "Pay the mana cost".into(),
            (K::ChooseAltCost, Choice::Number(n)) => format!("Alternative cost {n}"),
            // (One another permanent gives it in hand, ENGINE.md 99.15.)
            (K::ChooseAltCost, Choice::Card(i)) => format!("The alternative cost {} gives", nm(i)),
            (K::ChooseCost, Choice::Number(n)) => format!("Additional cost {}", n + 1),
            (K::PayPhyrexian, Choice::Number(n)) => format!("Pay {} life ({n} Phyrexian)", 2 * n as u32),
            // A mana ability activated outside a payment (a float, ENGINE.md 86; Lion's Eye
            // Diamond): the mana it adds, e.g. "Add {R}".
            (K::ChooseMana, Choice::Number(n)) => match self.agenda.last() {
                Some(Task::ManaNowPick { prods, .. }) if (n as usize) < prods.len() => {
                    let v = prods[n as usize];
                    let s: String = (0..6).flat_map(|t| std::iter::repeat(crate::mana::LETTERS[t]).take(v[t] as usize)).map(|c| format!("{{{c}}}")).collect();
                    format!("Add {s}")
                }
                _ => format!("Mana option {}", n + 1),
            },
            (K::DivideDamage, Choice::Number(n)) => format!("{n} damage"),
            (K::ChooseFace, Choice::Number(0)) => "Front face".into(),
            (K::ChooseFace, Choice::Number(_)) => "Other face".into(),
            (K::PayPool, Choice::Number(n)) => format!("Leave mana option {}", n + 1),
            (K::PlayDraw, Choice::Target(Target::Player(p))) => {
                if Some(p) == self.to_move() { "Play first".into() } else { "Draw first".into() }
            }
            (K::RevealCompanion, Choice::Def(d)) => format!("Reveal {}", self.db().def(d).name),
            (K::RevealCompanion, Choice::Pass) => "No companion".into(),
            (_, Choice::Card(i)) => nm(i),
            (_, Choice::Def(d)) => self.db().def(d).name.clone(),
            (_, Choice::Target(t)) => format!("Target {}", tgt(t)),
            // (the engine's debug wording, without its slot numbers)
            _ => self.describe_choice(c).split('#').next().unwrap_or("").to_string(),
        }
    }

    fn view_decision(&self) -> Value {
        let Some(d) = self.pending_decision() else {
            return Value::Null;
        };
        let options: Vec<Value> = d
            .options
            .iter()
            .map(|o| {
                let mut v = json!({"a": o.action, "t": self.option_text(d.kind, o.choice)});
                if let Some(c) = self.choice_card(o.choice) {
                    v["c"] = json!(c.0);
                }
                v
            })
            .collect();
        let mut v = json!({"player": d.player, "kind": format!("{:?}", d.kind), "options": options});
        if let Some(t) = self.asking_task() {
            v["task"] = json!(t);
        }
        // A decision while a spell or ability resolves ("you may ...", a pile to order): its source.
        if let Some(Task::Run(f)) = self.agenda.last() {
            v["source"] = json!(self.db().def(f.source_def).name);
        }
        // What is being cast or activated while its costs and targets are chosen.
        if let Some(top) = self.stack.last() {
            if top.controller == d.player && !matches!(d.kind, DecisionKind::Priority) {
                v["for"] = json!(top.id);
            }
        }
        v
    }

    /// The whole view (`MtgAz::export_state`).
    pub fn spectator_view(&self) -> Value {
        let t = self.turn();
        let log = self.log_lines();
        let tail = &log[log.len().saturating_sub(LOG_TAIL)..];
        let mut v = json!({
            "v": VIEW_VERSION,
            "turn": t.number,
            "active": t.active,
            "step": step_name(t.step),
            "priority": t.priority,
            "starting": t.starting,
            "players": [self.view_player(0), self.view_player(1)],
            "stack": self.view_stack(),
            "decision": self.view_decision(),
            "log_n": log.len(),
            "log": tail,
        });
        if !self.command.is_empty() {
            v["command"] = self.command.iter().map(|&c| self.view_card(c)).collect();
        }
        if let Some(m) = self.monarch {
            v["monarch"] = json!(m);
        }
        if let Some(i) = self.initiative {
            v["initiative"] = json!(i);
        }
        if let Some(o) = self.outcome() {
            v["over"] = json!(true);
            v["winner"] = json!(o.winner);
            v["reason"] = json!(format!("{:?}", o.reason));
        }
        v["known"] = json!([self.view_known(0), self.view_known(1)]);
        v
    }

    /// Player `p`'s hidden cards and who knows them (`Obj.known`, ENGINE.md 10 and 72; exact knowledge only,
    /// not the partial "clouds"): `library`, its known cards top first as `[position from the top, mask,
    /// name]` (mask bit q: player q knows the card there); `hand`, the cards of its hand the other player
    /// knows, `[id, name]` (an Urza's Bauble look, a card returned from play); `order`, the library's card
    /// ids top first (for a seat's view to follow a look the engine does not remember: a Mishra's Bauble at
    /// a card its owner does not know, 23.7). A spectator sees everything; a seat's view (the Delver Lab
    /// Play tab) keeps only what that seat may know. Read only: no behaviour or encoding depends on it.
    fn view_known(&self, p: u8) -> Value {
        let pl = self.player(p);
        let library: Vec<Value> = pl
            .library
            .iter()
            .rev()
            .enumerate()
            .filter_map(|(i, &c)| {
                let mask: u8 = (0..2u8).filter(|&q| self.obj(c).known & (1 << q) != 0).fold(0, |m, q| m | (1 << q));
                (mask != 0).then(|| json!([i, mask, self.name(c)]))
            })
            .collect();
        let other = 1 - p;
        let hand: Vec<Value> =
            pl.hand.iter().filter(|&&c| self.obj(c).known & (1 << other) != 0).map(|&c| json!([c.0, self.name(c)])).collect();
        let order: Vec<u32> = pl.library.iter().rev().map(|&c| c.0 as u32).collect();
        json!({"library": library, "hand": hand, "order": order})
    }
}

fn ability_kind(slot: AbilitySlot) -> &'static str {
    match slot {
        AbilitySlot::Spell => "spell",
        AbilitySlot::Trigger(_) => "trigger",
        AbilitySlot::Activated(_) | AbilitySlot::Hand(_) | AbilitySlot::Graveyard(_) => "activated",
    }
}

impl MtgAz {
    /// The spectator view of the match (JSON): the current game's (`Game::spectator_view`), the
    /// match's score, and while sideboarding the decision as action ids only.
    pub fn spectator_json(&self) -> String {
        let mut v = self.g.spectator_view();
        // a position-lab game's line, rule and libraries (ENGINE.md 76; no other game has it)
        if let Some(l) = &self.lab {
            v["lab"] = l.view(&self.g);
        }
        v["match"] = json!({
            "format": format!("{:?}", self.m.format).to_lowercase(),
            "game": self.m.game_no,
            "wins": self.m.wins,
            "over": self.match_over(),
        });
        if let Some(sb) = &self.m.sb {
            v["decision"] = json!({
                "player": sb.decider(),
                "kind": "Sideboard",
                "options": sb.decision.options.iter().map(|o| json!({"a": o.action, "t": format!("{:?}", o.choice)})).collect::<Vec<_>>(),
            });
        }
        v.to_string()
    }
}
