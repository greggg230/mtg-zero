//! Effects of M7b part 3 (copies, CR 707; ENGINE.md 26) -> programs. Same
//! rules as `effect.rs`: every field named, lowered onto shared
//! instructions.

use super::Lowerer;
use crate::copies::Except;
use crate::db::{types, Filter, TargetReq};
use crate::layers::Until;
use crate::mana::ManaCost;
use crate::program::{About, Amount, Builder, CopyOf, Ext, On, Op, SpellOf, Who};
use card_ir::{cats, nodes, Count};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

/// Plain keywords as a copy's "except it has ..." (CR 707.9a).
pub(crate) fn except_kws(l: &mut Lowerer, kws: &[cats::Keyword], what: &str) -> Except {
    let mut exc = Except::default();
    for k in kws {
        match super::printing::lower_keyword_full(l, k) {
            Some(super::printing::KwOrProt::Kw(kw)) => exc.kws.insert(kw),
            Some(_) => l.unsupported(format!("{what}.keywords(not a plain keyword)")),
            None => {}
        }
    }
    exc
}

/// A cost and its display twin, which must agree (ENGINE.md 3, rule 4a).
pub(crate) fn checked_cost(l: &mut Lowerer, what: &str, m: &cats::ManaCost, s: &str) -> ManaCost {
    let mana = super::printing::lower_mana_cost(l, what, m);
    if ManaCost::parse(s) != Some(mana) {
        l.unsupported(format!("{what}_str(differs from {what})"));
    }
    mana
}

/// Two target specifications, the second "another target" (CR 115.3: its
/// `distinct_from` names the first); returns the two requirement indices.
pub(crate) fn two_targets(l: &mut Lowerer, b: &mut Builder, specs: &[cats::TargetSpec], what: &str) -> Option<(u8, u8)> {
    if specs.len() != 2 {
        l.unsupported(format!("{what}.targets(len {})", specs.len()));
        return None;
    }
    let mut reqs = [0u8; 2];
    for (k, s) in specs.iter().enumerate() {
        let cats::TargetSpec::TargetSpec(nodes::TargetSpec { filter, count_min, count_max, distinct_from }) = s;
        let min = l.int("TargetSpec.count_min", *count_min, 0, 1) as u8;
        let max = match count_max {
            Count::Finite(n) => l.int("TargetSpec.count_max", *n, 1, 1) as u8,
            Count::Infinite => {
                l.unsupported("TargetSpec.count_max(any number)");
                1
            }
        };
        let f = super::filter::lower_pred_or_oneof(l, filter)?;
        let mut mask = 0u8;
        for &j in distinct_from {
            // (Only an earlier specification of the same effect.)
            if j < 0 || j as usize >= k {
                l.unsupported("TargetSpec.distinct_from(not an earlier target)");
                return None;
            }
            mask |= 1 << reqs[j as usize];
        }
        reqs[k] = b.target_distinct(f, min, max, mask);
    }
    Some((reqs[0], reqs[1]))
}

/// Lower an effect class of this group; `false` if it is not one of them.
pub(crate) fn lower_effect_copies(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Create a token that's a copy of another target nonlegendary
        // creature you control, except it has haste. Sacrifice it at the
        // beginning of the next end step." (Reflection of Kiki-Jiki.)
        E::CreateTokenCopyOfTargetWithKeywordsSacAtEndStep(nodes::CreateTokenCopyOfTargetWithKeywordsSacAtEndStep {
            targets,
            keywords,
        }) => {
            let Some(t) = super::effect::lower_targets(l, b, targets, "CreateTokenCopyOfTargetWithKeywordsSacAtEndStep") else { return true };
            let exc = except_kws(l, keywords, "CreateTokenCopyOfTargetWithKeywordsSacAtEndStep");
            b.op(Op::Ext(Ext::CreateTokenCopy { of: CopyOf::Target { i: t, lki: false }, who: Who::You, n: Amount::Fixed(1), exc }));
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::SacrificeEventObj));
            let k = l.delayed_ability(d.finish());
            b.op(Op::Ext(Ext::Delay { k, when: crate::db::DelayedWhen::NextEndStep, about: About::EachMade }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Create a token that's a copy of target <permanent>."
        E::CreateTokenCopyOfTargetPermanent(nodes::CreateTokenCopyOfTargetPermanent { target_filter, label, targets }) => {
            let _ = label; // display text
            let tf = super::filter::lower_pred(l, target_filter);
            let Some(t) = super::effect::lower_targets(l, b, targets, "CreateTokenCopyOfTargetPermanent") else { return true };
            if tf.as_ref() != Some(&b.targets[t as usize].filter) {
                l.unsupported("CreateTokenCopyOfTargetPermanent.target_filter(differs from targets)");
            }
            let exc = Except::default();
            b.op(Op::Ext(Ext::CreateTokenCopy { of: CopyOf::Target { i: t, lki: false }, who: Who::You, n: Amount::Fixed(1), exc }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "You may pay <cost> if this permanent is attached to a creature you
        // control. If you do, create a token that's a copy of that creature.
        // If you didn't create a token this way, create <token>."
        // (Springheart Nantuko.)
        E::PayToCopyEnchantedCreatureElseToken(nodes::PayToCopyEnchantedCreatureElseToken { cost, cost_str, token_printing }) => {
            let Some(m) = cost else {
                l.unsupported("PayToCopyEnchantedCreatureElseToken.cost(missing)");
                return true;
            };
            let cost = checked_cost(l, "PayToCopyEnchantedCreatureElseToken.cost", m, cost_str);
            let Some(tp) = token_printing else {
                l.unsupported("PayToCopyEnchantedCreatureElseToken.token_printing(missing)");
                return true;
            };
            let cats::CardPrinting::CardPrinting(tp) = &**tp;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("PayToCopyEnchantedCreatureElseToken.token_printing(not a token)");
            }
            let token = l.token(def);
            let host = Filter::AttachedTo(Box::new(and(Filter::Type(types::CREATURE), Filter::Controller(true))));
            let not_attached = b.op(Op::Ext(Ext::JumpUnlessSource { filter: host, to: 0 }));
            let offer = b.op(Op::OfferPay { who: Who::You, cost, if_paid: 0 });
            let declined = b.op(Op::Jump { to: 0 });
            b.patch(offer);
            let exc = Except::default();
            b.op(Op::Ext(Ext::CreateTokenCopy { of: CopyOf::SourceHost, who: Who::You, n: Amount::Fixed(1), exc }));
            let none_made = b.op(Op::Ext(Ext::JumpUnlessMadeYours { to: 0 }));
            b.op(Op::Ext(Ext::TokensCreated));
            let done = b.op(Op::Jump { to: 0 });
            b.patch(not_attached);
            b.patch(declined);
            b.patch(none_made);
            b.op(Op::CreateToken { token, n: 1, tapped: false });
            b.op(Op::Ext(Ext::TokensCreated));
            b.patch(done);
        }
        // Squad's and offspring's "create that many tokens that are copies
        // of it" / "a token that's a copy of it, except it's 1/1" (CR
        // 702.157a, 702.175a; its copiable values as it last existed).
        E::CreateTokenCopiesOfSelf(nodes::CreateTokenCopiesOfSelf { per_squad_payment, base_pt }) => {
            let n = if *per_squad_payment { Amount::EventTimes } else { Amount::Fixed(1) };
            let mut exc = Except::default();
            if let Some((p, t)) = base_pt {
                exc.pt = Some((l.int("CreateTokenCopiesOfSelf.base_pt", *p, 0, 99) as i8, l.int("CreateTokenCopiesOfSelf.base_pt", *t, 0, 99) as i8));
            }
            b.op(Op::Ext(Ext::CreateTokenCopy { of: CopyOf::Source, who: Who::You, n, exc }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        // "Target artifact you control becomes a copy of another target
        // artifact or creature you control until end of turn, except it's an
        // artifact in addition to its other types." (Saheeli, Sublime
        // Artificer's -2.)
        E::TargetBecomesCopyOfAnotherTargetUEOT(nodes::TargetBecomesCopyOfAnotherTargetUEOT { targets, add_card_types }) => {
            let Some((a, c)) = two_targets(l, b, targets, "TargetBecomesCopyOfAnotherTargetUEOT") else { return true };
            if b.targets[c as usize].distinct & (1 << a) == 0 {
                l.unsupported("TargetBecomesCopyOfAnotherTargetUEOT.targets(not \"another target\")");
            }
            let mut exc = Except::default();
            for t in add_card_types {
                if let Some(bits) = super::printing::given_type(l, "TargetBecomesCopyOfAnotherTargetUEOT.add_card_types", t) {
                    exc.types |= bits;
                }
            }
            b.op(Op::Ext(Ext::BecomeCopy { on: On::Target(a), of: CopyOf::Target { i: c, lki: false }, exc, until: Until::EndOfTurn }));
        }
        // "This land becomes a copy of target permanent card in your
        // graveyard until end of turn." (Shifting Woodland; the IR carries
        // no target specification: "a permanent card in your graveyard".)
        E::SourceBecomesCopyOfTargetGraveyardCardUEOT(nodes::SourceBecomesCopyOfTargetGraveyardCardUEOT {}) => {
            let f = Filter::InGraveyard(Box::new(and(Filter::Type(types::PERMANENT), Filter::Owner(true))));
            let t = b.target(f, 1, 1);
            let exc = Except::default();
            b.op(Op::Ext(Ext::BecomeCopy { on: On::Source, of: CopyOf::Target { i: t, lki: false }, exc, until: Until::EndOfTurn }));
        }
        // "Chain Lightning deals 3 damage to any target. Then that player or
        // that permanent's controller may pay {R}{R}. If the player does,
        // they may copy this spell and may choose a new target for that
        // copy."
        E::DealDamageThenChainCopy(nodes::DealDamageThenChainCopy { amount, chain_cost, targets }) => {
            let n = l.int("DealDamageThenChainCopy.amount", *amount, 0, 99);
            let Some(cost) = ManaCost::parse(chain_cost) else {
                l.unsupported(format!("DealDamageThenChainCopy.chain_cost({chain_cost})"));
                return true;
            };
            let Some(t) = super::effect::lower_targets(l, b, targets, "DealDamageThenChainCopy") else { return true };
            b.op(Op::DealDamage { amount: Amount::Fixed(n), to: t });
            let who = Who::TargetOrItsController(t);
            let offer = b.op(Op::OfferPay { who, cost, if_paid: 0 });
            let declined = b.op(Op::Jump { to: 0 });
            b.patch(offer);
            b.op(Op::Ext(Ext::CopySpell { of: SpellOf::This, who, n: Amount::Fixed(1), may: true, new_targets: true }));
            b.patch(declined);
        }
        // "That player may copy this spell and may choose a new target for
        // that copy" (Chain of Smog: the spell's target player). The IR's
        // `needs_target_with_cards_in_hand` is a Python shortcut (it offers
        // the copy only while a player with cards in hand can be targeted);
        // the copy is always offered here (ENGINE.md 14; docs/ir_defects.md
        // 191).
        E::TargetPlayerMayCopyThisSpell(nodes::TargetPlayerMayCopyThisSpell { needs_target_with_cards_in_hand }) => {
            let _ = needs_target_with_cards_in_hand;
            let players: Vec<usize> = b.targets.iter().enumerate().filter(|(_, r)| is_player_req(r)).map(|(i, _)| i).collect();
            let [t] = players[..] else {
                l.unsupported("TargetPlayerMayCopyThisSpell(not one target player)");
                return true;
            };
            let who = Who::TargetPlayer(t as u8);
            b.op(Op::Ext(Ext::CopySpell { of: SpellOf::This, who, n: Amount::Fixed(1), may: true, new_targets: true }));
        }
        // "Copy target instant or sorcery spell you control. You may choose
        // new targets for the copy." (Lutri; no target specification in the
        // IR.)
        E::CopyTargetInstantOrSorcerySpellYouControl(nodes::CopyTargetInstantOrSorcerySpellYouControl {}) => {
            let f = Filter::Spell(Box::new(and(Filter::Type(types::INSTANT | types::SORCERY), Filter::Controller(true))));
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::CopySpell { of: SpellOf::Target(t), who: Who::You, n: Amount::Fixed(1), may: false, new_targets: true }));
        }
        // "When you cast this spell, ... copy this spell" (the event's
        // spell; no new targets: the text doesn't allow them, CR 707.10).
        E::CopyThisSpell(nodes::CopyThisSpell {}) => {
            b.op(Op::Ext(Ext::CopySpell { of: SpellOf::EventSpell, who: Who::You, n: Amount::Fixed(1), may: false, new_targets: false }));
        }
        // "This creature becomes prepared" (CR 722.3a; the card's prepare
        // spell is checked by `lower_printing`).
        E::SourceBecomesPrepared(nodes::SourceBecomesPrepared {}) => {
            l.uses_prepare = true;
            b.op(Op::Ext(Ext::BecomePrepared { on: On::Source }));
        }
        _ => return false,
    }
    true
}

/// "Target player" (a requirement of players only).
fn is_player_req(r: &TargetReq) -> bool {
    matches!(r.filter, Filter::Player) || matches!(&r.filter, Filter::And(a, _) if **a == Filter::Player)
}
