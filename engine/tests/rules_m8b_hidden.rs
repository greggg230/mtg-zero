//! M8b part b (ENGINE.md 36.11): the hidden pool. In a best-of-three match
//! each player knows the other's pool but their first picks, and not which
//! pool cards are in the deck; `determinize(viewer)` samples the
//! opponent's unknown first picks, deck and sideboard from what the viewer
//! knows. Tests: the first picks drawn from the prior (seen ones kept), the
//! decks consistent with what the viewer knows (sizes, reveals, a revealed
//! companion), the second sideboarder blind to the first's deck, the
//! companion question asked of everyone, the bags, the heuristic bot, and
//! `force_chance` in matches.

use engine::az::bots::{self, Heuristic};
use engine::az::encode::{self, NUM_FEATURES};
use engine::az::{identity, sample, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::hidden::{key_kind, key_player, KEY_DRAW, KEY_LOOK};
use engine::rng::Rng;
use engine::{CardIdx, DecisionKind};
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::cube()
}

fn spec(json: &str) -> MtgSpec {
    MtgSpec::parse(json).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode(viewer, &mut x);
    x
}

fn world(g: &MtgAz, viewer: usize, seed: u64) -> MtgAz {
    let mut d = g.clone();
    d.determinize(viewer, &mut zero_search::rng::Rng::new(seed));
    d
}

/// The heuristic's action (sideboarding: `bots::sideboard`), or a random one.
fn mixed(g: &MtgAz, h: &mut Heuristic, rng: &mut Rng, bot: bool) -> u16 {
    let l = legal(g);
    match g.sideboarding() {
        Some(sb) if bot => bots::sideboard(sb, &l),
        None if bot => h.choose(&g.g, &l),
        _ => l[rng.below(l.len() as u32) as usize],
    }
}

/// States of best-of-three matches (`json`), mixed heuristic / random play,
/// one every ~`every` decisions.
fn states(json: &str, matches: u64, every: u32, seed: u64) -> Vec<MtgAz> {
    let sp = spec(json);
    let mut rng = Rng::new(seed);
    let mut h = Heuristic::new(seed);
    let mut out = Vec::new();
    for s in 0..matches {
        let mut g = MtgAz::new_game(&sp, 2, seed * 1000 + s).unwrap();
        let mut keys = Vec::new();
        while g.to_move().is_some() {
            if rng.below(every) == 0 {
                out.push(g.clone());
            }
            let bot = rng.below(2) == 0;
            let a = mixed(&g, &mut h, &mut rng, bot);
            g.step(a, &mut keys).unwrap();
        }
    }
    out
}

/// The identities (card definitions) of `o`'s starting-deck cards that
/// `viewer` knows exactly in the game, sorted.
fn known_deck_cards(g: &MtgAz, viewer: u8, o: u8) -> Vec<u16> {
    let all = sample::game_cards(g, o);
    let sizes: Vec<usize> = (0..2).map(|p| g.m.decks[p].iter().map(|&c| c as usize).sum()).collect();
    let start = if o == 0 { 0 } else { sizes[0] };
    let mut v: Vec<u16> = (start..start + sizes[o as usize])
        .filter(|&i| g.g.knows(viewer, CardIdx(i as u16)))
        .map(|i| sample::card_of(&g.g, CardIdx(i as u16)))
        .collect();
    v.sort_unstable();
    debug_assert!(v.iter().all(|d| all.contains(d)));
    v
}

fn contains_multiset(big: &[u16], small: &[u16]) -> bool {
    let mut b = big.to_vec();
    for d in small {
        match b.iter().position(|x| x == d) {
            Some(i) => {
                b.remove(i);
            }
            None => return false,
        }
    }
    true
}

#[test]
fn first_picks_are_drawn_from_the_prior_and_seen_ones_kept() {
    // At the first decision of a match nothing is seen: every world draws
    // the opponent's three first picks from the prior (supported cards
    // outside the known part of their pool and outside the viewer's pool),
    // mostly playable in the prior's colours, and they vary.
    let sp = spec(r#"{"format": "bo3"}"#);
    let mut real_kept = 0;
    let mut worlds = 0;
    let mut on_colour = 0;
    for seed in 0..12u64 {
        let g = MtgAz::new_game(&sp, 2, 70 + seed).unwrap();
        let v = g.to_move().unwrap();
        let o = 1 - v;
        let prior = &g.m.priors.as_ref().unwrap()[o];
        let real = g.m.pools[o].first_picks.clone();
        let mut sets = std::collections::BTreeSet::new();
        for w in 0..20 {
            let d = world(&g, v, seed * 100 + w);
            let fp = d.m.pools[o].first_picks.clone();
            assert_eq!(fp.len(), 3);
            for f in &fp {
                assert!(prior.cands.contains(f), "a first pick outside the prior");
                assert!(!g.m.pools[v].list.iter().any(|e| e.0 == *f), "a first pick in the viewer's pool");
                let pool = &g.m.pools[o];
                assert!(
                    !pool.list[..pool.n_cards].iter().any(|e| e.0 == *f) || real.contains(f),
                    "a first pick among the cards known to be in the pool"
                );
                let dd = db().def(*f);
                on_colour += (dd.colors & !prior.pair == 0) as u32;
            }
            real_kept += (fp == real) as u32;
            sets.insert(fp);
            worlds += 1;
        }
        assert!(sets.len() >= 15, "the first picks rarely vary: {}", sets.len());
    }
    assert!(real_kept <= 1, "the real first picks kept in {real_kept} of {worlds} worlds");
    assert!(on_colour * 2 > worlds * 3, "first picks in the prior's colours: {on_colour} of {}", 3 * worlds);

    // A first pick the viewer has seen (revealed in this or an earlier
    // game, or known in the game) stays in every world.
    let mut checked = 0;
    for g in states(r#"{"format": "bo3", "max_turns": 16}"#, 30, 9, 7) {
        let v = g.to_move().unwrap();
        let o = 1 - v;
        let ids = identity::identities(db());
        let known = known_deck_cards(&g, v as u8, o as u8);
        let seen: Vec<u16> = g.m.pools[o]
            .first_picks
            .iter()
            .copied()
            .filter(|&f| g.m.revealed_before(o as u8, ids[f as usize]) || (g.sideboarding().is_none() && known.contains(&f)))
            .collect();
        if seen.is_empty() {
            continue;
        }
        for w in 0..3 {
            let d = world(&g, v, w);
            for f in &seen {
                assert!(d.m.pools[o].first_picks.contains(f), "a seen first pick was resampled");
            }
            checked += 1;
        }
    }
    println!("{checked} worlds with seen first picks");
    assert!(checked > 30, "{checked}");
}

#[test]
fn sampled_decks_hold_what_the_viewer_knows() {
    // Each world's opponent deck: the public sizes (cards, pool cards,
    // basic lands, sideboard; 40 cards, the sampled pool's bounds 40..=40,
    // and the first sideboarder's sampled choice 40 too: ENGINE.md 42),
    // every card of theirs the viewer knows in the game, a legal deck of the
    // sampled pool; between games the last deck holds what the viewer knew
    // at the end of the last game. The decks vary between worlds.
    let mut checked = 0;
    let mut differ = 0;
    let mut between = 0;
    for g in states(r#"{"format": "bo3", "max_turns": 20}"#, 30, 7, 11) {
        let v = g.to_move().unwrap();
        let o = 1 - v;
        let known = known_deck_cards(&g, v as u8, o as u8);
        let size = |c: &[u8]| c.iter().map(|&n| n as usize).sum::<usize>();
        let pool_cards = |a: &MtgAz| {
            let p = &a.m.pools[o];
            size(&a.m.decks[o][..p.n_cards])
        };
        for w in 0..2 {
            let d = world(&g, v, 1000 + w);
            assert_eq!(size(&d.m.decks[o]), size(&g.m.decks[o]));
            assert_eq!(size(&d.m.decks[o]), 40, "a pool's deck is 40 cards");
            assert_eq!((d.m.pools[o].min_deck, d.m.pools[o].max_deck), (40, 40));
            if let Some(chosen) = d.sideboarding().and_then(|sb| sb.done[o].clone()) {
                assert_eq!(size(&chosen), 40);
            }
            assert_eq!(pool_cards(&d), pool_cards(&g), "the number of pool cards in the deck is public");
            assert_eq!(d.g.player(o as u8).sideboard.len(), g.g.player(o as u8).sideboard.len());
            let deck = d.m.pools[o].deck(&d.m.decks[o]);
            assert!(contains_multiset(&deck, &known), "the sampled deck lacks a card the viewer knows");
            if g.sideboarding().is_none() {
                let mut want = deck.clone();
                want.extend(d.g.player(o as u8).companion_def);
                want.sort_unstable();
                assert_eq!(sample::game_cards(&d, o as u8), want);
            } else {
                between += 1;
            }
            differ += (deck != g.m.pools[o].deck(&g.m.decks[o])) as u32;
            checked += 1;
        }
    }
    println!("{checked} worlds ({between} between games), {differ} with another deck");
    assert!(checked > 300 && between > 10, "{checked} {between}");
    assert!(differ * 10 > checked * 9, "{differ}/{checked}");
}

#[test]
fn the_second_sideboarder_cannot_see_the_first_players_deck() {
    // Two matches that differ only in the deck the first sideboarder chose:
    // the second sideboarder's worlds are the same (its tree is not
    // conditioned on the choice), and the chosen deck is resampled: the
    // first player's sampled last deck (the prior: kept).
    let sp = spec(r#"{"format": "bo3", "max_turns": 2}"#);
    let mut tested = 0;
    for seed in 0..10u64 {
        let mut g = MtgAz::new_game(&sp, 2, 30 + seed).unwrap();
        let mut keys = Vec::new();
        let mut rng = Rng::new(seed);
        while g.sideboarding().is_none() && !g.match_over() {
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
        if g.match_over() {
            continue;
        }
        let first = g.to_move().unwrap();
        let second = 1 - first;
        let pool = g.m.pools[first].clone();
        let mut h = g.clone();
        // g: the first player keeps; h: swaps two single cards.
        g.step(act::PASS, &mut keys).unwrap();
        let singles: Vec<u16> = legal(&h).into_iter().filter(|&a| a >= act::LIBRARY && ((a - act::LIBRARY) as usize) < pool.n_cards).take(2).collect();
        for a in singles {
            if legal(&h).contains(&a) {
                h.step(a, &mut keys).unwrap();
            }
        }
        while h.to_move() == Some(first) {
            let l = legal(&h);
            h.step(if l.contains(&act::PASS) { act::PASS } else { l[0] }, &mut keys).unwrap();
        }
        assert_eq!(g.to_move(), Some(second));
        let (dg, dh) = (g.sideboarding().unwrap().done[first].clone(), h.sideboarding().unwrap().done[first].clone());
        assert_ne!(dg, dh);
        for w in 0..4 {
            let (a, b) = (world(&g, second, w), world(&h, second, w));
            assert_eq!(a.m.pools[first], b.m.pools[first]);
            assert_eq!(a.m.decks[first], b.m.decks[first]);
            let (da, db_) = (a.sideboarding().unwrap().done[first].clone(), b.sideboarding().unwrap().done[first].clone());
            assert_eq!(da, db_, "the second sideboarder's world depends on the first's choice");
            assert_eq!(da.as_deref(), Some(&a.m.decks[first][..]), "the chosen deck: the sampled last deck");
            assert_eq!(legal(&a), legal(&g));
            assert_eq!(enc(&a, second), enc(&b, second));
            // Both worlds play on into the next game.
            let mut kk = Vec::new();
            let mut a2 = a.clone();
            while a2.sideboarding().is_some() {
                let l = legal(&a2);
                a2.step(if l.contains(&act::PASS) { act::PASS } else { l[0] }, &mut kk).unwrap();
            }
            a2.g.check_invariants().unwrap();
            tested += 1;
        }
    }
    assert!(tested >= 20, "{tested}");
}

#[test]
fn the_companion_question_is_asked_of_everyone_in_matches() {
    // Best-of-three: every player is asked in every game (`PASS` only when
    // their deck fulfills no sideboard companion: never answered
    // internally), so whether they are asked says nothing about their deck.
    // A revealed companion's condition holds in every sampled deck.
    let sp = spec(r#"{"format": "bo3", "max_turns": 10}"#);
    let (mut asked, mut games, mut pass_only, mut revealed_worlds) = (0, 0, 0, 0);
    for seed in 0..40u64 {
        let mut g = MtgAz::new_game(&sp, 2, 500 + seed).unwrap();
        let mut rng = Rng::new(seed);
        let mut h = Heuristic::new(seed);
        let mut keys = Vec::new();
        let mut game = 0;
        while g.to_move().is_some() {
            if g.m.game_no != game && g.sideboarding().is_none() {
                game = g.m.game_no;
                games += 1;
            }
            if let Some(d) = g.pending_decision().filter(|d| d.kind == DecisionKind::RevealCompanion) {
                asked += 1;
                pass_only += (d.options.len() == 1) as u32;
            }
            if g.sideboarding().is_none() && rng.below(25) == 0 {
                let v = g.to_move().unwrap();
                if let Some(c) = g.g.player(1 - v as u8).companion_def {
                    let cond = db().def(c).companion.as_ref().unwrap();
                    for w in 0..3 {
                        let d = world(&g, v, w);
                        let deck = d.m.pools[1 - v].deck(&d.m.decks[1 - v]);
                        assert!(db().deck_satisfies(cond, &deck), "a sampled deck breaks the revealed companion");
                        assert!(!deck.contains(&c));
                        revealed_worlds += 1;
                    }
                }
            }
            let a = mixed(&g, &mut h, &mut rng, true);
            g.step(a, &mut keys).unwrap();
        }
    }
    println!("{games} games, {asked} companion questions ({pass_only} with PASS only), {revealed_worlds} worlds after a reveal");
    assert_eq!(asked, 2 * games);
    assert!(pass_only > 0 && pass_only < asked);
    assert!(revealed_worlds > 10, "{revealed_worlds}");
    // Best-of-one keeps M8a's question: asked only with a candidate.
    let mut g = MtgAz::new_game(&spec(r#"{"pools": true}"#), 2, 3).unwrap();
    let mut keys = Vec::new();
    while g.g.turn().number == 0 && g.to_move().is_some() {
        if let Some(d) = g.pending_decision().filter(|d| d.kind == DecisionKind::RevealCompanion) {
            assert!(d.options.len() > 1);
        }
        let l = legal(&g);
        g.step(l[0], &mut keys).unwrap();
    }
}

#[test]
fn the_bags_show_what_the_viewer_knows_of_the_pool() {
    // `OPP_POOL`: the known part (no unseen first pick); `OPP_HIDDEN`: its
    // candidates, never an unseen first pick, never more than the known
    // part; both, and every feature, the same in every world of the viewer
    // (both viewers; sideboarding too).
    let ids = identity::identities(db());
    let mut checked = 0;
    for g in states(r#"{"format": "bo3", "max_turns": 20}"#, 20, 9, 13) {
        let mover = g.to_move().unwrap();
        for v in [mover, 1 - mover] {
            let o = 1 - v;
            let x = enc(&g, v);
            let bag = |b: usize| &x[encode::O_BAGS + b * encode::D..][..encode::D];
            let pool = &g.m.pools[o];
            for f in &pool.first_picks {
                if !g.m.revealed_before(o as u8, ids[*f as usize]) {
                    let i = ids[*f as usize] as usize;
                    assert_eq!(bag(encode::bag::OPP_POOL)[i], 0.0, "an unseen first pick in OPP_POOL");
                    assert_eq!(bag(encode::bag::OPP_HIDDEN)[i], 0.0, "an unseen first pick in OPP_HIDDEN");
                }
            }
            for i in 0..encode::D {
                assert!(bag(encode::bag::OPP_HIDDEN)[i] <= bag(encode::bag::OPP_POOL)[i]);
            }
            for w in 0..2 {
                let d = world(&g, v, 77 + w);
                let y = enc(&d, v);
                if let Some(i) = (0..NUM_FEATURES).find(|&i| x[i] != y[i]) {
                    panic!("features of P{v} changed at {i} ({} -> {}); mover P{mover}", x[i], y[i]);
                }
                checked += 1;
            }
        }
    }
    println!("{checked} worlds");
    assert!(checked > 300, "{checked}");
}

#[test]
fn the_heuristic_reads_no_hidden_card() {
    // The bot's choice is the same in the real state and in the decider's
    // worlds (the opponent's hand, library, deck, sideboard and first picks
    // resampled, its own library order too): it reads only what the decider
    // knows (ENGINE.md 36.10, item 7).
    let mut checked = 0;
    for g in states(r#"{"format": "bo3", "max_turns": 24}"#, 24, 5, 17) {
        let p = g.to_move().unwrap();
        let l = legal(&g);
        if l.len() < 2 {
            continue;
        }
        let want = match g.sideboarding() {
            Some(sb) => bots::sideboard(sb, &l),
            None => Heuristic::new(5).choose(&g.g, &l),
        };
        for w in 0..3 {
            let d = world(&g, p, 300 + w);
            let got = match d.sideboarding() {
                Some(sb) => bots::sideboard(sb, &l),
                None => Heuristic::new(5).choose(&d.g, &l),
            };
            assert_eq!(got, want, "the heuristic chose differently in a world of its decider: {:?}", g.pending_decision().map(|d| d.kind));
            checked += 1;
        }
    }
    println!("{checked} decisions compared");
    assert!(checked > 500, "{checked}");
}

#[test]
fn force_chance_replays_the_deciders_draws_in_matches() {
    // A transition's keys sampled in one world of the decider, forced in
    // another (each with its own sampled opponent deck): the decider's own
    // draws and looks are reproduced when that world allows it.
    let mut rng = Rng::new(5);
    let (mut tried, mut hits) = (0, 0);
    for g in states(r#"{"format": "bo3", "max_turns": 30}"#, 30, 6, 19) {
        if g.sideboarding().is_some() {
            continue;
        }
        let v = g.to_move().unwrap();
        let mut a = world(&g, v, rng.next_u64());
        let l = legal(&a);
        let x = l[rng.below(l.len() as u32) as usize];
        let mut ka = Vec::new();
        a.step(x, &mut ka).unwrap();
        let own = |k: &u64| (key_kind(*k) == KEY_DRAW || key_kind(*k) == KEY_LOOK) && key_player(*k) == v as u8;
        if !ka.iter().any(own) {
            continue;
        }
        for _ in 0..3 {
            let mut b = world(&g, v, rng.next_u64());
            b.force_chance(&ka);
            let mut kb = Vec::new();
            b.step(x, &mut kb).unwrap();
            b.force_chance(&[]);
            if b.sideboarding().is_none() && !b.match_over() {
                b.g.check_invariants().unwrap();
            }
            tried += 1;
            if kb == ka {
                hits += 1;
                assert_eq!(enc(&a, v), enc(&b, v), "forced to the same keys, different features");
            }
        }
    }
    println!("forced {hits}/{tried}");
    assert!(tried > 100, "{tried}");
    assert!(hits * 10 > tried * 7, "forced {hits}/{tried}");
}

#[test]
fn a_deck_of_basic_lands_up_to_the_cap_is_sampled() {
    // One player sideboards every pool card out and basic lands in, as many
    // of the first type as it can: 40 basic lands, 31 of a type (the count
    // cap; ENGINE.md 42: the deck stays at 40 cards). (runs/mtg4's network
    // built a 60-card deck with 34 basic lands when decks could grow to 60,
    // and the other player's determinize then found no deck: the basic
    // lands' spread over the deck's colours stopped at one type's count
    // cap.) Every world of the other player holds a deck of the real size.
    let sp = spec(r#"{"format": "bo3", "max_turns": 4}"#);
    let mut tested = 0;
    for seed in 0..16u64 {
        let mut g = MtgAz::new_game(&sp, 2, 90 + seed).unwrap();
        let mut keys = Vec::new();
        let mut rng = Rng::new(seed);
        while g.sideboarding().is_none() && !g.match_over() {
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
        if g.match_over() {
            continue;
        }
        let stuffer = g.to_move().unwrap();
        let other = 1 - stuffer;
        let basic0 = g.m.pools[stuffer].list.len() - 5;
        while g.to_move() == Some(stuffer) && g.sideboarding().is_some() {
            let l = legal(&g);
            let sb = g.sideboarding().unwrap();
            let a = if sb.decision.kind == DecisionKind::SideboardCount {
                let k = sb.asking.unwrap() as usize;
                let nums = l.iter().copied().filter(|&a| a >= act::NUMBER);
                if k >= basic0 { nums.max() } else { nums.min() }.unwrap()
            } else {
                l.iter()
                    .copied()
                    .find(|&a| {
                        a >= act::LIBRARY && {
                            let k = (a - act::LIBRARY) as usize;
                            k >= basic0 || sb.counts[k] > 0
                        }
                    })
                    .unwrap_or(if l.contains(&act::PASS) { act::PASS } else { l[0] })
            };
            g.step(a, &mut keys).unwrap();
        }
        let deck: Vec<u8> = g.sideboarding().map_or_else(|| g.m.decks[stuffer].to_vec(), |sb| sb.done[stuffer].as_ref().map_or(Vec::new(), |d| d.to_vec()));
        let size: usize = deck.iter().map(|&c| c as usize).sum();
        assert_eq!(size, 40, "the stuffed deck has {size} cards");
        assert!(deck[..basic0].iter().all(|&c| c == 0), "no pool card");
        assert_eq!(deck[basic0], 31, "31 of the first basic land type");
        // The other player sideboards (keeps), then game 2: the other
        // player's worlds at several points.
        let mut steps = 0;
        while !g.match_over() && steps < 400 {
            if g.to_move() == Some(other) && steps % 10 == 0 {
                for w in 0..6 {
                    let d = world(&g, other, 1000 * seed + w + steps as u64);
                    let n: usize = d.m.decks[stuffer].iter().map(|&c| c as usize).sum();
                    assert_eq!(n, g.m.decks[stuffer].iter().map(|&c| c as usize).sum::<usize>());
                    d.g.check_invariants().unwrap();
                    tested += 1;
                }
            }
            let l = legal(&g);
            let a = if g.sideboarding().is_some() && l.contains(&act::PASS) { act::PASS } else { l[rng.below(l.len() as u32) as usize] };
            g.step(a, &mut keys).unwrap();
            steps += 1;
        }
    }
    assert!(tested >= 200, "{tested}");
}
