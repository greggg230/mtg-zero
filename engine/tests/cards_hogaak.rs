//! The Legacy Hogaak Tempo deck's cards across the three lanes (ENGINE.md 220.4): Mockingbird (lane B) copying
//! Hogaak (lane A) and lane C's four, Hogaak convoked with Dryad Arbor, Stitcher's Supplier and the Orc Army and
//! delving the graveyard Stitcher's Supplier and Dark Matter Manipulator fill, Moonshadow beside Hogaak's delve and
//! Stitcher's Supplier's dies trigger, Massacre (lane B) against them all, and the deck's real lines. The CR rule in
//! each name, the official rulings cited where one applies (`// ruling: <hash8>`, `data/rulings/legacy.rulings.json`).
//! The cards are in the card table of constructed play only (`CardDb::constructed`); no test names an identity number.
//! Each card alone is its lane's file (`cards_hogaak_a.rs`, `_b.rs`, `_c.rs`).

use engine::az::constructed::Decklist;
use engine::db::{types, CardDb, Counter, Kw};
use engine::game::{Mark, StackItem, StackKind};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const HOGAAK: &str = "Hogaak, Arisen Necropolis";
const MB: &str = "Mockingbird";
const MASSACRE: &str = "Massacre";
const SUPPLIER: &str = "Stitcher's Supplier";
const DMM: &str = "Dark Matter Manipulator";
const MOON: &str = "Moonshadow";
const ARBOR: &str = "Dryad Arbor";
const BOW: &str = "Orcish Bowmasters";
const SEA: &str = "Underground Sea";
/// Red 2/2 (vanilla on the battlefield).
const RED: &str = "Simian Spirit Guide";
/// Colourless 3/3 artifact creature (vanilla in a game).
const GREY: &str = "Cogwork Librarian";

/// `ChooseAltCost`'s answer for Massacre's own "without paying its mana cost" (its alternative cost: `NUMBER` 1).
const MASSACRE_FREE: u8 = 1;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

fn sub(name: &str) -> u16 {
    db().subtype_id(name).unwrap_or_else(|| panic!("no subtype {name}"))
}

/// No auto-answers; twelve Islands in each library.
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

/// No auto-answers; each test gives its own libraries.
fn bare() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn on_bf(g: &Game, c: CardIdx) -> bool {
    g.obj(c).zone == Zone::Battlefield
}

fn m1(g: &Game, c: CardIdx) -> u64 {
    g.counters_on(c, Counter::M1M1) as u64
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(engine::Target::Player(p))
}

/// Answer decisions with `answer` (else the first option) until priority.
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

/// Pass priority and answer the other decisions with `answer` (else the first option) until the stack is empty at
/// priority (or the game's end).
fn resolve_with(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..2000 {
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
    panic!("the stack doesn't resolve: {}\n{}", opts(g), log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, &|_| None)
}

/// Pass priority and answer the other decisions with the first option until `stop` holds.
fn run_until(g: &mut Game, stop: &dyn Fn(&Game) -> bool) {
    for _ in 0..5000 {
        if stop(g) || g.is_over() {
            return;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("run_until: no progress: {}\n{}", opts(g), log(g));
}

/// The source's name of a stack item that is an ability.
fn ability_of(g: &Game, s: &StackItem) -> Option<&'static str> {
    match s.kind {
        StackKind::Ability { ability, .. } => Some(g.db().def(ability.def).name.as_str()),
        _ => None,
    }
}

/// The triggered abilities of sources named `name` on the stack.
fn triggers_of(g: &Game, name: &str) -> usize {
    g.stack().iter().filter(|s| ability_of(g, s) == Some(name)).count()
}

/// Activate the (last offered) ability of permanent `c` at priority.
fn activate(g: &mut Game, c: CardIdx) {
    let a = g.offered().into_iter().filter(|o| matches!(o, Choice::Ability(x, _) if *x == c)).last().unwrap_or_else(|| panic!("an ability of {}: {}", g.name(c), opts(g)));
    g.choose(a);
}

// ---- Mockingbird's helpers (lane B's) ----

/// The pending decision is "what Mockingbird enters as a copy of" (CR 707.5: `ChoosePermanent`, `PASS` = nothing).
fn copy_choice_pending(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::ChoosePermanent) && g.has_option(Choice::Pass)
}

fn copy_options(g: &Game) -> Vec<CardIdx> {
    assert!(copy_choice_pending(g), "no copy choice: {}\n{}", opts(g), log(g));
    g.offered()
        .into_iter()
        .filter_map(|c| match c {
            Choice::Card(x) => Some(x),
            _ => None,
        })
        .collect()
}

/// `p` casts its Mockingbird from hand with X = `x` (paying its mana cost) and passes until it resolves and its copy
/// choice is pending. Returns the card.
fn cast_mb(g: &mut Game, p: PlayerId, x: u8) -> CardIdx {
    let c = g.card(p, MB, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "Mockingbird castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    until_priority(g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(x)),
        _ => None,
    });
    assert_eq!(g.stack().last().map(|s| s.x), Some(x as u32), "X = {x}\n{}", log(g));
    g.pass_until(|g| g.stack().is_empty());
    assert!(copy_choice_pending(g), "the copy choice: {}\n{}", opts(g), log(g));
    c
}

fn copy(g: &mut Game, what: Option<CardIdx>) {
    assert!(copy_choice_pending(g), "{}\n{}", opts(g), log(g));
    g.choose(what.map_or(Choice::Pass, Choice::Card));
}

/// The amount of mana spent to cast `c` it records (`Mark::ManaSpent`; none: 0, CR 601.2h).
fn mana_spent(g: &Game, c: CardIdx) -> u32 {
    g.marks_of(c)
        .into_iter()
        .find_map(|m| match m {
            Mark::ManaSpent(n) => Some(n),
            _ => None,
        })
        .unwrap_or(0)
}

/// Play on (passing priority, first options) until a legend-rule choice (true) or an empty stack at priority.
fn until_legend_rule(g: &mut Game) -> bool {
    for _ in 0..200 {
        if g.kind() == Some(DecisionKind::LegendRule) {
            return true;
        }
        if g.stack().is_empty() && at_priority(g) {
            return false;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("no progress: {}\n{}", opts(g), log(g));
}

// ---- Hogaak's helpers (lane A's) ----

fn castable(g: &Game, p: PlayerId, zone: Zone) -> bool {
    g.find_card(p, HOGAAK, zone).is_some_and(|c| g.has_option(Choice::Cast(c)))
}

/// Begin casting `p`'s Hogaak from `zone` (601.2a: on the stack).
fn begin_hogaak(g: &mut Game, p: PlayerId, zone: Zone) -> CardIdx {
    let c = g.card(p, HOGAAK, zone);
    assert!(g.has_option(Choice::Cast(c)), "Hogaak castable from {zone:?}: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    c
}

fn cards_offered(g: &Game) -> Vec<CardIdx> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Card(c) => Some(c),
            _ => None,
        })
        .collect()
}

/// Pay a Hogaak being cast: at each delve decision (`ChooseGraveyardCard`) the next card of `delve` (in graveyard
/// order), then `PASS`; at each convoke decision (`ChoosePermanent`) the next creature of `convoke` (in battlefield
/// order). Every pick must be offered. Stops at priority.
fn pay_hogaak(g: &mut Game, p: PlayerId, delve: &[CardIdx], convoke: &[CardIdx]) {
    let gy_pos = |g: &Game, c: CardIdx| g.player(p).graveyard.iter().position(|&x| x == c).unwrap();
    let mut delve = delve.to_vec();
    delve.sort_by_key(|&c| gy_pos(g, c));
    let mut convoke = convoke.to_vec();
    convoke.sort_by_key(|&c| g.permanents_of(p).position(|x| x == c).unwrap());
    let (mut d, mut k) = (0, 0);
    for _ in 0..100 {
        if at_priority(g) {
            assert_eq!((d, k), (delve.len(), convoke.len()), "every pick made: {}", log(g));
            return;
        }
        if g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
            if d < delve.len() {
                assert!(g.has_option(Choice::Card(delve[d])), "delve {} offered: {}", g.name(delve[d]), opts(g));
                g.choose(Choice::Card(delve[d]));
                d += 1;
            } else {
                g.pass();
            }
            continue;
        }
        assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "convoke: {}\n{}", opts(g), log(g));
        for c in cards_offered(g) {
            assert!(g.is_creature(c), "convoke offers creatures: {}", opts(g));
        }
        assert!(k < convoke.len() && g.has_option(Choice::Card(convoke[k])), "convoke pick {k} offered: {}\n{}", opts(g), log(g));
        g.choose(Choice::Card(convoke[k]));
        k += 1;
    }
    panic!("the payment doesn't end: {}\n{}", opts(g), log(g));
}

fn creatures_of(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    g.permanents_of(p).filter(|&c| g.is_creature(c)).collect()
}

// ---- Massacre's helpers (lane B's) ----

/// `p` casts its Massacre from hand for cost `cost` (`NUMBER` 0: its mana cost; `MASSACRE_FREE`: free).
fn cast_massacre(g: &mut Game, p: PlayerId, cost: u8) -> CardIdx {
    let c = g.card(p, MASSACRE, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "Massacre castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    until_priority(g, &|g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(cost)));
    c
}

/// The cost options P0's Massacre cast offers (`NUMBER` k; [0] when no choice is asked), from a clone.
fn massacre_costs(g: &Game) -> Vec<u8> {
    let mut g = g.clone();
    let c = g.card(0, MASSACRE, Zone::Hand);
    if !g.has_option(Choice::Cast(c)) {
        return Vec::new();
    }
    g.choose(Choice::Cast(c));
    if g.kind() != Some(DecisionKind::ChooseAltCost) {
        return vec![0];
    }
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Number(k) => Some(k),
            _ => None,
        })
        .collect()
}

// ---- Mockingbird copying Hogaak ----

#[test]
fn cr_601_2h_202_3_mockingbird_copies_hogaak_only_with_seven_mana_spent() {
    // ruling: 5a1ea5ca
    // ruling: de35ba2a
    // Hogaak's mana value is 7 (its hybrid pips one each, CR 202.3f): six mana spent (X 5) don't reach it, seven (X
    // 6) do. The copy: Hogaak's copiable values (a legendary 8/8 Avatar with trample, black and green) plus Bird and
    // flying; the Mockingbird card still (its own definition), mana value 7. "You can't spend mana to cast this
    // spell", convoke, delve and the graveyard permission are about casting it: nothing of a permanent.
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &[HOGAAK]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let h = bf(&g, 1, HOGAAK);
    let c = cast_mb(&mut g, 0, 5);
    assert_eq!(mana_spent(&g, c), 6);
    assert!(!copy_options(&g).contains(&h), "mana value 7 > 6: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &[HOGAAK]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let h = bf(&g, 1, HOGAAK);
    let c = cast_mb(&mut g, 0, 6);
    assert_eq!(mana_spent(&g, c), 7);
    assert!(copy_options(&g).contains(&h), "{}", opts(&g));
    copy(&mut g, Some(h));
    resolve(&mut g);
    assert!(on_bf(&g, c) && g.is_copy(c), "{}", log(&g));
    assert_eq!(g.name(c), HOGAAK);
    assert_eq!(g.own_def(c), def(MB), "the Mockingbird card");
    assert_eq!(g.pt(c), (8, 8));
    assert!(g.has_kw(c, Kw::Trample) && g.has_kw(c, Kw::Flying));
    assert!(g.has_subtype(c, sub("Avatar")) && g.has_subtype(c, sub("Bird")) && !g.has_subtype(c, sub("Bard")));
    assert!(g.is_legendary(c));
    assert_eq!(g.colors_of(c), (1 << engine::mana::B) | (1 << engine::mana::G), "Hogaak's colours, not Mockingbird's blue");
    assert_eq!(g.mana_value_now(c), 7);
    // Different controllers: no legend rule (CR 704.5j), both stay.
    assert!(on_bf(&g, h));
}

#[test]
fn cr_704_5j_mockingbird_copying_its_controllers_hogaak_the_legend_rule_and_the_card_in_the_graveyard() {
    // ruling: de35ba2a
    // P0's own Hogaak copied: two legendary Hogaaks of one player; P0 keeps the original and puts the copy into its
    // owner's graveyard. There it is the Mockingbird card (CR 707.2: the copy effect ended as it left the battlefield,
    // 400.7): named Mockingbird, mana value 1, and without Hogaak's "You may cast this card from your graveyard" - not
    // castable from there with an Island untapped, while from a hand it would be.
    let mut g = scen().battlefield(0, &["Island"; 8]).battlefield(0, &[HOGAAK]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let h = bf(&g, 0, HOGAAK);
    let c = cast_mb(&mut g, 0, 6);
    copy(&mut g, Some(h));
    assert!(until_legend_rule(&mut g), "{}", log(&g));
    assert!(g.has_option(Choice::Card(c)) && g.has_option(Choice::Card(h)), "{}", opts(&g));
    g.choose(Choice::Card(h));
    resolve(&mut g);
    assert!(on_bf(&g, h), "{}", log(&g));
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.name(c), MB);
    assert!(!g.is_copy(c));
    assert_eq!(g.mana_value_now(c), 1);
    assert!(!g.def_of(c).casting.own_gy_cast && !g.def_of(c).casting.no_mana);
    assert!(g.player(0).graveyard.contains(&c));
    assert!(!g.has_option(Choice::Cast(c)), "no graveyard permission: {}", opts(&g));
}

// ---- Mockingbird copying lane C's cards ----

#[test]
fn cr_614_1c_707_5_mockingbird_copying_moonshadow_enters_with_six_counters_and_its_trigger_works() {
    // ruling: 4376972c
    // ruling: de35ba2a
    // Moonshadow (mana value 1) copied by Mockingbird with X 0: the copied "enters with six -1/-1 counters" applies
    // (a 7/7 with six: a 1/1), menace and flying, an Elemental Bird. The original's two counters aren't copied (CR
    // 707.2). Its trigger is the copy's: P0's Wasteland sacrificed (a permanent card into P0's graveyard) - the copy
    // loses a counter; the Underground Sea it destroys goes to P1's graveyard - P1's Moonshadow loses one.
    let mut g = scen()
        .battlefield(0, &["Island", "Wasteland"])
        .battlefield(1, &[MOON, SEA])
        .counters(1, MOON, Counter::M1M1, 2)
        .hand(0, &[MB])
        .at(Step::Main1, 0)
        .start();
    let orig = bf(&g, 1, MOON);
    let c = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&orig), "{}", opts(&g));
    copy(&mut g, Some(orig));
    resolve(&mut g);
    assert!(on_bf(&g, c) && g.is_copy(c), "{}", log(&g));
    assert_eq!(g.name(c), MOON);
    assert_eq!(m1(&g, c), 6, "{}", log(&g));
    assert_eq!(g.pt(c), (1, 1));
    assert!(g.has_kw(c, Kw::Menace) && g.has_kw(c, Kw::Flying));
    assert!(g.has_subtype(c, sub("Elemental")) && g.has_subtype(c, sub("Bird")));
    let w = bf(&g, 0, "Wasteland");
    let sea = bf(&g, 1, SEA);
    activate(&mut g, w);
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(sea)));
    resolve(&mut g);
    assert_eq!(g.obj(sea).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(m1(&g, c), 5, "the Wasteland: one trigger of the copy\n{}", log(&g));
    assert_eq!(m1(&g, orig), 1, "the Sea into P1's graveyard: P1's Moonshadow\n{}", log(&g));
}

#[test]
fn cr_707_5_305_mockingbird_copying_dryad_arbor_is_a_green_flying_land_creature_and_no_land_play() {
    // ruling: 1df01b0e
    // ruling: 9da0e7ba
    // ruling: de35ba2a
    // Dryad Arbor's mana value is 0: Mockingbird with X 0 copies it. The copy is a Land Creature - Forest Dryad Bird,
    // green by the copied colour indicator (a copiable value, CR 707.2, 204.1; not Mockingbird's blue), with flying
    // and the Forest's "{T}: Add {G}". It entered by a resolving spell, not played: no land play (CR 305.1, 305.2), so
    // P0 may still play a land. It entered this turn: its {T} mana ability waits for P0's next turn (CR 302.6).
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &[ARBOR])
        .hand(0, &[MB, "Forest", "Birds of Paradise"])
        .at(Step::Main1, 0)
        .start();
    let a = bf(&g, 1, ARBOR);
    let c = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&a), "{}", opts(&g));
    copy(&mut g, Some(a));
    resolve(&mut g);
    assert!(on_bf(&g, c) && g.is_copy(c), "{}", log(&g));
    assert_eq!(g.name(c), ARBOR);
    assert!(g.is_type(c, types::LAND) && g.is_creature(c));
    for s in ["Forest", "Dryad", "Bird"] {
        assert!(g.has_subtype(c, sub(s)), "{s}");
    }
    assert!(!g.has_subtype(c, sub("Bard")));
    assert_eq!(g.colors_of(c), 1 << engine::mana::G, "green only");
    assert!(g.has_kw(c, Kw::Flying));
    assert_eq!(g.pt(c), (1, 1));
    assert_eq!(g.mana_value_now(c), 0);
    assert_eq!(g.player(0).lands_played, 0, "no land play");
    let forest = g.card(0, "Forest", Zone::Hand);
    assert!(g.has_option(Choice::Play(forest)), "a land play left: {}", opts(&g));
    assert!(g.is_sick(c));
    let birds = g.card(0, "Birds of Paradise", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(birds)), "no {{G}}: the copy is summoning sick: {}", opts(&g));
    // P0's next turn: the copy taps for {G}.
    run_until(&mut g, &|g| g.turn().active == 0 && g.turn().number > 3 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(on_bf(&g, c) && !g.is_sick(c), "{}", log(&g));
    let birds = g.card(0, "Birds of Paradise", Zone::Hand);
    assert!(g.has_option(Choice::Cast(birds)), "{}", opts(&g));
    g.choose(Choice::Cast(birds));
    until_priority(&mut g, &|_| None);
    assert!(g.obj(c).tapped, "tapped for {{G}}\n{}", log(&g));
}

#[test]
fn cr_707_5_mockingbird_copying_stitchers_supplier_mills_as_it_enters_and_as_it_dies() {
    // ruling: 4376972c
    // The copied "When this creature enters or dies, mill three cards": the copy's; P0 (its controller) mills, P1's
    // library is untouched. Bolted, it dies: its dies trigger looks back (CR 603.10a) and mills three more; the card
    // in the graveyard is Mockingbird.
    let mut g = bare()
        .library(0, &["Island", "Forest", "Swamp", "Plains", "Mountain", "Ponder", "Island", "Island"])
        .library(1, &["Island"; 6])
        .battlefield(0, &["Island"])
        .battlefield(1, &[SUPPLIER, "Mountain"])
        .hand(0, &[MB])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 1, SUPPLIER);
    let c = cast_mb(&mut g, 0, 0);
    copy(&mut g, Some(s));
    resolve(&mut g);
    assert_eq!(g.name(c), SUPPLIER);
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Zombie")) && g.has_subtype(c, sub("Bird")));
    assert_eq!(gy(&g, 0), ["Island", "Forest", "Swamp"], "{}", log(&g));
    assert!(gy(&g, 1).is_empty());
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bolt)), "{}", opts(&g));
    g.choose(Choice::Cast(bolt));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c)));
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.name(c), MB);
    let names = gy(&g, 0);
    assert_eq!(names.len(), 7, "{names:?}\n{}", log(&g));
    for n in ["Plains", "Mountain", "Ponder", MB] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
}

#[test]
fn cr_613_4c_mockingbird_copying_dark_matter_manipulator_mills_and_has_the_bonus() {
    // ruling: 4376972c
    // ruling: de35ba2a
    // The copy's enter trigger mills P0's library (four cards there, three milled: seven); its copied static gives
    // +2/+0 for every seven cards in its controller's graveyard: a 3/2 flying Human Warlock Bird. P1's Manipulator
    // reads P1's empty graveyard: 1/2.
    let mut g = bare()
        .library(0, &["Island"; 6])
        .library(1, &["Island"; 3])
        .graveyard(0, &["Swamp", "Swamp", "Swamp", "Swamp"])
        .battlefield(0, &["Island"])
        .battlefield(1, &[DMM])
        .hand(0, &[MB])
        .at(Step::Main1, 0)
        .start();
    let d = bf(&g, 1, DMM);
    let c = cast_mb(&mut g, 0, 0);
    copy(&mut g, Some(d));
    assert_eq!(g.pt(c), (1, 2), "four cards before its trigger resolves: +0");
    resolve(&mut g);
    assert_eq!(g.player(0).graveyard.len(), 7, "{}", log(&g));
    assert_eq!(g.name(c), DMM);
    assert_eq!(g.pt(c), (3, 2), "{}", log(&g));
    assert_eq!(g.pt(d), (1, 2));
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Warlock")));
}

#[test]
fn cr_707_5_mockingbird_declines_with_the_decks_creatures_offered() {
    // ruling: f30367a6
    // ruling: 5a1ea5ca
    // X 1, two mana spent: Moonshadow, Stitcher's Supplier and Dark Matter Manipulator (mana value 1) and Dryad
    // Arbor (0) are offered, Hogaak (7) isn't. Declined: a 1/1 flying Bird Bard; nothing of theirs (no mill, no
    // counters).
    let mut g = bare()
        .library(0, &["Island"; 6])
        .library(1, &["Island"; 6])
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &[MOON, SUPPLIER, DMM, ARBOR, HOGAAK])
        .hand(0, &[MB])
        .at(Step::Main1, 0)
        .start();
    let c = cast_mb(&mut g, 0, 1);
    let o = copy_options(&g);
    for n in [MOON, SUPPLIER, DMM, ARBOR] {
        assert!(o.contains(&bf(&g, 1, n)), "{n}: {}", opts(&g));
    }
    assert!(!o.contains(&bf(&g, 1, HOGAAK)));
    copy(&mut g, None);
    resolve(&mut g);
    assert!(on_bf(&g, c) && !g.is_copy(c));
    assert_eq!((g.name(c), g.pt(c)), (MB, (1, 1)));
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Bard")));
    assert_eq!(m1(&g, c), 0);
    assert!(g.player(0).graveyard.is_empty() && g.player(0).library.len() == 6, "{}", log(&g));
}

// ---- Hogaak's convoke and delve with lane C's cards ----

#[test]
fn cr_702_51a_302_6_a_summoning_sick_dryad_arbor_convokes_hogaak_paying_a_hybrid_pip_as_green() {
    // ruling: fbdd4f47
    // ruling: dbf4ec31
    // Stitcher's Supplier (black), three red and two colourless creatures: six, one {B/G} payer - not castable.
    // Dryad Arbor played this turn (summoning sick: convoke isn't a {T} ability, CR 302.6) is the seventh creature
    // and green: it pays the other {B/G} pip. Its "{T}: Add {G}" isn't activated (no mana can be spent anyway).
    let mut g = scen().battlefield(0, &[SUPPLIER, RED, RED, RED, GREY, GREY]).hand(0, &[ARBOR, HOGAAK]).at(Step::Main1, 0).start();
    assert!(!castable(&g, 0, Zone::Hand), "{}", opts(&g));
    let a = g.card(0, ARBOR, Zone::Hand);
    g.choose(Choice::Play(a));
    let a = bf(&g, 0, ARBOR);
    assert!(g.is_sick(a));
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all = creatures_of(&g, 0);
    assert_eq!(all.len(), 7);
    pay_hogaak(&mut g, 0, &[], &all);
    assert!(g.obj(a).tapped);
    assert_eq!(g.player(0).pool.amt, [0; 6], "no mana made");
    resolve(&mut g);
    assert!(g.find_card(0, HOGAAK, Zone::Battlefield).is_some(), "{}", log(&g));
    // A red creature in the Arbor's place: one {B/G} payer for two pips.
    let g = scen().battlefield(0, &[SUPPLIER, RED, RED, RED, RED, GREY, GREY]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start();
    assert!(!castable(&g, 0, Zone::Hand), "{}", opts(&g));
}

#[test]
fn cr_702_66a_702_51a_the_decks_turn_three_hogaak_from_stitchers_and_dark_matter_mills_bowmasters_and_a_fetch() {
    // ruling: c6f70acd
    // ruling: fbdd4f47
    // The deck's line: Stitcher's Supplier and Dark Matter Manipulator mill three each, Verdant Catacombs fetches an
    // Underground Sea (the fetch land to the graveyard), Orcish Bowmasters enters (1 damage, amass Orcs 1: an Orc Army
    // token). Seven graveyard cards, four black creatures (all summoning sick but the board's): Hogaak by delving
    // three (generic only) and convoking the four (two pay the {B/G} pips).
    let mut g = bare()
        .library(0, &["Brainstorm", "Ponder", "Daze", "Force of Will", "Island", "Wasteland", SEA, "Island", "Island", "Island", "Island"])
        .library(1, &["Island"; 8])
        .battlefield(0, &[SEA, "Swamp", "Swamp", "Verdant Catacombs"])
        .hand(0, &[SUPPLIER, DMM, BOW, HOGAAK])
        .turn_number(5)
        .at(Step::Main1, 0)
        .start();
    for n in [SUPPLIER, DMM] {
        let c = g.card(0, n, Zone::Hand);
        g.choose(Choice::Cast(c));
        until_priority(&mut g, &|_| None);
        resolve(&mut g);
    }
    assert_eq!(gy(&g, 0), ["Brainstorm", "Ponder", "Daze", "Force of Will", "Island", "Wasteland"], "{}", log(&g));
    let vc = bf(&g, 0, "Verdant Catacombs");
    activate(&mut g, vc);
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(def(SEA))));
    assert_eq!(g.player(0).graveyard.len(), 7, "{}", log(&g));
    let b = g.card(0, BOW, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    let all = creatures_of(&g, 0);
    assert_eq!(all.len(), 4, "Supplier, Manipulator, Bowmasters, the Orc Army: {}", log(&g));
    assert!(all.iter().all(|&c| g.colors_of(c) == 1 << engine::mana::B));
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let fuel: Vec<CardIdx> = g.player(0).graveyard.iter().copied().take(3).collect();
    pay_hogaak(&mut g, 0, &fuel, &all);
    for &c in &fuel {
        assert_eq!(g.obj(c).zone, Zone::Exile);
    }
    resolve(&mut g);
    let h = bf(&g, 0, HOGAAK);
    assert_eq!(g.pt(h), (8, 8));
    assert_eq!(g.player(0).graveyard.len(), 4);
    assert!(all.iter().all(|&c| g.obj(c).tapped));
}

#[test]
fn cr_603_2_moonshadow_and_hogaaks_delve_exile_no_trigger_then_the_legend_rule_does() {
    // ruling: 4029cc96
    // Hogaak cast from P0's graveyard (601.2a: to the stack, not "put into your graveyard") delving four lands and a
    // Ponder (exiled: never put into a graveyard) and convoking Moonshadow and Stitcher's Supplier: no Moonshadow
    // trigger. It resolves beside P0's other Hogaak: the legend rule puts one into the graveyard - a permanent card:
    // one trigger, one counter removed.
    let mut g = scen()
        .battlefield(0, &[HOGAAK, MOON, SUPPLIER])
        .counters(0, MOON, Counter::M1M1, 6)
        .graveyard(0, &[HOGAAK, "Island", "Swamp", "Wasteland", SEA, "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let (m, s, old) = (bf(&g, 0, MOON), bf(&g, 0, SUPPLIER), bf(&g, 0, HOGAAK));
    let h = begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert_eq!(g.obj(h).zone, Zone::Stack);
    let fuel = g.player(0).graveyard.clone();
    assert_eq!(fuel.len(), 5);
    pay_hogaak(&mut g, 0, &fuel, &[m, s]);
    assert_eq!(triggers_of(&g, MOON), 0, "{}", log(&g));
    assert_eq!(m1(&g, m), 6);
    assert!(g.player(0).graveyard.is_empty());
    assert!(until_legend_rule(&mut g), "{}", log(&g));
    g.choose(Choice::Card(h));
    run_until(&mut g, &|g| triggers_of(g, MOON) > 0 || (g.stack().is_empty() && at_priority(g)));
    assert_eq!(triggers_of(&g, MOON), 1, "{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(old).zone, Zone::Graveyard);
    assert_eq!(m1(&g, m), 5, "{}", log(&g));
}

// ---- Massacre against the deck ----

#[test]
fn cr_603_2c_603_10a_massacre_kills_stitchers_supplier_dryad_arbor_and_dark_matter_at_once_moonshadow_triggers_once() {
    // Massacre (P1, its mana cost: P0 controls no Plains) gives every creature -2/-2: Stitcher's Supplier (1/1),
    // Dryad Arbor (1/1) and Dark Matter Manipulator (1/2) die at once in one state-based check (704.3, 704.5f); the
    // Moonshadow with three counters (4/4 -> 2/2) and Hogaak (8/8 -> 6/6) survive. "One or more permanent cards are put
    // into your graveyard" (603.2c): one Moonshadow trigger for the three; the Supplier's dies trigger (a look-back,
    // 603.10a) beside it. Its mill of three lands is another event: another trigger.
    let mut g = bare()
        .library(0, &["Island", "Forest", "Swamp", "Island"])
        .library(1, &["Island"; 4])
        .battlefield(0, &[MOON, SUPPLIER, ARBOR, DMM, HOGAAK])
        .counters(0, MOON, Counter::M1M1, 3)
        .battlefield(1, &["Swamp"; 4])
        .hand(1, &[MASSACRE])
        .at(Step::Main1, 1)
        .start();
    let (m, h) = (bf(&g, 0, MOON), bf(&g, 0, HOGAAK));
    let dying = [bf(&g, 0, SUPPLIER), bf(&g, 0, ARBOR), bf(&g, 0, DMM)];
    cast_massacre(&mut g, 1, 0);
    run_until(&mut g, &|g| triggers_of(g, MOON) + triggers_of(g, SUPPLIER) == 2 && at_priority(g));
    assert_eq!((triggers_of(&g, MOON), triggers_of(&g, SUPPLIER)), (1, 1), "{}", log(&g));
    for &c in &dying {
        assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", g.name(c));
    }
    assert_eq!(g.pt(h), (6, 6));
    resolve(&mut g);
    assert_eq!(m1(&g, m), 1, "the batch, then the mill: two triggers\n{}", log(&g));
    assert_eq!(g.pt(m), (4, 4), "7/7 with one counter, -2/-2");
    assert!(on_bf(&g, h) && on_bf(&g, m));
}

#[test]
fn cr_603_6c_massacre_moonshadow_dying_in_the_same_event_doesnt_trigger() {
    // A Moonshadow with six counters (1/1) dies with Stitcher's Supplier and Dryad Arbor: "from anywhere" doesn't look
    // back (603.6c, 603.10): no trigger of it. The Supplier's dies trigger still mills.
    let mut g = bare()
        .library(0, &["Island", "Forest", "Swamp", "Island"])
        .library(1, &["Island"; 4])
        .battlefield(0, &[MOON, SUPPLIER, ARBOR])
        .counters(0, MOON, Counter::M1M1, 6)
        .battlefield(1, &["Swamp"; 4])
        .hand(1, &[MASSACRE])
        .at(Step::Main1, 1)
        .start();
    cast_massacre(&mut g, 1, 0);
    resolve(&mut g);
    assert_eq!(g.count_in(0, MOON, Zone::Graveyard), 1, "{}", log(&g));
    assert!(!log(&g).contains("trigger of Moonshadow"), "{}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 6, "three creatures and the mill: {}", log(&g));
}

#[test]
fn cr_704_5f_massacre_against_moonshadows_counters() {
    // Four counters (3/3): a 1/1 until end of turn, alive; five (2/2): dies. Six (1/1) has toughness -1: dies.
    for (n, lives) in [(4u64, true), (5, false), (6, false)] {
        let mut g = bare()
            .library(0, &["Island"; 3])
            .library(1, &["Island"; 3])
            .battlefield(0, &[MOON])
            .counters(0, MOON, Counter::M1M1, n as engine::num::Count)
            .battlefield(1, &["Swamp"; 4])
            .hand(1, &[MASSACRE])
            .at(Step::Main1, 1)
            .start();
        let m = bf(&g, 0, MOON);
        cast_massacre(&mut g, 1, 0);
        resolve(&mut g);
        assert_eq!(on_bf(&g, m), lives, "{n} counters\n{}", log(&g));
        if lives {
            assert_eq!(g.pt(m), (1, 1));
        }
    }
}

#[test]
fn cr_118_9_massacres_free_cast_with_the_hogaak_decks_swamps_against_a_plains() {
    // Each Swamp-typed land of the Hogaak 75 (Underground Sea, Swamp, Undercity Sewers: an Island Swamp) against a
    // Tundra (a Plains Island): the free cast is offered with no other mana. Against every land of the Delver,
    // Doomsday and Sneak and Show 75s (none a Plains) it isn't: in the deck's real matchups Massacre is hard-cast.
    let h = Decklist::legacy_hogaak();
    let lands = |d: &Decklist| -> Vec<String> {
        d.main.iter().chain(&d.sideboard).map(|e| e.1.clone()).filter(|n| db().def(def(n)).types & types::LAND != 0).collect()
    };
    let mine: Vec<String> = lands(&h).into_iter().filter(|n| db().def(def(n)).subtypes.contains(&sub("Swamp"))).collect();
    let mut sorted = mine.clone();
    sorted.sort();
    assert_eq!(sorted, ["Swamp", "Undercity Sewers", SEA]);
    for m in &mine {
        let g = scen().battlefield(0, &[m.as_str()]).battlefield(1, &["Tundra"]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
        assert_eq!(massacre_costs(&g), vec![MASSACRE_FREE], "{m} against Tundra");
    }
    let others = [Decklist::izzet_delver(), Decklist::legacy_doomsday(), Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap()];
    for d in &others {
        for l in lands(d) {
            let g = scen().battlefield(0, &[SEA]).battlefield(1, &[l.as_str()]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
            assert!(!massacre_costs(&g).contains(&MASSACRE_FREE), "{} of {}", l, d.name);
        }
    }
    // Free against a Tundra and a Delver board: Dragon's Rage Channeler and Orcish Bowmasters' army die; P0's Hogaak
    // lives.
    let mut g = scen()
        .battlefield(0, &["Undercity Sewers", HOGAAK])
        .battlefield(1, &["Tundra", "Dragon's Rage Channeler", BOW])
        .hand(0, &[MASSACRE])
        .at(Step::Main1, 0)
        .start();
    let c = cast_massacre(&mut g, 0, MASSACRE_FREE);
    assert_eq!(g.stack().last().unwrap().alt, 1);
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Dragon's Rage Channeler", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(1, BOW, Zone::Graveyard), 1);
    assert_eq!(g.pt(bf(&g, 0, HOGAAK)), (6, 6));
    assert!(!g.obj(bf(&g, 0, "Undercity Sewers")).tapped, "nothing paid");
}

// ---- the deck's lines ----

#[test]
fn cr_601_3_hogaak_countered_by_force_of_will_is_cast_again_from_the_graveyard() {
    // The deck's line: P0's graveyard holds ten cards (the mills of its Stitcher's Supplier and Dark Matter
    // Manipulator); Hogaak from hand delves five and convokes the two; Force of Will (Brainstorm pitched) counters it
    // and it goes to its owner's graveyard; cast again from there it delves the other five and convokes Orcish
    // Bowmasters and Dryad Arbor (black and green: the pips).
    let mut g = scen()
        .battlefield(0, &[SUPPLIER, DMM, BOW, ARBOR])
        .graveyard(0, &["Brainstorm", "Ponder", "Daze", "Island", "Swamp", "Wasteland", "Misty Rainforest", "Thoughtseize", SEA, "Verdant Catacombs"])
        .hand(0, &[HOGAAK])
        .hand(1, &["Force of Will", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let (s, d, b, a) = (bf(&g, 0, SUPPLIER), bf(&g, 0, DMM), bf(&g, 0, BOW), bf(&g, 0, ARBOR));
    let first: Vec<CardIdx> = g.player(0).graveyard.iter().copied().take(5).collect();
    let h = begin_hogaak(&mut g, 0, Zone::Hand);
    pay_hogaak(&mut g, 0, &first, &[s, d]);
    g.pass();
    let fow = g.card(1, "Force of Will", Zone::Hand);
    g.choose(Choice::Cast(fow));
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => g.offered().into_iter().find(|c| *c != Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(h)),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(h).zone, Zone::Graveyard, "countered: {}", log(&g));
    assert!(castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Graveyard);
    let rest = g.player(0).graveyard.clone();
    assert_eq!(rest.len(), 5);
    pay_hogaak(&mut g, 0, &rest, &[b, a]);
    resolve(&mut g);
    assert!(on_bf(&g, h), "{}", log(&g));
    assert!(g.player(0).graveyard.is_empty());
}

#[test]
fn cr_205_4_karakas_returns_hogaak_and_a_mockingbird_copying_it_as_mockingbird() {
    // Karakas ("{T}: Return target legendary creature to its owner's hand"): P0's own Karakas saves its Hogaak from
    // Swords to Plowshares (the Swords then has no legal target, CR 608.2b) and Hogaak is cast again from hand by
    // convoke. And P1's Karakas returns P0's Mockingbird copying Hogaak (legendary as a copy): in its owner's hand it is
    // Mockingbird (CR 400.7).
    let mut g = scen()
        .battlefield(0, &[HOGAAK, "Karakas", SUPPLIER, DMM, BOW, ARBOR, RED, RED, GREY])
        .battlefield(1, &["Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let h = bf(&g, 0, HOGAAK);
    g.pass();
    let sw = g.card(1, "Swords to Plowshares", Zone::Hand);
    g.choose(Choice::Cast(sw));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(h)));
    // (its caster receives priority after casting it, CR 117.3c; P1 passes, P0 responds)
    assert_eq!(g.pending_decision().unwrap().player, 1);
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 0, "P0 may respond: {}", log(&g));
    let k = bf(&g, 0, "Karakas");
    activate(&mut g, k);
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(h)));
    resolve(&mut g);
    let h = g.card(0, HOGAAK, Zone::Hand);
    assert_eq!(g.obj(sw).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 20, "no life from the Swords: it did nothing");
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all = creatures_of(&g, 0);
    assert_eq!(all.len(), 7);
    pay_hogaak(&mut g, 0, &[], &all);
    resolve(&mut g);
    assert!(on_bf(&g, h), "{}", log(&g));
    // P1's Karakas and P0's Mockingbird as Hogaak.
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &[HOGAAK, "Karakas"]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let ph = bf(&g, 1, HOGAAK);
    let c = cast_mb(&mut g, 0, 6);
    copy(&mut g, Some(ph));
    resolve(&mut g);
    assert!(g.is_legendary(c) && g.name(c) == HOGAAK);
    g.pass();
    let k = bf(&g, 1, "Karakas");
    activate(&mut g, k);
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c)));
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.obj(c).owner, 0);
    assert_eq!(g.name(c), MB);
    assert_eq!(g.mana_value_now(c), 1);
}
