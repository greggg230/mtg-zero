//! M7b part 3b (ENGINE.md 27): the cards of spells and abilities on the
//! stack and of ward, each against its Oracle text: Tishana's Tidebinder,
//! Ertai Resurrected, Invert Polarity, Leovold, Emissary of Trest, Surrak,
//! Elusive Hunter, Sedgemoor Witch, Kappa Cannoneer, Raffine, Scheming
//! Seer, Hexing Squelcher, Sunset Saboteur. (The rules they rest on are in
//! rules_stack.rs and rules_ward.rs.)

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c))
}

fn player_if(g: &Game, p: PlayerId) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(p)))
}

fn stack_target(g: &Game) -> Option<Choice> {
    options(g).into_iter().find(|c| matches!(c, Choice::Target(Target::Stack(_))))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

/// Declare `attackers` (P0's turn, from the start of the main phase) and
/// stop when attackers are declared.
fn attack_with(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
}

// ---- Tishana's Tidebinder ---------------------------------------------------------------------

const TIDEBINDER: &str = "Tishana's Tidebinder";

#[test]
fn tidebinder_counters_a_loyalty_ability_and_the_planeswalker_loses_its_abilities() {
    // "...counter up to one target activated or triggered ability. If an
    // ability of an artifact, creature, or planeswalker is countered this
    // way, that permanent loses all abilities for as long as this creature
    // remains on the battlefield." Oko's +2 (Food) is countered; Oko keeps
    // its loyalty (the cost was paid) and has no abilities.
    let mut g = scen().battlefield(0, &["Oko, Thief of Crowns"]).battlefield(1, &["Island"; 3]).hand(1, &[TIDEBINDER]).start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(0)));
    finish(&mut g, |_| None);
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, stack_target);
    assert!(tokens_named(&g, "Food").is_empty(), "countered");
    assert_eq!(g.counters_on(oko, Counter::Loyalty), 6, "4 + 2 paid");
    // No loyalty ability (it lost them all), on a later turn too.
    assert!(!g.has_option(Choice::Ability(oko, AbilitySlot::Activated(1))));
}

#[test]
fn tidebinder_counters_a_triggered_ability_whose_source_is_gone() {
    // A dies trigger (Test Dies Draw: "When this dies, draw a card"): its
    // source is in the graveyard, so nothing loses its abilities.
    let mut g = scen()
        .battlefield(0, &["Test Dies Draw", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"; 3])
        .hand(1, &[TIDEBINDER])
        .start();
    let dd = bf(&g, 0, "Test Dies Draw");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, dd));
    g.pass();
    g.pass(); // the Bolt resolves; the dies trigger goes on the stack
    g.drive_until(|g| at_priority(g) && !g.stack().is_empty());
    assert_eq!(g.turn().priority, 0);
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, stack_target);
    assert_eq!(g.player(0).hand.len(), hand, "the dies trigger was countered");
    assert!(g.effects().is_empty(), "nothing lost its abilities");
}

#[test]
fn tidebinder_has_flash() {
    let mut g = scen().battlefield(0, &["Island"; 3]).hand(0, &[TIDEBINDER]).at(Step::Upkeep, 0).start();
    g.drive_until(at_priority);
    let tb = g.card(0, TIDEBINDER, Zone::Hand);
    assert!(g.has_option(Choice::Cast(tb)), "at instant speed");
}

// ---- Ertai Resurrected ------------------------------------------------------------------------

#[test]
fn ertai_counters_a_spell_and_its_controller_draws() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island", "Swamp", "Swamp"])
        .hand(1, &["Ertai Resurrected"])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| player_if(g, 1));
    g.pass();
    let er = g.card(1, "Ertai Resurrected", Zone::Hand);
    g.choose(Choice::Cast(er));
    finish(&mut g, |_| None);
    let hand0 = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(0)) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(bolt)),
        _ => None,
    });
    assert_eq!(g.life(1), 20);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), hand0 + 1, "the Bolt's controller drew");
}

#[test]
fn ertai_may_choose_no_mode_and_cant_destroy_itself() {
    // "Choose up to one"; "destroy another target creature or
    // planeswalker".
    let mut g = scen().battlefield(0, &["Island", "Island", "Swamp", "Swamp"]).hand(0, &["Ertai Resurrected"]).start();
    let er = g.card(0, "Ertai Resurrected", Zone::Hand);
    g.choose(Choice::Cast(er));
    finish(&mut g, |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    // No spell or ability to counter, no other creature: only no mode.
    assert_eq!(options(&g), vec![Choice::Pass]);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(er).zone, Zone::Battlefield);
}

// ---- Invert Polarity: in rules_stack.rs (both flips, new targets) ------------------------------

#[test]
fn invert_polarity_targets_only_spells() {
    let g = scen().battlefield(0, &["Island", "Island", "Mountain", "Test Bear"]).hand(0, &["Invert Polarity"]).start();
    let ip = g.card(0, "Invert Polarity", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(ip)), "no spell to target");
}

// ---- Leovold, Emissary of Trest ---------------------------------------------------------------

const LEOVOLD: &str = "Leovold, Emissary of Trest";

#[test]
fn leovold_opponents_draw_at_most_one_card_each_turn() {
    // ruling: f8af3256
    // "Each opponent can't draw more than one card each turn."
    let mut g = scen().battlefield(0, &[LEOVOLD]).battlefield(1, &["Island"]).hand(1, &["Ancestral Recall"]).at(Step::Main1, 1).start();
    let ar = g.card(1, "Ancestral Recall", Zone::Hand);
    g.choose(Choice::Cast(ar));
    finish(&mut g, |g| player_if(g, 1));
    let hand = g.player(1).hand.len();
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(1).hand.len(), hand + 1, "one of three");
}

#[test]
fn leovold_may_draw_when_a_permanent_you_control_is_targeted() {
    // "Whenever you or a permanent you control becomes the target of a
    // spell or ability an opponent controls, you may draw a card." Declined.
    let mut g = scen()
        .battlefield(0, &[LEOVOLD, "Test Bear"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, bear));
    let hand = g.player(0).hand.len();
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::YesNo)).then(|| {
            asked.set(true);
            Choice::Pass
        })
    });
    assert!(asked.get(), "a \"may\"");
    assert_eq!(g.player(0).hand.len(), hand);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

// ---- Surrak, Elusive Hunter -------------------------------------------------------------------

#[test]
fn surrak_draws_when_a_creature_you_control_is_targeted_by_an_opponents_ability() {
    // "Whenever a creature you control or a creature spell you control
    // becomes the target of a spell or ability an opponent controls, draw a
    // card." P1's Walking Ballista pings P0's Test Bear.
    let mut g = scen()
        .battlefield(0, &["Surrak, Elusive Hunter", "Test Bear"])
        .battlefield(1, &["Walking Ballista"])
        .counters(1, "Walking Ballista", Counter::P1P1, 1)
        .at(Step::Main1, 1)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let wb = bf(&g, 1, "Walking Ballista");
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    finish(&mut g, |g| tgt_if(g, bear));
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.obj(bear).damage, 1);
}

#[test]
fn surrak_has_trample() {
    let g = scen().battlefield(0, &["Surrak, Elusive Hunter"]).start();
    let s = bf(&g, 0, "Surrak, Elusive Hunter");
    assert!(g.has_kw(s, Kw::Trample));
    assert_eq!(g.pt(s), (4, 3));
}

// ---- Sedgemoor Witch -------------------------------------------------------------------------

#[test]
fn sedgemoor_witch_magecraft_makes_a_pest_that_gains_life_as_it_dies() {
    // "Menace. Ward-Pay 3 life. Magecraft - Whenever you cast or copy an
    // instant or sorcery spell, create a 1/1 black and green Pest creature
    // token with 'When this token dies, you gain 1 life.'"
    let mut g = scen().battlefield(0, &["Sedgemoor Witch", "Mountain", "Mountain"]).hand(0, &["Lightning Bolt", "Lightning Bolt"]).start();
    let w = bf(&g, 0, "Sedgemoor Witch");
    assert!(g.has_kw(w, Kw::Menace));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| player_if(g, 1));
    resolve_with(&mut g, |_| None);
    let pests = tokens_named(&g, "Pest");
    assert_eq!(pests.len(), 1);
    assert_eq!(g.pt(pests[0]), (1, 1));
    // Bolt the Pest: it dies, P0 gains 1 life (and the second Bolt makes
    // another Pest).
    let bolt2 = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt2));
    finish(&mut g, |g| tgt_if(g, pests[0]));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(0), 21);
    assert_eq!(tokens_named(&g, "Pest").len(), 1);
}

// ---- Kappa Cannoneer -------------------------------------------------------------------------

const KAPPA: &str = "Kappa Cannoneer";

#[test]
fn kappa_cannoneer_improvise_taps_artifacts_for_generic_mana_cr_702_126a() {
    // "Improvise. Ward {4}. Whenever this creature or another artifact you
    // control enters, put a +1/+1 counter on this creature. It can't be
    // blocked this turn." {5}{U}: two Islands and four artifacts.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Sensei's Divining Top", "Mishra's Bauble", "Urza's Bauble", "Grindstone"])
        .hand(0, &[KAPPA])
        .start();
    let k = g.card(0, KAPPA, Zone::Hand);
    assert!(g.has_option(Choice::Cast(k)), "castable with improvise");
    g.choose(Choice::Cast(k));
    // The Islands first (canonical class order), then the artifacts, each
    // offered by its permanent slot (improvise: `MY_PERM`).
    let improvised = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::PayMana) && options(g).iter().any(|c| matches!(c, Choice::Card(_))) {
            improvised.set(improvised.get() + 1);
        }
        None
    });
    assert_eq!(improvised.get(), 4, "four artifacts tapped for improvise");
    let k = bf(&g, 0, KAPPA);
    assert!(g.battlefield().iter().all(|&c| c == k || g.obj(c).tapped), "every artifact and land tapped");
    assert_eq!(g.counters_on(k, Counter::P1P1), 1);
    assert_eq!(g.pt(k), (5, 5));
}

#[test]
fn improvise_pays_only_generic_mana_cr_702_126a() {
    // Six artifacts and no blue source: the {U} can't be paid.
    let g = scen()
        .battlefield(0, &["Sensei's Divining Top", "Mishra's Bauble", "Urza's Bauble", "Grindstone", "Aether Spellbomb", "Pyrite Spellbomb"])
        .hand(0, &[KAPPA])
        .start();
    let k = g.card(0, KAPPA, Zone::Hand);
    assert!(!g.has_option(Choice::Cast(k)));
}

#[test]
fn kappa_cannoneer_grows_and_cant_be_blocked_when_an_artifact_enters() {
    let mut g = scen().battlefield(0, &[KAPPA]).battlefield(1, &["Test Bear"]).hand(0, &["Mishra's Bauble"]).start();
    let k = bf(&g, 0, KAPPA);
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.counters_on(k, Counter::P1P1), 1);
    attack_with(&mut g, &[k]);
    g.drive_until(|g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 15, "unblockable this turn: 5 damage");
}

#[test]
fn kappa_cannoneer_has_ward_four() {
    let mut g = scen()
        .battlefield(0, &[KAPPA])
        .battlefield(1, &["Mountain", "Island", "Island", "Island"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let k = bf(&g, 0, KAPPA);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, k));
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::YesNo) {
            assert_eq!(options(g), vec![Choice::Pass], "three lands left: {{4}} can't be paid");
        }
        None
    });
    assert_eq!(g.obj(k).damage, 0);
}

// ---- Raffine, Scheming Seer -------------------------------------------------------------------

const RAFFINE: &str = "Raffine, Scheming Seer";

#[test]
fn raffine_target_attacking_creature_connives_x_cr_701_50a() {
    // "Flying, ward {1}. Whenever you attack, target attacking creature
    // connives X, where X is the number of attacking creatures." Two
    // attackers: draw two, discard two; a +1/+1 counter per nonland card
    // discarded.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Lightning Bolt", "Island", "Island", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &[RAFFINE, "Test Bear"])
        .hand(0, &["Forest"])
        .start();
    let r = bf(&g, 0, RAFFINE);
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(r, Kw::Flying));
    attack_with(&mut g, &[r, bear]);
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        // Discard the Bolt and the Forest.
        Some(DecisionKind::Discard) => options(g)
            .into_iter()
            .find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Lightning Bolt"))
            .or_else(|| options(g).into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Forest"))),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand, "drew two, discarded two");
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "one nonland card discarded");
}

// ---- Hexing Squelcher ----------------------------------------------------------------------

#[test]
fn hexing_squelcher_spells_you_control_cant_be_countered() {
    // "This spell can't be countered. Ward-Pay 2 life. Spells you control
    // can't be countered. Other creatures you control have 'Ward-Pay 2
    // life.'"
    let mut g = scen()
        .battlefield(0, &["Hexing Squelcher", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| player_if(g, 1));
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    finish(&mut g, |g| tgt_if(g, bolt));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 17, "the Bolt resolved");
}

#[test]
fn hexing_squelcher_itself_cant_be_countered() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Hexing Squelcher"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .start();
    let h = g.card(0, "Hexing Squelcher", Zone::Hand);
    g.choose(Choice::Cast(h));
    finish(&mut g, |_| None);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    finish(&mut g, |g| tgt_if(g, h));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(h).zone, Zone::Battlefield);
}

// ---- Sunset Saboteur -----------------------------------------------------------------------

#[test]
fn sunset_saboteur_attacks_and_puts_a_counter_on_an_opponents_creature() {
    // "Menace. Ward-Discard a card. Whenever this creature attacks, put a
    // +1/+1 counter on target creature an opponent controls."
    let mut g = scen().battlefield(0, &["Sunset Saboteur", "Test Bear"]).battlefield(1, &["Test Bear"]).start();
    let s = bf(&g, 0, "Sunset Saboteur");
    let theirs = bf(&g, 1, "Test Bear");
    let mine = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(s, Kw::Menace));
    attack_with(&mut g, &[s]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(theirs)) && !g.has_option(g.tgt(mine)), "an opponent's creature");
    resolve_with(&mut g, |g| tgt_if(g, theirs));
    assert_eq!(g.counters_on(theirs, Counter::P1P1), 1);
}
