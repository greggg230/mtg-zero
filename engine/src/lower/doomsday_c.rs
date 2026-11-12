//! Lane C of the Legacy Doomsday cards (ENGINE.md 124.3): Leyline of the
//! Void's replacement effect. (Street Wraith's and Edge of Autumn's cycling
//! costs are cost atoms, `activated.rs`; landwalk is a keyword,
//! `printing.rs`.)

use super::Lowerer;
use crate::db::types;
use card_ir::{cats, nodes};

/// `CardPrinting.zone_abilities`: "If a card would be put into an
/// opponent's graveyard from anywhere, exile it instead" (Leyline of the
/// Void; CR 614.1a): a static ability of a permanent, a replacement effect
/// on cards only (not tokens, not copies of spells: CR 108.2, 111.1,
/// 707.10a) owned by an opponent of its controller, from any zone
/// (`Game::opp_gy_exile_for`). The IR's other forms (every graveyard; tokens
/// too, as Rest in Peace's "a card or token"; a colour) are refused loudly.
pub(crate) fn lower_zone_abilities(l: &mut Lowerer, zone_abilities: &[cats::GraveyardExileInstead], ty: u16) -> bool {
    let mut out = false;
    for z in zone_abilities {
        let cats::GraveyardExileInstead::GraveyardExileInstead(nodes::GraveyardExileInstead { opponents_only, tokens, colors }) = z;
        l.deny("GraveyardExileInstead.colors", colors);
        if !*opponents_only {
            l.unsupported("GraveyardExileInstead(every graveyard)");
        }
        if *tokens {
            l.unsupported("GraveyardExileInstead.tokens");
        }
        if ty & types::PERMANENT == 0 {
            l.unsupported("GraveyardExileInstead(not a permanent card)");
        }
        if out {
            l.unsupported("CardPrinting.zone_abilities(more than one)");
        }
        out = true;
    }
    out
}
