//! W5 of the Edge of Eternities port (ENGINE.md 99.15): Tannuk, Steadfast
//! Second, "Other creatures you control have haste. / Artifact cards and red
//! creature cards in your hand have warp {2}{R}." An alternative cost another
//! permanent's static ability gives cards in hand (CR 118.9, 604.1, 611.3a,
//! 702.185a). Test names cite the CR.

mod eoe_w5_kit;

use engine::db::{Counter, Kw};
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_w5_kit::*;

fn castable(g: &Game, p: engine::PlayerId, name: &str) -> bool {
    let c = g.card(p, name, Zone::Hand);
    g.pending_decision().unwrap().options.iter().any(|o| match o.choice {
        Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) => x == c,
        _ => false,
    })
}

/// "Artifact cards and red creature cards in your hand have warp {2}{R}"
/// (CR 611.3a, 702.185a): with three Mountains, a red creature card costing
/// {3}{R} and an artifact card costing {4} can be cast (for {2}{R}); a green
/// creature card and a red instant card can't (they don't have warp). Without
/// Tannuk none can.
#[test]
fn cr_611_3a_tannuk_gives_artifact_and_red_creature_cards_in_your_hand_warp() {
    let hand = ["W5 Ogre", "W5 Trinket", "W5 Elk", "W5 Blast"];
    let g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
        .hand(0, &hand)
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, "W5 Ogre"), "a red creature card: {:?}", g.options_desc());
    assert!(castable(&g, 0, "W5 Trinket"), "an artifact card: {:?}", g.options_desc());
    assert!(!castable(&g, 0, "W5 Elk"), "a green creature card has no warp");
    assert!(!castable(&g, 0, "W5 Blast"), "a red instant card has no warp");
    let g = scen().battlefield(0, &["Mountain", "Mountain", "Mountain"]).hand(0, &hand).at(Step::Main1, 0).start();
    for n in hand {
        assert!(!castable(&g, 0, n), "{n} without Tannuk");
    }
}

/// "In your hand": the cards in the hand of Tannuk's controller only (CR
/// 109.5). The opponent's red creature card has no warp.
#[test]
fn cr_109_5_tannuk_gives_warp_to_its_controllers_hand_only() {
    let g = scen()
        .battlefield(0, &[TANNUK])
        .battlefield(1, &["Mountain", "Mountain", "Mountain"])
        .hand(1, &["W5 Ogre"])
        .at(Step::Main1, 1)
        .start();
    assert!(!castable(&g, 1, "W5 Ogre"), "{:?}", g.options_desc());
}

/// Cast for the warp cost Tannuk gives (CR 601.2b, 118.9: `ChooseAltCost`,
/// the granted cost named by its permanent): {2}{R} is paid, void holds (a
/// spell was warped, CR 702.185c), the other creature has haste from Tannuk
/// (Tannuk itself doesn't); at the beginning of the next end step the
/// permanent is exiled, and its owner may cast it from exile on a later turn
/// (CR 702.185a), for its mana cost (it has no warp there).
#[test]
fn cr_702_185a_a_card_warped_with_tannuk_is_exiled_at_the_end_step_and_cast_later() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["W5 Ogre"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let ogre = begin_cast(&mut g, 0, "W5 Ogre");
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost));
    assert_eq!(options(&g), vec![Choice::Number(0), Choice::Card(tannuk)], "its mana cost or Tannuk's warp");
    assert!(!g.void_active());
    g.choose(Choice::Card(tannuk));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 1, "{{2}}{{R}} paid\n{}", log(&g));
    assert!(g.void_active(), "a spell was warped this turn");
    settle(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield);
    assert!(g.has_kw(ogre, Kw::Haste), "other creatures you control have haste");
    assert!(!g.has_kw(tannuk, Kw::Haste), "not Tannuk itself");
    to_next_turn(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Exile, "exiled at the beginning of the end step\n{}", log(&g));
    assert_eq!(g.obj(tannuk).zone, Zone::Battlefield);
    next_main(&mut g, 0);
    assert!(g.has_option(Choice::Cast(ogre)), "castable from exile on a later turn: {:?}", g.options_desc());
    g.choose(Choice::Cast(ogre));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "for its mana cost only: no warp in exile\n{}", log(&g));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 0, "{{3}}{{R}}");
    settle(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield);
    to_next_turn(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield, "cast for its mana cost: it stays");
}

/// Warp's exile is created as the permanent enters, by the spell's warp
/// ability (CR 608.3g); the spell keeps a granted ability only while the
/// granting effect would still apply to it (CR 400.7g, 611.3b). Tannuk dies
/// with the warped spell on the stack: the permanent stays at the end step.
/// It was still cast for its warp cost (void; Full Bore's trample and haste).
#[test]
fn cr_608_3g_400_7g_without_tannuk_as_the_spell_resolves_the_permanent_is_not_exiled() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["W5 Ogre", "W5 Bolt", "Full Bore"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let ogre = cast_granted(&mut g, 0, "W5 Ogre", tannuk);
    g.drive_until(at_priority);
    assert_eq!(g.pending_decision().unwrap().player, 0, "the caster gets priority");
    let t = g.tgt(tannuk);
    cast_plain(&mut g, 0, "W5 Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(tannuk).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield);
    assert!(g.void_active());
    assert!(!g.has_kw(ogre, Kw::Haste), "no Tannuk, no haste");
    let t = g.tgt(ogre);
    cast_plain(&mut g, 0, "Full Bore", &[t]);
    settle(&mut g);
    assert!(g.has_kw(ogre, Kw::Trample) && g.has_kw(ogre, Kw::Haste), "it was cast for its warp cost\n{}", log(&g));
    to_next_turn(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield, "the spell had no warp as it resolved\n{}", log(&g));
}

/// The grant lasts while Tannuk's ability applies (CR 611.3b): with three
/// Mountains a red creature card costing {3}{R} is castable (for {2}{R});
/// once the opponent's Bolt kills Tannuk, it isn't.
#[test]
fn cr_611_3b_the_warp_ends_when_tannuk_leaves_the_battlefield() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["W5 Ogre"])
        .hand(1, &["W5 Bolt"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    assert!(castable(&g, 0, "W5 Ogre"), "{:?}", g.options_desc());
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    let t = g.tgt(tannuk);
    cast_plain(&mut g, 1, "W5 Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(tannuk).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!((g.turn().step, g.pending_decision().unwrap().player), (Step::Main1, 0), "{}", log(&g));
    assert_eq!(untapped(&g, 0), 3);
    assert!(!castable(&g, 0, "W5 Ogre"), "{:?}", g.options_desc());
}

/// One alternative cost at most (CR 118.9a, 601.2b): Memorial Team Leader
/// ({3}{R}, warp {1}{R}) offers its mana cost, its own warp, Tannuk's warp,
/// and (Weftwalking) "without paying its mana cost"; the one chosen is paid.
/// Cast for Tannuk's {2}{R}, it is exiled at the end step by that warp.
#[test]
fn cr_118_9a_its_own_warp_tannuks_warp_or_its_mana_cost_one_of_them() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Weftwalking", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Memorial Team Leader"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let leader = begin_cast(&mut g, 0, "Memorial Team Leader");
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost));
    let o = options(&g);
    assert_eq!(o.len(), 4, "{o:?}");
    for c in [Choice::Number(0), Choice::Number(1), Choice::Card(tannuk), Choice::Number(31)] {
        assert!(o.contains(&c), "{c:?} in {o:?}");
    }
    g.choose(Choice::Card(tannuk));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 1, "{{2}}{{R}}, not its own {{1}}{{R}}\n{}", log(&g));
    settle(&mut g);
    assert_eq!(g.obj(leader).zone, Zone::Battlefield);
    to_next_turn(&mut g);
    assert_eq!(g.obj(leader).zone, Zone::Exile, "{}", log(&g));

    // Its own warp: {1}{R}.
    let mut g = scen().battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"]).hand(0, &["Memorial Team Leader"]).at(Step::Main1, 0).start();
    begin_cast(&mut g, 0, "Memorial Team Leader");
    assert_eq!(options(&g).len(), 2, "its warp and Tannuk's (not its mana cost): {:?}", g.options_desc());
    g.choose(Choice::Number(1));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 1, "{{1}}{{R}}");
}

/// An alternative cost gets the spell's cost increases and reductions (CR
/// 118.9d, 601.2f). "Noncreature spells cost {1} more": an artifact card's
/// warp {2}{R} costs {3}{R}, which three Mountains can't pay (its mana cost
/// {4} costs {5}). "The second spell you cast each turn costs {2} less"
/// (Uthros Psionicist): a second spell cast for Tannuk's warp costs {R}.
#[test]
fn cr_118_9d_increases_and_reductions_apply_to_tannuks_warp_cost() {
    let g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W5 Thalia"])
        .hand(0, &["W5 Trinket"])
        .at(Step::Main1, 0)
        .start();
    assert!(!castable(&g, 0, "W5 Trinket"), "{{3}}{{R}} with three Mountains: {:?}", g.options_desc());
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W5 Thalia"])
        .hand(0, &["W5 Trinket"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let trinket = begin_cast(&mut g, 0, "W5 Trinket");
    assert_eq!(options(&g), vec![Choice::Card(tannuk)], "{{5}} can't be paid; {{3}}{{R}} can");
    g.choose(Choice::Card(tannuk));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 0);
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Battlefield);

    let mut g = scen()
        .battlefield(0, &[TANNUK, "Uthros Psionicist", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W5 Elk"])
        .hand(0, &["W5 Bolt", "W5 Ogre"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let elk = g.tgt(bf(&g, 1, "W5 Elk"));
    cast_plain(&mut g, 0, "W5 Bolt", &[elk]);
    settle(&mut g);
    assert_eq!(untapped(&g, 0), 2);
    let ogre = cast_granted(&mut g, 0, "W5 Ogre", tannuk);
    assert_eq!(untapped(&g, 0), 1, "the second spell: {{2}}{{R}} less {{2}}\n{}", log(&g));
    settle(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield);
}

/// Cast for an alternative cost without {X}, a spell with {X} in its mana
/// cost has X = 0 (CR 107.3b): W5 Hydra ({X}{R}, "enters with X +1/+1
/// counters") cast for Tannuk's warp is offered X = 0 only and dies as a
/// 0/0. For its mana cost, X is chosen.
#[test]
fn cr_107_3b_x_is_zero_for_tannuks_warp() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["W5 Hydra"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let hydra = begin_cast(&mut g, 0, "W5 Hydra");
    g.choose(Choice::Card(tannuk));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || g.kind() == Some(DecisionKind::PayMana));
    assert_eq!(options(&g), vec![Choice::Number(0)], "X is 0: {:?}", g.options_desc());
    g.choose(Choice::Number(0));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 2, "{{2}}{{R}}");
    settle(&mut g);
    assert_eq!(g.obj(hydra).zone, Zone::Graveyard, "a 0/0\n{}", log(&g));

    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["W5 Hydra"])
        .at(Step::Main1, 0)
        .start();
    let hydra = begin_cast(&mut g, 0, "W5 Hydra");
    g.choose(Choice::Number(0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(4)), "{:?}", g.options_desc());
    g.choose(Choice::Number(4));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(hydra, Counter::P1P1), 4);
}

/// Whether a face can be cast for Tannuk's warp is decided by the face's
/// characteristics (CR 601.3e): an MDFC whose back face is a red creature
/// can be cast as that face for {2}{R} (and is exiled at the end step); an
/// MDFC whose back face is a green creature is cast as that face with no
/// alternative cost to choose; an adventurer card's Adventure, a red
/// sorcery, isn't a creature card: no warp for it, while the creature is
/// offered Tannuk's warp.
#[test]
fn cr_601_3e_a_face_has_warp_by_its_own_characteristics() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
        .hand(0, &["W5 Seer // W5 Raider"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let c = g.card(0, "W5 Seer // W5 Raider", Zone::Hand);
    assert!(g.has_option(Choice::CastFace(c)), "the red back face only: {:?}", g.options_desc());
    g.choose(Choice::CastFace(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert_eq!(options(&g), vec![Choice::Card(tannuk)], "{:?}\n{}", g.options_desc(), log(&g));
    g.choose(Choice::Card(tannuk));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.name(c), "W5 Raider");
    to_next_turn(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Exile, "{}", log(&g));

    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Forest"])
        .hand(0, &["W5 Brute // W5 Grower"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "W5 Brute // W5 Grower", Zone::Hand);
    assert!(g.has_option(Choice::Faces(c, 0)) || g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Faces(x, _) if x == c)), "both faces: {:?}", g.options_desc());
    let both = g.pending_decision().unwrap().options.iter().find(|o| matches!(o.choice, Choice::Faces(x, _) if x == c)).unwrap().choice;
    g.choose(both);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseFace));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() != Some(DecisionKind::ChooseFace));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "the green back face: no warp to choose\n{}", log(&g));

    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W5 Trinket"])
        .hand(0, &["W5 Shieldbreaker // W5 Display"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let c = g.card(0, "W5 Shieldbreaker // W5 Display", Zone::Hand);
    let both = g.pending_decision().unwrap().options.iter().find(|o| matches!(o.choice, Choice::Faces(x, _) if x == c)).map(|o| o.choice);
    let both = both.unwrap_or_else(|| panic!("the creature and its Adventure: {:?}", g.options_desc()));
    let mut h = g.clone();
    g.choose(both);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseFace));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() != Some(DecisionKind::ChooseFace));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "the Adventure: no warp to choose\n{}", log(&g));
    h.choose(both);
    h.drive_until(|g| g.kind() == Some(DecisionKind::ChooseFace));
    h.choose(Choice::Number(0));
    h.drive_until(|g| g.kind() != Some(DecisionKind::ChooseFace));
    assert_eq!(options(&h), vec![Choice::Number(0), Choice::Card(tannuk)], "the creature: {:?}", h.options_desc());
}

/// A card warped with Tannuk's warp, in exile, is a warped card (CR 702.185b:
/// exiled by the delayed triggered ability a warp ability created): Close
/// Encounter may choose it. It no longer has warp there (Tannuk's ability
/// gives it to cards in hand, CR 611.3a): Blade of the Swarm's "target exiled
/// card with warp" may target Memorial Team Leader (its own warp) but not it.
#[test]
fn cr_702_185b_a_card_warped_with_tannuk_is_warped_in_exile_without_warp() {
    let mut g = scen()
        .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain", "Mountain", "Swamp", "Swamp", "Forest"])
        .battlefield(1, &["W5 Elk"])
        .hand(0, &["Memorial Team Leader", "W5 Ogre", "Close Encounter", "Blade of the Swarm"])
        .at(Step::Main1, 0)
        .start();
    let tannuk = bf(&g, 0, TANNUK);
    let leader = begin_cast(&mut g, 0, "Memorial Team Leader");
    g.choose(Choice::Number(1)); // its own warp
    pay(&mut g);
    settle(&mut g);
    let ogre = cast_granted(&mut g, 0, "W5 Ogre", tannuk);
    settle(&mut g);
    to_next_turn(&mut g);
    assert_eq!((g.obj(ogre).zone, g.obj(leader).zone), (Zone::Exile, Zone::Exile), "{}", log(&g));
    next_main(&mut g, 0);
    // Close Encounter: the Ogre is a warped creature card in exile.
    let elk = bf(&g, 1, "W5 Elk");
    let ce = g.card(0, "Close Encounter", Zone::Hand);
    assert!(g.has_option(Choice::Cast(ce)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(ce));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled) || g.kind() == Some(DecisionKind::Target));
    if g.kind() == Some(DecisionKind::Target) {
        let t = g.tgt(elk);
        g.choose(t);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled));
    }
    assert!(g.has_option(Choice::Card(ogre)), "the warped Ogre: {:?}\n{}", g.options_desc(), log(&g));
    g.choose(Choice::Card(ogre));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana) || at_priority(g));
    if g.kind() == Some(DecisionKind::Target) {
        let t = g.tgt(elk);
        g.choose(t);
    }
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(elk).zone, Zone::Graveyard, "3 damage from the Ogre's power\n{}", log(&g));
    // Blade of the Swarm: "target exiled card with warp" is the Leader only.
    cast_plain(&mut g, 0, "Blade of the Swarm", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert_eq!(options(&g), vec![Choice::Number(0), Choice::Number(1)], "{:?}", g.options_desc());
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(options(&g), vec![g.tgt(leader)], "not the Ogre: {:?}", g.options_desc());
    let t = g.tgt(leader);
    g.choose(t);
    settle(&mut g);
    assert_eq!((g.obj(leader).zone, g.obj(ogre).zone), (Zone::Library, Zone::Exile), "{}", log(&g));
}

/// Cards in hand are hidden (CR 401.2-style hidden information; ENGINE.md
/// 10): whether P0's hand card can be cast for Tannuk's warp is in P0's own
/// options only. P1's encoding is the same with a red creature card (warp)
/// or a green one (none) in P0's hand, the other in P0's library.
#[test]
fn tannuks_warp_tells_the_opponent_nothing_of_the_hand() {
    let world = |card: &str, other: &str| {
        scen()
            .battlefield(0, &[TANNUK, "Mountain", "Mountain", "Mountain"])
            .hand(0, &[card])
            .library(0, &[other])
            .at(Step::Main1, 0)
            .start()
    };
    let (a, b) = (world("W5 Ogre", "W5 Elk"), world("W5 Elk", "W5 Ogre"));
    assert!(castable(&a, 0, "W5 Ogre") && !castable(&b, 0, "W5 Elk"));
    let features = |g: &Game, v| {
        let mut x = vec![0.0; engine::az::encode::NUM_FEATURES];
        g.encode_features(v, &mut x);
        x
    };
    assert!(features(&a, 1) == features(&b, 1), "P1's encoding reads P0's hand");
}
