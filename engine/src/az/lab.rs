//! The position lab (ENGINE.md 76): a game of `mtg` started from a spot of another game, with
//! the hidden cards dealt again and the decider's first moves forced, so zero-core's match
//! driver plays a candidate move to the end from a real position (`tools/position_lab.py`).
//!
//! The spec's optional `start` block (`MtgSpec::start`; without one every game is what it was):
//!
//! ```json
//! "start": {"seed": 889364660, "actions": [384, 0, ...], "deal": "belief", "deal_seed": 7,
//!           "pred": [...], "tilt": 1, "clip": 20, "mix": 0.25,
//!           "hold": ["Force of Will"], "exclude": ["Daze"],
//!           "forced": ["Cast Mishra's Bauble", "resolve", 97, "*"],
//!           "follow": {"rule": "bauble_fetch", "land_min": 4, "fetches": ["Flooded Strand"]}}
//! ```
//!
//! * **The spot**: the source game (`MtgAz::new` of the same spec without `start`, game seed
//!   `seed`) after `actions`, each taken after the single-option decisions before it are
//!   auto-stepped as zero-core's transitions do (a replay's frames: `tools/root_view.py`'s
//!   `rebuild`); single-option decisions after the last are stepped too. The decider there is
//!   `viewer` (default: the player to move).
//! * **The deal** (`deal`), from the deal seed (`deal_seed`, else the driver's game seed: two
//!   games with the same deal seed deal the same world whatever their gid):
//!   * `"belief"` (the search's deal, ENGINE.md 69): `determinize_with` for the decider with the
//!     network's prediction `pred` (`MtgAz::AUX` order, `aux_width` floats), `tilt`, `clip`; a
//!     share `mix` of the deals uniform (`determinize`), one `unit()` draw per deal when mix > 0,
//!     as the tree draws it;
//!   * `"uniform"`: `determinize` for the decider;
//!   * `"actual"`: everything stays where it is but the library cards nobody knows, whose
//!     identities are shuffled among themselves within each set of cards the clouds treat alike
//!     (`Game::redeal_unknown_library_orders`): the opponent's real hand, a hindsight check;
//!   * `"none"`: the source game itself (its random stream continuing; tests).
//!
//!   `hold` (a multiset of card names) and `exclude` (names): the opponent's hand must hold at
//!   least these and none of those; `top` ([[P1's], [P2's]] card names, top first): each library
//!   starts so, and with `top_known` its owner knows those cards - the belief (or uniform) deal
//!   conditioned on it by rejection
//!   (at most `tries` deals, default 100,000; an impossible pin is an error at once). Every deal
//!   keeps what each player knows (`Game::check_knowledge` must hold after it: an error, never a
//!   silent world), and the dealt world is a real game from then on (no determinization observer).
//! * **The forced line** (`forced`): the decider's next non-minor decisions (not mana payments)
//!   take these options in order - an option label (`view.rs`'s words: the first option, by
//!   action id, with exactly that text), an action id, `"*"` (the network's own choice there), or
//!   `"resolve"` (pass while the decider has priority with something on the stack, the network
//!   answering any other question on the way - a trigger's surveil, a tax -, done at the first
//!   decision with an empty stack, whoever's; the engine asks no decision whose only option is a pass,
//!   so the decider's own empty-stack priority may never come): `legal_actions` offers only that
//!   option, so the driver steps it without a search. When the next item is not among the options
//!   (the opponent's answer changed the decision) the line is broken: the network plays on from
//!   there, and the follow rule never starts.
//! * **The follow rule** (`follow`, after the line; one rule today, `"bauble_fetch"`): at the
//!   decider's first decision after the line, if it knows the top card of its library (a Mishra's
//!   Bauble look resolved) the rule holds while that card stays its known top card: the card is
//!   "bad" if it is a land and the decider has at least `land_min` lands (battlefield + hand) - then
//!   its next priority with an option to activate one of `fetches` activates it (the lowest action
//!   id), and the rule ends -, else "good" - then the decider activates no fetch of its own accord
//!   (an empty stack or its own item on top; in answer to the opponent's spell or ability the
//!   network decides, and a crack there ends the rule too) until the card is drawn or moved.
//!   Without a known top card at that decision the rule ends. With `bad` / `good` (forced lines) the
//!   verdict's line continues the forced line from that decision first (a branch: "a land seen: play
//!   the fetch, crack it, cast Tamiyo; else play the Island, cast Tamiyo"), the rule as above after it.
//!
//! The forced line and the rule are part of the game state: the searches' worlds (cloned and
//! determinized) keep them, so a seat's search knows the decider's scripted moves (the opponent's
//! too: a small leak, documented in ENGINE.md 76.3). Best-of-one only.
use super::{identity, MtgAz, MtgSpec};
use crate::db::{types, CardDb};
use crate::decision::{Choice, DecisionKind};
use crate::game::Game;
use crate::ids::{CardIdx, DefId, PlayerId};
use serde_json::{json, Map, Value};
use zero_search::game::{Belief, SearchGame};

/// How a lab game deals what its decider cannot see (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deal {
    None,
    Actual,
    Uniform,
    Belief,
}

/// One item of a forced line (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forced {
    Label(String),
    Action(u16),
    Any,
    /// `"resolve"`: let the stack resolve - pass whenever the decider has priority with something on
    /// the stack, the network answering any other question on the way (a trigger's surveil, a tax);
    /// done at the first decision with an empty stack, whoever's; the next item applies at the
    /// decider's next decision.
    Resolve,
}

/// The follow rule `"bauble_fetch"` (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    pub land_min: u32,
    pub fetches: Vec<DefId>,
    /// Forced lines that continue the line by the verdict when the rule starts (empty: none).
    pub bad: Vec<Forced>,
    pub good: Vec<Forced>,
}

/// A list of forced items (labels, action ids, `"*"`, `"resolve"`).
fn parse_forced(x: &Value, what: &str) -> Result<Vec<Forced>, String> {
    x.as_array()
        .ok_or_else(|| format!("mtg spec: {what} must be a list"))?
        .iter()
        .map(|f| match f {
            Value::String(s) if s == "*" => Ok(Forced::Any),
            Value::String(s) if s == "resolve" => Ok(Forced::Resolve),
            Value::String(s) => Ok(Forced::Label(s.clone())),
            Value::Number(n) => n
                .as_u64()
                .filter(|&a| a < crate::decision::NUM_ACTIONS as u64)
                .map(|a| Forced::Action(a as u16))
                .ok_or_else(|| format!("mtg spec: {what}: {n} is not an action id")),
            _ => Err(format!("mtg spec: {what}: {f} is neither an option label, an action id, \"*\" nor \"resolve\"")),
        })
        .collect()
}

/// A spec's `start` block (module docs).
#[derive(Clone, Debug)]
pub struct Start {
    pub seed: u64,
    pub actions: Vec<u16>,
    pub viewer: Option<PlayerId>,
    pub deal: Deal,
    pub deal_seed: Option<u64>,
    pub pred: Vec<f32>,
    pub tilt: f64,
    pub clip: f64,
    pub mix: f64,
    /// Sorted (a multiset).
    pub hold: Vec<DefId>,
    /// Sorted, distinct.
    pub exclude: Vec<DefId>,
    pub tries: u32,
    pub forced: Vec<Forced>,
    pub follow: Option<Follow>,
    /// Per player, the library's top cards (top first) the deal must put there; `top_known`: their owner
    /// knows them (as after a Brainstorm's put-back).
    pub top: [Vec<DefId>; 2],
    pub top_known: bool,
}

// (floats compared by their bits: a spec is equal to itself, and MtgSpec is Eq)
impl PartialEq for Start {
    fn eq(&self, o: &Start) -> bool {
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
        (self.seed, &self.actions, self.viewer, self.deal, self.deal_seed) == (o.seed, &o.actions, o.viewer, o.deal, o.deal_seed)
            && bits(&self.pred) == bits(&o.pred)
            && (self.tilt.to_bits(), self.clip.to_bits(), self.mix.to_bits()) == (o.tilt.to_bits(), o.clip.to_bits(), o.mix.to_bits())
            && (&self.hold, &self.exclude, self.tries, &self.forced, &self.follow) == (&o.hold, &o.exclude, o.tries, &o.forced, &o.follow)
            && (&self.top, self.top_known) == (&o.top, o.top_known)
    }
}
impl Eq for Start {}

const KEYS: [&str; 16] = [
    "seed", "actions", "viewer", "deal", "deal_seed", "pred", "tilt", "clip", "mix", "hold", "exclude", "tries", "forced", "follow", "top",
    "top_known",
];

/// The deal's own stream: split from the deal seed with this salt.
const SALT: u64 = 0x1AB0_5EED_D3A1_0001;

impl Start {
    /// Parse a `start` block (`db`: the spec's card table).
    pub fn parse(v: &Value, db: &CardDb) -> Result<Start, String> {
        let o = v.as_object().ok_or("mtg spec: start must be an object")?;
        for k in o.keys() {
            if !KEYS.contains(&k.as_str()) {
                return Err(format!("mtg spec: unknown start key {k:?}"));
            }
        }
        let uint = |k: &str| -> Result<Option<u64>, String> {
            o.get(k).map(|x| x.as_u64().ok_or_else(|| format!("mtg spec: start.{k} must be a non-negative integer"))).transpose()
        };
        let float = |k: &str, d: f64| -> Result<f64, String> {
            o.get(k).map_or(Ok(d), |x| x.as_f64().ok_or_else(|| format!("mtg spec: start.{k} must be a number")))
        };
        let names = |k: &str| -> Result<Vec<DefId>, String> {
            let Some(x) = o.get(k) else { return Ok(Vec::new()) };
            let a = x.as_array().ok_or_else(|| format!("mtg spec: start.{k} must be a list of card names"))?;
            let mut v = a
                .iter()
                .map(|n| db.expect_id(n.as_str().ok_or_else(|| format!("mtg spec: start.{k}: card names are strings"))?))
                .collect::<Result<Vec<_>, _>>()?;
            v.sort_unstable();
            Ok(v)
        };
        let seed = uint("seed")?.ok_or("mtg spec: start.seed (the source game's seed) is required")?;
        let actions = match o.get("actions") {
            None => Vec::new(),
            Some(x) => x
                .as_array()
                .ok_or("mtg spec: start.actions must be a list of action ids")?
                .iter()
                .map(|a| a.as_u64().filter(|&a| a < crate::decision::NUM_ACTIONS as u64).map(|a| a as u16))
                .collect::<Option<Vec<u16>>>()
                .ok_or("mtg spec: start.actions must be action ids")?,
        };
        let viewer = match uint("viewer")? {
            None => None,
            Some(p) if p < 2 => Some(p as PlayerId),
            Some(p) => return Err(format!("mtg spec: start.viewer {p} (0 or 1)")),
        };
        let deal = match o.get("deal").map(|x| x.as_str()) {
            None | Some(Some("belief")) => Deal::Belief,
            Some(Some("uniform")) => Deal::Uniform,
            Some(Some("actual")) => Deal::Actual,
            Some(Some("none")) => Deal::None,
            Some(_) => return Err(format!("mtg spec: start.deal must be \"belief\", \"uniform\", \"actual\" or \"none\", not {}", o["deal"])),
        };
        let pred = match o.get("pred") {
            None => Vec::new(),
            Some(x) => x
                .as_array()
                .ok_or("mtg spec: start.pred must be a list of numbers")?
                .iter()
                .map(|p| p.as_f64().filter(|p| p.is_finite() && *p >= 0.0).map(|p| p as f32))
                .collect::<Option<Vec<f32>>>()
                .ok_or("mtg spec: start.pred must hold finite numbers >= 0")?,
        };
        let width: usize = MtgAz::AUX.iter().map(|t| t.1).sum();
        match deal {
            Deal::Belief if pred.len() != width => {
                return Err(format!(
                    "mtg spec: start.deal \"belief\" needs start.pred, the network's {width} auxiliary predictions for the decider ({} given)",
                    pred.len()
                ))
            }
            Deal::Belief => {}
            _ if !pred.is_empty() => return Err("mtg spec: start.pred is for deal \"belief\" only".into()),
            _ => {}
        }
        let (tilt, clip, mix) = (float("tilt", 1.0)?, float("clip", zero_search::driver::BELIEF_CLIP)?, float("mix", 0.0)?);
        if deal != Deal::Belief && (o.contains_key("tilt") || o.contains_key("clip") || o.contains_key("mix")) {
            return Err("mtg spec: start.tilt / clip / mix are for deal \"belief\" only".into());
        }
        if !(tilt > 0.0 && tilt.is_finite()) || !(clip >= 1.0 && clip.is_finite()) || !(0.0..1.0).contains(&mix) {
            return Err(format!("mtg spec: start.tilt {tilt} (> 0), clip {clip} (>= 1), mix {mix} (in [0, 1))"));
        }
        let hold = names("hold")?;
        let mut exclude = names("exclude")?;
        exclude.dedup();
        if (!hold.is_empty() || !exclude.is_empty()) && !matches!(deal, Deal::Belief | Deal::Uniform) {
            return Err("mtg spec: start.hold / exclude need a dealt hand (deal \"belief\" or \"uniform\")".into());
        }
        if let Some(d) = hold.iter().find(|d| exclude.binary_search(d).is_ok()) {
            return Err(format!("mtg spec: start holds and excludes {}", db.def(*d).name));
        }
        let tries = uint("tries")?.unwrap_or(100_000).clamp(1, u32::MAX as u64) as u32;
        let forced = match o.get("forced") {
            None => Vec::new(),
            Some(x) => parse_forced(x, "start.forced")?,
        };
        let follow = match o.get("follow") {
            None | Some(Value::Null) => None,
            Some(f) => {
                let f = f.as_object().ok_or("mtg spec: start.follow must be an object")?;
                for k in f.keys() {
                    if !["rule", "land_min", "fetches", "bad", "good"].contains(&k.as_str()) {
                        return Err(format!("mtg spec: unknown start.follow key {k:?}"));
                    }
                }
                match f.get("rule").and_then(|r| r.as_str()) {
                    Some("bauble_fetch") => {}
                    _ => return Err("mtg spec: start.follow.rule must be \"bauble_fetch\"".into()),
                }
                let land_min = f.get("land_min").map_or(Some(4), |x| x.as_u64()).ok_or("mtg spec: start.follow.land_min must be an integer")?;
                let fetches = match f.get("fetches") {
                    None => ["Flooded Strand", "Misty Rainforest", "Scalding Tarn", "Polluted Delta"]
                        .iter()
                        .filter_map(|n| db.id(n))
                        .collect(),
                    Some(x) => x
                        .as_array()
                        .ok_or("mtg spec: start.follow.fetches must be a list of card names")?
                        .iter()
                        .map(|n| db.expect_id(n.as_str().ok_or("mtg spec: start.follow.fetches: card names are strings")?))
                        .collect::<Result<Vec<_>, _>>()?,
                };
                let line = |k: &str| f.get(k).map_or(Ok(Vec::new()), |x| parse_forced(x, &format!("start.follow.{k}")));
                Some(Follow { land_min: land_min as u32, fetches, bad: line("bad")?, good: line("good")? })
            }
        };
        let mut top: [Vec<DefId>; 2] = [Vec::new(), Vec::new()];
        if let Some(x) = o.get("top") {
            let a = x.as_array().filter(|a| a.len() == 2).ok_or("mtg spec: start.top must be [[P1's top cards], [P2's]]")?;
            for (p, l) in a.iter().enumerate() {
                top[p] = l
                    .as_array()
                    .ok_or("mtg spec: start.top: lists of card names")?
                    .iter()
                    .map(|n| db.expect_id(n.as_str().ok_or("mtg spec: start.top: card names are strings")?))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            if top.iter().any(|t| !t.is_empty()) && !matches!(deal, Deal::Belief | Deal::Uniform) {
                return Err("mtg spec: start.top needs a dealt world (deal \"belief\" or \"uniform\")".into());
            }
        }
        let top_known = o.get("top_known").map_or(Ok(false), |x| x.as_bool().ok_or("mtg spec: start.top_known must be a boolean"))?;
        Ok(Start { seed, actions, viewer, deal, deal_seed: uint("deal_seed")?, pred, tilt, clip, mix, hold, exclude, tries, forced, follow, top,
                   top_known })
    }
}

/// Where a lab game's follow rule is (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// No rule, or it ended.
    Off,
    /// Waiting for the decider's first decision after the line.
    Armed,
    /// Holding: the decider's known top card `card`, bad (crack) or not (hold).
    Active { card: CardIdx },
}

/// A lab game's state beside its `Game` (module docs): the forced line and the follow rule.
#[derive(Clone, Debug)]
pub struct Lab {
    /// The decider at the spot (the forced line's and the rule's player).
    pub player: PlayerId,
    pub forced: Vec<Forced>,
    /// The next forced item.
    pub pos: usize,
    /// The item at which the line broke (its decision did not offer it).
    pub broken: Option<usize>,
    pub follow: Option<Follow>,
    pub rule: Rule,
    /// The rule's start: the card it saw and whether it was bad then.
    pub seen: Option<(DefId, bool)>,
    /// Fetches activated by the rule.
    pub cracked: u32,
    /// The deal's rejected worlds (`hold` / `exclude`).
    pub rejected: u32,
}

/// What a forced item asks at the current decision.
enum Want {
    /// This action.
    Act(u16),
    /// The network's own choice.
    Free,
    /// Not among the options: the line breaks.
    Missing,
    /// A `resolve` item meets another question than priority: the network answers, the item stays.
    Wait,
    /// A label that names more than one option: an error (`Lab::ambiguity`).
    Ambiguous,
}

fn is_minor(g: &Game) -> bool {
    g.pending_decision().is_some_and(|d| matches!(d.kind, DecisionKind::PayMana | DecisionKind::PayPool))
}

/// Lands of `p` on the battlefield (controlled) and in hand.
fn lands(g: &Game, p: PlayerId) -> u32 {
    let bf = g.battlefield().iter().filter(|&&c| g.obj(c).controller == p && g.is_type(c, types::LAND)).count();
    let hand = g.player(p).hand.iter().filter(|&&c| g.db().def(g.ident(c)).types & types::LAND != 0).count();
    (bf + hand) as u32
}

/// `p`'s library's top card, if `p` knows it.
fn known_top(g: &Game, p: PlayerId) -> Option<CardIdx> {
    g.player(p).library.last().copied().filter(|&c| g.knows(p, c))
}

impl Lab {
    fn new(player: PlayerId, st: &Start) -> Lab {
        Lab {
            player,
            forced: st.forced.clone(),
            pos: 0,
            broken: None,
            follow: st.follow.clone(),
            rule: if st.follow.is_some() { Rule::Armed } else { Rule::Off },
            seen: None,
            cracked: 0,
            rejected: 0,
        }
    }

    /// The decider decides now (a non-minor decision of its own).
    fn deciding(&self, g: &Game) -> bool {
        g.to_move() == Some(self.player) && !is_minor(g)
    }

    fn want(&self, g: &Game) -> Want {
        let d = g.pending_decision().expect("the decider decides");
        match &self.forced[self.pos] {
            Forced::Any => Want::Free,
            Forced::Resolve if d.kind == DecisionKind::Priority => {
                d.options.iter().find(|o| o.choice == Choice::Pass).map_or(Want::Missing, |o| Want::Act(o.action))
            }
            Forced::Resolve => Want::Wait,
            Forced::Action(a) => d.options.iter().find(|o| o.action == *a).map_or(Want::Missing, |o| Want::Act(o.action)),
            Forced::Label(l) => {
                let mut m = d.options.iter().filter(|o| g.option_text(d.kind, o.choice) == *l);
                match (m.next(), m.next()) {
                    (Some(o), None) => Want::Act(o.action),
                    (Some(_), Some(_)) => Want::Ambiguous,
                    _ => Want::Missing,
                }
            }
        }
    }

    /// The error of a forced label that names two options of the decider's decision (a label is one
    /// option; give the action id): None when the line's next item is not one.
    pub(crate) fn ambiguity(&self, g: &Game) -> Option<String> {
        if !self.deciding(g) || self.pos >= self.forced.len() || !matches!(self.want(g), Want::Ambiguous) {
            return None;
        }
        let d = g.pending_decision().expect("the decider decides");
        let Forced::Label(l) = &self.forced[self.pos] else { return None };
        let ids: Vec<u16> = d.options.iter().filter(|o| g.option_text(d.kind, o.choice) == *l).map(|o| o.action).collect();
        Some(format!("start.forced item {}: the label {l:?} names {} options of P{}'s {:?} decision (actions {ids:?}): give an action id", self.pos, ids.len(), self.player, d.kind))
    }

    /// The rule's fetch activations among the pending options (Priority only): (action, fetch).
    fn cracks(&self, g: &Game) -> Vec<(u16, CardIdx)> {
        let (Some(f), Some(d)) = (&self.follow, g.pending_decision()) else { return Vec::new() };
        if d.kind != DecisionKind::Priority {
            return Vec::new();
        }
        d.options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Ability(i, _) if f.fetches.contains(&g.obj(i).def) => Some((o.action, i)),
                _ => None,
            })
            .collect()
    }

    fn bad(&self, g: &Game, card: CardIdx) -> bool {
        let f = self.follow.as_ref().expect("a rule");
        g.db().def(g.ident(card)).types & types::LAND != 0 && lands(g, self.player) >= f.land_min
    }

    /// `MtgAz::legal_actions`' filter (`out`: the game's legal actions).
    pub(crate) fn filter(&self, g: &Game, out: &mut Vec<u16>) {
        if !self.deciding(g) {
            return;
        }
        if self.pos < self.forced.len() {
            if let Want::Act(a) = self.want(g) {
                out.clear();
                out.push(a);
            }
            return;
        }
        let Rule::Active { card } = self.rule else { return };
        if known_top(g, self.player) != Some(card) {
            return;
        }
        let cracks = self.cracks(g);
        if cracks.is_empty() {
            return;
        }
        if self.bad(g, card) {
            out.clear();
            out.push(cracks[0].0);
        } else if g.stack().last().is_none_or(|s| s.controller == self.player) {
            // hold: no fetch activation of the decider's own accord (an empty stack, or its own
            // item on top); in answer to the opponent's spell or ability (a Daze's tax, a
            // Wasteland) the network decides
            out.retain(|a| !cracks.iter().any(|(c, _)| c == a));
        }
    }

    /// Before `a` is applied to `g`: the forced line moves on (or breaks), a rule crack counts.
    pub(crate) fn before_step(&mut self, g: &Game, a: u16) {
        if !self.deciding(g) {
            return;
        }
        if self.pos < self.forced.len() {
            let resolve = self.forced[self.pos] == Forced::Resolve;
            match self.want(g) {
                Want::Wait => {}
                Want::Act(x) if x == a && resolve => {}
                Want::Free => self.pos += 1,
                Want::Act(x) if x == a => self.pos += 1,
                _ => {
                    self.broken = Some(self.pos);
                    self.pos = self.forced.len();
                    self.rule = Rule::Off;
                }
            }
            return;
        }
        // a fetch activated while the rule holds (its crack, or an answer to a stack item that
        // targets it): the rule's work is done - one crack, not one per fetch
        if matches!(self.rule, Rule::Active { .. }) && self.cracks(g).iter().any(|(c, _)| *c == a) {
            self.cracked += 1;
            self.rule = Rule::Off;
        }
    }

    /// After a step (and at the start): the rule starts at the decider's first decision after the
    /// line, and ends when its card is no longer the decider's known top card.
    pub(crate) fn after_step(&mut self, g: &Game) {
        // a `resolve` item is done once the stack is empty at a decision, whoever's (the engine asks no
        // decision with "pass" its only option: the decider's empty-stack priority after its item resolved
        // may never come; ENGINE.md 76.1); the next item applies at the decider's next decision
        while self.pos < self.forced.len() && self.forced[self.pos] == Forced::Resolve && g.to_move().is_some() && g.stack().is_empty()
        {
            self.pos += 1;
        }
        match self.rule {
            Rule::Armed if self.pos >= self.forced.len() && self.deciding(g) => match known_top(g, self.player) {
                Some(card) => {
                    let bad = self.bad(g, card);
                    self.seen = Some((g.ident(card), bad));
                    self.rule = Rule::Active { card };
                    // a branch: the verdict's line continues the forced line from this decision
                    let f = self.follow.as_ref().expect("a rule");
                    let line = if bad { f.bad.clone() } else { f.good.clone() };
                    self.forced.extend(line);
                }
                None => self.rule = Rule::Off,
            },
            Rule::Active { card } if known_top(g, self.player) != Some(card) => self.rule = Rule::Off,
            _ => {}
        }
    }

    /// The `lab` field of a lab game's exported state (diagnostics, tests): the line, the rule and
    /// both libraries top first.
    pub(crate) fn view(&self, g: &Game) -> Value {
        let lib = |p: PlayerId| -> Vec<&str> { g.player(p).library.iter().rev().map(|&c| g.db().def(g.ident(c)).name.as_str()).collect() };
        let mut v = Map::new();
        v.insert("player".into(), json!(self.player));
        v.insert("forced_left".into(), json!(self.forced.len() - self.pos.min(self.forced.len())));
        v.insert("broken".into(), json!(self.broken));
        v.insert(
            "rule".into(),
            json!(match self.rule {
                Rule::Off if self.follow.is_none() => "none",
                Rule::Off => "off",
                Rule::Armed => "armed",
                Rule::Active { .. } => "active",
            }),
        );
        v.insert("seen".into(), json!(self.seen.map(|(d, _)| g.db().def(d).name.as_str())));
        v.insert("seen_bad".into(), json!(self.seen.map(|(_, b)| b)));
        v.insert("cracked".into(), json!(self.cracked));
        v.insert("rejected".into(), json!(self.rejected));
        v.insert("libraries".into(), json!([lib(0), lib(1)]));
        Value::Object(v)
    }
}

/// Does `p`'s hand hold at least `hold` (a sorted multiset) and nothing of `exclude`?
fn hand_fits(g: &Game, p: PlayerId, hold: &[DefId], exclude: &[DefId]) -> bool {
    let mut have: Vec<DefId> = g.player(p).hand.iter().map(|&c| g.ident(c)).collect();
    have.sort_unstable();
    if have.iter().any(|d| exclude.binary_search(d).is_ok()) {
        return false;
    }
    let mut i = 0;
    for &d in hold {
        while i < have.len() && have[i] < d {
            i += 1;
        }
        if i == have.len() || have[i] != d {
            return false;
        }
        i += 1;
    }
    true
}

impl MtgAz {
    /// Auto-step the single-option decisions (zero-core's transitions).
    fn auto_step(&mut self) -> Result<(), String> {
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for _ in 0..100_000 {
            if self.to_move().is_none() {
                return Ok(());
            }
            self.legal_actions(&mut legal);
            if legal.len() != 1 {
                return Ok(());
            }
            self.step(legal[0], &mut keys)?;
            keys.clear();
        }
        Err("start: 100,000 single-option decisions in a row".into())
    }

    /// A lab game (module docs): the spot of `spec.start`'s source game, dealt, with its line.
    pub fn from_start(spec: &MtgSpec, game_seed: u64) -> Result<MtgAz, String> {
        let st = spec.start.as_deref().ok_or("from_start: a spec without start")?;
        let src = MtgSpec { start: None, ..spec.clone() };
        let mut az = MtgAz::new(&src, st.seed)?;
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for (i, &a) in st.actions.iter().enumerate() {
            az.auto_step()?;
            if az.to_move().is_none() {
                return Err(format!("start: the source game is over before action {i} of {}", st.actions.len()));
            }
            az.legal_actions(&mut legal);
            if legal.binary_search(&a).is_err() {
                return Err(format!("start: action {i} ({a}) is not legal in the source game (legal: {legal:?})"));
            }
            az.step(a, &mut keys)?;
            keys.clear();
        }
        az.auto_step()?;
        let Some(decider) = az.g.to_move() else { return Err("start: the source game is over at the spot".into()) };
        let viewer = st.viewer.unwrap_or(decider);
        let deal_seed = st.deal_seed.unwrap_or(game_seed);
        let mut rng = zero_search::rng::Rng::new(zero_search::rng::split64(deal_seed ^ SALT));
        let mut rejected = 0u32;
        match st.deal {
            Deal::None => {}
            Deal::Actual => {
                let mut r = crate::rng::Rng::new(rng.next_u64());
                az.g.redeal_unknown_library_orders(&mut r);
            }
            Deal::Uniform | Deal::Belief => {
                let o = 1 - viewer;
                if !st.hold.is_empty() || !st.exclude.is_empty() || st.top.iter().any(|t| !t.is_empty()) {
                    // a pin no world can hold is an error at once (not `tries` rejections)
                    let mut could: Vec<DefId> = az.g.unknown_to(viewer, o).iter().map(|&c| az.g.ident(c)).collect();
                    could.extend(az.g.player(o).hand.iter().filter(|&&c| az.g.knows(viewer, c)).map(|&c| az.g.ident(c)));
                    could.sort_unstable();
                    let n_hand = az.g.player(o).hand.len();
                    if !hand_fits_pool(&could, &st.hold) || st.hold.len() > n_hand {
                        return Err(format!("start: no deal can give P{o}'s hand of {n_hand} the held cards {:?}", names(az.g.db(), &st.hold)));
                    }
                }
                let belief = Belief { pred: st.pred.clone(), tilt: st.tilt, clip: st.clip, tilts: Vec::new() };
                let base = az.clone();
                let mut dealt = None;
                for _ in 0..st.tries {
                    let mut w = base.clone();
                    let uniform = st.deal == Deal::Uniform || (st.mix > 0.0 && rng.unit() < st.mix);
                    if uniform {
                        w.determinize(viewer as usize, &mut rng);
                    } else {
                        w.determinize_belief(viewer as usize, &mut rng, &belief);
                    }
                    if hand_fits(&w.g, o, &st.hold, &st.exclude) && tops_fit(&w.g, &st.top) {
                        dealt = Some(w);
                        break;
                    }
                    rejected += 1;
                }
                az = dealt.ok_or_else(|| {
                    format!(
                        "start: no deal in {} tries gave P{o} a hand holding {:?} and none of {:?}",
                        st.tries,
                        names(base.g.db(), &st.hold),
                        names(base.g.db(), &st.exclude)
                    )
                })?;
                // the dealt world is the real game from here
                az.g.observer = None;
                if st.top_known {
                    for p in 0..2u8 {
                        let lib = az.g.player(p).library.clone();
                        for &c in lib.iter().rev().take(st.top[p as usize].len()) {
                            az.g.learn(p, c);
                        }
                    }
                }
            }
        }
        az.g.check_knowledge().map_err(|e| format!("start: the dealt world breaks a player's knowledge: {e}"))?;
        if az.m.sb.is_some() || az.m.format != super::Format::Bo1 {
            return Err("start: best-of-one games only".into());
        }
        let mut lab = Lab::new(decider, st);
        lab.rejected = rejected;
        lab.after_step(&az.g);
        if let Some(e) = lab.ambiguity(&az.g) {
            return Err(e);
        }
        az.lab = Some(Box::new(lab));
        Ok(az)
    }
}

/// Each player's library starts with its `top` cards (top first)?
fn tops_fit(g: &Game, top: &[Vec<DefId>; 2]) -> bool {
    (0..2u8).all(|p| {
        let lib = &g.player(p).library;
        top[p as usize].iter().enumerate().all(|(i, &d)| lib.len() > i && g.ident(lib[lib.len() - 1 - i]) == d)
    })
}

fn hand_fits_pool(pool: &[DefId], hold: &[DefId]) -> bool {
    let mut i = 0;
    for &d in hold {
        while i < pool.len() && pool[i] < d {
            i += 1;
        }
        if i == pool.len() || pool[i] != d {
            return false;
        }
        i += 1;
    }
    true
}

fn names(db: &CardDb, v: &[DefId]) -> Vec<String> {
    v.iter().map(|&d| db.def(d).name.clone()).collect()
}

/// The identity index of each of `names` in the belief head's layout (tests, analysis).
#[doc(hidden)]
pub fn identity_of(db: &'static CardDb, name: &str) -> Option<usize> {
    db.id(name).map(|d| identity::identities(db)[d as usize] as usize)
}
