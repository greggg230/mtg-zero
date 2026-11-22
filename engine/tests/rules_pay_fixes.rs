//! The payment fixes the Hogaak lanes found (ENGINE.md 221; 220.1.5's findings): a payment's
//! sources grouped as interchangeable only when the rest of the game can't tell them apart (CR
//! 601.2g-h; 221.2), castability counting convoke's and improvise's sources for a spell whose mana
//! is spent too (601.2f, 702.51a, 702.126a; 221.3), and delve's need counting them (702.66a;
//! 221.5). Test names cite the CR.
//!
//! The table: the cube's and the legacy pool's cards, Vote Out (Edge of Eternities: {3}{B} sorcery,
//! "Convoke / Destroy target creature."), and two test cards with delve beside convoke or improvise
//! whose mana may be spent (none of the tables has one).

use engine::db::{AbilitySlot, CardDb, Counter};
use engine::decision::act;
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const VOTE_OUT: &str = "Vote Out";
const KAPPA: &str = "Kappa Cannoneer";
/// Green 2/2 (vanilla on the battlefield).
const GREEN: &str = "Elvish Spirit Guide";
/// Black 2/1 (its trigger is its controller's upkeep's).
const BLACK: &str = "Dark Confidant";
/// Colourless 3/3 artifact creature (vanilla in a game).
const GREY: &str = "Cogwork Librarian";
/// {4}{G} Creature - Beast 5/5: "Convoke, delve" (a test card).
const CONVOKE_DELVE: &str = "Test Convoke Delve Beast";
/// {4}{U} Artifact Creature - Golem 4/4: "Improvise, delve" (a test card).
const IMPROVISE_DELVE: &str = "Test Improvise Delve Golem";
/// `ChooseAltCost`'s answer for "without paying its mana cost" (`eoe_c::FREE_FIRST_SPELL`).
const FREE: u8 = (act::NUMBER_N - 1) as u8;

fn db() -> &'static CardDb {
    use serde_json::json;
    static DB: std::sync::OnceLock<&'static CardDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let beast = json!({"name": CONVOKE_DELVE, "custom": true, "printing": {
            "$t": "CardPrinting", "name": CONVOKE_DELVE, "card_types": ["Creature"], "subtypes": ["Beast"],
            "supertypes": [], "type_line": "Creature - Beast", "power": 5, "toughness": 5,
            "mana_cost": {"$t": "ManaCost", "generic": 4, "colored": [["G", 1]]}, "mana_cost_str": "{4}{G}",
            "has_convoke": true, "has_delve": true}});
        let golem = json!({"name": IMPROVISE_DELVE, "custom": true, "printing": {
            "$t": "CardPrinting", "name": IMPROVISE_DELVE, "card_types": ["Artifact", "Creature"], "subtypes": ["Golem"],
            "supertypes": [], "type_line": "Artifact Creature - Golem", "power": 4, "toughness": 4,
            "mana_cost": {"$t": "ManaCost", "generic": 4, "colored": [["U", 1]]}, "mana_cost_str": "{4}{U}",
            "has_improvise": true, "has_delve": true}});
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(engine::db::load_legacy_document().cards);
        cards.extend(engine::db::load_eoe_document().cards.into_iter().filter(|c| c.name == VOTE_OUT));
        cards.push(serde_json::from_value::<card_ir::Card>(beast).expect("the test card's IR"));
        cards.push(serde_json::from_value::<card_ir::Card>(golem).expect("the test card's IR"));
        let db = CardDb::from_cards(&cards);
        for n in [VOTE_OUT, CONVOKE_DELVE, IMPROVISE_DELVE] {
            db.expect_id(n).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12]).at(Step::Main1, 0)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn all_bf(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.obj(c).controller == p && g.name(c) == name).collect()
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Every path of decisions from `g` to the next priority (`fixed` answering the decisions that
/// aren't the payment's: a target, the cost to pay), at most `cap`: the games there.
fn leaves(g: &Game, cap: usize, fixed: &dyn Fn(&Game) -> Option<Choice>) -> Vec<Game> {
    let mut out = Vec::new();
    let mut todo = vec![g.clone()];
    while let Some(w) = todo.pop() {
        if out.len() >= cap {
            break;
        }
        if at_priority(&w) || w.is_over() {
            out.push(w);
            continue;
        }
        let d = w.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(&w)));
        assert!(!d.options.is_empty(), "a decision without options\n{}", log(&w));
        if let Some(c) = fixed(&w) {
            let mut x = w.clone();
            x.choose(c);
            todo.push(x);
            continue;
        }
        for o in &d.options {
            let mut x = w.clone();
            x.choose(o.choice);
            todo.push(x);
        }
    }
    out
}

/// Answers a target with P1's first creature.
fn target_theirs(g: &Game) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| {
        let t = g.permanents_of(1).find(|&c| g.is_creature(c)).expect("P1's creature");
        g.tgt(t)
    })
}

/// Begin casting `name` from P0's hand.
fn begin(g: &mut Game, name: &str) -> CardIdx {
    let c = g.card(0, name, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    c
}

// ---- 221.2: a payment's sources the rest of the game tells apart -----------------------------

/// Vote Out's leaves on a board of `bf` (with a Swamp: its {B}) against P1's Dark Confidant: for each
/// way to pay, the creature left untapped.
fn vote_out_leftovers(g: &Game) -> Vec<CardIdx> {
    let mut g = g.clone();
    let v = begin(&mut g, VOTE_OUT);
    leaves(&g, 200, &target_theirs)
        .iter()
        .map(|w| {
            assert_eq!(w.obj(v).zone, Zone::Stack, "cast\n{}", log(w));
            let up: Vec<CardIdx> = w.permanents_of(0).filter(|&c| w.is_creature(c) && !w.obj(c).tapped).collect();
            assert_eq!(up.len(), 1, "three of the four creatures convoke\n{}", log(w));
            up[0]
        })
        .collect()
}

#[test]
fn cr_601_2h_702_51a_convoke_offers_a_creature_its_counters_tell_apart() {
    // Four green 2/2s, the first with a +1/+1 counter: three of them pay {3}. Which stays untapped is
    // the player's choice - the 3/3 or a 2/2 (before: one class, the first three always tapped).
    let g = scen()
        .battlefield(0, &["Swamp", GREEN, GREEN, GREEN, GREEN])
        .counters(0, GREEN, Counter::P1P1, 1)
        .battlefield(1, &[BLACK])
        .hand(0, &[VOTE_OUT])
        .start();
    let big = all_bf(&g, 0, GREEN)[0];
    let left = vote_out_leftovers(&g);
    assert_eq!(left.len(), 2, "two ways: the 3/3 or a 2/2 left untapped");
    assert!(left.contains(&big) && left.iter().any(|&c| c != big));
}

#[test]
fn cr_302_6_702_51a_convoke_offers_a_summoning_sick_creature_apart() {
    // One of the four entered this turn: tapping it keeps an attacker (CR 302.6, 508.1a).
    let g = scen()
        .battlefield(0, &["Swamp", GREEN, GREEN, GREEN])
        .battlefield_new(0, &[GREEN])
        .battlefield(1, &[BLACK])
        .hand(0, &[VOTE_OUT])
        .start();
    let sick = all_bf(&g, 0, GREEN)[3];
    assert!(g.is_sick(sick));
    let left = vote_out_leftovers(&g);
    assert_eq!(left.len(), 2, "the sick one or a ready one left untapped");
    assert!(left.contains(&sick) && left.iter().any(|&c| c != sick));
}

#[test]
fn cr_120_3e_702_51a_convoke_offers_a_damaged_creature_apart() {
    let mut g = scen().battlefield(0, &["Swamp", GREEN, GREEN, GREEN, GREEN]).battlefield(1, &[BLACK]).hand(0, &[VOTE_OUT]).start();
    let hurt = all_bf(&g, 0, GREEN)[1];
    g.set_damage(hurt, 1);
    let left = vote_out_leftovers(&g);
    assert_eq!(left.len(), 2, "the damaged one or another left untapped");
    assert!(left.contains(&hurt) && left.iter().any(|&c| c != hurt));
}

#[test]
fn cr_601_2c_702_51a_convoke_offers_the_spells_own_target_apart() {
    // Vote Out targets P0's own first 2/2: tapping it or a twin is two outcomes (the target is
    // named by the spell: CR 601.2c).
    let g = scen().battlefield(0, &["Swamp", GREEN, GREEN, GREEN, GREEN]).battlefield(1, &[BLACK]).hand(0, &[VOTE_OUT]).start();
    let mine = all_bf(&g, 0, GREEN)[0];
    let mut g = g.clone();
    let v = begin(&mut g, VOTE_OUT);
    let target_mine = |g: &Game| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(mine));
    let left: Vec<CardIdx> = leaves(&g, 200, &target_mine)
        .iter()
        .map(|w| {
            assert_eq!(w.obj(v).zone, Zone::Stack);
            w.permanents_of(0).find(|&c| w.is_creature(c) && !w.obj(c).tapped).expect("one left")
        })
        .collect();
    assert_eq!(left.len(), 2, "the target or a twin left untapped");
    assert!(left.contains(&mine));
}

#[test]
fn cr_601_2h_702_51a_twins_stay_one_option() {
    // Four green 2/2s alike, none named: one way to pay (the Swamp, then three of them).
    let g = scen().battlefield(0, &["Swamp", GREEN, GREEN, GREEN, GREEN]).battlefield(1, &[BLACK]).hand(0, &[VOTE_OUT]).start();
    assert_eq!(vote_out_leftovers(&g).len(), 1, "twins are one class");
}

#[test]
fn cr_605_3a_302_6_two_mana_creatures_a_counter_tells_apart_are_two_options() {
    // Two Llanowar Elves pay a third's {G}: alike, one option; one with a +1/+1 counter, two.
    for countered in [false, true] {
        let mut s = scen().battlefield(0, &["Llanowar Elves", "Llanowar Elves"]).hand(0, &["Llanowar Elves"]);
        if countered {
            s = s.counters(0, "Llanowar Elves", Counter::P1P1, 1);
        }
        let mut g = s.start();
        begin(&mut g, "Llanowar Elves");
        g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana));
        let elves = all_bf(&g, 0, "Llanowar Elves");
        let n = elves.iter().filter(|&&e| g.has_option(Choice::Ability(e, AbilitySlot::Activated(0)))).count();
        assert_eq!(n, if countered { 2 } else { 1 }, "countered {countered}: {}", opts(&g));
    }
}

#[test]
fn cr_302_6_601_2g_a_land_that_entered_this_turn_is_apart_from_an_older_twin() {
    // Dark Ritual's {B} from two Swamps: both older, one option; one played this turn, two (a land
    // that becomes a creature this turn is summoning sick: CR 302.6).
    for new in [false, true] {
        let mut s = scen().battlefield(0, &["Swamp"]).hand(0, &["Dark Ritual"]);
        s = if new { s.battlefield_new(0, &["Swamp"]) } else { s.battlefield(0, &["Swamp"]) };
        let mut g = s.start();
        begin(&mut g, "Dark Ritual");
        g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana));
        let swamps = all_bf(&g, 0, "Swamp");
        let n = swamps.iter().filter(|&&c| g.has_option(Choice::Ability(c, AbilitySlot::Activated(0)))).count();
        assert_eq!(n, if new { 2 } else { 1 }, "new {new}: {}", opts(&g));
    }
}

#[test]
fn cr_702_126a_605_3a_an_artifact_urza_can_tap_is_offered_for_improvise_too() {
    // Kappa Cannoneer ({5}{U}, improvise) with Urza, Lord High Artificer ("Tap an untapped artifact
    // you control: Add {U}"), an Island and five artifacts: the Island picked first, an artifact
    // picked is asked which - Urza's {U} (mana spent, a colour spent) or improvise's {1} (no mana;
    // CR 702.126a). Before, improvise wasn't offered for it.
    let mut g = scen()
        .battlefield(0, &["Island", "Urza, Lord High Artificer", "Mishra's Bauble", "Urza's Bauble", "Grindstone", "Sensei's Divining Top", "Mox Sapphire"])
        .hand(0, &[KAPPA])
        .start();
    let k = begin(&mut g, KAPPA);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana));
    let island = bf(&g, 0, "Island");
    g.choose(Choice::Ability(island, AbilitySlot::Activated(0)));
    let bauble = bf(&g, 0, "Mishra's Bauble");
    assert!(g.has_option(Choice::Card(bauble)), "{}", opts(&g));
    g.choose(Choice::Card(bauble));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "which: {}", opts(&g));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "Urza's {{U}} or improvise: {}", opts(&g));
    for w in leaves(&g, 400, &|_| None) {
        assert_eq!(w.obj(k).zone, Zone::Stack, "cast\n{}", log(&w));
    }
}

#[test]
fn cr_702_51a_605_3a_an_artifact_creature_urza_can_tap_convokes_for_its_colour() {
    // Vote Out ({3}{B}) with Urza and three artifacts: Cogwork Librarian (a colourless artifact
    // creature) is a convoke source as well as Urza's {U}; Urza himself (a red creature) convokes.
    let g = scen()
        .battlefield(0, &["Swamp", "Urza, Lord High Artificer", GREY, "Mishra's Bauble"])
        .battlefield(1, &[BLACK])
        .hand(0, &[VOTE_OUT])
        .start();
    let mut g = g.clone();
    let v = begin(&mut g, VOTE_OUT);
    let ws = leaves(&g, 400, &target_theirs);
    assert!(!ws.is_empty());
    for w in &ws {
        assert_eq!(w.obj(v).zone, Zone::Stack, "cast\n{}", log(w));
    }
    // Some path convokes the Librarian (no mana from it), another taps it for Urza's {U}.
    let lib = bf(&g, 0, GREY);
    let convoked = format!("taps {} for convoke", g.name(lib));
    assert!(ws.iter().any(|w| w.obj(lib).tapped && log(w).contains(&convoked)), "the Librarian convoked on some path");
    assert!(ws.iter().any(|w| w.obj(lib).tapped && !log(w).contains(&convoked)), "the Librarian tapped for Urza's {{U}} on another");
}

// ---- 221.3: castability counts convoke and improvise for a spell whose mana is spent ---------

#[test]
fn cr_118_9_702_51a_omniscience_still_offers_vote_outs_mana_cost_paid_with_convoke() {
    // A Swamp and three creatures pay {3}{B} only with convoke's help: with Omniscience both the free
    // cast and the mana cost are offered (before: the mana cost wasn't).
    let mut g = scen()
        .battlefield(0, &["Omniscience", "Swamp", GREEN, GREEN, BLACK])
        .battlefield(1, &[BLACK])
        .hand(0, &[VOTE_OUT])
        .start();
    begin(&mut g, VOTE_OUT);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::Target));
    if g.kind() == Some(DecisionKind::Target) {
        let t = target_theirs(&g).unwrap();
        g.choose(t);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    }
    assert!(g.has_option(Choice::Number(0)) && g.has_option(Choice::Number(FREE)), "{}", opts(&g));
    g.choose(Choice::Number(0));
    for w in leaves(&g, 200, &target_theirs) {
        assert_eq!(w.permanents_of(0).filter(|&c| w.obj(c).tapped).count(), 4, "the Swamp and three creatures\n{}", log(&w));
    }
}

#[test]
fn cr_118_9_702_126a_omniscience_still_offers_kappas_mana_cost_paid_with_improvise() {
    let mut g = scen()
        .battlefield(0, &["Omniscience", "Island", "Mishra's Bauble", "Urza's Bauble", "Grindstone", "Sensei's Divining Top", "Mox Sapphire"])
        .hand(0, &[KAPPA])
        .start();
    let k = begin(&mut g, KAPPA);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    assert!(g.has_option(Choice::Number(0)) && g.has_option(Choice::Number(FREE)), "{}", opts(&g));
    g.choose(Choice::Number(0));
    for w in leaves(&g, 400, &|_| None) {
        assert_eq!(w.obj(k).zone, Zone::Stack, "cast\n{}", log(&w));
    }
}

/// The brute force of a board: (units, units of the pip's colour), each source one unit but Sol
/// Ring's own two {C}; the artifacts each the best of their ways that keeps the pip's.
#[derive(Clone, Copy)]
enum Src {
    /// A land or a pool mana: of the pip's colour or not.
    Mana(bool),
    /// A creature (convoke): of the pip's colour or not.
    Creature(bool),
    /// An artifact with no mana ability (improvise {1}; Urza's {U} when he's there).
    Artifact,
    /// Mox Sapphire ({U}, or improvise, or Urza's {U}).
    Mox,
    /// Sol Ring ({C}{C}, or improvise, or Urza's {U}).
    SolRing,
}

/// Can `cost_n` symbols with one pip of a colour be paid from `srcs` (the pip by a unit of its
/// colour, the rest by any)? Every artifact's way tried (Urza's {U} with `urza`).
fn brute(srcs: &[Src], cost_n: u32, urza: bool) -> bool {
    let arts: Vec<Src> = srcs.iter().copied().filter(|s| matches!(s, Src::Artifact | Src::Mox | Src::SolRing)).collect();
    let (mut base, mut base_pip) = (0u32, 0u32);
    for s in srcs {
        match s {
            Src::Mana(c) | Src::Creature(c) => {
                base += 1;
                base_pip += *c as u32;
            }
            _ => {}
        }
    }
    // Each artifact: 0 = improvise, 1 = its own mana, 2 = Urza's {U}.
    let n = arts.len();
    (0..3u32.pow(n as u32)).any(|code| {
        let (mut units, mut pip) = (base, base_pip);
        let mut k = code;
        for a in &arts {
            let way = k % 3;
            k /= 3;
            match (a, way) {
                (_, 0) => units += 1,
                (Src::Mox, 1) => {
                    units += 1;
                    pip += 1;
                }
                (Src::SolRing, 1) => units += 2,
                (Src::Artifact, 1) => return false,
                (_, 2) if urza => {
                    units += 1;
                    pip += 1;
                }
                (_, 2) => return false,
                _ => unreachable!(),
            }
        }
        units >= cost_n && pip >= 1
    })
}

#[test]
fn cr_601_2f_702_51a_702_126a_castability_and_the_payment_agree_on_random_boards() {
    // Random boards for Vote Out ({3}{B}, convoke) and Kappa Cannoneer ({5}{U}, improvise): lands of
    // the pip's colour and not, duals, floating mana, creatures of every colour kind (black, green,
    // colourless; some summoning sick, some with counters or damage: twins and not), artifacts with
    // and without mana abilities, sometimes Urza (his {U} from any artifact) and Omniscience (a free
    // cast beside the mana cost). Castability - the Cast option, and with Omniscience the mana
    // cost's option at the cost's choice (`normal_payable`) - must agree with a brute force over
    // every source's way, and from every castable board every path of the payment must end with
    // the spell cast.
    let mut rng = Rng::new(221);
    let (mut castable_n, mut paths_n, mut alt_n) = (0, 0usize, 0);
    for case in 0..600 {
        let vote = case % 2 == 0;
        let omni = rng.below(3) == 0;
        let urza = !vote && rng.below(3) == 0;
        let mut names: Vec<&str> = Vec::new();
        let mut srcs: Vec<Src> = Vec::new();
        let lands: &[(&str, bool)] = if vote {
            &[("Swamp", true), ("Forest", false), ("Island", false), ("Bayou", true)]
        } else {
            &[("Island", true), ("Mountain", false), ("Volcanic Island", true)]
        };
        for _ in 0..rng.below(4) {
            let (n, pip) = lands[rng.below(lands.len() as u32) as usize];
            names.push(n);
            srcs.push(Src::Mana(pip));
        }
        let mut sick: Vec<&str> = Vec::new();
        let mut counter_on: Option<&str> = None;
        if vote {
            let kinds: [(&str, bool); 4] = [(GREEN, false), (BLACK, true), (GREY, false), ("Llanowar Elves", false)];
            for _ in 0..rng.below(6) {
                let (n, pip) = kinds[rng.below(4) as usize];
                if rng.below(4) == 0 {
                    sick.push(n);
                } else {
                    names.push(n);
                }
                srcs.push(Src::Creature(pip));
            }
            if rng.below(3) == 0 {
                counter_on = names.iter().copied().find(|n| kinds.iter().any(|k| k.0 == *n));
            }
        } else {
            let arts = [("Mishra's Bauble", Src::Artifact), ("Grindstone", Src::Artifact), ("Mox Sapphire", Src::Mox), ("Sol Ring", Src::SolRing), ("Urza's Bauble", Src::Artifact)];
            for _ in 0..rng.below(5) {
                let (n, s) = arts[rng.below(5) as usize];
                names.push(n);
                srcs.push(s);
            }
            if urza {
                names.push("Urza, Lord High Artificer");
            }
        }
        if omni {
            names.push("Omniscience");
        }
        let pool_pip = rng.below(3) == 0;
        let pool_other = rng.below(3) == 0;
        let mut pool = [0u8; 6];
        let pip_t = if vote { 2 } else { 1 };
        if pool_pip {
            pool[pip_t] += 1;
            srcs.push(Src::Mana(true));
        }
        if pool_other {
            pool[4] += 1;
            srcs.push(Src::Mana(false));
        }
        let card = if vote { VOTE_OUT } else { KAPPA };
        let mut s = scen().battlefield(0, &names).battlefield_new(0, &sick).battlefield(1, &[BLACK]).hand(0, &[card]).pool(0, pool, 0);
        if let Some(n) = counter_on {
            s = s.counters(0, n, Counter::P1P1, 1);
        }
        let mut g = s.start();
        // (A damaged creature now and then: a twin told apart.)
        if vote && rng.below(3) == 0 {
            let hurt = g.permanents_of(0).find(|&c| g.is_creature(c) && g.name(c) == GREEN);
            if let Some(c) = hurt {
                g.set_damage(c, 1);
            }
        }
        let c = g.card(0, card, Zone::Hand);
        let payable = brute(&srcs, if vote { 4 } else { 6 }, urza);
        let offered = g.has_option(Choice::Cast(c));
        assert_eq!(offered, omni || payable, "case {case}: offered {offered}, Omniscience {omni}, brute force {payable}: {names:?} sick {sick:?} pool {pool:?}\n{}", opts(&g));
        if !offered {
            continue;
        }
        castable_n += 1;
        let mut g0 = g.clone();
        g0.choose(Choice::Cast(c));
        // The decisions before the payment: the target, the cost (the mana cost when the brute force
        // says it is payable; the free cast otherwise).
        let mut alt_seen = false;
        for _ in 0..10 {
            match g0.kind() {
                Some(DecisionKind::Target) => {
                    let t = target_theirs(&g0).unwrap();
                    g0.choose(t);
                }
                Some(DecisionKind::ChooseAltCost) => {
                    alt_seen = true;
                    assert_eq!(g0.has_option(Choice::Number(0)), payable, "case {case}: the mana cost offered iff payable: {names:?} pool {pool:?}\n{}", opts(&g0));
                    g0.choose(Choice::Number(if payable { 0 } else { FREE }));
                }
                _ => break,
            }
        }
        assert_eq!(alt_seen, omni, "case {case}: the cost's choice asked iff Omniscience");
        alt_n += alt_seen as usize;
        let ws = leaves(&g0, 300, &target_theirs);
        assert!(!ws.is_empty(), "case {case}");
        for w in &ws {
            assert_eq!(w.obj(c).zone, Zone::Stack, "case {case}: cast\n{}", log(w));
        }
        paths_n += ws.len();
    }
    assert!(castable_n >= 200 && paths_n >= 1500 && alt_n >= 60, "castable boards {castable_n}, payment paths {paths_n}, cost choices {alt_n}");
}

// ---- 221.5: delve's need counts convoke and improvise -----------------------------------------

/// The graveyard cards a delve decision offers, by their place in P0's graveyard; and whether it
/// offers `PASS`.
fn delve_offer(g: &Game) -> (Vec<usize>, bool) {
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{}", opts(g));
    let gy = &g.player(0).graveyard;
    let mut at: Vec<usize> = g
        .offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Card(c) => gy.iter().position(|&x| x == c),
            _ => None,
        })
        .collect();
    at.sort_unstable();
    (at, g.has_option(Choice::Pass))
}

#[test]
fn cr_702_66a_702_51a_delves_need_counts_convoke_for_a_spell_whose_mana_is_spent() {
    // {4}{G}, convoke and delve, with a Forest, two green creatures and four graveyard cards: the
    // Forest and the creatures pay three symbols, so two cards must be delved (before: the Forest
    // alone, four). The first pick offers the first three cards (two must follow it), and after
    // two, PASS.
    let mut g = scen()
        .battlefield(0, &["Forest", GREEN, GREEN])
        .graveyard(0, &["Lightning Bolt", "Ponder", "Brainstorm", "Daze"])
        .hand(0, &[CONVOKE_DELVE])
        .start();
    let b = begin(&mut g, CONVOKE_DELVE);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
    assert_eq!(delve_offer(&g), (vec![0, 1, 2], false), "{}", opts(&g));
    let first = g.player(0).graveyard[0];
    g.choose(Choice::Card(first));
    assert_eq!(delve_offer(&g), (vec![1, 2, 3], false), "{}", opts(&g));
    let second = g.player(0).graveyard[1];
    g.choose(Choice::Card(second));
    let (_, pass) = delve_offer(&g);
    assert!(pass, "the rest {{2}}{{G}} is paid by the Forest and the creatures: {}", opts(&g));
    g.choose(Choice::Pass);
    for w in leaves(&g, 200, &|_| None) {
        assert_eq!(w.obj(b).zone, Zone::Stack, "cast\n{}", log(&w));
        assert_eq!(w.player(0).graveyard.len(), 2, "two delved\n{}", log(&w));
        assert!(w.permanents_of(0).all(|c| w.obj(c).tapped), "the Forest and both creatures\n{}", log(&w));
    }
}

#[test]
fn cr_702_66a_702_126a_delves_need_counts_improvise_for_a_spell_whose_mana_is_spent() {
    // {4}{U}, improvise and delve, with an Island, two artifacts and four graveyard cards: two
    // cards delved, then PASS.
    let mut g = scen()
        .battlefield(0, &["Island", "Mishra's Bauble", "Grindstone"])
        .graveyard(0, &["Lightning Bolt", "Ponder", "Brainstorm", "Daze"])
        .hand(0, &[IMPROVISE_DELVE])
        .start();
    let b = begin(&mut g, IMPROVISE_DELVE);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
    assert_eq!(delve_offer(&g), (vec![0, 1, 2], false), "{}", opts(&g));
    for _ in 0..2 {
        let (at, _) = delve_offer(&g);
        let c = g.player(0).graveyard[at[0]];
        g.choose(Choice::Card(c));
    }
    assert!(delve_offer(&g).1, "{}", opts(&g));
    g.choose(Choice::Pass);
    for w in leaves(&g, 200, &|_| None) {
        assert_eq!(w.obj(b).zone, Zone::Stack, "cast\n{}", log(&w));
    }
}

#[test]
fn cr_601_2g_702_66a_a_lotus_petal_is_delve_fuel_when_convoke_pays_the_rest() {
    // {4}{G} with a Lotus Petal, a Forest, two green creatures and one graveyard card: the Petal
    // activated and delved pays {1} and its mana a symbol; the Forest and the creatures pay the
    // other three (before: mana alone, the Petal wasn't offered).
    let mut g = scen()
        .battlefield(0, &["Lotus Petal", "Forest", GREEN, GREEN])
        .graveyard(0, &["Lightning Bolt"])
        .hand(0, &[CONVOKE_DELVE])
        .start();
    let b = begin(&mut g, CONVOKE_DELVE);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
    let petal = bf(&g, 0, "Lotus Petal");
    assert!(g.has_option(Choice::Card(petal)), "{}", opts(&g));
    g.choose(Choice::Card(petal));
    for w in leaves(&g, 200, &|_| None) {
        assert_eq!(w.obj(b).zone, Zone::Stack, "cast\n{}", log(&w));
        assert_eq!(w.obj(petal).zone, Zone::Exile, "the Petal delved\n{}", log(&w));
    }
}
