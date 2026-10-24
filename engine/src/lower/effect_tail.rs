//! Effects of the single-card tail (M7c, ENGINE.md 28). Same rules as
//! `effect.rs`: every field named, the rest denied.

use super::effect::lower_effect;
use super::Lowerer;
use crate::db::{types, Filter, KwSet};
use crate::mana::ManaCost;
use crate::program::{Amount, Builder, Ext, Op, PileDest, Who, PILE_ANY};
use card_ir::{cats, enums, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// Lower an effect class of the tail; `false` if the class is not one of
/// these.
pub(crate) fn lower_effect_tail(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "You may pay {4}. If you do, untap this artifact." (Mana Vault;
        // CR 118.12-style optional cost as it resolves.)
        E::OptionalManaCostThenEffect(nodes::OptionalManaCostThenEffect { cost_str, inner_effect }) => {
            let Some(cost) = ManaCost::parse(cost_str) else {
                l.unsupported(format!("OptionalManaCostThenEffect.cost_str({cost_str})"));
                return true;
            };
            let pay = b.op(Op::OfferPay { who: Who::You, cost, if_paid: 0 });
            let end = b.op(Op::Jump { to: 0 });
            b.patch(pay);
            lower_effect(l, b, inner_effect);
            b.patch(end);
        }
        // "It deals n damage to you."
        E::SourceDealsDamageToController(nodes::SourceDealsDamageToController { amount }) => {
            let amount = Amount::Fixed(l.int("SourceDealsDamageToController.amount", *amount, 0, 99));
            b.op(Op::DamagePlayer { amount, who: Who::You });
        }
        // "Put a charge counter on this artifact."
        E::PutChargeCountersOnSelf(nodes::PutChargeCountersOnSelf { amount, amount_from_x }) => {
            l.deny("PutChargeCountersOnSelf.amount_from_x", amount_from_x);
            let n = Amount::Fixed(l.int("PutChargeCountersOnSelf.amount", *amount, 1, 20));
            let counter = super::effect_ext::counter_by_name(l, "charge");
            b.op(Op::Ext(Ext::PutCounters { on: crate::program::On::Source, counter, n }));
        }
        // "Remove all charge counters from this artifact. Add one mana of
        // any color for each charge counter removed this way."
        E::RemoveAllCountersAddManaPerCounter(nodes::RemoveAllCountersAddManaPerCounter { kind }) => {
            let counter = super::effect_ext::counter_by_name(l, kind);
            b.op(Op::Ext(Ext::RemoveAllCountersAddMana { counter }));
        }
        // Imprint: "you may exile a nonartifact, nonland card from your
        // hand" (CR 702.? imprint is a linked ability, 607.2a).
        E::ImprintCardFromHand(nodes::ImprintCardFromHand { excluded_types, card_filter }) => {
            if card_filter.is_some() {
                l.unsupported("ImprintCardFromHand.card_filter");
            }
            let mut bits = 0u16;
            for t in excluded_types {
                match types::from_name(t) {
                    Some(x) => bits |= x,
                    None => l.unsupported(format!("ImprintCardFromHand.excluded_types({t})")),
                }
            }
            let filter = if bits == 0 { Filter::Any } else { Filter::Not(Box::new(Filter::Type(bits))) };
            b.op(Op::Ext(Ext::ExileFromHandLinked { chooser: Who::You, whose: Who::You, filter, optional: true, until: false }));
        }
        // "Then target opponent may have you draw a card. If that player
        // doesn't, you mill X cards, where X is the number of influence
        // counters on this, and that player loses life equal to the total
        // mana value of those cards." (Palantir of Orthanc.)
        E::TargetOpponentMayLetYouDrawElseMill(nodes::TargetOpponentMayLetYouDrawElseMill { kind }) => {
            let counter = super::effect_ext::counter_by_name(l, kind);
            let tgt = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            b.op(Op::Ext(Ext::LetDrawElseMill { tgt, counter }));
        }
        // "Exile a card at random from your graveyard. You may play that
        // card this turn." (Tersa Lightshatter.)
        E::ExileRandomGraveyardCardWithPlayPermission(nodes::ExileRandomGraveyardCardWithPlayPermission { duration }) => {
            if *duration != enums::ExilePlayDuration::ThisTurn {
                l.unsupported(format!("ExileRandomGraveyardCardWithPlayPermission.duration({duration:?})"));
            }
            b.op(Op::Ext(Ext::ExileRandomGraveyardPlayable));
        }
        // "Choose a card name. Exile the top six cards of your library, then
        // reveal cards from the top of your library until you reveal a card
        // with the chosen name. Put that card into your hand and exile all
        // other cards revealed this way." (Demonic Consultation.)
        E::ChooseNameExileThenRevealUntilNamed(nodes::ChooseNameExileThenRevealUntilNamed { exile_first }) => {
            let n = l.int("ChooseNameExileThenRevealUntilNamed.exile_first", *exile_first, 0, 60) as u8;
            b.op(Op::Ext(Ext::NameExileRevealUntil { n }));
        }
        E::BalanceEffect(nodes::BalanceEffect {}) => {
            b.op(Op::Ext(Ext::Balance));
        }
        // "Mill a card and choose an opponent at random. This creature
        // attacks that player this combat if able. Until end of turn, this
        // creature gains indestructible and gets +1/+1 for each card type
        // among cards in your graveyard." (Ursine Monstrosity.)
        E::MillChooseOpponentAttackPump(nodes::MillChooseOpponentAttackPump { mill, keywords }) => {
            let n = l.int("MillChooseOpponentAttackPump.mill", *mill, 0, 20) as u8;
            let mut kws = KwSet::default();
            for k in keywords {
                match super::printing::lower_keyword_full(l, k) {
                    Some(super::printing::KwOrProt::Kw(kw)) => kws.insert(kw),
                    Some(_) => l.unsupported("MillChooseOpponentAttackPump.keywords(not a plain keyword)"),
                    None => {}
                }
            }
            b.op(Op::Ext(Ext::MillAttackPump { n, kws }));
        }
        E::SacrificeArtifactTutorArtifactPayDifference(nodes::SacrificeArtifactTutorArtifactPayDifference {}) => {
            b.op(Op::Ext(Ext::SacrificeTutorPayDifference));
        }
        // Lim-Dul's Vault: the loop, then "put the last cards you looked at
        // this way on top in any order".
        E::LookAtTopRepeatPayLifeThenStackTop(nodes::LookAtTopRepeatPayLifeThenStackTop { count, life_per_repeat }) => {
            let n = l.int("LookAtTopRepeatPayLifeThenStackTop.count", *count, 1, 20) as u8;
            let life = l.int("LookAtTopRepeatPayLifeThenStackTop.life_per_repeat", *life_per_repeat, 0, 20) as u8;
            b.op(Op::Ext(Ext::LookTopRepeatPayLife { n, life }));
            b.op(Op::PileTo { who: Who::You, dest: PileDest::Top, filter: Filter::Any, min: 0, max: PILE_ANY, rest: true, reveal: false });
        }
        // "Draw a card, then discard a card. If there are four or more
        // chorus counters on this, you may cast the discarded card without
        // paying its mana cost." (Malcolm, Alluring Scoundrel.)
        E::LootThenMayCastDiscardedFreeIfCounters(nodes::LootThenMayCastDiscardedFreeIfCounters { kind, threshold }) => {
            let counter = super::effect_ext::counter_by_name(l, kind);
            let n = l.int("LootThenMayCastDiscardedFreeIfCounters.threshold", *threshold, 0, 20) as u8;
            b.op(Op::Ext(Ext::LootMayCastFree { counter, n }));
        }
        // "Look at target opponent's hand. You may exile a nonland card from
        // it. For as long as that card remains exiled, its owner may play
        // it. A spell cast this way costs {2} more to cast." (Elite
        // Spellbinder.)
        E::LookAtHandExileNonlandOwnerMayPlayTaxed(nodes::LookAtHandExileNonlandOwnerMayPlayTaxed { surcharge }) => {
            let tax = l.int("LookAtHandExileNonlandOwnerMayPlayTaxed.surcharge", *surcharge, 0, 20) as u8;
            let t = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            b.op(Op::RevealHand { whose: Who::TargetPlayer(t), to: Some(Who::You) });
            let filter = Filter::Not(Box::new(Filter::Type(types::LAND)));
            b.op(Op::Ext(Ext::ExileFromHandTaxed { whose: Who::TargetPlayer(t), filter, tax }));
        }
        // "Choose target opponent and up to one target creature they
        // control. They reveal their hand. You may exile a nonland card from
        // their hand or the chosen creature until this leaves the
        // battlefield." (Cloak and Dagger, Entwined; two players: "they
        // control" is "an opponent controls".)
        E::RevealHandMayExileCardOrChosenCreatureUntilSourceLeaves(
            nodes::RevealHandMayExileCardOrChosenCreatureUntilSourceLeaves { card_filter },
        ) => {
            let filter = match card_filter {
                Some(f) => match super::filter::lower_card_filter(l, f) {
                    Some(f) => f,
                    None => return true,
                },
                None => Filter::Any,
            };
            let t = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            let c = b.target(and(Filter::Type(types::CREATURE), Filter::Controller(false)), 0, 1);
            b.op(Op::RevealHand { whose: Who::TargetPlayer(t), to: None });
            b.op(Op::Ext(Ext::ExileHandCardOrTargetUntil { whose: Who::TargetPlayer(t), filter, tgt: c }));
        }
        // "Exile all creatures. Incubate X, where X is the number of
        // creatures exiled this way." (Sunfall.)
        E::ExileAllCreaturesThenIncubate(nodes::ExileAllCreaturesThenIncubate {}) => {
            let Some(token) = super::effect_ext::named_token(l, "Incubator") else { return true };
            b.op(Op::Ext(Ext::ExileAllCreaturesIncubate { token }));
        }
        // Earthbend n (CR 701.65a): "Target land you control becomes a 0/0
        // creature with haste that's still a land. Put n +1/+1 counters on
        // it. When it dies or is exiled, return it to the battlefield
        // tapped."
        E::Earthbend(nodes::Earthbend { n }) => {
            let n = l.int("Earthbend.n", *n, 1, 20);
            let t = b.target(and(Filter::Type(types::LAND), Filter::Controller(true)), 1, 1);
            let mods = vec![
                crate::db::Mod::Types { add: types::CREATURE, subtypes: Vec::new(), replace: false },
                crate::db::Mod::SetPt(0, 0),
                crate::db::Mod::Keywords({
                    let mut k = KwSet::default();
                    k.insert(crate::db::Kw::Haste);
                    k
                }),
            ];
            b.op(Op::Ext(Ext::Apply { on: crate::program::On::Target(t), mods, until: crate::layers::Until::Forever }));
            b.op(Op::Ext(Ext::PutCounters { on: crate::program::On::Target(t), counter: crate::db::Counter::P1P1, n: Amount::Fixed(n) }));
            let mut back = Builder::default();
            back.op(Op::Ext(Ext::ReturnEventCard));
            back.op(Op::Ext(Ext::Tap { on: crate::program::On::Made }));
            let k = l.delayed_ability(back.finish());
            b.op(Op::Ext(Ext::GrantLeaveTrigger { on: crate::program::On::Target(t), k, exile: true }));
        }
        // "Choose target artifact a player controls and target artifact card
        // in that player's graveyard. If both targets are still legal as
        // this ability resolves, that player simultaneously sacrifices the
        // artifact and returns the artifact card to the battlefield."
        // (Goblin Welder.)
        E::SwapArtifactWithGraveyardArtifact(nodes::SwapArtifactWithGraveyardArtifact {}) => {
            let artifact = Filter::Type(types::ARTIFACT);
            let a = b.target(artifact.clone(), 1, 1);
            let c = b.target(Filter::InGraveyard(Box::new(artifact)), 1, 1);
            b.targets[c as usize].pair = a + 1;
            b.op(Op::Ext(Ext::SwapWithGraveyardCard { a, b: c }));
        }
        E::ExileUntilNonlandThenCounterOrMayCastIt(nodes::ExileUntilNonlandThenCounterOrMayCastIt {}) => {
            b.op(Op::Ext(Ext::ExileUntilNonlandCounterOrCast));
        }
        _ => return false,
    }
    true
}
