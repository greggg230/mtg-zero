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
    /// A bank of `k` pools the K-sample deal reuses for every set of shown cards of one game
    /// (ENGINE.md 102.8: what the prior draws before it reads the shown cards); empty: none.
    fn bank(&self, _k: usize, _rng: &mut Rng) -> Vec<Vec<DefId>> {
        Vec::new()
    }
    /// [`sample`](Self::sample) from a pool of the [`bank`](Self::bank) instead of a new one.
    fn sample_from(&self, want: &DeckWant, _pool: Option<&[DefId]>, rng: &mut Rng) -> SampledDeck {
        self.sample(want, rng)
    }
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
    /// sampled ones. `Game::determinize` follows (`SearchGame::determinize`). `tilt`: weights per
    /// card definition the prior's own draws are multiplied by (a fresh sample). `deck_tilt`: the deck
    /// prediction's tilt (ENGINE.md 106): with K cached decks one of them drawn by `tilt_decks`'
    /// probabilities instead of uniformly; with K = 0 the prior sampled with `deck_weights` as `tilt`.
    /// Only for a viewer whose encoding is framed in its own seat (the prediction is of its opponent's
    /// cards: not while it decides for the player it controls, ENGINE.md 100.5) and who has unseen
    /// cards of theirs; else the untilted deal.
    pub(crate) fn sample_hidden_deck(&mut self, viewer: PlayerId, rng: &mut Rng, tilt: Option<&dyn Fn(DefId) -> f64>, deck_tilt: Option<DeckTilt>) {
        let o = opp(viewer);
        let Some(prior) = self.m.deck_priors[o as usize].clone() else { return };
        if self.m.sb.is_some() || self.g.is_over() {
            return;
        }
        let deck_tilt = deck_tilt.filter(|_| tilt.is_none() && self.g.encoding_seat(viewer) == viewer);
        if let (Some(t), 0) = (deck_tilt, self.m.deal.k) {
            // (K = 0: no cached decks to weigh - the prior's own draws tilted, ENGINE.md 102.1)
            let w = deck_weights(self, viewer, t.p, t.tilt, t.clip);
            let ids = identity::identities(self.g.db());
            return self.sample_hidden_deck(viewer, rng, Some(&|d: DefId| w[ids[d as usize] as usize]), None);
        }
        let db = self.g.db();
        let sizes = [self.m.decks[0].iter().map(|&c| c as usize).sum(), self.m.decks[1].iter().map(|&c| c as usize).sum()];
        let kn = sample::knowledge(&self.g, viewer, o, sizes);
        let comp = self.g.player(o).companion_def;
        let want = DeckWant { db, n: sizes[o as usize], shown: &kn.required, companion: comp, tilt };
        // The K-sample deal (ENGINE.md 102.8): one of the K decks the prior gave for this request
        // (memoized per game) - uniformly, or by the deck tilt (ENGINE.md 106) -, else a fresh sample
        // (K = 0, or a tilt of the prior's draws).
        let d: Arc<Dealt> = match deck_tilt {
            Some(t) if self.m.deal.k > 0 && !kn.slots.is_empty() => {
                let ess = self.m.deal.ess;
                let decks = self.m.deal.decks(&*prior, o, &want);
                let td = self.m.deal.tilted(tilt_key(DealCache::key(o, &want), &kn, &t, ess), || {
                    let ids = identity::identities(db);
                    tilt_decks(&decks, &kn.required, &kn.fixed_by_identity(ids), kn.slots.len(), ids, t, ess)
                });
                let mut x = unit(rng);
                let mut i = td.probs.len() - 1;
                for (j, &q) in td.probs.iter().enumerate() {
                    if x < q {
                        i = j;
                        break;
                    }
                    x -= q;
                }
                decks[i].clone()
            }
            _ if want.tilt.is_none() && self.m.deal.k > 0 => self.m.deal.pick(&*prior, o, &want, rng),
            _ => Arc::new(Dealt::new(&*prior, &want, prior.sample(&want, rng))),
        };
        let mut deal = d.deck.clone();
        deal.extend_from_slice(&kn.extras);
        deal.sort_unstable();
        for &d in &kn.known {
            assert!(take_one(&mut deal, d), "a card P{viewer} knows of P{o}'s is not in the sampled deck");
        }
        assert_eq!(deal.len(), kn.slots.len(), "P{o}'s sampled deck does not fit their cards");
        let _ = db;
        self.g.redeal(viewer, o, &kn.slots, &kn.pinned, &deal, d.side.clone(), rng);
        let mut pools = (*self.m.pools).clone();
        pools[o as usize] = d.pool.clone();
        self.m.pools = Arc::new(pools);
        self.m.decks[o as usize] = d.counts.clone();
        // (how their deck was made is theirs: unknown in the world, ENGINE.md 107)
        self.m.how[o as usize] = None;
    }

    /// `determinize` with the opponent's hidden deck dealt by the deck reader's weights `w` (per
    /// card definition, finite and > 0; ENGINE.md 102.1): the same constraints and the same order
    /// of draws as `determinize`. Without hidden decklists: `determinize`.
    pub fn determinize_deck_tilted(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng, w: &dyn Fn(DefId) -> f64) {
        let mut r = Rng::new(rng.next_u64());
        if self.hidden_decklists() {
            self.sample_hidden_deck(viewer as u8, &mut r, Some(w), None);
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
        self.sample_hidden_deck(viewer, rng, tilt, None);
    }

    /// The deck tilt's weighing of the K cached decks `viewer`'s deal of the live state would use
    /// (ENGINE.md 106; tests and analysis): None without hidden decklists, cached decks (K = 0), unseen
    /// cards of the opponent, or with the viewer deciding for the player it controls.
    #[doc(hidden)]
    pub fn deck_tilt_of(&self, viewer: u8, t: DeckTilt) -> Option<(Arc<Vec<Arc<Dealt>>>, Arc<TiltedDecks>)> {
        let o = opp(viewer);
        let prior = self.m.deck_priors[o as usize].clone()?;
        if self.m.sb.is_some() || self.g.is_over() || self.m.deal.k == 0 || self.g.encoding_seat(viewer) != viewer {
            return None;
        }
        let db = self.g.db();
        let sizes = [self.m.decks[0].iter().map(|&c| c as usize).sum(), self.m.decks[1].iter().map(|&c| c as usize).sum()];
        let kn = sample::knowledge(&self.g, viewer, o, sizes);
        if kn.slots.is_empty() {
            return None;
        }
        let comp = self.g.player(o).companion_def;
        let want = DeckWant { db, n: sizes[o as usize], shown: &kn.required, companion: comp, tilt: None };
        let decks = self.m.deal.decks(&*prior, o, &want);
        let ess = self.m.deal.ess;
        let td = self.m.deal.tilted(tilt_key(DealCache::key(o, &want), &kn, &t, ess), || {
            let ids = identity::identities(db);
            tilt_decks(&decks, &kn.required, &kn.fixed_by_identity(ids), kn.slots.len(), ids, t, ess)
        });
        Some((decks, td))
    }
}

// ---- The sealed prior (ENGINE.md 102.6): S2's collation and builder, conditioned ----

/// The default prior of a sealed seat's deck (ENGINE.md 102.6): what S2's `"sealed"` spec makes,
/// conditioned exactly on the shown cards R, every step bounded:
///
/// 1. **The pool**: the spec's collation and boosters (`sealed::generated_pool`), its seed drawn
///    from the deal's generator (never the game's `pool_seed`: the real pool would leak).
/// 2. **R's nonbasic cards placed in it**: each one the pool lacks replaces a pool card of its
///    rarity (from the set list) that is not itself shown, chosen uniformly; any card not shown
///    when that rarity runs out. The pool holds R.
/// 3. **The build**: S2's builder (`builder::build_tilted`: the spec's settings, the deck reader's
///    tilt on its spell weights), up to [`SEALED_TRIES`] builds, the first whose colours (pair and
///    splash) cover R's nonland cards' colours, else the one covering most (the first of those).
/// 4. **R forced in**: R's cards the build lacks replace build cards that are not shown, each of
///    its kind (a land for a land, a spell for a spell; any when the kind runs out), chosen with
///    probability proportional to 1 / tilt (the reader's favourites kept); then basic lands of
///    the build's colours (by its basic lands' proportions) up to the size n, or non-shown cards
///    removed down to it (a given build's size). The sideboard is the rest of the pool.
///
/// Exact: the deck has n cards, holds R and comes from a pool the collation's slots and rarities
/// allow but for R's own placement. Modelled (the prior's choices, not the Bayes posterior of
/// "collation, then builder" given R): the replacement by rarity, the bounded colour retries, the
/// forced swaps.
///
/// **The mixed field** (ENGINE.md 107.1, the spec's `best_share` p): the sample's builder is the
/// spec's mixture - with probability p (one uniform draw of the generator after step 2, none at p 0
/// or 1) the builder's best guess, else step 3's randomised builds. A best-guess sample is
/// `builder::best_guess_covering` of the pool with R's nonland colours as `need` (the strongest pair
/// holding R's colours: the best guess is a function of the pool, so this is the best guess of a pool
/// holding R whose colours could have shown R); when no pair holds them (three colours or more: the
/// best guess never splashes) the sample falls back to step 3's randomised builds. Step 4 as before.
///
/// **The deck choice by an evaluator** (ENGINE.md 112; [`SealedPrior::of_seat`] for a seat that chooses so): the
/// sample's deck is chosen as the seat chooses - with the choice's `explore` share (one uniform draw of the
/// generator after step 2, none at 0 or 1) a randomised build (step 3's, as an exploring seat plays a randomised
/// candidate), else the seat's K candidates of the pool holding R (its best guess and K - 1 randomised builds,
/// drawn from the generator) scored by the same evaluator file, the best of those whose colours (pair and splash)
/// cover R's nonland colours (or a draw at the choice's temperature among them); none covering: step 3's
/// randomised builds. The mixed field's `best_share` is not read for such a seat (it chooses instead). Step 4 as
/// before. Modelled, as 107.1's best guess: the chosen deck given that its colours could have shown R is the best
/// covering candidate (the choice restricted to R's colours), not the exact posterior of "the evaluator's choice
/// holds R".
pub struct SealedPrior {
    collation: super::sealed::collation::Collation,
    boosters: usize,
    settings: super::sealed::builder::Settings,
    /// The mixed field's best-guess share (ENGINE.md 107.1; 0: the randomised builder only).
    best_share: f64,
    /// The seat's deck choice by an evaluator (ENGINE.md 112), if it chooses so.
    choice: Option<PriorChoice>,
    /// Every card a pool of the set can hold and the basic lands (sorted).
    support: Arc<Vec<DefId>>,
    /// The set list's rarity of each definition (indexed by `DefId`; 0 none).
    rarity: Arc<Vec<u8>>,
}

/// A seat's deck choice by an evaluator as its prior makes it (ENGINE.md 112): the candidates, the explore share,
/// the evaluator and its temperature.
#[derive(Clone, Debug)]
pub struct PriorChoice {
    pub k: u8,
    pub explore: f64,
    pub choice: super::sealed::EvalChoice,
}

/// Step 3's builds at most (ENGINE.md 102.6).
pub const SEALED_TRIES: usize = 4;

impl SealedPrior {
    pub fn new(collation: super::sealed::collation::Collation, boosters: usize, settings: super::sealed::builder::Settings) -> SealedPrior {
        static TABLES: OnceLock<(Arc<Vec<DefId>>, Arc<Vec<u8>>)> = OnceLock::new();
        let (support, rarity) = TABLES
            .get_or_init(|| {
                let db = super::sealed::db();
                let mut rarity = vec![0u8; db.len()];
                let mut support: Vec<DefId> = super::pools::basic_ids(db).to_vec();
                let data = super::sealed::collation::set_data();
                let names = data.cards.iter().map(|c| (c.name, c.rarity)).chain(data.treatments.iter().flat_map(|t| t.1.iter().copied()));
                for (name, r) in names {
                    if let Some(id) = db.id(name) {
                        support.push(id);
                        if rarity[id as usize] == 0 {
                            rarity[id as usize] = r.bytes().next().unwrap_or(b'?');
                        }
                    }
                }
                support.sort_unstable();
                support.dedup();
                (Arc::new(support), Arc::new(rarity))
            })
            .clone();
        SealedPrior { collation, boosters, settings, best_share: 0.0, choice: None, support, rarity }
    }

    /// The prior of a seat choosing its deck by an evaluator (ENGINE.md 112).
    pub fn with_choice(mut self, c: PriorChoice) -> SealedPrior {
        assert!((0.0..=1.0).contains(&c.explore) && c.k >= 2, "a deck choice of {} candidates, explore {}", c.k, c.explore);
        self.choice = Some(c);
        self
    }

    /// The prior of seat `p`'s deck of a sealed spec (as the other player samples it): [`of_spec`](Self::of_spec),
    /// and for a built seat that chooses its deck by an evaluator (ENGINE.md 112) that choice.
    pub fn of_seat(s: &super::sealed::SealedSpec, p: u8) -> SealedPrior {
        let base = SealedPrior::of_spec(s);
        let built = !matches!(s.seats[p as usize], super::sealed::SeatSource::Build { .. });
        match s.deck_choice.as_ref().filter(|c| c.seats[p as usize] && built) {
            Some(c) => match c.evaluator() {
                Some(e) => base.with_choice(PriorChoice { k: c.k, explore: c.explore.0, choice: e.clone() }),
                None => base,
            },
            None => base,
        }
    }

    /// The deck the seat's choice by an evaluator makes of `pool` (R placed in it; `need`: R's nonland colours)
    /// (the type's docs), or None for step 3's randomised builds (an exploring seat, or no candidate covering R).
    fn chosen(&self, db: &CardDb, pool: &[DefId], tilt: Option<&dyn Fn(DefId) -> f64>, need: u8, rng: &mut Rng) -> Option<super::sealed::builder::Build> {
        use super::sealed::builder;
        let c = self.choice.as_ref()?;
        let explore = c.explore >= 1.0 || (c.explore > 0.0 && unit(rng) < c.explore);
        if explore {
            return None;
        }
        let mut cands = Vec::with_capacity(c.k as usize);
        cands.push(builder::best_guess_covering(db, pool, &self.settings, tilt, 0).expect("the best guess: every pool has a pair"));
        for _ in 1..c.k {
            cands.push(builder::build_tilted(db, pool, &self.settings, tilt, rng));
        }
        let covering: Vec<usize> = (0..cands.len()).filter(|&j| need & !(cands[j].colors | cands[j].splash) == 0).collect();
        if covering.is_empty() {
            return None;
        }
        let scores: Vec<f64> = covering
            .iter()
            .map(|&j| c.choice.scorer.score(&cands[j].deck).unwrap_or_else(|e| panic!("hidden decklists, the sealed prior's deck choice: {e}")))
            .collect();
        let i = super::sealed::pick_by_score(&scores, c.choice.temperature, rng);
        Some(cands.swap_remove(covering[i]))
    }

    /// The prior of the mixed field with best-guess share `p` (ENGINE.md 107.1).
    pub fn with_best_share(mut self, p: f64) -> SealedPrior {
        assert!((0.0..=1.0).contains(&p), "best_share {p}: a probability");
        self.best_share = p;
        self
    }

    /// A pool of the collation (sorted), its seed drawn from `rng` (never the game's `pool_seed`).
    fn pool(&self, rng: &mut Rng) -> Vec<DefId> {
        let seed = rng.next_u64();
        let mut pool = super::sealed::generated_pool(super::sealed::db(), self.collation, self.boosters, seed)
            .unwrap_or_else(|e| panic!("hidden decklists, the sealed prior: {e} (a set card the engine doesn't support)"));
        pool.sort_unstable();
        pool
    }

    /// The prior of seat `p`'s deck of a sealed spec: its collation (a seat without one, a given
    /// pool or build of the yardstick: `arena`, whose pools are Arena's, ENGINE.md 101.1), its
    /// boosters and builder settings, and its mixed field's best-guess share (ENGINE.md 107.1).
    pub fn of_spec(s: &super::sealed::SealedSpec) -> SealedPrior {
        SealedPrior::new(s.collation.unwrap_or(super::sealed::collation::Collation::Arena), s.boosters, s.builder.clone())
            .with_best_share(s.best_share.0)
    }
}

impl DeckPrior for SealedPrior {
    fn name(&self) -> &'static str {
        if self.choice.is_some() {
            "sealed+evaluator"
        } else {
            "sealed"
        }
    }

    fn support(&self) -> &[DefId] {
        &self.support
    }

    fn sample(&self, want: &DeckWant, rng: &mut Rng) -> SampledDeck {
        self.sample_from(want, None, rng)
    }

    /// Pools of the collation, each from its own seed drawn from `rng` (the K-sample deal's bank).
    fn bank(&self, k: usize, rng: &mut Rng) -> Vec<Vec<DefId>> {
        (0..k).map(|_| self.pool(rng)).collect()
    }

    fn sample_from(&self, want: &DeckWant, given: Option<&[DefId]>, rng: &mut Rng) -> SampledDeck {
        let db = want.db;
        let basics = super::pools::basic_ids(db);
        let is_land = |d: DefId| db.def(d).is(types::LAND);
        let tilt = |d: DefId| want.tilt.map_or(1.0, |t| t(d));
        // 1. The pool (a bank's, or a new one).
        let mut pool = match given {
            Some(p) => p.to_vec(),
            None => self.pool(rng),
        };
        // 2. R's nonbasic cards placed in it, by rarity.
        let mut shown_in = vec![false; pool.len()];
        let mut extra: Vec<DefId> = Vec::new();
        for &r in want.shown.iter().filter(|d| !basics.contains(d)) {
            if let Some(i) = (0..pool.len()).find(|&i| pool[i] == r && !shown_in[i]) {
                shown_in[i] = true;
                continue;
            }
            let free: Vec<usize> = (0..pool.len()).filter(|&i| !shown_in[i]).collect();
            let same: Vec<usize> = free.iter().copied().filter(|&i| self.rarity[pool[i] as usize] == self.rarity[r as usize]).collect();
            let from = if same.is_empty() { &free } else { &same };
            match from.len() {
                0 => extra.push(r),
                k => {
                    let i = from[rng.below(k as u32) as usize];
                    pool[i] = r;
                    shown_in[i] = true;
                }
            }
        }
        pool.extend(extra);
        pool.sort_unstable();
        // 3. The build: the first of a few whose colours cover the shown cards'.
        let need = want.shown.iter().filter(|&&d| !is_land(d)).fold(0u8, |m, &d| m | db.def(d).colors) & crate::mana::COLORS;
        // (The mixed field, ENGINE.md 107.1: the best guess holding R's colours with the spec's share;
        // one draw only when the share is strictly between 0 and 1. A seat choosing by an evaluator,
        // ENGINE.md 112: its choice instead.)
        let guess = if self.choice.is_some() {
            self.chosen(db, &pool, want.tilt, need, rng)
        } else {
            let p = self.best_share;
            let best_mode = p >= 1.0 || (p > 0.0 && unit(rng) < p);
            if best_mode { super::sealed::builder::best_guess_covering(db, &pool, &self.settings, want.tilt, need) } else { None }
        };
        let b = match guess {
            Some(b) => b,
            None => {
                let mut best: Option<(u32, super::sealed::builder::Build)> = None;
                // (Shown cards of four or five colours: no pair and splash covers them, one build.)
                let tries = if need.count_ones() > 3 { 1 } else { SEALED_TRIES };
                for _ in 0..tries {
                    let b = super::sealed::builder::build_tilted(db, &pool, &self.settings, want.tilt, rng);
                    let miss = (need & !(b.colors | b.splash)).count_ones();
                    if best.as_ref().is_none_or(|x| miss < x.0) {
                        best = Some((miss, b));
                    }
                    if miss == 0 {
                        break;
                    }
                }
                best.expect("a build").1
            }
        };
        // 4. R forced in, then the size n.
        let mut rest = b.deck.clone();
        let mut add: Vec<DefId> = Vec::new();
        for &r in want.shown {
            match rest.binary_search(&r) {
                Ok(i) => {
                    rest.remove(i);
                }
                Err(_) => add.push(r),
            }
        }
        for &r in &add {
            let kind: Vec<usize> = (0..rest.len()).filter(|&i| is_land(rest[i]) == is_land(r)).collect();
            let from: Vec<usize> = if kind.is_empty() { (0..rest.len()).collect() } else { kind };
            if from.is_empty() {
                break;
            }
            let w: Vec<f64> = from.iter().map(|&i| 1.0 / tilt(rest[i])).collect();
            let k = pick(rng, &w).unwrap_or(0);
            rest.remove(from[k]);
        }
        let mut deck: Vec<DefId> = want.shown.to_vec();
        deck.extend_from_slice(&rest);
        let basic_mix: Vec<DefId> = b.deck.iter().copied().filter(|d| basics.contains(d)).collect();
        while deck.len() < want.n {
            let d = if basic_mix.is_empty() { basics[rng.below(5) as usize] } else { basic_mix[rng.below(basic_mix.len() as u32) as usize] };
            deck.push(d);
        }
        while deck.len() > want.n {
            // (Only a deck holding R and more than n cards: R comes first, `rest` after it.)
            let k = want.shown.len() + rng.below((deck.len() - want.shown.len()) as u32) as usize;
            deck.remove(k);
        }
        deck.sort_unstable();
        let mut side = pool;
        for d in &deck {
            if let Ok(i) = side.binary_search(d) {
                side.remove(i);
            }
        }
        SampledDeck { deck, sideboard: side }
    }

    /// By the prior's own samples: 16 deals from a generator seeded by the request (the shown
    /// cards and the size), the mean count of each card beyond R.
    fn expected_fill(&self, want: &DeckWant) -> Vec<f64> {
        let mut e = vec![0.0; want.db.len()];
        let seed = want.shown.iter().fold(0x5EA1_ED_F111u64 ^ want.n as u64, |h, &d| crate::rng::split64(h ^ d as u64));
        let mut rng = Rng::new(seed);
        let plain = DeckWant { db: want.db, n: want.n, shown: want.shown, companion: want.companion, tilt: None };
        const K: usize = 16;
        for _ in 0..K {
            let s = self.sample(&plain, &mut rng);
            let mut left = s.deck;
            for &d in want.shown {
                take_one(&mut left, d);
            }
            for d in left {
                e[d as usize] += 1.0 / K as f64;
            }
        }
        e
    }
}

// ---- The K-sample deal (ENGINE.md 102.8) ----

/// A deck the prior gave, ready to deal: the deck, the sideboard (a revealed companion among it),
/// the match's pool and deck counts of it.
pub struct Dealt {
    pub deck: Vec<DefId>,
    pub side: Arc<[DefId]>,
    pub pool: MatchPool,
    pub counts: Arc<[u8]>,
}

impl Dealt {
    /// A sample `s` of `prior` for `want`, checked against the prior's contract (a prior that
    /// breaks it would deal a world outside the viewer's information: a panic).
    pub fn new(prior: &dyn DeckPrior, want: &DeckWant, s: SampledDeck) -> Dealt {
        if let Err(e) = check_sample(prior, want, &s) {
            panic!("hidden decklists, a deal: {e}");
        }
        let mut side = s.sideboard;
        if let Some(c) = want.companion.filter(|c| !side.contains(c)) {
            side.push(c);
        }
        side.sort_unstable();
        let (pool, counts) = MatchPool::new(want.db, &s.deck, &side, &[]);
        Dealt { deck: s.deck, side: Arc::from(side), pool, counts: Arc::from(counts) }
    }
}

/// The K decks of one request (its key), the per-seat pool banks of one game, and the deck tilt's
/// weighings of the K decks (ENGINE.md 106: by the request's and the prediction's key).
#[derive(Default)]
struct DealMemo {
    entries: Vec<(u64, Arc<Vec<Arc<Dealt>>>)>,
    banks: [Option<Arc<Vec<Vec<DefId>>>>; 2],
    tilted: Vec<(u64, Arc<TiltedDecks>)>,
}

/// Requests remembered (a game's shown cards change a few dozen times; the latest are reused).
const MEMO: usize = 16;

/// The K-sample deal (ENGINE.md 102.8): for a request of the prior (the opponent's seat, the deck's
/// size, the shown cards R, a revealed companion: everything the prior reads) the prior's K decks
/// are drawn once, from a generator seeded by the game's `salt` and the request alone, and every
/// deal picks one of them uniformly with its own generator. The decks are a function of the salt
/// and the viewer's information, so a world depends only on its seed and that information; the
/// memo (shared by a game's clones, behind a mutex: `MtgAz`'s, never `Game`'s) only saves work.
/// A sealed prior's pools come from a bank of K pools per seat and game (what it draws before it
/// reads R), so a new R costs K conditionings and builds, not K pool generations.
#[derive(Clone)]
pub struct DealCache {
    /// K (0: every deal samples the prior afresh).
    pub k: usize,
    pub salt: u64,
    /// The deck tilt's effective-sample-size floor, a share of K (ENGINE.md 106.1; spec key
    /// `"deck_tilt_ess"`, default `DECK_TILT_ESS`).
    pub ess: f64,
    memo: Arc<Mutex<DealMemo>>,
}

impl DealCache {
    pub fn new(k: usize, salt: u64) -> DealCache {
        DealCache { k, salt, ess: DECK_TILT_ESS, memo: Arc::new(Mutex::new(DealMemo::default())) }
    }

    /// The request's key: the seat, the size, the companion and R.
    fn key(o: PlayerId, want: &DeckWant) -> u64 {
        let mut h = crate::rng::split64(0x0DEA_15A1_0000 ^ o as u64 ^ (want.n as u64) << 8 ^ (want.companion.map_or(0xFFFF, |c| c as u64)) << 24);
        for &d in want.shown {
            h = crate::rng::split64(h ^ d as u64);
        }
        h
    }

    /// One of the K decks of `want` for seat `o`, drawn uniformly with `rng`.
    pub fn pick(&self, prior: &dyn DeckPrior, o: PlayerId, want: &DeckWant, rng: &mut Rng) -> Arc<Dealt> {
        let i = rng.below(self.k as u32) as usize;
        self.decks(prior, o, want)[i].clone()
    }

    /// The K decks of `want` for seat `o` (drawn once per request, from the salt and the request alone).
    pub fn decks(&self, prior: &dyn DeckPrior, o: PlayerId, want: &DeckWant) -> Arc<Vec<Arc<Dealt>>> {
        let key = Self::key(o, want);
        let mut m = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, v)) = m.entries.iter().find(|e| e.0 == key) {
            return v.clone();
        }
        let bank = m.banks[o as usize]
            .get_or_insert_with(|| Arc::new(prior.bank(self.k, &mut Rng::new(crate::rng::split64(self.salt ^ 0xBA4C ^ o as u64)))))
            .clone();
        let mut r = Rng::new(crate::rng::split64(self.salt ^ key));
        let v: Vec<Arc<Dealt>> =
            (0..self.k).map(|j| Arc::new(Dealt::new(prior, want, prior.sample_from(want, bank.get(j).map(|p| &p[..]), &mut r)))).collect();
        let v = Arc::new(v);
        if m.entries.len() >= MEMO {
            m.entries.remove(0);
        }
        m.entries.push((key, v.clone()));
        v
    }

    /// The deck tilt's weighing of a request's K decks under the key `key` (the request's and the
    /// prediction's, ENGINE.md 106), made by `make` the first time: a pure function of its key's inputs,
    /// so the memo only saves work.
    fn tilted(&self, key: u64, make: impl FnOnce() -> TiltedDecks) -> Arc<TiltedDecks> {
        let mut m = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, t)) = m.tilted.iter().find(|e| e.0 == key) {
            return t.clone();
        }
        let t = Arc::new(make());
        if m.tilted.len() >= MEMO {
            m.tilted.remove(0);
        }
        m.tilted.push((key, t.clone()));
        t
    }
}

// ---- The deck tilt (ENGINE.md 106): the K cached decks weighed by the deck prediction ----

/// The effective share of the K cached decks the deck tilt keeps at least, by default (ENGINE.md 106.1;
/// the spec key `"deck_tilt_ess"`): when the weighed decks' effective sample size would fall below this
/// x K, the tilt is tempered (its exponent scaled by beta < 1) until it is that.
pub const DECK_TILT_ESS: f64 = 0.5;

/// The deck tilt of a search (ENGINE.md 106): `p`, the network's prediction of the opponent's unseen
/// cards by identity (the `deck` target's distribution, `identity::D` floats: the root decider's,
/// from its own features), the tilt alpha > 0 and the clip R >= 1 (the seat's `belief_tilts.deck`
/// and `belief_clip`).
#[derive(Clone, Copy, Debug)]
pub struct DeckTilt<'a> {
    pub p: &'a [f32],
    pub tilt: f64,
    pub clip: f64,
}

/// How the deck tilt weighs one request's K cached decks (ENGINE.md 106.1).
#[derive(Clone, Debug, PartialEq)]
pub struct TiltedDecks {
    /// The weight w_k of each card identity (`identity::D`; 1 where no deck adds the identity).
    pub w: Vec<f64>,
    /// Each cached deck's probability (they sum to 1).
    pub probs: Vec<f64>,
    /// The tempering exponent in (0, 1] (1: the tilt as set).
    pub beta: f64,
    /// The probabilities' effective sample size, 1 / sum p^2 (K for the uniform pick).
    pub ess: f64,
    /// What they were made from (tests, analysis): N, the opponent's unseen cards; per identity the
    /// fixed part (`fixed`) and the prior's expected excess u_k (`u`); per cached deck its excess over
    /// the shown cards by identity (`excess`: (identity, copies), in the deck's order).
    pub n: usize,
    pub fixed: Vec<f64>,
    pub u: Vec<f64>,
    pub excess: Vec<Vec<(u16, f64)>>,
}

/// The effective sample size of the probabilities proportional to exp(beta (l - max l)).
fn tilt_probs(l: &[f64], beta: f64) -> (Vec<f64>, f64) {
    let mx = l.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let e: Vec<f64> = l.iter().map(|&x| (beta * (x - mx)).exp()).collect();
    let s: f64 = e.iter().sum();
    let p: Vec<f64> = e.iter().map(|x| x / s).collect();
    let ess = 1.0 / p.iter().map(|x| x * x).sum::<f64>();
    (p, ess)
}

/// The deck tilt's weighing of the K decks `decks` the prior gave for a request whose shown cards
/// are `shown` (R, sorted), of the opponent whose unseen cards number `n`, `fixed` (per identity) of
/// them the same in every deck (R's cards the viewer does not know where they are, and the non-deck
/// cards it lost track of): ENGINE.md 106.1. The excess e_k(D) = copies of identity k the prior added
/// to deck D beyond R; the prior's own expected excess u_k = mean over the K decks; the prediction's
/// excess N p_k - fixed_k; w_k = `belief_weight`(N p_k - fixed_k, u_k, alpha, R) where u_k > 0; deck D's
/// log weight sum_k e_k(D) ln w_k; its probability proportional to exp(beta x that), beta = 1 unless
/// the effective sample size would fall below `ess` x K (then the largest beta that keeps it, by
/// bisection: the ESS falls as beta grows).
pub fn tilt_decks(decks: &[Arc<Dealt>], shown: &[DefId], fixed: &[f64], n: usize, ids: &[u16], t: DeckTilt, ess: f64) -> TiltedDecks {
    let k = decks.len();
    assert!(k > 0, "the deck tilt needs the K cached decks (deal_samples > 0)");
    let d = identity::D;
    // each deck's excess over R, by identity (sparse)
    let mut ex: Vec<Vec<(u16, f64)>> = Vec::with_capacity(k);
    let mut u = vec![0.0f64; d];
    for dl in decks {
        let mut left = dl.deck.clone();
        for &s in shown {
            assert!(take_one(&mut left, s), "a cached deck without a shown card");
        }
        let mut e: Vec<(u16, f64)> = Vec::new();
        for c in left {
            let id = ids[c as usize];
            match e.iter_mut().find(|x| x.0 == id) {
                Some(x) => x.1 += 1.0,
                None => e.push((id, 1.0)),
            }
            u[id as usize] += 1.0 / k as f64;
        }
        ex.push(e);
    }
    let mut w = vec![1.0f64; d];
    for i in 0..d {
        if u[i] > 0.0 {
            w[i] = zero_search::game::belief_weight(n as f64 * t.p[i] as f64 - fixed[i], u[i], t.tilt, t.clip);
        }
    }
    let l: Vec<f64> = ex.iter().map(|e| e.iter().map(|&(id, c)| c * w[id as usize].ln()).sum()).collect();
    let floor = ess * k as f64;
    let (mut probs, mut ess) = tilt_probs(&l, 1.0);
    let mut beta = 1.0;
    if ess < floor {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if tilt_probs(&l, mid).1 >= floor {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        beta = lo;
        (probs, ess) = tilt_probs(&l, beta);
    }
    TiltedDecks { w, probs, beta, ess, n, fixed: fixed.to_vec(), u, excess: ex }
}

/// The key of a deck tilt's weighing: everything `tilt_decks` reads - the request's key (its K decks and
/// R), the viewer's other knowledge of the unseen cards (the deck cards it knows, the non-deck cards it lost
/// track of, their number: the fixed part and N) and the prediction, tilt and clip.
fn tilt_key(request: u64, kn: &sample::Knowledge, t: &DeckTilt, ess: f64) -> u64 {
    let mut h = crate::rng::split64(request ^ 0x7111_7DEC ^ t.tilt.to_bits().rotate_left(17) ^ t.clip.to_bits() ^ ess.to_bits().rotate_left(41));
    h = crate::rng::split64(h ^ (kn.slots.len() as u64) << 32 ^ kn.known.len() as u64);
    for &d in kn.known.iter().chain(&kn.extras) {
        h = crate::rng::split64(h ^ d as u64);
    }
    h = crate::rng::split64(h ^ kn.extras.len() as u64);
    for &x in t.p {
        h = crate::rng::split64(h ^ x.to_bits() as u64);
    }
    h
}
