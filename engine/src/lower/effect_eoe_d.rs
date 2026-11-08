//! Edge of Eternities, lane D (ENGINE.md 99.5): creatures, combat, removal,
//! damage, life, tokens and pumps. Same rules as `effect.rs`: every field
//! named, the rest denied. Many of these IR classes are named after a card
//! and carry no fields (their meaning is the class's Oracle text, mtg-bot's
//! docstring; docs/ir_defects.md 11): each lowers onto ops named after rules
//! actions, with its targets re-derived here.

use super::effect::{lower_effect, lower_targets};
use super::Lowerer;
use crate::db::{types, Cond, Counter, Filter, Kw, KwSet, Mod, TrigPred};
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, Who};
use card_ir::{cats, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn not(a: Filter) -> Filter {
    Filter::Not(Box::new(a))
}

fn creature() -> Filter {
    Filter::Type(types::CREATURE)
}

fn kws(list: &[Kw]) -> Mod {
    let mut s = KwSet::default();
    for &k in list {
        s.insert(k);
    }
    Mod::Keywords(s)
}

fn apply(b: &mut Builder, on: On, mods: Vec<Mod>, until: Until) {
    b.op(Op::Ext(Ext::Apply { on, mods, until }));
}

/// Keywords written as strings (`granted_keywords: Vec<String>`).
fn keyword_names(l: &mut Lowerer, what: &str, names: &[String]) -> Option<Mod> {
    let mut s = KwSet::default();
    for n in names {
        match super::effect_ext::kw_by_name(n) {
            Some(k) => s.insert(k),
            None => l.unsupported(format!("{what}({n})")),
        }
    }
    (!s.is_empty()).then_some(Mod::Keywords(s))
}

/// "You control n or more tapped creatures."
fn tapped_creatures(l: &mut Lowerer, n: i64) -> Cond {
    let n = l.int("ConditionalOnTappedCreatures.min_count", n, 1, 99) as u8;
    Cond::ControlAtLeast(and(creature(), Filter::Tapped), n)
}

/// A triggered ability's intervening "if" (CR 603.4) the IR writes as a
/// wrapper around the ability's effect: "At the beginning of your end step,
/// if you control two or more tapped creatures, <effect>" (mtg-bot's
/// `ConditionalOnTappedCreatures`, made from the trigger line). The
/// condition joins the trigger's predicate (checked as the event happens and
/// again on resolution, `TrigPred::If`); the effect is what is inside.
pub(crate) fn hoist_intervening_if<'a>(
    l: &mut Lowerer,
    effect: Option<&'a Box<cats::Effect>>,
    pred: Option<TrigPred>,
) -> (Option<&'a Box<cats::Effect>>, Option<TrigPred>) {
    match effect.map(|e| &**e) {
        Some(cats::Effect::ConditionalOnTappedCreatures(nodes::ConditionalOnTappedCreatures { effect: inner, min_count })) => {
            let c = tapped_creatures(l, *min_count);
            (Some(inner), pred.map(|p| TrigPred::And(Box::new(p), Box::new(TrigPred::If(c)))))
        }
        _ => (effect, pred),
    }
}

/// "If it's not your turn, untap those creatures" (Zealous Display): the
/// creatures `f` matches, checked as the spell resolves.
pub(crate) fn untap_unless_your_turn(b: &mut Builder, f: Filter) {
    let theirs = b.op(Op::JumpUnless { cond: Cond::YourTurn, to: 0 });
    let end = b.op(Op::Jump { to: 0 });
    b.patch(theirs);
    b.op(Op::Ext(Ext::Untap { on: On::Each(f) }));
    b.patch(end);
}

/// "Each player creates n <token>" (CR 101.4: in APNAP order).
pub(crate) fn each_player_creates(l: &mut Lowerer, b: &mut Builder, token: u16, n: u8, tapped: bool, other: bool) {
    if other {
        l.unsupported("CreateToken.each_player(attacking or sacrificed at end of combat)");
    }
    for who in [Who::Active, Who::NonActive] {
        b.op(Op::Ext(Ext::TokensFor { who, token, n, tapped }));
        b.op(Op::Ext(Ext::TokensCreated));
    }
}

/// "Sacrifice that token at end of combat": a delayed triggered ability
/// about each token just made (CR 603.7, 511.2; 701.21a).
pub(crate) fn sacrifice_made_at_end_of_combat(l: &mut Lowerer, b: &mut Builder) {
    let mut d = Builder::default();
    d.op(Op::Ext(Ext::SacrificeEventObj));
    let k = l.delayed_ability(d.finish());
    b.op(Op::Ext(Ext::Delay { k, when: crate::db::DelayedWhen::EndOfCombat, about: crate::program::About::EachMade }));
}

/// Lower a lane D effect class; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_d(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // Only a triggered ability's intervening "if" (`hoist_intervening_if`);
        // anywhere else the IR would say something no card says.
        E::ConditionalOnTappedCreatures(nodes::ConditionalOnTappedCreatures { effect, min_count }) => {
            let _ = (effect, min_count);
            l.unsupported("ConditionalOnTappedCreatures(not a triggered ability's intervening if)");
        }
        // "You may discard your hand. If you do, draw n cards" (Vaultguard
        // Trooper; the class's "may", its docstring). Discarding a hand of no
        // cards is still discarding your hand.
        E::DiscardHandThenDrawN(nodes::DiscardHandThenDrawN { amount }) => {
            let n = l.int("DiscardHandThenDrawN.amount", *amount, 1, 20);
            let at = b.op(Op::May { who: Who::You, skip: 0 });
            b.op(Op::DiscardHand { who: Who::You });
            b.op(Op::Draw { who: Who::You, n: Amount::Fixed(n) });
            b.patch(at);
        }
        // "It deals n damage to target creature an opponent controls" (Nova
        // Hellkite; the source deals it, CR 120.3).
        E::DealDamageToOpponentCreature(nodes::DealDamageToOpponentCreature { amount, targets }) => {
            let amount = Amount::Fixed(l.int("DealDamageToOpponentCreature.amount", *amount, 0, 99));
            let Some(to) = lower_targets(l, b, targets, "DealDamageToOpponentCreature") else { return true };
            if b.targets[to as usize].filter != and(creature(), Filter::Controller(false)) {
                l.unsupported("DealDamageToOpponentCreature.targets(not a creature an opponent controls)");
            }
            b.op(Op::DealDamage { amount, to });
        }
        // "Return up to one other target permanent you control to its
        // owner's hand" (Exosuit Savior).
        E::ReturnTargetPermanentYouControlToOwnersHand(nodes::ReturnTargetPermanentYouControlToOwnersHand {
            targets,
            optional,
            exclude_self,
        }) => {
            let Some(t) = lower_targets(l, b, targets, "ReturnTargetPermanentYouControlToOwnersHand") else { return true };
            let req = &b.targets[t as usize];
            // The flags restate the target spec: "up to one", "other".
            if *optional != (req.min == 0) || *exclude_self != mentions_not_source(&req.filter) {
                l.unsupported("ReturnTargetPermanentYouControlToOwnersHand(flags disagree with the targets)");
            }
            b.op(Op::ReturnToHand { tgt: t });
        }
        // "Tap target artifact or creature an opponent controls and put a
        // stun counter on it" (Mechanozoa; CR 701.26a, 122.1d).
        E::TapTargetOpponentPermanentAndStunIt(nodes::TapTargetOpponentPermanentAndStunIt {}) => {
            let f = and(Filter::Or(vec![Filter::Type(types::ARTIFACT), creature()]), Filter::Controller(false));
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::Tap { on: On::Target(t) }));
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::Stun, n: Amount::Fixed(1) }));
        }
        // "You may sacrifice another creature or artifact. If you do,
        // <effect>" (Swarm Culler; CR 701.21a).
        E::OptionalSacAnotherForEffect(nodes::OptionalSacAnotherForEffect { inner_effect }) => {
            let at = b.op(Op::Ext(Ext::SacrificeThen { filter: another_creature_or_artifact(), optional: true, skip: 0 }));
            lower_effect(l, b, inner_effect);
            b.patch(at);
        }
        // "You may sacrifice another creature or artifact. If you do, this
        // creature gets +p/+t [and gains ...] until end of turn" (Comet
        // Crawler).
        E::OptionalSacAnotherForPumpSelfUEOT(nodes::OptionalSacAnotherForPumpSelfUEOT {
            power_bonus,
            toughness_bonus,
            granted_keywords,
        }) => {
            let mut mods = Vec::new();
            let (p, t) = (
                l.int("OptionalSacAnotherForPumpSelfUEOT.power_bonus", *power_bonus, -99, 99),
                l.int("OptionalSacAnotherForPumpSelfUEOT.toughness_bonus", *toughness_bonus, -99, 99),
            );
            if p != 0 || t != 0 {
                mods.push(Mod::Pt(p, t));
            }
            mods.extend(keyword_names(l, "OptionalSacAnotherForPumpSelfUEOT.granted_keywords", granted_keywords));
            if mods.is_empty() {
                l.unsupported("pump(nothing)");
            }
            let at = b.op(Op::Ext(Ext::SacrificeThen { filter: another_creature_or_artifact(), optional: true, skip: 0 }));
            apply(b, On::Source, mods, Until::EndOfTurn);
            b.patch(at);
        }
        // "Target creature gets -3/-0 until end of turn. Then if that
        // creature's power is 0 or less, destroy it" (Depressurize; CR
        // 701.8a).
        E::DepressurizeEffect(nodes::DepressurizeEffect { power_penalty }) => {
            let p = l.int("DepressurizeEffect.power_penalty", *power_penalty, 1, 99);
            let t = b.target(creature(), 1, 1);
            apply(b, On::Target(t), vec![Mod::Pt(-p, 0)], Until::EndOfTurn);
            let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: Filter::PowerAtMost(0), to: 0 });
            b.op(Op::Destroy { tgt: t });
            b.patch(at);
        }
        // "Put a +1/+1 counter on target creature you control. It gains
        // reach, trample, and indestructible until end of turn. Untap it"
        // (Biosynthic Burst; CR 701.26b).
        E::BiosynthicBurstEffect(nodes::BiosynthicBurstEffect {}) => {
            let t = b.target(and(creature(), Filter::Controller(true)), 1, 1);
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(1) }));
            apply(b, On::Target(t), vec![kws(&[Kw::Reach, Kw::Trample, Kw::Indestructible])], Until::EndOfTurn);
            b.op(Op::Untap { tgt: t });
        }
        // "Target creature has base power and toughness p/t until end of
        // turn. If you control n or more lands, that creature has base power
        // and toughness p'/t' until end of turn instead" (Genemorph Imago;
        // layer 7b, CR 613.4b; the land count as the ability resolves).
        E::SetBasePTConditionalUntilEOT(nodes::SetBasePTConditionalUntilEOT {
            default_power,
            default_toughness,
            land_threshold,
            upgrade_power,
            upgrade_toughness,
        }) => {
            let base = Mod::SetPt(
                l.int("SetBasePTConditionalUntilEOT.default_power", *default_power, 0, 99),
                l.int("SetBasePTConditionalUntilEOT.default_toughness", *default_toughness, 0, 99),
            );
            let up = Mod::SetPt(
                l.int("SetBasePTConditionalUntilEOT.upgrade_power", *upgrade_power, 0, 99),
                l.int("SetBasePTConditionalUntilEOT.upgrade_toughness", *upgrade_toughness, 0, 99),
            );
            let n = l.int("SetBasePTConditionalUntilEOT.land_threshold", *land_threshold, 1, 99) as u8;
            let t = b.target(creature(), 1, 1);
            let few = b.op(Op::JumpUnless { cond: Cond::ControlAtLeast(Filter::Type(types::LAND), n), to: 0 });
            apply(b, On::Target(t), vec![up], Until::EndOfTurn);
            let end = b.op(Op::Jump { to: 0 });
            b.patch(few);
            apply(b, On::Target(t), vec![base], Until::EndOfTurn);
            b.patch(end);
        }
        _ => return false,
    }
    true
}

/// "Another creature or artifact" ("another" over both: no article before
/// "artifact").
fn another_creature_or_artifact() -> Filter {
    and(Filter::Or(vec![creature(), Filter::Type(types::ARTIFACT)]), not(Filter::Source))
}

/// Does the filter exclude the source ("other", `Not(Source)` in an `And`)?
fn mentions_not_source(f: &Filter) -> bool {
    match f {
        Filter::Not(x) => **x == Filter::Source,
        Filter::And(a, b) => mentions_not_source(a) || mentions_not_source(b),
        _ => false,
    }
}
