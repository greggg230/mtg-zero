//! Effects lowered onto the `Ext` instructions (continuous effects,
//! counters, tapping, equipment, fights, milling). Same rules as
//! `effect.rs`: every field named, the rest denied.

use super::effect::{lower_targets, lower_effect};
use super::Lowerer;
use crate::db::{types, Counter, Filter, Kw, KwSet, Mod, Prot};
use crate::layers::Until;
use crate::db::DelayedWhen;
use crate::program::{About, Amount, Builder, ChooseAct, Ext, On, Op, Who};
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
            Some(
                super::printing::KwOrProt::Exalted
                | super::printing::KwOrProt::BattleCry
                | super::printing::KwOrProt::Prowess
                | super::printing::KwOrProt::Persist
                | super::printing::KwOrProt::Cascade
                | super::printing::KwOrProt::Ascend
                | super::printing::KwOrProt::Annihilator(_)
                | super::printing::KwOrProt::Devoid,
            ) => {
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

/// The IR printing of a predefined token named by an effect ("creates
/// two Map tokens"), as the IR writes it where a card spells it out.
const MAP_TOKEN: &str = r#"{"$t": "CardPrinting", "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 1}, "mana_str": "{1}", "sacrifice_self": true, "tap_self": true}, "effect": {"$t": "Explore", "target": true}, "sorcery_speed": true}], "card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Map", "subtypes": ["Map"], "supertypes": ["Token"], "type_line": "Token Artifact \u2014 Map"}"#;

/// The Incubator token (CR 701.53a incubate): a double-faced token, an
/// artifact with "{2}: Transform this artifact" whose back face is a 0/0
/// colorless Phyrexian artifact creature named Phyrexian Token (CR 111.? /
/// 712: a transforming double-faced token).
const INCUBATOR_TOKEN: &str = r#"{"$t": "CardPrinting", "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 2}, "mana_str": "{2}"}, "effect": {"$t": "TransformSource"}}], "card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Incubator", "subtypes": ["Incubator"], "supertypes": ["Token"], "type_line": "Token Artifact — Incubator", "transform_back_face": {"$t": "CardPrinting", "card_types": ["Artifact", "Creature"], "face_colors": [], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Phyrexian Token", "power": 0, "subtypes": ["Phyrexian"], "supertypes": ["Token"], "toughness": 0, "type_line": "Token Artifact Creature — Phyrexian"}}"#;

/// A token's back face, while lowering: its index among the faces of the
/// card being lowered, tagged (`CardDb::from_cards` places it).
pub(crate) const TOKEN_FACE: u16 = 0x8000;

/// A predefined token by name (CR 111.10).
pub(crate) fn named_token(l: &mut Lowerer, name: &str) -> Option<u16> {
    let json = match name {
        "Map" => MAP_TOKEN,
        "Incubator" => INCUBATOR_TOKEN,
        other => {
            l.unsupported(format!("token({other})"));
            return None;
        }
    };
    let p: cats::CardPrinting = serde_json::from_str(json).expect("predefined token printing");
    let cats::CardPrinting::CardPrinting(tp) = &p;
    let mut def = super::printing::lower_printing(l, tp);
    // A double-faced token: its back face is among the card's faces.
    def.back = def.back.map(|k| k | TOKEN_FACE);
    Some(l.token(def))
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
        "lore" => Counter::Lore,
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
            if let Some(ta) = granted_triggered_ability.as_deref() {
                // "... and gains '<triggered ability>'" (EoE lane D,
                // ENGINE.md 99.5).
                super::effect_eoe_d::grant_trigger_this_turn(l, b, On::Target(t), ta);
            }
            // "If it has a +1/+1 counter on it, draw a card" (ENGINE.md 99.2).
            if *draw_if_target_has_plus_one_counter != 0 {
                super::eoe_a::draw_if_target_has_counter(l, b, t, *draw_if_target_has_plus_one_counter);
            }
            // "If that creature was cast for its warp cost, it also gains
            // <keywords> until end of turn" (ENGINE.md 99.8).
            if !granted_keywords_if_warp_cast.is_empty() {
                super::eoe_x::keywords_if_cast_for_warp(l, b, t, granted_keywords_if_warp_cast);
            }
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
            apply(b, On::Each(f.clone()), mods, Until::EndOfTurn);
            if *also_untap_if_not_your_turn {
                super::effect_eoe_d::untap_unless_your_turn(b, f);
            }
        }
        E::PumpSelfUntilEOT(nodes::PumpSelfUntilEOT { power_bonus, toughness_bonus, granted_keywords }) => {
            let mods = pump_mods(l, *power_bonus, *toughness_bonus, granted_keywords);
            apply(b, On::Source, mods, Until::EndOfTurn);
        }
        E::PumpAllCreaturesUEOT(nodes::PumpAllCreaturesUEOT { power, toughness, x_power, x_toughness, exclude_source }) => {
            let mut f = creature();
            if *exclude_source {
                f = and(f, not(Filter::Source));
            }
            if *x_power != 0 || *x_toughness != 0 {
                // "All creatures get -X/-X until end of turn" (Toxic Deluge:
                // the spell's X, locked in as it resolves, CR 608.2h).
                if !l.spell_x || *power != 0 || *toughness != 0 {
                    l.unsupported("PumpAllCreaturesUEOT.x_power(not the spell's X alone)");
                }
                let p = l.int("PumpAllCreaturesUEOT.x_power", *x_power, -9, 9) as i8;
                let t = l.int("PumpAllCreaturesUEOT.x_toughness", *x_toughness, -9, 9) as i8;
                b.op(Op::Ext(Ext::ApplyPtX { on: On::Each(f), p, t }));
                return true;
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
            let Some(t) = lower_targets(l, b, targets, "SourceFightsTarget") else { return true };
            // The IR repeats the target filter (`np_filter`); they must agree
            // (ENGINE.md 3, rule 4a).
            if let Some(p) = np_filter {
                if super::filter::lower_pred(l, p).as_ref() != Some(&b.targets[t as usize].filter) {
                    l.unsupported("SourceFightsTarget.np_filter(differs from targets)");
                }
            }
            let req = &b.targets[t as usize];
            if *optional && req.min != 0 {
                l.unsupported("SourceFightsTarget.optional(with a required target)");
            }
            if *other && !matches!(&req.filter, Filter::And(_, x) if **x == not(Filter::Source)) {
                l.unsupported("SourceFightsTarget.other(target may be the source)");
            }
            b.op(Op::Ext(Ext::Fight { tgt: t }));
            // "If that creature would die this turn, exile it instead"
            // (Mawloc: a replacement effect the resolution creates, CR 614,
            // whether or not the fight happened).
            if *exile_if_dies_this_turn {
                apply(b, On::Target(t), vec![Mod::ExileIfDies], Until::EndOfTurn);
            }
        }
        E::SelfMill(nodes::SelfMill { amount }) => {
            let n = Amount::Fixed(l.int("SelfMill.amount", *amount, 0, 99));
            b.op(Op::Ext(Ext::Mill { who: Who::You, n }));
        }
        E::RevealTopCardToHandLoseLifeEqualMV(nodes::RevealTopCardToHandLoseLifeEqualMV { unless_saddled }) => {
            b.op(Op::Ext(Ext::RevealTopToHand { who: Who::You }));
            if *unless_saddled {
                // "You lose life equal to that card's mana value if this
                // creature isn't saddled. Otherwise, each opponent loses that
                // much life" (Caustic Bronco; saddled as it resolves).
                let saddled = b.op(Op::Ext(Ext::JumpUnlessSource { filter: Filter::Saddled, to: 0 }));
                b.op(Op::LoseLife { who: Who::Opponent, n: Amount::LkiManaValue });
                let end = b.op(Op::Jump { to: 0 });
                b.patch(saddled);
                b.op(Op::LoseLife { who: Who::You, n: Amount::LkiManaValue });
                b.patch(end);
            } else {
                b.op(Op::LoseLife { who: Who::You, n: Amount::LkiManaValue });
            }
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
            granted_card_types,
            granted_subtypes,
            granted_activated_abilities,
            granted_triggered_abilities,
            set_colors,
            all_creature_types,
        }) => {
            // "Until end of turn, this land becomes a p/t [colours]
            // [subtype] creature with [keywords]. It's still a land." (CR
            // 205.1b: it keeps its land types.)
            l.deny("BecomeLandCreatureUEOT.granted_triggered_abilities", granted_triggered_abilities);
            if set_colors.is_empty() && l.land_colors != 0 {
                // A land that makes coloured mana animates into a coloured
                // creature in the cube; the IR lost the colours and the
                // creature type (Celestial Colonnade: "white and blue
                // Elemental"). An IR defect (ENGINE.md 20).
                l.unsupported("BecomeLandCreatureUEOT.set_colors(missing for a coloured land)");
            }
            // "a 1/1 Phyrexian Blinkmoth artifact creature" (Inkmoth Nexus):
            // the card types and creature types it gains beside creature.
            let mut subtypes: Vec<u16> = granted_subtype.iter().map(|s| l.subtype(s)).collect();
            for s in granted_subtypes {
                let k = l.subtype(s);
                if !subtypes.contains(&k) {
                    subtypes.push(k);
                }
            }
            let mut add = types::CREATURE;
            for t in granted_card_types {
                match types::from_name(t) {
                    Some(b) => add |= b,
                    None => l.unsupported(format!("BecomeLandCreatureUEOT.granted_card_types({t})")),
                }
            }
            let mut mods = vec![Mod::Types { add, subtypes, replace: false }];
            // "A 2/2 creature with all creature types" (Mutavault; CR 205.3m,
            // ENGINE.md 99.11).
            if *all_creature_types {
                mods.push(Mod::AllCreatureTypes);
            }
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
            // '... creature with "<ability>"' (Lavaclaw Reaches, Wandering
            // Fumarole; ENGINE.md 99.3): latent activated abilities of the
            // card it has until end of turn (layer 6, as `GainLatent`).
            for a in granted_activated_abilities {
                if let Some(ad) = super::activated::lower_activated(l, a, super::activated::From::Battlefield, false) {
                    l.latent.push(ad);
                    mods.push(Mod::GainLatent((l.latent.len() - 1) as u8));
                }
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
            // "[You may] <do>. If you do, <then>": the action happens or
            // not (a discard needs a card); `then` only if it did.
            let (inner, optional) = match &**do_action {
                E::MayDo(nodes::MayDo { inner, prompt }) => {
                    let _ = prompt; // display text
                    (&**inner, true)
                }
                other => (other, false),
            };
            match inner {
                E::DiscardSelf(nodes::DiscardSelf { amount }) if *amount == 1 => {
                    let at = b.op(Op::Ext(Ext::DiscardThen { skip: 0, optional }));
                    lower_effect(l, b, then_action);
                    b.patch(at);
                }
                E::DrawCards(nodes::DrawCards { amount }) if optional => {
                    // "You may draw n. If you do, ..."
                    let n = l.int("DrawCards.amount", *amount, 1, 9);
                    let at = b.op(Op::May { who: Who::You, skip: 0 });
                    b.op(Op::Draw { who: Who::You, n: Amount::Fixed(n) });
                    lower_effect(l, b, then_action);
                    b.patch(at);
                }
                E::SacrificeSelf(nodes::SacrificeSelf {}) if optional => {
                    // "You may sacrifice this. If you do, ..."
                    let at = b.op(Op::May { who: Who::You, skip: 0 });
                    let gone = b.op(Op::Ext(Ext::SacrificeSourceThen { skip: 0 }));
                    lower_effect(l, b, then_action);
                    b.patch(at);
                    b.patch(gone);
                }
                E::SacrificeAPermanent(nodes::SacrificeAPermanent { permanent_filter, another, label }) => {
                    // "[You may] sacrifice [another] <filter>. If you do, ..."
                    let mut f = match permanent_filter {
                        Some(p) => match super::filter::lower_pred(l, p) {
                            Some(f) => f,
                            None => return true,
                        },
                        None => Filter::Type(types::PERMANENT),
                    };
                    if *another {
                        // "Another creature or an artifact" (Gut): "another"
                        // belongs to the first noun phrase only, an artifact
                        // may be the source itself (ruling 06cb2c7b). The IR
                        // puts `another` on the whole `ObjectOr`; its label
                        // tells the two apart (docs/ir_defects.md 19).
                        let split = label.contains(" or a ") || label.contains(" or an ");
                        f = match f {
                            Filter::Or(mut parts) if split && parts.len() == 2 => {
                                parts[0] = and(parts[0].clone(), not(Filter::Source));
                                Filter::Or(parts)
                            }
                            f => and(f, not(Filter::Source)),
                        };
                    }
                    let at = b.op(Op::Ext(Ext::SacrificeThen { filter: f, optional, skip: 0 }));
                    lower_effect(l, b, then_action);
                    b.patch(at);
                }
                other => l.unsupported(format!("IfYouDo.do_action({}{})", if optional { "MayDo:" } else { "" }, other.tag())),
            }
        }
        E::PutPlusOneCounterOnEachOfUpToNTargetCreatures(nodes::PutPlusOneCounterOnEachOfUpToNTargetCreatures { max_n }) => {
            let n = l.int("PutPlusOneCounterOnEachOfUpToNTargetCreatures.max_n", *max_n, 1, 8) as u8;
            let t = b.target(creature(), 0, n);
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(1) }));
        }
        E::PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower(
            nodes::PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower {},
        ) => {
            // X is the source's power as the ability resolves, once (608.2h).
            let f = and(creature(), Filter::Controller(true));
            b.op(Op::Ext(Ext::PutCounters { on: On::Each(f), counter: Counter::P1P1, n: Amount::SourcePower }));
        }
        E::DoublePlusOneCountersOnEach(nodes::DoublePlusOneCountersOnEach { permanent_filter }) => {
            let Some(p) = permanent_filter else {
                l.unsupported("DoublePlusOneCountersOnEach.permanent_filter(missing)");
                return true;
            };
            let Some(f) = super::filter::lower_pred(l, p) else { return true };
            b.op(Op::Ext(Ext::DoubleCounters { on: On::Each(f), counter: Counter::P1P1 }));
        }
        E::PutStunCounterOnTappedCreature(nodes::PutStunCounterOnTappedCreature {}) => {
            // "Put a stun counter on up to one target tapped creature."
            let t = b.target(and(creature(), Filter::Tapped), 0, 1);
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::Stun, n: Amount::Fixed(1) }));
        }
        E::UntapUpToNPermanents(nodes::UntapUpToNPermanents { n, permanent_filter }) => {
            // "Untap up to n <permanents>" (not targeted). Untapped ones are
            // not offered: choosing one does nothing (ENGINE.md 13).
            let n = l.int("UntapUpToNPermanents.n", *n, 1, 8) as u8;
            let f = match permanent_filter {
                Some(p) => match super::filter::lower_pred(l, p) {
                    Some(f) => f,
                    None => return true,
                },
                None => Filter::Type(types::PERMANENT),
            };
            let filter = and(f, Filter::Tapped);
            b.op(Op::Ext(Ext::Choose { who: Who::You, filter, min: 0, max: n, act: ChooseAct::Untap }));
        }
        E::EachOpponentChoiceDestroyOneAtRandom(nodes::EachOpponentChoiceDestroyOneAtRandom {}) => {
            // "For each opponent, choose a nonland permanent that player
            // controls. Destroy one of them chosen at random": with one
            // opponent, the chosen one (not targeted).
            let filter = and(not(Filter::Type(types::LAND)), Filter::Controller(false));
            b.op(Op::Ext(Ext::Choose { who: Who::You, filter, min: 1, max: 1, act: ChooseAct::Destroy }));
        }
        E::TargetPlayerPerformsActions(nodes::TargetPlayerPerformsActions { actions, opponent_only }) => {
            // "Target opponent sacrifices ..., discards ..., and loses ...",
            // in printed order.
            let pf = if *opponent_only { and(Filter::Player, Filter::Opponent) } else { Filter::Player };
            let t = b.target(pf, 1, 1);
            let who = Who::TargetPlayer(t);
            for (verb, arg) in actions {
                use card_ir::unions::IntOrCardFilter as A;
                match (verb.as_str(), arg) {
                    ("sacrifice", A::CardFilter(cf)) => {
                        let Some(filter) = super::filter::lower_card_filter(l, cf) else { continue };
                        b.op(Op::Sacrifice { who, filter });
                    }
                    ("discard", A::Int(n)) => {
                        let n = l.int("TargetPlayerPerformsActions.discard", *n, 1, 9) as u8;
                        b.op(Op::Discard { who, n });
                    }
                    ("lose_life", A::Int(n)) => {
                        let n = Amount::Fixed(l.int("TargetPlayerPerformsActions.lose_life", *n, 0, 99));
                        b.op(Op::LoseLife { who, n });
                    }
                    (v, _) => l.unsupported(format!("TargetPlayerPerformsActions.actions({v})")),
                }
            }
        }
        E::FlickerUpToOneOtherTargetNonlandPermanentYouControl(
            nodes::FlickerUpToOneOtherTargetNonlandPermanentYouControl { other },
        ) => {
            let mut f = and(and(Filter::Type(types::PERMANENT), not(Filter::Type(types::LAND))), Filter::Controller(true));
            if *other {
                f = and(f, not(Filter::Source));
            }
            let t = b.target(f, 0, 1);
            b.op(Op::Ext(Ext::Flicker { tgt: t }));
        }
        E::CreateTokenThenMayAttachSelf(nodes::CreateTokenThenMayAttachSelf { create }) => {
            if !matches!(**create, E::CreateToken(_)) {
                l.unsupported(format!("CreateTokenThenMayAttachSelf.create({})", create.tag()));
                return true;
            }
            lower_effect(l, b, create);
            b.op(Op::Ext(Ext::MayAttachToMade));
        }
        E::FlipCoinIfLost(nodes::FlipCoinIfLost { on_loss }) => {
            let at = b.op(Op::Ext(Ext::FlipCoin { won: 0 }));
            match &**on_loss {
                E::SourceDealsDamageToController(nodes::SourceDealsDamageToController { amount }) => {
                    let amount = Amount::Fixed(l.int("SourceDealsDamageToController.amount", *amount, 0, 99));
                    b.op(Op::DamagePlayer { amount, who: Who::You });
                }
                other => lower_effect(l, b, other),
            }
            b.patch(at);
        }
        E::Explore(nodes::Explore { target }) => {
            let on = if *target { On::Target(b.target(and(creature(), Filter::Controller(true)), 1, 1)) } else { On::Source };
            b.op(Op::Ext(Ext::Explore { on }));
        }
        E::ReturnCardFromGraveyardToBattlefield(nodes::ReturnCardFromGraveyardToBattlefield {
            card_filter,
            source: enums::ReanimateSource::TopOfYourGraveyard,
            haste,
            exile_at_next_end_step,
            lose_life_equal_mv,
            with_counter,
            exile_if_it_would_leave,
            up_to_one,
        }) => {
            // "Return the top creature card of your graveyard to the
            // battlefield. That creature gains haste until end of turn.
            // Exile it at the beginning of the next end step." (Shallow
            // Grave: not targeted)
            l.deny("ReturnCardFromGraveyardToBattlefield.lose_life_equal_mv", lose_life_equal_mv);
            l.deny("ReturnCardFromGraveyardToBattlefield.with_counter", with_counter);
            l.deny("ReturnCardFromGraveyardToBattlefield.exile_if_it_would_leave", exile_if_it_would_leave);
            l.deny("ReturnCardFromGraveyardToBattlefield.up_to_one", up_to_one);
            let Some(cf) = super::filter::lower_card_filter(l, card_filter) else { return true };
            b.op(Op::Ext(Ext::TopOfGraveyardToBattlefield { filter: cf }));
            match haste {
                None => {}
                Some(h) => {
                    let mut kw = KwSet::default();
                    kw.insert(Kw::Haste);
                    let until = match h {
                        enums::HasteRider::UntilEndOfTurn => Until::EndOfTurn,
                        enums::HasteRider::Permanent => Until::Forever,
                    };
                    apply(b, On::Made, vec![Mod::Keywords(kw)], until);
                }
            }
            if *exile_at_next_end_step {
                let mut d = Builder::default();
                d.op(Op::Ext(Ext::ExileEventObj));
                let k = l.delayed_ability(d.finish());
                b.op(Op::Ext(Ext::Delay { k, when: DelayedWhen::NextEndStep, about: About::Made }));
            }
        }
        E::ReturnCardFromGraveyardToBattlefield(nodes::ReturnCardFromGraveyardToBattlefield {
            card_filter,
            source,
            haste,
            exile_at_next_end_step,
            lose_life_equal_mv,
            with_counter,
            exile_if_it_would_leave,
            up_to_one,
        }) => {
            // "Put/return target <filter> card from a graveyard / your
            // graveyard onto the battlefield [under your control] [with a
            // <kind> counter on it]. [If that creature would leave the
            // battlefield, exile it instead of putting it anywhere else.]"
            let Some(cf) = super::filter::lower_card_filter(l, card_filter) else { return true };
            let (inner, yours) = match source {
                enums::ReanimateSource::TargetAnyGraveyard => (cf, true),
                enums::ReanimateSource::TargetYourGraveyard => (and(cf, Filter::Owner(true)), true),
                other => {
                    l.unsupported(format!("ReturnCardFromGraveyardToBattlefield.source({other:?})"));
                    return true;
                }
            };
            let t = b.target(Filter::InGraveyard(Box::new(inner)), if *up_to_one { 0 } else { 1 }, 1);
            // "With a <kind> counter on it": as it enters (CR 614.1c).
            let counter = with_counter.as_ref().map(|name| counter_by_name(l, name));
            b.op(Op::Ext(Ext::FromGraveyard { tgt: t, to_battlefield: true, yours, tapped: false, counter }));
            // "That creature gains haste. Exile it at the beginning of the
            // next end step" (Goryo's Vengeance).
            if let Some(h) = haste {
                let mut kw = KwSet::default();
                kw.insert(Kw::Haste);
                let until = match h {
                    enums::HasteRider::UntilEndOfTurn => Until::EndOfTurn,
                    enums::HasteRider::Permanent => Until::Forever,
                };
                apply(b, On::Made, vec![Mod::Keywords(kw)], until);
            }
            if *exile_at_next_end_step {
                let mut d = Builder::default();
                d.op(Op::Ext(Ext::ExileEventObj));
                let k = l.delayed_ability(d.finish());
                b.op(Op::Ext(Ext::Delay { k, when: DelayedWhen::NextEndStep, about: About::Made }));
            }
            if *exile_if_it_would_leave {
                // A replacement effect on that permanent (CR 614.1a).
                apply(b, On::Made, vec![Mod::ExileIfLeaves], Until::Forever);
            }
            if *lose_life_equal_mv {
                b.op(Op::LoseLife { who: Who::You, n: Amount::LkiManaValue });
            }
        }
        // "Return target land card from your graveyard to your hand"
        // (Petrified Field; ENGINE.md 99.3): the same node as the others.
        E::ReturnTargetLandFromGraveyardToHand(nodes::ReturnTargetLandFromGraveyardToHand {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        })
        | E::ReturnCreatureFromYourGraveyardToBattlefield(nodes::ReturnCreatureFromYourGraveyardToBattlefield {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        })
        | E::ReturnCreatureFromYourGraveyardToHand(nodes::ReturnCreatureFromYourGraveyardToHand {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        })
        | E::ReturnFromGraveyard(nodes::ReturnFromGraveyard {
            creature,
            artifact,
            land,
            nonland_permanent,
            or_enchantment,
            or_spacecraft,
            or_artifact,
            max_mv,
            min_n,
            max_n,
            max_total_mv,
            destination,
            plus_one_counter,
            log_suffix,
            untargeted_all,
            target_kind,
        }) => {
            // "Return [up to n] target <type> card[s] [with mana value n or
            // less] from your graveyard to your hand / the battlefield."
            let _ = (log_suffix, target_kind); // display, Python dispatch
            l.deny("ReturnFromGraveyard.max_total_mv", max_total_mv);
            l.deny("ReturnFromGraveyard.plus_one_counter", plus_one_counter);
            l.deny("ReturnFromGraveyard.untargeted_all", untargeted_all);
            let t = |b: u16| Filter::Type(b);
            let primary = if *creature {
                let mut bits = types::CREATURE;
                if *or_enchantment {
                    bits |= types::ENCHANTMENT;
                }
                if *or_artifact {
                    bits |= types::ARTIFACT;
                }
                t(bits)
            } else if *artifact {
                t(types::ARTIFACT)
            } else if *land {
                t(types::LAND)
            } else if *nonland_permanent {
                // CR: a nonland permanent card (Python admits instants and
                // sorceries too, ENGINE.md 14).
                and(t(types::PERMANENT), not(t(types::LAND)))
            } else {
                l.unsupported("ReturnFromGraveyard(no card type)");
                return true;
            };
            if !*creature && (*or_enchantment || *or_artifact || *or_spacecraft) {
                l.unsupported("ReturnFromGraveyard.or_*(without creature)");
            }
            // "Creature or Spacecraft card" (a Spacecraft is an artifact
            // subtype, CR 205.3g; ENGINE.md 99.4).
            let primary = if *or_spacecraft { Filter::Or(vec![primary, Filter::Subtype(l.subtype("Spacecraft"))]) } else { primary };
            let mut f = and(primary, Filter::Owner(true));
            if *max_mv > 0 {
                f = and(f, Filter::ManaValueAtMost(l.int("ReturnFromGraveyard.max_mv", *max_mv, 0, 30) as u8));
            }
            let lo = l.int("ReturnFromGraveyard.min_n", *min_n, 0, 8) as u8;
            let hi = l.int("ReturnFromGraveyard.max_n", *max_n, 1, 8) as u8;
            let tgt = b.target(Filter::InGraveyard(Box::new(f)), lo, hi);
            let to_battlefield = *destination == enums::GraveyardReturnDestination::Battlefield;
            b.op(Op::Ext(Ext::FromGraveyard { tgt, to_battlefield, yours: true, tapped: false, counter: None }));
        }
        E::ExileUpToOneTargetCardFromGraveyard(nodes::ExileUpToOneTargetCardFromGraveyard {}) => {
            let t = b.target(Filter::InGraveyard(Box::new(Filter::Any)), 0, 1);
            b.op(Op::Exile { tgt: t });
        }
        E::ExileTargetCardFromGraveyard(nodes::ExileTargetCardFromGraveyard {}) => {
            let t = b.target(Filter::InGraveyard(Box::new(Filter::Any)), 1, 1);
            if l.link_exiles {
                // "Each card exiled with this" later (CR 607.2a).
                b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: false }));
            } else {
                b.op(Op::Exile { tgt: t });
            }
        }
        E::ExileEachOpponentsGraveyard(nodes::ExileEachOpponentsGraveyard {}) => {
            b.op(Op::Ext(Ext::ExileGraveyard { who: Who::Opponent }));
        }
        E::EachPlayerPutsCardFromGraveyardOntoBattlefield(nodes::EachPlayerPutsCardFromGraveyardOntoBattlefield {
            card_filter,
        }) => {
            let Some(filter) = super::filter::lower_card_filter(l, card_filter) else { return true };
            b.op(Op::Ext(Ext::EachPutFromGraveyard { filter }));
        }
        E::DealDamageDividedAmongTargets(nodes::DealDamageDividedAmongTargets {
            amount,
            max_targets,
            include_planeswalkers,
            double_if_x_at_least,
        }) => {
            // "N damage divided as you choose among any number of target
            // creatures [and/or planeswalkers]" (CR 601.2d: each target at
            // least 1, so at most N targets). "X damage ... among up to two
            // targets; if X is 6 or more, twice X damage instead"
            // (Shatterskull Smashing): X is announced first (601.2b), so the
            // targets are at most the amount (`cast::choose_targets`).
            let dbl = l.int("DealDamageDividedAmongTargets.double_if_x_at_least", *double_if_x_at_least, 0, 20) as u8;
            let (amt, n, min) = match amount {
                card_ir::unions::IntOrFormula::Int(n) => {
                    let n = l.int("DealDamageDividedAmongTargets.amount", *n, 1, 20) as u8;
                    if dbl > 0 {
                        l.unsupported("DealDamageDividedAmongTargets.double_if_x_at_least(without X)");
                    }
                    (crate::db::DivAmount::Fixed(n), n, 1)
                }
                card_ir::unions::IntOrFormula::Formula(f) if matches!(**f, cats::Formula::ResolvingX(_)) => {
                    // ("Up to": with X = 0 there is nothing to divide; "any
                    // number": at most X, `cast::choose_targets`.)
                    (crate::db::DivAmount::X { double_at: dbl }, 20, 0)
                }
                card_ir::unions::IntOrFormula::Formula(f) => {
                    l.unsupported(format!("Formula:{}", f.tag()));
                    return true;
                }
            };
            let most = match max_targets {
                Some(m) => (l.int("DealDamageDividedAmongTargets.max_targets", *m, 1, 20) as u8).min(n),
                None => n,
            };
            let f = if *include_planeswalkers { Filter::Type(types::CREATURE | types::PLANESWALKER) } else { creature() };
            if b.divide.is_some() {
                l.unsupported("DealDamageDividedAmongTargets(twice)");
            }
            let t = b.target(f, min, most);
            b.divide = Some((t, amt));
            b.op(Op::Ext(Ext::DealDivided { to: t }));
        }
        E::OwnerPutsTargetSpellOnTopOrBottom(nodes::OwnerPutsTargetSpellOnTopOrBottom { card_types, up_to_one }) => {
            let mut bits = 0u16;
            for t in card_types {
                match types::from_name(t) {
                    Some(x) => bits |= x,
                    None => l.unsupported(format!("OwnerPutsTargetSpellOnTopOrBottom.card_types({t})")),
                }
            }
            let f = Filter::Spell(Box::new(Filter::Type(bits)));
            let t = b.target(f, if *up_to_one { 0 } else { 1 }, 1);
            b.op(Op::Ext(Ext::SpellToLibraryOwnerChooses { tgt: t }));
        }
        E::ExileOpponentsNonlandPermanentUntilLeaves(nodes::ExileOpponentsNonlandPermanentUntilLeaves { max_mv }) => {
            // "Exile target nonland permanent an opponent controls [with mana
            // value n or less] until this leaves the battlefield."
            let mut f = and(and(Filter::Type(types::PERMANENT), not(Filter::Type(types::LAND))), Filter::Controller(false));
            if *max_mv > 0 {
                f = and(f, Filter::ManaValueAtMost(l.int("ExileOpponentsNonlandPermanentUntilLeaves.max_mv", *max_mv, 0, 30) as u8));
            }
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: true }));
        }
        E::ExileTargetUntilSourceLeaves(nodes::ExileTargetUntilSourceLeaves { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "ExileTargetUntilSourceLeaves") else { return true };
            b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: true }));
        }
        E::LookAtHandMayExileNonlandUntilSourceLeaves(nodes::LookAtHandMayExileNonlandUntilSourceLeaves {}) => {
            // "Look at target opponent's hand. You may exile a nonland card
            // from it until this leaves the battlefield." (Deep-Cavern Bat)
            let t = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            b.op(Op::RevealHand { whose: Who::TargetPlayer(t), to: Some(Who::You) });
            b.op(Op::Ext(Ext::ExileFromHandLinked {
                chooser: Who::You,
                whose: Who::TargetPlayer(t),
                filter: not(Filter::Type(types::LAND)),
                optional: true,
                until: true,
            }));
        }
        E::LinkExiledCardsToSource(nodes::LinkExiledCardsToSource { inner }) => match &**inner {
            // "Exile [up to one] target <...>" remembered by the source
            // (CR 607.2a; Skyclave Apparition).
            E::Exile(nodes::Exile { targets }) => {
                let Some(t) = lower_targets(l, b, targets, "Exile") else { return true };
                b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: false }));
            }
            // "Target opponent reveals their hand and you choose a nonland
            // card from it. Exile that card." (Tidehollow Sculler: its
            // leave trigger returns it.)
            E::TargetPlayerRevealsHandYouChooseDiscard(nodes::TargetPlayerRevealsHandYouChooseDiscard {
                card_filter,
                opponent_only,
                optional,
                then_draws,
                destination,
            }) if *destination == enums::RevealedCardFate::Exile => {
                l.deny("TargetPlayerRevealsHandYouChooseDiscard.then_draws", then_draws);
                let Some(filter) = super::filter::lower_card_filter(l, card_filter) else { return true };
                let pf = if *opponent_only { and(Filter::Player, Filter::Opponent) } else { Filter::Player };
                let t = b.target(pf, 1, 1);
                b.op(Op::RevealHand { whose: Who::TargetPlayer(t), to: None });
                b.op(Op::Ext(Ext::ExileFromHandLinked {
                    chooser: Who::You,
                    whose: Who::TargetPlayer(t),
                    filter,
                    optional: *optional,
                    until: false,
                }));
            }
            // "Exile target card from a graveyard" (Keen-Eyed Curator).
            E::ExileTargetCardFromGraveyard(nodes::ExileTargetCardFromGraveyard {}) => {
                let t = b.target(Filter::InGraveyard(Box::new(Filter::Any)), 1, 1);
                b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: false }));
            }
            other => l.unsupported(format!("LinkExiledCardsToSource.inner({})", other.tag())),
        },
        E::ReturnCardsExiledWithSourceToOwnersHand(nodes::ReturnCardsExiledWithSourceToOwnersHand {}) => {
            b.op(Op::Ext(Ext::ReturnLinkedToHand));
        }
        E::ExileTargetReturnAtNextEndStep(nodes::ExileTargetReturnAtNextEndStep { targets, counter_on_source_if_yours }) => {
            // "Exile [another] target permanent. Return that card to the
            // battlefield under its owner's control at the beginning of the
            // next end step [; if it entered under your control, a +1/+1
            // counter on this]."
            let Some(t) = lower_targets(l, b, targets, "ExileTargetReturnAtNextEndStep") else { return true };
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::ReturnEventCard));
            if *counter_on_source_if_yours {
                let at = d.op(Op::Ext(Ext::JumpUnlessMadeYours { to: 0 }));
                d.op(Op::Ext(Ext::PutCounters { on: On::Source, counter: Counter::P1P1, n: Amount::Fixed(1) }));
                d.patch(at);
            }
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::ExileThenDelay { tgt: t, k, when: DelayedWhen::NextEndStep }));
        }
        E::PutCreatureFromHandHasteSacrificeAtNextEndStep(nodes::PutCreatureFromHandHasteSacrificeAtNextEndStep {}) => {
            // "You may put a creature card from your hand onto the
            // battlefield. That creature gains haste. Sacrifice the creature
            // at the beginning of the next end step." (Sneak Attack)
            b.op(Op::PutFromHand { who: Who::You, filter: creature(), tapped: false, optional: true });
            let mut haste = KwSet::default();
            haste.insert(Kw::Haste);
            apply(b, On::Made, vec![Mod::Keywords(haste)], Until::Forever);
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::SacrificeEventObj));
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: DelayedWhen::NextEndStep, about: About::Made }));
        }
        E::CounterThenDelayedManaEqualToMV(nodes::CounterThenDelayedManaEqualToMV { counter, color }) => {
            // "Counter target spell. At the beginning of your next main
            // phase, add an amount of {C} equal to that spell's mana
            // value." (Mana Drain)
            if color != "C" {
                l.unsupported(format!("CounterThenDelayedManaEqualToMV.color({color})"));
            }
            match counter.as_deref() {
                Some(E::CounterTargetSpell(nodes::CounterTargetSpell { filter_types })) if filter_types.is_empty() => {}
                _ => {
                    l.unsupported("CounterThenDelayedManaEqualToMV.counter");
                    return true;
                }
            }
            let t = b.target(Filter::Spell(Box::new(Filter::Any)), 1, 1);
            b.op(Op::Ext(Ext::RememberSpellMv { tgt: t }));
            b.op(Op::Counter { tgt: t, dest: crate::program::SpellDest::Graveyard });
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::AddColorless { n: Amount::EventAmount }));
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: DelayedWhen::YourNextMainPhase, about: About::Remembered }));
        }
        E::ScheduleDelayedEffect(nodes::ScheduleDelayedEffect { inner, step }) => {
            let mut d = Builder::default();
            lower_effect(l, &mut d, inner);
            if !d.targets.is_empty() {
                l.unsupported("ScheduleDelayedEffect.inner(with targets)");
            }
            let k = l.delayed_ability(d.finish());
            let when = match step {
                enums::DelayedStep::Upkeep => DelayedWhen::NextUpkeep,
                enums::DelayedStep::End => DelayedWhen::NextEndStep,
            };
            b.op(Op::Ext(Ext::Delay { k, when, about: About::Nothing }));
        }
        E::PutPlusOneCountersOnTargetThenGrantKeywordsUEOT(nodes::PutPlusOneCountersOnTargetThenGrantKeywordsUEOT {
            amount,
            targets,
            granted_keywords,
        }) => {
            // "Put n +1/+1 counters on target <creature>[; it gains <kws>
            // until end of turn]."
            let n = l.int("PutPlusOneCountersOnTargetThenGrantKeywordsUEOT.amount", *amount, 1, 20);
            let Some(t) = lower_targets(l, b, targets, "PutPlusOneCountersOnTargetThenGrantKeywordsUEOT") else { return true };
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(n) }));
            let mods = keyword_mods(l, granted_keywords);
            if !mods.is_empty() {
                apply(b, On::Target(t), mods, Until::EndOfTurn);
            }
        }
        E::PumpTargetCreatureByFormulaUEOT(nodes::PumpTargetCreatureByFormulaUEOT {
            amount,
            power_sign,
            toughness_sign,
            targets,
        }) => {
            // "Target creature gets +-X/+-X until end of turn, where X is
            // <factor> times the number of <permanents> you control."
            let (filter, factor) = match amount.as_deref() {
                Some(cats::Formula::Scaled(nodes::Scaled { inner, factor })) => match &**inner {
                    cats::Formula::CountControlledMatching(nodes::CountControlledMatching { predicate }) => {
                        (super::filter::lower_pred(l, predicate), l.int("Scaled.factor", *factor, 1, 9))
                    }
                    other => {
                        l.unsupported(format!("Formula:Scaled({})", other.tag()));
                        return true;
                    }
                },
                Some(cats::Formula::CountControlledMatching(nodes::CountControlledMatching { predicate })) => {
                    (super::filter::lower_pred(l, predicate), 1)
                }
                Some(other) => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("PumpTargetCreatureByFormulaUEOT.amount(missing)");
                    return true;
                }
            };
            let Some(filter) = filter else { return true };
            let ps = l.int("PumpTargetCreatureByFormulaUEOT.power_sign", *power_sign, -1, 1);
            let ts = l.int("PumpTargetCreatureByFormulaUEOT.toughness_sign", *toughness_sign, -1, 1);
            let Some(t) = lower_targets(l, b, targets, "PumpTargetCreatureByFormulaUEOT") else { return true };
            let filter = and(filter, Filter::Controller(true));
            b.op(Op::Ext(Ext::ApplyPtPerCount {
                on: On::Target(t),
                filter,
                p: (ps * factor) as i8,
                t: (ts * factor) as i8,
            }));
        }
        E::UntapTargetOtherPermanent(nodes::UntapTargetOtherPermanent {}) => {
            // "Untap another target permanent."
            let t = b.target(and(Filter::Type(types::PERMANENT), not(Filter::Source)), 1, 1);
            b.op(Op::Untap { tgt: t });
        }
        E::MayTapOrUntapTarget(nodes::MayTapOrUntapTarget { targets }) => {
            // "You may tap or untap target creature."
            let Some(t) = lower_targets(l, b, targets, "MayTapOrUntapTarget") else { return true };
            b.op(Op::Ext(Ext::TapOrUntap { on: On::Target(t) }));
        }
        E::TargetPlayerPutsGraveyardOnBottomRandom(nodes::TargetPlayerPutsGraveyardOnBottomRandom { up_to_one }) => {
            // "[Up to one] target player puts all the cards from their
            // graveyard on the bottom of their library in a random order."
            let t = b.target(Filter::Player, if *up_to_one { 0 } else { 1 }, 1);
            b.op(Op::Ext(Ext::GraveyardToBottomRandom { who: Who::TargetPlayer(t) }));
        }
        E::CreateAttackingTokenForEachOpponent(nodes::CreateAttackingTokenForEachOpponent { token_printing }) => {
            // "For each opponent, create a <token> that's tapped and
            // attacking that player" (one opponent).
            let Some(tp) = token_printing else {
                l.unsupported("CreateAttackingTokenForEachOpponent.token_printing(missing)");
                return true;
            };
            let cats::CardPrinting::CardPrinting(tp) = &**tp;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateAttackingTokenForEachOpponent.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensAttacking { token, n: 1, tapped: true }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        E::DealDamageFromEvent(nodes::DealDamageFromEvent {
            amount,
            targets,
            subject_slot,
            to_defending_player,
            no_lifegain_if_player_hit,
        }) => {
            // "That creature deals damage equal to its power to any
            // target" (Pyrogoyf); "it deals that much damage to any other
            // target" (Screaming Nemesis).
            if *to_defending_player {
                if !targets.is_empty() || subject_slot.is_some() || *no_lifegain_if_player_hit {
                    l.unsupported("DealDamageFromEvent.to_defending_player(with targets)");
                }
                super::effect_tail3::damage_defending_player(l, b, amount.as_deref());
                return true;
            }
            let amount = match amount.as_deref() {
                Some(cats::Formula::EventSubjectPower(nodes::EventSubjectPower {})) => Amount::EventObjPower,
                Some(cats::Formula::ResolvingDamageAmount(nodes::ResolvingDamageAmount {})) => Amount::EventAmount,
                // "Damage equal to that spell's mana value" (Thor).
                Some(cats::Formula::EventSubjectManaValue(nodes::EventSubjectManaValue {})) => Amount::EventSpellMv,
                // "Damage equal to the number of creatures you control" (as
                // it resolves).
                Some(cats::Formula::CountControlledMatching(nodes::CountControlledMatching { predicate })) => {
                    match super::filter::lower_pred(l, predicate) {
                        Some(Filter::Type(bits)) => Amount::CountTypes(bits),
                        _ => {
                            l.unsupported("Formula:CountControlledMatching(not a card type)");
                            return true;
                        }
                    }
                }
                Some(other) => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("DealDamageFromEvent.amount(missing)");
                    return true;
                }
            };
            let event_obj = match subject_slot {
                None => false,
                // (The spell's mana value; the source deals the damage.)
                Some(enums::EventSubject::Spell) if amount == Amount::EventSpellMv => false,
                Some(enums::EventSubject::Obj) => true,
                Some(other) => {
                    l.unsupported(format!("DealDamageFromEvent.subject_slot({other:?})"));
                    return true;
                }
            };
            if amount == Amount::EventObjPower && !event_obj {
                l.unsupported("DealDamageFromEvent.amount(power without a subject)");
            }
            let Some(t) = lower_targets(l, b, targets, "DealDamageFromEvent") else { return true };
            b.op(Op::Ext(Ext::DamageFrom { event_obj, amount, tgt: t, no_lifegain: *no_lifegain_if_player_hit }));
        }
        E::PutCounterDoublingOnSecondResolution(nodes::PutCounterDoublingOnSecondResolution { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "PutCounterDoublingOnSecondResolution") else { return true };
            b.op(Op::Ext(Ext::CounterOrDoubleOnSecond { on: On::Target(t) }));
        }
        E::ReflexiveWhenYouDo(nodes::ReflexiveWhenYouDo { cost, effect, event_player_controls }) => {
            // "You may <cost>. When you do, <effect>" (CR 603.12): the
            // effect is a reflexive triggered ability; its targets are
            // chosen as it is put on the stack.
            let skip = match cost.as_deref() {
                Some(E::RemoveACounterFromSelf(nodes::RemoveACounterFromSelf {})) => {
                    b.op(Op::Ext(Ext::MayRemoveCounterThen { skip: 0 }))
                }
                Some(E::PayEnergy(nodes::PayEnergy { amount })) => {
                    let n = l.int("PayEnergy.amount", *amount, 1, 20) as u8;
                    b.op(Op::Ext(Ext::MayPayEnergyThen { n, skip: 0 }))
                }
                Some(other) => {
                    l.unsupported(format!("ReflexiveWhenYouDo.cost({})", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("ReflexiveWhenYouDo.cost(missing)");
                    return true;
                }
            };
            let Some(effect) = effect else {
                l.unsupported("ReflexiveWhenYouDo.effect(missing)");
                return true;
            };
            let mut d = Builder::default();
            lower_effect(l, &mut d, effect);
            if *event_player_controls {
                // "... that player controls": the player dealt combat
                // damage by a creature you control is your opponent (2p).
                if d.targets.len() != 1 {
                    l.unsupported("ReflexiveWhenYouDo.event_player_controls(targets)");
                } else {
                    let f = std::mem::replace(&mut d.targets[0].filter, Filter::Any);
                    d.targets[0].filter = and(f, Filter::Controller(false));
                }
            }
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Reflexive { k, per_token: false }));
            b.patch(skip);
        }
        E::ExileTopCardsWithPlayPermission(nodes::ExileTopCardsWithPlayPermission {
            count,
            free,
            duration,
            whose,
            cast_only,
            face_down,
            free_if,
            requires_attacked_with_subtype,
            any_color_mana,
        }) => {
            // "Exile the top n cards of <whose> library. You may play / cast
            // them this turn [without paying their mana costs]" (CR 601.2a,
            // 305.1: playing a land still uses the land drop). "If <state>,
            // you may play that card this turn without paying its mana cost.
            // Otherwise, you may play that card this turn" (`free_if`,
            // checked as it resolves).
            // "Exile the top card of your library face down. You may look at
            // and play that card this turn" (Headliner Scarlett).
            if *face_down {
                let plain = *count == 1
                    && !*free
                    && *duration == enums::ExilePlayDuration::ThisTurn
                    && *whose == enums::ImpulseLibraryOwner::You
                    && !*cast_only
                    && free_if.is_none()
                    && requires_attacked_with_subtype.is_empty()
                    && !*any_color_mana;
                if !plain {
                    l.unsupported("ExileTopCardsWithPlayPermission.face_down(with more)");
                    return true;
                }
                b.op(Op::Ext(Ext::ExileTopFaceDownPlayable));
                return true;
            }
            let free_if = match free_if.as_deref() {
                None => None,
                Some(s) => match super::trigger::lower_state_pred(l, s) {
                    Some((c, _)) => Some(c),
                    None => return true,
                },
            };
            if free_if.is_some() && *free {
                l.unsupported("ExileTopCardsWithPlayPermission.free_if(with free)");
            }
            let n = l.int("ExileTopCardsWithPlayPermission.count", *count, 1, 10) as u8;
            let who = match whose {
                enums::ImpulseLibraryOwner::You => Who::You,
                enums::ImpulseLibraryOwner::ThatPlayer => Who::EventPlayer,
                enums::ImpulseLibraryOwner::DefendingPlayer => Who::Opponent,
            };
            let attacked_subtype =
                (!requires_attacked_with_subtype.is_empty()).then(|| l.subtype(requires_attacked_with_subtype));
            // "During any turn you attacked with a Rogue": while it stays
            // exiled (`while_exiled`), not this turn only.
            let this_turn = match duration {
                enums::ExilePlayDuration::ThisTurn => {
                    if attacked_subtype.is_some() {
                        l.unsupported("ExileTopCardsWithPlayPermission.duration(this_turn with requires_attacked_with_subtype)");
                    }
                    true
                }
                enums::ExilePlayDuration::WhileExiled => false,
                // "You may play that card until your next end step" (Inti).
                enums::ExilePlayDuration::UntilYourNextEndStep if free_if.is_none() && attacked_subtype.is_none() => {
                    let kind = crate::game::PermKind::Play { free: *free, cast_only: *cast_only, any_mana: *any_color_mana, attacked_subtype };
                    b.op(Op::Ext(Ext::ExileTopUntilEndStep { who, n, kind }));
                    return true;
                }
                other => {
                    l.unsupported(format!("ExileTopCardsWithPlayPermission.duration({other:?})"));
                    true
                }
            };
            let kind = crate::game::PermKind::Play { free: *free, cast_only: *cast_only, any_mana: *any_color_mana, attacked_subtype };
            match free_if {
                None => {
                    b.op(Op::Ext(Ext::ExileTopWithPermission { who, n, kind, this_turn }));
                }
                Some(cond) => {
                    let j = b.op(Op::JumpUnless { cond, to: 0 });
                    let free_kind = crate::game::PermKind::Play { free: true, cast_only: *cast_only, any_mana: *any_color_mana, attacked_subtype };
                    b.op(Op::Ext(Ext::ExileTopWithPermission { who, n, kind: free_kind, this_turn }));
                    let end = b.op(Op::Jump { to: 0 });
                    b.patch(j);
                    b.op(Op::Ext(Ext::ExileTopWithPermission { who, n, kind, this_turn }));
                    b.patch(end);
                }
            }
        }
        E::GrantFlashbackToTargetCardUEOT(nodes::GrantFlashbackToTargetCardUEOT { card_filter }) => {
            // "Target instant or sorcery card in your graveyard gains
            // flashback until end of turn. The flashback cost is equal to
            // its mana cost."
            let Some(cf) = super::filter::lower_card_filter(l, card_filter) else { return true };
            let t = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), 1, 1);
            b.op(Op::Ext(Ext::GrantFlashback { tgt: t }));
        }
        E::OptionalPutLandFromHandOntoBattlefield(nodes::OptionalPutLandFromHandOntoBattlefield {}) => {
            // "You may put a land card from your hand onto the battlefield."
            b.op(Op::PutFromHand { who: Who::You, filter: Filter::Type(types::LAND), tapped: false, optional: true });
        }
        E::Backup(nodes::Backup { n, keywords, targets }) => {
            // Backup n (CR 702.165): "put n +1/+1 counters on target
            // creature. If that's another creature, it gains <keywords>
            // until end of turn."
            let n = l.int("Backup.n", *n, 1, 9);
            let Some(t) = lower_targets(l, b, targets, "Backup") else { return true };
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(n) }));
            let mods = keyword_mods(l, keywords);
            if !mods.is_empty() {
                let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: not(Filter::Source), to: 0 });
                apply(b, On::Target(t), mods, Until::EndOfTurn);
                b.patch(at);
            }
        }
        E::DiscardYourHand(nodes::DiscardYourHand {}) => {
            b.op(Op::DiscardHand { who: Who::You });
        }
        E::Amass(nodes::Amass { subtype, amount }) => {
            // Amass <subtype> n (CR 701.47).
            let n = l.int("Amass.amount", *amount, 1, 20) as u8;
            let json = format!(r#"{{"$t": "CardPrinting", "card_types": ["Creature"], "color_indicator": ["B"], "mana_cost": {{"$t": "ManaCost"}}, "mana_cost_str": "", "name": "{subtype} Army", "power": 0, "subtypes": ["{subtype}", "Army"], "supertypes": ["Token"], "toughness": 0, "type_line": "Token Creature - {subtype} Army"}}"#);
            let p: cats::CardPrinting = serde_json::from_str(&json).expect("an Army token printing");
            let cats::CardPrinting::CardPrinting(tp) = &p;
            let def = super::printing::lower_printing(l, tp);
            let token = l.token(def);
            let (army, sub) = (l.subtype("Army"), l.subtype(subtype));
            b.op(Op::Ext(Ext::Amass { token, army, subtype: sub, n }));
        }
        E::DealCombatDamageAmountToTarget(nodes::DealCombatDamageAmountToTarget { targets }) => {
            // "It deals that much damage to target <...>."
            let Some(t) = lower_targets(l, b, targets, "DealCombatDamageAmountToTarget") else { return true };
            b.op(Op::DealDamage { amount: Amount::EventAmount, to: t });
        }
        E::DestroyTargetNoncreaturePermanent(nodes::DestroyTargetNoncreaturePermanent { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "DestroyTargetNoncreaturePermanent") else { return true };
            b.op(Op::Destroy { tgt: t });
        }
        E::ExiledCardOwnerCreatesManaValueToken(nodes::ExiledCardOwnerCreatesManaValueToken { color, subtype }) => {
            // "The exiled card's owner creates an X/X <color> <subtype>
            // creature token, where X is the mana value of the exiled card."
            let json = format!(r#"{{"$t": "CardPrinting", "card_types": ["Creature"], "color_indicator": ["{color}"], "mana_cost": {{"$t": "ManaCost"}}, "mana_cost_str": "", "name": "{subtype}", "power": 0, "subtypes": ["{subtype}"], "supertypes": ["Token"], "toughness": 0, "type_line": "Token Creature - {subtype}"}}"#);
            let p: cats::CardPrinting = serde_json::from_str(&json).expect("a token printing");
            let cats::CardPrinting::CardPrinting(tp) = &p;
            let def = super::printing::lower_printing(l, tp);
            let token = l.token(def);
            b.op(Op::Ext(Ext::LinkedOwnersCreateMvToken { token }));
        }
        E::Monstrosity(nodes::Monstrosity { count }) => {
            let n = l.int("Monstrosity.count", *count, 1, 20) as u8;
            b.op(Op::Ext(Ext::Monstrosity { n }));
        }
        E::CreateTokenAndAttachSelf(nodes::CreateTokenAndAttachSelf { token_printing }) => {
            // Living weapon / For Mirrodin! (CR 702.92a, 702.163a).
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateTokenAndAttachSelf.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokenAttachSource { token }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        E::LookTopNKeepUpToOneOnTopRestBottom(nodes::LookTopNKeepUpToOneOnTopRestBottom { count }) => {
            // "Look at the top X cards of your library. Put up to one of
            // them on top of your library and the rest on the bottom of your
            // library in a random order."
            let n = match count {
                card_ir::unions::IntOrFormula::Int(n) => Amount::Fixed(l.int("LookTopNKeepUpToOneOnTopRestBottom.count", *n, 1, 20)),
                card_ir::unions::IntOrFormula::Formula(f) => match &**f {
                    cats::Formula::DevotionTo(nodes::DevotionTo { color }) => match crate::mana::letter_index(color) {
                        Some(i) if i < 5 => Amount::Devotion(i as u8),
                        _ => {
                            l.unsupported(format!("DevotionTo({color})"));
                            return true;
                        }
                    },
                    other => {
                        l.unsupported(format!("Formula:{}", other.tag()));
                        return true;
                    }
                },
            };
            b.op(Op::Ext(Ext::LookTopN { who: Who::You, n }));
            b.op(Op::PileTo {
                who: Who::You,
                dest: crate::program::PileDest::Top,
                filter: Filter::Any,
                min: 0,
                max: 1,
                rest: false,
                reveal: false,
            });
            b.op(Op::Ext(Ext::PileRestBottomRandom));
        }
        E::IfCondition(nodes::IfCondition { condition, then }) => {
            // "If <condition>, <effect>" (checked as it resolves).
            let Some(cond) = super::filter::lower_condition(l, condition) else { return true };
            let at = b.op(Op::JumpUnless { cond, to: 0 });
            lower_effect(l, b, then);
            b.patch(at);
        }
        E::Exile(nodes::Exile { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "Exile") else { return true };
            b.op(Op::Exile { tgt: t });
        }
        E::CreateTokenCopyOfSource(nodes::CreateTokenCopyOfSource { extra_card_types, eternalize }) => {
            // "Create a token that's a copy of it[, except it's an
            // artifact]" (its copiable values as it last existed, CR 707.2,
            // 608.2h; the exception, 707.9b). Eternalize (702.129a, a
            // graveyard ability that exiles the card as a cost): "a copy of
            // this card, except it's black, it's 4/4, it has no mana cost,
            // and it's a Zombie in addition to its other types".
            let mut exc = crate::copies::Except::default();
            for t in extra_card_types {
                match types::from_name(t) {
                    Some(b) => exc.types |= b,
                    None => l.unsupported(format!("CreateTokenCopyOfSource.extra_card_types({t})")),
                }
            }
            let of = if *eternalize {
                exc.colors = Some(1 << 2); // black
                exc.pt = Some((4, 4));
                exc.no_cost = true;
                let zombie = l.subtype("Zombie");
                exc.add_subtype(zombie);
                crate::program::CopyOf::SourceCard
            } else {
                crate::program::CopyOf::Source
            };
            b.op(Op::Ext(Ext::CreateTokenCopy { of, who: Who::You, n: Amount::Fixed(1), exc }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        E::RevealUntilCreatureOntoBattlefieldAttacking(nodes::RevealUntilCreatureOntoBattlefieldAttacking {}) => {
            // "Reveal cards from the top of your library until you reveal a
            // creature card. Put that card onto the battlefield tapped and
            // attacking and the rest on the bottom in a random order."
            b.op(Op::Ext(Ext::RevealUntilCreatureAttacking));
        }
        E::CounterThenWhenItAttacksMonarch(nodes::CounterThenWhenItAttacksMonarch { targets, keywords }) => {
            // "Put a +1/+1 counter on target creature. Whenever that
            // creature attacks the monarch this turn, it gains <kws> until
            // end of turn": a delayed triggered ability about that creature
            // (CR 603.7b-c), on the stack as it attacks.
            let Some(t) = lower_targets(l, b, targets, "CounterThenWhenItAttacksMonarch") else { return true };
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(1) }));
            let mut set = KwSet::default();
            for m in keyword_mods(l, keywords) {
                match m {
                    Mod::Keywords(k) => set = k,
                    _ => l.unsupported("CounterThenWhenItAttacksMonarch.keywords(protection)"),
                }
            }
            let mut d = Builder::default();
            apply(&mut d, On::EventObj, vec![Mod::Keywords(set)], Until::EndOfTurn);
            let ability = d.finish();
            let uses_event = ability.uses_event();
            l.delayed.push(crate::db::TriggerDef {
                event: crate::db::EventKind::Attacks,
                pred: crate::db::TrigPred::AttacksMonarch,
                ability,
                uses_event,
            });
            let k = (l.delayed.len() - 1) as u8;
            b.op(Op::Ext(Ext::M7d(crate::program::M7d::TriggerThisTurnFor { k, on: On::Target(t) })));
        }
        E::BecomeMonarch(nodes::BecomeMonarch {}) => {
            super::effect_stack::monarch_defs(l);
            b.op(Op::Ext(Ext::BecomeMonarch));
        }
        E::ExileUntilOpponentBecomesMonarch(nodes::ExileUntilOpponentBecomesMonarch { targets }) => {
            let Some(t) = lower_targets(l, b, targets, "ExileUntilOpponentBecomesMonarch") else { return true };
            b.op(Op::Ext(Ext::ExileUntilMonarch { tgt: t }));
        }
        E::TargetOpponentLosesLife(nodes::TargetOpponentLosesLife { amount }) => {
            let n = l.int("TargetOpponentLosesLife.amount", *amount, 0, 99);
            let t = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            b.op(Op::LoseLife { who: Who::TargetPlayer(t), n: Amount::Fixed(n) });
        }
        E::IfSourceHasNoCounters(nodes::IfSourceHasNoCounters { kind, inner }) => {
            // "Then if there are no <kind> counters on this, ..."
            let counter = counter_by_name(l, kind);
            let at = b.op(Op::Ext(Ext::JumpUnlessSourceNoCounters { counter, to: 0 }));
            lower_effect(l, b, inner);
            b.patch(at);
        }
        E::ExileThisSpell(nodes::ExileThisSpell {}) => {
            b.op(Op::Ext(Ext::ExileThisSpell));
        }
        E::UntapUpToNLands(nodes::UntapUpToNLands { amount }) => {
            // "You untap up to n lands" (not targeted; only tapped ones are
            // offered, section 23.11).
            let n = l.int("UntapUpToNLands.amount", *amount, 1, 20) as u8;
            let filter = and(and(Filter::Type(types::LAND), Filter::Controller(true)), Filter::Tapped);
            b.op(Op::Ext(Ext::Choose { who: Who::You, filter, min: 0, max: n, act: ChooseAct::Untap }));
        }
        E::DealDamageExileIfDiesThisTurn(nodes::DealDamageExileIfDiesThisTurn { amount, targets }) => {
            // "Deals n damage to target creature. If that creature would die
            // this turn, exile it instead."
            let n = l.int("DealDamageExileIfDiesThisTurn.amount", *amount, 0, 99);
            let Some(t) = lower_targets(l, b, targets, "DealDamageExileIfDiesThisTurn") else { return true };
            apply(b, On::Target(t), vec![Mod::ExileIfDies], Until::EndOfTurn);
            b.op(Op::DealDamage { amount: Amount::Fixed(n), to: t });
        }
        E::MayCastTargetGraveyardCardThisTurn(nodes::MayCastTargetGraveyardCardThisTurn { card_filter }) => {
            // "Choose target <card> in your graveyard. You may cast that card
            // this turn."
            let cf = match card_filter {
                Some(f) => match super::filter::lower_card_filter(l, f) {
                    Some(f) => f,
                    None => return true,
                },
                None => Filter::Any,
            };
            let t = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), 1, 1);
            b.op(Op::Ext(Ext::GraveyardCastPermission { tgt: t }));
        }
        E::CreateTokenCopyOfTargetTokenYouControl(nodes::CreateTokenCopyOfTargetTokenYouControl {}) => {
            // "Create a token that's a copy of target token you control"
            // (its copiable values, CR 707.2: a copy of a token copy has the
            // exceptions of the copy effect that made it).
            let t = b.target(and(Filter::Token, Filter::Controller(true)), 1, 1);
            let of = crate::program::CopyOf::Target { i: t, lki: false };
            let exc = crate::copies::Except::default();
            b.op(Op::Ext(Ext::CreateTokenCopy { of, who: Who::You, n: Amount::Fixed(1), exc }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        E::OpponentGainsControlOfSource(nodes::OpponentGainsControlOfSource {}) => {
            b.op(Op::Ext(Ext::GiveSourceToOpponent));
        }
        E::ExileThatCreature(nodes::ExileThatCreature {}) => {
            b.op(Op::Ext(Ext::ExileEventObj2));
        }
        E::SacrificeSelf(nodes::SacrificeSelf {}) => {
            b.op(Op::Ext(Ext::SacrificeSource));
        }
        E::MayMillDamageThenMayTakeCreature(nodes::MayMillDamageThenMayTakeCreature {}) => {
            b.op(Op::Ext(Ext::MayMillThenTakeCreature));
        }
        E::PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl(
            nodes::PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl { grant_flying_counter, grant_subtype_and_pt },
        ) => {
            let becomes = grant_subtype_and_pt.as_ref().map(|(s, p, t)| {
                (l.subtype(s), l.int("grant_subtype_and_pt.p", *p, 0, 99), l.int("grant_subtype_and_pt.t", *t, 0, 99))
            });
            b.op(Op::Ext(Ext::LinkedCreaturesToBattlefield { flying: *grant_flying_counter, becomes }));
        }
        E::LookAtRandomCardInTargetPlayersHand(nodes::LookAtRandomCardInTargetPlayersHand {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::LookAtRandomHandCard { who: Who::TargetPlayer(t) }));
        }
        E::GrantKeywordsUEOTAndPutCounters(nodes::GrantKeywordsUEOTAndPutCounters {
            targets,
            keywords,
            target_counters,
            self_counters,
        }) => {
            // "Another target creature gains <kws> until end of turn. Put
            // <counters> on that creature and <counters> on this."
            let Some(t) = lower_targets(l, b, targets, "GrantKeywordsUEOTAndPutCounters") else { return true };
            let mods = keyword_mods(l, keywords);
            if !mods.is_empty() {
                apply(b, On::Target(t), mods, Until::EndOfTurn);
            }
            for (on, list) in [(On::Target(t), target_counters), (On::Source, self_counters)] {
                for (name, n) in list {
                    let counter = counter_by_name(l, name);
                    let n = l.int("GrantKeywordsUEOTAndPutCounters.counters", *n, 1, 20);
                    b.op(Op::Ext(Ext::PutCounters { on: on.clone(), counter, n: Amount::Fixed(n) }));
                }
            }
        }
        E::RevealTopNOnePerCardTypeToHand(nodes::RevealTopNOnePerCardTypeToHand { count }) => {
            let n = l.int("RevealTopNOnePerCardTypeToHand.count", *count, 1, 20) as u8;
            b.op(Op::Ext(Ext::RevealTopOnePerType { n }));
        }
        E::DealXDamage(nodes::DealXDamage { amount, targets }) => {
            // "This deals X damage to <target>."
            l.deny("DealXDamage.amount", amount);
            let Some(t) = lower_targets(l, b, targets, "DealXDamage") else { return true };
            b.op(Op::DealDamage { amount: Amount::X, to: t });
        }
        E::ApplyEffect(nodes::ApplyEffect { inner, ends_at }) => {
            // "<This> becomes an artifact creature until end of turn"
            // (crew, CR 702.122a).
            let until = match &**ends_at {
                cats::Duration::EndOfTurn(nodes::EndOfTurn {}) => Until::EndOfTurn,
                other => {
                    l.unsupported(format!("ApplyEffect.ends_at({})", other.tag()));
                    return true;
                }
            };
            match &**inner {
                E::BecomeType(nodes::BecomeType { types_to_add, subtypes_to_add, base_pt }) => {
                    let mut add = 0u16;
                    for t in types_to_add {
                        match types::from_name(&format!("{t:?}")) {
                            Some(bits) => add |= bits,
                            None => l.unsupported(format!("BecomeType.types_to_add({t:?})")),
                        }
                    }
                    let subtypes: Vec<u16> = subtypes_to_add.iter().map(|s| l.subtype(s)).collect();
                    let mut mods = vec![Mod::Types { add, subtypes, replace: false }];
                    if let Some((p, t)) = base_pt {
                        mods.push(Mod::SetPt(l.int("BecomeType.base_pt", *p, 0, 99), l.int("BecomeType.base_pt", *t, 0, 99)));
                    }
                    apply(b, On::Source, mods, until);
                }
                other => l.unsupported(format!("ApplyEffect.inner({})", other.tag())),
            }
        }
        E::ReturnAllPermanentsToOwnersHands(nodes::ReturnAllPermanentsToOwnersHands {}) => {
            b.op(Op::Ext(Ext::BounceAll));
        }
        E::VoteExileNonlandPermanent(nodes::VoteExileNonlandPermanent {}) => {
            // "Each player votes for a nonland permanent you don't control"
            // (not targeted).
            let filter = and(not(Filter::Type(types::LAND)), Filter::Controller(false));
            b.op(Op::Ext(Ext::VoteExile { filter }));
        }
        E::MillTwoRepeatWhileShareColor(nodes::MillTwoRepeatWhileShareColor {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::MillTwoWhileShareColor { who: Who::TargetPlayer(t) }));
        }
        E::PutCreatureFromHandSacrificeUnlessPay(nodes::PutCreatureFromHandSacrificeUnlessPay { reduction }) => {
            // "You may put a creature card from your hand onto the
            // battlefield. If you do, sacrifice it unless you pay its mana
            // cost reduced by {n}."
            let n = l.int("PutCreatureFromHandSacrificeUnlessPay.reduction", *reduction, 0, 9) as u8;
            b.op(Op::PutFromHand { who: Who::You, filter: creature(), tapped: false, optional: true });
            b.op(Op::Ext(Ext::SacMadeUnlessPayReduced { n }));
        }
        E::OathRevealUntilCreature(nodes::OathRevealUntilCreature { card_type }) => {
            // "That player chooses target player who controls more
            // creatures than they do and is their opponent. The first player
            // may reveal cards ... until they reveal a creature card ..."
            let Some(bits) = types::from_name(&format!("{card_type:?}")) else {
                l.unsupported(format!("OathRevealUntilCreature.card_type({card_type:?})"));
                return true;
            };
            b.target(Filter::OathTarget, 1, 1);
            b.op(Op::Ext(Ext::RevealUntilToBattlefield { who: Who::Active, types: bits }));
        }
        E::GetEnergy(nodes::GetEnergy { amount }) => {
            let n = l.int("GetEnergy.amount", *amount, 1, 20) as u8;
            b.op(Op::Ext(Ext::GetEnergy { n }));
        }
        E::DealDamagePaidInEnergy(nodes::DealDamagePaidInEnergy { amount, targets, energy }) => {
            // "Choose target <...>. You get {E} x n, then you may pay any
            // amount of {E}. This deals that much damage to that permanent."
            l.deny("DealDamagePaidInEnergy.amount", amount);
            let n = l.int("DealDamagePaidInEnergy.energy", *energy, 0, 20) as u8;
            let Some(t) = lower_targets(l, b, targets, "DealDamagePaidInEnergy") else { return true };
            if n > 0 {
                b.op(Op::Ext(Ext::GetEnergy { n }));
            }
            b.op(Op::Ext(Ext::PayEnergyDamage { tgt: t }));
        }
        E::PutCountersOnTargetAndAddSubtype(nodes::PutCountersOnTargetAndAddSubtype { targets, counters, subtype }) => {
            // "Put <counters> on target <...>. It becomes a <subtype> in
            // addition to its other types."
            let Some(t) = lower_targets(l, b, targets, "PutCountersOnTargetAndAddSubtype") else { return true };
            for (name, n) in counters {
                let counter = counter_by_name(l, name);
                let n = l.int("PutCountersOnTargetAndAddSubtype.counters", *n, 1, 20);
                b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter, n: Amount::Fixed(n) }));
            }
            let s = l.subtype(subtype);
            apply(b, On::Target(t), vec![Mod::Types { add: 0, subtypes: vec![s], replace: false }], Until::Forever);
        }
        E::LookAtTopCardsDistribute(nodes::LookAtTopCardsDistribute { count, piles }) => {
            // "Look at the top three cards of your library. Put one of them
            // into your hand, put one of them on the bottom of your library,
            // and exile one of them. You may play the exiled card this turn."
            use enums::DistributePile as D;
            if *count != 3 || piles[..] != [D::Hand, D::Bottom, D::ExilePlayable] {
                l.unsupported("LookAtTopCardsDistribute(not hand, bottom, exile)");
                return true;
            }
            b.op(Op::LookTop { who: Who::You, n: 3, reveal: false });
            let pile = |dest| Op::PileTo { who: Who::You, dest, filter: Filter::Any, min: 1, max: 1, rest: false, reveal: false };
            b.op(pile(crate::program::PileDest::Hand));
            b.op(pile(crate::program::PileDest::Bottom));
            b.op(Op::Ext(Ext::PileRestExilePlayable));
        }
        E::LookAtTopCardOfTargetPlayersLibrary(nodes::LookAtTopCardOfTargetPlayersLibrary {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::LookAtTopOf { who: Who::TargetPlayer(t) }));
        }
        E::WinTheGame(nodes::WinTheGame {}) => {
            b.op(Op::Ext(Ext::WinGame));
        }
        E::SacrificeSelfUnlessEscaped(nodes::SacrificeSelfUnlessEscaped {}) => {
            b.op(Op::Ext(Ext::SacrificeSourceUnlessEscaped));
        }
        E::SacrificeSelfIfEvoked(nodes::SacrificeSelfIfEvoked {}) => {
            // (The "if evoked" is the trigger's: Source(WasEvoked).)
            b.op(Op::Ext(Ext::SacrificeSource));
        }
        E::AnOpponentDrawsCards(nodes::AnOpponentDrawsCards { amount }) => {
            let n = Amount::Fixed(l.int("AnOpponentDrawsCards.amount", *amount, 0, 99));
            b.op(Op::Draw { who: Who::Opponent, n });
        }
        E::TargetOpponentDiscardsCasterChoice(nodes::TargetOpponentDiscardsCasterChoice {}) => {
            // "Target opponent reveals their hand. You choose a nonland card
            // from it. That player discards that card." (Grief)
            let t = b.target(and(Filter::Player, Filter::Opponent), 1, 1);
            b.op(Op::RevealHand { whose: Who::TargetPlayer(t), to: None });
            let filter = not(Filter::Type(types::LAND));
            b.op(Op::ChooseDiscard { chooser: Who::You, whose: Who::TargetPlayer(t), filter, optional: false });
        }
        _ => return false,
    }
    true
}
