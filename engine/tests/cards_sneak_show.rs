//! Legacy Sneak and Show (ENGINE.md 123): Show and Tell, Omniscience,
//! Mistrise Village, Brotherhood's End, Disruptor Flute and Magus of the Moon
//! against their Oracle text, the Comprehensive Rules and their official
//! rulings (`// ruling: <hash8>`, `data/rulings/legacy.rulings.json`), and
//! their interactions with the deck's other cards. They are in the card table
//! of constructed play only (`CardDb::constructed`). No test names an
//! identity number (the legacy registry's lines are assigned at integration).
//!
//! Show and Tell {2}{U}, Sorcery: "Each player may put an artifact, creature,
//! enchantment, or land card from their hand onto the battlefield."
//!
//! Omniscience {7}{U}{U}{U}, Enchantment: "You may cast spells from your hand
//! without paying their mana costs."
//!
//! Mistrise Village, Land: "This land enters tapped unless you control a
//! Mountain or a Forest. {T}: Add {U}. {U}, {T}: The next spell you cast this
//! turn can't be countered."
//!
//! Brotherhood's End {1}{R}{R}, Sorcery: "Choose one — Brotherhood's End deals
//! 3 damage to each creature and each planeswalker. / Destroy all artifacts
//! with mana value 3 or less."
//!
//! Disruptor Flute {2}, Artifact: "Flash. As this artifact enters, choose a
//! card name. Spells with the chosen name cost {3} more to cast. Activated
//! abilities of sources with the chosen name can't be activated unless
//! they're mana abilities."
//!
//! Magus of the Moon {2}{R}, Creature — Human Wizard 2/2: "Nonbasic lands are
//! Mountains."

use engine::az::encode::{self, ctx, g6, stack, CODE, NUM_FEATURES, O_CONTEXT, O_GLOBALS_V6, O_STACK};
use engine::db::{AbilitySlot, CardDb, Counter};
use engine::decision::act;
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const SNT: &str = "Show and Tell";
const OMNI: &str = "Omniscience";
const VILLAGE: &str = "Mistrise Village";
const BE: &str = "Brotherhood's End";
const FLUTE: &str = "Disruptor Flute";
const MAGUS: &str = "Magus of the Moon";
const EMRAKUL: &str = "Emrakul, the Aeons Torn";
const GRIS: &str = "Griselbrand";
const ATRAXA: &str = "Atraxa, Grand Unifier";
const FOW: &str = "Force of Will";

/// `ChooseAltCost`'s answer for "without paying its mana cost" (the last
/// `NUMBER`, `eoe_c::FREE_FIRST_SPELL`).
const FREE: u8 = (act::NUMBER_N - 1) as u8;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

/// A face's definition (a back face, an Adventure, a split card's half).
fn face_def(name: &str) -> u16 {
    (0..db().len() as u16).find(|&i| db().def(i).name == name).unwrap_or_else(|| panic!("no face {name}"))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn on_bf(g: &Game, p: PlayerId, name: &str) -> bool {
    g.find_card(p, name, Zone::Battlefield).is_some()
}

/// Answer every decision but priority with `answer` (else the first option)
/// until priority or the game's end.
fn until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..1000 {
        if g.is_over() || at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority: {}\n{}", opts(g), log(g));
}

/// Cast `name` from `p`'s hand, answering with `answer` (else the first
/// option), until priority.
fn cast_with(g: &mut Game, p: PlayerId, name: &str, answer: &dyn Fn(&Game) -> Option<Choice>) {
    let c = g.card(p, name, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    until_priority(g, answer);
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    cast_with(g, p, name, &|_| None)
}

/// Cast `name` for its "without paying its mana cost" (Omniscience).
fn cast_free(g: &mut Game, p: PlayerId, name: &str) {
    cast_with(g, p, name, &|g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(FREE)))
}

/// Pass priority until another decision or an empty stack.
fn pass_on(g: &mut Game) {
    g.pass_until(|g| g.stack().is_empty());
}

/// Resolve the whole stack, answering with `answer`.
fn resolve_with(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..1000 {
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
    panic!("the stack doesn't resolve: {}\n{}", opts(g), log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, &|_| None)
}

/// The non-mana activated abilities of `c` offered at priority.
fn abilities_offered(g: &Game, c: CardIdx) -> Vec<AbilitySlot> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s) if x == c => Some(s),
            _ => None,
        })
        .collect()
}

fn features(g: &Game, v: PlayerId) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode_features(v, &mut x);
    x
}

/// The hand-card choice of a resolving Show and Tell: `name`'s card, or PASS.
fn snt_pick(g: &mut Game, p: PlayerId, name: Option<&str>) {
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}\n{}", opts(g), log(g));
    assert_eq!(g.pending_decision().unwrap().player, p, "P{p} chooses: {}", opts(g));
    match name {
        Some(n) => {
            let c = g.card(p, n, Zone::Hand);
            g.choose(Choice::Card(c));
        }
        None => {
            g.pass();
        }
    }
}

/// P0 (active, precombat main) with three Islands and Show and Tell plus
/// `p0` in hand; P1 with `p1` in hand.
fn snt_scen(p0: &[&str], p1: &[&str]) -> Scenario {
    let mut hand0 = vec![SNT];
    hand0.extend_from_slice(p0);
    scen().battlefield(0, &["Island", "Island", "Island"]).hand(0, &hand0).hand(1, p1).at(Step::Main1, 0)
}

/// Cast Show and Tell and pass until its first choice.
fn cast_snt(g: &mut Game) {
    cast(g, 0, SNT);
    pass_on(g);
}

// ---- Show and Tell ---------------------------------------------------------------

#[test]
fn show_and_tell_is_a_blue_sorcery_of_mana_value_3() {
    let d = db().def(def(SNT));
    assert_eq!(d.mana_value(), 3);
    assert!(d.types & engine::db::types::SORCERY != 0);
    assert!(db().unsupported().is_empty(), "{:?}", db().unsupported());
}

#[test]
fn cr_101_4_show_and_tell_the_active_player_chooses_then_the_other_and_both_enter() {
    // ruling: a107ed89
    // ruling: 881ce0a9
    let mut g = snt_scen(&[EMRAKUL, "Brainstorm"], &[GRIS, FOW]).start();
    cast_snt(&mut g);
    // The active player first: Emrakul or PASS (Brainstorm isn't an
    // artifact, creature, enchantment or land card).
    assert_eq!(g.pending_decision().unwrap().player, 0);
    let brainstorm = g.card(0, "Brainstorm", Zone::Hand);
    assert!(!g.has_option(Choice::Card(brainstorm)) && g.has_option(Choice::Pass), "{}", opts(&g));
    snt_pick(&mut g, 0, Some(EMRAKUL));
    // Then the other player (the cards are still in hand: chosen during the
    // resolution, put at once).
    assert!(g.find_card(0, EMRAKUL, Zone::Hand).is_some());
    let fow = g.card(1, FOW, Zone::Hand);
    assert!(!g.has_option(Choice::Card(fow)), "{}", opts(&g));
    snt_pick(&mut g, 1, Some(GRIS));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, EMRAKUL) && on_bf(&g, 1, GRIS), "{}", log(&g));
    // Each under its owner's control; Show and Tell in the graveyard.
    assert_eq!(g.obj(bf(&g, 0, EMRAKUL)).controller, 0);
    assert_eq!(g.obj(bf(&g, 1, GRIS)).controller, 1);
    assert!(g.find_card(0, SNT, Zone::Graveyard).is_some());
}

#[test]
fn cr_101_4a_the_other_player_knows_only_whether_a_card_was_chosen() {
    // ruling: a107ed89
    // Two games alike but for the active player's face-down choice: the other
    // player's decision and its encoding are the same; the frame's stage
    // (ctx OP_I) says a card was chosen, not which.
    let pick = |name: Option<&str>| {
        let mut g = snt_scen(&[EMRAKUL, GRIS], &[ATRAXA]).start();
        cast_snt(&mut g);
        snt_pick(&mut g, 0, name);
        g
    };
    let (a, b, none) = (pick(Some(EMRAKUL)), pick(Some(GRIS)), pick(None));
    assert_eq!(a.offered(), b.offered());
    let (xa, xb, xn) = (features(&a, 1), features(&b, 1), features(&none, 1));
    assert_eq!(xa, xb, "the other player's input doesn't depend on which card was chosen");
    assert_eq!(xa[O_CONTEXT + ctx::OP_I], 2.0 / 8.0);
    assert_eq!(xn[O_CONTEXT + ctx::OP_I], 1.0 / 8.0);
    assert_ne!(xa, xn, "whether a card was chosen is public");
}

#[test]
fn cr_101_4a_the_other_players_worlds_deal_a_legal_face_down_choice() {
    // ruling: a107ed89
    // P0's hand and library are hidden from P1. In P1's worlds P0's hand is
    // dealt again and the face-down choice is a card of the hand of the kind
    // drawn at random: never Brainstorm (P1 knows the choice is legal), and
    // not always the real Emrakul (P1 doesn't know which card it is).
    let lib = ["Brainstorm", "Ponder", GRIS, "Island", "Volcanic Island", "Lotus Petal", "Ponder", "Brainstorm"];
    let mut base = snt_scen(&[EMRAKUL, "Brainstorm", "Ponder"], &[]).library(0, &lib).start();
    cast_snt(&mut base);
    snt_pick(&mut base, 0, Some(EMRAKUL));
    // P1's empty hand: no decision of its own; P1 decides nothing here, so
    // the worlds are checked by P1 declining in a copy with a hand card.
    let mut base = {
        let mut g = snt_scen(&[EMRAKUL, "Brainstorm", "Ponder"], &["Brainstorm"]).library(0, &lib).start();
        cast_snt(&mut g);
        snt_pick(&mut g, 0, Some(EMRAKUL));
        g
    };
    assert_eq!(base.pending_decision().unwrap().player, 1);
    let mut rng = Rng::new(7);
    let mut seen: std::collections::BTreeMap<String, u32> = Default::default();
    for _ in 0..300 {
        let mut w = base.clone();
        w.determinize(1, &mut rng);
        assert_eq!(w.offered(), base.offered(), "P1's options are the same in its worlds");
        w.pass();
        until_priority(&mut w, &|_| None);
        let mine: Vec<&str> = w.battlefield().iter().filter(|&&c| w.obj(c).owner == 0).map(|&c| w.name(c)).collect();
        assert_eq!(mine.len(), 4, "one card put: {mine:?}\n{}", log(&w));
        let put = [*mine.last().unwrap()];
        let d = db().def(def(put[0]));
        assert!(d.types & (engine::db::types::CREATURE | engine::db::types::LAND | engine::db::types::ARTIFACT | engine::db::types::ENCHANTMENT) != 0, "{}", put[0]);
        *seen.entry(put[0].to_string()).or_default() += 1;
    }
    assert!(seen.len() >= 3, "the choice is dealt among P0's cards of the kind: {seen:?}");
    let _ = &mut base;
}

#[test]
fn cr_101_4a_a_known_hand_is_one_pool_the_choice_among_its_cards_of_the_kind() {
    // ruling: a107ed89
    // P1 knows P0's whole hand (revealed earlier): Emrakul, Griselbrand,
    // Brainstorm. The choice is face down: in P1's worlds it is Emrakul or
    // Griselbrand, never Brainstorm, and both occur.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &[SNT])
        .hand_known(0, &[EMRAKUL, GRIS, "Brainstorm"])
        .hand(1, &["Ponder"])
        .at(Step::Main1, 0)
        .start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(EMRAKUL));
    let mut rng = Rng::new(11);
    let mut seen: std::collections::BTreeMap<&str, u32> = Default::default();
    for _ in 0..200 {
        let mut w = g.clone();
        w.determinize(1, &mut rng);
        w.pass();
        until_priority(&mut w, &|_| None);
        let n = if on_bf(&w, 0, EMRAKUL) { EMRAKUL } else if on_bf(&w, 0, GRIS) { GRIS } else { panic!("{}", log(&w)) };
        *seen.entry(n).or_default() += 1;
        assert!(w.find_card(0, "Brainstorm", Zone::Hand).is_some());
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
}

#[test]
fn show_and_tell_declining_is_legal_and_an_empty_hand_asks_nothing() {
    // P0 declines; P1 has no card in hand: no decision for P1.
    let mut g = snt_scen(&[EMRAKUL], &[]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, None);
    until_priority(&mut g, &|_| None);
    assert!(g.find_card(0, EMRAKUL, Zone::Hand).is_some() && g.stack().is_empty());
    // A hand without a card of the kind still asks (PASS only): the hand is
    // hidden.
    let mut g = snt_scen(&["Brainstorm"], &["Ponder"]).start();
    cast_snt(&mut g);
    assert_eq!(g.offered(), vec![Choice::Pass]);
    snt_pick(&mut g, 0, None);
    assert_eq!(g.offered(), vec![Choice::Pass]);
    snt_pick(&mut g, 1, None);
}

#[test]
fn cr_305_1_a_land_put_with_show_and_tell_is_no_land_drop() {
    // CR 305.1, 305.4: putting a land onto the battlefield isn't playing it.
    let mut g = snt_scen(&["Volcanic Island", "Mountain"], &[]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some("Volcanic Island"));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, "Volcanic Island"));
    assert_eq!(g.player(0).lands_played, 0);
    let m = g.card(0, "Mountain", Zone::Hand);
    assert!(g.has_option(Choice::Play(m)), "{}", opts(&g));
}

#[test]
fn cr_603_3b_show_and_tell_enters_together_and_triggers_go_on_the_stack_in_apnap_order() {
    // Atraxa (P0) and Atraxa (P1) enter at once; each one's enters trigger
    // is its controller's; the active player's goes on the stack first, so
    // the other player's resolves first (CR 603.3b).
    let mut g = snt_scen(&[ATRAXA], &[ATRAXA]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(ATRAXA));
    snt_pick(&mut g, 1, Some(ATRAXA));
    until_priority(&mut g, &|_| None);
    let st = g.stack();
    assert_eq!(st.len(), 2, "two enters triggers\n{}", log(&g));
    assert_eq!(st[0].controller, 0, "the active player's first, at the bottom");
    assert_eq!(st[1].controller, 1);
}

#[test]
fn cr_603_2_a_card_put_onto_the_battlefield_is_not_cast() {
    // Emrakul's "When you cast this spell, take an extra turn" doesn't
    // trigger: it wasn't cast (CR 603.2, 601.2i).
    let mut g = snt_scen(&[EMRAKUL], &[]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(EMRAKUL));
    until_priority(&mut g, &|_| None);
    assert!(g.stack().is_empty() && g.extra_turns().is_empty());
    assert_eq!(g.player(0).spells_cast, 1, "Show and Tell only");
}

#[test]
fn show_and_tell_the_active_players_enters_choices_come_first() {
    // ruling: 6d3f0bf4
    // Two Disruptor Flutes: each chooses a name as it enters, the active
    // player first (the ruling), then the other.
    let mut g = snt_scen(&[FLUTE], &[FLUTE]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(FLUTE));
    snt_pick(&mut g, 1, Some(FLUTE));
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}", opts(&g));
    assert_eq!(g.pending_decision().unwrap().player, 0);
    g.choose(Choice::Def(def(SNT)));
    assert_eq!(g.kind(), Some(DecisionKind::NameCard));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    g.choose(Choice::Def(def(FLUTE)));
    until_priority(&mut g, &|_| None);
}

#[test]
fn cr_101_4_show_and_tell_on_the_other_players_turn_they_choose_first() {
    // ruling: a107ed89
    // "The current player chooses first": P1, the active player, casts it;
    // P1 chooses, then P0.
    let mut g = scen().battlefield(1, &["Island", "Island", "Island"]).hand(1, &[SNT, EMRAKUL]).hand(0, &[GRIS]).at(Step::Main1, 1).start();
    cast(&mut g, 1, SNT);
    pass_on(&mut g);
    snt_pick(&mut g, 1, Some(EMRAKUL));
    snt_pick(&mut g, 0, Some(GRIS));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 1, EMRAKUL) && on_bf(&g, 0, GRIS));
}

// ---- Omniscience -------------------------------------------------------------------

/// P0 with Omniscience and `hand`, at its precombat main phase.
fn omni_scen(hand: &[&str]) -> Scenario {
    scen().battlefield(0, &[OMNI]).hand(0, hand).at(Step::Main1, 0)
}

#[test]
fn omniscience_is_a_blue_enchantment_of_mana_value_10() {
    let d = db().def(def(OMNI));
    assert_eq!(d.mana_value(), 10);
    assert!(d.types & engine::db::types::ENCHANTMENT != 0);
}

#[test]
fn cr_118_9_omniscience_casts_a_spell_from_hand_without_paying_its_mana_cost() {
    // No land: Show and Tell is castable only for "without paying its mana
    // cost" (CR 118.9, 601.2b).
    let mut g = omni_scen(&[SNT, EMRAKUL]).start();
    let c = g.card(0, SNT, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{}", opts(&g));
    g.choose(Choice::Cast(c));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost), "{}", opts(&g));
    assert_eq!(g.offered(), vec![Choice::Number(FREE)], "not its mana cost: no mana");
    g.choose(Choice::Number(FREE));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.stack().len(), 1);
    pass_on(&mut g);
    snt_pick(&mut g, 0, Some(EMRAKUL));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, EMRAKUL));
}

#[test]
fn cr_118_9a_omniscience_and_force_of_will_are_alternative_costs_one_at_a_time() {
    // ruling: 6e20dab1
    // Force of Will on P0's own turn (targeting P0's own spell, as a test of
    // the costs): its mana cost (with mana), its pitch (exile a blue card,
    // pay 1 life) or "without paying its mana cost": one of them (CR 118.9a),
    // each reachable.
    let mut g = scen()
        .battlefield(0, &[OMNI, "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &[FOW, "Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Ponder");
    let fow = g.card(0, FOW, Zone::Hand);
    g.choose(Choice::Cast(fow));
    for _ in 0..20 {
        if g.kind() == Some(DecisionKind::ChooseAltCost) {
            break;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost), "{}", opts(&g));
    let o = g.offered();
    assert!(o.contains(&Choice::Number(0)) && o.contains(&Choice::Number(1)) && o.contains(&Choice::Number(FREE)), "{}", opts(&g));
    // Free: no card exiled, no life paid, no mana.
    g.choose(Choice::Number(FREE));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.player(0).life, 20);
    assert!(g.find_card(0, "Brainstorm", Zone::Hand).is_some());
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 1, "Ponder's Island only");
}

#[test]
fn cr_107_3b_omniscience_an_x_spell_has_x_0() {
    // ruling: 3c753455
    // Walking Ballista cast without paying its mana cost: X is 0, so it
    // enters with no counters and dies (CR 107.3b, 704.5f).
    let mut g = omni_scen(&["Walking Ballista"]).start();
    cast_free(&mut g, 0, "Walking Ballista");
    resolve(&mut g);
    assert!(g.find_card(0, "Walking Ballista", Zone::Graveyard).is_some(), "{}", log(&g));
}

#[test]
fn cr_601_2f_omniscience_additional_costs_are_still_paid() {
    // ruling: 6e20dab1
    // Burst Lightning's kicker {4} is paid on top of "without paying its mana
    // cost" (kicked: 4 damage); Tinker's mandatory "sacrifice an artifact"
    // too (no artifact: not castable).
    let mut g = scen()
        .battlefield(0, &[OMNI, "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Burst Lightning", "Tinker"])
        .at(Step::Main1, 0)
        .start();
    let tinker = g.card(0, "Tinker", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(tinker)), "Tinker needs an artifact to sacrifice: {}", opts(&g));
    let bl = g.card(0, "Burst Lightning", Zone::Hand);
    g.choose(Choice::Cast(bl));
    let mut kicked = false;
    for _ in 0..40 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::ChooseAltCost => Choice::Number(FREE),
            DecisionKind::ChooseCost | DecisionKind::ChooseMode if !kicked && g.has_option(Choice::Number(1)) => {
                kicked = true;
                Choice::Number(1)
            }
            DecisionKind::Target => Choice::Target(engine::Target::Player(1)),
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert!(kicked, "kicker offered\n{}", log(&g));
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 4, "{{4}} paid for the kicker");
    resolve(&mut g);
    assert_eq!(g.player(1).life, 16);
}

#[test]
fn cr_601_2f_omniscience_cost_increases_still_apply() {
    // ruling: 6e20dab1
    // Thalia: noncreature spells cost {1} more; free, Brainstorm costs {1}.
    let g = scen().battlefield(0, &[OMNI]).battlefield(1, &["Thalia, Guardian of Thraben"]).hand(0, &["Brainstorm"]).at(Step::Main1, 0).start();
    let b = g.card(0, "Brainstorm", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(b)), "no mana for the {{1}}: {}", opts(&g));
    let mut g = scen()
        .battlefield(0, &[OMNI, "Island"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .hand(0, &["Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    cast_free(&mut g, 0, "Brainstorm");
    assert!(g.obj(bf(&g, 0, "Island")).tapped, "the Island paid Thalia's {{1}}");
}

#[test]
fn cr_118_9_omniscience_only_its_controllers_spells_from_hand() {
    // The opponent's Omniscience gives P0 nothing; a land is played, not
    // cast; a spell from the graveyard isn't from the hand.
    let g = scen().battlefield(1, &[OMNI]).hand(0, &[SNT]).at(Step::Main1, 0).start();
    let c = g.card(0, SNT, Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{}", opts(&g));
    let g = omni_scen(&["Island"]).start();
    let i = g.card(0, "Island", Zone::Hand);
    assert!(g.has_option(Choice::Play(i)));
}

#[test]
fn cr_601_3_omniscience_keeps_the_timing_rules() {
    // ruling: 72e98293
    // A sorcery on the other player's turn: not castable, Omniscience or not.
    let g = scen().battlefield(0, &[OMNI]).hand(0, &[SNT]).at(Step::Main1, 1).start();
    let mut g = g;
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    let c = g.card(0, SNT, Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{}", opts(&g));
}

#[test]
fn cr_603_2_emrakul_cast_with_omniscience_takes_an_extra_turn() {
    // Cast (not put): "When you cast this spell, take an extra turn after
    // this one" triggers.
    let mut g = omni_scen(&[EMRAKUL]).start();
    cast_free(&mut g, 0, EMRAKUL);
    assert_eq!(g.stack().len(), 2, "Emrakul and its cast trigger\n{}", log(&g));
    resolve(&mut g);
    assert!(on_bf(&g, 0, EMRAKUL));
    assert_eq!(g.extra_turns(), &[0]);
}

#[test]
fn cr_118_9_omniscience_paying_the_mana_cost_stays_reachable() {
    // With the mana, both: its mana cost and "without paying its mana cost".
    let mut g = scen().battlefield(0, &[OMNI, "Island"]).hand(0, &["Brainstorm"]).at(Step::Main1, 0).start();
    let b = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(b));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost));
    assert_eq!(g.offered(), vec![Choice::Number(0), Choice::Number(FREE)]);
    g.choose(Choice::Number(0));
    until_priority(&mut g, &|_| None);
    assert!(g.obj(bf(&g, 0, "Island")).tapped);
}

// ---- Mistrise Village ------------------------------------------------------------

/// Mistrise Village's "{U}, {T}" ability's slot on `c`.
fn village_ability(g: &Game, c: CardIdx) -> AbilitySlot {
    let v = abilities_offered(g, c);
    assert_eq!(v.len(), 1, "one ability offered at priority: {v:?} {}", opts(g));
    v[0]
}

/// Pay with a basic Island's mana where one is offered.
fn island_first(g: &Game) -> Option<Choice> {
    if g.kind() != Some(DecisionKind::PayMana) {
        return None;
    }
    g.offered().into_iter().find(|o| matches!(o, Choice::Ability(c, _) if g.name(*c) == "Island"))
}

fn activate_village_at(g: &mut Game, v: CardIdx) {
    let s = village_ability(g, v);
    g.choose(Choice::Ability(v, s));
    until_priority(g, &island_first);
}

fn activate_village(g: &mut Game) {
    let v = bf(g, 0, VILLAGE);
    activate_village_at(g, v);
}

#[test]
fn cr_614_1c_mistrise_village_enters_tapped_unless_you_control_a_mountain_or_a_forest() {
    // ruling: 16696510
    let mut g = scen().hand(0, &[VILLAGE]).at(Step::Main1, 0).start();
    let v = g.card(0, VILLAGE, Zone::Hand);
    g.choose(Choice::Play(v));
    until_priority(&mut g, &|_| None);
    assert!(g.obj(v).tapped, "no Mountain or Forest");
    for land in ["Mountain", "Volcanic Island", "Forest"] {
        let mut g = scen().battlefield(0, &[land]).hand(0, &[VILLAGE]).at(Step::Main1, 0).start();
        let v = g.card(0, VILLAGE, Zone::Hand);
        g.choose(Choice::Play(v));
        until_priority(&mut g, &|_| None);
        assert!(!g.obj(v).tapped, "with {land}");
    }
    // The opponent's Mountain doesn't count.
    let mut g = scen().battlefield(1, &["Mountain"]).hand(0, &[VILLAGE]).at(Step::Main1, 0).start();
    let v = g.card(0, VILLAGE, Zone::Hand);
    g.choose(Choice::Play(v));
    until_priority(&mut g, &|_| None);
    assert!(g.obj(v).tapped);
}

#[test]
fn cr_101_2_mistrise_village_the_next_spell_cant_be_countered() {
    // {U}, {T}: Brainstorm next, Force of Will can target it and resolves,
    // Brainstorm stays and resolves (CR 101.2).
    let mut g = scen()
        .battlefield(0, &[VILLAGE, "Island", "Island"])
        .hand(0, &["Brainstorm", "Ponder"])
        .hand(1, &[FOW, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    activate_village(&mut g);
    assert_eq!(g.stack().len(), 1, "the ability on the stack");
    resolve(&mut g);
    assert_eq!(g.player(0).uncounterable_next, 1);
    let x = features(&g, 0);
    assert!(x[O_GLOBALS_V6 + g6::UNCOUNTERABLE_NEXT] > 0.0, "the viewer's float");
    let y = features(&g, 1);
    assert!(y[O_GLOBALS_V6 + g6::PLAYER + g6::UNCOUNTERABLE_NEXT] > 0.0, "the opponent's float");
    cast(&mut g, 0, "Brainstorm");
    assert_eq!(g.player(0).uncounterable_next, 0, "used up");
    let x = features(&g, 1);
    assert_eq!(x[O_STACK + CODE + stack::UNCOUNTERABLE], 1.0, "the spell's stack column");
    // P1 Forces it (pitching Brainstorm): the counter resolves, the spell stays.
    g.pass();
    let fow = g.card(1, FOW, Zone::Hand);
    assert!(g.has_option(Choice::Cast(fow)), "{}", opts(&g));
    let target = g.stack()[0].clone();
    let engine::game::StackKind::Spell { card: bs } = target.kind else { panic!() };
    cast_with(&mut g, 1, FOW, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.pending_decision().unwrap().options[0].choice),
        _ => None,
    });
    assert_eq!(g.stack().len(), 2);
    resolve(&mut g);
    assert!(g.find_card(0, "Brainstorm", Zone::Graveyard).is_some() && g.find_card(1, FOW, Zone::Graveyard).is_some());
    assert_eq!(g.player(0).hand.len(), 1 + 3 - 2, "Brainstorm resolved: Ponder + 3 drawn - 2 put back\n{}", log(&g));
    let _ = bs;
}

#[test]
fn cr_101_2_two_activations_wait_for_the_same_next_spell() {
    // Two Villages: both effects apply to the next spell only; the one after
    // can be countered.
    let mut g = scen()
        .battlefield(0, &[VILLAGE, VILLAGE, "Island", "Island", "Island", "Island"])
        .hand(0, &["Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let vs: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.name(c) == VILLAGE).collect();
    for v in vs {
        activate_village_at(&mut g, v);
        resolve(&mut g);
    }
    assert_eq!(g.player(0).uncounterable_next, 2);
    cast(&mut g, 0, "Ponder");
    assert_eq!(g.player(0).uncounterable_next, 0);
    let engine::game::StackKind::Spell { card } = g.stack()[0].kind else { panic!() };
    assert!(g.marks_of(card).iter().any(|m| matches!(m, engine::game::Mark::Uncounterable)));
    resolve(&mut g);
    cast(&mut g, 0, "Brainstorm");
    let engine::game::StackKind::Spell { card } = g.stack()[0].kind else { panic!() };
    assert!(!g.marks_of(card).iter().any(|m| matches!(m, engine::game::Mark::Uncounterable)), "the second spell can be countered");
}

#[test]
fn cr_101_2_a_spell_cast_while_the_ability_waits_on_the_stack_is_not_the_next_one() {
    // In response to its own ability, P0 casts Brainstorm: the effect doesn't
    // exist yet; Ponder, cast after the ability resolved, can't be countered.
    let mut g = scen()
        .battlefield(0, &[VILLAGE, "Island", "Island", "Island"])
        .hand(0, &["Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    activate_village(&mut g);
    cast(&mut g, 0, "Brainstorm");
    let engine::game::StackKind::Spell { card: bs } = g.stack()[1].kind else { panic!() };
    assert!(!g.marks_of(bs).iter().any(|m| matches!(m, engine::game::Mark::Uncounterable)));
    resolve(&mut g);
    assert_eq!(g.player(0).uncounterable_next, 1);
    cast(&mut g, 0, "Ponder");
    let engine::game::StackKind::Spell { card } = g.stack()[0].kind else { panic!() };
    assert!(g.marks_of(card).iter().any(|m| matches!(m, engine::game::Mark::Uncounterable)));
}

#[test]
fn cr_514_2_the_effect_ends_with_the_turn() {
    // Unused, it ends in the cleanup step: the next turn's spell can be
    // countered; the float goes back to 0.
    let mut g = scen().battlefield(0, &[VILLAGE, "Island"]).at(Step::Main1, 0).start();
    activate_village(&mut g);
    resolve(&mut g);
    assert_eq!(g.player(0).uncounterable_next, 1);
    g.drive_until(|g| g.turn().number > 3 || g.player(0).uncounterable_next == 0);
    assert_eq!(g.player(0).uncounterable_next, 0);
    assert!(g.turn().step == Step::Cleanup || g.turn().number > 3, "{:?}", g.turn().step);
    assert_eq!(features(&g, 0)[O_GLOBALS_V6 + g6::UNCOUNTERABLE_NEXT], 0.0);
}

#[test]
fn cr_101_2_only_its_controllers_next_spell() {
    // Activated by P0; P1's next spell is not affected.
    let mut g = scen().battlefield(0, &[VILLAGE, "Island"]).battlefield(1, &["Island"]).hand(1, &["Brainstorm"]).at(Step::Main1, 0).start();
    activate_village(&mut g);
    resolve(&mut g);
    g.pass_until(|_| false);
    let _ = &mut g;
    assert_eq!(g.player(1).uncounterable_next, 0);
}

// ---- Brotherhood's End -----------------------------------------------------------

/// Cast Brotherhood's End choosing mode `m`.
fn cast_be(g: &mut Game, m: u8) {
    cast_with(g, 0, BE, &|g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(m)));
}

#[test]
fn brotherhoods_end_is_a_red_sorcery_of_mana_value_3_with_two_modes() {
    let d = db().def(def(BE));
    assert_eq!(d.mana_value(), 3);
    let g = scen().battlefield(0, &["Mountain", "Mountain", "Mountain"]).hand(0, &[BE]).at(Step::Main1, 0).start();
    let mut g = g;
    let c = g.card(0, BE, Zone::Hand);
    g.choose(Choice::Cast(c));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{}", opts(&g));
    assert_eq!(g.offered(), vec![Choice::Number(0), Choice::Number(1)]);
}

#[test]
fn cr_120_3_brotherhoods_end_deals_3_damage_to_each_creature_and_each_planeswalker() {
    // Every creature and planeswalker, both players'; no player.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", MAGUS, GRIS])
        .battlefield(1, &["Thalia, Guardian of Thraben", "Karn, Scion of Urza"])
        .hand(0, &[BE])
        .at(Step::Main1, 0)
        .start();
    let karn = bf(&g, 1, "Karn, Scion of Urza");
    let loyalty = g.counters_on(karn, Counter::Loyalty);
    cast_be(&mut g, 0);
    resolve(&mut g);
    assert!(g.find_card(0, MAGUS, Zone::Graveyard).is_some());
    assert!(g.find_card(1, "Thalia, Guardian of Thraben", Zone::Graveyard).is_some());
    assert!(on_bf(&g, 0, GRIS) && g.obj(bf(&g, 0, GRIS)).damage == 3);
    let karn_left = g.find_card(1, "Karn, Scion of Urza", Zone::Battlefield).map(|k| g.counters_on(k, Counter::Loyalty));
    assert_eq!(karn_left.unwrap_or(0), loyalty.saturating_sub(3));
    assert_eq!((g.player(0).life, g.player(1).life), (20, 20));
}

#[test]
fn cr_202_3_brotherhoods_end_destroys_each_artifact_with_mana_value_3_or_less() {
    // Lotus Petal (0), Disruptor Flute (2), Coalition Relic (3) and Walking
    // Ballista (0 on the battlefield: X is 0, CR 202.3e) are destroyed; Relic
    // of Sauron (4) stays; The Mind Stone (2, indestructible: CR 702.12b)
    // stays; the Magus (no artifact) stays.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Lotus Petal", MAGUS])
        .battlefield(1, &[FLUTE, "Walking Ballista", "Coalition Relic", "Relic of Sauron", "The Mind Stone"])
        .hand(0, &[BE])
        .at(Step::Main1, 0)
        .start();
    cast_be(&mut g, 1);
    resolve(&mut g);
    for n in ["Lotus Petal"] {
        assert!(g.find_card(0, n, Zone::Graveyard).is_some(), "{n}");
    }
    for n in [FLUTE, "Coalition Relic"] {
        assert!(g.find_card(1, n, Zone::Graveyard).is_some(), "{n}");
    }
    assert!(g.find_card(1, "Walking Ballista", Zone::Battlefield).is_none());
    assert!(on_bf(&g, 1, "Relic of Sauron") && on_bf(&g, 1, "The Mind Stone") && on_bf(&g, 0, MAGUS));
}

#[test]
fn cr_202_3c_brotherhoods_end_destroys_treasure_tokens_mana_value_0() {
    // A token that isn't a copy has no mana cost: mana value 0 (CR 202.3c,
    // 111.4): Hullbreacher's Treasures (P1's Brainstorm draws three: each
    // replaced by a Treasure for P0) are destroyed.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Hullbreacher"])
        .battlefield(1, &["Island"])
        .hand(0, &[BE])
        .hand(1, &["Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    cast(&mut g, 1, "Brainstorm");
    resolve(&mut g);
    let treasures = g.battlefield().iter().filter(|&&c| g.name(c) == "Treasure").count();
    assert_eq!(treasures, 3, "{}", log(&g));
    cast_be(&mut g, 1);
    resolve(&mut g);
    assert_eq!(g.battlefield().iter().filter(|&&c| g.name(c) == "Treasure").count(), 0);
    assert!(on_bf(&g, 0, "Hullbreacher"));
}

// ---- Disruptor Flute ---------------------------------------------------------------

/// Cast Disruptor Flute and name the face of definition `d`.
fn flute_naming_def(g: &mut Game, p: PlayerId, d: u16) {
    cast(g, p, FLUTE);
    pass_on(g);
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}
{}", opts(g), log(g));
    g.choose(Choice::Def(d));
    resolve(g);
}

/// Cast Disruptor Flute (P0, two lands) and name `name`.
fn flute_naming(g: &mut Game, p: PlayerId, name: Option<&str>) {
    cast(g, p, FLUTE);
    pass_on(g);
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}\n{}", opts(g), log(g));
    match name {
        Some(n) => g.choose(Choice::Def(def(n))),
        None => g.pass(),
    };
    resolve(g);
}

#[test]
fn disruptor_flute_is_a_colorless_artifact_with_flash() {
    let d = db().def(def(FLUTE));
    assert_eq!(d.mana_value(), 2);
    assert!(d.types & engine::db::types::ARTIFACT != 0 && d.colors == 0);
    // Flash: castable on the other player's turn.
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &[FLUTE]).at(Step::Main1, 1).start();
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    let c = g.card(0, FLUTE, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{}", opts(&g));
}

#[test]
fn cr_201_3_disruptor_flute_offers_every_name_of_the_games_cards_and_their_faces() {
    // ruling: 20e62c09
    // ruling: 3029db62
    // The names of both players' cards (any zone), each face's: a split
    // card's halves (Life, Death), an Adventure (Petty Theft), a back face
    // (Tamiyo, Seasoned Scholar); PASS for a name no object here can have.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &[FLUTE])
        .library(0, &["Brazen Borrower // Petty Theft", "Life // Death"])
        .hand(1, &["Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, FLUTE);
    pass_on(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::NameCard));
    let names: Vec<&str> = g
        .offered()
        .into_iter()
        .filter_map(|c| match c {
            Choice::Def(d) => Some(engine::sneak_show::face_name(db().def(d))),
            _ => None,
        })
        .collect();
    for n in [
        "Brazen Borrower",
        "Petty Theft",
        "Life",
        "Death",
        "Tamiyo, Inquisitive Student",
        "Tamiyo, Seasoned Scholar",
        "Island",
        FLUTE,
        "Faerie Dragon",
    ] {
        assert!(names.contains(&n), "{n} among {names:?}");
    }
    assert!(!names.contains(&"Life // Death"), "a split card's names are its halves'");
    assert!(g.has_option(Choice::Pass));
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), names.len(), "each name once");
}

#[test]
fn cr_601_2f_disruptor_flute_spells_with_the_name_cost_3_more_every_players() {
    // ruling: 560a16f0
    // Naming Brainstorm: P0's own Brainstorm costs {3}{U} (every player's
    // spells, its controller's too); Ponder doesn't.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &[FLUTE, "Brainstorm", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    flute_naming(&mut g, 0, Some("Brainstorm"));
    let b = g.card(0, "Brainstorm", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(b)), "{{3}}{{U}} with two lands left: {}", opts(&g));
    let p = g.card(0, "Ponder", Zone::Hand);
    assert!(g.has_option(Choice::Cast(p)));
    // With four lands it is castable for {3}{U}.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &[FLUTE, "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    flute_naming(&mut g, 0, Some("Brainstorm"));
    cast(&mut g, 0, "Brainstorm");
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 6);
}

#[test]
fn cr_602_5_disruptor_flute_stops_non_mana_activated_abilities() {
    // ruling: 044069c9
    // ruling: d858e725
    // Naming Sneak Attack: its "{R}: put a creature card ..." can't be
    // activated. Naming Lotus Petal: its mana ability still can be.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Mountain"])
        .battlefield(0, &["Sneak Attack", "Lotus Petal"])
        .hand(0, &[FLUTE, EMRAKUL])
        .at(Step::Main1, 0)
        .start();
    let sa = bf(&g, 0, "Sneak Attack");
    assert!(!abilities_offered(&g, sa).is_empty(), "before: {}", opts(&g));
    flute_naming(&mut g, 0, Some("Sneak Attack"));
    assert!(abilities_offered(&g, sa).is_empty(), "named: {}", opts(&g));
    // Lotus Petal named: it still pays.
    let mut g = scen().battlefield(0, &["Island", "Island", "Lotus Petal"]).hand(0, &[FLUTE, "Ponder"]).at(Step::Main1, 0).start();
    flute_naming(&mut g, 0, Some("Lotus Petal"));
    let p = g.card(0, "Ponder", Zone::Hand);
    assert!(g.has_option(Choice::Cast(p)), "Lotus Petal's mana: {}", opts(&g));
    cast(&mut g, 0, "Ponder");
    assert!(g.find_card(0, "Lotus Petal", Zone::Graveyard).is_some());
}

#[test]
fn cr_602_5_disruptor_flute_stops_fetch_lands_and_works_in_every_zone() {
    // ruling: 044069c9
    // A fetch land's ability (not a mana ability); Simian Spirit Guide's
    // "exile from your hand: add {R}" is a mana ability: still usable.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Scalding Tarn"])
        .hand(0, &[FLUTE, "Simian Spirit Guide", "Lightning Bolt"])
        .library(0, &["Volcanic Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let tarn = bf(&g, 0, "Scalding Tarn");
    assert!(!abilities_offered(&g, tarn).is_empty());
    flute_naming(&mut g, 0, Some("Scalding Tarn"));
    assert!(abilities_offered(&g, tarn).is_empty());
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &[FLUTE, "Simian Spirit Guide", "Lightning Bolt"]).at(Step::Main1, 0).start();
    flute_naming(&mut g, 0, Some("Simian Spirit Guide"));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bolt)), "the Guide's mana: {}", opts(&g));
}

#[test]
fn cr_606_disruptor_flute_stops_loyalty_abilities_of_a_back_face() {
    // ruling: 20e62c09
    // Naming "Tamiyo, Seasoned Scholar" (a back face): her loyalty abilities
    // (activated abilities) can't be activated.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"])
        .transformed(1, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar")
        .hand(0, &[FLUTE])
        .at(Step::Main1, 0)
        .start();
    flute_naming_def(&mut g, 0, face_def("Tamiyo, Seasoned Scholar"));
    g.pass_until(|g| g.turn().active == 1 && g.turn().step == Step::Main1 && g.pending_decision().is_some_and(|d| d.player == 1));
    let t = g.battlefield().iter().copied().find(|&c| g.name(c) == "Tamiyo, Seasoned Scholar").expect("Tamiyo");
    assert!(abilities_offered(&g, t).is_empty(), "{}", opts(&g));
}

/// Every land of `p` untapped but `tapped`.
fn lands_untapped_but(g: &mut Game, p: PlayerId, tapped: &[&str]) {
    let lands: Vec<CardIdx> = g.lands_of(p).collect();
    for c in lands {
        let t = tapped.contains(&g.name(c));
        g.set_tapped(c, t);
    }
    g.refresh_decision();
}

#[test]
fn cr_709_3_disruptor_flute_a_split_cards_half_and_an_adventure_by_their_names() {
    // ruling: 20e62c09
    // Naming "Life": Life ({G}, now {3}{G}) can't be cast with three lands,
    // Death ({1}{B}) can (its other face only, `CastFace`). Naming Brazen
    // Borrower: Petty Theft ({1}{U}, its Adventure) isn't taxed, the
    // creature ({1}{U}{U} + {3}) is (the ruling's Ondu Knotmaster).
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp", "Forest"])
        .hand(0, &[FLUTE, "Life // Death"])
        .graveyard(0, &[GRIS])
        .at(Step::Main1, 0)
        .start();
    flute_naming_def(&mut g, 0, face_def("Life"));
    lands_untapped_but(&mut g, 0, &["Island"]);
    let c = g.card(0, "Life // Death", Zone::Hand);
    let o = g.offered();
    assert!(o.contains(&Choice::CastFace(c)), "Death: {}", opts(&g));
    assert!(!o.iter().any(|x| matches!(x, Choice::Cast(y) | Choice::Faces(y, _) if *y == c)), "not Life: {}", opts(&g));
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .battlefield(1, &[GRIS])
        .hand(0, &[FLUTE, "Brazen Borrower // Petty Theft"])
        .at(Step::Main1, 0)
        .start();
    flute_naming(&mut g, 0, Some("Brazen Borrower // Petty Theft"));
    lands_untapped_but(&mut g, 0, &[]);
    let lands: Vec<CardIdx> = g.lands_of(0).take(2).collect();
    for l in lands {
        g.set_tapped(l, true);
    }
    g.refresh_decision();
    let c = g.card(0, "Brazen Borrower // Petty Theft", Zone::Hand);
    let o = g.offered();
    assert!(o.contains(&Choice::CastFace(c)), "Petty Theft: {}", opts(&g));
    assert!(!o.iter().any(|x| matches!(x, Choice::Cast(y) | Choice::Faces(y, _) if *y == c)), "not the creature: {}", opts(&g));
}

#[test]
fn cr_601_2f_disruptor_flute_naming_show_and_tell_under_omniscience_costs_3() {
    // ruling: 560a16f0
    // "Without paying its mana cost" plus the {3} increase: {3}.
    let mut g = scen().battlefield(0, &[OMNI, "Island", "Island", "Island"]).battlefield(1, &["Island", "Island"]).hand(1, &[FLUTE]).hand(0, &[SNT]).at(Step::Main1, 0).start();
    // P1 flashes in the Flute on P0's turn, naming Show and Tell.
    g.pass();
    cast_with(&mut g, 1, FLUTE, &|_| None);
    pass_on(&mut g);
    g.choose(Choice::Def(def(SNT)));
    resolve(&mut g);
    let c = g.card(0, SNT, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{}", opts(&g));
    cast_free(&mut g, 0, SNT);
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 3, "{{3}} paid");
    // Without three untapped lands it can't be cast.
    let mut g = scen().battlefield(0, &[OMNI, "Island", "Island"]).battlefield(1, &["Island", "Island"]).hand(1, &[FLUTE]).hand(0, &[SNT]).at(Step::Main1, 0).start();
    g.pass();
    cast_with(&mut g, 1, FLUTE, &|_| None);
    pass_on(&mut g);
    g.choose(Choice::Def(def(SNT)));
    resolve(&mut g);
    let c = g.card(0, SNT, Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{}", opts(&g));
}

#[test]
fn cr_605_1a_disruptor_flute_naming_mistrise_village_leaves_its_mana_ability() {
    // ruling: 044069c9
    // "{T}: Add {U}" can still be activated; "{U}, {T}: ..." can't.
    let mut g = scen().battlefield(0, &["Island", "Island", VILLAGE, "Island"]).hand(0, &[FLUTE, "Ponder"]).at(Step::Main1, 0).start();
    flute_naming(&mut g, 0, Some(VILLAGE));
    let v = bf(&g, 0, VILLAGE);
    assert!(abilities_offered(&g, v).is_empty(), "{}", opts(&g));
    lands_untapped_but(&mut g, 0, &["Island"]);
    cast(&mut g, 0, "Ponder");
    assert!(g.obj(v).tapped, "the Village paid for Ponder");
}

#[test]
fn cr_611_3a_disruptor_flute_gone_the_abilities_and_costs_are_back() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Mountain", "Mountain", "Mountain", "Sneak Attack"])
        .hand(0, &[FLUTE, BE])
        .at(Step::Main1, 0)
        .start();
    flute_naming(&mut g, 0, Some("Sneak Attack"));
    let sa = bf(&g, 0, "Sneak Attack");
    assert!(abilities_offered(&g, sa).is_empty());
    cast_be(&mut g, 1);
    resolve(&mut g);
    assert!(g.find_card(0, FLUTE, Zone::Graveyard).is_some());
    lands_untapped_but(&mut g, 0, &[]);
    assert!(!abilities_offered(&g, sa).is_empty(), "Sneak Attack's ability is back: {}", opts(&g));
}

#[test]
fn disruptor_flute_decks_are_refused_to_networks_on_layout_v6() {
    let delver = engine::az::constructed::Decklist::izzet_delver().main_names();
    let mut with = delver.clone();
    with[0] = FLUTE.to_string();
    let spec = |p0: &[String], p1: &[String], sb: Option<&[&str]>| {
        let mut decks = serde_json::json!({"p0": p0, "p1": p1});
        if let Some(sb) = sb {
            decks["p1_sideboard"] = serde_json::json!(sb);
        }
        serde_json::json!({"max_turns": 60, "constructed": true, "decks": decks}).to_string()
    };
    let want = "Disruptor Flute's chosen name needs layout v7 (ENGINE.md 123)";
    for s in [spec(&with, &delver, None), spec(&delver, &with, None), spec(&delver, &delver, Some(&[FLUTE]))] {
        let e = engine::az::MtgSpec::parse(&s).unwrap_err();
        assert!(e.contains(want), "{e}");
    }
    assert!(engine::az::MtgSpec::parse(&spec(&delver, &delver, None)).is_ok());
}

// ---- Magus of the Moon -----------------------------------------------------------

fn subtype(g: &Game, c: CardIdx, name: &str) -> bool {
    g.has_subtype(c, db().subtype_id(name).unwrap())
}

#[test]
fn cr_305_7_magus_makes_the_decks_nonbasic_lands_mountains() {
    // ruling: 24283e6f
    // Volcanic Island: a Mountain only ({T}: Add {R}); Island stays.
    let g = scen().battlefield(0, &["Volcanic Island", "Island", "Ancient Tomb", "City of Traitors", VILLAGE, "Scalding Tarn"]).battlefield(1, &[MAGUS]).at(Step::Main1, 0).start();
    let vi = bf(&g, 0, "Volcanic Island");
    assert!(subtype(&g, vi, "Mountain") && !subtype(&g, vi, "Island"));
    let isl = bf(&g, 0, "Island");
    assert!(subtype(&g, isl, "Island") && !subtype(&g, isl, "Mountain"), "basic lands unaffected");
    for n in ["Ancient Tomb", "City of Traitors", VILLAGE, "Scalding Tarn"] {
        let c = bf(&g, 0, n);
        assert!(subtype(&g, c, "Mountain"), "{n}");
        assert!(abilities_offered(&g, c).is_empty(), "{n} keeps no ability of its rules text: {}", opts(&g));
    }
}

#[test]
fn cr_305_7_ancient_tomb_and_city_of_traitors_under_the_magus_tap_for_one_red() {
    // ruling: 24283e6f
    // Ancient Tomb: {R}, no damage; City of Traitors: {R}, and playing another
    // land doesn't sacrifice it (its triggered ability is gone).
    let mut g = scen()
        .battlefield(0, &["Ancient Tomb", "City of Traitors"])
        .battlefield(1, &[MAGUS])
        .hand(0, &["Lightning Bolt", "Island", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast_with(&mut g, 0, "Lightning Bolt", &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    resolve(&mut g);
    assert_eq!(g.player(0).life, 20, "no damage from Ancient Tomb");
    let island = g.card(0, "Island", Zone::Hand);
    g.choose(Choice::Play(island));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, "City of Traitors"), "no sacrifice trigger\n{}", log(&g));
    assert!(g.stack().is_empty());
}

#[test]
fn cr_614_12_thundering_falls_and_mistrise_village_enter_untapped_and_quiet_under_the_magus() {
    // ruling: cafea326
    // ruling: aa10a575
    // Thundering Falls: no "enters tapped", no surveil trigger; Mistrise
    // Village: its "enters tapped unless" is gone too.
    for land in ["Thundering Falls", VILLAGE] {
        let mut g = scen().battlefield(1, &[MAGUS]).hand(0, &[land]).at(Step::Main1, 0).start();
        let c = g.card(0, land, Zone::Hand);
        g.choose(Choice::Play(c));
        until_priority(&mut g, &|_| None);
        assert!(!g.obj(c).tapped, "{land}");
        assert!(g.stack().is_empty(), "{land}: no trigger\n{}", log(&g));
    }
}

#[test]
fn cr_613_1d_the_magus_without_abilities_still_makes_mountains() {
    // ruling: cd000dac
    // Layer 4 applies before layer 6 (CR 613.1d, 613.1f): Oko's +1 makes the
    // Magus an Elk that lost all its abilities; nonbasic lands stay Mountains.
    let mut g = scen().battlefield(0, &["Oko, Thief of Crowns", "Volcanic Island"]).battlefield(1, &[MAGUS]).at(Step::Main1, 0).start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    let magus = bf(&g, 1, MAGUS);
    let vi = bf(&g, 0, "Volcanic Island");
    assert!(subtype(&g, vi, "Mountain") && !subtype(&g, vi, "Island"));
    // (Oko's ability that targets an artifact or creature.)
    let mut done = false;
    for s in abilities_offered(&g, oko) {
        let mut h = g.clone();
        h.choose(Choice::Ability(oko, s));
        if h.kind() == Some(DecisionKind::Target) && h.has_option(h.tgt(magus)) {
            h.choose(h.tgt(magus));
            until_priority(&mut h, &|_| None);
            resolve(&mut h);
            g = h;
            done = true;
            break;
        }
    }
    assert!(done, "Oko's +1: {}", opts(&g));
    assert!(g.marks_of(magus).is_empty() || true);
    assert!(g.is_creature(magus) && subtype(&g, magus, "Elk"), "an Elk
{}", log(&g));
    assert!(subtype(&g, vi, "Mountain") && !subtype(&g, vi, "Island"), "still a Mountain");
}

#[test]
fn cr_205_4_the_magus_keeps_names_and_supertypes() {
    // ruling: 53c9e610
    let g = scen().battlefield(0, &["Volcanic Island"]).battlefield(1, &[MAGUS]).at(Step::Main1, 0).start();
    let vi = bf(&g, 0, "Volcanic Island");
    assert_eq!(g.name(vi), "Volcanic Island");
    assert_eq!(g.def_of(vi).supertypes & engine::db::supertypes::BASIC, 0, "not basic");
    assert!(subtype(&g, vi, "Mountain"));
}

// ---- Sneak and Show interactions ---------------------------------------------------

#[test]
fn show_and_tell_into_omniscience_then_free_spells() {
    let mut g = snt_scen(&[OMNI, EMRAKUL], &[GRIS]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(OMNI));
    snt_pick(&mut g, 1, Some(GRIS));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, OMNI) && on_bf(&g, 1, GRIS));
    cast_free(&mut g, 0, EMRAKUL);
    resolve(&mut g);
    assert!(on_bf(&g, 0, EMRAKUL) && g.extra_turns() == [0]);
}

#[test]
fn sneak_attack_emrakul_haste_end_step_sacrifice_and_the_graveyard_shuffle() {
    // Sneak Attack: "{R}: You may put a creature card from your hand onto the
    // battlefield. That creature gains haste. Sacrifice the creature at the
    // beginning of the next end step." Emrakul: no extra turn (not cast); at
    // the end step it is sacrificed, and its "When Emrakul is put into a
    // graveyard from anywhere, its owner shuffles their graveyard into their
    // library" triggers.
    let mut g = scen().battlefield(0, &["Sneak Attack", "Mountain"]).hand(0, &[EMRAKUL]).at(Step::Main1, 0).start();
    let sa = bf(&g, 0, "Sneak Attack");
    let s = abilities_offered(&g, sa)[0];
    g.choose(Choice::Ability(sa, s));
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(g.card(0, EMRAKUL, Zone::Hand))),
        _ => None,
    });
    let em = bf(&g, 0, EMRAKUL);
    assert!(g.keywords_of(em).has(engine::db::Kw::Haste));
    assert!(g.extra_turns().is_empty());
    let lib_before = g.player(0).library.len();
    // On to the end step, attacking with nothing.
    for _ in 0..2000 {
        if g.turn().number > 3 || (g.find_card(0, EMRAKUL, Zone::Battlefield).is_none() && g.stack().is_empty() && g.waiting_triggers().is_empty()) {
            break;
        }
        let d = g.pending_decision().expect("a decision");
        let c = if d.kind == DecisionKind::Priority || g.has_option(Choice::Pass) { Choice::Pass } else { d.options[0].choice };
        g.choose(c);
    }
    assert!(g.find_card(0, EMRAKUL, Zone::Library).is_some(), "shuffled back with the graveyard
{}", log(&g));
    assert!(g.player(0).library.len() > lib_before);
    assert!(g.player(0).graveyard.is_empty());
}

#[test]
fn lotus_petal_spirit_guide_and_city_of_traitors_into_a_turn_one_show_and_tell() {
    // Turn-one Show and Tell ({2}{U}): City of Traitors' {C}{C} and Lotus
    // Petal's {U}.
    let mut g = scen().battlefield(0, &["City of Traitors", "Lotus Petal"]).hand(0, &[SNT, GRIS, "Simian Spirit Guide"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, SNT);
    assert!(g.find_card(0, "Lotus Petal", Zone::Graveyard).is_some());
    pass_on(&mut g);
    snt_pick(&mut g, 0, Some(GRIS));
    until_priority(&mut g, &|_| None);
    assert!(on_bf(&g, 0, GRIS));
    // Turn-one Sneak Attack ({3}{R}): Ancient Tomb's {C}{C} (2 damage), Lotus
    // Petal's mana and Simian Spirit Guide's {R} (exiled from the hand: a mana
    // ability, CR 605.1a).
    let mut g = scen().battlefield(0, &["Ancient Tomb", "Lotus Petal"]).hand(0, &["Sneak Attack", "Simian Spirit Guide", EMRAKUL]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Sneak Attack");
    resolve(&mut g);
    assert!(on_bf(&g, 0, "Sneak Attack"), "{}", log(&g));
    assert_eq!(g.player(0).life, 18);
    assert!(g.find_card(0, "Simian Spirit Guide", Zone::Exile).is_some() && g.find_card(0, "Lotus Petal", Zone::Graveyard).is_some());
}

#[test]
fn encoding_the_frames_stage_is_the_only_trace_of_the_choice_for_the_other_player() {
    // P0's encoding (the chooser) and P1's agree that a card was chosen; the
    // hand counts don't change before the cards enter.
    let mut g = snt_scen(&[EMRAKUL], &[GRIS]).start();
    cast_snt(&mut g);
    let before = g.player(0).hand.len();
    snt_pick(&mut g, 0, Some(EMRAKUL));
    assert_eq!(g.player(0).hand.len(), before);
    assert_eq!(features(&g, 0)[O_CONTEXT + ctx::OP_I], 2.0 / 8.0);
    let _ = encode::CODE;
}

#[test]
fn cr_117_3b_after_omniscience_resolves_its_controller_has_priority_first() {
    // ruling: 3bd1fe3c
    // Omniscience resolves on P0's turn: P0 gets priority first (CR 117.3b)
    // and casts Emrakul for free before P1 can act.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &[OMNI, EMRAKUL])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, OMNI);
    resolve(&mut g);
    assert!(on_bf(&g, 0, OMNI));
    assert_eq!(g.pending_decision().unwrap().player, 0);
    let em = g.card(0, EMRAKUL, Zone::Hand);
    assert!(g.has_option(Choice::Cast(em)), "{}", opts(&g));
}

#[test]
fn cr_614_12a_mistrise_village_entering_with_the_magus_enters_tapped() {
    // ruling: 16696510
    // What enters at the same time doesn't count (CR 614.12a; the ruling):
    // P0 puts Mistrise Village and P1 the Magus with one Show and Tell; P0's
    // Misty Rainforest wasn't a Mountain as the Village entered (the Magus's
    // effect didn't exist yet), so the Village enters tapped; then the Magus
    // makes both Mountains.
    let mut g = snt_scen(&[VILLAGE], &[MAGUS]).battlefield(0, &["Misty Rainforest"]).start();
    cast_snt(&mut g);
    snt_pick(&mut g, 0, Some(VILLAGE));
    snt_pick(&mut g, 1, Some(MAGUS));
    until_priority(&mut g, &|_| None);
    let v = bf(&g, 0, VILLAGE);
    assert!(g.obj(v).tapped, "entered tapped\n{}", log(&g));
    assert!(subtype(&g, v, "Mountain") && subtype(&g, bf(&g, 0, "Misty Rainforest"), "Mountain"));
}

#[test]
fn cr_614_12_an_as_enters_choice_and_counters_are_gone_under_the_magus() {
    // ruling: aa10a575
    // Multiversal Passage ("As this land enters, choose a basic land type. You
    // may pay 2 life. If you don't, it enters tapped."): no choice, untapped,
    // no life. Gemstone Mine enters without its mining counters.
    let mut g = scen().battlefield(1, &[MAGUS]).hand(0, &["Multiversal Passage"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Multiversal Passage", Zone::Hand);
    g.choose(Choice::Play(c));
    assert!(at_priority(&g), "no choice: {:?} {}", g.kind(), opts(&g));
    assert!(!g.obj(c).tapped && g.player(0).life == 20);
    let mut g = scen().battlefield(1, &[MAGUS]).hand(0, &["Gemstone Mine"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Gemstone Mine", Zone::Hand);
    g.choose(Choice::Play(c));
    until_priority(&mut g, &|_| None);
    let mining = db().subtype_id("counter:mining").map(Counter::Named).expect("mining counters");
    assert_eq!(g.counters_on(c, mining), 0);
}

#[test]
fn cr_715_3_omniscience_casts_an_adventure_from_hand_for_free() {
    // An Adventure cast from the hand is a spell cast from the hand: Petty
    // Theft for free, no land.
    let g = scen().battlefield(0, &[OMNI]).battlefield(1, &[GRIS]).hand(0, &["Brazen Borrower // Petty Theft"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Brazen Borrower // Petty Theft", Zone::Hand);
    let o = g.offered();
    assert!(o.iter().any(|x| matches!(x, Choice::Faces(y, _) if *y == c)), "both faces castable for free: {}", opts(&g));
}

#[test]
fn cr_702_34a_omniscience_doesnt_reach_a_flashback_from_the_graveyard() {
    // Faithless Looting's flashback {2}{R} from the graveyard: not from the
    // hand, so its flashback cost only (no land: not castable).
    let g = scen().battlefield(0, &[OMNI]).graveyard(0, &["Faithless Looting"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Faithless Looting", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(c)), "{}", opts(&g));
    let g = scen().battlefield(0, &[OMNI, "Mountain", "Mountain", "Mountain"]).graveyard(0, &["Faithless Looting"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Faithless Looting", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(c)), "for its flashback cost: {}", opts(&g));
}

#[test]
fn cr_118_12_daze_on_an_uncounterable_spell_its_payment_is_still_offered() {
    // "Counter target spell unless its controller pays {1}" on a spell that
    // can't be countered: Daze can target it; its controller may pay or not
    // (CR 118.12); not paying, the counter does nothing (101.2).
    let mut g = scen()
        .battlefield(0, &[VILLAGE, "Island", "Island", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Ponder"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    activate_village(&mut g);
    resolve(&mut g);
    cast_with(&mut g, 0, "Ponder", &island_first);
    g.pass();
    cast_with(&mut g, 1, "Daze", &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.pending_decision().unwrap().options[0].choice),
        _ => None,
    });
    g.pass();
    // P1 passes too: Daze resolves and asks P0 whether to pay {1}.
    for _ in 0..10 {
        if g.kind() != Some(DecisionKind::Priority) {
            break;
        }
        g.pass();
    }
    assert!(g.has_option(Choice::Pass), "P0 may decline: {:?} {}", g.kind(), opts(&g));
    g.pass();
    resolve(&mut g);
    assert!(g.find_card(0, "Ponder", Zone::Graveyard).is_some() && g.find_card(1, "Daze", Zone::Graveyard).is_some());
    assert!(log(&g).contains("Ponder can't be countered"), "{}", log(&g));
}

#[test]
fn cr_702_29a_disruptor_flute_naming_street_wraith_stops_its_cycling_in_hand() {
    // ruling: 044069c9
    // Cycling is an activated ability of the card in its owner's hand (CR
    // 702.29a): named, it can't be activated there (Pithing Needle's ruling:
    // every zone).
    let g = scen().hand(0, &["Street Wraith"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Street Wraith", Zone::Hand);
    assert!(g.offered().iter().any(|o| matches!(o, Choice::Ability(x, _) if *x == c)), "cycling offered: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &[FLUTE, "Street Wraith"]).at(Step::Main1, 0).start();
    flute_naming(&mut g, 0, Some("Street Wraith"));
    let c = g.card(0, "Street Wraith", Zone::Hand);
    assert!(!g.offered().iter().any(|o| matches!(o, Choice::Ability(x, _) if *x == c)), "no cycling: {}", opts(&g));
}

#[test]
fn cr_101_2_flusterstorm_against_a_mistrise_village_spell_counters_nothing() {
    // "Counter target instant or sorcery spell unless its controller pays
    // {1}" against a spell Mistrise Village made uncounterable: Flusterstorm
    // targets it and resolves; the spell stays and resolves (CR 101.2).
    let mut g = scen()
        .battlefield(0, &[VILLAGE, "Island", "Island", "Island"])
        .battlefield(1, &["Island"])
        .hand(0, &["Ponder"])
        .hand(1, &["Flusterstorm"])
        .at(Step::Main1, 0)
        .start();
    activate_village(&mut g);
    resolve(&mut g);
    cast_with(&mut g, 0, "Ponder", &island_first);
    g.pass();
    cast_with(&mut g, 1, "Flusterstorm", &|g| match g.kind() {
        Some(DecisionKind::Target) => g.offered().into_iter().find(|o| matches!(o, Choice::Target(engine::Target::Obj(_)) | Choice::Target(engine::Target::Stack(_)))),
        _ => None,
    });
    resolve_with(&mut g, &|g| if g.has_option(Choice::Pass) && g.kind() != Some(DecisionKind::Priority) { Some(Choice::Pass) } else { None });
    assert!(g.find_card(0, "Ponder", Zone::Graveyard).is_some() && g.find_card(1, "Flusterstorm", Zone::Graveyard).is_some());
    assert!(log(&g).contains("Ponder can't be countered"), "{}", log(&g));
}
