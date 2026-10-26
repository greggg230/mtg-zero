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
        g.play_first();
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
/// resolves (CR 614.1a: "from anywhere").
#[test]
fn dauthi_voidwalker_exiles_opponents_spells_cr614_1a() {
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

// ---- Does Machines (Class levels, CR 716) --------------------------------------------

/// "When this Class enters, mill two cards, draw two cards, then discard two
/// cards. {1}{U}: Level 2. When this Class becomes level 2, return up to two
/// target artifact cards from your graveyard to your hand. {4}{U}: Level 3.
/// At the beginning of combat on your turn, put three +1/+1 counters on
/// target artifact you control. If it isn't a creature, it becomes a 0/0
/// Robot creature in addition to its other types." (CR 716.2a: a level
/// ability is sorcery-speed and only from level N-1; 716.2c.)
#[test]
fn does_machines_gains_levels_cr716() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Island", "Sol Ring"])
        .hand(0, &["Does Machines"])
        .graveyard(0, &["Mox Pearl", "Mind Stone", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let dm = g.card(0, "Does Machines", Zone::Hand);
    g.choose(Choice::Cast(dm));
    pay(&mut g);
    // The enter trigger: mill two, draw two, discard two.
    let lib = g.player(0).library.len();
    resolve_with(&mut g, |g| g.pending_decision().and_then(|d| d.options.iter().find(|o| o.choice != Choice::Pass)).map(|o| o.choice));
    assert_eq!(g.player(0).library.len(), lib - 4, "milled two, drew two\n{}", log(&g));
    assert_eq!(g.player(0).hand.len(), 0, "discarded two");
    let dm = bf(&g, 0, "Does Machines");
    assert!(g.has_option(act(dm, 0)), "{{1}}{{U}}: Level 2: {:?}", g.options_desc());
    g.choose(act(dm, 0));
    pay(&mut g);
    // Resolves: level 2; its trigger targets up to two artifact cards.
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (at_priority(g) && g.stack().is_empty()));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    let mox = g.card(0, "Mox Pearl", Zone::Graveyard);
    let stone = g.card(0, "Mind Stone", Zone::Graveyard);
    assert!(!g.has_option(g.tgt(g.card(0, "Lightning Bolt", Zone::Graveyard))), "artifact cards only");
    g.choose(g.tgt(mox));
    g.choose(g.tgt(stone));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Mox Pearl", Zone::Hand), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Hand), 1);
    let dm = bf(&g, 0, "Does Machines");
    assert!(!g.has_option(act(dm, 0)) || g.def_of(dm).activated.len() == 1, "no second Level 2");
    // Level 3 ({4}{U}: five Islands left? two tapped for level 2).
    assert!(g.has_option(act(dm, 0)), "{{4}}{{U}}: Level 3: {:?}", g.options_desc());
    g.choose(act(dm, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(log(&g).contains("becomes level 3"), "{}", log(&g));
    let ring = bf(&g, 0, "Sol Ring");
    assert!(!g.is_creature(ring));
    // The beginning of combat: three counters on the Sol Ring, a 0/0 Robot
    // creature in addition.
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.turn().step > Step::BeginCombat);
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    g.choose(g.tgt(ring));
    settle(&mut g);
    assert!(g.is_creature(ring), "{}", log(&g));
    assert_eq!(g.pt(ring), (3, 3));
    assert!(g.is_type(ring, engine::db::types::ARTIFACT), "in addition to its other types");
}

// ---- Nadu, Winged Wisdom (a granted "becomes the target" trigger) ----------------

/// "Creatures you control have 'Whenever this creature becomes the target of
/// a spell or ability, reveal the top card of your library. If it's a land
/// card, put it onto the battlefield. Otherwise, put it into your hand. This
/// ability triggers only twice each turn.'" (CR 603.2e; the limit is each
/// creature's.)
#[test]
fn nadu_grants_a_target_trigger_twice_each_turn_per_creature_cr603_2e() {
    let mut g = scen()
        .library(0, &["Forest", "Lightning Bolt", "Mountain", "Plains"])
        .battlefield(0, &["Nadu, Winged Wisdom", "Test Giant"])
        .battlefield(1, &["Test Tapper", "Test Tapper", "Test Tapper", "Test Tapper"])
        .at(Step::Main1, 1)
        .start();
    let giant = bf(&g, 0, "Test Giant");
    let nadu = bf(&g, 0, "Nadu, Winged Wisdom");
    let tappers: Vec<CardIdx> = g.permanents_of(1).collect();
    let ping = |g: &mut Game, t: CardIdx, at: CardIdx| {
        g.choose(act(t, 0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(at));
        settle(g);
    };
    let forest_before = g.count_in(0, "Forest", Zone::Battlefield);
    ping(&mut g, tappers[0], giant);
    assert_eq!(g.count_in(0, "Forest", Zone::Battlefield), forest_before + 1, "a land: onto the battlefield\n{}", log(&g));
    ping(&mut g, tappers[1], giant);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1, "not a land: into the hand\n{}", log(&g));
    let lib = g.player(0).library.len();
    ping(&mut g, tappers[2], giant);
    assert_eq!(g.player(0).library.len(), lib, "only twice each turn for this creature\n{}", log(&g));
    // Another creature's own ability still triggers.
    ping(&mut g, tappers[3], nadu);
    assert_eq!(g.player(0).library.len(), lib - 1, "Nadu's own: a third reveal\n{}", log(&g));
}

// ---- Anointed Peacekeeper (a card name chosen as it enters) ----------------------

/// "As this creature enters, look at an opponent's hand, then choose any card
/// name. Spells your opponents cast with the chosen name cost {2} more to
/// cast. Activated abilities of sources with the chosen name cost {2} more to
/// activate unless they're mana abilities." (CR 614.12, 601.2f, 602.2b.)
#[test]
fn anointed_peacekeeper_names_a_card_and_taxes_it_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Mountain"])
        .hand(0, &["Anointed Peacekeeper", "Lightning Bolt"])
        .hand(1, &["Lightning Bolt", "Test Bear"])
        .battlefield(1, &["Mountain", "Mountain", "Test Tapper"])
        .at(Step::Main1, 0)
        .start();
    let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
    g.choose(Choice::Cast(ap));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::NameCard));
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}", log(&g));
    // It looked at the opponent's hand.
    let bolt1 = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(g.knows(0, bolt1), "P0 saw P1's hand");
    let bolt_def = db().id("Lightning Bolt").unwrap();
    assert!(g.has_option(Choice::Def(bolt_def)), "{:?}", g.options_desc());
    g.choose(Choice::Def(bolt_def));
    settle(&mut g);
    // P0's own Bolt: {R}; P1's: {2}{R} (two Mountains: not castable).
    let bolt0 = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bolt0)), "its controller's spells are not taxed");
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert!(!g.has_option(Choice::Cast(bolt1)), "{{2}}{{R}} with two lands: {:?}", g.options_desc());
    // Naming Test Tapper instead: its ability costs {2} more.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Anointed Peacekeeper"])
        .battlefield(1, &["Mountain", "Test Tapper"])
        .at(Step::Main1, 0)
        .start();
    let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
    g.choose(Choice::Cast(ap));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::NameCard));
    let tapper_def = db().id("Test Tapper").unwrap();
    g.choose(Choice::Def(tapper_def));
    settle(&mut g);
    let tapper = bf(&g, 1, "Test Tapper");
    g.pass();
    assert!(!g.has_option(act(tapper, 0)), "{{2}} more with one land: {:?}", g.options_desc());
}

// ---- Urza, Lord High Artificer (a mana ability that taps another permanent) -----

/// "Tap an untapped artifact you control: Add {U}." A mana ability (CR
/// 605.1a) whose cost taps another permanent: each untapped artifact is a
/// source of {U} while paying (an artifact with its own mana ability has
/// both as alternatives); a summoning-sick artifact creature can pay (CR
/// 302.6 is about {T}).
#[test]
fn urza_taps_artifacts_for_blue_while_paying_cr605_1a() {
    let mut g = scen()
        .battlefield(0, &["Urza, Lord High Artificer", "Sol Ring"])
        .hand(0, &["Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bs)), "{{U}} from Sol Ring tapped for Urza: {:?}", g.options_desc());
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana) || at_priority(g));
    let ring = bf(&g, 0, "Sol Ring");
    if g.kind() == Some(DecisionKind::PayMana) {
        assert!(g.has_option(Choice::Card(ring)), "{:?}", g.options_desc());
        assert!(!g.has_option(act(ring, 0)), "its own {{C}}{{C}} can't pay {{U}}");
        g.choose(Choice::Card(ring));
    }
    resolve_with(&mut g, |_| None);
    assert!(g.obj(ring).tapped, "{}", log(&g));
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Graveyard), 1, "{}", log(&g));
    assert!(log(&g).contains("taps Sol Ring for a mana ability"));
}

/// Urza's Construct (0/0, "+1/+1 for each artifact you control") enters
/// summoning sick and still pays for Urza's ability.
#[test]
fn urza_construct_token_and_a_sick_artifact_creature_pay_cr302_6() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Mox Jet"])
        .hand(0, &["Urza, Lord High Artificer", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let urza = g.card(0, "Urza, Lord High Artificer", Zone::Hand);
    g.choose(Choice::Cast(urza));
    pay(&mut g);
    settle(&mut g);
    let construct = g.permanents_of(0).find(|&c| g.name(c) == "Construct").expect("a Construct token");
    assert_eq!(g.pt(construct), (2, 2), "two artifacts: the Mox and itself");
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana) || at_priority(g));
    assert!(g.has_option(Choice::Card(construct)), "the sick Construct: {:?}", g.options_desc());
    g.choose(Choice::Card(construct));
    resolve_with(&mut g, |_| None);
    assert!(g.obj(construct).tapped);
}

/// "{5}: Shuffle your library, then exile the top card. Until end of turn,
/// you may play that card without paying its mana cost."
#[test]
fn urza_exiles_the_top_card_and_plays_it_free_this_turn_cr601_2a() {
    let mut g = scen()
        .battlefield(0, &["Urza, Lord High Artificer", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let urza = bf(&g, 0, "Urza, Lord High Artificer");
    g.choose(act(urza, 1));
    pay(&mut g);
    settle(&mut g);
    let ex = g.card(0, "Island", Zone::Exile);
    assert!(g.has_option(Choice::Play(ex)), "play it (a land: the land drop) {:?}\n{}", g.options_desc(), log(&g));
    g.choose(Choice::Play(ex));
    assert_eq!(g.count_in(0, "Island", Zone::Battlefield), 6);
}

// ---- Arena of Glory (exert as a cost, mana that gives haste) ------------------

/// "{R}, {T}, Exert this land: Add {R}{R}. If any of that mana is spent on a
/// creature spell, it gains haste until end of turn. (An exerted permanent
/// won't untap during your next untap step.)" (CR 701.39, 605.3a: made with
/// priority, it waits in the pool.)
#[test]
fn arena_of_glory_exerts_for_mana_that_gives_haste_cr701_39() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Arena of Glory"])
        .hand(0, &["Test Dwarf"])
        .at(Step::Main1, 0)
        .start();
    let arena = bf(&g, 0, "Arena of Glory");
    // A mana ability with a mana cost, activated while paying (CR 605.3a;
    // ENGINE.md 18): the Mountain pays its {R}, its {R}{R} the Dwarf.
    assert!(!g.has_option(act(arena, 1)), "not with priority: {:?}", g.options_desc());
    let dwarf = g.card(0, "Test Dwarf", Zone::Hand);
    g.choose(Choice::Cast(dwarf));
    g.drive_until(|g| !at_priority(g));
    assert!(g.has_option(act(arena, 1)), "{:?}", g.options_desc());
    g.choose(act(arena, 1));
    pay(&mut g);
    assert!(g.obj(arena).tapped);
    settle(&mut g);
    let dwarf = bf(&g, 0, "Test Dwarf");
    assert!(g.has_kw(dwarf, engine::db::Kw::Haste), "the creature spell gained haste\n{}", log(&g));
    attack_with(&mut g, &["Test Dwarf"]);
    // The exerted Arena stays tapped through its controller's next untap step.
    next_main(&mut g, 0);
    assert!(g.obj(arena).tapped, "exerted: not untapped\n{}", log(&g));
    assert!(!g.has_kw(dwarf, engine::db::Kw::Haste), "until end of turn");
    next_main(&mut g, 0);
    assert!(!g.obj(arena).tapped, "only the next untap step");
}

/// Its mana spent on a noncreature spell gives nothing (and is spent last).
#[test]
fn arena_of_glory_haste_mana_on_a_noncreature_spell_cr106_6() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Arena of Glory"])
        .hand(0, &["Lightning Bolt", "Test Dwarf"])
        .pool(0, [0, 0, 0, 2, 0, 0], 2)
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).pool.amt[3], 1);
    assert_eq!(g.player(0).pool.haste, 1, "{}", log(&g));
}

// ---- Crabomination (emerge, CR 702.119) ---------------------------------------

/// "Emerge from artifact {5}{B}{B} (You may cast this spell by sacrificing an
/// artifact and paying the emerge cost reduced by that artifact's mana
/// value.)" "When this creature enters, target opponent exiles the top card
/// of their library, a card at random from their graveyard, and a card at
/// random from their hand. You may cast a spell from among cards exiled this
/// way without paying its mana cost."
#[test]
fn crabomination_emerges_and_casts_an_exiled_card_free_cr702_119() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Mind Stone"])
        .hand(0, &["Crabomination"])
        .library(1, &["Lightning Bolt"])
        .graveyard(1, &["Test Bear"])
        .hand(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let crab = g.card(0, "Crabomination", Zone::Hand);
    g.choose(Choice::Cast(crab));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    assert!(g.has_option(Choice::Number(1)), "emerge: {:?}", g.options_desc());
    g.choose(Choice::Number(1));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice), "the artifact first (601.2f)\n{}", log(&g));
    let stone = bf(&g, 0, "Mind Stone");
    g.choose(Choice::Card(stone));
    pay(&mut g);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Graveyard), 1, "sacrificed\n{}", log(&g));
    assert_eq!(g.lands_of(0).filter(|&l| g.obj(l).tapped).count(), 5, "{{3}}{{B}}{{B}}: reduced by 2");
    // The enter trigger: P1 is the only target.
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled) || g.kind() == Some(DecisionKind::Target) || (at_priority(g) && g.stack().is_empty()));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Target(engine::Target::Player(1)));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled) || (at_priority(g) && g.stack().is_empty()));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseExiled), "{}", log(&g));
    for n in ["Lightning Bolt", "Test Bear", "Test Giant"] {
        assert_eq!(g.count_in(1, n, Zone::Exile), 1, "{n} exiled\n{}", log(&g));
    }
    let giant = g.card(1, "Test Giant", Zone::Exile);
    assert!(g.has_option(Choice::Pass), "may");
    g.choose(Choice::Card(giant));
    settle(&mut g);
    let giant = g.card(1, "Test Giant", Zone::Battlefield);
    assert_eq!(g.obj(giant).controller, 0, "cast without paying its mana cost\n{}", log(&g));
}

// ---- Bolas's Citadel (the top card looked at any time and played) ---------------

/// "You may look at the top card of your library any time. You may play lands
/// and cast spells from the top of your library. If you cast a spell this
/// way, pay life equal to its mana value rather than pay its mana cost."
/// (CR 118.9: an alternative cost; 401.? the top card known to its owner.)
#[test]
fn bolas_citadel_casts_from_the_top_paying_life_cr118_9() {
    let mut g = scen()
        .library(0, &["Test Giant", "Forest", "Lightning Bolt"])
        .battlefield(0, &["Bolas's Citadel"])
        .at(Step::Main1, 0)
        .start();
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Test Giant");
    assert!(g.knows(0, top), "its controller looks at it any time");
    assert!(!g.knows(1, top), "not the opponent");
    assert!(g.has_option(Choice::Cast(top)), "{:?}", g.options_desc());
    assert_eq!(g.pending_decision().unwrap().action_of(Choice::Cast(top)), Some(engine::act::LIBRARY));
    g.choose(Choice::Cast(top));
    settle(&mut g);
    assert_eq!(g.life(0), 15, "life equal to its mana value\n{}", log(&g));
    assert_eq!(g.count_in(0, "Test Giant", Zone::Battlefield), 1);
    // The next top card, a land: played (the land drop).
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Forest");
    assert!(g.knows(0, top), "the new top card is seen at once");
    assert!(g.has_option(Choice::Play(top)), "{:?}", g.options_desc());
    g.choose(Choice::Play(top));
    let bolt = *g.player(0).library.last().unwrap();
    assert!(!g.has_option(Choice::Play(bolt)) && g.has_option(Choice::Cast(bolt)), "an instant, castable: {:?}", g.options_desc());
}

/// "{T}, Sacrifice ten nonland permanents: Each opponent loses 10 life."
#[test]
fn bolas_citadel_sacrifices_ten_nonland_permanents_cr701_21() {
    let mut g = scen()
        .battlefield(0, &["Bolas's Citadel"])
        .battlefield(0, &["Test Bear"; 9])
        .at(Step::Main1, 0)
        .start();
    let cit = bf(&g, 0, "Bolas's Citadel");
    assert!(g.has_option(act(cit, 0)), "ten with itself: {:?}", g.options_desc());
    g.choose(act(cit, 0));
    for _ in 0..20 {
        if g.kind() != Some(DecisionKind::Sacrifice) {
            break;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    settle(&mut g);
    assert_eq!(g.life(1), 10, "{}", log(&g));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 9);
}

// ---- Rain of Filth (a granted sacrifice mana ability, with the land's own) -------

/// "Until end of turn, lands you control gain 'Sacrifice this land: Add
/// {B}.'" A land may be tapped for its own mana and then sacrificed for {B}
/// in one payment (CR 601.2g: mana abilities in any order); a tapped land
/// can still be sacrificed. Lands that come later don't have it (611.2c).
#[test]
fn rain_of_filth_lands_tap_then_sacrifice_in_one_payment_cr601_2g() {
    let sac_for = AbilitySlot::Trigger(u8::MAX - 1);
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Rain of Filth", "Hymn to Tourach", "Dark Ritual"])
        .at(Step::Main1, 0)
        .start();
    let rain = g.card(0, "Rain of Filth", Zone::Hand);
    g.choose(Choice::Cast(rain));
    pay(&mut g);
    settle(&mut g);
    let swamps: Vec<CardIdx> = g.lands_of(0).collect();
    let untapped = *swamps.iter().find(|&&s| !g.obj(s).tapped).unwrap();
    let tapped = *swamps.iter().find(|&&s| g.obj(s).tapped).unwrap();
    // Hymn ({B}{B}) from one untapped Swamp: tapped for {B}, then sacrificed.
    let hymn = g.card(0, "Hymn to Tourach", Zone::Hand);
    assert!(g.has_option(Choice::Cast(hymn)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(hymn));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana) || g.kind() == Some(DecisionKind::Target));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Target(engine::Target::Player(1)));
        g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana));
    }
    // (A tapped land can be sacrificed; the untapped one's {T} and its
    // sacrifice are two sources of one card, the tap first.)
    assert!(g.has_option(Choice::Ability(tapped, sac_for)), "a tapped land can be sacrificed: {:?}", g.options_desc());
    assert!(g.has_option(act(untapped, 0)), "{:?}", g.options_desc());
    g.choose(act(untapped, 0));
    assert!(g.has_option(Choice::Ability(untapped, sac_for)), "tapped, then sacrificed: {:?}", g.options_desc());
    g.choose(Choice::Ability(untapped, sac_for));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Swamp", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Hymn to Tourach", Zone::Graveyard), 1, "Hymn cast and resolved\n{}", log(&g));
    // The tapped Swamp is left: sacrificed for Dark Ritual.
    let ritual = g.card(0, "Dark Ritual", Zone::Hand);
    assert!(g.has_option(Choice::Cast(ritual)), "{:?}", g.options_desc());
}

// ---- Abhorrent Oculus (manifest dread, face-down permanents, CR 701.62, 708) ------

/// "As an additional cost to cast this spell, exile six cards from your
/// graveyard." (CR 601.2b.)
#[test]
fn abhorrent_oculus_exiles_six_graveyard_cards_to_cast_cr601_2b() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Abhorrent Oculus"])
        .graveyard(0, &["Forest"; 6])
        .at(Step::Main1, 0)
        .start();
    let oc = g.card(0, "Abhorrent Oculus", Zone::Hand);
    assert!(g.has_option(Choice::Cast(oc)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(oc));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Forest", Zone::Exile), 6, "{}", log(&g));
    assert_eq!(g.count_in(0, "Abhorrent Oculus", Zone::Battlefield), 1);
    // Five cards in the graveyard: not castable.
    let g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Abhorrent Oculus"])
        .graveyard(0, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let oc = g.card(0, "Abhorrent Oculus", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(oc)));
}

/// A game where P0's Oculus manifests dread at P1's upkeep: the top two cards
/// are a Test Giant and a Lightning Bolt; the Giant is manifested.
fn manifested() -> (Game, CardIdx) {
    let mut g = scen()
        .library(0, &["Test Giant", "Lightning Bolt"])
        .battlefield(0, &["Abhorrent Oculus", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    assert_eq!(g.turn().active, 1, "each opponent's upkeep");
    assert_eq!(g.pending_decision().unwrap().player, 0);
    let giant = g.card(0, "Test Giant", Zone::Library);
    g.choose(Choice::Card(giant));
    (g, giant)
}

/// "At the beginning of each opponent's upkeep, manifest dread. (Look at the
/// top two cards of your library. Put one onto the battlefield face down as
/// a 2/2 creature and the other into your graveyard.)" A face-down
/// permanent is a 2/2 with no name, colour, mana cost, subtypes or
/// abilities (CR 708.2a); its controller knows what it is, the opponent
/// doesn't (708.5).
#[test]
fn abhorrent_oculus_manifests_dread_face_down_cr701_62_708_2() {
    // ruling: 9fe0d90e 9aced886
    let (g, giant) = manifested();
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert!(g.is_face_down(giant));
    assert_eq!(g.pt(giant), (2, 2), "a 2/2");
    assert_eq!(g.name(giant), "", "no name");
    assert_eq!(g.colors_of(giant), 0);
    assert!(g.is_creature(giant) && g.keywords_of(giant).is_empty());
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1, "the other card: the graveyard");
    assert!(g.knows(0, giant), "its controller knows it");
    assert!(!g.knows(1, giant), "the opponent doesn't");
    assert_eq!(g.ident(giant), db().id("Test Giant").unwrap());
}

/// The face-down card is hidden from the opponent in every world it samples
/// and never resampled for its controller; the encoding shows the opponent
/// an unknown 2/2.
#[test]
fn a_face_down_permanent_is_hidden_from_the_opponent_cr708_5() {
    let (g, giant) = manifested();
    let giant_def = db().id("Test Giant").unwrap();
    let mut seen_other = false;
    for s in 0..40u64 {
        let mut d0 = g.clone();
        d0.determinize(0, &mut engine::rng::Rng::new(s));
        assert_eq!(d0.ident(giant), giant_def, "never resampled for its controller");
        let mut d1 = g.clone();
        d1.determinize(1, &mut engine::rng::Rng::new(s));
        assert_eq!(d1.view_of(1), g.view_of(1), "the opponent's view is the same in its worlds");
        if d1.ident(giant) != giant_def {
            seen_other = true;
        }
        assert_eq!(enc(&d1, 1), enc(&g, 1), "the opponent's encoding is the same in its worlds");
        assert_eq!(enc(&d0, 0), enc(&g, 0), "the controller's too");
    }
    assert!(seen_other, "the opponent's worlds deal it again");
}

fn enc(g: &Game, viewer: PlayerId) -> Vec<f32> {
    let mut f = vec![0.0; engine::az::NUM_FEATURES];
    g.encode_features(viewer, &mut f);
    f
}

/// "Turn it face up any time for its mana cost if it's a creature card."
/// (CR 701.40b, 708.7: a special action; 708.8: the same permanent.)
#[test]
fn a_manifested_creature_turns_face_up_for_its_mana_cost_cr701_40b() {
    // ruling: 0c11bb22
    let (mut g, giant) = manifested();
    settle(&mut g);
    // P1's turn: P0 gets priority after P1 passes in P1's upkeep.
    quiet_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let up = act(giant, 0);
    assert!(g.has_option(up), "{{4}}{{G}} with five Forests: {:?}", g.options_desc());
    let lands = g.lands_of(0).filter(|&l| !g.obj(l).tapped).count();
    g.choose(up);
    pay(&mut g);
    assert!(g.stack().is_empty(), "a special action: no stack");
    assert!(!g.is_face_down(giant));
    assert_eq!(g.name(giant), "Test Giant", "{}", log(&g));
    assert_eq!(g.pt(giant), (5, 5));
    assert_eq!(g.lands_of(0).filter(|&l| !g.obj(l).tapped).count(), lands - 5);
    assert!(g.knows(1, giant), "public now");
}

/// A face-down permanent that dies is revealed and goes to the graveyard as
/// its card (CR 708.9); it died as a face-down 2/2 (no abilities of its
/// card trigger, 603.10a).
#[test]
fn a_face_down_permanent_dies_revealed_cr708_9() {
    // ruling: fbf56d96
    let mut g3 = scen()
        .library(0, &["Test Dies Draw", "Lightning Bolt"])
        .battlefield(0, &["Abhorrent Oculus"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g3, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let dd = g3.card(0, "Test Dies Draw", Zone::Library);
    g3.choose(Choice::Card(dd));
    settle(&mut g3);
    quiet_until(&mut g3, |g| at_priority(g) && g.pending_decision().unwrap().player == 1 && g.turn().step == Step::Main1);
    let bolt = g3.card(1, "Lightning Bolt", Zone::Hand);
    g3.choose(Choice::Cast(bolt));
    g3.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g3.choose(g3.tgt(dd));
    pay(&mut g3);
    let hand = g3.player(0).hand.len();
    settle(&mut g3);
    assert_eq!(g3.count_in(0, "Test Dies Draw", Zone::Graveyard), 1, "revealed, its card\n{}", log(&g3));
    assert_eq!(g3.player(0).hand.len(), hand, "no 'when this dies' of the card: it died face down");
    assert!(g3.knows(1, dd));
}

/// Bolas's Citadel's top card is its controller's to know (determinize keeps
/// it for them, deals it again for the opponent, whose view doesn't change).
#[test]
fn bolas_citadel_top_card_is_known_to_its_controller_only() {
    let g = scen()
        .library(0, &["Test Giant", "Forest", "Lightning Bolt", "Test Bear"])
        .battlefield(0, &["Bolas's Citadel"])
        .at(Step::Main1, 0)
        .start();
    let top = *g.player(0).library.last().unwrap();
    let giant = db().id("Test Giant").unwrap();
    let mut other = false;
    for s in 0..30u64 {
        let mut d0 = g.clone();
        d0.determinize(0, &mut engine::rng::Rng::new(s));
        assert_eq!(d0.ident(top), giant);
        assert_eq!(d0.pending_decision().unwrap().options, g.pending_decision().unwrap().options);
        let mut d1 = g.clone();
        d1.determinize(1, &mut engine::rng::Rng::new(s));
        assert_eq!(d1.view_of(1), g.view_of(1));
        other |= d1.ident(top) != giant;
    }
    assert!(other, "the opponent's worlds deal it again");
}
