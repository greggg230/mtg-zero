//! M6 (ENGINE.md 49): castability and payment agree. A spell or ability is
//! offered only if its cost can be paid (CR 601.2f-h, 602.2b, 605.3a), and
//! the payment then offers a way to pay it: every test here is a cube board
//! where the engine offered a cast (or an X) that its payment could not
//! complete (`PayMana: no way to pay`, a panic that fails a self-play run),
//! or counted mana the rules don't give. Each failed before its fix.

mod common;

use common::db;
use engine::mana::ManaCost;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

/// Every path of the decisions from here to the next priority (each option
/// of each decision in turn, depth first, at most `budget` paths): none
/// panics (a decision without options panics as it is asked), each reaches
/// priority. Returns the paths followed.
fn every_path_completes(g: &Game, budget: &mut usize) -> usize {
    if at_priority(g) || g.is_over() {
        *budget = budget.saturating_sub(1);
        return 1;
    }
    let opts = options(g);
    assert!(!opts.is_empty(), "a decision without options\n{}", log(g));
    let mut paths = 0;
    for c in opts {
        if *budget == 0 {
            break;
        }
        let mut h = g.clone();
        h.choose(c);
        paths += every_path_completes(&h, budget);
    }
    paths
}

/// Answer decisions (`answer`, else the first option) until priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

// ---- 49.1: Wizard's Rockets and its card's other uses ----------------------

/// CR 601.2g, 605.3a: Wizard's Rockets ("{X}, {T}, sacrifice this: add X
/// mana in any combination of colors") taps and sacrifices itself, so in the
/// payment it turns into colours it is not also an artifact tapped for
/// Urza's "tap an untapped artifact you control: add {U}". Three Swamps and
/// four Islands, Urza and the Rockets: seven mana once the Rockets converts,
/// so no Griselbrand ({4}{B}{B}{B}{B}). The engine counted the Rockets'
/// {U} and its colours both, offered the cast and panicked paying it (the
/// drafted match e8657). With a fifth Island the cast is offered and paid.
#[test]
fn cr_601_2g_wizards_rockets_converting_is_not_also_tapped_for_urza() {
    let board = ["Wizard's Rockets", "Urza, Lord High Artificer", "Swamp", "Swamp", "Swamp", "Island", "Island", "Island", "Island"];
    let g = scen().battlefield(0, &board).hand(0, &["Griselbrand"]).at(Step::Main1, 0).start();
    let gb = g.card(0, "Griselbrand", Zone::Hand);
    assert!(g.can_pay(0, &cost("{3}{B}{B}{B}{B}")), "seven mana, the Rockets' colours among them");
    assert!(!g.can_pay(0, &cost("{4}{B}{B}{B}{B}")), "the Rockets' {{U}} and its colours both counted");
    assert!(!g.has_option(Choice::Cast(gb)), "Griselbrand offered: {:?}", g.options_desc());
    // Without the need for its colours the Rockets is an artifact for Urza:
    // eight blue-or-black mana pay {5}{B}{B}{B}.
    assert!(g.can_pay(0, &cost("{5}{B}{B}{B}")));
    // A fifth Island: the Rockets converts, the eight lands pay.
    let mut board5 = board.to_vec();
    board5.push("Island");
    let mut g = scen().battlefield(0, &board5).hand(0, &["Griselbrand"]).at(Step::Main1, 0).start();
    let gb = g.card(0, "Griselbrand", Zone::Hand);
    assert!(g.has_option(Choice::Cast(gb)));
    g.choose(Choice::Cast(gb));
    let mut budget = 2_000;
    assert!(every_path_completes(&g, &mut budget) > 0);
    finish(&mut g, |_| None);
    assert_eq!(g.obj(gb).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.count_in(0, "Wizard's Rockets", Zone::Graveyard), 1, "{}", log(&g));
}

/// CR 107.3, 601.2f: X is announced no larger than the player can pay. The
/// drafted match of the search smoke's `team_draft_2v2`: Green Sun's Zenith
/// ({X}{G}) with Wizard's Rockets, Urza and lands of no green. Three Plains,
/// the Rockets and Urza: X is at most 2 (the Rockets turns the Plains' mana
/// into colours; it is then no artifact to tap for Urza). The engine offered
/// X = 3 and panicked paying {3}{G}.
#[test]
fn cr_107_3_x_is_at_most_what_the_payment_can_pay_with_wizards_rockets() {
    let mut g = scen()
        .battlefield(0, &["Wizard's Rockets", "Urza, Lord High Artificer", "Plains", "Plains", "Plains"])
        .hand(0, &["Green Sun's Zenith"])
        .at(Step::Main1, 0)
        .start();
    let gsz = g.card(0, "Green Sun's Zenith", Zone::Hand);
    assert!(g.can_pay(0, &cost("{2}{G}")));
    assert!(!g.can_pay(0, &cost("{3}{G}")));
    g.choose(Choice::Cast(gsz));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "X: {}", log(&g));
    assert_eq!(options(&g), vec![Choice::Number(0), Choice::Number(1), Choice::Number(2)], "X up to 2");
    // Every X, every way of paying it: the payment completes.
    for x in 0..=2u8 {
        let mut h = g.clone();
        h.choose(Choice::Number(x));
        let mut budget = 2_000;
        assert!(every_path_completes(&h, &mut budget) > 0, "X = {x}");
    }
    g.choose(Choice::Number(2));
    finish(&mut g, |_| None);
    assert_eq!(g.obj(gsz).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.count_in(0, "Wizard's Rockets", Zone::Graveyard), 1, "{}", log(&g));
}

/// CR 702.126a (improvise: tap an untapped artifact to pay for {1}), 601.2g:
/// Wizard's Rockets tapped for improvise is not also activated. Five
/// Mountains and the Rockets: Kappa Cannoneer ({5}{U}, improvise) needs the
/// Rockets' colours for its {U}, and then six mana are only five. The engine
/// counted the Rockets for improvise and for its colours.
#[test]
fn cr_702_126a_wizards_rockets_converting_is_not_also_tapped_for_improvise() {
    let board = ["Wizard's Rockets", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"];
    let g = scen().battlefield(0, &board).hand(0, &["Kappa Cannoneer"]).at(Step::Main1, 0).start();
    let kc = g.card(0, "Kappa Cannoneer", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(kc)), "Kappa Cannoneer offered: {:?}", g.options_desc());
    // A sixth Mountain: castable, and paid.
    let mut board6 = board.to_vec();
    board6.push("Mountain");
    let mut g = scen().battlefield(0, &board6).hand(0, &["Kappa Cannoneer"]).at(Step::Main1, 0).start();
    let kc = g.card(0, "Kappa Cannoneer", Zone::Hand);
    assert!(g.has_option(Choice::Cast(kc)));
    g.choose(Choice::Cast(kc));
    let mut budget = 2_000;
    assert!(every_path_completes(&g, &mut budget) > 0);
    finish(&mut g, |_| None);
    assert_eq!(g.obj(kc).zone, Zone::Stack, "{}", log(&g));
}

// ---- 49.2: mana with a spending restriction ----------------------------------

/// CR 106.6, 605.3a: Mishra's Workshop's mana ("spend this mana only to cast
/// artifact spells") can't pay Wizard's Rockets' {X} (activating it isn't
/// casting an artifact spell), so the Rockets doesn't turn it into colours.
/// The Workshop and the Rockets alone: no Cryogen Relic ({1}{U}, an
/// artifact); the engine turned the Workshop's {C} blue and cast it. With a
/// Mountain the Rockets turns the Mountain's mana blue and the Workshop's
/// {C} pays {1}; two artifact-only {C} stay in the pool.
#[test]
fn cr_106_6_wizards_rockets_does_not_turn_mishras_workshop_mana_into_colours() {
    let g = scen().battlefield(0, &["Mishra's Workshop", "Wizard's Rockets"]).hand(0, &["Cryogen Relic"]).at(Step::Main1, 0).start();
    let relic = g.card(0, "Cryogen Relic", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(relic)), "Cryogen Relic offered: {:?}", g.options_desc());
    let mut g = scen()
        .battlefield(0, &["Mishra's Workshop", "Wizard's Rockets", "Mountain"])
        .hand(0, &["Cryogen Relic"])
        .at(Step::Main1, 0)
        .start();
    let relic = g.card(0, "Cryogen Relic", Zone::Hand);
    assert!(g.has_option(Choice::Cast(relic)));
    g.choose(Choice::Cast(relic));
    let mut budget = 2_000;
    assert!(every_path_completes(&g, &mut budget) > 0);
    finish(&mut g, |_| None);
    assert_eq!(g.obj(relic).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.count_in(0, "Wizard's Rockets", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!((g.player(0).pool.amt, g.player(0).pool.art), ([0, 0, 0, 0, 0, 2], 2), "{}", log(&g));
}

// ---- 49.3: sources a graveyard cost needs, with no mana to pay --------------

/// CR 601.2g-h, 602.2b: mana abilities are activated before the rest of a
/// cost is paid, so Lotus Petals sacrificed for mana are graveyard cards an
/// "exile three cards from your graveyard" cost can take (ENGINE.md 34.4).
/// Psychic Frog's ability costs no mana: one card in the graveyard and two
/// Lotus Petals pay it. The payment activated the first Petal, saw no mana
/// left to pay and finished without the second, and the exile found two
/// cards for three (`ChooseGraveyardCard` without options, a panic). Found
/// by the agreement fuzz (49.4, board 85480).
#[test]
fn cr_601_2h_two_lotus_petals_for_psychic_frogs_graveyard_cost() {
    use engine::db::AbilitySlot;
    let mut g = scen()
        .battlefield(0, &["Psychic Frog", "Lotus Petal", "Lotus Petal"])
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let frog = g.card(0, "Psychic Frog", Zone::Battlefield);
    let exile3 = Choice::Ability(frog, AbilitySlot::Activated(1));
    assert!(g.has_option(exile3), "{:?}", g.options_desc());
    g.choose(exile3);
    let mut budget = 2_000;
    assert!(every_path_completes(&g, &mut budget) > 0);
    finish(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Exile), 2, "{}", log(&g));
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Exile), 1, "{}", log(&g));
    assert_eq!(g.player(0).pool.total(), 2, "the Petals' mana: {}", log(&g));
    // One Petal: two cards for three, not offered.
    let g = scen().battlefield(0, &["Psychic Frog", "Lotus Petal"]).graveyard(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let frog = g.card(0, "Psychic Frog", Zone::Battlefield);
    assert!(!g.has_option(Choice::Ability(frog, AbilitySlot::Activated(1))));
}

/// CR 702.66a, 601.2g-h: each mana source that sacrifices itself and is a
/// card pays its mana and, in the graveyard by the time delve's cards are
/// exiled, one more generic mana (ENGINE.md 34.4). Four Islands and two
/// Lotus Petals, an empty graveyard: Treasure Cruise ({7}{U}) is castable
/// (six mana, two Petals delved). The delve decision offered a Petal only
/// if graveyard cards could make up the rest, not the other Petal: no
/// option but to stop, and the payment panicked on {7}{U}. Found by the
/// agreement fuzz (board 980602: Gemstone Mine and a Lotus Petal).
#[test]
fn cr_702_66a_two_lotus_petals_delved_for_treasure_cruise() {
    let board = ["Island", "Island", "Island", "Island", "Lotus Petal", "Lotus Petal"];
    let mut g = scen().battlefield(0, &board).hand(0, &["Treasure Cruise"]).at(Step::Main1, 0).start();
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    assert!(g.has_option(Choice::Cast(tc)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(tc));
    let mut budget = 2_000;
    assert!(every_path_completes(&g, &mut budget) > 0);
    finish(&mut g, |_| None);
    assert_eq!(g.obj(tc).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Exile), 2, "{}", log(&g));
}

/// CR 614.1a, 702.66a: under an opponent's Dauthi Voidwalker ("if a card
/// would be put into an opponent's graveyard from anywhere, instead exile
/// it") a Lotus Petal sacrificed for mana never reaches the graveyard, so
/// delve can't exile it: four Islands and two Petals are six mana for
/// Treasure Cruise's {7}{U}. Castability counted the Petals for delve (the
/// delve decision did not), offered the cast, and the payment panicked.
#[test]
fn cr_614_1a_no_delve_for_lotus_petals_exiled_by_dauthi_voidwalker() {
    let board = ["Island", "Island", "Island", "Island", "Lotus Petal", "Lotus Petal"];
    let g = scen().battlefield(0, &board).battlefield(1, &["Dauthi Voidwalker"]).hand(0, &["Treasure Cruise"]).at(Step::Main1, 0).start();
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(tc)), "Treasure Cruise offered: {:?}", g.options_desc());
}

/// CR 702.138a (escape: the mana cost and exile three other cards from your
/// graveyard, Underworld Breach), 601.2h, 614.1a: Bone Shards' additional
/// cost "discard a card" feeds the graveyard escape exiles from, but not
/// with Blightsteel Colossus (shuffled into its library instead). Two other
/// cards in the graveyard, an Island and the Colossus in hand: the discard
/// must be the Island. The engine offered the Colossus too (a dead end:
/// `ChooseGraveyardCard` without options), and read whether discards feed
/// the graveyard off the first card in hand (the Colossus first: escape not
/// offered at all). Found by the agreement fuzz (board 226634).
#[test]
fn cr_702_138a_escape_discard_must_feed_the_graveyard() {
    for hand in [["Island", "Blightsteel Colossus"], ["Blightsteel Colossus", "Island"]] {
        let mut g = scen()
            .battlefield(0, &["Underworld Breach", "Swamp"])
            .battlefield(1, &["Test Bear"])
            .graveyard(0, &["Bone Shards", "Lightning Bolt", "Ponder"])
            .hand(0, &hand)
            .at(Step::Main1, 0)
            .start();
        let shards = g.card(0, "Bone Shards", Zone::Graveyard);
        assert!(g.has_option(Choice::Cast(shards)), "hand {hand:?}: {:?}", g.options_desc());
        g.choose(Choice::Cast(shards));
        let mut budget = 2_000;
        assert!(every_path_completes(&g, &mut budget) > 0, "hand {hand:?}");
        finish(&mut g, |_| None);
        assert_eq!(g.obj(shards).zone, Zone::Stack, "{}", log(&g));
        assert_eq!(g.count_in(0, "Island", Zone::Exile), 1, "the Island discarded, then exiled: {}", log(&g));
        assert_eq!(g.count_in(0, "Blightsteel Colossus", Zone::Hand), 1, "{}", log(&g));
    }
    // The Colossus alone: nothing to feed the graveyard, no escape.
    let g = scen()
        .battlefield(0, &["Underworld Breach", "Swamp"])
        .battlefield(1, &["Test Bear"])
        .graveyard(0, &["Bone Shards", "Lightning Bolt", "Ponder"])
        .hand(0, &["Blightsteel Colossus"])
        .at(Step::Main1, 0)
        .start();
    let shards = g.card(0, "Bone Shards", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(shards)), "{:?}", g.options_desc());
}

// ---- 49.6: found by the checks -----------------------------------------------

/// CR 702.75a, 712.11b, 715.3: Shelldock Isle's hideaway card is exiled face
/// down; played with the Isle, a card whose two faces can both be cast
/// (Brazen Borrower // Petty Theft) asks which face first, and the card is
/// public from then on, as when the choice is made at priority. The engine
/// asked without revealing it, so a world `determinize` sampled for the
/// opponent dealt the face-down card again, a card without another face,
/// and the choice of the other face panicked ("a card with another face").
/// Found by `FUZZ_POOL=interactions` (seed 7165, with the "mana abilities x
/// costs" group); on 01fa623 as well.
#[test]
fn cr_712_11b_a_face_down_card_cast_with_a_choice_of_faces_is_revealed() {
    use engine::db::AbilitySlot;
    use engine::rng::Rng;
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Shelldock Isle"])
        .library(0, &["Island", "Island", "Brazen Borrower // Petty Theft", "Test Bear", "Forest", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let isle = g.card(0, "Shelldock Isle", Zone::Hand);
    g.choose(Choice::Play(isle));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand));
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Library);
    g.choose(Choice::Card(bb));
    // To P0's next first main phase, passing and declining.
    let t = g.turn().number;
    for _ in 0..2_000 {
        if g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(&g) {
            break;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.obj(bb).zone, Zone::FaceDown, "exiled face down: {}", log(&g));
    let isle = g.card(0, "Shelldock Isle", Zone::Battlefield);
    g.choose(Choice::Ability(isle, AbilitySlot::Activated(1)));
    for _ in 0..200 {
        match g.kind() {
            Some(DecisionKind::ChooseFace) => break,
            Some(DecisionKind::Priority) => g.pass(),
            Some(DecisionKind::YesNo) => g.choose(Choice::Yes),
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c)
            }
        };
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseFace), "{}", log(&g));
    // Every world sampled for P1 keeps the card: both faces complete.
    for s in 0..16 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        let mut budget = 200;
        assert!(every_path_completes(&d, &mut budget) > 0, "world {s}");
    }
    assert_eq!(g.obj(bb).known, 0b11, "revealed to both players");
}

// ---- 49.4: the agreement, fuzzed --------------------------------------------

/// The cube's lands, its nonland permanents with a mana ability or a mana
/// bonus (Urza's tapped artifacts, Badgermole Cub, Squandered Resources,
/// Wizard's Rockets...), its nonland permanents and every nonland card.
struct Pools {
    lands: Vec<&'static str>,
    mana: Vec<&'static str>,
    nonland: Vec<&'static str>,
    permanents: Vec<&'static str>,
}

fn pools() -> Pools {
    use engine::db::types;
    let db = db();
    let mut p = Pools { lands: Vec::new(), mana: Vec::new(), nonland: Vec::new(), permanents: Vec::new() };
    for name in db.supported() {
        let d = db.def(db.id(name).unwrap());
        if d.is(types::LAND) {
            p.lands.push(name);
            continue;
        }
        p.nonland.push(name);
        if d.is(types::ARTIFACT | types::CREATURE | types::ENCHANTMENT | types::PLANESWALKER | types::BATTLE) {
            p.permanents.push(name);
            if d.activated.iter().any(|a| a.mana().is_some()) || !d.extra_mana_on_tap.is_empty() || d.extra_mana_creature != [0; 6] {
                p.mana.push(name);
            }
        }
    }
    p
}

/// A random board (`seed`): for P0 lands, mana permanents (Wizard's Rockets
/// and Urza one time in four each; the counters a mana ability removes),
/// sometimes another permanent (a tax, a lock), cards in hand and
/// graveyard, mana in the pool, a low life total; for P1 permanents, lands
/// and cards in hand (targets, taxes, responses). Nonland names are
/// distinct (no legend rule).
fn random_board(seed: u64, pools: &Pools, log: bool) -> (Game, String) {
    use engine::rng::Rng;
    let db = db();
    let mut rng = Rng::new(seed ^ 0xCA57_9A1D);
    let pick = |v: &[&'static str], rng: &mut Rng| v[rng.below(v.len() as u32) as usize];
    let add = |v: &mut Vec<&'static str>, n: &'static str, other: &[&str]| {
        if pools.lands.contains(&n) || !(v.contains(&n) || other.contains(&n)) {
            v.push(n);
        }
    };
    let (mut bf0, mut bf1): (Vec<&'static str>, Vec<&'static str>) = (Vec::new(), Vec::new());
    for _ in 0..1 + rng.below(8) {
        let n = pick(&pools.lands, &mut rng);
        add(&mut bf0, n, &[]);
    }
    for _ in 0..rng.below(5) {
        let n = pick(&pools.mana, &mut rng);
        add(&mut bf0, n, &[]);
    }
    if rng.below(4) == 0 {
        add(&mut bf0, "Wizard's Rockets", &[]);
    }
    if rng.below(4) == 0 {
        add(&mut bf0, "Urza, Lord High Artificer", &[]);
    }
    if rng.below(3) == 0 {
        let n = pick(&pools.permanents, &mut rng);
        add(&mut bf0, n, &[]);
    }
    for _ in 0..rng.below(4) {
        let n = pick(&pools.permanents, &mut rng);
        add(&mut bf1, n, &bf0);
    }
    for _ in 0..rng.below(4) {
        let n = pick(&pools.lands, &mut rng);
        add(&mut bf1, n, &bf0);
    }
    // The counters the permanents' abilities remove (Pentad Prism, Gemstone
    // Mine, ...), one to three uses' worth.
    let mut counters: Vec<(&str, engine::db::Counter, u32)> = Vec::new();
    for &n in &bf0 {
        let d = db.def(db.id(n).unwrap());
        if let Some((k, m)) = d.activated.iter().find_map(|a| a.cost.remove_counters) {
            if !counters.iter().any(|c| c.0 == n) {
                counters.push((n, k, m as u32 * (1 + rng.below(3))));
            }
        }
    }
    let mut hand: Vec<&str> = (0..1 + rng.below(3)).map(|_| pick(&pools.nonland, &mut rng)).collect();
    if rng.below(3) == 0 {
        hand.push(pick(&pools.mana, &mut rng));
    }
    let hand1: Vec<&str> = (0..rng.below(3)).map(|_| pick(&pools.nonland, &mut rng)).collect();
    let gy: Vec<&str> = if rng.below(2) == 0 { (0..rng.below(7)).map(|_| pick(&pools.nonland, &mut rng)).collect() } else { Vec::new() };
    let mut pool = [0u8; 6];
    if rng.below(4) == 0 {
        for _ in 0..1 + rng.below(3) {
            pool[rng.below(6) as usize] += 1;
        }
    }
    let life = if rng.below(4) == 0 { 1 + rng.below(6) as i32 } else { 20 };
    let desc = format!(
        "P0 battlefield {bf0:?} (counters {counters:?}), hand {hand:?}, graveyard {gy:?}, pool {pool:?}, life {life}; P1 battlefield {bf1:?}, hand {hand1:?}"
    );
    let mut s = Scenario::new(db)
        .spec(|s| s.log = log)
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
        .battlefield(0, &bf0)
        .battlefield(1, &bf1)
        .hand(0, &hand)
        .hand(1, &hand1)
        .graveyard(0, &gy)
        .pool(0, pool, 0)
        .life(0, life);
    for (n, k, m) in counters {
        s = s.counters(0, n, k, m);
    }
    (s.at(Step::Main1, 0).start(), desc)
}

/// The decisions of a path, described, and the end of the game's log before
/// a panic (`CASTPAY_LOG=1`).
type Trace = std::cell::RefCell<Vec<String>>;

/// `g.apply(a)`; with a trace, the action described first and, if it
/// panics, the end of the game's log before it.
fn step(g: &mut Game, a: u16, trace: Option<&Trace>) {
    let Some(t) = trace else {
        g.apply(a, &mut Vec::new()).unwrap();
        return;
    };
    t.borrow_mut().push(format!("{a} = {}", g.describe_action(a).unwrap_or_default()));
    let before = g.clone();
    if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.apply(a, &mut Vec::new()).unwrap())) {
        let lines = before.log_lines();
        if !lines.is_empty() {
            t.borrow_mut().push(format!("(the log before it)\n      {}", lines[lines.len().saturating_sub(40)..].join("\n      ")));
        }
        std::panic::resume_unwind(p);
    }
}

/// One path from `g` to the next priority (at most 400 decisions): with
/// `policy` 0 always the first option, 1 always the last (the largest X,
/// the last source); otherwise random, the largest number of a
/// `ChooseMode` one time in two.
fn follow(g: &mut Game, rng: &mut engine::rng::Rng, policy: u32, trace: Option<&Trace>) {
    let mut legal = Vec::new();
    for _ in 0..400 {
        if g.is_over() || at_priority(g) {
            break;
        }
        g.legal_actions(&mut legal);
        let a = match policy {
            0 => legal[0],
            1 => *legal.last().unwrap(),
            _ if g.kind() == Some(DecisionKind::ChooseMode) && rng.below(2) == 0 => *legal.last().unwrap(),
            _ => legal[rng.below(legal.len() as u32) as usize],
        };
        step(g, a, trace);
    }
}

/// From `g` (after an action), to P0's priority in the first main phase
/// with an empty stack: the stack resolves (each player passes three times
/// in four, else acts at random: P1 may respond, with its "unless pays"
/// and its counters; every decision random). `false` if the game moved on
/// (or ended).
fn settle(g: &mut Game, rng: &mut engine::rng::Rng, trace: Option<&Trace>) -> bool {
    let mut legal = Vec::new();
    for _ in 0..600 {
        if g.is_over() || g.turn().step != Step::Main1 || g.turn().active != 0 {
            return false;
        }
        if at_priority(g) && g.to_move() == Some(0) && g.stack().is_empty() {
            return true;
        }
        g.legal_actions(&mut legal);
        let a = if at_priority(g) && rng.below(4) != 0 && legal.contains(&engine::act::PASS) {
            engine::act::PASS
        } else {
            legal[rng.below(legal.len() as u32) as usize]
        };
        step(g, a, trace);
    }
    false
}

/// A panic's message.
fn message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// The rounds of a board: its options checked, then one taken at random and
/// the stack resolved, three times.
const ROUNDS: u32 = 3;

/// Board `seed`: at P0's priority, every spell and ability offered, with
/// `paths` paths each to the next priority; then a random action (a land, a
/// spell, an ability) and the stack resolved; `ROUNDS` times. The failures.
fn check_board(seed: u64, pools: &Pools, paths: u32, stats: &mut [u64; 4]) -> Vec<String> {
    use engine::rng::Rng;
    // (`CASTPAY_LOG=1`: the game's log, its end in each failure.)
    let logged = std::env::var("CASTPAY_LOG").is_ok();
    let (g0, desc) = random_board(seed, pools, logged);
    let mut rng = Rng::new(seed ^ 0x5EED);
    // (A trigger or a choice at the start: answered at random.)
    let mut g = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut g = g0.clone();
        follow(&mut g, &mut Rng::new(seed ^ 0x5EED), 2, None);
        g
    })) {
        Ok(g) => g,
        Err(p) => return vec![format!("board {seed} ({desc}): at the start: {}", message(p).chars().take(700).collect::<String>())],
    };
    let mut history: Vec<String> = Vec::new();
    let mut fails = Vec::new();
    for round in 0..ROUNDS {
        if g.is_over() || !at_priority(&g) || g.to_move() != Some(0) || !g.stack().is_empty() {
            break;
        }
        stats[0] += 1;
        let opts: Vec<(u16, Choice)> = g.pending_decision().unwrap().options.iter().map(|o| (o.action, o.choice)).collect();
        for &(a, c) in &opts {
            if matches!(c, Choice::Pass | Choice::Play(_) | Choice::PlayFace(_)) {
                continue;
            }
            stats[1] += 1;
            stats[2] += matches!(c, Choice::Cast(_) | Choice::CastFace(_) | Choice::Faces(..)) as u64;
            for k in 0..paths {
                stats[3] += 1;
                let path_seed = seed.wrapping_mul(0x9E37_79B9) ^ ((a as u64) << 32) ^ ((round as u64) << 24) ^ k as u64;
                let run = |trace: Option<&Trace>| {
                    let mut h = g.clone();
                    step(&mut h, a, trace);
                    follow(&mut h, &mut Rng::new(path_seed), k, trace);
                };
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(None))) {
                    let err: String = message(p).chars().take(700).collect();
                    let trace = Trace::default();
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(Some(&trace))));
                    fails.push(format!(
                        "board {seed} ({desc}), round {round} after {history:?}: option {a} ({}) path {k}: {err}\n  path:\n    {}",
                        g.describe_action(a).unwrap_or_default(),
                        trace.into_inner().join("\n    ")
                    ));
                    break;
                }
            }
        }
        if !fails.is_empty() {
            break;
        }
        // A random action other than passing, and the stack resolved.
        let acts: Vec<u16> = opts.iter().filter(|o| o.1 != Choice::Pass).map(|o| o.0).collect();
        if acts.is_empty() {
            break;
        }
        let a = acts[rng.below(acts.len() as u32) as usize];
        let step_seed = seed.wrapping_mul(0x2545_F491) ^ ((round as u64) << 40) ^ a as u64;
        let run = |trace: Option<&Trace>| {
            let mut h = g.clone();
            step(&mut h, a, trace);
            let ok = settle(&mut h, &mut Rng::new(step_seed), trace);
            (h, ok)
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(None))) {
            Ok((h, ok)) => {
                history.push(g.describe_action(a).unwrap_or_default());
                g = h;
                if !ok {
                    break;
                }
            }
            Err(p) => {
                let err: String = message(p).chars().take(700).collect();
                let trace = Trace::default();
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(Some(&trace))));
                fails.push(format!(
                    "board {seed} ({desc}), round {round} after {history:?}: playing on: {err}\n  path:\n    {}",
                    trace.into_inner().join("\n    ")
                ));
                break;
            }
        }
    }
    fails
}

/// ENGINE.md 49.4: on random boards of the cube's mana sources, lands and
/// permanents, with random hands, graveyards, pools and life totals, every
/// spell and ability offered at priority can be completed along random
/// paths (the largest X, the last source, the first, random answers): the
/// option offered and its payment agree. Three rounds per board, a random
/// action and the stack resolved between them (the opponent responding at
/// random). `CASTPAY_BOARDS=n` boards (default 4,000; `CASTPAY_PATHS`, the
/// paths per option, default 6; `CASTPAY_FROM=k`: from board k on, to
/// replay one with `CASTPAY_BOARDS=1 CASTPAY_LOG=1`).
#[test]
fn castability_and_payment_agree_on_random_cube_boards() {
    let boards: u64 = std::env::var("CASTPAY_BOARDS").ok().and_then(|v| v.parse().ok()).unwrap_or(4_000);
    let paths: u32 = std::env::var("CASTPAY_PATHS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
    let pools = pools();
    let (mut fails, mut stats) = (Vec::new(), [0u64; 4]);
    let from: u64 = std::env::var("CASTPAY_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for seed in from..from + boards {
        fails.extend(check_board(seed, &pools, paths, &mut stats));
    }
    println!(
        "{boards} boards: {} priority decisions of P0 checked, {} options ({} casts), {} paths; {} failures",
        stats[0],
        stats[1],
        stats[2],
        stats[3],
        fails.len()
    );
    assert!(fails.is_empty(), "{} failures:\n{}", fails.len(), fails.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}
