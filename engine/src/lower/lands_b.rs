//! Lane B of the Legacy Lands cards (ENGINE.md 270.2): the printing facts
//! that Life from the Loam's dredge and Yavimaya, Cradle of Growth's
//! "each land is a Forest in addition to its other land types" lower to.
//! (Exploration's additional land is `CardDef::extra_lands`, Fastbond's;
//! Carnivorous Cultivator // Enroot is a preparation card, ENGINE.md 26.7.)

use super::Lowerer;
use crate::db::{types, Filter};
use card_ir::{cats, nodes};

/// `CardPrinting.zone_abilities`: Leyline of the Void's replacement
/// (`doomsday_c::lower_zone_abilities`) and dredge (CR 702.52a: "Dredge N"
/// means "As long as you have at least N cards in your library, if you would
/// draw a card, you may instead mill N cards and return this card from your
/// graveyard to your hand"). Returns (Leyline's replacement, dredge's N or
/// 0). A card has at most one of them (none of the tables has more).
pub(crate) fn lower_zone_abilities(l: &mut Lowerer, zone_abilities: &[cats::DredgeOrGraveyardExileInstead], ty: u16) -> (bool, u8) {
    let mut exile = Vec::new();
    let mut dredge = 0u8;
    for z in zone_abilities {
        match z {
            cats::DredgeOrGraveyardExileInstead::GraveyardExileInstead(g) => {
                exile.push(cats::GraveyardExileInstead::GraveyardExileInstead(g.clone()));
            }
            cats::DredgeOrGraveyardExileInstead::Dredge(nodes::Dredge { n }) => {
                if dredge != 0 {
                    l.unsupported("CardPrinting.zone_abilities(two dredge abilities)");
                }
                // (A library holds at most the 60-odd cards of a deck; a dredge
                // number past 63 would never apply.)
                dredge = l.int("Dredge.n", *n, 1, 63) as u8;
            }
        }
    }
    if dredge != 0 && !exile.is_empty() {
        l.unsupported("CardPrinting.zone_abilities(dredge with another zone ability)");
    }
    (super::doomsday_c::lower_zone_abilities(l, &exile, ty), dredge)
}

/// `CardPrinting.grants_basic_land_type_to_all_lands`: "Each land is a
/// <basic land type> in addition to its other land types" (Yavimaya, Cradle
/// of Growth; CR 305.7's last sentence, 613.1d): a static ability of a
/// permanent giving every land on the battlefield, whoever controls it, the
/// type (the land cards elsewhere are not: Yavimaya's ruling 6466518a,
/// "Land cards not on the battlefield aren't Forests"), and with it the
/// type's intrinsic mana ability (305.6: `CardDb::basic_abilities`,
/// `Game::granted_activated`). Applied by `Game::granted_subtype_ts` (EoE
/// W3's "artifacts you control are Foods"), which leaves it out while its
/// source is a nonbasic land that is a Mountain (CR 305.7, 613.8a: it
/// depends on the "nonbasic lands are Mountains" effect, which removes it).
/// True when it gives one (`eoe_w3::SEEN_W3_BASIC_TYPE`: the type brings an
/// ability).
pub(crate) fn lower_land_type_grant(l: &mut Lowerer, def: &mut crate::db::CardDef, grant: &Option<String>) -> bool {
    let Some(t) = grant.as_deref() else { return false };
    if !matches!(t, "Plains" | "Island" | "Swamp" | "Mountain" | "Forest") {
        l.unsupported(format!("CardPrinting.grants_basic_land_type_to_all_lands({t}: not a basic land type)"));
        return false;
    }
    if !def.is(types::PERMANENT) {
        l.unsupported("CardPrinting.grants_basic_land_type_to_all_lands(not a permanent)");
        return false;
    }
    let Some(sub) = l.given_subtype("CardPrinting.grants_basic_land_type_to_all_lands", t) else { return false };
    def.w3.grants_subtype.push((Filter::Type(types::LAND), sub));
    true
}
