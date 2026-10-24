//! Effects of the single-card tail, part 2 (M7c, ENGINE.md 29). Same rules
//! as `effect.rs`: every field named, the rest denied.

use super::Lowerer;
use crate::db::{types, Filter, Mod};
use crate::program::{Amount, Builder, Ext, Op, Who};
use card_ir::{cats, enums, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// A token effect inside another effect: "create a <token>" (one,
/// untapped) -> its token index.
fn one_token(l: &mut Lowerer, e: &cats::Effect, what: &str) -> Option<u16> {
    match e {
        cats::Effect::CreateToken(nodes::CreateToken { token_printing, count, tapped, each_player, attacking, schedule_end_of_combat_sacrifice }) => {
            if *count != 1 || *tapped || *each_player || *attacking || *schedule_end_of_combat_sacrifice {
                l.unsupported(format!("{what}(not one plain token)"));
                return None;
            }
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported(format!("{what}(not a token)"));
            }
            Some(l.token(def))
        }
        other => {
            l.unsupported(format!("{what}({})", other.tag()));
            None
        }
    }
}

/// Lower an effect class of the tail, part 2; `false` if the class is not
/// one of these.
pub(crate) fn lower_effect_tail2(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Each opponent sacrifices three creatures of their choice" (CR
        // 701.21a: chosen, then sacrificed at once).
        E::EachOpponentSacrificesN(nodes::EachOpponentSacrificesN { amount, card_type }) => {
            let n = l.int("EachOpponentSacrificesN.amount", *amount, 1, 20) as u8;
            let Some(bits) = types::from_name(card_type) else {
                l.unsupported(format!("EachOpponentSacrificesN.card_type({card_type})"));
                return true;
            };
            b.op(Op::Ext(Ext::SacrificeN { who: Who::Opponent, filter: Filter::Type(bits), n }));
        }
        // "Put target creature card from a graveyard onto the battlefield
        // under your control. It's a Phyrexian in addition to its other
        // types." (Portal to Phyrexia; the type as it enters.)
        E::ReanimateTargetCreatureCardFromAGraveyard(nodes::ReanimateTargetCreatureCardFromAGraveyard { add_subtype }) => {
            let t = b.target(Filter::InGraveyard(Box::new(Filter::Type(types::CREATURE))), 1, 1);
            let mods = if add_subtype.is_empty() {
                Vec::new()
            } else {
                vec![Mod::Types { add: 0, subtypes: vec![l.subtype(add_subtype)], replace: false }]
            };
            b.op(Op::Ext(Ext::FromGraveyardAs { tgt: t, mods }));
        }
        // "Harness <this>" (once harnessed, its infinity ability is active).
        E::HarnessSelf(nodes::HarnessSelf {}) => {
            b.op(Op::Ext(Ext::Harness));
        }
        // "Exile target Equipment, instant, or sorcery card from your
        // graveyard. Until the end of your next turn, you may play that
        // card." (Thor, God of Thunder.)
        E::ExileTargetGraveyardCardWithPlayPermission(nodes::ExileTargetGraveyardCardWithPlayPermission { card_filter, duration }) => {
            if *duration != enums::ExilePlayDuration::UntilEndOfYourNextTurn {
                l.unsupported(format!("ExileTargetGraveyardCardWithPlayPermission.duration({duration:?})"));
                return true;
            }
            let f = match card_filter.as_deref() {
                None => Filter::Any,
                Some(cats::CardTypeOrSubtypeFilter::CardTypeOrSubtypeFilter(nodes::CardTypeOrSubtypeFilter { types: tys, subtypes })) => {
                    let mut any: Vec<Filter> = Vec::new();
                    let mut bits = 0u16;
                    for t in tys {
                        match types::from_name(t) {
                            Some(x) => bits |= x,
                            None => l.unsupported(format!("CardTypeOrSubtypeFilter.types({t})")),
                        }
                    }
                    if bits != 0 {
                        any.push(Filter::Type(bits));
                    }
                    any.extend(subtypes.iter().map(|s| Filter::Subtype(l.subtype(s))));
                    match any.len() {
                        0 => Filter::Any,
                        1 => any.pop().unwrap(),
                        _ => Filter::Or(any),
                    }
                }
            };
            let t = b.target(Filter::InGraveyard(Box::new(and(f, Filter::Owner(true)))), 1, 1);
            b.op(Op::Ext(Ext::ExilePlayUntilNextTurn { tgt: t }));
        }
        // "Create a number of 1/1 white Rabbit creature tokens equal to
        // this creature's power" (its power as the ability resolves, or as
        // it last existed, CR 608.2h).
        E::CreateTokensEqualToSourcePower(nodes::CreateTokensEqualToSourcePower { token_printing }) => {
            let Some(tp) = token_printing.as_deref() else {
                l.unsupported("CreateTokensEqualToSourcePower.token_printing(missing)");
                return true;
            };
            let cats::CardPrinting::CardPrinting(tp) = tp;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateTokensEqualToSourcePower.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::SourcePower, tapped: false, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "When this dies, if it was a creature, return it to the
        // battlefield under its owner's control. It's an enchantment. (It's
        // not a creature.)" (The Enduring cycle; the types as it enters:
        // its creature types go with the creature type, CR 205.3d.)
        E::EnduringDiesReturnAsEnchantmentEffect(nodes::EnduringDiesReturnAsEnchantmentEffect {}) => {
            b.op(Op::Ext(Ext::ReturnEventCardAs { mods: vec![Mod::BecomeTypes { types: types::ENCHANTMENT, subtypes: Vec::new() }] }));
        }
        // "Whenever you discard a card, you may exile that card from your
        // graveyard" (linked to this, CR 607.2a: Currency Converter).
        E::MayExileThatCardFromYourGraveyard(nodes::MayExileThatCardFromYourGraveyard { optional }) => {
            b.op(Op::Ext(Ext::ExileEventCardLinked { optional: *optional }));
        }
        // "Put a card exiled with this artifact into its owner's graveyard.
        // If it's a land card, create <token>. If it's a nonland card,
        // create <token>."
        E::ReturnExiledWithSourceToGraveyardThenToken(nodes::ReturnExiledWithSourceToGraveyardThenToken { land_effect, nonland_effect }) => {
            let land = one_token(l, land_effect, "ReturnExiledWithSourceToGraveyardThenToken.land_effect");
            let nonland = one_token(l, nonland_effect, "ReturnExiledWithSourceToGraveyardThenToken.nonland_effect");
            let (Some(land), Some(nonland)) = (land, nonland) else { return true };
            b.op(Op::Ext(Ext::LinkedToGraveyardThenToken { land, nonland }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        _ => return false,
    }
    true
}
