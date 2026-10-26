//! M8d part 2 (ENGINE.md 38, `docs/interactions.md`): cards with more than
//! one face (split cards, modal double-faced cards, Adventures,
//! transforming cards) against the subsystems written in other milestones:
//! permissions to cast from a graveyard (601.2a-b), copies (707.8), zone
//! changes and the new object (400.7, 712.14), mana value (709.3, 712.8).

mod common;
mod rulings_util;

use engine::db::types;
use engine::{CardIdx, Choice, DecisionKind, Game, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The priority option that casts `c` (any form).
fn option_of(g: &Game, c: CardIdx) -> Option<Choice> {
    g.pending_decision()?.options.iter().map(|o| o.choice).find(|ch| match *ch {
        Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) => x == c,
        _ => false,
    })
}

#[test]
fn a_split_card_with_two_graveyard_permissions_casts_its_left_half_cr_709_3_601_2b() {
    // Yawgmoth's Will (cast spells from your graveyard this turn) and
    // Snapcaster Mage's flashback both let Life // Death be cast from P0's
    // graveyard: casting Life (the left half) asks which permission, then
    // casts it. (Found by FUZZ_POOL=interactions, seed 35744: the cast
    // checked the routes of the whole split card, found none and
    // panicked.)
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Island", "Island", "Forest"])
        .graveyard(0, &["Life // Death"])
        .hand(0, &["Yawgmoth's Will", "Snapcaster Mage"])
        .start();
    let ld = g.card(0, "Life // Death", Zone::Graveyard);
    let will = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(will));
    resolve(&mut g);
    let snap = g.card(0, "Snapcaster Mage", Zone::Hand);
    let t = engine::Choice::Target(engine::Target::Obj(g.obj_ref(ld)));
    g.choose(Choice::Cast(snap));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(snap).zone, Zone::Battlefield, "{}", log(&g));
    let c = option_of(&g, ld).unwrap_or_else(|| panic!("Life // Death castable: {}", opts(&g)));
    g.choose(c);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(0));
    }
    let routes = std::cell::Cell::new(0usize);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseRoute) {
            routes.set(g.pending_decision().unwrap().options.len());
        }
        None
    });
    assert_eq!(routes.get(), 2, "flashback or Yawgmoth's Will\n{}", log(&g));
    let forest = bf(&g, 0, "Forest");
    assert!(g.is_creature(forest) && g.is_type(forest, types::LAND), "Life resolved: lands are 1/1 creatures\n{}", log(&g));
    assert_eq!(g.obj(ld).zone, Zone::Exile, "exiled after (flashback, or the Will's replacement)");
}
