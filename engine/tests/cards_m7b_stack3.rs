//! M7b part 3b (ENGINE.md 27.9): cards that needed one small feature after
//! the 20 of part 3b, each against its Oracle text: The Wondrous Wasp (a
//! duration "for as long as this remains"), Voice of Victory (mobilize),
//! Brain Freeze (storm), Occult Epiphany, Fallen Shinobi, Otawara, Soaring
//! City and Boseiju, Who Endures (an activated ability's cost reduction),
//! Forensic Gadgeteer (artifacts' abilities cost less).

mod common;

use common::db;
use engine::db::{AbilitySlot, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c))
}

fn player_if(g: &Game, p: PlayerId) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(p)))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

/// Drive the game (passing priority, the first option of other decisions:
/// the order of simultaneous triggers) until `stop`.
fn run_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..1000 {
        g.drive_until(|g| stop(g) || !at_priority(g));
        if stop(g) {
            return;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    panic!("run_until: no progress");
}

fn attack_with(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
}

// ---- The Wondrous Wasp ----------------------------------------------------------------------

const WASP: &str = "The Wondrous Wasp";

/// P0 casts the Wasp; its trigger targets `target` (or nothing); stops with
/// the trigger on the stack and P0 to act.
fn wasp_trigger_on_stack(g: &mut Game, target: Option<CardIdx>) {
    let w = g.card(0, WASP, Zone::Hand);
    g.choose(Choice::Cast(w));
    finish(g, |_| None);
    for _ in 0..50 {
        if g.stack().len() == 1 && at_priority(g) && !matches!(g.stack()[0].kind, engine::game::StackKind::Spell { .. }) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else if g.kind() == Some(DecisionKind::Target) {
            g.choose(target.map_or(Choice::Pass, |t| g.tgt(t)));
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    panic!("no Wasp trigger");
}

#[test]
fn wondrous_wasp_taps_and_the_creature_loses_its_abilities_while_the_wasp_remains_cr_611_2b() {
    // "Flash. Flying. Wasp's Sting - When The Wondrous Wasp enters, tap up
    // to one target creature. It loses all abilities for as long as The
    // Wondrous Wasp remains on the battlefield." P1's Test Flyer loses
    // flying until P0 Bolts the Wasp.
    let mut g = scen().battlefield(0, &["Island", "Island", "Mountain"]).hand(0, &[WASP, "Lightning Bolt"]).battlefield(1, &["Test Flyer"]).start();
    let flyer = bf(&g, 1, "Test Flyer");
    wasp_trigger_on_stack(&mut g, Some(flyer));
    resolve_with(&mut g, |_| None);
    let w = bf(&g, 0, WASP);
    assert!(g.has_kw(w, Kw::Flash) && g.has_kw(w, Kw::Flying));
    assert!(g.obj(flyer).tapped);
    assert!(!g.has_kw(flyer, Kw::Flying), "no abilities");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, w));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(w).zone, Zone::Graveyard);
    assert!(g.has_kw(flyer, Kw::Flying), "the Wasp left: the duration ended");
}

#[test]
fn wondrous_wasp_gone_before_its_trigger_resolves_only_taps_cr_611_2b() {
    // "If the 'for as long as' duration never starts, the effect does
    // nothing": P0 Bolts its own Wasp in response; the Flyer is tapped and
    // keeps flying.
    let mut g = scen().battlefield(0, &["Island", "Island", "Mountain"]).hand(0, &[WASP, "Lightning Bolt"]).battlefield(1, &["Test Flyer"]).start();
    let flyer = bf(&g, 1, "Test Flyer");
    wasp_trigger_on_stack(&mut g, Some(flyer));
    let w = bf(&g, 0, WASP);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, w));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(w).zone, Zone::Graveyard);
    assert!(g.obj(flyer).tapped);
    assert!(g.has_kw(flyer, Kw::Flying));
}

#[test]
fn wondrous_wasp_may_target_nothing() {
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &[WASP]).battlefield(1, &["Test Flyer"]).start();
    let flyer = bf(&g, 1, "Test Flyer");
    wasp_trigger_on_stack(&mut g, None);
    resolve_with(&mut g, |_| None);
    assert!(!g.obj(flyer).tapped && g.has_kw(flyer, Kw::Flying));
}

// ---- Voice of Victory -----------------------------------------------------------------------

#[test]
fn voice_of_victory_mobilizes_two_warriors_sacrificed_at_the_end_step_cr_702_181a() {
    // "Mobilize 2 (Whenever this creature attacks, create two tapped and
    // attacking 1/1 red Warrior creature tokens. Sacrifice them at the
    // beginning of the next end step.) Your opponents can't cast spells
    // during your turn."
    let mut g = scen().battlefield(0, &["Voice of Victory"]).battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]).start();
    let v = bf(&g, 0, "Voice of Victory");
    attack_with(&mut g, &[v]);
    // P1 gets priority with the trigger on the stack: no Bolt.
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "opponents can't cast spells during your turn");
    resolve_with(&mut g, |_| None);
    let w = tokens_named(&g, "Warrior");
    assert_eq!(w.len(), 2);
    for &t in &w {
        assert!(g.obj(t).tapped && g.obj(t).attacking);
        assert_eq!(g.pt(t), (1, 1));
        assert_eq!(g.colors_of(t), 1 << 3, "red");
    }
    g.drive_until(|g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 17, "1 + 1 + 1");
    run_until(&mut g, |g| g.turn().active == 1);
    assert!(tokens_named(&g, "Warrior").is_empty(), "sacrificed at the end step");
    assert_eq!(g.count_in(0, "Voice of Victory", Zone::Battlefield), 1);
}

// ---- Brain Freeze ---------------------------------------------------------------------------

#[test]
fn brain_freeze_storm_counts_every_players_spells_cr_702_40a() {
    // "Target player mills three cards. Storm (When you cast this spell,
    // copy it for each spell cast before it this turn. You may choose new
    // targets for the copies.)" P0's Mishra's Bauble and P1's Lightning
    // Bolt were cast before: two copies, nine cards milled.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Mishra's Bauble", "Brain Freeze"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    finish(&mut g, |_| None);
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| player_if(g, 0));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(0), 17);
    let bfz = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(bfz));
    finish(&mut g, |g| player_if(g, 1));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(1).library.len(), 3, "3 x 3 milled");
    assert_eq!(g.player(1).graveyard.len(), 10, "nine cards and the Bolt");
}

#[test]
fn brain_freeze_copies_may_get_new_targets_and_are_not_cast_cr_707_10() {
    // ruling: 500a78f9
    // P0 casts a Bauble, then Brain Freeze (one copy, which P0 points at
    // itself), then a second Brain Freeze: the spells cast before it are the
    // Bauble and the first Brain Freeze, not its copy - two copies.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &["Mishra's Bauble", "Brain Freeze", "Brain Freeze"])
        .library(0, &["Island"; 20])
        .library(1, &["Island"; 20])
        .start();
    let lib = |g: &Game| (g.player(0).library.len(), g.player(1).library.len());
    let (l0, l1) = lib(&g);
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    let first = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(first));
    finish(&mut g, |g| player_if(g, 1));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::NewTarget)).then_some(Choice::Target(Target::Player(0))));
    assert_eq!(lib(&g), (l0 - 3, l1 - 3), "the copy milled P0");
    let second = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(second));
    finish(&mut g, |g| player_if(g, 1));
    resolve_with(&mut g, |_| None);
    assert_eq!(lib(&g), (l0 - 3, l1 - 12), "three times three more");
}

// ---- Occult Epiphany ------------------------------------------------------------------------

#[test]
fn occult_epiphany_makes_a_spirit_per_card_type_discarded_cr_205_2a() {
    // "Draw X cards, then discard X cards. Create a 1/1 white Spirit
    // creature token with flying for each card type among cards discarded
    // this way." X = 3: it draws Test Bear, Lightning Bolt, Forest and
    // discards them (creature, instant, land): three Spirits.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Occult Epiphany", "Island"])
        .library(0, &["Test Bear", "Lightning Bolt", "Forest", "Island"])
        .start();
    let oe = g.card(0, "Occult Epiphany", Zone::Hand);
    g.choose(Choice::Cast(oe));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(3));
    finish(&mut g, |_| None);
    let keep = g.card(0, "Island", Zone::Hand);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Discard)).then(|| {
            let o = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(keep));
            o.unwrap()
        })
    });
    assert_eq!(g.player(0).hand, vec![keep]);
    let s = tokens_named(&g, "Spirit");
    assert_eq!(s.len(), 3);
    assert!(g.has_kw(s[0], Kw::Flying) && g.pt(s[0]) == (1, 1) && g.colors_of(s[0]) == 1);
}

#[test]
fn occult_epiphany_counts_each_type_once() {
    // ruling: d12fc076
    // Two lands and an artifact creature discarded: land, artifact,
    // creature - three Spirits; two Islands alone: one.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Occult Epiphany"])
        .library(0, &["Island", "Forest", "Walking Ballista", "Island"])
        .start();
    let oe = g.card(0, "Occult Epiphany", Zone::Hand);
    g.choose(Choice::Cast(oe));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(3));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert!(g.player(0).hand.is_empty());
    assert_eq!(tokens_named(&g, "Spirit").len(), 3);
    let mut g = scen().battlefield(0, &["Island"; 3]).hand(0, &["Occult Epiphany"]).start();
    let oe = g.card(0, "Occult Epiphany", Zone::Hand);
    g.choose(Choice::Cast(oe));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(2));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(tokens_named(&g, "Spirit").len(), 1, "lands only");
}

// ---- Fallen Shinobi -------------------------------------------------------------------------

#[test]
fn fallen_shinobi_plays_the_damaged_players_top_cards_free_this_turn() {
    // "Whenever this creature deals combat damage to a player, that player
    // exiles the top two cards of their library. Until end of turn, you may
    // play those cards without paying their mana costs." P1's Test Giant is
    // cast for free; P1's Forest is played with P0's land drop.
    let mut g = scen()
        .battlefield(0, &["Fallen Shinobi"])
        .library(1, &["Test Giant", "Forest", "Island", "Island"])
        .start();
    let fs = bf(&g, 0, "Fallen Shinobi");
    attack_with(&mut g, &[fs]);
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.life(1), 15);
    let giant = g.card(1, "Test Giant", Zone::Exile);
    let forest = g.card(1, "Forest", Zone::Exile);
    assert!(g.has_option(Choice::Cast(giant)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(giant));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!((g.obj(giant).controller, g.obj(giant).owner), (0, 1));
    assert!(g.has_option(Choice::Play(forest)));
    g.choose(Choice::Play(forest));
    assert_eq!((g.obj(forest).zone, g.obj(forest).controller), (Zone::Battlefield, 0));
    assert_eq!(g.player(0).lands_played, 1);
}

#[test]
fn fallen_shinobi_permission_ends_with_the_turn() {
    let mut g = scen().battlefield(0, &["Fallen Shinobi"]).library(1, &["Test Giant", "Forest", "Island", "Island"]).start();
    let fs = bf(&g, 0, "Fallen Shinobi");
    attack_with(&mut g, &[fs]);
    g.drive_until(|g| g.turn().active == 1 && at_priority(g));
    g.drive_until(|g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    // ruling: 2826b2c6
    // Not cast: it remains in exile (`card` finds it there), unplayable.
    let giant = g.card(1, "Test Giant", Zone::Exile);
    assert!(!g.has_option(Choice::Cast(giant)));
    assert_eq!(g.count_in(1, "Forest", Zone::Exile), 1);
}

/// P0 makes an Eldrazi Spawn ("Sacrifice this token: Add {C}") with
/// Malevolent Rumble, attacks with it alone next turn and stops in the
/// declare blockers step with Fallen Shinobi in hand and `lands` untapped.
fn spawn_attacks_alone(lands: &[&str]) -> (Game, CardIdx) {
    let mut g = scen().battlefield(0, lands).hand(0, &["Malevolent Rumble", "Fallen Shinobi"]).start();
    let mr = g.card(0, "Malevolent Rumble", Zone::Hand);
    g.choose(Choice::Cast(mr));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::PutInHand)).then_some(Choice::Pass));
    let spawn = tokens_named(&g, "Eldrazi Spawn")[0];
    let t = g.turn().number;
    run_until(&mut g, |g| g.turn().active == 0 && g.turn().number > t && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(spawn));
    g.choose(Choice::Pass);
    run_until(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g) && g.pending_decision().unwrap().player == 0);
    (g, spawn)
}

#[test]
fn ninjutsu_cant_pay_its_mana_with_the_attacker_it_returns_cr_601_2h() {
    // ruling: 28dc7333 ffe10a85
    // Ninjutsu {2}{U}{B}: "Return an unblocked attacker you control to
    // hand: Put this card onto the battlefield from your hand tapped and
    // attacking." The mana is paid first (601.2g-h): the Spawn can't be
    // sacrificed for {C} and then returned. Three lands and the Spawn: not
    // offered (the fuzzer found the payment taking it); four lands: it is,
    // and the Spawn is never a mana source.
    let (g, _) = spawn_attacks_alone(&["Forest", "Swamp", "Island"]);
    let fs = g.card(0, "Fallen Shinobi", Zone::Hand);
    assert!(!g.has_option(Choice::Ability(fs, AbilitySlot::Hand(0))), "{:?}", g.options_desc());
    let (mut g, spawn) = spawn_attacks_alone(&["Forest", "Swamp", "Island", "Island"]);
    let fs = g.card(0, "Fallen Shinobi", Zone::Hand);
    g.choose(Choice::Ability(fs, AbilitySlot::Hand(0)));
    finish(&mut g, |g| {
        assert!(!g.has_option(Choice::Ability(spawn, AbilitySlot::Activated(0))), "the Spawn pays no mana");
        None
    });
    resolve_with(&mut g, |_| None);
    let fs = bf(&g, 0, "Fallen Shinobi");
    assert!(g.obj(fs).tapped && g.obj(fs).attacking);
    assert!(tokens_named(&g, "Eldrazi Spawn").is_empty(), "returned to hand: gone");
}

// ---- Otawara, Soaring City and Boseiju, Who Endures -------------------------------------------

const OTAWARA: &str = "Otawara, Soaring City";
const BOSEIJU: &str = "Boseiju, Who Endures";

#[test]
fn otawara_channel_costs_one_less_per_legendary_creature_cr_602_2b() {
    // "Channel - {3}{U}, Discard this card: Return target artifact,
    // creature, enchantment, or planeswalker to its owner's hand. This
    // ability costs {1} less to activate for each legendary creature you
    // control." Two Islands: only with two legendary creatures ({1}{U}).
    let g = scen().battlefield(0, &["Island", "Island", "Test Legend"]).hand(0, &[OTAWARA]).battlefield(1, &["Test Bear"]).start();
    let o = g.card(0, OTAWARA, Zone::Hand);
    assert!(!g.has_option(Choice::Ability(o, AbilitySlot::Hand(0))), "{{2}}{{U}} with one legend");
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Test Legend", "Thalia, Guardian of Thraben"])
        .hand(0, &[OTAWARA])
        .battlefield(1, &["Test Bear"])
        .start();
    let o = g.card(0, OTAWARA, Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Ability(o, AbilitySlot::Hand(0)));
    finish(&mut g, |g| tgt_if(g, bear));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.obj(o).zone, Zone::Graveyard, "discarded as a cost");
    assert!(g.lands_of(0).all(|c| g.obj(c).tapped));
}

#[test]
fn otawara_never_costs_less_than_its_coloured_mana() {
    // Five legendary creatures: {U} still.
    let g = scen()
        .battlefield(0, &["Test Legend", "Thalia, Guardian of Thraben", "Ragavan, Nimble Pilferer", "Griselbrand", "Questing Beast"])
        .hand(0, &[OTAWARA])
        .battlefield(1, &["Test Bear"])
        .start();
    let o = g.card(0, OTAWARA, Zone::Hand);
    assert!(!g.has_option(Choice::Ability(o, AbilitySlot::Hand(0))));
}

#[test]
fn boseiju_destroys_and_that_player_may_search_for_a_land_with_a_basic_type() {
    // "Channel - {1}{G}, Discard this card: Destroy target artifact,
    // enchantment, or nonbasic land an opponent controls. That player may
    // search their library for a land card with a basic land type, put it
    // onto the battlefield, then shuffle. This ability costs {1} less to
    // activate for each legendary creature you control." With a legend: {G}.
    let mut g = scen()
        .battlefield(0, &["Forest", "Test Legend"])
        .hand(0, &[BOSEIJU])
        .battlefield(1, &["Tropical Island"])
        .library(1, &["Island", "Savannah", "Island"])
        .start();
    let b = g.card(0, BOSEIJU, Zone::Hand);
    let ti = bf(&g, 1, "Tropical Island");
    let lib = g.player(1).library.len();
    g.choose(Choice::Ability(b, AbilitySlot::Hand(0)));
    finish(&mut g, |g| tgt_if(g, ti));
    let savannah = db().id("Savannah").unwrap();
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| {
            assert_eq!(g.pending_decision().unwrap().player, 1, "that player searches");
            Choice::Def(savannah)
        })
    });
    assert_eq!(g.obj(ti).zone, Zone::Graveyard);
    let s = bf(&g, 1, "Savannah");
    assert!(!g.obj(s).tapped);
    assert_eq!(g.player(1).library.len(), lib - 1);
}

#[test]
fn boseiju_that_player_may_find_nothing() {
    let mut g = scen().battlefield(0, &["Forest", "Forest"]).hand(0, &[BOSEIJU]).battlefield(1, &["Tropical Island"]).start();
    let b = g.card(0, BOSEIJU, Zone::Hand);
    let ti = bf(&g, 1, "Tropical Island");
    g.choose(Choice::Ability(b, AbilitySlot::Hand(0)));
    finish(&mut g, |g| tgt_if(g, ti));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then_some(Choice::Pass));
    assert_eq!(g.obj(ti).zone, Zone::Graveyard);
    assert!(g.lands_of(1).next().is_none());
}

// ---- Forensic Gadgeteer ---------------------------------------------------------------------

#[test]
fn forensic_gadgeteer_investigates_and_clues_cost_one_less() {
    // "Whenever you cast an artifact spell, investigate. Activated
    // abilities of artifacts you control cost {1} less to activate. This
    // effect can't reduce the mana in that cost to less than one mana."
    // Mishra's Bauble makes a Clue, whose {2} becomes {1}.
    let mut g = scen().battlefield(0, &["Forensic Gadgeteer", "Island"]).hand(0, &["Mishra's Bauble"]).start();
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    let clue = tokens_named(&g, "Clue");
    assert_eq!(clue.len(), 1);
    let a = Choice::Ability(clue[0], AbilitySlot::Activated(0));
    assert!(g.has_option(a), "{:?}", g.options_desc());
    let hand = g.player(0).hand.len();
    g.choose(a);
    finish(&mut g, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

#[test]
fn forensic_gadgeteer_leaves_at_least_one_mana() {
    // Mind Stone's "{1}, {T}, Sacrifice this artifact: Draw a card" stays
    // {1}: not with no other mana.
    let g = scen().battlefield(0, &["Forensic Gadgeteer", "Mind Stone"]).start();
    let ms = bf(&g, 0, "Mind Stone");
    assert!(!g.has_option(Choice::Ability(ms, AbilitySlot::Activated(1))), "{:?}", g.options_desc());
    let g = scen().battlefield(0, &["Forensic Gadgeteer", "Mind Stone", "Island"]).start();
    let ms = bf(&g, 0, "Mind Stone");
    assert!(g.has_option(Choice::Ability(ms, AbilitySlot::Activated(1))));
}
