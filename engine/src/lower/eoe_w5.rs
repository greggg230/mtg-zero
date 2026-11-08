//! Edge of Eternities, W5 (ENGINE.md 99.15): `CardPrinting.hand_ability_grants`,
//! an alternative cost a permanent's static ability gives the cards in its
//! controller's hand ("Artifact cards and red creature cards in your hand
//! have warp {2}{R}": Tannuk, Steadfast Second; CR 604.1, 611.3a, 118.9,
//! 702.185a). Every field named, the rest denied.

use super::Lowerer;
use crate::db::{types, AltCost, Filter, HandAltGrant};
use card_ir::{cats, nodes};

/// The grants of a printing, each with its warp's delayed ability on the
/// definition being lowered (CR 702.185a: "exile the permanent this spell
/// becomes at the beginning of the next end step ...", `Ext::WarpExile`, as a
/// card's own warp). At most one a printing: `ChooseAltCost` names a granted
/// cost by its permanent, so each must have one (ENGINE.md 99.15).
pub(crate) fn hand_alt_grants(l: &mut Lowerer, grants: &[cats::HandAbilityGrant]) -> Vec<HandAltGrant> {
    if grants.len() > 1 {
        l.unsupported("CardPrinting.hand_ability_grants(more than one: a granted cost is named by its permanent)");
        return Vec::new();
    }
    let mut out = Vec::new();
    for g in grants {
        let cats::HandAbilityGrant::HandAbilityGrant(nodes::HandAbilityGrant { card_filter, granted_warp_cost, granted_warp_cost_str }) = g;
        let _ = granted_warp_cost_str; // display form of `granted_warp_cost`
        let Some(filter) = hand_card_filter(l, card_filter) else { continue };
        // What the grant gives: warp (the only ability the class carries).
        let Some(cost) = granted_warp_cost else {
            l.unsupported("HandAbilityGrant(no granted warp cost)");
            continue;
        };
        // A granted cost with {X} would take the card's X (CR 107.3a); none
        // is lowered, so a granted cost never has one (X is 0, 107.3b).
        let cats::ManaCost::ManaCost(n) = &**cost;
        if n.x_pips != 0 {
            l.unsupported("HandAbilityGrant.granted_warp_cost.x_pips");
            continue;
        }
        let mana = super::printing::lower_mana_cost(l, "HandAbilityGrant.granted_warp_cost", cost);
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::WarpExile));
        let k = l.delayed_ability(d.finish());
        out.push(HandAltGrant { filter, alt: AltCost { mana, dash: Some(k), warp: true, ..AltCost::default() } });
    }
    out
}

/// "Artifact cards and red creature cards": a disjunction of (card type,
/// optional colour) predicates (mtg-bot's `HandCardFilter`: a card matches
/// if it matches one of them).
fn hand_card_filter(l: &mut Lowerer, f: &cats::HandCardFilter) -> Option<Filter> {
    let cats::HandCardFilter::HandCardFilter(nodes::HandCardFilter { predicates }) = f;
    if predicates.is_empty() {
        l.unsupported("HandCardFilter(empty)");
        return None;
    }
    let mut any = Vec::new();
    for (t, color) in predicates {
        let Some(bits) = types::from_name(t) else {
            l.unsupported(format!("HandCardFilter.predicates(type {t})"));
            return None;
        };
        let ty = Filter::Type(bits);
        any.push(match color {
            None => ty,
            Some(c) => match crate::mana::letter_index(c) {
                Some(i) if i < 5 => Filter::And(Box::new(ty), Box::new(Filter::Color(1 << i))),
                _ => {
                    l.unsupported(format!("HandCardFilter.predicates(colour {c})"));
                    return None;
                }
            },
        });
    }
    Some(if any.len() == 1 { any.pop().unwrap() } else { Filter::Or(any) })
}
