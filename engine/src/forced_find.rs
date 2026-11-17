//! The forced find (ENGINE.md 180): a self-play training aid, never a rule of the game. With the `mtg` spec's
//! `"forced_find": {"spell": S, "card": C, "p": p, "unless_in_hand": true}` (a constructed best-of-one setting,
//! `az::MtgSpec`), when a spell S resolves and searches its controller's library and graveyard for a pile
//! (`DdA::PileFromGraveyard` then `DdA::PileFromLibrary`: Doomsday's "search your library and graveyard for five cards",
//! ENGINE.md 124.1), with probability p its pile must hold a card C - unless a C is in that player's hand
//! (`unless_in_hand`) or no C can be found (none in the library or the graveyard). The network makes every other pick
//! and the order; the constraint touches only the pile's LAST pick: when it comes and no C was chosen yet, its one
//! legal option is a C (in the library: the `SearchLibrary` option of C's definition; in the graveyard: the
//! `ChooseGraveyardCard` option of that card). A pile that already holds a C is never touched, so whether the network
//! chose the card itself stays visible (`FfClass`).
//!
//! * **Which resolutions** (`Game::ff_draw`): the draw of the game's `k`-th resolution of S (0-based, counted by
//!   `FfState::recs`) is `split64(seed ^ SALT ^ split64(k + 1)) >> 11 < threshold`, `seed` the game's own seed
//!   (`Game::empty`) and `threshold` = p x 2^53 (exact for p 0 and 1; no float in the draw). A function of the game's
//!   seed and a count of public resolutions: the same in every world `determinize` deals, of either viewer (the seed is
//!   not reseeded, the count is the game's), so a world never disagrees with the real game about a forced pick.
//! * **No policy target.** The forced pick has one option, so the engine answers it itself (`GameSpec::auto_single`,
//!   `flow.rs` `ask_options`): no decision reaches zero-core, which searches and records nothing for it (zero-core's
//!   rows are its decisions). Its answer is keyed `KEY_AUTO` like every forced answer (the other player's worlds see a
//!   searched card's place masked, ENGINE.md 161). Nor from the resolution's other decisions (ENGINE.md 180.12): every
//!   pick and order decision of a resolution the forcing applies to is searched and played, and recorded without its
//!   policy target (`Game::ff_value_only`, zero-core's value-only decision) - its searches know the Oracle is coming.
//! * **Not encoded.** Nothing of the spec's option or of `FfState` is read by an encoding: neither player is told that a
//!   resolution was forced (the user's decision: a warm-up mismatch, ENGINE.md 180).
//! * Without the option (`GameSpec::forced_find` None) nothing here runs: `FfState` stays empty.

use crate::ids::{CardIdx, DefId, PlayerId};

/// The spec's forced find (module docs): the spell, the card, the share of resolutions as a threshold over 53 bits,
/// and whether a card in the hand exempts a resolution.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ForcedFind {
    pub spell: DefId,
    pub card: DefId,
    /// p x 2^53, p in [0, 1]: a resolution is drawn forced when its 53-bit draw is below it.
    pub threshold: u64,
    pub unless_in_hand: bool,
}

/// 2^53: the draw's range (`ForcedFind::threshold` at p 1).
pub const ONE: u64 = 1 << 53;

const SALT: u64 = 0xF0_4CED_F14D_0180;

impl ForcedFind {
    /// The setting with share `p` (in [0, 1]: rounded to the nearest 2^-53).
    pub fn new(spell: DefId, card: DefId, p: f64, unless_in_hand: bool) -> ForcedFind {
        assert!((0.0..=1.0).contains(&p), "forced_find p {p} outside [0, 1]");
        ForcedFind { spell, card, threshold: (p * ONE as f64).round() as u64, unless_in_hand }
    }

    /// The share p.
    pub fn p(&self) -> f64 {
        self.threshold as f64 / ONE as f64
    }
}

/// How one resolution's pile met the card (module docs; in this order of precedence).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FfClass {
    /// The pile holds the card and the player chose it.
    Voluntary,
    /// The pile holds the card because its last pick was forced.
    Forced,
    /// No card was forced because the player had one in hand (`unless_in_hand`).
    InHand,
    /// No card could be found (none in the library or the graveyard), and the pile has none.
    NotFindable,
    /// One could be found, the draw did not force it, and the player did not choose it.
    Missed,
}

impl FfClass {
    pub fn name(self) -> &'static str {
        match self {
            FfClass::Voluntary => "voluntary",
            FfClass::Forced => "forced",
            FfClass::InHand => "in_hand",
            FfClass::NotFindable => "not_findable",
            FfClass::Missed => "missed",
        }
    }
}

/// One resolution of the spell (module docs): its controller, the draw, the class and the pile's definitions (the
/// cards it found, in definition order).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FfRec {
    pub player: PlayerId,
    pub drawn: bool,
    pub class: FfClass,
    pub pile: Vec<DefId>,
}

/// A game's forced-find state (`Game::ff`): its seed (from the game's), the resolutions so far and whether the
/// resolution in progress had its last pick forced. Empty in every game without the option.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct FfState {
    pub(crate) seed: u64,
    pub(crate) recs: Vec<FfRec>,
    pub(crate) forced_now: bool,
}

impl FfState {
    pub(crate) fn new(game_seed: u64) -> FfState {
        FfState { seed: crate::rng::split64(game_seed ^ SALT), recs: Vec::new(), forced_now: false }
    }
}

/// Where the forced card is to be found for the pile's last pick.
pub(crate) enum Must {
    /// In the library: its definition.
    Library,
    /// In the graveyard: this card.
    Graveyard(CardIdx),
}

impl crate::game::Game {
    /// The spec's forced find when the frame's resolution is the spell's (`source_def`).
    pub(crate) fn ff_of(&self, source_def: DefId) -> Option<ForcedFind> {
        self.spec.forced_find.filter(|ff| ff.spell == source_def)
    }

    /// The draw of the next (or running) resolution of the spell: forced or not (module docs). None without the option.
    pub fn ff_draw(&self) -> Option<bool> {
        let ff = self.spec.forced_find?;
        let k = self.ff.recs.len() as u64;
        let h = crate::rng::split64(self.ff.seed ^ crate::rng::split64(k + 1));
        Some((h >> 11) < ff.threshold)
    }

    /// The play-by-play's line of the newest resolution (`dd_pile_found`, once the cards moved: the step's last lines,
    /// which `tools/combo_stats.py` reads; the log is never encoded, and off in self-play).
    pub(crate) fn ff_note(&mut self) {
        if let Some(r) = self.ff.recs.last() {
            let (p, class, drawn) = (r.player, r.class.name(), r.drawn);
            self.note(|_| format!("P{p}'s forced find: {class} (drawn {drawn})"));
        }
    }

    /// The resolutions of the spell so far (ENGINE.md 180), oldest first; empty without the option.
    pub fn forced_finds(&self) -> &[FfRec] {
        &self.ff.recs
    }

    /// Whether `p` holds a card of definition `d` in hand.
    fn ff_in_hand(&self, p: PlayerId, d: DefId) -> bool {
        self.players[p as usize].hand.iter().any(|&c| self.objs[c.i()].def == d)
    }

    /// The pick to force now, if the pile's next pick is its last (`last`), the draw forces this resolution, no card of
    /// the definition is among the picks so far (`picked`: the graveyard's cards, `picked_defs`: the library's
    /// definitions), none is in the hand (with `unless_in_hand`) and one can be found: in the library (preferred while
    /// `library_first`), else an unpicked one in the graveyard (the first by position).
    pub(crate) fn ff_must(&self, ff: ForcedFind, p: PlayerId, last: bool, picked: &[CardIdx], picked_defs: &[DefId], library_first: bool) -> Option<Must> {
        if !last || self.ff_draw() != Some(true) {
            return None;
        }
        let d = ff.card;
        if picked_defs.contains(&d) || picked.iter().any(|&c| self.objs[c.i()].def == d) {
            return None;
        }
        if ff.unless_in_hand && self.ff_in_hand(p, d) {
            return None;
        }
        let in_lib = self.players[p as usize].library.iter().any(|&c| self.objs[c.i()].def == d);
        let in_gy = self.players[p as usize].graveyard.iter().copied().find(|&c| self.objs[c.i()].def == d && !picked.contains(&c));
        match (in_lib, in_gy) {
            (true, _) if library_first => Some(Must::Library),
            (_, Some(c)) => Some(Must::Graveyard(c)),
            (true, None) => Some(Must::Library),
            (false, None) => None,
        }
    }

    /// The end of the spell's pile search (`dd_pile_found`, before any card moves): the resolution's record - its class
    /// from the picks, the hand and what could be found (module docs) - and the resolution count moves on.
    pub(crate) fn ff_record(&mut self, ff: ForcedFind, p: PlayerId, lib_found: &[CardIdx], gy_found: &[CardIdx]) {
        let drawn = self.ff_draw() == Some(true);
        let d = ff.card;
        let mut pile: Vec<DefId> = lib_found.iter().chain(gy_found).map(|&c| self.objs[c.i()].def).collect();
        pile.sort_unstable();
        let holds = pile.contains(&d);
        let forced = std::mem::take(&mut self.ff.forced_now);
        assert!(!forced || (drawn && holds), "a forced pick without the draw, or a forced pile without the card");
        let class = if holds {
            if forced {
                FfClass::Forced
            } else {
                FfClass::Voluntary
            }
        } else if ff.unless_in_hand && self.ff_in_hand(p, d) {
            FfClass::InHand
        } else {
            let pl = &self.players[p as usize];
            let findable = pl.library.iter().chain(&pl.graveyard).any(|&c| self.objs[c.i()].def == d);
            // (A card that could be found is forced when drawn: a drawn resolution never misses it.)
            assert!(!(findable && drawn), "a drawn resolution left out a card it could find");
            if findable {
                FfClass::Missed
            } else {
                FfClass::NotFindable
            }
        };
        self.ff.recs.push(FfRec { player: p, drawn, class, pile });
    }

    /// Whether the pending decision is one of a **drawn** resolution's pile or order decisions (ENGINE.md 180.12):
    /// zero-core then records it without its policy target (`SearchGame::value_only_decision`, its docs/search.md
    /// 3.20) - searched and played as any decision, its row a value-only row of the decider. "Drawn" here is the
    /// forcing applying to the resolution: the draw forces it (`ff_draw`), no card of the spec's is in the caster's
    /// hand (with `unless_in_hand`) and one can be found (in the library or the graveyard) - exactly the resolutions
    /// whose pile is sure to hold the card, which every search at their picks knows (the draw is the same in every
    /// world). Every decision of such a resolution: each pick of the graveyard part (`DdA::PileFromGraveyard`) and the
    /// library part (`DdA::PileFromLibrary`, before `ff_record`: the draw is the running resolution's), and each pick
    /// of the order (`Op::PileTo`, after it: the newest record's - drawn, its pile holding the card, the hand still
    /// without one). A function of the state the decider sees: the same in every world, never encoded. False without
    /// the option, with no decision pending, and outside the spell's resolution.
    pub fn ff_value_only(&self) -> bool {
        use crate::doomsday_a::DdA;
        use crate::program::{Ext, Op};
        let Some(ff) = self.spec.forced_find else { return false };
        if self.pending.is_none() {
            return false;
        }
        // (The asking task is the agenda's last while a decision is pending: `ask_options`.)
        let Some(crate::flow::Task::Run(f)) = self.agenda.last() else { return false };
        if f.source_def != ff.spell {
            return false;
        }
        let p = f.controller;
        let exempt = ff.unless_in_hand && self.ff_in_hand(p, ff.card);
        match self.db.ability(f.ability).program.get(f.pc as usize) {
            Some(Op::Ext(Ext::DdA(DdA::PileFromGraveyard { .. } | DdA::PileFromLibrary { .. }))) => {
                let pl = &self.players[p as usize];
                let findable = pl.library.iter().chain(&pl.graveyard).any(|&c| self.objs[c.i()].def == ff.card);
                self.ff_draw() == Some(true) && !exempt && findable
            }
            Some(Op::PileTo { .. }) => {
                !exempt && self.ff.recs.last().is_some_and(|r| r.player == p && r.drawn && matches!(r.class, FfClass::Forced | FfClass::Voluntary))
            }
            _ => false,
        }
    }
}

impl crate::game::Game {
    /// The spec's forced find (ENGINE.md 180), if any.
    pub fn forced_find(&self) -> Option<ForcedFind> {
        self.spec.forced_find
    }

    /// The same state as a game without the forced find: the setting and its state dropped (tests and the fuzz: an
    /// encoding of either is the same, ENGINE.md 180).
    pub fn strip_forced_find(&mut self) {
        self.spec.forced_find = None;
        self.ff = FfState::default();
    }
}
