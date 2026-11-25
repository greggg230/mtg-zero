//! The metagame spec (ENGINE.md 176, `az/metagame.rs`): one constructed spec whose games play every ordered pairing of
//! its named 75s, each game's pairing drawn from its seed. A metagame game IS the game of the same seed whose spec names
//! that pairing's lists as `decks` (`Metagame::pair_json`): decision by decision the same decider, legal actions, chance
//! keys, both viewers' whole layout v7 rows, auxiliary targets, the worlds `determinize` deals (their zones and rows) and
//! the result - so the open decklists' pools, the constructed prior, the deck target and the worlds are this game's
//! pairing's. Settings built for one pair of 75s are refused loudly.

use engine::az::constructed::Decklist;
use engine::az::metagame::Metagame;
use engine::az::tail_encode::Row;
use engine::az::{identity, DeckMode, MtgAz, MtgSpec};
use engine::rng::Rng;
use serde_json::{json, Value};
use zero_search::game::{Next, SearchGame};

fn sneak_show() -> Decklist {
    Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap()
}

fn decks() -> [Decklist; 3] {
    [Decklist::izzet_delver(), Decklist::legacy_doomsday(), sneak_show()]
}

fn side_names(d: &Decklist) -> Vec<String> {
    d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect()
}

/// The three-deck metagame's spec value (`decklists.metagame_spec`'s shape): best-of-one the mains, best-of-three the 75s.
fn meta_value(bo3: bool, weights: Option<Value>) -> Value {
    let list: Vec<Value> = decks()
        .iter()
        .map(|d| {
            let mut e = json!({"name": d.name, "main": d.main_names()});
            if bo3 {
                e["sideboard"] = json!(side_names(d));
            }
            e
        })
        .collect();
    let mut mg = json!({"decks": list});
    if let Some(w) = weights {
        mg["weights"] = w;
    }
    let mut v = json!({"max_turns": 60, "mulligans": true, "constructed": true, "metagame": mg});
    if bo3 {
        v["format"] = json!("bo3");
        v["game_episodes"] = json!(true);
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

#[test]
fn the_pairing_is_drawn_from_the_seed_uniform_over_the_nine_cells_by_default() {
    let sp = parse(&meta_value(false, None)).unwrap();
    let DeckMode::Metagame(m) = &sp.decks else { panic!("a metagame") };
    assert_eq!((m.len(), m.total, m.weights.len()), (3, 9, 9));
    let mut cells = [0u32; 9];
    for seed in 0..9000u64 {
        let p = sp.pairing(seed).unwrap();
        assert_eq!(p, sp.pairing(seed).unwrap(), "deterministic");
        cells[(p.decks[0] * 3 + p.decks[1]) as usize] += 1;
        // the seats are the pairing's lists
        let seats = sp.seats(seed);
        let ds = decks();
        for s in 0..2 {
            let mut want = Decklist::ids(engine::db::CardDb::constructed(), &ds[p.decks[s] as usize].main).unwrap();
            want.sort_unstable();
            let mut got = seats[s].deck.clone();
            got.sort_unstable();
            assert_eq!(got, want, "seed {seed}: seat {s}'s deck is the pairing's main");
            assert_eq!(&*p.names[s], ds[p.decks[s] as usize].name);
        }
    }
    // 1,000 expected per cell (sd ~30)
    assert!(cells.iter().all(|&c| (850..1150).contains(&c)), "cells {cells:?}");
    // an existing spec has no pairing
    let fixed = parse(&json!({"constructed": true, "decks": {"p0": decks()[0].main_names(), "p1": decks()[1].main_names()}})).unwrap();
    assert_eq!(fixed.pairing(3), None);
}

#[test]
fn weights_choose_the_cells_exactly() {
    // only the mirrors, Doomsday's twice as often as the others
    let sp = parse(&meta_value(false, Some(json!([[1, 0, 0], [0, 2, 0], [0, 0, 1]])))).unwrap();
    let mut n = [0u32; 3];
    for seed in 0..4000u64 {
        let p = sp.pairing(seed).unwrap().decks;
        assert_eq!(p[0], p[1], "seed {seed}: a weight 0 cell played");
        n[p[0] as usize] += 1;
    }
    assert!((1850..2150).contains(&n[1]) && (850..1150).contains(&n[0]) && (850..1150).contains(&n[2]), "{n:?}");
    // a single cell: every game that pairing (Sneak and Show against Delver)
    let one = parse(&meta_value(false, Some(json!([[0, 0, 0], [0, 0, 0], [5, 0, 0]])))).unwrap();
    assert!((0..200).all(|s| one.pairing(s).unwrap().decks == [2, 0]));
}

#[test]
fn unsupported_combinations_are_refused_loudly() {
    let v = meta_value(false, None);
    let with = |k: &str, x: Value| {
        let mut w = v.clone();
        w[k] = x;
        w
    };
    refused(&with("decks", json!({"p0": [], "p1": []})), "metagame with \"decks\"");
    refused(&with("seed", json!(7)), "metagame with \"seed\"");
    refused(&with("hidden_decklists", json!(false)), "metagame with \"hidden_decklists\"");
    refused(&with("pools", json!(false)), "metagame with \"pools\"");
    let mut nc = v.clone();
    nc["constructed"] = json!(false);
    refused(&nc, "needs \"constructed\": true");
    let mut nc = v.clone();
    nc.as_object_mut().unwrap().remove("constructed");
    refused(&nc, "needs \"constructed\": true");
    let b3 = meta_value(true, None);
    let with3 = |k: &str, x: Value| {
        let mut w = b3.clone();
        w[k] = x;
        w
    };
    // (dealt decks are a metagame's in best-of-three since ENGINE.md 240: each seat from its own 75 - the per-deck
    // settings' test below; best-of-one refuses them as any spec does)
    refused(&with("dealt_decks", json!({"p": 0.5, "swaps": [0, 4]})), "dealt_decks is for a constructed best-of-three");
    refused(&with3("keep_deck", json!([0])), "metagame with \"keep_deck\"");
    refused(&with3("keep_known", json!(true)), "metagame with \"keep_known\"");
    refused(&with3("sideboard_table", json!({})), "metagame with \"sideboard_table\"");
    // start_game 2: mirrors only (game 1's result a fair coin, ENGINE.md 147.4)
    refused(&with3("start_game", json!(2)), "start_game 2 is for mirrors");
    let mut mirrors = meta_value(true, Some(json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]])));
    mirrors["start_game"] = json!(2);
    let sp = parse(&mirrors).unwrap();
    assert_eq!(sp.start_game, 2);
    // the metagame's own errors
    let mut bad = v.clone();
    bad["metagame"]["weights"] = json!([[1, 1], [1, 1]]);
    refused(&bad, "3 x 3 matrix");
    bad["metagame"]["weights"] = json!([[0, 0, 0], [0, 0, 0], [0, 0, 0]]);
    refused(&bad, "all 0");
    bad["metagame"]["weights"] = json!([[1, 1, 1], [1, -1, 1], [1, 1, 1]]);
    refused(&bad, "non-negative integers");
    bad["metagame"]["weights"] = json!([[1, 1, 1], [1, 1.5, 1], [1, 1, 1]]);
    refused(&bad, "non-negative integers");
    let mut dup = v.clone();
    dup["metagame"]["decks"][1]["name"] = dup["metagame"]["decks"][0]["name"].clone();
    refused(&dup, "twice");
    let mut extra = v.clone();
    extra["metagame"]["format"] = json!("bo1");
    refused(&extra, "unknown metagame key");
    let mut key = v.clone();
    key["metagame"]["decks"][0]["side"] = json!([]);
    refused(&key, "unknown key \"side\"");
    // (ENGINE.md 272: a best-of-three main of at least 60, CR 100.2a: a 61 is legal there too, a 59 refused)
    let mut long = b3.clone();
    let mut m: Vec<Value> = long["metagame"]["decks"][2]["main"].as_array().unwrap().clone();
    m.push(json!("Island"));
    long["metagame"]["decks"][2]["main"] = json!(m);
    parse(&long).unwrap();
    let mut short = b3.clone();
    let mut m: Vec<Value> = short["metagame"]["decks"][2]["main"].as_array().unwrap().clone();
    m.truncate(59);
    short["metagame"]["decks"][2]["main"] = json!(m);
    refused(&short, "59 cards in the deck (CR 100.2a: at least 60)");
    let mut unknown = v.clone();
    unknown["metagame"]["decks"][0]["main"][0] = json!("Not A Card");
    unknown["metagame"]["decks"][0]["main"][1] = json!("Not A Card Either");
    refused(&unknown, "2 cards the card table does not know");
    let mut five = v.clone();
    let mut m: Vec<Value> = five["metagame"]["decks"][0]["main"].as_array().unwrap().clone();
    m.push(json!("Brainstorm"));
    five["metagame"]["decks"][0]["main"] = json!(m);
    refused(&five, "copies of Brainstorm");
}

#[test]
fn the_evaluation_spec_keeps_the_metagame() {
    let v = meta_value(false, None);
    let e: Value = serde_json::from_str(&MtgSpec::eval_json(&v.to_string()).unwrap()).unwrap();
    assert_eq!(e, v, "no \"decks\": \"eval\" added to a metagame");
}

/// Both viewers' rows, the auxiliary targets and two worlds (their zones and the viewer's rows) of `a` and `b` agree.
fn same_position(a: &MtgAz, b: &MtgAz, seed: u64, step: u32) {
    let what = || format!("seed {seed} step {step}");
    for v in 0..2 {
        assert_eq!(Row::of_az(a, v), Row::of_az(b, v), "{}: P{v}'s row", what());
    }
    let Some(p) = a.to_move() else { return };
    if step % 9 == 4 && a.m.sb.is_none() {
        let w = MtgAz::aux_width();
        let (mut x, mut y) = (vec![0.0; w], vec![0.0; w]);
        a.aux_target(p, &mut x);
        b.aux_target(p, &mut y);
        assert_eq!(x, y, "{}: the auxiliary targets", what());
        assert!(w == 2 * identity::D);
    }
    if step % 6 == 1 {
        for viewer in 0..2 {
            let (mut da, mut db) = (a.clone(), b.clone());
            let k = seed.wrapping_mul(0x9E37_79B9) ^ (step as u64) << 8 ^ viewer as u64;
            da.determinize(viewer, &mut zero_search::rng::Rng::new(k));
            db.determinize(viewer, &mut zero_search::rng::Rng::new(k));
            assert_eq!(da.m.pools, db.m.pools, "{}: P{viewer}'s world's pools", what());
            assert_eq!(da.m.decks, db.m.decks, "{}: P{viewer}'s world's decks", what());
            assert_eq!(da.m.pairing, a.m.pairing, "{}: a world keeps the pairing", what());
            for q in 0..2u8 {
                let ids = |g: &engine::game::Game| -> Vec<u16> {
                    let pl = g.player(q);
                    pl.hand.iter().chain(&pl.library).map(|&c| g.obj(c).def).collect()
                };
                assert_eq!(ids(&da.g), ids(&db.g), "{}: P{viewer}'s world's P{q} hand and library", what());
            }
            assert_eq!(Row::of_az(&da, viewer), Row::of_az(&db, viewer), "{}: P{viewer}'s row in its world", what());
        }
    }
}

/// Play the metagame spec `meta` (JSON) and its pairing's fixed spec in lockstep from `seed` with random actions; every
/// position the same (`same_position`). The pairing played.
pub(crate) fn lockstep(meta: &str, seed: u64) -> [u8; 2] {
    let ms = MtgSpec::parse(meta).unwrap();
    let mut a = MtgAz::new_game(&ms, 2, seed).unwrap();
    let pairing = a.m.pairing.clone().expect("a metagame match has its pairing");
    assert_eq!(Some(&pairing), ms.pairing(seed).as_ref());
    let fixed = Metagame::pair_json(meta, pairing.decks).unwrap();
    let fs = MtgSpec::parse(&fixed).unwrap();
    let mut b = MtgAz::new_game(&fs, 2, seed).unwrap();
    assert_eq!(b.m.pairing, None);
    // (ENGINE.md 240: the per-deck settings follow the pairing - each seat's dealt flag and choice table)
    assert_eq!([a.m.is_dealt(0), a.m.is_dealt(1)], [b.m.is_dealt(0), b.m.is_dealt(1)], "seed {seed}: the dealt seats");
    assert_eq!(a.m.table, b.m.table, "seed {seed}: the seats' choice tables");
    // the game record names the pairing; otherwise it is the fixed game's
    let (ra, rb): (Value, Value) = (serde_json::from_str(&a.record_setup()).unwrap(), serde_json::from_str(&b.record_setup()).unwrap());
    assert_eq!(ra["decks"], rb["decks"]);
    assert_eq!(ra["pairing"], pairing.json());
    assert!(rb.get("pairing").is_none());
    let view: Value = serde_json::from_str(&a.spectator_json()).unwrap();
    assert_eq!(view["match"]["pairing"], pairing.json());
    let mut rng = Rng::new(seed ^ 0x3E7A);
    let (mut la, mut lb, mut ka, mut kb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut step = 0u32;
    loop {
        same_position(&a, &b, seed, step);
        let Some(p) = a.to_move() else {
            assert_eq!(b.to_move(), None);
            assert_eq!(a.result(), b.result(), "seed {seed}: the result");
            if !a.chained() {
                break;
            }
            match (a.next_episode(None).unwrap(), b.next_episode(None).unwrap()) {
                (Next::Continue(x), Next::Continue(y)) => {
                    assert_eq!(x.m.pairing, a.m.pairing, "the next episode keeps the pairing");
                    a = x;
                    b = y;
                    continue;
                }
                (Next::Over(x), Next::Over(y)) => {
                    assert_eq!(x, y);
                    break;
                }
                _ => panic!("seed {seed}: one match over, the other not"),
            }
        };
        assert_eq!(b.to_move(), Some(p), "seed {seed} step {step}: the decider");
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        assert_eq!(la, lb, "seed {seed} step {step}: the legal actions");
        // (ENGINE.md 240: a seat's choice table - the same in both games, played as zero-core's driver plays a
        // teacher decision; every other decision at random)
        let taught = a.table_action();
        assert_eq!(taught, b.table_action(), "seed {seed} step {step}: the table's action");
        let act = taught.unwrap_or_else(|| la[rng.below(la.len() as u32) as usize]);
        ka.clear();
        kb.clear();
        a.step(act, &mut ka).unwrap();
        b.step(act, &mut kb).unwrap();
        assert_eq!(ka, kb, "seed {seed} step {step}: the chance keys");
        step += 1;
        assert!(step < 200_000);
    }
    pairing.decks
}

#[test]
fn a_best_of_one_metagame_game_is_its_pairings_fixed_game() {
    let meta = meta_value(false, None).to_string();
    // at least 27 games, and on until every cell has been played
    let mut seen = [false; 9];
    let mut seed = 0;
    while seed < 27 || !seen.iter().all(|&s| s) {
        let [i, j] = lockstep(&meta, seed);
        seen[(i * 3 + j) as usize] = true;
        seed += 1;
        assert!(seed < 200, "not every cell in 200 games: {seen:?}");
    }
}

#[test]
fn a_best_of_three_metagame_match_is_its_pairings_fixed_match() {
    // game 1 the mains (open), then sideboarding and the hidden 60s dealt by each 75's own constructed prior
    let meta = meta_value(true, None).to_string();
    for seed in 0..9 {
        lockstep(&meta, 100 + seed);
    }
    // post-sideboard matches of the mirrors (start_game 2)
    let mut m = meta_value(true, Some(json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]])));
    m["start_game"] = json!(2);
    for seed in 0..4 {
        lockstep(&m.to_string(), 200 + seed);
    }
}
