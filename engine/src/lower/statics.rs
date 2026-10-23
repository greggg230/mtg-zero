//! Static abilities that modify objects (CR 604, 613) and
//! characteristic-defining power / toughness -> [`StaticDef`], [`PtCda`].

use super::filter::{lower_condition, lower_pred};
use super::Lowerer;
use crate::db::{types, Filter, Mod, PtCda, Scope, StaticDef};
use card_ir::{cats, enums, generated, nodes};

/// Does a static's scope filter use only what a static scope may read
/// (types, subtypes, colours, controller, token-ness; ENGINE.md 2a)?
fn scope_filter_ok(f: &Filter) -> bool {
    match f {
        Filter::Any
        | Filter::Type(_)
        | Filter::Subtype(_)
        | Filter::Supertype(_)
        | Filter::Color(_)
        | Filter::Controller(_)
        | Filter::Token => true,
        Filter::And(a, b) => scope_filter_ok(a) && scope_filter_ok(b),
        Filter::Or(v) => v.iter().all(scope_filter_ok),
        Filter::Not(a) => scope_filter_ok(a),
        _ => false,
    }
}

pub(crate) fn lower_static(l: &mut Lowerer, s: &cats::StaticAbility) -> Option<StaticDef> {
    let cats::StaticAbility::StaticAbility(nodes::StaticAbility {
        scope,
        granted_keywords,
        power_bonus,
        toughness_bonus,
        tier_threshold,
        room_half,
        grants_no_max_hand_size,
        grants_creature_spells_uncounterable,
        grants_spells_uncounterable,
        grants_ward_life_cost,
        must_attack_each_combat,
        opponents_enter_tapped_types,
        activation_cost_reduction,
        graveyard_permanent_cast_mv_max,
        granted_becomes_target_effect,
        granted_trigger_limit_per_turn,
        condition,
        replacement_effect,
        continuous_effect,
        bonus_per_condition_count,
        skips_your_draw_step,
    }) = s;
    l.deny("StaticAbility.tier_threshold", tier_threshold);
    l.deny("StaticAbility.room_half", room_half);
    l.deny("StaticAbility.grants_no_max_hand_size", grants_no_max_hand_size);
    l.deny("StaticAbility.grants_creature_spells_uncounterable", grants_creature_spells_uncounterable);
    l.deny("StaticAbility.grants_spells_uncounterable", grants_spells_uncounterable);
    l.deny("StaticAbility.grants_ward_life_cost", grants_ward_life_cost);
    l.deny("StaticAbility.activation_cost_reduction", activation_cost_reduction);
    l.deny_ne(
        "StaticAbility.graveyard_permanent_cast_mv_max",
        graveyard_permanent_cast_mv_max,
        &generated::defaults::d_StaticAbility_graveyard_permanent_cast_mv_max(),
    );
    l.deny("StaticAbility.granted_becomes_target_effect", granted_becomes_target_effect);
    l.deny("StaticAbility.granted_trigger_limit_per_turn", granted_trigger_limit_per_turn);
    l.deny("StaticAbility.replacement_effect", replacement_effect);
    l.deny("StaticAbility.bonus_per_condition_count", bonus_per_condition_count);
    l.deny("StaticAbility.skips_your_draw_step", skips_your_draw_step);

    let scope = lower_scope(l, scope)?;
    let cond = match condition.as_deref() {
        Some(c) => Some(lower_condition(l, c)?),
        None => None,
    };
    let mut mods = Vec::new();
    if *power_bonus != 0 || *toughness_bonus != 0 {
        mods.push(Mod::Pt(
            l.int("StaticAbility.power_bonus", *power_bonus, -99, 99),
            l.int("StaticAbility.toughness_bonus", *toughness_bonus, -99, 99),
        ));
    }
    mods.extend(super::effect_ext::keyword_mods(l, granted_keywords));
    if *must_attack_each_combat {
        mods.push(Mod::MustAttack);
    }
    if let Some(c) = continuous_effect.as_deref() {
        let cats::ContinuousModifier::ContinuousModifier(nodes::ContinuousModifier { layer, payload, subject_filter }) = c;
        l.deny("ContinuousModifier.subject_filter", subject_filter);
        use cats::LayerPayload as P;
        match (&**payload, layer) {
            (P::CantBeBlocked(nodes::CantBeBlocked {}), 6) => mods.push(Mod::CantBeBlocked),
            (P::GrantKeyword(nodes::GrantKeyword { keyword }), 6) => {
                mods.extend(super::effect_ext::keyword_mods(l, std::slice::from_ref(&**keyword)))
            }
            (P::PowerToughnessBonus(nodes::PowerToughnessBonus { power_bonus, toughness_bonus }), 7) => {
                mods.push(Mod::Pt(
                    l.int("PowerToughnessBonus.power_bonus", *power_bonus, -99, 99),
                    l.int("PowerToughnessBonus.toughness_bonus", *toughness_bonus, -99, 99),
                ))
            }
            (other, n) => l.unsupported(format!("LayerPayload:{}(layer {n})", other.tag())),
        }
    }
    // "<Types> your opponents control enter tapped" (a rule, on the card).
    if !opponents_enter_tapped_types.is_empty() {
        if !matches!(scope, Scope::This) || cond.is_some() {
            l.unsupported("StaticAbility.opponents_enter_tapped_types(scoped)");
        }
        for t in opponents_enter_tapped_types {
            match types::from_name(t) {
                Some(b) => l.opp_enter_tapped |= b,
                None => l.unsupported(format!("StaticAbility.opponents_enter_tapped_types({t})")),
            }
        }
        if mods.is_empty() {
            return None;
        }
    }
    if mods.is_empty() {
        l.unsupported("StaticAbility(no modification)");
        return None;
    }
    Some(StaticDef { scope, cond, mods })
}

fn lower_scope(l: &mut Lowerer, s: &cats::StaticScope) -> Option<Scope> {
    let cats::StaticScope::StaticScope(nodes::StaticScope { kind, subtype, other, card_type }) = s;
    use enums::ScopeKind as K;
    let you = |f: Filter| Filter::And(Box::new(f), Box::new(Filter::Controller(true)));
    let (filter, other) = match kind {
        K::Self_ => {
            l.deny("StaticScope.subtype", subtype);
            l.deny("StaticScope.other", other);
            l.deny("StaticScope.card_type", card_type);
            return Some(Scope::This);
        }
        K::CreaturesYouControl => (you(Filter::Type(types::CREATURE)), *other),
        K::OtherCreaturesYouControl => (you(Filter::Type(types::CREATURE)), true),
        K::SubtypeYouControl if !subtype.is_empty() => (you(Filter::Subtype(l.subtype(subtype))), *other),
        K::CardTypeYouControl => match types::from_name(card_type) {
            Some(t) => (you(Filter::Type(t)), *other),
            None => {
                l.unsupported(format!("StaticScope.card_type({card_type})"));
                return None;
            }
        },
        k => {
            l.unsupported(format!("StaticScope.kind({k:?})"));
            return None;
        }
    };
    if !matches!(kind, K::SubtypeYouControl) {
        l.deny("StaticScope.subtype", subtype);
    }
    if !matches!(kind, K::CardTypeYouControl) {
        l.deny("StaticScope.card_type", card_type);
    }
    debug_assert!(scope_filter_ok(&filter));
    Some(Scope::Matching { filter, other })
}

pub(crate) fn lower_pt_formula(l: &mut Lowerer, f: &cats::PTFormula) -> Option<PtCda> {
    use cats::PTFormula as F;
    Some(match f {
        // Level up (CR 711): base P/T by level counters, per range.
        F::LevelerPT(nodes::LevelerPT { ranges }) => {
            let counter = super::effect_ext::counter_by_name(l, "level");
            let mut out = Vec::new();
            for r in ranges {
                match r[..] {
                    [Some(lo), hi, Some(p), Some(t)] => out.push((
                        l.int("LevelerPT.lo", lo, 0, 99) as u8,
                        hi.map(|h| l.int("LevelerPT.hi", h, 0, 99) as u8),
                        l.int("LevelerPT.p", p, -99, 99),
                        l.int("LevelerPT.t", t, -99, 99),
                    )),
                    _ => {
                        l.unsupported("LevelerPT.ranges(shape)");
                        return None;
                    }
                }
            }
            PtCda::Leveler { counter, ranges: out }
        }
        F::CardTypesInAllGraveyardsPT(nodes::CardTypesInAllGraveyardsPT { toughness_plus }) => {
            PtCda::GraveyardTypes { plus: l.int("CardTypesInAllGraveyardsPT.toughness_plus", *toughness_plus, 0, 9) }
        }
        F::DomainPT(nodes::DomainPT {}) => PtCda::Domain,
        F::PowerEqualsCount(nodes::PowerEqualsCount { count_filter }) => {
            let Some(p) = count_filter else {
                l.unsupported("PowerEqualsCount.count_filter(missing)");
                return None;
            };
            let f = lower_pred(l, p)?;
            if !scope_filter_ok(&f) {
                l.unsupported("PowerEqualsCount.count_filter(not a static scope)");
                return None;
            }
            PtCda::PowerCount(f)
        }
    })
}
