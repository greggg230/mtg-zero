//! The Legacy Lands deck's cross-lane lines (ENGINE.md 270.4): the three lanes' cards together (270.1 Dark Depths,
//! Marit Lage, Thespian's Stage, Blast Zone, Ghost Quarter, Maze of Ith, Bojuka Bog; 270.2 Life from the Loam,
//! Exploration, Carnivorous Cultivator // Enroot, Yavimaya, Cradle of Growth; 270.3 Pithing Needle, Cursed Totem, Torpor
//! Orb, Damping Sphere, Choke, The Tabernacle at Pendrell Vale), each against the Comprehensive Rules (the CR in each
//! name) and the cards' rulings (`// ruling:`), in constructed play's table (`CardDb::constructed`). Also the context
//! floats of the draw instruction in progress (ENGINE.md 270.4, the user's encoding decision of 2026-11-24).

use engine::db::{AbilitySlot, CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const DEPTHS: &str = "Dark Depths";
const STAGE: &str = "Thespian's Stage";
const GQ: &str = "Ghost Quarter";
const MAZE: &str = "Maze of Ith";
const BOG: &str = "Bojuka Bog";
const LAGE: &str = "Marit Lage";
const LOAM: &str = "Life from the Loam";
const EXPLORATION: &str = "Exploration";
const CULTIVATOR: &str = "Carnivorous Cultivator // Enroot";
const YAVIMAYA: &str = "Yavimaya, Cradle of Growth";
const NEEDLE: &str = "Pithing Needle";
const TOTEM: &str = "Cursed Totem";
const ORB: &str = "Torpor Orb";
const SPHERE: &str = "Damping Sphere";
const CHOKE: &str = "Choke";
const TAB: &str = "The Tabernacle at Pendrell Vale";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

/// No auto-answers; ten Islands in each library (each test may give its own).
fn scen() -> Scenario {
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

fn on_bf(g: &Game, p: PlayerId, name: &str) -> bool {
    g.find_card(p, name, Zone::Battlefield).is_some()
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ice() -> Counter {
    Counter::Named(db().subtype_id("counter:ice").expect("the ice counter"))
}

fn subtype(name: &str) -> u16 {
    db().subtype_id(name).unwrap()
}

fn pick(g: &Game, prefs: &[Choice]) -> Option<Choice> {
    prefs.iter().copied().find(|&c| g.has_option(c))
}

/// A payment's source among the permanents named `with` (else `None`).
fn pay_with_names(g: &Game, with: &[&str]) -> Option<Choice> {
    let d = g.pending_decision()?;
    d.options.iter().map(|o| o.choice).find(|c| match c {
        Choice::Card(x) | Choice::Ability(x, _) => with.contains(&g.name(*x)),
        _ => false,
    })
}

const BASICS: [&str; 5] = ["Island", "Forest", "Plains", "Swamp", "Mountain"];

/// Answer the decisions until priority: `answer` first, else the first option.
fn until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}\n{}", opts(g), log(g));
}

/// Take action `c` (it must be offered) and answer what it asks (a payment's basic lands first).
fn act_with(g: &mut Game, c: Choice, answer: &dyn Fn(&Game) -> Option<Choice>) {
    assert!(g.has_option(c), "{c:?} offered: {}\n{}", opts(g), log(g));
    g.choose(c);
    until_priority(g, &|g| answer(g).or_else(|| if g.kind() == Some(DecisionKind::PayMana) { pay_with_names(g, &BASICS) } else { None }));
}

fn cast_with(g: &mut Game, p: PlayerId, name: &str, answer: &dyn Fn(&Game) -> Option<Choice>) {
    let c = hand(g, p, name);
    act_with(g, Choice::Cast(c), answer);
}

/// Resolve the whole stack (both passing), `answer` answering (else the first option).
fn resolve_with(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..1000 {
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
    resolve_with(g, &|_| None);
}

/// Resolve the top item only (both pass once; answers by `answer`).
fn resolve_top_with(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
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

/// Pass priority, declare no attackers or blockers, answer the rest with their first option, until `stop`.
fn pass_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..5000 {
        if stop(g) || g.is_over() {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("pass_until: no progress\n{}", log(g));
}

/// Player `p`'s next precombat main phase, at priority with an empty stack.
fn to_main(g: &mut Game, p: PlayerId) {
    let n = g.turn().number;
    pass_until(g, |g| g.turn().number > n && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty());
}

/// Player `p`'s next upkeep, at priority (its beginning-of-upkeep triggers on the stack).
fn to_upkeep(g: &mut Game, p: PlayerId) {
    let n = g.turn().number;
    pass_until(g, |g| g.turn().number > n && g.turn().active == p && g.turn().step == Step::Upkeep && at_priority(g));
}

fn abilities_offered(g: &Game, c: CardIdx) -> Vec<AbilitySlot> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s) if x == c => Some(s),
            _ => None,
        })
        .collect()
}

/// The slot of `c`'s `nth` activated ability that isn't a mana ability.
fn nonmana_slot(g: &Game, c: CardIdx, nth: usize) -> u8 {
    g.def_of(c).activated.iter().enumerate().filter(|(_, a)| a.mana().is_none()).nth(nth).map(|(i, _)| i as u8).unwrap_or_else(|| panic!("{}: no such ability", g.name(c)))
}

/// Thespian's Stage's copy ability's slot on `c` (its own, or kept by a copy, CR 707.9b).
fn stage_slot(g: &Game, c: CardIdx) -> u8 {
    let s = db().def(def(STAGE));
    let a = s.activated.iter().find(|a| a.mana().is_none()).unwrap();
    g.def_of(c).activated.iter().position(|x| x == a).map(|i| i as u8).unwrap_or_else(|| panic!("{} has no Stage ability", g.name(c)))
}

/// The Stage `stage` becomes a copy of `target` (activated, paid with basics, resolved).
fn stage_copy(g: &mut Game, stage: CardIdx, target: CardIdx) {
    let k = stage_slot(g, stage);
    act_with(g, Choice::Ability(stage, AbilitySlot::Activated(k)), &|g| pick(g, &[g.tgt(target)]));
    resolve_top_with(g, &|g| pick(g, &[g.tgt(target)]));
}

/// Dark Depths' "{3}: Remove an ice counter" `n` times, each resolved, paid by basic lands.
fn remove_ice(g: &mut Game, depths: CardIdx, n: usize) {
    for _ in 0..n {
        let k = nonmana_slot(g, depths, 0);
        act_with(g, Choice::Ability(depths, AbilitySlot::Activated(k)), &|_| None);
        resolve_top_with(g, &|_| None);
    }
}

/// Cast a Pithing Needle (paid by a basic) and name `name` (CR 201.3, 614.12).
fn needle_naming(g: &mut Game, p: PlayerId, name: &str) {
    cast_with(g, p, NEEDLE, &|_| None);
    g.pass_until(|g| g.stack().is_empty() || g.kind() == Some(DecisionKind::NameCard));
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}\n{}", opts(g), log(g));
    g.choose(Choice::Def(def(name)));
    resolve(g);
    assert!(on_bf(g, p, NEEDLE));
}

/// Float the mana of `c`'s mana ability `slot` (else its first) with priority (CR 605.3a).
fn float(g: &mut Game, c: CardIdx, slot: Option<AbilitySlot>) {
    assert!(at_priority(g));
    g.choose(Choice::Yes);
    assert_eq!(g.kind(), Some(DecisionKind::ActivateMana), "{}", opts(g));
    assert!(g.has_option(Choice::Card(c)), "{} offered: {}", g.name(c), opts(g));
    g.choose(Choice::Card(c));
    for _ in 0..50 {
        match g.kind() {
            Some(DecisionKind::ActivateMana) => {
                g.choose(Choice::Pass);
                break;
            }
            Some(DecisionKind::Priority) => break,
            _ => {
                let x = slot.map(|s| Choice::Ability(c, s)).filter(|&x| g.has_option(x)).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
                g.choose(x);
            }
        }
    }
    assert!(at_priority(g), "{}\n{}", opts(g), log(g));
}

/// The copy of a prepare spell in exile (Enroot), if any.
fn enroots(g: &Game) -> Vec<CardIdx> {
    g.exile_zone().iter().copied().filter(|&c| g.is_copy_obj(c) && g.name(c) == "Enroot").collect()
}

/// A dredge question is pending (`Task::Dredge`).
fn dredge_asked(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::ChooseGraveyardCard) && g.agenda_tail(1).contains("Dredge")
}

// ---- Pithing Needle x Dark Depths x Thespian's Stage ----------------------------------------------------------

/// Pithing Needle naming Dark Depths: Dark Depths' "{3}: Remove an ice counter" can't be activated (CR 602.5; not a
/// mana ability, 605.1a). A Thespian's Stage - named Thespian's Stage - can still become a copy of it; the copy is named
/// Dark Depths (707.2), so neither its {3} nor the Stage ability it keeps (707.9b) can be activated; but its state
/// trigger, a triggered ability (603.8), triggers - the copy has no ice counters (it didn't enter, 707.5) - and
/// Marit Lage is created.
#[test]
fn cr_602_5_pithing_needle_naming_dark_depths_stops_its_ability_not_its_state_trigger() {
    // ruling: 044069c9
    let mut g = scen()
        .battlefield(0, &[DEPTHS, "Island", "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 10)
        .hand(0, &[NEEDLE])
        .battlefield(1, &[STAGE, "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let d = bf(&g, 0, DEPTHS);
    assert!(!abilities_offered(&g, d).is_empty(), "before: {}", opts(&g));
    needle_naming(&mut g, 0, DEPTHS);
    assert!(abilities_offered(&g, d).is_empty(), "named: {}", opts(&g));
    to_main(&mut g, 1);
    let s = bf(&g, 1, STAGE);
    assert!(abilities_offered(&g, s).iter().any(|&a| a == AbilitySlot::Activated(stage_slot(&g, s))), "the Stage's name is its own: {}", opts(&g));
    stage_copy(&mut g, s, d);
    assert_eq!(g.name(s), DEPTHS, "{}", log(&g));
    assert_eq!(g.counters_on(s, ice()), 0);
    // Its state trigger is on the stack (603.8) though the Needle names its name.
    assert!(g.stack().iter().any(|i| i.controller == 1), "{}", log(&g));
    // Untapped by hand: still nothing to activate - Dark Depths' {3} and the kept Stage ability are a Dark Depths'.
    g.set_tapped(s, false);
    g.refresh_decision();
    assert!(abilities_offered(&g, s).is_empty(), "the copy is named Dark Depths: {}", opts(&g));
    resolve(&mut g);
    assert_eq!(all_bf(&g, 1, LAGE).len(), 1, "{}", log(&g));
    assert_eq!(g.counters_on(d, ice()), 10, "P0's Dark Depths untouched");
}

/// Pithing Needle naming Thespian's Stage: a Stage's copy ability can't be activated (CR 602.5), its mana ability can
/// (605.1a); a Stage that already became a copy of another land has that land's name (707.2) and activates the Stage
/// ability it keeps (707.9b): the Needle reads the name a source has now (201.2).
#[test]
fn cr_201_2_pithing_needle_naming_thespians_stage_reads_the_name_it_has_now() {
    let mut g = scen()
        .battlefield(1, &[STAGE, STAGE, "Island", "Island", "Island", "Island"])
        .battlefield(0, &[GQ, "Island"])
        .hand(0, &[NEEDLE])
        .at(Step::Main1, 1)
        .start();
    let stages = all_bf(&g, 1, STAGE);
    let (a, b) = (stages[0], stages[1]);
    let t = bf(&g, 0, GQ);
    stage_copy(&mut g, b, t);
    assert_eq!(g.name(b), GQ);
    to_main(&mut g, 0);
    needle_naming(&mut g, 0, STAGE);
    to_main(&mut g, 1);
    assert!(abilities_offered(&g, a).is_empty(), "the Stage's copy ability: {}", opts(&g));
    let mut h = g.clone();
    h.choose(Choice::Yes);
    assert!(h.has_option(Choice::Card(a)), "its mana ability: {}", opts(&h));
    let k = stage_slot(&g, b);
    assert!(abilities_offered(&g, b).contains(&AbilitySlot::Activated(k)), "a Ghost Quarter keeps the Stage ability: {}", opts(&g));
}

// ---- Torpor Orb, Cursed Totem x Dark Depths, Carnivorous Cultivator ---------------------------------------------

/// Torpor Orb stops abilities a creature entering triggers (CR 603.6a): not Dark Depths' state trigger (603.8, no
/// entering). Marit Lage enters; Guide of Souls' "whenever another creature you control enters" doesn't trigger.
#[test]
fn cr_603_8_torpor_orb_doesnt_stop_dark_depths_state_trigger() {
    let mut g = scen()
        .battlefield(0, &[ORB, "Guide of Souls", DEPTHS, "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .at(Step::Main1, 0)
        .start();
    let d = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, d, 1);
    assert_eq!(g.stack().len(), 1, "the state trigger: {}", log(&g));
    resolve(&mut g);
    assert_eq!(all_bf(&g, 0, LAGE).len(), 1, "{}", log(&g));
    assert_eq!(g.life(0), 20, "Guide of Souls didn't trigger: {}", log(&g));
    // (Without the Orb it would: the control.)
    let mut h = scen()
        .battlefield(0, &["Guide of Souls", DEPTHS, "Island", "Island", "Island"])
        .counters(0, DEPTHS, ice(), 1)
        .at(Step::Main1, 0)
        .start();
    let d = bf(&h, 0, DEPTHS);
    remove_ice(&mut h, d, 1);
    resolve(&mut h);
    assert_eq!(h.life(0), 21, "{}", log(&h));
}

/// Carnivorous Cultivator "enters prepared" (CR 723.3a): a status it enters with, no triggered ability - Torpor Orb
/// (603.6a) doesn't stop it; the Enroot copy is in exile and castable. Guide of Souls' trigger on it is stopped.
#[test]
fn cr_723_3a_torpor_orb_leaves_the_cultivator_entering_prepared() {
    let mut g = scen()
        .battlefield(0, &[ORB, "Guide of Souls", "Forest", "Forest", "Forest"])
        .hand(0, &[CULTIVATOR])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, CULTIVATOR, &|_| None);
    resolve(&mut g);
    let c = bf(&g, 0, CULTIVATOR);
    assert!(g.is_prepared(c), "{}", log(&g));
    let e = enroots(&g);
    assert_eq!(e.len(), 1);
    assert!(g.has_option(Choice::Cast(e[0])), "{}", opts(&g));
    assert_eq!(g.life(0), 20, "Guide of Souls stopped: {}", log(&g));
}

/// Cursed Totem: "Activated abilities of creatures can't be activated" (CR 602.5). Casting the prepared Cultivator's
/// Enroot copy is casting a spell (723.3c, 601.2), not an activated ability: castable, and it resolves.
#[test]
fn cr_723_3c_cursed_totem_leaves_the_enroot_copy_castable() {
    let mut g = scen()
        .battlefield(0, &[TOTEM, "Forest", "Forest", "Forest"])
        .library(0, &["Wasteland", "Island", "Island"])
        .hand(0, &[CULTIVATOR])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, CULTIVATOR, &|_| None);
    resolve(&mut g);
    let e = enroots(&g);
    assert_eq!(e.len(), 1, "{}", log(&g));
    act_with(&mut g, Choice::Cast(e[0]), &|_| None);
    resolve_with(&mut g, &|g| {
        (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| g.offered().into_iter().find(|c| matches!(c, Choice::Def(d) if g.db().def(*d).name == "Wasteland"))).flatten()
    });
    assert_eq!(gy(&g, 0), vec!["Wasteland"], "{}", log(&g));
    assert!(!g.is_prepared(bf(&g, 0, CULTIVATOR)));
}

// ---- The Tabernacle at Pendrell Vale x Marit Lage, the Cultivator ---------------------------------------------

/// 'All creatures have "At the beginning of your upkeep, destroy this creature unless you pay {1}"' (CR 113.7, 604.1):
/// the opponent's Tabernacle too. Marit Lage (a token creature) and the Cultivator each trigger at their controller's
/// upkeep (503.1a); unpaid, the Cultivator is destroyed (its Enroot copy then ceases to exist, 723.3c) and Marit Lage,
/// indestructible, isn't (702.12b).
#[test]
fn cr_702_12b_the_tabernacle_marit_lage_survives_the_cultivator_doesnt() {
    let mut g = scen()
        .battlefield(0, &[DEPTHS, "Forest", "Forest", "Forest"])
        .counters(0, DEPTHS, ice(), 1)
        .hand(0, &[CULTIVATOR])
        .battlefield(1, &[TAB])
        .at(Step::Main1, 0)
        .start();
    let d = bf(&g, 0, DEPTHS);
    remove_ice(&mut g, d, 1);
    resolve(&mut g);
    assert_eq!(all_bf(&g, 0, LAGE).len(), 1);
    let mut g2 = g.clone();
    // (The Cultivator, cast the next turn with the Forests untapped.)
    to_main(&mut g2, 1);
    to_main(&mut g2, 0);
    cast_with(&mut g2, 0, CULTIVATOR, &|_| None);
    resolve(&mut g2);
    assert_eq!(enroots(&g2).len(), 1);
    to_upkeep(&mut g2, 0);
    let srcs: Vec<String> = g2
        .stack()
        .iter()
        .filter_map(|s| match s.kind {
            engine::game::StackKind::Ability { ability, source } if ability.def == def(TAB) => Some(g2.name(source.idx).to_string()),
            _ => None,
        })
        .collect();
    let mut sorted = srcs.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["Carnivorous Cultivator // Enroot".to_string(), LAGE.to_string()], "{}", log(&g2));
    // Pay for neither.
    resolve_with(&mut g2, &|g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(all_bf(&g2, 0, LAGE).len(), 1, "indestructible: {}", log(&g2));
    assert!(!on_bf(&g2, 0, CULTIVATOR), "{}", log(&g2));
    assert!(enroots(&g2).is_empty(), "the copy ceased to exist");
}

// ---- Yavimaya x Choke, Dark Depths, Thespian's Stage ----------------------------------------------------------

/// Yavimaya makes each land a Forest in addition to its other types (CR 305.7): no land an Island that wasn't one.
/// Under Choke ("Islands don't untap during their controllers' untap steps", 502.3) the Island - an Island and a Forest
/// - stays tapped; the Forests by Yavimaya (Wasteland, Yavimaya, Dark Depths) untap.
#[test]
fn cr_502_3_choke_and_yavimaya_only_islands_stay_tapped() {
    let mut g = scen().battlefield(0, &[YAVIMAYA, "Wasteland", "Island", DEPTHS]).counters(0, DEPTHS, ice(), 10).battlefield(1, &[CHOKE]).at(Step::Main1, 0).start();
    for &c in g.battlefield().to_vec().iter() {
        if g.is_type(c, engine::db::types::LAND) {
            g.set_tapped(c, true);
        }
    }
    let island = bf(&g, 0, "Island");
    assert!(g.has_subtype(island, subtype("Island")) && g.has_subtype(island, subtype("Forest")));
    for n in [YAVIMAYA, "Wasteland", DEPTHS] {
        assert!(!g.has_subtype(bf(&g, 0, n), subtype("Island")), "{n} no Island");
    }
    to_main(&mut g, 1);
    to_main(&mut g, 0);
    assert!(g.obj(island).tapped, "{}", log(&g));
    for n in [YAVIMAYA, "Wasteland", DEPTHS] {
        assert!(!g.obj(bf(&g, 0, n)).tapped, "{n} untapped");
    }
}

/// Dark Depths under Yavimaya is a Forest with "{T}: Add {G}" (CR 305.6, 305.7: its own abilities kept): it taps for
/// {G}; its state trigger still triggers (603.8) and Marit Lage is created.
#[test]
fn cr_305_7_dark_depths_under_yavimaya_is_a_forest_with_its_abilities() {
    let mut g = scen().battlefield(0, &[YAVIMAYA, DEPTHS, "Island", "Island", "Island"]).counters(0, DEPTHS, ice(), 1).at(Step::Main1, 0).start();
    let d = bf(&g, 0, DEPTHS);
    assert!(g.has_subtype(d, subtype("Forest")));
    float(&mut g, d, None);
    assert_eq!(g.player(0).pool.amt[engine::mana::G], 1, "{}", log(&g));
    remove_ice(&mut g, d, 1);
    resolve(&mut g);
    assert_eq!(all_bf(&g, 0, LAGE).len(), 1, "{}", log(&g));
}

/// A Thespian's Stage that becomes a copy of the opponent's Yavimaya has its grant (CR 707.2, 613.1d): with the
/// original destroyed by Ghost Quarter, every land is still a Forest. A Stage copying Dark Depths meanwhile is a Forest
/// too.
#[test]
fn cr_707_2_a_stage_copy_of_yavimaya_grants_forest_after_the_original_leaves() {
    let mut g = scen()
        .battlefield(0, &[STAGE, GQ, "Island", "Island", "Island"])
        .battlefield(1, &[YAVIMAYA, "Island"])
        .library(1, &["Swamp", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, STAGE);
    let y = bf(&g, 1, YAVIMAYA);
    stage_copy(&mut g, s, y);
    assert_eq!(g.name(s), YAVIMAYA);
    let q = bf(&g, 0, GQ);
    let k = nonmana_slot(&g, q, 0);
    act_with(&mut g, Choice::Ability(q, AbilitySlot::Activated(k)), &|g| pick(g, &[g.tgt(y)]));
    resolve_with(&mut g, &|g| pick(g, &[g.tgt(y)]));
    assert!(!on_bf(&g, 1, YAVIMAYA), "{}", log(&g));
    for (p, n) in [(0, "Island"), (1, "Island"), (0, YAVIMAYA)] {
        assert!(g.has_subtype(bf(&g, p, n), subtype("Forest")), "P{p}'s {n} a Forest: {}", log(&g));
    }
}

// ---- Life from the Loam x Dark Depths, Wasteland, Bojuka Bog ----------------------------------------------------

/// Life from the Loam returns Dark Depths, Wasteland and Bojuka Bog (CR 601.2c, 115.1). Played again, Dark Depths is a
/// new object (400.7) and enters with ten ice counters (614.1c); the Loam, in the graveyard, dredges the next draw
/// (702.52a).
#[test]
fn cr_400_7_loam_returns_dark_depths_wasteland_and_bojuka_bog_and_dredges_again() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .graveyard(0, &[DEPTHS, "Wasteland", BOG])
        .hand(0, &[LOAM])
        .library(0, &["Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let loam = hand(&g, 0, LOAM);
    g.choose(Choice::Cast(loam));
    let mut picked = 0;
    for _ in 0..60 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Target {
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
        let c = pay_with_names(&g, &["Forest"]).unwrap_or(d.options[0].choice);
        g.choose(c);
    }
    resolve(&mut g);
    let mut h: Vec<String> = g.player(0).hand.iter().map(|&c| g.name(c).to_string()).collect();
    h.sort();
    assert_eq!(h, vec![BOG.to_string(), DEPTHS.to_string(), "Wasteland".to_string()], "{}", log(&g));
    let d = hand(&g, 0, DEPTHS);
    act_with(&mut g, Choice::Play(d), &|_| None);
    let d = bf(&g, 0, DEPTHS);
    assert_eq!(g.counters_on(d, ice()), 10);
    assert_eq!(gy(&g, 0), vec![LOAM.to_string()]);
    pass_until(&mut g, |g| dredge_asked(g) || (g.turn().active == 1 && g.turn().step == Step::Draw));
    // (P1's draw step: P1's own draw; the Loam is P0's - P0's next draw step.)
    pass_until(&mut g, |g| dredge_asked(g) || (g.turn().active == 0 && g.turn().step == Step::Main1));
    assert!(dredge_asked(&g), "P0's next draw: {}", log(&g));
    assert_eq!(g.pending_decision().unwrap().player, 0);
}

/// Bojuka Bog exiles the target player's graveyard (CR 406.2, 603.6a): a Life from the Loam there is gone, and the
/// player's next draw is no dredge question (702.52a: dredge works from the graveyard only).
#[test]
fn cr_406_2_bojuka_bog_exiles_a_loam_no_dredge() {
    let mut g = scen()
        .hand(0, &[BOG])
        .graveyard(1, &[LOAM, "Wasteland", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let b = hand(&g, 0, BOG);
    let target = Choice::Target(Target::Player(1));
    g.choose(Choice::Play(b));
    until_priority(&mut g, &|g| pick(g, &[target]));
    resolve_with(&mut g, &|g| pick(g, &[target]));
    assert!(g.player(1).graveyard.is_empty(), "{}", log(&g));
    assert!(g.exile_zone().iter().any(|&c| g.name(c) == LOAM));
    pass_until(&mut g, |g| dredge_asked(g) || (g.turn().active == 1 && g.turn().step == Step::Main1));
    assert!(!dredge_asked(&g), "{}", log(&g));
    assert_eq!(g.player(1).drawn, 1, "P1 drew");
}

// ---- Damping Sphere x Ancient Tomb, Otawara, Yavimaya's Forests ------------------------------------------------

/// Damping Sphere (CR 106.12b): a land tapped for two or more mana produces {C} instead. Under Yavimaya, Ancient Tomb
/// is a Forest whose "{T}: Add {G}" (305.6) makes one mana - unchanged: a {G} spell (Exploration) is castable from it
/// alone; its own {C}{C} is one {C} - a {2} spell isn't.
#[test]
fn cr_106_12b_damping_sphere_leaves_a_yavimaya_forests_one_green() {
    let mut g = scen()
        .battlefield(0, &[SPHERE, YAVIMAYA, "Ancient Tomb"])
        .hand(0, &[EXPLORATION, "Smuggler's Copter"])
        .at(Step::Main1, 0)
        .start();
    let y = bf(&g, 0, YAVIMAYA);
    g.set_tapped(y, true);
    g.refresh_decision();
    let cop = hand(&g, 0, "Smuggler's Copter");
    assert!(!g.has_option(Choice::Cast(cop)), "{{2}} from one damped Ancient Tomb: {}", opts(&g));
    let life = g.life(0);
    cast_with(&mut g, 0, EXPLORATION, &|g| pay_with_names(g, &["Ancient Tomb"]));
    resolve(&mut g);
    assert!(on_bf(&g, 0, EXPLORATION), "{}", log(&g));
    assert_eq!(g.life(0), life, "the Forest's {{G}}, not Ancient Tomb's damaging {{C}}{{C}}");
    // Without the Sphere the Copter is castable from Ancient Tomb's {C}{C}.
    let g = scen().battlefield(0, &[YAVIMAYA, "Ancient Tomb"]).hand(0, &["Smuggler's Copter"]).at(Step::Main1, 0).start();
    let mut g = g;
    let y = bf(&g, 0, YAVIMAYA);
    g.set_tapped(y, true);
    g.refresh_decision();
    assert!(g.has_option(Choice::Cast(hand(&g, 0, "Smuggler's Copter"))), "{}", opts(&g));
}

/// The Sphere's "each spell a player casts costs {1} more for each other spell that player has cast this turn" taxes
/// spells only (CR 601.2f): Otawara's channel (an activated ability from the hand, 602.2b) costs {3}{U} after a spell,
/// paid by Yavimaya's Forests ({G} each) and an Island.
#[test]
fn cr_602_2b_damping_sphere_doesnt_tax_otawaras_channel_paid_by_forests() {
    let mut g = scen()
        .battlefield(0, &[SPHERE, YAVIMAYA, "Wasteland", "Wasteland", "Island", "Island"])
        .battlefield(1, &["Guide of Souls"])
        .hand(0, &["Ponder", "Otawara, Soaring City"])
        .at(Step::Main1, 0)
        .start();
    // (Ponder paid by one Island: the payment's source, nothing floated beside it.)
    let p = hand(&g, 0, "Ponder");
    g.choose(Choice::Cast(p));
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::PayMana) => pay_with_names(g, &["Island"]),
        _ => g.has_option(Choice::Pass).then_some(Choice::Pass),
    });
    resolve(&mut g);
    assert_eq!(g.player(0).spells_cast, 1);
    assert_eq!(g.lands_of(0).filter(|&c| !g.obj(c).tapped).count(), 4, "{}", log(&g));
    let o = hand(&g, 0, "Otawara, Soaring City");
    let ch = abilities_offered(&g, o);
    assert_eq!(ch.len(), 1, "channel with four lands untapped: {}", opts(&g));
    let t = g.tgt(bf(&g, 1, "Guide of Souls"));
    act_with(&mut g, Choice::Ability(o, ch[0]), &|g| pick(g, &[t]));
    resolve(&mut g);
    assert!(g.find_card(1, "Guide of Souls", Zone::Hand).is_some(), "{}", log(&g));
    assert!(g.lands_of(0).all(|c| g.obj(c).tapped), "four lands paid {{3}}{{U}}");
}

// ---- Crop Rotation x Dark Depths, the Tabernacle -------------------------------------------------------------

/// Crop Rotation puts the found land onto the battlefield (CR 701.23, 614.1c): Dark Depths enters with ten ice counters
/// (it enters, unlike a Stage's copy); the Tabernacle enters and the opponent's creature triggers at its controller's
/// next upkeep (503.1a). Neither is a land play (305.4).
#[test]
fn cr_614_1c_crop_rotation_into_dark_depths_and_the_tabernacle() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .library(0, &[DEPTHS, TAB, "Island", "Island"])
        .hand(0, &["Crop Rotation", "Crop Rotation"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .at(Step::Main1, 0)
        .start();
    for find in [DEPTHS, TAB] {
        let cr = hand(&g, 0, "Crop Rotation");
        g.choose(Choice::Cast(cr));
        let forests: Vec<CardIdx> = all_bf(&g, 0, "Forest");
        let sac = *forests.last().unwrap();
        until_priority(&mut g, &|g| match g.kind() {
            Some(DecisionKind::Sacrifice) => pick(g, &[Choice::Card(sac)]),
            _ => pay_with_names(g, &["Forest"]),
        });
        resolve_with(&mut g, &|g| {
            (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| g.offered().into_iter().find(|c| matches!(c, Choice::Def(d) if g.db().def(*d).name == find))).flatten()
        });
        assert!(on_bf(&g, 0, find), "{find}: {}", log(&g));
    }
    assert_eq!(g.counters_on(bf(&g, 0, DEPTHS), ice()), 10);
    assert_eq!(g.player(0).lands_played, 0);
    to_upkeep(&mut g, 1);
    let tab: Vec<&str> = g
        .stack()
        .iter()
        .filter_map(|s| match s.kind {
            engine::game::StackKind::Ability { ability, source } if ability.def == def(TAB) => Some(g.name(source.idx)),
            _ => None,
        })
        .collect();
    assert_eq!(tab, vec!["Thalia, Guardian of Thraben"], "{}", log(&g));
}

// ---- Karakas, Ghost Quarter, Maze of Ith x Marit Lage, the Stage ----------------------------------------------

/// Karakas returns a legendary creature to its owner's hand: the Marit Lage a Stage's copy of Dark Depths made goes to
/// P0's hand and ceases to exist (CR 111.7, 704.5d); the Stage, sacrificed by the trigger, is in the graveyard as
/// Thespian's Stage (the copy effect ends as it leaves, 400.7).
#[test]
fn cr_111_7_karakas_bounces_a_stage_made_marit_lage() {
    let mut g = scen()
        .battlefield(0, &[STAGE, "Island", "Island"])
        .battlefield(1, &[DEPTHS, "Karakas"])
        .counters(1, DEPTHS, ice(), 10)
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, STAGE);
    let t = bf(&g, 1, DEPTHS);
    stage_copy(&mut g, s, t);
    resolve(&mut g);
    let m = all_bf(&g, 0, LAGE);
    assert_eq!(m.len(), 1, "{}", log(&g));
    assert_eq!(gy(&g, 0), vec![STAGE.to_string()]);
    g.pass();
    let k = bf(&g, 1, "Karakas");
    let slot = nonmana_slot(&g, k, 0);
    act_with(&mut g, Choice::Ability(k, AbilitySlot::Activated(slot)), &|g| pick(g, &[g.tgt(m[0])]));
    resolve(&mut g);
    assert!(all_bf(&g, 0, LAGE).is_empty(), "{}", log(&g));
    assert!(g.player(0).hand.iter().all(|&x| g.name(x) != LAGE), "a token in a hand ceases to exist");
}

/// Ghost Quarter destroys a Thespian's Stage that is a copy of Bojuka Bog (a land, CR 701.8); its controller may search
/// for a basic land card (701.23) and does; the card in the graveyard is Thespian's Stage.
#[test]
fn cr_701_23_ghost_quarter_on_a_stage_copy() {
    let mut g = scen()
        .battlefield(1, &[STAGE, "Island", "Island"])
        .battlefield(0, &[BOG, GQ])
        .library(1, &["Forest", "Island", "Island"])
        .at(Step::Main1, 1)
        .start();
    let s = bf(&g, 1, STAGE);
    let t = bf(&g, 0, BOG);
    stage_copy(&mut g, s, t);
    assert_eq!(g.name(s), BOG);
    to_main(&mut g, 0);
    let q = bf(&g, 0, GQ);
    let k = nonmana_slot(&g, q, 0);
    act_with(&mut g, Choice::Ability(q, AbilitySlot::Activated(k)), &|g| pick(g, &[g.tgt(s)]));
    resolve_with(&mut g, &|g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => g.offered().into_iter().find(|c| matches!(c, Choice::Def(d) if g.db().def(*d).name == "Forest")),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(gy(&g, 1), vec![STAGE.to_string()], "{}", log(&g));
    assert!(on_bf(&g, 1, "Forest"), "the searched basic: {}", log(&g));
}

/// Maze of Ith on an attacking Marit Lage (CR 615.1a, 506.4): untapped, still attacking, its combat damage prevented.
#[test]
fn cr_615_1a_maze_of_ith_stops_marit_lage() {
    let mut g = scen()
        .battlefield(1, &[DEPTHS, "Island", "Island", "Island"])
        .counters(1, DEPTHS, ice(), 1)
        .battlefield(0, &[MAZE])
        .at(Step::Main1, 1)
        .start();
    let d = bf(&g, 1, DEPTHS);
    remove_ice(&mut g, d, 1);
    resolve(&mut g);
    let m = all_bf(&g, 1, LAGE)[0];
    to_main(&mut g, 0);
    to_main(&mut g, 1);
    pass_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(m));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    pass_until(&mut g, |g| g.turn().step == Step::DeclareAttackers && at_priority(g) && g.pending_decision().unwrap().player == 0);
    let z = bf(&g, 0, MAZE);
    let k = nonmana_slot(&g, z, 0);
    act_with(&mut g, Choice::Ability(z, AbilitySlot::Activated(k)), &|g| pick(g, &[g.tgt(m)]));
    resolve_top_with(&mut g, &|g| pick(g, &[g.tgt(m)]));
    assert!(!g.obj(m).tapped && g.obj(m).attacking, "{}", log(&g));
    pass_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 20, "{}", log(&g));
}

// ---- the draw instruction in progress: context floats 251-252 ----------------------------------------------------

/// The draws left of a draw instruction under a dredge or miracle question, and whether the draw step's beginning event
/// follows (ENGINE.md 270.4: `ctx::DRAWS_LEFT`, `ctx::DRAW_STEP_FOLLOWS`), are public: both players' inputs, layouts v7
/// and v8. Brainstorm's first draw with a Loam in the graveyard - the dredge question, two left; the draw step's dredge
/// question - none left, the step's event after it; no question - zeros.
#[test]
fn the_draw_instruction_in_progress_reaches_both_players_inputs() {
    use engine::az::encode::{ctx, squash, O_CONTEXT};
    use engine::az::tail_encode::Row;
    let floats = |g: &Game, v: PlayerId, v8: bool| {
        let x = Row::of_layout(g, v, v8).head;
        (x[O_CONTEXT + ctx::DRAWS_LEFT], x[O_CONTEXT + ctx::DRAW_STEP_FOLLOWS])
    };
    let mut g = scen()
        .library(0, &["Island", "Forest", "Island", "Forest", "Island"])
        .graveyard(0, &[LOAM])
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .at(Step::Upkeep, 0)
        .start();
    for v in 0..2u8 {
        assert_eq!(floats(&g, v, false), (0.0, 0.0));
    }
    cast_with(&mut g, 0, "Brainstorm", &|_| None);
    pass_until(&mut g, dredge_asked);
    assert_eq!(g.draw_instruction_rest(), Some((2, false)));
    for v in 0..2u8 {
        for v8 in [false, true] {
            assert_eq!(floats(&g, v, v8), (squash(2.0 / 4.0), 0.0), "P{v} v8 {v8}");
        }
    }
    // The draw step's dredge question.
    let mut g = scen().library(0, &["Island", "Forest", "Island", "Forest"]).graveyard(0, &[LOAM]).at(Step::Upkeep, 0).start();
    pass_until(&mut g, dredge_asked);
    assert_eq!(g.draw_instruction_rest(), Some((0, true)));
    for v in 0..2u8 {
        for v8 in [false, true] {
            assert_eq!(floats(&g, v, v8), (0.0, 1.0), "P{v} v8 {v8}");
        }
    }
}

