//! M8d (ENGINE.md 37): the official rulings of the cube's replacement-effect
//! cards (Blightsteel Colossus, Dauthi Voidwalker, Yawgmoth's Will,
//! Hullbreacher, Quantum Riddler, Containment Priest, Suplex) as engine
//! tests: replacement effects (CR 614), their order when several apply
//! (616.1), infect (702.90), "dies" (700.4), playing a card with an effect's
//! permission (601.2, 305.2). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;

/// The first offered activated ability of `c`.
fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

/// Declare `attackers` (attacking the player), up to the blockers.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(1));
        }
    }
    g.choose(Choice::Pass);
}

fn poison(g: &Game, p: u8) -> u16 {
    g.player(p).poison
}

/// P1 (holding priority after P0 passes) takes `f`'s action in the declare
/// attackers step.
fn pass_once(g: &mut Game) {
    g.drive_until(at_priority);
    g.pass();
}

// ---- Blightsteel Colossus -------------------------------------------------------------------

#[test]
fn blightsteel_infect_damage_to_a_player_is_poison_and_ten_poison_loses_cr_702_90b_704_5c() {
    // ruling: 66755fa6 5736002e
    let mut g = scen().battlefield(0, &["Blightsteel Colossus"]).at(Step::DeclareAttackers, 0).start();
    let bs = bf(&g, 0, "Blightsteel Colossus");
    attack(&mut g, &[bs]);
    quiet_until(&mut g, |g| g.is_over() || g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "no life lost");
    assert_eq!(poison(&g, 1), 11);
    assert!(g.is_over() && g.player(1).lost, "ten or more poison counters (CR 704.5c)");
}

#[test]
fn blightsteel_infect_damage_kills_an_indestructible_blocker_with_counters_cr_702_90c() {
    // ruling: 66755fa6
    // The blocker gets -1/-1 counters, not damage: indestructible doesn't
    // save it (CR 704.5f, not 704.5g).
    let mut g = scen().battlefield(0, &["Blightsteel Colossus"]).battlefield(1, &["Test Indestructible"]).at(Step::DeclareAttackers, 0).start();
    let bs = bf(&g, 0, "Blightsteel Colossus");
    let ind = bf(&g, 1, "Test Indestructible");
    attack(&mut g, &[bs]);
    quiet_until_with(
        &mut g,
        |g| g.turn().step == Step::Main2,
        |g| match g.kind() {
            Some(DecisionKind::DeclareBlockers) => Some(Choice::Card(bs)),
            Some(DecisionKind::AssignDamage) => Some(Choice::Number(2)),
            _ => None,
        },
    );
    assert_eq!(g.obj(ind).zone, Zone::Graveyard);
    assert_eq!(poison(&g, 1), 9, "the trample excess as poison");
    assert_eq!(g.life(1), 20);
}

#[test]
fn blightsteel_infect_damage_to_a_planeswalker_removes_loyalty_cr_120_3c() {
    // ruling: 66755fa6
    let mut g = scen().battlefield(0, &["Blightsteel Colossus"]).battlefield(1, &["Jace, the Mind Sculptor"]).at(Step::DeclareAttackers, 0).start();
    let bs = bf(&g, 0, "Blightsteel Colossus");
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bs));
    g.choose(g.tgt(jace));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.obj(jace).zone, Zone::Graveyard, "lost its loyalty");
    assert_eq!(poison(&g, 1), 0);
    assert_eq!(g.life(1), 20);
}

#[test]
fn blightsteel_infect_damage_can_be_prevented_by_protection_from_colorless_cr_615_702_16e() {
    // ruling: e891f4a0 26c0eed3
    // Giver of Runes gives the Test Bear protection from colorless; it still
    // may block Blightsteel (protection restricts what blocks it, not what
    // it blocks), and the damage dealt to it is prevented: no counters.
    let mut g = scen()
        .battlefield(0, &["Blightsteel Colossus"])
        .battlefield(1, &["Giver of Runes", "Test Bear"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bs = bf(&g, 0, "Blightsteel Colossus");
    let giver = bf(&g, 1, "Giver of Runes");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, &[bs]);
    pass_once(&mut g);
    // P1: Giver of Runes on the Bear, protection from colorless.
    let a = act(&g, giver);
    g.choose(a);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => {
            let d = g.pending_decision().unwrap();
            // Number(0..4) = W U B R G, Number(5) = colorless.
            assert_eq!(d.options.len(), 6, "{}", opts(g));
            Some(Choice::Number(5))
        }
        _ => None,
    });
    assert!(g.protection_of(bear).colorless);
    quiet_until_with(
        &mut g,
        |g| g.turn().step == Step::Main2,
        |g| match g.kind() {
            Some(DecisionKind::DeclareBlockers) if g.pending_decision().unwrap().player == 1 => Some(Choice::Card(bs)),
            Some(DecisionKind::AssignDamage) => Some(Choice::Number(2)),
            _ => None,
        },
    );
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", g.log_lines().join("\n"));
    assert_eq!(g.counters_on(bear, Counter::M1M1), 0, "prevented");
    assert_eq!(poison(&g, 1), 9);
}

// ---- Dauthi Voidwalker ---------------------------------------------------------------------------

/// P0 Thoughtseizes `card` out of P1's hand (exiled with a void counter
/// under Dauthi Voidwalker).
fn thoughtseize(g: &mut Game, card: &str) -> CardIdx {
    let ts = g.card(0, "Thoughtseize", Zone::Hand);
    let target = g.card(1, card, Zone::Hand);
    g.choose(Choice::Cast(ts));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseHandCard) | Some(DecisionKind::ChooseDiscard) | Some(DecisionKind::Discard) => {
            g.has_option(Choice::Card(target)).then_some(Choice::Card(target))
        }
        _ => None,
    });
    assert_eq!(g.obj(target).zone, Zone::Exile, "voided\n{}", g.log_lines().join("\n"));
    target
}

/// Activate Dauthi Voidwalker choosing `card`.
fn dauthi_choose(g: &mut Game, card: CardIdx) {
    let d = bf(g, 0, "Dauthi Voidwalker");
    let a = act(g, d);
    g.choose(a);
    for _ in 0..50 {
        if at_priority(g) && !g.stack().is_empty() {
            g.pass();
            continue;
        }
        if g.kind() == Some(DecisionKind::ChooseExiled) {
            g.choose(Choice::Card(card));
            return;
        }
        if at_priority(g) {
            panic!("no exiled card chosen\n{}", g.log_lines().join("\n"));
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

#[test]
fn dauthi_an_opponents_creature_is_exiled_and_its_dies_ability_doesnt_trigger_cr_700_4() {
    // ruling: 2891b4c7
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Mountain"])
        .battlefield(1, &["Haywire Mite"])
        .hand(0, &["Lightning Bolt"])
        .start();
    let mite = bf(&g, 1, "Haywire Mite");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(mite);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(mite).zone, Zone::Exile);
    assert_eq!(g.life(1), 20, "\"when this creature dies, you gain 2 life\" didn't trigger");
}

#[test]
fn dauthi_an_opponents_token_still_dies_cr_111_7() {
    // ruling: 1f7c33df
    // P1's Servo (from Saheeli) equipped with Skullclamp is a 2/0 and dies:
    // Skullclamp's "whenever equipped creature dies" triggers.
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker"])
        .battlefield(1, &["Saheeli, Sublime Artificer", "Skullclamp", "Island"])
        .hand(1, &["Sol Ring"])
        .at(Step::Main1, 1)
        .start();
    let ring = g.card(1, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    resolve(&mut g);
    let servo = tokens_named(&g, "Servo")[0];
    let clamp = bf(&g, 1, "Skullclamp");
    let lib = g.player(1).library.len();
    let a = act(&g, clamp);
    g.choose(a);
    let t = g.tgt(servo);
    answer_targets(&mut g, &[t]);
    assert!(!g.battlefield().contains(&servo));
    assert_eq!(g.player(1).library.len(), lib - 2, "drew two\n{}", g.log_lines().join("\n"));
}

#[test]
fn dauthi_a_card_played_without_paying_its_mana_cost_has_x_zero_cr_107_3b() {
    // ruling: 9402dfbe
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Swamp"])
        .hand(0, &["Thoughtseize"])
        .hand(1, &["Walking Ballista"])
        .start();
    let wb = thoughtseize(&mut g, "Walking Ballista");
    dauthi_choose(&mut g, wb);
    assert!(g.has_option(Choice::Cast(wb)), "{}", opts(&g));
    g.choose(Choice::Cast(wb));
    resolve(&mut g);
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "X = 0: a 0/0 that dies\n{}", g.log_lines().join("\n"));
}

#[test]
fn dauthi_additional_costs_must_still_be_paid_cr_601_2f() {
    // ruling: 545961c8 a263d6ad
    // Abhorrent Oculus's "exile six cards from your graveyard": with five
    // cards in P0's graveyard it can't be cast; with six it can, and they
    // are exiled.
    // (Thoughtseize and the Dauthi go to P0's graveyard too: 3 + 2, 4 + 2.)
    for (gy, castable) in [(3usize, false), (4, true)] {
        let mut g = scen()
            .battlefield(0, &["Dauthi Voidwalker", "Swamp"])
            .graveyard(0, &vec!["Island"; gy])
            .hand(0, &["Thoughtseize"])
            .hand(1, &["Abhorrent Oculus"])
            .start();
        let oc = thoughtseize(&mut g, "Abhorrent Oculus");
        dauthi_choose(&mut g, oc);
        g.drive_until(at_priority);
        let n = g.player(0).graveyard.len();
        assert_eq!(n, gy + 2);
        assert_eq!(g.has_option(Choice::Cast(oc)), castable, "{n} cards: {}", opts(&g));
        if !castable {
            continue;
        }
        g.choose(Choice::Cast(oc));
        resolve(&mut g);
        assert_eq!(g.obj(oc).zone, Zone::Battlefield);
        assert_eq!(g.player(0).graveyard.len(), n - 6, "six exiled");
    }
}

#[test]
fn dauthi_a_card_played_this_way_follows_its_timing_cr_307_1() {
    // ruling: 19d382a1
    // A voided sorcery (Thoughtseize) can't be cast while a spell is on the
    // stack; once the stack is empty it can.
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Swamp", "Island"])
        .hand(0, &["Thoughtseize", "Sol Ring"])
        .hand(1, &["Thoughtseize"])
        .start();
    let ts1 = thoughtseize(&mut g, "Thoughtseize");
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1);
    dauthi_choose(&mut g, ts1);
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1, "Sol Ring still on the stack");
    assert!(!g.has_option(Choice::Cast(ts1)), "a sorcery, with a nonempty stack");
    resolve(&mut g);
    assert!(g.has_option(Choice::Cast(ts1)));
}

// ---- Yawgmoth's Will -------------------------------------------------------------------------------

fn cast_will(g: &mut Game) {
    let w = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(w));
    resolve(g);
}

#[test]
fn yawgmoths_will_exiles_itself_cr_614_1a() {
    // ruling: 8a5dcfec
    let mut g = scen().battlefield(0, &["Swamp"; 3]).hand(0, &["Yawgmoth's Will"]).start();
    cast_will(&mut g);
    assert_eq!(g.count_in(0, "Yawgmoth's Will", Zone::Exile), 1);
    assert_eq!(g.count_in(0, "Yawgmoth's Will", Zone::Graveyard), 0);
}

#[test]
fn yawgmoths_will_applies_to_costs_cr_614_1a() {
    // ruling: d58aedf3
    // A creature sacrificed to Goblin Bombardment's cost is exiled.
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp", "Goblin Bombardment", "Test Bear"]).hand(0, &["Yawgmoth's Will"]).start();
    cast_will(&mut g);
    let gb = bf(&g, 0, "Goblin Bombardment");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(&g, gb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Sacrifice) => Some(Choice::Card(bear)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.life(1), 19);
}

#[test]
fn yawgmoths_will_a_land_played_from_the_graveyard_is_the_land_drop_cr_305_2() {
    // ruling: 49e70d59
    let mut g = scen().battlefield(0, &["Swamp"; 3]).graveyard(0, &["Island", "Forest"]).hand(0, &["Yawgmoth's Will"]).start();
    cast_will(&mut g);
    let island = g.card(0, "Island", Zone::Graveyard);
    let forest = g.card(0, "Forest", Zone::Graveyard);
    assert!(g.has_option(Choice::Play(island)) && g.has_option(Choice::Play(forest)));
    g.choose(Choice::Play(island));
    g.drive_until(at_priority);
    assert_eq!(g.obj(island).zone, Zone::Battlefield);
    assert!(!g.has_option(Choice::Play(forest)), "one land per turn");
}

#[test]
fn yawgmoths_will_doesnt_let_you_cycle_from_the_graveyard_cr_702_29a() {
    // ruling: e99990e7
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp", "Island"]).graveyard(0, &["Lórien Revealed"]).hand(0, &["Yawgmoth's Will"]).start();
    cast_will(&mut g);
    let lr = g.card(0, "Lórien Revealed", Zone::Graveyard);
    let n = g.def_of(lr).activated.len() as u8;
    for i in 0..n.max(1) {
        assert!(!g.has_option(Choice::Ability(lr, AbilitySlot::Activated(i))), "islandcycling from the graveyard");
    }
}

// ---- Hullbreacher and Quantum Riddler ---------------------------------------------------------------

#[test]
fn two_hullbreachers_make_one_treasure_per_draw_cr_614_5() {
    // ruling: ebaa36d0
    let mut g = scen()
        .battlefield(0, &["Hullbreacher", "Hullbreacher"])
        .battlefield(1, &["Island"])
        .hand(1, &["Ancestral Recall"])
        .at(Step::Main1, 1)
        .start();
    let ar = g.card(1, "Ancestral Recall", Zone::Hand);
    cast_targets(&mut g, ar, &[player(1)]);
    assert_eq!(tokens_named(&g, "Treasure").len(), 3, "{}", g.log_lines().join("\n"));
    assert_eq!(g.player(1).hand.len(), 0);
}

#[test]
fn quantum_riddler_applies_to_the_whole_instruction_cr_614_1a() {
    // ruling: 11a4d136
    // Faithless Looting with no other card in hand: "draw two" becomes
    // "draw three" (not four), then discard two.
    let mut g = scen().battlefield(0, &["Quantum Riddler", "Mountain"]).hand(0, &["Faithless Looting"]).start();
    let fl = g.card(0, "Faithless Looting", Zone::Hand);
    let lib = g.player(0).library.len();
    g.choose(Choice::Cast(fl));
    resolve(&mut g);
    assert_eq!(g.player(0).library.len(), lib - 3);
    assert_eq!(g.player(0).hand.len(), 1);
}

#[test]
fn quantum_riddler_modifies_the_instruction_before_hullbreacher_replaces_each_draw_cr_616_1() {
    // ruling: 5a48fcc2 d9d5b8ea
    // P1 controls Quantum Riddler with no cards in hand and casts Ancestral
    // Recall on themself while P0 controls Hullbreacher: "draw three"
    // becomes "draw four" first; then each of the four draws is replaced by
    // a Treasure for P0.
    let mut g = scen()
        .battlefield(0, &["Hullbreacher"])
        .battlefield(1, &["Quantum Riddler", "Island"])
        .hand(1, &["Ancestral Recall"])
        .at(Step::Main1, 1)
        .start();
    let ar = g.card(1, "Ancestral Recall", Zone::Hand);
    cast_targets(&mut g, ar, &[player(1)]);
    assert_eq!(g.player(1).hand.len(), 0, "{}", g.log_lines().join("\n"));
    assert_eq!(tokens_named(&g, "Treasure").len(), 4, "{}", g.log_lines().join("\n"));
}

#[test]
fn hullbreacher_and_quantum_riddler_on_one_draw_the_drawer_gets_hullbreacher_first_cr_616_1() {
    // ruling: 60f3ab6b
    // P1's Quantum Riddler enters (P1's hand now empty) and its "draw a
    // card" meets P0's Hullbreacher: both replace the same draw and P1
    // chooses the order. Hullbreacher first: one Treasure and no draw left
    // for the Riddler. Riddler first: "draw two", each replaced: two
    // Treasures and no card either. The engine takes the first order for the
    // drawer (ENGINE.md 37); before M8d it took the second.
    let mut g = scen()
        .battlefield(0, &["Hullbreacher"])
        .battlefield(1, &["Island"; 5])
        .hand(1, &["Quantum Riddler"])
        .at(Step::Main1, 1)
        .start();
    let qr = g.card(1, "Quantum Riddler", Zone::Hand);
    g.choose(Choice::Cast(qr));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.obj(qr).zone, Zone::Battlefield);
    assert_eq!(g.player(1).hand.len(), 0);
    assert_eq!(tokens_named(&g, "Treasure").len(), 1, "{}", g.log_lines().join("
"));
}

// ---- Containment Priest ------------------------------------------------------------------------------

#[test]
fn containment_priest_doesnt_exile_a_creature_cast_from_the_graveyard_cr_614_1a() {
    // ruling: dfd5820c
    let mut g = scen()
        .battlefield(0, &["Containment Priest", "Swamp", "Swamp", "Swamp", "Forest", "Forest"])
        .graveyard(0, &["Test Bear"])
        .hand(0, &["Yawgmoth's Will"])
        .start();
    cast_will(&mut g);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(bear)));
    g.choose(Choice::Cast(bear));
    resolve(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
}

#[test]
fn containment_priest_entering_with_other_creatures_doesnt_exile_them_or_itself_cr_614_12() {
    // ruling: e9e31107 9cad2cc6
    // Exhume returns the Priest (P0) and a Test Giant (P1) at the same time.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .graveyard(0, &["Containment Priest"])
        .graveyard(1, &["Test Giant"])
        .hand(0, &["Exhume"])
        .start();
    let ex = g.card(0, "Exhume", Zone::Hand);
    g.choose(Choice::Cast(ex));
    resolve(&mut g);
    assert_eq!(g.count_in(0, "Containment Priest", Zone::Battlefield), 1);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Battlefield), 1, "{}", g.log_lines().join("\n"));
    // Reanimated alone, the Priest doesn't exile itself either.
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &["Containment Priest"]).hand(0, &["Reanimate"]).start();
    let r = g.card(0, "Reanimate", Zone::Hand);
    let p = g.card(0, "Containment Priest", Zone::Graveyard);
    let t = g.tgt(p);
    cast_targets(&mut g, r, &[t]);
    assert_eq!(g.obj(p).zone, Zone::Battlefield);
}

#[test]
fn containment_priest_exiles_a_manifested_card_cr_701_40_614_12() {
    // ruling: aa0d632c
    // A card manifested (by Abhorrent Oculus's manifest dread) would enter
    // as a 2/2 creature without being cast: exiled instead.
    let mut g = scen()
        .battlefield(0, &["Abhorrent Oculus"])
        .battlefield(1, &["Containment Priest"])
        .library(0, &["Island", "Island", "Lightning Bolt", "Test Bear"])
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Draw);
    assert!(g.battlefield().iter().all(|&c| !g.is_face_down(c)), "{}", g.log_lines().join("\n"));
    assert_eq!(g.exile_zone().len(), 1, "the manifested card is exiled\n{}", g.log_lines().join("\n"));
    assert_eq!(g.player(0).graveyard.len(), 1, "the other card");
}

// ---- Suplex ---------------------------------------------------------------------------------------------

#[test]
fn suplex_exiles_the_creature_if_it_would_die_later_this_turn_cr_614_1a() {
    // ruling: c3efcaa5
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Test Trampler"])
        .hand(0, &["Suplex", "Lightning Bolt"])
        .start();
    let tr = bf(&g, 1, "Test Trampler");
    let s = g.card(0, "Suplex", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(g.pending_decision().unwrap().options[0].choice),
        Some(DecisionKind::Target) => Some(g.tgt(tr)),
        _ => None,
    });
    assert_eq!(g.obj(tr).zone, Zone::Battlefield, "3 damage to a 4/4");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(tr);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(tr).zone, Zone::Exile);
    let _ = Target::Player(0);
}
