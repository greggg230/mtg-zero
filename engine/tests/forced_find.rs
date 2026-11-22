//! The forced find (ENGINE.md 180, `engine/src/forced_find.rs`): a self-play training aid. With the spec's
//! `"forced_find": {"spell": "Doomsday", "card": "Thassa's Oracle", "p": p, "unless_in_hand": true}` a share p of
//! Doomsday's resolutions must put a Thassa's Oracle in the pile: only the pile's LAST pick is touched, and only when no
//! Oracle was chosen, none is in the hand and one can be found - its one option is then the Oracle (in the library or the
//! graveyard). The draw comes from the game's seed and the resolution's index, the same in every world; the forced pick
//! has one option, so the engine answers it (`auto_single`) and zero-core never sees, searches or records it; nothing of
//! it reaches an encoding. Refused loudly where it cannot apply; the evaluation spec drops it.

use engine::az::constructed::Decklist;
use engine::az::tail_encode::Row;
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::forced_find::{FfClass, ForcedFind};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use zero_search::game::SearchGame;

const DD: &str = "Doomsday";
const ORACLE: &str = "Thassa's Oracle";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

fn ff(p: f64) -> ForcedFind {
    ForcedFind::new(def(DD), def(ORACLE), p, true)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

/// `p`'s library, top first, by name.
fn library(g: &Game, p: PlayerId) -> Vec<&'static str> {
    g.player(p).library.iter().rev().map(|&c| g.name(c)).collect()
}

/// P0 at its precombat main phase with three Swamps and Doomsday in hand, the forced find at `p` (None: off);
/// `auto`: single options answered by the engine (the self-play setting) or asked (to see the forced decision).
fn dd_game(lib: &[&str], gy: &[&str], hand: &[&str], p: Option<f64>, auto: bool) -> Game {
    let mut h = vec![DD];
    h.extend_from_slice(hand);
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = auto;
            s.forced_find = p.map(ff);
        })
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &h)
        .library(0, lib)
        .graveyard(0, gy)
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start()
}

/// Cast Doomsday (the first option for its mana) and pass until its first choice.
fn cast_doomsday(g: &mut Game) {
    let c = g.card(0, DD, Zone::Hand);
    g.choose(Choice::Cast(c));
    for _ in 0..100 {
        if matches!(g.kind(), Some(DecisionKind::SearchLibrary | DecisionKind::ChooseGraveyardCard | DecisionKind::PutOnTop)) {
            return;
        }
        if at_priority(g) {
            g.pass_until(|g| g.stack().is_empty() || !at_priority(g));
            return;
        }
        let ch = g.pending_decision().expect("a decision").options[0].choice;
        g.choose(ch);
    }
    panic!("cast Doomsday: no priority");
}

/// The pile decisions' options as card names (a library pick by definition, a graveyard pick by card).
fn offered(g: &Game) -> Vec<String> {
    g.pending_decision()
        .map(|d| {
            d.options
                .iter()
                .map(|o| match o.choice {
                    Choice::Def(x) => db().def(x).name.to_string(),
                    Choice::Card(c) => format!("gy:{}", g.name(c)),
                    Choice::Pass => "PASS".to_string(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `names` in the library search's order (non-decreasing definition).
fn sorted<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut v = names.to_vec();
    v.sort_by_key(|n| def(n));
    v
}

/// Library picks `names` in the search's order (non-decreasing definition, ENGINE.md 124.1), each a free choice.
fn pick_lib(g: &mut Game, names: &[&str]) {
    let mut v = names.to_vec();
    v.sort_by_key(|n| def(n));
    for n in v {
        assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}
{}", opts(g), g.log_lines().join("
"));
        choose_def(g, n);
    }
}

fn choose_def(g: &mut Game, name: &str) {
    assert!(g.has_option(Choice::Def(def(name))), "{name} offered: {}", opts(g));
    g.choose(Choice::Def(def(name)));
}

/// The rest of the resolution: the order's first option each time, until the stack is empty.
fn finish(g: &mut Game) {
    while !g.stack().is_empty() || !at_priority(g) {
        if g.is_over() {
            return;
        }
        let ch = g.pending_decision().expect("a decision").options[0].choice;
        g.choose(ch);
    }
}

const LIB: &[&str] = &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal", "Island"];

#[test]
fn the_last_pick_is_forced_to_the_oracle_when_none_was_chosen() {
    let mut g = dd_game(LIB, &[], &[], Some(1.0), false);
    cast_doomsday(&mut g);
    pick_lib(&mut g, &["Brainstorm", "Consider", "Dark Ritual", "Island"]);
    // The fifth pick: its only option is the Oracle (below the floor the earlier picks left).
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    assert_eq!(offered(&g), vec![ORACLE.to_string()]);
    assert!(g.log_lines().iter().any(|l| l.contains("forced_find")), "the log says so");
    choose_def(&mut g, ORACLE);
    finish(&mut g);
    let mut lib = library(&g, 0);
    lib.sort_unstable();
    assert_eq!(lib, vec!["Brainstorm", "Consider", "Dark Ritual", "Island", ORACLE]);
    let r = &g.forced_finds()[0];
    assert_eq!((r.player, r.drawn, r.class), (0, true, FfClass::Forced));
    assert!(r.pile.contains(&def(ORACLE)));
}

#[test]
fn an_oracle_the_player_chose_is_never_forced_and_reads_voluntary() {
    // Four other cards, one of them after the Oracle in the search's order (definition order): the Oracle is chosen
    // before the last pick, which stays free.
    // (Personal Tutor and Flow State are legacy cards, after the cube's in definition order.)
    const LIB2: &[&str] = &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal", "Island", "Personal Tutor", "Flow State"];
    let after: Vec<&str> = LIB2.iter().copied().filter(|&n| def(n) > def(ORACLE)).collect();
    let before: Vec<&str> = LIB2.iter().copied().filter(|&n| def(n) < def(ORACLE)).collect();
    assert!(!after.is_empty() && before.len() >= 3, "the fixture straddles the Oracle's definition");
    let mut g = dd_game(LIB2, &[], &[], Some(1.0), false);
    cast_doomsday(&mut g);
    let mut picks = before[..3].to_vec();
    picks.extend([ORACLE, after[0]]);
    pick_lib(&mut g, &picks[..4]);
    assert!(offered(&g).len() >= 1 && g.kind() == Some(DecisionKind::SearchLibrary));
    choose_def(&mut g, after[0]);
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::Voluntary);
    // An Oracle that would be the last pick in that order is the forced pick whenever the draw forces: the class is
    // the decision's (a forced decision), not the player's intent (ENGINE.md 180).
    let mut g = dd_game(LIB2, &[], &[], Some(1.0), false);
    cast_doomsday(&mut g);
    pick_lib(&mut g, &before[..4.min(before.len())]);
    if before.len() >= 4 {
        assert_eq!(offered(&g), vec![ORACLE.to_string()]);
        choose_def(&mut g, ORACLE);
        finish(&mut g);
        assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
    }
}

#[test]
fn an_oracle_in_hand_exempts_the_resolution() {
    let mut g = dd_game(&["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal", ORACLE], &[], &[ORACLE], Some(1.0), false);
    cast_doomsday(&mut g);
    // (every pick free, the last one too)
    pick_lib(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Lotus Petal"]);
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::InHand);
    // With unless_in_hand false the library's copy is forced all the same.
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.forced_find = Some(ForcedFind::new(def(DD), def(ORACLE), 1.0, false));
        })
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &[DD, ORACLE])
        .library(0, &["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal", ORACLE])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    cast_doomsday(&mut g);
    pick_lib(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze"]);
    assert_eq!(offered(&g), vec![ORACLE.to_string()]);
}

#[test]
fn no_oracle_to_find_and_a_draw_that_says_no_leave_the_picks_free() {
    // Not findable: the Oracle is exiled.
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.forced_find = Some(ff(1.0));
        })
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &[DD])
        .library(0, &["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal"])
        .exile(0, &[ORACLE])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    cast_doomsday(&mut g);
    pick_lib(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Lotus Petal"]);
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::NotFindable);
    // p 0: never forced; the Oracle left out is missed.
    let mut g = dd_game(LIB, &[], &[], Some(0.0), false);
    cast_doomsday(&mut g);
    pick_lib(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Island"]);
    finish(&mut g);
    let r = &g.forced_finds()[0];
    assert_eq!((r.drawn, r.class), (false, FfClass::Missed));
}

#[test]
fn an_oracle_in_the_graveyard_is_the_forced_last_pick_from_either_part() {
    // The graveyard part reaches the last pick: its one option is the graveyard's Oracle (out of position order).
    let gy = ["Daze", ORACLE, "Ponder", "Consider", "Dark Ritual", "Lotus Petal"];
    let mut g = dd_game(&["Island", "Island"], &gy, &[], Some(1.0), false);
    cast_doomsday(&mut g);
    for n in ["Ponder", "Consider", "Dark Ritual", "Lotus Petal"] {
        let c = g.card(0, n, Zone::Graveyard);
        g.choose(Choice::Card(c));
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
    assert_eq!(offered(&g), vec![format!("gy:{ORACLE}")]);
    let c = g.card(0, ORACLE, Zone::Graveyard);
    g.choose(Choice::Card(c));
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
    assert!(library(&g, 0).contains(&ORACLE));
    // The graveyard part passed early: the library part's last pick is the graveyard's Oracle (a graveyard choice).
    let mut g = dd_game(&["Island", "Brainstorm", "Ponder", "Consider"], &["Daze", ORACLE], &[], Some(1.0), false);
    cast_doomsday(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
    let c = g.card(0, "Daze", Zone::Graveyard);
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    // (the library picks offer what they offer without the forced find: the first option each time)
    let mut picked = vec!["Daze"];
    while g.kind() == Some(DecisionKind::SearchLibrary) {
        let Choice::Def(d) = g.pending_decision().unwrap().options[0].choice else { panic!("a library pick") };
        picked.push(&db().def(d).name);
        g.choose(Choice::Def(d));
    }
    assert_eq!(picked.len(), 4, "three library picks");
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{}", opts(&g));
    assert_eq!(offered(&g), vec![format!("gy:{ORACLE}")]);
    let c = g.card(0, ORACLE, Zone::Graveyard);
    g.choose(Choice::Card(c));
    finish(&mut g);
    let mut lib = library(&g, 0);
    lib.sort_unstable();
    picked.push(ORACLE);
    picked.sort_unstable();
    assert_eq!(lib, picked);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
}

#[test]
fn a_library_oracle_ends_the_graveyard_part_one_pick_early() {
    // Four graveyard picks: the fifth would be a graveyard card or PASS - the graveyard part ends without asking, and
    // the library part's one pick is the Oracle.
    let gy = ["Daze", "Ponder", "Consider", "Dark Ritual", "Lotus Petal"];
    let mut g = dd_game(&["Island", ORACLE], &gy, &[], Some(1.0), false);
    cast_doomsday(&mut g);
    for n in ["Daze", "Ponder", "Consider", "Dark Ritual"] {
        assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
        let c = g.card(0, n, Zone::Graveyard);
        g.choose(Choice::Card(c));
    }
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}", opts(&g));
    assert_eq!(offered(&g), vec![ORACLE.to_string()]);
    choose_def(&mut g, ORACLE);
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
}

/// Every decision path of a resolving Doomsday from `g` (P0 deciding): (paths, the distinct libraries it can leave).
fn outcomes(g: &Game) -> (usize, BTreeSet<Vec<&'static str>>) {
    let mut out = BTreeSet::new();
    let mut paths = 0;
    let mut stack = vec![g.clone()];
    while let Some(g) = stack.pop() {
        if g.stack().is_empty() && at_priority(&g) {
            paths += 1;
            out.insert(library(&g, 0));
            continue;
        }
        let d = g.pending_decision().unwrap().clone();
        for o in &d.options {
            let mut h = g.clone();
            h.choose(o.choice);
            if at_priority(&h) && !h.stack().is_empty() {
                h.pass_until(|g| g.stack().is_empty() || !at_priority(g));
            }
            stack.push(h);
        }
    }
    (paths, out)
}

#[test]
fn forcing_reaches_exactly_the_piles_that_hold_the_oracle_in_every_order() {
    // Six different cards, three in the library (the Oracle among them) and three in the graveyard: without the forced
    // find any five in any order (720, each once: ENGINE.md 124.1); forced, exactly the five-card piles holding the
    // Oracle (5 of the 6) in every order: 600.
    let lib = ["Brainstorm", ORACLE, "Consider"];
    let gy = ["Daze", "Dark Ritual", "Lotus Petal"];
    let mut free = dd_game(&lib, &gy, &[], None, false);
    cast_doomsday(&mut free);
    let (paths, all) = outcomes(&free);
    assert_eq!((paths, all.len()), (720, 720));
    let mut g = dd_game(&lib, &gy, &[], Some(1.0), false);
    cast_doomsday(&mut g);
    let (_, forced) = outcomes(&g);
    assert_eq!(forced.len(), 600);
    assert!(forced.iter().all(|l| l.contains(&ORACLE)));
    let holding: BTreeSet<Vec<&str>> = all.iter().filter(|l| l.contains(&ORACLE)).cloned().collect();
    assert_eq!(forced, holding);
    // p 0 is the game without the option, path for path.
    let mut z = dd_game(&lib, &gy, &[], Some(0.0), false);
    cast_doomsday(&mut z);
    assert_eq!(outcomes(&z), (720, all));
    // The same with the Oracle in the graveyard.
    let lib = ["Brainstorm", "Ponder", "Consider"];
    let gy = ["Daze", ORACLE, "Lotus Petal"];
    let mut g = dd_game(&lib, &gy, &[], Some(1.0), false);
    cast_doomsday(&mut g);
    let (_, forced) = outcomes(&g);
    assert_eq!(forced.len(), 600);
    assert!(forced.iter().all(|l| l.contains(&ORACLE)));
}

#[test]
fn with_single_options_answered_by_the_engine_the_forced_pick_is_never_a_decision() {
    // The self-play setting (`auto_single`): the fifth pick never reaches a player (zero-core's decisions are the
    // engine's pending ones): after the fourth pick comes the order, the Oracle in the pile; the answer is keyed.
    let mut g = dd_game(LIB, &[], &[], Some(1.0), true);
    cast_doomsday(&mut g);
    let four = sorted(&["Brainstorm", "Consider", "Dark Ritual", "Island"]);
    pick_lib(&mut g, &four[..3]);
    let keys = g.choose(Choice::Def(def(four[3])));
    assert_eq!(g.kind(), Some(DecisionKind::PutOnTop), "{}", opts(&g));
    assert!(g.player(0).library.iter().any(|&c| g.name(c) == ORACLE));
    assert!(keys.iter().any(|&k| engine::hidden::key_kind(k) == engine::hidden::KEY_AUTO), "the forced answer is keyed");
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
}

#[test]
fn the_draw_is_the_games_and_every_world_keeps_it() {
    // At a pick: the draw, the records and the options are the same in every world of either viewer.
    let mut g = dd_game(LIB, &[], &[], Some(0.5), true);
    cast_doomsday(&mut g);
    choose_def(&mut g, "Brainstorm");
    let mut rng = Rng::new(7);
    for viewer in 0..2u8 {
        for _ in 0..8 {
            let mut w = g.clone();
            w.determinize(viewer, &mut rng);
            assert_eq!(w.ff_draw(), g.ff_draw());
            assert_eq!(w.forced_finds(), g.forced_finds());
            assert_eq!(w.forced_find(), g.forced_find());
            if viewer == 0 {
                assert_eq!(offered(&w), offered(&g));
            }
        }
    }
}

#[test]
fn nothing_of_it_reaches_an_encoding() {
    // Before, during and after a forced resolution, both viewers' rows equal those of the same state without the
    // option (the setting and its records dropped).
    let mut g = dd_game(LIB, &[], &[], Some(1.0), true);
    let same = |g: &Game| {
        let mut s = g.clone();
        s.strip_forced_find();
        for v in 0..2u8 {
            assert!(Row::of(g, v) == Row::of(&s, v), "P{v}'s row shows the forced find");
        }
    };
    same(&g);
    cast_doomsday(&mut g);
    same(&g);
    for n in sorted(&["Brainstorm", "Consider", "Dark Ritual", "Island"]) {
        choose_def(&mut g, n);
        same(&g);
    }
    finish(&mut g);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
    same(&g);
}

// ---- the spec ---------------------------------------------------------------------------------------------------

fn dd_delver() -> Value {
    json!({"max_turns": 60, "constructed": true,
           "decks": {"p0": Decklist::legacy_doomsday().main_names(), "p1": Decklist::izzet_delver().main_names()}})
}

fn with_ff(mut v: Value, x: Value) -> Value {
    v["forced_find"] = x;
    v
}

fn parse(v: &Value) -> Result<MtgSpec, String> {
    <MtgAz as SearchGame>::parse_spec(&v.to_string())
}

fn refused(v: &Value, want: &str) {
    match parse(v) {
        Ok(_) => panic!("accepted: {v}"),
        Err(e) => assert!(e.contains(want), "refusal {e:?} lacks {want:?}"),
    }
}

fn wheel(p: f64) -> Value {
    json!({"spell": DD, "card": ORACLE, "p": p})
}

#[test]
fn the_spec_option_and_its_refusals() {
    let sp = parse(&with_ff(dd_delver(), wheel(0.25))).unwrap();
    let f = sp.forced_find.unwrap();
    assert_eq!((f.spell, f.card, f.unless_in_hand, f.p()), (def(DD), def(ORACLE), true, 0.25));
    let sp = parse(&with_ff(dd_delver(), json!({"spell": DD, "card": ORACLE, "p": 1, "unless_in_hand": false}))).unwrap();
    assert_eq!(sp.forced_find.unwrap().threshold, engine::forced_find::ONE);
    assert!(!sp.forced_find.unwrap().unless_in_hand);
    // the games carry it
    let az = MtgAz::new_game(&sp, 2, 5).unwrap();
    assert_eq!(az.g.forced_find(), sp.forced_find);
    // refusals
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": ORACLE, "p": 1.5})), "forced_find.p");
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": ORACLE})), "forced_find.p");
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": ORACLE, "p": 1, "who": 0})), "unknown forced_find key");
    refused(&with_ff(dd_delver(), json!({"spell": "Brainstorm", "card": ORACLE, "p": 1})), "does not search a library and graveyard for a pile");
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": "No Such Card", "p": 1})), "forced_find.card");
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": DD, "p": 1})), "the same card");
    refused(&with_ff(dd_delver(), json!({"spell": DD, "card": "Griselbrand", "p": 1})), "no deck's main holds both");
    let delver = json!({"max_turns": 60, "constructed": true,
                        "decks": {"p0": Decklist::izzet_delver().main_names(), "p1": Decklist::izzet_delver().main_names()}});
    refused(&with_ff(delver, wheel(1.0)), "no deck's main holds both");
    // best-of-three (ENGINE.md 223): every game of the match carries it; refused where no deck's 75 holds both
    let mut bo3 = with_ff(dd_delver(), wheel(1.0));
    bo3["format"] = json!("bo3");
    let sp3 = parse(&bo3).unwrap();
    assert_eq!(MtgAz::new_game(&sp3, 2, 5).unwrap().g.forced_find(), sp3.forced_find);
    let mut delver3 = json!({"max_turns": 60, "constructed": true, "format": "bo3",
                             "decks": {"p0": Decklist::izzet_delver().main_names(), "p1": Decklist::izzet_delver().main_names()}});
    delver3["forced_find"] = wheel(1.0);
    refused(&delver3, "no deck's 75 holds both");
    refused(&json!({"forced_find": wheel(1.0)}), "forced_find");
    // the evaluation spec drops it
    let e: Value = serde_json::from_str(&MtgSpec::eval_json(&with_ff(dd_delver(), wheel(1.0)).to_string()).unwrap()).unwrap();
    assert!(e.get("forced_find").is_none());
    assert_eq!(parse(&e).unwrap().forced_find, None);
}

#[test]
fn the_draw_is_a_share_p_of_games_deterministic_by_seed() {
    let sp = parse(&with_ff(dd_delver(), wheel(0.3))).unwrap();
    let n = 4000u64;
    let mut forced = 0;
    for seed in 0..n {
        let a = MtgAz::new_game(&sp, 2, seed).unwrap();
        let d = a.g.ff_draw().unwrap();
        assert_eq!(MtgAz::new_game(&sp, 2, seed).unwrap().g.ff_draw(), Some(d), "deterministic");
        forced += d as u64;
    }
    let share = forced as f64 / n as f64;
    assert!((share - 0.3).abs() < 4.0 * (0.3f64 * 0.7 / n as f64).sqrt(), "share {share}");
    for p in [0.0, 1.0] {
        let sp = parse(&with_ff(dd_delver(), wheel(p))).unwrap();
        assert!((0..200).all(|s| MtgAz::new_game(&sp, 2, s).unwrap().g.ff_draw() == Some(p == 1.0)));
    }
    // without the option: none, and the game is the same game (its first decision, keys and rows)
    let plain = parse(&dd_delver()).unwrap();
    let with = parse(&with_ff(dd_delver(), wheel(1.0))).unwrap();
    for seed in 0..20 {
        let (a, b) = (MtgAz::new_game(&plain, 2, seed).unwrap(), MtgAz::new_game(&with, 2, seed).unwrap());
        assert_eq!(a.g.ff_draw(), None);
        let (mut la, mut lb) = (Vec::new(), Vec::new());
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        assert_eq!((a.to_move(), la), (b.to_move(), lb));
        for v in 0..2 {
            assert!(Row::of_az(&a, v) == Row::of_az(&b, v));
        }
    }
}

// ---- no policy target from a drawn resolution (ENGINE.md 180.12) -------------------------------------------------

/// The rest of a cast Doomsday's resolution from `g`, `ff_value_only` asked at every decision: the library picks
/// `picks` (in the search's order) while offered, else each decision's first option -> (the answers, the kinds asked).
fn value_only_through(g: &mut Game, picks: &[&str]) -> (Vec<bool>, Vec<DecisionKind>) {
    let mut want: Vec<u16> = picks.iter().map(|n| def(n)).collect();
    want.sort_unstable();
    want.reverse();
    let (mut vo, mut kinds) = (Vec::new(), Vec::new());
    while !(g.stack().is_empty() && at_priority(g)) {
        let d = g.pending_decision().expect("a decision").clone();
        vo.push(g.ff_value_only());
        kinds.push(d.kind);
        let ch = match want.last() {
            Some(&x) if d.kind == DecisionKind::SearchLibrary && g.has_option(Choice::Def(x)) => {
                want.pop();
                Choice::Def(x)
            }
            _ => d.options[0].choice,
        };
        g.choose(ch);
    }
    assert!(!g.ff_value_only(), "at priority after the resolution");
    (vo, kinds)
}

#[test]
fn a_drawn_resolution_records_no_policy_target_at_any_pile_or_order_decision() {
    // Drawn (p 1, the self-play setting): every pick and every order decision is a value-only decision - zero-core
    // searches and plays it, and records its row without options or policy (its docs/search.md 3.20); not before.
    let mut g = dd_game(LIB, &[], &[], Some(1.0), true);
    assert!(!g.ff_value_only(), "before the cast");
    cast_doomsday(&mut g);
    let (vo, kinds) = value_only_through(&mut g, &["Brainstorm", "Consider", "Dark Ritual", "Island"]);
    assert_eq!(g.forced_finds()[0].class, FfClass::Forced);
    assert!(kinds.contains(&DecisionKind::SearchLibrary) && kinds.contains(&DecisionKind::PutOnTop), "{kinds:?}");
    assert!(vo.iter().all(|&v| v), "{vo:?} at {kinds:?}");
    // an Oracle the player picks itself in a drawn resolution: the same (the earlier picks knew it was coming)
    // (Personal Tutor and Flow State are legacy cards, after the Oracle in definition order: the Oracle is not the last pick)
    const LIB2: &[&str] = &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal", "Island", "Personal Tutor", "Flow State"];
    let mut picks: Vec<&str> = LIB2.iter().copied().filter(|&n| def(n) < def(ORACLE)).take(3).collect();
    picks.extend([ORACLE, *LIB2.iter().find(|&&n| def(n) > def(ORACLE)).unwrap()]);
    let mut g = dd_game(LIB2, &[], &[], Some(1.0), true);
    cast_doomsday(&mut g);
    let (vo, _) = value_only_through(&mut g, &picks);
    assert_eq!(g.forced_finds()[0].class, FfClass::Voluntary);
    assert!(vo.len() >= 5 && vo.iter().all(|&v| v), "{vo:?}");
    // the graveyard's Oracle: the graveyard part's picks too
    let gy = ["Daze", ORACLE, "Ponder", "Consider", "Dark Ritual", "Lotus Petal"];
    let mut g = dd_game(&["Island", "Island"], &gy, &[], Some(1.0), true);
    cast_doomsday(&mut g);
    let (vo, kinds) = value_only_through(&mut g, &[]);
    // (the first option each time: the graveyard's Oracle is picked in position order, voluntary)
    assert_eq!(g.forced_finds()[0].class, FfClass::Voluntary);
    assert!(kinds.contains(&DecisionKind::ChooseGraveyardCard) && vo.iter().all(|&v| v), "{vo:?} at {kinds:?}");
    // undrawn (p 0): every decision with its policy target, the Oracle missed
    let mut g = dd_game(LIB, &[], &[], Some(0.0), true);
    cast_doomsday(&mut g);
    let (vo, _) = value_only_through(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Island"]);
    assert_eq!(g.forced_finds()[0].class, FfClass::Missed);
    assert!(!vo.is_empty() && vo.iter().all(|&v| !v), "{vo:?}");
    // drawn but the forcing does not apply - an Oracle in hand (unless_in_hand), none to find - and without the option:
    // with their policy targets
    let mut g = dd_game(&["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal", ORACLE], &[], &[ORACLE], Some(1.0), true);
    cast_doomsday(&mut g);
    let (vo, _) = value_only_through(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Lotus Petal"]);
    assert_eq!((g.forced_finds()[0].drawn, g.forced_finds()[0].class), (true, FfClass::InHand));
    assert!(!vo.is_empty() && vo.iter().all(|&v| !v), "{vo:?}");
    let mut g = Scenario::new(db())
        .spec(|s| s.forced_find = Some(ff(1.0)))
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &[DD])
        .library(0, &["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal"])
        .exile(0, &[ORACLE])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    cast_doomsday(&mut g);
    let (vo, _) = value_only_through(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Lotus Petal"]);
    assert_eq!((g.forced_finds()[0].drawn, g.forced_finds()[0].class), (true, FfClass::NotFindable));
    assert!(!vo.is_empty() && vo.iter().all(|&v| !v), "{vo:?}");
    let mut g = dd_game(LIB, &[], &[], None, true);
    cast_doomsday(&mut g);
    let (vo, _) = value_only_through(&mut g, &["Brainstorm", "Ponder", "Consider", "Daze", "Island"]);
    assert!(g.forced_finds().is_empty() && !vo.is_empty() && vo.iter().all(|&v| !v));
    // the same in every world of either viewer (a function of what the decider sees)
    let mut g = dd_game(LIB, &[], &[], Some(1.0), true);
    cast_doomsday(&mut g);
    choose_def(&mut g, "Brainstorm");
    assert!(g.ff_value_only());
    let mut rng = Rng::new(11);
    for viewer in 0..2u8 {
        for _ in 0..8 {
            let mut w = g.clone();
            w.determinize(viewer, &mut rng);
            assert!(w.ff_value_only(), "P{viewer}'s world");
        }
    }
}

#[test]
fn the_search_game_hook_is_the_engines_and_off_outside_a_game() {
    // MtgAz's `value_only_decision` is `Game::ff_value_only` during a game: a spec's game at its first decision (not a
    // resolution) answers false, with the option and without.
    for v in [dd_delver(), with_ff(dd_delver(), wheel(1.0))] {
        let sp = parse(&v).unwrap();
        for seed in 0..5 {
            let a = MtgAz::new_game(&sp, 2, seed).unwrap();
            assert!(!a.value_only_decision() && !a.g.ff_value_only());
        }
    }
}
