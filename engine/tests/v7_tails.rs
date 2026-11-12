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
    assert_eq!(tails.len(), 14);
    let names: Vec<&str> = tails.iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "my_permanents", "opponent_permanents", "hand", "stack", "triggers", "list", "graveyard_abilities", "opponent_triggers",
            "delayed", "links", "exile_offered", "library_known", "stack_relations", "graveyard_order"
        ]
    );
    // (Layout v6's game keeps no tail: ENGINE.md 124.2.)
    assert!(<engine::az::eoe::MtgEoeAz as SearchGame>::TAILS.is_empty());
    assert_eq!(<engine::az::eoe::MtgEoeAz as SearchGame>::NUM_FEATURES, encode::NUM_FEATURES_V6);
    assert_eq!(NUM_FEATURES, 56_192);
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
/// `cargo test --release -p engine --test v7_tails -- --ignored dump_v6_projection`.
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
