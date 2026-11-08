//! Edge of Eternities, lane B (ENGINE.md 99.3): lands, mana abilities and
//! Lander tokens. Same rules as `effect.rs`: every field named, the rest
//! denied; ops named after rules actions, never cards.

use super::Lowerer;
use crate::db::{types, Cond, Filter, ManaProd, PtCda};
use crate::program::{About, Amount, Builder, EoeB, Ext, Op, SearchDest, Who};
use card_ir::{cats, nodes};

/// The Lander token (CR 111.10, the predefined tokens: "a colorless artifact
/// token named Lander with the artifact subtype Lander and '{2}, {T},
/// Sacrifice this token: Search your library for a basic land card, put it
/// onto the battlefield tapped, then shuffle.'"), written as IR for the
/// effect classes that create one without a printing. (The 13 printings
/// the IR spells out drop its {T}: docs/ir_defects.md 42.)
const LANDER_TOKEN: &str = r#"{"$t": "CardPrinting", "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 2}, "mana_str": "{2}", "sacrifice_self": true, "tap_self": true}, "effect": {"$t": "SearchLibraryForBasicLandTapped"}}], "card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Lander", "subtypes": ["Lander"], "supertypes": ["Token"], "type_line": "Token Artifact — Lander"}"#;

/// The Lander token's definition (an index of `Op::CreateToken`).
pub(crate) fn lander(l: &mut Lowerer) -> u16 {
    let p: cats::CardPrinting = serde_json::from_str(LANDER_TOKEN).expect("Lander token printing");
    let cats::CardPrinting::CardPrinting(tp) = &p;
    let def = super::printing::lower_printing(l, tp);
    l.token(def)
}

/// "A basic land card" (CR 205.4a).
fn basic_land() -> Filter {
    Filter::And(Box::new(Filter::Type(types::LAND)), Box::new(Filter::Supertype(crate::db::supertypes::BASIC)))
}

/// "You create a Lander token."
fn create_lander(l: &mut Lowerer, b: &mut Builder, who: Who) {
    let token = lander(l);
    if who == Who::You {
        b.op(Op::CreateToken { token, n: 1, tapped: false });
    } else {
        b.op(Op::Ext(Ext::CreateTokensFor { who, token, n: 1 }));
    }
    b.op(Op::Ext(Ext::TokensCreated));
}

/// Lower an effect class of lane B; `false` if the class is not one of
/// these.
pub(crate) fn lower_effect_eoe_b(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Search your library for a basic land card, put it onto the
        // battlefield tapped, then shuffle" (the Lander's ability; CR
        // 701.23: a card with a stated quality may be missed).
        E::SearchLibraryForBasicLandTapped(nodes::SearchLibraryForBasicLandTapped { count }) => {
            if *count != 1 {
                l.unsupported("SearchLibraryForBasicLandTapped.count(not 1)");
            }
            let dest = SearchDest::Battlefield { tapped: true };
            b.op(Op::Search { who: Who::You, filter: basic_land(), dest, reveal: false, may_fail: true });
        }
        // "Create a Lander token. At the beginning of the end step on your
        // next turn, sacrifice that token" (a delayed triggered ability
        // about the token, CR 603.7).
        E::CreateLanderAndScheduleSacrifice(nodes::CreateLanderAndScheduleSacrifice {}) => {
            create_lander(l, b, Who::You);
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::SacrificeEventObj));
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: crate::db::DelayedWhen::YourNextTurnEndStep, about: About::Made }));
        }
        // "Destroy target nonland permanent. Its controller creates a Lander
        // token" (the permanent's controller as it last existed, CR 608.2h;
        // whether or not it was destroyed).
        E::DestroyTargetNonlandPermanentWithLanderForController(nodes::DestroyTargetNonlandPermanentWithLanderForController {}) => {
            let f = Filter::And(Box::new(Filter::Type(types::PERMANENT)), Box::new(Filter::Not(Box::new(Filter::Type(types::LAND)))));
            let tgt = b.target(f, 1, 1);
            b.op(Op::Remember { tgt });
            b.op(Op::Destroy { tgt });
            create_lander(l, b, Who::LkiController);
        }
        // "<This> deals 6 damage to target creature. If excess damage was
        // dealt this way, create a Lander token" (CR 120.10).
        E::DealDamageToTargetCreatureWithExcessLanderRider(nodes::DealDamageToTargetCreatureWithExcessLanderRider { amount, targets }) => {
            let n = l.int("DealDamageToTargetCreatureWithExcessLanderRider.amount", *amount, 1, 99);
            let Some(tgt) = super::effect::lower_targets(l, b, targets, "DealDamageToTargetCreatureWithExcessLanderRider") else {
                return true;
            };
            let req = &b.targets[tgt as usize];
            if req.filter != Filter::Type(types::CREATURE) || (req.min, req.max) != (1, 1) {
                l.unsupported("DealDamageToTargetCreatureWithExcessLanderRider.targets(not one target creature)");
            }
            let at = b.op(Op::Ext(Ext::EoeB(EoeB::DamageJumpUnlessExcess { amount: Amount::Fixed(n), tgt, to: 0 })));
            create_lander(l, b, Who::You);
            b.patch(at);
        }
        // "Sacrifice two lands" (Lotus Field's enters trigger; CR 701.21a:
        // any lands its controller controls, itself included).
        E::SacrificeNLandsYouControl(nodes::SacrificeNLandsYouControl { count }) => {
            let n = l.int("SacrificeNLandsYouControl.count", *count, 1, 9) as u8;
            b.op(Op::Ext(Ext::SacrificeN { who: Who::You, filter: Filter::Type(types::LAND), n }));
        }
        // "<This> deals 1 damage to each opponent. If this is the second time
        // this ability has resolved this turn, draw a card."
        E::TannukEnsignLandfallEffect(nodes::TannukEnsignLandfallEffect {}) => {
            b.op(Op::DamagePlayer { amount: Amount::Fixed(1), who: Who::Opponent });
            let at = b.op(Op::Ext(Ext::EoeB(EoeB::JumpUnlessSecondResolution { to: 0 })));
            b.op(Op::Draw { who: Who::You, n: Amount::Fixed(1) });
            b.patch(at);
        }
        // "Exile cards from the top of your library until you exile a
        // nonland card. You may cast that card this turn."
        E::TerritorialBruntarLandfallEffect(nodes::TerritorialBruntarLandfallEffect {}) => {
            b.op(Op::Ext(Ext::EoeB(EoeB::ExileUntilNonlandMayCastThisTurn)));
        }
        // "Untap target land."
        E::UntapTargetLand(nodes::UntapTargetLand {}) => {
            let tgt = b.target(Filter::Type(types::LAND), 1, 1);
            b.op(Op::Untap { tgt });
        }
        // "You may put a basic land card from your hand onto the battlefield
        // tapped."
        E::PutBasicLandFromHandOntoBattlefieldTapped(nodes::PutBasicLandFromHandOntoBattlefieldTapped {}) => {
            b.op(Op::PutFromHand { who: Who::You, filter: basic_land(), tapped: true, optional: true });
        }
        _ => return false,
    }
    true
}

/// A characteristic-defining P/T formula of lane B (CR 604.3), or `None`
/// (with the blocker recorded) if the class is not one.
pub(crate) fn lower_pt_formula_eoe_b(l: &mut Lowerer, f: &cats::PTFormula) -> Option<PtCda> {
    match f {
        // "<This>'s power and toughness are each equal to the number of
        // lands you control."
        cats::PTFormula::LandsYouControlPT(nodes::LandsYouControlPT {}) => Some(PtCda::LandsYouControl),
        other => {
            l.unsupported(format!("PTFormula:{}", other.tag()));
            None
        }
    }
}

/// A cost reduction of lane B (CR 601.2f), or the blocker recorded.
pub(crate) fn lower_discount_eoe_b(l: &mut Lowerer, n: u8, c: &cats::Condition) -> Option<crate::db::Discount> {
    match c {
        // "This spell costs {X} less to cast, where X is the number of
        // differently named lands you control" (n = 1).
        cats::Condition::PerDifferentlyNamedLand(nodes::PerDifferentlyNamedLand {}) => {
            Some(crate::db::Discount::PerDifferentlyNamedLand(n))
        }
        other => {
            l.unsupported(format!("CostReduction.condition({})", other.tag()));
            None
        }
    }
}

/// A state predicate of lane B, for the source's controller (an
/// intervening "if", CR 603.4), or the blocker recorded.
pub(crate) fn lower_state_pred_eoe_b(l: &mut Lowerer, p: &cats::StatePredicate) -> Option<(Cond, bool)> {
    match p {
        // "If an opponent controls more lands than you."
        cats::StatePredicate::OpponentControlsMoreLandsThanYou(nodes::OpponentControlsMoreLandsThanYou {}) => {
            Some((Cond::OpponentControlsMoreLands, true))
        }
        other => {
            l.unsupported(format!("StatePredicate:{}", other.tag()));
            None
        }
    }
}

/// A mana ability's production of lane B (CR 605), or the blocker recorded.
pub(crate) fn lower_mana_prod_eoe_b(l: &mut Lowerer, e: &cats::Effect) -> Option<ManaProd> {
    use cats::Effect as E;
    match e {
        // "Choose a color of a permanent you control. Add one mana of that
        // color" (CR 105.2: colorless is not a color).
        E::AddOneManaOfChosenPermanentColor(nodes::AddOneManaOfChosenPermanentColor {}) => {
            Some(ManaProd::ColorAmong(Filter::Type(types::PERMANENT)))
        }
        // "Add one mana of any type that a land you control could produce"
        // (CR 106.7).
        E::AddOneManaFromControlledLandsProductions(nodes::AddOneManaFromControlledLandsProductions {}) => {
            Some(ManaProd::LandsCouldProduce)
        }
        other => {
            l.unsupported(format!("ManaAbility:{}", other.tag()));
            None
        }
    }
}
