//! M8d part 4 (ENGINE.md 40): the cube's "other" rulings left after part 3:
//! choices among a player's permanents by card type (Ajani), reconfigure
//! and Equipment (CR 301.5, 702.151), a naming tax in every zone, one
//! spell a turn over the whole turn, explore and connive with nothing to
//! reveal or discard (701.44, 701.50), control and life (Oust, Thieving
//! Skydiver, Oko, Dack), Balance's order, Narset, untapping lands, extra
//! turns (500.7), abilities of tapped permanents, durations, discard
//! spells, counters running out, colorless emblems, flurry, Sneak Attack,
//! Fastbond, Walking Ballista, Grindstone, living weapon, "lost life this
//! turn", modes in printed order (700.2), Lurrus and other permissions,
//! hideaway. `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

/// Answer `Target` decisions with `t` when it is an option (else first
/// options) and resolve the stack.
fn resolve_at(g: &mut Game, t: CardIdx) {
    let c = g.tgt(t);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(c)).then_some(c));
}

/// P`p` casts `name` from hand, answering targets with `t`, and everything
/// resolves.
fn cast_at(g: &mut Game, p: u8, name: &str, t: CardIdx) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve_at(g, t);
}

/// Declare `attackers` (at the defending player) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let defender = 1 - g.turn().active;
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(defender));
        }
    }
    g.choose(Choice::Pass);
}

fn to_step(g: &mut Game, step: Step, p: u8) {
    quiet_until(g, |g| g.turn().step == step && g.stack().is_empty() && at_priority(g) && g.turn().priority == p);
}

// ---- Ajani, Nacatl Avenger's -4: choices by card type ------------------------------------------

const AJANI: &str = "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger";

/// P0's Ajani, Nacatl Avenger uses its -4 against P1's `p1` permanents
/// (a Phyrexian Metamorph among them with two +1/+1 counters: it copied
/// nothing); P1 answers its choices with `pick` (by name, in order), else
/// the first.
fn ajani_minus_four(p1: &[&str], pick: &[&str]) -> Game {
    let mut s = scen()
        .battlefield(0, &[AJANI])
        .transformed(0, AJANI)
        .counters(0, "Ajani, Nacatl Avenger", Counter::Loyalty, 5)
        .battlefield(1, p1);
    if p1.contains(&"Phyrexian Metamorph") {
        s = s.counters(1, "Phyrexian Metamorph", Counter::P1P1, 2);
    }
    let mut g = s.at(Step::Main1, 0).start();
    let a = bf(&g, 0, "Ajani, Nacatl Avenger");
    g.choose(ab(a, 2));
    let i = std::cell::Cell::new(0);
    let pick: Vec<String> = pick.iter().map(|s| s.to_string()).collect();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => {
            let want = pick.get(i.get()).cloned();
            i.set(i.get() + 1);
            let d = g.pending_decision().unwrap();
            want.and_then(|w| d.options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) if g.name(x) == w)))
        }
        _ => None,
    });
    g
}

#[test]
fn ajanis_minus_four_lets_a_permanent_stand_for_any_of_its_types_and_skips_missing_types_cr_701_21a() {
    // ruling: 0c460f68 7d9c06e2
    // P1: Phyrexian Metamorph (an artifact creature that copied nothing,
    // two +1/+1 counters), a Test Bear, a Sol Ring. P1 keeps the Metamorph
    // as its artifact and its creature: the Bear and the Sol Ring are
    // sacrificed. P1 with no artifact (a Test Bear and
    // Underworld Breach): it still keeps a creature and an enchantment.
    let g = ajani_minus_four(&["Phyrexian Metamorph", "Test Bear", "Sol Ring"], &["Phyrexian Metamorph", "Phyrexian Metamorph"]);
    assert_eq!(g.count_in(1, "Phyrexian Metamorph", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1, "{}", log(&g));
    let g = ajani_minus_four(&["Test Bear", "Underworld Breach"], &[]);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "no artifact: still a creature kept\n{}", log(&g));
    assert_eq!(g.count_in(1, "Underworld Breach", Zone::Battlefield), 1, "and an enchantment");
}

// ---- reconfigure and Equipment (CR 301.5, 702.151) ---------------------------------------------

#[test]
fn lion_sash_attached_by_another_effect_or_while_tapped_isnt_a_creature_cr_702_151b() {
    // ruling: 35466139 19d3529a 05fbac90
    // Thieving Skydiver kicked (X = 2) steals P1's Lion Sash and attaches
    // it: attached, it isn't a creature. P0's Lion Sash attacks (tapped),
    // then reconfigures onto a Test Bear: attached and still tapped. Tishana's
    // Tidebinder counters its {W} ability: it loses all abilities, and stays
    // a noncreature while attached.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &["Thieving Skydiver"])
        .battlefield(1, &["Lion Sash"])
        .at(Step::Main1, 0)
        .start();
    let sash = bf(&g, 1, "Lion Sash");
    let ts = g.card(0, "Thieving Skydiver", Zone::Hand);
    g.choose(Choice::Cast(ts));
    let t = g.tgt(sash);
    // (Kicked once, then X = 2.)
    let modes = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            modes.set(modes.get() + 1);
            Some(Choice::Number(if modes.get() == 1 { 1 } else { 2 }))
        }
        Some(DecisionKind::Target) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(sash).controller, 0, "{}", log(&g));
    assert_eq!(g.obj(sash).attached, Some(g.obj_ref(ts)), "{}", log(&g));
    assert!(!g.is_creature(sash), "attached: not a creature");
    // Tapped, then attached; Tidebinder.
    let mut g = scen()
        .battlefield(0, &["Lion Sash", "Test Bear", "Plains", "Plains", "Plains"])
        .graveyard(1, &["Island"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Tishana's Tidebinder"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let sash = bf(&g, 0, "Lion Sash");
    let bear = bf(&g, 0, "Test Bear");
    attack(&mut g, &[sash]);
    to_step(&mut g, Step::Main2, 0);
    assert!(g.obj(sash).tapped);
    g.choose(ab(sash, 1));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(sash).attached, Some(g.obj_ref(bear)), "{}", log(&g));
    assert!(g.obj(sash).tapped, "attaching doesn't untap it");
    assert!(!g.is_creature(sash));
    let island = g.card(1, "Island", Zone::Graveyard);
    g.choose(ab(sash, 0));
    let ti = g.tgt(island);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (at_priority(g) && g.turn().priority == 1));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(ti);
    }
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let tb = g.card(1, "Tishana's Tidebinder", Zone::Hand);
    g.choose(Choice::Cast(tb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    assert!(log(&g).contains("Lion Sash") && g.obj(island).zone == Zone::Graveyard, "countered\n{}", log(&g));
    assert!(!g.has_option(ab(sash, 0)), "no abilities: {}", opts(&g));
    assert_eq!(g.obj(sash).attached, Some(g.obj_ref(bear)));
    assert!(!g.is_creature(sash), "still attached: still not a creature\n{}", log(&g));
}

#[test]
fn an_equipment_stays_when_its_creature_dies_and_moves_by_its_equip_ability_cr_301_5c_704_5n() {
    // ruling: 338a52df 745c7726 b88fdb37
    // Glimmer Lens's Rebel Bolted: the Lens stays; equipped to a Test Bear.
    // Nettlecyst's Germ Bolted: Nettlecyst stays. Equipped to the Bear, the
    // 0/0 Germ dies.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "Test Bear"])
        .hand(0, &["Glimmer Lens"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let gl = g.card(0, "Glimmer Lens", Zone::Hand);
    g.choose(Choice::Cast(gl));
    resolve(&mut g);
    let rebel = tokens_named(&g, "Rebel")[0];
    assert_eq!(g.obj(gl).attached, Some(g.obj_ref(rebel)));
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    cast_at(&mut g, 1, "Lightning Bolt", rebel);
    assert!(tokens_named(&g, "Rebel").is_empty());
    assert_eq!(g.obj(gl).zone, Zone::Battlefield);
    assert_eq!(g.obj(gl).attached, None);
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 0 && g.stack().is_empty());
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(gl, 0));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(gl).attached, Some(g.obj_ref(bear)), "{}", log(&g));
    // Nettlecyst.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Test Bear"])
        .hand(0, &["Nettlecyst"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let nc = g.card(0, "Nettlecyst", Zone::Hand);
    g.choose(Choice::Cast(nc));
    resolve(&mut g);
    let germ = tokens_named(&g, "Phyrexian Germ")[0];
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(nc, 0));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(nc).attached, Some(g.obj_ref(bear)), "{}", log(&g));
    assert!(tokens_named(&g, "Phyrexian Germ").is_empty(), "the unequipped 0/0 Germ dies\n{}", log(&g));
    let _ = germ;
    // A Germ destroyed: Nettlecyst stays.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Nettlecyst"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let nc = g.card(0, "Nettlecyst", Zone::Hand);
    g.choose(Choice::Cast(nc));
    resolve(&mut g);
    let germ = tokens_named(&g, "Phyrexian Germ")[0];
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    cast_at(&mut g, 1, "Lightning Bolt", germ);
    assert!(tokens_named(&g, "Phyrexian Germ").is_empty(), "{}", log(&g));
    assert_eq!(g.obj(nc).zone, Zone::Battlefield);
}

#[test]
fn shroud_from_lightning_greaves_keeps_other_equipment_off_your_only_creature_cr_702_18a() {
    // ruling: 4c667be8
    // Greaves on P0's only creature: Skateboard's equip can't target it,
    // and Greaves can't be unequipped (no action does it).
    let mut g = scen().battlefield(0, &["Lightning Greaves", "Skateboard", "Test Bear", "Mountain"]).at(Step::Main1, 0).start();
    let gr = bf(&g, 0, "Lightning Greaves");
    let bear = bf(&g, 0, "Test Bear");
    let sk = bf(&g, 0, "Skateboard");
    g.choose(ab(gr, 0));
    resolve_at(&mut g, bear);
    assert!(g.has_kw(bear, Kw::Shroud));
    assert!(!g.has_option(ab(sk, 0)), "no legal target for Skateboard's equip: {}", opts(&g));
    assert!(!g.has_option(ab(gr, 0)), "Greaves can't move: no other creature: {}", opts(&g));
}

// ---- a naming tax in every zone ----------------------------------------------------------------

#[test]
fn anointed_peacekeepers_tax_applies_to_a_named_card_cast_from_a_graveyard_cr_601_2f() {
    // ruling: 93ee06dd
    // P0's Peacekeeper named Faithless Looting: P1's flashback ({2}{R})
    // costs {2} more: not with three Mountains, yes with five.
    for (lands, castable) in [(3usize, false), (5, true)] {
        let mut g = scen()
            .battlefield(0, &["Plains", "Plains", "Plains"])
            .hand(0, &["Anointed Peacekeeper"])
            .hand(1, &["Island"])
            .graveyard(1, &["Faithless Looting"])
            .battlefield(1, &vec!["Mountain"; lands])
            .at(Step::Main1, 0)
            .start();
        let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
        g.choose(Choice::Cast(ap));
        let fl = db().expect_id("Faithless Looting").unwrap();
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::NameCard) if g.has_option(Choice::Def(fl)) => Some(Choice::Def(fl)),
            Some(DecisionKind::NameCard) => first_non_pass(g),
            _ => None,
        });
        assert!(log(&g).contains("Faithless Looting"), "named\n{}", log(&g));
        quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
        let c = g.card(1, "Faithless Looting", Zone::Graveyard);
        assert_eq!(g.has_option(Choice::Cast(c)), castable, "{lands} Mountains: {}", opts(&g));
    }
}

/// Every name the pending `NameCard` decision offers, over its pages (a
/// clone answers "next page").
fn names_offered(g: &Game) -> Vec<u16> {
    let mut g = g.clone();
    let mut out = Vec::new();
    loop {
        let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
        out.extend(opts.iter().filter_map(|c| match *c {
            Choice::Def(d) => Some(d),
            _ => None,
        }));
        match opts.iter().find(|c| matches!(c, Choice::Page(_))) {
            Some(&next) => {
                g.choose(next);
            }
            None => return out,
        }
    }
}

/// Pay with the first options and pass priority until a `NameCard`
/// decision is pending.
fn to_name_question(g: &mut Game) {
    for _ in 0..100 {
        if g.kind() == Some(DecisionKind::NameCard) {
            return;
        }
        let c = if at_priority(g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("no name question: {}", opts(g));
}

/// P0 casts Anointed Peacekeeper from its hand and, when it asks, names `d`
/// (`None`: `PASS`), paging to it; the stack resolves.
fn peacekeeper_names(g: &mut Game, d: Option<u16>) {
    let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
    g.choose(Choice::Cast(ap));
    let want = d.map_or(Choice::Pass, Choice::Def);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::NameCard) if g.has_option(want) => Some(want),
        Some(DecisionKind::NameCard) => {
            let next = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Page(_)));
            Some(next.unwrap_or_else(|| panic!("{want:?} is not offered")))
        }
        _ => None,
    });
    let ap = bf(g, 0, "Anointed Peacekeeper");
    let named = engine::game::Mark::ChosenName(d.unwrap_or(u16::MAX));
    assert!(g.marks_of(ap).contains(&named), "{:?}", g.marks_of(ap));
}

#[test]
fn anointed_peacekeepers_ability_tax_takes_equip_and_leaves_triggered_abilities_alone_cr_602_2b() {
    // ruling: a6040722 252f0b38
    // Named Skullclamp: equip {1} is an activated ability (a keyword one; it
    // costs mana and adds none, so it is no mana ability): {3}, not with two
    // Swamps, yes with three. Its "whenever equipped creature dies, draw two
    // cards" is a triggered ability: unaffected, it draws two.
    for (swamps, ok) in [(2usize, false), (3, true)] {
        let mut g = scen()
            .battlefield(0, &["Plains", "Plains", "Plains", "Skullclamp", "Test Hasty"])
            .battlefield(0, &vec!["Swamp"; swamps])
            .hand(0, &["Anointed Peacekeeper"])
            .at(Step::Main1, 0)
            .start();
        peacekeeper_names(&mut g, Some(id("Skullclamp")));
        let clamp = bf(&g, 0, "Skullclamp");
        assert_eq!(g.has_option(ab(clamp, 0)), ok, "{swamps} Swamps: {}", opts(&g));
        if ok {
            let hasty = bf(&g, 0, "Test Hasty");
            let before = g.player(0).hand.len();
            g.choose(ab(clamp, 0));
            resolve_at(&mut g, hasty);
            assert_eq!(g.obj(hasty).zone, Zone::Graveyard, "a 2/1 equipped is 3/0");
            assert_eq!(g.player(0).hand.len(), before + 2, "the trigger draws two");
        }
    }
}

#[test]
fn anointed_peacekeeper_leaves_mana_abilities_and_a_triggers_payment_alone_cr_605_1a() {
    // ruling: 252f0b38 a6040722
    // Named Mana Vault. "{T}: Add {C}{C}{C}" adds mana as it resolves: a
    // mana ability, no tax: with only the Vault untapped, Nettlecyst ({3})
    // is cast. "At the beginning of your upkeep, you may pay {4}. If you do,
    // untap Mana Vault" is a triggered ability: {4} (four lands), not {6}.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Island", "Mana Vault"])
        .hand(0, &["Anointed Peacekeeper", "Nettlecyst"])
        .at(Step::Main1, 0)
        .start();
    peacekeeper_names(&mut g, Some(id("Mana Vault")));
    let vault = bf(&g, 0, "Mana Vault");
    for c in g.lands_of(0).collect::<Vec<_>>() {
        g.set_tapped(c, true);
    }
    g.set_tapped(vault, false);
    g.refresh_decision();
    let nc = g.card(0, "Nettlecyst", Zone::Hand);
    assert!(g.has_option(Choice::Cast(nc)), "{}", opts(&g));
    g.choose(Choice::Cast(nc));
    resolve(&mut g);
    assert_eq!(g.count_in(0, "Nettlecyst", Zone::Battlefield), 1);
    assert!(g.obj(vault).tapped);
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.kind() == Some(DecisionKind::YesNo));
    assert_eq!((g.turn().active, g.turn().step), (0, Step::Upkeep));
    assert!(g.has_option(Choice::Yes), "four lands pay {{4}}: {}", opts(&g));
    g.choose(Choice::Yes);
    resolve(&mut g);
    assert!(!g.obj(vault).tapped, "paid {{4}}: untapped");
}

#[test]
fn anointed_peacekeeper_may_name_a_card_it_didnt_see_or_without_activated_abilities_cr_201_3() {
    // ruling: 2a160983
    // P1's hand is an Island; its library holds Lightning Bolt (on top) and
    // Test Bear (no activated ability), cards P0 never saw: both names are
    // offered. Named Lightning Bolt: P1 draws it, {2}{R}, not castable with
    // two Mountains.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Anointed Peacekeeper"])
        .hand(1, &["Island"])
        .library(1, &["Lightning Bolt", "Test Bear"])
        .battlefield(1, &["Mountain", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
    g.choose(Choice::Cast(ap));
    to_name_question(&mut g);
    let bolt = g.card(1, "Lightning Bolt", Zone::Library);
    assert!(!g.knows(0, bolt) && g.knows(0, g.card(1, "Island", Zone::Hand)), "it saw the hand only");
    let offered = names_offered(&g);
    assert!(offered.contains(&id("Lightning Bolt")) && offered.contains(&id("Test Bear")), "{offered:?}");
    while !g.has_option(Choice::Def(id("Lightning Bolt"))) {
        let next = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Page(_))).unwrap();
        g.choose(next);
    }
    g.choose(Choice::Def(id("Lightning Bolt")));
    resolve(&mut g);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty());
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "{{2}}{{R}} with two Mountains: {}", opts(&g));
}

#[test]
fn anointed_peacekeeper_offers_a_tokens_name_only_if_a_card_has_it_cr_201_3() {
    // ruling: abac3051
    // The table's token names: Faerie Dragon (Feywild Caretaker's token;
    // also an Astral card's name) is offered, Treasure, Clue and Goblin
    // Shaman (no card's names) are not; each token name offered is a card's.
    let mut g = scen().battlefield(0, &["Plains", "Plains", "Plains"]).hand(0, &["Anointed Peacekeeper"]).at(Step::Main1, 0).start();
    let ap = g.card(0, "Anointed Peacekeeper", Zone::Hand);
    g.choose(Choice::Cast(ap));
    to_name_question(&mut g);
    let names: Vec<&str> = names_offered(&g).iter().map(|&d| engine::sneak_show::face_name(db().def(d))).collect();
    let n = db().num_cards();
    let card_faces: Vec<&str> = (0..db().len() as u16)
        .filter(|&d| {
            let dd = db().def(d);
            !dd.token && ((d as usize) < n || dd.front.is_some_and(|f| (f as usize) < n))
        })
        .map(|d| engine::sneak_show::face_name(db().def(d)))
        .collect();
    assert!(db().token_by_name("Faerie Dragon").is_some() && names.contains(&"Faerie Dragon"), "{names:?}");
    for t in ["Treasure", "Clue", "Goblin Shaman"] {
        assert!(db().token_by_name(t).is_some(), "the table has a {t} token");
        assert!(!card_faces.contains(&t) && !names.contains(&t), "{t}");
    }
    for d in (n..db().len()).map(|t| t as u16).filter(|&t| db().def(t).token) {
        let name = engine::sneak_show::face_name(db().def(d));
        let card_name = card_faces.contains(&name) || name == "Faerie Dragon";
        assert_eq!(names.contains(&name), card_name, "token {name}");
    }
}

// ---- Archon of Emeria: one spell a turn --------------------------------------------------------

#[test]
fn archon_of_emeria_counts_spells_cast_this_turn_including_itself_and_countered_ones_cr_101_2() {
    // ruling: 0e494778 5153b574
    // P0 casts Archon of Emeria: no second spell that turn. P1's Bolt was
    // countered before P1's Archon resolved... P1 casts a Bolt, it is
    // countered (P0's Counterspell); P1's later spell is still its second.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Mountain"])
        .hand(0, &["Archon of Emeria", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let a = g.card(0, "Archon of Emeria", Zone::Hand);
    g.choose(Choice::Cast(a));
    resolve(&mut g);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(b)), "Archon itself was this turn's spell: {}", opts(&g));
    // A countered spell counts.
    let mut g = scen()
        .battlefield(0, &["Archon of Emeria", "Island", "Island"])
        .hand(0, &["Counterspell"])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let b1 = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let cs = g.card(0, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(b1);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(b1).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 20);
    let b2 = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(at_priority(&g) && g.turn().priority == 1);
    assert!(!g.has_option(Choice::Cast(b2)), "the countered Bolt counts: {}", opts(&g));
}

// ---- explore and connive with nothing (CR 701.44, 701.50) --------------------------------------

#[test]
fn exploring_with_an_empty_library_puts_a_counter_cr_701_44a() {
    // ruling: bf9c2cf4
    // A Map token (Sentinel of the Nameless City) makes a Test Bear explore
    // with P0's library empty: nothing revealed, a +1/+1 counter.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Island", "Island", "Forest", "Forest", "Test Bear"])
        .hand(0, &["Sentinel of the Nameless City"])
        .at(Step::Main1, 0)
        .start();
    let s = g.card(0, "Sentinel of the Nameless City", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve(&mut g);
    let map = tokens_named(&g, "Map")[0];
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(map, 0));
    resolve_at(&mut g, bear);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
    assert!(!g.is_over());
}

#[test]
fn conniving_with_no_card_discarded_puts_no_counter_and_connive_x_counts_nonland_discards_cr_701_50() {
    // ruling: 12cec5e6 18c98b5c
    // P1's Narset: P0 drew in its draw step, so Norman Osborn's connive
    // draws nothing; P0's hand is empty: no discard, no counter. Raffine
    // with two attackers: the Bear connives 2: two drawn (a Bolt and an
    // Island), two discarded (the Bolt and an Island from hand): one
    // counter.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Norman Osborn // Green Goblin"])
        .battlefield(1, &["Narset, Parter of Veils"])
        .at(Step::Untap, 0)
        .start();
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    let i = g.card(0, "Island", Zone::Hand);
    g.choose(Choice::Play(i));
    resolve(&mut g);
    assert!(g.player(0).hand.is_empty());
    let n = bf(&g, 0, "Norman Osborn // Green Goblin");
    attack(&mut g, &[n]);
    to_step(&mut g, Step::Main2, 0);
    assert_eq!(g.life(1), 19, "{}", log(&g));
    assert!(g.player(0).hand.is_empty(), "Narset: no second draw\n{}", log(&g));
    assert_eq!(g.counters_on(n, Counter::P1P1), 0, "nothing discarded: no counter");
    // Raffine: connive 2.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Lightning Bolt", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Raffine, Scheming Seer", "Test Bear"])
        .hand(0, &["Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let r = bf(&g, 0, "Raffine, Scheming Seer");
    let bear = bf(&g, 0, "Test Bear");
    attack(&mut g, &[r, bear]);
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let tb = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        _ if g.has_option(Choice::Card(bolt)) => Some(Choice::Card(bolt)),
        _ => None,
    });
    assert_eq!(g.graveyard_cards(0).count(), 2, "two discarded\n{}", log(&g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "one nonland card discarded\n{}", log(&g));
}

// ---- control, life, Balance ---------------------------------------------------------------------

#[test]
fn oust_gives_the_life_to_the_creatures_controller_and_oko_leaves_auras_and_equipment_cr_110_2() {
    // ruling: 259ab0fd 565d48d8
    // Oko exchanges P0's Food for P1's Test Bear (Skateboard attached, P1's):
    // P0 controls the Bear, Skateboard stays P1's and attached. P1 Ousts the
    // Bear (owned by P1, controlled by P0): P0 gains 3. Dack Fayden's -2
    // takes P1's Skateboard attached to P1's Test Cat: still attached.
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns"])
        .counters(0, "Oko, Thief of Crowns", Counter::Loyalty, 8)
        .battlefield(1, &["Test Bear", "Skateboard", "Plains", "Mountain"])
        .hand(1, &["Oust"])
        .at(Step::Main1, 1)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let sk = bf(&g, 1, "Skateboard");
    g.choose(ab(sk, 0));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(sk).attached, Some(g.obj_ref(bear)));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    g.choose(ab(oko, 0));
    resolve(&mut g);
    let food = tokens_named(&g, "Food")[0];
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().number > 5 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(ab(oko, 2));
    let (tf, tb) = (g.tgt(food), g.tgt(bear));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tf) => Some(tf),
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        _ => None,
    });
    assert_eq!(g.obj(bear).controller, 0, "{}", log(&g));
    assert_eq!(g.obj(sk).controller, 1, "the Equipment stays P1's");
    assert_eq!(g.obj(sk).attached, Some(g.obj_ref(bear)), "and attached");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let life0 = g.life(0);
    cast_at(&mut g, 1, "Oust", bear);
    assert_eq!(g.life(0), life0 + 3, "its controller gains the life\n{}", log(&g));
    // Dack Fayden.
    let mut g = scen()
        .battlefield(0, &["Dack Fayden"])
        .battlefield(1, &["Test Cat", "Skateboard", "Mountain"])
        .at(Step::Main1, 1)
        .start();
    let cat = bf(&g, 1, "Test Cat");
    let sk = bf(&g, 1, "Skateboard");
    g.choose(ab(sk, 0));
    resolve_at(&mut g, cat);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let dack = bf(&g, 0, "Dack Fayden");
    g.choose(ab(dack, 1));
    resolve_at(&mut g, sk);
    assert_eq!(g.obj(sk).controller, 0, "{}", log(&g));
    assert_eq!(g.obj(sk).attached, Some(g.obj_ref(cat)), "gaining control doesn't unattach it");
}

#[test]
fn thieving_skydivers_control_lasts_and_it_may_take_your_own_artifact_cr_611_2a() {
    // ruling: 3a9c110e 194f57a3
    // Kicked (X = 1), Thieving Skydiver takes P1's Sol Ring; the Skydiver
    // Bolted, the Ring stays P0's, also next turn. Kicked targeting P0's own
    // Skateboard: it is attached to the Skydiver.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Thieving Skydiver"])
        .battlefield(1, &["Sol Ring", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    let ts = g.card(0, "Thieving Skydiver", Zone::Hand);
    g.choose(Choice::Cast(ts));
    let t = g.tgt(ring);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(ring).controller, 0, "{}", log(&g));
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    cast_at(&mut g, 1, "Lightning Bolt", ts);
    assert_eq!(g.obj(ts).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.obj(ring).controller, 0, "indefinitely, the Skydiver gone");
    // P0's own Skateboard.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Skateboard"])
        .hand(0, &["Thieving Skydiver"])
        .at(Step::Main1, 0)
        .start();
    let sk = bf(&g, 0, "Skateboard");
    let ts = g.card(0, "Thieving Skydiver", Zone::Hand);
    g.choose(Choice::Cast(ts));
    let t = g.tgt(sk);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => {
            assert!(g.has_option(t), "its own artifact: {}", opts(g));
            Some(t)
        }
        _ => None,
    });
    assert_eq!(g.obj(sk).attached, Some(g.obj_ref(ts)), "{}", log(&g));
}

#[test]
fn balance_counts_creatures_after_the_lands_were_sacrificed_cr_608_2c() {
    // ruling: fe4ee6d9
    // P0's animated Creeping Tar Pit and five other lands, P1's five lands
    // and two Test Bears. Balance: P0 sacrifices the Tar Pit (a land); then
    // P0 controls no creature: P1 sacrifices both Bears.
    let mut g = scen()
        .battlefield(0, &["Creeping Tar Pit", "Plains", "Island", "Swamp", "Swamp", "Mountain"])
        .hand(0, &["Balance"])
        .battlefield(1, &["Forest", "Forest", "Forest", "Forest", "Forest", "Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let tp = bf(&g, 0, "Creeping Tar Pit");
    let pl = bf(&g, 0, "Plains");
    g.choose(ab(tp, 1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| !matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == tp || x == pl)),
        _ => None,
    });
    assert!(g.is_creature(tp));
    let bal = g.card(0, "Balance", Zone::Hand);
    g.choose(Choice::Cast(bal));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| !matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == tp)),
        // P0 keeps five lands: not the Tar Pit.
        Some(DecisionKind::ChoosePermanent) if g.pending_decision().unwrap().player == 0 => {
            g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(tp) && c != Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(g.obj(tp).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 2, "creatures counted after: none for P0\n{}", log(&g));
}

#[test]
fn narset_turns_a_multi_card_draw_into_one_if_none_drawn_this_turn_cr_121_2() {
    // ruling: a549c65a
    // P1's Narset: P0 (no card drawn yet this turn: its first turn's Main1)
    // casts Ancestral Recall on itself: one card. (No cube effect offers an
    // optional draw of several cards.)
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Ancestral Recall"])
        .battlefield(1, &["Narset, Parter of Veils"])
        .at(Step::Main1, 0)
        .start();
    assert_eq!(g.player(0).drawn, 0);
    let ar = g.card(0, "Ancestral Recall", Zone::Hand);
    g.choose(Choice::Cast(ar));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(0)));
    assert_eq!(g.player(0).hand.len(), 1, "one card\n{}", log(&g));
}

#[test]
fn frantic_search_untaps_lands_it_doesnt_target_including_an_opponents_cr_115_10() {
    // ruling: beb67500
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Frantic Search", "Test Bear", "Test Bear"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let m = bf(&g, 1, "Mountain");
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(0)));
    assert!(g.obj(m).tapped);
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 0);
    let fs = g.card(0, "Frantic Search", Zone::Hand);
    g.choose(Choice::Cast(fs));
    let seen = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => panic!("no target"),
        Some(DecisionKind::ChoosePermanent) => {
            seen.set(seen.get() || g.has_option(Choice::Card(m)));
            if g.has_option(Choice::Card(m)) {
                Some(Choice::Card(m))
            } else {
                Some(Choice::Pass)
            }
        }
        _ => None,
    });
    assert!(seen.get(), "P1's Mountain is offered");
    assert!(!g.obj(m).tapped, "untapped\n{}", log(&g));
}

#[test]
fn extra_turns_are_taken_most_recent_first_cr_500_7() {
    // ruling: 066e2812
    // P0 casts Time Walk (P0's extra turn), then Time Warp targeting P1
    // (P1's): P1's extra turn comes first, then P0's, then P1's normal
    // turn.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Time Walk", "Time Warp"])
        .at(Step::Main1, 0)
        .start();
    let tw = g.card(0, "Time Walk", Zone::Hand);
    g.choose(Choice::Cast(tw));
    resolve(&mut g);
    let tp = g.card(0, "Time Warp", Zone::Hand);
    g.choose(Choice::Cast(tp));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    let t = g.turn().number;
    let mut order = Vec::new();
    for k in 1..=3 {
        quiet_until(&mut g, |g| g.turn().number == t + k && g.turn().step == Step::Main1 && at_priority(g));
        order.push(g.turn().active);
    }
    assert_eq!(order, vec![1, 0, 1], "{}", log(&g));
}

// ---- abilities and durations ---------------------------------------------------------------------

#[test]
fn iron_shield_elf_activates_while_tapped_cr_602_1() {
    // ruling: 33154328
    let mut g = scen().battlefield(0, &["Iron-Shield Elf"]).hand(0, &["Island"]).at(Step::DeclareAttackers, 0).start();
    let e = bf(&g, 0, "Iron-Shield Elf");
    attack(&mut g, &[e]);
    to_step(&mut g, Step::DeclareAttackers, 0);
    assert!(g.obj(e).tapped);
    assert!(g.has_option(ab(e, 0)), "tapped: still activatable: {}", opts(&g));
    g.choose(ab(e, 0));
    resolve(&mut g);
    assert!(g.has_kw(e, Kw::Indestructible));
}

#[test]
fn corpse_dances_haste_ends_with_the_turn_cr_611_2a() {
    // ruling: 36b545c9
    // Corpse Dance returns a Test Bear with haste; P1's Tishana's
    // Tidebinder counters the end-step exile; next turn the Bear has no
    // haste.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &["Corpse Dance"])
        .graveyard(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Tishana's Tidebinder"])
        .at(Step::Main1, 0)
        .start();
    let cd = g.card(0, "Corpse Dance", Zone::Hand);
    g.choose(Choice::Cast(cd));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseCost)).then_some(Choice::Number(0)));
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(bear, Kw::Haste), "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::End && !g.stack().is_empty() && at_priority(g) && g.turn().priority == 1);
    let tb = g.card(1, "Tishana's Tidebinder", Zone::Hand);
    g.choose(Choice::Cast(tb));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| first_non_pass(g)).flatten());
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "the exile countered\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_kw(bear, Kw::Haste), "until end of turn only");
}

#[test]
fn hand_reveals_and_discards_targeting_yourself_or_an_empty_hand_cr_701_20b() {
    // ruling: 695c629c 311524ab 98549dec
    // Inquisition of Kozilek on P0 itself: P0's hand is revealed. Thoughtseize
    // on P1 with only lands: P0 loses 2 anyway. Gitaxian Probe on an empty
    // hand: P0 still draws.
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Inquisition of Kozilek", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.knows(1, bolt));
    let i = g.card(0, "Inquisition of Kozilek", Zone::Hand);
    g.choose(Choice::Cast(i));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(0)));
    assert!(g.knows(1, bolt) || g.obj(bolt).zone == Zone::Graveyard, "revealed to P1\n{}", log(&g));
    // Thoughtseize.
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Thoughtseize"]).hand(1, &["Forest", "Island"]).at(Step::Main1, 0).start();
    let ts = g.card(0, "Thoughtseize", Zone::Hand);
    g.choose(Choice::Cast(ts));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert_eq!(g.life(0), 18, "{}", log(&g));
    assert_eq!(g.player(1).hand.len(), 2);
    // Gitaxian Probe.
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Gitaxian Probe"]).at(Step::Main1, 0).start();
    let gp = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(gp));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::PayPhyrexian) => Some(Choice::Number(0)),
        _ => None,
    });
    assert!(g.player(1).hand.is_empty());
    assert_eq!(g.player(0).hand.len(), 1, "a card drawn\n{}", log(&g));
}

#[test]
fn wishclaw_talisman_without_wish_counters_stays_and_cant_be_activated_cr_118_3() {
    // ruling: 13cb7898
    // One wish counter left: P0 uses it; P1 gains control of the Talisman,
    // which stays on the battlefield with no counter: P1 can't activate it.
    let mut g = scen()
        .battlefield(0, &["Wishclaw Talisman", "Swamp"])
        .counters(0, "Wishclaw Talisman", Counter::Named(db().subtype_id("counter:wish").unwrap()), 1)
        .battlefield(1, &["Swamp", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let wt = bf(&g, 0, "Wishclaw Talisman");
    g.choose(ab(wt, 0));
    resolve(&mut g);
    assert_eq!(g.obj(wt).controller, 1, "{}", log(&g));
    assert_eq!(g.obj(wt).zone, Zone::Battlefield, "it stays");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_option(ab(wt, 0)), "no counter: no activation: {}", opts(&g));
}

#[test]
fn seasoned_pyromancer_with_one_card_discards_it_and_draws_two_cr_701_9() {
    // ruling: e165be5b
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .hand(0, &["Seasoned Pyromancer", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let sp = g.card(0, "Seasoned Pyromancer", Zone::Hand);
    g.choose(Choice::Cast(sp));
    resolve(&mut g);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), 2, "two drawn");
    assert_eq!(tokens_named(&g, "Elemental").len(), 1, "one nonland card discarded");
}

#[test]
fn chandras_emblem_is_a_colorless_source_cr_114_4_105_2c() {
    // ruling: 3c8289db
    // P1's Giver of Runes gives its Test Bear protection from red in P0's
    // upkeep; P0 casts a spell: the emblem's 5 damage (colorless) targets
    // and kills the Bear.
    let mut g = scen()
        .emblem(0, "Chandra, Torch of Defiance")
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm"])
        .battlefield(1, &["Giver of Runes", "Test Bear"])
        .at(Step::Upkeep, 0)
        .start();
    let giver = bf(&g, 1, "Giver of Runes");
    let bear = bf(&g, 1, "Test Bear");
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1);
    g.choose(ab(giver, 0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(3)),
        _ => None,
    });
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g) && g.turn().priority == 0);
    let b = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(g.has_option(t), "protection from red doesn't stop a colorless source: {}", opts(g));
            Some(t)
        }
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
}

#[test]
fn flurry_counts_spells_cast_before_the_cutter_cr_702_183a() {
    // ruling: 110f09f6
    // Cori-Steel Cutter is P0's first spell; the next one is the second: a
    // Monk.
    let mut g = scen().battlefield(0, &["Mountain", "Mountain", "Mountain"]).hand(0, &["Cori-Steel Cutter", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Cori-Steel Cutter", Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve(&mut g);
    assert!(tokens_named(&g, "Monk").is_empty());
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(tokens_named(&g, "Monk").len(), 1, "{}", log(&g));
}

#[test]
fn sneak_attack_doesnt_sacrifice_a_creature_that_left_and_came_back_cr_400_7() {
    // ruling: 921387ce
    // Sneak Attack puts a Test Bear in; Displacer Kitten (Mishra's Bauble
    // cast) flickers it: at the end step it isn't sacrificed.
    let mut g = scen()
        .battlefield(0, &["Sneak Attack", "Displacer Kitten", "Mountain"])
        .hand(0, &["Test Bear", "Mishra's Bauble"])
        .at(Step::Main1, 0)
        .start();
    let sa = bf(&g, 0, "Sneak Attack");
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(ab(sa, 0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(Choice::Card(bear)) => Some(Choice::Card(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    let mb = g.card(0, "Mishra's Bauble", Zone::Hand);
    g.choose(Choice::Cast(mb));
    resolve_at(&mut g, bear);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "a new object: not sacrificed\n{}", log(&g));
}

#[test]
fn questing_beasts_trigger_does_nothing_without_a_planeswalker_cr_603_3d() {
    // ruling: c026cdc4
    let mut g = scen().battlefield(0, &["Questing Beast"]).at(Step::DeclareAttackers, 0).start();
    let qb = bf(&g, 0, "Questing Beast");
    attack(&mut g, &[qb]);
    let asked = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g), |g| {
        if g.kind() == Some(DecisionKind::Target) {
            asked.set(true);
        }
        None
    });
    assert_eq!(g.life(1), 16);
    assert!(!asked.get(), "no planeswalker: no target, nothing\n{}", log(&g));
}

#[test]
fn fastbond_deals_damage_for_lands_played_not_put_onto_the_battlefield_cr_305_1() {
    // ruling: fbba272e 892e2be5
    // A land played, then Uro (cast) puts a Forest onto the battlefield: no
    // damage. A second land played from the graveyard with Crucible of
    // Worlds: 1 damage.
    let mut g = scen()
        .battlefield(0, &["Fastbond", "Crucible of Worlds", "Forest", "Island", "Island"])
        .hand(0, &["Island", "Uro, Titan of Nature's Wrath", "Forest"])
        .graveyard(0, &["Plains"])
        .at(Step::Main1, 0)
        .start();
    let i = g.card(0, "Island", Zone::Hand);
    g.choose(Choice::Play(i));
    resolve(&mut g);
    assert_eq!(g.life(0), 20);
    let uro = g.card(0, "Uro, Titan of Nature's Wrath", Zone::Hand);
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Cast(uro));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(Choice::Card(f)) => Some(Choice::Card(f)),
        _ => None,
    });
    assert_eq!(g.obj(f).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.life(0), 23, "Uro's 3 life; no Fastbond damage\n{}", log(&g));
    let pl = g.card(0, "Plains", Zone::Graveyard);
    g.choose(Choice::Play(pl));
    resolve(&mut g);
    assert_eq!(g.life(0), 22, "a second land played: 1 damage");
}

#[test]
fn walking_ballistas_damage_limits_its_removals_cr_704_5f_602_2() {
    // ruling: f730efa6
    // Ballista with three counters pings itself (1 damage); it removes a
    // second counter (1/1 with 1 damage): destroyed before the ability
    // resolves; no third activation.
    let mut g = scen().battlefield(0, &["Walking Ballista"]).counters(0, "Walking Ballista", Counter::P1P1, 3).at(Step::Main1, 0).start();
    let wb = bf(&g, 0, "Walking Ballista");
    g.choose(ab(wb, 1));
    resolve_at(&mut g, wb);
    assert_eq!(g.obj(wb).damage, 1, "{}", log(&g));
    assert_eq!(g.counters_on(wb, Counter::P1P1), 2);
    g.choose(ab(wb, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(at_priority);
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "1/1 with 1 damage\n{}", log(&g));
    assert!(!g.has_option(ab(wb, 1)));
    resolve(&mut g);
    assert_eq!(g.life(1), 19, "the second ping still resolves");
}

#[test]
fn candelabra_targets_untapped_lands_and_an_opponents_cr_115_1() {
    // ruling: 256006dc 9e0e545b
    let mut g = scen()
        .battlefield(0, &["Candelabra of Tawnos", "Island", "Island", "Island"])
        .battlefield(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, "Candelabra of Tawnos");
    let f = bf(&g, 1, "Forest");
    g.choose(ab(c, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    assert!(!g.obj(f).tapped);
    assert!(g.has_option(g.tgt(f)), "an untapped land of the opponent: {}", opts(&g));
    g.choose(g.tgt(f));
    pay(&mut g);
    resolve(&mut g);
    assert!(!g.obj(f).tapped);
}

#[test]
fn grindstone_doesnt_repeat_for_two_colorless_cards_cr_105_2c() {
    // ruling: ac790121
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Forest", "Island", "Lightning Bolt", "Lightning Bolt", "Forest"])
        .battlefield(0, &["Grindstone", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let gs = bf(&g, 0, "Grindstone");
    g.choose(ab(gs, 0));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert_eq!(g.player(1).graveyard.len(), 2, "two lands share no colour: once\n{}", log(&g));
}

#[test]
fn kaitos_zero_counts_an_opponent_who_lost_life_even_if_they_gained_more_cr_119_3() {
    // ruling: f36cad96
    // P1 gains 3 (Oust on its own Test Bear) after P0's Bolt dealt 3 earlier
    // this turn: P1 lost life this turn: Kaito's 0 draws a card.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Kaito, Bane of Nightmares", "Mountain", "Plains"])
        .hand(0, &["Lightning Bolt", "Oust"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    let bear = bf(&g, 1, "Test Bear");
    cast_at(&mut g, 0, "Oust", bear);
    assert_eq!(g.life(1), 20, "lost 3, gained 3");
    let k = bf(&g, 0, "Kaito, Bane of Nightmares");
    let hand = g.player(0).hand.len();
    g.choose(ab(k, 1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInGraveyard) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand + 1, "one opponent lost life\n{}", log(&g));
}

#[test]
fn a_modal_spells_modes_happen_in_printed_order_cr_700_2() {
    // ruling: ac9d14f8
    // Kolaghan's Command: return Test Bear to P0's hand, then P0 (the
    // target) discards a card: the Bear (its only card) is discarded.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .hand(0, &["Kolaghan's Command"])
        .graveyard(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.choose(Choice::Cast(kc));
    let tb = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(0)) => Some(Choice::Number(0)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        Some(DecisionKind::Target) => Some(player(0)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "returned, then discarded\n{}", log(&g));
    assert!(g.player(0).hand.is_empty());
}

#[test]
fn lurrus_stays_unused_when_another_permission_casts_from_the_graveyard_cr_601_2a() {
    // ruling: 5f5f59d3
    // Yawgmoth's Will: a Test Cat cast from the graveyard with the Will's
    // permission; Lurrus still casts a second one.
    let mut g = scen()
        .battlefield(0, &["Lurrus of the Dream-Den", "Swamp", "Swamp", "Swamp", "Plains", "Plains", "Plains", "Plains"])
        .hand(0, &["Yawgmoth's Will"])
        .graveyard(0, &["Test Cat", "Test Cat"])
        .at(Step::Main1, 0)
        .start();
    let yw = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(yw));
    resolve(&mut g);
    let c1 = g.card(0, "Test Cat", Zone::Graveyard);
    g.choose(Choice::Cast(c1));
    resolve_with(&mut g, |g| match g.kind() {
        // The Will's route (1), not Lurrus's (6).
        Some(DecisionKind::ChooseRoute) => Some(Choice::Number(1)),
        _ => None,
    });
    assert_eq!(g.obj(c1).zone, Zone::Battlefield, "{}", log(&g));
    // (Yawgmoth's Will gone: its turn permission stays; Lurrus's too.)
    let c2 = g.card(0, "Test Cat", Zone::Graveyard);
    g.choose(Choice::Cast(c2));
    let routes = std::cell::RefCell::new(String::new());
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseRoute) => {
            *routes.borrow_mut() = opts(g);
            Some(Choice::Number(6))
        }
        _ => None,
    });
    assert!(routes.borrow().contains("6"), "Lurrus's permission still unused: {}", routes.borrow());
    assert_eq!(g.obj(c2).zone, Zone::Battlefield);
}

#[test]
fn shelldock_isle_counts_any_library_cr_702_75a() {
    // ruling: 68db9ec9
    // P0's library has 30 cards, P1's 15: Shelldock Isle's hidden card is
    // played.
    let mut lib = vec!["Island", "Island", "Test Bear", "Island"];
    lib.extend(["Island"; 26]);
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &lib)
        .library(1, &["Forest"; 15])
        .battlefield(0, &["Island"])
        .hand(0, &["Shelldock Isle"])
        .at(Step::Main1, 0)
        .start();
    let isle = g.card(0, "Shelldock Isle", Zone::Hand);
    g.choose(Choice::Play(isle));
    let bear = g.card(0, "Test Bear", Zone::Library);
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.player(0).library.len() > 20 && g.player(1).library.len() <= 20);
    let isle = bf(&g, 0, "Shelldock Isle");
    assert!(g.has_option(ab(isle, 1)), "P1's library has twenty or fewer: {}", opts(&g));
    g.choose(ab(isle, 1));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
}

#[test]
fn thassas_oracle_with_devotion_zero_looks_at_nothing_and_wins_on_an_empty_library_cr_104_2b() {
    // ruling: cabf6b70
    // P0's library is empty; P1 Bolts the Oracle in response to its
    // trigger: devotion 0, nothing looked at, P0 wins.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Thassa's Oracle"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let o = g.card(0, "Thassa's Oracle", Zone::Hand);
    g.choose(Choice::Cast(o));
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1 && g.obj(o).zone == Zone::Battlefield && !g.stack().is_empty());
    cast_at(&mut g, 1, "Lightning Bolt", o);
    assert_eq!(g.obj(o).zone, Zone::Graveyard);
    assert_eq!(g.outcome().and_then(|x| x.winner), Some(0), "{}", log(&g));
}

#[test]
fn gut_made_an_artifact_may_sacrifice_itself_to_its_own_ability_cr_701_21a() {
    // ruling: 06cb2c7b
    // Phyrexian Metamorph copies Gut (an artifact in addition): attacking,
    // its "sacrifice another creature or an artifact" may sacrifice itself.
    let mut g = scen()
        .battlefield(0, &["Gut, True Soul Zealot", "Island", "Island", "Island", "Island"])
        .hand(0, &["Phyrexian Metamorph"])
        .at(Step::Main1, 0)
        .start();
    let gut = bf(&g, 0, "Gut, True Soul Zealot");
    let pm = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(pm));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayPhyrexian) => Some(Choice::Number(0)),
        // (The legend rule: P0 keeps the Metamorph copy.)
        Some(DecisionKind::LegendRule) => Some(Choice::Card(pm)),
        _ if g.has_option(Choice::Card(gut)) => Some(Choice::Card(gut)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(pm).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.is_type(pm, engine::db::types::ARTIFACT));
    // Next turn (no summoning sickness): it attacks; its trigger offers
    // itself.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.kind() == Some(DecisionKind::DeclareAttackers));
    attack(&mut g, &[pm]);
    let offered = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(Choice::Card(pm)) => {
            offered.set(true);
            Some(Choice::Card(pm))
        }
        _ => None,
    });
    assert!(offered.get(), "itself, an artifact\n{}", log(&g));
    assert_eq!(g.obj(pm).zone, Zone::Graveyard);
    assert_eq!(tokens_named(&g, "Skeleton").len(), 1, "{}", log(&g));
}
