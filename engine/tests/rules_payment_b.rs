//! Workstream B of the faithfulness audit (docs/faithfulness_audit.md 5.3,
//! 5.4; ENGINE.md 100.4): mana and payment, exact. Each test names the CR
//! rule it checks and failed before its fix.

mod common;
mod eoe_w2;

use engine::db::{AbilitySlot, CardDb, MANA_UNCOUNTERABLE, SPEND_ARTIFACT, SPEND_LEGENDARY};
use engine::mana::{self, ManaCost};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

/// The cube with the shared test cards.
fn scen() -> Scenario {
    Scenario::new(common::db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

/// Answer decisions (`answer`, else the first option) until priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

/// Run the stack down, answering with the first option.
fn settle(g: &mut Game) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    panic!("settle: no progress\n{}", log(g));
}

/// Every option of a (possibly paged) decision, applied to a copy, then
/// `finish`: "next page" options are followed, so each item once.
fn each_item(g: &Game) -> Vec<Game> {
    let mut out = Vec::new();
    for c in options(g) {
        let mut h = g.clone();
        h.choose(c);
        if matches!(c, Choice::Page(_)) {
            out.extend(each_item(&h));
        } else {
            finish(&mut h, |_| None);
            out.push(h);
        }
    }
    out
}

// ---- 100.4.1: the feasibility search -----------------------------------------

/// Gene Pollinator ("{T}, Tap an untapped permanent you control: Add one
/// mana of any color") with many lands: every land may be tapped for its own
/// ability or for a Pollinator's (CR 601.2g, 605.3a), and the payment's
/// feasibility is decided exactly, whatever the number of lands (EoE fuzz
/// seed 10023: two Pollinators and 13 lands made each check a search of 3^13
/// leaves, made thousands of times per decision; with 20 lands one check
/// passed the search's node limit, a loud failure; ENGINE.md 100.4).
#[test]
fn cr_601_2g_two_gene_pollinators_and_twenty_lands_are_decided_exactly() {
    let lands = ["Island", "Swamp", "Mountain", "Forest"].repeat(5);
    let mut bfs: Vec<&str> = vec!["Gene Pollinator", "Gene Pollinator"];
    bfs.extend(lands.iter().copied());
    let g = eoe_w2::scen().battlefield(0, &bfs).at(Step::Main1, 0).start();
    // Each Pollinator taps a land for {W}: two {W}, eighteen lands left.
    assert!(g.can_pay(0, &cost("{W}{W}{18}")));
    // One more is too much: a third {W} needs a third activation, and a
    // Pollinator tapping the other makes one {W} where two lands made two.
    assert!(!g.can_pay(0, &cost("{W}{W}{19}")));
    assert!(!g.can_pay(0, &cost("{W}{W}{W}")));
    // Twenty-one: one Pollinator taps the other, every land for itself.
    assert!(g.can_pay(0, &cost("{W}{20}")));
    assert!(!g.can_pay(0, &cost("{22}")));
}

// ---- 100.4.2: the pool's mana: restrictions, riders, every remainder ---------

/// CR 106.6: a spending restriction goes with the mana. Mishra's Workshop's
/// {C}{C}{C} floated in response to Wasteland (ENGINE.md 86) is still
/// "only to cast artifact spells" in the pool (docs/shortcuts.md N19: the
/// cube's table dropped the restriction).
#[test]
fn cr_106_6_floated_workshop_mana_keeps_its_restriction() {
    let mut g = Scenario::new(CardDb::constructed())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(1, &["Wasteland"])
        .battlefield(0, &["Mishra's Workshop"])
        .at(Step::Main1, 1)
        .start();
    let waste = bf(&g, 1, "Wasteland");
    let ws = bf(&g, 0, "Mishra's Workshop");
    g.choose(act(waste, 1));
    g.choose(g.tgt(ws));
    g.drive_until(at_priority);
    g.pass();
    assert!(at_priority(&g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(act(ws, 0)), "{:?}", g.options_desc());
    g.choose(act(ws, 0));
    finish(&mut g, |_| None);
    assert_eq!(g.player(0).pool.amt[mana::C], 3, "{}", log(&g));
    assert_eq!(g.restricted_mana(0), vec![(mana::C as u8, SPEND_ARTIFACT, 3)], "artifact spells only");
    assert!(!g.can_pay(0, &cost("{1}")), "not for anything else");
}

/// CR 106.6: a rider goes with the mana as well. Delighted Halfling's "Spend
/// this mana only to cast a legendary spell, and that spell can't be
/// countered": a legendary spell paid with such mana from the pool can't be
/// countered (docs/shortcuts.md N20: the pool had no such tag).
#[test]
fn cr_106_6_the_halfling_rider_stays_with_mana_in_the_pool() {
    let mut g = scen()
        .battlefield(0, &["Forest"])
        .pool_restricted(0, mana::W, SPEND_LEGENDARY | MANA_UNCOUNTERABLE, 1)
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Test Legend"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let leg = g.card(0, "Test Legend", Zone::Hand);
    g.choose(Choice::Cast(leg));
    finish(&mut g, |_| None);
    assert_eq!(g.player(0).pool.amt, [0; 6], "the pool's {{W}} paid: {}", log(&g));
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    assert!(g.has_option(Choice::Cast(cs)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(cs));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(leg).zone, Zone::Battlefield, "it can't be countered: {}", log(&g));
    assert_eq!(g.obj(cs).zone, Zone::Graveyard);
}

/// CR 106.4, 601.2h: which pool mana pays is the player's choice, also
/// between Mishra's Workshop's artifact-only {C} and another {C} (13.1k:
/// the restricted {C} was always spent first, "never worse"; the pools left
/// differ, and nothing turns plain {C} into the restricted kind).
#[test]
fn cr_601_2h_restricted_or_plain_colorless_for_an_artifact() {
    let mut g = scen().pool(0, [0, 0, 0, 0, 0, 1], 0).pool_restricted(0, mana::C, SPEND_ARTIFACT, 1).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let sr = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(sr));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<u8> = each_item(&g).iter().map(|h| h.player(0).pool.art).collect();
    left.sort();
    assert_eq!(left, vec![0, 1], "the plain {{C}} left, or the restricted one");
}

/// CR 106.4, 601.2h (Arena of Glory's haste mana): on a noncreature spell
/// too, the haste {R} or another {R} may pay (13.1k: the haste {R} was always
/// kept).
#[test]
fn cr_601_2h_haste_red_may_pay_a_noncreature_spell() {
    let mut g = scen().pool(0, [0, 0, 0, 2, 0, 0], 1).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(engine::Target::Player(1)));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<u8> = each_item(&g).iter().map(|h| h.player(0).pool.haste).collect();
    left.sort();
    assert_eq!(left, vec![0, 1]);
}

/// CR 106.4, 601.2h: every pool a payment can leave is offered, however many
/// (docs/shortcuts.md N11: at most 32, after a ranking). Three of each type
/// in the pool and a {6} spell: 336 pools left, paged (A0's helper).
#[test]
fn cr_601_2h_every_pool_left_is_offered() {
    let mut g = scen().pool(0, [3, 3, 3, 3, 3, 3], 0).hand(0, &["Coveted Jewel"]).at(Step::Main1, 0).start();
    let w = g.card(0, "Coveted Jewel", Zone::Hand);
    g.choose(Choice::Cast(w));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<[u8; 6]> = each_item(&g).iter().map(|h| h.player(0).pool.amt).collect();
    let n = left.len();
    left.sort();
    left.dedup();
    assert_eq!((n, left.len()), (336, 336), "each pool once");
    assert!(left.iter().all(|a| a.iter().map(|&x| x as u32).sum::<u32>() == 12 && a.iter().all(|&x| x <= 3)));
}

// ---- 100.4.3: mana abilities at any time (F3) ---------------------------------

/// The `ActivateMana` options' choices.
fn act_mana_options(g: &Game) -> Vec<Choice> {
    assert_eq!(g.kind(), Some(DecisionKind::ActivateMana), "{:?}\n{}", g.options_desc(), log(g));
    options(g)
}

/// CR 605.3a, 117.1d: a player may activate a mana ability whenever they
/// have priority, and the mana stays in the pool (106.4): `YES` at
/// priority, then the source; `PASS` (after the first) ends it, and the
/// player still has priority (605.3b; docs/shortcuts.md 13.12: mana
/// abilities were offered with priority only for a source about to be lost).
#[test]
fn cr_605_3a_mana_abilities_at_an_ordinary_priority() {
    let mut g = scen().battlefield(0, &["Island", "Mountain"]).at(Step::Main1, 0).start();
    let island = bf(&g, 0, "Island");
    let mountain = bf(&g, 0, "Mountain");
    assert!(g.has_option(Choice::Yes), "{:?}", g.options_desc());
    g.choose(Choice::Yes);
    let o = act_mana_options(&g);
    assert!(o.contains(&Choice::Card(island)) && o.contains(&Choice::Card(mountain)), "{o:?}");
    assert!(!o.contains(&Choice::Pass), "the first pick is an activation");
    g.choose(Choice::Card(island));
    let o = act_mana_options(&g);
    assert_eq!(o, vec![Choice::Pass, Choice::Card(mountain)], "the Island is tapped");
    g.choose(Choice::Pass);
    assert!(at_priority(&g) && g.pending_decision().unwrap().player == 0, "{}", log(&g));
    assert_eq!(g.player(0).pool.amt, [0, 1, 0, 0, 0, 0]);
    assert!(g.obj(island).tapped && !g.obj(mountain).tapped);
    // (The step's mana: it empties at the end of the step, CR 500.4.)
}

/// CR 605.3a (13.4): a Spirit Guide's mana ability from hand, with
/// priority: the card is exiled (the hand is smaller now) and {R} floats.
#[test]
fn cr_605_3a_a_spirit_guide_with_priority() {
    let mut g = scen().hand(0, &["Simian Spirit Guide"]).at(Step::Main1, 0).start();
    let ssg = g.card(0, "Simian Spirit Guide", Zone::Hand);
    g.choose(Choice::Yes);
    let o = act_mana_options(&g);
    assert_eq!(o.len(), 1, "{o:?}");
    g.choose(o[0]);
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::ActivateMana)).then_some(Choice::Pass));
    assert_eq!(g.obj(ssg).zone, Zone::Exile);
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 1, 0, 0]);
    assert_eq!(g.player(0).hand.len(), 0);
}

/// CR 601.2h by 602.2b: Gene Pollinator's "{T}, Tap an untapped permanent
/// you control" with priority: the permanent tapped is the player's choice
/// (one of each, ChoosePermanent), then the colour.
#[test]
fn cr_602_2b_gene_pollinator_with_priority_taps_the_chosen_permanent() {
    let mut g = eoe_w2::scen().battlefield(0, &["Gene Pollinator", "Island", "Swamp"]).at(Step::Main1, 0).start();
    let pol = bf(&g, 0, "Gene Pollinator");
    let island = bf(&g, 0, "Island");
    let swamp = bf(&g, 0, "Swamp");
    g.choose(Choice::Yes);
    g.choose(Choice::Card(pol));
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "{:?}", g.options_desc());
    let o = options(&g);
    assert!(o.contains(&Choice::Card(island)) && o.contains(&Choice::Card(swamp)) && !o.contains(&Choice::Card(pol)), "{o:?}");
    g.choose(Choice::Card(swamp));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana), "{:?}", g.options_desc());
    let green = options(&g).into_iter().find(|&c| {
        let mut h = g.clone();
        h.choose(c);
        h.player(0).pool.amt[mana::G] == 1
    });
    g.choose(green.expect("green among the colours"));
    g.choose(Choice::Pass);
    assert!(g.obj(pol).tapped && g.obj(swamp).tapped && !g.obj(island).tapped, "{}", log(&g));
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 1, 0]);
}

/// CR 106.1a, 605.3a: Cascading Cataracts' "{5}, {T}: Add five mana in any
/// combination of colors" with priority: each of the 126 productions is a
/// choice (paged past the `NUMBER` ids, A0's helper).
#[test]
fn cr_106_1a_cascading_cataracts_with_priority_offers_every_production() {
    let mut g = eoe_w2::scen().battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let cat = bf(&g, 0, "Cascading Cataracts");
    g.choose(Choice::Yes);
    g.choose(Choice::Card(cat));
    // Which ability: {T}: Add {C}, or the five.
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{:?}", g.options_desc());
    let five = options(&g).into_iter().find(|&c| {
        let mut h = g.clone();
        h.choose(c);
        h.kind() != Some(DecisionKind::ActivateMana)
    });
    g.choose(five.expect("the {5} ability"));
    // Its {5}: five Islands (the payment's decisions, first options).
    while matches!(g.kind(), Some(DecisionKind::PayMana | DecisionKind::PayPool)) {
        g.choose(options(&g)[0]);
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana), "{:?}\n{}", g.options_desc(), log(&g));
    let pools: Vec<[u8; 6]> = each_item_until(&g, DecisionKind::ChooseMana).iter().map(|h| h.player(0).pool.amt).collect();
    let mut distinct = pools.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!((pools.len(), distinct.len()), (126, 126), "each production once");
}

/// Every item of a (possibly paged) decision of `kind`, applied to a copy:
/// "next page" options followed, nothing more answered.
fn each_item_until(g: &Game, kind: DecisionKind) -> Vec<Game> {
    let mut out = Vec::new();
    for c in options(g) {
        let mut h = g.clone();
        h.choose(c);
        if matches!(c, Choice::Page(_)) && h.kind() == Some(kind) {
            out.extend(each_item_until(&h, kind));
        } else {
            out.push(h);
        }
    }
    out
}

/// CR 601.2g-h (13.13): while paying, a source may pay instead of the
/// pool's mana: {R} in the pool and an Island, Sol Ring ({1}): `PASS` spends
/// the {R}, or the Island pays and the {R} stays (for a Lightning Bolt
/// later). The payment spent the pool's mana whenever it could.
#[test]
fn cr_601_2g_a_land_pays_while_the_pool_keeps_its_mana() {
    let mut g = scen().pool(0, [0, 0, 0, 1, 0, 0], 0).battlefield(0, &["Island"]).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let island = bf(&g, 0, "Island");
    let sr = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(sr));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
    let o = options(&g);
    assert!(o.contains(&Choice::Pass) && o.contains(&act(island, 0)), "{o:?}");
    let mut h = g.clone();
    h.choose(Choice::Pass);
    finish(&mut h, |_| None);
    assert_eq!((h.player(0).pool.amt, h.obj(island).tapped), ([0; 6], false), "the {{R}} paid");
    g.choose(act(island, 0));
    finish(&mut g, |_| None);
    assert_eq!((g.player(0).pool.amt, g.obj(island).tapped), ([0, 0, 0, 1, 0, 0], true), "the Island paid: {}", log(&g));
}

/// CR 601.2g (13.14): Wizard's Rockets is a source like any other: with a
/// Mountain that pays Lightning Bolt, the Rockets may pay instead (the
/// Mountain stays untapped). It was offered only when nothing else paid.
#[test]
fn cr_601_2g_wizards_rockets_pays_while_a_land_could() {
    let mut g = scen().battlefield(0, &["Mountain", "Wizard's Rockets", "Island"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let rockets = bf(&g, 0, "Wizard's Rockets");
    let mountain = bf(&g, 0, "Mountain");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(engine::Target::Player(1)));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
    let o = options(&g);
    assert!(o.contains(&act(mountain, 0)), "{o:?}");
    let pick = *o.iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == rockets)).unwrap_or_else(|| panic!("the Rockets: {o:?}"));
    g.choose(pick);
    // The Island pays the Rockets' X (its {U} turned into {R}).
    let island = bf(&g, 0, "Island");
    finish(&mut g, |g| g.has_option(act(island, 0)).then(|| act(island, 0)));
    settle(&mut g);
    assert_eq!(g.life(1), 17, "{}", log(&g));
    assert!(!g.obj(mountain).tapped, "the Mountain kept");
    assert_ne!(g.obj(rockets).zone, Zone::Battlefield, "the Rockets sacrificed");
}

// ---- 100.4.5 (B2): restrictions on needs (N21), granted mana abilities -------

/// CR 106.6, 602.2b: Mishra's Workshop's {C} may be spent only to cast
/// artifact spells, so it never pays Chromatic Star's {1} (activating an
/// ability is not casting a spell), even while an artifact is being cast
/// (docs/shortcuts.md N21: `mana::fits` read types only). With an Island to
/// pay the Star, the {1}{U} artifact is castable.
#[test]
fn cr_106_6_workshop_mana_never_pays_a_mana_abilitys_cost() {
    let g = scen().battlefield(0, &["Mishra's Workshop", "Chromatic Star"]).hand(0, &["Cryogen Relic"]).at(Step::Main1, 0).start();
    let relic = g.card(0, "Cryogen Relic", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(relic)), "the Star's {{1}} has no mana to pay it: {:?}", g.options_desc());
    let mut g = scen().battlefield(0, &["Mishra's Workshop", "Chromatic Star", "Island"]).hand(0, &["Cryogen Relic"]).at(Step::Main1, 0).start();
    let relic = g.card(0, "Cryogen Relic", Zone::Hand);
    assert!(g.has_option(Choice::Cast(relic)), "the Island pays the Star: {:?}", g.options_desc());
    g.choose(Choice::Cast(relic));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(relic).zone, Zone::Stack, "{}", log(&g));
    assert!(g.obj(bf(&g, 0, "Island")).tapped && g.obj(bf(&g, 0, "Mishra's Workshop")).tapped);
}

/// CR 106.6, 605.3a: mana that may be spent only on abilities of lands (Sunken
/// Citadel's, W2's Test Citadel) pays Cascading Cataracts' {5} while a spell
/// is cast, and the Cataracts' five mana pays the spell (docs/shortcuts.md
/// N21: a source whose mana the spell can't take was never offered).
#[test]
fn cr_106_6_land_ability_mana_pays_cataracts_while_a_spell_is_cast() {
    let mut g = eoe_w2::scen()
        .battlefield(0, &["Test Citadel", "Cascading Cataracts", "Island", "Island", "Island"])
        .hand(0, &["Starbreach Whale"])
        .at(Step::Main1, 0)
        .start();
    let whale = g.card(0, "Starbreach Whale", Zone::Hand);
    assert!(g.has_option(Choice::Cast(whale)), "{:?}", g.options_desc());
    assert!(!g.can_pay(0, &cost("{6}")));
    g.choose(Choice::Cast(whale));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(whale).zone, Zone::Stack, "{}", log(&g));
    for name in ["Test Citadel", "Cascading Cataracts"] {
        assert!(g.obj(bf(&g, 0, name)).tapped, "{name}: {}", log(&g));
    }
    assert_eq!(g.player(0).pool.amt, [0; 6]);
    assert!(g.restricted_mana(0).is_empty(), "{:?}", g.restricted_mana(0));
}

/// CR 602.2b, 601.2g (ENGINE.md 49: castability and the payment agree): a
/// Gene Pollinator whose own cost taps it (Ragost's Food ability, "{2}, {T},
/// Sacrifice this artifact: You gain 3 life", on the artifact Pollinator) can't
/// activate its mana ability while paying it, so it doesn't count among the
/// Pollinators that could tap another permanent for mana (EoE fuzz seed 11752 of
/// W6's table: offered, then "PayMana: no way to pay {2}"). With a second
/// Pollinator it is payable.
#[test]
fn cr_602_2b_a_pollinator_paying_its_own_tap_cost_is_no_activator() {
    let g = eoe_w2::scen().battlefield(0, &["Ragost, Deft Gastronaut", "Gene Pollinator", "Forest"]).at(Step::Main1, 0).start();
    let pol = bf(&g, 0, "Gene Pollinator");
    assert!(!g.has_option(act(pol, 1)), "only the Forest pays: {:?}", g.options_desc());
    let mut g = eoe_w2::scen().battlefield(0, &["Ragost, Deft Gastronaut", "Gene Pollinator", "Gene Pollinator", "Forest"]).at(Step::Main1, 0).start();
    let pols: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Gene Pollinator").collect();
    assert!(g.has_option(act(pols[0], 1)), "the other Pollinator taps Ragost: {:?}", g.options_desc());
    g.choose(act(pols[0], 1));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.life(0), 23, "{}", log(&g));
    assert_ne!(g.obj(pols[0]).zone, Zone::Battlefield);
}

/// CR 305.6, 305.7 (its last sentence): a land given a basic land type in
/// addition to its others has that type's intrinsic mana ability and keeps
/// its own (docs/shortcuts.md N22: refused at lowering). W2's Test Forest
/// Maker becomes a Forest until end of turn: it taps for {G} or {C}, and
/// with priority it offers both abilities.
#[test]
fn cr_305_6_a_land_given_forest_taps_for_green() {
    let mut g = eoe_w2::scen().battlefield(0, &["Test Forest Maker", "Island"]).at(Step::Main1, 0).start();
    let fm = bf(&g, 0, "Test Forest Maker");
    let island = bf(&g, 0, "Island");
    assert!(!g.can_pay(0, &cost("{G}")));
    g.choose(act(fm, 1));
    finish(&mut g, |g| options(g).into_iter().find(|&c| c == act(island, 0)));
    settle(&mut g);
    assert!(g.obj(island).tapped && !g.obj(fm).tapped, "{}", log(&g));
    assert!(g.can_pay(0, &cost("{G}")), "the Forest's {{T}}: Add {{G}}: {}", log(&g));
    assert!(g.can_pay(0, &cost("{C}")), "its own {{T}}: Add {{C}}");
    assert!(!g.can_pay(0, &cost("{G}{C}")), "one tap");
    // With priority: its own ability and the Forest's (at the place after
    // its own two).
    g.choose(Choice::Yes);
    assert!(options(&g).contains(&Choice::Card(fm)), "{:?}", g.options_desc());
    g.choose(Choice::Card(fm));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{:?}", g.options_desc());
    let pools: Vec<[u8; 6]> = options(&g)
        .into_iter()
        .map(|c| {
            let mut h = g.clone();
            h.choose(c);
            h.player(0).pool.amt
        })
        .collect();
    assert_eq!(pools, vec![[0, 0, 0, 0, 0, 1], [0, 0, 0, 0, 1, 0]], "{{C}} or {{G}}");
}

/// CR 106.6, 601.2g: a land with "{T}: Add {B}" and "{T}: Add {B}{B}. Spend
/// this mana only to activate abilities of lands" (Sunken Citadel) pays a
/// spell's {B} with the first, even while Cascading Cataracts' {5} could take
/// the second's mana (EoE fuzz seed 3271 of W6's table, on 755fe0b: the
/// feasibility took the restricted {B}{B} for a superset of the plain {B},
/// its tag having more bits, so the land's plain {B} was lost and a payment
/// that picked Cataracts' {5} found no way to finish).
#[test]
fn cr_106_6_restricted_mana_never_stands_for_a_lands_plain_mana() {
    let g = eoe_w2::scen().battlefield(0, &["Test Sunken Citadel", "Cascading Cataracts"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{B}")), "the Citadel's plain {{B}}");
    assert!(g.can_pay(0, &cost("{B}{1}")), "and the Cataracts' {{C}}");
    assert!(!g.can_pay(0, &cost("{B}{B}")));
    let g = eoe_w2::scen()
        .battlefield(0, &["Test Sunken Citadel", "Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    // The Islands pay the Cataracts' {5}, its five mana and the Citadel's {B}
    // the spell; or the Citadel's {B}{B} pays two of the {5}, and two Islands
    // are left for the spell.
    assert!(g.can_pay(0, &cost("{5}{B}")));
    assert!(g.can_pay(0, &cost("{6}{B}")));
    assert!(!g.can_pay(0, &cost("{7}{B}")));
}

// ---- 100.4.6: B3, the last of mana and payment ----------------------------

fn charge() -> engine::db::Counter {
    engine::db::Counter::Named(common::db().subtype_id("counter:charge").expect("charge counters"))
}

/// docs/shortcuts.md N10, CR 122.1, 605.1a: Pentad Prism's "Remove a charge
/// counter from Pentad Prism: Add one mana of any color" is activated once
/// per counter, however many it has (counters have no maximum). With 40 it
/// pays {33} and Walking Ballista for X = 20 ({X}{X}); with the old 32 uses a
/// payment it could make was refused.
#[test]
fn cr_122_1_pentad_prism_pays_with_every_charge_counter() {
    let g = scen().battlefield(0, &["Pentad Prism"]).counters(0, "Pentad Prism", charge(), 40).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{33}")), "33 of its 40 counters");
    assert!(g.can_pay(0, &cost("{40}")));
    assert!(!g.can_pay(0, &cost("{41}")));
    assert!(g.can_pay(0, &cost("{W}{U}{B}{R}{G}{35}")));
    let mut g = scen()
        .battlefield(0, &["Pentad Prism"])
        .counters(0, "Pentad Prism", charge(), 40)
        .hand(0, &["Walking Ballista"])
        .at(Step::Main1, 0)
        .start();
    let ballista = g.card(0, "Walking Ballista", Zone::Hand);
    g.choose(Choice::Cast(ballista));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "X: {:?}", g.options_desc());
    // (X is asked with A0's digits past 31: take the largest each time.)
    for _ in 0..8 {
        if g.kind() != Some(DecisionKind::ChooseMode) {
            break;
        }
        let last = *options(&g).last().unwrap();
        g.choose(last);
    }
    finish(&mut g, |_| None);
    let prism = bf(&g, 0, "Pentad Prism");
    assert_eq!(g.counters_on(prism, charge()), 0, "X = 20 took all 40\n{}", log(&g));
    assert_eq!(g.stack().len(), 1);
}

/// The cube with the shared test cards and B3's own (ENGINE.md 100.4.6), a
/// table of its own (the cube's fuzz pool and digests keep theirs).
fn b3_db() -> &'static CardDb {
    use serde_json::json;
    static DB: std::sync::OnceLock<&'static CardDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let mana = |cost: serde_json::Value, g: i64| {
            json!({"$t": "ActivatedAbility", "cost": cost, "effect": {"$t": "AddManaToPool", "produces": [["G", g]]}, "is_mana_ability": true})
        };
        let creature = |name: &str, sub: &str, p: i64, abilities: serde_json::Value| {
            eoe_w2::card(name, json!({"card_types": ["Creature"], "subtypes": [sub], "power": p, "toughness": 1, "type_line": format!("Creature - {sub}"),
                "mana_cost": {"$t": "ManaCost", "colored": [["G", 1]]}, "mana_cost_str": "{G}", "activated_abilities": abilities}))
        };
        let custom = vec![
            // "Sacrifice this creature: Add {G}."
            creature("Test Sac Elf", "Elf", 1, json!([mana(json!({"$t": "ActivationCost", "sacrifice_self": true}), 1)])),
            // A Rebel with "{T}: Add {G}."
            creature("Test Mana Rebel", "Rebel", 2, json!([mana(json!({"$t": "ActivationCost", "tap_self": true}), 1)])),
        ];
        let land = |name: &str, draw_cost: serde_json::Value| {
            eoe_w2::card(name, json!({"card_types": ["Land"], "type_line": "Land", "activated_abilities": [
                mana(json!({"$t": "ActivationCost", "tap_self": true}), 1),
                {"$t": "ActivatedAbility", "cost": draw_cost, "effect": {"$t": "DrawCards", "amount": 1}}]}))
        };
        let one = json!({"$t": "ManaCost", "generic": 1});
        let custom = custom.into_iter().chain([
            // "{T}: Add {G}. {1}, Sacrifice this land: Draw a card."
            land("Test Cycle Land", json!({"$t": "ActivationCost", "mana": one, "mana_str": "{1}", "sacrifice_self": true})),
            // "{T}: Add {G}. {1}, {T}: Draw a card."
            land("Test Tap Draw Land", json!({"$t": "ActivationCost", "mana": one, "mana_str": "{1}", "tap_self": true})),
        ]);
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(common::custom_cards());
        cards.extend(custom.into_iter().map(|v| serde_json::from_value::<card_ir::Card>(v).expect("custom card IR")));
        CardDb::from_cards(&cards).leak()
    })
}

fn b3_scen() -> Scenario {
    Scenario::new(b3_db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

/// docs/shortcuts.md 27.13.4, CR 601.2g-h: Otharri, Suns' Glory's "{2}{R}{W},
/// Tap an untapped Rebel you control: Return this card from your graveyard to
/// the battlefield tapped" may tap a Rebel with a mana ability while lands pay
/// the mana (the mana abilities come first, the Rebel is tapped after). Only
/// Rebels without one counted before. The Rebel can't do both: with three
/// lands the ability isn't offered.
#[test]
fn cr_601_2h_a_rebel_with_a_mana_ability_pays_a_tap_cost() {
    let lands = ["Mountain", "Plains", "Mountain", "Plains"];
    let mut g = b3_scen().battlefield(0, &lands).battlefield(0, &["Test Mana Rebel"]).graveyard(0, &["Otharri, Suns' Glory"]).at(Step::Main1, 0).start();
    let o = g.card(0, "Otharri, Suns' Glory", Zone::Graveyard);
    let back = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == o));
    let back = back.unwrap_or_else(|| panic!("Otharri's ability is offered: {:?}", g.options_desc()));
    g.choose(back);
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(o).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.obj(bf(&g, 0, "Test Mana Rebel")).tapped, "the Rebel tapped for the cost");
    let g = b3_scen().battlefield(0, &lands[..3]).battlefield(0, &["Test Mana Rebel"]).graveyard(0, &["Otharri, Suns' Glory"]).at(Step::Main1, 0).start();
    let o = g.card(0, "Otharri, Suns' Glory", Zone::Graveyard);
    assert!(!options(&g).iter().any(|c| matches!(c, Choice::Ability(x, _) if *x == o)), "the Rebel can't pay the mana and the tap");
}

/// P1 casts Lightning Bolt at P0, who then casts We Say Thee Nay! (teamwork
/// 2: "you may tap any number of untapped creatures you control with total
/// power 2 or more") with the board `mine`: the game at the teamwork choice
/// (`ChooseCost`), and the Bolt.
fn nay_with(mine: &[&str]) -> (Game, CardIdx) {
    let mut g = b3_scen()
        .battlefield(0, mine)
        .hand(0, &["We Say Thee Nay!"])
        .battlefield(1, &["Mountain", "Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.choose(Choice::Target(engine::Target::Player(0)));
    finish(&mut g, |_| None);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let n = g.card(0, "We Say Thee Nay!", Zone::Hand);
    g.choose(Choice::Cast(n));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseCost));
    (g, b)
}

/// docs/shortcuts.md N8, CR 702.122a-like teamwork, 601.2g-h: with ten Birds
/// of Paradise (power 0) and then two Llanowar Elves, teamwork 2 taps the two
/// Elves and two Birds pay {1}{U}. The castability tried the subsets of only
/// the first ten creatures with mana abilities, so the Elves never counted
/// and teamwork wasn't offered.
#[test]
fn cr_601_2h_teamwork_counts_every_creature_with_a_mana_ability() {
    let mut mine = vec!["Birds of Paradise"; 10];
    mine.extend(["Llanowar Elves", "Llanowar Elves"]);
    let (mut g, b) = nay_with(&mine);
    assert!(g.has_option(Choice::Number(1)), "teamwork offered: {:?}", g.options_desc());
    g.choose(Choice::Number(1));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(b)),
        _ => None,
    });
    let elf = b3_db().id("Llanowar Elves").unwrap();
    let elves: Vec<&engine::game::Obj> = g.objs().iter().filter(|o| o.zone == Zone::Battlefield && o.def == elf).collect();
    assert_eq!(elves.len(), 2);
    assert!(elves.iter().all(|o| o.tapped), "both Elves tapped for teamwork: {}", log(&g));
    assert_eq!(g.stack().len(), 2, "Nay cast\n{}", log(&g));
}

/// docs/shortcuts.md 13.17, CR 601.2g-h: a creature tapped for teamwork is
/// tapped after the spell's mana abilities, so none of them sacrificed it.
/// Two Test Sac Elves ("Sacrifice this creature: Add {G}") and an Island: the
/// Elves tapped for teamwork, {1}{U} isn't payable (castability says so);
/// with two Islands, the payment doesn't offer the tapped Elves'
/// sacrifices (the engine taps them first; before, it offered them).
#[test]
fn cr_601_2g_a_creature_tapped_for_teamwork_pays_no_mana() {
    let (g, _) = nay_with(&["Test Sac Elf", "Test Sac Elf", "Island"]);
    assert!(!g.has_option(Choice::Number(1)), "teamwork leaves {{1}}{{U}} unpaid: {:?}", g.options_desc());
    let (mut g, b) = nay_with(&["Test Sac Elf", "Test Sac Elf", "Island", "Island"]);
    assert!(g.has_option(Choice::Number(1)));
    g.choose(Choice::Number(1));
    let elf = b3_db().id("Test Sac Elf").unwrap();
    for _ in 0..40 {
        match g.kind() {
            Some(DecisionKind::Target) => g.choose(g.tgt(b)),
            Some(DecisionKind::PayMana) => {
                let elves: Vec<CardIdx> = options(&g)
                    .into_iter()
                    .filter_map(|c| match c {
                        Choice::Ability(x, _) | Choice::Card(x) if g.obj(x).def == elf => Some(x),
                        _ => None,
                    })
                    .collect();
                assert!(elves.is_empty(), "a tapped Elf sacrificed for the spell's mana: {:?}", g.options_desc());
                let c = options(&g)[0];
                g.choose(c)
            }
            _ if at_priority(&g) => break,
            _ => {
                let c = options(&g)[0];
                g.choose(c)
            }
        };
    }
    assert_eq!(g.stack().len(), 2, "Nay cast: {}", log(&g));
}

/// docs/shortcuts.md 13.3, CR 601.2g-h: a cost's source pays the cost's mana
/// with a mana ability that doesn't conflict with it. "{T}: Add {G}" and
/// "{1}, Sacrifice this land: Draw a card": the land taps for {G}, then is
/// sacrificed (the source was excluded whole before). "{1}, {T}: Draw a card"
/// can't be paid by its own {T} (both tap it): with a Forest it can.
#[test]
fn cr_601_2g_a_source_taps_for_mana_then_is_sacrificed_for_its_cost() {
    let draws = |g: &Game, name: &str| {
        let c = g.card(0, name, Zone::Battlefield);
        options(g).into_iter().filter(|o| matches!(o, Choice::Ability(x, AbilitySlot::Activated(1)) if *x == c)).collect::<Vec<_>>()
    };
    let mut g = b3_scen().battlefield(0, &["Test Cycle Land"]).at(Step::Main1, 0).start();
    let land = g.card(0, "Test Cycle Land", Zone::Battlefield);
    let opt = draws(&g, "Test Cycle Land");
    assert_eq!(opt.len(), 1, "the draw ability offered: {:?}", g.options_desc());
    g.choose(opt[0]);
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(land).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), 1, "drew a card: {}", log(&g));
    let g = b3_scen().battlefield(0, &["Test Tap Draw Land"]).at(Step::Main1, 0).start();
    assert!(draws(&g, "Test Tap Draw Land").is_empty(), "its own {{T}} can't pay {{1}}, {{T}}");
    let g = b3_scen().battlefield(0, &["Test Tap Draw Land", "Forest"]).at(Step::Main1, 0).start();
    assert_eq!(draws(&g, "Test Tap Draw Land").len(), 1);
}

/// docs/shortcuts.md 13.16, CR 601.2g-h, 702.66a: Black Lotus ("{T}, Sacrifice
/// this artifact: Add three mana of any one color") sacrificed for mana is in
/// the graveyard before delve's cards are exiled, so it is one of them in the
/// payment it helps. Treasure Cruise ({7}{U}, delve) with the Lotus, an Island
/// and three cards in the graveyard: three mana, the Lotus and the three
/// cards delved, the Island: eight. Only one-mana sources (Lotus Petal) were
/// counted before.
#[test]
fn cr_702_66a_black_lotus_is_delved_in_the_payment_it_helps() {
    let mut g = scen()
        .battlefield(0, &["Black Lotus", "Island"])
        .graveyard(0, &["Mountain", "Mountain", "Mountain"])
        .hand(0, &["Treasure Cruise"])
        .at(Step::Main1, 0)
        .start();
    let cruise = g.card(0, "Treasure Cruise", Zone::Hand);
    let lotus = bf(&g, 0, "Black Lotus");
    assert!(g.has_option(Choice::Cast(cruise)), "castable with the Lotus delved: {:?}", g.options_desc());
    g.choose(Choice::Cast(cruise));
    finish(&mut g, |g| options(g).contains(&Choice::Card(lotus)).then_some(Choice::Card(lotus)));
    assert_eq!(g.obj(cruise).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(lotus).zone, Zone::Exile, "the Lotus delved: {}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 0, "{}", log(&g));
    let g = scen()
        .battlefield(0, &["Black Lotus", "Island"])
        .graveyard(0, &["Mountain", "Mountain"])
        .hand(0, &["Treasure Cruise"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Treasure Cruise", Zone::Hand))), "seven");
}

/// docs/shortcuts.md 24.7.4, CR 111.2, 614.1a: a creation doubled past 64
/// makes every token there is room for. Seven Exalted Sunborns ("twice that
/// many of those tokens") and Eusocial Engineering's landfall Robot: 128
/// Robots, as many as the player's 96 permanents leave room for (the engine
/// limit of docs/shortcuts.md 26.11.6, with room for every card they own);
/// at most 64 were made before.
#[test]
fn cr_614_1a_doubled_tokens_past_sixty_four() {
    let mut mine = vec!["Exalted Sunborn"; 7];
    mine.push("Eusocial Engineering");
    let mut g = eoe_w2::scen().battlefield(0, &mine).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    settle(&mut g);
    let robots = g.permanents_of(0).filter(|&c| g.name(c) == "Robot").count();
    // (96 less the eight permanents, the Forest and the ten library cards.)
    assert_eq!(robots, 96 - 9 - 10, "{}", log(&g));
}

/// The payment's life, exact (F7's list, ENGINE.md 100.4.6), CR 119.4,
/// 107.3: Toxic Deluge's "As an additional cost to cast this spell, pay X
/// life" may take every point of a life total past 255 (the payment kept
/// life in a u8, so X stopped at 255).
#[test]
fn cr_119_4_pay_x_life_past_255() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).hand(0, &["Toxic Deluge"]).life(0, 400).at(Step::Main1, 0).start();
    let d = g.card(0, "Toxic Deluge", Zone::Hand);
    g.choose(Choice::Cast(d));
    // (X with A0's digits past 31: the largest each time.)
    for _ in 0..12 {
        if g.kind() != Some(DecisionKind::ChooseMode) {
            break;
        }
        let last = *options(&g).last().unwrap();
        g.choose(last);
    }
    finish(&mut g, |_| None);
    assert_eq!(g.life(0), 0, "X = 400: {}", log(&g));
}

// ---- 100.4.7: B4, the last of mana and payment ------------------------------

/// The cube, the shared test cards, Gene Pollinator and B4's: Pollinators of
/// other kinds of permanent (a noncreature artifact, a Forest, a Rebel), each
/// with Gene Pollinator's "{T}, Tap an untapped permanent you control: Add
/// one mana of any color".
fn b4_db() -> &'static CardDb {
    use serde_json::json;
    static DB: std::sync::OnceLock<&'static CardDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let pol = json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_permanent_you_control": true, "tap_self": true},
            "effect": {"$t": "AddManaToPool", "produces": [["any", 1]]}, "is_mana_ability": true});
        let tap_g = json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
            "effect": {"$t": "AddManaToPool", "produces": [["G", 1]]}, "is_mana_ability": true});
        let custom = vec![
            eoe_w2::card("Test Pollinator Rock", json!({"card_types": ["Artifact"], "type_line": "Artifact",
                "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}", "activated_abilities": [pol.clone()]})),
            eoe_w2::card("Test Pollinator Grove", json!({"card_types": ["Land"], "subtypes": ["Forest"], "type_line": "Land - Forest",
                "activated_abilities": [tap_g, pol.clone()]})),
            eoe_w2::card("Test Rebel Pollinator", json!({"card_types": ["Creature"], "subtypes": ["Rebel"], "power": 1, "toughness": 1,
                "type_line": "Creature - Rebel", "mana_cost": {"$t": "ManaCost", "colored": [["G", 1]]}, "mana_cost_str": "{G}",
                "activated_abilities": [pol]})),
            // A vanilla artifact costing {U}{U}{U}.
            eoe_w2::card("Test Blue Rock", json!({"card_types": ["Artifact"], "type_line": "Artifact",
                "mana_cost": {"$t": "ManaCost", "colored": [["U", 3]]}, "mana_cost_str": "{U}{U}{U}"})),
            // "Tap an untapped Rebel you control, Sacrifice a Rebel: Draw a card."
            eoe_w2::card("Test Rebel Altar", json!({"card_types": ["Artifact"], "type_line": "Artifact",
                "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}", "activated_abilities": [{"$t": "ActivatedAbility",
                    "cost": {"$t": "ActivationCost", "sacrifice_subtype": "Rebel", "extra_costs": [{"$t": "TapUntappedPermanentsCost",
                        "label": "an untapped Rebel you control", "permanent_filter": {"$t": "HasSubtype", "name": "Rebel"}}]},
                    "effect": {"$t": "DrawCards", "amount": 1}}]})),
        ];
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(common::custom_cards());
        cards.extend(engine::db::load_eoe_document().cards.into_iter().filter(|c| c.name == "Gene Pollinator"));
        cards.extend(custom.into_iter().map(|v| serde_json::from_value::<card_ir::Card>(v).expect("custom card IR")));
        let db = CardDb::from_cards(&cards);
        for n in ["Gene Pollinator", "Test Pollinator Rock", "Test Pollinator Grove", "Test Rebel Pollinator", "Test Rebel Altar", "Test Blue Rock"] {
            db.expect_id(n).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

fn b4_scen() -> Scenario {
    Scenario::new(b4_db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

/// docs/shortcuts.md N23, CR 106.12, 605.1b: Badgermole Cub's "whenever you
/// tap a creature for mana, add an additional {G}" adds {G} to the mana of a
/// Pollinator that is a creature (it is the permanent tapped for mana: its
/// ability has {T}), not to a noncreature one's. Gene Pollinator tapping an
/// Island makes two mana, the artifact Pollinator tapping the Cub one, the
/// other Island {U}: {G}{G}{G}{U}, never four {G}. (A loud failure before:
/// Pollinators of which only some are creatures.)
#[test]
fn cr_106_12_the_creature_mana_bonus_follows_the_pollinator_that_taps() {
    let g = b4_scen()
        .battlefield(0, &["Badgermole Cub", "Gene Pollinator", "Test Pollinator Rock", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    assert!(g.can_pay(0, &cost("{G}{G}{G}{U}")));
    assert!(g.can_pay(0, &cost("{G}{G}{U}{U}")), "the Islands for themselves, Gene Pollinator tapping the other Pollinator");
    assert!(!g.can_pay(0, &cost("{G}{G}{G}{G}")));
    assert!(!g.can_pay(0, &cost("{5}")));
}

/// docs/shortcuts.md N23, CR 106.12, 305.6: Nissa, Who Shakes the World's
/// "whenever you tap a Forest for mana, add an additional {G}" adds {G} to the
/// mana of a Pollinator that is a Forest (no Pollinator got a land-subtype
/// bonus). Such a Pollinator tapped for its own "{T}: Add {G}" can't also
/// activate its Pollinator ability (its other mana ability took no part of
/// the Pollinators' count: {R}{G}{G} was payable).
#[test]
fn cr_106_12_a_forest_pollinator_gets_the_forest_bonus_and_taps_once() {
    let g = b4_scen().battlefield(0, &["Nissa, Who Shakes the World", "Test Pollinator Grove", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}{G}")), "the Grove tapping the Island: one mana of any color and Nissa's {{G}}");
    assert!(g.can_pay(0, &cost("{G}{G}{U}")), "the Grove for its own {{G}} and Nissa's, the Island for {{U}}");
    assert!(!g.can_pay(0, &cost("{R}{G}{G}")), "the Grove taps once");
}

/// docs/shortcuts.md N23, CR 602.2b, 601.2g: which Gene Pollinator activates
/// for a permanent picked for a Pollinator's ability is the player's choice
/// (`ChoosePermanent`); the payment tapped the first ready one.
#[test]
fn cr_601_2g_which_pollinator_activates_is_the_players_choice() {
    let mut g = b4_scen().battlefield(0, &["Gene Pollinator", "Gene Pollinator", "Island"]).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let pols: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Gene Pollinator").collect();
    let island = bf(&g, 0, "Island");
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    // The Island by its own id: tapped for a Pollinator's ability.
    assert!(options(&g).contains(&Choice::Card(island)), "{:?}", g.options_desc());
    g.choose(Choice::Card(island));
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "which Pollinator: {:?}", g.options_desc());
    let o = options(&g);
    assert!(o.contains(&Choice::Card(pols[0])) && o.contains(&Choice::Card(pols[1])), "{o:?}");
    g.choose(Choice::Card(pols[1]));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(ring).zone, Zone::Stack, "{}", log(&g));
    assert!(g.obj(pols[1]).tapped && !g.obj(pols[0]).tapped && g.obj(island).tapped, "{}", log(&g));
}

/// docs/shortcuts.md N23, CR 602.2b, 601.2g: which Orcish Lumberjack
/// sacrifices the Forest (and is tapped) is the player's choice; the first
/// able one was.
#[test]
fn cr_601_2g_which_lumberjack_sacrifices_the_forest_is_the_players_choice() {
    let mut g = scen().battlefield(0, &["Orcish Lumberjack", "Orcish Lumberjack", "Forest"]).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let ljs: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Orcish Lumberjack").collect();
    let forest = bf(&g, 0, "Forest");
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    // The Lumberjacks' sacrifice of the Forest: after the Forest's own ability.
    let sac = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, s) if *x == forest && *s != AbilitySlot::Activated(0)));
    g.choose(sac.unwrap_or_else(|| panic!("the Lumberjacks' source on the Forest: {:?}", g.options_desc())));
    // (The production, then which Lumberjack.)
    for _ in 0..4 {
        if g.kind() == Some(DecisionKind::ChoosePermanent) {
            break;
        }
        let c = options(&g)[0];
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "which Lumberjack: {:?}", g.options_desc());
    let o = options(&g);
    assert!(o.contains(&Choice::Card(ljs[0])) && o.contains(&Choice::Card(ljs[1])), "{o:?}");
    g.choose(Choice::Card(ljs[1]));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(ring).zone, Zone::Stack, "{}", log(&g));
    assert!(g.obj(ljs[1]).tapped && !g.obj(ljs[0]).tapped && g.obj(forest).zone == Zone::Graveyard, "{}", log(&g));
}

/// docs/shortcuts.md N23, CR 601.2g-h: a Pollinator a later tap cost needs is
/// not the one that activates. Otharri, Suns' Glory's "{2}{R}{W}, Tap an
/// untapped Rebel you control" with a Pollinator that is the only Rebel and
/// Gene Pollinator: Gene Pollinator taps the Bear (the payment tapped the
/// first ready Pollinator, the Rebel, and the cost had no Rebel to tap).
#[test]
fn cr_601_2h_the_pollinator_a_tap_cost_needs_doesnt_activate() {
    let mut g = b4_scen()
        .battlefield(0, &["Test Rebel Pollinator", "Gene Pollinator", "Test Bear", "Mountain", "Plains", "Island"])
        .graveyard(0, &["Otharri, Suns' Glory"])
        .at(Step::Main1, 0)
        .start();
    let o = g.card(0, "Otharri, Suns' Glory", Zone::Graveyard);
    let rebel = bf(&g, 0, "Test Rebel Pollinator");
    let gene = bf(&g, 0, "Gene Pollinator");
    let back = options(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == o));
    g.choose(back.unwrap_or_else(|| panic!("Otharri's ability is offered: {:?}", g.options_desc())));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(o).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.obj(rebel).tapped && g.obj(gene).tapped && g.obj(bf(&g, 0, "Test Bear")).tapped, "{}", log(&g));
}

/// Can `c` (in P0's graveyard) be cast now (any route)?
fn castable_from_gy(g: &Game, c: CardIdx) -> bool {
    options(g).iter().any(|o| matches!(o, Choice::Cast(x) if *x == c))
}

/// docs/shortcuts.md 40.1, CR 702.81a, 118.9, 601.2b: Six's retrace ("a land
/// card discarded") with Grief's evoke ("exile a black card from your hand")
/// is one cost with two parts that take hand cards (`merge_costs` kept one
/// reservation: retrace wasn't offered with evoke). With a Swamp and Dark
/// Ritual in hand, Grief is cast from the graveyard for no mana: the Swamp
/// discarded, Dark Ritual exiled.
#[test]
fn cr_702_81a_retrace_with_a_pitch_evoke() {
    let mut g = scen().battlefield(0, &["Six"]).graveyard(0, &["Grief"]).hand(0, &["Swamp", "Dark Ritual"]).at(Step::Main1, 0).start();
    let grief = g.card(0, "Grief", Zone::Graveyard);
    assert!(castable_from_gy(&g, grief), "retrace with evoke: {:?}", g.options_desc());
    g.choose(Choice::Cast(grief));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => options(g).into_iter().find(|c| *c != Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.obj(grief).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.count_in(0, "Swamp", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Dark Ritual", Zone::Exile), 1, "{}", log(&g));
    // (One card in hand can't be both.)
    let g = scen().battlefield(0, &["Six"]).graveyard(0, &["Grief"]).hand(0, &["Swamp"]).at(Step::Main1, 0).start();
    assert!(!castable_from_gy(&g, g.card(0, "Grief", Zone::Graveyard)));
}

/// docs/shortcuts.md 24.7.5k, CR 702.81a, 601.2b, 601.2g-h: Wrenn and Six's
/// emblem's retrace ("a land card discarded") with Bitter Triumph's "discard a
/// card" is one cost of two discards (two discards with different filters
/// weren't one cost: not offered). P0 has 2 life (3 can't be paid). With a
/// Forest and Simian Spirit Guide in hand and a Swamp, the Spirit Guide's
/// mana is needed for {1}{B} and it can't be discarded too: not castable;
/// with an Island as well, castable (the Forest and the Island discarded).
#[test]
fn cr_702_81a_retrace_with_the_cards_own_discard() {
    let setup = |hand: &[&str]| {
        scen()
            .battlefield(0, &["Swamp"])
            .graveyard(0, &["Bitter Triumph"])
            .hand(0, hand)
            .battlefield(1, &["Test Bear"])
            .emblem(0, "Wrenn and Six")
            .life(0, 2)
            .at(Step::Main1, 0)
            .start()
    };
    let g = setup(&["Forest", "Simian Spirit Guide"]);
    assert!(!castable_from_gy(&g, g.card(0, "Bitter Triumph", Zone::Graveyard)), "{:?}", g.options_desc());
    let mut g = setup(&["Forest", "Simian Spirit Guide", "Island"]);
    let bt = g.card(0, "Bitter Triumph", Zone::Graveyard);
    let bear = bf(&g, 1, "Test Bear");
    assert!(castable_from_gy(&g, bt), "{:?}", g.options_desc());
    g.choose(Choice::Cast(bt));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => options(g).into_iter().find(|c| *c == Choice::Target(engine::Target::Obj(g.obj_ref(bear)))),
        _ => None,
    });
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard) + g.count_in(0, "Island", Zone::Graveyard), 2, "{}", log(&g));
    assert_eq!(g.life(0), 2);
}

/// docs/shortcuts.md 40.1, CR 702.81a, 118.9: Six's retrace with Daze's "return
/// an Island you control to its owner's hand" (an instant, by Wrenn and Six's
/// emblem): one cost, an Island returned and a land card discarded.
#[test]
fn cr_702_81a_retrace_with_daze_returning_an_island() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .graveyard(0, &["Daze"])
        .hand(0, &["Forest"])
        .emblem(0, "Wrenn and Six")
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.choose(Choice::Target(engine::Target::Player(0)));
    finish(&mut g, |_| None);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let daze = g.card(0, "Daze", Zone::Graveyard);
    assert!(castable_from_gy(&g, daze), "retrace with Daze's return: {:?}", g.options_desc());
}

/// CR 601.2h: a cost's parts are paid in any order, so one permanent may be
/// tapped for "tap an untapped Rebel you control" and then sacrificed for
/// "sacrifice a Rebel" (the feasibility let it, `Keep::taps`; the payment's
/// tap choice left out a permanent picked to be sacrificed: no Rebel to tap).
#[test]
fn cr_601_2h_one_permanent_tapped_and_sacrificed_for_one_cost() {
    let mut g = b4_scen().battlefield(0, &["Test Rebel Altar", "Test Rebel Pollinator"]).at(Step::Main1, 0).start();
    let altar = bf(&g, 0, "Test Rebel Altar");
    let rebel = bf(&g, 0, "Test Rebel Pollinator");
    assert!(g.has_option(act(altar, 0)), "{:?}", g.options_desc());
    g.choose(act(altar, 0));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(rebel).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), 1, "drew a card
{}", log(&g));
}

/// docs/shortcuts.md 13.15, CR 605.3a, 602.2b: the mana of a mana ability with
/// a mana cost may pay another such ability's cost when that one is activated
/// after it (`MADE` mana paid no ability's cost). Two Sunken Ruins ("{U/B},
/// {T}: Add {U}{U}, {U}{B} or {B}{B}") and a Swamp: the Swamp for one, one of
/// its mana for the other: {U}{U}{U} (a Ruins' own "{T}: Add {C}" made the
/// third mana before); two alone make no {U} (neither can be first).
#[test]
fn cr_605_3a_a_mana_abilitys_mana_pays_a_later_ones_cost() {
    let g = scen().battlefield(0, &["Sunken Ruins", "Sunken Ruins", "Swamp"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}{U}{U}")), "the Swamp for one Ruins, its mana for the other");
    assert!(g.can_pay(0, &cost("{3}")));
    assert!(!g.can_pay(0, &cost("{4}")));
    let g = scen().battlefield(0, &["Sunken Ruins", "Sunken Ruins"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{2}")), "their own {{C}}");
    assert!(!g.can_pay(0, &cost("{U}")), "neither Ruins can be first");
}

/// The same in a payment (CR 601.2g): B4's Test Blue Rock ({U}{U}{U}) cast
/// with two Sunken Ruins and a Swamp (not castable before: a Ruins' own "{T}:
/// Add {C}" made the third mana): both Ruins activated.
#[test]
fn cr_601_2g_a_payment_with_a_chain_of_mana_abilities() {
    let mut g = b4_scen().battlefield(0, &["Sunken Ruins", "Sunken Ruins", "Swamp"]).hand(0, &["Test Blue Rock"]).at(Step::Main1, 0).start();
    let relic = g.card(0, "Test Blue Rock", Zone::Hand);
    assert!(options(&g).contains(&Choice::Cast(relic)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(relic));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(relic).zone, Zone::Stack, "{}", log(&g));
    assert!(g.permanents_of(0).all(|c| g.obj(c).tapped), "{}", log(&g));
    assert_eq!(g.player(0).pool.amt.iter().map(|&n| n as u32).sum::<u32>(), 0, "{}", log(&g));
}
