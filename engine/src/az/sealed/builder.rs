//! The randomised sealed deck builder (DESIGN.md 9 item 1, ENGINE.md 101.2): a 40-card deck from a
//! sealed pool plus basic lands (free in sealed), the rest of the pool the sideboard. Randomised
//! (colours, splash, land count, curve, which cards) so self-play sees good and mediocre decks;
//! seeded and deterministic.
//!
//! **Pure self-play (DESIGN.md 0): no human data of any kind.** A card's weight comes only from its
//! own properties, as `pools::weight` (creature 3, interaction 2 - damage, removal, counters,
//! discard, edicts, token makers in its IR -, other coloured cards 1, colourless 0.5), tilted by
//! its mana value for the deck's curve. No ratings, win rates, pick orders or hand-made grades.
//!
//! The rules ([`build`]):
//!
//! 1. **Land count** `n` uniform in `lands` (16..=18); spells 40 - `n`.
//! 2. **Colours.** Each pair's score is the weight of its best 40 - `n` playable spells (colours
//!    within the pair, every coloured pip payable by it, hybrid by either colour; colourless
//!    spells are playable in every pair). A pair needs 40 - `n` playable spells to be eligible.
//!    With probability `mono` a single colour with 40 - `n` playable spells is tried first (chosen
//!    the same way); else, or if none has enough, the pair is drawn among the eligible ones with
//!    probability proportional to exp((score - best score) / `temperature`) (`temperature` 0: the
//!    best, the first on a tie). No eligible pair (a pool too thin for 40 - `n` spells in any
//!    pair, never seen in generated pools): the best pair, and basics make up the 40.
//! 3. **Splash** (pairs only), with probability `splash`: a third colour X is allowed only when
//!    the deck can **cast** it: the pool holds a *fixer* for X playable in the pair - a nonbasic
//!    land that makes or fetches X, or a spell whose own abilities add mana of X, search for a
//!    land that makes X (a basic land, or X's basic land type) or create a token that does (a
//!    Lander, a Treasure) - and splash spells: playable with the pair plus X, of colour X, with
//!    exactly one X pip (no X phyrexian pip; a hybrid pip payable by the pair is not an X pip).
//!    X is drawn among such colours with probability proportional to its best `max_splash`
//!    splash spells' weight; then 1..=`max_splash` (uniform, at most the candidates) splash
//!    spells are drawn by weight. The deck plays every fixer for X it was allowed by (a nonland
//!    fixer counts as one of its spells), then basics of X until it has `splash_sources` sources
//!    of X (fixers and basics) and at least one basic of X when its only fixers are spells.
//! 4. **Spells**: the splash spells and nonland fixers, then drawn one by one without
//!    replacement among the playable spells (copies of a card are separate candidates) with
//!    probability proportional to weight^`power` x exp(-t (mana value - 3) / 2), where the
//!    deck's curve tilt t is uniform in [-`curve`, `curve`] (t > 0: a low curve, t < 0: a high
//!    one); at most `max_mv5` spells of mana value 5 or more and `max_mv4` of 4 (lifted when the
//!    playable spells run out).
//! 5. **Lands**: the pool's nonbasic lands that make or fetch a colour of the deck (main or
//!    splash), at most one colourless-only land (half the time), up to min(9, `n` - 7); then
//!    basics: the splash's (step 3), the rest split between the main colours by the spells'
//!    coloured pips (+1 each; at least a third each in a pair).
//!
//! The result is always exactly 40 cards (ENGINE.md 42: the user's rule for a deck built from a
//! pool; CR 100.2b makes 40 a minimum); the sideboard is the rest of the pool (CR 100.4b).
//!
//! **The best guess** ([`best_guess`], ENGINE.md 107.1): the same rules with every random choice at
//! its most likely value - 17 lands, the strongest pair, no splash, no curve tilt, the spells greedy by
//! weight - a deterministic function of the pool: the mixed field's best-guess decks (the sealed
//! spec's `best_share` of them).
use crate::az::decks::{self, BASICS, DECK_SIZE};
use crate::az::pools;
use crate::db::{supertypes, types, AbilityDef, ActEffect, CardDb, CardDef, Filter};
use crate::ids::DefId;
use crate::mana::COLORS;
use crate::program::Op;
use crate::rng::Rng;
use serde_json::Value;

/// The builder's settings (the `mtg` spec's `sealed.builder`, ENGINE.md 101.3).
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Lands per deck, uniform in `lands.0..=lands.1`.
    pub lands: (u8, u8),
    /// Probability of trying a single colour (when one has enough playable spells).
    pub mono: f64,
    /// Probability of trying a splash (when one is castable, step 3).
    pub splash: f64,
    /// At most this many splash spells.
    pub max_splash: u8,
    /// Sources of the splash colour a splashing deck plays (fixers and basics).
    pub splash_sources: u8,
    /// The colour pair's softmax temperature, in weight points (0: always the best pair).
    pub temperature: f64,
    /// The spells' selection: probability proportional to weight^power (0: uniform).
    pub power: f64,
    /// The curve tilt's range: t uniform in [-curve, curve] (0: no tilt).
    pub curve: f64,
    /// At most this many spells of mana value 5 or more, and of 4.
    pub max_mv5: u8,
    pub max_mv4: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lands: (16, 18),
            mono: 0.1,
            splash: 0.3,
            max_splash: 3,
            splash_sources: 3,
            temperature: 4.0,
            power: 2.0,
            curve: 1.0,
            max_mv5: 5,
            max_mv4: 7,
        }
    }
}

// (The settings are part of the spec, which is `Eq`: compared bit for bit.)
impl PartialEq for Settings {
    fn eq(&self, o: &Self) -> bool {
        let f = |x: f64| x.to_bits();
        (self.lands, self.max_splash, self.splash_sources, self.max_mv5, self.max_mv4)
            == (o.lands, o.max_splash, o.splash_sources, o.max_mv5, o.max_mv4)
            && [f(self.mono), f(self.splash), f(self.temperature), f(self.power), f(self.curve)]
                == [f(o.mono), f(o.splash), f(o.temperature), f(o.power), f(o.curve)]
    }
}
impl Eq for Settings {}

impl Settings {
    /// Settings from a JSON object (every key optional; an unknown key is an error).
    pub fn parse(v: &Value) -> Result<Settings, String> {
        let mut s = Settings::default();
        let o = v.as_object().ok_or("sealed builder: not a JSON object")?;
        for (k, x) in o {
            let num = || -> Result<f64, String> {
                x.as_f64().filter(|f| f.is_finite() && *f >= 0.0).ok_or_else(|| format!("sealed builder: {k} must be a number >= 0"))
            };
            let prob = || -> Result<f64, String> {
                num().and_then(|p| if p <= 1.0 { Ok(p) } else { Err(format!("sealed builder: {k} must be a probability")) })
            };
            let small = || -> Result<u8, String> {
                x.as_u64().filter(|&n| n <= 40).map(|n| n as u8).ok_or_else(|| format!("sealed builder: {k} must be an integer 0..=40"))
            };
            match k.as_str() {
                "lands" => {
                    let a = x.as_array().filter(|a| a.len() == 2).ok_or("sealed builder: lands must be [min, max]")?;
                    let (lo, hi) = (a[0].as_u64().unwrap_or(0), a[1].as_u64().unwrap_or(0));
                    if lo < 10 || hi < lo || hi > 24 {
                        return Err(format!("sealed builder: lands {lo}..{hi} out of range (10..=24)"));
                    }
                    s.lands = (lo as u8, hi as u8);
                }
                "mono" => s.mono = prob()?,
                "splash" => s.splash = prob()?,
                "max_splash" => s.max_splash = small()?.max(1),
                "splash_sources" => s.splash_sources = small()?,
                "temperature" => s.temperature = num()?,
                "power" => s.power = num()?,
                "curve" => s.curve = num()?,
                "max_mv5" => s.max_mv5 = small()?,
                "max_mv4" => s.max_mv4 = small()?,
                other => return Err(format!("sealed builder: unknown key {other:?}")),
            }
        }
        Ok(s)
    }

    /// The settings as JSON (every key).
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "lands": [self.lands.0, self.lands.1], "mono": self.mono, "splash": self.splash,
            "max_splash": self.max_splash, "splash_sources": self.splash_sources,
            "temperature": self.temperature, "power": self.power, "curve": self.curve,
            "max_mv5": self.max_mv5, "max_mv4": self.max_mv4,
        })
    }
}

/// A built deck: 40 cards and the sideboard (both sorted), and how it was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub deck: Vec<DefId>,
    pub sideboard: Vec<DefId>,
    /// The main colours (one or two, a `ManaMask`).
    pub colors: u8,
    /// The splash colour (0: none).
    pub splash: u8,
    /// Lands in the deck (nonbasic and basic).
    pub lands: u8,
    /// The deck's spells drawn for the splash colour.
    pub splash_spells: u8,
    /// The curve tilt, x 1000 (rounded; for reports).
    pub tilt_milli: i32,
}

/// The colours (WUBRG bits) a card can make or find for its controller: its mana abilities'
/// colours, the colours of lands its abilities search for (a basic land: any colour; a basic
/// land type: that colour), and those of the tokens it creates (a Lander token, a Treasure).
pub fn fixes(db: &CardDb, d: &CardDef) -> u8 {
    fixes_depth(db, d, 2)
}

fn fixes_depth(db: &CardDb, d: &CardDef, depth: u8) -> u8 {
    let mut mana = 0u8;
    let mut m = 0u8;
    let mut prog = |ops: &[Op]| {
        for op in ops {
            match op {
                Op::Search { filter, .. } => m |= finds(db, filter).unwrap_or(0),
                Op::CreateToken { token, .. } if depth > 0 => m |= fixes_depth(db, db.def(db.token_def(*token)), depth - 1),
                _ => {}
            }
        }
    };
    let mut ab = |a: &AbilityDef| {
        prog(&a.program);
        if let Some(modes) = &a.modes {
            for x in &modes.list {
                prog(&x.program);
            }
        }
    };
    if let Some(s) = &d.spell {
        ab(s);
    }
    for t in &d.triggers {
        ab(&t.ability);
    }
    for a in d.activated.iter().chain(&d.hand_abilities) {
        match &a.effect {
            ActEffect::Mana(x) => mana |= crate::lower::activated::prod_mask(&x.prod),
            ActEffect::Stack(x) => ab(x),
        }
    }
    (m | mana) & COLORS
}

/// The colours of the lands a library search's filter can find (None: it names no land): a land
/// card or a basic card, any colour (the deck's basics of every colour it plays); a basic land
/// type, its colour; "and" narrows, "or" widens.
fn finds(db: &CardDb, f: &Filter) -> Option<u8> {
    match f {
        Filter::Type(bits) if bits & types::LAND != 0 => Some(COLORS),
        Filter::Supertype(s) if s & supertypes::BASIC != 0 => Some(COLORS),
        Filter::Subtype(s) => BASICS.iter().position(|b| db.subtype_name(*s) == *b).map(|i| 1u8 << i),
        Filter::And(a, b) => match (finds(db, a), finds(db, b)) {
            (Some(x), Some(y)) => Some(x & y),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        },
        Filter::Or(v) => v.iter().filter_map(|x| finds(db, x)).reduce(|a, b| a | b),
        _ => None,
    }
}

/// The coloured pips of `d`'s mana cost a deck of colours `cols` can't pay with them: per colour,
/// the plain and phyrexian pips outside `cols` (hybrid pips payable by `cols` excluded, others
/// counted for both of their colours).
fn foreign_pips(d: &CardDef, cols: u8) -> [crate::num::Count; 5] {
    let c = &d.cost;
    let mut out: [crate::num::Count; 5] = [0; 5];
    for (i, o) in out.iter_mut().enumerate() {
        if cols & (1 << i) == 0 {
            *o += c.pips[i] + c.phyrexian[i];
        }
    }
    for (k, &n) in c.hybrid.iter().enumerate() {
        let m = crate::mana::pair_mask(k);
        if n > 0 && m & cols == 0 {
            for (i, o) in out.iter_mut().enumerate() {
                if m & (1 << i) != 0 {
                    *o += n;
                }
            }
        }
    }
    out
}

/// A spell a deck of colours `cols` can play: its colours within `cols` and every coloured pip
/// payable (as `pools::playable`).
pub fn playable(d: &CardDef, cols: u8) -> bool {
    !d.is(types::LAND) && pools::playable(d, cols)
}

/// A splash spell for colour `x` (bit) in a deck of colours `pair`: playable with `pair | x`, of
/// colour `x`, exactly one plain `x` pip and no other pip outside the pair.
fn splash_spell(d: &CardDef, pair: u8, x: u8) -> bool {
    if d.is(types::LAND) || d.colors & x == 0 || !pools::playable(d, pair | x) {
        return false;
    }
    let xi = x.trailing_zeros() as usize;
    let f = foreign_pips(d, pair);
    f.iter().enumerate().all(|(i, &n)| if i == xi { n == 1 } else { n == 0 }) && d.cost.pips[xi] == 1
}

/// The spells (nonland cards, copies separate) of `pool` with their weights and mana values, in the pool's order: what
/// the colours' scores read, each spell's weight computed once per build (ENGINE.md 112's speed-up: the scores and
/// orders of before, so every build draw for draw as before).
fn spells_of(db: &CardDb, pool: &[DefId]) -> Vec<(f64, u32, DefId)> {
    pool.iter()
        .copied()
        .filter(|&id| !db.def(id).is(types::LAND))
        .map(|id| {
            let d = db.def(id);
            (pools::weight(d), d.mana_value() as u32, id)
        })
        .collect()
}

/// (score, playable spells) of colours `cols`: the weight of its best `n` playable spells - the spells of `spells`
/// (`spells_of`) playable in `cols`, by weight (desc), mana value, id.
fn score(db: &CardDb, spells: &[(f64, u32, DefId)], cols: u8, n: usize) -> (f64, usize) {
    let mut v: Vec<(f64, u32, DefId)> = spells.iter().copied().filter(|x| playable(db.def(x.2), cols)).collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    (v.iter().take(n).map(|x| x.0).sum(), v.len())
}

/// The ten pairs in the builder's order (WU, WB, WR, WG, UB, ...): (score, pair, playable spells).
fn pair_scores(db: &CardDb, spells: &[(f64, u32, DefId)], n_spells: usize) -> Vec<(f64, u8, usize)> {
    let mut pairs: Vec<(f64, u8, usize)> = Vec::with_capacity(10);
    for a in 0..5 {
        for b in a + 1..5 {
            let p = (1u8 << a) | (1u8 << b);
            let (sc, n) = score(db, spells, p, n_spells);
            pairs.push((sc, p, n));
        }
    }
    pairs
}

/// A uniform float in [0, 1).
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// One of `opts` (score, value) drawn with probability proportional to exp((score - best) / t);
/// t = 0: the best (the first on a tie).
fn softmax_pick(rng: &mut Rng, opts: &[(f64, u8)], t: f64) -> u8 {
    let best = opts.iter().map(|o| o.0).fold(f64::MIN, f64::max);
    if t <= 0.0 {
        return opts.iter().find(|o| o.0 == best).expect("an option").1;
    }
    let w: Vec<f64> = opts.iter().map(|o| ((o.0 - best) / t).exp()).collect();
    opts[decks::pick(rng, &w).expect("positive weights")].1
}

/// The colour bits of a mask, lowest first.
fn bits(m: u8) -> impl Iterator<Item = usize> {
    (0..5).filter(move |&i| m & (1 << i) != 0)
}

/// Build a 40-card deck from `pool` (nonbasic cards of `db`, any order, copies repeated) with
/// `rng` (module docs). Deterministic in `rng`'s state; the pool's order doesn't matter.
pub fn build(db: &CardDb, pool: &[DefId], s: &Settings, rng: &mut Rng) -> Build {
    build_tilted(db, pool, s, None, rng)
}

/// [`build`] with the spells' draw weights (step 4) times `tilt` of each card (finite, > 0): the
/// hidden decklists' sealed prior, tilted by the deck reader (ENGINE.md 102). `None`: `build`,
/// draw for draw.
pub fn build_tilted(db: &CardDb, pool: &[DefId], s: &Settings, tilt: Option<&dyn Fn(DefId) -> f64>, rng: &mut Rng) -> Build {
    let mut pool: Vec<DefId> = pool.to_vec();
    pool.sort_unstable();
    let spells_w = spells_of(db, &pool);
    let spells = &spells_w[..];
    let n_lands = s.lands.0 as usize + rng.below((s.lands.1 - s.lands.0) as u32 + 1) as usize;
    let n_spells = DECK_SIZE - n_lands;
    // 2. Colours.
    let mut cols = 0u8;
    if unit(rng) < s.mono {
        let monos: Vec<(f64, u8)> = (0..5)
            .map(|i| 1u8 << i)
            .filter_map(|c| {
                let (sc, n) = score(db, &spells, c, n_spells);
                (n >= n_spells).then_some((sc, c))
            })
            .collect();
        if !monos.is_empty() {
            cols = softmax_pick(rng, &monos, s.temperature);
        }
    }
    if cols == 0 {
        let pairs = pair_scores(db, &spells, n_spells);
        let eligible: Vec<(f64, u8)> = pairs.iter().filter(|x| x.2 >= n_spells).map(|x| (x.0, x.1)).collect();
        cols = if eligible.is_empty() {
            let all: Vec<(f64, u8)> = pairs.iter().map(|x| (x.0, x.1)).collect();
            softmax_pick(rng, &all, 0.0)
        } else {
            softmax_pick(rng, &eligible, s.temperature)
        };
    }
    // 3. Splash.
    let mut splash = 0u8;
    let mut forced: Vec<DefId> = Vec::new(); // splash spells and nonland fixers
    let mut splash_spells = 0u8;
    if cols.count_ones() == 2 && unit(rng) < s.splash {
        let mut opts: Vec<(f64, u8)> = Vec::new();
        for x in (0..5).map(|i| 1u8 << i).filter(|x| cols & x == 0) {
            let has_fixer = pool.iter().any(|&id| {
                let d = db.def(id);
                fixes(db, d) & x != 0 && (d.is(types::LAND) || playable(d, cols))
            });
            if !has_fixer {
                continue;
            }
            let mut ws: Vec<f64> = pool.iter().filter(|&&id| splash_spell(db.def(id), cols, x)).map(|&id| pools::weight(db.def(id))).collect();
            if ws.is_empty() {
                continue;
            }
            ws.sort_by(|a, b| b.total_cmp(a));
            opts.push((ws.iter().take(s.max_splash as usize).sum(), x));
        }
        if !opts.is_empty() {
            let w: Vec<f64> = opts.iter().map(|o| o.0).collect();
            let x = opts[decks::pick(rng, &w).expect("positive splash weights")].1;
            let cands: Vec<DefId> = pool.iter().copied().filter(|&id| splash_spell(db.def(id), cols, x)).collect();
            let k = 1 + rng.below(s.max_splash as u32) as usize;
            let mut w: Vec<f64> = cands.iter().map(|&id| pools::weight(db.def(id)).powf(s.power)).collect();
            for _ in 0..k.min(cands.len()) {
                let Some(i) = decks::pick(rng, &w) else { break };
                w[i] = 0.0;
                forced.push(cands[i]);
            }
            splash_spells = forced.len() as u8;
            // Every nonland fixer for x the pair can play.
            forced.extend(pool.iter().copied().filter(|&id| {
                let d = db.def(id);
                !d.is(types::LAND) && fixes(db, d) & x != 0 && playable(d, cols)
            }));
            splash = x;
        }
    }
    // 4. Spells.
    let t = if s.curve > 0.0 { (unit(rng) * 2.0 - 1.0) * s.curve } else { 0.0 };
    let mut spells: Vec<DefId> = Vec::with_capacity(n_spells);
    let (mut mv5, mut mv4) = (0u8, 0u8);
    fn count_mv(d: &CardDef, mv5: &mut u8, mv4: &mut u8) {
        match d.mana_value() {
            5.. => *mv5 += 1,
            4 => *mv4 += 1,
            _ => {}
        }
    }
    for &id in forced.iter().take(n_spells) {
        count_mv(db.def(id), &mut mv5, &mut mv4);
        spells.push(id);
    }
    let mut cands: Vec<DefId> = pool.iter().copied().filter(|&id| playable(db.def(id), cols)).collect();
    for f in &spells {
        if let Some(i) = cands.iter().position(|c| c == f) {
            cands.remove(i);
        }
    }
    let mut w: Vec<f64> = cands
        .iter()
        .map(|&id| {
            let d = db.def(id);
            let w = pools::weight(d).powf(s.power) * (-t * (d.mana_value() as f64 - 3.0) / 2.0).exp();
            match tilt {
                Some(f) => w * f(id),
                None => w,
            }
        })
        .collect();
    let mut capped: Vec<usize> = Vec::new();
    while spells.len() < n_spells {
        let Some(i) = decks::pick(rng, &w) else { break };
        let d = db.def(cands[i]);
        let mv = d.mana_value();
        if (mv >= 5 && mv5 >= s.max_mv5) || (mv == 4 && mv4 >= s.max_mv4) {
            capped.push(i);
            w[i] = 0.0;
            continue;
        }
        w[i] = 0.0;
        count_mv(d, &mut mv5, &mut mv4);
        spells.push(cands[i]);
    }
    // (The caps lifted: the capped spells, cheapest first, then by id.)
    capped.sort_by_key(|&i| (db.def(cands[i]).mana_value(), cands[i]));
    for i in capped {
        if spells.len() == n_spells {
            break;
        }
        spells.push(cands[i]);
    }
    // 5. Lands. Nonbasic: those that make or fetch a colour of the deck, then at most one
    // colourless-only land (half the time).
    let (mut lands, colourless) = nonbasic_lands(db, &pool, cols | splash);
    if !colourless.is_empty() && rng.below(2) == 0 {
        lands.push(colourless[rng.below(colourless.len() as u32) as usize]);
    }
    finish(db, pool, spells, lands, cols, splash, n_lands, s, splash_spells, t)
}

/// Step 5's candidates: the pool's nonbasic lands that make or fetch a colour of `deck_cols` (in
/// the pool's order), and its colourless-only lands (no mana colour, not a fetch).
fn nonbasic_lands(db: &CardDb, pool: &[DefId], deck_cols: u8) -> (Vec<DefId>, Vec<DefId>) {
    let basics = pools::basic_ids(db);
    let mut lands: Vec<DefId> = Vec::new();
    let mut colourless: Vec<DefId> = Vec::new();
    for &id in pool {
        let d = db.def(id);
        if !d.is(types::LAND) || basics.contains(&id) {
            continue;
        }
        let m = fixes(db, d);
        if m & deck_cols != 0 {
            lands.push(id);
        } else if m == 0 && !decks::is_fetch(d) {
            colourless.push(id);
        }
    }
    (lands, colourless)
}

/// The rest of step 5 (no randomness): the nonbasic lands `lands` cut to min(9, `n_lands` - 7) (the
/// splash's first, then those with more of the deck's colours), then basics - the splash's, the rest
/// by the spells' pips -, the deck sorted, the sideboard the rest of `pool` (sorted).
#[allow(clippy::too_many_arguments)]
fn finish(db: &CardDb, pool: Vec<DefId>, spells: Vec<DefId>, mut lands: Vec<DefId>, cols: u8, splash: u8, n_lands: usize, s: &Settings, splash_spells: u8, t: f64) -> Build {
    let basics = pools::basic_ids(db);
    let deck_cols = cols | splash;
    let max_nonbasic = 9.min(n_lands.saturating_sub(7));
    if lands.len() > max_nonbasic {
        // (Too many: the splash's first, then those with more of the deck's colours.)
        lands.sort_by_key(|&id| {
            let m = fixes(db, db.def(id));
            (m & splash == 0, std::cmp::Reverse((m & deck_cols).count_ones()), id)
        });
        lands.truncate(max_nonbasic);
    }
    let mut deck: Vec<DefId> = spells.clone();
    deck.extend(&lands);
    // Basics: the splash's, then the main colours by pips.
    let n_basics = DECK_SIZE - deck.len();
    let mut basic_counts = [0usize; 5];
    if splash != 0 {
        let xi = splash.trailing_zeros() as usize;
        let lands_x = lands.iter().filter(|&&id| fixes(db, db.def(id)) & splash != 0).count();
        let spells_x = spells.iter().filter(|&&id| fixes(db, db.def(id)) & splash != 0).count();
        let mut need = (s.splash_sources as usize).saturating_sub(lands_x + spells_x);
        if lands_x == 0 {
            need = need.max(1);
        }
        basic_counts[xi] = need.min(n_basics.saturating_sub(cols.count_ones() as usize));
    }
    let rest = n_basics - basic_counts.iter().sum::<usize>();
    let main: Vec<usize> = bits(cols).collect();
    if main.len() == 1 {
        basic_counts[main[0]] += rest;
    } else {
        let mut pips = [0u32; 5];
        for &id in &spells {
            let c = &db.def(id).cost;
            for (i, p) in pips.iter_mut().enumerate() {
                *p += c.pips[i] as u32 + c.phyrexian[i] as u32;
            }
            for (k, &n) in c.hybrid.iter().enumerate() {
                let m = crate::mana::pair_mask(k);
                for (i, p) in pips.iter_mut().enumerate() {
                    if m & (1 << i) != 0 {
                        *p += n as u32;
                    }
                }
            }
        }
        let (pa, pb) = (pips[main[0]] as f64 + 1.0, pips[main[1]] as f64 + 1.0);
        let lo = rest / 3;
        let na = ((rest as f64 * pa / (pa + pb)).round() as usize).clamp(lo, rest - lo);
        basic_counts[main[0]] += na;
        basic_counts[main[1]] += rest - na;
    }
    for (i, &n) in basic_counts.iter().enumerate() {
        deck.extend(std::iter::repeat_n(basics[i], n));
    }
    debug_assert_eq!(deck.len(), DECK_SIZE);
    deck.sort_unstable();
    let mut side = pool;
    for id in &deck {
        if let Ok(i) = side.binary_search(id) {
            side.remove(i);
        }
    }
    let n_land = deck.iter().filter(|&&id| db.def(id).is(types::LAND)).count() as u8;
    Build { deck, sideboard: side, colors: cols, splash, lands: n_land, splash_spells, tilt_milli: (t * 1000.0).round() as i32 }
}

// ---- The best guess (ENGINE.md 107.1) ----

/// A spell's weight in the builder (module docs: creature 3, interaction 2, other coloured 1,
/// colourless 0.5; `pools::weight`), for tests and reports.
pub fn spell_weight(d: &CardDef) -> f64 {
    pools::weight(d)
}

/// The best guess's land count: the middle of `lands` (rounded down; 17 for the default 16..=18), the
/// most likely draw of the uniform land count.
pub fn best_lands(s: &Settings) -> usize {
    (s.lands.0 as usize + s.lands.1 as usize) / 2
}

/// The builder's **best guess** (ENGINE.md 107.1): [`build`] with every random choice replaced by its
/// most likely outcome under the same settings, so no randomness at all - a function of the pool (its
/// order aside) and the settings:
///
/// 1. lands: [`best_lands`] (17 by default: the middle of the uniform 16..=18);
/// 2. colours: no mono try (the `mono` coin's likelier side at 0.1), the pair the colour softmax ranks
///    first at temperature 0 (the strongest eligible pair by score, the first in the builder's order on
///    a tie);
/// 3. **no splash**: the `splash` coin's likelier side (0.3 by default); the splash's colour and its
///    1..=`max_splash` spells have no most likely value of their own, so the best guess is the plain
///    pair at any `splash` (documented as the rule, ENGINE.md 107.1);
/// 4. spells: no curve tilt (t = 0, the tilt's middle), drawn **greedily**: the playable spells by
///    weight, the heaviest first (weight^`power` keeps the order); on a tie of weight the mana value
///    nearest 3 (the curve tilt's pivot, where every tilt agrees), then the lower, then the definition;
///    under the caps `max_mv5` / `max_mv4` (lifted as in [`build`]: the capped spells cheapest first);
/// 5. lands: the pool's nonbasic lands that make or fetch a colour of the pair (cut as in [`build`]), no
///    colourless-only land (its coin is even: left out, as a land that casts nothing of the deck), and
///    basics by the spells' pips.
pub fn best_guess(db: &CardDb, pool: &[DefId], s: &Settings) -> Build {
    best_guess_covering(db, pool, s, None, 0).expect("the best guess: every pool has a pair")
}

/// [`best_guess`] whose colours hold `need` (a colour mask; the hidden decklists' sealed prior passes the
/// shown cards' colours, ENGINE.md 107.1): the first pair holding `need` in the order the best guess ranks
/// the pairs (eligible pairs first, by score), its spells greedy by weight x `tilt` (the deck reader's
/// tilt of the spells' draw weights, as [`build_tilted`]'s; None: the weights); None when no pair holds
/// `need` (three colours or more: the best guess never splashes). `need` 0, `tilt` None: [`best_guess`].
pub fn best_guess_covering(db: &CardDb, pool: &[DefId], s: &Settings, tilt: Option<&dyn Fn(DefId) -> f64>, need: u8) -> Option<Build> {
    let mut pool: Vec<DefId> = pool.to_vec();
    pool.sort_unstable();
    let n_lands = best_lands(s);
    let n_spells = DECK_SIZE.saturating_sub(n_lands);
    let ps = pair_scores(db, &spells_of(db, &pool), n_spells);
    // temperature 0's pick first: eligible pairs, then by score, then the builder's order
    let mut order: Vec<usize> = (0..ps.len()).collect();
    order.sort_by(|&i, &j| (ps[j].2 >= n_spells).cmp(&(ps[i].2 >= n_spells)).then(ps[j].0.total_cmp(&ps[i].0)).then(i.cmp(&j)));
    let cols = ps[order.into_iter().find(|&i| need & !ps[i].1 == 0)?].1;
    let spells = greedy_spells(db, &pool, cols, n_spells, s, tilt);
    let (lands, _) = nonbasic_lands(db, &pool, cols);
    Some(finish(db, pool, spells, lands, cols, 0, n_lands, s, 0, 0.0))
}

/// Step 4 of the best guess: up to `n` spells of `pool` (sorted) playable in `cols`, greedily
/// ([`best_guess`]'s order), under the caps.
fn greedy_spells(db: &CardDb, pool: &[DefId], cols: u8, n: usize, s: &Settings, tilt: Option<&dyn Fn(DefId) -> f64>) -> Vec<DefId> {
    let mut cands: Vec<(f64, DefId)> = pool
        .iter()
        .copied()
        .filter(|&id| playable(db.def(id), cols))
        .map(|id| (pools::weight(db.def(id)) * tilt.map_or(1.0, |f| f(id)), id))
        .collect();
    let mv = |id: DefId| db.def(id).mana_value() as i64;
    cands.sort_by(|a, b| b.0.total_cmp(&a.0).then((mv(a.1) - 3).abs().cmp(&(mv(b.1) - 3).abs())).then(mv(a.1).cmp(&mv(b.1))).then(a.1.cmp(&b.1)));
    let (mut spells, mut capped) = (Vec::with_capacity(n), Vec::new());
    let (mut mv5, mut mv4) = (0u8, 0u8);
    for &(_, id) in &cands {
        if spells.len() == n {
            break;
        }
        match mv(id) {
            5.. if mv5 >= s.max_mv5 => capped.push(id),
            4 if mv4 >= s.max_mv4 => capped.push(id),
            m => {
                mv5 += (m >= 5) as u8;
                mv4 += (m == 4) as u8;
                spells.push(id);
            }
        }
    }
    // (The caps lifted: the capped spells, cheapest first, then by id.)
    capped.sort_by_key(|&id| (mv(id), id));
    for id in capped {
        if spells.len() == n {
            break;
        }
        spells.push(id);
    }
    spells
}

/// The colours of a mask as letters (`"WU"`, `""`).
pub fn letters(m: u8) -> String {
    bits(m).map(|i| ['W', 'U', 'B', 'R', 'G'][i]).collect()
}

/// The basic land names (for reports).
pub fn basic_names() -> [&'static str; 5] {
    BASICS
}
