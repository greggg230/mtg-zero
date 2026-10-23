//! Effects of M7b part 2 (cards with more than one face; ENGINE.md 25) ->
//! programs. Same rules as `effect.rs`: every field named, lowered onto
//! shared instructions.

use super::Lowerer;
use crate::db::{types, CardDef, Filter, Mod};
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, Who};
use card_ir::{cats, nodes};

/// Lower an effect class of this group; `false` if it is not one of them.
pub(crate) fn lower_effect_faces(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    let _ = &l;
    match e {
        // "Transform this" (CR 701.27).
        E::TransformSource(nodes::TransformSource {}) => {
            b.op(Op::Ext(Ext::TransformSource));
        }
        // "Exile this, then return it to the battlefield transformed under
        // its owner's (your) control" (CR 712.14a).
        E::ExileSourceThenReturnTransformed(nodes::ExileSourceThenReturnTransformed { under_owners_control }) => {
            b.op(Op::Ext(Ext::ExileReturnTransformed { yours: !*under_owners_control }));
        }
        // "You lose that much life" (the event's amount: the damage dealt).
        E::LoseLifeEqualToEventAmount(nodes::LoseLifeEqualToEventAmount {}) => {
            b.op(Op::LoseLife { who: Who::You, n: Amount::EventAmount });
        }
        // "Return target spell or nonland permanent [an opponent controls]
        // to its owner's hand."
        E::ReturnTargetSpellOrNonlandPermanentToHand(nodes::ReturnTargetSpellOrNonlandPermanentToHand { opponent_only }) => {
            let theirs = |f: Filter| if *opponent_only { Filter::And(Box::new(f), Box::new(Filter::Controller(false))) } else { f };
            let nonland = Filter::And(
                Box::new(Filter::Type(types::PERMANENT)),
                Box::new(Filter::Not(Box::new(Filter::Type(types::LAND)))),
            );
            let f = Filter::Or(vec![Filter::Spell(Box::new(theirs(Filter::Any))), theirs(nonland)]);
            let t = b.target(f, 1, 1);
            b.op(Op::ReturnToHand { tgt: t });
        }
        // "Up to one target creature gets -2/-0 until your next turn"
        // (CR 611.2b).
        E::PumpTargetUntilYourNextTurn(nodes::PumpTargetUntilYourNextTurn { power, toughness, targets }) => {
            let (p, t) = (l.int("PumpTargetUntilYourNextTurn.power", *power, -99, 99), l.int("PumpTargetUntilYourNextTurn.toughness", *toughness, -99, 99));
            if let Some(tgt) = super::effect::lower_targets(l, b, targets, "PumpTargetUntilYourNextTurn") {
                b.op(Op::Ext(Ext::Apply { on: On::Target(tgt), mods: vec![Mod::Pt(p, t)], until: Until::YourNextTurn }));
            }
        }
        // "Target opponent mills n cards" (CR 701.13).
        E::TargetOpponentMills(nodes::TargetOpponentMills { amount }) => {
            let n = l.int("TargetOpponentMills.amount", *amount, 1, 60);
            let t = b.target(Filter::And(Box::new(Filter::Player), Box::new(Filter::Opponent)), 1, 1);
            b.op(Op::Ext(Ext::Mill { who: Who::TargetPlayer(t), n: Amount::Fixed(n) }));
        }
        // "You may cast target <card> from your graveyard this turn. If that
        // spell would be put into your graveyard, exile it instead."
        E::MayCastTargetGraveyardCardThisTurnExileInstead(nodes::MayCastTargetGraveyardCardThisTurnExileInstead { card_filter }) => {
            let Some(cf) = super::filter::lower_card_filter(l, card_filter) else { return true };
            let f = Filter::InGraveyard(Box::new(Filter::And(Box::new(cf), Box::new(Filter::Owner(true)))));
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::GraveyardCastThenExile { tgt: t }));
        }
        // "You get an emblem with <static abilities>" (CR 114.2): "you have
        // no maximum hand size" (Tamiyo, Seasoned Scholar).
        E::CreateEmblemWithStatics(nodes::CreateEmblemWithStatics { name, static_abilities }) => {
            let _ = name; // display text
            let mut repl = super::statics::Repl::default();
            let mut statics = Vec::new();
            for s in static_abilities {
                if let Some(st) = super::statics::lower_static(l, s, &mut repl) {
                    if !matches!(st.scope, crate::db::Scope::Matching { .. }) {
                        l.unsupported("CreateEmblemWithStatics.static_abilities(scope)");
                    }
                    statics.push(st);
                }
            }
            if repl.token_factor != 0 || repl.wins_on_empty_draw {
                l.unsupported("CreateEmblemWithStatics(replacement effect)");
            }
            if statics.is_empty() && !repl.no_max_hand {
                l.unsupported("CreateEmblemWithStatics(no ability)");
            }
            let def = CardDef {
                name: format!("{} emblem", l.card_name),
                emblem: true,
                statics,
                no_max_hand: repl.no_max_hand,
                ..Default::default()
            };
            let emblem = l.token(def);
            b.op(Op::Ext(Ext::CreateEmblem { emblem }));
        }
        // "Until your next turn, whenever ..., <effect>": triggered
        // abilities the controller has for a while (CR 611.2b-like
        // duration); each is a delayed-style ability of the card.
        E::CreateTriggersUntilYourNextTurn(nodes::CreateTriggersUntilYourNextTurn { triggered_abilities }) => {
            for t in triggered_abilities {
                if let Some(td) = super::trigger::lower_trigger(l, t, 0) {
                    if td.event == crate::db::EventKind::Delayed || !td.pred.intervening().is_empty() {
                        l.unsupported("CreateTriggersUntilYourNextTurn(trigger shape)");
                    }
                    l.delayed.push(td);
                    let k = (l.delayed.len() - 1) as u8;
                    b.op(Op::Ext(Ext::TriggerUntilYourNextTurn { k }));
                }
            }
            if triggered_abilities.is_empty() {
                l.unsupported("CreateTriggersUntilYourNextTurn(none)");
            }
        }
        // "It gets +p/+t until end of turn" (the triggered ability's event
        // object, CR 611.2c).
        E::EventSubjectGetsUEOT(nodes::EventSubjectGetsUEOT { power, toughness }) => {
            let (p, t) = (l.int("EventSubjectGetsUEOT.power", *power, -99, 99), l.int("EventSubjectGetsUEOT.toughness", *toughness, -99, 99));
            b.op(Op::Ext(Ext::Apply { on: On::EventObj, mods: vec![Mod::Pt(p, t)], until: Until::EndOfTurn }));
        }
        // "Draw cards equal to half the number of cards in your library,
        // rounded up."
        E::DrawHalfLibraryRoundedUp(nodes::DrawHalfLibraryRoundedUp {}) => {
            b.op(Op::Draw { who: Who::You, n: Amount::HalfLibraryUp });
        }
        // "Each player exiles cards from the top of their library until they
        // exile a nonland card. You may cast any number of spells from among
        // the nonland cards exiled this way without paying their mana costs."
        E::EachPlayerExilesUntilNonlandMayCastFree(nodes::EachPlayerExilesUntilNonlandMayCastFree {}) => {
            b.op(Op::Ext(Ext::EachExileUntilNonlandCastFree));
        }
        // "Whenever this deals combat damage to a player, they get that many
        // poison counters" (CR 122.1f).
        E::PoisonCountersEqualToDamage(nodes::PoisonCountersEqualToDamage {}) => {
            b.op(Op::Ext(Ext::Poison { who: Who::EventPlayer, n: Amount::EventAmount }));
        }
        // "This Saga gains '<ability>'" (Urza's Saga): a latent activated
        // ability of the card that the permanent has once it gains it.
        E::GrantActivatedAbility(nodes::GrantActivatedAbility { ability }) => {
            let Some(a) = ability.as_deref() else {
                l.unsupported("GrantActivatedAbility.ability(missing)");
                return true;
            };
            if let Some(ad) = super::activated::lower_activated(l, a, super::activated::From::Battlefield, false) {
                l.latent.push(ad);
                let k = (l.latent.len() - 1) as u8;
                b.op(Op::Ext(Ext::Apply { on: On::Source, mods: vec![Mod::GainLatent(k)], until: Until::Forever }));
            }
        }
        // "You may discard up to n cards. If you do, draw that many cards."
        E::DiscardUpToNThenDrawThatMany(nodes::DiscardUpToNThenDrawThatMany { n }) => {
            let n = l.int("DiscardUpToNThenDrawThatMany.n", *n, 1, 7) as u8;
            b.op(Op::Ext(Ext::DiscardUpToThenDraw { n }));
        }
        _ => return false,
    }
    true
}
