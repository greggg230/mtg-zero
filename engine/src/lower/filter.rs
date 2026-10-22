//! Object predicates -> [`Filter`]; conditions -> [`Cond`].

use super::Lowerer;
use crate::db::{supertypes, types, Cond, Filter};
use card_ir::{cats, nodes};

/// Arms shared by `ObjectPredicate` and `ObjectPredicateOrOneOf` (the
/// generated enums repeat the same node types).
macro_rules! lower_object_predicate {
    ($l:expr, $p:expr, $E:ident, { $($extra:tt)* }) => {{
        use cats::$E as E;
        match $p {
            E::IsCreature(nodes::IsCreature {}) => Some(Filter::Type(types::CREATURE)),
            E::IsPlaneswalker(nodes::IsPlaneswalker {}) => Some(Filter::Type(types::PLANESWALKER)),
            E::IsBattle(nodes::IsBattle {}) => Some(Filter::Type(types::BATTLE)),
            E::IsArtifact(nodes::IsArtifact {}) => Some(Filter::Type(types::ARTIFACT)),
            E::IsEnchantment(nodes::IsEnchantment {}) => Some(Filter::Type(types::ENCHANTMENT)),
            E::IsLand(nodes::IsLand {}) => Some(Filter::Type(types::LAND)),
            E::CubeWalkersIsPermanent(nodes::CubeWalkersIsPermanent {})
            | E::ValuePredicatesIsPermanent(nodes::ValuePredicatesIsPermanent {}) => {
                Some(Filter::Type(types::PERMANENT))
            }
            E::IsPlayer(nodes::IsPlayer {}) => Some(Filter::Player),
            E::IsBasic(nodes::IsBasic {}) => Some(Filter::Supertype(supertypes::BASIC)),
            E::IsLegendary(nodes::IsLegendary {}) => Some(Filter::Supertype(supertypes::LEGENDARY)),
            E::HasSubtype(nodes::HasSubtype { name }) => Some(Filter::Subtype($l.subtype(name))),
            E::HasTotalPowerAndToughnessAtMost(nodes::HasTotalPowerAndToughnessAtMost { n }) => {
                Some(Filter::TotalPtAtMost($l.int("HasTotalPowerAndToughnessAtMost.n", *n, -99, 99)))
            }
            E::CubeWalkersHasManaValueAtMost(nodes::CubeWalkersHasManaValueAtMost { n })
            | E::ValuePredicatesHasManaValueAtMost(nodes::ValuePredicatesHasManaValueAtMost { n }) => {
                Some(Filter::ManaValueAtMost($l.int("HasManaValueAtMost.n", *n, 0, 30) as u8))
            }
            E::HasPowerAtLeast(nodes::HasPowerAtLeast { n }) => {
                Some(Filter::PowerAtLeast($l.int("HasPowerAtLeast.n", *n, -99, 99)))
            }
            E::HasPowerAtMost(nodes::HasPowerAtMost { n }) => {
                Some(Filter::PowerAtMost($l.int("HasPowerAtMost.n", *n, -99, 99)))
            }
            E::HasToughnessAtMost(nodes::HasToughnessAtMost { n }) => {
                Some(Filter::ToughnessAtMost($l.int("HasToughnessAtMost.n", *n, -99, 99)))
            }
            E::ControlledByYou(nodes::ControlledByYou {}) => Some(Filter::Controller(true)),
            E::ControlledByOpponent(nodes::ControlledByOpponent {}) => Some(Filter::Controller(false)),
            E::IsSource(nodes::IsSource {}) => Some(Filter::Source),
            E::HasColor(nodes::HasColor { color }) => match crate::mana::letter_index(color) {
                Some(i) if i < 5 => Some(Filter::Color(1 << i)),
                _ => {
                    $l.unsupported(format!("HasColor.color({color})"));
                    None
                }
            },
            E::ObjectAnd(nodes::ObjectAnd { left, right }) => {
                let a = lower_pred_or_oneof($l, left);
                let b = lower_pred_or_oneof($l, right);
                Some(Filter::And(Box::new(a?), Box::new(b?)))
            }
            E::ObjectOr(nodes::ObjectOr { left, right }) => {
                let a = lower_pred($l, left);
                let b = lower_pred($l, right);
                Some(Filter::Or(vec![a?, b?]))
            }
            E::ObjectNot(nodes::ObjectNot { inner }) => Some(Filter::Not(Box::new(lower_pred($l, inner)?))),
            $($extra)*
            other => {
                $l.unsupported(format!("ObjectPredicate:{}", other.tag()));
                None
            }
        }
    }};
}

pub(crate) fn lower_pred(l: &mut Lowerer, p: &cats::ObjectPredicate) -> Option<Filter> {
    lower_object_predicate!(l, p, ObjectPredicate, {})
}

pub(crate) fn lower_pred_or_oneof(l: &mut Lowerer, p: &cats::ObjectPredicateOrOneOf) -> Option<Filter> {
    lower_object_predicate!(l, p, ObjectPredicateOrOneOf, {
        E::OneOf(nodes::OneOf { filters }) => {
            let fs: Vec<Option<Filter>> = filters.iter().map(|f| lower_pred_or_oneof(l, f)).collect();
            fs.into_iter().collect::<Option<Vec<_>>>().map(Filter::Or)
        }
    })
}

/// Arms shared by `Condition` and `ConditionOrStatePredicate`.
macro_rules! lower_condition {
    ($l:expr, $c:expr, $E:ident, { $($extra:tt)* }) => {{
        use cats::$E as E;
        match $c {
            E::ControlPermanent(nodes::ControlPermanent { filter }) => {
                Some(Cond::ControlAtLeast(lower_pred($l, filter)?, 1))
            }
            E::ControlsAtMost(nodes::ControlsAtMost { filter, n }) => {
                let n = $l.int("ControlsAtMost.n", *n, 0, 99) as u8;
                Some(Cond::ControlAtMost(lower_pred($l, filter)?, n))
            }
            E::HandSizeExactly(nodes::HandSizeExactly { n }) => {
                Some(Cond::HandExactly($l.int("HandSizeExactly.n", *n, 0, 99) as u8))
            }
            E::DeliriumActive(nodes::DeliriumActive { min_card_types }) => {
                Some(Cond::Delirium($l.int("DeliriumActive.min_card_types", *min_card_types, 1, 9) as u8))
            }
            E::PermanentLeftYourControlThisTurn(nodes::PermanentLeftYourControlThisTurn {}) => Some(Cond::Revolt),
            E::YourTurnOfGameAtMost(nodes::YourTurnOfGameAtMost { n }) => {
                Some(Cond::YourTurnAtMost($l.int("YourTurnOfGameAtMost.n", *n, 0, 99) as u8))
            }
            $($extra)*
            other => {
                $l.unsupported(format!("Condition:{}", other.tag()));
                None
            }
        }
    }};
}

pub(crate) fn lower_condition(l: &mut Lowerer, c: &cats::Condition) -> Option<Cond> {
    lower_condition!(l, c, Condition, {})
}

pub(crate) fn lower_condition_or_state(l: &mut Lowerer, c: &cats::ConditionOrStatePredicate) -> Option<Cond> {
    lower_condition!(l, c, ConditionOrStatePredicate, {
        E::CardsInGraveyardAtLeast(nodes::CardsInGraveyardAtLeast { n }) => {
            Some(Cond::GraveyardAtLeast(l.int("CardsInGraveyardAtLeast.n", *n, 0, 99) as u8))
        }
    })
}

/// A `CardFilter` (characteristics of a card) -> [`Filter`].
pub(crate) fn lower_card_filter(l: &mut Lowerer, f: &cats::CardFilter) -> Option<Filter> {
    let cats::CardFilter::CardFilter(n) = f;
    lower_card_filter_node(l, n)
}

/// A library search's filter: a `CardFilter` or "a card of one of these
/// types or subtypes".
pub(crate) fn lower_search_filter(l: &mut Lowerer, f: &cats::CardFilterOrCardTypeOrSubtypeFilter) -> Option<Filter> {
    match f {
        cats::CardFilterOrCardTypeOrSubtypeFilter::CardFilter(n) => lower_card_filter_node(l, n),
        cats::CardFilterOrCardTypeOrSubtypeFilter::CardTypeOrSubtypeFilter(nodes::CardTypeOrSubtypeFilter {
            types: tys,
            subtypes,
        }) => {
            let mut any: Vec<Filter> = Vec::new();
            let mut bits = 0u16;
            for t in tys {
                match card_type_bits(t) {
                    Some(b) => bits |= b,
                    None => l.unsupported(format!("CardTypeOrSubtypeFilter.types({t})")),
                }
            }
            if bits != 0 {
                any.push(Filter::Type(bits));
            }
            any.extend(subtypes.iter().map(|s| Filter::Subtype(l.subtype(s))));
            if any.is_empty() {
                l.unsupported("CardTypeOrSubtypeFilter(empty)");
                return None;
            }
            Some(if any.len() == 1 { any.pop().unwrap() } else { Filter::Or(any) })
        }
    }
}

/// A card type name, or "Permanent" (every permanent type).
fn card_type_bits(t: &str) -> Option<u16> {
    match t {
        "Permanent" => Some(types::PERMANENT),
        _ => types::from_name(t),
    }
}

pub(crate) fn lower_card_filter_node(l: &mut Lowerer, f: &nodes::CardFilter) -> Option<Filter> {
    let nodes::CardFilter { any_types, excluded_types, max_mv, color, supertype, subtype, mana_costs } = f;
    l.deny("CardFilter.mana_costs", mana_costs);
    let mut parts: Vec<Filter> = Vec::new();
    let bits = |l: &mut Lowerer, names: &[String]| {
        let mut b = 0u16;
        for t in names {
            match card_type_bits(t) {
                Some(x) => b |= x,
                None => l.unsupported(format!("CardFilter.types({t})")),
            }
        }
        b
    };
    if !any_types.is_empty() {
        parts.push(Filter::Type(bits(l, any_types)));
    }
    if !excluded_types.is_empty() {
        parts.push(Filter::Not(Box::new(Filter::Type(bits(l, excluded_types)))));
    }
    if let Some(n) = max_mv {
        parts.push(Filter::ManaValueAtMost(l.int("CardFilter.max_mv", *n, 0, 30) as u8));
    }
    if let Some(c) = color {
        match crate::mana::letter_index(c) {
            Some(i) if i < 5 => parts.push(Filter::Color(1 << i)),
            _ => l.unsupported(format!("CardFilter.color({c})")),
        }
    }
    if let Some(s) = supertype {
        match supertypes::from_name(s) {
            Some(b) => parts.push(Filter::Supertype(b)),
            None => l.unsupported(format!("CardFilter.supertype({s})")),
        }
    }
    if let Some(s) = subtype {
        parts.push(Filter::Subtype(l.subtype(s)));
    }
    let mut it = parts.into_iter();
    let first = it.next().unwrap_or(Filter::Any);
    Some(it.fold(first, |a, x| Filter::And(Box::new(a), Box::new(x))))
}
