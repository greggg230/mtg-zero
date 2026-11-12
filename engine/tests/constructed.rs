//! Constructed play (ENGINE.md 54): the pinned Izzet Delver 75 and its
//! provenance, the deck rules of CR 100.2a / 100.4a, the `"constructed"` spec,
//! the card table of constructed play kept apart from the cube's (the legacy
//! cards' identities 546.., the cube's definitions unchanged), and the game
//! with 60-card decks: the play-or-draw choice, the London mulligan, the turn
//! cap, open decklists in `determinize`, the heuristic in the mirror.

use engine::az::bots::Heuristic;
use engine::az::constructed::{self, Decklist};
use engine::az::encode::NUM_FEATURES;
use engine::az::identity;
use engine::az::{DeckMode, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::rng::Rng;
use engine::{DecisionKind, Zone};
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn delver() -> Decklist {
    Decklist::izzet_delver()
}

/// The Delver mirror's spec (both seats the pinned 60, best-of-one).
fn mirror_json(max_turns: u32) -> String {
    let names = delver().main_names();
    serde_json::json!({"max_turns": max_turns, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string()
}

fn mirror(max_turns: u32) -> MtgSpec {
    MtgSpec::parse(&mirror_json(max_turns)).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

#[test]
fn the_pinned_75_and_its_provenance() {
    let d = delver();
    assert_eq!(d.name, "Izzet Delver");
    assert_eq!((d.player.as_str(), d.event.as_str(), d.date.as_str()), ("i_b_TRUE", "Legacy Challenge 32", "2026-10-23"));
    assert_eq!(d.source, "https://www.mtggoldfish.com/deck/8016541");
    assert_eq!(d.main.iter().map(|e| e.0).sum::<u32>(), 60);
    assert_eq!(d.sideboard.iter().map(|e| e.0).sum::<u32>(), 15);
    // The JSON is the MTGO export verbatim, but for the card table's names.
    let (main, side) = d.parse_export(constructed::IZZET_DELVER_EXPORT).unwrap();
    assert_eq!(main, d.main);
    assert_eq!(side, d.sideboard);
    // Every maindeck card is supported in constructed play; the sideboard's
    // five cards outside the cube are not (yet: ENGINE.md 54.7).
    let ids = Decklist::ids(db(), &d.main).unwrap();
    assert_eq!(ids.len(), 60);
    constructed::check(db(), &ids, &[]).unwrap();
    assert_eq!(
        d.unsupported_sideboard(db()),
        ["Consign to Memory", "Hydroblast", "Meltdown", "Price of Progress", "Rough // Tumble"]
    );
}

#[test]
fn cr_100_2a_and_100_4a_constructed_deck_rules() {
    let db = db();
    let id = |n: &str| db.expect_id(n).unwrap();
    let main = Decklist::ids(db, &delver().main).unwrap();
    constructed::check(db, &main, &[]).unwrap();
    // At least 60 cards.
    assert!(constructed::check(db, &main[..59], &[]).unwrap_err().contains("at least 60"));
    // More than 60 is legal, up to the engine's 64 (a hand, a library list, a
    // graveyard each have 64 action slots); 65 is refused, not played.
    let mut more = main.clone();
    more.push(id("Island"));
    constructed::check(db, &more, &[]).unwrap();
    assert_eq!(constructed::MAX_DECK, 64);
    let mut most = main.clone();
    most.extend(std::iter::repeat_n(id("Island"), constructed::MAX_DECK - main.len()));
    constructed::check(db, &most, &[]).unwrap();
    most.push(id("Island"));
    assert!(constructed::check(db, &most, &[]).unwrap_err().contains("at most 64"));
    // At most four of a card but basic lands: a fifth Brainstorm in the deck
    // or in the sideboard (the limit is over both, 100.4a).
    let mut five = main.clone();
    let island = five.iter().position(|&x| x == id("Island")).unwrap();
    five[island] = id("Brainstorm");
    let five_bs = five.iter().filter(|&&x| x == id("Brainstorm")).count();
    assert_eq!(five_bs, 5);
    assert!(constructed::check(db, &five, &[]).unwrap_err().contains("Brainstorm"));
    assert!(constructed::check(db, &main, &[id("Brainstorm")]).unwrap_err().contains("5 copies of Brainstorm"));
    // Two Unholy Heat in the deck and one in the sideboard: three.
    constructed::check(db, &main, &[id("Unholy Heat")]).unwrap();
    // Basic lands: any number (up to the engine's 64 cards).
    let mut islands = main.clone();
    for c in islands.iter_mut().filter(|c| db.def(**c).types & engine::db::types::LAND != 0) {
        *c = id("Island");
    }
    assert!(islands.iter().filter(|&&c| c == id("Island")).count() > 4);
    constructed::check(db, &islands, &[]).unwrap();
    // A sideboard of at most 15.
    assert!(constructed::check(db, &main, &[id("Island"); 16]).unwrap_err().contains("at most 15"));
    constructed::check(db, &main, &[id("Island"); 15]).unwrap();
    // A token is not a card.
    let t = db.token_def(0);
    assert!(constructed::check(db, &[main.clone(), vec![t]].concat(), &[]).is_err());
}

#[test]
fn the_constructed_spec() {
    let s = mirror(60);
    assert!(s.constructed);
    assert!(std::ptr::eq(s.db(), CardDb::constructed()));
    match &s.decks {
        DeckMode::Lists(a, b) => {
            assert_eq!(a.len(), 60);
            assert_eq!(a, b);
        }
        other => panic!("{other:?}"),
    }
    let names = delver().main_names();
    let bad = |v: serde_json::Value| MtgSpec::parse(&v.to_string()).unwrap_err();
    // Best-of-three isn't built for constructed decks; nor random,
    // evaluation or pool decks (limited's).
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "decks": {"p0": names, "p1": names}})).contains("bo1"));
    assert!(bad(serde_json::json!({"constructed": true})).contains("decklists"));
    assert!(bad(serde_json::json!({"constructed": true, "decks": "eval"})).contains("decklists"));
    // The deck rules are checked.
    assert!(bad(serde_json::json!({"constructed": true, "decks": {"p0": &names[..40], "p1": names}})).contains("at least 60"));
    // More cards than the engine's action slots can hold: refused at the spec.
    let big: Vec<String> = names.iter().cloned().chain(std::iter::repeat_n("Island".to_string(), 5)).collect();
    assert!(bad(serde_json::json!({"constructed": true, "decks": {"p0": names, "p1": big}})).contains("p1's constructed deck: 65 cards"));
    assert!(bad(serde_json::json!({"constructed": "yes"})).contains("boolean"));
    // Without "constructed" the cube's card table: no legacy card there.
    assert!(bad(serde_json::json!({"decks": {"p0": names, "p1": names}})).contains("Murktide Regent"));
    // A cube spec is what it was.
    let cube = MtgSpec::parse(r#"{"max_turns": 60}"#).unwrap();
    assert!(!cube.constructed && std::ptr::eq(cube.db(), CardDb::cube()));
    assert_eq!(cube, MtgSpec { max_turns: 60, ..MtgSpec::default() });
    // The evaluation spec of a decklist spec is the spec.
    assert_eq!(MtgSpec::parse(&MtgSpec::eval_json(&mirror_json(60)).unwrap()).unwrap(), s);
}

#[test]
fn the_constructed_card_table_keeps_the_cubes_and_adds_the_legacy_cards() {
    let (cube, con) = (CardDb::cube(), CardDb::constructed());
    // The cube's table has no legacy card; the constructed one has all of
    // them (the Delver mirror's, ENGINE.md 54; the Doomsday deck's, 110).
    let legacy: Vec<String> = engine::db::load_legacy_document().cards.into_iter().map(|c| c.name).collect();
    for n in ["Murktide Regent", "Prismari Charm", "Doomsday", "Pact of Negation", "Personal Tutor", "Flow State"] {
        assert!(legacy.iter().any(|x| x == n), "{n} in the legacy IR");
    }
    for n in &legacy {
        assert!(cube.id(n).is_none(), "{n} in the cube's card table");
        assert!(con.id(n).is_some(), "{n}: {:?}", con.expect_id(n));
    }
    assert!(cube.unsupported().is_empty() && con.unsupported().is_empty());
    // Every cube card at the same id, with the same definition; its tokens
    // and faces k ids later (after the k legacy cards), the same too.
    let k = legacy.len() as u16;
    assert_eq!(con.num_cards(), cube.num_cards() + k as usize);
    assert_eq!(con.len(), cube.len() + k as usize, "the legacy cards make no token or face");
    let to_cube = |id: u16| if id < cube.num_cards() as u16 { id } else { id - k };
    for i in 0..cube.len() as u16 {
        let j = if i < cube.num_cards() as u16 { i } else { i + k };
        let a = cube.def(i);
        let mut b = con.def(j).clone();
        b.back = b.back.map(to_cube);
        b.adventure = b.adventure.map(to_cube);
        b.front = b.front.map(to_cube);
        b.halves = b.halves.map(|(x, y)| (to_cube(x), to_cube(y)));
        b.bestow = b.bestow.map(to_cube);
        b.prepare = b.prepare.map(to_cube);
        b.levels.iter_mut().for_each(|x| *x = to_cube(*x));
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "definition {i} ({}) differs", a.name);
    }
    // Identities: cube cards and tokens as in the cube's table; the legacy
    // cards at 546 + their registry line (layout v4's free range, below
    // CUBE_CAP).
    let (ic, ik) = (identity::identities(cube), identity::identities(con));
    for i in 0..cube.num_cards() {
        assert_eq!(ic[i], ik[i]);
    }
    let reg = identity::legacy_registry();
    assert_eq!(reg[..2], ["Murktide Regent", "Prismari Charm"]);
    assert_eq!(ik[con.id("Murktide Regent").unwrap() as usize], 546);
    assert_eq!(ik[con.id("Prismari Charm").unwrap() as usize], 547);
    // Each legacy card at LEGACY_BASE + its registry line (ENGINE.md 110:
    // the Doomsday deck's after the Delver mirror's).
    for (line, n) in reg.iter().enumerate() {
        if let Some(id) = con.id(n) {
            assert_eq!(ik[id as usize] as usize, identity::LEGACY_BASE + line, "{n}");
        }
    }
    assert!(identity::LEGACY_BASE + identity::legacy_registry().len() <= identity::CUBE_CAP);
    assert_eq!(identity::cube_names().len(), identity::LEGACY_BASE);
    // Every card of the legacy IR has a registry line (append-only: a new
    // legacy card is a new line at the end).
    for c in engine::db::load_legacy_document().cards {
        assert!(identity::legacy_registry().contains(&c.name.as_str()), "{} has no legacy identity", c.name);
    }
    // Tokens: the same identities by registry key.
    for t in 0..(cube.len() - cube.num_cards()) {
        let d = cube.def((cube.num_cards() + t) as u16);
        if d.token {
            let k = con.token_by_name(&d.name).unwrap();
            assert_eq!(ik[k as usize] != identity::UNKNOWN, ic[cube.num_cards() + t] != identity::UNKNOWN, "{}", d.name);
        }
    }
}

#[test]
fn sixty_card_games_play_draw_mulligan_and_turn_cap() {
    let sp = mirror(60);
    let mut keys = Vec::new();
    let (mut mulls, mut bottoms, mut draws_first) = (0, 0, 0);
    for seed in 0..40u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        assert_eq!(g.g.player(0).library.len() + g.g.player(1).library.len(), 120);
        // CR 103.1: the chooser's play-or-draw decision first.
        assert_eq!(g.g.pending_decision().unwrap().kind, DecisionKind::PlayDraw);
        let mut h = Heuristic::new(seed);
        let mut rng = Rng::new(seed);
        while g.to_move().is_some() {
            let l = legal(&g);
            let kind = g.g.pending_decision().map(|d| d.kind);
            let a = match kind {
                // Mulligan a third of the time (London: draw seven, bottom one per mulligan).
                Some(DecisionKind::Mulligan) if rng.below(3) == 0 && l.contains(&engine::act::YES) => {
                    mulls += 1;
                    engine::act::YES
                }
                Some(DecisionKind::PlayDraw) if rng.below(2) == 0 => {
                    draws_first += 1;
                    engine::act::PLAYER + 1
                }
                Some(DecisionKind::MulliganBottom) => {
                    bottoms += 1;
                    h.choose(&g.g, &l)
                }
                _ => h.choose(&g.g, &l),
            };
            g.step(a, &mut keys).unwrap();
            g.g.check_invariants().unwrap();
        }
        // Every card of each deck is in a zone (none lost; tokens apart).
        let cards = g.g.objs().iter().filter(|o| !g.g.db().def(o.def).token && o.zone != Zone::Nowhere).count();
        assert!(cards >= 120, "{cards}");
        assert!(g.g.is_over());
    }
    assert!(mulls > 10 && bottoms >= mulls && draws_first > 5, "{mulls} {bottoms} {draws_first}");
    // The turn cap: a draw.
    let sp = mirror(3);
    let mut g = MtgAz::new_game(&sp, 2, 1).unwrap();
    while g.to_move().is_some() {
        let a = legal(&g)[0];
        g.step(a, &mut keys).unwrap();
    }
    assert_eq!(g.result()[..2], [0.5, 0.5]);
    assert_eq!(g.rounds(), 3);
}

/// Open decklists (CR 100.2a play; ENGINE.md 10, 53): `determinize` samples
/// the opponent's unseen cards from their known 60 minus every card of theirs
/// the viewer has seen: in every world each player's cards are exactly its
/// decklist, the viewer's knowledge holds, and what's hidden varies.
#[test]
fn determinize_samples_the_opponents_unseen_cards_from_their_decklist() {
    let sp = mirror(60);
    let deck: Vec<u16> = {
        let mut d = Decklist::ids(db(), &delver().main).unwrap();
        d.sort_unstable();
        d
    };
    let mut rng = zero_search::rng::Rng::new(9);
    let mut keys = Vec::new();
    let (mut worlds, mut varied) = (0, 0);
    for seed in 0..12u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut h = Heuristic::new(seed);
        let mut step = 0;
        while g.to_move().is_some() {
            step += 1;
            if step % 37 == 0 {
                let viewer = g.to_move().unwrap() as u8;
                let opp = 1 - viewer;
                let hidden_of = |x: &MtgAz| -> Vec<u16> {
                    let mut v: Vec<u16> = x.g.player(opp).hand.iter().chain(&x.g.player(opp).library).map(|&c| x.g.obj(c).def).collect();
                    v.sort_unstable();
                    v
                };
                let base_hand: Vec<u16> = g.g.player(opp).hand.iter().map(|&c| g.g.obj(c).def).collect();
                for _ in 0..4 {
                    let mut d = g.clone();
                    d.determinize(viewer as usize, &mut rng);
                    worlds += 1;
                    // Each player's cards: exactly its 60 (deck card slots
                    // keep their owner; tokens aside).
                    for p in 0..2u8 {
                        let n_own = d.g.objs().iter().take(120).filter(|o| o.owner == p).count();
                        assert_eq!(n_own, 60);
                    }
                    let mut cards: Vec<u16> =
                        (0..120u32).filter(|&i| d.g.obj(engine::CardIdx::new(i)).owner == opp).map(|i| d.g.ident(engine::CardIdx::new(i))).collect();
                    cards.iter_mut().for_each(|c| {
                        if let Some(f) = d.g.db().def(*c).front {
                            *c = f;
                        }
                    });
                    cards.sort_unstable();
                    assert_eq!(cards, deck, "a world of P{viewer} gave P{opp} cards outside its decklist");
                    // The viewer's cards and knowledge are kept; only hidden
                    // identities moved, within the opponent's unseen cards.
                    assert_eq!(hidden_of(&d), hidden_of(&g), "the opponent's hidden multiset changed");
                    for i in 0..120u32 {
                        let c = engine::CardIdx::new(i);
                        if g.g.knows(viewer, c) {
                            assert_eq!(g.g.ident(c), d.g.ident(c));
                        }
                    }
                    let mut x = vec![0.0; NUM_FEATURES];
                    let mut y = vec![0.0; NUM_FEATURES];
                    g.encode(viewer as usize, &mut x);
                    d.encode(viewer as usize, &mut y);
                    assert!(x == y, "the viewer's encoding changed");
                    let hand: Vec<u16> = d.g.player(opp).hand.iter().map(|&c| d.g.obj(c).def).collect();
                    varied += (hand != base_hand) as u32;
                }
            }
            let l = legal(&g);
            let a = h.choose(&g.g, &l);
            g.step(a, &mut keys).unwrap();
        }
    }
    println!("{worlds} worlds, opponent's hand resampled in {varied}");
    assert!(worlds > 100 && varied > worlds / 3, "{varied}/{worlds}");
}

#[test]
fn the_heuristic_plays_the_mirror() {
    // Both seats the heuristic; then the heuristic against random.
    let sp = mirror(60);
    let mut keys = Vec::new();
    let (mut first_wins, mut decided, mut turns, mut random_lost) = (0, 0, 0, 0);
    for seed in 0..60u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut hs = [Heuristic::new(seed), Heuristic::new(seed + 1)];
        let mut rng = Rng::new(seed);
        while let Some(p) = g.to_move() {
            let l = legal(&g);
            let a = if seed >= 40 && p == 1 { l[rng.below(l.len() as u32) as usize] } else { hs[p].choose(&g.g, &l) };
            g.step(a, &mut keys).unwrap();
        }
        let r = g.result();
        if seed < 40 && r[0] != r[1] {
            decided += 1;
            first_wins += (r[g.g.turn().starting as usize] == 1.0) as u32;
        }
        if seed >= 40 {
            random_lost += (r[0] == 1.0) as u32;
        }
        turns += g.g.turn().number;
    }
    println!("first player won {first_wins}/{decided}; the heuristic beat random {random_lost}/20; {:.1} turns per game", turns as f64 / 60.0);
    assert!(decided > 30);
    // (A sanity floor: ENGINE.md 54.6 measures the score over 2,000 games.)
    assert!(random_lost >= 13, "the heuristic beat random only {random_lost}/20 in the mirror");
}
