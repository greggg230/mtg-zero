//! The sealed deck choice (ENGINE.md 107.2): the spec key, the candidates (the best guess and the randomised
//! builds), the explore share, and the pre-game decision through `MtgAz` - a seat's choice is its own (the
//! other seat's encoding and worlds never show it), `determinize` deals the other seat's deck from its prior
//! (in the decider's world the other seat has chosen), the game starts with the chosen decks, and the game log
//! records them.

use engine::az::eoe::MtgEoeAz;
use engine::az::sealed::{self, builder, collation::Collation, SealedSpec};
use engine::az::{encode, MtgAz, MtgSpec};
use engine::decision::{act, DecisionKind};
use engine::rng::Rng;
use zero_search::game::SearchGame;

fn spec(k: u8, explore: f64, extra: &str) -> MtgSpec {
    MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "paper", "deck_choice": {{"k": {k}, "explore": {explore}}}{extra}}}, "max_turns": 30}}"#)).unwrap()
}

fn enc(a: &MtgAz, v: usize) -> Vec<f32> {
    let mut x = vec![0.0; encode::NUM_FEATURES];
    a.encode(v, &mut x);
    x
}

fn legal(a: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    a.legal_actions(&mut v);
    v
}

/// The cards each player starts the game with (library, hand: the whole deck before any play), sorted.
fn game_deck(a: &MtgAz, p: u8) -> Vec<u16> {
    let g = &a.g;
    let mut v: Vec<u16> = g.player(p).library.iter().chain(g.player(p).hand.iter()).map(|&c| g.obj(c).def).collect();
    v.sort_unstable();
    v
}

#[test]
fn the_deck_choice_key_parses_and_refuses() {
    let s = spec(8, 0.25, "");
    let dc = s.sealed.as_deref().unwrap().deck_choice.unwrap();
    assert_eq!((dc.k, dc.explore.0), (8, 0.25));
    let sl = s.sealed.as_deref().unwrap();
    assert_eq!(SealedSpec::parse(&sl.to_json()).unwrap(), *sl, "the JSON round trip");
    let d = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "deck_choice": {}}}"#).unwrap();
    let dc = d.sealed.as_deref().unwrap().deck_choice.unwrap();
    assert_eq!((dc.k, dc.explore.0), (32, 0.1), "the defaults");
    for bad in [r#"{"k": 1}"#, r#"{"k": 33}"#, r#"{"explore": 1.5}"#, r#"{"temperature": 1}"#, "3"] {
        let e = MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "arena", "deck_choice": {bad}}}}}"#)).unwrap_err();
        assert!(e.contains("deck_choice"), "{bad}: {e}");
    }
    let e = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "deck_choice": {}}, "hidden_decklists": false}"#).unwrap_err();
    assert!(e.contains("hidden decklists"), "{e}");
}

#[test]
fn the_candidates_are_the_best_guess_and_randomised_builds() {
    let db = sealed::db();
    let s = spec(8, 0.0, "");
    let sl = s.sealed.as_deref().unwrap();
    for seed in 0..20u64 {
        for p in 0..2u8 {
            let (cands, ps) = sl.candidates(seed, p).unwrap().unwrap();
            let (pool, ps2) = sl.pool_of(seed, p).unwrap().unwrap();
            assert_eq!((cands.len(), ps), (8, ps2));
            assert_eq!(cands[0], builder::best_guess(db, &pool, &sl.builder), "candidate 0: the best guess");
            for (j, c) in cands.iter().enumerate().skip(1) {
                assert_eq!(*c, builder::build(db, &pool, &sl.builder, &mut sealed::candidate_rng(seed, p, j as u8)));
            }
            assert_eq!(sl.candidates(seed, p).unwrap().unwrap().0, cands, "seeded");
        }
    }
    // a given deck has none; a spec without the key none
    let given: Vec<String> = sl.candidates(3, 0).unwrap().unwrap().0[0].deck.iter().map(|&d| db.def(d).name.clone()).collect();
    let g = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "paper", "deck_choice": {"k": 4}, "p1": {"deck": given}})).unwrap();
    assert!(g.candidates(0, 1).unwrap().is_none() && g.candidates(0, 0).unwrap().is_some());
    assert!(SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "paper"})).unwrap().candidates(0, 0).unwrap().is_none());
}

#[test]
fn the_explore_share_plays_a_random_build() {
    let (mut n, mut picked) = (0u32, [0u32; 8]);
    for seed in 0..4000u64 {
        for p in 0..2u8 {
            if let Some(j) = sealed::explore_pick(seed, p, 0.1, 8) {
                assert!((1..8).contains(&j));
                picked[j as usize] += 1;
                n += 1;
            }
            assert_eq!(sealed::explore_pick(seed, p, 0.0, 8), None);
            assert!(sealed::explore_pick(seed, p, 1.0, 8).is_some());
        }
    }
    println!("explore 0.1: {n} of 8,000 seats; picks {:?}", &picked[1..]);
    assert!((680..=920).contains(&n), "{n}");
    assert!(picked[1..].iter().all(|&c| c > 60), "{picked:?}");
}

#[test]
fn the_deck_choice_is_a_hidden_pre_game_decision() {
    let db = sealed::db();
    let s = spec(6, 0.0, "");
    let sl = s.sealed.as_deref().unwrap();
    let mut zr = zero_search::rng::Rng::new(1072);
    let mut rng = Rng::new(1072);
    let mut keys = Vec::new();
    let (mut worlds, mut other) = (0u32, 0u32);
    for seed in 0..10u64 {
        let ss = s.sealed_seed(seed);
        let cands = [sl.candidates(ss, 0).unwrap().unwrap().0, sl.candidates(ss, 1).unwrap().unwrap().0];
        let a = MtgAz::new_game(&s, 2, seed).unwrap();
        // seat 0 decides first: the value choice among its candidates
        assert_eq!(a.to_move(), Some(0));
        assert!(a.value_choice() && !a.minor_decision());
        assert_eq!(a.pending_decision().unwrap().kind, DecisionKind::ChooseDeck);
        assert_eq!(legal(&a), (0..6).map(|j| act::NUMBER + j).collect::<Vec<_>>());
        assert_eq!(a.m.how, [None, None]);
        // the worlds of the decider: its decision and view kept, the other seat chosen (dealt from its prior)
        for _ in 0..3 {
            let mut w = a.clone();
            w.determinize(0, &mut zr);
            assert_eq!((w.to_move(), legal(&w)), (Some(0), legal(&a)));
            assert_eq!(enc(&w, 0), enc(&a, 0), "seed {seed}: the decider's encoding in its world");
            let dealt = w.m.deck(1);
            assert_eq!(dealt.len(), 40);
            worlds += 1;
            other += cands[1].iter().all(|c| c.deck != dealt) as u32;
            // one step: the game begins with the decider's choice and the dealt deck
            w.step(act::NUMBER + 2, &mut keys).unwrap();
            assert!(!w.value_choice() && w.deck_choice().is_none());
            assert_eq!(game_deck(&w, 0), cands[0][2].deck);
            assert_eq!(game_deck(&w, 1), dealt);
        }
        // the other viewer's world: the decider's candidates all its dealt deck (unknown to the viewer)
        let mut w = a.clone();
        w.determinize(1, &mut zr);
        assert_eq!((w.to_move(), legal(&w)), (Some(0), legal(&a)));
        assert_eq!(enc(&w, 1), enc(&a, 1));
        let d0 = w.m.deck(0);
        assert!(w.deck_choice().unwrap().cands[0].as_ref().unwrap().iter().all(|c| c.deck == d0));
        // seat 0 chooses j: hidden from seat 1 (its encoding the same whatever j)
        let j = rng.below(6) as u16;
        let mut b = a.clone();
        b.step(act::NUMBER + j, &mut keys).unwrap();
        let mut b2 = a.clone();
        b2.step(act::NUMBER + (j + 1) % 6, &mut keys).unwrap();
        assert_eq!(b.to_move(), Some(1));
        assert_eq!(enc(&b, 1), enc(&b2, 1), "seed {seed}: seat 0's choice reaches seat 1's encoding");
        assert_eq!(b.m.deck(0), cands[0][j as usize].deck);
        let h = b.m.how[0].unwrap();
        assert_eq!((h.mode, h.candidate), (sealed::Mode::Chosen, Some(j as u8)));
        // seat 1's worlds: seat 0's choice dealt from its prior
        let mut w = b.clone();
        w.determinize(1, &mut zr);
        assert_eq!((w.to_move(), legal(&w), enc(&w, 1)), (Some(1), legal(&b), enc(&b, 1)));
        assert_eq!(w.m.how[0], None);
        other += (w.m.deck(0) != b.m.deck(0)) as u32;
        worlds += 1;
        // seat 1 chooses: the game begins with both chosen decks, its seed the match's
        let k = rng.below(6) as u16;
        b.step(act::NUMBER + k, &mut keys).unwrap();
        assert!(!b.value_choice() && b.deck_choice().is_none());
        assert_eq!(game_deck(&b, 0), cands[0][j as usize].deck);
        assert_eq!(game_deck(&b, 1), cands[1][k as usize].deck);
        let mut again = a.clone();
        again.step(act::NUMBER + j, &mut keys).unwrap();
        again.step(act::NUMBER + k, &mut keys).unwrap();
        assert_eq!(enc(&again, 0), enc(&b, 0), "the same choices, the same game");
        // played to the end
        let mut steps = 0;
        while b.to_move().is_some() {
            let l = legal(&b);
            b.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            steps += 1;
            assert!(steps < 100_000);
        }
        let _ = db;
    }
    println!("{worlds} deck-choice worlds, {other} with another deck for the other seat");
    assert!(other * 10 >= worlds * 9, "{other} of {worlds}");
}

#[test]
fn explore_seats_and_given_decks_do_not_choose() {
    let db = sealed::db();
    // both explore: no choice, the game begins at once with the picks
    let s = spec(5, 1.0, "");
    let sl = s.sealed.as_deref().unwrap();
    for seed in 0..6u64 {
        let a = MtgAz::new_game(&s, 2, seed).unwrap();
        assert!(a.deck_choice().is_none() && !a.value_choice());
        for p in 0..2u8 {
            let j = sealed::explore_pick(seed, p, 1.0, 5).unwrap();
            let c = &sl.candidates(seed, p).unwrap().unwrap().0[j as usize];
            assert_eq!(game_deck(&a, p), c.deck);
            let h = a.m.how[p as usize].unwrap();
            assert_eq!((h.mode, h.candidate), (sealed::Mode::Explore, Some(j)));
        }
    }
    // a given deck in seat 1: seat 0 alone chooses, then the game begins
    let given: Vec<String> = sl.candidates(3, 1).unwrap().unwrap().0[0].deck.iter().map(|&d| db.def(d).name.clone()).collect();
    let g = MtgSpec::parse(&serde_json::json!({"sealed": {"set": "eoe", "collation": "paper", "deck_choice": {"k": 4, "explore": 0.0}, "p1": {"deck": given}}}).to_string()).unwrap();
    let mut a = MtgAz::new_game(&g, 2, 9).unwrap();
    assert_eq!((a.to_move(), a.value_choice()), (Some(0), true));
    assert_eq!(a.m.how[1].unwrap().mode, sealed::Mode::Given);
    a.step(act::NUMBER + 3, &mut Vec::new()).unwrap();
    assert!(a.deck_choice().is_none());
    assert_eq!(game_deck(&a, 1).len(), 40);
    // a spec without the key: no choice, the decks of before
    let plain = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper"}}"#).unwrap();
    let a = MtgAz::new_game(&plain, 2, 4).unwrap();
    assert!(a.deck_choice().is_none());
    assert_eq!(game_deck(&a, 0), plain.try_seats(4).unwrap()[0].deck);
    let _ = Collation::Paper;
}

#[test]
fn the_game_log_records_the_chosen_decks() {
    let dir = std::env::temp_dir().join(format!("mtgz-deck-choice-log-{}", std::process::id()));
    let path = dir.join("games.jsonl");
    let _ = std::fs::remove_dir_all(&dir);
    let s = spec(4, 0.0, "");
    let sl = s.sealed.as_deref().unwrap();
    engine::az::game_log::set(Some(path.to_str().unwrap())).unwrap();
    let mut rng = Rng::new(5);
    let mut want = Vec::new();
    for seed in 0..3u64 {
        let mut g = MtgEoeAz::new_game(&s, 2, seed).unwrap();
        let mut keys = Vec::new();
        let mut picks = Vec::new();
        while g.to_move().is_some() {
            let mut l = Vec::new();
            g.legal_actions(&mut l);
            let a = l[rng.below(l.len() as u32) as usize];
            if g.value_choice() {
                picks.push((a - act::NUMBER) as usize);
            }
            g.step(a, &mut keys).unwrap();
        }
        want.push((seed, picks));
    }
    engine::az::game_log::set(None).unwrap();
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let games: Vec<&serde_json::Value> = lines.iter().filter(|l| l.get("seats").is_some()).collect();
    assert_eq!(games.len(), 3);
    let db = sealed::db();
    for (l, (seed, picks)) in games.iter().zip(&want) {
        for p in 0..2usize {
            let c = &sl.candidates(*seed, p as u8).unwrap().unwrap().0[picks[p]];
            assert_eq!(l["seats"][p]["mode"], "chosen");
            assert_eq!(l["seats"][p]["candidate"], picks[p]);
            let mut m = serde_json::Map::new();
            for &d in &c.deck {
                let n = db.def(d).name.clone();
                let k = m.get(&n).and_then(|v| v.as_u64()).unwrap_or(0);
                m.insert(n, serde_json::json!(k + 1));
            }
            assert_eq!(l["seats"][p]["deck"], serde_json::Value::Object(m));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
