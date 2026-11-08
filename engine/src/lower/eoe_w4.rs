//! Edge of Eternities, W4 (ENGINE.md 99.14): exile, graveyard, copies and
//! control. The effect classes of W4's cards. Same rules as `effect.rs`:
//! every field named, the rest denied. A class named after a card carries
//! no fields: its meaning is the card's Oracle text (mtg-bot's docstring;
//! docs/ir_defects.md 11), lowered onto ops named after rules actions, its
//! targets derived here.

use super::Lowerer;
use crate::program::{Amount, Builder, CopyOf, Ext, Op, Who};
use card_ir::{cats, nodes};

/// Lower an effect class of W4; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_w4(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Draw cards equal to the sacrificed creature's power" (Susur
        // Secundi's "12+" ability: its power as it last existed on the
        // battlefield, CR 608.2h; the cost's sacrifice, 602.2b).
        E::DrawCardsEqualTo(nodes::DrawCardsEqualTo { count }) => {
            let n = match count.as_deref() {
                Some(cats::Formula::PowerOfCostPaidObject(nodes::PowerOfCostPaidObject {})) => {
                    if !l.sac_cost {
                        l.unsupported("Formula:PowerOfCostPaidObject(no sacrifice cost)");
                    }
                    Amount::CostPower
                }
                Some(other) => {
                    l.unsupported(format!("DrawCardsEqualTo.count(Formula:{})", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("DrawCardsEqualTo.count(missing)");
                    return true;
                }
            };
            b.op(Op::Draw { who: Who::You, n });
        }
        // "Create a token that's a copy of target <permanent>, except <it's
        // legendary / it has haste>" (Adagia's "12+" ability; CR 707.2,
        // 707.9a-b: the exceptions are copiable values of the token, 111.10).
        E::CreateTokenCopyOfTargetWithExceptions(nodes::CreateTokenCopyOfTargetWithExceptions {
            targets,
            exceptions,
            gains_haste,
            exile_at_next_end_step,
        }) => {
            let cats::CopyExceptions::CopyExceptions(x) = exceptions.as_ref();
            let nodes::CopyExceptions { add_types, add_subtypes, add_supertypes, base_pt, colors, add_keywords } = x;
            // (The `Never` lists can hold nothing: the schema has no value
            // for them.)
            let _: (&Vec<card_ir::Never>, &Vec<card_ir::Never>) = (add_types, add_subtypes);
            if base_pt.is_some() {
                l.unsupported("CopyExceptions.base_pt");
            }
            if colors.is_some() {
                l.unsupported("CopyExceptions.colors");
            }
            if *exile_at_next_end_step {
                l.unsupported("CreateTokenCopyOfTargetWithExceptions.exile_at_next_end_step");
            }
            let mut exc = super::effect_copies::except_kws(l, add_keywords, "CopyExceptions.add_keywords");
            if *gains_haste {
                exc.kws.insert(crate::db::Kw::Haste);
            }
            for s in add_supertypes {
                match crate::db::supertypes::from_name(s) {
                    Some(bit) => exc.supers |= bit,
                    None => l.unsupported(format!("CopyExceptions.add_supertypes({s})")),
                }
            }
            let Some(t) = super::effect::lower_targets(l, b, targets, "CreateTokenCopyOfTargetWithExceptions") else { return true };
            b.op(Op::Ext(Ext::CreateTokenCopy { of: CopyOf::Target { i: t, lki: false }, who: Who::You, n: Amount::Fixed(1), exc }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        _ => return false,
    }
    true
}
