//! Decisions, choices and the action-id layout (ENGINE.md 11).

use crate::db::AbilitySlot;
use crate::ids::{CardIdx, DefId, PlayerId, Target};

/// Global action ids are `< NUM_ACTIONS` (zero-core's `SearchGame::NUM_ACTIONS`).
pub const NUM_ACTIONS: usize = 1024;

/// Decision-relative action id ranges. Object slots are canonical and
/// relative to the decider (ENGINE.md 11).
pub mod act {
    /// Pass priority / no / keep / done / no block / decline.
    pub const PASS: u16 = 0;
    /// Yes / take a mulligan / pay.
    pub const YES: u16 = 1;
    /// 2 = the decider, 3 = the opponent.
    pub const PLAYER: u16 = 2;
    /// At priority: the companion special action (CR 116.2g; the decider's
    /// companion, ENGINE.md 35). A `PLAYER` id: "the decider's" card
    /// outside the game.
    pub const COMPANION: u16 = PLAYER;
    /// n = 0..31.
    pub const NUMBER: u16 = 4;
    pub const NUMBER_N: u16 = 32;
    /// The decider's hand, slot 0..63.
    pub const HAND: u16 = 36;
    pub const HAND_N: u16 = 64;
    /// The decider's permanents, slot 0..95.
    pub const MY_PERM: u16 = 100;
    pub const PERM_N: u16 = 96;
    /// The opponent's permanents, slot 0..95.
    pub const OPP_PERM: u16 = 196;
    /// Stack slot 0..31, 0 = top.
    pub const STACK: u16 = 292;
    pub const STACK_N: u16 = 32;
    /// The decider's waiting triggers, slot 0..15.
    pub const TRIGGER: u16 = 324;
    pub const TRIGGER_N: u16 = 16;
    /// A searched / looked-at pile, slot 0..63 (M4b); a graveyard-card
    /// choice or target: its slot in the decision's graveyard list
    /// (`Game::graveyard_list`, ENGINE.md 65.10).
    pub const LIBRARY: u16 = 340;
    pub const LIBRARY_N: u16 = 64;
    /// Graveyards: the decider's 0..63, the opponent's 64..127 (M4b). Only
    /// Priority's casts and plays from the decider's graveyard use them
    /// (ENGINE.md 65.10): every other graveyard choice is a list id.
    pub const GRAVEYARD: u16 = 404;
    /// Slots per graveyard.
    pub const GRAVEYARD_N: u16 = 64;
    /// Activate ability i of the decider's permanent in `MY_PERM` slot s:
    /// `ACTIVATE + s * 4 + i` (at priority, and mana abilities while paying;
    /// in a `Sacrifice` or `BounceCost` decision: its mana ability i, then
    /// the permanent is the one sacrificed or returned, ENGINE.md 86.2).
    pub const ACTIVATE: u16 = 532;
    /// Activate the ability of the card in the decider's hand slot 0..63
    /// (cycling, Spirit Guides).
    pub const HAND_ABILITY: u16 = 916;
    /// Play / cast the k-th card in exile the decider may play (exile
    /// order; M7a), k = 0..31.
    pub const EXILE: u16 = 980;
    pub const EXILE_N: u16 = 32;
    /// Activate the graveyard ability of the k-th card in the decider's
    /// graveyard that has one (graveyard order, bottom first; whether or
    /// not it can be activated now), k = 0..11 (M7b part 3b; ENGINE.md
    /// 27.5).
    pub const GY_ABILITY: u16 = 1012;
    pub const GY_ABILITY_N: u16 = 12;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DecisionKind {
    /// Pass, play a land, cast a spell (CR 117).
    Priority,
    /// Keep (`PASS`) or take a mulligan (`YES`) (CR 103.5).
    Mulligan,
    /// Put a hand card on the bottom after a mulligan (CR 103.5).
    MulliganBottom,
    /// Choose a target (CR 601.2c, 603.3d); `PASS` = no more targets.
    Target,
    /// Choose the next mana source to tap (CR 601.2g).
    PayMana,
    /// Add an attacker (increasing slot order) or `PASS` = done (CR 508.1).
    DeclareAttackers,
    /// Which attacker the next potential blocker blocks, or `PASS` (CR 509.1).
    DeclareBlockers,
    /// Combat damage to the next blocker (`NUMBER`) (CR 510.1c).
    AssignDamage,
    /// Pay a cost / do an optional action: `YES` or `PASS`.
    YesNo,
    /// Choose a card in hand for an effect (Brainstorm).
    ChooseHandCard,
    /// Discard to hand size (CR 514.1).
    Discard,
    /// Which waiting trigger goes on the stack next (CR 603.3b).
    OrderTriggers,
    /// Which legendary permanent to keep (CR 704.5j).
    LegendRule,
    /// How many Phyrexian pips of a colour to pay with 2 life each
    /// (`NUMBER`, CR 107.4f).
    PayPhyrexian,
    /// Which mana a mana ability makes (`NUMBER` = production index, CR
    /// 106.1b).
    ChooseMana,
    /// A permanent to sacrifice as a cost (CR 701.17): `MY_PERM` + its slot;
    /// `ACTIVATE` + slot x 4 + i: an untapped mana source's mana ability i
    /// first (CR 601.2g-h, ENGINE.md 86.2), then it is the one sacrificed.
    Sacrifice,
    /// A card to discard as a cost.
    DiscardCost,
    /// Which additional cost to pay (`NUMBER` = option index, CR 601.2b).
    ChooseCost,
    /// A card to find in a library search (`LIBRARY` = the slot of its
    /// definition in the library sorted by definition; `PASS` = find
    /// nothing; CR 701.19).
    SearchLibrary,
    /// The next card of a looked-at pile to put on top of the library
    /// (`LIBRARY` = its slot in the pile sorted by definition).
    PutOnTop,
    /// The next card of the pile to put on the bottom (`PASS` = done when
    /// the number is free: scry).
    PutOnBottom,
    /// A card of the pile to put into the graveyard (`PASS` = done:
    /// surveil).
    PutInGraveyard,
    /// A card of the pile to put into the hand (`PASS` = done when allowed).
    PutInHand,
    /// A card of another player's revealed hand for them to discard
    /// (`LIBRARY` = its slot in that hand sorted by definition; `PASS` =
    /// none, when allowed).
    ChooseDiscard,
    /// The next mode of a modal spell or ability (`NUMBER` = mode index, in
    /// non-decreasing order; `PASS` = no more, once enough are chosen; CR
    /// 700.2).
    ChooseMode,
    /// A colour (`NUMBER` = W U B R G index; 5 = colorless).
    ChooseColor,
    /// A permanent chosen by an effect (not targeted), one per decision in
    /// increasing slot order; `PASS` = no more.
    ChoosePermanent,
    /// A card in a graveyard chosen by an effect or a cost (delve, Exhume, a
    /// graveyard exile cost): `LIBRARY` + its slot in the decision's
    /// graveyard list (`Game::graveyard_list`, ENGINE.md 65.10).
    ChooseGraveyardCard,
    /// Which cost to cast a spell for: `NUMBER` 0 = its mana cost, k + 1 =
    /// alternative cost k (CR 118.9, 601.2b).
    ChooseAltCost,
    /// A card in the decider's hand to exile as a cost.
    ExileCost,
    /// A permanent to return to its owner's hand as a cost (Daze): as
    /// `Sacrifice`, `ACTIVATE` ids included (ENGINE.md 86.2).
    BounceCost,
    /// The amount of divided damage for the next target (`NUMBER`, CR
    /// 601.2d).
    DivideDamage,
    /// What the attacker just declared (or put onto the battlefield
    /// attacking) attacks: `PLAYER` + 1 = the defending player, `OPP_PERM`
    /// = a planeswalker they control (CR 508.1b, 508.4). Asked only when
    /// the defending player controls a planeswalker.
    AttackTarget,
    /// A card in exile chosen by an effect (`EXILE` + k = the k-th offered
    /// card in exile order; Karn's "a card you own with a silver counter").
    ChooseExiled,
    /// Name a card (`LIBRARY` = a definition in the namer's library sorted
    /// by definition, `Choice::Def`; `PASS` = a name of no card in it).
    NameCard,
    /// Which face of the card chosen at priority to cast or play (CR
    /// 712.11b, 712.12, 715.3): `NUMBER` 0 = its front face (the card
    /// itself), 1 = its other face (a modal double-faced card's back face,
    /// an Adventure). Asked only when both can be cast or played.
    ChooseFace,
    /// A new target for a copy of a spell (CR 707.10c), its targets one
    /// decision each in order: a legal target for that requirement, or
    /// `PASS` = keep the one it has (even if illegal). M7b part 3.
    NewTarget,
    /// The room the venture marker moves into (CR 309.5, 701.49b): `NUMBER`
    /// = the room's index in its dungeon (Undercity: `dungeon::ROOMS`),
    /// among the rooms the current one leads to. M7b part 3b.
    ChooseRoom,
    /// A card of a looked-at or revealed pile to put onto the battlefield
    /// (`LIBRARY` = its slot in the pile sorted by definition; Undercity's
    /// Throne of the Dead Three). M7b part 3b.
    PutOntoBattlefield,
    /// Which replacement effect applies to an object moving (the result:
    /// `NUMBER` 1 = shuffled into its owner's library, 2 = exiled, 3 =
    /// exiled with a void counter), chosen by its controller or owner (CR
    /// 616.1). M7e.
    ChooseReplacement,
    /// Which permission casts the card chosen at priority from its owner's
    /// graveyard (`NUMBER` = `cast::route` code: flashback, Yawgmoth's
    /// Will, escape, retrace, mayhem, Serra Paragon, Lurrus), when several
    /// can (CR 601.2a-b). M7e.
    ChooseRoute,
    /// Which mana of the pool a payment leaves (CR 106.4, 601.2h: the
    /// player chooses which mana pays): `NUMBER` + k = the k-th pool it can
    /// leave, asked once the sources are picked and only when the pools
    /// differ (the pool's mana, or a several-mana source's, beyond what the
    /// cost needs). M7e part 2.
    PayPool,
    /// Who takes the first turn (CR 103.1): `PLAYER` = the decider (play
    /// first), `PLAYER` + 1 = the opponent (draw first). M8a.
    PlayDraw,
    /// Reveal a companion from the sideboard (CR 103.2b, 702.139a):
    /// `LIBRARY` + k = the k-th card of the decider's sideboard with a
    /// companion whose condition their starting deck fulfills, by
    /// definition (`Choice::Def`); `PASS` = none. Asked only when there is
    /// one. M8a.
    RevealCompanion,
    /// Sideboarding between the games of a match (CR 100.4, 100.2b; the
    /// match's, not a game's: ENGINE.md 36): the decider changes the count
    /// of entries of their pool list (the pool's cards sorted by
    /// definition, then the five basic lands), in increasing list order:
    /// `LIBRARY` + k = change entry k (a single card in or out of the deck;
    /// a basic land or a card with several copies: a `SideboardCount`
    /// follows), k after the last entry changed; `PASS` = done (the deck as
    /// it is, legal: 40 cards or more). M8b.
    Sideboard,
    /// The new count in the deck of the entry just chosen (`NUMBER` + n, n
    /// other than its count now). M8b.
    SideboardCount,
    // (At most 64 kinds: the network's context block has 64 slots, layout v3. "Top or
    // bottom" is a `YesNo`: `YES` = top.)
}

/// What an action means to the task that asked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Choice {
    Pass,
    Yes,
    Number(u8),
    /// A card or permanent (hand card, attacker, blocker's attacker, mana
    /// source, legend to keep).
    Card(CardIdx),
    /// Priority: play this land.
    Play(CardIdx),
    /// Priority: cast this spell.
    Cast(CardIdx),
    Target(Target),
    /// Index into the decider's waiting triggers.
    Trigger(u32),
    /// Activate this ability of this card (priority; a mana source while
    /// paying).
    Ability(CardIdx, AbilitySlot),
    /// A card of this definition in a zone hidden from the decider (a
    /// library search): which physical card is never part of the choice
    /// (ENGINE.md 10).
    Def(DefId),
    /// Priority: cast this card's other face (a modal double-faced card's
    /// back face, an Adventure; CR 712.11b, 715.3).
    CastFace(CardIdx),
    /// Priority: play this card's other face, a land (CR 712.12).
    PlayFace(CardIdx),
    /// Priority: this card, whose two faces can both be cast or played: a
    /// `ChooseFace` decision follows (`FACE_*` bits: what each face does).
    Faces(CardIdx, u8),
    /// Priority: the companion special action (CR 116.2g): pay {3} and put
    /// the decider's companion into their hand (`act::COMPANION`).
    Companion,
    /// A paged choice's "next page" ([`Game::ask_paged`], ENGINE.md 100.1):
    /// the page the asking task asks next.
    Page(Page),
    /// The k-th of the asking task's list, a paged choice of any length (the
    /// pools a payment can leave, `PayPool`; ENGINE.md 100.4).
    Item(u32),
}

/// `Choice::Faces` bits: the front face is cast / played, the other face
/// is cast / played.
pub const FACE_CAST: u8 = 1;
pub const FACE_PLAY: u8 = 2;
pub const OTHER_CAST: u8 = 4;
pub const OTHER_PLAY: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Opt {
    pub action: u16,
    pub choice: Choice,
}

/// The one pending decision: options sorted by action id, no duplicates,
/// never empty.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Decision {
    pub player: PlayerId,
    pub kind: DecisionKind,
    pub options: Vec<Opt>,
    /// Where the decision stands in a choice made in several decisions (a
    /// page, a digit; ENGINE.md 100.1): the context block's 246-250.
    pub stage: Stage,
}

// ---- paged choices and big numbers (ENGINE.md 100.1) -----------------------
//
// Every legal option stays reachable inside the fixed action ranges (CR
// 601.2c: any legal target; 601.2b, 107.3: any X): a choice among more items
// than a range has ids is made in pages, and a number beyond `NUMBER`'s 32
// ids in base-32 digits. Each item or value is reached by exactly one
// sequence of decisions, and the page or the digits chosen live in the
// asking task, so a refresh after `determinize` asks the same decision again
// (ENGINE.md 10, 99.8).

/// Which page of a paged choice a decision shows ([`Game::ask_paged`]),
/// kept in the asking task. `window` 0: the first page (every range's first
/// window, the ranges that fit whole, the other options); `window` w >= 1:
/// the items of range `range` in window w alone (positions w (n - 1) ..= w
/// (n - 1) + n - 2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Page {
    pub range: u8,
    pub window: u32,
}

impl Page {
    /// A paged choice's first decision.
    pub const FIRST: Page = Page { range: 0, window: 0 };
}

/// One action range of a paged choice: the ids `base .. base + n`, and its
/// items by their position in the range's canonical order (strictly
/// increasing; an item that fits has the id `base + position`, as a choice
/// that is not paged gives it: positions may skip, the stack's spells among
/// its abilities).
#[derive(Clone, Copy, Debug)]
pub struct PagedRange<'a> {
    pub base: u16,
    pub n: u16,
    pub items: &'a [(u32, Choice)],
}

impl PagedRange<'_> {
    /// An item lies past the range's last id: the range is paged.
    fn paged(&self) -> bool {
        self.items.last().is_some_and(|&(pos, _)| pos >= self.n as u32)
    }

    /// The window of a position: `n - 1` positions each (the last id is
    /// "next page").
    fn window(&self, pos: u32) -> u32 {
        pos / (self.n as u32 - 1)
    }

    /// The first window after `w` that holds an item (windows with none are
    /// skipped: no page offers only "next").
    fn next_window(&self, w: u32) -> Option<u32> {
        self.items.iter().map(|&(pos, _)| self.window(pos)).find(|&x| x > w)
    }

    fn holds(&self, action: u16) -> bool {
        (self.base..self.base + self.n).contains(&action)
    }
}

/// Where a big-number choice stands ([`Game::ask_number`]), kept in the
/// asking task: `left` 0 for its first decision; else the base-32 digits
/// still to choose (this one included), most significant first, and
/// `prefix` the digits chosen so far (as a number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct NumberStep {
    pub left: u8,
    pub prefix: u64,
}

/// What an answer to [`Game::ask_number`]'s decision means: the value
/// chosen, or the step whose decision comes next (the asking task pushes
/// itself with it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumberAnswer {
    Value(u64),
    Next(NumberStep),
}

/// The largest number of base-32 digits a `u64` range needs (32^13 > 2^64).
const MAX_DIGITS: u8 = 13;

impl NumberStep {
    /// A number's first decision.
    pub const FIRST: NumberStep = NumberStep { left: 0, prefix: 0 };

    /// The digits of the rest `d = v - lo - 31` of a range wider than
    /// `NUMBER` (`hi - lo >= 32`): enough for its largest, `hi - lo - 31`
    /// (at least one; 7 for any `u32` range).
    fn digits(lo: u64, hi: u64) -> u8 {
        let d = hi - lo - 31;
        let mut n = 1;
        while n < MAX_DIGITS && d >> (5 * n as u32) > 0 {
            n += 1;
        }
        n
    }

    /// `d >> (5 r)`: the rest's digits above the last `r` (0 when `r`
    /// covers them all).
    fn above(d: u64, r: u8) -> u64 {
        d.checked_shr(5 * r as u32).unwrap_or(0)
    }

    /// The step as the range allows it: the first decision when the range
    /// fits `NUMBER`, or when the digits chosen have no legal completion
    /// left (another player's decision refreshed after `determinize` in a
    /// world where the range differs, ENGINE.md 10; never the decider's,
    /// whose range is public to them).
    fn normalized(self, lo: u64, hi: u64) -> NumberStep {
        if self.left == 0 {
            return self;
        }
        if hi - lo <= 31 {
            return NumberStep::FIRST;
        }
        let d = hi - lo - 31;
        if self.left > Self::digits(lo, hi) || self.prefix > Self::above(d, self.left) {
            return NumberStep::FIRST;
        }
        self
    }

    /// The largest digit this digit decision offers (prefix feasibility:
    /// the prefix with the digit and zeros after it is still at most the
    /// rest's largest, so a legal completion exists; at least 0).
    fn max_digit(self, lo: u64, hi: u64) -> u64 {
        let d = hi - lo - 31;
        (Self::above(d, self.left - 1) - self.prefix * 32).min(31)
    }

    /// What answer `NUMBER + k` to this step's decision for a number in
    /// `lo..=hi` means (the same range the decision was asked with).
    pub fn answer(self, lo: u64, hi: u64, k: u8) -> NumberAnswer {
        assert!(lo <= hi, "a number in {lo}..={hi}");
        let s = self.normalized(lo, hi);
        let k64 = k as u64;
        if s.left == 0 {
            if hi - lo <= 31 {
                assert!(k64 <= hi - lo, "NUMBER + {k} for a number in {lo}..={hi}");
                return NumberAnswer::Value(lo + k64);
            }
            if k < 31 {
                return NumberAnswer::Value(lo + k64);
            }
            assert_eq!(k, 31, "NUMBER + {k} for a number in {lo}..={hi}");
            return NumberAnswer::Next(NumberStep { left: Self::digits(lo, hi), prefix: 0 });
        }
        assert!(k64 <= s.max_digit(lo, hi), "digit {k} after {s:?} for a number in {lo}..={hi}");
        let prefix = s.prefix * 32 + k64;
        if s.left == 1 {
            NumberAnswer::Value(lo + 31 + prefix)
        } else {
            NumberAnswer::Next(NumberStep { left: s.left - 1, prefix })
        }
    }
}

impl NumberStep {
    /// What answer `NUMBER + k` means for an amount asked by
    /// [`Game::ask_amount`] in `lo..=hi`: the value `k` itself while every
    /// value fits `NUMBER`, else as [`answer`](Self::answer).
    pub fn answer_amount(self, lo: u64, hi: u64, k: u8) -> NumberAnswer {
        if hi <= 31 {
            assert!((lo..=hi).contains(&(k as u64)), "NUMBER + {k} for an amount in {lo}..={hi}");
            NumberAnswer::Value(k as u64)
        } else {
            self.answer(lo, hi, k)
        }
    }
}

/// The largest value in `lo..=cap` that `ok` allows, `ok(lo)` being true
/// (ENGINE.md 100.2): one value at a time through 31 (the values `NUMBER`
/// names, checked in the order they were before; the first refused one
/// ends it), then by doubling and bisection, `ok` being monotone there (a
/// larger X or count costs more).
pub(crate) fn most_allowed(lo: u64, cap: u64, mut ok: impl FnMut(u64) -> bool) -> u64 {
    let mut v = lo;
    while v < cap && v < 31 {
        if !ok(v + 1) {
            return v;
        }
        v += 1;
    }
    if v >= cap {
        return v;
    }
    let (mut good, mut step) = (v, 1u64);
    let mut bad = loop {
        let t = good.saturating_add(step).min(cap);
        if !ok(t) {
            break t;
        }
        good = t;
        if good == cap {
            return cap;
        }
        step = step.saturating_mul(2);
    };
    while bad - good > 1 {
        let m = good + (bad - good) / 2;
        if ok(m) {
            good = m;
        } else {
            bad = m;
        }
    }
    good
}

/// Where a decision stands in a choice made in several decisions (ENGINE.md
/// 100.1), for the network (the context block's 246-250).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Stage {
    /// A choice made in one decision.
    #[default]
    One,
    /// A page of a paged choice ([`Game::ask_paged`]: a paged range's last
    /// id is "next page", its other ids name the items of window `window`),
    /// or a big number's first decision (`NUMBER` + 31 = "31 or more",
    /// window 0).
    Paged { window: u32 },
    /// A digit of a big number ([`Game::ask_number`]): `left` digits to
    /// choose (this one included), `prefix` the digits chosen.
    Digit { left: u8, prefix: u64 },
}

impl Decision {
    pub fn choice_of(&self, action: u16) -> Option<Choice> {
        self.options
            .binary_search_by_key(&action, |o| o.action)
            .ok()
            .map(|i| self.options[i].choice)
    }

    pub fn action_of(&self, choice: Choice) -> Option<u16> {
        self.options.iter().find(|o| o.choice == choice).map(|o| o.action)
    }
}

// ---- the slot codec (ENGINE.md 11) -----------------------------------------

use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::Zone;
use crate::svec::SVec;

impl Game {
    /// `p`'s hand in canonical order: by definition, then card index.
    pub fn hand_sorted(&self, p: PlayerId) -> Vec<CardIdx> {
        self.hand_sorted_in(p).to_vec()
    }

    /// [`hand_sorted`](Self::hand_sorted) in inline storage (no allocation
    /// for a hand of up to 16 cards; ENGINE.md 17).
    pub(crate) fn hand_sorted_in(&self, p: PlayerId) -> SVec<CardIdx, 16> {
        let mut h: SVec<CardIdx, 16> = self.players[p as usize].hand.iter().copied().collect();
        h.sort_by_key(|c| (self.objs[c.i()].def, c.0));
        h
    }

    /// One (slot, card) per distinct definition in `p`'s hand: cards in a
    /// hidden zone have no other state, so equal definitions are equivalent.
    pub fn hand_slots(&self, p: PlayerId) -> Vec<(u16, CardIdx)> {
        self.hand_slots_in(p).to_vec()
    }

    /// [`hand_slots`](Self::hand_slots) in inline storage.
    pub(crate) fn hand_slots_in(&self, p: PlayerId) -> SVec<(u16, CardIdx), 16> {
        let h = self.hand_sorted_in(p);
        let mut out = SVec::new();
        for (i, &c) in h.iter().enumerate() {
            if i > 0 && self.objs[h[i - 1].i()].def == self.objs[c.i()].def {
                continue;
            }
            assert!((i as u16) < act::HAND_N, "hand slot {i} out of range");
            out.push((act::HAND + i as u16, c));
        }
        out
    }

    /// Options "choose a card in `p`'s hand", one per distinct definition.
    pub(crate) fn hand_options(&self, p: PlayerId) -> Vec<Opt> {
        self.hand_slots_in(p).iter().map(|&(a, c)| Opt { action: a, choice: Choice::Card(c) }).collect()
    }

    /// [`hand_options`](Self::hand_options) without one card (an ability's
    /// source in hand); a definition keeps its canonical slot.
    pub(crate) fn hand_options_except(&self, p: PlayerId, except: Option<CardIdx>) -> Vec<Opt> {
        let h = self.hand_sorted_in(p);
        let mut out = Vec::new();
        let mut i = 0;
        while i < h.len() {
            let def = self.objs[h[i].i()].def;
            let mut j = i;
            while j < h.len() && self.objs[h[j].i()].def == def {
                j += 1;
            }
            if let Some(&c) = h[i..j].iter().find(|&&c| Some(c) != except) {
                assert!((i as u16) < act::HAND_N, "hand slot {i} out of range");
                out.push(Opt { action: act::HAND + i as u16, choice: Choice::Card(c) });
            }
            i = j;
        }
        out
    }

    /// `p`'s library in canonical order: by definition, then card index.
    pub fn library_sorted(&self, p: PlayerId) -> Vec<CardIdx> {
        let mut l = self.players[p as usize].library.clone();
        l.sort_by_key(|c| (self.objs[c.i()].def, c.0));
        l
    }

    /// `MY_PERM` / `OPP_PERM` slot of a permanent, relative to `viewer`.
    pub fn perm_action(&self, viewer: PlayerId, idx: CardIdx) -> u16 {
        let ctl = self.objs[idx.i()].controller;
        let slot = self.perm_slot(idx);
        assert!(slot < act::PERM_N as u32, "permanent slot {slot} out of range");
        if ctl == viewer {
            act::MY_PERM + slot as u16
        } else {
            act::OPP_PERM + slot as u16
        }
    }

    /// A permanent's place among its controller's permanents (timestamp
    /// order, ENGINE.md 11): its `MY_PERM` / `OPP_PERM` position, which a
    /// paged choice may put past the 96 slots (ENGINE.md 100.2).
    pub fn perm_slot(&self, idx: CardIdx) -> u32 {
        let ctl = self.objs[idx.i()].controller;
        self.permanents_of(ctl).position(|c| c == idx).expect("permanent on the battlefield") as u32
    }

    /// `STACK` slot (0 = top) of the stack item with this id.
    pub fn stack_action(&self, id: u32) -> u16 {
        let slot = self.stack_slot(id);
        assert!(slot < act::STACK_N as u32, "stack slot {slot} out of range");
        act::STACK + slot as u16
    }

    /// A stack item's place from the top (0 = top): its `STACK` position,
    /// which a paged choice may put past the 32 slots (ENGINE.md 100.2).
    pub fn stack_slot(&self, id: u32) -> u32 {
        let pos = self.stack.iter().position(|s| s.id == id).expect("stack item");
        (self.stack.len() - 1 - pos) as u32
    }

    /// `GRAVEYARD` slot of a card: its position in its owner's graveyard
    /// (0 = bottom), the decider's first (ENGINE.md 11). Priority's casts
    /// and plays from the graveyard keep these ids; every other decision
    /// renames them to the card's slot in its graveyard list
    /// ([`graveyard_list`](Self::graveyard_list), ENGINE.md 65.10).
    pub fn graveyard_action(&self, viewer: PlayerId, idx: CardIdx) -> u16 {
        let owner = self.objs[idx.i()].owner;
        let pos = self.players[owner as usize].graveyard.iter().position(|&c| c == idx).expect("card in a graveyard");
        assert!(pos < act::GRAVEYARD_N as usize, "graveyard slot {pos} out of range");
        act::GRAVEYARD + if owner == viewer { 0 } else { act::GRAVEYARD_N } + pos as u16
    }

    /// The card in a graveyard an option of a graveyard-list decision names
    /// (ENGINE.md 65.10): a card chosen (`Choice::Card`) or targeted
    /// (`Choice::Target`) there. A graveyard is public: the choice names the
    /// card itself (ENGINE.md 10's rule is for hidden zones).
    fn graveyard_choice(&self, c: Choice) -> Option<CardIdx> {
        let idx = match c {
            Choice::Card(i) => i,
            Choice::Target(Target::Obj(r)) => r.idx,
            _ => return None,
        };
        (self.objs[idx.i()].zone == Zone::Graveyard).then_some(idx)
    }

    /// The graveyard list of a decision of `kind` for `p` with these
    /// options (ENGINE.md 65.10), for a `ChooseGraveyardCard`, `Target` or
    /// `NewTarget` decision offering a card in a graveyard: when the offered
    /// cards are in one graveyard, that graveyard whole, in order (bottom
    /// first: the cards passed, chosen and still to come of a set picked in
    /// increasing position, delve's); when they are in both, the offered
    /// cards only, `p`'s graveyard first, each in order. A graveyard card's
    /// option is `LIBRARY` + its slot here, and the list is what the
    /// network's list block holds while the decision is pending, so each
    /// such option is scored by its card's token. Empty for every other
    /// decision (Priority's casts from the graveyard keep their `GRAVEYARD`
    /// ids). At most 64 cards (the list's slots), asserted when the decision
    /// is asked: one graveyard holds at most 64 (the deck limit, ENGINE.md
    /// 58.3); more than 64 cards offered from both is a loud failure
    /// (docs/shortcuts.md 65.10).
    pub fn graveyard_list(&self, p: PlayerId, kind: DecisionKind, options: &[Opt]) -> Vec<CardIdx> {
        use DecisionKind as K;
        if !matches!(kind, K::ChooseGraveyardCard | K::Target | K::NewTarget) {
            return Vec::new();
        }
        let offered: SVec<CardIdx, 16> = options.iter().filter_map(|o| self.graveyard_choice(o.choice)).collect();
        self.graveyard_list_of(p, &offered)
    }

    /// The graveyard list of a decision offering the graveyard cards
    /// `offered` to `p` ([`graveyard_list`](Self::graveyard_list)): the one
    /// graveyard holding them whole, in order, or the offered cards of both,
    /// `p`'s first.
    fn graveyard_list_of(&self, p: PlayerId, offered: &[CardIdx]) -> Vec<CardIdx> {
        let mine = offered.iter().any(|&c| self.objs[c.i()].owner == p);
        let theirs = offered.iter().any(|&c| self.objs[c.i()].owner != p);
        let gy = |q: PlayerId| &self.players[q as usize].graveyard;
        match (mine, theirs) {
            (false, false) => Vec::new(),
            (true, false) => gy(p).clone(),
            (false, true) => gy(crate::ids::opp(p)).clone(),
            (true, true) => gy(p).iter().chain(gy(crate::ids::opp(p))).copied().filter(|c| offered.contains(c)).collect(),
        }
    }

    /// `ask`'s renaming (ENGINE.md 65.10): every option of a graveyard-list
    /// decision that names a graveyard card (offered with its `GRAVEYARD`
    /// id) gets `LIBRARY` + its slot in the decision's graveyard list. The
    /// list is the decider's graveyard then the other player's, each in
    /// order, so the renaming keeps the options' order (only `PASS`,
    /// `PLAYER`, permanent and stack ids, all below `LIBRARY`, share these
    /// decisions). Any other decision but Priority must not offer a
    /// `GRAVEYARD` id.
    pub(crate) fn name_graveyard_options(&self, p: PlayerId, kind: DecisionKind, options: &mut [Opt]) {
        if kind == DecisionKind::Priority {
            return;
        }
        let gy_id = |a: u16| (act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&a);
        // (Options [`ask_objects`](Self::ask_objects) named already carry
        // their list ids: nothing to rename.)
        let list = if options.iter().any(|o| gy_id(o.action)) { self.graveyard_list(p, kind, options) } else { Vec::new() };
        if !list.is_empty() {
            assert!(
                list.len() <= act::LIBRARY_N as usize,
                "{kind:?} for P{p}: {} graveyard cards offered from both graveyards (they hold {} and {}): more than the list's {} slots \
                 (docs/shortcuts.md 65.10)",
                list.len(),
                self.players[p as usize].graveyard.len(),
                self.players[crate::ids::opp(p) as usize].graveyard.len(),
                act::LIBRARY_N
            );
            for o in options.iter_mut() {
                if let Some(c) = self.graveyard_choice(o.choice) {
                    assert_eq!(o.action, self.graveyard_action(p, c), "{kind:?}: a graveyard card's option is offered with its GRAVEYARD id");
                    let k = list.iter().position(|&x| x == c).expect("an offered card is in its graveyard list");
                    o.action = act::LIBRARY + k as u16;
                }
            }
        }
        assert!(
            !options.iter().any(|o| (act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&o.action)),
            "{kind:?} offers a GRAVEYARD id: only Priority's casts keep them (ENGINE.md 65.10)"
        );
    }

    /// The list block's graveyard cards for `p`'s pending decision `d` (the
    /// network's list, ENGINE.md 65.10, 100.2): slot k holds the card of
    /// `LIBRARY` + k. Unpaged, the decision's graveyard list
    /// ([`graveyard_list`](Self::graveyard_list)), every slot filled; on a
    /// page of a paged list (more graveyard cards offered than the 64 list
    /// slots, [`ask_objects`](Self::ask_objects)), the page's offered cards
    /// at their slots and the other slots empty, so every option is still
    /// scored by its card's token.
    pub fn decision_graveyard_list(&self, p: PlayerId, d: &Decision) -> Vec<Option<CardIdx>> {
        use DecisionKind as K;
        if !matches!(d.kind, K::ChooseGraveyardCard | K::Target | K::NewTarget) {
            return Vec::new();
        }
        let list_id = |a: u16| (act::LIBRARY..act::LIBRARY + act::LIBRARY_N).contains(&a);
        let named = |o: &Opt| list_id(o.action) && self.graveyard_choice(o.choice).is_some();
        let paged = d.options.iter().any(|o| o.action == act::LIBRARY + act::LIBRARY_N - 1 && matches!(o.choice, Choice::Page(_)))
            || matches!(d.stage, Stage::Paged { window } if window > 0) && d.options.iter().any(named);
        if !paged {
            return self.graveyard_list(p, d.kind, &d.options).into_iter().map(Some).collect();
        }
        let mut v = Vec::new();
        for o in d.options.iter().filter(|o| named(o)) {
            let k = (o.action - act::LIBRARY) as usize;
            v.resize(v.len().max(k + 1), None);
            v[k] = self.graveyard_choice(o.choice);
        }
        v
    }

    /// Ask `player` to choose one of the objects `items` (each a
    /// `Choice::Card` or `Choice::Target`: a player, a permanent, a spell or
    /// an ability on the stack, a card in a graveyard or in exile) or one of
    /// `extra` (`PASS`, ...), showing page `page` (ENGINE.md 100.2; the
    /// faithfulness audit's F1). Each object has the id its range gives it
    /// when the choice is not paged: a player `PLAYER` (+ 1: the
    /// opponent), a permanent its `MY_PERM` / `OPP_PERM` slot, a stack item
    /// its `STACK` slot (0 = top), a graveyard card `LIBRARY` + its slot in
    /// the decision's graveyard list (65.10), an exiled card `EXILE` + its
    /// place among the exiled cards offered (in `items`' order: exile order,
    /// 99.14). A range with an object past its last id is paged
    /// ([`ask_paged_ranges`](Self::ask_paged_ranges); the ranges' order
    /// below is `Page::range`), so every object is offered (CR 115.1,
    /// 601.2c: any legal target; a choice "of a card" names any). The
    /// asking task follows `ask_paged`'s pattern: it answers `Choice::Page`
    /// by asking again with that page, and recomputes `items` each time.
    pub(crate) fn ask_objects(&mut self, task: Task, page: Page, player: PlayerId, kind: DecisionKind, items: &[Choice], mut extra: Vec<Opt>) {
        let gy_cards: SVec<CardIdx, 16> = items.iter().filter_map(|&c| self.graveyard_choice(c)).collect();
        let list = self.graveyard_list_of(player, &gy_cards);
        let (mut mine, mut theirs, mut stack, mut gy, mut ex) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &c in items {
            let t = match c {
                Choice::Card(i) => Target::Obj(self.obj_ref(i)),
                Choice::Target(t) => t,
                other => panic!("{kind:?}: {other:?} is not an object"),
            };
            match t {
                Target::Player(q) => extra.push(Opt { action: act::PLAYER + (q != player) as u16, choice: c }),
                Target::Stack(id) => stack.push((self.stack_slot(id), c)),
                Target::Obj(r) => {
                    let o = &self.objs[r.idx.i()];
                    match o.zone {
                        Zone::Battlefield if o.controller == player => mine.push((self.perm_slot(r.idx), c)),
                        Zone::Battlefield => theirs.push((self.perm_slot(r.idx), c)),
                        Zone::Stack => {
                            let s = self.stack.iter().find(|s| s.kind == StackKind::Spell { card: r.idx }).expect("a spell on the stack");
                            stack.push((self.stack_slot(s.id), c));
                        }
                        Zone::Graveyard => {
                            let k = list.iter().position(|&x| x == r.idx).expect("an offered card is in its graveyard list");
                            gy.push((k as u32, c));
                        }
                        Zone::Exile => ex.push((ex.len() as u32, c)),
                        z => panic!("{kind:?}: no range for an object in {z:?}"),
                    }
                }
            }
        }
        for v in [&mut mine, &mut theirs, &mut stack, &mut gy] {
            v.sort_by_key(|&(pos, _)| pos);
        }
        let ranges = [
            PagedRange { base: act::MY_PERM, n: act::PERM_N, items: &mine },
            PagedRange { base: act::OPP_PERM, n: act::PERM_N, items: &theirs },
            PagedRange { base: act::STACK, n: act::STACK_N, items: &stack },
            PagedRange { base: act::LIBRARY, n: act::LIBRARY_N, items: &gy },
            PagedRange { base: act::EXILE, n: act::EXILE_N, items: &ex },
        ];
        self.ask_paged_ranges(task, page, player, kind, &ranges, extra);
    }

    /// [`ask_objects`](Self::ask_objects) for options built with their
    /// unpaged ids: an option naming an object (a `Choice::Card` or
    /// `Choice::Target` with a permanent, stack, list, graveyard or exile id)
    /// is an item, the others (`PASS`, `PLAYER`) stay as they are.
    pub(crate) fn ask_object_opts(&mut self, task: Task, page: Page, player: PlayerId, kind: DecisionKind, opts: Vec<Opt>) {
        let (objs, extra): (Vec<Opt>, Vec<Opt>) =
            opts.into_iter().partition(|o| o.action >= act::MY_PERM && matches!(o.choice, Choice::Card(_) | Choice::Target(_)));
        let items: Vec<Choice> = objs.into_iter().map(|o| o.choice).collect();
        self.ask_objects(task, page, player, kind, &items, extra);
    }

    // ---- paged choices and big numbers (ENGINE.md 100.1) -------------------

    /// Ask `player` to choose among `items` in the range `base .. base + n`
    /// (positions in the range's canonical order), or one of `extra` (ids
    /// outside the range: `PASS`, other ranges): `ask_paged_ranges` with
    /// one range ([`Self::ask_paged_ranges`]).
    // (No caller yet: workstream A applies the helpers; the unit tests below
    // drive them. ENGINE.md 100.1.)
    #[allow(dead_code, clippy::too_many_arguments)]
    pub(crate) fn ask_paged(
        &mut self,
        task: Task,
        page: Page,
        player: PlayerId,
        kind: DecisionKind,
        base: u16,
        n: u16,
        items: &[(u32, Choice)],
        extra: Vec<Opt>,
    ) {
        self.ask_paged_ranges(task, page, player, kind, &[PagedRange { base, n, items }], extra);
    }

    /// Ask `player` a choice whose items may outnumber their action ranges
    /// (ENGINE.md 100.1; the faithfulness audit's F1), showing page `page`
    /// (kept in `task`, which the decision's answer comes back to, as for
    /// [`ask`](Self::ask)):
    ///
    /// * A range whose items' positions are all below its `n` ids is offered
    ///   whole at `base + position`, as an unpaged choice offers it: with no
    ///   paged range, the decision is exactly the one `ask` makes.
    /// * A paged range (an item at position `n` or beyond) has windows of
    ///   `n - 1` positions. The first page offers its window-0 items at
    ///   `base + position` and `base + n - 1` = "next page"
    ///   (`Choice::Page`), with the whole ranges and `extra`; a range page
    ///   (`page.window` >= 1) offers the range's items of that window alone,
    ///   at `base + position - window (n - 1)`, and "next page" if a later
    ///   window holds one. "Next page" goes to the next window that holds an
    ///   item; the last page has none.
    ///
    /// Each item lies in one window and the pages are visited in increasing
    /// order, and `extra` and the whole ranges are on the first page only,
    /// so every option is reached by exactly one sequence of decisions. The
    /// asking task answers `Choice::Page(p)` by pushing itself with page `p`
    /// (it re-asks), and any other choice as its own. It recomputes `items`
    /// from the game each time it runs (re-entrant), so a refresh asks the
    /// same page again; a range page that holds no item any more (another
    /// player's decision refreshed in a world where the items differ,
    /// ENGINE.md 10) shows the first page.
    ///
    /// `ask` renames graveyard and exile cards of `Target`, `NewTarget` and
    /// `ChooseGraveyardCard` options (ENGINE.md 65.10, 99.14) after this:
    /// a paged range of such cards takes its positions from that naming
    /// (workstream A).
    #[allow(dead_code)]
    pub(crate) fn ask_paged_ranges(
        &mut self,
        task: Task,
        page: Page,
        player: PlayerId,
        kind: DecisionKind,
        ranges: &[PagedRange],
        extra: Vec<Opt>,
    ) {
        for (k, r) in ranges.iter().enumerate() {
            assert!(r.n >= 2 && (r.base + r.n) as usize <= NUM_ACTIONS, "{kind:?}: paged range {k} at {} of {} ids", r.base, r.n);
            assert!(r.items.windows(2).all(|w| w[0].0 < w[1].0), "{kind:?}: paged range {k}'s positions are not increasing");
            assert!(ranges[..k].iter().all(|q| q.base + q.n <= r.base || r.base + r.n <= q.base), "{kind:?}: paged ranges overlap");
            assert!(!extra.iter().any(|o| r.holds(o.action)), "{kind:?}: an extra option in paged range {k}");
        }
        let shown = match ranges.get(page.range as usize) {
            Some(r) if page.window > 0 && r.paged() && r.items.iter().any(|&(pos, _)| r.window(pos) == page.window) => page,
            _ => Page::FIRST,
        };
        let mut opts = Vec::new();
        if shown.window == 0 {
            opts = extra;
            for (k, r) in ranges.iter().enumerate() {
                if !r.paged() {
                    opts.extend(r.items.iter().map(|&(pos, c)| Opt { action: r.base + pos as u16, choice: c }));
                    continue;
                }
                for &(pos, c) in r.items.iter().take_while(|&&(pos, _)| r.window(pos) == 0) {
                    opts.push(Opt { action: r.base + pos as u16, choice: c });
                }
                let next = r.next_window(0).expect("a paged range holds an item past its first window");
                opts.push(Opt { action: r.base + r.n - 1, choice: Choice::Page(Page { range: k as u8, window: next }) });
            }
        } else {
            let r = &ranges[shown.range as usize];
            let first = shown.window * (r.n as u32 - 1);
            for &(pos, c) in r.items.iter().filter(|&&(pos, _)| r.window(pos) == shown.window) {
                opts.push(Opt { action: r.base + (pos - first) as u16, choice: c });
            }
            if let Some(next) = r.next_window(shown.window) {
                opts.push(Opt { action: r.base + r.n - 1, choice: Choice::Page(Page { range: shown.range, window: next }) });
            }
        }
        let paged = ranges.iter().any(|r| r.paged());
        self.ask(task, player, kind, opts);
        if paged {
            if let Some(d) = &mut self.pending {
                d.stage = Stage::Paged { window: shown.window };
            }
        }
    }

    /// Ask `player` for a number in `lo..=hi` (ENGINE.md 100.1; the
    /// faithfulness audit's F2), at step `step` (kept in `task`, as for
    /// [`ask`](Self::ask)):
    ///
    /// * `hi - lo <= 31`: `NUMBER + k` = `lo + k`, every value in one
    ///   decision, exactly as an unpaged number is asked.
    /// * Wider: the first decision offers `NUMBER + k` = `lo + k` for `k` <=
    ///   30 and `NUMBER + 31` = "`lo + 31` or more". The rest `v - lo - 31`
    ///   is then chosen in base-32 digits, most significant first, as many
    ///   as its largest value needs (7 for any `u32` range, 13 for `u64`);
    ///   each digit decision offers exactly the digits that leave a legal
    ///   completion (prefix feasibility).
    ///
    /// Every value is reached by exactly one sequence of decisions (a value
    /// below `lo + 31` has its own id; the others have one fixed-length
    /// digit string). The asking task gives its answer `NUMBER + k` to
    /// [`NumberStep::answer`] with the same range: a value, or the next step
    /// (it pushes itself with it and re-asks). It recomputes `lo` and `hi`
    /// from the game each time it runs, so a refresh asks the same digit
    /// again; digits with no legal completion left (another player's
    /// decision refreshed in a world where the range differs) restart at the
    /// first decision.
    #[allow(dead_code)]
    pub(crate) fn ask_number(&mut self, task: Task, step: NumberStep, player: PlayerId, kind: DecisionKind, lo: u64, hi: u64) {
        assert!(lo <= hi, "{kind:?}: a number in {lo}..={hi}");
        let step = step.normalized(lo, hi);
        let (top, stage) = match step.left {
            0 if hi - lo <= 31 => (hi - lo, Stage::One),
            0 => (31, Stage::Paged { window: 0 }),
            left => (step.max_digit(lo, hi), Stage::Digit { left, prefix: step.prefix }),
        };
        let opts = (0..=top).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
        self.ask(task, player, kind, opts);
        if let Some(d) = &mut self.pending {
            d.stage = stage;
        }
    }

    /// Ask `player` for an amount in `lo..=hi` (ENGINE.md 100.2): `NUMBER` +
    /// the amount itself while every value fits `NUMBER` (`hi` <= 31, as
    /// amounts were asked before), else [`ask_number`](Self::ask_number)
    /// (`NUMBER` + the amount - `lo`, "31 or more" and digits). The asking
    /// task decodes the answer with [`NumberStep::answer_amount`].
    pub(crate) fn ask_amount(&mut self, task: Task, step: NumberStep, player: PlayerId, kind: DecisionKind, lo: u64, hi: u64) {
        assert!(lo <= hi, "{kind:?}: an amount in {lo}..={hi}");
        if hi <= 31 {
            let opts = (lo..=hi).map(|v| Opt { action: act::NUMBER + v as u16, choice: Choice::Number(v as u8) }).collect();
            self.ask(task, player, kind, opts);
        } else {
            self.ask_number(task, step, player, kind, lo, hi);
        }
    }

    /// Human-readable description of an option (tests, logs).
    pub fn describe_choice(&self, c: Choice) -> String {
        match c {
            Choice::Pass => "pass".into(),
            Choice::Yes => "yes".into(),
            Choice::Number(n) => format!("{n}"),
            Choice::Card(i) => format!("{}#{}", self.name(i), i.0),
            Choice::Play(i) => format!("play {}#{}", self.name(i), i.0),
            Choice::Cast(i) => format!("cast {}#{}", self.name(i), i.0),
            Choice::Target(t) => format!("target {}", self.describe_target(t)),
            Choice::Trigger(k) => format!("trigger {k}"),
            Choice::Item(k) => format!("option {}", k + 1),
            Choice::Ability(i, slot) => format!("activate {}#{} {slot:?}", self.name(i), i.0),
            Choice::Def(d) => format!("a {}", self.db.def(d).name),
            Choice::CastFace(i) => format!("cast the other face of {}#{}", self.name(i), i.0),
            Choice::PlayFace(i) => format!("play the other face of {}#{}", self.name(i), i.0),
            Choice::Faces(i, m) => format!("{}#{} (choose a face, {m:#b})", self.name(i), i.0),
            Choice::Companion => "put the companion into hand".into(),
            Choice::Page(p) => format!("more (range {}, window {})", p.range, p.window),
        }
    }
}

/// The paged-choice and big-number helpers (ENGINE.md 100.1), driven by a
/// test-only task (`Task::Probe`) that asks through them and logs what was
/// chosen: every item or value is reached by exactly one sequence of
/// decisions, a choice that fits its range is asked as before, a page or a
/// digit is asked again identically after `determinize`, and the context
/// floats.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::az::encode::{ctx, squash, NUM_FEATURES, O_CONTEXT};
    use crate::db::CardDb;
    use crate::game::Step;
    use crate::rng::Rng;
    use crate::testkit::Scenario;

    /// A test's choice, asked as `Game::run_probe` says.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub(crate) enum Probe {
        /// `ask_paged_ranges` over `ranges` (base, n, the items' positions;
        /// item `pos` of range `r` is [`item`]`(r, pos)`) and `extra`.
        Paged { p: PlayerId, kind: DecisionKind, ranges: Vec<(u16, u16, Vec<u32>)>, extra: Vec<Opt>, page: Page },
        /// `ask_number` for a number in `lo..=hi`.
        Number { p: PlayerId, kind: DecisionKind, lo: u64, hi: u64, step: NumberStep },
    }

    /// The choice naming item `pos` of range `r` (a definition no card has:
    /// the probe only logs it).
    fn item(r: usize, pos: u32) -> Choice {
        assert!(r < 16 && pos < 4096);
        Choice::Def(((r as u16) << 12) | pos as u16)
    }

    impl Game {
        /// Ask-or-consume (ENGINE.md 4), as an engine task using the
        /// helpers does: a page answer or a digit pushes the task with its
        /// new page or step; an item or a value is logged ("probe: ...").
        pub(crate) fn run_probe(&mut self, probe: Probe) {
            match probe {
                Probe::Paged { p, kind, ranges, extra, page } => match self.answer.take() {
                    Some(Choice::Page(next)) => {
                        self.agenda.push(Task::Probe(Box::new(Probe::Paged { p, kind, ranges, extra, page: next })))
                    }
                    Some(c) => self.probe_log(format!("{c:?}")),
                    None => {
                        let items: Vec<Vec<(u32, Choice)>> =
                            ranges.iter().enumerate().map(|(r, (_, _, ps))| ps.iter().map(|&pos| (pos, item(r, pos))).collect()).collect();
                        let rs: Vec<PagedRange> = ranges.iter().zip(&items).map(|(&(base, n, _), items)| PagedRange { base, n, items }).collect();
                        let task = Task::Probe(Box::new(Probe::Paged { p, kind, ranges: ranges.clone(), extra: extra.clone(), page }));
                        self.ask_paged_ranges(task, page, p, kind, &rs, extra);
                    }
                },
                Probe::Number { p, kind, lo, hi, step } => match self.answer.take() {
                    Some(Choice::Number(k)) => match step.answer(lo, hi, k) {
                        NumberAnswer::Next(s) => self.agenda.push(Task::Probe(Box::new(Probe::Number { p, kind, lo, hi, step: s }))),
                        NumberAnswer::Value(v) => self.probe_log(format!("value {v}")),
                    },
                    Some(c) => unreachable!("number answer {c:?}"),
                    None => self.ask_number(Task::Probe(Box::new(Probe::Number { p, kind, lo, hi, step })), step, p, kind, lo, hi),
                },
            }
        }

        fn probe_log(&mut self, s: String) {
            self.log.as_mut().expect("a test game logs").push(format!("probe: {s}"));
        }
    }

    /// P0's first main phase with cards hidden in both libraries and hands
    /// (`determinize` deals them); a single option is asked unless `auto`.
    fn game(auto: bool) -> Game {
        Scenario::new(CardDb::cube())
            .spec(|s| s.auto_single = auto)
            .library(0, &["Island", "Mountain", "Lightning Bolt", "Counterspell", "Island"])
            .library(1, &["Mountain", "Island", "Llanowar Elves", "Brainstorm", "Mountain"])
            .hand(0, &["Brainstorm", "Mountain"])
            .hand(1, &["Lightning Bolt", "Counterspell", "Island"])
            .at(Step::Main1, 0)
            .start()
    }

    /// `g` with `probe` asked in place of P0's priority (which is asked again
    /// once the probe has its answer).
    fn start(mut g: Game, probe: Probe) -> Game {
        assert_eq!(g.kind(), Some(DecisionKind::Priority));
        g.pending = None;
        g.agenda.push(Task::Probe(Box::new(probe)));
        g.advance();
        g.keys.clear();
        g
    }

    fn paged(kind: DecisionKind, ranges: &[(u16, u16, Vec<u32>)], extra: &[Opt]) -> Probe {
        Probe::Paged { p: 0, kind, ranges: ranges.to_vec(), extra: extra.to_vec(), page: Page::FIRST }
    }

    fn number(lo: u64, hi: u64) -> Probe {
        Probe::Number { p: 0, kind: DecisionKind::ChooseMode, lo, hi, step: NumberStep::FIRST }
    }

    fn last_probe(g: &Game) -> String {
        g.log.as_ref().unwrap().iter().rev().find_map(|l| l.strip_prefix("probe: ")).expect("the probe has answered").to_string()
    }

    fn actions(g: &Game) -> Vec<u16> {
        g.pending.as_ref().expect("a decision").options.iter().map(|o| o.action).collect()
    }

    fn pick(g: &mut Game, action: u16) {
        g.apply(action, &mut Vec::new()).unwrap_or_else(|e| panic!("{e}: {:?}", actions(g)));
    }

    /// Every sequence of decisions from `g` until the probe has its answer:
    /// (the answer, the actions taken). Each decision is the probe's, for P0.
    fn sequences(g: &Game) -> Vec<(String, Vec<u16>)> {
        fn walk(g: &Game, path: &mut Vec<u16>, out: &mut Vec<(String, Vec<u16>)>) {
            if g.kind() == Some(DecisionKind::Priority) {
                out.push((last_probe(g), path.clone()));
                return;
            }
            let d = g.pending.as_ref().expect("a decision");
            assert!(matches!(g.agenda.last(), Some(Task::Probe(_))) && d.player == 0, "{d:?}");
            for o in &d.options {
                let mut c = g.clone();
                pick(&mut c, o.action);
                path.push(o.action);
                walk(&c, path, out);
                path.pop();
            }
        }
        let mut out = Vec::new();
        walk(g, &mut Vec::new(), &mut out);
        out
    }

    /// The answers a paged probe can give: each item and each extra option,
    /// once.
    fn every_option(ranges: &[(u16, u16, Vec<u32>)], extra: &[Opt]) -> Vec<String> {
        let mut v: Vec<String> = extra.iter().map(|o| format!("{:?}", o.choice)).collect();
        for (r, (_, _, ps)) in ranges.iter().enumerate() {
            v.extend(ps.iter().map(|&pos| format!("{:?}", item(r, pos))));
        }
        v.sort();
        v
    }

    fn reached(seqs: &[(String, Vec<u16>)]) -> Vec<String> {
        let mut v: Vec<String> = seqs.iter().map(|(a, _)| a.clone()).collect();
        v.sort();
        v
    }

    fn pass() -> Opt {
        Opt { action: act::PASS, choice: Choice::Pass }
    }

    /// Every range of the action layout whose slots a choice may outnumber,
    /// with its width (ENGINE.md 11, 58.4).
    const RANGES: [(u16, u16); 7] = [
        (act::NUMBER, act::NUMBER_N),
        (act::MY_PERM, act::PERM_N),
        (act::STACK, act::STACK_N),
        (act::TRIGGER, act::TRIGGER_N),
        (act::LIBRARY, act::LIBRARY_N),
        (act::EXILE, act::EXILE_N),
        (act::GY_ABILITY, act::GY_ABILITY_N),
    ];

    /// CR 601.2c (any legal target; the same for every choice): a range whose
    /// items all fit its ids is asked in one decision, each item at `base +
    /// position` with the other options, exactly the decision `ask` makes,
    /// and the context floats stay 0 (`Stage::One`): no behaviour or digest
    /// changes where nothing pages.
    #[test]
    fn cr_601_2c_a_choice_that_fits_its_range_is_asked_as_before() {
        for (base, n) in RANGES {
            let n32 = n as u32;
            for positions in [vec![0], (0..n32).collect(), vec![1, 3, n32 - 1], (0..n32).step_by(2).collect::<Vec<_>>()] {
                for extra in [vec![], vec![pass()]] {
                    let ranges = vec![(base, n, positions.clone())];
                    let g = start(game(false), paged(DecisionKind::Target, &ranges, &extra));
                    let d = g.pending.as_ref().unwrap();
                    let mut want: Vec<Opt> = extra.clone();
                    want.extend(positions.iter().map(|&p| Opt { action: base + p as u16, choice: item(0, p) }));
                    assert_eq!(d.options, want, "range {base}+{n}");
                    assert_eq!(d.stage, Stage::One);
                    let mut plain = game(false);
                    plain.pending = None;
                    plain.ask(Task::PriorityAsk { page: Page::FIRST }, 0, DecisionKind::Target, want.clone());
                    assert_eq!(plain.pending.as_ref(), Some(d), "the same decision as ask's");
                    let seqs = sequences(&g);
                    assert!(seqs.iter().all(|(_, path)| path.len() == 1), "one decision each");
                    assert_eq!(reached(&seqs), every_option(&ranges, &extra));
                }
            }
        }
    }

    /// CR 601.2c: at the boundaries (n - 1, n, n + 1 items, two and three
    /// pages' worth, one more and one less) every item and every other
    /// option is reached by exactly one sequence of decisions, in every
    /// range; a page offers at most `n - 1` items and the last id only as
    /// "next page".
    #[test]
    fn cr_601_2c_every_item_is_reached_exactly_once_at_the_page_boundaries() {
        for (base, n) in RANGES {
            let w = n as u32 - 1;
            for count in [n as u32 - 1, n as u32, n as u32 + 1, 2 * w, 2 * w + 1, 2 * w + 2, 3 * w, 3 * w + 1] {
                for extra in [vec![], vec![pass(), Opt { action: act::YES, choice: Choice::Yes }]] {
                    if extra.iter().any(|o| (base..base + n).contains(&o.action)) {
                        continue;
                    }
                    let ranges = vec![(base, n, (0..count).collect::<Vec<u32>>())];
                    let g = start(game(false), paged(DecisionKind::ChooseMode, &ranges, &extra));
                    let seqs = sequences(&g);
                    assert_eq!(reached(&seqs), every_option(&ranges, &extra), "range {base}+{n}, {count} items");
                    let pages = if count <= n as u32 { 1 } else { count.div_ceil(w) };
                    for (answer, path) in &seqs {
                        assert!(path.len() as u32 <= pages, "{answer}: {path:?}");
                        // Each "next page" is the range's last id.
                        assert!(path[..path.len() - 1].iter().all(|&a| a == base + n - 1), "{answer}: {path:?}");
                    }
                    if count > n as u32 {
                        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Paged { window: 0 });
                    }
                }
            }
        }
    }

    /// CR 601.2c: pages of `n - 1`: the first page holds items 0..=n-2 at
    /// `base + 0 ..= base + n - 2` and "next page" at `base + n - 1`; page w
    /// holds items w (n - 1) .. at `base + 0 ..`, the last page no "next".
    /// The page lives in the task.
    #[test]
    fn cr_601_2c_pages_of_n_minus_1_items_then_next_page() {
        let (base, n) = (act::STACK, act::STACK_N);
        let ranges = vec![(base, n, (0..70).collect::<Vec<u32>>())];
        let mut g = start(game(false), paged(DecisionKind::Target, &ranges, &[pass()]));
        let d = g.pending.clone().unwrap();
        let mut want = vec![pass()];
        want.extend((0..31).map(|p| Opt { action: base + p as u16, choice: item(0, p) }));
        want.push(Opt { action: base + 31, choice: Choice::Page(Page { range: 0, window: 1 }) });
        assert_eq!(d.options, want);
        pick(&mut g, base + 31);
        let d = g.pending.clone().unwrap();
        let mut want: Vec<Opt> = (31..62).map(|p| Opt { action: base + p as u16 - 31, choice: item(0, p) }).collect();
        want.push(Opt { action: base + 31, choice: Choice::Page(Page { range: 0, window: 2 }) });
        assert_eq!(d.options, want, "page 1: items 31..=61, no PASS");
        assert_eq!(d.stage, Stage::Paged { window: 1 });
        assert!(matches!(g.agenda.last(), Some(Task::Probe(p)) if matches!(**p, Probe::Paged { page: Page { range: 0, window: 1 }, .. })));
        pick(&mut g, base + 31);
        let d = g.pending.clone().unwrap();
        let want: Vec<Opt> = (62..70).map(|p| Opt { action: base + p as u16 - 62, choice: item(0, p) }).collect();
        assert_eq!(d.options, want, "the last page: items 62..=69, no next");
        pick(&mut g, base + 7);
        assert_eq!(last_probe(&g), format!("{:?}", item(0, 69)));
        assert_eq!(g.kind(), Some(DecisionKind::Priority));
    }

    /// CR 601.2c: positions that skip (the stack's spells among its
    /// abilities, the permanents a filter matches) keep `base + position`
    /// on the first page; a window with no item is skipped, so no page
    /// offers only "next page"; a first window with none offers "next page"
    /// with the other options.
    #[test]
    fn cr_601_2c_windows_without_an_item_are_skipped() {
        let (base, n) = (act::STACK, act::STACK_N);
        let ranges = vec![(base, n, vec![3, 200, 201, 400])];
        let mut g = start(game(false), paged(DecisionKind::Target, &ranges, &[]));
        assert_eq!(
            g.pending.as_ref().unwrap().options,
            vec![Opt { action: base + 3, choice: item(0, 3) }, Opt { action: base + 31, choice: Choice::Page(Page { range: 0, window: 6 }) }]
        );
        pick(&mut g, base + 31);
        assert_eq!(
            g.pending.as_ref().unwrap().options,
            vec![
                Opt { action: base + 14, choice: item(0, 200) },
                Opt { action: base + 15, choice: item(0, 201) },
                Opt { action: base + 31, choice: Choice::Page(Page { range: 0, window: 12 }) },
            ]
        );
        pick(&mut g, base + 31);
        assert_eq!(g.pending.as_ref().unwrap().options, vec![Opt { action: base + 28, choice: item(0, 400) }]);
        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Paged { window: 12 });
        // Only items past the first window, with another option.
        let ranges = vec![(base, n, vec![40, 41])];
        let g = start(game(false), paged(DecisionKind::Target, &ranges, &[pass()]));
        assert_eq!(g.pending.as_ref().unwrap().options, vec![pass(), Opt { action: base + 31, choice: Choice::Page(Page { range: 0, window: 1 }) }]);
        assert_eq!(reached(&sequences(&g)), every_option(&ranges, &[pass()]));
        // Sparse positions in every range: exactly once.
        for (base, n) in RANGES {
            for positions in [vec![0, 5 * n as u32], vec![n as u32 - 1, n as u32], (0..400).filter(|p| p % 7 == 3 || p % 13 == 0).collect()] {
                let ranges = vec![(base, n, positions)];
                let g = start(game(false), paged(DecisionKind::ChooseMode, &ranges, &[pass()]));
                assert_eq!(reached(&sequences(&g)), every_option(&ranges, &[pass()]), "range {base}+{n}");
            }
        }
    }

    /// CR 601.2c with several ranges at once (a target that is a permanent of
    /// either player or a spell, "or a player"): each paged range has its own
    /// pages, entered from the first page, which holds the ranges that fit
    /// and the other options; every option exactly once.
    #[test]
    fn cr_601_2c_several_paged_ranges_and_other_options_in_one_decision() {
        let ranges = vec![
            (act::MY_PERM, act::PERM_N, (0..100).collect::<Vec<u32>>()),
            (act::OPP_PERM, act::PERM_N, (0..197).filter(|p| p % 2 == 0).collect()),
            (act::STACK, act::STACK_N, (1..41).collect()),
            (act::EXILE, act::EXILE_N, (0..32).collect()),
        ];
        let extra = vec![
            pass(),
            Opt { action: act::PLAYER, choice: Choice::Target(Target::Player(0)) },
            Opt { action: act::PLAYER + 1, choice: Choice::Target(Target::Player(1)) },
        ];
        let g = start(game(false), paged(DecisionKind::ChooseMode, &ranges, &extra));
        let d = g.pending.as_ref().unwrap();
        let nexts: Vec<u16> = d.options.iter().filter(|o| matches!(o.choice, Choice::Page(_))).map(|o| o.action).collect();
        assert_eq!(nexts, vec![act::MY_PERM + 95, act::OPP_PERM + 95, act::STACK + 31], "the exile range fits: no page");
        assert_eq!(d.options.len(), 3 + 95 + 3 + 48 + 30 + 32);
        let seqs = sequences(&g);
        assert_eq!(reached(&seqs), every_option(&ranges, &extra));
        // A range page offers that range's items alone.
        for (answer, path) in &seqs {
            if path.len() > 1 {
                let &(base, n, _) = ranges.iter().find(|&&(b, n, _)| path[0] == b + n - 1).unwrap();
                assert!(path[1..].iter().all(|&a| (base..base + n).contains(&a)), "{answer}: {path:?}");
            }
        }
    }

    /// ENGINE.md 10, 99.8 (fuzz seed 624's lesson): the page is the task's,
    /// so a refresh (re-running the asker) and `determinize` for either
    /// player ask the same page again, and the same answer chooses the same
    /// item.
    #[test]
    fn paged_choice_is_asked_again_identically_after_determinize() {
        let ranges = vec![(act::MY_PERM, act::PERM_N, (0..250).collect::<Vec<u32>>()), (act::STACK, act::STACK_N, (0..5).collect())];
        let mut g = start(game(false), paged(DecisionKind::ChoosePermanent, &ranges, &[pass()]));
        pick(&mut g, act::MY_PERM + 95);
        pick(&mut g, act::MY_PERM + 95);
        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Paged { window: 2 });
        let mut rng = Rng::new(7);
        for viewer in [0, 1] {
            for k in 0..4 {
                let mut c = g.clone();
                if k == 0 {
                    c.refresh_pending();
                } else {
                    c.determinize(viewer, &mut rng);
                }
                assert_eq!(c.pending, g.pending, "viewer {viewer}, world {k}");
                assert_eq!(c.agenda.last(), g.agenda.last());
                pick(&mut c, act::MY_PERM + 10);
                assert_eq!(last_probe(&c), format!("{:?}", item(0, 200)));
            }
        }
    }

    /// A range page that holds no item (another player's decision refreshed
    /// in a world where its items differ) shows the first page; so does a
    /// page of a range that fits.
    #[test]
    fn a_stale_range_page_shows_the_first_page() {
        let ranges = vec![(act::STACK, act::STACK_N, (0..40).collect::<Vec<u32>>())];
        let first = start(game(false), paged(DecisionKind::Target, &ranges, &[pass()])).pending;
        for page in [Page { range: 0, window: 9 }, Page { range: 3, window: 1 }] {
            let probe = Probe::Paged { p: 0, kind: DecisionKind::Target, ranges: ranges.clone(), extra: vec![pass()], page };
            assert_eq!(start(game(false), probe).pending, first, "{page:?}");
        }
        let fits = vec![(act::STACK, act::STACK_N, (0..20).collect::<Vec<u32>>())];
        let probe = Probe::Paged { p: 0, kind: DecisionKind::Target, ranges: fits.clone(), extra: vec![], page: Page { range: 0, window: 1 } };
        assert_eq!(start(game(false), probe).pending, start(game(false), paged(DecisionKind::Target, &fits, &[])).pending);
    }

    /// CR 107.3 (X), 601.2b: a number whose range fits `NUMBER` (`hi - lo`
    /// <= 31) is one decision, `NUMBER + k` = `lo + k`, as before.
    #[test]
    fn cr_107_3_a_number_that_fits_number_is_one_decision() {
        for (lo, hi) in [(0, 0), (0, 1), (0, 30), (0, 31), (5, 36), (u32::MAX as u64 - 31, u32::MAX as u64), (u32::MAX as u64, u32::MAX as u64), (u64::MAX - 31, u64::MAX)] {
            let g = start(game(false), number(lo, hi));
            let d = g.pending.as_ref().unwrap();
            assert_eq!(actions(&g), (0..=(hi - lo) as u16).map(|k| act::NUMBER + k).collect::<Vec<_>>(), "{lo}..={hi}");
            assert_eq!(d.stage, Stage::One);
            let seqs = sequences(&g);
            assert!(seqs.iter().all(|(_, p)| p.len() == 1));
            for (answer, path) in &seqs {
                assert_eq!(*answer, format!("value {}", lo + (path[0] - act::NUMBER) as u64));
            }
        }
    }

    /// The digits a range of rests `0..=d` needs: the least `L` with `d <
    /// 32^L` (independently of `NumberStep::digits`).
    fn digits_of(d: u64) -> u32 {
        (1..=13).find(|&l| (d as u128) < 32u128.pow(l)).unwrap()
    }

    /// CR 107.3, 601.2b: every value of a range wider than `NUMBER` is
    /// reached by exactly one sequence of decisions: `lo + k` (k <= 30) in
    /// one, the others in `NUMBER + 31` and then exactly as many digits as
    /// the range's largest rest needs; at the digit boundaries (rests up to
    /// 31, 32, 1023, 1024, 32767, 32768) and at `u32::MAX` and `u64::MAX`.
    #[test]
    fn cr_107_3_every_value_of_a_wide_range_is_reached_exactly_once() {
        let spans: [u64; 12] = [32, 33, 62, 63, 64, 95, 1054, 1055, 1056, 1100, 32798, 32799];
        for &span in &spans {
            let los: Vec<u64> =
                if span > 2000 { vec![0] } else { vec![0, 3, u32::MAX as u64 - span, u64::MAX - span] };
            for lo in los {
                let hi = lo + span;
                let g = start(game(false), number(lo, hi));
                assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Paged { window: 0 }, "{lo}..={hi}");
                let seqs = sequences(&g);
                let mut values: Vec<u64> = seqs.iter().map(|(a, _)| a.strip_prefix("value ").unwrap().parse().unwrap()).collect();
                values.sort();
                assert!(values.iter().copied().eq(lo..=hi), "{lo}..={hi}: each value once");
                let l = digits_of(span - 31) as usize;
                for (answer, path) in &seqs {
                    let v: u64 = answer.strip_prefix("value ").unwrap().parse().unwrap();
                    assert_eq!(path.len(), if v - lo <= 30 { 1 } else { 1 + l }, "{lo}..={hi}: {v} by {path:?}");
                }
            }
        }
    }

    /// CR 107.3, 601.2b: on the way to any value of a range (`u32` and `u64`
    /// extremes among them), the first decision of a wide range offers
    /// `NUMBER + 0 ..= 31` and each digit decision exactly the digits with a
    /// legal completion (computed here independently, in `u128`), the
    /// completions of the offered digits partition the decision's (so no
    /// value is reached twice or missed), and a `u32` range needs at most 7
    /// digits.
    #[test]
    fn cr_107_3_each_digit_offers_exactly_the_digits_with_a_legal_completion() {
        let m32 = u32::MAX as u64;
        let ranges: [(u64, u64); 9] = [
            (0, m32),
            (1, m32),
            (m32 - 40, m32),
            (0, u64::MAX),
            (u64::MAX - 100, u64::MAX),
            (12_345, 12_345 + 31 + 32u64.pow(4)),
            (12_345, 12_345 + 31 + 32u64.pow(4) - 1),
            (7, 7 + 31 + 32u64.pow(6) * 3),
            (0, m32 + 1),
        ];
        let mut rng = Rng::new(11);
        for (lo, hi) in ranges {
            let d = hi - lo - 31;
            let l = digits_of(d);
            if hi <= m32 + 1 {
                assert!(l <= 7, "{lo}..={hi}");
            }
            let mut targets = vec![lo, lo + 1, lo + 30, lo + 31, lo + 32, lo + 62, lo + 63, lo + (hi - lo) / 2, hi - 1, hi];
            targets.extend((0..20).map(|_| {
                let r = rng.next_u64();
                lo + (hi - lo).checked_add(1).map_or(r, |m| r % m)
            }));
            targets.retain(|&v| v <= hi);
            for v in targets {
                let mut g = start(game(false), number(lo, hi));
                assert_eq!(actions(&g), (0..32).map(|k| act::NUMBER + k).collect::<Vec<_>>());
                if v - lo <= 30 {
                    pick(&mut g, act::NUMBER + (v - lo) as u16);
                } else {
                    pick(&mut g, act::NUMBER + 31);
                    let rest = (v - lo - 31) as u128;
                    let (lo128, hi128) = (lo as u128, hi as u128);
                    let mut prefix: u128 = 0;
                    for i in 0..l {
                        let r = l - 1 - i; // digits after this one
                        let place = 32u128.pow(r);
                        // Digit q has a legal completion iff the least value
                        // with the prefix and q is at most hi.
                        let feasible: Vec<u128> = (0..32).filter(|&q| lo128 + 31 + (prefix * 32 + q) * place <= hi128).collect();
                        assert_eq!(actions(&g), feasible.iter().map(|&q| act::NUMBER + q as u16).collect::<Vec<_>>(), "{lo}..={hi} -> {v}, digit {i}");
                        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Digit { left: (r + 1) as u8, prefix: prefix as u64 });
                        // The completions under each offered digit partition
                        // those under the prefix.
                        let span = |p: u128, r: u32| -> u128 {
                            let a = lo128 + 31 + p * 32u128.pow(r);
                            let b = (lo128 + 31 + (p + 1) * 32u128.pow(r) - 1).min(hi128);
                            if a > hi128 { 0 } else { b - a + 1 }
                        };
                        assert_eq!(feasible.iter().map(|&q| span(prefix * 32 + q, r)).sum::<u128>(), span(prefix, r + 1));
                        let q = (rest / place) % 32;
                        pick(&mut g, act::NUMBER + q as u16);
                        prefix = prefix * 32 + q;
                    }
                }
                assert_eq!(last_probe(&g), format!("value {v}"), "{lo}..={hi}");
                assert_eq!(g.kind(), Some(DecisionKind::Priority));
            }
        }
    }

    /// CR 107.3: the `u32` extremes: 0 and `u32::MAX` in `0..=u32::MAX` (one
    /// decision; eight), a range ending at `u32::MAX` one wider than
    /// `NUMBER` (one digit of two), `lo = hi = u32::MAX` (one option).
    #[test]
    fn cr_107_3_the_u32_extremes() {
        let m32 = u32::MAX as u64;
        let mut g = start(game(false), number(0, m32));
        pick(&mut g, act::NUMBER);
        assert_eq!(last_probe(&g), "value 0");
        let mut g = start(game(false), number(0, m32));
        pick(&mut g, act::NUMBER + 31);
        let mut n = 1;
        while g.kind() != Some(DecisionKind::Priority) && n < 20 {
            let top = *actions(&g).last().unwrap();
            pick(&mut g, top);
            n += 1;
        }
        assert_eq!((last_probe(&g), n), (format!("value {m32}"), 8), "the largest digit each time");
        let g = start(game(false), number(m32 - 32, m32));
        let seqs = sequences(&g);
        assert_eq!(seqs.len(), 33);
        assert!(seqs.iter().any(|(a, p)| *a == format!("value {m32}") && *p == vec![act::NUMBER + 31, act::NUMBER + 1]));
        let g = start(game(false), number(m32, m32));
        assert_eq!(actions(&g), vec![act::NUMBER]);
        assert_eq!(sequences(&g), vec![(format!("value {m32}"), vec![act::NUMBER])]);
    }

    /// ENGINE.md 10, 99.8: the digits are the task's, so a refresh and
    /// `determinize` for either player ask the same digit again, and the same
    /// answers give the same value.
    #[test]
    fn number_digits_are_asked_again_identically_after_determinize() {
        let mut g = start(game(false), number(0, u32::MAX as u64));
        pick(&mut g, act::NUMBER + 31);
        pick(&mut g, act::NUMBER + 2);
        pick(&mut g, act::NUMBER + 5);
        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Digit { left: 5, prefix: 2 * 32 + 5 });
        let mut rng = Rng::new(3);
        for viewer in [0, 1] {
            for k in 0..4 {
                let mut c = g.clone();
                if k == 0 {
                    c.refresh_pending();
                } else {
                    c.determinize(viewer, &mut rng);
                }
                assert_eq!(c.pending, g.pending, "viewer {viewer}, world {k}");
                assert_eq!(c.agenda.last(), g.agenda.last());
                for q in [1, 0, 0, 9, 31] {
                    pick(&mut c, act::NUMBER + q);
                }
                let rest = ((((((2u64 * 32 + 5) * 32 + 1) * 32) * 32) * 32 + 9) * 32) + 31;
                assert_eq!(last_probe(&c), format!("value {}", 31 + rest));
            }
        }
    }

    /// Digits with no legal completion left (another player's decision
    /// refreshed in a world where the range differs) restart at the first
    /// decision; feasible ones go on.
    #[test]
    fn a_stale_number_step_restarts_at_the_first_decision() {
        let first = start(game(false), number(0, 100)).pending;
        for step in [NumberStep { left: 9, prefix: 0 }, NumberStep { left: 2, prefix: 5 }, NumberStep { left: 1, prefix: 3 }] {
            let probe = Probe::Number { p: 0, kind: DecisionKind::ChooseMode, lo: 0, hi: 100, step };
            assert_eq!(start(game(false), probe).pending, first, "{step:?}");
        }
        let probe = Probe::Number { p: 0, kind: DecisionKind::ChooseMode, lo: 0, hi: 100, step: NumberStep { left: 1, prefix: 2 } };
        let g = start(game(false), probe);
        assert_eq!(actions(&g), (0..=5).map(|k| act::NUMBER + k).collect::<Vec<_>>(), "rests 64..=69");
        let probe = Probe::Number { p: 0, kind: DecisionKind::ChooseMode, lo: 0, hi: 20, step: NumberStep { left: 1, prefix: 0 } };
        assert_eq!(start(game(false), probe).pending, start(game(false), number(0, 20)).pending, "the range fits now");
    }

    /// With single options answered by the engine (`auto_single`, the
    /// search's setting): a forced digit is answered and keyed (`KEY_AUTO`,
    /// ENGINE.md 10), a last page of one item too, and every value and item
    /// is still reached exactly once.
    #[test]
    fn forced_digits_and_one_item_pages_are_answered_by_the_engine() {
        let mut g = start(game(true), number(0, 63));
        pick(&mut g, act::NUMBER + 31);
        let mut keys = Vec::new();
        g.apply(act::NUMBER + 1, &mut keys).unwrap();
        assert_eq!(last_probe(&g), "value 63", "digit 1 then the only digit, 0");
        assert!(keys.iter().any(|&k| crate::hidden::key_kind(k) == crate::hidden::KEY_AUTO), "{keys:?}");
        let g = start(game(true), number(0, 63));
        let mut values: Vec<u64> = sequences(&g).iter().map(|(a, _)| a.strip_prefix("value ").unwrap().parse().unwrap()).collect();
        values.sort();
        assert!(values.iter().copied().eq(0..=63), "{values:?}");
        let ranges = vec![(act::STACK, act::STACK_N, (0..63).collect::<Vec<u32>>())];
        let g = start(game(true), paged(DecisionKind::Target, &ranges, &[]));
        assert_eq!(reached(&sequences(&g)), every_option(&ranges, &[]));
    }

    fn context(g: &Game, viewer: PlayerId) -> Vec<f32> {
        let mut x = vec![0.0; NUM_FEATURES];
        g.encode_features(viewer, &mut x);
        x[O_CONTEXT + ctx::PAGED..O_CONTEXT + 256].to_vec()
    }

    /// The context floats (layout v5's spare 246-250, ENGINE.md 100.1):
    /// for the decider only, 246 = 1 on a page of a paged choice and on a
    /// big number's first decision, 247 = the window / 4, 249 = the digits
    /// left / 8, 250 = the digits chosen / 32 (squashed); everything 0 for a
    /// choice made in one decision, for the other player, and in 248 and
    /// 251-255 always.
    #[test]
    fn paged_and_digit_context_floats_are_the_deciders_only() {
        let zero = vec![0.0; 256 - ctx::PAGED];
        let with = |vals: &[(usize, f32)]| {
            let mut v = zero.clone();
            for &(i, x) in vals {
                v[i - ctx::PAGED] = x;
            }
            v
        };
        let check = |g: &Game, want: Vec<f32>, what: &str| {
            assert_eq!(context(g, 0), want, "{what}: the decider");
            assert_eq!(context(g, 1), zero, "{what}: the other player");
        };
        let fits = vec![(act::STACK, act::STACK_N, (0..32).collect::<Vec<u32>>())];
        check(&start(game(false), paged(DecisionKind::Target, &fits, &[])), zero.clone(), "a range that fits");
        check(&start(game(false), number(0, 31)), zero.clone(), "a number that fits");
        let ranges = vec![(act::STACK, act::STACK_N, vec![0, 200, 400])];
        let mut g = start(game(false), paged(DecisionKind::Target, &ranges, &[]));
        check(&g, with(&[(ctx::PAGED, 1.0)]), "the first page");
        pick(&mut g, act::STACK + 31);
        check(&g, with(&[(ctx::PAGED, 1.0), (ctx::PAGE, 1.5)]), "window 6");
        let mut g = start(game(false), number(0, u32::MAX as u64));
        check(&g, with(&[(ctx::PAGED, 1.0)]), "a wide number's first decision");
        pick(&mut g, act::NUMBER + 31);
        check(&g, with(&[(ctx::DIGIT, 7.0 / 8.0)]), "the first digit");
        pick(&mut g, act::NUMBER + 3);
        check(&g, with(&[(ctx::DIGIT, 6.0 / 8.0), (ctx::PREFIX, squash(3.0 / 32.0))]), "the second digit");
        for _ in 0..5 {
            pick(&mut g, act::NUMBER + 31);
        }
        check(&g, with(&[(ctx::DIGIT, 1.0 / 8.0), (ctx::PREFIX, squash(((3u64 << 25) | ((1 << 25) - 1)) as f32 / 32.0))]), "the last digit");
        pick(&mut g, act::NUMBER);
        check(&g, zero.clone(), "priority again");
    }

    /// Thirty-four Lightning Bolts in exile that P0 may play ("its owner may
    /// play it for as long as it stays exiled", `PermKind::Taxed(0)`): each
    /// is a priority option (CR 601.2, 117.1a: cast with its permission).
    /// The first page offers the first 31 at `EXILE` + 0..=30 and "next page"
    /// at `EXILE` + 31 with the other options; the range page the other 3 at
    /// `EXILE` + 0..=2 alone (ENGINE.md 100.2; before it, the first 32 only,
    /// docs/shortcuts.md 58.4). A refresh asks the same page.
    #[test]
    fn cr_601_2_every_exiled_card_a_player_may_cast_is_offered_at_priority() {
        let mut g = Scenario::new(CardDb::cube())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 5])
            .library(1, &["Island"; 5])
            .exile(0, &["Lightning Bolt"; 34])
            .pool(0, [0, 0, 0, 1, 0, 0], 0)
            .at(Step::Main1, 0)
            .start();
        let bolts = g.exile.clone();
        for &c in &bolts {
            let card = g.obj_ref(c);
            g.permissions.push(crate::game::Permission { card, player: 0, turn: None, kind: crate::game::PermKind::Taxed(0) });
        }
        g.refresh_pending();
        let exile_opts = |g: &Game| -> Vec<(u16, Choice)> {
            g.pending.as_ref().unwrap().options.iter().filter(|o| (act::EXILE..act::EXILE + act::EXILE_N).contains(&o.action)).map(|o| (o.action, o.choice)).collect()
        };
        let first = exile_opts(&g);
        assert_eq!(first.len(), 32);
        for (k, &(a, c)) in first[..31].iter().enumerate() {
            assert_eq!((a, c), (act::EXILE + k as u16, Choice::Cast(bolts[k])));
        }
        let Choice::Page(next) = first[31].1 else { panic!("{:?}", first[31]) };
        assert!(g.pending.as_ref().unwrap().options.iter().any(|o| o.choice == Choice::Pass), "the first page has the other options");
        pick(&mut g, act::EXILE + 31);
        assert_eq!(g.kind(), Some(DecisionKind::Priority));
        assert_eq!(g.pending.as_ref().unwrap().stage, Stage::Paged { window: next.window });
        let page: Vec<(u16, Choice)> = g.pending.as_ref().unwrap().options.iter().map(|o| (o.action, o.choice)).collect();
        assert_eq!(page, (31..34).map(|k| (act::EXILE + k as u16 - 31, Choice::Cast(bolts[k]))).collect::<Vec<_>>());
        let mut r = g.clone();
        r.refresh_pending();
        assert_eq!(r.pending, g.pending, "a refresh asks the same page");
        pick(&mut g, act::EXILE + 2);
        assert_eq!(g.objs[bolts[33].i()].zone, crate::ids::Zone::Stack, "the 34th is cast");
    }

    /// A permanent entering with an additional cost paid 300 times and an X
    /// of 70 (CR 702.156a: ravenous reads the X; "if it was kicked" the
    /// times): its `Enters` note can't hold them (bits 1-8 and 9-14), so it
    /// carries `ENTER_BIG` and the permanent a `Mark::Entered` with both,
    /// exactly (ENGINE.md 100.2; before it the X was cut at 63, the audit's
    /// N9). Small counts keep the note's bits.
    #[test]
    fn cr_702_156a_an_entering_x_past_63_and_times_past_255_are_kept_exactly() {
        let mut g = Scenario::new(CardDb::cube()).hand(0, &["Llanowar Elves"]).at(Step::Main1, 0).start();
        let c = g.players[0].hand[0];
        g.entering_x = 70;
        g.entering_times = 300;
        g.put_onto_battlefield(c, 0);
        let r = g.obj_ref(c);
        assert!(g.marks.contains(&(r, crate::game::Mark::Entered { times: 300, x: 70 })));
        assert_eq!(g.entered_counts(crate::game::ENTER_BIG, Some(r)), (300, 70));
        assert_eq!(g.entered_counts((5 << 1) | (9 << 9), None), (5, 9));
    }
}
