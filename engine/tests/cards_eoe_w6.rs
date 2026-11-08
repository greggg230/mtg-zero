//! Card tests of the Edge of Eternities cards W6 finishes (ENGINE.md 99.16),
//! each ability against its Oracle text and the official rulings.

mod eoe_b;

use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_b::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// P1 casts `spell` for alternative cost `alt` (0: its mana cost) with X =
/// `x`; P0 gets priority with it on the stack.
fn p1_casts_with(g: &mut Game, spell: &str, alt: u8, x: u8) {
    let c = g.card(1, spell, Zone::Hand);
    start_with(g, Choice::Cast(c), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(alt)),
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(x)) => Some(Choice::Number(x)),
        _ => None,
    });
    assert_eq!(g.stack().len(), 1, "{spell} on the stack\n{}", log(g));
    g.pass();
    assert!(at_priority(g) && g.pending_decision().unwrap().player == 0, "P0's priority\n{}", log(g));
}

// ---- Unravel (docs/ir_defects.md 44) -----------------------------------------

/// Unravel: "Counter target spell. If the amount of mana spent to cast that
/// spell was less than its mana value, you draw a card." Broodguard Elite
/// ({X}{G}{G}, warp {X}{G}) cast for its mana cost with X = 1 spent 3, its
/// mana value 3 (X counts on the stack: CR 202.3e, Unravel's ruling of
/// 2025-07-25): no card. Cast for warp with X = 1 it spent 2: a card (an
/// alternative cost doesn't change the mana value, CR 118.9c, the other
/// ruling). Germinating Wurm warped ({1}{G} for a 5-mana spell): a card.
#[test]
fn cr_601_2h_unravel_draws_when_less_mana_was_spent_than_the_mana_value() {
    for (spell, alt, x, draws) in [("Broodguard Elite", 0u8, 1u8, false), ("Broodguard Elite", 1, 1, true), ("Germinating Wurm", 1, 0, true), ("Germinating Wurm", 0, 0, false)] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Island"])
            .hand(0, &["Unravel"])
            .battlefield(1, &["Forest"; 5])
            .hand(1, &[spell])
            .at(Step::Main1, 1)
            .start();
        p1_casts_with(&mut g, spell, alt, x);
        let hand = g.player(0).hand.len();
        let u = g.card(0, "Unravel", Zone::Hand);
        start_with(&mut g, Choice::Cast(u), |_| None);
        settle(&mut g);
        assert!(g.find_card(1, spell, Zone::Graveyard).is_some(), "{spell} countered\n{}", log(&g));
        // (Unravel left the hand; a card drawn makes up for it.)
        assert_eq!(g.player(0).hand.len(), hand - 1 + draws as usize, "{spell} alt {alt} x {x}\n{}", log(&g));
    }
}

// ---- Tractor Beam (docs/ir_defects.md 29) -------------------------------------

/// P0 casts Tractor Beam on `target` (P1's) and runs its enters trigger.
fn beam(g: &mut Game, target: engine::CardIdx) {
    let tb = g.card(0, "Tractor Beam", Zone::Hand);
    start_with(g, Choice::Cast(tb), |g| pick(g, &[g.tgt(target)]));
    settle(g);
}

/// Tractor Beam: "Enchant creature or Spacecraft / When this Aura enters, tap
/// enchanted permanent. / You control enchanted permanent. / Enchanted
/// permanent doesn't untap during its controller's untap step." On P1's
/// creature: tapped, P0 controls it (CR 613.1b, a control-changing effect of
/// a static ability, 611.3a), and it stays tapped through P0's untap step
/// (CR 502.3). When the Aura leaves (Shattered Wings), the effect ends and
/// P1 controls the creature again (CR 611.3b).
#[test]
fn cr_613_1b_tractor_beam_controls_taps_and_holds_the_enchanted_permanent() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Tractor Beam"])
        .battlefield(1, &["Test Bear", "Forest", "Forest", "Forest"])
        .hand(1, &["Shattered Wings"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    beam(&mut g, bear);
    assert_eq!(g.obj(bear).controller, 0, "P0 controls it\n{}", log(&g));
    assert!(g.obj(bear).tapped, "the enters trigger tapped it");
    // P1's turn: not P1's to untap; P0's next turn: it doesn't untap.
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.obj(bear).controller, 0);
    let wings = g.card(1, "Shattered Wings", Zone::Hand);
    let tb = bf(&g, 0, "Tractor Beam");
    start_with(&mut g, Choice::Cast(wings), |g| pick(g, &[g.tgt(tb)]));
    settle(&mut g);
    assert_eq!(g.obj(tb).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(bear).controller, 1, "the effect ended with the Aura\n{}", log(&g));
}

/// "Enchanted permanent doesn't untap during its controller's untap step"
/// (CR 502.3): through P0's next untap step the stolen creature stays
/// tapped, while P0's lands untap.
#[test]
fn cr_502_3_the_enchanted_permanent_doesnt_untap() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Tractor Beam"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    beam(&mut g, bear);
    quiet_until(&mut g, main1_of(1));
    quiet_until(&mut g, main1_of(0));
    assert!(g.obj(bear).tapped, "{}", log(&g));
    assert!(g.permanents_of(0).filter(|&c| g.name(c) == "Island").all(|c| !g.obj(c).tapped));
}

/// Control-changing effects apply in timestamp order (CR 613.7): P1 casts
/// Systems Override ("Gain control of target artifact or creature until end
/// of turn. Untap that permanent. ...") on its creature that Tractor Beam
/// took: P1 controls it this turn (the later effect); when that effect ends
/// (CR 514.2), Tractor Beam's (still applying) gives it back to P0, not to
/// its owner.
#[test]
fn cr_613_7_a_later_control_effect_wins_and_the_aura_takes_over_when_it_ends() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Tractor Beam"])
        .battlefield(1, &["Test Bear", "Mountain", "Mountain", "Mountain"])
        .hand(1, &["Systems Override"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    beam(&mut g, bear);
    quiet_until(&mut g, main1_of(1));
    let so = g.card(1, "Systems Override", Zone::Hand);
    start_with(&mut g, Choice::Cast(so), |g| pick(g, &[g.tgt(bear)]));
    settle(&mut g);
    assert_eq!(g.obj(bear).controller, 1, "Systems Override's later effect\n{}", log(&g));
    assert!(!g.obj(bear).tapped, "untapped by Systems Override");
    quiet_until(&mut g, main1_of(0));
    assert_eq!(g.obj(bear).controller, 0, "back to Tractor Beam's controller\n{}", log(&g));
}

/// "Enchant creature or Spacecraft": a Spacecraft below its station
/// threshold (a noncreature artifact) is a legal target; a noncreature
/// artifact that isn't one is not.
#[test]
fn cr_303_4a_tractor_beam_enchants_a_spacecraft() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Tractor Beam"])
        .battlefield(1, &["Wurmwall Sweeper", "Nutrient Block"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 1, "Wurmwall Sweeper");
    let block = bf(&g, 1, "Nutrient Block");
    let tb = g.card(0, "Tractor Beam", Zone::Hand);
    g.choose(Choice::Cast(tb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(g.has_option(g.tgt(ship)) && !g.has_option(g.tgt(block)), "{:?}", g.options_desc());
    g.choose(g.tgt(ship));
    while !at_priority(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    settle(&mut g);
    assert_eq!(g.obj(ship).controller, 0, "{}", log(&g));
    assert!(g.obj(ship).tapped);
}

// ---- Starfield Vocalist (docs/ir_defects.md 66) --------------------------------

fn landers(g: &Game, p: u8) -> usize {
    g.permanents_of(p).filter(|&c| g.name(c) == "Lander" && g.def_of(c).token).count()
}

/// Starfield Vocalist: "If a permanent entering the battlefield causes a
/// triggered ability of a permanent you control to trigger, that ability
/// triggers an additional time." A creature's own "when this enters" (its
/// ruling: "a permanent's own 'enters' triggered abilities"): twice; with
/// two Vocalists three times, not four (CR 603.2d, the ruling). An
/// opponent's creature's own ability isn't an ability of a permanent you
/// control: once.
#[test]
fn cr_603_2d_starfield_vocalist_makes_enters_triggers_trigger_again() {
    for (vocalists, caster, want) in [(1usize, 0u8, 2usize), (2, 0, 3), (1, 1, 1), (0, 0, 1)] {
        let mut bf0: Vec<&str> = vec!["Forest", "Forest"];
        bf0.extend(std::iter::repeat_n("Starfield Vocalist", vocalists));
        let mut g = scen()
            .battlefield(0, &bf0)
            .battlefield(1, &["Forest", "Forest"])
            .hand(caster, &["Test Lander Maker"])
            .at(Step::Main1, caster)
            .start();
        cast_and_resolve(&mut g, caster, "Test Lander Maker", |_| None);
        assert_eq!(landers(&g, caster), want, "vocalists {vocalists} caster P{caster}\n{}", log(&g));
    }
}

/// "If a permanent entering the battlefield at the same time as Starfield
/// Vocalist (including Starfield Vocalist itself) causes a triggered ability
/// of a permanent you control to trigger, that ability triggers an
/// additional time" (ruling of 2025-07-25). Endless Sands returns both
/// creature cards it exiled together ("return each creature card exiled
/// with this land to the battlefield"), in either order: two Landers.
#[test]
fn cr_603_2d_starfield_vocalist_entering_at_the_same_time() {
    for vocalist_first in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Endless Sands", "Starfield Vocalist", "Test Lander Maker"])
            .battlefield(0, &["Forest"; 6])
            .at(Step::Main1, 0)
            .start();
        let sands = bf(&g, 0, "Endless Sands");
        let (voc, maker) = (bf(&g, 0, "Starfield Vocalist"), bf(&g, 0, "Test Lander Maker"));
        let order = if vocalist_first { [voc, maker] } else { [maker, voc] };
        let slot = |k: u8| Choice::Ability(sands, engine::db::AbilitySlot::Activated(k));
        for (turn, &c) in order.iter().enumerate() {
            if turn > 0 {
                quiet_until(&mut g, main1_of(1));
                quiet_until(&mut g, main1_of(0));
            }
            let ex = if g.has_option(slot(1)) { slot(1) } else { slot(0) };
            start_with(&mut g, ex, |g| pick(g, &[g.tgt(c)]));
            settle(&mut g);
            assert_eq!(g.obj(c).zone, Zone::Exile, "{}", log(&g));
        }
        quiet_until(&mut g, main1_of(1));
        quiet_until(&mut g, main1_of(0));
        let before = landers(&g, 0);
        start_with(&mut g, slot(2), |_| None);
        settle(&mut g);
        assert_eq!(g.find_card(0, "Starfield Vocalist", Zone::Battlefield).is_some(), true, "{}", log(&g));
        assert_eq!(landers(&g, 0) - before, 2, "vocalist first {vocalist_first}\n{}", log(&g));
    }
}

// ---- Plaza of Heroes (docs/ir_defects.md 160) ----------------------------------

/// Plaza of Heroes' third ability: "{T}: Add one mana of any color among
/// legendary permanents you control" (no spending restriction, docs/
/// ir_defects.md 160): with a red legendary creature (Tannuk, Steadfast
/// Second), Plasma Bolt ({R}) can be cast with the Plaza alone; with a
/// nonlegendary red creature, it can't (the second ability's mana is only
/// for legendary spells; CR 106.6).
#[test]
fn cr_106_6_plaza_of_heroes_makes_a_colour_among_legendary_permanents() {
    for legend in [true, false] {
        let red = if legend { "Tannuk, Steadfast Second" } else { "Sunset Saboteur" };
        let g = scen().battlefield(0, &["Plaza of Heroes", red]).hand(0, &["Plasma Bolt"]).at(Step::Main1, 0).start();
        let bolt = g.card(0, "Plasma Bolt", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(bolt)), legend, "{red}: {:?}", g.options_desc());
    }
}
