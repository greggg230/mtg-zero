//! Thompson-sampled sideboard plans (ENGINE.md 274, a choice table's `"explore": {"share", "members"}`): each seat of
//! the table draws its plan for the match once, from the match's seed (a member with probability `share`, uniform; else
//! the table's own choices), and its teacher decisions play that plan. The draw is the same in every world and every
//! replay; a dealt seat's base (ENGINE.md 261) stays the table's own choice; malformed alternatives are refused loudly.

use engine::az::constructed::Decklist;
use engine::az::metagame::Metagame;
use engine::az::sbtable::draw_member;
use engine::az::{MtgAz, MtgSpec};
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

/// A choice table of deck `d`: each context the main with 1..=12 random one-for-one swaps (seeded); with `members` > 0
/// that many Thompson members (each such a random table's choices) at `share`.
fn table_of(d: &Decklist, seed: u64, share: f64, members: u64) -> Value {
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
    let choices = |s: u64| -> Vec<Value> {
        let mut r = Rng::new(s);
        let mut out = Vec::new();
        for game in 2..=3u8 {
            for first in 0..2u8 {
                for lead in -1..=1i8 {
                    for od in 0..2u8 {
                        let mut c = base.clone();
                        for _ in 0..1 + r.below(12) {
                            let out_: Vec<usize> = (0..c.len()).filter(|&k| c[k] > 0).collect();
                            let inn: Vec<usize> = (0..c.len()).filter(|&k| c[k] < copies[k]).collect();
                            let (o, i) = (out_[r.below(out_.len() as u32) as usize], inn[r.below(inn.len() as u32) as usize]);
                            if o != i {
                                c[o] -= 1;
                                c[i] += 1;
                            }
                        }
                        out.push(json!([game, first, lead, od, c]));
                    }
                }
            }
        }
        out
    };
    let list: Vec<Value> = names.iter().zip(&copies).map(|(n, k)| json!([n, k])).collect();
    let mut t = json!({"version": format!("t{seed}"), "list": list, "choices": choices(seed)});
    if members > 0 {
        let ms: Vec<Value> = (1..=members).map(|j| json!(choices(seed ^ j << 40))).collect();
        t["explore"] = json!({"share": share, "members": ms});
    }
    t
}

/// The fixed two-75 best-of-three of Delver (seat 0) and Doomsday (seat 1), whole matches, one episode per game,
/// `extra` merged in (`decks.*` keys under `decks`).
fn fixed(extra: Value) -> Value {
    let (da, db) = (deck(0), deck(1));
    let mut v = json!({"max_turns": 12, "constructed": true, "format": "bo3", "game_episodes": true,
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

fn parse(v: &Value) -> Result<MtgSpec, String> {
    <MtgAz as SearchGame>::parse_spec(&v.to_string())
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn dist(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).map(|(&x, &y)| x.saturating_sub(y) as usize).sum()
}

#[test]
fn explore_parses_and_malformed_alternatives_are_refused() {
    let t = table_of(&deck(0), 7, 0.5, 3);
    let s = parse(&fixed(json!({"decks.p0_table": t.clone()}))).unwrap();
    let st = s.seat_tables[0].as_ref().unwrap();
    assert_eq!(st.members(), 3);
    assert_eq!(st.explore.as_ref().unwrap().share, 0.5);
    // without explore: the same table as before, its key unchanged; with it another key
    let mut plain = t.clone();
    plain.as_object_mut().unwrap().remove("explore");
    let p = parse(&fixed(json!({"decks.p0_table": plain.clone()}))).unwrap();
    let pt = p.seat_tables[0].as_ref().unwrap();
    assert!(pt.explore.is_none() && pt.members() == 0);
    assert_ne!(pt.key, st.key);
    assert_eq!(pt.list, st.list);
    // every context of every member read: member k's plan is its choices
    let m1 = &t["explore"]["members"][1];
    for c in m1.as_array().unwrap() {
        let a = c.as_array().unwrap();
        let want: Vec<u8> = a[4].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
        let got = st.plan(Some(1), a[0].as_u64().unwrap() as u8, a[1] == 1, a[2].as_i64().unwrap() as i8, a[3] == 1);
        // (the parsed list is in the definitions' order: compare as (card name, copies) sets)
        let names: Vec<String> = t["list"].as_array().unwrap().iter().map(|e| e[0].as_str().unwrap().to_string()).collect();
        let mut w: Vec<(String, u8)> = names.into_iter().zip(want.iter().copied()).collect();
        let mut gv: Vec<(String, u8)> = st.list.iter().zip(got.iter()).map(|(&(d, _), &n)| (s.db().def(d).name.clone(), n)).collect();
        w.sort();
        gv.sort();
        assert_eq!(gv, w, "member 1's choice {:?}", &a[..4]);
    }
    assert_eq!(st.plan(None, 2, true, 0, false), st.choice(2, true, 0, false));
    // refusals, each naming explore
    let bad = |f: &dyn Fn(&mut Value)| {
        let mut x = t.clone();
        f(&mut x);
        let e = parse(&fixed(json!({"decks.p0_table": x}))).err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("explore"), "{e}");
    };
    for share in [json!(0.0), json!(-0.1), json!(1.5), json!("x"), json!(null)] {
        bad(&|x: &mut Value| x["explore"]["share"] = share.clone());
    }
    bad(&|x: &mut Value| {
        x["explore"].as_object_mut().unwrap().remove("share");
    });
    bad(&|x: &mut Value| x["explore"]["extra"] = json!(1));
    bad(&|x: &mut Value| x["explore"] = json!([1]));
    bad(&|x: &mut Value| x["explore"]["members"] = json!([]));
    bad(&|x: &mut Value| x["explore"]["members"] = json!(3));
    bad(&|x: &mut Value| {
        x["explore"]["members"][2].as_array_mut().unwrap().pop();
    });
    bad(&|x: &mut Value| {
        let c = x["explore"]["members"][0][0].clone();
        x["explore"]["members"][0][1] = c;
    });
    bad(&|x: &mut Value| {
        // a member's deck of another size: one copy more of an entry the deck does not hold in full
        let d = &mut x["explore"]["members"][1][5][4];
        let copies: Vec<u64> = t["list"].as_array().unwrap().iter().map(|e| e[1].as_u64().unwrap()).collect();
        let i = (0..copies.len()).find(|&i| d[i].as_u64().unwrap() < copies[i]).unwrap();
        d[i] = json!(d[i].as_u64().unwrap() + 1);
    });
    bad(&|x: &mut Value| {
        let n = t["list"][0][1].as_u64().unwrap();
        x["explore"]["members"][0][3][4][0] = json!(n + 1);
    });
    // the evaluation spec drops the tables, explore with them; the pairing's fixed spec carries them
    let ev: Value = serde_json::from_str(&MtgSpec::eval_json(&fixed(json!({"decks.p0_table": t.clone()})).to_string()).unwrap()).unwrap();
    assert!(ev["decks"].get("p0_table").is_none());
    let decks: Vec<Value> = (0..2)
        .map(|k| {
            let d = deck(k);
            json!({"name": d.name, "main": d.main_names(), "sideboard": side_names(&d), "table": table_of(&d, 9 + k as u64, 0.5, 2)})
        })
        .collect();
    let meta = json!({"max_turns": 12, "constructed": true, "format": "bo3", "metagame": {"decks": decks, "weights": [[0, 1], [1, 0]]}});
    let f: Value = serde_json::from_str(&Metagame::pair_json(&meta.to_string(), [1, 0]).unwrap()).unwrap();
    assert_eq!(f["decks"]["p0_table"]["explore"]["members"].as_array().unwrap().len(), 2);
    assert!(parse(&f).is_ok());
}

#[test]
fn the_draw_is_the_seeds_per_seat_and_uniform_over_its_members() {
    let n = scale(40_000);
    let (share, k) = (0.5, 3usize);
    let mut hist = [[0u64; 4]; 2];
    let mut both_none = 0u64;
    for seed in 0..n {
        let d = [0u8, 1].map(|p| draw_member(seed, p, share, k));
        assert_eq!(d, [0u8, 1].map(|p| draw_member(seed, p, share, k)), "the draw is a function of the seed");
        for p in 0..2 {
            hist[p][d[p].map_or(3, |x| x as usize)] += 1;
        }
        both_none += (d[0].is_none() && d[1].is_none()) as u64;
        assert!(draw_member(seed, 0, 1.0, k).is_some(), "share 1: always a member");
    }
    let within = |c: u64, p: f64, what: &str| {
        let (m, se) = (n as f64 * p, (n as f64 * p * (1.0 - p)).sqrt());
        assert!((c as f64 - m).abs() < 5.0 * se, "{what}: {c} of {n}, expected {m:.0} +- {se:.0}");
    };
    for p in 0..2 {
        within(hist[p][3], 1.0 - share, "the table's own");
        for j in 0..k {
            within(hist[p][j], share / k as f64, "a member");
        }
    }
    within(both_none, (1.0 - share) * (1.0 - share), "both seats the table's own (independent seats)");
    // the match's draws are the table's: `MtgAz::new` sets them, a spec without explore draws nothing
    let t0 = table_of(&deck(0), 11, 0.5, 3);
    let t1 = table_of(&deck(1), 12, 1.0, 2);
    let s = parse(&fixed(json!({"decks.p0_table": t0, "decks.p1_table": t1}))).unwrap();
    let plain = parse(&fixed(json!({"decks.p0_table": table_of(&deck(0), 11, 0.0, 0)}))).unwrap();
    for seed in 0..200 {
        let g = MtgAz::new(&s, seed).unwrap();
        assert_eq!(g.m.table_member, [draw_member(seed, 0, 0.5, 3), draw_member(seed, 1, 1.0, 2)]);
        assert_eq!(MtgAz::new(&plain, seed).unwrap().m.table_member, [None, None]);
    }
}

/// Whole matches by random actions (the table's at its seats' decisions, as zero-core's driver plays a teacher
/// decision): every table seat's next game is its drawn plan's 60 for its context; returns (the seats checked, the
/// members drawn, the seats whose plan differed from the table's own).
fn play_and_check(s: &MtgSpec, seed: u64, actions: &mut Vec<u16>) -> (u32, std::collections::BTreeSet<u16>, u32) {
    let mut g = MtgAz::new(s, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0x7404);
    let mut pending: [Option<Vec<u8>>; 2] = [None, None];
    let (mut checked, mut members, mut differ) = (0u32, std::collections::BTreeSet::new(), 0u32);
    let mut game = g.m.game_no;
    for _ in 0..200_000 {
        if g.m.game_no != game && g.sideboarding().is_none() {
            game = g.m.game_no;
            for p in 0..2 {
                if let Some(want) = pending[p].take() {
                    assert_eq!(&g.m.decks[p][..], &want[..], "seed {seed} game {game}: P{p}'s 60 is not its drawn plan's");
                    checked += 1;
                }
            }
        }
        let Some(p) = g.to_move() else {
            actions.push(u16::MAX);
            match g.next_episode(None).unwrap() {
                Next::Continue(n) => {
                    g = n;
                    continue;
                }
                Next::Over(_) => break,
            }
        };
        let a = match g.table_action() {
            Some(a) => {
                let sb = g.sideboarding().unwrap();
                let lead = g.m.wins[p] as i8 - g.m.wins[1 - p] as i8;
                let t = g.m.table[p].as_ref().unwrap();
                let m = g.m.table_member[p];
                let want = t.plan(m, g.m.game_no + 1, sb.turn == 0, lead, g.m.is_dealt(1 - p as u8)).to_vec();
                if pending[p].is_none() {
                    if let Some(k) = m {
                        members.insert(k);
                    }
                    differ += (want != t.choice(g.m.game_no + 1, sb.turn == 0, lead, g.m.is_dealt(1 - p as u8)).to_vec()) as u32;
                }
                pending[p] = Some(want);
                a
            }
            None => {
                let l = legal(&g);
                l[rng.below(l.len() as u32) as usize]
            }
        };
        actions.push(a);
        g.step(a, &mut Vec::new()).unwrap();
    }
    (checked, members, differ)
}

#[test]
fn a_table_seat_plays_its_drawn_plan_and_a_replay_draws_the_same() {
    let s = parse(&fixed(json!({"decks.p0_table": table_of(&deck(0), 21, 1.0, 4), "decks.p1_table": table_of(&deck(1), 22, 0.5, 3)}))).unwrap();
    let (mut checked, mut seen0, mut differ) = (0u32, std::collections::BTreeSet::new(), 0u32);
    for seed in 0..scale(40) {
        let mut acts = Vec::new();
        let (c, m, d) = play_and_check(&s, seed, &mut acts);
        checked += c;
        differ += d;
        if MtgAz::new(&s, seed).unwrap().m.table_member[0].is_some() {
            seen0.extend(m);
        }
        // the replay: the same seed and actions, the same draws and the same match
        let mut a = MtgAz::new(&s, seed).unwrap();
        let mut b = MtgAz::new(&s, seed).unwrap();
        assert_eq!(a.m.table_member, b.m.table_member);
        let mut actions = acts.clone();
        actions.reverse();
        while let Some(x) = actions.pop() {
            if x == u16::MAX {
                match (a.next_episode(None).unwrap(), b.next_episode(None).unwrap()) {
                    (Next::Continue(na), Next::Continue(nb)) => {
                        a = na;
                        b = nb;
                    }
                    _ => break,
                }
                continue;
            }
            a.step(x, &mut Vec::new()).unwrap();
            b.step(x, &mut Vec::new()).unwrap();
            assert_eq!(a.m.table_member, b.m.table_member, "seed {seed}: a replay's draws");
        }
        assert_eq!(a.export_state(), b.export_state(), "seed {seed}: the replay");
    }
    println!("{checked} table seats' 60s checked, P0's members drawn {seen0:?}, {differ} plans other than the table's own");
    assert!(checked > 20 && seen0.len() >= 3 && differ > 10, "checked {checked}, members {seen0:?}, differ {differ}");
}

/// Every world of either viewer keeps both seats' draws (never reseeded); a dealt seat's 60 is dealt around the table's
/// OWN choice (ENGINE.md 261), never its drawn plan's.
#[test]
fn worlds_keep_the_draw_and_a_dealt_base_is_the_tables_own_choice() {
    let s = parse(&fixed(json!({"dealt_decks": {"p": 0.5, "swaps": [1, 1], "per_seat": true, "around": "table"},
        "decks.p0_table": table_of(&deck(0), 31, 1.0, 3), "decks.p1_table": table_of(&deck(1), 32, 1.0, 3)}))).unwrap();
    let (mut worlds, mut dealt_checked, mut off_plan) = (0u32, 0u32, 0u32);
    for seed in 0..scale(40) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let mut rng = Rng::new(seed ^ 0x7405);
        let mut game = g.m.game_no;
        let real = g.m.table_member;
        assert!(real.iter().all(|m| m.is_some()), "share 1: both seats draw a member");
        for step in 0..200_000u32 {
            if g.m.game_no != game && g.sideboarding().is_none() {
                game = g.m.game_no;
                for q in 0..2usize {
                    if !g.m.is_dealt(q as u8) {
                        continue;
                    }
                    let first = g.g.turn().chooser == q as u8;
                    let mut w = [0i8; 2];
                    for i in 0..(game as usize - 1) {
                        if let r @ (1 | 2) = g.m.results[i] {
                            w[(r - 1) as usize] += 1;
                        }
                    }
                    let lead = w[q] - w[1 - q];
                    let t = g.m.table[q].as_ref().unwrap();
                    let own = t.choice(game, first, lead, g.m.is_dealt(1 - q as u8));
                    assert_eq!(dist(&g.m.decks[q], own), 1, "seed {seed} game {game}: P{q}'s dealt 60 is not 1 swap from its table's own choice");
                    let plan = t.plan(g.m.table_member[q], game, first, lead, g.m.is_dealt(1 - q as u8));
                    off_plan += (dist(&g.m.decks[q], plan) != 1) as u32;
                    dealt_checked += 1;
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
            if step % 5 == 0 {
                for v in 0..2usize {
                    let mut w = g.clone();
                    w.determinize(v, &mut zero_search::rng::Rng::new(seed ^ (v as u64) << 40 ^ step as u64));
                    assert_eq!(w.m.table_member, real, "seed {seed} step {step}: a world of P{v} changed a Thompson draw");
                    assert_eq!(w.m.table, g.m.table);
                    worlds += 1;
                }
            }
            let a = g.table_action().unwrap_or_else(|| {
                let l = legal(&g);
                l[rng.below(l.len() as u32) as usize]
            });
            g.step(a, &mut Vec::new()).unwrap();
            assert_eq!(g.m.table_member, real, "seed {seed}: the draw moved in the match");
        }
    }
    println!("{worlds} worlds kept the draws; {dealt_checked} dealt 60s 1 swap from the table's own choice, {off_plan} of them not 1 from the drawn plan");
    assert!(worlds > 100 && dealt_checked > 10 && off_plan > 0, "worlds {worlds}, dealt {dealt_checked}, off plan {off_plan}");
}
