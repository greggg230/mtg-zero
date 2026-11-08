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
use crate::program::{Amount, Builder, EoeD, Ext, On, Op, Who};
use card_ir::{cats, enums, nodes};

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
        // "At the beginning of your upkeep, if you control three or more
        // artifacts, <effect>" (Inventors' Fair).
        Some(cats::Effect::ConditionalIfYouControlAtLeast(nodes::ConditionalIfYouControlAtLeast { threshold, predicate, inner })) => {
            let n = l.int("ConditionalIfYouControlAtLeast.threshold", *threshold, 1, 99) as u8;
            let c = Cond::ControlAtLeast(Filter::Type(type_word(*predicate)), n);
            (Some(inner), pred.map(|p| TrigPred::And(Box::new(p), Box::new(TrigPred::If(c)))))
        }
        _ => (effect, pred),
    }
}

/// A card type word ("artifact") -> its `types` bit.
fn type_word(w: enums::CardTypeWord) -> u16 {
    use enums::CardTypeWord as W;
    match w {
        W::Artifact => types::ARTIFACT,
        W::Battle => types::BATTLE,
        W::Creature => types::CREATURE,
        W::Enchantment => types::ENCHANTMENT,
        W::Instant => types::INSTANT,
        W::Land => types::LAND,
        W::Planeswalker => types::PLANESWALKER,
        W::Sorcery => types::SORCERY,
    }
}

fn eoe(b: &mut Builder, op: EoeD) {
    b.op(Op::Ext(Ext::EoeD(op)));
}

/// "Until end of turn, <it> gains 'When this creature dies, <effect>'"
/// (Perigee Beckoner): the granted ability's effect is a delayed ability of
/// the card, attached to each object of `on` until end of turn.
pub(crate) fn grant_trigger_this_turn(l: &mut Lowerer, b: &mut Builder, on: On, t: &cats::TriggeredAbility) {
    let cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility {
        effect,
        requires_void,
        once_per_turn,
        tier_threshold,
        event,
        predicate,
        room_half,
        functions_in_graveyard,
    }) = t;
    l.deny("granted_triggered_ability.requires_void", requires_void);
    l.deny("granted_triggered_ability.once_per_turn", once_per_turn);
    l.deny("granted_triggered_ability.tier_threshold", tier_threshold);
    l.deny("granted_triggered_ability.room_half", room_half);
    l.deny("granted_triggered_ability.functions_in_graveyard", functions_in_graveyard);
    let dies_self = *event == Some(enums::Event::Dies)
        && matches!(predicate.as_deref(), Some(cats::TriggerPredicate::IsSelf(nodes::IsSelf { subject: None })));
    if !dies_self {
        l.unsupported("granted_triggered_ability(not \"when this creature dies\")");
        return;
    }
    let Some(e) = effect else {
        l.unsupported("granted_triggered_ability.effect(missing)");
        return;
    };
    let ab = super::effect::lower_ability(l, e);
    let k = l.delayed_ability(ab);
    eoe(b, EoeD::GrantDiesTriggerThisTurn { on, k });
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
        // "Destroy [up to one] [other] target creature [or Spacecraft]. [If
        // that creature wasn't dealt damage this turn, its controller draws
        // n cards.]" (Faller's Faithful, Vote Out, Embrace Oblivion; CR
        // 701.8a). The draw doesn't need the destruction (an indestructible
        // creature's controller draws too); an illegal target is neither
        // destroyed nor makes anyone draw (608.2b).
        E::DestroyTargetCreature(nodes::DestroyTargetCreature {
            targets,
            draw_if_undamaged_amount,
            or_spacecraft,
            or_enchantment,
            optional,
            exclude_self,
        }) => {
            let Some(t) = lower_targets(l, b, targets, "DestroyTargetCreature") else { return true };
            let spacecraft = Filter::Subtype(l.subtype("Spacecraft"));
            let req = b.targets[t as usize].clone();
            if *optional != (req.min == 0)
                || *exclude_self != mentions_not_source(&req.filter)
                || *or_spacecraft != mentions(&req.filter, &spacecraft)
                || *or_enchantment != mentions(&req.filter, &Filter::Type(types::ENCHANTMENT))
            {
                l.unsupported("DestroyTargetCreature(flags disagree with the targets)");
            }
            let n = l.int("DestroyTargetCreature.draw_if_undamaged_amount", *draw_if_undamaged_amount, 0, 20);
            if n == 0 {
                b.op(Op::Destroy { tgt: t });
                return true;
            }
            b.op(Op::Remember { tgt: t });
            let undamaged = b.op(Op::JumpUnlessTarget { tgt: t, filter: Filter::DealtDamageThisTurn, to: 0 });
            b.op(Op::Destroy { tgt: t });
            let end = b.op(Op::Jump { to: 0 });
            b.patch(undamaged);
            b.op(Op::Destroy { tgt: t });
            b.op(Op::Draw { who: Who::LkiController, n: Amount::Fixed(n) });
            b.patch(end);
        }
        // "Target creature deals damage to itself equal to its power. If
        // that creature has flying, it deals twice that much damage to itself
        // instead" (Cut Propulsion; CR 120.3).
        E::CutPropulsionEffect(nodes::CutPropulsionEffect {}) => {
            let t = b.target(creature(), 1, 1);
            eoe(b, EoeD::PowerDamageByTarget { by: t, to: None, twice_if: Some(Filter::HasKeyword(Kw::Flying)) });
        }
        // "Target creature you control gets +1/+0 and gains vigilance until
        // end of turn. It deals damage equal to its power to target creature
        // an opponent controls" (Diplomatic Relations).
        E::DiplomaticRelationsEffect(nodes::DiplomaticRelationsEffect {}) => {
            let mine = b.target(and(creature(), Filter::Controller(true)), 1, 1);
            let theirs = b.target(and(creature(), Filter::Controller(false)), 1, 1);
            apply(b, On::Target(mine), vec![Mod::Pt(1, 0), kws(&[Kw::Vigilance])], Until::EndOfTurn);
            eoe(b, EoeD::PowerDamageByTarget { by: mine, to: Some(theirs), twice_if: None });
        }
        // "<This> deals X damage to target attacking or blocking creature,
        // where X is n plus the number of creatures and/or Spacecraft you
        // control" (Focus Fire; counted as it resolves).
        E::FocusFireEffect(nodes::FocusFireEffect { base, targets }) => {
            let Some(t) = lower_targets(l, b, targets, "FocusFireEffect") else { return true };
            if b.targets[t as usize].filter != and(creature(), Filter::Or(vec![Filter::Attacking, Filter::Blocking])) {
                l.unsupported("FocusFireEffect.targets(not an attacking or blocking creature)");
            }
            let plus = l.int("FocusFireEffect.base", *base, 0, 99);
            let filter = Filter::Or(vec![creature(), Filter::Subtype(l.subtype("Spacecraft"))]);
            eoe(b, EoeD::CountControlled { filter, plus });
            b.op(Op::DealDamage { amount: Amount::Remembered, to: t });
        }
        // "Another target creature you control gets +X/+X until end of turn,
        // where X is this creature's power" (Hemosymbic Mite; as the ability
        // resolves, its last known power if it left, CR 608.2h).
        E::PumpTargetOtherCreatureByXOverXWhereXIsSourcePower(nodes::PumpTargetOtherCreatureByXOverXWhereXIsSourcePower {}) => {
            let t = b.target(and(and(creature(), Filter::Controller(true)), not(Filter::Source)), 1, 1);
            eoe(b, EoeD::ApplyPtTimes { on: On::Target(t), p: 1, t: 1, n: Amount::SourcePower, until: Until::EndOfTurn });
        }
        // "Double the power of target creature you control until end of
        // turn" (Mightform Harmonizer; CR 701.10b: it gets +X/+0, X its power
        // as the ability resolves).
        E::DoubleTargetCreaturesPowerUEOT(nodes::DoubleTargetCreaturesPowerUEOT {}) => {
            let t = b.target(and(creature(), Filter::Controller(true)), 1, 1);
            b.op(Op::Remember { tgt: t });
            eoe(b, EoeD::ApplyPtTimes { on: On::Target(t), p: 1, t: 0, n: Amount::LkiPower, until: Until::EndOfTurn });
        }
        // "Until end of turn, target creature you control gets +X/+0 and
        // target creature an opponent controls gets -0/-X, where X is the
        // number of artifacts you control" (Monoist Circuit-Feeder; the
        // class's two kinds are "a creature you control" and "a creature an
        // opponent controls", its docstring and legal targets).
        E::PairedTargetPumpDebuffUEOT(nodes::PairedTargetPumpDebuffUEOT {
            formula,
            positive_power,
            positive_toughness,
            negative_power,
            negative_toughness,
            positive_kind,
            negative_kind,
        }) => {
            if positive_kind != "my_creature" || negative_kind != "creature" {
                l.unsupported(format!("PairedTargetPumpDebuffUEOT.kinds({positive_kind}, {negative_kind})"));
            }
            let n = match formula.as_deref() {
                Some(cats::Formula::CountControlledMatching(nodes::CountControlledMatching { predicate })) => {
                    match super::filter::lower_pred(l, predicate) {
                        Some(Filter::Type(bits)) => Amount::CountTypes(bits),
                        _ => {
                            l.unsupported("PairedTargetPumpDebuffUEOT.formula(not a count of a card type)");
                            return true;
                        }
                    }
                }
                Some(other) => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("PairedTargetPumpDebuffUEOT.formula(missing)");
                    return true;
                }
            };
            let k = |l: &mut Lowerer, w: &str, v: i64| l.int(w, v, -9, 9) as i8;
            let (pp, pt) = (k(l, "PairedTargetPumpDebuffUEOT.positive_power", *positive_power), k(l, "PairedTargetPumpDebuffUEOT.positive_toughness", *positive_toughness));
            let (np, nt) = (k(l, "PairedTargetPumpDebuffUEOT.negative_power", *negative_power), k(l, "PairedTargetPumpDebuffUEOT.negative_toughness", *negative_toughness));
            let mine = b.target(and(creature(), Filter::Controller(true)), 1, 1);
            let theirs = b.target(and(creature(), Filter::Controller(false)), 1, 1);
            eoe(b, EoeD::ApplyPtTimes { on: On::Target(mine), p: pp, t: pt, n, until: Until::EndOfTurn });
            eoe(b, EoeD::ApplyPtTimes { on: On::Target(theirs), p: np, t: nt, n, until: Until::EndOfTurn });
        }
        // "Creatures your opponents control get +p/+t [until your next turn]"
        // (Mouth of the Storm: the creatures they control as it resolves,
        // CR 611.2c; "until your next turn", 611.2b).
        E::PumpCreaturesOpponentsControlUntilEOT(nodes::PumpCreaturesOpponentsControlUntilEOT {
            power_bonus,
            toughness_bonus,
            granted_keywords,
            until_caster_next_turn,
        }) => {
            let mut mods = Vec::new();
            let (p, t) = (
                l.int("PumpCreaturesOpponentsControlUntilEOT.power_bonus", *power_bonus, -99, 99),
                l.int("PumpCreaturesOpponentsControlUntilEOT.toughness_bonus", *toughness_bonus, -99, 99),
            );
            if p != 0 || t != 0 {
                mods.push(Mod::Pt(p, t));
            }
            mods.extend(keyword_names(l, "PumpCreaturesOpponentsControlUntilEOT.granted_keywords", granted_keywords));
            if mods.is_empty() {
                l.unsupported("pump(nothing)");
            }
            let until = if *until_caster_next_turn { Until::YourNextTurn } else { Until::EndOfTurn };
            apply(b, On::Each(and(creature(), Filter::Controller(false))), mods, until);
        }
        // "Return it to the battlefield tapped under its owner's control"
        // (the ability Perigee Beckoner grants, about the creature that
        // died).
        E::PerigeeBeckonerDiesReturnTappedEffect(nodes::PerigeeBeckonerDiesReturnTappedEffect {}) => {
            eoe(b, EoeD::ReturnEventCardTapped);
        }
        // "Your life total becomes half your starting life total, rounded
        // up" (The Endstone; CR 119.5).
        E::SetLifeToHalfStartingRoundedUp(nodes::SetLifeToHalfStartingRoundedUp {}) => {
            eoe(b, EoeD::LifeBecomesHalfStarting { who: Who::You });
        }
        // "Sacrifice it unless you tap an untapped permanent you control"
        // (Command Bridge).
        E::SacrificeSelfUnlessTapPermanent(nodes::SacrificeSelfUnlessTapPermanent {}) => {
            eoe(b, EoeD::SacrificeSourceUnlessTap { filter: Filter::Type(types::PERMANENT) });
        }
        // "It deals n damage to target opponent" (Biotech Specialist): the IR
        // writes the opponent as "a player controlled by an opponent".
        E::DealDamageToTargetOpponent(nodes::DealDamageToTargetOpponent { amount, targets }) => {
            let amount = Amount::Fixed(l.int("DealDamageToTargetOpponent.amount", *amount, 0, 99));
            let Some(t) = lower_targets(l, b, targets, "DealDamageToTargetOpponent") else { return true };
            let req = &mut b.targets[t as usize];
            if req.filter != and(Filter::Player, Filter::Controller(false)) || (req.min, req.max) != (1, 1) {
                l.unsupported("DealDamageToTargetOpponent.targets(not one target opponent)");
                return true;
            }
            req.filter = and(Filter::Player, Filter::Opponent);
            b.op(Op::DealDamage { amount, to: t });
        }
        // "Tap another target creature" (Starport Security; CR 701.26a).
        E::TapAnotherTargetCreature(nodes::TapAnotherTargetCreature { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "TapAnotherTargetCreature") else { return true };
            if !mentions_not_source(&b.targets[t as usize].filter) {
                l.unsupported("TapAnotherTargetCreature.targets(not another)");
            }
            b.op(Op::Ext(Ext::Tap { on: On::Target(t) }));
        }
        // "<This> deals n damage to target tapped creature" (Reroute
        // Systems).
        E::DealDamageToTargetTappedCreature(nodes::DealDamageToTargetTappedCreature { amount, targets }) => {
            let amount = Amount::Fixed(l.int("DealDamageToTargetTappedCreature.amount", *amount, 0, 99));
            let Some(t) = lower_targets(l, b, targets, "DealDamageToTargetTappedCreature") else { return true };
            if b.targets[t as usize].filter != and(creature(), Filter::Tapped) {
                l.unsupported("DealDamageToTargetTappedCreature.targets(not a tapped creature)");
            }
            b.op(Op::DealDamage { amount, to: t });
        }
        // "It deals damage equal to its power to any target" (Rust
        // Harvester; its power as the ability resolves, CR 608.2h).
        E::DealDamageEqualToSourcePowerAnyTarget(nodes::DealDamageEqualToSourcePowerAnyTarget { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "DealDamageEqualToSourcePowerAnyTarget") else { return true };
            b.op(Op::DealDamage { amount: Amount::SourcePower, to: t });
        }
        // "Return up to n target [non-Spacecraft] creatures to their owners'
        // hands" (Specimen Freighter).
        E::ReturnUpToNTargetCreaturesToOwnersHands(nodes::ReturnUpToNTargetCreaturesToOwnersHands { max_n, exclude_spacecraft }) => {
            let n = l.int("ReturnUpToNTargetCreaturesToOwnersHands.max_n", *max_n, 1, 8) as u8;
            let mut f = creature();
            if *exclude_spacecraft {
                f = and(f, not(Filter::Subtype(l.subtype("Spacecraft"))));
            }
            let t = b.target(f, 0, n);
            b.op(Op::ReturnToHand { tgt: t });
        }
        // "You may put a creature card from your hand onto the battlefield"
        // (Sledge-Class Seedship).
        E::OptionalPutCreatureFromHandOntoBattlefield(nodes::OptionalPutCreatureFromHandOntoBattlefield {}) => {
            b.op(Op::PutFromHand { who: Who::You, filter: creature(), tapped: false, optional: true });
        }
        // "Gain control of target Sliver. (This effect lasts indefinitely.)"
        // (Sliver Overlord; CR 613.1b).
        E::GainControlOfTargetSliverPermanently(nodes::GainControlOfTargetSliverPermanently {}) => {
            let t = b.target(Filter::Subtype(l.subtype("Sliver")), 1, 1);
            b.op(Op::Ext(Ext::GainControl { tgt: t }));
        }
        // Only a triggered ability's intervening "if"
        // (`hoist_intervening_if`).
        E::ConditionalIfYouControlAtLeast(nodes::ConditionalIfYouControlAtLeast { threshold, predicate, inner }) => {
            let _ = (threshold, predicate, inner);
            l.unsupported("ConditionalIfYouControlAtLeast(not a triggered ability's intervening if)");
        }
        _ => return false,
    }
    true
}

/// Does the filter name `x` somewhere (in an `And` or an `Or`)?
fn mentions(f: &Filter, x: &Filter) -> bool {
    f == x
        || match f {
            Filter::And(a, b) => mentions(a, x) || mentions(b, x),
            Filter::Or(v) => v.iter().any(|y| mentions(y, x)),
            _ => false,
        }
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
