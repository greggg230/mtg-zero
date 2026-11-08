//! Edge of Eternities, workstream W3 (ENGINE.md 99.13): combat and
//! abilities. The costs, effect classes and printing flags of the set's last
//! combat cards. Same rules as `effect.rs`: every field named, the rest
//! denied. Most classes are named after a card and carry no fields (their
//! meaning is the class's Oracle text, mtg-bot's docstring;
//! docs/ir_defects.md 11): each lowers onto ops named after rules actions,
//! its targets derived here.

use super::Lowerer;
use crate::db::{types, Cost, Counter, Filter};
use crate::eoe_w3::W3;
use crate::program::{Builder, Ext, On, Op};
use card_ir::{cats, nodes};

/// The cost atoms of `ActivationCost` this workstream implements.
pub(crate) struct CostW3 {
    pub exile_artifact_from_graveyard: bool,
    pub remove_plus_one_counter_from_your_creature: bool,
    pub tap_x_untapped_artifacts_you_control: bool,
}

/// Lower the cost atoms of [`CostW3`] into `cost`.
pub(crate) fn lower_cost_w3(l: &mut Lowerer, cost: &mut Cost, c: CostW3) {
    let CostW3 { exile_artifact_from_graveyard, remove_plus_one_counter_from_your_creature, tap_x_untapped_artifacts_you_control } = c;
    // "Exile an artifact card from your graveyard" (CR 118.3, 602.2b: a card
    // chosen as the cost is paid; its printed types, 400.7).
    if exile_artifact_from_graveyard {
        if cost.exile_graveyard > 0 {
            l.unsupported("ActivationCost.exile_artifact_from_graveyard(with another graveyard exile)");
        }
        cost.exile_graveyard = 1;
        cost.exile_graveyard_types = types::ARTIFACT;
    }
    // "Remove a +1/+1 counter from a creature you control" (CR 118.3,
    // 602.2b).
    if remove_plus_one_counter_from_your_creature {
        if cost.crew > 0 {
            l.unsupported("ActivationCost.remove_plus_one_counter_from_your_creature(with crew)");
        }
        cost.remove_counter_from = Some((Filter::Type(types::CREATURE), Counter::P1P1));
    }
    // "Tap X untapped artifacts you control" (CR 107.3a, 601.2b: X is
    // announced as the ability is activated; the effect must read it).
    if tap_x_untapped_artifacts_you_control {
        if l.act_x || cost.crew > 0 {
            l.unsupported("ActivationCost.tap_x_untapped_artifacts_you_control(with {X} or crew)");
        }
        l.act_x = true;
        cost.tap_x = Some(Filter::Type(types::ARTIFACT));
    }
}

fn w3(b: &mut Builder, op: W3) -> usize {
    b.op(Op::Ext(Ext::EoeW3(op)))
}

/// `op` (a destruction), with "it can't be regenerated" if `no_regen` (CR
/// 701.19c: the flag is set around it).
pub(crate) fn cant_be_regenerated(b: &mut Builder, no_regen: bool, op: Op) {
    if no_regen {
        w3(b, W3::CantBeRegenerated(true));
    }
    b.op(op);
    if no_regen {
        w3(b, W3::CantBeRegenerated(false));
    }
}

/// Lower an effect class of workstream W3; `false` if the class is not one
/// of these.
pub(crate) fn lower_effect_w3(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Regenerate target Insect, Rat, Spider, or Squirrel" (CR 701.19a;
        // the target: a permanent with one of the subtypes).
        E::RegenerateTargetCreature(nodes::RegenerateTargetCreature { allowed_subtypes }) => {
            let filter = if allowed_subtypes.is_empty() {
                Filter::Type(types::CREATURE)
            } else {
                Filter::Or(allowed_subtypes.iter().map(|s| Filter::Subtype(l.subtype(s))).collect())
            };
            let t = b.target(filter, 1, 1);
            w3(b, W3::Regenerate { on: On::Target(t) });
        }
        _ => return false,
    }
    true
}
