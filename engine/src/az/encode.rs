//! The network input of the `mtg` game (ENGINE.md 21.2, 22.2, 24): a fixed-width
//! f32 vector for a viewer, with no hidden information. Layout v3 (M7b: 26,500
//! floats), in order:
//!
//! | Block | Floats | What |
//! |---|---:|---|
//! | globals | 128 | turn, step, who is active / has priority, per player (viewer first) life, zone sizes, mana pool, board summary, lost life this turn, sorceries as though they had flash |
//! | context | 256 | the pending decision: kind (64 kinds), decider, the asking task, its card (card code) and ability, the resolving op, the cost left to pay, damage left, counters of the asking task |
//! | bags | 15 x 1024 | per identity counts (0.25 per card): own hand, both battlefields, graveyards, exiles, own library, the opponent's hidden pool, the opponent's hand known exactly and by clouds, own library cards known, the opponent's library known exactly or by clouds, own and the opponent's emblems (by the identity of the card that made them) |
//! | tops | 2 x 3 x 54 | the top three library positions of each player: present, known to the viewer, the card code |
//! | permanents | 2 x 32 x 100 | `MY_PERM` / `OPP_PERM` slots 0..31: card code + status (tapped, damage, P/T, sickness, combat and what it attacks, keywords, loyalty, loyalty ability available, decision flags, options) |
//! | hand | 16 x 60 | `HAND` slots 0..15 (the viewer's hand sorted by definition): card code, count, options, known to the opponent |
//! | stack | 8 x 76 | `STACK` slots 0..7 (top first): source card code, kind, controller, targets |
//! | triggers | 4 x 56 | `TRIGGER` slots 0..3 (the viewer's waiting triggers) |
//! | list | 40 x 56 | `LIBRARY` slots 0..39 of the viewer's own decision: the searched library, the looked-at pile or the seen hand, sorted by definition |
//!
//! A card code (52 floats) is the card's stable identity (`identity.rs`: its
//! position in the cube's IR card list, tokens from a registry; capacity
//! 1024) as three one-hots (index / 64, index / 8 % 8, index % 8: 16 + 8 + 8)
//! plus a descriptor (card types, colours, mana value, printed power and
//! toughness, token, legendary; M7b: planeswalker, printed loyalty, emblem).
//! Layout v1 (M5a, 13,532 floats) used the
//! card table's index (capacity 256), which renumbers as support grows;
//! v2 (M5b, 23,324 floats) had a 192-float context block with room for 32
//! decision kinds.
//! "Me" is the viewer. Options of the pending decision are encoded only
//! when the viewer decides (its own legal actions); the list block too.
//! Everything else is public or the viewer's own knowledge, so the vector is
//! the same in every world `determinize(viewer)` can produce (tested).

use crate::db::{types, AbilitySlot, CardDb, Kw};
use crate::decision::act;
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::program::Op;

use super::identity;

/// Identity width: the capacity of the stable card identities (`identity.rs`).
pub const D: usize = identity::D;
/// Card code: the identity's three one-hot digits, then the descriptor.
pub const CODE: usize = 52;
pub const GLOBALS: usize = 128;
pub const CONTEXT: usize = 256;
pub const BAGS: usize = 15;
pub const TOPS: usize = 3;
pub const TOP_W: usize = CODE + 2;
pub const PERMS: usize = 32;
pub const PERM_W: usize = CODE + 48;
pub const HANDS: usize = 16;
pub const HAND_W: usize = CODE + 8;
pub const STACKS: usize = 8;
pub const STACK_W: usize = CODE + 24;
pub const TRIGS: usize = 4;
pub const TRIG_W: usize = CODE + 4;
pub const LIST: usize = 40;
pub const LIST_W: usize = CODE + 4;

pub const O_GLOBALS: usize = 0;
pub const O_CONTEXT: usize = O_GLOBALS + GLOBALS;
pub const O_BAGS: usize = O_CONTEXT + CONTEXT;
pub const O_TOPS: usize = O_BAGS + BAGS * D;
pub const O_MY_PERM: usize = O_TOPS + 2 * TOPS * TOP_W;
pub const O_OPP_PERM: usize = O_MY_PERM + PERMS * PERM_W;
pub const O_HAND: usize = O_OPP_PERM + PERMS * PERM_W;
pub const O_STACK: usize = O_HAND + HANDS * HAND_W;
pub const O_TRIG: usize = O_STACK + STACKS * STACK_W;
pub const O_LIST: usize = O_TRIG + TRIGS * TRIG_W;
pub const NUM_FEATURES: usize = O_LIST + LIST * LIST_W;

/// Bags (each `D` wide, 0.25 per card).
pub mod bag {
    pub const MY_HAND: usize = 0;
    pub const MY_BATTLEFIELD: usize = 1;
    pub const OPP_BATTLEFIELD: usize = 2;
    pub const MY_GRAVEYARD: usize = 3;
    pub const OPP_GRAVEYARD: usize = 4;
    pub const MY_EXILE: usize = 5;
    pub const OPP_EXILE: usize = 6;
    pub const MY_LIBRARY: usize = 7;
    /// The opponent's hand and library cards the viewer does not know
    /// exactly (open decklists: their identities as a multiset).
    pub const OPP_HIDDEN: usize = 8;
    pub const OPP_HAND_KNOWN: usize = 9;
    /// Identities the viewer's clouds place in the opponent's hand.
    pub const OPP_HAND_CLOUD: usize = 10;
    pub const MY_LIBRARY_KNOWN: usize = 11;
    /// The opponent's library cards known exactly, plus the identities the
    /// viewer's clouds place in it.
    pub const OPP_LIBRARY_KNOWN: usize = 12;
    /// Emblems (CR 114), by the identity of the card that made them.
    pub const MY_EMBLEMS: usize = 13;
    pub const OPP_EMBLEMS: usize = 14;
}

/// Offsets inside a permanent's status (after its card code).
pub mod perm {
    pub const PRESENT: usize = 0;
    pub const TAPPED: usize = 1;
    pub const DAMAGE: usize = 2;
    pub const POWER: usize = 3;
    pub const TOUGHNESS: usize = 4;
    pub const SICK: usize = 5;
    pub const ATTACKING: usize = 6;
    pub const BLOCKED: usize = 7;
    pub const BLOCKING: usize = 8;
    /// (slot + 1) / 32 of the attacker it blocks.
    pub const BLOCKING_SLOT: usize = 9;
    pub const STRUCK_FIRST: usize = 10;
    /// The potential blocker the pending `DeclareBlockers` decision is about.
    pub const ASKED_BLOCKER: usize = 11;
    /// The attacker / the blocker of the pending `AssignDamage` decision.
    pub const DAMAGE_ATTACKER: usize = 12;
    pub const DAMAGE_BLOCKER: usize = 13;
    /// Its slot id (`MY_PERM` / `OPP_PERM`) is an option of the viewer's decision.
    pub const OPTION: usize = 14;
    /// `ACTIVATE` options of abilities 0..3 (the viewer's permanents).
    pub const ACTIVATE: usize = 15;
    pub const TARGETED: usize = 19;
    pub const BLOCKERS: usize = 20;
    pub const KEYWORDS: usize = 21;
    pub const TOKEN: usize = 36;
    pub const NEW: usize = 37;
    pub const STOLEN: usize = 38;
    /// Loyalty counters / 8 (CR 306.5c).
    pub const LOYALTY: usize = 39;
    /// A planeswalker whose loyalty ability may still be activated this
    /// turn (CR 606.3).
    pub const LOYALTY_READY: usize = 40;
    /// An attacking creature attacking a planeswalker (not the player), and
    /// that planeswalker's slot (slot + 1) / 32 (CR 506.3, 508.1b).
    pub const ATTACKS_WALKER: usize = 41;
    pub const ATTACKS_SLOT: usize = 42;
    /// Creatures attacking this planeswalker / 4.
    pub const ATTACKED_BY: usize = 43;
    /// The attacker the pending `AttackTarget` decision is about.
    pub const ASKED_ATTACKER: usize = 44;
}

/// A card code: the identity's digits (16, 8, 8), then types, colours, size.
fn code(out: &mut [f32], db: &CardDb, ids: &[u16], def: DefId) {
    let i = ids[def as usize] as usize;
    out[(i >> 6) & 15] = 1.0;
    out[16 + ((i >> 3) & 7)] = 1.0;
    out[24 + (i & 7)] = 1.0;
    let d = db.def(def);
    for (k, t) in [types::ARTIFACT, types::CREATURE, types::ENCHANTMENT, types::INSTANT, types::LAND, types::SORCERY]
        .iter()
        .enumerate()
    {
        if d.types & t != 0 {
            out[32 + k] = 1.0;
        }
    }
    for c in 0..5 {
        if d.colors & (1 << c) != 0 {
            out[38 + c] = 1.0;
        }
    }
    out[43] = d.mana_value() as f32 / 8.0;
    out[44] = d.power.unwrap_or(0) as f32 / 8.0;
    out[45] = d.toughness.unwrap_or(0) as f32 / 8.0;
    out[46] = d.token as u8 as f32;
    out[47] = (d.supertypes & crate::db::supertypes::LEGENDARY != 0) as u8 as f32;
    out[48] = d.is(types::PLANESWALKER) as u8 as f32;
    out[49] = d.loyalty.unwrap_or(0) as f32 / 8.0;
    out[50] = d.emblem as u8 as f32;
}

fn bag_add(out: &mut [f32], ids: &[u16], b: usize, def: DefId) {
    out[O_BAGS + b * D + ids[def as usize] as usize] += 0.25;
}

/// Kinds of the asking task (context block).
fn task_code(t: &Task) -> usize {
    match t {
        Task::PriorityAsk => 0,
        Task::MulliganDeclare { .. } => 1,
        Task::MulliganBottom { .. } => 2,
        Task::DeclareAttackers { .. } => 3,
        Task::DeclareBlockers { .. } => 4,
        Task::AssignDamage { .. } => 5,
        Task::CleanupDiscard => 6,
        Task::ChooseTargets { .. } => 7,
        Task::PayMana(_) => 8,
        Task::Run(_) => 9,
        Task::PutTriggers => 10,
        Task::SbaCheck { .. } => 11,
        Task::PayCosts { .. } => 12,
        Task::CastOption { .. } => 13,
        Task::ChooseModes { .. } => 14,
        Task::ManaNowPick { .. } => 15,
        Task::AsEnters { .. } => 16,
        Task::CastPay { .. } | Task::ActPay { .. } => 17,
        Task::AttackTarget { .. } => 19,
        Task::ChooseFace { .. } => 20,
        _ => 18,
    }
}

/// The effect op a frame is at (context block); exhaustive, so a new op is
/// a compile error here.
fn op_code(op: &Op) -> usize {
    match op {
        Op::DealDamage { .. } => 0,
        Op::DamageAll { .. } => 1,
        Op::DamagePlayer { .. } => 2,
        Op::Draw { .. } => 3,
        Op::GainLife { .. } => 4,
        Op::AddMana { .. } => 5,
        Op::Counter { .. } => 6,
        Op::OfferPay { .. } => 7,
        Op::PutFromHandOnTop { .. } => 8,
        Op::DiscardRandom { .. } => 9,
        Op::Destroy { .. } => 10,
        Op::Exile { .. } => 11,
        Op::ExtraTurn { .. } => 12,
        Op::ReturnToHand { .. } => 13,
        Op::Untap { .. } => 14,
        Op::Discard { .. } => 15,
        Op::DiscardHand { .. } => 16,
        Op::LoseLife { .. } => 17,
        Op::Remember { .. } => 18,
        Op::DestroyAll { .. } => 19,
        Op::PutInLibrary { .. } => 20,
        Op::AddManaChoice { .. } => 21,
        Op::Jump { .. } => 22,
        Op::JumpUnless { .. } => 23,
        Op::JumpUnlessTarget { .. } => 24,
        Op::May { .. } => 25,
        Op::Search { .. } => 26,
        Op::LookTop { .. } => 27,
        Op::PileTo { .. } => 28,
        Op::Shuffle { .. } => 29,
        Op::SourceToLibraryTop => 30,
        Op::CreateToken { .. } => 31,
        Op::Sacrifice { .. } => 32,
        Op::ShuffleIntoLibrary { .. } => 33,
        Op::PutFromHand { .. } => 34,
        Op::RevealHand { .. } => 35,
        Op::ChooseDiscard { .. } => 36,
        Op::Ext(_) => 37,
    }
}

/// The context of the pending decision.
pub(crate) mod ctx {
    /// Decision kinds (one-hot; `DecisionKind as usize` must stay below `KINDS`).
    pub const KINDS: usize = 64;
    /// Asking-task codes (`task_code`).
    pub const TASKS: usize = 32;
    /// Effect-op codes (`op_code`).
    pub const OPS: usize = 64;
    pub const KIND: usize = 0;
    pub const DECIDER_IS_VIEWER: usize = KIND + KINDS;
    pub const PENDING: usize = DECIDER_IS_VIEWER + 1;
    pub const TASK: usize = PENDING + 1;
    pub const CARD: usize = TASK + TASKS; // a card code
    pub const HAS_CARD: usize = CARD + super::CODE;
    pub const SLOT_KIND: usize = HAS_CARD + 1; // 4
    pub const SLOT_INDEX: usize = SLOT_KIND + 4;
    pub const MODE: usize = SLOT_INDEX + 1;
    pub const OP: usize = MODE + 1;
    pub const PC: usize = OP + OPS;
    pub const OP_I: usize = PC + 1;
    pub const COST: usize = OP_I + 1; // 9
    pub const DAMAGE_LEFT: usize = COST + 9;
    pub const REQ: usize = DAMAGE_LEFT + 1;
    pub const NEXT_ATTACKER: usize = REQ + 1;
    pub const BLOCKER_INDEX: usize = NEXT_ATTACKER + 1;
    pub const BOTTOMS_LEFT: usize = BLOCKER_INDEX + 1;
    pub const PILE: usize = BOTTOMS_LEFT + 1;
    pub const OPTIONS: usize = PILE + 1;
    pub const MODES_CHOSEN: usize = OPTIONS + 1;
    pub const PICKED: usize = MODES_CHOSEN + 1;
    pub const END: usize = PICKED + 1;
}
const _: () = assert!(ctx::END <= CONTEXT);

impl Game {
    /// The network input for `viewer` (module docs); `out` has
    /// [`NUM_FEATURES`] floats, zeroed by the caller.
    pub fn encode_features(&self, viewer: PlayerId, out: &mut [f32]) {
        assert_eq!(out.len(), NUM_FEATURES);
        let ids = identity::identities(self.db);
        let me = viewer;
        let op_ = opp(me);
        let decision = self.pending.as_ref();
        let mine_to_decide = decision.is_some_and(|d| d.player == me);
        let legal = |a: u16| -> bool {
            mine_to_decide && decision.is_some_and(|d| d.options.binary_search_by_key(&a, |o| o.action).is_ok())
        };
        self.encode_globals(me, &mut out[O_GLOBALS..O_GLOBALS + GLOBALS]);
        self.encode_bags(me, ids, out);
        // Tops of the libraries.
        for (side, p) in [me, op_].into_iter().enumerate() {
            let lib = &self.players[p as usize].library;
            for k in 0..TOPS.min(lib.len()) {
                let c = lib[lib.len() - 1 - k];
                let o = O_TOPS + (side * TOPS + k) * TOP_W;
                out[o] = 1.0;
                if self.knows(me, c) {
                    out[o + 1] = 1.0;
                    code(&mut out[o + 2..o + 2 + CODE], self.db, ids, self.objs[c.i()].def);
                }
            }
        }
        // Permanents.
        let targeted: Vec<CardIdx> = self
            .stack
            .iter()
            .flat_map(|s| s.targets.iter())
            .filter_map(|&(_, t)| match t {
                Target::Obj(r) if self.live(r) => Some(r.idx),
                _ => None,
            })
            .collect();
        let (asked_blocker, dmg_att, dmg_blk) = self.combat_context();
        let asked_attacker = match (self.pending.is_some(), self.agenda.last()) {
            (true, Some(Task::AttackTarget { card })) => Some(*card),
            _ => None,
        };
        let mine: Vec<CardIdx> = self.permanents_of(me).collect();
        let theirs: Vec<CardIdx> = self.permanents_of(op_).collect();
        let slot_of = |c: CardIdx| -> Option<usize> {
            let ctl = self.objs[c.i()].controller;
            let list = if ctl == me { &mine } else { &theirs };
            list.iter().position(|&x| x == c)
        };
        for (side, list) in [&mine, &theirs].into_iter().enumerate() {
            let base = if side == 0 { O_MY_PERM } else { O_OPP_PERM };
            let range = if side == 0 { act::MY_PERM } else { act::OPP_PERM };
            for (s, &c) in list.iter().enumerate().take(PERMS) {
                let o = base + s * PERM_W;
                let ob = &self.objs[c.i()];
                code(&mut out[o..o + CODE], self.db, ids, ob.def);
                let st = &mut out[o + CODE..o + PERM_W];
                st[perm::PRESENT] = 1.0;
                st[perm::TAPPED] = ob.tapped as u8 as f32;
                st[perm::DAMAGE] = ob.damage as f32 / 8.0;
                st[perm::POWER] = self.power(c) as f32 / 8.0;
                st[perm::TOUGHNESS] = self.toughness(c) as f32 / 8.0;
                st[perm::SICK] = (self.is_creature(c) && self.is_sick(c) && !self.has_kw(c, Kw::Haste)) as u8 as f32;
                st[perm::ATTACKING] = ob.attacking as u8 as f32;
                st[perm::BLOCKED] = ob.blocked as u8 as f32;
                if let Some(a) = ob.blocking {
                    st[perm::BLOCKING] = 1.0;
                    st[perm::BLOCKING_SLOT] = slot_of(a).map_or(0.0, |x| (x + 1) as f32 / 32.0);
                }
                st[perm::STRUCK_FIRST] = ob.struck_first as u8 as f32;
                st[perm::ASKED_BLOCKER] = (asked_blocker == Some(c)) as u8 as f32;
                st[perm::DAMAGE_ATTACKER] = (dmg_att == Some(c)) as u8 as f32;
                st[perm::DAMAGE_BLOCKER] = (dmg_blk == Some(c)) as u8 as f32;
                st[perm::OPTION] = legal(range + s as u16) as u8 as f32;
                if side == 0 {
                    for i in 0..4u16 {
                        st[perm::ACTIVATE + i as usize] = legal(act::ACTIVATE + s as u16 * 4 + i) as u8 as f32;
                    }
                }
                st[perm::TARGETED] = targeted.contains(&c) as u8 as f32;
                st[perm::BLOCKERS] = self.battlefield.iter().filter(|&&x| self.objs[x.i()].blocking == Some(c)).count()
                    as f32
                    / 4.0;
                for (k, kw) in KWS.iter().enumerate() {
                    st[perm::KEYWORDS + k] = self.has_kw(c, *kw) as u8 as f32;
                }
                st[perm::TOKEN] = self.def_of(c).token as u8 as f32;
                st[perm::NEW] = (ob.control_since == self.turn.number) as u8 as f32;
                st[perm::STOLEN] = (ob.controller != ob.owner) as u8 as f32;
                let walker = self.is_type(c, types::PLANESWALKER);
                st[perm::LOYALTY] = self.counters_on(c, crate::db::Counter::Loyalty) as f32 / 8.0;
                st[perm::LOYALTY_READY] = (walker && self.loyalty_ready(c)) as u8 as f32;
                if let Some(Target::Obj(r)) = self.attack_dest(c) {
                    st[perm::ATTACKS_WALKER] = 1.0;
                    st[perm::ATTACKS_SLOT] = slot_of(r.idx).map_or(0.0, |x| (x + 1) as f32 / 32.0);
                }
                if walker {
                    let by = self
                        .battlefield
                        .iter()
                        .filter(|&&x| self.attack_dest(x) == Some(Target::Obj(self.obj_ref(c))))
                        .count();
                    st[perm::ATTACKED_BY] = by as f32 / 4.0;
                }
                st[perm::ASKED_ATTACKER] = (asked_attacker == Some(c)) as u8 as f32;
            }
        }
        // The viewer's hand.
        let hand = self.hand_sorted(me);
        let opp_knows = |c: CardIdx| self.objs[c.i()].known & (1 << op_) != 0;
        for (s, &c) in hand.iter().enumerate().take(HANDS) {
            let o = O_HAND + s * HAND_W;
            let def = self.objs[c.i()].def;
            code(&mut out[o..o + CODE], self.db, ids, def);
            let st = &mut out[o + CODE..o + HAND_W];
            st[0] = 1.0;
            st[1] = hand.iter().filter(|&&x| self.objs[x.i()].def == def).count() as f32 / 4.0;
            st[2] = legal(act::HAND + s as u16) as u8 as f32;
            st[3] = legal(act::HAND_ABILITY + s as u16) as u8 as f32;
            st[4] = opp_knows(c) as u8 as f32;
            st[5] = self.def_of(c).is(types::LAND) as u8 as f32;
        }
        // The stack, top first.
        for (s, item) in self.stack.iter().rev().enumerate().take(STACKS) {
            let o = O_STACK + s * STACK_W;
            let (def, kind) = match item.kind {
                StackKind::Spell { card } => (self.objs[card.i()].def, 1),
                StackKind::Ability { ability, .. } => {
                    (ability.def, if matches!(ability.slot, AbilitySlot::Trigger(_)) { 3 } else { 2 })
                }
            };
            code(&mut out[o..o + CODE], self.db, ids, def);
            let st = &mut out[o + CODE..o + STACK_W];
            st[0] = 1.0;
            st[kind] = 1.0;
            st[4] = (item.controller == me) as u8 as f32;
            st[5] = item.modes.len() as f32 / 4.0;
            st[6] = item.cost_option as f32 / 4.0;
            st[7] = item.targets.len() as f32 / 4.0;
            for (k, &(_, t)) in item.targets.iter().take(2).enumerate() {
                let tb = &mut st[8 + 8 * k..16 + 8 * k];
                match t {
                    Target::Player(p) => tb[(p != me) as usize] = 1.0,
                    Target::Obj(r) if self.live(r) => match self.objs[r.idx.i()].zone {
                        Zone::Battlefield => {
                            let side = (self.objs[r.idx.i()].controller != me) as usize;
                            tb[2 + 2 * side] = 1.0;
                            tb[3 + 2 * side] = slot_of(r.idx).map_or(0.0, |x| x as f32 / 32.0);
                        }
                        Zone::Stack => {
                            tb[6] = 1.0;
                            let pos = self
                                .stack
                                .iter()
                                .rev()
                                .position(|x| x.kind == StackKind::Spell { card: r.idx })
                                .unwrap_or(0);
                            tb[7] = pos as f32 / 8.0;
                        }
                        _ => {}
                    },
                    Target::Obj(_) => {}
                }
            }
        }
        // The viewer's waiting triggers (the `TRIGGER` slots when it orders them).
        for (k, t) in self.waiting.iter().filter(|t| t.controller == me).enumerate().take(TRIGS) {
            let o = O_TRIG + k * TRIG_W;
            code(&mut out[o..o + CODE], self.db, ids, t.source_def);
            out[o + CODE] = 1.0;
            out[o + CODE + 1] = (t.slot as f32 + 1.0) / 4.0;
            out[o + CODE + 2] = legal(act::TRIGGER + k as u16) as u8 as f32;
        }
        // The `LIBRARY` list of the viewer's own decision.
        if mine_to_decide {
            let list = self.decision_list(me);
            for (k, &def) in list.iter().enumerate().take(LIST) {
                let o = O_LIST + k * LIST_W;
                code(&mut out[o..o + CODE], self.db, ids, def);
                out[o + CODE] = 1.0;
                out[o + CODE + 1] = legal(act::LIBRARY + k as u16) as u8 as f32;
                out[o + CODE + 2] = list.iter().filter(|&&x| x == def).count() as f32 / 4.0;
            }
        }
        self.encode_context(me, ids, &mut out[O_CONTEXT..O_CONTEXT + CONTEXT]);
    }

    /// Per-identity counts.
    fn encode_bags(&self, me: PlayerId, ids: &[u16], out: &mut [f32]) {
        let op_ = opp(me);
        for &c in &self.players[me as usize].hand {
            bag_add(out, ids, bag::MY_HAND, self.objs[c.i()].def);
        }
        for &c in &self.battlefield {
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.controller == me { bag::MY_BATTLEFIELD } else { bag::OPP_BATTLEFIELD }, o.def);
        }
        for p in [me, op_] {
            for &c in &self.players[p as usize].graveyard {
                bag_add(out, ids, if p == me { bag::MY_GRAVEYARD } else { bag::OPP_GRAVEYARD }, self.objs[c.i()].def);
            }
        }
        for &c in &self.exile {
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.owner == me { bag::MY_EXILE } else { bag::OPP_EXILE }, o.def);
        }
        for &c in &self.command {
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.controller == me { bag::MY_EMBLEMS } else { bag::OPP_EMBLEMS }, o.def);
        }
        for &c in &self.players[me as usize].library {
            bag_add(out, ids, bag::MY_LIBRARY, self.objs[c.i()].def);
            if self.knows(me, c) {
                bag_add(out, ids, bag::MY_LIBRARY_KNOWN, self.objs[c.i()].def);
            }
        }
        let opl = &self.players[op_ as usize];
        for &c in opl.hand.iter().chain(opl.library.iter()) {
            let d = self.objs[c.i()].def;
            if !self.knows(me, c) {
                bag_add(out, ids, bag::OPP_HIDDEN, d);
            } else if self.objs[c.i()].zone == Zone::Hand {
                bag_add(out, ids, bag::OPP_HAND_KNOWN, d);
            } else {
                bag_add(out, ids, bag::OPP_LIBRARY_KNOWN, d);
            }
        }
        for k in self.clouds.iter().filter(|k| k.viewer == me && k.owner == op_) {
            let b = if k.hand { bag::OPP_HAND_CLOUD } else { bag::OPP_LIBRARY_KNOWN };
            for &d in &k.defs {
                bag_add(out, ids, b, d);
            }
        }
    }

    fn encode_globals(&self, me: PlayerId, g: &mut [f32]) {
        let t = &self.turn;
        g[0] = t.number as f32 / 50.0;
        g[1 + t.step as usize] = 1.0; // 14 steps: 1..15
        g[15] = (t.active == me) as u8 as f32;
        g[16] = (t.priority == me) as u8 as f32;
        g[17] = t.passes as f32 / 2.0;
        g[18] = (t.starting == me) as u8 as f32;
        g[19] = self.spec.max_turns.saturating_sub(t.number) as f32 / 50.0;
        g[20] = self.stack.len() as f32 / 8.0;
        g[21] = self.waiting.len() as f32 / 4.0;
        g[22] = self.extra_turns.len() as f32;
        g[23] = self.outcome.is_some() as u8 as f32;
        for (side, p) in [me, opp(me)].into_iter().enumerate() {
            let pl = &self.players[p as usize];
            let o = &mut g[24 + 40 * side..64 + 40 * side];
            o[0] = pl.life as f32 / 20.0;
            o[1] = pl.hand.len() as f32 / 10.0;
            o[2] = pl.library.len() as f32 / 40.0;
            o[3] = pl.graveyard.len() as f32 / 20.0;
            o[4] = self.exile.iter().filter(|&&c| self.objs[c.i()].owner == p).count() as f32 / 10.0;
            o[5] = pl.lands_played as f32;
            for k in 0..6 {
                o[6 + k] = pl.pool.amt[k] as f32 / 4.0;
            }
            o[12] = pl.mulligans as f32 / 4.0;
            o[13] = pl.kept as u8 as f32;
            o[14] = pl.drew_from_empty as u8 as f32;
            o[15] = pl.lost as u8 as f32;
            o[16] = pl.turns as f32 / 25.0;
            o[17] = pl.left_battlefield as u8 as f32;
            let (mut perms, mut creatures, mut lands, mut untapped_lands, mut power, mut tough) = (0, 0, 0, 0, 0, 0);
            let (mut ready, mut artifacts, mut tokens) = (0, 0, 0);
            for c in self.permanents_of(p) {
                perms += 1;
                let ob = &self.objs[c.i()];
                if self.is_creature(c) {
                    creatures += 1;
                    power += self.power(c).max(0);
                    tough += self.toughness(c).max(0);
                    if !ob.tapped && (!self.is_sick(c) || self.has_kw(c, Kw::Haste)) {
                        ready += 1;
                    }
                }
                if self.is_type(c, types::LAND) {
                    lands += 1;
                    if !ob.tapped {
                        untapped_lands += 1;
                    }
                }
                if self.is_type(c, types::ARTIFACT) {
                    artifacts += 1;
                }
                if self.def_of(c).token {
                    tokens += 1;
                }
            }
            o[18] = perms as f32 / 20.0;
            o[19] = creatures as f32 / 10.0;
            o[20] = lands as f32 / 10.0;
            o[21] = untapped_lands as f32 / 10.0;
            o[22] = power as f32 / 20.0;
            o[23] = tough as f32 / 20.0;
            o[24] = ready as f32 / 10.0;
            o[25] = artifacts as f32 / 10.0;
            o[26] = tokens as f32 / 10.0;
            o[27] = self.extra_turns.iter().filter(|&&x| x == p).count() as f32;
            o[28] = pl.lost_life as u8 as f32;
            o[29] = pl.flash_sorceries as u8 as f32;
        }
    }

    /// The potential blocker being asked (DeclareBlockers), and the attacker
    /// and blocker of a damage assignment (AssignDamage).
    fn combat_context(&self) -> (Option<CardIdx>, Option<CardIdx>, Option<CardIdx>) {
        if self.pending.is_none() {
            return (None, None, None);
        }
        match self.agenda.last() {
            Some(Task::DeclareBlockers { i }) => (self.potential_blockers().get(*i as usize).copied(), None, None),
            Some(Task::AssignDamage { att, j, .. }) => {
                let blk = self.blockers_of(*att);
                (None, Some(*att), blk.get(*j as usize).copied())
            }
            _ => (None, None, None),
        }
    }

    /// The definitions behind the `LIBRARY` slots of `p`'s pending decision:
    /// a searched library, a looked-at pile, a seen hand (sorted by
    /// definition, ENGINE.md 11).
    fn decision_list(&self, p: PlayerId) -> Vec<DefId> {
        use crate::decision::DecisionKind as K;
        let Some(d) = &self.pending else { return Vec::new() };
        let cards: Vec<CardIdx> = match d.kind {
            K::SearchLibrary | K::NameCard => self.library_sorted(p),
            K::PutOnTop | K::PutOnBottom | K::PutInGraveyard | K::PutInHand => match self.agenda.last() {
                Some(Task::Run(f)) => {
                    let mut v = f.pile.clone();
                    v.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                    v
                }
                _ => Vec::new(),
            },
            K::ChooseDiscard => {
                let owner = d
                    .options
                    .iter()
                    .find_map(|o| match o.choice {
                        crate::decision::Choice::Card(c) => Some(self.objs[c.i()].owner),
                        _ => None,
                    })
                    .unwrap_or(opp(p));
                self.hand_sorted(owner)
            }
            _ => Vec::new(),
        };
        cards.iter().map(|c| self.objs[c.i()].def).collect()
    }

    fn encode_context(&self, me: PlayerId, ids: &[u16], x: &mut [f32]) {
        let Some(d) = &self.pending else { return };
        assert!((d.kind as usize) < ctx::KINDS, "decision kind {:?} beyond the context block", d.kind);
        x[ctx::KIND + d.kind as usize] = 1.0;
        x[ctx::DECIDER_IS_VIEWER] = (d.player == me) as u8 as f32;
        x[ctx::PENDING] = 1.0;
        if d.player == me {
            x[ctx::OPTIONS] = d.options.len() as f32 / 32.0;
        }
        let Some(task) = self.agenda.last() else { return };
        let t = task_code(task);
        assert!(t < ctx::TASKS);
        x[ctx::TASK + t] = 1.0;
        // The card behind the decision: the resolving or paid-for ability,
        // the spell being cast, the land entering, the attacker.
        let mut card: Option<(DefId, Option<AbilitySlot>, Option<u8>)> = None;
        let stack_item = |id: u32| -> Option<(DefId, Option<AbilitySlot>, Option<u8>)> {
            let i = self.item_index(id)?;
            Some(match self.stack[i].kind {
                StackKind::Spell { card } => (self.objs[card.i()].def, Some(AbilitySlot::Spell), None),
                StackKind::Ability { ability, .. } => (ability.def, Some(ability.slot), ability.mode),
            })
        };
        let frame_card = |t: &Task| -> Option<(DefId, Option<AbilitySlot>, Option<u8>)> {
            match t {
                Task::Run(f) => Some((f.ability.def, Some(f.ability.slot), f.ability.mode)),
                Task::ChooseTargets { id, .. }
                | Task::CastOption { id }
                | Task::ChooseModes { id }
                | Task::PayCosts { id, .. }
                | Task::CastPay { id }
                | Task::ActPay { id } => stack_item(*id),
                _ => None,
            }
        };
        match task {
            Task::Run(f) => {
                card = frame_card(task);
                let prog = &self.db.ability(f.ability).program;
                if let Some(op) = prog.get(f.pc as usize) {
                    let o = op_code(op);
                    assert!(o < ctx::OPS);
                    x[ctx::OP + o] = 1.0;
                }
                x[ctx::PC] = f.pc as f32 / 32.0;
                x[ctx::OP_I] = f.i as f32 / 8.0;
                x[ctx::PILE] = f.pile.len() as f32 / 8.0;
            }
            Task::PayMana(pm) => {
                // The cost being paid: the nearest task below that names it.
                card = self.agenda.iter().rev().skip(1).find_map(&frame_card);
                let c = &pm.cost;
                x[ctx::COST] = c.generic as f32 / 8.0;
                for k in 0..6 {
                    x[ctx::COST + 1 + k] = c.pips[k] as f32 / 4.0;
                }
                x[ctx::COST + 7] = c.hybrid.iter().map(|&n| n as f32).sum::<f32>() / 4.0;
                x[ctx::COST + 8] = c.phyrexian.iter().map(|&n| n as f32).sum::<f32>() / 4.0;
            }
            Task::AsEnters { card: c } => card = Some((self.objs[c.i()].def, None, None)),
            Task::AssignDamage { att, left, .. } => {
                card = Some((self.objs[att.i()].def, None, None));
                x[ctx::DAMAGE_LEFT] = *left as f32 / 16.0;
            }
            Task::ChooseTargets { req, .. } => {
                card = frame_card(task);
                x[ctx::REQ] = *req as f32 / 4.0;
            }
            Task::ChooseModes { id } => {
                card = frame_card(task);
                if let Some(i) = self.item_index(*id) {
                    x[ctx::MODES_CHOSEN] = self.stack[i].modes.len() as f32 / 4.0;
                }
            }
            Task::PayCosts { picked, .. } => {
                card = frame_card(task);
                x[ctx::PICKED] = picked.len() as f32 / 4.0;
            }
            Task::DeclareAttackers { next } => x[ctx::NEXT_ATTACKER] = *next as f32 / 32.0,
            Task::AttackTarget { card: c } => card = Some((self.objs[c.i()].def, None, None)),
            Task::ChooseFace { card: c, .. } => card = Some((self.objs[c.i()].def, None, None)),
            Task::DeclareBlockers { i } => x[ctx::BLOCKER_INDEX] = *i as f32 / 16.0,
            Task::MulliganBottom { left, .. } => x[ctx::BOTTOMS_LEFT] = *left as f32 / 7.0,
            Task::ManaNow { src } => card = Some((self.objs[src.card.i()].def, Some(src.slot), None)),
            _ => card = frame_card(task),
        }
        if let Some((def, slot, mode)) = card {
            code(&mut x[ctx::CARD..ctx::CARD + CODE], self.db, ids, def);
            x[ctx::HAS_CARD] = 1.0;
            if let Some(s) = slot {
                let (k, i) = match s {
                    AbilitySlot::Spell => (0, 0),
                    AbilitySlot::Trigger(i) => (1, i),
                    AbilitySlot::Activated(i) => (2, i),
                    AbilitySlot::Hand(i) => (3, i),
                };
                x[ctx::SLOT_KIND + k] = 1.0;
                x[ctx::SLOT_INDEX] = i as f32 / 4.0;
            }
            if let Some(m) = mode {
                x[ctx::MODE] = (m as f32 + 1.0) / 8.0;
            }
        }
    }
}

/// The object layout of this encoding for zero-core's token network (arch `tokens`, zero-core
/// `python/zero/net.py` `TokenNet`; ENGINE.md 22): JSON with the identity capacity as the
/// vocabulary, the object segments (slot offsets and widths, the present flag, the identity as
/// one-hot digits of 16, 8 and 8, the action ids each slot scores) and the bags (15 multisets of
/// 1024 identities). Every other feature (the 128 globals) is the global token's input.
/// Checked in as `mtgz/az/layout_v3.json` (a test keeps the two equal). `cards` (the name behind
/// each identity of `db`, for analysis; null where none) is not read by the network.
pub fn layout_json(db: &'static CardDb) -> String {
    use serde_json::json;
    let id = json!({"at": 0, "radix": [16, 8, 8]});
    let seg = |name: &str, offset: usize, count: usize, width: usize, present: usize, actions: serde_json::Value| {
        json!({"name": name, "offset": offset, "count": count, "width": width, "present": present,
               "id": id.clone(), "actions": actions})
    };
    let a = |base: u16, stride: u16| json!([base, stride]);
    let ids = identity::identities(db);
    let mut cards: Vec<Option<String>> = vec![None; D];
    for (def, &i) in ids.iter().enumerate() {
        // An emblem shares the identity of the card that makes it, a face
        // its card's.
        if i != identity::UNKNOWN && !db.def(def as DefId).emblem && !db.is_face(def as DefId) {
            cards[i as usize] = Some(db.def(def as DefId).name.clone());
        }
    }
    let v = json!({
        "version": 3,
        "features": NUM_FEATURES,
        "vocab": D,
        "segments": [
            {"name": "context", "offset": O_CONTEXT, "count": 1, "width": CONTEXT, "present": null,
             "id": {"at": ctx::CARD, "radix": [16, 8, 8]}},
            {"name": "tops", "offset": O_TOPS, "count": 2 * TOPS, "width": TOP_W, "present": 1,
             "id": {"at": 2, "radix": [16, 8, 8]}},
            seg("my_permanents", O_MY_PERM, PERMS, PERM_W, CODE + perm::PRESENT,
                json!([a(act::MY_PERM, 1), a(act::ACTIVATE, 4), a(act::ACTIVATE + 1, 4), a(act::ACTIVATE + 2, 4),
                       a(act::ACTIVATE + 3, 4)])),
            seg("opponent_permanents", O_OPP_PERM, PERMS, PERM_W, CODE + perm::PRESENT, json!([a(act::OPP_PERM, 1)])),
            seg("hand", O_HAND, HANDS, HAND_W, CODE, json!([a(act::HAND, 1), a(act::HAND_ABILITY, 1)])),
            seg("stack", O_STACK, STACKS, STACK_W, CODE, json!([a(act::STACK, 1)])),
            seg("triggers", O_TRIG, TRIGS, TRIG_W, CODE, json!([a(act::TRIGGER, 1)])),
            seg("list", O_LIST, LIST, LIST_W, CODE, json!([a(act::LIBRARY, 1)])),
        ],
        "bags": [{"name": "bags", "offset": O_BAGS, "count": BAGS, "width": D}],
        "cards": cards,
    });
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

/// Keywords in the permanent status, in this order.
const KWS: [Kw; 15] = [
    Kw::Flying,
    Kw::Reach,
    Kw::FirstStrike,
    Kw::DoubleStrike,
    Kw::Deathtouch,
    Kw::Trample,
    Kw::Lifelink,
    Kw::Vigilance,
    Kw::Haste,
    Kw::Defender,
    Kw::Menace,
    Kw::Flash,
    Kw::Hexproof,
    Kw::Shroud,
    Kw::Indestructible,
];
