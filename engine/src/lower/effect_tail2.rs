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
        // "This creature deals damage equal to 2 plus the sacrificed
        // permanent's mana value to any target" (Broadside Bombardiers).
        E::DealDamageByFormula(nodes::DealDamageByFormula { amount, targets }) => {
            let amount = match amount.as_deref() {
                Some(cats::Formula::MVOfCostPaidObjectPlus(nodes::MVOfCostPaidObjectPlus { constant })) => {
                    if !l.sac_cost {
                        l.unsupported("Formula:MVOfCostPaidObjectPlus(no sacrifice cost)");
                    }
                    Amount::CostMvPlus(l.int("MVOfCostPaidObjectPlus.constant", *constant, 0, 20) as u8)
                }
                Some(other) => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("DealDamageByFormula.amount(missing)");
                    return true;
                }
            };
            let Some(to) = super::effect::lower_targets(l, b, targets, "DealDamageByFormula") else { return true };
            b.op(Op::DealDamage { amount, to });
        }
        // Yawgmoth's Will.
        E::MayPlayFromGraveyardThisTurn(nodes::MayPlayFromGraveyardThisTurn {}) => {
            b.op(Op::Ext(Ext::PlayFromGraveyardThisTurn));
        }
        E::ExileInsteadOfGraveyardThisTurn(nodes::ExileInsteadOfGraveyardThisTurn {}) => {
            b.op(Op::Ext(Ext::GraveyardToExileThisTurn));
        }
        // "Search your library for a green creature card with mana value X
        // or less, put it onto the battlefield, then shuffle."
        E::SearchLibraryForGreenCreatureWithMVAtMostX(nodes::SearchLibraryForGreenCreatureWithMVAtMostX {}) => {
            let filter = and(and(Filter::Type(types::CREATURE), Filter::Color(1 << 4)), Filter::MvAtMostX);
            b.op(Op::Search { who: Who::You, filter, dest: crate::program::SearchDest::Battlefield { tapped: false }, reveal: false, may_fail: true });
        }
        // Cascade (CR 702.85a): "when you cast this spell" (the keyword and
        // its trigger, both in the IR: checked together in printing.rs).
        E::Cascade(nodes::Cascade {}) => {
            l.cascade = true;
            b.op(Op::Ext(Ext::Cascade));
        }
        // Ocelot Pride: "for each token you control that entered this
        // turn, create a token that's a copy of it".
        E::CopyEachTokenYouControlEnteredThisTurn(nodes::CopyEachTokenYouControlEnteredThisTurn {}) => {
            b.op(Op::Ext(Ext::CopyTokensEnteredThisTurn));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Mill three cards. You may put a land card from among them into
        // your hand." (Six.)
        E::MillThenMayPutLandIntoHand(nodes::MillThenMayPutLandIntoHand { n }) => {
            let n = l.int("MillThenMayPutLandIntoHand.n", *n, 1, 20) as u8;
            b.op(Op::Ext(Ext::MillThenLandToHand { n }));
        }
        // "You may cast an artifact, instant, or sorcery spell from your
        // graveyard. If an instant or sorcery spell cast this way would be
        // put into your graveyard, exile it instead." (Bilbo.)
        E::MayCastSpellFromGraveyardNow(nodes::MayCastSpellFromGraveyardNow { card_filter, exile_instants_and_sorceries }) => {
            let filter = match card_filter.as_deref() {
                Some(f) => match super::filter::lower_card_filter(l, f) {
                    Some(f) => f,
                    None => return true,
                },
                None => Filter::Any,
            };
            b.op(Op::Ext(Ext::MayCastFromGraveyardNow { filter, exile: *exile_instants_and_sorceries }));
        }
        // "After this phase, there is an additional combat phase" (CR
        // 500.8).
        E::AdditionalCombatPhaseAfterThisPhase(nodes::AdditionalCombatPhaseAfterThisPhase {}) => {
            b.op(Op::Ext(Ext::ExtraCombat));
        }
        // Airbend (Aang): "airbend up to one other target creature or
        // spell. (Exile it. While it's exiled, its owner may cast it for
        // {2} rather than its mana cost.)"
        E::Airbend(nodes::Airbend { alt_cost }) => {
            let Some(cost) = crate::mana::ManaCost::parse(alt_cost).filter(|c| c.total() == c.generic) else {
                l.unsupported(format!("Airbend.alt_cost({alt_cost})"));
                return true;
            };
            let f = Filter::Or(vec![
                and(Filter::Type(types::CREATURE), Filter::Not(Box::new(Filter::Source))),
                Filter::Spell(Box::new(Filter::Any)),
            ]);
            let t = b.target(f, 0, 1);
            b.op(Op::Ext(Ext::Airbend { tgt: t, cost }));
        }
        // "Untap X target lands" (Candelabra of Tawnos: X of its cost).
        E::UntapXTargets(nodes::UntapXTargets { targets, target_count_is_x }) => {
            if !*target_count_is_x || !l.act_x || targets.len() != 1 {
                l.unsupported("UntapXTargets(not X of an activation cost)");
                return true;
            }
            let cats::TargetSpec::TargetSpec(nodes::TargetSpec { filter, count_min, count_max, distinct_from }) = &targets[0];
            l.deny("TargetSpec.distinct_from", distinct_from);
            l.deny("TargetSpec.count_min", count_min);
            if *count_max != card_ir::Count::Infinite {
                l.unsupported("UntapXTargets.targets(count_max)");
            }
            let Some(f) = super::filter::lower_pred_or_oneof(l, filter) else { return true };
            let t = b.target(f, 0, crate::db::X_TARGETS);
            b.op(Op::Ext(Ext::Untap { on: crate::program::On::Target(t) }));
        }
        _ => return false,
    }
    true
}
