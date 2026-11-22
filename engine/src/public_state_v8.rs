//! The public-state gate's positions of layout v8's facts (ENGINE.md 232; `tests/az.rs` adds them to
//! the gate's positions in all its passes): payments in progress in each of their decisions (the
//! picks, a several-mana source's production, the source a permanent picked by its own id stands
//! for, a shared source's actor, the Phyrexian split, the pool left) and a 601.2g step's; gone objects
//! whose last known information an ability waiting or on the stack reads (a Phyrexian Metamorph that
//! left as a copy of Concealing Curtains, a Jacked Rabbit's power, a countered spell an ability about
//! it names); and a Doomsday pile mixing library finds with a graveyard pick (a find group), then
//! shuffled (a whole-library group). Built here, inside the engine: they need moves and triggers by
//! hand, which the engine's own API keeps to itself.

use crate::az::{MtgAz, MtgSpec};
use crate::db::CardDb;
use crate::decision::{Choice, DecisionKind};
use crate::game::{Game, Pos, Step};
use crate::ids::Zone;
use crate::testkit::Scenario;
use zero_search::game::SearchGame;

fn wrap(sp: &MtgSpec, g: &Game, seed: u64) -> MtgAz {
    let mut a = MtgAz::new_game(sp, 2, seed).unwrap();
    a.g = g.clone();
    a
}

fn cube_spec() -> MtgSpec {
    MtgSpec { max_turns: 60, ..MtgSpec::default() }
}

fn constructed_spec() -> MtgSpec {
    let deck: Vec<&str> = std::iter::repeat_n("Island", 60).collect();
    MtgSpec::parse(&serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": deck, "p1": deck}}).to_string()).unwrap()
}

/// The payment decisions (ENGINE.md 6, 18, 99.12, 231).
const PAY_KINDS: [DecisionKind; 7] =
    [DecisionKind::PayMana, DecisionKind::ChooseMana, DecisionKind::ChooseMode, DecisionKind::ChoosePermanent, DecisionKind::PayPhyrexian, DecisionKind::PayPool, DecisionKind::CastMana];

/// Positions of payments in progress: P0 casts Sol Ring with {U}{B} floating (the pool it leaves:
/// `PayPool`), Gitaxian Probe (the Phyrexian split), then Kappa Cannoneer (improvise; Urza's taps of
/// artifacts - a permanent picked by its own id that stands for two sources: `ChooseMode`; Black
/// Lotus's three of a colour and two Orcish Lumberjacks' sacrifices of a Forest - `ChooseMana`, and
/// which Lumberjack activates: `ChoosePermanent`), picking a different source at each pick.
pub fn payment_positions() -> Vec<(&'static str, MtgAz)> {
    let db = CardDb::cube();
    let sp = cube_spec();
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Urza, Lord High Artificer", "Sol Ring", "Black Lotus", "Orcish Lumberjack", "Orcish Lumberjack", "Forest", "Forest", "Forest"])
        .battlefield(0, &["Island", "Island", "Llanowar Elves", "Chromatic Star"])
        .battlefield(1, &["Island"])
        .hand(0, &["Sol Ring", "Gitaxian Probe", "Kappa Cannoneer"])
        .pool(0, [0, 1, 1, 0, 0, 0], 0)
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let mut out = Vec::new();
    let mut seen = [0usize; 7];
    let mut turn = 0usize;
    for (spell, pass_first) in [("Sol Ring", true), ("Gitaxian Probe", false), ("Kappa Cannoneer", false)] {
        let Some(c) = g.player(0).hand.iter().copied().find(|&c| g.name(c) == spell) else { continue };
        if !g.has_option(Choice::Cast(c)) {
            continue;
        }
        g.choose(Choice::Cast(c));
        for _ in 0..200 {
            let Some(d) = g.pending_decision() else { break };
            if d.kind == DecisionKind::Priority {
                break;
            }
            if let Some(k) = PAY_KINDS.iter().position(|&x| x == d.kind) {
                seen[k] += 1;
                out.push(("payment", wrap(&sp, &g, 232)));
            }
            let opts = &d.options;
            let ch = match d.kind {
                DecisionKind::PayMana if pass_first && opts.iter().any(|o| o.choice == Choice::Pass) => Choice::Pass,
                // (A permanent picked by its own id first, then the others in turn.)
                DecisionKind::PayMana => {
                    turn += 1;
                    let by_perm: Vec<Choice> = opts.iter().filter(|o| matches!(o.choice, Choice::Card(_))).map(|o| o.choice).collect();
                    let other: Vec<Choice> = opts.iter().filter(|o| o.choice != Choice::Pass).map(|o| o.choice).collect();
                    if turn % 2 == 1 && !by_perm.is_empty() {
                        by_perm[turn / 2 % by_perm.len()]
                    } else if !other.is_empty() {
                        other[turn % other.len()]
                    } else {
                        opts[0].choice
                    }
                }
                DecisionKind::CastMana => opts.iter().find(|o| o.choice == Choice::Pass).unwrap_or(&opts[0]).choice,
                _ => opts[turn % opts.len()].choice,
            };
            g.choose(ch);
        }
        g.drive_until(|g| g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority));
    }
    let need = [DecisionKind::PayMana, DecisionKind::ChooseMana, DecisionKind::ChooseMode, DecisionKind::PayPhyrexian, DecisionKind::PayPool];
    for k in need {
        let i = PAY_KINDS.iter().position(|&x| x == k).unwrap();
        assert!(seen[i] > 0, "the payment positions: no {k:?} ({seen:?})\n{}", g.log_lines().join("\n"));
    }
    out
}

/// Positions of gone objects whose last known information (CR 608.2h) an ability reads:
/// * constructed play's table: P0's Phyrexian Metamorph enters as a copy of P1's Concealing Curtains
///   (mana value 1, not its card's 4); a triggered ability whose source it is waits (built by hand:
///   Terminal Velocity's granted leave trigger is EoE's, not in this table) and the Metamorph goes to
///   the graveyard - the trigger's source is a gone copy;
/// * the cube: Jacked Rabbit with three +1/+1 counters attacks, its trigger on the stack ("equal to
///   Jacked Rabbit's power"), and the Rabbit leaves (ENGINE.md 226's test);
/// * the cube: Thor, God of Thunder's trigger about a spell ("that spell's mana value") on the stack,
///   and the spell leaves the stack (countered, by hand): `Game::spell_lki`.
pub fn lki_positions() -> Vec<(&'static str, MtgAz)> {
    let mut out = Vec::new();
    // The Metamorph as a copy.
    let csp = constructed_spec();
    let db = CardDb::constructed();
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .battlefield(1, &["Concealing Curtains // Revealing Eye", "Jacked Rabbit"])
        .hand(0, &["Phyrexian Metamorph"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let m = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    let cur = g.card(1, "Concealing Curtains // Revealing Eye", Zone::Battlefield);
    g.choose(Choice::Cast(m));
    for _ in 0..200 {
        if g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        let d = g.pending_decision().expect("a decision");
        let ch = match d.kind {
            DecisionKind::Priority => Choice::Pass,
            DecisionKind::ChoosePermanent if g.has_option(Choice::Card(cur)) => Choice::Card(cur),
            _ => d.options.iter().find(|o| o.choice != Choice::Pass).unwrap_or(&d.options[0]).choice,
        };
        g.choose(ch);
    }
    assert!(g.objs[m.i()].zone == Zone::Battlefield && g.is_copy(m), "the Metamorph as a copy of Curtains\n{}", g.log_lines().join("\n"));
    let mr = g.obj_ref(m);
    let def = g.objs[m.i()].def;
    g.waiting.push(crate::triggers::PendingTrigger { controller: 0, source: mr, source_def: def, slot: 0, ev: Default::default() });
    g.move_card(m, Zone::Graveyard, Pos::Top);
    g.refresh_decision();
    assert!(g.lki_copy.iter().any(|x| x.0 == mr && x.2 == 1), "the copy's last known mana value 1");
    out.push(("lki", wrap(&csp, &g, 233)));
    // Jacked Rabbit's power.
    let sp = cube_spec();
    let db = CardDb::cube();
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Jacked Rabbit"])
        .counters(0, "Jacked Rabbit", crate::db::Counter::P1P1, 3)
        .at(Step::DeclareAttackers, 0)
        .turn_number(3)
        .start();
    let c = g.card(0, "Jacked Rabbit", Zone::Battlefield);
    let attacked = g.obj_ref(c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(Choice::Target(crate::ids::Target::Player(1)));
    }
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && !g.stack().is_empty());
    out.push(("lki", wrap(&sp, &g, 234)));
    g.move_card(c, Zone::Graveyard, Pos::Top);
    g.refresh_decision();
    assert_eq!(g.lki_power_of(attacked), Some(4));
    out.push(("lki", wrap(&sp, &g, 235)));
    // A spell gone from the stack while an ability about it waits there (Thor's "that spell's mana value").
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Forest"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Thor, God of Thunder", "Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Green Sun's Zenith"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let gsz = g.card(0, "Green Sun's Zenith", Zone::Hand);
    g.choose(Choice::Cast(gsz));
    for _ in 0..100 {
        let d = g.pending_decision().expect("a decision");
        if d.kind == DecisionKind::Priority && g.stack().len() >= 2 {
            break;
        }
        let ch = match d.kind {
            DecisionKind::ChooseMode if g.has_option(Choice::Number(2)) => Choice::Number(2),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options.iter().find(|o| o.choice != Choice::Pass).unwrap_or(&d.options[0]).choice,
        };
        g.choose(ch);
    }
    if g.stack().len() >= 2 && g.objs[gsz.i()].zone == Zone::Stack {
        let r = g.obj_ref(gsz);
        g.move_card(gsz, Zone::Graveyard, Pos::Top);
        g.refresh_decision();
        assert!(g.spell_lki.iter().any(|x| x.0 == r), "the countered spell's item is kept\n{}", g.log_lines().join("\n"));
        out.push(("lki", wrap(&sp, &g, 238)));
    } else {
        panic!("Thor's trigger about Green Sun's Zenith never waited on the stack\n{}", g.log_lines().join("\n"));
    }
    out
}

/// Positions of a find group (ENGINE.md 223's decision 4): P0's Doomsday pile of four library finds and Force of
/// Will from the graveyard, ordered privately (a group of the top five places: four finds, one other card), then a
/// fetch land that finds nothing and shuffles (the whole library a group: four finds).
pub fn find_group_positions() -> Vec<(&'static str, MtgAz)> {
    let db = CardDb::constructed();
    let sp = constructed_spec();
    let pile = ["Thassa's Oracle", "Force of Will", "Brainstorm", "Ponder", "Consider"];
    let gy = ["Force of Will"];
    let lib = ["Brainstorm", "Ponder", "Consider", "Daze", "Thassa's Oracle", "Dark Ritual", "Lotus Petal", "Island", "Underground Sea", "Brainstorm"];
    let mut g = Scenario::new(db)
        .spec(|s| {
            s.auto_single = false;
            s.deck_clouds = true;
        })
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Polluted Delta"])
        .hand(0, &["Doomsday"])
        .library(0, &lib)
        .graveyard(0, &gy)
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let dd = g.card(0, "Doomsday", Zone::Hand);
    g.choose(Choice::Cast(dd));
    while g.kind() != Some(DecisionKind::Priority) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    g.pass_until(|g| g.kind() != Some(DecisionKind::Priority));
    let fow = g.card(0, "Force of Will", Zone::Graveyard);
    while g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
        if g.has_option(Choice::Card(fow)) && g.objs[fow.i()].zone == Zone::Graveyard {
            g.choose(Choice::Card(fow));
        } else {
            g.pass();
        }
    }
    let mut picks: Vec<u16> = pile.iter().filter(|n| !gy.contains(n)).map(|n| db.expect_id(n).unwrap()).collect();
    picks.sort();
    for d in picks {
        g.choose(Choice::Def(d));
    }
    for want in pile.iter().rev() {
        let pick = g.pending_decision().unwrap().options.iter().find_map(|o| match o.choice {
            Choice::Card(c) if g.name(c) == *want => Some(o.choice),
            _ => None,
        });
        g.choose(pick.expect("the pile's next card"));
    }
    g.drive_until(|g| g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority));
    assert_eq!(g.find_groups.len(), 1, "the pile's group\n{}", g.log_lines().join("\n"));
    assert_eq!((g.find_groups[0].members.len(), g.find_groups[0].finds), (5, 4));
    let mut out = vec![("find_groups", wrap(&sp, &g, 239))];
    let fetch = g.card(0, "Polluted Delta", Zone::Battlefield);
    let opt = g.pending_decision().unwrap().options.iter().find(|o| matches!(o.choice, Choice::Ability(c, _) if c == fetch)).map(|o| o.choice);
    if let Some(ch) = opt {
        g.choose(ch);
        for _ in 0..50 {
            if g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority) {
                break;
            }
            let d = g.pending_decision().unwrap();
            let ch = match d.kind {
                DecisionKind::Priority => Choice::Pass,
                _ => d.options.iter().find(|o| o.choice == Choice::Pass).unwrap_or(&d.options[0]).choice,
            };
            g.choose(ch);
        }
        out.push(("find_groups", wrap(&sp, &g, 240)));
    }
    out
}
