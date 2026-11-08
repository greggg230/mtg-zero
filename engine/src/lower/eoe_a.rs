//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): station and the
//! "N+ |" abilities of station cards (CR 702.184, 722), counters, artifacts,
//! Equipment and Auras.

use super::activated::{lower_activated, From};
use super::Lowerer;
use crate::db::{types, AbilityDef, ActEffect, ActivatedDef, Cond, Cost, Counter, Filter, Mod, SacCost, Scope, StaticDef, TrigPred, TriggerDef};
use crate::program::{Amount, Ext, On, Op};
use card_ir::{cats, nodes};

/// The charge counter (CR 122.1; station's, 702.184a).
pub(crate) fn charge(l: &mut Lowerer) -> Counter {
    super::effect_ext::counter_by_name(l, "charge")
}

/// "As long as this permanent has N or more charge counters on it" (a
/// station symbol, CR 722.2a).
fn tier_cond(l: &mut Lowerer, what: &str, n: i64) -> Cond {
    let n = l.int(what, n, 1, 99) as u8;
    Cond::SourceCountersIn(charge(l), n, None)
}

/// A condition and another that must hold too.
fn and_cond(a: Option<Cond>, b: Cond) -> Cond {
    match a {
        None => b,
        Some(Cond::All(mut v)) => {
            v.push(b);
            Cond::All(v)
        }
        Some(a) => Cond::All(vec![a, b]),
    }
}

/// What a station card's printing adds (CR 702.184, 722).
pub(crate) struct Station {
    /// The station keyword ability (CR 702.184a): "Tap another untapped
    /// creature you control: Put a number of charge counters on this
    /// permanent equal to the tapped creature's power. Activate only as a
    /// sorcery." A station card has it at all times (722.4).
    pub ability: ActivatedDef,
    /// The static ability of the striation with the stationed keywords and,
    /// if the card has one, its power/toughness box (722.2a-b): "as long as
    /// this permanent has N or more charge counters on it, it has
    /// [abilities] [and is a creature with base power and toughness P/T in
    /// addition to its other types]". None: no keywords, no box.
    pub statik: Option<StaticDef>,
}

/// A station card's station ability and the static ability of its
/// `threshold` striation (the IR's `station_threshold`: the striation that
/// holds the `stationed_keywords` and the power/toughness box, if any).
/// `pt` is the printing's power and toughness: a station card has them only
/// as that static ability gives them (722.2c: not in any other zone, nor
/// below the threshold), so the caller's definition has none.
pub(crate) fn station(l: &mut Lowerer, card_types: u16, threshold: i64, kws: &[cats::Keyword], pt: Option<(i32, i32)>) -> Station {
    if card_types & types::CREATURE != 0 {
        // (A station card with a P/T box is a creature only at its
        // threshold; no creature card has station.)
        l.unsupported("CardPrinting.station_threshold(a creature card)");
    }
    if card_types & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.station_threshold(not a permanent card)");
    }
    let cond = tier_cond(l, "CardPrinting.station_threshold", threshold);
    let counter = charge(l);
    let mut b = crate::program::Builder::default();
    b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter, n: Amount::TappedPower }));
    let ability = ActivatedDef {
        cost: Cost {
            // "Tap another untapped creature you control" (not {T}: summoning
            // sickness doesn't matter, CR 302.6).
            tap_untapped: Some(SacCost { filter: Filter::Type(types::CREATURE), n: 1, another: true }),
            ..Cost::default()
        },
        effect: ActEffect::Stack(b.finish()),
        sorcery: true,
        instant_only: false,
        exhaust: false,
        only_if: None,
        your_turn: false,
        loyalty: None,
        discounts: Vec::new(),
        boast: false,
        x: false,
        special: None,
    };
    let mut mods = Vec::new();
    if let Some((p, t)) = pt {
        mods.push(Mod::Types { add: types::CREATURE, subtypes: Vec::new(), replace: false });
        mods.push(Mod::SetPt(p, t));
    }
    let kw = super::effect_ext::keyword_mods(l, kws);
    mods.extend(kw);
    let statik = (!mods.is_empty()).then_some(StaticDef { scope: Scope::This, cond: Some(cond), mods });
    Station { ability, statik }
}

/// A triggered ability, with its station symbol if it has one: it exists
/// only while the permanent has N or more charge counters (CR 722.2a), so
/// it triggers only then; once triggered it resolves regardless (113.7a).
pub(crate) fn lower_trigger_tiered(l: &mut Lowerer, t: &cats::TriggeredAbility, card_types: u16) -> Option<TriggerDef> {
    let cats::TriggeredAbility::TriggeredAbility(n) = t;
    if n.tier_threshold == 0 {
        return super::trigger::lower_trigger(l, t, card_types);
    }
    let plain = cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility { tier_threshold: 0, ..n.clone() });
    let mut td = super::trigger::lower_trigger(l, &plain, card_types)?;
    // (Its source's counters as the event happens: not an ability that
    // looks back at a permanent leaving the battlefield, CR 603.10a.)
    if matches!(td.event, crate::db::EventKind::Dies | crate::db::EventKind::LeavesBattlefield | crate::db::EventKind::IsSacrificed) {
        l.unsupported("TriggeredAbility.tier_threshold(a leaves-the-battlefield ability)");
    }
    let cond = tier_cond(l, "TriggeredAbility.tier_threshold", n.tier_threshold);
    td.pred = TrigPred::And(Box::new(td.pred), Box::new(TrigPred::State(cond)));
    Some(td)
}

/// A static ability, with its station symbol if it has one (CR 722.2a): it
/// applies while the source has N or more charge counters. Only a static
/// ability that modifies objects (a `StaticDef`) can carry the condition.
pub(crate) fn lower_static_tiered(l: &mut Lowerer, s: &cats::StaticAbility, repl: &mut super::statics::Repl) -> Option<StaticDef> {
    let cats::StaticAbility::StaticAbility(n) = s;
    if n.tier_threshold == 0 {
        return super::statics::lower_static(l, s, repl);
    }
    let plain = cats::StaticAbility::StaticAbility(nodes::StaticAbility { tier_threshold: 0, ..n.clone() });
    let before = format!("{repl:?}");
    let st = super::statics::lower_static(l, &plain, repl);
    if format!("{repl:?}") != before || st.is_none() {
        l.unsupported("StaticAbility.tier_threshold(not a modification of objects)");
        return None;
    }
    let mut st = st?;
    let cond = tier_cond(l, "StaticAbility.tier_threshold", n.tier_threshold);
    st.cond = Some(and_cond(st.cond.take(), cond));
    Some(st)
}

/// An activated ability of a permanent, with its station symbol if it has
/// one (CR 722.2a): the permanent has it, so it can be activated, only while
/// it has N or more charge counters (an "activate only if" that reads the
/// source, 602.5b).
pub(crate) fn lower_activated_tiered(l: &mut Lowerer, a: &cats::ActivatedAbility, walker: bool) -> Option<ActivatedDef> {
    let cats::ActivatedAbility::ActivatedAbility(n) = a;
    if n.tier_threshold == 0 {
        return lower_activated(l, a, From::Battlefield, walker);
    }
    let plain = cats::ActivatedAbility::ActivatedAbility(nodes::ActivatedAbility { tier_threshold: 0, ..n.clone() });
    let mut ad = lower_activated(l, &plain, From::Battlefield, walker)?;
    if walker {
        l.unsupported("ActivatedAbility.tier_threshold(a loyalty ability)");
    }
    let cond = tier_cond(l, "ActivatedAbility.tier_threshold", n.tier_threshold);
    ad.only_if = Some(and_cond(ad.only_if.take(), cond));
    Some(ad)
}

/// Lane A's effect classes; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_a(l: &mut Lowerer, b: &mut crate::program::Builder, e: &cats::Effect) -> bool {
    let _ = (l, b, AbilityDef::default());
    match e {
        _ => false,
    }
}
