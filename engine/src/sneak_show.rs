//! Legacy Sneak and Show (ENGINE.md 123): the rules Show and Tell,
//! Omniscience, Mistrise Village and Disruptor Flute need (Brotherhood's End
//! lowers onto the existing `Op::DamageAll` and `Op::DestroyAll`; Magus of the
//! Moon is EoE W2's `nonbasic_lands_are_mountains`). Named after the rules
//! actions, each citing its rule; never after a card.
//!
//! * Each player, in APNAP order, may put a card of a kind from their hand
//!   onto the battlefield; the cards enter at once (CR 101.4, 101.4a, 603.6a,
//!   614.12): [`SnS::EachPlayerPutsFromHand`]. The active player's card is
//!   chosen face down (101.4a): the other player decides knowing only whether
//!   one was chosen (the frame's stage, `ctx::OP_I`), and its worlds deal the
//!   chooser's hand so that the chosen card is one of the kind
//!   ([`Game::hidden_pick_ok`]).
//! * "The next spell you cast this turn can't be countered" (CR 101.2, 611.2,
//!   514.2): [`SnS::NextSpellUncounterable`], `Player::uncounterable_next`,
//!   used up as a spell becomes cast (601.2i, `Game::cast_done`).
//! * "You may cast spells from your hand without paying their mana costs" (an
//!   alternative cost, CR 118.9, 601.2b): [`Game::free_from_hand`], the same
//!   `ChooseAltCost` answer and cast route as EoE's "the first spell each
//!   turn" (`eoe_c::FREE_CAST`).
//! * "As this enters, choose a card name" (CR 201.3, 614.12) with "spells with
//!   the chosen name cost {n} more" (601.2f) and "activated abilities of
//!   sources with the chosen name can't be activated [unless they're mana
//!   abilities]" (602.5, 605.1a): [`SnsDef`], [`Game::nameable_defs`],
//!   [`Game::named_ban_hits`].

use crate::db::Filter;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::ids::{opp, CardIdx, DefId, PlayerId, Zone};
use crate::resolve::{Flow, Frame};

/// The printing facts of the Sneak and Show cards (`CardDef::sns`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct SnsDef {
    /// "You may cast spells from your hand without paying their mana costs"
    /// (CR 118.9; its controller's spells only).
    pub free_casts_from_hand: bool,
    /// "As this enters, choose a card name" without looking at a hand first:
    /// any card name (CR 201.3), the faces' names too (a split card's halves,
    /// a double-faced card's back face, an Adventure; 709.3, 712, 715); the
    /// name compared with a split card's halves as well (709.4). (Anointed
    /// Peacekeeper's "look at an opponent's hand, then choose any card name"
    /// is `CardDef::chooses_name` alone, unchanged.)
    pub plain_name: bool,
    /// "Spells with the chosen name cost {n} more to cast": every player's
    /// spells (`CardDef::named_spell_tax`; Peacekeeper's are its controller's
    /// opponents' only).
    pub named_tax_everyone: bool,
    /// "Activated abilities of sources with the chosen name can't be
    /// activated [unless they're mana abilities]" (CR 602.5): `Some(true)`
    /// with the mana abilities excepted (605.1a), `Some(false)` without.
    pub named_ban: Option<bool>,
}

/// The Sneak and Show instructions (`Ext::SnS`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum SnS {
    /// "Each player may put a <kind> card from their hand onto the
    /// battlefield." The active player chooses first, then the other player
    /// (CR 101.4); a choice is a card of their hand matching `filter`
    /// (`ChooseHandCard`, one option per definition, ENGINE.md 11) or `PASS`
    /// (a "may"; asked even with no such card: the hand is hidden). The
    /// active player's card stays face down until the cards enter (101.4a,
    /// Show and Tell's ruling): the frame's stage `Frame::i` (`ctx::OP_I`)
    /// tells the other player only whether a card was chosen (0: the active
    /// player chooses; 1: the other, the active player chose none; 2: the
    /// other, the active player chose one). Then the chosen cards enter the
    /// battlefield at once, each under its owner's control (one entry,
    /// 603.6a, 614.12a), their "as this enters" choices the active player's
    /// first (the ruling; the tasks of the other player's card are queued
    /// first, so they run after).
    EachPlayerPutsFromHand { filter: Filter },
    /// "The next spell you cast this turn can't be countered": one more
    /// waiting instance for the controller (`Player::uncounterable_next`).
    NextSpellUncounterable,
}

impl SnS {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            SnS::EachPlayerPutsFromHand { .. } | SnS::NextSpellUncounterable => false,
        }
    }
}

/// `Frame::i` of [`SnS::EachPlayerPutsFromHand`].
pub(crate) const STAGE_ACTIVE: u32 = 0;
pub(crate) const STAGE_OTHER_NONE: u32 = 1;
pub(crate) const STAGE_OTHER_CHOSEN: u32 = 2;
const STAGE_DONE: u32 = 3;

/// Token names that are also the names of cards (CR 201.3: a token's name may
/// be chosen only if it's also a card's name), outside the card table's own
/// card names: the card table's token definitions' names checked against
/// Scryfall's card-name catalogue on 2026-11-11 (35,542 names; tokens' own
/// entries, such as "Blood" and "Illusion", excluded): "Faerie Dragon" (an
/// Astral card; Feywild Caretaker's token). ENGINE.md 123.
pub(crate) const TOKEN_NAMES_THAT_ARE_CARD_NAMES: &[&str] = &["Faerie Dragon"];

impl Game {
    pub(crate) fn exec_sns(&mut self, f: &mut Frame, op: &SnS) -> Flow {
        match op {
            SnS::EachPlayerPutsFromHand { filter } => self.each_player_puts_from_hand(f, filter),
            SnS::NextSpellUncounterable => {
                // CR 611.2, 514.2: the effect lasts until the spell is cast or
                // the turn's cleanup step ends "this turn" effects. Several
                // wait for the same next spell (each says "the next spell").
                let p = f.controller as usize;
                let n = self.players[p].uncounterable_next;
                self.players[p].uncounterable_next = self.exact_inc(n, "waiting \"the next spell can't be countered\" effects");
                self.note(|_| format!("P{p}'s next spell this turn can't be countered"));
                Flow::Next
            }
        }
    }

    /// [`SnS::EachPlayerPutsFromHand`].
    fn each_player_puts_from_hand(&mut self, f: &mut Frame, filter: &Filter) -> Flow {
        let ap = self.turn.active;
        let other = opp(ap);
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                f.picked.push(c);
                f.i = if f.i == STAGE_ACTIVE { STAGE_OTHER_CHOSEN } else { STAGE_DONE };
            }
            Some(Choice::Pass) => f.i = if f.i == STAGE_ACTIVE { STAGE_OTHER_NONE } else { STAGE_DONE },
            Some(other) => unreachable!("each-player-puts answer {other:?}"),
            None => {}
        }
        while f.i < STAGE_DONE {
            let p = if f.i == STAGE_ACTIVE { ap } else { other };
            // (An empty hand is public: nothing to choose.)
            if self.players[p as usize].hand.is_empty() {
                f.i = if f.i == STAGE_ACTIVE { STAGE_OTHER_NONE } else { STAGE_DONE };
                continue;
            }
            let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
            let mut opts: Vec<Opt> = self
                .hand_options(p)
                .into_iter()
                .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(filter, c, &ctx)))
                .collect();
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
            self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseHandCard, opts);
            return Flow::Suspend;
        }
        // Every player has chosen: the cards enter at once (CR 101.4). The
        // other player's card first, so that the active player's "as this
        // enters" tasks, queued last, run first (the ruling).
        let mut chosen = std::mem::take(&mut f.picked);
        chosen.sort_by_key(|&c| (self.objs[c.i()].owner == ap) as u8);
        for &c in &chosen {
            let owner = self.objs[c.i()].owner;
            let ctx = crate::cast::Ctx { controller: owner, source: Some(f.source) };
            // (Loud: a world dealt the active player's hand so that the
            // face-down choice is a card of the kind, `hidden_pick_ok`.)
            assert!(
                self.objs[c.i()].zone == Zone::Hand && self.card_matches(filter, c, &ctx),
                "a card chosen to be put onto the battlefield from P{owner}'s hand is not a card of the kind there (ENGINE.md 123)"
            );
        }
        if chosen.is_empty() {
            return Flow::Next;
        }
        self.begin_entering(false);
        for c in chosen {
            let owner = self.objs[c.i()].owner;
            self.note(|g| format!("P{owner} puts {} onto the battlefield", g.name(c)));
            self.put_onto_battlefield(c, owner);
        }
        self.end_entering();
        Flow::Next
    }

    /// A pending [`SnS::EachPlayerPutsFromHand`] whose face-down choice of a
    /// card in `owner`'s hand `viewer` doesn't know: the frame on the agenda
    /// (its index), the card and the filter.
    fn hidden_pick(&self, viewer: PlayerId, owner: PlayerId) -> Option<(usize, CardIdx, Filter)> {
        if viewer == owner || self.turn.active != owner {
            return None;
        }
        self.agenda.iter().enumerate().rev().find_map(|(k, t)| {
            let Task::Run(f) = t else { return None };
            if f.i != STAGE_OTHER_CHOSEN {
                return None;
            }
            let op = self.db.ability(f.ability).program.get(f.pc as usize)?;
            let crate::program::Op::Ext(crate::program::Ext::SnS(SnS::EachPlayerPutsFromHand { filter })) = op else { return None };
            let c = *f.picked.first()?;
            (self.objs[c.i()].owner == owner).then(|| (k, c, filter.clone()))
        })
    }

    /// `determinize`'s check of a deal of `owner`'s hidden cards for `viewer`
    /// (CR 101.4a): with a pending face-down choice of `owner`'s that
    /// `viewer` doesn't know (`hidden_pick`), a card of `owner`'s hand drawn
    /// uniformly is the choice in this world if it is a card of the kind
    /// (then true; else false, and the deal is drawn again). Accepting the
    /// deal and the card together by rejection deals each world in
    /// proportion to its probability under the plain deal times the chance
    /// that the chosen card is a legal one: `viewer` knows a card of the kind
    /// was chosen and nothing of which (the hand is one pool, ENGINE.md 10:
    /// it may be a card `viewer` knows is there).
    pub(crate) fn hidden_pick_ok(&mut self, viewer: PlayerId, owner: PlayerId, rng: &mut crate::rng::Rng) -> bool {
        let Some((k, _, filter)) = self.hidden_pick(viewer, owner) else { return true };
        let hand = self.players[owner as usize].hand.clone();
        let c = hand[rng.below(hand.len() as u32) as usize];
        let ctx = crate::cast::Ctx { controller: owner, source: None };
        if !self.card_matches(&filter, c, &ctx) {
            return false;
        }
        if let Task::Run(f) = &mut self.agenda[k] {
            f.picked[0] = c;
        }
        true
    }

    /// May `p` cast a spell from their hand (cast the way `from` says, from
    /// zone `origin`) without paying its mana cost by a permanent they
    /// control that says so (CR 118.9; one alternative cost at most, 118.9a:
    /// only a cast from the hand for its costs, `CastFrom::Hand`)?
    pub(crate) fn free_from_hand(&self, p: PlayerId, from: crate::game::CastFrom, origin: Zone) -> bool {
        from == crate::game::CastFrom::Hand && origin == Zone::Hand && self.permanents_of(p).any(|c| self.abilities(c).sns.free_casts_from_hand)
    }

    /// Has object `c` the name `name` (CR 201.2): its face's ([`face_name`]);
    /// a split card's, outside the stack, both halves' (709.4; on the stack
    /// it is the half cast, 709.3).
    pub(crate) fn has_name(&self, c: CardIdx, name: &str) -> bool {
        let d = self.exc(c).and_then(|e| e.name).unwrap_or(self.objs[c.i()].def);
        let def = self.db.def(d);
        match def.halves {
            Some((l, r)) => self.db.def(l).name == name || self.db.def(r).name == name,
            None => face_name(def) == name,
        }
    }

    /// The name chosen for permanent `src` as a "choose a card name" of the
    /// plain kind (`SnsDef::plain_name`), if it is one some object can have.
    fn plain_chosen(&self, src: CardIdx) -> Option<&'static str> {
        let nd = self.chosen_name(self.obj_ref(src))?;
        (nd != crate::game::NO_NAME).then(|| face_name(self.db.def(nd)))
    }

    /// May activated ability `ad` of source `card` not be activated because
    /// of a permanent's "activated abilities of sources with the chosen name
    /// can't be activated [unless they're mana abilities]" (CR 602.5, 605.1a;
    /// in every zone: hand, graveyard, battlefield, Pithing Needle's ruling)?
    pub(crate) fn named_ban_hits(&self, card: CardIdx, ad: &crate::db::ActivatedDef) -> bool {
        // (A special action is no ability, CR 116.2b: turning a face-down
        // permanent face up; a face-down permanent has no name, 708.2.)
        if ad.special.is_some() {
            return false;
        }
        let mana = ad.mana().is_some();
        self.battlefield.iter().any(|&s| match self.abilities(s).sns.named_ban {
            Some(except_mana) if !(except_mana && mana) => self.plain_chosen(s).is_some_and(|n| self.has_name(card, n)),
            _ => false,
        })
    }

    /// The names `p` may choose for a permanent's "choose a card name" (CR
    /// 201.3), as definitions (one per name, the smallest definition with
    /// it; sorted by definition), besides `PASS`, which stands for every
    /// name no object of this game can have. The objects of a game are its
    /// cards (created as it starts, from the decks; the sideboards' cards can
    /// join it only as a companion, CR 103.2b, 116.2g: no instruction of the
    /// engine brings another card into a game), its tokens (from the card
    /// table's token definitions) and copies of these, whose names are their
    /// copiable values' (CR 707.2; a copy exception names the copying card,
    /// `EntersAsCopy::keep_name`); a face-down permanent has no name (708.2).
    /// So every name an object can have is a name of a face of a card of the
    /// game or of its sideboards (a split card's halves, 709.4; a back face,
    /// 712; an Adventure, 715; a prepare spell, 722) or a token definition's
    /// name, and a token's name may be chosen only if it is also a card's
    /// (201.3): the names of the table's cards, and
    /// [`TOKEN_NAMES_THAT_ARE_CARD_NAMES`]. With open decklists (ENGINE.md
    /// 54.5) these are the same in every world; with hidden decks (36.11)
    /// the other player's are those of their pool `p` knows (the whole pool
    /// is their deck and sideboard) and their cards `p` knows.
    pub(crate) fn nameable_defs(&self, p: PlayerId) -> Vec<DefId> {
        let q = opp(p);
        let hidden = self.known_pools[q as usize].is_some();
        let mut cards: Vec<DefId> = (0..self.objs.len())
            .map(|i| CardIdx::new(i as u32))
            .filter(|&c| !self.is_token(c) && !self.is_copy_obj(c))
            .filter(|&c| self.objs[c.i()].zone != Zone::Nowhere || self.players.iter().any(|pl| pl.companion == Some(c)))
            .filter(|&c| self.objs[c.i()].owner == p || !hidden || self.knows(p, c))
            .map(|c| self.ident_card(c))
            .collect();
        cards.extend(self.players[p as usize].sideboard.iter().copied());
        match &self.known_pools[q as usize] {
            Some(known) => {
                cards.extend(known.iter().copied());
                for k in self.clouds.iter().filter(|k| k.viewer == p && k.owner == q) {
                    cards.extend(k.defs.iter().map(|&d| self.db.def(d).card_def(d)));
                }
            }
            None => cards.extend(self.players[q as usize].sideboard.iter().copied()),
        }
        let mut names: Vec<DefId> = Vec::new();
        for d in cards {
            let def = self.db.def(d);
            match def.halves {
                Some((l, r)) => names.extend([l, r]),
                None => {
                    names.push(d);
                    names.extend(def.back);
                    names.extend(def.adventure);
                    names.extend(def.prepare);
                }
            }
        }
        // The token definitions whose names are cards' names.
        for t in self.db.num_cards()..self.db.len() {
            let d = self.db.def(t as DefId);
            if d.token
                && !d.name.is_empty()
                && (TOKEN_NAMES_THAT_ARE_CARD_NAMES.contains(&d.name.as_str()) || self.db.id(&d.name).is_some_and(|x| (x as usize) < self.db.num_cards()))
            {
                names.push(t as DefId);
            }
        }
        // One definition per name (`face_name`), the smallest.
        names.sort_unstable_by(|&a, &b| face_name(self.db.def(a)).cmp(face_name(self.db.def(b))).then(a.cmp(&b)));
        names.dedup_by(|a, b| face_name(self.db.def(*a)) == face_name(self.db.def(*b)));
        names.sort_unstable();
        names
    }

    /// `Task::AsEnters` for a plain "as this enters, choose a card name"
    /// (`SnsDef::plain_name`, CR 614.12): `NameCard` over
    /// [`nameable_defs`](Self::nameable_defs) (`LIBRARY` + k: the k-th name;
    /// paged past the 64 ids, ENGINE.md 100.2), `PASS` = a name no object of
    /// this game can have. The name is `Mark::ChosenName` (Anointed
    /// Peacekeeper's).
    pub(crate) fn choose_plain_name_as_enters(&mut self, card: CardIdx, page: crate::decision::Page) {
        let p = self.objs[card.i()].controller;
        let r = self.obj_ref(card);
        match self.answer.take() {
            Some(Choice::Page(next)) => self.agenda.push(Task::AsEnters { card, page: next }),
            Some(Choice::Def(d)) => {
                self.marks.push((r, crate::game::Mark::ChosenName(d)));
                self.note(|g| format!("chosen name: {}", face_name(g.db.def(d))));
            }
            Some(Choice::Pass) => self.marks.push((r, crate::game::Mark::ChosenName(crate::game::NO_NAME))),
            Some(other) => unreachable!("name answer {other:?}"),
            None => {
                let items: Vec<(u32, Choice)> = self.nameable_defs(p).into_iter().enumerate().map(|(i, d)| (i as u32, Choice::Def(d))).collect();
                let pass = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                self.ask_paged(Task::AsEnters { card, page }, page, p, DecisionKind::NameCard, act::LIBRARY, act::LIBRARY_N, &items, pass);
            }
        }
    }
}

/// The name of a face (CR 201.2): a card's definition of a double-faced or
/// adventurer card is named "<front> // <other>" in the card table (its
/// card's name), but the face up (712.8, 715.4) is named "<front>"; every
/// other definition (a back face, an Adventure, a split card's half, a card
/// with one face, a token) is its name. (A split card's own definition has
/// its halves' names, 709.4: `Game::has_name`, `Game::nameable_defs`.)
pub fn face_name(d: &crate::db::CardDef) -> &str {
    if d.front.is_none() && d.halves.is_none() && (d.back.is_some() || d.adventure.is_some()) {
        if let Some((front, _)) = d.name.split_once(" // ") {
            return front;
        }
    }
    &d.name
}

/// Does a card of definition `d` make state layout v6 can't encode exactly
/// (ENGINE.md 123): a permanent's card name chosen as it enters of the plain
/// kind (Disruptor Flute; layout v7's chosen-name column, ENGINE.md 125,
/// encodes `Mark::ChosenName`)? `Some(the refusal)`; one entry of
/// `az::needs_layout_v7`.
pub fn plain_name_needs_v7(d: &crate::db::CardDef) -> Option<String> {
    (d.chooses_name && d.sns.plain_name).then(|| format!("{}'s chosen name needs layout v7 (ENGINE.md 123)", d.name))
}
