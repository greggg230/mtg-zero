//! M8a (ENGINE.md 35): the play-or-draw choice (CR 103.1), sideboards
//! (100.4, 400.11a), companions (702.139: the reveal, 103.2b; the special
//! action, 116.2g).

mod common;

use common::db;
use engine::az::{decks, MtgSpec};
use engine::decision::act;
use engine::ids::Target;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, GameSpec, Step, Zone};

fn ids(names: &[&str]) -> Vec<u16> {
    names.iter().map(|n| db().expect_id(n).unwrap()).collect()
}

/// 40 cards: `spells` once each, the rest `land`.
fn deck(spells: &[&str], land: &str) -> Vec<u16> {
    let mut d = ids(spells);
    while d.len() < 40 {
        d.push(db().id(land).unwrap());
    }
    d
}

const LURRUS: &str = "Lurrus of the Dream-Den";
const LUTRI: &str = "Lutri, the Spellchaser";
const ZIRDA: &str = "Zirda, the Dawnwaker";

/// Permanents of mana value 2 or less, instants: Lurrus's condition.
fn lurrus_deck() -> Vec<u16> {
    deck(&["Mother of Runes", "Ragavan, Nimble Pilferer", "Thalia, Guardian of Thraben", "Lightning Bolt", "Ancestral Recall"], "Plains")
}

/// Every permanent has an activated ability (basic lands: their intrinsic
/// mana ability, CR 305.6): Zirda's condition.
fn zirda_deck() -> Vec<u16> {
    deck(&["Birds of Paradise", "Mother of Runes", "Walking Ballista", "Sol Ring", "Lightning Bolt"], "Mountain")
}

fn spec() -> GameSpec {
    GameSpec { mulligans: false, auto_single: false, log: true, ..GameSpec::default() }
}

fn game(d0: &[u16], s0: &[u16], d1: &[u16], s1: &[u16], spec: GameSpec, seed: u64) -> Game {
    Game::with_sideboards(db(), spec, [d0, d1], [s0, s1], seed).unwrap()
}

#[test]
fn cr_103_1_the_chooser_chooses_who_plays_first_before_hands() {
    let d = deck(&["Lightning Bolt"], "Mountain");
    let mut chooser_seen = [false; 2];
    for seed in 0..16 {
        let mut g = game(&d, &[], &d, &[], spec(), seed);
        // The first decision is the chooser's (a player chosen at random in
        // the first game of a match), before the shuffles and the opening
        // hands (103.1 before 103.3, 103.5).
        let c = g.turn().chooser;
        chooser_seen[c as usize] = true;
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::PlayDraw), Some(c)));
        assert!(g.player(0).hand.is_empty() && g.player(1).hand.is_empty());
        let d = g.pending_decision().unwrap();
        assert_eq!(d.options.iter().map(|o| o.action).collect::<Vec<_>>(), vec![act::PLAYER, act::PLAYER + 1]);
        // Draw first: the other player is the starting player.
        let keys = g.choose(Choice::Target(Target::Player(1 - c)));
        assert_eq!(g.turn().starting, 1 - c);
        assert_eq!(g.turn().chooser, c);
        // The opening hands are drawn now (keyed draws: 7 + 7).
        assert_eq!(keys.len(), 14);
        assert_eq!((g.player(0).hand.len(), g.player(1).hand.len()), (7, 7));
        // The starting player takes the first turn and skips its draw (103.8a).
        g.drive_until(|g| g.turn().number == 1 && g.turn().step == Step::Main1);
        assert_eq!(g.turn().active, 1 - c);
        assert_eq!(g.player(1 - c).hand.len(), 7);
    }
    assert_eq!(chooser_seen, [true, true], "the chooser is random");
}

#[test]
fn cr_103_1_the_chooser_is_a_spec_parameter() {
    // In a later game of a match the loser of the previous game chooses.
    let d = deck(&["Lightning Bolt"], "Mountain");
    for seed in 0..6 {
        let mut g = game(&d, &[], &d, &[], GameSpec { chooser: Some(1), ..spec() }, seed);
        assert_eq!((g.kind(), g.to_move(), g.turn().chooser), (Some(DecisionKind::PlayDraw), Some(1), 1));
        g.choose(Choice::Target(Target::Player(1)));
        assert_eq!(g.turn().starting, 1);
    }
    assert!(Game::with_sideboards(db(), GameSpec { chooser: Some(2), ..spec() }, [&d, &d], [&[], &[]], 1).is_err());
}

/// Reveal decisions of a game with these decks and sideboards, P0 choosing
/// to play first: (player, the companions offered) in order.
fn reveals(d0: &[u16], s0: &[u16], d1: &[u16], s1: &[u16]) -> (Game, Vec<(u8, Vec<u16>)>) {
    let mut g = game(d0, s0, d1, s1, GameSpec { chooser: Some(0), ..spec() }, 3);
    g.choose(Choice::Target(Target::Player(0)));
    let mut out = Vec::new();
    while g.kind() == Some(DecisionKind::RevealCompanion) {
        let d = g.pending_decision().unwrap().clone();
        // Before the opening hands (103.2b before 103.5).
        assert!(g.player(0).hand.is_empty() && g.player(1).hand.is_empty());
        assert_eq!(d.options[0].choice, Choice::Pass);
        let offered: Vec<u16> = d
            .options
            .iter()
            .skip(1)
            .enumerate()
            .map(|(k, o)| {
                assert_eq!(o.action, act::LIBRARY + k as u16);
                match o.choice {
                    Choice::Def(x) => x,
                    other => panic!("{other:?}"),
                }
            })
            .collect();
        out.push((d.player, offered));
        let pick = d.options[1].choice;
        g.choose(pick);
    }
    (g, out)
}

#[test]
fn cr_702_139a_lurrus_condition_each_permanent_mana_value_2_or_less() {
    // ruling: bd8a17c9 f103228a
    let lurrus = db().expect_id(LURRUS).unwrap();
    let ok = lurrus_deck();
    let (g, r) = reveals(&ok, &ids(&[LURRUS, "Counterspell"]), &ok, &[]);
    assert_eq!(r, vec![(0, vec![lurrus])]);
    // Revealed: a public card outside the game (no zone), known to both.
    let c = g.player(0).companion.unwrap();
    assert_eq!((g.obj(c).def, g.obj(c).zone, g.obj(c).known), (lurrus, Zone::Nowhere, 0b11));
    assert_eq!(g.companion_outside(0), Some(c));
    assert_eq!(g.player(0).hand.len(), 7, "the hands follow (103.5)");
    // A permanent of mana value 3 (Adeline) breaks it; an instant of mana
    // value 3 would not (only permanent cards count).
    let bad = deck(&["Mother of Runes", "Adeline, Resplendent Cathar"], "Plains");
    assert!(reveals(&bad, &ids(&[LURRUS]), &ok, &[]).1.is_empty());
    let fine = deck(&["Mother of Runes", "Force of Will"], "Plains");
    assert_eq!(reveals(&fine, &ids(&[LURRUS]), &ok, &[]).1, vec![(0, vec![lurrus])]);
}

#[test]
fn cr_702_139a_lutri_condition_each_nonland_card_a_different_name() {
    let lutri = db().expect_id(LUTRI).unwrap();
    // Forty basics share a name, but they are lands.
    let singleton = deck(&["Lightning Bolt", "Counterspell", "Brainstorm"], "Island");
    assert_eq!(reveals(&singleton, &[], &singleton, &ids(&[LUTRI])).1, vec![(1, vec![lutri])]);
    let twice = deck(&["Lightning Bolt", "Lightning Bolt", "Brainstorm"], "Island");
    assert!(reveals(&twice, &ids(&[LUTRI]), &twice, &[]).1.is_empty());
}

#[test]
fn cr_702_139a_zirda_condition_each_permanent_has_an_activated_ability() {
    let zirda = db().expect_id(ZIRDA).unwrap();
    let ok = zirda_deck();
    assert_eq!(reveals(&ok, &ids(&[ZIRDA]), &ok, &[]).1, vec![(0, vec![zirda])]);
    // Ragavan (dash is an alternative cost) and Thalia have none.
    let bad = deck(&["Birds of Paradise", "Thalia, Guardian of Thraben"], "Mountain");
    assert!(reveals(&bad, &ids(&[ZIRDA]), &ok, &[]).1.is_empty());
    let bad = deck(&["Ragavan, Nimble Pilferer"], "Mountain");
    assert!(reveals(&bad, &ids(&[ZIRDA]), &ok, &[]).1.is_empty());
}

#[test]
fn cr_103_2b_companions_in_turn_order_and_at_most_one() {
    // ruling: a2e75b69 aa67eb2c b4178eed
    let (lurrus, zirda) = (db().expect_id(LURRUS).unwrap(), db().expect_id(ZIRDA).unwrap());
    // A deck fulfilling both: Lurrus and Zirda are offered, one is revealed
    // (CR 103.2b: no more than one); the starting player first, then the
    // other (101.4).
    let both = deck(&["Birds of Paradise", "Mother of Runes", "Lightning Bolt"], "Plains");
    let (g, r) = reveals(&both, &[], &both, &ids(&[ZIRDA, LURRUS]));
    let mut want = vec![lurrus, zirda];
    want.sort_unstable();
    assert_eq!(r, vec![(1, want.clone())]);
    assert_eq!(g.obj(g.player(1).companion.unwrap()).def, want[0]);
    let (_, r) = reveals(&both, &ids(&[LURRUS]), &both, &ids(&[ZIRDA]));
    assert_eq!(r, vec![(0, vec![lurrus]), (1, vec![zirda])]);
    // No candidate: no decision (the sideboard is the player's own; ENGINE.md
    // 35). Declining keeps it in the sideboard, unrevealed.
    let mut g = game(&both, &ids(&[LURRUS]), &both, &[], GameSpec { chooser: Some(0), ..spec() }, 3);
    g.choose(Choice::Target(Target::Player(0)));
    assert_eq!(g.kind(), Some(DecisionKind::RevealCompanion));
    g.choose(Choice::Pass);
    assert_ne!(g.kind(), Some(DecisionKind::RevealCompanion));
    assert_eq!(g.player(0).companion, None);
    assert_eq!(g.sideboard_size(0), 1);
}

#[test]
fn cr_116_2g_the_companion_special_action() {
    // ruling: a037c9c9 e980e48f
    let start = |step: Step, active: u8| {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .companion(0, LURRUS)
            .sideboard(0, &["Counterspell"])
            .battlefield(0, &["Plains", "Plains", "Swamp", "Swamp"])
            .library(0, &["Plains"; 5])
            .library(1, &["Island"; 5])
            .at(step, active)
            .start()
    };
    let has = |g: &Game| g.has_option(Choice::Companion);
    // Not in the upkeep, not on the opponent's turn (sorcery timing).
    assert!(!has(&start(Step::Upkeep, 0)));
    assert!(!has(&start(Step::Main1, 1)));
    let mut g = start(Step::Main1, 0);
    assert!(has(&g));
    assert_eq!(g.pending_decision().unwrap().action_of(Choice::Companion), Some(act::COMPANION));
    assert_eq!(g.sideboard_size(0), 2);
    // Pay {3}, then it is put into the hand (no stack: 116.2g), known to both.
    g.choose(Choice::Companion);
    while g.kind() != Some(DecisionKind::Priority) {
        let d = g.pending_decision().unwrap();
        assert!(matches!(d.kind, DecisionKind::PayMana | DecisionKind::PayPool), "{:?}", d.kind);
        let pick = d.options.iter().find(|o| o.action != act::PASS).unwrap().choice;
        g.choose(pick);
    }
    let c = g.find_card(0, LURRUS, Zone::Hand).expect("in the hand");
    assert!(g.stack().is_empty());
    assert_eq!(g.obj(c).known, 0b11);
    assert_eq!(g.battlefield().iter().filter(|&&x| g.obj(x).tapped).count(), 3);
    assert!(g.player(0).companion_used);
    assert_eq!(g.companion_outside(0), None);
    assert_eq!(g.sideboard_size(0), 1);
    // Once per game.
    assert!(!has(&g));
    // Not without {3}.
    let g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .companion(0, LURRUS)
        .battlefield(0, &["Plains", "Plains"])
        .library(0, &["Plains"; 5])
        .library(1, &["Island"; 5])
        .start();
    assert!(!has(&g));
    g.check_invariants().unwrap();
    // Not with the stack not empty (a Lightning Bolt cast, its caster has
    // priority).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .companion(0, LURRUS)
        .battlefield(0, &["Mountain"; 4])
        .hand(0, &["Lightning Bolt"])
        .library(0, &["Plains"; 5])
        .library(1, &["Island"; 5])
        .start();
    assert!(has(&g));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    while !(g.kind() == Some(DecisionKind::Priority) && !g.stack().is_empty()) {
        let pick = g.pending_decision().unwrap().options.iter().find(|o| o.action != act::PASS).unwrap().choice;
        g.choose(pick);
    }
    assert!(!has(&g));
}

#[test]
fn cr_400_11a_the_sideboard_is_hidden_from_the_opponent() {
    // Two games that differ only in an unrevealed sideboard card look the
    // same to the other player (its view and its encoding); its owner sees
    // the difference. A revealed companion is public.
    let ok = lurrus_deck();
    let mk = |s0: &[&str]| {
        let mut g = game(&ok, &ids(s0), &ok, &[], GameSpec { chooser: Some(1), ..spec() }, 5);
        g.choose(Choice::Target(Target::Player(1)));
        while g.kind() == Some(DecisionKind::RevealCompanion) {
            let first = g.pending_decision().unwrap().options[1].choice;
            g.choose(first);
        }
        g
    };
    let a = mk(&[LURRUS, "Counterspell"]);
    let b = mk(&[LURRUS, "Brainstorm"]);
    assert_eq!(a.view_of(1), b.view_of(1));
    assert_ne!(a.view_of(0), b.view_of(0));
    let enc = |g: &Game, v: u8| {
        let mut x = vec![0.0f32; engine::az::NUM_FEATURES];
        g.encode_features(v, &mut x);
        x
    };
    assert_eq!(enc(&a, 1), enc(&b, 1));
    // The revealed companion is in both players' views and encodings.
    let c = mk(&[LURRUS]);
    let d = mk(&["Counterspell"]);
    assert_ne!(c.view_of(1), d.view_of(1));
    let o = engine::az::encode::O_COMP + engine::az::encode::COMP_W;
    let x = enc(&c, 1);
    assert_eq!(x[o + engine::az::encode::CODE + engine::az::encode::comp::PRESENT], 1.0, "the opponent's companion slot");
    assert_eq!(x[o + engine::az::encode::CODE + engine::az::encode::comp::AVAILABLE], 1.0);
}

#[test]
fn determinize_keeps_sideboards_and_the_revealed_companion() {
    let ok = lurrus_deck();
    let mut g = game(&ok, &ids(&[LURRUS, "Counterspell"]), &ok, &ids(&["Brainstorm", LUTRI]), GameSpec { chooser: Some(0), ..spec() }, 9);
    g.choose(Choice::Target(Target::Player(0)));
    while g.kind() == Some(DecisionKind::RevealCompanion) {
        let first = g.pending_decision().unwrap().options[1].choice;
        g.choose(first);
    }
    let lurrus = db().expect_id(LURRUS).unwrap();
    for viewer in 0..2u8 {
        for k in 0..8 {
            let mut w = g.clone();
            w.determinize(viewer, &mut engine::rng::Rng::new(k));
            assert_eq!(w.view_of(viewer), g.view_of(viewer));
            for p in 0..2u8 {
                assert_eq!(w.player(p).sideboard, g.player(p).sideboard, "kept: phase 1's information model");
            }
            let c = w.player(0).companion.unwrap();
            assert_eq!((w.obj(c).def, w.obj(c).zone), (lurrus, Zone::Nowhere));
        }
    }
}

#[test]
fn self_play_decks_have_sideboards_and_companions() {
    let sp = MtgSpec::default();
    let gen = decks::DeckGen::default();
    let comps = decks::companions(db());
    assert_eq!(comps.len(), 3);
    let mut seen = std::collections::BTreeMap::new();
    for seed in 0..400u64 {
        let pools = sp.pools(seed);
        assert_eq!(pools, sp.pools(seed), "seeded");
        assert_eq!(sp.decks(seed), [pools[0].0.clone(), pools[1].0.clone()]);
        for (deck, side) in &pools {
            decks::check(db(), deck, &gen).unwrap();
            assert_eq!(side.len(), decks::SIDEBOARD);
            assert!(side.windows(2).all(|w| w[0] < w[1]), "sorted, singleton");
            assert!(side.iter().all(|c| !deck.contains(c)), "outside the deck");
            for &c in side.iter().filter(|c| comps.contains(c)) {
                // A companion in a generated sideboard is one the deck can
                // reveal.
                assert!(db().deck_satisfies(db().def(c).companion.as_ref().unwrap(), deck));
                *seen.entry(db().def(c).name.clone()).or_insert(0) += 1;
            }
        }
    }
    println!("companions in 800 generated sideboards: {seen:?}");
    for n in [LURRUS, LUTRI, ZIRDA] {
        assert!(seen.get(n).copied().unwrap_or(0) >= 3, "{n} in too few sideboards: {seen:?}");
    }
    // The evaluation set has sideboards too.
    let e = MtgSpec::parse(&MtgSpec::eval_json("").unwrap()).unwrap();
    assert!(e.pools(1).iter().all(|(_, s)| s.len() == decks::SIDEBOARD));
}

#[test]
fn the_heuristic_bot_plays_first_reveals_and_uses_its_companion() {
    let ok = lurrus_deck();
    let mut used = 0;
    for seed in 0..6u64 {
        let spec = GameSpec { max_turns: 16, ..GameSpec::default() };
        let mut g = game(&ok, &ids(&[LURRUS]), &ok, &[], spec, seed);
        let mut bot = engine::az::bots::Heuristic::new(seed);
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        let chooser = g.turn().chooser;
        let mut first = true;
        while !g.is_over() {
            g.legal_actions(&mut legal);
            let a = bot.choose(&g, &legal);
            assert!(legal.contains(&a));
            g.apply(a, &mut keys).unwrap();
            if first {
                assert_eq!(g.turn().starting, chooser, "plays first");
                first = false;
            }
        }
        assert!(g.player(0).companion.is_some(), "revealed");
        used += g.player(0).companion_used as u32;
    }
    assert!(used > 0, "the special action was never taken");
}
