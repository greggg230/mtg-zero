//! Legacy Hogaak Tempo, lane B (ENGINE.md 220.2): Mockingbird and Massacre
//! against their Oracle text, the Comprehensive Rules and their official
//! rulings (`// ruling: <hash8>`, `data/rulings/legacy.rulings.json`), and
//! their interactions with the deck's other cards and the Delver deck's.
//! They are in the card table of constructed play only
//! (`CardDb::constructed`). No test names an identity number (the legacy
//! registry's lines are assigned at integration). Hogaak, Stitcher's
//! Supplier, Dark Matter Manipulator, Moonshadow and Dryad Arbor are other
//! lanes' cards: Mockingbird copying them and Massacre against them are the
//! integration's tests.
//!
//! Mockingbird {X}{U}, Creature — Bird Bard 1/1: "Flying / You may have this
//! creature enter as a copy of any creature on the battlefield with mana
//! value less than or equal to the amount of mana spent to cast this
//! creature, except it's a Bird in addition to its other types and it has
//! flying."
//!
//! Massacre {2}{B}{B}, Sorcery: "If an opponent controls a Plains and you
//! control a Swamp, you may cast this spell without paying its mana cost. /
//! All creatures get -2/-2 until end of turn."

use engine::az::encode::{perm, squash, CODE, O_MY_PERM, O_OPP_PERM, PERM_W};
use engine::az::identity;
use engine::az::tail_encode::{self as te, code_identity, seg, Row};
use engine::db::{types, AbilitySlot, CardDb, Counter, Kw};
use engine::decision::act;
use engine::game::{Mark, StackKind};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const MB: &str = "Mockingbird";
const MASSACRE: &str = "Massacre";
const BOW: &str = "Orcish Bowmasters";
const BORROWER: &str = "Brazen Borrower // Petty Theft";
const KAITO: &str = "Kaito, Bane of Nightmares";
const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
const CURTAINS: &str = "Concealing Curtains // Revealing Eye";
const BALLISTA: &str = "Walking Ballista";
const RAGAVAN: &str = "Ragavan, Nimble Pilferer";
const TNN: &str = "True-Name Nemesis";
const ARWEN: &str = "Arwen, Mortal Queen";
const MURKTIDE: &str = "Murktide Regent";
const METAMORPH: &str = "Phyrexian Metamorph";
const TAR_PIT: &str = "Creeping Tar Pit";
const FOW: &str = "Force of Will";
const OMNI: &str = "Omniscience";
const SNT: &str = "Show and Tell";
const FLUTE: &str = "Disruptor Flute";
const THALIA: &str = "Thalia, Guardian of Thraben";
const MAGUS: &str = "Magus of the Moon";
const EVANGELIST: &str = "Sanguine Evangelist";
const MITE: &str = "Haywire Mite";
const SEA: &str = "Underground Sea";

/// `ChooseAltCost`'s answer for Massacre's own "without paying its mana cost"
/// (its alternative cost 0: `NUMBER` 1; `NUMBER` 0 pays the mana cost).
const MASSACRE_FREE: u8 = 1;
/// `ChooseAltCost`'s answer for Omniscience's "without paying their mana
/// costs" (the last `NUMBER`, `eoe_c::FREE_FIRST_SPELL`).
const OMNI_FREE: u8 = (act::NUMBER_N - 1) as u8;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

fn ident(d: u16) -> usize {
    identity::identities(db())[d as usize] as usize
}

fn sub(name: &str) -> u16 {
    db().subtype_id(name).unwrap_or_else(|| panic!("no subtype {name}"))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
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

/// Answer every decision but priority with `answer` (else the first
/// option), passing priority, until the stack is empty at priority (or the
/// game's end).
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

/// The pending decision is "what Mockingbird enters as a copy of" (CR 707.5:
/// `ChoosePermanent`, `PASS` = nothing).
fn copy_choice_pending(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::ChoosePermanent) && g.has_option(Choice::Pass)
}

/// The permanents the pending copy choice offers on its current page.
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

/// `p` casts its Mockingbird from hand with X = `x` (paying its mana cost),
/// and passes until it resolves and its copy choice is pending. Returns the
/// card.
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

/// Answer the pending copy choice (`None`: nothing), then resolve the stack
/// with first options.
fn copy(g: &mut Game, what: Option<CardIdx>) {
    assert!(copy_choice_pending(g), "{}\n{}", opts(g), log(g));
    g.choose(what.map_or(Choice::Pass, Choice::Card));
}

/// The amount of mana spent to cast `c` that it records (`Mark::ManaSpent`;
/// none: 0, CR 601.2h).
fn mana_spent(g: &Game, c: CardIdx) -> u32 {
    g.marks_of(c)
        .into_iter()
        .find_map(|m| match m {
            Mark::ManaSpent(n) => Some(n),
            _ => None,
        })
        .unwrap_or(0)
}

/// The status floats of permanent `c` in viewer `v`'s encoding (its slot).
fn perm_status(g: &Game, v: PlayerId, c: CardIdx) -> Vec<f32> {
    let ctl = g.obj(c).controller;
    let k = g.permanents_of(ctl).position(|x| x == c).expect("a permanent");
    let base = if ctl == v { O_MY_PERM } else { O_OPP_PERM } + k * PERM_W;
    Row::of(g, v).head[base..base + PERM_W].to_vec()
}

/// Play on (passing priority, first options: no attack, no block) until
/// player `p`'s turn begins.
fn to_turn_of(g: &mut Game, p: PlayerId) {
    for _ in 0..2000 {
        if g.turn().active == p && g.turn().number > 3 || g.is_over() {
            return;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("no turn of P{p}: {}", log(g));
}

// ---- Mockingbird: the card --------------------------------------------------------------

#[test]
fn mockingbird_is_a_1_1_blue_bird_bard_with_flying_and_x_in_its_cost() {
    let d = db().def(def(MB));
    assert!(d.types & types::CREATURE != 0 && d.colors != 0);
    assert_eq!((d.power, d.toughness), (Some(1), Some(1)));
    assert_eq!(d.x_pips, 1);
    assert!(d.subtypes.contains(&sub("Bird")) && d.subtypes.contains(&sub("Bard")));
    assert!(db().unsupported().is_empty(), "{:?}", db().unsupported());
}

#[test]
fn cr_202_3e_mockingbirds_mana_value_counts_x_only_on_the_stack() {
    // {X}{U}: X counts on the stack (1 + X), 0 elsewhere (hand, battlefield,
    // graveyard).
    let mut g = scen().battlefield(0, &["Island"; 4]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = g.card(0, MB, Zone::Hand);
    assert_eq!(g.mana_value_now(c), 1, "in hand");
    g.choose(Choice::Cast(c));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(3)));
    assert_eq!(g.obj(c).zone, Zone::Stack);
    assert_eq!(g.mana_value_now(c), 4, "on the stack with X = 3");
    g.pass_until(|g| g.stack().is_empty());
    copy(&mut g, None);
    resolve(&mut g);
    assert!(on_bf(&g, c));
    assert_eq!(g.mana_value_now(c), 1, "on the battlefield X is 0");
}

// ---- the choice: optional, among creatures with mana value <= the mana spent --------------

#[test]
fn cr_707_5_mockingbird_may_decline_and_is_a_1_1_flying_bird_bard() {
    // ruling: f30367a6
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[BOW]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    copy(&mut g, None);
    resolve(&mut g);
    assert!(on_bf(&g, c) && !g.is_copy(c));
    assert_eq!(g.name(c), MB);
    assert_eq!(g.pt(c), (1, 1));
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Bard")));
    // (No enter ability of its own: the Bowmasters did nothing.)
    assert_eq!(g.count_in(0, "Orc Army", Zone::Battlefield), 0);
}

#[test]
fn cr_601_2h_the_amount_counts_x_and_the_blue_mana() {
    // ruling: 5a1ea5ca
    // X = 1: two mana spent (X and {U}): a creature of mana value 2 may be
    // copied, one of 3 not.
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[BOW, BORROWER, RAGAVAN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 1);
    assert_eq!(mana_spent(&g, c), 2);
    let o = copy_options(&g);
    assert!(o.contains(&bf(&g, 1, BOW)) && o.contains(&bf(&g, 1, RAGAVAN)), "{}", opts(&g));
    assert!(!o.contains(&bf(&g, 1, BORROWER)), "mana value 3 > 2: {}", opts(&g));
    // And not itself (CR 707.5: another creature on the battlefield).
    assert!(!o.contains(&c));
}

#[test]
fn cr_601_2h_with_x_2_mana_value_3_may_be_copied() {
    // ruling: 5a1ea5ca
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[BORROWER]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    assert_eq!(mana_spent(&g, c), 3);
    assert!(copy_options(&g).contains(&bf(&g, 1, BORROWER)), "{}", opts(&g));
}

#[test]
fn cr_601_2h_a_cost_increase_counts_toward_the_mana_spent() {
    // ruling: 5a1ea5ca
    // "Counts all mana spent to cast Mockingbird, not just mana spent on
    // {X}": Disruptor Flute naming Mockingbird makes X = 0 cost {3}{U}, four
    // mana spent: a creature of mana value 4 or less may be copied (Brazen
    // Borrower, 3), Murktide Regent (7) not.
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .battlefield(1, &[BORROWER, MURKTIDE])
        .hand(0, &[FLUTE, MB])
        .at(Step::Main1, 0)
        .start();
    let f = g.card(0, FLUTE, Zone::Hand);
    g.choose(Choice::Cast(f));
    until_priority(&mut g, &|_| None);
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Def(def(MB)));
    resolve(&mut g);
    let c = cast_mb(&mut g, 0, 0);
    assert_eq!(mana_spent(&g, c), 4, "{{3}}{{U}}: {}", log(&g));
    let o = copy_options(&g);
    assert!(o.contains(&bf(&g, 1, BORROWER)) && !o.contains(&bf(&g, 1, MURKTIDE)), "{}", opts(&g));
}

#[test]
fn cr_601_2h_mana_from_one_source_of_two_counts_twice_and_unspent_mana_not() {
    // ruling: 5a1ea5ca
    // Ancient Tomb's {C}{C} pays X = 2 and an Island the {U}: three mana
    // spent. (Mana floating unspent isn't spent to cast it.)
    let mut g = scen().battlefield(0, &["Ancient Tomb", "Island"]).battlefield(1, &[BORROWER]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    assert_eq!(mana_spent(&g, c), 3, "{}", log(&g));
    assert!(copy_options(&g).contains(&bf(&g, 1, BORROWER)));
    // Mana in the pool beyond the cost: X = 0 with three floating mana
    // spends one.
    let mut g = scen().pool(0, [0, 2, 0, 0, 0, 1], 0).battlefield(1, &[BOW, RAGAVAN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 0);
    assert_eq!(mana_spent(&g, c), 1, "{}", log(&g));
    let o = copy_options(&g);
    assert!(o.contains(&bf(&g, 1, RAGAVAN)) && !o.contains(&bf(&g, 1, BOW)), "{}", opts(&g));
}

#[test]
fn cr_118_9_cast_without_paying_its_mana_cost_x_is_0_and_no_mana_is_spent() {
    // CR 107.3b, 601.2h: Omniscience's free cast: X is 0, no mana spent: only
    // a creature of mana value 0 (Walking Ballista on the battlefield, X 0).
    let mut g = scen().battlefield(0, &[OMNI]).battlefield(1, &[BALLISTA, RAGAVAN]).counters(1, BALLISTA, Counter::P1P1, 2).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = g.card(0, MB, Zone::Hand);
    g.choose(Choice::Cast(c));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(OMNI_FREE)));
    assert_eq!(g.stack().last().unwrap().x, 0);
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(mana_spent(&g, c), 0);
    let o = copy_options(&g);
    assert_eq!(o, vec![bf(&g, 1, BALLISTA)], "{}", opts(&g));
}

#[test]
fn cr_601_2h_put_onto_the_battlefield_without_casting_it_no_mana_was_spent() {
    // Show and Tell puts Mockingbird: it wasn't cast, so the amount is 0:
    // the Bowmasters (2) and Ragavan (1) can't be copied, Walking Ballista
    // (0) can. And not a creature entering at the same time (614.12a,
    // 707.5): the other player's Walking Ballista put by the same Show and
    // Tell.
    let mut g = scen()
        .battlefield(0, &["Island"; 3])
        .battlefield(1, &[BOW, RAGAVAN, BALLISTA])
        .counters(1, BALLISTA, Counter::P1P1, 1)
        .hand(0, &[SNT, MB])
        .hand(1, &[BALLISTA])
        .at(Step::Main1, 0)
        .start();
    let s = g.card(0, SNT, Zone::Hand);
    g.choose(Choice::Cast(s));
    until_priority(&mut g, &|_| None);
    g.pass_until(|g| g.stack().is_empty());
    let (mb, b2) = (g.card(0, MB, Zone::Hand), g.card(1, BALLISTA, Zone::Hand));
    g.choose(Choice::Card(mb));
    g.choose(Choice::Card(b2));
    for _ in 0..20 {
        if copy_choice_pending(&g) {
            break;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(mana_spent(&g, mb), 0);
    let o = copy_options(&g);
    let old = g.battlefield().iter().copied().find(|&x| g.name(x) == BALLISTA && x != b2).unwrap();
    assert_eq!(o, vec![old], "only the Ballista that was there: {}", opts(&g));
}

#[test]
fn cr_707_5_every_creature_of_both_players_and_tokens_is_offered_past_a_page() {
    // ruling: f2b97e8c
    // Forty creatures of mana value 2 on the other side and two of Mockingbird's
    // controller's (one a token Orc Army): every one is a legal choice, the
    // later ones on later pages (`Choice::Page`); the controller's own too.
    let mut g = scen()
        .battlefield(0, &[SEA, "Island", "Island", "Island"])
        .battlefield(0, &[BOW])
        .battlefield(1, &[BOW; 40])
        .hand(0, &[BOW, MB])
        .at(Step::Main1, 0)
        .start();
    // P0 flashes a second Bowmasters: an Orc Army token.
    let b = g.card(0, BOW, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    let army = g.battlefield().iter().copied().find(|&x| g.is_token(x)).expect("an Orc Army");
    let mut all: Vec<CardIdx> = Vec::new();
    cast_mb(&mut g, 0, 1);
    for _ in 0..10 {
        all.extend(copy_options(&g));
        let next = g.offered().into_iter().find(|c| matches!(c, Choice::Page(p) if *p != engine::decision::Page::FIRST));
        match next {
            Some(p) if all.len() < 43 => g.choose(p),
            _ => break,
        };
    }
    let creatures: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&x| g.is_creature(x) && g.obj(x).zone == Zone::Battlefield && g.name(x) != MB).collect();
    all.sort();
    all.dedup();
    let mut want = creatures.clone();
    want.sort();
    assert_eq!(all, want, "every creature but Mockingbird itself, over the pages: {}", opts(&g));
    assert!(all.contains(&army) && want.len() == 43);
}

// ---- what it copies ----------------------------------------------------------------------

#[test]
fn cr_707_5_copying_orcish_bowmasters_its_enter_trigger_and_the_exception() {
    // ruling: 4376972c
    // ruling: de35ba2a
    // A 1/1 Orc Archer that is also a Bird and has flying (and flash, the
    // Bowmasters'); its "when this creature enters" ability triggers: 1
    // damage to any target, then amass Orcs 1 (P0's army).
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[BOW]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 1);
    let bow = bf(&g, 1, BOW);
    copy(&mut g, Some(bow));
    let life = g.life(1);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    assert!(g.is_copy(c) && on_bf(&g, c));
    assert_eq!(g.name(c), BOW);
    assert_eq!(g.own_def(c), def(MB), "still the Mockingbird card");
    assert_eq!(g.pt(c), (1, 1));
    assert!(g.has_kw(c, Kw::Flying) && g.has_kw(c, Kw::Flash));
    assert!(g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Orc")) && g.has_subtype(c, sub("Archer")));
    assert!(!g.has_subtype(c, sub("Bard")), "not a Bard: it copies the Bowmasters");
    assert_eq!(g.life(1), life - 1, "the copy's enter trigger: {}", log(&g));
    let army = g.battlefield().iter().copied().find(|&x| g.is_token(x) && g.obj(x).controller == 0).expect("an Orc Army");
    assert_eq!(g.counters_on(army, Counter::P1P1), 1);
    // The copy has the Bowmasters' mana value (2) and isn't a token.
    assert_eq!(g.mana_value_now(c), 2);
    assert!(!g.is_token(c));
}

#[test]
fn cr_707_5_copying_bowmasters_its_draw_trigger_is_the_copys_too() {
    // ruling: de35ba2a
    // The copy has both of the Bowmasters' triggers: P1 draws a second card
    // in P0's turn (Brainstorm) -> the copy deals 1 damage and amasses.
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[BOW, "Island"]).hand(0, &[MB]).hand(1, &["Brainstorm"]).at(Step::Main1, 0).start();
    cast_mb(&mut g, 0, 1);
    let t = bf(&g, 1, BOW);
    copy(&mut g, Some(t));
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(0))));
    let life = g.life(1);
    // P1 casts Brainstorm in P0's turn (draws three, none in P1's draw
    // step): the copy triggers three times.
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 1));
    let bs = g.card(1, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    until_priority(&mut g, &|_| None);
    // (P1's own Bowmasters doesn't trigger on P1's draws; P0's copy aims
    // each 1 damage at P1.)
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    assert_eq!(g.life(1), life - 3, "three draws past the first: {}", log(&g));
}

#[test]
fn cr_707_5_copying_brazen_borrower_a_3_1_flash_flyer_that_blocks_only_fliers() {
    // ruling: de35ba2a
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[BORROWER]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    let t = bf(&g, 1, BORROWER);
    copy(&mut g, Some(t));
    resolve(&mut g);
    assert_eq!(g.pt(c), (3, 1));
    assert!(g.has_kw(c, Kw::Flying) && g.has_kw(c, Kw::Flash));
    assert!(g.has_subtype(c, sub("Faerie")) && g.has_subtype(c, sub("Rogue")) && g.has_subtype(c, sub("Bird")));
    assert_eq!(g.mana_value_now(c), 3);
    assert_eq!(g.own_def(c), def(MB), "still the Mockingbird card");
}

#[test]
fn cr_707_2_copying_a_token_it_copies_the_tokens_original_values_and_isnt_a_token() {
    // ruling: f2b97e8c
    // The Orc Army made by the Bowmasters is a 0/0 with a +1/+1 counter:
    // Mockingbird (X 0, one mana: the Army's mana value 0) copies a 0/0 Orc
    // Army that is a Bird with flying, without the counter: it dies (704.5f)
    // and goes to the graveyard as the Mockingbird card (not a token).
    let mut g = scen().battlefield(0, &[SEA, "Island", "Island"]).hand(0, &[BOW, MB]).at(Step::Main1, 0).start();
    let b = g.card(0, BOW, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    let army = g.battlefield().iter().copied().find(|&x| g.is_token(x)).expect("an Orc Army");
    let c = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&army));
    copy(&mut g, Some(army));
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "a 0/0: {}", log(&g));
    assert_eq!(g.name(c), MB);
    assert!(on_bf(&g, army) && g.counters_on(army, Counter::P1P1) == 1);
}

#[test]
fn cr_707_2_copying_copies_no_counters_tapped_status_or_other_effects() {
    // ruling: de35ba2a
    // A tapped Brazen Borrower with two +1/+1 counters: the copy is an untapped
    // 3/1.
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[BORROWER]).counters(1, BORROWER, Counter::P1P1, 2).hand(0, &[MB]).at(Step::Main1, 0).start();
    let bor = bf(&g, 1, BORROWER);
    g.set_tapped(bor, true);
    let c = cast_mb(&mut g, 0, 2);
    copy(&mut g, Some(bor));
    resolve(&mut g);
    assert_eq!(g.pt(bor), (5, 3));
    assert_eq!(g.pt(c), (3, 1));
    assert!(!g.obj(c).tapped && g.counters_on(c, Counter::P1P1) == 0);
}

#[test]
fn cr_707_3_a_copy_of_a_copy_copies_what_it_copied_with_the_exceptions() {
    // ruling: 5239ef09
    // Mockingbird A copies P1's Bowmasters; Mockingbird B (X 1) copies A: a
    // Bowmasters, Bird, flying (the exceptions are copiable values, 707.9b).
    // Phyrexian Metamorph copying A: a Bowmasters that is an artifact, a Bird
    // and has flying.
    let mut g = scen()
        .battlefield(0, &["Island"; 8])
        .battlefield(1, &[BOW])
        .hand(0, &[MB, MB, METAMORPH])
        .at(Step::Main1, 0)
        .start();
    let a = cast_mb(&mut g, 0, 1);
    let t = bf(&g, 1, BOW);
    copy(&mut g, Some(t));
    resolve(&mut g);
    let b = cast_mb(&mut g, 0, 1);
    assert!(copy_options(&g).contains(&a));
    copy(&mut g, Some(a));
    resolve(&mut g);
    assert_eq!(g.name(b), BOW);
    assert!(g.has_kw(b, Kw::Flying) && g.has_subtype(b, sub("Bird")) && g.has_subtype(b, sub("Orc")));
    assert_eq!(g.pt(b), (1, 1));
    // The Metamorph ({3}{U/P}): it may copy the artifact-or-creature A.
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    until_priority(&mut g, &|_| None);
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Card(a));
    resolve(&mut g);
    assert_eq!(g.name(m), BOW);
    assert!(g.is_type(m, types::ARTIFACT) && g.is_creature(m));
    assert!(g.has_kw(m, Kw::Flying) && g.has_subtype(m, sub("Bird")), "Mockingbird's exceptions copied: {}", log(&g));
}

#[test]
fn cr_107_3m_copying_walking_ballista_x_is_mockingbirds() {
    // ruling: 602bf769
    // The Ballista on the battlefield has mana value 0 (its X is 0, CR
    // 202.3e): Mockingbird with X 0 may copy it and enters with no counters
    // (Mockingbird's X, 0): a 0/0 that dies. With X 2, the copy's "enters
    // with X +1/+1 counters" uses the X of the spell that became it (CR
    // 107.3m): a 2/2; its own mana value is 0.
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &[BALLISTA]).counters(1, BALLISTA, Counter::P1P1, 3).hand(0, &[MB, MB]).at(Step::Main1, 0).start();
    let ball = bf(&g, 1, BALLISTA);
    let a = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&ball));
    copy(&mut g, Some(ball));
    resolve(&mut g);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "no counters: a 0/0\n{}", log(&g));
    let b = cast_mb(&mut g, 0, 2);
    copy(&mut g, Some(ball));
    resolve(&mut g);
    assert!(on_bf(&g, b));
    assert_eq!(g.counters_on(b, Counter::P1P1), 2, "{}", log(&g));
    assert_eq!(g.pt(b), (2, 2));
    assert_eq!(g.mana_value_now(b), 0);
    assert!(g.has_kw(b, Kw::Flying));
}

#[test]
fn cr_614_1c_copying_arwen_its_enters_with_counter_applies() {
    // ruling: 4376972c
    // "Arwen enters with an indestructible counter on her": the copy enters
    // with one.
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(1, &[ARWEN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    let t = bf(&g, 1, ARWEN);
    copy(&mut g, Some(t));
    resolve(&mut g);
    assert_eq!(g.name(c), ARWEN);
    assert_eq!(g.counters_on(c, Counter::Kw(Kw::Indestructible)), 1, "{}", log(&g));
    assert!(g.has_kw(c, Kw::Indestructible) && g.has_kw(c, Kw::Flying));
}

#[test]
fn cr_614_12_copying_true_name_nemesis_its_as_enters_choice_is_made() {
    // ruling: 4376972c
    // "As this creature enters, choose a player": asked as the copy enters;
    // protection from the chosen player.
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &[TNN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 2);
    let t = bf(&g, 1, TNN);
    copy(&mut g, Some(t));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Target(engine::Target::Player(1)));
    resolve(&mut g);
    assert_eq!(g.name(c), TNN);
    assert_eq!(g.obj(c).chosen_player, Some(1));
}

#[test]
fn cr_607_2q_copying_murktide_regent_no_card_was_exiled_with_it() {
    // ruling: 4376972c
    // "Enters with a +1/+1 counter for each instant and sorcery card exiled
    // with it": Mockingbird (X 6) exiled none (no delve): a 3/3 Dragon Bird.
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &[MURKTIDE]).counters(1, MURKTIDE, Counter::P1P1, 2).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 6);
    assert_eq!(mana_spent(&g, c), 7);
    let t = bf(&g, 1, MURKTIDE);
    copy(&mut g, Some(t));
    resolve(&mut g);
    assert_eq!(g.name(c), MURKTIDE);
    assert_eq!(g.pt(c), (3, 3));
    assert!(g.has_subtype(c, sub("Dragon")) && g.has_subtype(c, sub("Bird")));
}


/// Answer forced and default decisions until the legend rule is asked (or
/// priority with an empty stack: none).
fn until_legend_rule(g: &mut Game) -> bool {
    for _ in 0..50 {
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

#[test]
fn cr_704_5j_copying_your_own_legend_brings_the_legend_rule() {
    // Mockingbird (X 0) copying P0's Ragavan: two legendary Ragavans of one
    // player; P0 keeps one (the copy here).
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(0, &[RAGAVAN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let rag = bf(&g, 0, RAGAVAN);
    let c = cast_mb(&mut g, 0, 0);
    copy(&mut g, Some(rag));
    assert!(until_legend_rule(&mut g), "{}", log(&g));
    assert!(g.has_option(Choice::Card(c)) && g.has_option(Choice::Card(rag)), "{}", opts(&g));
    g.choose(Choice::Card(c));
    resolve(&mut g);
    assert!(on_bf(&g, c) && g.obj(rag).zone == Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.name(c), RAGAVAN);
    // The other player's Ragavan: no legend rule (different controllers).
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[RAGAVAN]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let rag = bf(&g, 1, RAGAVAN);
    let c = cast_mb(&mut g, 0, 0);
    copy(&mut g, Some(rag));
    assert!(!until_legend_rule(&mut g));
    assert!(on_bf(&g, c) && on_bf(&g, rag));
}

#[test]
fn cr_306_5b_copying_kaito_it_enters_with_loyalty_and_is_a_creature_during_your_turn() {
    // Kaito, Bane of Nightmares (the deck's two): during its controller's
    // turn with a loyalty counter he's a 3/4 Ninja creature (not a
    // planeswalker, its ruling) with hexproof, so Mockingbird (X 3, four mana:
    // Kaito's mana value) may copy P0's own Kaito. The copy enters with
    // Kaito's printed loyalty (306.5b: it is a planeswalker as it enters), is a
    // 3/4 Ninja creature with flying and hexproof now; two Kaitos: the legend
    // rule. On the other player's turn it is a planeswalker, not a creature,
    // so not a Bird (205.3d), still with flying.
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(0, &[KAITO]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, KAITO);
    assert!(g.is_creature(k));
    let c = cast_mb(&mut g, 0, 3);
    assert!(copy_options(&g).contains(&k), "{}", opts(&g));
    copy(&mut g, Some(k));
    assert!(until_legend_rule(&mut g), "{}", log(&g));
    g.choose(Choice::Card(c));
    resolve(&mut g);
    assert!(on_bf(&g, c) && g.obj(k).zone == Zone::Graveyard, "{}", log(&g));
    let loyalty = db().def(def(KAITO)).loyalty.unwrap() as u64;
    assert_eq!(g.counters_on(c, Counter::Loyalty) as u64, loyalty);
    assert!(g.is_creature(c) && !g.is_type(c, types::PLANESWALKER));
    assert_eq!(g.pt(c), (3, 4));
    assert!(g.has_kw(c, Kw::Flying) && g.has_kw(c, Kw::Hexproof) && g.has_subtype(c, sub("Ninja")));
    // P1's turn: a planeswalker with flying, no creature type.
    to_turn_of(&mut g, 1);
    assert!(g.is_type(c, types::PLANESWALKER) && !g.is_creature(c), "{}", log(&g));
    assert!(g.has_kw(c, Kw::Flying) && !g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Kaito")));
}

#[test]
fn cr_712_11a_copying_tamiyo_it_cant_return_transformed() {
    // The Delver deck's Tamiyo, Inquisitive Student (mana value 1): the copy
    // (X 0) is a 0/3 legendary Moonfolk Wizard Bird with flying and her
    // triggers. When its controller draws its third card in a turn it is
    // exiled and, not a double-faced card, stays in exile (712.11a).
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[TAMIYO]).hand(0, &[MB, "Brainstorm"]).at(Step::Main1, 0).start();
    let tam = bf(&g, 1, TAMIYO);
    let c = cast_mb(&mut g, 0, 0);
    copy(&mut g, Some(tam));
    resolve(&mut g);
    assert_eq!(g.name(c), g.name(tam));
    assert_eq!(g.pt(c), (0, 3));
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Bird")) && g.has_subtype(c, sub("Moonfolk")));
    // Brainstorm draws three: the third card's trigger exiles the copy.
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.name(c), MB, "in exile it is the Mockingbird card");
    assert!(on_bf(&g, tam), "P1's Tamiyo is untouched");
}

#[test]
fn cr_202_3b_copying_a_transformed_permanent_the_copy_has_mana_value_0() {
    // P1's Concealing Curtains transformed into Revealing Eye (mana value 1,
    // its front face's, CR 202.3b). Mockingbird (X 0, one mana) copies it: a
    // 3/4 Eye Horror Bird with menace and flying (it isn't double-faced: it
    // can't transform); a copy of a back face has mana value 0 (202.3b), so a
    // second Mockingbird cast without paying its mana cost (no mana spent)
    // may copy the copy but not the original.
    let mut g = scen()
        .battlefield(0, &["Island"; 2])
        .battlefield(0, &[OMNI])
        .battlefield(1, &[CURTAINS])
        .transformed(1, CURTAINS)
        .hand(0, &[MB, MB])
        .at(Step::Main1, 0)
        .start();
    let eye = g.battlefield().iter().copied().find(|&x| g.obj(x).owner == 1).unwrap();
    assert_eq!(g.name(eye), "Revealing Eye");
    assert_eq!(g.mana_value_now(eye), 1, "a back face: its front face's mana value");
    let a = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&eye));
    copy(&mut g, Some(eye));
    resolve(&mut g);
    assert_eq!(g.name(a), "Revealing Eye");
    assert_eq!(g.pt(a), (3, 4));
    assert!(g.has_kw(a, Kw::Menace) && g.has_kw(a, Kw::Flying) && g.has_subtype(a, sub("Bird")));
    assert_eq!(g.mana_value_now(a), 0, "a copy of a back face");
    let b = g.card(0, MB, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(OMNI_FREE)));
    g.pass_until(|g| g.stack().is_empty());
    let o = copy_options(&g);
    assert!(o.contains(&a) && !o.contains(&eye), "{}", opts(&g));
}

#[test]
fn cr_205_3d_copying_an_animated_land_it_is_a_land_no_land_play_not_a_bird() {
    // P0's Creeping Tar Pit animated (a 3/2 creature by an effect, not a
    // copiable value; mana value 0): Mockingbird (X 0) copies a Creeping Tar
    // Pit: a land, not a creature, so not a Bird (205.3d), with flying; it
    // enters tapped (the copied "enters tapped", 707.5); it is no land play
    // (305.1): P0 may still play a land this turn.
    let mut g = scen().battlefield(0, &[TAR_PIT, "Island", "Island", "Swamp", "Island"]).hand(0, &[MB, "Island"]).at(Step::Main1, 0).start();
    let tp = bf(&g, 0, TAR_PIT);
    let animate = g
        .offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s @ AbilitySlot::Activated(_)) if x == tp => Some(s),
            _ => None,
        })
        .last()
        .expect("the Tar Pit's abilities");
    g.choose(Choice::Ability(tp, animate));
    until_priority(&mut g, &|g| match g.pending_decision().unwrap().options.iter().find(|o| !matches!(o.choice, Choice::Ability(x, _) if x == tp)) {
        Some(o) if g.kind() == Some(DecisionKind::PayMana) => Some(o.choice),
        _ => None,
    });
    resolve(&mut g);
    assert!(g.is_creature(tp), "animated: {}", log(&g));
    let c = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&tp), "{}", opts(&g));
    copy(&mut g, Some(tp));
    resolve(&mut g);
    assert_eq!(g.name(c), TAR_PIT);
    assert!(g.is_type(c, types::LAND) && !g.is_creature(c));
    assert!(g.obj(c).tapped, "enters tapped");
    assert!(g.has_kw(c, Kw::Flying) && !g.has_subtype(c, sub("Bird")));
    assert_eq!(g.player(0).lands_played, 0);
    let island = g.card(0, "Island", Zone::Hand);
    assert!(g.has_option(Choice::Play(island)) || g.offered().iter().any(|o| matches!(o, Choice::Cast(x) if *x == island)), "a land play left: {}", opts(&g));
}

// ---- Mockingbird: hidden information and the encoding ---------------------------------------

#[test]
fn the_copy_choice_and_the_copy_are_public_and_survive_determinize() {
    // CR 707.5: what Mockingbird may copy is public (battlefield objects and
    // the mana it records); a world sampled for either player keeps the
    // pending choice, and after it the copy and its encoding.
    let mut g = scen()
        .battlefield(0, &["Island"; 3])
        .battlefield(1, &[BOW, BORROWER])
        .hand(0, &[MB, "Island", "Brainstorm"])
        .hand(1, &["Island", "Ponder", FOW])
        .at(Step::Main1, 0)
        .start();
    let c = cast_mb(&mut g, 0, 1);
    for viewer in [0u8, 1] {
        let mut w = g.clone();
        w.determinize(viewer, &mut engine::rng::Rng::new(11));
        assert_eq!(w.pending_decision(), g.pending_decision());
        assert_eq!(w.view_of(viewer), g.view_of(viewer));
    }
    let t = bf(&g, 1, BOW);
    copy(&mut g, Some(t));
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    assert!(g.is_copy(c));
    for viewer in [0u8, 1] {
        let x0 = Row::of(&g, viewer).flat();
        for seed in 0..4 {
            let mut w = g.clone();
            w.determinize(viewer, &mut engine::rng::Rng::new(seed));
            assert!(w.is_copy(c) && w.copiable(c) == g.copiable(c));
            assert!(Row::of(&w, viewer).flat() == x0, "viewer {viewer}: the encoding differs in a sampled world");
        }
    }
}

#[test]
fn the_copy_its_identity_bird_flying_and_mana_spent_reach_the_encoding() {
    // The permanent: its identity is Mockingbird's (what it is), the copy flag
    // and the copied card's identity (`perm::COPY_OF`), flying, the mana spent
    // to cast it (`perm::MANA_SPENT`, 2 / 8 squashed). The copy effect's
    // `effects` token: the copied identity and the exceptions - one subtype,
    // Bird (its stable name id), and flying - for both viewers.
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[BOW]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 1);
    let t = bf(&g, 1, BOW);
    copy(&mut g, Some(t));
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    for v in 0..2u8 {
        let x = perm_status(&g, v, c);
        assert_eq!(code_identity(&x[..CODE]), Some(ident(def(MB))), "P{v}: what it is");
        let st = &x[CODE..];
        assert_eq!(st[perm::COPY], 1.0);
        assert_eq!(code_identity(&st[perm::COPY_OF..perm::COPY_OF + 32]), Some(ident(def(BOW))), "P{v}: what it copies");
        assert_eq!(st[engine::az::encode::kw_float(Kw::Flying)], 1.0);
        assert_eq!(st[perm::MANA_SPENT], squash(2.0 / 8.0));
        let row = Row::of(&g, v);
        let toks = row.tokens(seg::EFFECTS);
        let cp: Vec<&[f32]> = toks.iter().map(|t| &t.1[CODE..]).filter(|s| s[te::ef::COPY] == 1.0).collect();
        assert_eq!(cp.len(), 1, "P{v}: one copy effect");
        let s = cp[0];
        assert_eq!(code_identity(&s[te::ef::COPY_DEF..te::ef::COPY_DEF + 32]), Some(ident(def(BOW))));
        assert_eq!(s[te::ef::SUBTYPES_N], 0.25, "one subtype");
        let id: u64 = (0..32).map(|b| (s[te::ef::SUBTYPES + b] as u64) << b).sum();
        assert_eq!(id, te::name_id("Bird") as u64, "P{v}: Bird");
        assert_eq!(s[te::ef::KWS], 1.0, "P{v}: flying (the first keyword)");
    }
    // A declined Mockingbird: no copy flag, no copy effect.
    let mut g = scen().battlefield(0, &["Island"; 2]).battlefield(1, &[BOW]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 1);
    copy(&mut g, None);
    resolve(&mut g);
    for v in 0..2u8 {
        assert_eq!(perm_status(&g, v, c)[CODE + perm::COPY], 0.0);
        assert!(Row::of(&g, v).tokens(seg::EFFECTS).iter().all(|t| t.1[CODE + te::ef::COPY] == 0.0));
    }
}

#[test]
fn every_tables_copy_exceptions_fit_the_effects_token() {
    // A copy of a copy has every exception of both (707.9b): the subtypes the
    // copy exceptions of a table name, all together, must fit the effects
    // token (`ef::SUBS`, a loud failure past it). The constructed table's:
    // Superior Spider-Man's three and Mockingbird's Bird; every one a
    // creature or land type (205.3d, `Lowerer::exc_subtype`).
    use engine::program::{Ext, Op};
    for d0 in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        let mut mask = 0u64;
        for d in 0..d0.len() as u16 {
            let def = d0.def(d);
            if let Some(e) = &def.enters_as_copy {
                mask |= e.exc.subtypes;
            }
            let abs = def.spell.iter().chain(def.triggers.iter().map(|t| &t.ability)).chain(def.activated.iter().filter_map(|a| match &a.effect {
                engine::db::ActEffect::Stack(ab) => Some(ab),
                _ => None,
            }));
            for ab in abs {
                for list in std::iter::once(&ab.program).chain(ab.modes.iter().flat_map(|m| m.list.iter().map(|x| &x.program))) {
                    for op in list {
                        if let Op::Ext(Ext::CreateTokenCopy { exc, .. } | Ext::BecomeCopy { exc, .. }) = op {
                            mask |= exc.subtypes;
                        }
                    }
                }
            }
        }
        assert!(mask.count_ones() as usize <= te::ef::SUBS, "copy exceptions name {} subtypes", mask.count_ones());
    }
    let con = CardDb::constructed();
    let e = con.def(con.id(MB).unwrap()).enters_as_copy.as_ref().unwrap();
    assert!(e.exc.has_subtype(con, con.subtype_id("Bird").unwrap()) && e.exc.kws.has(Kw::Flying));
}

// ---- Massacre --------------------------------------------------------------------------------

#[test]
fn massacre_is_a_black_sorcery_of_mana_value_4() {
    let d = db().def(def(MASSACRE));
    assert_eq!(d.mana_value(), 4);
    assert!(d.types & types::SORCERY != 0);
    assert_eq!(d.alt_costs.len(), 1);
}

/// P0 casts Massacre, answering the cost choice with `cost` (`NUMBER` 0: the
/// mana cost; `MASSACRE_FREE`), paying with first options; then the stack
/// resolves with first options.
fn cast_massacre(g: &mut Game, cost: u8) -> CardIdx {
    let c = g.card(0, MASSACRE, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "Massacre castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    until_priority(g, &|g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(cost)));
    c
}

/// The cost options Massacre's cast offers (`NUMBER` k), from a clone.
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

#[test]
fn cr_118_9_massacre_free_with_an_opponents_plains_and_your_swamp() {
    // CR 118.9, 601.2b: P1's Tundra is a Plains, P0's Underground Sea a Swamp:
    // with no other mana Massacre is cast for its alternative cost alone,
    // nothing paid (the Sea stays untapped); with four lands both costs are
    // offered.
    let base = || scen().battlefield(1, &["Tundra"]).hand(0, &[MASSACRE]).at(Step::Main1, 0);
    let mut g = base().battlefield(0, &[SEA]).start();
    assert_eq!(massacre_costs(&g), vec![MASSACRE_FREE]);
    let c = cast_massacre(&mut g, MASSACRE_FREE);
    let item = g.stack().last().unwrap();
    assert!(matches!(item.kind, StackKind::Spell { card } if card == c));
    assert_eq!(item.alt, 1, "cast for its alternative cost");
    assert!(!g.obj(bf(&g, 0, SEA)).tapped, "nothing paid");
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    let g = base().battlefield(0, &[SEA, "Swamp", "Swamp", "Island"]).start();
    assert_eq!(massacre_costs(&g), vec![0, MASSACRE_FREE]);
}

#[test]
fn cr_205_3_massacres_plains_and_swamp_are_subtypes_on_either_land() {
    // A Plains: Tundra, Scrubland, a basic Plains. A Swamp: a basic Swamp,
    // Underground Sea, Scrubland (Plains Swamp) on P0's side.
    for (theirs, mine) in [("Plains", "Swamp"), ("Scrubland", SEA), ("Tundra", "Scrubland")] {
        let g = scen().battlefield(0, &[mine]).battlefield(1, &[theirs]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
        assert_eq!(massacre_costs(&g), vec![MASSACRE_FREE], "{theirs} / {mine}");
    }
}

#[test]
fn cr_118_9_massacre_isnt_free_without_both() {
    // No Plains on the other side (Volcanic Island, an Island Mountain; a
    // Plains in P1's hand isn't controlled); no Swamp of P0's (Tundra); the
    // Plains on P0's own side (Scrubland) doesn't count ("an opponent
    // controls"): Massacre needs its mana cost, uncastable without mana.
    let cases: [(&[&str], &[&str]); 3] = [(&[SEA], &["Volcanic Island"]), (&["Tundra"], &["Tundra"]), (&["Scrubland"], &["Volcanic Island"])];
    for (mine, theirs) in cases {
        let g = scen().battlefield(0, mine).battlefield(1, theirs).hand(0, &[MASSACRE]).hand(1, &["Plains"]).at(Step::Main1, 0).start();
        assert_eq!(massacre_costs(&g), Vec::<u8>::new(), "{mine:?} vs {theirs:?}");
    }
    // With its mana: only the mana cost.
    let g = scen().battlefield(0, &[SEA, "Swamp", "Swamp", "Swamp"]).battlefield(1, &["Volcanic Island"]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
    assert_eq!(massacre_costs(&g), vec![0]);
}

#[test]
fn cr_305_7_under_magus_of_the_moon_nonbasic_lands_lose_plains_and_swamp() {
    // P1's Magus of the Moon: Tundra and Underground Sea are Mountains only
    // (305.7, 613.1d); basic lands keep their types.
    let case = |mine: &str, theirs: &str| {
        let g = scen().battlefield(0, &[mine]).battlefield(1, &[theirs, MAGUS]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
        massacre_costs(&g)
    };
    assert_eq!(case("Swamp", "Tundra"), Vec::<u8>::new(), "Tundra is no Plains");
    assert_eq!(case(SEA, "Plains"), Vec::<u8>::new(), "Underground Sea is no Swamp");
    assert_eq!(case("Swamp", "Plains"), vec![MASSACRE_FREE], "basic lands keep their types");
}

#[test]
fn cr_118_9d_a_cost_increase_still_applies_to_the_free_cast() {
    // P1's Thalia, Guardian of Thraben ({1} more for noncreature spells): the
    // free cast costs {1}, paid with the Underground Sea (the condition was
    // checked as the cost was chosen, 601.2b). Thalia, a 2/1, dies.
    let mut g = scen().battlefield(0, &[SEA]).battlefield(1, &["Tundra", THALIA]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
    assert_eq!(massacre_costs(&g), vec![MASSACRE_FREE]);
    let c = cast_massacre(&mut g, MASSACRE_FREE);
    assert!(g.obj(bf(&g, 0, SEA)).tapped, "{{1}} paid: {}", log(&g));
    let th = bf(&g, 1, THALIA);
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert_eq!(g.obj(th).zone, Zone::Graveyard);
    // Without any mana: not castable.
    let g = scen().battlefield(0, &["Swamp"]).battlefield(1, &["Tundra", THALIA]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
    let mut g2 = g.clone();
    let s = g2.card(0, "Swamp", Zone::Battlefield);
    g2.set_tapped(s, true);
    g2.refresh_decision();
    assert_eq!(massacre_costs(&g2), Vec::<u8>::new());
    let _ = g;
}

#[test]
fn cr_601_2i_a_free_massacre_is_cast_and_force_of_will_counters_it() {
    // Cast for free it is still a spell cast: Force of Will (exiling a blue
    // card) counters it; nothing gets -2/-2.
    let mut g = scen()
        .battlefield(0, &[SEA, BOW])
        .battlefield(1, &["Tundra", RAGAVAN])
        .hand(0, &[MASSACRE])
        .hand(1, &[FOW, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let c = cast_massacre(&mut g, MASSACRE_FREE);
    // P1 gets priority with Massacre on the stack.
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 1));
    let f = g.card(1, FOW, Zone::Hand);
    assert!(g.has_option(Choice::Cast(f)), "{}", opts(&g));
    g.choose(Choice::Cast(f));
    let mc = g.obj_ref(c);
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => g.offered().into_iter().find(|o| matches!(o, Choice::Number(k) if *k > 0)),
        Some(DecisionKind::Target) => Some(g.tgt(mc.idx)),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert!(on_bf(&g, bf(&g, 0, BOW)) && on_bf(&g, bf(&g, 1, RAGAVAN)), "countered: {}", log(&g));
    assert_eq!(g.pt(bf(&g, 0, BOW)), (1, 1));
}

#[test]
fn cr_704_5f_massacre_kills_both_sides_creatures_at_once_dies_triggers_and_a_later_token() {
    // CR 611.2c, 704.3: every creature gets -2/-2; those with toughness 2 or
    // less die at once in one state-based check: P0's Bowmasters and its Orc
    // Army, P1's Haywire Mite (dies: P1 gains 2 life) and Sanguine Evangelist
    // (dies: a 1/1 Bat token). The Bat entered after Massacre's effect began:
    // not affected (611.2c), a 1/1 flier. Brazen Borrower (3/1) dies too;
    // Murktide Regent with two counters (5/5) is a 3/3.
    let mut g = scen()
        .battlefield(0, &[SEA, "Swamp", "Swamp", "Island", "Island"])
        .battlefield(1, &["Tundra", MITE, EVANGELIST, BORROWER, MURKTIDE])
        .counters(1, MURKTIDE, Counter::P1P1, 2)
        .hand(0, &[BOW, MASSACRE])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, BOW, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    let army = g.battlefield().iter().copied().find(|&x| g.is_token(x)).expect("an Orc Army");
    let life = g.life(1);
    let mk = bf(&g, 1, MURKTIDE);
    cast_massacre(&mut g, MASSACRE_FREE);
    resolve(&mut g);
    for c in [b, army, g.card(1, MITE, Zone::Graveyard), g.card(1, EVANGELIST, Zone::Graveyard), g.card(1, BORROWER, Zone::Graveyard)] {
        assert!(!on_bf(&g, c), "{} died: {}", g.name(c), log(&g));
    }
    assert_eq!(g.life(1), life + 2, "Haywire Mite's trigger");
    let bat = g.battlefield().iter().copied().find(|&x| g.is_token(x) && g.obj(x).controller == 1).expect("the Bat");
    assert_eq!(g.pt(bat), (1, 1), "entered after Massacre resolved: not affected");
    assert_eq!(g.pt(mk), (3, 3));
}

#[test]
fn cr_611_2c_a_creature_entering_or_becoming_one_later_isnt_affected() {
    // After Massacre resolves: P0 flashes in Orcish Bowmasters (a 1/1 with
    // its 1/1 Orc Army) and animates Creeping Tar Pit (a 3/2): neither was a
    // creature when the set was locked.
    let mut g = scen()
        .battlefield(0, &[SEA, TAR_PIT, "Swamp", "Island", "Island", "Island"])
        .battlefield(1, &["Tundra"])
        .hand(0, &[BOW, MASSACRE])
        .at(Step::Main1, 0)
        .start();
    cast_massacre(&mut g, MASSACRE_FREE);
    resolve(&mut g);
    let b = g.card(0, BOW, Zone::Hand);
    g.choose(Choice::Cast(b));
    let tp = bf(&g, 0, TAR_PIT);
    let not_tp = |g: &Game| g.pending_decision().unwrap().options.iter().find(|o| !matches!(o.choice, Choice::Ability(x, _) if x == tp)).map(|o| o.choice);
    until_priority(&mut g, &|g| if g.kind() == Some(DecisionKind::PayMana) { not_tp(g) } else { None });
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    assert_eq!(g.pt(b), (1, 1), "{}", log(&g));
    let army = g.battlefield().iter().copied().find(|&x| g.is_token(x)).expect("an Orc Army");
    assert_eq!(g.pt(army), (1, 1));
    let animate = g
        .offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s @ AbilitySlot::Activated(_)) if x == tp => Some(s),
            _ => None,
        })
        .last()
        .expect("the Tar Pit's abilities");
    g.choose(Choice::Ability(tp, animate));
    until_priority(&mut g, &|g| if g.kind() == Some(DecisionKind::PayMana) { not_tp(g) } else { None });
    resolve(&mut g);
    assert!(g.is_creature(tp));
    assert_eq!(g.pt(tp), (3, 2), "{}", log(&g));
}

#[test]
fn cr_514_2_massacres_effect_ends_in_the_cleanup_step() {
    // P1's Tamiyo (0/3) is a -2/1 this turn, a 0/3 again the next.
    let mut g = scen().battlefield(0, &[SEA]).battlefield(1, &["Tundra", TAMIYO]).hand(0, &[MASSACRE]).at(Step::Main1, 0).start();
    let t = bf(&g, 1, TAMIYO);
    cast_massacre(&mut g, MASSACRE_FREE);
    resolve(&mut g);
    assert!(on_bf(&g, t));
    assert_eq!(g.pt(t), (-2, 1));
    to_turn_of(&mut g, 1);
    assert_eq!(g.pt(t), (0, 3), "{}", log(&g));
}

#[test]
fn cr_613_4c_massacre_against_the_delver_board_and_a_later_bonus() {
    // The Delver deck's creatures: Tamiyo 0/3 lives (-2/1), Dragon's Rage
    // Channeler 1/1 dies, Murktide Regent 3/3 is a 1/1, one with a counter
    // (4/4) a 2/2. P0's own Kaito (a 3/4 creature in P0's turn) is a 1/2; Kaito's
    // +1 emblem afterwards ("Ninjas you control get +1/+1", layer 7c with a
    // later timestamp) makes him 2/3: the modifications add up.
    let mut g = scen()
        .battlefield(0, &[SEA, KAITO])
        .battlefield(1, &["Tundra", TAMIYO, "Dragon's Rage Channeler", MURKTIDE, MURKTIDE])
        .counters(1, MURKTIDE, Counter::P1P1, 1)
        .hand(0, &[MASSACRE])
        .at(Step::Main1, 0)
        .start();
    let (t, k) = (bf(&g, 1, TAMIYO), bf(&g, 0, KAITO));
    let drc = bf(&g, 1, "Dragon's Rage Channeler");
    let murks: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&x| g.name(x) == MURKTIDE).collect();
    let (big, small) = if g.counters_on(murks[0], Counter::P1P1) == 1 { (murks[0], murks[1]) } else { (murks[1], murks[0]) };
    cast_massacre(&mut g, MASSACRE_FREE);
    resolve(&mut g);
    assert_eq!(g.pt(t), (-2, 1));
    assert!(!on_bf(&g, drc), "{}", log(&g));
    assert_eq!((g.pt(big), g.pt(small)), ((2, 2), (1, 1)));
    assert_eq!(g.pt(k), (1, 2));
    let plus = g
        .offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s) if x == k => Some(s),
            _ => None,
        })
        .next()
        .expect("Kaito's loyalty abilities");
    g.choose(Choice::Ability(k, plus));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    assert_eq!(g.pt(k), (2, 3), "{}", log(&g));
}

#[test]
fn cr_708_2_copying_a_face_down_creature_a_2_2_with_no_name_and_the_exception() {
    // CR 707.2, 708.2a: a face-down permanent's copiable values are its
    // face-down characteristics. P1's Abhorrent Oculus manifests dread at
    // P0's upkeep (a face-down 2/2 of mana value 0, the card hidden from
    // P0). Mockingbird (X 0) copies it: a 2/2 colourless creature with no
    // name and no abilities but flying, a Bird; face up (it can't be turned
    // face up), and nothing of the hidden card reaches P0: P0's worlds deal
    // that card again and P0's row stays the same.
    let mut g = scen()
        .library(1, &["Lightning Bolt", BORROWER, "Island", "Island"])
        .battlefield(0, &["Island"; 2])
        .battlefield(1, &["Abhorrent Oculus", "Island"])
        .hand(0, &[MB])
        .at(Step::End, 1)
        .start();
    for _ in 0..100 {
        if g.kind() == Some(DecisionKind::PutOntoBattlefield) {
            break;
        }
        let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::PutOntoBattlefield), "{}\n{}", opts(&g), log(&g));
    let fd = g.card(1, BORROWER, Zone::Library);
    g.choose(Choice::Card(fd));
    for _ in 0..100 {
        if g.turn().step == Step::Main1 && at_priority(&g) && g.stack().is_empty() {
            break;
        }
        let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    assert!(g.is_face_down(fd) && on_bf(&g, fd) && !g.knows(0, fd), "{}", log(&g));
    let c = cast_mb(&mut g, 0, 0);
    assert!(copy_options(&g).contains(&fd), "mana value 0: {}", opts(&g));
    copy(&mut g, Some(fd));
    resolve(&mut g);
    assert!(g.is_copy(c) && !g.is_face_down(c));
    assert_eq!(g.name(c), "");
    assert_eq!(g.pt(c), (2, 2));
    assert_eq!(g.colors_of(c), 0);
    assert!(g.has_kw(c, Kw::Flying) && g.has_subtype(c, sub("Bird")));
    assert!(!g.has_kw(c, Kw::Flash), "none of the hidden card's abilities");
    assert_eq!(g.copiable(c).def, db().face_down_def());
    let x0 = Row::of(&g, 0).flat();
    for seed in 0..8 {
        let mut w = g.clone();
        w.determinize(0, &mut engine::rng::Rng::new(seed));
        assert!(w.is_copy(c) && w.copiable(c) == g.copiable(c));
        assert!(Row::of(&w, 0).flat() == x0, "P0's row differs in a world (seed {seed})");
    }
}

#[test]
fn cr_601_2h_mana_from_lotus_petal_counts() {
    // ruling: 5a1ea5ca
    // Mana from any source counts: X = 1 paid with Lotus Petal's mana (its
    // sacrifice) and an Island: two mana spent, Brazen Borrower (3) out of
    // reach.
    let mut g = scen().battlefield(0, &["Lotus Petal", "Island"]).battlefield(1, &[BORROWER]).hand(0, &[MB]).at(Step::Main1, 0).start();
    let c = cast_mb(&mut g, 0, 1);
    assert_eq!(mana_spent(&g, c), 2, "{}", log(&g));
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Graveyard), 1);
    assert!(!copy_options(&g).contains(&bf(&g, 1, BORROWER)));
}
