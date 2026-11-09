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
        | Filter::Token
        | Filter::HasCounter(_)
        | Filter::Tapped => true,
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
    /// "Creature spells you control can't be countered" (EoE W3).
    pub creature_spells_uncounterable: bool,
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
    /// "If an opponent would draw a card except the first one they draw in
    /// each of their draw steps, instead you create <token>" (Hullbreacher).
    pub opp_draw_token: Option<u16>,
    /// "As long as you have n or fewer cards in hand, if you would draw one
    /// or more cards, you draw that many cards plus d instead" (Quantum
    /// Riddler): (d, n).
    pub draw_plus: Option<(u8, u8)>,
    /// "Creatures you control have 'Whenever this creature becomes the
    /// target of a spell or ability, <effect>. This ability triggers only n
    /// times each turn'" (Nadu, Winged Wisdom): the granted triggered
    /// ability is this card's (as granted ward's, ENGINE.md 27.3).
    pub grants_target_trigger: Option<(crate::db::AbilityDef, u8)>,
    /// "If one or more counters would be put on a <filter> you control,
    /// twice that many ... instead" (EoE W6: `W6Def::counter_doubler`).
    pub w6_counter_doubler: Option<Filter>,
    /// "If a permanent entering the battlefield causes a triggered ability
    /// of a permanent you control to trigger, that ability triggers an
    /// additional time" (EoE W6: `W6Def::trigger_doubler`).
    pub w6_trigger_doubler: bool,
}

/// A static ability's replacement effect -> `repl`.
fn lower_replacement(l: &mut Lowerer, r: &cats::ReplacementEffect, repl: &mut Repl, condition: Option<&cats::Condition>) {
    let cats::ReplacementEffect::ReplacementEffect(nodes::ReplacementEffect { event_pattern, transform, scope_filter, affects }) = r;
    let cats::EventPattern::EventPattern(nodes::EventPattern { event, subject_filters }) = &**event_pattern;
    l.deny("ReplacementEffect.scope_filter", scope_filter);
    // "If an opponent would draw a card except the first one they draw in
    // each of their draw steps, instead you create a Treasure token"
    // (Hullbreacher; CR 614.1a, 614.11).
    if let (enums::Event::WouldDrawACard, cats::EventTransform::InsteadForSourceController(nodes::InsteadForSourceController { effect })) =
        (event, &**transform)
    {
        let first_draw_excepted = matches!(
            subject_filters.as_slice(),
            [(s, p)] if s == "player"
                && super::filter::lower_pred(l, p) == Some(Filter::Not(Box::new(Filter::FirstDrawInDrawStep)))
        );
        if !first_draw_excepted || *affects != enums::AffectedPlayers::Opponents || condition.is_some() {
            l.unsupported("ReplacementEffect(WouldDrawACard, not an opponent's draw but the first in their draw step)");
            return;
        }
        match &**effect {
            cats::Effect::CreateToken(nodes::CreateToken { token_printing, count, tapped, each_player, attacking, schedule_end_of_combat_sacrifice })
                if *count == 1 && !*tapped && !*each_player && !*attacking && !*schedule_end_of_combat_sacrifice =>
            {
                let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
                let def = super::printing::lower_printing(l, tp);
                repl.opp_draw_token = Some(l.token(def));
            }
            other => l.unsupported(format!("InsteadForSourceController.effect({})", other.tag())),
        }
        return;
    }
    if super::eoe_w6::lower_replacement_w6(l, event, transform, subject_filters, affects, condition, repl) {
        return;
    }
    l.deny("EventPattern.subject_filters", subject_filters);
    if *affects != enums::AffectedPlayers::You {
        l.unsupported(format!("ReplacementEffect.affects({affects:?})"));
    }
    // "As long as you have one or fewer cards in hand, if you would draw one
    // or more cards, you draw that many cards plus one instead" (Quantum
    // Riddler; CR 614.1a, 614.11).
    if let (enums::Event::WouldDrawCards, cats::EventTransform::AddToCount(nodes::AddToCount { delta })) = (event, &**transform) {
        let d = l.int("AddToCount.delta", *delta, 1, 3) as u8;
        match condition {
            Some(cats::Condition::HandSizeAtMost(nodes::HandSizeAtMost { n })) => {
                repl.draw_plus = Some((d, l.int("HandSizeAtMost.n", *n, 0, 20) as u8));
            }
            _ => l.unsupported("ReplacementEffect(WouldDrawCards, without \"as long as you have n or fewer cards in hand\")"),
        }
        return;
    }
    if condition.is_some() {
        l.unsupported("StaticAbility.replacement_effect(conditional)");
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
    l.deny("StaticAbility.skips_your_draw_step", skips_your_draw_step);

    let scope = lower_scope(l, scope)?;
    // A granted "whenever this creature becomes the target of a spell or
    // ability" triggered ability (Nadu; M7d, ENGINE.md 31).
    if let Some(e) = granted_becomes_target_effect.as_deref() {
        let creatures_yours = Scope::Matching {
            filter: Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Controller(true))),
            other: false,
        };
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
        if scope != creatures_yours || other || repl.grants_target_trigger.is_some() {
            l.unsupported("StaticAbility.granted_becomes_target_effect(not creatures you control alone)");
        }
        let n = l.int("StaticAbility.granted_trigger_limit_per_turn", *granted_trigger_limit_per_turn, 1, 9) as u8;
        let ab = super::effect::lower_ability(l, e);
        repl.grants_target_trigger = Some((ab, n));
        return None;
    }
    l.deny("StaticAbility.granted_trigger_limit_per_turn", granted_trigger_limit_per_turn);
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
    // "Creature spells you control can't be countered" (a rule for its
    // controller's creature spells, CR 101.2; EoE W3).
    if *grants_creature_spells_uncounterable {
        if !matches!(scope, Scope::This) || mods_besides(true) || *grants_ward_life_cost != 0 || *grants_spells_uncounterable {
            l.unsupported("StaticAbility.grants_creature_spells_uncounterable(with more)");
        }
        repl.creature_spells_uncounterable = true;
        return None;
    }
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
        if !matches!(scope, Scope::This) {
            l.unsupported("StaticAbility.replacement_effect(scoped)");
        }
        lower_replacement(l, r, repl, condition.as_deref());
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
            // "This creature can attack as though it didn't have defender"
            // (CR 702.3b; Mechan Shieldmate, ENGINE.md 99.11).
            (P::CanAttackAsThoughNoDefender(nodes::CanAttackAsThoughNoDefender {}), 6) => mods.push(Mod::AttackDespiteDefender),
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
        // "Each creature you control with a +1/+1 counter on it" (ENGINE.md
        // 99.2; the scope's own counter kind).
        K::CreaturesYouControlWithCounter => (
            Filter::And(Box::new(you(Filter::Type(types::CREATURE))), Box::new(Filter::HasCounter(crate::db::Counter::P1P1))),
            *other,
        ),
        K::SubtypeYouControl if !subtype.is_empty() => (you(Filter::Subtype(l.subtype(subtype))), *other),
        // "Other tapped legendary creatures you control" (The Seriema;
        // ENGINE.md 99.8): being tapped is no characteristic, nothing
        // recurses.
        K::OtherTappedLegendaryCreaturesYouControl => (
            Filter::And(
                Box::new(you(Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Tapped)))),
                Box::new(Filter::Supertype(crate::db::supertypes::LEGENDARY)),
            ),
            true,
        ),
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
        // "Power equal to the number of cards you own in exile and
        // toughness equal to that number plus 1" (ENGINE.md 99.4).
        F::CardsYouOwnInExilePT(nodes::CardsYouOwnInExilePT { toughness_plus }) => {
            PtCda::OwnedInExile { plus: l.int("CardsYouOwnInExilePT.toughness_plus", *toughness_plus, 0, 9) }
        }
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
        // A formula no supported card uses yet (the EoE port adds them): the
        // card stays blocked, named by the formula's class.
        other => return super::eoe_b::lower_pt_formula_eoe_b(l, other),
    })
}
