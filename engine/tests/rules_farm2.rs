//! ENGINE.md 71: the fuzz farm's failures of 2026-10-29 (main b78c757) and
//! three the belief-weighted deal's fuzz found (ENGINE.md 69.8). Each test
//! builds, on real cube cards, the state one of them reached, and fails on
//! main d0b62b1 unless said otherwise:
//!
//! * 71.1 same keys, different encodings of the hand's owner (the hand
//!   block's "known to the opponent"): Urza's Bauble's look at a random hand
//!   card was keyed for the looker only, though its owner sees which card;
//! * 71.2 same keys, different views: a mark naming another object (Animate
//!   Dead's creature, a Nadu's granted trigger) hashed into the view by its
//!   slot; and a second Nadu counted the first one's triggers (the mark
//!   named the granting permanent by its incarnation number alone);
//! * 71.3 determinize changed the encoding of the other player: a "you may
//!   cast it" question skipped when the card could not be cast, its
//!   castability counting the caster's hidden hand (Bilbo, Chandra);
//! * 71.4 a face choice did not pin the caster's hand mana source (a world
//!   for the other player dealt the Simian Spirit Guide away);
//! * 71.5 teamwork's cost offered, then impossible (Orcish Lumberjack's
//!   sacrifices dropped whenever any creature was tapped).

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_LOOK};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use engine::az::NUM_FEATURES;

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    })
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn encoding(g: &Game, viewer: PlayerId) -> Vec<f32> {
    let mut x = vec![0.0f32; NUM_FEATURES];
    g.encode_features(viewer, &mut x);
    x
}

/// The pending decision's action whose description names `what`.
fn action_named(g: &Game, what: &str) -> u16 {
    let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
    d.options
        .iter()
        .map(|o| o.action)
        .find(|&a| g.describe_action(a).unwrap_or_default().contains(what))
        .unwrap_or_else(|| panic!("no option naming {what:?}: {:?}\n{}", g.options_desc(), log(g)))
}

/// Pay a pending mana payment with the first source offered each time.
fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Pass priority (answering other decisions with `answer`, else the first
/// option) until the stack is empty and a player has priority.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
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

/// Apply `a` in both worlds; their decisions must agree first (the same
/// node of a search: decider and legal actions). Returns both worlds' keys.
fn step_both(a: &mut Game, b: &mut Game, act: u16) -> (Vec<u64>, Vec<u64>) {
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    a.legal_actions(&mut la);
    b.legal_actions(&mut lb);
    assert_eq!((a.to_move(), &la), (b.to_move(), &lb), "the worlds' decisions differ\n{}\n----\n{}", log(a), log(b));
    let (mut ka, mut kb) = (Vec::new(), Vec::new());
    a.apply(act, &mut ka).unwrap();
    b.apply(act, &mut kb).unwrap();
    (ka, kb)
}

// ---- 71.1 Urza's Bauble: the hand's owner sees which card was looked at ----------

/// "{T}, Sacrifice Urza's Bauble: Look at a card at random in target
/// player's hand. You draw a card at the beginning of the next turn's
/// upkeep." Its owner sees which of their cards was seen (it is remembered:
/// ENGINE.md 23.7, and their encoding's hand block shows it), so in a
/// search of theirs the look's key must name it: before, it was the
/// looker's private key, masked for the owner, and two worlds of the owner
/// (each with its own generator) in which different cards were seen shared
/// the key while the owner's encodings differed (the farm's a549df8eb756,
/// 37acdaaec70a, 795476f18604; the Delver deck plays the Bauble).
#[test]
fn urzas_bauble_keys_the_card_seen_for_the_hands_owner_too() {
    let mut g = scen()
        .battlefield(0, &["Urza's Bauble"])
        .hand(1, &["Lightning Bolt", "Counterspell", "Brainstorm", "Ponder"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let ub = bf(&g, 0, "Urza's Bauble");
    g.choose(act(ub, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1, "the Bauble's ability on the stack\n{}", log(&g));
    let mut seen: Vec<(Vec<u64>, Vec<f32>)> = Vec::new();
    for s in 0..24u64 {
        // A world of the hand's owner (its own generator), resolved.
        let mut r = g.clone();
        r.determinize(1, &mut Rng::new(1000 + s));
        let mut keys = Vec::new();
        for _ in 0..20 {
            if r.stack().is_empty() && at_priority(&r) {
                break;
            }
            let a = if at_priority(&r) { engine::decision::act::PASS } else { r.pending_decision().unwrap().options[0].action };
            r.apply(a, &mut keys).unwrap();
        }
        let looks: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_LOOK).collect();
        assert_eq!(looks.len(), 1, "one look: {keys:?}\n{}", log(&r));
        assert_eq!(key_player(looks[0]), 0, "the looker's key");
        assert_ne!(key_payload(looks[0]), HIDDEN, "the owner sees which card: the key names it");
        let known: Vec<CardIdx> = r.player(1).hand.iter().copied().filter(|&c| r.knows(0, c)).collect();
        assert_eq!(known.len(), 1);
        assert_eq!(key_payload(looks[0]), r.obj(known[0]).def as u64, "the key names the card seen");
        seen.push((keys, encoding(&r, 1)));
    }
    // The owner's information set: equal keys, equal encodings (and the
    // test sees more than one card seen).
    for i in 0..seen.len() {
        for j in i + 1..seen.len() {
            if seen[i].0 == seen[j].0 {
                assert!(seen[i].1 == seen[j].1, "same keys, different encodings of the hand's owner");
            }
        }
    }
    let mut distinct: Vec<&Vec<u64>> = seen.iter().map(|x| &x.0).collect();
    distinct.sort();
    distinct.dedup();
    assert!(distinct.len() > 1, "the worlds saw different cards");
}

// ---- 71.2 marks naming another object, in the view; a second Nadu ----------------

/// Animate Dead's creature came from a library searched by Entomb: which
/// slot it is differs between the worlds of the other player (the library's
/// identities are dealt again), and the Aura's mark (`Mark::Reanimated`)
/// named it by its slot in the view (the belief fuzz's seed 15683: Grief
/// from a resampled hand). Played with the same actions, the worlds look the
/// same to that player.
#[test]
fn an_animated_creature_from_a_searched_library_looks_the_same_in_every_world() {
    let g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand_known(0, &["Entomb", "Animate Dead"])
        .library(0, &["Island", "Island", "Island", "Island", "Baleful Strix", "Island", "Island", "Island", "Island", "Island"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let mut differed_slot = false;
    for s in 0..12u64 {
        let mut a = g.clone();
        a.determinize(1, &mut Rng::new(7 + s));
        let mut b = g.clone();
        b.determinize(1, &mut Rng::new((7 + s) ^ 0x5EED));
        let mut keys = (Vec::new(), Vec::new());
        let mut go = |a: &mut Game, b: &mut Game, what: &str| {
            let act = if what.is_empty() { a.pending_decision().unwrap().options[0].action } else { action_named(a, what) };
            let (ka, kb) = step_both(a, b, act);
            keys.0.extend(ka);
            keys.1.extend(kb);
        };
        go(&mut a, &mut b, "cast Entomb");
        while a.kind() != Some(DecisionKind::Priority) {
            go(&mut a, &mut b, "");
        }
        // Resolve Entomb: pass, pass; then its search.
        for _ in 0..40 {
            if a.stack().is_empty() && at_priority(&a) {
                break;
            }
            match a.kind() {
                Some(DecisionKind::Priority) => go(&mut a, &mut b, "pass"),
                _ if a.options_desc().iter().any(|o| o.contains("Baleful Strix")) => go(&mut a, &mut b, "Baleful Strix"),
                _ => go(&mut a, &mut b, ""),
            }
        }
        let strix = |w: &Game| w.card(0, "Baleful Strix", Zone::Graveyard);
        differed_slot |= strix(&a) != strix(&b);
        go(&mut a, &mut b, "cast Animate Dead");
        for _ in 0..40 {
            if a.stack().is_empty() && at_priority(&a) {
                break;
            }
            match a.kind() {
                Some(DecisionKind::Priority) => go(&mut a, &mut b, "pass"),
                Some(DecisionKind::Target) => go(&mut a, &mut b, "Baleful Strix"),
                _ => go(&mut a, &mut b, ""),
            }
        }
        assert_eq!(a.count_in(0, "Baleful Strix", Zone::Battlefield), 1, "reanimated\n{}", log(&a));
        assert_eq!(keys.0, keys.1, "nothing told the worlds apart");
        assert_eq!(a.view_of(1), b.view_of(1), "same keys, different views of the other player\n{}\n----\n{}", log(&a), log(&b));
    }
    assert!(differed_slot, "the worlds' creatures were different slots");
}

/// Nadu cast from a hand the opponent doesn't know: in the opponent's
/// worlds its identity is on one slot or another (one drawn this turn, one
/// held since), whose incarnation numbers differ, and the mark its granted
/// trigger leaves (`Mark::Triggered`) named it by that number in the view
/// (the farm's 32e7e5be2b2d, interactions game 10588610: a Nadu found by
/// Green Sun's Zenith). The worlds, played alike, look the same to the
/// opponent.
#[test]
fn a_nadus_granted_trigger_looks_the_same_in_every_world() {
    let g = scen()
        .battlefield(0, &["Tropical Island", "Tropical Island", "Tropical Island"])
        .hand(0, &["Nadu, Winged Wisdom", "Island"])
        .library(0, &["Island", "Island", "Island", "Island"])
        .battlefield(1, &["Test Tapper"])
        .library(1, &["Island"; 10])
        .at(Step::Upkeep, 0)
        .start();
    let mut g = g;
    until_kind(&mut g, DecisionKind::Priority);
    while g.turn().step != Step::Main1 {
        g.pass();
        until_kind(&mut g, DecisionKind::Priority);
    }
    assert_eq!(g.player(0).hand.len(), 3, "drew\n{}", log(&g));
    let (mut compared, mut differed) = (0, false);
    for s in 0..60u64 {
        let mut a = g.clone();
        a.determinize(1, &mut Rng::new(500 + s));
        let mut b = g.clone();
        b.determinize(1, &mut Rng::new((500 + s) ^ 0x5EED));
        let nadu = |w: &Game| w.find_card(0, "Nadu, Winged Wisdom", Zone::Hand);
        let (Some(na), Some(nb)) = (nadu(&a), nadu(&b)) else { continue };
        let mut keys = (Vec::new(), Vec::new());
        let mut go = |a: &mut Game, b: &mut Game, what: &str| {
            let act = if what.is_empty() { a.pending_decision().unwrap().options[0].action } else { action_named(a, what) };
            let (ka, kb) = step_both(a, b, act);
            keys.0.extend(ka);
            keys.1.extend(kb);
        };
        go(&mut a, &mut b, "cast Nadu");
        while !(a.stack().is_empty() && at_priority(&a)) {
            if at_priority(&a) {
                go(&mut a, &mut b, "pass");
            } else {
                go(&mut a, &mut b, "");
            }
        }
        differed |= a.obj(na).inc != b.obj(nb).inc;
        // P0 passes; P1 pings Nadu; it resolves.
        go(&mut a, &mut b, "pass");
        go(&mut a, &mut b, "Test Tapper");
        go(&mut a, &mut b, "Nadu");
        while !(a.stack().is_empty() && at_priority(&a)) {
            if at_priority(&a) {
                go(&mut a, &mut b, "pass");
            } else {
                go(&mut a, &mut b, "");
            }
        }
        if keys.0 != keys.1 {
            continue;
        }
        assert_eq!(a.view_of(1), b.view_of(1), "same keys, different views of the other player\n{}\n----\n{}", log(&a), log(&b));
        compared += 1;
    }
    assert!(compared > 0 && differed, "worlds compared ({compared}) with Nadus of different incarnation numbers ({differed})");
}

/// Nadu's "this ability triggers only twice each turn" counts per granting
/// object (a Nadu that leaves and comes back grants a new ability: CR 400.7,
/// ruling 96e9a886). The count named the granting Nadu by its incarnation
/// number alone, which another Nadu that came the same way (hand, stack,
/// battlefield) shares: the second Nadu's grant found the first one's two
/// triggers and never triggered (ENGINE.md 71.2).
#[test]
fn a_second_nadu_grants_a_new_twice_each_turn_count_cr400_7() {
    let mut g = scen()
        .battlefield(0, &["Tropical Island", "Tropical Island", "Tropical Island", "Tropical Island", "Tropical Island", "Tropical Island", "Test Giant"])
        .hand(0, &["Nadu, Winged Wisdom", "Nadu, Winged Wisdom"])
        .library(0, &["Island"; 12])
        .battlefield(1, &["Test Tapper", "Test Tapper", "Test Tapper"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 0, "Test Giant");
    let tappers: Vec<CardIdx> = g.permanents_of(1).collect();
    let cast_nadu = |g: &mut Game| {
        let c = g.card(0, "Nadu, Winged Wisdom", Zone::Hand);
        g.choose(Choice::Cast(c));
        pay(g);
        // (The legend rule: the new Nadu stays.)
        resolve_with(g, |g| g.pending_decision().unwrap().options.iter().find(|o| o.choice == Choice::Card(c)).map(|o| o.choice));
        c
    };
    // P1 pings the Giant with a tapper at its next priority.
    let ping = |g: &mut Game, t: CardIdx| {
        g.pass();
        g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
        g.choose(act(t, 0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(giant));
        resolve_with(g, |_| None);
    };
    let first = cast_nadu(&mut g);
    let inc = g.obj(first).inc;
    let lib = g.player(0).library.len();
    ping(&mut g, tappers[0]);
    ping(&mut g, tappers[1]);
    assert_eq!(g.player(0).library.len(), lib - 2, "two reveals\n{}", log(&g));
    let second = cast_nadu(&mut g);
    assert_eq!(g.obj(second).zone, Zone::Battlefield);
    assert_ne!(g.obj(first).zone, Zone::Battlefield, "the legend rule");
    assert_eq!(g.obj(second).inc, inc, "the two Nadus' incarnation numbers agree (the case)");
    let lib = g.player(0).library.len();
    ping(&mut g, tappers[2]);
    assert_eq!(g.player(0).library.len(), lib - 1, "the new Nadu's grant triggers\n{}", log(&g));
}

// ---- 71.3 "you may cast it": asked even when it can't be cast ---------------------

/// A world `determinize` deals for `viewer` keeps the pending decision of
/// the other player (its kind and decider) and `viewer`'s encoding; returns
/// how many of the worlds lacked `card` in the decider's hand.
fn refresh_keeps_the_question(g: &Game, viewer: PlayerId, card: &str) -> usize {
    let d = g.pending_decision().unwrap();
    let x = encoding(g, viewer);
    let mut without = 0;
    for s in 0..40u64 {
        let mut w = g.clone();
        w.determinize(viewer, &mut Rng::new(31 + s));
        let e = w.pending_decision().unwrap();
        assert_eq!((e.player, e.kind), (d.player, d.kind), "the world asks something else\n{}", log(&w));
        assert!(encoding(&w, viewer) == x, "determinize changed the encoding of P{viewer}");
        without += w.find_card(d.player, card, Zone::Hand).is_none() as usize;
    }
    without
}

/// Drive `g` (answering with the first option, passing priority) until a
/// decision of `kind`.
fn until_kind(g: &mut Game, kind: DecisionKind) {
    for _ in 0..60 {
        if g.kind() == Some(kind) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap_or_else(|| panic!("no decision
{}", log(g))).options[0].choice;
            g.choose(c);
        }
    }
    panic!("no {kind:?}
{}", log(g));
}

/// Bilbo attacks with Lightning Bolt in the graveyard, no land and `hand`.
fn bilbo_attacks(hand: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, &["Bilbo, Thief in the Night"])
        .graveyard(0, &["Lightning Bolt"])
        .hand(0, hand)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::BeginCombat, 0)
        .start();
    let bilbo = bf(&g, 0, "Bilbo, Thief in the Night");
    until_kind(&mut g, DecisionKind::DeclareAttackers);
    g.choose(Choice::Card(bilbo));
    until_kind(&mut g, DecisionKind::ChooseGraveyardCard);
    g
}

/// Bilbo's attack trigger: "you may cast an artifact, instant or sorcery
/// spell from your graveyard". Lightning Bolt there, no land, a Simian
/// Spirit Guide in hand: castable, asked. A world dealt for the opponent may
/// deal the Guide away, and the question was skipped there (the farm's
/// 7f0953e0e769, drafted match 10278736): its refresh must ask it too, with
/// "pass" only.
#[test]
fn bilbos_graveyard_cast_is_asked_whether_or_not_the_hand_can_pay() {
    let g = bilbo_attacks(&["Simian Spirit Guide"]);
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(g.has_option(Choice::Card(bolt)), "the Guide pays for the Bolt: {:?}", g.options_desc());
    let without = refresh_keeps_the_question(&g, 1, "Simian Spirit Guide");
    assert!(without > 0, "some world dealt the Guide away");
    // With no way to pay, the question still comes (only "pass").
    let h = bilbo_attacks(&["Island"]);
    assert_eq!(h.pending_decision().unwrap().options.iter().map(|o| o.choice).collect::<Vec<_>>(), vec![Choice::Pass]);
}

/// Chandra, Torch of Defiance's +1: "Exile the top card of your library.
/// You may cast that card. If you don't, Chandra deals 2 damage to each
/// opponent." The same with a Lightning Bolt exiled and the Guide in hand
/// (f10db98's 7b7fc0b85e1b, interactions match 10319082).
#[test]
fn chandras_exiled_card_is_asked_whether_or_not_the_hand_can_pay() {
    let mut g = scen()
        .battlefield(0, &["Chandra, Torch of Defiance"])
        .hand(0, &["Simian Spirit Guide"])
        .library(0, &["Lightning Bolt", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let chandra = bf(&g, 0, "Chandra, Torch of Defiance");
    g.choose(act(chandra, 0));
    for _ in 0..40 {
        if g.kind() == Some(DecisionKind::YesNo) {
            break;
        }
        if at_priority(&g) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "the cast offered\n{}", log(&g));
    assert!(g.has_option(Choice::Yes), "the Guide pays for the Bolt");
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Exile), 1);
    let without = refresh_keeps_the_question(&g, 1, "Simian Spirit Guide");
    assert!(without > 0, "some world dealt the Guide away");
    // Said "no" (no way to pay): Chandra's 2 damage follows.
    let mut w = g.clone();
    w.choose(Choice::Pass);
    resolve_with(&mut w, |_| None);
    assert_eq!(w.life(1), 18);
}

// ---- 71.4 a face choice pins the caster's hand mana sources ----------------------

/// "Which face?" of Brazen Borrower: both castable only with the Simian
/// Spirit Guide in hand ({1}{U}{U} from two Islands and the Guide). The cast
/// has begun, so a world dealt for the opponent keeps the Guide (pinned, as
/// a payment pins it: ENGINE.md 10), and either face can then be paid (the
/// belief fuzz's seed 7193 under the `fuzz` profile: "PayMana: no way to
/// pay").
#[test]
fn a_face_choice_pins_the_casters_hand_mana_source() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Brazen Borrower // Petty Theft", "Simian Spirit Guide"])
        .library(0, &["Island"; 10])
        .battlefield(1, &["Test Bear"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Hand);
    let guide = g.card(0, "Simian Spirit Guide", Zone::Hand);
    let faces = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Faces(x, _) if *x == bb));
    let faces = faces.unwrap_or_else(|| panic!("both faces castable: {:?}", g.options_desc()));
    g.choose(faces);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseFace), "{}", log(&g));
    assert!(g.pinned_for(1).contains(&guide), "the Guide is pinned for the opponent's worlds");
    for s in 0..40u64 {
        for face in [0u8, 1] {
            let mut w = g.clone();
            w.determinize(1, &mut Rng::new(77 + s));
            assert_eq!(w.find_card(0, "Simian Spirit Guide", Zone::Hand), Some(guide), "the pinned Guide stays");
            w.choose(Choice::Number(face));
            // Every option of what follows completes (targets, then the mana).
            for _ in 0..40 {
                if at_priority(&w) {
                    break;
                }
                let c = w.pending_decision().unwrap().options[0].choice;
                w.choose(c);
            }
            assert!(at_priority(&w), "face {face} cast\n{}", log(&w));
        }
    }
}

// ---- 71.5 teamwork with a Lumberjack -------------------------------------------

/// We Say Thee Nay!'s teamwork ("tap creatures with total power 2"): the
/// cost check found it payable with Test Bear tapped and the mana from
/// Gleemox and Orcish Lumberjack (sacrificing a Forest tapped earlier), but
/// the creature decision dropped every Lumberjack sacrifice as soon as any
/// creature was tapped (`classes_without`), so it had no option ("ChoosePermanent
/// with no options": the belief fuzz's seed 14537, a world of P0 with Gleemox,
/// Mishra's Workshop, a Lumberjack, Enduring Innocence and Aragorn).
#[test]
fn teamwork_keeps_a_lumberjacks_mana_when_another_creature_is_tapped() {
    let mut g = scen()
        .battlefield(0, &["Gleemox", "Orcish Lumberjack", "Test Bear", "Forest", "Island"])
        .hand(0, &["We Say Thee Nay!", "Test Reacher"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let forest = bf(&g, 0, "Forest");
    let island = bf(&g, 0, "Island");
    // Test Reacher ({1}{G}) paid with the Forest and the Island: the Forest
    // is tapped, still there to sacrifice.
    let reacher = g.card(0, "Test Reacher", Zone::Hand);
    g.choose(Choice::Cast(reacher));
    for src in [forest, island] {
        assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
        g.choose(act(src, 0));
    }
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    assert!(at_priority(&g) && g.stack().len() == 1, "the Reacher on the stack, P0 to act\n{}", log(&g));
    assert!(g.obj(forest).tapped);
    let nay = g.card(0, "We Say Thee Nay!", Zone::Hand);
    g.choose(Choice::Cast(nay));
    // The teamwork option (cost option 1), then the target, then the taps.
    for _ in 0..20 {
        match g.kind() {
            Some(DecisionKind::ChooseCost) => {
                assert!(g.has_option(Choice::Number(1)), "teamwork offered: {:?}", g.options_desc());
                g.choose(Choice::Number(1));
            }
            Some(DecisionKind::Target) => {
                let c = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass).unwrap();
                g.choose(c);
            }
            Some(DecisionKind::ChoosePermanent) => break,
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "teamwork's creatures asked\n{}", log(&g));
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_option(Choice::Card(bear)), "Test Bear can be tapped, the Lumberjack's mana left: {:?}", g.options_desc());
    g.choose(Choice::Card(bear));
    for _ in 0..20 {
        if at_priority(&g) {
            break;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert!(at_priority(&g) && g.stack().len() == 2, "We Say Thee Nay! cast\n{}", log(&g));
    assert!(g.obj(bear).tapped);
}
