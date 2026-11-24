//! Legacy Lands, lane C (ENGINE.md 270.3): the lowering of the printings'
//! board rules (`CardPrinting.board_rules`: Choke, Torpor Orb, Damping
//! Sphere, The Tabernacle at Pendrell Vale) and of "destroy this creature
//! unless you pay {n}". Same rules as `effect.rs`: every field named, the
//! rest denied; each lowers onto facts and ops named after rules actions.
//! (Pithing Needle lowers through `sneak_show::printing_sns`, Cursed Totem
//! through `locks_activated_abilities_of`.)

use super::Lowerer;
use crate::db::{types, TriggerDef};
use crate::lands_c::{LcDef, LC};
use crate::mana::ManaCost;
use crate::program::{Builder, Ext, Op, Who};
use card_ir::{cats, nodes};

/// The board rules of a printing of card types `ty`: its lane-C facts and
/// the triggered abilities it grants every creature (appended to its
/// triggers by the caller, their slots in `LcDef::granted_to_creatures`
/// once placed). Every rule is a permanent's static ability: on a card
/// that isn't a permanent, refused.
pub(crate) fn printing_board_rules(
    l: &mut Lowerer,
    rules: &[cats::DoesntUntapOrEnteringDoesntCauseTriggersOrGrantsTriggerToCreaturesOrMultiManaLandsProduceColorlessOrSpellTaxPerOtherSpellThisTurnOrStateTrigger],
    ty: u16,
) -> (LcDef, Vec<TriggerDef>) {
    use cats::DoesntUntapOrEnteringDoesntCauseTriggersOrGrantsTriggerToCreaturesOrMultiManaLandsProduceColorlessOrSpellTaxPerOtherSpellThisTurnOrStateTrigger as R;
    let mut out = LcDef::default();
    let mut granted = Vec::new();
    // (A state trigger, Dark Depths', is lowered by `lands_a::lower_board_rules`, ENGINE.md 270.1, 270.4.)
    let statics = rules.iter().filter(|r| !matches!(r, R::StateTrigger(_))).count();
    if statics > 0 && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.board_rules(not a permanent)");
        return (out, granted);
    }
    for r in rules {
        match r {
            // "<Permanents> don't untap during their controllers' untap
            // steps." (CR 502.3; Choke: Islands.)
            R::DoesntUntap(nodes::DoesntUntap { permanent_filter, label: _ }) => {
                if out.no_untap.is_some() {
                    l.unsupported("CardPrinting.board_rules(two DoesntUntap)");
                }
                out.no_untap = super::filter::lower_pred(l, permanent_filter);
            }
            // "<Permanents> entering don't cause abilities to trigger."
            // (Torpor Orb: creatures; CR 603.2, 603.6a.)
            R::EnteringDoesntCauseTriggers(nodes::EnteringDoesntCauseTriggers { permanent_filter, label: _ }) => {
                if out.entering_silent.is_some() {
                    l.unsupported("CardPrinting.board_rules(two EnteringDoesntCauseTriggers)");
                }
                out.entering_silent = super::filter::lower_pred(l, permanent_filter);
            }
            // "If a land is tapped for two or more mana, it produces {C}
            // instead of any other type and amount." (CR 106.12b.)
            R::MultiManaLandsProduceColorless(nodes::MultiManaLandsProduceColorless {}) => {
                out.damp_lands = true;
            }
            // "Each spell a player casts costs {n} more to cast for each other
            // spell that player has cast this turn." (CR 601.2f.)
            R::SpellTaxPerOtherSpellThisTurn(nodes::SpellTaxPerOtherSpellThisTurn { amount }) => {
                out.tax_per_other_spell = l.int("SpellTaxPerOtherSpellThisTurn.amount", *amount, 1, 9) as u8;
            }
            // 'All creatures have "<triggered ability>".' (The Tabernacle at
            // Pendrell Vale; CR 113.7, 604.1): the ability, a trigger of this
            // definition that every creature has.
            R::GrantsTriggerToCreatures(nodes::GrantsTriggerToCreatures { ability }) => {
                if let Some(t) = super::trigger::lower_trigger(l, ability, types::CREATURE) {
                    // (A step's beginning, the granting permanent and the
                    // creatures on the battlefield as it begins: the
                    // abilities `Game::event` fires per creature. Another
                    // event - a creature leaving, entering - is refused.)
                    use crate::db::EventKind as K;
                    if !matches!(
                        t.event,
                        K::BeginningOfUpkeep | K::BeginningOfDrawStep | K::BeginningOfPrecombatMain | K::BeginningOfCombat | K::BeginningOfEndStep
                    ) {
                        l.unsupported(format!("GrantsTriggerToCreatures.ability({:?})", t.event));
                    }
                    granted.push(t);
                }
            }
            // A state-triggered ability (Dark Depths'): `lands_a::lower_board_rules`.
            R::StateTrigger(_) => {}
        }
    }
    (out, granted)
}

pub(crate) fn lower_effect_lands_c(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Destroy this creature unless you pay {1}." (The Tabernacle's
        // granted ability; CR 118.12: its controller may pay as it resolves,
        // then the source is destroyed unless they did.)
        E::DestroySourceUnlessPay(nodes::DestroySourceUnlessPay { cost }) => {
            let Some(cost) = ManaCost::parse(cost) else {
                l.unsupported(format!("DestroySourceUnlessPay.cost({cost})"));
                return true;
            };
            let pay = b.op(Op::OfferPay { who: Who::You, cost, if_paid: 0 });
            b.op(Op::Ext(Ext::LandsC(LC::DestroySource)));
            b.patch(pay);
        }
        _ => return false,
    }
    true
}
