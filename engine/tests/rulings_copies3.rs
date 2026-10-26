//! M8d part 3 (ENGINE.md 39): the cube's copy and layer rulings left after
//! the triage was narrowed: token copies and their copiable values (CR
//! 707.2, 707.9b; Ocelot Pride, Three Steps Ahead, Fractured Identity,
//! Esika's Chariot, Vaultborn Tyrant, Thundertrap Trainer), copies of
//! kicked, dashed and saddled permanents and spells (702.33, 702.109,
//! 702.171, 707.10), characteristic-defining abilities in every zone
//! (604.3), type, colour and P/T effects and their timestamps (613: Oko,
//! Painter's Servant, Does Machines and Tezzeret's emblem on a Vehicle,
//! Figure of Fable and Destiny, Badgermole Cub's earthbend), backup.
//! `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{types, AbilitySlot, Counter, Kw};
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

fn sub(name: &str) -> u16 {
    db().subtype_id(name).unwrap()
}

/// Activate `c`'s ability `i` at `target` and resolve everything.
fn activate_at(g: &mut Game, c: CardIdx, i: u8, target: CardIdx) {
    g.choose(ab(c, i));
    let t = g.tgt(target);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
}

/// Declare `attackers` (at the player) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(1));
        }
    }
    g.choose(Choice::Pass);
}

/// At an `OrderTriggers` decision, the option putting next the waiting
/// trigger of the deciding player whose source is named `name`.
fn first_trigger_from(g: &Game, name: &str) -> Option<Choice> {
    let d = g.pending_decision().unwrap();
    let mine: Vec<_> = g.waiting_triggers().iter().filter(|t| t.controller == d.player).collect();
    let k = mine.iter().position(|t| db().def(t.source_def).name == name)?;
    let c = Choice::Trigger(k as u8);
    g.has_option(c).then_some(c)
}

fn next_turn_main(g: &mut Game, p: u8) {
    let t = g.turn().number;
    quiet_until(g, |g| g.turn().number > t && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g));
}

const METAMORPH: &str = "Phyrexian Metamorph";
const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";

fn metamorph_copies(g: &mut Game, what: CardIdx) -> CardIdx {
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(what)));
    m
}

/// Mother of Runes (untapped, P0's) gives `c` protection from colour
/// `color` (0..4 = W U B R G).
fn mother(g: &mut Game, c: CardIdx, color: u8) {
    let m = bf(g, 0, "Mother of Runes");
    g.choose(ab(m, 0));
    let t = g.tgt(c);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(color)),
        _ => None,
    });
}

// ---- Guide of Souls, energy (CR 107.14, 611.2a) -------------------------------------------------

#[test]
fn guide_of_souls_angel_lasts_past_the_turn_and_the_guide_energy_stays_cr_611_2a_107_14() {
    // ruling: 253ce5ed feeffce5 d8a8c57b
    // Galvanic Discharge paying no energy: P0 still gets {E}{E}{E}, no
    // damage. The energy stays to P0's next turn (it isn't mana). Then
    // Guide of Souls's attack trigger pays it: the attacking Bear gets two
    // +1/+1 counters and a flying counter and becomes an Angel, which lasts
    // after the turn and after the Guide is gone.
    let mut g = scen()
        .battlefield(0, &["Guide of Souls", "Test Bear", "Mountain"])
        .hand(0, &["Galvanic Discharge"])
        .battlefield(1, &["Test Wall", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let wall = bf(&g, 1, "Test Wall");
    let gd = g.card(0, "Galvanic Discharge", Zone::Hand);
    g.choose(Choice::Cast(gd));
    let t = g.tgt(wall);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.player(0).energy, 3, "{}", log(&g));
    assert_eq!(g.obj(wall).damage, 0, "paid 0: no damage");
    next_turn_main(&mut g, 0);
    assert_eq!(g.player(0).energy, 3, "energy isn't emptied like mana");
    let bear = bf(&g, 0, "Test Bear");
    attack(&mut g, &[bear]);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.player(0).energy, 0, "{}", log(&g));
    assert!(g.has_subtype(bear, sub("Angel")) && g.has_kw(bear, Kw::Flying));
    // P1 Bolts the Guide; the Bear stays an Angel.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let guide = bf(&g, 0, "Guide of Souls");
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(guide);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(guide).zone, Zone::Graveyard);
    next_turn_main(&mut g, 0);
    assert!(g.has_subtype(bear, sub("Angel")) && g.has_kw(bear, Kw::Flying), "{}", log(&g));
    assert_eq!(g.pt(bear), (4, 4));
}

// ---- Ocelot Pride (CR 702.131, 707.2) ----------------------------------------------------------

#[test]
fn ocelot_pride_copies_a_copying_token_with_its_exceptions_untapped_and_its_enter_ability_cr_707_9b_702_131() {
    // ruling: c8005233 154a043c 8a378f1a 32a898bd
    // P0 (the city's blessing: ten permanents with Ocelot Pride) makes a
    // Reflection token of Test May Draw (with haste), attacks with it and
    // the Ocelot (lifelink: life gained). At the end step Ocelot creates a
    // Cat, then a copy of each token that entered this turn (none targeted):
    // a Test May Draw with haste (the Reflection token's copiable values),
    // untapped, whose enter ability triggers; it isn't sacrificed.
    let mut g = scen()
        .battlefield(0, &["Ocelot Pride", FABLE, "Test May Draw", "Mountain"])
        .battlefield(0, &["Plains"; 6])
        .transformed(0, FABLE)
        .at(Step::Main1, 0)
        .start();
    assert!(g.player(0).city_blessing, "ten permanents");
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let md = bf(&g, 0, "Test May Draw");
    g.choose(ab(r, 0));
    let t = g.tgt(md);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    let tok = *tokens_named(&g, "Test May Draw").first().expect("the Reflection's token");
    let oc = bf(&g, 0, "Ocelot Pride");
    attack(&mut g, &[oc, tok]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert!(g.obj(tok).tapped);
    let hand = g.player(0).hand.len();
    // The end step: Ocelot's trigger first (before the Reflection token is
    // sacrificed); draw with the copy's enter ability.
    quiet_until_with(
        &mut g,
        |g| g.turn().active == 1,
        |g| match g.kind() {
            // (The first put on the stack resolves last.)
            Some(DecisionKind::OrderTriggers) => first_trigger_from(g, "Reflection of Kiki-Jiki"),
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            _ => None,
        },
    );
    let copies = tokens_named(&g, "Test May Draw");
    assert_eq!(copies.len(), 1, "the Reflection token sacrificed, Ocelot's copy stays\n{}", log(&g));
    let c = copies[0];
    assert_ne!(c, tok);
    assert!(!g.obj(c).tapped && g.has_kw(c, Kw::Haste), "untapped, with the copiable haste\n{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand + 1, "its enter ability\n{}", log(&g));
    assert_eq!(tokens_named(&g, "Cat").len(), 2, "the Cat and its copy");
}

#[test]
fn ascend_gives_the_blessing_at_once_even_from_ocelots_own_cat_cr_702_131b() {
    // ruling: 94506897 0cbf6a07 f3760b65 08ffb162 63693660
    // Ten lands without an ascend permanent: no blessing. Life gained before
    // Ocelot Pride is cast (a lifelinker's combat damage) still counts at
    // the end step; nine permanents with Ocelot: its Cat is the tenth, the
    // blessing comes before the copy check, and the Cat is copied.
    let mut g = scen()
        .battlefield(0, &["Test Lifelinker"])
        .battlefield(0, &["Plains"; 7])
        .hand(0, &["Ocelot Pride"])
        .at(Step::Main1, 0)
        .start();
    let ll = bf(&g, 0, "Test Lifelinker");
    attack(&mut g, &[ll]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(0), 22);
    let oc = g.card(0, "Ocelot Pride", Zone::Hand);
    g.choose(Choice::Cast(oc));
    resolve(&mut g);
    assert!(!g.player(0).city_blessing, "nine permanents");
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(g.player(0).city_blessing, "{}", log(&g));
    assert_eq!(tokens_named(&g, "Cat").len(), 2, "the tenth permanent's blessing came first\n{}", log(&g));
    // Ten permanents without Ocelot: none; the tenth a legend that meets
    // the legend rule at once: the blessing all the same.
    let g = scen().battlefield(0, &["Plains"; 10]).start();
    assert!(!g.player(0).city_blessing);
    let mut g = scen()
        .battlefield(0, &["Ocelot Pride", "Test Legend"])
        .battlefield(0, &["Plains"; 7])
        .hand(0, &["Test Legend"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.player(0).city_blessing);
    let tl = g.card(0, "Test Legend", Zone::Hand);
    g.choose(Choice::Cast(tl));
    resolve(&mut g);
    assert_eq!(g.permanents_of(0).count(), 9, "the legend rule\n{}", log(&g));
    assert!(g.player(0).city_blessing, "kept\n{}", log(&g));
}

// ---- characteristic-defining abilities in every zone (CR 604.3) ---------------------------------

#[test]
fn power_defining_abilities_work_in_every_zone_and_count_types_not_cards_cr_604_3() {
    // ruling: c264835f fa9b77b4 754b54cb e3872b77 b2aa1e74
    // Adeline's power is the number of creatures you control, in hand too;
    // on the battlefield she counts herself. Barrowgoyf and Pyrogoyf count
    // card types among cards in all graveyards (an artifact creature card:
    // two; more such cards add nothing), in any zone.
    let mut g = scen().battlefield(0, &["Test Bear", "Plains", "Plains", "Plains"]).hand(0, &["Adeline, Resplendent Cathar"]).at(Step::Main1, 0).start();
    let ad = g.card(0, "Adeline, Resplendent Cathar", Zone::Hand);
    assert_eq!(g.pt(ad).0, 1, "in hand: one creature");
    g.choose(Choice::Cast(ad));
    resolve(&mut g);
    assert_eq!(g.pt(ad).0, 2, "herself too");
    // On the stack too: with three creatures, Adeline is a 3/4 spell, not
    // one Stern Scolding ("creature spell with power or toughness 2 or
    // less") can target.
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Test Bear", "Test Bear", "Plains", "Plains", "Plains"])
        .hand(0, &["Adeline, Resplendent Cathar"])
        .battlefield(1, &["Island"])
        .hand(1, &["Stern Scolding"])
        .at(Step::Main1, 0)
        .start();
    let ad = g.card(0, "Adeline, Resplendent Cathar", Zone::Hand);
    g.choose(Choice::Cast(ad));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.pt(ad), (3, 4), "{}", log(&g));
    let ss = g.card(1, "Stern Scolding", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(ss)), "no legal target: {}", opts(&g));
    for goyf in ["Barrowgoyf", "Pyrogoyf"] {
        let g = scen().graveyard(1, &["Walking Ballista"]).hand(0, &[goyf]).start();
        let c = g.card(0, goyf, Zone::Hand);
        let base = g.pt(c);
        let g2 = scen().graveyard(1, &["Walking Ballista"; 5]).graveyard(0, &["Phyrexian Metamorph"; 5]).hand(0, &[goyf]).start();
        let c2 = g2.card(0, goyf, Zone::Hand);
        assert_eq!(g2.pt(c2), base, "{goyf}: types, not cards");
        let g3 = scen().hand(0, &[goyf]).start();
        let c3 = g3.card(0, goyf, Zone::Hand);
        assert_eq!((base.0 - g3.pt(c3).0, base.1 - g3.pt(c3).1), (2, 2), "{goyf}: artifact and creature, in hand");
    }
}

// ---- Enduring Innocence (CR 205.1a, 707.2, 111.7) ----------------------------------------------

#[test]
fn enduring_innocence_returns_an_enchantment_without_creature_types_and_its_token_copy_doesnt_cr_205_1a_111_7() {
    // ruling: 0a655fc3 56a77c16 525efd15
    // Enduring Innocence dies: it returns, an enchantment, neither a Sheep
    // nor a Glimmer. A Phyrexian Metamorph copying it (an artifact too)
    // dies: it returns as an enchantment only (as it returns it may copy an
    // artifact or creature; Enduring Innocence, an enchantment now, isn't
    // one, so it is a Metamorph that is only an enchantment). A Three Steps
    // Ahead token copy of it dies: it doesn't return.
    let mut g = scen()
        .battlefield(0, &["Enduring Innocence", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Lightning Bolt", "Lightning Bolt", METAMORPH])
        .at(Step::Main1, 0)
        .start();
    let ei = bf(&g, 0, "Enduring Innocence");
    let m = metamorph_copies(&mut g, ei);
    assert_eq!(g.name(m), "Enduring Innocence");
    for target in [ei, m] {
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        let t = g.tgt(target);
        g.choose(Choice::Cast(b));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(t),
            Some(DecisionKind::ChoosePermanent) => {
                assert!(!g.has_option(Choice::Card(ei)), "not an artifact or a creature now");
                Some(Choice::Pass)
            }
            _ => None,
        });
        assert_eq!(g.obj(target).zone, Zone::Battlefield, "returned\n{}", log(&g));
        assert!(!g.is_creature(target) && g.is_type(target, types::ENCHANTMENT), "{}", log(&g));
        assert!(!g.is_type(target, types::ARTIFACT), "an enchantment, nothing else");
        assert!(!g.has_subtype(target, sub("Sheep")) && !g.has_subtype(target, sub("Glimmer")));
    }
    let mut g = scen()
        .battlefield(0, &["Enduring Innocence", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Island", "Island", "Island", "Island"])
        .hand(0, &["Three Steps Ahead", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ei = bf(&g, 0, "Enduring Innocence");
    let tsa = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(tsa));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(1)) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Pass),
        Some(DecisionKind::Target) => Some(g.tgt(ei)),
        _ => None,
    });
    let tok = tokens_named(&g, "Enduring Innocence")[0];
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(tok);
    cast_targets(&mut g, b, &[t]);
    assert!(!g.battlefield().contains(&tok) && tokens_named(&g, "Enduring Innocence").is_empty(), "{}", log(&g));
}

// ---- copies of kicked spells and permanents (CR 702.33, 707.10) --------------------------------

#[test]
fn a_copy_of_a_kicked_permanent_isnt_kicked_nor_a_reanimated_one_cr_702_33d_707_2() {
    // ruling: f4a1b9c8 fbef54b0
    // P0's Thieving Skydiver kicked (X = 1) takes P1's Sol Ring; a
    // Metamorph copying it isn't kicked: its enter ability doesn't trigger
    // (P1's Mox Pearl stays). Reanimated, the Skydiver isn't kicked either.
    let mut g = scen()
        .battlefield(0, &["Island"; 7])
        .hand(0, &["Thieving Skydiver", METAMORPH])
        .battlefield(1, &["Sol Ring", "Mox Pearl"])
        .at(Step::Main1, 0)
        .start();
    let sky = g.card(0, "Thieving Skydiver", Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    let pearl = bf(&g, 1, "Mox Pearl");
    g.choose(Choice::Cast(sky));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Number(1));
    }
    pay(&mut g);
    let t = g.tgt(ring);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(ring).controller, 0, "{}", log(&g));
    let m = metamorph_copies(&mut g, sky);
    assert_eq!(g.name(m), "Thieving Skydiver");
    assert_eq!(g.obj(pearl).controller, 1, "not kicked: no trigger\n{}", log(&g));
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &["Thieving Skydiver"]).hand(0, &["Reanimate"]).battlefield(1, &["Mox Pearl"]).at(Step::Main1, 0).start();
    let sky = g.card(0, "Thieving Skydiver", Zone::Graveyard);
    let re = g.card(0, "Reanimate", Zone::Hand);
    let t = g.tgt(sky);
    cast_targets(&mut g, re, &[t]);
    assert_eq!(g.obj(sky).zone, Zone::Battlefield);
    assert_eq!(g.obj(bf(&g, 1, "Mox Pearl")).controller, 1, "{}", log(&g));
}

/// Cast Lutri from P0's hand while a spell of P0's is on the stack; its
/// trigger copies that spell (answering `answer`), then everything resolves.
fn lutri(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    resolve_with(g, answer);
}

#[test]
fn lutris_copy_of_a_kicked_spell_is_kicked_cr_707_10_702_33() {
    // ruling: cef3bc5c 85680b80
    // Burst Lightning kicked (4 damage) at P1: Lutri's copy is kicked too.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 5])
        .battlefield(0, &["Island"; 3])
        .hand(0, &["Burst Lightning", "Lutri, the Spellchaser"])
        .at(Step::Main1, 0)
        .start();
    let bl = g.card(0, "Burst Lightning", Zone::Hand);
    g.choose(Choice::Cast(bl));
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::Target)));
    if g.kind() == Some(DecisionKind::ChooseCost) {
        g.choose(Choice::Number(1)); // kicked
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    lutri(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.stack().last().map(|_| ()).and(g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.life(1), 12, "4 and 4\n{}", log(&g));
    // Consult the Star Charts kicked: "put two of them into your hand"; the
    // copy, kicked, takes two as well.
    let mut g = scen()
        .battlefield(0, &["Island"; 7])
        .battlefield(0, &["Mountain"; 2])
        .hand(0, &["Consult the Star Charts", "Lutri, the Spellchaser"])
        .at(Step::Main1, 0)
        .start();
    let cs = g.card(0, "Consult the Star Charts", Zone::Hand);
    g.choose(Choice::Cast(cs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseCost) || at_priority(g));
    if g.kind() == Some(DecisionKind::ChooseCost) {
        g.choose(Choice::Number(1));
    }
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let hand = g.player(0).hand.len();
    lutri(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand - 1 + 4, "two and two (Lutri left the hand)\n{}", log(&g));
}

#[test]
fn lutris_copy_of_an_alternative_cost_spell_and_of_a_split_half_cr_707_10() {
    // ruling: b0b57c4a ae909e09
    // Baleful Mastery cast for {1}{B} ("an opponent draws a card"): the
    // copy resolves as though that cost was paid (P1 draws twice). A copy
    // of Death is Death.
    let mut g = scen()
        .battlefield(0, &["Swamp"; 2])
        .battlefield(0, &["Island", "Mountain", "Island"])
        .hand(0, &["Baleful Mastery", "Lutri, the Spellchaser"])
        .battlefield(1, &["Test Bear", "Test Wall"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let wall = bf(&g, 1, "Test Wall");
    let hand1 = g.player(1).hand.len();
    let bm = g.card(0, "Baleful Mastery", Zone::Hand);
    g.choose(Choice::Cast(bm));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::Target));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(1));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let tw = g.tgt(wall);
    lutri(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) if g.has_option(tw) => Some(tw),
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.obj(wall).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.player(1).hand.len(), hand1 + 2, "both paid the alternative cost\n{}", log(&g));
    // Death: "Return target creature card from your graveyard to the
    // battlefield. You lose 2 life."
    let ld = "Life // Death";
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Island", "Mountain", "Island"])
        .graveyard(0, &["Test Bear", "Test Wall"])
        .hand(0, &[ld, "Lutri, the Spellchaser"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, ld, Zone::Hand);
    let wall = g.card(0, "Test Wall", Zone::Graveyard);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.choose(Choice::CastFace(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let tw = g.tgt(wall);
    lutri(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) if g.has_option(tw) => Some(tw),
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(wall).zone, Zone::Battlefield, "the copy is Death\n{}", log(&g));
    assert_eq!(g.life(0), 16);
}

// ---- token copies: copiable values only (CR 707.2, 707.9b) -------------------------------------

#[test]
fn thundertraps_offspring_copies_none_of_the_trainers_protection_cr_707_2() {
    // ruling: eab6cd25
    // Thundertrap Trainer cast with offspring; with its offspring trigger
    // waiting, Mother of Runes gives the Trainer protection from red. The
    // 1/1 token copy has no protection (a non-copy effect).
    let mut g = scen()
        .battlefield(0, &["Mother of Runes"])
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Thundertrap Trainer"])
        .at(Step::Main1, 0)
        .start();
    let tt = g.card(0, "Thundertrap Trainer", Zone::Hand);
    g.choose(Choice::Cast(tt));
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) || at_priority(g));
    if matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) {
        g.choose(Choice::Number(1)); // offspring
    }
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers) || (g.obj(tt).zone == Zone::Battlefield && at_priority(g) && !g.stack().is_empty()));
    if g.kind() == Some(DecisionKind::OrderTriggers) {
        g.choose(Choice::Trigger(0));
        g.drive_until(|g| at_priority(g));
    }
    mother(&mut g, tt, 3);
    let toks = tokens_named(&g, "Thundertrap Trainer");
    assert_eq!(toks.len(), 1, "{}", log(&g));
    assert!(g.protection_of(tt).colors != 0, "the Trainer has it");
    assert_eq!(g.protection_of(toks[0]).colors, 0, "the token doesn't");
    assert_eq!(g.pt(toks[0]), (1, 1));
}

/// Cast Three Steps Ahead from P0's hand choosing only its copy mode, at
/// `target`; resolve, answering yes-or-no with `yes`.
fn three_steps_copy(g: &mut Game, target: CardIdx, yes: bool) -> Vec<CardIdx> {
    let before: Vec<CardIdx> = g.battlefield().to_vec();
    let tsa = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(tsa));
    let t = g.tgt(target);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(1)) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Pass),
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(if yes { Choice::Yes } else { Choice::Pass }),
        _ => None,
    });
    g.battlefield().iter().copied().filter(|c| !before.contains(c) && g.is_token(*c)).collect()
}

#[test]
fn three_steps_ahead_copies_whats_copiable_of_a_tapped_pumped_token_or_copy_cr_707_2_707_9b() {
    // ruling: b9b0e621 b045bc66 6c18a1f3 dcdbba23 f0054ad7
    // A tapped Bear with two +1/+1 counters, Skullclamp and Mother of
    // Runes's protection: the copy is an untapped plain 2/2. A Metamorph
    // copying a Bear: the copy is a Bear that is an artifact (the
    // Metamorph's copiable values). A Saheeli Servo: a Servo. Test May Draw:
    // its enter ability triggers for the token. Walking Ballista: X is 0, a
    // 0/0 that dies.
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Skullclamp", "Mother of Runes"])
        .battlefield(0, &["Island"; 8])
        .counters(0, "Test Bear", Counter::P1P1, 2)
        .hand(0, &["Three Steps Ahead"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let clamp = bf(&g, 0, "Skullclamp");
    activate_at(&mut g, clamp, 0, bear);
    mother(&mut g, bear, 3);
    let t = three_steps_copy(&mut g, bear, false);
    assert_eq!(t.len(), 1, "{}", log(&g));
    assert_eq!(g.pt(t[0]), (2, 2), "no counters, no Equipment\n{}", log(&g));
    assert!(!g.obj(t[0]).tapped && g.protection_of(t[0]).colors == 0);
    // A Metamorph copying a Bear.
    let mut g = scen().battlefield(0, &["Test Bear"]).battlefield(0, &["Island"; 8]).hand(0, &[METAMORPH, "Three Steps Ahead"]).at(Step::Main1, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    let m = metamorph_copies(&mut g, bear);
    let t = three_steps_copy(&mut g, m, false);
    assert_eq!(g.name(t[0]), "Test Bear");
    assert!(g.is_type(t[0], types::ARTIFACT), "the Metamorph's exception is copiable\n{}", log(&g));
    // Test May Draw: its own enter ability.
    let mut g = scen().battlefield(0, &["Test May Draw"]).battlefield(0, &["Island"; 4]).hand(0, &["Three Steps Ahead"]).at(Step::Main1, 0).start();
    let md = bf(&g, 0, "Test May Draw");
    let hand = g.player(0).hand.len();
    three_steps_copy(&mut g, md, true);
    assert_eq!(g.player(0).hand.len(), hand, "Three Steps Ahead left, the card drawn came\n{}", log(&g));
    // Walking Ballista with counters: the copy is a 0/0.
    let mut g = scen()
        .battlefield(0, &["Walking Ballista"])
        .counters(0, "Walking Ballista", Counter::P1P1, 3)
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Three Steps Ahead"])
        .at(Step::Main1, 0)
        .start();
    let wb = bf(&g, 0, "Walking Ballista");
    three_steps_copy(&mut g, wb, false);
    assert!(tokens_named(&g, "Walking Ballista").is_empty(), "X = 0: it dies\n{}", log(&g));
    // A Servo token (Saheeli's): a Servo.
    let mut g = scen()
        .battlefield(0, &["Saheeli, Sublime Artificer"])
        .battlefield(0, &["Island"; 5])
        .hand(0, &["Sol Ring", "Three Steps Ahead"])
        .at(Step::Main1, 0)
        .start();
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    resolve(&mut g);
    let servo = tokens_named(&g, "Servo")[0];
    let t = three_steps_copy(&mut g, servo, false);
    assert_eq!(g.name(t[0]), "Servo");
    assert_eq!(tokens_named(&g, "Servo").len(), 3, "Sol Ring's, Three Steps Ahead's (Saheeli) and the copy");
}

#[test]
fn fractured_identitys_tokens_copy_whats_copiable_cr_707_2_707_9b() {
    // ruling: 5361d9f5 c556a504 38a3327f 430af403 e92fa75a
    // P0's Fractured Identity on P1's permanents: a tapped Bear with
    // counters gives P0 a plain untapped 2/2; a Metamorph copying a Bear
    // gives an artifact Bear; a Reflection token copying Test May Draw gives
    // a Test May Draw with haste whose enter ability triggers; a Walking
    // Ballista a 0/0 (X is 0) that dies.
    let fi = |p1: &[&str], prep: &dyn Fn(&mut Game) -> CardIdx| -> (Game, Vec<CardIdx>) {
        let mut g = scen()
            .battlefield(1, p1)
            .counters(1, p1[0], Counter::P1P1, 2)
            .battlefield(0, &["Island", "Island", "Island", "Plains", "Plains"])
            .hand(0, &["Fractured Identity"])
            .battlefield(1, &["Island"; 4])
            .battlefield(1, &["Mountain"])
            .hand(1, &[METAMORPH])
            .at(Step::Main1, 1)
            .start();
        let target = prep(&mut g);
        quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
        let before: Vec<CardIdx> = g.battlefield().to_vec();
        let c = g.card(0, "Fractured Identity", Zone::Hand);
        let t = g.tgt(target);
        g.choose(Choice::Cast(c));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(t),
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            _ => None,
        });
        let made = g.battlefield().iter().copied().filter(|c| !before.contains(c) && g.is_token(*c)).collect();
        (g, made)
    };
    let (g, t) = fi(&["Test Bear"], &|g: &mut Game| {
        let b = bf(g, 1, "Test Bear");
        g.choose(Choice::Pass);
        b
    });
    assert_eq!(g.pt(t[0]), (2, 2));
    assert_eq!(g.obj(t[0]).controller, 0);
    let (g, t) = fi(&["Test Bear"], &|g: &mut Game| {
        let b = bf(g, 1, "Test Bear");
        let m = g.card(1, METAMORPH, Zone::Hand);
        g.choose(Choice::Cast(m));
        resolve_with(g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(b)));
        m
    });
    assert_eq!(g.name(t[0]), "Test Bear");
    assert!(g.is_type(t[0], types::ARTIFACT), "{}", log(&g));
    let (g, t) = fi(&["Walking Ballista"], &|g: &mut Game| {
        let b = bf(g, 1, "Walking Ballista");
        g.choose(Choice::Pass);
        b
    });
    assert!(t.iter().all(|c| !g.battlefield().contains(c)), "a 0/0\n{}", log(&g));
    // P0's own Reflection token of Test May Draw: P1 gets the copy, a Test
    // May Draw with haste, whose enter ability triggers (P1 draws).
    let mut g = scen()
        .battlefield(0, &[FABLE, "Test May Draw", "Mountain", "Island", "Island", "Island", "Plains", "Plains"])
        .transformed(0, FABLE)
        .hand(0, &["Fractured Identity"])
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let md = bf(&g, 0, "Test May Draw");
    g.choose(ab(r, 0));
    let t = g.tgt(md);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    let tok = tokens_named(&g, "Test May Draw")[0];
    let hand1 = g.player(1).hand.len();
    let c = g.card(0, "Fractured Identity", Zone::Hand);
    let t = g.tgt(tok);
    g.choose(Choice::Cast(c));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    let theirs: Vec<CardIdx> = tokens_named(&g, "Test May Draw").into_iter().filter(|&c| g.obj(c).controller == 1).collect();
    assert_eq!(theirs.len(), 1, "{}", log(&g));
    assert!(g.has_kw(theirs[0], Kw::Haste), "the Reflection's exception is copiable");
    assert_eq!(g.player(1).hand.len(), hand1 + 1, "its enter ability
{}", log(&g));
}

// ---- Vehicles made creatures (CR 301.7, 613.4b) -------------------------------------------------

#[test]
fn does_machines_and_tezzerets_emblem_make_a_copter_a_0_0_robot_that_crewing_doesnt_restore_cr_613_4b() {
    // ruling: 0a86d2b2 ce58981e a0398e1e ea843c44 8733df71 fc909d02 95070e2f 111530fb
    // Smuggler's Copter (not a creature) at the beginning of combat: three
    // +1/+1 counters, it becomes a 0/0 Robot artifact creature: a 3/3, still
    // a Vehicle with flying and its loot ability; crewing it doesn't bring
    // back its printed 3/3 (a 3/3 with the counters, not 6/6); it can attack
    // (controlled since the turn began). Does Machines (level 3) and
    // Tezzeret's emblem alike.
    for source in ["Does Machines", "emblem"] {
        let mut sc = scen().battlefield(0, &["Smuggler's Copter", "Test Bear"]).at(Step::Main1, 0);
        sc = if source == "emblem" {
            sc.emblem(0, "Tezzeret, Cruel Captain")
        } else {
            sc.battlefield(0, &["Does Machines"]).battlefield(0, &["Island"; 7])
        };
        let mut g = sc.start();
        let copter = bf(&g, 0, "Smuggler's Copter");
        if source == "Does Machines" {
            let dm = bf(&g, 0, "Does Machines");
            for _ in 0..2 {
                g.choose(ab(dm, 0));
                pay(&mut g);
                resolve(&mut g);
            }
        }
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(copter));
        resolve(&mut g);
        assert!(g.is_creature(copter), "{source}\n{}", log(&g));
        assert_eq!(g.pt(copter), (3, 3), "{source}: a 0/0 with three counters");
        assert!(g.has_subtype(copter, sub("Vehicle")), "{source}: still a Vehicle (CR 205.1b)");
        assert!(g.has_subtype(copter, sub("Robot")), "{source}: a Robot");
        assert!(g.is_type(copter, types::ARTIFACT));
        assert!(g.has_kw(copter, Kw::Flying), "{source}: keeps its abilities");
        // Crew it anyway: still a 3/3.
        let bear = bf(&g, 0, "Test Bear");
        if g.has_option(ab(copter, 0)) {
            g.choose(ab(copter, 0));
            resolve_with(&mut g, |g| (g.pending_decision().unwrap().options.iter().any(|o| o.choice == Choice::Card(bear))).then_some(Choice::Card(bear)));
            assert_eq!(g.pt(copter), (3, 3), "{source}: crewing doesn't restore 3/3\n{}", log(&g));
        }
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
        assert!(g.has_option(Choice::Card(copter)), "{source}: it may attack\n{}", opts(&g));
    }
}

#[test]
fn a_copy_of_a_crewed_copter_isnt_a_creature_and_a_crewed_copter_has_no_creature_type_cr_707_2_301_7() {
    // ruling: 1bccbeb4 65d7c435
    let mut g = scen().battlefield(0, &["Smuggler's Copter", "Test Bear"]).battlefield(0, &["Island"; 4]).hand(0, &[METAMORPH]).at(Step::Main1, 0).start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| (g.pending_decision().unwrap().options.iter().any(|o| o.choice == Choice::Card(bear))).then_some(Choice::Card(bear)));
    assert!(g.is_creature(copter), "{}", log(&g));
    assert!(!g.has_subtype(copter, sub("Robot")) && g.has_subtype(copter, sub("Vehicle")));
    let m = metamorph_copies(&mut g, copter);
    assert_eq!(g.name(m), "Smuggler's Copter");
    assert!(!g.is_creature(m), "the crew effect isn't copied\n{}", log(&g));
}

// ---- copies of saddled and dashed permanents (CR 702.171, 702.109) -----------------------------

#[test]
fn a_copy_of_a_saddled_bronco_isnt_saddled_cr_702_171b_707_2() {
    // ruling: 90fe76e8
    // Caustic Bronco saddled; a Metamorph copying it, given haste by
    // Lightning Greaves, attacks: not saddled, so P0 loses life equal to the
    // revealed card's mana value (a Lightning Bolt: 1).
    let mut g = scen()
        .battlefield(0, &["Caustic Bronco", "Test Giant", "Lightning Greaves"])
        .battlefield(0, &["Island"; 4])
        .library(0, &["Lightning Bolt", "Lightning Bolt", "Lightning Bolt"])
        .hand(0, &[METAMORPH])
        .at(Step::Main1, 0)
        .start();
    let bronco = bf(&g, 0, "Caustic Bronco");
    let giant = bf(&g, 0, "Test Giant");
    g.choose(ab(bronco, 0));
    resolve_with(&mut g, |g| (g.pending_decision().unwrap().options.iter().any(|o| o.choice == Choice::Card(giant))).then_some(Choice::Card(giant)));
    let m = metamorph_copies(&mut g, bronco);
    assert_eq!(g.name(m), "Caustic Bronco");
    let greaves = bf(&g, 0, "Lightning Greaves");
    activate_at(&mut g, greaves, 0, m);
    attack(&mut g, &[m]);
    resolve(&mut g);
    assert_eq!(g.life(0), 19, "the copy isn't saddled: its controller loses 1\n{}", log(&g));
    assert_eq!(g.life(1), 20);
}

#[test]
fn a_copy_of_a_dashed_ragavan_has_no_haste_and_isnt_returned_cr_702_109a_707_2() {
    // ruling: d7de1c3d
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Island", "Island", "Island", "Island"])
        .hand(0, &["Ragavan, Nimble Pilferer", METAMORPH])
        .at(Step::Main1, 0)
        .start();
    let rag = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    g.choose(Choice::Cast(rag));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(1)));
    assert!(g.has_kw(rag, Kw::Haste), "dashed\n{}", log(&g));
    // The Metamorph copies Ragavan; the legend rule: P0 keeps the copy.
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(rag)),
        Some(DecisionKind::LegendRule) => Some(Choice::Card(m)),
        _ => None,
    });
    assert_eq!(g.obj(rag).zone, Zone::Graveyard, "the legend rule
{}", log(&g));
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.has_kw(m, Kw::Haste), "the dash's haste isn't copied");
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "not returned to hand\n{}", log(&g));
}

// ---- backup (CR 702.165) ------------------------------------------------------------------------

#[test]
fn backup_grants_only_its_printed_ability_and_on_itself_just_the_counter_cr_702_165a() {
    // ruling: 5380460f d189cf68
    // Death-Greeter's Champion cast with Arena of Glory's mana has haste;
    // its backup on the Bear gives a +1/+1 counter and double strike, not
    // haste. On itself: a +1/+1 counter.
    let mut g = scen()
        .battlefield(0, &["Arena of Glory", "Mountain", "Mountain", "Test Bear"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Death-Greeter's Champion"])
        .at(Step::Main1, 0)
        .start();
    let arena = bf(&g, 0, "Arena of Glory");
    let bear = bf(&g, 0, "Test Bear");
    let dgc = g.card(0, "Death-Greeter's Champion", Zone::Hand);
    g.choose(Choice::Cast(dgc));
    g.drive_until(|g| (!at_priority(g) && g.has_option(ab(arena, 1))) || g.kind() == Some(DecisionKind::ChooseAltCost));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
        g.drive_until(|g| g.has_option(ab(arena, 1)));
    }
    g.choose(ab(arena, 1));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    resolve(&mut g);
    assert!(g.has_kw(dgc, Kw::Haste), "Arena's haste\n{}", log(&g));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert!(g.has_kw(bear, Kw::DoubleStrike), "backup's printed ability");
    assert!(!g.has_kw(bear, Kw::Haste), "not the gained haste");
    let mut g = scen().battlefield(0, &["Mountain"; 3]).hand(0, &["Death-Greeter's Champion"]).at(Step::Main1, 0).start();
    let dgc = g.card(0, "Death-Greeter's Champion", Zone::Hand);
    g.choose(Choice::Cast(dgc));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(dgc)),
        _ => None,
    });
    assert_eq!(g.counters_on(dgc, Counter::P1P1), 1, "{}", log(&g));
    assert_eq!(g.pt(dgc), (3, 2));
}

// ---- Oko's Elk (CR 613.1d, 613.4b, 611.2) -------------------------------------------------------

fn oko_elk(g: &mut Game, target: CardIdx) {
    let oko = bf(g, 0, "Oko, Thief of Crowns");
    activate_at(g, oko, 1, target);
}

#[test]
fn okos_elk_set_base_3_3_counters_and_later_abilities_still_apply_and_it_lasts_cr_613_4_611_2() {
    // ruling: b08d1e13 29633b54 31ae0455 0714a6b9 b9f6eb08 32bd2f57 35a42edc 13ea38a2
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", "Walking Ballista", "Lightning Greaves", "Celestial Colonnade"])
        .counters(0, "Walking Ballista", Counter::P1P1, 2)
        .battlefield(0, &["Island"; 4])
        .battlefield(0, &["Plains"; 2])
        .at(Step::Main1, 0)
        .start();
    let wb = bf(&g, 0, "Walking Ballista");
    oko_elk(&mut g, wb);
    // A 3/3 base and its two counters: 5/5; not an artifact any more.
    assert_eq!(g.pt(wb), (5, 5), "{}", log(&g));
    assert!(!g.is_type(wb, types::ARTIFACT) && g.is_creature(wb));
    assert!(g.has_subtype(wb, sub("Elk")));
    // An ability gained later is kept: Lightning Greaves.
    let greaves = bf(&g, 0, "Lightning Greaves");
    activate_at(&mut g, greaves, 0, wb);
    assert!(g.has_kw(wb, Kw::Haste) && g.has_kw(wb, Kw::Shroud));
    // Lasts: next turn, Oko gone or not, still an Elk.
    next_turn_main(&mut g, 0);
    assert_eq!(g.pt(wb), (5, 5));
    assert!(g.has_subtype(wb, sub("Elk")));
    // A land creature until end of turn (Colonnade, a 4/4 Elemental),
    // elked: it stays a 3/3 Elk after the turn; Oko's later set overrides
    // the Colonnade's 4/4 (613.4b).
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", "Celestial Colonnade"])
        .battlefield(0, &["Island"; 3])
        .battlefield(0, &["Plains"; 2])
        .at(Step::Main1, 0)
        .start();
    let col = bf(&g, 0, "Celestial Colonnade");
    let a = (0..4u8).map(|i| ab(col, i)).find(|&a| g.has_option(a)).expect("the Colonnade's animation");
    g.choose(a);
    pay(&mut g);
    resolve(&mut g);
    assert_eq!(g.pt(col), (4, 4), "{}", log(&g));
    oko_elk(&mut g, col);
    assert_eq!(g.pt(col), (3, 3));
    assert!(!g.is_type(col, types::LAND), "an Elk creature only");
    next_turn_main(&mut g, 0);
    assert!(g.is_creature(col) && g.pt(col) == (3, 3), "the Colonnade's until-end-of-turn ended, the Elk didn't\n{}", log(&g));
    // A legendary creature keeps legendary.
    let mut g = scen().battlefield(0, &["Oko, Thief of Crowns", "Test Legend"]).at(Step::Main1, 0).start();
    let tl = bf(&g, 0, "Test Legend");
    oko_elk(&mut g, tl);
    assert!(g.is_legendary(tl), "{}", log(&g));
    // Marked damage: a 5/5 dealt 4 and elked dies (13ea38a2).
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(giant);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(giant).damage, 3);
    oko_elk(&mut g, giant);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "3 damage on a 3/3\n{}", log(&g));
}

#[test]
fn okos_exchange_lasts_though_the_creature_grows_later_cr_611_2a() {
    // ruling: ecd10cda
    // P0's Food for P1's Test Bear (power 2); a +1/+1 counter later (power
    // 3, still at most 3), Luminarch Aspirant's: P0 keeps it; next turn too.
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", "Luminarch Aspirant"])
        .counters(0, "Oko, Thief of Crowns", Counter::Loyalty, 8)
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    g.choose(ab(oko, 0));
    resolve(&mut g);
    let food = tokens_named(&g, "Food")[0];
    let bear = bf(&g, 1, "Test Bear");
    next_turn_main(&mut g, 0);
    g.choose(ab(oko, 2));
    let (tf, tb) = (g.tgt(food), g.tgt(bear));
    let i = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| {
        i.set(i.get() + 1);
        if g.has_option(tf) { tf } else { tb }
    }));
    assert_eq!(g.obj(bear).controller, 0, "{}", log(&g));
    next_turn_main(&mut g, 0);
    assert_eq!(g.obj(bear).controller, 0);
}

// ---- Badgermole Cub's earthbend (CR 701.65) ----------------------------------------------------

#[test]
fn earthbend_makes_a_colourless_land_creature_that_keeps_its_mana_ability_and_resets_to_0_0_cr_701_65a() {
    // ruling: a1b92c37 0decf500 28832e4a
    // Badgermole Cub earthbends 1 a Forest: a colourless 0/0 land creature
    // with haste and a +1/+1 counter, still a Forest that taps for {G}. A
    // second Cub earthbends it again: another counter, base 0/0 again (a
    // 2/2).
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Forest"]).hand(0, &["Badgermole Cub", "Badgermole Cub"]).at(Step::Main1, 0).start();
    let forest = bf(&g, 0, "Forest");
    for n in 1..=2u16 {
        let cub = g.card(0, "Badgermole Cub", Zone::Hand);
        g.choose(Choice::Cast(cub));
        let t = g.tgt(forest);
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
        assert!(g.is_creature(forest) && g.is_type(forest, types::LAND), "{}", log(&g));
        assert_eq!(g.counters_on(forest, Counter::P1P1), n);
        assert_eq!(g.pt(forest), (n as i32, n as i32));
        assert_eq!(g.colors_of(forest), 0, "colourless");
        assert!(g.has_subtype(forest, sub("Forest")) && g.has_kw(forest, Kw::Haste));
    }
}

// ---- eternalize's token (CR 702.129, 707.9b) ---------------------------------------------------

#[test]
fn an_eternalized_tokens_changes_are_copiable_cr_707_9b() {
    // ruling: 90ab1f80
    // Fanatic of Rhonas eternalized: a 4/4 black Zombie with no mana cost;
    // Three Steps Ahead's copy of the token is one too.
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Island", "Island", "Island", "Island"]).graveyard(0, &["Fanatic of Rhonas"]).hand(0, &["Three Steps Ahead"]).battlefield(1, &["Plains"]).hand(1, &["Prismatic Ending"]).at(Step::Main1, 0).start();
    let f = g.card(0, "Fanatic of Rhonas", Zone::Graveyard);
    let a = Choice::Ability(f, AbilitySlot::Graveyard(0));
    g.choose(a);
    pay(&mut g);
    resolve(&mut g);
    let tok = tokens_named(&g, "Fanatic of Rhonas")[0];
    let t = three_steps_copy(&mut g, tok, false);
    assert_eq!(t.len(), 1, "{}", log(&g));
    assert_eq!(g.pt(t[0]), (4, 4));
    assert_eq!(g.colors_of(t[0]), 1 << 2, "black");
    assert!(g.has_subtype(t[0], sub("Zombie")));
    // No mana cost (mana value 0): P1's Prismatic Ending with one colour
    // spent exiles it (the Fanatic card's mana value is 2).
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let pe = g.card(1, "Prismatic Ending", Zone::Hand);
    g.choose(Choice::Cast(pe));
    let tt = g.tgt(t[0]);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)), // X = 0: {W}, converge 1
        Some(DecisionKind::Target) => Some(tt),
        _ => None,
    });
    assert_eq!(g.obj(t[0]).zone, Zone::Nowhere, "exiled (a token ceases to exist)
{}", log(&g));
}

// ---- Painter's Servant (CR 105.3, 613.1e) ------------------------------------------------------

#[test]
fn painters_servant_adds_its_colour_cumulatively_and_a_later_colour_setting_wins_cr_105_3_613_7() {
    // ruling: 0d163622 1896f056 1095bb67 720f8bdd
    // Two Painter's Servants (blue, red): Sol Ring and Sowing Mycospawn
    // (devoid) are blue and red, not also colourless. Oko's Elk (green, a
    // later timestamp) is green only.
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Oko, Thief of Crowns"])
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Painter's Servant", "Painter's Servant", "Sowing Mycospawn"])
        .at(Step::Main1, 0)
        .start();
    for color in [1u8, 3] {
        let ps = g.card(0, "Painter's Servant", Zone::Hand);
        g.choose(Choice::Cast(ps));
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseColor)).then_some(Choice::Number(color)));
    }
    let ring = bf(&g, 0, "Sol Ring");
    let myco = g.card(0, "Sowing Mycospawn", Zone::Hand);
    let (blue, red, green) = (1u8 << 1, 1u8 << 3, 1u8 << 4);
    assert_eq!(g.colors_of(ring), blue | red, "{}", log(&g));
    assert_eq!(g.colors_of(myco), blue | red);
    oko_elk(&mut g, ring);
    assert_eq!(g.colors_of(ring), green, "the Elk's colour is set after the Servants'");
}

// ---- Vaultborn Tyrant (CR 707.2, 707.9b) -------------------------------------------------------

#[test]
fn vaultborn_tyrants_token_has_no_counters_and_a_copys_token_copies_the_copy_cr_707_2_707_9b() {
    // ruling: d02a995d c28e08d7 be7d384c
    // Vaultborn Tyrant with two +1/+1 counters dies: its token (an artifact
    // too) is a 6/6, and its own second ability triggers as it enters (gain
    // 3, draw). A Metamorph copying the Tyrant dies: the token copies the
    // Metamorph's copiable values (the Tyrant, an artifact).
    let mut g = scen()
        .battlefield(0, &["Vaultborn Tyrant", "Mountain"])
        .counters(0, "Vaultborn Tyrant", Counter::P1P1, 2)
        .battlefield(1, &["Swamp"])
        .hand(1, &["Snuff Out"])
        .at(Step::Main1, 1)
        .start();
    let vt = bf(&g, 0, "Vaultborn Tyrant");
    let so = g.card(1, "Snuff Out", Zone::Hand);
    let t = g.tgt(vt);
    let life = g.life(0);
    let hand = g.player(0).hand.len();
    g.choose(Choice::Cast(so));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        _ => None,
    });
    let tok = tokens_named(&g, "Vaultborn Tyrant");
    assert_eq!(tok.len(), 1, "{}", log(&g));
    assert_eq!(g.pt(tok[0]), (6, 6), "no counters");
    assert!(g.is_type(tok[0], types::ARTIFACT));
    assert_eq!(g.life(0), life + 3, "its enter ability triggered\n{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand + 1);
    // A Metamorph copying the Tyrant dies: its token is the Tyrant (the
    // Metamorph's copiable values) and an artifact.
    let mut g = scen()
        .battlefield(0, &["Vaultborn Tyrant"])
        .battlefield(0, &["Island"; 4])
        .hand(0, &[METAMORPH])
        .battlefield(1, &["Swamp"])
        .hand(1, &["Snuff Out"])
        .at(Step::Main1, 0)
        .start();
    let vt = bf(&g, 0, "Vaultborn Tyrant");
    let m = metamorph_copies(&mut g, vt);
    assert_eq!(g.name(m), "Vaultborn Tyrant");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let so = g.card(1, "Snuff Out", Zone::Hand);
    let t = g.tgt(m);
    g.choose(Choice::Cast(so));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        _ => None,
    });
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    let tok = tokens_named(&g, "Vaultborn Tyrant");
    assert_eq!(tok.len(), 1, "a token copy of the dead Metamorph\n{}", log(&g));
    assert!(g.is_type(tok[0], types::ARTIFACT) && g.pt(tok[0]) == (6, 6));
}

// ---- Esika's Chariot (CR 707.2, 508.4) ---------------------------------------------------------

#[test]
fn esikas_chariots_copy_of_an_attacking_token_doesnt_attack_and_copies_its_copy_cr_707_2_508_4() {
    // ruling: 018fe53e 5a42cc51 09abdfb0
    // Esika's Chariot (crewed by the two Cats it made) attacks with a
    // Reflection token of Test May Draw (haste): the copy is a Test May
    // Draw with haste, not attacking, whose enter ability triggers.
    let mut g = scen()
        .battlefield(0, &[FABLE, "Test May Draw", "Mountain", "Forest", "Forest", "Forest", "Forest"])
        .transformed(0, FABLE)
        .hand(0, &["Esika's Chariot"])
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let md = bf(&g, 0, "Test May Draw");
    let ec = g.card(0, "Esika's Chariot", Zone::Hand);
    g.choose(Choice::Cast(ec));
    resolve(&mut g);
    // (The Chariot entered this turn: Lightning Greaves isn't needed, the
    // test gives it a turn.)
    next_turn_main(&mut g, 0);
    g.choose(ab(r, 0));
    let t = g.tgt(md);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    let tok = tokens_named(&g, "Test May Draw")[0];
    let cats = tokens_named(&g, "Cat");
    assert_eq!(cats.len(), 2);
    g.choose(ab(ec, 0));
    let crew = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| {
        let d = g.pending_decision().unwrap();
        let c = cats.iter().map(|&c| Choice::Card(c)).find(|&c| d.options.iter().any(|o| o.choice == c));
        if c.is_some() {
            crew.set(crew.get() + 1);
        }
        c
    });
    assert!(g.is_creature(ec), "crewed\n{}", log(&g));
    let hand = g.player(0).hand.len();
    attack(&mut g, &[ec, tok]);
    let tt = g.tgt(tok);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(tt),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    let copies: Vec<CardIdx> = tokens_named(&g, "Test May Draw").into_iter().filter(|&c| c != tok).collect();
    assert_eq!(copies.len(), 1, "{}", log(&g));
    assert!(!g.obj(copies[0]).attacking && !g.obj(copies[0]).tapped, "not attacking");
    assert!(g.has_kw(copies[0], Kw::Haste));
    assert_eq!(g.player(0).hand.len(), hand + 1, "its enter ability\n{}", log(&g));
}

// ---- Figure of Fable and Figure of Destiny (CR 613.4b-c) ---------------------------------------

#[test]
fn a_figures_set_pt_doesnt_override_counters_cr_613_4c() {
    // ruling: 3cebd2ad 42771029
    // With a +1/+1 counter (Luminarch Aspirant's), Figure of Fable becoming
    // a 2/3 Scout is a 3/4; Figure of Destiny becoming a 2/2 Spirit, a 3/3.
    for (fig, pt) in [("Figure of Fable", (3, 4)), ("Figure of Destiny", (3, 3))] {
        let mut g = scen().battlefield(0, &[fig, "Plains", "Plains"]).counters(0, fig, Counter::P1P1, 1).at(Step::Main1, 0).start();
        let f = bf(&g, 0, fig);
        g.choose(ab(f, 0));
        pay(&mut g);
        resolve(&mut g);
        assert_eq!(g.pt(f), pt, "{fig}\n{}", log(&g));
    }
}
