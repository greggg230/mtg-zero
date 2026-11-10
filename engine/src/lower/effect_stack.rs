//! Effects of M7b part 3b (ENGINE.md 27) -> programs: spells and abilities
//! on the stack as targets, ward, the initiative and the Undercity dungeon,
//! and the effects of the graveyard abilities' cards. Same rules as
//! `effect.rs`: every field named, lowered onto shared instructions.

use super::effect::{lower_effect, lower_targets};
use super::Lowerer;
use crate::db::{
    types, CardDef, Counter, EventKind, Filter, Kw, KwSet, Mod, Slot, TrigPred, TriggerDef, WardCost,
    ABILITY_ACTIVATED, ABILITY_TRIGGERED,
};
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, PileDest, SearchDest, SpellDest, Who, PILE_ANY};
use card_ir::{cats, enums, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// "Target spell, activated ability, or triggered ability" from the IR's
/// kinds.
fn stack_object_filter(kinds: &[enums::StackObjectKind]) -> Filter {
    let mut spell = false;
    let mut ab = 0u8;
    for k in kinds {
        match k {
            enums::StackObjectKind::Spell => spell = true,
            enums::StackObjectKind::Activated => ab |= ABILITY_ACTIVATED,
            enums::StackObjectKind::Triggered => ab |= ABILITY_TRIGGERED,
        }
    }
    let mut v = Vec::new();
    if spell {
        v.push(Filter::Spell(Box::new(Filter::Any)));
    }
    if ab != 0 {
        v.push(Filter::Ability(ab));
    }
    if v.len() == 1 {
        v.pop().unwrap()
    } else {
        Filter::Or(v)
    }
}

/// Lower an effect class of this group; `false` if it is not one of them.
pub(crate) fn lower_effect_stack(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Counter [up to one] target spell, activated ability, or
        // triggered ability" (CR 701.5a, 115.1); Tishana's Tidebinder: "If
        // an ability of an artifact, creature, or planeswalker is countered
        // this way, that permanent loses all abilities for as long as this
        // creature remains on the battlefield."
        E::CounterTargetStackObject(nodes::CounterTargetStackObject { kinds, up_to_one, lose_abilities_while_source }) => {
            if kinds.is_empty() {
                l.unsupported("CounterTargetStackObject.kinds(empty)");
                return true;
            }
            let t = b.target(stack_object_filter(kinds), if *up_to_one { 0 } else { 1 }, 1);
            if *lose_abilities_while_source {
                b.op(Op::Ext(Ext::SourceOfTargetLosesAbilities { tgt: t }));
            }
            b.op(Op::Counter { tgt: t, dest: SpellDest::Graveyard });
        }
        // "<Effect on its target>. Its controller draws a card": the
        // target's controller as the effect began (608.2h: remembered
        // before the effect, so a countered spell's or destroyed creature's
        // controller still draws).
        E::ThenItsControllerDraws(nodes::ThenItsControllerDraws { inner, amount }) => {
            let n = Amount::Fixed(l.int("ThenItsControllerDraws.amount", *amount, 1, 9));
            let (at, t0) = (b.program.len(), b.targets.len());
            lower_effect(l, b, inner);
            if b.targets.len() != t0 + 1 {
                l.unsupported("ThenItsControllerDraws.inner(not one target)");
                return true;
            }
            b.insert(at, Op::Remember { tgt: t0 as u8 });
            b.op(Op::Draw { who: Who::LkiController, n });
        }
        // "Choose target spell, then flip a coin. If you win the flip, gain
        // control of that spell and you may choose new targets for it. If
        // you lose the flip, counter that spell."
        E::FlipCoinStealOrCounterTargetSpell(nodes::FlipCoinStealOrCounterTargetSpell {}) => {
            let t = b.target(Filter::Spell(Box::new(Filter::Any)), 1, 1);
            let won = b.op(Op::Ext(Ext::FlipCoin { won: 0 }));
            b.op(Op::Counter { tgt: t, dest: SpellDest::Graveyard });
            let end = b.op(Op::Jump { to: 0 });
            b.patch(won);
            b.op(Op::Ext(Ext::GainControlOfSpell { tgt: t }));
            b.patch(end);
        }
        // "<Tap up to one target creature>. It loses all abilities for as
        // long as this creature remains on the battlefield" (The Wondrous
        // Wasp; CR 611.2b: nothing if it already left).
        E::ThenTargetLosesAbilitiesWhileSourceRemains(nodes::ThenTargetLosesAbilitiesWhileSourceRemains { inner }) => {
            let t0 = b.targets.len();
            lower_effect(l, b, inner);
            if b.targets.len() != t0 + 1 {
                l.unsupported("ThenTargetLosesAbilitiesWhileSourceRemains.inner(not one target)");
                return true;
            }
            b.op(Op::Ext(Ext::Apply { on: On::Target(t0 as u8), mods: vec![Mod::LoseAllAbilities], until: Until::WhileSource }));
        }
        // Mobilize N (CR 702.181a): "Whenever this creature attacks, create
        // N 1/1 red Warrior creature tokens. Those tokens enter tapped and
        // attacking. Sacrifice them at the beginning of the next end step."
        E::Mobilize(nodes::Mobilize { n }) => {
            let n = l.int("Mobilize.n", *n, 1, 10) as u8;
            let warrior = json_token(l, WARRIOR);
            b.op(Op::Ext(Ext::CreateTokensAttacking { token: warrior, n: n.into(), tapped: true }));
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::SacrificeEventObj));
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: crate::db::DelayedWhen::NextEndStep, about: crate::program::About::EachMade }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "<Destroy target ...>. That player may search their library for a
        // land card with a basic land type, put it onto the battlefield,
        // then shuffle." (Boseiju: "that player" is the target's controller
        // as the effect began, CR 608.2h; remembered first.)
        E::ThenTargetControllerResolves(nodes::ThenTargetControllerResolves { first, then }) => {
            let (at, t0) = (b.program.len(), b.targets.len());
            lower_effect(l, b, first);
            if b.targets.len() != t0 + 1 {
                l.unsupported("ThenTargetControllerResolves.first(not one target)");
                return true;
            }
            b.insert(at, Op::Remember { tgt: t0 as u8 });
            let from = b.program.len();
            lower_effect(l, b, then);
            if b.targets.len() != t0 + 1 {
                l.unsupported("ThenTargetControllerResolves.then(with targets)");
                return true;
            }
            for op in &mut b.program[from..] {
                match op {
                    Op::Search { who, .. } if *who == Who::You => *who = Who::LkiController,
                    _ => {
                        l.unsupported("ThenTargetControllerResolves.then(not a search)");
                        return true;
                    }
                }
            }
        }
        // "That player exiles the top N cards of their library. Until end of
        // turn, you may play those cards [without paying their mana costs]."
        // (Fallen Shinobi: "that player" is the player dealt combat damage,
        // the trigger's event player; CR 601.2a, 305.1: a land uses the land
        // play.)
        E::TargetPlayerExilesTopNYouMayPlayThem(nodes::TargetPlayerExilesTopNYouMayPlayThem { amount, free }) => {
            let n = l.int("TargetPlayerExilesTopNYouMayPlayThem.amount", *amount, 1, 10) as u8;
            let kind = crate::game::PermKind::Play { free: *free, cast_only: false, any_mana: false, only_if: crate::game::PlayIf::Always };
            b.op(Op::Ext(Ext::ExileTopWithPermission { who: Who::EventPlayer, n, kind, this_turn: true }));
        }
        // "Tap up to one target creature." (The Wondrous Wasp.)
        E::TapUpToOneTargetCreature(nodes::TapUpToOneTargetCreature { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "TapUpToOneTargetCreature") else { return true };
            let tr = &b.targets[t as usize];
            if (tr.min, tr.max) != (0, 1) {
                l.unsupported("TapUpToOneTargetCreature.targets(not up to one)");
            }
            b.op(Op::Ext(Ext::Tap { on: On::Target(t) }));
        }
        // "You take the initiative" (CR 725.2).
        E::TakeInitiative(nodes::TakeInitiative {}) => {
            initiative_defs(l);
            b.op(Op::Ext(Ext::TakeInitiative { who: Who::You }));
        }
        // "Target attacking creature connives X, where X is the number of
        // attacking creatures" (CR 701.50a).
        E::TargetConnives(nodes::TargetConnives { amount, targets, attacking_only }) => {
            let n = match amount.as_deref() {
                None => Amount::Fixed(1),
                Some(cats::Formula::CountAttackingCreatures(nodes::CountAttackingCreatures {})) => Amount::Attacking,
                Some(other) => {
                    l.unsupported(format!("TargetConnives.amount({})", other.tag()));
                    return true;
                }
            };
            let Some(t) = lower_targets(l, b, targets, "TargetConnives") else { return true };
            // (The IR repeats "attacking" in the target's filter.)
            let f = &b.targets[t as usize].filter;
            let attacking = matches!(f, Filter::And(_, x) if **x == Filter::Attacking);
            if *attacking_only != attacking {
                l.unsupported("TargetConnives.attacking_only(differs from targets)");
            }
            b.op(Op::Ext(Ext::Connive { on: On::Target(t), n }));
        }
        // "Put a +1/+1 counter on target creature an opponent controls."
        E::PutPlusOneCounterOnOpponentsCreature(nodes::PutPlusOneCounterOnOpponentsCreature { amount, targets }) => {
            let n = Amount::Fixed(l.int("PutPlusOneCounterOnOpponentsCreature.amount", *amount, 1, 20));
            let Some(t) = lower_targets(l, b, targets, "PutPlusOneCounterOnOpponentsCreature") else { return true };
            if b.targets[t as usize].filter != and(Filter::Type(types::CREATURE), Filter::Controller(false)) {
                l.unsupported("PutPlusOneCounterOnOpponentsCreature.targets(not an opponent's creature)");
            }
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n }));
        }
        // "Target <creature> gains <keywords> until end of turn. It
        // explores." (CR 701.44)
        E::GrantKeywordsUEOTThenExplores(nodes::GrantKeywordsUEOTThenExplores { targets, keywords }) => {
            let Some(t) = lower_targets(l, b, targets, "GrantKeywordsUEOTThenExplores") else { return true };
            let mods = super::effect_ext::keyword_mods(l, keywords);
            if mods.is_empty() {
                l.unsupported("GrantKeywordsUEOTThenExplores.keywords(none)");
            }
            b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods, until: Until::EndOfTurn }));
            b.op(Op::Ext(Ext::Explore { on: On::Target(t) }));
        }
        // "Untap <n> <permanent> you control" (chosen as it resolves, not
        // targeted; only tapped ones are offered, ENGINE.md 23.11).
        E::UntapChosenPermanents(nodes::UntapChosenPermanents { n, permanent_filter }) => {
            let n = l.int("UntapChosenPermanents.n", *n, 1, 9) as u8;
            let Some(p) = permanent_filter.as_deref() else {
                l.unsupported("UntapChosenPermanents.permanent_filter(missing)");
                return true;
            };
            let Some(f) = super::filter::lower_pred(l, p) else { return true };
            let filter = and(f, Filter::Tapped);
            b.op(Op::Ext(Ext::Choose { who: Who::You, filter, min: n, max: n, act: crate::program::ChooseAct::Untap }));
        }
        // "Untap all <permanents> you control".
        E::UntapAllMatching(nodes::UntapAllMatching { permanent_filter }) => {
            let Some(p) = permanent_filter.as_deref() else {
                l.unsupported("UntapAllMatching.permanent_filter(missing)");
                return true;
            };
            let Some(f) = super::filter::lower_pred(l, p) else { return true };
            b.op(Op::Ext(Ext::Untap { on: On::Each(f) }));
        }
        // "You get an experience counter" (CR 122.1).
        E::GetExperienceCounter(nodes::GetExperienceCounter {}) => {
            b.op(Op::Ext(Ext::GetExperience { n: 1 }));
        }
        // "Create a <token> [tapped and attacking] for each <formula>".
        E::CreateTokenPerFormula(nodes::CreateTokenPerFormula { token_effect, amount }) => {
            let n = match amount.as_deref() {
                Some(cats::Formula::ExperienceCounters(nodes::ExperienceCounters {})) => Amount::Experience,
                Some(other) => {
                    l.unsupported(format!("CreateTokenPerFormula.amount({})", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("CreateTokenPerFormula.amount(missing)");
                    return true;
                }
            };
            let Some((token, tapped, attacking)) = token_of(l, token_effect.as_deref(), "CreateTokenPerFormula") else { return true };
            b.op(Op::Ext(Ext::CreateTokensN { token, n, tapped, attacking }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Discard two cards, then draw two cards. For each nonland card
        // discarded this way, create a <token>."
        E::DiscardThenDrawTokensPerNonland(nodes::DiscardThenDrawTokensPerNonland { discard, draw, token_effect }) => {
            let discard = l.int("DiscardThenDrawTokensPerNonland.discard", *discard, 1, 9) as u8;
            let draw = l.int("DiscardThenDrawTokensPerNonland.draw", *draw, 0, 9) as u8;
            let Some((token, tapped, attacking)) = token_of(l, token_effect.as_deref(), "DiscardThenDrawTokensPerNonland") else { return true };
            b.op(Op::Ext(Ext::DiscardThenDraw { discard: Amount::Fixed(discard as i32), draw, types: false }));
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::Remembered, tapped, attacking }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Draw X cards, then discard X cards. Create a 1/1 white Spirit
        // creature token with flying for each card type among cards
        // discarded this way." (Occult Epiphany; CR 205.2a.)
        E::DrawDiscardThenTokensPerDiscardedType(nodes::DrawDiscardThenTokensPerDiscardedType { amount, token_printing }) => {
            let n = super::effect::int_amount(l, "DrawDiscardThenTokensPerDiscardedType.amount", amount);
            let Some(tp) = token_printing.as_deref() else {
                l.unsupported("DrawDiscardThenTokensPerDiscardedType.token_printing(missing)");
                return true;
            };
            let cats::CardPrinting::CardPrinting(tp) = tp;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("DrawDiscardThenTokensPerDiscardedType.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Draw { who: Who::You, n });
            b.op(Op::Ext(Ext::DiscardThenDraw { discard: n, draw: 0, types: true }));
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::Remembered, tapped: false, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Return this card from your graveyard to the battlefield
        // [tapped]" (an ability of a card in a graveyard).
        E::ReturnThisCardFromGraveyardToBattlefield(nodes::ReturnThisCardFromGraveyardToBattlefield { tapped }) => {
            b.op(Op::Ext(Ext::SourceFromGraveyard { tapped: *tapped }));
        }
        _ => return false,
    }
    true
}

/// The token of a `CreateToken` effect nested in another effect: its index,
/// tapped, attacking (one token per creation; the outer effect counts).
fn token_of(l: &mut Lowerer, e: Option<&cats::Effect>, what: &str) -> Option<(u16, bool, bool)> {
    match e {
        Some(cats::Effect::CreateToken(nodes::CreateToken {
            token_printing,
            count,
            tapped,
            each_player,
            attacking,
            schedule_end_of_combat_sacrifice,
        })) => {
            l.deny("CreateToken.each_player", each_player);
            l.deny("CreateToken.schedule_end_of_combat_sacrifice", schedule_end_of_combat_sacrifice);
            l.deny_ne(&format!("{what}.token_effect.count"), count, &1);
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported(format!("{what}.token_effect(not a token)"));
            }
            Some((l.token(def), *tapped, *attacking))
        }
        Some(other) => {
            l.unsupported(format!("{what}.token_effect({})", other.tag()));
            None
        }
        None => {
            l.unsupported(format!("{what}.token_effect(missing)"));
            None
        }
    }
}

// ---- ward (CR 702.21) -------------------------------------------------------------------------

/// Ward's triggered ability (CR 702.21a): "Whenever this permanent becomes
/// the target of a spell or ability an opponent controls, counter that spell
/// or ability unless that player pays <cost>." With `granted`, the ability a
/// card gives the other creatures its controller controls ("other creatures
/// you control have ward"), as that card's own trigger (ENGINE.md 27.3):
/// the target is another creature its controller controls which still has
/// the granted ability.
pub(crate) fn ward_trigger(cost: WardCost, granted: bool) -> TriggerDef {
    let mut b = Builder::default();
    let at = match cost {
        WardCost::Mana(m) => b.op(Op::OfferPay { who: Who::EventPlayer, cost: m, if_paid: 0 }),
        WardCost::Life(n) => b.op(Op::Ext(Ext::OfferPayOther { who: Who::EventPlayer, life: n, discard: false, if_paid: 0 })),
        WardCost::Discard => b.op(Op::Ext(Ext::OfferPayOther { who: Who::EventPlayer, life: 0, discard: true, if_paid: 0 })),
        WardCost::DiscardRandom => b.op(Op::Ext(Ext::EoeW4(crate::eoe_w4::EoeW4::OfferDiscardRandom { who: Who::EventPlayer, if_paid: 0 }))),
    };
    b.op(Op::Ext(Ext::CounterEventItem));
    b.patch(at);
    let opponent = TrigPred::Controlled(Slot::Controller, false);
    let subject = if granted {
        let other_creature_yours = and(
            and(Filter::Type(types::CREATURE), Filter::Controller(true)),
            Filter::Not(Box::new(Filter::Source)),
        );
        TrigPred::And(Box::new(TrigPred::Subject(Slot::Target, other_creature_yours)), Box::new(TrigPred::Granted(Slot::Target)))
    } else {
        TrigPred::IsSelf(Slot::Target)
    };
    TriggerDef { event: EventKind::BecomesTarget, pred: TrigPred::And(Box::new(subject), Box::new(opponent)), ability: b.finish(), uses_event: true }
}

// ---- the initiative and Undercity (CR 725, 309) ------------------------------------------------

/// The IR printings of the tokens Undercity's rooms create, as cube cards
/// write them (Bejeweled Warg's Treasure, Gut's Skeleton).
const TREASURE: &str = r#"{"$t": "CardPrinting", "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "sacrifice_self": true, "tap_self": true}, "effect": {"$t": "AddManaToPool", "produces": [["any", 1]]}, "is_mana_ability": true}], "card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Treasure", "subtypes": ["Treasure"], "supertypes": ["Token"], "type_line": "Token Artifact — Treasure"}"#;
const SKELETON: &str = r#"{"$t": "CardPrinting", "card_types": ["Creature"], "color_indicator": ["B"], "keywords": [{"$t": "Keyword", "kind": "menace"}], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Skeleton", "power": 4, "subtypes": ["Skeleton"], "supertypes": ["Token"], "toughness": 1, "type_line": "Token Creature — Skeleton"}"#;

/// Mobilize's token (CR 702.181a): a 1/1 red Warrior creature token.
const WARRIOR: &str = r#"{"$t": "CardPrinting", "card_types": ["Creature"], "color_indicator": ["R"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Warrior", "power": 1, "subtypes": ["Warrior"], "supertypes": ["Token"], "toughness": 1, "type_line": "Token Creature — Warrior"}"#;

fn json_token(l: &mut Lowerer, json: &str) -> u16 {
    let p: cats::CardPrinting = serde_json::from_str(json).expect("predefined token printing");
    let cats::CardPrinting::CardPrinting(tp) = &p;
    let def = super::printing::lower_printing(l, tp);
    l.token(def)
}

/// The rules definitions of the initiative (CR 725.2: its three inherent
/// triggered abilities) and of the Undercity dungeon (309.4: its rooms'
/// abilities), stored with the tokens (`CardDef::rules`; `dungeon.rs` names
/// them by these names).
fn initiative_defs(l: &mut Lowerer) {
    let treasure = json_token(l, TREASURE);
    let skeleton = json_token(l, SKELETON);
    let undercity = undercity_def(treasure, skeleton);
    l.token(undercity);
    l.token(initiative_def());
}

/// The monarch's rules definition (CR 724.2), for the cards that make a
/// player the monarch.
pub(crate) fn monarch_defs(l: &mut Lowerer) {
    l.token(monarch_def());
}

/// "The monarch": (0) "at the beginning of the monarch's end step, that
/// player draws a card"; (1) "whenever a creature deals combat damage to the
/// monarch, its controller becomes the monarch" (the creature is the
/// event's).
pub(crate) fn monarch_def() -> CardDef {
    let mut b = Builder::default();
    b.op(Op::Draw { who: Who::You, n: Amount::Fixed(1) });
    let mut triggers = vec![fired(b)];
    let mut b = Builder::default();
    b.op(Op::Ext(Ext::EventControllerBecomesMonarch));
    triggers.push(fired(b));
    CardDef { name: crate::dungeon::MONARCH.to_string(), rules: true, triggers, ..CardDef::default() }
}

/// A triggered ability that only the engine fires (never an event's).
fn fired(program: Builder) -> TriggerDef {
    let ability = program.finish();
    let uses_event = ability.uses_event();
    TriggerDef { event: EventKind::Delayed, pred: TrigPred::Always, ability, uses_event }
}

/// "The initiative": (0) "whenever a player takes the initiative, that
/// player ventures into Undercity"; (1) "at the beginning of the upkeep of
/// the player who has the initiative, that player ventures into Undercity";
/// (2) "whenever one or more creatures a player controls deal combat damage
/// to the player who has the initiative, the controller of those creatures
/// takes the initiative" (that player is the event's).
pub(crate) fn initiative_def() -> CardDef {
    let mut triggers = Vec::new();
    for _ in 0..2 {
        let mut b = Builder::default();
        b.op(Op::Ext(Ext::Venture { who: Who::You }));
        triggers.push(fired(b));
    }
    let mut b = Builder::default();
    b.op(Op::Ext(Ext::TakeInitiative { who: Who::EventPlayer }));
    triggers.push(fired(b));
    CardDef { name: crate::dungeon::INITIATIVE.to_string(), rules: true, triggers, ..CardDef::default() }
}

/// Undercity (the dungeon of the initiative, CR 309, 725): one room ability
/// per room (309.4c: "when you move your venture marker into this room,
/// <effect>"), in `dungeon::ROOMS` order.
fn undercity_def(treasure: u16, skeleton: u16) -> CardDef {
    let creature = Filter::Type(types::CREATURE);
    let mut rooms = Vec::new();
    // Secret Entrance: "Search your library for a basic land card, reveal
    // it, put it into your hand, then shuffle."
    let mut b = Builder::default();
    let basic = and(Filter::Type(types::LAND), Filter::Supertype(crate::db::supertypes::BASIC));
    b.op(Op::Search { who: Who::You, filter: basic, dest: SearchDest::Hand, reveal: true, may_fail: true });
    rooms.push(fired(b));
    // Forge: "Put two +1/+1 counters on target creature."
    let mut b = Builder::default();
    let t = b.target(creature.clone(), 1, 1);
    b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(2) }));
    rooms.push(fired(b));
    // Lost Well: "Scry 2."
    let mut b = Builder::default();
    b.op(Op::LookTop { who: Who::You, n: 2, reveal: false });
    b.op(Op::PileTo { who: Who::You, dest: PileDest::Bottom, filter: Filter::Any, min: 0, max: PILE_ANY, rest: false, reveal: false });
    b.op(Op::PileTo { who: Who::You, dest: PileDest::Top, filter: Filter::Any, min: 0, max: 0, rest: true, reveal: false });
    rooms.push(fired(b));
    // Trap!: "Target player loses 5 life."
    let mut b = Builder::default();
    let t = b.target(Filter::Player, 1, 1);
    b.op(Op::LoseLife { who: Who::TargetPlayer(t), n: Amount::Fixed(5) });
    rooms.push(fired(b));
    // Arena: "Goad target creature." (CR 701.38: until your next turn)
    let mut b = Builder::default();
    let t = b.target(creature.clone(), 1, 1);
    b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods: vec![Mod::Goad], until: Until::YourNextTurn }));
    rooms.push(fired(b));
    // Stash: "Create a Treasure token."
    let mut b = Builder::default();
    b.op(Op::CreateToken { token: treasure, n: 1, tapped: false });
    b.op(Op::Ext(Ext::TokensCreated));
    rooms.push(fired(b));
    // Archives: "Draw a card."
    let mut b = Builder::default();
    b.op(Op::Draw { who: Who::You, n: Amount::Fixed(1) });
    rooms.push(fired(b));
    // Catacombs: "Create a 4/1 black Skeleton creature token with menace."
    let mut b = Builder::default();
    b.op(Op::CreateToken { token: skeleton, n: 1, tapped: false });
    b.op(Op::Ext(Ext::TokensCreated));
    rooms.push(fired(b));
    // Throne of the Dead Three: "Reveal the top ten cards of your library.
    // Put a creature card from among them onto the battlefield with three
    // +1/+1 counters on it. It gains hexproof until your next turn. Then
    // shuffle."
    let mut b = Builder::default();
    b.op(Op::LookTop { who: Who::You, n: 10, reveal: true });
    b.op(Op::Ext(Ext::PileToBattlefield { filter: creature, counters: Some((Counter::P1P1, 3)) }));
    let mut hexproof = KwSet::default();
    hexproof.insert(Kw::Hexproof);
    b.op(Op::Ext(Ext::Apply { on: On::Made, mods: vec![Mod::Keywords(hexproof)], until: Until::YourNextTurn }));
    b.op(Op::Shuffle { who: Who::You });
    rooms.push(fired(b));
    debug_assert_eq!(rooms.len(), crate::dungeon::ROOMS.len());
    CardDef { name: crate::dungeon::UNDERCITY.to_string(), rules: true, triggers: rooms, ..CardDef::default() }
}
