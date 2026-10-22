//! Object predicates -> [`Filter`].

use super::Lowerer;
use crate::db::{types, Filter};
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
            E::IsPlayer(nodes::IsPlayer {}) => Some(Filter::Player),
            E::HasTotalPowerAndToughnessAtMost(nodes::HasTotalPowerAndToughnessAtMost { n }) => {
                Some(Filter::TotalPtAtMost($l.int("HasTotalPowerAndToughnessAtMost.n", *n, -99, 99)))
            }
            E::ObjectAnd(nodes::ObjectAnd { left, right }) => {
                let a = lower_pred_or_oneof($l, left);
                let b = lower_pred_or_oneof($l, right);
                Some(Filter::And(Box::new(a?), Box::new(b?)))
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
