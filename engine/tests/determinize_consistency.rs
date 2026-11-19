//! ENGINE.md 210: `determinize` keeps what the viewer sees and the viewer's pending decision (the fuzz steward's
//! queue entries 2076692a1ae6 / 8dd5a36f4ce0 and 88c42634bd1f).
//!
//! * A payment of no mana (a {0} spell, a cost whose Phyrexian pips were paid with life) asked "`PASS`, or a source
//!   instead" on its first run - a source that pays nothing, a filter land's or Wizard's Rockets' - and finished at
//!   once when run again, so the decision was not one its asker asks again (`refresh_pending`): a world dealt for the
//!   other player ran past it (the spell cast, priority), and the fuzz profile's check of the decider's own world
//!   failed. Now such a payment asks nothing (CR 601.2g: "if the total cost includes a mana payment").
//! * A card revealed for miracle stays revealed while its trigger waits or is on the stack (CR 702.94b). Its owner's
//!   Brainstorm put cards back (a private choice: the other player loses track of the hand, `Game::scramble`) and the
//!   other player forgot the revealed card too: its worlds dealt the miracle card anew, the trigger then had no miracle
//!   card to ask about, and the world ran past the owner's "cast it?" decision. Now a told-apart card's knowledge stays.

use engine::db::CardDb;
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn db() -> &'static CardDb {
    CardDb::cube()
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// At the pending decision: its asker run again asks it again (`Game::refresh_decision`), and the worlds
/// `determinize` deals for either player show that player what the game shows it (the decider's own decision kept; the
/// other player's worlds refresh the decider's decision and must find it again).
fn worlds_agree(g: &Game, what: &str) {
    let mut again = g.clone();
    again.refresh_decision();
    assert_eq!(again.pending_decision(), g.pending_decision(), "{what}: the decision asked again\n{}", log(g));
    for viewer in 0..2 as PlayerId {
        for s in 0..24 {
            let mut w = g.clone();
            w.determinize(viewer, &mut Rng::new(s));
            assert_eq!(w.to_move(), g.to_move(), "{what}: P{viewer}'s world {s}: the decider");
            assert_eq!(w.view_of(viewer), g.view_of(viewer), "{what}: P{viewer}'s world {s}: the view\n{}", log(g));
        }
    }
}

/// The first option that is not `PASS` (a payment's source).
fn a_source(g: &Game) -> Choice {
    *g.offered().iter().find(|c| **c != Choice::Pass).unwrap_or_else(|| panic!("no source: {:?}\n{}", g.options_desc(), log(g)))
}

/// CR 601.2g: "If the total cost includes a mana payment, the player then has a chance to activate mana abilities."
/// Mox Pearl's cost is {0}: P0 casts it beside Wizard's Rockets ("{X}, {T}, Sacrifice this artifact: Add X mana in any
/// combination of colors", a source the payment offered "instead" of nothing, X = 0), and the payment asks nothing:
/// the spell is on the stack and P0 holds priority. Before ENGINE.md 210 the payment asked "`PASS`, or Wizard's
/// Rockets instead" on its first run and finished at once when asked again: P1's worlds (which refresh P0's decision)
/// ran past it to the cast. (The fuzz seeds of 210 add Arena of Glory's and Sunken Ruins' abilities with a mana cost:
/// `fuzz.rs` `determinize_keeps_the_viewers_decision_and_view_default_seeds`.)
#[test]
fn cr_601_2g_a_payment_of_no_mana_asks_nothing() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Mox Pearl"])
        .battlefield(0, &["Wizard's Rockets", "Mountain", "Island"])
        .hand(1, &["Lightning Bolt", "Counterspell", "Brainstorm"])
        .library(0, &["Island"; 8])
        .library(1, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let pearl = g.card(0, "Mox Pearl", Zone::Hand);
    worlds_agree(&g, "before the cast");
    g.choose(Choice::Cast(pearl));
    worlds_agree(&g, "after the cast");
    assert_eq!(g.kind(), Some(DecisionKind::Priority), "{:?}\n{}", g.options_desc(), log(&g));
    assert_eq!(g.obj(pearl).zone, Zone::Stack, "Mox Pearl was cast");
}

/// The same for a cost whose Phyrexian pip is paid with 2 life (CR 107.4f): Gitaxian Probe's {U/P}. After the life
/// choice nothing is left to pay, and nothing more is asked of the payment.
#[test]
fn cr_601_2g_a_phyrexian_cost_paid_with_life_asks_nothing_more() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Gitaxian Probe"])
        .battlefield(0, &["Wizard's Rockets", "Swamp"])
        .hand(1, &["Lightning Bolt", "Counterspell"])
        .library(0, &["Island"; 8])
        .library(1, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let probe = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(probe));
    let mut life_paid = false;
    for _ in 0..20 {
        worlds_agree(&g, "Gitaxian Probe");
        match g.kind() {
            Some(DecisionKind::PayPhyrexian) => {
                g.choose(Choice::Number(1));
                life_paid = true;
            }
            Some(DecisionKind::Priority) => break,
            Some(DecisionKind::PayMana) => panic!("a payment of no mana asked: {:?}\n{}", g.options_desc(), log(&g)),
            _ => {
                let c = g.offered()[0];
                g.choose(c);
            }
        }
    }
    assert!(life_paid, "the pip was paid with life\n{}", log(&g));
    assert_eq!(g.obj(probe).zone, Zone::Stack, "Gitaxian Probe was cast\n{}", log(&g));
    assert_eq!(g.player(0).life, 18);
}

const FANATIC: &str = "Metamorphosis Fanatic";

/// P0 in its upkeep (no card drawn this turn) casts Brainstorm; its first draw is Metamorphosis Fanatic, revealed
/// for miracle. The game at the first card to put back.
fn brainstorm_draws_a_miracle() -> (Game, CardIdx) {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Brainstorm"])
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .library(0, &[FANATIC, "Lightning Bolt", "Counterspell", "Island", "Island", "Island"])
        .hand(1, &["Lightning Bolt", "Force of Will"])
        .library(1, &["Island"; 8])
        .turn_number(3)
        .at(Step::Upkeep, 0)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && g.to_move() == Some(0));
    g.choose(Choice::Cast(bs));
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = a_source(&g);
        g.choose(c);
    }
    // (Both players pass; Brainstorm draws the Fanatic first: reveal it.)
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    let fanatic = g.card(0, FANATIC, Zone::Hand);
    g.choose(Choice::Yes);
    assert!(g.knows(1, fanatic), "revealed for miracle\n{}", log(&g));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}", log(&g));
    (g, fanatic)
}

/// CR 702.94b: "If a player chooses to reveal a card using its miracle ability, they play with that card revealed
/// until that card leaves their hand, that ability resolves, or that ability otherwise leaves the stack." P0's
/// Brainstorm puts back two other cards (a private choice: P1 loses track of the rest of P0's hand). P1 still knows
/// the revealed Fanatic: every world of P1's keeps it, the miracle trigger asks P0 "cast it?" in every one, and P1's
/// view is the game's. Before ENGINE.md 210 the put-back took P1's knowledge of the revealed card too: P1's worlds
/// dealt it anew, and where it was not a miracle card the trigger asked nothing (the fuzz steward's 88c42634bd1f).
#[test]
fn cr_702_94b_a_revealed_miracle_card_stays_known_while_its_owner_puts_cards_back() {
    let (mut g, fanatic) = brainstorm_draws_a_miracle();
    let d = g.obj(fanatic).def;
    for name in ["Lightning Bolt", "Counterspell"] {
        worlds_agree(&g, "the put-back");
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Card(c));
    }
    assert!(g.knows(1, fanatic), "P1 still knows the revealed card\n{}", log(&g));
    for s in 0..40 {
        let mut w = g.clone();
        w.determinize(1, &mut Rng::new(s));
        assert_eq!(w.obj(fanatic).def, d, "P1's world {s} keeps the revealed card");
    }
    // The miracle trigger resolves: "cast it for its miracle cost?" - asked in every world of either player.
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
    assert!(g.offered().contains(&Choice::Yes), "{:?}", g.options_desc());
    worlds_agree(&g, "the miracle cast question");
}

/// The same with the revealed card put back first: it leaves the hand revealed (every player sees it go, CR
/// 702.94b), so P1 knows it is under the other card on top of P0's library, in every world.
#[test]
fn cr_702_94b_a_revealed_miracle_card_put_back_is_known_in_the_library() {
    let (mut g, fanatic) = brainstorm_draws_a_miracle();
    let d = g.obj(fanatic).def;
    worlds_agree(&g, "the put-back");
    g.choose(Choice::Card(fanatic));
    worlds_agree(&g, "the second card");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    assert_eq!(g.obj(fanatic).zone, Zone::Library);
    assert!(g.knows(1, fanatic), "P1 saw the revealed card go back\n{}", log(&g));
    for s in 0..40 {
        let mut w = g.clone();
        w.determinize(1, &mut Rng::new(s));
        assert_eq!(w.obj(fanatic).def, d, "P1's world {s} keeps it");
    }
    if g.pending_decision().is_some() {
        worlds_agree(&g, "after Brainstorm");
    }
}
