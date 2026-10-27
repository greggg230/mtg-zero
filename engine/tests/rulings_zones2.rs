//! M8d part 4 (ENGINE.md 40): the cube's zones rulings left after part 3:
//! exile that returns a new object (CR 400.7: Auras to the graveyard,
//! Equipment unattached, counters gone), cards played from exile or a
//! graveyard keeping their normal timing (601.3, 305.1-305.2, 307.1),
//! "cast" never playing a land (305.9, 601.2), permissions that end with
//! the turn or the card's zone change, escape and retrace again from the
//! graveyard (702.138, 702.81), face-up and face-down exile (406.3),
//! searches and shuffles that may find nothing (701.19), counts of split
//! cards (709.4), dual lands' basic types (205.3i, 305.6). `ruling:
//! <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Declare `attackers` (at the defending player) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let defender = 1 - g.turn().active;
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(defender));
        }
    }
    g.choose(Choice::Pass);
}

/// Answer `Target` decisions with `t` when it is an option (else first
/// options) and resolve the stack.
fn resolve_at(g: &mut Game, t: CardIdx) {
    let c = g.tgt(t);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(c)).then_some(c));
}

/// P`p` casts `name` from hand, answering targets with `t`, and everything
/// resolves.
fn cast_at(g: &mut Game, p: u8, name: &str, t: CardIdx) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve_at(g, t);
}

/// Pass (answering other decisions with first options) until P`p`'s next
/// first main phase with an empty stack.
fn to_main_of(g: &mut Game, p: u8) {
    let t0 = g.turn().number;
    quiet_until(g, |g| g.turn().number > t0 && g.turn().active == p && g.turn().step == Step::Main1 && g.stack().is_empty() && at_priority(g));
}

// ---- exile and return: a new object (CR 400.7) -------------------------------------------------

/// P1's first main phase: P1 equips Skateboard to its Test Cat (two +1/+1
/// counters) and returns its Test Bear with Animate Dead; then P0's first
/// main phase with `p0` on the battlefield and `hand0` in hand.
fn p1_equipped_and_enchanted(p0: &[&str], hand0: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, p0)
        .hand(0, hand0)
        .battlefield(1, &["Test Cat", "Skateboard", "Swamp", "Swamp", "Plains"])
        .counters(1, "Test Cat", Counter::P1P1, 2)
        .hand(1, &["Animate Dead"])
        .graveyard(1, &["Test Bear"])
        .at(Step::Main1, 1)
        .start();
    let cat = bf(&g, 1, "Test Cat");
    let board = bf(&g, 1, "Skateboard");
    g.choose(ab(board, 0));
    resolve_at(&mut g, cat);
    let bear = g.card(1, "Test Bear", Zone::Graveyard);
    cast_at(&mut g, 1, "Animate Dead", bear);
    assert_eq!(g.obj(board).attached, Some(g.obj_ref(cat)), "{}", log(&g));
    let ad = bf(&g, 1, "Animate Dead");
    assert_eq!(g.obj(ad).attached, Some(g.obj_ref(bear)), "{}", log(&g));
    assert_eq!(g.counters_on(cat, Counter::P1P1), 2);
    to_main_of(&mut g, 0);
    g
}

/// After P1's Cat (equipped) or Bear (enchanted) was exiled: the
/// Equipment stays on the battlefield unattached, the Aura goes to the
/// graveyard.
fn check_left(g: &Game, cat: bool, who: &str) {
    if cat {
        let board = bf(g, 1, "Skateboard");
        assert_eq!(g.obj(board).attached, None, "{who}: the Equipment stays, unattached\n{}", log(g));
    } else {
        assert_eq!(g.count_in(1, "Animate Dead", Zone::Graveyard), 1, "{who}: the Aura goes to the graveyard\n{}", log(g));
    }
}

#[test]
fn exiled_permanents_lose_auras_equipment_and_counters_and_return_as_new_objects_cr_400_7() {
    // ruling: 35f3a5dc 408732b7 9398525c 429a116f d395b444
    // P1's Test Cat (two +1/+1 counters, Skateboard) and its Test Bear
    // (returned by Animate Dead) are exiled by Palace Jailer, Portable Hole,
    // Leyline Binding, Touch the Spirit Realm and Flickerwisp: Animate Dead
    // goes to the graveyard, Skateboard stays unattached, the counters are
    // gone; what returns is a new object, unattached, without counters.
    let lands = ["Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Swamp", "Swamp", "Swamp"];
    for exiler in ["Palace Jailer", "Portable Hole", "Leyline Binding", "Touch the Spirit Realm", "Flickerwisp"] {
        for victim in ["Test Cat", "Test Bear"] {
            let who = format!("{exiler} on {victim}");
            let mut g = p1_equipped_and_enchanted(&lands, &[exiler, "Vindicate"]);
            let v = bf(&g, 1, victim);
            let old = g.obj_ref(v);
            cast_at(&mut g, 0, exiler, v);
            assert_eq!(g.obj(v).zone, Zone::Exile, "{who}\n{}", log(&g));
            check_left(&g, victim == "Test Cat", &who);
            assert_eq!(g.counters_on(v, Counter::P1P1), 0, "{who}: counters cease to exist");
            // Back: Flickerwisp's at the next end step; the others' when
            // they leave (Vindicate; Palace Jailer's when P1 becomes the
            // monarch: not here).
            match exiler {
                "Flickerwisp" => quiet_until(&mut g, |g| g.turn().step == Step::End && g.stack().is_empty() && at_priority(g)),
                "Palace Jailer" => continue,
                _ => {
                    let e = bf(&g, 0, exiler);
                    cast_at(&mut g, 0, "Vindicate", e);
                }
            }
            assert_eq!(g.obj(v).zone, Zone::Battlefield, "{who}: returned\n{}", log(&g));
            assert_ne!(g.obj_ref(v), old, "{who}: a new object");
            assert_eq!(g.obj(v).controller, 1, "{who}: under its owner's control");
            assert_eq!(g.counters_on(v, Counter::P1P1), 0, "{who}: no counters");
            let base = if victim == "Test Cat" { (2, 1) } else { (2, 2) };
            assert_eq!(g.pt(v), base, "{who}: no counters, no Equipment, no Aura\n{}", log(&g));
            let host = g.obj(bf(&g, 1, "Skateboard")).attached;
            assert!(host.is_none() || victim == "Test Bear", "{who}: still unattached");
            assert_eq!(g.count_in(1, "Animate Dead", Zone::Battlefield), (victim == "Test Cat") as usize, "{who}");
        }
    }
}

#[test]
fn displacer_kittens_flicker_returns_a_new_object_without_its_aura_equipment_or_counters_cr_400_7() {
    // ruling: d395b444
    // P0's Displacer Kitten: Animate Dead (a noncreature spell) flickers
    // P0's equipped Test Cat with two +1/+1 counters; Mishra's Bauble then
    // flickers the Test Bear Animate Dead returned. Skateboard stays
    // unattached, Animate Dead goes to the graveyard (and its "sacrifice"
    // finds no creature: the Bear is a new object), no counters.
    let mut g = scen()
        .battlefield(0, &["Displacer Kitten", "Test Cat", "Skateboard", "Swamp", "Swamp", "Swamp"])
        .counters(0, "Test Cat", Counter::P1P1, 2)
        .hand(0, &["Animate Dead", "Mishra's Bauble"])
        .graveyard(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let cat = bf(&g, 0, "Test Cat");
    let board = bf(&g, 0, "Skateboard");
    g.choose(ab(board, 0));
    resolve_at(&mut g, cat);
    assert_eq!(g.obj(board).attached, Some(g.obj_ref(cat)));
    let old = g.obj_ref(cat);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    let ad = g.card(0, "Animate Dead", Zone::Hand);
    g.choose(Choice::Cast(ad));
    let (tb, tc) = (g.tgt(bear), g.tgt(cat));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        Some(DecisionKind::Target) if g.has_option(tc) => Some(tc),
        _ => None,
    });
    assert_eq!(g.obj(cat).zone, Zone::Battlefield, "{}", log(&g));
    assert_ne!(g.obj_ref(cat), old, "a new object");
    assert_eq!(g.counters_on(cat, Counter::P1P1), 0, "no counters");
    assert_eq!(g.obj(board).attached, None, "the Equipment stays, unattached");
    assert_eq!(g.pt(cat), (2, 1));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    let old = g.obj_ref(bear);
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    assert_ne!(g.obj_ref(bear), old);
    assert_eq!(g.count_in(0, "Animate Dead", Zone::Graveyard), 1, "the Aura goes to the graveyard");
    assert_eq!(g.pt(bear), (2, 2), "no -1/-0");
}

#[test]
fn phelias_exile_returns_a_new_object_under_its_owners_control_without_its_counters_cr_400_7() {
    // ruling: 35f3a5dc
    // Phelia attacks and exiles P1's Test Cat (two +1/+1 counters,
    // Skateboard attached): Skateboard stays unattached; at the end step the
    // Cat returns under P1's control, a new object without counters (and
    // Phelia gets no counter: it didn't enter under P0's control).
    let mut g = p1_equipped_and_enchanted(&["Phelia, Exuberant Shepherd"], &[]);
    let ph = bf(&g, 0, "Phelia, Exuberant Shepherd");
    let cat = bf(&g, 1, "Test Cat");
    let old = g.obj_ref(cat);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    attack(&mut g, &[ph]);
    resolve_at(&mut g, cat);
    assert_eq!(g.obj(cat).zone, Zone::Exile, "{}", log(&g));
    check_left(&g, true, "Phelia");
    quiet_until_with(&mut g, |g| g.turn().step == Step::End && g.stack().is_empty() && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(cat).zone, Zone::Battlefield, "{}", log(&g));
    assert_ne!(g.obj_ref(cat), old);
    assert_eq!(g.obj(cat).controller, 1);
    assert_eq!(g.counters_on(cat, Counter::P1P1), 0);
    assert_eq!(g.counters_on(ph, Counter::P1P1), 0, "it entered under P1's control");
}

// ---- searches, shuffles and empty libraries ----------------------------------------------------

#[test]
fn path_to_exiles_search_may_be_declined_and_then_the_library_isnt_shuffled_cr_701_19b() {
    // ruling: cf14fa1b
    // P1 declines Path to Exile's search: no basic land, the library in the
    // same order (no shuffle).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Forest", "Test Bear", "Island", "Plains", "Lightning Bolt", "Mountain", "Swamp", "Counterspell"])
        .battlefield(0, &["Plains"])
        .hand(0, &["Path to Exile"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let lib = g.player(1).library.clone();
    let p = g.card(0, "Path to Exile", Zone::Hand);
    g.choose(Choice::Cast(p));
    let t = g.tgt(giant);
    let asked = std::cell::Cell::new(None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(k @ (DecisionKind::YesNo | DecisionKind::SearchLibrary)) if g.pending_decision().unwrap().player == 1 => {
            asked.set(Some(k));
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(g.obj(giant).zone, Zone::Exile);
    assert!(asked.get().is_some(), "P1 is asked\n{}", log(&g));
    assert_eq!(g.player(1).library, lib, "declined: the same library, not shuffled\n{}", log(&g));
    assert_eq!(g.permanents_of(1).count(), 0, "no land");
}

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

fn def_names(g: &Game) -> Vec<String> {
    let mut v: Vec<String> = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Def(d) => Some(db().def(d).name.to_string()),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn wan_shi_tong_triggers_on_cards_put_into_a_library_from_elsewhere_not_on_a_scry_or_reorder_cr_701_22_603_2() {
    // ruling: 35e7caab
    // Preordain's scry (one card to the bottom) and Ponder's reorder: no
    // Spirits. Brainstorm puts two cards from P0's hand on top: two Spirits.
    let mut g = scen()
        .battlefield(0, &["Wan Shi Tong, All-Knowing", "Island", "Island", "Island"])
        .hand(0, &["Preordain", "Ponder", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let spirits = |g: &Game| g.permanents_of(0).filter(|&c| g.name(c) == "Spirit").count();
    for spell in ["Preordain", "Ponder"] {
        let c = g.card(0, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        resolve_with(&mut g, |g| match g.kind() {
            // Scry: the top card to the bottom; Ponder: don't shuffle.
            Some(DecisionKind::PutOnBottom) => first_non_pass(g),
            Some(DecisionKind::YesNo) => Some(Choice::Pass),
            _ => None,
        });
        assert_eq!(spirits(&g), 0, "{spell}\n{}", log(&g));
    }
    let c = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve(&mut g);
    assert_eq!(spirits(&g), 2, "{}", log(&g));
}

#[test]
fn jace_ultimate_on_an_empty_hand_leaves_the_library_empty_and_the_player_loses_only_on_a_draw_cr_704_5b() {
    // ruling: 80ee1489
    // Jace's -12 on P1 with no cards in hand: P1's library is exiled and
    // stays empty; P1 hasn't lost until its next draw.
    let mut g = scen()
        .battlefield(0, &["Jace, the Mind Sculptor"])
        .counters(0, "Jace, the Mind Sculptor", Counter::Loyalty, 12)
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 0, "Jace, the Mind Sculptor");
    g.choose(ab(jace, 3));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert!(g.player(1).library.is_empty(), "{}", log(&g));
    assert!(g.player(1).hand.is_empty());
    assert!(!g.is_over(), "not lost yet");
    quiet_until(&mut g, |g| g.is_over() || (g.turn().active == 1 && g.turn().step == Step::Main1));
    assert_eq!(g.outcome().and_then(|o| o.winner), Some(0), "P1 loses as it draws from an empty library\n{}", log(&g));
}

#[test]
fn transmute_artifacts_sacrifice_is_an_effect_so_a_countered_transmute_sacrifices_nothing_cr_608_2() {
    // ruling: 81386b1f
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Sol Ring"])
        .hand(0, &["Transmute Artifact"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 0, "Sol Ring");
    let ta = g.card(0, "Transmute Artifact", Zone::Hand);
    g.choose(Choice::Cast(ta));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "no sacrifice as a cost");
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(ta);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(ta).zone, Zone::Graveyard);
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "countered: nothing sacrificed\n{}", log(&g));
}

#[test]
fn fatal_push_revolt_counts_a_token_leaving_the_battlefield_cr_111_7() {
    // ruling: 94a79602
    // P0 sacrifices its Clue (a token): revolt; Fatal Push destroys a
    // creature with mana value 4.
    let mut g = scen()
        .battlefield(0, &["Plains", "Swamp", "Swamp", "Swamp"])
        .hand(0, &["Thraben Inspector", "Fatal Push"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let ti = g.card(0, "Thraben Inspector", Zone::Hand);
    g.choose(Choice::Cast(ti));
    resolve(&mut g);
    let clue = tokens_named(&g, "Clue")[0];
    assert!(!g.player(0).left_battlefield);
    g.choose(ab(clue, 0));
    resolve(&mut g);
    assert!(g.player(0).left_battlefield, "{}", log(&g));
    let tr = bf(&g, 1, "Test Trampler");
    cast_at(&mut g, 0, "Fatal Push", tr);
    assert_eq!(g.obj(tr).zone, Zone::Graveyard, "revolt: mana value 4 or less\n{}", log(&g));
}

#[test]
fn a_split_card_is_one_card_when_discarded_cr_709_4() {
    // ruling: 7892efbd
    // Faithless Looting with Life // Death and two drawn Islands: discarding
    // Life // Death and an Island is two cards; one Island is left.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain"])
        .hand(0, &["Faithless Looting", "Life // Death"])
        .at(Step::Main1, 0)
        .start();
    let ld = g.card(0, "Life // Death", Zone::Hand);
    let fl = g.card(0, "Faithless Looting", Zone::Hand);
    g.choose(Choice::Cast(fl));
    let c = Choice::Card(ld);
    resolve_with(&mut g, |g| g.has_option(c).then_some(c));
    assert_eq!(g.obj(ld).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), 1, "two discarded: Life // Death and an Island\n{}", log(&g));
    assert_eq!(g.graveyard_cards(0).count(), 3, "Looting, Life // Death, an Island");
}

#[test]
fn unholy_heat_counts_the_graveyard_without_itself_cr_608_2h() {
    // ruling: 009c63c0
    // Three card types in P0's graveyard (land, creature, artifact): Unholy
    // Heat, an instant, isn't there yet as it resolves: 2 damage, not 6.
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .graveyard(0, &["Island", "Test Bear", "Sol Ring"])
        .hand(0, &["Unholy Heat"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let tr = bf(&g, 1, "Test Trampler");
    cast_at(&mut g, 0, "Unholy Heat", tr);
    assert_eq!(g.obj(tr).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(tr).damage, 2);
    assert_eq!(g.count_in(0, "Unholy Heat", Zone::Graveyard), 1);
}

#[test]
fn ursine_monstrosity_with_nothing_to_mill_still_does_the_rest_cr_701_13b() {
    // ruling: ccae9ec2
    // P0's library is empty: no card milled, but Ursine still gains
    // indestructible and +1/+1 per card type in the graveyard (one: land).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Ursine Monstrosity"])
        .graveyard(0, &["Island"])
        .at(Step::BeginCombat, 0)
        .start();
    let u = bf(&g, 0, "Ursine Monstrosity");
    resolve(&mut g);
    assert!(g.has_kw(u, Kw::Indestructible), "{}", log(&g));
    assert_eq!(g.pt(u), (4, 4), "{}", log(&g));
    assert!(!g.is_over());
}

#[test]
fn a_token_with_persist_dies_and_triggers_but_cant_return_cr_111_7_702_79a() {
    // ruling: 15f83ca8
    // Reflection of Kiki-Jiki copies Woodfall Primus (a token with persist
    // and no -1/-1 counter); sacrificed at the end step, persist triggers
    // and resolves, and nothing returns.
    let mut g = scen()
        .battlefield(0, &["Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Woodfall Primus", "Mountain"])
        .transformed(0, "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki")
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let wp = bf(&g, 0, "Woodfall Primus");
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(ab(r, 0));
    let (tw, tr) = (g.tgt(wp), g.tgt(ring));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tw) => Some(tw),
        Some(DecisionKind::Target) if g.has_option(tr) => Some(tr),
        _ => None,
    });
    assert_eq!(tokens_named(&g, "Woodfall Primus").len(), 1, "{}", log(&g));
    let n = g.log_lines().len();
    quiet_until(&mut g, |g| g.turn().step == Step::End && g.stack().is_empty() && at_priority(g) && tokens_named(g, "Woodfall Primus").is_empty());
    resolve(&mut g);
    let later = g.log_lines()[n..].join("\n");
    assert!(later.contains("trigger of Woodfall Primus goes on the stack"), "persist triggers\n{later}");
    assert!(tokens_named(&g, "Woodfall Primus").is_empty(), "the token can't return\n{later}");
    assert_eq!(g.count_in(0, "Woodfall Primus", Zone::Battlefield), 1, "the card is untouched");
}

#[test]
fn uros_land_isnt_a_land_play_cr_305_2_305_4() {
    // ruling: 5aaccf2e
    // P0 has played its land this turn; Uro (cast, sacrificed) still puts a
    // land from hand onto the battlefield, and the land play count is
    // unchanged.
    let mut g = scen()
        .battlefield(0, &["Forest", "Island", "Island"])
        .hand(0, &["Uro, Titan of Nature's Wrath", "Forest"])
        .lands_played(0, 1)
        .at(Step::Main1, 0)
        .start();
    let forest = g.card(0, "Forest", Zone::Hand);
    assert!(!g.has_option(Choice::Play(forest)), "the land play is used");
    let uro = g.card(0, "Uro, Titan of Nature's Wrath", Zone::Hand);
    g.choose(Choice::Cast(uro));
    let f = Choice::Card(forest);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(f) => Some(f),
        _ => None,
    });
    assert_eq!(g.obj(forest).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.player(0).lands_played, 1);
}

#[test]
fn atraxa_takes_a_card_with_two_types_as_either_type_cr_205_2a() {
    // ruling: 8b54ef2f
    // Revealed: Phyrexian Metamorph (an artifact creature), a Test Bear and
    // a Sol Ring. The Metamorph taken as the artifact, the Bear as the
    // creature; then the Sol Ring can't be taken (no type left for it).
    let lib = ["Phyrexian Metamorph", "Test Bear", "Sol Ring", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island"];
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Plains", "Island", "Swamp", "Forest", "Plains", "Island", "Swamp"])
        .hand(0, &["Atraxa, Grand Unifier"])
        .at(Step::Main1, 0)
        .start();
    let meta = g.card(0, "Phyrexian Metamorph", Zone::Library);
    let bear = g.card(0, "Test Bear", Zone::Library);
    let ring = g.card(0, "Sol Ring", Zone::Library);
    let c = g.card(0, "Atraxa, Grand Unifier", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand));
    g.choose(Choice::Card(meta));
    assert!(g.has_option(Choice::Card(bear)), "the Metamorph can be the artifact: {}", opts(&g));
    g.choose(Choice::Card(bear));
    assert!(!g.has_option(Choice::Card(ring)), "artifact and creature both taken: {}", opts(&g));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(meta).zone, Zone::Hand);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.obj(ring).zone, Zone::Library);
}

#[test]
fn dual_lands_have_basic_land_types_but_arent_basic_cr_205_3i_205_4a() {
    // ruling: fa3e7a03 e20baeb2 1e872694 81d5d41d
    // Path to Exile's "basic land card" finds the Island and the Forest,
    // not Watery Grave, Underground Sea or Temple Garden; Polluted Delta's
    // "Island or Swamp card" finds Watery Grave and Underground Sea; domain
    // counts the types of Hallowed Fountain, Blood Crypt and Stomping Ground
    // (Leyline Binding costs {W}).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Watery Grave", "Underground Sea", "Temple Garden", "Island", "Forest"])
        .battlefield(0, &["Plains"])
        .hand(0, &["Path to Exile"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let p = g.card(0, "Path to Exile", Zone::Hand);
    g.choose(Choice::Cast(p));
    let t = g.tgt(bear);
    let found = std::cell::RefCell::new(Vec::new());
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::SearchLibrary) => {
            *found.borrow_mut() = def_names(g);
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(*found.borrow(), vec!["Forest".to_string(), "Island".to_string()], "basic lands only\n{}", log(&g));
    // Polluted Delta.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Watery Grave", "Underground Sea", "Temple Garden", "Forest"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Polluted Delta"])
        .at(Step::Main1, 0)
        .start();
    let pd = bf(&g, 0, "Polluted Delta");
    g.choose(ab(pd, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    assert_eq!(def_names(&g), vec!["Underground Sea".to_string(), "Watery Grave".to_string()], "Island or Swamp cards");
    // Domain.
    let mut g = scen()
        .battlefield(0, &["Hallowed Fountain", "Blood Crypt", "Stomping Ground"])
        .hand(0, &["Leyline Binding"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let lb = g.card(0, "Leyline Binding", Zone::Hand);
    assert!(g.has_option(Choice::Cast(lb)), "domain 5: {{W}}: {}", opts(&g));
    let bear = bf(&g, 1, "Test Bear");
    cast_at(&mut g, 0, "Leyline Binding", bear);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 1, "one land tapped");
}

// ---- cards played with a permission keep their normal timing (CR 601.3, 307.1, 305.2) ----------

/// Cast `c`'s Adventure (its other face).
fn cast_adventure(g: &mut Game, c: CardIdx) {
    let ch = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&o| matches!(o, Choice::CastFace(x) | Choice::Faces(x, _) if x == c));
    g.choose(ch.unwrap_or_else(|| panic!("no option for the card: {}", opts(g))));
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(1));
    }
}

/// Can the player with priority play `c` (cast it or play it as a land)?
fn playable(g: &Game, c: CardIdx) -> bool {
    g.has_option(Choice::Cast(c)) || g.has_option(Choice::Play(c))
}

/// The card a combat trigger of P0's `source` exiles: `top` is the top of
/// the library it exiles from (P0's for Laelia and Tersa's graveyard,
/// P1's for Robber of the Rich, Ragavan and Fallen Shinobi). The game
/// stops at P0's priority in combat with the trigger resolved.
fn exiled_in_combat(source: &str, top: &str, lands_played: u8) -> (Game, CardIdx) {
    let mut s = Scenario::new(db()).spec(|s| s.auto_single = false).lands_played(0, lands_played);
    s = match source {
        "Laelia, the Blade Reforged" => s.library(0, &[top, "Island", "Island", "Island"]).library(1, &["Forest"; 5]),
        "Tersa Lightshatter" => s.library(0, &["Island"; 5]).library(1, &["Forest"; 5]).graveyard(0, &[top; 7]),
        _ => s.library(0, &["Island"; 5]).library(1, &[top, top, "Forest", "Forest"]).hand(1, &["Forest", "Forest"]),
    };
    let mut g = s.battlefield(0, &[source, "Island"]).at(Step::DeclareAttackers, 0).start();
    let a = bf(&g, 0, source);
    attack(&mut g, &[a]);
    quiet_until(&mut g, |g| g.exile_zone().iter().any(|&c| g.name(c) == top) && g.stack().is_empty() && at_priority(g) && g.turn().priority == 0);
    assert!(g.turn().step < Step::Main2, "{source}: still in combat");
    let c = *g.exile_zone().iter().find(|&&c| g.name(c) == top).unwrap();
    (g, c)
}

fn to_main2(g: &mut Game) {
    quiet_until(g, |g| g.turn().step == Step::Main2 && g.stack().is_empty() && at_priority(g));
}

#[test]
fn cards_exiled_by_combat_triggers_are_played_with_their_normal_timing_cr_601_3_307_1_305_2() {
    // ruling: 8e48cc76 0614c7a7 93cdcc40 1e4d5a35 b5df7fc5 edc91fad
    // Laelia, Robber of the Rich, Ragavan, Fallen Shinobi and Tersa exile a
    // sorcery (Preordain) or a land in combat. The sorcery can be cast only
    // in the main phase after combat (paying its cost, except Fallen
    // Shinobi's free play); the land only there, and only with a land play
    // left. Robber's and Ragavan's "cast" never plays a land.
    for source in ["Laelia, the Blade Reforged", "Robber of the Rich", "Ragavan, Nimble Pilferer", "Fallen Shinobi", "Tersa Lightshatter"] {
        let (mut g, pre) = exiled_in_combat(source, "Preordain", 0);
        assert!(!playable(&g, pre), "{source}: a sorcery, not in combat: {}", opts(&g));
        to_main2(&mut g);
        assert!(g.has_option(Choice::Cast(pre)), "{source}: in the main phase: {}\n{}", opts(&g), log(&g));
        let island = bf(&g, 0, "Island");
        g.choose(Choice::Cast(pre));
        resolve(&mut g);
        assert_ne!(g.obj(pre).zone, Zone::Exile, "{source}: cast");
        let free = source == "Fallen Shinobi";
        assert_eq!(g.obj(island).tapped, !free, "{source}: its cost paid (Fallen Shinobi: free)\n{}", log(&g));
        let cast_only = matches!(source, "Robber of the Rich" | "Ragavan, Nimble Pilferer");
        let (mut g, isl) = exiled_in_combat(source, "Island", 0);
        assert!(!playable(&g, isl), "{source}: a land, not in combat");
        to_main2(&mut g);
        assert_eq!(g.has_option(Choice::Play(isl)), !cast_only, "{source}: a land with the land play free ('cast' plays no land): {}", opts(&g));
        if !cast_only {
            let (mut g, isl) = exiled_in_combat(source, "Island", 1);
            to_main2(&mut g);
            assert!(!g.has_option(Choice::Play(isl)), "{source}: no land play left");
        }
    }
}

/// P0 casts Lightning Bolt at P1 and keeps priority (a spell on the
/// stack).
fn bolt_held(g: &mut Game) {
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(player(1));
    }
    pay(g);
    g.drive_until(|g| at_priority(g));
    assert!(!g.stack().is_empty() && g.turn().priority == 0, "{}", log(g));
}

/// Check `c` (a sorcery-speed card P0 may play) isn't playable with a spell
/// on the stack and is once it resolves.
fn sorcery_timing(g: &mut Game, c: CardIdx, who: &str) {
    assert!(playable(g, c), "{who}: main phase, empty stack: {}\n{}", opts(g), log(g));
    bolt_held(g);
    assert!(!playable(g, c), "{who}: not with a spell on the stack: {}", opts(g));
    resolve(g);
    assert!(playable(g, c), "{who}: again once it resolved");
}

#[test]
fn cards_exiled_by_urza_golos_ugin_and_expressive_iteration_keep_their_timing_cr_601_3_307_1_305_2() {
    // ruling: a32fb196 3f63bb36 8d32d60f 9a50f5e0
    // Each exiles a card P0 may play this turn (Urza and Golos free, Ugin's
    // -11 free, Expressive Iteration for its cost): a sorcery-speed card
    // isn't playable with a spell on the stack; a land needs a land play.
    let lands = ["Plains", "Island", "Swamp", "Mountain", "Forest", "Mountain", "Island", "Mountain", "Mountain"];
    let start = |bf0: &[&str], lib: &[&str], played: u8| {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, lib)
            .library(1, &["Forest"; 5])
            .battlefield(0, bf0)
            .battlefield(0, &lands)
            .hand(0, &["Lightning Bolt", "Lightning Bolt"])
            .lands_played(0, played)
            .at(Step::Main1, 0)
            .start()
    };
    for (top, played) in [("Preordain", 0), ("Island", 0), ("Island", 1)] {
        // Urza ({5}: shuffle, exile the top card).
        let mut g = start(&["Urza, Lord High Artificer"], &[top; 8], played);
        let u = bf(&g, 0, "Urza, Lord High Artificer");
        g.choose(ab(u, 1));
        resolve(&mut g);
        let c = g.card(0, top, Zone::Exile);
        if played == 1 {
            assert!(!playable(&g, c), "Urza: no land play left");
        } else {
            sorcery_timing(&mut g, c, "Urza");
        }
        // Golos.
        let mut g = start(&["Golos, Tireless Pilgrim"], &[top, "Plains", "Plains", "Plains"], played);
        let go = bf(&g, 0, "Golos, Tireless Pilgrim");
        g.choose(ab(go, 0));
        resolve(&mut g);
        let c = g.card(0, top, Zone::Exile);
        if played == 1 {
            assert!(!playable(&g, c), "Golos: no land play left");
        } else {
            sorcery_timing(&mut g, c, "Golos");
        }
        // Expressive Iteration: the third card exiled.
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Plains", "Plains", top, "Plains"])
            .library(1, &["Forest"; 5])
            .battlefield(0, &lands)
            .hand(0, &["Expressive Iteration", "Lightning Bolt"])
            .lands_played(0, played)
            .at(Step::Main1, 0)
            .start();
        let it = g.card(0, "Expressive Iteration", Zone::Hand);
        g.choose(Choice::Cast(it));
        let tc = g.card(0, top, Zone::Library);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::PutInHand | DecisionKind::PutOnBottom) => {
                g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass && c != Choice::Card(tc))
            }
            _ => None,
        });
        assert_eq!(g.obj(tc).zone, Zone::Exile, "Expressive Iteration\n{}", log(&g));
        if played == 1 {
            assert!(!playable(&g, tc), "Expressive Iteration: no land play left");
        } else {
            sorcery_timing(&mut g, tc, "Expressive Iteration");
        }
    }
    // Ugin's -11: Sol Ring (an artifact: sorcery timing), free.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Sol Ring", "Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Ugin, Eye of the Storms", "Mountain"])
        .counters(0, "Ugin, Eye of the Storms", Counter::Loyalty, 11)
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ug = bf(&g, 0, "Ugin, Eye of the Storms");
    let ring = g.card(0, "Sol Ring", Zone::Library);
    g.choose(ab(ug, 2));
    let r = Choice::Def(g.obj(ring).def);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) if g.has_option(r) => Some(r),
        Some(DecisionKind::SearchLibrary) => Some(Choice::Pass),
        _ => None,
    });
    let ring = g.card(0, "Sol Ring", Zone::Exile);
    sorcery_timing(&mut g, ring, "Ugin");
}

#[test]
fn intis_exiled_card_is_played_with_its_timing_and_costs_cr_601_3_307_1() {
    // ruling: a4ed176d
    // Inti's attack trigger discards a Bolt: Preordain is exiled. Not
    // castable in combat; in the second main phase, not with a spell on the
    // stack, and for its cost.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Preordain", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Inti, Seneschal of the Sun", "Island", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let inti = bf(&g, 0, "Inti, Seneschal of the Sun");
    attack(&mut g, &[inti]);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let pre = g.card(0, "Preordain", Zone::Library);
    let t = g.tgt(inti);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(Choice::Card(bolt)) => Some(Choice::Card(bolt)),
        Some(DecisionKind::Target) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(pre).zone, Zone::Exile, "{}", log(&g));
    assert!(g.turn().step < Step::Main2);
    assert!(!playable(&g, pre), "not in combat");
    to_main2(&mut g);
    sorcery_timing(&mut g, pre, "Inti");
    let island = bf(&g, 0, "Island");
    g.choose(Choice::Cast(pre));
    resolve(&mut g);
    assert!(g.obj(island).tapped, "its cost paid\n{}", log(&g));
}

#[test]
fn cards_cast_or_played_from_a_graveyard_keep_their_timing_cr_601_3_307_1_305_2() {
    // ruling: 6d4a6721 2deb11e0 c96d5ecf 84c13ea1 a6bfcffb
    // Emry's Sol Ring, Lurrus's Test Cat: not with a spell on the stack
    // (Lurrus's Test Flash: yes, it has flash). Crucible of Worlds and
    // Icetill Explorer: a land from the graveyard not with a spell on the
    // stack, and only with a land play left (Icetill: two). Lurrus alone
    // plays no land from the graveyard.
    let base = |bf0: &[&str], gy: &[&str]| {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 5])
            .library(1, &["Forest"; 5])
            .battlefield(0, bf0)
            .battlefield(0, &["Mountain", "Mountain", "Island", "Island", "Plains"])
            .graveyard(0, gy)
            .hand(0, &["Lightning Bolt", "Lightning Bolt"])
            .at(Step::Main1, 0)
            .start()
    };
    // Emry.
    let mut g = base(&["Emry, Lurker of the Loch"], &["Sol Ring"]);
    let emry = bf(&g, 0, "Emry, Lurker of the Loch");
    let ring = g.card(0, "Sol Ring", Zone::Graveyard);
    g.choose(ab(emry, 0));
    resolve_at(&mut g, ring);
    sorcery_timing(&mut g, ring, "Emry");
    // Lurrus.
    let mut g = base(&["Lurrus of the Dream-Den"], &["Test Cat", "Test Flash", "Island"]);
    let bear = g.card(0, "Test Cat", Zone::Graveyard);
    let flash = g.card(0, "Test Flash", Zone::Graveyard);
    let island = g.card(0, "Island", Zone::Graveyard);
    assert!(!g.has_option(Choice::Play(island)), "Lurrus plays no land");
    assert!(g.has_option(Choice::Cast(flash)));
    sorcery_timing(&mut g, bear, "Lurrus");
    bolt_held(&mut g);
    assert!(g.has_option(Choice::Cast(flash)), "flash: with a spell on the stack: {}", opts(&g));
    // Crucible of Worlds; Icetill Explorer.
    for (perm, plays) in [("Crucible of Worlds", 1u8), ("Icetill Explorer", 2)] {
        let mut g = base(&[perm], &["Island", "Island", "Island"]);
        let i0 = g.card(0, "Island", Zone::Graveyard);
        sorcery_timing(&mut g, i0, perm);
        for _ in 0..plays {
            let i = g.card(0, "Island", Zone::Graveyard);
            assert!(g.has_option(Choice::Play(i)), "{perm}");
            g.choose(Choice::Play(i));
            resolve(&mut g);
        }
        let i = g.card(0, "Island", Zone::Graveyard);
        assert!(!g.has_option(Choice::Play(i)), "{perm}: no land play left: {}", opts(&g));
    }
}

#[test]
fn an_owners_permission_to_cast_an_exiled_card_keeps_its_timing_spellbinder_and_adventures_cr_601_3_715_4() {
    // ruling: f0d7b0c7 7ddcc330 5b5ff3d6
    // Elite Spellbinder exiles P1's Preordain: P1 can't cast it on P0's
    // turn, only in its own main phase (for {2} more). Battle Display
    // resolved: Embereth Shieldbreaker on its adventure is cast at sorcery
    // speed; Petty Theft resolved: Brazen Borrower (flash) on P1's turn.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Elite Spellbinder"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Preordain"])
        .at(Step::Main1, 0)
        .start();
    let pre = g.card(1, "Preordain", Zone::Hand);
    let es = g.card(0, "Elite Spellbinder", Zone::Hand);
    g.choose(Choice::Cast(es));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ if g.has_option(Choice::Card(pre)) => Some(Choice::Card(pre)),
        _ => None,
    });
    assert_eq!(g.obj(pre).zone, Zone::Exile, "{}", log(&g));
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert!(!playable(&g, pre), "P0's turn: a sorcery can't be cast");
    to_main_of(&mut g, 1);
    assert!(g.has_option(Choice::Cast(pre)), "P1's main phase: {} for {{U}}{{2}}", opts(&g));
    // Embereth Shieldbreaker // Battle Display.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Embereth Shieldbreaker // Battle Display", "Lightning Bolt"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let es = g.card(0, "Embereth Shieldbreaker // Battle Display", Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    cast_adventure(&mut g, es);
    resolve_at(&mut g, ring);
    assert_eq!(g.obj(es).zone, Zone::Exile, "on an adventure\n{}", log(&g));
    sorcery_timing(&mut g, es, "Embereth Shieldbreaker");
    // Brazen Borrower // Petty Theft: the creature has flash.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Brazen Borrower // Petty Theft"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    cast_adventure(&mut g, bb);
    resolve_at(&mut g, ring);
    assert_eq!(g.obj(bb).zone, Zone::Exile, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Upkeep && at_priority(g) && g.turn().priority == 0);
    assert!(g.has_option(Choice::Cast(bb)), "flash: on P1's turn: {}", opts(&g));
}

// ---- "cast" plays no land (CR 305.1, 601.2) ------------------------------------------------------

#[test]
fn crabominations_cast_plays_no_land_and_the_cards_not_cast_stay_exiled_cr_305_1_406_6() {
    // ruling: 33c73fb6
    // Crabomination exiles P1's top card (an Island), a card at random from
    // its graveyard (a Forest) and from its hand (Lightning Bolt): the lands
    // can't be played, P0 declines the Bolt; all three stay in exile and
    // none can be played later.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Mountain"])
        .hand(0, &["Crabomination"])
        .library(1, &["Island"])
        .graveyard(1, &["Forest"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let crab = g.card(0, "Crabomination", Zone::Hand);
    g.choose(Choice::Cast(crab));
    let offered = std::cell::RefCell::new(String::new());
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseExiled) => {
            *offered.borrow_mut() = opts(g);
            Some(Choice::Pass)
        }
        _ => None,
    });
    let offered = offered.borrow().clone();
    assert!(offered.contains("Lightning Bolt") && !offered.contains("Island") && !offered.contains("Forest"), "only the spell: {offered}\n{}", log(&g));
    for n in ["Island", "Forest", "Lightning Bolt"] {
        let c = g.card(1, n, Zone::Exile);
        assert!(!playable(&g, c), "{n}: not playable later");
    }
    assert_eq!(g.player(0).lands_played, 0);
}

#[test]
fn malcolm_cant_play_a_discarded_land_cr_305_1() {
    // ruling: c86d4e2f
    // Malcolm's fourth chorus counter: P0 draws an Island and discards it;
    // "cast the discarded card" doesn't play a land: it stays in the
    // graveyard, the land play unused.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Malcolm, Alluring Scoundrel"])
        .counters(0, "Malcolm, Alluring Scoundrel", Counter::Named(db().subtype_id("counter:chorus").unwrap()), 3)
        .at(Step::DeclareAttackers, 0)
        .start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    attack(&mut g, &[m]);
    let asked = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::Discard) | Some(DecisionKind::ChooseHandCard) => first_non_pass(g),
        Some(DecisionKind::YesNo) => {
            asked.set(true);
            Some(Choice::Yes)
        }
        _ => None,
    });
    let i = g.card(0, "Island", Zone::Graveyard);
    assert!(!asked.get(), "no cast offered for a land\n{}", log(&g));
    assert!(!g.has_option(Choice::Play(i)), "{}", opts(&g));
    assert_eq!(g.player(0).lands_played, 0);
}

#[test]
fn airbent_lands_and_tokens_cant_be_cast_from_exile_cr_305_1_111_7() {
    // ruling: f3306d74 9e31018b
    // Life makes P0's lands creatures; Aang airbends a Plains creature: the
    // Plains card stays in exile, never cast or played. Aang airbends a
    // Spirit token: it ceases to exist.
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Forest", "Plains", "Plains", "Island"])
        .hand(0, &["Life // Death", "Aang, Swift Savior // Aang and La, Ocean's Fury"])
        .at(Step::Main1, 0)
        .start();
    let ld = g.card(0, "Life // Death", Zone::Hand);
    g.choose(Choice::Cast(ld));
    resolve(&mut g);
    let plains = bf(&g, 0, "Plains");
    assert!(g.is_creature(plains), "Life: a land creature\n{}", log(&g));
    let aang = g.card(0, "Aang, Swift Savior // Aang and La, Ocean's Fury", Zone::Hand);
    g.choose(Choice::Cast(aang));
    let tp = g.tgt(plains);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => {
            // (Pay with the other lands.)
            let d = g.pending_decision().unwrap();
            d.options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(plains))
        }
        Some(DecisionKind::Target) if g.has_option(tp) => Some(tp),
        _ => None,
    });
    assert_eq!(g.obj(plains).zone, Zone::Exile, "{}", log(&g));
    assert!(!playable(&g, plains), "a land card: not cast for {{2}}, not played: {}", opts(&g));
    to_main_of(&mut g, 0);
    assert_eq!(g.obj(plains).zone, Zone::Exile);
    assert!(!playable(&g, plains), "{}", opts(&g));
    // A token.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Island", "Plains", "Island"])
        .hand(0, &["Lingering Souls", "Aang, Swift Savior // Aang and La, Ocean's Fury"])
        .at(Step::Main1, 0)
        .start();
    let ls = g.card(0, "Lingering Souls", Zone::Hand);
    g.choose(Choice::Cast(ls));
    resolve(&mut g);
    let sp = tokens_named(&g, "Spirit");
    assert_eq!(sp.len(), 2);
    let aang = g.card(0, "Aang, Swift Savior // Aang and La, Ocean's Fury", Zone::Hand);
    g.choose(Choice::Cast(aang));
    let t = g.tgt(sp[0]);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(tokens_named(&g, "Spirit").len(), 1, "{}", log(&g));
    assert!(g.exile_zone().iter().all(|&c| !g.is_token(c)), "the token ceased to exist");
}

// ---- permissions that end; cards left where they are (CR 611.2a, 400.7) ------------------------

#[test]
fn an_unused_permission_ends_and_the_card_stays_where_it_is_cr_611_2a() {
    // ruling: 703251e7 751982e2 2b2a5f1d
    // Jace, Telepath Unbound's -3 on a Lightning Bolt in P0's graveyard,
    // Urza's exiled Preordain, Expressive Iteration's exiled Preordain: not
    // played this turn, on P0's next turn the Bolt is still in the
    // graveyard and the Preordains in exile, none playable.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Preordain"; 10])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Jace, Vryn's Prodigy // Jace, Telepath Unbound", "Urza, Lord High Artificer", "Island", "Island", "Island", "Island", "Island"])
        .transformed(0, "Jace, Vryn's Prodigy // Jace, Telepath Unbound")
        .graveyard(0, &["Lightning Bolt"])
        .battlefield(0, &["Mountain"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 0, "Jace, Telepath Unbound");
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    g.choose(ab(jace, 1));
    resolve_at(&mut g, bolt);
    assert!(g.has_option(Choice::Cast(bolt)), "Jace: this turn: {}", opts(&g));
    let urza = bf(&g, 0, "Urza, Lord High Artificer");
    g.choose(ab(urza, 1));
    resolve(&mut g);
    let pre = g.card(0, "Preordain", Zone::Exile);
    assert!(g.has_option(Choice::Cast(pre)), "Urza: this turn");
    quiet_until(&mut g, |g| g.turn().number > 4 && g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "Jace: it remains in the graveyard");
    assert!(!g.has_option(Choice::Cast(bolt)), "Jace: the turn is over");
    assert_eq!(g.obj(pre).zone, Zone::Exile, "Urza: it remains in exile");
    assert!(!g.has_option(Choice::Cast(pre)), "Urza: the turn is over");
    // Expressive Iteration.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island", "Preordain", "Island", "Island", "Island", "Island"])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Island", "Mountain", "Island"])
        .hand(0, &["Expressive Iteration"])
        .at(Step::Main1, 0)
        .start();
    let pre = g.card(0, "Preordain", Zone::Library);
    let it = g.card(0, "Expressive Iteration", Zone::Hand);
    g.choose(Choice::Cast(it));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand | DecisionKind::PutOnBottom) => {
            g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass && c != Choice::Card(pre))
        }
        _ => None,
    });
    assert!(g.has_option(Choice::Cast(pre)), "this turn: {}", opts(&g));
    to_main_of(&mut g, 0);
    assert_eq!(g.obj(pre).zone, Zone::Exile, "Expressive Iteration: still exiled");
    assert!(!g.has_option(Choice::Cast(pre)), "not on a later turn: {}", opts(&g));
}

// ---- a card that leaves and comes back is a new object (CR 400.7) ------------------------------

#[test]
fn a_card_cast_with_a_permission_that_comes_back_is_a_new_object_without_it_cr_400_7() {
    // ruling: b2c5a21b 9b682c79 1e2502fc
    // Emry's Sol Ring, countered back into the graveyard, can't be cast
    // again; Robber's exiled Lightning Bolt cast once is gone from exile;
    // Laelia's exiled Test Hasty cast and then exiled by Path to Exile
    // can't be played again.
    let mut g = scen()
        .battlefield(0, &["Emry, Lurker of the Loch", "Island", "Island"])
        .graveyard(0, &["Sol Ring"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let emry = bf(&g, 0, "Emry, Lurker of the Loch");
    let ring = g.card(0, "Sol Ring", Zone::Graveyard);
    g.choose(ab(emry, 0));
    resolve_at(&mut g, ring);
    g.choose(Choice::Cast(ring));
    g.drive_until(|g| g.kind() != Some(DecisionKind::PayMana) && !(at_priority(g) && g.turn().priority == 0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(ring);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "{}", log(&g));
    assert!(!g.has_option(Choice::Cast(ring)), "a new object: no permission: {}", opts(&g));
    // Robber of the Rich: the Bolt cast once.
    let (mut g, bolt) = exiled_in_combat("Robber of the Rich", "Lightning Bolt", 0);
    g.choose(Choice::Cast(bolt));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "{}", log(&g));
    assert!(!g.has_option(Choice::Cast(bolt)), "once");
    // Laelia: Test Hasty cast, then exiled again.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Test Hasty", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Laelia, the Blade Reforged", "Mountain", "Plains"])
        .hand(0, &["Path to Exile"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let l = bf(&g, 0, "Laelia, the Blade Reforged");
    attack(&mut g, &[l]);
    to_main2(&mut g);
    let th = g.card(0, "Test Hasty", Zone::Exile);
    g.choose(Choice::Cast(th));
    resolve(&mut g);
    assert_eq!(g.obj(th).zone, Zone::Battlefield, "{}", log(&g));
    cast_at(&mut g, 0, "Path to Exile", th);
    assert_eq!(g.obj(th).zone, Zone::Exile, "{}", log(&g));
    assert!(!playable(&g, th), "a new object in exile: {}", opts(&g));
}

#[test]
fn a_flickered_loot_is_a_new_object_whose_exhaust_abilities_can_be_activated_again_cr_400_7_702_177() {
    // ruling: fb4b130d
    // Loot draws three with its exhaust ability; Displacer Kitten (Mishra's
    // Bauble cast) flickers it: the new Loot (haste) may draw three again.
    let mut g = scen()
        .battlefield(0, &["Loot, the Pathfinder", "Displacer Kitten", "Island", "Island"])
        .hand(0, &["Mishra's Bauble"])
        .at(Step::Main1, 0)
        .start();
    let loot = bf(&g, 0, "Loot, the Pathfinder");
    g.choose(ab(loot, 1));
    resolve(&mut g);
    assert_eq!(g.player(0).hand.len(), 4, "{}", log(&g));
    // Exhausted (and tapped).
    assert!(!g.has_option(ab(loot, 1)));
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    resolve_at(&mut g, loot);
    assert_eq!(g.obj(loot).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.has_option(ab(loot, 1)), "a new object: {}", opts(&g));
    g.choose(ab(loot, 1));
    resolve(&mut g);
    assert_eq!(g.player(0).hand.len(), 6);
}

// ---- escape and retrace again (CR 702.138, 702.81) -----------------------------------------------

/// Cast `c` from P0's graveyard, answering targets with `t` (if any) and
/// costs with first options, and resolve.
fn cast_from_gy(g: &mut Game, c: CardIdx, t: Option<Choice>) {
    g.choose(Choice::Cast(c));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => t.filter(|&t| g.has_option(t)).or_else(|| first_non_pass(g)),
        Some(DecisionKind::ChooseRoute) => first_non_pass(g),
        _ => None,
    });
}

#[test]
fn escaped_spells_return_to_the_graveyard_and_can_escape_again_cr_702_138() {
    // ruling: fd5ee67c a7269192 5a231246
    // Underworld Breach: an escaped Lightning Bolt returns to the graveyard
    // and escapes again. From the Catacombs escaped returns to the
    // graveyard and is offered again. Phlage and Uro escaped stay on the
    // battlefield; destroyed (Vindicate), they can escape again.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Mountain", "Mountain"])
        .graveyard(0, &["Lightning Bolt", "Island", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    cast_from_gy(&mut g, bolt, Some(player(1)));
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "{}", log(&g));
    assert!(g.has_option(Choice::Cast(bolt)), "escape again: {}", opts(&g));
    cast_from_gy(&mut g, bolt, Some(player(1)));
    assert_eq!(g.life(1), 14);
    // From the Catacombs (its own escape).
    let gy = ["From the Catacombs", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island"];
    let mut g = scen()
        .battlefield(0, &["Swamp"; 10])
        .graveyard(0, &gy)
        .graveyard(1, &["Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "From the Catacombs", Zone::Graveyard);
    cast_from_gy(&mut g, fc, None);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.obj(fc).zone, Zone::Graveyard, "it returns to the graveyard");
    assert!(g.has_option(Choice::Cast(fc)), "five more cards: escape again: {}", opts(&g));
    // Phlage and Uro.
    for (titan, lands) in [("Phlage, Titan of Fire's Fury", ["Mountain", "Mountain", "Plains", "Plains"]), ("Uro, Titan of Nature's Wrath", ["Forest", "Forest", "Island", "Island"])] {
        let mut gy = vec![titan];
        gy.extend(["Island"; 10]);
        let mut g = scen()
            .battlefield(0, &lands)
            .battlefield(0, &lands)
            .battlefield(0, &lands)
            .battlefield(0, &["Swamp", "Plains", "Swamp"])
            .graveyard(0, &gy)
            .hand(0, &["Vindicate"])
            .at(Step::Main1, 0)
            .start();
        let t = g.card(0, titan, Zone::Graveyard);
        cast_from_gy(&mut g, t, Some(player(1)));
        assert_eq!(g.obj(t).zone, Zone::Battlefield, "{titan}: escaped, it stays\n{}", log(&g));
        cast_at(&mut g, 0, "Vindicate", t);
        assert_eq!(g.obj(t).zone, Zone::Graveyard, "{titan}: destroyed, it goes to the graveyard");
        assert!(g.has_option(Choice::Cast(t)), "{titan}: escape again: {}", opts(&g));
    }
}

#[test]
fn retraced_spells_resolved_or_countered_return_to_the_graveyard_and_can_be_retraced_again_cr_702_81a() {
    // ruling: a276cb65 0bcbef41
    // Six's retrace: a Test Cat cast from the graveyard (a land discarded)
    // is countered, goes back and is retraced again. Wrenn and Six's
    // emblem: a Lightning Bolt retraced resolves and is retraced again.
    let mut g = scen()
        .battlefield(0, &["Six", "Plains", "Plains", "Plains", "Plains"])
        .graveyard(0, &["Test Cat"])
        .hand(0, &["Island", "Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let cat = g.card(0, "Test Cat", Zone::Graveyard);
    g.choose(Choice::Cast(cat));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 || !at_priority(g) && g.kind() != Some(DecisionKind::Priority));
    while g.pending_decision().unwrap().player == 0 && !at_priority(&g) {
        let c = first_non_pass(&g).unwrap();
        g.choose(c);
        g.drive_until(|g| at_priority(g) && g.turn().priority == 1 || !at_priority(g));
    }
    assert_eq!(g.count_in(0, "Island", Zone::Hand), 1, "a land discarded\n{}", log(&g));
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(cat);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(cat).zone, Zone::Graveyard, "countered: back in the graveyard\n{}", log(&g));
    assert!(g.has_option(Choice::Cast(cat)), "retrace again: {}", opts(&g));
    cast_from_gy(&mut g, cat, None);
    assert_eq!(g.obj(cat).zone, Zone::Battlefield);
    // Wrenn and Six's emblem.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .emblem(0, "Wrenn and Six")
        .graveyard(0, &["Lightning Bolt"])
        .hand(0, &["Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    cast_from_gy(&mut g, bolt, Some(player(1)));
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "resolved: back in the graveyard");
    assert!(g.has_option(Choice::Cast(bolt)), "{}", opts(&g));
    cast_from_gy(&mut g, bolt, Some(player(1)));
    assert_eq!(g.life(1), 14);
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 2);
}

// ---- face-up exile (CR 406.3) ----------------------------------------------------------------------

#[test]
fn robber_laelia_and_cascade_exile_face_up_for_both_players_to_see_cr_406_3() {
    // ruling: c92140df 551aa933 d4aa6557
    for source in ["Robber of the Rich", "Laelia, the Blade Reforged"] {
        let (g, c) = exiled_in_combat(source, "Lightning Bolt", 0);
        assert_eq!(g.obj(c).zone, Zone::Exile, "{source}: face up");
        assert!(g.knows(0, c) && g.knows(1, c), "{source}: both players see it");
    }
    // Bloodbraid Challenger's cascade.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Forest", "Test Cat", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger"])
        .at(Step::Main1, 0)
        .start();
    let lib = &g.player(0).library;
    let (island, forest) = (lib[lib.len() - 1], lib[lib.len() - 2]);
    let bc = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    g.choose(Choice::Cast(bc));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || (g.stack().is_empty() && at_priority(g)));
    // (The cascade trigger resolved up to its "may cast".)
    let ex: Vec<CardIdx> = g.exile_zone().to_vec();
    assert!(ex.contains(&island) && ex.contains(&forest), "{}", log(&g));
    for c in ex {
        assert!(g.knows(0, c) && g.knows(1, c), "{}: face up", g.name(c));
    }
}

#[test]
fn robbers_card_can_be_cast_after_robber_left_the_battlefield_cr_611_2a() {
    // ruling: 7d258f4b
    // Robber exiles P1's Preordain; P0 Bolts its own Robber in combat. In
    // the second main phase, combat over and Robber gone, P0 still casts the
    // exiled card (a turn it attacked with a Rogue).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Preordain", "Forest", "Forest"])
        .hand(1, &["Forest", "Forest"])
        .battlefield(0, &["Robber of the Rich", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let robber = bf(&g, 0, "Robber of the Rich");
    attack(&mut g, &[robber]);
    resolve(&mut g);
    let pre = g.card(1, "Preordain", Zone::Exile);
    cast_at(&mut g, 0, "Lightning Bolt", robber);
    assert_eq!(g.obj(robber).zone, Zone::Graveyard, "{}", log(&g));
    to_main2(&mut g);
    assert!(g.has_option(Choice::Cast(pre)), "Robber gone, combat over: {}", opts(&g));
    g.choose(Choice::Cast(pre));
    resolve(&mut g);
    assert_eq!(g.obj(pre).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- Ajani (CR 701.27c, 701.21a) -----------------------------------------------------------------

#[test]
fn a_single_faced_copy_of_ajani_exiled_to_return_transformed_stays_in_exile_cr_701_27c() {
    // ruling: e62216f3
    // Phyrexian Metamorph copies P1's Ajani, Nacatl Pariah (and makes a Cat
    // Warrior); the Cat Warrior dies: P0 exiles the Metamorph to return it
    // transformed, but it isn't a double-faced card: it stays in exile.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Mountain"])
        .hand(0, &["Phyrexian Metamorph", "Lightning Bolt"])
        .battlefield(1, &["Ajani, Nacatl Pariah // Ajani, Nacatl Avenger"])
        .at(Step::Main1, 0)
        .start();
    let ajani = bf(&g, 1, "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger");
    let pm = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(pm));
    let ta = Choice::Card(ajani);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayPhyrexian) => Some(Choice::Number(0)),
        _ if g.has_option(ta) => Some(ta),
        _ if g.has_option(g.tgt(ajani)) => Some(g.tgt(ajani)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(pm).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.name(pm).starts_with("Ajani, Nacatl Pariah"), "{}", log(&g));
    let cat = tokens_named(&g, "Cat Warrior")[0];
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(cat);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(pm).zone, Zone::Exile, "not a double-faced card: it stays in exile\n{}", log(&g));
}

#[test]
fn ajanis_minus_four_never_chooses_or_sacrifices_a_land_cr_701_21a() {
    // ruling: 18fa2a99
    // P1 animates Creeping Tar Pit in response to Ajani's -4: Tar Pit, a
    // land creature, can't be chosen as the creature and isn't sacrificed;
    // P1 keeps the Test Bear and one of its two artifacts.
    let mut g = scen()
        .battlefield(0, &["Ajani, Nacatl Pariah // Ajani, Nacatl Avenger"])
        .transformed(0, "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger")
        .counters(0, "Ajani, Nacatl Avenger", Counter::Loyalty, 4)
        .battlefield(1, &["Creeping Tar Pit", "Island", "Swamp", "Swamp", "Test Bear", "Sol Ring", "Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let ajani = bf(&g, 0, "Ajani, Nacatl Avenger");
    let tp = bf(&g, 1, "Creeping Tar Pit");
    let bear = bf(&g, 1, "Test Bear");
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(ab(ajani, 2));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    g.choose(ab(tp, 1));
    let tar_offered = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => {
            let d = g.pending_decision().unwrap();
            d.options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(tp))
        }
        Some(DecisionKind::ChoosePermanent) => {
            tar_offered.set(tar_offered.get() || g.has_option(Choice::Card(tp)));
            [ring, bear].into_iter().map(Choice::Card).find(|&c| g.has_option(c))
        }
        _ => None,
    });
    assert!(g.is_creature(tp), "animated\n{}", log(&g));
    assert!(!tar_offered.get(), "a land can't be chosen");
    assert_eq!(g.obj(tp).zone, Zone::Battlefield, "nor sacrificed\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(ring).zone, Zone::Battlefield);
    assert_eq!(g.count_in(1, "Mind Stone", Zone::Graveyard), 1);
}

// ---- small libraries -------------------------------------------------------------------------------

#[test]
fn lim_duls_vault_with_five_cards_shows_them_again_and_paying_nothing_shuffles_the_rest_cr_401_4() {
    // ruling: 2e75e441 89452e23
    // Five cards in P0's library: after paying 1 life the same five are
    // looked at again. With a bigger library and no life paid, the same
    // top five go back on top in the order chosen (the rest shuffled under
    // them).
    let lib = ["Lightning Bolt", "Island", "Island", "Island", "Island"];
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .hand(0, &["Lim-Dûl's Vault"])
        .battlefield(0, &["Underground Sea", "Underground Sea"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let v = g.card(0, "Lim-Dûl's Vault", Zone::Hand);
    g.choose(Choice::Cast(v));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
    g.choose(Choice::Yes);
    // (Putting them on the bottom in an order may be asked.)
    while g.kind() == Some(DecisionKind::PutOnBottom) {
        let c = first_non_pass(&g).unwrap();
        g.choose(c);
    }
    assert_eq!(g.life(0), 19);
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "again: {}", log(&g));
    g.choose(Choice::Yes);
    while g.kind() == Some(DecisionKind::PutOnBottom) {
        let c = first_non_pass(&g).unwrap();
        g.choose(c);
    }
    assert_eq!(g.life(0), 18, "as many times as P0 likes");
    g.choose(Choice::Pass);
    while g.kind() == Some(DecisionKind::PutOnTop) {
        let c = *g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect::<Vec<_>>().iter().find(|&&c| c != Choice::Card(bolt)).unwrap_or(&Choice::Card(bolt));
        g.choose(c);
    }
    g.drive_until(at_priority);
    assert_eq!(g.player(0).library.len(), 5);
    assert_eq!(*g.player(0).library.last().unwrap(), bolt, "the same five, the Bolt on top");
    // No life paid: the top five stay on top, in the chosen order.
    let mut lib = vec!["Island", "Island", "Lightning Bolt", "Island", "Island"];
    lib.extend(["Forest"; 10]);
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .hand(0, &["Lim-Dûl's Vault"])
        .battlefield(0, &["Underground Sea", "Underground Sea"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let v = g.card(0, "Lim-Dûl's Vault", Zone::Hand);
    g.choose(Choice::Cast(v));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Pass);
    while g.kind() == Some(DecisionKind::PutOnTop) {
        let c = *g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect::<Vec<_>>().iter().find(|&&c| c != Choice::Card(bolt)).unwrap_or(&Choice::Card(bolt));
        g.choose(c);
    }
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 20);
    let l = &g.player(0).library;
    assert_eq!(*l.last().unwrap(), bolt, "the Bolt on top\n{}", log(&g));
    assert!(l[l.len() - 5..].iter().all(|&c| g.name(c) != "Forest"), "the same five on top");
}

#[test]
fn expressive_iteration_with_two_cards_does_what_it_can_in_order_cr_609_3() {
    // ruling: 96109749
    // Two cards: one to the hand, one to the bottom, none exiled.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Lightning Bolt", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Mountain"])
        .hand(0, &["Expressive Iteration"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let it = g.card(0, "Expressive Iteration", Zone::Hand);
    g.choose(Choice::Cast(it));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand) if g.has_option(Choice::Card(bolt)) => Some(Choice::Card(bolt)),
        _ => None,
    });
    assert_eq!(g.obj(bolt).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.player(0).library.len(), 1, "the other on the bottom");
    assert!(g.exile_zone().is_empty(), "nothing exiled");
}

#[test]
fn ragavan_makes_its_treasure_even_with_nothing_to_exile_cr_609_3() {
    // ruling: 421327a3
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .battlefield(0, &["Ragavan, Nimble Pilferer"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let r = bf(&g, 0, "Ragavan, Nimble Pilferer");
    attack(&mut g, &[r]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 18);
    assert_eq!(tokens_named(&g, "Treasure").len(), 1, "{}", log(&g));
    assert!(g.exile_zone().is_empty());
}
