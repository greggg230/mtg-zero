//! Lane C of the Legacy Doomsday cards (ENGINE.md 124.3) against their Oracle
//! text, the Comprehensive Rules and their official rulings (`// ruling:
//! <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card table
//! of constructed play only (`CardDb::constructed`).
//!
//! Edge of Autumn {1}{G}, Sorcery: "If you control four or fewer lands, search
//! your library for a basic land card, put it onto the battlefield tapped,
//! then shuffle. Cycling—Sacrifice a land. (Sacrifice a land, Discard this
//! card: Draw a card.)"
//!
//! Street Wraith {3}{B}{B}, Creature — Wraith 3/4: "Swampwalk (This creature
//! can't be blocked as long as defending player controls a Swamp.)
//! Cycling—Pay 2 life. (Pay 2 life, Discard this card: Draw a card.)"
//!
//! Leyline of the Void {2}{B}{B}, Enchantment: "If this card is in your
//! opening hand, you may begin the game with it on the battlefield. If a card
//! would be put into an opponent's graveyard from anywhere, exile it
//! instead."

use engine::db::{AbilitySlot, CardDb, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, GameSpec, PlayerId, Step, Target, Zone};

const EDGE: &str = "Edge of Autumn";
const WRAITH: &str = "Street Wraith";
const LEYLINE: &str = "Leyline of the Void";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

/// [`scen`] with empty libraries (each test gives its own).
fn bare() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn cycle(c: CardIdx) -> Choice {
    Choice::Ability(c, AbilitySlot::Hand(0))
}

/// Answer the decisions of a cast or an activation until priority: `answer`
/// first, else the first option.
fn pay_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}", log(g));
}

fn pay(g: &mut Game) {
    pay_with(g, |_| None);
}

/// Resolve the stack (passing priority), `answer` answering what is asked
/// (else the first option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
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
    panic!("resolve_with: no progress\n{}", log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Cast `name` from P`p`'s hand, paying with the first options, then resolve
/// everything, `answer` answering what is asked.
fn cast_and_resolve(g: &mut Game, p: PlayerId, name: &str, answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let c = hand(g, p, name);
    g.choose(Choice::Cast(c));
    pay_with(g, answer);
    resolve_with(g, answer);
}

fn names(g: &Game, cs: &[CardIdx]) -> Vec<String> {
    cs.iter().map(|&c| g.name(c).to_string()).collect()
}

fn in_exile(g: &Game, p: PlayerId, name: &str) -> usize {
    g.count_in(p, name, Zone::Exile)
}

fn in_gy(g: &Game, p: PlayerId, name: &str) -> usize {
    g.count_in(p, name, Zone::Graveyard)
}

// ---- the definitions -------------------------------------------------------------

#[test]
fn the_three_cards_are_defined_as_printed() {
    let d = db();
    let edge = d.def(d.id(EDGE).unwrap());
    assert_eq!(edge.mana_value(), 2);
    assert!(edge.spell.is_some());
    assert_eq!(edge.hand_abilities.len(), 1);
    let c = &edge.hand_abilities[0].cost;
    assert!(c.discard_self && c.mana.is_zero() && c.life == 0);
    let s = c.sacrifice.as_ref().expect("Sacrifice a land");
    assert_eq!((s.n, s.another), (1, false));

    let wraith = d.def(d.id(WRAITH).unwrap());
    assert_eq!(wraith.mana_value(), 5);
    assert!(wraith.keywords.has(Kw::Swampwalk));
    assert_eq!(wraith.hand_abilities.len(), 1);
    let c = &wraith.hand_abilities[0].cost;
    assert!(c.discard_self && c.mana.is_zero() && c.sacrifice.is_none());
    assert_eq!(c.life, 2);

    let leyline = d.def(d.id(LEYLINE).unwrap());
    assert_eq!(leyline.mana_value(), 4);
    assert!(leyline.opp_gy_exile && leyline.void_exile.is_none());
    let o = leyline.opening.as_ref().expect("an opening-hand action");
    assert!(!o.not_starting && o.counters.is_empty() && o.exile == 0);
}

#[test]
fn street_wraith_is_a_3_4_wraith() {
    let g = scen().battlefield(0, &[WRAITH]).start();
    let w = bf(&g, 0, WRAITH);
    assert_eq!(g.pt(w), (3, 4));
    assert!(g.has_kw(w, Kw::Swampwalk));
}

// ---- Street Wraith: cycling for 2 life (CR 702.29a, 119.4) -------------------------

/// "Cycling—Pay 2 life": activated from the hand at instant speed (no timing
/// restriction, CR 702.29a), on the opponent's turn too; the life and the
/// discard are the cost (602.2b, 601.2h), the draw is the ability's effect,
/// which can be responded to.
#[test]
fn cr_702_29a_street_wraith_cycles_for_2_life_at_instant_speed() {
    let mut g = scen().hand(0, &[WRAITH]).library(0, &["Swamp", "Island"]).at(Step::Upkeep, 1).start();
    assert!(at_priority(&g) && g.to_move() == Some(1));
    g.pass();
    // P0 has priority in P1's upkeep with an empty stack.
    assert_eq!(g.to_move(), Some(0), "{}", log(&g));
    let w = hand(&g, 0, WRAITH);
    assert!(g.has_option(cycle(w)), "{}", opts(&g));
    g.choose(cycle(w));
    pay(&mut g);
    // Paid: 2 life and the card, before the ability resolves.
    assert_eq!(g.life(0), 18);
    assert_eq!(in_gy(&g, 0, WRAITH), 1, "discarded as the cost");
    assert_eq!(g.stack().len(), 1, "the draw ability is on the stack");
    assert_eq!(g.player(0).hand.len(), 0);
    resolve(&mut g);
    assert_eq!(names(&g, &g.player(0).hand.clone()), vec!["Swamp"], "drew a card");
}

/// CR 119.4: a player may pay life only if their life total is at least the
/// amount; at 2 life Street Wraith cycles, to 0 life, and its controller
/// loses as state-based actions are next checked (704.5a), before the draw
/// resolves. At 1 life it can't be cycled.
#[test]
fn cr_119_4_street_wraith_cycles_at_2_life_and_its_controller_loses_at_0() {
    let mut g = scen().hand(0, &[WRAITH]).life(0, 2).start();
    let w = hand(&g, 0, WRAITH);
    assert!(g.has_option(cycle(w)), "{}", opts(&g));
    g.choose(cycle(w));
    pay(&mut g);
    assert_eq!(g.life(0), 0);
    assert!(g.is_over(), "{}", log(&g));
    assert_eq!(g.outcome().unwrap().winner, Some(1));
}

#[test]
fn cr_119_4_street_wraith_cant_be_cycled_at_1_life() {
    let g = scen().hand(0, &[WRAITH]).life(0, 1).start();
    let w = hand(&g, 0, WRAITH);
    assert!(!g.has_option(cycle(w)), "{}", opts(&g));
}

/// The discard is part of the cost (CR 702.29a): "whenever you discard"
/// abilities trigger as the cost is paid and go on the stack above the
/// cycling ability (Inti: exile the top card of your library), so they
/// resolve first.
#[test]
fn cr_702_29a_cycling_discards_as_a_cost_and_discard_triggers_resolve_first() {
    let mut g = scen()
        .battlefield(0, &["Inti, Seneschal of the Sun"])
        .hand(0, &[WRAITH])
        .library(0, &["Swamp", "Forest", "Plains"])
        .start();
    let w = hand(&g, 0, WRAITH);
    g.choose(cycle(w));
    pay(&mut g);
    assert_eq!(g.stack().len(), 2, "the cycling ability and Inti's trigger above it\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(in_exile(&g, 0, "Swamp"), 1, "Inti exiled the top card first\n{}", log(&g));
    assert_eq!(names(&g, &g.player(0).hand.clone()), vec!["Forest"], "then the cycling drew");
}

// ---- Street Wraith: swampwalk (CR 702.14) ------------------------------------------

/// P0 attacks with Street Wraith; P1 controls `lands` and a 2/2 that could
/// block it. Whether the blocker may block the Wraith.
fn wraith_blockable(p1_lands: &[&str], p0_lands: &[&str], blocker: &str) -> bool {
    let mut g = scen()
        .battlefield(0, &[WRAITH])
        .battlefield(0, p0_lands)
        .battlefield(1, p1_lands)
        .battlefield(1, &[blocker])
        .at(Step::DeclareAttackers, 0)
        .start();
    let w = bf(&g, 0, WRAITH);
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers), "{}", log(&g));
    g.choose(Choice::Card(w));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step > Step::DeclareBlockers);
    g.kind() == Some(DecisionKind::DeclareBlockers) && g.has_option(Choice::Card(w))
}

#[test]
fn cr_702_14c_swampwalk_cant_be_blocked_while_the_defender_controls_a_swamp() {
    let bear = "Simian Spirit Guide";
    // No Swamp: blockable.
    assert!(wraith_blockable(&["Island"], &[], bear));
    assert!(wraith_blockable(&["Volcanic Island"], &[], bear));
    // Any land with the land type Swamp (CR 702.14c, 205.3i): a basic Swamp,
    // a dual land, Undercity Sewers.
    assert!(!wraith_blockable(&["Swamp"], &[], bear));
    assert!(!wraith_blockable(&["Underground Sea"], &[], bear));
    assert!(!wraith_blockable(&["Undercity Sewers"], &[], bear));
    assert!(!wraith_blockable(&["Island", "Underground Sea"], &[], bear));
    // The defending player's lands, not the attacker's.
    assert!(wraith_blockable(&["Island"], &["Swamp", "Underground Sea"], bear));
}

/// CR 702.14d: landwalk abilities don't cancel one another: a blocker with
/// swampwalk of its own can't block a swampwalker either.
#[test]
fn cr_702_14d_a_blocker_with_swampwalk_doesnt_cancel_it() {
    assert!(!wraith_blockable(&["Swamp"], &[], WRAITH));
    assert!(wraith_blockable(&["Island"], &[], WRAITH));
}

/// Swampwalk only restricts blocking (CR 702.14b, 509.1b): without a Swamp
/// the Wraith is blocked and deals no damage to the player.
#[test]
fn cr_702_14c_a_blocked_street_wraith_deals_its_damage_to_the_blocker() {
    let mut g = scen().battlefield(0, &[WRAITH]).battlefield(1, &["Island", "Simian Spirit Guide"]).at(Step::DeclareAttackers, 0).start();
    let w = bf(&g, 0, WRAITH);
    g.choose(Choice::Card(w));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(w));
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
    assert_eq!(g.life(1), 20);
}

// ---- Edge of Autumn: the spell (CR 608.2c, 701.23) ---------------------------------

const BASICS: [&str; 6] = ["Island", "Swamp", "Underground Sea", "Forest", "Plains", "Mountain"];

/// The cards of P`p`'s library, top first.
fn library(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    let mut l = g.player(p).library.clone();
    l.reverse();
    l
}

fn offered_defs(g: &Game) -> Vec<String> {
    let mut v: Vec<String> = g
        .offered()
        .iter()
        .filter_map(|c| match c {
            Choice::Def(x) => Some(g.db().def(*x).name.clone()),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

fn offered_cards(g: &Game) -> Vec<CardIdx> {
    g.offered()
        .iter()
        .filter_map(|c| match c {
            Choice::Card(x) => Some(*x),
            _ => None,
        })
        .collect()
}

/// "If you control four or fewer lands, search your library for a basic land
/// card, put it onto the battlefield tapped, then shuffle": with four lands
/// as it resolves, a basic land card (not Underground Sea) enters tapped.
#[test]
fn edge_of_autumn_with_four_lands_puts_a_basic_land_onto_the_battlefield_tapped_cr_701_23() {
    let mut g = bare().battlefield(0, &["Forest", "Swamp", "Swamp", "Island"]).hand(0, &[EDGE]).library(0, &BASICS).library(1, &["Island"; 5]).start();
    let e = hand(&g, 0, EDGE);
    g.choose(Choice::Cast(e));
    pay(&mut g);
    g.pass_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}", log(&g));
    assert_eq!(offered_defs(&g), vec!["Forest", "Island", "Mountain", "Plains", "Swamp"], "basic land cards only");
    // CR 701.23b: a search for a card with a stated quality may fail to find.
    assert!(g.has_option(Choice::Pass));
    g.choose(Choice::Def(db().id("Plains").unwrap()));
    resolve(&mut g);
    let pl = bf(&g, 0, "Plains");
    assert!(g.obj(pl).tapped, "it enters tapped (CR 614.1c)");
    assert_eq!(in_gy(&g, 0, EDGE), 1);
    assert_eq!(g.player(0).library.len(), 5);
}

/// The number of lands is checked as it resolves (ruling 3079d582): with
/// five lands it does nothing: no search, no shuffle.
#[test]
fn edge_of_autumn_with_five_lands_does_nothing_and_doesnt_shuffle() {
    // ruling: 3079d582
    let mut g = scen().battlefield(0, &["Forest", "Swamp", "Swamp", "Island", "Island"]).hand(0, &[EDGE]).library(0, &BASICS).start();
    let before = library(&g, 0);
    cast_and_resolve(&mut g, 0, EDGE, |g| {
        assert_ne!(g.kind(), Some(DecisionKind::SearchLibrary), "no search with five lands");
        None
    });
    assert_eq!(library(&g, 0), before, "the library is not shuffled");
    assert_eq!(g.count_in(0, "Plains", Zone::Battlefield), 0);
    assert_eq!(in_gy(&g, 0, EDGE), 1);
}

/// Checked on resolution, not on casting (ruling 3079d582, CR 608.2c): cast
/// with five lands, then one sacrificed to another Edge of Autumn's cycling
/// in response: four lands as it resolves, so it searches.
#[test]
fn edge_of_autumn_counts_lands_on_resolution_after_a_cycling_sacrifice() {
    // ruling: 3079d582
    let mut g = scen()
        .battlefield(0, &["Forest", "Swamp", "Swamp", "Island", "Underground Sea"])
        .hand(0, &[EDGE, EDGE])
        .library(0, &["Plains", "Island", "Swamp"])
        .start();
    let e = hand(&g, 0, EDGE);
    g.choose(Choice::Cast(e));
    pay(&mut g);
    assert_eq!(g.to_move(), Some(0));
    let e2 = hand(&g, 0, EDGE);
    g.choose(cycle(e2));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice), "{}", log(&g));
    let sea = bf(&g, 0, "Underground Sea");
    g.choose(Choice::Card(sea));
    pay(&mut g);
    assert_eq!(in_gy(&g, 0, "Underground Sea"), 1);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(g.db().id("Swamp").unwrap())));
    assert_eq!(g.count_in(0, "Swamp", Zone::Battlefield), 3, "four lands on resolution: a Swamp was found\n{}", log(&g));
}

/// With no basic land card in the library the search finds nothing, and the
/// library is still shuffled (CR 701.23b: the search happened).
#[test]
fn edge_of_autumn_finds_nothing_without_a_basic_land_card() {
    let mut g = bare().battlefield(0, &["Forest", "Swamp"]).hand(0, &[EDGE]).library(0, &["Underground Sea", "Lightning Bolt"]).library(1, &["Island"; 5]).start();
    let e = hand(&g, 0, EDGE);
    g.choose(Choice::Cast(e));
    pay(&mut g);
    g.pass_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    assert_eq!(g.offered(), vec![Choice::Pass], "nothing to find");
    g.choose(Choice::Pass);
    resolve(&mut g);
    assert_eq!(g.permanents_of(0).count(), 2);
    assert!(g.log_lines().iter().any(|l| l.contains("finds nothing")));
}

// ---- Edge of Autumn: cycling by sacrificing a land (CR 702.29a, 118.3) -------------

/// "Cycling—Sacrifice a land": any land P0 controls, tapped or untapped (a
/// sacrifice cost doesn't need an untapped permanent, CR 118.3, 701.17a); at
/// instant speed; the draw on the stack.
#[test]
fn cr_702_29a_edge_of_autumn_cycles_by_sacrificing_any_land_tapped_or_not() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Underground Sea", "Polluted Delta"])
        .battlefield(1, &["Island"])
        .hand(0, &[EDGE])
        .library(0, &["Forest", "Plains"])
        .at(Step::Upkeep, 1)
        .start();
    let swamp = bf(&g, 0, "Swamp");
    g.set_tapped(swamp, true);
    g.pass();
    assert_eq!(g.to_move(), Some(0));
    let e = hand(&g, 0, EDGE);
    assert!(g.has_option(cycle(e)), "{}", opts(&g));
    g.choose(cycle(e));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let cards = offered_cards(&g);
    assert_eq!(names(&g, &cards), vec!["Swamp", "Underground Sea", "Polluted Delta"], "every land P0 controls, the tapped Swamp too");
    g.choose(Choice::Card(swamp));
    pay(&mut g);
    assert_eq!(in_gy(&g, 0, "Swamp"), 1);
    assert_eq!(in_gy(&g, 0, EDGE), 1);
    assert_eq!(g.stack().len(), 1);
    resolve(&mut g);
    assert_eq!(names(&g, &g.player(0).hand.clone()), vec!["Forest"]);
}

/// A sacrifice cost needs a land to sacrifice (CR 118.3): with none, Edge of
/// Autumn can't be cycled; with one, it can, the last land too.
#[test]
fn cr_118_3_edge_of_autumn_cycles_only_with_a_land_and_may_sacrifice_the_last() {
    let g = scen().battlefield(0, &["Lotus Petal"]).hand(0, &[EDGE]).start();
    let e = hand(&g, 0, EDGE);
    assert!(!g.has_option(cycle(e)), "no land: {}", opts(&g));

    let mut g = scen().battlefield(0, &["Island"]).hand(0, &[EDGE]).start();
    let e = hand(&g, 0, EDGE);
    g.choose(cycle(e));
    let island = bf(&g, 0, "Island");
    pay_with(&mut g, |g| (g.kind() == Some(DecisionKind::Sacrifice)).then_some(Choice::Card(island)));
    assert_eq!(g.permanents_of(0).count(), 0, "the last land sacrificed");
    resolve(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
}

/// The land may first be tapped for mana and then sacrificed (CR 601.2g
/// before 601.2h, through 602.2b: mana abilities are activated before costs
/// are paid; ENGINE.md 86.2): the mana stays in the pool.
#[test]
fn cr_602_2b_edge_of_autumn_sacrifices_a_land_just_tapped_for_mana() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &[EDGE, "Dark Ritual"]).start();
    let e = hand(&g, 0, EDGE);
    g.choose(cycle(e));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let swamp = bf(&g, 0, "Swamp");
    let tap_then_sac = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == swamp));
    let tap_then_sac = tap_then_sac.unwrap_or_else(|| panic!("tap the Swamp for mana first: {}", opts(&g)));
    g.choose(tap_then_sac);
    pay(&mut g);
    assert_eq!(in_gy(&g, 0, "Swamp"), 1);
    // The floating {B} casts Dark Ritual.
    let r = hand(&g, 0, "Dark Ritual");
    assert!(g.has_option(Choice::Cast(r)), "{{B}} floats: {}\n{}", opts(&g), log(&g));
}

/// Sacrificing a land a Wasteland targets, in response (CR 608.2b): the
/// Wasteland's ability has no legal target and does nothing; the fetch land
/// was sacrificed to the cycling, not destroyed.
#[test]
fn edge_of_autumn_cycles_away_a_fetch_land_a_wasteland_targets() {
    let mut g = scen()
        .battlefield(0, &["Polluted Delta", "Swamp"])
        .battlefield(1, &["Wasteland"])
        .hand(0, &[EDGE])
        .at(Step::Main1, 1)
        .start();
    let w = bf(&g, 1, "Wasteland");
    let delta = bf(&g, 0, "Polluted Delta");
    let wl = g
        .offered()
        .into_iter()
        .find(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(i)) if *x == w && *i > 0))
        .unwrap_or_else(|| panic!("Wasteland's ability: {}", opts(&g)));
    g.choose(wl);
    let t = g.tgt(delta);
    pay_with(&mut g, |g| g.has_option(t).then_some(t));
    // P0 responds.
    g.pass();
    assert_eq!(g.to_move(), Some(0), "{}", log(&g));
    let e = hand(&g, 0, EDGE);
    g.choose(cycle(e));
    pay_with(&mut g, |g| (g.kind() == Some(DecisionKind::Sacrifice)).then_some(Choice::Card(delta)));
    resolve(&mut g);
    assert_eq!(in_gy(&g, 0, "Polluted Delta"), 1);
    assert_eq!(g.count_in(0, "Swamp", Zone::Battlefield), 1, "nothing else destroyed");
    assert_eq!(g.player(0).hand.len(), 1, "the cycling drew");
}

// ---- Leyline of the Void: the opening-hand action (CR 103.6) ------------------------

/// A 60-card deck of `n` Leylines and Swamps.
fn leyline_deck(n: usize) -> Vec<u16> {
    let d = db();
    let (l, s) = (d.id(LEYLINE).unwrap(), d.id("Swamp").unwrap());
    (0..60).map(|i| if i < n { l } else { s }).collect()
}

/// P`p`'s Leylines in hand, in canonical order (the first is the one a
/// choice of the definition names, ENGINE.md 10).
fn leylines_in_hand(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    g.hand_sorted(p).into_iter().filter(|&c| g.name(c) == LEYLINE).collect()
}

/// "If this card is in your opening hand, you may begin the game with it on
/// the battlefield" (CR 103.6a): after the mulligans the starting player takes
/// such actions first, then the other player (ruling 5ef962e6), each with any
/// number of such cards, one per decision (`PASS` = no more).
#[test]
fn cr_103_6_leylines_begin_the_game_on_the_battlefield_the_starting_player_first() {
    // ruling: 5ef962e6
    let deck = leyline_deck(30);
    let mut done = 0;
    for seed in 0..30u64 {
        let spec = GameSpec { auto_single: false, mulligans: false, log: true, ..Default::default() };
        let mut g = Game::new(db(), spec, [&deck, &deck], seed).unwrap();
        g.play_first();
        let s = g.turn().starting;
        let o = 1 - s;
        if leylines_in_hand(&g, s).len() < 2 || leylines_in_hand(&g, o).is_empty() {
            continue;
        }
        // The starting player first.
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(s)), "{}", log(&g));
        let mine = leylines_in_hand(&g, s);
        assert!(g.has_option(Choice::Pass));
        // (Identical cards in hand are one choice, ENGINE.md 10.)
        assert!(g.has_option(Choice::Card(mine[0])), "{}", opts(&g));
        // Not a Swamp (no opening-hand action).
        let swamp = g.player(s).hand.iter().copied().find(|&c| g.name(c) == "Swamp");
        if let Some(sw) = swamp {
            assert!(!g.has_option(Choice::Card(sw)));
        }
        // Two of them, one decision each.
        g.choose(Choice::Card(mine[0]));
        assert_eq!(g.obj(mine[0]).zone, Zone::Battlefield);
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(s)), "asked again");
        let next = leylines_in_hand(&g, s)[0];
        g.choose(Choice::Card(next));
        g.choose(Choice::Pass);
        // Then the other player, who sees them.
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(o)), "{}", log(&g));
        let theirs = leylines_in_hand(&g, o);
        g.choose(Choice::Card(theirs[0]));
        g.choose(Choice::Pass);
        assert_eq!(g.turn().number, 1, "the first turn begins (CR 103.8)\n{}", log(&g));
        assert_eq!(g.count_in(s, LEYLINE, Zone::Battlefield), 2);
        assert_eq!(g.count_in(o, LEYLINE, Zone::Battlefield), 1);
        g.check_invariants().unwrap();
        done += 1;
        if done == 3 {
            break;
        }
    }
    assert!(done > 0);
}

/// "Opening hand" is the hand after the mulligans (ruling 5ef962e6; CR
/// 103.5, 103.6): a Leyline put on the bottom of the library by the London
/// mulligan is not offered; one kept in hand is.
#[test]
fn cr_103_6_a_leyline_put_on_the_bottom_by_a_mulligan_is_not_in_the_opening_hand() {
    // ruling: 5ef962e6
    let deck = leyline_deck(20);
    let mut done = 0;
    for seed in 0..40u64 {
        let spec = GameSpec { auto_single: false, mulligans: true, log: true, ..Default::default() };
        let mut g = Game::new(db(), spec, [&deck, &deck], seed).unwrap();
        g.play_first();
        let s = g.turn().starting;
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Mulligan), Some(s)));
        g.choose(Choice::Yes);
        g.choose(Choice::Pass); // the other player keeps
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::MulliganBottom), Some(s)));
        let ls = leylines_in_hand(&g, s);
        if ls.len() < 2 {
            g.choose(g.pending_decision().unwrap().options[0].choice);
            continue;
        }
        let bottom = ls[0];
        g.choose(Choice::Card(bottom));
        assert_eq!(g.obj(bottom).zone, Zone::Library);
        g.choose(Choice::Pass); // keep six
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(s)), "{}", log(&g));
        assert!(!g.has_option(Choice::Card(bottom)), "the bottomed Leyline: {}", opts(&g));
        let kept = leylines_in_hand(&g, s);
        assert!(!kept.contains(&bottom) && g.has_option(Choice::Card(kept[0])), "the kept one: {}", opts(&g));
        // Only Leylines (and PASS) are offered.
        assert_eq!(g.offered().len(), 2);
        done += 1;
        if done == 2 {
            break;
        }
    }
    assert!(done > 0);
}

// ---- Leyline of the Void: the replacement effect (CR 614.1a) ------------------------

/// P0 controls Leyline of the Void; P1 is active in its main phase.
fn leyline_scen() -> Scenario {
    scen().battlefield(0, &[LEYLINE]).at(Step::Main1, 1)
}

/// Pick the target `t` when asked, else the first option.
fn target(t: Choice) -> impl Fn(&Game) -> Option<Choice> + Copy {
    move |g: &Game| g.has_option(t).then_some(t)
}

/// The opponent's spells: a resolved instant and a countered one are exiled
/// (from the stack), the Leyline controller's own spells go to their own
/// graveyard ("an opponent's graveyard", CR 614.1a, 404.2).
#[test]
fn cr_614_1a_leyline_exiles_the_opponents_resolved_and_countered_spells() {
    let mut g = leyline_scen()
        .battlefield(1, &["Volcanic Island", "Volcanic Island"])
        .battlefield(0, &["Island", "Island"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .hand(0, &["Counterspell"])
        .start();
    // Resolved: exiled instead of put into P1's graveyard.
    let p0 = Choice::Target(Target::Player(0));
    cast_and_resolve(&mut g, 1, "Lightning Bolt", target(p0));
    assert_eq!(g.life(0), 17);
    assert_eq!(in_exile(&g, 1, "Lightning Bolt"), 1);
    assert_eq!(in_gy(&g, 1, "Lightning Bolt"), 0);
    // Countered: exiled too; P0's Counterspell goes to P0's graveyard.
    let b = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(b));
    pay_with(&mut g, target(p0));
    g.pass();
    assert_eq!(g.to_move(), Some(0));
    let cs = hand(&g, 0, "Counterspell");
    g.choose(Choice::Cast(cs));
    let bolt_on_stack = Choice::Target(Target::Obj(g.obj_ref(b)));
    pay_with(&mut g, target(bolt_on_stack));
    resolve(&mut g);
    assert_eq!(g.life(0), 17, "countered");
    assert_eq!(in_exile(&g, 1, "Lightning Bolt"), 2);
    assert_eq!(in_gy(&g, 0, "Counterspell"), 1, "the Leyline's controller's own card");
}

/// Discard (CR 701.9): the opponent's discarded card is exiled; it was still
/// discarded (ruling e64b1395): "whenever you discard" abilities trigger
/// (Inti), and Street Wraith cycled under the opponent's Leyline is exiled.
#[test]
fn cr_614_1a_leyline_exiles_discarded_cards_which_were_still_discarded() {
    // ruling: e64b1395
    let mut g = scen()
        .battlefield(0, &[LEYLINE, "Swamp"])
        .battlefield(1, &["Inti, Seneschal of the Sun"])
        .hand(1, &[WRAITH, "Lightning Bolt"])
        .hand(0, &["Thoughtseize"])
        .library(1, &["Plains", "Forest", "Mountain"])
        .at(Step::Main1, 1)
        .start();
    let w = hand(&g, 1, WRAITH);
    g.choose(cycle(w));
    pay(&mut g);
    assert_eq!(in_exile(&g, 1, WRAITH), 1, "cycled: exiled, not in P1's graveyard");
    assert_eq!(g.stack().len(), 2, "Inti's discard trigger fired\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(in_exile(&g, 1, "Plains"), 1, "Inti exiled the top card");
    assert_eq!(g.life(1), 18);
    // P0's Thoughtseize on P1's turn? It's a sorcery: on P0's turn.
    let mut g = scen()
        .battlefield(0, &[LEYLINE, "Swamp"])
        .hand(1, &["Lightning Bolt", "Island"])
        .hand(0, &["Thoughtseize"])
        .start();
    let bolt = hand(&g, 1, "Lightning Bolt");
    cast_and_resolve(&mut g, 0, "Thoughtseize", |g| {
        if g.has_option(Choice::Target(Target::Player(1))) {
            Some(Choice::Target(Target::Player(1)))
        } else {
            g.has_option(Choice::Card(bolt)).then_some(Choice::Card(bolt))
        }
    });
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(in_gy(&g, 0, "Thoughtseize"), 1);
    assert_eq!(g.life(0), 18);
}

/// "Nontoken creatures your opponents control won't die. They'll be exiled
/// instead. Abilities that would trigger when those creatures die won't
/// trigger" (ruling ef9cd497; CR 700.4): Sanguine Evangelist makes no Bat.
/// "Tokens can still die" (ruling 4226f05f): a Pest token's dies trigger
/// gains its controller 1 life.
#[test]
fn cr_700_4_under_leyline_opposing_creature_cards_dont_die_but_tokens_do() {
    // ruling: ef9cd497
    // ruling: 4226f05f
    let mut g = scen()
        .battlefield(0, &[LEYLINE, "Mountain", "Mountain"])
        .battlefield(1, &["Sanguine Evangelist", "Forest", "Forest", "Forest"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .hand(1, &["Pest Infestation"])
        .at(Step::Main1, 1)
        .start();
    // P1 makes two Pests (X = 1, no target).
    let pi = hand(&g, 1, "Pest Infestation");
    g.choose(Choice::Cast(pi));
    pay_with(&mut g, |g| {
        if g.has_option(Choice::Number(1)) {
            Some(Choice::Number(1))
        } else {
            g.has_option(Choice::Pass).then_some(Choice::Pass)
        }
    });
    resolve(&mut g);
    assert_eq!(in_exile(&g, 1, "Pest Infestation"), 1);
    let pests: Vec<CardIdx> = g.permanents_of(1).filter(|&c| g.name(c) == "Pest").collect();
    assert_eq!(pests.len(), 2, "{}", log(&g));
    // P0 bolts the Evangelist and a Pest at instant speed.
    let ev = bf(&g, 1, "Sanguine Evangelist");
    g.pass_until(|g| g.to_move() == Some(0));
    let t = g.tgt(ev);
    let b = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(b));
    pay_with(&mut g, target(t));
    resolve(&mut g);
    assert_eq!(g.obj(ev).zone, Zone::Exile, "exiled, didn't die\n{}", log(&g));
    assert_eq!(g.permanents_of(1).filter(|&c| g.name(c) == "Bat").count(), 0, "no dies trigger: no Bat");
    let life = g.life(1);
    g.pass_until(|g| g.to_move() == Some(0));
    let t = g.tgt(pests[0]);
    let b = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(b));
    pay_with(&mut g, target(t));
    resolve(&mut g);
    assert_eq!(g.life(1), life + 1, "the token died: its trigger gained 1 life\n{}", log(&g));
    assert_eq!(g.permanents_of(1).filter(|&c| g.name(c) == "Pest").count(), 1);
}

/// From the library (surveil, CR 701.25) and the battlefield (sacrifice, CR
/// 701.17): Consider's surveilled card and the opponent's sacrificed
/// Chromatic Star are exiled; the Star's "when this is put into a graveyard
/// from the battlefield" doesn't trigger (CR 603.6c... its event never
/// happened: CR 614.6).
#[test]
fn cr_614_6_leyline_exiles_surveilled_and_sacrificed_cards_and_their_triggers_dont_happen() {
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .at(Step::Main1, 1)
        .battlefield(1, &["Chromatic Star", "Swamp"])
        .hand(1, &["Consider"])
        .library(1, &["Lightning Bolt", "Plains", "Forest"])
        .library(0, &["Island"; 5])
        .start();
    let star = bf(&g, 1, "Chromatic Star");
    // Consider's {U} from the Star (its {1} from the Swamp): the Star is
    // sacrificed. Surveil the Bolt into the "graveyard".
    let c = hand(&g, 1, "Consider");
    g.choose(Choice::Cast(c));
    pay(&mut g);
    assert_eq!(g.obj(star).zone, Zone::Exile, "the sacrificed Star\n{}", log(&g));
    resolve_with(&mut g, |g| {
        let bolt = g.find_card(1, "Lightning Bolt", Zone::Library)?;
        g.has_option(Choice::Card(bolt)).then_some(Choice::Card(bolt))
    });
    assert_eq!(in_exile(&g, 1, "Lightning Bolt"), 1, "surveilled into exile\n{}", log(&g));
    assert_eq!(in_exile(&g, 1, "Consider"), 1);
    // Consider drew one card (Plains); the Star's draw never triggered.
    assert_eq!(names(&g, &g.player(1).hand.clone()), vec!["Plains"], "{}", log(&g));
}

/// Lion's Eye Diamond: its sacrifice and the discarded hand (a cost) are all
/// exiled; Brainstorm's put-back goes to the library, unaffected (no
/// graveyard involved).
#[test]
fn leyline_exiles_lion_s_eye_diamond_and_the_discarded_hand_but_not_brainstorm_s_put_back() {
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .at(Step::Main1, 1)
        .battlefield(1, &["Lion's Eye Diamond", "Island"])
        .hand(1, &["Brainstorm", "Lightning Bolt", "Swamp"])
        .library(1, &["Plains", "Forest", "Mountain", "Island"])
        .library(0, &["Island"; 5])
        .start();
    let bs = hand(&g, 1, "Brainstorm");
    cast_and_resolve(&mut g, 1, "Brainstorm", |_| None);
    assert_eq!(in_exile(&g, 1, "Brainstorm"), 1);
    assert_eq!(g.player(1).library.len(), 3, "two put back: in the library");
    let _ = bs;
    // LED with priority: sacrifice it and discard the hand for three mana.
    let led = bf(&g, 1, "Lion's Eye Diamond");
    let act = g
        .offered()
        .into_iter()
        .find(|c| matches!(c, Choice::Ability(x, _) if *x == led))
        .unwrap_or_else(|| panic!("LED's ability: {}", opts(&g)));
    let n = g.player(1).hand.len();
    g.choose(act);
    pay(&mut g);
    assert_eq!(g.obj(led).zone, Zone::Exile);
    assert_eq!(g.player(1).hand.len(), 0);
    assert_eq!(g.player(1).graveyard.len(), 0);
    assert_eq!(g.exile_zone().iter().filter(|&&c| g.obj(c).owner == 1).count(), n + 2, "the hand, LED and Brainstorm");
}

/// A fetch land's sacrifice (its cost) is exiled; the opponent's graveyard
/// stays empty, so delve (Murktide Regent) has nothing to exile.
#[test]
fn leyline_exiles_a_sacrificed_fetch_land() {
    let mut g = leyline_scen().battlefield(1, &["Polluted Delta"]).library(1, &["Island", "Swamp"]).start();
    let delta = bf(&g, 1, "Polluted Delta");
    let act = g
        .offered()
        .into_iter()
        .find(|c| matches!(c, Choice::Ability(x, _) if *x == delta))
        .unwrap_or_else(|| panic!("the fetch: {}", opts(&g)));
    g.choose(act);
    pay(&mut g);
    resolve(&mut g);
    assert_eq!(g.obj(delta).zone, Zone::Exile);
    assert!(g.player(1).graveyard.is_empty());
    assert_eq!(g.life(1), 19);
}

/// Two Leylines of one player: both would apply, every order gives the same
/// result (exile), so there is nothing to choose (CR 616.1). Each player's
/// Leyline exiles the other's cards.
#[test]
fn cr_616_1_two_leylines_exile_without_a_choice_and_each_side_has_its_own() {
    let mut g = scen()
        .battlefield(0, &[LEYLINE, LEYLINE, "Mountain"])
        .battlefield(1, &[LEYLINE, "Volcanic Island"])
        .hand(1, &["Lightning Bolt"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let p0 = Choice::Target(Target::Player(0));
    cast_and_resolve(&mut g, 1, "Lightning Bolt", |g| {
        assert_ne!(g.kind(), Some(DecisionKind::ChooseReplacement));
        target(p0)(g)
    });
    assert_eq!(in_exile(&g, 1, "Lightning Bolt"), 1);
    g.pass_until(|g| g.to_move() == Some(0));
    let p1 = Choice::Target(Target::Player(1));
    let b = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(b));
    pay_with(&mut g, target(p1));
    resolve(&mut g);
    assert_eq!(in_exile(&g, 0, "Lightning Bolt"), 1, "P1's Leyline exiles P0's Bolt");
    assert_eq!((g.life(0), g.life(1)), (17, 17));
}

/// Leyline of the Void and Dauthi Voidwalker (same controller) both apply to
/// a discarded card; their results differ (a void counter), so the affected
/// card's owner chooses which applies (CR 616.1, 616.1e).
#[test]
fn cr_616_1_leyline_and_dauthi_voidwalker_the_owner_chooses() {
    let mut g = scen()
        .battlefield(0, &[LEYLINE, "Dauthi Voidwalker", "Swamp"])
        .hand(1, &["Lightning Bolt", "Island"])
        .hand(0, &["Thoughtseize"])
        .start();
    let bolt = hand(&g, 1, "Lightning Bolt");
    let th = hand(&g, 0, "Thoughtseize");
    g.choose(Choice::Cast(th));
    pay_with(&mut g, target(Choice::Target(Target::Player(1))));
    g.pass_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Card(bolt));
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseReplacement), Some(1)), "{}", log(&g));
    let mut ks: Vec<Choice> = g.offered();
    ks.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(ks, vec![Choice::Number(2), Choice::Number(3)], "exile, or exile with a void counter");
    g.choose(Choice::Number(2));
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert_eq!(g.counters_on(bolt, Counter::Named(db().subtype_id("counter:void").unwrap())), 0, "Leyline's: no void counter");
}

/// Leyline and Blightsteel Colossus's own "shuffle it into its owner's
/// library instead": the owner chooses (CR 616.1).
#[test]
fn cr_616_1_leyline_and_blightsteel_the_owner_chooses_library_or_exile() {
    let mut g = scen()
        .battlefield(0, &[LEYLINE, "Swamp"])
        .hand(1, &["Blightsteel Colossus", "Island"])
        .hand(0, &["Thoughtseize"])
        .start();
    let bc = hand(&g, 1, "Blightsteel Colossus");
    let th = hand(&g, 0, "Thoughtseize");
    g.choose(Choice::Cast(th));
    pay_with(&mut g, target(Choice::Target(Target::Player(1))));
    g.pass_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Card(bc));
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseReplacement), Some(1)), "{}", log(&g));
    let mut ks: Vec<Choice> = g.offered();
    ks.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(ks, vec![Choice::Number(1), Choice::Number(2)], "the library or exile");
    g.choose(Choice::Number(1));
    resolve(&mut g);
    assert_eq!(g.obj(bc).zone, Zone::Library);
}

/// Yawgmoth's Will's "exile that card instead" and the opponent's Leyline:
/// the same result, no choice.
#[test]
fn cr_616_1_leyline_and_yawgmoths_will_exile_without_a_choice() {
    let mut g = leyline_scen()
        .battlefield(1, &["Underground Sea", "Underground Sea", "Underground Sea", "Volcanic Island"])
        .hand(1, &["Yawgmoth's Will", "Lightning Bolt"])
        .start();
    cast_and_resolve(&mut g, 1, "Yawgmoth's Will", |g| {
        assert_ne!(g.kind(), Some(DecisionKind::ChooseReplacement));
        None
    });
    assert_eq!(in_exile(&g, 1, "Yawgmoth's Will"), 1);
    let p0 = Choice::Target(Target::Player(0));
    cast_and_resolve(&mut g, 1, "Lightning Bolt", |g| {
        assert_ne!(g.kind(), Some(DecisionKind::ChooseReplacement));
        target(p0)(g)
    });
    assert_eq!(in_exile(&g, 1, "Lightning Bolt"), 1);
}

/// Edge of Autumn's cycling under the opponent's Leyline: the sacrificed land
/// is exiled, so Titania ("whenever a land you control is put into a
/// graveyard from the battlefield") doesn't trigger; without the Leyline it
/// does.
#[test]
fn edge_of_autumn_cycling_under_leyline_doesnt_trigger_titania() {
    for leyline in [false, true] {
        let mut s = scen().battlefield(1, &["Titania, Protector of Argoth", "Forest", "Forest"]).hand(1, &[EDGE]).at(Step::Main1, 1);
        if leyline {
            s = s.battlefield(0, &[LEYLINE]);
        }
        let mut g = s.start();
        let e = hand(&g, 1, EDGE);
        g.choose(cycle(e));
        pay(&mut g);
        resolve(&mut g);
        let elementals = g.permanents_of(1).filter(|&c| g.name(c) == "Elemental").count();
        assert_eq!(elementals, if leyline { 0 } else { 1 }, "leyline {leyline}\n{}", log(&g));
        assert_eq!(in_exile(&g, 1, "Forest"), leyline as usize);
        assert_eq!(in_exile(&g, 1, EDGE), leyline as usize);
    }
}

/// A permanent leaving at the same time as the Leyline: the replacement
/// exists as the event begins (CR 614.1a, 603.10a): Pest Infestation
/// destroying P0's Leyline and P1's own Chromatic Star together exiles the
/// Star; the Leyline goes to its owner's graveyard.
#[test]
fn cr_614_1a_a_card_destroyed_together_with_the_leyline_is_still_exiled() {
    // (Both target orders: the Leyline destroyed before or after the Star
    // within the one event.)
    for leyline_first in [false, true] {
        destroyed_together(leyline_first);
    }
}

fn destroyed_together(leyline_first: bool) {
    let mut g = leyline_scen()
        .battlefield(1, &["Chromatic Star", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .hand(1, &["Pest Infestation"])
        .start();
    let star = bf(&g, 1, "Chromatic Star");
    let ley = bf(&g, 0, LEYLINE);
    let pi = hand(&g, 1, "Pest Infestation");
    g.choose(Choice::Cast(pi));
    let (ts, tl) = (g.tgt(star), g.tgt(ley));
    let (t1, t2) = if leyline_first { (tl, ts) } else { (ts, tl) };
    pay_with(&mut g, |g| {
        if g.has_option(Choice::Number(2)) {
            Some(Choice::Number(2))
        } else if g.has_option(t1) {
            Some(t1)
        } else if g.has_option(t2) {
            Some(t2)
        } else {
            // (Paid with the Forests: not the Star.)
            g.offered().into_iter().find(|c| !matches!(c, Choice::Ability(x, _) | Choice::Card(x) if *x == star))
        }
    });
    assert_eq!(g.obj(star).zone, Zone::Battlefield, "{}", log(&g));
    resolve(&mut g);
    if std::env::var("DDC_LOG").is_ok() {
        println!("leyline_first {leyline_first}:
{}", log(&g));
    }
    assert_eq!(g.obj(ley).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(star).zone, Zone::Exile, "destroyed with the Leyline: exiled\n{}", log(&g));
    assert_eq!(in_exile(&g, 1, "Pest Infestation"), 0, "the Leyline was gone as the spell went to the graveyard");
    assert_eq!(in_gy(&g, 1, "Pest Infestation"), 1);
}

/// The opening-hand question is asked of a player whose deck holds such a
/// card (open decklists, ENGINE.md 54.5): a deck without Leylines is never
/// asked.
#[test]
fn cr_103_6_a_deck_without_leylines_is_not_asked() {
    let with = leyline_deck(30);
    let without = leyline_deck(0);
    for seed in 0..10u64 {
        let spec = GameSpec { auto_single: false, mulligans: false, log: true, ..Default::default() };
        let mut g = Game::new(db(), spec, [&with, &without], seed).unwrap();
        g.play_first();
        while g.turn().number == 0 {
            assert_ne!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(1)), "{}", log(&g));
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
        assert_eq!(g.count_in(1, LEYLINE, Zone::Battlefield), 0);
    }
}

/// Connive's "if you discarded a nonland card" reads the discarded card's
/// characteristics where it went, exile (ruling e64b1395): Norman Osborn,
/// conniving under the opponent's Leyline, discards Lightning Bolt into exile
/// and gets its +1/+1 counter.
#[test]
fn connive_under_leyline_reads_the_exiled_discarded_card() {
    // ruling: e64b1395
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .battlefield(1, &["Norman Osborn // Green Goblin"])
        .library(1, &["Lightning Bolt", "Island"])
        .library(0, &["Island"; 5])
        .at(Step::DeclareAttackers, 1)
        .start();
    let n = g.permanents_of(1).find(|&c| g.name(c).starts_with("Norman")).unwrap();
    g.choose(Choice::Card(n));
    g.choose(Choice::Pass);
    let bolt = g.card(1, "Lightning Bolt", Zone::Library);
    for _ in 0..200 {
        if g.counters_on(n, Counter::P1P1) > 0 || g.turn().step >= Step::EndCombat {
            break;
        }
        let c = if g.has_option(Choice::Card(bolt)) && g.obj(bolt).zone == Zone::Hand {
            Choice::Card(bolt)
        } else if at_priority(&g) {
            Choice::Pass
        } else {
            g.pending_decision().unwrap().options[0].choice
        };
        g.choose(c);
    }
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "discarded into exile\n{}", log(&g));
    assert_eq!(g.counters_on(n, Counter::P1P1), 1, "a nonland card was discarded\n{}", log(&g));
}

/// Delve under the opponent's Leyline: the cantrip went to exile, the
/// graveyard is empty, so Murktide Regent has nothing to delve.
#[test]
fn murktide_regent_has_nothing_to_delve_under_the_opponents_leyline() {
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .at(Step::Main1, 1)
        .battlefield(1, &["Volcanic Island"; 8])
        .hand(1, &["Consider", "Murktide Regent"])
        .library(1, &["Island"; 5])
        .library(0, &["Island"; 5])
        .start();
    cast_and_resolve(&mut g, 1, "Consider", |_| None);
    assert!(g.player(1).graveyard.is_empty());
    let m = hand(&g, 1, "Murktide Regent");
    g.choose(Choice::Cast(m));
    pay_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
            assert_eq!(g.offered(), vec![Choice::Pass], "nothing to delve");
        }
        None
    });
    resolve(&mut g);
    let m = bf(&g, 1, "Murktide Regent");
    assert_eq!(g.counters_on(m, Counter::P1P1), 0);
}

/// Copies of a spell are not cards (CR 707.10a, 108.2): a replicated Lose
/// Focus's copy ceases to exist as it leaves the stack, not exiled; the
/// original card is exiled by the opponent's Leyline. The countered Lightning
/// Bolt is the Leyline controller's own: their graveyard.
#[test]
fn cr_707_10a_a_spell_copy_is_not_exiled_by_leyline_the_card_is() {
    let mut g = bare()
        .battlefield(0, &[LEYLINE, "Mountain"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Lose Focus"])
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .start();
    let bolt = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    pay_with(&mut g, target(Choice::Target(Target::Player(1))));
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    let lf = hand(&g, 1, "Lose Focus");
    g.choose(Choice::Cast(lf));
    let tb = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    let answer = move |g: &Game| {
        if g.has_option(Choice::Number(1)) && g.kind() != Some(DecisionKind::PayMana) {
            Some(Choice::Number(1))
        } else if g.has_option(tb) {
            Some(tb)
        } else if g.to_move() == Some(0) && g.has_option(Choice::Pass) && !at_priority(g) {
            Some(Choice::Pass)
        } else {
            None
        }
    };
    pay_with(&mut g, answer);
    resolve_with(&mut g, answer);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "countered; P0's own card\n{}", log(&g));
    assert_eq!(g.life(1), 20);
    assert_eq!(in_exile(&g, 1, "Lose Focus"), 1, "the card only, not its copy\n{}", log(&g));
    assert!(g.log_lines().iter().any(|l| l.contains("copies Lose Focus")), "a copy was made\n{}", log(&g));
}

/// More of the Delver and Doomsday decks' paths to a graveyard under the
/// opponent's Leyline: the discard to hand size in the cleanup step (CR
/// 514.1), the legend rule (704.5j), Force of Will's resolution (its pitched
/// card is exiled as a cost, 118.1, not put into a graveyard) and a Lotus
/// Petal sacrificed for mana.
#[test]
fn leyline_exiles_cleanup_discards_legend_rule_losers_and_force_of_will() {
    // The cleanup discard (P1 active with nine cards).
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .hand(1, &["Island"; 9])
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .at(Step::End, 1)
        .start();
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard) || g.turn().active == 0);
    while g.kind() == Some(DecisionKind::Discard) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.player(1).hand.len(), 7, "{}", log(&g));
    assert_eq!(in_exile(&g, 1, "Island"), 2);
    assert!(g.player(1).graveyard.is_empty());

    // The legend rule: P1's second Thalia.
    let mut g = bare()
        .battlefield(0, &[LEYLINE])
        .battlefield(1, &["Thalia, Guardian of Thraben", "Thalia, Guardian of Thraben"])
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .at(Step::Main1, 1)
        .start();
    g.drive_until(at_priority);
    if g.kind() == Some(DecisionKind::LegendRule) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(in_exile(&g, 1, "Thalia, Guardian of Thraben"), 1, "{}", log(&g));

    // Force of Will, cast for its alternative cost (exile a blue card),
    // countering P0's Lightning Bolt; P1's Lotus Petal sacrificed for mana
    // first.
    let mut g = bare()
        .battlefield(0, &[LEYLINE, "Mountain"])
        .battlefield(1, &["Lotus Petal"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Force of Will", "Brainstorm", "Ponder"])
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .start();
    // P1 floats the Petal's mana? Not needed: the Petal is sacrificed below
    // to pay Ponder's {U} on P1's own turn later; here, Force of Will.
    let bolt = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    pay_with(&mut g, target(Choice::Target(Target::Player(1))));
    g.pass();
    let fow = hand(&g, 1, "Force of Will");
    g.choose(Choice::Cast(fow));
    let tb = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    let bs = hand(&g, 1, "Brainstorm");
    pay_with(&mut g, |g| {
        if g.has_option(tb) {
            Some(tb)
        } else if g.kind() == Some(DecisionKind::ChooseAltCost) {
            // The alternative cost: pay 1 life, exile a blue card.
            g.offered().into_iter().find(|c| *c != Choice::Pass && !matches!(c, Choice::Number(0)))
        } else if g.has_option(Choice::Card(bs)) {
            Some(Choice::Card(bs))
        } else {
            None
        }
    });
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "countered: P0's own card\n{}", log(&g));
    assert_eq!(g.life(1), 19, "Force of Will's 1 life\n{}", log(&g));
    assert_eq!(in_exile(&g, 1, "Force of Will"), 1, "resolved: exiled by the Leyline");
    assert_eq!(in_exile(&g, 1, "Brainstorm"), 1, "exiled as the cost");
    assert!(g.player(1).graveyard.is_empty());
    // Ponder paid by sacrificing the Lotus Petal (on P1's turn).
    g.pass_until(|g| g.turn().active == 1 && g.turn().step == Step::Main1);
    g.drive_until(|g| at_priority(g) && g.turn().active == 1 && g.turn().step == Step::Main1);
    let petal = bf(&g, 1, "Lotus Petal");
    cast_and_resolve(&mut g, 1, "Ponder", |_| None);
    assert_eq!(g.obj(petal).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(in_exile(&g, 1, "Ponder"), 1);
}

/// A replacement effect of a permanent leaving in the same event exists as
/// the event begins, with the permanent's abilities as they are then (CR
/// 614.1a, 613.1f): Dauthi Voidwalker destroyed by Wrath of God with the
/// opponent's creatures exiles them with void counters; one that had lost all
/// abilities (The Wondrous Wasp) exiles nothing (`Lki::void_exile`, ENGINE.md
/// 124.3: before, the leaving Voidwalker's printed definition was read).
#[test]
fn cr_614_1a_a_voidwalker_leaving_with_the_creatures_uses_its_abilities_as_they_were() {
    for wasp in [false, true] {
        let mut g = bare()
            .battlefield(0, &["Dauthi Voidwalker"])
            .battlefield(1, &["Simian Spirit Guide", "Plains", "Plains", "Plains", "Plains", "Island", "Island"])
            .hand(1, &["The Wondrous Wasp", "Wrath of God"])
            .library(0, &["Island"; 5])
            .library(1, &["Island"; 5])
            .at(Step::Main1, 1)
            .start();
        let dauthi = bf(&g, 0, "Dauthi Voidwalker");
        if wasp {
            let t = g.tgt(dauthi);
            cast_and_resolve(&mut g, 1, "The Wondrous Wasp", target(t));
            assert_eq!(g.count_in(1, "The Wondrous Wasp", Zone::Battlefield), 1, "{}", log(&g));
        }
        cast_and_resolve(&mut g, 1, "Wrath of God", |_| None);
        assert_eq!(in_gy(&g, 0, "Dauthi Voidwalker"), 1, "{}", log(&g));
        let guide = g.find_card(1, "Simian Spirit Guide", Zone::Graveyard).or_else(|| g.find_card(1, "Simian Spirit Guide", Zone::Exile)).unwrap();
        if wasp {
            assert_eq!(g.obj(guide).zone, Zone::Graveyard, "the Voidwalker had no abilities\n{}", log(&g));
            assert_eq!(in_gy(&g, 1, "The Wondrous Wasp"), 1);
        } else {
            assert_eq!(g.obj(guide).zone, Zone::Exile, "exiled with a void counter\n{}", log(&g));
            assert_eq!(g.counters_on(guide, Counter::Named(db().subtype_id("counter:void").unwrap())), 1);
        }
    }
}
