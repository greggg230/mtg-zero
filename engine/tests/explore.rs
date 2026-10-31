//! The habit tags of self-play exploration (ENGINE.md 75, `az/explore.rs`): each tag on a constructed
//! position of the Delver mirror's cards, and not on the positions next to it (another step, turn,
//! hand or board), and the tags of a whole game: only on the decisions they name, never changing a
//! game (the same moves with the tags read at every decision).
use engine::az::explore::{self, DAZE_TAPOUT, DELVE_SKIP_SPELL, FETCH_CRACK, FOW_PITCH_FOW, KEEP_ONE_LANDER, VOLCANIC_FETCH};
use engine::az::{MtgAz, MtgSpec};
use engine::db::{AbilitySlot, CardDb};
use engine::decision::DecisionKind;
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, Game, PlayerId, Step, Zone};
use zero_search::SearchGame;

fn scen() -> Scenario {
    Scenario::new(CardDb::constructed()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

/// The pending decision's options with their tags: (choice, tag).
fn tagged(g: &Game) -> Vec<(Choice, u8)> {
    let d = g.pending_decision().expect("a decision");
    let legal: Vec<u16> = d.options.iter().map(|o| o.action).collect();
    let mut t = vec![0u8; legal.len()];
    explore::tags(g, &legal, &mut t);
    d.options.iter().map(|o| o.choice).zip(t).collect()
}

fn tag_of(g: &Game, c: Choice) -> u8 {
    tagged(g).into_iter().find(|x| x.0 == c).unwrap_or_else(|| panic!("{c:?} not offered: {:?}", g.options_desc())).1
}

fn no_tags(g: &Game) -> bool {
    tagged(g).iter().all(|x| x.1 == 0)
}

/// The Delver mirror (both seats the pinned 60, mulligans on).
fn delver_mirror() -> MtgSpec {
    let names = engine::az::constructed::Decklist::izzet_delver().main_names();
    MtgSpec::parse(&serde_json::json!({"max_turns": 60, "mulligans": true, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string()).unwrap()
}

fn card(g: &Game, p: PlayerId, name: &str, z: Zone) -> CardIdx {
    g.card(p, name, z)
}

#[test]
fn the_tags_and_their_ids() {
    let names: Vec<&str> = explore::TAGS.iter().map(|t| t.name).collect();
    assert_eq!(names, ["fetch_crack", "daze_tapout", "fow_pitch_fow", "delve_skip_spell", "volcanic_fetch", "keep_one_lander"]);
    assert_eq!((FETCH_CRACK, DAZE_TAPOUT, FOW_PITCH_FOW, DELVE_SKIP_SPELL, VOLCANIC_FETCH, KEEP_ONE_LANDER), (1, 2, 3, 4, 5, 6));
    assert_eq!(<MtgAz as SearchGame>::EXPLORE_TAGS, explore::TAGS);
    assert_eq!(explore::TAGS.iter().map(|t| t.window).collect::<Vec<_>>(), [2, 1, 0, 1, 0, 0]);
    for f in explore::FETCHES {
        if let Ok(d) = CardDb::constructed().expect_id(f) {
            assert!(CardDb::constructed().def(d).types & engine::db::types::LAND != 0, "{f}");
        }
    }
}

#[test]
fn fetch_crack_tags_a_fetchland_activation_only() {
    let g = scen().battlefield(0, &["Flooded Strand", "Wasteland", "Volcanic Island"]).hand(0, &["Brainstorm"]).at(Step::Main1, 0).start();
    let strand = card(&g, 0, "Flooded Strand", Zone::Battlefield);
    let t = tagged(&g);
    let fetch: Vec<_> = t.iter().filter(|x| matches!(x.0, Choice::Ability(c, _) if c == strand)).collect();
    assert!(!fetch.is_empty() && fetch.iter().all(|x| x.1 == FETCH_CRACK), "{:?}", g.options_desc());
    assert!(t.iter().filter(|x| !matches!(x.0, Choice::Ability(c, _) if c == strand)).all(|x| x.1 == 0), "{t:?}");
    assert_eq!(tag_of(&g, Choice::Pass), 0);
    // on the opponent's turn, in response: still a crack (the window counts turns, not whose)
    let mut g = scen().battlefield(0, &["Misty Rainforest"]).at(Step::Upkeep, 1).start();
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    let misty = card(&g, 0, "Misty Rainforest", Zone::Battlefield);
    assert!(tagged(&g).iter().any(|x| matches!(x.0, Choice::Ability(c, _) if c == misty) && x.1 == FETCH_CRACK));
}

/// P0 at its main phase with `lands` (the first `tapped` of them tapped), `hand`; P1 with
/// `opp_lands`, `opp_hand`; turn `turn`.
fn daze_spot(lands: &[&str], tapped: usize, hand: &[&str], opp_lands: &[&str], opp_hand: &[&str], turn: u32) -> Game {
    let mut s = scen().battlefield(0, lands).hand(0, hand).battlefield(1, opp_lands).hand(1, opp_hand).at(Step::Main1, 0).turn_number(turn);
    for l in &lands[..tapped] {
        s = s.tapped(0, l);
    }
    s.start()
}

#[test]
fn daze_tapout_tags_a_cast_that_leaves_no_land_into_a_live_daze() {
    let drc = "Dragon's Rage Channeler";
    let g = daze_spot(&["Volcanic Island"], 0, &[drc, "Ponder"], &["Island"], &["Daze"], 3);
    let c = card(&g, 0, drc, Zone::Hand);
    assert_eq!(tag_of(&g, Choice::Cast(c)), DAZE_TAPOUT);
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, "Ponder", Zone::Hand))), DAZE_TAPOUT);
    assert_eq!(tag_of(&g, Choice::Pass), 0);
    // a land left up: none (an uncracked fetch counts as one)
    let g = daze_spot(&["Volcanic Island", "Polluted Delta"], 0, &[drc], &["Island"], &["Daze"], 3);
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, drc, Zone::Hand))), 0);
    // two lands, one tapped: the cast taps out
    let g = daze_spot(&["Island", "Volcanic Island"], 1, &[drc], &["Island"], &["Daze"], 3);
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, drc, Zone::Hand))), DAZE_TAPOUT);
    // Daze not live: no Island on the other side, an empty hand, turn 2
    for (opp_lands, opp_hand, turn) in [(&["Wasteland"][..], &["Daze"][..], 3), (&["Island"][..], &[][..], 3), (&["Island"][..], &["Daze"][..], 2)] {
        let g = daze_spot(&["Volcanic Island"], 0, &[drc], opp_lands, opp_hand, turn);
        assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, drc, Zone::Hand))), 0, "{opp_lands:?} {opp_hand:?} turn {turn}");
    }
    // Thundering Falls is an Island too (Daze returns it); a free spell never taps out
    let g = daze_spot(&["Volcanic Island"], 0, &[drc, "Mishra's Bauble"], &["Thundering Falls"], &["Daze"], 4);
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, drc, Zone::Hand))), DAZE_TAPOUT);
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, "Mishra's Bauble", Zone::Hand))), 0);
    // Murktide Regent: its cost with the most delve (UU with five cards in the graveyard)
    let mut g = scen()
        .battlefield(0, &["Volcanic Island", "Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Lightning Bolt", "Island", "Wasteland"])
        .hand(0, &["Murktide Regent"])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .turn_number(5)
        .start();
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, "Murktide Regent", Zone::Hand))), DAZE_TAPOUT);
    // not its own main phase (a response): none
    let bolt_spot = scen().battlefield(0, &["Volcanic Island"]).hand(0, &["Brainstorm"]).battlefield(1, &["Island"]).hand(1, &["Daze"]).at(Step::Upkeep, 1).turn_number(4);
    g = bolt_spot.start();
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    assert_eq!(tag_of(&g, Choice::Cast(card(&g, 0, "Brainstorm", Zone::Hand))), 0);
}

/// P0 casts Dragon's Rage Channeler; P1 casts Force of Will for its alternative cost: the game at
/// P1's first ExileCost decision.
fn fow_spot(p1_hand: &[&str]) -> Game {
    let mut g = scen().battlefield(0, &["Volcanic Island"]).hand(0, &["Dragon's Rage Channeler"]).hand(1, p1_hand).at(Step::Main1, 0).turn_number(3).start();
    let drc = card(&g, 0, "Dragon's Rage Channeler", Zone::Hand);
    g.choose(Choice::Cast(drc));
    for _ in 0..50 {
        if g.pending_decision().is_some_and(|d| d.player == 1) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = d.options[0].choice;
        g.choose(c);
    }
    let fow = card(&g, 1, "Force of Will", Zone::Hand);
    g.choose(Choice::Cast(fow));
    for _ in 0..50 {
        match g.kind() {
            Some(DecisionKind::ExileCost) => return g,
            Some(DecisionKind::ChooseAltCost) => {
                g.choose(Choice::Number(1));
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    panic!("no ExileCost: {:?}\n{}", g.options_desc(), g.log_lines().join("\n"));
}

#[test]
fn fow_pitch_fow_tags_pitching_a_second_force_of_will_over_another_blue_card() {
    let g = fow_spot(&["Force of Will", "Force of Will", "Brainstorm", "Lightning Bolt"]);
    let t = tagged(&g);
    let fows: Vec<_> = t.iter().filter(|x| matches!(x.0, Choice::Card(c) if g.name(c) == "Force of Will")).collect();
    assert_eq!(fows.len(), 1, "{:?}", g.options_desc());
    assert!(fows.iter().all(|x| x.1 == FOW_PITCH_FOW));
    assert!(t.iter().filter(|x| !matches!(x.0, Choice::Card(c) if g.name(c) == "Force of Will")).all(|x| x.1 == 0));
    assert!(t.iter().any(|x| matches!(x.0, Choice::Card(c) if g.name(c) == "Brainstorm")));
    // only Forces of Will to pitch: nothing to deviate to, no tag
    let g = fow_spot(&["Force of Will", "Force of Will", "Lightning Bolt"]);
    assert!(no_tags(&g), "{:?}", g.options_desc());
    // only another blue card: no Force of Will to pitch
    let g = fow_spot(&["Force of Will", "Ponder"]);
    assert!(no_tags(&g));
}

#[test]
fn delve_skip_spell_tags_a_non_spell_pick_while_a_spell_is_offered() {
    let mut g = scen()
        .battlefield(0, &["Island", "Volcanic Island"])
        .graveyard(0, &["Island", "Brainstorm", "Wasteland", "Ponder", "Mishra's Bauble", "Volcanic Island"])
        .hand(0, &["Murktide Regent"])
        .at(Step::Main1, 0)
        .start();
    let m = card(&g, 0, "Murktide Regent", Zone::Hand);
    g.choose(Choice::Cast(m));
    let mut seen = 0;
    let mut picks = 0;
    for _ in 0..80 {
        if g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        let t = tagged(&g);
        if g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
            let spell = |c: CardIdx| matches!(g.name(c), "Brainstorm" | "Ponder");
            let offered = t.iter().any(|x| matches!(x.0, Choice::Card(c) if spell(c)));
            for x in &t {
                let want = match x.0 {
                    Choice::Card(c) if offered && !spell(c) => DELVE_SKIP_SPELL,
                    _ => 0,
                };
                assert_eq!(x.1, want, "{:?}: {:?}", x.0, g.options_desc());
            }
            seen += offered as usize;
            // pick the first card up the graveyard: a land first (tagged), then the spells
            let c = t.iter().find_map(|x| if let Choice::Card(c) = x.0 { Some(c) } else { None }).unwrap();
            picks += 1;
            g.choose(Choice::Card(c));
        } else {
            assert!(no_tags(&g), "{:?} {:?}", g.kind(), g.options_desc());
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    assert!(seen >= 2 && picks >= 3, "seen {seen}, picks {picks}");
    // another graveyard choice (not delve): none - e.g. a graveyard of lands only: no spell offered
    let mut g = scen()
        .battlefield(0, &["Island", "Volcanic Island"])
        .graveyard(0, &["Island", "Wasteland", "Volcanic Island", "Island", "Wasteland"])
        .hand(0, &["Murktide Regent"])
        .at(Step::Main1, 0)
        .start();
    let m = card(&g, 0, "Murktide Regent", Zone::Hand);
    g.choose(Choice::Cast(m));
    for _ in 0..40 {
        if g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        assert!(no_tags(&g));
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// P0 cracks its Flooded Strand on turn `turn` (P`active` active) with `library`: the game at the
/// search.
fn fetch_search(turn: u32, active: PlayerId, library: &[&str]) -> Game {
    let mut g = Scenario::new(CardDb::constructed())
        .spec(|s| s.auto_single = false)
        .library(0, library)
        .library(1, &["Island"; 10])
        .battlefield(0, &["Flooded Strand"])
        .at(if active == 0 { Step::Main1 } else { Step::Upkeep }, active)
        .turn_number(turn)
        .start();
    if active == 1 {
        g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    }
    let s = card(&g, 0, "Flooded Strand", Zone::Battlefield);
    let a = g.pending_decision().unwrap().options.iter().find_map(|o| if let Choice::Ability(c, sl) = o.choice { (c == s).then_some((c, sl)) } else { None });
    let (c, sl): (CardIdx, AbilitySlot) = a.unwrap_or_else(|| panic!("no fetch: {:?}", g.options_desc()));
    g.choose(Choice::Ability(c, sl));
    for _ in 0..60 {
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            assert!(matches!(g.stack().last().map(|x| x.kind), Some(StackKind::Ability { .. })), "the fetch's ability on the stack");
            return g;
        }
        let c = if g.kind() == Some(DecisionKind::Priority) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("no search: {:?}\n{}", g.options_desc(), g.log_lines().join("\n"));
}

#[test]
fn volcanic_fetch_tags_volcanic_island_over_the_basic_in_the_first_two_turns() {
    let lib = ["Volcanic Island", "Island", "Brainstorm", "Ponder", "Volcanic Island"];
    let db = CardDb::constructed();
    let def = |n: &str| Choice::Def(db.expect_id(n).unwrap());
    for turn in [1, 3, 4] {
        let g = fetch_search(turn, 0, &lib);
        assert_eq!(tag_of(&g, def("Volcanic Island")), VOLCANIC_FETCH, "turn {turn}");
        assert_eq!(tag_of(&g, def("Island")), 0);
    }
    // later, on the opponent's turn, or without the basic: none
    assert!(no_tags(&fetch_search(5, 0, &lib)));
    assert!(no_tags(&fetch_search(4, 1, &lib)));
    assert!(no_tags(&fetch_search(3, 0, &["Volcanic Island", "Brainstorm", "Volcanic Island"])));
}

#[test]
fn keep_one_lander_tags_keeping_an_opening_seven_with_at_most_one_land() {
    // Delver mirror games until both kinds of hands are seen at the opening seven (the decider's
    // own hand); a second hand (after a mulligan) is never tagged
    let spec = delver_mirror();
    let (mut few, mut many, mut after) = (0, 0, 0);
    for seed in 0..400u64 {
        let mut g = MtgAz::new(&spec, seed).unwrap();
        let mut key = Vec::new();
        for _ in 0..8 {
            let Some(d) = g.g.pending_decision() else { break };
            if d.kind == DecisionKind::Mulligan {
                let p = d.player;
                let pl = g.g.player(p);
                let lands = pl.hand.iter().filter(|&&c| g.g.is_type(c, engine::db::types::LAND)).count();
                let mut legal = Vec::new();
                g.legal_actions(&mut legal);
                let mut t = vec![0u8; legal.len()];
                g.explore_tags(&legal, &mut t);
                let keep = d.options.iter().position(|o| o.choice == Choice::Pass).unwrap();
                let want = if pl.mulligans == 0 && lands <= 1 { KEEP_ONE_LANDER } else { 0 };
                assert_eq!(t[keep], want, "seed {seed}: {lands} lands, mulligans {}", pl.mulligans);
                assert!(t.iter().enumerate().all(|(k, &x)| k == keep || x == 0));
                if pl.mulligans > 0 {
                    after += 1;
                } else if lands <= 1 {
                    few += 1;
                } else {
                    many += 1;
                }
                // take a mulligan with a one-lander (the next seven is not tagged), keep otherwise
                let a = if want != 0 { engine::decision::act::YES } else { engine::decision::act::PASS };
                g.step(a, &mut key).unwrap();
                continue;
            }
            let a = d.options[0].action;
            g.step(a, &mut key).unwrap();
        }
        if few >= 5 && many >= 20 && after >= 5 {
            break;
        }
    }
    assert!(few >= 5 && many >= 20 && after >= 5, "one-landers {few}, others {many}, after a mulligan {after}");
}

#[test]
fn a_game_reads_tags_without_changing() {
    // A whole random Delver game with the tags read at every decision plays the same moves and
    // ends in the same state as without; tags only on their decision kinds.
    let spec = delver_mirror();
    for seed in 0..6u64 {
        let play = |read: bool| {
            let mut g = MtgAz::new(&spec, seed).unwrap();
            let mut rng = engine::rng::Rng::new(seed);
            let (mut key, mut legal) = (Vec::new(), Vec::new());
            let mut kinds = std::collections::BTreeMap::new();
            let mut moves = Vec::new();
            for _ in 0..4000 {
                if g.to_move().is_none() {
                    break;
                }
                legal.clear();
                g.legal_actions(&mut legal);
                if read {
                    let mut t = vec![0u8; legal.len()];
                    g.explore_tags(&legal, &mut t);
                    for &x in t.iter().filter(|&&x| x != 0) {
                        *kinds.entry((x, format!("{:?}", g.g.pending_decision().unwrap().kind))).or_insert(0) += 1;
                    }
                }
                let a = legal[rng.below(legal.len() as u32) as usize];
                moves.push(a);
                g.step(a, &mut key).unwrap();
            }
            (moves, g.g.fingerprint(), kinds)
        };
        let (m0, f0, _) = play(false);
        let (m1, f1, kinds) = play(true);
        assert_eq!((m0, f0), (m1, f1), "seed {seed}");
        let allowed = |t: u8, k: &str| match t {
            FETCH_CRACK | DAZE_TAPOUT => k == "Priority",
            FOW_PITCH_FOW => k == "ExileCost",
            DELVE_SKIP_SPELL => k == "ChooseGraveyardCard",
            VOLCANIC_FETCH => k == "SearchLibrary",
            KEEP_ONE_LANDER => k == "Mulligan",
            _ => false,
        };
        assert!(kinds.keys().all(|(t, k)| allowed(*t, k)), "{kinds:?}");
    }
}
