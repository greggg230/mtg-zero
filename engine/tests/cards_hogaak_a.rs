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
    // A creature sacrificed for mana is gone: Lotus Petal is no creature, so
    // the sacrifice half of the ruling is shown by a creature that leaves -
    // a creature that isn't on the battlefield is no candidate (here: the
    // seventh creature is in the graveyard).
    let g = scen().battlefield(0, &[GREEN, GREEN, RED, RED, RED, GREY]).graveyard(0, &[GREY]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start();
    // (The graveyard's card is delve fuel: one creature fewer, one card more.)
    assert!(castable(&g, 0, Zone::Hand), "{}", opts(&g));
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
