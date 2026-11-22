//! pb3's sideboarding method for a pairing of two DIFFERENT 75s (ENGINE.md 223): each seat dealt from its own 75
//! (`"dealt_decks"` with `"per_seat"`, ENGINE.md 154, 168.1) and one choice table per deck (ENGINE.md 168.3) - in the
//! fixed two-75 constructed best-of-three spec (`decks.p0_table`, `decks.p1_table`: each seat its own deck's table) and
//! in the metagame spec (a deck entry's `"table"`, `"dealt_decks"`, and the forced find in best-of-three), passed
//! through `Metagame::pair_json` so a metagame match stays exactly its pairing's fixed match (176's lockstep, here with
//! dealt decks, tables played as teacher decisions and the forced find). A deck's table follows the deck to whichever
//! seat plays it; a mirror's is pb3's (both seats one table) whatever form carries it. Every refusal is loud.

use engine::az::constructed::Decklist;
use engine::az::metagame::Metagame;
use engine::az::sbtable::{context_index, CONTEXTS};
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT};
use engine::rng::Rng;
use serde_json::{json, Value};
use zero_search::game::{Next, SearchGame};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn sneak_show() -> Decklist {
    Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap()
}

/// 0 Delver, 1 Doomsday, 2 Sneak and Show (the real 75s of `data/decks/`).
fn deck(k: usize) -> Decklist {
    match k {
        0 => Decklist::izzet_delver(),
        1 => Decklist::legacy_doomsday(),
        _ => sneak_show(),
    }
}

fn side_names(d: &Decklist) -> Vec<String> {
    d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect()
}

/// A 75 as a table's list (first-seen order of main then sideboard), its copies and the main's.
fn list_of(d: &Decklist) -> (Vec<String>, Vec<u8>, Vec<u8>) {
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
    (names, copies, base)
}

/// A choice table of deck `d` (random legal 60s of its 75: the main with random one-for-one swaps), seeded.
fn table_of(d: &Decklist, seed: u64) -> Value {
    let (names, copies, base) = list_of(d);
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

fn per_seat(p: f64, lo: u8, hi: u8) -> Value {
    json!({"p": p, "swaps": [lo, hi], "per_seat": true})
}

fn wheel(p: f64) -> Value {
    json!({"spell": "Doomsday", "card": "Thassa's Oracle", "p": p})
}

/// The fixed two-75 constructed best-of-three of decks `a` (seat 0) and `b` (seat 1), whole matches (`start_game` 1,
/// one episode per game), the keys of `extra` merged in (`decks.*` keys under `decks`).
fn fixed(a: usize, b: usize, extra: Value) -> Value {
    let (da, db) = (deck(a), deck(b));
    let mut v = json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true,
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

/// The best-of-three metagame of `ks` (deck indices) with weights `w`, whole matches, `tables` per deck (None: none),
/// the keys of `extra` merged in.
fn meta(ks: &[usize], w: Value, tables: &[Option<Value>], extra: Value) -> Value {
    let decks: Vec<Value> = ks
        .iter()
        .zip(tables)
        .map(|(&k, t)| {
            let d = deck(k);
            let mut e = json!({"name": d.name, "main": d.main_names(), "sideboard": side_names(&d)});
            if let Some(t) = t {
                e["table"] = t.clone();
            }
            e
        })
        .collect();
    let mut v = json!({"max_turns": 60, "mulligans": true, "constructed": true, "format": "bo3", "game_episodes": true,
        "metagame": {"decks": decks, "weights": w}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn parse(v: &Value) -> Result<MtgSpec, String> {
    <MtgAz as SearchGame>::parse_spec(&v.to_string())
}

fn refused(v: &Value, want: &str) {
    match parse(v) {
        Ok(_) => panic!("accepted: {v}"),
        Err(e) => assert!(e.contains(want), "refusal {e:?} lacks {want:?}"),
    }
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn next(g: &MtgAz) -> Option<MtgAz> {
    if !g.m.game_episodes {
        return None;
    }
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

#[test]
fn per_deck_tables_and_dealt_decks_apply_to_two_75s_and_are_refused_elsewhere() {
    let (t0, t1, t2) = (table_of(&deck(0), 1), table_of(&deck(1), 2), table_of(&deck(2), 3));
    // the fixed two-75 spec: each seat its own deck's table, each seat dealt from its own 75
    let ok = fixed(0, 1, json!({"decks.p0_table": t0, "decks.p1_table": t1, "dealt_decks": per_seat(0.85, 0, 15)}));
    let sp = parse(&ok).unwrap();
    assert_eq!(sp.seat_tables.each_ref().map(|t| t.as_ref().map(|t| t.version.clone())), [Some("t1".into()), Some("t2".into())]);
    let one = parse(&fixed(0, 1, json!({"decks.p1_table": t1}))).unwrap();
    assert!(one.seat_tables[0].is_none() && one.seat_tables[1].is_some(), "one seat's table only");
    refused(&fixed(0, 1, json!({"decks.p0_table": t1})), "decks.p0_table (version \"t2\") is not of p0's 75");
    refused(&fixed(0, 1, json!({"decks.p1_table": t2})), "is not of p1's 75");
    refused(&fixed(0, 0, json!({"decks.p0_table": t0, "sideboard_table": t0})), "not both");
    refused(&fixed(0, 1, json!({"decks.p0_table": t0, "keep_deck": [1]})), "keep_deck");
    let mut bo1 = fixed(0, 1, json!({"decks.p0_table": t0}));
    bo1.as_object_mut().unwrap().remove("format");
    bo1.as_object_mut().unwrap().remove("game_episodes");
    refused(&bo1, "is for a constructed best-of-three");
    refused(&fixed(0, 1, json!({"decks.p0_table": json!({"version": "x"})})), "decks.p0_table: list must be");
    refused(&fixed(0, 1, json!({"decks.p2_table": t0})), "unknown decks key \"p2_table\"");
    // the metagame: a deck entry's table, dealt decks in best-of-three, sideboard_table still refused
    let m = meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), Some(t1.clone())], json!({"dealt_decks": per_seat(0.85, 0, 15)}));
    let ms = parse(&m).unwrap();
    let engine::az::DeckMode::Metagame(mg) = &ms.decks else { panic!("a metagame") };
    assert_eq!(mg.decks.iter().map(|d| d.table.as_ref().unwrap().version.clone()).collect::<Vec<_>>(), ["t1", "t2"]);
    assert!(ms.dealt_decks.unwrap().per_seat);
    refused(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t1.clone()), None], json!({})), "metagame deck \"Izzet Delver\"'s table (version \"t2\") is not of its 75");
    let mut mbo1 = meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), None], json!({}));
    // (best-of-one keeps the decks' sideboards here, so the table is its 75's: the format refuses it)
    for k in ["format", "game_episodes"] {
        mbo1.as_object_mut().unwrap().remove(k);
    }
    refused(&mbo1, "a metagame deck's table is for a constructed best-of-three");
    refused(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[None, None], json!({"sideboard_table": t0})), "metagame with \"sideboard_table\"");
    refused(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), None], json!({"keep_deck": [0]})), "metagame with \"keep_deck\"");
    // dealt decks: every deck of a played pairing must have the range's top in its sideboard (a deck of 10 here)
    let mut small = meta(&[0, 1, 2], json!([[0, 1, 0], [1, 0, 0], [0, 0, 0]]), &[None, None, None], json!({"dealt_decks": per_seat(1.0, 0, 12)}));
    let s10: Vec<Value> = small["metagame"]["decks"][2]["sideboard"].as_array().unwrap()[..10].to_vec();
    small["metagame"]["decks"][2]["sideboard"] = json!(s10);
    parse(&small).expect("a deck of no played pairing is never dealt");
    small["metagame"]["weights"] = json!([[0, 1, 1], [1, 0, 0], [1, 0, 0]]);
    refused(&small, "metagame deck \"Sneak and Show\"'s sideboard has 10 cards");
    // the forced find in best-of-three: some deck's 75 holds Doomsday and Thassa's Oracle
    let ff = parse(&fixed(0, 1, json!({"forced_find": wheel(0.5), "dealt_decks": per_seat(0.85, 0, 15)}))).unwrap();
    assert_eq!(ff.forced_find.unwrap().p(), 0.5);
    parse(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[None, None], json!({"forced_find": wheel(1.0)}))).unwrap();
    refused(&fixed(0, 0, json!({"forced_find": wheel(1.0)})), "no deck's 75 holds both");
    refused(&meta(&[0, 2], json!([[0, 1], [1, 0]]), &[None, None], json!({"forced_find": wheel(1.0)})), "no deck's 75 holds both");
}

#[test]
fn the_evaluation_spec_drops_the_tables_the_deals_and_the_forced_find() {
    let (t0, t1) = (table_of(&deck(0), 1), table_of(&deck(1), 2));
    let extra = json!({"dealt_decks": per_seat(0.85, 0, 15), "forced_find": wheel(1.0)});
    let f = fixed(0, 1, json!({"decks.p0_table": t0, "decks.p1_table": t1, "dealt_decks": per_seat(0.85, 0, 15), "forced_find": wheel(1.0)}));
    let m = meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), Some(t1.clone())], extra);
    for v in [f, m] {
        let e: Value = serde_json::from_str(&MtgSpec::eval_json(&v.to_string()).unwrap()).unwrap();
        for k in ["dealt_decks", "forced_find", "sideboard_table"] {
            assert!(e.get(k).is_none(), "{k} kept");
        }
        assert!(e["decks"].get("p0_table").is_none() && e["decks"].get("p1_table").is_none());
        if let Some(ds) = e["metagame"]["decks"].as_array() {
            assert!(ds.iter().all(|d| d.get("table").is_none()));
        }
        let sp = parse(&e).unwrap();
        assert!(sp.dealt_decks.is_none() && sp.forced_find.is_none() && sp.seat_tables.iter().all(|t| t.is_none()));
        let mut w = v.clone();
        if let Some(d) = w.get_mut("decks").and_then(|d| d.as_object_mut()) {
            d.remove("p0_table");
            d.remove("p1_table");
        }
        if let Some(ds) = w.get_mut("metagame").and_then(|m| m.get_mut("decks")).and_then(|d| d.as_array_mut()) {
            ds.iter_mut().for_each(|d| {
                d.as_object_mut().unwrap().remove("table");
            });
        }
        for k in ["dealt_decks", "forced_find"] {
            w.as_object_mut().unwrap().remove(k);
        }
        assert_eq!(e, w, "only the self-play settings dropped");
    }
}

#[test]
fn pair_json_puts_each_decks_table_on_the_seat_that_plays_it() {
    let (t0, t1) = (table_of(&deck(0), 1), table_of(&deck(1), 2));
    let m = meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), Some(t1.clone())], json!({"dealt_decks": per_seat(0.85, 0, 15), "forced_find": wheel(0.5)}));
    for (p, (a, b)) in [([0u8, 1], (&t0, &t1)), ([1, 0], (&t1, &t0))] {
        let f: Value = serde_json::from_str(&Metagame::pair_json(&m.to_string(), p).unwrap()).unwrap();
        assert_eq!((&f["decks"]["p0_table"], &f["decks"]["p1_table"]), (a, b), "pairing {p:?}");
        assert_eq!((&f["dealt_decks"], &f["forced_find"]), (&m["dealt_decks"], &m["forced_find"]));
        assert!(f.get("metagame").is_none());
        parse(&f).unwrap();
    }
    // a deck without a table: its seat has none in the pairing's fixed spec
    let half = meta(&[0, 1], json!([[0, 1], [1, 0]]), &[None, Some(t1.clone())], json!({}));
    let f: Value = serde_json::from_str(&Metagame::pair_json(&half.to_string(), [0, 1]).unwrap()).unwrap();
    assert!(f["decks"].get("p0_table").is_none() && f["decks"]["p1_table"] == t1);
}

/// The forced find through `pair_json` (ENGINE.md 180, 223): kept for a pairing one of whose decks holds both Doomsday
/// and Thassa's Oracle (in its 75: best of three), left out for one where it could never apply - the engine refuses
/// such a fixed spec, and in the metagame it is inert for that pairing (the fuzz pool meta_pair's lockstep plays it).
#[test]
fn pair_json_leaves_out_a_forced_find_the_pairing_can_never_apply() {
    let m = meta(&[0, 1, 2], json!([[1, 1, 1], [1, 1, 1], [1, 1, 1]]), &[None, None, None], json!({"forced_find": wheel(0.5)}));
    parse(&m).unwrap();
    for (p, kept) in [([0u8, 1], true), ([1, 1], true), ([2, 1], true), ([0, 0], false), ([0, 2], false), ([2, 2], false)] {
        let f: Value = serde_json::from_str(&Metagame::pair_json(&m.to_string(), p).unwrap()).unwrap();
        assert_eq!(f.get("forced_find").is_some(), kept, "pairing {p:?}");
        parse(&f).unwrap_or_else(|e| panic!("pairing {p:?}: {e}"));
        // (the engine's own rule: the same spec with the forced find kept is refused)
        if !kept {
            let mut g = f.clone();
            g["forced_find"] = wheel(0.5);
            assert!(parse(&g).unwrap_err().contains("could never apply"), "pairing {p:?}");
        }
    }
}

#[test]
fn a_decks_table_follows_the_deck_to_its_seat() {
    let (t0, t1) = (table_of(&deck(0), 1), table_of(&deck(1), 2));
    let m = parse(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t0.clone()), Some(t1.clone())], json!({}))).unwrap();
    let want = [parse(&fixed(0, 0, json!({"decks.p0_table": t0}))).unwrap().seat_tables[0].clone(), parse(&fixed(1, 1, json!({"decks.p0_table": t1}))).unwrap().seat_tables[0].clone()];
    let mut seen = [false; 2];
    for seed in 0..40 {
        let g = MtgAz::new(&m, seed).unwrap();
        let p = g.m.pairing.clone().unwrap();
        assert_ne!(p.decks[0], p.decks[1], "weights [[0, 1], [1, 0]]: never a mirror");
        for s in 0..2 {
            assert_eq!(g.m.table[s], want[p.decks[s] as usize], "seed {seed}: seat {s} plays {} and its table", p.names[s]);
        }
        seen[p.decks[0] as usize] = true;
    }
    assert!(seen[0] && seen[1], "both seat orders in 40 matches");
    // a mirror metagame: both seats the one deck's table - pb3's mirror, whatever form carries the table
    let mm = parse(&meta(&[0], json!([[1]]), &[Some(t0.clone())], json!({}))).unwrap();
    let pb3 = parse(&fixed(0, 0, json!({"sideboard_table": t0}))).unwrap();
    for seed in 0..4 {
        let (a, b) = (MtgAz::new(&mm, seed).unwrap(), MtgAz::new(&pb3, seed).unwrap());
        assert!(a.m.table[0].is_some() && a.m.table == b.m.table, "seed {seed}: the mirror's tables");
    }
}

/// The metagame's pairing specs of the stew's first field (Delver against Doomsday, Delver against Sneak and Show,
/// the Delver mirror), with pb3's per-seat deal, a table per deck and, with Doomsday, the forced find.
fn stew_specs(p: f64, wheel_p: f64) -> Vec<Value> {
    let t = [table_of(&deck(0), 11), table_of(&deck(1), 12), table_of(&deck(2), 13)];
    let dealt = json!({"dealt_decks": per_seat(p, 0, 15)});
    let mut dd = dealt.clone();
    dd["forced_find"] = wheel(wheel_p);
    vec![
        meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t[0].clone()), Some(t[1].clone())], dd),
        meta(&[0, 2], json!([[0, 1], [1, 0]]), &[Some(t[0].clone()), Some(t[2].clone())], dealt.clone()),
        meta(&[0], json!([[1]]), &[Some(t[0].clone())], dealt),
    ]
}

#[test]
fn a_pair_metagame_match_with_deals_tables_and_the_forced_find_is_its_pairings_fixed_match() {
    // (ENGINE.md 176.5's lockstep: decision by decision the same decider, legal actions, table actions - played as
    // teacher decisions -, chance keys, both viewers' rows and worlds; the dealt seats and tables the same)
    for (k, m) in stew_specs(0.85, 1.0).iter().enumerate() {
        for seed in 0..scale(6) {
            crate::metagame::lockstep(&m.to_string(), 500 + 10 * k as u64 + seed);
        }
    }
    // the mirror: the metagame with its deck's table IS pb3's spec with its one table (the same matches)
    let t0 = table_of(&deck(0), 11);
    let mm = meta(&[0], json!([[1]]), &[Some(t0.clone())], json!({"dealt_decks": per_seat(0.85, 0, 15)}));
    let pb3 = fixed(0, 0, json!({"sideboard_table": t0, "dealt_decks": per_seat(0.85, 0, 15)}));
    let fixed_mirror: Value = serde_json::from_str(&Metagame::pair_json(&mm.to_string(), [0, 0]).unwrap()).unwrap();
    let (a_sp, b_sp) = (parse(&fixed_mirror).unwrap(), parse(&pb3).unwrap());
    for seed in 0..scale(4) {
        let (mut a, mut b) = (MtgAz::new(&a_sp, seed).unwrap(), MtgAz::new(&b_sp, seed).unwrap());
        let mut rng = Rng::new(seed ^ 0x223);
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        loop {
            assert_eq!(a.m.table, b.m.table);
            let Some(_) = a.to_move() else {
                assert_eq!(b.to_move(), None);
                match (next(&a), next(&b)) {
                    (Some(x), Some(y)) => {
                        a = x;
                        b = y;
                        continue;
                    }
                    (None, None) => break,
                    _ => panic!("seed {seed}: one match over"),
                }
            };
            let (la, lb) = (legal(&a), legal(&b));
            assert_eq!(la, lb);
            assert_eq!(a.table_action(), b.table_action());
            let act = a.table_action().unwrap_or_else(|| la[rng.below(la.len() as u32) as usize]);
            ka.clear();
            kb.clear();
            a.step(act, &mut ka).unwrap();
            b.step(act, &mut kb).unwrap();
            assert_eq!(ka, kb, "seed {seed}: the keys");
        }
    }
}

/// Every table seat (a seat whose deck has a table and that is not dealt) sideboards by teacher decisions - one-hot,
/// legal, at its sideboarding decisions and nowhere else - to its own deck's 60 for its context, against whichever
/// deck the other seat plays; the two decks' tables both reached.
#[test]
fn each_seat_follows_its_own_decks_table_to_its_contexts_60() {
    let t = [table_of(&deck(0), 21), table_of(&deck(1), 22)];
    let m = parse(&meta(&[0, 1], json!([[0, 1], [1, 0]]), &[Some(t[0].clone()), Some(t[1].clone())], json!({"dealt_decks": per_seat(0.5, 0, 15), "max_turns": 12}))).unwrap();
    let db = CardDb::constructed();
    // each deck's context choices in pool order (the pool's list: sorted by definition)
    let by_ctx = |k: usize, g: &MtgAz, p: usize, ctx: usize| -> Vec<u8> {
        let (names, _, _) = list_of(&deck(k));
        let given: Vec<u8> = t[k]["choices"].as_array().unwrap().iter().find(|c| {
            let c = c.as_array().unwrap();
            context_index(c[0].as_u64().unwrap() as u8, c[1].as_u64().unwrap() == 1, c[2].as_i64().unwrap() as i8, c[3].as_u64().unwrap() == 1) == ctx
        }).unwrap()[4].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
        g.m.pools[p].list.iter().map(|&(d, _)| given[names.iter().position(|n| db.id(n) == Some(d)).unwrap()]).collect()
    };
    let (mut reached, mut taught) = ([0u32; 2], 0u32);
    let mut ctxs = std::collections::BTreeSet::new();
    for seed in 0..scale(80) {
        let mut g = MtgAz::new(&m, seed).unwrap();
        let pairing = g.m.pairing.clone().unwrap();
        let mut rng = Rng::new(seed ^ 0x7EAC);
        let mut target: [Option<Vec<u8>>; 2] = [None, None];
        let mut game = g.m.game_no;
        for _ in 0..200_000 {
            if g.m.game_no != game && g.sideboarding().is_none() {
                game = g.m.game_no;
                for q in 0..2 {
                    if let Some(x) = target[q].take() {
                        assert_eq!(&g.m.decks[q][..], &x[..], "seed {seed}: P{q} ({}) game {game}: not its context's 60", pairing.names[q]);
                        reached[pairing.decks[q] as usize] += 1;
                    }
                }
            }
            let Some(p) = g.to_move() else {
                match next(&g) {
                    Some(x) => {
                        g = x;
                        continue;
                    }
                    None => break,
                }
            };
            let l = legal(&g);
            let mut pi = Vec::new();
            let teach = g.teacher_policy(&l, &mut pi);
            let sb = g.sideboarding().is_some();
            assert_eq!(teach, sb, "seed {seed}: a teacher decision exactly at a sideboarding (both decks have a table)");
            assert!(!(sb && g.m.is_dealt(p as u8)), "seed {seed}: a dealt seat decides at a sideboarding");
            let a = if teach {
                assert_eq!(pi.iter().filter(|&&w| w == 1.0).count(), 1, "one-hot");
                let s = g.sideboarding().unwrap();
                let lead = g.m.wins[p] as i8 - g.m.wins[1 - p] as i8;
                let k = context_index(g.m.game_no + 1, s.turn == 0, lead, g.m.is_dealt(1 - p as u8));
                ctxs.insert((pairing.decks[p], k));
                let want = by_ctx(pairing.decks[p] as usize, &g, p, k);
                assert!(target[p].as_ref().is_none_or(|x| *x == want), "seed {seed}: P{p}'s target moved in a sideboarding");
                target[p] = Some(want);
                taught += 1;
                l[pi.iter().position(|&w| w == 1.0).unwrap()]
            } else {
                l[rng.below(l.len() as u32) as usize]
            };
            g.step(a, &mut Vec::new()).unwrap();
        }
    }
    println!("{taught} teacher decisions; table 60s reached: Delver {}, Doomsday {}; {} (deck, context) pairs met", reached[0], reached[1], ctxs.len());
    assert!(reached[0] > 10 && reached[1] > 10 && ctxs.len() >= 8, "reached {reached:?} over {} (deck, context) pairs", ctxs.len());
    let _ = CONTEXTS;
}

/// Both seats dealt from two different 75s (p 1, whole matches): each seat's deal is keyed `KEY_DEALT` with as many keys
/// as its own 75's list needs (public: the 75 is), its counts its own 60 exactly; a world of the other player keys it
/// `HIDDEN` (the same number of keys), the dealt seat's own world in full.
#[test]
fn two_75s_deals_are_keyed_privately_each_by_its_own_75() {
    let s = parse(&fixed(0, 1, json!({"dealt_decks": per_seat(1.0, 0, 15), "max_turns": 12, "game_episodes": false}))).unwrap();
    let (mut checked, mut lens, mut world_ran_short) = (0, [0usize; 2], 0);
    for seed in 0..scale(30) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let n_keys = [0, 1].map(|p| g.m.pools[p].list.len().div_ceil(9));
        lens = [g.m.pools[0].list.len(), g.m.pools[1].list.len()];
        assert_ne!(lens[0], lens[1], "two 75s of different list lengths: the key counts tell them apart");
        let mut rng = Rng::new(seed ^ 0xD3A1);
        for _ in 0..200_000 {
            let Some(_) = g.to_move() else { break };
            let l = legal(&g);
            let a = l[rng.below(l.len() as u32) as usize];
            let mut keys = Vec::new();
            let mut probe = g.clone();
            probe.step(a, &mut keys).unwrap();
            let dk: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
            if !dk.is_empty() {
                for q in 0..2u8 {
                    let mine: Vec<u64> = dk.iter().copied().filter(|&k| key_player(k) == q).collect();
                    assert_eq!(mine.len(), n_keys[q as usize], "seed {seed}: P{q}'s deal keys");
                    // (the payload: 5-bit copies, nine a key, the key's index in bits 45-47)
                    let mut counts = Vec::new();
                    for (j, &k) in mine.iter().enumerate() {
                        let x = key_payload(k);
                        assert_eq!(x >> 45, j as u64);
                        counts.extend((0..9).map(|i| ((x >> (5 * i)) & 31) as u8));
                    }
                    counts.truncate(lens[q as usize]);
                    assert_eq!(&counts[..], &probe.m.decks[q as usize][..], "seed {seed}: P{q}'s keys are its dealt 60");
                }
                for v in 0..2u8 {
                    let mut w = g.clone();
                    w.determinize(v as usize, &mut zero_search::rng::Rng::new(seed ^ 0x77));
                    let mut wk = Vec::new();
                    if !legal(&w).contains(&a) || w.step(a, &mut wk).is_err() {
                        continue;
                    }
                    let wd: Vec<u64> = wk.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
                    if wd.is_empty() {
                        // How far one step runs is hidden information: the real step ran on through forced decisions
                        // to the end of the game (seed 0: P1's mana choice, then nothing but forced passes to the turn
                        // cap), while in a world whose resampled hand gives a player a real choice the step stops there
                        // and the game (and its deal) goes on. Only a world whose step ends the game is checked.
                        world_ran_short += 1;
                        continue;
                    }
                    assert_eq!(w.m.game_no, probe.m.game_no, "seed {seed}: a world that deals starts the real step's next game");
                    for q in 0..2u8 {
                        assert_eq!(wd.iter().filter(|&&k| key_player(k) == q).count(), n_keys[q as usize], "seed {seed}: viewer P{v}'s world keys all of P{q}'s deal");
                    }
                    for k in wd {
                        assert_eq!(key_payload(k) == HIDDEN, key_player(k) != v, "seed {seed}: viewer P{v}'s world keys P{}'s deal", key_player(k));
                    }
                    checked += 1;
                }
            }
            g = probe;
        }
    }
    println!("{checked} worlds of a dealing step checked, {world_ran_short} whose step stopped before the game ended");
    assert!(checked > 0, "no dealing step checked (75 lists of {lens:?} entries)");
}

/// The forced find in best-of-three (ENGINE.md 223): every game of a match carries it, each game's draws from that
/// game's own seed - game 2's draw a share p of matches, independent of game 1's -, the same in every world.
#[test]
fn the_forced_find_draws_each_games_own_in_best_of_three() {
    let sp = parse(&fixed(1, 1, json!({"forced_find": wheel(0.3), "dealt_decks": per_seat(1.0, 0, 15), "start_game": 2}))).unwrap();
    let whole = parse(&fixed(1, 1, json!({"forced_find": wheel(0.3)}))).unwrap();
    let n = scale(3000);
    let (mut g2, mut both) = (0u64, 0u64);
    for seed in 0..n {
        // (both seats dealt at start_game 2: game 2 has begun at once)
        let a = MtgAz::new(&sp, seed).unwrap();
        assert_eq!(a.m.game_no, 2, "seed {seed}: game 2 at once");
        assert_eq!(a.g.forced_find(), sp.forced_find, "seed {seed}: game 2 carries the forced find");
        let d2 = a.g.ff_draw().unwrap();
        let d1 = MtgAz::new(&whole, seed).unwrap().g.ff_draw().unwrap();
        g2 += d2 as u64;
        both += (d1 && d2) as u64;
        let mut w = a.clone();
        w.determinize(0, &mut zero_search::rng::Rng::new(seed));
        assert_eq!(w.g.ff_draw(), Some(d2), "seed {seed}: a world keeps game 2's draw");
    }
    let se = |p: f64| (p * (1.0 - p) / n as f64).sqrt();
    let (s2, sb) = (g2 as f64 / n as f64, both as f64 / n as f64);
    assert!((s2 - 0.3).abs() < 4.0 * se(0.3), "game 2's draws: share {s2}");
    assert!((sb - 0.09).abs() < 4.0 * se(0.09), "games 1 and 2 both drawn: {sb} (independent: 0.09)");
}
