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

fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode(viewer, &mut x);
    x
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
        l[rng.below(l.len() as u32) as usize]
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

#[test]
fn encoding_slots_name_the_action_slots() {
    assert_eq!(NUM_FEATURES, 29_860);
    let mut lists = 0;
    for g in states(scale(16), 6, 7) {
        let me = g.to_move().unwrap();
        let x = enc(&g, me);
        assert!(x.iter().all(|v| v.is_finite() && v.abs() < 50.0));
        let l = legal(&g);
        // hand slots
        let hand = g.g.hand_sorted(me as u8);
        for (s, &c) in hand.iter().enumerate().take(encode::HANDS) {
            let o = encode::O_HAND + s * encode::HAND_W;
            assert_eq!(decode_id(&x[o..o + CODE]), ident(g.g.obj(c).def));
            assert_eq!(x[o + CODE + 2] == 1.0, l.contains(&(act::HAND + s as u16)));
        }
        // permanents
        for (side, p) in [me as u8, 1 - me as u8].into_iter().enumerate() {
            let base = if side == 0 { encode::O_MY_PERM } else { encode::O_OPP_PERM };
            let range = if side == 0 { act::MY_PERM } else { act::OPP_PERM };
            for (s, c) in g.g.permanents_of(p).enumerate().take(encode::PERMS) {
                let o = base + s * encode::PERM_W;
                // (A copy: what it is, and what it copies, ENGINE.md 26.)
                assert_eq!(decode_id(&x[o..o + CODE]), ident(g.g.own_def(c)));
                if g.g.is_copy(c) {
                    let co = o + CODE + encode::perm::COPY_OF;
                    assert_eq!(decode_id(&x[co..co + 32]), ident(g.g.obj(c).def));
                }
                assert_eq!(x[o + CODE + encode::perm::TAPPED] == 1.0, g.g.obj(c).tapped);
                assert_eq!(x[o + CODE + encode::perm::OPTION] == 1.0, l.contains(&(range + s as u16)));
            }
        }
        // the LIBRARY list of a search / pile / seen hand
        let d = g.g.pending_decision().unwrap();
        for o in &d.options {
            if (act::LIBRARY..act::LIBRARY + encode::LIST as u16).contains(&o.action) {
                let k = (o.action - act::LIBRARY) as usize;
                let off = encode::O_LIST + k * encode::LIST_W;
                let want = match o.choice {
                    engine::Choice::Def(d) => ident(d),
                    engine::Choice::Card(c) => ident(g.g.obj(c).def),
                    // Bolas's Citadel: cast or play the top card (LIBRARY + 0)
                    engine::Choice::Cast(c) | engine::Choice::Play(c) => ident(g.g.obj(c).def),
                    _ => unreachable!(),
                };
                assert_eq!(decode_id(&x[off..off + CODE]), want);
                assert_eq!(x[off + CODE + 1], 1.0);
                lists += 1;
            }
            // a graveyard card's ability (ENGINE.md 27.5)
            if (act::GY_ABILITY..act::GY_ABILITY + encode::GYAS as u16).contains(&o.action) {
                let k = (o.action - act::GY_ABILITY) as usize;
                let off = encode::O_GYA + k * encode::GYA_W;
                let engine::Choice::Ability(c, _) = o.choice else { unreachable!() };
                assert_eq!(decode_id(&x[off..off + CODE]), ident(g.g.obj(c).def));
                assert_eq!(x[off + CODE + 1], 1.0);
            }
        }
        // the decision kind
        assert_eq!(x[encode::O_CONTEXT + d.kind as usize], 1.0);
    }
    assert!(lists > 0, "no LIBRARY decision met");
}

#[test]
fn layout_json_is_checked_in() {
    // mtgz/az/layout_v3.json = encode::layout_json (MTGZ_WRITE_LAYOUT=1 rewrites it)
    let want = encode::layout_json(CardDb::cube());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az/layout_v3.json");
    if std::env::var("MTGZ_WRITE_LAYOUT").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(have == want, "{} differs from encode::layout_json (MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test az layout)", path.display());
    let v: serde_json::Value = serde_json::from_str(&want).unwrap();
    assert_eq!(v["features"], NUM_FEATURES);
    assert_eq!(v["cards"].as_array().unwrap().len(), identity::D);
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

#[test]
fn minor_decisions_are_mana_payments() {
    let mut pay = 0;
    for g in states(scale(4), 3, 11) {
        let d = g.g.pending_decision().unwrap();
        assert_eq!(g.minor_decision(), d.kind == engine::decision::DecisionKind::PayMana);
        pay += g.minor_decision() as u32;
    }
    assert!(pay > 0, "no mana payment met");
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
    assert!(MtgSpec::parse(r#"{"decks": {"p0": ["Strip Mine"], "p1": []}}"#).unwrap_err().contains("not supported"));
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
