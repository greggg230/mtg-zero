//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, and best-of-three matches, with every check after
//! every action. The loops live in the engine library, `engine::fuzz::games`
//! (ENGINE.md 114.1; its module docs list every `FUZZ_*` setting and what
//! each mode checks), so zero-farm's worker runs the same code; these tests
//! run them with the process environment's settings (`tools/fuzz_farm.py`
//! drives them unchanged), and check that games and matches replay.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300), `FUZZ_FIRST=s` the
//! first seed (to rerun a failing seed alone), `FUZZ_MATCHES=n` the matches
//! (default `FUZZ_GAMES` / 20, at least 20).

use engine::fuzz::games::{
    self, constructed_bo3_mode, constructed_mode, delver_post_spec, eoe_mode, hidden_decks_mode, match_spec, new_game, play, play_hidden, play_match,
    play_match_spec, pool_mode, replay_match, sealed_mode, sealed_unplayable, HiddenStats, PoolMode, POOL_OVERRIDE,
};
use engine::fuzz::{NoHooks, Run};

/// ENGINE.md 147.8 (lane G's F1, 148.6): the default pool's game 2122 passes every check of `play`. After a mana ability
/// activated through `ActivateMana` (CR 605.3a) the next `ActivateMana` returned silently to `Priority` when no source
/// was left - a fact of the decider's hidden hand (a Spirit Guide) - so two worlds of the other player's showed it
/// `Priority` and `ActivateMana` after equal keys and equal legal actions (the open-decklist information-set walk).
/// Now the one-option `ActivateMana` (`PASS`) is asked and keyed (`KEY_AUTO`). Default pool only (another pool's seed
/// 2122 is another game); the cross pool's games 13536 and 19515 are the same finding (147.8).
#[test]
fn activate_mana_end_is_keyed_default_seed_2122() {
    if pool_mode() != PoolMode::Default || hidden_decks_mode() {
        return;
    }
    let p = play(2122, true);
    assert!(!p.actions.is_empty());
}

/// ENGINE.md 160: the same finding in the interactions pool (`FUZZ_POOL=interactions`), games 2031 (after a
/// `ChooseMode` of P0's: which mana ability of a permanent, inside `ActivateMana`) and 13518 (after a `ChooseMana`,
/// Karakas's ability on the stack) - the walk's decisions `Priority` / `ActivateMana`, the hand slot's ability the same
/// action id in both worlds. Reported on branches without 147.8 (`fix/hidden-refs` 3ca4fb2d, `fix/encoding-gate`);
/// they pass with 147.8's keyed end. Interactions pool only (another pool's seeds are other games).
#[test]
fn activate_mana_end_is_keyed_interactions_seeds() {
    if pool_mode() != PoolMode::Interactions || hidden_decks_mode() {
        return;
    }
    for seed in [2031u64, 13518] {
        let p = play(seed, true);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 161: default-pool games whose open-decklist walk found two worlds of P1's keyed apart only by P0's forced
/// answer naming a hidden card by its place, with equal views (160.4's count, on 296a45b8): 318 and 692 (`ExileCost`, a
/// lone blue card for Force of Will's exile at hand slots 2 / 4 and 2 / 3), 2456 and 2704 (`PayMana`, a lone Spirit
/// Guide's ability by its hand slot, `HAND_ABILITY` + 1 / 2 and + 3 / 4). They pass every check of `play`, the walk's
/// failing forced-answer check included, with the place masked in P1's worlds (`Game::forced_answer_seen`).
#[test]
fn forced_answers_naming_a_hidden_place_are_masked_default_seeds() {
    if pool_mode() != PoolMode::Default || hidden_decks_mode() {
        return;
    }
    for seed in [318u64, 692, 2456, 2704] {
        let p = play(seed, true);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 247: the farm fuzz's payment and cost stops, default pool, each passing every check of `play`. The
/// payment search's 2,000,000-copy cap (247.1: Rain of Filth's granted sacrifices and Squandered Resources' searched
/// for an escape whose graveyard cards were there; 100004730 ran past the farm's 300 s a seed before the cap):
/// 100002232, 100004730, 100014288, 4235003677. "picks within the budget" (247.2: a mana source activated first for
/// an escape's exile though the cost's own sacrifice fed it): 100061712. "PayMana: no way to pay" (247.5: delve's
/// graveyard picks took the cards its Black Lotus had to make up for): 100353550. "DiscardCost with no options" (247.4:
/// retrace's land card not pinned at a face choice): 100654376.
#[test]
fn payment_and_cost_stops_of_247_default_seeds() {
    if pool_mode() != PoolMode::Default || hidden_decks_mode() {
        return;
    }
    for seed in [100002232u64, 100004730, 100014288, 4235003677, 100061712, 100353550, 100654376] {
        let p = play(seed, true);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 247, hidden decklists (`FUZZ_HIDDEN_DECKS=1`; the default pool's games): "ChooseCost with no options"
/// (247.3: an additional cost's choice that castability counted through the 601.2g step's feeding abilities):
/// 100438064; the payment search's cap (247.1): 232013638, 100004138.
#[test]
fn payment_and_cost_stops_of_247_hidden_decklist_seeds() {
    if pool_mode() != PoolMode::Default || !hidden_decks_mode() {
        return;
    }
    for seed in [100438064u64, 232013638, 100004138] {
        let p = play_hidden(seed);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 140.10: the hidden-decklist games whose checks compared the deck's objects alone (a companion put into
/// the hand and shuffled away traded identities with a deck card in a world: "the viewer's own cards", "the match's
/// deck of the opponent", "the sampled deck lacks a shown ..."), replayed with every check of `play_hidden`: seed 331
/// (this branch's worlds), 2679, 3191, 3279, 3671, 5095, 6247, 6375, 6419, 7943 and 9671 (the rule-invariant
/// campaign's on 267ae603). The default pool only (the seeds are its games).
#[test]
fn hidden_decklist_worlds_keep_the_deck_whatever_object_holds_the_companion() {
    if pool_mode() != PoolMode::Default || sealed_mode() {
        return;
    }
    for seed in [331u64, 2679, 3191, 3279, 3671, 5095, 6247, 6375, 6419, 7943, 9671] {
        std::panic::catch_unwind(|| {
            play_hidden(seed);
        })
        .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1): {e:?}"));
    }
}

/// ENGINE.md 162: `FUZZ_POOL=cross`'s post-sideboard match 98 (a Doomsday mirror, P0 keeping its deck) passes every
/// check of `play_match_spec`. In game 2 P1 knew P0's whole 60 (R = the deck: no free card) and saw P0 find a listed
/// Underground Sea by a search; the sighting's terms were a = cnt F = 0, b = 0 - "no listed copy stood" - so the find
/// asked a fifth Sea and the constructed prior panicked "no deck holds what the viewer knows". Now a = cnt. Run in
/// every pool (its own pool for its thread, over `FUZZ_POOL`), not with hidden decklists.
#[test]
fn a_listed_card_s_find_with_the_deck_known_cross_post_sideboard_match_98() {
    if hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Cross)));
    let r = std::panic::catch_unwind(|| {
        let mut stats = HiddenStats::default();
        play_match_spec(98, &delver_post_spec(98), true, &mut stats)
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    let p = r.unwrap_or_else(|e| panic!("cross post-sideboard match 98 failed (rerun: FUZZ_POOL=cross FUZZ_GAMES=0 FUZZ_FIRST=98 FUZZ_MATCHES=1): {e:?}"));
    assert!(p.games >= 2, "game 2 was played");
}

/// ENGINE.md 173: the fuzz steward's queue entry c8af53b37bd8. `FUZZ_POOL=delver_dealt2` matches (both seats dealt,
/// 168.1) in which one step resolves another player's Urza's Bauble look at a card of the viewer's hand (keyed public by
/// design, 141.2) and then ends the game, so the next game begins in the same step (no sideboarding decision): the
/// match fuzz's world that plays on checked that look against the new game's hand and failed "P0's private key (kind
/// 3) carries the card's identity". The engine keyed nothing private: the card was the viewer's own, and the look was
/// the game that ended's. The check now reads the hand the look saw. The seeds were found under the `fuzz` profile
/// (determinize checks 1 in 4); the denominator is drawn from the match's random stream, so they are replayed at 4 in
/// every profile (`CHECKPOINT_EVERY_OVERRIDE`). Each must reach such a look (positive evidence). (ENGINE.md 192: the
/// remembered looks at another library moved the worlds' random draws; 52, 163 and 3,900,000,197 no longer reach one,
/// 30, 55 and 3,900,000,012 do - a scan of seeds 0..300 and 3,900,000,000..300 found 41. ENGINE.md 220.1-220.3: the
/// Legacy Hogaak Tempo cards in constructed play's table move the table prior's draws (TablePrior, 129.1) and renumber
/// the legacy definitions after them: 3,900,000,012 and 079 no longer reach one, 3,900,000,019 and 087 do. ENGINE.md
/// 220.4, the seven merged: the same scan finds 41, the nine below among them.)
#[test]
fn a_look_at_the_viewers_hand_in_a_step_that_begins_the_next_game() {
    use engine::fuzz::games::{CHECKPOINT_EVERY_OVERRIDE, PK_ENDED_GAME_HAND_LOOKS};
    use std::sync::atomic::Ordering::Relaxed;
    if hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::DelverDealtPerSeat)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        let mut short = Vec::new();
        for seed in [30u64, 55, 95, 135, 3_900_000_015, 3_900_000_019, 3_900_000_026, 3_900_000_087, 3_900_000_151] {
            let looks0 = PK_ENDED_GAME_HAND_LOOKS.load(Relaxed);
            let mut stats = HiddenStats::default();
            let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play_match_spec(seed, &match_spec(seed), true, &mut stats)))
                .unwrap_or_else(|e| panic!("delver_dealt2 match {seed} failed (rerun: FUZZ_POOL=delver_dealt2 FUZZ_FIRST={seed} FUZZ_MATCHES=1, fuzz profile): {e:?}"));
            assert!(p.games >= 2, "match {seed}: a next game was played");
            if PK_ENDED_GAME_HAND_LOOKS.load(Relaxed) == looks0 {
                short.push(seed);
            }
        }
        assert!(short.is_empty(), "matches {short:?} checked no look at the viewer's hand in a step that began the next game");
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 175: the fuzz steward's queue entry 349c142ff50b (`FUZZ_HIDDEN_DECKS=1` game 100017685, found under the
/// `fuzz` profile on b309fa08 and reproduced on main 4c7c3b58): in a hidden-decklist world of P0 played on, one step
/// resolves P1's Urza's Bauble look at a card at random of P0's hand (a Plains: keyed public by design, 141.2), then
/// runs on to the end step, where Memory Jar's delayed trigger discards P0's hand - so after the step the hand is empty
/// and the check, which read the hand after the step, failed "P1's private key (kind 3) carries the card's identity
/// (337) in a world of P0". The engine keyed nothing private (the card was the viewer's own); the check now reads the
/// hand the look saw. Replayed at the `fuzz` profile's denominator (4) in every profile, every check of `play_hidden`;
/// it must check a look at a card that left the hand in its step (positive evidence). Ignored since ENGINE.md 231 (the
/// game is gone, 231.8).
#[test]
#[ignore = "seed lost: ENGINE.md 231's 601.2g step changed the game (its decisions draw from the game's random stream); no new seed in the scans of 231.6; a farm-scale scan re-pins it (231.8)"]
fn a_look_at_the_viewers_hand_at_a_card_that_leaves_it_in_the_same_step() {
    use engine::fuzz::games::{CHECKPOINT_EVERY_OVERRIDE, PK_LEFT_HAND_LOOKS};
    use std::sync::atomic::Ordering::Relaxed;
    if pool_mode() != PoolMode::Default || sealed_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        let seed = 100_017_685u64;
        let looks0 = PK_LEFT_HAND_LOOKS.load(Relaxed);
        std::panic::catch_unwind(|| play_hidden(seed))
            .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        assert!(PK_LEFT_HAND_LOOKS.load(Relaxed) > looks0, "seed {seed} checked no look at a card that left the viewer's hand in its step");
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 206: the fuzz steward's queue entries 19cb3e1c1ea2, 4df7ea8a5770 and d95b549fc7a5, each found under the
/// `fuzz` profile and reproduced on main 4a84d570. In a world of the viewer one step resolves the other player's Urza's
/// Bauble look at a card at random of the viewer's hand (keyed public by design, 141.2) while the same step changes the
/// hand around the look, and the check - which guessed the hand the look saw from the hands before and after the step
/// (173, 175) - failed "P{p}'s private key (kind 3) carries the card's identity":
/// * `delver_dealt2` match 100,111,083: the viewer's Daze returned Volcanic Island to its hand (its alternative cost), the
///   look saw it, and the turn limit ended the game in the same step (both seats dealt: the next game began);
/// * default-pool game 100,037,121: the other player's Memory Jar exiled the viewer's hand and the viewer drew seven, the
///   look saw a drawn Gemstone Caverns, and Memory Jar's delayed trigger discarded it at the end step the step ran into;
/// * hidden-decklist game 100,098,483: the look saw the viewer's only card (Harvester of Misery), then the looker's
///   Timetwister shuffled it into the library and the viewer drew it back (the looker's knowledge of it gone).
/// The engine keyed nothing private: each card was the viewer's own. The check now recognises the look by the step's
/// record of it (`Game::hand_looks`). Replayed at the `fuzz` profile's denominator (4) in every profile with every check;
/// each must check its kind of look (positive evidence): a card that came into the hand in the step (the first two), one
/// that changed zones after the look (the last two). (The default-pool and hidden-decklist games are
/// `the_looks_of_206_in_default_seed_100037121_and_hidden_seed_100098483`, ignored since ENGINE.md 231: the match only.)
#[test]
fn a_look_at_the_viewers_hand_that_its_step_changes_around_the_look() {
    use engine::fuzz::games::{CHECKPOINT_EVERY_OVERRIDE, PK_ENTERED_HAND_LOOKS, PK_LEFT_HAND_LOOKS};
    use std::sync::atomic::Ordering::Relaxed;
    let counts = || (PK_ENTERED_HAND_LOOKS.load(Relaxed), PK_LEFT_HAND_LOOKS.load(Relaxed));
    // (the run's own pool and modes, read before the overrides: another pool's seeds are other games)
    let match_part = !hidden_decks_mode();
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        if match_part {
            POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::DelverDealtPerSeat)));
            let seed = 100_111_083u64;
            let (e0, _) = counts();
            let mut stats = HiddenStats::default();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play_match_spec(seed, &match_spec(seed), true, &mut stats)))
                .unwrap_or_else(|e| panic!("delver_dealt2 match {seed} failed (rerun: FUZZ_POOL=delver_dealt2 FUZZ_FIRST={seed} FUZZ_MATCHES=1, fuzz profile): {e:?}"));
            assert!(counts().0 > e0, "match {seed} checked no look at a card that came into the viewer's hand in its step");
        }
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 206's default-pool game 100,037,121 and hidden-decklist game 100,098,483 (see
/// `a_look_at_the_viewers_hand_that_its_step_changes_around_the_look`): a look at a card that came into the viewer's hand
/// and left it in the same step, one at a card that changed zones after the look. Ignored since ENGINE.md 231 (the games
/// are gone; until a scan re-pins them only the delver_dealt2 match's look, a card that came into the hand, is replayed:
/// 231.8).
#[test]
#[ignore = "seed lost: ENGINE.md 231's 601.2g step changed the game (its decisions draw from the game's random stream); no new seed in the scans of 231.6; a farm-scale scan re-pins it (231.8)"]
fn the_looks_of_206_in_default_seed_100037121_and_hidden_seed_100098483() {
    use engine::fuzz::games::{CHECKPOINT_EVERY_OVERRIDE, PK_ENTERED_HAND_LOOKS, PK_LEFT_HAND_LOOKS};
    use std::sync::atomic::Ordering::Relaxed;
    if pool_mode() != PoolMode::Default || sealed_mode() {
        return;
    }
    let counts = || (PK_ENTERED_HAND_LOOKS.load(Relaxed), PK_LEFT_HAND_LOOKS.load(Relaxed));
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        let seed = 100_037_121u64;
        let (e0, l0) = counts();
        std::panic::catch_unwind(|| play(seed, true))
            .unwrap_or_else(|e| panic!("default-pool seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        assert!(counts().0 > e0 && counts().1 > l0, "seed {seed} checked no look at a card that came into the viewer's hand and left it in its step");
        let seed = 100_098_483u64;
        let (_, l0) = counts();
        std::panic::catch_unwind(|| play_hidden(seed))
            .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        assert!(counts().1 > l0, "seed {seed} checked no look at a card that changed zones after the look in its step");
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 208: the fuzz steward's queue entries 971a41eb6fe1 and 7a779fdfd7e6 (default games found under the `fuzz`
/// profile on main, both in the open-decklist information-set walk of the player not deciding). 100056684: Kaito's
/// surveil in P0's worlds of P1's library, a Blightsteel Colossus put into the graveyard first - revealed and kept in
/// the library until the pile is placed, at its slot among the looked-at cards, so P0 saw its depth in the pile (the
/// 15th or the 14th card in the two worlds: "same keys, different views"); it now waits at the bottom. 100080416: P0
/// paying Celestial Colonnade's ability, the pool covering the rest - in one of P1's worlds no source could pay instead
/// and the payment ended silently (then the game: Ancient Tomb's damage), in the other P0 held a Simian Spirit Guide
/// and was asked ("same keys, the game over in one world only"); the one-option `PASS` is now asked and keyed.
/// Replayed at the `fuzz` profile's denominator (4) in every profile, every check of `play`.
#[test]
fn info_set_walk_default_seeds_of_208() {
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    if pool_mode() != PoolMode::Default || sealed_mode() || hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        for seed in [100_056_684u64, 100_080_416] {
            let p = std::panic::catch_unwind(|| play(seed, true))
                .unwrap_or_else(|e| panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1, fuzz profile): {e:?}"));
            assert!(!p.actions.is_empty(), "seed {seed}");
        }
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 209: the fuzz steward's queue entry 13f02539e054 (site games.rs:360, "rules checks violated: CR 608.1, 405.5",
/// 12 farm chunks of `games` and `hidden-games` on three main builds; the steward's replay of each chunk's first seed
/// passed). The violations were in five seeds of those chunks - 100,042,696 (open decklists) and 100,101,433, 100,134,291,
/// 100,148,738, 100,159,224 (hidden decklists), in worlds played on and in one game itself: Remand or Reprieve returns its
/// target spell to its owner's hand, then its "draw a card" with Jace, Wielder of Mysteries out and an empty library wins
/// the game (CR 104.1: at once), Remand still on the stack and the agenda dropped. The rules check now passes a departure in
/// a step that ended the game. Replayed at the `fuzz` profile's denominator (4) in every profile, every check of `play` /
/// `play_hidden`: no violation, and at least one departure the game's end excused (positive evidence).
#[test]
fn a_game_won_in_the_middle_of_a_resolution_leaves_its_spell_on_the_stack() {
    use engine::fuzz::games::{rules_stats, take_rules_violations, CHECKPOINT_EVERY_OVERRIDE};
    if pool_mode() != PoolMode::Default || sealed_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        let _ = take_rules_violations();
        let ended0 = rules_stats().ended_departures;
        play(100_042_696, true);
        for seed in [100_101_433u64, 100_134_291, 100_148_738, 100_159_224] {
            play_hidden(seed);
        }
        let v = take_rules_violations();
        assert!(v.is_empty(), "rules checks violated: {v:?}");
        assert!(rules_stats().ended_departures > ended0, "no departure in a step that ended the game was checked");
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 209: the fuzz steward's hang 9a40cd0d7010 (default-pool game 100,061,919, "no exit after 300 s", on three main
/// builds). At step 421 P1 orders 28 Ocelot Pride end-step triggers (copies of Ocelot Pride from Reflection of
/// Kiki-Jiki); with the city's blessing each creates a Cat and a copy of every token that entered this turn, doubling them
/// - billions of tokens, made one at a time at a cost growing with the copies (the completion check's paths through the
/// ordering never ended). Token creation now ends the game aborted at the permanents a row can show
/// (`rules_numbers::cr_111_2_tokens_past_the_permanents_a_row_can_show_end_the_game_aborted`): the game plays to its end with
/// every check, at the `fuzz` profile's denominator (4) in every profile. Ignored since ENGINE.md 231 (the game is gone;
/// the token limit itself stays tested by that rules_numbers scenario until a scan re-pins it, 231.8).
#[test]
#[ignore = "seed lost: ENGINE.md 231's 601.2g step changed the game (its decisions draw from the game's random stream); no new seed in the scans of 231.6; a farm-scale scan re-pins it (231.8)"]
fn an_exponential_run_of_ocelot_pride_copies_ends_default_seed_100061919() {
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    if pool_mode() != PoolMode::Default || sealed_mode() || hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| play(100_061_919, true));
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    let p = r.unwrap_or_else(|e| std::panic::resume_unwind(e));
    assert!(p.turns >= 37, "the game reached the end step of turn 37: {} turns", p.turns);
}

/// ENGINE.md 209: the fuzz steward's queue entry e940641803b8 (`FUZZ_POOL=delver_dealt2` games, site games.rs:3750): farm
/// chunks of 239, 319 and 325 games - the rest of a 1,000-seed chunk after a failing seed - failed their end-of-run check
/// "the legacy cards weren't played: 165 0 234" (Murktide Regents cast, their leave-the-graveyard triggers seen on the
/// stack, Prismari Charms cast). The trigger is seen in about one game in 80, and the check asked for it over any 200
/// games; now over `DELVER_TRIGGER_GAMES` (2,000), the casts still over 200. The 325-game chunk's counters pass; the same
/// counters over 2,000 games, or a run with no Regent or no Charm cast, still fail. Not with hidden decklists (the
/// counters below are the open-decklist walks').
#[test]
fn the_murktide_trigger_is_asserted_over_enough_games() {
    use engine::fuzz::Totals;
    if hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::DelverDealtPerSeat)));
    let totals = |m: u64, tr: u64, c: u64| {
        let mut t = Totals::default();
        for (k, v) in [("delver_murktides", m), ("delver_murk_triggers", tr), ("delver_charms", c), ("gy_lists", 500), ("is_walks", 4800), ("is_steps", 14000)] {
            t.set(k, v);
        }
        t
    };
    let fails = |n: u64, t: Totals| std::panic::catch_unwind(move || games::check_games(n, &t)).is_err();
    let r = std::panic::catch_unwind(|| {
        assert!(!fails(325, totals(165, 0, 234)), "the farm's 325-game chunk");
        assert!(!fails(1999, totals(1000, 0, 1500)), "under 2,000 games the trigger is not asserted");
        assert!(fails(games::DELVER_TRIGGER_GAMES, totals(1000, 0, 1500)), "over 2,000 games it is");
        assert!(!fails(games::DELVER_TRIGGER_GAMES, totals(1000, 1, 1500)));
        assert!(fails(325, totals(0, 0, 234)) && fails(325, totals(165, 0, 0)), "the casts over 200 games");
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 210: the fuzz steward's queue entries 2076692a1ae6 (8dd5a36f4ce0 before a line moved) and 88c42634bd1f,
/// found under the `fuzz` profile on main and replayed at its denominator (4) in every profile, every check of `play` /
/// `play_hidden`. Default-pool games 100,024,301, 100,031,416, 100,066,908 and 100,089,619 and hidden-decklist games
/// 100,021,777, 100,068,104, 100,124,410, 100,131,330 and 100,147,748: a spell of mana cost {0} (Mox Pearl, Mox Opal,
/// Black Lotus, Lotus Petal, Zuran Orb, Mishra's Bauble, Walking Ballista with X 0) cast beside a source that could be
/// activated for nothing; its payment asked "`PASS`, or that source instead" on its first run and finished when run
/// again (`refresh_pending`): the decider's own world failed the fuzz profile's "determinize changed the viewer's
/// decision", the other player's world ran past the decision (in release, its view check). Default-pool game
/// 100,054,973: P1's Brainstorm, cast in its upkeep, drew Metamorphosis Fanatic first and revealed it for miracle,
/// then put two other cards back, and P0 forgot the revealed card (`scramble`): P0's world dealt it anew as a card
/// with no miracle, the trigger asked P1 nothing and the world ran past P1's "cast it?" ("determinize changed the
/// view of P0 (mover P1)").
#[test]
fn determinize_keeps_the_viewers_decision_and_view_default_seeds() {
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    if pool_mode() != PoolMode::Default || hidden_decks_mode() || sealed_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        for seed in [100_024_301u64, 100_031_416, 100_066_908, 100_089_619, 100_054_973] {
            std::panic::catch_unwind(|| play(seed, true))
                .unwrap_or_else(|e| panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        }
        for seed in [100_021_777u64, 100_068_104, 100_124_410, 100_131_330, 100_147_748] {
            std::panic::catch_unwind(|| play_hidden(seed))
                .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        }
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 167: a sharded fuzz's end-of-run checks. `tools/fuzz_shards.py` runs `fuzz_random_games` /
/// `fuzz_random_matches` over disjoint seed ranges with `FUZZ_SHARD=1` (each shard prints its `FUZZ_TOTALS` and leaves
/// the checks over the whole run to this test), sums the shards' counters and runs this test with
/// `FUZZ_TOTALS_FILE=<the merged totals>`: the same checks as one run of all the seeds, on the totals. Without the
/// variable it checks nothing (the gate wants its "fuzz totals: ... checked" lines as positive evidence).
#[test]
fn fuzz_shard_totals() {
    let Ok(path) = std::env::var("FUZZ_TOTALS_FILE") else {
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("FUZZ_TOTALS_FILE={path}: {e}"));
    for line in games::check_merged(&text).unwrap_or_else(|e| panic!("FUZZ_TOTALS_FILE={path}: {e}")) {
        println!("{line}");
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Run::Failed { seed, payload: e } = games::random_games(first, n, &mut NoHooks) {
        panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
    }
}

/// ENGINE.md 142.10 (the Fable review's X3): interactions games 5,682, 6,025, 7,790 and 12,725
/// reach picks in progress in two tasks of the agenda - Etali, Primal Conqueror's trigger keeps the
/// cards it exiled as picks while a free cast from among them pays a cost that picks objects (5,682:
/// Abhorrent Oculus exiling six graveyard cards) - a loud failure of the encoding before. Each
/// plays through with every check, and its rows have the enclosing task's picks as `links` tokens
/// with their depth (`lk::DEPTH`); 5,682's for both viewers. The seeds are release-profile games: the checkpoint's
/// denominator (40) is drawn from the game's random stream, so under the `fuzz` profile (4) they would be other games
/// without Etali - the test pins 40 (`CHECKPOINT_EVERY_OVERRIDE`, ENGINE.md 173.2).
#[test]
fn enclosing_picks_of_etali_s_casts_are_encoded() {
    use engine::az::encode::CODE;
    use engine::az::tail_encode::{lk, seg, Row};
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Interactions)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(40)));
    // (ENGINE.md 231.6: the 601.2g step changed these games; the scan `scan_seeds_for_the_seed_pinned_tests` re-found
    // 12,471 in interactions seeds 0..24,000 (every check, the checkpoint at 40), its rows with the enclosing picks for
    // both viewers.)
    for seed in [12471] {
        let p = play(seed, true);
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        let mut deep = [0u32; 2];
        for &x in &p.actions {
            for v in 0..2u8 {
                deep[v as usize] += Row::of(&g, v)
                    .tokens(seg::LINKS)
                    .iter()
                    .filter(|(_, t)| (t[CODE + lk::KIND + lk::K_PICKED] == 1.0 || t[CODE + lk::KIND + lk::K_PICKED_DEF] == 1.0) && t[CODE + lk::DEPTH] > 0.0)
                    .count() as u32;
            }
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), p.fingerprint, "seed {seed}: the replay");
        println!("seed {seed}: an enclosing task's pick tokens by viewer {deep:?}");
        assert!(deep[0] + deep[1] > 0, "seed {seed}: no enclosing task's pick token");
        if seed == 12471 {
            assert!(deep[0] > 0 && deep[1] > 0, "seed {seed}: {deep:?}");
        }
    }
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
}

#[test]
fn same_seed_and_actions_replay_identically() {
    for seed in 1000..1030 {
        if sealed_unplayable(seed).is_some() {
            continue;
        }
        let a = play(seed, false);
        // Replay the recorded actions from the same seed.
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), a.fingerprint, "seed {seed}");
        assert_eq!(keys, a.keys, "seed {seed}: chance keys differ");
        assert_eq!(g.outcome(), Some(a.outcome));
        // And the whole random game is reproducible.
        let b = play(seed, false);
        assert_eq!(b.fingerprint, a.fingerprint);
    }
}

#[test]
fn fuzz_random_matches() {
    let games: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n: u64 = std::env::var("FUZZ_MATCHES").ok().and_then(|s| s.parse().ok()).unwrap_or((games / 20).max(20));
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Run::Failed { seed, payload: e } = games::random_matches(first, n, &mut NoHooks) {
        panic!("match {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_MATCHES=1): {e:?}")
    }
}

#[test]
fn same_seed_and_actions_replay_matches_identically() {
    if (constructed_mode() && !constructed_bo3_mode()) || eoe_mode() || sealed_mode() {
        return;
    }
    for seed in 2000..2012 {
        let a = play_match(seed, false);
        let (fp, results, keys) = replay_match(seed, &match_spec(seed), &a.actions);
        assert_eq!((fp, results), (a.fingerprint, a.results), "match {seed}");
        assert_eq!(keys, a.keys, "match {seed}: chance keys differ");
        assert_eq!(play_match(seed, false).fingerprint, a.fingerprint);
        // the post-sideboard matches too (ENGINE.md 130)
        if constructed_bo3_mode() {
            let sp = delver_post_spec(seed);
            let a = play_match_spec(seed, &sp, false, &mut HiddenStats::default());
            let (fp, results, keys) = replay_match(seed, &sp, &a.actions);
            assert_eq!((fp, results, &keys), (a.fingerprint, a.results, &a.keys), "post-sideboard match {seed}");
            assert_eq!(play_match_spec(seed, &sp, false, &mut HiddenStats::default()).fingerprint, a.fingerprint);
        }
    }
}

/// ENGINE.md 231: the seed scan that re-pinned the four seed-pinned tests above after the 601.2g step changed their
/// games (a new decision draws from each game's random stream). Ignored; `SCAN_KIND` = `left_look` (hidden-decklist
/// default games with a look at a card that left the viewer's hand in its step), `entered_look` (default games with a
/// look at a card that came into the hand and left it in its step), `etali` (interactions games whose rows carry an
/// enclosing task's pick tokens for both viewers), `ocelot` (default games that end aborted at the permanents a row can
/// show: an exponential token run); `SCAN_FROM`, `SCAN_N`. Each kind under its test's checkpoint denominator, every
/// check on (the games the tests replay). Prints each matching seed.
#[test]
#[ignore]
fn scan_seeds_for_the_seed_pinned_tests() {
    use engine::az::encode::CODE;
    use engine::az::tail_encode::{lk, seg, Row};
    use engine::fuzz::games::{CHECKPOINT_EVERY_OVERRIDE, PK_ENTERED_HAND_LOOKS, PK_LEFT_HAND_LOOKS};
    use std::sync::atomic::Ordering::Relaxed;
    let kind = std::env::var("SCAN_KIND").expect("SCAN_KIND");
    let from: u64 = std::env::var("SCAN_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(100_000_000);
    let n: u64 = std::env::var("SCAN_N").ok().and_then(|s| s.parse().ok()).unwrap_or(1000);
    // (`SCAN_CHECKS=0`: the games without the checks, a pre-filter for the kinds the checks don't count - the games are
    // the same, the checks read copies; each hit is then replayed by its test with every check.)
    let checks = std::env::var("SCAN_CHECKS").map_or(true, |v| v != "0");
    let pool = if kind == "etali" { PoolMode::Interactions } else { PoolMode::Default };
    POOL_OVERRIDE.with(|c| c.set(Some(pool)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(if kind == "etali" { 40 } else { 4 })));
    for seed in from..from + n {
        let (e0, l0) = (PK_ENTERED_HAND_LOOKS.load(Relaxed), PK_LEFT_HAND_LOOKS.load(Relaxed));
        let hit = match kind.as_str() {
            "left_look" => std::panic::catch_unwind(|| play_hidden(seed)).is_ok() && PK_LEFT_HAND_LOOKS.load(Relaxed) > l0,
            "entered_look" => std::panic::catch_unwind(|| play(seed, true)).is_ok() && PK_ENTERED_HAND_LOOKS.load(Relaxed) > e0 && PK_LEFT_HAND_LOOKS.load(Relaxed) > l0,
            "ocelot" => std::panic::catch_unwind(|| play(seed, checks)).is_ok_and(|p| p.outcome.reason == engine::game::EndReason::Aborted),
            "etali" => std::panic::catch_unwind(|| {
                let p = play(seed, checks);
                let mut g = new_game(seed);
                let mut keys = Vec::new();
                let mut deep = [0u32; 2];
                for &x in &p.actions {
                    for v in 0..2u8 {
                        deep[v as usize] += Row::of(&g, v)
                            .tokens(seg::LINKS)
                            .iter()
                            .filter(|(_, t)| (t[CODE + lk::KIND + lk::K_PICKED] == 1.0 || t[CODE + lk::KIND + lk::K_PICKED_DEF] == 1.0) && t[CODE + lk::DEPTH] > 0.0)
                            .count() as u32;
                    }
                    g.apply(x, &mut keys).unwrap();
                }
                if std::env::var("SCAN_VERBOSE").is_ok() {
                    println!("seed {seed}: deep {deep:?} turns {}", p.turns);
                }
                if deep[0] + deep[1] > 0 {
                    println!("SCAN etali_any seed {seed} deep {deep:?}");
                }
                deep[0] > 0 && deep[1] > 0
            })
            .unwrap_or_else(|e| {
                println!("seed {seed}: panicked {e:?}");
                false
            }),
            other => panic!("SCAN_KIND={other}"),
        };
        if hit {
            println!("SCAN {kind} seed {seed}");
        }
    }
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
}

/// ENGINE.md 223: the Doomsday mirror's post-sideboard best-of-three match 163 (one episode a game) passes every check
/// of `play_match_spec`, the information-set walk (`check_match_information_set`) included. Reported by the Hogaak
/// integration's first FUZZ_POOL=hogaak probe (pre-existing on main 1e7416c5 and 45dbf2e1). A scan:
/// `ENGINE_223_DECKS=<p0>,<p1>` (files of `data/decks` without `.json`; default the Doomsday 75 twice),
/// `ENGINE_223_START=1|2` (the first game played, default 2: post-sideboard), `ENGINE_223_SEEDS=a,b,..` or
/// `ENGINE_223_SEEDS=a..b` (default 163).
#[test]
fn engine_223_doomsday_mirror_post_board_match_163_information_set() {
    use engine::az::constructed::Decklist;
    let load = |name: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/decks").join(format!("{name}.json"));
        Decklist::parse(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
    };
    let decks = std::env::var("ENGINE_223_DECKS").unwrap_or("doomsday_8027957,doomsday_8027957".into());
    let d: Vec<Decklist> = decks.split(',').map(load).collect();
    assert_eq!(d.len(), 2, "ENGINE_223_DECKS: two decks");
    let side = |d: &Decklist| -> Vec<String> { d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect() };
    let start: u64 = std::env::var("ENGINE_223_START").map(|s| s.parse().unwrap()).unwrap_or(2);
    let v = serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3",
        "decks": {"p0": d[0].main_names(), "p1": d[1].main_names(), "p0_sideboard": side(&d[0]), "p1_sideboard": side(&d[1])},
        "start_game": start, "game_episodes": true});
    let sp = engine::az::MtgSpec::parse(&v.to_string()).unwrap();
    let seeds: Vec<u64> = match std::env::var("ENGINE_223_SEEDS") {
        Ok(s) if s.contains("..") => {
            let (a, b) = s.split_once("..").unwrap();
            (a.parse().unwrap()..b.parse().unwrap()).collect()
        }
        Ok(s) => s.split(',').map(|x| x.parse().unwrap()).collect(),
        Err(_) => vec![163],
    };
    let n = seeds.len();
    for seed in seeds {
        play_match_spec(seed, &sp, true, &mut HiddenStats::default());
    }
    println!("ENGINE.md 223 scan: {decks} from game {start}: {n} matches passed");
}

/// ENGINE.md 248: the hidden-information entries of the fuzz steward's queue of 2026-11-23, found under the `fuzz`
/// profile on main and replayed at its denominator (4) in every profile, every check of `play` / `play_hidden`.
/// Default-pool games: 100,030,136 (106b63d67b1d: a searched-for Metamorphosis Fanatic drawn beside a twin and
/// revealed for miracle, its place in its owner's row by card index - "same keys, different rows"), 100,391,600 and
/// 100,661,596 (f1eebdfda0a1: the drawn Fanatic's knowledge moved onto a twin before the reveal - "same keys,
/// different views"), 100,398,690 (f1eebdfda0a1: a fetch land's ability whose sacrificed source was shuffled back,
/// its card slot found by a search in one world), 100,690,719 (88ef40b06b03: Robber of the Rich shuffled away by
/// Timetwister, "attacked with a Rogue" read from its slot's dealt card - "determinize changed the viewer's
/// decision", the `fuzz` profile's assertion) and 100,492,136 (fb92013286d3: three face-down permanents tapped for
/// waterbend in two orders - "keyed apart only by the other player's forced answers"). Hidden-decklist games:
/// 100,153,769 (ea73c6338e3f: Lim-Dul's Vault's random bottom order over a tutor's find group), 100,085,630
/// (d4639eb826ec: a graveyard target's list slot after the loop rule withheld an option) and 100,096,300 and
/// 100,237,234 (a3c8e29c2e3e: Tamiyo's +1 with a Blightsteel Colossus among the revealed cards). The payment entries
/// of the same queue (467b098e2037, ddfc59e36d7e and f1eebdfda0a1's game 100,445,205): 251's test below.
#[test]
fn info_set_walk_seeds_of_248() {
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    if pool_mode() != PoolMode::Default || sealed_mode() || hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        for seed in [100_030_136u64, 100_391_600, 100_661_596, 100_398_690, 100_690_719, 100_492_136] {
            let p = std::panic::catch_unwind(|| play(seed, true))
                .unwrap_or_else(|e| panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1, fuzz profile): {e:?}"));
            assert!(!p.actions.is_empty(), "seed {seed}");
        }
        for seed in [100_153_769u64, 100_085_630, 100_096_300, 100_237_234] {
            std::panic::catch_unwind(|| play_hidden(seed))
                .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1, fuzz profile): {e:?}"));
        }
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}

/// ENGINE.md 251 (248.2's open entries of the fuzz steward's queue): a payment's `vm` and `min_class` are shown only to
/// the viewers of the payer's hand. Default-pool games 100,069,409 and 100,148,623 (ddfc59e36d7e: "determinize changed
/// the view" of the player not paying - the world dealt the payer a Spirit Guide, `vm` `None` / `Some`) and 100,445,205
/// (f1eebdfda0a1: "same keys, different views", `min_class` 4 / 3); hidden-decklist game 100,200,461 (467b098e2037: "a
/// hidden-decklist world ...: the view"). Replayed at the `fuzz` profile's denominator (4) in every profile.
#[test]
fn a_payments_search_facts_are_the_payers_seeds_of_251() {
    use engine::fuzz::games::CHECKPOINT_EVERY_OVERRIDE;
    if pool_mode() != PoolMode::Default || sealed_mode() || hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Default)));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(Some(4)));
    let r = std::panic::catch_unwind(|| {
        for seed in [100_069_409u64, 100_148_623, 100_445_205] {
            let p = std::panic::catch_unwind(|| play(seed, true))
                .unwrap_or_else(|e| panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1, fuzz profile): {e:?}"));
            assert!(!p.actions.is_empty(), "seed {seed}");
        }
        std::panic::catch_unwind(|| play_hidden(100_200_461))
            .unwrap_or_else(|e| panic!("hidden-decklist seed 100200461 failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST=100200461 FUZZ_GAMES=1, fuzz profile): {e:?}"));
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    CHECKPOINT_EVERY_OVERRIDE.with(|c| c.set(None));
    r.unwrap_or_else(|e| std::panic::resume_unwind(e));
}
