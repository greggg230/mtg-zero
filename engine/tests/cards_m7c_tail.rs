//! M7c, the single-card tail part 1 (ENGINE.md 28): mana sources and the
//! cards needing one feature each. Test names cite the CR.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::mana::ManaCost;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    }).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn named(kind: &str) -> Counter {
    Counter::Named(db().subtype_id(&format!("counter:{kind}")).unwrap())
}

fn pool(g: &Game, p: PlayerId) -> [u8; 6] {
    g.player(p).pool.amt
}

fn opts(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
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

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
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
    panic!("quiet_until: no progress\n{}", g.log_lines().join("\n"));
}

// ---- Mana Vault ------------------------------------------------------------

/// "Doesn't untap during your untap step" (CR 502.3); "at the beginning of
/// your upkeep, you may pay {4}. If you do, untap it" (118.12-style optional
/// cost); "at the beginning of your draw step, if it is tapped, it deals 1
/// damage to you" (an intervening if, 603.4).
#[test]
fn mana_vault_stays_tapped_cr502_3_pays_4_to_untap_and_pings_if_tapped_cr603_4() {
    let mut g = scen()
        .battlefield(0, &["Mana Vault", "Island", "Island", "Island", "Island"])
        .hand(0, &["Test Zero"])
        .at(Step::Main1, 0)
        .start();
    let v = bf(&g, 0, "Mana Vault");
    assert!(g.can_pay(0, &cost("{7}")) && !g.can_pay(0, &cost("{8}")), "{{T}}: Add {{C}}{{C}}{{C}}");
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.choose(act(v, 0));
    settle(&mut g);
    assert!(g.obj(v).tapped);
    let t = g.turn().number;
    // Next upkeep: decline the {4}.
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.kind() == Some(DecisionKind::YesNo));
    assert_eq!((g.turn().active, g.turn().step), (0, Step::Upkeep));
    assert!(g.has_option(Choice::Yes), "four Islands pay {{4}}");
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1);
    assert!(g.obj(v).tapped, "it doesn't untap");
    assert_eq!(g.life(0), 19, "tapped at the draw step: 1 damage");
    // The upkeep after: pay {4}, it untaps; no damage.
    quiet_until(&mut g, |g| g.turn().number == t + 4 && g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    pay(&mut g);
    quiet_until(&mut g, |g| g.turn().number == t + 4 && g.turn().step == Step::Main1);
    assert!(!g.obj(v).tapped, "paid: untapped");
    assert_eq!(g.life(0), 19, "untapped at the draw step: no damage");
}

// ---- Coalition Relic -------------------------------------------------------

/// "{T}: Put a charge counter on this artifact. At the beginning of your
/// precombat main phase, remove all charge counters from it. Add one mana
/// of any color for each charge counter removed this way" (CR 106.4: each
/// mana its own colour).
#[test]
fn coalition_relic_charge_counters_become_mana_of_any_colours_cr106_4() {
    // (Main 2: the precombat-main trigger isn't waiting.)
    let mut g = scen().battlefield(0, &["Coalition Relic"]).at(Step::Main2, 0).start();
    let r = bf(&g, 0, "Coalition Relic");
    assert!(g.can_pay(0, &cost("{W}")) && g.can_pay(0, &cost("{G}")), "{{T}}: any colour");
    g.choose(act(r, 1));
    settle(&mut g);
    assert_eq!(g.counters_on(r, named("charge")), 1, "{}", g.log_lines().join("\n"));
    assert!(g.obj(r).tapped);
    let mut g = scen().battlefield(0, &["Coalition Relic"]).counters(0, "Coalition Relic", named("charge"), 2).at(Step::Upkeep, 0).start();
    let r = bf(&g, 0, "Coalition Relic");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseColor));
    assert_eq!(g.turn().step, Step::Main1);
    assert_eq!(opts(&g).len(), 5, "any colour");
    g.choose(Choice::Number(1));
    assert_eq!(opts(&g).len(), 4, "the second from blue on (each pair of colours once)");
    g.choose(Choice::Number(4));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0, 1, 0, 0, 1, 0]);
    assert_eq!(g.counters_on(r, named("charge")), 0, "all removed");
}

// ---- Chrome Mox ------------------------------------------------------------

/// Imprint (CR 607.2a linked abilities): "you may exile a nonartifact,
/// nonland card from your hand. {T}: Add one mana of any of the exiled
/// card's colors."
#[test]
fn chrome_mox_imprints_a_card_and_taps_for_its_colours_cr607_2a() {
    let mut g = scen().hand(0, &["Chrome Mox", "Lightning Bolt", "Island", "Mox Jet"]).at(Step::Main1, 0).start();
    let m = g.card(0, "Chrome Mox", Zone::Hand);
    g.choose(Choice::Cast(m));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseDiscard));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert_eq!(opts(&g), vec![Choice::Pass, Choice::Card(bolt)], "nonartifact, nonland only");
    g.choose(Choice::Card(bolt));
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert!(g.can_pay(0, &cost("{R}")) && !g.can_pay(0, &cost("{U}")), "the exiled card's colour");
    // Nothing imprinted: no mana.
    let mut g = scen().hand(0, &["Chrome Mox", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let m = g.card(0, "Chrome Mox", Zone::Hand);
    g.choose(Choice::Cast(m));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseDiscard));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert!(!g.can_pay(0, &cost("{1}")), "no exiled card: no colour, no mana");
}

// ---- City of Traitors ------------------------------------------------------

/// "When you play another land, sacrifice this land" (CR 305.1: playing a
/// land, not putting one onto the battlefield). "{T}: Add {C}{C}."
#[test]
fn city_of_traitors_is_sacrificed_when_you_play_another_land_cr305_1() {
    let mut g = scen().battlefield(0, &["City of Traitors"]).hand(0, &["Island", "City of Traitors"]).at(Step::Main1, 0).start();
    let city = bf(&g, 0, "City of Traitors");
    assert!(g.can_pay(0, &cost("{2}")) && !g.can_pay(0, &cost("{3}")));
    let island = g.card(0, "Island", Zone::Hand);
    g.choose(Choice::Play(island));
    settle(&mut g);
    assert_eq!(g.obj(city).zone, Zone::Graveyard, "another land played");
    // Playing a City doesn't sacrifice itself.
    let mut g = scen().hand(0, &["City of Traitors"]).at(Step::Main1, 0).start();
    let c = g.card(0, "City of Traitors", Zone::Hand);
    g.choose(Choice::Play(c));
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    // An opponent's land doesn't either.
    let mut g = scen().battlefield(0, &["City of Traitors"]).hand(1, &["Forest"]).at(Step::Main1, 1).start();
    let city = bf(&g, 0, "City of Traitors");
    let f = g.card(1, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    settle(&mut g);
    assert_eq!(g.obj(city).zone, Zone::Battlefield, "\"you play\"");
}

// ---- Gemstone Mine ---------------------------------------------------------

/// "This land enters with three mining counters on it. {T}, Remove a mining
/// counter: Add one mana of any color. If there are no mining counters on
/// this land, sacrifice it" (CR 122.6, 605.3b: all part of the ability).
#[test]
fn gemstone_mine_spends_a_mining_counter_per_mana_and_goes_at_zero_cr605_3b() {
    let mut g = scen().hand(0, &["Gemstone Mine", "Test Zero"]).at(Step::Main1, 0).start();
    let m = g.card(0, "Gemstone Mine", Zone::Hand);
    g.choose(Choice::Play(m));
    settle(&mut g);
    assert_eq!(g.counters_on(m, named("mining")), 3);
    assert!(g.can_pay(0, &cost("{B}")) && g.can_pay(0, &cost("{W}")));
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.choose(act(m, 0));
    settle(&mut g);
    assert_eq!(g.counters_on(m, named("mining")), 2);
    assert!(g.obj(m).tapped);
    // The last counter: sacrificed.
    let mut g = scen()
        .battlefield(0, &["Gemstone Mine"])
        .counters(0, "Gemstone Mine", named("mining"), 1)
        .hand(0, &["Test Zero"])
        .at(Step::Main1, 0)
        .start();
    let m = bf(&g, 0, "Gemstone Mine");
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.choose(act(m, 0));
    settle(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "no mining counters: sacrificed");
    assert_ne!(g.obj(z).zone, Zone::Hand, "its mana paid");
    // No counters: no mana.
    let g = scen().battlefield(0, &["Gemstone Mine"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{1}")), "a cost it can't pay");
}

// ---- Sunken Ruins, Loot, Orcish Lumberjack, Squandered Resources -----------

/// "{U/B}, {T}: Add {U}{U}, {U}{B}, or {B}{B}": a mana ability with a mana
/// cost, activated with priority (CR 605.3a; ENGINE.md 28), never while
/// paying another cost.
#[test]
fn sunken_ruins_filters_with_priority_cr605_3a() {
    let mut g = scen().battlefield(0, &["Sunken Ruins", "Island"]).at(Step::Main1, 0).start();
    let r = bf(&g, 0, "Sunken Ruins");
    assert!(g.can_pay(0, &cost("{C}{U}")), "{{T}}: Add {{C}}");
    assert!(!g.can_pay(0, &cost("{B}")), "not a source while paying");
    assert!(g.has_option(act(r, 1)));
    g.choose(act(r, 1));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{{U/B}} first");
    let c = opts(&g)[0];
    g.choose(c);
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana));
    assert_eq!(opts(&g).len(), 3, "UU, UB, BB");
    g.choose(Choice::Number(2));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0, 0, 2, 0, 0, 0]);
    assert!(g.obj(r).tapped);
}

/// Exhaust (CR 702.177a: "activate each exhaust ability only once"):
/// "{G}, {T}: Add three mana of any one color."
#[test]
fn loot_exhaust_mana_ability_only_once_cr702_177a() {
    let mut g = scen().battlefield(0, &["Loot, the Pathfinder", "Forest"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Loot, the Pathfinder");
    assert!(!g.can_pay(0, &cost("{R}")), "not a source while paying");
    g.choose(act(l, 0));
    let c = opts(&g)[0];
    g.choose(c);
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert_eq!(opts(&g).len(), 5);
    g.choose(Choice::Number(3));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0, 0, 0, 3, 0, 0]);
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.obj(l).tapped);
    assert!(!g.has_option(act(l, 0)), "exhausted");
    // Another Loot (a new object) has its own.
    let g = scen().battlefield(0, &["Loot, the Pathfinder", "Forest", "Island"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Loot, the Pathfinder");
    assert!(g.has_option(act(l, 0)) && g.has_option(act(l, 1)));
}

/// "{T}, Sacrifice a Forest: Add three mana in any combination of {R}
/// and/or {G}" (a choice in the cost: activated with priority).
#[test]
fn orcish_lumberjack_sacrifices_a_forest_for_three_cr605_3a() {
    let mut g = scen().battlefield(0, &["Orcish Lumberjack", "Forest", "Island"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Orcish Lumberjack");
    let f = bf(&g, 0, "Forest");
    g.choose(act(l, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    assert_eq!(opts(&g), vec![Choice::Card(f)], "a Forest");
    g.choose(Choice::Card(f));
    assert_eq!(opts(&g).len(), 4, "RRR, RRG, RGG, GGG");
    g.choose(Choice::Number(1));
    g.drive_until(at_priority);
    let p = pool(&g, 0);
    assert_eq!(p[3] + p[4], 3);
    assert_eq!(g.obj(f).zone, Zone::Graveyard);
    // No Forest: not offered.
    let g = scen().battlefield(0, &["Orcish Lumberjack", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Orcish Lumberjack"), 0)));
}

/// "Sacrifice a land: Add one mana of any type the sacrificed land could
/// produce" (CR 106.7).
#[test]
fn squandered_resources_mana_the_sacrificed_land_could_make_cr106_7() {
    let mut g = scen().battlefield(0, &["Squandered Resources", "Volcanic Island", "Wasteland"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, "Squandered Resources");
    let v = bf(&g, 0, "Volcanic Island");
    g.choose(act(s, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    assert_eq!(opts(&g).len(), 2);
    g.choose(Choice::Card(v));
    assert_eq!(opts(&g).len(), 2, "U or R");
    g.choose(Choice::Number(1));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0, 0, 0, 1, 0, 0]);
    assert_eq!(g.obj(v).zone, Zone::Graveyard);
    let w = bf(&g, 0, "Wasteland");
    g.choose(act(s, 0));
    g.choose(Choice::Card(w));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0, 0, 0, 1, 0, 1], "Wasteland: colorless only, no choice");
}

// ---- Palantir of Orthanc ---------------------------------------------------

/// "At the beginning of your end step, put an influence counter on it and
/// scry 2. Then target opponent may have you draw a card. If that player
/// doesn't, you mill X cards, X the influence counters, and that player
/// loses life equal to their total mana value" (the opponent decides, CR
/// 101.4).
#[test]
fn palantir_the_opponent_chooses_your_draw_or_your_mill_and_their_life_cr101_4() {
    let mut g = scen()
        .battlefield(0, &["Palantír of Orthanc"])
        .library(0, &["Lightning Bolt"; 10])
        .at(Step::Main2, 0)
        .start();
    let pal = bf(&g, 0, "Palantír of Orthanc");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.pending_decision().unwrap().player, 1, "the opponent decides");
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert_eq!(g.counters_on(pal, named("influence")), 1);
    assert_eq!(g.graveyard_cards(0).count(), 1, "mills X = 1");
    assert_eq!(g.life(1), 19, "a Bolt's mana value");
    let hand = g.player(0).hand.len();
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.kind() == Some(DecisionKind::YesNo) && g.turn().step == Step::End);
    g.choose(Choice::Yes);
    g.drive_until(at_priority);
    assert_eq!(g.counters_on(pal, named("influence")), 2);
    assert_eq!(g.player(0).hand.len(), hand + 2, "its draw step and the card drawn");
    assert_eq!(g.life(1), 19);
}

// ---- Tersa Lightshatter ----------------------------------------------------

/// "Whenever Tersa attacks, if there are seven or more cards in your
/// graveyard, exile a card at random from your graveyard. You may play that
/// card this turn" (an intervening if, CR 603.4; a random choice, a public
/// chance).
#[test]
fn tersa_exiles_a_random_graveyard_card_to_play_this_turn_cr603_4() {
    let gy = ["Lightning Bolt"; 7];
    let mut g = scen().battlefield(0, &["Tersa Lightshatter", "Mountain"]).graveyard(0, &gy).at(Step::DeclareAttackers, 0).start();
    let t = bf(&g, 0, "Tersa Lightshatter");
    g.choose(Choice::Card(t));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.graveyard_cards(0).count(), 6, "{}", g.log_lines().join("\n"));
    let ex = *g.exile_zone().last().unwrap();
    assert!(g.has_option(Choice::Cast(ex)) || g.has_option(Choice::Play(ex)), "may play it this turn");
    // Six cards: nothing.
    let mut g = scen().battlefield(0, &["Tersa Lightshatter"]).graveyard(0, &gy[..6]).at(Step::DeclareAttackers, 0).start();
    let t = bf(&g, 0, "Tersa Lightshatter");
    g.choose(Choice::Card(t));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.graveyard_cards(0).count(), 6);
}

// ---- Demonic Consultation --------------------------------------------------

/// "Choose a card name. Exile the top six cards of your library, then reveal
/// cards from the top until you reveal a card with the chosen name. Put that
/// card into your hand and exile all other cards revealed this way" (CR
/// 701.20a; a name of no card in the library exiles it all).
#[test]
fn demonic_consultation_names_exiles_six_then_reveals_until_the_name_cr701_20a() {
    // (The first card is the top.)
    let lib = ["Island", "Island", "Island", "Island", "Island", "Island", "Island", "Lightning Bolt", "Island", "Island"];
    let mut g = scen().library(0, &lib).hand(0, &["Demonic Consultation"]).battlefield(0, &["Swamp"]).at(Step::Main1, 0).start();
    let dc = g.card(0, "Demonic Consultation", Zone::Hand);
    g.choose(Choice::Cast(dc));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::NameCard));
    let bolt = db().id("Lightning Bolt").unwrap();
    assert!(g.has_option(Choice::Def(bolt)) && g.has_option(Choice::Pass));
    g.choose(Choice::Def(bolt));
    g.drive_until(at_priority);
    // The Bolt is the 8th from the top: six exiled, one revealed and exiled.
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1, "found after the six");
    assert_eq!(g.exile_zone().len(), 7);
    assert_eq!(g.player(0).library.len(), 12, "the ten Islands below are left");
    // A name not in the library: the whole library is exiled.
    let mut g = scen().library(0, &["Island"; 8]).hand(0, &["Demonic Consultation"]).battlefield(0, &["Swamp"]).at(Step::Main1, 0).start();
    let dc = g.card(0, "Demonic Consultation", Zone::Hand);
    g.choose(Choice::Cast(dc));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::NameCard));
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert!(g.player(0).library.is_empty());
    assert_eq!(g.exile_zone().len(), 18);
}

// ---- Balance ---------------------------------------------------------------

/// "Each player chooses a number of lands they control equal to the number
/// of lands controlled by the player who controls the fewest, then
/// sacrifices the rest. Players discard cards and sacrifice creatures the
/// same way" (choices in APNAP order, CR 101.4).
#[test]
fn balance_evens_lands_hands_and_creatures_cr101_4() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Test Bear"])
        .battlefield(1, &["Forest", "Forest", "Forest", "Forest", "Test Bear", "Test Bear", "Test Giant"])
        .hand(0, &["Balance"])
        .hand(1, &["Island", "Island", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, "Balance", Zone::Hand);
    g.choose(Choice::Cast(b));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g));
    // Lands: P1 keeps 0 (P0 has none left after paying? both Plains are
    // still P0's lands) -> P1 sacrifices two of four.
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    let first = opts(&g)[0];
    g.choose(first);
    assert_eq!(opts(&g).len(), 3, "a set: slots after the first pick");
    let second = opts(&g)[0];
    g.choose(second);
    // Hands: P0 has 0 cards, P1 discards all three.
    for _ in 0..3 {
        assert_eq!(g.kind(), Some(DecisionKind::Discard));
        let c = opts(&g)[0];
        g.choose(c);
    }
    // Creatures: P1 keeps one of three.
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let giant = bf(&g, 1, "Test Giant");
    let picks: Vec<Choice> = opts(&g);
    assert!(!picks.contains(&Choice::Card(giant)), "the last slot can't start a set of two");
    g.choose(picks[0]);
    let c = opts(&g)[0];
    g.choose(c);
    g.drive_until(at_priority);
    assert_eq!(g.permanents_of(1).filter(|&c| g.is_type(c, engine::db::types::LAND)).count(), 2);
    assert!(g.player(1).hand.is_empty());
    assert_eq!(g.creatures_of(1).count(), 1);
    assert_eq!(g.creatures_of(0).count(), 1);
}

// ---- Ursine Monstrosity ----------------------------------------------------

/// "At the beginning of combat on your turn, mill a card and choose an
/// opponent at random. This creature attacks that player this combat if
/// able (CR 508.1d). Until end of turn, it gains indestructible and gets
/// +1/+1 for each card type among cards in your graveyard" (205.2a).
#[test]
fn ursine_monstrosity_mills_must_attack_and_grows_by_card_types_cr508_1d() {
    let mut g = scen()
        .battlefield(0, &["Ursine Monstrosity"])
        .graveyard(0, &["Lightning Bolt", "Test Bear"])
        .library(0, &["Island"; 5])
        .at(Step::Main1, 0)
        .start();
    let u = bf(&g, 0, "Ursine Monstrosity");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    // Instant, creature, land: +3/+3; indestructible.
    assert_eq!((g.power(u), g.toughness(u)), (6, 6));
    assert!(g.has_kw(u, engine::db::Kw::Indestructible));
    assert!(g.obj(u).attacking, "it attacks if able (declared first, CR 508.1d)");
    assert_eq!(opts(&g), vec![Choice::Pass]);
}

// ---- Transmute Artifact ----------------------------------------------------

/// "Sacrifice an artifact. If you do, search your library for an artifact
/// card. If its mana value is at most the sacrificed one's, put it onto the
/// battlefield; if greater, you may pay the difference to do so, else put it
/// into its owner's graveyard. Then shuffle" (CR 701.19).
#[test]
fn transmute_artifact_trades_up_paying_the_difference_cr701_19() {
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Island", "Island", "Island", "Island"])
        .library(0, &["Mana Vault", "Coalition Relic", "Island", "Island"])
        .hand(0, &["Transmute Artifact"])
        .at(Step::Main1, 0)
        .start();
    let t = g.card(0, "Transmute Artifact", Zone::Hand);
    let sol = bf(&g, 0, "Sol Ring");
    g.choose(Choice::Cast(t));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g));
    assert_eq!(opts(&g), vec![Choice::Card(sol)]);
    g.choose(Choice::Card(sol));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}", g.log_lines().join("\n"));
    let relic = db().id("Coalition Relic").unwrap();
    g.choose(Choice::Def(relic));
    // {3} against {1}: pay {2}.
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    pay(&mut g);
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Coalition Relic", Zone::Battlefield), 1);
    assert_eq!(g.obj(sol).zone, Zone::Graveyard);
    // Declined: the graveyard.
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Island", "Island"])
        .library(0, &["Coalition Relic", "Island"])
        .hand(0, &["Transmute Artifact"])
        .at(Step::Main1, 0)
        .start();
    let t = g.card(0, "Transmute Artifact", Zone::Hand);
    let sol = bf(&g, 0, "Sol Ring");
    g.choose(Choice::Cast(t));
    // Pay {U}{U} with the Islands, keeping the Sol Ring.
    g.drive_until(|g| g.kind() != Some(DecisionKind::PayMana) || !g.has_option(act(sol, 0)));
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = *opts(&g).iter().find(|c| **c != act(sol, 0)).unwrap();
        g.choose(c);
    }
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Card(sol));
    g.choose(Choice::Def(relic));
    assert!(!g.has_option(Choice::Yes), "{{2}} can't be paid");
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Coalition Relic", Zone::Graveyard), 1);
    // A cheaper one enters for free.
    let mut g = scen()
        .battlefield(0, &["Coalition Relic", "Island", "Island"])
        .library(0, &["Sol Ring", "Island"])
        .hand(0, &["Transmute Artifact"])
        .at(Step::Main2, 0)
        .start();
    let t = g.card(0, "Transmute Artifact", Zone::Hand);
    let r = bf(&g, 0, "Coalition Relic");
    g.choose(Choice::Cast(t));
    while g.kind() == Some(DecisionKind::PayMana) || g.kind() == Some(DecisionKind::ChooseMana) {
        let c = *opts(&g).iter().find(|c| **c != act(r, 0)).unwrap();
        g.choose(c);
    }
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Card(r));
    g.choose(Choice::Def(db().id("Sol Ring").unwrap()));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Battlefield), 1);
}

// ---- Lim-Dul's Vault -------------------------------------------------------

/// "Look at the top five cards of your library. As many times as you choose,
/// you may pay 1 life, put those cards on the bottom of your library in any
/// order, then look at the top five. Then shuffle and put the last cards you
/// looked at this way on top in any order" (CR 119.4, 701.20).
#[test]
fn lim_duls_vault_pays_life_to_dig_then_stacks_the_top_cr119_4() {
    // (The first card is the top: the Bolt is in the second five.)
    let mut lib = vec!["Island"; 5];
    lib.extend(["Island", "Island", "Lightning Bolt", "Island", "Island"]);
    lib.extend(["Island"; 10]);
    let mut g = scen().library(0, &lib).hand(0, &["Lim-Dûl's Vault"]).battlefield(0, &["Underground Sea", "Underground Sea"]).at(Step::Main1, 0).start();
    let v = g.card(0, "Lim-Dûl's Vault", Zone::Hand);
    g.choose(Choice::Cast(v));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    assert_eq!(g.life(0), 19);
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "the next five");
    g.choose(Choice::Pass);
    // Order the five on top: the Bolt last (on top).
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    while g.kind() == Some(DecisionKind::PutOnTop) {
        let c = *opts(&g).iter().find(|&&c| c != Choice::Card(bolt)).unwrap_or(&Choice::Card(bolt));
        g.choose(c);
    }
    g.drive_until(at_priority);
    assert_eq!(*g.player(0).library.last().unwrap(), bolt, "on top");
    assert_eq!(g.player(0).library.len(), 30);
}

// ---- Malcolm, Alluring Scoundrel -------------------------------------------

/// "Whenever Malcolm deals combat damage to a player, put a chorus counter on
/// it. Draw a card, then discard a card. If there are four or more chorus
/// counters on Malcolm, you may cast the discarded card without paying its
/// mana cost" (CR 608.2g).
#[test]
fn malcolm_loots_and_casts_the_discard_free_at_four_chorus_counters_cr608_2g() {
    let mut g = scen()
        .battlefield(0, &["Malcolm, Alluring Scoundrel"])
        .counters(0, "Malcolm, Alluring Scoundrel", named("chorus"), 3)
        .hand(0, &["Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    g.choose(Choice::Card(m));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    assert_eq!(g.counters_on(m, named("chorus")), 4);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "four counters: may cast it");
    g.choose(Choice::Yes);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(1), 20 - 2 - 3, "combat damage and the free Bolt");
    // Fewer counters: no offer.
    let mut g = scen().battlefield(0, &["Malcolm, Alluring Scoundrel"]).hand(0, &["Lightning Bolt"]).at(Step::DeclareAttackers, 0).start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    g.choose(Choice::Card(m));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
}

// ---- IR defects ------------------------------------------------------------

/// Chromatic Star's IR triggers on leaving the battlefield (it would draw when
/// exiled or bounced); its Oracle text on being put into a graveyard (CR
/// 700.4-style): blocked (docs/ir_defects.md 15).
#[test]
fn chromatic_star_is_blocked_by_its_ir_event() {
    let err = engine::db::CardDb::cube().expect_id("Chromatic Star").unwrap_err();
    assert!(err.contains("IR:TriggeredAbility.event"), "{err}");
}
