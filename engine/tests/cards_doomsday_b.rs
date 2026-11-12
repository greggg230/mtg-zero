//! Lane B of the Legacy Doomsday deck (ENGINE.md 110.2) against the cards'
//! Oracle text and their official rulings (`// ruling: <hash8>`, as the
//! cube's in `docs/rulings.md`; `data/rulings/legacy.rulings.json`). They are
//! in the card table of constructed play (`CardDb::constructed`); these tests
//! use that table plus one custom card (`Test Twin Shock`, below).
//!
//! Misdirection {3}{U}{U}, Instant: "You may exile a blue card from your hand
//! rather than pay this spell's mana cost. Change the target of target spell
//! with a single target."
//!
//! Flusterstorm {U}, Instant: "Counter target instant or sorcery spell unless
//! its controller pays {1}. Storm (When you cast this spell, copy it for each
//! spell cast before it this turn. You may choose new targets for the
//! copies.)"
//!
//! Cavern of Souls, Land: "As this land enters, choose a creature type. {T}:
//! Add {C}. {T}: Add one mana of any color. Spend this mana only to cast a
//! creature spell of the chosen type, and that spell can't be countered."

use engine::db::{AbilitySlot, CardDb};
use engine::doomsday_b::CREATURE_TYPES_CR;
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{mana, CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use serde_json::json;
use std::sync::OnceLock;

const MISDIRECTION: &str = "Misdirection";
const FLUSTERSTORM: &str = "Flusterstorm";
const CAVERN: &str = "Cavern of Souls";
const TWIN: &str = "Test Twin Shock";

/// The constructed table plus `Test Twin Shock` ({R} Instant: "This deals 1
/// damage to any target. This deals 1 damage to any target.": two words
/// "target", so the same player or object may be chosen twice, CR 115.3).
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(engine::db::load_legacy_document().cards);
        let any = json!({"$t": "TargetSpec", "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"}, {"$t": "IsPlaneswalker"}, {"$t": "IsBattle"}, {"$t": "IsPlayer"}]}});
        let shock = json!({"$t": "DealDamageAnyTarget", "amount": 1, "targets": [any]});
        let twin = json!({"name": TWIN, "custom": true, "printing": {
            "$t": "CardPrinting", "name": TWIN, "mana_cost_str": "{R}",
            "mana_cost": {"$t": "ManaCost", "colored": [["R", 1]]},
            "type_line": "Instant", "supertypes": [], "card_types": ["Instant"], "subtypes": [],
            "spell_effect": {"$t": "Sequence", "effects": [shock.clone(), shock]},
        }});
        cards.push(serde_json::from_value(twin).expect("the test card's IR"));
        let db = CardDb::from_cards(&cards);
        for n in [MISDIRECTION, FLUSTERSTORM, CAVERN, TWIN] {
            db.expect_id(n).unwrap_or_else(|e| panic!("{e}"));
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
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

fn decider(g: &Game) -> PlayerId {
    g.pending_decision().unwrap().player
}

fn tgt(g: &Game, c: CardIdx) -> Choice {
    g.tgt(c)
}

/// The stack item of spell `c`.
fn item_of(g: &Game, c: CardIdx) -> &engine::game::StackItem {
    g.stack().iter().find(|s| s.kind == StackKind::Spell { card: c }).unwrap_or_else(|| panic!("{} is not on the stack\n{}", g.name(c), log(g)))
}

/// The targets of spell `c` on the stack.
fn targets_of(g: &Game, c: CardIdx) -> Vec<Target> {
    item_of(g, c).targets.iter().map(|&(_, t)| t).collect()
}

fn obj_target(g: &Game, c: CardIdx) -> Target {
    Target::Obj(g.obj_ref(c))
}

/// Answer every decision but priority (`answer`, else the first option)
/// until priority.
fn to_priority(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
        if g.is_over() || at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| options(g)[0]);
        g.choose(c);
    }
    panic!("to_priority: no progress\n{}", log(g));
}

/// Pass priority, answering everything else (`answer`, else the first
/// option), until the stack is empty at priority.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..1000 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| options(g)[0]);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Resolve the top stack item only: both players pass once (the stack
/// item resolves), its decisions answered by `answer`, back at priority.
fn resolve_top(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    assert!(at_priority(g) && !g.stack().is_empty());
    let id = g.stack().last().unwrap().id;
    for _ in 0..200 {
        if !g.stack().iter().any(|s| s.id == id) && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| options(g)[0]);
        g.choose(c);
    }
    panic!("resolve_top: no progress\n{}", log(g));
}

/// Cast `name` from P`p`'s hand: `answer` for the casting decisions (else
/// the first option), until priority.
fn cast_with(g: &mut Game, p: PlayerId, name: &str, answer: impl Fn(&Game) -> Option<Choice>) -> CardIdx {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "P{p} can't cast {name}: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    to_priority(g, answer);
    c
}

/// Cast Misdirection by its alternative cost, exiling `blue` (CR 118.9),
/// targeting the spell `at`.
fn misdirect(g: &mut Game, p: PlayerId, blue: &str, at: CardIdx) -> CardIdx {
    let b = hand(g, p, blue);
    let t = tgt(g, at);
    cast_with(g, p, MISDIRECTION, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(b)),
        Some(DecisionKind::Target) => Some(t),
        _ => None,
    })
}

// ---- Misdirection -----------------------------------------------------------------------

#[test]
fn misdirection_is_a_five_mana_blue_instant_with_an_alternative_cost() {
    let g = scen().hand(0, &[MISDIRECTION]).start();
    let m = hand(&g, 0, MISDIRECTION);
    let d = g.db().def(g.obj(m).def);
    assert_eq!(d.mana_value(), 5);
    assert_eq!(g.db().def(g.obj(m).def).alt_costs.len(), 1, "one alternative cost");
}

/// CR 118.9: "You may exile a blue card from your hand rather than pay this
/// spell's mana cost": no mana, no life; a blue card other than itself
/// (601.2a: it is on the stack as its costs are paid); a red card can't be
/// exiled for it, and with no other blue card it can't be cast this way.
#[test]
fn cr_118_9_misdirection_exiles_another_blue_card_from_hand() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &[MISDIRECTION, "Brainstorm", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    assert_eq!(decider(&g), 1);
    let m = hand(&g, 1, MISDIRECTION);
    let bs = hand(&g, 1, "Brainstorm");
    let red = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(m));
    for _ in 0..20 {
        match g.kind() {
            Some(DecisionKind::ChooseAltCost) => {
                g.choose(Choice::Number(1));
            }
            Some(DecisionKind::ExileCost) => {
                assert_eq!(options(&g), vec![Choice::Card(bs)], "the one other blue card: {}", opts(&g));
                g.choose(Choice::Card(bs));
            }
            Some(DecisionKind::Priority) => break,
            _ => {
                let c = options(&g)[0];
                g.choose(c);
            }
        };
    }
    assert!(at_priority(&g), "{}", log(&g));
    assert_eq!(g.obj(bs).zone, Zone::Exile);
    assert_eq!(g.obj(red).zone, Zone::Hand);
    assert_eq!(g.life(1), 20, "no life paid");
    assert_eq!(targets_of(&g, m), vec![obj_target(&g, bolt)]);
    // Alone with a red card: no alternative cost, and {3}{U}{U} unpaid.
    let g = scen().hand(1, &[MISDIRECTION, "Lightning Bolt"]).battlefield(0, &["Mountain"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let mut g = g;
    cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, MISDIRECTION))), "it can't exile itself: {}", opts(&g));
}

/// CR 115.7a: Lightning Bolt aimed at P1 goes to P1's creature instead (the
/// new target chosen by Misdirection's controller as it resolves, among the
/// targets legal for Bolt; the change is not optional when one exists).
/// ruling: 130c9dd0 (the spell targeted as Misdirection is cast, the new
/// target chosen as it resolves)
#[test]
fn cr_115_7a_misdirection_changes_lightning_bolts_target() {
    // ruling: 130c9dd0
    let mut g = scen()
        .battlefield(0, &["Mountain", "Thassa's Oracle"])
        .battlefield(1, &["Dragon's Rage Channeler"])
        .hand(0, &["Lightning Bolt", MISDIRECTION, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let drc = bf(&g, 1, "Dragon's Rage Channeler");
    let oracle = bf(&g, 0, "Thassa's Oracle");
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    let m = misdirect(&mut g, 0, "Brainstorm", bolt);
    assert_eq!(targets_of(&g, bolt), vec![Target::Player(1)], "nothing changes as it is cast");
    // Misdirection resolves: the new target.
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}\n{}", opts(&g), log(&g));
    assert_eq!(decider(&g), 0);
    let o = options(&g);
    for t in [Target::Player(0), obj_target(&g, drc), obj_target(&g, oracle)] {
        assert!(o.contains(&Choice::Target(t)), "{t:?} is a legal target for Bolt: {}", opts(&g));
    }
    assert!(!o.contains(&Choice::Target(Target::Player(1))), "another target: {}", opts(&g));
    assert!(!o.contains(&Choice::Pass), "the change isn't optional (CR 115.7a)");
    g.choose(Choice::Target(obj_target(&g, drc)));
    assert!(at_priority(&g));
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    assert_eq!(targets_of(&g, bolt), vec![obj_target(&g, drc)]);
    resolve(&mut g);
    assert_eq!(g.obj(drc).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(1), 20);
}

/// CR 115.7a: Thoughtseize's one target is a player; the other player is the
/// one other legal target ("target player").
#[test]
fn cr_115_7a_misdirection_turns_thoughtseize_on_its_caster() {
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Thoughtseize", "Fatal Push"])
        .hand(1, &[MISDIRECTION, "Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let ts = cast_with(&mut g, 0, "Thoughtseize", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    misdirect(&mut g, 1, "Brainstorm", ts);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    assert_eq!(options(&g), vec![Choice::Target(Target::Player(0))]);
    g.choose(Choice::Target(Target::Player(0)));
    let push = hand(&g, 0, "Fatal Push");
    // Thoughtseize: P0 reveals, P0 (its controller) chooses, P0 loses 2.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseDiscard)).then_some(Choice::Card(push)));
    assert_eq!(g.obj(push).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 18);
    assert_eq!(g.player(1).hand.len(), 1, "P1's Ponder kept");
}

/// CR 115.7a, 115.5, 608.2b: Force of Will countering Lightning Bolt is
/// redirected onto Misdirection itself (still on the stack as it resolves,
/// a legal target for "target spell"); Misdirection then leaves the stack,
/// and Force of Will, its only target gone, doesn't resolve: Bolt does.
/// ruling: 1f76d6f6 (a spell can be made to target this spell, which is
/// still on the stack as the new target is chosen)
/// ruling: 2ef42b09 (a spell can't be made to target itself)
#[test]
fn cr_115_7a_force_of_will_redirected_onto_misdirection_itself_fizzles() {
    // ruling: 1f76d6f6 2ef42b09
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt", MISDIRECTION, "Brainstorm"])
        .hand(1, &["Force of Will", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    let ponder = hand(&g, 1, "Ponder");
    let fow = cast_with(&mut g, 1, "Force of Will", |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(ponder)),
        _ => None,
    });
    assert_eq!(targets_of(&g, fow), vec![obj_target(&g, bolt)]);
    g.pass();
    let m = misdirect(&mut g, 0, "Brainstorm", fow);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    let o = options(&g);
    assert_eq!(o, vec![Choice::Target(obj_target(&g, m))], "Misdirection: not Bolt (the current target), not Force of Will itself (115.5)");
    g.choose(o[0]);
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    resolve(&mut g);
    assert_eq!(g.obj(fow).zone, Zone::Graveyard);
    assert!(log(&g).contains("fizzles"), "{}", log(&g));
    assert_eq!(g.life(1), 19 - 3, "Force of Will's life, then Bolt's 3");
}

/// CR 115.7a: with no other legal target, the target is unchanged (no
/// decision); Fatal Push still destroys the creature it targeted.
/// ruling: f1ca52a5 (no other legal target: the target doesn't change)
#[test]
fn cr_115_7a_no_other_legal_target_leaves_fatal_push_unchanged() {
    // ruling: f1ca52a5
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .battlefield(1, &["Snapcaster Mage"])
        .hand(0, &["Fatal Push"])
        .hand(1, &[MISDIRECTION, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let snap = bf(&g, 1, "Snapcaster Mage");
    let push = cast_with(&mut g, 0, "Fatal Push", |_| None);
    assert_eq!(targets_of(&g, push), vec![obj_target(&g, snap)]);
    g.pass();
    misdirect(&mut g, 1, "Brainstorm", push);
    g.pass();
    g.pass();
    assert!(at_priority(&g), "no decision: {:?} {}
{}", g.kind(), opts(&g), log(&g));
    assert!(log(&g).contains("no other legal target"), "{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(snap).zone, Zone::Graveyard);
}

/// CR 115.7a: Fatal Push on P1's creature turned on P0's own Thassa's Oracle.
#[test]
fn cr_115_7a_misdirection_turns_fatal_push_on_its_casters_creature() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Thassa's Oracle"])
        .battlefield(1, &["Snapcaster Mage"])
        .hand(0, &["Fatal Push"])
        .hand(1, &[MISDIRECTION, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let snap = bf(&g, 1, "Snapcaster Mage");
    let oracle = bf(&g, 0, "Thassa's Oracle");
    let push = cast_with(&mut g, 0, "Fatal Push", |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt(g, snap)));
    g.pass();
    misdirect(&mut g, 1, "Brainstorm", push);
    g.pass();
    g.pass();
    assert_eq!(options(&g), vec![Choice::Target(obj_target(&g, oracle))], "{}", opts(&g));
    g.choose(Choice::Target(obj_target(&g, oracle)));
    resolve(&mut g);
    assert_eq!(g.obj(oracle).zone, Zone::Graveyard);
    assert_eq!(g.obj(snap).zone, Zone::Battlefield);
}

/// CR 115.7a with Daze: "counter target spell unless its controller pays
/// {1}" changed to another spell on the stack, here one below it.
/// ruling: aee352b1 (a spell that targets a spell can be changed to target
/// any other spell, even one that resolves before it)
#[test]
fn cr_115_7a_daze_redirected_onto_another_spell() {
    // ruling: aee352b1
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Lightning Bolt", "Brainstorm", MISDIRECTION, "Ponder"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    let mountain = bf(&g, 0, "Mountain");
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        Some(DecisionKind::PayMana) => g.has_option(Choice::Card(mountain)).then_some(Choice::Card(mountain)),
        _ => None,
    });
    let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
    g.pass();
    // Daze by its alternative cost (its Island returned), at Brainstorm.
    let daze = cast_with(&mut g, 1, "Daze", |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(tgt(g, bs)),
        _ => None,
    });
    assert_eq!(targets_of(&g, daze), vec![obj_target(&g, bs)]);
    g.pass();
    misdirect(&mut g, 0, "Ponder", daze);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    let o = options(&g);
    assert!(o.contains(&Choice::Target(obj_target(&g, bolt))), "Bolt, below Daze: {}", opts(&g));
    g.choose(Choice::Target(obj_target(&g, bolt)));
    assert_eq!(targets_of(&g, daze), vec![obj_target(&g, bolt)]);
    // Daze resolves: P0 declines to pay (no mana left anyway): Bolt is
    // countered; Brainstorm resolves.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20);
    assert_eq!(g.obj(bs).zone, Zone::Graveyard);
}

/// CR 115.9a: "a spell with a single target" counts the targets chosen,
/// each instance (Test Twin Shock at P1 twice has two), not whether they
/// are legal now: Lightning Bolt whose creature left can still be targeted
/// and changed.
/// ruling: 83bf218e (it checks only that the spell has a single target,
/// not that the target is legal)
#[test]
fn cr_115_9a_a_spell_with_a_single_target() {
    // ruling: 83bf218e
    let mut g = scen()
        .battlefield(0, &["Volcanic Island", "Volcanic Island", "Volcanic Island", "Volcanic Island"])
        .battlefield(1, &["Snapcaster Mage"])
        .hand(0, &["Lightning Bolt", TWIN, "Brazen Borrower // Petty Theft"])
        .hand(1, &[MISDIRECTION, "Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let snap = bf(&g, 1, "Snapcaster Mage");
    let twin = cast_with(&mut g, 0, TWIN, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    assert_eq!(targets_of(&g, twin), vec![Target::Player(1), Target::Player(1)], "the same player twice (two words \"target\")");
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt(g, snap)));
    // Petty Theft returns the Channeler: Bolt's one target is illegal now.
    let bb = hand(&g, 0, "Brazen Borrower // Petty Theft");
    g.choose(Choice::CastFace(bb));
    to_priority(&mut g, |_| None);
    resolve_top(&mut g, |_| None);
    assert_eq!(g.obj(snap).zone, Zone::Hand, "{}", log(&g));
    g.pass();
    assert_eq!(decider(&g), 1);
    let m = hand(&g, 1, MISDIRECTION);
    g.choose(Choice::Cast(m));
    let bs = hand(&g, 1, "Brainstorm");
    for _ in 0..10 {
        match g.kind() {
            Some(DecisionKind::ChooseAltCost) => {
                g.choose(Choice::Number(1));
            }
            Some(DecisionKind::ExileCost) => {
                g.choose(Choice::Card(bs));
            }
            Some(DecisionKind::Target) => {
                let o = options(&g);
                assert!(o.contains(&tgt(&g, bolt)), "Bolt, its one target gone: {}", opts(&g));
                assert!(!o.contains(&tgt(&g, twin)), "the Twin Shock has two targets: {}", opts(&g));
                g.choose(tgt(&g, bolt));
            }
            _ => break,
        }
    }
    assert!(at_priority(&g), "{}", log(&g));
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    g.choose(Choice::Target(Target::Player(0)));
    resolve(&mut g);
    assert_eq!(g.life(0), 20 - 3, "Bolt at P0: {}", log(&g));
    assert_eq!(g.life(1), 20 - 2, "the Twin Shock");
}

/// Misdirection targets only the spell (ward on the spell's target doesn't
/// see it); the new target becomes a target of the deflected spell (ward
/// sees that: Kappa Cannoneer's ward {4} triggers for P0's Bolt).
/// ruling: 9a4256c2 (it targets only the spell it changes)
/// ruling: 4e80ba07 (the new target is targeted by the deflected spell:
/// "becomes the target" abilities trigger)
#[test]
fn cr_702_21a_the_new_target_becomes_a_target_misdirection_targets_only_the_spell() {
    // ruling: 9a4256c2 4e80ba07
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Kappa Cannoneer"])
        .hand(0, &["Lightning Bolt", MISDIRECTION, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let kappa = bf(&g, 1, "Kappa Cannoneer");
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    let m = misdirect(&mut g, 0, "Brainstorm", bolt);
    assert_eq!(g.stack().len(), 2, "no ward trigger: Misdirection targets only Bolt: {}", log(&g));
    assert_eq!(targets_of(&g, m), vec![obj_target(&g, bolt)]);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    g.choose(Choice::Target(obj_target(&g, kappa)));
    // Ward: "whenever this becomes the target of a spell an opponent
    // controls, counter it unless that player pays {4}".
    to_priority(&mut g, |_| None);
    assert!(g.stack().iter().any(|s| matches!(s.kind, StackKind::Ability { .. })), "the ward trigger: {}", log(&g));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.obj(kappa).zone, Zone::Battlefield, "Bolt countered by ward: {}", log(&g));
    assert_eq!(g.life(1), 20);
}

/// CR 115.9a: a spell, not an ability: Misdirection can't target the storm
/// trigger (or any ability), and needs a spell with exactly one target.
#[test]
fn cr_115_9a_misdirection_needs_a_spell_with_a_single_target() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .hand(1, &[MISDIRECTION, "Ponder"])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, "Brainstorm", |_| None);
    g.pass();
    let m = hand(&g, 1, MISDIRECTION);
    assert!(!g.has_option(Choice::Cast(m)), "Brainstorm has no target: {}", opts(&g));
}

/// CR 115.7a and storm: Misdirection changes the target of a copy of
/// Flusterstorm (a copy of a spell is a spell with a single target), onto
/// another instant: the original's target is untouched.
#[test]
fn cr_707_10_misdirection_changes_a_flusterstorm_copys_target() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Brainstorm", "Consider", MISDIRECTION, "Force of Will"])
        .hand(1, &[FLUSTERSTORM])
        .at(Step::Main1, 0)
        .start();
    let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
    let ponder = cast_with(&mut g, 0, "Consider", |_| None);
    g.pass();
    // Flusterstorm at Consider; its storm trigger: two copies (two spells
    // before it this turn), each kept on Consider.
    let fl = cast_with(&mut g, 1, FLUSTERSTORM, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt(g, ponder)));
    resolve_top(&mut g, |g| (g.kind() == Some(DecisionKind::NewTarget)).then_some(Choice::Pass));
    let copies: Vec<CardIdx> = g
        .stack()
        .iter()
        .filter_map(|s| match s.kind {
            StackKind::Spell { card } if card != fl && g.name(card) == FLUSTERSTORM => Some(card),
            _ => None,
        })
        .collect();
    assert_eq!(copies.len(), 2, "{}", log(&g));
    let copy = copies[1];
    assert_eq!(targets_of(&g, copy), vec![obj_target(&g, ponder)]);
    assert_eq!(decider(&g), 0, "the active player first after the trigger resolved");
    misdirect(&mut g, 0, "Force of Will", copy);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}", log(&g));
    let o = options(&g);
    assert!(o.contains(&Choice::Target(obj_target(&g, bs))), "{}", opts(&g));
    g.choose(Choice::Target(obj_target(&g, bs)));
    assert_eq!(targets_of(&g, copy), vec![obj_target(&g, bs)]);
    assert_eq!(targets_of(&g, fl), vec![obj_target(&g, ponder)], "the original's target unchanged");
}


// ---- Flusterstorm -----------------------------------------------------------------------

/// The Flusterstorm copies on the stack (not the original).
fn copies_of(g: &Game, original: CardIdx) -> Vec<CardIdx> {
    g.stack()
        .iter()
        .filter_map(|s| match s.kind {
            StackKind::Spell { card } if card != original && g.name(card) == FLUSTERSTORM => Some(card),
            _ => None,
        })
        .collect()
}

/// "Counter target instant or sorcery spell unless its controller pays {1}"
/// (CR 701.5, 118.12): the spell's controller decides as it resolves and may
/// pay with a mana ability then (605.3a); paid, the spell isn't countered;
/// declined, it is. Brainstorm, cast before it this turn, makes one copy
/// (storm), which resolves first and asks too.
#[test]
fn cr_118_12_flusterstorm_counters_unless_its_controller_pays_1() {
    for pay in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Island"])
            .battlefield(1, &["Island"])
            .hand(1, &[FLUSTERSTORM])
            .hand(0, &["Brainstorm"])
            .at(Step::Main1, 0)
            .start();
        let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
        g.pass();
        let fl = cast_with(&mut g, 1, FLUSTERSTORM, |_| None);
        assert_eq!(targets_of(&g, fl), vec![obj_target(&g, bs)]);
        resolve_top(&mut g, |g| (g.kind() == Some(DecisionKind::NewTarget)).then_some(Choice::Pass));
        assert_eq!(copies_of(&g, fl).len(), 1, "storm 1: {}", log(&g));
        for k in 0..2 {
            g.pass();
            g.pass();
            assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
            assert_eq!(decider(&g), 0, "the target spell's controller");
            if pay {
                g.choose(Choice::Yes);
                // The {1}: an untapped Island, a mana ability during the
                // resolution.
                to_priority(&mut g, |_| None);
                assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 2 + k);
            } else {
                g.choose(Choice::Pass);
                to_priority(&mut g, |_| None);
                break;
            }
        }
        resolve(&mut g);
        assert_eq!(g.obj(bs).zone, Zone::Graveyard);
        assert_eq!(g.player(0).hand.len(), if pay { 1 } else { 0 }, "Brainstorm drew 3, put back 2: {}", log(&g));
    }
}

/// Flusterstorm's target: an instant or sorcery spell only (not a creature
/// spell, not an ability).
#[test]
fn flusterstorm_targets_an_instant_or_sorcery_spell() {
    let mut g = scen().battlefield(0, &["Island", "Island"]).battlefield(1, &["Island"]).hand(0, &["Thassa's Oracle"]).hand(1, &[FLUSTERSTORM]).at(Step::Main1, 0).start();
    cast_with(&mut g, 0, "Thassa's Oracle", |_| None);
    g.pass();
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, FLUSTERSTORM))), "a creature spell isn't a legal target: {}", opts(&g));
}

/// CR 702.40a: storm copies Flusterstorm once for each spell cast before it
/// this turn, by either player, a countered spell or one cast from a
/// graveyard too; the copies are put on the stack by the trigger, not cast
/// (707.10, 702.40a), so they don't count for a later storm spell; each copy
/// may get a new target (707.10c), its own.
/// ruling: 158501c3 (spells cast from other zones and countered spells count)
/// ruling: 500a78f9 (the copies aren't cast and don't count for later storm)
/// ruling: bf7e6c38 (new targets for any of the copies, each its own)
#[test]
fn cr_702_40a_flusterstorm_storm_counts_every_spell_cast_before_it_this_turn() {
    // ruling: 158501c3 500a78f9 bf7e6c38
    let mut g = scen()
        .battlefield(0, &["Swamp", "Island", "Island", "Island", "Mountain"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Dark Ritual", "Lotus Petal", "Snapcaster Mage", "Brainstorm", "Lightning Bolt", "Consider"])
        .hand(1, &[FLUSTERSTORM, "Spell Pierce", FLUSTERSTORM])
        .at(Step::Main1, 0)
        .start();
    let swamp = bf(&g, 0, "Swamp");
    let ritual = cast_with(&mut g, 0, "Dark Ritual", |g| Some(Choice::Ability(swamp, AbilitySlot::Activated(0))).filter(|&c| g.kind() == Some(DecisionKind::PayMana) && g.has_option(c)));
    resolve(&mut g);
    assert_eq!(g.player(0).pool.amt[mana::B], 3);
    cast_with(&mut g, 0, "Lotus Petal", |_| None);
    resolve(&mut g);
    // Snapcaster Mage ({1}{U}: {B} from the pool and an Island): its
    // trigger gives Dark Ritual flashback; Ritual cast again from the
    // graveyard ({B} from the pool).
    cast_with(&mut g, 0, "Snapcaster Mage", |_| None);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tgt(g, ritual))).then(|| tgt(g, ritual)));
    assert!(g.has_option(Choice::Cast(ritual)), "flashback: {}", opts(&g));
    g.choose(Choice::Cast(ritual));
    to_priority(&mut g, |_| None);
    resolve(&mut g);
    // Lightning Bolt at P1, countered by Spell Pierce (P0 doesn't pay {2}).
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    cast_with(&mut g, 1, "Spell Pierce", |_| None);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "Bolt countered: {}", log(&g));
    let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
    let con = cast_with(&mut g, 0, "Consider", |_| None);
    g.pass();
    // Before Flusterstorm: Dark Ritual, Lotus Petal, Snapcaster Mage, Dark
    // Ritual (flashback), Lightning Bolt, Spell Pierce, Brainstorm,
    // Consider: 8, both players'.
    let fls: Vec<CardIdx> = g.player(1).hand.iter().copied().filter(|&c| g.name(c) == FLUSTERSTORM).collect();
    g.choose(Choice::Cast(fls[0]));
    to_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| tgt(g, con)));
    let fl = fls[0];
    assert_eq!(g.player(1).spells_cast, 2);
    // The storm trigger resolves: 8 copies, each asked for a new target:
    // the first moves to Brainstorm, the others keep Consider.
    let first = std::cell::Cell::new(true);
    resolve_top(&mut g, |g| {
        (g.kind() == Some(DecisionKind::NewTarget)).then(|| {
            if first.replace(false) {
                tgt(g, bs)
            } else {
                Choice::Pass
            }
        })
    });
    let copies = copies_of(&g, fl);
    assert_eq!(copies.len(), 8, "{}", log(&g));
    let on = |t: CardIdx| copies.iter().filter(|&&c| targets_of(&g, c) == vec![obj_target(&g, t)]).count();
    assert_eq!((on(bs), on(con)), (1, 7));
    assert_eq!(g.player(1).spells_cast, 2, "copies aren't cast");
    // A second Flusterstorm now: 9 spells before it (the 8 and the first
    // Flusterstorm), not the 8 copies.
    assert_eq!(decider(&g), 0);
    g.pass();
    g.choose(Choice::Cast(fls[1]));
    to_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| tgt(g, con)));
    resolve_top(&mut g, |g| (g.kind() == Some(DecisionKind::NewTarget)).then_some(Choice::Pass));
    assert_eq!(copies_of(&g, fl).len() - 8, 1 + 9, "the second and its 9 copies: {}", log(&g));
}

/// The storm trigger can be countered (Tishana's Tidebinder: "counter up to
/// one target activated or triggered ability"): no copies.
/// ruling: 543beb2e (the triggered ability that creates the copies can be
/// countered; then no copies)
#[test]
fn cr_702_40a_a_countered_storm_trigger_makes_no_copies() {
    // ruling: 543beb2e
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Brainstorm", "Tishana's Tidebinder"])
        .hand(1, &[FLUSTERSTORM])
        .at(Step::Main1, 0)
        .start();
    let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
    g.pass();
    let fl = cast_with(&mut g, 1, FLUSTERSTORM, |_| None);
    assert!(g.stack().last().is_some_and(|s| matches!(s.kind, StackKind::Ability { .. })), "the storm trigger");
    let storm = g.stack().last().unwrap().id;
    g.pass();
    cast_with(&mut g, 0, "Tishana's Tidebinder", |_| None);
    // Tidebinder resolves; its trigger targets the storm trigger.
    resolve_top(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Stack(storm))));
    resolve_top(&mut g, |_| None);
    assert!(!g.stack().iter().any(|s| s.id == storm), "countered: {}", log(&g));
    assert!(copies_of(&g, fl).is_empty(), "{}", log(&g));
    assert_eq!(targets_of(&g, fl), vec![obj_target(&g, bs)]);
}

/// Countering Flusterstorm leaves its copies: each resolves (counters
/// Brainstorm unless P0 pays).
/// ruling: c33f0cb3 (a copy must be countered by itself; countering the
/// storm spell doesn't affect the copies)
#[test]
fn cr_707_10_countering_flusterstorm_leaves_its_copies() {
    // ruling: c33f0cb3
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Brainstorm", "Counterspell"])
        .hand(1, &[FLUSTERSTORM])
        .at(Step::Main1, 0)
        .start();
    let bs = cast_with(&mut g, 0, "Brainstorm", |_| None);
    g.pass();
    let fl = cast_with(&mut g, 1, FLUSTERSTORM, |_| None);
    resolve_top(&mut g, |g| (g.kind() == Some(DecisionKind::NewTarget)).then_some(Choice::Pass));
    let copy = copies_of(&g, fl)[0];
    cast_with(&mut g, 0, "Counterspell", |g| (g.kind() == Some(DecisionKind::Target)).then(|| tgt(g, fl)));
    resolve_top(&mut g, |_| None);
    assert_eq!(g.obj(fl).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(targets_of(&g, copy), vec![obj_target(&g, bs)], "the copy stays");
    // The copy resolves: P0 has no untapped land: Brainstorm countered.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(bs).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), 0, "Brainstorm countered by the copy: {}", log(&g));
}

// ---- Cavern of Souls ---------------------------------------------------------------------

fn type_index(name: &str) -> u16 {
    CREATURE_TYPES_CR.iter().position(|&n| n == name).unwrap_or_else(|| panic!("{name} is no creature type of CR 205.3m")) as u16
}

/// The `NUMBER` answers that choose creature type `t` (`ask_number` over
/// 0..=366: t itself below 31, else 31 then the rest's two base-32 digits).
fn type_answers(t: u16) -> Vec<u8> {
    if t < 31 {
        vec![t as u8]
    } else {
        let r = t - 31;
        vec![31, (r / 32) as u8, (r % 32) as u8]
    }
}

/// Answer the creature-type choice pending for P`p` with type `name`.
fn choose_type(g: &mut Game, p: PlayerId, name: &str) {
    for k in type_answers(type_index(name)) {
        assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{}\n{}", opts(g), log(g));
        assert_eq!(decider(g), p);
        g.choose(Choice::Number(k));
    }
}

/// Play Cavern of Souls from P`p`'s hand, naming `name`.
fn play_cavern(g: &mut Game, p: PlayerId, name: &str) -> CardIdx {
    let c = hand(g, p, CAVERN);
    g.choose(Choice::Play(c));
    choose_type(g, p, name);
    assert!(at_priority(g), "{}", log(g));
    assert_eq!(g.chosen_creature_type(c), Some(type_index(name)));
    c
}

/// Cavern's abilities: (the {C} one, the restricted coloured one).
fn cavern_abilities(g: &Game, cav: CardIdx) -> (Choice, Choice) {
    let d = g.db().def(g.obj(cav).def);
    let i = d.activated.iter().position(|a| a.mana().is_some_and(|m| m.only == 0)).expect("{T}: Add {C}");
    let j = d.activated.iter().position(|a| a.mana().is_some_and(|m| m.only != 0)).expect("the restricted ability");
    (Choice::Ability(cav, AbilitySlot::Activated(i as u8)), Choice::Ability(cav, AbilitySlot::Activated(j as u8)))
}

/// Pay picking these sources first, when offered.
fn prefer(picks: Vec<Choice>) -> impl Fn(&Game) -> Option<Choice> {
    move |g| (g.kind() == Some(DecisionKind::PayMana)).then(|| picks.iter().copied().find(|&c| g.has_option(c))).flatten()
}

/// Cast `name` as P`p`'s and let P1 answer with `counter` (by its
/// alternative cost when it has one: `pitch` exiled), then resolve all;
/// whether `name`'s card ended on the battlefield.
fn countered_by(g: &mut Game, spell: CardIdx, counter: &str, pitch: Option<&str>) -> bool {
    g.pass();
    let pitch = pitch.map(|n| hand(g, 1, n));
    let t = tgt(g, spell);
    cast_with(g, 1, counter, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) if pitch.is_some() => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => pitch.map(Choice::Card),
        Some(DecisionKind::Target) => Some(t),
        _ => None,
    });
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    g.obj(spell).zone != Zone::Battlefield
}

/// CR 614.12, 205.3e, 205.3m: as Cavern of Souls enters, its controller
/// chooses one creature type of CR 205.3m's 367 (no card type, no other
/// subtype): a number, the first 31 directly, the rest in two base-32
/// digits (ENGINE.md 100.1); every type is reached, by exactly one sequence
/// of at most three decisions.
/// ruling: dc0c0658 (an existing creature type, such as Human or Warrior;
/// card types such as artifact can't be chosen)
#[test]
fn cr_614_12_cavern_chooses_one_of_the_367_creature_types_as_it_enters() {
    // ruling: dc0c0658
    assert_eq!(CREATURE_TYPES_CR.len(), 367);
    assert!(CREATURE_TYPES_CR.windows(2).all(|w| w[0] < w[1]), "sorted, distinct");
    for n in ["Human", "Warrior", "Merfolk", "Wizard", "Dragon", "Time Lord", "C'tan", "Zubera", "Advisor"] {
        type_index(n);
    }
    for n in ["Artifact", "Creature", "Equipment", "Island", "Aura", "Jace", "Tribal", "Kindred"] {
        assert!(!CREATURE_TYPES_CR.contains(&n), "{n} is no creature type");
    }
    let mut g = scen().hand(0, &[CAVERN]).at(Step::Main1, 0).start();
    let cav = hand(&g, 0, CAVERN);
    g.choose(Choice::Play(cav));
    assert_eq!(g.obj(cav).zone, Zone::Battlefield);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    assert_eq!(options(&g), (0..32).map(Choice::Number).collect::<Vec<_>>(), "31 types and \"31 or more\"");
    // Every type, from the first decision.
    let mut seen = vec![false; 367];
    for t in 0..367u16 {
        let mut h = g.clone();
        let answers = type_answers(t);
        assert!(answers.len() <= 3);
        for k in answers {
            assert!(h.has_option(Choice::Number(k)), "type {t}: {k} offered: {}", opts(&h));
            h.choose(Choice::Number(k));
        }
        let got = h.chosen_creature_type(cav).expect("a type chosen") as usize;
        assert!(!seen[got]);
        seen[got] = true;
        assert_eq!(got, t as usize);
        assert!(at_priority(&h));
    }
    assert!(seen.iter().all(|&x| x));
    // The digits offered: exactly those with a legal completion (366 - 31 =
    // 335 = 10 * 32 + 15).
    g.choose(Choice::Number(31));
    assert_eq!(options(&g).len(), 11);
    g.choose(Choice::Number(10));
    assert_eq!(options(&g).len(), 16);
}

/// CR 106.6, 101.2: Cavern of Souls naming Merfolk: its coloured mana casts
/// Thassa's Oracle (Merfolk Wizard), which then can't be countered (Force of
/// Will, Counterspell and Daze target it and do nothing); it pays for no
/// noncreature spell and no creature spell of another type; its {C} pays for
/// anything and makes nothing uncounterable.
#[test]
fn cr_106_6_cavern_mana_casts_a_creature_spell_of_the_chosen_type_that_cant_be_countered() {
    for counter in ["Force of Will", "Counterspell", "Daze"] {
        let mut g = scen()
            .battlefield(0, &["Island"])
            .battlefield(1, &["Island", "Island"])
            .hand(0, &[CAVERN, "Thassa's Oracle", "Brainstorm"])
            .hand(1, &[counter, "Ponder"])
            .at(Step::Main1, 0)
            .start();
        let cav = play_cavern(&mut g, 0, "Merfolk");
        let (_c, coloured) = cavern_abilities(&g, cav);
        let oracle = cast_with(&mut g, 0, "Thassa's Oracle", prefer(vec![coloured]));
        assert!(g.obj(cav).tapped);
        let pitch = (counter == "Force of Will").then_some("Ponder");
        // (Daze by its mana cost here: P1's Islands.)
        assert!(!countered_by(&mut g, oracle, counter, pitch), "{counter}: it can't be countered: {}", log(&g));
        assert_eq!(g.obj(oracle).zone, Zone::Battlefield);
        assert_eq!(g.count_in(1, counter, Zone::Graveyard), 1, "{counter} resolved (it targeted Oracle) and did nothing");
    }
}

/// CR 106.6: the coloured mana pays only for a creature spell of the chosen
/// type: not Brainstorm (an instant), not Snapcaster Mage (Human Wizard)
/// with Merfolk chosen; with Wizard chosen, Snapcaster Mage and Thassa's
/// Oracle both.
#[test]
fn cr_106_6_cavern_mana_only_for_the_chosen_type() {
    let mut g = scen().hand(0, &[CAVERN, "Brainstorm", "Snapcaster Mage", "Thassa's Oracle"]).battlefield(0, &["Swamp"]).at(Step::Main1, 0).start();
    play_cavern(&mut g, 0, "Merfolk");
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Brainstorm"))), "{}", opts(&g));
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Snapcaster Mage"))), "Human Wizard: {}", opts(&g));
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Thassa's Oracle"))), "{{U}}{{U}}: one blue mana only");
    let mut g = scen().hand(0, &[CAVERN, "Snapcaster Mage", "Thassa's Oracle"]).battlefield(0, &["Island"]).at(Step::Main1, 0).start();
    play_cavern(&mut g, 0, "Wizard");
    assert!(g.has_option(Choice::Cast(hand(&g, 0, "Snapcaster Mage"))));
    assert!(g.has_option(Choice::Cast(hand(&g, 0, "Thassa's Oracle"))), "Merfolk Wizard");
    let mut g = scen().hand(0, &[CAVERN, "Snapcaster Mage"]).battlefield(0, &["Island"]).at(Step::Main1, 0).start();
    play_cavern(&mut g, 0, "Merfolk");
    // Snapcaster Mage with the {C} ability: castable, and countered.
    let cav = bf(&g, 0, CAVERN);
    let (c, _) = cavern_abilities(&g, cav);
    let snap = cast_with(&mut g, 0, "Snapcaster Mage", prefer(vec![c]));
    let _ = snap;
    let mut g2 = scen().hand(0, &[CAVERN, "Snapcaster Mage"]).battlefield(0, &["Island"]).hand(1, &["Counterspell"]).battlefield(1, &["Island", "Island"]).at(Step::Main1, 0).start();
    play_cavern(&mut g2, 0, "Merfolk");
    let cav = bf(&g2, 0, CAVERN);
    let (c, _) = cavern_abilities(&g2, cav);
    let snap = cast_with(&mut g2, 0, "Snapcaster Mage", prefer(vec![c]));
    assert!(countered_by(&mut g2, snap, "Counterspell", None), "{{C}} carries no rider: {}", log(&g2));
}

/// CR 106.6: Cavern's coloured mana floated with priority keeps its
/// restriction and rider in the pool (to the end of the step): it pays for
/// no noncreature spell, and the Oracle it pays for can't be countered.
#[test]
fn cr_106_6_floated_cavern_mana_keeps_its_type_and_rider() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &[CAVERN, "Thassa's Oracle", "Brainstorm"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let cav = play_cavern(&mut g, 0, "Merfolk");
    let (_, coloured) = cavern_abilities(&g, cav);
    // YES at priority: activate a mana ability; the Cavern; its coloured
    // ability; blue.
    g.choose(Choice::Yes);
    g.choose(Choice::Card(cav));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        let k = options(&g)
            .into_iter()
            .find(|&o| {
                let mut h = g.clone();
                h.choose(o);
                h.kind() == Some(DecisionKind::ChooseMana) || h.player(0).pool.amt[mana::C] == 0 && h.player(0).pool.total() > 0
            })
            .expect("the coloured ability");
        g.choose(k);
    }
    let _ = coloured;
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana), "{}", opts(&g));
    let blue = options(&g)
        .into_iter()
        .find(|&o| {
            let mut h = g.clone();
            h.choose(o);
            h.player(0).pool.amt[mana::U] == 1
        })
        .expect("blue");
    g.choose(blue);
    if g.kind() == Some(DecisionKind::ActivateMana) {
        g.choose(Choice::Pass);
    }
    assert!(at_priority(&g), "{}", log(&g));
    let r = g.restricted_mana(0);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].0 as usize, mana::U);
    assert_eq!(engine::doomsday_b::type_of(r[0].1), Some(type_index("Merfolk")));
    assert!(r[0].1 & engine::db::MANA_UNCOUNTERABLE != 0, "the rider");
    // Brainstorm with the Island only, not the floating blue.
    let bs = hand(&g, 0, "Brainstorm");
    assert!(g.has_option(Choice::Cast(bs)));
    let island = bf(&g, 0, "Island");
    // Oracle: the floating {U} and the Island.
    let oracle = cast_with(&mut g, 0, "Thassa's Oracle", |_| None);
    assert!(g.obj(island).tapped && g.restricted_mana(0).is_empty() && g.player(0).pool.total() == 0, "{}", log(&g));
    assert!(!countered_by(&mut g, oracle, "Counterspell", None), "{}", log(&g));
}

/// The floated coloured mana can't pay for Brainstorm, nor for an
/// ability's or an "unless pays" cost: with only it, Brainstorm can't be
/// cast; it empties at the end of the step (CR 106.4, 500.4).
#[test]
fn cr_106_6_floated_cavern_mana_pays_for_no_other_spell() {
    let mut g = scen().hand(0, &["Brainstorm"]).at(Step::Main1, 0).start();
    // (The pool's restricted blue, as the Cavern made it, by the test kit.)
    let only = engine::doomsday_b::with_type(engine::db::MANA_UNCOUNTERABLE, Some(type_index("Merfolk")));
    let mut g2 = scen().hand(0, &["Brainstorm", "Thassa's Oracle"]).pool_restricted(0, mana::U, only, 2).at(Step::Main1, 0).start();
    assert!(!g2.has_option(Choice::Cast(hand(&g2, 0, "Brainstorm"))), "{}", opts(&g2));
    assert!(g2.has_option(Choice::Cast(hand(&g2, 0, "Thassa's Oracle"))));
    assert!(!g2.can_pay(0, &engine::mana::ManaCost::parse("{1}").unwrap()), "no ability or unless cost");
    g2.pass();
    g2.pass();
    assert_eq!(g2.player(0).pool.total(), 0, "emptied: {}", log(&g2));
    assert!(g2.restricted_mana(0).is_empty());
    let _ = g.pass();
}

/// A Cavern that chose no type (it entered without its "as enters" choice:
/// the test kit puts it onto the battlefield directly): no spell is of the
/// chosen type, so its coloured mana pays for nothing (CR 106.6); its {C}
/// still does.
#[test]
fn cr_106_6_a_cavern_that_chose_no_type_makes_mana_for_nothing() {
    let g = scen().battlefield(0, &[CAVERN]).hand(0, &["Thassa's Oracle", "Snapcaster Mage", "Ponder"]).at(Step::Main1, 0).start();
    let cav = bf(&g, 0, CAVERN);
    assert_eq!(g.chosen_creature_type(cav), None);
    assert!(!g.can_pay(0, &engine::mana::ManaCost::parse("{U}").unwrap()));
    assert!(g.can_pay(0, &engine::mana::ManaCost::parse("{1}").unwrap()));
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Ponder"))));
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Thassa's Oracle"))));
}

/// An Adventure creature card cast as its Adventure is an instant spell
/// (CR 715.3): Faerie mana (Cavern naming Faerie, floated) pays for Brazen
/// Borrower (Faerie Rogue), not for Petty Theft.
#[test]
fn cr_715_3_cavern_mana_casts_the_creature_not_its_adventure() {
    let faerie = engine::doomsday_b::with_type(engine::db::MANA_UNCOUNTERABLE, Some(type_index("Faerie")));
    let g = scen()
        .battlefield(0, &["Swamp"])
        .battlefield(1, &["Dragon's Rage Channeler"])
        .hand(0, &["Brazen Borrower // Petty Theft"])
        .pool_restricted(0, mana::U, faerie, 2)
        .at(Step::Main1, 0)
        .start();
    let bb = hand(&g, 0, "Brazen Borrower // Petty Theft");
    // {1}{U}{U}: the two Faerie {U} and the Swamp.
    assert!(g.has_option(Choice::Cast(bb)), "{}", opts(&g));
    // Petty Theft {1}{U}: no {U} it may spend.
    assert!(!g.has_option(Choice::CastFace(bb)), "{}", opts(&g));
    assert!(!options(&g).iter().any(|c| matches!(c, Choice::Faces(..))), "only the creature face: {}", opts(&g));
    let g = scen()
        .battlefield(0, &["Swamp", "Island"])
        .battlefield(1, &["Dragon's Rage Channeler"])
        .hand(0, &["Brazen Borrower // Petty Theft"])
        .pool_restricted(0, mana::U, faerie, 2)
        .at(Step::Main1, 0)
        .start();
    let bb = hand(&g, 0, "Brazen Borrower // Petty Theft");
    // With an Island's {U}, Petty Theft too.
    assert!(options(&g).iter().any(|c| matches!(c, Choice::Faces(x, _) if *x == bb)), "both faces: {}", opts(&g));
}

/// The ruling's additional cost: Thieving Skydiver (Merfolk Rogue) kicked
/// for X = 1, its {1}{U} paid by Islands and the Cavern's mana spent on the
/// whole cost: it can't be countered.
/// ruling: 919d28a6 (Cavern's mana spent on any cost of the spell, even an
/// additional cost such as kicker, makes it uncounterable)
#[test]
fn cr_106_6_cavern_mana_on_a_kicker_cost_makes_the_spell_uncounterable() {
    // ruling: 919d28a6
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Island", "Island", "Sol Ring"])
        .hand(0, &[CAVERN, "Thieving Skydiver"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let cav = play_cavern(&mut g, 0, "Merfolk");
    let (_, coloured) = cavern_abilities(&g, cav);
    let sky = hand(&g, 0, "Thieving Skydiver");
    g.choose(Choice::Cast(sky));
    for _ in 0..30 {
        match g.kind() {
            // Kicked, X = 1.
            Some(DecisionKind::ChooseCost) | Some(DecisionKind::YesNo) if g.has_option(Choice::Yes) => {
                g.choose(Choice::Yes);
            }
            Some(DecisionKind::PayMana) if g.has_option(coloured) => {
                g.choose(coloured);
            }
            Some(DecisionKind::Priority) => break,
            Some(DecisionKind::Target) => {
                let c0 = options(&g)[0];
                g.choose(c0);
            }
            _ => {
                let o = options(&g);
                let c0 = if g.has_option(Choice::Number(1)) && g.kind() != Some(DecisionKind::PayMana) { Choice::Number(1) } else { o[0] };
                g.choose(c0);
            }
        }
    }
    assert!(g.obj(cav).tapped, "{}", log(&g));
    assert_eq!(item_of(&g, sky).x, 1, "kicked for X = 1: {}", log(&g));
    assert!(!countered_by(&mut g, sky, "Counterspell", None), "{}", log(&g));
}

/// Flusterstorm can't target a Cavern-cast creature (it is a creature
/// spell); Force of Negation neither (a noncreature spell's); Force of Will
/// targets it and does nothing.
#[test]
fn cavern_cast_thassas_oracle_against_the_counterspells() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Island"])
        .hand(0, &[CAVERN, "Thassa's Oracle"])
        .hand(1, &[FLUSTERSTORM, "Force of Negation", "Force of Will", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let cav = play_cavern(&mut g, 0, "Merfolk");
    let (_, coloured) = cavern_abilities(&g, cav);
    let oracle = cast_with(&mut g, 0, "Thassa's Oracle", prefer(vec![coloured]));
    g.pass();
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, FLUSTERSTORM))), "{}", opts(&g));
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, "Force of Negation"))), "{}", opts(&g));
    let fow = hand(&g, 1, "Force of Will");
    assert!(g.has_option(Choice::Cast(fow)));
    let ponder = hand(&g, 1, "Ponder");
    cast_with(&mut g, 1, "Force of Will", |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(ponder)),
        _ => None,
    });
    assert_eq!(targets_of(&g, fow), vec![obj_target(&g, oracle)], "it can be targeted");
    resolve(&mut g);
    assert_eq!(g.obj(oracle).zone, Zone::Battlefield, "{}", log(&g));
}

/// Two Caverns, Merfolk and Human: each one's mana is restricted to its own
/// type, apart in the pool (two kinds), and a Merfolk spell can't spend the
/// Human mana.
#[test]
fn cr_106_6_two_caverns_two_types() {
    let merfolk = engine::doomsday_b::with_type(engine::db::MANA_UNCOUNTERABLE, Some(type_index("Merfolk")));
    let human = engine::doomsday_b::with_type(engine::db::MANA_UNCOUNTERABLE, Some(type_index("Human")));
    let g = scen()
        .hand(0, &["Thassa's Oracle", "Snapcaster Mage", "Dragon's Rage Channeler"])
        .pool_restricted(0, mana::U, merfolk, 1)
        .pool_restricted(0, mana::U, human, 1)
        .at(Step::Main1, 0)
        .start();
    assert_eq!(g.restricted_mana(0).len(), 2, "two kinds: {:?}", g.restricted_mana(0));
    // Oracle ({U}{U}, Merfolk Wizard): one Merfolk {U} only.
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Thassa's Oracle"))));
    // Snapcaster ({1}{U}, Human Wizard): the Human {U} and ... no other.
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Snapcaster Mage"))));
    // Dragon's Rage Channeler ({R}, Human Shaman): no red.
    assert!(!g.has_option(Choice::Cast(hand(&g, 0, "Dragon's Rage Channeler"))));
    let g = scen()
        .hand(0, &["Snapcaster Mage"])
        .pool_restricted(0, mana::U, merfolk, 1)
        .pool_restricted(0, mana::U, human, 2)
        .at(Step::Main1, 0)
        .start();
    assert!(g.has_option(Choice::Cast(hand(&g, 0, "Snapcaster Mage"))), "two Human {{U}}");
}

/// The chosen type is public state, in the permanent's encoding (ENGINE.md
/// 110.2: perm status 133-141, its index + 1 in binary) for both viewers.
#[test]
fn the_chosen_creature_type_is_encoded_on_the_permanent() {
    use engine::az::encode::{perm, CODE, NUM_FEATURES, O_MY_PERM, O_OPP_PERM, PERM_W};
    let mut g = scen().hand(0, &[CAVERN]).at(Step::Main1, 0).start();
    play_cavern(&mut g, 0, "Merfolk");
    let v = type_index("Merfolk") as usize + 1;
    for viewer in 0..2u8 {
        let mut x = vec![0.0; NUM_FEATURES];
        g.encode_features(viewer, &mut x);
        let base = if viewer == 0 { O_MY_PERM } else { O_OPP_PERM };
        // (Cavern, the only permanent: slot 0.)
        let st = &x[base + CODE..base + PERM_W];
        let bits: Vec<f32> = (0..9).map(|b| (v >> b & 1) as f32).collect();
        assert_eq!(&st[perm::CHOSEN_CREATURE_TYPE..perm::CHOSEN_CREATURE_TYPE + 9], &bits[..], "viewer {viewer}");
    }
}

