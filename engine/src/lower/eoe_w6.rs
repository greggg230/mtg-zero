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
