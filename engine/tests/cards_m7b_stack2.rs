//! M7b part 3b (ENGINE.md 27): the initiative cards and the cards with
//! abilities activated from a graveyard, each against its Oracle text:
//! Feywild Caretaker, Passageway Seer, White Plume Adventurer, Caves of
//! Chaos Adventurer, Seasoned Dungeoneer, Undermountain Adventurer, From the
//! Catacombs; Fanatic of Rhonas, Otharri, Suns' Glory, Seasoned Pyromancer.
//! (The rules of the initiative and Undercity are in rules_initiative.rs.)

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{act, CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

fn attack_with(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
}

/// The first option naming a basic land in a library search (Secret
/// Entrance).
fn find_basic(g: &Game) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| options(g).into_iter().find(|c| matches!(c, Choice::Def(_))).unwrap_or(Choice::Pass))
}

/// Cast `name` from P0's hand and resolve everything (searching for a basic
/// land if Secret Entrance asks).
fn cast_resolve(g: &mut Game, name: &str) {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    finish(g, |_| None);
    resolve_with(g, find_basic);
}

// ---- Feywild Caretaker, Passageway Seer: "if you have the initiative" -------------------------

#[test]
fn feywild_caretaker_makes_a_faerie_dragon_at_your_end_step_with_the_initiative() {
    // "When this creature enters, you take the initiative. At the beginning
    // of your end step, if you have the initiative, create a 1/1 blue
    // Faerie Dragon creature token with flying."
    let mut g = scen().battlefield(0, &["Island"; 5]).hand(0, &["Feywild Caretaker"]).start();
    cast_resolve(&mut g, "Feywild Caretaker");
    assert_eq!(g.initiative(), Some(0));
    g.drive_until(|g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty() && !tokens_named(g, "Faerie Dragon").is_empty() || g.turn().step == Step::Cleanup);
    let fd = tokens_named(&g, "Faerie Dragon");
    assert_eq!(fd.len(), 1);
    assert!(g.has_kw(fd[0], Kw::Flying));
}

#[test]
fn feywild_caretaker_makes_nothing_without_the_initiative_cr_603_4() {
    let mut g = scen().battlefield(0, &["Feywild Caretaker"]).initiative(1).dungeon(1, Some(0), 0).at(Step::Main2, 0).start();
    g.drive_until(|g| g.turn().active == 1);
    assert!(tokens_named(&g, "Faerie Dragon").is_empty());
}

#[test]
fn passageway_seer_grows_at_your_end_step_with_the_initiative() {
    // "Lifelink. When this creature enters, you take the initiative. At the
    // beginning of your end step, if you have the initiative, put a +1/+1
    // counter on this creature."
    let mut g = scen().battlefield(0, &["Passageway Seer"]).initiative(0).dungeon(0, Some(0), 0).at(Step::Main2, 0).start();
    let ps = bf(&g, 0, "Passageway Seer");
    assert!(g.has_kw(ps, Kw::Lifelink));
    g.drive_until(|g| g.turn().active == 1);
    assert_eq!(g.counters_on(ps, Counter::P1P1), 1);
}

#[test]
fn passageway_seer_takes_the_initiative_as_it_enters() {
    let mut g = scen().battlefield(0, &["Swamp"; 4]).hand(0, &["Passageway Seer"]).initiative(1).dungeon(1, Some(3), 0).start();
    cast_resolve(&mut g, "Passageway Seer");
    assert_eq!(g.initiative(), Some(0));
    assert_eq!(g.dungeon(0), (Some(0), 0));
}

// ---- White Plume Adventurer -----------------------------------------------------------------

const WHITE_PLUME: &str = "White Plume Adventurer";

/// P0 attacks with its creatures, then P1's upkeep comes.
fn to_p1_upkeep_after_attacking(g: &mut Game, attackers: &[CardIdx]) {
    attack_with(g, attackers);
    g.drive_until(|g| g.turn().active == 1 && g.turn().step == Step::Upkeep && !at_priority(g) || g.turn().active == 1 && g.turn().step == Step::Upkeep && !g.stack().is_empty());
}

#[test]
fn white_plume_adventurer_untaps_a_creature_at_each_opponents_upkeep() {
    // "At the beginning of each opponent's upkeep, untap a creature you
    // control. If you've completed a dungeon, untap all creatures you
    // control instead."
    let mut g = scen().battlefield(0, &[WHITE_PLUME, "Test Bear", "Test Striker"]).start();
    let wp = bf(&g, 0, WHITE_PLUME);
    let bear = bf(&g, 0, "Test Bear");
    let st = bf(&g, 0, "Test Striker");
    to_p1_upkeep_after_attacking(&mut g, &[wp, bear, st]);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(bear)),
        _ => None,
    });
    assert!(!g.obj(bear).tapped);
    assert!(g.obj(wp).tapped && g.obj(st).tapped, "one creature");
}

#[test]
fn white_plume_adventurer_untaps_all_after_a_completed_dungeon() {
    let mut g = scen().battlefield(0, &[WHITE_PLUME, "Test Bear", "Test Striker"]).dungeon(0, None, 1).start();
    let wp = bf(&g, 0, WHITE_PLUME);
    let bear = bf(&g, 0, "Test Bear");
    let st = bf(&g, 0, "Test Striker");
    to_p1_upkeep_after_attacking(&mut g, &[wp, bear, st]);
    resolve_with(&mut g, |_| None);
    assert!(!g.obj(wp).tapped && !g.obj(bear).tapped && !g.obj(st).tapped);
}

// ---- Caves of Chaos Adventurer --------------------------------------------------------------

const CAVES: &str = "Caves of Chaos Adventurer";

#[test]
fn caves_of_chaos_adventurer_exiles_the_top_card_to_play_this_turn() {
    // "Trample. When this creature enters, you take the initiative.
    // Whenever this creature attacks, exile the top card of your library. If
    // you've completed a dungeon, you may play that card this turn without
    // paying its mana cost. Otherwise, you may play that card this turn."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Test Giant", "Island", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &[CAVES, "Forest", "Forest", "Forest", "Forest", "Forest"])
        .start();
    let c = bf(&g, 0, CAVES);
    assert!(g.has_kw(c, Kw::Trample));
    attack_with(&mut g, &[c]);
    resolve_with(&mut g, |_| None);
    let giant = g.card(0, "Test Giant", Zone::Exile);
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(Choice::Cast(giant)), "playable this turn, for its mana cost");
    g.choose(Choice::Cast(giant));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.lands_of(0).filter(|&l| g.obj(l).tapped).count(), 5, "paid {{4}}{{G}}");
}

#[test]
fn caves_of_chaos_adventurer_plays_it_free_after_a_completed_dungeon() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Test Giant", "Island", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &[CAVES])
        .dungeon(0, None, 1)
        .start();
    let c = bf(&g, 0, CAVES);
    attack_with(&mut g, &[c]);
    resolve_with(&mut g, |_| None);
    let giant = g.card(0, "Test Giant", Zone::Exile);
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(Choice::Cast(giant)), "free, with no land");
    g.choose(Choice::Cast(giant));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
}

// ---- Seasoned Dungeoneer --------------------------------------------------------------------

#[test]
fn seasoned_dungeoneer_protects_and_explores_an_attacking_warrior() {
    // "When this creature enters, you take the initiative. Whenever you
    // attack, target attacking Cleric, Rogue, Warrior, or Wizard gains
    // protection from creatures until end of turn. It explores." The
    // Dungeoneer is a Human Warrior: it can't be blocked by creatures.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Lightning Bolt", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &["Seasoned Dungeoneer", "Test Bear"])
        .battlefield(1, &["Test Wall"])
        .start();
    let sd = bf(&g, 0, "Seasoned Dungeoneer");
    let bear = bf(&g, 0, "Test Bear");
    attack_with(&mut g, &[sd, bear]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(sd)) && !g.has_option(g.tgt(bear)), "a Warrior, not the Bear");
    g.choose(g.tgt(sd));
    // Explore reveals the Bolt (nonland): a +1/+1 counter; keep it on top.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.counters_on(sd, Counter::P1P1), 1);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert!(!g.has_option(Choice::Card(sd)), "protection from creatures: the Wall can't block it");
}

// ---- Undermountain Adventurer ---------------------------------------------------------------

#[test]
fn undermountain_adventurer_adds_two_or_six_green() {
    // "Vigilance. When this creature enters, you take the initiative. {T}:
    // Add {G}{G}. If you've completed a dungeon, add six {G} instead."
    let g = scen().battlefield(0, &["Undermountain Adventurer"]).hand(0, &["Test Giant"]).start();
    let ua = bf(&g, 0, "Undermountain Adventurer");
    assert!(g.has_kw(ua, Kw::Vigilance));
    let giant = g.card(0, "Test Giant", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(giant)), "{{G}}{{G}} isn't {{4}}{{G}}");
    let mut g = scen().battlefield(0, &["Undermountain Adventurer"]).hand(0, &["Test Giant"]).dungeon(0, None, 1).start();
    let giant = g.card(0, "Test Giant", Zone::Hand);
    assert!(g.has_option(Choice::Cast(giant)), "six {{G}}");
    g.choose(Choice::Cast(giant));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.player(0).pool.amt[4], 1, "the sixth {{G}} stays in the pool (CR 106.4)");
}

// ---- From the Catacombs ----------------------------------------------------------------------

const CATACOMBS: &str = "From the Catacombs";

#[test]
fn from_the_catacombs_reanimates_with_a_corpse_counter_and_takes_the_initiative() {
    // "Put target creature card from a graveyard onto the battlefield under
    // your control with a corpse counter on it. You take the initiative. If
    // that creature would leave the battlefield, exile it instead of putting
    // it anywhere else."
    let mut g = scen().battlefield(0, &["Swamp"; 5]).hand(0, &[CATACOMBS]).graveyard(1, &["Test Giant"]).start();
    let giant = g.card(1, "Test Giant", Zone::Graveyard);
    let fc = g.card(0, CATACOMBS, Zone::Hand);
    g.choose(Choice::Cast(fc));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(giant)));
    resolve_with(&mut g, find_basic);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.obj(giant).controller, 0);
    assert_eq!(g.counters_of(giant).len(), 1, "a corpse counter");
    assert_eq!(g.initiative(), Some(0));
    assert_eq!(g.dungeon(0), (Some(0), 0));
}

#[test]
fn a_creature_from_the_catacombs_is_exiled_instead_of_dying_cr_614_1a() {
    let mut g = scen()
        .battlefield(0, &["Swamp"; 5])
        .hand(0, &[CATACOMBS])
        .graveyard(0, &["Test Bear"])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    let fc = g.card(0, CATACOMBS, Zone::Hand);
    g.choose(Choice::Cast(fc));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    resolve_with(&mut g, find_basic);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    g.pass(); // P1 may act in P0's main phase
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "exiled instead of put into the graveyard");
}

#[test]
fn from_the_catacombs_has_escape() {
    // "Escape-{3}{B}{B}, Exile five other cards from your graveyard."
    let g = scen()
        .battlefield(0, &["Swamp"; 5])
        .graveyard(0, &[CATACOMBS, "Island", "Island", "Island", "Island", "Island"])
        .graveyard(1, &["Test Bear"])
        .start();
    let fc = g.card(0, CATACOMBS, Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(fc)));
    let g = scen()
        .battlefield(0, &["Swamp"; 5])
        .graveyard(0, &[CATACOMBS, "Island", "Island", "Island", "Island"])
        .graveyard(1, &["Test Bear"])
        .start();
    let fc = g.card(0, CATACOMBS, Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(fc)), "four other cards aren't five");
}

// ---- Fanatic of Rhonas ----------------------------------------------------------------------

const FANATIC: &str = "Fanatic of Rhonas";

#[test]
fn fanatic_of_rhonas_eternalize_from_the_graveyard_cr_702_129a() {
    // ruling: 450b7583 d971bcd1
    // "Eternalize {2}{G}{G} ({2}{G}{G}, Exile this card from your graveyard:
    // Create a token that's a copy of it, except it's a 4/4 black Zombie
    // Snake Druid with no mana cost. Eternalize only as a sorcery.)" The
    // graveyard ability's id: `GY_ABILITY` + 0 (the first card with one).
    let mut g = scen().battlefield(0, &["Forest"; 4]).graveyard(0, &["Island", FANATIC]).start();
    let f = g.card(0, FANATIC, Zone::Graveyard);
    let a = Choice::Ability(f, AbilitySlot::Graveyard(0));
    assert_eq!(g.pending_decision().unwrap().action_of(a), Some(act::GY_ABILITY));
    // Encoded: graveyard-ability slot 0 is the Fanatic, an option of P0's.
    use engine::az::encode::{self, CODE, GYA_W, NUM_FEATURES};
    for viewer in [0u8, 1] {
        let mut x = vec![0.0; NUM_FEATURES];
        g.encode_features(viewer, &mut x);
        let s = &x[encode::O_GYA..encode::O_GYA + GYA_W];
        let want = if viewer == 0 { (1.0, 1.0) } else { (0.0, 0.0) };
        assert_eq!((s[CODE], s[CODE + 1]), want, "P0's graveyard for P0; P1's (empty) for P1");
    }
    g.choose(a);
    finish(&mut g, |_| None);
    assert_eq!(g.obj(f).zone, Zone::Exile, "exiled as a cost");
    resolve_with(&mut g, |_| None);
    let t = tokens_named(&g, FANATIC);
    assert_eq!(t.len(), 1);
    let t = t[0];
    assert_eq!(g.pt(t), (4, 4));
    assert_eq!(g.colors_of(t), 1 << 2, "black");
    // Its card code's descriptor: black, mana value 0, a 4/4 (CR 707.9b).
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode_features(0, &mut x);
    let o = encode::O_MY_PERM + g.permanents_of(0).position(|c| c == t).unwrap() * encode::PERM_W;
    assert_eq!(&x[o + 38..o + 46], &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 0.5]);
    let zombie = db().subtype_id("Zombie").unwrap();
    assert!(g.has_subtype(t, zombie) && g.has_subtype(t, db().subtype_id("Snake").unwrap()));
}

#[test]
fn fanatic_of_rhonas_eternalize_only_as_a_sorcery() {
    let mut g = scen().battlefield(0, &["Forest"; 4]).graveyard(0, &[FANATIC]).at(Step::Upkeep, 0).start();
    g.drive_until(at_priority);
    let f = g.card(0, FANATIC, Zone::Graveyard);
    assert!(!g.has_option(Choice::Ability(f, AbilitySlot::Graveyard(0))));
}

#[test]
fn fanatic_of_rhonas_ferocious_mana() {
    // "{T}: Add {G}. Ferocious - {T}: Add {G}{G}{G}{G}. Activate only if you
    // control a creature with power 4 or greater." With Test Giant (5/5):
    // Fanatic alone pays for another Test Giant's {4}{G}... no: four {G}
    // plus a Forest.
    let g = scen().battlefield(0, &[FANATIC, "Test Giant", "Forest"]).hand(0, &["Test Giant"]).start();
    let tg = g.card(0, "Test Giant", Zone::Hand);
    assert!(g.has_option(Choice::Cast(tg)), "{{G}}{{G}}{{G}}{{G}} + {{G}}");
    let g = scen().battlefield(0, &[FANATIC, "Test Bear", "Forest"]).hand(0, &["Test Giant"]).start();
    let tg = g.card(0, "Test Giant", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(tg)), "no creature with power 4");
}

// ---- Otharri, Suns' Glory -------------------------------------------------------------------

const OTHARRI: &str = "Otharri, Suns' Glory";

#[test]
fn otharri_gets_experience_and_makes_attacking_rebels() {
    // "Flying, lifelink, haste. Whenever Otharri attacks, you get an
    // experience counter. Then create a 2/2 red Rebel creature token that's
    // tapped and attacking for each experience counter you have."
    let mut g = scen().battlefield(0, &[OTHARRI]).start();
    let o = bf(&g, 0, OTHARRI);
    assert!(g.has_kw(o, Kw::Flying) && g.has_kw(o, Kw::Lifelink) && g.has_kw(o, Kw::Haste));
    attack_with(&mut g, &[o]);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).experience, 1);
    let r = tokens_named(&g, "Rebel");
    assert_eq!(r.len(), 1);
    assert!(g.obj(r[0]).tapped && g.obj(r[0]).attacking);
    g.drive_until(|g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 15, "3 + 2");
    assert_eq!(g.life(0), 23, "lifelink");
}

#[test]
fn otharri_cant_return_without_an_untapped_rebel() {
    // "{2}{R}{W}, Tap an untapped Rebel you control: Return this card from
    // your graveyard to the battlefield tapped."
    let g = scen().battlefield(0, &["Mountain", "Plains", "Mountain", "Plains", "Test Bear"]).graveyard(0, &[OTHARRI]).start();
    let o = g.card(0, OTHARRI, Zone::Graveyard);
    assert!(!g.has_option(Choice::Ability(o, AbilitySlot::Graveyard(0))), "no Rebel");
}

#[test]
fn otharri_returns_tapped_paying_with_a_rebel_token() {
    // Otharri attacks (one Rebel), dies; next turn the Rebel pays.
    let mut g = scen()
        .battlefield(0, &[OTHARRI, "Mountain", "Plains", "Mountain", "Plains"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let o = bf(&g, 0, OTHARRI);
    attack_with(&mut g, &[o]);
    resolve_with(&mut g, |_| None);
    let rebel = tokens_named(&g, "Rebel")[0];
    // P1 Bolts Otharri after combat.
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(o)));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(o).zone, Zone::Graveyard);
    // P0's next turn.
    g.drive_until(|g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.obj(rebel).tapped);
    let a = Choice::Ability(o, AbilitySlot::Graveyard(0));
    assert!(g.has_option(a), "{:?}", g.options_desc());
    g.choose(a);
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(rebel)));
    assert!(g.obj(rebel).tapped, "the Rebel paid");
    resolve_with(&mut g, |_| None);
    let o = bf(&g, 0, OTHARRI);
    assert!(g.obj(o).tapped, "returned tapped");
}

// ---- Seasoned Pyromancer --------------------------------------------------------------------

const PYRO: &str = "Seasoned Pyromancer";

#[test]
fn seasoned_pyromancer_discards_two_draws_two_and_makes_elementals_per_nonland() {
    // "When this creature enters, discard two cards, then draw two cards.
    // For each nonland card discarded this way, create a 1/1 red Elemental
    // creature token."
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .hand(0, &[PYRO, "Lightning Bolt", "Forest", "Test Bear"])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let p = g.card(0, PYRO, Zone::Hand);
    g.choose(Choice::Cast(p));
    finish(&mut g, |_| None);
    let picks = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Discard)).then(|| {
            picks.set(picks.get() + 1);
            if picks.get() == 1 { Choice::Card(bolt) } else { Choice::Card(bear) }
        })
    });
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), 3, "the Forest and two cards drawn");
    let e = tokens_named(&g, "Elemental");
    assert_eq!(e.len(), 2);
    assert_eq!(g.pt(e[0]), (1, 1));
}

#[test]
fn seasoned_pyromancer_with_one_card_discards_it() {
    let mut g = scen().battlefield(0, &["Mountain"; 3]).hand(0, &[PYRO, "Forest"]).start();
    cast_resolve(&mut g, PYRO);
    assert_eq!(g.player(0).hand.len(), 2, "discarded the Forest, drew two");
    assert!(tokens_named(&g, "Elemental").is_empty(), "a land: no token");
}

#[test]
fn seasoned_pyromancer_from_the_graveyard_makes_two_elementals() {
    // "{3}{R}{R}, Exile this card from your graveyard: Create two 1/1 red
    // Elemental creature tokens." (At instant speed.)
    let mut g = scen().battlefield(0, &["Mountain"; 5]).graveyard(0, &[PYRO]).at(Step::Upkeep, 0).start();
    g.drive_until(at_priority);
    let p = g.card(0, PYRO, Zone::Graveyard);
    g.choose(Choice::Ability(p, AbilitySlot::Graveyard(0)));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(p).zone, Zone::Exile);
    assert_eq!(tokens_named(&g, "Elemental").len(), 2);
}

/// Fourteen Fanatics of Rhonas in the graveyard: each card's eternalize is
/// an option at priority (CR 602.2: an activated ability of a card in a
/// graveyard may be activated). The first 11 are `GY_ABILITY` + 0..=10 and
/// `GY_ABILITY` + 11 is "next page"; the page after holds the other three at
/// + 0..=2 (ENGINE.md 100.2; before it the 13th and later were not offered,
/// docs/shortcuts.md 27.13.5). The 14th is eternalized.
#[test]
fn cr_602_2_every_graveyard_ability_is_offered_past_the_12_ids() {
    let mut g = scen().battlefield(0, &["Forest"; 4]).graveyard(0, &[FANATIC; 14]).start();
    let fans: Vec<CardIdx> = g.player(0).graveyard.clone();
    let gy_opts = |g: &Game| -> Vec<(u16, Choice)> {
        g.pending_decision()
            .unwrap()
            .options
            .iter()
            .filter(|o| (act::GY_ABILITY..act::GY_ABILITY + act::GY_ABILITY_N).contains(&o.action))
            .map(|o| (o.action, o.choice))
            .collect()
    };
    let first = gy_opts(&g);
    assert_eq!(first.len(), 12);
    for (k, &(a, c)) in first[..11].iter().enumerate() {
        assert_eq!((a, c), (act::GY_ABILITY + k as u16, Choice::Ability(fans[k], AbilitySlot::Graveyard(0))));
    }
    assert!(matches!(first[11], (a, Choice::Page(_)) if a == act::GY_ABILITY + 11));
    g.choose(first[11].1);
    assert_eq!(g.kind(), Some(DecisionKind::Priority));
    let page: Vec<(u16, Choice)> = g.pending_decision().unwrap().options.iter().map(|o| (o.action, o.choice)).collect();
    let want: Vec<(u16, Choice)> = (11..14).map(|k| (act::GY_ABILITY + k as u16 - 11, Choice::Ability(fans[k], AbilitySlot::Graveyard(0)))).collect();
    assert_eq!(page, want, "the range page holds the rest alone");
    g.choose(Choice::Ability(fans[13], AbilitySlot::Graveyard(0)));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(fans[13]).zone, Zone::Exile, "the 14th exiled as its cost");
}

/// The identity a block slot at `o` holds, if present (its card code's three
/// one-hot digits; present: the status's first float).
fn slot_identity(x: &[f32], o: usize) -> Option<usize> {
    use engine::az::encode::CODE;
    let digit = |a: usize, n: usize| x[o + a..o + a + n].iter().position(|&v| v == 1.0);
    (x[o + CODE] == 1.0).then(|| digit(0, 16).unwrap() * 64 + digit(16, 8).unwrap() * 8 + digit(24, 8).unwrap())
}

/// On a page of Priority's graveyard abilities, the graveyard-abilities
/// block shows the page's cards at their slots (ENGINE.md 100.5.1:
/// `GY_ABILITY` + j is the card at place 11 + j on the second page):
/// fourteen Fanatics of Rhonas, the second page holds three, in slots 0..2,
/// and slot 3 is empty (the first page: four).
#[test]
fn cr_602_2_a_page_of_graveyard_abilities_is_the_blocks() {
    use engine::az::encode::{GYAS, GYA_W, O_GYA};
    let mut g = scen().battlefield(0, &["Forest"; 4]).graveyard(0, &[FANATIC; 14]).start();
    let present = |g: &Game| {
        let mut x = vec![0.0f32; engine::az::NUM_FEATURES];
        g.encode_features(0, &mut x);
        (0..GYAS).map(|k| slot_identity(&x, O_GYA + k * GYA_W).is_some()).collect::<Vec<bool>>()
    };
    assert_eq!(present(&g), [true; 4]);
    let next = options(&g).into_iter().find(|c| matches!(c, Choice::Page(_))).unwrap();
    g.choose(next);
    assert_eq!(g.pending_decision().unwrap().options.len(), 3);
    assert_eq!(present(&g), [true, true, true, false], "places 11..13");
}
