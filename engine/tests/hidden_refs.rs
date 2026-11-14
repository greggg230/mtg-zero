//! ENGINE.md 156: an object reference to a card in a hidden zone shows what the viewer knows of
//! that object, the same in every world of the viewer's information set.
//!
//! * A hand card a reference of the public state names (an ability's source revealed by its
//!   activation, CR 602.2a, 701.20a; a card revealed for miracle, 702.94b; ...) is told apart from
//!   its twins (`Game::told_apart`): `canon_hand_knowledge` and `knowledge_leaves` leave its
//!   knowledge alone, so the reference's identity is the same in every world (before 156, a world
//!   that dealt an unknown twin below it moved the viewer's knowledge onto the twin: lane G's
//!   fuzz seeds, cross 1503 and others).
//! * A stale reference (the object is gone, CR 400.7) shows the identity the object had as it
//!   left its zone where the viewer knew it then (`Game::ref_lki`), never the card's identity
//!   now (delver 4252 and others).
//! * The other player's pick of a card of its own hand (Show and Tell's face-down choice, CR
//!   101.4a; a cost's picks) is a `links` token with no identity (cross 4799, 5450, 8706).

use engine::az::encode::{self, stack, CODE, O_STACK};
use engine::az::tail_encode::{ev, lk, rf, seg, Row};
use engine::db::{AbilitySlot, CardDb};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn id(name: &str) -> u16 {
    db().expect_id(name).unwrap()
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

/// `viewer`'s rows in `n` worlds `determinize(viewer)` deals of `g`, with whether each world dealt
/// `twin` the definition `d`.
fn world_rows(g: &Game, viewer: PlayerId, n: u64, twin: CardIdx, d: u16) -> Vec<(Vec<f32>, bool)> {
    (0..n)
        .map(|s| {
            let mut w = g.clone();
            w.determinize(viewer, &mut Rng::new(s));
            (Row::of(&w, viewer).flat(), w.obj(twin).def == d)
        })
        .collect()
}

const EDGE: &str = "Edge of Autumn";

/// P0 cycles Edge of Autumn ("Cycling - Sacrifice a land"): the activation reveals it (CR 602.2a,
/// 701.20a: revealed until the ability leaves the stack) and it stays in P0's hand while P0
/// chooses the land, the cycling ability's source. P0's hand also holds a card P1 doesn't know at
/// a lower card index, which some of P1's worlds deal as another Edge of Autumn. In every world P1's
/// row is the same, and the stack ability's source shows Edge of Autumn (before ENGINE.md 156 the
/// worlds with the twin moved P1's knowledge of the revealed card onto it, `canon_hand_knowledge`,
/// and the source's identity was gone: lane G's fuzz seed cross 1503).
#[test]
fn cr_602_2a_a_revealed_sources_identity_is_the_same_in_every_world() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Brainstorm", EDGE])
        .battlefield(0, &["Swamp", "Island"])
        .library(0, &[EDGE, EDGE, EDGE, "Swamp", "Island", "Forest"])
        .library(1, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let twin = g.card(0, "Brainstorm", Zone::Hand);
    let edge = g.card(0, EDGE, Zone::Hand);
    assert!(twin.get() < edge.get(), "the unknown card first by card index");
    assert!(g.has_option(Choice::Ability(edge, AbilitySlot::Hand(0))), "{}", opts(&g));
    g.choose(Choice::Ability(edge, AbilitySlot::Hand(0)));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice), "{}\n{}", opts(&g), log(&g));
    assert_eq!(g.obj(edge).zone, Zone::Hand, "the source waits in the hand while its costs are paid");
    assert!(g.knows(1, edge) && !g.knows(1, twin), "revealed; the other card unknown to P1");
    let real = Row::of(&g, 1).flat();
    let src = O_STACK + CODE + stack::SOURCE;
    assert!(real[src + rf::DIGITS..src + rf::DIGITS + 32].iter().any(|&x| x != 0.0), "P1 sees the source's identity");
    let rows = world_rows(&g, 1, 200, twin, id(EDGE));
    assert!(rows.iter().any(|r| r.1), "some worlds deal the unknown card as another Edge of Autumn");
    for (s, (row, _)) in rows.iter().enumerate() {
        assert!(*row == real, "world {s}: P1's row differs from the real game's (one information set)");
    }
}

const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
const SCHOLAR: &str = "Tamiyo, Seasoned Scholar";

/// Tamiyo, Seasoned Scholar's -3 returns Daze from P1's graveyard to P1's hand; Murktide Regent's
/// "whenever an instant or sorcery card leaves your graveyard" triggers, its event's object the
/// Daze as it was in the graveyard (CR 603.10a), a reference that is stale now (CR 400.7: the
/// card in the hand is a new object). P1's hand also holds a card P0 doesn't know at a lower
/// card index, which some of P0's worlds deal as another Daze. The trigger's event object shows
/// the identity the Daze had as it left the graveyard (public: `Game::ref_lki`), in every world:
/// before ENGINE.md 156 it showed the hand card's identity where P0 knew it now, and the worlds
/// with the twin had moved P0's knowledge onto the twin (lane G's fuzz seed delver 4252).
#[test]
fn cr_400_7_a_stale_reference_shows_the_identity_its_object_had_as_it_left() {
    use engine::db::Counter;
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Daze", "Daze", "Daze", "Island", "Island", "Island"])
        .hand(1, &["Brainstorm"])
        .battlefield(1, &[TAMIYO, "Murktide Regent", "Island"])
        .transformed(1, TAMIYO)
        .counters(1, SCHOLAR, Counter::Loyalty, 3)
        .graveyard(1, &["Daze"])
        .at(Step::Main1, 1)
        .start();
    let twin = g.card(1, "Brainstorm", Zone::Hand);
    let daze = g.card(1, "Daze", Zone::Graveyard);
    assert!(twin.get() < daze.get(), "the unknown card first by card index");
    let t = g.card(1, SCHOLAR, Zone::Battlefield);
    let minus3 = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Ability(c, s) if c == t => Some(Choice::Ability(c, s)),
            _ => None,
        })
        .nth(1)
        .expect("the -3");
    g.choose(minus3);
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", opts(&g));
    g.choose(g.tgt(daze));
    // Resolve the -3; Murktide's trigger goes on the stack.
    for _ in 0..100 {
        let murktide_on_stack = g.stack().iter().any(|s| s.ev.obj.is_some_and(|r| r.idx == daze));
        if murktide_on_stack && g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        if g.kind() == Some(DecisionKind::Priority) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    let item = g.stack().iter().find(|s| s.ev.obj.is_some_and(|r| r.idx == daze)).expect("Murktide's trigger on the stack").clone();
    let r = item.ev.obj.unwrap();
    assert!(!g.live(r) && g.obj(daze).zone == Zone::Hand, "a stale reference; the card is in P1's hand\n{}", log(&g));
    assert!(g.knows(0, daze) && !g.knows(0, twin), "P0 saw it return; the other card unknown to P0");
    let top = g.stack().len() - 1 - g.stack().iter().position(|s| s.id == item.id).unwrap();
    let viewer = 0;
    let real = Row::of(&g, viewer).flat();
    let at = O_STACK + top * encode::STACK_W + CODE + stack::EV + ev::OBJ + rf::DIGITS;
    assert!(real[at..at + 32].iter().any(|&x| x != 0.0), "P0 sees the event object's identity (it left a public zone)");
    let rows = world_rows(&g, viewer, 200, twin, id("Daze"));
    assert!(rows.iter().any(|r| r.1), "some worlds deal the unknown card as another Daze");
    for (s, (row, _)) in rows.iter().enumerate() {
        assert!(*row == real, "world {s}: P0's row differs from the real game's (one information set)");
    }
}

const SNT: &str = "Show and Tell";
const EMRAKUL: &str = "Emrakul, the Aeons Torn";
const GRIS: &str = "Griselbrand";

/// The links tokens of kind `kind` in `row`.
fn links_of(row: &Row, kind: usize) -> Vec<Vec<f32>> {
    row.tail.iter().filter(|(s, _, x)| *s == seg::LINKS && x[CODE + lk::KIND + kind] == 1.0).map(|(_, _, x)| x.clone()).collect()
}

/// Show and Tell: P0 (active) chooses Emrakul face down (CR 101.4a; its ruling: "a player does
/// not have to reveal the chosen card, so long as it is clear which card was chosen"); P1, who
/// knows P0 holds an Emrakul (seen earlier) but not P0's other cards, decides next knowing only
/// that a card was chosen (101.4b, the frame's stage), and its worlds draw the choice again among
/// P0's cards of the kind (`hidden_pick_ok`: the hand is one pool). P1's row has one pick token
/// of the other player with no identity, in every world (before ENGINE.md 156 the token showed
/// the card where P1 knew it: present in the worlds whose pick was the known Emrakul, absent in
/// the others; lane G's fuzz seeds cross 4799, 5450, 8706).
#[test]
fn cr_101_4a_the_other_players_face_down_pick_shows_no_identity_in_any_world() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &[GRIS, "Island", EMRAKUL, "Island", "Brainstorm", "Island", GRIS, "Island"])
        .library(1, &["Island"; 8])
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &[SNT, GRIS, "Island"])
        .hand_known(0, &[EMRAKUL])
        .hand(1, &["Atraxa, Grand Unifier"])
        .at(Step::Main1, 0)
        .start();
    let snt = g.card(0, SNT, Zone::Hand);
    g.choose(Choice::Cast(snt));
    for _ in 0..50 {
        if g.kind() == Some(DecisionKind::ChooseHandCard) {
            break;
        }
        if g.kind() == Some(DecisionKind::Priority) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}\n{}", opts(&g), log(&g));
    assert_eq!(g.pending_decision().unwrap().player, 0);
    let emrakul = g.card(0, EMRAKUL, Zone::Hand);
    g.choose(Choice::Card(emrakul));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}\n{}", opts(&g), log(&g));
    assert_eq!(g.pending_decision().unwrap().player, 1, "P1 chooses second");
    let real = Row::of(&g, 1);
    let picks = links_of(&real, lk::K_PICKED);
    assert_eq!(picks.len(), 1, "one pick in progress, the other player's");
    let p = &picks[0];
    assert_eq!(p[CODE + lk::BY_OPP], 1.0);
    assert!(p[..CODE].iter().all(|&x| x == 0.0), "no card code");
    assert!(p[CODE + lk::B + rf::DIGITS..CODE + lk::B + rf::DIGITS + 32].iter().all(|&x| x == 0.0), "no identity");
    // (Each world draws the choice again among P0's cards of the kind: the known Emrakul in some,
    // a dealt card P1 doesn't know in others.)
    let real = real.flat();
    for s in 0..200 {
        let mut w = g.clone();
        w.determinize(1, &mut Rng::new(s));
        assert!(Row::of(&w, 1).flat() == real, "world {s}: P1's row differs from the real game's (one information set)");
    }
}
