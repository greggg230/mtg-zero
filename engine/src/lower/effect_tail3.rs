//! Effects of the single-card tail, part 3 (M7c, ENGINE.md 30). Same rules
//! as `effect.rs`: every field named, the rest denied.

use super::effect::lower_effect;
use super::Lowerer;
use crate::db::{Counter, Filter};
use crate::program::{Amount, Builder, Ext, Op, Who};
use card_ir::{cats, nodes};

/// "One <token>" of a `CreateToken` inside another effect -> (token, n,
/// tapped).
fn token_of(l: &mut Lowerer, e: &cats::Effect, what: &str) -> Option<(u16, u8, bool)> {
    match e {
        cats::Effect::CreateToken(nodes::CreateToken { token_printing, count, tapped, each_player, attacking, schedule_end_of_combat_sacrifice }) => {
            if *each_player || *attacking || *schedule_end_of_combat_sacrifice {
                l.unsupported(format!("{what}(not plain tokens)"));
                return None;
            }
            let n = l.int(&format!("{what}.count"), *count, 1, 20) as u8;
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported(format!("{what}(not a token)"));
            }
            Some((l.token(def), n, *tapped))
        }
        other => {
            l.unsupported(format!("{what}({})", other.tag()));
            None
        }
    }
}

/// Lower an effect class of the tail, part 3; `false` if the class is not
/// one of these.
pub(crate) fn lower_effect_tail3(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Create twice X 1/1 ... tokens" (Pest Infestation: the spell's X).
        E::CreateTokensScaledByX(nodes::CreateTokensScaledByX { token_printing, multiplier }) => {
            if !l.spell_x {
                l.unsupported("CreateTokensScaledByX(no X)");
            }
            let k = l.int("CreateTokensScaledByX.multiplier", *multiplier, 1, 4) as u8;
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateTokensScaledByX.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::XTimes(k), tapped: false, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "You may <first>. When you do, <then>" (CR 603.12: a reflexive
        // triggered ability; its targets are chosen as it is put on the
        // stack). "You may discard a card" (Inti) or an effect without
        // targets (Generous Plunderer's Treasure).
        E::MayDoThenReflexive(nodes::MayDoThenReflexive { first, then }) => {
            let (Some(first), Some(then)) = (first.as_deref(), then.as_deref()) else {
                l.unsupported("MayDoThenReflexive(missing a part)");
                return true;
            };
            let skip = match first {
                E::DiscardSelf(nodes::DiscardSelf { amount }) => {
                    if *amount != 1 {
                        l.unsupported("MayDoThenReflexive.first(DiscardSelf of more than one)");
                    }
                    b.op(Op::Ext(Ext::DiscardThen { skip: 0, optional: true }))
                }
                other => {
                    let at = b.op(Op::May { who: Who::You, skip: 0 });
                    let n = b.targets.len();
                    lower_effect(l, b, other);
                    if b.targets.len() != n {
                        l.unsupported("MayDoThenReflexive.first(with targets)");
                    }
                    at
                }
            };
            let mut d = Builder::default();
            lower_effect(l, &mut d, then);
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Reflexive { k }));
            b.patch(skip);
        }
        // "Target opponent creates a tapped Treasure token."
        E::TargetOpponentCreatesToken(nodes::TargetOpponentCreatesToken { token_effect }) => {
            let Some(e) = token_effect.as_deref() else {
                l.unsupported("TargetOpponentCreatesToken.token_effect(missing)");
                return true;
            };
            let Some((token, n, tapped)) = token_of(l, e, "TargetOpponentCreatesToken.token_effect") else { return true };
            let t = b.target(Filter::And(Box::new(Filter::Player), Box::new(Filter::Opponent)), 1, 1);
            b.op(Op::Ext(Ext::TokensFor { who: Who::TargetPlayer(t), token, n, tapped }));
        }
        // "You gain protection from everything until your next turn" (CR
        // 702.16j, for a player: 702.16b-e).
        E::GainProtectionFromEverythingUntilYourNextTurn(nodes::GainProtectionFromEverythingUntilYourNextTurn {}) => {
            b.op(Op::Ext(Ext::PlayerProtection));
        }
        // "You lose 1 life for each burden counter on The One Ring" (as it
        // last existed if it left, CR 608.2h).
        E::LoseLifePerCounterOnSource(nodes::LoseLifePerCounterOnSource { kind, per }) => {
            if *per != 1 {
                l.unsupported("LoseLifePerCounterOnSource.per");
            }
            let k: Counter = super::effect_ext::counter_by_name(l, kind);
            b.op(Op::LoseLife { who: Who::You, n: Amount::SourceCounters(k) });
        }
        // "Draw a card for each burden counter on The One Ring."
        E::DrawCardsPerCounterOnSource(nodes::DrawCardsPerCounterOnSource { kind }) => {
            let k: Counter = super::effect_ext::counter_by_name(l, kind);
            b.op(Op::Draw { who: Who::You, n: Amount::SourceCounters(k) });
        }
        // "Shuffle it into its owner's library" (Worldspine Wurm: the card
        // in the graveyard that triggered).
        E::ShuffleSourceIntoOwnersLibrary(nodes::ShuffleSourceIntoOwnersLibrary {}) => {
            b.op(Op::Ext(Ext::ShuffleSourceIntoLibrary));
        }
        // "Its owner shuffles their graveyard into their library" (Emrakul:
        // the ability's controller is the card's owner).
        E::OwnerShufflesGraveyardIntoLibrary(nodes::OwnerShufflesGraveyardIntoLibrary {}) => {
            b.op(Op::ShuffleIntoLibrary { who: Who::You, hand: false, graveyard: true });
        }
        // Annihilator's trigger: "defending player sacrifices n permanents"
        // (two players: the controller's opponent).
        E::DefendingPlayerSacrificesPermanents(nodes::DefendingPlayerSacrificesPermanents { count }) => {
            let n = l.int("DefendingPlayerSacrificesPermanents.count", *count, 1, 20) as u8;
            b.op(Op::Ext(Ext::SacrificeN { who: Who::Opponent, filter: Filter::Type(crate::db::types::PERMANENT), n }));
        }
        // "He deals damage equal to his power to each of up to X targets,
        // where X is the number of times he was kicked" (Batroc: the IR's
        // target specification gives the filter; the count is the class's).
        E::DealPowerDamageToEachOfUpToKickedTargets(nodes::DealPowerDamageToEachOfUpToKickedTargets { targets }) => {
            let [cats::TargetSpec::TargetSpec(nodes::TargetSpec { filter, count_min, count_max, distinct_from })] = &targets[..] else {
                l.unsupported("DealPowerDamageToEachOfUpToKickedTargets.targets(not one)");
                return true;
            };
            l.deny("TargetSpec.distinct_from", distinct_from);
            if *count_min != 0 || *count_max != card_ir::Count::Finite(1) {
                l.unsupported("DealPowerDamageToEachOfUpToKickedTargets.targets(counts)");
            }
            let Some(f) = super::filter::lower_pred_or_oneof(l, filter) else { return true };
            let t = b.target(f, 0, crate::db::KICKS_UP_TO);
            b.op(Op::DealDamage { amount: Amount::SourcePower, to: t });
        }
        // "Gain control of target artifact with mana value X or less. If that
        // artifact is an Equipment, attach it to this creature" (Thieving
        // Skydiver; X its kicker's). The IR exports no target specification
        // (docs/ir_defects.md 6): the target is the Oracle text's.
        E::GainControlOfTargetThenAttachIfEquipment(nodes::GainControlOfTargetThenAttachIfEquipment {}) => {
            let f = Filter::And(Box::new(Filter::Type(crate::db::types::ARTIFACT)), Box::new(Filter::MvAtMostSourceX));
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::GainControl { tgt: t }));
            b.op(Op::Ext(Ext::AttachTargetToSource { tgt: t }));
        }
        _ => return false,
    }
    true
}

/// "It deals damage to defending player equal to the number of <type>s
/// they control" (Generous Plunderer: `DealDamageFromEvent` with
/// `to_defending_player`; two players: the defending player is the
/// controller's opponent, CR 506.2).
pub(crate) fn damage_defending_player(l: &mut Lowerer, b: &mut Builder, amount: Option<&cats::Formula>) {
    match amount {
        Some(cats::Formula::CountPermanentsOfPlayer(nodes::CountPermanentsOfPlayer { predicate: Some(p) })) => {
            match super::filter::lower_pred(l, p) {
                Some(Filter::Type(bits)) => {
                    b.op(Op::DamagePlayer { amount: Amount::CountTypesOpp(bits), who: Who::Opponent });
                }
                _ => l.unsupported("Formula:CountPermanentsOfPlayer(not a card type)"),
            }
        }
        Some(other) => l.unsupported(format!("DealDamageFromEvent.to_defending_player(amount {})", other.tag())),
        None => l.unsupported("DealDamageFromEvent.amount(missing)"),
    }
}
