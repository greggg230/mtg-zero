//! Drafted matches (M6 part 3, DESIGN.md 2.3.6, ENGINE.md 45): the `mtg` spec of
//! a `team_draft` event's match (`pN_first_picks`, `pN_elsewhere`, `seed`) plays
//! exactly the event's match (`draft_bots::match_game`), the first-pick prior
//! is the cards the viewer's team never saw (the real first picks among them),
//! `determinize` samples from it, the spec checks its knowledge; specs without
//! the keys are what they were. Also `team_draft`'s other part-3 keys: a
//! position from a draft history (the human-pick yardstick) and deckbuilding's
//! simulations divisor.

use engine::az::bots::{self, Heuristic};
use engine::az::draft::{cube, DraftSpec, Phase, TeamDraftAz};
use engine::az::draft_bots::{self, match_game, match_spec, Consensus};
use engine::az::draft_encode;
use engine::az::matches::Format;
use engine::az::{DeckMode, MtgAz, MtgSpec, NUM_FEATURES};
use engine::db::CardDb;
use engine::ids::DefId;
use engine::rng::Rng;
use zero_search::game::{Bot, SearchGame};

fn legal<G: SearchGame>(g: &G) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// An event drafted and built: random actions, or `consensus` for team A
/// (seats 0, 2, ...) and random for team B.
fn drafted(players: usize, seed: u64, consensus: bool) -> TeamDraftAz {
    let mut g = TeamDraftAz::new(&DraftSpec::default(), players, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0xD4AF7);
    let mut bot = Consensus::default();
    let mut keys = Vec::new();
    while let Some(p) = g.to_move() {
        let l = legal(&g);
        let a = if consensus && p % 2 == 0 { bot.choose(&g, &l) } else { l[rng.below(l.len() as u32) as usize] };
        g.step(a, &mut keys).unwrap();
    }
    assert!(g.deferred());
    g
}

fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode(viewer, &mut x);
    x
}

/// Play a match with the heuristic on both sides -> (keys, results, steps).
fn play(mut az: MtgAz, seed: u64) -> (Vec<u64>, [f32; 2], [u8; 3], u64) {
    let mut h = [Heuristic::new(seed ^ 1), Heuristic::new(seed ^ 2)];
    let mut all = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0;
    while let Some(p) = az.to_move() {
        let l = legal(&az);
        let a = if l.len() == 1 {
            l[0]
        } else {
            match az.sideboarding() {
                Some(sb) => bots::sideboard(sb, &l),
                None => h[p].choose(&az.g, &l),
            }
        };
        keys.clear();
        az.step(a, &mut keys).unwrap();
        all.extend_from_slice(&keys);
        steps += 1;
        assert!(steps < 400_000);
    }
    let r = az.result();
    (all, [r[0], r[1]], az.m.results, steps)
}

#[test]
fn a_drafted_spec_plays_the_events_match() {
    // 4v4 and 2v2, random and consensus-built decks, best-of-three and best-of-one
    let mut checked = 0;
    for (players, seed, cons, fmt) in [(8, 1, false, "bo3"), (8, 2, true, "bo3"), (4, 3, true, "bo3"), (4, 4, false, "bo1")] {
        let mut ev = drafted(players, seed, cons);
        if fmt == "bo1" {
            // (the event's format is the matches': rebuild the event with it)
            let mut g = TeamDraftAz::new(&DraftSpec::parse(r#"{"format": "bo1"}"#).unwrap(), players, seed).unwrap();
            let mut rng = Rng::new(seed);
            let mut keys = Vec::new();
            while g.to_move().is_some() {
                let l = legal(&g);
                g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            }
            ev = g;
        }
        let format = ev.ev.format;
        for (i, setup) in ev.matches().unwrap().iter().enumerate() {
            let json = match_spec(setup, ev.ev.max_turns, format);
            let spec = MtgSpec::parse(&json).unwrap();
            assert!(matches!(spec.decks, DeckMode::Drafted(_)) && spec.seed == Some(setup.seed));
            // the driver's own seed is ignored: the spec carries the event's match seed
            let a = MtgAz::new_game(&spec, 2, 12345).unwrap();
            let b = match_game(setup, ev.ev.max_turns, format).unwrap();
            assert_eq!(a.g.fingerprint(), b.g.fingerprint(), "match {i}");
            assert_eq!(a.m.pools, b.m.pools);
            assert_eq!(a.m.decks, b.m.decks);
            for p in 0..2 {
                assert_eq!(a.m.deck(p as u8).len(), 40);
                assert_eq!(enc(&a, p), enc(&b, p), "match {i}: the encoding of P{p}");
            }
            match (&a.m.priors, &b.m.priors) {
                (Some(x), Some(y)) => {
                    assert_eq!(format, Format::Bo3);
                    assert_eq!((&x[0].cands, &x[1].cands), (&y[0].cands, &y[1].cands));
                }
                (None, None) => assert_eq!(format, Format::Bo1),
                _ => panic!("priors"),
            }
            let pa = play(a, seed + i as u64);
            let pb = play(b, seed + i as u64);
            assert_eq!(pa, pb, "match {i}: the spec's match and the event's differ");
            checked += 1;
        }
    }
    assert_eq!(checked, 12 + 12 + 4 + 4);
}

#[test]
fn the_first_pick_prior_is_the_cards_the_team_never_saw() {
    let c = cube().unwrap();
    for (players, want) in [(8usize, 193usize), (4, 367)] {
        for seed in 0..3u64 {
            let ev = drafted(players, 100 + seed, seed == 1);
            for setup in ev.matches().unwrap() {
                let spec = MtgSpec::parse(&match_spec(&setup, 60, Format::Bo3)).unwrap();
                let az = MtgAz::new_game(&spec, 2, 0).unwrap();
                let pri = az.m.priors.as_ref().unwrap();
                for owner in 0..2usize {
                    let viewer = 1 - owner;
                    let team = setup.seats[viewer] % 2;
                    let known = ev.team_known(team);
                    // the candidates: exactly the draftable cards the viewer's team has no place for
                    let mut never: Vec<DefId> =
                        c.cards.iter().copied().filter(|x| known.binary_search(x).is_err()).map(|x| c.def_of(x)).collect();
                    never.sort_unstable();
                    let mut cands = pri[owner].cands.clone();
                    cands.sort_unstable();
                    assert_eq!(cands, never, "{players} seats, seed {seed}");
                    assert_eq!(cands.len(), want);
                    // the opponent's real first picks are among them, none of the viewer's knowledge
                    for f in &setup.players[owner].first_picks {
                        assert!(cands.binary_search(f).is_ok());
                    }
                    for e in &setup.players[viewer].elsewhere {
                        assert!(cands.binary_search(e).is_err());
                    }
                    // without `elsewhere` (a generated pool's prior) the candidates are ~450
                    let mut s2 = spec.clone();
                    if let DeckMode::Drafted(seats) = &mut s2.decks {
                        seats[viewer].elsewhere.clear();
                    }
                    let az2 = MtgAz::new_game(&s2, 2, 0).unwrap();
                    assert!(az2.m.priors.as_ref().unwrap()[owner].cands.len() > 400);
                }
            }
        }
    }
}

#[test]
fn determinize_samples_unseen_first_picks_from_the_prior() {
    let mut worlds = 0;
    let mut real_kept = 0;
    let mut differ = 0;
    for seed in 0..4u64 {
        let ev = drafted(8, 200 + seed, seed % 2 == 0);
        for setup in ev.matches().unwrap().iter().take(3) {
            let spec = MtgSpec::parse(&match_spec(setup, 60, Format::Bo3)).unwrap();
            let mut g = MtgAz::new_game(&spec, 2, 0).unwrap();
            let mut rng = Rng::new(seed);
            let mut keys = Vec::new();
            for step in 0..600 {
                let Some(p) = g.to_move() else { break };
                if step % 37 == 0 {
                    for viewer in [p, 1 - p] {
                        let o = 1 - viewer;
                        let mut d = g.clone();
                        d.determinize(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()));
                        let fp = &d.m.pools[o].first_picks;
                        let real = &setup.players[o].first_picks;
                        let cands = &g.m.priors.as_ref().unwrap()[o].cands;
                        assert_eq!(fp.len(), 3);
                        for f in fp {
                            // a sampled first pick is a card the viewer's team never saw (or one
                            // of the real ones the viewer has seen this match: kept)
                            assert!(cands.contains(f) || real.contains(f));
                            assert!(!setup.players[viewer].elsewhere.contains(f));
                            assert!(!setup.players[viewer].deck.contains(f) && !setup.players[viewer].sideboard.contains(f));
                        }
                        assert_eq!(enc(&g, viewer), enc(&d, viewer), "determinize changed the viewer's encoding");
                        worlds += 1;
                        real_kept += (fp == real) as u32;
                        differ += (fp != real) as u32;
                    }
                }
                let l = legal(&g);
                g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            }
        }
    }
    assert!(worlds > 50 && differ > worlds * 3 / 4, "{worlds} worlds, {differ} with other first picks, {real_kept} real");
}

#[test]
fn a_drafted_spec_checks_its_knowledge() {
    let ev = drafted(4, 300, true);
    let setup = &ev.matches().unwrap()[0];
    let good = match_spec(setup, 60, Format::Bo3);
    assert!(MtgSpec::parse(&good).is_ok());
    let db = CardDb::cube();
    let name = |d: DefId| serde_json::Value::from(db.def(d).name.clone());
    let edit = |f: &dyn Fn(&mut serde_json::Value)| -> Result<MtgSpec, String> {
        let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
        f(&mut v);
        MtgSpec::parse(&v.to_string())
    };
    let [a, b] = &setup.players;
    let basics = engine::az::pools::basic_ids(db);
    let opp_card = name(b.deck.iter().find(|d| !b.first_picks.contains(d) && !basics.contains(d)).copied().unwrap());
    let c = cube().unwrap();
    let not_pool = name(c.cards.iter().map(|&x| c.def_of(x)).find(|d| !a.deck.contains(d) && !a.sideboard.contains(d)).unwrap());
    let opp_fp = name(b.first_picks[0]);
    let push = |key: &'static str, x: serde_json::Value| move |v: &mut serde_json::Value| v["decks"][key].as_array_mut().unwrap().push(x.clone());
    let cases: Vec<(Box<dyn Fn(&mut serde_json::Value)>, &str)> = vec![
        (Box::new(move |v| v["decks"]["p0_first_picks"][0] = not_pool.clone()), "is not in its pool"),
        (Box::new(|v| {
            let x = v["decks"]["p0_first_picks"][0].clone();
            v["decks"]["p0_first_picks"][1] = x;
        }), "twice"),
        (Box::new(push("p0_elsewhere", opp_fp)), "a first pick of p1"),
        (Box::new(push("p0_elsewhere", opp_card)), "a card of p1"),
        (Box::new(push("p1_elsewhere", "Island".into())), "basic land"),
        (Box::new(|v| v["decks"]["p1_wishes"] = serde_json::json!([])), "unknown decks key"),
        (Box::new(|v| v["seed"] = serde_json::json!(-1)), "seed"),
    ];
    for (f, want) in cases {
        let e = edit(&*f).unwrap_err();
        assert!(e.contains(want), "{e} (want {want:?})");
    }
}

#[test]
fn specs_without_the_keys_are_what_they_were() {
    // lists without knowledge keep their modes; the seed key only replaces the driver's seed
    let s = MtgSpec::parse(r#"{"decks": {"p0": ["Island"], "p1": ["Plains"]}}"#).unwrap();
    assert!(matches!(s.decks, DeckMode::Lists(..)) && s.seed.is_none());
    let s = MtgSpec::parse(r#"{"decks": {"p0": ["Island"], "p1": ["Plains"], "p1_sideboard": ["Swamp"]}}"#).unwrap();
    assert!(matches!(s.decks, DeckMode::ListsWithSideboards(..)));
    for json in [r#"{"format": "bo3"}"#, r#"{"format": "bo3", "decks": "eval"}"#, r#"{}"#] {
        let s = MtgSpec::parse(json).unwrap();
        let a = MtgAz::new_game(&s, 2, 77).unwrap();
        let mut s2 = MtgSpec::parse(json).unwrap();
        s2.seed = Some(77);
        let b = MtgAz::new_game(&s2, 2, 5).unwrap();
        assert_eq!(a.g.fingerprint(), b.g.fingerprint());
        assert_eq!(s.seats(77).map(|x| x.elsewhere.len()), [0, 0]);
    }
}

/// Seat 0's history in an event drafted with random picks, `k` picks in.
fn history_of(players: usize, seed: u64, k: usize) -> (TeamDraftAz, String) {
    let mut g = TeamDraftAz::new(&DraftSpec::default(), players, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0x415);
    let mut keys = Vec::new();
    let c = cube().unwrap();
    let mut views: Vec<Vec<String>> = Vec::new();
    let mut picks: Vec<String> = Vec::new();
    loop {
        let p = g.to_move().unwrap();
        let l = legal(&g);
        if p == 0 {
            views.push(l.iter().map(|&x| c.name(x).to_string()).collect());
            if picks.len() == k {
                break;
            }
        }
        let a = l[rng.below(l.len() as u32) as usize];
        if p == 0 {
            picks.push(c.name(a).to_string());
        }
        g.step(a, &mut keys).unwrap();
    }
    (g, serde_json::json!({"history": {"views": views, "picks": picks}}).to_string())
}

#[test]
fn a_position_from_a_draft_history_is_seat_0s_information_set() {
    let mut checked = 0;
    for (players, k) in [(8usize, 0usize), (8, 5), (8, 9), (8, 23), (8, 44), (4, 13), (4, 31), (2, 40)] {
        for seed in 0..3u64 {
            let (real, json) = history_of(players, seed * 31 + k as u64, k);
            let spec = DraftSpec::parse(&json).unwrap();
            let h = TeamDraftAz::new_game(&spec, players, 999 + seed).unwrap();
            h.check_invariants().unwrap();
            assert_eq!((h.phase(), h.to_move(), h.pick_round()), (Phase::Draft, Some(0), real.pick_round()));
            assert_eq!(legal(&h), legal(&real));
            assert_eq!(h.knowledge(1), real.knowledge(1), "{players} seats, {k} picks");
            let (mut x, mut y) = (vec![0.0; draft_encode::NUM_FEATURES], vec![0.0; draft_encode::NUM_FEATURES]);
            h.encode(0, &mut x);
            real.encode(0, &mut y);
            assert_eq!(x, y, "{players} seats, {k} picks: the encoding of seat 0");
            // the event goes on from there
            let mut g = h.clone();
            let mut rng = Rng::new(seed);
            let mut keys = Vec::new();
            while g.to_move().is_some() {
                let l = legal(&g);
                g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            }
            assert!(g.deferred() && g.matches().unwrap().len() == players / 2 * (players / 2).min(3));
            checked += 1;
        }
    }
    assert_eq!(checked, 24);
    // what does not fit the event is an error
    let (_, json) = history_of(8, 5, 10);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let bad = |v: &serde_json::Value, f: &dyn Fn(&mut serde_json::Value)| {
        let mut w = v.clone();
        f(&mut w);
        DraftSpec::parse(&w.to_string()).and_then(|s| TeamDraftAz::new_game(&s, 8, 1).map(|_| ())).unwrap_err()
    };
    assert!(bad(&v, &|w| w["history"]["picks"].as_array_mut().unwrap().truncate(9)).contains("views"));
    assert!(bad(&v, &|w| w["history"]["views"][3].as_array_mut().unwrap().truncate(5)).contains("cards"));
    let copy = |from: (usize, usize), to: (&'static str, usize, Option<usize>)| {
        move |w: &mut serde_json::Value| {
            let x = w["history"]["views"][from.0][from.1].clone();
            match to.2 {
                Some(j) => w["history"][to.0][to.1][j] = x,
                None => w["history"][to.0][to.1] = x,
            }
        }
    };
    assert!(bad(&v, &copy((1, 0), ("picks", 2, None))).contains("not in its pack"));
    let e = bad(&v, &copy((0, 1), ("views", 9, Some(0))));
    assert!(e.contains("in two packs"), "{e}");
    assert!(bad(&v, &|w| w["history"]["views"][0][0] = "Island".into()).contains("not a draftable card"));
    // a pack seen again (pick 8 in an 8-seat pod) that gained a card
    let (_, json) = history_of(8, 6, 9);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = cube().unwrap();
    let unseen: Vec<String> = {
        let seen: std::collections::HashSet<String> =
            v["history"]["views"].as_array().unwrap().iter().flat_map(|x| x.as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string())).collect();
        c.cards.iter().map(|&x| c.name(x).to_string()).filter(|n| !seen.contains(n)).take(1).collect()
    };
    let j = (0..7).find(|&j| v["history"]["views"][8][j] != v["history"]["picks"][8]).unwrap();
    let e = bad(&v, &|w| w["history"]["views"][8][j] = unseen[0].clone().into());
    assert!(e.contains("gained"), "{e}");
}

#[test]
fn deckbuilding_has_its_own_simulations_divisor() {
    for (json, div) in [("{}", 1u32), (r#"{"build_sims_div": 2}"#, 2)] {
        let mut g = TeamDraftAz::new_game(&DraftSpec::parse(json).unwrap(), 4, 1).unwrap();
        let mut keys = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        while g.to_move().is_some() {
            seen.insert((g.phase() == Phase::Build, g.sims_divisor()));
            let l = legal(&g);
            g.step(l[0], &mut keys).unwrap();
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![(false, 1), (true, div)]);
    }
    for bad in [r#"{"build_sims_div": 0}"#, r#"{"build_sims_div": 17}"#] {
        assert!(DraftSpec::parse(bad).is_err());
    }
    // the heuristic-played matches are unchanged by `elsewhere` (the bot reads the real game)
    let mut ev = drafted(4, 7, true);
    let r = draft_bots::resolve_with_heuristic(&mut ev, 7).unwrap();
    assert_eq!(r.len(), 4);
}
