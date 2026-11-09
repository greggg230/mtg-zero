//! Hidden decklists (ENGINE.md 102): best-of-one where neither player knows the other's deck
//! (EoE sealed: nobody sees the opponent's pool). `MtgAz::determinize(viewer)` then deals the
//! opponent's whole unseen deck from a prior, conditioned on what the viewer knows:
//!
//! * **K1** the deck's size n (public); **K2** the multiset R of the opponent's cards the viewer
//!   has seen - their deck cards it knows exactly (every public card) and its clouds' identities
//!   (`sample::knowledge`'s `required`, ENGINE.md 36.11; with hidden decklists a cloud over the
//!   whole hand and library is kept, so R counts every card seen and lost track of); every card
//!   it knows stays where it is, every cloud's identities in its domain; **K3** every card in the
//!   prior's support, a revealed companion's condition kept; the pinned cards of a payment in
//!   progress and the viewer's own cards untouched.
//! * The deal (`MtgAz::sample_hidden_deck`): the prior samples a deck D of n cards holding R
//!   ([`DeckPrior::sample`]); D less the cards the viewer knows is dealt to the opponent's cards
//!   it does not know (`Game::redeal`), their sideboard becomes the sampled one, the match's pool
//!   and deck of theirs the sampled ones; `Game::determinize` follows.
//! * The prior is pluggable ([`DeckPrior`], one per seat in `MatchState::deck_priors`). The stub
//!   until S2's sealed builder lands is [`TablePrior`]: FUZZ_POOL=eoe's random deck generator
//!   conditioned on R. The deck reader's tilt ([`DeckWant::tilt`], [`deck_weights`]) multiplies
//!   the prior's probability of each card it adds beyond R.
//!
//! What is exact (K1-K3: every world a legal state holding all the viewer knows) and what is the
//! prior's modelling choice (the distribution over the rest of the deck): ENGINE.md 102.1.

use std::sync::{Arc, Mutex, OnceLock};

use crate::db::{types, CardDb};
use crate::ids::{opp, DefId, PlayerId};
use crate::rng::Rng;

use super::matches::MatchPool;
use super::{identity, sample, MtgAz};

/// What a deal asks a prior for (ENGINE.md 102.1).
pub struct DeckWant<'a> {
    pub db: &'static CardDb,
    /// The deck's size (public: the opponent's starting deck).
    pub n: usize,
    /// The cards the deck must hold (card definitions, sorted, copies counted): R.
    pub shown: &'a [DefId],
    /// A revealed companion (CR 702.139a): it stays in the sideboard and every card of the deck
    /// passes its condition.
    pub companion: Option<DefId>,
    /// The deck reader's weight per card definition (finite, > 0) for the cards the prior adds
    /// beyond `shown`: each such draw is made with probability proportional to the prior's own
    /// weight times this one (sequential draws, Wallenius' urn, as ENGINE.md 69.2). None: 1.
    pub tilt: Option<&'a dyn Fn(DefId) -> f64>,
}

/// A prior's sample: the deck (exactly `n` cards holding `shown`, sorted) and its sideboard
/// (the rest of the sampled pool, sorted; a revealed companion among it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampledDeck {
    pub deck: Vec<DefId>,
    pub sideboard: Vec<DefId>,
}

/// The prior of one player's deck as the other player samples it (ENGINE.md 102.1). A
/// [`sample`](Self::sample) reads only the [`DeckWant`] and the generator (a world depends on
/// the viewer's information only), always returns a deck that meets it (no failure, bounded
/// time: no rejection loop), and every card it adds is in [`support`](Self::support).
pub trait DeckPrior: Send + Sync {
    /// A short name (logs, errors).
    fn name(&self) -> &'static str;
    /// Every card a sampled deck may hold (card definitions, sorted): the format as the other
    /// player knows it, and the names the game's questions may name for this player
    /// (`Game::known_pools`, ENGINE.md 36.11).
    fn support(&self) -> &[DefId];
    /// A deck of `want.n` cards holding `want.shown`, and its sideboard.
    fn sample(&self, want: &DeckWant, rng: &mut Rng) -> SampledDeck;
    /// The expected number of copies of each card definition (indexed by `DefId`, `want.db.len()`
    /// long) among the cards [`sample`](Self::sample) adds beyond `want.shown`, untilted: the
    /// baseline of the deck reader's weights ([`deck_weights`]).
    fn expected_fill(&self, want: &DeckWant) -> Vec<f64>;
}

/// The stub prior until S2's sealed builder lands (ENGINE.md 102.1): FUZZ_POOL=eoe's random deck
/// generator conditioned on R. The deck is R, then lands up to round(17 n / 40) and spells up to
/// n, each drawn uniformly with replacement (times the tilt) from the table's supported cards
/// (the basic lands among the lands); with a revealed companion only cards its condition allows
/// (and one copy per name where it says so). The sideboard is the revealed companion, if any.
/// Built for any card table: the cube's decks run it too (the leak tests need the cube's stable
/// identities).
pub struct TablePrior {
    /// The supported cards of the table (not tokens, emblems, rules or faces), sorted.
    support: Vec<DefId>,
    lands: Vec<DefId>,
    spells: Vec<DefId>,
}

/// round(17 n / 40): FUZZ_POOL=eoe's 17 lands of 40.
fn land_target(n: usize) -> usize {
    (17 * n + 20) / 40
}

/// A uniform in [0, 1) from 53 bits.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// An index drawn with probability proportional to `w` (None if every weight is 0).
fn pick(rng: &mut Rng, w: &[f64]) -> Option<usize> {
    let tot: f64 = w.iter().sum();
    if tot <= 0.0 {
        return None;
    }
    let mut x = unit(rng) * tot;
    let mut last = None;
    for (i, &q) in w.iter().enumerate() {
        if q > 0.0 {
            last = Some(i);
            if x < q {
                return Some(i);
            }
            x -= q;
        }
    }
    last
}

impl TablePrior {
    pub fn new(db: &'static CardDb) -> TablePrior {
        let mut support: Vec<DefId> = db
            .supported()
            .filter_map(|n| db.id(n))
            .filter(|&id| {
                let d = db.def(id);
                !d.token && !d.emblem && !d.rules && d.card_def(id) == id
            })
            .collect();
        support.sort_unstable();
        support.dedup();
        let (lands, spells): (Vec<DefId>, Vec<DefId>) = support.iter().partition(|&&id| db.def(id).is(types::LAND));
        assert!(!lands.is_empty() && !spells.is_empty(), "TablePrior: a card table without supported lands and spells");
        TablePrior { support, lands, spells }
    }

    /// The table's prior, built once per card table.
    pub fn of(db: &'static CardDb) -> Arc<TablePrior> {
        static PRIORS: OnceLock<Mutex<Vec<(usize, Arc<TablePrior>)>>> = OnceLock::new();
        let key = db as *const CardDb as usize;
        let mut v = PRIORS.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
        if let Some((_, p)) = v.iter().find(|(k, _)| *k == key) {
            return p.clone();
        }
        let p = Arc::new(TablePrior::new(db));
        v.push((key, p.clone()));
        p
    }

    /// How many lands and spells `sample` adds to `want.shown`.
    fn counts(&self, want: &DeckWant) -> (usize, usize) {
        assert!(want.shown.len() <= want.n, "TablePrior: {} shown cards for a deck of {}", want.shown.len(), want.n);
        let free = want.n - want.shown.len();
        let shown_lands = want.shown.iter().filter(|&&d| want.db.def(d).is(types::LAND)).count();
        let lands = land_target(want.n).saturating_sub(shown_lands).min(free);
        (lands, free - lands)
    }
}

impl DeckPrior for TablePrior {
    fn name(&self) -> &'static str {
        "table"
    }

    fn support(&self) -> &[DefId] {
        &self.support
    }

    fn sample(&self, want: &DeckWant, rng: &mut Rng) -> SampledDeck {
        let db = want.db;
        let cond = want.companion.and_then(|c| db.def(c).companion.as_ref());
        let (add_lands, add_spells) = self.counts(want);
        let mut deck: Vec<DefId> = want.shown.to_vec();
        // A name the companion allows once (Lutri) is spent by a shown card or a draw.
        let mut spent: Vec<&str> = deck.iter().filter(|&&d| sample::one_per_name(db, cond, d)).map(|&d| db.def(d).name.as_str()).collect();
        let tilt = |d: DefId| want.tilt.map_or(1.0, |t| t(d));
        for (cands, k) in [(&self.lands, add_lands), (&self.spells, add_spells)] {
            let mut w: Vec<f64> = cands.iter().map(|&d| if sample::allowed(db, cond, d) && !spent.contains(&db.def(d).name.as_str()) { tilt(d) } else { 0.0 }).collect();
            for _ in 0..k {
                // (Every candidate spent or forbidden, which no table's spells come near for
                // a 40-card deck: a basic land keeps the deck legal and its size exact.)
                let d = match pick(rng, &w) {
                    Some(i) => {
                        let d = cands[i];
                        if sample::one_per_name(db, cond, d) {
                            spent.push(db.def(d).name.as_str());
                            for (j, &x) in cands.iter().enumerate() {
                                if db.def(x).name == db.def(d).name {
                                    w[j] = 0.0;
                                }
                            }
                        }
                        d
                    }
                    None => {
                        let basics = super::pools::basic_ids(db);
                        basics[rng.below(5) as usize]
                    }
                };
                deck.push(d);
            }
        }
        deck.sort_unstable();
        SampledDeck { deck, sideboard: want.companion.into_iter().collect() }
    }

    fn expected_fill(&self, want: &DeckWant) -> Vec<f64> {
        let db = want.db;
        let cond = want.companion.and_then(|c| db.def(c).companion.as_ref());
        let (add_lands, add_spells) = self.counts(want);
        let mut e = vec![0.0; db.len()];
        for (cands, k) in [(&self.lands, add_lands), (&self.spells, add_spells)] {
            let ok: Vec<DefId> = cands.iter().copied().filter(|&d| sample::allowed(db, cond, d)).collect();
            for &d in &ok {
                e[d as usize] += k as f64 / ok.len() as f64;
            }
        }
        e
    }
}

/// Remove one `d` from a sorted multiset; false if absent.
fn take_one(v: &mut Vec<DefId>, d: DefId) -> bool {
    match v.binary_search(&d) {
        Ok(i) => {
            v.remove(i);
            true
        }
        Err(_) => false,
    }
}

/// Does `s` meet `want` (the prior's contract, checked at every deal: a prior that breaks it is a
/// bug that would deal a world outside the viewer's information)? `Err` names what is wrong.
pub fn check_sample(prior: &dyn DeckPrior, want: &DeckWant, s: &SampledDeck) -> Result<(), String> {
    if s.deck.len() != want.n {
        return Err(format!("prior {}: a deck of {} cards, not {}", prior.name(), s.deck.len(), want.n));
    }
    let mut left = s.deck.clone();
    left.sort_unstable();
    for &d in want.shown {
        if !take_one(&mut left, d) {
            return Err(format!("prior {}: the deck lacks a shown {}", prior.name(), want.db.def(d).name));
        }
    }
    if let Some(&d) = left.iter().find(|&&d| prior.support().binary_search(&d).is_err()) {
        return Err(format!("prior {}: {} is outside its support", prior.name(), want.db.def(d).name));
    }
    if let Some(cond) = want.companion.and_then(|c| want.db.def(c).companion.as_ref()) {
        if !want.db.deck_satisfies(cond, &s.deck) {
            return Err(format!("prior {}: the deck breaks the revealed companion's condition", prior.name()));
        }
    }
    Ok(())
}

/// The deck reader's weights (ENGINE.md 102.1), one per card identity (`identity::D`; 1 where
/// the prior adds nothing), as 69's `deal_weights` makes the hand's: `p` is the prediction over
/// identities of the opponent's unseen cards (the `deck` target's distribution), N their number,
/// c_k the identities the viewer's clouds already place among them, u_k the prior's expected
/// fill of identity k (`DeckPrior::expected_fill` summed by identity): w_k =
/// `zero_search::belief_weight`(N p_k - c_k, u_k, tilt, clip). Everything it reads is the
/// viewer's: the public counts, its clouds, the prior and its information.
pub fn deck_weights(a: &MtgAz, viewer: PlayerId, p: &[f32], tilt: f64, clip: f64) -> Vec<f64> {
    let g = &a.g;
    let db = g.db();
    let ids = identity::identities(db);
    let o = opp(viewer);
    let mut w = vec![1.0; identity::D];
    let Some(prior) = a.m.deck_priors[o as usize].clone() else { return w };
    let sizes = [a.m.decks[0].iter().map(|&c| c as usize).sum(), a.m.decks[1].iter().map(|&c| c as usize).sum()];
    let kn = sample::knowledge(g, viewer, o, sizes);
    let comp = g.player(o).companion_def;
    let want = DeckWant { db, n: sizes[o as usize], shown: &kn.required, companion: comp, tilt: None };
    let fill = prior.expected_fill(&want);
    let mut u = vec![0.0f64; identity::D];
    for (d, &e) in fill.iter().enumerate() {
        u[ids[d] as usize] += e;
    }
    let mut c = vec![0.0f64; identity::D];
    for &d in &kn.required {
        c[ids[d as usize] as usize] += 1.0;
    }
    for &d in &kn.known {
        c[ids[d as usize] as usize] -= 1.0;
    }
    let n = kn.slots.len() as f64;
    for k in 0..identity::D {
        if u[k] > 0.0 {
            w[k] = zero_search::game::belief_weight(n * p[k] as f64 - c[k].max(0.0), u[k], tilt, clip);
        }
    }
    w
}

impl MtgAz {
    /// Are the decklists hidden (ENGINE.md 102)?
    pub fn hidden_decklists(&self) -> bool {
        self.g.spec().hidden_decklists
    }

    /// The hidden-decklist deal of `viewer`'s opponent (module docs; ENGINE.md 102.1): their deck
    /// sampled from the prior conditioned on what `viewer` knows, its unknown cards dealt to their
    /// cards `viewer` does not know, their sideboard and the match's pool and deck of theirs the
    /// sampled ones. `Game::determinize` follows (`SearchGame::determinize`). `tilt`: the deck
    /// reader's weights per card definition.
    pub(crate) fn sample_hidden_deck(&mut self, viewer: PlayerId, rng: &mut Rng, tilt: Option<&dyn Fn(DefId) -> f64>) {
        let o = opp(viewer);
        let Some(prior) = self.m.deck_priors[o as usize].clone() else { return };
        if self.m.sb.is_some() || self.g.is_over() {
            return;
        }
        let db = self.g.db();
        let sizes = [self.m.decks[0].iter().map(|&c| c as usize).sum(), self.m.decks[1].iter().map(|&c| c as usize).sum()];
        let kn = sample::knowledge(&self.g, viewer, o, sizes);
        let comp = self.g.player(o).companion_def;
        let want = DeckWant { db, n: sizes[o as usize], shown: &kn.required, companion: comp, tilt };
        let s = prior.sample(&want, rng);
        if let Err(e) = check_sample(&*prior, &want, &s) {
            panic!("hidden decklists, P{viewer}'s deal of P{o}'s deck: {e}");
        }
        let mut deal = s.deck.clone();
        deal.extend_from_slice(&kn.extras);
        deal.sort_unstable();
        for &d in &kn.known {
            assert!(take_one(&mut deal, d), "a card P{viewer} knows of P{o}'s is not in the sampled deck");
        }
        assert_eq!(deal.len(), kn.slots.len(), "P{o}'s sampled deck does not fit their cards");
        let mut side = s.sideboard;
        if let Some(c) = comp.filter(|c| !side.contains(c)) {
            side.push(c);
        }
        side.sort_unstable();
        self.g.redeal(viewer, o, &kn.slots, &kn.pinned, &deal, Arc::from(side.clone()), rng);
        let (pool, counts) = MatchPool::new(db, &s.deck, &side, &[]);
        let mut pools = (*self.m.pools).clone();
        pools[o as usize] = pool;
        self.m.pools = Arc::new(pools);
        self.m.decks[o as usize] = Arc::from(counts);
    }

    /// `determinize` with the opponent's hidden deck dealt by the deck reader's weights `w` (per
    /// card definition, finite and > 0; ENGINE.md 102.1): the same constraints and the same order
    /// of draws as `determinize`. Without hidden decklists: `determinize`.
    pub fn determinize_deck_tilted(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng, w: &dyn Fn(DefId) -> f64) {
        let mut r = Rng::new(rng.next_u64());
        if self.hidden_decklists() {
            self.sample_hidden_deck(viewer as u8, &mut r, Some(w));
        }
        if self.m.sb.is_none() {
            self.g.determinize(viewer as u8, &mut r);
        }
        self.m.rng = Rng::new(r.next_u64());
    }

    /// The hidden-decklist deal alone, for tests (ENGINE.md 102.3): the opponent's deck sampled
    /// from what `viewer` knows, the game's cards dealt to it (no `Game::determinize` after it).
    #[doc(hidden)]
    pub fn sample_hidden_deck_for(&mut self, viewer: u8, rng: &mut Rng, tilt: Option<&dyn Fn(DefId) -> f64>) {
        self.sample_hidden_deck(viewer, rng, tilt);
    }
}
