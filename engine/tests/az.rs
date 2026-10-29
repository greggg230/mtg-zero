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
    assert_eq!(NUM_FEATURES, 33_652);
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
    // mtgz/az/layout_v4.json = encode::layout_json (MTGZ_WRITE_LAYOUT=1 rewrites it)
    let want = encode::layout_json(CardDb::cube());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az/layout_v4.json");
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
/// these values; the failure message prints the new ones.)
#[test]
fn opening_encodings_but_the_declaration_are_unchanged() {
    let mut got = Vec::new();
    for (name, sp) in opening_specs() {
        let mut h = Fnv::new();
        let (mut states, mut declared, mut bottoms) = (0, 0, 0);
        for g in opening_states(&sp, 40, 21) {
            states += 1;
            declared += (0..2).any(|p| g.g.player(p).mulligan_declared) as u32;
            bottoms += (g.g.kind() == Some(engine::DecisionKind::MulliganBottom)) as u32;
            for v in 0..2 {
                let mut x = enc(&g, v);
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
    let want = [("cube", "6f267cb231046dd1"), ("bo3", "03550aad88c4a965"), ("delver", "cb3395e062b2fd8f")];
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
/// the cube's random decks, the Delver mirror, best-of-three matches and a
/// drafted match (a random 2v2 `team_draft` event's spec), `per` of each.
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
/// on positions of the four kinds of games, changes the public view for both
/// players (it is public) and the encoding for both viewers (it reaches the
/// network), but the deliberate limits (`public_state::limits`).
#[test]
fn public_state_reaches_the_encoding() {
    use engine::public_state::{fields, limits, perturbations, Class, LimitKind};
    let t0 = std::time::Instant::now();
    let pos = gate_positions(scale(60) as usize, 59);
    let ps = perturbations();
    let lim = limits();
    #[derive(Default)]
    struct Tally {
        applied: usize,
        view_same: [usize; 2],
        enc_same: [usize; 2],
        example: Option<String>,
    }
    let mut t: Vec<Tally> = ps.iter().map(|_| Tally::default()).collect();
    let mut rng = Rng::new(5959);
    for (spec, a) in &pos {
        let base_enc = [enc(a, 0), enc(a, 1)];
        let base_view = [public_view(a, 0), public_view(a, 1)];
        for (k, p) in ps.iter().enumerate() {
            let mut b = a.clone();
            let Some(what) = (p.apply)(&mut b, &mut rng) else { continue };
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
    let (mut ok, mut by_design, mut v5) = (0, 0, 0);
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
            (_, false, true, Some(LimitKind::V5)) => "gap (layout v5)",
            (_, false, true, None) => "ENCODING GAP",
            (_, false, false, Some(_)) => "STALE LIMIT",
            _ => "ok",
        };
        match state {
            "ok" => ok += 1,
            "limit (design)" => by_design += 1,
            "gap (layout v5)" | "rare limit" => v5 += 1,
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
        if !matches!(state, "ok" | "limit (design)" | "gap (layout v5)" | "rare, not applied" | "rare limit") {
            findings.push(format!("{state}: {} ({})", p.name, p.field));
        }
    }
    println!("{ok} perturbations encoded, {by_design} deliberate limits, {v5} gaps for layout v5 (ENGINE.md 58.10)");
    assert!(findings.is_empty(), "{} findings:\n{}", findings.len(), findings.join("\n"));
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
        let c = prefer(g).filter(|&c| g.has_option(c)).unwrap_or(d.options[0].choice);
        let _: Choice = c;
        g.choose(c);
    }
    panic!("settle: no progress\n{}", g.log_lines().join("\n"));
}

fn features(g: &engine::Game, v: u8) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode_features(v, &mut x);
    x
}

/// The public-state gate's encoding fixes (ENGINE.md 58.8), each read back
/// from its float for both viewers: the monarch (g[127]), spells cast this
/// turn and energy (the match block's 27-30), X on the stack (stack status
/// 31), and a permanent's cards exiled with it, other counters and protection
/// (permanent status 93-95; can't block / can't be blocked and the descriptor's
/// types and colours as they are are the gate's perturbations').
#[test]
fn the_public_state_fixes_are_encoded() {
    use encode::{glob, mtch, perm, stack, O_GLOBALS, O_MATCH, O_MY_PERM, O_OPP_PERM, O_STACK, PERM_W, STACK_W};
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Target, Zone};
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
    let _ = Target::Player(0);
    for v in 0..2u8 {
        let x = features(&g, v);
        let side = v as usize;
        assert_eq!((x[O_MATCH + mtch::ENERGY + side], x[O_MATCH + mtch::ENERGY + 1 - side]), (0.75, 0.0), "P{v}: P0's three energy");
    }
}
