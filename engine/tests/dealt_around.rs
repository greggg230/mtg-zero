//! Dealt decks around the seat's choice table (ENGINE.md 261, the spec's `dealt_decks.around`): `"main"` (the default)
//! is ENGINE.md 154's deal, byte for byte; `"table"` deals each dealt 60 with its swaps counted from the 60 the seat's
//! choice table picks for the deal's context (the next game, whether the seat sideboards first, its game wins less the
//! other's, whether the other seat is dealt) - the main for a seat without a table. The hidden-information rules of the
//! deal are 154's: the deal keyed `KEY_DEALT` of the dealt seat, `HIDDEN` for the other observer; the other player's
//! worlds deal the dealt 60s from the prior around the same (public) base, the next 60 between games around the next
//! game's base; `determinize` reseeds the dealt seed.

use engine::az::constructed::Decklist;
use engine::az::metagame::Metagame;
use engine::az::{DealtAround, MtgAz, MtgSpec};
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT};
use engine::rng::Rng;
use serde_json::{json, Value};
use zero_search::game::{Next, SearchGame};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

/// 0 Delver, 1 Doomsday (the real 75s of `data/decks/`).
fn deck(k: usize) -> Decklist {
    if k == 0 {
        Decklist::izzet_delver()
    } else {
        Decklist::legacy_doomsday()
    }
}

fn side_names(d: &Decklist) -> Vec<String> {
    d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect()
}

/// A choice table of deck `d`: each context the main with 1..=12 random one-for-one swaps (seeded).
fn table_of(d: &Decklist, seed: u64) -> Value {
    let (main, side) = (d.main_names(), side_names(d));
    let (mut names, mut copies, mut base): (Vec<String>, Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new(), Vec::new());
    for (i, n) in main.iter().chain(&side).enumerate() {
        let k = names.iter().position(|x| x == n).unwrap_or_else(|| {
            names.push(n.clone());
            copies.push(0);
            base.push(0);
            names.len() - 1
        });
        copies[k] += 1;
        if i < main.len() {
            base[k] += 1;
        }
    }
    let mut r = Rng::new(seed);
    let mut choices = Vec::new();
    for game in 2..=3u8 {
        for first in 0..2u8 {
            for lead in -1..=1i8 {
                for od in 0..2u8 {
                    let mut c = base.clone();
                    for _ in 0..1 + r.below(12) {
                        let out: Vec<usize> = (0..c.len()).filter(|&k| c[k] > 0).collect();
                        let inn: Vec<usize> = (0..c.len()).filter(|&k| c[k] < copies[k]).collect();
                        let (o, i) = (out[r.below(out.len() as u32) as usize], inn[r.below(inn.len() as u32) as usize]);
                        if o != i {
                            c[o] -= 1;
                            c[i] += 1;
                        }
                    }
                    choices.push(json!([game, first, lead, od, c]));
                }
            }
        }
    }
    let list: Vec<Value> = names.iter().zip(&copies).map(|(n, k)| json!([n, k])).collect();
    json!({"version": format!("t{seed}"), "list": list, "choices": choices})
}

/// The fixed two-75 best-of-three of decks `a` (seat 0) and `b` (seat 1), whole matches, `extra` merged in (`decks.*`
/// keys under `decks`).
fn fixed(a: usize, b: usize, extra: Value) -> Value {
    let (da, db) = (deck(a), deck(b));
    let mut v = json!({"max_turns": 12, "constructed": true, "format": "bo3",
        "decks": {"p0": da.main_names(), "p1": db.main_names(), "p0_sideboard": side_names(&da), "p1_sideboard": side_names(&db)}});
    for (k, x) in extra.as_object().unwrap() {
        if let Some(dk) = k.strip_prefix("decks.") {
            v["decks"][dk] = x.clone();
        } else {
            v[k] = x.clone();
        }
    }
    v
}

/// The best-of-three metagame Delver against Doomsday (either seat), whole matches, one episode per game, `tables` per
/// deck, `extra` merged in.
fn meta(tables: [Option<Value>; 2], extra: Value) -> Value {
    let decks: Vec<Value> = (0..2)
        .map(|k| {
            let d = deck(k);
            let mut e = json!({"name": d.name, "main": d.main_names(), "sideboard": side_names(&d)});
            if let Some(t) = &tables[k] {
                e["table"] = t.clone();
            }
            e
        })
        .collect();
    let mut v = json!({"max_turns": 12, "constructed": true, "format": "bo3", "game_episodes": true,
        "metagame": {"decks": decks, "weights": [[0, 1], [1, 0]]}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn dd(p: f64, lo: u8, hi: u8, per_seat: bool, around: Option<&str>) -> Value {
    let mut v = json!({"p": p, "swaps": [lo, hi], "per_seat": per_seat});
    if let Some(a) = around {
        v["around"] = json!(a);
    }
    v
}

fn parse(v: &Value) -> Result<MtgSpec, String> {
    <MtgAz as SearchGame>::parse_spec(&v.to_string())
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// The swaps of 60 `a` from 60 `b` (copies per entry of one 75): its copies beyond `b`'s.
fn dist(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).map(|(&x, &y)| x.saturating_sub(y) as usize).sum()
}

/// Seat `q`'s game wins less the other's before game `game` (from the results of the games before it).
fn lead_before(g: &MtgAz, q: usize, game: u8) -> i8 {
    let mut w = [0i8; 2];
    for i in 0..(game as usize - 1) {
        if let r @ (1 | 2) = g.m.results[i] {
            w[(r - 1) as usize] += 1;
        }
    }
    w[q] - w[1 - q]
}

/// The base of seat `q`'s dealt 60 for game `game` (computed here from the match's course): its table's 60 for the
/// context - `first`: it sideboarded first before that game (that game's chooser, CR 103.1) -, else `main`.
fn base_of(g: &MtgAz, q: usize, game: u8, first: bool, main: &[u8]) -> Vec<u8> {
    match &g.m.table[q] {
        Some(t) => t.choice(game, first, lead_before(g, q, game), g.m.is_dealt(1 - q as u8)).to_vec(),
        None => main.to_vec(),
    }
}

/// Every step of match `seed` of `s` by random actions from the seed: (the action, its keys, both decks after it).
fn trace(s: &MtgSpec, seed: u64) -> Vec<(u16, Vec<u64>, [Vec<u8>; 2])> {
    let mut g = MtgAz::new(s, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0xA70D);
    let mut out = Vec::new();
    for _ in 0..200_000 {
        if g.to_move().is_none() {
            if !g.m.game_episodes {
                break;
            }
            match g.next_episode(None).unwrap() {
                Next::Continue(n) => {
                    g = n;
                    out.push((u16::MAX, Vec::new(), [g.m.decks[0].to_vec(), g.m.decks[1].to_vec()]));
                    continue;
                }
                Next::Over(_) => break,
            }
        }
        let l = legal(&g);
        let a = l[rng.below(l.len() as u32) as usize];
        let mut keys = Vec::new();
        g.step(a, &mut keys).unwrap();
        out.push((a, keys, [g.m.decks[0].to_vec(), g.m.decks[1].to_vec()]));
    }
    out
}

#[test]
fn around_is_main_or_table_and_anything_else_is_refused() {
    let base = |a: Option<&str>| fixed(0, 1, json!({"dealt_decks": dd(0.5, 1, 8, true, a)}));
    assert_eq!(parse(&base(None)).unwrap().dealt_decks.unwrap().around, DealtAround::Main);
    assert_eq!(parse(&base(Some("main"))).unwrap().dealt_decks.unwrap().around, DealtAround::Main);
    assert_eq!(parse(&base(Some("table"))).unwrap().dealt_decks.unwrap().around, DealtAround::Table);
    for bad in [json!("Table"), json!("tables"), json!(1), json!(null), json!(true)] {
        let mut v = base(None);
        v["dealt_decks"]["around"] = bad.clone();
        let e = parse(&v).err().unwrap_or_else(|| panic!("around {bad} accepted"));
        assert!(e.contains("dealt_decks.around"), "{e}");
    }
    // around "table" without a table in the spec is no error (every seat's base is then its main)
    assert!(parse(&meta([None, None], json!({"dealt_decks": dd(1.0, 0, 15, true, Some("table"))}))).is_ok());
    // the pairing's fixed spec carries it; the evaluation spec drops the dealt decks, around with them
    let m = meta([Some(table_of(&deck(0), 1)), Some(table_of(&deck(1), 2))], json!({"dealt_decks": dd(0.85, 0, 15, true, Some("table"))}));
    let f = parse(&serde_json::from_str(&Metagame::pair_json(&m.to_string(), [1, 0]).unwrap()).unwrap()).unwrap();
    assert_eq!(f.dealt_decks.unwrap().around, DealtAround::Table);
    let ev: Value = serde_json::from_str(&MtgSpec::eval_json(&m.to_string()).unwrap()).unwrap();
    assert!(ev.get("dealt_decks").is_none());
}

/// `"around": "main"` and `"around": "table"` with no table for any seat are ENGINE.md 154's deal byte for byte: the
/// same actions, keys (the deals' `KEY_DEALT` included) and decks at every step as the spec without the key.
#[test]
fn around_main_and_around_table_without_tables_are_the_old_deal_byte_for_byte() {
    for (p, lo, hi, per_seat) in [(1.0, 0, 15, true), (0.5, 1, 8, true), (1.0, 1, 8, false)] {
        let specs = [None, Some("main"), Some("table")].map(|a| parse(&fixed(0, 1, json!({"dealt_decks": dd(p, lo, hi, per_seat, a)}))).unwrap());
        let mspecs = [None, Some("main"), Some("table")].map(|a| parse(&meta([None, None], json!({"dealt_decks": dd(p, lo, hi, per_seat, a)}))).unwrap());
        let mut dealt = 0;
        for seed in 0..scale(12) {
            for sp in [&specs, &mspecs] {
                let t0 = trace(&sp[0], seed);
                dealt += t0.iter().flat_map(|x| &x.1).filter(|&&k| key_kind(k) == KEY_DEALT).count();
                assert!(t0 == trace(&sp[1], seed), "seed {seed}: around main differs from the default");
                assert!(t0 == trace(&sp[2], seed), "seed {seed}: around table without tables differs from the default");
            }
        }
        assert!(dealt > 0, "no deal keyed");
    }
}

/// Around the table: every dealt 60 is within the range of swaps from its seat's table's 60 for the deal's context
/// (computed here from the match's course), the swap counts cover the range, and the dealt 60s leave the main's range
/// (they are not dealt around the main); a seat whose deck has no table is dealt around its main.
#[test]
fn a_dealt_60_is_within_the_range_of_its_tables_choice_for_the_context() {
    let (lo, hi) = (1u8, 4u8);
    let tables = [Some(table_of(&deck(0), 31)), Some(table_of(&deck(1), 32))];
    for with in [[true, true], [true, false]] {
        let t = [0, 1].map(|k| if with[k] { tables[k].clone() } else { None });
        let s = parse(&meta(t, json!({"dealt_decks": dd(0.7, lo, hi, true, Some("table"))}))).unwrap();
        let (mut hist, mut off_main, mut checked, mut no_table) = ([0u32; 16], 0u32, 0u32, 0u32);
        let mut ctxs = std::collections::BTreeSet::new();
        for seed in 0..scale(60) {
            let mut g = MtgAz::new(&s, seed).unwrap();
            let pairing = g.m.pairing.clone().unwrap();
            let mains = [0, 1].map(|q| g.m.decks[q].to_vec());
            let mut rng = Rng::new(seed ^ 0x5EED);
            let mut game = g.m.game_no;
            for _ in 0..200_000 {
                if g.m.game_no != game && g.sideboarding().is_none() {
                    game = g.m.game_no;
                    for q in 0..2usize {
                        if !g.m.is_dealt(q as u8) {
                            continue;
                        }
                        let first = g.g.turn().chooser == q as u8;
                        let base = base_of(&g, q, game, first, &mains[q]);
                        let k = dist(&g.m.decks[q], &base);
                        assert!((lo as usize..=hi as usize).contains(&k), "seed {seed} game {game}: P{q}'s dealt 60 {k} swaps from its base");
                        hist[k] += 1;
                        off_main += (dist(&g.m.decks[q], &mains[q]) > hi as usize) as u32;
                        checked += 1;
                        if g.m.table[q].is_none() {
                            assert_eq!(base, mains[q]);
                            no_table += 1;
                        } else {
                            assert!(with[pairing.decks[q] as usize], "a table on a deck without one");
                            ctxs.insert((pairing.decks[q], game, first, lead_before(&g, q, game), g.m.is_dealt(1 - q as u8)));
                        }
                    }
                }
                let Some(_) = g.to_move() else {
                    match g.next_episode(None).unwrap() {
                        Next::Continue(n) => {
                            g = n;
                            continue;
                        }
                        Next::Over(_) => break,
                    }
                };
                let l = legal(&g);
                let a = l[rng.below(l.len() as u32) as usize];
                g.step(a, &mut Vec::new()).unwrap();
            }
        }
        println!("tables {with:?}: {checked} dealt 60s, swaps from the base {:?}, {off_main} past the range from the main, {no_table} of a seat without a table, {} contexts", &hist[..=hi as usize], ctxs.len());
        for k in lo..=hi {
            assert!(hist[k as usize] * 8 > checked, "swap count {k}: {} of {checked} (uniform over {lo}..={hi})", hist[k as usize]);
        }
        assert!(off_main > 0 && ctxs.len() >= 6, "off main {off_main}, contexts {}", ctxs.len());
        assert_eq!(no_table > 0, !with[1], "a seat without a table dealt around its main");
    }
}

/// The deal's hidden information (ENGINE.md 154.2, unchanged by the base): the dealt seat's deal is keyed `KEY_DEALT`
/// with its 60 exactly, a world of the other player keys it `HIDDEN` (the same number of keys), the dealt seat's own
/// world in full; the other player's worlds deal the dealt seat's 60 around the same base - in a game the game's, between
/// games the last game's and (when it sideboarded first) the next game's -, within the range; every world reseeds the
/// dealt seed.
#[test]
fn the_other_players_worlds_deal_around_the_same_base_and_key_the_deal_hidden() {
    let (lo, hi) = (1u8, 3u8);
    let s = parse(&fixed(0, 1, json!({"dealt_decks": dd(1.0, lo, hi, false, Some("table")),
        "decks.p0_table": table_of(&deck(0), 41), "decks.p1_table": table_of(&deck(1), 42)}))).unwrap();
    let (mut keyed, mut in_game, mut last, mut next, mut reseeded, mut worlds) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    for seed in 0..scale(24) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.expect("p 1: one dealt seat") as usize;
        let v = 1 - d;
        let main_d = g.m.decks[d].to_vec();
        let n_keys = g.m.pools[d].list.len().div_ceil(9);
        let mut rng = Rng::new(seed ^ 0xD1D1);
        for step in 0..200_000u32 {
            let Some(_) = g.to_move() else { break };
            // the other player's worlds (every 7th step: they are costly)
            if g.m.game_no >= 2 && step % 7 == 0 {
                for w_seed in 0..2u64 {
                    let mut w = g.clone();
                    w.determinize(v, &mut zero_search::rng::Rng::new(seed ^ w_seed << 32 ^ step as u64));
                    worlds += 1;
                    reseeded += (w.m.dealt_seed != g.m.dealt_seed) as u32;
                    let game = g.m.game_no;
                    match g.sideboarding() {
                        None => {
                            let base = base_of(&g, d, game, g.g.turn().chooser == d as u8, &main_d);
                            let k = dist(&w.m.decks[d], &base);
                            assert!((lo as usize..=hi as usize).contains(&k), "seed {seed} game {game}: a world's dealt 60 {k} swaps from the game's base");
                            in_game += 1;
                        }
                        Some(sb) => {
                            let base = base_of(&g, d, game, g.m.choosers[game as usize - 1] == d as u8, &main_d);
                            let k = dist(&w.m.decks[d], &base);
                            assert!((lo as usize..=hi as usize).contains(&k), "seed {seed} after game {game}: a world's last dealt 60 {k} swaps from that game's base");
                            last += 1;
                            if let Some(x) = &w.sideboarding().unwrap().done[d] {
                                let nb = base_of(&g, d, game + 1, sb.order[0] == d as u8, &main_d);
                                let k = dist(x, &nb);
                                assert!((lo as usize..=hi as usize).contains(&k), "seed {seed} before game {}: a world's next dealt 60 {k} swaps from the next base", game + 1);
                                next += 1;
                            }
                        }
                    }
                }
            }
            let l = legal(&g);
            let a = l[rng.below(l.len() as u32) as usize];
            let mut keys = Vec::new();
            let mut probe = g.clone();
            probe.step(a, &mut keys).unwrap();
            let dk: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
            if !dk.is_empty() {
                assert!(dk.iter().all(|&k| key_player(k) == d as u8) && dk.len() == n_keys, "seed {seed}: the deal's keys");
                let mut counts = Vec::new();
                for (j, &k) in dk.iter().enumerate() {
                    let x = key_payload(k);
                    assert_eq!(x >> 45, j as u64);
                    counts.extend((0..9).map(|i| ((x >> (5 * i)) & 31) as u8));
                }
                counts.truncate(g.m.pools[d].list.len());
                let dealt = probe.sideboarding().and_then(|sb| sb.done[d].clone()).map_or_else(|| probe.m.decks[d].to_vec(), |x| x.to_vec());
                assert_eq!(counts, dealt, "seed {seed}: the keys are the dealt 60");
                for viewer in 0..2usize {
                    let mut w = g.clone();
                    w.determinize(viewer, &mut zero_search::rng::Rng::new(seed ^ 0x77 ^ step as u64));
                    let mut wk = Vec::new();
                    if !legal(&w).contains(&a) || w.step(a, &mut wk).is_err() {
                        continue;
                    }
                    let wd: Vec<u64> = wk.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
                    if wd.is_empty() {
                        // (how far a step runs is hidden information: ENGINE.md 240.7)
                        continue;
                    }
                    assert_eq!(wd.len(), n_keys);
                    for k in wd {
                        assert_eq!(key_payload(k) == HIDDEN, viewer != d, "seed {seed}: viewer P{viewer}'s world keys the deal");
                    }
                    keyed += 1;
                }
            }
            g = probe;
        }
    }
    println!("{keyed} worlds keyed a deal; the other player's worlds: {in_game} in a game, {last} last 60s and {next} next 60s between games; {reseeded} of {worlds} reseeded");
    assert!(keyed > 0 && in_game > 0 && last > 0 && next > 0, "keyed {keyed}, in game {in_game}, last {last}, next {next}");
    assert_eq!(reseeded, worlds, "a world kept the real dealt seed");
}
