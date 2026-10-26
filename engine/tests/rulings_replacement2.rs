//! M8d part 3 (ENGINE.md 39): the cube's replacement-effect rulings left
//! after the triage was narrowed: ravenous and copies (614.1c, 707.2),
//! reconfigure and Equipment that become creatures (702.151, 301.5c,
//! 704.5n), "enters tapped" against a condition (614.12), token doubling
//! (Elspeth, Storm Slayer, 614.1a) with "create and attach" and "tapped and
//! attacking" tokens, an Aura returned without being cast (303.4f), draws
//! replaced or disallowed (Narset, Leovold, Hullbreacher, 614.6, 121.2),
//! flashback against another destination (702.34a), finality counters
//! (122.1), Aura rules for Necromancy (704.5m), bestow (702.103), Sword of
//! the Meek (603.2, 603.6a), Mawloc's exile, Tamiyo, Collector of Tales.
//! `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The `i`th activated ability of `c`.
fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

/// Answer the mana payment with first options.
fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Cast `c` with X = `x` and pay; back at priority with it on the stack.
fn cast_x(g: &mut Game, c: CardIdx, x: u8) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    pay(g);
}

/// Activate `c`'s ability `i` at `target`, pay and resolve everything.
fn activate_at(g: &mut Game, c: CardIdx, i: u8, target: CardIdx) {
    g.choose(ab(c, i));
    let t = g.tgt(target);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
}

const METAMORPH: &str = "Phyrexian Metamorph";

/// Cast the Metamorph from P0's hand copying `what`.
fn metamorph_copies(g: &mut Game, what: CardIdx) -> CardIdx {
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(what)));
    m
}

// ---- ravenous (CR 614.1c, 707.2) ---------------------------------------------------------------

#[test]
fn ravenous_counters_come_with_it_and_a_copy_of_it_gets_none_cr_614_1c_707_2() {
    // ruling: b2b3d893 a22e8f66 5ebd5143
    // Mawloc cast with X = 2 is a 4/4 as it enters: its counters are on it
    // while its enter abilities wait (CR 614.1c). A Phyrexian Metamorph
    // copying Mawloc, or Jacked Rabbit, enters with no counters: the copy
    // wasn't cast, so its X is 0 (707.2, 107.3m).
    let mut g = scen()
        .battlefield(0, &["Mountain", "Forest", "Island", "Island", "Island", "Island", "Island", "Plains"])
        .battlefield(1, &["Jacked Rabbit"])
        .counters(1, "Jacked Rabbit", Counter::P1P1, 3)
        .hand(0, &["Mawloc", METAMORPH])
        .start();
    let maw = g.card(0, "Mawloc", Zone::Hand);
    cast_x(&mut g, maw, 2);
    g.drive_until(|g| g.obj(maw).zone == Zone::Battlefield);
    assert_eq!(g.counters_on(maw, Counter::P1P1), 2, "{}", log(&g));
    assert_eq!(g.pt(maw), (4, 4));
    resolve(&mut g);
    let m = metamorph_copies(&mut g, maw);
    assert_eq!(g.name(m), "Mawloc");
    assert_eq!(g.counters_on(m, Counter::P1P1), 0, "{}", log(&g));
    assert_eq!(g.pt(m), (2, 2));
    // Jacked Rabbit (P1's, with three counters): the copy has none.
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &["Jacked Rabbit"]).counters(1, "Jacked Rabbit", Counter::P1P1, 3).hand(0, &[METAMORPH]).start();
    let jr = bf(&g, 1, "Jacked Rabbit");
    let hand = g.player(0).hand.len();
    let m = metamorph_copies(&mut g, jr);
    assert_eq!(g.counters_on(m, Counter::P1P1), 0);
    assert_eq!(g.pt(m), (1, 2));
    assert_eq!(g.player(0).hand.len(), hand - 1, "X is 0: no draw");
}

// ---- reconfigure, Equipment becoming a creature (CR 702.151, 301.5c, 704.5n) -------------------

#[test]
fn reconfiguring_lion_sash_onto_a_creature_unattaches_its_equipment_cr_702_151b_704_5n() {
    // ruling: a2560fad
    // Lightning Greaves equipped to Lion Sash (a creature); Lion Sash then
    // reconfigures onto the Bear: it isn't a creature, so the Greaves
    // become unattached (CR 704.5n) and stay on the battlefield.
    let mut g = scen().battlefield(0, &["Lion Sash", "Lightning Greaves", "Test Bear", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    let sash = bf(&g, 0, "Lion Sash");
    let greaves = bf(&g, 0, "Lightning Greaves");
    let bear = bf(&g, 0, "Test Bear");
    activate_at(&mut g, greaves, 0, sash);
    assert_eq!(g.obj(greaves).attached.map(|r| r.idx), Some(sash), "{}", log(&g));
    activate_at(&mut g, sash, 1, bear);
    assert_eq!(g.obj(sash).attached.map(|r| r.idx), Some(bear), "{}", log(&g));
    assert!(!g.is_creature(sash));
    assert_eq!(g.obj(greaves).zone, Zone::Battlefield);
    assert!(g.obj(greaves).attached.is_none(), "unattached from a noncreature\n{}", log(&g));
}

/// Raise P0's Does Machines to level 3 ({1}{U}, then {4}{U}) and go to
/// the beginning of combat, where its level-3 trigger asks for a target.
fn does_machines_level_three(g: &mut Game) -> CardIdx {
    let dm = bf(g, 0, "Does Machines");
    for _ in 0..2 {
        g.choose(ab(dm, 0));
        pay(g);
        resolve(g);
    }
    assert!(log(g).contains("becomes level 3"), "{}", log(g));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    dm
}

#[test]
fn an_attached_equipment_made_a_creature_by_does_machines_becomes_unattached_cr_301_5c_704_5n() {
    // ruling: 006c710e 576a8296
    // Does Machines's level 3 makes the attached Skullclamp a 0/0 Robot
    // artifact creature with three +1/+1 counters: an Equipment that's a
    // creature can't equip (CR 301.5c), so it becomes unattached (704.5n)
    // and its equip ability can't attach it again. Lion Sash, attached by
    // reconfigure, made a creature the same way, becomes unattached too
    // (ruling 576a8296).
    for (eq, reconfigure) in [("Skullclamp", false), ("Lion Sash", true)] {
        let mut g = scen()
            .battlefield(0, &["Does Machines", eq, "Test Bear"])
            .battlefield(0, &["Island"; 7])
            .battlefield(0, &["Plains"; 2])
            .at(Step::Main1, 0)
            .start();
        let e = bf(&g, 0, eq);
        let bear = bf(&g, 0, "Test Bear");
        activate_at(&mut g, e, if reconfigure { 1 } else { 0 }, bear);
        assert_eq!(g.obj(e).attached.map(|r| r.idx), Some(bear), "{eq}\n{}", log(&g));
        does_machines_level_three(&mut g);
        g.choose(g.tgt(e));
        resolve(&mut g);
        assert!(g.is_creature(e), "{eq}\n{}", log(&g));
        assert!(g.obj(e).attached.is_none(), "{eq}: unattached\n{}", log(&g));
        assert_eq!(g.pt(e), (3, 3), "{eq}: a 0/0 with three counters");
        if !reconfigure {
            g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
            assert!(!g.has_option(ab(e, 0)), "an Equipment creature can't equip: {}", opts(&g));
        }
    }
}

// ---- "enters tapped" against a condition (CR 614.12) ------------------------------------------

#[test]
fn archon_of_emeria_taps_an_opponents_fast_land_or_paid_shock_land_anyway_cr_614_12() {
    // ruling: e0fa70ce c355f025
    // Under P0's Archon of Emeria ("nonbasic lands your opponents control
    // enter tapped"), P1's Seachrome Coast with no other land enters tapped
    // though its condition is met; P1's Hallowed Fountain enters tapped
    // whether or not 2 life is paid.
    let mut g = scen().battlefield(0, &["Archon of Emeria"]).hand(1, &["Seachrome Coast"]).at(Step::Main1, 1).start();
    let sc = g.card(1, "Seachrome Coast", Zone::Hand);
    g.choose(Choice::Play(sc));
    resolve(&mut g);
    assert_eq!(g.obj(sc).zone, Zone::Battlefield);
    assert!(g.obj(sc).tapped, "{}", log(&g));
    let mut g = scen().battlefield(0, &["Archon of Emeria"]).hand(1, &["Hallowed Fountain"]).at(Step::Main1, 1).start();
    let hf = g.card(1, "Hallowed Fountain", Zone::Hand);
    g.choose(Choice::Play(hf));
    // Pay 2 life if asked.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.obj(hf).zone, Zone::Battlefield);
    assert!(g.obj(hf).tapped, "{}", log(&g));
}

// ---- token doubling (Elspeth, Storm Slayer; CR 614.1a) -----------------------------------------

#[test]
fn elspeth_doubles_adelines_token_and_both_enter_tapped_and_attacking_cr_614_1a_508_4() {
    // ruling: 18b42e53
    let mut g = scen().battlefield(0, &["Elspeth, Storm Slayer", "Adeline, Resplendent Cathar"]).at(Step::DeclareAttackers, 0).start();
    let ad = bf(&g, 0, "Adeline, Resplendent Cathar");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(ad));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    resolve(&mut g);
    let humans = tokens_named(&g, "Human");
    assert_eq!(humans.len(), 2, "doubled\n{}", log(&g));
    for h in humans {
        assert!(g.obj(h).tapped && g.obj(h).attacking, "{}", log(&g));
    }
}

#[test]
fn elspeth_doubles_a_create_and_attach_token_and_the_equipment_attaches_to_one_cr_614_1a_301_5() {
    // ruling: faade90a f9096d07 81bbe983
    // Glimmer Lens (For Mirrodin!) and Nettlecyst (living weapon) under
    // Elspeth: two tokens, the Equipment attached to one; the other
    // Phyrexian Germ is a 0/0 and dies. Cori-Steel Cutter's Monk (flurry)
    // doubled: two Monks, the Cutter may attach to one of them.
    for (eq, token, survivors) in [("Glimmer Lens", "Rebel", 2usize), ("Nettlecyst", "Phyrexian Germ", 1)] {
        let mut g = scen().battlefield(0, &["Elspeth, Storm Slayer"]).battlefield(0, &["Plains"; 3]).hand(0, &[eq]).start();
        let c = g.card(0, eq, Zone::Hand);
        g.choose(Choice::Cast(c));
        resolve(&mut g);
        let toks = tokens_named(&g, token);
        assert_eq!(toks.len(), survivors, "{eq}\n{}", log(&g));
        let att = g.obj(c).attached.map(|r| r.idx).expect("attached");
        assert!(toks.contains(&att));
        assert_eq!(toks.iter().filter(|&&t| g.obj(c).attached.map(|r| r.idx) == Some(t)).count(), 1);
    }
    let mut g = scen()
        .battlefield(0, &["Elspeth, Storm Slayer", "Cori-Steel Cutter", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .start();
    let cut = bf(&g, 0, "Cori-Steel Cutter");
    for _ in 0..2 {
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(b));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(player(1)),
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            _ => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        });
    }
    let monks = tokens_named(&g, "Monk");
    assert_eq!(monks.len(), 2, "{}", log(&g));
    let att = g.obj(cut).attached.map(|r| r.idx);
    assert!(att.is_some_and(|a| monks.contains(&a)), "attached to one of them\n{}", log(&g));
}

#[test]
fn elspeth_doubles_reflections_token_and_each_is_sacrificed_at_the_end_step_cr_614_1a_603_7() {
    // ruling: d1d85157
    let fable = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";
    let mut g = scen()
        .battlefield(0, &["Elspeth, Storm Slayer", fable, "Test Bear", "Mountain", "Mountain"])
        .transformed(0, fable)
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let bear = bf(&g, 0, "Test Bear");
    activate_at(&mut g, r, 0, bear);
    let copies: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == "Test Bear").collect();
    assert_eq!(copies.len(), 2, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(copies.iter().all(|c| !g.battlefield().contains(c)), "both sacrificed\n{}", log(&g));
}

// ---- an Aura returned without being cast (CR 303.4f) -------------------------------------------


#[test]
fn portable_hole_returning_animate_dead_lets_its_owner_choose_what_it_enchants_cr_303_4f() {
    // ruling: aff80611
    // P1's Animate Dead (on its reanimated Griselbrand) is exiled by P0's
    // Portable Hole: Griselbrand is sacrificed. P1 Vindicates the Hole:
    // Animate Dead returns, its owner chooses what it enchants (a creature
    // card in a graveyard, not targeted), and its ability returns that card.
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .hand(0, &["Portable Hole"])
        .battlefield(1, &["Swamp", "Swamp", "Scrubland", "Scrubland"])
        .graveyard(1, &["Griselbrand"])
        .hand(1, &["Animate Dead", "Vindicate"])
        .at(Step::Main1, 1)
        .start();
    let ad = g.card(1, "Animate Dead", Zone::Hand);
    let gris = g.card(1, "Griselbrand", Zone::Graveyard);
    let t = g.tgt(gris);
    cast_targets(&mut g, ad, &[t]);
    assert_eq!(g.obj(gris).zone, Zone::Battlefield, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let hole = g.card(0, "Portable Hole", Zone::Hand);
    let t = g.tgt(ad);
    cast_targets(&mut g, hole, &[t]);
    assert_eq!(g.obj(ad).zone, Zone::Exile, "{}", log(&g));
    let gris = g.card(1, "Griselbrand", Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let vin = g.card(1, "Vindicate", Zone::Hand);
    let t = g.tgt(hole);
    cast_targets(&mut g, vin, &[t]);
    let ad = g.card(1, "Animate Dead", Zone::Battlefield);
    assert_eq!(g.obj(gris).zone, Zone::Battlefield, "returned by Animate Dead again\n{}", log(&g));
    assert_eq!(g.obj(gris).controller, 1);
    assert_eq!(g.obj(ad).attached.map(|r| r.idx), Some(gris), "{}", log(&g));
}

// ---- Thieving Skydiver gone (CR 608.2b) --------------------------------------------------------

#[test]
fn thieving_skydiver_gone_before_its_trigger_resolves_leaves_the_equipment_where_it_is_cr_608_2b() {
    // ruling: bd583829
    // P0 casts Thieving Skydiver kicked (X = 1) at P1's Skullclamp, attached
    // to P1's Bear; P1 Bolts the Skydiver in response. P0 still gains
    // control of the Skullclamp, which stays attached to the Bear.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Thieving Skydiver"])
        .battlefield(1, &["Skullclamp", "Test Bear", "Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let clamp = bf(&g, 1, "Skullclamp");
    let bear = bf(&g, 1, "Test Bear");
    activate_at(&mut g, clamp, 0, bear);
    assert_eq!(g.obj(clamp).attached.map(|r| r.idx), Some(bear));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let sky = g.card(0, "Thieving Skydiver", Zone::Hand);
    g.choose(Choice::Cast(sky));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1)); // kicked
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Number(1)); // X = 1
    }
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(clamp));
    // P1 Bolts the Skydiver in response.
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    let sky = bf(&g, 0, "Thieving Skydiver");
    g.choose(Choice::Cast(bolt));
    let t = g.tgt(sky);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(sky).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(clamp).controller, 0, "control gained\n{}", log(&g));
    assert_eq!(g.obj(clamp).attached.map(|r| r.idx), Some(bear), "still on the Bear\n{}", log(&g));
}

// ---- draws replaced or disallowed (CR 614.6, 121.2) --------------------------------------------

#[test]
fn a_draw_hullbreacher_replaced_doesnt_count_for_narset_or_leovold_cr_614_6() {
    // ruling: 64a8b100 a137ea0d
    // P0 controls Narset (or Leovold) and Hullbreacher. P1 has drawn nothing
    // this turn and casts Ancestral Recall: each draw is replaced by a
    // Treasure for P0, so none of them happened and the limit never stops
    // the next (three Treasures, no card).
    for limiter in ["Narset, Parter of Veils", "Leovold, Emissary of Trest"] {
        let mut g = scen().battlefield(0, &[limiter, "Hullbreacher"]).battlefield(1, &["Island"]).hand(1, &["Ancestral Recall"]).at(Step::Main1, 1).start();
        let ar = g.card(1, "Ancestral Recall", Zone::Hand);
        cast_targets(&mut g, ar, &[player(1)]);
        assert_eq!(tokens_named(&g, "Treasure").len(), 3, "{limiter}\n{}", log(&g));
        assert!(g.player(1).hand.is_empty(), "{limiter}");
    }
}

#[test]
fn a_draw_narset_or_leovold_disallows_cant_be_replaced_by_hullbreacher_cr_614_6_121_2() {
    // ruling: 64a8b100 a137ea0d
    // P1 draws its card in its draw step (the first draw there: Hullbreacher
    // doesn't apply), then casts Ancestral Recall: Narset or Leovold
    // disallows the three draws, so Hullbreacher has nothing to replace.
    for limiter in ["Narset, Parter of Veils", "Leovold, Emissary of Trest"] {
        let mut g = scen().battlefield(0, &[limiter, "Hullbreacher"]).battlefield(1, &["Island"]).hand(1, &["Ancestral Recall"]).at(Step::Upkeep, 1).start();
        quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
        assert_eq!(g.player(1).hand.len(), 2, "{limiter}: the draw step's card");
        let ar = g.card(1, "Ancestral Recall", Zone::Hand);
        cast_targets(&mut g, ar, &[player(1)]);
        assert!(tokens_named(&g, "Treasure").is_empty(), "{limiter}\n{}", log(&g));
        assert_eq!(g.player(1).hand.len(), 1, "{limiter}");
    }
}

#[test]
fn leovold_an_optional_draw_is_taken_only_while_none_was_drawn_cr_121_2() {
    // ruling: f6790404
    // Under P0's Leovold, P1's "Test May Draw" ("When this enters, you may
    // draw a card"): with no card drawn this turn the draw is offered and
    // happens; after that, Ancestral Recall's three are ignored.
    let mut g = scen()
        .battlefield(0, &["Leovold, Emissary of Trest"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Test May Draw", "Ancestral Recall"])
        .at(Step::Main1, 1)
        .start();
    let md = g.card(1, "Test May Draw", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.player(1).hand.len(), 2, "drew one\n{}", log(&g));
    let ar = g.card(1, "Ancestral Recall", Zone::Hand);
    cast_targets(&mut g, ar, &[player(1)]);
    assert_eq!(g.player(1).hand.len(), 1, "none of the three\n{}", log(&g));
}

// ---- flashback against another destination (CR 702.34a) ---------------------------------------

#[test]
fn memory_lapse_on_a_flashed_back_spell_exiles_it_cr_702_34a() {
    // ruling: 9ded89e9
    // Memory Lapse puts the spell it counters on top of its owner's library
    // (a self-replacement), but flashback's "exile it instead of putting it
    // anywhere else" wins: P1's flashed-back Lingering Souls is exiled.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Memory Lapse"])
        .battlefield(1, &["Swamp", "Swamp"])
        .graveyard(1, &["Lingering Souls"])
        .at(Step::Main1, 1)
        .start();
    let ls = g.card(1, "Lingering Souls", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(ls)), "{}", opts(&g));
    g.choose(Choice::Cast(ls));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let ml = g.card(0, "Memory Lapse", Zone::Hand);
    g.choose(Choice::Cast(ml));
    let t = g.tgt(ls);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(ls).zone, Zone::Exile, "{}", log(&g));
    assert!(tokens_named(&g, "Spirit").is_empty());
}

// ---- finality counters (CR 122.1) --------------------------------------------------------------

fn finality() -> Counter {
    Counter::Named(db().subtype_id("counter:finality").unwrap())
}

#[test]
fn a_finality_counter_exiles_an_elk_and_lets_a_bounce_go_to_the_hand_cr_122_1_614_1a() {
    // ruling: 3d6c926a 8680e6f8
    // P1's Bear with a finality counter, turned into an Elk by Oko (it loses
    // its abilities; the counter isn't an ability) and destroyed: exiled.
    // Bounced by Petty Theft instead: it goes to its owner's hand.
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .counters(1, "Test Bear", finality(), 1)
        .at(Step::Main1, 0)
        .start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    let bear = bf(&g, 1, "Test Bear");
    let elk = (0..3u8).find(|&i| {
        g.has_option(ab(oko, i)) && {
            let mut h = g.clone();
            h.choose(ab(oko, i));
            h.drive_until(|h| h.kind() == Some(DecisionKind::Target) || at_priority(h));
            h.kind() == Some(DecisionKind::Target) && h.has_option(h.tgt(bear))
        }
    });
    activate_at(&mut g, oko, elk.expect("Oko's elk ability"), bear);
    assert_eq!(g.pt(bear), (3, 3), "an Elk\n{}", log(&g));
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    // Petty Theft returns it to its owner's hand.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Brazen Borrower // Petty Theft"])
        .battlefield(1, &["Test Bear"])
        .counters(1, "Test Bear", finality(), 1)
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    g.choose(Choice::CastFace(bb));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(bear).zone, Zone::Hand, "{}", log(&g));
}

// ---- Necromancy follows the Aura rules (CR 704.5m) ---------------------------------------------

#[test]
fn necromancy_is_an_aura_once_its_trigger_resolves_and_goes_when_its_creature_leaves_cr_704_5m() {
    // ruling: 3886f2e5
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Plains"])
        .hand(0, &["Necromancy", "Swords to Plowshares"])
        .graveyard(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let nec = g.card(0, "Necromancy", Zone::Hand);
    let giant = g.card(1, "Test Giant", Zone::Graveyard);
    let t = g.tgt(giant);
    cast_targets(&mut g, nec, &[t]);
    let giant = g.card(1, "Test Giant", Zone::Battlefield);
    assert_eq!(g.obj(nec).attached.map(|r| r.idx), Some(giant), "{}", log(&g));
    let stp = g.card(0, "Swords to Plowshares", Zone::Hand);
    let t = g.tgt(giant);
    cast_targets(&mut g, stp, &[t]);
    assert_eq!(g.obj(nec).zone, Zone::Graveyard, "an Aura attached to nothing (704.5m)\n{}", log(&g));
}

// ---- bestow (CR 702.103) -----------------------------------------------------------------------

#[test]
fn springheart_nantuko_put_onto_the_battlefield_is_an_enchantment_creature_cr_702_103_303_4() {
    // ruling: 3786a663
    let mut g = scen().battlefield(0, &["Swamp"; 3]).graveyard(0, &["Springheart Nantuko"]).hand(0, &["Reanimate"]).at(Step::Main1, 0).start();
    let sn = g.card(0, "Springheart Nantuko", Zone::Graveyard);
    let re = g.card(0, "Reanimate", Zone::Hand);
    let t = g.tgt(sn);
    cast_targets(&mut g, re, &[t]);
    assert_eq!(g.obj(sn).zone, Zone::Battlefield);
    assert!(g.is_creature(sn) && g.obj(sn).attached.is_none(), "{}", log(&g));
}

#[test]
fn a_bestowed_nantuko_whose_creature_leaves_stays_untapped_and_can_attack_cr_702_103f() {
    // ruling: 25014ba1 6a72e0e6
    // Bestowed on P0's Bear; on P1's turn the Bear is Bolted: Nantuko is an
    // enchantment creature again, untapped, and on P0's next turn it may
    // attack (under P0's control since the turn began, as an Aura before).
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Springheart Nantuko"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let sn = g.card(0, "Springheart Nantuko", Zone::Hand);
    g.choose(Choice::Cast(sn));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(sn).attached.map(|r| r.idx), Some(bear), "bestowed\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, b, &[t]);
    assert!(g.is_creature(sn) && g.obj(sn).attached.is_none(), "{}", log(&g));
    assert!(!g.obj(sn).tapped);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(sn)), "{}", opts(&g));
}

// ---- Sword of the Meek (CR 603.2, 603.6a) ------------------------------------------------------

#[test]
fn sword_of_the_meek_returns_unattached_when_its_creature_is_gone_cr_608_2b_301_5() {
    // ruling: dee45a6f
    // Llanowar Elves (a 1/1) enters: Sword of the Meek's trigger; P1 Bolts
    // the Elves in response. The Sword still returns, unattached.
    let mut g = scen()
        .battlefield(0, &["Forest"])
        .graveyard(0, &["Sword of the Meek"])
        .hand(0, &["Llanowar Elves"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let le = g.card(0, "Llanowar Elves", Zone::Hand);
    let sword = g.card(0, "Sword of the Meek", Zone::Graveyard);
    g.choose(Choice::Cast(le));
    pay(&mut g);
    g.drive_until(|g| g.obj(le).zone == Zone::Battlefield && !g.stack().is_empty() && at_priority(g) && g.turn().priority == 1);
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(le);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(le).zone, Zone::Graveyard);
    assert_eq!(g.obj(sword).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.obj(sword).attached.is_none());
}

#[test]
fn a_nettlecyst_germ_enters_as_a_0_0_so_sword_of_the_meek_doesnt_trigger_cr_603_6a() {
    // ruling: bc0bcd33
    // The Germ enters as a 0/0 and Nettlecyst is attached to it afterwards
    // (a 1/1 then): Sword of the Meek's "whenever a 1/1 creature you
    // control enters" saw a 0/0. The Germ survives (attached before
    // state-based actions).
    let mut g = scen().battlefield(0, &["Plains"; 3]).graveyard(0, &["Sword of the Meek"]).hand(0, &["Nettlecyst"]).at(Step::Main1, 0).start();
    let nc = g.card(0, "Nettlecyst", Zone::Hand);
    let sword = g.card(0, "Sword of the Meek", Zone::Graveyard);
    g.choose(Choice::Cast(nc));
    resolve(&mut g);
    let germ = tokens_named(&g, "Phyrexian Germ");
    assert_eq!(germ.len(), 1, "{}", log(&g));
    assert_eq!(g.pt(germ[0]), (1, 1));
    assert_eq!(g.obj(sword).zone, Zone::Graveyard, "no trigger for a 0/0\n{}", log(&g));
}

// ---- Mawloc's exile (CR 614.1a) ----------------------------------------------------------------

#[test]
fn mawlocs_fought_creature_is_exiled_when_it_dies_later_that_turn_cr_614_1a() {
    // ruling: e9268eaf
    // Mawloc (X = 0, a 2/2) fights P1's Test Trampler (4/4) and dies; the
    // Trampler, damaged, is Bolted later that turn: exiled, not put into the
    // graveyard, though Mawloc is gone.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Forest"])
        .hand(0, &["Mawloc", "Lightning Bolt"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let maw = g.card(0, "Mawloc", Zone::Hand);
    let tr = bf(&g, 1, "Test Trampler");
    cast_x(&mut g, maw, 0);
    let t = g.tgt(tr);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(maw).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(tr).damage, 2);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(tr).zone, Zone::Exile, "{}", log(&g));
}

// ---- Tamiyo, Collector of Tales -----------------------------------------------------------------

#[test]
fn tamiyo_stops_an_opponents_edict_and_discard_but_not_lethal_damage_cr_101_2() {
    // ruling: d6c4fd3b 348a2bf6
    let mut g = scen()
        .battlefield(0, &["Tamiyo, Collector of Tales", "Test Bear"])
        .hand(0, &["Island", "Lightning Bolt"])
        .battlefield(1, &["Swamp", "Swamp", "Swamp", "Mountain"])
        .hand(1, &["Sheoldred's Edict", "Thoughtseize", "Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let se = g.card(1, "Sheoldred's Edict", Zone::Hand);
    g.choose(Choice::Cast(se));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(0)));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "no sacrifice\n{}", log(&g));
    let ts = g.card(1, "Thoughtseize", Zone::Hand);
    cast_targets(&mut g, ts, &[player(0)]);
    assert_eq!(g.player(0).hand.len(), 2, "no discard\n{}", log(&g));
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "lethal damage isn't a sacrifice");
}
