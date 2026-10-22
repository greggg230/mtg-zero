//! Activated abilities, their costs and mana abilities -> [`ActivatedDef`]
//! (CR 602, 605, 118; ENGINE.md 18).

use super::effect::lower_ability;
use super::filter::{lower_condition, lower_pred};
use super::printing::lower_mana_cost;
use super::Lowerer;
use crate::db::{types, ActEffect, ActivatedDef, Cond, Cost, Filter, ManaAbility, ManaProd, SacCost};
use crate::mana::{letter_index, ManaMask, COLORS};
use card_ir::{cats, enums, nodes};

/// Where an ability is activated from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum From {
    Battlefield,
    Hand,
}

pub(crate) fn lower_activated(l: &mut Lowerer, a: &cats::ActivatedAbility, from: From) -> Option<ActivatedDef> {
    let cats::ActivatedAbility::ActivatedAbility(nodes::ActivatedAbility {
        cost,
        effect,
        sorcery_speed,
        is_mana_ability,
        cost_reductions,
        room_half,
        tier_threshold,
        activate_only_once,
        activate_only_if,
        activate_only_as_instant,
        activate_only_during_your_turn,
        exhaust,
        boast,
    }) = a;
    l.deny("ActivatedAbility.cost_reductions", cost_reductions);
    l.deny("ActivatedAbility.room_half", room_half);
    l.deny("ActivatedAbility.tier_threshold", tier_threshold);
    l.deny("ActivatedAbility.activate_only_once", activate_only_once);
    l.deny("ActivatedAbility.exhaust", exhaust);
    l.deny("ActivatedAbility.boast", boast);
    let mut only_if = Vec::new();
    let cost = lower_cost(l, cost, from, &mut only_if);
    if let Some(c) = activate_only_if {
        if let Some(c) = lower_condition(l, c) {
            only_if.push(c);
        }
    }
    let effect = if *is_mana_ability {
        let m = lower_mana_ability(l, effect)?;
        // Mana abilities are activated while paying a cost (ENGINE.md 18):
        // only costs without choices, and no mana cost of their own (a
        // nested payment) unless activated only with priority.
        if !*activate_only_as_instant {
            if !cost.mana.is_zero() {
                l.unsupported("ManaAbility.cost.mana");
            }
            if cost.sacrifice.is_some() || cost.discard > 0 || cost.discard_hand {
                l.unsupported("ManaAbility.cost(with a choice)");
            }
            if cost.life > 0 && !matches!(m.prod, ManaProd::One(_) | ManaProd::ColorAmong(_)) {
                l.unsupported("ManaAbility.cost.life(more than one mana)");
            }
        }
        if *sorcery_speed {
            l.unsupported("ManaAbility.sorcery_speed");
        }
        // (The mana-ability payment path pays none of these.)
        if cost.remove_counters.is_some()
            || cost.crew > 0
            || cost.exile_graveyard > 0
            || cost.bounce.is_some()
            || cost.exile_from_hand.is_some()
        {
            l.unsupported("ManaAbility.cost(counters, crew, exile or bounce)");
        }
        // Payment treats a card's mana abilities as exclusive alternatives,
        // true when each one taps, sacrifices or exiles its source.
        if !(cost.tap || cost.sac_self || cost.exile_self || cost.discard_self) {
            l.unsupported("ManaAbility.cost(no tap or sacrifice of the source)");
        }
        if from == From::Hand && !matches!(m.prod, ManaProd::One(_)) {
            l.unsupported("ManaAbility(from hand, more than one mana)");
        }
        ActEffect::Mana(m)
    } else {
        if *activate_only_as_instant {
            l.unsupported("ActivatedAbility.activate_only_as_instant(non-mana)");
        }
        ActEffect::Stack(lower_ability(l, effect))
    };
    if only_if.len() > 1 {
        l.unsupported("ActivatedAbility.only_if(more than one condition)");
    }
    Some(ActivatedDef {
        cost,
        effect,
        sorcery: *sorcery_speed,
        instant_only: *activate_only_as_instant,
        only_if: only_if.into_iter().next(),
        your_turn: *activate_only_during_your_turn,
    })
}

/// `ActivationCost` -> [`Cost`]; "activate only if" gates hidden in the
/// cost struct go to `only_if`.
fn lower_cost(l: &mut Lowerer, c: &cats::ActivationCost, from: From, only_if: &mut Vec<Cond>) -> Cost {
    let cats::ActivationCost::ActivationCost(nodes::ActivationCost {
        mana,
        mana_str,
        tap_self,
        sacrifice_self,
        exile_self,
        discard_a_card,
        pay_energy,
        tap_permanent_you_control,
        tap_permanent_type,
        exile_artifact_from_graveyard,
        exile_cards_from_graveyard,
        return_unblocked_attacker,
        sacrifice_another_creature_or_artifact,
        sacrifice_another_artifact_or_land,
        sacrifice_another_artifact,
        remove_plus_one_counter_from_your_creature,
        sacrifice_food,
        exile_granting_equipment,
        mana_reduced_by_source_power,
        pay_life,
        sacrifice_a_land,
        sacrifice_a_desert,
        requires_own_creature_power_at_least,
        requires_own_artifact_count_at_least,
        requires_control_of_subtype,
        tap_x_untapped_artifacts_you_control,
        discard_hand,
        sacrifice_subtype,
        sacrifice_nontoken_artifact,
        remove_counters_from_self_kind,
        remove_counters_from_self_amount,
        crew,
        saddle,
        sacrifice_n_nonland_permanents,
        loyalty,
        loyalty_x,
        discard_self,
        remove_named_counter_from_self,
        exert_self,
        sacrifice_a_permanent_type,
        extra_costs,
        waterbend,
    }) = c;
    let _ = mana_str; // display form of `mana`
    l.deny("ActivationCost.pay_energy", pay_energy);
    l.deny("ActivationCost.tap_permanent_you_control", tap_permanent_you_control);
    l.deny("ActivationCost.tap_permanent_type", tap_permanent_type);
    l.deny("ActivationCost.exile_artifact_from_graveyard", exile_artifact_from_graveyard);
    l.deny("ActivationCost.return_unblocked_attacker", return_unblocked_attacker);
    l.deny("ActivationCost.remove_plus_one_counter_from_your_creature", remove_plus_one_counter_from_your_creature);
    l.deny("ActivationCost.sacrifice_food", sacrifice_food);
    l.deny("ActivationCost.exile_granting_equipment", exile_granting_equipment);
    l.deny("ActivationCost.mana_reduced_by_source_power", mana_reduced_by_source_power);
    l.deny("ActivationCost.tap_x_untapped_artifacts_you_control", tap_x_untapped_artifacts_you_control);
    l.deny("ActivationCost.saddle", saddle);
    l.deny("ActivationCost.loyalty", loyalty);
    l.deny("ActivationCost.loyalty_x", loyalty_x);
    l.deny("ActivationCost.exert_self", exert_self);
    l.deny("ActivationCost.waterbend", waterbend);

    let mut cost = Cost {
        mana: match mana {
            Some(m) => lower_mana_cost(l, "ActivationCost.mana", m),
            None => Default::default(),
        },
        tap: *tap_self,
        life: l.int("ActivationCost.pay_life", *pay_life, 0, 99) as u8,
        sac_self: *sacrifice_self,
        exile_self: *exile_self,
        discard_self: *discard_self,
        discard_hand: *discard_hand,
        sacrifice: None,
        discard: *discard_a_card as u8,
        exile_from_hand: None,
        bounce: None,
        exile_graveyard: l.int("ActivationCost.exile_cards_from_graveyard", *exile_cards_from_graveyard, 0, 20) as u8,
        remove_counters: None,
        crew: l.int("ActivationCost.crew", *crew, 0, 20) as u8,
    };
    // "Remove a <name> counter from this."
    if !remove_named_counter_from_self.is_empty() {
        let name = remove_named_counter_from_self;
        if cost.remove_counters.is_some() {
            l.unsupported("ActivationCost.remove_named_counter_from_self(with another removal)");
        }
        cost.remove_counters = Some((super::effect_ext::counter_by_name(l, name), 1));
    }
    // "Remove n <kind> counters from this" (CR 118.3).
    match (remove_counters_from_self_kind.as_str(), *remove_counters_from_self_amount) {
        ("", 0) => {}
        (k, n) if !k.is_empty() && n > 0 => {
            let counter = super::effect_ext::counter_by_name(l, k);
            cost.remove_counters = Some((counter, l.int("ActivationCost.remove_counters_from_self_amount", n, 1, 20) as u8));
        }
        _ => l.unsupported("ActivationCost.remove_counters_from_self(kind and amount)"),
    }
    match from {
        From::Battlefield => {
            l.deny("ActivationCost.exile_self(on the battlefield)", exile_self);
            l.deny("ActivationCost.discard_self(on the battlefield)", discard_self);
        }
        From::Hand => {
            l.deny("ActivationCost.tap_self(in hand)", tap_self);
            l.deny("ActivationCost.sacrifice_self(in hand)", sacrifice_self);
        }
    }

    // Sacrifice costs: each flag names a filter; at most one per cost.
    let mut sacs: Vec<SacCost> = Vec::new();
    let t = |bits: u16| Filter::Type(bits);
    if *sacrifice_another_creature_or_artifact {
        sacs.push(SacCost { filter: t(types::CREATURE | types::ARTIFACT), n: 1, another: true });
    }
    if *sacrifice_another_artifact_or_land {
        sacs.push(SacCost { filter: t(types::ARTIFACT | types::LAND), n: 1, another: true });
    }
    if *sacrifice_another_artifact {
        sacs.push(SacCost { filter: t(types::ARTIFACT), n: 1, another: true });
    }
    if *sacrifice_nontoken_artifact {
        let nontoken = Filter::Not(Box::new(Filter::Token));
        sacs.push(SacCost { filter: Filter::And(Box::new(t(types::ARTIFACT)), Box::new(nontoken)), n: 1, another: false });
    }
    if *sacrifice_a_land {
        sacs.push(SacCost { filter: t(types::LAND), n: 1, another: false });
    }
    if *sacrifice_a_desert {
        sacs.push(SacCost { filter: Filter::Subtype(l.subtype("Desert")), n: 1, another: false });
    }
    if !sacrifice_subtype.is_empty() {
        sacs.push(SacCost { filter: Filter::Subtype(l.subtype(sacrifice_subtype)), n: 1, another: false });
    }
    if *sacrifice_n_nonland_permanents > 0 {
        let n = l.int("ActivationCost.sacrifice_n_nonland_permanents", *sacrifice_n_nonland_permanents, 1, 9) as u8;
        sacs.push(SacCost {
            filter: Filter::And(Box::new(t(types::PERMANENT)), Box::new(Filter::Not(Box::new(t(types::LAND))))),
            n,
            another: false,
        });
    }
    if let Some(w) = sacrifice_a_permanent_type {
        let bits = match w {
            enums::CardTypeWord::Artifact => types::ARTIFACT,
            enums::CardTypeWord::Battle => types::BATTLE,
            enums::CardTypeWord::Creature => types::CREATURE,
            enums::CardTypeWord::Enchantment => types::ENCHANTMENT,
            enums::CardTypeWord::Land => types::LAND,
            enums::CardTypeWord::Planeswalker => types::PLANESWALKER,
            enums::CardTypeWord::Instant | enums::CardTypeWord::Sorcery => {
                l.unsupported(format!("ActivationCost.sacrifice_a_permanent_type({w:?})"));
                0
            }
        };
        sacs.push(SacCost { filter: t(bits), n: 1, another: false });
    }
    for x in extra_costs {
        match x {
            cats::CostAtom::SacrificePermanentsCost(nodes::SacrificePermanentsCost {
                permanent_filter,
                count,
                another,
                label,
            }) => {
                let _ = label; // display text
                let n = l.int("SacrificePermanentsCost.count", *count, 1, 9) as u8;
                if let Some(f) = lower_pred(l, permanent_filter) {
                    sacs.push(SacCost { filter: f, n, another: *another });
                }
            }
            // "Remove n +1/+1 counters from this" (Walking Ballista).
            cats::CostAtom::RemoveCountersFromSelfCost(nodes::RemoveCountersFromSelfCost { count }) => {
                if cost.remove_counters.is_some() {
                    l.unsupported("CostAtom:RemoveCountersFromSelfCost(twice)");
                }
                let n = l.int("RemoveCountersFromSelfCost.count", *count, 1, 20) as u8;
                cost.remove_counters = Some((crate::db::Counter::P1P1, n));
            }
            other => l.unsupported(format!("CostAtom:{}", other.tag())),
        }
    }
    if sacs.len() > 1 {
        l.unsupported("ActivationCost.sacrifice(more than one kind)");
    }
    cost.sacrifice = sacs.into_iter().next();
    // A payment reserves candidates for one later choice (ENGINE.md 18).
    if cost.sacrifice.is_some() && cost.discard > 0 {
        l.unsupported("ActivationCost.sacrifice(with a discard)");
    }

    // "Activate only if ..." gates stored on the cost.
    if *requires_own_creature_power_at_least > 0 {
        let n = l.int("ActivationCost.requires_own_creature_power_at_least", *requires_own_creature_power_at_least, 1, 99);
        only_if.push(Cond::ControlAtLeast(
            Filter::And(Box::new(t(types::CREATURE)), Box::new(Filter::PowerAtLeast(n))),
            1,
        ));
    }
    if *requires_own_artifact_count_at_least > 0 {
        let n = l.int("ActivationCost.requires_own_artifact_count_at_least", *requires_own_artifact_count_at_least, 1, 99);
        only_if.push(Cond::ControlAtLeast(t(types::ARTIFACT), n as u8));
    }
    if !requires_control_of_subtype.is_empty() {
        let fs: Vec<Filter> = requires_control_of_subtype.iter().map(|s| Filter::Subtype(l.subtype(s))).collect();
        only_if.push(Cond::ControlAtLeast(Filter::Or(fs), 1));
    }
    cost
}

/// A mask from IR colour letters ("any" = the five colours).
fn mask_of(l: &mut Lowerer, what: &str, syms: &[String]) -> ManaMask {
    let mut m = 0;
    for s in syms {
        match s.as_str() {
            "any" => m |= COLORS,
            x => match letter_index(x) {
                Some(i) => m |= 1 << i,
                None => l.unsupported(format!("{what}({x})")),
            },
        }
    }
    m
}

/// `[(symbol, n)]` -> a production: one mana of a mask, or fixed amounts.
fn lower_produces(l: &mut Lowerer, what: &str, produces: &[(String, i64)]) -> Option<ManaProd> {
    let mut fixed = [0u8; 6];
    let mut any = 0;
    for (sym, n) in produces {
        let n = l.int(what, *n, 0, 20) as u8;
        if sym == "any" {
            any += n;
        } else {
            match letter_index(sym) {
                Some(i) => fixed[i] += n,
                None => {
                    l.unsupported(format!("{what}({sym})"));
                    return None;
                }
            }
        }
    }
    let total: u8 = fixed.iter().sum::<u8>() + any;
    if any > 0 && total > any {
        l.unsupported(format!("{what}(any mixed with fixed)"));
        return None;
    }
    Some(match (any, total) {
        (0, 1) => ManaProd::One(1 << fixed.iter().position(|&n| n > 0).unwrap()),
        (0, _) => ManaProd::Fixed(fixed),
        (1, _) => ManaProd::One(COLORS),
        // "Add N mana of any colors" mints N independent wildcards.
        (n, _) => ManaProd::Combo { mask: COLORS, n },
    })
}

/// The effect of a mana ability -> [`ManaAbility`] (its production plus
/// damage its source deals to you).
fn lower_mana_ability(l: &mut Lowerer, e: &cats::Effect) -> Option<ManaAbility> {
    use cats::Effect as E;
    let mut damage_to_you = 0u8;
    let main = match e {
        E::Sequence(nodes::Sequence { effects }) => {
            let [first, rest @ ..] = &effects[..] else {
                l.unsupported("ManaAbility.Sequence(empty)");
                return None;
            };
            for x in rest {
                match x {
                    E::SourceDealsDamageToController(nodes::SourceDealsDamageToController { amount }) => {
                        damage_to_you += l.int("SourceDealsDamageToController.amount", *amount, 0, 20) as u8;
                    }
                    other => l.unsupported(format!("ManaAbility:{}", other.tag())),
                }
            }
            first
        }
        other => other,
    };
    let prod = match main {
        E::AddManaToPool(nodes::AddManaToPool {
            produces,
            spend_restriction,
            grants_haste_to_creature_spell,
            makes_spell_uncounterable,
        }) => {
            l.deny("AddManaToPool.spend_restriction", spend_restriction);
            l.deny("AddManaToPool.grants_haste_to_creature_spell", grants_haste_to_creature_spell);
            l.deny("AddManaToPool.makes_spell_uncounterable", makes_spell_uncounterable);
            lower_produces(l, "AddManaToPool.produces", produces)?
        }
        E::AddNManaOfChosenColor(nodes::AddNManaOfChosenColor { amount, options }) => {
            let n = l.int("AddNManaOfChosenColor.amount", *amount, 1, 20) as u8;
            let mask = mask_of(l, "AddNManaOfChosenColor.options", options);
            if n == 1 {
                ManaProd::One(mask)
            } else {
                ManaProd::SameType { mask, n }
            }
        }
        E::AddManaInAnyCombination(nodes::AddManaInAnyCombination { amount, options, amount_is_x }) => {
            l.deny("AddManaInAnyCombination.amount_is_x", amount_is_x);
            let n = l.int("AddManaInAnyCombination.amount", *amount, 1, 20) as u8;
            let mask = mask_of(l, "AddManaInAnyCombination.options", options);
            if n == 1 {
                ManaProd::One(mask)
            } else {
                ManaProd::Combo { mask, n }
            }
        }
        E::AddManaOfColorPerCount(nodes::AddManaOfColorPerCount { color, amount }) => {
            let ty = match letter_index(color) {
                Some(i) => i as u8,
                None => {
                    l.unsupported(format!("AddManaOfColorPerCount.color({color})"));
                    return None;
                }
            };
            match &**amount {
                cats::Formula::CountControlledMatching(nodes::CountControlledMatching { predicate }) => {
                    ManaProd::PerPermanent { ty, filter: lower_pred(l, predicate)? }
                }
                other => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return None;
                }
            }
        }
        E::AddOneManaOfColorAmongControlled(nodes::AddOneManaOfColorAmongControlled {
            card_types,
            legendary,
            group_description,
        }) => {
            let _ = group_description; // display text
            let mut bits = 0u16;
            for t in card_types {
                match types::from_name(t) {
                    Some(b) => bits |= b,
                    None => l.unsupported(format!("AddOneManaOfColorAmongControlled.card_types({t})")),
                }
            }
            let mut f = if bits == 0 { Filter::Type(types::PERMANENT) } else { Filter::Type(bits) };
            if *legendary {
                f = Filter::And(
                    Box::new(f),
                    Box::new(Filter::Supertype(crate::db::supertypes::LEGENDARY)),
                );
            }
            ManaProd::ColorAmong(f)
        }
        E::DealDamageToControllerAndAddMana(nodes::DealDamageToControllerAndAddMana { damage, produces }) => {
            damage_to_you += l.int("DealDamageToControllerAndAddMana.damage", *damage, 0, 20) as u8;
            lower_produces(l, "DealDamageToControllerAndAddMana.produces", produces)?
        }
        other => {
            l.unsupported(format!("ManaAbility:{}", other.tag()));
            return None;
        }
    };
    Some(ManaAbility { prod, damage_to_you })
}

/// The types a production can make (which `produces` colours it covers).
pub(crate) fn prod_mask(p: &ManaProd) -> ManaMask {
    match p {
        ManaProd::One(m) | ManaProd::SameType { mask: m, .. } | ManaProd::Combo { mask: m, .. } => *m,
        ManaProd::Fixed(v) => (0..6).filter(|&i| v[i] > 0).fold(0, |m, i| m | (1 << i)),
        ManaProd::PerPermanent { ty, .. } => 1 << ty,
        ManaProd::ColorAmong(_) => COLORS,
    }
}
