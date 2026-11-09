//! W6's casting from the top of a library (ENGINE.md 99.16): "You may cast
//! artifact spells from the top of your library" (Mm'menon, the Right
//! Hand's; CR 601.3, 601.3e), on a test card that has only it and "you may
//! look at the top card of your library any time".

mod eoe_w6_kit;

use engine::{Choice, Game, Step, Zone};
use eoe_w6_kit::*;

fn top(g: &Game, p: u8) -> engine::CardIdx {
    *g.player(p).library.last().expect("a library")
}

/// The top card, an artifact spell (Nutrient Block), can be cast from the
/// library for its mana cost ({1}: one land tapped, no life paid); then it
/// is on the battlefield and the next card is on top.
#[test]
fn cr_601_3_cast_an_artifact_spell_from_the_top_of_your_library() {
    let mut g = scen()
        .battlefield(0, &["W6 Top Caster", "Island"])
        .library(0, &["Nutrient Block", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let block = top(&g, 0);
    assert_eq!(g.name(block), "Nutrient Block");
    assert!(g.has_option(Choice::Cast(block)), "{:?}", g.options_desc());
    let life = g.player(0).life;
    start_with(&mut g, Choice::Cast(block), |_| None);
    settle(&mut g);
    assert_eq!(g.obj(block).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.player(0).life, life, "its mana cost, not life");
    assert!(g.obj(bf(&g, 0, "Island")).tapped);
    assert_eq!(g.name(top(&g, 0)), "Island");
}

/// Only artifact spells (CR 601.3): a nonartifact creature card or a land on
/// top can't be cast or played this way; nor can the opponent's top card;
/// nor without the permission's source.
#[test]
fn cr_601_3_only_artifact_spells_from_your_own_library() {
    for (mine, top_name, perm) in [(true, "Test Bear", true), (true, "Forest", true), (true, "Nutrient Block", false), (false, "Nutrient Block", true)] {
        let mut s = scen().battlefield(0, &["Island", "Forest", "Forest"]);
        if perm {
            s = s.battlefield(0, &["W6 Top Caster"]);
        }
        let s = if mine { s.library(0, &[top_name, "Island"]) } else { s.library(1, &[top_name, "Forest"]) };
        let g = s.at(Step::Main1, 0).start();
        let t = top(&g, if mine { 0 } else { 1 });
        assert_eq!(g.name(t), top_name);
        assert!(!g.has_option(Choice::Cast(t)) && !g.has_option(Choice::Play(t)), "{top_name} mine {mine} perm {perm}: {:?}", g.options_desc());
    }
}
