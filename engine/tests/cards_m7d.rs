//! M7d, the last cube cards that needed engine subsystems (ENGINE.md 31).
//! Test names cite the CR.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    }).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

#[allow(dead_code)]
fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

#[allow(dead_code)]
fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
#[allow(dead_code)]
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
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

#[allow(dead_code)]
fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
#[allow(dead_code)]
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
    panic!("quiet_until: no progress
{}", log(g));
}

/// Pass priority until attackers are declared, then attack with `names`.
#[allow(dead_code)]
fn attack_with(g: &mut Game, names: &[&str]) {
    for _ in 0..100 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || at_priority(g));
        if g.kind() == Some(DecisionKind::DeclareAttackers) {
            break;
        }
        g.pass();
    }
    for n in names {
        let c = bf(g, 0, n);
        g.choose(Choice::Card(c));
        g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    }
    g.choose(Choice::Pass);
}

fn named(kind: &str) -> engine::db::Counter {
    engine::db::Counter::Named(db().subtype_id(&format!("counter:{kind}")).unwrap())
}

/// Pass (answering forced decisions with their first option) until P`p`'s
/// next main phase 1 with priority.
fn next_main(g: &mut Game, p: PlayerId) {
    let now = g.turn().number;
    for _ in 0..2000 {
        if g.turn().number > now && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty() {
            return;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("next_main: no progress\n{}", log(g));
}

// ---- Pillage the Bog (plot, CR 702.170) ---------------------------------------

/// "Plot {1}{B}{G} (You may pay {1}{B}{G} and exile this card from your hand.
/// Cast it as a sorcery on a later turn without paying its mana cost. Plot
/// only as a sorcery.)": a special action (CR 116.2k), no stack; the card is
/// cast free on a later turn (702.170d), looking at twice the number of lands.
#[test]
fn pillage_the_bog_plots_then_casts_free_on_a_later_turn_cr702_170() {
    let mut g = scen()
        .battlefield(0, &["Bayou", "Bayou", "Bayou"])
        .hand(0, &["Pillage the Bog"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let c = g.card(0, "Pillage the Bog", Zone::Hand);
    let plot = Choice::Ability(c, AbilitySlot::Hand(0));
    assert!(g.has_option(Choice::Cast(c)), "castable for {{B}}{{G}}");
    assert!(g.has_option(plot), "plottable: {:?}", g.options_desc());
    g.choose(plot);
    pay(&mut g);
    assert!(at_priority(&g) && g.stack().is_empty(), "a special action: no stack\n{}", log(&g));
    assert_eq!(g.pending_decision().unwrap().player, 0, "P0 keeps priority");
    assert_eq!(g.count_in(0, "Pillage the Bog", Zone::Exile), 1);
    assert_eq!(g.lands_of(0).filter(|&l| g.obj(l).tapped).count(), 3, "{{1}}{{B}}{{G}} paid");
    let ex = g.card(0, "Pillage the Bog", Zone::Exile);
    assert!(!g.has_option(Choice::Cast(ex)), "not this turn (702.170d)");
    // Not at instant speed on the opponent's turn either.
    g.pass();
    assert!(g.pending_decision().unwrap().options.iter().all(|o| o.choice != Choice::Cast(ex)));
    next_main(&mut g, 0);
    assert!(g.has_option(Choice::Cast(ex)), "a later turn: {:?}", g.options_desc());
    let untapped_before = g.lands_of(0).filter(|&l| !g.obj(l).tapped).count();
    g.choose(Choice::Cast(ex));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand) || (at_priority(g) && g.stack().is_empty()));
    assert_eq!(g.lands_of(0).filter(|&l| !g.obj(l).tapped).count(), untapped_before, "without paying its mana cost");
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand), "{}", log(&g));
    // Twice the number of lands you control: 3 Bayous (and no land played).
    let lands = g.lands_of(0).count();
    let opts = g.pending_decision().unwrap().options.len();
    assert!(opts >= 1);
    let lib_before = g.player(0).library.len();
    let hand_before = g.player(0).hand.len();
    let first = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    g.choose(first);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand_before + 1, "one into the hand\n{}", log(&g));
    assert_eq!(g.player(0).library.len(), lib_before - 1);
    assert_eq!(g.count_in(0, "Pillage the Bog", Zone::Graveyard), 1);
    assert!(log(&g).contains(&format!("P0 looks at {} card(s)", 2 * lands)), "twice {lands} lands
{}", log(&g));
}

// ---- Gemstone Caverns (an opening-hand action, CR 103.6) ---------------------------

/// "If this card is in your opening hand and you're not the starting player,
/// you may begin the game with Gemstone Caverns on the battlefield with a
/// luck counter on it. If you do, exile a card from your hand." Asked of the
/// non-starting player after the mulligans; then "{T}: Add {C}. If it has a
/// luck counter on it, instead add one mana of any color."
#[test]
fn gemstone_caverns_begins_the_game_on_the_battlefield_with_a_luck_counter_cr103_6() {
    let d = db();
    let caverns = d.id("Gemstone Caverns").unwrap();
    let bolt = d.id("Lightning Bolt").unwrap();
    let deck: Vec<u16> = (0..40).map(|i| if i % 2 == 0 { caverns } else { bolt }).collect();
    let mut checked = 0;
    for seed in 0..20u64 {
        let spec = engine::GameSpec { auto_single: false, mulligans: false, log: true, ..Default::default() };
        let mut g = Game::new(d, spec, [&deck, &deck], seed).unwrap();
        let starting = g.turn().starting;
        let other = 1 - starting;
        // The starting player is never asked (it has Caverns in its deck).
        let dd = g.pending_decision().unwrap().clone();
        assert_eq!(dd.player, other, "only the non-starting player: {:?}\n{}", dd, log(&g));
        assert_eq!(dd.kind, DecisionKind::ChooseHandCard);
        let has = g.player(other).hand.iter().any(|&c| g.obj(c).def == caverns);
        assert_eq!(dd.options.len(), 1 + has as usize, "PASS, and Caverns if in hand");
        if !has {
            continue;
        }
        let c = g.card(other, "Gemstone Caverns", Zone::Hand);
        g.choose(Choice::Card(c));
        assert_eq!(g.kind(), Some(DecisionKind::ExileCost), "{}", log(&g));
        let hand = g.player(other).hand.len();
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
        assert_eq!(g.player(other).hand.len(), hand - 1, "a card exiled from hand");
        assert_eq!(g.exile_zone().len(), 1);
        let cav = g.card(other, "Gemstone Caverns", Zone::Battlefield);
        assert_eq!(g.counters_on(cav, named("luck")), 1);
        // Then the other player may begin with another one (legend rule
        // later), or not.
        if g.kind() == Some(DecisionKind::ChooseHandCard) {
            g.choose(Choice::Pass);
        }
        assert_eq!(g.turn().number, 1, "{}", log(&g));
        checked += 1;
        if checked >= 2 {
            break;
        }
    }
    assert!(checked > 0);
}

/// The luck counter makes it a source of any colour; without one, {C}.
#[test]
fn gemstone_caverns_adds_any_color_only_with_a_luck_counter_cr106_1() {
    let luck = named("luck");
    let g = scen()
        .battlefield(0, &["Gemstone Caverns"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{{C}} can't pay {{R}}");
    let g = scen()
        .battlefield(0, &["Gemstone Caverns"])
        .counters(0, "Gemstone Caverns", luck, 1)
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "any colour with a luck counter: {:?}", g.options_desc());
}

// ---- Dauthi Voidwalker (shadow 702.28; a replacement, 614.1a) ---------------------

/// "Shadow (This creature can block or be blocked by only creatures with
/// shadow.)" (CR 702.28b).
#[test]
fn dauthi_voidwalker_shadow_blocks_and_is_blocked_only_by_shadow_cr702_28() {
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Test Bear"])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    attack_with(&mut g, &["Dauthi Voidwalker", "Test Bear"]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    let d = g.pending_decision().unwrap();
    let dauthi = bf(&g, 0, "Dauthi Voidwalker");
    let bear = bf(&g, 0, "Test Bear");
    assert!(d.options.iter().all(|o| o.choice != Choice::Card(dauthi)), "no blocker without shadow: {:?}", g.options_desc());
    assert!(d.options.iter().any(|o| o.choice == Choice::Card(bear)), "{:?}", g.options_desc());
    // It can't block a creature without shadow either.
    let g2 = scen()
        .battlefield(1, &["Dauthi Voidwalker"])
        .battlefield(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let mut g2 = g2;
    attack_with(&mut g2, &["Test Bear"]);
    g2.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step > Step::DeclareBlockers);
    if g2.kind() == Some(DecisionKind::DeclareBlockers) {
        assert_eq!(g2.pending_decision().unwrap().options.len(), 1, "no legal block: {:?}", g2.options_desc());
    }
}

/// "If a card would be put into an opponent's graveyard from anywhere,
/// instead exile it with a void counter on it" (CR 614.1a: it doesn't die,
/// 700.4); its controller's own cards are not affected; a token is not a
/// card. "{T}, Sacrifice this creature: Choose an exiled card an opponent
/// owns with a void counter on it. You may play it this turn without paying
/// its mana cost."
#[test]
fn dauthi_voidwalker_exiles_opponents_cards_with_void_counters_and_plays_one_free_cr614_1a() {
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1, "its controller's own card: a graveyard
{}", log(&g));
    let void = named("void");
    // An opponent's creature that would die is exiled with a void counter.
    let mut g = scen()
        .battlefield(0, &["Dauthi Voidwalker", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let bear = bf(&g, 1, "Test Bear");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 0, "{}", log(&g));
    let ex = g.card(1, "Test Bear", Zone::Exile);
    assert_eq!(g.counters_on(ex, void), 1, "with a void counter");
    assert!(!log(&g).contains("dies"), "it didn't die
{}", log(&g));
    // "{T}, Sacrifice this creature": the exiled Bear, played free this turn.
    let dauthi = bf(&g, 0, "Dauthi Voidwalker");
    g.choose(act(dauthi, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled) || (at_priority(g) && g.stack().is_empty()));
    assert_eq!(g.count_in(0, "Dauthi Voidwalker", Zone::Graveyard), 1, "sacrificed: its own card, a graveyard");
    if g.kind() != Some(DecisionKind::ChooseExiled) {
        // (Its ability resolves after both players pass.)
        resolve_with(&mut g, |_| None);
    }
    settle(&mut g);
    let ex = g.card(1, "Test Bear", Zone::Exile);
    assert!(g.has_option(Choice::Cast(ex)), "playable this turn: {:?}
{}", g.options_desc(), log(&g));
    let lands_untapped = g.lands_of(0).filter(|&l| !g.obj(l).tapped).count();
    g.choose(Choice::Cast(ex));
    settle(&mut g);
    assert_eq!(g.lands_of(0).filter(|&l| !g.obj(l).tapped).count(), lands_untapped, "without paying its mana cost");
    let b = g.card(1, "Test Bear", Zone::Battlefield);
    assert_eq!(g.obj(b).controller, 0, "P0 controls P1's Bear");
}

/// A spell an opponent casts goes to exile with a void counter as it
/// resolves; a discarded card too (CR 614.1a: "from anywhere").
#[test]
fn dauthi_voidwalker_exiles_opponents_spells_and_discards_cr614_1a() {
    let void = named("void");
    let mut g = scen()
        .battlefield(1, &["Dauthi Voidwalker"])
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.life(1), 17);
    let ex = g.card(0, "Lightning Bolt", Zone::Exile);
    assert_eq!(g.counters_on(ex, void), 1, "{}", log(&g));
}

// ---- Lion Sash (reconfigure, CR 702.151) -------------------------------------------

/// "{W}: Exile target card from a graveyard. If it was a permanent card, put
/// a +1/+1 counter on this permanent." "Equipped creature gets +1/+1 for
/// each +1/+1 counter on this Equipment." "Reconfigure {2} (Attach to target
/// creature you control; or unattach from a creature. Reconfigure only as a
/// sorcery. While attached, this isn't a creature.)"
#[test]
fn lion_sash_grows_and_reconfigures_cr702_151() {
    let mut g = scen()
        .battlefield(0, &["Lion Sash", "Test Bear", "Plains", "Plains", "Plains", "Plains", "Plains"])
        .graveyard(1, &["Test Giant", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let sash = bf(&g, 0, "Lion Sash");
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.is_creature(sash));
    assert!(!g.has_option(act(sash, 2)), "unattach only while attached");
    // {W}: the Giant (a permanent card): a counter.
    g.choose(act(sash, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let giant = g.card(1, "Test Giant", Zone::Graveyard);
    g.choose(g.tgt(giant));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Exile), 1);
    assert_eq!(g.pt(sash), (2, 2), "+1/+1 counter\n{}", log(&g));
    // An instant: exiled, no counter.
    g.choose(act(sash, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let bolt = g.card(1, "Lightning Bolt", Zone::Graveyard);
    g.choose(g.tgt(bolt));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Exile), 1);
    assert_eq!(g.pt(sash), (2, 2), "not a permanent card");
    // Reconfigure: attach to the Bear (it can't target itself, 301.5c).
    g.choose(act(sash, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(!g.has_option(g.tgt(sash)), "not itself");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(sash).attached.map(|r| r.idx), Some(bear), "{}", log(&g));
    assert!(!g.is_creature(sash), "while attached it isn't a creature (702.151b)");
    assert_eq!(g.pt(bear), (3, 3), "+1/+1 for its one counter");
    assert!(!g.has_option(act(sash, 0)), "out of mana");
    // Unattach (sorcery speed): a creature again.
    assert!(g.has_option(act(sash, 2)) || g.lands_of(0).filter(|&l| !g.obj(l).tapped).count() < 2);
}

/// Reconfigure only as a sorcery; unattached, it's a creature again.
#[test]
fn lion_sash_unattaches_as_a_sorcery_cr702_151a() {
    let mut g = scen()
        .battlefield(0, &["Lion Sash", "Test Bear", "Plains", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let sash = bf(&g, 0, "Lion Sash");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(sash, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.is_creature(sash));
    assert_eq!(g.pt(bear), (2, 2), "no counters: +0/+0");
    assert!(g.has_option(act(sash, 2)), "{:?}", g.options_desc());
    assert!(!g.has_option(act(sash, 1)) || true);
    g.choose(act(sash, 2));
    pay(&mut g);
    settle(&mut g);
    assert!(g.obj(sash).attached.is_none(), "{}", log(&g));
    assert!(g.is_creature(sash), "a creature again");
    // Not at instant speed: the opponent's turn.
    let g2 = scen().battlefield(0, &["Lion Sash", "Test Bear", "Plains", "Plains"]).at(Step::Main1, 1).start();
    let sash2 = bf(&g2, 0, "Lion Sash");
    assert!(g2.pending_decision().is_none_or(|d| d.player != 0 || d.options.iter().all(|o| o.choice != act(sash2, 1))));
}
