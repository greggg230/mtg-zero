//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): station and the
//! "N+ |" abilities of station cards (CR 702.184, 722), counters, artifacts,
//! Equipment and Auras.

use super::activated::{lower_activated, From};
use super::Lowerer;
use crate::db::{types, ActEffect, ActivatedDef, Cond, Cost, Counter, Filter, Mod, SacCost, Scope, StaticDef, TrigPred, TriggerDef};
use crate::layers::Until;
use crate::program::{Amount, Builder, EoeA, Ext, On, Op, SearchDest, Who};
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
    let mut b = Builder::default();
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

/// "If it has a +1/+1 counter on it, draw n cards" after an effect on the
/// target of requirement `t` (checked as it resolves).
pub(crate) fn draw_if_target_has_counter(l: &mut Lowerer, b: &mut Builder, t: u8, n: i64) {
    let n = l.int("draw_if_target_has_plus_one_counter", n, 1, 9);
    let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: Filter::HasCounter(Counter::P1P1), to: 0 });
    b.op(Op::Draw { who: Who::You, n: Amount::Fixed(n) });
    b.patch(at);
}

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// Lane A's effect classes; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_a(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use super::effect::lower_targets;
    use cats::Effect as E;
    match e {
        // "Exile all creatures and Spacecraft." (At once.)
        E::ExileAllCreaturesAndSpacecraft(nodes::ExileAllCreaturesAndSpacecraft {}) => {
            let filter = Filter::Or(vec![Filter::Type(types::CREATURE), Filter::Subtype(l.subtype("Spacecraft"))]);
            b.op(Op::Ext(Ext::ExileAll { filter, basics: None }));
        }
        // "Exile all artifacts with mana value n or less."
        E::ExileAllArtifactsWithManaValueAtMost(nodes::ExileAllArtifactsWithManaValueAtMost { max_mv }) => {
            let n = l.int("ExileAllArtifactsWithManaValueAtMost.max_mv", *max_mv, 0, 30) as u8;
            let filter = and(Filter::Type(types::ARTIFACT), Filter::ManaValueAtMost(n));
            b.op(Op::Ext(Ext::ExileAll { filter, basics: None }));
        }
        // "Search your library for an Equipment card, put it onto the
        // battlefield, then shuffle." (A card with a stated quality: the
        // search may fail, CR 701.19b.)
        E::SearchLibraryForEquipmentToBattlefield(nodes::SearchLibraryForEquipmentToBattlefield {}) => {
            let filter = Filter::Subtype(l.subtype("Equipment"));
            b.op(Op::Search { who: Who::You, filter, dest: SearchDest::Battlefield { tapped: false }, reveal: false, may_fail: true });
        }
        // "<This> deals 3 damage to target creature. It deals 5 damage
        // instead if you control a Spacecraft." (As it resolves.)
        E::DealDamageToTargetCreatureSpacecraftBoosted(nodes::DealDamageToTargetCreatureSpacecraftBoosted { base, boosted, targets }) => {
            let Some(t) = lower_targets(l, b, targets, "DealDamageToTargetCreatureSpacecraftBoosted") else { return true };
            let base = l.int("DealDamageToTargetCreatureSpacecraftBoosted.base", *base, 0, 99);
            let boosted = l.int("DealDamageToTargetCreatureSpacecraftBoosted.boosted", *boosted, 0, 99);
            let cond = Cond::ControlAtLeast(Filter::Subtype(l.subtype("Spacecraft")), 1);
            let at = b.op(Op::JumpUnless { cond, to: 0 });
            b.op(Op::DealDamage { amount: Amount::Fixed(boosted), to: t });
            let end = b.op(Op::Jump { to: 0 });
            b.patch(at);
            b.op(Op::DealDamage { amount: Amount::Fixed(base), to: t });
            b.patch(end);
        }
        // "<This> deals damage equal to the number of artifacts you control
        // to target creature an opponent controls." (Counted as it
        // resolves.)
        E::DealDamageToOpponentCreatureByArtifactCount(nodes::DealDamageToOpponentCreatureByArtifactCount { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "DealDamageToOpponentCreatureByArtifactCount") else { return true };
            b.op(Op::DealDamage { amount: Amount::CountTypes(types::ARTIFACT), to: t });
        }
        // "Put n charge counters on target Spacecraft or Planet you control."
        E::PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(nodes::PutChargeCountersOnTargetSpacecraftOrPlanetYouControl { amount }) => {
            let n = l.int("PutChargeCountersOnTargetSpacecraftOrPlanetYouControl.amount", *amount, 1, 99);
            let kinds = Filter::Or(vec![Filter::Subtype(l.subtype("Spacecraft")), Filter::Subtype(l.subtype("Planet"))]);
            let t = b.target(and(kinds, Filter::Controller(true)), 1, 1);
            let counter = charge(l);
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter, n: Amount::Fixed(n) }));
        }
        // "Target artifact or creature's owner puts it on their choice of the
        // top or bottom of their library."
        E::OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(nodes::OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary { nonland_permanent }) => {
            let f = if *nonland_permanent {
                and(Filter::Type(types::PERMANENT), Filter::Not(Box::new(Filter::Type(types::LAND))))
            } else {
                Filter::Type(types::ARTIFACT | types::CREATURE)
            };
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::EoeA(EoeA::OwnerPutsTopOrBottom { tgt: t })));
        }
        // "Destroy target artifact or tapped creature." / "Destroy target
        // artifact, enchantment, or creature with flying." (The IR's
        // target specs say which.)
        E::DestroyTargetArtifactOrTappedCreature(nodes::DestroyTargetArtifactOrTappedCreature { targets })
        | E::DestroyTargetArtifactEnchantmentOrFlyingCreature(nodes::DestroyTargetArtifactEnchantmentOrFlyingCreature { targets }) => {
            let Some(t) = lower_targets(l, b, targets, e.tag()) else { return true };
            b.op(Op::Destroy { tgt: t });
        }
        // "Return target artifact card from your graveyard to your hand."
        E::ReturnTargetArtifactCardFromYourGraveyardToHand(nodes::ReturnTargetArtifactCardFromYourGraveyardToHand {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        }) => {
            let _ = (log_suffix, target_kind); // display, Python dispatch
            let what = "ReturnTargetArtifactCardFromYourGraveyardToHand";
            l.deny(&format!("{what}.creature"), creature);
            l.deny(&format!("{what}.land"), land);
            l.deny(&format!("{what}.nonland_permanent"), nonland_permanent);
            l.deny(&format!("{what}.or_enchantment"), or_enchantment);
            l.deny(&format!("{what}.or_spacecraft"), or_spacecraft);
            l.deny(&format!("{what}.or_artifact"), or_artifact);
            l.deny(&format!("{what}.max_mv"), max_mv);
            l.deny(&format!("{what}.max_total_mv"), max_total_mv);
            l.deny(&format!("{what}.plus_one_counter"), plus_one_counter);
            l.deny(&format!("{what}.untargeted_all"), untargeted_all);
            if !*artifact {
                l.unsupported(format!("{what}(not an artifact card)"));
            }
            if *destination != card_ir::enums::GraveyardReturnDestination::Hand {
                l.unsupported(format!("{what}.destination"));
            }
            let lo = l.int(&format!("{what}.min_n"), *min_n, 0, 8) as u8;
            let hi = l.int(&format!("{what}.max_n"), *max_n, 1, 8) as u8;
            let f = and(Filter::Type(types::ARTIFACT), Filter::Owner(true));
            let tgt = b.target(Filter::InGraveyard(Box::new(f)), lo, hi);
            b.op(Op::Ext(Ext::FromGraveyard { tgt, to_battlefield: false, yours: true, tapped: false, counter: None }));
        }
        // "That player gets n poison counters" (CR 122.1f): the class's
        // "target" player is the one its trigger names (Virulent Silencer's
        // "deals combat damage to a player": the event's player).
        E::PutPoisonCountersOnTargetPlayer(nodes::PutPoisonCountersOnTargetPlayer { amount }) => {
            let n = l.int("PutPoisonCountersOnTargetPlayer.amount", *amount, 1, 20);
            b.op(Op::Ext(Ext::Poison { who: Who::EventPlayer, n: Amount::Fixed(n) }));
        }
        // "Up to one other target artifact you control becomes an artifact
        // creature with base power and toughness p/t and gains <keywords>
        // until end of turn." ("An artifact creature": it keeps its other
        // types, CR 205.1b; base P/T in layer 7b, 613.4b.)
        E::TargetArtifactBecomesCreatureUEOT(nodes::TargetArtifactBecomesCreatureUEOT { power, toughness, granted_keywords }) => {
            let f = and(and(Filter::Type(types::ARTIFACT), Filter::Controller(true)), Filter::Not(Box::new(Filter::Source)));
            let t = b.target(f, 0, 1);
            let mut mods = vec![
                Mod::Types { add: types::ARTIFACT | types::CREATURE, subtypes: Vec::new(), replace: false },
                Mod::SetPt(
                    l.int("TargetArtifactBecomesCreatureUEOT.power", *power, 0, 99),
                    l.int("TargetArtifactBecomesCreatureUEOT.toughness", *toughness, 0, 99),
                ),
            ];
            mods.extend(super::effect_ext::keyword_mods(l, granted_keywords));
            b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods, until: Until::EndOfTurn }));
        }
        // "When this Equipment enters, attach it to target creature you
        // control. That creature gains <keywords> until end of turn." (CR
        // 701.3; the creature gains them even if the attach does nothing.)
        E::AttachToTargetCreatureYouControl(nodes::AttachToTargetCreatureYouControl { granted_keywords_ueot }) => {
            let t = b.target(and(Filter::Type(types::CREATURE), Filter::Controller(true)), 1, 1);
            b.op(Op::Ext(Ext::Attach { tgt: t }));
            let mods = super::effect_ext::keyword_mods(l, granted_keywords_ueot);
            if !mods.is_empty() {
                b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods, until: Until::EndOfTurn }));
            }
        }
        _ => return false,
    }
    true
}
