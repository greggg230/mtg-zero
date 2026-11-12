//! The Legacy Izzet Delver 75's five sideboard cards outside the cube
//! (ENGINE.md 122) against their Oracle text, the Comprehensive Rules and
//! their official rulings (`// ruling: <hash8>`, `data/rulings/legacy.rulings.json`).
//! They are in the card table of constructed play only
//! (`CardDb::constructed`). No test names an identity number.
//!
//! Consign to Memory {U}, Instant: "Replicate {1} (When you cast this spell,
//! copy it for each time you paid its replicate cost. You may choose new
//! targets for the copies.) Counter target triggered ability or colorless
//! spell."
//!
//! Hydroblast {U}, Instant: "Choose one — Counter target spell if it's red.
//! • Destroy target permanent if it's red."
//!
//! Meltdown {X}{R}, Sorcery: "Destroy each artifact with mana value X or
//! less."
//!
//! Price of Progress {1}{R}, Instant: "Price of Progress deals damage to each
//! player equal to twice the number of nonbasic lands that player controls."
//!
//! Rough // Tumble {1}{R} // {5}{R}, Sorcery // Sorcery (a split card):
//! "Rough deals 2 damage to each creature without flying." // "Tumble deals
//! 6 damage to each creature with flying."

use engine::db::{types, AbilitySlot, CardDb, Counter, Kw};
use engine::decision::{act, FACE_CAST, OTHER_CAST};
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const CONSIGN: &str = "Consign to Memory";
const HYDRO: &str = "Hydroblast";
const MELTDOWN: &str = "Meltdown";
const PRICE: &str = "Price of Progress";
const RT: &str = "Rough // Tumble";
const DRC: &str = "Dragon's Rage Channeler";
const OMNI: &str = "Omniscience";
const EMRAKUL: &str = "Emrakul, the Aeons Torn";

/// `ChooseAltCost`'s answer for "without paying its mana cost" (the last
/// `NUMBER`).
const FREE: u8 = (act::NUMBER_N - 1) as u8;

/// The colours' bits (W U B R G).
const BLUE: u8 = 1 << 1;
const RED: u8 = 1 << 3;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

/// A face's definition (a split card's half).
fn face_def(name: &str) -> u16 {
    (0..db().len() as u16).find(|&i| db().def(i).name == name).unwrap_or_else(|| panic!("no face {name}"))
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

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

fn decider(g: &Game) -> PlayerId {
    g.pending_decision().unwrap().player
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn obj_target(g: &Game, c: CardIdx) -> Target {
    Target::Obj(g.obj_ref(c))
}

/// The id of the newest ability on the stack.
fn top_ability(g: &Game) -> u32 {
    let s = g.stack().iter().rev().find(|s| matches!(s.kind, StackKind::Ability { .. })).unwrap_or_else(|| panic!("no ability on the stack\n{}", log(g)));
    s.id
}

/// The stack item of spell `c`.
fn item_of(g: &Game, c: CardIdx) -> &engine::game::StackItem {
    g.stack().iter().find(|s| s.kind == StackKind::Spell { card: c }).unwrap_or_else(|| panic!("{} is not on the stack\n{}", g.name(c), log(g)))
}

fn targets_of(g: &Game, c: CardIdx) -> Vec<Target> {
    item_of(g, c).targets.iter().map(|&(_, t)| t).collect()
}

/// Answer every decision but priority (`answer`, else the first option)
/// until priority or the game's end.
fn to_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
        if g.is_over() || at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| options(g)[0]);
        g.choose(c);
    }
    panic!("to_priority: no progress {}\n{}", opts(g), log(g));
}

/// Cast `name` from P`p`'s hand (its left half for a split card), `answer`
/// for the casting decisions (else the first option), until priority.
fn cast_with(g: &mut Game, p: PlayerId, name: &str, answer: &dyn Fn(&Game) -> Option<Choice>) -> CardIdx {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "P{p} can't cast {name}: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    to_priority(g, answer);
    c
}

/// Cast `name` without paying its mana cost (Omniscience).
fn cast_free(g: &mut Game, p: PlayerId, name: &str, answer: &dyn Fn(&Game) -> Option<Choice>) -> CardIdx {
    cast_with(g, p, name, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(FREE)),
        _ => answer(g),
    })
}

/// Pass priority, answering everything else (`answer`, else the first
/// option), until the stack is empty at priority.
fn resolve_with(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
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
    resolve_with(g, &|_| None);
}

/// Resolve the top stack item only, its decisions answered by `answer`.
fn resolve_top(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    assert!(at_priority(g) && !g.stack().is_empty(), "{}", log(g));
    let id = g.stack().last().unwrap().id;
    for _ in 0..200 {
        if g.is_over() || (!g.stack().iter().any(|s| s.id == id) && at_priority(g)) {
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

/// Pass priority and decline attacks and blocks until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..5000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("game over\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", log(g));
}

/// The mode / X / replicate answer: `NUMBER` n when offered.
fn number(g: &Game, n: u8) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::ChooseMode) && g.has_option(Choice::Number(n))).then_some(Choice::Number(n))
}

/// Cast Painter's Servant (P`p`, two lands) naming `color` (W U B R G
/// index), and resolve it.
fn painter_naming(g: &mut Game, p: PlayerId, color: u8) {
    let ps = hand(g, p, "Painter's Servant");
    g.choose(Choice::Cast(ps));
    while g.kind() != Some(DecisionKind::ChooseColor) {
        let c = if at_priority(g) { Choice::Pass } else { options(g)[0] };
        g.choose(c);
    }
    g.choose(Choice::Number(color));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
}

// ---- Consign to Memory ----------------------------------------------------------------------

#[test]
fn consign_to_memory_is_a_one_mana_blue_instant_with_replicate() {
    let d = db().def(def(CONSIGN));
    assert_eq!(d.mana_value(), 1);
    assert!(d.types & types::INSTANT != 0);
    let g = scen().hand(0, &[CONSIGN]).start();
    assert_eq!(g.colors_of(hand(&g, 0, CONSIGN)), BLUE);
}

/// P0's stack: Mishra's Bauble (a colourless spell), Dragon's Rage
/// Channeler's surveil trigger for it, Lightning Bolt (a red spell), the
/// trigger for it, Polluted Delta's ability (an activated ability).
fn p0_stack() -> (Game, CardIdx, CardIdx) {
    let mut g = scen()
        .battlefield(0, &[DRC, "Mountain", "Polluted Delta"])
        .hand(0, &["Mishra's Bauble", "Lightning Bolt"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &[CONSIGN])
        .at(Step::Main1, 0)
        .start();
    let bauble = cast_with(&mut g, 0, "Mishra's Bauble", &|_| None);
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    let delta = bf(&g, 0, "Polluted Delta");
    g.choose(Choice::Ability(delta, AbilitySlot::Activated(0)));
    to_priority(&mut g, &|_| None);
    assert_eq!(g.stack().len(), 5, "{}", log(&g));
    g.pass();
    assert_eq!(decider(&g), 1);
    (g, bauble, bolt)
}

/// CR 115.1, 701.6a: "target triggered ability or colorless spell": the two
/// triggers and the colourless Mishra's Bauble are its targets; Lightning
/// Bolt (red) and the fetch land's activated ability are not. A countered
/// spell goes to its owner's graveyard: the Bauble never resolves.
/// ruling: 19831073 (triggered abilities use "when", "whenever" or "at")
#[test]
fn cr_115_1_consign_to_memory_targets_triggered_abilities_and_colorless_spells() {
    // ruling: 19831073
    let (mut g, bauble, bolt) = p0_stack();
    let st = g.stack().to_vec();
    let trigs: Vec<u32> = st.iter().filter(|s| matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Trigger(_)))).map(|s| s.id).collect();
    let fetch = st.iter().find(|s| matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Activated(_)))).unwrap().id;
    assert_eq!(trigs.len(), 2, "{}", log(&g));
    let c = hand(&g, 1, CONSIGN);
    g.choose(Choice::Cast(c));
    let seen = std::cell::RefCell::new(Vec::new());
    to_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => {
            *seen.borrow_mut() = options(g);
            Some(Choice::Target(obj_target(g, bauble)))
        }
        _ => None,
    });
    let mut want = vec![Choice::Target(obj_target(&g, bauble))];
    want.extend(trigs.iter().map(|&id| Choice::Target(Target::Stack(id))));
    let mut got = seen.borrow().clone();
    got.sort_by_key(|c| format!("{c:?}"));
    want.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(got, want, "not Bolt, not the fetch land's ability");
    assert!(!got.contains(&Choice::Target(obj_target(&g, bolt))));
    assert!(!got.contains(&Choice::Target(Target::Stack(fetch))));
    resolve_top(&mut g, &|_| None);
    assert_eq!(g.obj(bauble).zone, Zone::Graveyard, "countered: {}", log(&g));
    assert_eq!(g.stack().len(), 4);
    assert!(!g.stack().iter().any(|s| s.kind == StackKind::Spell { card: bauble }));
}

/// CR 601.2c, 115.1: with no triggered ability and no colourless spell on
/// the stack it can't be cast (a red spell and an activated ability are no
/// targets).
#[test]
fn cr_601_2c_consign_to_memory_needs_a_triggered_ability_or_a_colorless_spell() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Polluted Delta"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &[CONSIGN])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    let delta = bf(&g, 0, "Polluted Delta");
    g.choose(Choice::Ability(delta, AbilitySlot::Activated(0)));
    to_priority(&mut g, &|_| None);
    g.pass();
    assert_eq!(decider(&g), 1);
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, CONSIGN))), "{}", opts(&g));
}

/// CR 105.2c, 613.1e: "colorless" is the spell's colour now: under Painter's
/// Servant naming red, Mishra's Bauble is a red spell, no target.
#[test]
fn cr_105_2c_a_spell_made_red_by_painters_servant_is_not_colorless() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Painter's Servant", "Mishra's Bauble"])
        .battlefield(1, &["Island"])
        .hand(1, &[CONSIGN])
        .at(Step::Main1, 0)
        .start();
    painter_naming(&mut g, 0, 3);
    let bauble = cast_with(&mut g, 0, "Mishra's Bauble", &|_| None);
    assert_eq!(g.colors_of(bauble), RED);
    g.pass();
    assert_eq!(decider(&g), 1);
    assert!(!g.has_option(Choice::Cast(hand(&g, 1, CONSIGN))), "{}", opts(&g));
    // Without the Servant the Bauble is a target.
    let mut g = scen().hand(0, &["Mishra's Bauble"]).battlefield(1, &["Island"]).hand(1, &[CONSIGN]).at(Step::Main1, 0).start();
    cast_with(&mut g, 0, "Mishra's Bauble", &|_| None);
    g.pass();
    assert!(g.has_option(Choice::Cast(hand(&g, 1, CONSIGN))), "{}", opts(&g));
}

/// CR 701.6a, 603.7: Pact of Negation's "at the beginning of your next
/// upkeep, pay {3}{U}{U}. If you don't, you lose the game" countered as it
/// waits on the stack: no payment asked, no loss.
#[test]
fn cr_701_6a_consign_to_memory_counters_pact_of_negations_upkeep_trigger() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Pact of Negation"])
        .battlefield(1, &["Mountain", "Island"])
        .hand(1, &["Lightning Bolt", CONSIGN])
        .at(Step::Main1, 1)
        .start();
    let bolt = cast_with(&mut g, 1, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(0))));
    g.pass();
    let t = obj_target(&g, bolt);
    cast_with(&mut g, 0, "Pact of Negation", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(t)));
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    // P0's upkeep: the trigger on the stack, P1 with priority.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep && !g.stack().is_empty() && at_priority(g) && decider(g) == 1);
    let pact = top_ability(&g);
    cast_with(&mut g, 1, CONSIGN, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Stack(pact))),
        _ => None,
    });
    resolve_with(&mut g, &|g| {
        assert_ne!(g.kind(), Some(DecisionKind::YesNo), "no payment: {}", log(g));
        None
    });
    assert!(!g.stack().iter().any(|s| s.id == pact));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert!(!g.is_over(), "{}", log(&g));
}

/// CR 101.2, 701.6a: Emrakul, the Aeons Torn can't be countered, but its cast
/// trigger can: countered, no extra turn. Targeting Emrakul itself (a
/// colourless spell) does nothing.
#[test]
fn cr_101_2_consign_to_memory_and_emrakuls_cast_trigger() {
    let start = || {
        let mut g = scen()
            .battlefield(0, &[OMNI])
            .hand(0, &[EMRAKUL])
            .battlefield(1, &["Island"])
            .hand(1, &[CONSIGN])
            .at(Step::Main1, 0)
            .start();
        let e = cast_free(&mut g, 0, EMRAKUL, &|_| None);
        let trig = top_ability(&g);
        g.pass();
        assert_eq!(decider(&g), 1);
        (g, e, trig)
    };
    // The trigger.
    let (mut g, e, trig) = start();
    let offered = std::cell::RefCell::new(Vec::new());
    cast_with(&mut g, 1, CONSIGN, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => {
            *offered.borrow_mut() = options(g);
            Some(Choice::Target(Target::Stack(trig)))
        }
        _ => None,
    });
    assert!(offered.borrow().contains(&Choice::Target(obj_target(&g, e))), "Emrakul is a colourless spell: {:?}", offered.borrow());
    resolve(&mut g);
    assert_eq!(g.obj(e).zone, Zone::Battlefield);
    assert!(g.extra_turns().is_empty(), "no extra turn: {}", log(&g));
    // Emrakul itself: it can't be countered; the trigger resolves.
    let (mut g, e, _) = start();
    cast_with(&mut g, 1, CONSIGN, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => Some(Choice::Target(obj_target(g, e))),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(e).zone, Zone::Battlefield);
    assert_eq!(g.extra_turns(), &[0]);
}

/// CR 702.56a, 707.10c: replicate {1} paid once: a copy, which may target
/// another triggered ability; each counters its own.
#[test]
fn cr_702_56a_consign_to_memory_replicated_counters_two_triggers() {
    let (mut g, bauble, bolt) = p0_stack();
    let trigs: Vec<u32> = g.stack().iter().filter(|s| matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Trigger(_)))).map(|s| s.id).collect();
    let c = hand(&g, 1, CONSIGN);
    g.choose(Choice::Cast(c));
    to_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            // {U} and {1} once: two Islands; not twice.
            assert!(!g.has_option(Choice::Number(2)), "{}", opts(g));
            number(g, 1)
        }
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Stack(trigs[0]))),
        _ => None,
    });
    // The replicate trigger resolves: the copy, its new target.
    let new_targets = std::cell::Cell::new(0);
    resolve_top(&mut g, &|g| {
        if g.kind() == Some(DecisionKind::NewTarget) {
            new_targets.set(new_targets.get() + 1);
            assert!(g.has_option(Choice::Target(Target::Stack(trigs[1]))), "{}", opts(g));
            assert!(g.has_option(Choice::Target(obj_target(g, bauble))));
            assert!(!g.has_option(Choice::Target(obj_target(g, bolt))));
            return Some(Choice::Target(Target::Stack(trigs[1])));
        }
        None
    });
    assert_eq!(new_targets.get(), 1, "{}", log(&g));
    // The copy, then the original: both triggers countered; no surveil.
    resolve_top(&mut g, &|_| None);
    resolve_top(&mut g, &|_| None);
    assert!(!g.stack().iter().any(|s| trigs.contains(&s.id)), "{}", log(&g));
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    resolve_with(&mut g, &|g| {
        assert_ne!(g.kind(), Some(DecisionKind::PutInGraveyard), "no surveil: {}", log(g));
        None
    });
    assert_eq!(g.life(1), 17, "Bolt resolves");
}

/// CR 707.10: countering the replicated spell leaves its copy, which still
/// counters its target.
/// ruling: 5c226a47 (a copy must be countered individually; countering the
/// spell with replicate doesn't affect the copies)
#[test]
fn cr_707_10_countering_consign_to_memory_leaves_its_copy() {
    // ruling: 5c226a47
    let mut g = scen()
        .battlefield(0, &[DRC, "Island"])
        .hand(0, &["Mishra's Bauble", "Lightning Bolt", "Daze"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &[CONSIGN])
        .at(Step::Main1, 0)
        .start();
    // P0: Bauble (a trigger), then P1 answers; the Bolt waits in hand.
    let bauble = cast_with(&mut g, 0, "Mishra's Bauble", &|_| None);
    let trig = top_ability(&g);
    g.pass();
    let c = hand(&g, 1, CONSIGN);
    g.choose(Choice::Cast(c));
    to_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 1),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Stack(trig))),
        _ => None,
    });
    // The copy targets the Bauble.
    resolve_top(&mut g, &|g| (g.kind() == Some(DecisionKind::NewTarget)).then(|| Choice::Target(obj_target(g, bauble))));
    let copy = g.stack().iter().rev().find_map(|s| match s.kind {
        StackKind::Spell { card } if card != c && g.name(card) == CONSIGN => Some(card),
        _ => None,
    });
    let copy = copy.unwrap_or_else(|| panic!("the copy\n{}", log(&g)));
    assert_eq!(targets_of(&g, copy), vec![obj_target(&g, bauble)]);
    // P0 Dazes the original (P1 has no mana left to pay {1}).
    while decider(&g) != 0 {
        g.pass();
    }
    let t = obj_target(&g, c);
    cast_with(&mut g, 0, "Daze", &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(Choice::Target(t)),
        _ => None,
    });
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    // The copy countered the Bauble; the trigger resolved (a surveil).
    assert_eq!(g.obj(bauble).zone, Zone::Graveyard, "{}", log(&g));
    assert!(g.find_card(0, "Mishra's Bauble", Zone::Battlefield).is_none());
}

/// CR 702.21a: ward is a triggered ability (a keyword's): Consign to Memory
/// counters a ward trigger, so the spell that targeted the permanent is not
/// countered by it.
/// ruling: 19831073 (keywords such as prowess are triggered abilities)
#[test]
fn cr_702_21a_consign_to_memory_counters_a_ward_trigger() {
    // ruling: 19831073
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island"])
        .hand(0, &["Lightning Bolt", CONSIGN])
        .battlefield(1, &["Kappa Cannoneer"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 1, "Kappa Cannoneer");
    let tk = obj_target(&g, k);
    cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(tk)));
    let ward = top_ability(&g);
    assert_eq!(g.stack().len(), 2, "Bolt and the ward trigger: {}", log(&g));
    cast_with(&mut g, 0, CONSIGN, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Stack(ward))),
        _ => None,
    });
    resolve_with(&mut g, &|g| {
        assert_ne!(g.kind(), Some(DecisionKind::YesNo), "ward's payment never asked: {}", log(g));
        None
    });
    assert_eq!(g.obj(k).damage, 3, "Bolt resolved: {}", log(&g));
}

/// CR 702.40a: storm's trigger countered: no copies of Flusterstorm.
/// ruling: 19831073
#[test]
fn cr_702_40a_consign_to_memory_counters_a_storm_trigger() {
    // ruling: 19831073
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Brainstorm", CONSIGN])
        .battlefield(1, &["Island"])
        .hand(1, &["Flusterstorm"])
        .at(Step::Main1, 0)
        .start();
    let bs = cast_with(&mut g, 0, "Brainstorm", &|_| None);
    g.pass();
    let tb = obj_target(&g, bs);
    let fl = cast_with(&mut g, 1, "Flusterstorm", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(tb)));
    let storm = top_ability(&g);
    g.pass();
    cast_with(&mut g, 0, CONSIGN, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => number(g, 0),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Stack(storm))),
        _ => None,
    });
    resolve_top(&mut g, &|_| None);
    assert!(!g.stack().iter().any(|s| s.id == storm), "{}", log(&g));
    let copies = g.stack().iter().filter(|s| matches!(s.kind, StackKind::Spell { card } if card != fl && g.name(card) == "Flusterstorm")).count();
    assert_eq!(copies, 0);
}

// ---- Hydroblast ---------------------------------------------------------------------------

/// Cast Hydroblast (P`p`) in mode `mode` (0 = counter, 1 = destroy) at `t`.
fn hydroblast(g: &mut Game, p: PlayerId, mode: u8, t: Target) -> CardIdx {
    cast_with(g, p, HYDRO, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(mode)),
        Some(DecisionKind::Target) => Some(Choice::Target(t)),
        _ => None,
    })
}

#[test]
fn hydroblast_is_a_one_mana_blue_instant() {
    let d = db().def(def(HYDRO));
    assert_eq!(d.mana_value(), 1);
    assert!(d.types & types::INSTANT != 0);
    let g = scen().hand(0, &[HYDRO]).start();
    assert_eq!(g.colors_of(hand(&g, 0, HYDRO)), BLUE);
}

/// CR 700.2a, 701.6a: "Counter target spell if it's red": Lightning Bolt is
/// countered.
#[test]
fn cr_700_2a_hydroblast_counters_a_red_spell() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 0)
        .start();
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    let t = obj_target(&g, bolt);
    hydroblast(&mut g, 1, 0, t);
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "{}", log(&g));
}

/// CR 608.2b: any spell or permanent is a target; the colour is checked as
/// Hydroblast resolves. A blue Brainstorm and a blue Snapcaster Mage are
/// targeted and untouched.
/// ruling: 53e85191 (any spell or permanent, not just a red one; the colour
/// checked only on resolution)
#[test]
fn cr_608_2b_hydroblast_targets_anything_and_checks_the_colour_on_resolution() {
    // ruling: 53e85191
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .battlefield(1, &["Island"])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 0)
        .start();
    let bs = cast_with(&mut g, 0, "Brainstorm", &|_| None);
    g.pass();
    let t = obj_target(&g, bs);
    let h = hydroblast(&mut g, 1, 0, t);
    resolve_top(&mut g, &|_| None);
    assert_eq!(g.obj(h).zone, Zone::Graveyard);
    assert_eq!(g.obj(bs).zone, Zone::Stack, "not countered: {}", log(&g));
    // The destroy mode on a blue creature and on a land: every permanent is
    // offered.
    let mut g = scen()
        .battlefield(0, &["Snapcaster Mage", "Island"])
        .battlefield(1, &["Island"])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 1)
        .start();
    let snap = bf(&g, 0, "Snapcaster Mage");
    let island = bf(&g, 0, "Island");
    let h = hand(&g, 1, HYDRO);
    g.choose(Choice::Cast(h));
    to_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => {
            assert!(g.has_option(Choice::Target(obj_target(g, island))), "{}", opts(g));
            Some(Choice::Target(obj_target(g, snap)))
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(snap).zone, Zone::Battlefield, "{}", log(&g));
}

/// CR 700.2a, 701.8a: "Destroy target permanent if it's red": Dragon's Rage
/// Channeler (a red creature) and Cori-Steel Cutter (a red artifact).
#[test]
fn cr_701_8a_hydroblast_destroys_a_red_permanent() {
    for name in [DRC, "Cori-Steel Cutter"] {
        let mut g = scen()
            .battlefield(0, &[name])
            .battlefield(1, &["Island"])
            .hand(1, &[HYDRO])
            .at(Step::Main1, 1)
            .start();
        let c = bf(&g, 0, name);
        let t = obj_target(&g, c);
        hydroblast(&mut g, 1, 1, t);
        resolve(&mut g);
        assert_eq!(g.obj(c).zone, Zone::Graveyard, "{name}: {}", log(&g));
    }
}

/// CR 613.1e, 105.2: the colour on resolution includes colour-changing
/// effects: under Painter's Servant naming red, an Island is red and
/// Hydroblast destroys it.
#[test]
fn cr_613_1e_hydroblast_reads_painters_servants_red() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Island"])
        .hand(0, &["Painter's Servant"])
        .battlefield(1, &["Island"])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 0)
        .start();
    painter_naming(&mut g, 0, 3);
    let island = bf(&g, 0, "Island");
    assert_eq!(g.colors_of(island), RED);
    g.pass();
    let t = obj_target(&g, island);
    hydroblast(&mut g, 1, 1, t);
    resolve(&mut g);
    assert_eq!(g.obj(island).zone, Zone::Graveyard, "{}", log(&g));
}

/// CR 700.2a, 115.7a: the mode is chosen as Hydroblast is cast, before its
/// target; Misdirection changing the counter mode's target offers spells
/// only, never a permanent.
/// ruling: fa2aaa26 (the mode can't change on a redirect: only targets of
/// the selected type are valid)
#[test]
fn cr_700_2a_a_redirected_hydroblast_keeps_its_mode() {
    // ruling: fa2aaa26
    let mut g = scen()
        .battlefield(0, &["Mountain", DRC])
        .hand(0, &["Lightning Bolt", "Misdirection", "Brainstorm"])
        .battlefield(1, &["Island"])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 0)
        .start();
    let bolt = cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    resolve_top(&mut g, &|_| None); // DRC's surveil trigger
    g.pass();
    let t = obj_target(&g, bolt);
    let h = hydroblast(&mut g, 1, 0, t);
    g.pass();
    let bs = hand(&g, 0, "Brainstorm");
    let th = obj_target(&g, h);
    let m = cast_with(&mut g, 0, "Misdirection", &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(bs)),
        Some(DecisionKind::Target) => Some(Choice::Target(th)),
        _ => None,
    });
    // DRC's trigger for Misdirection, then Misdirection resolves.
    resolve_top(&mut g, &|_| None);
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{}\n{}", opts(&g), log(&g));
    let o = options(&g);
    assert_eq!(o, vec![Choice::Target(obj_target(&g, m))], "a spell only (Misdirection itself), no permanent: {}", opts(&g));
    let drc = bf(&g, 0, DRC);
    assert!(!o.contains(&Choice::Target(obj_target(&g, drc))));
}

// ---- Meltdown -----------------------------------------------------------------------------

/// Cast Meltdown (P`p`) with X = `x`, and resolve.
fn meltdown(g: &mut Game, p: PlayerId, x: u8) {
    cast_with(g, p, MELTDOWN, &|g| {
        if g.kind() == Some(DecisionKind::ChooseMode) {
            assert!(g.has_option(Choice::Number(x)), "X = {x} not offered: {}", opts(g));
        }
        number(g, x)
    });
    assert_eq!(g.stack().last().unwrap().x, x as u32);
    resolve(g);
}

#[test]
fn meltdown_is_an_x_red_sorcery_of_mana_value_one_outside_the_stack() {
    // CR 202.3e: X is 0 outside the stack.
    let d = db().def(def(MELTDOWN));
    assert_eq!(d.mana_value(), 1);
    assert!(d.types & types::SORCERY != 0);
    let g = scen().hand(0, &[MELTDOWN]).start();
    assert_eq!(g.colors_of(hand(&g, 0, MELTDOWN)), RED);
}

/// CR 107.3a, 701.8a: X = 1 destroys each artifact of mana value 1 or less,
/// both players', the caster's own too; mana value 2 and more stay, and so
/// does everything that isn't an artifact.
#[test]
fn cr_107_3a_meltdown_destroys_each_artifact_with_mana_value_x_or_less() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Sol Ring", "Mox Ruby", "Cori-Steel Cutter"])
        .battlefield(1, &["Lotus Petal", "Chrome Mox", "Kappa Cannoneer", "Painter's Servant", "Island"])
        .hand(0, &[MELTDOWN])
        .at(Step::Main1, 0)
        .start();
    meltdown(&mut g, 0, 1);
    for (p, n) in [(0, "Sol Ring"), (0, "Mox Ruby"), (1, "Lotus Petal"), (1, "Chrome Mox")] {
        assert_eq!(g.count_in(p, n, Zone::Graveyard), 1, "{n}: {}", log(&g));
    }
    for (p, n) in [(0, "Cori-Steel Cutter"), (1, "Kappa Cannoneer"), (1, "Painter's Servant"), (1, "Island"), (0, "Mountain")] {
        assert!(g.find_card(p, n, Zone::Battlefield).is_some(), "{n}: {}", log(&g));
    }
}

/// CR 202.3e: X = 0 still destroys an artifact whose mana cost has {X}
/// (Walking Ballista's mana value on the battlefield is 0) and the {0}
/// artifacts; Sol Ring (1) stays.
#[test]
fn cr_202_3e_meltdown_for_zero_destroys_walking_ballista_and_the_zero_artifacts() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Sol Ring"])
        .battlefield(1, &["Walking Ballista", "Mox Ruby"])
        .counters(1, "Walking Ballista", Counter::P1P1, 2)
        .hand(0, &[MELTDOWN])
        .at(Step::Main1, 0)
        .start();
    let wb = bf(&g, 1, "Walking Ballista");
    meltdown(&mut g, 0, 0);
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.count_in(1, "Mox Ruby", Zone::Graveyard), 1);
    assert!(g.find_card(0, "Sol Ring", Zone::Battlefield).is_some());
}

/// CR 702.12b: an indestructible artifact isn't destroyed (Kaldra Compleat,
/// mana value 7, with X = 7); Kappa Cannoneer (6) is.
#[test]
fn cr_702_12b_meltdown_leaves_an_indestructible_artifact() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 8])
        .battlefield(1, &["Kaldra Compleat", "Kappa Cannoneer"])
        .hand(0, &[MELTDOWN])
        .at(Step::Main1, 0)
        .start();
    meltdown(&mut g, 0, 7);
    assert!(g.find_card(1, "Kaldra Compleat", Zone::Battlefield).is_some(), "{}", log(&g));
    assert_eq!(g.count_in(1, "Kappa Cannoneer", Zone::Graveyard), 1);
}

/// CR 107.3b: cast without paying its mana cost (Omniscience), X can only be
/// 0: the {0} artifacts are destroyed, Sol Ring stays.
#[test]
fn cr_107_3b_meltdown_cast_for_free_has_x_zero() {
    let mut g = scen()
        .battlefield(0, &[OMNI, "Sol Ring"])
        .battlefield(1, &["Mox Ruby", "Lotus Petal"])
        .hand(0, &[MELTDOWN])
        .at(Step::Main1, 0)
        .start();
    cast_free(&mut g, 0, MELTDOWN, &|g| {
        if g.kind() == Some(DecisionKind::ChooseMode) {
            assert_eq!(options(g), vec![Choice::Number(0)], "X = 0 only: {}", opts(g));
        }
        None
    });
    assert_eq!(g.stack().last().unwrap().x, 0);
    resolve(&mut g);
    assert_eq!(g.count_in(1, "Mox Ruby", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Lotus Petal", Zone::Graveyard), 1);
    assert!(g.find_card(0, "Sol Ring", Zone::Battlefield).is_some());
}

// ---- Price of Progress --------------------------------------------------------------------

fn price(g: &mut Game, p: PlayerId) -> CardIdx {
    cast_with(g, p, PRICE, &|_| None)
}

#[test]
fn price_of_progress_is_a_two_mana_red_instant() {
    let d = db().def(def(PRICE));
    assert_eq!(d.mana_value(), 2);
    assert!(d.types & types::INSTANT != 0);
    let g = scen().hand(0, &[PRICE]).start();
    assert_eq!(g.colors_of(hand(&g, 0, PRICE)), RED);
}

/// CR 120.3a, 205.4c: each player is dealt twice the number of nonbasic
/// lands they control, its caster too; basic lands and nonland permanents
/// don't count; a nonbasic land with basic land types (Volcanic Island,
/// Underground Sea) is nonbasic.
#[test]
fn cr_120_3a_price_of_progress_deals_twice_each_players_nonbasic_lands() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Volcanic Island", "Scalding Tarn", "Mountain"])
        .battlefield(1, &["Underground Sea", "Wasteland", "Karakas", "Island", "Mox Ruby"])
        .hand(0, &[PRICE])
        .at(Step::Main1, 0)
        .start();
    price(&mut g, 0);
    resolve(&mut g);
    assert_eq!(g.life(0), 16, "two nonbasic lands: {}", log(&g));
    assert_eq!(g.life(1), 14, "three nonbasic lands");
}

/// CR 608.2h: the lands are counted as it resolves: a fetch land sacrificed
/// in response (for a basic Island) no longer counts.
#[test]
fn cr_608_2h_price_of_progress_counts_the_lands_as_it_resolves() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .battlefield(1, &["Underground Sea", "Polluted Delta"])
        .hand(0, &[PRICE])
        .at(Step::Main1, 0)
        .start();
    price(&mut g, 0);
    g.pass();
    assert_eq!(decider(&g), 1);
    let delta = bf(&g, 1, "Polluted Delta");
    g.choose(Choice::Ability(delta, AbilitySlot::Activated(0)));
    to_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(def("Island"))));
    assert_eq!(g.count_in(1, "Island", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.life(1), 20 - 1 - 2, "the fetch land's life, then one nonbasic land");
    assert_eq!(g.life(0), 20);
}

/// CR 305.7, 205.4b: under Magus of the Moon nonbasic lands are Mountains but
/// still nonbasic (their supertypes don't change): they count.
#[test]
fn cr_305_7_nonbasic_lands_under_magus_of_the_moon_still_count() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Volcanic Island", "Scalding Tarn"])
        .battlefield(1, &["Magus of the Moon", "Underground Sea", "Island"])
        .hand(0, &[PRICE])
        .at(Step::Main1, 0)
        .start();
    let vi = bf(&g, 0, "Volcanic Island");
    assert!(g.has_subtype(vi, db().subtype_id("Mountain").unwrap()));
    price(&mut g, 0);
    resolve(&mut g);
    assert_eq!((g.life(0), g.life(1)), (16, 18), "{}", log(&g));
}

/// CR 120.8: a player with no nonbasic land is dealt no damage (0 damage is
/// none): no life lost.
#[test]
fn cr_120_8_price_of_progress_deals_no_damage_to_a_player_with_basics_only() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Volcanic Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &[PRICE])
        .at(Step::Main1, 0)
        .start();
    price(&mut g, 0);
    resolve(&mut g);
    assert_eq!(g.life(1), 20);
    assert!(!g.player(1).lost_life, "no damage, no life lost");
    assert_eq!(g.life(0), 18);
}

/// CR 104.4a: both players dealt lethal damage by the one spell lose at
/// once: a draw.
#[test]
fn cr_104_4a_price_of_progress_killing_both_players_is_a_draw() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Volcanic Island"])
        .battlefield(1, &["Underground Sea"])
        .life(0, 2)
        .life(1, 2)
        .hand(0, &[PRICE])
        .at(Step::Main1, 0)
        .start();
    price(&mut g, 0);
    resolve(&mut g);
    let o = g.outcome().unwrap_or_else(|| panic!("over: {}", log(&g)));
    assert_eq!(o.winner, None, "a draw");
    assert!(g.player(0).lost && g.player(1).lost);
}

// ---- Rough // Tumble ----------------------------------------------------------------------

/// Cast the half `face` (0 = Rough, 1 = Tumble) of P`p`'s Rough // Tumble.
fn cast_half(g: &mut Game, p: PlayerId, face: u8) -> CardIdx {
    let c = hand(g, p, RT);
    let ch = g
        .offered()
        .into_iter()
        .find(|ch| matches!(*ch, Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) if x == c))
        .unwrap_or_else(|| panic!("Rough // Tumble not castable: {}", opts(g)));
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(face));
    }
    to_priority(g, &|_| None);
    c
}

/// CR 709.3, 709.4: in hand, the card's characteristics are both halves'
/// combined (mana value 8, red, a sorcery, the name "Rough // Tumble"); it is
/// cast as one half (`Faces`: either; with two lands, Rough only); on the
/// stack only that half's characteristics exist (Rough: mana value 2).
/// ruling: c622b600 (cast one of its halves; no way to cast both)
/// ruling: 9fbd1525 (the other half's characteristics are ignored on the
/// stack)
/// ruling: d9bc4e52 (a split card's mana value outside the stack is both
/// halves' combined)
#[test]
fn cr_709_3_rough_tumble_is_cast_as_one_half() {
    // ruling: c622b600 9fbd1525 d9bc4e52
    let g = scen().battlefield(0, &["Mountain"; 6]).hand(0, &[RT]).at(Step::Main1, 0).start();
    let c = hand(&g, 0, RT);
    assert_eq!(g.def_of(c).mana_value(), 8);
    assert_eq!(g.colors_of(c), RED);
    assert!(g.def_of(c).types & types::SORCERY != 0);
    assert_eq!(g.name(c), RT);
    assert!(g.offered().contains(&Choice::Faces(c, FACE_CAST | OTHER_CAST)), "{}", opts(&g));
    let mut g = scen().battlefield(0, &["Mountain"; 2]).hand(0, &[RT]).at(Step::Main1, 0).start();
    let c = hand(&g, 0, RT);
    let o = g.offered();
    assert!(o.contains(&Choice::Cast(c)), "Rough: {}", opts(&g));
    assert!(!o.iter().any(|x| matches!(x, Choice::CastFace(y) | Choice::Faces(y, _) if *y == c)), "not Tumble: {}", opts(&g));
    cast_half(&mut g, 0, 0);
    assert_eq!(g.name(c), "Rough");
    assert_eq!(g.def_of(c).mana_value(), 2, "CR 202.3d");
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert_eq!(g.name(c), RT);
    assert_eq!(g.def_of(c).mana_value(), 8);
}

/// CR 202.3d: off the stack its mana value is 8: Inquisition of Kozilek
/// ("a nonland card with mana value 3 or less") can't take it.
/// ruling: d9bc4e52
#[test]
fn cr_202_3d_rough_tumble_in_hand_has_mana_value_eight() {
    // ruling: d9bc4e52
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Inquisition of Kozilek"])
        .hand(1, &[RT, "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, "Inquisition of Kozilek", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::ChooseDiscard), "{}\n{}", opts(&g), log(&g));
    let names: Vec<String> = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Card(c) => Some(g.name(c).to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["Lightning Bolt".to_string()], "{}", opts(&g));
}

/// CR 120.3, 702.9a: Rough deals 2 damage to each creature without flying,
/// both players', and to no player; creatures with flying are untouched.
#[test]
fn cr_120_3_rough_damages_each_creature_without_flying() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", DRC, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"])
        .battlefield(1, &["Kappa Cannoneer", "Snapcaster Mage", "Brazen Borrower // Petty Theft"])
        .hand(0, &[RT])
        .at(Step::Main1, 0)
        .start();
    let (k, b, t) = (bf(&g, 1, "Kappa Cannoneer"), bf(&g, 1, "Brazen Borrower // Petty Theft"), bf(&g, 0, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"));
    cast_half(&mut g, 0, 0);
    resolve(&mut g);
    assert_eq!(g.count_in(0, DRC, Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(1, "Snapcaster Mage", Zone::Graveyard), 1);
    assert_eq!(g.obj(k).damage, 2);
    assert_eq!((g.obj(b).damage, g.obj(t).damage), (0, 0), "fliers untouched");
    assert_eq!((g.life(0), g.life(1)), (20, 20));
}

/// CR 120.3, 702.9a: Tumble deals 6 damage to each creature with flying and
/// none to the others.
#[test]
fn cr_120_3_tumble_damages_each_creature_with_flying() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(0, &["Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"])
        .battlefield(1, &["Kappa Cannoneer", "Brazen Borrower // Petty Theft"])
        .hand(0, &[RT])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 1, "Kappa Cannoneer");
    cast_half(&mut g, 0, 1);
    assert_eq!(g.name(hand_or_stack(&g)), "Tumble");
    resolve(&mut g);
    assert_eq!(g.count_in(1, "Brazen Borrower // Petty Theft", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar", Zone::Graveyard), 1);
    assert_eq!(g.obj(k).damage, 0);
    assert_eq!((g.life(0), g.life(1)), (20, 20));
}

/// The Rough // Tumble card on the stack.
fn hand_or_stack(g: &Game) -> CardIdx {
    g.stack().iter().find_map(|s| match s.kind {
        StackKind::Spell { card } => Some(card),
        _ => None,
    }).unwrap()
}

/// CR 613.1f, 702.9a: flying as the creatures have it now: Dragon's Rage
/// Channeler with delirium has flying (Tumble kills it, Rough doesn't);
/// without, Rough does.
#[test]
fn cr_613_1f_rough_and_tumble_read_flying_as_it_is_now() {
    // Delirium: four card types in P1's graveyard.
    let gy = ["Lightning Bolt", "Mishra's Bauble", "Island", "Ponder", "Snapcaster Mage"];
    for (face, delirium, dies) in [(1u8, true, true), (0, true, false), (0, false, true), (1, false, false)] {
        let lands = if face == 1 { 6 } else { 2 };
        let mut s = scen().battlefield(0, &vec!["Mountain"; lands]).battlefield(1, &[DRC]).hand(0, &[RT]).at(Step::Main1, 0);
        if delirium {
            s = s.graveyard(1, &gy);
        }
        let mut g = s.start();
        let d = bf(&g, 1, DRC);
        assert_eq!(g.has_kw(d, Kw::Flying), delirium);
        cast_half(&mut g, 0, face);
        resolve(&mut g);
        assert_eq!(g.obj(d).zone == Zone::Graveyard, dies, "face {face}, delirium {delirium}: {}", log(&g));
    }
}

/// CR 707.10, 709.3: a copy of Rough is Rough (Lutri, the Spellchaser's):
/// two times 2 damage kill Kappa Cannoneer (4/4), which Rough alone doesn't.
/// ruling: ae909e09 (a copy of a half is that same half)
#[test]
fn cr_707_10_a_copy_of_rough_is_rough() {
    // ruling: ae909e09
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Volcanic Island", "Volcanic Island", "Island"])
        .battlefield(1, &["Kappa Cannoneer"])
        .hand(0, &[RT, "Lutri, the Spellchaser"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 1, "Kappa Cannoneer");
    let c = cast_half(&mut g, 0, 0);
    let tr = obj_target(&g, c);
    cast_with(&mut g, 0, "Lutri, the Spellchaser", &|_| None);
    let copied = std::cell::Cell::new(false);
    resolve_with(&mut g, &|g| match g.kind() {
        Some(DecisionKind::Target) => {
            copied.set(true);
            Some(Choice::Target(tr))
        }
        _ => None,
    });
    assert!(copied.get(), "{}", log(&g));
    assert!(log(&g).contains("copies Rough"), "{}", log(&g));
    assert_eq!(g.obj(k).zone, Zone::Graveyard, "Rough twice: {}", log(&g));
}

/// CR 709.4a, 201.3: Disruptor Flute naming "Rough": Rough costs {3} more
/// ({4}{R}: five lands needed), Tumble doesn't.
/// ruling: b83b7a27 (a split card has two names; one of them may be chosen,
/// not both)
#[test]
fn cr_709_4a_disruptor_flute_naming_rough_taxes_rough_only() {
    // ruling: b83b7a27
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .hand(0, &[RT])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Disruptor Flute"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    cast_with(&mut g, 1, "Disruptor Flute", &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::NameCard)).then_some(Choice::Def(face_def("Rough"))));
    let c = hand(&g, 0, RT);
    // Six lands: Rough ({4}{R}) and Tumble ({5}{R}) both.
    assert!(g.offered().contains(&Choice::Faces(c, FACE_CAST | OTHER_CAST)), "{}", opts(&g));
    // Four lands: neither Rough (5 now) nor Tumble.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .hand(0, &[RT])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Disruptor Flute"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    cast_with(&mut g, 1, "Disruptor Flute", &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::NameCard)).then_some(Choice::Def(face_def("Rough"))));
    let c = hand(&g, 0, RT);
    assert!(!g.offered().iter().any(|x| matches!(x, Choice::Cast(y) | Choice::CastFace(y) | Choice::Faces(y, _) if *y == c)), "{}", opts(&g));
    // Naming "Tumble": Rough for {1}{R} with two lands.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 2])
        .hand(0, &[RT])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Disruptor Flute"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    cast_with(&mut g, 1, "Disruptor Flute", &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::NameCard)).then_some(Choice::Def(face_def("Tumble"))));
    let c = hand(&g, 0, RT);
    assert!(g.offered().contains(&Choice::Cast(c)), "{}", opts(&g));
}

/// CR 709.4: a split card is one card: Murktide Regent delving Rough //
/// Tumble gets one +1/+1 counter (one sorcery card exiled).
/// ruling: 7892efbd (a split card is a single card: counted once)
#[test]
fn cr_709_4_rough_tumble_is_one_card_for_murktide_regent() {
    // ruling: 7892efbd
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .graveyard(0, &[RT, "Island", "Island", "Island", "Island"])
        .hand(0, &["Murktide Regent"])
        .at(Step::Main1, 0)
        .start();
    let m = hand(&g, 0, "Murktide Regent");
    let gy: Vec<CardIdx> = g.player(0).graveyard.clone();
    g.choose(Choice::Cast(m));
    let mut picked = 0;
    for _ in 0..100 {
        if at_priority(&g) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::ChooseGraveyardCard) if picked < gy.len() && g.has_option(Choice::Card(gy[picked])) => {
                picked += 1;
                Choice::Card(gy[picked - 1])
            }
            _ => options(&g)[0],
        };
        g.choose(c);
    }
    assert_eq!(picked, 5, "{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.counters_on(m, Counter::P1P1), 1, "one sorcery card: {}", log(&g));
}

/// Hydroblast counters Rough (a red spell); Pyroblast can't touch Tumble
/// (it isn't blue): the colour of the half on the stack (CR 709.3b).
#[test]
fn cr_709_3b_hydroblast_counters_rough() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .battlefield(1, &["Island", DRC])
        .hand(0, &[RT])
        .hand(1, &[HYDRO])
        .at(Step::Main1, 0)
        .start();
    let c = cast_half(&mut g, 0, 0);
    g.pass();
    let t = obj_target(&g, c);
    hydroblast(&mut g, 1, 0, t);
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert_eq!(g.obj(bf(&g, 1, DRC)).damage, 0, "countered: {}", log(&g));
}
