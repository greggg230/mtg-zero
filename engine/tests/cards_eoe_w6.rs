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

// ---- Moonlit Meditation (docs/ir_defects.md 30) --------------------------------

/// P0 casts Moonlit Meditation on its `host`.
fn meditate(g: &mut Game, host: engine::CardIdx) {
    let m = g.card(0, "Moonlit Meditation", Zone::Hand);
    start_with(g, Choice::Cast(m), |g| pick(g, &[g.tgt(host)]));
    settle(g);
    assert_eq!(g.obj(m).attached.map(|h| h.idx), Some(host), "{}", log(g));
}

fn tokens_named(g: &Game, p: u8, name: &str) -> usize {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.obj(c).owner == p && g.is_token(c)).count()
}

/// Moonlit Meditation: "Enchant artifact or creature you control / The first
/// time you would create one or more tokens each turn, you may instead
/// create that many tokens that are copies of enchanted permanent." (CR
/// 614.1a, 707.2.) Applied: Test Lander Maker's Lander becomes a token copy
/// of the enchanted Test Bear (any token, its ruling: a predefined artifact
/// token too). The next creation that turn is not the first: a Lander, no
/// question. Declined: a Lander, and no choice again that turn (the ruling).
#[test]
fn cr_614_1a_moonlit_meditation_replaces_the_first_tokens_with_copies() {
    for apply in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Island", "Forest", "Forest", "Forest", "Forest", "Test Bear"])
            .hand(0, &["Moonlit Meditation", "Test Lander Maker", "Test Lander Maker"])
            .at(Step::Main1, 0)
            .start();
        let bear = bf(&g, 0, "Test Bear");
        meditate(&mut g, bear);
        let med = bf(&g, 0, "Moonlit Meditation");
        let mut asked = 0;
        for _ in 0..2 {
            let c = g.card(0, "Test Lander Maker", Zone::Hand);
            start_with(&mut g, Choice::Cast(c), |_| None);
            for _ in 0..50 {
                g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
                if g.stack().is_empty() && at_priority(&g) {
                    break;
                }
                if at_priority(&g) {
                    g.pass();
                    continue;
                }
                if g.kind() == Some(DecisionKind::ChoosePermanent) && g.has_option(Choice::Card(med)) {
                    asked += 1;
                    assert!(g.has_option(Choice::Pass), "a may");
                    g.choose(if apply { Choice::Card(med) } else { Choice::Pass });
                    continue;
                }
                let ch = g.pending_decision().unwrap().options[0].choice;
                g.choose(ch);
            }
        }
        assert_eq!(asked, 1, "asked the first time only\n{}", log(&g));
        assert_eq!(tokens_named(&g, 0, "Test Bear"), apply as usize, "{}", log(&g));
        assert_eq!(landers(&g, 0), if apply { 1 } else { 2 }, "{}", log(&g));
    }
}

/// "That many tokens": Cosmogrand Zenith's "create two 1/1 white Human
/// Soldier creature tokens" (its trigger on the second spell of the turn,
/// Honor) becomes two copies of the enchanted creature.
#[test]
fn cr_614_1a_moonlit_meditation_makes_that_many_copies() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Plains", "Cosmogrand Zenith", "Test Bear"])
        .hand(0, &["Moonlit Meditation", "Honor"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    meditate(&mut g, bear);
    let med = bf(&g, 0, "Moonlit Meditation");
    let honor = g.card(0, "Honor", Zone::Hand);
    start_with(&mut g, Choice::Cast(honor), |g| pick(g, &[g.tgt(bear)]));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        Some(DecisionKind::ChoosePermanent) => pick(g, &[Choice::Card(med)]),
        _ => None,
    });
    assert_eq!(tokens_named(&g, 0, "Test Bear"), 2, "{}", log(&g));
    assert_eq!(tokens_named(&g, 0, "Human Soldier"), 0);
}

/// "If you create one or more tokens, and then Moonlit Meditation comes
/// under your control that same turn, the replacement effect won't apply
/// to any tokens you create for the rest of the turn" (ruling).
#[test]
fn cr_614_1a_moonlit_meditation_after_tokens_this_turn_waits() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Forest", "Forest", "Forest", "Forest", "Test Bear"])
        .hand(0, &["Test Lander Maker", "Moonlit Meditation", "Test Lander Maker"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Test Lander Maker", |_| None);
    let bear = bf(&g, 0, "Test Bear");
    meditate(&mut g, bear);
    let med = bf(&g, 0, "Moonlit Meditation");
    cast_and_resolve(&mut g, 0, "Test Lander Maker", |g| {
        assert!(!(g.kind() == Some(DecisionKind::ChoosePermanent) && g.has_option(Choice::Card(med))), "not this turn");
        None
    });
    assert_eq!(landers(&g, 0), 2, "{}", log(&g));
}

// ---- Thespian's Stage ---------------------------------------------------------

/// The place of Thespian's Stage's "{2}, {T}: This land becomes a copy of
/// target land, except it has this ability" among `c`'s activated
/// abilities, if `c` has it.
fn stage_ability(g: &Game, c: engine::CardIdx) -> Option<u8> {
    let s = db().def(def("Thespian's Stage"));
    let a = s.activated.iter().find(|a| matches!(a.effect, engine::db::ActEffect::Stack(_)))?;
    g.def_of(c).activated.iter().position(|x| x == a).map(|i| i as u8)
}

/// P0 activates `stage`'s copy ability on `target`, paying with the first
/// sources.
fn stage_copy(g: &mut Game, stage: engine::CardIdx, target: engine::CardIdx) {
    let k = stage_ability(g, stage).expect("the Stage's ability");
    // (Paid with Forests: another Stage stays untapped.)
    start_with(g, act(stage, k), |g| {
        pick(g, &[g.tgt(target)]).or_else(|| {
            g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Ability(x, _) if g.name(*x) == "Forest"))
        })
    });
    settle(g);
}

/// Thespian's Stage: "{T}: Add {C}. / {2}, {T}: This land becomes a copy of
/// target land, except it has this ability." It becomes Mutavault (CR
/// 707.2: its copiable values), keeps the copy ability (707.9b) and loses
/// its first one (the ruling); it stays tapped (the ruling). The next turn
/// it has Mutavault's abilities and the Stage's, and copies another land:
/// a Forest with the Stage's ability.
#[test]
fn cr_707_9b_thespians_stage_becomes_a_copy_and_keeps_this_ability() {
    let mut g = scen()
        .battlefield(0, &["Thespian's Stage", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Mutavault", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let stage = bf(&g, 0, "Thespian's Stage");
    let muta = bf(&g, 1, "Mutavault");
    stage_copy(&mut g, stage, muta);
    assert_eq!(g.name(stage), "Mutavault", "{}", log(&g));
    assert!(g.obj(stage).tapped, "not untapped by becoming a copy");
    let m = db().def(def("Mutavault"));
    assert_eq!(g.def_of(stage).activated.len(), m.activated.len() + 1, "Mutavault's abilities and the Stage's");
    assert!(stage_ability(&g, stage).is_some());
    quiet_until(&mut g, main1_of(1));
    quiet_until(&mut g, main1_of(0));
    let forest = bf(&g, 1, "Forest");
    stage_copy(&mut g, stage, forest);
    assert_eq!(g.name(stage), "Forest", "{}", log(&g));
    assert!(stage_ability(&g, stage).is_some(), "still has this ability");
}

/// A copy of a Stage that became a copy has the copy ability too (CR
/// 707.9b: the exception is part of the copiable values), once: a second
/// Stage copying the first (Mutavault with the ability) is that Mutavault.
/// Copiable values only (the ruling): an animated Mutavault is copied as the
/// land it is printed as, not a creature.
#[test]
fn cr_707_9b_a_copy_of_the_copy_keeps_the_ability_once() {
    let mut g = scen()
        .battlefield(0, &["Thespian's Stage", "Thespian's Stage", "Forest", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Mutavault"])
        .at(Step::Main1, 0)
        .start();
    let stages: Vec<_> = g.permanents_of(0).filter(|&c| g.name(c) == "Thespian's Stage").collect();
    let muta = bf(&g, 1, "Mutavault");
    stage_copy(&mut g, stages[0], muta);
    stage_copy(&mut g, stages[1], stages[0]);
    assert_eq!(g.name(stages[1]), "Mutavault", "{}", log(&g));
    let m = db().def(def("Mutavault"));
    assert_eq!(g.def_of(stages[1]).activated.len(), m.activated.len() + 1, "one instance of the Stage's ability");
    assert!(!g.is_type(stages[1], engine::db::types::CREATURE));
}

/// The other order (CR 613.7, 514.2): P0 takes P1's creature with Systems
/// Override until end of turn, then puts Tractor Beam on it; when the
/// "until end of turn" effect ends, Tractor Beam's later effect still gives
/// it to P0.
#[test]
fn cr_514_2_an_ending_control_effect_leaves_the_auras() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Systems Override", "Tractor Beam"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let so = g.card(0, "Systems Override", Zone::Hand);
    start_with(&mut g, Choice::Cast(so), |g| pick(g, &[g.tgt(bear)]));
    settle(&mut g);
    assert_eq!(g.obj(bear).controller, 0, "{}", log(&g));
    beam(&mut g, bear);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.obj(bear).controller, 0, "Tractor Beam's effect remains\n{}", log(&g));
}
