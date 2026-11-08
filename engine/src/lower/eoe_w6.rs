//! Edge of Eternities, W6 (ENGINE.md 99.16): the lowering of the set's
//! remaining cards. Same rules as `effect.rs`: every field named, the rest
//! denied; each lowers onto ops named after rules actions.

use super::Lowerer;
use crate::eoe_w6::W6;
use crate::program::{Builder, Ext, Op, SpellDest};
use card_ir::cats;

/// "Counter target spell. If the amount of mana spent to cast that spell
/// was less than its mana value, <effect>" (Unravel; CR 601.2h, 202.3): the
/// condition is read just before the counter (the spell's last known
/// information on the stack is the same, 608.2h), the effect follows the
/// counter either way it went (a spell that can't be countered is still
/// "that spell").
pub(crate) fn counter_then_if_spent_below_mv(l: &mut Lowerer, b: &mut Builder, tgt: u8, dest: SpellDest, effect: &cats::Effect) {
    let check = b.op(Op::Ext(Ext::EoeW6(W6::JumpUnlessSpentBelowMv { tgt, to: 0 })));
    b.op(Op::Counter { tgt, dest });
    super::effect::lower_effect(l, b, effect);
    let end = b.op(Op::Jump { to: 0 });
    b.patch(check);
    b.op(Op::Counter { tgt, dest });
    b.patch(end);
}

/// A replacement effect of a static ability (CR 614.1a) that W6 lowers into
/// `repl`; `false` if it is not one of these (the caller goes on).
pub(crate) fn lower_replacement_w6(
    l: &mut Lowerer,
    event: &card_ir::enums::Event,
    transform: &cats::EventTransform,
    subject_filters: &[(String, Box<cats::ObjectPredicate>)],
    affects: &card_ir::enums::AffectedPlayers,
    condition: Option<&cats::Condition>,
    repl: &mut super::statics::Repl,
) -> bool {
    use card_ir::enums;
    match (event, transform) {
        // "If one or more counters would be put on a creature, Spacecraft,
        // or Planet you control, twice that many of each of those kinds of
        // counters are put on it instead" (Loading Zone; CR 614.1a, 122.6):
        // every kind of counter; "you control" is `affects` (the recipient's
        // controller).
        (enums::Event::WouldPlaceCounter, cats::EventTransform::MultiplyCount(card_ir::nodes::MultiplyCount { factor })) => {
            if *factor != 2 {
                l.unsupported(format!("ReplacementEffect(WouldPlaceCounter, MultiplyCount {factor})"));
            }
            if condition.is_some() || *affects != enums::AffectedPlayers::You {
                l.unsupported("ReplacementEffect(WouldPlaceCounter, conditional or not yours)");
            }
            match subject_filters {
                [(s, p)] if s == "obj" => {
                    if let Some(f) = super::filter::lower_pred(l, p) {
                        repl.w6_counter_doubler = Some(f);
                    }
                }
                _ => l.unsupported("ReplacementEffect(WouldPlaceCounter, recipients other than one object filter)"),
            }
            true
        }
        _ => false,
    }
}
