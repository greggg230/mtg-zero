//! Library searches, looking at and arranging the top of a library (CR
//! 701.19, 701.18, 701.42, 701.20, 401.2) and what each player knows
//! afterwards (ENGINE.md 10).

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, KEY_REVEAL};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const LIB: &[&str] = &["Forest", "Island", "Swamp", "Island", "Lightning Bolt", "Counterspell", "Island", "Plains"];

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, LIB).library(1, &["Island"; 8])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

/// Cast from hand, answering forced decisions, until the next real one.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

/// Pass priority until something other than priority is asked or the stack
/// is empty; returns the chance keys.
fn resolve(g: &mut Game) -> Vec<u64> {
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty())
}

fn reveals(keys: &[u64]) -> Vec<u16> {
    keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).map(|&k| key_payload(k) as u16).collect()
}

/// The definitions offered by a search decision.
fn offered_defs(g: &Game) -> Vec<u16> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::SearchLibrary);
    d.options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Def(x) => Some(x),
            _ => None,
        })
        .collect()
}

#[test]
fn cr_701_19c_a_search_for_a_card_must_find_one() {
    // Demonic Tutor: "a card". Every distinct card of the library is offered
    // once, by definition, and finding nothing is not an option.
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Demonic Tutor"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Demonic Tutor");
    resolve(&mut g);
    let mut want: Vec<u16> = ["Forest", "Island", "Swamp", "Lightning Bolt", "Counterspell", "Plains"].map(def).to_vec();
    want.sort();
    assert_eq!(offered_defs(&g), want);
    assert!(!g.has_option(Choice::Pass));
    // Slots are the first index of each definition in the library sorted
    // by definition.
    let lib = g.library_sorted(0);
    for o in &g.pending_decision().unwrap().options {
        let Choice::Def(d) = o.choice else { unreachable!() };
        let first = lib.iter().position(|&c| g.obj(c).def == d).unwrap() as u16;
        assert_eq!(o.action, act::LIBRARY + first);
    }
    let keys = g.choose(Choice::Def(def("Lightning Bolt")));
    // Not revealed: no key names it.
    assert!(reveals(&keys).is_empty(), "{keys:?}");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.knows(0, bolt) && !g.knows(1, bolt));
    assert_eq!(g.player(0).library.len(), 7);
    // The library was shuffled: nobody knows any of its cards.
    assert!(g.player(0).library.iter().all(|&c| !g.knows(0, c) && !g.knows(1, c)));
    // P1 does not know which card P0 took.
    let moved = (0..40).any(|s| {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.obj(bolt).def != def("Lightning Bolt")
    });
    assert!(moved);
    g.check_invariants().unwrap();
}

#[test]
fn cr_701_19c_an_empty_library_finds_nothing() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Demonic Tutor"])
        .library(1, &["Island"; 4])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Demonic Tutor");
    resolve(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    assert_eq!(g.options_desc(), vec!["0=pass".to_string()]);
    g.pass();
    assert!(g.stack().is_empty());
}

#[test]
fn cr_701_19b_a_search_for_a_stated_quality_may_fail_and_reveals_on_top() {
    // Mystical Tutor: an instant or sorcery card, reveal it, then shuffle
    // and put it on top.
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Mystical Tutor"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Mystical Tutor");
    resolve(&mut g);
    let mut want = vec![def("Lightning Bolt"), def("Counterspell")];
    want.sort();
    assert_eq!(offered_defs(&g), want);
    assert!(g.has_option(Choice::Pass), "701.19b: may find nothing");
    let keys = g.choose(Choice::Def(def("Counterspell")));
    assert_eq!(reveals(&keys), vec![def("Counterspell")]);
    assert!(keys.iter().all(|&k| key_kind(k) != KEY_REVEAL || key_player(k) == 0));
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Counterspell");
    assert!(g.knows(0, top) && g.knows(1, top));
    for v in 0..2 {
        for s in 0..10 {
            let mut d = g.clone();
            d.determinize(v, &mut Rng::new(s));
            assert_eq!(d.obj(top).def, def("Counterspell"), "a revealed top card stays for viewer {v}");
        }
    }
    // Declining: nothing moves, the library is still shuffled.
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Mystical Tutor"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Mystical Tutor");
    resolve(&mut g);
    g.pass();
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.player(0).library.len(), 8);
    assert_eq!(g.count_in(0, "Mystical Tutor", Zone::Graveyard), 1);
}

#[test]
fn vampiric_tutor_puts_an_unrevealed_card_on_top_known_to_its_owner() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Vampiric Tutor"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Vampiric Tutor");
    resolve(&mut g);
    let keys = g.choose(Choice::Def(def("Plains")));
    assert!(reveals(&keys).is_empty());
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Plains");
    assert!(g.knows(0, top) && !g.knows(1, top));
    assert_eq!(g.life(0), 18);
    for s in 0..10 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        assert_eq!(d.obj(top).def, def("Plains"), "its owner knows the top card");
    }
    let moved = (0..40).any(|s| {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.obj(top).def != def("Plains")
    });
    assert!(moved, "the opponent does not");
}

#[test]
fn entomb_puts_the_card_into_the_graveyard_revealed() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Entomb"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Entomb");
    resolve(&mut g);
    assert!(!g.has_option(Choice::Pass));
    let keys = g.choose(Choice::Def(def("Counterspell")));
    assert_eq!(reveals(&keys), vec![def("Counterspell")]);
    assert_eq!(g.count_in(0, "Counterspell", Zone::Graveyard), 1);
}

#[test]
fn a_fetch_land_finds_a_land_with_one_of_its_types() {
    // Polluted Delta: {T}, pay 1 life, sacrifice: an Island or Swamp card
    // onto the battlefield (untapped), then shuffle. Underground Sea and
    // Watery Grave qualify; a shock land asks for its life at once.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Polluted Delta"])
        .library(0, &["Watery Grave", "Underground Sea", "Forest", "Plains", "Island", "Swamp"])
        .library(1, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let delta = g.card(0, "Polluted Delta", Zone::Battlefield);
    g.choose(Choice::Ability(delta, AbilitySlot::Activated(0)));
    resolve(&mut g);
    assert_eq!(g.life(0), 19);
    assert_eq!(g.count_in(0, "Polluted Delta", Zone::Graveyard), 1);
    let mut want: Vec<u16> = ["Watery Grave", "Underground Sea", "Island", "Swamp"].map(def).to_vec();
    want.sort();
    assert_eq!(offered_defs(&g), want);
    assert!(g.has_option(Choice::Pass));
    g.choose(Choice::Def(def("Watery Grave")));
    // 614.12: the shock land's choice comes right away.
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)));
    g.choose(Choice::Yes);
    let grave = g.card(0, "Watery Grave", Zone::Battlefield);
    assert!(!g.obj(grave).tapped);
    assert_eq!(g.life(0), 17);
    assert!(at_priority(&g) && g.stack().is_empty());
}

#[test]
fn path_to_exile_lets_the_creatures_controller_search_for_a_tapped_basic() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Plains"])
        .hand(0, &["Path to Exile"])
        .battlefield(1, &["Test Bear"])
        .library(0, &["Island"; 4])
        .library(1, &["Forest", "Underground Sea", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let path = g.card(0, "Path to Exile", Zone::Hand);
    g.choose(Choice::Cast(path));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    resolve(&mut g);
    // The exiled creature's controller decides ("may"), then searches.
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(1)));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1);
    g.choose(Choice::Yes);
    let mut want = vec![def("Forest"), def("Swamp")];
    want.sort();
    assert_eq!(offered_defs(&g), want, "basic lands only");
    g.choose(Choice::Def(def("Swamp")));
    let swamp = g.card(1, "Swamp", Zone::Battlefield);
    assert!(g.obj(swamp).tapped);
    // Declining: no search, no shuffle.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Plains"])
        .hand(0, &["Path to Exile"])
        .battlefield(1, &["Test Bear"])
        .library(0, &["Island"; 4])
        .library(1, &["Forest", "Underground Sea", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let before = g.player(1).library.clone();
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let path = g.card(0, "Path to Exile", Zone::Hand);
    g.choose(Choice::Cast(path));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    resolve(&mut g);
    g.pass();
    assert_eq!(g.player(1).library, before);
}

#[test]
fn islandcycling_searches_for_an_island_card_and_reveals_it() {
    // Lórien Revealed: islandcycling {1}. Underground Sea is an Island card.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lórien Revealed"])
        .library(0, &["Underground Sea", "Forest", "Swamp"])
        .library(1, &["Island"; 8])
        .at(Step::Upkeep, 0)
        .start();
    let lr = g.card(0, "Lórien Revealed", Zone::Hand);
    g.choose(Choice::Ability(lr, AbilitySlot::Hand(0)));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Lórien Revealed", Zone::Graveyard), 1);
    resolve(&mut g);
    assert_eq!(offered_defs(&g), vec![def("Underground Sea")]);
    let keys = g.choose(Choice::Def(def("Underground Sea")));
    assert_eq!(reveals(&keys), vec![def("Underground Sea")]);
    let sea = g.card(0, "Underground Sea", Zone::Hand);
    assert!(g.knows(1, sea));
}

#[test]
fn a_search_decision_is_the_same_in_every_world_its_searcher_imagines() {
    // While P0 searches, P0's determinize permutes the library's positions
    // only; the options (definitions at canonical slots) stay. P1's
    // determinize resamples P0's library and refreshes the decision.
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Demonic Tutor", "Plains"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Demonic Tutor");
    resolve(&mut g);
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    let opts = g.pending_decision().unwrap().clone();
    for s in 0..20 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        assert_eq!(d.pending_decision().unwrap(), &opts);
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_invariants().unwrap();
        let mut l2 = Vec::new();
        d.legal_actions(&mut l2);
        let a = l2[(s as usize) % l2.len()];
        let mut keys = Vec::new();
        d.apply(a, &mut keys).unwrap();
        d.check_invariants().unwrap();
    }
}

#[test]
fn cr_701_20_a_shuffle_keeps_what_was_known_in_the_library_but_not_where() {
    // Mystical Tutor reveals Counterspell on top; then a fetch land shuffles
    // P0's library. P1 still knows Counterspell is in that library (never in
    // P0's hand), but no longer where.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Flooded Strand"])
        .hand(0, &["Mystical Tutor", "Test Bear", "Mountain"])
        .library(0, &["Counterspell", "Forest", "Plains", "Swamp", "Island", "Lightning Bolt", "Forest", "Forest"])
        .library(1, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Mystical Tutor");
    resolve(&mut g);
    g.choose(Choice::Def(def("Counterspell")));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let cs = *g.player(0).library.last().unwrap();
    assert!(g.knows(1, cs));
    let strand = g.card(0, "Flooded Strand", Zone::Battlefield);
    g.choose(Choice::Ability(strand, AbilitySlot::Activated(0)));
    resolve(&mut g);
    g.choose(Choice::Def(def("Plains")));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert!(!g.knows(1, cs), "positions are lost");
    let k = g.clouds().iter().find(|k| k.viewer == 1).expect("a cloud");
    assert_eq!((k.owner, k.hand, k.library, k.defs.clone()), (0, false, true, vec![def("Counterspell")]));
    let mut places = std::collections::BTreeSet::new();
    for s in 0..100 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_invariants().unwrap();
        let at: Vec<CardIdx> = d
            .player(0)
            .hand
            .iter()
            .chain(d.player(0).library.iter())
            .copied()
            .filter(|&c| d.obj(c).def == def("Counterspell"))
            .collect();
        assert!(at.iter().any(|c| d.player(0).library.contains(c)), "seed {s}: still in the library");
        assert!(at.iter().all(|c| !d.player(0).hand.contains(c)), "seed {s}: never in P0's hand");
        for c in at {
            places.insert(d.player(0).library.iter().position(|&x| x == c));
        }
    }
    assert!(places.len() > 3, "anywhere in the library: {places:?}");
}
