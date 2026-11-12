//! Lane B of the Legacy Doomsday deck (ENGINE.md 124.2): the effect classes
//! of Misdirection (mtg-bot's `legacy_stack`). Same rules as `effect.rs`:
//! every field named, the rest denied; each lowers onto ops named after rules
//! actions.

use super::Lowerer;
use crate::db::Filter;
use crate::doomsday_b::DoomsdayB;
use crate::program::{Builder, Ext, Op};
use card_ir::{cats, enums, nodes};

/// Lower a lane-B effect class; `false` if the class is not one of these.
pub(crate) fn lower_effect_doomsday_b(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Change the target of target spell with a single target" (CR
        // 115.7a, 115.9a): the target a spell (matching the printed spell
        // phrase) with exactly one target instance; on resolution its one
        // target changes (`DoomsdayB::ChangeTheTarget`).
        E::ChangeTheTargetOfTargetWithASingleTarget(nodes::ChangeTheTargetOfTargetWithASingleTarget { kinds, spell_filter }) => {
            if kinds.as_slice() != [enums::StackObjectKind::Spell] {
                l.unsupported("ChangeTheTargetOfTargetWithASingleTarget.kinds(not a spell alone)");
                return true;
            }
            let inner = match spell_filter {
                Some(p) => match super::filter::lower_pred(l, p) {
                    Some(f) => Filter::And(Box::new(f), Box::new(Filter::SingleTarget)),
                    None => return true,
                },
                None => Filter::SingleTarget,
            };
            let tgt = b.target(Filter::Spell(Box::new(inner)), 1, 1);
            b.op(Op::Ext(Ext::DoomsdayB(DoomsdayB::ChangeTheTarget { tgt })));
            true
        }
        _ => false,
    }
}
