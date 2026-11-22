//! Legacy Hogaak Tempo, lane A (ENGINE.md 220.1): Hogaak, Arisen Necropolis
//! against its Oracle text, the Comprehensive Rules and its official rulings
//! (`// ruling: <hash8>`, `data/rulings/legacy.rulings.json`), and its
//! interactions with the deck's other cards that are on this branch. It is in
//! the card table of constructed play only (`CardDb::constructed`). No test
//! names an identity number (the legacy registry's lines are assigned at
//! integration).
//!
//! Hogaak, Arisen Necropolis {5}{B/G}{B/G}, Legendary Creature — Avatar 8/8:
//! "You can't spend mana to cast this spell. / Convoke, delve (Each creature
//! you tap while casting this spell pays for {1} or one mana of that
//! creature's color. Each card you exile from your graveyard pays for {1}.) /
//! You may cast this card from your graveyard. / Trample"

use engine::az::encode::{self, ctx, perm, CODE, O_CONTEXT};
use engine::az::tail_encode::Row;
use engine::db::CardDb;
use engine::decision::act;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const HOGAAK: &str = "Hogaak, Arisen Necropolis";
/// Green 2/2 (vanilla on the battlefield).
const GREEN: &str = "Elvish Spirit Guide";
/// Red 2/2 (vanilla on the battlefield).
const RED: &str = "Simian Spirit Guide";
/// Colourless 3/3 artifact creature (vanilla in a game).
const GREY: &str = "Cogwork Librarian";
/// Black 1/1 with flash; "When this creature enters, ..." and "Whenever an
/// opponent draws a card except the first one they draw in each of their draw
/// steps, ..." (amass Orcs 1).
const BOWMASTERS: &str = "Orcish Bowmasters";

/// `ChooseAltCost`'s answer for "without paying its mana cost" (the last
/// `NUMBER`, `eoe_c::FREE_FIRST_SPELL`).
const FREE: u8 = (act::NUMBER_N - 1) as u8;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
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

fn all_bf(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.obj(c).controller == p && g.name(c) == name).collect()
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

/// Is Hogaak (in `zone`) castable now?
fn castable(g: &Game, p: PlayerId, zone: Zone) -> bool {
    g.has_option(Choice::Cast(g.card(p, HOGAAK, zone)))
}

/// Begin casting `p`'s Hogaak from `zone` (the cast, then the decisions up to
/// the first payment decision, answered with the first option).
fn begin_hogaak(g: &mut Game, p: PlayerId, zone: Zone) -> CardIdx {
    let c = g.card(p, HOGAAK, zone);
    assert!(g.has_option(Choice::Cast(c)), "Hogaak castable: {}\n{}", opts(g), log(g));
    g.choose(Choice::Cast(c));
    c
}

/// Cast Hogaak from `zone`, answering with `answer` (else the first option),
/// until priority.
fn cast_hogaak_with(g: &mut Game, p: PlayerId, zone: Zone, answer: &dyn Fn(&Game) -> Option<Choice>) -> CardIdx {
    let c = begin_hogaak(g, p, zone);
    until_priority(g, answer);
    c
}

fn cast_hogaak(g: &mut Game, p: PlayerId, zone: Zone) -> CardIdx {
    cast_hogaak_with(g, p, zone, &|_| None)
}

/// The cards an option list offers (`Choice::Card`).
fn cards_offered(g: &Game) -> Vec<CardIdx> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Card(c) => Some(c),
            _ => None,
        })
        .collect()
}

/// Is the pending decision Hogaak's convoke (a permanent to tap)?
fn convoking(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::ChoosePermanent)
}

/// Is the pending decision Hogaak's delve (a graveyard card or `PASS`)?
fn delving(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::ChooseGraveyardCard)
}

/// `v`'s layout v7 row head.
fn head(g: &Game, v: PlayerId) -> Vec<f32> {
    Row::of(g, v).head
}

/// The status float `k` of permanent `c` in `v`'s row head (its controller's
/// side of the board: `MY_PERM` for the viewer's, `OPP_PERM` for the other's).
fn perm_float(g: &Game, v: PlayerId, c: CardIdx, k: usize) -> f32 {
    let ctl = g.obj(c).controller;
    let s = g.permanents_of(ctl).position(|x| x == c).unwrap();
    let base = if ctl == v { encode::O_MY_PERM } else { encode::O_OPP_PERM };
    head(g, v)[base + s * encode::PERM_W + CODE + k]
}

/// Pay a Hogaak being cast: at each delve decision the next card of `delve`
/// (in graveyard order), then `PASS`; at each convoke decision the next
/// creature of `convoke` (in battlefield order). Every pick must be offered;
/// a convoke decision offers creatures only and a delve decision graveyard
/// cards (or `PASS`). Stops at priority.
fn pay(g: &mut Game, p: PlayerId, delve: &[CardIdx], convoke: &[CardIdx]) {
    let gy_pos = |g: &Game, c: CardIdx| g.player(p).graveyard.iter().position(|&x| x == c).unwrap();
    let mut delve = delve.to_vec();
    delve.sort_by_key(|&c| gy_pos(g, c));
    let mut convoke = convoke.to_vec();
    convoke.sort_by_key(|&c| g.permanents_of(p).position(|x| x == c).unwrap());
    let (mut d, mut k) = (0, 0);
    for _ in 0..100 {
        if at_priority(g) {
            assert_eq!((d, k), (delve.len(), convoke.len()), "every pick made: {}", log(g));
            return;
        }
        if delving(g) {
            for c in cards_offered(g) {
                assert_eq!(g.obj(c).zone, Zone::Graveyard, "delve offers graveyard cards: {}", opts(g));
            }
            if d < delve.len() {
                assert!(g.has_option(Choice::Card(delve[d])), "delve {} offered: {}", g.name(delve[d]), opts(g));
                g.choose(Choice::Card(delve[d]));
                d += 1;
            } else {
                g.pass();
            }
            continue;
        }
        assert!(convoking(g), "a payment decision: {:?} {}
{}", g.kind(), opts(g), log(g));
        for c in cards_offered(g) {
            assert!(g.is_creature(c) && g.obj(c).zone == Zone::Battlefield, "convoke offers creatures: {}", opts(g));
        }
        assert!(k < convoke.len(), "more convoke picks than {}: {}
{}", convoke.len(), opts(g), log(g));
        assert!(g.has_option(Choice::Card(convoke[k])), "convoke {} offered: {}
{}", g.name(convoke[k]), opts(g), log(g));
        g.choose(Choice::Card(convoke[k]));
        k += 1;
    }
    panic!("the payment doesn't end: {}
{}", opts(g), log(g));
}

/// `p`'s creatures on the battlefield named `name`, in battlefield order.
fn creatures(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    all_bf(g, p, name)
}

/// `p`'s graveyard cards named `name`, in graveyard order.
fn gy(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.player(p).graveyard.iter().copied().filter(|&c| g.name(c) == name).collect()
}

// ---- the card ----------------------------------------------------------------

#[test]
fn hogaak_is_a_legendary_8_8_avatar_with_trample_convoke_delve_and_its_two_casting_rules() {
    let d = db().def(def(HOGAAK));
    assert_eq!(d.mana_value(), 7);
    assert_eq!((d.power, d.toughness), (Some(8), Some(8)));
    assert!(d.types & engine::db::types::CREATURE != 0);
    assert!(d.supertypes & engine::db::supertypes::LEGENDARY != 0);
    assert!(d.keywords.has(engine::db::Kw::Trample));
    assert!(d.casting.convoke && d.delve && d.casting.own_gy_cast && d.casting.no_mana);
    assert_eq!(format!("{}", d.cost), "{5}{B/G}{B/G}");
    assert_eq!(d.cost.colors(), (1 << 2) | (1 << 4), "black and green (CR 105.2, 202.2: its hybrid pips)");
    assert!(db().unsupported().is_empty(), "{:?}", db().unsupported());
}

// ---- "You can't spend mana to cast this spell" -------------------------------

#[test]
fn cr_601_2h_hogaak_is_paid_by_convoke_alone_never_by_mana() {
    // Seven creatures, two of them green, and ten untapped lands: Hogaak is
    // cast by tapping all seven creatures; no land is ever offered (only the
    // empty graveyard's delve PASS, then creatures) and none is tapped.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Forest", "Forest", "Forest", "Swamp", "Swamp", "Forest", "Forest"])
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    for c in g.battlefield().to_vec() {
        if g.obj(c).controller == 0 {
            assert_eq!(g.obj(c).tapped, g.is_creature(c), "{} tapped iff a creature", g.name(c));
        }
    }
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_601_2h_lands_and_floating_mana_never_make_hogaak_castable() {
    // Six creatures (one short), ten lands and seven mana floating: not
    // castable; with a seventh creature it is, and the floating mana stays in
    // the pool (none spent on it), to be spent on something else.
    let g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Forest", "Forest", "Forest", "Swamp", "Swamp", "Forest", "Forest"])
        .battlefield(0, &[GREEN, GREEN, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .pool(0, [0, 0, 4, 0, 3, 0], 0)
        .at(Step::Main1, 0)
        .start();
    assert!(!castable(&g, 0, Zone::Hand), "{}", opts(&g));
    let mut g2 = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, GREY, GREY, RED])
        .hand(0, &[HOGAAK])
        .pool(0, [0, 0, 4, 0, 3, 0], 0)
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g2, 0, Zone::Hand), "{}", opts(&g2));
    cast_hogaak(&mut g2, 0, Zone::Hand);
    assert_eq!(g2.player(0).pool.amt, [0, 0, 4, 0, 3, 0], "no mana spent: {}", log(&g2));
    assert!(g2.stack().iter().any(|s| matches!(s.kind, engine::game::StackKind::Spell { card } if g2.name(card) == HOGAAK)));
}

// ---- convoke (CR 702.51) -----------------------------------------------------

#[test]
fn cr_107_4e_702_51a_a_hybrid_pip_takes_a_black_or_a_green_creature() {
    // Seven creatures but only one green: not castable (two {B/G} pips need
    // two black or green creatures); a red, blue or colourless creature pays
    // {1} only.
    let one_green = scen().battlefield(0, &[GREEN, RED, RED, RED, GREY, GREY, GREY]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start();
    assert!(!castable(&one_green, 0, Zone::Hand), "{}", opts(&one_green));
    // A black and a green creature pay the two pips.
    let mut g = scen()
        .battlefield(0, &["Dark Confidant", GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
}

#[test]
fn cr_702_51a_each_pick_leaves_the_rest_payable() {
    // Seven creatures, exactly two green: the payment never offers a pick
    // that leaves the two pips unpaid. Picked in battlefield order, the reds
    // and colourless ones come first; once five non-green creatures are
    // picked, only the greens are left - and a sixth non-green creature never
    // is offered (there is none), while with eight creatures (three reds
    // spare) a pick of the fifth red leaves too few greens? no: the greens
    // stay. Here: with the greens last on the battlefield, every prefix is
    // completable, and no decision offers a creature whose pick would make
    // the greens insufficient.
    let mut g = scen()
        .battlefield(0, &[RED, RED, RED, GREY, GREY, RED, GREEN, GREEN])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    begin_hogaak(&mut g, 0, Zone::Hand);
    g.pass(); // (no delve: an empty graveyard)
    let reds: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c) && g.name(c) != GREEN).collect();
    let greens = creatures(&g, 0, GREEN);
    assert_eq!(reds.len(), 6);
    // Five of the six non-green ones, then the greens: after the fifth no
    // non-green creature is offered (both greens must pay the pips).
    for &c in &reds[..5] {
        assert!(convoking(&g) && g.has_option(Choice::Card(c)), "{}", opts(&g));
        g.choose(Choice::Card(c));
    }
    assert_eq!(cards_offered(&g), vec![greens[0]], "only the greens now: {}", opts(&g));
    g.choose(Choice::Card(greens[0]));
    assert_eq!(cards_offered(&g), vec![greens[1]], "{}", opts(&g));
    g.choose(Choice::Card(greens[1]));
    assert!(at_priority(&g), "{}", opts(&g));
    assert!(!g.obj(reds[5]).tapped, "the sixth non-green creature stays untapped");
}

#[test]
fn cr_702_51a_a_multicoloured_creature_pays_either_of_its_colours() {
    // ruling: dbf4ec31
    // Uro (green and blue) and Fallen Shinobi (blue and black) pay the two
    // {B/G} pips, one each; five red or colourless creatures the {5}.
    let mut g = scen()
        .battlefield(0, &["Uro, Titan of Nature's Wrath", "Fallen Shinobi", RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
    // Alpha Deathclaw (black and green) is one creature: one pip only.
    let g = scen().battlefield(0, &["Alpha Deathclaw", RED, RED, RED, GREY, GREY, GREY]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start();
    assert!(!castable(&g, 0, Zone::Hand), "one black-green creature pays one pip: {}", opts(&g));
}

#[test]
fn cr_302_6_702_51a_summoning_sick_creatures_convoke() {
    // ruling: fbdd4f47
    // Three Llanowar Elves cast this turn (summoning sick: their {T} mana
    // ability can't be activated, CR 302.6) convoke with four other creatures.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", GREEN, RED, RED, GREY])
        .hand(0, &[HOGAAK, "Llanowar Elves", "Llanowar Elves", "Llanowar Elves"])
        .at(Step::Main1, 0)
        .start();
    assert!(!castable(&g, 0, Zone::Hand), "four creatures: {}", opts(&g));
    for _ in 0..3 {
        let e = g.card(0, "Llanowar Elves", Zone::Hand);
        g.choose(Choice::Cast(e));
        until_priority(&mut g, &|_| None);
        resolve(&mut g);
    }
    let elves = creatures(&g, 0, "Llanowar Elves");
    assert_eq!(elves.len(), 3);
    assert!(elves.iter().all(|&c| g.is_sick(c)), "{}", log(&g));
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    assert!(elves.iter().all(|&c| g.obj(c).tapped));
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
}

#[test]
fn cr_702_51a_a_creature_tapped_for_mana_first_cant_convoke_and_one_sacrificed_is_gone() {
    // ruling: 078779fc
    // Sylvan Caryatid ("{T}: Add one mana of any color") tapped for mana with
    // priority (CR 605.3a, 117.1d; its mana floats) is tapped when Hogaak is
    // cast: it can't convoke. With it untapped, six other creatures and it
    // pay; tapped, the six don't.
    let mut g = scen()
        .battlefield(0, &["Sylvan Caryatid", GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    // Float its mana: `YES` (activate mana abilities), the Caryatid, a colour.
    g.choose(Choice::Yes);
    let car = bf(&g, 0, "Sylvan Caryatid");
    g.choose(Choice::Card(car));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ActivateMana)).then_some(Choice::Pass));
    assert!(g.obj(car).tapped, "{}", log(&g));
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1);
    assert!(!castable(&g, 0, Zone::Hand), "the Caryatid is tapped: six creatures left: {}", opts(&g));
    // A creature sacrificed for mana ("Test Blood Imp": "Sacrifice this
    // creature: Add {B}.") is not on the battlefield when the cost is paid:
    // no convoke candidate. (Its card, now in the graveyard, is delve fuel.)
    let mut g = Scenario::new(flash_db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
        .battlefield(0, &["Test Blood Imp", GREEN, GREEN, RED, RED, RED, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let imp = bf(&g, 0, "Test Blood Imp");
    g.choose(Choice::Yes);
    g.choose(Choice::Card(imp));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ActivateMana)).then_some(Choice::Pass));
    assert_eq!(g.obj(imp).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.player(0).pool.amt[2], 1);
    begin_hogaak(&mut g, 0, Zone::Hand);
    assert!(delving(&g) && g.has_option(Choice::Card(imp)), "{}", opts(&g));
    g.choose(Choice::Card(imp));
    g.pass(); // (no other card to delve)
    assert!(convoking(&g), "{}", opts(&g));
    assert!(!cards_offered(&g).contains(&imp));
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    assert_eq!(all.len(), 6);
    pay(&mut g, 0, &[], &all);
    assert_eq!(g.obj(imp).zone, Zone::Exile);
    assert_eq!(g.player(0).pool.amt[2], 1, "the Imp's {{B}} unspent: {}", log(&g));
}

#[test]
fn cr_702_51a_every_set_of_creatures_and_graveyard_cards_that_pays_is_one_decision_path() {
    // Five creatures (two green: the pips), three graveyard cards: delve d
    // cards and convoke 7 - d creatures. d = 2: all five creatures, any two of
    // the three cards (3 outcomes); d = 3: both greens and two of the three
    // others (3 outcomes). Six outcomes, each reached by exactly one path of
    // decisions (delve's picks go up the graveyard, convoke's up the
    // battlefield); no payment spends mana.
    let g = scen()
        .battlefield(0, &[GREEN, RED, GREEN, RED, GREY, "Swamp", "Forest"])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let gy0: Vec<CardIdx> = g.player(0).graveyard.clone();
    let mut g0 = g.clone();
    begin_hogaak(&mut g0, 0, Zone::Hand);
    let mut outcomes: Vec<(Vec<CardIdx>, Vec<CardIdx>)> = Vec::new();
    let mut paths = 0;
    let mut stack = vec![g0];
    while let Some(g) = stack.pop() {
        if at_priority(&g) {
            paths += 1;
            let tapped: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.obj(c).tapped).collect();
            let exiled: Vec<CardIdx> = gy0.iter().copied().filter(|&c| g.obj(c).zone == Zone::Exile).collect();
            assert!(tapped.iter().all(|&c| g.is_creature(c)), "only creatures tapped: {}", log(&g));
            outcomes.push((tapped, exiled));
            continue;
        }
        for c in g.offered() {
            let mut w = g.clone();
            w.choose(c);
            stack.push(w);
        }
    }
    let n = outcomes.len();
    outcomes.sort();
    outcomes.dedup();
    assert_eq!((paths, n, outcomes.len()), (6, 6, 6), "{outcomes:?}");
    for (t, e) in &outcomes {
        assert_eq!(t.len() + e.len(), 7);
    }
}

// ---- delve (CR 702.66), with convoke ------------------------------------------

#[test]
fn cr_702_66a_delve_and_convoke_pay_one_cost_together() {
    // ruling: c6f70acd
    // Seven graveyard cards and two green creatures: delve pays the {5} (no
    // more than the generic mana: five cards at most), the greens the pips.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Force of Will", "Island", "Swamp", "Wasteland"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    // Five picks: the first leaves enough cards above it for the four still
    // needed (the creatures can't pay generic mana here: the pips need both).
    for k in 0..5 {
        assert!(delving(&g), "{}", opts(&g));
        assert!(!g.has_option(Choice::Pass), "more cards needed: {}", opts(&g));
        g.choose(Choice::Card(gyc[k]));
    }
    // No sixth card: delve pays generic mana only.
    assert!(convoking(&g), "convoke next: {:?} {}", g.kind(), opts(&g));
    let greens = creatures(&g, 0, GREEN);
    pay(&mut g, 0, &[], &greens);
    assert!(gyc[..5].iter().all(|&c| g.obj(c).zone == Zone::Exile));
    assert!(gyc[5..].iter().all(|&c| g.obj(c).zone == Zone::Graveyard));
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
    // One green creature and seven cards: the second pip is unpayable.
    let g = scen()
        .battlefield(0, &[GREEN, RED])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Force of Will", "Island", "Swamp", "Wasteland"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(!castable(&g, 0, Zone::Hand), "{}", opts(&g));
}

#[test]
fn cr_702_66a_delving_more_leaves_creatures_untapped() {
    // Four graveyard cards, seven creatures (two green): delve any number from
    // none to four; with four, three creatures convoke. PASS is offered from
    // the start (the creatures alone can pay).
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    begin_hogaak(&mut g, 0, Zone::Hand);
    assert!(delving(&g) && g.has_option(Choice::Pass), "{}", opts(&g));
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    let greens = creatures(&g, 0, GREEN);
    let red = creatures(&g, 0, RED)[0];
    pay(&mut g, 0, &gyc, &[greens[0], greens[1], red]);
    let tapped = g.permanents_of(0).filter(|&c| g.obj(c).tapped).count();
    assert_eq!(tapped, 3);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
}

#[test]
fn cr_702_66a_delve_doesnt_change_hogaaks_mana_value() {
    // ruling: 4029cc96
    // Cast with five cards delved, Hogaak's mana value is still 7: Fatal Push
    // ("destroy target creature if it has mana value 2 or less; revolt: 4 or
    // less") has no legal target.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Force of Will", "Island"])
        .hand(0, &[HOGAAK])
        .battlefield(1, &["Swamp"])
        .hand(1, &["Fatal Push"])
        .at(Step::Main1, 0)
        .start();
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    begin_hogaak(&mut g, 0, Zone::Hand);
    let greens = creatures(&g, 0, GREEN);
    pay(&mut g, 0, &gyc, &greens);
    resolve(&mut g);
    let h = bf(&g, 0, HOGAAK);
    assert_eq!(db().def(g.obj(h).def).mana_value(), 7);
    // P1's Fatal Push at it destroys nothing: mana value 7, not 2 or less.
    g.pass();
    assert_eq!(g.to_move(), Some(1), "{}", opts(&g));
    let push = g.card(1, "Fatal Push", Zone::Hand);
    g.choose(Choice::Cast(push));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(h)));
    resolve(&mut g);
    assert!(g.obj(h).zone == Zone::Battlefield, "{}", log(&g));
}

#[test]
fn cr_118_9_702_66a_delve_and_convoke_with_an_alternative_cost_and_a_cost_increase() {
    // ruling: 3719645d
    // ruling: 1dce867e
    // ruling: 0a2cf1a9
    // Disruptor Flute naming Hogaak: "spells with the chosen name cost {3}
    // more" (CR 601.2f): the total cost is {8}{B/G}{B/G}, all of it convoke's
    // and delve's. With Omniscience ("without paying its mana cost", an
    // alternative cost, CR 118.9) the total cost is {3}: convoke and delve
    // still pay it (neither is an alternative cost), and no mana does.
    // P0 casts its own Flute (it taxes every player's spells) and names
    // Hogaak.
    let flute_named = |g: &mut Game| {
        let f = g.card(0, "Disruptor Flute", Zone::Hand);
        g.choose(Choice::Cast(f));
        until_priority(g, &|_| None);
        g.pass_until(|g| g.kind() == Some(DecisionKind::NameCard));
        g.choose(Choice::Def(def(HOGAAK)));
        resolve(g);
        assert!(on_bf(g, 0, "Disruptor Flute"), "{}", log(g));
    };
    // Ten units: seven creatures and three cards.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY, "Swamp", "Swamp", "Swamp"])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze"])
        .hand(0, &[HOGAAK, "Disruptor Flute"])
        .at(Step::Main1, 0)
        .start();
    flute_named(&mut g);
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    let mut short = g.clone();
    short.set_tapped(creatures(&short, 0, GREY)[0], true);
    short.refresh_decision();
    assert!(!castable(&short, 0, Zone::Hand), "nine units for ten: {}", opts(&short));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &gyc, &all);
    // (Two Swamps paid for the Flute; the third stays untapped.)
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Swamp" && g.obj(c).tapped).count(), 2, "no land tapped for Hogaak: {}", log(&g));
    // Omniscience: the free cast's total cost is the Flute's {3}.
    let mut g = scen()
        .battlefield(0, &["Omniscience", RED, "Swamp", "Swamp", "Swamp"])
        .graveyard(0, &["Ponder", "Brainstorm"])
        .hand(0, &[HOGAAK, "Disruptor Flute"])
        .at(Step::Main1, 0)
        .start();
    flute_named(&mut g);
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost), "{}", opts(&g));
    // (The mana cost isn't payable: one creature and two cards for {8}{B/G}{B/G}.)
    assert_eq!(g.offered(), vec![Choice::Number(FREE)], "{}", opts(&g));
    g.choose(Choice::Number(FREE));
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    let red = creatures(&g, 0, RED);
    pay(&mut g, 0, &gyc, &red);
    assert!(gyc.iter().all(|&c| g.obj(c).zone == Zone::Exile) && g.obj(red[0]).tapped);
    // (Two Swamps paid for the Flute; the third stays untapped.)
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Swamp" && g.obj(c).tapped).count(), 2, "no land for Hogaak: {}", log(&g));
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
    // Omniscience without a tax: nothing to pay at all.
    let mut g = scen().battlefield(0, &["Omniscience"]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start();
    assert!(castable(&g, 0, Zone::Hand));
    begin_hogaak(&mut g, 0, Zone::Hand);
    g.choose(Choice::Number(FREE));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_601_2f_thalia_doesnt_tax_hogaak_a_creature_spell() {
    // Thalia: "Noncreature spells cost {1} more to cast": seven creatures
    // still pay Hogaak.
    let g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
}

#[test]
fn cr_601_2f_spells_cast_from_the_graveyard_cost_less_green_goblin() {
    // Green Goblin (Norman Osborn's back face): "Spells you cast from your
    // graveyard cost {2} less to cast": Hogaak from the graveyard costs
    // {3}{B/G}{B/G}, five units (the reduction takes generic mana, CR 118.7a);
    // with Norman Osborn's front face (no such ability) seven, so six
    // creatures don't cast it.
    let goblin = || {
        scen()
            .battlefield(0, &["Norman Osborn // Green Goblin"])
            .battlefield(0, &[GREEN, GREEN, RED, RED, GREY])
            .graveyard(0, &[HOGAAK])
            .at(Step::Main1, 0)
    };
    let front = goblin().start();
    assert!(!castable(&front, 0, Zone::Graveyard), "{}", opts(&front));
    let mut g = goblin().transformed(0, "Norman Osborn // Green Goblin").start();
    assert!(castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
    let h = cast_hogaak(&mut g, 0, Zone::Graveyard);
    assert_eq!(g.obj(h).zone, Zone::Stack);
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 5, "{}", log(&g));
}

// ---- "You may cast this card from your graveyard" ------------------------------

#[test]
fn cr_601_3_hogaak_is_cast_from_its_owners_graveyard() {
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .graveyard(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
    // (Priority's "cast or play from the graveyard", ENGINE.md 100.5.)
    assert!(g.has_option(Choice::FromGraveyard), "{}", opts(&g));
    let h = cast_hogaak(&mut g, 0, Zone::Graveyard);
    assert_eq!(g.obj(h).zone, Zone::Stack);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_601_3_only_its_owner_casts_it_from_their_own_graveyard() {
    // P0's Hogaak in P0's graveyard: P1, with seven creatures, can't cast it.
    let mut g = scen()
        .graveyard(0, &[HOGAAK])
        .battlefield(1, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .at(Step::Main1, 1)
        .start();
    let h = g.card(0, HOGAAK, Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(h)), "{}", opts(&g));
    assert!(!g.has_option(Choice::FromGraveyard), "{}", opts(&g));
    let _ = &mut g;
}

#[test]
fn cr_307_1_601_3_hogaak_keeps_a_creature_spells_timing_from_the_graveyard() {
    // Not in the opponent's turn, not with a spell on the stack, not outside a
    // main phase: from the graveyard or the hand.
    let board = [GREEN, GREEN, RED, RED, RED, GREY, GREY];
    let g = scen().battlefield(0, &board).graveyard(0, &[HOGAAK]).hand(0, &[HOGAAK]).at(Step::Main1, 1).start();
    let g = {
        let mut g = g;
        g.pass_until(|g| g.to_move() == Some(0) && at_priority(g));
        g
    };
    assert!(!castable(&g, 0, Zone::Graveyard) && !castable(&g, 0, Zone::Hand), "{}", opts(&g));
    let g = scen().battlefield(0, &board).graveyard(0, &[HOGAAK]).at(Step::Upkeep, 0).start();
    let g = {
        let mut g = g;
        g.drive_until(at_priority);
        g
    };
    assert!(!castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
}

#[test]
fn cr_601_2a_a_hogaak_cast_from_the_graveyard_is_on_the_stack_and_cant_delve_itself() {
    // Hogaak and four other cards in the graveyard, three creatures (two
    // green): four cards delved and three creatures - castable. Delve's
    // choices never offer the Hogaak being cast (it is on the stack, CR
    // 601.2a). With only three other cards, not castable.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED])
        .graveyard(0, &["Ponder", HOGAAK, "Brainstorm", "Daze", "Island"])
        .at(Step::Main1, 0)
        .start();
    assert!(castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
    let h = begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert!(delving(&g), "{}", opts(&g));
    assert!(!g.has_option(Choice::Card(h)) && g.obj(h).zone == Zone::Stack, "{}", opts(&g));
    let others: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert_eq!(others.len(), 4);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &others, &all);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
    let g = scen().battlefield(0, &[GREEN, GREEN, RED]).graveyard(0, &["Ponder", HOGAAK, "Brainstorm", "Daze"]).at(Step::Main1, 0).start();
    assert!(!castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
}

#[test]
fn cr_702_66a_another_hogaak_in_the_graveyard_is_delve_fuel() {
    // Two Hogaaks and three other cards in the graveyard, three creatures (two
    // green): one Hogaak is cast, the other delved (it is a card in the
    // graveyard; the one cast is on the stack).
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED])
        .graveyard(0, &[HOGAAK, "Ponder", HOGAAK, "Brainstorm", "Daze"])
        .at(Step::Main1, 0)
        .start();
    let hs = gy(&g, 0, HOGAAK);
    assert!(g.has_option(Choice::Cast(hs[0])) && g.has_option(Choice::Cast(hs[1])), "{}", opts(&g));
    g.choose(Choice::Cast(hs[1]));
    assert!(delving(&g), "{}", opts(&g));
    assert!(g.has_option(Choice::Card(hs[0])), "the other Hogaak is delve fuel: {}", opts(&g));
    let rest: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert_eq!(rest.len(), 4);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &rest, &all);
    assert_eq!(g.obj(hs[0]).zone, Zone::Exile);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
}

#[test]
fn cr_601_3_yawgmoths_will_and_its_own_permission_are_one_cast() {
    // With Yawgmoth's Will ("you may cast spells from your graveyard") the
    // two permissions allow the same cast (its costs): no route to choose.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY, "Swamp", "Swamp", "Swamp"])
        .hand(0, &["Yawgmoth's Will"])
        .graveyard(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let w = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(w));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert_ne!(g.kind(), Some(DecisionKind::ChooseRoute), "{}", opts(&g));
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_702_138a_underworld_breach_offers_escape_besides_its_own_permission() {
    // ruling: 1dce867e
    // ruling: 0a2cf1a9
    // Underworld Breach gives Hogaak escape ({5}{B/G}{B/G} and three other
    // graveyard cards exiled, an alternative cost): the route is chosen (its
    // own permission or escape). Escaping, its convoke and delve pay the
    // escape cost's mana (neither is an alternative cost), and three other
    // cards are exiled besides.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", GREEN, GREEN, RED, RED, RED])
        .graveyard(0, &[HOGAAK, "Ponder", "Brainstorm", "Daze", "Island", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let h = begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseRoute), "{}", opts(&g));
    // NUMBER + route code: escape (2) and its own permission (10).
    let mut routes: Vec<u8> = g.offered().into_iter().filter_map(|c| if let Choice::Number(k) = c { Some(k) } else { None }).collect();
    routes.sort();
    assert_eq!(routes, vec![2, 10], "{}", opts(&g));
    g.choose(Choice::Number(2));
    // Delve: with three cards kept for escape, two of the five may be delved.
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert_eq!(gyc.len(), 5);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay_then_rest(&mut g, 0, &gyc[..2], &all);
    assert!(gyc.iter().all(|&c| g.obj(c).zone == Zone::Exile), "two delved, three escape's: {}", log(&g));
    assert_eq!(g.obj(h).zone, Zone::Stack);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

/// `pay`, then the cost's other parts (escape's graveyard cards: the first
/// offered each time) until priority.
fn pay_then_rest(g: &mut Game, p: PlayerId, delve: &[CardIdx], convoke: &[CardIdx]) {
    let gy_pos = |g: &Game, c: CardIdx| g.player(p).graveyard.iter().position(|&x| x == c).unwrap();
    let mut delve = delve.to_vec();
    delve.sort_by_key(|&c| gy_pos(g, c));
    let mut convoke = convoke.to_vec();
    convoke.sort_by_key(|&c| g.permanents_of(p).position(|x| x == c).unwrap());
    let (mut d, mut k) = (0, 0);
    for _ in 0..100 {
        if at_priority(g) {
            assert_eq!((d, k), (delve.len(), convoke.len()), "{}", log(g));
            return;
        }
        if delving(g) && d < delve.len() + 1 && k == 0 && g.agenda_tail(1).contains("Delve") {
            if d < delve.len() {
                g.choose(Choice::Card(delve[d]));
            } else {
                g.pass();
            }
            d += 1;
            if d > delve.len() {
                d = delve.len();
            }
            continue;
        }
        if convoking(g) {
            g.choose(Choice::Card(convoke[k]));
            k += 1;
            continue;
        }
        let c = g.offered()[0];
        g.choose(c);
    }
    panic!("the payment doesn't end: {}\n{}", opts(g), log(g));
}

#[test]
fn cr_702_81a_six_gives_it_retrace_and_the_discarded_land_may_be_delved() {
    // Six: "During your turn, nonland permanent cards in your graveyard have
    // retrace" (cast it by discarding a land card besides its other costs,
    // CR 702.81a). The routes are its own permission and retrace; retraced,
    // the land card the cost discards may be exiled by delve once discarded
    // (CR 601.2h: the costs in any order; ENGINE.md 100.4.9).
    let mut g = scen()
        .battlefield(0, &["Six", GREEN, RED])
        .graveyard(0, &[HOGAAK, "Ponder", "Brainstorm", "Daze"])
        .hand(0, &["Island"])
        .at(Step::Main1, 0)
        .start();
    // Own permission: three cards and three creatures, one short. Retrace:
    // the Island too - the only route that completes, so no route is asked.
    let h = begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert_ne!(g.kind(), Some(DecisionKind::ChooseRoute), "{}", opts(&g));
    assert_eq!(g.stack().last().unwrap().from, engine::game::CastFrom::Retrace);
    // Delve: the land in hand promised first (HAND id), then the graveyard.
    let island = g.card(0, "Island", Zone::Hand);
    assert!(delving(&g) && g.has_option(Choice::Card(island)), "{}", opts(&g));
    g.choose(Choice::Card(island));
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    for &c in &gyc {
        g.choose(Choice::Card(c));
    }
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    until_priority(&mut g, &|g| {
        if convoking(g) {
            cards_offered(g).first().map(|&c| Choice::Card(c))
        } else {
            None
        }
    });
    assert!(all.iter().all(|&c| g.obj(c).tapped), "{}", log(&g));
    assert_eq!(g.obj(island).zone, Zone::Exile, "discarded, then exiled by delve: {}", log(&g));
    assert_eq!(g.obj(h).zone, Zone::Stack);
}

// ---- the deck's interactions ----------------------------------------------------

#[test]
fn cr_118_12_daze_is_paid_with_mana_after_hogaak_is_cast() {
    // Hogaak's restriction is on casting it: Daze's "counter target spell
    // unless its controller pays {1}" is paid with a land's mana.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY, "Swamp"])
        .hand(0, &[HOGAAK])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    let h = cast_hogaak(&mut g, 0, Zone::Hand);
    g.pass();
    assert_eq!(g.to_move(), Some(1), "{}", opts(&g));
    let daze = g.card(1, "Daze", Zone::Hand);
    g.choose(Choice::Cast(daze));
    until_priority(&mut g, &|g| match g.kind() {
        // Its alternative cost: return the Island.
        Some(DecisionKind::ChooseAltCost) => g.offered().into_iter().find(|c| *c != Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(h)),
        _ => None,
    });
    // Both pass: Daze resolves and P0 is asked to pay {1}: yes, with the Swamp.
    g.pass_until(|g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}\n{}", opts(&g), log(&g));
    g.choose(Choice::Yes);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "paid: {}", log(&g));
    assert!(g.obj(bf(&g, 0, "Swamp")).tapped);
}

#[test]
fn cr_601_3_force_of_will_counters_it_then_it_is_cast_again_from_the_graveyard() {
    // Countered by Force of Will (pitching Brainstorm), Hogaak goes to its
    // owner's graveyard; its owner casts it again from there with five cards
    // delved and two more green creatures.
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY, GREEN, GREEN])
        .graveyard(0, &["Ponder", "Daze", "Island", "Swamp", "Wasteland"])
        .hand(0, &[HOGAAK])
        .hand(1, &["Force of Will", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let first: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).take(7).collect();
    let h = begin_hogaak(&mut g, 0, Zone::Hand);
    pay(&mut g, 0, &[], &first);
    g.pass();
    let fow = g.card(1, "Force of Will", Zone::Hand);
    g.choose(Choice::Cast(fow));
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => g.offered().into_iter().find(|c| *c != Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(h)),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(h).zone, Zone::Graveyard, "countered: {}", log(&g));
    assert!(castable(&g, 0, Zone::Graveyard), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Graveyard);
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert_eq!(gyc.len(), 5);
    let last: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c) && !g.obj(c).tapped).collect();
    pay(&mut g, 0, &gyc, &last);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_205_4_karakas_returns_hogaak_to_its_owners_hand() {
    let mut g = scen()
        .battlefield(0, &[HOGAAK, GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .battlefield(1, &["Karakas"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    let k = bf(&g, 1, "Karakas");
    let h = bf(&g, 0, HOGAAK);
    let bounce = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == k)).expect("Karakas's ability");
    g.choose(bounce);
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(h)));
    resolve(&mut g);
    let h = g.card(0, HOGAAK, Zone::Hand);
    // Back in hand: cast again by convoke.
    assert!(castable(&g, 0, Zone::Hand) || g.to_move() != Some(0), "{}", opts(&g));
    let _ = h;
}

#[test]
fn cr_614_1a_dauthi_voidwalker_leaves_no_delve_fuel_and_exiles_a_countered_hogaak() {
    // P1's Dauthi Voidwalker: a card that would go to P0's graveyard is exiled
    // with a void counter instead. Ponder cast: exiled, no delve fuel. Hogaak
    // countered: exiled, not castable from there by P0; P1 sacrifices the
    // Voidwalker and casts it "without paying its mana cost" (an alternative
    // cost of no mana: Hogaak's restriction is met).
    let mut g = scen()
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY, "Island"])
        .hand(0, &[HOGAAK, "Ponder"])
        .battlefield(1, &["Dauthi Voidwalker", "Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let p = g.card(0, "Ponder", Zone::Hand);
    g.choose(Choice::Cast(p));
    until_priority(&mut g, &|_| None);
    resolve(&mut g);
    assert_eq!(g.obj(p).zone, Zone::Exile, "{}", log(&g));
    let h = begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(h)));
    resolve(&mut g);
    assert_eq!(g.obj(h).zone, Zone::Exile, "exiled with a void counter: {}", log(&g));
    assert!(!g.has_option(Choice::Cast(h)), "{}", opts(&g));
}

#[test]
fn cr_704_5j_the_legend_rule_for_a_second_hogaak() {
    let mut g = scen()
        .battlefield(0, &[HOGAAK, GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let old = bf(&g, 0, HOGAAK);
    let new = cast_hogaak(&mut g, 0, Zone::Hand);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::LegendRule)).then_some(Choice::Card(new)));
    assert_eq!(all_bf(&g, 0, HOGAAK), vec![new], "{}", log(&g));
    assert_eq!(g.obj(old).zone, Zone::Graveyard);
}

#[test]
fn cr_702_19b_hogaak_tramples_over_a_blocker() {
    let mut g = scen().battlefield(0, &[HOGAAK]).battlefield(1, &[RED]).at(Step::DeclareAttackers, 0).start();
    let h = bf(&g, 0, HOGAAK);
    g.choose(Choice::Card(h));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(h));
    g.drive_until(|g| g.kind() == Some(DecisionKind::AssignDamage));
    // Lethal (2) to the blocker, the rest (6) to the player.
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.turn().step > Step::CombatDamage || g.is_over());
    assert_eq!(g.life(1), 14, "{}", log(&g));
}

#[test]
fn cr_702_51a_orcish_bowmasters_army_token_convokes() {
    // Orcish Bowmasters (flash) enters: 1 damage, then amass Orcs 1 - a black
    // 0/0 Orc Army token with a +1/+1 counter. Both black: they pay the pips.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", RED, RED, RED, GREY, GREY])
        .hand(0, &[BOWMASTERS, HOGAAK])
        .at(Step::Main1, 0)
        .start();
    assert!(!castable(&g, 0, Zone::Hand));
    let b = g.card(0, BOWMASTERS, Zone::Hand);
    g.choose(Choice::Cast(b));
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    let army: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c).contains("Army")).collect();
    assert_eq!(army.len(), 1, "{}", log(&g));
    assert_eq!(g.colors_of(army[0]), 1 << 2, "a black token");
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    assert_eq!(all.len(), 7);
    pay(&mut g, 0, &[], &all);
    assert!(g.obj(army[0]).tapped);
}

#[test]
fn cr_702_51a_kaito_a_creature_during_its_controllers_turn_convokes() {
    // Kaito, Bane of Nightmares is a 3/4 Ninja creature during its
    // controller's turn while it has a loyalty counter: blue and black, it
    // pays a {B/G} pip.
    let mut g = scen()
        .battlefield(0, &["Kaito, Bane of Nightmares", GREEN, RED, RED, RED, GREY, GREY])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Kaito, Bane of Nightmares");
    assert!(g.is_creature(k), "{}", log(&g));
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &[], &all);
    assert!(g.obj(k).tapped);
}

#[test]
fn cr_601_2g_a_lotus_petal_is_no_source_of_hogaak_but_floated_first_it_is_delve_fuel() {
    // Hogaak's payment activates no mana ability (its mana can't be spent),
    // so a Lotus Petal isn't offered in its delve; sacrificed for mana with
    // priority first (CR 605.3a), the Petal is a graveyard card, delved, and
    // its mana stays in the pool.
    let mut g = scen()
        .battlefield(0, &["Lotus Petal", GREEN, GREEN])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    // (Castable: the Petal sacrificed during the cast is delve's fifth card, CR
    // 601.2g; ENGINE.md 229, `rules_cast_mana.rs`.)
    assert!(castable(&g, 0, Zone::Hand), "four cards, the Petal and two creatures: {}", opts(&g));
    g.choose(Choice::Yes);
    let petal = bf(&g, 0, "Lotus Petal");
    g.choose(Choice::Card(petal));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ActivateMana)).then_some(Choice::Pass));
    assert_eq!(g.obj(petal).zone, Zone::Graveyard, "{}", log(&g));
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
    begin_hogaak(&mut g, 0, Zone::Hand);
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert!(gyc.contains(&petal));
    let greens = creatures(&g, 0, GREEN);
    pay(&mut g, 0, &gyc, &greens);
    assert_eq!(g.obj(petal).zone, Zone::Exile);
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1, "the Petal's mana floats on: {}", log(&g));
}

#[test]
fn cr_601_2f_elite_spellbinders_tax_on_hogaak_cast_from_exile() {
    // P1's Elite Spellbinder exiles Hogaak from P0's hand: P0 may cast it
    // from exile for {2} more ({7}{B/G}{B/G}), by convoke and delve.
    let mut g = scen()
        .battlefield(1, &["Plains", "Plains", "Plains"])
        .hand(1, &["Elite Spellbinder"])
        .battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .graveyard(0, &["Ponder", "Brainstorm"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 1)
        .start();
    let es = g.card(1, "Elite Spellbinder", Zone::Hand);
    g.choose(Choice::Cast(es));
    until_priority(&mut g, &|_| None);
    resolve_with(&mut g, &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(engine::Target::Player(0))),
        Some(DecisionKind::ChooseDiscard) => g.offered().into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == HOGAAK)),
        _ => None,
    });
    let h = g.card(0, HOGAAK, Zone::Exile);
    g.drive_until(|g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g) && g.to_move() == Some(0));
    assert!(g.has_option(Choice::Cast(h)), "nine units for {{7}}{{B/G}}{{B/G}}: {}\n{}", opts(&g), log(&g));
    g.choose(Choice::Cast(h));
    let gyc: Vec<CardIdx> = g.player(0).graveyard.clone();
    let all: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    pay(&mut g, 0, &gyc, &all);
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK), "{}", log(&g));
}

#[test]
fn cr_118_9_bolass_citadel_casts_hogaak_for_life_rather_than_its_mana_cost() {
    // Bolas's Citadel: Hogaak on top of the library is cast for 7 life
    // (rather than its mana cost, an alternative cost of no mana).
    let mut g = scen()
        .battlefield(0, &["Bolas's Citadel"])
        .library(0, &[HOGAAK, "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, HOGAAK, Zone::Library);
    assert!(g.has_option(Choice::Cast(h)), "{}", opts(&g));
    g.choose(Choice::Cast(h));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.life(0), 13, "{}", log(&g));
    resolve(&mut g);
    assert!(on_bf(&g, 0, HOGAAK));
}


// ---- convoke with a blocking creature (a test card with flash) -------------------

/// Constructed play's table with "Test Flash Necropolis" (Creature - Avatar
/// 1/1, {2}: "Flash. You can't spend mana to cast this spell. Convoke"): the
/// payment of no mana at instant speed, which Hogaak (no flash) never has;
/// and "Test Blood Imp" (black Creature - Imp 1/1, {B}: "Sacrifice this
/// creature: Add {B}."), a creature sacrificed for mana.
fn flash_db() -> &'static CardDb {
    use serde_json::json;
    static DB: std::sync::OnceLock<&'static CardDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let card = json!({"name": "Test Flash Necropolis", "custom": true, "printing": {
            "$t": "CardPrinting", "name": "Test Flash Necropolis", "card_types": ["Creature"], "subtypes": ["Avatar"],
            "supertypes": [], "type_line": "Creature - Avatar", "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "generic": 2}, "mana_cost_str": "{2}",
            "keywords": [{"$t": "Keyword", "kind": "flash"}], "has_convoke": true, "cant_spend_mana_to_cast": true}});
        let imp = json!({"name": "Test Blood Imp", "custom": true, "printing": {
            "$t": "CardPrinting", "name": "Test Blood Imp", "card_types": ["Creature"], "subtypes": ["Imp"],
            "supertypes": [], "type_line": "Creature - Imp", "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "colored": [["B", 1]]}, "mana_cost_str": "{B}",
            "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "sacrifice_self": true},
                "effect": {"$t": "AddManaToPool", "produces": [["B", 1]]}, "is_mana_ability": true}]}});
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(engine::db::load_legacy_document().cards);
        cards.push(serde_json::from_value::<card_ir::Card>(card).expect("the test card's IR"));
        cards.push(serde_json::from_value::<card_ir::Card>(imp).expect("the test card's IR"));
        let db = CardDb::from_cards(&cards);
        db.expect_id("Test Flash Necropolis").unwrap_or_else(|e| panic!("{e}"));
        db.expect_id("Test Blood Imp").unwrap_or_else(|e| panic!("{e}"));
        db.leak()
    })
}

#[test]
fn cr_506_4b_702_51a_a_blocking_creature_tapped_for_convoke_still_blocks_and_deals_damage() {
    // ruling: 7f0ee199
    // P0's Cogwork Librarian (3/3) attacks; P1 blocks with its Librarian and,
    // in the declare blockers step, casts the flash test card by tapping that
    // blocker and another creature (no mana: two untapped Islands stay). The
    // tapped blocker still blocks: the two Librarians trade. (Hogaak itself
    // is cast only as a sorcery, so never with an attacker or blocker
    // untapped in combat: the test card's is the same payment.)
    let mut g = Scenario::new(flash_db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
        .battlefield(0, &[GREY])
        .battlefield(1, &[GREY, RED, "Island", "Island"])
        .hand(1, &["Test Flash Necropolis"])
        .hand(0, &[HOGAAK])
        .at(Step::DeclareAttackers, 0)
        .start();
    let att = bf(&g, 0, GREY);
    let blk = bf(&g, 1, GREY);
    g.choose(Choice::Card(att));
    g.choose(Choice::Pass);
    // (Hogaak isn't castable in combat: a creature spell without flash.)
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(att));
    // (The red creature doesn't block.)
    if g.kind() == Some(DecisionKind::DeclareBlockers) {
        g.pass();
    }
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1) && g.turn().step == Step::DeclareBlockers);
    let t = g.card(1, "Test Flash Necropolis", Zone::Hand);
    assert!(g.has_option(Choice::Cast(t)), "{}", opts(&g));
    g.choose(Choice::Cast(t));
    let red = bf(&g, 1, RED);
    until_priority(&mut g, &|g| {
        if convoking(g) {
            // The blocker and the red creature.
            cards_offered(g).into_iter().find(|&c| c == blk || c == red).map(Choice::Card)
        } else {
            None
        }
    });
    assert!(g.obj(blk).tapped && g.obj(red).tapped, "{}", log(&g));
    assert!(g.permanents_of(1).filter(|&c| g.name(c) == "Island").all(|c| !g.obj(c).tapped), "no mana: {}", log(&g));
    assert!(g.blockers_of(att).contains(&blk), "still blocking (CR 506.4b)");
    g.drive_until(|g| g.turn().step > Step::CombatDamage || g.is_over());
    assert_eq!(g.obj(att).zone, Zone::Graveyard, "the tapped blocker dealt its damage: {}", log(&g));
    assert_eq!(g.obj(blk).zone, Zone::Graveyard);
}

// ---- the encoding ----------------------------------------------------------------

#[test]
fn convokes_picks_and_cost_reach_both_players_inputs() {
    // During Hogaak's convoke (a task holding picks: ENGINE.md 142.10's pick
    // floats) the creatures picked so far are each a pick of its permanent's
    // row, in both players' inputs (public), the context names Hogaak, the
    // task (a cost's picks), how many picks and the cost they pay.
    let mut g = scen()
        .battlefield(0, &[RED, RED, RED, GREY, GREY, GREY, GREEN, GREEN])
        .graveyard(0, &["Ponder"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    begin_hogaak(&mut g, 0, Zone::Hand);
    let ponder = g.card(0, "Ponder", Zone::Graveyard);
    g.choose(Choice::Card(ponder));
    g.pass(); // (no other card to delve)
    assert!(convoking(&g), "{}", opts(&g));
    let cs: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.is_creature(c)).collect();
    for v in 0..2u8 {
        assert_eq!(perm_float(&g, v, cs[0], perm::PICK_RANK), 0.0);
    }
    let x0 = head(&g, 0);
    assert_eq!(x0[O_CONTEXT + ctx::TASK + 12], 1.0, "a cost's picks");
    assert_eq!(x0[O_CONTEXT + ctx::HAS_CARD], 1.0);
    // {5}{B/G}{B/G} less the Ponder delved: {4}{B/G}{B/G}.
    assert_eq!(x0[O_CONTEXT + ctx::COST], 4.0 / 8.0);
    assert_eq!(x0[O_CONTEXT + ctx::COST + 7], 2.0 / 4.0);
    assert_eq!(x0[O_CONTEXT + ctx::PICKED], 0.0);
    g.choose(Choice::Card(cs[0]));
    for v in 0..2u8 {
        assert!(perm_float(&g, v, cs[0], perm::PICK_RANK) > 0.0, "P{v} sees the pick");
        assert_eq!(perm_float(&g, v, cs[1], perm::PICK_RANK), 0.0);
    }
    assert!(perm_float(&g, 0, cs[0], perm::PICKS_ME) > 0.0, "picked by the frame player (P0)");
    assert!(perm_float(&g, 1, cs[0], perm::PICKS_OPP) > 0.0, "picked by the other player (P0, for P1)");
    assert_eq!(head(&g, 0)[O_CONTEXT + ctx::PICKED], 0.25);
    // Which creature was picked is in the input: two games that differ only
    // in the first pick encode apart for both players.
    let mut other = scen()
        .battlefield(0, &[RED, RED, RED, GREY, GREY, GREY, GREEN, GREEN])
        .graveyard(0, &["Ponder"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    begin_hogaak(&mut other, 0, Zone::Hand);
    let p2 = other.card(0, "Ponder", Zone::Graveyard);
    other.choose(Choice::Card(p2));
    other.pass();
    let os: Vec<CardIdx> = other.permanents_of(0).filter(|&c| other.is_creature(c)).collect();
    other.choose(Choice::Card(os[1]));
    for v in 0..2u8 {
        assert_ne!(Row::of(&g, v).head, Row::of(&other, v).head, "P{v}: which creature was picked");
    }
}

// ---- castability and the payment agree (ENGINE.md 49) ------------------------------

#[test]
fn cr_601_2h_castability_and_the_payment_agree_on_random_boards() {
    // Random boards: creatures of every colour kind (green, black, red,
    // colourless, black-green, green-white, blue-black), graveyard cards,
    // Hogaak in the hand or the graveyard (another copy maybe in the
    // graveyard), lands and floating mana (never a source), sometimes
    // Underworld Breach (escape), Green Goblin (a reduction), Disruptor Flute
    // naming Hogaak (+{3}) or Omniscience. Castability (the payment classes'
    // matching) must agree with a brute force over delve counts and
    // creature sets (convoke's matching), and from a castable board every
    // option of every payment decision must lead to the cast's end with no
    // mana spent.
    use engine::rng::Rng;
    // (No legendary creature: two of one would ask the legend rule first.)
    let kinds = [GREEN, "Dark Confidant", RED, GREY, "Alpha Deathclaw", "Knight of Autumn", "Fallen Shinobi"];
    let fillers = ["Ponder", "Brainstorm", "Daze", "Island", "Swamp", "Wasteland", "Force of Will"];
    let mut rng = Rng::new(220);
    let (mut castable_n, mut paths_n) = (0, 0usize);
    for case in 0..600 {
        let n_cre = rng.below(9) as usize;
        let cre: Vec<&str> = (0..n_cre).map(|_| kinds[rng.below(kinds.len() as u32) as usize]).collect();
        let n_gy = rng.below(7) as usize;
        let mut gyv: Vec<&str> = (0..n_gy).map(|_| fillers[rng.below(fillers.len() as u32) as usize]).collect();
        let from_gy = rng.below(2) == 0;
        if from_gy || rng.below(4) == 0 {
            let at = rng.below(gyv.len() as u32 + 1) as usize;
            gyv.insert(at, HOGAAK);
        }
        let extra = rng.below(6);
        let mut bf0: Vec<&str> = cre.clone();
        bf0.extend(["Swamp", "Forest", "Bayou"]);
        match extra {
            0 => bf0.push("Underworld Breach"),
            1 => bf0.push("Omniscience"),
            _ => {}
        }
        let mut s = scen().battlefield(0, &bf0).graveyard(0, &gyv).pool(0, [0, 0, 1, 0, 1, 0], 0).at(Step::Main1, 0);
        if !from_gy {
            s = s.hand(0, &[HOGAAK]);
        }
        if extra == 2 {
            s = s.battlefield(0, &["Norman Osborn // Green Goblin"]).transformed(0, "Norman Osborn // Green Goblin");
        }
        let g = s.start();
        let zone = if from_gy { Zone::Graveyard } else { Zone::Hand };
        let h = g.card(0, HOGAAK, zone);
        let offered = g.has_option(Choice::Cast(h));
        // Brute force: the needs of the cheapest route this board has, every
        // delve count, convoke's matching over the creatures.
        let creatures: Vec<u8> = g.permanents_of(0).filter(|&c| g.is_creature(c)).map(|c| g.colors_of(c) & 0x1f).collect();
        let others = g.player(0).graveyard.iter().filter(|&&c| c != h).count() as u64;
        let goblin = extra == 2 && from_gy;
        let route_payable = |generic: u64, keep: u64| -> bool {
            let gy = others.saturating_sub(keep);
            (0..=generic.min(gy)).any(|d| {
                let mut needs = vec![0u8; (generic - d) as usize];
                needs.extend([(1u8 << 2) | (1 << 4); 2]);
                needs.len() <= creatures.len() && {
                    // (convoke's matching: generic 0 = any, else a colour of the mask)
                    let mut used = vec![false; creatures.len()];
                    fn place(i: usize, needs: &[u8], cs: &[u8], used: &mut [bool]) -> bool {
                        if i == needs.len() {
                            return true;
                        }
                        (0..cs.len()).any(|j| {
                            if used[j] || (needs[i] != 0 && cs[j] & needs[i] == 0) {
                                return false;
                            }
                            used[j] = true;
                            let ok = place(i + 1, needs, cs, used);
                            used[j] = false;
                            ok
                        })
                    }
                    place(0, &needs, &creatures, &mut used)
                }
            })
        };
        let generic = if goblin { 3 } else { 5 };
        let free = extra == 1 && !from_gy;
        let escape = extra == 0 && from_gy && others >= 3;
        let brute = free || route_payable(generic, 0) || (escape && route_payable(generic, 3));
        assert_eq!(offered, brute, "case {case}: offered {offered}, brute force {brute}: {cre:?} gy {gyv:?} extra {extra} from_gy {from_gy}\n{}", opts(&g));
        if !offered {
            continue;
        }
        castable_n += 1;
        // Every path of the payment's decisions ends with Hogaak cast (a cap
        // on the paths walked: the first 400).
        let mut g0 = g.clone();
        g0.choose(Choice::Cast(h));
        let mut stack = vec![g0];
        let mut leaves = 0;
        while let Some(w) = stack.pop() {
            if leaves >= 400 {
                break;
            }
            if at_priority(&w) {
                leaves += 1;
                assert_eq!(w.obj(h).zone, Zone::Stack, "case {case}: cast\n{}", log(&w));
                assert_eq!(w.player(0).pool.amt, [0, 0, 1, 0, 1, 0], "case {case}: no mana spent\n{}", log(&w));
                for &c in w.permanents_of(0).filter(|&c| !w.is_creature(c)).collect::<Vec<_>>().iter() {
                    assert!(!w.obj(c).tapped, "case {case}: {} tapped\n{}", w.name(c), log(&w));
                }
                continue;
            }
            let d = w.pending_decision().unwrap_or_else(|| panic!("case {case}: no decision\n{}", log(&w)));
            assert!(!d.options.is_empty(), "case {case}: a decision without options\n{}", log(&w));
            for o in &d.options {
                let mut x = w.clone();
                x.choose(o.choice);
                stack.push(x);
            }
        }
        paths_n += leaves;
    }
    assert!(castable_n >= 120 && paths_n >= 1200, "castable boards {castable_n}, payment paths {paths_n}");
}

#[test]
fn cr_601_2g_a_lotus_petal_sacrificed_while_hogaak_is_cast_feeds_escapes_exile() {
    // Underworld Breach's escape exiles three other graveyard cards; with two
    // there and a Lotus Petal on the battlefield, the Petal sacrificed for
    // mana while Hogaak is cast (CR 601.2g: mana abilities before the cost is
    // paid, its mana floating: Hogaak's can't be spent) is the third (ENGINE.md
    // 229, which replaced lane A's reading that Hogaak's payment activates no
    // mana source). Both permissions are routes; cast by escape, the 601.2g
    // step must sacrifice the Petal (`PASS` alone can't pay). The Petal floated
    // first (its mana stays) is the same.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Lotus Petal", GREEN, GREEN, RED, RED, RED, GREY, GREY])
        .graveyard(0, &[HOGAAK, "Ponder", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let petal = bf(&g, 0, "Lotus Petal");
    let mut esc = g.clone();
    begin_hogaak(&mut esc, 0, Zone::Graveyard);
    assert_eq!(esc.kind(), Some(DecisionKind::ChooseRoute), "{}", opts(&esc));
    let mut routes: Vec<Choice> = esc.offered();
    routes.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(routes, vec![Choice::Number(10), Choice::Number(2)], "{}", opts(&esc));
    esc.choose(Choice::Number(2));
    assert_eq!(esc.stack().last().unwrap().from, engine::game::CastFrom::Escape, "{}", log(&esc));
    assert_eq!(esc.kind(), Some(DecisionKind::CastMana), "{}", opts(&esc));
    assert_eq!(esc.offered(), vec![Choice::Card(petal)], "only the Petal leaves escape's exile payable: {}", opts(&esc));
    until_priority(&mut esc, &|_| None);
    let h = esc.card(0, HOGAAK, Zone::Stack);
    assert_eq!(esc.obj(h).zone, Zone::Stack, "{}", log(&esc));
    assert_eq!(esc.obj(petal).zone, Zone::Exile, "the Petal exiled by escape: {}", log(&esc));
    g.choose(Choice::Yes);
    g.choose(Choice::Card(petal));
    until_priority(&mut g, &|g| (g.kind() == Some(DecisionKind::ActivateMana)).then_some(Choice::Pass));
    begin_hogaak(&mut g, 0, Zone::Graveyard);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseRoute), "{}", opts(&g));
    let mut routes: Vec<Choice> = g.offered();
    routes.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(routes, vec![Choice::Number(10), Choice::Number(2)], "{}", opts(&g));
}
