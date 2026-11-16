//! Layout v7's variable-length rows (ENGINE.md 125; zero-core docs/search.md 1.4, 2.15): the
//! tail tokens of the capped segments, the tail-only segments, the pointer overrides, the
//! refusals (R1, R17), and the decoder test over boards past every old cap.

use engine::az::encode::{self, CODE, NUM_FEATURES};
use engine::az::identity;
use engine::az::tail_encode::{self as te, board, check_pointers, code_identity, seg, Row};
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Zone};
use zero_search::game::SearchGame;

fn ident(g: &Game, def: engine::ids::DefId) -> usize {
    identity::identities(g.db())[def as usize] as usize
}

/// The layout's constants and the game's trait items agree (zero-core R15 checks them against
/// the layout at registration).
#[test]
fn the_rows_constants() {
    assert_eq!(te::HEAD_TOKENS, 170);
    assert_eq!(<MtgAz as SearchGame>::HEAD_TOKENS, 170);
    assert_eq!(<MtgAz as SearchGame>::MAX_ROW_TOKENS, 4096);
    assert!(<MtgAz as SearchGame>::OPTION_POINTERS);
    let tails = <MtgAz as SearchGame>::TAILS;
    assert_eq!(tails.len(), 21);
    let names: Vec<&str> = tails.iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "my_permanents", "opponent_permanents", "hand", "stack", "triggers", "list", "graveyard_abilities", "opponent_triggers",
            "delayed", "links", "exile_offered", "library_known", "stack_relations", "graveyard_order", "face_down", "my_revealed",
            "effects", "counters", "numbers", "opp_revealed", "found"
        ]
    );
    // (Layout v6's game keeps no tail: ENGINE.md 124.2.)
    assert!(<engine::az::eoe::MtgEoeAz as SearchGame>::TAILS.is_empty());
    assert_eq!(<engine::az::eoe::MtgEoeAz as SearchGame>::NUM_FEATURES, encode::NUM_FEATURES_V6);
    assert_eq!(NUM_FEATURES, 56_384);
}

/// ENGINE.md 142.1: `Row::flat` (what the gate, the hidden-information tests and the fuzz compare
/// rows by) is exact: rows whose overrides point at different slots of one segment and column, or
/// whose tail tokens sit at slots past 2^24, give different vectors (the Fable review's probe A:
/// slots 33, 34 and 70,000 compared equal).
#[test]
fn flat_rows_are_exact() {
    use zero_search::tail::pack_tok;
    let row = |tail: Vec<(u16, u32, Vec<f32>)>, ov: Vec<(u32, i64)>| Row { head: vec![0.0; 8], tail, ov };
    let ovs = [pack_tok(2, 0, 33), pack_tok(2, 0, 34), pack_tok(2, 0, 70_000), pack_tok(2, 1, 33), pack_tok(3, 0, 33), -1];
    for (i, &a) in ovs.iter().enumerate() {
        for (j, &b) in ovs.iter().enumerate() {
            let (x, y) = (row(vec![], vec![(0, a)]), row(vec![], vec![(0, b)]));
            assert_eq!(x.flat() == y.flat(), i == j, "overrides {a} and {b}");
            assert_eq!(x.flat() == y.flat(), x == y);
        }
    }
    // the option index too
    assert_ne!(row(vec![], vec![(1 << 24, -1)]).flat(), row(vec![], vec![((1 << 24) + 1, -1)]).flat());
    // tail slots past 2^24 (a `my_revealed` token's slot is an identity)
    let t = |sl: u32| row(vec![(seg::MY_REVEALED, sl, vec![1.0; 53])], vec![]);
    assert_ne!(t(1 << 24).flat(), t((1 << 24) + 1).flat());
    assert_ne!(t(5).flat(), t(6).flat());
    assert_eq!(t(7).flat(), t(7).flat());
}

/// R1: a head-only encoding of a state with tail tokens is a loud failure (`encode_features`,
/// `MtgAz::encode`); `encode_row` has the row whole.
#[test]
fn r1_encode_refuses_a_row_with_tail_tokens() {
    let g = board::of(board::Size { perms: 40, hand: 2, stack: 1, triggers: 1, delayed: 0 });
    let row = Row::of(&g, 0);
    assert!(row.tail.iter().any(|t| t.0 == seg::MY_PERM), "P0's 33rd permanent is a tail token");
    let r = std::panic::catch_unwind(|| {
        let mut x = vec![0.0f32; NUM_FEATURES];
        g.encode_features(0, &mut x);
    });
    assert!(r.is_err(), "encode_features of a row with a tail");
    // A match's `encode` likewise: the first state of a cube game whose row has a tail.
    let sp = MtgSpec::parse(r#"{"max_turns": 60}"#).unwrap();
    let mut a = MtgAz::new_game(&sp, 2, 7).unwrap();
    let mut rng = engine::rng::Rng::new(7);
    let (mut keys, mut legal) = (Vec::new(), Vec::new());
    let mut seen = false;
    for _ in 0..3000 {
        let Some(p) = a.to_move() else { break };
        let row = Row::of_az(&a, p);
        if !row.tail.is_empty() {
            let b = a.clone();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let mut x = vec![0.0f32; NUM_FEATURES];
                b.encode(p, &mut x);
            }));
            assert!(r.is_err(), "MtgAz::encode of a row with a tail");
            seen = true;
            break;
        }
        a.legal_actions(&mut legal);
        a.step(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
    }
    assert!(seen, "no row with a tail in a cube game");
}

/// The decoder test (the design's section 9.1): on boards with every segment's object count swept
/// across its head and its ids' pages, each object is exactly one token of its segment at its
/// position, with its identity; the tail-only segments hold one token per waiting trigger of the
/// other player, delayed trigger, link and known deep library card. Both viewers.
#[test]
fn every_object_is_exactly_one_token() {
    let sizes = [
        (3, 3, 3, 3, 0),
        (4, 15, 7, 4, 1),
        (5, 16, 8, 5, 2),
        (31, 17, 9, 15, 3),
        (32, 40, 31, 16, 1),
        (33, 5, 33, 17, 0),
        (63, 2, 2, 2, 0),
        (64, 2, 2, 2, 0),
        (65, 2, 2, 2, 0),
        (95, 2, 2, 2, 0),
        (96, 2, 2, 2, 0),
        (97, 2, 2, 2, 0),
        (191, 1, 1, 1, 0),
        (192, 1, 1, 1, 0),
    ];
    for &(perms, hand, stack, triggers, delayed) in &sizes {
        let g = board::of(board::Size { perms, hand, stack, triggers, delayed });
        for v in 0..2u8 {
            let row = Row::of(&g, v);
            let what = format!("perms {perms}, hand {hand}, stack {stack}, triggers {triggers}: P{v}");
            for (side, p) in [v, 1 - v].into_iter().enumerate() {
                let sg = if side == 0 { seg::MY_PERM } else { seg::OPP_PERM };
                let toks = row.tokens(sg);
                let objs: Vec<engine::CardIdx> = g.permanents_of(p).collect();
                assert_eq!(toks.len(), objs.len(), "{what}: side {side}'s permanents");
                for (k, (slot, x)) in toks.iter().enumerate() {
                    assert_eq!(*slot as usize, k, "{what}: side {side}: absolute positions");
                    assert_eq!(code_identity(&x[..CODE]), Some(ident(&g, g.obj(objs[k]).def)), "{what}: side {side} #{k}");
                    assert_eq!(x[CODE + encode::perm::PRESENT], 1.0);
                }
            }
            let hand_cards = g.hand_sorted(v);
            let toks = row.tokens(seg::HAND);
            assert_eq!(toks.len(), hand_cards.len(), "{what}: the hand");
            for (k, (slot, x)) in toks.iter().enumerate() {
                assert_eq!((*slot as usize, code_identity(&x[..CODE])), (k, Some(ident(&g, g.obj(hand_cards[k]).def))), "{what}: hand #{k}");
            }
            let toks = row.tokens(seg::STACK);
            assert_eq!(toks.len(), g.stack().len(), "{what}: the stack");
            for (k, (slot, x)) in toks.iter().enumerate() {
                let item = &g.stack()[g.stack().len() - 1 - k];
                let engine::game::StackKind::Spell { card } = item.kind else { panic!() };
                assert_eq!((*slot as usize, code_identity(&x[..CODE])), (k, Some(ident(&g, g.obj(card).def))), "{what}: stack #{k}");
            }
            assert_eq!(row.tokens(seg::TRIGGERS).len(), triggers, "{what}: the frame's waiting triggers");
            assert_eq!(row.tokens(seg::OPP_TRIGGERS).len(), triggers, "{what}: the other player's waiting triggers");
            // Delayed: the Mana Drains and the two Memory Jars' (no temporary trigger).
            assert_eq!(row.tokens(seg::DELAYED).len(), delayed + 2, "{what}: delayed triggers");
            assert_eq!(row.tokens(seg::LINKS).len(), 2, "{what}: the two linked cards");
            // One known card at depth 5 in each library; its identity only for its owner.
            let lk = row.tokens(seg::LIBRARY_KNOWN);
            assert_eq!(lk.len(), 2, "{what}: known library cards below the top three");
            for (_, x) in &lk {
                let mine = x[CODE + te::lb::MINE] == 1.0;
                assert_eq!(code_identity(&x[..CODE]).is_some(), mine, "{what}: a known card's identity is its owner's knowledge");
                assert_eq!(x[CODE + te::lb::DEPTH], 6.0 / 64.0);
            }
            // Every value bounded, every token sorted and unique (sealed).
            assert!(row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).all(|v| v.is_finite() && v.abs() <= encode::FEATURE_BOUND));
            assert!(row.tail.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)));
            assert!(row.capacity() <= te::MAX_ROW_TOKENS, "{what}: {} tokens", row.capacity());
        }
    }
}

/// The public-state gate past the old caps (the design's section 9.2): each change of an object
/// past a segment's head slots, of a delayed trigger's amount, of Memory Jars' pairing, of a deep
/// known library card, and each swap of two objects' order changes both viewers' rows.
#[test]
fn perturbations_past_the_old_caps_change_the_row() {
    let g0 = board::of(board::Size { perms: 48, hand: 20, stack: 12, triggers: 18, delayed: 2 });
    let base = [Row::of(&g0, 0).flat(), Row::of(&g0, 1).flat()];
    let mut n = 0;
    for (name, f) in board::perturbations() {
        let mut g = g0.clone();
        let what = f(&mut g).unwrap_or_else(|| panic!("{name}: not applied"));
        for v in 0..2u8 {
            assert_ne!(Row::of(&g, v).flat(), base[v as usize], "{name} ({what}): P{v}'s row unchanged");
        }
        n += 1;
    }
    println!("{n} perturbations past the old caps, every one encoded for both viewers");
    assert_eq!(n, board::perturbations().len());
}

/// Every option points at its object's token on boards past the caps (the decider's hand past 16
/// cards: `HAND` ids past slot 15 point at tail tokens through overrides).
#[test]
fn options_past_the_head_point_at_tail_tokens() {
    // 20 copies of a card sorted before Lightning Bolt, then the Bolt: its option is `HAND` + 20.
    let db = CardDb::cube();
    let bolt = db.expect_id("Lightning Bolt").unwrap();
    let filler = ["Island", "Ponder", "Brainstorm", "Counterspell", "Mountain"].into_iter().find(|n| db.expect_id(n).unwrap() < bolt).expect("a card sorted before the Bolt");
    let mut hand = vec![filler; 20];
    hand.push("Lightning Bolt");
    let g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Ponder"; 10])
        .library(1, &["Ponder"; 10])
        .battlefield(0, &["Mountain"])
        .hand(0, &hand)
        .at(Step::Main1, 0)
        .start();
    let row = Row::of(&g, 0);
    let n = check_pointers(&g, 0, &row).unwrap_or_else(|e| panic!("{e}"));
    assert!(n > 0);
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    let past: Vec<(usize, u16)> = legal.iter().copied().enumerate().filter(|&(_, a)| (act::HAND + 16..act::HAND + 64).contains(&a)).collect();
    assert!(!past.is_empty(), "a Lightning Bolt past the 16th hand card is castable");
    for (k, a) in past {
        let tok = row.ov.iter().find(|o| o.0 as usize == k).map(|o| zero_search::tail::unpack_tok(o.1));
        assert_eq!(tok, Some((seg::HAND, 0, (a - act::HAND) as u32)), "HAND + {}", a - act::HAND);
    }
    // A row within the old caps lists no override.
    let small = board::of(board::Size { perms: 4, hand: 5, stack: 0, triggers: 0, delayed: 0 });
    assert!(Row::of(&small, 0).ov.is_empty(), "a row within the old caps has no overrides");
}

/// A paged choice of permanents (ENGINE.md 100.4.8): on the range page the options point at the
/// permanents' absolute positions (overrides), the static slots would be other permanents.
#[test]
fn a_range_page_points_at_absolute_positions() {
    let db = CardDb::cube();
    let mut g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Ouroboroid"; 120])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    for _ in 0..20 {
        if g.kind() == Some(DecisionKind::Target) {
            break;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    let row = Row::of(&g, 0);
    check_pointers(&g, 0, &row).unwrap_or_else(|e| panic!("first page: {e}"));
    let page = g.pending_decision().unwrap().options.iter().find_map(|o| match o.choice {
        Choice::Page(_) => Some(o.choice),
        _ => None,
    });
    let page = page.expect("the opponent's 120 creatures page");
    g.choose(page);
    let d = g.pending_decision().unwrap().clone();
    assert!(matches!(d.stage, engine::decision::Stage::Paged { window: 1 }), "{:?}", d.stage);
    let row = Row::of(&g, 0);
    let n = check_pointers(&g, 0, &row).unwrap_or_else(|e| panic!("range page: {e}"));
    assert!(n >= 20);
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    for (k, &a) in legal.iter().enumerate() {
        if (act::OPP_PERM..act::OPP_PERM + 95).contains(&a) {
            let tok = row.ov.iter().find(|o| o.0 as usize == k).map(|o| zero_search::tail::unpack_tok(o.1));
            assert_eq!(tok, Some((seg::OPP_PERM, 0, 95 + (a - act::OPP_PERM) as u32)), "OPP_PERM + {} on page 1", a - act::OPP_PERM);
        }
    }
}

/// R17 (ENGINE.md 125): the tables' graveyard readers are encodable (the cube's and constructed
/// play's: Shallow Grave's "the top creature card of your graveyard"; EoE's none), and a reader
/// whose filter a graveyard card can change (a colour: Painter's Servant) fails the build, naming
/// the card.
#[test]
fn r17_the_graveyard_order_check() {
    use engine::lower::graveyard_order::{check_defs, readers};
    // (Shallow Grave and Corpse Dance share the reader: "the top creature card of your graveyard";
    // its key is the first of them by id.)
    let cube = readers(CardDb::cube());
    assert_eq!(cube.len(), 1, "{cube:?}");
    assert!(["Shallow Grave", "Corpse Dance"].contains(&CardDb::cube().def(cube[0].def).name.as_str()));
    assert_eq!(cube[0].filter, engine::db::Filter::Type(engine::db::types::CREATURE));
    assert_eq!(readers(CardDb::constructed()).len(), 1);
    assert!(readers(CardDb::eoe()).is_empty());
    // A synthetic card: Shallow Grave reading the top red card.
    let db = CardDb::cube();
    let sg = db.expect_id("Shallow Grave").unwrap();
    let mut d = db.def(sg).clone();
    d.name = "Synthetic Grave".into();
    for op in d.spell.as_mut().unwrap().program.iter_mut() {
        if let engine::program::Op::Ext(engine::program::Ext::TopOfGraveyardToBattlefield { filter }) = op {
            *filter = engine::db::Filter::Color(1 << 3);
        }
    }
    let e = check_defs([(0u16, &d)].into_iter()).unwrap_err();
    assert!(e.contains("Synthetic Grave"), "{e}");
}

/// The graveyard order a positional reader reads (`graveyard_order` tokens): the creature cards of
/// each graveyard, top first, with their identities; another order of them is another row, the
/// other cards' order is not encoded.
#[test]
fn graveyard_order_tokens_hold_a_readers_cards_top_first() {
    let db = CardDb::cube();
    let mut g = Scenario::new(db)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .graveyard(0, &["Ouroboroid", "Island", "Ragavan, Nimble Pilferer", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let row = Row::of(&g, 0);
    let toks = row.tokens(seg::GRAVEYARD_ORDER);
    let rag = ident(&g, db.expect_id("Ragavan, Nimble Pilferer").unwrap());
    let oro = ident(&g, db.expect_id("Ouroboroid").unwrap());
    let got: Vec<Option<usize>> = toks.iter().map(|t| code_identity(&t.1[..CODE])).collect();
    assert_eq!(got, [Some(rag), Some(oro)], "top first");
    assert_eq!(toks[0].1[CODE + te::go::RANK], 1.0 / 32.0);
    // Two non-creature cards swap: the same row; the two creatures swap: another.
    let base = Row::of(&g, 1).flat();
    let mut h = g.clone();
    board::swap_graveyard(&mut h, 0, 1, 3);
    assert_eq!(Row::of(&h, 1).flat(), base, "graveyards are otherwise unordered");
    board::swap_graveyard(&mut g, 0, 0, 2);
    assert_ne!(Row::of(&g, 1).flat(), base, "a reader's order is encoded");
}

/// Every `Mark` variant has a perturbation of the public-state gate (G1), each a perturbation
/// that exists (`public_state::mark_variant` is an exhaustive match: a new variant is a compile
/// error until it has one).
#[test]
fn every_mark_variant_has_a_perturbation() {
    use engine::public_state::{perturbations, MARK_PERTURBATIONS};
    let ps = perturbations();
    let mut variants: Vec<&str> = MARK_PERTURBATIONS.iter().map(|x| x.0).collect();
    variants.sort_unstable();
    variants.dedup();
    assert_eq!(variants.len(), MARK_PERTURBATIONS.len(), "one perturbation a variant");
    for (v, name) in MARK_PERTURBATIONS {
        let p = ps.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{v}: no perturbation {name:?}"));
        assert_eq!(p.field, "Game.marks", "{v}");
    }
    let m = engine::game::Mark::ChosenName(0);
    assert!(MARK_PERTURBATIONS.iter().any(|x| x.0 == engine::public_state::mark_variant(&m)));
}

/// ENGINE.md 142 (the Fable review's B4): every `Mod` variant a continuous effect can have has a
/// perturbation of the public-state gate (`MOD_PERTURBATIONS`), and the others are static only
/// (`STATIC_ONLY_MODS`, `Mod::static_only`: `Game::add_effect` refuses them); `mod_variant` is an
/// exhaustive match, `mod_samples` one `Mod` of each variant. The known gaps name perturbations
/// that exist.
#[test]
fn every_mod_variant_has_a_perturbation_or_is_static_only() {
    use engine::public_state::{known_gaps, mod_samples, mod_variant, perturbations, MOD_PERTURBATIONS, STATIC_ONLY_MODS};
    let ps = perturbations();
    let samples = mod_samples();
    let mut names: Vec<&str> = samples.iter().map(mod_variant).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), samples.len(), "one sample a variant");
    assert_eq!(names.len(), MOD_PERTURBATIONS.len() + STATIC_ONLY_MODS.len(), "every variant classified once");
    for m in &samples {
        let v = mod_variant(m);
        let effect = MOD_PERTURBATIONS.iter().find(|x| x.0 == v);
        let fixed = STATIC_ONLY_MODS.iter().any(|x| x.0 == v);
        assert!(effect.is_some() != fixed, "{v}: a perturbation or static only, not both");
        assert_eq!(m.static_only(), fixed, "{v}: Mod::static_only");
        // (the effects token's `Mod` one-hot, ENGINE.md 142.7: one index a variant an effect can have)
        if !fixed {
            let i = te::mod_index(m);
            assert!(i < te::ef::MODS, "{v}");
            assert_eq!(samples.iter().filter(|x| !x.static_only() && te::mod_index(x) == i).count(), 1, "{v}: its own index");
        }
        if let Some((_, name)) = effect {
            let p = ps.iter().find(|p| p.name == *name).unwrap_or_else(|| panic!("{v}: no perturbation {name:?}"));
            assert_eq!(p.field, "Game.effects", "{v}");
        }
    }
    for k in known_gaps() {
        for a in k.alone.iter().chain(&k.alike) {
            assert!(*a == engine::public_state::BASE || ps.iter().any(|p| p.name == *a), "known gap {}: no perturbation {a:?}", k.name);
        }
        assert!(!k.decision.is_empty() && k.layouts != 0, "{}", k.name);
    }
}

/// The layout v6 game `mtg_eoe` keeps layout v6 byte for byte (ENGINE.md 124.2): its layout is
/// layout_eoe.json, version 6, without a variable-length key.
#[test]
fn mtg_eoe_keeps_layout_v6() {
    let want = encode::layout_json(CardDb::eoe());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az/layout_eoe.json");
    let have = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    assert!(have == want, "layout_eoe.json is not encode::layout_json(eoe)");
    let v: serde_json::Value = serde_json::from_str(&want).unwrap();
    assert_eq!((v["version"].as_u64(), v["features"].as_u64()), (Some(6), Some(encode::NUM_FEATURES_V6 as u64)));
    for k in ["option_pointers", "max_row_tokens", "draft"] {
        assert!(v.get(k).is_none(), "{k}");
    }
    assert!(v["segments"].as_array().unwrap().iter().all(|s| s.get("tail").is_none()));
}

/// `perm::EXHAUSTED_K` holds which exhaust ability `Mark::Exhausted(k)` marks (8 bits): no card of
/// a table has an exhaust ability past its 8th activated ability (the encoder fails loudly there).
#[test]
fn every_exhaust_ability_has_a_float() {
    let mut most = 0;
    for db in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        for d in 0..db.len() as engine::ids::DefId {
            for (i, a) in db.def(d).activated.iter().enumerate() {
                if a.exhaust {
                    assert!(i < encode::perm::EXHAUSTED_K_N, "{}: exhaust ability {i}", db.def(d).name);
                    most = most.max(i + 1);
                }
            }
        }
    }
    println!("exhaust abilities up to the {most}th");
    assert!(most >= 1);
}

/// The real-checkpoint widen proof's input (ENGINE.md 125.7): `encode::v5_projection` and
/// `encode::v6_projection` as JSON (`{"v5": [...], "v6": [...]}`: each layout v5 / v6 feature's v7
/// index) at `$V6_PROJECTION_OUT`, which the proof compares with zero-core's `token_feature_map` of
/// the layouts before it reads a row's older encoding through it.
/// `cargo test --release -p engine --test misc_suite v7_tails::dump_v6_projection -- --ignored`.
#[test]
#[ignore]
fn dump_v6_projection() {
    let out = std::env::var("V6_PROJECTION_OUT").expect("V6_PROJECTION_OUT: where to write the projections");
    let (p5, p6) = (encode::v5_projection(), encode::v6_projection());
    assert_eq!(p6.len(), encode::NUM_FEATURES_V6);
    let v = serde_json::json!({"v5": p5, "v6": p6});
    std::fs::write(&out, v.to_string()).unwrap();
    println!("{} and {} indices -> {out}", p5.len(), p6.len());
}

/// The face-down cards (ENGINE.md 125.7; a limit by design until v7): a card exiled face down is a
/// `face_down` token for both viewers, its identity only for the viewers who know it, and who knows
/// it (the owner, the other player) is in every viewer's token. The gate's perturbations, applied by
/// name: an exiled card turned face down (only its owner knows it), then the other player learns it.
#[test]
fn face_down_tokens_show_who_knows_each_card() {
    use engine::public_state::perturbations;
    let db = CardDb::cube();
    let sp = MtgSpec::parse(r#"{"max_turns": 60}"#).unwrap();
    let mut a = MtgAz::new_game(&sp, 2, 3).unwrap();
    a.g = Scenario::new(db)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .exile(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = ident(&a.g, db.expect_id("Lightning Bolt").unwrap());
    let apply = |a: &mut MtgAz, name: &str| {
        let p = perturbations().into_iter().find(|p| p.name == name).unwrap();
        (p.apply)(a, &mut engine::rng::Rng::new(1)).unwrap_or_else(|| panic!("{name}: not applied"))
    };
    let fd = |a: &MtgAz, v: u8| Row::of(&a.g, v).tokens(seg::FACE_DOWN).into_iter().map(|(s, x)| (s, x.to_vec())).collect::<Vec<_>>();
    assert!(fd(&a, 0).is_empty() && fd(&a, 1).is_empty(), "a face-up exiled card: no token");
    apply(&mut a, "an exiled card is turned face down");
    let (mine, theirs) = (fd(&a, 0), fd(&a, 1));
    assert_eq!((mine.len(), theirs.len()), (1, 1));
    let (m, t) = (&mine[0].1, &theirs[0].1);
    assert_eq!(code_identity(&m[..CODE]), Some(bolt), "its owner knows it");
    assert_eq!(code_identity(&t[..CODE]), None, "the other player doesn't");
    use te::fd;
    assert_eq!((m[CODE + fd::EXILED], m[CODE + fd::OWNER_MINE], m[CODE + fd::OWNER_KNOWS], m[CODE + fd::OTHER_KNOWS], m[CODE + fd::VIEWER_KNOWS]), (1.0, 1.0, 1.0, 0.0, 1.0));
    assert_eq!((t[CODE + fd::OWNER_THEIRS], t[CODE + fd::OWNER_KNOWS], t[CODE + fd::OTHER_KNOWS], t[CODE + fd::VIEWER_KNOWS]), (1.0, 1.0, 0.0, 0.0));
    let before = [Row::of(&a.g, 0).flat(), Row::of(&a.g, 1).flat()];
    apply(&mut a, "the other player learns a face-down exiled card");
    let (mine, theirs) = (fd(&a, 0), fd(&a, 1));
    assert_eq!(code_identity(&theirs[0].1[..CODE]), Some(bolt), "the other player knows it now");
    assert_eq!((mine[0].1[CODE + fd::OTHER_KNOWS], theirs[0].1[CODE + fd::VIEWER_KNOWS]), (1.0, 1.0));
    for v in 0..2u8 {
        assert_ne!(Row::of(&a.g, v).flat(), before[v as usize], "P{v}'s row shows the other player's knowledge");
    }
}

/// ENGINE.md 142.3 (the Fable review's B7): Memory Jar exiles each hand face down in the hand
/// vector's order (`Game::face_down`). A hand has no order, so two games whose only difference is
/// the place of the one card P0 knows among P1's hidden cards are one information state for P0:
/// its rows are equal (the `face_down` tokens, their references and the `links` tokens are in the
/// canonical order `tail_encode::face_down_order`, not the pile's). P1 (who knows its whole
/// former hand) sees equal rows too.
#[test]
fn memory_jars_face_down_pile_shows_no_hand_order() {
    let db = CardDb::cube();
    let play = |known_first: bool| {
        let s = Scenario::new(db)
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 12])
            .library(1, &["Island"; 12])
            .battlefield(0, &["Memory Jar"])
            .hand(0, &["Lightning Bolt"]);
        let s = if known_first {
            s.hand_known(1, &["Ponder"]).hand(1, &["Counterspell", "Brainstorm"])
        } else {
            s.hand(1, &["Counterspell", "Brainstorm"]).hand_known(1, &["Ponder"])
        };
        let mut g = s.at(Step::Main1, 0).start();
        let jar = g.card(0, "Memory Jar", Zone::Battlefield);
        g.choose(Choice::Ability(jar, engine::db::AbilitySlot::Activated(0)));
        for _ in 0..200 {
            if g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority) {
                break;
            }
            if g.kind() == Some(DecisionKind::Priority) {
                g.pass();
            } else {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
        assert!(g.stack().is_empty(), "the Jar's ability resolved");
        let ponder = g.card(1, "Ponder", Zone::FaceDown);
        assert!(g.knows(0, ponder) && g.knows(1, ponder));
        let order: Vec<&str> = g.face_down_pile().iter().filter(|&&c| g.obj(c).owner == 1).map(|&c| g.name(c)).collect();
        (g, order)
    };
    let ((a, oa), (b, ob)) = (play(true), play(false));
    assert_ne!(oa, ob, "the piles are in different orders");
    for v in 0..2u8 {
        assert_eq!(a.view_of(v), b.view_of(v), "one information state for P{v}");
        let (ra, rb) = (Row::of(&a, v), Row::of(&b, v));
        assert!(!ra.tokens(seg::FACE_DOWN).is_empty());
        assert_eq!(ra.flat(), rb.flat(), "P{v}'s rows: the pile's order is not shown");
    }
}

/// ENGINE.md 142.7: the `effects`, `counters` and `numbers` tokens on a hand-made board, both
/// viewers: an effect's `Mod`, parameters, duration and controller; a group effect's filter; a copy
/// effect; counters by kind with exact counts; a number past float16's exact range (and none below).
#[test]
fn effects_counters_and_numbers_tokens() {
    use engine::db::{Counter, Mod, Prot};
    use engine::public_state::perturbations;
    use te::{ct, ef, nb};
    let db = CardDb::cube();
    let sp = MtgSpec::parse(r#"{"max_turns": 60}"#).unwrap();
    let mut a = MtgAz::new_game(&sp, 2, 3).unwrap();
    a.g = Scenario::new(db)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Mother of Runes", "Plains"])
        .battlefield(1, &["Thraben Inspector", "Mountain"])
        .counters(0, "Plains", Counter::Stun, 1)
        .counters(1, "Thraben Inspector", Counter::P1P1, 70_000)
        .life(1, 5_000)
        .at(Step::Main1, 0)
        .start();
    let apply = |a: &mut MtgAz, name: &str| {
        let p = perturbations().into_iter().find(|p| p.name == name).unwrap();
        (p.apply)(a, &mut engine::rng::Rng::new(1)).unwrap_or_else(|| panic!("{name}: not applied"))
    };
    let tok = |a: &MtgAz, v: u8, sg: u16| Row::of(&a.g, v).tokens(sg).into_iter().map(|(s, x)| (s, x.to_vec())).collect::<Vec<_>>();
    assert!(tok(&a, 0, seg::EFFECTS).is_empty(), "no effect yet");
    // counters: the stun counter and the +1/+1 counters, kinds and exact counts, for both viewers
    for v in 0..2u8 {
        let cs = tok(&a, v, seg::COUNTERS);
        assert_eq!(cs.len(), 2, "P{v}: two (object, kind) tokens");
        let kind = |x: &[f32]| (0..ct::KINDS).find(|&k| x[CODE + ct::KIND + k] == 1.0);
        let count = |x: &[f32]| (0..64).map(|b| (x[CODE + ct::COUNT_BITS + b] as u64) << b).sum::<u64>();
        let mut got: Vec<(Option<usize>, u64)> = cs.iter().map(|(_, x)| (kind(x), count(x))).collect();
        got.sort();
        assert_eq!(got, [(Some(0), 70_000), (Some(2), 1)], "P{v}: +1/+1 x 70,000 and stun x 1");
        // numbers: P1's life 5,000 and the Inspector's power and toughness past `BIG`; P0's life 20 none
        let ns = tok(&a, v, seg::NUMBERS);
        let which: Vec<usize> = ns.iter().map(|(_, x)| (0..nb::WHICH_N).find(|&k| x[CODE + nb::WHICH + k] == 1.0).unwrap()).collect();
        assert_eq!(which, [nb::LIFE, nb::POWER, nb::TOUGHNESS], "P{v}");
        let life = &ns[0].1;
        assert_eq!((0..64).map(|b| (life[CODE + nb::BITS + b] as u64) << b).sum::<u64>(), 5_000);
        assert_eq!(life[CODE + if v == 1 { nb::PLAYER_ME } else { nb::PLAYER_OPP }], 1.0, "P{v}: P1's life");
    }
    // effects: protection from red until end of turn, +1/+1 until the other player's next turn
    apply(&mut a, "a creature gains protection from red");
    apply(&mut a, "+1/+1 until the other player's next turn");
    let es = tok(&a, 0, seg::EFFECTS);
    assert_eq!(es.len(), 2);
    let (prot, pump) = (&es[0].1[CODE..], &es[1].1[CODE..]);
    assert_eq!((prot[ef::MOD + 3], prot[ef::PROT + 3], prot[ef::PROT], prot[ef::UNTIL]), (1.0, 1.0, 0.0, 1.0), "protection from red until end of turn");
    assert_eq!((pump[ef::MOD + 5], pump[ef::PT_PRESENT], pump[ef::POWER + 2], pump[ef::UNTIL + 2]), (1.0, 1.0, 1.0, 1.0), "+1/+1 until a next turn");
    assert_eq!(pump[ef::CONTROLLER_ME] + pump[ef::CONTROLLER_OPP], 1.0);
    // which protection, which duration: different rows (the gaps of ENGINE.md 142.2)
    let mut b = a.clone();
    let e = b.g.effects_mut().iter_mut().find(|e| matches!(e.m, Mod::Protection(_))).unwrap();
    e.m = Mod::Protection(Prot { colors: 1 << 2, ..Default::default() });
    let mut c = a.clone();
    c.g.effects_mut()[1].until = engine::Until::Forever;
    for v in 0..2u8 {
        assert_ne!(Row::of(&a.g, v).flat(), Row::of(&b.g, v).flat(), "P{v}: protection from red vs black");
        assert_ne!(Row::of(&a.g, v).flat(), Row::of(&c.g, v).flat(), "P{v}: until a next turn vs no duration");
    }
    // a group effect: "combat damage is prevented this turn" (Darkness), its filter
    apply(&mut a, "combat damage is prevented this turn (a rules effect)");
    let g3 = &tok(&a, 1, seg::EFFECTS)[2].1[CODE..];
    assert_eq!((g3[ef::GROUP], g3[ef::GROUP_TYPES + 1], g3[ef::GROUP_ANY], g3[ef::MOD + 19]), (1.0, 1.0, 1.0, 1.0), "creatures, any player's");
    // a copy effect, after the effects
    apply(&mut a, "a permanent becomes a copy of another");
    let es = tok(&a, 0, seg::EFFECTS);
    let cp = &es.last().unwrap().1[CODE..];
    assert_eq!((cp[ef::COPY], cp[ef::UNTIL]), (1.0, 1.0), "a copy effect until end of turn");
    assert!(cp[ef::COPY_DEF..ef::COPY_DEF + 32].iter().any(|&x| x == 1.0), "the copied definition's identity");
}

/// ENGINE.md 142.7: the stable name ids (`tail_encode::name_id`: subtypes and named counters by
/// name, the same id in every table) of every interned name of the tables are distinct.
#[test]
fn name_ids_are_distinct() {
    let mut all: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
    for db in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        for n in db.subtype_names() {
            let id = te::name_id(n);
            if let Some(o) = all.insert(id, n.clone()) {
                assert_eq!(&o, n, "{o:?} and {n:?} share the name id {id:#x}");
            }
        }
    }
    assert!(all.len() > 100, "{} names", all.len());
}

/// ENGINE.md 142.7: every group (rules) effect a table's cards make (`Ext::ApplyRules`) has a
/// filter the `effects` token writes (`tail_encode::group_filter`: else a loud failure), and every
/// effect's subtypes fit the token (`ef::SUBS`); the three tables, every ability and mode.
#[test]
fn every_tables_effects_fit_the_effects_token() {
    use engine::program::{Ext, Op};
    let mut groups = 0;
    for db in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        for d in 0..db.len() as engine::ids::DefId {
            let def = db.def(d);
            let mut abs: Vec<&engine::db::AbilityDef> = def.spell.iter().chain(def.triggers.iter().map(|t| &t.ability)).collect();
            for a in def.activated.iter().chain(&def.hand_abilities).chain(&def.gy_abilities) {
                if let engine::db::ActEffect::Stack(ab) = &a.effect {
                    abs.push(ab);
                }
            }
            let mut all = Vec::new();
            for ab in abs {
                all.push(ab);
                if let Some(m) = &ab.modes {
                    all.extend(m.list.iter());
                }
            }
            for ab in all {
                for op in &ab.program {
                    let mods: Vec<&engine::db::Mod> = match op {
                        Op::Ext(Ext::ApplyRules { filter, m, .. }) => {
                            te::group_filter(filter);
                            groups += 1;
                            vec![m]
                        }
                        Op::Ext(Ext::Apply { mods, .. }) | Op::Ext(Ext::ReturnEventCardAs { mods }) => mods.iter().collect(),
                        _ => vec![],
                    };
                    for m in mods {
                        let n = match m {
                            engine::db::Mod::Types { subtypes, .. } | engine::db::Mod::BecomeTypes { subtypes, .. } => subtypes.len(),
                            _ => 0,
                        };
                        assert!(n <= te::ef::SUBS, "{}: an effect with {n} subtypes", def.name);
                        assert!(!m.static_only(), "{}: an effect with a static ability's mod {m:?}", def.name);
                    }
                }
            }
        }
    }
    assert!(groups > 0, "no group effect in the tables");
}

/// The identities a player revealed in earlier games of a match (ENGINE.md 125.7; a limit by
/// design until v7): `my_revealed` tokens of that player's rows (what the other player knows of its
/// pool), the other player's bag `OPP_REVEALED`; a head-only `MtgAz::encode` of a row with any
/// refuses (R1).
#[test]
fn my_revealed_tokens_hold_the_frames_earlier_reveals() {
    use engine::az::encode::{bag, D, O_BAGS};
    let sp = MtgSpec::parse(r#"{"max_turns": 60, "format": "bo3"}"#).unwrap();
    let mut a = MtgAz::new_game(&sp, 2, 5).unwrap();
    assert!(Row::of_az(&a, 0).tail.iter().all(|t| t.0 != seg::MY_REVEALED));
    let base = [Row::of_az(&a, 0), Row::of_az(&a, 1)];
    a.m.earlier[0][1] |= 1 << 3; // identity 67
    a.m.earlier[0][0] |= 1 << 9; // identity 9
    let r0 = Row::of_az(&a, 0);
    let toks = r0.tokens(seg::MY_REVEALED);
    assert_eq!(toks.iter().map(|t| (t.0, code_identity(&t.1[..CODE]))).collect::<Vec<_>>(), [(9, Some(9)), (67, Some(67))]);
    assert!(toks.iter().all(|t| t.1[CODE + te::mr::PRESENT] == 1.0));
    assert_eq!(r0.head, base[0].head, "P0's own reveals: tokens only");
    let r1 = Row::of_az(&a, 1);
    assert!(r1.tokens(seg::MY_REVEALED).is_empty());
    assert_eq!(r1.head[O_BAGS + bag::OPP_REVEALED * D + 67], 0.25, "P1 sees them in OPP_REVEALED");
    // ENGINE.md 142.8: and as `opp_revealed` tokens, the same token as P0's own; P0's row has no
    // `opp_revealed` (P1 revealed nothing)
    let ot: Vec<(u32, Option<usize>)> = r1.tokens(seg::OPP_REVEALED).iter().map(|t| (t.0, code_identity(&t.1[..CODE]))).collect();
    assert_eq!(ot, [(9, Some(9)), (67, Some(67))]);
    assert!(r0.tokens(seg::OPP_REVEALED).is_empty());
    // the games the reveals were made in (the user's decision of 2026-11-13: how a player boarded game 2
    // is an input for game 3): identity 67 in game 2, 9 in game 1 - and the same reveals made in the
    // other games are another row for both players
    a.m.earlier_by_game[0][1][1] |= 1 << 3;
    a.m.earlier_by_game[0][0][0] |= 1 << 9;
    let game = |r: &Row, sg: u16, slot: u32| -> Vec<f32> { r.token(sg, slot).unwrap()[CODE + te::mr::GAME..CODE + te::mr::GAME + 3].to_vec() };
    let (r0, r1) = (Row::of_az(&a, 0), Row::of_az(&a, 1));
    assert_eq!((game(&r0, seg::MY_REVEALED, 67), game(&r1, seg::OPP_REVEALED, 67)), (vec![0.0, 1.0, 0.0], vec![0.0, 1.0, 0.0]));
    assert_eq!((game(&r0, seg::MY_REVEALED, 9), game(&r1, seg::OPP_REVEALED, 9)), (vec![1.0, 0.0, 0.0], vec![1.0, 0.0, 0.0]));
    let mut c = a.clone();
    c.m.earlier_by_game[0][1][1] &= !(1 << 3);
    c.m.earlier_by_game[0][0][1] |= 1 << 3;
    for v in 0..2 {
        assert_ne!(Row::of_az(&a, v).flat(), Row::of_az(&c, v).flat(), "P{v}: a reveal of game 2 vs one of game 1");
    }
    let b = a.clone();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut x = vec![0.0f32; NUM_FEATURES];
        b.encode(0, &mut x);
    }));
    assert!(r.is_err(), "MtgAz::encode of a row with my_revealed tokens");
}

/// A chosen name is a face's (ENGINE.md 125.11; Disruptor Flute names the faces and halves of the
/// game's cards, ENGINE.md 123.4): a split card's two halves encode apart - the card's identity and
/// the face (left half 4, right half 5) - and `NO_NAME` is its own value; `encode::name_face` names
/// every face of the table (each face definition is one of its card's faces).
#[test]
fn chosen_names_are_faces() {
    use engine::az::encode::{name_face, perm};
    let db = CardDb::cube();
    let (l, r) = (0..db.num_cards() as engine::ids::DefId).find_map(|d| db.def(d).halves).expect("a split card in the cube");
    let card = db.def(l).front.expect("a half's card");
    assert_eq!((name_face(db, l), name_face(db, r), name_face(db, card)), ((card, 4), (card, 5), (card, 0)));
    for t in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        for d in 0..t.len() as engine::ids::DefId {
            let (c, f) = name_face(t, d);
            assert!(f <= 5 && (f == 0) == (c == d || t.def(d).name == t.def(c).name || t.def(d).name == engine::sneak_show::face_name(t.def(c))));
        }
    }
    for d in 0..db.len() as engine::ids::DefId {
        let (c, f) = name_face(db, d);
        assert!(f <= 5 && (f == 0) == (c == d || db.def(d).name == db.def(c).name || db.def(d).name == engine::sneak_show::face_name(db.def(c))));
    }
    let sp = MtgSpec::parse(r#"{"max_turns": 60}"#).unwrap();
    let mut a = MtgAz::new_game(&sp, 2, 3).unwrap();
    a.g = Scenario::new(db).library(0, &["Island"; 10]).library(1, &["Island"; 10]).battlefield(0, &["Island"]).at(Step::Main1, 0).start();
    let face = |a: &MtgAz| {
        let row = Row::of(&a.g, 0);
        let x = row.token(seg::MY_PERM, 0).unwrap().to_vec();
        (code_identity(&x[CODE + perm::CHOSEN_NAME_DIGITS..CODE + perm::CHOSEN_NAME_DIGITS + 32]), (0..3).map(|b| x[CODE + perm::CHOSEN_NAME_FACE + b]).collect::<Vec<_>>())
    };
    let (mut al, mut ar) = (a.clone(), a.clone());
    engine::public_state::name_a_split_half(&mut al, true).unwrap();
    engine::public_state::name_a_split_half(&mut ar, false).unwrap();
    let want = Some(ident(&a.g, card));
    assert_eq!(face(&al), (want, vec![0.0, 0.0, 1.0]), "the left half: face 4");
    assert_eq!(face(&ar), (want, vec![1.0, 0.0, 1.0]), "the right half: face 5");
    assert_ne!(Row::of(&al.g, 1).flat(), Row::of(&ar.g, 1).flat(), "the other player's row too");
}

/// ENGINE.md 142.12: Vampiric Tutor's card, put on top of the library unrevealed, is a find the other
/// player tells apart (`Game::found`, ENGINE.md 140.9; kept with whole-deck clouds): a `found` token in
/// both players' rows - the card's identity in its owner's row (it knows the card), none in the
/// other's -, its depth (the top), its owner; drawn, it is a hand card and no find.
#[test]
fn a_tutors_card_on_top_is_a_found_token() {
    use te::{fs, rf};
    let db = CardDb::cube();
    let mut g = Scenario::new(db)
        .spec(|s| {
            s.auto_single = false;
            s.deck_clouds = true;
        })
        .library(0, &["Forest", "Island", "Plains", "Island", "Swamp"])
        .library(1, &["Island"; 8])
        .battlefield(0, &["Swamp"])
        .hand(0, &["Vampiric Tutor"])
        .at(Step::Main1, 0)
        .start();
    let found = |g: &Game, v: u8| Row::of(g, v).tokens(seg::FOUND).into_iter().map(|(s, x)| (s, x.to_vec())).collect::<Vec<_>>();
    assert!(found(&g, 0).is_empty() && found(&g, 1).is_empty());
    let vt = g.card(0, "Vampiric Tutor", Zone::Hand);
    g.choose(Choice::Cast(vt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    let plains = db.expect_id("Plains").unwrap();
    g.choose(Choice::Def(plains));
    g.drive_until(|g| g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority));
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Plains");
    for v in 0..2u8 {
        let t = found(&g, v);
        assert_eq!(t.len(), 1, "P{v}: one find");
        let (slot, x) = &t[0];
        assert_eq!(*slot, 0);
        assert_eq!(code_identity(&x[..CODE]), (v == 0).then(|| ident(&g, plains)), "P{v}: the identity in its owner's row only");
        let s = &x[CODE..];
        assert_eq!((s[fs::PRESENT], s[fs::OBJ + rf::PRESENT], s[fs::DEPTH]), (1.0, 1.0, 1.0 / 64.0), "P{v}: the top of the library");
        assert_eq!(s[fs::OBJ + if v == 0 { rf::MINE } else { rf::THEIRS }], 1.0, "P{v}: P0's card");
    }
    // P0's next draw: a hand card, no find
    let t0 = g.turn().number;
    g.drive_until(|g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1);
    assert_eq!(g.obj(top).zone, Zone::Hand, "{}", g.log_lines().join("\n"));
    assert!(found(&g, 0).is_empty() && found(&g, 1).is_empty());
}

