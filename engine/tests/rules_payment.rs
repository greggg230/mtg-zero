//! M7e part 2: the payment model's shortcuts removed (docs/shortcuts.md,
//! group 1). Each test names the CR rule it checks and failed before its fix.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::mana::ManaCost;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

/// Answer decisions (`answer`, else the first option) until priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

/// Run the stack down, answering with the first option.
fn settle(g: &mut Game) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    panic!("settle: no progress\n{}", log(g));
}

/// Each option of the pending decision, applied to a copy, then `finish`.
fn each_option(g: &Game) -> Vec<Game> {
    options(g)
        .into_iter()
        .map(|c| {
            let mut h = g.clone();
            h.choose(c);
            finish(&mut h, |_| None);
            h
        })
        .collect()
}

// ---- 13.1: the pool's mana -------------------------------------------------

/// CR 106.4, 601.2h: the player chooses which mana in their pool pays a
/// cost. {U}{R} in the pool and Sol Ring ({1}): either can pay, leaving the
/// other (the engine spent the more plentiful, then the first, colour).
#[test]
fn cr_106_4_the_player_chooses_which_pool_mana_pays() {
    let mut g = scen().pool(0, [0, 1, 0, 1, 0, 0], 0).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let sr = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(sr));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<[u8; 6]> = each_option(&g).iter().map(|h| h.player(0).pool.amt).collect();
    left.sort();
    assert_eq!(left, vec![[0, 0, 0, 1, 0, 0], [0, 1, 0, 0, 0, 0]]);
    // Two of a kind are one choice: {R}{R} in the pool, no question.
    let mut g = scen().pool(0, [0, 0, 0, 2, 0, 0], 0).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let sr = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(sr));
    assert!(at_priority(&g), "{:?}", g.kind());
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 1, 0, 0]);
}

/// CR 106.4, 601.2h: which mana of a several-mana source pays is chosen as
/// well (its mana and the pool's together). {B} in the pool, Sol Ring
/// ({C}{C}) for a {2}: the pool keeps {B} or {C}.
#[test]
fn cr_601_2h_a_sources_mana_and_the_pools_together() {
    let mut g = scen()
        .pool(0, [0, 0, 1, 0, 0, 0], 0)
        .battlefield(0, &["Sol Ring"])
        .hand(0, &["Test Bear", "Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let ms = g.card(0, "Mind Stone", Zone::Hand);
    g.choose(Choice::Cast(ms));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
    let sol = bf(&g, 0, "Sol Ring");
    g.choose(act(sol, 0));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<[u8; 6]> = each_option(&g).iter().map(|h| h.player(0).pool.amt).collect();
    left.sort();
    assert_eq!(left, vec![[0, 0, 0, 0, 0, 1], [0, 0, 1, 0, 0, 0]]);
}

/// CR 106.4, 601.2h, and converge (an ability word, 207.2c: "the number of
/// colors of mana spent to cast this spell"): the colours spent are the
/// ones the player chose. Prismatic
/// Ending with X = 1 ({1}{W}) paid from {W}{W}{U}: {W}{U} is converge 2.
/// The engine spent {W}{W} (one colour) and the Bear survived.
#[test]
fn cr_601_2h_converge_counts_the_pool_mana_chosen() {
    let mut g = scen()
        .pool(0, [2, 1, 0, 0, 0, 0], 0)
        .hand(0, &["Prismatic Ending"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let pe = g.card(0, "Prismatic Ending", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Cast(pe));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Number(1));
    }
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    g.choose(g.tgt(bear));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let outcomes: Vec<(bool, [u8; 6])> = each_option(&g)
        .into_iter()
        .map(|mut h| {
            let left = h.player(0).pool.amt;
            settle(&mut h);
            (h.count_in(1, "Test Bear", Zone::Exile) == 1, left)
        })
        .collect();
    assert!(outcomes.contains(&(true, [1, 0, 0, 0, 0, 0])), "{{W}}{{U}} spent: two colours: {outcomes:?}");
    assert!(outcomes.contains(&(false, [0, 1, 0, 0, 0, 0])), "{{W}}{{W}} spent: one colour: {outcomes:?}");
}

/// CR 106.4 (and Arena of Glory's "if that mana is spent on a creature
/// spell, it gains haste"): which {R} pays a creature spell is the player's
/// choice; the haste goes to this spell or stays for another. The engine
/// always spent the haste mana first.
#[test]
fn cr_106_4_which_red_pays_a_creature_spell() {
    let mut g = scen().pool(0, [0, 0, 0, 3, 0, 0], 1).hand(0, &["Test Dwarf"]).at(Step::Main1, 0).start();
    let d = g.card(0, "Test Dwarf", Zone::Hand);
    g.choose(Choice::Cast(d));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut out: Vec<(bool, u8)> = each_option(&g)
        .into_iter()
        .map(|mut h| {
            let haste_left = h.player(0).pool.haste;
            settle(&mut h);
            let dwarf = bf(&h, 0, "Test Dwarf");
            (h.has_kw(dwarf, engine::db::Kw::Haste), haste_left)
        })
        .collect();
    out.sort();
    assert_eq!(out, vec![(false, 1), (true, 0)]);
}

// ---- 28.8.1, 30.7.4, 31.7.1: mana abilities with a mana cost, while paying ---

/// CR 605.3a: a mana ability may be activated while paying a cost during
/// resolution ("counter unless its controller pays", 608.2), its own mana
/// cost too (602.2b by 601.2g). Loot, the Pathfinder ("{G}, {T}, exhaust:
/// add three mana of any one color") and a Forest pay Mana Leak's {3}. The
/// engine activated such abilities with priority only: no "yes".
#[test]
fn cr_605_3a_a_mana_ability_with_a_mana_cost_pays_an_unless_cost() {
    let mut g = scen()
        .pool(0, [0, 0, 0, 0, 2, 0], 0)
        .battlefield(0, &["Loot, the Pathfinder", "Forest"])
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Mana Leak"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    finish(&mut g, |_| None);
    g.pass();
    let ml = g.card(1, "Mana Leak", Zone::Hand);
    g.choose(Choice::Cast(ml));
    finish(&mut g, |_| None);
    g.pass_until(|g| !at_priority(g));
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)), "{}", log(&g));
    assert!(g.has_option(Choice::Yes), "Loot and the Forest pay {{3}}: {:?}", g.options_desc());
    g.choose(Choice::Yes);
    let loot = bf(&g, 0, "Loot, the Pathfinder");
    assert_eq!(options(&g), vec![act(loot, 0)], "{}", log(&g));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "{}", log(&g));
    assert!(g.obj(loot).tapped && g.obj(bf(&g, 0, "Forest")).tapped);
}

/// CR 605.3a, 602.2b: a mana ability's own mana cost is paid within the
/// payment, never by its own mana (Sunken Ruins alone makes no {B}).
#[test]
fn cr_602_2b_no_mana_ability_pays_for_itself() {
    let g = scen().battlefield(0, &["Sunken Ruins"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{1}")), "its {{T}}: add {{C}}");
    assert!(!g.can_pay(0, &cost("{B}")));
    let g = scen().battlefield(0, &["Sunken Ruins", "Swamp"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}{U}")), "the Swamp pays {{U/B}}");
    assert!(!g.can_pay(0, &cost("{U}{U}{B}")));
}

/// CR 605.3a (Pentad Prism, "remove a charge counter: add one mana of any
/// color"): activated while paying, once per counter. The engine offered it
/// with priority only.
#[test]
fn cr_605_3a_pentad_prism_pays_while_paying() {
    let charge = engine::db::Counter::Named(db().subtype_id("counter:charge").unwrap());
    let mut g = scen()
        .battlefield(0, &["Pentad Prism"])
        .counters(0, "Pentad Prism", charge, 2)
        .hand(0, &["Hymn to Tourach"])
        .hand(1, &["Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, "Hymn to Tourach", Zone::Hand);
    assert!(g.has_option(Choice::Cast(h)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(h));
    finish(&mut g, |g| options(g).into_iter().find(|c| matches!(c, Choice::Target(Target::Player(1)))));
    let prism = bf(&g, 0, "Pentad Prism");
    assert_eq!(g.counters_on(prism, charge), 0, "{}", log(&g));
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), 0);
}

/// CR 605.3a, 106.4 (Arena of Glory, "{R}, {T}, exert: add {R}{R}; if that
/// mana is spent on a creature spell, it gains haste"): activated while
/// paying for the creature.
#[test]
fn cr_605_3a_arena_of_glory_while_paying_for_a_creature() {
    let mut g = scen().battlefield(0, &["Mountain", "Arena of Glory"]).hand(0, &["Test Dwarf"]).at(Step::Main1, 0).start();
    let d = g.card(0, "Test Dwarf", Zone::Hand);
    assert!(g.has_option(Choice::Cast(d)));
    g.choose(Choice::Cast(d));
    let arena = bf(&g, 0, "Arena of Glory");
    assert!(g.has_option(act(arena, 1)), "{:?}", g.options_desc());
    let mut h = g.clone();
    g.choose(act(arena, 1));
    finish(&mut g, |_| None);
    settle(&mut g);
    let dwarf = bf(&g, 0, "Test Dwarf");
    assert!(g.has_kw(dwarf, engine::db::Kw::Haste), "{}", log(&g));
    // Or the Mountain and the Arena's {T}: add {C}: no haste.
    finish(&mut h, |g| options(g).into_iter().find(|&c| c != act(arena, 1)));
    settle(&mut h);
    let dwarf = bf(&h, 0, "Test Dwarf");
    assert!(!h.has_kw(dwarf, engine::db::Kw::Haste), "{}", log(&h));
}

/// CR 605.3a, 107.3 (Wizard's Rockets, "{X}, {T}, sacrifice: add X mana in
/// any combination of colors"): while paying, the Island's mana pays X and
/// comes back red. With priority it is still offered (X = 0 draws a card).
#[test]
fn cr_605_3a_wizards_rockets_turns_mana_into_colours_while_paying() {
    let mut g = scen().battlefield(0, &["Wizard's Rockets", "Island"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let rockets = bf(&g, 0, "Wizard's Rockets");
    assert!(g.has_option(act(rockets, 0)), "with priority (its draw)");
    assert!(g.can_pay(0, &cost("{R}")));
    assert!(!g.can_pay(0, &cost("{R}{R}")));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| options(g).into_iter().find(|c| matches!(c, Choice::Target(Target::Player(1)))));
    assert_eq!(g.obj(rockets).zone, Zone::Graveyard, "{}", log(&g));
    settle(&mut g);
    assert_eq!(g.player(1).life, 17, "{}", log(&g));
}

// ---- 31.7.2: Rain of Filth's granted sacrifice after any tap ability ----------

/// CR 601.2g: a land may be tapped for mana and then sacrificed for mana in
/// one payment; Rain of Filth's granted "sacrifice this land: add {B}"
/// follows its tap ability whatever that ability's condition (Ancient Tomb:
/// {C}{C} and 2 damage). The engine combined only plain one-mana taps.
#[test]
fn cr_601_2g_rain_of_filth_after_a_tap_ability_with_damage() {
    let mut g = scen().battlefield(0, &["Swamp", "Ancient Tomb"]).hand(0, &["Rain of Filth"]).at(Step::Main1, 0).start();
    let rain = g.card(0, "Rain of Filth", Zone::Hand);
    g.choose(Choice::Cast(rain));
    let swamp = bf(&g, 0, "Swamp");
    finish(&mut g, |g| options(g).into_iter().find(|&c| c == act(swamp, 0)));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Rain of Filth", Zone::Graveyard), 1, "{}", log(&g));
    // The Tomb's {C}{C}, its {B} sacrificed, the tapped Swamp's {B}.
    assert!(g.can_pay(0, &cost("{2}{B}{B}")), "{}", log(&g));
    assert!(!g.can_pay(0, &cost("{2}{B}{B}{B}")));
}

/// Squandered Resources' sacrifice and a land's own conditioned tap: Mana
/// Confluence ("{T}, pay 1 life: add one mana of any color") tapped, then
/// sacrificed for one mana of a type it could produce (CR 601.2g, 106.7).
#[test]
fn cr_601_2g_squandered_resources_after_a_tap_ability_with_life() {
    let g = scen().battlefield(0, &["Squandered Resources", "Mana Confluence"]).life(0, 1).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}{R}")));
    let g = scen().battlefield(0, &["Squandered Resources", "Mana Confluence"]).life(0, 0).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}")), "sacrificed without its life: {}", log(&g));
    assert!(!g.can_pay(0, &cost("{U}{R}")), "no life to pay (CR 119.4)");
}
