//! Planeswalkers' effects (M7b; ENGINE.md 24) -> programs. Same rules as
//! `effect.rs`: every field named, the rest denied.

use super::effect::{lower_effect, lower_target_spec, lower_targets};
use super::effect_ext::{counter_by_name, keyword_mods};
use super::Lowerer;
use crate::db::{types, CardDef, Counter, Filter, Mod, Scope};
use crate::layers::Until;
use crate::program::{Amount, Builder, Ext, On, Op, PileDest, Who};
use card_ir::{cats, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn not(a: Filter) -> Filter {
    Filter::Not(Box::new(a))
}

/// "Any target" (CR 115.4): a creature, a planeswalker, a battle or a
/// player, as the IR's own any-target filter lowers.
pub(crate) fn any_target() -> Filter {
    Filter::Or(vec![
        Filter::Type(types::CREATURE),
        Filter::Type(types::PLANESWALKER),
        Filter::Type(types::BATTLE),
        Filter::Player,
    ])
}

fn apply(b: &mut Builder, on: On, mods: Vec<Mod>, until: Until) {
    b.op(Op::Ext(Ext::Apply { on, mods, until }));
}

fn opt_card_filter(l: &mut Lowerer, f: &Option<Box<cats::CardFilter>>) -> Option<Filter> {
    match f {
        Some(f) => super::filter::lower_card_filter(l, f),
        None => Some(Filter::Any),
    }
}

/// Lower an M7b effect class; `false` if the class is not one of these.
pub(crate) fn lower_effect_walkers(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        E::CreateEmblemWithAbilities(nodes::CreateEmblemWithAbilities {
            name,
            triggered_abilities,
            static_abilities,
            grants_retrace_to,
        }) => {
            // "You get an emblem with <abilities>" (CR 114.2): a definition
            // with abilities only, put into the command zone.
            let _ = name; // display text
            let delayed = l.delayed.len();
            let mut triggers = Vec::new();
            for t in triggered_abilities {
                if let Some(td) = super::trigger::lower_trigger(l, t, 0) {
                    triggers.push(td);
                }
            }
            if l.delayed.len() != delayed {
                l.unsupported("CreateEmblemWithAbilities(a delayed ability)");
            }
            let mut repl = super::statics::Repl::default();
            let mut statics = Vec::new();
            for s in static_abilities {
                if let Some(st) = super::statics::lower_static(l, s, &mut repl) {
                    if !matches!(st.scope, Scope::Matching { .. }) {
                        l.unsupported("CreateEmblemWithAbilities.static_abilities(scope)");
                    }
                    statics.push(st);
                }
            }
            if repl.token_factor != 0 || repl.wins_on_empty_draw {
                l.unsupported("CreateEmblemWithAbilities(replacement effect)");
            }
            let grants_retrace = match grants_retrace_to {
                Some(f) => super::filter::lower_card_filter(l, f),
                None => None,
            };
            if triggers.is_empty() && statics.is_empty() && grants_retrace.is_none() {
                l.unsupported("CreateEmblemWithAbilities(no ability)");
            }
            let trig_mask = triggers.iter().fold(0u32, |m, t| m | 1 << t.event as u32);
            let def = CardDef {
                name: format!("{} emblem", l.card_name),
                emblem: true,
                delayed_base: triggers.len() as u8,
                trig_mask,
                triggers,
                statics,
                grants_retrace,
                ..Default::default()
            };
            let emblem = l.token(def);
            b.op(Op::Ext(Ext::CreateEmblem { emblem }));
        }
        E::RollDieTable(nodes::RollDieTable { sides, table, gate }) => {
            // CR 706.3a: roll, then the effect of the result's range.
            // "Name Sticker" Goblin's gate: "if it's on the battlefield and
            // you control 9 or fewer creatures named ..." as it resolves
            // (CR 603.4).
            let gate_at = match gate.as_deref() {
                None => None,
                Some(cats::Condition::NameStickerGate(nodes::NameStickerGate { name, at_most })) => {
                    let n = l.int("NameStickerGate.at_most", *at_most, 0, 99) as u8;
                    let on_bf = b.op(Op::Ext(Ext::JumpUnlessSource { filter: Filter::Any, to: 0 }));
                    let f = Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Named(name.clone())));
                    let count = b.op(Op::JumpUnless { cond: crate::db::Cond::ControlAtMost(f, n), to: 0 });
                    Some((on_bf, count))
                }
                Some(other) => {
                    l.unsupported(format!("RollDieTable.gate({})", other.tag()));
                    None
                }
            };
            let sides = l.int("RollDieTable.sides", *sides, 2, 20) as u8;
            let targets = b.targets.len();
            let at = b.op(Op::Ext(Ext::RollDie { sides, table: Vec::new() }));
            let none = b.op(Op::Jump { to: 0 });
            let mut entries = Vec::new();
            let mut ends = Vec::new();
            for (lo, hi, eff) in table {
                let lo = l.int("RollDieTable.table.lo", *lo, 1, sides as i64) as u8;
                let hi = l.int("RollDieTable.table.hi", *hi, lo as i64, sides as i64) as u8;
                entries.push((lo, hi, b.here()));
                lower_effect(l, b, eff);
                ends.push(b.op(Op::Jump { to: 0 }));
            }
            if b.targets.len() != targets {
                // (A result's targets would be chosen before the roll.)
                l.unsupported("RollDieTable.table(targets in a result)");
            }
            if let Op::Ext(Ext::RollDie { table, .. }) = &mut b.program[at] {
                *table = entries;
            }
            b.patch(none);
            for x in ends {
                b.patch(x);
            }
            if let Some((a, c)) = gate_at {
                b.patch(a);
                b.patch(c);
            }
        }
        E::AdjustSourceLoyalty(nodes::AdjustSourceLoyalty { delta }) => {
            // "[+n]" / "[-n]" inside an effect: loyalty counters on the
            // source (CR 122.1e).
            let n = l.int("AdjustSourceLoyalty.delta", *delta, -20, 20);
            if n > 0 {
                b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter: Counter::Loyalty, n: Amount::Fixed(n) }));
            } else if n < 0 {
                b.op(Op::Ext(Ext::RemoveCounters { on: On::Source, counter: Counter::Loyalty, n: Amount::Fixed(-n) }));
            }
        }
        E::CreateThenTheyGainKeywordsUEOT(nodes::CreateThenTheyGainKeywordsUEOT { create, keywords }) => {
            // "Create <tokens>. They gain <keywords> until end of turn."
            match create.as_deref() {
                Some(c @ E::CreateToken(_)) => lower_effect(l, b, c),
                Some(other) => {
                    l.unsupported(format!("CreateThenTheyGainKeywordsUEOT.create({})", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("CreateThenTheyGainKeywordsUEOT.create(missing)");
                    return true;
                }
            }
            let mods = keyword_mods(l, keywords);
            if mods.is_empty() {
                l.unsupported("CreateThenTheyGainKeywordsUEOT.keywords(none)");
            }
            apply(b, On::MadeAll, mods, Until::EndOfTurn);
        }
        E::ReturnChosenGraveyardCardToHand(nodes::ReturnChosenGraveyardCardToHand { card_filter }) => {
            // "Return a card <...> from your graveyard to your hand" (not
            // targeted: chosen as it resolves).
            let Some(filter) = opt_card_filter(l, card_filter) else { return true };
            b.op(Op::Ext(Ext::ChooseFromGraveyardToHand { filter }));
        }
        E::SourceDealsLoyaltyDamageToChosen(nodes::SourceDealsLoyaltyDamageToChosen {}) => {
            // "<This> deals damage equal to the number of loyalty counters
            // on it to a creature or player" (chosen as it resolves).
            b.op(Op::Ext(Ext::DamageChosen { amount: Amount::SourceCounters(Counter::Loyalty) }));
        }
        E::GrantExtraLoyaltyActivationsThisTurn(nodes::GrantExtraLoyaltyActivationsThisTurn { count }) => {
            let n = l.int("GrantExtraLoyaltyActivationsThisTurn.count", *count, 1, 9) as u8;
            b.op(Op::Ext(Ext::ExtraLoyaltyActivations { n }));
        }
        E::TargetPlayerDrawsThenDiscards(nodes::TargetPlayerDrawsThenDiscards { draw, discard }) => {
            // "Target player draws n cards, then discards m cards."
            let d = l.int("TargetPlayerDrawsThenDiscards.draw", *draw, 0, 20);
            let x = l.int("TargetPlayerDrawsThenDiscards.discard", *discard, 0, 20) as u8;
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Draw { who: Who::TargetPlayer(t), n: Amount::Fixed(d) });
            b.op(Op::Discard { who: Who::TargetPlayer(t), n: x });
        }
        E::GainControlOfTargetPermanently(nodes::GainControlOfTargetPermanently { targets }) => {
            let Some(tgt) = lower_targets(l, b, targets, "GainControlOfTargetPermanently") else { return true };
            b.op(Op::Ext(Ext::GainControl { tgt }));
        }
        E::GainControlOfTriggeringSpellTargets(nodes::GainControlOfTriggeringSpellTargets { noun_filter }) => {
            // "Gain control of those permanents" (the event spell's targets).
            let Some(filter) = super::filter::lower_pred(l, noun_filter) else { return true };
            b.op(Op::Ext(Ext::GainControlOfEventSpellTargets { filter }));
        }
        E::PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords(
            nodes::PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords { amount, granted_keywords, until_your_next_turn },
        ) => {
            // "Put a +1/+1 counter on each creature you control. Those
            // creatures gain <keywords> until [end of turn | your next turn]."
            let n = l.int("PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords.amount", *amount, 1, 20);
            let f = and(Filter::Type(types::CREATURE), Filter::Controller(true));
            b.op(Op::Ext(Ext::PutCounters { on: On::Each(f.clone()), counter: Counter::P1P1, n: Amount::Fixed(n) }));
            let mods = keyword_mods(l, granted_keywords);
            if !mods.is_empty() {
                let until = if *until_your_next_turn { Until::YourNextTurn } else { Until::EndOfTurn };
                apply(b, On::Each(f), mods, until);
            }
        }
        E::MillTargetPlayer(nodes::MillTargetPlayer { amount, use_controller_life }) => {
            l.deny("MillTargetPlayer.use_controller_life", use_controller_life);
            let n = l.int("MillTargetPlayer.amount", *amount, 1, 60);
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::Mill { who: Who::TargetPlayer(t), n: Amount::Fixed(n) }));
        }
        E::ExileTargetPlayersLibraryThenShuffleHandIn(nodes::ExileTargetPlayersLibraryThenShuffleHandIn {}) => {
            // "Exile all cards from target player's library, then that
            // player shuffles their hand into their library."
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::ExileLibrary { who: Who::TargetPlayer(t) }));
            b.op(Op::ShuffleIntoLibrary { who: Who::TargetPlayer(t), hand: true, graveyard: false });
        }
        E::LookAtTopCardOfTargetPlayersLibraryMayBottom(nodes::LookAtTopCardOfTargetPlayersLibraryMayBottom {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::LookAtTopMayBottom { who: Who::TargetPlayer(t) }));
        }
        E::DrawPerOpponentWhoLostLifeThisTurn(nodes::DrawPerOpponentWhoLostLifeThisTurn {}) => {
            b.op(Op::Draw { who: Who::You, n: Amount::OpponentsLostLife });
        }
        E::PutSourceOntoBattlefieldTappedAndAttacking(nodes::PutSourceOntoBattlefieldTappedAndAttacking {}) => {
            // Ninjutsu's effect (CR 702.49a).
            b.op(Op::Ext(Ext::NinjutsuEnter));
        }
        E::TapTargetPutStunCounters(nodes::TapTargetPutStunCounters { targets, count }) => {
            // "Tap target creature. Put n stun counters on it."
            let n = l.int("TapTargetPutStunCounters.count", *count, 1, 9);
            let Some(t) = lower_targets(l, b, targets, "TapTargetPutStunCounters") else { return true };
            b.op(Op::Ext(Ext::Tap { on: On::Target(t) }));
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::Stun, n: Amount::Fixed(n) }));
        }
        E::RevealTopNOpponentChoosesOneRestExiledWithCounter(nodes::RevealTopNOpponentChoosesOneRestExiledWithCounter {
            n,
            counter,
        }) => {
            let n = l.int("RevealTopNOpponentChoosesOneRestExiledWithCounter.n", *n, 1, 9) as u8;
            let counter = counter_by_name(l, counter);
            b.op(Op::Ext(Ext::RevealTopOpponentChooses { n, counter }));
        }
        E::PutCardYouOwnWithCounterFromExileIntoHand(nodes::PutCardYouOwnWithCounterFromExileIntoHand { counter }) => {
            let counter = counter_by_name(l, counter);
            b.op(Op::Ext(Ext::ExiledWithCounterToHand { counter }));
        }
        E::SacrificeCreatureThenDamageEqualToItsPower(nodes::SacrificeCreatureThenDamageEqualToItsPower {
            draw_if_subtype,
        }) => {
            // "Sacrifice a creature. When you do, <this> deals X damage to
            // any target, where X is that creature's power. If the
            // sacrificed creature was a <subtype>, draw X cards." (a
            // reflexive trigger, CR 603.12)
            let mut d = Builder::default();
            let t = d.target(any_target(), 1, 1);
            d.op(Op::DealDamage { amount: Amount::EventAmount, to: t });
            if !draw_if_subtype.is_empty() {
                let s = l.subtype(draw_if_subtype);
                let at = d.op(Op::Ext(Ext::JumpUnlessEventObj { filter: Filter::Subtype(s), to: 0 }));
                d.op(Op::Draw { who: Who::You, n: Amount::EventAmount });
                d.patch(at);
            }
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::SacrificeThenReflexive { filter: Filter::Type(types::CREATURE), k }));
        }
        E::LookTopNMayRevealMatchingToHandRestBottomRandom(nodes::LookTopNMayRevealMatchingToHandRestBottomRandom {
            n,
            card_filter,
        }) => {
            // "Look at the top n cards of your library. You may reveal a
            // <...> card from among them and put it into your hand. Put the
            // rest on the bottom of your library in a random order."
            let n = l.int("LookTopNMayRevealMatchingToHandRestBottomRandom.n", *n, 1, 20) as u8;
            let filter = match card_filter {
                Some(p) => match super::filter::lower_pred(l, p) {
                    Some(f) => f,
                    None => return true,
                },
                None => Filter::Any,
            };
            b.op(Op::LookTop { who: Who::You, n, reveal: false });
            b.op(Op::PileTo { who: Who::You, dest: PileDest::Hand, filter, min: 0, max: 1, rest: false, reveal: true });
            b.op(Op::Ext(Ext::PileRestBottomRandom));
        }
        E::PutCountersUntapItBecomesCreature(nodes::PutCountersUntapItBecomesCreature {
            targets,
            counter_kind,
            counter_count,
            base_pt,
            subtypes,
            keywords,
        }) => {
            // "Put n counters on target <...>. Untap it. It becomes a p/t
            // <subtype> creature with <keywords> that's still a land" (no
            // duration).
            let Some(t) = lower_targets(l, b, targets, "PutCountersUntapItBecomesCreature") else { return true };
            let counter = counter_by_name(l, counter_kind);
            let n = l.int("PutCountersUntapItBecomesCreature.counter_count", *counter_count, 0, 20);
            if n > 0 {
                b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter, n: Amount::Fixed(n) }));
            }
            b.op(Op::Untap { tgt: t });
            let subs = subtypes.iter().map(|s| l.subtype(s)).collect();
            let mut mods = vec![Mod::Types { add: types::CREATURE, subtypes: subs, replace: false }];
            match base_pt[..] {
                [p, q] => mods.push(Mod::SetPt(
                    l.int("PutCountersUntapItBecomesCreature.base_pt", p, 0, 99),
                    l.int("PutCountersUntapItBecomesCreature.base_pt", q, 0, 99),
                )),
                _ => l.unsupported("PutCountersUntapItBecomesCreature.base_pt(shape)"),
            }
            mods.extend(keyword_mods(l, keywords));
            apply(b, On::Target(t), mods, Until::Forever);
        }
        E::SearchAnyNumberOntoBattlefield(nodes::SearchAnyNumberOntoBattlefield { card_filter, tapped }) => {
            let Some(filter) = opt_card_filter(l, card_filter) else { return true };
            b.op(Op::Ext(Ext::SearchAnyNumber { filter, tapped: *tapped, exile_free: false }));
        }
        E::SearchAnyNumberExileCastFreeThisTurn(nodes::SearchAnyNumberExileCastFreeThisTurn { card_filter, colorless }) => {
            // "Search your library for any number of [colorless] <...>
            // cards, exile them, then shuffle. Until end of turn, you may
            // cast those cards without paying their mana costs."
            let Some(mut filter) = opt_card_filter(l, card_filter) else { return true };
            if *colorless {
                filter = and(filter, Filter::Colorless);
            }
            b.op(Op::Ext(Ext::SearchAnyNumber { filter, tapped: false, exile_free: true }));
        }
        E::ExchangeControlOfTargets(nodes::ExchangeControlOfTargets { targets }) => {
            // "Exchange control of target <A> and target <B>" (CR 701.12).
            let [x, y] = &targets[..] else {
                l.unsupported(format!("ExchangeControlOfTargets.targets(len {})", targets.len()));
                return true;
            };
            let (Some((fa, a0, a1)), Some((fb, b0, b1))) = (lower_target_spec(l, x), lower_target_spec(l, y)) else { return true };
            if (a0, a1, b0, b1) != (1, 1, 1, 1) {
                l.unsupported("ExchangeControlOfTargets.targets(counts)");
            }
            let a = b.target(fa, 1, 1);
            let c = b.target(fb, 1, 1);
            b.op(Op::Ext(Ext::ExchangeControl { a, b: c }));
        }
        E::TargetBecomesCreatureIndefinitely(nodes::TargetBecomesCreatureIndefinitely {
            targets,
            lose_all_abilities,
            colors,
            creature_subtypes,
            base_pt,
        }) => {
            // "Target <...> [loses all abilities and] becomes a <colour>
            // <subtype> creature with base power and toughness p/t" (no
            // duration). CR 205.1a: its card types become exactly creature,
            // its creature types exactly these.
            let Some(t) = lower_targets(l, b, targets, "TargetBecomesCreatureIndefinitely") else { return true };
            let subtypes = creature_subtypes.iter().map(|s| l.subtype(s)).collect();
            let mut mods = vec![Mod::BecomeTypes { types: types::CREATURE, subtypes }];
            if !colors.is_empty() {
                let mut m = 0;
                for c in colors {
                    match crate::mana::letter_index(c) {
                        Some(i) if i < 5 => m |= 1 << i,
                        _ => l.unsupported(format!("TargetBecomesCreatureIndefinitely.colors({c})")),
                    }
                }
                mods.push(Mod::Colors(m));
            }
            if *lose_all_abilities {
                mods.push(Mod::LoseAllAbilities);
            }
            match base_pt[..] {
                [p, q] => mods.push(Mod::SetPt(
                    l.int("TargetBecomesCreatureIndefinitely.base_pt", p, 0, 99),
                    l.int("TargetBecomesCreatureIndefinitely.base_pt", q, 0, 99),
                )),
                _ => l.unsupported("TargetBecomesCreatureIndefinitely.base_pt(shape)"),
            }
            apply(b, On::Target(t), mods, Until::Forever);
        }
        E::ChooseNameRevealTopPutNamedIntoHand(nodes::ChooseNameRevealTopPutNamedIntoHand { count, nonland }) => {
            let n = l.int("ChooseNameRevealTopPutNamedIntoHand.count", *count, 1, 20) as u8;
            b.op(Op::Ext(Ext::NameRevealTopToHand { n, nonland: *nonland }));
        }
        E::ReturnTargetCardFromYourGraveyardToHand(nodes::ReturnTargetCardFromYourGraveyardToHand {
            card_filter,
            bonus_color,
            bonus,
        }) => {
            let Some(cf) = super::filter::lower_card_filter(l, card_filter) else { return true };
            let t = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), 1, 1);
            let back = Op::Ext(Ext::FromGraveyard { tgt: t, to_battlefield: false, yours: true, tapped: false });
            match (bonus_color, bonus) {
                (None, None) => {
                    b.op(back);
                }
                // "If it's a <colour> card, <bonus>" (Tamiyo, Seasoned
                // Scholar): its colour in the graveyard (a card's colour
                // doesn't change on the way to a hand).
                (Some(c), Some(e)) => match crate::mana::letter_index(c) {
                    Some(i) if i < 5 => {
                        let filter = Filter::InGraveyard(Box::new(Filter::Color(1 << i)));
                        let at = b.op(Op::JumpUnlessTarget { tgt: t, filter, to: 0 });
                        b.op(back.clone());
                        lower_effect(l, b, e);
                        let end = b.op(Op::Jump { to: 0 });
                        b.patch(at);
                        b.op(back);
                        b.patch(end);
                    }
                    _ => l.unsupported(format!("ReturnTargetCardFromYourGraveyardToHand.bonus_color({c})")),
                },
                _ => l.unsupported("ReturnTargetCardFromYourGraveyardToHand.bonus(without bonus_color)"),
            }
        }
        E::ReturnTargetGraveyardCardToHand(nodes::ReturnTargetGraveyardCardToHand { card_filter, optional }) => {
            // "Return [up to one] target <...> card from your graveyard to
            // your hand."
            let Some(cf) = opt_card_filter(l, card_filter) else { return true };
            let t = b.target(Filter::InGraveyard(Box::new(and(cf, Filter::Owner(true)))), if *optional { 0 } else { 1 }, 1);
            b.op(Op::Ext(Ext::FromGraveyard { tgt: t, to_battlefield: false, yours: true, tapped: false }));
        }
        E::AtBeginningOfNextEndStep(nodes::AtBeginningOfNextEndStep { inner }) => {
            // A delayed triggered ability (CR 603.7).
            let mut d = Builder::default();
            lower_effect(l, &mut d, inner);
            if !d.targets.is_empty() {
                l.unsupported("AtBeginningOfNextEndStep.inner(with targets)");
            }
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: crate::db::DelayedWhen::NextEndStep, about: crate::program::About::Nothing }));
        }
        E::PutTargetIntoOwnersLibraryNthFromTop(nodes::PutTargetIntoOwnersLibraryNthFromTop { position, targets }) => {
            // "Put target <...> into its owner's library nth from the top."
            let depth = l.int("PutTargetIntoOwnersLibraryNthFromTop.position", *position, 1, 20) as u8 - 1;
            let Some(tgt) = lower_targets(l, b, targets, "PutTargetIntoOwnersLibraryNthFromTop") else { return true };
            b.op(Op::PutInLibrary { tgt, depth });
        }
        E::MayCastSorceriesAsThoughFlashUntilYourNextTurn(nodes::MayCastSorceriesAsThoughFlashUntilYourNextTurn {}) => {
            b.op(Op::Ext(Ext::FlashSorceries));
        }
        E::UntapTargetArtifactOrCreatureConditionalCounter(nodes::UntapTargetArtifactOrCreatureConditionalCounter {}) => {
            // "Untap target artifact or creature. If it's an artifact
            // creature, put a +1/+1 counter on it."
            let t = b.target(Filter::Type(types::ARTIFACT | types::CREATURE), 1, 1);
            b.op(Op::Untap { tgt: t });
            let both = and(Filter::Type(types::ARTIFACT), Filter::Type(types::CREATURE));
            let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: both, to: 0 });
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(1) }));
            b.patch(at);
        }
        E::TezzeretEmblemBeginCombat(nodes::TezzeretEmblemBeginCombat { amount }) => {
            // "Put n +1/+1 counters on target artifact you control. If it's
            // not a creature, it becomes a 0/0 Robot artifact creature" (CR
            // 205.1b: it keeps its types; its creature types become Robot).
            let n = l.int("TezzeretEmblemBeginCombat.amount", *amount, 1, 20);
            let t = b.target(and(Filter::Type(types::ARTIFACT), Filter::Controller(true)), 1, 1);
            b.op(Op::Ext(Ext::PutCounters { on: On::Target(t), counter: Counter::P1P1, n: Amount::Fixed(n) }));
            let at = b.op(Op::JumpUnlessTarget { tgt: t, filter: not(Filter::Type(types::CREATURE)), to: 0 });
            let robot = l.subtype("Robot");
            let mods = vec![Mod::Types { add: types::CREATURE, subtypes: vec![robot], replace: true }, Mod::SetPt(0, 0)];
            apply(b, On::Target(t), mods, Until::Forever);
            b.patch(at);
        }
        E::ExileTopCardMayCastOtherwise(nodes::ExileTopCardMayCastOtherwise { otherwise }) => {
            // "Exile the top card of your library. You may cast that card.
            // If you don't, <otherwise>."
            let at = b.op(Op::Ext(Ext::ExileTopMayCast { skip: 0 }));
            match otherwise.as_deref() {
                Some(o) => lower_effect(l, b, o),
                None => l.unsupported("ExileTopCardMayCastOtherwise.otherwise(missing)"),
            }
            b.patch(at);
        }
        E::PutLoyaltyCounterOnSelf(nodes::PutLoyaltyCounterOnSelf { amount }) => {
            let n = l.int("PutLoyaltyCounterOnSelf.amount", *amount, 1, 20);
            b.op(Op::Ext(Ext::PutCounters { on: On::Source, counter: Counter::Loyalty, n: Amount::Fixed(n) }));
        }
        _ => return false,
    }
    true
}
