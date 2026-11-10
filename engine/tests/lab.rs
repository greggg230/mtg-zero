//! The position lab's `start` block (ENGINE.md 76, `az/lab.rs`): a game started at a spot of
//! another game, its hidden cards dealt again, the decider's first moves forced.

use engine::az::constructed::Decklist;
use engine::az::lab::{identity_of, Rule};
use engine::az::{identity, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::rng::Rng;
use engine::{Choice, DecisionKind, Zone};
use serde_json::{json, Value};
use zero_search::game::SearchGame;

fn mirror() -> Value {
    let names = Decklist::izzet_delver().main_names();
    json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}})
}

fn with_start(start: Value) -> MtgSpec {
    let mut v = mirror();
    v["start"] = start;
    MtgSpec::parse(&v.to_string()).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn step(g: &mut MtgAz, a: u16) {
    let mut k = Vec::new();
    g.step(a, &mut k).unwrap();
}

/// Step the single-option decisions (zero-core's transitions).
fn auto(g: &mut MtgAz) {
    while g.to_move().is_some() && legal(g).len() == 1 {
        let a = legal(g)[0];
        step(g, a);
    }
}

fn state(g: &MtgAz) -> Value {
    let mut v: Value = serde_json::from_str(&g.export_state()).unwrap();
    v.as_object_mut().unwrap().remove("lab");
    v
}

fn option_texts(g: &MtgAz) -> Vec<(u16, String)> {
    let v: Value = serde_json::from_str(&g.export_state()).unwrap();
    v["decision"]["options"].as_array().unwrap().iter().map(|o| (o["a"].as_u64().unwrap() as u16, o["t"].as_str().unwrap().to_string())).collect()
}

fn names_in(g: &MtgAz, p: u8, zone: Zone) -> Vec<String> {
    let pl = g.g.player(p);
    let cards = match zone {
        Zone::Hand => &pl.hand,
        _ => &pl.library,
    };
    let mut v: Vec<String> = cards.iter().map(|&c| g.g.db().def(g.g.ident(c)).name.clone()).collect();
    if zone == Zone::Hand {
        v.sort();
    }
    v
}

/// A random game of the mirror (`seed`), `n` multi-option decisions in (each after the
/// single-option ones before it, as a replay's frames): the game and its actions.
fn walk(seed: u64, n: usize) -> Option<(MtgAz, Vec<u16>)> {
    let spec = MtgSpec::parse(&mirror().to_string()).unwrap();
    let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
    let mut r = Rng::new(seed ^ 0xABCD);
    let mut acts = Vec::new();
    for _ in 0..n {
        auto(&mut g);
        if g.to_move().is_none() {
            return None;
        }
        let l = legal(&g);
        let a = l[r.below(l.len() as u32) as usize];
        acts.push(a);
        step(&mut g, a);
    }
    auto(&mut g);
    g.to_move().is_some().then_some((g, acts))
}

#[test]
fn a_spec_without_start_is_the_spec_of_before() {
    let j = mirror().to_string();
    let a = MtgSpec::parse(&j).unwrap();
    assert!(a.start.is_none());
    // the same games: new_game is MtgAz::new, no lab state, the same legal actions throughout
    let mut g = MtgAz::new_game(&a, 2, 11).unwrap();
    let mut h = MtgAz::new(&a, 11).unwrap();
    assert!(g.lab.is_none());
    let mut r = Rng::new(5);
    for _ in 0..400 {
        if g.to_move().is_none() {
            break;
        }
        assert_eq!(legal(&g), legal(&h));
        assert_eq!(g.export_state(), h.export_state());
        let l = legal(&g);
        let a = l[r.below(l.len() as u32) as usize];
        step(&mut g, a);
        step(&mut h, a);
    }
    assert!(!g.export_state().contains("\"lab\""));
}

#[test]
fn start_blocks_are_checked() {
    let bad = |s: Value| {
        let mut v = mirror();
        v["start"] = s;
        MtgSpec::parse(&v.to_string()).unwrap_err()
    };
    assert!(bad(json!({})).contains("start.seed"));
    assert!(bad(json!({"seed": 1, "what": 2})).contains("unknown start key"));
    assert!(bad(json!({"seed": 1})).contains("needs start.pred"), "belief is the default deal");
    assert!(bad(json!({"seed": 1, "deal": "actual", "hold": ["Daze"]})).contains("dealt hand"));
    assert!(bad(json!({"seed": 1, "deal": "uniform", "hold": ["Daze"], "exclude": ["Daze"]})).contains("holds and excludes"));
    assert!(bad(json!({"seed": 1, "deal": "uniform", "mix": 0.5})).contains("belief"));
    assert!(bad(json!({"seed": 1, "deal": "uniform", "forced": [true]})).contains("start.forced"));
    assert!(bad(json!({"seed": 1, "deal": "uniform", "follow": {"rule": "x"}})).contains("bauble_fetch"));
    let ok = with_start(json!({"seed": 1, "deal": "uniform", "forced": ["Keep", 3, "*"], "follow": {"rule": "bauble_fetch"}}));
    assert_eq!(ok.start.as_ref().unwrap().forced.len(), 3);
    assert_eq!(ok.clone(), ok, "a spec with a start is Eq to itself");
}

#[test]
fn the_spot_is_the_source_game_after_its_actions() {
    for seed in [3u64, 4, 5] {
        let Some((src, acts)) = walk(seed, 60) else { continue };
        // deal "none": the source game itself
        let g = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "none"})), 2, 99).unwrap();
        assert_eq!(state(&g), state(&src), "seed {seed}");
        assert_eq!(legal(&g), legal(&src));
        for p in 0..2 {
            assert_eq!(names_in(&g, p, Zone::Library), names_in(&src, p, Zone::Library));
        }
        // an action the source game did not offer is an error, not another game
        let mut bad = acts.clone();
        bad[10] = 1023;
        let e = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": bad, "deal": "none"})), 2, 1).err().unwrap();
        assert!(e.contains("action 10"), "{e}");
    }
}

#[test]
fn the_actual_deal_keeps_the_real_hands_and_shuffles_only_unknown_library_cards() {
    let (src, acts) = walk(21, 80).expect("a live game");
    let mut orders = std::collections::HashSet::new();
    for ds in 0..12u64 {
        let g = MtgAz::new_game(&with_start(json!({"seed": 21, "actions": acts, "deal": "actual", "deal_seed": ds})), 2, 0).unwrap();
        // everything a spectator sees is the source game's: both hands, the board, the decision
        assert_eq!(state(&g), state(&src));
        for p in 0..2u8 {
            let (a, b) = (names_in(&g, p, Zone::Library), names_in(&src, p, Zone::Library));
            let (mut sa, mut sb) = (a.clone(), b.clone());
            sa.sort();
            sb.sort();
            assert_eq!(sa, sb, "the same library cards");
            // a library card someone knows stays where it is
            for (i, &c) in src.g.player(p).library.iter().enumerate() {
                if src.g.obj(c).known != 0 {
                    assert_eq!(g.g.ident(g.g.player(p).library[i]), src.g.ident(c));
                }
            }
            orders.insert((p, a));
        }
        g.g.check_knowledge().unwrap();
    }
    assert!(orders.len() >= 20, "the unknown orders are dealt again ({} distinct)", orders.len());
}

/// Deals of the spot by `deal` and `deal_seed`, the opponent's hand of each.
fn hands(acts: &[u16], seed: u64, extra: Value, n: u64) -> Vec<Vec<String>> {
    (0..n)
        .map(|ds| {
            let mut s = json!({"seed": seed, "actions": acts, "deal_seed": ds});
            for (k, v) in extra.as_object().unwrap() {
                s[k] = v.clone();
            }
            let g = MtgAz::new_game(&with_start(s), 2, 0).unwrap();
            let d = g.lab.as_ref().unwrap().player;
            names_in(&g, 1 - d, Zone::Hand)
        })
        .collect()
}

#[test]
fn deals_are_paired_by_their_deal_seed_whatever_the_candidate() {
    let (_, acts) = walk(8, 50).expect("a live game");
    for deal in ["uniform", "actual"] {
        for ds in 0..6u64 {
            let mk = |forced: Value, gid_seed: u64| {
                let g = MtgAz::new_game(
                    &with_start(json!({"seed": 8, "actions": acts, "deal": deal, "deal_seed": ds, "forced": forced})),
                    2,
                    gid_seed,
                )
                .unwrap();
                let v: Value = serde_json::from_str(&g.export_state()).unwrap();
                (state(&g), v["lab"]["libraries"].clone(), g.g.player(0).hand.iter().map(|&c| g.g.ident(c)).collect::<Vec<_>>())
            };
            // two candidates (another forced line, another driver game seed): the same world
            let a = mk(json!([]), 1);
            let b = mk(json!(["*", "Pass priority"]), 777);
            assert_eq!(a, b, "{deal} deal seed {ds}");
        }
    }
    // without a deal seed, the driver's game seed deals
    let s = with_start(json!({"seed": 8, "actions": acts, "deal": "uniform"}));
    let x = MtgAz::new_game(&s, 2, 5).unwrap();
    let y = MtgAz::new_game(&s, 2, 5).unwrap();
    assert_eq!(x.export_state(), y.export_state());
    let other: Vec<String> = (6..30).map(|gs| MtgAz::new_game(&s, 2, gs).unwrap().export_state()).collect();
    assert!(other.iter().any(|o| *o != x.export_state()));
}

#[test]
fn held_cards_are_in_the_hand_and_excluded_ones_are_not() {
    let (src, acts) = walk(8, 50).expect("a live game");
    let d = src.g.to_move().unwrap();
    let n = src.g.player(1 - d).hand.len();
    assert!(n >= 2, "a hand to deal ({n})");
    let hs = hands(&acts, 8, json!({"deal": "uniform", "hold": ["Force of Will"], "exclude": ["Daze", "Brainstorm"]}), 60);
    for h in &hs {
        assert!(h.iter().any(|c| c == "Force of Will"), "{h:?}");
        assert!(!h.iter().any(|c| c == "Daze" || c == "Brainstorm"), "{h:?}");
    }
    let free = hands(&acts, 8, json!({"deal": "uniform"}), 60);
    assert!(free.iter().any(|h| !h.iter().any(|c| c == "Force of Will")), "the pin changed the deal");
    // two copies held: two in hand
    for h in hands(&acts, 8, json!({"deal": "uniform", "hold": ["Lightning Bolt", "Lightning Bolt"]}), 20) {
        assert!(h.iter().filter(|c| *c == "Lightning Bolt").count() >= 2, "{h:?}");
    }
    // a hand cannot hold more cards than it has
    let many: Vec<&str> = vec!["Force of Will"; 4].into_iter().chain(vec!["Daze"; 4]).collect();
    if n < 8 {
        let mut s = json!({"seed": 8, "actions": acts, "deal": "uniform", "hold": many});
        s["deal_seed"] = json!(1);
        let e = MtgAz::new_game(&with_start(s), 2, 0).err().unwrap();
        assert!(e.contains("no deal can give"), "{e}");
    }
}

#[test]
fn the_belief_deal_follows_the_prediction() {
    // a constructed belief: the opponent's hand is mostly Force of Will
    let db = CardDb::constructed();
    // (every target's prediction: the hand's, then the deck's, uniform: ENGINE.md 102.4)
    let w: usize = MtgAz::AUX.iter().map(|t| t.1).sum();
    assert_eq!(w, 2 * identity::D);
    let mut pred = vec![1e-4f32; identity::D];
    pred[identity_of(db, "Force of Will").unwrap()] = 0.5;
    let total: f32 = pred.iter().sum();
    let mut pred: Vec<f32> = pred.iter().map(|p| p / total).collect();
    pred.extend(std::iter::repeat_n(1.0 / identity::D as f32, identity::D));
    let fow = |hs: &[Vec<String>]| hs.iter().map(|h| h.iter().filter(|c| *c == "Force of Will").count()).sum::<usize>();
    let mut seen = [0usize; 3];
    let mut tried = 0;
    for seed in 30..60u64 {
        let Some((src, acts)) = walk(seed, 40) else { continue };
        let d = src.g.to_move().unwrap();
        if src.g.player(1 - d).hand.len() < 3 {
            continue;
        }
        tried += 1;
        seen[0] += fow(&hands(&acts, seed, json!({"deal": "uniform"}), 40));
        seen[1] += fow(&hands(&acts, seed, json!({"deal": "belief", "pred": pred, "mix": 0.5}), 40));
        seen[2] += fow(&hands(&acts, seed, json!({"deal": "belief", "pred": pred, "tilt": 1, "clip": 20}), 40));
        if tried == 6 {
            break;
        }
    }
    assert!(tried >= 3);
    // FoW's copies hidden from the decider are dealt to its hand far more often than uniformly,
    // and a mix of uniform deals lies in between
    assert!(seen[2] as f64 > 1.8 * seen[0] as f64 + 10.0, "belief {} vs uniform {}", seen[2], seen[0]);
    assert!(seen[0] < seen[1] && seen[1] < seen[2], "uniform < mix 0.5 < belief: {seen:?}");
}

/// A random mirror game's spot where the decider may activate a Mishra's Bauble and a fetchland
/// with an empty stack in its main phase.
fn bauble_spot() -> (u64, Vec<u16>, MtgAz) {
    let fetches = ["Flooded Strand", "Misty Rainforest", "Scalding Tarn", "Polluted Delta"];
    for seed in 0..400u64 {
        let spec = MtgSpec::parse(&mirror().to_string()).unwrap();
        let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
        let mut r = Rng::new(seed ^ 0x77);
        let mut acts = Vec::new();
        for _ in 0..300 {
            auto(&mut g);
            if g.to_move().is_none() {
                break;
            }
            let o = option_texts(&g);
            let has = |t: &str| o.iter().any(|x| x.1 == t);
            if g.g.stack().is_empty()
                && g.g.turn().active == g.g.to_move().unwrap()
                && has("Activate Mishra's Bauble")
                && fetches.iter().any(|f| has(&format!("Activate {f}")))
            {
                return (seed, acts, g);
            }
            // random play that keeps permanents around: never activate a fetch or a Bauble
            let l: Vec<u16> = o.iter().filter(|x| !x.1.starts_with("Activate ")).map(|x| x.0).collect();
            let l = if l.is_empty() { legal(&g) } else { l };
            let a = l[r.below(l.len() as u32) as usize];
            acts.push(a);
            step(&mut g, a);
        }
    }
    panic!("no Bauble + fetch spot in 400 random games");
}

/// Play `g` on: the decider's single options as they come, the opponent passing (or its lowest
/// option), until the lab's rule is no longer armed.
fn until_rule_starts(g: &mut MtgAz) {
    for _ in 0..200 {
        let lab = g.lab.as_ref().unwrap();
        if lab.rule != Rule::Armed || g.to_move().is_none() {
            return;
        }
        let l = legal(g);
        let a = if g.to_move() == Some(lab.player as usize) {
            // the forced line offers one option (a `resolve` item lets other questions through)
            assert!(l.len() == 1 || g.g.pending_decision().unwrap().kind != DecisionKind::Priority, "{:?}", option_texts(g));
            l[0]
        } else {
            option_texts(g).iter().find(|x| x.1 == "Pass priority").map_or(l[0], |x| x.0)
        };
        step(g, a);
    }
    panic!("the rule never started");
}

#[test]
fn the_forced_line_and_the_bauble_fetch_rule() {
    let (seed, acts, spot) = bauble_spot();
    let d = spot.g.to_move().unwrap();
    let line = json!(["Activate Mishra's Bauble", format!("Target P{}", d + 1), "resolve"]);
    let mut outcomes = [0usize; 2];
    for ds in 0..40u64 {
        let spec = with_start(json!({"seed": seed, "actions": acts, "deal": "uniform", "deal_seed": ds, "forced": line,
                                     "follow": {"rule": "bauble_fetch", "land_min": 0}}));
        let mut g = MtgAz::new_game(&spec, 2, 0).unwrap();
        // the line's first item: one option, the Bauble's activation
        let o = option_texts(&g);
        assert_eq!(legal(&g).len(), 1);
        assert_eq!(o.iter().find(|x| x.0 == legal(&g)[0]).unwrap().1, "Activate Mishra's Bauble");
        until_rule_starts(&mut g);
        let lab = g.lab.as_ref().unwrap().clone();
        assert_eq!(lab.broken, None);
        let Rule::Active { card } = lab.rule else { panic!("the look resolved, the rule holds: {:?}", lab.rule) };
        assert_eq!(g.g.to_move(), Some(d));
        assert_eq!(g.g.player(d).library.last(), Some(&card));
        let (seen, bad) = lab.seen.unwrap();
        assert_eq!(seen, g.g.ident(card));
        let land = g.g.db().def(seen).types & engine::db::types::LAND != 0;
        assert_eq!(bad, land, "land_min 0: bad iff a land");
        let texts = option_texts(&g);
        let cracks: Vec<u16> = texts.iter().filter(|x| is_fetch(&x.1)).map(|x| x.0).collect();
        let l = legal(&g);
        if bad {
            assert_eq!(l.len(), 1, "a bad card: the crack is forced");
            assert!(cracks.contains(&l[0]));
        } else {
            assert!(l.iter().all(|a| !cracks.contains(a)), "a good card: no crack while it is on top");
            assert!(l.len() >= 1);
        }
        // the decider's own worlds keep the decision (zero-core checks the root decision)
        for k in 0..4 {
            let mut w = g.clone();
            w.determinize(d as usize, &mut zero_search::rng::Rng::new(k));
            assert_eq!(legal(&w), l);
        }
        outcomes[bad as usize] += 1;
        // a crack, then the rule ends (the library shuffled)
        if bad {
            let a = l[0];
            step(&mut g, a);
            assert_eq!(g.lab.as_ref().unwrap().cracked, 1);
            assert_eq!(g.lab.as_ref().unwrap().rule, Rule::Off);
        }
    }
    assert!(outcomes[0] > 0 && outcomes[1] > 0, "both verdicts seen: {outcomes:?}");
    // a label the decision does not offer breaks the line: the network decides from there
    let spec = with_start(json!({"seed": seed, "actions": acts, "deal": "uniform", "deal_seed": 1,
                                 "forced": ["Cast Lightning Bolt Twice"], "follow": {"rule": "bauble_fetch"}}));
    let mut g = MtgAz::new_game(&spec, 2, 0).unwrap();
    assert!(legal(&g).len() > 1);
    assert_eq!(legal(&g), legal(&spot));
    let a = legal(&g)[0];
    step(&mut g, a);
    assert_eq!(g.lab.as_ref().unwrap().broken, Some(0));
    assert_eq!(g.lab.as_ref().unwrap().rule, Rule::Off);
}

#[test]
fn the_rule_branches_into_its_verdicts_line() {
    let (seed, acts, spot) = bauble_spot();
    let d = spot.g.to_move().unwrap();
    let line = json!(["Activate Mishra's Bauble", format!("Target P{}", d + 1), "resolve"]);
    let mut seen = [0usize; 2];
    for ds in 0..30u64 {
        let follow = json!({"rule": "bauble_fetch", "land_min": 0, "bad": ["Pass priority"], "good": ["*", "Pass priority"]});
        let mut g = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "uniform", "deal_seed": ds,
                                                         "forced": line, "follow": follow})), 2, 0).unwrap();
        until_rule_starts(&mut g);
        let lab = g.lab.as_ref().unwrap().clone();
        let (_, bad) = lab.seen.expect("the rule started");
        let texts = option_texts(&g);
        if bad {
            // the bad line: "Pass priority" alone (not the crack the rule alone would force)
            assert_eq!(lab.forced.len(), 4);
            assert_eq!(legal(&g).len(), 1);
            assert_eq!(texts.iter().find(|x| x.0 == legal(&g)[0]).unwrap().1, "Pass priority");
        } else {
            // the good line: "*" (the network's own choice: every option), then a pass
            assert_eq!(lab.forced.len(), 5);
            assert_eq!(legal(&g).len(), texts.len());
        }
        seen[bad as usize] += 1;
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

#[test]
fn a_resolve_item_passes_until_the_stack_is_empty() {
    let (seed, acts, spot) = bauble_spot();
    let d = spot.g.to_move().unwrap();
    // at an empty stack a leading `resolve` is done at once: the next item applies to this decision
    let g = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "none", "forced": ["resolve", "Pass priority"]})), 2, 0).unwrap();
    assert_eq!(g.lab.as_ref().unwrap().pos, 1);
    assert_eq!(legal(&g).len(), 1);
    assert_eq!(option_texts(&g).iter().find(|x| x.0 == legal(&g)[0]).unwrap().1, "Pass priority");
    // with the Bauble's ability on the stack: the decider passes (one option), the opponent answers,
    // and the item is done at the decider's next priority with an empty stack
    let line = json!(["Activate Mishra's Bauble", format!("Target P{}", d + 1), "resolve", "*"]);
    let mut g = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "none", "forced": line})), 2, 0).unwrap();
    let mut passes = 0;
    for _ in 0..50 {
        let lab = g.lab.as_ref().unwrap().clone();
        if lab.pos >= 3 {
            break;
        }
        let l = legal(&g);
        let a = if g.g.to_move() == Some(d) {
            if lab.pos == 2 && !g.g.stack().is_empty() {
                assert_eq!(l.len(), 1);
                assert_eq!(option_texts(&g).iter().find(|x| x.0 == l[0]).unwrap().1, "Pass priority");
                passes += 1;
            }
            l[0]
        } else {
            option_texts(&g).iter().find(|x| x.1 == "Pass priority").map_or(l[0], |x| x.0)
        };
        step(&mut g, a);
    }
    let lab = g.lab.as_ref().unwrap();
    assert!(passes >= 1);
    assert_eq!((lab.pos, lab.broken), (3, None));
    assert!(g.g.stack().is_empty() && g.g.to_move() == Some(d));
    assert!(legal(&g).len() > 1, "'*': the network's own choice");
}

fn is_fetch(t: &str) -> bool {
    ["Flooded Strand", "Misty Rainforest", "Scalding Tarn", "Polluted Delta"].iter().any(|f| t == format!("Activate {f}"))
}

#[test]
fn lab_games_play_to_the_end() {
    // the dealt worlds are real games from the spot on: random play to the end, every deal mode
    let mut ended = 0;
    for seed in 50..62u64 {
        let Some((_, acts)) = walk(seed, 70) else { continue };
        for deal in ["actual", "uniform"] {
            let mut g = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": deal, "deal_seed": seed})), 2, 0).unwrap();
            let mut r = Rng::new(seed);
            for _ in 0..20_000 {
                if g.to_move().is_none() {
                    break;
                }
                let l = legal(&g);
                let a = l[r.below(l.len() as u32) as usize];
                step(&mut g, a);
            }
            assert!(g.to_move().is_none(), "seed {seed} {deal}: not over");
            g.g.check_knowledge().unwrap();
            ended += 1;
        }
    }
    assert!(ended >= 10);
    let _ = (Choice::Pass, DecisionKind::Priority);
}

#[test]
fn an_ambiguous_label_is_an_error() {
    // a random mirror game's decision with two options of one label (two of a permanent, ...)
    for seed in 0..300u64 {
        let spec = MtgSpec::parse(&mirror().to_string()).unwrap();
        let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
        let mut r = Rng::new(seed ^ 0x55);
        let mut acts = Vec::new();
        for _ in 0..400 {
            auto(&mut g);
            if g.to_move().is_none() {
                break;
            }
            let o = option_texts(&g);
            let dup = o.iter().find(|x| o.iter().filter(|y| y.1 == x.1).count() > 1).map(|x| x.1.clone());
            if let Some(label) = dup {
                let e = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "none", "forced": [label]})), 2, 0).err();
                assert!(e.as_deref().is_some_and(|e| e.contains("names 2 options") || e.contains("names 3 options")), "{e:?}");
                // by action id it is fine
                let a = o.iter().find(|x| x.1 == label).unwrap().0;
                let g2 = MtgAz::new_game(&with_start(json!({"seed": seed, "actions": acts, "deal": "none", "forced": [a]})), 2, 0).unwrap();
                assert_eq!(legal(&g2), vec![a]);
                return;
            }
            let l = legal(&g);
            let a = l[r.below(l.len() as u32) as usize];
            acts.push(a);
            step(&mut g, a);
        }
    }
    panic!("no decision with a duplicate label in 300 random games");
}

#[test]
fn top_pins_put_the_cards_on_top_and_their_owner_may_know_them() {
    let (src, acts) = walk(8, 50).expect("a live game");
    let d = src.g.to_move().unwrap();
    for ds in 0..10u64 {
        let mut top = [json!([]), json!([])];
        top[d as usize] = json!(["Brainstorm", "Daze"]);
        let g = MtgAz::new_game(&with_start(json!({"seed": 8, "actions": acts, "deal": "uniform", "deal_seed": ds,
                                                    "top": top, "top_known": true})), 2, 0).unwrap();
        let lib = names_in(&g, d, Zone::Library);
        assert_eq!(&lib[lib.len() - 2..], ["Daze", "Brainstorm"], "top first: the last two");
        let l = &g.g.player(d).library;
        assert!(g.g.knows(d, l[l.len() - 1]) && g.g.knows(d, l[l.len() - 2]));
        g.g.check_knowledge().unwrap();
    }
    let bad = MtgSpec::parse(&{
        let mut v = mirror();
        v["start"] = json!({"seed": 8, "deal": "actual", "top": [["Daze"], []]});
        v.to_string()
    });
    assert!(bad.unwrap_err().contains("dealt world"));
}
