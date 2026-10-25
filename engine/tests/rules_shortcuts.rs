//! M7e part 1: the rules shortcuts the engine documented, removed
//! (docs/shortcuts.md). Each test names the CR rule it checks and failed
//! before its fix.

mod common;

use common::db;
use engine::db::{AbilitySlot, Cond, Counter, Kw};
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
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
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

/// Run the stack down; `answer` answers what is asked (else the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
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

fn stack_target(g: &Game) -> Option<Choice> {
    options(g).into_iter().find(|c| matches!(c, Choice::Target(Target::Stack(_))))
}

fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(c))).then(|| g.tgt(c))
}

/// Answer target decisions with `cards` in order (one per decision).
fn targets_in_order(g: &mut Game, cards: &[CardIdx]) {
    let mut k = 0;
    for _ in 0..200 {
        if at_priority(g) {
            return;
        }
        let c = if g.kind() == Some(DecisionKind::Target) && k < cards.len() {
            k += 1;
            g.tgt(cards[k - 1])
        } else {
            g.pending_decision().unwrap().options[0].choice
        };
        g.choose(c);
    }
    panic!("targets_in_order: no progress");
}

/// Pass through combat to the second main phase, blocking nothing.
fn to_main2(g: &mut Game) {
    for _ in 0..1000 {
        g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g) || g.kind() == Some(DecisionKind::DeclareBlockers));
        if g.kind() == Some(DecisionKind::DeclareBlockers) {
            g.choose(Choice::Pass);
            continue;
        }
        return;
    }
}

fn tokens_named(g: &Game, name: &str) -> usize {
    g.battlefield().iter().filter(|&&c| g.is_token(c) && g.name(c) == name).count()
}

fn monarch(g: &Game, p: PlayerId) -> bool {
    g.holds(&Cond::IsMonarch, p)
}

const TIDEBINDER: &str = "Tishana's Tidebinder";

// ---- the monarch's triggered abilities (CR 724.2) ---------------------------------------------

/// "At the beginning of the monarch's end step, that player draws a card"
/// is a triggered ability: it goes on the stack (controlled by the monarch)
/// and the card is drawn as it resolves.
#[test]
fn cr_724_2_the_monarchs_end_step_draw_uses_the_stack() {
    let mut g = scen().monarch(0).at(Step::Main2, 0).start();
    let hand = g.player(0).hand.len();
    g.pass_until(|g| g.turn().step == Step::End);
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.turn().step, Step::End);
    assert_eq!(g.stack().len(), 1, "the draw trigger is on the stack\n{}", log(&g));
    assert_eq!(g.stack()[0].controller, 0);
    assert_eq!(g.player(0).hand.len(), hand, "not drawn yet");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

/// Tishana's Tidebinder counters the monarch's draw (CR 701.5a: a
/// triggered ability on the stack can be countered).
#[test]
fn cr_724_2_tidebinder_counters_the_monarchs_draw() {
    let mut g = scen().monarch(0).battlefield(1, &["Island"; 3]).hand(1, &[TIDEBINDER]).at(Step::Main2, 0).start();
    let hand = g.player(0).hand.len();
    g.pass_until(|g| g.turn().step == Step::End);
    g.drive_until(|g| at_priority(g));
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, stack_target);
    assert_eq!(g.player(0).hand.len(), hand, "countered: no card\n{}", log(&g));
}

/// "Whenever a creature deals combat damage to the monarch, its controller
/// becomes the monarch": one trigger per creature, controlled by the
/// monarch, on the stack; the monarchy changes as it resolves.
#[test]
fn cr_724_2_combat_damage_to_the_monarch_triggers_per_creature() {
    let mut g = scen().monarch(1).battlefield(0, &["Test Hasty", "Test Bear"]).at(Step::Main1, 0).start();
    let (a, b) = (bf(&g, 0, "Test Hasty"), bf(&g, 0, "Test Bear"));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(a));
    g.choose(Choice::Card(b));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.turn().step == Step::CombatDamage && at_priority(g) || g.kind() == Some(DecisionKind::OrderTriggers));
    assert_eq!(g.waiting_triggers().len() + g.stack().len(), 2, "one per creature\n{}", log(&g));
    g.drive_until(|g| at_priority(g) && !g.stack().is_empty());
    assert!(g.stack().iter().all(|s| s.controller == 1), "controlled by the monarch");
    assert!(monarch(&g, 1) && !monarch(&g, 0), "not yet");
    resolve_with(&mut g, |_| None);
    assert!(monarch(&g, 0), "{}", log(&g));
}

/// Okoye: "Whenever that creature attacks the monarch this turn, it gains
/// double strike and trample until end of turn" is a delayed triggered
/// ability (CR 603.7b-c): on the stack as the creature attacks, so it can
/// be countered (and Okoye then loses all abilities, Tidebinder's text).
#[test]
fn cr_603_7_okoyes_attack_trigger_uses_the_stack_and_can_be_countered() {
    let mut g = scen()
        .monarch(1)
        .battlefield(0, &["Okoye, Mighty and Adored", "Test Hasty"])
        .battlefield(1, &["Island"; 3])
        .hand(1, &[TIDEBINDER])
        .at(Step::Main1, 0)
        .start();
    let hasty = bf(&g, 0, "Test Hasty");
    let okoye = bf(&g, 0, "Okoye, Mighty and Adored");
    g.pass_until(|g| g.turn().step == Step::BeginCombat);
    finish(&mut g, |g| tgt_if(g, hasty));
    resolve_with(&mut g, |g| tgt_if(g, hasty));
    assert_eq!(g.counters_on(hasty, Counter::P1P1), 1);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1, "the attack trigger\n{}", log(&g));
    assert!(!g.has_kw(hasty, Kw::DoubleStrike), "not before it resolves");
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, stack_target);
    assert!(!g.has_kw(hasty, Kw::DoubleStrike) && !g.has_kw(hasty, Kw::Trample), "countered");
    assert!(!g.has_option(Choice::Ability(okoye, AbilitySlot::Activated(0))));
    to_main2(&mut g);
    assert_eq!(g.life(1), 17, "3 damage once");
}

/// Uncountered, the trigger resolves before blockers: double strike.
#[test]
fn cr_603_7_okoyes_attack_trigger_resolves_before_damage() {
    let mut g = scen().monarch(1).battlefield(0, &["Okoye, Mighty and Adored", "Test Hasty"]).at(Step::Main1, 0).start();
    let hasty = bf(&g, 0, "Test Hasty");
    g.pass_until(|g| g.turn().step == Step::BeginCombat);
    finish(&mut g, |g| tgt_if(g, hasty));
    resolve_with(&mut g, |g| tgt_if(g, hasty));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert!(g.has_kw(hasty, Kw::DoubleStrike) && g.has_kw(hasty, Kw::Trample));
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 14, "3 first-strike and 3 regular damage");
    assert!(monarch(&g, 0));
}

// ---- "one or more" triggers: once per event (CR 603.2c) -------------------------------------

/// Laelia: "Whenever one or more cards are put into exile from your library
/// and/or your graveyard": two instructions of one resolution are two events
/// (two counters); one instruction exiling two cards is one.
#[test]
fn cr_603_2c_laelia_triggers_once_per_exile_event() {
    let mut g = scen()
        .battlefield(0, &["Laelia, the Blade Reforged", "Swamp"])
        .hand(0, &["Test Twice Exile"])
        .graveyard(0, &["Lightning Bolt", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let l = bf(&g, 0, "Laelia, the Blade Reforged");
    let (bolt, cs) = (g.card(0, "Lightning Bolt", Zone::Graveyard), g.card(0, "Counterspell", Zone::Graveyard));
    let s = g.card(0, "Test Twice Exile", Zone::Hand);
    g.choose(Choice::Cast(s));
    targets_in_order(&mut g, &[bolt, cs]);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert_eq!(g.obj(cs).zone, Zone::Exile);
    assert_eq!(g.counters_on(l, Counter::P1P1), 2, "two events\n{}", log(&g));
}

/// Wan Shi Tong: "Whenever one or more cards are put into a library from
/// anywhere": two instructions, two triggers (four Spirits).
#[test]
fn cr_603_2c_wan_shi_tong_triggers_once_per_library_event() {
    let mut g = scen()
        .battlefield(0, &["Wan Shi Tong, All-Knowing", "Plains"])
        .battlefield(1, &["Test Bear", "Test Hasty"])
        .hand(0, &["Test Twice Oust"])
        .at(Step::Main1, 0)
        .start();
    let (a, b) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Hasty"));
    let s = g.card(0, "Test Twice Oust", Zone::Hand);
    g.choose(Choice::Cast(s));
    targets_in_order(&mut g, &[a, b]);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(a).zone, Zone::Library);
    assert_eq!(g.obj(b).zone, Zone::Library);
    assert_eq!(tokens_named(&g, "Spirit"), 4, "two events\n{}", log(&g));
}

/// Inti: "Whenever you discard one or more cards": Faithless Looting's
/// "discard two cards" is one event, one trigger.
#[test]
fn cr_603_2c_inti_triggers_once_for_one_discard_instruction() {
    let mut g = scen()
        .battlefield(0, &["Inti, Seneschal of the Sun", "Mountain"])
        .hand(0, &["Faithless Looting", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let fl = g.card(0, "Faithless Looting", Zone::Hand);
    g.choose(Choice::Cast(fl));
    finish(&mut g, |_| None);
    g.pass();
    g.pass();
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1, "one Inti trigger\n{}", log(&g));
}

// ---- entering with counters (CR 614.1c, 122.6) ------------------------------------------------

/// Throne of the Dead Three puts the creature onto the battlefield "with
/// three +1/+1 counters on it": it enters with them (CR 614.1c), so Vaultborn
/// Tyrant's "another creature you control with power 4 or greater enters"
/// sees a 5/4.
#[test]
fn cr_614_1c_a_creature_enters_with_the_counters_an_effect_gives_it() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Test Hasty", "Island", "Island", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &["Vaultborn Tyrant"])
        .initiative(0)
        .dungeon(0, Some(6), 0)
        .at(Step::Upkeep, 0)
        .start();
    g.drive_until(|g| at_priority(g) && g.stack().first().is_some_and(|s| s.controller == 0) && g.dungeon(0).0 == Some(8));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutOntoBattlefield) => options(g).into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Test Hasty")),
        _ => None,
    });
    let h = bf(&g, 0, "Test Hasty");
    assert_eq!(g.counters_on(h, Counter::P1P1), 3);
    assert_eq!(g.life(0), 23, "Vaultborn Tyrant saw a 5/4 enter\n{}", log(&g));
}

// ---- entering as a copy (CR 707.5, 614.12a) ---------------------------------------------------

/// Phyrexian Metamorph put onto the battlefield by Exhume can't copy the
/// creature the other player exhumed: it enters at the same time (the
/// ruling; CR 614.12a: what a replacement effect looks at is checked before
/// the event).
#[test]
fn cr_614_12a_metamorph_cant_copy_a_permanent_entering_at_the_same_time() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Test Bear"])
        .hand(0, &["Exhume"])
        .graveyard(0, &["Phyrexian Metamorph"])
        .graveyard(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let ex = g.card(0, "Exhume", Zone::Hand);
    g.choose(Choice::Cast(ex));
    finish(&mut g, |_| None);
    g.pass();
    g.pass();
    let mut copy_options = Vec::new();
    for _ in 0..50 {
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
        if at_priority(&g) {
            break;
        }
        if g.kind() == Some(DecisionKind::ChoosePermanent) {
            copy_options = options(&g);
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    let giant = bf(&g, 1, "Test Giant");
    let bear = bf(&g, 0, "Test Bear");
    assert!(copy_options.contains(&Choice::Card(bear)), "{copy_options:?}\n{}", log(&g));
    assert!(!copy_options.contains(&Choice::Card(giant)), "entered at the same time");
}

// ---- last known information (CR 608.2h, 113.7a) ---------------------------------------------

/// Pyrogoyf: "that creature deals damage equal to its power": the creature
/// left the battlefield, so the damage comes from it as it last existed
/// there: a red Phyrexian Metamorph copying Pyrogoyf, not the blue card,
/// so a creature with protection from blue is dealt the damage.
#[test]
fn cr_608_2h_pyrogoyfs_creature_deals_damage_as_it_last_existed() {
    let mut g = scen()
        .battlefield(0, &["Pyrogoyf", "Island", "Island", "Island", "Island", "Mountain"])
        .hand(0, &["Phyrexian Metamorph", "Lightning Bolt"])
        .battlefield(1, &["Mother of Runes", "Test Bear"])
        .graveyard(1, &["Island", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let mother = bf(&g, 1, "Mother of Runes");
    let pyro = bf(&g, 0, "Pyrogoyf");
    let mm = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(mm));
    finish(&mut g, |_| None);
    g.pass();
    // P1: Mother of Runes gives the Bear protection from blue.
    g.choose(Choice::Ability(mother, AbilitySlot::Activated(0)));
    finish(&mut g, |g| tgt_if(g, bear));
    g.pass();
    g.pass();
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseColor));
    g.choose(Choice::Number(1));
    g.drive_until(|g| at_priority(g));
    g.pass();
    g.pass();
    // The Metamorph enters as a copy of Pyrogoyf: two triggers ("Pyrogoyf
    // or another Lhurgoyf"), both at the Bear.
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(pyro)),
        Some(DecisionKind::Target) => tgt_if(g, bear),
        _ => None,
    });
    assert_eq!(g.stack().len(), 2, "{}", log(&g));
    // In response, Bolt the copy.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, mm));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(mm).zone, Zone::Graveyard);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "dealt damage by a red source\n{}", log(&g));
}

// ---- saddle and teamwork (CR 702.171a, 702.122) -----------------------------------------------

/// "Crews Vehicles as though its power were 2 greater" counts for crew,
/// not for saddle: a 0-power Pilot alone can crew Smuggler's Copter but
/// can't saddle Caustic Bronco (saddle 1).
#[test]
fn cr_702_171a_saddle_doesnt_count_a_crew_bonus() {
    let g = scen().battlefield(0, &["Caustic Bronco", "Smuggler's Copter", "Test Pilot"]).at(Step::Main1, 0).start();
    let bronco = bf(&g, 0, "Caustic Bronco");
    let copter = bf(&g, 0, "Smuggler's Copter");
    let crew_ok = (0..4).any(|i| g.has_option(Choice::Ability(copter, AbilitySlot::Activated(i))));
    assert!(crew_ok, "crew counts the bonus: {:?}", g.options_desc());
    let saddle_ok = (0..4).any(|i| g.has_option(Choice::Ability(bronco, AbilitySlot::Activated(i))));
    assert!(!saddle_ok, "saddle doesn't: {:?}", g.options_desc());
}

/// Teamwork (We Say Thee Nay!): creatures with mana abilities can be
/// tapped for it (a creature can be tapped for a cost whatever its
/// abilities); the taps are paid first and the mana from the rest.
#[test]
fn cr_601_2h_teamwork_can_tap_creatures_with_mana_abilities() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island", "Llanowar Elves", "Delighted Halfling"])
        .hand(1, &["We Say Thee Nay!"])
        .at(Step::Main1, 0)
        .start();
    let elves = bf(&g, 1, "Llanowar Elves");
    let halfling = bf(&g, 1, "Delighted Halfling");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    g.pass();
    let nay = g.card(1, "We Say Thee Nay!", Zone::Hand);
    g.choose(Choice::Cast(nay));
    g.drive_until(|g| !at_priority(g) || g.kind() == Some(DecisionKind::ChooseCost));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseCost), "{}", log(&g));
    assert!(g.has_option(Choice::Number(1)), "teamwork offered: {:?}", g.options_desc());
    g.choose(Choice::Number(1));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Card(elves)) => Some(Choice::Card(elves)),
        Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Card(halfling)) => Some(Choice::Card(halfling)),
        _ => None,
    });
    assert!(g.obj(elves).tapped && g.obj(halfling).tapped);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "countered unless {{4}}\n{}", log(&g));
}

// ---- special actions (CR 116.2b, 708.7) -------------------------------------------------------

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
    panic!("quiet_until: no progress");
}

/// Turning a face-down permanent face up is a special action, not an
/// ability: a face-down permanent that lost all abilities (Oko's +1) can
/// still be turned face up.
#[test]
fn cr_116_2b_a_face_down_permanent_that_lost_its_abilities_can_turn_face_up() {
    let mut g = scen()
        .library(0, &["Test Giant", "Lightning Bolt"])
        .battlefield(0, &["Abhorrent Oculus", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Oko, Thief of Crowns"])
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let giant = g.card(0, "Test Giant", Zone::Library);
    g.choose(Choice::Card(giant));
    let oko = bf(&g, 1, "Oko, Thief of Crowns");
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g) && g.pending_decision().unwrap().player == 1);
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(1)));
    finish(&mut g, |g| tgt_if(g, giant));
    resolve_with(&mut g, |g| tgt_if(g, giant));
    assert!(g.is_face_down(giant));
    assert_eq!(g.pt(giant), (3, 3), "an Elk\n{}", log(&g));
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 0);
    assert!(g.has_option(Choice::Ability(giant, AbilitySlot::Activated(0))), "{:?}", g.options_desc());
}

// ---- Necromancy's flash permission (CR 702.8a) ------------------------------------------------

/// "You may cast this spell as though it had flash" is a permission, not
/// the flash keyword: Necromancy is cast at instant speed, and has no flash
/// (as a card or as the permanent it becomes).
#[test]
fn cr_702_8a_necromancy_may_be_cast_as_though_it_had_flash_without_having_it() {
    let def = db().def(db().id("Necromancy").unwrap());
    assert!(!def.keywords.has(Kw::Flash), "no flash keyword");
    let mut g = scen()
        .battlefield(0, &["Swamp"; 3])
        .hand(0, &["Necromancy"])
        .graveyard(0, &["Griselbrand"])
        .at(Step::Main1, 1)
        .start();
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let c = g.card(0, "Necromancy", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "cast on the opponent's turn");
    g.choose(Choice::Cast(c));
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    finish(&mut g, |g| tgt_if(g, gris));
    resolve_with(&mut g, |g| tgt_if(g, gris));
    let nec = bf(&g, 0, "Necromancy");
    assert!(!g.has_kw(nec, Kw::Flash));
}

// ---- converge (CR 702.?/106.1b: the colours of mana spent) -------------------------------------

/// Pay with the first source offered until the payment is over.
fn pay_first(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Prismatic Ending with X = 7 paid by seven Plains and an Underground Sea:
/// at most two colours can be spent (white, and blue or black), exactly,
/// however many sources pay (no upper bound with more than seven).
#[test]
fn cr_106_1b_converge_counts_exactly_with_many_sources() {
    let lands = ["Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Underground Sea"];
    let mut g = scen().battlefield(0, &lands).battlefield(1, &["Coalition Relic"]).hand(0, &["Prismatic Ending"]).at(Step::Main1, 0).start();
    let pe = g.card(0, "Prismatic Ending", Zone::Hand);
    let relic = bf(&g, 1, "Coalition Relic");
    g.choose(Choice::Cast(pe));
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(7));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(relic));
    pay_first(&mut g);
    resolve_with(&mut g, |_| None);
    assert!(g.lands_of(0).all(|l| g.obj(l).tapped));
    assert_eq!(g.obj(relic).zone, Zone::Battlefield, "two colours: mana value 3 stays\n{}", log(&g));
}

// ---- Mox Diamond however it would enter (CR 614.1a) -------------------------------------------

/// Tinker puts Mox Diamond onto the battlefield: "If Mox Diamond would
/// enter, you may discard a land card instead. If you do, put Mox Diamond
/// onto the battlefield. If you don't, put it into its owner's graveyard"
/// applies to that too, not only to the cast Mox.
#[test]
fn cr_614_1a_mox_diamond_put_onto_the_battlefield_asks_for_a_land() {
    for discard in [true, false] {
        let mut g = scen()
            .library(0, &["Mox Diamond", "Island", "Island"])
            .battlefield(0, &["Island", "Island", "Island", "Mind Stone"])
            .hand(0, &["Tinker", "Forest"])
            .at(Step::Main1, 0)
            .start();
        let relic = bf(&g, 0, "Mind Stone");
        let forest = g.card(0, "Forest", Zone::Hand);
        let t = g.card(0, "Tinker", Zone::Hand);
        g.choose(Choice::Cast(t));
        finish(&mut g, |g| match g.kind() {
            Some(DecisionKind::Sacrifice) => Some(Choice::Card(relic)),
            _ => None,
        });
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::SearchLibrary) => options(g).into_iter().find(|c| matches!(c, Choice::Def(d) if *d == db().id("Mox Diamond").unwrap())),
            Some(DecisionKind::Discard) => Some(if discard { Choice::Card(forest) } else { Choice::Pass }),
            _ => None,
        });
        let mox = g.find_card(0, "Mox Diamond", Zone::Battlefield);
        if discard {
            assert!(mox.is_some(), "a land discarded: it enters\n{}", log(&g));
            assert_eq!(g.obj(forest).zone, Zone::Graveyard);
        } else {
            assert!(mox.is_none());
            assert_eq!(g.count_in(0, "Mox Diamond", Zone::Graveyard), 1, "no land discarded: the graveyard\n{}", log(&g));
            assert_eq!(g.obj(forest).zone, Zone::Hand);
        }
    }
}

// ---- the order of replacement effects (CR 616.1) ----------------------------------------------

/// Blightsteel Colossus sacrificed while Yawgmoth's Will applies to its
/// owner: "shuffle it into its owner's library instead" and "exile that card
/// instead" both apply; its controller chooses which (CR 616.1, 616.1e).
#[test]
fn cr_616_1_blightsteel_and_yawgmoths_will_the_controller_chooses() {
    for (k, zone) in [(1u8, Zone::Library), (2, Zone::Exile)] {
        let mut g = scen()
            .battlefield(0, &["Blightsteel Colossus", "Goblin Bombardment", "Swamp", "Swamp", "Swamp"])
            .hand(0, &["Yawgmoth's Will"])
            .at(Step::Main1, 0)
            .start();
        let bs = bf(&g, 0, "Blightsteel Colossus");
        let gb = bf(&g, 0, "Goblin Bombardment");
        let w = g.card(0, "Yawgmoth's Will", Zone::Hand);
        g.choose(Choice::Cast(w));
        finish(&mut g, |_| None);
        resolve_with(&mut g, |_| None);
        g.choose(Choice::Ability(gb, AbilitySlot::Activated(0)));
        let mut asked = Vec::new();
        for _ in 0..20 {
            if at_priority(&g) {
                break;
            }
            let c = match g.kind() {
                Some(DecisionKind::Sacrifice) => Choice::Card(bs),
                Some(DecisionKind::ChooseReplacement) => {
                    asked = options(&g);
                    Choice::Number(k)
                }
                Some(DecisionKind::Target) => Choice::Target(Target::Player(1)),
                _ => g.pending_decision().unwrap().options[0].choice,
            };
            g.choose(c);
        }
        assert_eq!(asked, vec![Choice::Number(1), Choice::Number(2)], "{}", log(&g));
        assert_eq!(g.obj(bs).zone, zone);
    }
}

/// A flashback spell leaving the stack while an opponent controls Dauthi
/// Voidwalker: flashback's "exile it instead" and Dauthi's "exile it with a
/// void counter instead" both apply; the spell's controller chooses, and
/// can keep the void counter off it (CR 616.1).
#[test]
fn cr_616_1_flashback_and_dauthi_voidwalker_the_controller_chooses() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .graveyard(0, &["Faithless Looting"])
        .battlefield(1, &["Dauthi Voidwalker"])
        .at(Step::Main1, 0)
        .start();
    let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
    g.choose(Choice::Cast(fl));
    finish(&mut g, |_| None);
    let mut asked = Vec::new();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseReplacement) if g.has_option(Choice::Number(2)) && g.has_option(Choice::Number(3)) => {
            Some(Choice::Number(2))
        }
        _ => None,
    });
    for c in g.exile_zone().iter() {
        if g.name(*c) == "Faithless Looting" {
            asked.push(g.counters_on(*c, Counter::Named(db().subtype_id("counter:void").unwrap())));
        }
    }
    assert_eq!(asked, vec![0], "exiled by flashback, no void counter\n{}", log(&g));
}

// ---- casting routes (CR 601.2a-b, 601.2f-h) ---------------------------------------------------

/// Underworld Breach grants escape ("escape cost equal to its mana cost plus
/// exile three other cards from your graveyard") to Bitter Triumph, which
/// has an additional cost of its own ("discard a card or pay 3 life"): it is
/// cast paying both (CR 601.2b, 601.2f-h).
#[test]
fn cr_601_2b_breach_escape_with_the_cards_own_additional_cost() {
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Swamp", "Swamp"])
        .graveyard(0, &["Bitter Triumph", "Island", "Island", "Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bt = g.card(0, "Bitter Triumph", Zone::Graveyard);
    let bear = bf(&g, 1, "Test Bear");
    assert!(g.has_option(Choice::Cast(bt)), "escape offered: {:?}", g.options_desc());
    g.choose(Choice::Cast(bt));
    // (No card in hand: the 3 life.)
    finish(&mut g, |g| tgt_if(g, bear));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 17, "3 life");
    assert_eq!(g.exile_zone().len(), 3, "three other cards exiled\n{}", log(&g));
}
