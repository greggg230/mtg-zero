//! Lowering: card IR (`card_ir`) -> engine [`CardDef`] (ENGINE.md 3).
//!
//! Rules (so nothing is silently dropped):
//! * every node struct a lowering arm accepts is destructured exhaustively
//!   (no `..`): a new IR field breaks the build until it is handled;
//! * each field is either used or checked to be at its default with
//!   [`Lowerer::deny`] / [`Lowerer::deny_ne`], which records a blocker;
//! * an unknown variant records `Category:Tag`;
//! * all blockers of a card are collected, not just the first.

pub mod activated;
pub mod effect;
pub mod effect_copies;
pub mod effect_ext;
pub mod effect_faces;
pub mod effect_stack;
pub mod effect_tail;
pub mod effect_tail2;
pub mod effect_tail3;
pub mod effect_walkers;
pub mod effect_m7d;
pub mod eoe_b;
pub mod eoe_c;
pub mod eoe_a;
pub mod effect_eoe_d;
pub mod filter;
pub mod printing;
pub mod statics;
pub mod support;
pub mod trigger;

use crate::db::CardDef;
use card_ir::cats;

/// A field value that can be compared with the schema's plain default.
pub trait IsDefault {
    fn is_default(&self) -> bool;
}

impl IsDefault for bool {
    fn is_default(&self) -> bool {
        !*self
    }
}
impl IsDefault for i64 {
    fn is_default(&self) -> bool {
        *self == 0
    }
}
impl IsDefault for String {
    fn is_default(&self) -> bool {
        self.is_empty()
    }
}
impl<T> IsDefault for Vec<T> {
    fn is_default(&self) -> bool {
        self.is_empty()
    }
}
impl<T> IsDefault for Option<T> {
    fn is_default(&self) -> bool {
        self.is_none()
    }
}

/// Lowering context: the subtype and token interners and the blockers found
/// so far.
pub struct Lowerer<'a> {
    pub subtypes: &'a mut Vec<String>,
    /// Token definitions the cards create (`Op::CreateToken` indexes).
    pub tokens: &'a mut Vec<CardDef>,
    /// The card refers to "cards exiled with this": its exile effects
    /// link (CR 607.2a).
    pub link_exiles: bool,
    /// "<Types> your opponents control enter tapped" from a static ability.
    pub opp_enter_tapped: u16,
    /// The card's name.
    pub card_name: String,
    pub errs: Vec<String>,
    /// The colours of mana the card being lowered produces as a land
    /// (`produces`), for consistency checks.
    pub land_colors: crate::mana::ManaMask,
    /// Delayed triggered abilities the card's effects create (CR 603.7),
    /// appended to its triggers.
    pub delayed: Vec<crate::db::TriggerDef>,
    /// The faces of the card being lowered (its back face, its Adventure:
    /// CR 712, 715); `CardDef.back` / `adventure` hold their index here
    /// until `CardDb::from_cards` places them.
    pub faces: Vec<CardDef>,
    /// Which face the printing being lowered is.
    pub face: Face,
    /// Latent activated abilities the printing's effects grant it ("this
    /// Saga gains ..."), appended to its activated abilities.
    pub latent: Vec<crate::db::ActivatedDef>,
    /// An effect makes the card prepared (CR 722.3a): it needs a prepare
    /// spell.
    pub uses_prepare: bool,
    /// A predicate reads the colours of mana spent to cast the spell.
    pub converge: bool,
    /// The activated ability being lowered sacrifices one permanent as a
    /// cost ("the sacrificed permanent's mana value").
    pub sac_cost: bool,
    /// The printing says it watches its controller's library and graveyard
    /// for exiled cards (Laelia).
    pub watches_exile: bool,
    /// An effect of the card is cascade (CR 702.85a).
    pub cascade: bool,
    /// The activated ability being lowered has {X} in its mana cost.
    pub act_x: bool,
    /// The printing being lowered has an X its spell announces (its mana
    /// cost's {X}, or "pay X life").
    pub spell_x: bool,
    /// An effect of the card reanimates and attaches it (a reanimating
    /// Aura).
    pub reanimates: bool,
}

/// Which face of a card a printing describes (ENGINE.md 25).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Face {
    /// The card itself (a single-faced card, a front face, an adventurer).
    #[default]
    Card,
    /// The back face of a nonmodal (transforming) double-faced card (CR
    /// 712.2): no mana cost, colours from its colour indicator.
    Back,
    /// The back face of a modal double-faced card (CR 712.3).
    ModalBack,
    /// An Adventure (CR 715.2).
    Adventure,
    /// A half of a split card (CR 709).
    Half,
    /// A preparation card's prepare spell (CR 722.2a).
    Prepare,
    /// A Class's level (CR 716): the card's abilities at that level.
    Level,
}

impl Lowerer<'_> {
    /// Register a delayed triggered ability with this program; returns its
    /// index among the card's delayed abilities.
    pub fn delayed_ability(&mut self, ability: crate::db::AbilityDef) -> u8 {
        let uses_event = ability.uses_event();
        self.delayed.push(crate::db::TriggerDef {
            event: crate::db::EventKind::Delayed,
            pred: crate::db::TrigPred::Always,
            ability,
            uses_event,
        });
        (self.delayed.len() - 1) as u8
    }

    /// [`delayed_ability`](Self::delayed_ability) with an intervening "if"
    /// (CR 603.4): a reflexive ability that triggers only if `cond` holds,
    /// checked again as it resolves (`TrigPred::If`).
    pub fn delayed_ability_if(&mut self, ability: crate::db::AbilityDef, cond: crate::db::Cond) -> u8 {
        let k = self.delayed_ability(ability);
        self.delayed[k as usize].pred = crate::db::TrigPred::If(cond);
        k
    }
}

impl Lowerer<'_> {
    /// Record a feature the engine does not implement.
    pub fn unsupported(&mut self, what: impl Into<String>) {
        self.errs.push(what.into());
    }

    /// Record `what` unless the field is at its (plain) default.
    pub fn deny<T: IsDefault>(&mut self, what: &str, v: &T) {
        if !v.is_default() {
            self.errs.push(what.to_string());
        }
    }

    /// Record `what` unless the field equals the schema default `d`.
    pub fn deny_ne<T: PartialEq>(&mut self, what: &str, v: &T, d: &T) {
        if v != d {
            self.errs.push(what.to_string());
        }
    }

    /// The index of a token definition (equal definitions share one).
    pub fn token(&mut self, def: CardDef) -> u16 {
        match self.tokens.iter().position(|t| *t == def) {
            Some(i) => i as u16,
            None => {
                self.tokens.push(def);
                (self.tokens.len() - 1) as u16
            }
        }
    }

    pub fn subtype(&mut self, s: &str) -> u16 {
        match self.subtypes.iter().position(|x| x == s) {
            Some(i) => i as u16,
            None => {
                self.subtypes.push(s.to_string());
                (self.subtypes.len() - 1) as u16
            }
        }
    }

    /// An IR integer that must fit the engine's range.
    pub fn int(&mut self, what: &str, v: i64, lo: i64, hi: i64) -> i32 {
        if v < lo || v > hi {
            self.errs.push(format!("{what}(out of range {v})"));
            return lo as i32;
        }
        v as i32
    }
}

/// IR defects found by reading cards' IR against their Oracle text (ENGINE.md
/// 20, 24.8): what the IR lost cannot be lowered from it, so the card stays
/// blocked, with this blocker, until the IR is fixed in mtg-bot. The cube's
/// list is empty since the IR of mtg-bot feature/ir-fixes-mtgzero 8ea9fcca
/// (docs/ir_defects.md); the entries below are Edge of Eternities cards
/// (ENGINE.md 99). mtg-bot feature/eoe-ir-fixes fixed entries 20-23, 26, 40,
/// 42, 46, 48, 51, 60, 63, 64, 81, 82, Gravkill's 62, Blinkmoth Nexus' and
/// Inkmoth Nexus' 41, Dockworker Drone's and Blast Zone's 21 (ENGINE.md
/// 99.9); what is left below is still wrong in the IR.
const IR_DEFECTS: &[(&str, &str)] = &[
    // Lane A (ENGINE.md 99.2; docs/ir_defects.md 21-39).
    // "Enters with" a count of +1/+1 counters per permanent written as an
    // enters trigger (21; the schema has no such count).
    ("Luxknight Breacher", "ir_defect(enters-with counters as a trigger, docs/ir_defects.md 21)"),
    // "Can attack as though it didn't have defender" as losing defender (24).
    ("Mechan Shieldmate", "ir_defect(attack permission as losing defender, docs/ir_defects.md 24)"),
    // The counter doubler's recipients dropped (25).
    ("Loading Zone", "ir_defect(counter doubler without its recipients, docs/ir_defects.md 25)"),
    // "Move a counter" narrowed to a +1/+1 counter between your permanents
    // (27).
    ("Nesting Grounds", "ir_defect(move a counter narrowed, docs/ir_defects.md 27)"),
    // Menace for "can't be blocked by more than one creature" (28).
    ("Meltstrider's Resolve", "ir_defect(menace for \"by more than one\", docs/ir_defects.md 28)"),
    // An enchant ability narrowed to "creature" (29, 30).
    ("Tractor Beam", "ir_defect(enchant creature or Spacecraft as creature, docs/ir_defects.md 29)"),
    ("Moonlit Meditation", "ir_defect(enchant artifact or creature as creature, docs/ir_defects.md 30)"),
    // "Whenever you put ... counters" without its "you put" (31).
    ("Terrasymbiosis", "ir_defect(who puts the counters dropped, docs/ir_defects.md 31)"),

    // Lane B (ENGINE.md 99.3; docs/ir_defects.md 40-59).
    // "A 2/2 creature with all creature types": the IR has no "all creature
    // types" (docs/ir_defects.md 41).
    ("Mutavault", "ir_defect(animated land's types dropped, docs/ir_defects.md 41)"),
    // Clauses of an effect dropped (docs/ir_defects.md 43-45, 49).
    ("Divert Disaster", "ir_defect(unless-pays and Lander dropped, docs/ir_defects.md 43)"),
    ("Unravel", "ir_defect(draw clause dropped, docs/ir_defects.md 44)"),
    ("Xu-Ifit, Osteoharmonist", "ir_defect(Skeleton and no abilities dropped, docs/ir_defects.md 45)"),
    ("Pull Through the Weft", "ir_defect(lands-to-battlefield clause dropped, docs/ir_defects.md 49)"),
    // A restriction or a choice reshaped (docs/ir_defects.md 47, 50).
    ("Power Depot", "ir_defect(modular dropped, spend restriction narrowed, docs/ir_defects.md 47)"),
    ("Sunken Citadel", "ir_defect(chosen colour dropped, docs/ir_defects.md 50)"),

    // Lane C (ENGINE.md 99.4; docs/ir_defects.md 60-79).
    // "Exile target creature you control" read as "until this leaves the
    // battlefield"; the ability returning the exiled cards is an empty
    // effect (docs/ir_defects.md 61).
    ("Endless Sands", "ir_defect(linked exile and its return dropped, docs/ir_defects.md 61)"),
    // "Exile target creature or planeswalker": the target filter is a
    // creature only; the second mode's planeswalker cards dropped
    // (docs/ir_defects.md 62).
    ("Archenemy's Charm", "ir_defect(target types dropped, docs/ir_defects.md 62)"),
    // "Create a 2/2 colorless Robot artifact creature token for each ...":
    // the token by name only (docs/ir_defects.md 65, as 3).
    ("Pinnacle Starcage", "ir_defect(token printing by name only, docs/ir_defects.md 65)"),
    // "If a permanent entering the battlefield causes a triggered ability
    // of a permanent you control to trigger": both restrictions dropped
    // (docs/ir_defects.md 66).
    ("Starfield Vocalist", "ir_defect(event subjects dropped, docs/ir_defects.md 66)"),
    // "{2}{C}" / "{4}{C}" activation costs as generic {3} / {5}
    // (docs/ir_defects.md 67, as 5).
    ("Mirrorpool", "ir_defect(colorless pips as generic, docs/ir_defects.md 67)"),

    // Lane D (ENGINE.md 99.5; docs/ir_defects.md 80-99).
    // "Dies or is put into exile" widened to "leaves the battlefield", and
    // "while its power is 4 or greater" moved to resolution (80).
    ("Syr Vondam, Sunstar Exemplar", "ir_defect(dies-or-exiled widened to leaves the battlefield, docs/ir_defects.md 80)"),
];

/// Lower one card. `Err` carries every blocking feature, sorted and unique.
pub fn lower_card(
    card: &card_ir::Card,
    subtypes: &mut Vec<String>,
    tokens: &mut Vec<CardDef>,
) -> Result<(CardDef, Vec<CardDef>), Vec<String>> {
    let cats::CardPrinting::CardPrinting(p) = &card.printing;
    // A card that refers to "cards exiled with this" links its exiles.
    let link_exiles = format!("{p:?}").contains("ExiledWithSource");
    let mut l = Lowerer {
        subtypes,
        tokens,
        errs: Vec::new(),
        land_colors: 0,
        delayed: Vec::new(),
        link_exiles,
        opp_enter_tapped: 0,
        card_name: p.name.clone(),
        faces: Vec::new(),
        face: Face::Card,
        latent: Vec::new(),
        uses_prepare: false,
        converge: false,
        sac_cost: false,
        watches_exile: false,
        cascade: false,
        act_x: false,
        spell_x: false,
        reanimates: false,
    };
    let mut def = printing::lower_printing(&mut l, p);
    def.converge = l.converge;
    if l.uses_prepare && def.prepare.is_none() {
        l.unsupported("Effect:SourceBecomesPrepared(without a prepare spell)");
    }
    for &(_, blocker) in IR_DEFECTS.iter().filter(|(n, _)| *n == card.name) {
        l.unsupported(blocker);
    }
    if l.errs.is_empty() {
        Ok((def, l.faces))
    } else {
        let mut errs = l.errs;
        errs.sort();
        errs.dedup();
        Err(errs)
    }
}
