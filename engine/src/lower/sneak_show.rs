//! Legacy Sneak and Show (ENGINE.md 123): the lowering of Show and Tell,
//! Mistrise Village's activated ability, Brotherhood's End's first mode,
//! Omniscience's static ability and Disruptor Flute's. Same rules as
//! `effect.rs`: every field named, the rest denied; each lowers onto ops and
//! facts named after rules actions. (Brotherhood's End's second mode,
//! "destroy all artifacts with mana value 3 or less", and its "choose one"
//! lower through the existing `RemoveAllMatching` and `ModalChooseOne` arms;
//! Mistrise Village's "enters tapped unless you control a Mountain or a
//! Forest" through `enters_tapped_unless`; Magus of the Moon through
//! `nonbasic_lands_are_mountains`.)

use super::Lowerer;
use crate::db::{types, Filter};
use crate::program::{Amount, Builder, Ext, Op};
use crate::sneak_show::{SnS, SnsDef};
use card_ir::{cats, nodes};

fn sns(op: SnS) -> Op {
    Op::Ext(Ext::SnS(op))
}

pub(crate) fn lower_effect_sns(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Each player may put an artifact, creature, enchantment, or land card
        // from their hand onto the battlefield." (Show and Tell; CR 101.4.)
        E::EachPlayerMayPutFromHand(nodes::EachPlayerMayPutFromHand { card_filter }) => {
            if let Some(filter) = super::filter::lower_card_filter(l, card_filter) {
                b.op(sns(SnS::EachPlayerPutsFromHand { filter }));
            }
        }
        // "The next spell you cast this turn can't be countered." (Mistrise
        // Village; CR 101.2.)
        E::NextSpellThisTurnCantBeCountered(nodes::NextSpellThisTurnCantBeCountered {}) => {
            b.op(sns(SnS::NextSpellUncounterable));
        }
        // "<This> deals N damage to each creature and each planeswalker."
        // (Brotherhood's End.) One damage event per permanent that is a
        // creature or a planeswalker or both (CR 120.3: its results, marked
        // damage and lost loyalty, both apply to one that is both); battles
        // are not included.
        E::DealNDamageToEachCreatureAndPlaneswalker(nodes::DealNDamageToEachCreatureAndPlaneswalker { amount }) => {
            let amount = l.int("DealNDamageToEachCreatureAndPlaneswalker.amount", *amount, 0, 99);
            b.op(Op::DamageAll { amount: Amount::Fixed(amount), filter: Filter::Type(types::CREATURE | types::PLANESWALKER) });
        }
        _ => return false,
    }
    true
}

/// The Sneak and Show facts of a printing (`CardDef::sns`), and whether it
/// chooses a card name as it enters and the named spell tax of a plain
/// choice ("As this artifact enters, choose a card name. Spells with the
/// chosen name cost {3} more to cast. Activated abilities of sources with the
/// chosen name can't be activated unless they're mana abilities.":
/// `roto_statics` `ChooseCardNameAsEnters`, `SpellTax { named }`,
/// `ActivationProhibition`; CR 201.3, 614.12, 601.2f, 602.5). A tax or a
/// prohibition without a name chosen, or a name without a permanent, is
/// refused; so is "choose a nonland card name" (not built) and every other
/// tax rider.
pub(crate) fn printing_sns(
    l: &mut Lowerer,
    roto_statics: &[cats::ActivationProhibitionOrChooseCardNameAsEntersOrSpellTax],
    free_casts_from_hand: bool,
    ty: u16,
    chooses_card_name_as_enters: bool,
) -> (SnsDef, bool, u8) {
    use cats::ActivationProhibitionOrChooseCardNameAsEntersOrSpellTax as R;
    let mut out = SnsDef { free_casts_from_hand, ..Default::default() };
    // "You may cast spells from your hand without paying their mana costs"
    // (Omniscience: a permanent's static ability).
    if free_casts_from_hand && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.grants_free_casts_from_hand(not a permanent)");
    }
    let (mut name, mut tax) = (false, 0u8);
    for r in roto_statics {
        match r {
            R::ChooseCardNameAsEnters(nodes::ChooseCardNameAsEnters { nonland }) => {
                l.deny("ChooseCardNameAsEnters.nonland", nonland);
                name = true;
            }
            R::SpellTax(nodes::SpellTax { amount, opponents_only, except_during_casters_turn, from_graveyard_or_exile, named }) => {
                l.deny("SpellTax.opponents_only", opponents_only);
                l.deny("SpellTax.except_during_casters_turn", except_during_casters_turn);
                l.deny("SpellTax.from_graveyard_or_exile", from_graveyard_or_exile);
                if !*named {
                    l.unsupported("SpellTax(not named)");
                }
                tax = l.int("SpellTax.amount", *amount, 1, 9) as u8;
                out.named_tax_everyone = true;
            }
            R::ActivationProhibition(nodes::ActivationProhibition { except_mana_abilities }) => {
                out.named_ban = Some(*except_mana_abilities);
            }
        }
    }
    if (tax > 0 || out.named_ban.is_some()) && !name {
        l.unsupported("CardPrinting.roto_statics(a named tax or prohibition without a name chosen as it enters)");
    }
    if name && (ty & types::PERMANENT == 0 || chooses_card_name_as_enters) {
        l.unsupported("CardPrinting.roto_statics(ChooseCardNameAsEnters: not a permanent, or a second name choice)");
    }
    out.plain_name = name;
    (out, name, tax)
}
