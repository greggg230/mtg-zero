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
    assert!(g.has_option(c), "{c:?} offered: {}", opts(g));
    g.choose(c);
    answer_until_priority(g, &answer);
}

/// Resolve the stack (both players passing), `answer` answering.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..800 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g) && g.pending_decision().is_some()) {
            if g.stack().is_empty() && at_priority(g) {
                return;
            }
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
        act_with(g, ability(depths, k), |_| None);
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
