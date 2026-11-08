//! Edge of Eternities, the cross-lane pass (ENGINE.md 99.8): the effect
//! classes of the cards that needed features of more than one lane, which
//! no lane implemented. Same rules as `effect.rs`: every field named, the
//! rest denied. Most classes are named after a card and carry no fields
//! (their meaning is the class's Oracle text, mtg-bot's docstring;
//! docs/ir_defects.md 11): each lowers onto ops named after rules actions,
//! its targets derived here.

use super::Lowerer;
use crate::db::{types, Counter, Filter, Mod};
use crate::eoe_x::EoeX;
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, Who};
use card_ir::{cats, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn creature() -> Filter {
    Filter::Type(types::CREATURE)
}

/// "Any target" (CR 115.4): a creature, a planeswalker, a battle or a
/// player.
fn any_target() -> Filter {
    Filter::Or(vec![creature(), Filter::Type(types::PLANESWALKER), Filter::Type(types::BATTLE), Filter::Player])
}

/// "A basic land card" (CR 205.4a).
fn basic_land() -> Filter {
    and(Filter::Type(types::LAND), Filter::Supertype(crate::db::supertypes::BASIC))
}

fn eoe(b: &mut Builder, op: EoeX) -> usize {
    b.op(Op::Ext(Ext::EoeX(op)))
}

/// "You create n Lander tokens" (CR 111.10: the predefined token, lane B's
/// definition).
fn create_landers(l: &mut Lowerer, b: &mut Builder, n: u8) {
    let token = super::eoe_b::lander(l);
    b.op(Op::CreateToken { token, n, tapped: false });
    b.op(Op::Ext(Ext::TokensCreated));
}

/// "If that creature was cast for its warp cost, it also gains <keywords>
/// until end of turn" (Full Bore; CR 702.185a): checked as the spell
/// resolves, on the first legal target of requirement `t`.
pub(crate) fn keywords_if_cast_for_warp(l: &mut Lowerer, b: &mut Builder, t: u8, keywords: &[cats::Keyword]) {
    let mods = super::effect_ext::keyword_mods(l, keywords);
    let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: Filter::CastForWarp, to: 0 });
    b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods, until: Until::EndOfTurn }));
    b.patch(at);
}

/// Lower an effect class of the cross-lane pass; `false` if the class is not
/// one of these.
pub(crate) fn lower_effect_eoe_x(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "It deals 3 damage to any target. Target player draws three cards
        // and gains 3 life" (Survey Mechan's sacrifice ability: the source
        // as it last existed deals the damage, CR 608.2h; two targets, each
        // checked on its own as it resolves, 608.2b).
        E::SurveyMechanActivationEffect(nodes::SurveyMechanActivationEffect {}) => {
            let any = b.target(any_target(), 1, 1);
            let who = b.target(Filter::Player, 1, 1);
            b.op(Op::DealDamage { amount: Amount::Fixed(3), to: any });
            b.op(Op::Draw { who: Who::TargetPlayer(who), n: Amount::Fixed(3) });
            b.op(Op::GainLife { who: Who::TargetPlayer(who), n: Amount::Fixed(3) });
        }
        // "Destroy target artifact or enchantment. If its mana value was 2 or
        // less, create a Lander token" (CR 701.8a; its mana value read before
        // it is destroyed, which destroying doesn't change; whether or not it
        // was destroyed).
        E::SeedshipImpactEffect(nodes::SeedshipImpactEffect {}) => {
            let tgt = b.target(Filter::Or(vec![Filter::Type(types::ARTIFACT), Filter::Type(types::ENCHANTMENT)]), 1, 1);
            let big = b.op(Op::JumpUnlessTarget { tgt, filter: Filter::ManaValueAtMost(2), to: 0 });
            b.op(Op::Destroy { tgt });
            create_landers(l, b, 1);
            let end = b.op(Op::Jump { to: 0 });
            b.patch(big);
            b.op(Op::Destroy { tgt });
            b.patch(end);
        }
        // "Target opponent may have you create two Lander tokens. If they
        // don't, put two +1/+1 counters on this creature" (the opponent's
        // choice, CR 603.5 / 608.2d).
        E::TerrapactIntimidatorETBEffect(nodes::TerrapactIntimidatorETBEffect {}) => {
            let opp = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            let no = b.op(Op::May { who: Who::TargetPlayer(opp), skip: 0 });
            create_landers(l, b, 2);
            let end = b.op(Op::Jump { to: 0 });
            b.patch(no);
            b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter: Counter::P1P1, n: Amount::Fixed(2) }));
            b.patch(end);
        }
        // "Choose up to one target creature. That creature can't be blocked
        // this turn and has base power and toughness 1/1 until end of turn"
        // (Atomic Microsizer; CR 509.1b, 613.4b: layer 7b).
        E::AtomicMicrosizerAttackEffect(nodes::AtomicMicrosizerAttackEffect {}) => {
            let t = b.target(creature(), 0, 1);
            let mods = vec![Mod::CantBeBlocked, Mod::SetPt(1, 1)];
            b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods, until: Until::EndOfTurn }));
        }
        // "You may sacrifice a land or Lander. If you do, search your library
        // for up to two basic land cards, put them onto the battlefield
        // tapped, then shuffle" (Larval Scoutlander; CR 701.21a, 701.23).
        E::OptionalSacLandOrLanderForBasicLandRamp(nodes::OptionalSacLandOrLanderForBasicLandRamp { land_count }) => {
            let n = l.int("OptionalSacLandOrLanderForBasicLandRamp.land_count", *land_count, 1, 8) as u8;
            let filter = Filter::Or(vec![Filter::Type(types::LAND), Filter::Subtype(l.subtype("Lander"))]);
            let at = b.op(Op::Ext(Ext::SacrificeThen { filter, optional: true, skip: 0 }));
            eoe(b, EoeX::SearchUpToOntoBattlefield { filter: basic_land(), n, tapped: true });
            b.patch(at);
        }
        // "Then discard two cards unless you discard an artifact card"
        // (Alpharael, Dreaming Acolyte; CR 701.9a: the controller chooses).
        E::AlpharaelDreamingAcolyteDiscardEffect(nodes::AlpharaelDreamingAcolyteDiscardEffect {}) => {
            let at = eoe(b, EoeX::DiscardMatchingOrContinue { filter: Filter::Type(types::ARTIFACT), to: 0 });
            b.op(Op::Discard { who: Who::You, n: 2 });
            b.patch(at);
        }
        // "Create a tapped 2/2 colorless Robot artifact creature token for
        // each multicolored permanent you control" (Infinite Guideline
        // Station; CR 105.2a: two or more colors; counted as it resolves).
        E::CreateTokenPerMulticoloredPermanent(nodes::CreateTokenPerMulticoloredPermanent { token_printing, tapped }) => {
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateTokenPerMulticoloredPermanent.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::MulticoloredPermanents, tapped: *tapped, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        _ => return false,
    }
    true
}
