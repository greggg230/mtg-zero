//! M7b part 3 (ENGINE.md 26): copies of permanents, part 1: cards that enter
//! as copies (Phyrexian Metamorph, Superior Spider-Man), become copies
//! (Saheeli, Sublime Artificer; Shifting Woodland) or make token copies
//! (Fable of the Mirror-Breaker's Reflection of Kiki-Jiki, Fractured
//! Identity, Springheart Nantuko), each against its Oracle text.

mod common;

use common::db;
use engine::db::{types, AbilitySlot, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Answer decisions with `answer` (else the first option) and pass
/// priority until the stack is empty and a player has priority.
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

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

fn id(name: &str) -> u16 {
    db().expect_id(name).unwrap()
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

const METAMORPH: &str = "Phyrexian Metamorph";

// ---- Phyrexian Metamorph ---------------------------------------------------------------

#[test]
fn phyrexian_metamorph_enters_as_a_copy_except_its_an_artifact_cr_707_5_707_9b() {
    // "You may have this creature enter as a copy of any artifact or creature
    // on the battlefield, except it's an artifact in addition to its other
    // types."
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Thraben Inspector"])
        .battlefield(1, &["Questing Beast"])
        .hand(0, &[METAMORPH])
        .start();
    let m = g.card(0, METAMORPH, Zone::Hand);
    let insp = bf(&g, 0, "Thraben Inspector");
    let qb = bf(&g, 1, "Questing Beast");
    g.choose(Choice::Cast(m));
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => {
            asked.set(true);
            // Artifacts and creatures on the battlefield (not lands), or none.
            assert!(g.has_option(Choice::Card(insp)) && g.has_option(Choice::Card(qb)) && g.has_option(Choice::Pass));
            assert_eq!(g.pending_decision().unwrap().options.len(), 3);
            Some(Choice::Card(insp))
        }
        _ => None,
    });
    assert!(asked.get());
    assert_eq!(g.obj(m).zone, Zone::Battlefield);
    assert!(g.is_copy(m));
    assert_eq!(g.name(m), "Thraben Inspector");
    assert_eq!(g.own_def(m), id(METAMORPH), "it is still the Metamorph card");
    assert_eq!(g.pt(m), (1, 2));
    // CR 707.9b: an artifact in addition to its other types.
    assert!(g.is_type(m, types::ARTIFACT) && g.is_creature(m));
    assert!(!g.is_token(m));
    // CR 707.5: the copied "when this enters" ability triggered (investigate).
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 1);
}

#[test]
fn phyrexian_metamorph_copies_a_legend_of_the_opponent_cr_707_2() {
    // A copy of the opponent's Questing Beast: legendary with its name and
    // abilities, an artifact too; one each, so no legend rule (CR 704.5j).
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &["Questing Beast"]).hand(0, &[METAMORPH]).start();
    let m = g.card(0, METAMORPH, Zone::Hand);
    let qb = bf(&g, 1, "Questing Beast");
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(qb)));
    assert_eq!(g.name(m), "Questing Beast");
    assert!(g.is_legendary(m));
    assert_eq!(g.pt(m), (4, 4));
    for k in [Kw::Vigilance, Kw::Deathtouch, Kw::Haste] {
        assert!(g.has_kw(m, k), "{k:?}");
    }
    assert!(g.is_type(m, types::ARTIFACT));
    assert_eq!(g.obj(qb).zone, Zone::Battlefield);
    assert_eq!(g.obj(m).zone, Zone::Battlefield);
}

#[test]
fn phyrexian_metamorph_may_copy_nothing_and_dies_as_a_0_0_cr_704_5f() {
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &["Questing Beast"]).hand(0, &[METAMORPH]).start();
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Pass));
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
}

#[test]
fn a_copy_of_a_copy_has_its_exceptions_cr_707_3_707_9b() {
    // A second Metamorph copying the first (a copy of Thraben Inspector, an
    // artifact by the first's exception): an Inspector and an artifact;
    // its own "when this enters" (the Inspector's) triggers.
    let mut g = scen()
        .battlefield(0, &["Island"; 8])
        .battlefield(0, &["Thraben Inspector"])
        .hand(0, &[METAMORPH, METAMORPH])
        .start();
    let insp = bf(&g, 0, "Thraben Inspector");
    let m1 = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m1));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(insp)));
    let m2 = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m2));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(m1)));
    assert_eq!(g.name(m2), "Thraben Inspector");
    assert!(g.is_type(m2, types::ARTIFACT));
    assert_eq!(g.copiable(m2).def, id("Thraben Inspector"));
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 2);
}

#[test]
fn a_copy_leaving_the_battlefield_is_its_card_again_cr_400_7() {
    // Metamorph copying Thraben Inspector, returned to hand by Petty Theft:
    // in hand it is the Metamorph card.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .battlefield(0, &["Thraben Inspector"])
        .battlefield(1, &["Island"; 2])
        .hand(0, &[METAMORPH])
        .hand(1, &["Brazen Borrower // Petty Theft"])
        .start();
    let insp = bf(&g, 0, "Thraben Inspector");
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(insp)));
    assert!(g.is_copy(m));
    // Its last known information is the copy (CR 608.2h).
    let r = g.obj_ref(m);
    g.choose(Choice::Pass);
    let bb = g.card(1, "Brazen Borrower // Petty Theft", Zone::Hand);
    g.choose(Choice::CastFace(bb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(m)))),
        _ => None,
    });
    assert_eq!(g.obj(m).zone, Zone::Hand);
    assert!(!g.is_copy(m));
    assert_eq!(g.name(m), METAMORPH);
    assert_ne!(g.obj_ref(m), r);
}

// ---- Superior Spider-Man -----------------------------------------------------------------

const SSM: &str = "Superior Spider-Man";

#[test]
fn superior_spider_man_enters_as_a_copy_of_a_graveyard_creature_card_cr_707_9b() {
    // "Mind Swap - You may have Superior Spider-Man enter as a copy of any
    // creature card in a graveyard, except his name is Superior Spider-Man
    // and he's a 4/4 Spider Human Hero in addition to his other types. When
    // you do, exile that card."
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp"])
        .graveyard(0, &["Lightning Bolt"])
        .graveyard(1, &["Baleful Strix", "Island"])
        .hand(0, &[SSM])
        .start();
    let s = g.card(0, SSM, Zone::Hand);
    let strix = g.card(1, "Baleful Strix", Zone::Graveyard);
    let hand = g.player(0).hand.len();
    g.choose(Choice::Cast(s));
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseGraveyardCard) => {
            asked.set(true);
            // Creature cards in any graveyard (not the Bolt, not the Island),
            // or none.
            assert_eq!(g.pending_decision().unwrap().options.len(), 2);
            Some(Choice::Card(strix))
        }
        _ => None,
    });
    assert!(asked.get());
    assert_eq!(g.name(s), SSM, "his name");
    assert_eq!(g.pt(s), (4, 4), "a 4/4 (CR 707.9b)");
    assert!(g.has_kw(s, Kw::Flying) && g.has_kw(s, Kw::Deathtouch));
    assert!(g.is_type(s, types::ARTIFACT), "Baleful Strix's types");
    for sub in ["Spider", "Human", "Hero"] {
        assert!(g.has_subtype(s, db().subtype_id(sub).unwrap()), "{sub}");
    }
    assert!(!g.is_legendary(s), "the copied card isn't legendary");
    // The copied card's "when this enters, draw a card"; then "exile that
    // card" (a triggered ability, CR 603.11).
    assert_eq!(g.player(0).hand.len(), hand, "cast one, drew one");
    assert_eq!(g.obj(strix).zone, Zone::Exile);
}

#[test]
fn superior_spider_man_copying_a_legend_is_legendary_under_his_own_name_cr_704_5j() {
    // A copy of Griselbrand from his own graveyard: legendary, named Superior
    // Spider-Man, 4/4 (not 7/7), with Griselbrand's abilities.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp"])
        .graveyard(0, &["Griselbrand"])
        .hand(0, &[SSM])
        .start();
    let s = g.card(0, SSM, Zone::Hand);
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then_some(Choice::Card(gris)));
    assert_eq!(g.name(s), SSM);
    assert!(g.is_legendary(s));
    assert_eq!(g.pt(s), (4, 4));
    assert!(g.has_kw(s, Kw::Flying) && g.has_kw(s, Kw::Lifelink));
    assert_eq!(g.obj(gris).zone, Zone::Exile);
    // Griselbrand's "Pay 7 life: Draw seven cards" is his.
    assert!(g.has_option(Choice::Ability(s, AbilitySlot::Activated(0))));
}

#[test]
fn superior_spider_man_may_enter_as_himself() {
    let mut g = scen().battlefield(0, &["Island", "Island", "Swamp", "Swamp"]).graveyard(1, &["Griselbrand"]).hand(0, &[SSM]).start();
    let s = g.card(0, SSM, Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then_some(Choice::Pass));
    assert!(!g.is_copy(s));
    assert_eq!(g.pt(s), (4, 4));
    assert!(g.is_legendary(s));
    assert_eq!(g.count_in(1, "Griselbrand", Zone::Graveyard), 1);
}

// ---- Saheeli, Sublime Artificer -----------------------------------------------------------

const SAHEELI: &str = "Saheeli, Sublime Artificer";

#[test]
fn saheeli_minus_two_makes_an_artifact_a_copy_until_end_of_turn_cr_707_2() {
    // "-2: Target artifact you control becomes a copy of another target
    // artifact or creature you control until end of turn, except it's an
    // artifact in addition to its other types."
    let mut g = scen().battlefield(0, &[SAHEELI, "Sol Ring", "Test Bear"]).start();
    let sa = bf(&g, 0, SAHEELI);
    let ring = bf(&g, 0, "Sol Ring");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if !g.has_option(g.tgt(bear)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => {
            // "Another target": not the Sol Ring again.
            assert!(!g.has_option(g.tgt(ring)));
            Some(g.tgt(bear))
        }
        _ => None,
    });
    assert_eq!(g.counters_on(sa, engine::db::Counter::Loyalty), 3);
    assert_eq!(g.name(ring), "Test Bear");
    assert!(g.is_creature(ring) && g.is_type(ring, types::ARTIFACT));
    assert_eq!(g.pt(ring), (2, 2));
    // It has the copied card's abilities only: no "{T}: Add {C}{C}".
    assert!(g.def_of(ring).activated.is_empty());
    // Under its controller's control since before the turn: it can attack.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(ring)));
    quiet_until(&mut g, |g| g.turn().number == 4);
    assert_eq!(g.name(ring), "Sol Ring", "until end of turn (CR 514.2)");
    assert!(!g.is_creature(ring));
}

#[test]
fn saheeli_minus_two_needs_two_different_permanents_cr_115_3() {
    // Only one artifact creature: no second target, so no activation.
    let g = scen().battlefield(0, &[SAHEELI, "Haywire Mite", "Island"]).start();
    let sa = bf(&g, 0, SAHEELI);
    assert!(!g.has_option(Choice::Ability(sa, AbilitySlot::Activated(0))));
    let g = scen().battlefield(0, &[SAHEELI, "Haywire Mite", "Sol Ring"]).start();
    let sa = bf(&g, 0, SAHEELI);
    assert!(g.has_option(Choice::Ability(sa, AbilitySlot::Activated(0))));
}

#[test]
fn saheeli_copy_of_an_equipment_becomes_unattached_when_it_reverts_cr_704_5p() {
    // The Sol Ring becomes a copy of Lightning Greaves (Equipment, equip
    // {0}) and is attached to a creature; at end of turn it is a Sol Ring
    // again, no longer an Equipment, and becomes unattached (CR 704.5p).
    let mut g = scen().battlefield(0, &[SAHEELI, "Sol Ring", "Lightning Greaves", "Test Bear"]).start();
    let sa = bf(&g, 0, SAHEELI);
    let ring = bf(&g, 0, "Sol Ring");
    let greaves = bf(&g, 0, "Lightning Greaves");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => Some(g.tgt(greaves)),
        _ => None,
    });
    assert_eq!(g.name(ring), "Lightning Greaves");
    // Equip {0}: the copy's ability.
    let slot = (0..g.def_of(ring).activated.len() as u8).find(|&i| g.has_option(Choice::Ability(ring, AbilitySlot::Activated(i)))).expect("equip offered");
    g.choose(Choice::Ability(ring, AbilitySlot::Activated(slot)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(ring).attached.map(|h| h.idx), Some(bear));
    assert!(g.has_kw(bear, Kw::Haste));
    quiet_until(&mut g, |g| g.turn().number == 4);
    assert_eq!(g.name(ring), "Sol Ring");
    assert_eq!(g.obj(ring).attached, None, "no longer an Equipment: unattached (CR 704.5p)");
    assert_eq!(g.obj(ring).zone, Zone::Battlefield);
    assert!(!g.has_kw(bear, Kw::Haste));
}

#[test]
fn an_equipment_made_a_copy_of_a_creature_becomes_unattached_cr_704_5p() {
    // Skullclamp, attached to the Bear, becomes a copy of the Bear
    // (an artifact creature): a creature attached to an object becomes
    // unattached and stays on the battlefield (CR 704.5p).
    let mut g = scen().battlefield(0, &[SAHEELI, "Skullclamp", "Test Bear", "Island"]).start();
    let sa = bf(&g, 0, SAHEELI);
    let clamp = bf(&g, 0, "Skullclamp");
    let bear = bf(&g, 0, "Test Bear");
    let slot = (0..g.def_of(clamp).activated.len() as u8).find(|&i| g.has_option(Choice::Ability(clamp, AbilitySlot::Activated(i)))).expect("equip offered");
    g.choose(Choice::Ability(clamp, AbilitySlot::Activated(slot)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(clamp).attached.map(|h| h.idx), Some(bear));
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(clamp)) => Some(g.tgt(clamp)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert!(g.is_creature(clamp) && g.is_type(clamp, types::ARTIFACT));
    assert_eq!(g.obj(clamp).attached, None);
    assert_eq!(g.obj(clamp).zone, Zone::Battlefield);
}

// ---- Shifting Woodland ----------------------------------------------------------------

const WOODLAND: &str = "Shifting Woodland";

#[test]
fn shifting_woodland_becomes_a_copy_of_a_permanent_card_in_your_graveyard_cr_707_2() {
    // "Delirium - {2}{G}{G}: This land becomes a copy of target permanent
    // card in your graveyard until end of turn. Activate only if there are
    // four or more card types among cards in your graveyard."
    let mut g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &["Griselbrand", "Lightning Bolt", "Mox Pearl", "Island"])
        .start();
    let w = bf(&g, 0, WOODLAND);
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    let slot = (0..g.def_of(w).activated.len() as u8).find(|&i| g.has_option(Choice::Ability(w, AbilitySlot::Activated(i)))).unwrap();
    g.choose(Choice::Ability(w, AbilitySlot::Activated(slot)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            // A permanent card (not the Bolt).
            assert!(!g.has_option(Choice::Target(Target::Obj(g.obj_ref(bolt)))));
            Some(Choice::Target(Target::Obj(g.obj_ref(gris))))
        }
        // (Pay with the Forests, not the Woodland itself.)
        Some(DecisionKind::PayMana) => g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == w))
            .map(|o| o.choice),
        _ => None,
    });
    assert_eq!(g.name(w), "Griselbrand");
    assert!(g.is_creature(w) && !g.is_type(w, types::LAND), "a copy of a creature card: no longer a land");
    assert_eq!(g.pt(w), (7, 7));
    assert_eq!(g.obj(gris).zone, Zone::Graveyard, "the card stays");
    quiet_until(&mut g, |g| g.turn().number == 4);
    assert_eq!(g.name(w), WOODLAND);
    assert!(g.is_type(w, types::LAND));
}

#[test]
fn shifting_woodland_needs_delirium() {
    let g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &["Griselbrand", "Lightning Bolt", "Island"])
        .start();
    let w = bf(&g, 0, WOODLAND);
    let n = g.def_of(w).activated.len() as u8;
    assert!((0..n).all(|i| !g.has_option(Choice::Ability(w, AbilitySlot::Activated(i)))));
}

// ---- Fable of the Mirror-Breaker // Reflection of Kiki-Jiki ----------------------------

const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";

#[test]
fn reflection_of_kiki_jiki_makes_a_hasty_token_copy_sacrificed_at_end_step_cr_707_9a() {
    // "{1}, {T}: Create a token that's a copy of another target nonlegendary
    // creature you control, except it has haste. Sacrifice it at the
    // beginning of the next end step."
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Thraben Inspector", "Questing Beast"])
        .transformed(0, FABLE)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let insp = bf(&g, 0, "Thraben Inspector");
    let qb = bf(&g, 0, "Questing Beast");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            // Another, nonlegendary creature you control.
            assert!(!g.has_option(g.tgt(r)) && !g.has_option(g.tgt(qb)));
            Some(g.tgt(insp))
        }
        _ => None,
    });
    let tokens: Vec<CardIdx> =
        g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == "Thraben Inspector").collect();
    assert_eq!(tokens.len(), 1);
    let t = tokens[0];
    assert!(g.has_kw(t, Kw::Haste), "except it has haste");
    assert_eq!(g.pt(t), (1, 2));
    assert_eq!(g.copiable(t).def, id("Thraben Inspector"));
    // The token's own "when this enters" (investigate) triggered.
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 1);
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().number == 4);
    assert_ne!(g.obj(t).zone, Zone::Battlefield, "sacrificed at the end step");
    assert_eq!(g.obj(insp).zone, Zone::Battlefield);
}

// ---- Fractured Identity -------------------------------------------------------------------

#[test]
fn fractured_identity_exiles_and_the_other_player_gets_a_copy_cr_608_2h() {
    // "Exile target nonland permanent. Each player other than its
    // controller creates a token that's a copy of it."
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Questing Beast"])
        .hand(0, &["Fractured Identity"])
        .start();
    let qb = bf(&g, 1, "Questing Beast");
    let fi = g.card(0, "Fractured Identity", Zone::Hand);
    g.choose(Choice::Cast(fi));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(qb)));
    assert_eq!(g.obj(qb).zone, Zone::Exile);
    let t = bf(&g, 0, "Questing Beast");
    assert!(g.is_token(t));
    assert!(g.is_legendary(t));
    assert_eq!(g.pt(t), (4, 4));
    assert_eq!(g.def_of(t).mana_value(), 4, "a token copy has the copied mana cost (CR 707.2)");
    assert_eq!(g.permanents_of(1).count(), 0);
}

#[test]
fn fractured_identity_on_your_own_permanent_gives_the_opponent_the_copy() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Island", "Island", "Island", "Sol Ring"])
        .hand(0, &["Fractured Identity"])
        .start();
    let ring = bf(&g, 0, "Sol Ring");
    let fi = g.card(0, "Fractured Identity", Zone::Hand);
    g.choose(Choice::Cast(fi));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        // (Pay with the lands, keeping the Sol Ring.)
        Some(DecisionKind::PayMana) => g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == ring))
            .map(|o| o.choice),
        _ => None,
    });
    let t = bf(&g, 1, "Sol Ring");
    assert!(g.is_token(t));
    assert_eq!(g.obj(t).controller, 1);
    assert_eq!(g.obj(t).owner, 1, "the player who creates a token owns it (CR 111.2)");
}

// ---- Springheart Nantuko --------------------------------------------------------------------

const NANTUKO: &str = "Springheart Nantuko";

/// Cast Springheart Nantuko bestowed on `host` (or not), resolve it.
fn cast_nantuko(g: &mut Game, host: Option<CardIdx>) -> CardIdx {
    let n = g.card(0, NANTUKO, Zone::Hand);
    g.choose(Choice::Cast(n));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(host.is_some() as u8)),
        Some(DecisionKind::Target) => host.map(|h| g.tgt(h)),
        _ => None,
    });
    n
}

#[test]
fn springheart_nantuko_cast_bestowed_is_an_aura_cr_702_103b() {
    // "Bestow {1}{G}. Enchanted creature gets +1/+1."
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Test Bear"]).hand(0, &[NANTUKO]).start();
    let bear = bf(&g, 0, "Test Bear");
    let n = cast_nantuko(&mut g, Some(bear));
    assert_eq!(g.obj(n).zone, Zone::Battlefield);
    assert!(g.def_of(n).bestowed);
    assert!(!g.is_creature(n) && g.is_type(n, types::ENCHANTMENT));
    assert!(g.has_subtype(n, db().subtype_id("Aura").unwrap()));
    assert_eq!(g.obj(n).attached.map(|h| h.idx), Some(bear));
    assert_eq!(g.pt(bear), (3, 3));
}

#[test]
fn springheart_nantuko_landfall_copies_its_creature_or_makes_an_insect() {
    // "Landfall - Whenever a land you control enters, you may pay {1}{G} if
    // this permanent is attached to a creature you control. If you do,
    // create a token that's a copy of that creature. If you didn't create a
    // token this way, create a 1/1 green Insect creature token."
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Test Bear"])
        .hand(0, &[NANTUKO, "Forest", "Forest"])
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast_nantuko(&mut g, Some(bear));
    // Pay: a token copy of the Test Bear (2/2: the bonus isn't copied).
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    let copies: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == "Test Bear").collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(g.pt(copies[0]), (2, 2));
    assert_eq!(g.count_in(0, "Insect", Zone::Battlefield), 0);
    // Next turn, decline: an Insect.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    let insect = bf(&g, 0, "Insect");
    assert_eq!(g.pt(insect), (1, 1));
    assert_eq!(g.colors_of(insect), 1 << 4);
}

#[test]
fn springheart_nantuko_as_a_creature_makes_insects_cr_702_103() {
    let mut g = scen().battlefield(0, &["Forest", "Forest"]).hand(0, &[NANTUKO, "Forest"]).start();
    let n = cast_nantuko(&mut g, None);
    assert!(g.is_creature(n) && !g.def_of(n).bestowed);
    assert_eq!(g.pt(n), (1, 1));
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::YesNo) {
            asked.set(true);
        }
        None
    });
    assert!(!asked.get(), "not attached: no payment offered");
    assert_eq!(g.count_in(0, "Insect", Zone::Battlefield), 1);
}

#[test]
fn springheart_nantuko_stays_when_its_creature_dies_cr_702_103f() {
    // An unattached bestowed Aura ceases to be bestowed: an enchantment
    // creature again (an exception to 704.5m).
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Mountain", "Test Bear"])
        .hand(0, &[NANTUKO, "Lightning Bolt"])
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let n = cast_nantuko(&mut g, Some(bear));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(n).zone, Zone::Battlefield);
    assert!(!g.def_of(n).bestowed && g.is_creature(n));
    assert_eq!(g.obj(n).attached, None);
    assert_eq!(g.pt(n), (1, 1));
}

#[test]
fn springheart_nantuko_with_its_target_gone_resolves_as_a_creature_cr_702_103e() {
    // The Bear is bolted in response: the bestowed spell ceases to be
    // bestowed and resolves as a creature spell (608.3b), not fizzling.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Test Bear"])
        .battlefield(1, &["Mountain"])
        .hand(0, &[NANTUKO])
        .hand(1, &["Lightning Bolt"])
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let n = g.card(0, NANTUKO, Zone::Hand);
    g.choose(Choice::Cast(n));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(n).zone, Zone::Battlefield);
    assert!(g.is_creature(n) && !g.def_of(n).bestowed);
}
