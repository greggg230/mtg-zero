//! The 601.2g step's payment search (ENGINE.md 231.4, 247.1): every option of a cast's 601.2g step must leave its payment
//! completable (CR 601.2h), decided by searching the step's activations of the mana abilities that feed the graveyard
//! a payment reads and its castability doesn't count (Squandered Resources' "sacrifice a land: add one mana of any
//! type that land could produce" for Underworld Breach's escape exile). Such activations commute, so the search makes
//! each set of them once, in increasing order of their card and of the permanent each sacrifices, and leaves the types
//! of their mana unchosen until a choice reads them (`feeders_commute`, `Game::pool_wild`). Before, it made every
//! sequence of sacrifices and chose every type at once: ten lands passed the search's 2,000,000-copy cap (fuzz seeds
//! 4,235,003,677 and others).

use engine::db::CardDb;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Zone};

fn cube() -> Scenario {
    Scenario::new(CardDb::cube()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// The payment search's collapses (`set_payment_search_collapse`), restored when dropped.
struct Collapse;

impl Collapse {
    fn set(on: bool) -> Collapse {
        engine::set_payment_search_collapse(on);
        Collapse
    }
}

impl Drop for Collapse {
    fn drop(&mut self) {
        engine::set_payment_search_collapse(true);
    }
}

/// Answer every decision but priority with `answer` (else the first option) until priority.
fn until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..1000 {
        if g.is_over() || at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority: {}\n{}", opts(g), log(g));
}

/// Ten lands, none of them able to make {G}, and one more.
const TEN: [&str; 10] = [
    "Volcanic Island",
    "Underground Sea",
    "Tundra",
    "Badlands",
    "Plateau",
    "Scrubland",
    "Steam Vents",
    "Hallowed Fountain",
    "Watery Grave",
    "Darkslick Shores",
];

#[test]
fn cr_601_2g_702_138a_squandered_resources_sacrifices_are_searched_as_sets() {
    // Underworld Breach's escape for Llanowar Elves: {G} and three other graveyard cards exiled, two there. Squandered
    // Resources may sacrifice any of ten lands while it is cast (CR 601.2g), each land a third card and one mana of a
    // type it could make (CR 106.7) - none green: not castable. The search made every sequence of sacrifices and every
    // type (sum over k of 10!/(10-k)! sequences) and failed loudly past 2,000,000 copies (ENGINE.md 247.1); each set of
    // sacrifices once, their types unchosen, it is 2^10 sets.
    let mut b = vec!["Underworld Breach", "Squandered Resources"];
    b.extend(TEN);
    let g = cube().battlefield(0, &b).graveyard(0, &["Llanowar Elves", "Ponder", "Brainstorm"]).at(Step::Main1, 0).start();
    let elves = g.card(0, "Llanowar Elves", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(elves)), "no green mana: {}", opts(&g));
    // With a Bayou among them it escapes: the Bayou sacrificed for {G}, its card the third.
    let mut b = vec!["Underworld Breach", "Squandered Resources", "Bayou"];
    b.extend(&TEN[1..]);
    let mut g = cube().battlefield(0, &b).graveyard(0, &["Llanowar Elves", "Ponder", "Brainstorm"]).at(Step::Main1, 0).start();
    let elves = g.card(0, "Llanowar Elves", Zone::Graveyard);
    let bayou = g.card(0, "Bayou", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(elves)), "through the Bayou: {}", opts(&g));
    g.choose(Choice::Cast(elves));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.obj(elves).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(bayou).zone, Zone::Exile, "the Bayou sacrificed, then exiled by escape: {}", log(&g));
}

/// Walk the decision trees of `on` (the collapsed search) and `off` (the full one) together from their pending
/// decisions, choosing the same options, until priority or `limit` leaves: every decision must offer the same
/// options. The number of leaves.
fn walk_together(on: &Game, off: &Game, limit: usize, what: &str) -> usize {
    let mut stack = vec![(on.clone(), off.clone())];
    let mut leaves = 0;
    while let Some((a, b)) = stack.pop() {
        if leaves >= limit {
            break;
        }
        assert_eq!(a.offered(), b.offered(), "{what}: the collapsed and the full search offer different options\n{}\n{}", opts(&a), log(&a));
        if a.is_over() || at_priority(&a) {
            leaves += 1;
            continue;
        }
        for c in a.offered() {
            let (mut x, mut y) = (a.clone(), b.clone());
            {
                let _on = Collapse::set(true);
                x.choose(c);
            }
            {
                let _off = Collapse::set(false);
                y.choose(c);
            }
            stack.push((x, y));
        }
    }
    leaves
}

#[test]
fn cr_601_2g_the_collapsed_payment_search_answers_as_the_full_one_on_random_boards() {
    // Random boards around Underworld Breach's escape (three other graveyard cards exiled): Squandered Resources, Orcish
    // Lumberjack ("{T}, sacrifice a Forest: add three mana in any combination of {R} and/or {G}"), Chromatic Star (its
    // {1} paid from the pool: the types chosen first), Wizard's Rockets ({X}), a Lotus Petal (counted by castability),
    // up to four lands - basics (twins), duals, Tolarian Academy and Gaea's Cradle (counts of permanents), Gemstone
    // Caverns, Ancient Tomb, Mishra's Workshop (restricted mana) - Mind Stone and Llanowar Elves for the counts, a
    // floating mana, and graveyard cards of different costs (Tinker's additional sacrifice: a cost whose parts' order
    // can matter). Every option of the priority and of every decision of each castable escape (up to 60 paths a card)
    // must be the same with the payment search's collapses (231.4's order, 247.1's) and without them: the search's
    // answers are the full search's.
    use engine::rng::Rng;
    let lands = ["Mountain", "Forest", "Island", "Forest", "Volcanic Island", "Bayou", "Tundra", "Tolarian Academy", "Gaea's Cradle", "Gemstone Caverns", "Ancient Tomb", "Mishra's Workshop", "Taiga"];
    let spells = ["Lightning Bolt", "Llanowar Elves", "Ponder", "Preordain", "Frantic Search", "Oko, Thief of Crowns", "Demonic Tutor", "Tinker", "Fiery Confluence", "Mox Emerald"];
    let fillers = ["Brainstorm", "Ponder", "Island", "Strip Mine"];
    let mut rng = Rng::new(236);
    let (mut castable, mut paths) = (0, 0);
    for case in 0..160 {
        let mut b: Vec<&str> = vec!["Underworld Breach"];
        if rng.below(4) != 0 {
            b.push("Squandered Resources");
        }
        for (name, odds) in [("Orcish Lumberjack", 3), ("Chromatic Star", 3), ("Wizard's Rockets", 4), ("Lotus Petal", 4), ("Mind Stone", 4), ("Llanowar Elves", 4)] {
            if rng.below(odds) == 0 {
                b.push(name);
            }
        }
        for _ in 0..rng.below(5) {
            b.push(lands[rng.below(lands.len() as u32) as usize]);
        }
        let mut gy: Vec<&str> = (0..1 + rng.below(2)).map(|_| spells[rng.below(spells.len() as u32) as usize]).collect();
        gy.extend((0..rng.below(3)).map(|_| fillers[rng.below(fillers.len() as u32) as usize]));
        let pool = rng.below(4) == 0;
        let make = |on: bool| {
            let _c = Collapse::set(on);
            let mut s = cube().battlefield(0, &b).graveyard(0, &gy).at(Step::Main1, 0);
            if pool {
                s = s.pool(0, [0, 0, 0, 1, 0, 0], 0);
            }
            s.start()
        };
        let (on, off) = (make(true), make(false));
        let what = format!("case {case}: {b:?} gy {gy:?} pool {pool}");
        assert_eq!(on.offered(), off.offered(), "{what}\n{}", opts(&on));
        for &name in gy.iter().filter(|n| spells.contains(n)) {
            let c = on.card(0, name, Zone::Graveyard);
            if !on.has_option(Choice::Cast(c)) {
                continue;
            }
            castable += 1;
            let (mut x, mut y) = (on.clone(), off.clone());
            {
                let _c = Collapse::set(true);
                x.choose(Choice::Cast(c));
            }
            {
                let _c = Collapse::set(false);
                y.choose(Choice::Cast(c));
            }
            paths += walk_together(&x, &y, 60, &format!("{what}, {name} escaped"));
        }
    }
    println!("castable escapes {castable}, paths walked {paths}");
    assert!(castable >= 40 && paths >= 400, "castable escapes {castable}, paths {paths}");
}

#[test]
#[ignore = "found, not fixed (ENGINE.md 247.1.6): the search activates no mana ability of a land before a feeder sacrifices it"]
fn cr_601_2g_a_land_tapped_for_mana_then_sacrificed_while_casting() {
    // Fiery Confluence ({2}{R}{R}) escaped by Underworld Breach (three other graveyard cards, one there): each Mountain
    // tapped for {R}, then sacrificed to Squandered Resources for {R} (CR 601.2g: any mana abilities, in any order, CR
    // 106.7: a land as it last existed could produce {R}) - four mana, two cards. The 601.2g step's search offers only
    // the feeding abilities (231.4) and so never taps a land a feeder then sacrifices: castability says no. (Floating
    // the Mountains' mana with priority first, the escape is offered.)
    let g = cube()
        .battlefield(0, &["Underworld Breach", "Squandered Resources", "Mountain", "Mountain"])
        .graveyard(0, &["Fiery Confluence", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(fc)), "{}", opts(&g));
}

#[test]
fn cr_605_3a_the_lands_mana_floated_first_then_sacrificed_while_casting() {
    // The line of the test above with the Mountains' mana floated with priority (CR 605.3a) before the cast: the escape
    // is offered, Squandered Resources sacrificing both tapped Mountains while it is cast.
    let mut g = cube()
        .battlefield(0, &["Underworld Breach", "Squandered Resources", "Mountain", "Mountain"])
        .graveyard(0, &["Fiery Confluence", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Graveyard);
    for _ in 0..10 {
        if g.player(0).pool.amt[3] == 2 && at_priority(&g) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::Priority) => Choice::Yes,
            Some(DecisionKind::ActivateMana) => {
                g.offered().into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Mountain" && !g.obj(*x).tapped)).unwrap_or(Choice::Pass)
            }
            _ => g.offered()[0],
        };
        g.choose(c);
    }
    assert_eq!(g.player(0).pool.amt[3], 2, "{}
{}", opts(&g), log(&g));
    assert!(g.has_option(Choice::Cast(fc)), "{}", opts(&g));
    g.choose(Choice::Cast(fc));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.obj(fc).zone, Zone::Stack, "{}", log(&g));
}

/// Twelve lands, none of them able to make {R} or {W}.
const TWELVE: [&str; 12] = [
    "Underground Sea",
    "Bayou",
    "Tropical Island",
    "Watery Grave",
    "Overgrown Tomb",
    "Breeding Pool",
    "Swamp",
    "Island",
    "Forest",
    "Darkslick Shores",
    "Blooming Marsh",
    "Botanical Sanctum",
];

#[test]
fn cr_601_2g_702_138a_a_graveyard_that_holds_the_escapes_cards_asks_the_payment_alone() {
    // Phlage, Titan of Fire's Fury escapes for {R}{R}{W}{W} and five other graveyard cards exiled; six are there. Rain of
    // Filth ("until end of turn, lands you control gain 'Sacrifice this land: Add {B}'") makes every land a feeding
    // ability castability doesn't count for an exile, so the 601.2g step's search activated them: each land not at all,
    // tapped, sacrificed or both - 4^n states for n lands, past the 2,000,000-copy cap with eleven (fuzz seed 100002232
    // and 334 more stops, ENGINE.md 247.1). But the graveyard already holds the escape's cards and nothing else of
    // the payment reads it: a sacrifice's card changes nothing, its mana is the payment's own to make. The search has
    // nothing to activate (`Game::graveyard_fed`): no red or white mana, not castable, at once.
    let gy = ["Phlage, Titan of Fire's Fury", "Ponder", "Brainstorm", "Preordain", "Counterspell", "Mountain", "Lightning Bolt"];
    let mut g = cube().battlefield(0, &TWELVE).hand(0, &["Rain of Filth"]).graveyard(0, &gy).at(Step::Main1, 0).start();
    let rain = g.card(0, "Rain of Filth", Zone::Hand);
    let phlage = g.card(0, "Phlage, Titan of Fire's Fury", Zone::Graveyard);
    g.choose(Choice::Cast(rain));
    until_priority(&mut g, &|_| None);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g) && g.pending_decision().is_some_and(|d| d.player == 0));
    assert_eq!(g.obj(rain).zone, Zone::Graveyard, "{}", log(&g));
    assert!(!g.has_option(Choice::Cast(phlage)), "no red or white mana: {}", opts(&g));
    // With two Plateaus and two Sacred Foundries among the lands it escapes.
    let mut b: Vec<&str> = vec!["Plateau", "Plateau", "Sacred Foundry", "Sacred Foundry"];
    b.extend(&TWELVE[..9]);
    let mut g = cube().battlefield(0, &b).hand(0, &["Rain of Filth"]).graveyard(0, &gy).at(Step::Main1, 0).start();
    let rain = g.card(0, "Rain of Filth", Zone::Hand);
    let phlage = g.card(0, "Phlage, Titan of Fire's Fury", Zone::Graveyard);
    g.choose(Choice::Cast(rain));
    // (The Rain's {B} from a land that makes no red or white mana.)
    until_priority(&mut g, &|g| g.offered().into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Swamp")));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g) && g.pending_decision().is_some_and(|d| d.player == 0));
    assert!(g.has_option(Choice::Cast(phlage)), "{}
{}", opts(&g), log(&g));
    g.choose(Choice::Cast(phlage));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.obj(phlage).zone, Zone::Stack, "{}", log(&g));
}
