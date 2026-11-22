//! The Legacy Hogaak Tempo deck's cards, lane C (ENGINE.md 220.3): the
//! lowering of Moonshadow's "remove a -1/-1 counter from this creature" and
//! the filter check of Dark Matter Manipulator's "for every seven cards in
//! your graveyard". Same rules as `effect.rs`: every field named, the rest
//! denied; each lowers onto ops named after rules actions. (Moonshadow's
//! trigger event is `lower/trigger.rs`'s `CardPutIntoGraveyard` arm, its six
//! -1/-1 counters `CardPrinting.enters_with_named_counters`; Dark Matter
//! Manipulator's static `lower/statics.rs`; Stitcher's Supplier lowers
//! through the existing `SelfMill`; Dryad Arbor is a land creature, its
//! "{T}: Add {G}" the IR's `produces`, `lower/printing.rs`.)

use super::Lowerer;
use crate::db::Filter;
use crate::program::{Amount, Builder, Ext, On, Op};
use card_ir::{cats, nodes};

/// A filter a graveyard count can apply to a card there: characteristics
/// only (types, subtypes, supertypes, colours, mana value), nothing about a
/// permanent's state or a player.
pub(crate) fn graveyard_count_filter_ok(f: &Filter) -> bool {
    match f {
        Filter::Any
        | Filter::Type(_)
        | Filter::Subtype(_)
        | Filter::Supertype(_)
        | Filter::Color(_)
        | Filter::Colorless
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtLeast(_) => true,
        Filter::And(a, b) => graveyard_count_filter_ok(a) && graveyard_count_filter_ok(b),
        Filter::Or(v) => v.iter().all(graveyard_count_filter_ok),
        Filter::Not(x) => graveyard_count_filter_ok(x),
        _ => false,
    }
}

pub(crate) fn lower_effect_hogaak_c(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Remove a -1/-1 counter from this creature" (Moonshadow; Dark
        // Depths' "remove a lore counter from this land" shape, CR 122.8):
        // from the ability's source if it is still the same object on the
        // battlefield (a new object after a zone change, CR 400.7), as many
        // as it has up to the amount.
        E::RemoveCountersFromSource(nodes::RemoveCountersFromSource { kind, amount }) => {
            if kind.is_empty() {
                l.unsupported("RemoveCountersFromSource.kind(empty)");
                return true;
            }
            let counter = super::effect_ext::counter_by_name(l, kind);
            let n = l.int("RemoveCountersFromSource.amount", *amount, 1, 99);
            b.op(Op::Ext(Ext::RemoveCounters { on: On::Source, counter, n: Amount::Fixed(n) }));
            true
        }
        _ => false,
    }
}
