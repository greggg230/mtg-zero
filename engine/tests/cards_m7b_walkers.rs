//! M7b planeswalkers (ENGINE.md 24), part 1: Chandra, Torch of Defiance;
//! Comet, Stellar Pup; Dack Fayden; Elspeth, Storm Slayer; Jace, Wielder of
//! Mysteries; Jace, the Mind Sculptor; Kaito, Bane of Nightmares.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::hidden::{key, key_kind, key_player, KEY_DIE, KEY_LOOK};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    manual().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn loyalty(g: &Game, c: CardIdx) -> u64 {
    g.counters_on(c, Counter::Loyalty)
}

fn tgt_player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// Answer the decisions with a single option that aren't priority.
fn settle(g: &mut Game) {
    while let Some(d) = g.pending_decision() {
        if d.kind == DecisionKind::Priority || d.options.len() != 1 {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
}

/// Answer everything with first options until a player has priority.
fn pay(g: &mut Game) {
    while !at_priority(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress");
}

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

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    settle(g);
}

/// Activate loyalty ability `i` and pay (targets answered by `answer`).
fn activate(g: &mut Game, c: CardIdx, i: u8, answer: impl Fn(&Game) -> Option<Choice>) {
    g.choose(act(c, i));
    while !at_priority(g) {
        let ch = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(ch);
    }
}

fn none(_: &Game) -> Option<Choice> {
    None
}

// ---- Chandra, Torch of Defiance ------------------------------------------

const CHANDRA: &str = "Chandra, Torch of Defiance";

#[test]
fn chandra_plus_one_exiles_the_top_card_and_casts_it() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[CHANDRA, "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, CHANDRA);
    activate(&mut g, ch, 0, none);
    assert_eq!(loyalty(&g, ch), 5);
    resolve_with(&mut g, |g| match g.kind() {
        // CR 608.2g: cast during the resolution (an instant here, but its
        // card type doesn't matter).
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Target) => Some(tgt_player(1)),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
    assert_eq!(g.life(1), 17, "the Bolt, not Chandra's 2");
}

#[test]
fn chandra_plus_one_deals_two_when_the_card_is_not_cast() {
    // A land can't be cast; a spell may be declined.
    for (top, decline) in [("Island", false), ("Lightning Bolt", true)] {
        let mut g = manual()
            .library(0, &[top, "Island"])
            .library(1, &["Forest"; 5])
            .battlefield(0, &[CHANDRA, "Mountain"])
            .at(Step::Main1, 0)
            .start();
        let ch = bf(&g, 0, CHANDRA);
        activate(&mut g, ch, 0, none);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::YesNo) => {
                assert!(decline, "a land is never offered");
                Some(Choice::Pass)
            }
            _ => None,
        });
        // ruling: dcd6db69
        // "Cast" plays no land: the Island stays exiled, unplayable.
        assert_eq!(g.count_in(0, top, Zone::Exile), 1);
        assert_eq!(g.life(1), 18);
        let c = g.card(0, top, Zone::Exile);
        assert!(!g.has_option(Choice::Play(c)) && !g.has_option(Choice::Cast(c)));
    }
}

#[test]
fn chandra_plus_one_cant_cast_under_teferi_time_raveler() {
    // "Each opponent can cast spells only any time they could cast a
    // sorcery": during a resolution the stack isn't empty.
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[CHANDRA, "Mountain"])
        .battlefield(1, &["Teferi, Time Raveler"])
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, CHANDRA);
    activate(&mut g, ch, 0, none);
    resolve_with(&mut g, |g| {
        assert_ne!(g.kind(), Some(DecisionKind::YesNo));
        None
    });
    assert_eq!(g.life(1), 18);
}

#[test]
fn chandra_mana_and_emblem() {
    // +1: add {R}{R} (a loyalty ability, so not a mana ability: it uses the
    // stack, CR 605.1a).
    let mut g = scen().battlefield(0, &[CHANDRA]).at(Step::Main1, 0).start();
    let ch = bf(&g, 0, CHANDRA);
    activate(&mut g, ch, 1, none);
    assert_eq!(g.stack().len(), 1);
    resolve_with(&mut g, none);
    assert_eq!(g.player(0).pool.amt[3], 2);
    // -7: the emblem; Chandra (7 loyalty) dies.
    let mut g = scen().battlefield(0, &[CHANDRA]).counters(0, CHANDRA, Counter::Loyalty, 7).at(Step::Main1, 0).start();
    let ch = bf(&g, 0, CHANDRA);
    activate(&mut g, ch, 3, none);
    resolve_with(&mut g, none);
    assert_eq!(g.obj(ch).zone, Zone::Graveyard);
    assert_eq!(g.command_zone().len(), 1);
    assert_eq!(g.name(g.command_zone()[0]), "Chandra, Torch of Defiance emblem");
}

// ---- Comet, Stellar Pup -----------------------------------------------------

const COMET: &str = "Comet, Stellar Pup";

/// Activate Comet's 0 with the die forced to `n`.
fn comet_roll(g: &mut Game, comet: CardIdx, n: u64, answer: impl Fn(&Game) -> Option<Choice>) {
    activate(g, comet, 0, none);
    g.pass(); // P0
    g.force_chance(&[key(KEY_DIE, 0, n)]);
    let keys = g.pass(); // P1: it resolves
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_DIE && key_player(k) == 0), "a public die key");
    resolve_with(g, answer);
}

#[test]
fn comet_one_or_two_makes_hasty_squirrels_doubled_by_elspeth() {
    let mut g = scen().battlefield(0, &[COMET]).at(Step::Main1, 0).start();
    let comet = bf(&g, 0, COMET);
    comet_roll(&mut g, comet, 2, none);
    assert_eq!(loyalty(&g, comet), 7);
    assert_eq!(g.count_in(0, "Squirrel", Zone::Battlefield), 2);
    let sq = bf(&g, 0, "Squirrel");
    assert!(g.has_kw(sq, Kw::Haste));
    // With Elspeth, Storm Slayer: twice that many (CR 614.1a), all hasty.
    let mut g = scen().battlefield(0, &[COMET, "Elspeth, Storm Slayer"]).at(Step::Main1, 0).start();
    let comet = bf(&g, 0, COMET);
    comet_roll(&mut g, comet, 1, none);
    assert_eq!(g.count_in(0, "Squirrel", Zone::Battlefield), 4);
    let squirrels: Vec<CardIdx> =
        g.battlefield().iter().copied().filter(|&c| g.name(c) == "Squirrel").collect();
    assert!(squirrels.iter().all(|&c| g.has_kw(c, Kw::Haste)));
}

#[test]
fn comet_three_returns_a_cheap_card_from_the_graveyard() {
    let mut g = scen()
        .battlefield(0, &[COMET])
        .graveyard(0, &["Lightning Bolt", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let comet = bf(&g, 0, COMET);
    comet_roll(&mut g, comet, 3, |g| {
        if g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
            // Only mana value 2 or less: the Bolt (not chosen by a target).
            assert_eq!(g.pending_decision().unwrap().options.len(), 1);
        }
        None
    });
    assert_eq!(loyalty(&g, comet), 4);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1);
    assert_eq!(g.count_in(0, "Test Giant", Zone::Graveyard), 1);
}

#[test]
fn comet_four_or_five_deals_its_loyalty_to_a_chosen_creature_or_player_then_minus_two() {
    // ruling: 46ac16d1
    let mut g = scen().battlefield(0, &[COMET]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let comet = bf(&g, 0, COMET);
    let bear = bf(&g, 1, "Test Bear");
    comet_roll(&mut g, comet, 5, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(g.has_option(g.tgt(bear)) && g.has_option(tgt_player(0)));
            Some(tgt_player(1))
        }
        _ => None,
    });
    assert_eq!(g.life(1), 15, "5 loyalty");
    assert_eq!(loyalty(&g, comet), 3);
}

#[test]
fn comet_six_allows_two_more_activations_this_turn() {
    let mut g = scen().battlefield(0, &[COMET]).at(Step::Main1, 0).start();
    let comet = bf(&g, 0, COMET);
    comet_roll(&mut g, comet, 6, none);
    assert_eq!(loyalty(&g, comet), 6);
    assert!(g.has_option(act(comet, 0)), "CR 606.3: two more this turn");
    comet_roll(&mut g, comet, 1, none);
    assert!(g.has_option(act(comet, 0)));
    comet_roll(&mut g, comet, 1, none);
    assert!(!g.has_option(act(comet, 0)), "three in all");
    assert_eq!(loyalty(&g, comet), 10);
}

// ---- Dack Fayden ---------------------------------------------------------------

#[test]
fn dack_fayden_draws_then_discards_and_steals_an_artifact_for_good() {
    let mut g = scen().battlefield(0, &["Dack Fayden"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let dack = bf(&g, 0, "Dack Fayden");
    activate(&mut g, dack, 0, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt_player(0)));
    resolve_with(&mut g, none);
    assert_eq!(g.player(0).hand.len(), 1, "1 + 2 - 2");
    assert_eq!(g.player(0).graveyard.len(), 2);
    assert_eq!(loyalty(&g, dack), 4);
    // -2: gain control of target artifact (no duration).
    let mut g = scen().battlefield(0, &["Dack Fayden"]).battlefield(1, &["Sol Ring"]).at(Step::Main1, 0).start();
    let dack = bf(&g, 0, "Dack Fayden");
    let ring = bf(&g, 1, "Sol Ring");
    activate(&mut g, dack, 1, none);
    resolve_with(&mut g, none);
    assert_eq!(g.obj(ring).controller, 0);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main2);
    assert_eq!(g.obj(ring).controller, 0, "permanently");
}

#[test]
fn dack_fayden_emblem_steals_the_permanents_a_spell_targets() {
    let mut g = scen()
        .emblem(0, "Dack Fayden")
        .battlefield(0, &["Mountain", "Swamp", "Plains", "Mountain"])
        .hand(0, &["Vindicate", "Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    // A spell targeting only a player: no trigger.
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(tgt_player(1));
    pay(&mut g);
    assert_eq!(g.stack().len(), 1);
    resolve_with(&mut g, none);
    // Vindicate targets the Bear: the emblem's trigger resolves first.
    cast(&mut g, 0, "Vindicate");
    g.choose(g.tgt(bear));
    pay(&mut g);
    assert_eq!(g.stack().len(), 2);
    g.pass();
    g.pass();
    settle(&mut g);
    assert_eq!(g.obj(bear).controller, 0);
    resolve_with(&mut g, none);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn dack_fayden_emblem_steals_even_when_the_spell_is_countered_first() {
    // ruling: e2bef5e6
    // "Gain control of those permanents": the permanents the spell targeted
    // as the ability triggered, whether or not the spell is still there.
    let mut g = scen()
        .emblem(0, "Dack Fayden")
        .battlefield(0, &["Mountain", "Swamp", "Plains", "Mountain"])
        .hand(0, &["Vindicate"])
        .battlefield(1, &["Test Bear", "Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Vindicate");
    g.choose(g.tgt(bear));
    pay(&mut g);
    assert_eq!(g.stack().len(), 2, "Vindicate and the emblem's trigger");
    // Player 1 counters Vindicate with the trigger still on the stack.
    g.pass();
    // (Its only legal target, so the target is chosen automatically.)
    cast(&mut g, 1, "Counterspell");
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Vindicate", Zone::Graveyard), 1, "countered");
    assert_eq!(g.obj(bear).controller, 0, "the trigger still gains control");
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
}

// ---- Elspeth, Storm Slayer -------------------------------------------------------

const ELSPETH: &str = "Elspeth, Storm Slayer";

#[test]
fn elspeth_makes_two_soldiers_and_her_flying_lasts_until_her_next_turn() {
    let mut g = scen().battlefield(0, &[ELSPETH, "Test Bear"]).at(Step::Main1, 0).start();
    let el = bf(&g, 0, ELSPETH);
    activate(&mut g, el, 0, none);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Soldier", Zone::Battlefield), 2, "doubled (CR 614.1a)");
    // Next turn: 0: counters, flying until her controller's next turn.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    activate(&mut g, el, 1, none);
    resolve_with(&mut g, none);
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(bear), (3, 3));
    assert!(g.has_kw(bear, Kw::Flying));
    quiet_until(&mut g, |g| g.turn().number == 6 && g.turn().step == Step::Main2);
    assert!(g.has_kw(bear, Kw::Flying), "through the opponent's turn");
    quiet_until(&mut g, |g| g.turn().number == 7 && g.turn().step == Step::Main1);
    assert!(!g.has_kw(bear, Kw::Flying), "ended as her controller's turn began");
    assert_eq!(g.pt(bear), (3, 3));
}

#[test]
fn elspeth_minus_three_destroys_a_creature_with_mana_value_three_or_more() {
    let mut g = scen().battlefield(0, &[ELSPETH]).battlefield(1, &["Test Bear", "Test Giant"]).at(Step::Main1, 0).start();
    let el = bf(&g, 0, ELSPETH);
    let bear = bf(&g, 1, "Test Bear");
    let giant = bf(&g, 1, "Test Giant");
    g.choose(act(el, 2));
    assert!(!g.has_option(g.tgt(bear)), "mana value 2");
    g.choose(g.tgt(giant));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard);
    assert_eq!(loyalty(&g, el), 2);
}

// ---- Jace, Wielder of Mysteries ---------------------------------------------------

const JWM: &str = "Jace, Wielder of Mysteries";

#[test]
fn jace_wielder_mills_draws_and_wins_on_an_empty_library() {
    // ruling: bc4ce4a3
    let mut g = scen().battlefield(0, &[JWM]).at(Step::Main1, 0).start();
    let j = bf(&g, 0, JWM);
    activate(&mut g, j, 0, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt_player(1)));
    resolve_with(&mut g, none);
    assert_eq!(g.player(1).library.len(), 8);
    assert_eq!(g.player(1).graveyard.len(), 2);
    assert_eq!(g.player(0).hand.len(), 1);
    // "If you would draw a card while your library has no cards in it, you
    // win the game instead" (CR 614.1a).
    let mut g = manual().library(1, &["Forest"; 5]).battlefield(0, &[JWM]).at(Step::Upkeep, 0).start();
    quiet_until(&mut g, |g| g.is_over());
    let o = g.outcome().unwrap();
    assert_eq!(o.winner, Some(0));
}

#[test]
fn jace_wielder_minus_eight_draws_seven_then_wins_with_an_empty_library() {
    // ruling: 0dc0242d
    for (n, wins) in [(7usize, true), (5, true), (10, false)] {
        let lib = vec!["Island"; n];
        let mut g = manual()
            .library(0, &lib)
            .library(1, &["Forest"; 5])
            .battlefield(0, &[JWM])
            .counters(0, JWM, Counter::Loyalty, 8)
            .at(Step::Main1, 0)
            .start();
        let j = bf(&g, 0, JWM);
        activate(&mut g, j, 1, none);
        resolve_with(&mut g, none);
        assert_eq!(g.outcome().map(|o| o.winner), if wins { Some(Some(0)) } else { None }, "{n} cards");
    }
}

// ---- Jace, the Mind Sculptor -----------------------------------------------------

const JTMS: &str = "Jace, the Mind Sculptor";

#[test]
fn jace_the_mind_sculptor_plus_two_looks_at_the_top_card_and_may_bottom_it() {
    let mut g = manual()
        .library(0, &["Island"; 5])
        .library(1, &["Lightning Bolt", "Forest", "Forest"])
        .battlefield(0, &[JTMS])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 0, JTMS);
    let bolt = g.card(1, "Lightning Bolt", Zone::Library);
    activate(&mut g, jace, 0, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt_player(1)));
    g.pass();
    let keys = g.pass();
    // P0's private look (keyed for P0).
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_LOOK && key_player(k) == 0));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    resolve_with(&mut g, none);
    assert_eq!(g.player(1).library[0], bolt, "on the bottom");
    // P1 didn't know its top card: P0 can't keep knowing it (ENGINE.md
    // 23.7).
    assert!(!g.knows(0, bolt));
    assert_eq!(loyalty(&g, jace), 5);
}

#[test]
fn jace_the_mind_sculptor_brainstorms_bounces_and_ultimates() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Consider", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[JTMS])
        .hand(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 0, JTMS);
    activate(&mut g, jace, 1, none);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => {
            let bear = g.find_card(0, "Test Bear", Zone::Hand);
            let island = g.find_card(0, "Island", Zone::Hand);
            Some(Choice::Card(bear.or(island).unwrap()))
        }
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), 2, "1 + 3 - 2");
    assert_eq!(g.player(0).library.len(), 3);
    // -1: return target creature.
    let mut g = scen().battlefield(0, &[JTMS]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let jace = bf(&g, 0, JTMS);
    activate(&mut g, jace, 2, none);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Hand), 1);
    assert_eq!(loyalty(&g, jace), 2);
    // -12: exile target player's library; they shuffle their hand in.
    let mut g = scen()
        .battlefield(0, &[JTMS])
        .counters(0, JTMS, Counter::Loyalty, 12)
        .hand(1, &["Test Bear", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 0, JTMS);
    activate(&mut g, jace, 3, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tgt_player(1)));
    resolve_with(&mut g, none);
    assert_eq!(g.exile_zone().len(), 10);
    assert_eq!(g.player(1).library.len(), 2);
    assert!(g.player(1).hand.is_empty());
}

// ---- Kaito, Bane of Nightmares ----------------------------------------------------

const KAITO: &str = "Kaito, Bane of Nightmares";

#[test]
fn kaito_ninjutsu_returns_an_unblocked_attacker_and_attacks_in_its_place() {
    // ruling: 5a72d756
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Island", "Swamp", "Swamp"])
        .hand(0, &[KAITO])
        .battlefield(1, &["Jace, the Mind Sculptor"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    let kaito = g.card(0, KAITO, Zone::Hand);
    g.choose(Choice::Card(bear));
    g.choose(g.tgt(jace));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Pass);
    // Not before blockers are declared (CR 509.1h: no unblocked creature).
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let ninjutsu = Choice::Ability(kaito, AbilitySlot::Hand(0));
    assert!(!g.has_option(ninjutsu));
    g.drive_until(|g| g.turn().step == Step::DeclareBlockers && at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(ninjutsu));
    g.choose(ninjutsu);
    pay(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1);
    assert!(g.knows(1, kaito), "revealed (CR 702.49b)");
    resolve_with(&mut g, none);
    let k = bf(&g, 0, KAITO);
    assert!(g.obj(k).tapped && g.obj(k).attacking);
    assert_eq!(loyalty(&g, k), 4);
    // CR 702.49c: attacking what the Bear attacked.
    assert_eq!(g.attack_dest(k), Some(Target::Obj(g.obj_ref(jace))));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.obj(jace).zone, Zone::Graveyard, "3 damage to Jace's 3 loyalty");
}

#[test]
fn kaito_is_a_hexproof_ninja_creature_during_his_turn_only() {
    let mut g = scen().battlefield(0, &[KAITO]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, KAITO);
    assert!(g.is_creature(k));
    assert!(g.has_kw(k, Kw::Hexproof));
    assert_eq!(g.pt(k), (3, 4));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    assert!(!g.is_creature(k) && !g.has_kw(k, Kw::Hexproof));
}

#[test]
fn kaito_emblem_surveil_draw_and_stun() {
    // +1: "Ninjas you control get +1/+1" (Kaito himself, during his turn).
    let mut g = scen().battlefield(0, &[KAITO]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, KAITO);
    activate(&mut g, k, 0, none);
    resolve_with(&mut g, none);
    assert_eq!(g.command_zone().len(), 1);
    assert_eq!(g.pt(k), (4, 5));
    // 0: surveil 2, then a card for each opponent who lost life this turn.
    for bolt_first in [false, true] {
        let mut g = scen()
            .battlefield(0, &[KAITO, "Mountain"])
            .hand(0, &["Lightning Bolt"])
            .at(Step::Main1, 0)
            .start();
        let k = bf(&g, 0, KAITO);
        if bolt_first {
            cast(&mut g, 0, "Lightning Bolt");
            g.choose(tgt_player(1));
            pay(&mut g);
            resolve_with(&mut g, none);
        }
        let hand = g.player(0).hand.len();
        activate(&mut g, k, 1, none);
        resolve_with(&mut g, |g| matches!(g.kind(), Some(DecisionKind::PutInGraveyard)).then_some(Choice::Pass));
        assert_eq!(g.player(0).hand.len(), hand + bolt_first as usize);
    }
    // -2: tap target creature, two stun counters.
    let mut g = scen().battlefield(0, &[KAITO]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, KAITO);
    let bear = bf(&g, 1, "Test Bear");
    // (Kaito is a creature himself during his turn.)
    activate(&mut g, k, 2, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    resolve_with(&mut g, none);
    assert!(g.obj(bear).tapped);
    assert_eq!(g.counters_on(bear, Counter::Stun), 2);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    assert!(g.obj(bear).tapped, "a stun counter removed instead (CR 122.1d)");
    assert_eq!(g.counters_on(bear, Counter::Stun), 1);
}
