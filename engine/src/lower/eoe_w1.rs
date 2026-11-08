//! Edge of Eternities, W1 (ENGINE.md 99.11): the effect classes mtg-bot's
//! parser gained when it stopped flattening the printings of
//! docs/ir_defects.md 21-101. Same rules as `effect.rs`: every field named,
//! the rest denied; each lowers onto ops named after rules actions.

use super::Lowerer;
use crate::db::{types, Filter, Mod};
use crate::eoe_w1::EoeW1;
use crate::program::{Amount, Builder, CopyOf, Ext, Op, SpellOf, Who};
use card_ir::{cats, enums, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// A `CardFilter` slot (`None`: any card).
fn opt_card_filter(l: &mut Lowerer, f: &Option<Box<cats::CardFilter>>) -> Option<Filter> {
    match f {
        Some(f) => super::filter::lower_card_filter(l, f),
        None => Some(Filter::Any),
    }
}

/// Lower a W1 effect class; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_w1(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Return one or two target creature and/or planeswalker cards from
        // your graveyard to your hand" (Archenemy's Charm); "Return up to
        // two target nonland permanent cards from your graveyard to your
        // hand, then return up to two target land cards from your graveyard
        // to the battlefield tapped" (Pull Through the Weft): one target
        // requirement per group (CR 115.1, 601.2c), the groups' moves in
        // order (608.2c).
        E::ReturnTargetCardsFromYourGraveyard(nodes::ReturnTargetCardsFromYourGraveyard { groups }) => {
            if groups.is_empty() {
                l.unsupported("ReturnTargetCardsFromYourGraveyard.groups(none)");
                return true;
            }
            let mut moves = Vec::new();
            for g in groups {
                let cats::GraveyardReturnGroup::GraveyardReturnGroup(nodes::GraveyardReturnGroup {
                    card_filter,
                    min_n,
                    max_n,
                    destination,
                    tapped,
                }) = g;
                let Some(cf) = opt_card_filter(l, card_filter) else { return true };
                let lo = l.int("GraveyardReturnGroup.min_n", *min_n, 0, 8) as u8;
                let hi = l.int("GraveyardReturnGroup.max_n", *max_n, 1, 8) as u8;
                if lo > hi {
                    l.unsupported("GraveyardReturnGroup.min_n(above max_n)");
                    return true;
                }
                let to_battlefield = *destination == enums::GraveyardReturnDestination::Battlefield;
                if *tapped && !to_battlefield {
                    l.unsupported("GraveyardReturnGroup.tapped(to a hand)");
                }
                let tgt = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), lo, hi);
                moves.push(Ext::FromGraveyard { tgt, to_battlefield, yours: true, tapped: *tapped, counter: None });
            }
            for m in moves {
                b.op(Op::Ext(m));
            }
        }
        // "Return target creature card from your graveyard to the
        // battlefield. It's a Skeleton in addition to its other types and
        // has no abilities." (Xu-Ifit): the effect of a resolved ability on
        // the permanent it becomes, for as long as it stays (CR 611.2c; layer
        // 4 and layer 6, 613.1d, 613.1f), applied as it enters: it loses its
        // "when this enters" and "enters with / as" abilities before they
        // apply (Xu-Ifit's ruling of 2025-07-25; CR 614.12).
        E::ReturnTargetCardFromYourGraveyardToBattlefieldAs(nodes::ReturnTargetCardFromYourGraveyardToBattlefieldAs {
            card_filter,
            added_subtypes,
            loses_all_abilities,
        }) => {
            let Some(cf) = opt_card_filter(l, card_filter) else { return true };
            let tgt = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), 1, 1);
            let mut mods = Vec::new();
            if !added_subtypes.is_empty() {
                let subtypes = added_subtypes.iter().map(|s| l.subtype(s)).collect();
                mods.push(Mod::Types { add: 0, subtypes, replace: false });
            }
            if *loses_all_abilities {
                mods.push(Mod::LoseAllAbilities);
            }
            if mods.is_empty() {
                l.unsupported("ReturnTargetCardFromYourGraveyardToBattlefieldAs(no change)");
            } else {
                b.op(Op::Ext(Ext::FromGraveyardAs { tgt, mods }));
            }
        }
        // "Return each creature card exiled with this land to the
        // battlefield under its owner's control" (Endless Sands; CR 607.2a).
        E::ReturnCardsExiledWithSourceToBattlefield(nodes::ReturnCardsExiledWithSourceToBattlefield { card_filter }) => {
            let Some(filter) = opt_card_filter(l, card_filter) else { return true };
            b.op(Op::Ext(Ext::EoeW1(EoeW1::LinkedToBattlefieldOwnersMatching { filter })));
        }
        // "Destroy up to one target nonland permanent" (CR 701.8a; the
        // target specification as the IR states it).
        E::DestroyUpToOneTargetNonlandPermanent(nodes::DestroyUpToOneTargetNonlandPermanent { targets }) => {
            let Some(tgt) = super::effect::lower_targets(l, b, targets, "DestroyUpToOneTargetNonlandPermanent") else { return true };
            if b.targets[tgt as usize].min != 0 {
                l.unsupported("DestroyUpToOneTargetNonlandPermanent.targets(not up to one)");
            }
            b.op(Op::Destroy { tgt });
        }
        // Mirrorpool: "Copy target instant or sorcery spell you control. You
        // may choose new targets for the copy." (CR 707.10: not cast; 707.10c)
        E::CopyTargetInstantOrSorceryYouControl(nodes::CopyTargetInstantOrSorceryYouControl {}) => {
            let f = Filter::Spell(Box::new(and(Filter::Type(types::INSTANT | types::SORCERY), Filter::Controller(true))));
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::CopySpell { of: SpellOf::Target(t), who: Who::You, n: Amount::Fixed(1), may: false, new_targets: true }));
        }
        // Mirrorpool: "Create a token that's a copy of target creature you
        // control." (CR 707.2: its copiable values.)
        E::CreateTokenCopyOfTargetCreatureYouControl(nodes::CreateTokenCopyOfTargetCreatureYouControl { additional_subtype }) => {
            if additional_subtype.is_some() {
                l.unsupported("CreateTokenCopyOfTargetCreatureYouControl.additional_subtype");
                return true;
            }
            let t = b.target(and(Filter::Type(types::CREATURE), Filter::Controller(true)), 1, 1);
            let of = CopyOf::Target { i: t, lki: false };
            b.op(Op::Ext(Ext::CreateTokenCopy { of, who: Who::You, n: Amount::Fixed(1), exc: crate::copies::Except::default() }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // Pinnacle Starcage: "exile all artifacts and creatures with mana
        // value 2 or less until this artifact leaves the battlefield" (CR
        // 610.3).
        E::ExileAllMatchingUntilSourceLeaves(nodes::ExileAllMatchingUntilSourceLeaves { match_artifact, match_creature, max_mv }) => {
            let mut bits = 0;
            if *match_artifact {
                bits |= types::ARTIFACT;
            }
            if *match_creature {
                bits |= types::CREATURE;
            }
            if bits == 0 {
                l.unsupported("ExileAllMatchingUntilSourceLeaves(no card type)");
                return true;
            }
            let mv = l.int("ExileAllMatchingUntilSourceLeaves.max_mv", *max_mv, 0, 30) as u8;
            let filter = and(Filter::Type(bits), Filter::ManaValueAtMost(mv));
            b.op(Op::Ext(Ext::EoeW1(EoeW1::ExileEachUntilSourceLeaves { filter })));
        }
        // "Put each card exiled with this artifact into its owner's
        // graveyard" (CR 607.2a), remembering how many.
        E::MoveEachExiledWithSourceToOwnersGraveyard(nodes::MoveEachExiledWithSourceToOwnersGraveyard {}) => {
            b.op(Op::Ext(Ext::EoeW1(EoeW1::LinkedToGraveyardsCounted)));
        }
        // "Then create a <token> for each card put into a graveyard this
        // way" (the count the previous op remembered).
        E::CreateNTokensFromLastExileRecall(nodes::CreateNTokensFromLastExileRecall { token_name, token_printing }) => {
            let _ = token_name; // display: the printing is the token
            let Some(tp) = token_printing else {
                l.unsupported("CreateNTokensFromLastExileRecall.token_printing(none: a token by name)");
                return true;
            };
            let cats::CardPrinting::CardPrinting(tp) = &**tp;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateNTokensFromLastExileRecall.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::Remembered, tapped: false, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // Meltstrider's Resolve: "When this Aura enters, enchanted creature
        // fights up to one target creature an opponent controls." (CR 701.14)
        E::EnchantedCreatureFightsTargetEffect(nodes::EnchantedCreatureFightsTargetEffect {}) => {
            let t = b.target(and(Filter::Type(types::CREATURE), Filter::Controller(false)), 0, 1);
            b.op(Op::Ext(Ext::EoeW1(EoeW1::HostFights { tgt: t })));
        }
        // Sothera, the Supervoid: "each opponent chooses a creature they
        // control and exiles it" (two players: the opponent), the cards
        // exiled with Sothera (`link_to_source`, CR 607.2a).
        E::EachOpponentExilesOwnCreature(nodes::EachOpponentExilesOwnCreature { link_to_source }) => {
            b.op(Op::Ext(Ext::EoeW1(EoeW1::ChooseOwnExileLinked { who: Who::Opponent, filter: Filter::Type(types::CREATURE), linked: *link_to_source })));
        }
        // "Sacrifice Sothera": the source (its exiles stay linked to it, CR
        // 607.2a: the exile had no duration).
        E::SacrificeSelfPreservingExileLink(nodes::SacrificeSelfPreservingExileLink {}) => {
            b.op(Op::Ext(Ext::SacrificeSource));
        }
        // "... then put a creature card exiled with it onto the battlefield
        // under your control with two additional +1/+1 counters on it."
        E::ReanimateOneExiledWithSourceWithCounters(nodes::ReanimateOneExiledWithSourceWithCounters { counter_amount }) => {
            let n = l.int("ReanimateOneExiledWithSourceWithCounters.counter_amount", *counter_amount, 0, 20) as u8;
            b.op(Op::Ext(Ext::EoeW1(EoeW1::LinkedCreatureToBattlefieldWith { counter: crate::db::Counter::P1P1, n })));
        }
        _ => return false,
    }
    true
}
