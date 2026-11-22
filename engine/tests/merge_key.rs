//! Near-tie branches' merge key and alternative check (ENGINE.md 227, `az/merge_key.rs`; zero-core docs/search.md
//! 2.11): two lines from one built Delver-mirror position (`build`, ENGINE.md 95), played by option label, keyed at
//! their decisions - the branch's view of its game. Equal keys must mean equivalent states (a false merge labels a
//! branch's rows with another future's outcome); a missed merge only costs compute.
//!
//! The cost study (`merge_key_cost`, ignored: `--ignored --nocapture`) times the key over random and heuristic games
//! of the Delver mirror and its best-of-three dealt preset.

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::game::Step;
use engine::ids::Zone;
use engine::rng::Rng;
use engine::testkit::Scenario;
use serde_json::{json, Value};
use zero_search::game::SearchGame;

fn delver() -> Vec<String> {
    Decklist::izzet_delver().main_names()
}

fn delver_spec(extra: Value) -> MtgSpec {
    let d = delver();
    let mut spec = json!({"max_turns": 60, "constructed": true, "decks": {"p0": d, "p1": d}});
    for (k, v) in extra.as_object().unwrap() {
        spec[k] = v.clone();
    }
    MtgSpec::parse(&spec.to_string()).unwrap()
}

/// The Delver mirror (best-of-one) starting at the position `desc` describes (P0's view).
fn built(desc: Value) -> MtgAz {
    MtgAz::new_game(&delver_spec(json!({"build": desc})), 2, 5).unwrap()
}

/// The pending decision's options: (action, label, card id).
fn options(g: &MtgAz) -> Vec<(u16, String, Option<u32>)> {
    let s: Value = serde_json::from_str(&g.export_state()).unwrap();
    s["decision"]["options"].as_array().map_or(Vec::new(), |o| {
        o.iter().map(|x| (x["a"].as_u64().unwrap() as u16, x["t"].as_str().unwrap().to_string(), x["c"].as_u64().map(|c| c as u32))).collect()
    })
}

fn kind(g: &MtgAz) -> String {
    let s: Value = serde_json::from_str(&g.export_state()).unwrap();
    s["decision"]["kind"].as_str().unwrap_or("").to_string()
}

/// The action of the first option labelled `label`.
fn action(g: &MtgAz, label: &str) -> u16 {
    let o = options(g);
    o.iter().find(|x| x.1 == label).unwrap_or_else(|| panic!("{label:?} is not an option of {} {o:?}", kind(g))).0
}

fn act(g: &mut MtgAz, label: &str) {
    let a = action(g, label);
    g.step(a, &mut Vec::new()).unwrap();
}

/// Answer single-option decisions and pass priority until `stop` (checked first) holds.
fn pass_until(g: &mut MtgAz, stop: impl Fn(&MtgAz) -> bool) {
    for _ in 0..1000 {
        if stop(g) {
            return;
        }
        let o = options(g);
        if o.len() == 1 {
            g.step(o[0].0, &mut Vec::new()).unwrap();
        } else if kind(g) == "Priority" {
            act(g, "Pass priority");
        } else if kind(g) == "PayMana" || kind(g) == "PayPool" {
            g.step(o[0].0, &mut Vec::new()).unwrap();
        } else {
            panic!("pass_until: a {} decision {o:?}", kind(g));
        }
    }
    panic!("pass_until: no progress");
}

/// P0's priority with an empty stack (the step `s`).
fn p0_free(s: Step) -> impl Fn(&MtgAz) -> bool {
    move |g: &MtgAz| g.g.stack().is_empty() && g.to_move() == Some(0) && kind(g) == "Priority" && g.g.turn().step == s
}

/// The next turn's first decision.
fn next_turn(g: &mut MtgAz) {
    let t = g.g.turn().number;
    pass_until(g, |g| g.g.turn().number == t + 1);
}

fn key(g: &MtgAz) -> Option<u128> {
    g.merge_key()
}

fn names(g: &MtgAz, cards: &[engine::ids::CardIdx]) -> Vec<String> {
    cards.iter().map(|&c| g.g.name(c).to_string()).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// Turn 3, P0 active in its first main phase with priority (or `step`): `hand` in hand, Volcanic Island, Island
/// and Polluted Delta untapped; P1 a Volcanic Island and six cards.
fn start(step: &str, hand: Vec<Value>) -> Value {
    let lib = 60 - 3 - hand.len();
    json!({"viewer": 0, "deal_seed": 11, "turn": 3, "active": 0, "starting": 0, "step": step, "priority": 0,
        "players": [
            {"life": 20, "hand": hand, "library_size": lib, "graveyard": [], "drawn": 1, "draw_step_draws": 1},
            {"life": 20, "hand_size": 6, "library_size": 53, "graveyard": []}],
        "battlefield": [
            {"name": "Volcanic Island", "owner": 0, "entered": 1},
            {"name": "Volcanic Island", "owner": 1, "entered": 2},
            {"name": "Island", "owner": 0, "entered": 3},
            {"name": "Polluted Delta", "owner": 0, "entered": 3}],
        "exile": [], "stack": []})
}

fn hand(cards: &[&str]) -> Vec<Value> {
    cards.iter().map(|s| json!(s)).collect()
}

fn bolt_face(g: &mut MtgAz) {
    act(g, "Cast Lightning Bolt");
    act(g, "Target P2");
    pass_until(g, p0_free(Step::Main1));
}

fn fetch_island(g: &mut MtgAz) {
    act(g, "Activate Polluted Delta");
    pass_until(g, |g| kind(g) == "SearchLibrary");
    act(g, "Find Volcanic Island");
    pass_until(g, p0_free(Step::Main1));
}

/// Brainstorm, putting back `back` (the first on top).
fn brainstorm(g: &mut MtgAz, back: [&str; 2], s: Step) {
    act(g, "Cast Brainstorm");
    pass_until(g, |g| kind(g) == "ChooseHandCard");
    act(g, &format!("Choose {}", back[0]));
    act(g, &format!("Choose {}", back[1]));
    pass_until(g, p0_free(s));
}

#[test]
fn bolt_then_fetch_merges_with_fetch_then_bolt_within_the_turn() {
    let base = built(start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    assert!(key(&base).is_some());
    let mut a = base.clone();
    bolt_face(&mut a);
    fetch_island(&mut a);
    let mut b = base.clone();
    b.g.reseed_rng(99);
    fetch_island(&mut b);
    bolt_face(&mut b);
    // different true library orders, nobody knows either
    let (la, lb) = (names(&a, &a.g.player(0).library), names(&b, &b.g.player(0).library));
    assert_ne!(la, lb, "the shuffles came out alike");
    assert_eq!(sorted(la), sorted(lb));
    // The same key within the turn: the fetch's loop record (which holds the view it was activated in, different
    // in the two orders) is left out, its source sacrificed; Bolt and the fetchland in the graveyard in either order.
    assert!(key(&a).is_some());
    assert_eq!(key(&a), key(&b), "Bolt-then-fetch and fetch-then-Bolt merge within the turn");
    assert!(a.g.check_dropped_loop_records().0 > 0 && a.g.check_dropped_loop_records().1 == 0);
    next_turn(&mut a);
    next_turn(&mut b);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn a_fetch_before_a_draw_does_not_merge_with_one_after() {
    let base = built(start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    let mut a = base.clone();
    a.g.reseed_rng(7);
    fetch_island(&mut a);
    brainstorm(&mut a, ["Daze", "Ponder"], Step::Main1);
    let mut b = base.clone();
    b.g.reseed_rng(7);
    brainstorm(&mut b, ["Daze", "Ponder"], Step::Main1);
    fetch_island(&mut b);
    next_turn(&mut a);
    next_turn(&mut b);
    let (ha, hb) = (sorted(names(&a, &a.g.player(0).hand)), sorted(names(&b, &b.g.player(0).hand)));
    assert_ne!(ha, hb, "the draws came out alike");
    assert!(key(&a).is_some() && key(&b).is_some());
    assert_ne!(key(&a), key(&b));
}

#[test]
fn brainstorm_in_the_draw_step_merges_with_brainstorm_in_the_main_phase_only_at_the_next_turn() {
    let base = built(start("draw", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    let mut a = base.clone();
    brainstorm(&mut a, ["Daze", "Ponder"], Step::Draw);
    pass_until(&mut a, p0_free(Step::Main1));
    let mut b = base.clone();
    pass_until(&mut b, p0_free(Step::Main1));
    brainstorm(&mut b, ["Daze", "Ponder"], Step::Main1);
    assert_eq!(names(&a, &a.g.player(0).library), names(&b, &b.g.player(0).library));
    // The draw-step Brainstorm's draws count as draw-step draws (Player::draw_step_draws, a network input) for the
    // turn: they merge only as the next turn begins.
    assert_ne!(key(&a), key(&b), "the turn's draw-step draws differ");
    next_turn(&mut a);
    next_turn(&mut b);
    assert!(key(&a).is_some());
    assert_eq!(key(&a), key(&b));
}

#[test]
fn a_brainstorm_put_back_then_a_shuffle_merges_only_with_the_same_knowledge() {
    // The put-back's order is forgotten by the shuffle: merge.
    let base = built(start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    let mut a = base.clone();
    brainstorm(&mut a, ["Daze", "Ponder"], Step::Main1);
    fetch_island(&mut a);
    let mut b = base.clone();
    b.g.reseed_rng(3);
    brainstorm(&mut b, ["Ponder", "Daze"], Step::Main1);
    fetch_island(&mut b);
    next_turn(&mut a);
    next_turn(&mut b);
    assert_ne!(names(&a, &a.g.player(0).library), names(&b, &b.g.player(0).library));
    assert!(key(&a).is_some());
    assert_eq!(key(&a), key(&b));
    // The opponent's Mishra's Bauble looks at P0's top card before the Brainstorm (it then knows a card of P0's
    // hand) or after it (it knows the Bolt put back, then shuffled away): the same cards everywhere, different
    // knowledge.
    let mut d = start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"]));
    d["players"][1]["library_size"] = json!(52);
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Mishra's Bauble", "owner": 1, "entered": 2}));
    let base = built(d);
    let bauble = |g: &mut MtgAz| {
        act(g, "Pass priority");
        act(g, "Activate Mishra's Bauble");
        act(g, "Target P1");
        pass_until(g, p0_free(Step::Main1));
    };
    let mut k = base.clone();
    brainstorm(&mut k, ["Lightning Bolt", "Ponder"], Step::Main1);
    bauble(&mut k);
    fetch_island(&mut k);
    next_turn(&mut k);
    let mut u = base.clone();
    bauble(&mut u);
    brainstorm(&mut u, ["Lightning Bolt", "Ponder"], Step::Main1);
    fetch_island(&mut u);
    next_turn(&mut u);
    assert_eq!(sorted(names(&k, &k.g.player(0).hand)), sorted(names(&u, &u.g.player(0).hand)));
    assert_eq!(sorted(names(&k, &k.g.player(0).library)), sorted(names(&u, &u.g.player(0).library)));
    assert!(key(&k).is_some() && key(&u).is_some());
    assert_ne!(key(&k), key(&u), "the opponent's knowledge differs");
}

/// A Scenario position (cards added in the order `order` says) in a Delver-mirror match's frame.
fn scenario(order: u8, p0_hand: &[&str], p0_library: &[&str]) -> MtgAz {
    let mut az = built(start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    let db = CardDb::constructed();
    let p1_library = ["Island", "Ponder", "Daze", "Volcanic Island"];
    let s = Scenario::new(db).at(Step::Main1, 0).turn_number(3);
    let s = match order {
        0 => s.hand(0, p0_hand).library(0, p0_library).library(1, &p1_library).battlefield(0, &["Volcanic Island", "Island"]).battlefield(1, &["Volcanic Island"]),
        _ => s.battlefield(0, &["Volcanic Island", "Island"]).battlefield(1, &["Volcanic Island"]).library(1, &p1_library).library(0, p0_library).hand(0, p0_hand),
    };
    az.g = s.hand(1, &["Force of Will", "Brainstorm"]).start();
    az
}

/// A Scenario position whose P0 graveyard holds `gy` (bottom first).
fn with_graveyard(gy: &[&str]) -> MtgAz {
    let mut az = built(start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"])));
    let s = Scenario::new(CardDb::constructed()).at(Step::Main1, 0).turn_number(3);
    let s = s.hand(0, &["Lightning Bolt"]).library(0, &["Island", "Ponder"]).library(1, &["Island", "Daze"]).battlefield(0, &["Volcanic Island"]);
    az.g = s.graveyard(0, gy).hand(1, &["Brainstorm"]).start();
    az
}

#[test]
fn graveyards_key_the_order_of_their_creatures_only() {
    let a = with_graveyard(&["Dragon's Rage Channeler", "Lightning Bolt", "Murktide Regent", "Ponder"]);
    // the non-creature cards elsewhere among them: the same key
    let b = with_graveyard(&["Ponder", "Dragon's Rage Channeler", "Murktide Regent", "Lightning Bolt"]);
    // the creatures the other way round: another key
    let c = with_graveyard(&["Murktide Regent", "Lightning Bolt", "Dragon's Rage Channeler", "Ponder"]);
    assert!(key(&a).is_some());
    assert_eq!(key(&a), key(&b));
    assert_ne!(key(&a), key(&c));
}

#[test]
fn renumbered_object_ids_keep_the_key() {
    let lib = ["Ponder", "Daze", "Lightning Bolt", "Island"];
    let a = scenario(0, &["Lightning Bolt", "Brainstorm"], &lib);
    let b = scenario(1, &["Lightning Bolt", "Brainstorm"], &lib);
    let id = |g: &MtgAz| g.g.player(0).hand.iter().map(|c| c.get()).collect::<Vec<_>>();
    assert_ne!(id(&a), id(&b), "the ids are the same");
    assert!(key(&a).is_some());
    assert_eq!(key(&a), key(&b));
    // nobody knows the library's order: another order, the same key
    let c = scenario(1, &["Lightning Bolt", "Brainstorm"], &["Island", "Lightning Bolt", "Daze", "Ponder"]);
    assert_eq!(key(&a), key(&c));
}

#[test]
fn different_hands_knowledge_or_library_cards_give_different_keys() {
    let lib = ["Ponder", "Daze", "Lightning Bolt", "Island"];
    let a = scenario(0, &["Lightning Bolt", "Brainstorm"], &lib);
    // a different hand (the same cards in all: the library holds the other)
    let h = scenario(0, &["Lightning Bolt", "Ponder"], &["Brainstorm", "Daze", "Lightning Bolt", "Island"]);
    // a different library multiset
    let l = scenario(0, &["Lightning Bolt", "Brainstorm"], &["Ponder", "Daze", "Lightning Bolt", "Volcanic Island"]);
    assert!(key(&h).is_some() && key(&l).is_some());
    assert_ne!(key(&a), key(&h));
    assert_ne!(key(&a), key(&l));
    // the opponent knows a hand card
    let base = start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"]));
    let mut known = base.clone();
    known["players"][0]["hand"][0] = json!({"name": "Lightning Bolt", "known_to_opponent": true});
    let (x, y) = (built(base), built(known));
    assert_eq!(sorted(names(&x, &x.g.player(1).hand)), sorted(names(&y, &y.g.player(1).hand)));
    assert!(key(&x).is_some() && key(&y).is_some());
    assert_ne!(key(&x), key(&y));
    // P0 knows its top card
    let mut top = start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder", "Daze"]));
    top["players"][0]["library_known"] = json!([{"pos": 0, "name": "Ponder", "known_by": [0]}]);
    let mut top2 = top.clone();
    top2["players"][0]["library_known"] = json!([{"pos": 1, "name": "Ponder", "known_by": [0]}]);
    let (t, t2) = (built(top), built(top2));
    assert!(key(&t).is_some() && key(&t2).is_some());
    assert_ne!(key(&t), key(&t2));
    assert_ne!(key(&t), key(&x));
}

#[test]
fn a_private_peek_has_no_key() {
    // P0's Mishra's Bauble looks at P1's top card; P1's fetch then shuffles it away: P0 remembers a card somewhere
    // in P1's library that P1 never saw (ENGINE.md 192) - `describe` refuses it, and so does the key.
    let mut d = start("main1", hand(&["Lightning Bolt", "Brainstorm", "Ponder"]));
    d["players"][0]["library_size"] = json!(53);
    d["players"][1]["library_size"] = json!(52);
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Mishra's Bauble", "owner": 0, "entered": 2}));
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Scalding Tarn", "owner": 1, "entered": 2}));
    let mut g = built(d);
    act(&mut g, "Activate Mishra's Bauble");
    act(&mut g, "Target P2");
    pass_until(&mut g, p0_free(Step::Main1));
    assert!(key(&g).is_some(), "a look before any shuffle is a known position");
    act(&mut g, "Pass priority");
    act(&mut g, "Activate Scalding Tarn");
    pass_until(&mut g, |g| kind(g) == "SearchLibrary");
    act(&mut g, "Find Volcanic Island");
    pass_until(&mut g, |g| g.g.stack().is_empty() && kind(g) == "Priority");
    assert_eq!(key(&g), None);
    assert!(g.describe(0).is_err());
}

#[test]
fn branch_alt_ok_excludes_twins_and_mana_details() {
    // P0 holds Wasteland; P1 controls two untapped Volcanic Islands (twins) and P0 one.
    let mut d = start("main1", hand(&["Lightning Bolt", "Ponder", "Daze"]));
    d["players"][0]["library_size"] = json!(53);
    d["players"][1]["library_size"] = json!(52);
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Volcanic Island", "owner": 1, "entered": 2}));
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Wasteland", "owner": 0, "entered": 1}));
    let g = built(d);
    let (pass, mana, ponder, bolt) = (action(&g, "Pass priority"), action(&g, "Activate mana abilities"), action(&g, "Cast Ponder"), action(&g, "Cast Lightning Bolt"));
    assert!(!g.branch_alt_ok(pass, pass), "the same action");
    assert!(!g.branch_alt_ok(pass, mana) && !g.branch_alt_ok(mana, ponder), "the mana-ability window");
    assert!(g.branch_alt_ok(pass, ponder) && g.branch_alt_ok(bolt, ponder));
    // Wasteland's targets: P1's two Volcanic Islands are twins, P0's own is not theirs
    let mut w = g.clone();
    act(&mut w, "Activate Wasteland");
    assert_eq!(kind(&w), "Target");
    let o = options(&w);
    let volcs: Vec<(u16, u32)> = o.iter().filter(|x| x.1 == "Target Volcanic Island").map(|x| (x.0, x.2.unwrap())).collect();
    let theirs: Vec<u16> = volcs.iter().filter(|v| w.g.obj(engine::ids::CardIdx::new(v.1)).controller == 1).map(|v| v.0).collect();
    let mine: Vec<u16> = volcs.iter().filter(|v| w.g.obj(engine::ids::CardIdx::new(v.1)).controller == 0).map(|v| v.0).collect();
    assert_eq!((theirs.len(), mine.len()), (2, 1), "{o:?}");
    assert!(!w.branch_alt_ok(theirs[0], theirs[1]), "two copies of a card, the same thing");
    assert!(w.branch_alt_ok(theirs[0], mine[0]));
    // the mana-ability window's own decisions: which source, which mana
    let mut m = g.clone();
    act(&mut m, "Activate mana abilities");
    assert_eq!(kind(&m), "ActivateMana");
    let o = options(&m);
    assert!(o.len() >= 2 && !m.branch_alt_ok(o[0].0, o[1].0));
    act(&mut m, "Activate a mana ability of Volcanic Island");
    assert_eq!(kind(&m), "ChooseMana", "{:?}", options(&m));
    let o = options(&m);
    assert!(!m.branch_alt_ok(o[0].0, o[1].0));
    // a target choice between players stays a choice
    let mut t = g.clone();
    act(&mut t, "Cast Lightning Bolt");
    assert!(t.branch_alt_ok(action(&t, "Target P1"), action(&t, "Target P2")));
}

/// Random and heuristic play over the Delver mirror (and its best-of-three dealt preset): the key at every
/// decision of a game - its time per call, the share keyed - against a stand-in for a 64-simulation search's game
/// side (64 clones, determinizations and encodings of the decider's row).
#[test]
#[ignore]
fn merge_key_cost() {
    let d = Decklist::izzet_delver();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let bo3 = json!({"format": "bo3", "game_episodes": true, "start_game": 2, "decks": {"p0": d.main_names(), "p1": d.main_names(), "p0_sideboard": side, "p1_sideboard": side},
        "dealt_decks": {"p": 0.5, "swaps": [1, 8]}});
    for (name, spec) in [("delver bo1", delver_spec(json!({}))), ("delver bo3 dealt", delver_spec(bo3))] {
        let (mut calls, mut keyed, mut ns_key, mut ns_search, mut searches) = (0u64, 0u64, 0u128, 0u128, 0u64);
        let mut keys = std::collections::BTreeSet::new();
        for seed in 0..20u64 {
            let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
            let mut h = Heuristic::new(seed);
            let mut r = Rng::new(seed ^ 0x5EED);
            for _ in 0..3000 {
                let Some(p) = g.to_move() else { break };
                let mut legal = Vec::new();
                g.legal_actions(&mut legal);
                let t = std::time::Instant::now();
                let k = g.merge_key();
                ns_key += t.elapsed().as_nanos();
                calls += 1;
                if let Some(k) = k {
                    keyed += 1;
                    keys.insert(k);
                }
                if calls % 25 == 0 {
                    let t = std::time::Instant::now();
                    let mut zr = zero_search::rng::Rng::new(seed ^ calls);
                    for _ in 0..64 {
                        let mut w = g.clone();
                        w.determinize(p, &mut zr);
                        std::hint::black_box(engine::az::tail_encode::Row::of_az(&w, p));
                    }
                    ns_search += t.elapsed().as_nanos();
                    searches += 1;
                }
                let a = if r.below(2) == 0 && g.sideboarding().is_none() { h.choose(&g.g, &legal) } else { legal[r.below(legal.len() as u32) as usize] };
                g.step(a, &mut Vec::new()).unwrap();
            }
        }
        println!(
            "{name}: {calls} decisions, {keyed} keyed ({:.1}%), {} distinct keys; merge_key {:.1} us a call; 64 x (clone + determinize + row) {:.0} us",
            100.0 * keyed as f64 / calls as f64,
            keys.len(),
            ns_key as f64 / calls as f64 / 1000.0,
            ns_search as f64 / searches.max(1) as f64 / 1000.0
        );
    }
    let _ = Zone::Library;
}


