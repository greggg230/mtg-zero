//! The zero-core game `mtg_eoe` (ENGINE.md 105): Edge of Eternities sealed, the `mtg` game on the
//! EoE card table under a name of its own. Its network reads the EoE identity space
//! (`identity::Space::Eoe`: the set's cards and tokens in set order), not the cube's, so it is a
//! separate game for zero-core: the game package gives it its own token layout (named `eoe`,
//! `mtgz/az/layout_eoe.json`) and the encoding guard refuses a cube or Delver network (game
//! `mtg`) on its encodings and an EoE network on `mtg`'s. Everything else is `MtgAz`'s: the
//! rules, the match, the encoding (the same v6 floats, the identities read from the EoE table),
//! the auxiliary targets, the bots.
//!
//! Its spec is an `mtg` spec with the `"sealed"` key (ENGINE.md 101.3): a spec on another card
//! table is refused, as `mtg` refuses a sealed spec (`MtgSpec::check_game`).
//!
//! With the game log on (`game_log::set`, ENGINE.md 107.1) each live game carries its record: a
//! search's worlds drop it (`determinize`, `determinize_with`), the live game writes it at the step
//! that ends it. Nothing else of the game reads it.
use std::sync::Arc;

use zero_search::game::{Belief, Bot, SearchGame, MAX_SEATS};
use zero_search::rng::Rng;

use super::{game_log, MtgAz, MtgSpec};

/// An `mtg` game on the EoE table, as the zero-core game `mtg_eoe`; with the game log on, the live
/// game's record (ENGINE.md 107.1).
#[derive(Clone)]
pub struct MtgEoeAz(pub MtgAz, Option<Arc<game_log::Record>>);

impl MtgEoeAz {
    /// Does this game carry a game-log record (a live game while the log is on, ENGINE.md 107.1)?
    pub fn logging(&self) -> bool {
        self.1.is_some()
    }

    /// A world of the game: the record dropped, `viewer`'s search marked.
    fn world(&mut self, viewer: usize) {
        if let Some(r) = self.1.take() {
            r.searched(viewer);
        }
    }
}

/// One of `MtgAz`'s bots playing `mtg_eoe` (the same game underneath).
struct EoeBot(Box<dyn Bot<MtgAz>>);

impl Bot<MtgEoeAz> for EoeBot {
    fn choose(&mut self, g: &MtgEoeAz, legal: &[u16]) -> u16 {
        self.0.choose(&g.0, legal)
    }
}

impl SearchGame for MtgEoeAz {
    type Spec = MtgSpec;
    const NAME: &'static str = "mtg_eoe";
    const NUM_ACTIONS: usize = <MtgAz as SearchGame>::NUM_ACTIONS;
    const NUM_FEATURES: usize = <MtgAz as SearchGame>::NUM_FEATURES;
    const MIN_PLAYERS: usize = <MtgAz as SearchGame>::MIN_PLAYERS;
    const MAX_PLAYERS: usize = <MtgAz as SearchGame>::MAX_PLAYERS;
    const AUX: &'static [(&'static str, usize)] = <MtgAz as SearchGame>::AUX;

    fn parse_spec(json: &str) -> Result<MtgSpec, String> {
        let s = MtgSpec::parse(json)?;
        s.check_game(Self::NAME)?;
        Ok(s)
    }

    fn new_game(spec: &MtgSpec, players: usize, seed: u64) -> Result<Self, String> {
        let g = <MtgAz as SearchGame>::new_game(spec, players, seed)?;
        // (the record of the game log, ENGINE.md 107.1: None while it is off; the match seed as `new_game`'s)
        Ok(MtgEoeAz(g, game_log::Record::start(spec, spec.seed.unwrap_or(seed))))
    }

    fn num_players(&self) -> usize {
        self.0.num_players()
    }

    fn to_move(&self) -> Option<usize> {
        self.0.to_move()
    }

    fn aborted(&self) -> bool {
        self.0.aborted()
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        self.0.legal_actions(out)
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        self.0.step(a, chance)?;
        // (the game log, ENGINE.md 107.1: the live game's line at the step that ends it)
        if self.1.is_some() && (self.0.to_move().is_none() || self.0.aborted()) {
            if let Some(r) = self.1.take() {
                r.finish(&self.0);
            }
        }
        Ok(())
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.0.force_chance(forced)
    }

    fn result(&self) -> [f32; MAX_SEATS] {
        self.0.result()
    }

    fn margin(&self, seat: usize) -> f32 {
        self.0.margin(seat)
    }

    fn score(&self, seat: usize) -> f32 {
        self.0.score(seat)
    }

    fn determinize(&mut self, viewer: usize, rng: &mut Rng) {
        self.world(viewer);
        self.0.determinize(viewer, rng)
    }

    fn encode(&self, viewer: usize, out: &mut [f32]) {
        self.0.encode(viewer, out)
    }

    fn rounds(&self) -> u32 {
        self.0.rounds()
    }

    fn minor_decision(&self) -> bool {
        self.0.minor_decision()
    }

    fn export_state(&self) -> String {
        self.0.export_state()
    }

    fn active_seat(&self) -> Option<usize> {
        self.0.active_seat()
    }

    fn bot_names() -> &'static [&'static str] {
        <MtgAz as SearchGame>::bot_names()
    }

    fn make_bot(spec: &MtgSpec, name: &str, seed: u64) -> Option<Box<dyn Bot<Self>>> {
        <MtgAz as SearchGame>::make_bot(spec, name, seed).map(|b| Box::new(EoeBot(b)) as Box<dyn Bot<Self>>)
    }

    fn aux_target(&self, viewer: usize, out: &mut [f32]) {
        self.0.aux_target(viewer, out)
    }

    fn determinize_with(&mut self, viewer: usize, rng: &mut Rng, belief: &Belief) {
        self.world(viewer);
        self.0.determinize_with(viewer, rng, belief)
    }
}
