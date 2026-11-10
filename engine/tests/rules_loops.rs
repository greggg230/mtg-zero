//! Loops of optional actions (CR 732.2a; MTR 4.4; `engine/src/loops.rs`): a
//! player may repeat an optional loop any number of times, and a loop action
//! (a non-mana ability of a permanent, YES to "may copy this spell") is not
//! offered again at a state identical to one where its player took it (as
//! that player sees it): an iteration that made no progress ends the loop.
//! The engine's former caps (31 activations of a permanent's ability, 31
//! "may copy" copies a turn) are gone.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::game::EndReason;
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Answer the payment until priority: `src`'s mana ability when offered, else
/// the first option that isn't one of `avoid`'s.
fn pay_with(g: &mut Game, src: CardIdx, avoid: &[CardIdx]) {
    for _ in 0..50 {
        if at_priority(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        assert!(
            matches!(d.kind, DecisionKind::PayMana | DecisionKind::PayPool | DecisionKind::ChooseMana),
            "{:?} {:?}\n{}",
            d.kind,
            g.options_desc(),
            log(g)
        );
        let opts: Vec<Choice> = d.options.iter().map(|o| o.choice).collect();
        let c = opts
            .iter()
            .copied()
            .find(|c| matches!(c, Choice::Ability(x, _) if *x == src))
            .or_else(|| opts.iter().copied().find(|c| !matches!(c, Choice::Ability(x, _) if avoid.contains(x))))
            .unwrap_or_else(|| panic!("no payment option without {avoid:?}: {:?}", g.options_desc()));
        g.choose(c);
    }
    panic!("pay_with: no priority\n{}", log(g));
}

/// Pass priority until the stack is empty (the first option otherwise).
fn settle(g: &mut Game) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("settle: no progress\n{}", log(g));
}

/// Float `src`'s mana with priority (CR 605.3a, 117.1d: `YES`, then the
/// source, then no more).
fn float(g: &mut Game, src: CardIdx) {
    g.choose(Choice::Yes);
    assert_eq!(g.kind(), Some(DecisionKind::ActivateMana), "{:?}", g.options_desc());
    g.choose(Choice::Card(src));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ActivateMana) || at_priority(g));
    if g.kind() == Some(DecisionKind::ActivateMana) {
        g.choose(Choice::Pass);
    }
    assert!(at_priority(g), "{:?}\n{}", g.options_desc(), log(g));
}

/// Basalt Monolith ("{T}: Add {C}{C}{C}"; "{3}: Untap this artifact"): tap
/// it for {C}{C}{C}, untap it with that mana, and the game is where it was.
/// Tapped again, the untap is not offered: that state is the one where it
/// was taken, and it made no progress. (The former cap offered it 31 times.)
/// An unused land in play changes nothing: the state is the same.
#[test]
fn cr_732_2a_basalt_monolith_loops_once_back_at_the_same_state() {
    for land in [false, true] {
        let mut s = scen().battlefield(0, &["Basalt Monolith"]).hand(0, &["Island"]).hand(1, &["Forest"]);
        if land {
            s = s.battlefield(0, &["Swamp"]);
        }
        let mut g = s.at(Step::Main1, 0).start();
        let m = bf(&g, 0, "Basalt Monolith");
        let avoid: Vec<CardIdx> = g.find_card(0, "Swamp", Zone::Battlefield).into_iter().collect();
        float(&mut g, m);
        assert!(g.obj(m).tapped && g.player(0).pool.amt.iter().sum::<u8>() == 3);
        assert!(g.has_option(act(m, 1)), "the untap: {:?}", g.options_desc());
        g.choose(act(m, 1));
        pay_with(&mut g, m, &avoid);
        settle(&mut g);
        assert!(!g.obj(m).tapped && g.player(0).pool.amt.iter().sum::<u8>() == 0, "{}", log(&g));
        // Back where it started (the untap was never taken here: offered).
        assert!(g.has_option(act(m, 1)));
        float(&mut g, m);
        assert!(!g.has_option(act(m, 1)), "the same state as the untap's: {:?}\n{}", g.options_desc(), log(&g));
        if land {
            assert!(!g.obj(avoid[0]).tapped);
        }
    }
}

/// The same loop with the untap paid by tapping the Monolith during its own
/// payment (CR 601.2g): the untap is taken at the starting state, and
/// resolved, it comes back to it.
#[test]
fn cr_732_2a_basalt_monolith_untap_paid_by_itself_is_not_offered_again() {
    let mut g = scen().battlefield(0, &["Basalt Monolith"]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Basalt Monolith");
    g.choose(act(m, 1));
    pay_with(&mut g, m, &[]);
    settle(&mut g);
    assert!(!g.obj(m).tapped);
    assert!(at_priority(&g) && g.stack().is_empty());
    assert!(!g.has_option(act(m, 1)), "{:?}\n{}", g.options_desc(), log(&g));
    // The game goes on: the next turn offers it again.
    g.pass();
    g.drive_until(|g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    g.drive_until(|g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(m, 1)), "{:?}", g.options_desc());
}

/// Grim Monolith ("{T}: Add {C}{C}{C}"; "{4}: Untap this artifact"): each
/// untap spends a land's mana besides the Monolith's, so every iteration
/// makes progress and the loop goes on while lands last: 40 untaps in one
/// turn, past the former cap of 31.
#[test]
fn cr_732_2a_grim_monolith_untaps_past_31_while_each_iteration_spends_a_land() {
    let mut g = scen().battlefield(0, &["Grim Monolith"]).battlefield(0, &["Swamp"; 40]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Grim Monolith");
    let mut n = 0;
    while g.has_option(act(m, 1)) {
        g.choose(act(m, 1));
        pay_with(&mut g, m, &[]);
        settle(&mut g);
        n += 1;
        assert!(!g.obj(m).tapped, "{}", log(&g));
        assert!(n <= 40, "{}", log(&g));
    }
    assert_eq!(n, 40, "one untap per land: {}", log(&g));
    let tapped = g.battlefield().iter().filter(|&&c| g.name(c) == "Swamp" && g.obj(c).tapped).count();
    assert_eq!(tapped, 40);
}

/// A scenario on the Edge of Eternities table.
fn eoe_scen() -> Scenario {
    Scenario::new(engine::db::CardDb::eoe())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
}

/// Answer an activation's questions until priority: X = 0, else the first
/// option.
fn answer_x_0(g: &mut Game) {
    for _ in 0..20 {
        if at_priority(g) {
            return;
        }
        let c = if g.has_option(Choice::Number(0)) { Choice::Number(0) } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("no priority
{}", log(g));
}

/// Lavaclaw Reaches, animated ("{X}: This creature gets +X/+0 until end of
/// turn"): with X = 0 the activation is free and its effect, +0/+0, changes
/// nothing (CR 613.4c), so once it resolves the game is where it was and the
/// ability is not offered again (the effect is left out of the loop hash,
/// `loops::inert`; the fuzz's Edge of Eternities seed 1004). Holding
/// priority, another X = 0 activation isn't offered either: the board is what
/// it was when the first was taken (its stack object is the loop's own, MTR
/// 4.4: no progress of the board state).
#[test]
fn cr_732_2a_lavaclaw_reaches_x_0_pump_is_no_progress() {
    let mut g = eoe_scen().battlefield(0, &["Lavaclaw Reaches", "Swamp", "Mountain", "Island"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Lavaclaw Reaches");
    let n = g.def_of(l).activated.len() as u8;
    assert!(!g.has_option(act(l, n - 1)), "not animated yet");
    let animate = (0..n).find(|&i| g.def_of(l).activated[i as usize].cost.mana.total() == 3).expect("{{1}}{{B}}{{R}}");
    g.choose(act(l, animate));
    let swamp = bf(&g, 0, "Swamp");
    pay_with(&mut g, swamp, &[l]);
    settle(&mut g);
    assert!(g.has_option(act(l, n - 1)), "animated: {:?}
{}", g.options_desc(), log(&g));
    g.choose(act(l, n - 1));
    answer_x_0(&mut g);
    assert!(!g.has_option(act(l, n - 1)), "holding priority, the board is unchanged: {:?}
{}", g.options_desc(), log(&g));
    settle(&mut g);
    assert!(!g.has_option(act(l, n - 1)), "X = 0 changed nothing: {:?}
{}", g.options_desc(), log(&g));
}

/// The same ability paid for: X = 1 taps a land, so holding priority the
/// board has changed and another activation is offered (a loop that spends
/// resources progresses, MTR 4.4).
#[test]
fn cr_732_2a_lavaclaw_reaches_x_1_pumps_stack_while_lands_pay() {
    let mut g = eoe_scen().battlefield(0, &["Lavaclaw Reaches", "Swamp", "Mountain", "Island", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Lavaclaw Reaches");
    let n = g.def_of(l).activated.len() as u8;
    let animate = (0..n).find(|&i| g.def_of(l).activated[i as usize].cost.mana.total() == 3).expect("{{1}}{{B}}{{R}}");
    g.choose(act(l, animate));
    let swamp = bf(&g, 0, "Swamp");
    pay_with(&mut g, swamp, &[l]);
    settle(&mut g);
    let untapped = |g: &Game| g.permanents_of(0).filter(|&c| !g.obj(c).tapped).count();
    let before = untapped(&g);
    assert!(before >= 3, "the Reaches and two Mountains untapped: {before}
{}", log(&g));
    for k in 0..2 {
        assert!(g.has_option(act(l, n - 1)), "pump {k}: {:?}
{}", g.options_desc(), log(&g));
        g.choose(act(l, n - 1));
        for _ in 0..20 {
            if at_priority(&g) {
                break;
            }
            let c = if g.has_option(Choice::Number(1)) { Choice::Number(1) } else { g.pending_decision().unwrap().options[0].choice };
            g.choose(c);
        }
    }
    assert_eq!(untapped(&g), before - 2, "each X = 1 tapped a land (the Reaches itself or a Mountain)
{}", log(&g));
    settle(&mut g);
    assert_eq!(g.pt(l).0, 4, "2 + 1 + 1
{}", log(&g));
}

/// The EoE fuzz's seeds 1016 and 1024: Wandering Fumarole's "{0}: Switch" and
/// Lavaclaw Reaches' X = 0 pump activated while holding priority in endless
/// new orders, each a longer stack. By the board rule (the user's decision:
/// tournament rules require a player looping without progressing the board
/// state to choose a different action, MTR 4.4), each is offered once a
/// board; then the player must pass, and random play from there ends the turn.
#[test]
fn mtr_4_4_free_activations_stacked_without_board_progress_stop() {
    let mut g = eoe_scen()
        .battlefield(0, &["Lavaclaw Reaches", "Wandering Fumarole", "Swamp", "Mountain", "Island", "Island", "Island", "Mountain", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let (l, w) = (bf(&g, 0, "Lavaclaw Reaches"), bf(&g, 0, "Wandering Fumarole"));
    let (nl, nw) = (g.def_of(l).activated.len() as u8, g.def_of(w).activated.len() as u8);
    let al = (0..nl).find(|&i| g.def_of(l).activated[i as usize].cost.mana.total() == 3).expect("{{1}}{{B}}{{R}}");
    let aw = (0..nw).find(|&i| g.def_of(w).activated[i as usize].cost.mana.total() == 4).expect("{{2}}{{U}}{{R}}");
    g.choose(act(l, al));
    let swamp = bf(&g, 0, "Swamp");
    pay_with(&mut g, swamp, &[l, w]);
    settle(&mut g);
    g.choose(act(w, aw));
    let island = bf(&g, 0, "Island");
    pay_with(&mut g, island, &[l, w]);
    settle(&mut g);
    assert!(g.has_option(act(w, nw - 1)) && g.has_option(act(l, nl - 1)), "both animated: {:?}
{}", g.options_desc(), log(&g));
    g.choose(act(w, nw - 1));
    answer_x_0(&mut g);
    assert!(!g.has_option(act(w, nw - 1)), "the switch again, same board: {:?}", g.options_desc());
    assert!(g.has_option(act(l, nl - 1)), "the pump not taken yet: {:?}", g.options_desc());
    g.choose(act(l, nl - 1));
    answer_x_0(&mut g);
    assert!(!g.has_option(act(w, nw - 1)) && !g.has_option(act(l, nl - 1)), "neither again: {:?}", g.options_desc());
    // Random play (X always 0, so nothing is paid) ends the turn.
    let turn = g.turn().number;
    let mut rng = Rng::new(1016);
    let mut steps = 0;
    while g.turn().number == turn && !g.is_over() {
        let d = g.pending_decision().unwrap();
        let c = if g.has_option(Choice::Number(0)) { Choice::Number(0) } else { d.options[rng.below(d.options.len() as u32) as usize].choice };
        g.choose(c);
        steps += 1;
        assert!(steps < 2_000, "the turn goes on: {steps} decisions
{}", g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
"));
    }
}

/// Wandering Fumarole, animated ("{0}: Switch this creature's power and
/// toughness until end of turn"): two switches are no switch (layer 7d holds
/// only switches and each toggles, CR 613.4d), so after two the game is back
/// where it was and the free ability is not offered; after one it is a
/// different state, and it is.
#[test]
fn cr_732_2a_wandering_fumarole_two_switches_are_no_progress() {
    let mut g = eoe_scen().battlefield(0, &["Wandering Fumarole", "Island", "Island", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, "Wandering Fumarole");
    let n = g.def_of(w).activated.len() as u8;
    let animate = (0..n).find(|&i| g.def_of(w).activated[i as usize].cost.mana.total() == 4).expect("{{2}}{{U}}{{R}}");
    g.choose(act(w, animate));
    let island = bf(&g, 0, "Island");
    pay_with(&mut g, island, &[w]);
    settle(&mut g);
    assert_eq!(g.pt(w), (1, 4));
    g.choose(act(w, n - 1));
    answer_x_0(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(w), (4, 1));
    assert!(g.has_option(act(w, n - 1)), "one switch is a new state: {:?}
{}", g.options_desc(), log(&g));
    g.choose(act(w, n - 1));
    answer_x_0(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(w), (1, 4));
    assert!(!g.has_option(act(w, n - 1)), "two switches are no switch: {:?}
{}", g.options_desc(), log(&g));
}

/// Time Vault ("{T}: Take an extra turn after this one") and Voltaic Key
/// ("{1}, {T}: Untap target artifact"): each turn the Vault (tapped for an
/// extra turn, so it doesn't untap) is untapped by the Key and tapped for
/// another extra turn (each iteration adds a turn: progress). Played on, the
/// game ends at the turn limit, a draw, never stuck.
#[test]
fn cr_732_2a_time_vault_and_voltaic_key_take_turns_until_the_turn_limit() {
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.max_turns = 14;
        })
        .library(0, &["Island"; 40])
        .library(1, &["Forest"; 40])
        .battlefield(0, &["Time Vault", "Voltaic Key", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let (v, k) = (bf(&g, 0, "Time Vault"), bf(&g, 0, "Voltaic Key"));
    let mut vault_taps = 0;
    for _ in 0..20_000 {
        if g.is_over() {
            break;
        }
        let d = g.pending_decision().unwrap();
        let main = g.turn().active == 0 && g.turn().step == Step::Main1 && g.stack().is_empty();
        let c = match d.kind {
            DecisionKind::Priority if main && d.player == 0 && g.has_option(act(v, 0)) => {
                vault_taps += 1;
                act(v, 0)
            }
            DecisionKind::Priority if main && d.player == 0 && g.obj(v).tapped && g.has_option(act(k, 0)) => act(k, 0),
            DecisionKind::Target => g.tgt(v),
            // Not skipping a turn to untap the Vault (the Key does).
            DecisionKind::YesNo => Choice::Pass,
            DecisionKind::PayMana | DecisionKind::PayPool | DecisionKind::ChooseMana => d.options[0].choice,
            _ if g.has_option(Choice::Pass) => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    let o = g.outcome().unwrap_or_else(|| panic!("the game is stuck\n{}", log(&g)));
    assert_eq!((o.reason, o.winner), (EndReason::TurnLimit, None));
    assert!(vault_taps >= 10, "the Vault tapped in each of P0's turns: {vault_taps}");
    assert!(g.player(0).turns > g.player(1).turns + 5, "P0 took the extra turns");
}

/// Chain of Smog ("Target player discards two cards. That player may copy
/// this spell and may choose a new target for that copy"): the copies keep
/// coming past 31, each making a player discard two (progress), until both
/// hands are empty; then a copy comes back to a state where its player
/// copied, the "may" isn't asked, and the game goes on.
#[test]
fn cr_732_2a_chain_of_smog_copies_past_31_until_the_hands_are_empty() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Chain of Smog", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(1, &["Forest"; 70])
        .at(Step::Main1, 0)
        .start();
    let cs = g.card(0, "Chain of Smog", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let mut asked = 0;
    for _ in 0..5_000 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        // The player with cards left is the target (P1 first).
        let aim = if g.player(1).hand.is_empty() { 0 } else { 1 };
        let c = match d.kind {
            DecisionKind::Priority => Choice::Pass,
            DecisionKind::Target => player(1),
            DecisionKind::YesNo => {
                asked += 1;
                Choice::Yes
            }
            DecisionKind::NewTarget if g.has_option(player(aim)) => player(aim),
            DecisionKind::NewTarget => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert!(g.stack().is_empty() && at_priority(&g), "{}", log(&g));
    assert!(g.player(0).hand.is_empty() && g.player(1).hand.is_empty(), "{}", log(&g));
    // 35 copies at P1's 70 cards, 3 at P0's 6, the last ones at empty hands.
    assert!(asked > 31 && asked < 45, "asked {asked} times\n{}", log(&g));
    assert!(g.outcome().is_none());
    assert_eq!(g.obj(cs).zone, Zone::Graveyard);
}

/// The loop rule reads only what the decider sees (`Game::view_of`): after a
/// pruned revisit, every world `determinize` deals for the decider offers
/// the same options (the untap still not offered) and the same view. In the
/// opponent's worlds the decider's records are dropped (hashes of views
/// with what the opponent doesn't know): the untap is offered there.
#[test]
fn cr_732_2a_the_loop_rule_is_the_same_in_every_determinized_world() {
    let mut g = scen()
        .library(0, &["Island", "Swamp", "Forest", "Mountain", "Plains", "Lightning Bolt", "Brainstorm", "Island"])
        .library(1, &["Forest", "Mountain", "Lightning Bolt", "Plains", "Swamp", "Brainstorm", "Island", "Forest"])
        .battlefield(0, &["Basalt Monolith"])
        .hand(0, &["Lightning Bolt", "Island", "Brainstorm"])
        .hand(1, &["Brainstorm", "Mountain", "Lightning Bolt", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let m = bf(&g, 0, "Basalt Monolith");
    g.choose(act(m, 1));
    pay_with(&mut g, m, &[]);
    settle(&mut g);
    assert!(at_priority(&g) && !g.has_option(act(m, 1)), "{:?}", g.options_desc());
    let (mut legal, mut wl) = (Vec::new(), Vec::new());
    g.legal_actions(&mut legal);
    for seed in 0..40u64 {
        let mut w = g.clone();
        w.determinize(0, &mut Rng::new(seed));
        w.legal_actions(&mut wl);
        assert_eq!(wl, legal, "world {seed} for the decider");
        assert_eq!(w.pending_decision(), g.pending_decision());
        assert_eq!(w.view_of(0), g.view_of(0));
        assert!(!w.has_option(act(m, 1)));
        let mut o = g.clone();
        o.determinize(1, &mut Rng::new(seed));
        assert!(o.has_option(act(m, 1)), "world {seed} for the opponent: {:?}", o.options_desc());
    }
}
