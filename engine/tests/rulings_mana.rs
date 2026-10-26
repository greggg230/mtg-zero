//! M8d (ENGINE.md 37): the official rulings about costs and mana as engine
//! tests: the total cost (CR 601.2f: an alternative cost plus increases),
//! paying life (119.4), Phyrexian mana (107.4f), X in a card outside the
//! stack (107.3b), delve (702.66), targets chosen before costs are paid
//! (602.2b, 601.2c). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::AbilitySlot;
use engine::{Choice, DecisionKind, Step, Zone};
use rulings_util::*;

#[test]
fn thalias_tax_applies_to_its_controllers_own_spells_cr_601_2f() {
    // ruling: 032de84a
    let g = scen().battlefield(0, &["Thalia, Guardian of Thraben", "Mountain"]).hand(0, &["Lightning Bolt"]).start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(b)), "{{R}} plus {{1}} with one land");
    let g = scen().battlefield(0, &["Thalia, Guardian of Thraben", "Mountain", "Mountain"]).hand(0, &["Lightning Bolt"]).start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(b)));
}

#[test]
fn thalias_tax_applies_to_an_alternative_cost_cr_601_2f() {
    // ruling: 560a16f0 a00dc8c2 f94acd2b
    // Force of Will's "pay 1 life and exile a blue card" is an alternative
    // cost; Thalia adds {1}: castable with one Island, not with none.
    for (lands, castable) in [(&[][..], false), (&["Island"][..], true)] {
        let mut g = scen()
            .battlefield(0, &["Thalia, Guardian of Thraben", "Test Bear"])
            .battlefield(1, lands)
            .hand(0, &["Lightning Bolt"])
            .hand(1, &["Force of Will", "Brainstorm"])
            .battlefield(0, &["Mountain", "Mountain"])
            .start();
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        cast_until_priority(&mut g, b);
        g.pass();
        let fow = g.card(1, "Force of Will", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(fow)), castable, "{:?}: {}", lands, opts(&g));
    }
    // Daze: "return an Island", plus {1}: with the Island the only land, the
    // Island is tapped for the {1} (CR 601.2g) and then returned (601.2h:
    // the costs in any order; a tapped Island may be returned).
    let mut g = scen()
        .battlefield(0, &["Thalia, Guardian of Thraben", "Mountain", "Mountain"])
        .battlefield(1, &["Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Daze"])
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_until_priority(&mut g, b);
    g.pass();
    let d = g.card(1, "Daze", Zone::Hand);
    assert!(g.has_option(Choice::Cast(d)), "{}", opts(&g));
    g.choose(Choice::Cast(d));
    for _ in 0..20 {
        if at_priority(&g) {
            break;
        }
        let dd = g.pending_decision().unwrap();
        let c = match dd.kind {
            DecisionKind::ChooseAltCost => dd.options.iter().map(|o| o.choice).find(|&c| c != Choice::Number(0)).unwrap_or(dd.options[0].choice),
            DecisionKind::Target => dd.options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(engine::Target::Stack(_)))).unwrap_or(dd.options[0].choice),
            _ => dd.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.obj(d).zone, Zone::Stack, "{}", g.log_lines().join("
"));
    assert_eq!(g.count_in(1, "Island", Zone::Hand), 1, "the Island returned");
}

/// Cast `c` (P0) aimed at P1, up to P0's priority with it on the stack.
fn cast_until_priority(g: &mut engine::Game, c: engine::CardIdx) {
    g.choose(Choice::Cast(c));
    for _ in 0..20 {
        if at_priority(g) {
            return;
        }
        let d = g.pending_decision().unwrap();
        let ch = if g.has_option(player(1)) { player(1) } else { d.options[0].choice };
        g.choose(ch);
    }
}

#[test]
fn phyrexian_mana_cant_be_paid_with_life_you_dont_have_cr_119_4() {
    // ruling: cab93df4
    let g = scen().hand(0, &["Gitaxian Probe"]).life(0, 1).start();
    let p = g.card(0, "Gitaxian Probe", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(p)), "2 life at 1");
    let g = scen().hand(0, &["Gitaxian Probe"]).life(0, 2).start();
    let p = g.card(0, "Gitaxian Probe", Zone::Hand);
    assert!(g.has_option(Choice::Cast(p)), "2 life at 2");
}

#[test]
fn griselbrand_cant_pay_seven_life_at_six_cr_119_4() {
    // ruling: 0cd49558
    for (life, ok) in [(6, false), (7, true)] {
        let g = scen().battlefield(0, &["Griselbrand"]).life(0, life).start();
        let gr = bf(&g, 0, "Griselbrand");
        assert_eq!(g.has_option(Choice::Ability(gr, AbilitySlot::Activated(0))), ok, "life {life}");
    }
}

#[test]
fn reanimate_a_card_with_x_loses_life_for_x_zero_cr_107_3b() {
    // ruling: e7efe1ca 7e8fdb68
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(1, &["Walking Ballista"]).hand(0, &["Reanimate"]).start();
    let r = g.card(0, "Reanimate", Zone::Hand);
    let wb = g.card(1, "Walking Ballista", Zone::Graveyard);
    let t = g.tgt(wb);
    cast_targets(&mut g, r, &[t]);
    assert_eq!(g.life(0), 20, "mana value 0 in the graveyard");
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "enters with no counters: a 0/0");
}

#[test]
fn delve_pays_only_generic_mana_cr_702_66a() {
    // ruling: c6f70acd
    // Dig Through Time ({6}{U}{U}) with ten cards in the graveyard: castable
    // with two Islands (six exiled), not with one.
    for (lands, ok) in [(&["Island"][..], false), (&["Island", "Island"][..], true)] {
        let g = scen().battlefield(0, lands).graveyard(0, &["Forest"; 10]).hand(0, &["Dig Through Time"]).at(Step::Main1, 0).start();
        let d = g.card(0, "Dig Through Time", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(d)), ok, "{lands:?}: {}", opts(&g));
    }
}

#[test]
fn goblin_engineer_cant_target_the_artifact_it_sacrifices_cr_602_2b() {
    // ruling: c7e12cda
    // Targets come before costs: with no artifact card in the graveyard, the
    // ability can't be activated, though sacrificing Sol Ring would put one
    // there.
    let g = scen().battlefield(0, &["Goblin Engineer", "Sol Ring", "Mountain"]).start();
    let ge = bf(&g, 0, "Goblin Engineer");
    assert!(!(0..2u8).any(|i| g.has_option(Choice::Ability(ge, AbilitySlot::Activated(i)))), "{}", opts(&g));
    let mut g = scen().battlefield(0, &["Goblin Engineer", "Sol Ring", "Mountain"]).graveyard(0, &["Mox Pearl"]).start();
    let ge = bf(&g, 0, "Goblin Engineer");
    let a = (0..2u8).map(|i| Choice::Ability(ge, AbilitySlot::Activated(i))).find(|&a| g.has_option(a)).expect("offered");
    g.choose(a);
    let mox = g.card(0, "Mox Pearl", Zone::Graveyard);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(!g.options_desc().iter().any(|d| d.contains("Sol Ring")), "{}", opts(g));
            Some(g.tgt(mox))
        }
        _ => None,
    });
    assert_eq!(g.obj(mox).zone, Zone::Battlefield);
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Graveyard), 1);
}
