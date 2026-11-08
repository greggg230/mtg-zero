//! Workstream C of the faithfulness audit (docs/faithfulness_audit.md 5.5,
//! 5.6; ENGINE.md 100.3): rules timing and characteristics. Each test names
//! the CR rule it checks and failed before its fix.

mod common;

use engine::db::{CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

fn card(name: &str, printing: Value) -> Value {
    let mut p = printing;
    p["$t"] = json!("CardPrinting");
    p["name"] = json!(name);
    for (k, v) in [
        ("mana_cost", json!({"$t": "ManaCost"})),
        ("subtypes", json!([])),
        ("supertypes", json!([])),
        ("mana_cost_str", json!("")),
        ("type_line", json!("")),
    ] {
        if p.get(k).is_none() {
            p[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": p})
}

fn mana(generic: i64, colored: &[(&str, i64)]) -> Value {
    let mut m = json!({"$t": "ManaCost"});
    if generic > 0 {
        m["generic"] = json!(generic);
    }
    if !colored.is_empty() {
        m["colored"] = json!(colored.iter().map(|(c, n)| json!([c, n])).collect::<Vec<_>>());
    }
    m
}

/// This suite's own test cards, written as card IR (the shapes real cards
/// have), lowered with the cube and the Edge of Eternities table.
fn c_cards() -> Vec<card_ir::Card> {
    let v = vec![
        // "Whenever a creature dies this turn, you gain 1 life." (A "this
        // turn" triggered ability, Forth Eorlingas!'s shape.)
        card(
            "C Grim Vigil",
            json!({"card_types": ["Instant"], "mana_cost": mana(0, &[("B", 1)]), "mana_cost_str": "{B}",
                "spell_effect": {"$t": "CreateThisTurnTriggeredAbility", "triggered_abilities": [{"$t": "TriggeredAbility",
                    "event": "dies", "predicate": {"$t": "Subject", "inner": {"$t": "IsCreature"}, "slot": null},
                    "effect": {"$t": "GainLife", "amount": 1}}]}}),
        ),
        // "Target creature gets +2/+2 until end of turn."
        card(
            "C Brace",
            json!({"card_types": ["Instant"], "mana_cost": mana(0, &[("G", 1)]), "mana_cost_str": "{G}",
                "spell_effect": {"$t": "PumpTargetCreatureUntilEOT", "power_bonus": 2, "toughness_bonus": 2}}),
        ),
        // A 2/2 with "{T}: Put two -1/-1 counters on this creature."
        card(
            "C Withering Bear",
            json!({"card_types": ["Creature"], "subtypes": ["Bear"], "power": 2, "toughness": 2,
                "mana_cost": mana(1, &[("G", 1)]), "mana_cost_str": "{1}{G}",
                "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                    "effect": {"$t": "PutNamedCountersOnSelf", "kind": "-1/-1", "amount": 2}}]}),
        ),
        // "After this phase, there is an additional combat phase." (Fear of
        // Missing Out's sentence, on an instant.)
        card(
            "C Second Wind",
            json!({"card_types": ["Instant"], "mana_cost": mana(0, &[("R", 1)]), "mana_cost_str": "{R}",
                "spell_effect": {"$t": "AdditionalCombatPhaseAfterThisPhase"}}),
        ),
        // "You may have this enter as a copy of a creature on the battlefield,
        // except it's an Elf Wizard Pirate in addition to its other types."
        card(
            "C Mimic A",
            json!({"card_types": ["Creature"], "subtypes": ["Shapeshifter"], "power": 0, "toughness": 0,
                "mana_cost": mana(2, &[("U", 1)]), "mana_cost_str": "{2}{U}",
                "enters_as_copy_types": ["Creature"], "enters_as_copy_add_subtypes": ["Elf", "Wizard", "Pirate"]}),
        ),
        // The same, "except it's a Ninja Rogue in addition to its other types".
        card(
            "C Mimic B",
            json!({"card_types": ["Creature"], "subtypes": ["Shapeshifter"], "power": 0, "toughness": 0,
                "mana_cost": mana(2, &[("U", 1)]), "mana_cost_str": "{2}{U}",
                "enters_as_copy_types": ["Creature"], "enters_as_copy_add_subtypes": ["Ninja", "Rogue"]}),
        ),
        // A double-faced artifact creature with no abilities (its back face
        // bigger).
        card(
            "C Twofold Relic",
            json!({"card_types": ["Artifact", "Creature"], "subtypes": ["Construct"], "power": 1, "toughness": 1,
                "mana_cost": mana(2, &[]), "mana_cost_str": "{2}",
                "transform_back_face": {"$t": "CardPrinting", "name": "C Twofold Idol", "card_types": ["Artifact", "Creature"],
                    "subtypes": ["Construct"], "supertypes": [], "power": 3, "toughness": 3, "face_colors": [],
                    "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "type_line": "Artifact Creature - Construct"}}),
        ),
        // "You may have this enter as a copy of an enchantment on the
        // battlefield."
        card(
            "C Echo",
            json!({"card_types": ["Enchantment"], "mana_cost": mana(2, &[("U", 1)]), "mana_cost_str": "{2}{U}",
                "enters_as_copy_types": ["Enchantment"]}),
        ),
        // "... as a copy of a land, except it's a Forest in addition to its
        // other types": refused (a basic land type from an effect, N22).
        card(
            "C Forest Mimic",
            json!({"card_types": ["Land"], "enters_as_copy_types": ["Land"], "enters_as_copy_add_subtypes": ["Forest"]}),
        ),
        // A battle (a Siege's shape without its back face): refused until CR
        // 310 is built.
        card(
            "C Siege",
            json!({"card_types": ["Battle"], "subtypes": ["Siege"], "mana_cost": mana(2, &[("R", 1)]), "mana_cost_str": "{2}{R}"}),
        ),
    ];
    v.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// This suite's cards the lowering must refuse.
const UNSUPPORTED: &[&str] = &["C Siege", "C Forest Mimic"];

/// The cube, the Edge of Eternities cards it doesn't have, the shared test
/// cards and this suite's, lowered once.
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        for c in engine::db::load_eoe_document().cards {
            if !cards.iter().any(|x| x.name == c.name) {
                cards.push(c);
            }
        }
        cards.extend(common::custom_cards());
        cards.extend(c_cards());
        let db = CardDb::from_cards(&cards);
        for c in c_cards().iter().filter(|c| !UNSUPPORTED.contains(&c.name.as_str())) {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

/// Answer decisions (`answer`, else the first option) until priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

/// Run the stack down; `answer` answers what is asked (else the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", log(g));
}

fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(c))).then(|| g.tgt(c))
}

/// Cast `name` from P's hand, answering with `answer`, and resolve it.
fn cast_resolve(g: &mut Game, p: PlayerId, name: &str, answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let c = g.card(p, name, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {:?}", g.options_desc());
    g.choose(Choice::Cast(c));
    finish(g, answer);
    resolve_with(g, answer);
}

// ---- 32.5.2: "this turn" triggered abilities end in the cleanup step (CR 514.2) ---------------

/// A "this turn" triggered ability ends as "until end of turn" effects do,
/// in the cleanup step (CR 514.2), not as the next turn begins: a creature
/// kept alive by an until-end-of-turn +2/+2 that dies to its -1/-1 counters
/// right after 514.2 doesn't trigger "whenever a creature dies this turn".
#[test]
fn cr_514_2_a_this_turn_trigger_ends_in_the_cleanup_step() {
    let mut g = scen()
        .battlefield(0, &["C Withering Bear", "Forest", "Swamp"])
        .hand(0, &["C Brace", "C Grim Vigil"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "C Withering Bear");
    cast_resolve(&mut g, 0, "C Brace", |g| tgt_if(g, bear));
    g.choose(Choice::Ability(bear, engine::db::AbilitySlot::Activated(0)));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.counters_on(bear, Counter::M1M1), 2, "{}", log(&g));
    cast_resolve(&mut g, 0, "C Grim Vigil", |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "4/4 less two: alive\n{}", log(&g));
    // To the next turn: the bear dies in the cleanup step.
    g.drive_until(|g| g.turn().number == 2 && at_priority(g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 20, "the trigger ended at 514.2\n{}", log(&g));
}

// ---- 29.6.4: "after this phase, there is an additional combat phase" (CR 500.8) ---------------

/// The steps at which P0 gets priority from now until the next turn
/// begins, attacking with every creature it may and blocking nothing.
fn steps_to_next_turn(g: &mut Game) -> Vec<Step> {
    let turn = g.turn().number;
    let mut steps = Vec::new();
    for _ in 0..500 {
        if g.turn().number != turn || g.is_over() {
            return steps;
        }
        match g.kind() {
            Some(DecisionKind::Priority) => {
                if g.pending_decision().unwrap().player == 0 && steps.last() != Some(&g.turn().step) {
                    steps.push(g.turn().step);
                }
                g.pass();
            }
            Some(DecisionKind::DeclareBlockers) => {
                g.choose(Choice::Pass);
            }
            _ => {
                let c = options(g).into_iter().find(|c| !matches!(c, Choice::Pass)).unwrap_or(options(g)[0]);
                g.choose(c);
            }
        }
    }
    panic!("no next turn\n{}", log(g));
}

/// Resolved in the postcombat main phase, "after this phase, there is an
/// additional combat phase" adds a combat phase after the postcombat main
/// phase, and the ending phase follows it (CR 500.8): the bear attacks a
/// second time.
#[test]
fn cr_500_8_an_additional_combat_phase_after_the_postcombat_main_phase() {
    let mut g = scen().battlefield(0, &["Test Bear", "Mountain"]).hand(0, &["C Second Wind"]).at(Step::Main2, 0).start();
    cast_resolve(&mut g, 0, "C Second Wind", |_| None);
    assert_eq!(g.turn().step, Step::Main2);
    let steps = steps_to_next_turn(&mut g);
    assert_eq!(
        steps,
        [Step::Main2, Step::BeginCombat, Step::DeclareAttackers, Step::DeclareBlockers, Step::CombatDamage, Step::EndCombat, Step::End],
        "{}",
        log(&g)
    );
    assert_eq!(g.life(1), 18, "the bear attacked in the added combat\n{}", log(&g));
}

/// Resolved in the upkeep, it adds a combat phase after the beginning
/// phase: then the precombat main phase, its combat phase and the rest
/// (CR 500.8, 505.1a). (The bear, tapped by its first attack, can't attack
/// in the second.)
#[test]
fn cr_500_8_an_additional_combat_phase_after_the_beginning_phase() {
    let mut g = scen().battlefield(0, &["Test Bear", "Mountain"]).hand(0, &["C Second Wind"]).at(Step::Upkeep, 0).start();
    cast_resolve(&mut g, 0, "C Second Wind", |_| None);
    assert_eq!(g.turn().step, Step::Upkeep);
    let steps = steps_to_next_turn(&mut g);
    let combat = [Step::BeginCombat, Step::DeclareAttackers, Step::DeclareBlockers, Step::CombatDamage, Step::EndCombat];
    let mut want = vec![Step::Upkeep, Step::Draw];
    want.extend(combat);
    want.extend([Step::Main1, Step::BeginCombat, Step::DeclareAttackers, Step::EndCombat, Step::Main2, Step::End]);
    assert_eq!(steps, want, "{}", log(&g));
    assert_eq!(g.life(1), 18, "{}", log(&g));
}

/// Resolved in the end step, it adds a combat phase after the ending
/// phase; the turn ends after it (CR 500.8).
#[test]
fn cr_500_8_an_additional_combat_phase_after_the_ending_phase() {
    let mut g = scen().battlefield(0, &["Test Bear", "Mountain"]).hand(0, &["C Second Wind"]).at(Step::End, 0).start();
    cast_resolve(&mut g, 0, "C Second Wind", |_| None);
    let steps = steps_to_next_turn(&mut g);
    assert_eq!(
        steps,
        [Step::End, Step::BeginCombat, Step::DeclareAttackers, Step::DeclareBlockers, Step::CombatDamage, Step::EndCombat],
        "{}",
        log(&g)
    );
    assert_eq!(g.life(1), 18, "{}", log(&g));
    assert_eq!(g.turn().active, 1, "then P1's turn");
}

/// The other "this turn" effects end in the cleanup step too (CR 514.2):
/// after Yawgmoth's Will, a creature dying to a state-based action right
/// after 514.2 goes to the graveyard (its "exile it instead" has ended), and
/// with priority in that cleanup step (514.3a) no spell may be cast from the
/// graveyard ("until end of turn, you may ... cast spells from your
/// graveyard").
#[test]
fn cr_514_2_yawgmoths_will_ends_in_the_cleanup_step() {
    let mut g = scen()
        .battlefield(0, &["C Withering Bear", "Forest", "Swamp", "Swamp", "Swamp", "Mountain"])
        .hand(0, &["C Brace", "Yawgmoth's Will"])
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "C Withering Bear");
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    cast_resolve(&mut g, 0, "C Brace", |g| tgt_if(g, bear));
    g.choose(Choice::Ability(bear, engine::db::AbilitySlot::Activated(0)));
    resolve_with(&mut g, |_| None);
    cast_resolve(&mut g, 0, "Yawgmoth's Will", |_| None);
    assert!(g.has_option(Choice::Cast(bolt)), "castable from the graveyard this turn");
    g.drive_until(|g| g.turn().step == Step::Cleanup && at_priority(g) || g.turn().number == 2);
    assert_eq!(g.turn().step, Step::Cleanup, "priority in the cleanup step (514.3a)\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "not exiled: the replacement ended at 514.2\n{}", log(&g));
    if g.pending_decision().unwrap().player == 0 {
        assert!(!g.has_option(Choice::Cast(bolt)), "the permission ended at 514.2: {:?}", g.options_desc());
    }
}

// ---- 13.10: battles are refused until CR 310 is built -------------------------------------------

/// A battle has defense counters, a protector and can be attacked (CR 310);
/// the engine has none of it, so a card with the type is refused loudly
/// (AGENTS.md: no silent drops), not played as a permanent of no type.
#[test]
fn cr_310_a_battle_card_is_refused() {
    let why = db().unsupported().get("C Siege").unwrap_or_else(|| panic!("C Siege must be unsupported"));
    assert!(why.iter().any(|w| w.contains("Battle")), "{why:?}");
    assert!(!db().supported().any(|n| n == "C Siege"));
}

// ---- 26.11.5: a copy of a copy keeps every exception (CR 707.9b) ------------------------------

/// A copy of a permanent that is copying something with exceptions copies
/// them too (CR 707.9b: the exceptions are part of the copiable values), and
/// adds its own: a copy of a "Bear, Elf Wizard Pirate" made by "except it's a
/// Ninja Rogue" has all six subtypes (the old exception list held four).
#[test]
fn cr_707_9b_a_copy_of_a_copy_has_every_exception_subtype() {
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["C Mimic A", "C Mimic B"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let (a, b) = (g.card(0, "C Mimic A", Zone::Hand), g.card(0, "C Mimic B", Zone::Hand));
    cast_resolve(&mut g, 0, "C Mimic A", |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    assert_eq!(g.name(a), "Test Bear", "a copy of the bear\n{}", log(&g));
    cast_resolve(&mut g, 0, "C Mimic B", |g| g.has_option(Choice::Card(a)).then_some(Choice::Card(a)));
    assert_eq!(g.obj(b).zone, Zone::Battlefield);
    assert_eq!(g.name(b), "Test Bear", "{}", log(&g));
    for s in ["Test", "Elf", "Wizard", "Pirate", "Ninja", "Rogue"] {
        let id = db().subtype_id(s).unwrap();
        assert!(g.has_subtype(b, id), "{s}\n{}", log(&g));
    }
    assert!(!g.has_subtype(b, db().subtype_id("Shapeshifter").unwrap()));
}

// ---- 28.8.4: Painter's Servant's colour in timestamp order (CR 613.7) -------------------------

/// Celestial Colonnade animated ("becomes a white and blue Elemental"), then
/// Painter's Servant naming red: layer 5's effects apply in timestamp order
/// (CR 613.7), so the later Painter adds red to the earlier "becomes white
/// and blue". Painter first, then the animation: white and blue only.
#[test]
fn cr_613_7_painters_servant_after_a_becomes_colour_effect_adds_its_colour() {
    const W: u8 = 1;
    const U: u8 = 2;
    const R: u8 = 8;
    for painter_first in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Celestial Colonnade", "Plains", "Plains", "Island", "Island", "Island", "Island", "Island"])
            .hand(0, &["Painter's Servant"])
            .at(Step::Main1, 0)
            .start();
        let cc = bf(&g, 0, "Celestial Colonnade");
        let painter = |g: &mut Game| {
            cast_resolve(g, 0, "Painter's Servant", |g| {
                (g.kind() == Some(DecisionKind::ChooseColor)).then_some(Choice::Number(3))
            });
        };
        if painter_first {
            painter(&mut g);
        }
        // Colonnade's animation: its activated ability (not its mana ones).
        let anim = (0..4u8)
            .map(engine::db::AbilitySlot::Activated)
            .find(|&s| g.has_option(Choice::Ability(cc, s)))
            .expect("the animation");
        g.choose(Choice::Ability(cc, anim));
        resolve_with(&mut g, |_| None);
        assert!(g.is_creature(cc), "animated\n{}", log(&g));
        if !painter_first {
            painter(&mut g);
        }
        let want = if painter_first { W | U } else { W | U | R };
        assert_eq!(g.colors_of(cc), want, "painter first: {painter_first}\n{}", log(&g));
    }
}

// ---- N4: granted graveyard casting judged by the face being cast (CR 601.3e) ------------------

/// The ways P0's priority offers to cast `c`'s other face: `CastFace`, or a
/// `Faces` choice with the other face castable.
fn other_face_offered(g: &Game, c: CardIdx) -> Option<Choice> {
    options(g).into_iter().find(|o| match *o {
        Choice::CastFace(x) => x == c,
        Choice::Faces(x, m) => x == c && m & engine::decision::OTHER_CAST != 0,
        _ => false,
    })
}

fn front_offered(g: &Game, c: CardIdx) -> bool {
    options(g).into_iter().any(|o| match o {
        Choice::Cast(x) => x == c,
        Choice::Faces(x, m) => x == c && m & engine::decision::FACE_CAST != 0,
        _ => false,
    })
}

/// Wrenn and Six's emblem ("instant and sorcery cards in your graveyard have
/// retrace") lets Brazen Borrower's Adventure, Petty Theft (an instant), be
/// cast from the graveyard with retrace, and not the creature: the
/// characteristics of the spell being cast decide (Lier, Disciple of the
/// Drowned's ruling of 2025-10-02; CR 601.3e).
#[test]
fn cr_601_3e_retrace_granted_to_instants_casts_an_adventure_from_the_graveyard() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Island"])
        .graveyard(0, &["Brazen Borrower // Petty Theft"])
        .battlefield(1, &["Test Bear"])
        .emblem(0, "Wrenn and Six")
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Graveyard);
    let bear = bf(&g, 1, "Test Bear");
    assert!(!front_offered(&g, bb), "the creature isn't an instant: {:?}", g.options_desc());
    let c = other_face_offered(&g, bb).unwrap_or_else(|| panic!("Petty Theft by retrace: {:?}", g.options_desc()));
    g.choose(c);
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseFace) => Some(Choice::Number(1)),
        _ => tgt_if(g, bear),
    });
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Hand, "Petty Theft resolved\n{}", log(&g));
    assert_eq!(g.find_card(0, "Island", Zone::Hand), None, "retrace's land discarded\n{}", log(&g));
}

/// Six ("during your turn, nonland permanent cards in your graveyard have
/// retrace") lets Brazen Borrower be cast as the creature with retrace, not
/// as its Adventure, an instant (the same ruling, the other way round).
#[test]
fn cr_601_3e_retrace_granted_to_permanent_cards_doesnt_cast_the_adventure() {
    let g = scen()
        .battlefield(0, &["Six", "Island", "Island", "Island"])
        .hand(0, &["Island"])
        .graveyard(0, &["Brazen Borrower // Petty Theft"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Graveyard);
    assert!(front_offered(&g, bb), "the creature by retrace: {:?}", g.options_desc());
    assert_eq!(other_face_offered(&g, bb), None, "not Petty Theft: {:?}", g.options_desc());
}

// ---- N1: a 616.1 choice is made before the event (no provisional exile) -----------------------

/// Blightsteel Colossus sacrificed while the opponent controls Dauthi
/// Voidwalker: its controller chooses shuffled into the library or exiled
/// with a void counter (CR 616.1), before the event. While the choice is
/// pending the card is in no zone (not exile), and Syr Vondam's "whenever
/// another creature you control dies or is put into exile" triggers only if
/// it is exiled.
#[test]
fn cr_616_1_the_replacement_choice_comes_before_the_event_syr_vondam() {
    for (k, want_counters) in [(1u8, 0u32), (3, 1)] {
        let mut g = scen()
            .battlefield(0, &["Syr Vondam, Sunstar Exemplar", "Blightsteel Colossus", "Goblin Bombardment"])
            .battlefield(1, &["Dauthi Voidwalker"])
            .at(Step::Main1, 0)
            .start();
        let bs = bf(&g, 0, "Blightsteel Colossus");
        let sv = bf(&g, 0, "Syr Vondam, Sunstar Exemplar");
        let gb = bf(&g, 0, "Goblin Bombardment");
        g.choose(Choice::Ability(gb, engine::db::AbilitySlot::Activated(0)));
        let mut asked = false;
        for _ in 0..40 {
            if at_priority(&g) {
                break;
            }
            let c = match g.kind() {
                Some(DecisionKind::Sacrifice) => Choice::Card(bs),
                Some(DecisionKind::ChooseReplacement) => {
                    asked = true;
                    assert!(!g.exile_zone().contains(&bs), "not exiled for now\n{}", log(&g));
                    assert_eq!(g.obj(bs).zone, Zone::Nowhere);
                    Choice::Number(k)
                }
                _ => g.pending_decision().unwrap().options[0].choice,
            };
            g.choose(c);
        }
        assert!(asked, "{}", log(&g));
        resolve_with(&mut g, |_| None);
        let zone = if k == 1 { Zone::Library } else { Zone::Exile };
        assert_eq!(g.obj(bs).zone, zone, "{}", log(&g));
        assert_eq!(g.counters_on(sv, Counter::P1P1), want_counters, "choice {k}\n{}", log(&g));
    }
}

// ---- 29.6.5: miracle's reveal as the card is drawn (CR 702.94a) -------------------------------

/// Brainstorm drawing the first card of the turn, a miracle card: its owner
/// may reveal it "as you draw it" (CR 702.94a), so the question comes before
/// Brainstorm's other two draws, not after the put-backs.
#[test]
fn cr_702_94a_miracle_is_revealed_as_the_card_is_drawn_before_the_next_draw() {
    let mut g = scen()
        .library(0, &["Metamorphosis Fanatic", "Mountain", "Forest"])
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .hand(0, &["Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    let fan = g.card(0, "Metamorphosis Fanatic", Zone::Library);
    g.choose(Choice::Cast(bs));
    finish(&mut g, |_| None);
    let mut seen = None;
    for _ in 0..50 {
        if at_priority(&g) && !g.stack().is_empty() {
            g.pass();
            continue;
        }
        if at_priority(&g) {
            break;
        }
        if g.kind() == Some(DecisionKind::YesNo) && g.has_option(Choice::Yes) && seen.is_none() {
            seen = Some((g.obj(fan).zone, g.find_card(0, "Mountain", Zone::Hand).is_some(), g.find_card(0, "Forest", Zone::Hand).is_some()));
            g.choose(Choice::Yes);
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(seen, Some((Zone::Hand, false, false)), "asked right after the miracle card's draw\n{}", log(&g));
    assert!(g.log_lines().iter().any(|l| l.contains("reveals Metamorphosis Fanatic (miracle)")), "{}", log(&g));
}

// ---- 13.8: waiting triggers from different objects are ordered (CR 603.3b) --------------------

/// Two Ouroboroids of different power ("at the beginning of combat on your
/// turn, put X +1/+1 counters on each creature you control, where X is this
/// creature's power"): their triggers read their own source, so they are not
/// interchangeable and their controller orders them (CR 603.3b); the two
/// orders end with different counters.
#[test]
fn cr_603_3b_triggers_of_one_ability_from_different_objects_are_ordered() {
    let mut ends = Vec::new();
    for pick in 0..2usize {
        let mut g = scen()
            .battlefield(0, &["Ouroboroid", "Ouroboroid"])
            .counters(0, "Ouroboroid", Counter::P1P1, 2)
            .at(Step::Main1, 0)
            .start();
        let os: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Ouroboroid").collect();
        assert_ne!(g.power(os[0]), g.power(os[1]));
        g.pass();
        let mut asked = 0;
        for _ in 0..50 {
            if g.kind() == Some(DecisionKind::OrderTriggers) {
                asked = options(&g).len();
                let c = options(&g)[pick];
                g.choose(c);
                continue;
            }
            if at_priority(&g) && g.stack().is_empty() && g.turn().step == Step::BeginCombat {
                break;
            }
            if at_priority(&g) {
                g.pass();
                continue;
            }
            let c = options(&g)[0];
            g.choose(c);
        }
        assert_eq!(asked, 2, "the two triggers are ordered\n{}", log(&g));
        ends.push((g.counters_on(os[0], Counter::P1P1), g.counters_on(os[1], Counter::P1P1)));
    }
    assert_ne!(ends[0], ends[1], "the orders differ: {ends:?}");
}

// ---- 39.1: the Adventure's exile is one of the replacements ordered (CR 715.4, 616.1) -------

/// Petty Theft cast from the graveyard by Bilbo, Thief in the Night's
/// attack trigger and resolving: Bilbo's "exile it instead" and the
/// Adventure's exile (CR 715.4) both apply, and its controller chooses
/// (616.1): on an adventure (the creature may be cast later from exile) or
/// just exiled (Bilbo's ruling of 2026-06-29: "you can exile it using the
/// replacement effect associated with the Adventure").
#[test]
fn cr_715_4_an_adventure_cast_with_bilbo_the_controller_orders_the_exiles() {
    const BORROWER: &str = "Brazen Borrower // Petty Theft";
    for k in [4u8, 2] {
        let mut g = scen()
            .battlefield(0, &["Bilbo, Thief in the Night", "Island", "Island", "Island", "Island"])
            .graveyard(0, &[BORROWER])
            .battlefield(1, &["Sol Ring"])
            .at(Step::DeclareAttackers, 0)
            .start();
        let bilbo = bf(&g, 0, "Bilbo, Thief in the Night");
        let b = g.card(0, BORROWER, Zone::Graveyard);
        let ring = bf(&g, 1, "Sol Ring");
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
        g.choose(Choice::Card(bilbo));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(Choice::Target(engine::Target::Player(1)));
        }
        g.choose(Choice::Pass);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
        let ch = options(&g).into_iter().find(|c| matches!(*c, Choice::Card(x) | Choice::Faces(x, _) | Choice::CastFace(x) if x == b)).unwrap();
        g.choose(ch);
        if g.kind() == Some(DecisionKind::ChooseFace) {
            g.choose(Choice::Number(1));
        }
        let tr = g.tgt(ring);
        let mut asked = Vec::new();
        for _ in 0..100 {
            if g.obj(b).zone != Zone::Stack && g.obj(b).zone != Zone::Graveyard && at_priority(&g) {
                break;
            }
            let c = match g.kind() {
                Some(DecisionKind::ChooseReplacement) => {
                    asked = options(&g);
                    Choice::Number(k)
                }
                Some(DecisionKind::Target) if g.has_option(tr) => tr,
                Some(DecisionKind::Priority) => Choice::Pass,
                _ => options(&g)[0],
            };
            g.choose(c);
        }
        assert_eq!(asked, vec![Choice::Number(2), Choice::Number(4)], "{}", log(&g));
        assert_eq!(g.obj(b).zone, Zone::Exile, "{}", log(&g));
        assert_eq!(g.obj(ring).zone, Zone::Hand);
        let adventure = g.log_lines().iter().any(|l| l.contains("goes on an adventure"));
        assert_eq!(adventure, k == 4, "choice {k}\n{}", log(&g));
    }
}

// ---- N6: Hullbreacher and Quantum Riddler on one draw (CR 616.1) -------------------------------

/// P1 draws one card (Consider) with P0's Hullbreacher out and P1's Quantum
/// Riddler ("if you would draw one or more cards, you draw that many plus
/// one instead" with one or fewer cards in hand): both replace the draw and
/// the drawer orders them (CR 616.1): Hullbreacher's first gives P0 one
/// Treasure, Quantum Riddler's first two.
#[test]
fn cr_616_1_hullbreacher_and_quantum_riddler_the_drawer_orders_them() {
    for (k, treasures) in [(5u8, 1usize), (6, 2)] {
        let mut g = scen()
            .battlefield(0, &["Hullbreacher"])
            .battlefield(1, &["Quantum Riddler", "Island"])
            .hand(1, &["Consider"])
            .at(Step::Main1, 1)
            .start();
        let c = g.card(1, "Consider", Zone::Hand);
        g.choose(Choice::Cast(c));
        finish(&mut g, |_| None);
        let mut asked = Vec::new();
        for _ in 0..100 {
            if g.stack().is_empty() && at_priority(&g) {
                break;
            }
            let ch = match g.kind() {
                Some(DecisionKind::ChooseReplacement) => {
                    asked = options(&g);
                    Choice::Number(k)
                }
                Some(DecisionKind::Priority) => Choice::Pass,
                _ => options(&g)[0],
            };
            g.choose(ch);
        }
        assert_eq!(asked, vec![Choice::Number(5), Choice::Number(6)], "{}", log(&g));
        let n = g.battlefield().iter().filter(|&&x| g.is_token(x) && g.obj(x).controller == 0).count();
        assert_eq!(n, treasures, "order {k}\n{}", log(&g));
        assert!(g.find_card(1, "Island", Zone::Hand).is_none(), "no card drawn\n{}", log(&g));
    }
}

// ---- 26.11.3: a double-faced card that is copying something transforms (CR 701.27c, 707.2) ---

/// A double-faced artifact made a copy of Concealing Curtains by Saheeli,
/// Sublime Artificer's -2, then transformed by the copied "{2}{B}:
/// Transform this": it is a transforming double-faced card, so it
/// transforms (701.27c stops only permanents that aren't); the copy effect
/// still gives it Concealing Curtains' characteristics (layer 1, 707.2), and
/// when the effect ends it shows its back face.
#[test]
fn cr_701_27c_a_double_faced_card_copying_something_transforms() {
    let mut g = scen()
        .battlefield(0, &["Saheeli, Sublime Artificer", "C Twofold Relic", "Concealing Curtains // Revealing Eye", "Swamp", "Swamp", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let sah = bf(&g, 0, "Saheeli, Sublime Artificer");
    let relic = bf(&g, 0, "C Twofold Relic");
    let curtains = bf(&g, 0, "Concealing Curtains // Revealing Eye");
    let minus = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == sah)).expect("Saheeli's -2");
    g.choose(minus);
    let mut k = 0;
    finish(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| if g.has_option(g.tgt(relic)) { g.tgt(relic) } else { g.tgt(curtains) })
    });
    let _ = &mut k;
    resolve_with(&mut g, |_| None);
    assert_eq!(g.name(relic), "Concealing Curtains // Revealing Eye", "a copy\n{}", log(&g));
    let tr = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == relic)).expect("the copied transform ability");
    g.choose(tr);
    resolve_with(&mut g, |_| None);
    assert!(g.log_lines().iter().any(|l| l.contains("transforms")), "it transformed\n{}", log(&g));
    assert_eq!(g.name(relic), "Concealing Curtains // Revealing Eye", "still the copy (layer 1)\n{}", log(&g));
    g.drive_until(|g| g.turn().number == 2 && at_priority(g));
    assert_eq!(g.name(relic), "C Twofold Idol", "its back face once the copy ended\n{}", log(&g));
}

// ---- 31.7.7k: a copy of a Class has a level of its own (CR 716, 707.2) ---------------------------

/// A copy of Does Machines at level 2 is level 1 (the level isn't a copiable
/// value), and it may gain levels with its own copied level ability (CR
/// 716.2a).
#[test]
fn cr_716_2a_a_copy_of_a_class_starts_at_level_1_and_levels_up() {
    let mut g = scen()
        .battlefield(0, &["Does Machines", "Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["C Echo"])
        .at(Step::Main1, 0)
        .start();
    let dm = bf(&g, 0, "Does Machines");
    let lv = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == dm)).expect("level 2");
    g.choose(lv);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.def_of(dm).class_level, 2, "{}", log(&g));
    let echo = g.card(0, "C Echo", Zone::Hand);
    cast_resolve(&mut g, 0, "C Echo", |g| g.has_option(Choice::Card(dm)).then_some(Choice::Card(dm)));
    assert_eq!(g.name(echo), "Does Machines", "a copy\n{}", log(&g));
    assert_eq!(g.def_of(echo).class_level, 1, "the level isn't copied\n{}", log(&g));
    let lv = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == echo)).expect("the copy's level 2 ability");
    g.choose(lv);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.def_of(echo).class_level, 2, "{}", log(&g));
}

// ---- N22: a basic land type from an effect (CR 305.6-305.7) ---------------------------------------

/// A land given a basic land type has that type's intrinsic mana ability
/// (CR 305.6-305.7); the engine has it only where it is built (a printed
/// type, Multiversal Passage's chosen type), so an effect or copy exception
/// that gives one is refused loudly.
#[test]
fn cr_305_6_an_effect_giving_a_basic_land_type_is_refused() {
    let why = db().unsupported().get("C Forest Mimic").unwrap_or_else(|| panic!("C Forest Mimic must be unsupported"));
    assert!(why.iter().any(|w| w.contains("basic land type")), "{why:?}");
}
