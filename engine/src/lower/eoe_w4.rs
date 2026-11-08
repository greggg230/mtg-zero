//! Edge of Eternities, W4 (ENGINE.md 99.14): exile, graveyard, copies and
//! control. The effect classes of W4's cards. Same rules as `effect.rs`:
//! every field named, the rest denied. A class named after a card carries
//! no fields: its meaning is the card's Oracle text (mtg-bot's docstring;
//! docs/ir_defects.md 11), lowered onto ops named after rules actions, its
//! targets derived here.

use super::Lowerer;
use crate::db::{types, Counter, Filter};
use crate::eoe_w4::EoeW4;
use crate::program::{Amount, Builder, CopyOf, Ext, Op, Who};
use card_ir::{cats, enums, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn w4(b: &mut Builder, op: EoeW4) -> usize {
    b.op(Op::Ext(Ext::EoeW4(op)))
}

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
        // "Return up to three target creature cards with total mana value 3
        // or less from your graveyard to the battlefield. Put a +1/+1 counter
        // on each of them" (Scout for Survivors; CR 601.2c: the total is a
        // restriction on the set of targets, `TargetReq::total_mv`).
        E::ReturnUpToNSmallCreaturesFromGraveyardWithCounters(nodes::ReturnUpToNSmallCreaturesFromGraveyardWithCounters {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        }) => {
            let _ = (log_suffix, target_kind); // display, Python dispatch
            if !*creature || *artifact || *land || *nonland_permanent || *or_enchantment || *or_spacecraft || *or_artifact {
                l.unsupported("ReturnUpToNSmallCreaturesFromGraveyardWithCounters(not creature cards)");
            }
            l.deny("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.untargeted_all", untargeted_all);
            if *destination != enums::GraveyardReturnDestination::Battlefield {
                l.unsupported("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.destination(not the battlefield)");
            }
            let mut f = and(Filter::Type(types::CREATURE), Filter::Owner(true));
            if *max_mv > 0 {
                f = and(f, Filter::ManaValueAtMost(l.int("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.max_mv", *max_mv, 0, 30) as u8));
            }
            let lo = l.int("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.min_n", *min_n, 0, 8) as u8;
            let hi = l.int("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.max_n", *max_n, 1, 8) as u8;
            let tgt = b.target(Filter::InGraveyard(Box::new(f)), lo, hi);
            if *max_total_mv > 0 {
                b.targets[tgt as usize].total_mv =
                    Some(l.int("ReturnUpToNSmallCreaturesFromGraveyardWithCounters.max_total_mv", *max_total_mv, 0, 30) as u8);
            }
            let counter = plus_one_counter.then_some(Counter::P1P1);
            w4(b, EoeW4::GraveyardTargetsOntoBattlefield { tgt, counter });
        }
        _ => return false,
    }
    true
}
