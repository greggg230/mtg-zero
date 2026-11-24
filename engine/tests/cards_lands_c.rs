//! Lane C of the Legacy Lands cards (ENGINE.md 270.3) against their Oracle
//! text, the Comprehensive Rules and their official rulings (`// ruling:
//! <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card table
//! of constructed play only (`CardDb::constructed`).
//!
//! Pithing Needle {1}, Artifact: "As this artifact enters, choose a card name.
//! / Activated abilities of sources with the chosen name can't be activated
//! unless they're mana abilities."
//!
//! Cursed Totem {2}, Artifact: "Activated abilities of creatures can't be
//! activated."
//!
//! Torpor Orb {2}, Artifact: "Creatures entering don't cause abilities to
//! trigger."
//!
//! Damping Sphere {2}, Artifact: "If a land is tapped for two or more mana, it
//! produces {C} instead of any other type and amount. / Each spell a player
//! casts costs {1} more to cast for each other spell that player has cast this
//! turn."
//!
//! Choke {2}{G}, Enchantment: "Islands don't untap during their controllers'
//! untap steps."
//!
//! The Tabernacle at Pendrell Vale, Legendary Land: "All creatures have "At
//! the beginning of your upkeep, destroy this creature unless you pay {1}.""

use engine::db::{types, AbilitySlot, CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const NEEDLE: &str = "Pithing Needle";
const TOTEM: &str = "Cursed Totem";
const ORB: &str = "Torpor Orb";
const SPHERE: &str = "Damping Sphere";
const CHOKE: &str = "Choke";
const TAB: &str = "The Tabernacle at Pendrell Vale";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

/// No auto-answers; ten Islands in each library (each test may give its own).
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Answer the decisions until priority: `answer` first, else the first
/// option.
fn until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}\n{}", opts(g), log(g));
}

fn cast_with(g: &mut Game, p: PlayerId, name: &str, answer: &dyn Fn(&Game) -> Option<Choice>) {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    until_priority(g, answer);
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    cast_with(g, p, name, &|_| None);
}

/// Resolve the whole stack, answering with `answer` (else the first option).
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
    panic!("resolve_with: no progress\n{}", log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, &|_| None);
}

fn cast_and_resolve(g: &mut Game, p: PlayerId, name: &str) {
    cast(g, p, name);
    resolve(g);
}

fn abilities_offered(g: &Game, c: CardIdx) -> Vec<AbilitySlot> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Ability(x, s) if x == c => Some(s),
            _ => None,
        })
        .collect()
}

fn on_bf(g: &Game, p: PlayerId, name: &str) -> bool {
    g.find_card(p, name, Zone::Battlefield).is_some()
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

/// Float the mana of `c`'s mana ability (`ActivateMana` with priority, CR
/// 605.3a): its first mana ability unless `slot` names one.
fn float(g: &mut Game, c: CardIdx, slot: Option<AbilitySlot>) {
    assert!(at_priority(g));
    g.choose(Choice::Yes);
    assert_eq!(g.kind(), Some(DecisionKind::ActivateMana), "{}", opts(g));
    assert!(g.has_option(Choice::Card(c)), "{} offered: {}", g.name(c), opts(g));
    g.choose(Choice::Card(c));
    for _ in 0..50 {
        match g.kind() {
            Some(DecisionKind::ActivateMana) => {
                g.choose(Choice::Pass);
                break;
            }
            Some(DecisionKind::Priority) => break,
            _ => {
                let c = slot
                    .map(|s| Choice::Ability(c, s))
                    .filter(|&x| g.has_option(x))
                    .unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
                g.choose(c);
            }
        }
    }
    assert!(at_priority(g), "{}\n{}", opts(g), log(g));
}

/// Does `p` have the option to start floating mana with `c` (`ActivateMana`)?
fn can_float(g: &Game, c: CardIdx) -> bool {
    let mut h = g.clone();
    if !h.has_option(Choice::Yes) {
        return false;
    }
    h.choose(Choice::Yes);
    h.kind() == Some(DecisionKind::ActivateMana) && h.has_option(Choice::Card(c))
}

/// Pass priority (no attackers, no blockers) until `stop` holds; a decision
/// with one option is answered, `answer` answers the others (else panic).
fn pass_to_with(g: &mut Game, stop: impl Fn(&Game) -> bool, answer: &dyn Fn(&Game) -> Option<Choice>) {
    for _ in 0..10_000 {
        if stop(g) || g.is_over() {
            return;
        }
        if let Some(c) = answer(g) {
            g.choose(c);
            continue;
        }
        if g.has_option(Choice::Pass) {
            g.pass();
            continue;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        assert_eq!(d.options.len(), 1, "a real decision: {}\n{}", opts(g), log(g));
        let c = d.options[0].choice;
        g.choose(c);
    }
    panic!("pass_to: no progress\n{}", log(g));
}

fn pass_to(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    pass_to_with(g, stop, &|_| None);
}

/// Player `p`'s next turn's precombat main phase, at priority.
fn to_main(g: &mut Game, p: PlayerId) {
    let n = g.turn().number;
    pass_to(g, |g| g.turn().number > n && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g));
}

/// Cast a Pithing Needle and name `name` (`None`: `PASS`, a name no object
/// of the game can have).
fn needle_naming(g: &mut Game, p: PlayerId, name: Option<&str>) {
    cast(g, p, NEEDLE);
    g.pass_until(|g| g.stack().is_empty() || g.kind() == Some(DecisionKind::NameCard));
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}\n{}", opts(g), log(g));
    match name {
        Some(n) => g.choose(Choice::Def(def(n))),
        None => g.pass(),
    };
    resolve(g);
    assert!(on_bf(g, p, NEEDLE));
}

// ---- the definitions -------------------------------------------------------------

#[test]
fn the_six_cards_are_defined_as_printed() {
    let d = db();
    for (n, mv, ty) in [(NEEDLE, 1, types::ARTIFACT), (TOTEM, 2, types::ARTIFACT), (ORB, 2, types::ARTIFACT), (SPHERE, 2, types::ARTIFACT), (CHOKE, 3, types::ENCHANTMENT), (TAB, 0, types::LAND)] {
        let c = d.def(def(n));
        assert_eq!((c.types, c.mana_value()), (ty, mv), "{n}");
        assert!(c.activated.is_empty(), "{n}: no activated ability (the Tabernacle has no mana ability either)");
    }
    assert_eq!(d.def(def(CHOKE)).colors, 1 << engine::mana::G);
    assert!(d.def(def(NEEDLE)).chooses_name && d.def(def(NEEDLE)).sns.plain_name && d.def(def(NEEDLE)).sns.named_ban == Some(true));
    assert_eq!(d.def(def(NEEDLE)).named_spell_tax, 0, "no tax, unlike Disruptor Flute");
    assert_eq!(d.def(def(TOTEM)).locks_activated, types::CREATURE);
    let t = d.def(def(TAB));
    assert!(t.supertypes & engine::db::supertypes::LEGENDARY != 0);
    assert_eq!(t.lc.granted_to_creatures, vec![0]);
    assert_eq!(t.triggers[0].event, engine::db::EventKind::BeginningOfUpkeep);
    assert!(d.def(def(SPHERE)).lc.damp_lands && d.def(def(SPHERE)).lc.tax_per_other_spell == 1);
    assert!(d.def(def(ORB)).lc.entering_silent.is_some() && d.def(def(CHOKE)).lc.no_untap.is_some());
}

// ---- Pithing Needle ------------------------------------------------------------------

/// "As this artifact enters, choose a card name" (CR 201.3, 614.12): any
/// card's name, a card without activated abilities too (Lightning Bolt); a
/// token's only when it's a card's (Faerie Dragon, not the Construct Urza's
/// Saga makes); `PASS` for any other name.
#[test]
fn cr_201_3_pithing_needle_names_any_card_not_a_token_name() {
    // ruling: 194bf700
    let mut g = scen()
        .battlefield(0, &["Island", "Urza's Saga"])
        .hand(0, &[NEEDLE, "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    resolve(&mut g); // (Urza's Saga's chapter I, as it entered)
    cast(&mut g, 0, NEEDLE);
    g.pass_until(|g| g.kind() == Some(DecisionKind::NameCard));
    let names: Vec<&str> = g
        .offered()
        .into_iter()
        .filter_map(|c| match c {
            Choice::Def(d) => Some(engine::sneak_show::face_name(db().def(d))),
            _ => None,
        })
        .collect();
    for n in ["Lightning Bolt", "Urza's Saga", NEEDLE, "Island", "Faerie Dragon"] {
        assert!(names.contains(&n), "{n} among {names:?}");
    }
    assert!(!names.contains(&"Construct"), "a token name that is no card's");
    assert!(g.has_option(Choice::Pass));
    g.choose(Choice::Def(def("Lightning Bolt")));
    resolve(&mut g);
    let n = bf(&g, 0, NEEDLE);
    assert!(g.marks_of(n).contains(&engine::game::Mark::ChosenName(def("Lightning Bolt"))), "{:?}", g.marks_of(n));
}

/// Naming Karakas: its "{T}: Return target legendary creature" can't be
/// activated; its mana ability can (CR 602.5, 605.1a).
#[test]
fn cr_602_5_pithing_needle_stops_a_non_mana_ability_not_the_mana_ability() {
    // ruling: 044069c9
    let mut g = scen()
        .battlefield(0, &["Island", "Karakas"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .hand(0, &[NEEDLE, "Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Karakas");
    assert!(!abilities_offered(&g, k).is_empty(), "before: {}", opts(&g));
    needle_naming(&mut g, 0, Some("Karakas"));
    assert!(abilities_offered(&g, k).is_empty(), "named: {}", opts(&g));
    // Its mana still pays (untapped again: the payment may have used it).
    g.set_tapped(k, false);
    g.refresh_decision();
    assert!(can_float(&g, k), "Karakas's mana ability: {}", opts(&g));
    float(&mut g, k, None);
    assert_eq!(g.player(0).pool.amt[engine::mana::W], 1);
}

/// Pithing Needle affects every zone (its ruling): channel (Otawara's, from
/// the hand) and cycling (Street Wraith's) can't be activated; Otawara's mana
/// ability on the battlefield can.
#[test]
fn cr_602_5_pithing_needle_stops_abilities_in_the_hand_channel_and_cycling() {
    // ruling: 7e0da1dd
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Otawara, Soaring City"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .hand(0, &[NEEDLE, "Otawara, Soaring City", "Street Wraith"])
        .at(Step::Main1, 0)
        .start();
    let oh = hand(&g, 0, "Otawara, Soaring City");
    let sw = hand(&g, 0, "Street Wraith");
    assert!(!abilities_offered(&g, oh).is_empty() && !abilities_offered(&g, sw).is_empty(), "before: {}", opts(&g));
    needle_naming(&mut g, 0, Some("Otawara, Soaring City"));
    assert!(abilities_offered(&g, oh).is_empty(), "channel: {}", opts(&g));
    assert!(!abilities_offered(&g, sw).is_empty(), "Street Wraith not named: {}", opts(&g));
    let ob = bf(&g, 0, "Otawara, Soaring City");
    assert!(can_float(&g, ob), "the battlefield Otawara's mana: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &[NEEDLE, "Street Wraith"]).at(Step::Main1, 0).start();
    needle_naming(&mut g, 0, Some("Street Wraith"));
    let sw = hand(&g, 0, "Street Wraith");
    assert!(abilities_offered(&g, sw).is_empty(), "cycling: {}", opts(&g));
}

/// Once Pithing Needle has left the battlefield, the named source's
/// abilities can be activated again (CR 611.3a: a static ability's effect
/// lasts as long as its source is there).
#[test]
fn cr_611_3a_pithing_needle_gone_the_abilities_come_back() {
    // ruling: 85cc0060
    let mut g = scen()
        .battlefield(0, &["Island", "Karakas"])
        .battlefield(1, &["Forest", "Forest", "Forest", "Forest", "Forest", "Thalia, Guardian of Thraben"])
        .hand(0, &[NEEDLE])
        .hand(1, &["Force of Vigor"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Karakas");
    needle_naming(&mut g, 0, Some("Karakas"));
    assert!(abilities_offered(&g, k).is_empty());
    to_main(&mut g, 1);
    let n = bf(&g, 0, NEEDLE);
    let t = g.tgt(n);
    cast_with(&mut g, 1, "Force of Vigor", &|g| g.has_option(t).then_some(t));
    resolve_with(&mut g, &|g| g.has_option(Choice::Pass).then_some(Choice::Pass).filter(|_| g.kind() == Some(DecisionKind::Target)));
    assert!(!on_bf(&g, 0, NEEDLE), "{}", log(&g));
    to_main(&mut g, 0);
    assert!(!abilities_offered(&g, k).is_empty(), "Karakas works again: {}", opts(&g));
}

/// Triggered and static abilities of the named card work normally (only
/// activated abilities are stopped; "[cost]: [effect]" is an activated
/// ability): Urza's Saga named keeps its chapter abilities (a trigger) and
/// gains "{T}: Add {C}" (a mana ability, usable), while its chapter II
/// ability "{2}, {T}: Create a Construct" can't be activated.
#[test]
fn cr_602_1_pithing_needle_leaves_triggered_and_static_abilities() {
    // ruling: 258063a6
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Urza's Saga"])
        .hand(0, &[NEEDLE])
        .at(Step::Main1, 0)
        .start();
    resolve(&mut g);
    needle_naming(&mut g, 0, Some("Urza's Saga"));
    to_main(&mut g, 0);
    let s = bf(&g, 0, "Urza's Saga");
    assert_eq!(g.counters_on(s, Counter::Lore), 2, "the lore counter (a turn-based action) and chapter II: {}", log(&g));
    assert!(abilities_offered(&g, s).is_empty(), "the Construct ability can't be activated: {}", opts(&g));
    assert!(can_float(&g, s), "its {{T}}: Add {{C}}");
    // Thalia's static ability (named) still taxes.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Thalia, Guardian of Thraben"])
        .hand(0, &[NEEDLE, "Ponder"])
        .at(Step::Main1, 0)
        .start();
    needle_naming(&mut g, 0, Some("Thalia, Guardian of Thraben"));
    let p = hand(&g, 0, "Ponder");
    assert!(!g.has_option(Choice::Cast(p)), "Ponder costs {{1}}{{U}} with one land left: {}", opts(&g));
}

/// A loyalty ability is an activated ability (CR 606.3): naming a
/// planeswalker stops them all.
#[test]
fn cr_606_3_pithing_needle_stops_loyalty_abilities() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Nissa, Who Shakes the World", "Forest"])
        .hand(0, &[NEEDLE])
        .at(Step::Main1, 0)
        .start();
    needle_naming(&mut g, 0, Some("Nissa, Who Shakes the World"));
    to_main(&mut g, 1);
    let n = bf(&g, 1, "Nissa, Who Shakes the World");
    assert!(abilities_offered(&g, n).is_empty(), "{}", opts(&g));
    // Nissa's static ability still works: a Forest makes {G}{G}.
    let f = bf(&g, 1, "Forest");
    float(&mut g, f, None);
    assert_eq!(g.player(1).pool.amt[engine::mana::G], 2);
}

// ---- Cursed Totem -------------------------------------------------------------------

/// "Activated abilities of creatures can't be activated" (CR 602.5): mana
/// abilities too (Birds of Paradise can't pay; its ruling); a land creature's
/// (Dryad Arbor's) too; a land's (Karakas) and an Equipment's equip
/// (Skullclamp, an ability of the Equipment) aren't creatures' abilities.
#[test]
fn cr_602_5_cursed_totem_stops_creatures_mana_abilities() {
    // ruling: f4c6dda8
    // ruling: 7340e3ca
    let mut g = scen()
        .battlefield(0, &["Birds of Paradise", "Dryad Arbor", TOTEM])
        .hand(0, &["Lightning Bolt", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    for n in ["Lightning Bolt", "Ponder"] {
        let c = hand(&g, 0, n);
        assert!(!g.has_option(Choice::Cast(c)), "{n}: no mana from creatures: {}", opts(&g));
    }
    let birds = bf(&g, 0, "Birds of Paradise");
    assert!(!can_float(&g, birds));
    // Without the Totem: both pay.
    let g2 = scen().battlefield(0, &["Birds of Paradise", "Dryad Arbor"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    assert!(g2.has_option(Choice::Cast(hand(&g2, 0, "Lightning Bolt"))));
    // A land's ability and an Equipment's equip are not creatures'.
    let mut g = scen()
        .battlefield(0, &["Karakas", "Plains", "Plains", "Skullclamp", "Guide of Souls", TOTEM])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Karakas");
    let clamp = bf(&g, 0, "Skullclamp");
    assert!(!abilities_offered(&g, k).is_empty(), "Karakas: {}", opts(&g));
    assert!(!abilities_offered(&g, clamp).is_empty(), "equip: {}", opts(&g));
    g.choose(Choice::Ability(clamp, abilities_offered(&g, clamp)[0]));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    assert!(g.obj(clamp).attached.is_some(), "{}", log(&g));
}

/// Abilities of creature cards that aren't on the battlefield aren't
/// creatures' (CR 109.2, 302.1): cycling (Street Wraith in the hand) and a
/// Spirit Guide's "exile this card from your hand: add {R}" still work.
#[test]
fn cr_302_1_cursed_totem_leaves_creature_cards_in_other_zones() {
    // ruling: 6e863b0a
    let mut g = scen()
        .battlefield(0, &[TOTEM])
        .hand(0, &["Street Wraith", "Simian Spirit Guide", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let sw = hand(&g, 0, "Street Wraith");
    assert!(!abilities_offered(&g, sw).is_empty(), "cycling: {}", opts(&g));
    let bolt = hand(&g, 0, "Lightning Bolt");
    assert!(g.has_option(Choice::Cast(bolt)), "the Spirit Guide's mana: {}", opts(&g));
}

/// A Vehicle's crew is an ability of a noncreature permanent: it can be
/// activated; once the Vehicle is a creature, its abilities (crew again)
/// can't be (CR 702.122a, 301.7b).
#[test]
fn cr_702_122a_cursed_totem_lets_a_vehicle_crew_then_stops_it() {
    // ruling: ec03f345
    let mut g = scen()
        .battlefield(0, &["Smuggler's Copter", "Thalia, Guardian of Thraben", "Birds of Paradise", TOTEM])
        .at(Step::Main1, 0)
        .start();
    let cop = bf(&g, 0, "Smuggler's Copter");
    let crew = abilities_offered(&g, cop);
    assert_eq!(crew.len(), 1, "crew: {}", opts(&g));
    g.choose(Choice::Ability(cop, crew[0]));
    let th = bf(&g, 0, "Thalia, Guardian of Thraben");
    until_priority(&mut g, &|g| g.has_option(Choice::Card(th)).then_some(Choice::Card(th)));
    resolve(&mut g);
    assert!(g.is_creature(cop), "{}", log(&g));
    assert!(abilities_offered(&g, cop).is_empty(), "a creature now: {}", opts(&g));
}

/// An animated land is a creature (Nissa's +1): its mana ability can't be
/// activated under Cursed Totem; the Totem gone, it can.
#[test]
fn cr_602_5_cursed_totem_stops_an_animated_lands_mana_ability() {
    let mut g = scen()
        .battlefield(0, &["Nissa, Who Shakes the World", "Forest", TOTEM])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let nissa = bf(&g, 0, "Nissa, Who Shakes the World");
    let f = bf(&g, 0, "Forest");
    let plus = abilities_offered(&g, nissa)[0];
    g.choose(Choice::Ability(nissa, plus));
    let t = g.tgt(f);
    until_priority(&mut g, &|g| g.has_option(t).then_some(t));
    resolve(&mut g);
    assert!(g.is_creature(f), "{}", log(&g));
    assert!(!can_float(&g, f), "a creature's mana ability: {}", opts(&g));
}

/// Static and triggered abilities of creatures still work (CR 603.2a:
/// "effects that preclude abilities from being activated don't affect"
/// triggered abilities): Thalia's tax, Guide of Souls' trigger.
#[test]
fn cr_603_2a_cursed_totem_leaves_static_and_triggered_abilities() {
    // ruling: e974070e
    let mut g = scen()
        .battlefield(0, &["Plains", "Thalia, Guardian of Thraben", "Guide of Souls", TOTEM])
        .hand(0, &["Llanowar Elves", "Ponder"])
        .battlefield(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let life = g.player(0).life;
    cast_and_resolve(&mut g, 0, "Llanowar Elves");
    assert_eq!(g.player(0).life, life + 1, "Guide of Souls: {}", log(&g));
    let p = hand(&g, 0, "Ponder");
    assert!(!g.has_option(Choice::Cast(p)), "Thalia's tax, one land left: {}", opts(&g));
}

// ---- Torpor Orb ----------------------------------------------------------------------

/// "Creatures entering don't cause abilities to trigger" (CR 603.2, 603.6a):
/// a creature's own enters-the-battlefield ability (Archon of Cruelty's
/// "enters or attacks": not as it enters) and another permanent's (Guide of
/// Souls' "whenever another creature you control enters") don't trigger.
#[test]
fn cr_603_6a_torpor_orb_stops_a_creatures_own_and_others_enter_triggers() {
    // ruling: cf1f1651
    // ruling: b2cc8d3a
    let start = |orb: bool| {
        let mut b = scen()
            .battlefield(0, &["Guide of Souls"])
            .battlefield(0, &["Swamp"; 8])
            .hand(0, &["Archon of Cruelty"])
            .hand(1, &["Island", "Island"])
            .battlefield(1, &["Thalia, Guardian of Thraben"])
            .at(Step::Main1, 0);
        if orb {
            b = b.battlefield(0, &[ORB]);
        }
        b.start()
    };
    let mut g = start(true);
    let (life0, life1, hand1) = (g.player(0).life, g.player(1).life, g.player(1).hand.len());
    cast(&mut g, 0, "Archon of Cruelty");
    resolve(&mut g);
    assert!(on_bf(&g, 0, "Archon of Cruelty"));
    assert_eq!((g.player(0).life, g.player(1).life, g.player(1).hand.len()), (life0, life1, hand1), "no trigger: {}", log(&g));
    assert!(on_bf(&g, 1, "Thalia, Guardian of Thraben"));
    // Without the Orb both trigger.
    let mut g = start(false);
    cast(&mut g, 0, "Archon of Cruelty");
    resolve(&mut g);
    assert_eq!(g.player(0).life, life0 + 1 + 3, "Guide +1, Archon +3: {}", log(&g));
    assert!(!on_bf(&g, 1, "Thalia, Guardian of Thraben"));
}

/// The trigger event needn't say "creature": an artifact creature entering
/// doesn't trigger Kappa Cannoneer's "another artifact you control enters", a
/// noncreature artifact does; a land creature (Dryad Arbor) entering doesn't
/// trigger landfall (Lotus Cobra).
#[test]
fn cr_603_6a_torpor_orb_stops_any_ability_a_creature_entering_triggers() {
    // ruling: d1f1a438
    let mut g = scen()
        .battlefield(0, &[ORB, "Kappa Cannoneer", "Lotus Cobra"])
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Walking Ballista", "Sol Ring", "Dryad Arbor"])
        .at(Step::Main1, 0)
        .start();
    let kc = bf(&g, 0, "Kappa Cannoneer");
    cast_with(&mut g, 0, "Walking Ballista", &|g| g.has_option(Choice::Number(1)).then_some(Choice::Number(1)));
    resolve(&mut g);
    assert!(on_bf(&g, 0, "Walking Ballista"));
    assert_eq!(g.counters_on(kc, Counter::P1P1), 0, "an artifact creature: {}", log(&g));
    cast_and_resolve(&mut g, 0, "Sol Ring");
    assert_eq!(g.counters_on(kc, Counter::P1P1), 1, "a noncreature artifact: {}", log(&g));
    let pool = g.player(0).pool.total();
    let a = hand(&g, 0, "Dryad Arbor");
    g.choose(Choice::Play(a));
    until_priority(&mut g, &|_| None);
    assert!(g.stack().is_empty() && g.player(0).pool.total() == pool, "no landfall for a land creature: {}", log(&g));
    // Without the Orb: the Cobra's landfall triggers.
    let mut g = scen().battlefield(0, &["Lotus Cobra"]).hand(0, &["Dryad Arbor"]).at(Step::Main1, 0).start();
    let a = hand(&g, 0, "Dryad Arbor");
    g.choose(Choice::Play(a));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.stack().len(), 1, "{}", log(&g));
}

/// A permanent is looked at as it exists on the battlefield, with continuous
/// effects (CR 603.6b, 707.2): Phyrexian Metamorph entering as a copy of Sol
/// Ring is a noncreature artifact and triggers Kappa Cannoneer; as a copy of
/// a creature it doesn't.
#[test]
fn cr_603_6b_torpor_orb_looks_at_the_permanent_as_it_entered() {
    // ruling: 0ac2f0bc
    for (copy, triggers) in [("Sol Ring", true), ("Guide of Souls", false)] {
        let mut g = scen()
            .battlefield(0, &[ORB, "Kappa Cannoneer", "Sol Ring", "Guide of Souls"])
            .battlefield(0, &["Island"; 4])
            .hand(0, &["Phyrexian Metamorph"])
            .at(Step::Main1, 0)
            .start();
        let kc = bf(&g, 0, "Kappa Cannoneer");
        let src = bf(&g, 0, copy);
        cast(&mut g, 0, "Phyrexian Metamorph");
        resolve_with(&mut g, &|g| {
            if g.has_option(Choice::Card(src)) {
                Some(Choice::Card(src))
            } else if g.kind() == Some(DecisionKind::YesNo) {
                Some(Choice::Yes)
            } else {
                None
            }
        });
        let copies = g.battlefield().iter().filter(|&&c| g.name(c) == copy).count();
        assert_eq!(copies, 2, "{}", log(&g));
        assert_eq!(g.counters_on(kc, Counter::P1P1) == 1, triggers, "a copy of {copy}: {}", log(&g));
    }
}

/// Replacement effects and "as this enters" abilities aren't triggered
/// abilities (CR 614.1c, 614.12): Walking Ballista still enters with its X
/// +1/+1 counters; Anointed Peacekeeper still looks and names.
#[test]
fn cr_614_12_torpor_orb_leaves_replacement_effects_and_as_enters() {
    // ruling: d73b5f20
    // ruling: 0e210050
    let mut g = scen().battlefield(0, &[ORB]).battlefield(0, &["Plains"; 4]).hand(0, &["Walking Ballista"]).at(Step::Main1, 0).start();
    cast_with(&mut g, 0, "Walking Ballista", &|g| g.has_option(Choice::Number(2)).then_some(Choice::Number(2)));
    resolve(&mut g);
    assert_eq!(g.counters_on(bf(&g, 0, "Walking Ballista"), Counter::P1P1), 2, "{}", log(&g));
    let mut g = scen().battlefield(0, &[ORB]).battlefield(0, &["Plains"; 3]).hand(0, &["Anointed Peacekeeper"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Anointed Peacekeeper");
    g.pass_until(|g| g.kind() == Some(DecisionKind::NameCard) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "the as-enters choice: {}\n{}", opts(&g), log(&g));
    g.choose(Choice::Def(def("Lightning Bolt")));
    resolve(&mut g);
    let pk = bf(&g, 0, "Anointed Peacekeeper");
    assert!(g.marks_of(pk).contains(&engine::game::Mark::ChosenName(def("Lightning Bolt"))));
}

/// Torpor Orb and a creature entering at the same time: the creature
/// entering triggers nothing (CR 603.6a: the entry's events are matched once
/// all of it is on the battlefield). Show and Tell: P0's Torpor Orb, P1's
/// Archon of Cruelty.
#[test]
fn cr_603_6a_torpor_orb_entering_with_a_creature_stops_its_trigger() {
    // ruling: d6538b6e
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Show and Tell", ORB, "Island"])
        .hand(1, &["Archon of Cruelty", "Island"])
        .at(Step::Main1, 0)
        .start();
    let (life0, life1) = (g.player(0).life, g.player(1).life);
    let orb = hand(&g, 0, ORB);
    let archon = hand(&g, 1, "Archon of Cruelty");
    cast(&mut g, 0, "Show and Tell");
    resolve_with(&mut g, &|g| {
        if g.has_option(Choice::Card(orb)) {
            Some(Choice::Card(orb))
        } else if g.has_option(Choice::Card(archon)) {
            Some(Choice::Card(archon))
        } else {
            None
        }
    });
    assert!(on_bf(&g, 0, ORB) && on_bf(&g, 1, "Archon of Cruelty"), "{}", log(&g));
    assert_eq!((g.player(0).life, g.player(1).life), (life0, life1), "no Archon trigger: {}", log(&g));
}

/// Events other than the entering still trigger (ENGINE.md 270.3's reading,
/// with Hushbringer's rulings: an ability triggering on another event that
/// comes with a creature's entering - playing a land, creating a token -
/// still triggers): Dryad Arbor played makes City of Traitors sacrifice
/// itself; Staff of the Storyteller's own enters ability (an artifact's)
/// makes a Spirit token, its "whenever you create one or more creature
/// tokens" triggers, Guide of Souls' "another creature you control enters"
/// doesn't.
#[test]
fn cr_603_2_torpor_orb_leaves_other_events() {
    let mut g = scen()
        .battlefield(0, &[ORB, "City of Traitors", "Guide of Souls", "Plains", "Plains"])
        .hand(0, &["Dryad Arbor", "Staff of the Storyteller"])
        .at(Step::Main1, 0)
        .start();
    let life = g.player(0).life;
    let a = hand(&g, 0, "Dryad Arbor");
    g.choose(Choice::Play(a));
    resolve(&mut g);
    assert!(!on_bf(&g, 0, "City of Traitors"), "a land played: {}", log(&g));
    cast_and_resolve(&mut g, 0, "Staff of the Storyteller");
    let st = bf(&g, 0, "Staff of the Storyteller");
    assert!(g.battlefield().iter().any(|&c| g.name(c) == "Spirit"), "{}", log(&g));
    let story = Counter::Named(db().subtype_id("counter:story").expect("the story counter"));
    assert_eq!(g.counters_on(st, story), 1, "a token created: {}", log(&g));
    assert_eq!(g.player(0).life, life, "no Guide of Souls trigger: {}", log(&g));
}

/// A creature's dies and "at the beginning of" abilities are unaffected:
/// Stitcher's Supplier entering mills nothing; dying, it mills three.
#[test]
fn cr_603_6c_torpor_orb_leaves_dies_triggers() {
    let mut g = scen()
        .library(0, &["Island", "Forest", "Plains", "Swamp", "Mountain"])
        .battlefield(0, &[ORB, "Swamp"])
        .hand(0, &["Stitcher's Supplier"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Stitcher's Supplier");
    assert!(gy(&g, 0).is_empty(), "no enters trigger: {}", log(&g));
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 1));
    let s = bf(&g, 0, "Stitcher's Supplier");
    let t = g.tgt(s);
    cast_with(&mut g, 1, "Lightning Bolt", &|g| g.has_option(t).then_some(t));
    resolve(&mut g);
    assert_eq!(gy(&g, 0).len(), 4, "the Supplier and three milled: {}", log(&g));
}

// ---- Damping Sphere -------------------------------------------------------------------

/// "If a land is tapped for two or more mana, it produces {C} instead of any
/// other type and amount" (CR 106.12b, 614.1a): Ancient Tomb makes {C} (and
/// still deals its 2 damage: the rest of the ability is unchanged); a land
/// tapped for one mana is unaffected.
#[test]
fn cr_106_12b_damping_sphere_ancient_tomb_makes_one_colorless() {
    let mut g = scen().battlefield(0, &[SPHERE, "Ancient Tomb", "Island"]).at(Step::Main1, 0).start();
    let life = g.player(0).life;
    let t = bf(&g, 0, "Ancient Tomb");
    float(&mut g, t, None);
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 0, 1], "{}", log(&g));
    assert_eq!(g.player(0).life, life - 2);
    let i = bf(&g, 0, "Island");
    float(&mut g, i, None);
    assert_eq!(g.player(0).pool.amt, [0, 1, 0, 0, 0, 1]);
    // Without the Sphere: {C}{C}.
    let mut g = scen().battlefield(0, &["Ancient Tomb"]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, "Ancient Tomb");
    float(&mut g, t, None);
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 0, 2]);
}

/// The replacement in a payment (CR 601.2g): with Ancient Tomb its only
/// source, a {2} spell can't be cast; Ancient Tomb and an Island cast it.
#[test]
fn cr_601_2g_damping_sphere_ancient_tomb_pays_one() {
    let g = scen().battlefield(0, &[SPHERE, "Ancient Tomb"]).hand(0, &["Sol Ring", "Ponder"]).battlefield(1, &[]).at(Step::Main1, 0).start();
    let sol = hand(&g, 0, "Sol Ring");
    assert!(g.has_option(Choice::Cast(sol)), "{{1}}: {}", opts(&g));
    let g = scen().battlefield(0, &[SPHERE, "Ancient Tomb"]).hand(0, &["Smuggler's Copter"]).at(Step::Main1, 0).start();
    let cop = hand(&g, 0, "Smuggler's Copter");
    assert!(!g.has_option(Choice::Cast(cop)), "{{2}} from one {{C}}: {}", opts(&g));
    let mut g = scen().battlefield(0, &[SPHERE, "Ancient Tomb", "Island"]).hand(0, &["Smuggler's Copter"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Smuggler's Copter");
    assert!(on_bf(&g, 0, "Smuggler's Copter"));
    assert!(g.lands_of(0).all(|c| g.obj(c).tapped));
}

/// "Any other type and amount": a choice of two mana (Sunken Ruins' {U}{U},
/// {U}{B} or {B}{B}) is one {C}; a count (Gaea's Cradle) of two or more is one
/// {C}, of one is unchanged; restrictions on spending stay (Mishra's
/// Workshop: its {C} only for artifact spells, CR 106.6).
#[test]
fn cr_106_12b_damping_sphere_any_other_type_and_amount() {
    let mut g = scen().battlefield(0, &[SPHERE, "Sunken Ruins", "Island"]).at(Step::Main1, 0).start();
    let r = bf(&g, 0, "Sunken Ruins");
    g.choose(Choice::Yes);
    g.choose(Choice::Card(r));
    // Its two abilities: {T}: Add {C}, and the filter: both make one {C}.
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ActivateMana) => Some(Choice::Pass),
        Some(DecisionKind::ChooseMode) => g.offered().into_iter().last(),
        _ => None,
    });
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 0, 1], "{}", log(&g));
    for (creatures, want) in [(1usize, [0, 0, 0, 0, 1, 0]), (2, [0, 0, 0, 0, 0, 1]), (3, [0, 0, 0, 0, 0, 1])] {
        let mut g = scen().battlefield(0, &[SPHERE, "Gaea's Cradle"]).battlefield(0, &vec!["Birds of Paradise"; creatures]).at(Step::Main1, 0).start();
        let c = bf(&g, 0, "Gaea's Cradle");
        float(&mut g, c, None);
        assert_eq!(g.player(0).pool.amt, want, "{creatures} creatures");
    }
    let mut g = scen().battlefield(0, &[SPHERE, "Mishra's Workshop"]).hand(0, &["Ponder"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, "Mishra's Workshop");
    float(&mut g, w, None);
    assert_eq!((g.player(0).pool.amt, g.player(0).pool.art), ([0, 0, 0, 0, 0, 1], 1), "restricted {{C}}");
}

/// Two Damping Spheres: one applies, the production is then one mana and the
/// other no longer applies (CR 616.1f; a replacement effect can't apply to
/// the same event twice). A triggered mana ability's "additional" mana is its
/// own production, not the land's (CR 605.1b, 106.12a): Nissa's Forest makes
/// {G}{G} (the Forest's one {G}, Nissa's {G}).
#[test]
fn cr_616_1f_damping_sphere_two_spheres_and_additional_mana() {
    // ruling: 73f3a5a5
    let mut g = scen().battlefield(0, &[SPHERE, "Ancient Tomb"]).battlefield(1, &[SPHERE]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, "Ancient Tomb");
    float(&mut g, t, None);
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 0, 1]);
    let mut g = scen().battlefield(0, &[SPHERE, "Nissa, Who Shakes the World", "Forest"]).at(Step::Main1, 0).start();
    let f = bf(&g, 0, "Forest");
    float(&mut g, f, None);
    assert_eq!(g.player(0).pool.amt, [0, 0, 0, 0, 2, 0], "{}", log(&g));
}

/// "Each spell a player casts costs {1} more to cast for each other spell
/// that player has cast this turn" (CR 601.2f): the first spell of a turn
/// costs its cost, the second {1} more; the other player's spells don't
/// count; the spells cast before the Sphere was there count (its ruling: the
/// Sphere as the third spell, the next costs {3} more).
#[test]
fn cr_601_2f_damping_sphere_taxes_each_other_spell_this_turn() {
    // ruling: 0e1c9f8a
    let mut g = scen()
        .battlefield(0, &["Island"; 7])
        .hand(0, &["Ponder", "Brainstorm", SPHERE, "Lightning Bolt"])
        .battlefield(0, &["Mountain"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Ponder");
    cast_and_resolve(&mut g, 0, "Brainstorm");
    cast_and_resolve(&mut g, 0, SPHERE);
    let untapped = || g.lands_of(0).filter(|&c| !g.obj(c).tapped).count();
    assert_eq!(untapped(), 8 - 1 - 1 - 2);
    // Lightning Bolt: {R} + {3} (three other spells this turn): four lands.
    let bolt = hand(&g, 0, "Lightning Bolt");
    assert!(g.has_option(Choice::Cast(bolt)));
    let p1 = engine::Choice::Target(engine::Target::Player(1));
    cast_with(&mut g, 0, "Lightning Bolt", &|g| g.has_option(p1).then_some(p1));
    assert_eq!(g.lands_of(0).filter(|&c| !g.obj(c).tapped).count(), 0, "{}", log(&g));
    // The other player's first spell this turn costs its cost.
    let mut g = scen()
        .battlefield(0, &[SPHERE, "Island", "Island"])
        .hand(0, &["Ponder", "Brainstorm"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Ponder");
    let bs = hand(&g, 0, "Brainstorm");
    assert!(!g.has_option(Choice::Cast(bs)), "{{1}}{{U}}, one land left: {}", opts(&g));
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 1));
    let bolt = hand(&g, 1, "Lightning Bolt");
    assert!(g.has_option(Choice::Cast(bolt)), "P1's first spell: {}", opts(&g));
}

/// The total cost starts from the mana cost or the alternative cost, adds
/// the increases, then applies the reductions; the mana value is the mana
/// cost's (CR 601.2f, 202.3): Force of Will's alternative cost as the second
/// spell of the turn needs {1} too; on the stack its mana value is 5.
#[test]
fn cr_601_2f_damping_sphere_taxes_an_alternative_cost() {
    // ruling: b6f2bdbe
    let start = |lands: usize| {
        scen()
            .battlefield(0, &[SPHERE, "Island"])
            .battlefield(0, &vec!["Island"; lands])
            .hand(0, &["Ponder", "Force of Will", "Brainstorm"])
            .at(Step::Main1, 0)
            .start()
    };
    for (lands, castable) in [(0usize, false), (1, true)] {
        let mut g = start(lands);
        cast(&mut g, 0, "Ponder");
        let fow = hand(&g, 0, "Force of Will");
        assert_eq!(g.has_option(Choice::Cast(fow)), castable, "{lands} land(s) left: {}", opts(&g));
        if castable {
            g.choose(Choice::Cast(fow));
            until_priority(&mut g, &|g| {
                // The alternative cost (exile Brainstorm, 1 life), then the {1}.
                let bs = g.find_card(0, "Brainstorm", Zone::Hand)?;
                g.has_option(Choice::Card(bs)).then_some(Choice::Card(bs)).or_else(|| g.offered().into_iter().find(|c| !matches!(c, Choice::Pass)))
            });
            let f = g.card(0, "Force of Will", Zone::Stack);
            assert_eq!(g.mana_value_now(f), 5);
            assert!(g.find_card(0, "Brainstorm", Zone::Exile).is_some(), "{}", log(&g));
            assert!(g.lands_of(0).all(|c| g.obj(c).tapped), "the {{1}} paid: {}", log(&g));
        }
    }
}

// ---- Choke --------------------------------------------------------------------------

/// "Islands don't untap during their controllers' untap steps" (CR 502.3,
/// 205.3i): every Island, both players', a basic or a nonbasic one (Volcanic
/// Island); other lands untap.
#[test]
fn cr_502_3_choke_islands_dont_untap() {
    let mut g = scen()
        .battlefield(0, &[CHOKE, "Island", "Forest"])
        .battlefield(1, &["Island", "Volcanic Island", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    for &c in g.battlefield().to_vec().iter() {
        if g.is_type(c, types::LAND) {
            g.set_tapped(c, true);
        }
    }
    to_main(&mut g, 1);
    assert!(g.obj(bf(&g, 1, "Island")).tapped && g.obj(bf(&g, 1, "Volcanic Island")).tapped, "{}", log(&g));
    assert!(!g.obj(bf(&g, 1, "Mountain")).tapped);
    to_main(&mut g, 0);
    assert!(g.obj(bf(&g, 0, "Island")).tapped, "its controller's own Island too");
    assert!(!g.obj(bf(&g, 0, "Forest")).tapped);
}

/// The land types after the layers (CR 613.1d, 305.7): under Magus of the
/// Moon a Volcanic Island is a Mountain, no Island: it untaps; a basic Island
/// stays an Island. Choke gone, Islands untap again.
#[test]
fn cr_305_7_choke_reads_land_types_after_magus_of_the_moon() {
    let mut g = scen()
        .battlefield(0, &[CHOKE])
        .battlefield(1, &["Island", "Volcanic Island", "Magus of the Moon"])
        .at(Step::Main1, 0)
        .start();
    for n in ["Island", "Volcanic Island"] {
        let c = bf(&g, 1, n);
        g.set_tapped(c, true);
    }
    to_main(&mut g, 1);
    assert!(g.obj(bf(&g, 1, "Island")).tapped);
    assert!(!g.obj(bf(&g, 1, "Volcanic Island")).tapped, "a Mountain now: {}", log(&g));
    // Choke gone (Force of Vigor): the Island untaps in the next untap step.
    let mut g = scen()
        .battlefield(0, &[CHOKE, "Island"])
        .battlefield(1, &["Forest", "Forest", "Forest", "Forest"])
        .hand(1, &["Force of Vigor"])
        .at(Step::Main1, 1)
        .start();
    let i = bf(&g, 0, "Island");
    g.set_tapped(i, true);
    let ch = bf(&g, 0, CHOKE);
    let t = g.tgt(ch);
    cast_with(&mut g, 1, "Force of Vigor", &|g| g.has_option(t).then_some(t));
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target) && g.has_option(Choice::Pass)).then_some(Choice::Pass));
    assert!(!on_bf(&g, 0, CHOKE), "{}", log(&g));
    to_main(&mut g, 0);
    assert!(!g.obj(i).tapped);
}

/// Only the untap step's untapping is stopped (CR 502.3): an effect that
/// untaps an Island (Nissa's +1: "Untap it") still does.
#[test]
fn cr_502_3_choke_leaves_other_untap_effects() {
    let mut g = scen().battlefield(0, &[CHOKE, "Nissa, Who Shakes the World", "Island"]).at(Step::Main1, 0).start();
    let i = bf(&g, 0, "Island");
    g.set_tapped(i, true);
    let nissa = bf(&g, 0, "Nissa, Who Shakes the World");
    let plus = abilities_offered(&g, nissa)[0];
    g.choose(Choice::Ability(nissa, plus));
    let t = g.tgt(i);
    until_priority(&mut g, &|g| g.has_option(t).then_some(t));
    resolve(&mut g);
    assert!(!g.obj(i).tapped, "{}", log(&g));
}

// ---- The Tabernacle at Pendrell Vale ----------------------------------------------------

/// Pass to P`p`'s next upkeep, its triggers on the stack (ordered as
/// offered).
fn to_upkeep(g: &mut Game, p: PlayerId) {
    let n = g.turn().number;
    pass_to_with(
        g,
        |g| g.turn().number > n && g.turn().active == p && g.turn().step == Step::Upkeep && at_priority(g),
        &|g| (g.kind() == Some(DecisionKind::OrderTriggers)).then(|| g.pending_decision().unwrap().options[0].choice),
    );
}

/// The Tabernacle's triggers on the stack, by their sources' names.
fn tab_triggers(g: &Game) -> Vec<String> {
    g.stack()
        .iter()
        .filter_map(|s| match s.kind {
            engine::game::StackKind::Ability { ability, source } if ability.def == def(TAB) => Some(g.name(source.idx).to_string()),
            _ => None,
        })
        .collect()
}

/// 'All creatures have "At the beginning of your upkeep, destroy this
/// creature unless you pay {1}."' (CR 113.7, 604.1, 503.1a): at P0's upkeep
/// each creature P0 controls triggers, its own ability (its source the
/// creature, its controller P0); P1's creatures don't. P0 pays {1} for one,
/// not for the other: that one is destroyed (CR 118.12).
#[test]
fn cr_503_1a_the_tabernacle_each_creature_triggers_at_its_controllers_upkeep() {
    let mut g = scen()
        .battlefield(0, &[TAB, "Thalia, Guardian of Thraben", "Guide of Souls", "Plains"])
        .battlefield(1, &["Lotus Cobra"])
        .at(Step::Main1, 1)
        .start();
    to_upkeep(&mut g, 0);
    let mut t = tab_triggers(&g);
    t.sort();
    assert_eq!(t, vec!["Guide of Souls", "Thalia, Guardian of Thraben"], "{}", log(&g));
    assert!(g.stack().iter().all(|s| s.controller == 0));
    // The top one: pay; the next: don't.
    let top = tab_triggers(&g).last().unwrap().clone();
    let mut first = true;
    let mut paid = false;
    for _ in 0..100 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        if g.kind() == Some(DecisionKind::YesNo) {
            let c = if first { Choice::Yes } else { Choice::Pass };
            paid |= first;
            first = false;
            g.choose(c);
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert!(paid);
    assert!(on_bf(&g, 0, &top), "paid for: {}", log(&g));
    let other = if top == "Guide of Souls" { "Thalia, Guardian of Thraben" } else { "Guide of Souls" };
    assert!(!on_bf(&g, 0, other), "not paid for: {}", log(&g));
    assert!(g.obj(bf(&g, 0, "Plains")).tapped);
    assert!(on_bf(&g, 1, "Lotus Cobra"));
}

/// Without mana to pay, "pay {1}" isn't offered (CR 118.3, 601.2g: a cost
/// that can't be paid); the creature is destroyed. P1's creatures trigger at
/// P1's upkeep though P0 controls the Tabernacle ("your upkeep": the
/// creature's controller's).
#[test]
fn cr_118_3_the_tabernacle_without_mana_the_creature_is_destroyed() {
    let mut g = scen().battlefield(0, &[TAB]).battlefield(1, &["Lotus Cobra"]).at(Step::Main1, 0).start();
    to_upkeep(&mut g, 1);
    assert_eq!(tab_triggers(&g), vec!["Lotus Cobra"]);
    assert!(g.stack().iter().all(|s| s.controller == 1));
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", opts(&g));
    assert!(!g.has_option(Choice::Yes), "no mana: {}", opts(&g));
    g.choose(Choice::Pass);
    resolve(&mut g);
    assert!(!on_bf(&g, 1, "Lotus Cobra"), "{}", log(&g));
}

/// The payment is a mana payment (CR 118.12, 605.3a): a creature's own mana
/// ability pays for it (Birds of Paradise, Dryad Arbor: controlled since the
/// turn began, CR 302.6).
#[test]
fn cr_605_3a_the_tabernacle_a_creature_pays_for_itself() {
    let mut g = scen().battlefield(0, &[TAB, "Birds of Paradise", "Dryad Arbor"]).at(Step::Main1, 1).start();
    to_upkeep(&mut g, 0);
    assert_eq!(tab_triggers(&g).len(), 2);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::YesNo) && g.has_option(Choice::Yes)).then_some(Choice::Yes));
    assert!(on_bf(&g, 0, "Birds of Paradise") && on_bf(&g, 0, "Dryad Arbor"), "{}", log(&g));
    assert!(g.obj(bf(&g, 0, "Birds of Paradise")).tapped && g.obj(bf(&g, 0, "Dryad Arbor")).tapped);
}

/// "Destroy this creature": the trigger's source as it is (CR 400.7); bounced
/// in response (Karakas returns a legendary creature), nothing is destroyed;
/// the payment is still asked (CR 118.12: a player may pay).
#[test]
fn cr_400_7_the_tabernacle_the_source_gone_nothing_happens() {
    let mut g = scen()
        .battlefield(0, &[TAB, "Thalia, Guardian of Thraben", "Karakas", "Plains"])
        .at(Step::Main1, 1)
        .start();
    to_upkeep(&mut g, 0);
    assert_eq!(tab_triggers(&g), vec!["Thalia, Guardian of Thraben"]);
    let k = bf(&g, 0, "Karakas");
    let th = bf(&g, 0, "Thalia, Guardian of Thraben");
    let bounce = abilities_offered(&g, k);
    assert_eq!(bounce.len(), 1, "{}", opts(&g));
    g.choose(Choice::Ability(k, bounce[0]));
    let t = g.tgt(th);
    until_priority(&mut g, &|g| g.has_option(t).then_some(t));
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.card(0, "Thalia, Guardian of Thraben", Zone::Hand), th.max(g.card(0, "Thalia, Guardian of Thraben", Zone::Hand)));
    assert!(g.find_card(0, "Thalia, Guardian of Thraben", Zone::Graveyard).is_none(), "{}", log(&g));
}

/// A nonbasic land that is a Mountain loses its abilities (CR 305.7: Magus of
/// the Moon): the Tabernacle grants nothing, no creature triggers.
#[test]
fn cr_305_7_the_tabernacle_under_magus_of_the_moon_grants_nothing() {
    let mut g = scen().battlefield(0, &[TAB, "Thalia, Guardian of Thraben"]).battlefield(1, &["Magus of the Moon"]).at(Step::Main1, 1).start();
    to_upkeep(&mut g, 0);
    assert!(tab_triggers(&g).is_empty(), "{}", log(&g));
    // Torpor Orb doesn't stop it (not an enters trigger).
    let mut g = scen().battlefield(0, &[TAB, "Thalia, Guardian of Thraben"]).battlefield(1, &[ORB]).at(Step::Main1, 1).start();
    to_upkeep(&mut g, 0);
    assert_eq!(tab_triggers(&g).len(), 1);
}

/// Layer 6 in timestamp order (CR 613.1f, 613.7): a creature that lost all
/// its abilities after the Tabernacle entered doesn't have the granted
/// ability (The Wondrous Wasp's sting, later); one that lost them before
/// the Tabernacle entered has it.
#[test]
fn cr_613_7_the_tabernacle_and_losing_all_abilities_by_timestamp() {
    // The Wasp after the Tabernacle: no trigger for Thalia.
    let mut g = scen()
        .battlefield(0, &[TAB, "Thalia, Guardian of Thraben"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["The Wondrous Wasp"])
        .at(Step::Main1, 1)
        .start();
    let th = bf(&g, 0, "Thalia, Guardian of Thraben");
    let t = g.tgt(th);
    cast(&mut g, 1, "The Wondrous Wasp");
    resolve_with(&mut g, &|g| g.has_option(t).then_some(t));
    to_upkeep(&mut g, 0);
    assert!(tab_triggers(&g).is_empty(), "{}", log(&g));
    // The Tabernacle played after the sting: Thalia has the ability.
    let mut g = scen()
        .battlefield(0, &["Thalia, Guardian of Thraben"])
        .hand(0, &[TAB])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["The Wondrous Wasp"])
        .at(Step::Main1, 1)
        .start();
    let th = bf(&g, 0, "Thalia, Guardian of Thraben");
    let t = g.tgt(th);
    cast(&mut g, 1, "The Wondrous Wasp");
    resolve_with(&mut g, &|g| g.has_option(t).then_some(t));
    to_main(&mut g, 0);
    let tab = hand(&g, 0, TAB);
    g.choose(Choice::Play(tab));
    to_upkeep(&mut g, 0);
    assert_eq!(tab_triggers(&g), vec!["Thalia, Guardian of Thraben"], "{}", log(&g));
}

/// The Tabernacle is legendary (CR 704.5j): a second one its controller
/// plays goes to the graveyard by the legend rule; each player's own is
/// theirs, and both grant: every creature then triggers twice.
#[test]
fn cr_704_5j_the_tabernacle_two_grants_trigger_twice() {
    let mut g = scen().battlefield(0, &[TAB, "Thalia, Guardian of Thraben"]).battlefield(1, &[TAB]).at(Step::Main1, 1).start();
    to_upkeep(&mut g, 0);
    assert_eq!(tab_triggers(&g).len(), 2, "{}", log(&g));
}
