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

/// Replacement effects of static abilities (CR 614.1a) the card gets.
#[derive(Default, Debug)]
pub(crate) struct Repl {
    /// "If one or more tokens would be created under your control, n times
    /// that many of those tokens are created instead."
    pub token_factor: u8,
    /// "If you would draw a card while your library has no cards in it,
    /// you win the game instead."
    pub wins_on_empty_draw: bool,
    /// "You have no maximum hand size" (a rule of the game for you, CR
    /// 402.2).
    pub no_max_hand: bool,
    /// "Spells you control can't be countered."
    pub spells_uncounterable: bool,
    /// "Other creatures you control have ward—<cost>" (CR 702.21).
    pub grants_ward: Option<crate::db::WardCost>,
    /// "Once during each of your turns, you may cast a permanent spell with
    /// mana value n or less from your graveyard" (Lurrus).
    pub gy_cast_mv: u8,
    /// "Abilities you activate that aren't mana abilities cost {n} less to
    /// activate" (Zirda).
    pub ability_discount: u8,
    /// "If this would be put into a graveyard from anywhere, ... shuffle it
    /// into its owner's library instead" (a self-scoped replacement).
    pub gy_to_library: bool,
}

/// A static ability's replacement effect -> `repl`.
fn lower_replacement(l: &mut Lowerer, r: &cats::ReplacementEffect, repl: &mut Repl) {
    let cats::ReplacementEffect::ReplacementEffect(nodes::ReplacementEffect { event_pattern, transform, scope_filter, affects }) = r;
    let cats::EventPattern::EventPattern(nodes::EventPattern { event, subject_filters }) = &**event_pattern;
    l.deny("ReplacementEffect.scope_filter", scope_filter);
    l.deny("EventPattern.subject_filters", subject_filters);
    if *affects != enums::AffectedPlayers::You {
        l.unsupported(format!("ReplacementEffect.affects({affects:?})"));
    }
    match (event, &**transform) {
        (enums::Event::WouldCreateToken, cats::EventTransform::MultiplyCount(nodes::MultiplyCount { factor })) => {
            repl.token_factor = l.int("MultiplyCount.factor", *factor, 2, 4) as u8;
        }
        (enums::Event::WouldDrawFromEmptyLibrary, cats::EventTransform::WinTheGameInstead(nodes::WinTheGameInstead {})) => {
            repl.wins_on_empty_draw = true;
        }
        // "If <this> would be put into a graveyard from anywhere, reveal it
        // and shuffle it into its owner's library instead" (Blightsteel;
        // the scope, self, is checked by the caller).
        (enums::Event::WouldGoToGraveyard, cats::EventTransform::Replace(nodes::Replace { new_destination })) => {
            if *new_destination != enums::ZoneName::Library {
                l.unsupported(format!("Replace.new_destination({new_destination:?})"));
            }
            repl.gy_to_library = true;
        }
        (e, t) => l.unsupported(format!("ReplacementEffect({e:?}, {})", t.tag())),
    }
}

pub(crate) fn lower_static(l: &mut Lowerer, s: &cats::StaticAbility, repl: &mut Repl) -> Option<StaticDef> {
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
    l.deny("StaticAbility.grants_creature_spells_uncounterable", grants_creature_spells_uncounterable);
    l.deny("StaticAbility.granted_becomes_target_effect", granted_becomes_target_effect);
    l.deny("StaticAbility.granted_trigger_limit_per_turn", granted_trigger_limit_per_turn);
    l.deny("StaticAbility.skips_your_draw_step", skips_your_draw_step);

    let scope = lower_scope(l, scope)?;
    // Rules for the controller alone (ENGINE.md 29): a cost reduction of
    // abilities, a graveyard permission. Nothing else in the static.
    let alone = |l: &mut Lowerer, what: &str| {
        let other = *power_bonus != 0
            || *toughness_bonus != 0
            || !granted_keywords.is_empty()
            || *must_attack_each_combat
            || continuous_effect.is_some()
            || replacement_effect.is_some()
            || !opponents_enter_tapped_types.is_empty()
            || *bonus_per_condition_count
            || *grants_no_max_hand_size
            || *grants_spells_uncounterable
            || *grants_ward_life_cost != 0
            || condition.is_some();
        if !matches!(scope, Scope::This) || other {
            l.unsupported(format!("StaticAbility.{what}(with more)"));
        }
    };
    // "Abilities you activate that aren't mana abilities cost {2} less to
    // activate. This effect can't reduce the mana in that cost to less than
    // one mana." (Zirda; CR 602.2b, 601.2f.)
    if *activation_cost_reduction != 0 {
        alone(l, "activation_cost_reduction");
        if *graveyard_permanent_cast_mv_max != generated::defaults::d_StaticAbility_graveyard_permanent_cast_mv_max() {
            l.unsupported("StaticAbility.activation_cost_reduction(with a graveyard permission)");
        }
        repl.ability_discount = l.int("StaticAbility.activation_cost_reduction", *activation_cost_reduction, 1, 9) as u8;
        return None;
    }
    // "Once during each of your turns, you may cast a permanent spell with
    // mana value 2 or less from your graveyard." (Lurrus; CR 601.2a.)
    if *graveyard_permanent_cast_mv_max != generated::defaults::d_StaticAbility_graveyard_permanent_cast_mv_max() {
        alone(l, "graveyard_permanent_cast_mv_max");
        repl.gy_cast_mv = l.int("StaticAbility.graveyard_permanent_cast_mv_max", *graveyard_permanent_cast_mv_max, 1, 20) as u8;
        return None;
    }
    let mods_besides = |cond: bool| {
        *power_bonus != 0
            || *toughness_bonus != 0
            || !granted_keywords.is_empty()
            || *must_attack_each_combat
            || continuous_effect.is_some()
            || replacement_effect.is_some()
            || !opponents_enter_tapped_types.is_empty()
            || *bonus_per_condition_count
            || *grants_no_max_hand_size
            || (cond && condition.is_some())
    };
    // "Spells you control can't be countered" (a rule for its controller's
    // spells, CR 101.2).
    if *grants_spells_uncounterable {
        if !matches!(scope, Scope::This) || mods_besides(true) || *grants_ward_life_cost != 0 {
            l.unsupported("StaticAbility.grants_spells_uncounterable(with more)");
        }
        repl.spells_uncounterable = true;
        return None;
    }
    // "Other creatures you control have ward—pay n life" (CR 702.21): the
    // granted triggered ability is this card's (ENGINE.md 27.3).
    if *grants_ward_life_cost != 0 {
        let other_creatures = Scope::Matching {
            filter: Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Controller(true))),
            other: true,
        };
        if scope != other_creatures || mods_besides(true) || repl.grants_ward.is_some() {
            l.unsupported("StaticAbility.grants_ward_life_cost(not \"other creatures you control\" alone)");
        }
        repl.grants_ward = Some(crate::db::WardCost::Life(l.int("StaticAbility.grants_ward_life_cost", *grants_ward_life_cost, 1, 20) as u8));
        return None;
    }
    // "You have no maximum hand size" (CR 402.2): a rule for its controller.
    if *grants_no_max_hand_size {
        let other = *power_bonus != 0
            || *toughness_bonus != 0
            || !granted_keywords.is_empty()
            || *must_attack_each_combat
            || continuous_effect.is_some()
            || replacement_effect.is_some()
            || !opponents_enter_tapped_types.is_empty()
            || *bonus_per_condition_count;
        if !matches!(scope, Scope::This) || condition.is_some() || other {
            l.unsupported("StaticAbility.grants_no_max_hand_size(with more)");
        }
        repl.no_max_hand = true;
        return None;
    }
    // A replacement effect (CR 614.1a) of the card itself: no condition, no
    // modification besides it.
    if let Some(r) = replacement_effect.as_deref() {
        if !matches!(scope, Scope::This) || condition.is_some() {
            l.unsupported("StaticAbility.replacement_effect(scoped or conditional)");
        }
        lower_replacement(l, r, repl);
        let other = *power_bonus != 0
            || *toughness_bonus != 0
            || !granted_keywords.is_empty()
            || *must_attack_each_combat
            || continuous_effect.is_some()
            || !opponents_enter_tapped_types.is_empty()
            || *bonus_per_condition_count;
        if other {
            l.unsupported("StaticAbility.replacement_effect(with modifications)");
        }
        return None;
    }
    // "This gets +p/+t for each <permanent> you control" (the condition is
    // the count).
    if *bonus_per_condition_count {
        let f = match condition.as_deref() {
            Some(cats::Condition::PerControlled(nodes::PerControlled { filter })) => lower_pred(l, filter),
            Some(other) => {
                l.unsupported(format!("StaticAbility.bonus_per_condition_count({})", other.tag()));
                None
            }
            None => {
                l.unsupported("StaticAbility.bonus_per_condition_count(no condition)");
                None
            }
        };
        let f = f?;
        if !scope_filter_ok(&f) {
            l.unsupported("StaticAbility.bonus_per_condition_count(not a static scope filter)");
            return None;
        }
        let filter = Filter::And(Box::new(f), Box::new(Filter::Controller(true)));
        let (p, t) = (
            l.int("StaticAbility.power_bonus", *power_bonus, -9, 9),
            l.int("StaticAbility.toughness_bonus", *toughness_bonus, -9, 9),
        );
        let other = !granted_keywords.is_empty()
            || *must_attack_each_combat
            || continuous_effect.is_some()
            || !opponents_enter_tapped_types.is_empty();
        if other || (p == 0 && t == 0) {
            l.unsupported("StaticAbility.bonus_per_condition_count(with other modifications)");
        }
        return Some(StaticDef { scope, cond: None, mods: vec![Mod::PtPer { filter, p, t }] });
    }
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
