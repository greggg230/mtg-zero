//! M8d part 3 (ENGINE.md 39): the cube's rulings on cards with more than
//! one face left after the triage was narrowed: a transforming DFC's
//! characteristics in and out of play, its colour indicator and front-face
//! mana value (CR 712.8, 202.3c); a modal DFC's face-by-face mana value and
//! legality (712.8f, 712.11); "put onto the battlefield" sees the front
//! face (712.14); Adventures' alternative characteristics on the stack and
//! their exile permission (715.3-715.4); preparation cards (722).
//! `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use engine::db::{types, AbilitySlot, Counter};
use engine::decision::{FACE_CAST, OTHER_CAST, OTHER_PLAY};
use engine::mana::{B, G, R, U, W};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

const AJANI: &str = "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger";
const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";
const CECIL: &str = "Cecil, Dark Knight // Cecil, Redeemed Paladin";
const JACE: &str = "Jace, Vryn's Prodigy // Jace, Telepath Unbound";
const WITCH: &str = "Witch Enchanter // Witch-Blessed Meadow";
const SINK: &str = "Sink into Stupor // Soporific Springs";
const SMASHING: &str = "Shatterskull Smashing // Shatterskull, the Hammer Pass";
const NORMAN: &str = "Norman Osborn // Green Goblin";
const BORROWER: &str = "Brazen Borrower // Petty Theft";
const SHIELDBREAKER: &str = "Embereth Shieldbreaker // Battle Display";
const EMERITUS: &str = "Emeritus of Abundance // Regrowth";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The priority option that plays or casts `c`, if any.
fn option_of(g: &Game, c: CardIdx) -> Option<Choice> {
    // (`offered`: a cast or play from the graveyard too, ENGINE.md 100.5.)
    g.offered().into_iter().find(|ch| match *ch {
        Choice::Cast(x) | Choice::Play(x) | Choice::CastFace(x) | Choice::PlayFace(x) | Choice::Faces(x, _) => x == c,
        _ => false,
    })
}

/// Cast `c`'s other face (an Adventure, an MDFC's back).
fn cast_other(g: &mut Game, c: CardIdx) {
    let ch = option_of(g, c).unwrap_or_else(|| panic!("no option for the card: {}", opts(g)));
    assert!(matches!(ch, Choice::CastFace(_) | Choice::Faces(_, _)), "{ch:?}");
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        let i = g.pending_decision().unwrap().options.iter().position(|o| o.choice == Choice::Number(1)).unwrap();
        let c = g.pending_decision().unwrap().options[i].choice;
        g.choose(c);
    }
}

fn mv(g: &Game, c: CardIdx) -> u8 {
    g.def_of(c).mana_value()
}

/// Pay the pending mana costs, with sources named `first` when offered.
fn pay_with(g: &mut Game, first: &str) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let d = g.options_desc();
        let i = d.iter().position(|s| s.contains(first)).unwrap_or(0);
        let c = g.pending_decision().unwrap().options[i].choice;
        g.choose(c);
    }
}

// ---- transforming DFCs (CR 712.8) ---------------------------------------------------------------

#[test]
fn a_transforming_dfc_is_its_front_face_off_the_battlefield_and_keeps_its_mana_value_cr_712_8() {
    // ruling: 22502308 8ca288c2 177cd810 cba16ce1 f8573515 1155292d
    // ruling: ec269542 165eb81a 4c2dcef3 bb602230 4d5c3fbb 8dff2c25
    // In the hand and the graveyard each card is its front face (name,
    // colour, mana value); on the battlefield back face up it has the back
    // face's name and its colour indicator's colours, no mana cost, and the
    // front face's mana value.
    let cases: [(&str, &str, u8, u8, u8); 5] = [
        (AJANI, "Ajani, Nacatl Avenger", 2, 1 << W, 1 << R | 1 << W),
        (TAMIYO, "Tamiyo, Seasoned Scholar", 1, 1 << U, 1 << U | 1 << G),
        (FABLE, "Reflection of Kiki-Jiki", 3, 1 << R, 1 << R),
        (CECIL, "Cecil, Redeemed Paladin", 1, 1 << B, 1 << W),
        (JACE, "Jace, Telepath Unbound", 2, 1 << U, 1 << U),
    ];
    for (card, back, mana_value, front_colors, back_colors) in cases {
        let g = scen().hand(0, &[card]).graveyard(0, &[card]).battlefield(0, &[card]).transformed(0, card).start();
        for zone in [Zone::Hand, Zone::Graveyard] {
            let c = g.card(0, card, zone);
            assert_eq!(g.name(c), card, "{zone:?}");
            assert_eq!(g.colors_of(c), front_colors, "{card} in {zone:?}");
            assert_eq!(mv(&g, c), mana_value, "{card} in {zone:?}");
        }
        let b = bf(&g, 0, back);
        assert_eq!(g.colors_of(b), back_colors, "{back}: its colour indicator");
        assert!(g.def_of(b).cost.is_zero(), "{back}: no mana cost");
        assert_eq!(mv(&g, b), mana_value, "{back}: the front face's mana value");
    }
}

#[test]
fn a_sacrificed_reflection_counts_fables_mana_value_and_is_fable_in_the_graveyard_cr_712_8e() {
    // ruling: 22f30094 8ca288c2
    // Broadside Bombardiers boasts, sacrificing Reflection of Kiki-Jiki:
    // 2 plus mana value 3 (Fable's). In the graveyard the card is Fable, an
    // enchantment (Saga), not a creature.
    let mut g = scen().battlefield(0, &["Broadside Bombardiers", FABLE]).transformed(0, FABLE).at(Step::Main1, 0).start();
    let b = bf(&g, 0, "Broadside Bombardiers");
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(b));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    g.choose(Choice::Ability(b, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ if g.has_option(Choice::Card(r)) => Some(Choice::Card(r)),
        _ => None,
    });
    assert_eq!(g.life(1), 15, "2 plus 3\n{}", log(&g));
    assert_eq!(g.obj(r).zone, Zone::Graveyard);
    assert_eq!(g.name(r), FABLE);
    assert!(g.is_type(r, types::ENCHANTMENT) && !g.is_creature(r));
}

#[test]
fn jaces_back_face_cant_be_cast_and_two_jaces_one_per_face_coexist_cr_712_11a() {
    // ruling: 29c3ea17 004fe294
    // In hand Jace offers only his front face's cast. Two Jaces, one each
    // face up, have different names: the legend rule leaves both.
    let mut g = scen().battlefield(0, &["Island", "Island", JACE, JACE]).transformed(0, JACE).hand(0, &[JACE]).at(Step::Main1, 0).start();
    let h = g.card(0, JACE, Zone::Hand);
    assert_eq!(option_of(&g, h), Some(Choice::Cast(h)), "{}", opts(&g));
    resolve(&mut g);
    let front = bf(&g, 0, JACE);
    let back = bf(&g, 0, "Jace, Telepath Unbound");
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert_eq!(g.obj(front).zone, Zone::Battlefield);
    assert_eq!(g.obj(back).zone, Zone::Battlefield, "{}", log(&g));
}

// ---- modal DFCs (CR 712.8f, 712.11, 712.14) -----------------------------------------------------

#[test]
fn an_mdfcs_mana_value_is_the_up_faces_in_play_and_the_fronts_elsewhere_cr_712_8f() {
    // ruling: 23bfbbce 9671ede4 a3b82184 eea821be 02e23d1b
    // Witch Enchanter in hand: mana value 4; played as Witch-Blessed Meadow:
    // 0. Shatterskull, the Hammer Pass on the battlefield: 0; Shatterskull
    // Smashing in the graveyard: 2. Green Goblin cast from hand is Green
    // Goblin on the stack (mana value 4); bolted, the card in the graveyard
    // is Norman Osborn again with his mana value.
    let mut g = scen()
        .hand(0, &[WITCH, NORMAN])
        .battlefield(0, &["Island", "Swamp", "Mountain", "Mountain", SMASHING])
        .transformed(0, SMASHING)
        .graveyard(0, &[SMASHING])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let w = g.card(0, WITCH, Zone::Hand);
    assert_eq!(mv(&g, w), 4);
    assert!(g.is_creature(w));
    let hp = bf(&g, 0, "Shatterskull, the Hammer Pass");
    assert_eq!(mv(&g, hp), 0);
    let gy = g.card(0, SMASHING, Zone::Graveyard);
    assert_eq!(mv(&g, gy), 2, "the front face's {{X}}{{R}}{{R}}");
    assert!(!g.is_type(gy, types::LAND));
    let ch = option_of(&g, w).unwrap();
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(1));
    }
    if g.kind() == Some(DecisionKind::YesNo) {
        g.choose(Choice::Pass); // (no 3 life paid: it enters tapped)
    }
    assert_eq!(g.obj(w).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.name(w), "Witch-Blessed Meadow");
    assert_eq!(mv(&g, w), 0);
    let n = g.card(0, NORMAN, Zone::Hand);
    let front_mv = mv(&g, n);
    cast_other(&mut g, n);
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    while !at_priority(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.obj(n).zone, Zone::Stack);
    assert_eq!(g.name(n), "Green Goblin");
    assert_eq!(mv(&g, n), 4);
    resolve(&mut g);
    assert_eq!(g.name(n), "Green Goblin");
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(n);
    g.choose(Choice::Cast(bolt));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(n).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.name(n), NORMAN);
    assert_eq!(mv(&g, n), front_mv);
}

#[test]
fn putting_an_mdfc_onto_the_battlefield_sees_only_its_front_face_cr_712_14() {
    // ruling: ab202931 8bd16ad8
    // Animate Dead returns Witch Enchanter and Norman Osborn (creature
    // cards in the graveyard) front face up. (Contrast: Crucible of Worlds
    // lets a player *play* lands from the graveyard, and playing a modal
    // DFC considers the face played (CR 712.11b): Sink into Stupor's
    // Soporific Springs may be played from there.)
    for card in [WITCH, NORMAN] {
        let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).graveyard(0, &[card]).hand(0, &["Animate Dead"]).at(Step::Main1, 0).start();
        let c = g.card(0, card, Zone::Graveyard);
        let a = g.card(0, "Animate Dead", Zone::Hand);
        let t = g.tgt(c);
        g.choose(Choice::Cast(a));
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
        assert_eq!(g.obj(c).zone, Zone::Battlefield, "{card}\n{}", log(&g));
        assert_eq!(g.name(c), card, "front face up");
        assert!(g.is_creature(c));
    }
    let g = scen().battlefield(0, &["Crucible of Worlds"]).graveyard(0, &[SINK, "Forest"]).at(Step::Main1, 0).start();
    let s = g.card(0, SINK, Zone::Graveyard);
    let f = g.card(0, "Forest", Zone::Graveyard);
    assert!(g.has_option(Choice::Play(f)));
    assert_eq!(option_of(&g, s), Some(Choice::PlayFace(s)), "{}", opts(&g));
}

#[test]
fn playing_an_mdfc_checks_only_the_face_being_played_cr_712_11() {
    // ruling: 24e3281b 819a55be
    // With the land drop used, Witch Enchanter and Shatterskull Smashing
    // offer only their spell faces; with no mana and the land drop free,
    // only their land faces. On P1's turn Sink into Stupor offers only the
    // instant (a land can't be played then).
    for card in [WITCH, SMASHING] {
        let g = scen()
            .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "Mountain", "Mountain"])
            .hand(0, &[card])
            .lands_played(0, 1)
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, card, Zone::Hand);
        let ch = option_of(&g, c);
        assert!(matches!(ch, Some(Choice::Cast(_)) | Some(Choice::Faces(_, FACE_CAST))), "{card}: {ch:?}");
        let g = scen().hand(0, &[card]).at(Step::Main1, 0).start();
        let c = g.card(0, card, Zone::Hand);
        assert_eq!(option_of(&g, c), Some(Choice::PlayFace(c)), "{card}: {}", opts(&g));
    }
    let mut g = scen().battlefield(0, &["Island", "Island", "Island"]).hand(0, &[SINK]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    g.pass();
    let s = g.card(0, SINK, Zone::Hand);
    assert_eq!(g.pending_decision().unwrap().player, 0);
    assert_eq!(option_of(&g, s), Some(Choice::Cast(s)), "{}", opts(&g));
}

#[test]
fn spellbinders_exiled_mdfc_may_be_played_as_its_land_by_its_owner_cr_712_11b() {
    // ruling: 43d5100f
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Elite Spellbinder"])
        .hand(1, &[SMASHING])
        .at(Step::Main1, 0)
        .start();
    let e = g.card(0, "Elite Spellbinder", Zone::Hand);
    let s = g.card(1, SMASHING, Zone::Hand);
    g.choose(Choice::Cast(e));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ if g.has_option(Choice::Card(s)) => Some(Choice::Card(s)),
        _ => None,
    });
    assert_eq!(g.obj(s).zone, Zone::Exile, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let ch = option_of(&g, s);
    assert!(
        matches!(ch, Some(Choice::PlayFace(_))) || matches!(ch, Some(Choice::Faces(_, m)) if m & OTHER_PLAY != 0),
        "its land face: {}",
        opts(&g)
    );
    g.choose(ch.unwrap());
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(1));
    }
    assert_eq!(g.obj(s).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.name(s), "Shatterskull, the Hammer Pass");
}

// ---- other faces: Incubator, Touch the Spirit Realm ---------------------------------------------

#[test]
fn sunfall_with_no_creatures_incubates_zero_and_the_token_can_still_transform_cr_701_53a() {
    // ruling: f1bf85a9
    let mut g = scen().battlefield(0, &["Plains"; 7]).hand(0, &["Sunfall"]).at(Step::Main1, 0).start();
    let s = g.card(0, "Sunfall", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve(&mut g);
    let inc = *g.battlefield().iter().filter(|&&c| g.is_token(c)).last().expect("an Incubator token");
    assert_eq!(g.counters_on(inc, Counter::P1P1), 0);
    assert!(!g.is_creature(inc));
    assert!(g.has_option(Choice::Ability(inc, AbilitySlot::Activated(0))), "{{2}}: transform: {}", opts(&g));
}

#[test]
fn a_reflection_exiled_by_touch_the_spirit_realms_channel_returns_as_fable_cr_712_14() {
    // ruling: d07fb867
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Touch the Spirit Realm"])
        .battlefield(1, &[FABLE])
        .transformed(1, FABLE)
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 1, "Reflection of Kiki-Jiki");
    let t = g.card(0, "Touch the Spirit Realm", Zone::Hand);
    let channel = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Ability(x, _) if *x == t)).expect("channel");
    g.choose(channel);
    let tr = g.tgt(r);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tr)).then_some(tr));
    assert_eq!(g.obj(r).zone, Zone::Exile, "{}", log(&g));
    quiet_until(&mut g, |g| g.obj(r).zone == Zone::Battlefield && g.stack().is_empty() && at_priority(g));
    assert_eq!(g.name(r), FABLE, "front face up\n{}", log(&g));
    assert!(!g.is_creature(r));
    assert_eq!(g.obj(r).controller, 1);
}

// ---- Adventures (CR 715) ------------------------------------------------------------------------

#[test]
fn an_adventurer_card_has_its_alternative_characteristics_only_on_the_stack_cr_715_3() {
    // ruling: 266aee1f 442dc321 ee323737 99a330f8
    // Brazen Borrower in hand is a blue creature card of mana value 3;
    // Petty Theft on the stack is an instant of mana value 2; exiled on
    // its adventure it's the creature card again. Same for Embereth
    // Shieldbreaker (mana value 2) / Battle Display (a sorcery, 1).
    for (card, adv, card_mv, adv_mv, adv_type) in [
        (BORROWER, "Petty Theft", 3u8, 2u8, types::INSTANT),
        (SHIELDBREAKER, "Battle Display", 2, 1, types::SORCERY),
    ] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Mountain"])
            .hand(0, &[card])
            .graveyard(0, &[card])
            .battlefield(1, &["Sol Ring"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, card, Zone::Hand);
        let gy = g.card(0, card, Zone::Graveyard);
        for x in [c, gy] {
            assert!(g.is_creature(x), "{card}");
            assert_eq!(mv(&g, x), card_mv);
        }
        let ring = bf(&g, 1, "Sol Ring");
        cast_other(&mut g, c);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(ring));
        g.drive_until(|g| g.obj(c).zone == Zone::Stack && at_priority(g));
        assert_eq!(g.name(c), adv);
        assert_eq!(mv(&g, c), adv_mv, "{adv}");
        assert!(g.is_type(c, adv_type) && !g.is_creature(c), "{adv}");
        resolve(&mut g);
        assert_eq!(g.obj(c).zone, Zone::Exile, "{}", log(&g));
        assert_eq!(g.name(c), card);
        assert!(g.is_creature(c));
        assert_eq!(mv(&g, c), card_mv);
    }
}

#[test]
fn an_adventure_needs_its_own_legal_target_to_be_cast_cr_715_3a() {
    // ruling: 4b8b633b abf4aaa4
    // No nonland permanent of an opponent's: Petty Theft can't be cast,
    // Brazen Borrower can. No artifact: Battle Display can't, Embereth
    // Shieldbreaker can.
    for card in [BORROWER, SHIELDBREAKER] {
        let g = scen().battlefield(0, &["Island", "Island", "Island", "Mountain"]).hand(0, &[card]).at(Step::Main1, 0).start();
        let c = g.card(0, card, Zone::Hand);
        assert_eq!(option_of(&g, c), Some(Choice::Cast(c)), "{card}: {}", opts(&g));
    }
}

#[test]
fn a_copy_of_an_adventure_ceases_to_exist_and_only_the_card_may_be_cast_later_cr_715_4() {
    // ruling: 9a1260e0 5d8546ba
    // Lutri, cast with Petty Theft on the stack, copies it; the copy
    // resolves (exiled, then gone, CR 704.5e); the card resolves and is
    // exiled on its adventure: only the card may be cast from exile.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Mountain"])
        .hand(0, &[BORROWER, "Lutri, the Spellchaser"])
        .battlefield(1, &["Sol Ring", "Mox Pearl"])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, BORROWER, Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    let mox = bf(&g, 1, "Mox Pearl");
    cast_other(&mut g, b);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ring));
    pay_with(&mut g, "Island");
    assert!(at_priority(&g));
    let lutri = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(lutri));
    let tm = g.tgt(mox);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::NewTarget) | Some(DecisionKind::Target) if g.has_option(tm) => Some(tm),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.obj(mox).zone, Zone::Hand, "the copy resolved\n{}", log(&g));
    assert_eq!(g.obj(b).zone, Zone::Exile);
    let copies: Vec<_> = g.exile_zone().iter().copied().filter(|&c| g.is_copy_obj(c)).collect();
    assert!(copies.is_empty(), "the copy ceased to exist");
    assert_eq!(g.exile_zone().len(), 1);
}

#[test]
fn an_adventurer_card_exiled_another_way_gives_no_permission_cr_715_4() {
    // ruling: 571ea1ea
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", SHIELDBREAKER])
        .battlefield(1, &["Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 1)
        .start();
    let s = bf(&g, 0, SHIELDBREAKER);
    let stp = g.card(1, "Swords to Plowshares", Zone::Hand);
    let t = g.tgt(s);
    g.choose(Choice::Cast(stp));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(s).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(option_of(&g, s), None, "{}", opts(&g));
}

#[test]
fn bilbo_casts_an_adventure_from_the_graveyard_resolved_it_may_be_cast_later_countered_not_cr_715_4() {
    // ruling: d26ce62d
    for countered in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Bilbo, Thief in the Night", "Island", "Island", "Island", "Island"])
            .graveyard(0, &[BORROWER])
            .battlefield(1, &["Sol Ring", "Island", "Island"])
            .hand(1, if countered { &["Counterspell"][..] } else { &[][..] })
            .at(Step::DeclareAttackers, 0)
            .start();
        let bilbo = bf(&g, 0, "Bilbo, Thief in the Night");
        let b = g.card(0, BORROWER, Zone::Graveyard);
        let ring = bf(&g, 1, "Sol Ring");
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
        g.choose(Choice::Card(bilbo));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(1));
        }
        g.choose(Choice::Pass);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
        assert!(g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Card(x) | Choice::Faces(x, _) | Choice::CastFace(x) if x == b)), "the Adventure: {}", opts(&g));
        let ch = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(*c, Choice::Card(x) | Choice::Faces(x, _) | Choice::CastFace(x) if x == b)).unwrap();
        g.choose(ch);
        if g.kind() == Some(DecisionKind::ChooseFace) {
            g.choose(Choice::Number(1));
        }
        let tr = g.tgt(ring);
        let cs = if countered { Some(g.card(1, "Counterspell", Zone::Hand)) } else { None };
        quiet_until_with(&mut g, |g| g.obj(b).zone != Zone::Stack && g.obj(b).zone != Zone::Graveyard && at_priority(g), |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(tr) => Some(tr),
            Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
            Some(DecisionKind::Priority) if g.pending_decision().unwrap().player == 1 && cs.is_some_and(|c| g.has_option(Choice::Cast(c))) => Some(Choice::Cast(cs.unwrap())),
            // Resolved: Bilbo's "exile it instead" and the Adventure's exile
            // both apply, and the controller uses the Adventure's (CR 616.1,
            // 715.4; ENGINE.md 100.3).
            Some(DecisionKind::ChooseReplacement) => Some(Choice::Number(4)),
            _ => None,
        });
        assert_eq!(g.obj(b).zone, Zone::Exile, "{}", log(&g));
        assert_eq!(g.obj(ring).zone, if countered { Zone::Battlefield } else { Zone::Hand });
        g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
        assert_eq!(option_of(&g, b).is_some(), !countered, "countered {countered}: {}\n{}", opts(&g), log(&g));
    }
}

#[test]
fn an_adventure_cast_with_robber_of_the_rich_lets_robbers_controller_cast_the_creature_cr_715_4() {
    // ruling: dca8fc68
    let mut g = scen()
        .battlefield(0, &["Robber of the Rich", "Island", "Island", "Island", "Island", "Island"])
        .library(1, &[BORROWER, "Forest", "Forest"])
        .battlefield(1, &["Sol Ring"])
        .hand(1, &["Island", "Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let robber = bf(&g, 0, "Robber of the Rich");
    let ring = bf(&g, 1, "Sol Ring");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(robber));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    resolve(&mut g);
    let b = g.card(1, BORROWER, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    cast_other(&mut g, b);
    let tr = g.tgt(ring);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tr)).then_some(tr));
    assert_eq!(g.obj(ring).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.obj(b).zone, Zone::Exile);
    assert_eq!(option_of(&g, b), Some(Choice::Cast(b)), "P0 may cast Brazen Borrower: {}", opts(&g));
    g.choose(Choice::Cast(b));
    resolve(&mut g);
    assert_eq!(g.obj(b).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(b).controller, 0);
    assert_eq!(g.obj(b).owner, 1);
}

// ---- preparation cards (CR 722) -----------------------------------------------------------------

#[test]
fn a_preparation_card_is_a_creature_card_everywhere_and_casts_only_as_one_cr_722_1() {
    // ruling: 1a88f316 81739185
    // Emeritus of Abundance in hand offers only its creature cast; in the
    // graveyard it's a creature card: Snapcaster Mage can't target it.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Forest", "Forest", "Forest"])
        .hand(0, &[EMERITUS, "Snapcaster Mage"])
        .graveyard(0, &[EMERITUS, "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, EMERITUS, Zone::Hand);
    assert_eq!(option_of(&g, h), Some(Choice::Cast(h)), "{}", opts(&g));
    let gy = g.card(0, EMERITUS, Zone::Graveyard);
    assert!(g.is_creature(gy) && !g.is_type(gy, types::SORCERY));
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    let sc = g.card(0, "Snapcaster Mage", Zone::Hand);
    g.choose(Choice::Cast(sc));
    let tb = g.tgt(bolt);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(gy)), "a creature card: {}", opts(g));
            return g.has_option(tb).then_some(tb);
        }
        None
    });
    // (A lone legal target is chosen without a decision.)
    let l = log(&g);
    assert!(l.contains("target Lightning Bolt") && !l.contains("target Emeritus"), "{l}");
}

const _: (u8, u8) = (OTHER_CAST, FACE_CAST);
