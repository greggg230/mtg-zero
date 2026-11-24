//! Lane A of the Legacy Lands cards (ENGINE.md 270.1) against their Oracle
//! text, the Comprehensive Rules and their official rulings (`// ruling:
//! <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card table
//! of constructed play only (`CardDb::constructed`).
//!
//! Dark Depths, Legendary Snow Land: "Dark Depths enters with ten ice
//! counters on it. / {3}: Remove an ice counter from Dark Depths. / When Dark
//! Depths has no ice counters on it, sacrifice it. If you do, create Marit
//! Lage, a legendary 20/20 black Avatar creature token with flying and
//! indestructible."
//!
//! Thespian's Stage, Land: "{T}: Add {C}. / {2}, {T}: This land becomes a
//! copy of target land, except it has this ability."
//!
//! Blast Zone, Land: "This land enters with a charge counter on it. / {T}:
//! Add {C}. / {X}{X}, {T}: Put X charge counters on this land. / {3}, {T},
//! Sacrifice this land: Destroy each nonland permanent with mana value equal
//! to the number of charge counters on this land."
//!
//! Ghost Quarter, Land: "{T}: Add {C}. / {T}, Sacrifice this land: Destroy
//! target land. Its controller may search their library for a basic land
//! card, put it onto the battlefield, then shuffle."
//!
//! Maze of Ith, Land: "{T}: Untap target attacking creature. Prevent all
//! combat damage that would be dealt to and dealt by that creature this
//! turn."
//!
//! Bojuka Bog, Land: "This land enters tapped. / When this land enters, exile
//! target player's graveyard. / {T}: Add {B}."

use engine::db::{types, AbilitySlot, CardDb, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const DEPTHS: &str = "Dark Depths";
const STAGE: &str = "Thespian's Stage";
const BLAST: &str = "Blast Zone";
const GQ: &str = "Ghost Quarter";
const MAZE: &str = "Maze of Ith";
const BOG: &str = "Bojuka Bog";
const LAGE: &str = "Marit Lage";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

/// No auto-answers; ten-card libraries (each test may give its own).
fn bare() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn all_bf(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name).collect()
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ice() -> Counter {
    Counter::Named(db().subtype_id("counter:ice").expect("the ice counter is interned"))
}

fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").expect("the charge counter is interned"))
}

/// The answers to decisions until priority: `answer` first, else the first
/// option.
fn answer_until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}", log(g));
}

/// Take action `c` (it must be offered) and answer what it asks.
fn act_with(g: &mut Game, c: Choice, answer: impl Fn(&Game) -> Option<Choice>) {
    assert!(g.has_option(c), "{c:?} offered: P{:?} {:?} {} stack {:?}
{}", g.pending_decision().map(|d| d.player), g.kind(), opts(g), stack_names(g), log(g));
    g.choose(c);
    answer_until_priority(g, &answer);
}

/// Resolve the stack (both players passing), `answer` answering.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..800 {
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
    panic!("resolve_with: no progress
{}", log(g));
}

/// Pass priority (and take each other decision's first option) until `stop`.
fn pass_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..800 {
        if stop(g) || g.is_over() {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    panic!("pass_until: no progress
{}", log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Resolve the top item only: both players pass once (answers by `answer`).
fn resolve_top_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    let id = g.stack().last().expect("a stack item").id;
    for _ in 0..400 {
        if !g.stack().iter().any(|s| s.id == id) && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_top: no progress\n{}", log(g));
}

fn resolve_top(g: &mut Game) {
    resolve_top_with(g, |_| None);
}

/// The activated ability `k` of `c` (its slot among the permanent's).
fn ability(c: CardIdx, k: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(k))
}

/// The slot of `c`'s first activated ability that isn't a mana ability.
fn nonmana_slot(g: &Game, c: CardIdx, nth: usize) -> u8 {
    g.def_of(c).activated.iter().enumerate().filter(|(_, a)| a.mana().is_none()).nth(nth).map(|(i, _)| i as u8).unwrap_or_else(|| panic!("{}: no such ability", g.name(c)))
}

/// Pay mana with the permanents named `with` first (else the first option).
fn pay_with_names(g: &Game, with: &[&str]) -> Option<Choice> {
    let d = g.pending_decision()?;
    d.options.iter().map(|o| o.choice).find(|c| match c {
        Choice::Card(x) | Choice::Ability(x, _) => with.contains(&g.name(*x)),
        _ => false,
    })
}

fn tgt(g: &Game, c: CardIdx) -> Choice {
    g.tgt(c)
}

fn stack_names(g: &Game) -> Vec<String> {
    g.stack()
        .iter()
        .map(|s| match s.kind {
            engine::game::StackKind::Spell { card } => g.name(card).to_string(),
            engine::game::StackKind::Ability { ability, .. } => format!("ability of {}", db().def(ability.def).name),
        })
        .collect()
}

fn lages(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    all_bf(g, p, LAGE)
}

/// Activate Dark Depths' "{3}: Remove an ice counter" `n` times, each
/// resolved, paid by the Islands and other lands the player has (the
/// tests' boards give enough).
fn remove_ice(g: &mut Game, depths: CardIdx, n: usize) {
    for _ in 0..n {
        let k = nonmana_slot(g, depths, 0);
        act_with(g, ability(depths, k), |g| pay_with_names(g, &["Island", "Mountain", "Forest", "Swamp", "Plains"]));
        resolve_top(g);
    }
}

// ---- Dark Depths -------------------------------------------------------------

/// The card: a legendary snow land with no mana ability (the ruling), its
/// "{3}" ability not a mana ability (CR 605.1a), and its state trigger.
/// ruling: f01a8c16
#[test]
fn cr_205_4_dark_depths_is_a_legendary_snow_land_without_a_mana_ability() {
    let d = db().def(db().id(DEPTHS).unwrap());
    assert!(d.is(types::LAND) && d.types == types::LAND);
    assert_eq!(d.supertypes, engine::db::supertypes::LEGENDARY | engine::db::supertypes::SNOW);
    assert_eq!(d.activated.len(), 1, "{:?}", d.activated);
    assert!(d.activated[0].mana().is_none(), "no mana ability");
    let g = bare().battlefield(0, &[DEPTHS]).at(Step::Main1, 0).start();
    let c = bf(&g, 0, DEPTHS);
    assert!(!g.offered().iter().any(|o| matches!(o, Choice::Ability(x, _) if *x == c)), "nothing to activate without {{3}}: {}", opts(&g));
}

/// "Dark Depths enters with ten ice counters on it" (CR 614.1c, 122.6):
/// played from the hand it is on the battlefield with ten; "{3}: Remove an
/// ice counter" removes one as it resolves.
#[test]
fn cr_614_1c_dark_depths_enters_with_ten_ice_counters_and_its_ability_removes_one() {
    let mut g = bare().hand(0, &[DEPTHS]).battlefield(0, &["Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let c = g.card(0, DEPTHS, Zone::Hand);
    act_with(&mut g, Choice::Play(c), |_| None);
    let c = bf(&g, 0, DEPTHS);
    assert_eq!(g.counters_on(c, ice()), 10);
    remove_ice(&mut g, c, 1);
    assert_eq!(g.counters_on(c, ice()), 9, "{}", log(&g));
}

/// CR 603.8: the last ice counter removed, the state trigger triggers; as it
/// resolves Dark Depths is sacrificed and Marit Lage is created: a legendary
/// 20/20 black Avatar creature token with flying and indestructible.
#[test]
fn cr_603_8_no_ice_counters_sacrifices_dark_depths_and_creates_marit_lage() {
    let mut g = bare().battlefield(0, &[DEPTHS, "Island", "Island", "Island"]).counters(0, DEPTHS, ice(), 1).at(Step::Main1, 0).start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "{}", log(&g));
    resolve(&mut g);
    assert!(all_bf(&g, 0, DEPTHS).is_empty(), "sacrificed");
    assert_eq!(g.player(0).graveyard.iter().map(|&x| g.name(x)).collect::<Vec<_>>(), [DEPTHS]);
    let l = lages(&g, 0);
    assert_eq!(l.len(), 1, "{}", log(&g));
    let m = l[0];
    let o = g.obj(m);
    assert_eq!(o.controller, 0);
    assert_eq!(g.pt(m), (20, 20));
    assert!(g.is_legendary(m) && g.is_creature(m) && g.is_token(m));
    assert_eq!(g.colors_of(m), 1 << engine::mana::B);
    assert!(g.has_kw(m, Kw::Flying) && g.has_kw(m, Kw::Indestructible));
    assert!(g.has_subtype(m, db().subtype_id("Avatar").unwrap()));
    assert_eq!(g.mana_value_now(m), 0, "a token's mana value is 0");
}

/// Dark Depths with no ice counters, the trigger on the stack (CR 603.8):
/// it doesn't trigger again while its ability is on the stack, though the
/// condition still holds and priority passes back and forth (Dark Depths'
/// ability activated again, removing nothing).
/// ruling: faf3075b
#[test]
fn cr_603_8_the_state_trigger_does_not_trigger_again_while_on_the_stack() {
    let mut g = bare().battlefield(0, &[DEPTHS, "Island", "Island", "Island", "Island", "Island", "Island"]).counters(0, DEPTHS, ice(), 1).at(Step::Main1, 0).start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    assert_eq!(stack_names(&g).len(), 1);
    // Activated again with the trigger on the stack: resolves, removes nothing.
    let k = nonmana_slot(&g, c, 0);
    act_with(&mut g, ability(c, k), |_| None);
    assert_eq!(stack_names(&g).len(), 2);
    resolve_top(&mut g);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "still one instance\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1, "one Marit Lage\n{}", log(&g));
}

/// Countered (Consign to Memory: "counter target triggered ability"), the
/// state trigger has left the stack; Dark Depths is still there with no
/// ice counters, so it triggers again at once (CR 603.8, the ruling).
/// ruling: faf3075b
#[test]
fn cr_603_8_a_countered_state_trigger_triggers_again() {
    let mut g = bare()
        .battlefield(0, &[DEPTHS, "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .battlefield(1, &["Island"])
        .hand(1, &["Consign to Memory"])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")]);
    let trig = g.stack()[0].id;
    // P1 counters it.
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    let consign = g.card(1, "Consign to Memory", Zone::Hand);
    act_with(&mut g, Choice::Cast(consign), |g| {
        g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(Target::Stack(id)) if *id == trig))
    });
    resolve_top(&mut g);
    assert!(!g.stack().iter().any(|s| s.id == trig), "countered\n{}", log(&g));
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "triggered again at once\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1, "{}", log(&g));
    assert!(all_bf(&g, 0, DEPTHS).is_empty());
}

/// Dark Depths destroyed in response (Ghost Quarter): its trigger resolves
/// but can't sacrifice it, so no Marit Lage (CR 701.21a; "if you do").
/// ruling: 79635c14
#[test]
fn cr_701_21a_dark_depths_gone_before_its_trigger_resolves_makes_no_marit_lage() {
    let mut g = bare()
        .battlefield(0, &[DEPTHS, "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .battlefield(1, &[GQ])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    g.pass();
    let gq = bf(&g, 1, GQ);
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, c)]));
    resolve(&mut g);
    assert!(lages(&g, 0).is_empty() && lages(&g, 1).is_empty(), "{}", log(&g));
    assert!(g.player(0).graveyard.iter().any(|&x| g.name(x) == DEPTHS));
}

/// The state trigger is an ability of the permanent (CR 113.6, 603.8):
/// Dark Depths in a hand or a graveyard has no ice counters and nothing
/// triggers.
#[test]
fn cr_113_6_dark_depths_off_the_battlefield_does_not_trigger() {
    let mut g = bare().hand(0, &[DEPTHS]).graveyard(0, &[DEPTHS]).battlefield(0, &["Island"]).at(Step::Main1, 0).start();
    g.drive_until(at_priority);
    assert!(g.stack().is_empty() && lages(&g, 0).is_empty(), "{}", log(&g));
    g.pass();
    g.drive_until(at_priority);
    assert!(g.stack().is_empty() && lages(&g, 0).is_empty(), "{}", log(&g));
}

/// Magus of the Moon makes Dark Depths a Mountain (CR 305.7, 613.1d): it
/// loses its abilities, the state trigger too, so with no ice counters it
/// doesn't trigger; once the Magus is gone (Lightning Bolt) the ability is
/// back and, the condition holding, it triggers (CR 603.8).
#[test]
fn cr_305_7_dark_depths_as_a_mountain_has_no_state_trigger_until_the_magus_leaves() {
    let mut g = bare()
        .battlefield(0, &[DEPTHS, "Mountain"])
        .counters(0, DEPTHS, ice(), 0)
        .battlefield(1, &["Magus of the Moon"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    g.drive_until(at_priority);
    assert!(g.stack().is_empty(), "a Mountain: no ability\n{}", log(&g));
    let magus = bf(&g, 1, "Magus of the Moon");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    act_with(&mut g, Choice::Cast(bolt), |g| pick(g, &[tgt(g, magus)]));
    resolve_top(&mut g);
    assert!(all_bf(&g, 1, "Magus of the Moon").is_empty(), "{}", log(&g));
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "the condition holds: it triggers\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1);
}

/// "Sacrifice it": only a permanent its controller controls (CR 701.21a).
/// The other player gains control of Dark Depths with its trigger on the
/// stack: the trigger's controller can't sacrifice it, so no Marit Lage for
/// them; the new controller's Dark Depths still has no ice counters and its
/// ability, now theirs, triggers (603.8, 603.3a): they sacrifice it and get
/// Marit Lage.
#[test]
fn cr_701_21a_only_the_controller_sacrifices_and_the_new_controller_s_trigger_follows() {
    let mut g = bare().battlefield(0, &[DEPTHS, "Island", "Island", "Island"]).counters(0, DEPTHS, ice(), 1).at(Step::Main1, 0).start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    assert_eq!(g.stack()[0].controller, 0);
    g.take_control(c, 1);
    resolve_top(&mut g);
    assert!(lages(&g, 0).is_empty(), "P0 can't sacrifice P1's permanent\n{}", log(&g));
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "it triggers for P1\n{}", log(&g));
    assert_eq!(g.stack()[0].controller, 1);
    resolve(&mut g);
    assert_eq!(lages(&g, 1).len(), 1, "{}", log(&g));
    assert!(g.player(0).graveyard.iter().any(|&x| g.name(x) == DEPTHS), "its owner's graveyard");
}

/// Marit Lage is legendary (CR 704.5j): a second one makes its controller
/// keep one. Karakas returns it to its owner's hand, where a token ceases
/// to exist (CR 111.7, 704.5d).
#[test]
fn cr_704_5j_two_marit_lages_and_karakas_bouncing_one() {
    let mut g = bare()
        .battlefield(0, &[DEPTHS, "Island", "Island", "Island", "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .battlefield(1, &["Karakas"])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    resolve(&mut g);
    let m = lages(&g, 0)[0];
    // A second Dark Depths (a Stage would do it too): put by hand.
    let mut g2 = bare()
        .battlefield(0, &[DEPTHS, DEPTHS, "Island", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    g2.drive_until(|g| g.kind() == Some(DecisionKind::LegendRule) || at_priority(g));
    assert_eq!(g2.kind(), Some(DecisionKind::LegendRule), "two Dark Depths");
    // Karakas bounces Marit Lage.
    g.pass();
    let k = bf(&g, 1, "Karakas");
    let slot = nonmana_slot(&g, k, 0);
    act_with(&mut g, ability(k, slot), |g| pick(g, &[tgt(g, m)]));
    resolve(&mut g);
    assert!(lages(&g, 0).is_empty(), "{}", log(&g));
    assert!(g.player(0).hand.iter().all(|&x| g.name(x) != LAGE), "a token in a hand ceases to exist");
}

/// Two Marit Lages under one player (CR 704.5j): they keep one.
#[test]
fn cr_704_5j_a_second_marit_lage_and_the_legend_rule() {
    let mut g = bare()
        .battlefield(0, &[DEPTHS, STAGE, "Island", "Island", "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .battlefield(1, &[DEPTHS])
        .counters(1, DEPTHS, ice(), 5)
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, c, 1);
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1);
    let s = bf(&g, 0, STAGE);
    let k = stage_slot(&g, s);
    act_with(&mut g, ability(s, k), |g| pick(g, &[tgt(g, bf(g, 1, DEPTHS))]));
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::LegendRule) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule), "{}", log(&g));
    let ls = lages(&g, 0);
    assert_eq!(ls.len(), 2);
    g.choose(Choice::Card(ls[1]));
    g.drive_until(at_priority);
    assert_eq!(lages(&g, 0), [ls[1]]);
}

fn pick(g: &Game, prefs: &[Choice]) -> Option<Choice> {
    prefs.iter().copied().find(|&c| g.has_option(c))
}

// ---- Thespian's Stage --------------------------------------------------------

/// Thespian's Stage's copy ability's slot on `c` (its own, or kept by a
/// copy, CR 707.9b).
fn stage_slot(g: &Game, c: CardIdx) -> u8 {
    let s = db().def(db().id(STAGE).unwrap());
    let a = s.activated.iter().find(|a| a.mana().is_none()).unwrap();
    g.def_of(c).activated.iter().position(|x| x == a).map(|i| i as u8).unwrap_or_else(|| panic!("{} has no Stage ability", g.name(c)))
}

/// The Stage `stage` becomes a copy of `target` (its ability activated,
/// paid with the player's other lands, resolved).
fn stage_copy(g: &mut Game, stage: CardIdx, target: CardIdx) {
    let k = stage_slot(g, stage);
    act_with(g, ability(stage, k), |g| pick(g, &[tgt(g, target)]).or_else(|| pay_with_names(g, &["Island", "Plains", "Forest", "Mountain", "Swamp"])).or_else(|| pay_not(g, stage)));
    resolve_top(g);
}

/// A mana payment's first source other than `not`.
fn pay_not(g: &Game, not: CardIdx) -> Option<Choice> {
    let d = g.pending_decision()?;
    d.options.iter().map(|o| o.choice).find(|c| match c {
        Choice::Card(x) | Choice::Ability(x, _) => *x != not,
        _ => false,
    })
}

/// Thespian's Stage becomes a copy of Dark Depths (CR 707.2, 707.9b): no ice
/// counters (it didn't enter: "enters with" doesn't apply, the ruling), so
/// its state trigger triggers (603.8); P0 keeps the copy by the legend rule
/// (704.5j: both are "Dark Depths"), the trigger sacrifices it and creates
/// Marit Lage.
/// ruling: 56b267d6
#[test]
fn cr_707_9b_thespians_stage_copying_dark_depths_makes_marit_lage() {
    let mut g = bare().battlefield(0, &[STAGE, DEPTHS, "Island", "Island"]).counters(0, DEPTHS, ice(), 10).at(Step::Main1, 0).start();
    let (s, d) = (bf(&g, 0, STAGE), bf(&g, 0, DEPTHS));
    let k = stage_slot(&g, s);
    act_with(&mut g, ability(s, k), |g| pick(g, &[tgt(g, d)]).or_else(|| pay_with_names(g, &["Island"])));
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::LegendRule) || g.stack().is_empty());
    assert_eq!(g.name(s), DEPTHS, "{}", log(&g));
    assert_eq!(g.counters_on(s, ice()), 0, "no ice counters");
    assert!(g.is_legendary(s) && g.obj(s).tapped);
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule), "{}", log(&g));
    g.choose(Choice::Card(s));
    g.drive_until(at_priority);
    assert!(g.player(0).graveyard.iter().any(|&x| g.name(x) == DEPTHS), "the real one goes");
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "{}", log(&g));
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1, "{}", log(&g));
    assert!(g.player(0).graveyard.iter().any(|&x| g.name(x) == STAGE), "the Stage was sacrificed");
}

/// The other choice: P0 keeps the real Dark Depths (ten ice counters); the
/// copy goes to the graveyard and its trigger, already triggered, can't
/// sacrifice it: no Marit Lage (CR 603.8 then 704.5j; the Dark Depths
/// ruling).
/// ruling: 79635c14
#[test]
fn cr_704_5j_keeping_the_real_dark_depths_leaves_the_copy_s_trigger_with_nothing() {
    let mut g = bare().battlefield(0, &[STAGE, DEPTHS, "Island", "Island"]).counters(0, DEPTHS, ice(), 10).at(Step::Main1, 0).start();
    let (s, d) = (bf(&g, 0, STAGE), bf(&g, 0, DEPTHS));
    let k = stage_slot(&g, s);
    act_with(&mut g, ability(s, k), |g| pick(g, &[tgt(g, d)]).or_else(|| pay_with_names(g, &["Island"])));
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::LegendRule) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule), "{}", log(&g));
    g.choose(Choice::Card(d));
    g.drive_until(at_priority);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "it had triggered\n{}", log(&g));
    resolve(&mut g);
    assert!(lages(&g, 0).is_empty(), "{}", log(&g));
    assert_eq!(g.counters_on(d, ice()), 10);
}

/// The opponent's Dark Depths: no legend rule (different controllers), the
/// copy's trigger is P0's and P0 gets Marit Lage; P1's Dark Depths keeps its
/// counters.
#[test]
fn cr_707_2_copying_the_opponent_s_dark_depths() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island"]).battlefield(1, &[DEPTHS]).counters(1, DEPTHS, ice(), 10).at(Step::Main1, 0).start();
    let (s, d) = (bf(&g, 0, STAGE), bf(&g, 1, DEPTHS));
    stage_copy(&mut g, s, d);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")], "{}", log(&g));
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1);
    assert_eq!(g.counters_on(d, ice()), 10);
}

/// The copy loses the Stage's first ability ("{T}: Add {C}") and keeps the
/// copy ability (CR 707.9b, the ruling); it stays tapped (the ruling); the
/// copy effect has no duration and another one overwrites it (613.7): a
/// Forest the next turn, with the Stage's ability again.
/// ruling: 7bf3dd51
/// ruling: d4cd124c
#[test]
fn cr_707_9b_the_copy_keeps_the_stage_ability_stays_tapped_and_is_overwritten() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island", "Island", "Island"]).battlefield(1, &[GQ, "Forest"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, STAGE);
    let t = bf(&g, 1, GQ);
    stage_copy(&mut g, s, t);
    assert_eq!(g.name(s), GQ);
    assert!(g.obj(s).tapped, "not untapped");
    // Ghost Quarter's two abilities and the Stage's.
    assert_eq!(g.def_of(s).activated.len(), 3, "{:?}", g.def_of(s).activated);
    assert_eq!(g.def_of(s).activated.iter().filter(|a| a.mana().is_some()).count(), 1, "Ghost Quarter's mana ability only");
    pass_until_main1(&mut g, 1);
    pass_until_main1(&mut g, 0);
    let f = bf(&g, 1, "Forest");
    stage_copy(&mut g, s, f);
    assert_eq!(g.name(s), "Forest", "{}", log(&g));
    assert!(g.has_subtype(s, db().subtype_id("Forest").unwrap()));
    assert_eq!(g.def_of(s).supertypes, engine::db::supertypes::BASIC, "a basic land's copiable values: Basic too");
    stage_slot(&g, s);
}

fn pass_until_main1(g: &mut Game, p: PlayerId) {
    for _ in 0..3000 {
        if g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty() {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("pass_until_main1: no progress\n{}", log(g));
}

/// Copiable values only (CR 707.2; the ruling): counters and other effects
/// aren't copied. Celestial Colonnade animated (a 4/4 creature until end of
/// turn) is copied as the land; Blast Zone with charge counters is copied
/// without them.
/// ruling: 6445c953
#[test]
fn cr_707_2_the_stage_copies_the_printed_land_not_its_effects_or_counters() {
    let mut g = bare()
        .battlefield(0, &[STAGE, STAGE, "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Celestial Colonnade", BLAST, "Island", "Island", "Island", "Plains", "Plains"])
        .counters(1, BLAST, charge(), 3)
        .at(Step::Main1, 1)
        .start();
    let col = bf(&g, 1, "Celestial Colonnade");
    g.set_tapped(col, false);
    let k = nonmana_slot(&g, col, 0);
    act_with(&mut g, ability(col, k), |g| pay_not(g, col));
    resolve_top(&mut g);
    assert!(g.is_creature(col), "animated\n{}", log(&g));
    // P1 passes priority with the Colonnade animated: P0 acts (instant
    // speed, P1's main phase).
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 0, "{}", log(&g));
    assert!(g.is_creature(col), "still animated this turn");
    let st = all_bf(&g, 0, STAGE);
    let blast = bf(&g, 1, BLAST);
    stage_copy(&mut g, st[0], blast);
    assert_eq!(g.name(st[0]), BLAST, "{}", log(&g));
    assert_eq!(g.counters_on(st[0], charge()), 0, "no counters copied, none put (it didn't enter)");
    g.pass();
    stage_copy(&mut g, st[1], col);
    assert_eq!(g.name(st[1]), "Celestial Colonnade", "{}", log(&g));
    assert!(!g.is_creature(st[1]), "the land, unanimated");
}

/// No enters abilities of the copied land (the ruling): copying Bojuka Bog
/// exiles no graveyard; it isn't tapped by "enters tapped" either (it
/// stays as it was).
/// ruling: 56b267d6
#[test]
fn cr_603_6a_the_stage_copying_bojuka_bog_has_no_enters_trigger() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island"]).battlefield(1, &[BOG]).graveyard(1, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, STAGE);
    let t = bf(&g, 1, BOG);
    stage_copy(&mut g, s, t);
    assert_eq!(g.name(s), BOG);
    assert!(g.stack().is_empty(), "{}", log(&g));
    assert_eq!(g.player(1).graveyard.len(), 1);
}

/// "Target land": the Stage may target itself (CR 115.1, 602.2b); it becomes
/// a copy of itself, which changes nothing.
#[test]
fn cr_707_2_the_stage_targeting_itself() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, STAGE);
    let k = stage_slot(&g, s);
    act_with(&mut g, ability(s, k), |g| pick(g, &[tgt(g, s)]).or_else(|| pay_not(g, s)));
    resolve(&mut g);
    assert_eq!(g.name(s), STAGE, "{}", log(&g));
    assert_eq!(g.def_of(s).activated.len(), 2);
}

/// Urza's Saga copied (CR 714, 707.2): an Enchantment Land - Urza's Saga
/// with no lore counters, so not sacrificed (714.4); as its controller's
/// next precombat main phase begins a lore counter is put on it (714.3b) and
/// chapter I triggers. It keeps the Stage's ability (707.9b).
#[test]
fn cr_714_3b_the_stage_copying_urzas_saga() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island"]).battlefield(1, &["Urza's Saga"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, STAGE);
    let t = bf(&g, 1, "Urza's Saga");
    stage_copy(&mut g, s, t);
    g.drive_until(at_priority);
    assert_eq!(g.name(s), "Urza's Saga", "{}", log(&g));
    assert!(g.is_type(s, types::ENCHANTMENT) && g.is_type(s, types::LAND));
    assert_eq!(g.counters_on(s, Counter::Lore), 0);
    assert_eq!(g.obj(s).zone, Zone::Battlefield, "no lore counters: not sacrificed");
    stage_slot(&g, s);
    pass_until_main1(&mut g, 1);
    pass_until_main1(&mut g, 0);
    assert_eq!(g.counters_on(s, Counter::Lore), 1, "{}", log(&g));
}

/// Karakas copied: a legendary land named Karakas (CR 707.2) - with P0's own
/// Karakas the legend rule applies (704.5j); it has Karakas's ability.
#[test]
fn cr_704_5j_the_stage_copying_karakas() {
    let mut g = bare().battlefield(0, &[STAGE, "Karakas", "Island", "Island"]).at(Step::Main1, 0).start();
    let (s, k) = (bf(&g, 0, STAGE), bf(&g, 0, "Karakas"));
    let slot = stage_slot(&g, s);
    act_with(&mut g, ability(s, slot), |g| pick(g, &[tgt(g, k)]).or_else(|| pay_with_names(g, &["Island"])));
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::LegendRule) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule), "{}", log(&g));
    g.choose(Choice::Card(s));
    g.drive_until(at_priority);
    assert_eq!(g.name(s), "Karakas");
    assert!(g.is_legendary(s));
    assert_eq!(g.def_of(s).activated.len(), 3, "Karakas's mana and bounce abilities and the Stage's");
}

// ---- Blast Zone --------------------------------------------------------------

/// Blast Zone's ability `k` (0: its mana ability, 1: {X}{X}, 2: the
/// destruction), as the card lists them.
fn blast_slot(g: &Game, c: CardIdx, k: usize) -> u8 {
    nonmana_slot(g, c, k)
}

/// Played, Blast Zone enters with a charge counter (CR 122.6, 614.1c);
/// "{X}{X}, {T}" pays twice X (CR 107.3; the ruling): X = 2 taps four Islands
/// and puts two more.
/// ruling: 3f985fb3
#[test]
fn cr_107_3_blast_zone_enters_with_a_charge_counter_and_x_x_pays_twice_x() {
    let mut g = bare().hand(0, &[BLAST]).battlefield(0, &["Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let c = g.card(0, BLAST, Zone::Hand);
    act_with(&mut g, Choice::Play(c), |_| None);
    let c = bf(&g, 0, BLAST);
    assert_eq!(g.counters_on(c, charge()), 1);
    let k = blast_slot(&g, c, 0);
    g.choose(ability(c, k));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "X: {}", opts(&g));
    assert_eq!(g.offered().len(), 3, "X = 0, 1 or 2 with five Islands: {}", opts(&g));
    let x2 = g.offered()[2];
    g.choose(x2);
    answer_until_priority(&mut g, &|g| pay_with_names(g, &["Island"]));
    resolve(&mut g);
    assert_eq!(g.counters_on(c, charge()), 3, "{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&x| g.name(x) == "Island" && g.obj(x).tapped).count(), 4);
}

/// "{3}, {T}, Sacrifice this land: Destroy each nonland permanent with mana
/// value equal to the number of charge counters on this land": its counters
/// as it last existed (sacrificed as a cost, CR 608.2h). One counter:
/// Llanowar Elves (1) of both players; Lightning... lands stay, the Dryad
/// Arbor among them (a land creature, mana value 0); a mana value 2 stays.
#[test]
fn cr_608_2h_blast_zone_destroys_each_nonland_permanent_of_its_counters() {
    let mut g = bare()
        .battlefield(0, &[BLAST, "Island", "Island", "Island", "Llanowar Elves"])
        .counters(0, BLAST, charge(), 1)
        .battlefield(1, &["Llanowar Elves", "Dryad Arbor", "Orcish Bowmasters", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, BLAST);
    let k = blast_slot(&g, c, 1);
    act_with(&mut g, ability(c, k), |g| pay_with_names(g, &["Island"]));
    resolve(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert!(all_bf(&g, 0, "Llanowar Elves").is_empty() && all_bf(&g, 1, "Llanowar Elves").is_empty(), "{}", log(&g));
    assert_eq!(all_bf(&g, 1, "Dryad Arbor").len(), 1, "a land");
    assert_eq!(all_bf(&g, 1, "Orcish Bowmasters").len(), 1, "mana value 2");
}

/// With no charge counters (a Thespian's Stage that became a copy of Blast
/// Zone: it didn't enter, so it has none) it destroys the mana value 0
/// permanents: a token (the ruling: a token's mana value is 0 - the Orc
/// Army), a permanent with {X} in its mana cost (X is 0 there: Walking
/// Ballista, the ruling), Mox Diamond; Marit Lage is indestructible and
/// stays (CR 702.12b).
/// ruling: fc7d2534
/// ruling: 0e713983
#[test]
fn cr_202_3_blast_zone_at_zero_destroys_tokens_and_x_permanents() {
    let mut g = bare()
        .battlefield(0, &[STAGE, BLAST, "Island", "Island", "Island", "Island", "Island", DEPTHS])
        .counters(0, BLAST, charge(), 4)
        .counters(0, DEPTHS, ice(), 1)
        .battlefield(1, &["Walking Ballista", "Mox Diamond", "Swamp", "Swamp"])
        .counters(1, "Walking Ballista", Counter::P1P1, 2)
        .hand(1, &["Orcish Bowmasters"])
        .at(Step::Main1, 1)
        .start();
    // P1 makes an Orc Army (Orcish Bowmasters' enters trigger: amass Orcs 1).
    let bow = g.card(1, "Orcish Bowmasters", Zone::Hand);
    act_with(&mut g, Choice::Cast(bow), |_| None);
    resolve(&mut g);
    assert_eq!(all_bf(&g, 1, "Orc Army").len(), 1, "{}", log(&g));
    pass_until_main1(&mut g, 0);
    let d = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, d, 1);
    resolve(&mut g);
    assert_eq!(lages(&g, 0).len(), 1);
    let s = bf(&g, 0, STAGE);
    let bz = bf(&g, 0, BLAST);
    stage_copy(&mut g, s, bz);
    assert_eq!(g.name(s), BLAST);
    assert_eq!(g.counters_on(s, charge()), 0);
    pass_until_main1(&mut g, 1);
    pass_until_main1(&mut g, 0);
    let k = blast_slot(&g, s, 1);
    act_with(&mut g, ability(s, k), |g| pay_with_names(g, &["Island"]));
    resolve(&mut g);
    assert!(all_bf(&g, 1, "Orc Army").is_empty(), "the token (mana value 0)\n{}", log(&g));
    assert!(all_bf(&g, 1, "Walking Ballista").is_empty(), "{{X}}{{X}}: mana value 0");
    assert!(all_bf(&g, 1, "Mox Diamond").is_empty());
    assert_eq!(lages(&g, 0).len(), 1, "indestructible");
    assert_eq!(all_bf(&g, 1, "Orcish Bowmasters").len(), 1);
    assert_eq!(all_bf(&g, 0, BLAST).len(), 1, "the real Blast Zone: a land");
}

// ---- Ghost Quarter -----------------------------------------------------------

/// "{T}: Add {C}" is a mana ability (CR 605.1a); "{T}, Sacrifice this land:
/// Destroy target land. Its controller may search their library for a
/// basic land card, put it onto the battlefield, then shuffle": the
/// destroyed land's controller finds a basic land, untapped (CR 701.23).
#[test]
fn cr_701_23_ghost_quarter_destroys_a_land_and_its_controller_may_search() {
    let mut g = bare().battlefield(0, &[GQ]).battlefield(1, &["Volcanic Island"]).library(1, &["Island", "Mountain", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let gq = bf(&g, 0, GQ);
    assert_eq!(g.def_of(gq).activated.iter().filter(|a| a.mana().is_some()).count(), 1);
    let v = bf(&g, 1, "Volcanic Island");
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, v)]));
    assert_eq!(g.obj(gq).zone, Zone::Graveyard, "sacrificed as a cost");
    let mountain = db().id("Mountain").unwrap();
    resolve_with(&mut g, |g| {
        let d = g.pending_decision().unwrap();
        if d.player == 1 {
            pick(g, &[Choice::Yes, Choice::Def(mountain)])
        } else {
            None
        }
    });
    assert_eq!(g.obj(v).zone, Zone::Graveyard);
    let m = bf(&g, 1, "Mountain");
    assert!(!g.obj(m).tapped, "untapped");
    assert_eq!(g.player(1).library.len(), 12, "the ten Islands of the setup, the Island and the Bolt");
}

/// "May": declined, no land and no search (the library stays as it was).
#[test]
fn cr_701_23_ghost_quarter_s_search_may_be_declined() {
    let mut g = bare().battlefield(0, &[GQ]).battlefield(1, &["Volcanic Island"]).library(1, &["Island", "Mountain", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let gq = bf(&g, 0, GQ);
    let v = bf(&g, 1, "Volcanic Island");
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, v)]));
    let before: Vec<CardIdx> = g.player(1).library.clone();
    resolve_with(&mut g, |g| pick(g, &[Choice::Pass]));
    assert_eq!(g.obj(v).zone, Zone::Graveyard);
    assert_eq!(g.player(1).library, before, "{}", log(&g));
    assert_eq!(g.permanents_of(1).count(), 0);
}

/// Ghost Quarter may target itself (CR 602.2b: targets are chosen before the
/// costs are paid); sacrificed as the cost, its target is gone and the
/// ability doesn't resolve: no search (608.2b; the ruling).
/// ruling: 3aa7df53
#[test]
fn cr_608_2b_ghost_quarter_targeting_itself_does_nothing() {
    let mut g = bare().battlefield(0, &[GQ]).library(0, &["Island", "Island"]).at(Step::Main1, 0).start();
    let gq = bf(&g, 0, GQ);
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, gq)]));
    assert_eq!(g.obj(gq).zone, Zone::Graveyard);
    resolve_with(&mut g, |g| panic!("nothing is asked: {}\n{}", opts(g), log(g)));
    assert_eq!(g.permanents_of(0).count(), 0);
    assert_eq!(g.player(0).library.len(), 12);
}

/// The target illegal on resolution (sacrificed to Crop Rotation in
/// response): the ability doesn't resolve, no search (CR 608.2b; the
/// ruling).
/// ruling: 962d2c28
#[test]
fn cr_608_2b_ghost_quarter_s_target_gone_no_search() {
    let mut g = bare()
        .battlefield(0, &[GQ])
        .battlefield(1, &["Forest", "Volcanic Island"])
        .hand(1, &["Crop Rotation"])
        .library(1, &["Island", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let gq = bf(&g, 0, GQ);
    let v = bf(&g, 1, "Volcanic Island");
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, v)]));
    g.pass();
    let crop = g.card(1, "Crop Rotation", Zone::Hand);
    let island = db().id("Island").unwrap();
    act_with(&mut g, Choice::Cast(crop), |g| pick(g, &[Choice::Card(v)]).or_else(|| pay_with_names(g, &["Forest"])));
    resolve_with(&mut g, |g| pick(g, &[Choice::Def(island)]));
    assert_eq!(g.obj(v).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(all_bf(&g, 1, "Island").len(), 1, "Crop Rotation's land");
    assert!(all_bf(&g, 1, "Mountain").is_empty(), "no search for Ghost Quarter's target's controller\n{}", log(&g));
    assert_eq!(g.player(1).library.len(), 11);
}

/// A constructed table with an indestructible land (a test card: none of
/// the tables has one).
fn db_indestructible_land() -> &'static CardDb {
    static DB: std::sync::OnceLock<&'static CardDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(engine::db::load_legacy_document().cards);
        let c: card_ir::Card = serde_json::from_value(serde_json::json!({"name": "Test Indestructible Land", "custom": true, "printing": {
            "$t": "CardPrinting", "name": "Test Indestructible Land", "card_types": ["Land"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "",
            "subtypes": [], "supertypes": [], "type_line": "Land", "keywords": [{"$t": "Keyword", "kind": "indestructible"}], "produces": ["C"]}}))
        .unwrap();
        cards.push(c);
        let db = CardDb::from_cards(&cards);
        db.expect_id("Test Indestructible Land").unwrap();
        db.leak()
    })
}

/// The land's controller searches even if it wasn't destroyed
/// (indestructible: CR 702.12b; the ruling).
/// ruling: a7f48de4
#[test]
fn cr_702_12b_ghost_quarter_s_search_even_if_the_land_survives() {
    let mut g = Scenario::new(db_indestructible_land())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &[GQ])
        .battlefield(1, &["Test Indestructible Land"])
        .library(0, &["Island"; 5])
        .library(1, &["Forest", "Island"])
        .at(Step::Main1, 0)
        .start();
    let gq = bf(&g, 0, GQ);
    let t = bf(&g, 1, "Test Indestructible Land");
    let k = nonmana_slot(&g, gq, 0);
    act_with(&mut g, ability(gq, k), |g| pick(g, &[tgt(g, t)]));
    let forest = db_indestructible_land().id("Forest").unwrap();
    resolve_with(&mut g, |g| pick(g, &[Choice::Yes, Choice::Def(forest)]));
    assert_eq!(g.obj(t).zone, Zone::Battlefield, "indestructible");
    assert_eq!(all_bf(&g, 1, "Forest").len(), 1, "{}", log(&g));
}

// ---- Maze of Ith -------------------------------------------------------------

/// P0 attacks with `attackers` (they must be untapped and able).
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    pass_until(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
        g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    }
    g.choose(Choice::Pass);
}

/// The defending player blocks `attacker` with whatever it is asked about
/// first (else declines).
fn block(g: &mut Game, attacker: Option<CardIdx>) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step > Step::DeclareBlockers);
    for _ in 0..20 {
        if g.kind() != Some(DecisionKind::DeclareBlockers) {
            return;
        }
        match attacker {
            Some(a) if g.has_option(Choice::Card(a)) => {
                g.choose(Choice::Card(a));
            }
            _ => {
                g.choose(Choice::Pass);
            }
        }
    }
}

/// P1 activates Maze of Ith on `target` now (P1 holding priority).
fn maze(g: &mut Game, m: CardIdx, target: CardIdx) {
    let k = nonmana_slot(g, m, 0);
    act_with(g, ability(m, k), |g| pick(g, &[tgt(g, target)]));
    resolve_top(g);
}

/// Pass priority until `p` holds it in `step`.
fn priority_in(g: &mut Game, p: PlayerId, step: Step) {
    pass_until(g, |g| g.turn().step == step && at_priority(g) && g.pending_decision().unwrap().player == p);
}

/// Maze of Ith has no mana ability (the ruling); "{T}: Untap target
/// attacking creature. Prevent all combat damage that would be dealt to and
/// dealt by that creature this turn" (CR 615.1a): the Mazed attacker,
/// unblocked, deals no damage; untapped.
/// ruling: a5613231
#[test]
fn cr_615_1a_maze_of_ith_prevents_an_unblocked_attacker_s_damage() {
    let mut g = bare().battlefield(0, &["Orcish Bowmasters"]).battlefield(1, &[MAZE]).at(Step::Main1, 0).start();
    let m = bf(&g, 1, MAZE);
    assert!(g.def_of(m).activated.iter().all(|a| a.mana().is_none()), "no mana ability");
    assert_eq!(g.def_of(m).activated.len(), 1);
    let a = bf(&g, 0, "Orcish Bowmasters");
    attack(&mut g, &[a]);
    priority_in(&mut g, 1, Step::DeclareAttackers);
    assert!(g.obj(a).tapped);
    maze(&mut g, m, a);
    assert!(!g.obj(a).tapped, "untapped\n{}", log(&g));
    assert!(g.obj(a).attacking, "still attacking");
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "{}", log(&g));
    // The next turn it deals damage again (514.2: the effect ended).
    pass_until_main1(&mut g, 1);
    pass_until_main1(&mut g, 0);
    attack(&mut g, &[a]);
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 19, "{}", log(&g));
}

/// Blocked, its damage to the blocker and the blocker's to it are both
/// prevented (CR 615.1a): both live. It is still an attacking creature, and
/// still blocked (the ruling: not removed from combat, CR 506.4).
/// ruling: ba3777e8
#[test]
fn cr_506_4_a_mazed_creature_stays_in_combat_and_neither_deals_nor_takes_damage() {
    let mut g = bare().battlefield(0, &["Orcish Bowmasters"]).battlefield(1, &[MAZE, "Llanowar Elves"]).at(Step::Main1, 0).start();
    let a = bf(&g, 0, "Orcish Bowmasters");
    let e = bf(&g, 1, "Llanowar Elves");
    let m = bf(&g, 1, MAZE);
    attack(&mut g, &[a]);
    priority_in(&mut g, 1, Step::DeclareAttackers);
    maze(&mut g, m, a);
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert!(g.has_option(Choice::Card(a)), "it can still be blocked: an attacking creature\n{}", opts(&g));
    block(&mut g, Some(a));
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.obj(e).zone, Zone::Battlefield, "the blocker lives\n{}", log(&g));
    assert_eq!(g.obj(a).zone, Zone::Battlefield, "the attacker lives");
    assert_eq!(g.obj(a).damage, 0);
    assert_eq!(g.obj(e).damage, 0);
}

/// "Combat damage that would be dealt by creatures you control can't be
/// prevented" (Questing Beast): Mazed, it still deals its combat damage
/// (CR 615.12); blocking a Mazed attacker, its damage to it is dealt too
/// (deathtouch: the attacker dies), the attacker's to it prevented.
#[test]
fn cr_615_12_questing_beast_s_damage_is_not_prevented_by_maze_of_ith() {
    let mut g = bare().battlefield(0, &["Questing Beast"]).battlefield(1, &[MAZE]).at(Step::Main1, 0).start();
    let qb = bf(&g, 0, "Questing Beast");
    let m = bf(&g, 1, MAZE);
    attack(&mut g, &[qb]);
    priority_in(&mut g, 1, Step::DeclareAttackers);
    maze(&mut g, m, qb);
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 16, "can't be prevented\n{}", log(&g));
    // Questing Beast blocking a Mazed attacker.
    let mut g = bare().battlefield(0, &["Orcish Bowmasters", MAZE]).battlefield(1, &["Questing Beast"]).at(Step::Main1, 0).start();
    let a = bf(&g, 0, "Orcish Bowmasters");
    let qb = bf(&g, 1, "Questing Beast");
    let m = bf(&g, 0, MAZE);
    attack(&mut g, &[a]);
    priority_in(&mut g, 0, Step::DeclareAttackers);
    maze(&mut g, m, a);
    block(&mut g, Some(a));
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "Questing Beast's damage to it is dealt\n{}", log(&g));
    assert_eq!(g.obj(qb).damage, 0, "its damage to Questing Beast prevented");
}

/// Maze of Ith targets an untapped attacking creature (vigilance: an
/// animated Celestial Colonnade); its combat damage is still prevented (the
/// ruling). Only attacking creatures are targets.
/// ruling: 7d47c20c
#[test]
fn cr_115_1_maze_of_ith_targets_an_untapped_attacker() {
    let mut g = bare()
        .battlefield(0, &["Celestial Colonnade", "Island", "Island", "Island", "Plains", "Plains", "Orcish Bowmasters"])
        .battlefield(1, &[MAZE])
        .at(Step::Main1, 0)
        .start();
    let col = bf(&g, 0, "Celestial Colonnade");
    g.set_tapped(col, false);
    let k = nonmana_slot(&g, col, 0);
    act_with(&mut g, ability(col, k), |g| pay_not(g, col));
    resolve(&mut g);
    assert!(g.is_creature(col));
    let m = bf(&g, 1, MAZE);
    // Not attacking yet: no target, so nothing to activate.
    g.pass();
    assert!(!g.has_option(ability(m, nonmana_slot(&g, m, 0))), "{}", opts(&g));
    let a = col;
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(a));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    priority_in(&mut g, 1, Step::DeclareAttackers);
    assert!(!g.obj(a).tapped, "vigilance");
    let bow = bf(&g, 0, "Orcish Bowmasters");
    let k = nonmana_slot(&g, m, 0);
    g.choose(ability(m, k));
    assert!(!g.has_option(tgt(&g, bow)), "not attacking: no target");
    g.choose(tgt(&g, a));
    answer_until_priority(&mut g, &|_| None);
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "{}", log(&g));
}

/// Activated in the combat damage step or the end of combat step on one's
/// own attacking creature after it dealt its damage: it untaps it and the
/// damage dealt stands (CR 615.1: prevention acts on damage still to come;
/// the ruling).
/// ruling: 8abfa1d8
#[test]
fn cr_615_1_maze_of_ith_after_combat_damage_untaps_and_changes_nothing_dealt() {
    for step in [Step::CombatDamage, Step::EndCombat] {
        let mut g = bare().battlefield(0, &["Orcish Bowmasters", MAZE]).at(Step::Main1, 0).start();
        let a = bf(&g, 0, "Orcish Bowmasters");
        let m = bf(&g, 0, MAZE);
        attack(&mut g, &[a]);
        priority_in(&mut g, 0, step);
        assert_eq!(g.life(1), 19, "{step:?}: dealt\n{}", log(&g));
        assert!(g.obj(a).tapped);
        maze(&mut g, m, a);
        assert!(!g.obj(a).tapped, "{step:?}: untapped");
        pass_until(&mut g, |g| g.turn().step == Step::Main2);
        assert_eq!(g.life(1), 19, "{step:?}");
    }
}

// ---- Bojuka Bog --------------------------------------------------------------

/// "This land enters tapped. / When this land enters, exile target player's
/// graveyard. / {T}: Add {B}." (CR 614.1c, 603.6a, 406.2): played, it is
/// tapped and its trigger exiles the chosen player's graveyard - either
/// player's, its controller's too.
#[test]
fn cr_603_6a_bojuka_bog_enters_tapped_and_exiles_target_player_s_graveyard() {
    for victim in [1u8, 0u8] {
        let mut g = bare()
            .hand(0, &[BOG])
            .graveyard(0, &["Lightning Bolt", "Island"])
            .graveyard(1, &["Brainstorm", "Murktide Regent", "Forest"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, BOG, Zone::Hand);
        g.choose(Choice::Play(c));
        let target = Choice::Target(Target::Player(victim));
        answer_until_priority(&mut g, &|g| pick(g, &[target]));
        let bog = bf(&g, 0, BOG);
        assert!(g.obj(bog).tapped, "enters tapped");
        resolve_with(&mut g, |g| pick(g, &[target]));
        assert!(g.player(victim).graveyard.is_empty(), "P{victim}'s graveyard exiled\n{}", log(&g));
        assert!(!g.player(1 - victim).graveyard.is_empty(), "the other's stays");
        let m = g.def_of(bog).activated.iter().filter_map(|a| a.mana()).count();
        assert_eq!(m, 1, "{{T}}: Add {{B}}");
    }
}

/// With empty graveyards the trigger still asks for a target player and
/// resolves (CR 603.3d: a player is a legal target).
#[test]
fn cr_603_3d_bojuka_bog_with_empty_graveyards() {
    let mut g = bare().hand(0, &[BOG]).at(Step::Main1, 0).start();
    let c = g.card(0, BOG, Zone::Hand);
    g.choose(Choice::Play(c));
    answer_until_priority(&mut g, &|_| None);
    assert_eq!(stack_names(&g), [format!("ability of {BOG}")], "{}", log(&g));
    resolve(&mut g);
}

// ---- the encoding ------------------------------------------------------------

/// Both viewers' rows, layouts v7 and v8.
fn rows(g: &Game) -> Vec<engine::az::tail_encode::Row> {
    use engine::az::tail_encode::Row;
    vec![Row::of(g, 0), Row::of(g, 1), Row::of_v8(g, 0), Row::of_v8(g, 1)]
}

/// The tokens of segment `seg` in a row.
fn tokens(r: &engine::az::tail_encode::Row, seg: u16) -> Vec<&[f32]> {
    r.tail.iter().filter(|t| t.0 == seg).map(|t| &t.2[engine::az::encode::CODE..]).collect()
}

fn digits_of(s: &[f32]) -> u64 {
    s.iter().enumerate().map(|(b, &x)| ((x == 1.0) as u64) << b).sum()
}

/// Dark Depths' ice counters reach both players' inputs exactly (the
/// `counters` segment, ENGINE.md 142.7: a named counter's stable name id and
/// its number's 64 digits), in layouts v7 and v8; Marit Lage has an
/// identity (the token registry's line).
#[test]
fn ice_counters_and_marit_lage_reach_both_players_inputs() {
    use engine::az::tail_encode::{ct, name_id, seg};
    let g = bare().battlefield(0, &[DEPTHS]).counters(0, DEPTHS, ice(), 7).at(Step::Main1, 0).start();
    for r in rows(&g) {
        let found: Vec<u64> = tokens(&r, seg::COUNTERS)
            .into_iter()
            .filter(|s| s[ct::KIND + 4] == 1.0 && digits_of(&s[ct::NAME..ct::NAME + 32]) == name_id("counter:ice") as u64)
            .map(|s| digits_of(&s[ct::COUNT_BITS..ct::COUNT_BITS + 64]))
            .collect();
        assert_eq!(found, [7]);
    }
    let ids = engine::az::identity::identities(db());
    let lage = db().token_by_name(LAGE).expect("Marit Lage's definition");
    assert_ne!(ids[lage as usize], engine::az::identity::UNKNOWN, "Marit Lage is registered (az/tokens.txt)");
    // Every token, face and made definition of constructed play's table has an identity.
    for i in 0..db().len() {
        assert_ne!(ids[i], engine::az::identity::UNKNOWN, "definition {i} ({})", db().def(i as u16).name);
    }
}

/// Maze of Ith's prevention (`Mod::PreventCombatDamage` on that creature: to
/// and by it, ENGINE.md 270.1) reaches both inputs as an `effects` token on
/// that object (its `Mod` float, the object named).
#[test]
fn maze_of_ith_s_prevention_reaches_both_players_inputs() {
    use engine::az::tail_encode::{ef, seg};
    let mut g = bare().battlefield(0, &["Orcish Bowmasters"]).battlefield(1, &[MAZE]).at(Step::Main1, 0).start();
    let a = bf(&g, 0, "Orcish Bowmasters");
    let m = bf(&g, 1, MAZE);
    let before = rows(&g);
    attack(&mut g, &[a]);
    priority_in(&mut g, 1, Step::DeclareAttackers);
    maze(&mut g, m, a);
    for r in rows(&g) {
        let fx = tokens(&r, seg::EFFECTS);
        let prevention: Vec<&&[f32]> = fx.iter().filter(|s| s[ef::MOD + 19] == 1.0).collect();
        assert_eq!(prevention.len(), 1);
        assert_eq!(prevention[0][ef::GROUP], 0.0, "an object's");
        assert!(prevention[0][ef::OBJ..ef::OBJ + engine::az::tail_encode::rf::W].iter().any(|&x| x != 0.0), "the creature named");
    }
    for r in before {
        assert!(tokens(&r, seg::EFFECTS).is_empty());
    }
}

/// Thespian's Stage as a copy of Dark Depths: its copy effect and its kept
/// ability (CR 707.9b; EoE W6's `perm::KEPT_ABILITY`, `g6::KEPT_FROM`) and
/// its state trigger on the stack reach both inputs: the rows differ from
/// the Stage's own.
#[test]
fn the_stage_as_dark_depths_reaches_both_players_inputs() {
    let mut g = bare().battlefield(0, &[STAGE, "Island", "Island"]).battlefield(1, &[DEPTHS]).counters(1, DEPTHS, ice(), 10).at(Step::Main1, 0).start();
    let s = bf(&g, 0, STAGE);
    let before = rows(&g);
    let d = bf(&g, 1, DEPTHS);
    stage_copy(&mut g, s, d);
    assert_eq!(stack_names(&g), [format!("ability of {DEPTHS}")]);
    let after = rows(&g);
    for (a, b) in before.iter().zip(&after) {
        assert!(a.head != b.head || a.tail != b.tail);
        let copies = tokens(b, engine::az::tail_encode::seg::EFFECTS).into_iter().filter(|t| t[engine::az::tail_encode::ef::COPY] == 1.0).count();
        assert_eq!(copies, 1, "the copy effect");
    }
}
