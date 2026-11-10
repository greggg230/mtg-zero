//! The Edge of Eternities Play Booster collations, `paper` and `arena`, and the seeded sealed-pool
//! generator: a bit-identical port of `mtgz.sealed.collation` (docs/sealed_data.md 3, ENGINE.md
//! 101.1). The same seed and collation give the same boosters, slot by slot, as the Python
//! reference (`tests/test_sealed_rust.py` compares them through the engine module).
//!
//! * The card list is `data/sealed/eoe_set.json` (compiled in), as the reference reads it: the
//!   sheets keep the file's card order, the booster-fun printings the treatments' order.
//! * The randomness is the reference's own generator ([`SplitMix`]: splitmix64 with integer-only
//!   sampling: rejection for uniform integers, a partial Fisher-Yates shuffle for distinct
//!   positions, integer weights), never the engine's `Rng`, and the draws come in the reference's
//!   order (`_paper_booster`, `_arena_booster`).
//! * Paradox Haze (no IR) is drawn again within the Special Guest slot unless `paradox_haze`.
//!
//! The slot tables, the run constants and their sources are the reference's (its module
//! docstring and docs/sealed_data.md 3-4); they are repeated here, and a change to either side
//! shows in the comparison.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

/// The card list the collations draw from (`tools/sealed_eoe_set.py`'s output).
pub const EOE_SET: &str = include_str!("../../../../data/sealed/eoe_set.json");

const TOTAL: u64 = 100_000;
const COMMONS: usize = 7;
const UNCOMMONS: usize = 3;
/// Boosters per 100,000 whose seventh common is a Special Guest.
const SPECIAL_GUEST: u64 = 1_800;
/// Boosters in a sealed pool.
pub const BOOSTERS: usize = 6;
/// The Special Guest the engine excludes (no IR).
pub const EXCLUDED: &[&str] = &["Paradox Haze"];
/// The basic lands of the paper land slot (the reference's `cards.BASICS`, in its order).
pub const BASICS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];
const COLOURS: [char; 5] = ['W', 'U', 'B', 'R', 'G'];
const TREATMENTS: [&str; 3] = ["viewport", "triumphant", "surreal"];

/// A slot table: (printing groups, rarity, boosters per 100,000).
type Table = &'static [(&'static [&'static str], &'static str, u64)];

const WILDCARD: Table = &[
    (&["eoe"], "common", 12_500),
    (&["eoe"], "uncommon", 62_500),
    (&["eoe"], "rare", 10_600),
    (&["eoe"], "mythic", 225),
    (&["eos"], "rare", 10_000),
    (&["eos"], "mythic", 2_500),
    (&["viewport"], "rare", 1_000),
    (&["viewport"], "mythic", 225),
    (&["triumphant", "surreal"], "rare", 225),
    (&["triumphant", "surreal"], "mythic", 225),
];
const RARE: Table = &[
    (&["eoe"], "rare", 80_400),
    (&["eoe"], "mythic", 14_200),
    (&["triumphant"], "rare", 2_000),
    (&["surreal"], "rare", 2_000),
    (&["triumphant"], "mythic", 467),
    (&["surreal"], "mythic", 467),
    (&["viewport"], "mythic", 466),
];
const FOIL: Table = &[
    (&["eoe"], "common", 58_000),
    (&["eoe"], "uncommon", 32_000),
    (&["eoe"], "rare", 6_400),
    (&["eoe"], "mythic", 1_100),
    (&["eos"], "rare", 1_000),
    (&["eos"], "mythic", 500),
    (&["viewport", "triumphant", "surreal"], "rare", 500),
    (&["viewport", "triumphant", "surreal"], "mythic", 500),
];
const PROMO: Table = &[(&["eoe"], "rare", 80_400), (&["eoe"], "mythic", 14_200)];
const ARENA_WILDCARD: Table = &[
    (&["eoe"], "common", 12_500),
    (&["eoe"], "uncommon", 62_500),
    (&["eoe"], "rare", 10_673),
    (&["eoe"], "mythic", 609),
    (&["eos"], "rare", 10_974),
    (&["eos"], "mythic", 2_744),
];
const ARENA_RARE: Table = &[(&["eoe"], "rare", 84_400), (&["eoe"], "mythic", 15_600)];
const ARENA_FOIL: Table = &[
    (&["eoe"], "common", 58_000),
    (&["eoe"], "uncommon", 32_000),
    (&["eoe"], "rare", 6_900),
    (&["eoe"], "mythic", 1_600),
    (&["eos"], "rare", 1_000),
    (&["eos"], "mythic", 500),
];

// Arena's print runs (docs/sealed_data.md 4.3).
const GROUP9: [&str; 9] = [
    "Chrome Companion",
    "Command Bridge",
    "Drill Too Deep",
    "Meltstrider's Gear",
    "Nanoform Sentinel",
    "Red Tiger Mechan",
    "Temporal Intervention",
    "Thaumaton Torpedo",
    "Zealous Display",
];
const COMMON_RUN: usize = 81;
const GROUP_STEP: usize = 9;
const COLOURLESS_GAP: usize = 2;
const COLOURLESS_PER_9: usize = 2;
const UNCOMMON_RUN: usize = 100;
const UNCOMMON_GAP: usize = 4;
const UNCOMMON_GAP_5: usize = 3;
const MULTI_WEIGHT: u128 = 10;
const COLOURLESS_UNCOMMON_GAP: usize = 5;
/// In an uncommon run colourless is a colour of its own.
const COLOURLESS_BIT: u32 = 1 << 5;

/// The weight of continuing the colour `gap` places back, by the run's gap.
fn lane_weight(gap: usize) -> u128 {
    match gap {
        4 => 1,
        3 => 5,
        _ => unreachable!("uncommon runs have gap 3 or 4"),
    }
}

/// splitmix64 (Steele, Lea, Flood 2014) with the reference's integer-only sampling
/// (`mtgz.sealed.collation.Rng`).
#[derive(Clone, Debug)]
pub struct SplitMix(u64);

impl SplitMix {
    pub fn new(seed: u64) -> SplitMix {
        SplitMix(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        let r = crate::rng::split64(self.0);
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        r
    }

    /// Uniform in `0..n` for `0 < n <= 2^64`: rejection of the top partial block, then modulo.
    pub fn below(&mut self, n: u128) -> u64 {
        assert!(n > 0 && n <= 1u128 << 64, "below({n})");
        let limit = (1u128 << 64) - (1u128 << 64) % n;
        loop {
            let x = self.next_u64() as u128;
            if x < limit {
                return (x % n) as u64;
            }
        }
    }

    pub fn choice<T: Copy>(&mut self, s: &[T]) -> T {
        s[self.below(s.len() as u128) as usize]
    }

    /// `k` distinct elements of `s` (a partial Fisher-Yates shuffle over a copy).
    pub fn sample<T: Copy>(&mut self, s: &[T], k: usize) -> Vec<T> {
        let mut a = s.to_vec();
        for i in 0..k {
            let j = i + self.below((a.len() - i) as u128) as usize;
            a.swap(i, j);
        }
        a.truncate(k);
        a
    }

    /// An index with probability weight / sum (integer weights).
    pub fn weighted(&mut self, w: &[u128]) -> usize {
        let total: u128 = w.iter().sum();
        let mut x = self.below(total) as u128;
        for (i, &v) in w.iter().enumerate() {
            if x < v {
                return i;
            }
            x -= v;
        }
        unreachable!("weighted: x < total")
    }
}

/// Which collation (docs/sealed_data.md 3.1, 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Collation {
    /// Wizards' published Play Booster (14 cards with the land slot).
    Paper,
    /// MTG Arena's boosters as 17lands shows them (13 cards, colour-balanced print runs).
    Arena,
}

impl Collation {
    pub fn parse(s: &str) -> Result<Collation, String> {
        match s {
            "paper" => Ok(Collation::Paper),
            "arena" => Ok(Collation::Arena),
            _ => Err(format!("collation must be \"paper\" or \"arena\", not {s:?}")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Collation::Paper => "paper",
            Collation::Arena => "arena",
        }
    }
}

/// A booster's slot (the reference's slot names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Common,
    SpecialGuest,
    Uncommon,
    Wildcard,
    Rare,
    Foil,
    Land,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Common => "common",
            Slot::SpecialGuest => "special_guest",
            Slot::Uncommon => "uncommon",
            Slot::Wildcard => "wildcard",
            Slot::Rare => "rare",
            Slot::Foil => "foil",
            Slot::Land => "land",
        }
    }
}

/// A generated sealed pool: each booster's (slot, name), and the paper prerelease promo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedPool {
    pub seed: u64,
    pub collation: Collation,
    pub boosters: Vec<Vec<(Slot, &'static str)>>,
    pub promo: Option<&'static str>,
}

impl SealedPool {
    /// The pool's nonbasic card names, sorted (the land slots' basics are not pool cards; the
    /// promo is): 78 for six boosters.
    pub fn cards(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> =
            self.boosters.iter().flatten().filter(|(s, _)| *s != Slot::Land).map(|&(_, n)| n).chain(self.promo).collect();
        v.sort_unstable();
        v
    }

    /// The land slots' basics (paper).
    pub fn basics(&self) -> Vec<&'static str> {
        self.boosters.iter().flatten().filter(|(s, _)| *s == Slot::Land).map(|&(_, n)| n).collect()
    }
}

/// How a pool is generated (the reference's keyword arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub boosters: usize,
    /// Paper only: the Prerelease Pack's foil rare or mythic promo.
    pub prerelease_promo: bool,
    /// Keep Paradox Haze (statistics only: the engine has no IR for it).
    pub paradox_haze: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { boosters: BOOSTERS, prerelease_promo: false, paradox_haze: false }
    }
}

/// A card of the list: its name, sheet, rarity and colours (`"WU"`, `""` colourless).
#[derive(Clone, Debug)]
pub struct SetCard {
    pub name: &'static str,
    pub sheet: &'static str,
    pub rarity: &'static str,
    pub colors: &'static str,
}

/// The card list and the booster-fun printings, read once.
pub struct SetData {
    pub cards: Vec<SetCard>,
    /// Per treatment (viewport, triumphant, surreal): (name, rarity) per printing.
    pub treatments: Vec<(&'static str, Vec<(&'static str, &'static str)>)>,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The parsed `eoe_set.json`.
pub fn set_data() -> &'static SetData {
    static DATA: OnceLock<SetData> = OnceLock::new();
    DATA.get_or_init(|| {
        let v: Value = serde_json::from_str(EOE_SET).expect("eoe_set.json");
        assert_eq!(v["format"], "mtgz-eoe-set", "eoe_set.json: not an mtgz-eoe-set file");
        let s = |x: &Value, k: &str| leak(x[k].as_str().unwrap_or_else(|| panic!("eoe_set.json: {k} of {x}")));
        let cards: Vec<SetCard> = v["cards"]
            .as_array()
            .expect("eoe_set.json: cards")
            .iter()
            .map(|c| SetCard { name: s(c, "name"), sheet: s(c, "sheet"), rarity: s(c, "rarity"), colors: s(c, "colors") })
            .collect();
        for (i, c) in cards.iter().enumerate() {
            assert!(cards[..i].iter().all(|d| d.name != c.name), "eoe_set.json: {} twice", c.name);
        }
        let treatments = TREATMENTS
            .iter()
            .map(|&t| {
                let rows = v["treatments"][t].as_array().unwrap_or_else(|| panic!("eoe_set.json: treatment {t}"));
                (t, rows.iter().map(|r| (s(r, "name"), s(r, "rarity"))).collect())
            })
            .collect();
        SetData { cards, treatments }
    })
}

/// {(group, rarity): printings}: the reference's `_sheets` (`booster_fun` false folds the
/// booster-fun printings into the main-set sheets).
fn sheets(booster_fun: bool) -> BTreeMap<(&'static str, &'static str), Vec<&'static str>> {
    let d = set_data();
    let mut m: BTreeMap<(&'static str, &'static str), Vec<&'static str>> = BTreeMap::new();
    for c in &d.cards {
        m.entry((c.sheet, c.rarity)).or_default().push(c.name);
    }
    for (t, rows) in &d.treatments {
        for &(name, rarity) in rows {
            m.entry((t, rarity)).or_default().push(name);
        }
    }
    if !booster_fun {
        for t in TREATMENTS {
            for r in ["rare", "mythic"] {
                let main = m[&("eoe", r)].clone();
                m.insert((t, r), main);
            }
        }
    }
    m
}

/// The fixed inputs of Arena's print runs (the reference's `_runs`): commons by colour, GROUP9
/// or not; uncommons by colour mask.
#[derive(Clone, Debug)]
struct Runs {
    /// Colour key ('W'..'G', 'C') -> GROUP9 commons of it (sorted).
    g9: BTreeMap<char, Vec<&'static str>>,
    /// Colour key -> the other commons of it (sorted).
    rest: BTreeMap<char, Vec<&'static str>>,
    /// The colour with two GROUP9 commons.
    doubled: char,
    /// The uncommons' colour masks (by number of colours, then mask) and their cards (sorted).
    umasks: Vec<u32>,
    ugroups: Vec<Vec<&'static str>>,
    incidences: [u32; 5],
}

fn runs() -> &'static Runs {
    static RUNS: OnceLock<Runs> = OnceLock::new();
    RUNS.get_or_init(|| {
        let d = set_data();
        let (mut g9, mut rest): (BTreeMap<char, Vec<&str>>, BTreeMap<char, Vec<&str>>) = (BTreeMap::new(), BTreeMap::new());
        let mut unc: Vec<&SetCard> = Vec::new();
        for c in &d.cards {
            if c.sheet != "eoe" {
                continue;
            }
            match c.rarity {
                "common" => {
                    assert!(c.colors.len() <= 1, "a multicolour common ({}): the common run has none", c.name);
                    let key = c.colors.chars().next().unwrap_or('C');
                    let into = if GROUP9.contains(&c.name) { &mut g9 } else { &mut rest };
                    into.entry(key).or_default().push(c.name);
                }
                "uncommon" => unc.push(c),
                _ => {}
            }
        }
        for v in g9.values_mut().chain(rest.values_mut()) {
            v.sort_unstable();
        }
        let mut sizes: Vec<usize> = g9.values().map(|v| v.len()).collect();
        sizes.sort_unstable();
        assert!(
            sizes == [1, 1, 1, 1, 2, 3] && g9.get(&'C').map_or(0, |v| v.len()) == 3,
            "GROUP9 must be one card of four colours, two of the fifth, three colourless"
        );
        let doubled = *g9.iter().find(|(k, v)| **k != 'C' && v.len() == 2).expect("the doubled colour").0;
        for c in COLOURS.iter().copied().chain(['C']) {
            let want = if c == 'C' { 6 } else { 15 } - g9.get(&c).map_or(0, |v| v.len());
            assert_eq!(rest.get(&c).map_or(0, |v| v.len()), want, "commons of colour {c}: the cycle needs {want}");
        }
        unc.sort_by(|a, b| a.name.cmp(b.name));
        let mut groups: BTreeMap<u32, Vec<&'static str>> = BTreeMap::new();
        for c in unc {
            let mut m = 0u32;
            for ch in c.colors.chars() {
                m |= 1 << COLOURS.iter().position(|&x| x == ch).unwrap_or_else(|| panic!("{}: colour {ch}", c.name));
            }
            assert!(m.count_ones() <= 2, "{}: an uncommon of more than two colours", c.name);
            groups.entry(if m == 0 { COLOURLESS_BIT } else { m }).or_default().push(c.name);
        }
        let mut umasks: Vec<u32> = groups.keys().copied().collect();
        umasks.sort_by_key(|&m| (m.count_ones(), m));
        let mut incidences = [0u32; 5];
        for (b, inc) in incidences.iter_mut().enumerate() {
            *inc = umasks.iter().filter(|&&m| m >> b & 1 != 0).map(|m| groups[m].len() as u32).sum();
        }
        let ugroups = umasks.iter().map(|m| groups[m].clone()).collect();
        Runs { g9, rest, doubled, umasks, ugroups, incidences }
    })
}

fn cyclic_gap(a: usize, b: usize, n: usize) -> usize {
    let d = a.abs_diff(b) % n;
    d.min(n - d)
}

/// The colour of each place of a fresh 81-common run (the reference's `common_pattern`).
fn common_pattern(rng: &mut SplitMix, r: &Runs) -> Vec<char> {
    let gpos: Vec<usize> = (0..COMMON_RUN).step_by(GROUP_STEP).collect();
    let others: Vec<usize> = (0..COMMON_RUN).filter(|p| p % GROUP_STEP != 0).collect();
    loop {
        let mut cl = rng.sample(&gpos, r.g9[&'C'].len());
        cl.extend(rng.sample(&others, r.rest[&'C'].len()));
        cl.sort_unstable();
        let m = cl.len();
        if (0..m).any(|i| (i + 1..m).any(|j| cyclic_gap(cl[i], cl[j], COMMON_RUN) < COLOURLESS_GAP)) {
            continue;
        }
        if (0..m).any(|i| (cl[(i + COLOURLESS_PER_9) % m] as i64 - cl[i] as i64).rem_euclid(COMMON_RUN as i64) < GROUP_STEP as i64) {
            continue; // three colourless within nine places
        }
        let mut index = [usize::MAX; COMMON_RUN];
        let mut i = 0;
        for (p, x) in index.iter_mut().enumerate() {
            if !cl.contains(&p) {
                *x = i;
                i += 1;
            }
        }
        let mut counts = [0usize; 5];
        for &p in &gpos {
            if !cl.contains(&p) {
                counts[index[p] % 5] += 1;
            }
        }
        let mut sorted = counts;
        sorted.sort_unstable();
        if sorted != [1, 1, 1, 1, 2] {
            continue;
        }
        let mut cycle = [None; 5];
        cycle[counts.iter().position(|&c| c == 2).expect("a residue of two")] = Some(r.doubled);
        let others_c: Vec<char> = COLOURS.iter().copied().filter(|&c| c != r.doubled).collect();
        let mut order = rng.sample(&others_c, 4).into_iter();
        for c in cycle.iter_mut() {
            if c.is_none() {
                *c = order.next();
            }
        }
        return (0..COMMON_RUN).map(|p| if cl.contains(&p) { 'C' } else { cycle[index[p] % 5].expect("filled") }).collect();
    }
}

/// The reference's `_uncommon_weight`.
fn uncommon_weight(size: usize, mask: u32, r: &[u32; 5], left: usize, lane: u128) -> u128 {
    let mut w = size as u128 * if (mask & 31).count_ones() == 2 { MULTI_WEIGHT } else { 1 } * lane;
    let mut nc = 0u32;
    for (b, &rb) in r.iter().enumerate() {
        if mask >> b & 1 != 0 {
            w *= (rb as u128 * UNCOMMON_RUN as u128).pow(2);
            nc += 1;
        }
    }
    w * (21 * left as u128).pow(2 * (2 - nc))
}

/// The colour mask of each place of a fresh 100-uncommon run (the reference's
/// `uncommon_pattern`).
fn uncommon_pattern(rng: &mut SplitMix, r: &Runs, gap: usize) -> Vec<u32> {
    assert!(r.incidences.iter().all(|&x| x == 21), "the urgency weights assume 21 colour incidences per colour");
    let n = UNCOMMON_RUN;
    'run: loop {
        let mut rem: Vec<usize> = r.ugroups.iter().map(|g| g.len()).collect();
        let mut left_inc = r.incidences;
        let mut seq: Vec<u32> = Vec::with_capacity(n);
        for step in 0..n {
            let mut recent = 0u32;
            for &m in &seq[seq.len().saturating_sub(gap - 1)..] {
                recent |= m;
            }
            for &m in &seq[seq.len().saturating_sub(COLOURLESS_UNCOMMON_GAP - 1)..] {
                recent |= m & COLOURLESS_BIT;
            }
            let left = n - step;
            for j in 0..COLOURLESS_UNCOMMON_GAP.saturating_sub(left) {
                recent |= seq[j] & if j as i64 >= gap as i64 - left as i64 { COLOURLESS_BIT } else { !0 };
            }
            let back = if step >= gap { seq[step - gap] } else { 0 };
            let allowed: Vec<usize> = (0..r.umasks.len()).filter(|&i| rem[i] > 0 && r.umasks[i] & recent == 0).collect();
            if allowed.is_empty() {
                continue 'run;
            }
            let weights: Vec<u128> = allowed
                .iter()
                .map(|&i| uncommon_weight(rem[i], r.umasks[i], &left_inc, left, if r.umasks[i] & back != 0 { lane_weight(gap) } else { 1 }))
                .collect();
            let i = allowed[rng.weighted(&weights)];
            seq.push(r.umasks[i]);
            rem[i] -= 1;
            for (b, x) in left_inc.iter_mut().enumerate() {
                if r.umasks[i] >> b & 1 != 0 {
                    *x -= 1;
                }
            }
        }
        return seq;
    }
}

fn window(rng: &mut SplitMix, run: usize, k: usize) -> Vec<usize> {
    let s = rng.below(run as u128) as usize;
    (0..k).map(|j| (s + j) % run).collect()
}

/// `k` commons: a window of a fresh common run.
fn arena_commons(rng: &mut SplitMix, r: &Runs, k: usize) -> Vec<&'static str> {
    let pattern = common_pattern(rng, r);
    let mut out = Vec::with_capacity(k);
    let mut used: Vec<&'static str> = Vec::new();
    for p in window(rng, COMMON_RUN, k) {
        let c = pattern[p];
        if p % GROUP_STEP == 0 {
            out.push(rng.choice(&r.g9[&c]));
        } else {
            let avail: Vec<&'static str> = r.rest[&c].iter().copied().filter(|x| !used.contains(x)).collect();
            let x = rng.choice(&avail);
            used.push(x);
            out.push(x);
        }
    }
    out
}

/// `k` uncommons: a window of a fresh uncommon run.
fn arena_uncommons(rng: &mut SplitMix, r: &Runs, k: usize) -> Vec<&'static str> {
    let pattern = uncommon_pattern(rng, r, if k <= UNCOMMON_GAP { UNCOMMON_GAP } else { UNCOMMON_GAP_5 });
    let mut out: Vec<&'static str> = Vec::with_capacity(k);
    for p in window(rng, UNCOMMON_RUN, k) {
        let g = &r.ugroups[r.umasks.iter().position(|&m| m == pattern[p]).expect("a mask of the run")];
        let avail: Vec<&'static str> = g.iter().copied().filter(|x| !out.contains(x)).collect();
        out.push(rng.choice(&avail));
    }
    out
}

/// A slot's categories: (weight, rarity, printings).
type Cats = Vec<(u64, &'static str, Vec<&'static str>)>;

/// One collation, ready to generate (the reference's `Collation`).
pub struct Generator {
    pub collation: Collation,
    pub paradox_haze: bool,
    commons: Vec<&'static str>,
    uncommons: Vec<&'static str>,
    guests: Vec<&'static str>,
    wildcard: Cats,
    rare: Cats,
    foil: Cats,
    promo: Cats,
}

impl Generator {
    pub fn new(collation: Collation, paradox_haze: bool) -> Generator {
        let sh = sheets(collation == Collation::Paper);
        let cats = |t: Table, total: u64, what: &str| -> Cats {
            assert_eq!(t.iter().map(|x| x.2).sum::<u64>(), total, "{} {what} slot rates", collation.name());
            t.iter()
                .map(|&(groups, rarity, w)| {
                    let printings: Vec<&'static str> = groups
                        .iter()
                        .flat_map(|&g| {
                            let v = sh.get(&(g, rarity)).filter(|v| !v.is_empty());
                            v.unwrap_or_else(|| panic!("{} {what} slot: no {rarity} printings in {g}", collation.name())).iter().copied()
                        })
                        .collect();
                    (w, rarity, printings)
                })
                .collect()
        };
        let (wildcard, rare, foil, promo) = match collation {
            Collation::Paper => (cats(WILDCARD, TOTAL, "wildcard"), cats(RARE, TOTAL, "rare"), cats(FOIL, TOTAL, "foil"), cats(PROMO, 94_600, "promo")),
            Collation::Arena => (cats(ARENA_WILDCARD, TOTAL, "wildcard"), cats(ARENA_RARE, TOTAL, "rare"), cats(ARENA_FOIL, TOTAL, "foil"), Vec::new()),
        };
        if collation == Collation::Arena {
            runs();
        }
        Generator {
            collation,
            paradox_haze,
            commons: sh[&("eoe", "common")].clone(),
            uncommons: sh[&("eoe", "uncommon")].clone(),
            guests: sh[&("spg", "mythic")].clone(),
            wildcard,
            rare,
            foil,
            promo,
        }
    }

    /// The generator of `collation` (cached per collation and Paradox Haze setting).
    pub fn get(collation: Collation, paradox_haze: bool) -> &'static Generator {
        static GENS: [OnceLock<Generator>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
        let i = (collation == Collation::Arena) as usize * 2 + paradox_haze as usize;
        GENS[i].get_or_init(|| Generator::new(collation, paradox_haze))
    }

    fn category<'a>(&'a self, rng: &mut SplitMix, cats: &'a Cats) -> (&'static str, &'a [&'static str]) {
        let w: Vec<u128> = cats.iter().map(|c| c.0 as u128).collect();
        let c = &cats[rng.weighted(&w)];
        (c.1, &c.2)
    }

    fn guest(&self, rng: &mut SplitMix) -> &'static str {
        let mut name = rng.choice(&self.guests);
        while !self.paradox_haze && EXCLUDED.contains(&name) {
            name = rng.choice(&self.guests); // drawn again within the slot
        }
        name
    }

    fn paper_booster(&self, rng: &mut SplitMix) -> Vec<(Slot, &'static str)> {
        let guest = rng.below(TOTAL as u128) < SPECIAL_GUEST as u64;
        let mut out: Vec<(Slot, &'static str)> =
            rng.sample(&self.commons, COMMONS - guest as usize).into_iter().map(|n| (Slot::Common, n)).collect();
        if guest {
            out.push((Slot::SpecialGuest, self.guest(rng)));
        }
        out.extend(rng.sample(&self.uncommons, UNCOMMONS).into_iter().map(|n| (Slot::Uncommon, n)));
        for (slot, cats) in [(Slot::Wildcard, &self.wildcard), (Slot::Rare, &self.rare), (Slot::Foil, &self.foil)] {
            let (_, printings) = self.category(rng, cats);
            out.push((slot, rng.choice(printings)));
        }
        out.push((Slot::Land, rng.choice(&BASICS)));
        out
    }

    fn arena_booster(&self, rng: &mut SplitMix) -> Vec<(Slot, &'static str)> {
        let r = runs();
        let guest = rng.below(TOTAL as u128) < SPECIAL_GUEST as u64;
        let slots = [(Slot::Wildcard, &self.wildcard), (Slot::Rare, &self.rare), (Slot::Foil, &self.foil)];
        let cats: Vec<(Slot, &'static str, &[&'static str])> = slots
            .iter()
            .map(|&(s, c)| {
                let (rarity, printings) = self.category(rng, c);
                (s, rarity, printings)
            })
            .collect();
        let extra = |rarity: &str| -> Vec<Slot> {
            cats.iter().filter(|c| c.0 != Slot::Rare && c.1 == rarity).map(|c| c.0).collect()
        };
        let mut slots_c = vec![Slot::Common; COMMONS - guest as usize];
        slots_c.extend(extra("common"));
        let mut out: Vec<(Slot, &'static str)> = slots_c.iter().copied().zip(arena_commons(rng, r, slots_c.len())).collect();
        if guest {
            out.push((Slot::SpecialGuest, self.guest(rng)));
        }
        let mut slots_u = vec![Slot::Uncommon; UNCOMMONS];
        slots_u.extend(extra("uncommon"));
        out.extend(slots_u.iter().copied().zip(arena_uncommons(rng, r, slots_u.len())));
        let mut names: Vec<&'static str> = out.iter().map(|x| x.1).collect();
        for &(slot, rarity, printings) in &cats {
            if rarity == "common" || rarity == "uncommon" {
                continue;
            }
            let mut name = rng.choice(printings);
            while names.contains(&name) {
                name = rng.choice(printings); // no repeated card in a pack
            }
            names.push(name);
            out.push((slot, name));
        }
        out
    }

    pub fn booster(&self, rng: &mut SplitMix) -> Vec<(Slot, &'static str)> {
        match self.collation {
            Collation::Paper => self.paper_booster(rng),
            Collation::Arena => self.arena_booster(rng),
        }
    }

    pub fn generate(&self, seed: u64, boosters: usize, prerelease_promo: bool) -> Result<SealedPool, String> {
        if prerelease_promo && self.collation != Collation::Paper {
            return Err("the prerelease promo is a paper product".into());
        }
        let mut rng = SplitMix::new(seed);
        let boosters = (0..boosters).map(|_| self.booster(&mut rng)).collect();
        let promo = prerelease_promo.then(|| {
            let (_, printings) = self.category(&mut rng, &self.promo);
            rng.choice(printings)
        });
        Ok(SealedPool { seed, collation: self.collation, boosters, promo })
    }
}

/// A sealed pool of `opts.boosters` EoE Play Boosters from `seed` in `collation` (the reference's
/// `generate`).
pub fn generate(seed: u64, collation: Collation, opts: Options) -> Result<SealedPool, String> {
    Generator::get(collation, opts.paradox_haze).generate(seed, opts.boosters, opts.prerelease_promo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_matches_the_reference_stream() {
        // (tests/test_sealed_collation.py::test_splitmix64_matches_the_reference_stream)
        let mut r = SplitMix::new(0);
        assert_eq!([r.next_u64(), r.next_u64(), r.next_u64()], [0xE220A8397B1DCDAF, 0x6E789E6AA1B965F4, 0x06C45D188009454F]);
    }

    #[test]
    fn the_generator_is_pinned_as_the_reference() {
        // (tests/test_sealed_collation.py::test_the_generator_is_pinned: paper seed 1, booster 0)
        let p = generate(1, Collation::Paper, Options::default()).unwrap();
        let first: Vec<(&str, &str)> = p.boosters[0].iter().take(6).map(|&(s, n)| (s.name(), n)).collect();
        assert_eq!(
            first,
            [
                ("common", "Nebula Dragon"),
                ("common", "Comet Crawler"),
                ("common", "Brightspear Zealot"),
                ("common", "Biosynthic Burst"),
                ("common", "Selfcraft Mechan"),
                ("common", "Nanoform Sentinel"),
            ]
        );
    }

    #[test]
    fn pools_have_78_cards_and_arena_packs_13() {
        for c in [Collation::Paper, Collation::Arena] {
            for seed in 0..50 {
                let p = generate(seed, c, Options::default()).unwrap();
                assert_eq!(p.cards().len(), 78, "{c:?} seed {seed}");
                let size = if c == Collation::Paper { 14 } else { 13 };
                assert!(p.boosters.iter().all(|b| b.len() == size), "{c:?} seed {seed}");
                assert!(!p.cards().contains(&"Paradox Haze"));
            }
        }
        assert!(generate(1, Collation::Arena, Options { prerelease_promo: true, ..Options::default() }).is_err());
        let p = generate(1, Collation::Paper, Options { prerelease_promo: true, ..Options::default() }).unwrap();
        assert_eq!(p.cards().len(), 79);
    }
}
