//! Worlds of a match that start a new game, and the keep-its-deck yardstick's informed arm (ENGINE.md 141).
//!
//! * **A world keeps its observer in the games it starts.** A world `determinize` deals while sideboarding, during
//!   the sealed deck choice, or in a game it then plays past the end of (a best-of-three without one episode per
//!   game), keys the other player's private draws and looks of the next game as hidden (`Game::emit_private`), as
//!   a world dealt in a game does. At 267ae603 the new `Game` of `next_game` / `start_chosen_game` had no observer
//!   (the Fable review's H2): every one of those keys carried the card's identity in the viewer's tree.
//! * **`"keep_known": true`**: in the other seat's worlds a seat of `"keep_deck"` keeps its deck - the registered
//!   main, never dealt (its prior knows its policy: a point mass) - so `keep_h2h`'s side A searches against the deck
//!   side B plays. Without the key (ENGINE.md 130.3) A's worlds deal B's 60 from the constructed prior: the main in
//!   about one world in sixteen.

use engine::az::constructed::Decklist;
use engine::az::{MtgAz, MtgSpec};
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DRAW, KEY_LOOK};
use engine::rng::Rng;
use zero_search::game::{Next, SearchGame};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// The encoding of `viewer`, bit for bit (its whole row, layout v7).
fn enc(g: &MtgAz, viewer: usize) -> Vec<u32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat().iter().map(|f| f.to_bits()).collect()
}

/// The cards player `p` started the current game with (its deck, by the card each is), sorted.
fn game_deck(a: &MtgAz, p: u8) -> Vec<u16> {
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if p == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| engine::az::sample::card_of(&a.g, engine::CardIdx::new(i as u32))).collect();
    v.sort_unstable();
    v
}

/// The state after a game's end with one episode per game: the next episode's (None: the match is over).
fn next(g: &MtgAz) -> Option<MtgAz> {
    if !g.m.between {
        return None;
    }
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

/// The other player's private keys (draws and looks) in `keys`, as `viewer` sees them: (masked, with the card's
/// identity); and the viewer's own private keys with an identity (a world masks only the other player's).
fn private(keys: &[u64], viewer: u8) -> (usize, usize, usize) {
    let (mut masked, mut open, mut own) = (0, 0, 0);
    for &k in keys {
        if !matches!(key_kind(k), KEY_DRAW | KEY_LOOK) {
            continue;
        }
        match (key_player(k) == viewer, key_payload(k) == HIDDEN) {
            (false, true) => masked += 1,
            (false, false) => open += 1,
            (true, false) => own += 1,
            (true, true) => panic!("the viewer's own private key masked"),
        }
    }
    (masked, open, own)
}

#[derive(Default, Debug)]
struct Walks {
    worlds: usize,
    started: usize,
    masked: usize,
    open: usize,
    own: usize,
}

/// `w` (a world of `viewer`) walked as the search walks it: a sideboarding or a deck choice to its end (`PASS`
/// where it is legal, else a random option), past a game's end into the sideboarding and the next game, then 60
/// actions of the game the world started; at most `cap` actions in all. The keys of the game it started counted.
fn walk(mut w: MtgAz, viewer: u8, cap: usize, rng: &mut Rng, out: &mut Walks) {
    let between = |x: &MtgAz| x.sideboarding().is_some() || x.deck_choice().is_some();
    let (mut started, mut since, mut keys) = (false, 0usize, Vec::new());
    out.worlds += 1;
    for _ in 0..cap {
        if w.to_move().is_none() || since >= 60 {
            break;
        }
        let l = legal(&w);
        let before = between(&w);
        let a = if before && l[0] == act::PASS { act::PASS } else { l[rng.below(l.len() as u32) as usize] };
        keys.clear();
        w.step(a, &mut keys).unwrap();
        if before && !between(&w) && !w.match_over() {
            started = true;
            out.started += 1;
        } else if started {
            since += 1;
            let (m, o, n) = private(&keys, viewer);
            out.masked += m;
            out.open += o;
            out.own += n;
        }
    }
}

/// The Delver mirror's constructed best-of-three (both seats the pinned 75, ENGINE.md 120), one episode per game,
/// and the keys of `extra`.
fn delver_json(extra: serde_json::Value) -> String {
    let d = Decklist::izzet_delver();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let mut v = serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v.to_string()
}

/// Worlds of both seats dealt at the first sideboarding decision of `seeds` matches of `json` (random play to it),
/// each walked into the next game.
fn sideboarding_worlds(json: &str, seeds: u64) -> Walks {
    let spec = MtgSpec::parse(json).unwrap();
    let mut out = Walks::default();
    for seed in 0..seeds {
        let mut g = MtgAz::new(&spec, seed).unwrap();
        let mut rng = Rng::new(seed ^ 0x5B0A);
        let mut k = Vec::new();
        let mut steps = 0;
        while g.sideboarding().is_none() && steps < 50_000 {
            if g.to_move().is_none() {
                match next(&g) {
                    Some(n) => {
                        g = n;
                        continue;
                    }
                    None => break,
                }
            }
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
            steps += 1;
        }
        if g.sideboarding().is_none() {
            continue;
        }
        for v in [0u8, 1] {
            let mut w = g.clone();
            w.determinize(v as usize, &mut zero_search::rng::Rng::new(seed ^ 0xD5 ^ (v as u64) << 9));
            walk(w, v, 600, &mut rng, &mut out);
        }
    }
    out
}

/// CR 103 / ENGINE.md 10, 141: a world dealt while sideboarding (the root of every game 2 and 3 of the Delver
/// best-of-three presets) plays the next game with its observer - the other player's opening hand, mulligan draws
/// and later draws and looks keyed as hidden, the viewer's own with their identities. At 267ae603 every such world
/// keyed them by identity (`next_game` built the game with no observer).
#[test]
fn a_world_dealt_while_sideboarding_masks_the_next_games_private_keys() {
    for (label, json) in [
        ("cube bo3", r#"{"max_turns": 12, "format": "bo3"}"#.to_string()),
        ("cube bo3, one episode per game", r#"{"max_turns": 12, "format": "bo3", "game_episodes": true}"#.to_string()),
        ("Delver bo3, post-sideboard games", delver_json(serde_json::json!({"start_game": 2}))),
    ] {
        let w = sideboarding_worlds(&json, scale(16));
        println!("{label}: {w:?}");
        assert!(w.started * 10 >= w.worlds * 9, "{label}: the walks did not reach the next game: {w:?}");
        assert!(w.masked > 0 && w.own > 0, "{label}: no private keys in the next games: {w:?}");
        assert_eq!(w.open, 0, "{label}: the other player's private keys carried identities in a world's next game: {w:?}");
    }
}

/// The same for a world dealt in a game that it plays past the game's end (a best-of-three without one episode
/// per game: the search crosses into the sideboarding and the next game).
#[test]
fn a_world_dealt_in_a_game_masks_the_private_keys_past_its_end() {
    let spec = MtgSpec::parse(r#"{"max_turns": 6, "format": "bo3"}"#).unwrap();
    let mut out = Walks::default();
    for seed in 0..scale(24) {
        let mut g = MtgAz::new(&spec, seed).unwrap();
        let mut rng = Rng::new(seed ^ 0x6A3E);
        let mut k = Vec::new();
        // to the last turn of game 1
        let mut steps = 0;
        while g.to_move().is_some() && g.sideboarding().is_none() && g.g.turn().number < 6 && steps < 20_000 {
            steps += 1;
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
        }
        if g.to_move().is_none() || g.sideboarding().is_some() {
            continue;
        }
        let v = g.to_move().unwrap() as u8;
        let mut w = g.clone();
        w.determinize(v as usize, &mut zero_search::rng::Rng::new(seed ^ 0x77));
        walk(w, v, 5_000, &mut rng, &mut out);
    }
    println!("cube bo3, worlds dealt in game 1's last turn: {out:?}");
    assert!(out.started * 2 >= out.worlds && out.masked > 0 && out.own > 0, "{out:?}");
    assert_eq!(out.open, 0, "the other player's private keys carried identities past the world's game: {out:?}");
}

/// The same for a world dealt during the sealed deck choice (ENGINE.md 107.2): the game the choice starts
/// (`start_chosen_game`) keeps the world's observer.
#[test]
fn a_world_dealt_during_the_deck_choice_masks_its_games_private_keys() {
    let spec = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper", "deck_choice": {"k": 4, "explore": 0.0}}, "max_turns": 30}"#).unwrap();
    let mut out = Walks::default();
    for seed in 0..scale(4) {
        let a = MtgAz::new_game(&spec, 2, seed).unwrap();
        assert!(a.deck_choice().is_some());
        let mut rng = Rng::new(seed ^ 0xC401);
        for v in [0u8, 1] {
            let mut w = a.clone();
            w.determinize(v as usize, &mut zero_search::rng::Rng::new(seed ^ 0xDC ^ (v as u64) << 7));
            walk(w, v, 400, &mut rng, &mut out);
        }
    }
    println!("sealed deck choice: {out:?}");
    assert!(out.started == out.worlds && out.masked > 0 && out.own > 0, "{out:?}");
    assert_eq!(out.open, 0, "the other player's private keys carried identities in the game a deck-choice world started: {out:?}");
}

/// `"keep_known"`: a setting of a constructed best-of-three with a keeper, refused without one; the evaluation spec
/// keeps it.
#[test]
fn keep_known_is_a_setting_of_the_keep_deck_yardstick() {
    let s = MtgSpec::parse(&delver_json(serde_json::json!({"start_game": 2, "keep_deck": [1], "keep_known": true}))).unwrap();
    assert!(s.keep_known && s.keep_deck == [false, true]);
    assert!(!MtgSpec::parse(&delver_json(serde_json::json!({"start_game": 2, "keep_deck": [1]}))).unwrap().keep_known);
    let e = MtgSpec::parse(&delver_json(serde_json::json!({"start_game": 2, "keep_known": true}))).unwrap_err();
    assert!(e.contains("keep_known needs keep_deck"), "{e}");
    let e = MtgSpec::parse(&delver_json(serde_json::json!({"keep_deck": [0], "keep_known": 1}))).unwrap_err();
    assert!(e.contains("keep_known must be a boolean"), "{e}");
    let j = MtgSpec::eval_json(&delver_json(serde_json::json!({"start_game": 2, "keep_deck": [0], "keep_known": true}))).unwrap();
    assert!(MtgSpec::parse(&j).unwrap().keep_known);
}

/// ENGINE.md 141 (130.3's yardstick): with `"keep_known"` every world of the other seat holds the keeper's real
/// deck, the registered main (in a game its cards are the main's; between games its last deck and its choice are
/// the main and its sideboarding in progress only passes), where the yardstick's old arm dealt it from the
/// constructed prior; the other seat's encodings and legal actions are the same in both arms at every step (the
/// setting changes only what its worlds deal), and the keeper's own worlds are the same in both arms.
#[test]
fn a_known_keeper_is_dealt_its_main_in_the_other_seats_worlds() {
    let main = {
        let db = engine::db::CardDb::constructed();
        let mut m = Decklist::ids(db, &Decklist::izzet_delver().main).unwrap();
        m.sort_unstable();
        m
    };
    let (mut worlds, mut informed_main, mut plain_main, mut keeper_sb_worlds, mut keeper_worlds) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut fresh, mut fresh_main) = (0u64, 0u64);
    for seed in 0..scale(12) {
        let k = (seed % 2) as usize;
        let o = 1 - k;
        let informed = MtgSpec::parse(&delver_json(serde_json::json!({"start_game": 2, "keep_deck": [k], "keep_known": true}))).unwrap();
        let plain = MtgSpec::parse(&delver_json(serde_json::json!({"start_game": 2, "keep_deck": [k]}))).unwrap();
        let (mut a, mut b) = (MtgAz::new(&informed, seed).unwrap(), MtgAz::new(&plain, seed).unwrap());
        let mut rng = Rng::new(seed ^ 0x4EE9);
        let mut step = 0usize;
        loop {
            if a.to_move().is_none() {
                assert!(b.to_move().is_none());
                match (next(&a), next(&b)) {
                    (Some(x), Some(y)) => {
                        a = x;
                        b = y;
                        continue;
                    }
                    (None, None) => break,
                    _ => panic!("seed {seed}: the arms' matches diverged"),
                }
            }
            let p = a.to_move().unwrap();
            assert_eq!((b.to_move(), legal(&a)), (Some(p), legal(&b)), "seed {seed} step {step}: the arms' real matches");
            assert_eq!(enc(&a, o), enc(&b, o), "seed {seed} step {step}: the other seat's encoding differs between the arms");
            if step % 4 == 0 || a.sideboarding().is_some() {
                let s = seed ^ (step as u64) << 20;
                // the other seat's worlds
                let (mut x, mut y) = (a.clone(), b.clone());
                x.determinize(o, &mut zero_search::rng::Rng::new(s));
                y.determinize(o, &mut zero_search::rng::Rng::new(s));
                for (w, real) in [(&x, &a), (&y, &b)] {
                    assert_eq!((w.to_move(), enc(w, o)), (real.to_move(), enc(real, o)), "seed {seed} step {step}: a world of the other seat");
                    if p == o {
                        assert_eq!(legal(w), legal(real));
                    }
                }
                worlds += 1;
                assert_eq!(x.m.deck(k as u8), main, "seed {seed} step {step}: a known keeper's deck in the other seat's world");
                assert!(x.m.keep[k], "seed {seed}: the known keeper keeps in the other seat's world");
                informed_main += 1;
                plain_main += (y.m.deck(k as u8) == main) as u64;
                // (the old arm's deal with nothing shown: the keeper's choice for game 2, dealt from the prior)
                if let Some(d) = y.sideboarding().filter(|_| y.m.game_no == 1).and_then(|sb| sb.done[k].clone()) {
                    fresh += 1;
                    fresh_main += (y.m.pools[k].deck(&d) == main) as u64;
                }
                if let Some(sb) = x.sideboarding() {
                    if let Some(d) = &sb.done[k] {
                        assert_eq!(x.m.pools[k].deck(d), main, "seed {seed}: the known keeper's choice");
                    }
                    if sb.decider() == k as u8 {
                        assert_eq!(legal(&x), vec![act::PASS], "seed {seed}: the known keeper only passes in the other seat's world");
                        keeper_sb_worlds += 1;
                    }
                } else {
                    assert_eq!(game_deck(&x, k as u8), main, "seed {seed} step {step}: the known keeper's cards in the other seat's world");
                }
                // the keeper's own worlds: the same in both arms
                let (mut x, mut y) = (a.clone(), b.clone());
                x.determinize(k, &mut zero_search::rng::Rng::new(s ^ 1));
                y.determinize(k, &mut zero_search::rng::Rng::new(s ^ 1));
                assert_eq!((&x.m.decks, &x.m.pools), (&y.m.decks, &y.m.pools), "seed {seed}: the keeper's worlds");
                if x.sideboarding().is_none() {
                    assert_eq!(x.g.fingerprint(), y.g.fingerprint(), "seed {seed}: the keeper's worlds");
                }
                keeper_worlds += 1;
            }
            let l = legal(&a);
            let c = if a.sideboarding().is_some() && p == k { act::PASS } else { l[rng.below(l.len() as u32) as usize] };
            let (mut k1, mut k2) = (Vec::new(), Vec::new());
            a.step(c, &mut k1).unwrap();
            b.step(c, &mut k2).unwrap();
            assert_eq!(k1, k2);
            step += 1;
        }
    }
    println!(
        "{worlds} worlds of the other seat: the keeper's real 60 (the main) in {informed_main} with keep_known, in {plain_main} without \
         (of the {fresh} worlds that dealt the keeper's choice for game 2 with nothing shown: {fresh_main}); \
         {keeper_sb_worlds} of them at the keeper's sideboarding; {keeper_worlds} keeper's worlds the same in both arms"
    );
    assert!(worlds > 0 && informed_main == worlds && keeper_sb_worlds > 0 && fresh > 0);
    assert!(plain_main * 2 < worlds, "the old arm dealt the keeper's main in {plain_main} of {worlds} worlds");
}
