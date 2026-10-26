//! M8d part 3 (ENGINE.md 39): the cube's protection, targeting and
//! planeswalker rulings left after the triage was narrowed: effects that
//! don't target (CR 702.16b: Balance, overload, voting), protection from a
//! player on a reanimated creature and the Aura that can't attach (303.4d,
//! 704.5m), protection from everything (702.16j), modes that need a legal
//! target (700.2a), several wards (702.21a). `ruling: <hash8>` names the
//! ruling (docs/rulings.md).

mod common;
mod rulings_util;

use engine::db::Counter;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

/// P1 casts True-Name Nemesis choosing P0 (protection from P0); back to
/// P0's main phase.
fn p1_nemesis(g: &mut Game) -> CardIdx {
    let tnn = g.card(1, "True-Name Nemesis", Zone::Hand);
    g.choose(Choice::Cast(tnn));
    resolve_with(g, |g| g.has_option(player(0)).then(|| player(0)));
    assert_eq!(g.obj(tnn).chosen_player, Some(0), "protection from P0\n{}", log(g));
    quiet_until(g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    tnn
}

// ---- effects that don't target (CR 702.16b) ----------------------------------------------------

#[test]
fn balance_overload_and_votes_reach_a_creature_with_protection_from_their_player_cr_702_16b() {
    // ruling: 408d5ec3 c1c20030 f5cbaa26 7297f704
    // P1's True-Name Nemesis (protection from P0): P0's Balance (P0 has no
    // creatures) makes P1 sacrifice it; Damn and Winds of Abandon cast for
    // their overload costs (no targets) destroy and exile it; Council's
    // Judgment's vote exiles it.
    let base = |spells: &[&str], lands: &[&str]| {
        scen()
            .battlefield(0, lands)
            .hand(0, spells)
            .battlefield(1, &["Island", "Island", "Island"])
            .hand(1, &["True-Name Nemesis"])
            .at(Step::Main1, 1)
            .start()
    };
    let mut g = base(&["Balance"], &["Plains", "Plains"]);
    let tnn = p1_nemesis(&mut g);
    let b = g.card(0, "Balance", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Sacrifice) || g.kind() == Some(DecisionKind::ChoosePermanent)).then(|| first_non_pass(g)).flatten());
    assert_eq!(g.obj(tnn).zone, Zone::Graveyard, "Balance\n{}", log(&g));
    for (spell, zone) in [("Damn", Zone::Graveyard), ("Winds of Abandon", Zone::Exile)] {
        let mut g = base(&[spell], &["Plains"; 6]);
        let tnn = p1_nemesis(&mut g);
        let c = g.card(0, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        let mut targeted = false;
        for _ in 0..50 {
            if at_priority(&g) && g.stack().is_empty() {
                break;
            }
            let d = g.pending_decision().unwrap();
            let ch = match d.kind {
                DecisionKind::ChooseAltCost => Choice::Number(1),
                DecisionKind::Target => {
                    targeted = true;
                    first_non_pass(&g).unwrap_or(Choice::Pass)
                }
                DecisionKind::Priority => Choice::Pass,
                _ => first_non_pass(&g).unwrap_or(d.options[0].choice),
            };
            g.choose(ch);
        }
        assert!(!targeted, "{spell}: overloaded, no target");
        assert_eq!(g.obj(tnn).zone, zone, "{spell}\n{}", log(&g));
    }
    let mut g = base(&["Council's Judgment"], &["Plains"; 3]);
    let tnn = p1_nemesis(&mut g);
    let c = g.card(0, "Council's Judgment", Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve_with(&mut g, |g| (g.pending_decision().unwrap().options.iter().any(|o| o.choice == Choice::Card(tnn))).then_some(Choice::Card(tnn)));
    assert_eq!(g.obj(tnn).zone, Zone::Exile, "voted for\n{}", log(&g));
}

#[test]
fn a_reanimated_nemesis_with_protection_from_its_controller_cant_hold_the_aura_cr_303_4d_704_5m() {
    // ruling: 2d1b2d97 e3ce0176 9b32972a
    // True-Name Nemesis in P0's graveyard is a legal target for Animate
    // Dead and Necromancy (protection works on the battlefield). Returned,
    // P0 chooses itself: protection from P0 means P0's Aura can't enchant
    // it: the Aura goes to the graveyard and its leave trigger sacrifices
    // the Nemesis.
    for aura in ["Animate Dead", "Necromancy"] {
        let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).graveyard(0, &["True-Name Nemesis"]).hand(0, &[aura]).at(Step::Main1, 0).start();
        let tnn = g.card(0, "True-Name Nemesis", Zone::Graveyard);
        let a = g.card(0, aura, Zone::Hand);
        let t = g.tgt(tnn);
        g.choose(Choice::Cast(a));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(t) => Some(t),
            _ if g.has_option(player(0)) => Some(player(0)),
            _ => None,
        });
        assert!(log(&g).contains("chosen player: P0"));
        assert_eq!(g.obj(a).zone, Zone::Graveyard, "{aura}: can't be attached\n{}", log(&g));
        assert_eq!(g.obj(tnn).zone, Zone::Graveyard, "{aura}: sacrificed\n{}", log(&g));
    }
}

#[test]
fn necromancy_answers_emrakuls_shuffle_and_enchants_it_despite_protection_from_colored_spells_cr_702_16() {
    // ruling: 075cc040
    // Entomb puts Emrakul into the graveyard; with its shuffle trigger
    // waiting, Necromancy (flash) returns it: the black Aura (a permanent,
    // not a spell) can enchant Emrakul.
    let mut g = scen()
        .battlefield(0, &["Swamp"; 4])
        .hand(0, &["Entomb", "Necromancy"])
        .library(0, &["Emrakul, the Aeons Torn", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let en = g.card(0, "Entomb", Zone::Hand);
    let em = g.card(0, "Emrakul, the Aeons Torn", Zone::Library);
    g.choose(Choice::Cast(en));
    quiet_until_with(&mut g, |g| g.obj(em).zone == Zone::Graveyard && at_priority(g) && g.turn().priority == 0 && !g.stack().is_empty(), |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => Some(Choice::Def(id("Emrakul, the Aeons Torn"))),
        _ => None,
    });
    let nec = g.card(0, "Necromancy", Zone::Hand);
    let t = g.tgt(em);
    g.choose(Choice::Cast(nec));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    let em = g.card(0, "Emrakul, the Aeons Torn", Zone::Battlefield);
    assert_eq!(g.obj(nec).attached.map(|r| r.idx), Some(em), "{}", log(&g));
}

// ---- protection from everything (CR 702.16j) ----------------------------------------------------

#[test]
fn protection_from_everything_and_from_instants_stop_only_what_they_name_cr_702_16() {
    // ruling: 98b8d4c4 33f1a3d1 b0eae858 7b7445e2
    // Hexdrinker at level 8: Lightning Bolt can't target it, no blocks;
    // Winds of Abandon overloaded still exiles it. At level 3 (protection
    // from instants): Bolt can't target it, Chain Lightning (a sorcery) can.
    // The One Ring's protection: P1's creature attacks P0 and its damage is
    // prevented; P1's Sheoldred's Edict (no target) still makes P0
    // sacrifice.
    let level = Counter::Named(common::db().subtype_id("counter:level").unwrap());
    let mut g = scen()
        .battlefield(0, &["Hexdrinker"])
        .counters(0, "Hexdrinker", level, 8)
        .battlefield(1, &["Mountain", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains"])
        .hand(1, &["Lightning Bolt", "Winds of Abandon"])
        .at(Step::Main1, 1)
        .start();
    let hd = bf(&g, 0, "Hexdrinker");
    assert_eq!(g.pt(hd), (6, 6));
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(hd)), "protection from everything");
    g.choose(player(0));
    resolve(&mut g);
    let w = g.card(1, "Winds of Abandon", Zone::Hand);
    g.choose(Choice::Cast(w));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        _ => None,
    });
    assert_eq!(g.obj(hd).zone, Zone::Exile, "not targeted: exiled\n{}", log(&g));
    let mut g = scen()
        .battlefield(0, &["Hexdrinker"])
        .counters(0, "Hexdrinker", level, 3)
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Chain Lightning"])
        .at(Step::Main1, 1)
        .start();
    let hd = bf(&g, 0, "Hexdrinker");
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(hd)), "protection from instants");
    g.choose(player(0));
    resolve(&mut g);
    let c = g.card(1, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(hd)), "a sorcery may target it");
    // The One Ring.
    let mut g = scen()
        .battlefield(0, &["Mox Pearl", "Mox Pearl", "Mox Pearl", "Mox Pearl", "Test Bear"])
        .hand(0, &["The One Ring"])
        .battlefield(1, &["Test Giant", "Swamp", "Swamp"])
        .hand(1, &["Sheoldred's Edict"])
        .at(Step::Main1, 0)
        .start();
    let ring = g.card(0, "The One Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    resolve(&mut g);
    let bear = bf(&g, 0, "Test Bear");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let giant = bf(&g, 1, "Test Giant");
    g.choose(Choice::Card(giant));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(0));
    }
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(0), 20, "combat damage prevented\n{}", log(&g));
    let se = g.card(1, "Sheoldred's Edict", Zone::Hand);
    g.choose(Choice::Cast(se));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(0)));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "an edict doesn't target\n{}", log(&g));
}

// ---- modes and targets (CR 700.2a, 115.3) -------------------------------------------------------

#[test]
fn a_mode_needing_a_target_is_offered_only_with_one_and_may_repeat_it_cr_700_2a() {
    // ruling: 4cdbead7 ff410448 0bcfa967
    // With no artifact or creature of P0's, Three Steps Ahead doesn't offer
    // its copy mode; with no artifact anywhere, Kolaghan's Command and Fiery
    // Confluence don't offer "destroy target artifact"; with one Sol Ring,
    // Fiery Confluence may destroy it twice (the same target).
    let mut g = scen().battlefield(0, &["Island"; 6]).hand(0, &["Three Steps Ahead"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(!g.has_option(Choice::Number(1)), "no artifact or creature of yours: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Mountain"; 4]).battlefield(0, &["Swamp"]).hand(0, &["Kolaghan's Command", "Fiery Confluence"]).at(Step::Main1, 0).start();
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(kc));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(!g.has_option(Choice::Number(2)), "no artifact: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Mountain"; 4]).hand(0, &["Fiery Confluence"]).battlefield(1, &["Sol Ring"]).at(Step::Main1, 0).start();
    let ring = bf(&g, 1, "Sol Ring");
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    assert!(g.has_option(Choice::Number(2)));
    for _ in 0..3 {
        g.choose(Choice::Number(2));
    }
    for _ in 0..3 {
        assert_eq!(g.kind(), Some(DecisionKind::Target), "each instance targets: {}", opts(&g));
        g.choose(g.tgt(ring));
    }
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- ward (CR 702.21a) -------------------------------------------------------------------------

#[test]
fn two_wards_each_trigger_and_one_unpaid_counters_and_abrupt_decay_ignores_ward_cr_702_21a() {
    // ruling: e50427e3 dbbd77b1
    // P0's Kolaghan's Command at P1's Kappa Cannoneer (ward {4}: destroy
    // the artifact) and Sedgemoor Witch (ward - pay 3 life: 2 damage): both
    // wards trigger; P0 pays the life but not {4}: the spell is countered.
    // Abrupt Decay at the Witch, ward unpaid: it can't be countered, the
    // Witch is destroyed.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Swamp"])
        .hand(0, &["Kolaghan's Command"])
        .battlefield(1, &["Kappa Cannoneer", "Sedgemoor Witch"])
        .at(Step::Main1, 0)
        .start();
    let kc = bf(&g, 1, "Kappa Cannoneer");
    let sw = bf(&g, 1, "Sedgemoor Witch");
    let cmd = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(cmd));
    let m = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            m.set(m.get() + 1);
            Some(Choice::Number(if m.get() == 1 { 2 } else { 3 }))
        }
        Some(DecisionKind::Target) if g.has_option(g.tgt(kc)) && m.get() <= 2 && !g.has_option(g.tgt(sw)) => Some(g.tgt(kc)),
        Some(DecisionKind::Target) if g.has_option(g.tgt(sw)) => Some(g.tgt(sw)),
        Some(DecisionKind::Target) => first_non_pass(g),
        Some(DecisionKind::YesNo) => Some(if g.has_option(Choice::Yes) { Choice::Yes } else { Choice::Pass }),
        _ => None,
    });
    let l = log(&g);
    assert!(l.contains("trigger of Kappa Cannoneer goes on the stack") && l.contains("trigger of Sedgemoor Witch goes on the stack"), "both wards\n{l}");
    assert!(l.contains("P0 pays 3 life") && l.contains("counters Kolaghan's Command"), "{l}");
    assert_eq!(g.obj(cmd).zone, Zone::Graveyard);
    assert_eq!(g.obj(kc).zone, Zone::Battlefield, "countered: one ward unpaid\n{}", log(&g));
    assert_eq!(g.obj(sw).zone, Zone::Battlefield);
    let mut g = scen().battlefield(0, &["Bayou", "Bayou"]).hand(0, &["Abrupt Decay"]).battlefield(1, &["Sedgemoor Witch"]).at(Step::Main1, 0).start();
    let sw = bf(&g, 1, "Sedgemoor Witch");
    let ad = g.card(0, "Abrupt Decay", Zone::Hand);
    g.choose(Choice::Cast(ad));
    let t = g.tgt(sw);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(sw).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- small things: self-targets, channel needs a target ---------------------------------------

#[test]
fn luminarch_may_target_itself_and_channel_isnt_offered_without_a_target_cr_115_1_602_1() {
    // ruling: 0770cb81 5edd9028
    let mut g = scen().battlefield(0, &["Luminarch Aspirant"]).at(Step::Main1, 0).start();
    let la = bf(&g, 0, "Luminarch Aspirant");
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareAttackers || g.kind() == Some(DecisionKind::DeclareAttackers), |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(la)));
    assert_eq!(g.counters_on(la, Counter::P1P1), 1, "{}", log(&g));
    // Boseiju's and Otawara's channel with no target: not offered.
    let g = scen().battlefield(0, &["Forest", "Forest", "Island", "Island", "Island", "Island"]).hand(0, &["Boseiju, Who Endures", "Otawara, Soaring City"]).at(Step::Main1, 0).start();
    for land in ["Boseiju, Who Endures", "Otawara, Soaring City"] {
        let c = g.card(0, land, Zone::Hand);
        let offered = g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Ability(x, _) if x == c));
        assert!(!offered, "{land}: no target, no channel: {}", opts(&g));
    }
}
