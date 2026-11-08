//! Triggered abilities: events and trigger predicates -> [`TriggerDef`]
//! (CR 603; ENGINE.md 8).

use super::effect::lower_ability;
use super::filter::lower_pred;
use super::Lowerer;
use crate::db::{types, Cond, EventKind, Filter, Slot, TrigPred, TriggerDef};
use card_ir::{cats, enums, nodes};

pub(crate) fn lower_trigger(l: &mut Lowerer, t: &cats::TriggeredAbility, card_types: u16) -> Option<TriggerDef> {
    let cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility {
        effect,
        requires_void,
        once_per_turn,
        tier_threshold,
        event,
        predicate,
        room_half,
        functions_in_graveyard,
    }) = t;
    l.deny("TriggeredAbility.tier_threshold", tier_threshold);
    l.deny("TriggeredAbility.room_half", room_half);

    use enums::Event as E;
    // "Whenever one or more cards are put into exile from your library
    // and/or your graveyard" (Laelia): the IR's marker predicate, with the
    // `dies` event as a placeholder (the Python engine sweeps for it).
    if let (Some(E::Dies), Some(cats::TriggerPredicate::CardsExiledFromLibraryOrGraveyard(nodes::CardsExiledFromLibraryOrGraveyard {}))) =
        (event, predicate.as_deref())
    {
        if !l.watches_exile {
            l.unsupported("TriggerPredicate:CardsExiledFromLibraryOrGraveyard(no watches_library_graveyard_exile)");
        }
        let Some(e) = effect else {
            l.unsupported("TriggeredAbility.effect(missing)");
            return None;
        };
        let ability = lower_ability(l, e);
        let pred = TrigPred::And(Box::new(TrigPred::Controlled(Slot::Player, true)), Box::new(TrigPred::Batch));
        if *once_per_turn {
            l.unsupported("TriggeredAbility.once_per_turn(cards exiled)");
        }
        l.deny("TriggeredAbility.requires_void(cards exiled)", requires_void);
        let uses_event = ability.uses_event();
        return Some(TriggerDef { event: EventKind::ExiledFromLibGy, pred, ability, uses_event });
    }
    // "When this is put into a graveyard from anywhere" (Worldspine Wurm,
    // Emrakul): the IR's `dies` event with this marker predicate; it
    // triggers from the graveyard, the card's own ability (CR 603.6a, its
    // owner controls it).
    if let (Some(E::Dies), Some(cats::TriggerPredicate::PutIntoGraveyardFromAnywhere(nodes::PutIntoGraveyardFromAnywhere {}))) =
        (event, predicate.as_deref())
    {
        let Some(e) = effect else {
            l.unsupported("TriggeredAbility.effect(missing)");
            return None;
        };
        if *once_per_turn {
            l.unsupported("TriggeredAbility.once_per_turn(from anywhere)");
        }
        l.deny("TriggeredAbility.requires_void(from anywhere)", requires_void);
        let ability = lower_ability(l, e);
        let uses_event = ability.uses_event();
        return Some(TriggerDef { event: EventKind::ToGraveyard, pred: TrigPred::IsSelf(Slot::Default), ability, uses_event });
    }
    let ev = match event {
        Some(E::Enters) => Some(EventKind::Enters),
        Some(E::Dies) => Some(EventKind::Dies),
        Some(E::LeavesBattlefield) => Some(EventKind::LeavesBattlefield),
        Some(E::Attacks) => Some(EventKind::Attacks),
        Some(E::YouAttack) => Some(EventKind::YouAttack),
        Some(E::DealsCombatDamage) => Some(EventKind::DealsCombatDamage),
        Some(E::IsCast) => Some(EventKind::IsCast),
        Some(E::DrawsCard) => Some(EventKind::DrawsCard),
        Some(E::BecomesTapped) => Some(EventKind::BecomesTapped),
        Some(E::IsSacrificed) => Some(EventKind::IsSacrificed),
        Some(E::BeginningOfUpkeep) => Some(EventKind::BeginningOfUpkeep),
        Some(E::BeginningOfDrawStep) => Some(EventKind::BeginningOfDrawStep),
        Some(E::BeginningOfPrecombatMain) => Some(EventKind::BeginningOfPrecombatMain),
        Some(E::BeginningOfCombat) => Some(EventKind::BeginningOfCombat),
        Some(E::BeginningOfEndStep) => Some(EventKind::BeginningOfEndStep),
        Some(E::DealtDamage) => Some(EventKind::DealtDamage),
        Some(E::CardDiscarded) => Some(EventKind::CardDiscarded),
        // "Whenever you discard ..." (Inti): the same event, by the
        // discarding player (one per card; "one or more" batches them).
        Some(E::DiscardsCard) => Some(EventKind::CardDiscarded),
        Some(E::BecomesMonstrous) => Some(EventKind::BecomesMonstrous),
        Some(E::Blocks) => Some(EventKind::Blocks),
        Some(E::Transforms) => Some(EventKind::Transforms),
        Some(E::SagaChapter) => Some(EventKind::Chapter),
        // "When this Class becomes level n" (CR 716.2c).
        Some(E::ClassLevelGained) => Some(EventKind::ClassLevel),
        Some(E::DealsDamage) => Some(EventKind::DealsDamage),
        Some(E::DealsCombatDamageToCreature) => Some(EventKind::DealsCombatDamageToCreature),
        Some(E::CreatureTokensCreated) => Some(EventKind::CreatureTokensCreated),
        // "Becomes the target of a spell or ability" (ENGINE.md 27.2).
        Some(E::BecomesTarget) => Some(EventKind::BecomesTarget),
        // "When you play another land" (CR 305.1: playing, not putting).
        Some(E::PlaysLand) => Some(EventKind::PlaysLand),
        // "Whenever one or more +1/+1 counters are put on this creature".
        Some(E::CountersAdded) => Some(EventKind::CountersAdded),
        // "Whenever one or more cards are put into a library from anywhere".
        Some(E::CardsPutIntoLibrary) => Some(EventKind::CardsPutIntoLibrary),
        // "Whenever one or more creatures an opponent controls attack you and
        // aren't blocked".
        Some(E::AttackersUnblocked) => Some(EventKind::AttackersUnblocked),
        // "Whenever an instant or sorcery card leaves your graveyard"
        // (Murktide Regent; ENGINE.md 54).
        Some(E::LeavesGraveyard) => Some(EventKind::LeavesGraveyard),
        Some(other) => {
            l.unsupported(format!("Event:{other:?}"));
            None
        }
        None => {
            l.unsupported("TriggeredAbility.event(missing)");
            None
        }
    };
    let pred = match predicate.as_deref() {
        Some(p) => lower_trig_pred(l, p),
        None => {
            l.unsupported("TriggeredAbility.predicate(missing)");
            None
        }
    };
    // "Whenever <a player> discards a card": the IR's `discards_card` event
    // has one subject, the discarding player (its default); the engine's
    // event names the card too (ENGINE.md 99.4).
    let pred = if *event == Some(E::DiscardsCard) { pred.map(|p| default_slot_to(p, Slot::Player)) } else { pred };
    // "When this enters, if you cast it, <effect>" (The One Ring; the IR's
    // `ConditionalIfCastFromHand` stands for "if you cast it", its
    // docstring's reading): an intervening "if" on how it entered (CR 603.4;
    // it can't change before the ability resolves).
    let (effect, pred) = match (effect.as_deref(), ev, pred) {
        (Some(cats::Effect::ConditionalIfCastFromHand(nodes::ConditionalIfCastFromHand { inner_effect })), Some(EventKind::Enters), Some(p)) => {
            (Some(inner_effect), Some(TrigPred::And(Box::new(p), Box::new(TrigPred::EnteredCast))))
        }
        (_, _, p) => (effect.as_ref(), p),
    };
    let mut ability = match effect {
        Some(e) => lower_ability(l, e),
        None => {
            l.unsupported("TriggeredAbility.effect(missing)");
            return None;
        }
    };
    // "You may [do this]. Do this only once each turn" (The Sensational
    // She-Hulk, CR 603.2h): what is limited is the optional action, done
    // or not; the ability doesn't trigger once it was done that turn
    // (`triggers.rs`). Any other effect: "this ability triggers only once
    // each turn" (Enduring Innocence; the IR's one flag for both,
    // docs/ir_defects.md 14), `TrigPred::Once`.
    let mut once = false;
    if *once_per_turn {
        match ability.program.first() {
            Some(&crate::program::Op::May { who: crate::program::Who::You, skip }) if ability.modes.is_none() => {
                ability.program[0] = crate::program::Op::Ext(crate::program::Ext::OnceMay { skip });
            }
            _ => once = true,
        }
    }
    let (event, mut pred) = (ev?, pred?);
    if once {
        pred = TrigPred::And(Box::new(TrigPred::Once), Box::new(pred));
    }
    // Void (Edge of Eternities' ability word): "..., if a nonland permanent
    // left the battlefield this turn or a spell was warped this turn, ...":
    // an intervening "if" (CR 603.4), checked again as it resolves
    // (ENGINE.md 99.4).
    if *requires_void {
        pred = TrigPred::And(Box::new(pred), Box::new(TrigPred::If(Cond::Void)));
    }
    // "When this creature dies" (CR 700.4): it was a creature as it left;
    // the IR's `dies` event covers any permanent put into a graveyard from
    // the battlefield, with typed subjects for the others.
    if event == EventKind::Dies && pred == TrigPred::IsSelf(Slot::Default) && card_types & types::CREATURE != 0 {
        pred = TrigPred::And(Box::new(pred), Box::new(TrigPred::Subject(Slot::Default, Filter::Type(types::CREATURE))));
    }
    if let Some(s) = slots_used(&pred).into_iter().find(|&s| event.slot_index(s).is_none()) {
        l.unsupported(format!("TriggerPredicate.subject({s:?} of {event:?})"));
    }
    // "If this spell was kicked" on a cast trigger reads the spell.
    if event == EventKind::IsCast {
        pred = paid_on_cast(pred);
    }
    // Predicates read from one event kind's note.
    let (enters, cast) = (needs(&pred, &|p| matches!(p, TrigPred::EnteredCast | TrigPred::EnteredPaid | TrigPred::EnteredXAtLeast(_))), needs(&pred, &|p| matches!(p, TrigPred::SpellPaid | TrigPred::OrCopy)));
    if enters && event != EventKind::Enters || cast && event != EventKind::IsCast {
        l.unsupported(format!("TriggerPredicate(how it was cast, on {event:?})"));
    }
    // "At the beginning of ..., if this is tapped" (Mana Vault): an
    // intervening "if" about the source (CR 603.4), checked again as the
    // ability resolves.
    let step = matches!(
        event,
        EventKind::BeginningOfUpkeep
            | EventKind::BeginningOfDrawStep
            | EventKind::BeginningOfPrecombatMain
            | EventKind::BeginningOfCombat
            | EventKind::BeginningOfEndStep
    );
    if step {
        // (Not "once harnessed": that says whether the ability exists, and
        // a triggered ability resolves without its source, CR 113.7a.)
        if let Some(f) = source_condition(&pred).filter(|f| *f != Filter::Harnessed) {
            let mut b = crate::program::Builder { program: std::mem::take(&mut ability.program), ..Default::default() };
            b.insert(0, crate::program::Op::Ext(crate::program::Ext::JumpUnlessSource { filter: f, to: 0 }));
            b.patch(0);
            ability.program = b.program;
        }
    }
    // "Return this card from your graveyard ..." (Sword of the Meek): the
    // ability functions in the graveyard only.
    if *functions_in_graveyard {
        pred = TrigPred::And(Box::new(TrigPred::InGraveyard), Box::new(pred));
    }
    let uses_event = ability.uses_event();
    Some(TriggerDef { event, pred, ability, uses_event })
}

/// A predicate's default subject slot named `to` (an IR event with one
/// subject that the engine's event kind gives two).
fn default_slot_to(p: TrigPred, to: Slot) -> TrigPred {
    let s = |x: Slot| if x == Slot::Default { to } else { x };
    match p {
        TrigPred::IsSelf(x) => TrigPred::IsSelf(s(x)),
        TrigPred::Subject(x, f) => TrigPred::Subject(s(x), f),
        TrigPred::Controlled(x, you) => TrigPred::Controlled(s(x), you),
        TrigPred::FormerHost(x) => TrigPred::FormerHost(s(x)),
        TrigPred::Granted(x) => TrigPred::Granted(s(x)),
        TrigPred::IsHost(x) => TrigPred::IsHost(s(x)),
        TrigPred::And(a, b) => TrigPred::And(Box::new(default_slot_to(*a, to)), Box::new(default_slot_to(*b, to))),
        TrigPred::Or(a, b) => TrigPred::Or(Box::new(default_slot_to(*a, to)), Box::new(default_slot_to(*b, to))),
        TrigPred::Not(a) => TrigPred::Not(Box::new(default_slot_to(*a, to))),
        other => other,
    }
}

/// `EnteredPaid` -> `SpellPaid` (a cast trigger's "if it was kicked").
fn paid_on_cast(p: TrigPred) -> TrigPred {
    match p {
        TrigPred::EnteredPaid => TrigPred::SpellPaid,
        TrigPred::And(a, b) => TrigPred::And(Box::new(paid_on_cast(*a)), Box::new(paid_on_cast(*b))),
        TrigPred::Or(a, b) => TrigPred::Or(Box::new(paid_on_cast(*a)), Box::new(paid_on_cast(*b))),
        TrigPred::Not(a) => TrigPred::Not(Box::new(paid_on_cast(*a))),
        other => other,
    }
}

/// A condition on the source at the top level of a predicate (`Source`,
/// possibly under `And`).
fn source_condition(p: &TrigPred) -> Option<Filter> {
    match p {
        TrigPred::Source(f) => Some(f.clone()),
        TrigPred::And(a, b) => source_condition(a).or_else(|| source_condition(b)),
        _ => None,
    }
}

/// Does some part of the predicate satisfy `f`?
fn needs(p: &TrigPred, f: &dyn Fn(&TrigPred) -> bool) -> bool {
    f(p)
        || match p {
            TrigPred::And(a, b) | TrigPred::Or(a, b) => needs(a, f) || needs(b, f),
            TrigPred::Not(a) => needs(a, f),
            _ => false,
        }
}

/// Every subject slot a predicate names (checked against the event).
fn slots_used(p: &TrigPred) -> Vec<Slot> {
    match p {
        TrigPred::And(a, b) | TrigPred::Or(a, b) => {
            let mut v = slots_used(a);
            v.extend(slots_used(b));
            v
        }
        TrigPred::Not(a) => slots_used(a),
        TrigPred::IsSelf(s)
        | TrigPred::Subject(s, _)
        | TrigPred::Controlled(s, _)
        | TrigPred::FormerHost(s)
        | TrigPred::Granted(s)
        | TrigPred::IsHost(s) => {
            vec![*s]
        }
        TrigPred::Always
        | TrigPred::If(_)
        | TrigPred::State(_)
        | TrigPred::Source(_)
        | TrigPred::Chapter(_)
        | TrigPred::Level(_)
        | TrigPred::TimesPerSubject(..)
        | TrigPred::Batch
        | TrigPred::HostAttacksWithAnother
        | TrigPred::EnteredCast
        | TrigPred::EnteredPaid
        | TrigPred::SpellPaid
        | TrigPred::OrCopy
        | TrigPred::EnteredXAtLeast(_)
        | TrigPred::FirstAttack
        | TrigPred::Once
        | TrigPred::EnteredNotFromGyExile
        | TrigPred::InGraveyard
        | TrigPred::SpellMvParity
        | TrigPred::AttacksMonarch => Vec::new(),
    }
}

fn slot(l: &mut Lowerer, s: &Option<enums::EventSubject>) -> Option<Slot> {
    use enums::EventSubject as S;
    Some(match s {
        None => Slot::Default,
        Some(S::Obj) => Slot::Obj,
        Some(S::Attacker) => Slot::Attacker,
        Some(S::Blocker) => Slot::Blocker,
        Some(S::Source) => Slot::Source,
        Some(S::Target) => Slot::Target,
        Some(S::Spell) => Slot::Spell,
        Some(S::Controller) => Slot::Controller,
        Some(S::Player) => Slot::Player,
        Some(S::Card) => Slot::Card,
        Some(S::TargetSpell) => Slot::TargetSpell,
        Some(S::Land) => Slot::Land,
        Some(S::Defender) => Slot::Defender,
        Some(other) => {
            l.unsupported(format!("TriggerPredicate.subject({other:?})"));
            return None;
        }
    })
}

fn lower_trig_pred(l: &mut Lowerer, p: &cats::TriggerPredicate) -> Option<TrigPred> {
    use cats::TriggerPredicate as P;
    let b = Box::new;
    match p {
        P::Always(nodes::Always {}) => Some(TrigPred::Always),
        P::And(nodes::And { left, right }) => {
            let a = lower_trig_pred(l, left);
            let c = lower_trig_pred(l, right);
            Some(TrigPred::And(b(a?), b(c?)))
        }
        P::Or(nodes::Or { left, right }) => {
            let a = lower_trig_pred(l, left);
            let c = lower_trig_pred(l, right);
            Some(TrigPred::Or(b(a?), b(c?)))
        }
        P::Not(nodes::Not { inner }) => Some(TrigPred::Not(b(lower_trig_pred(l, inner)?))),
        P::IsSelf(nodes::IsSelf { subject }) => Some(TrigPred::IsSelf(slot(l, subject)?)),
        P::IsThisSpell(nodes::IsThisSpell {}) => Some(TrigPred::IsSelf(Slot::Spell)),
        P::IsFormerHost(nodes::IsFormerHost { subject }) => Some(TrigPred::FormerHost(slot(l, subject)?)),
        // "Whenever you cast or copy": a copy of a spell counts too (CR
        // 707.10; `TrigPred::OrCopy`).
        P::CastOrCopy(nodes::CastOrCopy {}) => Some(TrigPred::OrCopy),
        // "If you cast it": it entered as a resolving spell its controller
        // cast (its `Enters` event's note).
        P::EnteredViaCast(nodes::EnteredViaCast {}) => Some(TrigPred::EnteredCast),
        // "If you've cast another spell this turn" (while casting this one;
        // CR 603.4, checked again on resolution).
        P::CastAnotherSpellThisTurn(nodes::CastAnotherSpellThisTurn {}) => Some(TrigPred::If(Cond::SpellsCastAtLeast(2))),
        // "If its squad / offspring cost was paid" (CR 702.157a, 702.175a):
        // how its spell was cast, the `Enters` event's note.
        P::Source(nodes::Source { inner })
            if matches!(**inner, cats::ObjectPredicate::SquadCostPaid(_) | cats::ObjectPredicate::OffspringCostPaid(_)) =>
        {
            Some(TrigPred::EnteredPaid)
        }
        P::Subject(nodes::Subject { slot: s, inner }) => {
            let s = slot(l, s);
            let f = lower_pred(l, inner);
            Some(TrigPred::Subject(s?, f?))
        }
        P::Controlled(nodes::Controlled { by, subject }) => {
            let you = *by == enums::Controller::You;
            Some(TrigPred::Controlled(slot(l, subject)?, you))
        }
        P::Source(nodes::Source { inner }) => Some(TrigPred::Source(lower_pred(l, inner)?)),
        P::HostAttacksWithAnother(nodes::HostAttacksWithAnother {}) => Some(TrigPred::HostAttacksWithAnother),
        // "Whenever this creature attacks for the first time each turn".
        P::FirstAttackThisTurn(nodes::FirstAttackThisTurn {}) => Some(TrigPred::FirstAttack),
        // Ravenous: "if X is 5 or more, draw a card when it enters" (CR
        // 702.156a).
        P::EnteringWithXAtLeast(nodes::EnteringWithXAtLeast { n }) => {
            Some(TrigPred::EnteredXAtLeast(l.int("EnteringWithXAtLeast.n", *n, 1, 99) as u8))
        }
        // "Whenever one or more ..." (one trigger per simultaneous batch).
        P::OneOrMoreBatch(nodes::OneOrMoreBatch {}) => Some(TrigPred::Batch),
        // A chapter ability (CR 714.2b).
        P::ChapterReached(nodes::ChapterReached { n }) => Some(TrigPred::Chapter(l.int("ChapterReached.n", *n, 1, 20) as u8)),
        // "When this Class becomes level n" (CR 716.2c).
        P::ClassLevelReached(nodes::ClassLevelReached { n }) => Some(TrigPred::Level(l.int("ClassLevelReached.n", *n, 2, 9) as u8)),
        // "When you draw your nth card in a turn": the drawing player (the
        // draw event's subject, after the draw counts) has drawn n.
        P::NthCardDrawnThisTurn(nodes::NthCardDrawnThisTurn { n }) => Some(TrigPred::Subject(
            Slot::Player,
            Filter::DrewThisTurn(l.int("NthCardDrawnThisTurn.n", *n, 1, 99) as u8),
        )),
        // "A spell with mana value of the chosen quality" (Gollum).
        P::SpellMVHasSourceParity(nodes::SpellMVHasSourceParity {}) => Some(TrigPred::SpellMvParity),
        // "Enters from anywhere other than a graveyard or exile" ("Name
        // Sticker" Goblin).
        P::EnteredNotFromZones(nodes::EnteredNotFromZones { zones }) => {
            let mut z = zones.clone();
            z.sort_by_key(|x| format!("{x:?}"));
            if z != [enums::ZoneName::Exile, enums::ZoneName::Graveyard] {
                l.unsupported(format!("EnteredNotFromZones.zones({zones:?})"));
                return None;
            }
            Some(TrigPred::EnteredNotFromGyExile)
        }
        // "If you control n or fewer creatures named <this card's name>".
        P::ControlsAtMostNamed(nodes::ControlsAtMostNamed { name, at_most }) => {
            let n = l.int("ControlsAtMostNamed.at_most", *at_most, 0, 99) as u8;
            let f = Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Named(name.clone())));
            Some(TrigPred::If(Cond::ControlAtMost(f, n)))
        }
        // "If it was kicked" (CR 702.33d): how it was cast; `EnteredPaid`
        // for an enter trigger, `SpellPaid` for a cast trigger (fixed up
        // by the caller from the event).
        P::ThisSpellWasKicked(nodes::ThisSpellWasKicked {}) => Some(TrigPred::EnteredPaid),
        P::StateBound(nodes::StateBound { inner }) => {
            let (c, intervening) = lower_state_pred(l, inner)?;
            Some(if intervening { TrigPred::If(c) } else { TrigPred::State(c) })
        }
        other => {
            l.unsupported(format!("TriggerPredicate:{}", other.tag()));
            None
        }
    }
}

/// A state predicate for the source's controller -> a [`Cond`], and
/// whether it is an intervening "if" (CR 603.4) rather than part of the
/// trigger event ("your second spell each turn").
pub(crate) fn lower_state_pred(l: &mut Lowerer, p: &cats::StatePredicate) -> Option<(Cond, bool)> {
    use cats::StatePredicate as S;
    Some(match p {
        S::CardsInGraveyardAtLeast(nodes::CardsInGraveyardAtLeast { n }) => {
            (Cond::GraveyardAtLeast(l.int("CardsInGraveyardAtLeast.n", *n, 0, 99) as u8), true)
        }
        S::Delirium(nodes::Delirium { min_card_types }) => {
            (Cond::Delirium(l.int("Delirium.min_card_types", *min_card_types, 1, 9) as u8), true)
        }
        S::GainedLifeThisTurn(nodes::GainedLifeThisTurn {}) => (Cond::GainedLifeThisTurn, true),
        S::IsMonarch(nodes::IsMonarch {}) => (Cond::IsMonarch, true),
        S::OpponentHasMoreCardsInHand(nodes::OpponentHasMoreCardsInHand {}) => (Cond::OpponentHasMoreCards, true),
        S::YouControlAtLeast(nodes::YouControlAtLeast { n, card_type }) => {
            let Some(bits) = types::from_name(card_type) else {
                l.unsupported(format!("YouControlAtLeast.card_type({card_type})"));
                return None;
            };
            (Cond::ControlAtLeast(Filter::Type(bits), l.int("YouControlAtLeast.n", *n, 0, 99) as u8), true)
        }
        S::SpellsCastThisTurn(nodes::SpellsCastThisTurn { n }) => {
            let n = l.int("SpellsCastThisTurn.n", *n, 1, 99) as u8;
            (Cond::SpellsCastThisTurn { n, noncreature: false }, false)
        }
        S::NoncreatureSpellsCastThisTurn(nodes::NoncreatureSpellsCastThisTurn { n }) => {
            let n = l.int("NoncreatureSpellsCastThisTurn.n", *n, 1, 99) as u8;
            (Cond::SpellsCastThisTurn { n, noncreature: true }, false)
        }
        // "If you have the initiative" (CR 725); "if you've completed a
        // dungeon" (309.7).
        S::HasInitiative(nodes::HasInitiative {}) => (Cond::HasInitiative, true),
        S::CompletedADungeon(nodes::CompletedADungeon {}) => (Cond::CompletedDungeon, true),
        // "If it wasn't the first land you played this turn" (Fastbond; n
        // = 2 by the schema's default).
        S::LandsPlayedThisTurnAtLeast(nodes::LandsPlayedThisTurnAtLeast { n }) => {
            (Cond::LandsPlayedAtLeast(l.int("LandsPlayedThisTurnAtLeast.n", *n, 1, 99) as u8), true)
        }
        // Void: an intervening "if" (CR 603.4; ENGINE.md 99.4).
        S::VoidActive(nodes::VoidActive {}) => (Cond::Void, true),
        other => {
            l.unsupported(format!("StatePredicate:{}", other.tag()));
            return None;
        }
    })
}

/// A triggered ability an Equipment grants its creature, as the
/// Equipment's own: "this creature" becomes "the equipped creature".
pub(crate) fn granted_to_host(p: TrigPred) -> TrigPred {
    match p {
        TrigPred::IsSelf(s) => TrigPred::IsHost(s),
        TrigPred::And(a, b) => TrigPred::And(Box::new(granted_to_host(*a)), Box::new(granted_to_host(*b))),
        TrigPred::Or(a, b) => TrigPred::Or(Box::new(granted_to_host(*a)), Box::new(granted_to_host(*b))),
        TrigPred::Not(a) => TrigPred::Not(Box::new(granted_to_host(*a))),
        other => other,
    }
}
