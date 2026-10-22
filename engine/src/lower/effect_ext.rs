//! Effects lowered onto the `Ext` instructions (continuous effects,
//! counters, tapping, equipment, fights, milling). Same rules as
//! `effect.rs`: every field named, the rest denied.

use super::effect::{lower_targets, lower_effect};
use super::Lowerer;
use crate::db::{types, Counter, Filter, Kw, KwSet, Mod, Prot};
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, Who};
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

/// Keywords and protection from IR keywords (layer 6 grants).
pub(crate) fn keyword_mods(l: &mut Lowerer, kws: &[cats::Keyword]) -> Vec<Mod> {
    let mut set = KwSet::default();
    let mut prot = Prot::default();
    for k in kws {
        match super::printing::lower_keyword_full(l, k) {
            Some(super::printing::KwOrProt::Kw(kw)) => set.insert(kw),
            Some(super::printing::KwOrProt::Prot(p)) => prot.union(&p),
            // Granting a triggered keyword needs a granted trigger.
            Some(super::printing::KwOrProt::Exalted | super::printing::KwOrProt::BattleCry) => {
                l.unsupported("Keyword(granted trigger)")
            }
            None => {}
        }
    }
    let mut out = Vec::new();
    if !set.is_empty() {
        out.push(Mod::Keywords(set));
    }
    if !prot.is_empty() {
        out.push(Mod::Protection(prot));
    }
    out
}

/// "+p/+t and gains keywords".
fn pump_mods(l: &mut Lowerer, p: i64, t: i64, kws: &[cats::Keyword]) -> Vec<Mod> {
    let mut mods = Vec::new();
    if p != 0 || t != 0 {
        mods.push(Mod::Pt(l.int("pump.power", p, -99, 99), l.int("pump.toughness", t, -99, 99)));
    }
    mods.extend(keyword_mods(l, kws));
    if mods.is_empty() {
        l.unsupported("pump(nothing)");
    }
    mods
}

fn apply(b: &mut Builder, on: On, mods: Vec<Mod>, until: Until) {
    b.op(Op::Ext(Ext::Apply { on, mods, until }));
}

/// A keyword by its IR spelling (equipment bonuses list them as strings).
pub(crate) fn kw_by_name(s: &str) -> Option<Kw> {
    Some(match s {
        "flying" => Kw::Flying,
        "reach" => Kw::Reach,
        "first strike" => Kw::FirstStrike,
        "double strike" => Kw::DoubleStrike,
        "deathtouch" => Kw::Deathtouch,
        "trample" => Kw::Trample,
        "lifelink" => Kw::Lifelink,
        "vigilance" => Kw::Vigilance,
        "haste" => Kw::Haste,
        "defender" => Kw::Defender,
        "menace" => Kw::Menace,
        "flash" => Kw::Flash,
        "hexproof" => Kw::Hexproof,
        "shroud" => Kw::Shroud,
        "indestructible" => Kw::Indestructible,
        _ => return None,
    })
}

/// A named counter kind: keyword counters (CR 122.1b), stun counters,
/// the rest by interned name.
pub(crate) fn counter_by_name(l: &mut Lowerer, s: &str) -> Counter {
    match s {
        "+1/+1" => Counter::P1P1,
        "-1/-1" => Counter::M1M1,
        "stun" => Counter::Stun,
        _ => match Kw::from_counter_name(s) {
            Some(k) => Counter::Kw(k),
            None => Counter::Named(l.subtype(&format!("counter:{s}"))),
        },
    }
}

/// Lower an effect class onto `Ext` ops; `false` if the class is not one
/// of these.
pub(crate) fn lower_effect_ext(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        E::PumpTargetCreatureUntilEOT(nodes::PumpTargetCreatureUntilEOT {
            power_bonus,
            toughness_bonus,
            granted_keywords,
            you_control_only,
            opponents_only,
            granted_keywords_if_warp_cast,
            draw_if_target_has_plus_one_counter,
            target_subtype,
            granted_triggered_ability,
            optional,
            exclude_self,
            attacking_only,
            legendary_only,
            power_bonus_from_x,
        }) => {
            l.deny("PumpTargetCreatureUntilEOT.granted_keywords_if_warp_cast", granted_keywords_if_warp_cast);
            l.deny("PumpTargetCreatureUntilEOT.draw_if_target_has_plus_one_counter", draw_if_target_has_plus_one_counter);
            l.deny("PumpTargetCreatureUntilEOT.granted_triggered_ability", granted_triggered_ability);
            l.deny("PumpTargetCreatureUntilEOT.power_bonus_from_x", power_bonus_from_x);
            let mut f = creature();
            if *you_control_only {
                f = and(f, Filter::Controller(true));
            }
            if *opponents_only {
                f = and(f, Filter::Controller(false));
            }
            if let Some(s) = target_subtype {
                f = and(f, Filter::Subtype(l.subtype(s)));
            }
            if *exclude_self {
                f = and(f, not(Filter::Source));
            }
            if *attacking_only {
                f = and(f, Filter::Attacking);
            }
            if *legendary_only {
                f = and(f, Filter::Supertype(crate::db::supertypes::LEGENDARY));
            }
            let mods = pump_mods(l, *power_bonus, *toughness_bonus, granted_keywords);
            let t = b.target(f, if *optional { 0 } else { 1 }, 1);
            apply(b, On::Target(t), mods, Until::EndOfTurn);
        }
        E::PumpCreaturesYouControlUntilEOT(nodes::PumpCreaturesYouControlUntilEOT {
            power_bonus,
            toughness_bonus,
            granted_keywords,
            exclude_source,
            attacking_only,
            also_untap_if_not_your_turn,
            tokens_only,
        }) => {
            l.deny("PumpCreaturesYouControlUntilEOT.also_untap_if_not_your_turn", also_untap_if_not_your_turn);
            let mut f = and(creature(), Filter::Controller(true));
            if *exclude_source {
                f = and(f, not(Filter::Source));
            }
            if *attacking_only {
                f = and(f, Filter::Attacking);
            }
            if *tokens_only {
                f = and(f, Filter::Token);
            }
            let mods = pump_mods(l, *power_bonus, *toughness_bonus, granted_keywords);
            apply(b, On::Each(f), mods, Until::EndOfTurn);
        }
        E::PumpSelfUntilEOT(nodes::PumpSelfUntilEOT { power_bonus, toughness_bonus, granted_keywords }) => {
            let mods = pump_mods(l, *power_bonus, *toughness_bonus, granted_keywords);
            apply(b, On::Source, mods, Until::EndOfTurn);
        }
        E::PumpAllCreaturesUEOT(nodes::PumpAllCreaturesUEOT { power, toughness, x_power, x_toughness, exclude_source }) => {
            l.deny("PumpAllCreaturesUEOT.x_power", x_power);
            l.deny("PumpAllCreaturesUEOT.x_toughness", x_toughness);
            let mut f = creature();
            if *exclude_source {
                f = and(f, not(Filter::Source));
            }
            let mods = pump_mods(l, *power, *toughness, &[]);
            apply(b, On::Each(f), mods, Until::EndOfTurn);
        }
        E::PutPlusOneCounterOnSelf(nodes::PutPlusOneCounterOnSelf { amount }) => {
            let n = Amount::Fixed(l.int("PutPlusOneCounterOnSelf.amount", *amount, 0, 99));
            b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter: Counter::P1P1, n }));
        }
        E::PutPlusOneCounterOnTargetCreature(nodes::PutPlusOneCounterOnTargetCreature {
            amount,
            targets,
            attacking_only,
        }) => {
            let n = Amount::Fixed(l.int("PutPlusOneCounterOnTargetCreature.amount", *amount, 0, 99));
            let t = if targets.is_empty() {
                b.target(creature(), 1, 1)
            } else {
                match lower_targets(l, b, targets, "PutPlusOneCounterOnTargetCreature") {
                    Some(t) => t,
                    None => return true,
                }
            };
            if *attacking_only {
                let f = &mut b.targets[t as usize].filter;
                *f = and(f.clone(), Filter::Attacking);
            }
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n }));
        }
        E::PutPlusOneCounterOnCreatureYouControl(nodes::PutPlusOneCounterOnCreatureYouControl { amount, targets }) => {
            let n = Amount::Fixed(l.int("PutPlusOneCounterOnCreatureYouControl.amount", *amount, 0, 99));
            let t = if targets.is_empty() {
                b.target(and(creature(), Filter::Controller(true)), 1, 1)
            } else {
                match lower_targets(l, b, targets, "PutPlusOneCounterOnCreatureYouControl") {
                    Some(t) => t,
                    None => return true,
                }
            };
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n }));
        }
        E::PutPlusOneCounterOnEachCreatureYouControl(nodes::PutPlusOneCounterOnEachCreatureYouControl {
            amount,
            exclude_source,
        }) => {
            let n = Amount::Fixed(l.int("PutPlusOneCounterOnEachCreatureYouControl.amount", *amount, 0, 99));
            let mut f = and(creature(), Filter::Controller(true));
            if *exclude_source {
                f = and(f, not(Filter::Source));
            }
            b.op(Op::Ext(Ext::PutCounters { on: On::Each(f), counter: Counter::P1P1, n }));
        }
        E::PutNamedCountersOnSelf(nodes::PutNamedCountersOnSelf { kind, amount }) => {
            let counter = counter_by_name(l, kind);
            let n = Amount::Fixed(l.int("PutNamedCountersOnSelf.amount", *amount, 0, 99));
            b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter, n }));
        }
        E::GrantProtectionOfChoiceUEOT(nodes::GrantProtectionOfChoiceUEOT {
            you_control_only,
            exclude_self,
            allow_colorless,
        }) => {
            let mut f = creature();
            if *you_control_only {
                f = and(f, Filter::Controller(true));
            }
            if *exclude_self {
                f = and(f, not(Filter::Source));
            }
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::ProtectionChoice { on: On::Target(t), colorless: *allow_colorless, until: Until::EndOfTurn }));
        }
        E::TargetCreatureCantBlockThisTurn(nodes::TargetCreatureCantBlockThisTurn { optional, targets }) => {
            let t = if targets.is_empty() {
                b.target(creature(), if *optional { 0 } else { 1 }, 1)
            } else {
                match lower_targets(l, b, targets, "TargetCreatureCantBlockThisTurn") {
                    Some(t) => t,
                    None => return true,
                }
            };
            if *optional {
                b.targets[t as usize].min = 0;
            }
            apply(b, On::Target(t), vec![Mod::CantBlock], Until::EndOfTurn);
        }
        E::CreaturesCantBlockThisTurn(nodes::CreaturesCantBlockThisTurn { target_player }) => {
            // "Creatures [target player controls] can't block this turn": a
            // rules effect, so creatures that arrive later are affected too
            // (CR 611.2c does not lock it).
            if *target_player {
                let t = b.target(Filter::Player, 1, 1);
                let filter = and(creature(), Filter::Controller(true));
                b.op(Op::Ext(Ext::ApplyRules { filter, who: Who::TargetPlayer(t), m: Mod::CantBlock, until: Until::EndOfTurn }));
            } else {
                b.op(Op::Ext(Ext::ApplyRules { filter: creature(), who: Who::You, m: Mod::CantBlock, until: Until::EndOfTurn }));
            }
        }
        E::GrantUnblockableUEOT(nodes::GrantUnblockableUEOT { targets, attacking_only, exclude_self, power_at_most }) => {
            let Some(t) = lower_targets(l, b, targets, "GrantUnblockableUEOT") else { return true };
            let mut extra = Vec::new();
            if *attacking_only {
                extra.push(Filter::Attacking);
            }
            if *exclude_self {
                extra.push(not(Filter::Source));
            }
            if let Some(n) = power_at_most {
                extra.push(Filter::PowerAtMost(l.int("GrantUnblockableUEOT.power_at_most", *n, -99, 99)));
            }
            for x in extra {
                let f = &mut b.targets[t as usize].filter;
                *f = and(f.clone(), x);
            }
            apply(b, On::Target(t), vec![Mod::CantBeBlocked], Until::EndOfTurn);
        }
        E::SourceCantBeBlockedThisTurn(nodes::SourceCantBeBlockedThisTurn {}) => {
            apply(b, On::Source, vec![Mod::CantBeBlocked], Until::EndOfTurn);
        }
        E::Tap(nodes::Tap { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "Tap") else { return true };
            b.op(Op::Ext(Ext::Tap { on: On::Target(t) }));
        }
        E::TapSelf(nodes::TapSelf {}) => {
            b.op(Op::Ext(Ext::Tap { on: On::Source }));
        }
        E::UntapSelf(nodes::UntapSelf {}) => {
            b.op(Op::Ext(Ext::Untap { on: On::Source }));
        }
        E::EquipToTargetCreatureYouControl(nodes::EquipToTargetCreatureYouControl { granted_keywords_ueot }) => {
            // Equip (CR 702.6a): "Attach to target creature you control."
            l.deny("EquipToTargetCreatureYouControl.granted_keywords_ueot", granted_keywords_ueot);
            let t = b.target(and(creature(), Filter::Controller(true)), 1, 1);
            b.op(Op::Ext(Ext::Attach { tgt: t }));
        }
        E::SourceFightsTarget(nodes::SourceFightsTarget { optional, other, targets, np_filter, exile_if_dies_this_turn }) => {
            l.deny("SourceFightsTarget.np_filter", np_filter);
            l.deny("SourceFightsTarget.exile_if_dies_this_turn", exile_if_dies_this_turn);
            let Some(t) = lower_targets(l, b, targets, "SourceFightsTarget") else { return true };
            let req = &b.targets[t as usize];
            if *optional && req.min != 0 {
                l.unsupported("SourceFightsTarget.optional(with a required target)");
            }
            if *other && !matches!(&req.filter, Filter::And(_, x) if **x == not(Filter::Source)) {
                l.unsupported("SourceFightsTarget.other(target may be the source)");
            }
            b.op(Op::Ext(Ext::Fight { tgt: t }));
        }
        E::SelfMill(nodes::SelfMill { amount }) => {
            let n = Amount::Fixed(l.int("SelfMill.amount", *amount, 0, 99));
            b.op(Op::Ext(Ext::Mill { who: Who::You, n }));
        }
        E::RevealTopCardToHandLoseLifeEqualMV(nodes::RevealTopCardToHandLoseLifeEqualMV { unless_saddled }) => {
            l.deny("RevealTopCardToHandLoseLifeEqualMV.unless_saddled", unless_saddled);
            b.op(Op::Ext(Ext::RevealTopToHand { who: Who::You }));
            b.op(Op::LoseLife { who: Who::You, n: Amount::LkiManaValue });
        }
        E::EachOpponentLosesLife(nodes::EachOpponentLosesLife { amount }) => {
            let n = Amount::Fixed(l.int("EachOpponentLosesLife.amount", *amount, 0, 99));
            b.op(Op::LoseLife { who: Who::Opponent, n });
        }
        E::BindEventPlayer(nodes::BindEventPlayer { inner, slot }) => {
            // "That player ...": the inner effect's target player is the
            // event's player (no targeting).
            if *slot != enums::EventSubject::Player {
                l.unsupported(format!("BindEventPlayer.slot({slot:?})"));
            }
            match &**inner {
                E::TargetPlayerLosesLife(nodes::TargetPlayerLosesLife { amount }) => {
                    let n = Amount::Fixed(l.int("TargetPlayerLosesLife.amount", *amount, 0, 99));
                    b.op(Op::LoseLife { who: Who::EventPlayer, n });
                }
                other => l.unsupported(format!("BindEventPlayer.inner({})", other.tag())),
            }
        }
        E::BecomeLandCreatureUEOT(nodes::BecomeLandCreatureUEOT {
            power,
            toughness,
            granted_keywords,
            unblockable,
            granted_subtype,
            granted_activated_abilities,
            granted_triggered_abilities,
            set_colors,
        }) => {
            // "Until end of turn, this land becomes a p/t [colours]
            // [subtype] creature with [keywords]. It's still a land." (CR
            // 205.1b: it keeps its land types.)
            l.deny("BecomeLandCreatureUEOT.granted_activated_abilities", granted_activated_abilities);
            l.deny("BecomeLandCreatureUEOT.granted_triggered_abilities", granted_triggered_abilities);
            if set_colors.is_empty() && l.land_colors != 0 {
                // A land that makes coloured mana animates into a coloured
                // creature in the cube; the IR lost the colours and the
                // creature type (Celestial Colonnade: "white and blue
                // Elemental"). An IR defect (ENGINE.md 20).
                l.unsupported("BecomeLandCreatureUEOT.set_colors(missing for a coloured land)");
            }
            let subtypes = granted_subtype.iter().map(|s| l.subtype(s)).collect();
            let mut mods = vec![Mod::Types { add: types::CREATURE, subtypes, replace: false }];
            if !set_colors.is_empty() {
                let mut m = 0;
                for c in set_colors {
                    match crate::mana::letter_index(c) {
                        Some(i) if i < 5 => m |= 1 << i,
                        _ => l.unsupported(format!("BecomeLandCreatureUEOT.set_colors({c})")),
                    }
                }
                mods.push(Mod::Colors(m));
            }
            mods.push(Mod::SetPt(
                l.int("BecomeLandCreatureUEOT.power", *power, 0, 99),
                l.int("BecomeLandCreatureUEOT.toughness", *toughness, 0, 99),
            ));
            mods.extend(keyword_mods(l, granted_keywords));
            if *unblockable {
                mods.push(Mod::CantBeBlocked);
            }
            apply(b, On::Source, mods, Until::EndOfTurn);
        }
        E::SourceBecomesIfSubtype(nodes::SourceBecomesIfSubtype { subtypes, base_pt, keywords, requires_subtype }) => {
            // "[If this creature is a <requires>,] it becomes a <subtypes>
            // with base power and toughness p/t [and keywords]": its
            // creature types are replaced (CR 205.1b), no duration.
            let at = if requires_subtype.is_empty() {
                None
            } else {
                let s = l.subtype(requires_subtype);
                Some(b.op(Op::Ext(Ext::JumpUnlessSource { filter: Filter::Subtype(s), to: 0 })))
            };
            let subs = subtypes.iter().map(|s| l.subtype(s)).collect();
            let mut mods = vec![Mod::Types { add: 0, subtypes: subs, replace: true }];
            match base_pt[..] {
                [p, t] => mods.push(Mod::SetPt(
                    l.int("SourceBecomesIfSubtype.base_pt", p, 0, 99),
                    l.int("SourceBecomesIfSubtype.base_pt", t, 0, 99),
                )),
                _ => l.unsupported("SourceBecomesIfSubtype.base_pt(shape)"),
            }
            mods.extend(keyword_mods(l, keywords));
            apply(b, On::Source, mods, Until::Forever);
            if let Some(at) = at {
                b.patch(at);
            }
        }
        E::SetBasePTUntilEOT(nodes::SetBasePTUntilEOT { power, toughness, optional, exclude_self }) => {
            let mut f = creature();
            if *exclude_self {
                f = and(f, not(Filter::Source));
            }
            let t = b.target(f, if *optional { 0 } else { 1 }, 1);
            let m = Mod::SetPt(
                l.int("SetBasePTUntilEOT.power", *power, 0, 99),
                l.int("SetBasePTUntilEOT.toughness", *toughness, 0, 99),
            );
            apply(b, On::Target(t), vec![m], Until::EndOfTurn);
        }
        E::IfYouDo(nodes::IfYouDo { do_action, then_action }) => {
            // "You may <do>. If you do, <then>": an optional action whose
            // effect is all-or-nothing here (a discard needs a card).
            match &**do_action {
                E::DiscardSelf(nodes::DiscardSelf { amount }) if *amount == 1 => {
                    let at = b.op(Op::Ext(Ext::MayDiscard { skip: 0 }));
                    lower_effect(l, b, then_action);
                    b.patch(at);
                }
                other => l.unsupported(format!("IfYouDo.do_action({})", other.tag())),
            }
        }
        _ => return false,
    }
    true
}
