//! Mana abilities while a spell is cast (CR 601.2g-h; ENGINE.md 222): after the total cost is determined (601.2f)
//! the caster may activate mana abilities, then pays the cost in any order (601.2h). A card a mana ability puts
//! into the graveyard then is delve's to exile for that same spell; a mana ability whose mana isn't needed may be
//! activated then, its triggered ability waiting until the spell is cast (601.2i, 603.3: above it); an "activate
//! only as an instant" ability (Lion's Eye Diamond) never is (no player has priority while a spell is cast). One
//! collapse: a permanent's own "{T}: add <fixed mana>" with nothing else is left to the priority after the cast when
//! nothing between can tell (`cast_mana.rs`).

use engine::db::{AbilitySlot, CardDb};
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
    let gy: Vec<CardIdx> = g.player(0).graveyard.clone();
    until_priority(&mut g, &|g| delve_from(g, &gy));
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
    g.choose(Choice::Cast(tc));
    assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{}", opts(&g));
    assert!(g.has_option(Choice::Pass), "{}", opts(&g));
    g.choose(Choice::Card(petal));
    // (The Petal's colour: green, not the Cruise's blue.)
    until_priority(&mut g, &|g| {
        if g.kind() == Some(DecisionKind::ChooseMana) {
            return g.offered().into_iter().last();
        }
        let mut picks = vec![petal];
        picks.extend(g.player(0).graveyard.iter().copied().filter(|&c| c != petal).take(6));
        delve_from(g, &picks)
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
    // as activated with the priority right after it (the collapse, ENGINE.md 222.3): no 601.2g decision.
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
fn cr_602_5_timing_restricted_mana_abilities_are_never_offered_while_casting() {
    // Every mana ability of the tables' cards with a timing restriction (CR 602.5b, 602.5d: "activate only as a
    // sorcery", "as an instant", "only during your turn", "only if ...", exhaust): the 601.2g step honours each as
    // activation with priority does, with a spell on the stack (no sorcery timing) and no priority.
    let mut found = Vec::new();
    for t in [CardDb::cube(), CardDb::constructed(), CardDb::eoe()] {
        for d in 0..t.len() as u16 {
            let def = t.def(d);
            for ad in def.activated.iter().chain(def.hand_abilities.iter()) {
                if ad.mana().is_some() && (ad.sorcery || ad.instant_only || ad.your_turn || ad.only_if.is_some() || ad.exhaust) {
                    found.push((def.name.clone(), ad.sorcery, ad.instant_only, ad.your_turn, ad.only_if.is_some(), ad.exhaust));
                }
            }
        }
    }
    found.sort();
    found.dedup();
    println!("timing-restricted mana abilities: {found:?}");
    // The instant-only ones (Lion's Eye Diamond): `cr_601_2g_lions_eye_diamond_is_never_activated_while_a_spell_is_cast`.
    for (name, sorcery, instant, your_turn, only_if, exhaust) in &found {
        assert!(*instant || *sorcery || *your_turn || *only_if || *exhaust, "{name}");
    }
    let _ = AbilitySlot::Spell;
}
