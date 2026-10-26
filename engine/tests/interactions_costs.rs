//! M8d part 2 (ENGINE.md 38, `docs/interactions.md`): costs (CR 601.2f-h,
//! 118.9; M7a, M7e) against the ways to cast without the mana cost written
//! in other milestones: Bolas's Citadel's life (M7c) with additional mana
//! costs (spree), taxes on free casts (Etali, Dauthi Voidwalker) and on
//! Citadel's casts.

mod common;
mod rulings_util;

use engine::{CardIdx, Choice, DecisionKind, Game, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn top(g: &Game, p: u8) -> CardIdx {
    *g.player(p).library.last().unwrap()
}

#[test]
fn citadel_spree_mode_costs_keep_the_life_the_citadel_takes_cr_601_2h_702_172a() {
    // Three Steps Ahead on top of P0's library, cast with Bolas's Citadel
    // (1 life, its mana value); its "+{3}: create a token copy of target
    // artifact or creature you control" needs Mana Confluence ({T}, pay 1
    // life) and two Islands. At 1 life, 1 for the Citadel and 1 for the
    // Confluence can't both be paid (the Confluence's life is paid as its
    // mana ability is activated, CR 601.2g, the Citadel's after, 601.2h):
    // the mode isn't offered. ("+{2}: draw two, then discard" is.) At 3
    // life it is, and the cast completes. (Found by FUZZ_POOL=interactions,
    // seed 40409: the mode was offered and the payment panicked.)
    for (life, offered) in [(1, false), (3, true)] {
        let mut g = scen()
            .library(0, &["Three Steps Ahead", "Island", "Island", "Island"])
            .battlefield(0, &["Bolas's Citadel", "Mana Confluence", "Island", "Island", "Test Bear"])
            .life(0, life)
            .start();
        let tsa = top(&g, 0);
        assert!(g.has_option(Choice::Cast(tsa)), "{}", opts(&g));
        g.choose(Choice::Cast(tsa));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
        assert!(g.has_option(Choice::Number(2)), "{}", opts(&g));
        assert_eq!(g.has_option(Choice::Number(1)), offered, "at {life} life: {}", opts(&g));
        if !offered {
            continue;
        }
        g.choose(Choice::Number(1));
        let bear = bf(&g, 0, "Test Bear");
        let t = g.tgt(bear);
        quiet_until_with(&mut g, |g| at_priority(g) || g.is_over(), |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => Some(Choice::Pass),
            Some(DecisionKind::Target) => Some(t),
            _ => None,
        });
        assert_eq!(g.life(0), 1, "1 for the Confluence, 1 for the Citadel\n{}", log(&g));
        resolve(&mut g);
        assert_eq!(tokens_named(&g, "Test Bear").len(), 1, "{}", log(&g));
    }
}

#[test]
fn etali_free_casts_pay_thalias_tax_cr_601_2f_118_9d() {
    // Etali exiles P1's Lightning Bolt; with P1's Thalia, Guardian of
    // Thraben ("Noncreature spells cost {1} more to cast"), casting it
    // without paying its mana cost still costs {1} (CR 601.2f: the total
    // cost includes increases). With an eighth Mountain it is offered and
    // the Mountain pays; with none it isn't.
    for (lands, offered) in [(8usize, true), (7, false)] {
        let mut g = scen()
            .library(0, &["Island", "Island"])
            .library(1, &["Lightning Bolt", "Forest"])
            .battlefield(0, &vec!["Mountain"; lands])
            .battlefield(1, &["Thalia, Guardian of Thraben"])
            .hand(0, &["Etali, Primal Conqueror // Etali, Primal Sickness"])
            .start();
        let e = g.card(0, "Etali, Primal Conqueror // Etali, Primal Sickness", Zone::Hand);
        g.choose(Choice::Cast(e));
        quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseExiled) || (g.stack().is_empty() && at_priority(g)));
        let bolt = g.card(1, "Lightning Bolt", Zone::Exile);
        assert_eq!(g.kind() == Some(DecisionKind::ChooseExiled) && g.has_option(Choice::Card(bolt)), offered, "{lands} lands: {}", opts(&g));
        if !offered {
            continue;
        }
        g.choose(Choice::Card(bolt));
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
        assert_eq!(g.life(1), 17, "{}", log(&g));
        assert!(g.lands_of(0).all(|c| g.obj(c).tapped), "the tax was paid with the eighth Mountain");
    }
}

#[test]
fn citadel_casts_pay_life_and_thalias_tax_in_mana_cr_118_9d_601_2f() {
    // Lightning Bolt on top of P0's library with Bolas's Citadel and P1's
    // Thalia: 1 life (its mana value) and {1} (the tax, mana).
    for (lands, offered) in [(1usize, true), (0, false)] {
        let mut g = scen()
            .library(0, &["Lightning Bolt", "Island"])
            .battlefield(0, &["Bolas's Citadel"])
            .battlefield(0, &vec!["Mountain"; lands])
            .battlefield(1, &["Thalia, Guardian of Thraben"])
            .start();
        let bolt = top(&g, 0);
        assert_eq!(g.has_option(Choice::Cast(bolt)), offered, "{lands} lands: {}", opts(&g));
        if !offered {
            continue;
        }
        g.choose(Choice::Cast(bolt));
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
        assert_eq!((g.life(0), g.life(1)), (19, 17), "{}", log(&g));
        assert!(g.lands_of(0).all(|c| g.obj(c).tapped));
    }
}
