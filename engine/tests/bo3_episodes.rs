//! Best-of-three with one episode per game (ENGINE.md 121, `"game_episodes": true`; zero-core docs/az.md 42): a
//! game's end ends the episode (terminal for the search and the labels), `next_episode` continues the match by
//! 36.1's rules (or ends it), a concession (CR 104.3a) ends the game in progress, and the driver labels every row with
//! its own game's outcome - the sideboarding before game g among game g's rows.

use engine::az::encode::{mtch, NUM_FEATURES, O_MATCH};
use engine::az::{Format, MtgAz, MtgSpec};
use engine::game::EndReason;
use engine::rng::Rng;
use zero_search::driver::{Driver, DriverCfg, GameResult, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{Next, SearchGame, MAX_SEATS};
use zero_search::tree::{leaf_values, Scratch, Sim, Tree, Walk};

const BO3: &str = r#"{"max_turns": 60, "eval_decks": 16, "eval_seed": 1, "format": "bo3"}"#;
const BO3_GAMES: &str = r#"{"max_turns": 60, "eval_decks": 16, "eval_seed": 1, "format": "bo3", "game_episodes": true}"#;

fn spec(json: &str) -> MtgSpec {
    MtgSpec::parse(json).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

#[test]
fn game_episodes_is_a_best_of_three_setting() {
    let e = MtgSpec::parse(r#"{"game_episodes": true}"#).unwrap_err();
    assert!(e.contains("best-of-three"), "{e}");
    assert!(MtgSpec::parse(r#"{"game_episodes": 1, "format": "bo3"}"#).unwrap_err().contains("boolean"));
    let s = spec(BO3_GAMES);
    assert!(s.game_episodes && s.format == Format::Bo3);
    assert!(!spec(BO3).game_episodes);
    // the evaluation spec keeps the key (it changes only the decks)
    assert!(spec(&MtgSpec::eval_json(BO3_GAMES).unwrap()).game_episodes);
    let g = MtgAz::new(&s, 7).unwrap();
    assert!(g.chained() && g.can_concede());
    let g0 = MtgAz::new(&spec(BO3), 7).unwrap();
    assert!(!g0.chained());
    assert!(g0.next_episode(None).is_err(), "a match without the key does not chain");
}

/// One match played twice with the same random actions: as one episode (36.6) and as one episode per game (112):
/// the same games, decisions, legal actions and keys - the match cut at its games' ends, nothing else changed - and
/// at each game's end the episode's terminal (the game's result and life margin), the match's result at its end.
#[test]
fn the_match_is_36_1s_cut_at_its_games_ends() {
    let mut checked = [0usize; 3];
    for seed in 0..12u64 {
        let mut a = MtgAz::new(&spec(BO3), seed).unwrap();
        let mut b = MtgAz::new(&spec(BO3_GAMES), seed).unwrap();
        let mut rng = Rng::new(seed ^ 99);
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        let mut episodes = 1;
        loop {
            if b.to_move().is_none() {
                // an episode's end: the game's result and margin, never the match's
                assert!(b.g.is_over() && b.sideboarding().is_none());
                let r = b.result();
                let gv = b.g.result_values();
                assert_eq!((r[0], r[1]), (gv[0], gv[1]));
                for p in 0..2 {
                    let life = ((b.g.life(p as u8).max(0) - b.g.life(1 - p as u8).max(0)) as f32 / 20.0).clamp(-1.0, 1.0);
                    assert_eq!(b.margin(p), life);
                }
                match b.next_episode(None).unwrap() {
                    Next::Continue(n) => {
                        // the next episode begins with the sideboarding: the loser of the game first (CR 103.1)
                        let sb = n.sideboarding().expect("sideboarding begins the next episode");
                        let w = b.g.outcome().unwrap().winner;
                        match w {
                            Some(w) => assert_eq!(sb.decider(), 1 - w),
                            None => assert_eq!(sb.decider(), b.g.turn().chooser),
                        }
                        assert!(!n.can_concede(), "no game is in progress while sideboarding");
                        b = n;
                        episodes += 1;
                        checked[1] += 1;
                        continue;
                    }
                    Next::Over(r) => {
                        // 36.1: two wins or three games; the result by game wins - the one-episode match's
                        assert!(a.match_over() && a.to_move().is_none());
                        assert!(b.m.wins.iter().any(|&w| w >= 2) || b.m.game_no == 3);
                        let ra = a.result();
                        assert_eq!((r[0], r[1]), (ra[0], ra[1]));
                        assert_eq!(episodes, b.m.game_no as usize);
                        assert_eq!(b.rounds(), a.rounds());
                        checked[2] += 1;
                        break;
                    }
                }
            }
            assert_eq!(a.to_move(), b.to_move(), "seed {seed}");
            let l = legal(&a);
            assert_eq!(l, legal(&b), "seed {seed}");
            let x = l[rng.below(l.len() as u32) as usize];
            ka.clear();
            kb.clear();
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            assert_eq!(ka, kb, "seed {seed}: the same keys");
            checked[0] += 1;
        }
    }
    assert!(checked[1] >= 12 && checked[2] == 12, "{checked:?}");
}

#[test]
fn a_concession_ends_the_game_in_progress() {
    let mut g = MtgAz::new(&spec(BO3_GAMES), 3).unwrap();
    let mut rng = Rng::new(3);
    let mut k = Vec::new();
    for _ in 0..40 {
        let l = legal(&g);
        g.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
    }
    assert!(g.to_move().is_some() && !g.g.is_over() && g.can_concede());
    let p = g.to_move().unwrap();
    let Next::Continue(n) = g.next_episode(Some(p)).unwrap() else { panic!("game 1 conceded: the match goes on") };
    let o = n.g.outcome().unwrap();
    assert_eq!((o.winner, o.reason), (Some(1 - p as u8), EndReason::Conceded), "CR 104.3a: the conceding player loses");
    assert_eq!(n.m.wins[1 - p], 1);
    assert_eq!(n.m.results[0], 1 + (1 - p as u8));
    assert_eq!(n.sideboarding().unwrap().decider() as usize, p, "the loser sideboards first and chooses (CR 103.1)");
    assert!(!n.can_concede());
    assert!(n.next_episode(Some(p)).is_err(), "no game is in progress to concede while sideboarding");
    // a second concession by the same player ends the match: two wins
    let mut m = n;
    while m.sideboarding().is_some() || m.m.game_no == 1 {
        let l = legal(&m);
        m.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
    }
    while m.to_move().is_some() && !m.can_concede() {
        let l = legal(&m);
        m.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
    }
    let Next::Over(r) = m.next_episode(Some(p)).unwrap() else { panic!("two wins end the match") };
    assert_eq!((r[p], r[1 - p]), (0.0, 1.0));
}

/// A position `left` real moves before game 1's end (the match's random line, `seed`).
fn late_in_game_1(json: &str, seed: u64, left: usize) -> Option<MtgAz> {
    let line = |stop: Option<usize>| -> (MtgAz, usize) {
        let mut g = MtgAz::new(&spec(json), seed).unwrap();
        let mut rng = Rng::new(seed);
        let mut k = Vec::new();
        let mut n = 0;
        while g.m.game_no == 1 && g.sideboarding().is_none() && !g.g.is_over() && Some(n) != stop {
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
            n += 1;
        }
        (g, n)
    };
    let (_, len) = line(None);
    let (g, _) = line(Some(len.checked_sub(left)?));
    g.to_move().map(|_| g)
}

/// Searches from late in game 1 (uniform priors, value 0.5): the states of every expanded node the walks pass.
fn search_states(json: &str) -> (usize, usize, usize) {
    let w = Walk { c_puct: 1.0, chance_k: 0, margin_w: 0.0 };
    let (mut past_end, mut terminals, mut nodes) = (0, 0, 0);
    for seed in 0..8u64 {
        let Some(live) = late_in_game_1(json, seed, 6) else { continue };
        let p = live.to_move().unwrap();
        let mut t = Tree::new(&live, p, &legal(&live), true, &mut zero_search::rng::Rng::new(seed));
        let mut g = live.clone();
        let mut sc = Scratch::default();
        let mut zrng = zero_search::rng::Rng::new(seed ^ 5);
        for _ in 0..400 {
            let r = t
                .simulate_with(&live, &mut g, &mut zrng, &w, &mut sc, |_, _, s: &MtgAz| {
                    nodes += 1;
                    past_end += (s.sideboarding().is_some() || s.m.game_no > 1) as usize;
                    None
                })
                .unwrap();
            match r {
                Sim::Terminal => terminals += 1,
                Sim::Leaf { node, player } => {
                    let k = t.acts(node).len();
                    let mut v = [0.0; MAX_SEATS];
                    v[0] = 0.5;
                    v[1] = 0.5;
                    let (util, win) = leaf_values(&v, &[0.0; MAX_SEATS], player, 2, 0.0);
                    t.expand(node, &vec![1.0 / k as f32; k], &util);
                    t.backup(&sc.path, &util, &win);
                }
            }
        }
    }
    (past_end, terminals, nodes)
}

#[test]
fn the_search_treats_a_games_end_as_terminal() {
    // one episode per game: walks from late in game 1 reach the game's end (terminals) and never a state past it
    let (past, terminals, nodes) = search_states(BO3_GAMES);
    assert!(terminals > 0 && nodes > 0, "{terminals} terminals, {nodes} nodes");
    assert_eq!(past, 0, "a walk crossed a game's end into sideboarding or the next game");
    // the match as one episode (36.6): the same walks do cross into sideboarding (the check above is not vacuous)
    let (past0, _, _) = search_states(BO3);
    assert!(past0 > 0, "the one-episode match's search never crossed a game's end");
}

/// Uniform priors and value 0.5 for every row.
fn answer(d: &mut Driver<MtgAz>, b: &zero_search::driver::Batch) {
    let mut pri = Vec::new();
    for r in 0..b.b {
        let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
        pri.extend(std::iter::repeat_n(1.0 / k as f32, k));
    }
    let mut v = vec![0.0; b.b * MAX_SEATS];
    for r in 0..b.b {
        v[r * MAX_SEATS] = 0.5;
        v[r * MAX_SEATS + 1] = 0.5;
    }
    d.submit(b.id, &pri, &v, &vec![0.0; b.b * MAX_SEATS]).unwrap();
}

fn self_play(json: &str, matches: usize, seed: u64) -> Vec<(GameResult, Vec<Record>)> {
    let mut s = NnSeat::eval(0, 4);
    s.selfplay = true;
    s.temp_moves = 1000;
    let cfg = DriverCfg {
        spec: json.into(),
        seats: vec![SeatCfg::Nn(s)],
        games: vec![vec![0, 0]; matches],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: matches,
        seed,
        record: true,
        threads: 4,
        groups: 2,
        max_steps: 60_000,
        rolling: false,
        net: 0,
        queue_only: false,
        move_log: 0,
        aux: false,
        paired: false,
        sparse: false,
        branch: None,
        starts: None,
        // (layout v7's row limit, the game's own: ENGINE.md 125)
        max_row_tokens: None,
    };
    let mut d = Driver::<MtgAz>::new(cfg).unwrap();
    let mut rows = Vec::new();
    let take = |d: &mut Driver<MtgAz>, rows: &mut Vec<(GameResult, Vec<Record>)>| {
        let rs = d.take_results();
        let smp = d.take_samples();
        let mut a = 0;
        for r in rs {
            let k = r.samples as usize;
            rows.push((r, smp[a..a + k].to_vec()));
            a += k;
        }
        assert_eq!(a, smp.len(), "a row's samples follow it");
    };
    while let Some(b) = d.next_batch(true).unwrap() {
        answer(&mut d, &b);
        take(&mut d, &mut rows);
    }
    take(&mut d, &mut rows);
    rows.sort_by_key(|(r, _)| r.gid);
    rows
}

fn sideboarding_row(s: &Record) -> bool {
    s.x.len() == NUM_FEATURES && s.x[O_MATCH + mtch::SIDEBOARDING] > 0.5
}

#[test]
fn every_row_takes_its_own_games_result() {
    let rows = self_play(BO3_GAMES, 24, 31);
    assert_eq!(rows.len(), 24, "one row per match");
    let (mut case, mut sb_rows) = (0, 0);
    for (r, smp) in &rows {
        assert!(r.over && (2..=3).contains(&r.episodes.len()), "game {}: {} games", r.gid, r.episodes.len());
        let mut wins = [0u8; 2];
        let mut a = 0;
        for (k, e) in r.episodes.iter().enumerate() {
            assert!(e.over);
            let part = &smp[a..a + e.samples as usize];
            a += e.samples as usize;
            for s in part {
                for j in 0..2 {
                    let seat = (s.player as usize + j) % 2;
                    assert_eq!(s.z[j], e.result[seat], "game {} game {}: z is its game's", r.gid, k + 1);
                    assert_eq!(s.sc[j], e.margins[seat]);
                }
                // the sideboarding before game k + 1 is that game's: never among game 1's rows
                if sideboarding_row(s) {
                    assert!(k > 0, "game {}: a sideboarding row among game 1's", r.gid);
                    sb_rows += 1;
                }
            }
            if let Some(w) = (0..2).find(|&p| e.result[p] > 0.75) {
                wins[w] += 1;
            }
        }
        // 36.1: the match ends at two wins or after three games, its result by game wins
        assert!(wins.iter().any(|&w| w >= 2) || r.episodes.len() == 3);
        let want = match wins[0].cmp(&wins[1]) {
            std::cmp::Ordering::Greater => vec![1.0, 0.0],
            std::cmp::Ordering::Less => vec![0.0, 1.0],
            std::cmp::Ordering::Equal => vec![0.5, 0.5],
        };
        assert_eq!(r.result, want);
        // the user's case: lost 1-2 by game 2's winner - its game-2 rows (its sideboarding before game 2 among them)
        // z = win, its game-1 and game-3 rows z = loss
        let winner = |k: usize| (0..2).find(|&p| r.episodes[k].result[p] > 0.75);
        if r.episodes.len() == 3 {
            if let (Some(w1), Some(w2), Some(w3)) = (winner(0), winner(1), winner(2)) {
                if w1 != w2 && w3 == w1 {
                    let p = w2;
                    assert_eq!(r.result[p], 0.0);
                    let (n1, n2) = (r.episodes[0].samples as usize, r.episodes[1].samples as usize);
                    let mine = |part: &[Record]| part.iter().filter(|s| s.player as usize == p).cloned().collect::<Vec<_>>();
                    let (g1, g2, g3) = (mine(&smp[..n1]), mine(&smp[n1..n1 + n2]), mine(&smp[n1 + n2..]));
                    assert!(g1.iter().all(|s| s.z[0] == 0.0) && g3.iter().all(|s| s.z[0] == 0.0));
                    assert!(g2.iter().all(|s| s.z[0] == 1.0));
                    let sb: Vec<&Record> = g2.iter().filter(|s| sideboarding_row(s)).collect();
                    assert!(!sb.is_empty(), "game {}: P{p} lost game 1 and sideboards first before game 2", r.gid);
                    assert!(sb.iter().all(|s| s.z[0] == 1.0));
                    case += 1;
                }
            }
        }
    }
    assert!(sb_rows > 0 && case > 0, "{sb_rows} sideboarding rows, {case} matches lost 1-2 by game 2's winner");
}

/// The Delver mirror's constructed best-of-three with one episode per game (the presets' spec, ENGINE.md 120, 121):
/// the pinned 75 for both seats.
fn constructed_bo3_games() -> String {
    let d = engine::az::constructed::Decklist::izzet_delver();
    let db = engine::db::CardDb::constructed();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().filter(|(_, n)| db.id(n).is_some()).flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}})
    .to_string()
}

/// Layout v7 (ENGINE.md 125.12): a match with one episode per game carries its rows' tails and overrides exactly
/// across the episodes' boundaries, for the cube's best-of-three and the Delver mirror's constructed one. A
/// sideboarding row (the next episode's first rows) holds the frame's earlier reveals (`my_revealed`) as its only
/// tail tokens and no pointer overrides (sideboarding keeps the static table); within an episode a player's
/// `my_revealed` tokens are the same at every row (the earlier games' reveals change only between games: `end_game`),
/// game 1's rows have none, and a later episode's hold the earlier's. No row passed the limit (the match finished).
#[test]
fn v7_rows_carry_their_tails_across_the_episodes() {
    use engine::az::tail_encode::seg;
    for (name, json) in [("cube", BO3_GAMES.to_string()), ("constructed", constructed_bo3_games())] {
        let rows = self_play(&json, 12, 47);
        let (mut sb_rows, mut revealed_rows, mut game_rows) = (0, 0, 0);
        for (r, smp) in &rows {
            assert!(r.over && !r.aborted, "{name} match {}: finished, none aborted", r.gid);
            let mut a = 0;
            let mut before: [std::collections::BTreeSet<u32>; 2] = Default::default();
            for (k, e) in r.episodes.iter().enumerate() {
                let part = &smp[a..a + e.samples as usize];
                a += e.samples as usize;
                let mut mine: [Option<std::collections::BTreeSet<u32>>; 2] = [None, None];
                for s in part {
                    assert_eq!(s.x.len(), NUM_FEATURES);
                    assert_eq!(s.tail.val.len(), s.tail.seg.iter().map(|&g| engine::az::tail_encode::SEG_W[g as usize]).sum::<usize>());
                    let rev: std::collections::BTreeSet<u32> =
                        s.tail.seg.iter().zip(&s.tail.slot).filter(|(&g, _)| g == seg::MY_REVEALED).map(|(_, &l)| l).collect();
                    if sideboarding_row(s) {
                        assert!(k > 0, "{name} match {}: a sideboarding row in game 1", r.gid);
                        assert!(s.tail.seg.iter().all(|&g| g == seg::MY_REVEALED), "{name} match {}: a sideboarding row's tails", r.gid);
                        assert!(s.ov.is_empty(), "{name} match {}: a sideboarding row's overrides", r.gid);
                        sb_rows += 1;
                    } else {
                        game_rows += 1;
                    }
                    let p = s.player as usize;
                    match &mine[p] {
                        Some(m) => assert_eq!(m, &rev, "{name} match {} game {}: P{p}'s earlier reveals changed within a game", r.gid, k + 1),
                        None => mine[p] = Some(rev.clone()),
                    }
                    if k == 0 {
                        assert!(rev.is_empty(), "{name} match {}: earlier reveals in game 1", r.gid);
                    }
                    assert!(before[p].is_subset(&rev), "{name} match {} game {}: P{p} forgot an earlier game's reveal", r.gid, k + 1);
                    revealed_rows += !rev.is_empty() as u32;
                }
                for p in 0..2 {
                    if let Some(m) = mine[p].take() {
                        before[p] = m;
                    }
                }
            }
        }
        println!("{name}: {sb_rows} sideboarding rows, {game_rows} game rows, {revealed_rows} rows with earlier reveals");
        assert!(sb_rows > 0 && revealed_rows > 0, "{name}: {sb_rows} {revealed_rows}");
    }
}
