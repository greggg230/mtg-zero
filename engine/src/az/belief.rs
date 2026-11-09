//! Belief-weighted determinization (ENGINE.md 69; zero-core docs/search.md 3.8): the search's
//! worlds dealt from the network's prediction of the opponent's hand (the belief head's `belief`
//! target, ENGINE.md 66) instead of uniformly.
//!
//! * **The weights** (`deal_weights`), per card identity k of the decider's opponent's hidden pool:
//!   e_k = n p_k (n = their hand size, p the head's distribution over the hand's cards), known_k =
//!   their hand cards of identity k the decider knows exactly, m = n - those (the hand's unknown
//!   slots), h_k / H = identity k's share of their cards hidden from the decider (hand, library,
//!   face down); w_k = `zero_search::belief_weight`(e_k - known_k, m h_k / H, tilt, clip) =
//!   clip(((e_k - known_k)^+ / (m h_k / H))^tilt, 1/R, R): the ratio of the belief's expected count
//!   of k among the unknown hand cards to the uniform deal's, tilted and clipped. Everything it
//!   reads is the decider's: its features (the head's input), the public hand size, what it knows,
//!   and the hidden pool's multiset - public in best-of-one (the decklist less what is seen), the
//!   sampled deck's in best-of-three (sampled first, from what the decider knows: ENGINE.md 36.11).
//! * **The deal** (`Game::determinize_weighted`): `determinize`'s, the hand's unknown slots drawn
//!   one at a time from the hidden pool with probability proportional to w x copies left.
use super::{identity, MtgAz};
use crate::game::Game;
use zero_search::game::{belief_weight, Belief, SearchGame};

/// The `belief` segment of a prediction of `MtgAz::AUX` (the head's distribution over the
/// opponent's hand's cards by identity, `identity::D` floats).
pub fn belief_segment(pred: &[f32]) -> &[f32] {
    let mut off = 0;
    for &(name, w) in MtgAz::AUX {
        if name == "belief" {
            return &pred[off..off + w];
        }
        off += w;
    }
    unreachable!("MtgAz::AUX has the belief target")
}

/// The weights per card identity (`identity::D`; 1 for identities not in the hidden pool) of the
/// belief-weighted deal of `viewer`'s opponent's hand in `g` (module docs): `p` = the head's
/// distribution (`belief_segment`), `tilt` > 0, `clip` >= 1.
pub fn deal_weights(g: &Game, viewer: u8, p: &[f32], tilt: f64, clip: f64) -> Vec<f64> {
    let ids = identity::identities(g.db());
    let o = 1 - viewer;
    let hand = &g.player(o).hand;
    let n = hand.len();
    let mut known = vec![0u32; identity::D];
    let mut m = 0usize;
    for &c in hand {
        if g.knows(viewer, c) {
            known[ids[g.ident(c) as usize] as usize] += 1;
        } else {
            m += 1;
        }
    }
    let mut h = vec![0u32; identity::D];
    let pool = g.unknown_to(viewer, o);
    for &c in &pool {
        h[ids[g.ident(c) as usize] as usize] += 1;
    }
    let mut w = vec![1.0; identity::D];
    if m == 0 || pool.is_empty() {
        return w;
    }
    let big_h = pool.len() as f64;
    for k in 0..identity::D {
        if h[k] > 0 {
            let e = n as f64 * p[k] as f64 - known[k] as f64;
            let u = m as f64 * h[k] as f64 / big_h;
            w[k] = belief_weight(e, u, tilt, clip);
        }
    }
    w
}

impl MtgAz {
    /// `SearchGame::determinize_with` returning the weights it dealt by (None: between the games of
    /// a match, a uniform `determinize`; tests and analysis, ENGINE.md 69.4).
    #[doc(hidden)]
    pub fn determinize_belief(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng, belief: &Belief) -> Option<Vec<f64>> {
        if self.m.sb.is_some() || belief.tilt <= 0.0 {
            self.determinize(viewer, rng);
            return None;
        }
        // as `determinize`: one draw for the deal's own generator, the hidden pool first
        // (best-of-three), then the game's deal, then the match's generator
        let mut r = crate::rng::Rng::new(rng.next_u64());
        if self.m.format == super::Format::Bo3 && !self.m.over {
            self.sample_hidden_pool(viewer as u8, &mut r);
        }
        // (Hidden decklists, ENGINE.md 102: the opponent's deck first, as `determinize`; the hand's
        // weights then read the dealt world's hidden pool, as best-of-three's do.)
        if self.hidden_decklists() {
            self.sample_hidden_deck(viewer as u8, &mut r, None);
        }
        let w = deal_weights(&self.g, viewer as u8, belief_segment(&belief.pred), belief.tilt, belief.clip);
        let ids = identity::identities(self.g.db());
        self.g.determinize_weighted(viewer as u8, &mut r, &|d| w[ids[d as usize] as usize]);
        self.m.rng = crate::rng::Rng::new(r.next_u64());
        Some(w)
    }

    /// `sample_hidden_pool` for tests (ENGINE.md 69.4): the opponent's best-of-three pool and deck
    /// sampled from what `viewer` knows, the game's cards dealt to it.
    #[doc(hidden)]
    pub fn sample_hidden_pool_for(&mut self, viewer: u8, rng: &mut crate::rng::Rng) {
        self.sample_hidden_pool(viewer, rng);
    }
}
