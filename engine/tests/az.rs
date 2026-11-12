//! The zero-core game `mtg` (ENGINE.md 21): the `SearchGame` contract (the
//! toy game's tests in zero-core, crates/search/src/toy/tests.rs): games end
//! with sane results, clones continue identically, `determinize` keeps the
//! viewer's features and legal actions for both viewers, chance keys are
//! canonical (equal keys, decider and actions mean equal features),
//! `force_chance` replays draws and looks and stays aligned; the encoding's
//! slots name what the action slots refer to; decks; the bots.

use engine::az::bots::Heuristic;
use engine::az::decks::{self, DeckGen};
use engine::az::encode::{self, CODE, NUM_FEATURES};
use engine::az::identity;
use engine::az::{DeckMode, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::hidden::{key_kind, key_player, KEY_DRAW, KEY_LOOK};
use engine::rng::Rng;
use zero_search::game::{make_bot, SearchGame};

/// `AZ_SCALE=k` plays k times as many games in the sampled-state tests.
fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn spec() -> MtgSpec {
    MtgSpec { max_turns: 60, ..MtgSpec::default() }
}

/// `viewer`'s whole row (layout v7, ENGINE.md 125): the head ([`NUM_FEATURES`] floats first),
/// then the tail tokens and the pointer overrides (`tail_encode::Row::flat`).
fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// An action: the heuristic's with probability 1/2, else uniform.
fn mixed(g: &MtgAz, h: &mut Heuristic, rng: &mut Rng) -> u16 {
    let l = legal(g);
    if rng.below(2) == 0 {
        h.choose(&g.g, &l)
    } else {
        // (Mana abilities with priority, `YES` (ENGINE.md 100.4.3), one
        // random pick in eight: uniform picks would float most of the mana
        // and keep the games simple, below the states the gates need.)
        let pick = l[rng.below(l.len() as u32) as usize];
        if pick == engine::act::YES && g.g.kind() == Some(engine::DecisionKind::Priority) && l.len() > 1 && rng.below(8) != 0 {
            l[0]
        } else {
            pick
        }
    }
}

/// Mid-game states of `games` games (mixed heuristic / random play), one
/// every ~`every` decisions.
fn states(games: u64, every: u32, seed: u64) -> Vec<MtgAz> {
    let sp = spec();
    let mut rng = Rng::new(seed);
    let mut h = Heuristic::new(seed);
    let mut out = Vec::new();
    for s in 0..games {
        let mut g = MtgAz::new_game(&sp, 2, seed * 1000 + s).unwrap();
        let mut keys = Vec::new();
        while g.to_move().is_some() {
            if rng.below(every) == 0 {
                out.push(g.clone());
            }
            let a = mixed(&g, &mut h, &mut rng);
            g.step(a, &mut keys).unwrap();
        }
    }
    out
}

#[test]
fn games_end_with_sane_results() {
    let sp = spec();
    let mut rng = Rng::new(1);
    let mut h = Heuristic::new(1);
    let mut draws = 0;
    for s in 0..60 {
        let mut g = MtgAz::new_game(&sp, 2, s).unwrap();
        assert_eq!(g.num_players(), 2);
        let mut keys = Vec::new();
        let mut steps = 0;
        while let Some(p) = g.to_move() {
            assert!(p < 2);
            let l = legal(&g);
            assert!(!l.is_empty() && l.windows(2).all(|w| w[0] < w[1]));
            assert!(l.iter().all(|&a| (a as usize) < MtgAz::NUM_ACTIONS));
            let a = mixed(&g, &mut h, &mut rng);
            g.step(a, &mut keys).unwrap();
            steps += 1;
            assert!(steps < 20_000, "game {s} does not end");
        }
        let r = g.result();
        assert!((r.iter().sum::<f32>() - 1.0).abs() < 1e-6, "{r:?}");
        assert!(r[2..].iter().all(|&x| x == 0.0));
        if r[0] == 0.5 {
            draws += 1;
            assert_eq!(g.margin(0), -g.margin(1));
        } else {
            let w = (r[1] == 1.0) as usize;
            assert!(r[w] == 1.0 && r[1 - w] == 0.0);
        }
        for seat in 0..2 {
            assert!((-1.0..=1.0).contains(&g.margin(seat)));
        }
        assert!(g.rounds() >= 1);
        // an illegal action is an error, not a panic
        assert!(g.step(0, &mut keys).is_err());
    }
    assert!(draws < 20, "{draws} draws in 60 games");
}

#[test]
fn turn_cap_is_a_draw() {
    let sp = MtgSpec { max_turns: 2, ..spec() };
    let mut g = MtgAz::new_game(&sp, 2, 7).unwrap();
    let mut keys = Vec::new();
    while g.to_move().is_some() {
        let a = legal(&g)[0];
        g.step(a, &mut keys).unwrap();
    }
    assert_eq!(g.result()[..2], [0.5, 0.5]);
    assert_eq!(g.rounds(), 2);
}

#[test]
fn clone_continues_identically() {
    let mut rng = Rng::new(9);
    for g in states(scale(12), 60, 2).into_iter().step_by(3) {
        let (mut a, mut b) = (g.clone(), g.clone());
        let mut steps = 0;
        while a.to_move().is_some() && steps < 200 {
            let l = legal(&a);
            assert_eq!(l, legal(&b));
            let x = l[rng.below(l.len() as u32) as usize];
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            assert_eq!(ka, kb);
            assert_eq!(enc(&a, 0), enc(&b, 0));
            steps += 1;
        }
        assert_eq!(a.g.fingerprint(), b.g.fingerprint());
    }
}

#[test]
fn determinize_keeps_features_and_legal_actions_for_both_viewers() {
    let mut rng = zero_search::rng::Rng::new(3);
    let (mut checked, mut changed) = (0, 0);
    for g in states(scale(24), 25, 3) {
        let mover = g.to_move().unwrap();
        for viewer in [mover, 1 - mover] {
            let x0 = enc(&g, viewer);
            for _ in 0..3 {
                let mut d = g.clone();
                d.determinize(viewer, &mut rng);
                assert_eq!(d.to_move(), Some(mover));
                if viewer == mover {
                    assert_eq!(legal(&g), legal(&d), "determinize changed the decider's actions");
                }
                let x1 = enc(&d, viewer);
                if let Some(i) = (0..NUM_FEATURES).find(|&i| x0[i] != x1[i]) {
                    panic!("features of P{viewer} changed at {i} ({} -> {}); mover P{mover}", x0[i], x1[i]);
                }
                d.g.check_invariants().unwrap();
                checked += 1;
                changed += g.g.objs().iter().zip(d.g.objs()).any(|(x, y)| x.def != y.def) as u32;
            }
        }
    }
    println!("determinized {checked}, changed {changed}");
    assert!(checked > 300, "{checked}");
    assert!(changed > checked / 2, "determinize rarely resampled anything: {changed}/{checked}");
}

/// zero-core's sparse rows (its docs/search.md 2.12, ENGINE.md 89): a sparse
/// driver reads every row with `SearchGame::encode_sparse` (MTG: the default,
/// `encode` + `sparse_scan`) and keys its evaluation cache by
/// `obs_hash_sparse` of the pairs. For both viewers of mid-game states of the
/// cube's and the Delver mirror's games: the pairs are exactly the non-zero
/// floats of `encode` (bits, index order), the scratch is zeroed again, and
/// the key is the dense row's `obs_hash`. (A guard for an MTG override that
/// emits its pairs directly: it must pass unchanged.)
#[test]
fn encode_sparse_is_the_rows_non_zeros_with_the_same_key() {
    use zero_search::lookahead::{obs_hash, obs_hash_sparse};
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    let mirror = serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string();
    let mut gs = states(scale(12), 5, 5);
    let sp = MtgSpec::parse(&mirror).unwrap();
    let mut rng = Rng::new(6);
    let mut h = Heuristic::new(6);
    for s in 0..scale(12) {
        let mut g = MtgAz::new_game(&sp, 2, 6000 + s).unwrap();
        let mut keys = Vec::new();
        while g.to_move().is_some() {
            if rng.below(5) == 0 {
                gs.push(g.clone());
            }
            let a = mixed(&g, &mut h, &mut rng);
            g.step(a, &mut keys).unwrap();
        }
    }
    let mut scratch = vec![0.0f32; NUM_FEATURES];
    let (mut idx, mut val) = (Vec::new(), Vec::new());
    let mut rows = 0;
    for g in &gs {
        for viewer in 0..2 {
            // (Layout v7, ENGINE.md 125: a row with tail tokens is read by `encode_row`, never
            // `encode_sparse`, whose `encode` refuses it: zero-core R1, `tests/v7_tails.rs`.)
            let row = engine::az::tail_encode::Row::of_az(g, viewer);
            if !row.tail.is_empty() {
                continue;
            }
            let x = row.head;
            idx.clear();
            val.clear();
            idx.push(u32::MAX);
            val.push(1.0);
            g.encode_sparse(viewer, &mut scratch, &mut idx, &mut val);
            let want: Vec<(u32, u32)> = x.iter().enumerate().filter(|(_, v)| v.to_bits() != 0).map(|(i, v)| (i as u32, v.to_bits())).collect();
            let got: Vec<(u32, u32)> = idx[1..].iter().zip(&val[1..]).map(|(&i, v)| (i, v.to_bits())).collect();
            assert_eq!(got, want, "encode_sparse is not encode's non-zeros (viewer {viewer})");
            assert_eq!((idx[0], val[0]), (u32::MAX, 1.0), "encode_sparse did not append");
            assert!(scratch.iter().all(|v| v.to_bits() == 0), "the scratch not zeroed again");
            assert_eq!(obs_hash_sparse(&idx[1..], &val[1..], NUM_FEATURES), obs_hash(&x), "another cache key");
            rows += 1;
        }
    }
    // (Most rows of these games have tail tokens: a mulligan's bottom cards known below the top
    // three, delayed triggers, ... ENGINE.md 125.)
    println!("encode_sparse checked on {rows} rows without a tail");
    assert!(rows > 300, "{rows}");
}

/// zero-core keys children by (chance keys, next decider, next legal
/// actions): two worlds sampled for the decider, played with the same
/// actions, must give the decider the same features while those agree.
#[test]
fn equal_keys_mean_equal_features() {
    let mut rng = Rng::new(4);
    let mut zr = zero_search::rng::Rng::new(4);
    let mut compared = 0;
    for g in states(scale(20), 20, 4) {
        let viewer = g.to_move().unwrap();
        let (mut a, mut b) = (g.clone(), g.clone());
        a.determinize(viewer, &mut zr);
        b.determinize(viewer, &mut zr);
        for _ in 0..30 {
            let (la, lb) = (legal(&a), legal(&b));
            if a.to_move() != b.to_move() || la != lb {
                assert!(!(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)), "the viewer's options differ");
                break;
            }
            assert_eq!(enc(&a, viewer), enc(&b, viewer), "same keys and decision, different features");
            compared += 1;
            if a.to_move().is_none() {
                break;
            }
            let x = la[rng.below(la.len() as u32) as usize];
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            if ka != kb {
                break;
            }
        }
    }
    println!("compared {compared}");
    assert!(compared > 500, "{compared}");
}

/// `force_chance`: a transition's keys sampled in one world, forced in
/// another world of the same viewer, are reproduced when that world allows
/// it; the actual outcome is always reported and the world stays valid.
#[test]
fn force_chance_replays_draws_and_looks() {
    let mut rng = Rng::new(5);
    let mut zr = zero_search::rng::Rng::new(5);
    let (mut tried, mut hits) = (0, 0);
    for g in states(scale(40), 8, 5) {
        let viewer = g.to_move().unwrap();
        let mut a = g.clone();
        a.determinize(viewer, &mut zr);
        let l = legal(&a);
        let x = l[rng.below(l.len() as u32) as usize];
        let mut ka = Vec::new();
        a.step(x, &mut ka).unwrap();
        let own = |k: &u64| (key_kind(*k) == KEY_DRAW || key_kind(*k) == KEY_LOOK) && key_player(*k) == viewer as u8;
        if !ka.iter().any(own) {
            continue;
        }
        for _ in 0..4 {
            let mut b = g.clone();
            b.determinize(viewer, &mut zr);
            b.force_chance(&ka);
            let mut kb = Vec::new();
            b.step(x, &mut kb).unwrap();
            b.force_chance(&[]);
            b.g.check_invariants().unwrap();
            tried += 1;
            if kb == ka {
                hits += 1;
                assert_eq!(enc(&a, viewer), enc(&b, viewer), "forced to the same keys, different features");
            }
            // without forcing, the same world reports its own outcome
            let mut c = g.clone();
            c.determinize(viewer, &mut zero_search::rng::Rng::new(77));
            let mut kc = Vec::new();
            c.step(x, &mut kc).unwrap();
            assert_eq!(kc.len(), kc.len());
        }
    }
    println!("forced {hits}/{tried}");
    assert!(tried > 100, "{tried}");
    assert!(hits * 10 > tried * 8, "forced {hits}/{tried}");
}

#[test]
fn forced_keys_are_consumed_in_order_and_impossible_ones_fall_back() {
    let mut zr = zero_search::rng::Rng::new(6);
    let mut rng = Rng::new(6);
    let mut checked = 0;
    // 90 games: steps with two or more draws for the viewer are rare (30 games gave 2 at 438
    // supported cards, 3 needed).
    for g in states(scale(90), 8, 6) {
        let viewer = g.to_move().unwrap();
        let l = legal(&g);
        let x = l[rng.below(l.len() as u32) as usize];
        let mut a = g.clone();
        a.determinize(viewer, &mut zr);
        let mut ka = Vec::new();
        a.step(x, &mut ka).unwrap();
        let draws: Vec<usize> = (0..ka.len()).filter(|&i| key_kind(ka[i]) == KEY_DRAW && key_player(ka[i]) == viewer as u8).collect();
        if draws.len() < 2 {
            continue;
        }
        // The first draw asks for an impossible identity (a token's); the
        // second still gets its own forced key.
        let mut forced = ka.clone();
        let token = CardDb::cube().token_def(0) as u64;
        forced[draws[0]] = engine::hidden::key(KEY_DRAW, viewer as u8, token);
        let mut b = g.clone();
        b.determinize(viewer, &mut zr);
        b.force_chance(&forced);
        let mut kb = Vec::new();
        b.step(x, &mut kb).unwrap();
        // (Another world may stop earlier: the opponent may have a choice
        // there, or none, which is keyed.)
        if kb.len() != ka.len() || key_kind(kb[draws[0]]) != KEY_DRAW {
            continue;
        }
        assert_ne!(kb[draws[0]], forced[draws[0]], "an impossible outcome happened");
        b.g.check_invariants().unwrap();
        if kb[draws[1]] == ka[draws[1]] {
            checked += 1;
        }
    }
    assert!(checked >= 3, "{checked}");
}

/// The identity in a card code (digits 16, 8, 8).
fn decode_id(code: &[f32]) -> usize {
    let a = code[..16].iter().position(|&v| v == 1.0).unwrap();
    let b = code[16..24].iter().position(|&v| v == 1.0).unwrap();
    let c = code[24..32].iter().position(|&v| v == 1.0).unwrap();
    a * 64 + b * 8 + c
}

fn ident(def: engine::ids::DefId) -> usize {
    identity::identities(CardDb::cube())[def as usize] as usize
}

/// Every option of the decider's row points at the token of the object it names (layout v7,
/// ENGINE.md 125: absolute positions, tail tokens past a segment's head slots, pointer overrides
/// on range pages and for exile offers; `tail_encode::check_pointers`), each token holding its
/// object's identity and option flag; the encoding is bounded.
#[test]
fn encoding_slots_name_the_action_slots() {
    assert_eq!(NUM_FEATURES, 56_192);
    let (mut lists, mut checked) = (0, 0);
    for g in states(scale(16), 6, 7) {
        let me = g.to_move().unwrap();
        let x = enc(&g, me);
        let row = engine::az::tail_encode::Row::of(&g.g, me as u8);
        // (Bounded by the encoding's bound: amounts are squashed, and mana
        // floated with priority, ENGINE.md 100.4.3, reaches large X and
        // event amounts in these games; the head and every tail token.)
        let bad = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).find(|v| !(v.is_finite() && v.abs() <= encode::FEATURE_BOUND));
        assert!(bad.is_none(), "{bad:?}");
        checked += engine::az::tail_encode::check_pointers(&g.g, me as u8, &row).unwrap_or_else(|e| panic!("{e}"));
        let d = g.g.pending_decision().unwrap();
        lists += d.options.iter().filter(|o| (act::LIBRARY..act::LIBRARY + encode::LIST as u16).contains(&o.action)).count();
        // the decision kind
        assert_eq!(x[encode::O_CONTEXT + d.kind as usize], 1.0);
    }
    println!("{checked} options' pointers checked");
    assert!(lists > 0, "no LIBRARY decision met");
}

#[test]
fn layout_json_is_checked_in() {
    // mtgz/az/layout_v7.json = encode::layout_json (MTGZ_WRITE_LAYOUT=1 rewrites it; layout_v6.json,
    // layout_v5.json and layout_v4.json stay as they were, for the widen of v6, v5 and v4 networks:
    // `layout_v7_widens_layout_v6_exactly`, `layout_v6_widens_layout_v5_exactly`,
    // `layout_v5_widens_layout_v4_exactly`; ENGINE.md 124.2)
    let want = encode::layout_json(CardDb::cube());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az/layout_v7.json");
    if std::env::var("MTGZ_WRITE_LAYOUT").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(have == want, "{} differs from encode::layout_json (MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test az layout)", path.display());
    let v: serde_json::Value = serde_json::from_str(&want).unwrap();
    assert_eq!(v["features"], NUM_FEATURES);
    assert_eq!(v["version"], 7);
    assert_eq!(v["cards"].as_array().unwrap().len(), identity::D);
}

/// Layout v7 widens layout v6 exactly (ENGINE.md 124.2, 125): the map a widen derives from the
/// two checked-in descriptions is `encode::v6_projection` (v6's segments, bags and global blocks
/// the same, the permanents', hand's, stack's, triggers' and list's slots wider at their ends, the
/// Cavern block and v7's tail-only segments new), layout_v6.json is layout v6 as frozen (ENGINE.md
/// 100.5.1), v7's segments are v6's plus the appended tail-only, gated ones, and the variable-length
/// keys are set (tails on the seven capped segments, pointer overrides, the row limit, a draft).
#[test]
fn layout_v7_widens_layout_v6_exactly() {
    let (l6, l7) = (read_layout("layout_v6.json"), read_layout("layout_v7.json"));
    assert_eq!((l6.features, l7.features), (encode::NUM_FEATURES_V6, NUM_FEATURES));
    assert_eq!(l6.globals.iter().map(|g| g.0.clone().unwrap()).collect::<Vec<_>>(), ["globals", "match", "globals_v5", "globals_v6"]);
    assert_eq!(
        l7.globals.iter().map(|g| g.0.clone().unwrap()).collect::<Vec<_>>(),
        ["globals", "match", "globals_v5", "globals_v6", "globals_v7"]
    );
    assert_eq!(widen_map(&l6, &l7), encode::v6_projection(), "the widen's map is the v6 projection");
    let v7 = &l7.globals[4].1;
    assert_eq!((v7[0], v7.len()), (encode::O_GLOBALS_V7, encode::GLOBALS_V7));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az");
    let read = |f: &str| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap() };
    let j7 = read("layout_v7.json");
    assert_eq!((j7["option_pointers"].as_bool(), j7["max_row_tokens"].as_u64(), j7["draft"].as_bool()), (Some(true), Some(4096), Some(true)));
    let segs = j7["segments"].as_array().unwrap();
    let names: Vec<&str> = segs.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 18);
    let tails: Vec<&str> = segs.iter().filter(|s| s["tail"] == true).map(|s| s["name"].as_str().unwrap()).collect();
    let want: Vec<&str> = engine::az::tail_encode::TAILS.iter().map(|t| t.name).collect();
    assert_eq!(tails, want, "the tail segments are TAILS");
    for t in engine::az::tail_encode::TAILS.iter() {
        let s = &segs[t.seg as usize];
        assert_eq!((s["name"].as_str().unwrap(), s["width"].as_u64().unwrap() as usize, s["count"].as_u64().unwrap() as usize), (t.name, t.width, t.head));
        assert_eq!(s["gate"] == true, t.head == 0, "{}: gated iff tail-only", t.name);
        assert_eq!(s["offset"].is_null(), t.head == 0);
    }
    assert_eq!(segs[engine::az::tail_encode::seg::EXILE_OFFERED as usize]["columns"], 1);
    let head: usize = 1 + segs.iter().map(|s| s["count"].as_u64().unwrap() as usize).sum::<usize>();
    assert_eq!(head, engine::az::tail_encode::HEAD_TOKENS);
    // (and v5's widen onto v7 is v5 -> v6 -> v7)
    let l5 = read_layout("layout_v5.json");
    assert_eq!(widen_map(&l5, &l7), encode::v5_projection());
}

/// The features of a token layout description (`layout_v*.json`) as zero-core's `net.widen`
/// reads them (its `token_feature_map`): per segment its slots' floats, per bag its rows, the
/// globals by declared block (else one block of every other feature, in feature order).
struct Layout {
    features: usize,
    segs: Vec<(String, usize, usize, usize, serde_json::Value)>,
    bags: Vec<(String, usize, usize, usize)>,
    globals: Vec<(Option<String>, Vec<usize>)>,
}

fn read_layout(file: &str) -> Layout {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az").join(file);
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let u = |x: &serde_json::Value| x.as_u64().unwrap() as usize;
    let features = u(&v["features"]);
    let segs: Vec<_> = v["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let key = serde_json::json!([s["present"], s["id"], s["actions"]]);
            (s["name"].as_str().unwrap().to_string(), s["offset"].as_u64().map_or(0, |o| o as usize), u(&s["count"]), u(&s["width"]), key)
        })
        .collect();
    let bags: Vec<_> = v["bags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["name"].as_str().unwrap().to_string(), u(&b["offset"]), u(&b["count"]), u(&b["width"])))
        .collect();
    let globals = match v.get("globals") {
        Some(gs) => gs
            .as_array()
            .unwrap()
            .iter()
            .map(|g| (Some(g["name"].as_str().unwrap().to_string()), (u(&g["offset"])..u(&g["offset"]) + u(&g["count"])).collect()))
            .collect(),
        None => {
            let mut inside = vec![false; features];
            for &(_, o, n, w, _) in &segs {
                inside[o..o + n * w].iter_mut().for_each(|x| *x = true);
            }
            for &(_, o, n, w) in &bags {
                inside[o..o + n * w].iter_mut().for_each(|x| *x = true);
            }
            vec![(None, (0..features).filter(|&i| !inside[i]).collect())]
        }
    };
    Layout { features, segs, bags, globals }
}

/// The map zero-core's `net.widen` derives from two checked-in token layouts (its
/// `token_feature_map`): segments by name with their slots' old floats first (the same slots,
/// present flags, identities and actions), bags by name with their old rows first, the globals by
/// declared block (or, when the old layout declares none, as the new layout's first globals in
/// feature order).
fn widen_map(l4: &Layout, l5: &Layout) -> Vec<usize> {
    // (New segments are appended after the old ones: layout v7's tail-only segments, ENGINE.md 125.)
    assert_eq!(l4.segs.iter().map(|s| &s.0).collect::<Vec<_>>(), l5.segs[..l4.segs.len()].iter().map(|s| &s.0).collect::<Vec<_>>());
    assert!(l5.segs[l4.segs.len()..].iter().all(|s| s.2 == 0), "new segments are tail-only");
    let mut map = vec![usize::MAX; l4.features];
    for (a, b) in l4.segs.iter().zip(&l5.segs) {
        assert_eq!((a.2, &a.4), (b.2, &b.4), "segment {}: the same slots, present flag, identity and actions", a.0);
        assert!(b.3 >= a.3, "segment {} got narrower", a.0);
        for s in 0..a.2 {
            for j in 0..a.3 {
                map[a.1 + s * a.3 + j] = b.1 + s * b.3 + j;
            }
        }
    }
    for a in &l4.bags {
        let b = l5.bags.iter().find(|b| b.0 == a.0).expect("the old bags are the new layout's");
        assert!(b.3 == a.3 && b.2 >= a.2);
        for i in 0..a.2 * a.3 {
            map[a.1 + i] = b.1 + i;
        }
    }
    match &l4.globals[0].0 {
        None => {
            let g5: Vec<usize> = l5.globals.iter().flat_map(|g| g.1.iter().copied()).collect();
            for (k, &f) in l4.globals[0].1.iter().enumerate() {
                map[f] = g5[k];
            }
        }
        Some(_) => {
            for (name, fs) in &l4.globals {
                let b = l5.globals.iter().find(|g| &g.0 == name).expect("the old global blocks are the new layout's");
                assert!(b.1.len() >= fs.len(), "global block {name:?} got narrower");
                for (k, &f) in fs.iter().enumerate() {
                    map[f] = b.1[k];
                }
            }
        }
    }
    let mut seen = map.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), l4.features, "one to one");
    assert!(map.iter().all(|&i| i < l5.features));
    map
}

/// Layout v6 is layout v5 with additions only (ENGINE.md 100.5), as v5 was v4's: the map a widen
/// derives from the two checked-in descriptions is `encode::v5_projection`, the map the v5 digest
/// is reproduced through (tests/digest.rs, `DIGEST_ENCODING=v5`); the tokens are the same, the
/// global blocks of v5 are v6's (v6's own block after them), and layout_v5.json is layout v5
/// (37,620 floats, as frozen: ENGINE.md 60).
#[test]
fn layout_v6_widens_layout_v5_exactly() {
    let (l5, l6) = (read_layout("layout_v5.json"), read_layout("layout_v6.json"));
    assert_eq!((l5.features, l6.features), (encode::v5::NUM_FEATURES, encode::NUM_FEATURES_V6));
    assert_eq!(l5.globals.iter().map(|g| g.0.clone().unwrap()).collect::<Vec<_>>(), ["globals", "match", "globals_v5"]);
    assert_eq!(widen_map(&l5, &l6), encode::v5_to_v6(), "the widen's map is the v5 projection");
    // (and v4's widen onto v6 is v4 -> v5 -> v6)
    let l4 = read_layout("layout_v4.json");
    let (a, b) = (encode::v4_to_v5(), encode::v5_to_v6());
    assert_eq!(widen_map(&l4, &l6), a.into_iter().map(|i| b[i]).collect::<Vec<_>>(), "v4 widens onto v6 through the v4 projection");
}

/// Layout v5 is layout v4 with additions only (ENGINE.md 60.2): the map a widen derives from the
/// two checked-in descriptions (segments by name with their slots' old floats first, bags by name
/// with their old rows first, v4's globals as v5's first globals) is `encode::v4_to_v5`; the tokens
/// are the same (segments, slot counts, present flags, identities, actions), and layout_v4.json is
/// layout v4 (33,652 floats, as frozen: ENGINE.md 22.7).
#[test]
fn layout_v5_widens_layout_v4_exactly() {
    let (l4, l5) = (read_layout("layout_v4.json"), read_layout("layout_v5.json"));
    assert_eq!((l4.features, l5.features), (encode::v4::NUM_FEATURES, encode::v5::NUM_FEATURES));
    assert!(l4.globals[0].0.is_none(), "layout v4 declares no global blocks");
    assert_eq!(l4.globals[0].1.len(), encode::GLOBALS + encode::MATCH);
    assert_eq!(widen_map(&l4, &l5), encode::v4_to_v5(), "the widen's map is the v4 projection");
}

#[test]
fn token_registry_and_stable_identities() {
    // every token of the cube's card table is registered (MTGZ_WRITE_TOKENS=1 appends the
    // missing keys to engine/src/az/tokens.txt; a registered key never moves)
    let db = CardDb::cube();
    let reg = identity::registry();
    let mut keys = Vec::new();
    for i in db.num_cards()..db.len() {
        // (an emblem or a face, stored with the tokens, has its card's identity)
        if !db.def(i as engine::ids::DefId).emblem && !db.is_face(i as engine::ids::DefId) {
            keys.push(identity::token_key(db, db.def(i as engine::ids::DefId)));
        }
    }
    let missing: Vec<&String> = keys.iter().filter(|k| !reg.contains(&k.as_str())).collect();
    if !missing.is_empty() && std::env::var("MTGZ_WRITE_TOKENS").is_ok_and(|v| v == "1") {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/az/tokens.txt");
        let mut text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let mut seen = std::collections::BTreeSet::new();
        for k in &missing {
            if seen.insert(k.as_str()) {
                text.push_str(k);
                text.push('\n');
            }
        }
        std::fs::write(&path, text).unwrap();
        panic!("appended {} token keys to {}: rebuild and rerun", seen.len(), path.display());
    }
    assert!(missing.is_empty(), "unregistered tokens (MTGZ_WRITE_TOKENS=1 cargo test -p engine --test az token_registry): {missing:?}");
    let uniq: std::collections::BTreeSet<&String> = keys.iter().collect();
    assert_eq!(uniq.len(), keys.len(), "two token definitions share a key: {keys:?}");
    // identities: cube cards at their IR position, tokens above CUBE_CAP, all distinct
    let ids = identity::identities(db);
    let names = identity::cube_names();
    assert_eq!(names.len(), 546);
    for i in 0..db.len() {
        let d = db.def(i as engine::ids::DefId);
        let id = ids[i] as usize;
        assert!(id != identity::UNKNOWN as usize, "{} has no identity", d.name);
        if d.token || d.rules || i as engine::ids::DefId == db.face_down_def() {
            assert!((identity::CUBE_CAP..identity::D - 1).contains(&id));
        } else if d.emblem {
            // an emblem (CR 114) has the identity of the card that makes it
            assert_eq!(Some(names[id].as_str()), d.name.strip_suffix(" emblem"));
        } else if let Some(f) = d.front {
            // a face (CR 712, 715) has its card's identity
            assert_eq!(names[id], db.def(f).name);
        } else {
            assert_eq!(names[id], d.name);
        }
    }
    let not_emblems: Vec<u16> = ids
        .iter()
        .enumerate()
        .filter(|&(i, _)| !db.def(i as engine::ids::DefId).emblem && !db.is_face(i as engine::ids::DefId))
        .map(|(_, &x)| x)
        .collect();
    let distinct: std::collections::BTreeSet<u16> = not_emblems.iter().copied().collect();
    assert_eq!(distinct.len(), not_emblems.len());
    // the identity does not depend on what else is supported: a card table of a few cards
    // gives them the same identities as the full one
    let doc = engine::db::load_cube_document();
    let few: Vec<_> = doc.cards.iter().filter(|c| ["Forest", "Lightning Bolt", "Island", "Wurmcoil Engine"].contains(&c.name.as_str()) || c.name.starts_with("Young Pyro")).cloned().collect();
    let small = CardDb::from_cards(&few).leak();
    let sids = identity::identities(small);
    for i in 0..small.len() {
        let d = small.def(i as engine::ids::DefId);
        let full = if d.token || d.rules {
            (db.num_cards()..db.len()).find(|&j| identity::token_key(db, db.def(j as engine::ids::DefId)) == identity::token_key(small, d))
        } else {
            db.id(&d.name).map(|x| x as usize)
        };
        if let Some(j) = full {
            assert_eq!(sids[i], ids[j], "{}", d.name);
        }
    }
}

/// The legacy card registry (ENGINE.md 22.2, 53): every card of the legacy IR has a line, the
/// lines are distinct and none is a cube card, and the identities are 546 + the line, below
/// `CUBE_CAP` (layout v4's free range); the cube's identities are the same in both card tables.
#[test]
fn legacy_registry_and_identities() {
    let reg = identity::legacy_registry();
    let cube = identity::cube_names();
    let legacy = engine::db::load_legacy_document();
    for c in &legacy.cards {
        assert!(reg.contains(&c.name.as_str()), "{} has no line in az/legacy_cards.txt (append it)", c.name);
    }
    let mut seen = std::collections::HashSet::new();
    for n in &reg {
        assert!(seen.insert(*n), "{n} twice");
        assert!(!cube.iter().any(|c| c == n), "{n} is a cube card");
    }
    assert_eq!(cube.len(), identity::LEGACY_BASE);
    assert!(identity::LEGACY_BASE + reg.len() <= identity::CUBE_CAP);
    let con = CardDb::constructed();
    let ids = identity::identities(con);
    for (k, n) in reg.iter().enumerate() {
        assert_eq!(ids[con.id(n).unwrap() as usize] as usize, identity::LEGACY_BASE + k, "{n}");
    }
    let cube_ids = identity::identities(CardDb::cube());
    assert_eq!(&ids[..CardDb::cube().num_cards()], &cube_ids[..CardDb::cube().num_cards()]);
}

#[test]
fn minor_decisions_are_mana_payments() {
    let mut pay = 0;
    for g in states(scale(4), 3, 11) {
        let d = g.g.pending_decision().unwrap();
        // (Which pool mana pays is minor too, ENGINE.md 34.5.)
        assert_eq!(g.minor_decision(), matches!(d.kind, engine::decision::DecisionKind::PayMana | engine::decision::DecisionKind::PayPool));
        pay += g.minor_decision() as u32;
    }
    assert!(pay > 0, "no mana payment met");
}

/// zero-core's lookahead horizon 'next_turn' (ENGINE.md 80): `active_seat` is the exported state's
/// active player, every state.
#[test]
fn active_seat_is_the_active_player() {
    let mut seen = [0u32; 2];
    for g in states(scale(4), 3, 13) {
        let st: serde_json::Value = serde_json::from_str(&g.export_state()).unwrap();
        let a = st["active"].as_u64().unwrap() as usize;
        assert_eq!(g.active_seat(), Some(a));
        assert_eq!(g.active_seat(), Some(g.g.turn().active as usize));
        seen[a] += 1;
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

#[test]
fn decks_are_legal_seeded_and_varied() {
    let db = CardDb::cube();
    let gen = DeckGen::default();
    let mut pairs = std::collections::BTreeSet::new();
    for s in 0..400 {
        let mut rng = Rng::new(s);
        let d = decks::generate(db, &mut rng, &gen);
        decks::check(db, &d, &gen).unwrap_or_else(|e| panic!("seed {s}: {e}"));
        let mut again = Rng::new(s);
        assert_eq!(d, decks::generate(db, &mut again, &gen));
        let colors = d.iter().fold(0u8, |m, &x| m | db.def(x).colors);
        pairs.insert(colors);
    }
    assert!(pairs.len() >= 10, "{pairs:?}");
    // the evaluation set is fixed
    for i in 0..16 {
        let a = decks::eval_pair(db, &gen, 1, i);
        assert_eq!(a, decks::eval_pair(db, &gen, 1, i));
        for d in &a {
            decks::check(db, d, &gen).unwrap();
        }
    }
    assert_ne!(decks::eval_pair(db, &gen, 1, 0), decks::eval_pair(db, &gen, 1, 1));
    assert_ne!(decks::eval_pair(db, &gen, 1, 0), decks::eval_pair(db, &gen, 2, 0));
}

#[test]
fn spec_parsing() {
    let s = MtgSpec::parse("").unwrap();
    assert_eq!(s, MtgSpec::default());
    let s = MtgSpec::parse(r#"{"max_turns": 30, "mulligans": false, "lands": [17, 17], "eval_decks": 4}"#).unwrap();
    assert_eq!((s.max_turns, s.mulligans, s.gen.min_lands, s.eval_decks), (30, false, 17, 4));
    let lists = r#"{"decks": {"p0": ["Forest", "Llanowar Elves"], "p1": ["Island"]}}"#;
    match MtgSpec::parse(lists).unwrap().decks {
        DeckMode::Lists(a, b) => assert_eq!((a.len(), b.len()), (2, 1)),
        other => panic!("{other:?}"),
    }
    assert!(MtgSpec::parse(r#"{"decks": {"p0": ["No Such Card"], "p1": []}}"#).unwrap_err().contains("unknown card"));
    assert!(MtgSpec::parse(r#"{"nope": 1}"#).is_err());
    assert!(MtgSpec::parse(r#"{"max_turns": 0}"#).is_err());
    assert_eq!(MtgSpec::parse(r#"{"decks": "eval"}"#).unwrap().decks, DeckMode::Eval);
    let e = MtgSpec::eval_json(r#"{"max_turns": 50}"#).unwrap();
    assert_eq!(MtgSpec::parse(&e).unwrap().decks, DeckMode::Eval);
    assert_eq!(MtgSpec::parse(&e).unwrap().max_turns, 50);
    // explicit lists stay
    let e = MtgSpec::eval_json(lists).unwrap();
    assert!(matches!(MtgSpec::parse(&e).unwrap().decks, DeckMode::Lists(..)));
    // the same seed gives the same decks; eval games use the fixed set
    let sp = MtgSpec::parse(r#"{"decks": "eval", "eval_decks": 3}"#).unwrap();
    let sets: std::collections::BTreeSet<_> = (0..40).map(|s| sp.decks(s)).collect();
    assert_eq!(sets.len(), 3);
    assert!(MtgAz::new_game(&sp, 3, 1).is_err());
}

#[test]
fn heuristic_beats_random_and_is_always_legal() {
    let sp = spec();
    let mut score = 0.0;
    let n = 120;
    let mut legal_v = Vec::new();
    for s in 0..n {
        let mut g = MtgAz::new_game(&sp, 2, 500 + s).unwrap();
        let hseat = (s % 2) as usize;
        let mut bots = [make_bot::<MtgAz>(&sp, "heuristic", s).unwrap(), make_bot::<MtgAz>(&sp, "random", s).unwrap()];
        let mut keys = Vec::new();
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal_v);
            let a = bots[(p != hseat) as usize].choose(&g, &legal_v);
            assert!(legal_v.contains(&a));
            g.step(a, &mut keys).unwrap();
        }
        score += g.result()[hseat];
    }
    let w = score / n as f32;
    assert!(w > 0.8, "heuristic vs random {w}");
    assert_eq!(MtgAz::bot_names(), &["heuristic"]);
    assert!(make_bot::<MtgAz>(&sp, "nope", 0).is_none());
}

// ---- The opening: the declared mulligan (ENGINE.md 55) ---------------------

/// The specs of the opening tests: the cube's random best-of-one decks, a
/// best-of-three match's first game, the Legacy Izzet Delver mirror
/// (`runs/delver1`, ENGINE.md 54).
fn opening_specs() -> Vec<(&'static str, MtgSpec)> {
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    let mirror = serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string();
    vec![
        ("cube", spec()),
        ("bo3", MtgSpec::parse(r#"{"format": "bo3"}"#).unwrap()),
        ("delver", MtgSpec::parse(&mirror).unwrap()),
    ]
}

/// Every state of the opening of `games` games of `sp` (from the
/// play-or-draw choice to the first decision of turn 1, included), with
/// seeded random answers: play or draw, reveal a companion or not, keep or
/// mulligan (keep after three), random bottoms.
fn opening_states(sp: &MtgSpec, games: u64, seed: u64) -> Vec<MtgAz> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for s in 0..games {
        let mut g = MtgAz::new_game(sp, 2, seed * 1000 + s).unwrap();
        let mut keys = Vec::new();
        while let Some(p) = g.to_move() {
            out.push(g.clone());
            if g.g.turn().number >= 1 {
                break;
            }
            let l = legal(&g);
            let a = match g.g.kind() {
                Some(engine::DecisionKind::Mulligan) if g.g.player(p as u8).mulligans >= 3 => act::PASS,
                _ => l[rng.below(l.len() as u32) as usize],
            };
            g.step(a, &mut keys).unwrap();
        }
    }
    out
}

/// FNV-1a over 64-bit words (as `tests/digest.rs`).
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// The encodings of the opening, both viewers, with the two declared-mulligan
/// globals masked, are what they were before the flag (recorded on main at
/// 05e37bb, which did not encode it): the flag changed no other float of any
/// opening state. (A later change to the encoding of these states re-records
/// these values; the failure message prints the new ones.) Layout v5 (ENGINE.md
/// 60): through the v4 projection, so the same recorded values say the v5
/// encodings (`MtgAz::encode`: the game's and the match's) hold v4's exactly.
#[test]
fn opening_encodings_but_the_declaration_are_unchanged() {
    let proj = encode::v4_projection();
    let mut got = Vec::new();
    for (name, sp) in opening_specs() {
        let mut h = Fnv::new();
        let (mut states, mut declared, mut bottoms) = (0, 0, 0);
        for g in opening_states(&sp, 40, 21) {
            states += 1;
            declared += (0..2).any(|p| g.g.player(p).mulligan_declared) as u32;
            bottoms += (g.g.kind() == Some(engine::DecisionKind::MulliganBottom)) as u32;
            for v in 0..2 {
                let x5 = enc(&g, v);
                let mut x: Vec<f32> = proj.iter().map(|&i| x5[i]).collect();
                x[MULL] = 0.0;
                x[MULL + 1] = 0.0;
                for f in x {
                    h.u64(f.to_bits() as u64);
                }
            }
        }
        println!("{name}: {states} opening states, {declared} with a declared mulligan, {bottoms} bottoms: {:016x}", h.0);
        assert!(declared > 10 && bottoms > 10, "{name}: {declared} declared, {bottoms} bottoms");
        got.push((name, format!("{:016x}", h.0)));
    }
    // (Re-recorded for mana abilities with priority, ENGINE.md 100.4.3: the
    // openings' first priority decisions offer `YES`.)
    let want = [("cube", "fe662b6d88338070"), ("bo3", "96e0eb0155f75cd4"), ("delver", "cb3395e062b2fd8f")];
    let want: Vec<(&str, String)> = want.iter().map(|&(n, h)| (n, h.to_string())).collect();
    assert_eq!(got, want, "the opening's encodings changed beyond the declared-mulligan flag");
}

/// The declared-mulligan globals: `g[MULL + side]`, viewer first.
const MULL: usize = encode::glob::MULLIGAN_DECLARED;

/// The per-player global "kept" of `side` (viewer first).
fn kept_at(side: usize) -> usize {
    24 + 40 * side + 13
}

/// A new game of `sp` driven to the starting player's first declaration
/// (they play first; no companion revealed).
fn first_declaration(sp: &MtgSpec, seed: u64) -> MtgAz {
    let mut g = MtgAz::new_game(sp, 2, seed).unwrap();
    let mut keys = Vec::new();
    loop {
        match g.g.kind() {
            Some(engine::DecisionKind::Mulligan) => return g,
            Some(engine::DecisionKind::PlayDraw) => {
                g.step(act::PLAYER, &mut keys).unwrap();
            }
            Some(engine::DecisionKind::RevealCompanion) => {
                g.step(act::PASS, &mut keys).unwrap();
            }
            k => panic!("{k:?} before the mulligans"),
        }
    }
}

/// `g` after action `a`.
fn after(g: &MtgAz, a: u16) -> MtgAz {
    let mut h = g.clone();
    h.step(a, &mut Vec::new()).unwrap();
    h
}

/// The features where `a` and `b` differ (outside the decision's context
/// block unless `context`).
fn diff(a: &[f32], b: &[f32], context: bool) -> Vec<usize> {
    let cx = encode::O_CONTEXT..encode::O_CONTEXT + encode::CONTEXT;
    (0..NUM_FEATURES).filter(|&i| a[i] != b[i] && (context || !cx.contains(&i))).collect()
}

/// CR 103.5: the starting player's declaration of a mulligan is public and
/// encoded at once, for both viewers: after "Mulligan" the position differs
/// from the one before the declaration outside the decision's context (main
/// at 05e37bb: only the context's decider differed), and from "Keep"
/// exactly in the kept and declared flags.
#[test]
fn cr_103_5_a_declared_mulligan_is_encoded() {
    for (name, sp) in opening_specs() {
        for seed in 0..6 {
            let g = first_declaration(&sp, seed);
            let s = g.g.turn().starting as usize;
            let o = 1 - s;
            assert_eq!(g.to_move(), Some(s));
            let (m, k) = (after(&g, act::YES), after(&g, act::PASS));
            assert!(m.g.player(s as u8).mulligan_declared && !k.g.player(s as u8).mulligan_declared);
            assert!(k.g.player(s as u8).kept && !m.g.player(s as u8).kept);
            assert_eq!((m.to_move(), k.to_move()), (Some(o), Some(o)), "{name}: the other player declares next");
            for v in 0..2 {
                let side = (v != s) as usize; // the starting player's side for viewer v
                let (xb, xm, xk) = (enc(&g, v), enc(&m, v), enc(&k, v));
                assert_eq!((xb[MULL], xb[MULL + 1]), (0.0, 0.0), "{name} seed {seed}: nobody declared yet");
                assert_eq!(diff(&xb, &xm, false), vec![MULL + side], "{name} seed {seed} viewer {v}: before / after Mulligan");
                let mut want = vec![kept_at(side), MULL + side];
                want.sort();
                assert_eq!(diff(&xk, &xm, true), want, "{name} seed {seed} viewer {v}: Keep / Mulligan");
                assert_eq!((xm[MULL + side], xm[MULL + 1 - side]), (1.0, 0.0));
                assert_eq!((xk[MULL], xk[MULL + 1]), (0.0, 0.0));
                // The observable view of the hidden-information checks
                // (`Game::view_of`) holds it too (main: Keep and Mulligan
                // looked the same there).
                assert_ne!(m.g.view_of(v as u8), k.g.view_of(v as u8), "{name} seed {seed} viewer {v}: view_of");
            }
        }
    }
}

/// CR 103.5: the second player declares seeing the first player's declared
/// mulligan, in the first round and in a later one; once the mulligans are
/// taken (new hands, the counts up) the flags are clear again.
#[test]
fn cr_103_5_the_second_player_sees_the_declaration_and_it_clears() {
    use engine::DecisionKind as K;
    let mut checked = 0;
    for (name, sp) in opening_specs() {
        for seed in 0..6 {
            let g = first_declaration(&sp, seed);
            let s = g.g.turn().starting;
            let o = 1 - s;
            // Round 1: the starting player declares; the other decides seeing it.
            let m = after(&g, act::YES);
            let x = enc(&m, o as usize);
            assert_eq!(x[encode::O_CONTEXT + K::Mulligan as usize], 1.0);
            assert_eq!((x[MULL], x[MULL + 1]), (0.0, 1.0), "{name} seed {seed}: P{o} sees P{s}'s declaration");
            // Both declare: the mulligans are taken at once (one new hand each).
            let mut r = after(&m, act::YES);
            for p in [s, o] {
                assert!(!r.g.player(p).mulligan_declared, "{name} seed {seed}: P{p}'s flag after its mulligan");
                assert_eq!((r.g.player(p).mulligans, r.g.player(p).hand.len()), (1, 7));
            }
            assert_eq!((r.g.kind(), r.to_move()), (Some(K::MulliganBottom), Some(s as usize)));
            while r.g.kind() == Some(K::MulliganBottom) {
                for v in 0..2 {
                    let x = enc(&r, v);
                    assert_eq!((x[MULL], x[MULL + 1]), (0.0, 0.0), "{name} seed {seed}: bottoms");
                }
                let a = legal(&r)[0];
                r = after(&r, a);
            }
            // Round 2: the same, one card fewer.
            assert_eq!((r.g.kind(), r.to_move()), (Some(K::Mulligan), Some(s as usize)));
            assert_eq!((r.g.player(s).hand.len(), r.g.player(o).hand.len()), (6, 6));
            for v in 0..2 {
                let x = enc(&r, v);
                assert_eq!((x[MULL], x[MULL + 1]), (0.0, 0.0), "{name} seed {seed}: round 2 before the declaration");
            }
            let m2 = after(&r, act::YES);
            let x = enc(&m2, o as usize);
            assert_eq!((x[MULL], x[MULL + 1]), (0.0, 1.0), "{name} seed {seed}: round 2");
            // The other keeps: the starting player's second mulligan is taken.
            let t = after(&m2, act::PASS);
            assert!(!t.g.player(s).mulligan_declared && t.g.player(o).kept);
            assert_eq!(t.g.player(s).mulligans, 2);
            for v in 0..2 {
                let x = enc(&t, v);
                assert_eq!((x[MULL], x[MULL + 1]), (0.0, 0.0), "{name} seed {seed}: after round 2");
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 18);
}

/// In every opening state of the sampled openings (both viewers) the flags
/// are the players' `mulligan_declared`, set only while the second player
/// declares after the starting player declared a mulligan.
#[test]
fn cr_103_5_the_declared_flags_follow_the_game() {
    let mut set = 0;
    for (name, sp) in opening_specs() {
        for g in opening_states(&sp, 40, 22) {
            let s = g.g.turn().starting;
            if (0..2).any(|p| g.g.player(p).mulligan_declared) {
                set += 1;
                assert!(g.g.player(s).mulligan_declared && !g.g.player(1 - s).mulligan_declared, "{name}");
                assert_eq!((g.g.kind(), g.to_move()), (Some(engine::DecisionKind::Mulligan), Some(1 - s as usize)), "{name}");
            }
            for v in 0..2u8 {
                let x = enc(&g, v as usize);
                for (side, p) in [v, 1 - v].into_iter().enumerate() {
                    assert_eq!(x[MULL + side], g.g.player(p).mulligan_declared as u8 as f32, "{name}: P{p} for viewer P{v}");
                }
            }
        }
    }
    assert!(set > 30, "{set}");
}

/// The flag survives clones and `determinize` (for both viewers: the flag is
/// public, so the viewer's encoding is the same in every world, and every
/// world takes the declared mulligan); equal states encode equally (the
/// same game rebuilt and replayed; two worlds of the decider after the same
/// declaration: zero-core's child signature).
#[test]
fn the_declared_mulligan_survives_clones_and_determinize() {
    use engine::DecisionKind as K;
    let mut zr = zero_search::rng::Rng::new(55);
    for (name, sp) in opening_specs() {
        for seed in 0..6 {
            let g = first_declaration(&sp, seed);
            let s = g.g.turn().starting;
            let m = after(&g, act::YES);
            let rebuilt = after(&first_declaration(&sp, seed), act::YES);
            let c = m.clone();
            assert!(c.g.player(s).mulligan_declared);
            for v in 0..2 {
                assert_eq!(enc(&m, v), enc(&rebuilt, v), "{name} seed {seed}: equal states");
                assert_eq!(enc(&m, v), enc(&c, v), "{name} seed {seed}: a clone");
            }
            assert_eq!(after(&c, act::PASS).g.player(s).mulligans, 1);
            for v in 0..2 {
                for _ in 0..3 {
                    let mut d = m.clone();
                    d.determinize(v, &mut zr);
                    assert!(d.g.player(s).mulligan_declared, "{name} seed {seed}: determinized for P{v}");
                    assert_eq!(enc(&d, v), enc(&m, v), "{name} seed {seed}: the viewer's encoding in a world of P{v}");
                    let t = after(&d, act::PASS);
                    assert!(!t.g.player(s).mulligan_declared);
                    assert_eq!(t.g.player(s).mulligans, 1);
                    assert_eq!((t.g.kind(), t.to_move()), (Some(K::MulliganBottom), Some(s as usize)));
                }
            }
            // Two worlds of the deciding starting player, the same declaration.
            let (mut a, mut b) = (g.clone(), g.clone());
            a.determinize(s as usize, &mut zr);
            b.determinize(s as usize, &mut zr);
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            a.step(act::YES, &mut ka).unwrap();
            b.step(act::YES, &mut kb).unwrap();
            assert_eq!((ka, a.to_move(), legal(&a)), (kb, b.to_move(), legal(&b)));
            assert_eq!(enc(&a, s as usize), enc(&b, s as usize), "{name} seed {seed}: two worlds of P{s}");
            assert_eq!(enc(&a, s as usize)[MULL], 1.0);
        }
    }
}

/// The audit of the opening (ENGINE.md 55.4): in every sampled opening state
/// (the play-or-draw choice, companion reveals, declarations, bottoms,
/// opening-hand actions; cube, best-of-three, the Delver mirror) both
/// players' encodings and observable views are the same in every world
/// `determinize` makes for them (nothing hidden is encoded), and two worlds
/// of the decider played with the same action give it the same encoding
/// while the keys, the next decider and its options agree.
#[test]
fn the_opening_encodes_no_hidden_information() {
    let mut zr = zero_search::rng::Rng::new(56);
    let mut rng = Rng::new(56);
    let (mut worlds, mut pairs) = (0, 0);
    for (name, sp) in opening_specs() {
        for g in opening_states(&sp, 40, 23) {
            let mover = g.to_move().unwrap();
            for v in 0..2 {
                let (x, view) = (enc(&g, v), g.g.view_of(v as u8));
                for _ in 0..2 {
                    let mut d = g.clone();
                    d.determinize(v, &mut zr);
                    assert_eq!(d.to_move(), Some(mover));
                    let y = enc(&d, v);
                    if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                        panic!("{name}: P{v}'s feature {i} changed in a world at {:?} of P{mover}", g.g.kind());
                    }
                    assert_eq!(d.g.view_of(v as u8), view, "{name}: P{v}'s view at {:?}", g.g.kind());
                    worlds += 1;
                }
            }
            let (mut a, mut b) = (g.clone(), g.clone());
            a.determinize(mover, &mut zr);
            b.determinize(mover, &mut zr);
            let l = legal(&a);
            let x = l[rng.below(l.len() as u32) as usize];
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            if ka == kb && a.to_move() == b.to_move() && legal(&a) == legal(&b) {
                assert_eq!(enc(&a, mover), enc(&b, mover), "{name}: two worlds of P{mover} after {:?}", g.g.kind());
                pairs += 1;
            }
        }
    }
    println!("{worlds} worlds, {pairs} pairs of worlds compared");
    assert!(worlds > 3000 && pairs > 300, "{worlds} {pairs}");
}

// ---- The public-state encoding gate (ENGINE.md 58.6) ----

/// The positions of the public-state gate: mid-game states (mixed heuristic
/// and random play, one every ~`every` decisions, sideboarding left out) of
/// the cube's random decks, the Delver mirror, best-of-three matches, a
/// drafted match (a random 2v2 `team_draft` event's spec) and the cube's
/// random decks with hidden decklists (ENGINE.md 102; last, so the others'
/// positions are as before), `per` of each.
fn gate_positions(per: usize, seed: u64) -> Vec<(&'static str, MtgAz)> {
    use engine::az::draft::{DraftSpec, TeamDraftAz};
    let mut specs: Vec<(&'static str, MtgSpec)> = opening_specs();
    let mut ev = TeamDraftAz::new(&DraftSpec::default(), 4, seed).unwrap();
    let mut rng = Rng::new(seed);
    let mut keys = Vec::new();
    while ev.to_move().is_some() {
        let mut l = Vec::new();
        ev.legal_actions(&mut l);
        ev.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
    }
    let setup = ev.matches().unwrap().remove(0);
    let json = engine::az::draft_bots::match_spec(&setup, ev.ev.max_turns, ev.ev.format);
    specs.push(("drafted", MtgSpec::parse(&json).unwrap()));
    specs.push(("hidden decklists", MtgSpec::parse(r#"{"hidden_decklists": true}"#).unwrap()));
    // Edge of Eternities sealed (hidden decklists: the sealed spec's default): the EoE table, its
    // identity space (ENGINE.md 105); last, so the other kinds' positions are as before
    specs.push(("eoe", MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena"}, "max_turns": 60}"#).unwrap()));
    let mut out = Vec::new();
    for (name, sp) in specs {
        let mut h = Heuristic::new(seed);
        let mut n = 0;
        let mut s = 0;
        while n < per {
            let mut g = MtgAz::new_game(&sp, 2, seed * 1000 + s).unwrap();
            s += 1;
            while g.to_move().is_some() && n < per {
                if g.sideboarding().is_none() && g.g.turn().number >= 2 && rng.below(40) == 0 {
                    out.push((name, g.clone()));
                    n += 1;
                }
                let a = mixed(&g, &mut h, &mut rng);
                g.step(a, &mut keys).unwrap();
            }
        }
    }
    out
}

fn public_view(g: &MtgAz, v: u8) -> Vec<u64> {
    let mut x = g.g.view_of(v);
    x.push(u64::MAX - 77);
    x.extend(engine::public_state::match_view(g, v));
    x
}

/// The dual of the hidden-information checks (ENGINE.md 58.6): every public
/// field of the game state is classified (`public_state::fields`, an
/// exhaustive destructuring) and has a perturbation; each perturbation, made
/// on positions of the six kinds of games (cube hidden decklists since ENGINE.md 102, EoE sealed since 105), changes the public view for both
/// players (it is public) and the encoding for both viewers (it reaches the
/// network), but the deliberate limits (`public_state::limits`; layout v5,
/// ENGINE.md 60, left no gap).
#[test]
fn public_state_reaches_the_encoding() {
    use engine::public_state::{fields, limits, perturbations, Class, LimitKind};
    let t0 = std::time::Instant::now();
    let pos = gate_positions(scale(60) as usize, 59);
    let ps = perturbations();
    let lim = limits();
    // Each perturbation draws from a random stream of its own, keyed by its
    // name (ENGINE.md 100.5.1: with one shared stream a new perturbation
    // shifted every later one's picks, and what the gate found depended on
    // the perturbations before it). `GATE_SEED=n`: other streams.
    let seed: u64 = std::env::var("GATE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(5959);
    let fnv = |name: &str| name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let mut rngs: Vec<Rng> = ps.iter().map(|p| Rng::new(seed ^ fnv(p.name))).collect();
    #[derive(Default)]
    struct Tally {
        applied: usize,
        view_same: [usize; 2],
        enc_same: [usize; 2],
        example: Option<String>,
    }
    let mut t: Vec<Tally> = ps.iter().map(|_| Tally::default()).collect();
    // With hidden decklists (ENGINE.md 102) the fields `HIDDEN_BY_DECKLISTS` names are hidden
    // from the other player: such a perturbation must change its owner's view and encoding and
    // neither of the other player's.
    let (mut hidden_ok, mut leaks) = (0, Vec::new());
    for (spec, a) in &pos {
        let base_enc = [enc(a, 0), enc(a, 1)];
        let base_view = [public_view(a, 0), public_view(a, 1)];
        for (k, p) in ps.iter().enumerate() {
            let mut b = a.clone();
            let Some(what) = (p.apply)(&mut b, &mut rngs[k]) else { continue };
            if a.g.spec().hidden_decklists && engine::public_state::HIDDEN_BY_DECKLISTS.contains(&p.name) {
                let view: Vec<bool> = (0..2).map(|v| public_view(&b, v as u8) != base_view[v]).collect();
                let code: Vec<bool> = (0..2).map(|v| enc(&b, v) != base_enc[v]).collect();
                if view.iter().filter(|&&x| x).count() == 1 && view == code {
                    hidden_ok += 1;
                } else {
                    leaks.push(format!("{spec}: {what}: views changed {view:?}, encodings {code:?}"));
                }
                continue;
            }
            t[k].applied += 1;
            for v in 0..2 {
                if public_view(&b, v as u8) == base_view[v] {
                    t[k].view_same[v] += 1;
                    t[k].example.get_or_insert_with(|| format!("{spec}: {what} (P{v}'s view unchanged)"));
                }
                if enc(&b, v) == base_enc[v] {
                    t[k].enc_same[v] += 1;
                    t[k].example.get_or_insert_with(|| format!("{spec}: {what} (P{v}'s encoding unchanged)"));
                }
            }
        }
    }
    let f = fields();
    let count = |c: Class| f.iter().filter(|x| x.class == c).count();
    println!(
        "{} positions, {} fields ({} public, {} hidden, {} derived), {} perturbations, {:.1} s",
        pos.len(),
        f.len(),
        count(Class::Public),
        count(Class::Hidden),
        count(Class::Derived),
        ps.len(),
        t0.elapsed().as_secs_f64()
    );
    let mut findings = Vec::new();
    let (mut ok, mut by_design, mut next) = (0, 0, 0);
    for (p, x) in ps.iter().zip(&t) {
        let limited = lim.iter().find(|l| l.name == p.name).map(|l| l.kind);
        let view_bad = x.view_same.iter().any(|&n| n > 0);
        let enc_bad = x.enc_same.iter().any(|&n| n > 0);
        let state = match (x.applied, view_bad, enc_bad, limited) {
            (0, _, _, None) if p.rare => "rare, not applied",
            (0, _, _, Some(_)) if p.rare => "rare limit",
            (0, ..) => "NOT APPLIED",
            (_, true, ..) => "VIEW GAP",
            (_, false, true, Some(LimitKind::Design)) => "limit (design)",
            (_, false, true, Some(LimitKind::NextLayout)) => "gap (next layout)",
            (_, false, true, None) => "ENCODING GAP",
            (_, false, false, Some(_)) => "STALE LIMIT",
            _ => "ok",
        };
        match state {
            "ok" => ok += 1,
            "limit (design)" => by_design += 1,
            "gap (next layout)" | "rare limit" => next += 1,
            _ => {}
        }
        println!(
            "{state:>18} | {:<58} | {:<26} | {:>4} applied, view same {:?}, encoding same {:?}{}",
            p.name,
            p.field,
            x.applied,
            x.view_same,
            x.enc_same,
            x.example.as_deref().map(|e| format!(" | {e}")).unwrap_or_default()
        );
        if !matches!(state, "ok" | "limit (design)" | "gap (next layout)" | "rare, not applied" | "rare limit") {
            findings.push(format!("{state}: {} ({})", p.name, p.field));
        }
    }
    println!("{ok} perturbations encoded, {by_design} deliberate limits, {next} gaps for the next layout (ENGINE.md 58.10, 60)");
    println!("hidden decklists: {hidden_ok} perturbations of fields they hide reached only their owner");
    assert!(leaks.is_empty(), "hidden decklists: {} perturbations of hidden fields reached the other player:\n{}", leaks.len(), leaks.join("\n"));
    assert!(hidden_ok > 0, "hidden decklists: no perturbation of a field they hide applied");
    assert!(findings.is_empty(), "{} findings:\n{}", findings.len(), findings.join("\n"));
    // Layout v5 (ENGINE.md 60) encodes layout v4's 42 gaps, layout v6 (ENGINE.md 100.5) the Edge
    // of Eternities port's and workstream C's (`public_state::V6_FIXED`), and the graveyard's
    // order is no id's since Priority's casts from a graveyard became one option and a list
    // choice (ENGINE.md 100.5): only deliberate limits are left.
    let gaps: Vec<&str> = lim.iter().filter(|l| l.kind == LimitKind::NextLayout).map(|l| l.name).collect();
    // (W2's and W6's six, added after layout v6 was built, too: ENGINE.md 100.5.1.)
    assert!(gaps.is_empty(), "next-layout gaps: {gaps:?}");
    assert_eq!(next, 0);
    // (Layout v7 encodes the library cards known below the top three, ENGINE.md 125.)
    assert_eq!(by_design, 3, "the deliberate limits");
}

/// The gate's classification (ENGINE.md 58.7), as a table.
#[test]
fn public_state_classification() {
    use engine::public_state::{fields, perturbations, Class};
    let ps = perturbations();
    for x in fields() {
        let n = ps.iter().filter(|p| p.field == x.path).count();
        println!("{:<28} {:<8} {} | {}", x.path, format!("{:?}", x.class), n, x.why);
        if x.class == Class::Public {
            assert!(n > 0, "{} has no perturbation", x.path);
        }
    }
}

/// Answer decisions (`prefer`, else the first option) and pass priority until
/// the stack is empty at a priority decision.
fn settle(g: &mut engine::Game, prefer: impl Fn(&engine::Game) -> Option<engine::Choice>) {
    use engine::{Choice, DecisionKind};
    for _ in 0..300 {
        let Some(d) = g.pending_decision() else { return };
        if d.kind == DecisionKind::Priority {
            if g.stack().is_empty() {
                return;
            }
            g.pass();
            continue;
        }
        let c: Choice = prefer(g).filter(|&c| g.has_option(c)).unwrap_or(d.options[0].choice);
        g.choose(c);
    }
    panic!("settle: no progress\n{}", g.log_lines().join("\n"));
}

/// `v`'s whole row of game `g` (`tail_encode::Row::flat`: the head first).
fn features(g: &engine::Game, v: u8) -> Vec<f32> {
    engine::az::tail_encode::Row::of(g, v).flat()
}

/// The public-state gate's encoding fixes (ENGINE.md 58.8), each read back
/// from its float for both viewers: the monarch (g[127]), spells cast this
/// turn and energy (the match block's 27-30), X on the stack (stack status
/// 31), and a permanent's cards exiled with it, other counters and protection
/// (permanent status 93-95; can't block / can't be blocked and the descriptor's
/// types and colours as they are are the gate's perturbations').
#[test]
fn the_public_state_fixes_are_encoded() {
    use encode::{glob, mtch, perm, stack, O_GLOBALS, O_MATCH, O_MY_PERM, O_OPP_PERM, O_STACK, PERM_W};
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Zone};
    let db = CardDb::cube();
    let perm_slot = |g: &engine::Game, v: u8, c: engine::CardIdx| -> usize {
        let ctl = g.obj(c).controller;
        let k = g.permanents_of(ctl).position(|x| x == c).unwrap();
        (if ctl == v { O_MY_PERM } else { O_OPP_PERM }) + k * PERM_W + CODE
    };
    // Palace Jailer: its controller becomes the monarch, an opponent's
    // creature is exiled with it; then Walking Ballista is cast with X = 2.
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Plains"; 10])
        .library(1, &["Island"; 10])
        .hand(0, &["Palace Jailer", "Walking Ballista"])
        .pool(0, [2, 0, 0, 0, 0, 6], 0)
        .battlefield(1, &["Llanowar Elves"])
        .start();
    for v in 0..2 {
        let x = features(&g, v);
        assert_eq!((x[O_GLOBALS + glob::MONARCH], x[O_MATCH + mtch::SPELLS], x[O_MATCH + mtch::SPELLS + 1]), (0.0, 0.0, 0.0));
    }
    let elves = g.card(1, "Llanowar Elves", Zone::Battlefield);
    let jailer = g.card(0, "Palace Jailer", Zone::Hand);
    g.choose(Choice::Cast(jailer));
    let t = g.tgt(elves);
    settle(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    let jailer = g.card(0, "Palace Jailer", Zone::Battlefield);
    assert_eq!(g.obj(elves).zone, Zone::Exile);
    for (v, me) in [(0u8, 1.0f32), (1, -1.0)] {
        let x = features(&g, v);
        assert_eq!(x[O_GLOBALS + glob::MONARCH], me, "P{v}: the monarch");
        assert_eq!(x[perm_slot(&g, v, jailer) + perm::EXILED_WITH], 0.25, "P{v}: the Elves exiled with Palace Jailer");
        let side = v as usize; // P0 cast one spell: the viewer's own float for P0, the opponent's for P1
        assert_eq!(x[O_MATCH + mtch::SPELLS + side], 0.25, "P{v}: spells cast this turn");
        assert_eq!(x[O_MATCH + mtch::SPELLS + 1 - side], 0.0);
    }
    let wb = g.card(0, "Walking Ballista", Zone::Hand);
    g.choose(Choice::Cast(wb));
    for _ in 0..20 {
        match g.kind() {
            Some(DecisionKind::Priority) => break,
            Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(2)) => g.choose(Choice::Number(2)),
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c)
            }
        };
    }
    assert_eq!(g.stack().last().unwrap().x, 2);
    for v in 0..2u8 {
        let x = features(&g, v);
        assert_eq!(x[O_STACK + CODE + stack::X], 0.25, "P{v}: X = 2 on the stack");
        assert_eq!(x[O_MATCH + mtch::SPELLS + v as usize], 0.5, "P{v}: two spells");
    }
    // Other counters and protection: a Walking Ballista with two stun
    // counters, True-Name Nemesis (protection from the chosen player).
    let g = Scenario::new(db)
        .battlefield(0, &["True-Name Nemesis", "Walking Ballista"])
        .counters(0, "Walking Ballista", engine::db::Counter::Stun, 2)
        .counters(0, "Walking Ballista", engine::db::Counter::P1P1, 3)
        .start();
    let (tnn, wb) = (g.card(0, "True-Name Nemesis", Zone::Battlefield), g.card(0, "Walking Ballista", Zone::Battlefield));
    for v in 0..2 {
        let x = features(&g, v);
        assert_eq!((x[perm_slot(&g, v, tnn) + perm::PROTECTED], x[perm_slot(&g, v, wb) + perm::PROTECTED]), (1.0, 0.0));
        assert_eq!(x[perm_slot(&g, v, wb) + perm::OTHER_COUNTERS], 0.5, "P{v}: the stun counters, not the +1/+1 ones");
        assert_eq!((x[perm_slot(&g, v, wb) + perm::CANT_BLOCK], x[perm_slot(&g, v, wb) + perm::CANT_BE_BLOCKED]), (0.0, 0.0));
    }
    // Energy: Galvanic Discharge gives three, none paid.
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Mountain"; 10])
        .library(1, &["Island"; 10])
        .hand(0, &["Galvanic Discharge"])
        .pool(0, [0, 0, 0, 1, 0, 0], 0)
        .battlefield(1, &["Llanowar Elves"])
        .start();
    let elves = g.card(1, "Llanowar Elves", Zone::Battlefield);
    let gd = g.card(0, "Galvanic Discharge", Zone::Hand);
    g.choose(Choice::Cast(gd));
    let t = g.tgt(elves);
    settle(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.player(0).energy, 3, "{}", g.log_lines().join("\n"));
    for v in 0..2u8 {
        let x = features(&g, v);
        let side = v as usize;
        assert_eq!((x[O_MATCH + mtch::ENERGY + side], x[O_MATCH + mtch::ENERGY + 1 - side]), (0.75, 0.0), "P{v}: P0's three energy");
    }
}

// ---- Layout v5 (ENGINE.md 60) ----

/// Answer decisions (`answer`, else the first option) until a priority
/// decision.
fn to_priority(g: &mut engine::Game, answer: impl Fn(&engine::Game) -> Option<engine::Choice>) {
    for _ in 0..300 {
        let Some(d) = g.pending_decision() else { return };
        if d.kind == engine::DecisionKind::Priority {
            return;
        }
        let c = answer(g).filter(|&c| g.has_option(c)).unwrap_or(d.options[0].choice);
        g.choose(c);
    }
    panic!("to_priority: no progress\n{}", g.log_lines().join("\n"));
}

/// The facts layout v5 encodes (ENGINE.md 60.3), read back from their floats
/// for both viewers, for the Delver mirror's cards first: Prismari Charm's
/// chosen mode, Force of Will's pitch (an alternative cost), the cards
/// Murktide Regent delved, and the counts of Brainstorm's turn (cards drawn
/// this turn, noncreature spells this turn, spells this game); then a
/// permanent's v5 status (the chosen player and colour, exerted, evoked, must
/// attack, abilities resolved).
#[test]
fn the_v5_facts_are_encoded() {
    use encode::{g5, perm, stack, O_GLOBALS_V5, O_MY_PERM, O_OPP_PERM, O_STACK, PERM_W};
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Target, Zone};
    let at = |side: usize, k: usize| O_GLOBALS_V5 + g5::PLAYER * side + k;
    let top = O_STACK + CODE;
    let scen = || {
        Scenario::new(CardDb::constructed()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
    };
    // Prismari Charm, mode 2 (return target nonland permanent), on the stack.
    let mut g = scen()
        .battlefield(0, &["Volcanic Island", "Volcanic Island"])
        .hand(0, &["Prismari Charm"])
        .battlefield(1, &["Dragon's Rage Channeler"])
        .at(Step::Main1, 0)
        .start();
    let drc = g.card(1, "Dragon's Rage Channeler", Zone::Battlefield);
    let t = g.tgt(drc);
    let charm = g.card(0, "Prismari Charm", Zone::Hand);
    g.choose(Choice::Cast(charm));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    to_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.stack().last().unwrap().modes, vec![2]);
    for v in 0..2u8 {
        let x = features(&g, v);
        let modes: Vec<f32> = (0..stack::MODES_N).map(|k| x[top + stack::MODES + k]).collect();
        assert_eq!(modes, [0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0], "P{v}: Prismari Charm's mode 2");
        assert_eq!((x[top + stack::ALT], x[top + stack::DELVED]), (0.0, 0.0));
        let side = v as usize;
        assert_eq!(x[at(side, g5::NONCREATURE)], 0.25, "P{v}: P0's noncreature spell this turn");
        assert_eq!(x[at(side, g5::SPELLS_GAME)], 1.0 / 16.0, "P{v}: P0's spell this game");
        assert_eq!((x[at(1 - side, g5::NONCREATURE)], x[at(1 - side, g5::SPELLS_GAME)]), (0.0, 0.0));
    }
    // Force of Will for its alternative cost (exile a blue card, pay 1 life).
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Force of Will", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    to_priority(&mut g, |_| Some(Choice::Target(Target::Player(1))));
    g.pass();
    let fow = g.card(1, "Force of Will", Zone::Hand);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(fow));
    to_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(cs)),
        _ => None,
    });
    let item = g.stack().last().unwrap();
    assert_eq!(item.alt, 1, "{}", g.log_lines().join("\n"));
    for v in 0..2u8 {
        let x = features(&g, v);
        assert_eq!((x[top + stack::ALT], x[top + stack::ALT_INDEX]), (1.0, 0.25), "P{v}: Force of Will's pitch");
        let bolt_slot = top + encode::STACK_W;
        assert_eq!(x[bolt_slot + stack::ALT], 0.0, "P{v}: the Bolt for its mana cost");
    }
    // Murktide Regent delving five cards, on the stack.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Lightning Bolt", "Volcanic Island", "Mishra's Bauble"])
        .hand(0, &["Murktide Regent"])
        .at(Step::Main1, 0)
        .start();
    let picks = g.player(0).graveyard.clone();
    let m = g.card(0, "Murktide Regent", Zone::Hand);
    g.choose(Choice::Cast(m));
    let k = std::cell::Cell::new(0);
    to_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseGraveyardCard) if k.get() < picks.len() => {
            k.set(k.get() + 1);
            Some(Choice::Card(picks[k.get() - 1]))
        }
        Some(DecisionKind::ChooseGraveyardCard) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.stack().last().unwrap().delved, 5, "{}", g.log_lines().join("\n"));
    for v in 0..2u8 {
        let x = features(&g, v);
        assert_eq!(x[top + stack::DELVED], 1.25, "P{v}: five cards delved");
    }
    // Brainstorm: three cards drawn this turn.
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Brainstorm"]).at(Step::Main1, 0).start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    settle(&mut g, |_| None);
    assert_eq!(g.player(0).drawn, 3, "{}", g.log_lines().join("\n"));
    for v in 0..2u8 {
        let x = features(&g, v);
        let side = v as usize;
        assert_eq!(x[at(side, g5::DRAWN)], 0.75, "P{v}: P0 drew three");
        assert_eq!(x[at(1 - side, g5::DRAWN)], 0.0);
    }
    // True-Name Nemesis (the chosen player), Painter's Servant (the chosen
    // colour), and a permanent's other v5 floats set by hand.
    let mut g = Scenario::new(CardDb::cube())
        .spec(|s| s.auto_single = false)
        .library(0, &["Plains"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Island"; 5])
        .hand(0, &["True-Name Nemesis", "Painter's Servant"])
        .at(Step::Main1, 0)
        .start();
    for name in ["True-Name Nemesis", "Painter's Servant"] {
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        settle(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
            Some(DecisionKind::ChooseColor) => Some(Choice::Number(3)),
            _ => None,
        });
    }
    let tnn = g.card(0, "True-Name Nemesis", Zone::Battlefield);
    let ps = g.card(0, "Painter's Servant", Zone::Battlefield);
    let slot = |g: &engine::Game, v: u8, c: engine::CardIdx| -> usize {
        let k = g.permanents_of(0).position(|x| x == c).unwrap();
        (if v == 0 { O_MY_PERM } else { O_OPP_PERM }) + k * PERM_W + CODE
    };
    for (v, chosen) in [(0u8, -1.0f32), (1, 1.0)] {
        let x = features(&g, v);
        assert_eq!(x[slot(&g, v, tnn) + perm::CHOSEN_PLAYER], chosen, "P{v}: True-Name Nemesis chose P1\n{}", g.log_lines().join("\n"));
        let colors: Vec<f32> = (0..5).map(|k| x[slot(&g, v, ps) + perm::CHOSEN_COLOR + k]).collect();
        assert_eq!(colors, [0.0, 0.0, 0.0, 1.0, 0.0], "P{v}: Painter's Servant chose red");
        assert_eq!(x[slot(&g, v, ps) + perm::CHOSEN_PLAYER], 0.0);
    }
}

// ---- Graveyard choices on the list's ids (ENGINE.md 65) ----

/// The card a graveyard-list option names (ENGINE.md 65.10): chosen or
/// targeted in a graveyard.
fn gy_card(g: &engine::Game, c: engine::Choice) -> Option<engine::CardIdx> {
    use engine::{Choice, Target, Zone};
    let i = match c {
        Choice::Card(i) => i,
        Choice::Target(Target::Obj(r)) => r.idx,
        // (A cast or play from the graveyard, ENGINE.md 100.5.)
        Choice::Cast(i) | Choice::Play(i) | Choice::CastFace(i) | Choice::PlayFace(i) | Choice::Faces(i, _) => i,
        _ => return None,
    };
    (g.obj(i).zone == Zone::Graveyard).then_some(i)
}

/// Gap #1 of the public-state gate (ENGINE.md 100.5): Priority's casts and
/// plays from the graveyard are one option, `GRAVEYARD` + 0 ("cast or play
/// from the graveyard"), whatever the graveyard's order; then a
/// `ChooseGraveyardCard` over the graveyard list asks which, each option
/// `LIBRARY` + its card's slot with that card's token (`check_graveyard_list`),
/// exactly the casts Priority had; the cast goes on as Priority's (CR
/// 702.34a: Lingering Souls's flashback, then exiled).
#[test]
fn cr_702_34a_a_cast_from_the_graveyard_is_one_priority_option_then_the_cards_list_token() {
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    for order in [["Lingering Souls", "Island"], ["Island", "Lingering Souls"]] {
        let mut g = Scenario::new(CardDb::cube())
            .spec(|s| s.auto_single = false)
            .battlefield(0, &["Swamp", "Plains"])
            .graveyard(0, &order)
            .library(0, &["Island"; 5])
            .library(1, &["Island"; 5])
            .at(Step::Main1, 0)
            .start();
        let ls = g.card(0, "Lingering Souls", Zone::Graveyard);
        let d = g.pending_decision().unwrap().clone();
        assert_eq!(d.kind, DecisionKind::Priority);
        let gy: Vec<_> = d.options.iter().filter(|o| (act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&o.action)).collect();
        assert_eq!(gy.len(), 1, "{order:?}: one graveyard option");
        assert_eq!((gy[0].action, gy[0].choice), (act::GRAVEYARD, Choice::FromGraveyard), "{order:?}");
        let mut keys = Vec::new();
        g.apply(act::GRAVEYARD, &mut keys).unwrap();
        assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
        assert_eq!(g.to_move(), Some(0));
        let list = check_graveyard_list(&g, &format!("{order:?}"));
        assert_eq!(list, g.player(0).graveyard, "the whole graveyard, in order");
        let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
        assert_eq!(opts, vec![Choice::Cast(ls)], "exactly Priority's cast");
        g.choose(Choice::Cast(ls));
        for _ in 0..30 {
            if g.kind() == Some(DecisionKind::Priority) && g.stack().is_empty() {
                break;
            }
            let c = match g.kind() {
                Some(DecisionKind::Priority) => Choice::Pass,
                _ => g.pending_decision().unwrap().options[0].choice,
            };
            g.choose(c);
        }
        assert_eq!(g.obj(ls).zone, Zone::Exile, "{order:?}: flashback, then exiled
{}", g.log_lines().join("
"));
        assert_eq!(g.count_in(0, "Spirit", Zone::Battlefield), 2);
    }
}

/// The pending decision's graveyard list (ENGINE.md 65.10), checked card by
/// card against the network's input: the list is the graveyard of the
/// offered cards, whole and in order, or with offered cards in both
/// graveyards the offered cards only, the decider's first, each graveyard in
/// order; each option naming a graveyard card is `LIBRARY` + k, the list
/// segment's pointer action of slot k (the layout's), and the decider's list
/// token in slot k is exactly that card (its identity, present, an option,
/// whose graveyard); every other slot up to the list's end holds the next
/// card of the list, not an option, and the slots after it are empty; the
/// other viewer's list block is empty (not its decision); no option keeps a
/// `GRAVEYARD` id. Returns the list.
fn check_graveyard_list(g: &engine::Game, what: &str) -> Vec<engine::CardIdx> {
    use encode::{list, LIST, LIST_W, O_LIST};
    let d = g.pending_decision().expect("a decision").clone();
    let p = d.player;
    let ids = identity::identities(g.db());
    let offered: Vec<engine::CardIdx> = d.options.iter().filter_map(|o| gy_card(g, o.choice)).collect();
    let sides: Vec<u8> = [p, 1 - p].into_iter().filter(|&q| offered.iter().any(|&c| g.obj(c).owner == q)).collect();
    let mut want: Vec<engine::CardIdx> = sides.iter().flat_map(|&q| g.player(q).graveyard.iter().copied()).collect();
    if sides.len() == 2 {
        want.retain(|c| offered.contains(c));
    }
    assert!(!want.is_empty(), "{what}: no graveyard card offered: {}", g.options_desc().join(" | "));
    assert_eq!(g.graveyard_list(p, d.kind, &d.options), want, "{what}: the graveyard list");
    // The layout: list slot k scores LIBRARY + k from its token.
    let layout: serde_json::Value = serde_json::from_str(&encode::layout_json(g.db())).unwrap();
    let seg = layout["segments"].as_array().unwrap().iter().find(|s| s["name"] == "list").unwrap().clone();
    assert_eq!((seg["offset"].as_u64(), seg["count"].as_u64(), seg["width"].as_u64()), (Some(O_LIST as u64), Some(LIST as u64), Some(LIST_W as u64)));
    assert_eq!(seg["actions"], serde_json::json!([[act::LIBRARY, 1]]));
    let x = features(g, p);
    let mut named = 0;
    for o in &d.options {
        assert!(
            !(act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&o.action),
            "{what}: {:?} keeps a GRAVEYARD id",
            o.choice
        );
        let Some(c) = gy_card(g, o.choice) else {
            assert!(!(act::LIBRARY..act::LIBRARY + act::LIBRARY_N).contains(&o.action), "{what}: {:?}", o.choice);
            continue;
        };
        let k = want.iter().position(|&y| y == c).unwrap();
        assert_eq!(o.action, act::LIBRARY + k as u16, "{what}: {} is list slot {k}", g.name(c));
        let s = &x[O_LIST + k * LIST_W..O_LIST + (k + 1) * LIST_W];
        assert_eq!(decode_id(&s[..CODE]), ids[g.obj(c).def as usize] as usize, "{what}: slot {k}'s token is {}", g.name(c));
        assert_eq!((s[CODE + list::PRESENT], s[CODE + list::OPTION]), (1.0, 1.0), "{what}: {}", g.name(c));
        assert_eq!(s[CODE + list::THEIRS], (g.obj(c).owner != p) as u8 as f32, "{what}: {}'s graveyard", g.name(c));
        named += 1;
    }
    assert!(named > 0);
    for k in 0..LIST {
        let s = &x[O_LIST + k * LIST_W..O_LIST + (k + 1) * LIST_W];
        match want.get(k) {
            Some(&c) => {
                assert_eq!(decode_id(&s[..CODE]), ids[g.obj(c).def as usize] as usize, "{what}: slot {k}");
                assert_eq!(s[CODE + list::PRESENT], 1.0);
                assert_eq!(s[CODE + list::OPTION] == 1.0, d.options.iter().any(|o| o.action == act::LIBRARY + k as u16), "{what}: slot {k}");
                assert_eq!(s[CODE + list::THEIRS], (g.obj(c).owner != p) as u8 as f32);
                let copies = want.iter().filter(|&&y| g.obj(y).def == g.obj(c).def).count();
                assert_eq!(s[CODE + list::COPIES], copies as f32 / 4.0);
            }
            None => assert!(s.iter().all(|&v| v == 0.0), "{what}: slot {k} after the list"),
        }
    }
    let y = features(g, 1 - p);
    assert!(y[O_LIST..O_LIST + LIST * LIST_W].iter().all(|&v| v == 0.0), "{what}: P{}'s list block (not its decision)", 1 - p);
    want
}

/// A real Murktide Regent delve, the graveyard of the Delver mirror's replay
/// 20261029-080154_002 at turn 8 (Ponder, Daze, Mishra's Bauble, Misty
/// Rainforest, Lightning Bolt, Volcanic Island, Brainstorm, Polluted Delta;
/// two Islands: five cards to delve), picked Ponder, Daze, Bolt, Brainstorm,
/// Delta, cast by each player in turn (the other's graveyard holds two cards,
/// not offered). At every pick:
/// * the action mapping (ENGINE.md 65.10): the list is the caster's graveyard
///   in order (bottom first; the other's isn't offered, so not in it); each
///   option is `LIBRARY` + its card's position, the list token there is that
///   card for the caster's encoding (`check_graveyard_list`), the option's
///   text names it, and the positions are stable while picking (the chosen
///   cards stay in the graveyard until the payment ends, ENGINE.md 51.6); the
///   options are the positions above the last pick that leave enough cards
///   for the picks still needed (each set once); the cards chosen so far are
///   `list::CHOSEN`, the passed ones below the last pick present and not
///   options;
/// * the encoding (ENGINE.md 65.3, both viewers): the context's card is the
///   Regent, its cost floats the mana cost left ({5}{U}{U} less one generic
///   per pick), the stack slot's cards delved, and each card chosen so far is
///   in its owner's exile bag, out of the graveyard bag;
/// * what stays blind: no segment scores a `GRAVEYARD` id (Priority's casts
///   from a graveyard: flashback, escape, ENGINE.md 65.10).
/// The Regent enters with four counters (the four instants and sorceries).
#[test]
fn murktide_delve_maps_each_graveyard_id_to_its_card_and_encodes_the_picks() {
    use encode::{bag, ctx, list, stack, CONTEXT, LIST_W, O_BAGS, O_CONTEXT, O_LIST, O_STACK};
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    let db = CardDb::constructed();
    let ids = identity::identities(db);
    // (The layout's pointer actions: [base, stride] per segment slot.)
    let layout: serde_json::Value = serde_json::from_str(&encode::layout_json(db)).unwrap();
    for s in layout["segments"].as_array().unwrap() {
        let count = s["count"].as_u64().unwrap() as u16;
        for pair in s["actions"].as_array().into_iter().flatten() {
            let (base, stride) = (pair[0].as_u64().unwrap() as u16, pair[1].as_u64().unwrap() as u16);
            assert!(
                !(0..count).any(|k| (act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&(base + k * stride))),
                "segment {} scores GRAVEYARD ids: Priority's graveyard casts are no longer blind (ENGINE.md 65.10), update this test",
                s["name"]
            );
        }
        assert!(s.get("id_actions").is_none());
    }
    let names = ["Ponder", "Daze", "Mishra's Bauble", "Misty Rainforest", "Lightning Bolt", "Volcanic Island", "Brainstorm", "Polluted Delta"];
    for caster in 0..2u8 {
        let other = 1 - caster;
        let mut g = Scenario::new(db)
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 10])
            .library(1, &["Island"; 10])
            .battlefield(caster, &["Island", "Island"])
            .graveyard(caster, &names)
            .graveyard(other, &["Brainstorm", "Island"])
            .hand(caster, &["Murktide Regent"])
            .at(Step::Main1, caster)
            .start();
        let gy = g.player(caster).graveyard.clone();
        assert_eq!(gy.iter().map(|&c| g.name(c)).collect::<Vec<_>>(), names, "bottom first");
        let m = g.card(caster, "Murktide Regent", Zone::Hand);
        let regent = ids[g.obj(m).def as usize] as usize;
        let idents: Vec<usize> = gy.iter().map(|&c| ids[g.obj(c).def as usize] as usize).collect();
        g.choose(Choice::Cast(m));
        to_choice(&mut g, DecisionKind::ChooseGraveyardCard);
        let picks = [0usize, 1, 4, 6, 7];
        let mut floor = 0;
        for (n, &pick) in picks.iter().enumerate() {
            let what = format!("P{caster}'s Regent, pick {n}");
            assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{what}: {}", g.options_desc().join(" | "));
            assert_eq!(g.player(caster).graveyard, gy, "{what}: the chosen cards stay in place");
            let d = g.pending_decision().unwrap().clone();
            assert_eq!(d.player, caster);
            assert_eq!(check_graveyard_list(&g, &what), gy, "{what}: the list is the caster's graveyard");
            let view = g.spectator_view();
            let text = |a: u16| -> String {
                let o = view["decision"]["options"].as_array().unwrap().iter().find(|o| o["a"] == a).unwrap();
                o["t"].as_str().unwrap().to_string()
            };
            let left = picks.len() - n;
            let want: Vec<u16> = (floor..=gy.len() - left).map(|k| act::LIBRARY + k as u16).collect();
            assert_eq!(d.options.iter().map(|o| o.action).collect::<Vec<_>>(), want, "{what}: above the last pick, {left} still needed");
            for o in &d.options {
                let Choice::Card(c) = o.choice else { panic!("{what}: {:?}", o.choice) };
                let k = gy.iter().position(|&x| x == c).unwrap();
                assert_eq!(o.action, act::LIBRARY + k as u16, "{what}: {} is graveyard position {k}", g.name(c));
                assert_eq!(text(o.action), format!("Choose {}", names[k]), "{what}: the option's text names its card");
                assert_eq!(ids[g.obj(c).def as usize] as usize, idents[k]);
            }
            for v in [caster, other] {
                let x = features(&g, v);
                let c = &x[O_CONTEXT..O_CONTEXT + CONTEXT];
                assert_eq!(c[ctx::KIND + DecisionKind::ChooseGraveyardCard as usize], 1.0);
                assert_eq!((c[ctx::HAS_CARD], decode_id(&c[ctx::CARD..ctx::CARD + CODE])), (1.0, regent), "P{v}, {what}: the context's card");
                let cost: Vec<f32> = (0..9).map(|k| c[ctx::COST + k]).collect();
                assert_eq!(cost, [(5 - n) as f32 / 8.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], "P{v}, {what}: {{{}}}{{U}}{{U}} left", 5 - n);
                assert_eq!(x[O_STACK + CODE + stack::DELVED], n as f32 / 4.0, "P{v}, {what}: cards delved");
                let (gb, eb) = if v == caster { (bag::MY_GRAVEYARD, bag::MY_EXILE) } else { (bag::OPP_GRAVEYARD, bag::OPP_EXILE) };
                for (k, &id) in idents.iter().enumerate() {
                    let chosen = picks[..n].contains(&k);
                    // (The other player's Brainstorm is in its owner's bag.)
                    let at = |b: usize| x[O_BAGS + b * encode::D + id];
                    assert_eq!((at(gb), at(eb)), if chosen { (0.0, 0.25) } else { (0.25, 0.0) }, "P{v}, {what}: {} chosen {chosen}", names[k]);
                }
                // The list's chosen cards: the caster's only.
                for k in 0..gy.len() {
                    let s = &x[O_LIST + k * LIST_W..O_LIST + (k + 1) * LIST_W];
                    let chosen = picks[..n].contains(&k);
                    if v == caster {
                        assert_eq!(s[CODE + list::CHOSEN], chosen as u8 as f32, "{what}: slot {k} ({}) chosen {chosen}", names[k]);
                        assert_eq!(s[CODE + list::OPTION] == 1.0, k >= floor && k <= gy.len() - left, "{what}: slot {k}");
                    }
                }
            }
            g.choose(Choice::Card(gy[pick]));
            floor = pick + 1;
        }
        // The payment ends: the five exiled (the bags as during the picks), the Regent resolves.
        to_priority(&mut g, |_| None);
        assert!(gy.iter().enumerate().all(|(k, &c)| (g.obj(c).zone == Zone::Exile) == picks.contains(&k)), "{}", g.log_lines().join("\n"));
        settle(&mut g, |_| None);
        let m = g.card(caster, "Murktide Regent", Zone::Battlefield);
        assert_eq!(g.counters_on(m, engine::db::Counter::P1P1), 4, "{}", g.log_lines().join("\n"));
    }
}

/// Tamiyo, Seasoned Scholar's -3 ("return target instant or sorcery card
/// from your graveyard to your hand"; the Delver mirror's other graveyard
/// choice, ENGINE.md 65.1): a `Target` decision whose options are list ids
/// (ENGINE.md 65.10), for each player as the controller. The list is the
/// controller's graveyard in order (its lands and creatures present, not
/// options; the other graveyard's instants aren't targets of "your
/// graveyard": not in it), each target `LIBRARY` + its card's position,
/// pointing at that card's token; the chosen card returns.
#[test]
fn tamiyo_minus_three_targets_each_graveyard_card_by_its_list_token() {
    use engine::db::Counter;
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
    const SCHOLAR: &str = "Tamiyo, Seasoned Scholar";
    let names = ["Ponder", "Island", "Force of Will", "Dragon's Rage Channeler", "Lightning Bolt", "Ponder"];
    for p in 0..2u8 {
        let mut g = Scenario::new(CardDb::constructed())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 10])
            .library(1, &["Island"; 10])
            .battlefield(p, &[TAMIYO])
            .transformed(p, TAMIYO)
            .counters(p, SCHOLAR, Counter::Loyalty, 3)
            .graveyard(p, &names)
            .graveyard(1 - p, &["Brainstorm", "Daze"])
            .at(Step::Main1, p)
            .start();
        let t = g.card(p, SCHOLAR, Zone::Battlefield);
        let gy = g.player(p).graveyard.clone();
        let ability = g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Ability(c, s) if c == t => Some(Choice::Ability(c, s)),
                _ => None,
            })
            .nth(1)
            .expect("the -3");
        g.choose(ability);
        assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", g.options_desc().join(" | "));
        let what = format!("P{p}'s Tamiyo -3");
        assert_eq!(check_graveyard_list(&g, &what), gy, "{what}: the list is the controller's graveyard");
        let d = g.pending_decision().unwrap().clone();
        let offered: Vec<usize> = d.options.iter().filter_map(|o| gy_card(&g, o.choice)).map(|c| gy.iter().position(|&x| x == c).unwrap()).collect();
        assert_eq!(offered, [0, 2, 4, 5], "{what}: the instants and sorceries ({})", g.options_desc().join(" | "));
        // The second Ponder: its own slot (graveyard cards are not deduplicated).
        let bolt = gy[4];
        g.choose(g.tgt(bolt));
        settle(&mut g, |_| None);
        assert_eq!(g.obj(bolt).zone, Zone::Hand, "{}", g.log_lines().join("\n"));
    }
}

/// Graveyard choices over both graveyards (ENGINE.md 65.10). Reanimate
/// ("put target creature card from a graveyard onto the battlefield under
/// your control"): the list is the offered cards, the caster's graveyard's
/// then the other's, each in order (the other's `list::THEIRS`); each target
/// points at its token, for each player as the caster. Exhume (each player
/// chooses a creature card in their own graveyard, the active player first):
/// each decider's list is their own graveyard whole, the non-active player
/// choosing during the active player's spell.
#[test]
fn graveyard_targets_and_choices_over_both_graveyards_use_the_list() {
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    let db = CardDb::cube();
    for p in 0..2u8 {
        let mut g = Scenario::new(db)
            .spec(|s| s.auto_single = false)
            .library(0, &["Swamp"; 10])
            .library(1, &["Swamp"; 10])
            .battlefield(p, &["Swamp"])
            .graveyard(p, &["Island", "Griselbrand", "Lightning Bolt"])
            .graveyard(1 - p, &["Archon of Cruelty", "Swamp", "Griselbrand"])
            .hand(p, &["Reanimate"])
            .at(Step::Main1, p)
            .start();
        let r = g.card(p, "Reanimate", Zone::Hand);
        g.choose(Choice::Cast(r));
        assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", g.options_desc().join(" | "));
        let what = format!("P{p}'s Reanimate");
        let list = check_graveyard_list(&g, &what);
        let mine = g.card(p, "Griselbrand", Zone::Graveyard);
        let theirs = g.card(1 - p, "Griselbrand", Zone::Graveyard);
        let archon = g.card(1 - p, "Archon of Cruelty", Zone::Graveyard);
        assert_eq!(list, [mine, archon, theirs], "{what}: the creature cards, the caster's first");
        let ids: Vec<u16> = g.pending_decision().unwrap().options.iter().map(|o| o.action).collect();
        assert_eq!(ids, [act::LIBRARY, act::LIBRARY + 1, act::LIBRARY + 2], "{what}: the creature cards' slots");
        g.choose(g.tgt(archon));
        to_priority(&mut g, |_| None);
        settle(&mut g, |_| None);
        assert_eq!((g.obj(archon).zone, g.obj(archon).controller), (Zone::Battlefield, p), "{}", g.log_lines().join("\n"));
    }
    // Exhume: P0 casts it; P0 then P1 choose from their own graveyards.
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Swamp"; 10])
        .library(1, &["Swamp"; 10])
        .battlefield(0, &["Swamp", "Swamp"])
        .graveyard(0, &["Island", "Griselbrand", "Lightning Bolt", "Archon of Cruelty"])
        .graveyard(1, &["Swamp", "Griselbrand", "Brainstorm", "Archon of Cruelty"])
        .hand(0, &["Exhume"])
        .at(Step::Main1, 0)
        .start();
    let e = g.card(0, "Exhume", Zone::Hand);
    g.choose(Choice::Cast(e));
    to_choice(&mut g, DecisionKind::ChooseGraveyardCard);
    let mut deciders = Vec::new();
    while g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
        let p = g.pending_decision().unwrap().player;
        let what = format!("Exhume, P{p}'s choice");
        assert_eq!(check_graveyard_list(&g, &what), g.player(p).graveyard, "{what}: the decider's own graveyard");
        let ids: Vec<u16> = g.pending_decision().unwrap().options.iter().map(|o| o.action).collect();
        assert_eq!(ids, [act::LIBRARY + 1, act::LIBRARY + 3], "{what}");
        deciders.push(p);
        let c = g.card(p, "Archon of Cruelty", Zone::Graveyard);
        g.choose(Choice::Card(c));
        to_choice_or_priority(&mut g, DecisionKind::ChooseGraveyardCard);
    }
    assert_eq!(deciders, [0, 1]);
}

/// Answer decisions (the first option) until one of kind `k` or priority.
fn to_choice_or_priority(g: &mut engine::Game, k: engine::DecisionKind) {
    for _ in 0..50 {
        let Some(d) = g.pending_decision() else { return };
        if d.kind == k || d.kind == engine::DecisionKind::Priority {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
    panic!("no {k:?} decision\n{}", g.log_lines().join("\n"));
}

/// Answer decisions (the first option) until one of kind `k`.
fn to_choice(g: &mut engine::Game, k: engine::DecisionKind) {
    for _ in 0..50 {
        let d = g.pending_decision().expect("a decision");
        if d.kind == k {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
    panic!("no {k:?} decision\n{}", g.log_lines().join("\n"));
}

/// Layout v5's facts hide nothing (ENGINE.md 60.4): on the gate's positions
/// (the cube, the Delver mirror, best-of-three, a drafted match, hidden decklists, EoE sealed) with each of
/// the 42 perturbations v5 encodes applied, every world `determinize` makes
/// for a viewer encodes the same for it; and a card exiled face
/// down that its owner may play (Headliner Scarlett) is in its owner's
/// playable-from-exile bag only, its count public.
#[test]
fn the_v5_facts_encode_no_hidden_information() {
    use encode::{bag, g5, O_BAGS, O_GLOBALS_V5};
    use engine::public_state::{perturbations, V5_FIXED, V6_FIXED, V7_FIXED};
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    let ps = perturbations();
    // (and layout v6's, ENGINE.md 100.5, and v7's, 124.2)
    let fixed: Vec<_> = ps.iter().filter(|p| V5_FIXED.contains(&p.name) || V6_FIXED.contains(&p.name) || V7_FIXED.contains(&p.name)).collect();
    assert_eq!(fixed.len(), V5_FIXED.len() + V6_FIXED.len() + V7_FIXED.len());
    // (A stream of its own for each perturbation, as the gate's: ENGINE.md
    // 100.5.1.)
    let fnv = |name: &str| name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let mut rngs: Vec<Rng> = fixed.iter().map(|p| Rng::new(60 ^ fnv(p.name))).collect();
    let mut zr = zero_search::rng::Rng::new(60);
    let (mut worlds, mut applied) = (0, 0);
    // (the deck tilt, ENGINE.md 106.3: on hidden-decklist positions one more world dealt by a random deck
    // prediction at tilt 1, from a stream of its own)
    let (mut trng, mut tilted) = (Rng::new(106), 0);
    for (spec, a) in gate_positions(scale(24) as usize, 61) {
        for (k, p) in fixed.iter().enumerate() {
            let mut b = a.clone();
            let Some(what) = (p.apply)(&mut b, &mut rngs[k]) else { continue };
            applied += 1;
            // (Worlds compared with each other: a change made by hand can
            // leave the other player's pending decision stale, and every world
            // for a viewer who doesn't decide recomputes it.)
            for v in 0..2 {
                let mut first = b.clone();
                first.determinize(v, &mut zr);
                let x = enc(&first, v);
                for _ in 0..2 {
                    let mut d = b.clone();
                    d.determinize(v, &mut zr);
                    let y = enc(&d, v);
                    if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                        panic!("{spec}: {} ({what}): P{v}'s feature {i} differs between worlds ({} / {})", p.name, x[i], y[i]);
                    }
                    worlds += 1;
                }
                if b.hidden_decklists() {
                    let mut d = b.clone();
                    let bel = zbelief_deck(&mut trng, 0.0, 1.0, 20.0);
                    d.determinize_with(v, &mut zero_search::rng::Rng::new(trng.next_u64()), &bel);
                    let y = enc(&d, v);
                    if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                        panic!("{spec}: {} ({what}): P{v}'s feature {i} differs in a deck-tilted world ({} / {})", p.name, x[i], y[i]);
                    }
                    tilted += 1;
                }
            }
        }
    }
    println!("{applied} perturbed positions, {worlds} worlds compared, {tilted} of them deck-tilted");
    assert!(applied > 1000 && worlds > 4000 && tilted > 500, "{applied} {worlds} {tilted}");
    // Headliner Scarlett: the top card of P0's library exiled face down, P0
    // may play it this turn.
    let mut g = Scenario::new(CardDb::cube())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Headliner Scarlett"])
        .library(0, &["Island", "Forest", "Lightning Bolt", "Mountain", "Swamp", "Plains", "Island"])
        .library(1, &["Island"; 10])
        .at(Step::Main2, 1)
        .start();
    for _ in 0..2000 {
        if g.turn().active == 0 && g.turn().step == Step::Main1 && g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    let fd: Vec<engine::CardIdx> =
        g.objs().iter().enumerate().filter(|(_, o)| o.zone == Zone::FaceDown).map(|(i, _)| engine::CardIdx::new(i as u32)).collect();
    assert_eq!(fd.len(), 1, "{}", g.log_lines().join("\n"));
    let c = fd[0];
    assert!(g.knows(0, c) && !g.knows(1, c));
    let id = ident(g.obj(c).def);
    let x0 = features(&g, 0);
    assert_eq!(x0[O_BAGS + bag::MY_EXILE_PLAY * encode::D + id], 0.25, "P0 may play the card it knows");
    assert_eq!(x0[O_GLOBALS_V5 + g5::EXILE_PLAY], 0.25);
    let x1 = features(&g, 1);
    let bag1 = &x1[O_BAGS + bag::OPP_EXILE_PLAY * encode::D..O_BAGS + (bag::OPP_EXILE_PLAY + 1) * encode::D];
    assert!(bag1.iter().all(|&f| f == 0.0), "P1 doesn't know the card: not in its bag");
    assert_eq!(x1[O_GLOBALS_V5 + g5::PLAYER + g5::EXILE_PLAY], 0.25, "P1 sees that P0 may play one");
    let mut resampled = false;
    for s in 0..40 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        assert_eq!(features(&d, 1), x1, "P1's world {s}");
        resampled |= d.obj(c).def != g.obj(c).def;
    }
    assert!(resampled, "the card was resampled for P1");
}

// ---- The belief target (ENGINE.md 66) ----

fn belief(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut t = vec![0.0; MtgAz::aux_width()];
    g.aux_target(viewer, &mut t);
    t.truncate(identity::D);
    t
}

/// The `deck` target (ENGINE.md 102.4): the second segment of `aux_target`.
fn deck_target(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut t = vec![0.0; MtgAz::aux_width()];
    g.aux_target(viewer, &mut t);
    t[identity::D..].to_vec()
}

/// The frame opponent's cards `viewer` does not know exactly, by identity (what the `deck` target
/// must be).
fn unseen_cards(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let ids = identity::identities(g.g.db());
    let o = 1 - g.g.encoding_seat(viewer as u8);
    let mut t = vec![0.0; identity::D];
    for c in g.g.unknown_to(viewer as u8, o) {
        t[ids[g.g.ident(c) as usize] as usize] += 1.0;
    }
    t
}

/// The opponent's actual hand of `viewer` by identity (what the target must be).
fn opponent_hand(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let ids = identity::identities(g.g.db());
    let mut t = vec![0.0; identity::D];
    for &c in &g.g.player(1 - viewer as u8).hand {
        t[ids[g.g.obj(c).def as usize] as usize] += 1.0;
    }
    t
}

/// The auxiliary target `belief` (zero-core docs/az.md 15) is the decider's opponent's TRUE hand
/// by card identity and never an input: in the openings and mid-game positions of the cube, best-of-
/// three, the Delver mirror and a drafted match, worlds determinized for the decider - whose
/// opponent hands differ - encode identically for it while their targets differ, each that world's
/// own hand (never the decider's); between the games of a match there is none.
#[test]
fn the_belief_target_is_the_true_opponent_hand_and_never_an_input() {
    assert_eq!(MtgAz::AUX, &[("belief", identity::D), ("deck", identity::D)]);
    let mut zr = zero_search::rng::Rng::new(66);
    let mut pos: Vec<(&'static str, MtgAz)> = gate_positions(scale(40) as usize, 66);
    for (name, sp) in opening_specs() {
        pos.extend(opening_states(&sp, 8, 66).into_iter().map(|g| (name, g)));
    }
    let (mut worlds, mut differ, mut nonempty) = (0, 0, 0);
    for (name, g) in &pos {
        let mover = g.to_move().unwrap();
        let t = belief(g, mover);
        assert_eq!(t, opponent_hand(g, mover), "{name}: the target is the opponent's hand");
        assert_eq!(t.iter().sum::<f32>() as usize, g.g.player(1 - mover as u8).hand.len());
        nonempty += (t.iter().sum::<f32>() > 0.0) as u32;
        let x = enc(g, mover);
        for _ in 0..3 {
            let mut d = g.clone();
            d.determinize(mover, &mut zr);
            let y = enc(&d, mover);
            if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                panic!("{name}: feature {i} of P{mover} changed in a world of another opponent hand");
            }
            let td = belief(&d, mover);
            assert_eq!(td, opponent_hand(&d, mover), "{name}: a world's target is its own opponent hand");
            differ += (td != t) as u32;
            worlds += 1;
        }
    }
    println!("{} positions, {worlds} worlds, {differ} with another opponent hand, {nonempty} non-empty targets", pos.len());
    // (the openings start before the deal: the play-or-draw choice)
    assert!(worlds > 600 && differ > worlds / 2 && nonempty as usize > pos.len() * 7 / 10, "{worlds} {differ} {nonempty}");
    // between the games of a best-of-three match: no target
    let sp = MtgSpec::parse(r#"{"format": "bo3"}"#).unwrap();
    let (mut rng, mut h, mut keys) = (Rng::new(67), Heuristic::new(67), Vec::new());
    let mut g = MtgAz::new_game(&sp, 2, 67).unwrap();
    while g.sideboarding().is_none() {
        let a = mixed(&g, &mut h, &mut rng);
        g.step(a, &mut keys).unwrap();
    }
    let p = g.to_move().unwrap();
    assert!(belief(&g, p).iter().all(|&v| v == 0.0) && belief(&g, 1 - p).iter().all(|&v| v == 0.0));
}

/// zero-core's driver with `aux` records the decider's opponent's hand with every sample of the
/// Delver mirror (the same records otherwise), and the team draft has no target.
#[test]
fn selfplay_records_the_belief_target_with_the_samples() {
    use zero_search::driver::{Driver, DriverCfg, KeyKind, NnSeat, SeatCfg};
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    let spec = serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string();
    let run = |aux: bool| {
        let mut s = NnSeat::eval(0, 4);
        s.selfplay = true;
        s.sims_fast = Some(2);
        s.p_full = 0.5;
        let cfg = DriverCfg { spec: spec.clone(), seats: vec![SeatCfg::Nn(s)], games: vec![vec![0, 0]; 2], keys: vec![("m".into(), KeyKind::Nn)],
                              concurrency: 2, seed: 5, record: true, threads: 2, groups: 1, max_steps: 100_000, rolling: false, net: 0,
                              queue_only: false, move_log: 0, aux, paired: false, sparse: false, branch: None, starts: None,
                              max_row_tokens: None };
        let mut d = Driver::<MtgAz>::new(cfg).unwrap();
        let mut recs = Vec::new();
        while let Some(b) = d.next_batch(true).unwrap() {
            let mut pri = Vec::new();
            for r in 0..b.b {
                let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
                pri.extend(std::iter::repeat_n(1.0 / k as f32, k));
            }
            let mut v = vec![0.0; b.b * 8];
            for r in 0..b.b {
                v[r * 8] = 0.5;
                v[r * 8 + 1] = 0.5;
            }
            d.submit(b.id, &pri, &v, &vec![0.0; b.b * 8]).unwrap();
            recs.extend(d.take_samples());
        }
        recs.extend(d.take_samples());
        recs
    };
    let (r0, r1) = (run(false), run(true));
    assert_eq!(r0.len(), r1.len());
    assert!(!r1.is_empty());
    for (a, b) in r0.iter().zip(&r1) {
        assert_eq!((&a.x, &a.pi, a.player), (&b.x, &b.pi, b.player));
        assert!(a.aux.is_empty() && b.aux.len() == 2 * identity::D);
        let n: f32 = b.aux[..identity::D].iter().sum();
        assert!((0.0..=12.0).contains(&n) && b.aux.iter().all(|&v| v >= 0.0 && v.fract() == 0.0));
        // (the deck target: at most the 60 cards of a Delver deck)
        assert!(b.aux[identity::D..].iter().sum::<f32>() <= 60.0);
    }
    assert!(r1.iter().filter(|r| r.aux[..identity::D].iter().sum::<f32>() > 0.0).count() > r1.len() / 2);
    assert!(engine::az::draft::TeamDraftAz::aux_width() == 0);
}

/// The auxiliary target `deck` (ENGINE.md 102.4) is the frame opponent's unseen cards - their
/// hand, library and face-down cards the decider does not know exactly - by identity, its total
/// public (the decider knows how many cards it knows), and never an input: on the gate's positions
/// (open decklists, best-of-three, a drafted match, hidden decklists, EoE sealed) every world of the decider's
/// encodes alike while the target is that world's own; with hidden decklists the worlds' targets
/// differ beyond the order of the cards (their decks differ), which the reader is to predict.
#[test]
fn the_deck_target_is_the_opponents_unseen_cards_and_never_an_input() {
    let mut zr = zero_search::rng::Rng::new(102);
    // (cube hidden decklists, EoE sealed: worlds and those with another unseen deck)
    let (mut worlds, mut other) = ([0u32; 2], [0u32; 2]);
    for (name, g) in gate_positions(scale(20) as usize, 1024) {
        let v = g.to_move().unwrap();
        let t = deck_target(&g, v);
        assert_eq!(t, unseen_cards(&g, v), "{name}: the deck target");
        if name == "eoe" {
            // in the EoE identity space (ENGINE.md 105): no unseen card is UNKNOWN
            assert_eq!(t[identity::UNKNOWN as usize], 0.0, "{name}: an unseen card without an EoE identity");
        }
        let o = 1 - g.g.encoding_seat(v as u8);
        assert_eq!(t.iter().sum::<f32>() as usize, g.g.unknown_to(v as u8, o).len(), "{name}: its total");
        let x = enc(&g, v);
        for _ in 0..3 {
            let mut d = g.clone();
            d.determinize(v, &mut zr);
            assert_eq!(enc(&d, v), x, "{name}: the decider's features");
            let td = deck_target(&d, v);
            assert_eq!(td, unseen_cards(&d, v), "{name}: a world's target is its own");
            assert_eq!(td.iter().sum::<f32>(), t.iter().sum::<f32>(), "{name}: the public total");
            if name == "hidden decklists" || name == "eoe" {
                let k = (name == "eoe") as usize;
                worlds[k] += 1;
                other[k] += (td != t) as u32;
            }
        }
    }
    println!("hidden decklists: {} worlds, {} with another unseen deck; EoE sealed: {} worlds, {} with another", worlds[0], other[0], worlds[1], other[1]);
    for k in 0..2 {
        assert!(worlds[k] >= 60 && other[k] * 10 > worlds[k] * 9, "{worlds:?} {other:?}");
    }
}

/// The deck reader's declared inputs (ENGINE.md 102.4) are checked in (`mtgz/az/deck_reader_v6.json`
/// = `encode::deck_reader_json`; `MTGZ_WRITE_LAYOUT=1` rewrites it) and are what zero-core's reader
/// accepts (its docs/az.md 36.2): ranges in feature order, disjoint, inside the features, none in a
/// token segment (the bags and the global blocks only); every viewer-side bag left out.
#[test]
fn deck_reader_inputs_are_checked_in_and_read_bags_and_globals_only() {
    // (Layout v7's for `mtg`, v6's for `mtg_eoe`: ENGINE.md 125.)
    for (file, lay, db) in [("deck_reader_v7.json", &encode::LAY7, CardDb::cube()), ("deck_reader_v6.json", &encode::LAY6, CardDb::eoe())] {
        let want = encode::deck_reader_json(lay);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az").join(file);
        if std::env::var("MTGZ_WRITE_LAYOUT").is_ok_and(|v| v == "1") {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(have == want, "{} differs from encode::deck_reader_json (MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test az deck_reader)", path.display());
        let ins = encode::deck_reader_inputs(lay);
        let l: serde_json::Value = serde_json::from_str(&encode::layout_json(db)).unwrap();
        let mut end = 0;
        for &(name, o, n) in &ins {
            assert!(n > 0 && o >= end && o + n <= lay.features, "{name}: [{o}, {n}) out of order or outside");
            end = o + n;
            for sg in l["segments"].as_array().unwrap() {
                let Some(so) = sg["offset"].as_u64() else { continue };
                let (so, sc, sw) = (so as usize, sg["count"].as_u64().unwrap() as usize, sg["width"].as_u64().unwrap() as usize);
                assert!(o + n <= so || o >= so + sc * sw, "{name}: inside the token segment {}", sg["name"]);
            }
        }
        let bag_of = |b: usize| encode::O_BAGS + b * encode::D;
        use encode::bag;
        for b in [bag::MY_HAND, bag::MY_BATTLEFIELD, bag::MY_GRAVEYARD, bag::MY_EXILE, bag::MY_LIBRARY, bag::OPP_HIDDEN, bag::OPP_POOL, bag::MY_HAND_KNOWN] {
            assert!(!ins.iter().any(|&(_, o, _)| o == bag_of(b)), "bag {b} is not the opponent's shown cards");
        }
    }
}

// ---- A non-deciding viewer's encoding (ENGINE.md 67) ------------------------

/// zero-core's root-view leaf values (its docs/search.md 3.6) evaluate a
/// position the other player decides from the root player's view. On the
/// public-state gate's positions (the cube, the Delver mirror, best-of-three,
/// a drafted match), the non-deciding player's encoding is the same in every
/// world that deals the decider's hidden cards again - including the cards a
/// payment in progress pins, which `determinize` keeps (`resample_unpinned`).
#[test]
fn a_non_deciding_viewers_encoding_ignores_the_deciders_hidden_cards() {
    let mut rng = Rng::new(67);
    let (mut worlds, mut changed) = (0, 0);
    let mut kinds = std::collections::BTreeMap::new();
    for (spec, g) in gate_positions(scale(40) as usize, 67) {
        let mover = g.to_move().unwrap();
        let viewer = 1 - mover;
        let x = enc(&g, viewer);
        *kinds.entry(spec).or_insert(0) += 1;
        for _ in 0..3 {
            let mut d = g.clone();
            d.g.resample_unpinned(viewer as u8, &mut rng);
            changed += g.g.objs().iter().zip(d.g.objs()).any(|(a, b)| a.def != b.def) as u32;
            let y = enc(&d, viewer);
            if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                panic!("{spec}: P{viewer}'s feature {i} reads the decider's hidden cards ({} -> {})", x[i], y[i]);
            }
            worlds += 1;
        }
    }
    println!("{worlds} worlds of {kinds:?} positions, {changed} with cards dealt again");
    assert!(worlds > 400 && changed > worlds / 2, "{worlds} {changed}");
}

/// The search's own case: worlds sampled for the root decider at its
/// decision, played on with the same actions while their chance keys agree;
/// at every position where the OTHER player decides, the root decider's
/// encoding is the same in both worlds even when that player's options
/// differ (they follow its sampled hand: different tree nodes, one
/// information state of the root decider) - a root-view leaf's value is a
/// function of what the root decider knows. (`equal_keys_mean_equal_features`
/// compares only while the options agree.)
#[test]
fn root_view_leaves_encode_alike_whatever_the_deciders_options() {
    let mut rng = Rng::new(68);
    let mut zr = zero_search::rng::Rng::new(68);
    let (mut compared, mut other_options) = (0, 0);
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    let mirror = serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string();
    let delver = MtgSpec::parse(&mirror).unwrap();
    let mut all = states(scale(14), 16, 68);
    let mut h = Heuristic::new(68);
    // (24 mirror games: 14 compared 293 positions once Priority's casts from a graveyard became
    // two decisions, ENGINE.md 100.5, which changed the random games.)
    for s in 0..scale(24) {
        let mut g = MtgAz::new_game(&delver, 2, 680_000 + s).unwrap();
        let mut keys = Vec::new();
        let mut k = 0;
        while g.to_move().is_some() && k < 400 {
            if k % 16 == 15 {
                all.push(g.clone());
            }
            let a = mixed(&g, &mut h, &mut rng);
            g.step(a, &mut keys).unwrap();
            k += 1;
        }
    }
    for g in all {
        let viewer = g.to_move().unwrap();
        let (mut a, mut b) = (g.clone(), g.clone());
        a.determinize(viewer, &mut zr);
        b.determinize(viewer, &mut zr);
        for _ in 0..30 {
            let (la, lb) = (legal(&a), legal(&b));
            if a.to_move() != b.to_move() {
                break;
            }
            if a.to_move().is_some_and(|p| p != viewer) {
                assert_eq!(enc(&a, viewer), enc(&b, viewer), "the root decider's encoding of the other player's decision differs");
                compared += 1;
                other_options += (la != lb) as u32;
            }
            if la != lb || a.to_move().is_none() {
                break;
            }
            let x = la[rng.below(la.len() as u32) as usize];
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            if ka != kb {
                break;
            }
        }
    }
    println!("compared {compared} positions of the other player, {other_options} with other options in the two worlds");
    assert!(compared > 300 && other_options > 10, "{compared} {other_options}");
}

// ---- Belief-weighted determinization (ENGINE.md 69) -------------------------

/// A random belief over the identities (a Dirichlet-like draw, sharpened by `sharp`).
fn random_belief(rng: &mut Rng, sharp: u32) -> Vec<f32> {
    let mut p: Vec<f32> = (0..identity::D).map(|_| (rng.below(1000) as f32 + 1.0).powi(sharp as i32)).collect();
    let s: f32 = p.iter().sum();
    p.iter_mut().for_each(|x| *x /= s);
    p
}

fn zbelief(pred: Vec<f32>, tilt: f64, clip: f64) -> zero_search::game::Belief {
    zero_search::game::Belief { pred, tilt, clip, tilts: Vec::new() }
}

/// Every card `owner` has in the game, by identity (sorted).
fn cards_of(g: &MtgAz, owner: u8) -> Vec<u16> {
    let mut v: Vec<u16> = g.g.objs().iter().enumerate().filter(|(_, o)| o.owner == owner).map(|(i, _)| g.g.ident(engine::CardIdx::new(i as u32))).collect();
    v.sort();
    v
}

/// `determinize_with` keeps every invariant `determinize` keeps, whatever the belief: on the
/// public-state gate's positions (the cube, the Delver mirror, best-of-three, a drafted match) and
/// random beliefs at tilts 0.5 / 1 / 8 (clip 20 or 1e6), for both viewers - the decider's actions,
/// each viewer's features, the cards it knows where they are, the game's invariants and clouds, every
/// player's cards (best-of-one: the same multiset; best-of-three: the sampled deck's), hand sizes.
#[test]
fn belief_deals_keep_every_determinize_invariant() {
    let mut rng = Rng::new(69);
    let (mut worlds, mut moved) = (0, 0);
    for (name, g) in gate_positions(scale(20) as usize, 69) {
        let mover = g.to_move().unwrap();
        for viewer in [mover, 1 - mover] {
            let x0 = enc(&g, viewer);
            for (tilt, clip) in [(0.5, 20.0), (1.0, 20.0), (8.0, 1e6)] {
                let b = zbelief(random_belief(&mut rng, 3), tilt, clip);
                let mut d = g.clone();
                let w = d.determinize_belief(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()), &b);
                assert!(w.is_some(), "{name}: a game in progress takes the belief");
                assert_eq!(d.to_move(), Some(mover), "{name}");
                if viewer == mover {
                    assert_eq!(legal(&g), legal(&d), "{name}: the decider's actions changed");
                }
                let x1 = enc(&d, viewer);
                if let Some(i) = (0..NUM_FEATURES).find(|&i| x0[i] != x1[i]) {
                    panic!("{name}: feature {i} of P{viewer} changed in a belief-dealt world ({} -> {})", x0[i], x1[i]);
                }
                for (i, o) in g.g.objs().iter().enumerate() {
                    let c = engine::CardIdx::new(i as u32);
                    if g.g.knows(viewer as u8, c) {
                        assert_eq!((o.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "{name}: a card P{viewer} knows was dealt again");
                    }
                }
                d.g.check_invariants().unwrap_or_else(|e| panic!("{name}: {e}"));
                d.g.check_knowledge().unwrap_or_else(|e| panic!("{name}: {e}"));
                for p in 0..2u8 {
                    assert_eq!(g.g.player(p).hand.len(), d.g.player(p).hand.len(), "{name}: hand size");
                    // (EoE sealed is a hidden-decklist game too: its spec's default, ENGINE.md 102.6, 105)
                    if name != "bo3" && name != "drafted" && name != "hidden decklists" && name != "eoe" {
                        assert_eq!(cards_of(&g, p), cards_of(&d, p), "{name}: P{p}'s cards");
                    }
                }
                moved += (g.g.player(1 - viewer as u8).hand.iter().any(|&c| g.g.ident(c) != d.g.ident(c))) as u32;
                worlds += 1;
            }
        }
    }
    println!("{worlds} belief-dealt worlds, {moved} with another opponent hand");
    assert!(worlds > 300 && moved > worlds / 3, "{worlds} {moved}");
}

/// The belief only steers the deal: at tilt 0 `determinize_with` is `determinize` bit for bit (the
/// same world, the same generators).
#[test]
fn tilt_zero_is_determinize() {
    for (name, g) in gate_positions(4, 70) {
        let v = g.to_move().unwrap();
        let (mut a, mut b) = (g.clone(), g.clone());
        a.determinize(v, &mut zero_search::rng::Rng::new(5));
        b.determinize_with(v, &mut zero_search::rng::Rng::new(5), &zbelief(vec![1.0 / identity::D as f32; identity::D], 0.0, 20.0));
        assert_eq!(a.g.fingerprint(), b.g.fingerprint(), "{name}");
        assert_eq!(enc(&a, 1 - v), enc(&b, 1 - v), "{name}");
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        let l = legal(&a);
        a.step(l[0], &mut ka).unwrap();
        b.step(l[0], &mut kb).unwrap();
        assert_eq!((ka, a.g.fingerprint()), (kb, b.g.fingerprint()), "{name}: the reseeded generators");
    }
}

/// The opponent's hand by identity in `g`, for `viewer`.
fn hand_counts(g: &MtgAz, viewer: usize) -> Vec<f32> {
    opponent_hand(g, viewer)
}

/// The weighting works (ENGINE.md 69.4): on Delver positions where the opponent's hand has unknown
/// cards, a belief whose ratio to the uniform deal is exactly 1 deals the uniform expected counts; a
/// belief on one identity of the hidden pool at a high tilt and clip deals as many of its copies as
/// fit (min(copies, unknown hand slots)) in nearly every world, one that rules it out deals almost
/// none - every constraint held (`belief_deals_keep_every_determinize_invariant` covers those).
#[test]
fn a_sharp_belief_concentrates_the_deal() {
    let mut checked = 0;
    for (name, g) in gate_positions(scale(30) as usize, 71).into_iter().filter(|(n, _)| *n == "delver") {
        let ids = identity::identities(g.g.db());
        let v = g.to_move().unwrap();
        let o = 1 - v as u8;
        let hand = &g.g.player(o).hand;
        let unknown: Vec<_> = hand.iter().filter(|&&c| !g.g.knows(v as u8, c)).copied().collect();
        let m = unknown.len();
        if m < 2 {
            continue;
        }
        // (A cloud over the hand - "one of these cards is Thundering Falls", after a known card went
        // back among hidden ones - takes the whole hand as its domain, and its free places are filled
        // from the uniform rest of the pool before the weights choose among them: the weighted deal
        // cannot concentrate there (`Game::resample_weighted`; ENGINE.md 86.5). Such positions are
        // left out: the positions are played partly at random, so float-mana's new options moved them
        // onto one.)
        if g.g.clouds().iter().any(|k| k.viewer == v as u8 && k.owner == o && k.hand) {
            continue;
        }
        let pool = g.g.unknown_to(v as u8, o);
        let mut h = vec![0u32; identity::D];
        for &c in &pool {
            h[ids[g.g.ident(c) as usize] as usize] += 1;
        }
        let mut known = vec![0f32; identity::D];
        for &c in hand.iter().filter(|&&c| g.g.knows(v as u8, c)) {
            known[ids[g.g.ident(c) as usize] as usize] += 1.0;
        }
        let n = hand.len() as f32;
        let big_h = pool.len() as f32;
        // the identity of the pool with the fewest copies (the hardest to concentrate on: least likely uniformly)
        let j = (0..identity::D).filter(|&k| h[k] > 0).min_by_key(|&k| (h[k], k)).unwrap();
        let fit = (h[j] as usize).min(m) as f32;
        let uni = m as f32 * h[j] as f32 / big_h;
        // exactly the uniform deal's expected counts: weights 1
        let p_uni: Vec<f32> = (0..identity::D).map(|k| (known[k] + m as f32 * h[k] as f32 / big_h) / n).collect();
        // on j / ruling j out
        let mut p_on = vec![1e-6f32; identity::D];
        p_on[j] = 1.0;
        let mut p_off = p_uni.clone();
        p_off[j] = 0.0;
        let reps = 300;
        let mut mean = [0f32; 3];
        for (k, p) in [p_uni, p_on, p_off].into_iter().enumerate() {
            let b = zbelief(p, if k == 0 { 1.0 } else { 8.0 }, if k == 0 { 20.0 } else { 1e6 });
            for s in 0..reps {
                let mut d = g.clone();
                d.determinize_with(v, &mut zero_search::rng::Rng::new(s * 31 + k as u64), &b);
                mean[k] += (hand_counts(&d, v)[j] - known[j]) / reps as f32;
            }
        }
        let se = (uni.max(0.05) / reps as f32).sqrt();
        assert!((mean[0] - uni).abs() < 5.0 * se + 0.02, "{name}: ratio-1 belief dealt {} of identity {j}, uniform {uni}", mean[0]);
        assert!(mean[1] > 0.9 * fit, "{name}: a sharp belief dealt {} of {fit} possible (uniform {uni})", mean[1]);
        assert!(mean[2] < 0.05 * uni.max(0.2) + 0.02, "{name}: a belief ruling it out dealt {} (uniform {uni})", mean[2]);
        checked += 1;
    }
    println!("{checked} Delver positions with >= 2 unknown hand cards");
    assert!(checked >= 5, "{checked} Delver positions with >= 2 unknown hand cards");
}

/// The belief's input and weights read only the decider's information (ENGINE.md 69.2): worlds
/// `determinize(decider)` deals - other opponent hands, other best-of-three decks - give the decider
/// the same features (so the head's prediction is the same) and, for one prediction, the same
/// weights: best-of-one bit for bit; best-of-three after the hidden pool is sampled with one seed,
/// hidden decklists (ENGINE.md 102) after the opponent's deck is (the deal samples it first, from
/// what the decider knows).
#[test]
fn the_belief_weights_ignore_the_opponents_hidden_cards() {
    let mut rng = Rng::new(72);
    let (mut worlds, mut differ) = (0, 0);
    for (name, g) in gate_positions(scale(24) as usize, 72) {
        let v = g.to_move().unwrap();
        let p = random_belief(&mut rng, 2);
        let x = enc(&g, v);
        let bo3 = g.m.format == engine::az::Format::Bo3;
        let weights = |w: &MtgAz| -> Vec<f64> {
            let mut w = w.clone();
            if bo3 {
                w.sample_hidden_pool_for(v as u8, &mut Rng::new(9));
            }
            if w.hidden_decklists() {
                w.sample_hidden_deck_for(v as u8, &mut Rng::new(9), None);
            }
            engine::az::belief::deal_weights(&w.g, v as u8, &p, 1.0, 20.0)
        };
        let w0 = weights(&g);
        for k in 0..3 {
            let mut d = g.clone();
            d.determinize(v, &mut zero_search::rng::Rng::new(rng.next_u64() ^ k));
            assert_eq!(enc(&d, v), x, "{name}: the decider's features");
            differ += (opponent_hand(&d, v) != opponent_hand(&g, v)) as u32;
            assert_eq!(weights(&d), w0, "{name}: the weights read the opponent's hidden cards");
            worlds += 1;
        }
    }
    println!("{worlds} worlds, {differ} with another opponent hand");
    assert!(worlds > 200 && differ > worlds / 3, "{worlds} {differ}");
}

/// Who knows a library's top cards (ENGINE.md 72), read back from
/// `g5::TOP_OWNER` / `TOP_OTHER` for both viewers: P0 Brainstorms (it knows
/// the two cards it put back); P1's Mishra's Bauble looks at P0's top card,
/// which its owner knows (P1 remembers it, ENGINE.md 23.7); P0 Ponders and
/// keeps (P1 loses track of the card it saw, P0 knows the two left on top
/// after the draw); a fetch land's shuffle forgets them all. Every world
/// either viewer's `determinize` deals encodes the same for it.
#[test]
fn who_knows_a_librarys_top_cards_is_encoded() {
    use encode::{g5, O_GLOBALS_V5};
    use engine::db::AbilitySlot;
    use engine::testkit::Scenario;
    use engine::{Choice, Step, Target, Zone};
    // `owner`'s library as `v` encodes it: (its owner knows, the other
    // player knows) per position, top first.
    let tops = |g: &engine::Game, v: u8, owner: u8| -> [(f32, f32); 3] {
        let x = features(g, v);
        let at = O_GLOBALS_V5 + g5::PLAYER * (owner != v) as usize;
        std::array::from_fn(|k| (x[at + g5::TOP_OWNER + k], x[at + g5::TOP_OTHER + k]))
    };
    let check = |g: &engine::Game, what: &str, p0: [(f32, f32); 3]| {
        for v in 0..2u8 {
            assert_eq!(tops(g, v, 0), p0, "{what}: P0's library as P{v} encodes it\n{}", g.log_lines().join("\n"));
            assert_eq!(tops(g, v, 1), [(0.0, 0.0); 3], "{what}: P1's library as P{v} encodes it");
            let x = features(g, v);
            for s in 0..20 {
                let mut d = g.clone();
                d.determinize(v, &mut Rng::new(s));
                assert_eq!(features(&d, v), x, "{what}: P{v}'s world {s}");
            }
        }
    };
    let mut g = Scenario::new(CardDb::constructed())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Polluted Delta"])
        .hand(0, &["Brainstorm", "Ponder", "Mountain", "Lightning Bolt"])
        .library(0, &["Forest", "Plains", "Swamp", "Island", "Counterspell", "Forest", "Plains", "Swamp", "Forest", "Plains"])
        .battlefield(1, &["Mishra's Bauble"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let (x, y) = ((1.0, 0.0), (1.0, 1.0));
    let no = (0.0, 0.0);
    check(&g, "the start", [no; 3]);
    // Brainstorm, the Mountain and the Bolt put back.
    let (mountain, bolt) = (g.card(0, "Mountain", Zone::Hand), g.card(0, "Lightning Bolt", Zone::Hand));
    g.choose(Choice::Cast(g.card(0, "Brainstorm", Zone::Hand)));
    settle(&mut g, |g| [Choice::Card(mountain), Choice::Card(bolt)].into_iter().find(|&c| g.has_option(c)));
    check(&g, "Brainstorm", [x, x, no]);
    // P1's Mishra's Bauble at P0's top card (P0 knows it).
    assert_eq!(g.turn().priority, 0);
    g.pass();
    assert_eq!(g.turn().priority, 1);
    g.choose(Choice::Ability(g.card(1, "Mishra's Bauble", Zone::Battlefield), AbilitySlot::Activated(0)));
    settle(&mut g, |_| Some(Choice::Target(Target::Player(0))));
    let top = *g.player(0).library.last().unwrap();
    assert!(g.knows(1, top), "P1 saw a card its owner knows: remembered");
    check(&g, "the Bauble", [y, x, no]);
    // Ponder, kept: three seen, one drawn; P1's card is somewhere in P0's
    // hand or the top two now (a cloud), not known there.
    assert_eq!(g.turn().priority, 0);
    g.choose(Choice::Cast(g.card(0, "Ponder", Zone::Hand)));
    settle(&mut g, |g| (g.kind() == Some(engine::DecisionKind::YesNo)).then_some(Choice::Pass));
    check(&g, "Ponder", [x, x, no]);
    // Polluted Delta: the search shuffles.
    g.choose(Choice::Ability(g.card(0, "Polluted Delta", Zone::Battlefield), AbilitySlot::Activated(0)));
    settle(&mut g, |_| None);
    g.card(0, "Polluted Delta", Zone::Graveyard);
    check(&g, "the fetch land's shuffle", [no; 3]);
}

/// A belief of both targets (ENGINE.md 106): random hand and deck predictions, the hand's tilt `hand`
/// (`belief_tilt`) and the deck's `deck` (`belief_tilts.deck`).
fn zbelief_deck(rng: &mut Rng, hand: f64, deck: f64, clip: f64) -> zero_search::game::Belief {
    let mut pred = random_belief(rng, 3);
    pred.extend(random_belief(rng, 3));
    zero_search::game::Belief { pred, tilt: hand, clip, tilts: vec![0.0, deck] }
}

/// The deck tilt keeps every `determinize` invariant (ENGINE.md 106.3): the gate's hidden-decklist
/// positions (the cube's random decks and EoE sealed; an open-decklist game refuses it), random
/// predictions of both targets at deck tilts 0.5 / 1 / 8 (clips 20 / 1e6), with and without the hand's
/// tilt, both viewers: the decider's actions, each viewer's whole encoding, the cards it knows, the
/// invariants and knowledge, hand sizes; in the open-decklist kinds every player's cards.
#[test]
fn deck_tilt_deals_keep_every_determinize_invariant() {
    let mut rng = Rng::new(106);
    let (mut worlds, mut decks) = (0, 0);
    for (name, g) in gate_positions(scale(40) as usize, 106) {
        if !g.hidden_decklists() {
            continue;
        }
        let mover = g.to_move().unwrap();
        for viewer in [mover, 1 - mover] {
            let x0 = enc(&g, viewer);
            for (hand, deck, clip) in [(0.0, 0.5, 20.0), (1.0, 1.0, 20.0), (0.0, 8.0, 1e6)] {
                let b = zbelief_deck(&mut rng, hand, deck, clip);
                let mut d = g.clone();
                d.determinize_with(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()), &b);
                assert_eq!(d.to_move(), Some(mover), "{name}");
                if viewer == mover {
                    assert_eq!(legal(&g), legal(&d), "{name}: the decider's actions changed");
                }
                let x1 = enc(&d, viewer);
                if let Some(i) = (0..NUM_FEATURES).find(|&i| x0[i] != x1[i]) {
                    panic!("{name}: feature {i} of P{viewer} changed in a deck-tilted world ({} -> {})", x0[i], x1[i]);
                }
                for (i, o) in g.g.objs().iter().enumerate() {
                    let c = engine::CardIdx::new(i as u32);
                    if g.g.knows(viewer as u8, c) {
                        assert_eq!((o.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "{name}: a card P{viewer} knows was dealt again");
                    }
                }
                d.g.check_invariants().unwrap_or_else(|e| panic!("{name}: {e}"));
                d.g.check_knowledge().unwrap_or_else(|e| panic!("{name}: {e}"));
                for p in 0..2u8 {
                    assert_eq!(g.g.player(p).hand.len(), d.g.player(p).hand.len(), "{name}: hand size");
                }
                assert_eq!(cards_of(&g, viewer as u8), cards_of(&d, viewer as u8), "{name}: the viewer's own cards");
                decks += (cards_of(&g, 1 - viewer as u8) != cards_of(&d, 1 - viewer as u8)) as u32;
                worlds += 1;
            }
        }
    }
    println!("{worlds} deck-tilted worlds, {decks} with another opponent deck");
    assert!(worlds > 300 && decks * 10 > worlds * 9, "{worlds} {decks}");
}

/// A deck tilt on an open-decklist game is refused loudly (ENGINE.md 106.2: it would deal nothing
/// differently), never a silent no-op.
#[test]
#[should_panic(expected = "the deck tilt deals hidden decklists")]
fn a_deck_tilt_on_open_decklists_is_refused() {
    let (name, g) = gate_positions(4, 108).into_iter().find(|(_, g)| !g.hidden_decklists() && g.m.sb.is_none()).unwrap();
    let mut d = g.clone();
    let b = zbelief_deck(&mut Rng::new(1), 0.0, 1.0, 20.0);
    d.determinize_with(g.to_move().unwrap(), &mut zero_search::rng::Rng::new(1), &b);
    println!("{name}: dealt");
}

/// The deck tilt reads only the viewer's information (ENGINE.md 106.2): on the gate's hidden-decklist
/// positions, worlds the decider cannot tell from the real game (its features identical) weigh the
/// cached decks the same, bit for bit, with a cold memo - the K decks, the weights, the probabilities -
/// and deal the same opponent deck from the same seed.
#[test]
fn the_deck_tilt_ignores_the_opponents_hidden_cards() {
    use engine::az::hidden_decks::{DealCache, DeckTilt};
    let mut rng = Rng::new(107);
    let (mut worlds, mut differ) = (0, 0);
    for (name, g) in gate_positions(scale(24) as usize, 107) {
        if !g.hidden_decklists() {
            continue;
        }
        let v = g.to_move().unwrap();
        let p = random_belief(&mut rng, 2);
        let t = DeckTilt { p: &p, tilt: 1.0, clip: 20.0 };
        let x = enc(&g, v);
        let cold = |w: &MtgAz| {
            let mut w = w.clone();
            w.m.deal = DealCache::new(w.m.deal.k, w.m.deal.salt);
            w.deck_tilt_of(v as u8, t).map(|(decks, td)| (decks.iter().map(|d| d.deck.clone()).collect::<Vec<_>>(), (*td).clone()))
        };
        let dealt = |w: &MtgAz, s: u64| {
            let mut w = w.clone();
            w.m.deal = DealCache::new(w.m.deal.k, w.m.deal.salt);
            let mut pred = random_belief(&mut Rng::new(1), 1);
            pred.extend_from_slice(&p);
            w.determinize_with(v, &mut zero_search::rng::Rng::new(s), &zero_search::game::Belief { pred, tilt: 0.0, clip: 20.0, tilts: vec![0.0, 1.0] });
            cards_of(&w, 1 - v as u8)
        };
        let (t0, d0) = (cold(&g), dealt(&g, 11));
        for k in 0..3 {
            let mut d = g.clone();
            d.determinize(v, &mut zero_search::rng::Rng::new(rng.next_u64() ^ k));
            assert_eq!(enc(&d, v), x, "{name}: the decider's features");
            differ += (cards_of(&d, 1 - v as u8) != cards_of(&g, 1 - v as u8)) as u32;
            assert!(cold(&d) == t0, "{name}: the deck tilt read the opponent's hidden cards");
            assert_eq!(dealt(&d, 11), d0, "{name}: the deck-tilted deal read the opponent's hidden cards");
            worlds += 1;
        }
    }
    println!("{worlds} worlds, {differ} with another opponent deck");
    assert!(worlds >= 60 && differ > worlds / 2, "{worlds} {differ}");
}
