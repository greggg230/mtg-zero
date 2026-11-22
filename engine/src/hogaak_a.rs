//! Legacy Hogaak Tempo, lane A (ENGINE.md 220.1): the rules Hogaak, Arisen
//! Necropolis needs beyond the engine's convoke (ENGINE.md 99.8), delve (54,
//! 65), graveyard casts (27, 100.5), trample and the legend rule. Named after
//! the rules, each citing its rule; never after a card.
//!
//! * "You may cast this card from your graveyard" (CR 601.3, 113.6f): its
//!   owner's route `cast::route::OWN`, for its costs (`CastingRules::own_gy_cast`).
//! * "You can't spend mana to cast this spell" (CR 113.6e, 601.2f-h): the
//!   payment satisfies `doomsday_b::Allow::no_mana` (from `spend_allow_from`,
//!   however the spell is cast), so no mana ability's mana and none of the
//!   pool's pays any part of its total cost; castability and every check of
//!   the payment count its convoke's creatures (and improvise's artifacts)
//!   only (`Game::no_mana_classes`, `Game::spell_pay_classes`). Delve's cards
//!   are chosen first, as for every delve spell (`Task::Delve`, 702.66a), the
//!   fewest more cards that leave the rest payable now counted by convoke
//!   alone (`Game::delve_rest_payable`). Then the rest is paid by tapping
//!   untapped creatures, one decision a creature in increasing battlefield
//!   order (each set once), every one of them by its own id
//!   ([`Game::convoke_pay`], `Task::ConvokePay`): a pick is offered when it
//!   and the creatures after it can still pay the whole rest. Each creature
//!   pays one unit: {1}, or a pip one of its colours meets (702.51a; a hybrid
//!   pip either of its colours, 107.4e; a colourless creature {1} only).
//!   Which pip a creature pays leaves no trace (no mana is spent: nothing
//!   reads it), so the payment asks which creatures and which graveyard
//!   cards, every set that pays being reachable.

use crate::db::types;
use crate::decision::{Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{CardIdx, PlayerId};
use crate::mana::{pair_mask, ManaCost, ManaMask, COLORS};

/// A need of a cost convoke may pay (CR 702.51a): any untapped creature for
/// {1} (`GENERIC`), else one with a colour of the mask (a coloured pip, a
/// hybrid pip's two colours).
const GENERIC: ManaMask = 0;

/// The needs of `cost` one convoke source each pays (CR 702.51a), or `None`
/// when no `sources` can pay it: more mana symbols than sources (each pays
/// one), or a part convoke, improvise and delve can't pay at all - a {C} pip
/// (neither coloured nor generic mana), a Phyrexian pip (the lowering refuses
/// a spell of no mana with one, and no cost increase adds one).
pub(crate) fn convoke_needs(cost: &ManaCost, sources: usize) -> Option<Vec<ManaMask>> {
    if cost.pips[crate::mana::C] > 0 || cost.phyrexian.iter().any(|&n| n > 0) || cost.total() > sources as u64 {
        return None;
    }
    let mut needs = Vec::with_capacity(cost.total() as usize);
    for k in 0..5 {
        needs.extend(std::iter::repeat_n(1 << k, cost.pips[k] as usize));
    }
    for (i, &n) in cost.hybrid.iter().enumerate() {
        needs.extend(std::iter::repeat_n(pair_mask(i), n as usize));
    }
    needs.extend(std::iter::repeat_n(GENERIC, cost.generic as usize));
    Some(needs)
}

/// Does a cost's part besides its mana take permanents (sacrifice, return,
/// tap, crew, a counter removed from one, waterbend's taps)? A spell whose
/// mana can't be spent has none in any reachable cast: the lowering refuses
/// the card's own additional and alternative costs, and the routes and grants
/// add a land card discarded (retrace), graveyard cards exiled (escape) or
/// life (a granted warp): convoke's taps never compete with a part.
pub(crate) fn rest_takes_permanents(r: &crate::db::Cost) -> bool {
    r.sacrifice.is_some() || r.bounce.is_some() || r.tap_untapped.is_some() || r.tap_x.is_some() || r.crew > 0 || r.remove_counter_from.is_some() || r.waterbend
}

/// Can source unit `src` (a creature's colours, 0 for none) pay need `need`?
fn fits(src: ManaMask, need: ManaMask) -> bool {
    need == GENERIC || src & need != 0
}

/// Kuhn's augmenting path from `a` (one side, `adj(a, b)` its edges to the
/// other side's `b`), `owner[b]` the side-A vertex `b` is matched to.
fn augment(a: usize, nb: usize, adj: &dyn Fn(usize, usize) -> bool, seen: &mut [bool], owner: &mut [usize]) -> bool {
    for b in 0..nb {
        if adj(a, b) && !seen[b] {
            seen[b] = true;
            if owner[b] == usize::MAX || augment(owner[b], nb, adj, seen, owner) {
                owner[b] = a;
                return true;
            }
        }
    }
    false
}

/// A matching of every vertex of side A (`na`) into side B (`nb`)?
fn saturates(na: usize, nb: usize, adj: &dyn Fn(usize, usize) -> bool) -> bool {
    let mut owner = vec![usize::MAX; nb];
    (0..na).all(|a| augment(a, nb, adj, &mut vec![false; nb], &mut owner))
}

/// Can sources `picked` (all of them) and some of `later` pay every need of
/// `needs`, one need each (CR 702.51a)? A matching saturating the needs in
/// the needs x (picked + later) graph and one saturating the picked sources in
/// the picked x needs graph give one saturating both (Mendelsohn-Dulmage),
/// whose sources are the picked ones and |needs| - |picked| of the later ones.
pub(crate) fn convoke_covers(needs: &[ManaMask], picked: &[ManaMask], later: &[ManaMask]) -> bool {
    if picked.len() > needs.len() || picked.len() + later.len() < needs.len() {
        return false;
    }
    let all: Vec<ManaMask> = picked.iter().chain(later.iter()).copied().collect();
    saturates(needs.len(), all.len(), &|a, b| fits(all[b], needs[a])) && saturates(picked.len(), needs.len(), &|a, b| fits(picked[a], needs[b]))
}

impl Game {
    /// The sources of a payment of no mana for spell `card` cast by `p` (CR
    /// 601.2g-h with "you can't spend mana to cast this spell"): its
    /// convoke's untapped creatures (702.51a) and its improvise's untapped
    /// artifacts (702.126a), as `pay::Class`es (`spell_pay_classes`).
    pub(crate) fn no_mana_classes(&self, p: PlayerId, card: CardIdx) -> Vec<crate::pay::Class> {
        let def = self.def_of(card);
        let allow = crate::doomsday_b::Allow { no_mana: true, ..Default::default() };
        self.spell_pay_classes(p, None, if def.improvise { types::ARTIFACT } else { 0 }, def.casting.convoke, allow)
    }

    /// The convoke (and improvise) sources of `p` for spell `card`, in
    /// battlefield order, each with the unit it pays: an untapped creature
    /// its colours (CR 702.51a; summoning sickness doesn't matter, 302.6), an
    /// untapped artifact for improvise none ({1} only, 702.126a).
    pub(crate) fn convoke_sources(&self, p: PlayerId, card: CardIdx) -> Vec<(CardIdx, ManaMask)> {
        let def = self.def_of(card);
        self.permanents_of(p)
            .filter(|&c| !self.objs[c.i()].tapped)
            .filter_map(|c| {
                if def.casting.convoke && self.is_creature(c) {
                    Some((c, self.colors_of(c) & COLORS))
                } else if def.improvise && self.is_type(c, types::ARTIFACT) {
                    Some((c, 0))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Can `p` pay `cost` (the rest of spell `card`'s total cost) without
    /// spending mana: its convoke's creatures and improvise's artifacts alone
    /// (CR 702.51a, 702.126a)?
    pub(crate) fn no_mana_payable(&self, p: PlayerId, card: CardIdx, cost: &ManaCost) -> bool {
        let srcs: Vec<ManaMask> = self.convoke_sources(p, card).into_iter().map(|s| s.1).collect();
        let Some(needs) = convoke_needs(cost, srcs.len()) else { return false };
        convoke_covers(&needs, &[], &srcs)
    }

    /// Delve's question while choosing its cards (CR 702.66a): can `p` pay
    /// `cost`, the rest of spell `card`'s mana once its cards are exiled,
    /// with the payment's other sources: its mana abilities and the pool,
    /// and its convoke's creatures and improvise's artifacts as its payment
    /// has them (`spell_pay_classes`; ENGINE.md 221.5: the mana alone before,
    /// `can_pay_for`), or for a spell whose mana can't be spent
    /// (`Allow::no_mana`) its convoke alone (ENGINE.md 220.1).
    pub(crate) fn delve_rest_payable(&self, p: PlayerId, card: CardIdx, cost: &ManaCost, allow: crate::doomsday_b::Allow) -> bool {
        if allow.no_mana {
            return self.no_mana_payable(p, card, cost);
        }
        let def = self.def_of(card);
        if def.improvise || def.casting.convoke {
            let classes = self.spell_pay_classes(p, None, if def.improvise { types::ARTIFACT } else { 0 }, def.casting.convoke, allow);
            return self.can_pay_with(p, cost, &classes, &crate::pay::Keep { allow, ..Default::default() });
        }
        self.can_pay_for(p, cost, allow)
    }

    /// The cost `Task::ConvokePay` pays for stack item `pos` (spell `card`):
    /// its total cost (CR 601.2f) less one generic mana per card delve chose
    /// (702.66a), as `cast_pay` reads it.
    fn convoke_rest(&self, card: CardIdx, pos: usize) -> ManaCost {
        let item = &self.stack[pos];
        self.total_mana(card, item).less_generic(item.delved as u64)
    }

    /// `Task::ConvokePay`: pay the rest of the total cost of a spell whose
    /// mana can't be spent (CR 113.6e, 601.2h) by tapping untapped creatures
    /// for its convoke (702.51a; artifacts for its improvise, 702.126a). One
    /// decision a source (`ChoosePermanent`, its `MY_PERM` id), in increasing
    /// battlefield order after the last pick (each set once), each leaving
    /// the rest payable by it and the sources after it; done when the picks
    /// are as many as the cost's mana symbols (each pays one; no mana pays
    /// the rest, so no fewer). Then the sources are tapped, delve's cards
    /// exiled (601.2h) and the life the cost takes after its mana paid
    /// (Bolas's Citadel's). A creature tapped this way spends no mana (no
    /// "mana spent", no colour of it).
    pub(crate) fn convoke_pay(&mut self, id: u32, mut picked: Vec<CardIdx>) {
        let Some(pos) = self.item_index(id) else { return };
        let StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Card(c)) => picked.push(c),
            Some(other) => unreachable!("convoke answer {other:?}"),
            None => {}
        }
        let cost = self.convoke_rest(card, pos);
        if picked.len() as u64 == cost.total() {
            self.finish_convoke_pay(card, pos, p, &picked);
            return;
        }
        // (The picks are still untapped: sources until the cost is paid.)
        let srcs = self.convoke_sources(p, card);
        let needs = convoke_needs(&cost, srcs.len()).unwrap_or_else(|| {
            panic!("ConvokePay: {} sources can't pay the rest {cost} of {}
{}", srcs.len(), self.name(card), self.log_lines().join("
"))
        });
        let mask_of = |c: CardIdx| srcs.iter().find(|s| s.0 == c).map(|s| s.1).expect("a picked convoke source");
        let chosen: Vec<ManaMask> = picked.iter().map(|&c| mask_of(c)).collect();
        // (Each set once: the picks go up the battlefield.)
        let floor = picked.last().map(|&c| self.perm_slot(c));
        let cands: Vec<(CardIdx, ManaMask)> = srcs.iter().copied().filter(|&(c, _)| floor.is_none_or(|f| self.perm_slot(c) > f)).collect();
        let mut opts = Vec::new();
        for (k, &(c, m)) in cands.iter().enumerate() {
            let mut with = chosen.clone();
            with.push(m);
            let later: Vec<ManaMask> = cands[k + 1..].iter().map(|s| s.1).collect();
            if convoke_covers(&needs, &with, &later) {
                opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Card(c) });
            }
        }
        assert!(
            !opts.is_empty(),
            "ConvokePay: no source pays the rest {cost} of {} (picked {picked:?})\n{}",
            self.name(card),
            self.log_lines().join("\n")
        );
        self.ask(Task::ConvokePay { id, picked }, p, DecisionKind::ChoosePermanent, opts);
    }

    /// The end of `convoke_pay`: the sources tapped (CR 702.51a, 702.126a:
    /// rather than paying mana; no mana ability, none "tapped for mana"),
    /// delve's cards exiled and the later life paid (601.2h).
    fn finish_convoke_pay(&mut self, card: CardIdx, pos: usize, p: PlayerId, picked: &[CardIdx]) {
        let convoke = self.def_of(card).casting.convoke;
        for &c in picked {
            let why = if convoke && self.is_creature(c) { "convoke" } else { "improvise" };
            self.note(|g| format!("P{p} taps {} for {why}", g.name(c)));
            self.tap(c);
        }
        // "The amount of mana spent to cast this" (EoE W3): none.
        if self.def_of(card).w3.mana_spent || self.db.w6.mana_spent_all {
            self.note_mana_spent(card, 0);
        }
        self.exile_delved(p, Some(card));
        let life = self.life_later(card, &self.stack[pos]);
        if life > 0 {
            self.note(|_| format!("P{p} pays {life} life"));
            self.lose_life(p, i32::try_from(life).expect("life within the life total"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mana::{B, G, R, U};

    fn hogaak() -> ManaCost {
        let mut c = ManaCost { generic: 5, ..Default::default() };
        c.hybrid[crate::mana::pair_index(B, G).unwrap()] = 2;
        c
    }

    #[test]
    fn cr_702_51a_hybrid_pips_need_a_creature_of_either_colour() {
        let needs = convoke_needs(&hogaak(), 7).unwrap();
        assert_eq!(convoke_needs(&hogaak(), 6), None);
        assert_eq!(needs.len(), 7);
        let (b, g, r, u, none) = (1 << B, 1 << G, 1 << R, 1 << U, 0u8);
        // Seven sources, two of them black or green: payable.
        assert!(convoke_covers(&needs, &[], &[b, g, r, r, u, none, none]));
        // A black-green creature pays one hybrid pip only.
        assert!(!convoke_covers(&needs, &[], &[b | g, r, r, u, none, none, none]));
        assert!(convoke_covers(&needs, &[], &[b | g, b | g, r, r, u, none, none]));
        // Six sources: not enough.
        assert!(!convoke_covers(&needs, &[], &[b, g, r, r, u, none]));
        // A red creature picked first, then: still payable with the rest;
        // two colourless picked and only two black or green left: payable;
        // seven picked of which none is black or green: never.
        assert!(convoke_covers(&needs, &[r], &[b, g, none, none, none, none]));
        assert!(convoke_covers(&needs, &[none, none], &[b, g, r, r, u]));
        assert!(!convoke_covers(&needs, &[r, r, u, none, none, none], &[b, g]));
        // Every pick must pay a need: more picks than needs never.
        assert!(!convoke_covers(&needs[..2], &[b, g, b], &[]));
    }

    #[test]
    fn cr_702_51a_a_colourless_pip_is_paid_by_nothing_but_mana() {
        let mut c = ManaCost { generic: 1, ..Default::default() };
        c.pips[crate::mana::C] = 1;
        assert_eq!(convoke_needs(&c, 9), None);
    }
}
