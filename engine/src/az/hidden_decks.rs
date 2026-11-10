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
        // The K-sample deal (ENGINE.md 102.8): one of the K decks the prior gave for this request
        // (memoized per game), else a fresh sample (K = 0, or a tilt).
        let d: Arc<Dealt> = if want.tilt.is_none() && self.m.deal.k > 0 {
            self.m.deal.pick(&*prior, o, &want, rng)
        } else {
            Arc::new(Dealt::new(&*prior, &want, prior.sample(&want, rng)))
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
pub struct SealedPrior {
    collation: super::sealed::collation::Collation,
    boosters: usize,
    settings: super::sealed::builder::Settings,
    /// Every card a pool of the set can hold and the basic lands (sorted).
    support: Arc<Vec<DefId>>,
    /// The set list's rarity of each definition (indexed by `DefId`; 0 none).
    rarity: Arc<Vec<u8>>,
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
        SealedPrior { collation, boosters, settings, support, rarity }
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
    /// boosters and builder settings.
    pub fn of_spec(s: &super::sealed::SealedSpec) -> SealedPrior {
        SealedPrior::new(s.collation.unwrap_or(super::sealed::collation::Collation::Arena), s.boosters, s.builder.clone())
    }
}

impl DeckPrior for SealedPrior {
    fn name(&self) -> &'static str {
        "sealed"
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
        let b = best.expect("a build").1;
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

/// The K decks of one request (its key) and the per-seat pool banks of one game.
#[derive(Default)]
struct DealMemo {
    entries: Vec<(u64, Arc<Vec<Arc<Dealt>>>)>,
    banks: [Option<Arc<Vec<Vec<DefId>>>>; 2],
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
    memo: Arc<Mutex<DealMemo>>,
}

impl DealCache {
    pub fn new(k: usize, salt: u64) -> DealCache {
        DealCache { k, salt, memo: Arc::new(Mutex::new(DealMemo::default())) }
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
        let key = Self::key(o, want);
        let i = rng.below(self.k as u32) as usize;
        let mut m = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, v)) = m.entries.iter().find(|e| e.0 == key) {
            return v[i].clone();
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
        v[i].clone()
    }
}
