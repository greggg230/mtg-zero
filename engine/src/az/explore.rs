//! Habit tags for zero-core's self-play exploration (ENGINE.md 75, zero-core
//! docs/search.md 3.9): the Delver line's frozen habits (73.5's fingerprints)
//! as tags on the options of the pending decision, so that an exploring
//! self-play seat plays the alternative now and then. A tag reads only what
//! the decider sees (its hand, the public zones, the decision's options) and
//! changes nothing: rules, legal actions, encoding and digests are the same
//! with exploration off or on (the driver only reads the tags of its live
//! game at an exploring seat's decision).
//!
//! | tag | the habit | window |
//! |---|---|---|
//! | `fetch_crack` | activating a fetchland | the rest of the turn and the next |
//! | `daze_tapout` | casting a spell in its own main phase (turn >= 3, empty stack) that leaves it no untapped land while the opponent controls an Island and holds a card | the rest of the turn |
//! | `fow_pitch_fow` | exiling Force of Will to pay its own Force of Will while another blue card could be exiled | the decision |
//! | `delve_skip_spell` | a delve pick of a card that is not an instant or sorcery while one is still offered | the rest of the turn (the cast's picks) |
//! | `volcanic_fetch` | a fetch of its own, in its own first two turns, finding Volcanic Island while the basic Island is offered | the decision |
//! | `keep_one_lander` | keeping an opening seven with at most one land | the decision |
//!
//! The definitions follow `tools/delver_fingerprints.py` (73.5) where the
//! decision can see the same; where the fingerprint looks ahead (the
//! opponent's Daze in hand, the rest of the turn's spells) the tag stays with
//! what is visible: an Island and a card in the opponent's hand, the basic
//! offered.
use super::MtgAz;
use crate::db::types;
use crate::decision::{Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, StackKind, Step};
use crate::ids::{CardIdx, PlayerId};
use zero_search::game::ExploreTag;

/// The tags in id order (id k + 1 = entry k) and their defaults: the window
/// in turns (zero-core `ExploreTag::window`: 0 = the decision only, 1 = the
/// rest of the turn, 2 = ... and the next turn) and the chance per
/// opportunity (ENGINE.md 75.2: fetch cracks come ~5 a game, a one-lander's
/// keep once in four).
pub const TAGS: &[ExploreTag] = &[
    ExploreTag { name: "fetch_crack", window: 2, p: 0.25 },
    ExploreTag { name: "daze_tapout", window: 1, p: 0.5 },
    ExploreTag { name: "fow_pitch_fow", window: 0, p: 1.0 },
    ExploreTag { name: "delve_skip_spell", window: 1, p: 0.5 },
    ExploreTag { name: "volcanic_fetch", window: 0, p: 0.5 },
    ExploreTag { name: "keep_one_lander", window: 0, p: 1.0 },
];

pub const FETCH_CRACK: u8 = 1;
pub const DAZE_TAPOUT: u8 = 2;
pub const FOW_PITCH_FOW: u8 = 3;
pub const DELVE_SKIP_SPELL: u8 = 4;
pub const VOLCANIC_FETCH: u8 = 5;
pub const KEEP_ONE_LANDER: u8 = 6;

/// The fetchlands (a land that sacrifices itself to find a land): the
/// fingerprint's four and their siblings.
pub const FETCHES: &[&str] = &[
    "Flooded Strand",
    "Polluted Delta",
    "Bloodstained Mire",
    "Wooded Foothills",
    "Windswept Heath",
    "Marsh Flats",
    "Scalding Tarn",
    "Verdant Catacombs",
    "Arid Mesa",
    "Misty Rainforest",
    "Prismatic Vista",
];

const FOW: &str = "Force of Will";

fn is_fetch(g: &Game, c: CardIdx) -> bool {
    FETCHES.contains(&g.name(c))
}

/// The habit tag of each option of the match's pending decision (`out`: one
/// per `legal` action, zeroed by the caller). None between the games of a
/// match (sideboarding).
pub fn tags_az(az: &MtgAz, legal: &[u16], out: &mut [u8]) {
    if az.m.sb.is_none() {
        tags(&az.g, legal, out);
    }
}

/// The habit tag of each option of `g`'s pending decision (`legal`: its
/// actions in order; `out` zeroed by the caller).
pub fn tags(g: &Game, legal: &[u16], out: &mut [u8]) {
    let Some(d) = g.pending_decision() else { return };
    let p = d.player;
    let choice = |k: usize| d.choice_of(legal[k]);
    match d.kind {
        DecisionKind::Priority => {
            let tapout = daze_live(g, p);
            for (k, t) in out.iter_mut().enumerate() {
                match choice(k) {
                    Some(Choice::Ability(c, _)) if is_fetch(g, c) => *t = FETCH_CRACK,
                    Some(Choice::Cast(c)) if tapout && taps_out(g, p, c) => *t = DAZE_TAPOUT,
                    _ => {}
                }
            }
        }
        DecisionKind::ExileCost => {
            // Force of Will's alternative cost, paid for the caster's own Force
            // of Will (the spell whose costs are being paid)
            let Some(Task::PayCosts { id, .. }) = g.agenda.last() else { return };
            let Some(item) = g.stack().iter().find(|s| s.id == *id) else { return };
            let StackKind::Spell { card } = item.kind else { return };
            if g.name(card) != FOW || item.controller != p {
                return;
            }
            let fow = |k: usize| matches!(choice(k), Some(Choice::Card(c)) if g.name(c) == FOW);
            let other = (0..legal.len()).any(|k| matches!(choice(k), Some(Choice::Card(c)) if g.name(c) != FOW));
            if other {
                for (k, t) in out.iter_mut().enumerate() {
                    if fow(k) {
                        *t = FOW_PITCH_FOW;
                    }
                }
            }
        }
        DecisionKind::ChooseGraveyardCard => {
            if !matches!(g.agenda.last(), Some(Task::Delve { .. })) {
                return;
            }
            let spell = |c: CardIdx| g.is_type(c, types::INSTANT | types::SORCERY);
            let offered = (0..legal.len()).any(|k| matches!(choice(k), Some(Choice::Card(c)) if spell(c)));
            if offered {
                for (k, t) in out.iter_mut().enumerate() {
                    if matches!(choice(k), Some(Choice::Card(c)) if !spell(c)) {
                        *t = DELVE_SKIP_SPELL;
                    }
                }
            }
        }
        DecisionKind::SearchLibrary => {
            // its own fetch's search in its own first two turns (global turns 1-4)
            let turn = g.turn();
            if turn.active != p || turn.number > 4 {
                return;
            }
            let Some(top) = g.stack().last() else { return };
            let StackKind::Ability { source, .. } = top.kind else { return };
            if top.controller != p || !is_fetch(g, source.idx) {
                return;
            }
            let db = g.db();
            let def = |k: usize| match choice(k) {
                Some(Choice::Def(x)) => Some(db.def(x).name.as_str()),
                _ => None,
            };
            if (0..legal.len()).any(|k| def(k) == Some("Island")) {
                for (k, t) in out.iter_mut().enumerate() {
                    if def(k) == Some("Volcanic Island") {
                        *t = VOLCANIC_FETCH;
                    }
                }
            }
        }
        DecisionKind::Mulligan => {
            let pl = g.player(p);
            if pl.mulligans == 0 && pl.hand.len() == 7 && pl.hand.iter().filter(|&&c| g.is_type(c, types::LAND)).count() <= 1 {
                for (k, t) in out.iter_mut().enumerate() {
                    if choice(k) == Some(Choice::Pass) {
                        *t = KEEP_ONE_LANDER;
                    }
                }
            }
        }
        _ => {}
    }
}

/// At `p`'s priority: its own main phase with an empty stack from turn 3 on
/// (73.5), the opponent controlling an Island (Daze's alternative cost) and
/// holding a card.
fn daze_live(g: &Game, p: PlayerId) -> bool {
    let t = g.turn();
    if t.active != p || !matches!(t.step, Step::Main1 | Step::Main2) || !g.stack().is_empty() || t.number < 3 {
        return false;
    }
    let o = 1 - p;
    if g.player(o).hand.is_empty() {
        return false;
    }
    let Some(island) = g.db().subtype_id("Island") else { return false };
    g.permanents_of(o).any(|c| g.is_type(c, types::LAND) && g.has_subtype(c, island))
}

/// Casting `card` leaves `p` no untapped land: its cost (the static
/// increases, delve at most) at least the untapped lands and the pool (an
/// uncracked fetch counts: it finds an untapped land). A free spell (no mana)
/// never does.
fn taps_out(g: &Game, p: PlayerId, card: CardIdx) -> bool {
    let cost = g.delve_reduced(card, p, g.spell_cost(card, p), 0).total() as u32;
    let lands = g.permanents_of(p).filter(|&c| g.is_type(c, types::LAND) && !g.obj(c).tapped).count() as u32;
    cost > 0 && cost >= lands + g.player(p).pool.total()
}
