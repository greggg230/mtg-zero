//! Lane B of the Legacy Lands cards (ENGINE.md 270.2): dredge (CR 702.52),
//! a replacement of a card draw. Named after the rules, never after a card.
//! (Yavimaya's land type is EoE W3's subtype grant, `Game::granted_subtype_ts`;
//! Exploration's additional land is `CardDef::extra_lands`; Carnivorous
//! Cultivator // Enroot is a preparation card, ENGINE.md 26.7.)

/// The printing facts lane B adds (`CardDef::lands_b`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct LandsBDef {
    /// "Dredge N" (CR 702.52a): N, 0 = none. A static ability of the card
    /// while it is in a graveyard.
    pub dredge: u8,
}
