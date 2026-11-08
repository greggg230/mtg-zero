//! Edge of Eternities W2 (ENGINE.md 99.12): the lands and the other cards
//! (the spending restrictions are in `cards_eoe_w2.rs`).

mod eoe_w2;

use eoe_w2::*;
use engine::db::{Counter, Kw};
use engine::{Choice, DecisionKind, Step, Zone};

/// Declare `c` (the active player's) an attacker of the opponent and finish
/// the declaration.
fn attack(g: &mut engine::Game, c: engine::CardIdx) {
    run_until(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers), |_| None);
    g.choose(Choice::Card(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
}

fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").expect("charge counters"))
}

/// The Eternity Elevator: "{T}: Add {C}{C}{C}." and, with 20 or more charge
/// counters (station, CR 702.184, 722.2a), "{T}: Add X mana of any one color,
/// where X is the number of charge counters on The Eternity Elevator" (CR
/// 106.1a).
#[test]
fn cr_106_1a_the_eternity_elevator_adds_x_mana_of_one_color_at_20() {
    let g = scen().battlefield(0, &["The Eternity Elevator"]).counters(0, "The Eternity Elevator", charge(), 19).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{3}")) && !g.can_pay(0, &cost("{4}")), "19 counters: {{C}}{{C}}{{C}} only");
    assert!(!g.can_pay(0, &cost("{G}")));
    let g = scen().battlefield(0, &["The Eternity Elevator"]).counters(0, "The Eternity Elevator", charge(), 21).at(Step::Main1, 0).start();
    let greens = "{G}".repeat(21);
    assert!(g.can_pay(0, &cost(&greens)), "21 green mana");
    assert!(!g.can_pay(0, &cost(&format!("{greens}{{G}}"))));
    assert!(!g.can_pay(0, &cost("{G}{U}")), "of one color");
    assert!(g.can_pay(0, &cost("{U}{U}{U}{U}")));
}

/// Cascading Cataracts: indestructible; "{5}, {T}: Add five mana in any
/// combination of colors" (a mana ability with a mana cost, paid within the
/// payment it helps: CR 605.3a, 602.2b; its own mana never pays it).
#[test]
fn cr_605_3a_cascading_cataracts_turns_five_mana_into_five_of_any_colors() {
    let g = scen().battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let cat = bf(&g, 0, "Cascading Cataracts");
    assert!(g.keywords_of(cat).has(Kw::Indestructible));
    assert!(g.can_pay(0, &cost("{W}{U}{B}{R}{G}")), "the Islands pay {{5}}, the Cataracts make the colours");
    assert!(g.can_pay(0, &cost("{R}{R}{G}{G}{W}")));
    assert!(!g.can_pay(0, &cost("{R}{R}{G}{G}{W}{U}")), "five, not six");
    let g = scen().battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{R}")), "four Islands can't pay {{5}}");
    assert!(g.can_pay(0, &cost("{5}")), "{{C}} and the Islands");
}

/// The Cataracts in a payment: picked, its {5} is paid by the payment's other
/// sources; a spell of {2}{R} (CR 601.2g-h, 605.3a).
#[test]
fn cr_601_2g_cascading_cataracts_pays_within_a_payment() {
    let mut g = scen()
        .battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Test Double"])
        .at(Step::Main1, 0)
        .start();
    let cat = bf(&g, 0, "Cascading Cataracts");
    let spell = g.card(0, "Test Double", Zone::Hand);
    assert!(g.has_option(Choice::Cast(spell)));
    g.choose(Choice::Cast(spell));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    assert!(g.has_option(act(cat, 1)), "{:?}", g.options_desc());
    g.choose(act(cat, 1));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(spell).zone, Zone::Battlefield);
    assert!(g.obj(cat).tapped);
    // (The Islands pay its {5}; its five mana pay {2}{R}, two stay in the
    // pool: the payment spends the virtual pool's mana first, ENGINE.md 18.)
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 5, "its {{5}}");
    assert_eq!(g.player(0).pool.total(), 2);
}

/// Crystal Quarry: "{5}, {T}: Add {W}{U}{B}{R}{G}" (CR 605.3a).
#[test]
fn cr_605_3a_crystal_quarry_makes_one_of_each_color() {
    let g = scen().battlefield(0, &["Crystal Quarry", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{U}{B}{R}{G}")));
    assert!(!g.can_pay(0, &cost("{R}{R}")), "one of each");
    let g = scen().battlefield(0, &["Crystal Quarry", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")));
}

/// Blast Zone: "This land enters with a charge counter on it." (CR 122.6)
/// and "{X}{X}, {T}: Put X charge counters on this land." (CR 107.3: X paid
/// twice).
#[test]
fn cr_107_3_blast_zone_enters_with_a_counter_and_takes_x_for_x_x() {
    let mut g = scen().battlefield(0, &["Island", "Island", "Island", "Island", "Island"]).hand(0, &["Blast Zone"]).at(Step::Main1, 0).start();
    let bz = g.card(0, "Blast Zone", Zone::Hand);
    g.choose(Choice::Play(bz));
    settle(&mut g);
    assert_eq!(g.counters_on(bz, charge()), 1);
    // ({T}: the land just played can be tapped: lands have no summoning
    // sickness, CR 302.6.) Five Islands: X is at most 2.
    g.choose(act(bz, 1));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "X: {:?}", g.options_desc());
    assert_eq!(opts(&g).len(), 3, "X = 0, 1 or 2");
    let x2 = opts(&g)[2];
    g.choose(x2);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bz, charge()), 3);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 4, "{{X}}{{X}} with X = 2");
}

/// "{3}, {T}, Sacrifice this land: Destroy each nonland permanent with mana
/// value equal to the number of charge counters on this land" (sacrificed as
/// the cost: its counters as it last existed, CR 608.2h; 701.8).
#[test]
fn cr_608_2h_blast_zone_destroys_by_its_last_known_counters() {
    let mut g = scen()
        .battlefield(0, &["Blast Zone", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Flyer", "Test Double", "Forest"])
        .counters(0, "Blast Zone", charge(), 2)
        .at(Step::Main1, 0)
        .start();
    let bz = bf(&g, 0, "Blast Zone");
    g.choose(act(bz, 2));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(bz).zone, Zone::Graveyard);
    assert!(g.find_card(0, "Test Bear", Zone::Graveyard).is_some(), "the Bear ({{1}}{{G}})");
    assert!(g.find_card(1, "Test Flyer", Zone::Graveyard).is_some(), "the Flyer ({{1}}{{U}})");
    assert!(g.find_card(1, "Test Double", Zone::Battlefield).is_some(), "mana value 3 stays");
    assert!(g.find_card(1, "Forest", Zone::Battlefield).is_some(), "lands stay");
}

/// Contested War Zone (its trigger as the Oracle text says it,
/// docs/ir_defects.md 161): "Whenever a creature deals combat damage to you,
/// that creature's controller gains control of this land" (CR 510.1, 603.2,
/// 613.1b).
#[test]
fn cr_613_1b_contested_war_zone_goes_to_the_attacker_that_damaged_you() {
    let mut g = scen().battlefield(0, &["Test War Zone"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g), |_| None);
    assert_eq!(g.player(0).life, 18);
    assert_eq!(g.obj(zone).controller, 1, "the Bear's controller gains control of it");
}

/// Combat damage to a creature you control is not combat damage to you: the
/// War Zone stays (CR 510.1c).
#[test]
fn cr_510_1c_contested_war_zone_ignores_damage_to_your_creatures() {
    let mut g = scen().battlefield(0, &["Test War Zone", "Test Reacher"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(
        &mut g,
        |g| g.turn().step == Step::Main2 && at_priority(g),
        |g| if g.kind() == Some(DecisionKind::DeclareBlockers) { pick(g, &[Choice::Card(bear)]) } else { None },
    );
    assert_eq!(g.player(0).life, 20, "blocked");
    assert_eq!(g.obj(zone).controller, 0);
}

/// "{1}, {T}: Attacking creatures get +1/+0 until end of turn": every
/// attacking creature, whoever controls it (CR 611.2c), activated by the
/// defending player too.
#[test]
fn cr_611_2c_contested_war_zone_pumps_every_attacking_creature() {
    let mut g = scen().battlefield(0, &["Test War Zone", "Island"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0 && g.turn().step == Step::DeclareAttackers, |_| None);
    assert!(g.has_option(act(zone, 1)), "{:?}", g.options_desc());
    start_with(&mut g, act(zone, 1), |_| None);
    settle(&mut g);
    assert_eq!(g.power(bear), 3, "the opponent's attacker gets +1/+0");
}

/// Plaza of Heroes (its third ability as the Oracle text says it,
/// docs/ir_defects.md 160): "{T}: Add one mana of any color. Spend this mana
/// only to cast a legendary spell." (CR 106.6) and "{T}: Add one mana of any
/// color among legendary permanents you control." (CR 106.1, 105.4).
#[test]
fn cr_106_6_plaza_of_heroes_mana_for_legends_and_legends_colors() {
    let g = scen().battlefield(0, &["Test Plaza"]).hand(0, &["Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")), "no legendary permanent: no colour but for a legendary spell");
    assert!(g.can_pay(0, &cost("{1}")));
    let g = scen().battlefield(0, &["Test Plaza", "Plains"]).hand(0, &["Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    let legend = g.card(0, "Test Legend", Zone::Hand);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(legend)), "{{1}}{{W}}: the Plaza's restricted mana and the Plains");
    assert!(!g.has_option(Choice::Cast(bear)), "{{1}}{{G}}: no green");
    let g = scen().battlefield(0, &["Test Plaza", "Test Legend"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}")), "white among legendary permanents");
    assert!(!g.can_pay(0, &cost("{U}")));
}

/// "{3}, {T}, Exile this land: Target legendary creature gains hexproof and
/// indestructible until end of turn" (CR 702.11, 702.12).
#[test]
fn cr_702_12_plaza_of_heroes_protects_a_legendary_creature() {
    let mut g = scen().battlefield(0, &["Test Plaza", "Island", "Island", "Island", "Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    let plaza = bf(&g, 0, "Test Plaza");
    let legend = bf(&g, 0, "Test Legend");
    g.choose(act(plaza, 3));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(legend)) && !g.has_option(g.tgt(bf(&g, 0, "Test Bear"))), "legendary creatures only");
    let t = g.tgt(legend);
    g.choose(t);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(plaza).zone, Zone::Exile);
    assert!(g.keywords_of(legend).has(Kw::Hexproof) && g.keywords_of(legend).has(Kw::Indestructible));
}

/// Magus of the Moon: "Nonbasic lands are Mountains." (CR 305.7, 613.1d:
/// a nonbasic land loses its land types and the abilities of its rules
/// text, and has "{T}: Add {R}"; basic lands are unaffected.)
#[test]
fn cr_305_7_magus_of_the_moon_makes_nonbasic_lands_mountains() {
    let g = scen().battlefield(0, &["Breeding Pool", "Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let pool = bf(&g, 0, "Breeding Pool");
    let cat = bf(&g, 0, "Cascading Cataracts");
    assert!(g.can_pay(0, &cost("{G}{W}{B}")), "without the Magus");
    assert!(subtype(&g, pool, "Forest") && !subtype(&g, pool, "Mountain"));
    let g = scen()
        .battlefield(0, &["Breeding Pool", "Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Magus of the Moon"])
        .at(Step::Main1, 0)
        .start();
    assert!(subtype(&g, pool, "Mountain") && !subtype(&g, pool, "Forest") && !subtype(&g, pool, "Island"), "its land types become Mountain");
    assert!(g.is_type(pool, engine::db::types::LAND));
    assert!(!g.keywords_of(cat).has(Kw::Indestructible), "the Cataracts lose indestructible");
    assert!(g.can_pay(0, &cost("{R}{R}{U}{U}{U}{U}{U}")), "{{R}} from each nonbasic land; the Islands stay Islands");
    assert!(!g.can_pay(0, &cost("{G}")), "no {{G}}: the Pool's and the Cataracts' abilities are gone");
}

/// A nonbasic land entering while it would be a Mountain has none of its
/// own replacement or triggered abilities (CR 614.12: its characteristics as
/// it would exist on the battlefield): Lotus Field enters untapped and its
/// "sacrifice two lands" doesn't trigger; Breeding Pool asks for no life.
#[test]
fn cr_614_12_lands_enter_as_mountains_without_their_abilities() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Magus of the Moon"])
        .hand(0, &["Lotus Field", "Breeding Pool"])
        .at(Step::Main1, 0)
        .start();
    let field = g.card(0, "Lotus Field", Zone::Hand);
    g.choose(Choice::Play(field));
    settle(&mut g);
    assert_eq!(g.obj(field).zone, Zone::Battlefield);
    assert!(!g.obj(field).tapped, "no 'enters tapped'");
    assert_eq!(g.lands_of(0).count(), 3, "no lands sacrificed");
    assert!(!g.keywords_of(field).has(Kw::Hexproof));
    let mut g = scen().battlefield(1, &["Magus of the Moon"]).hand(0, &["Breeding Pool"]).at(Step::Main1, 0).start();
    let pool = g.card(0, "Breeding Pool", Zone::Hand);
    g.choose(Choice::Play(pool));
    assert!(at_priority(&g), "no 'you may pay 2 life': {:?}", g.kind());
    assert!(!g.obj(pool).tapped && g.player(0).life == 20);
}

/// The Magus gone, the lands are themselves again (CR 611.3a, 613.1d).
#[test]
fn cr_611_3a_without_the_magus_the_lands_are_themselves() {
    let mut g = scen()
        .battlefield(0, &["Breeding Pool", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Magus of the Moon"])
        .hand(0, &["Bombard"])
        .at(Step::Main1, 0)
        .start();
    let pool = bf(&g, 0, "Breeding Pool");
    assert!(!g.can_pay(0, &cost("{G}")));
    let magus = bf(&g, 1, "Magus of the Moon");
    let bombard = g.card(0, "Bombard", Zone::Hand);
    start_with(&mut g, Choice::Cast(bombard), |g| pick(g, &[g.tgt(magus)]));
    settle(&mut g);
    assert_eq!(g.obj(magus).zone, Zone::Graveyard);
    assert!(subtype(&g, pool, "Forest"));
    g.set_tapped(pool, false);
    assert!(g.can_pay(0, &cost("{G}")), "the Pool makes {{G}} again");
}

/// Bioengineered Future: "When this enchantment enters, create a Lander
/// token." and "Each creature you control enters with an additional +1/+1
/// counter on it for each land that entered the battlefield under your
/// control this turn" (CR 614.1c, 122.6; lands before the creature only).
#[test]
fn cr_614_1c_bioengineered_future_counts_the_lands_that_entered_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Bioengineered Future", "Forest", "Forest", "Forest"])
        .hand(0, &["Test Bear", "Forest", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bears: Vec<_> = g.player(0).hand.iter().copied().filter(|&c| g.name(c) == "Test Bear").collect();
    // A creature before any land entered: no counter.
    start_with(&mut g, Choice::Cast(bears[0]), |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bears[0], Counter::P1P1), 0);
    let forest = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(forest));
    settle(&mut g);
    start_with(&mut g, Choice::Cast(bears[1]), |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bears[1], Counter::P1P1), 1, "one land entered this turn");
    assert_eq!(g.counters_on(bears[0], Counter::P1P1), 0, "the earlier creature keeps none");
    assert_eq!(g.power(bears[1]), 3);
}

/// The count is per turn and per player: the opponent's lands don't count,
/// and it starts again each turn (the Future's Lander as it enters).
#[test]
fn cr_614_1c_bioengineered_future_counts_only_your_lands_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Forest", "Forest", "Forest"])
        .hand(0, &["Bioengineered Future", "Test Bear"])
        .hand(1, &["Forest"])
        .at(Step::Main1, 1)
        .start();
    let f1 = g.card(1, "Forest", Zone::Hand);
    g.choose(Choice::Play(f1));
    quiet_until(&mut g, main1_of(0));
    let future = g.card(0, "Bioengineered Future", Zone::Hand);
    start_with(&mut g, Choice::Cast(future), |_| None);
    settle(&mut g);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Lander").count(), 1, "its Lander");
    let bear = g.card(0, "Test Bear", Zone::Hand);
    start_with(&mut g, Choice::Cast(bear), |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 0, "no land entered under P0's control this turn");
}

/// Gene Pollinator: "{T}, Tap an untapped permanent you control: Add one mana
/// of any color" (CR 602.2b, 605.3a: both tapped as the cost; another
/// permanent, not itself).
#[test]
fn cr_605_3a_gene_pollinator_taps_another_permanent_for_any_color() {
    let g = scen().battlefield(0, &["Gene Pollinator"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{G}")), "nothing else to tap");
    let g = scen().battlefield(0, &["Gene Pollinator", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}")), "the Pollinator and the Island: one mana of any color");
    assert!(!g.can_pay(0, &cost("{R}{U}")), "the Island is tapped for it");
    let g = scen().battlefield(0, &["Gene Pollinator", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}{U}")));
    assert!(!g.can_pay(0, &cost("{R}{G}")), "one Pollinator, one activation");
    // Two Pollinators alone: one taps the other (one mana, not two).
    let g = scen().battlefield(0, &["Gene Pollinator", "Gene Pollinator"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}")) && !g.can_pay(0, &cost("{W}{W}")));
    let g = scen().battlefield(0, &["Gene Pollinator", "Gene Pollinator", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{B}")), "each taps an Island");
}

/// In a payment: the permanent tapped is picked (an `ACTIVATE` id after its
/// own abilities); a Pollinator cast this turn can't activate (summoning
/// sickness, CR 302.6) but can be tapped for another's ability.
#[test]
fn cr_302_6_gene_pollinator_in_a_payment_and_summoning_sickness() {
    let mut g = scen().battlefield(0, &["Gene Pollinator", "Island"]).hand(0, &["Gene Pollinator"]).at(Step::Main1, 0).start();
    let pol = bf(&g, 0, "Gene Pollinator");
    let island = bf(&g, 0, "Island");
    let new = g.card(0, "Gene Pollinator", Zone::Hand);
    g.choose(Choice::Cast(new));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{:?}", g.options_desc());
    let pick = opts(&g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == island) && *c != act(island, 0)).expect("the Island as the Pollinator's tapped permanent");
    g.choose(pick);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(new).zone, Zone::Battlefield);
    assert!(g.obj(pol).tapped && g.obj(island).tapped);
    // The new one is summoning sick: it can't activate (nothing else is
    // untapped but it, and it can't tap itself).
    assert!(!g.can_pay(0, &cost("{G}")));
}

/// Famished Worldsire: devour land 3 (CR 702.82a: as it enters, sacrifice
/// any number of lands; three +1/+1 counters for each) and "When this
/// creature enters, look at the top X cards of your library, where X is
/// this creature's power. Put any number of land cards from among them onto
/// the battlefield tapped, then shuffle."
#[test]
fn cr_702_82a_famished_worldsire_devours_lands_and_puts_lands_back() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Famished Worldsire"])
        .library(0, &["Test Bear", "Island", "Island", "Test Bear", "Island", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let ws = g.card(0, "Famished Worldsire", Zone::Hand);
    g.choose(Choice::Cast(ws));
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::Sacrifice), |_| None);
    // Two Forests (each a pick, then stop).
    let forests: Vec<_> = opts(&g).into_iter().filter(|c| matches!(c, Choice::Card(_))).collect();
    assert_eq!(forests.len(), 8, "any of the lands");
    g.choose(forests[0]);
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let next = opts(&g).into_iter().find(|c| matches!(c, Choice::Card(_))).unwrap();
    g.choose(next);
    g.choose(Choice::Pass);
    assert_eq!(g.counters_on(ws, Counter::P1P1), 6, "devour land 3, two lands");
    assert_eq!(g.lands_of(0).count(), 6);
    // Its trigger: the top six (power 6); take every land.
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::PutInHand) || (at_priority(g) && g.stack().is_empty()), |_| None);
    let mut took = 0;
    while g.kind() == Some(DecisionKind::PutInHand) {
        let c = opts(&g).into_iter().find(|c| matches!(c, Choice::Card(_)));
        match c {
            Some(c) => {
                g.choose(c);
                took += 1;
            }
            None => {
                g.choose(Choice::Pass);
            }
        }
    }
    settle(&mut g);
    assert_eq!(took, 4, "the four Islands among the top six");
    assert_eq!(g.lands_of(0).count(), 10);
    assert_eq!(g.lands_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 4, "tapped");
    assert_eq!(g.power(ws), 6);
}

/// Echoing Deeps: "You may have this land enter tapped as a copy of any land
/// card in a graveyard, except it's a Cave in addition to its other types"
/// (CR 707.5, 707.9b; either graveyard; declined, it enters untapped as
/// itself).
#[test]
fn cr_707_5_echoing_deeps_enters_tapped_as_a_copy_of_a_land_card_in_a_graveyard() {
    let mut g = scen().hand(0, &["Echoing Deeps"]).graveyard(1, &["Mana Confluence"]).at(Step::Main1, 0).start();
    let deeps = g.card(0, "Echoing Deeps", Zone::Hand);
    let conf = g.card(1, "Mana Confluence", Zone::Graveyard);
    g.choose(Choice::Play(deeps));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{:?}", g.options_desc());
    assert!(g.has_option(Choice::Card(conf)) && g.has_option(Choice::Pass));
    g.choose(Choice::Card(conf));
    settle(&mut g);
    assert_eq!(g.name(deeps), "Mana Confluence");
    assert!(subtype(&g, deeps, "Cave"), "a Cave in addition");
    assert!(g.obj(deeps).tapped, "it enters tapped");
    g.set_tapped(deeps, false);
    assert!(g.can_pay(0, &cost("{B}")), "the copied ability");
    // Declined: itself, untapped.
    let mut g = scen().hand(0, &["Echoing Deeps"]).graveyard(1, &["Mana Confluence"]).at(Step::Main1, 0).start();
    let deeps = g.card(0, "Echoing Deeps", Zone::Hand);
    g.choose(Choice::Play(deeps));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.name(deeps), "Echoing Deeps");
    assert!(!g.obj(deeps).tapped && g.can_pay(0, &cost("{1}")) && !g.can_pay(0, &cost("{B}")));
}

/// A Pollinator tapped for convoke can't activate (CR 702.51a, 602.2b):
/// castability counts it once, as the payment does (ENGINE.md 49, 99.12;
/// EoE fuzz seed 6224). Vote Out ({3}{B}, convoke) with Eldrazi Temple's
/// {C}, the Pollinator's mana (tapping the Microsizer) and a black creature
/// convoking: three, not four.
#[test]
fn cr_702_51a_a_pollinator_convoking_does_not_also_activate() {
    let g = scen()
        .battlefield(0, &["Eldrazi Temple", "Gene Pollinator", "Test Deathtouch", "Atomic Microsizer"])
        .hand(0, &["Vote Out"])
        .at(Step::Main1, 0)
        .start();
    let vote = g.card(0, "Vote Out", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(vote)), "three mana and convokes, not four");
    let g = scen()
        .battlefield(0, &["Eldrazi Temple", "Gene Pollinator", "Test Deathtouch", "Atomic Microsizer", "Island"])
        .hand(0, &["Vote Out"])
        .at(Step::Main1, 0)
        .start();
    let vote = g.card(0, "Vote Out", Zone::Hand);
    assert!(g.has_option(Choice::Cast(vote)), "with an Island, four");
}
