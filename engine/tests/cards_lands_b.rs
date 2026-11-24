//! Lane B of the Legacy Lands cards (ENGINE.md 270.2) against their Oracle
//! text, the Comprehensive Rules (the 2026-11-06 text: prepare is CR 723 there,
//! 722 in the engine's older notes) and their official rulings (`// ruling:
//! <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card table of
//! constructed play only (`CardDb::constructed`).
//!
//! Life from the Loam {1}{G}, Sorcery: "Return up to three target land cards
//! from your graveyard to your hand. / Dredge 3 (If you would draw a card, you
//! may mill three cards instead. If you do, return this card from your
//! graveyard to your hand.)"
//!
//! Exploration {G}, Enchantment: "You may play an additional land on each of
//! your turns."
//!
//! Carnivorous Cultivator {1}{G}, Creature — Elf Warlock 2/3: "Deathtouch /
//! This creature enters prepared. / Whenever this creature deals combat damage
//! to a player, return target land card from your graveyard to your hand." //
//! Enroot {G}, Sorcery: "Search your library for a land card, put it into your
//! graveyard, then shuffle."
//!
//! Yavimaya, Cradle of Growth, Legendary Land: "Each land is a Forest in
//! addition to its other land types."

use engine::db::{types, AbilitySlot, CardDb, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const LOAM: &str = "Life from the Loam";
const EXPLORATION: &str = "Exploration";
const CULTIVATOR: &str = "Carnivorous Cultivator // Enroot";
const YAVIMAYA: &str = "Yavimaya, Cradle of Growth";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

/// No auto-answers; empty libraries (each test gives its own).
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

fn gyc(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Graveyard)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Answer decisions until priority: `answer` first, else the first option.
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

/// Pass priority, declare no attackers or blockers and answer one-option
/// decisions until `stop` holds.
fn pass_to(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) || g.is_over() {
            return;
        }
        if g.has_option(Choice::Pass) {
            g.pass();
            continue;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        assert_eq!(d.options.len(), 1, "a real decision: {}\n{}", opts(g), log(g));
        let c = d.options[0].choice;
        g.choose(c);
    }
    panic!("pass_to: no progress\n{}", log(g));
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    pay(g);
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

fn hand_names(g: &Game, p: PlayerId) -> Vec<String> {
    let mut v: Vec<String> = g.player(p).hand.iter().map(|&c| g.name(c).to_string()).collect();
    v.sort();
    v
}

/// P`p`'s library, top first, by name.
fn lib(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).library.iter().rev().map(|&c| g.name(c).to_string()).collect()
}

/// A dredge question is pending: the drawer's dredge cards offered (by
/// card), and `PASS` (draw).
fn dredge_asked(g: &Game) -> Option<Vec<CardIdx>> {
    let d = g.pending_decision()?;
    if d.kind != DecisionKind::ChooseGraveyardCard || !g.agenda_tail(1).contains("Dredge") {
        return None;
    }
    assert!(g.has_option(Choice::Pass), "a dredge question offers drawing: {}", opts(g));
    Some(d.options.iter().filter_map(|o| if let Choice::Card(c) = o.choice { Some(c) } else { None }).collect())
}

/// From P0's upkeep (turn 3), pass on to its draw: the turn's draw is due.
fn to_draw(g: &mut Game) {
    pass_to(g, |g| g.turn().step == Step::Draw || dredge_asked(g).is_some());
}

/// The first card a library search offers (by definition), else `None`.
fn find_any(g: &Game) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| g.offered().into_iter().find(|c| matches!(c, Choice::Def(_)))).flatten()
}

/// A library search's offer of `name`.
fn find(g: &Game, name: &str) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::SearchLibrary))
        .then(|| g.offered().into_iter().find(|c| matches!(c, Choice::Def(d) if g.db().def(*d).name == name)))
        .flatten()
}

// ---- the definitions ---------------------------------------------------------------------

#[test]
fn the_four_cards_are_defined_as_printed() {
    let d = db();
    let l = d.def(d.id(LOAM).unwrap());
    assert!(l.is(types::SORCERY));
    assert_eq!((l.mana_value(), l.lands_b.dredge), (2, 3));
    let e = d.def(d.id(EXPLORATION).unwrap());
    assert!(e.is(types::ENCHANTMENT));
    assert_eq!((e.mana_value(), e.extra_lands), (1, 1));
    let c = d.def(d.id(CULTIVATOR).unwrap());
    assert_eq!((c.power, c.toughness, c.mana_value()), (Some(2), Some(3), 2));
    assert!(c.keywords.has(Kw::Deathtouch) && c.enters_prepared && c.prepare.is_some());
    assert_eq!(c.triggers.len(), 1);
    let y = d.def(d.id(YAVIMAYA).unwrap());
    assert!(y.is(types::LAND) && y.activated.is_empty(), "no printed mana ability: its {{G}} is the Forest type's");
    assert_eq!(y.supertypes & engine::db::supertypes::LEGENDARY, engine::db::supertypes::LEGENDARY);
    // Only Yavimaya gives a land type to other permanents among the four.
    assert!(l.lands_b.dredge > 0 && e.lands_b.dredge == 0 && c.lands_b.dredge == 0 && y.lands_b.dredge == 0);
}

// ---- Life from the Loam: the spell ---------------------------------------------------------

/// "Return up to three target land cards from your graveyard to your hand"
/// (CR 115.1, 601.2c: up to three, any number from zero; land cards only,
/// the caster's graveyard only).
#[test]
fn cr_601_2c_life_from_the_loam_returns_up_to_three_target_land_cards() {
    let mut g = bare()
        .battlefield(0, &["Forest", "Forest"])
        .graveyard(0, &["Wasteland", "Forest", "Lightning Bolt", "Urza's Saga", "Ancient Tomb"])
        .graveyard(1, &["Island"])
        .hand(0, &[LOAM])
        .start();
    let loam = hand(&g, 0, LOAM);
    g.choose(Choice::Cast(loam));
    let mut offered = Vec::new();
    let mut picked = 0;
    for _ in 0..50 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Target {
            if picked == 0 {
                offered = g.offered();
            }
            let c = d.options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(Target::Obj(_)))).filter(|_| picked < 3);
            match c {
                Some(c) => {
                    g.choose(c);
                    picked += 1;
                }
                None => {
                    g.choose(Choice::Pass);
                }
            }
            continue;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
    let names: Vec<&str> = offered.iter().filter_map(|c| if let Choice::Target(Target::Obj(r)) = c { Some(g.name(r.idx)) } else { None }).collect();
    assert!(names.contains(&"Wasteland") && names.contains(&"Urza's Saga") && names.contains(&"Ancient Tomb"), "{names:?}");
    assert!(!names.contains(&"Lightning Bolt") && !names.contains(&"Island"), "land cards of its caster's graveyard only: {names:?}");
    resolve(&mut g);
    assert_eq!(g.player(0).hand.len(), 3, "{}", log(&g));
    assert_eq!(gy(&g, 0).iter().filter(|n| *n != LOAM).count(), 2, "{:?}", gy(&g, 0));
    assert!(gy(&g, 0).contains(&LOAM.to_string()), "the sorcery goes to its graveyard");
}

/// Zero targets is a legal choice of "up to three" (CR 115.1, 601.2c): cast
/// with no land in the graveyard, it resolves and does nothing - and is then
/// in the graveyard, ready to dredge.
#[test]
fn cr_601_2c_life_from_the_loam_with_no_land_card_resolves_and_does_nothing() {
    let mut g = bare().battlefield(0, &["Forest", "Forest"]).graveyard(0, &["Lightning Bolt"]).hand(0, &[LOAM]).start();
    let loam = hand(&g, 0, LOAM);
    assert!(g.has_option(Choice::Cast(loam)), "{}", opts(&g));
    g.choose(Choice::Cast(loam));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Pass));
    assert_eq!(g.obj(loam).zone, Zone::Graveyard, "{}", log(&g));
    assert!(g.player(0).hand.is_empty());
}

// ---- Life from the Loam: dredge 3 (CR 702.52) -------------------------------------------

/// CR 702.52a: the turn's draw is replaced: three cards milled, Life from the
/// Loam returned to its owner's hand; no card drawn (614.6): the draw count
/// of the turn stays 0.
#[test]
fn cr_702_52a_dredge_replaces_the_draw_step_draw() {
    // ruling: d3cba85a
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp"])
        .graveyard(0, &[LOAM])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    let loam = gyc(&g, 0, LOAM);
    assert_eq!(dredge_asked(&g), Some(vec![loam]), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Card(loam));
    assert_eq!(g.obj(loam).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(gy(&g, 0), vec!["Island", "Forest", "Wasteland"]);
    assert_eq!(lib(&g, 0), vec!["Swamp"]);
    assert_eq!(g.player(0).drawn, 0, "no card was drawn");
    assert!(at_priority(&g) && g.turn().step == Step::Draw, "{}", opts(&g));
    // Everyone saw the card return (a public move from a public zone).
    assert_eq!(g.obj(loam).known, 0b11);
}

/// Declined (`PASS`), the draw happens as usual.
#[test]
fn cr_702_52a_dredge_is_optional() {
    let mut g = bare().library(0, &["Island", "Forest", "Wasteland", "Swamp"]).graveyard(0, &[LOAM]).at(Step::Upkeep, 0).start();
    to_draw(&mut g);
    assert!(dredge_asked(&g).is_some());
    g.choose(Choice::Pass);
    assert_eq!(hand_names(&g, 0), vec!["Island"]);
    assert_eq!(gy(&g, 0), vec![LOAM]);
    assert_eq!(g.player(0).drawn, 1);
}

/// Any draw, not only the draw step's (ruling): Brainstorm's, a cycled Street
/// Wraith's.
#[test]
fn cr_702_52a_dredge_replaces_any_draw() {
    // ruling: d3cba85a
    // Street Wraith's cycling ("Pay 2 life: draw a card").
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp"])
        .graveyard(0, &[LOAM])
        .hand(0, &["Street Wraith"])
        .start();
    let w = hand(&g, 0, "Street Wraith");
    g.choose(Choice::Ability(w, AbilitySlot::Hand(0)));
    resolve_with(&mut g, |g| dredge_asked(g).map(|c| Choice::Card(c[0])));
    assert_eq!(hand_names(&g, 0), vec![LOAM], "{}", log(&g));
    assert_eq!(gy(&g, 0), vec!["Street Wraith", "Island", "Forest", "Wasteland"]);
    assert_eq!(g.player(0).life, 18);
}

/// "If an effect puts a card into your hand without specifically using the
/// word 'draw', you're not drawing a card" (ruling): Expedition Map's search
/// puts a land into the hand - no dredge question.
#[test]
fn cr_121_1_a_card_put_into_a_hand_is_no_draw_no_dredge() {
    // ruling: cb6b05ff
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp"])
        .graveyard(0, &[LOAM])
        .battlefield(0, &["Expedition Map", "Forest", "Forest", "Forest"])
        .start();
    let map = bf(&g, 0, "Expedition Map");
    g.choose(Choice::Ability(map, AbilitySlot::Activated(0)));
    let mut asked = false;
    for _ in 0..50 {
        if at_priority(&g) && g.stack().is_empty() {
            break;
        }
        if dredge_asked(&g).is_some() {
            asked = true;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        let c = find_any(&g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    assert!(!asked, "no dredge question");
    assert_eq!(g.player(0).hand.len(), 1, "{}", log(&g));
    assert_eq!(gy(&g, 0), vec![LOAM, "Expedition Map"]);
}

/// "Each draw is performed one at a time" (CR 121.2, 121.6b; ruling):
/// Brainstorm's three draws are each asked; the first dredged mills a second
/// Life from the Loam, which may then replace the second draw; the third is
/// drawn.
#[test]
fn cr_121_6b_each_draw_of_brainstorm_may_be_dredged_by_a_card_the_first_dredge_milled() {
    // ruling: 43bacf7c
    let mut g = bare()
        .library(0, &["Island", LOAM, "Wasteland", "Swamp", "Forest", "Plains", "Mountain"])
        .graveyard(0, &[LOAM])
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .start();
    let first = gyc(&g, 0, LOAM);
    cast(&mut g, 0, "Brainstorm");
    g.pass();
    g.pass();
    // Draw 1: the graveyard's Loam.
    assert_eq!(dredge_asked(&g), Some(vec![first]), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Card(first));
    assert_eq!(g.obj(first).zone, Zone::Hand);
    // Draw 2: the milled Loam (the first is in the hand now).
    let second = gyc(&g, 0, LOAM);
    assert_ne!(second, first);
    assert_eq!(dredge_asked(&g), Some(vec![second]), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Card(second));
    assert_eq!(gy(&g, 0), vec!["Island", "Wasteland", "Swamp", "Forest", "Plains"], "{}", log(&g));
    // Draw 3: two Loams in hand, none in the graveyard: drawn.
    assert!(dredge_asked(&g).is_none());
    assert_eq!(hand_names(&g, 0), vec![LOAM, LOAM, "Mountain"], "{}", log(&g));
    // Then Brainstorm's "put two cards back".
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}", opts(&g));
}

/// "Once you've announced that you're applying a card's dredge ability ...,
/// players can't take any actions until you've put that card into your hand
/// and milled cards" (ruling): choosing it, the mill and the return happen at
/// once - the next decision is the next draw's (or what follows the
/// instruction), never a priority between.
#[test]
fn cr_702_52a_dredging_is_done_at_once() {
    // ruling: ea37d02f
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain", "Island"])
        .graveyard(0, &[LOAM])
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .start();
    cast(&mut g, 0, "Brainstorm");
    g.pass();
    g.pass();
    let loam = gyc(&g, 0, LOAM);
    assert!(dredge_asked(&g).is_some());
    g.choose(Choice::Card(loam));
    // The second draw: nothing in the graveyard dredges now (Loam is in the
    // hand): drawn at once, then the third; then Brainstorm's put-back.
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}\n{}", opts(&g), log(&g));
    assert_eq!(gy(&g, 0), vec!["Island", "Forest", "Wasteland"]);
    assert_eq!(hand_names(&g, 0), vec![LOAM, "Plains", "Swamp"]);
}

/// "One card draw can't be replaced by multiple dredge abilities" (ruling):
/// two Loams in the graveyard are two options of one question; one is
/// applied, the draw is gone, the other stays.
#[test]
fn cr_614_5_one_draw_is_replaced_by_one_dredge_ability() {
    // ruling: 41dfb0fe
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain"])
        .graveyard(0, &[LOAM, "Lightning Bolt", LOAM])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    let both = dredge_asked(&g).expect("asked");
    assert_eq!(both.len(), 2, "each Loam an option: {}", opts(&g));
    g.choose(Choice::Card(both[1]));
    assert_eq!(g.obj(both[1]).zone, Zone::Hand);
    assert_eq!(g.obj(both[0]).zone, Zone::Graveyard, "the other stays");
    assert_eq!(lib(&g, 0), vec!["Swamp", "Plains", "Mountain"]);
    assert!(at_priority(&g), "one draw, one dredge: {}", opts(&g));
}

/// CR 702.52b and the ruling: with fewer than three cards in the library no
/// question; with exactly three, it is asked (and empties the library: no
/// loss until a draw from it, 704.5b).
#[test]
fn cr_702_52b_dredge_needs_at_least_n_cards_in_the_library() {
    // ruling: ebef1beb
    let mut g = bare().library(0, &["Island", "Forest"]).graveyard(0, &[LOAM]).at(Step::Upkeep, 0).start();
    to_draw(&mut g);
    assert!(dredge_asked(&g).is_none(), "two cards: no dredge");
    assert_eq!(hand_names(&g, 0), vec!["Island"]);
    let mut g = bare().library(0, &["Island", "Forest", "Swamp"]).graveyard(0, &[LOAM]).at(Step::Upkeep, 0).start();
    to_draw(&mut g);
    let loam = gyc(&g, 0, LOAM);
    assert_eq!(dredge_asked(&g), Some(vec![loam]));
    g.choose(Choice::Card(loam));
    assert!(g.player(0).library.is_empty());
    assert!(!g.is_over(), "milling the last card is no draw from an empty library");
}

/// CR 702.52a, "your graveyard": an opponent's Life from the Loam doesn't
/// replace my draws; my own does, whoever's turn it is.
#[test]
fn cr_702_52a_dredge_works_from_its_owners_graveyard_only() {
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp"])
        .library(1, &["Island", "Forest", "Wasteland", "Swamp"])
        .graveyard(1, &[LOAM])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    assert!(dredge_asked(&g).is_none(), "{}", opts(&g));
    assert_eq!(hand_names(&g, 0), vec!["Island"]);
}

/// The draw replaced is no draw (CR 614.6): a dredged turn draw isn't the
/// "first card drawn this turn" a miracle reads (702.94a) - the next draw is.
#[test]
fn cr_702_94a_a_dredged_draw_is_not_the_first_card_drawn() {
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Metamorphosis Fanatic", "Swamp"])
        .graveyard(0, &[LOAM])
        .hand(0, &["Street Wraith"])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    let loam = gyc(&g, 0, LOAM);
    g.choose(Choice::Card(loam));
    pass_to(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.player(0).drawn, 0);
    let w = hand(&g, 0, "Street Wraith");
    g.choose(Choice::Ability(w, AbilitySlot::Hand(0)));
    // The cycling draw: Loam in hand, nothing to dredge; the Fanatic is the
    // first card drawn this turn: its miracle question.
    resolve_with(&mut g, |g| {
        assert!(dredge_asked(g).is_none());
        None
    });
    assert!(log(&g).to_lowercase().contains("miracle") || g.player(0).drawn == 1, "{}", log(&g));
    assert_eq!(g.player(0).drawn, 1, "{}", log(&g));
}

/// Narset's "each opponent can't draw more than one card each turn" (CR
/// 121.2b): a draw it stops isn't replaced by dredge either (Narset's and
/// Leovold's rulings); a draw replaced by dredge didn't happen, so the next
/// one is still allowed - and may be dredged or drawn.
#[test]
fn cr_121_2b_a_disallowed_draw_isnt_dredged_but_a_dredged_one_doesnt_count() {
    // P0 drew its turn's card: Brainstorm's draws are stopped, no question.
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain"])
        .graveyard(0, &[LOAM])
        .battlefield(0, &["Island"])
        .battlefield(1, &["Narset, Parter of Veils"])
        .hand(0, &["Brainstorm"])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    g.choose(Choice::Pass); // draw the Island
    pass_to(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    cast(&mut g, 0, "Brainstorm");
    g.pass();
    g.pass();
    assert!(dredge_asked(&g).is_none(), "{}\n{}", opts(&g), log(&g));
    assert_eq!(gy(&g, 0), vec![LOAM]);

    // P0 dredged its turn's draw: Brainstorm's first draw is allowed (and
    // asked); its second isn't.
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain", "Island", "Island"])
        .graveyard(0, &[LOAM, LOAM])
        .battlefield(0, &["Island"])
        .battlefield(1, &["Leovold, Emissary of Trest"])
        .hand(0, &["Brainstorm"])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    let a = dredge_asked(&g).expect("asked")[0];
    g.choose(Choice::Card(a));
    pass_to(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    let bs = hand(&g, 0, "Brainstorm");
    g.choose(Choice::Cast(bs));
    pay(&mut g);
    g.pass();
    g.pass();
    assert!(dredge_asked(&g).is_some(), "the first draw is allowed: {}\n{}", opts(&g), log(&g));
    g.choose(Choice::Pass);
    assert!(dredge_asked(&g).is_none(), "the second isn't: {}", opts(&g));
    assert_eq!(g.player(0).drawn, 1, "{}", log(&g));
}

/// CR 616.1, Hullbreacher's ruling: an opponent's "create a Treasure instead"
/// and the drawer's dredge both replace a draw: the drawer chooses which
/// applies - dredge (no Treasure), or not (the Treasure, no card).
#[test]
fn cr_616_1_dredge_or_an_opponents_hullbreacher() {
    let lib0 = ["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain", "Island", "Island"];
    let start = || {
        bare()
            .library(0, &lib0)
            .graveyard(0, &[LOAM])
            .battlefield(0, &["Island"])
            .battlefield(1, &["Hullbreacher"])
            .hand(0, &["Street Wraith"])
            .start()
    };
    let treasures = |g: &Game| g.battlefield().iter().filter(|&&c| g.obj(c).controller == 1 && g.name(c) == "Treasure").count();
    let mut g = start();
    let w = hand(&g, 0, "Street Wraith");
    g.choose(Choice::Ability(w, AbilitySlot::Hand(0)));
    resolve_with(&mut g, |g| dredge_asked(g).map(|c| Choice::Card(c[0])));
    assert_eq!(treasures(&g), 0, "{}", log(&g));
    assert_eq!(hand_names(&g, 0), vec![LOAM]);
    let mut g = start();
    let w = hand(&g, 0, "Street Wraith");
    g.choose(Choice::Ability(w, AbilitySlot::Hand(0)));
    resolve_with(&mut g, |g| dredge_asked(g).map(|_| Choice::Pass));
    assert_eq!(treasures(&g), 1, "{}", log(&g));
    assert!(g.player(0).hand.is_empty());
    assert_eq!(gy(&g, 0), vec![LOAM, "Street Wraith"]);
}

/// CR 616.1, 121.2a, Quantum Riddler's rulings: "draw that many plus one"
/// modifies a one-card instruction as a whole; dredge replaces a draw. For the
/// turn's draw with one card in hand, the drawer chooses: dredge first (three
/// milled, no card drawn), or Riddler first - then two draws, each of which
/// may be dredged (here: the first dredged, the second drawn).
#[test]
fn cr_616_1_dredge_or_quantum_riddler_first() {
    let lib0 = ["Island", "Forest", "Wasteland", "Swamp", "Plains", "Mountain", "Island", "Island"];
    let start = || bare().library(0, &lib0).graveyard(0, &[LOAM]).battlefield(0, &["Quantum Riddler"]).at(Step::Upkeep, 0).start();
    // Dredge first.
    let mut g = start();
    to_draw(&mut g);
    let loam = gyc(&g, 0, LOAM);
    assert_eq!(dredge_asked(&g), Some(vec![loam]));
    g.choose(Choice::Card(loam));
    assert!(at_priority(&g), "{}\n{}", opts(&g), log(&g));
    assert_eq!(hand_names(&g, 0), vec![LOAM]);
    assert_eq!(lib(&g, 0).len(), 5);
    // Riddler first: two draws; the first asked again and dredged, the
    // second drawn (Loam is in the hand).
    let mut g = start();
    to_draw(&mut g);
    g.choose(Choice::Pass);
    let loam = gyc(&g, 0, LOAM);
    assert_eq!(dredge_asked(&g), Some(vec![loam]), "the first of the two draws: {}\n{}", opts(&g), log(&g));
    g.choose(Choice::Card(loam));
    assert!(at_priority(&g), "{}\n{}", opts(&g), log(&g));
    assert_eq!(hand_names(&g, 0), vec![LOAM, "Swamp"], "{}", log(&g));
    assert_eq!(gy(&g, 0), vec!["Island", "Forest", "Wasteland"]);
    // Riddler first, both drawn (each draw asked).
    let mut g = start();
    to_draw(&mut g);
    g.choose(Choice::Pass);
    g.choose(Choice::Pass);
    assert!(dredge_asked(&g).is_some(), "the second draw: {}", opts(&g));
    g.choose(Choice::Pass);
    assert!(at_priority(&g));
    assert_eq!(hand_names(&g, 0), vec!["Forest", "Island"], "{}", log(&g));
}

/// An opponent's Leyline of the Void: the milled cards are exiled instead
/// (CR 614.1a); the dredge card still returns (CR 702.52a: "mill N cards and
/// return this card").
#[test]
fn cr_702_52a_dredge_under_an_opponents_leyline_of_the_void() {
    let mut g = bare()
        .library(0, &["Island", "Forest", "Wasteland", "Swamp"])
        .graveyard(0, &[LOAM])
        .battlefield(1, &["Leyline of the Void"])
        .at(Step::Upkeep, 0)
        .start();
    to_draw(&mut g);
    let loam = gyc(&g, 0, LOAM);
    g.choose(Choice::Card(loam));
    assert_eq!(g.obj(loam).zone, Zone::Hand);
    assert!(gy(&g, 0).is_empty());
    assert_eq!(g.exile_zone().iter().filter(|&&c| g.obj(c).owner == 0).count(), 3, "{}", log(&g));
}

/// With an empty library dredge can't apply (702.52b); Jace, Wielder of
/// Mysteries's replacement does (121.6a): no question, the game is won.
#[test]
fn cr_121_6a_an_empty_library_draw_with_jace_is_no_dredge() {
    let mut g = bare().graveyard(0, &[LOAM]).battlefield(0, &["Jace, Wielder of Mysteries"]).at(Step::Upkeep, 0).start();
    pass_to(&mut g, |g| g.is_over() || dredge_asked(g).is_some());
    assert!(dredge_asked(&g).is_none());
    assert_eq!(g.outcome().and_then(|o| o.winner), Some(0), "{}", log(&g));
}

// ---- Exploration (CR 305.2) ---------------------------------------------------------------

fn land_plays(g: &Game) -> usize {
    g.offered().iter().filter(|c| matches!(c, Choice::Play(_))).count()
}

/// CR 305.2, 305.2a: with Exploration a player may play two lands on their
/// turn; a third is not offered.
#[test]
fn cr_305_2_exploration_allows_one_additional_land() {
    let mut g = bare().battlefield(0, &[EXPLORATION]).hand(0, &["Forest", "Wasteland", "Ancient Tomb"]).start();
    let f = hand(&g, 0, "Forest");
    g.choose(Choice::Play(f));
    assert!(land_plays(&g) > 0, "{}", opts(&g));
    let w = hand(&g, 0, "Wasteland");
    g.choose(Choice::Play(w));
    assert_eq!(land_plays(&g), 0, "{}", opts(&g));
    assert_eq!(g.player(0).lands_played, 2);
}

/// "Cumulative with other effects that allow you to play additional lands,
/// including other Explorations" (ruling): two Explorations, three lands;
/// with Fastbond, any number.
#[test]
fn cr_305_2_explorations_are_cumulative() {
    // ruling: 08f2525f
    let mut g = bare().battlefield(0, &[EXPLORATION, EXPLORATION]).hand(0, &["Forest", "Wasteland", "Ancient Tomb", "Karakas"]).start();
    for n in ["Forest", "Wasteland", "Ancient Tomb"] {
        let c = hand(&g, 0, n);
        assert!(g.has_option(Choice::Play(c)), "{n}: {}", opts(&g));
        g.choose(Choice::Play(c));
    }
    assert_eq!(land_plays(&g), 0, "{}", opts(&g));
    let mut g = bare().battlefield(0, &[EXPLORATION, "Fastbond"]).life(0, 20).hand(0, &["Forest", "Wasteland", "Ancient Tomb", "Karakas"]).start();
    for n in ["Forest", "Wasteland", "Ancient Tomb", "Karakas"] {
        let c = hand(&g, 0, n);
        assert!(g.has_option(Choice::Play(c)), "{n}: {}", opts(&g));
        g.choose(Choice::Play(c));
        // (Fastbond's trigger after the first land.)
        resolve(&mut g);
    }
    assert_eq!(g.player(0).lands_played, 4);
}

/// CR 305.2b: Exploration gone after the second land, no third even if one
/// returned; CR 305.3: no land on the other player's turn, Exploration or not.
#[test]
fn cr_305_2b_305_3_the_count_of_land_plays_now_and_only_on_your_turn() {
    let mut g = bare().battlefield(0, &[EXPLORATION]).hand(0, &["Forest", "Wasteland"]).start();
    let f = hand(&g, 0, "Forest");
    g.choose(Choice::Play(f));
    // One more allowed; Exploration leaves (hand-moved: a test of the count).
    let e = bf(&g, 0, EXPLORATION);
    g.move_to(e, Zone::Graveyard);
    assert_eq!(land_plays(&g), 0, "one land played, one allowed now: {}", opts(&g));
    // The opponent's turn: no land play for P0.
    let mut g = bare().battlefield(0, &[EXPLORATION]).hand(0, &["Forest"]).at(Step::Main1, 1).start();
    pass_to(&mut g, |g| g.to_move() == Some(0) && at_priority(g));
    assert_eq!(land_plays(&g), 0, "{}", opts(&g));
}

/// A land put onto the battlefield by an effect isn't played (CR 305.4):
/// Crop Rotation's land doesn't use a land play; Crucible of Worlds' land
/// from the graveyard does (CR 305.1).
#[test]
fn cr_305_4_exploration_counts_plays_not_lands_put_onto_the_battlefield() {
    let mut g = bare()
        .battlefield(0, &[EXPLORATION, "Forest", "Crucible of Worlds"])
        .library(0, &["Ancient Tomb", "Island"])
        .graveyard(0, &["Wasteland"])
        .hand(0, &["Crop Rotation", "Forest"])
        .start();
    let cr = hand(&g, 0, "Crop Rotation");
    g.choose(Choice::Cast(cr));
    let forest = bf(&g, 0, "Forest");
    pay_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Sacrifice) => g.offered().into_iter().find(|&c| c == Choice::Card(forest)),
        _ => None,
    });
    resolve_with(&mut g, |g| find(g, "Ancient Tomb"));
    assert_eq!(g.player(0).lands_played, 0, "{}", log(&g));
    assert!(g.find_card(0, "Ancient Tomb", Zone::Battlefield).is_some(), "{}", log(&g));
    // Two plays left: the hand's Forest and the graveyard's Wasteland.
    let f = hand(&g, 0, "Forest");
    g.choose(Choice::Play(f));
    let w = gyc(&g, 0, "Wasteland");
    assert!(g.has_option(Choice::Play(w)) || g.offered().contains(&Choice::FromGraveyard), "{}", opts(&g));
}

// ---- Yavimaya, Cradle of Growth -------------------------------------------------------------

fn forest(g: &Game) -> u16 {
    g.db().subtype_id("Forest").unwrap()
}

fn is_forest(g: &Game, c: CardIdx) -> bool {
    g.has_subtype(c, forest(g))
}

/// Ruling 5bfb8e33, CR 305.6, 305.7: every land on the battlefield, either
/// player's, is a Forest with "{T}: Add {G}" besides what it was: an Island
/// still an Island and basic, Wasteland still Wasteland (its abilities kept),
/// Yavimaya itself a Forest.
#[test]
fn cr_305_7_each_land_is_a_forest_in_addition() {
    // ruling: 5bfb8e33
    let g = bare().battlefield(0, &[YAVIMAYA, "Wasteland", "Island"]).battlefield(1, &["Island", "Urza's Saga"]).start();
    let island = g.db().subtype_id("Island").unwrap();
    for (p, n) in [(0, YAVIMAYA), (0, "Wasteland"), (0, "Island"), (1, "Island"), (1, "Urza's Saga")] {
        let c = bf(&g, p, n);
        assert!(is_forest(&g, c), "{n} of P{p}");
    }
    let i = bf(&g, 0, "Island");
    assert!(g.has_subtype(i, island));
    assert_ne!(g.supertypes_of(i) & engine::db::supertypes::BASIC, 0, "still basic");
    // Wasteland's own abilities kept (305.7: "in addition").
    let w = bf(&g, 0, "Wasteland");
    assert!(g.activated_count(w) >= 3, "{{T}}: {{C}}, its destroy, and the Forest's {{G}}: {}", g.activated_count(w));
}

/// The Forest's {G} pays: Exploration cast with only Wasteland and Ancient
/// Tomb untapped (neither makes green mana of its own).
#[test]
fn cr_305_6_a_land_made_a_forest_taps_for_green() {
    let mut g = bare().battlefield(0, &[YAVIMAYA, "Wasteland"]).hand(0, &[EXPLORATION]).start();
    let y = bf(&g, 0, YAVIMAYA);
    let w = bf(&g, 0, "Wasteland");
    g.set_tapped(y, true);
    let e = hand(&g, 0, EXPLORATION);
    assert!(g.has_option(Choice::Cast(e)), "Wasteland taps for {{G}}: {}", opts(&g));
    g.choose(Choice::Cast(e));
    pay(&mut g);
    assert!(g.obj(w).tapped, "{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(e).zone, Zone::Battlefield);
    // Without Yavimaya, no.
    let g = bare().battlefield(0, &["Wasteland"]).hand(0, &[EXPLORATION]).start();
    let e = hand(&g, 0, EXPLORATION);
    assert!(!g.has_option(Choice::Cast(e)));
}

/// Rulings 6466518a, 7bbd22d4: land cards elsewhere aren't Forests -
/// Windswept Heath ("a Forest or Plains card") doesn't find a library's
/// Wasteland; Yavimaya in a hand or graveyard isn't one.
#[test]
fn cr_305_7_only_lands_on_the_battlefield_are_forests() {
    // ruling: 6466518a 7bbd22d4
    let mut g = bare()
        .battlefield(0, &[YAVIMAYA, "Windswept Heath"])
        .library(0, &["Wasteland", "Island"])
        .hand(0, &["Wasteland", YAVIMAYA])
        .graveyard(0, &[YAVIMAYA, "Ancient Tomb"])
        .start();
    for z in [Zone::Hand, Zone::Graveyard] {
        let c = g.card(0, YAVIMAYA, z);
        assert!(!is_forest(&g, c), "{z:?}");
    }
    let w = hand(&g, 0, "Wasteland");
    assert!(!is_forest(&g, w));
    let heath = bf(&g, 0, "Windswept Heath");
    let a = (0..4u8).map(|i| Choice::Ability(heath, AbilitySlot::Activated(i))).find(|&a| g.has_option(a)).expect("the fetch");
    g.choose(a);
    let mut searched = false;
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            assert!(g.offered().iter().all(|c| !matches!(c, Choice::Def(_))), "nothing to find: {}", opts(g));
        }
        let _ = &searched;
        None
    });
    searched = true;
    assert!(searched);
    assert_eq!(g.player(0).library.len(), 2, "{}", log(&g));
}

/// Legendary (CR 704.5j): a second Yavimaya, then one is kept.
#[test]
fn cr_704_5j_two_yavimayas() {
    let mut g = bare().battlefield(0, &[YAVIMAYA]).hand(0, &[YAVIMAYA]).start();
    let y = hand(&g, 0, YAVIMAYA);
    g.choose(Choice::Play(y));
    pay_with(&mut g, |_| None);
    assert_eq!(g.lands_of(0).count(), 1, "{}", log(&g));
}

/// CR 613.8a-b, 305.7: under Magus of the Moon Yavimaya is a nonbasic land,
/// so a Mountain whose rules-text ability is gone: its grant depends on the
/// Moon's effect and doesn't exist, whatever the timestamps - no land is a
/// Forest by it, a basic Island neither; when the Magus leaves, every land
/// is a Forest again.
#[test]
fn cr_613_8_yavimaya_under_magus_of_the_moon_makes_no_forest() {
    for moon_first in [true, false] {
        let s = bare();
        let s = if moon_first { s.battlefield(1, &["Magus of the Moon"]).battlefield(0, &[YAVIMAYA]) } else { s.battlefield(0, &[YAVIMAYA]).battlefield(1, &["Magus of the Moon"]) };
        let mut g = s.battlefield(0, &["Island", "Wasteland"]).battlefield(1, &["Mountain"]).start();
        let mountain = g.db().subtype_id("Mountain").unwrap();
        let y = bf(&g, 0, YAVIMAYA);
        assert!(g.has_subtype(y, mountain) && !is_forest(&g, y), "moon first {moon_first}");
        for (p, n) in [(0, "Island"), (0, "Wasteland"), (1, "Mountain")] {
            let c = bf(&g, p, n);
            assert!(!is_forest(&g, c), "{n}, moon first {moon_first}");
        }
        let m = bf(&g, 1, "Magus of the Moon");
        g.move_to(m, Zone::Graveyard);
        for (p, n) in [(0, YAVIMAYA), (0, "Island"), (0, "Wasteland"), (1, "Mountain")] {
            let c = bf(&g, p, n);
            assert!(is_forest(&g, c), "{n} after the Magus left");
        }
    }
}

/// Dryad Arbor is a Forest already: one "{T}: Add {G}", not two (CR 305.6:
/// an ability per basic land type it has).
#[test]
fn cr_305_6_a_printed_forest_gets_no_second_green_ability() {
    let g = bare().battlefield(0, &[YAVIMAYA, "Dryad Arbor", "Forest"]).start();
    let a = bf(&g, 0, "Dryad Arbor");
    let f = bf(&g, 0, "Forest");
    let g0 = bare().battlefield(0, &["Dryad Arbor", "Forest"]).start();
    assert_eq!(g.activated_count(a), g0.activated_count(bf(&g0, 0, "Dryad Arbor")));
    assert_eq!(g.activated_count(f), g0.activated_count(bf(&g0, 0, "Forest")));
}

// ---- Carnivorous Cultivator // Enroot (prepare, CR 723) ----------------------------------------

/// The copy of a prepare spell in exile, if any.
fn enroots(g: &Game) -> Vec<CardIdx> {
    g.exile_zone().iter().copied().filter(|&c| g.is_copy_obj(c) && g.name(c) == "Enroot").collect()
}

fn cast_cultivator(g: &mut Game) -> CardIdx {
    let c = hand(g, 0, CULTIVATOR);
    g.choose(Choice::Cast(c));
    pay(g);
    resolve(g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(g));
    c
}

/// Ruling 4afe6989, CR 723.3a, 723.3c: it enters prepared: an Enroot copy in
/// exile its controller may cast for {G}; cast, the Cultivator is unprepared
/// and the copy goes to the stack, then ceases to exist (704.5e); Enroot puts
/// a land card from the library into the graveyard and shuffles.
#[test]
fn cr_723_3c_cultivator_enters_prepared_and_casts_enroot() {
    // ruling: 4afe6989 c6772882
    let mut g = bare().battlefield(0, &["Forest"; 3]).library(0, &["Island", "Wasteland", "Lightning Bolt"]).hand(0, &[CULTIVATOR]).start();
    let c = cast_cultivator(&mut g);
    assert!(g.is_prepared(c));
    let copy = enroots(&g);
    assert_eq!(copy.len(), 1, "{}", log(&g));
    let copy = copy[0];
    assert!(g.def_of(copy).is(types::SORCERY) && !g.def_of(copy).is(types::CREATURE));
    assert_eq!(g.mana_value_now(copy), 1);
    assert!(g.has_option(Choice::Cast(copy)), "{}", opts(&g));
    g.choose(Choice::Cast(copy));
    pay(&mut g);
    assert!(!g.is_prepared(c), "unprepared as the copy is cast (601.2i)");
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            let w = g.offered().into_iter().find(|c| matches!(c, Choice::Def(d) if g.db().def(*d).name == "Wasteland"));
            return w;
        }
        None
    });
    assert_eq!(gy(&g, 0), vec!["Wasteland"], "{}", log(&g));
    assert_eq!(g.player(0).library.len(), 2);
    assert_eq!(g.obj(copy).zone, Zone::Nowhere);
    assert!(enroots(&g).is_empty());
}

/// Ruling 94077065, CR 723.3c (the exception to 704.5e): the copy stays in
/// exile through state-based checks while the Cultivator is prepared; when
/// the Cultivator leaves, it ceases to exist.
#[test]
fn cr_723_3c_the_enroot_copy_stays_in_exile_while_prepared() {
    // ruling: 94077065
    let mut g = bare().battlefield(0, &["Forest"; 3]).library(0, &["Island"; 4]).library(1, &["Island"; 4]).battlefield(1, &["Plains"]).hand(1, &["Swords to Plowshares"]).hand(0, &[CULTIVATOR]).start();
    let c = cast_cultivator(&mut g);
    pass_to(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(enroots(&g).len(), 1, "through every check of a turn\n{}", log(&g));
    let stp = hand(&g, 1, "Swords to Plowshares");
    g.choose(Choice::Cast(stp));
    let t = g.tgt(c);
    pay_with(&mut g, |g| g.has_option(t).then_some(t));
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Exile);
    assert!(enroots(&g).is_empty(), "{}", log(&g));
}

/// Ruling eeba768b, CR 723.3: in a hand it is cast only as the creature - no
/// "cast Enroot" option.
#[test]
fn cr_723_3_cultivator_is_cast_only_as_the_creature() {
    // ruling: eeba768b
    let g = bare().battlefield(0, &["Forest"; 3]).hand(0, &[CULTIVATOR]).start();
    let c = hand(&g, 0, CULTIVATOR);
    assert!(g.has_option(Choice::Cast(c)));
    assert!(!g.offered().iter().any(|o| matches!(o, Choice::CastFace(x) | Choice::Faces(x, _) if *x == c)), "{}", opts(&g));
}

/// Ruling 2d0caf19, CR 723.2c: a green creature card with mana value 2 in
/// every zone: in a hand, a graveyard, a library; on the stack.
#[test]
fn cr_723_2_cultivator_is_a_creature_card_in_every_zone() {
    // ruling: 2d0caf19
    let g = bare().hand(0, &[CULTIVATOR]).graveyard(0, &[CULTIVATOR]).library(0, &[CULTIVATOR]).start();
    for z in [Zone::Hand, Zone::Graveyard, Zone::Library] {
        let c = g.card(0, CULTIVATOR, z);
        assert_eq!(g.types_of(c), types::CREATURE, "{z:?}");
        assert_eq!(g.mana_value_now(c), 2, "{z:?}");
        assert_eq!(g.colors_of(c), 1 << engine::mana::G, "{z:?}");
    }
    // Life from the Loam's "land card" targets never offer it.
    let mut g = bare().battlefield(0, &["Forest"; 2]).graveyard(0, &[CULTIVATOR, "Forest"]).hand(0, &[LOAM]).start();
    let loam = hand(&g, 0, LOAM);
    g.choose(Choice::Cast(loam));
    let cult = gyc(&g, 0, CULTIVATOR);
    assert!(!g.has_option(g.tgt(cult)), "{}", opts(&g));
}

/// Deathtouch (CR 702.2b): blocked by Murktide Regent (a 3/3 with no card
/// exiled by delve), it kills it.
#[test]
fn cr_702_2b_cultivator_has_deathtouch() {
    let mut g = bare().battlefield(0, &[CULTIVATOR]).battlefield(1, &["Murktide Regent"]).graveyard(0, &["Lightning Bolt", "Forest", "Brainstorm"]).start();
    let c = bf(&g, 0, CULTIVATOR);
    let goyf = bf(&g, 1, "Murktide Regent");
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    let att = g.offered().into_iter().find(|o| matches!(o, Choice::Card(x) if *x == c)).expect("block the Cultivator");
    g.choose(att);
    pass_to(&mut g, |g| g.turn().step == Step::End || g.obj(goyf).zone != Zone::Battlefield);
    assert_eq!(g.obj(goyf).zone, Zone::Graveyard, "{}", log(&g));
}

/// "Whenever this creature deals combat damage to a player, return target
/// land card from your graveyard to your hand" (CR 510.3a, 603.2): unblocked,
/// the trigger returns a land card; with none, it is removed (603.3d).
#[test]
fn cr_603_3d_cultivator_returns_a_land_card_on_combat_damage() {
    let mut g = bare().battlefield(0, &[CULTIVATOR]).graveyard(0, &["Lightning Bolt", "Wasteland"]).start();
    let c = bf(&g, 0, CULTIVATOR);
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    let w = gyc(&g, 0, "Wasteland");
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::Target) || g.turn().step == Step::End);
    assert!(g.has_option(g.tgt(w)), "{}\n{}", opts(&g), log(&g));
    assert!(!g.offered().iter().any(|o| matches!(o, Choice::Target(Target::Obj(r)) if g.name(r.idx) == "Lightning Bolt")));
    let t = g.tgt(w);
    g.choose(t);
    resolve(&mut g);
    assert_eq!(g.obj(w).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.player(1).life, 18);
    // No land card: no target, nothing.
    let mut g = bare().battlefield(0, &[CULTIVATOR]).graveyard(0, &["Lightning Bolt"]).start();
    let c = bf(&g, 0, CULTIVATOR);
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    pass_to(&mut g, |g| g.turn().step == Step::End);
    assert_eq!(g.player(1).life, 18);
    assert!(g.player(0).hand.is_empty());
}
