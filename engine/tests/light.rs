//! Light decisions (ENGINE.md 77, `az/light.rs`; zero-core docs/search.md 3.10): the timing windows
//! - a priority decision with an empty stack outside the decider's own main phases - and their
//! classes, on constructed positions of the Delver mirror's cards (window / not a window: another
//! step, whose turn, a spell on the stack, another kind of decision), and on every decision of whole
//! Delver games against the measurement's classifier (E:/mtg-zero-scratch/measure-play/measure.py's
//! `category`, read from the spectator view the replays store): the same windows, the same classes;
//! no trigger ever waiting at a priority decision, no priority in untap or setup; reading the hook
//! never changes a game.
use engine::az::light::{self, CLASSES};
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::DecisionKind;
use engine::testkit::Scenario;
use engine::{Choice, Game, Step, Zone};
use zero_search::SearchGame;

fn scen() -> Scenario {
    Scenario::new(CardDb::constructed()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

fn class(g: &Game) -> Option<&'static str> {
    light::window(g).map(|c| CLASSES[c])
}

fn decider(g: &Game) -> u8 {
    g.pending_decision().expect("a decision").player
}

/// The Delver mirror (both seats the pinned 60, mulligans on).
fn delver_mirror() -> MtgSpec {
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    MtgSpec::parse(&serde_json::json!({"max_turns": 60, "mulligans": true, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string()).unwrap()
}

#[test]
fn the_classes() {
    assert_eq!(<MtgAz as SearchGame>::LIGHT_CLASSES, CLASSES);
    assert_eq!(CLASSES.len(), 22);
    let mut seen = std::collections::BTreeSet::new();
    assert!(CLASSES.iter().all(|c| seen.insert(*c)), "a class twice");
    let steps = [
        (Step::Upkeep, "upkeep"),
        (Step::Draw, "draw"),
        (Step::Main1, "main1"),
        (Step::BeginCombat, "begin_combat"),
        (Step::DeclareAttackers, "declare_attackers"),
        (Step::DeclareBlockers, "declare_blockers"),
        (Step::FirstStrikeDamage, "first_strike_damage"),
        (Step::CombatDamage, "combat_damage"),
        (Step::EndCombat, "end_combat"),
        (Step::Main2, "main2"),
        (Step::End, "end"),
        (Step::Cleanup, "cleanup"),
    ];
    for (step, name) in steps {
        let own = light::class_of(true, step).map(|c| CLASSES[c]);
        let opp = light::class_of(false, step).map(|c| CLASSES[c]);
        if step.is_main() {
            assert_eq!(own, None, "the decider's own {name} is no window");
        } else {
            assert_eq!(own.unwrap(), format!("own_{name}"));
        }
        assert_eq!(opp.unwrap(), format!("opp_{name}"));
    }
    for step in [Step::Setup, Step::Untap] {
        assert_eq!((light::class_of(true, step), light::class_of(false, step)), (None, None));
    }
}

#[test]
fn windows_on_constructed_positions() {
    // the own upkeep, then the opponent's priority in it
    let mut g = scen().battlefield(0, &["Volcanic Island", "Volcanic Island"]).hand(0, &["Brainstorm", "Lightning Bolt"]).hand(1, &["Brainstorm"]).at(Step::Upkeep, 0).start();
    assert_eq!((g.kind(), decider(&g)), (Some(DecisionKind::Priority), 0));
    assert_eq!(class(&g), Some("own_upkeep"));
    g.pass();
    assert_eq!(decider(&g), 1);
    assert_eq!(class(&g), Some("opp_upkeep"));
    g.pass();
    // the own draw step
    g.drive_until(|g| g.turn().step == Step::Draw && g.kind() == Some(DecisionKind::Priority));
    assert_eq!((class(&g), decider(&g)), (Some("own_draw"), 0));
    // a spell on the stack: a response, never a window, for either player
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority));
    assert!(!g.stack().is_empty());
    assert_eq!((class(&g), decider(&g)), (None, 0));
    g.pass();
    assert_eq!((class(&g), decider(&g)), (None, 1));
    // resolved (Brainstorm's put-back choices answered): the stack empty again, the own draw step
    g.pass();
    for _ in 0..10 {
        if g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        assert_eq!(class(&g), None, "a resolving spell's choice ({:?}) is no window", g.kind());
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert!(g.stack().is_empty());
    assert_eq!((class(&g), decider(&g)), (Some("own_draw"), 0));
    // the own main phase: no window; the opponent's priority in it: one
    g.drive_until(|g| g.turn().step == Step::Main1 && g.kind() == Some(DecisionKind::Priority));
    assert_eq!((class(&g), decider(&g)), (None, 0));
    g.pass();
    assert_eq!((class(&g), decider(&g)), (Some("opp_main1"), 1));
    // another kind of decision (Lightning Bolt's target, its payment) is never light
    g.pass();
    g.drive_until(|g| g.turn().step == Step::BeginCombat && g.kind() == Some(DecisionKind::Priority));
    assert_eq!((class(&g), decider(&g)), (Some("own_begin_combat"), 0));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    let mut kinds = Vec::new();
    for _ in 0..10 {
        if g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        assert_eq!(class(&g), None, "{:?} is no window", g.kind());
        kinds.push(g.kind().unwrap());
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert!(kinds.contains(&DecisionKind::Target), "{kinds:?}");
    assert_eq!(class(&g), None, "Bolt on the stack");
    // the end step: the active player's own, the other's opp_end
    let mut g = scen().hand(0, &["Brainstorm"]).at(Step::End, 1).start();
    assert_eq!((class(&g), decider(&g)), (Some("own_end"), 1));
    g.pass();
    assert_eq!((class(&g), decider(&g)), (Some("opp_end"), 0));
    // the opponent's second main phase, the end of combat
    let mut g = scen().at(Step::EndCombat, 1).start();
    assert_eq!((class(&g), decider(&g)), (Some("own_end_combat"), 1));
    g.pass();
    assert_eq!((class(&g), decider(&g)), (Some("opp_end_combat"), 0));
    g.pass();
    assert_eq!(g.turn().step, Step::Main2);
    assert_eq!((class(&g), decider(&g)), (None, 1));
    g.pass();
    assert_eq!((class(&g), decider(&g)), (Some("opp_main2"), 0));
}

/// The measurement's classifier (measure.py's `category` and the window's step key) on the spectator
/// view the replays store: the class name of a timing window, None otherwise.
fn measured(v: &serde_json::Value) -> Option<String> {
    let dec = &v["decision"];
    if dec["kind"] != "Priority" {
        return None;
    }
    if v["stack"].as_array().is_some_and(|s| !s.is_empty()) {
        return None;
    }
    let p = dec["player"].as_u64().unwrap();
    let own = v["active"].as_u64().unwrap() == p;
    let step = v["step"].as_str().unwrap();
    if own && (step == "main1" || step == "main2") {
        return None;
    }
    Some(format!("{}_{}", if own { "own" } else { "opp" }, step))
}

#[test]
fn whole_games_match_the_measurements_classifier() {
    let spec = delver_mirror();
    let (mut searched, mut windows, mut priority) = (0usize, 0usize, 0usize);
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for seed in 0..16u64 {
        let play = |read: bool| {
            let mut g = MtgAz::new(&spec, seed).unwrap();
            let mut rng = engine::rng::Rng::new(seed + 7);
            let (mut key, mut legal) = (Vec::new(), Vec::new());
            let mut moves = Vec::new();
            let mut seen = Vec::new();
            for _ in 0..6000 {
                if g.to_move().is_none() {
                    break;
                }
                legal.clear();
                g.legal_actions(&mut legal);
                if read {
                    let got = g.light_class().map(|c| CLASSES[c].to_string());
                    let v: serde_json::Value = serde_json::from_str(&g.export_state()).unwrap();
                    assert_eq!(got, measured(&v), "seed {seed}: {}", v["decision"]);
                    let d = g.g.pending_decision().unwrap();
                    if d.kind == DecisionKind::Priority {
                        assert!(g.g.waiting_triggers().is_empty(), "seed {seed}: a trigger waiting at a priority decision");
                        assert!(!matches!(g.g.turn().step, Step::Setup | Step::Untap), "seed {seed}: priority in {:?}", g.g.turn().step);
                    }
                    seen.push((legal.len(), d.kind == DecisionKind::Priority, got));
                }
                let a = legal[rng.below(legal.len() as u32) as usize];
                moves.push(a);
                g.step(a, &mut key).unwrap();
            }
            (moves, g.g.fingerprint(), seen)
        };
        let (m0, f0, _) = play(false);
        let (m1, f1, seen) = play(true);
        assert_eq!((m0, f0), (m1, f1), "seed {seed}: reading the hook changed the game");
        for (n, pri, c) in seen {
            if n >= 2 {
                searched += 1;
                priority += pri as usize;
                if let Some(c) = c {
                    windows += 1;
                    *by.entry(c).or_insert(0) += 1;
                }
            }
        }
    }
    // random play: windows are a good share of the searched decisions, on both sides of the table
    eprintln!("searched {searched}, priority {priority}, windows {windows}: {by:?}");
    assert!(windows * 10 > searched && windows < priority, "windows {windows} of {searched} searched, {priority} priority");
    assert!(by.keys().any(|k| k.starts_with("own_")) && by.keys().any(|k| k.starts_with("opp_")), "{by:?}");
    assert!(by.contains_key("own_upkeep") && by.contains_key("opp_upkeep"), "{by:?}");
}
