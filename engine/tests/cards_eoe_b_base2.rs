//! Card tests of the Edge of Eternities base cards new to mtg-zero, part 2
//! (ENGINE.md 99.3): spells, activated abilities, Equipment and exile
//! effects, the Stellar Sights manlands and Dust Bowl, each against its
//! Oracle text.

mod eoe_b;

use engine::db::{types, Counter, Kw};
use engine::mana::ManaCost;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};
use eoe_b::*;

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn tokens_named(g: &Game, p: u8, name: &str) -> Vec<engine::CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.def_of(c).token).collect()
}

// ---- spells ------------------------------------------------------------------

/// Annul: "Counter target artifact or enchantment spell."
#[test]
fn annul_counters_only_artifact_and_enchantment_spells() {
    let rows = [("Nutrient Block", true), ("Seam Rip", true), ("Hullcarver", true), ("Gravpack Monoist", false), ("Honor", false)];
    for (spell, ok) in rows {
        let mut g = scen().battlefield(0, &["Plains", "Swamp", "Island", "Test Bear"]).hand(0, &[spell]).battlefield(1, &["Island", "Test Bear"]).hand(1, &["Annul"]).at(Step::Main1, 0).start();
        let c = g.card(0, spell, Zone::Hand);
        start_with(&mut g, Choice::Cast(c), |g| pick(g, &[g.tgt(bf(g, 1, "Test Bear"))]));
        let annul = g.card(1, "Annul", Zone::Hand);
        g.pass();
        assert_eq!(g.has_option(Choice::Cast(annul)), ok, "{spell}");
    }
}

/// Bombard: "Bombard deals 4 damage to target creature." Rig for War:
/// "Target creature gets +3/+0 and gains first strike and reach until end of
/// turn." Honor: "Put a +1/+1 counter on target creature. / Draw a card."
#[test]
fn bombard_rig_for_war_and_honor() {
    let mut g = scen().battlefield(0, &["Mountain"; 5]).battlefield(1, &["Test Trampler", "Test Bear"]).hand(0, &["Bombard", "Rig for War"]).at(Step::Main1, 0).start();
    let (t, b) = (bf(&g, 1, "Test Trampler"), bf(&g, 1, "Test Bear"));
    let tt = g.tgt(t);
    cast_and_resolve(&mut g, 0, "Bombard", move |g| pick(g, &[tt]));
    assert_eq!(g.obj(t).zone, Zone::Graveyard, "4 damage to a 4/4");
    let tb = g.tgt(b);
    cast_and_resolve(&mut g, 0, "Rig for War", move |g| pick(g, &[tb]));
    assert_eq!(g.pt(b), (5, 2));
    assert!(g.has_kw(b, Kw::FirstStrike) && g.has_kw(b, Kw::Reach));

    let mut g = scen().battlefield(0, &["Plains", "Hullcarver"]).hand(0, &["Honor"]).at(Step::Main1, 0).start();
    let h = bf(&g, 0, "Hullcarver");
    let hand = g.player(0).hand.len();
    let th = g.tgt(h);
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[th]));
    assert_eq!(g.counters_on(h, Counter::P1P1), 1);
    assert_eq!(g.player(0).hand.len(), hand, "Honor left, a card drawn");
}

// ---- exile until it leaves ---------------------------------------------------

/// Banishing Light / Seam Rip: "When this enchantment enters, exile target
/// nonland permanent an opponent controls [with mana value 2 or less] until
/// this enchantment leaves the battlefield" (CR 610.3).
#[test]
fn banishing_light_and_seam_rip_exile_until_they_leave() {
    let mut g = scen().battlefield(0, &["Plains"; 4]).battlefield(1, &["Test Trampler", "Hullcarver", "Forest"]).hand(0, &["Banishing Light", "Seam Rip"]).at(Step::Main1, 0).start();
    let (tr, hc) = (bf(&g, 1, "Test Trampler"), bf(&g, 1, "Hullcarver"));
    cast_and_resolve(&mut g, 0, "Seam Rip", |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(bf(g, 1, "Test Trampler"))), "mana value 2 or less");
            assert!(!g.has_option(g.tgt(bf(g, 1, "Forest"))), "nonland");
        }
        pick(g, &[g.tgt(bf(g, 1, "Hullcarver"))])
    });
    assert_eq!(g.obj(hc).zone, Zone::Exile);
    let ttr = g.tgt(tr);
    cast_and_resolve(&mut g, 0, "Banishing Light", move |g| pick(g, &[ttr]));
    assert_eq!(g.obj(tr).zone, Zone::Exile);
}

/// Seam Rip leaving returns the card (CR 610.3a).
#[test]
fn cr_610_3a_seam_rip_leaving_returns_the_card() {
    let mut g = scen().battlefield(0, &["Plains"]).battlefield(1, &["Hullcarver", "Plains", "Plains", "Plains"]).hand(0, &["Seam Rip"]).hand(1, &["Emergency Eject"]).at(Step::Main1, 0).start();
    let hc = bf(&g, 1, "Hullcarver");
    let th = g.tgt(hc);
    cast_and_resolve(&mut g, 0, "Seam Rip", move |g| pick(g, &[th]));
    assert_eq!(g.obj(hc).zone, Zone::Exile);
    let rip = bf(&g, 0, "Seam Rip");
    let tr = g.tgt(rip);
    quiet_until(&mut g, main1_of(1));
    cast_and_resolve(&mut g, 1, "Emergency Eject", move |g| pick(g, &[tr]));
    assert_eq!(g.obj(rip).zone, Zone::Graveyard);
    assert!(g.find_card(1, "Hullcarver", Zone::Battlefield).is_some(), "the exiled card returns");
}

// ---- Equipment ---------------------------------------------------------------

/// Auxiliary Boosters: "When this Equipment enters, create a 2/2 colorless
/// Robot artifact creature token and attach this Equipment to it. / Equipped
/// creature gets +1/+2 and has flying. / Equip {3}."
#[test]
fn auxiliary_boosters_makes_a_robot_and_equips_it() {
    let mut g = scen().battlefield(0, &["Plains"; 5]).hand(0, &["Auxiliary Boosters"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Auxiliary Boosters", |_| None);
    let r = tokens_named(&g, 0, "Robot");
    assert_eq!(r.len(), 1);
    assert_eq!(g.pt(r[0]), (3, 4));
    assert!(g.has_kw(r[0], Kw::Flying));
    let eq = bf(&g, 0, "Auxiliary Boosters");
    assert!(subtype(&g, eq, "Equipment"));
    let d = g.def_of(eq);
    assert!(d.activated.iter().any(|a| a.cost.mana == cost("{3}") && a.sorcery), "equip {{3}}, as a sorcery");
}

// ---- Food --------------------------------------------------------------------

/// Dubious Delicacy: "Flash / When this artifact enters, up to one target
/// creature gets -3/-3 until end of turn. / {2}, {T}, Sacrifice this
/// artifact: You gain 3 life. / {2}, {T}, Sacrifice this artifact: Target
/// opponent loses 3 life."
#[test]
fn dubious_delicacy_shrinks_then_gains_or_drains() {
    let mut g = scen().battlefield(0, &["Swamp"; 5]).battlefield(1, &["Test Trampler"]).hand(0, &["Dubious Delicacy"]).at(Step::Main1, 0).start();
    let t = bf(&g, 1, "Test Trampler");
    let tt = g.tgt(t);
    cast_and_resolve(&mut g, 0, "Dubious Delicacy", move |g| pick(g, &[tt]));
    assert_eq!(g.pt(t), (1, 1));
    let dd = bf(&g, 0, "Dubious Delicacy");
    start_with(&mut g, act(dd, 1), |g| pick(g, &[Choice::Target(Target::Player(1))]));
    settle(&mut g);
    assert_eq!(g.life(1), 17);
    // Up to one: no target at all.
    let mut g = scen().battlefield(0, &["Swamp"; 3]).hand(0, &["Dubious Delicacy"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Dubious Delicacy", |g| pick(g, &[Choice::Pass]));
    let dd = bf(&g, 0, "Dubious Delicacy");
    assert!(g.def_of(dd).activated.iter().any(|a| a.cost.sac_self && a.cost.tap && a.cost.mana == cost("{2}")));
}

// ---- activated abilities -----------------------------------------------------

/// Dual-Sun Adepts: "{5}: Creatures you control get +1/+1 until end of
/// turn." Intrepid Tenderfoot: "{3}: Put a +1/+1 counter on this creature.
/// Activate only as a sorcery."
#[test]
fn dual_sun_adepts_and_intrepid_tenderfoot_activations() {
    let mut g = scen().battlefield(0, &["Dual-Sun Adepts", "Hullcarver"]).battlefield(0, &["Plains"; 5]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let (a, h, b) = (bf(&g, 0, "Dual-Sun Adepts"), bf(&g, 0, "Hullcarver"), bf(&g, 1, "Test Bear"));
    start_with(&mut g, act(a, 0), |_| None);
    settle(&mut g);
    assert_eq!((g.pt(a), g.pt(h), g.pt(b)), ((3, 3), (2, 2), (2, 2)));
    let mut g = scen().battlefield(0, &["Intrepid Tenderfoot"]).battlefield(0, &["Forest"; 3]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, "Intrepid Tenderfoot");
    start_with(&mut g, act(t, 0), |_| None);
    assert!(!g.has_option(act(t, 0)), "a sorcery: not with the ability on the stack");
    settle(&mut g);
    assert_eq!(g.counters_on(t, Counter::P1P1), 1);
}

/// Illvoi Galeblade: "{2}, Sacrifice this creature: Draw a card." Kavaron
/// Turbodrone: "{T}: Target creature you control gets +1/+1 and gains haste
/// until end of turn. Activate only as a sorcery." Zookeeper Mechan: "{T}:
/// Add {R}. / {6}{R}: Target creature you control gets +4/+0 until end of
/// turn. Activate only as a sorcery."
#[test]
fn galeblade_turbodrone_and_zookeeper_activations() {
    let mut g = scen().battlefield(0, &["Illvoi Galeblade", "Island", "Island"]).at(Step::Main1, 0).start();
    let gb = bf(&g, 0, "Illvoi Galeblade");
    let hand = g.player(0).hand.len();
    start_with(&mut g, act(gb, 0), |_| None);
    settle(&mut g);
    assert_eq!((g.obj(gb).zone, g.player(0).hand.len()), (Zone::Graveyard, hand + 1));

    let mut g = scen().battlefield(0, &["Kavaron Turbodrone", "Hullcarver"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let (k, h) = (bf(&g, 0, "Kavaron Turbodrone"), bf(&g, 0, "Hullcarver"));
    g.choose(act(k, 0));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Bear"))), "a creature you control");
    let th = g.tgt(h);
    g.choose(th);
    settle(&mut g);
    assert_eq!(g.pt(h), (2, 2));
    assert!(g.has_kw(h, Kw::Haste));

    let g = scen().battlefield(0, &["Zookeeper Mechan"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}")));
    let mut g = scen().battlefield(0, &["Zookeeper Mechan", "Hullcarver"]).battlefield(0, &["Mountain"; 7]).at(Step::Main1, 0).start();
    let (z, h) = (bf(&g, 0, "Zookeeper Mechan"), bf(&g, 0, "Hullcarver"));
    let th = g.tgt(h);
    start_with(&mut g, act(z, 1), move |g| pick(g, &[th]));
    settle(&mut g);
    assert_eq!(g.pt(h), (5, 1));
}

/// Slagdrill Scrapper: "{2}, {T}, Sacrifice another artifact or land: Draw a
/// card." Umbral Collar Zealot: "Sacrifice another creature or artifact:
/// Surveil 1." Lightless Evangel: "Whenever you sacrifice another creature or
/// artifact, put a +1/+1 counter on this creature."
#[test]
fn slagdrill_scrapper_umbral_collar_zealot_and_lightless_evangel_sacrifices() {
    let mut g = scen().battlefield(0, &["Slagdrill Scrapper", "Lightless Evangel", "Island", "Island", "Forest"]).at(Step::Main1, 0).start();
    let (s, e, f) = (bf(&g, 0, "Slagdrill Scrapper"), bf(&g, 0, "Lightless Evangel"), bf(&g, 0, "Forest"));
    let hand = g.player(0).hand.len();
    start_with(&mut g, act(s, 0), move |g| {
        if g.kind() == Some(DecisionKind::Sacrifice) {
            assert!(!g.has_option(Choice::Card(s)), "another");
            assert!(!g.has_option(Choice::Card(e)), "an artifact or land");
        }
        pick(g, &[Choice::Card(f)])
    });
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.counters_on(e, Counter::P1P1), 0, "a land isn't a creature or artifact");

    let mut g = scen().battlefield(0, &["Umbral Collar Zealot", "Lightless Evangel", "Hullcarver"]).at(Step::Main1, 0).start();
    let (z, e, h) = (bf(&g, 0, "Umbral Collar Zealot"), bf(&g, 0, "Lightless Evangel"), bf(&g, 0, "Hullcarver"));
    start_with(&mut g, act(z, 0), move |g| pick(g, &[Choice::Card(h)]));
    settle(&mut g);
    assert_eq!(g.obj(h).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(e, Counter::P1P1), 1);
    assert!(!g.has_option(act(z, 0)) || g.permanents_of(0).count() == 2, "Zealot can't sacrifice itself");
}

/// Melded Moxite: "When this artifact enters, you may discard a card. If you
/// do, draw two cards. / {3}, Sacrifice this artifact: Create a tapped 2/2
/// colorless Robot artifact creature token."
#[test]
fn melded_moxite_rummages_and_becomes_a_robot() {
    let mut g = scen().battlefield(0, &["Mountain"; 5]).hand(0, &["Melded Moxite", "Bombard"]).at(Step::Main1, 0).start();
    let bombard = g.card(0, "Bombard", Zone::Hand);
    cast_and_resolve(&mut g, 0, "Melded Moxite", move |g| pick(g, &[Choice::Yes, Choice::Card(bombard)]));
    assert_eq!(g.obj(bombard).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), 2, "discarded one, drew two");
    let m = bf(&g, 0, "Melded Moxite");
    start_with(&mut g, act(m, 0), |_| None);
    settle(&mut g);
    let r = tokens_named(&g, 0, "Robot");
    assert_eq!(r.len(), 1);
    assert!(g.obj(r[0]).tapped);
}

// ---- combat triggers ---------------------------------------------------------

/// Molecular Modifier: "At the beginning of combat on your turn, target
/// creature you control gets +1/+0 and gains first strike until end of
/// turn." Syr Vondam: "Whenever Syr Vondam enters or attacks, other
/// creatures you control get +1/+0 and gain deathtouch until end of turn."
#[test]
fn molecular_modifier_and_syr_vondam() {
    let mut g = scen().battlefield(0, &["Molecular Modifier", "Test Bear"]).at(Step::Main1, 0).start();
    let b = bf(&g, 0, "Test Bear");
    let tb = g.tgt(b);
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers), move |g| pick(g, &[tb]));
    assert_eq!(g.pt(b), (3, 2));
    assert!(g.has_kw(b, Kw::FirstStrike));

    let mut g = scen().battlefield(0, &["Plains", "Plains", "Swamp", "Swamp", "Swamp", "Test Bear"]).hand(0, &["Syr Vondam, the Lucent"]).at(Step::Main1, 0).start();
    let b = bf(&g, 0, "Test Bear");
    cast_and_resolve(&mut g, 0, "Syr Vondam, the Lucent", |_| None);
    let v = bf(&g, 0, "Syr Vondam, the Lucent");
    assert_eq!(g.pt(b), (3, 2));
    assert!(g.has_kw(b, Kw::Deathtouch));
    assert_eq!(g.pt(v), (4, 4), "other creatures");
}

// ---- Burgeoning --------------------------------------------------------------

/// Burgeoning: "Whenever an opponent plays a land, you may put a land card
/// from your hand onto the battlefield."
#[test]
fn burgeoning_puts_a_land_when_an_opponent_plays_one() {
    let mut g = scen().battlefield(0, &["Burgeoning"]).hand(0, &["Swamp", "Bombard"]).hand(1, &["Forest"]).at(Step::Main1, 1).start();
    let swamp = g.card(0, "Swamp", Zone::Hand);
    play_land(&mut g, 1, "Forest", move |g| pick(g, &[Choice::Yes, Choice::Card(swamp)]));
    assert_eq!(g.obj(swamp).zone, Zone::Battlefield);
    let mut g = scen().battlefield(0, &["Burgeoning"]).hand(0, &["Swamp", "Forest"]).at(Step::Main1, 0).start();
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(g.card(0, "Swamp", Zone::Hand), g.card(0, "Swamp", Zone::Hand), "your own land: nothing");
    assert!(g.waiting_triggers().is_empty() && g.stack().is_empty());
}

// ---- lands -------------------------------------------------------------------

/// The Stellar Sights manlands: "This land enters tapped. / {T}: Add {A} or
/// {B}. / <cost>: Until end of turn, this land becomes a p/t A and B
/// Elemental creature with <keyword>. It's still a land."
#[test]
fn stellar_sights_manlands_animate_as_their_oracle_text() {
    let rows: &[(&str, &str, &str, (i32, i32), u8, Kw)] = &[
        ("Hissing Quagmire", "{B}{G}", "{1}{B}{G}", (2, 2), 0b10100, Kw::Deathtouch),
        ("Lumbering Falls", "{G}{U}", "{2}{G}{U}", (3, 3), 0b10010, Kw::Hexproof),
        ("Needle Spires", "{R}{W}", "{2}{R}{W}", (2, 1), 0b01001, Kw::DoubleStrike),
        ("Shambling Vent", "{W}{B}", "{1}{W}{B}", (2, 3), 0b00101, Kw::Lifelink),
        ("Stirring Wildwood", "{G}{W}", "{1}{G}{W}", (3, 4), 0b10001, Kw::Reach),
    ];
    for &(name, two, anim, pt, colors, kw) in rows {
        let mut g = scen().hand(0, &[name]).battlefield(0, &["Plains", "Island", "Swamp", "Mountain", "Forest"]).at(Step::Main1, 0).start();
        play_land(&mut g, 0, name, |_| None);
        let l = bf(&g, 0, name);
        assert!(g.obj(l).tapped, "{name} enters tapped");
        g.set_tapped(l, false);
        let d = g.def_of(l);
        let mana = d.activated.iter().find(|a| a.mana().is_some()).expect("a mana ability");
        assert!(matches!(mana.mana().unwrap().prod, engine::db::ManaProd::One(m) if m as u8 == colors), "{name}: {two}");
        let i = d.activated.iter().position(|a| a.cost.mana == cost(anim)).expect("the animation") as u8;
        start_with(&mut g, act(l, i), |_| None);
        settle(&mut g);
        assert!(g.is_creature(l) && g.is_type(l, types::LAND), "{name}: still a land");
        assert_eq!(g.pt(l), pt, "{name}");
        assert_eq!(g.colors_of(l), colors, "{name}");
        assert!(g.has_kw(l, kw), "{name}");
        assert!(subtype(&g, l, "Elemental"), "{name}");
    }
}

/// Dust Bowl: "{T}: Add {C}. / {3}, {T}, Sacrifice a land: Destroy target
/// nonbasic land."
#[test]
fn dust_bowl_destroys_a_nonbasic_land() {
    let mut g = scen().battlefield(0, &["Dust Bowl", "Island", "Island", "Island", "Swamp"]).battlefield(1, &["Petrified Field", "Forest"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{C}")));
    let db_ = bf(&g, 0, "Dust Bowl");
    let (lotus, forest, swamp) = (bf(&g, 1, "Petrified Field"), bf(&g, 1, "Forest"), bf(&g, 0, "Swamp"));
    start_with(&mut g, act(db_, 1), move |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(forest)), "nonbasic");
        }
        pick(g, &[g.tgt(lotus), Choice::Card(swamp)])
    });
    settle(&mut g);
    assert_eq!(g.obj(lotus).zone, Zone::Graveyard);
}
