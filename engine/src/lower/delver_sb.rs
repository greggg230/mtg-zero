//! The Legacy Izzet Delver 75's sideboard (ENGINE.md 122): the lowering of
//! Consign to Memory's "counter target triggered ability or colorless
//! spell", Price of Progress's damage to each player by the nonbasic lands
//! they control and Rough // Tumble's halves. Same rules as `effect.rs`:
//! every field named, the rest denied; each lowers onto ops named after
//! rules actions. (Consign to Memory's replicate is `replicate_cost`,
//! `lower/printing.rs`; Hydroblast's "choose one" of "counter target spell if
//! it's red" and "destroy target permanent if it's red" lowers through the
//! existing `ModalChooseOne`, `CounterSpell` and `RemoveTargetPermanents`
//! arms, as Pyroblast's; Meltdown through `RemoveAllMatching` with
//! `max_mana_value_from_x`; Rough // Tumble's split card through
//! `split_faces`, ENGINE.md 25.)

use super::filter::lower_pred;
use super::Lowerer;
use crate::db::{types, Filter};
use crate::delver_sb::DelverSb;
use crate::program::{Amount, Builder, Ext, Op, SpellDest};
use card_ir::{cats, enums, nodes};

/// Does a filter compare an object's controller or owner with the
/// ability's controller ("you control", "an opponent controls")?
fn names_a_player(f: &Filter) -> bool {
    match f {
        Filter::Controller(_) | Filter::Owner(_) | Filter::Opponent | Filter::Player | Filter::Source => true,
        Filter::And(a, b) => names_a_player(a) || names_a_player(b),
        Filter::Or(v) => v.iter().any(names_a_player),
        Filter::Not(x) => names_a_player(x),
        _ => false,
    }
}

pub(crate) fn lower_effect_delver_sb(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Counter target triggered ability or colorless spell." (Consign to
        // Memory; CR 701.6a: the ability or spell is removed from the stack,
        // a spell put into its owner's graveyard; 115.1: the target chosen
        // as it is cast.) The kinds as `CounterTargetStackObject`'s
        // (ENGINE.md 27.2), the spells limited by `spell_filter` (abilities
        // are not spells: the filter doesn't apply to them).
        E::CounterTargetStackObjectFiltered(nodes::CounterTargetStackObjectFiltered {
            kinds,
            up_to_one,
            lose_abilities_while_source,
            spell_filter,
        }) => {
            if kinds.is_empty() {
                l.unsupported("CounterTargetStackObjectFiltered.kinds(empty)");
                return true;
            }
            let spells = kinds.contains(&enums::StackObjectKind::Spell);
            let inner = match spell_filter.as_deref() {
                None => Filter::Any,
                Some(_) if !spells => {
                    l.unsupported("CounterTargetStackObjectFiltered.spell_filter(without spells)");
                    return true;
                }
                Some(p) => match lower_pred(l, p) {
                    Some(f) => f,
                    None => return true,
                },
            };
            let filter = super::effect_stack::stack_object_filter_with(kinds, inner);
            let t = b.target(filter, if *up_to_one { 0 } else { 1 }, 1);
            if *lose_abilities_while_source {
                b.op(Op::Ext(Ext::SourceOfTargetLosesAbilities { tgt: t }));
            }
            b.op(Op::Counter { tgt: t, dest: SpellDest::Graveyard });
        }
        // "<This> deals damage to each player equal to [twice] the number of
        // <permanents> that player controls." (Price of Progress: "twice the
        // number of nonbasic lands"; CR 120.3, 608.2h, 205.4c.)
        E::DamageEachPlayerPerControlled(nodes::DamageEachPlayerPerControlled { per_filter, multiplier, label }) => {
            let _ = label; // display text ("nonbasic lands")
            let times = l.int("DamageEachPlayerPerControlled.multiplier", *multiplier, 1, 10) as u8;
            let Some(filter) = lower_pred(l, per_filter) else { return true };
            if names_a_player(&filter) {
                // "That player controls" is the grouping; a filter naming a
                // player would read it from the spell's controller.
                l.unsupported("DamageEachPlayerPerControlled.per_filter(names a player)");
                return true;
            }
            b.op(Op::Ext(Ext::DelverSb(DelverSb::DamageEachPlayerByCount { filter, times })));
        }
        // "<This> deals N damage to each creature with / without <keyword>."
        // (Rough // Tumble's halves; CR 120.3, 702.9a: flying as the
        // creatures have it now, layer 6.) One damage event per matching
        // creature, no player.
        E::DealNDamageToEachCreatureMatching(nodes::DealNDamageToEachCreatureMatching { amount, creature_filter }) => {
            let amount = l.int("DealNDamageToEachCreatureMatching.amount", *amount, 0, 99);
            let Some(f) = lower_pred(l, creature_filter) else { return true };
            let filter = Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(f));
            b.op(Op::DamageAll { amount: Amount::Fixed(amount), filter });
        }
        _ => return false,
    }
    true
}
