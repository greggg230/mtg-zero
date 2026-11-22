//! Mana abilities while a spell is cast (CR 601.2g-h; ENGINE.md 228): after the total cost is determined (601.2f)
//! the caster may activate mana abilities, then pays the cost in any order (601.2h). A card a mana ability puts
//! into the graveyard then is delve's to exile for that same spell; a mana ability whose mana isn't needed may be
//! activated then, its triggered ability waiting until the spell is cast (601.2i, 603.3: above it); an "activate
//! only as an instant" ability (Lion's Eye Diamond) never is (no player has priority while a spell is cast). One
//! collapse: a permanent's own "{T}: add <fixed mana>" with nothing else is left to the priority after the cast when
//! nothing between can tell (`cast_mana.rs`).

use engine::db::CardDb;
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const HOGAAK: &str = "Hogaak, Arisen Necropolis";
/// Green 2/2 (vanilla on the battlefield).
const GREEN: &str = "Elvish Spirit Guide";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn cube() -> Scenario {
    Scenario::new(CardDb::cube()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
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

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// The cards the pending decision offers (`Choice::Card`).
fn cards_offered(g: &Game) -> Vec<CardIdx> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Card(c) => Some(c),
            _ => None,
        })
        .collect()
}

/// Answer every decision but priority with `answer` (else the first option) until priority; the kinds asked.
fn until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) -> Vec<DecisionKind> {
    let mut kinds = Vec::new();
    for _ in 0..1000 {
        if g.is_over() || at_priority(g) {
            return kinds;
        }
        kinds.push(g.kind().unwrap());
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority: {}\n{}", opts(g), log(g));
}

/// The names of the stack's objects, bottom first (an ability by its source's name and "(ability)").
fn stack_names(g: &Game) -> Vec<String> {
    g.stack()
        .iter()
        .map(|s| match s.kind {
            StackKind::Spell { card } => g.name(card).to_string(),
            StackKind::Ability { source, .. } => format!("{} (ability)", g.name(source.idx)),
        })
        .collect()
}

/// Delve's answer: the next graveyard card of `cards` offered, else `PASS`.
fn delve_from(g: &Game, cards: &[CardIdx]) -> Option<Choice> {
    if g.kind() != Some(DecisionKind::ChooseGraveyardCard) {
        return None;
    }
    let offered = cards_offered(g);
    Some(cards.iter().copied().find(|c| offered.contains(c)).map_or(Choice::Pass, Choice::Card))
}

/// Delve's answer: the first graveyard card offered but `keep`, else `PASS`.
fn delve_any(g: &Game, keep: &[CardIdx]) -> Option<Choice> {
    if g.kind() != Some(DecisionKind::ChooseGraveyardCard) {
        return None;
    }
    Some(cards_offered(g).into_iter().find(|c| !keep.contains(c)).map_or(Choice::Pass, Choice::Card))
}

/// The permanents and hand cards the pending decision offers (`Choice::Card`, `Choice::Ability`).
fn sources_offered(g: &Game) -> Vec<CardIdx> {
    g.offered()
        .into_iter()
        .filter_map(|o| match o {
            Choice::Card(c) | Choice::Ability(c, _) => Some(c),
            _ => None,
        })
        .collect()
}

#[test]
fn cr_601_2g_a_lotus_petal_sacrificed_while_hogaak_is_cast_is_delved_for_it() {
    // Four graveyard cards and two green creatures are one short of Hogaak's {5}{B/G}{B/G}; a Lotus Petal sacrificed
    // for mana during the cast (its mana can't pay Hogaak, CR 113.6e: it floats) is the fifth card delve exiles.
    let mut g = scen()
        .battlefield(0, &["Lotus Petal", GREEN, GREEN])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, HOGAAK, Zone::Hand);
    let petal = bf(&g, 0, "Lotus Petal");
    assert!(g.has_option(Choice::Cast(h)), "castable with the Petal delved: {}", opts(&g));
    g.choose(Choice::Cast(h));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    // (Without the Petal the cost can't be paid: `PASS` isn't offered.)
    assert_eq!(g.offered(), vec![Choice::Card(petal)], "{}", opts(&g));
    g.choose(Choice::Card(petal));
    let gy: Vec<CardIdx> = g.player(0).graveyard.clone();
    assert!(gy.contains(&petal), "the Petal is in the graveyard before delve's cards are chosen: {}", log(&g));
    until_priority(&mut g, &|g| delve_from(g, &gy));
    assert_eq!(g.obj(petal).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.obj(h).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1, "the Petal's mana floats: {}", log(&g));
}

#[test]
fn cr_601_2g_chromatic_star_activated_while_hogaak_is_cast_is_delved_and_its_draw_goes_above_hogaak() {
    // Hogaak is castable anyway; Chromatic Star ({1}, {T}, sacrifice: one mana of any colour; "when this is put into
    // a graveyard from the battlefield, draw a card") activated during the cast is delve's, and its trigger waits
    // until Hogaak is cast: on the stack above it (CR 601.2i, 603.3).
    let mut g = scen()
        .battlefield(0, &["Chromatic Star", "Swamp", GREEN, GREEN])
        .graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island", "Swamp"])
        .hand(0, &[HOGAAK])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, HOGAAK, Zone::Hand);
    let star = bf(&g, 0, "Chromatic Star");
    g.choose(Choice::Cast(h));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    assert!(g.has_option(Choice::Pass) && g.has_option(Choice::Card(star)), "{}", opts(&g));
    assert!(!cards_offered(&g).contains(&bf(&g, 0, "Swamp")), "the Swamp's {{T}}: add {{B}} is left to the priority after the cast: {}", opts(&g));
    g.choose(Choice::Card(star));
    let mut picks: Vec<CardIdx> = vec![star];
    picks.extend(g.player(0).graveyard.iter().copied().filter(|&c| c != star).take(4));
    let kinds = until_priority(&mut g, &|g| delve_from(g, &picks));
    assert!(kinds.contains(&DecisionKind::ChooseGraveyardCard), "{kinds:?}");
    assert_eq!(g.obj(star).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(stack_names(&g), vec![HOGAAK.to_string(), "Chromatic Star (ability)".to_string()], "{}", log(&g));
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1, "the Star's mana floats: {}", log(&g));
}

#[test]
fn cr_601_2g_chromatic_star_activated_while_treasure_cruise_is_cast_makes_it_castable_and_is_delved() {
    // Five graveyard cards and two Islands: {7}{U} is one short, the Star's mana paid by an Island being one mana
    // for one; the Star's card, in the graveyard once activated during the cast (CR 601.2g), is the eighth.
    let mut g = cube()
        .battlefield(0, &["Chromatic Star", "Island", "Island"])
        .graveyard(0, &["Ponder", "Brainstorm", "Lightning Bolt", "Mountain", "Swamp"])
        .hand(0, &["Treasure Cruise"])
        .at(Step::Main1, 0)
        .start();
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    let star = bf(&g, 0, "Chromatic Star");
    assert!(g.has_option(Choice::Cast(tc)), "castable with the Star delved: {}", opts(&g));
    g.choose(Choice::Cast(tc));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    assert_eq!(g.offered(), vec![Choice::Card(star)], "only the Star's activation leaves the cost payable: {}", opts(&g));
    g.choose(Choice::Card(star));
    until_priority(&mut g, &|g| delve_any(g, &[]));
    assert_eq!(g.obj(tc).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(star).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.stack().last().map(|s| matches!(s.kind, StackKind::Ability { .. })), Some(true), "{:?}", stack_names(&g));
}

#[test]
fn cr_601_2g_a_lotus_petal_activated_while_treasure_cruise_is_cast_is_delved_its_mana_unspent() {
    // Seven graveyard cards and an Island pay {7}{U}; the Petal may still be sacrificed during the cast, delved in
    // place of a graveyard card, its mana floating (an outcome the payment alone, which activates a source only to
    // spend its mana, doesn't reach).
    let mut g = cube()
        .battlefield(0, &["Lotus Petal", "Island"])
        .graveyard(0, &["Ponder", "Brainstorm", "Lightning Bolt", "Mountain", "Swamp", "Forest", "Plains"])
        .hand(0, &["Treasure Cruise"])
        .at(Step::Main1, 0)
        .start();
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    let petal = bf(&g, 0, "Lotus Petal");
    let plains = g.card(0, "Plains", Zone::Graveyard);
    g.choose(Choice::Cast(tc));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    assert!(g.has_option(Choice::Pass), "{}", opts(&g));
    g.choose(Choice::Card(petal));
    // (The Petal's colour: green, not the Cruise's blue; every graveyard card but the Plains delved.)
    until_priority(&mut g, &|g| {
        if g.kind() == Some(DecisionKind::ChooseMana) {
            return g.offered().into_iter().last();
        }
        delve_any(g, &[plains])
    });
    assert_eq!(g.obj(tc).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(petal).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 1, "one graveyard card kept: {}", log(&g));
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1, "the Petal's mana floats: {}", log(&g));
}

#[test]
fn cr_601_2g_an_unneeded_chromatic_star_activated_while_casting_puts_its_draw_above_the_spell() {
    // Lightning Bolt paid by a Mountain; Chromatic Star activated during the cast (its {1} paid by the other
    // Mountain), its mana unneeded: its draw trigger goes on the stack above the Bolt as the Bolt becomes cast.
    let mut g = cube()
        .battlefield(0, &["Chromatic Star", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let star = bf(&g, 0, "Chromatic Star");
    g.choose(Choice::Cast(bolt));
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        Some(DecisionKind::CastMana) if g.obj(star).zone == Zone::Battlefield => Some(Choice::Card(star)),
        _ => None,
    });
    assert_eq!(stack_names(&g), vec!["Lightning Bolt".to_string(), "Chromatic Star (ability)".to_string()], "{}", log(&g));
    // (Floated first with priority instead, the trigger is put on the stack first, below the Bolt.)
    let mut f = cube()
        .battlefield(0, &["Chromatic Star", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let star = bf(&f, 0, "Chromatic Star");
    let bolt = f.card(0, "Lightning Bolt", Zone::Hand);
    let float = f.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == star)).expect("the Star with priority");
    f.choose(float);
    until_priority(&mut f, &|_| None);
    f.choose(Choice::Cast(bolt));
    until_priority(&mut f, &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    assert_eq!(stack_names(&f), vec!["Chromatic Star (ability)".to_string(), "Lightning Bolt".to_string()], "{}", log(&f));
}

#[test]
fn cr_601_2g_lions_eye_diamond_is_never_activated_while_a_spell_is_cast() {
    // "Activate only as an instant" (ruling 2004-10-04: only when you could cast an instant): not while a spell is
    // cast, whether its mana is needed or not; the Star beside it is offered.
    let mut g = cube()
        .battlefield(0, &["Lion's Eye Diamond", "Chromatic Star", "Mountain", "Mountain"])
        .graveyard(0, &["Ponder", "Brainstorm", "Lightning Bolt", "Mountain", "Swamp", "Forest", "Plains"])
        .hand(0, &["Treasure Cruise", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let led = bf(&g, 0, "Lion's Eye Diamond");
    let star = bf(&g, 0, "Chromatic Star");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    let mut seen = 0;
    for _ in 0..50 {
        if at_priority(&g) {
            break;
        }
        if g.kind() == Some(DecisionKind::CastMana) {
            seen += 1;
            assert!(!cards_offered(&g).contains(&led), "{}", opts(&g));
            assert!(cards_offered(&g).contains(&star), "{}", opts(&g));
        }
        if g.kind() == Some(DecisionKind::PayMana) {
            assert!(!cards_offered(&g).contains(&led), "{}", opts(&g));
        }
        let c = match g.kind() {
            Some(DecisionKind::Target) => Choice::Target(Target::Player(1)),
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(seen, 1, "{}", log(&g));
    assert_eq!(g.obj(led).zone, Zone::Battlefield);
}

#[test]
fn cr_605_3a_lions_eye_diamond_activated_with_priority_before_the_cast_feeds_delve() {
    // Floated with priority first (the hand discarded, the Diamond sacrificed), the cards are in the graveyard before
    // Hogaak is cast from it: delve exiles them.
    let mut g = scen()
        .battlefield(0, &["Lion's Eye Diamond", GREEN, GREEN])
        .graveyard(0, &[HOGAAK, "Ponder"])
        .hand(0, &["Brainstorm", "Daze", "Island"])
        .at(Step::Main1, 0)
        .start();
    let h = g.card(0, HOGAAK, Zone::Graveyard);
    let led = bf(&g, 0, "Lion's Eye Diamond");
    assert!(!g.offered().iter().any(|c| *c == Choice::Cast(h) || *c == Choice::FromGraveyard), "two cards: {}", opts(&g));
    let float = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == led)).expect("the Diamond with priority");
    g.choose(float);
    until_priority(&mut g, &|_| None);
    assert_eq!(g.player(0).graveyard.len(), 6, "{}", log(&g));
    assert!(g.player(0).hand.is_empty());
    let cast = if g.has_option(Choice::Cast(h)) { Choice::Cast(h) } else { Choice::FromGraveyard };
    g.choose(cast);
    let gy: Vec<CardIdx> = g.player(0).graveyard.iter().copied().filter(|&c| c != h).collect();
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseGraveyardCard) if g.offered().contains(&Choice::Cast(h)) => Some(Choice::Cast(h)),
        _ => delve_from(g, &gy),
    });
    assert_eq!(g.obj(h).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(led).zone, Zone::Exile, "the Diamond delved: {}", log(&g));
}

#[test]
fn cr_601_2g_plain_lands_are_left_to_the_priority_after_the_cast() {
    // A basic land's {T}: add one mana, nothing else: activated during the cast with its mana unspent it is the same
    // as activated with the priority right after it (the collapse, ENGINE.md 228.3): no 601.2g decision.
    let mut g = cube().battlefield(0, &["Mountain", "Mountain", "Island"]).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    let kinds = until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    assert!(!kinds.contains(&DecisionKind::CastMana), "{kinds:?}\n{}", log(&g));
    // (With priority right after, the lands' abilities are there: `YES`.)
    assert!(g.has_option(Choice::Yes), "{}", opts(&g));
}

#[test]
fn cr_601_2g_a_mana_ability_with_life_or_damage_is_offered_while_casting() {
    // Mana Confluence ({T}, pay 1 life: one mana of any colour) and a Talisman's coloured mana (1 damage to you)
    // aren't plain "{T}: add mana" (their life, their damage): offered during the cast; the Mountain isn't.
    let mut g = cube()
        .battlefield(0, &["Mountain", "Mana Confluence", "Talisman of Dominance"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let conf = bf(&g, 0, "Mana Confluence");
    let tal = bf(&g, 0, "Talisman of Dominance");
    let mountain = bf(&g, 0, "Mountain");
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    let offered = cards_offered(&g);
    assert!(offered.contains(&conf) && offered.contains(&tal) && !offered.contains(&mountain), "{}", opts(&g));
    // Confluence now: 1 life, its mana in the pool pays the Bolt.
    g.choose(Choice::Card(conf));
    until_priority(&mut g, &|_| None);
    assert_eq!(g.player(0).life, 19, "{}", log(&g));
    assert_eq!(g.obj(bolt).zone, Zone::Stack);
}

#[test]
fn cr_602_5_601_2g_timing_restricted_mana_abilities_while_casting_as_while_paying() {
    // Every card of the tables with a timing-restricted mana ability (CR 602.5b, 602.5d: "activate only as an instant",
    // "only if ...", "only during your turn", "only as a sorcery", exhaust): the 601.2g step honours each restriction
    // as the payment that follows it does (no player has priority, a spell is on the stack): an "only as an instant" or
    // "only as a sorcery" one is never offered (Lion's Eye Diamond, ruling 2004-10-04); an "only if" or "only during
    // your turn" one exactly when the payment may activate it; any other only when the payment may. On a poor board
    // and a rich one (every basic land type, more artifacts and a 4-power creature: the conditions both ways), casting
    // an artifact of generic mana (any mana pays it) of the card's own table.
    let mut found: Vec<(&'static CardDb, String, bool, bool)> = Vec::new();
    for t in [CardDb::constructed(), CardDb::eoe()] {
        for d in 0..t.len() as u16 {
            let def = t.def(d);
            for ad in def.activated.iter() {
                if ad.mana().is_some() && (ad.sorcery || ad.instant_only || ad.your_turn || ad.only_if.is_some() || ad.exhaust) && !found.iter().any(|f| f.1 == def.name) {
                    // (`exact`: every mana ability of the card has an "only if" or "only during your turn": the
                    // card is offered exactly when one of them may be activated. A card with an unrestricted one
                    // besides - a Verge's first, a plain "{T}: add" the step leaves to the priority after the cast,
                    // ENGINE.md 228.3 - is offered while casting only if the payment may activate it.)
                    let all = def.activated.iter().filter(|a| a.mana().is_some()).all(|a| a.only_if.is_some() || a.your_turn);
                    found.push((t, def.name.clone(), ad.instant_only || ad.sorcery, all));
                }
            }
            assert!(def.hand_abilities.iter().all(|ad| !(ad.mana().is_some() && (ad.sorcery || ad.instant_only || ad.your_turn || ad.only_if.is_some() || ad.exhaust))), "{}: a restricted hand mana ability (add it here)", def.name);
        }
    }
    assert!(found.iter().any(|f| f.1 == "Lion's Eye Diamond"), "{:?}", found.iter().map(|f| &f.1).collect::<Vec<_>>());
    let mut seen = Vec::new();
    for (t, name, never, exact) in &found {
        // An artifact spell of generic mana, no X, no target, of the card's table.
        let spell = (0..t.len() as u16)
            .map(|d| t.def(d))
            .find(|d| d.is(engine::db::types::ARTIFACT) && !d.is(engine::db::types::CREATURE) && d.cost.generic == 1 && d.cost.total() == 1 && d.x_pips == 0 && d.spell.is_none() && d.additional.is_empty() && d.alt_costs.is_empty())
            .expect("a {1} artifact")
            .name
            .clone();
        let land = if t.id("Mountain").is_some() { "Mountain" } else { "Wastes" };
        for rich in [false, true] {
            let mut board: Vec<&str> = vec![name.as_str(), land];
            if rich {
                board.extend(["Plains", "Island", "Swamp", "Forest", spell.as_str(), spell.as_str(), spell.as_str()]);
                if let Some(big) = (0..t.len() as u16).map(|d| t.def(d)).find(|d| d.is(engine::db::types::CREATURE) && d.power.unwrap_or(0) >= 4 && d.statics.is_empty() && d.triggers.is_empty()) {
                    board.push(big.name.as_str());
                }
            }
            let mut g = Scenario::new(t).spec(|s| s.auto_single = false).library(0, &[land; 12]).library(1, &[land; 12]).battlefield(0, &board).hand(0, &[spell.as_str()]).at(Step::Main1, 0).start();
            let c = bf(&g, 0, name);
            let s = g.card(0, &spell, Zone::Hand);
            if !g.has_option(Choice::Cast(s)) {
                continue;
            }
            g.choose(Choice::Cast(s));
            let in_step = g.kind() == Some(DecisionKind::CastMana) && sources_offered(&g).contains(&c);
            if g.kind() == Some(DecisionKind::CastMana) {
                if !g.has_option(Choice::Pass) {
                    continue;
                }
                g.choose(Choice::Pass);
            }
            let Some(DecisionKind::PayMana) = g.kind() else { continue };
            let in_payment = sources_offered(&g).contains(&c);
            seen.push(format!("{name} rich {rich}: step {in_step} payment {in_payment}"));
            assert!(!in_step || in_payment, "{name} (rich {rich}): offered while casting, not while paying\n{}", log(&g));
            if *never {
                assert!(!in_step, "{name}: an instant- or sorcery-speed mana ability offered while casting");
            }
            if *exact {
                assert_eq!(in_step, in_payment, "{name} (rich {rich}): its condition read alike while casting and paying\n{}", log(&g));
            }
        }
    }
    println!("{}", seen.join("\n"));
    assert!(seen.len() >= found.len(), "boards checked {}: {seen:?}", seen.len());
}

#[test]
fn cr_601_2g_hogaak_is_castable_only_through_a_chromatic_star_its_cost_paid_by_a_land() {
    // Four graveyard cards and two green creatures: Hogaak's {5} is one card short. Chromatic Star ({1}, {T},
    // sacrifice: one mana) activated while Hogaak is cast, its {1} paid by the Swamp (no mana pays Hogaak, CR 113.6e,
    // but a mana ability's cost is another payment: 605.3a), is the fifth card delve exiles. Castability counts it
    // (`feed_castable`, ENGINE.md 228.4); without a mana source for the Star's {1} Hogaak isn't castable.
    let board = |swamp: bool| {
        let mut b = vec!["Chromatic Star", GREEN, GREEN];
        if swamp {
            b.push("Swamp");
        }
        scen().battlefield(0, &b).graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island"]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start()
    };
    let none = board(false);
    assert!(!none.has_option(Choice::Cast(none.card(0, HOGAAK, Zone::Hand))), "nothing pays the Star's {{1}}: {}", opts(&none));
    let mut g = board(true);
    let h = g.card(0, HOGAAK, Zone::Hand);
    let star = bf(&g, 0, "Chromatic Star");
    assert!(g.has_option(Choice::Cast(h)), "castable through the Star: {}", opts(&g));
    g.choose(Choice::Cast(h));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    assert_eq!(g.offered(), vec![Choice::Card(star)], "only the Star leaves the cost payable: {}", opts(&g));
    g.choose(Choice::Card(star));
    until_priority(&mut g, &|g| delve_any(g, &[]));
    assert_eq!(g.obj(h).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(star).zone, Zone::Exile, "the Star delved: {}", log(&g));
    assert!(g.obj(bf(&g, 0, "Swamp")).tapped, "the Swamp paid the Star: {}", log(&g));
    assert_eq!(stack_names(&g), vec![HOGAAK.to_string(), "Chromatic Star (ability)".to_string()], "{}", log(&g));
}

#[test]
fn cr_601_2g_702_51a_a_creature_that_pays_a_chromatic_stars_cost_doesnt_convoke() {
    // Llanowar Elves ({T}: add {G}) may pay the Star's {1} or convoke for Hogaak, not both (tapped for mana it can't
    // be tapped for convoke): with one other green creature and four graveyard cards, the Elves paying the Star leaves
    // one creature for Hogaak's two hybrid pips, and the Elves convoking leaves the Star's {1} unpaid: not castable. A
    // Swamp paying the Star instead: castable, the Elves convoking.
    let board = |swamp: bool| {
        let mut b = vec!["Chromatic Star", "Llanowar Elves", GREEN];
        if swamp {
            b.push("Swamp");
        }
        scen().battlefield(0, &b).graveyard(0, &["Ponder", "Brainstorm", "Daze", "Island"]).hand(0, &[HOGAAK]).at(Step::Main1, 0).start()
    };
    let none = board(false);
    assert!(!none.has_option(Choice::Cast(none.card(0, HOGAAK, Zone::Hand))), "{}", opts(&none));
    let mut g = board(true);
    let h = g.card(0, HOGAAK, Zone::Hand);
    assert!(g.has_option(Choice::Cast(h)), "{}", opts(&g));
    g.choose(Choice::Cast(h));
    until_priority(&mut g, &|g| delve_any(g, &[]));
    assert_eq!(g.obj(h).zone, Zone::Stack, "{}", log(&g));
    assert!(g.log_lines().iter().any(|l| l.contains("taps Llanowar Elves for convoke")), "the Elves convoked: {}", log(&g));
    assert!(g.obj(bf(&g, 0, "Swamp")).tapped, "the Swamp paid the Star: {}", log(&g));
}

#[test]
fn cr_601_2g_702_138a_a_chromatic_star_activated_while_a_spell_is_cast_feeds_escapes_exile() {
    // Underworld Breach's escape: Lightning Bolt's {R} and three other graveyard cards exiled; two are there. The
    // Mountain pays the Star's {1} while the Bolt is cast, the Star's {R} pays the Bolt, and the Star's card is the
    // third (CR 601.2g before 601.2h). Castability counts it (`feed_castable`); the Mountain alone can't.
    let board = |star: bool| {
        let mut b = vec!["Underworld Breach", "Mountain"];
        if star {
            b.push("Chromatic Star");
        }
        scen().battlefield(0, &b).graveyard(0, &["Lightning Bolt", "Ponder", "Brainstorm"]).at(Step::Main1, 0).start()
    };
    let none = board(false);
    assert!(!none.has_option(Choice::Cast(none.card(0, "Lightning Bolt", Zone::Graveyard))), "{}", opts(&none));
    let mut g = board(true);
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    let star = bf(&g, 0, "Chromatic Star");
    assert!(g.has_option(Choice::Cast(bolt)), "escape through the Star: {}", opts(&g));
    g.choose(Choice::Cast(bolt));
    until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    assert_eq!(g.obj(bolt).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.stack()[0].from, engine::game::CastFrom::Escape, "{}", log(&g));
    assert_eq!(g.obj(star).zone, Zone::Exile, "the Star exiled by escape: {}", log(&g));
}

#[test]
fn cr_601_2g_castability_and_the_cast_agree_with_mana_abilities_that_feed_the_graveyard_on_random_boards() {
    // Random boards around Hogaak (its mana can't be spent, CR 113.6e) and the 601.2g step: green, red and colourless
    // creatures, Llanowar Elves (it pays a Chromatic Star's {1} or convokes, not both), 0-2 Chromatic Stars ({1}, {T},
    // sacrifice: one mana of any colour; a Star's {1} paid by a land, the Elves, a Lotus Petal, the pool or another
    // Star's mana), 0-1 Lotus Petal, 0-2 lands, sometimes floating mana, graveyard cards, Hogaak in the hand or the
    // graveyard, sometimes Underworld Breach (escape: three other graveyard cards exiled). Castability must agree with
    // a brute force over the Stars and the Petal sacrificed while it is cast, the Elves tapped for a Star, delve's
    // count and convoke's matching; and from a castable board every option of every decision of the cast must lead
    // to Hogaak cast.
    use engine::rng::Rng;
    let kinds = [GREEN, GREEN, "Simian Spirit Guide", "Cogwork Librarian", "Llanowar Elves", "Dark Confidant"];
    let fillers = ["Ponder", "Brainstorm", "Daze", "Island", "Swamp", "Wasteland"];
    let mut rng = Rng::new(222);
    let (mut castable_n, mut through_star, mut paths_n) = (0, 0, 0usize);
    for case in 0..500 {
        let cre: Vec<&str> = (0..rng.below(5)).map(|_| kinds[rng.below(kinds.len() as u32) as usize]).collect();
        let gyv0: Vec<&str> = (0..rng.below(7)).map(|_| fillers[rng.below(fillers.len() as u32) as usize]).collect();
        let stars = rng.below(3) as usize;
        let petals = rng.below(3) as usize / 2;
        let lands = rng.below(3) as usize;
        let pool = rng.below(4) == 0;
        let breach = rng.below(3) == 0;
        let from_gy = rng.below(2) == 0;
        let mut gyv = gyv0.clone();
        if from_gy {
            let at = rng.below(gyv.len() as u32 + 1) as usize;
            gyv.insert(at, HOGAAK);
        }
        let mut b: Vec<&str> = cre.clone();
        b.extend(std::iter::repeat_n("Chromatic Star", stars));
        b.extend(std::iter::repeat_n("Lotus Petal", petals));
        b.extend(["Swamp", "Forest"].iter().take(lands));
        if breach {
            b.push("Underworld Breach");
        }
        let mut s = scen().battlefield(0, &b).graveyard(0, &gyv).at(Step::Main1, 0);
        if pool {
            s = s.pool(0, [0, 0, 1, 0, 0, 0], 0);
        }
        if !from_gy {
            s = s.hand(0, &[HOGAAK]);
        }
        let g = s.start();
        let zone = if from_gy { Zone::Graveyard } else { Zone::Hand };
        let h = g.card(0, HOGAAK, zone);
        let offered = g.has_option(Choice::Cast(h));
        // Brute force.
        let colours: Vec<u8> = g.permanents_of(0).filter(|&c| g.is_creature(c)).map(|c| g.colors_of(c) & 0x1f).collect();
        let elves = cre.iter().filter(|&&n| n == "Llanowar Elves").count();
        let others = gyv0.len();
        let ext = lands + pool as usize;
        let matching = |cs: &[u8], generic: usize| -> bool {
            let mut needs = vec![0u8; generic];
            needs.extend([(1u8 << 2) | (1 << 4); 2]);
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
            needs.len() <= cs.len() && place(0, &needs, cs, &mut vec![false; cs.len()])
        };
        let brute_with = |max_stars: usize| -> bool {
            (0..=max_stars).any(|st| {
                (0..=petals).any(|pe| {
                    (0..=elves.min(1)).any(|el| {
                        // (One mana pays the first Star's {1}; each Star's mana pays the next one's.)
                        if st > 0 && ext + pe + el == 0 || st == 0 && el > 0 {
                            return false;
                        }
                        // (The Elves tapped for a Star: one green creature fewer for convoke.)
                        let mut cs = colours.clone();
                        if el > 0 {
                            let i = cs.iter().position(|&c| c == 1 << 4).expect("the Elves' green");
                            cs.remove(i);
                        }
                        let fuel = others + st + pe;
                        let pays = |fuel: usize| (0..=5usize.min(fuel)).any(|d| matching(&cs, 5 - d));
                        pays(fuel) || (breach && from_gy && fuel >= 3 && pays(fuel - 3))
                    })
                })
            })
        };
        let brute = brute_with(stars);
        assert_eq!(offered, brute, "case {case}: offered {offered}, brute force {brute}: {b:?} gy {gyv:?} pool {pool}\n{}", opts(&g));
        if !offered {
            continue;
        }
        castable_n += 1;
        through_star += !brute_with(0) as usize;
        let mut g0 = g.clone();
        g0.choose(Choice::Cast(h));
        let mut stack = vec![g0];
        let mut leaves = 0;
        while let Some(w) = stack.pop() {
            if leaves >= 300 {
                break;
            }
            if at_priority(&w) {
                leaves += 1;
                assert_eq!(w.obj(h).zone, Zone::Stack, "case {case}: cast\n{}", log(&w));
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
    println!("castable boards {castable_n}, castable only through a Star {through_star}, cast paths {paths_n}");
    assert!(castable_n >= 100 && through_star >= 10 && paths_n >= 1000, "castable boards {castable_n}, through a Star {through_star}, paths {paths_n}");
}

#[test]
fn cr_702_66a_a_source_delved_isnt_also_tapped_by_another_ability() {
    // Dig Through Time ({6}{U}{U}): two Islands, Urza, Lord High Artificer ("tap an untapped artifact you control: add
    // {U}") and Wizard's Rockets, five graveyard cards. Urza tapping the Rockets for {U} pays one mana; the Rockets
    // delved (sacrificed by their own ability as the payment begins, CR 601.2g-h) is one card - not both. With the
    // Rockets chosen in delve, the rest needs all five graveyard cards: `PASS` is never offered after four (before,
    // delve counted Urza's tap of the Rockets and the payment then had no way to pay; ENGINE.md 228, cube board 2213).
    let mut g = cube()
        .battlefield(0, &["Island", "Island", "Urza, Lord High Artificer", "Wizard's Rockets"])
        .graveyard(0, &["Ponder", "Brainstorm", "Lightning Bolt", "Mountain", "Swamp"])
        .hand(0, &["Dig Through Time"])
        .at(Step::Main1, 0)
        .start();
    let dig = g.card(0, "Dig Through Time", Zone::Hand);
    let rockets = bf(&g, 0, "Wizard's Rockets");
    assert!(g.has_option(Choice::Cast(dig)), "{}", opts(&g));
    g.choose(Choice::Cast(dig));
    g.pass_cast_mana();
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{}", opts(&g));
    assert!(g.has_option(Choice::Card(rockets)), "the Rockets may be delved: {}", opts(&g));
    g.choose(Choice::Card(rockets));
    let mut picks = 0;
    while g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
        let cards = cards_offered(&g);
        if picks < 5 {
            assert!(!g.has_option(Choice::Pass), "after {picks} graveyard cards: {}", opts(&g));
        }
        match cards.first() {
            Some(&c) => g.choose(Choice::Card(c)),
            None => g.choose(Choice::Pass),
        };
        picks += 1;
    }
    until_priority(&mut g, &|_| None);
    assert_eq!(g.obj(dig).zone, Zone::Stack, "{}", log(&g));
    assert_eq!(g.obj(rockets).zone, Zone::Exile, "{}", log(&g));
}
