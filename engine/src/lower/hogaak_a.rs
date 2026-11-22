//! Hogaak, Arisen Necropolis's printing fields (ENGINE.md 220.1):
//! `castable_from_graveyard` ("You may cast this card from your graveyard",
//! CR 601.3, 113.6f) and `cant_spend_mana_to_cast` ("You can't spend mana to
//! cast this spell", 113.6e, 601.2f-h). Its convoke, delve and trample are
//! the existing fields' (`has_convoke`, `has_delve`, the keyword).

use super::Lowerer;
use crate::db::{types, CastingRules};
use crate::mana::ManaCost;

/// The two fields onto `casting`, refusing the combinations the engine
/// doesn't play: a land with either (lands aren't cast, CR 305.9 / 601.3);
/// a spell of no mana with {X} or a Phyrexian or {C} pip in its mana cost,
/// or with costs of its own besides its mana cost (additional, alternative,
/// kicker, buyback, flashback, escape, miracle, dash, warp, bestow, times
/// paid; `own_costs`): none of the pools' cards has one, and their parts would
/// compete with convoke's taps (`hogaak_a::rest_takes_permanents`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn casting_rules(l: &mut Lowerer, casting: &mut CastingRules, own_gy: bool, no_mana: bool, ty: u16, cost: &ManaCost, x_pips: u8, own_costs: bool) {
    if (own_gy || no_mana) && ty & types::LAND != 0 {
        l.unsupported("CardPrinting.castable_from_graveyard / cant_spend_mana_to_cast(a land)");
    }
    if no_mana && (x_pips > 0 || cost.phyrexian.iter().any(|&n| n > 0) || cost.pips[crate::mana::C] > 0) {
        l.unsupported("CardPrinting.cant_spend_mana_to_cast(with {X}, a Phyrexian or a {C} pip)");
    }
    if no_mana && own_costs {
        l.unsupported("CardPrinting.cant_spend_mana_to_cast(with costs of its own besides its mana cost)");
    }
    casting.own_gy_cast = own_gy;
    casting.no_mana = no_mana;
}

/// After the whole printing is lowered: a spell of no mana that reads the
/// mana spent on it (converge, sunburst) is refused (it reads none; the
/// engine records colours only from a mana payment).
pub(crate) fn check_def(l: &mut Lowerer, def: &crate::db::CardDef) {
    if def.casting.no_mana && def.converge {
        l.unsupported("CardPrinting.cant_spend_mana_to_cast(with converge or sunburst)");
    }
}
