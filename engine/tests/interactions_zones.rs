//! M8d part 2 (ENGINE.md 38, `docs/interactions.md`): zone changes and the
//! new object (CR 400.7) against face-down permanents (708, M7d),
//! transformed double-faced cards (712, M7b part 2), Auras that returned a
//! creature (Animate Dead), linked "until" exile (610.3, 607), control
//! changes (Oko's exchange, Fractured Identity: 302.6, 306.5b, 110.2), and
//! combat with protection (702.16, 702.19).

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;

const OKO: &str = "Oko, Thief of Crowns";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// P0's Abhorrent Oculus manifests `top` at P1's upkeep; the game goes on to
/// P0's next main phase. Returns the face-down card.
fn manifested_then_main(top: &str, lands: &[&str], hand: &[&str]) -> (Game, CardIdx) {
    let mut g = scen()
        .library(0, &["Island", top, "Island", "Island", "Island"])
        .battlefield(0, &["Abhorrent Oculus"])
        .battlefield(0, lands)
        .hand(0, hand)
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let c = g.card(0, top, Zone::Library);
    g.choose(Choice::Card(c));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.is_face_down(c));
    (g, c)
}

/// P0 casts Flickerwisp ("When this creature enters, exile another target
/// permanent. Return that card to the battlefield under its owner's control
/// at the beginning of the next end step") at `target`.
fn flicker(g: &mut Game, target: CardIdx) -> CardIdx {
    let fw = g.card(0, "Flickerwisp", Zone::Hand);
    let t = g.tgt(target);
    cast_targets(g, fw, &[t]);
    fw
}

fn to_end_step(g: &mut Game) {
    let t = g.turn().number;
    quiet_until(g, |g| (g.turn().step == Step::End && g.stack().is_empty() && at_priority(g)) || g.turn().number > t);
}

#[test]
fn a_flickered_face_down_creature_returns_face_up_cr_708_9_400_7() {
    // A manifested Test Giant exiled by Flickerwisp returns at the end step
    // as the Test Giant, face up (a new object).
    let (mut g, fd) = manifested_then_main("Test Giant", &["Plains", "Plains", "Plains"], &["Flickerwisp"]);
    flicker(&mut g, fd);
    assert_eq!(g.obj(fd).zone, Zone::Exile);
    assert!(g.knows(1, fd), "revealed as it left the battlefield (CR 708.9)");
    to_end_step(&mut g);
    assert_eq!(g.obj(fd).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.is_face_down(fd));
    assert_eq!(g.name(fd), "Test Giant");
    assert_eq!(g.pt(fd), (5, 5));
}

#[test]
fn a_flickered_face_down_instant_cant_return_and_stays_exiled_cr_708_9_400_4a() {
    // A manifested Lightning Bolt: exiled, it can't be put onto the
    // battlefield (an instant card), so it stays in exile.
    let (mut g, fd) = manifested_then_main("Lightning Bolt", &["Plains", "Plains", "Plains"], &["Flickerwisp"]);
    flicker(&mut g, fd);
    to_end_step(&mut g);
    assert_eq!(g.obj(fd).zone, Zone::Exile, "{}", log(&g));
    assert!(!g.is_face_down(fd));
}

#[test]
fn a_flickered_transformed_card_returns_front_face_up_cr_712_14_400_7() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Ulvenwald Oddity // Ulvenwald Behemoth"])
        .transformed(0, "Ulvenwald Oddity // Ulvenwald Behemoth")
        .hand(0, &["Flickerwisp"])
        .start();
    let o = bf(&g, 0, "Ulvenwald Behemoth");
    flicker(&mut g, o);
    to_end_step(&mut g);
    assert_eq!(g.obj(o).zone, Zone::Battlefield, "{}", log(&g));
    assert_ne!(g.name(o), "Ulvenwald Behemoth");
    assert_eq!(g.pt(o), (4, 4), "Ulvenwald Oddity, its front face");
}

#[test]
fn a_reanimated_creature_flickered_returns_under_its_owner_and_the_aura_is_gone_cr_400_7_704_5m() {
    // P0's Animate Dead returns P1's Test Giant under P0's control; P0's
    // Flickerwisp exiles the Giant: Animate Dead goes to the graveyard
    // (attached to nothing, CR 704.5m) and the Giant returns at the end
    // step under its owner's control (a new object, 400.7), not P0's.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Plains", "Plains", "Plains"])
        .graveyard(1, &["Test Giant"])
        .hand(0, &["Animate Dead", "Flickerwisp"])
        .start();
    let giant = g.card(1, "Test Giant", Zone::Graveyard);
    let ad = g.card(0, "Animate Dead", Zone::Hand);
    let t = Choice::Target(Target::Obj(g.obj_ref(giant)));
    cast_targets(&mut g, ad, &[t]);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(giant).controller, 0);
    assert_eq!(g.pt(giant), (4, 5), "Animate Dead's -1/-0");
    flicker(&mut g, giant);
    assert_eq!(g.obj(ad).zone, Zone::Graveyard, "{}", log(&g));
    to_end_step(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(giant).controller, 1);
    assert_eq!(g.pt(giant), (5, 5));
}

#[test]
fn leyline_binding_flickered_returns_its_card_and_exiles_anew_cr_610_3_400_7() {
    // Leyline Binding exiled P1's Bear "until this leaves the battlefield";
    // Flickerwisp exiles the Binding: the Bear returns at once (610.3a). At
    // the end step the Binding returns, a new object whose enter ability
    // exiles a new target (the Bear again).
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Swamp", "Mountain", "Forest", "Plains", "Plains"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Leyline Binding", "Flickerwisp"])
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let lb = g.card(0, "Leyline Binding", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, lb, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    flicker(&mut g, lb);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "back when the Binding left\n{}", log(&g));
    let bear2 = bf(&g, 1, "Test Bear");
    let tb = g.tgt(bear2);
    let t = g.turn().number;
    quiet_until_with(&mut g, |g| g.turn().number > t, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tb)).then_some(tb));
    assert_eq!(g.obj(lb).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(bear2).zone, Zone::Exile, "the new Binding exiled it again\n{}", log(&g));
}

#[test]
fn an_exchanged_creature_is_summoning_sick_and_its_controller_gains_the_life_cr_302_6_110_2() {
    // Oko's -5 exchanges P0's Food for P1's Test Bear: the Bear can't attack
    // this turn (P0 hasn't controlled it continuously since the turn
    // began); Swords to Plowshares on it: "its controller gains life equal
    // to its power": P0.
    let mut g = scen()
        .battlefield(0, &[OKO, "Plains"])
        .counters(0, OKO, Counter::Loyalty, 7)
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Swords to Plowshares"])
        .start();
    let oko = bf(&g, 0, OKO);
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(0)));
    resolve(&mut g);
    let food = bf(&g, 0, "Food");
    // Next turn: the exchange.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(2)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(food)) => Some(g.tgt(food)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).controller, 0, "{}", log(&g));
    assert_eq!(g.obj(food).controller, 1);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step > Step::DeclareAttackers);
    assert!(!g.has_option(Choice::Card(bear)), "summoning sick: {}", opts(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    let sw = g.card(0, "Swords to Plowshares", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, sw, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!((g.life(0), g.life(1)), (22, 20), "{}", log(&g));
}

#[test]
fn fractured_identity_on_a_planeswalker_gives_a_token_with_its_printed_loyalty_cr_707_2_306_5b() {
    // "Exile target nonland permanent. Each player other than its
    // controller creates a token that's a copy of it." P1's Teferi, Time
    // Raveler, down to 1 loyalty: P0's token copy enters with 4.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Island", "Island", "Island"])
        .battlefield(1, &["Teferi, Time Raveler"])
        .counters(1, "Teferi, Time Raveler", Counter::Loyalty, 1)
        .hand(0, &["Fractured Identity"])
        .start();
    let tef = bf(&g, 1, "Teferi, Time Raveler");
    let fi = g.card(0, "Fractured Identity", Zone::Hand);
    let t = g.tgt(tef);
    cast_targets(&mut g, fi, &[t]);
    assert_eq!(g.obj(tef).zone, Zone::Exile);
    let tok = tokens_named(&g, "Teferi, Time Raveler");
    assert_eq!(tok.len(), 1, "{}", log(&g));
    assert_eq!(g.obj(tok[0]).controller, 0);
    assert_eq!(g.counters_on(tok[0], Counter::Loyalty), 4, "{}", log(&g));
}

#[test]
fn trample_assigns_lethal_damage_to_a_protected_blocker_before_the_player_cr_702_19c_702_16e() {
    // P0's Test Trampler (4/4 trample, green) blocked by P1's Test Bear,
    // which Mother of Runes then gives protection from green: lethal damage
    // (2) must still be assigned to the Bear (CR 702.19c: assignment ignores
    // protection, which only prevents), so P1 takes 2 and the Bear lives.
    let mut g = scen().battlefield(0, &["Test Trampler"]).battlefield(1, &["Test Bear", "Mother of Runes"]).start();
    let tr = bf(&g, 0, "Test Trampler");
    let bear = bf(&g, 1, "Test Bear");
    let mother = bf(&g, 1, "Mother of Runes");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(tr));
    g.choose(Choice::Pass);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g) && g.turn().priority == 1, |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) => {
            let d = g.pending_decision().unwrap();
            if d.options.iter().any(|o| o.choice == Choice::Card(tr)) && g.blockers_of(tr).is_empty() {
                Some(Choice::Card(tr))
            } else {
                Some(Choice::Pass)
            }
        }
        _ => None,
    });
    assert_eq!(g.blockers_of(tr), vec![bear], "{}", log(&g));
    g.choose(Choice::Ability(mother, AbilitySlot::Activated(0)));
    let tb = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(tb),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)), // green
        _ => None,
    });
    quiet_until_with(&mut g, |g| g.turn().step >= Step::EndCombat, |g| match g.kind() {
        // The least to the Bear: lethal, 2.
        Some(DecisionKind::AssignDamage) => g.pending_decision().unwrap().options.first().map(|o| o.choice),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(bear).damage, 0);
    assert_eq!(g.life(1), 18, "{}", log(&g));
}

#[test]
fn fractured_identity_on_a_face_down_permanent_gives_a_nameless_2_2_cr_708_2_707_2() {
    // P0 casts Fractured Identity at its own manifested Test Giant: the card
    // is exiled (revealed, CR 708.9) and P1 creates a token copy of the
    // face-down permanent: a face-up 2/2 with no name or abilities.
    let (mut g, fd) = manifested_then_main("Test Giant", &["Plains", "Plains", "Island", "Island", "Island"], &["Fractured Identity"]);
    let fi = g.card(0, "Fractured Identity", Zone::Hand);
    let t = g.tgt(fd);
    cast_targets(&mut g, fi, &[t]);
    assert_eq!(g.obj(fd).zone, Zone::Exile, "{}", log(&g));
    assert!(g.knows(1, fd));
    let tok: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.obj(c).controller == 1).collect();
    assert_eq!(tok.len(), 1, "{}", log(&g));
    assert!(!g.is_face_down(tok[0]));
    assert_eq!((g.pt(tok[0]), g.name(tok[0])), ((2, 2), ""));
}
