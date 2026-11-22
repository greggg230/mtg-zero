//! Building an arbitrary position from a description (ENGINE.md 95): the spec's optional `build`
//! block starts every game of `mtg` at the position it describes, as one seat (`viewer`) sees it,
//! with what that seat cannot see dealt at random; `describe` is the inverse - the description of an
//! engine position from a seat's view (the round-trip test, and the schema's executable
//! documentation). Made for playing real Magic Online games with the network: at each decision the
//! adapter describes what MTGO shows our seat, the engine builds it, and the usual search runs.
//!
//! ```json
//! "build": {"viewer": 0, "deal_seed": 7,
//!   "turn": 3, "active": 1, "step": "main1", "priority": 0, "passes": 1, "starting": 1,
//!   "decision": "priority",
//!   "players": [
//!     {"life": 19, "hand": ["Daze", {"name": "Force of Will", "known_to_opponent": true}],
//!      "library_size": 50, "library_known": [{"pos": 0, "name": "Ponder", "known_by": [0]}],
//!      "graveyard": ["Mishra's Bauble"], "drawn": 1, "lost_life": true},
//!     {"life": 20, "hand_size": 5, "hand": ["Brainstorm"], "library_size": 49,
//!      "library_known": [{"pos": 0, "known_by": [1]}, {"pos": 1, "known_by": [1]}],
//!      "graveyard": [], "lands_played": 1, "spells_cast": 1, "noncreature_cast": 1}],
//!   "battlefield": [{"name": "Volcanic Island", "owner": 0},
//!                   {"name": "Volcanic Island", "owner": 1, "tapped": true, "entered": 3},
//!                   {"name": "Dragon's Rage Channeler", "owner": 0, "entered": 2, "control_since": 2}],
//!   "exile": [],
//!   "stack": [{"spell": "Lightning Bolt", "controller": 1, "targets": [{"req": 0, "obj": "b2"}]}]}
//! ```
//!
//! References name objects by place: `b<i>` the i-th permanent of `battlefield` (timestamp order),
//! `g<p>.<i>` player p's graveyard card i (bottom first), `x<i>` exile card i, `s<i>` the stack's item
//! i (bottom first; as an object, its spell's card), `e<i>` emblem i, `h<p>.<i>` card i of player p's
//! described hand; any entry may also carry `"ref": "<alias>"` (an MTGO object id). A reference to an
//! earlier object of a card (a fetchland's ability whose source is now in the graveyard) is
//! `{"ref": "g0.3", "stale": true}`; a token that ceased to exist is `{"gone": "Clue", "owner": 0}`.
//! Either may carry what it shows (`Game::ref_lki`, ENGINE.md 95.11): `known_by` (who knew the object as
//! it left; default both) and `was` (its identity then, where the viewer knew it; default the card's now).
//!
//! The decks are the spec's lists (best-of-one, open decklists): every card of each list is placed
//! exactly once - the named cards where the description says, the rest in the unknown slots (the
//! opponent's unknown hand cards, both libraries' unknown positions); a description that does not
//! fit its deck is an error, never fixed. The unknown slots are then dealt (`determinize` for the
//! viewer from the deal seed): the same description gives the same encoding for every deal seed.
//!
//! Every field of the engine's state is described, derived (the last turn of each player, the
//! fast-path counts, the agenda of the decision), or unsupported: `describe` refuses a position with
//! an unsupported field set (an error naming it), and the builder has no key for it. The decisions:
//! priority (any step, any stack), declaring attackers / an attacker's target / blockers, the cleanup
//! discard, play-or-draw, mulligan and the mulligan's bottom; never a decision inside a resolving
//! spell or a cast being announced: those are reached by `then`, the actions (option labels or action
//! ids) taken from the built decision on, before the deal (the opponent's Daze resolving and asking
//! us to pay: the priority before it, then `["Pass priority"]`).
use super::matches::MatchState;
use super::view::{counter_name, step_name, KWS, TYPES};
use super::{MtgAz, MtgSpec};
use crate::db::{AbilityRef, AbilitySlot, CardDb, Counter, DelayedWhen, KwSet, Mod};
use crate::decision::DecisionKind;
use crate::flow::Task;
use crate::game::{CastFrom, Delayed, Game, GameSpec, Mark, StackItem, StackKind, Step, TempTrigger};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::layers::{Affected, Effect, Until};
use crate::triggers::EvCtx;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// The description's format (its `format` key, optional; `describe` writes it).
pub const FORMAT: &str = "mtg-build/1";

/// The deal's own stream: split from the deal seed with this salt.
const SALT: u64 = 0xB01D_5EED_D3A1_0093;

/// A spec's `build` block (module docs): the description, checked when the spec is parsed (a trial
/// build) and built for every game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub desc: Value,
    pub viewer: PlayerId,
    pub deal_seed: Option<u64>,
}

impl Build {
    pub fn parse(v: &Value) -> Result<Build, String> {
        let o = v.as_object().ok_or("mtg spec: build must be an object (docs/ENGINE.md 95)")?;
        let viewer = match o.get("viewer").and_then(|x| x.as_u64()) {
            Some(p) if p < 2 => p as PlayerId,
            _ => return Err("mtg spec: build.viewer (0 or 1, the seat whose view it is) is required".into()),
        };
        let deal_seed = match o.get("deal_seed") {
            None | Some(Value::Null) => None,
            Some(x) => Some(x.as_u64().ok_or("mtg spec: build.deal_seed must be a non-negative integer")?),
        };
        if let Some(f) = o.get("format") {
            if f != FORMAT {
                return Err(format!("mtg spec: build.format {f} (this engine builds {FORMAT:?})"));
            }
        }
        Ok(Build { desc: v.clone(), viewer, deal_seed })
    }
}

// ---------------------------------------------------------------------------------------------
// names

fn step_of(s: &str) -> Result<Step, String> {
    use Step::*;
    [Setup, Untap, Upkeep, Draw, Main1, BeginCombat, DeclareAttackers, DeclareBlockers, FirstStrikeDamage, CombatDamage, EndCombat, Main2, End, Cleanup]
        .into_iter()
        .find(|&x| step_name(x) == s)
        .ok_or_else(|| format!("build: unknown step {s:?}"))
}

fn zone_name(z: Zone) -> &'static str {
    match z {
        Zone::Library => "library",
        Zone::Hand => "hand",
        Zone::Battlefield => "battlefield",
        Zone::Graveyard => "graveyard",
        Zone::Stack => "stack",
        Zone::Exile => "exile",
        Zone::Nowhere => "nowhere",
        Zone::Command => "command",
        Zone::FaceDown => "face_down",
        Zone::PhasedOut => "phased_out",
    }
}

fn zone_of(s: &str) -> Result<Zone, String> {
    use Zone::*;
    [Library, Hand, Battlefield, Graveyard, Stack, Exile, Nowhere, Command, FaceDown, PhasedOut]
        .into_iter()
        .find(|&z| zone_name(z) == s)
        .ok_or_else(|| format!("build: unknown zone {s:?}"))
}

fn counter_of(db: &CardDb, s: &str) -> Result<Counter, String> {
    Ok(match s {
        "+1/+1" => Counter::P1P1,
        "-1/-1" => Counter::M1M1,
        "stun" => Counter::Stun,
        "loyalty" => Counter::Loyalty,
        "lore" => Counter::Lore,
        _ => match KWS.iter().find(|(_, n)| *n == s) {
            Some((k, _)) => Counter::Kw(*k),
            None => Counter::Named(db.subtype_id(s).ok_or_else(|| format!("build: unknown counter kind {s:?}"))?),
        },
    })
}

fn kw_names(k: KwSet) -> Vec<&'static str> {
    KWS.iter().filter(|(x, _)| k.has(*x)).map(|(_, n)| *n).collect()
}

fn kws_of(v: &Value) -> Result<KwSet, String> {
    let mut k = KwSet::default();
    for n in v.as_array().ok_or("build: keywords must be a list of names")? {
        let n = n.as_str().ok_or("build: keyword names are strings")?;
        let (kw, _) = KWS.iter().find(|(_, x)| *x == n).ok_or_else(|| format!("build: unknown keyword {n:?}"))?;
        k.insert(*kw);
    }
    Ok(k)
}

fn type_names(t: u16) -> Result<Vec<&'static str>, String> {
    let known: u16 = TYPES.iter().map(|(b, _)| b).fold(0, |a, b| a | b);
    if t & !known != 0 {
        return Err(format!("unsupported: card type bits {t:#x}"));
    }
    Ok(TYPES.iter().filter(|(b, _)| t & b != 0).map(|(_, n)| *n).collect())
}

fn types_of(v: &Value) -> Result<u16, String> {
    let mut t = 0;
    for n in v.as_array().ok_or("build: types must be a list of names")? {
        let n = n.as_str().ok_or("build: type names are strings")?;
        t |= TYPES.iter().find(|(_, x)| *x == n).ok_or_else(|| format!("build: unknown card type {n:?}"))?.0;
    }
    Ok(t)
}

fn subtype_names(db: &CardDb, s: &[u16]) -> Vec<String> {
    s.iter().map(|&i| db.subtype_name(i).to_string()).collect()
}

fn subtypes_of(db: &CardDb, v: &Value) -> Result<Vec<u16>, String> {
    v.as_array()
        .ok_or("build: subtypes must be a list of names")?
        .iter()
        .map(|n| {
            let n = n.as_str().ok_or("build: subtype names are strings")?;
            db.subtype_id(n).ok_or_else(|| format!("build: unknown subtype {n:?}"))
        })
        .collect()
}

fn colors_str(m: u8) -> String {
    (0..5).filter(|i| m & (1 << i) != 0).map(|i| crate::mana::LETTERS[i]).collect()
}

fn colors_of(s: &str) -> Result<u8, String> {
    let mut m = 0;
    for ch in s.chars() {
        let i = crate::mana::LETTERS[..5].iter().position(|&l| l == ch).ok_or_else(|| format!("build: unknown colour {ch:?}"))?;
        m |= 1 << i;
    }
    Ok(m)
}

fn from_json(f: CastFrom) -> Value {
    match f {
        CastFrom::Hand => json!("hand"),
        CastFrom::Flashback => json!("flashback"),
        CastFrom::FlashbackMana => json!("flashback_mana"),
        CastFrom::Escape => json!("escape"),
        CastFrom::Permission { free, any_mana } => json!({"permission": {"free": free, "any_mana": any_mana}}),
        CastFrom::Retrace => json!("retrace"),
        CastFrom::Mayhem => json!("mayhem"),
        CastFrom::Taxed(n) => json!({"taxed": n}),
        CastFrom::Paragon => json!("paragon"),
        CastFrom::Miracle => json!("miracle"),
        CastFrom::Airbent(n) => json!({"airbent": n}),
        CastFrom::LibraryTop => json!("library_top"),
        CastFrom::LibraryTopMana => json!("library_top_mana"),
    }
}

fn from_of(v: &Value) -> Result<CastFrom, String> {
    let bad = || format!("build: unknown cast route {v}");
    Ok(match v {
        Value::String(s) => match s.as_str() {
            "hand" => CastFrom::Hand,
            "flashback" => CastFrom::Flashback,
            "flashback_mana" => CastFrom::FlashbackMana,
            "escape" => CastFrom::Escape,
            "retrace" => CastFrom::Retrace,
            "mayhem" => CastFrom::Mayhem,
            "paragon" => CastFrom::Paragon,
            "miracle" => CastFrom::Miracle,
            "library_top" => CastFrom::LibraryTop,
            "library_top_mana" => CastFrom::LibraryTopMana,
            _ => return Err(bad()),
        },
        Value::Object(o) => {
            if let Some(p) = o.get("permission") {
                let b = |k: &str| p.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
                CastFrom::Permission { free: b("free"), any_mana: b("any_mana") }
            } else if let Some(n) = o.get("taxed").and_then(|x| x.as_u64()) {
                CastFrom::Taxed(n as u8)
            } else if let Some(n) = o.get("airbent").and_then(|x| x.as_u64()) {
                CastFrom::Airbent(n as u8)
            } else {
                return Err(bad());
            }
        }
        _ => return Err(bad()),
    })
}

fn when_name(w: DelayedWhen) -> &'static str {
    match w {
        DelayedWhen::NextEndStep => "next_end_step",
        DelayedWhen::NextCleanup => "next_cleanup",
        DelayedWhen::NextUpkeep => "next_upkeep",
        DelayedWhen::YourNextMainPhase => "your_next_main_phase",
        DelayedWhen::YourNextTurnEndStep => "your_next_turn_end_step",
        DelayedWhen::EndOfCombat => "end_of_combat",
        DelayedWhen::YourNextUpkeep => "your_next_upkeep",
    }
}

fn when_of(s: &str) -> Result<DelayedWhen, String> {
    use DelayedWhen::*;
    [NextEndStep, NextCleanup, NextUpkeep, YourNextMainPhase, YourNextTurnEndStep, EndOfCombat, YourNextUpkeep].into_iter().find(|&w| when_name(w) == s).ok_or_else(|| format!("build: unknown delayed moment {s:?}"))
}

fn slot_json(s: AbilitySlot) -> (&'static str, u8) {
    match s {
        AbilitySlot::Spell => ("spell", 0),
        AbilitySlot::Trigger(i) => ("trigger", i),
        AbilitySlot::Activated(i) => ("activated", i),
        AbilitySlot::Hand(i) => ("hand", i),
        AbilitySlot::Graveyard(i) => ("graveyard", i),
    }
}

fn slot_of(kind: &str, i: u8) -> Result<AbilitySlot, String> {
    Ok(match kind {
        "spell" => AbilitySlot::Spell,
        "trigger" => AbilitySlot::Trigger(i),
        "activated" => AbilitySlot::Activated(i),
        "hand" => AbilitySlot::Hand(i),
        "graveyard" => AbilitySlot::Graveyard(i),
        _ => return Err(format!("build: unknown ability kind {kind:?} (spell, trigger, activated, hand, graveyard)")),
    })
}

/// A card of a deck by any of its names: (the card's definition, the face up). The card's own name
/// ("Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"), its front face's ("Tamiyo,
/// Inquisitive Student": the card, front face up) or another face's ("Tamiyo, Seasoned Scholar": that
/// face up).
pub fn lookup_card(db: &CardDb, name: &str) -> Result<(DefId, DefId), String> {
    if let Some(d) = db.id(name) {
        if db.def(d).token {
            return Err(format!("build: {name} is a token, not a card (\"token\": true)"));
        }
        return Ok((d, d));
    }
    if let Some(f) = db.token_by_name(name) {
        if let Some(front) = db.def(f).front {
            return Ok((front, f));
        }
    }
    let pre = format!("{name} // ");
    let m: Vec<DefId> = (0..db.num_cards() as DefId).filter(|&i| db.def(i).name.starts_with(&pre)).collect();
    if m.len() == 1 {
        return Ok((m[0], m[0]));
    }
    db.expect_id(name).map(|d| (d, d)).map_err(|e| format!("build: {e}"))
}

/// A token's or an emblem's definition by name.
fn lookup_token(db: &CardDb, name: &str, emblem: bool) -> Result<DefId, String> {
    let d = db.token_by_name(name).ok_or_else(|| format!("build: no token or emblem named {name:?}"))?;
    if db.def(d).front.is_some() || (!emblem && !db.def(d).token) {
        return Err(format!("build: {name:?} is not a token"));
    }
    Ok(d)
}

/// Any definition an ability can belong to (a card, a face, a token, an emblem), by name.
fn lookup_any(db: &CardDb, name: &str) -> Result<DefId, String> {
    if let Some(d) = db.id(name) {
        return Ok(d);
    }
    db.token_by_name(name).ok_or_else(|| format!("build: no card, face, token or emblem named {name:?}"))
}

// ---------------------------------------------------------------------------------------------
// describe

/// The description of `g`'s position from `viewer`'s view (module docs; ENGINE.md 95): everything the
/// viewer sees and knows, nothing it does not. An error names what the schema does not support.
pub fn describe(g: &Game, viewer: PlayerId) -> Result<Value, String> {
    if viewer > 1 {
        return Err(format!("describe: viewer {viewer}"));
    }
    let mut hands: [Vec<CardIdx>; 2] = [Vec::new(), Vec::new()];
    for p in 0..2u8 {
        let mut h: Vec<CardIdx> = g.players[p as usize].hand.iter().copied().filter(|&c| p == viewer || g.knows(viewer, c)).collect();
        h.sort_by_key(|&c| (g.ident(c), !g.objs[c.i()].known, c.i()));
        hands[p as usize] = h;
    }
    Describer { g, me: viewer, hands }.run()
}

struct Describer<'a> {
    g: &'a Game,
    me: PlayerId,
    /// The hand cards described (all of the viewer's, the opponent's the viewer knows), in order.
    hands: [Vec<CardIdx>; 2],
}

fn unsupported<T>(what: impl std::fmt::Display) -> Result<T, String> {
    Err(format!("unsupported: {what}"))
}

impl Describer<'_> {
    fn place(&self, c: CardIdx) -> Option<String> {
        let g = self.g;
        let o = &g.objs[c.i()];
        match o.zone {
            Zone::Battlefield => g.battlefield.iter().position(|&x| x == c).map(|i| format!("b{i}")),
            Zone::Graveyard => g.players[o.owner as usize].graveyard.iter().position(|&x| x == c).map(|i| format!("g{}.{i}", o.owner)),
            Zone::Exile => g.exile.iter().position(|&x| x == c).map(|i| format!("x{i}")),
            Zone::Stack => g.stack.iter().position(|s| s.kind == StackKind::Spell { card: c }).map(|i| format!("s{i}")),
            Zone::Command => g.command.iter().position(|&x| x == c).map(|i| format!("e{i}")),
            Zone::Hand => self.hands[o.owner as usize].iter().position(|&x| x == c).map(|i| format!("h{}.{i}", o.owner)),
            _ => None,
        }
    }

    /// A reference to an object (module docs).
    fn obj(&self, r: ObjRef) -> Result<Value, String> {
        let g = self.g;
        let c = r.idx;
        let o = &g.objs[c.i()];
        if g.live(r) {
            return match self.place(c) {
                Some(s) => Ok(json!(s)),
                None => unsupported(format!("a reference to a card in a {} the viewer does not see", zone_name(o.zone))),
            };
        }
        let mut v = if o.zone == Zone::Nowhere {
            if !g.is_token(c) || g.is_copy_obj(c) {
                return unsupported("a reference to a card outside the game");
            }
            json!({"gone": g.db.def(o.def).name, "owner": o.owner})
        } else {
            match self.place(c) {
                Some(s) => json!({"ref": s, "stale": true}),
                None => return unsupported(format!("a reference to an earlier object of a card now in a {} the viewer does not see", zone_name(o.zone))),
            }
        };
        // What the stale reference shows (`Game::ref_lki`, ENGINE.md 156; the encoding writes it): who knew the object
        // as it left (default both) and, where the viewer did, its identity then (default the card's identity now).
        match g.ref_lki.iter().rev().find(|x| x.0 == r) {
            None => v["known_by"] = json!([]),
            Some(&(_, d, k)) => {
                if k & 0b11 != 0b11 {
                    v["known_by"] = json!((0..2u8).filter(|q| k >> q & 1 != 0).collect::<Vec<u8>>());
                }
                if k >> self.me & 1 != 0 && d != g.ident(c) {
                    v["was"] = json!(g.db.def(d).name);
                }
            }
        }
        Ok(v)
    }

    fn item(&self, id: u32) -> Option<String> {
        self.g.stack.iter().position(|s| s.id == id).map(|i| format!("s{i}"))
    }

    fn target(&self, req: u8, t: Target) -> Result<Value, String> {
        Ok(match t {
            Target::Player(p) => json!({"req": req, "player": p}),
            Target::Obj(r) => json!({"req": req, "obj": self.obj(r)?}),
            Target::Stack(id) => match self.item(id) {
                Some(s) => json!({"req": req, "item": s}),
                None => return unsupported("a target that is a stack item no longer on the stack"),
            },
        })
    }

    fn ev(&self, ev: &EvCtx) -> Result<Option<Value>, String> {
        if *ev == EvCtx::default() {
            return Ok(None);
        }
        let mut m = Map::new();
        if let Some(p) = ev.player {
            m.insert("player".into(), json!(p));
        }
        if let Some(r) = ev.obj {
            m.insert("obj".into(), self.obj(r)?);
        }
        if let Some(r) = ev.obj2 {
            m.insert("obj2".into(), self.obj(r)?);
        }
        if ev.amount != 0 {
            m.insert("amount".into(), json!(ev.amount));
        }
        if ev.item != 0 {
            m.insert("item".into(), json!(self.item(ev.item).unwrap_or_else(|| "gone".into())));
        }
        Ok(Some(Value::Object(m)))
    }

    fn name(&self, c: CardIdx) -> &'static str {
        &self.g.db.def(self.g.ident(c)).name
    }

    fn decision(&self) -> Result<Value, String> {
        use Task as T;
        let g = self.g;
        let d = g.pending.as_ref().ok_or("describe: no decision is pending")?;
        let s = g.turn.starting;
        Ok(match (d.kind, &g.agenda[..]) {
            (DecisionKind::Priority, [T::PriorityAsk { page }]) if *page == crate::decision::Page::FIRST => json!("priority"),
            (DecisionKind::DeclareAttackers, [T::Priority, T::DeclareAttackers { next }]) => json!({"kind": "declare_attackers", "next": next}),
            // (what each new attacker attacks, the last first: forced attackers are declared together)
            (DecisionKind::AttackTarget, [T::Priority, T::DeclareAttackers { next }, rest @ ..])
                if !rest.is_empty() && rest.iter().all(|t| matches!(t, T::AttackTarget { .. })) =>
            {
                let att: Vec<Value> = rest.iter().map(|t| match t {
                    T::AttackTarget { card } => self.obj(g.obj_ref(*card)),
                    _ => unreachable!(),
                }).collect::<Result<_, _>>()?;
                json!({"kind": "attack_target", "next": next, "attackers": att})
            }
            (DecisionKind::DeclareBlockers, [T::Priority, T::DeclareBlockers { i }]) => json!({"kind": "declare_blockers", "next": i}),
            (DecisionKind::Discard, [T::CleanupRest, T::CleanupDiscard]) => json!({"kind": "cleanup_discard"}),
            (DecisionKind::PlayDraw, [T::PlayDraw { p }]) => json!({"kind": "play_draw", "player": p}),
            (DecisionKind::Mulligan, [T::BeginTurn, T::Opening { p: o, exile: 0 }, T::MulliganDeclare { p }]) if *o == s => {
                json!({"kind": "mulligan", "player": p})
            }
            (DecisionKind::MulliganBottom, [T::BeginTurn, T::Opening { p: o, exile: 0 }, T::MulliganDeclare { p: q }, rest @ ..])
                if *o == s && *q == s && !rest.is_empty() && rest.iter().all(|t| matches!(t, T::MulliganBottom { .. })) =>
            {
                let b: Vec<Value> = rest.iter().rev().map(|t| match t {
                    T::MulliganBottom { p, left } => json!([p, left]),
                    _ => unreachable!(),
                }).collect();
                json!({"kind": "mulligan_bottom", "bottoms": b})
            }
            (k, a) => {
                let top: Vec<String> = a.iter().rev().take(3).map(|t| {
                    let t = format!("{t:?}");
                    let end = t.find(|ch: char| !ch.is_alphanumeric()).unwrap_or(t.len());
                    t[..end].to_string()
                }).collect();
                return unsupported(format!("the decision {k:?} (agenda top {top:?}): only priority, combat declarations, the cleanup discard and the setup's decisions are built"));
            }
        })
    }

    fn run(&self) -> Result<Value, String> {
        let g = self.g;
        let me = self.me;
        if g.outcome.is_some() {
            return Err("describe: the game is over".into());
        }
        let decision = self.decision()?;
        // Every field of the game's state (public_state.rs's list): described below, derived, or
        // unsupported unless empty. An exhaustive destructuring: a new field is a compile error here.
        let Game {
            db, spec: _, rng: _, objs: _, players, battlefield, exile, face_down, face_down_ids, entering_face_down: _, moving_face_down: _,
            command, stack, turn, agenda: _, pending: _, answer, waiting, extra_turns, combat_dmg, next_ts: _, next_stack_id: _, outcome: _,
            clouds, tokens: _, first_token: _, batching: _, ev_seq: _, batch_fired: _, ts_group: _, entering_cast: _, entering_evoked: _,
            observer: _, keys, forced: _, forced_at: _, decisions: _, no_auto: _, effects, any_loss: _, counters: _, n_static: _,
            n_self_types: _, leaving: _, links, delayed, resolved, permissions, combat_damage_now: _, quiet_leave: _, monarch,
            lki_counters: _, entering_escaped: _, entering_x: _, entering_sunburst: _, entering_exiled: _, entering_mods: _,
            entering_counters: _, entering_tapped: _, discard_entered: _, repl_decided: _, route_choice: _, cast_lock, pool_wild: _, entering_dash: _,
            entering_warp: _, miracle_cast, entering_face: _, day, done_once, discarded, temp_triggers, gy_exile, copies,
            copy_objs, lki_copy: _, lki_damage: _, spell_lki: _, entering_copy: _, entering_times: _, enter_note: _, defer_enters: _,
            pile_moving: _, pile_shuffles: _, prepared, seen: _, initiative, dungeons, rules_srcs,
            choosing_for: _, marks, known_pools, log: _, log_mark: _, hand_looks: _, peeks,
            // (v6 engines: transient or derived - public_state.rs - or EoE-only, checked below)
            page, releasing, gy_arrivals, oracle: _, oracle_dead: _, leaving_to: _, leaving_choice: _, leaving_card: _, resolving_adventure: _,
            held, void_turn, next_turn_controller, turn_controller, lki_attached: _, lki_toughness: _, lki_power: _, loyalty: _,
            pool_restr, lands_entered: _, moon_game: _, entries, loop_taken: _, w3, w6: _,
            // (layout v7 engines, ENGINE.md 95.11: the hidden decklists' sightings and a search's finds - kept only
            // where whole-deck clouds are -, the last known identities of gone objects - described with each stale
            // reference, `obj` -, a loop action's activation in progress)
            sightings, sight_at: _, found, sightings_lost: _, ref_lki: _, loop_open,
            // (ENGINE.md 180: the forced find's records, a self-play setting's; a described position's game has none)
            ff: _,
        } = g;
        let empty = |n: usize, what: &str| if n == 0 { Ok(()) } else { unsupported(what) };
        empty(face_down.len() + face_down_ids.len(), "face-down cards (exiled face down, face-down permanents)")?;
        empty(waiting.len(), "triggered abilities waiting to be put on the stack")?;
        empty(peeks.len(), "a cloud entry naming a card its owner never saw (a look at the opponent's library, then a shuffle; ENGINE.md 192)")?;
        empty(extra_turns.len(), "extra turns")?;
        empty(combat_dmg.len(), "combat damage being assigned")?;
        empty(links.len(), "cards exiled with a permanent (links)")?;
        empty(permissions.len(), "permissions to play cards from other zones")?;
        empty(gy_exile.len(), "spells exiled instead of going to a graveyard")?;
        empty(copies.len() + copy_objs.len() + prepared.len(), "copies (copy effects, copies of spells or cards, prepared permanents)")?;
        empty(rules_srcs.len(), "the initiative's or monarch's rules sources")?;
        empty(answer.is_some() as usize + keys.len(), "an answer or chance keys between apply calls")?;
        empty(monarch.is_some() as usize + initiative.is_some() as usize + day.is_some() as usize, "the monarch, the initiative, day and night")?;
        empty(miracle_cast.is_some() as usize, "a miracle in progress")?;
        empty(page.is_some() as usize, "a paged decision (more than 95 permanents)")?;
        empty(releasing.len() + gy_arrivals.len() + held.len() + entries.len(), "a simultaneous move or entry in progress")?;
        empty(cast_lock.len(), "a spell's total cost locked by a mana ability during its cast (ENGINE.md 230)")?;
        empty(next_turn_controller.iter().flatten().count() + turn_controller.is_some() as usize, "a player controlling another's turn")?;
        empty(pool_restr.len(), "mana with a spending restriction")?;
        empty((0..2u8).map(|p| g.lands_entered_this_turn(p) as usize).sum::<usize>(), "the lands entered this turn (Bioengineered Future)")?;
        empty(w3.phased.len() + w3.indirect.len(), "phased-out permanents")?;
        empty(g.loyalty.iter().filter(|l| l.extra > 0).count(), "extra loyalty activations")?;
        empty(dungeons.iter().filter(|d| d.obj.is_some() || d.completed > 0).count(), "dungeons")?;
        empty(known_pools.iter().filter(|k| k.is_some()).count(), "hidden decks (best-of-three)")?;
        empty(sightings.len(), "the hidden decklists' sightings (ENGINE.md 140.9)")?;
        empty(found.len(), "a library search's finds the other player tells apart (hidden decklists, ENGINE.md 140.9)")?;
        empty(loop_open.is_some() as usize, "a loop action's activation in progress")?;

        let mut out = Map::new();
        out.insert("format".into(), json!(FORMAT));
        out.insert("viewer".into(), json!(me));
        // the turn
        let crate::game::Turn { number, active, step, priority, passes, starting, normal, cleanup_again, extra_combats, chooser, after_combat, had_attackers } = *turn;
        if extra_combats != 0 || after_combat != Step::Main2 {
            return unsupported("additional combat phases");
        }
        if normal != if number > 0 { active } else { 0 } {
            return unsupported("a turn after an extra turn (the last normal turn's player is not the active one)");
        }
        out.insert("turn".into(), json!(number));
        out.insert("active".into(), json!(active));
        out.insert("step".into(), json!(step_name(step)));
        out.insert("priority".into(), json!(priority));
        out.insert("passes".into(), json!(passes));
        out.insert("starting".into(), json!(starting));
        out.insert("chooser".into(), json!(chooser));
        if cleanup_again != (step == Step::Cleanup && decision == "priority") {
            out.insert("cleanup_again".into(), json!(cleanup_again));
        }
        out.insert("decision".into(), decision);
        // CR 508.8 (ENGINE.md 151.4): a creature was declared as an attacker or put onto the battlefield attacking in
        // this declare attackers step; derived from the attacking creatures, given only when one left combat since
        if had_attackers != had_attackers_derived(step, battlefield.iter().any(|&c| g.objs[c.i()].attacking)) {
            out.insert("had_attackers".into(), json!(had_attackers));
        }
        // void (v6: encoded): a nonland permanent left the battlefield or a spell was warped this turn
        if *void_turn != 0 && *void_turn == number {
            out.insert("void".into(), json!(true));
        }
        // the players
        let mut ps = Vec::new();
        for p in 0..2u8 {
            ps.push(self.player(p, &players[p as usize])?);
        }
        out.insert("players".into(), Value::Array(ps));
        // the battlefield, in timestamp order (with effects, each permanent's and effect's timestamp: their
        // order is what the layers read, CR 613.7)
        let ts = !effects.is_empty();
        let mut bf = Vec::new();
        for &c in battlefield {
            let mut p = self.permanent(c)?;
            if ts {
                p["ts"] = json!(g.objs[c.i()].timestamp);
            }
            bf.push(p);
        }
        out.insert("battlefield".into(), Value::Array(bf));
        let mut ex = Vec::new();
        for &c in exile {
            let mut e = Map::new();
            e.insert("name".into(), json!(self.name(c)));
            e.insert("owner".into(), json!(g.objs[c.i()].owner));
            if g.is_token(c) {
                return unsupported("a token in exile");
            }
            let cs: Map<String, Value> = g.counters_of(c).into_iter().map(|(k, n)| (counter_name(g, k), json!(n))).collect();
            if !cs.is_empty() {
                e.insert("counters".into(), Value::Object(cs));
            }
            // (the turn it was exiled: every move sets it)
            if g.objs[c.i()].control_since != 0 {
                e.insert("since".into(), json!(g.objs[c.i()].control_since));
            }
            ex.push(Value::Object(e));
        }
        out.insert("exile".into(), Value::Array(ex));
        // the stack, bottom first
        let mut st = Vec::new();
        for s in stack {
            st.push(self.stack_item(s)?);
        }
        out.insert("stack".into(), Value::Array(st));
        // the command zone: emblems
        if !command.is_empty() {
            let mut em = Vec::new();
            for &c in command {
                let o = &g.objs[c.i()];
                if o.owner != o.controller {
                    return unsupported("an emblem whose owner is not its controller");
                }
                em.push(json!({"name": db.def(o.def).name, "controller": o.controller}));
            }
            out.insert("emblems".into(), Value::Array(em));
        }
        if !effects.is_empty() {
            let mut ef = Vec::new();
            for e in effects {
                let mut x = self.effect(e)?;
                x["ts"] = json!(e.ts);
                ef.push(x);
            }
            out.insert("effects".into(), Value::Array(ef));
        }
        if !delayed.is_empty() {
            let mut dl = Vec::new();
            for d in delayed {
                let mut m = Map::new();
                m.insert("when".into(), json!(when_name(d.when)));
                m.insert("controller".into(), json!(d.controller));
                m.insert("source".into(), self.obj(d.source)?);
                m.insert("card".into(), json!(db.def(d.source_def).name));
                m.insert("index".into(), json!(d.slot));
                if d.turn != g.turn.number {
                    m.insert("turn".into(), json!(d.turn));
                }
                if let Some(e) = self.ev(&d.ev)? {
                    m.insert("ev".into(), e);
                }
                dl.push(Value::Object(m));
            }
            out.insert("delayed".into(), Value::Array(dl));
        }
        if !temp_triggers.is_empty() {
            let mut tt = Vec::new();
            for t in temp_triggers {
                let mut m = Map::new();
                m.insert("controller".into(), json!(t.controller));
                m.insert("source".into(), self.obj(t.source)?);
                m.insert("card".into(), json!(db.def(t.source_def).name));
                m.insert("index".into(), json!(t.slot));
                if t.this_turn {
                    m.insert("this_turn".into(), json!(true));
                }
                if let Some(r) = t.obj {
                    m.insert("obj".into(), self.obj(r)?);
                }
                tt.push(Value::Object(m));
            }
            out.insert("temp_triggers".into(), Value::Array(tt));
        }
        if !marks.is_empty() {
            let mut mk = Vec::new();
            for &(r, m) in marks {
                match m {
                    Mark::ExiledToPay(x) => mk.push(json!({"on": self.obj(r)?, "exiled_to_pay": self.obj(x)?})),
                    other => return unsupported(format!("the mark {other:?}")),
                }
            }
            out.insert("marks".into(), Value::Array(mk));
        }
        if !discarded.is_empty() {
            out.insert("discarded".into(), Value::Array(discarded.iter().map(|&r| self.obj(r)).collect::<Result<_, _>>()?));
        }
        if !resolved.is_empty() {
            let v: Result<Vec<Value>, String> = resolved.iter().map(|&(r, n)| Ok(json!({"on": self.obj(r)?, "count": n}))).collect();
            out.insert("resolved".into(), Value::Array(v?));
        }
        if !done_once.is_empty() {
            let v: Result<Vec<Value>, String> = done_once.iter().map(|&(r, k)| Ok(json!({"on": self.obj(r)?, "trigger": k}))).collect();
            out.insert("done_once".into(), Value::Array(v?));
        }
        // the viewer's partial knowledge of the opponent's hidden cards (ENGINE.md 10)
        let mut cl = Vec::new();
        for k in clouds.iter().filter(|k| k.viewer == me) {
            let lib = &players[k.owner as usize].library;
            let mut pos = Vec::new();
            for c in &k.slots {
                match lib.iter().position(|x| x == c) {
                    Some(i) => pos.push(lib.len() - 1 - i),
                    None => return unsupported("a cloud slot outside a library"),
                }
            }
            pos.sort_unstable();
            let cards: Vec<&str> = k.defs.iter().map(|&d| db.def(d).name.as_str()).collect();
            cl.push(json!({"owner": k.owner, "hand": k.hand, "library": k.library, "positions": pos, "cards": cards}));
        }
        if !cl.is_empty() {
            out.insert("clouds".into(), Value::Array(cl));
        }
        Ok(Value::Object(out))
    }

    fn player(&self, p: PlayerId, pl: &crate::game::Player) -> Result<Value, String> {
        let g = self.g;
        let me = self.me;
        let crate::game::Player {
            life, library, hand, graveyard, pool, lands_played, drew_from_empty, lost, mulligans, kept, mulligan_declared, last_turn, turns,
            left_battlefield, drawn, spells_cast, noncreature_cast, gained_life, cant_gain_life, attacked_with, draw_step_draws, spells_game,
            energy, lost_life, flash_sorceries, poison, experience, gy_play_turn, gy_exile_turn, city_blessing, prot_until, sideboard,
            companion, companion_def, companion_used, artifact_entered, life_gained, uncounterable_next,
        } = pl;
        if *drew_from_empty || *lost {
            return unsupported("a player who drew from an empty library or lost (state-based actions pending)");
        }
        if *cant_gain_life || *flash_sorceries || *poison > 0 || *experience > 0 || *gy_play_turn > 0 || *gy_exile_turn > 0 || *city_blessing || *prot_until > 0 {
            return unsupported("a rare player state (can't gain life, sorceries with flash, poison, experience, graveyard play or exile, the city's blessing, protection)");
        }
        if !sideboard.is_empty() || companion.is_some() || companion_def.is_some() || *companion_used {
            return unsupported("sideboards and companions");
        }
        if *uncounterable_next > 0 {
            return unsupported("\"the next spell you cast can't be countered\" effects");
        }
        let (dt_last, dt_turns) = derived_turns(&g.turn, p);
        if *last_turn != dt_last {
            return unsupported(format!("P{p}'s last turn {last_turn} (expected {dt_last})"));
        }
        let mut m = Map::new();
        m.insert("life".into(), json!(life));
        // the hand
        let mut h = Vec::new();
        for &c in &self.hands[p as usize] {
            let o = &g.objs[c.i()];
            if o.known & (1 << p) == 0 {
                return unsupported("a hand card its owner does not know");
            }
            if p == me && o.known & (1 << opp(me)) != 0 {
                h.push(json!({"name": self.name(c), "known_to_opponent": true}));
            } else {
                h.push(json!(self.name(c)));
            }
        }
        m.insert("hand".into(), Value::Array(h));
        if p != me || hand.len() != self.hands[p as usize].len() {
            m.insert("hand_size".into(), json!(hand.len()));
        }
        // the library: its size and the positions somebody knows
        m.insert("library_size".into(), json!(library.len()));
        let mut lk = Vec::new();
        for (i, &c) in library.iter().rev().enumerate() {
            let k = g.objs[c.i()].known & 0b11;
            if k == 0 {
                continue;
            }
            let by: Vec<u8> = (0..2u8).filter(|q| k & (1 << q) != 0).collect();
            let mut e = Map::new();
            e.insert("pos".into(), json!(i));
            if g.knows(me, c) {
                e.insert("name".into(), json!(self.name(c)));
            }
            e.insert("known_by".into(), json!(by));
            lk.push(Value::Object(e));
        }
        if !lk.is_empty() {
            m.insert("library_known".into(), Value::Array(lk));
        }
        let mut gy = Vec::new();
        for &c in graveyard {
            if g.is_token(c) {
                return unsupported("a token in a graveyard (state-based actions pending)");
            }
            gy.push(json!(self.name(c)));
        }
        m.insert("graveyard".into(), Value::Array(gy));
        if pool.art > 0 || pool.haste > 0 {
            return unsupported("restricted mana in a pool (artifact-only, haste)");
        }
        let s: String = (0..6).flat_map(|t| std::iter::repeat_n(crate::mana::LETTERS[t], pool.amt[t] as usize)).collect();
        if !s.is_empty() {
            m.insert("pool".into(), json!(s));
        }
        let mut num = |k: &str, n: u64| {
            if n != 0 {
                m.insert(k.into(), json!(n));
            }
        };
        num("lands_played", *lands_played as u64);
        num("drawn", *drawn as u64);
        num("draw_step_draws", *draw_step_draws as u64);
        num("spells_cast", *spells_cast as u64);
        num("noncreature_cast", *noncreature_cast as u64);
        num("spells_game", *spells_game as u64);
        num("energy", *energy as u64);
        num("mulligans", *mulligans as u64);
        if *life_gained != 0 {
            m.insert("life_gained".into(), json!(life_gained));
        }
        for (k, b) in [("gained_life", *gained_life), ("lost_life", *lost_life), ("left_battlefield", *left_battlefield), ("artifact_entered", *artifact_entered)] {
            if b {
                m.insert(k.into(), json!(true));
            }
        }
        if g.turn.step == Step::Setup {
            m.insert("kept".into(), json!(kept));
            m.insert("mulligan_declared".into(), json!(mulligan_declared));
        } else if !*kept || *mulligan_declared {
            return unsupported("a mulligan state after the setup");
        }
        if *turns != dt_turns {
            m.insert("turns".into(), json!(turns));
        }
        if !attacked_with.is_empty() {
            m.insert("attacked_with".into(), Value::Array(attacked_with.iter().map(|&r| self.obj(r)).collect::<Result<_, _>>()?));
        }
        Ok(Value::Object(m))
    }

    fn permanent(&self, c: CardIdx) -> Result<Value, String> {
        let g = self.g;
        let o = &g.objs[c.i()];
        let crate::game::Obj {
            def, owner, controller, zone: _, inc: _, timestamp: _, tapped, damage, deathtouched, control_since, attacking, blocked, blocking,
            struck_first, known: _, attached, evoked, flashback, escaped, chosen_player, monstrous, attacks, entered, flips,
        } = o;
        // (loyalty activations: out of Obj on v6 engines, Game::loyalty_of; extra ones are refused in run)
        let crate::game::Loyalty { turn: pw_turn, used: pw_used, .. } = g.loyalty_of(c);
        if *def == g.db.face_down_def() {
            return unsupported("a face-down permanent");
        }
        if g.is_copy_obj(c) {
            return unsupported("a copy");
        }
        if *evoked || *flashback || *escaped || chosen_player.is_some() || *monstrous || *deathtouched {
            return unsupported(format!("a rare permanent state of {} (evoked, flashback, escaped, a chosen player, monstrous, deathtouched, extra loyalty activations)", self.name(c)));
        }
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name(c)));
        if g.is_token(c) {
            m.insert("token".into(), json!(true));
        }
        m.insert("owner".into(), json!(owner));
        if controller != owner {
            m.insert("controller".into(), json!(controller));
        }
        if *tapped {
            m.insert("tapped".into(), json!(true));
        }
        if *entered != 0 {
            m.insert("entered".into(), json!(entered));
        }
        if *control_since != 0 {
            m.insert("control_since".into(), json!(control_since));
        }
        let cs: Map<String, Value> = g.counters_of(c).into_iter().map(|(k, n)| (counter_name(g, k), json!(n))).collect();
        if !cs.is_empty() {
            m.insert("counters".into(), Value::Object(cs));
        }
        if *damage > 0 {
            m.insert("damage".into(), json!(damage));
        }
        if let Some(h) = attached {
            m.insert("attached_to".into(), self.obj(*h)?);
        }
        if *attacking {
            m.insert("attacking".into(), json!(true));
            if let Some(pw) = attacks {
                m.insert("attacks".into(), self.obj(*pw)?);
            }
        } else if attacks.is_some() {
            return unsupported("a planeswalker attacked by a creature not attacking");
        }
        if *blocked {
            m.insert("blocked".into(), json!(true));
        }
        if let Some(a) = blocking {
            m.insert("blocking".into(), self.obj(g.obj_ref(*a))?);
        }
        if *struck_first {
            m.insert("struck_first".into(), json!(true));
        }
        if pw_turn != 0 || pw_used != 0 {
            m.insert("loyalty_turn".into(), json!(pw_turn));
            m.insert("loyalty_used".into(), json!(pw_used));
        }
        if *flips != 0 {
            m.insert("flips".into(), json!(flips));
        }
        Ok(Value::Object(m))
    }

    fn stack_item(&self, s: &StackItem) -> Result<Value, String> {
        let g = self.g;
        let StackItem {
            id: _, kind, controller, targets, cost_option, modes, ev, alt, from, delved, delve_floor: _, discarded_land, x, division,
            attack_as, noted, flips, origin, times, spliced,
        } = s;
        let mut m = Map::new();
        m.insert("controller".into(), json!(controller));
        match *kind {
            StackKind::Spell { card } => {
                let o = &g.objs[card.i()];
                if g.is_copy_obj(card) {
                    return unsupported("a copy of a spell");
                }
                if o.flashback || o.evoked || o.escaped {
                    return unsupported("a spell cast with flashback (or evoked, escaped)");
                }
                m.insert("spell".into(), json!(self.name(card)));
                if o.owner != *controller {
                    m.insert("owner".into(), json!(o.owner));
                }
                if *origin != Zone::Hand {
                    m.insert("origin".into(), json!(zone_name(*origin)));
                }
                if *flips != 0 {
                    return unsupported("a spell's transformation count");
                }
            }
            StackKind::Ability { ability, source } => {
                let (k, i) = slot_json(ability.slot);
                let mut a = json!({"card": g.db.def(ability.def).name, "kind": k, "index": i});
                if let Some(md) = ability.mode {
                    a["mode"] = json!(md);
                }
                m.insert("ability".into(), a);
                m.insert("source".into(), self.obj(source)?);
                if *origin != Zone::Nowhere {
                    return unsupported("an ability with a cast zone");
                }
                let want = if g.live(source) { g.objs[source.idx.i()].flips } else { 0 };
                if *flips != want {
                    return unsupported("an ability whose source transformed since it was put on the stack");
                }
            }
        }
        if !targets.is_empty() {
            m.insert("targets".into(), Value::Array(targets.iter().map(|&(r, t)| self.target(r, t)).collect::<Result<_, _>>()?));
        }
        if *cost_option != 0 {
            m.insert("cost_option".into(), json!(cost_option));
        }
        if !modes.is_empty() {
            m.insert("modes".into(), json!(modes));
        }
        if let Some(e) = self.ev(ev)? {
            m.insert("ev".into(), e);
        }
        if *alt != 0 {
            m.insert("alt".into(), json!(alt));
        }
        if *from != CastFrom::Hand {
            m.insert("from".into(), from_json(*from));
        }
        if *delved != 0 {
            m.insert("delved".into(), json!(delved));
        }
        if *discarded_land {
            return unsupported("Mox Diamond's discarded land");
        }
        if *x != 0 {
            m.insert("x".into(), json!(x));
        }
        if !division.is_empty() {
            m.insert("division".into(), json!(division));
        }
        match attack_as {
            None => {}
            Some(None) => {
                m.insert("attack_as".into(), json!("player"));
            }
            Some(Some(r)) => {
                m.insert("attack_as".into(), self.obj(*r)?);
            }
        }
        if !noted.is_empty() {
            m.insert("noted".into(), Value::Array(noted.iter().map(|&r| self.obj(r)).collect::<Result<_, _>>()?));
        }
        if *times != 0 {
            m.insert("times".into(), json!(times));
        }
        if !spliced.is_empty() {
            m.insert("spliced".into(), json!(spliced.iter().map(|&d| g.db.def(d).name.as_str()).collect::<Vec<_>>()));
        }
        Ok(Value::Object(m))
    }

    fn effect(&self, e: &Effect) -> Result<Value, String> {
        let db = self.g.db;
        let mut m = Map::new();
        match &e.on {
            Affected::Obj(r) => {
                m.insert("on".into(), self.obj(*r)?);
            }
            Affected::Group(_) => return unsupported("an effect on a group of permanents"),
        }
        let until = match e.until {
            Until::EndOfTurn => json!("end_of_turn"),
            Until::Forever => json!("forever"),
            Until::YourNextTurn => json!("your_next_turn"),
            Until::WhileOnBattlefield(r) => json!({"while_on_battlefield": self.obj(r)?}),
            Until::WhileSource => return unsupported("an unlowered duration"),
        };
        m.insert("until".into(), until);
        m.insert("controller".into(), json!(e.controller));
        match &e.m {
            Mod::Pt(p, t) => m.insert("pt".into(), json!([p, t])),
            Mod::SetPt(p, t) => m.insert("set_pt".into(), json!([p, t])),
            Mod::Keywords(k) => m.insert("keywords".into(), json!(kw_names(*k))),
            Mod::Types { add, subtypes, replace } => {
                m.insert("types".into(), json!({"add": type_names(*add)?, "subtypes": subtype_names(db, subtypes), "replace": replace}))
            }
            Mod::BecomeTypes { types, subtypes } => {
                m.insert("become_types".into(), json!({"types": type_names(*types)?, "subtypes": subtype_names(db, subtypes)}))
            }
            Mod::Colors(c) => m.insert("colors".into(), json!(colors_str(*c))),
            Mod::CantBlock => m.insert("cant_block".into(), json!(true)),
            Mod::CantBeBlocked => m.insert("cant_be_blocked".into(), json!(true)),
            Mod::MustAttack => m.insert("must_attack".into(), json!(true)),
            Mod::ExileIfDies => m.insert("exile_if_dies".into(), json!(true)),
            Mod::ExileIfLeaves => m.insert("exile_if_leaves".into(), json!(true)),
            Mod::MustAttackPlayer => m.insert("must_attack_player".into(), json!(true)),
            Mod::Goad => m.insert("goad".into(), json!(true)),
            Mod::LoseAllAbilities => m.insert("lose_all_abilities".into(), json!(true)),
            other => return unsupported(format!("the effect {other:?}")),
        };
        Ok(Value::Object(m))
    }
}

/// `Turn::had_attackers` as the attacking creatures show it (CR 508.8, ENGINE.md 151.4): in the declare attackers
/// step, a creature is attacking. (True with none attacking - one left combat since - is described.)
fn had_attackers_derived(step: Step, attacking: bool) -> bool {
    step == Step::DeclareAttackers && attacking
}

/// A player's last turn and turns begun, as a game without extra turns has them (`Scenario` too).
fn derived_turns(t: &crate::game::Turn, p: PlayerId) -> (u32, u32) {
    if t.number == 0 {
        return (0, 0);
    }
    let last = if p == t.active { t.number } else { t.number - 1 };
    let turns = if p == t.starting { t.number.div_ceil(2) } else { t.number / 2 };
    (last, turns)
}

impl MtgAz {
    /// The description of the current position from `viewer`'s view (`build::describe`).
    pub fn describe(&self, viewer: PlayerId) -> Result<Value, String> {
        if self.m.sb.is_some() {
            return unsupported("sideboarding between games");
        }
        describe(&self.g, viewer)
    }
}

// ---------------------------------------------------------------------------------------------
// build

/// The keys of an object, checked (a misspelt key is an error, never ignored).
fn keys(o: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("build: unknown key {k:?} in {what} (allowed: {})", allowed.join(", "))),
        None => Ok(()),
    }
}

fn obj<'v>(v: &'v Value, what: &str) -> Result<&'v Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("build: {what} must be an object"))
}

fn arr<'v>(o: &'v Map<String, Value>, k: &str) -> Result<&'v [Value], String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(format!("build: {k} must be a list")),
    }
}

fn uint(o: &Map<String, Value>, k: &str, default: u64) -> Result<u64, String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(x) => x.as_u64().ok_or_else(|| format!("build: {k} must be a non-negative integer")),
    }
}

fn int(o: &Map<String, Value>, k: &str, default: i64) -> Result<i64, String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(x) => x.as_i64().ok_or_else(|| format!("build: {k} must be an integer")),
    }
}

fn boolean(o: &Map<String, Value>, k: &str) -> Result<bool, String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(x) => x.as_bool().ok_or_else(|| format!("build: {k} must be a boolean")),
    }
}

fn player_of(o: &Map<String, Value>, k: &str) -> Result<Option<PlayerId>, String> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => match x.as_u64() {
            Some(p) if p < 2 => Ok(Some(p as PlayerId)),
            _ => Err(format!("build: {k} must be a player (0 or 1)")),
        },
    }
}

fn need_player(o: &Map<String, Value>, k: &str, what: &str) -> Result<PlayerId, String> {
    player_of(o, k)?.ok_or_else(|| format!("build: {what}.{k} (a player, 0 or 1) is required"))
}

/// A card entry: a name, or an object with `name` (and `ref`, ...).
fn entry_name(v: &Value) -> Result<&str, String> {
    match v {
        Value::String(s) => Ok(s),
        Value::Object(o) => o.get("name").and_then(|n| n.as_str()).ok_or_else(|| format!("build: a card entry without a name: {v}")),
        _ => Err(format!("build: a card entry must be a name or an object: {v}")),
    }
}

fn entry_obj(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

/// A planned hidden-zone slot of a player's hand or library: its identity (if the description names
/// it) and who knows it.
#[derive(Clone, Copy)]
struct Slot {
    def: Option<DefId>,
    card: Option<DefId>,
    known: u8,
}

struct Builder<'a> {
    db: &'static CardDb,
    g: Game,
    me: PlayerId,
    d: &'a Map<String, Value>,
    /// Per player, the deck's cards not placed yet (a sorted multiset of card definitions).
    remaining: [Vec<DefId>; 2],
    /// Object references: places and aliases.
    refs: BTreeMap<String, CardIdx>,
    /// Stack item references (index from the bottom).
    items: BTreeMap<String, usize>,
    /// The stale objects described, each with its `ref_lki` entry (None: known to nobody as it left).
    lki: Vec<(ObjRef, Option<(ObjRef, DefId, u8)>)>,
}

impl Builder<'_> {
    fn take(&mut self, owner: PlayerId, card: DefId, name: &str) -> Result<(), String> {
        let r = &mut self.remaining[owner as usize];
        match r.binary_search(&card) {
            Ok(i) => {
                r.remove(i);
                Ok(())
            }
            Err(_) => Err(format!("build: P{owner}'s deck has no {name} left to place (the description names more of it than the decklist has)")),
        }
    }

    fn alias(&mut self, v: &Value, c: CardIdx) -> Result<(), String> {
        if let Some(r) = entry_obj(v).and_then(|o| o.get("ref")) {
            let r = r.as_str().ok_or("build: ref must be a string")?;
            if self.refs.insert(r.to_string(), c).is_some() {
                return Err(format!("build: the reference {r:?} names two objects"));
            }
        }
        Ok(())
    }

    fn place_ref(&mut self, place: String, c: CardIdx) {
        self.refs.insert(place, c);
    }

    /// A deck card into a public zone (known to both; every slot's incarnation is 1, so 0 names an
    /// earlier object of it: a stale reference).
    fn card(&mut self, owner: PlayerId, v: &Value, zone: Zone) -> Result<CardIdx, String> {
        let name = entry_name(v)?;
        let (card, face) = lookup_card(self.db, name)?;
        if face != card && !matches!(zone, Zone::Battlefield | Zone::Stack) {
            return Err(format!("build: {name} (a face other than the front) in the {}: only a permanent or a spell has one up", zone_name(zone)));
        }
        self.take(owner, card, name)?;
        let c = self.g.add_card(card, owner, zone);
        self.g.objs[c.i()].def = face;
        self.g.objs[c.i()].inc = 1;
        self.g.objs[c.i()].known = 0b11;
        Ok(c)
    }

    fn token(&mut self, owner: PlayerId, name: &str, zone: Zone) -> Result<CardIdx, String> {
        let def = lookup_token(self.db, name, false)?;
        let c = self.g.add_card(def, owner, zone);
        self.g.token_slot_made(c);
        self.g.objs[c.i()].inc = 1;
        self.g.objs[c.i()].known = 0b11;
        Ok(c)
    }

    /// An object reference (module docs) -> the object (stale: an earlier incarnation of the slot).
    fn obj(&mut self, v: &Value) -> Result<ObjRef, String> {
        match v {
            Value::String(s) => {
                let c = *self.refs.get(s).ok_or_else(|| format!("build: no object {s:?}"))?;
                Ok(ObjRef { idx: c, inc: self.g.objs[c.i()].inc })
            }
            Value::Object(o) if o.contains_key("gone") => {
                keys(o, &["gone", "owner", "known_by", "was"], "a gone token")?;
                let name = o["gone"].as_str().ok_or("build: gone must be a token name")?;
                let owner = need_player(o, "owner", "a gone token")?;
                let c = self.token(owner, name, Zone::Nowhere)?;
                let r = ObjRef { idx: c, inc: 0 };
                self.stale_lki(r, o)?;
                Ok(r)
            }
            Value::Object(o) => {
                keys(o, &["ref", "stale", "known_by", "was"], "a reference")?;
                let s = o.get("ref").and_then(|x| x.as_str()).ok_or_else(|| format!("build: a reference without ref: {v}"))?;
                let c = *self.refs.get(s).ok_or_else(|| format!("build: no object {s:?}"))?;
                if !boolean(o, "stale")? {
                    if o.contains_key("known_by") || o.contains_key("was") {
                        return Err(format!("build: known_by and was are a stale reference's: {v}"));
                    }
                    return Ok(ObjRef { idx: c, inc: self.g.objs[c.i()].inc });
                }
                let r = ObjRef { idx: c, inc: 0 };
                self.stale_lki(r, o)?;
                Ok(r)
            }
            _ => Err(format!("build: a reference must be a name of a place, or an object: {v}")),
        }
    }

    /// A stale reference's last known identity (`Game::ref_lki`, ENGINE.md 156: what the encoding shows of it):
    /// `known_by` the players who knew the object as it left (default both; none: nothing is kept), `was` its
    /// identity then, where the viewer knew it (default the card's identity now; one the viewer did not know is the
    /// card's, as `determinize` makes it). Every reference to one stale object must say the same.
    fn stale_lki(&mut self, r: ObjRef, o: &Map<String, Value>) -> Result<(), String> {
        let known = match o.get("known_by") {
            None | Some(Value::Null) => 0b11,
            Some(x) => {
                let a = x.as_array().ok_or("build: known_by must be a list of players")?;
                let mut k = 0u8;
                for q in a {
                    let q = q.as_u64().filter(|&q| q < 2).ok_or("build: known_by must be a list of players (0, 1)")?;
                    k |= 1 << q;
                }
                k
            }
        };
        let def = match o.get("was") {
            None | Some(Value::Null) => self.g.ident(r.idx),
            Some(x) => {
                if known >> self.me & 1 == 0 {
                    return Err("build: was names the viewer's last known identity of a stale object, and known_by says it did not know it".into());
                }
                lookup_any(self.db, x.as_str().ok_or("build: was must be a card name")?)?
            }
        };
        let entry = (known != 0).then_some((r, def, known));
        match self.lki.iter().find(|x| x.0 == r) {
            Some(x) if x.1 == entry => Ok(()),
            Some(_) => Err("build: two references to one stale object say different things of it (known_by, was)".into()),
            None => {
                self.lki.push((r, entry));
                if let Some(e) = entry {
                    self.g.ref_lki.push(e);
                }
                Ok(())
            }
        }
    }

    fn opt_obj(&mut self, o: &Map<String, Value>, k: &str) -> Result<Option<ObjRef>, String> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => self.obj(v).map(Some),
        }
    }

    fn item_id(&self, s: &str) -> Result<u32, String> {
        self.items.get(s).map(|&i| i as u32 + 1).ok_or_else(|| format!("build: no stack item {s:?}"))
    }

    fn ev(&mut self, v: Option<&Value>) -> Result<EvCtx, String> {
        let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(EvCtx::default()) };
        let o = obj(v, "ev")?;
        keys(o, &["player", "obj", "obj2", "amount", "item"], "ev")?;
        let item = match o.get("item").and_then(|x| x.as_str()) {
            None => 0,
            // (an item that left the stack: an id no item has)
            Some("gone") => u32::MAX - 1,
            Some(s) => self.item_id(s)?,
        };
        Ok(EvCtx { player: player_of(o, "player")?, obj: self.opt_obj(o, "obj")?, obj2: self.opt_obj(o, "obj2")?, amount: uint(o, "amount", 0)? as u64, item })
    }

    fn ability(&self, v: &Value) -> Result<AbilityRef, String> {
        let o = obj(v, "ability")?;
        keys(o, &["card", "kind", "index", "mode"], "ability")?;
        let name = o.get("card").and_then(|x| x.as_str()).ok_or("build: ability.card is required")?;
        let def = lookup_any(self.db, name)?;
        let kind = o.get("kind").and_then(|x| x.as_str()).ok_or("build: ability.kind is required")?;
        let slot = slot_of(kind, uint(o, "index", 0)? as u8)?;
        let mode = o.get("mode").and_then(|x| x.as_u64()).map(|m| m as u8);
        let d = self.db.def(def);
        let ok = match slot {
            AbilitySlot::Spell => d.spell.is_some(),
            AbilitySlot::Trigger(i) => (i as usize) < d.triggers.len(),
            AbilitySlot::Activated(i) => (i as usize) < d.activated.len(),
            AbilitySlot::Hand(i) => (i as usize) < d.hand_abilities.len(),
            AbilitySlot::Graveyard(_) => true,
        };
        if !ok {
            return Err(format!("build: {name} has no {kind} ability {}", uint(o, "index", 0)?));
        }
        Ok(AbilityRef { def, slot, mode })
    }

    fn target(&mut self, v: &Value) -> Result<(u8, Target), String> {
        let o = obj(v, "a target")?;
        keys(o, &["req", "player", "obj", "item"], "a target")?;
        let req = uint(o, "req", 0)? as u8;
        let t = if let Some(p) = player_of(o, "player")? {
            Target::Player(p)
        } else if let Some(x) = o.get("obj") {
            Target::Obj(self.obj(x)?)
        } else if let Some(s) = o.get("item").and_then(|x| x.as_str()) {
            Target::Stack(self.item_id(s)?)
        } else {
            return Err(format!("build: a target must name a player, an obj or an item: {v}"));
        };
        Ok((req, t))
    }

    fn effect(&mut self, v: &Value) -> Result<(Effect, Option<u64>), String> {
        let o = obj(v, "an effect")?;
        const MODS: [&str; 15] = [
            "pt", "set_pt", "keywords", "types", "become_types", "colors", "cant_block", "cant_be_blocked", "must_attack", "exile_if_dies",
            "exile_if_leaves", "must_attack_player", "goad", "lose_all_abilities", "on",
        ];
        let mut allowed: Vec<&str> = MODS.to_vec();
        allowed.extend(["until", "controller", "ts"]);
        keys(o, &allowed, "an effect")?;
        let on = Affected::Obj(self.obj(o.get("on").ok_or("build: effect.on is required")?)?);
        let until = match o.get("until") {
            None => Until::EndOfTurn,
            Some(Value::String(s)) if s == "end_of_turn" => Until::EndOfTurn,
            Some(Value::String(s)) if s == "forever" => Until::Forever,
            Some(Value::String(s)) if s == "your_next_turn" => Until::YourNextTurn,
            Some(Value::Object(u)) if u.contains_key("while_on_battlefield") => Until::WhileOnBattlefield(self.obj(&u["while_on_battlefield"])?),
            Some(x) => return Err(format!("build: unknown effect duration {x}")),
        };
        let controller = need_player(o, "controller", "effect")?;
        let pair = |k: &str| -> Result<(i64, i64), String> {
            let a = o[k].as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("build: effect.{k} must be [power, toughness]"))?;
            Ok((a[0].as_i64().ok_or("build: P/T are integers")?, a[1].as_i64().ok_or("build: P/T are integers")?))
        };
        let present: Vec<&str> = MODS[..14].iter().copied().filter(|k| o.contains_key(*k)).collect();
        if present.len() != 1 {
            return Err(format!("build: an effect needs exactly one modification ({}), not {present:?}", MODS[..14].join(", ")));
        }
        let db = self.db;
        let m = match present[0] {
            "pt" => {
                let (p, t) = pair("pt")?;
                Mod::Pt(p, t)
            }
            "set_pt" => {
                let (p, t) = pair("set_pt")?;
                Mod::SetPt(p, t)
            }
            "keywords" => Mod::Keywords(kws_of(&o["keywords"])?),
            "types" => {
                let t = obj(&o["types"], "effect.types")?;
                keys(t, &["add", "subtypes", "replace"], "effect.types")?;
                Mod::Types {
                    add: t.get("add").map_or(Ok(0), types_of)?,
                    subtypes: t.get("subtypes").map_or(Ok(Vec::new()), |x| subtypes_of(db, x))?,
                    replace: boolean(t, "replace")?,
                }
            }
            "become_types" => {
                let t = obj(&o["become_types"], "effect.become_types")?;
                keys(t, &["types", "subtypes"], "effect.become_types")?;
                Mod::BecomeTypes {
                    types: t.get("types").map_or(Ok(0), types_of)?,
                    subtypes: t.get("subtypes").map_or(Ok(Vec::new()), |x| subtypes_of(db, x))?,
                }
            }
            "colors" => Mod::Colors(colors_of(o["colors"].as_str().ok_or("build: effect.colors is a string of WUBRG letters")?)?),
            "cant_block" => Mod::CantBlock,
            "cant_be_blocked" => Mod::CantBeBlocked,
            "must_attack" => Mod::MustAttack,
            "exile_if_dies" => Mod::ExileIfDies,
            "exile_if_leaves" => Mod::ExileIfLeaves,
            "must_attack_player" => Mod::MustAttackPlayer,
            "goad" => Mod::Goad,
            "lose_all_abilities" => Mod::LoseAllAbilities,
            _ => unreachable!(),
        };
        let ts = o.get("ts").map(|x| x.as_u64().ok_or("build: effect.ts must be a non-negative integer")).transpose()?;
        Ok((Effect { ts: 0, until, on, m, controller }, ts))
    }

    /// The description's hidden-zone slots of player `p`: (hand, library from the top).
    fn hidden_plan(&mut self, p: PlayerId, po: &Map<String, Value>) -> Result<(Vec<Slot>, Vec<Slot>), String> {
        let me = self.me;
        let o = opp(p);
        let db = self.db;
        let hand = arr(po, "hand")?;
        let size = uint(po, "hand_size", hand.len() as u64)? as usize;
        if size < hand.len() {
            return Err(format!("build: P{p}'s hand_size {size} is less than the {} hand cards named", hand.len()));
        }
        if p == me && size != hand.len() {
            return Err(format!("build: the viewer's hand is known: P{p}'s hand names {} of {size} cards", hand.len()));
        }
        let mut h = Vec::new();
        for e in hand {
            let name = entry_name(e)?;
            let (card, face) = lookup_card(db, name)?;
            if face != card {
                return Err(format!("build: {name} in a hand: name the card"));
            }
            if let Some(eo) = entry_obj(e) {
                keys(eo, &["name", "known_to_opponent", "ref"], "a hand card")?;
            }
            let kto = entry_obj(e).map_or(Ok(false), |eo| boolean(eo, "known_to_opponent"))?;
            if p != me && kto {
                return Err("build: known_to_opponent is for the viewer's own hand cards".into());
            }
            self.take(p, card, name)?;
            let known = if p == me { (1 << me) | if kto { 1 << o } else { 0 } } else { 0b11 };
            h.push(Slot { def: Some(card), card: Some(card), known });
        }
        for _ in hand.len()..size {
            h.push(Slot { def: None, card: None, known: 1 << p });
        }
        let lsize = uint(po, "library_size", 0)? as usize;
        let mut lib = vec![Slot { def: None, card: None, known: 0 }; lsize];
        for e in arr(po, "library_known")? {
            let eo = obj(e, "a library_known entry")?;
            keys(eo, &["pos", "name", "known_by"], "a library_known entry")?;
            let pos = uint(eo, "pos", u64::MAX)? as usize;
            if pos >= lsize {
                return Err(format!("build: P{p}'s library_known position {pos} (the library has {lsize} cards)"));
            }
            let mut known = 0u8;
            for q in arr(eo, "known_by")? {
                match q.as_u64() {
                    Some(q) if q < 2 => known |= 1 << q,
                    _ => return Err("build: known_by lists players (0, 1)".into()),
                }
            }
            if known == 0 {
                return Err(format!("build: P{p}'s library position {pos}: known_by names nobody"));
            }
            if lib[pos].known != 0 {
                return Err(format!("build: P{p}'s library position {pos} is described twice"));
            }
            let named = eo.get("name").and_then(|x| x.as_str());
            match (named, known & (1 << me) != 0) {
                (Some(n), true) => {
                    let (card, face) = lookup_card(db, n)?;
                    if face != card {
                        return Err(format!("build: {n} in a library: name the card"));
                    }
                    self.take(p, card, n)?;
                    lib[pos] = Slot { def: Some(card), card: Some(card), known };
                }
                (None, false) => lib[pos] = Slot { def: None, card: None, known },
                (Some(n), false) => return Err(format!("build: P{p}'s library position {pos} names {n}, which the viewer does not know")),
                (None, true) => return Err(format!("build: P{p}'s library position {pos} is known to the viewer: name it")),
            }
        }
        Ok((h, lib))
    }
}

impl MtgAz {
    /// A game started at the position of `spec.build` (module docs; ENGINE.md 95), its unknown cards
    /// dealt from the deal seed (the build's `deal_seed`, else `game_seed`).
    pub fn from_build(spec: &MtgSpec, game_seed: u64) -> Result<MtgAz, String> {
        let b = spec.build.as_deref().ok_or("from_build: a spec without build")?;
        if spec.format != super::Format::Bo1 {
            return Err("build: best-of-one only".into());
        }
        let db = spec.db();
        let seats = spec.seats(game_seed);
        let gspec = GameSpec { max_turns: spec.max_turns, mulligans: spec.mulligans, chooser: spec.chooser, hidden_decks: false, log: spec.log, ..GameSpec::default() };
        let d = obj(&b.desc, "build")?;
        keys(
            d,
            &[
                "format", "viewer", "deal_seed", "turn", "active", "step", "priority", "passes", "starting", "chooser", "cleanup_again", "decision", "players",
                "battlefield", "exile", "stack", "emblems", "effects", "delayed", "temp_triggers", "marks", "discarded", "resolved", "done_once",
                "activations", "clouds", "then", "void", "had_attackers",
            ],
            "build",
        )?;
        let mut remaining = [seats[0].deck.clone(), seats[1].deck.clone()];
        for r in &mut remaining {
            r.sort_unstable();
        }
        let mut bl = Builder { db, g: Game::empty(db, gspec, game_seed), me: b.viewer, d, remaining, refs: BTreeMap::new(), items: BTreeMap::new(), lki: Vec::new() };
        bl.build()?;
        let g = bl.g;
        let m = MatchState::new(db, spec.format, gspec, &seats, game_seed)?;
        let mut az = MtgAz { g, m, lab: None, describe: spec.describe };
        az.set_known_pools();
        // `then`: actions taken from the built decision on (an option label or an action id each), before
        // the deal - a decision inside a resolution is the priority before it and the passes to it
        for (i, x) in arr(d, "then")?.iter().enumerate() {
            let a = then_action(&az.g, x).map_err(|e| format!("build: then[{i}]: {e}"))?;
            zero_search::game::SearchGame::step(&mut az, a, &mut Vec::new()).map_err(|e| format!("build: then[{i}]: {e}"))?;
        }
        // the deal: everything the viewer does not know, dealt again (the description has none of it)
        let deal_seed = b.deal_seed.unwrap_or(game_seed);
        let mut rng = zero_search::rng::Rng::new(zero_search::rng::split64(deal_seed ^ SALT));
        zero_search::game::SearchGame::determinize(&mut az, b.viewer as usize, &mut rng);
        // the dealt world is a real game from here (no determinization observer: ENGINE.md 76.1)
        az.g.observer = None;
        az.g.check_invariants().map_err(|e| format!("build: the built position breaks an invariant: {e}"))?;
        Ok(az)
    }
}

/// A `then` item at the pending decision: an action id among its options, or the one option whose
/// label (`view.rs`'s words, as `export_state` shows them) it is.
fn then_action(g: &Game, x: &Value) -> Result<u16, String> {
    let d = g.pending.as_ref().ok_or("the game is over")?;
    match x {
        Value::Number(n) => {
            let a = n.as_u64().ok_or("an action id is a non-negative integer")?;
            d.options.iter().find(|o| o.action as u64 == a).map(|o| o.action).ok_or_else(|| format!("{a} is not an option of P{}'s {:?} decision", d.player, d.kind))
        }
        Value::String(l) => {
            let m: Vec<u16> = d.options.iter().filter(|o| g.option_text(d.kind, o.choice) == *l).map(|o| o.action).collect();
            match m[..] {
                [a] => Ok(a),
                [] => Err(format!("{l:?} is not an option of P{}'s {:?} decision", d.player, d.kind)),
                _ => Err(format!("{l:?} names {} options of P{}'s {:?} decision (actions {m:?}): give an action id", m.len(), d.player, d.kind)),
            }
        }
        _ => Err(format!("{x} is neither an option label nor an action id")),
    }
}

impl Builder<'_> {
    fn build(&mut self) -> Result<(), String> {
        let d = self.d;
        let db = self.db;
        let me = self.me;
        let number = uint(d, "turn", 0)? as u32;
        let step = step_of(d.get("step").and_then(|x| x.as_str()).ok_or("build: step is required")?)?;
        if (number == 0) != (step == Step::Setup) {
            return Err(format!("build: turn {number} in step {}: turn 0 is the setup, and the setup is turn 0", step_name(step)));
        }
        if number > self.g.spec.max_turns {
            return Err(format!("build: turn {number} past the turn cap {}", self.g.spec.max_turns));
        }
        let active = need_player(d, "active", "build")?;
        let starting = need_player(d, "starting", "build")?;
        if number > 0 && active != if number % 2 == 1 { starting } else { opp(starting) } {
            return Err(format!("build: P{active} is not the active player of turn {number} (P{starting} took the first turn; extra turns are not built)"));
        }
        let chooser = player_of(d, "chooser")?.unwrap_or(starting);
        let players = arr(d, "players")?;
        if players.len() != 2 {
            return Err("build: players must list both players".into());
        }
        let pos: Vec<&Map<String, Value>> = players.iter().map(|p| obj(p, "a player")).collect::<Result<_, _>>()?;
        for p in &pos {
            keys(
                p,
                &[
                    "life", "hand", "hand_size", "library_size", "library_known", "graveyard", "pool", "lands_played", "drawn", "draw_step_draws",
                    "spells_cast", "noncreature_cast", "spells_game", "energy", "mulligans", "gained_life", "lost_life", "left_battlefield", "kept",
                    "mulligan_declared", "turns", "attacked_with", "artifact_entered", "life_gained",
                ],
                "a player",
            )?;
        }

        // ---- the deck cards in public zones (placed: their places are their references) ----
        for p in 0..2u8 {
            for (i, e) in arr(pos[p as usize], "graveyard")?.iter().enumerate() {
                if let Some(eo) = entry_obj(e) {
                    keys(eo, &["name", "ref"], "a graveyard card")?;
                }
                let c = self.card(p, e, Zone::Graveyard)?;
                self.g.players[p as usize].graveyard.push(c);
                self.place_ref(format!("g{p}.{i}"), c);
                self.alias(e, c)?;
            }
        }
        let mut exile_counters = Vec::new();
        for (i, e) in arr(d, "exile")?.iter().enumerate() {
            let eo = obj(e, "an exile card")?;
            keys(eo, &["name", "owner", "ref", "counters", "since"], "an exile card")?;
            let owner = need_player(eo, "owner", "an exile card")?;
            let c = self.card(owner, e, Zone::Exile)?;
            self.g.objs[c.i()].control_since = uint(eo, "since", 0)? as u32;
            self.g.exile.push(c);
            self.place_ref(format!("x{i}"), c);
            self.alias(e, c)?;
            if let Some(cs) = eo.get("counters") {
                exile_counters.push((c, cs.clone()));
            }
        }
        let bfv = arr(d, "battlefield")?;
        let mut bf: Vec<Option<CardIdx>> = vec![None; bfv.len()];
        for (i, e) in bfv.iter().enumerate() {
            let eo = obj(e, "a permanent")?;
            keys(
                eo,
                &[
                    "name", "token", "owner", "controller", "tapped", "entered", "control_since", "sick", "counters", "damage", "attached_to",
                    "attacking", "attacks", "blocked", "blocking", "struck_first", "loyalty_turn", "loyalty_used", "flips", "ts", "ref",
                ],
                "a permanent",
            )?;
            if boolean(eo, "token")? {
                continue;
            }
            let owner = need_player(eo, "owner", "a permanent")?;
            let c = self.card(owner, e, Zone::Battlefield)?;
            bf[i] = Some(c);
        }
        let stv = arr(d, "stack")?;
        let mut spells: Vec<Option<CardIdx>> = vec![None; stv.len()];
        for (i, e) in stv.iter().enumerate() {
            let eo = obj(e, "a stack item")?;
            keys(
                eo,
                &[
                    "controller", "spell", "owner", "origin", "ability", "source", "targets", "cost_option", "modes", "ev", "alt", "from", "delved",
                    "x", "division", "attack_as", "noted", "times", "spliced", "ref",
                ],
                "a stack item",
            )?;
            let controller = need_player(eo, "controller", "a stack item")?;
            if let Some(s) = eo.get("spell") {
                let owner = player_of(eo, "owner")?.unwrap_or(controller);
                let c = self.card(owner, s, Zone::Stack)?;
                spells[i] = Some(c);
                self.place_ref(format!("s{i}"), c);
            } else if !eo.contains_key("ability") {
                return Err("build: a stack item is a spell or an ability".into());
            }
            self.items.insert(format!("s{i}"), i);
            if let Some(r) = eo.get("ref").and_then(|x| x.as_str()) {
                self.items.insert(r.to_string(), i);
                if let Some(c) = spells[i] {
                    self.refs.insert(r.to_string(), c);
                }
            }
        }

        // ---- the hidden zones: the named cards, then the deck's rest dealt into the unknown slots ----
        let mut plans = Vec::new();
        for p in 0..2u8 {
            plans.push(self.hidden_plan(p, pos[p as usize])?);
        }
        for p in 0..2u8 {
            let (h, l) = &plans[p as usize];
            let unknown = h.iter().chain(l).filter(|s| s.def.is_none()).count();
            let left = self.remaining[p as usize].len();
            if unknown != left {
                return Err(format!(
                    "build: P{p}'s deck does not fit the description: {left} of its cards are not named anywhere but the description leaves {unknown} unknown slots (hand and library)"
                ));
            }
        }
        // the viewer's clouds over the opponent: their cards into their domains first
        let clv = arr(d, "clouds")?;
        let mut clouds = Vec::new();
        for e in clv {
            let eo = obj(e, "a cloud")?;
            keys(eo, &["owner", "hand", "library", "positions", "cards"], "a cloud")?;
            let owner = need_player(eo, "owner", "a cloud")?;
            if owner == me {
                return Err("build: a cloud is the viewer's partial knowledge of the opponent's cards (owner: the opponent)".into());
            }
            let (hand, library) = (boolean(eo, "hand")?, boolean(eo, "library")?);
            let positions: Vec<usize> = arr(eo, "positions")?.iter().map(|x| x.as_u64().map(|n| n as usize).ok_or("build: cloud positions are integers")).collect::<Result<_, _>>()?;
            let mut cards = Vec::new();
            for n in arr(eo, "cards")? {
                let n = n.as_str().ok_or("build: cloud cards are names")?;
                cards.push(lookup_card(db, n)?.0);
            }
            cards.sort_unstable();
            let (h, l) = &mut plans[owner as usize];
            for &pp in &positions {
                if pp >= l.len() {
                    return Err(format!("build: a cloud's library position {pp} (P{owner}'s library has {})", l.len()));
                }
            }
            for &card in &cards {
                self.take(owner, card, &db.def(card).name).map_err(|_| format!("build: a cloud lists {} but P{owner} has none left to deal", db.def(card).name))?;
                // (the plan's library is indexed from the top, as the positions are)
                let slot = h
                    .iter_mut()
                    .filter(|_| hand)
                    .chain(l.iter_mut().enumerate().filter(|(i, _)| library || positions.contains(i)).map(|(_, s)| s))
                    .find(|s| s.def.is_none() && s.card.is_none())
                    .ok_or_else(|| format!("build: a cloud lists more cards than its domain has unknown slots ({})", db.def(card).name))?;
                slot.card = Some(card);
            }
            clouds.push((owner, hand, library, positions, cards));
        }
        // the deck's rest, in order (`determinize` deals them again at the end)
        for p in 0..2u8 {
            let mut rest = std::mem::take(&mut self.remaining[p as usize]).into_iter();
            let (h, l) = &mut plans[p as usize];
            for s in h.iter_mut().chain(l.iter_mut()) {
                if s.card.is_none() {
                    s.card = Some(rest.next().expect("counted above"));
                }
            }
        }
        for p in 0..2u8 {
            let (h, l) = plans[p as usize].clone();
            for (i, s) in h.iter().enumerate() {
                let c = self.g.add_card(s.card.unwrap(), p, Zone::Hand);
                self.g.objs[c.i()].inc = 1;
                self.g.objs[c.i()].known = s.known;
                self.g.players[p as usize].hand.push(c);
                if s.def.is_some() {
                    self.place_ref(format!("h{p}.{i}"), c);
                    let e = &arr(pos[p as usize], "hand")?[i];
                    self.alias(e, c)?;
                }
            }
            // (the library's index 0 is its bottom)
            for s in l.iter().rev() {
                let c = self.g.add_card(s.card.unwrap(), p, Zone::Library);
                self.g.objs[c.i()].inc = 1;
                self.g.objs[c.i()].known = s.known;
                self.g.players[p as usize].library.push(c);
            }
        }

        // ---- tokens and emblems (after every card: the first token slot) ----
        for (i, e) in bfv.iter().enumerate() {
            let eo = obj(e, "a permanent")?;
            if boolean(eo, "token")? {
                let owner = need_player(eo, "owner", "a permanent")?;
                bf[i] = Some(self.token(owner, entry_name(e)?, Zone::Battlefield)?);
            }
        }
        for (i, e) in bfv.iter().enumerate() {
            let c = bf[i].unwrap();
            self.place_ref(format!("b{i}"), c);
            self.alias(e, c)?;
        }
        for (i, e) in arr(d, "emblems")?.iter().enumerate() {
            let eo = obj(e, "an emblem")?;
            keys(eo, &["name", "controller", "ref"], "an emblem")?;
            let def = lookup_token(db, entry_name(e)?, true)?;
            let p = need_player(eo, "controller", "an emblem")?;
            let c = self.g.add_card(def, p, Zone::Command);
            self.g.objs[c.i()].inc = 1;
            self.g.objs[c.i()].known = 0b11;
            self.g.command.push(c);
            self.place_ref(format!("e{i}"), c);
            self.alias(e, c)?;
        }

        // ---- the battlefield's state ----
        let mut ts_keys: Vec<(u64, u8, usize)> = Vec::new();
        for (i, e) in bfv.iter().enumerate() {
            let eo = obj(e, "a permanent")?;
            let c = bf[i].unwrap();
            let owner = need_player(eo, "owner", "a permanent")?;
            let controller = player_of(eo, "controller")?.unwrap_or(owner);
            let sick = boolean(eo, "sick")?;
            let entered = uint(eo, "entered", if sick { number as u64 } else { 0 })?;
            let since = uint(eo, "control_since", if sick { number as u64 } else { 0 })?;
            {
                let o = &mut self.g.objs[c.i()];
                o.controller = controller;
                o.tapped = boolean(eo, "tapped")?;
                o.entered = entered as u16;
                o.control_since = since as u32;
                o.damage = uint(eo, "damage", 0)? as u64;
                o.attacking = boolean(eo, "attacking")?;
                o.blocked = boolean(eo, "blocked")?;
                o.struck_first = boolean(eo, "struck_first")?;
                o.flips = uint(eo, "flips", 0)? as u32;
            }
            let (lt, lu) = (uint(eo, "loyalty_turn", 0)?, uint(eo, "loyalty_used", 0)?);
            if lt != 0 || lu != 0 {
                let l = self.g.loyalty_mut(c);
                l.turn = lt as u16;
                l.used = lu as u32;
            }
            if let Some(cs) = eo.get("counters") {
                self.counters(c, cs)?;
            }
            if let Some(t) = eo.get("ts") {
                ts_keys.push((t.as_u64().ok_or("build: ts must be a non-negative integer")?, 0, i));
            }
            self.g.battlefield.push(c);
        }
        for (c, cs) in exile_counters {
            self.counters(c, &cs)?;
        }
        for (i, e) in bfv.iter().enumerate() {
            let eo = obj(e, "a permanent")?;
            let c = bf[i].unwrap();
            let attached = self.opt_obj(eo, "attached_to")?;
            let attacks = self.opt_obj(eo, "attacks")?;
            let blocking = self.opt_obj(eo, "blocking")?;
            let o = &mut self.g.objs[c.i()];
            o.attached = attached;
            o.attacks = attacks;
            if attacks.is_some() && !o.attacking {
                return Err("build: attacks is for an attacking creature".into());
            }
            o.blocking = blocking.map(|r| r.idx);
        }

        // ---- the stack, bottom first ----
        for (i, e) in stv.iter().enumerate() {
            let eo = obj(e, "a stack item")?;
            let controller = need_player(eo, "controller", "a stack item")?;
            let kind = match spells[i] {
                Some(card) => StackKind::Spell { card },
                None => {
                    let ability = self.ability(&eo["ability"])?;
                    let source = self.obj(eo.get("source").ok_or("build: an ability needs its source")?)?;
                    StackKind::Ability { ability, source }
                }
            };
            let mut targets = Vec::new();
            for t in arr(eo, "targets")? {
                targets.push(self.target(t)?);
            }
            let ev = self.ev(eo.get("ev"))?;
            let attack_as = match eo.get("attack_as") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s == "player" => Some(None),
                Some(v) => Some(Some(self.obj(v)?)),
            };
            let mut noted = Vec::new();
            for r in arr(eo, "noted")? {
                noted.push(self.obj(r)?);
            }
            let bytes = |k: &str| -> Result<Vec<u8>, String> {
                arr(eo, k)?.iter().map(|x| x.as_u64().map(|n| n as u8).ok_or_else(|| format!("build: {k} holds integers"))).collect()
            };
            let spliced: Vec<DefId> = arr(eo, "spliced")?
                .iter()
                .map(|n| lookup_card(db, n.as_str().unwrap_or("")).map(|x| x.0))
                .collect::<Result<_, _>>()?;
            let origin = match (spells[i], eo.get("origin").and_then(|x| x.as_str())) {
                (_, Some(z)) => zone_of(z)?,
                (Some(_), None) => Zone::Hand,
                (None, None) => Zone::Nowhere,
            };
            let flips = match kind {
                StackKind::Ability { source, .. } if self.g.live(source) => self.g.objs[source.idx.i()].flips,
                _ => 0,
            };
            let item = StackItem {
                id: i as u32 + 1,
                kind,
                controller,
                targets,
                cost_option: uint(eo, "cost_option", 0)? as u8,
                modes: bytes("modes")?,
                ev,
                alt: uint(eo, "alt", 0)? as u8,
                from: eo.get("from").map_or(Ok(CastFrom::Hand), from_of)?,
                delved: uint(eo, "delved", 0)? as u8,
                delve_floor: 0,
                discarded_land: false,
                x: uint(eo, "x", 0)? as u32,
                division: bytes("division")?.into_iter().map(u64::from).collect(),
                attack_as,
                noted,
                flips,
                origin,
                times: uint(eo, "times", 0)? as u32,
                spliced,
            };
            self.g.stack.push(item);
        }
        self.g.next_stack_id = stv.len() as u32 + 1;

        // ---- continuous effects, delayed and temporary triggers, marks, this turn's lists ----
        let mut effects = Vec::new();
        for (i, e) in arr(d, "effects")?.iter().enumerate() {
            let (ef, ts) = self.effect(e)?;
            if let Some(t) = ts {
                ts_keys.push((t, 1, i));
            }
            effects.push(ef);
        }
        // timestamps: the battlefield's order, the effects' order, interleaved by `ts` when given
        let n_bf = bfv.len();
        if !ts_keys.is_empty() && ts_keys.len() != n_bf + effects.len() {
            return Err("build: ts is given for every permanent and effect, or for none".into());
        }
        let mut order: Vec<(u8, usize)> = if ts_keys.is_empty() {
            (0..n_bf).map(|i| (0, i)).chain((0..effects.len()).map(|i| (1, i))).collect()
        } else {
            ts_keys.sort_unstable();
            if ts_keys.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err("build: two timestamps are equal".into());
            }
            ts_keys.iter().map(|&(_, k, i)| (k, i)).collect()
        };
        // (given timestamps are kept as they are: the next one is later than all)
        let given: BTreeMap<(u8, usize), u32> = ts_keys.iter().map(|&(t, k, i)| ((k, i), t as u32)).collect();
        let mut next_ts = 0u32;
        let mut last_bf: Option<usize> = None;
        for (k, i) in order.drain(..) {
            next_ts = given.get(&(k, i)).copied().unwrap_or(next_ts + 1);
            if k == 0 {
                if last_bf.is_some_and(|l| l > i) {
                    return Err("build: the battlefield is listed in timestamp order".into());
                }
                last_bf = Some(i);
                let c = bf[i].unwrap();
                self.g.objs[c.i()].timestamp = next_ts;
            } else {
                effects[i].ts = next_ts;
            }
        }
        self.g.next_ts = next_ts;
        self.g.effects = effects;
        self.g.effects_changed();
        for e in arr(d, "delayed")? {
            let eo = obj(e, "a delayed trigger")?;
            keys(eo, &["when", "controller", "source", "card", "index", "ev", "turn"], "a delayed trigger")?;
            let when = when_of(eo.get("when").and_then(|x| x.as_str()).ok_or("build: delayed.when is required")?)?;
            let source_def = lookup_any(db, eo.get("card").and_then(|x| x.as_str()).ok_or("build: delayed.card is required")?)?;
            let slot = uint(eo, "index", 0)? as u8;
            if slot as usize >= db.def(source_def).triggers.len() {
                return Err(format!("build: {} has no trigger {slot}", db.def(source_def).name));
            }
            let source = self.obj(eo.get("source").ok_or("build: delayed.source is required")?)?;
            let ev = self.ev(eo.get("ev"))?;
            self.g.delayed.push(Delayed { when, controller: need_player(eo, "controller", "a delayed trigger")?, source, source_def, slot, ev, turn: uint(eo, "turn", number as u64)? as u32 });
        }
        for e in arr(d, "temp_triggers")? {
            let eo = obj(e, "a temporary trigger")?;
            keys(eo, &["controller", "source", "card", "index", "this_turn", "obj"], "a temporary trigger")?;
            let source_def = lookup_any(db, eo.get("card").and_then(|x| x.as_str()).ok_or("build: temp_triggers.card is required")?)?;
            let slot = uint(eo, "index", 0)? as u8;
            let source = self.obj(eo.get("source").ok_or("build: temp_triggers.source is required")?)?;
            let o2 = self.opt_obj(eo, "obj")?;
            self.g.temp_triggers.push(TempTrigger {
                controller: need_player(eo, "controller", "a temporary trigger")?,
                source,
                source_def,
                slot,
                this_turn: boolean(eo, "this_turn")?,
                obj: o2,
            });
        }
        for e in arr(d, "marks")? {
            let eo = obj(e, "a mark")?;
            keys(eo, &["on", "exiled_to_pay"], "a mark")?;
            let on = self.obj(eo.get("on").ok_or("build: mark.on is required")?)?;
            let x = self.obj(eo.get("exiled_to_pay").ok_or("build: a mark is exiled_to_pay (the only kind built)")?)?;
            self.g.marks.push((on, Mark::ExiledToPay(x)));
        }
        for r in arr(d, "discarded")? {
            let r = self.obj(r)?;
            self.g.discarded.push(r);
        }
        for e in arr(d, "resolved")? {
            let eo = obj(e, "a resolved count")?;
            keys(eo, &["on", "count"], "a resolved count")?;
            let r = self.obj(eo.get("on").ok_or("build: resolved.on is required")?)?;
            self.g.resolved.push((r, uint(eo, "count", 1)? as u32));
        }
        for e in arr(d, "done_once")? {
            let eo = obj(e, "a done_once entry")?;
            keys(eo, &["on", "trigger"], "a done_once entry")?;
            let r = self.obj(eo.get("on").ok_or("build: done_once.on is required")?)?;
            self.g.done_once.push((r, uint(eo, "trigger", 0)? as u8));
        }
        for e in arr(d, "activations")? {
            let eo = obj(e, "an activation count")?;
            keys(eo, &["on", "ability", "count"], "an activation count")?;
            let r = self.obj(eo.get("on").ok_or("build: activations.on is required")?)?;
            // (v6 engines: the activation cap is gone with its counts, loops.rs; a count is accepted and read by nothing)
            let _ = (r, uint(eo, "ability", 0)?, uint(eo, "count", 1)?);
        }
        for (owner, hand, library, positions, cards) in clouds {
            let lib = &self.g.players[owner as usize].library;
            let n = lib.len();
            let slots: Vec<CardIdx> = positions.iter().map(|&pp| lib[n - 1 - pp]).collect();
            self.g.clouds.push(crate::hidden::Cloud { viewer: me, owner, hand, library, slots, defs: cards });
        }

        // ---- the players and the turn ----
        let priority = player_of(d, "priority")?.unwrap_or(active);
        let passes = match d.get("passes") {
            Some(x) if !x.is_null() => x.as_u64().filter(|&n| n < 2).ok_or("build: passes must be 0 or 1")? as u8,
            // (a pass by the other player gave this one priority)
            _ => match self.g.stack.last() {
                None => (priority != active) as u8,
                Some(top) => (priority != top.controller) as u8,
            },
        };
        self.g.turn = crate::game::Turn {
            number,
            active,
            step,
            priority,
            passes,
            starting,
            normal: if number > 0 { active } else { 0 },
            cleanup_again: false,
            extra_combats: 0,
            after_combat: Step::Main2,
            chooser,
            had_attackers: false,
        };
        // CR 508.8 (ENGINE.md 151.4): derived from the attacking creatures (placed above) unless given - true with
        // none attacking: an attacker left combat since
        let attacking = self.g.battlefield.iter().any(|&c| self.g.objs[c.i()].attacking);
        self.g.turn.had_attackers = match d.get("had_attackers") {
            None | Some(Value::Null) => had_attackers_derived(step, attacking),
            Some(x) => {
                let b = x.as_bool().ok_or("build: had_attackers must be a boolean")?;
                if b && step != Step::DeclareAttackers {
                    return Err("build: had_attackers is the declare attackers step's (CR 508.8)".into());
                }
                b
            }
        };
        // void (v6: encoded, ENGINE.md 99.4): the description says it; nothing else sets it in a build
        self.g.void_turn = if boolean(d, "void")? { number } else { 0 };
        for p in 0..2u8 {
            let po = pos[p as usize];
            let (last, turns) = derived_turns(&self.g.turn, p);
            let mut attacked = Vec::new();
            for r in arr(po, "attacked_with")? {
                attacked.push(self.obj(r)?);
            }
            let mut pool = crate::mana::ManaPool::default();
            for ch in po.get("pool").and_then(|x| x.as_str()).unwrap_or("").chars() {
                let i = crate::mana::LETTERS.iter().position(|&l| l == ch).ok_or_else(|| format!("build: unknown mana {ch:?} in a pool (WUBRGC)"))?;
                pool.amt[i] += 1;
            }
            let setup = step == Step::Setup;
            let pl = &mut self.g.players[p as usize];
            pl.life = int(po, "life", 20)? as i64;
            pl.pool = pool;
            pl.lands_played = uint(po, "lands_played", 0)? as u8;
            pl.drawn = uint(po, "drawn", 0)? as u32;
            pl.draw_step_draws = uint(po, "draw_step_draws", 0)? as u32;
            pl.spells_cast = uint(po, "spells_cast", 0)? as u32;
            pl.noncreature_cast = uint(po, "noncreature_cast", 0)? as u32;
            pl.spells_game = uint(po, "spells_game", 0)? as u32;
            pl.energy = uint(po, "energy", 0)? as crate::num::Count;
            pl.mulligans = uint(po, "mulligans", 0)? as u8;
            pl.gained_life = boolean(po, "gained_life")?;
            pl.lost_life = boolean(po, "lost_life")?;
            pl.left_battlefield = boolean(po, "left_battlefield")?;
            pl.artifact_entered = boolean(po, "artifact_entered")?;
            pl.life_gained = int(po, "life_gained", 0)?;
            pl.kept = if setup { boolean(po, "kept")? } else { true };
            pl.mulligan_declared = if setup { boolean(po, "mulligan_declared")? } else { false };
            pl.last_turn = last;
            pl.turns = uint(po, "turns", turns as u64)? as u32;
            pl.attacked_with = attacked;
        }

        // ---- the fast paths ----
        let g = &mut self.g;
        g.n_static = (g.battlefield.iter().filter(|&&c| !g.db.def(g.objs[c.i()].def).statics.is_empty()).count()
            + g.command.iter().filter(|&&c| !g.db.def(g.objs[c.i()].def).statics.is_empty()).count()) as u16;
        g.n_self_types = g.battlefield.iter().filter(|&&c| crate::layers::self_typed(g.db.def(g.objs[c.i()].def))).count() as u16;

        // ---- the decision ----
        self.decision(step)
    }

    fn counters(&mut self, c: CardIdx, v: &Value) -> Result<(), String> {
        let cs = obj(v, "counters")?;
        let mut list = Vec::new();
        for (k, n) in cs {
            let n = n.as_u64().ok_or("build: a counter count is a non-negative integer")?;
            if n > 0 {
                list.push((counter_of(self.db, k)?, n as crate::num::Count));
            }
        }
        list.sort();
        for (k, n) in list {
            self.g.counters.push((c, k, n));
        }
        Ok(())
    }

    fn decision(&mut self, step: Step) -> Result<(), String> {
        use Task as T;
        let d = self.d;
        let g = &mut self.g;
        let (s, a) = (g.turn.starting, g.turn.active);
        let dv = d.get("decision").cloned().unwrap_or(json!("priority"));
        let (kind, decider) = if dv == "priority" {
            if step == Step::Setup || step == Step::Untap {
                return Err(format!("build: no priority in the {} step", step_name(step)));
            }
            g.turn.cleanup_again = d.get("cleanup_again").and_then(|x| x.as_bool()).unwrap_or(step == Step::Cleanup);
            g.agenda = vec![T::PriorityAsk { page: crate::decision::Page::FIRST }];
            (DecisionKind::Priority, g.turn.priority)
        } else {
            let o = obj(&dv, "decision")?;
            let k = o.get("kind").and_then(|x| x.as_str()).ok_or("build: decision.kind is required")?;
            let want_step = |w: Step| if step == w { Ok(()) } else { Err(format!("build: a {k} decision in the {} step", step_name(step))) };
            g.turn.cleanup_again = d.get("cleanup_again").and_then(|x| x.as_bool()).unwrap_or(false);
            match k {
                "declare_attackers" => {
                    keys(o, &["kind", "next"], "decision")?;
                    want_step(Step::DeclareAttackers)?;
                    g.agenda = vec![T::Priority, T::DeclareAttackers { next: uint(o, "next", 0)? as u32 }];
                    (DecisionKind::DeclareAttackers, a)
                }
                "attack_target" => {
                    keys(o, &["kind", "next", "attackers"], "decision")?;
                    want_step(Step::DeclareAttackers)?;
                    let next = uint(o, "next", 0)? as u32;
                    let mut ag = vec![T::Priority, T::DeclareAttackers { next }];
                    for v in arr(o, "attackers")? {
                        ag.push(T::AttackTarget { card: self.obj(v)?.idx });
                    }
                    if ag.len() == 2 {
                        return Err("build: attack_target needs its attackers (the creatures whose target is to choose, the deciding one last)".into());
                    }
                    self.g.agenda = ag;
                    (DecisionKind::AttackTarget, a)
                }
                "declare_blockers" => {
                    keys(o, &["kind", "next"], "decision")?;
                    want_step(Step::DeclareBlockers)?;
                    g.agenda = vec![T::Priority, T::DeclareBlockers { i: uint(o, "next", 0)? as u32 }];
                    (DecisionKind::DeclareBlockers, opp(a))
                }
                "cleanup_discard" => {
                    keys(o, &["kind"], "decision")?;
                    want_step(Step::Cleanup)?;
                    g.agenda = vec![T::CleanupRest, T::CleanupDiscard];
                    (DecisionKind::Discard, a)
                }
                "play_draw" => {
                    keys(o, &["kind", "player"], "decision")?;
                    want_step(Step::Setup)?;
                    let p = need_player(o, "player", "decision")?;
                    g.agenda = vec![T::PlayDraw { p }];
                    (DecisionKind::PlayDraw, p)
                }
                "mulligan" => {
                    keys(o, &["kind", "player"], "decision")?;
                    want_step(Step::Setup)?;
                    let p = need_player(o, "player", "decision")?;
                    g.agenda = vec![T::BeginTurn, T::Opening { p: s, exile: 0 }, T::MulliganDeclare { p }];
                    (DecisionKind::Mulligan, p)
                }
                "mulligan_bottom" => {
                    keys(o, &["kind", "bottoms"], "decision")?;
                    want_step(Step::Setup)?;
                    let mut ag = vec![T::BeginTurn, T::Opening { p: s, exile: 0 }, T::MulliganDeclare { p: s }];
                    let bottoms = arr(o, "bottoms")?;
                    let mut first = None;
                    for bv in bottoms.iter().rev() {
                        let pair = bv.as_array().filter(|x| x.len() == 2).ok_or("build: bottoms are [player, cards left to bottom]")?;
                        let p = pair[0].as_u64().filter(|&p| p < 2).ok_or("build: bottoms: a player")? as PlayerId;
                        let left = pair[1].as_u64().ok_or("build: bottoms: a count")? as u8;
                        ag.push(T::MulliganBottom { p, left });
                        first = Some(p);
                    }
                    let p = first.ok_or("build: mulligan_bottom needs bottoms")?;
                    g.agenda = ag;
                    (DecisionKind::MulliganBottom, p)
                }
                other => {
                    return Err(format!(
                        "build: unknown decision {other:?} (priority, declare_attackers, attack_target, declare_blockers, cleanup_discard, play_draw, mulligan, mulligan_bottom)"
                    ))
                }
            }
        };
        let g = &mut self.g;
        if g.sba_needed() {
            return Err("build: the position has state-based actions to perform (CR 704): no player would get a decision".into());
        }
        // (never answered internally: the built game stops at the decision described, whatever its options)
        g.no_auto = true;
        g.advance();
        g.no_auto = false;
        g.keys.clear();
        match &g.pending {
            Some(p) if p.kind == kind && p.player == decider => Ok(()),
            Some(p) => Err(format!("build: the engine asks P{} a {:?} decision here, not P{decider}'s {kind:?}", p.player, p.kind)),
            None => Err(format!("build: no decision at the position (the game ended: {:?})", g.outcome)),
        }
    }
}
