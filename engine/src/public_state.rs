//! The public-state encoding gate (ENGINE.md 58.6): the dual of the
//! hidden-information checks. Those prove that worlds differing only in hidden
//! things encode identically; this proves that public things get into the
//! network's input (the declared mulligan of ENGINE.md 55 passed every check
//! while missing from it).
//!
//! 1. **Classification** ([`fields`]): every field of the structs that hold a
//!    game's state (`Game`, `Player`, `Obj`, `Turn`, `StackItem`, `ManaPool`,
//!    `Dungeon`, the match's `MatchState`) is listed, by an exhaustive
//!    destructuring without `..` (as `lower/` does for the IR): a new field is
//!    a compile error here until it is classified **public** (it needs a
//!    perturbation below), **hidden** (the hidden-information checks cover
//!    it) or **derived** (internal, a cache, a transient, settings: exempt,
//!    with a reason).
//! 2. **Perturbations** ([`perturbations`]): for each public field one or more
//!    small legal-ish changes of a position's clone (life + 1, a counter,
//!    tap / untap, a card from a graveyard to exile, the step, ...), made by
//!    hand on the state (never through the rules, whose triggers and events
//!    would change other things too). `tests/az.rs` applies each to positions
//!    of random games (the cube, the Delver mirror, best-of-three, a drafted
//!    match) and asserts that the view (`Game::view_of` and [`match_view`])
//!    changed for both players (the change is public) and that the encoding
//!    changed for both viewers.
//! 3. **Limits** ([`limits`]): the perturbations whose change the encoding
//!    leaves out, each with its reason: by design (bag order, what the
//!    opponent knows of the viewer's cards; the perturbations change objects
//!    in the encoded slots only: 32 permanents a side, 8 stack items), or a gap
//!    with no spare float left in layout v4, for a layout v5 (ENGINE.md
//!    58.10). Anything else that fails is a finding.

use crate::az::matches::{Format, MatchState};
use crate::az::MtgAz;
use crate::db::{types, AbilitySlot, Counter, Kw, KwSet, Mod};
use crate::game::{Dungeon, Game, Obj, Player, StackItem, StackKind, Step, Turn};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::layers::{Affected, Until};
use crate::mana::ManaPool;
use crate::rng::Rng;

/// A field's class (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Public state: it needs a perturbation.
    Public,
    /// Hidden from (at least) one player: the hidden-information checks
    /// (`determinize`, the leak tests) cover it.
    Hidden,
    /// Internal, a cache, a transient that is clear at every decision, the
    /// settings, or a container whose elements are classified on their own:
    /// exempt.
    Derived,
}

/// One classified field: `Owner.field`, its class and why.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub path: &'static str,
    pub class: Class,
    pub why: &'static str,
}

/// The classification of one struct: a destructuring without `..` (a field
/// missing here, or a new field of the struct, is a compile error), and the
/// table of its fields.
macro_rules! classify {
    ($out:ident, $ty:ident { $($f:ident : $class:ident $why:literal),* $(,)? }) => {{
        #[allow(dead_code, unused_variables)]
        fn exhaustive(x: &$ty) {
            let $ty { $($f: _),* } = x;
        }
        $($out.push(Field { path: concat!(stringify!($ty), ".", stringify!($f)), class: Class::$class, why: $why });)*
    }};
}

/// Every field of the game-state structs, classified (module docs).
pub fn fields() -> Vec<Field> {
    let mut v = Vec::new();
    classify!(v, Game {
        db: Derived "the static card table, the same in every position",
        spec: Derived "the game's settings, fixed for a game and the same in every world; the one that varies a position, the turn cap, is encoded as the turns left (g[19])",
        rng: Hidden "the engine's future randomness: determinize reseeds it",
        objs: Derived "the container of objects: their fields are classified under Obj",
        players: Derived "the container of players: their fields are classified under Player",
        battlefield: Public "which permanents, in timestamp order (the slot order)",
        exile: Public "the face-up exiled cards",
        face_down: Public "cards exiled face down: their number is public, their identities hidden from the player who doesn't know them (leak-tested)",
        face_down_ids: Hidden "the cards behind face-down permanents (ENGINE.md 31): hidden from the player who doesn't know them",
        entering_face_down: Derived "transient: set and cleared around one move",
        moving_face_down: Derived "transient: set and cleared around one move",
        command: Public "emblems and dungeons in the command zone",
        stack: Derived "the container of stack items: their fields are classified under StackItem",
        turn: Derived "the turn's fields are classified under Turn",
        agenda: Derived "the engine's continuation stack: what a player sees of it is the pending decision (the context block)",
        pending: Derived "the pending decision, computed from the agenda: its kind and decider are the context block's, its options the decider's",
        answer: Derived "the answer being applied: always empty between apply calls",
        waiting: Public "triggered abilities waiting to be put on the stack",
        extra_turns: Public "extra turns to come",
        combat_dmg: Derived "combat damage assigned so far in a damage step, read by the assigning player's next AssignDamage decision (the context's damage left)",
        next_ts: Derived "a counter for new timestamps (their order is the battlefield's)",
        next_stack_id: Derived "a counter for new stack ids",
        outcome: Public "the game's result",
        clouds: Hidden "a player's partial knowledge of hidden cards (ENGINE.md 10): the viewer's own, leak-tested",
        tokens: Derived "a fast path: tokens created so far",
        first_token: Derived "a fast path: the first slot made as a token or a copy",
        batching: Derived "transient: a simultaneous move in progress",
        ev_seq: Derived "event-group bookkeeping",
        batch_fired: Derived "event-group bookkeeping (\"one or more\" triggers once per group)",
        group_ts: Derived "event-group bookkeeping (what entered at the same time)",
        entering_cast: Derived "transient: set and cleared around one move",
        entering_evoked: Derived "transient: set and cleared around one move",
        observer: Derived "which world determinize made, for its key masking",
        keys: Derived "chance keys of the current apply: always empty between calls",
        forced: Derived "the search's forced chance outcomes",
        forced_at: Derived "the search's forced chance outcomes",
        decisions: Derived "a count of decisions answered (statistics)",
        no_auto: Derived "a flag of determinize's refresh",
        effects: Public "continuous effects from resolved spells and abilities",
        any_loss: Derived "a fast path: some effect removes all abilities",
        counters: Public "counters on permanents",
        n_static: Derived "a fast path: permanents with static abilities",
        n_self_types: Derived "a fast path: permanents that change their own types",
        leaving: Derived "transient: last known information during one simultaneous move",
        links: Public "cards exiled with a permanent (until it leaves, CR 610.3)",
        delayed: Public "delayed triggered abilities waiting for their moment (CR 603.7)",
        resolved: Public "how many times abilities of objects resolved this turn",
        permissions: Public "permissions to play cards from other zones (CR 601.2a)",
        combat_damage_now: Derived "transient: combat damage is being dealt",
        quiet_leave: Derived "transient: set and cleared around one move",
        monarch: Public "the monarch (CR 724)",
        lki_counters: Derived "last known information (CR 608.2h) of permanents that left this turn",
        entering_escaped: Derived "transient: set and cleared around one move",
        entering_x: Derived "transient: set and cleared around one move",
        entering_sunburst: Derived "transient: set and cleared around one move",
        entering_exiled: Derived "transient: set and cleared around one move",
        entering_mods: Derived "transient: set and cleared around one move",
        entering_counters: Derived "transient: set and cleared around one move",
        entering_tapped: Derived "transient: set and cleared around one move",
        discard_entered: Derived "transient: Mox Diamond's answer, consumed by its entering",
        repl_decided: Derived "transient: a replacement choice made for the next move",
        route_choice: Derived "transient: the route of the cast about to begin (its decision's context)",
        entering_dash: Derived "transient: set and cleared around one move",
        entering_warp: Derived "transient: set and cleared around one move",
        miracle_cast: Derived "transient: the card a miracle trigger is casting (its decisions' context)",
        miracle_check: Derived "transient: a first draw that may be revealed for miracle, asked before the next priority",
        entering_face: Derived "transient: set and cleared around one move",
        day: Public "day or night (CR 731)",
        done_once: Public "optional \"only once each turn\" actions done this turn",
        discarded: Public "cards discarded this turn (mayhem, CR 702.187b)",
        temp_triggers: Public "triggered abilities a player has until their next turn",
        gy_exile: Public "spells that are exiled instead of being put into a graveyard",
        copies: Public "copy effects on permanents (CR 707)",
        copy_objs: Derived "which slots are copies (the copy flags of permanents and spells)",
        lki_copy: Derived "last known information (CR 608.2h)",
        lki_damage: Derived "last known information (CR 608.2h)",
        spell_lki: Derived "last known information (CR 608.2h)",
        entering_copy: Derived "transient: set and cleared around one move",
        entering_times: Derived "transient: set and cleared around one move",
        enter_note: Derived "transient: set and cleared around one move",
        defer_enters: Derived "transient: an enters event held back for one choice",
        pile_moving: Derived "transient: between ops",
        pile_shuffles: Derived "transient: between ops",
        prepared: Public "prepared permanents and their prepare spell copies (CR 722.3)",
        may_copies: Derived "an engine loop cap's counter (docs/shortcuts.md 26.11.7)",
        activations: Derived "an engine loop cap's counter (docs/shortcuts.md 29.6.6): no rule reads it",
        seen: Derived "fast paths: rare rules some card of the game has",
        initiative: Public "the initiative (CR 725)",
        dungeons: Derived "each player's dungeon: its fields are classified under Dungeon",
        rules_srcs: Derived "objects in no zone naming the initiative's and the monarch's triggers' source",
        choosing_for: Derived "transient: the item whose modes are chosen (0 between tasks)",
        targeting: Derived "transient: the item whose targets are chosen (0 between tasks; ENGINE.md 58.1)",
        marks: Public "rare per-object facts (exhausted, a chosen colour, exerted, can't be countered, ...)",
        known_pools: Derived "with hidden decks, the pool names each player knows of the other's: set by the match, fixed for the game",
        log: Derived "the human-readable log (tests and the viewer)",
    });
    classify!(v, Player {
        life: Public "CR 119",
        library: Public "its size is public (its cards and order hidden: leak-tested)",
        hand: Public "its size is public (its cards hidden from the other player: leak-tested)",
        graveyard: Public "the cards and their order (CR 404.2)",
        pool: Derived "the mana pool: its fields are classified under ManaPool",
        lands_played: Public "lands played this turn",
        drew_from_empty: Public "drew from an empty library (704.5b)",
        lost: Public "lost the game",
        mulligans: Public "CR 103.5",
        kept: Public "CR 103.5",
        mulligan_declared: Public "CR 103.5 (ENGINE.md 55)",
        last_turn: Derived "the number of the player's last turn: the turn number, who is active and the extra turns determine it",
        turns: Public "turns begun",
        left_battlefield: Public "a permanent left under their control this turn (revolt)",
        drawn: Public "cards drawn this turn",
        spells_cast: Public "spells cast this turn (storm, prowess, flurry)",
        noncreature_cast: Public "noncreature spells cast this turn",
        gained_life: Public "gained life this turn",
        cant_gain_life: Public "can't gain life for the rest of the game",
        attacked_with: Public "creatures declared as attackers this turn",
        draw_step_draws: Public "cards drawn in their draw step this turn",
        spells_game: Public "spells cast this game",
        energy: Public "energy counters (CR 122.1)",
        lost_life: Public "lost life this turn",
        flash_sorceries: Public "may cast sorceries as though they had flash",
        poison: Public "poison counters",
        experience: Public "experience counters",
        gy_play_turn: Public "may play lands and cast spells from their graveyard this turn",
        gy_exile_turn: Public "cards going to their graveyard this turn are exiled instead",
        city_blessing: Public "the city's blessing (CR 702.131b)",
        prot_until: Public "protection from everything until their turn count reaches it (CR 702.16j)",
        sideboard: Public "its size is public (its cards are the player's own: hidden, leak-tested)",
        companion: Public "the revealed companion's card (CR 103.2b)",
        companion_def: Public "the revealed companion's identity",
        companion_used: Public "the companion special action was taken (116.2g)",
    });
    classify!(v, Obj {
        def: Public "what a permanent or spell is (the face up); in a hidden zone hidden (leak-tested)",
        owner: Public "CR 108.3",
        controller: Public "CR 108.4",
        zone: Derived "mirrors the zone lists (battlefield, exile, graveyards, ...: perturbed as moves there)",
        inc: Derived "the incarnation counter that makes stale references (CR 400.7)",
        timestamp: Public "the battlefield's order (CR 613.7)",
        tapped: Public "CR 110.5",
        damage: Public "marked damage (CR 120.3e)",
        deathtouched: Derived "transient: dealt deathtouch damage since the last state-based check",
        control_since: Public "when its controller gained control (summoning sickness, CR 302.6)",
        attacking: Public "CR 506.4",
        blocked: Public "CR 509.1h",
        blocking: Public "the attacker it blocks",
        struck_first: Public "dealt first-strike combat damage (510.4)",
        known: Hidden "which players know a hidden card (ENGINE.md 10): leak-tested",
        attached: Public "what it is attached to (CR 301.5, 303.4)",
        evoked: Public "it entered evoked (CR 702.74a)",
        lki_power: Derived "last known information (CR 608.2h)",
        flashback: Public "a spell cast with flashback: exiled when it leaves the stack (CR 702.34a)",
        escaped: Public "it entered as a spell cast with escape (CR 702.138c)",
        chosen_player: Public "the player chosen as it entered (CR 614.12)",
        monstrous: Public "CR 701.37b",
        attacks: Public "what it attacks (a planeswalker, CR 506.3)",
        entered: Public "the turn it entered (\"entered this turn\")",
        pw_turn: Public "loyalty abilities activated this turn (CR 606.3)",
        pw_used: Public "loyalty abilities activated this turn (CR 606.3)",
        pw_extra: Derived "how many more loyalty abilities it may activate this turn (Comet): read through pw_used's readiness",
        flips: Derived "a wrapping count of transformations, for CR 701.27f's check",
    });
    classify!(v, Turn {
        number: Public "the turn number",
        active: Public "the active player",
        step: Public "the step",
        priority: Public "who has priority",
        passes: Public "consecutive passes (CR 117.4)",
        starting: Public "the starting player",
        normal: Public "the player of the last normal turn (extra turns, CR 500.7)",
        cleanup_again: Derived "transient within a cleanup step (CR 514.3a)",
        extra_combats: Public "additional combat phases to follow (CR 500.8)",
        chooser: Public "who chose who plays first (CR 103.1)",
    });
    classify!(v, StackItem {
        id: Derived "an internal name (the stack's order is its slots)",
        kind: Public "a spell or which ability of which source",
        controller: Public "CR 108.4",
        targets: Public "CR 601.2c",
        cost_option: Public "the additional cost chosen (CR 601.2b)",
        modes: Public "the modes chosen (CR 700.2)",
        ev: Public "a triggered ability's event: its subjects and amount (\"that much\")",
        alt: Public "the alternative cost it was cast for (CR 118.9)",
        from: Public "where and how it was cast from",
        delved: Public "cards exiled for delve so far (CR 702.66)",
        delve_floor: Derived "transient: where delve's next pick starts",
        discarded_land: Derived "Mox Diamond's internal note",
        x: Public "the value of X (CR 107.3a)",
        division: Public "divided damage (CR 601.2d)",
        attack_as: Public "what a ninjutsu creature will attack (CR 702.49c)",
        noted: Public "permanents a trigger noted (Dack Fayden's emblem)",
        flips: Derived "its source's transformation count as it was put on the stack (CR 701.27f)",
        origin: Public "the zone it was cast from",
        times: Public "times an additional cost was paid (replicate, squad)",
        spliced: Public "cards spliced onto it (CR 702.47)",
    });
    classify!(v, ManaPool {
        amt: Public "mana in the pool (CR 106.4)",
        art: Public "the part of the colorless mana that pays only for artifact spells (Mishra's Workshop)",
        haste: Public "the part of the red mana that gives haste (Arena of Glory)",
    });
    classify!(v, Dungeon {
        obj: Public "the dungeon card in the command zone (CR 309.4)",
        room: Public "the venture marker's room",
        completed: Public "dungeons completed (CR 309.7)",
    });
    classify!(v, MatchState {
        format: Public "best-of-one or best-of-three",
        gspec: Derived "the games' settings",
        pools: Hidden "each player's pool: what the other knows of it (open decklists; best-of-three but the first picks) is the OPP_POOL bag; the rest hidden (ENGINE.md 36.11)",
        decks: Derived "each player's deck: its size is the match block's, its cards the game's zones",
        game_no: Public "the game of the match",
        results: Public "each game's result",
        choosers: Public "who chose who played first in each game",
        starters: Public "who played first in each game",
        wins: Public "game wins",
        draws: Public "drawn games",
        turns_before: Derived "the match's turn accounting of the games before",
        over: Derived "the match is over: a terminal state, never encoded for a decision",
        seen: Derived "identities revealed in the current game: become `earlier` between games",
        earlier: Public "identities the players revealed in earlier games",
        sb: Derived "sideboarding between games: encoded by encode_sideboarding, not a game position",
        rng: Hidden "seeds the later games: determinize reseeds it",
        priors: Hidden "the prior of each player's unknown first picks (ENGINE.md 36.11)",
    });
    v
}

/// One perturbation: `apply` changes a clone of a position by hand and
/// describes the change, or returns `None` when the position has nothing to
/// change (no creature, no stack item, ...). `field` is the classified field
/// it perturbs; `rare`: positions of random games seldom have what it needs,
/// so the gate reports it without requiring it to be applied.
pub struct Perturbation {
    pub name: &'static str,
    pub field: &'static str,
    pub rare: bool,
    pub apply: fn(&mut MtgAz, &mut Rng) -> Option<String>,
}

/// A public change the encoding leaves out (`name`: the perturbation's), and
/// why: by design, or a gap that only a layout v5 can close.
pub struct Limit {
    pub name: &'static str,
    pub kind: LimitKind,
    pub why: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// A deliberate limit of the encoding (ENGINE.md 58.9).
    Design,
    /// A genuine gap with no spare float left in layout v4: for layout v5
    /// (ENGINE.md 58.10).
    V5,
}

fn pick<T: Copy>(v: &[T], r: &mut Rng) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[r.below(v.len() as u32) as usize])
    }
}

/// The permanents in the encoded slots (the first 32 of each controller,
/// timestamp order), but face-down ones (their card is hidden).
fn slots(g: &Game) -> Vec<CardIdx> {
    let mut v: Vec<CardIdx> = g.permanents_of(0).take(crate::az::encode::PERMS).collect();
    v.extend(g.permanents_of(1).take(crate::az::encode::PERMS));
    v.retain(|&c| g.objs[c.i()].def != g.db.face_down_def());
    v
}

fn slot_creatures(g: &Game) -> Vec<CardIdx> {
    slots(g).into_iter().filter(|&c| g.is_creature(c)).collect()
}

/// The stack items in the encoded slots (the top 8), as indices into
/// `Game::stack`.
fn stack_slots(g: &Game) -> Vec<usize> {
    (0..g.stack.len()).rev().take(crate::az::encode::STACKS).collect()
}

fn stack_spells(g: &Game) -> Vec<(usize, CardIdx)> {
    stack_slots(g)
        .into_iter()
        .filter_map(|i| match g.stack[i].kind {
            StackKind::Spell { card } => Some((i, card)),
            StackKind::Ability { .. } => None,
        })
        .collect()
}

fn player(r: &mut Rng) -> PlayerId {
    r.below(2) as PlayerId
}

/// A card moves from where it is to `to` by hand (no events, no triggers):
/// out of its zone list, into the new one (the top of an ordered zone), a
/// new object (CR 400.7: references to it go stale), its status cleared.
fn raw_move(g: &mut Game, c: CardIdx, to: Zone) {
    let from = g.objs[c.i()].zone;
    let owner = g.objs[c.i()].owner as usize;
    match from {
        Zone::Battlefield => g.battlefield.retain(|&x| x != c),
        Zone::Exile => g.exile.retain(|&x| x != c),
        Zone::FaceDown => g.face_down.retain(|&x| x != c),
        Zone::Graveyard => g.players[owner].graveyard.retain(|&x| x != c),
        Zone::Hand => g.players[owner].hand.retain(|&x| x != c),
        Zone::Library => g.players[owner].library.retain(|&x| x != c),
        Zone::Command => g.command.retain(|&x| x != c),
        _ => {}
    }
    if from == Zone::Battlefield {
        g.counters.retain(|x| x.0 != c);
    }
    let token = g.is_token(c);
    let to = if token && to != Zone::Battlefield { Zone::Nowhere } else { to };
    match to {
        Zone::Exile => g.exile.push(c),
        Zone::FaceDown => g.face_down.push(c),
        Zone::Graveyard => g.players[owner].graveyard.push(c),
        Zone::Hand => g.players[owner].hand.push(c),
        _ => {}
    }
    let o = &mut g.objs[c.i()];
    o.zone = to;
    o.inc = o.inc.wrapping_add(1);
    o.controller = o.owner;
    o.tapped = false;
    o.damage = 0;
    o.attacking = false;
    o.blocked = false;
    o.blocking = None;
    o.attached = None;
    o.attacks = None;
    o.known = match to {
        Zone::FaceDown | Zone::Hand | Zone::Library => 1 << o.owner,
        _ => 0b11,
    };
}

/// What continuous effects change of permanent `c` (layers 4-7 and the rules
/// flags), from the engine's accessors.
fn characteristics(g: &Game, c: CardIdx) -> (u16, u8, (i32, i32), u32, crate::layers::Flags, crate::db::Prot, bool) {
    (g.types_of(c), g.colors_of(c), g.pt(c), g.keywords_of(c).0, g.flags_of(c), g.protection_of(c), g.lost_abilities(c))
}

/// An until-end-of-turn effect `m` on permanent `c`.
fn effect(g: &mut Game, c: CardIdx, m: Mod) -> Option<String> {
    let r = g.obj_ref(c);
    let p = g.objs[c.i()].controller;
    let what = format!("{m:?} on {}", g.name(c));
    g.add_effect(Affected::Obj(r), m, Until::EndOfTurn, p);
    Some(what)
}

/// `n` counters of kind `k` on permanent `c`, by hand (no events).
fn counter(g: &mut Game, c: CardIdx, k: Counter) -> Option<String> {
    match g.counters.iter_mut().find(|x| x.0 == c && x.1 == k) {
        Some(x) => x.2 += 1,
        None => g.counters.push((c, k, 1)),
    }
    Some(format!("a {k:?} counter on {}", g.name(c)))
}

fn mark(g: &mut Game, c: CardIdx, m: crate::game::Mark) -> Option<String> {
    let r = g.obj_ref(c);
    if g.marks.contains(&(r, m)) {
        return None;
    }
    g.marks.push((r, m));
    Some(format!("{m:?} on {}", g.name(c)))
}

/// A player's field changed by `f` (a random player).
fn pl(a: &mut MtgAz, r: &mut Rng, f: impl FnOnce(&mut Player) -> Option<String>) -> Option<String> {
    let p = player(r);
    f(&mut a.g.players[p as usize]).map(|s| format!("P{p}: {s}"))
}

/// A stack item in the encoded slots changed by `f`.
fn item(a: &mut MtgAz, r: &mut Rng, f: impl Fn(&mut StackItem) -> Option<String>) -> Option<String> {
    let i = pick(&stack_slots(&a.g), r)?;
    f(&mut a.g.stack[i]).map(|s| format!("stack item {} from the top: {s}", a.g.stack.len() - 1 - i))
}

fn flip(b: &mut bool) -> Option<String> {
    *b = !*b;
    Some(format!("now {b}"))
}

macro_rules! p {
    ($name:literal, $field:literal, $f:expr) => {
        Perturbation { name: $name, field: $field, rare: false, apply: $f }
    };
    (rare $name:literal, $field:literal, $f:expr) => {
        Perturbation { name: $name, field: $field, rare: true, apply: $f }
    };
}

/// Every perturbation (module docs).
pub fn perturbations() -> Vec<Perturbation> {
    vec![
        // ---- Game ----
        p!("a permanent leaves the battlefield", "Game.battlefield", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let n = a.g.name(c);
            raw_move(&mut a.g, c, Zone::Graveyard);
            Some(format!("{n} to its owner's graveyard"))
        }),
        p!("a graveyard card is exiled", "Game.exile", |a, r| {
            let cards: Vec<CardIdx> = (0..2).flat_map(|p| a.g.players[p].graveyard.clone()).collect();
            let c = pick(&cards, r)?;
            raw_move(&mut a.g, c, Zone::Exile);
            Some(format!("{} exiled", a.g.name(c)))
        }),
        p!("an exiled card is turned face down", "Game.face_down", |a, r| {
            let c = pick(&a.g.exile.clone(), r)?;
            raw_move(&mut a.g, c, Zone::FaceDown);
            Some(format!("{} face down", a.g.name(c)))
        }),
        p!("a player gets an emblem", "Game.command", |a, r| {
            let db = a.g.db;
            let def = (0..db.len() as DefId).find(|&d| db.def(d).emblem && db.def(d).statics.is_empty())?;
            let p = player(r);
            a.g.create_emblem(def, p);
            Some(format!("P{p} gets {}", db.def(def).name))
        }),
        p!("a trigger waits to be put on the stack", "Game.waiting", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.def_of(c).triggers.is_empty()).collect();
            let c = pick(&cs, r)?;
            let t = crate::triggers::PendingTrigger {
                controller: a.g.objs[c.i()].controller,
                source: a.g.obj_ref(c),
                source_def: a.g.objs[c.i()].def,
                slot: 0,
                ev: Default::default(),
            };
            a.g.waiting.push(t);
            Some(format!("{}'s trigger", a.g.name(c)))
        }),
        p!("a player gets an extra turn", "Game.extra_turns", |a, r| {
            let p = player(r);
            a.g.extra_turns.push(p);
            Some(format!("P{p}"))
        }),
        p!("the game ends", "Game.outcome", |a, r| {
            if a.g.outcome.is_some() {
                return None;
            }
            let w = player(r);
            a.g.outcome = Some(crate::game::Outcome { winner: Some(w), reason: crate::game::EndReason::Effect });
            Some(format!("P{w} wins"))
        }),
        p!("an effect ends", "Game.effects", |a, r| {
            // (An effect on a permanent in the encoded slots that is the only
            // one of its kind there: removing a duplicate or an effect on an
            // object that left changes no characteristic. The kinds the
            // limits name have perturbations of their own.)
            let ss = slots(&a.g);
            let g = &a.g;
            let own = |m: &Mod| {
                matches!(m, Mod::MustAttack | Mod::MustAttackPlayer | Mod::ExileIfDies | Mod::GainSacMana(_) | Mod::GainLatent(_))
                    || matches!(m, Mod::Types { add: 0, .. })
            };
            let live: Vec<usize> = (0..g.effects.len())
                .filter(|&i| !own(&g.effects[i].m))
                .filter(|&i| match &g.effects[i].on {
                    Affected::Obj(o) => {
                        g.live(*o)
                            && ss.contains(&o.idx)
                            && !g.effects.iter().enumerate().any(|(j, e)| j != i && e.on == g.effects[i].on && e.m == g.effects[i].m)
                    }
                    Affected::Group(_) => false,
                })
                .collect();
            let i = pick(&live, r)?;
            let Affected::Obj(o) = a.g.effects[i].on.clone() else { return None };
            let before = characteristics(&a.g, o.idx);
            let e = a.g.effects.remove(i);
            a.g.effects_changed();
            // (Only an end that changes its characteristics or rules flags: a
            // base 0/0 set on a 0/0, "becomes a <subtype> creature" on a
            // creature, which changes only subtypes, as "a creature gains a
            // creature type".)
            if characteristics(&a.g, o.idx) == before {
                return None;
            }
            Some(format!("{:?} on {:?}", e.m, e.on))
        }),
        p!("+1/+1 until end of turn", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::Pt(1, 1))
        }),
        p!("base power and toughness 7/7", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.pt(c) != (7, 7)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::SetPt(7, 7))
        }),
        p!("a creature gains flying", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.has_kw(c, Kw::Flying)).collect();
            let c = pick(&cs, r)?;
            let mut k = KwSet::default();
            k.insert(Kw::Flying);
            effect(&mut a.g, c, Mod::Keywords(k))
        }),
        p!("a permanent loses all abilities", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.lost_abilities(c)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::LoseAllAbilities)
        }),
        p!("a creature is goaded", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.goaded_by(c, 0) && !a.g.goaded_by(c, 1)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::Goad)
        }),
        p!("exiled if it would leave", "Game.effects", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            effect(&mut a.g, c, Mod::ExileIfLeaves)
        }),
        p!("a creature can't block", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::CantBlock)
        }),
        p!("a creature can't be blocked", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::CantBeBlocked)
        }),
        p!("a creature attacks each combat", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::MustAttack)
        }),
        p!("exiled if it would die", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::ExileIfDies)
        }),
        p!("a permanent becomes an artifact too", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.is_type(c, types::ARTIFACT)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::Types { add: types::ARTIFACT, subtypes: Vec::new(), replace: false })
        }),
        p!("a land becomes a 3/3 creature", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::LAND) && !a.g.is_creature(c)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::Types { add: types::CREATURE, subtypes: Vec::new(), replace: false })?;
            effect(&mut a.g, c, Mod::SetPt(3, 3))
        }),
        p!("a creature gains a creature type", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::Types { add: 0, subtypes: vec![1], replace: false })
        }),
        p!("a permanent's colour becomes another", "Game.effects", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let now = a.g.colors_of(c);
            let to = (0..5u8).map(|k| 1u8 << k).find(|&m| m != now)?;
            effect(&mut a.g, c, Mod::Colors(to))
        }),
        p!("a creature gains protection from red", "Game.effects", |a, r| {
            // (One without protection: the encoding's flag is a summary.)
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.protection_of(c).is_empty()).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::Protection(crate::db::Prot { colors: 1 << 3, ..Default::default() }))
        }),
        p!("a +1/+1 counter on a creature", "Game.counters", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            counter(&mut a.g, c, Counter::P1P1)
        }),
        p!("a -1/-1 counter on a creature", "Game.counters", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            counter(&mut a.g, c, Counter::M1M1)
        }),
        p!("a +1/+1 counter on a noncreature permanent", "Game.counters", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.is_creature(c)).collect();
            let c = pick(&cs, r)?;
            counter(&mut a.g, c, Counter::P1P1)
        }),
        p!("a loyalty counter on a planeswalker", "Game.counters", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::PLANESWALKER)).collect();
            let c = pick(&cs, r)?;
            counter(&mut a.g, c, Counter::Loyalty)
        }),
        p!("a stun counter", "Game.counters", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            counter(&mut a.g, c, Counter::Stun)
        }),
        p!("a named counter (charge, oil, ...)", "Game.counters", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            counter(&mut a.g, c, Counter::Named(0))
        }),
        p!("a flying counter on a creature", "Game.counters", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.has_kw(c, Kw::Flying)).collect();
            let c = pick(&cs, r)?;
            counter(&mut a.g, c, Counter::Kw(Kw::Flying))
        }),
        p!("a card is exiled until a permanent leaves", "Game.links", |a, r| {
            let s = pick(&slots(&a.g), r)?;
            let c = pick(&a.g.exile.clone(), r)?;
            let (source, card) = (a.g.obj_ref(s), a.g.obj_ref(c));
            a.g.links.push(crate::game::Link { source, card, until: true, back: Zone::Battlefield, until_monarch: None });
            Some(format!("{} with {}", a.g.name(c), a.g.name(s)))
        }),
        p!("a delayed trigger waits", "Game.delayed", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let d = crate::game::Delayed {
                when: crate::db::DelayedWhen::NextEndStep,
                controller: a.g.objs[c.i()].controller,
                source: a.g.obj_ref(c),
                source_def: a.g.objs[c.i()].def,
                slot: 0,
                ev: crate::triggers::EvCtx { obj: Some(a.g.obj_ref(c)), ..Default::default() },
            };
            a.g.delayed.push(d);
            Some(format!("at the next end step, about {}", a.g.name(c)))
        }),
        p!("an ability resolved this turn", "Game.resolved", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let o = a.g.obj_ref(c);
            match a.g.resolved.iter_mut().find(|x| x.0 == o) {
                Some(x) => x.1 += 1,
                None => a.g.resolved.push((o, 1)),
            }
            Some(format!("of {}", a.g.name(c)))
        }),
        p!("an exiled card may be played this turn", "Game.permissions", |a, r| {
            let c = pick(&a.g.exile.clone(), r)?;
            let card = a.g.obj_ref(c);
            let player = a.g.objs[c.i()].owner;
            let kind = crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None };
            a.g.permissions.push(crate::game::Permission { card, player, turn: Some(a.g.turn.number), kind });
            Some(format!("{} by P{player}", a.g.name(c)))
        }),
        p!("a player becomes the monarch", "Game.monarch", |a, r| {
            let p = player(r);
            if a.g.monarch == Some(p) {
                return None;
            }
            a.g.monarch = Some(p);
            Some(format!("P{p}"))
        }),
        p!("day becomes night or night day", "Game.day", |a, _| {
            a.g.day = Some(!a.g.day.unwrap_or(false));
            Some(format!("{:?}", a.g.day))
        }),
        p!("a once-each-turn action was done", "Game.done_once", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.def_of(c).triggers.is_empty()).collect();
            let c = pick(&cs, r)?;
            let o = a.g.obj_ref(c);
            if a.g.done_once.contains(&(o, 0)) {
                return None;
            }
            a.g.done_once.push((o, 0));
            Some(format!("{}'s trigger 0", a.g.name(c)))
        }),
        p!("a graveyard card was discarded this turn", "Game.discarded", |a, r| {
            let cards: Vec<CardIdx> = (0..2).flat_map(|p| a.g.players[p].graveyard.clone()).collect();
            let c = pick(&cards, r)?;
            let o = a.g.obj_ref(c);
            if a.g.discarded.contains(&o) {
                return None;
            }
            a.g.discarded.push(o);
            Some(a.g.name(c).to_string())
        }),
        p!("a trigger until a player's next turn", "Game.temp_triggers", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.def_of(c).triggers.is_empty()).collect();
            let c = pick(&cs, r)?;
            let t = crate::game::TempTrigger {
                controller: a.g.objs[c.i()].controller,
                source: a.g.obj_ref(c),
                source_def: a.g.objs[c.i()].def,
                slot: 0,
                this_turn: true,
                obj: None,
            };
            a.g.temp_triggers.push(t);
            Some(format!("{}'s trigger", a.g.name(c)))
        }),
        p!("a spell will be exiled instead of put into a graveyard", "Game.gy_exile", |a, r| {
            let cs: Vec<CardIdx> = stack_spells(&a.g).into_iter().map(|x| x.1).filter(|c| !a.g.gy_exile.contains(c)).collect();
            let c = pick(&cs, r)?;
            a.g.gy_exile.push(c);
            Some(a.g.name(c).to_string())
        }),
        p!("a permanent becomes a copy of another", "Game.copies", |a, r| {
            let ss = slots(&a.g);
            let c = pick(&ss, r)?;
            let others: Vec<DefId> = ss.iter().map(|&x| a.g.objs[x.i()].def).filter(|&d| d != a.g.objs[c.i()].def).collect();
            let d = pick(&others, r)?;
            a.g.add_copy_effect(c, crate::copies::CopyVals { def: d, exc: Default::default() }, Until::EndOfTurn);
            Some(format!("{} copies {}", a.g.name(c), a.g.db.def(d).name))
        }),
        p!(rare "a prepared permanent stops being prepared", "Game.prepared", |a, r| {
            if a.g.prepared.is_empty() {
                return None;
            }
            let i = r.below(a.g.prepared.len() as u32) as usize;
            a.g.prepared.remove(i);
            Some("removed".into())
        }),
        p!("a player takes the initiative", "Game.initiative", |a, r| {
            let p = player(r);
            if a.g.initiative == Some(p) {
                return None;
            }
            a.g.initiative = Some(p);
            Some(format!("P{p}"))
        }),
        p!("a permanent was exhausted (an exhaust ability)", "Game.marks", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            mark(&mut a.g, c, crate::game::Mark::Exhausted(0))
        }),
        p!("a colour was chosen as a permanent entered", "Game.marks", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let k = r.below(5) as u8;
            if a.g.marks.contains(&(a.g.obj_ref(c), crate::game::Mark::ChosenColor(k))) {
                return None;
            }
            a.g.marks.retain(|(o, m)| !(o.idx == c && matches!(m, crate::game::Mark::ChosenColor(_))));
            mark(&mut a.g, c, crate::game::Mark::ChosenColor(k))
        }),
        p!("a creature was exerted", "Game.marks", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            mark(&mut a.g, c, crate::game::Mark::Exerted)
        }),
        p!("a spell can't be countered", "Game.marks", |a, r| {
            let (_, c) = pick(&stack_spells(&a.g), r)?;
            mark(&mut a.g, c, crate::game::Mark::Uncounterable)
        }),
        // ---- Dungeon ----
        p!("a player completes a dungeon", "Dungeon.completed", |a, r| {
            let p = player(r);
            a.g.dungeons[p as usize].completed += 1;
            Some(format!("P{p}"))
        }),
        p!(rare "the venture marker moves to the next room", "Dungeon.room", |a, r| {
            let p = player(r) as usize;
            a.g.dungeons[p].obj?;
            a.g.dungeons[p].room += 1;
            Some(format!("P{p}"))
        }),
        p!(rare "a dungeon leaves the command zone", "Dungeon.obj", |a, r| {
            let p = player(r) as usize;
            a.g.dungeons[p].obj.take().map(|_| format!("P{p}"))
        }),
        // ---- Turn ----
        p!("the turn number + 1", "Turn.number", |a, _| {
            a.g.turn.number += 1;
            Some(format!("{}", a.g.turn.number))
        }),
        p!("the other player is active", "Turn.active", |a, _| {
            a.g.turn.active = opp(a.g.turn.active);
            Some(format!("P{}", a.g.turn.active))
        }),
        p!("another step", "Turn.step", |a, r| {
            let steps = [Step::Upkeep, Step::Draw, Step::Main1, Step::BeginCombat, Step::DeclareAttackers, Step::Main2, Step::End];
            let s: Vec<Step> = steps.into_iter().filter(|&s| s != a.g.turn.step).collect();
            a.g.turn.step = pick(&s, r)?;
            Some(format!("{:?}", a.g.turn.step))
        }),
        p!("the other player has priority", "Turn.priority", |a, _| {
            a.g.turn.priority = opp(a.g.turn.priority);
            Some(format!("P{}", a.g.turn.priority))
        }),
        p!("a player passed", "Turn.passes", |a, _| {
            if a.g.turn.passes >= 1 {
                return None;
            }
            a.g.turn.passes += 1;
            Some(format!("{}", a.g.turn.passes))
        }),
        p!("the other player started", "Turn.starting", |a, _| {
            a.g.turn.starting = opp(a.g.turn.starting);
            Some(format!("P{}", a.g.turn.starting))
        }),
        p!("the last normal turn was the other player's", "Turn.normal", |a, _| {
            a.g.turn.normal = opp(a.g.turn.normal);
            Some(format!("P{}", a.g.turn.normal))
        }),
        p!("an additional combat phase follows", "Turn.extra_combats", |a, _| {
            a.g.turn.extra_combats += 1;
            Some(format!("{}", a.g.turn.extra_combats))
        }),
        p!("the other player chose who played first", "Turn.chooser", |a, _| {
            a.g.turn.chooser = opp(a.g.turn.chooser);
            Some(format!("P{}", a.g.turn.chooser))
        }),
        // ---- Player ----
        p!("life + 1", "Player.life", |a, r| pl(a, r, |p| {
            p.life += 1;
            Some(format!("{}", p.life))
        })),
        p!("a library card leaves the game", "Player.library", |a, r| {
            let p = player(r) as usize;
            let c = *a.g.players[p].library.first()?;
            raw_move(&mut a.g, c, Zone::Nowhere);
            Some(format!("P{p}'s bottom card"))
        }),
        p!("a hand card is discarded", "Player.hand", |a, r| {
            let p = player(r) as usize;
            let c = pick(&a.g.players[p].hand.clone(), r)?;
            raw_move(&mut a.g, c, Zone::Graveyard);
            Some(format!("P{p}'s {}", a.g.name(c)))
        }),
        p!("two graveyard cards swap places", "Player.graveyard", |a, r| {
            let p = player(r) as usize;
            let gy = &a.g.players[p].graveyard;
            let pairs: Vec<(usize, usize)> = (0..gy.len())
                .flat_map(|i| (i + 1..gy.len()).map(move |j| (i, j)))
                .filter(|&(i, j)| a.g.objs[gy[i].i()].def != a.g.objs[gy[j].i()].def)
                .collect();
            let (i, j) = pick(&pairs, r)?;
            a.g.players[p].graveyard.swap(i, j);
            Some(format!("P{p}'s {i} and {j}"))
        }),
        p!("mana in a pool", "ManaPool.amt", |a, r| {
            let k = r.below(6) as usize;
            pl(a, r, |p| {
                p.pool.amt[k] += 1;
                Some(format!("type {k}"))
            })
        }),
        p!("colorless mana for artifacts only", "ManaPool.art", |a, r| pl(a, r, |p| {
            if p.pool.amt[5] == p.pool.art {
                return None;
            }
            p.pool.art += 1;
            Some(format!("{} of {}", p.pool.art, p.pool.amt[5]))
        })),
        p!(rare "red mana that gives haste", "ManaPool.haste", |a, r| pl(a, r, |p| {
            if p.pool.amt[3] == p.pool.haste {
                return None;
            }
            p.pool.haste += 1;
            Some(format!("{} of {}", p.pool.haste, p.pool.amt[3]))
        })),
        p!("a land played", "Player.lands_played", |a, r| pl(a, r, |p| {
            p.lands_played += 1;
            Some(format!("{}", p.lands_played))
        })),
        p!("drew from an empty library", "Player.drew_from_empty", |a, r| pl(a, r, |p| flip(&mut p.drew_from_empty))),
        p!("lost", "Player.lost", |a, r| pl(a, r, |p| flip(&mut p.lost))),
        p!("a mulligan taken", "Player.mulligans", |a, r| pl(a, r, |p| {
            p.mulligans += 1;
            Some(format!("{}", p.mulligans))
        })),
        p!("kept", "Player.kept", |a, r| pl(a, r, |p| flip(&mut p.kept))),
        p!("declared a mulligan", "Player.mulligan_declared", |a, r| pl(a, r, |p| flip(&mut p.mulligan_declared))),
        p!("a turn begun", "Player.turns", |a, r| pl(a, r, |p| {
            p.turns += 1;
            Some(format!("{}", p.turns))
        })),
        p!("a permanent left their battlefield this turn", "Player.left_battlefield", |a, r| pl(a, r, |p| flip(&mut p.left_battlefield))),
        p!("a card drawn this turn", "Player.drawn", |a, r| pl(a, r, |p| {
            p.drawn += 1;
            Some(format!("{}", p.drawn))
        })),
        p!("a spell cast this turn", "Player.spells_cast", |a, r| pl(a, r, |p| {
            p.spells_cast += 1;
            Some(format!("{}", p.spells_cast))
        })),
        p!("a noncreature spell cast this turn", "Player.noncreature_cast", |a, r| pl(a, r, |p| {
            p.noncreature_cast += 1;
            Some(format!("{}", p.noncreature_cast))
        })),
        p!("gained life this turn", "Player.gained_life", |a, r| pl(a, r, |p| flip(&mut p.gained_life))),
        p!("can't gain life", "Player.cant_gain_life", |a, r| pl(a, r, |p| flip(&mut p.cant_gain_life))),
        p!("a creature attacked this turn", "Player.attacked_with", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            let p = a.g.objs[c.i()].controller as usize;
            let o = a.g.obj_ref(c);
            if a.g.players[p].attacked_with.contains(&o) {
                return None;
            }
            a.g.players[p].attacked_with.push(o);
            Some(format!("P{p}'s {}", a.g.name(c)))
        }),
        p!("a card drawn in the draw step", "Player.draw_step_draws", |a, r| pl(a, r, |p| {
            p.draw_step_draws += 1;
            Some(format!("{}", p.draw_step_draws))
        })),
        p!("a spell cast this game", "Player.spells_game", |a, r| pl(a, r, |p| {
            p.spells_game += 1;
            Some(format!("{}", p.spells_game))
        })),
        p!("an energy counter", "Player.energy", |a, r| pl(a, r, |p| {
            p.energy += 1;
            Some(format!("{}", p.energy))
        })),
        p!("lost life this turn", "Player.lost_life", |a, r| pl(a, r, |p| flip(&mut p.lost_life))),
        p!("sorceries as though they had flash", "Player.flash_sorceries", |a, r| pl(a, r, |p| flip(&mut p.flash_sorceries))),
        p!("a poison counter", "Player.poison", |a, r| pl(a, r, |p| {
            p.poison += 1;
            Some(format!("{}", p.poison))
        })),
        p!("an experience counter", "Player.experience", |a, r| pl(a, r, |p| {
            p.experience += 1;
            Some(format!("{}", p.experience))
        })),
        p!("may play from their graveyard this turn", "Player.gy_play_turn", |a, r| {
            let t = a.g.turn.number;
            pl(a, r, |p| {
                if p.gy_play_turn == t {
                    return None;
                }
                p.gy_play_turn = t;
                Some(format!("turn {t}"))
            })
        }),
        p!("cards to their graveyard are exiled this turn", "Player.gy_exile_turn", |a, r| {
            let t = a.g.turn.number;
            pl(a, r, |p| {
                if p.gy_exile_turn == t {
                    return None;
                }
                p.gy_exile_turn = t;
                Some(format!("turn {t}"))
            })
        }),
        p!("the city's blessing", "Player.city_blessing", |a, r| pl(a, r, |p| flip(&mut p.city_blessing))),
        p!("protection from everything until their next turn", "Player.prot_until", |a, r| pl(a, r, |p| {
            if p.prot_until > p.turns {
                return None;
            }
            p.prot_until = p.turns + 1;
            Some(format!("until turn {}", p.prot_until))
        })),
        p!("a sideboard card fewer", "Player.sideboard", |a, r| pl(a, r, |p| {
            if p.sideboard.is_empty() {
                return None;
            }
            let v: Vec<DefId> = p.sideboard[1..].to_vec();
            p.sideboard = std::sync::Arc::from(v);
            Some(format!("{} left", p.sideboard.len()))
        })),
        p!(rare "a revealed companion is put into its owner's hand", "Player.companion", |a, r| {
            let p = player(r) as usize;
            let c = a.g.players[p].companion?;
            if a.g.objs[c.i()].zone != Zone::Nowhere {
                return None;
            }
            raw_move(&mut a.g, c, Zone::Hand);
            Some(format!("P{p}: {}", a.g.name(c)))
        }),
        p!(rare "a companion is revealed", "Player.companion_def", |a, r| {
            let p = player(r) as usize;
            if a.g.players[p].companion_def.is_some() {
                return None;
            }
            let d = *a.g.players[p].sideboard.first()?;
            a.g.players[p].companion_def = Some(d);
            Some(format!("P{p}: {}", a.g.db.def(d).name))
        }),
        p!(rare "the companion special action was taken", "Player.companion_used", |a, r| {
            let p = player(r) as usize;
            a.g.players[p].companion_def?;
            flip(&mut a.g.players[p].companion_used).map(|s| format!("P{p}: {s}"))
        }),
        // ---- Obj ----
        p!("a permanent is another card", "Obj.def", |a, r| {
            let ss = slots(&a.g);
            let c = pick(&ss, r)?;
            let others: Vec<DefId> = ss.iter().map(|&x| a.g.objs[x.i()].def).filter(|&d| d != a.g.objs[c.i()].def).collect();
            let d = pick(&others, r)?;
            let n = a.g.name(c);
            a.g.objs[c.i()].def = d;
            Some(format!("{n} is {}", a.g.db.def(d).name))
        }),
        p!("a permanent's owner is the other player", "Obj.owner", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            if a.g.is_token(c) {
                return None;
            }
            let o = &mut a.g.objs[c.i()];
            o.owner = opp(o.owner);
            Some(format!("{}", a.g.name(c)))
        }),
        p!("a permanent changes control", "Obj.controller", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let o = &mut a.g.objs[c.i()];
            o.controller = opp(o.controller);
            o.attacking = false;
            o.blocking = None;
            Some(format!("{}", a.g.name(c)))
        }),
        p!("two permanents swap timestamps", "Obj.timestamp", |a, r| {
            let g = &a.g;
            let pairs: Vec<(usize, usize)> = (0..g.battlefield.len())
                .flat_map(|i| (i + 1..g.battlefield.len()).map(move |j| (i, j)))
                .filter(|&(i, j)| {
                    let (x, y) = (&g.objs[g.battlefield[i].i()], &g.objs[g.battlefield[j].i()]);
                    x.controller == y.controller
                        && x.def != y.def
                        && slots(g).contains(&g.battlefield[i])
                        && slots(g).contains(&g.battlefield[j])
                })
                .collect();
            let (i, j) = pick(&pairs, r)?;
            let (x, y) = (a.g.battlefield[i], a.g.battlefield[j]);
            let (tx, ty) = (a.g.objs[x.i()].timestamp, a.g.objs[y.i()].timestamp);
            a.g.objs[x.i()].timestamp = ty;
            a.g.objs[y.i()].timestamp = tx;
            a.g.battlefield.swap(i, j);
            Some(format!("{} and {}", a.g.name(x), a.g.name(y)))
        }),
        p!("a permanent taps or untaps", "Obj.tapped", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].tapped).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("damage on a creature", "Obj.damage", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            a.g.objs[c.i()].damage += 1;
            Some(format!("{}: {}", a.g.name(c), a.g.objs[c.i()].damage))
        }),
        p!("a creature came under its controller's control this turn", "Obj.control_since", |a, r| {
            let t = a.g.turn.number;
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.objs[c.i()].control_since != t).collect();
            let c = pick(&cs, r)?;
            a.g.objs[c.i()].control_since = t;
            Some(a.g.name(c).to_string())
        }),
        p!("a creature attacks", "Obj.attacking", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.objs[c.i()].attacking).collect();
            let c = pick(&cs, r)?;
            a.g.objs[c.i()].attacking = true;
            Some(a.g.name(c).to_string())
        }),
        p!("an attacker becomes blocked", "Obj.blocked", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].blocked).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("a creature blocks another", "Obj.blocking", |a, r| {
            let cs = slot_creatures(&a.g);
            let c = pick(&cs, r)?;
            let others: Vec<CardIdx> = cs.iter().copied().filter(|&x| a.g.objs[x.i()].controller != a.g.objs[c.i()].controller).collect();
            let x = pick(&others, r)?;
            if a.g.objs[c.i()].blocking == Some(x) {
                return None;
            }
            a.g.objs[c.i()].blocking = Some(x);
            Some(format!("{} blocks {}", a.g.name(c), a.g.name(x)))
        }),
        p!("a creature dealt first-strike damage", "Obj.struck_first", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].struck_first).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("a permanent attaches to a creature", "Obj.attached", |a, r| {
            let ss = slots(&a.g);
            let c = pick(&ss, r)?;
            let hosts: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&h| h != c).collect();
            let h = pick(&hosts, r)?;
            let hr = a.g.obj_ref(h);
            if a.g.objs[c.i()].attached == Some(hr) {
                return None;
            }
            a.g.objs[c.i()].attached = Some(hr);
            Some(format!("{} to {}", a.g.name(c), a.g.name(h)))
        }),
        p!("a permanent entered evoked", "Obj.evoked", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].evoked).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("a spell was cast with flashback", "Obj.flashback", |a, r| {
            let (_, c) = pick(&stack_spells(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].flashback).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("a permanent escaped", "Obj.escaped", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].escaped).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!("a player was chosen as a permanent entered", "Obj.chosen_player", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let p = player(r);
            if a.g.objs[c.i()].chosen_player == Some(p) {
                return None;
            }
            a.g.objs[c.i()].chosen_player = Some(p);
            Some(format!("{}: P{p}", a.g.name(c)))
        }),
        p!("a creature is monstrous", "Obj.monstrous", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            flip(&mut a.g.objs[c.i()].monstrous).map(|s| format!("{}: {s}", a.g.name(c)))
        }),
        p!(rare "an attacker attacks a planeswalker", "Obj.attacks", |a, r| {
            let atk: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.objs[c.i()].attacking).collect();
            let c = pick(&atk, r)?;
            let ctl = a.g.objs[c.i()].controller;
            let pws: Vec<CardIdx> =
                slots(&a.g).into_iter().filter(|&w| a.g.is_type(w, types::PLANESWALKER) && a.g.objs[w.i()].controller != ctl).collect();
            let w = pick(&pws, r)?;
            let wr = a.g.obj_ref(w);
            if a.g.objs[c.i()].attacks == Some(wr) {
                return None;
            }
            a.g.objs[c.i()].attacks = Some(wr);
            Some(format!("{} attacks {}", a.g.name(c), a.g.name(w)))
        }),
        p!("a permanent entered this turn", "Obj.entered", |a, r| {
            let t = a.g.turn.number as u16;
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.objs[c.i()].entered != t).collect();
            let c = pick(&cs, r)?;
            a.g.objs[c.i()].entered = t;
            Some(a.g.name(c).to_string())
        }),
        p!("a planeswalker's loyalty ability was activated this turn", "Obj.pw_used", |a, r| {
            let t = a.g.turn.number as u16;
            let cs: Vec<CardIdx> = slots(&a.g)
                .into_iter()
                .filter(|&c| a.g.is_type(c, types::PLANESWALKER) && !(a.g.objs[c.i()].pw_turn == t && a.g.objs[c.i()].pw_used > 0))
                .collect();
            let c = pick(&cs, r)?;
            let o = &mut a.g.objs[c.i()];
            o.pw_turn = t;
            o.pw_used = 1;
            Some(a.g.name(c).to_string())
        }),
        p!("a planeswalker's activations are another turn's", "Obj.pw_turn", |a, r| {
            let t = a.g.turn.number as u16;
            let cs: Vec<CardIdx> =
                slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::PLANESWALKER) && a.g.objs[c.i()].pw_turn == t && a.g.objs[c.i()].pw_used > 0).collect();
            let c = pick(&cs, r)?;
            a.g.objs[c.i()].pw_turn = t.wrapping_sub(1);
            Some(a.g.name(c).to_string())
        }),
        // ---- StackItem ----
        p!("a stack item is another ability of its source", "StackItem.kind", |a, r| item(a, r, |s| match &mut s.kind {
            StackKind::Ability { ability, .. } => match &mut ability.slot {
                AbilitySlot::Trigger(i) | AbilitySlot::Activated(i) => {
                    *i ^= 1;
                    Some(format!("ability {i}"))
                }
                _ => None,
            },
            StackKind::Spell { .. } => None,
        })),
        p!("a stack item's controller is the other player", "StackItem.controller", |a, r| item(a, r, |s| {
            s.controller = opp(s.controller);
            Some(format!("P{}", s.controller))
        })),
        p!("a stack item targets the other player", "StackItem.targets", |a, r| item(a, r, |s| {
            let t = s.targets.iter_mut().find(|t| matches!(t.1, Target::Player(_)))?;
            if let Target::Player(p) = &mut t.1 {
                *p = opp(*p);
            }
            Some(format!("{:?}", t.1))
        })),
        p!("another additional cost was chosen", "StackItem.cost_option", |a, r| item(a, r, |s| {
            s.cost_option ^= 1;
            Some(format!("{}", s.cost_option))
        })),
        p!("a spell's mode is another", "StackItem.modes", |a, r| item(a, r, |s| {
            let m = s.modes.first_mut()?;
            *m ^= 1;
            Some(format!("mode {m}"))
        })),
        p!("a trigger's amount is another", "StackItem.ev", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Trigger(_))) {
                return None;
            }
            s.ev.amount += 1;
            Some(format!("{}", s.ev.amount))
        })),
        p!("a spell was cast for an alternative cost", "StackItem.alt", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Spell { .. }) {
                return None;
            }
            s.alt ^= 1;
            Some(format!("alt {}", s.alt))
        })),
        p!("a spell was cast from a graveyard with flashback", "StackItem.from", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Spell { .. }) || s.from == crate::game::CastFrom::Flashback {
                return None;
            }
            s.from = crate::game::CastFrom::Flashback;
            Some("flashback".into())
        })),
        p!("a card exiled for delve", "StackItem.delved", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Spell { .. }) {
                return None;
            }
            s.delved += 1;
            Some(format!("{}", s.delved))
        })),
        p!("X is another", "StackItem.x", |a, r| item(a, r, |s| {
            s.x += 1;
            Some(format!("X = {}", s.x))
        })),
        p!(rare "divided damage is divided otherwise", "StackItem.division", |a, r| item(a, r, |s| {
            if s.division.len() < 2 || s.division[0] < 2 {
                return None;
            }
            s.division[0] -= 1;
            s.division[1] += 1;
            Some(format!("{:?}", s.division))
        })),
        p!(rare "a ninjutsu creature will attack a planeswalker", "StackItem.attack_as", |a, r| item(a, r, |s| {
            s.attack_as?;
            s.attack_as = Some(None).filter(|_| s.attack_as != Some(None));
            Some(format!("{:?}", s.attack_as))
        })),
        p!(rare "a trigger noted a permanent", "StackItem.noted", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let o = a.g.obj_ref(c);
            item(a, r, move |s| {
                if !matches!(s.kind, StackKind::Ability { .. }) || s.noted.contains(&o) {
                    return None;
                }
                s.noted.push(o);
                Some("noted".into())
            })
        }),
        p!("a spell was cast from another zone", "StackItem.origin", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Spell { .. }) {
                return None;
            }
            s.origin = if s.origin == Zone::Graveyard { Zone::Hand } else { Zone::Graveyard };
            Some(format!("{:?}", s.origin))
        })),
        p!("an additional cost was paid once more", "StackItem.times", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Spell { .. }) {
                return None;
            }
            s.times += 1;
            Some(format!("{}", s.times))
        })),
        p!(rare "a card is spliced onto a spell", "StackItem.spliced", |a, r| item(a, r, |s| {
            let StackKind::Spell { .. } = s.kind else { return None };
            let d = *s.spliced.first()?;
            s.spliced.push(d);
            Some("spliced".into())
        })),
        // ---- MatchState ----
        p!(rare "a best-of-three is a best-of-one", "MatchState.format", |a, _| {
            // (Not the other way: a best-of-one has no prior of first picks.)
            if a.m.format != Format::Bo3 {
                return None;
            }
            a.m.format = Format::Bo1;
            Some("bo1".into())
        }),
        p!("the next game of the match", "MatchState.game_no", |a, _| {
            a.m.game_no += 1;
            Some(format!("{}", a.m.game_no))
        }),
        p!("an earlier game's result", "MatchState.results", |a, r| {
            let i = (0..3).find(|&i| a.m.results[i] == 0)?;
            a.m.results[i] = 1 + r.below(3) as u8;
            Some(format!("game {}: {}", i + 1, a.m.results[i]))
        }),
        p!(rare "another chose who played first", "MatchState.choosers", |a, _| {
            // (Of a game played: the current game's is the game's chooser.)
            if a.m.results[0] == 0 {
                return None;
            }
            a.m.choosers[0] = opp(a.m.choosers[0]);
            Some("game 1".into())
        }),
        p!(rare "another played first", "MatchState.starters", |a, _| {
            if a.m.results[0] == 0 {
                return None;
            }
            a.m.starters[0] = opp(a.m.starters[0]);
            Some("game 1".into())
        }),
        p!("a game win", "MatchState.wins", |a, r| {
            let p = player(r) as usize;
            a.m.wins[p] += 1;
            Some(format!("P{p}"))
        }),
        p!("a drawn game", "MatchState.draws", |a, _| {
            a.m.draws += 1;
            Some(format!("{}", a.m.draws))
        }),
        p!("an identity revealed in an earlier game", "MatchState.earlier", |a, r| {
            let p = player(r) as usize;
            let w = r.below(8) as usize;
            let b = 1u64 << r.below(64);
            if a.m.earlier[p][w] & b != 0 {
                return None;
            }
            a.m.earlier[p][w] |= b;
            Some(format!("P{p}"))
        }),
    ]
}

/// The perturbations whose public change the encoding leaves out (ENGINE.md
/// 58.9 by design, 58.10 for layout v5), each with its reason.
pub fn limits() -> Vec<Limit> {
    use LimitKind::{Design, V5};
    let l = |name, kind, why| Limit { name, kind, why };
    vec![
        l("two graveyard cards swap places", Design, "bag order: graveyards are bags of identities (counts), not ordered slots (ENGINE.md 22.2); the order matters to a few cards only"),
        l("an exiled card is turned face down", Design, "the owner's view: a card of the viewer's exiled face down is in its exile bag by identity like a face-up one (it knows it); that the opponent doesn't is not encoded (what the opponent knows of the viewer's cards is encoded for the hand only, hand st[4])"),
        l("an identity revealed in an earlier game", Design, "the revealing viewer's view: OPP_REVEALED is the opponent's reveals; what the opponent knows of the viewer's own pool is not encoded (as for a card the viewer exiled face down)"),
        l("a permanent entered this turn", Design, "NEW is 'under its controller's control since this turn' (CR 302.6); the turn it entered differs from it only after a control change, read by two cards"),
        l("a creature attacks each combat", V5, "rules effect MustAttack (MustAttackPlayer; goad is encoded): no spare permanent float"),
        l("a creature gains a creature type", V5, "subtypes (layer 4 changes of them: 'it's a Zombie in addition'): a card's are its identity's, an effect's are not encoded (tribal cards are rare in the cube)"),
        l("exiled if it would die", V5, "rules effect ExileIfDies: no spare permanent float"),
        l("a delayed trigger waits", V5, "delayed triggered abilities (Mana Drain's mana, Sneak Attack's sacrifice, Flickerwisp's return, a Bauble's draw): a per-player count needs globals"),
        l("an ability resolved this turn", V5, "times abilities resolved this turn: a per-permanent float (rare: 'the second time this ability resolved')"),
        l("an exiled card may be played this turn", V5, "permissions to play exiled cards (impulse draw, Urza, Fallen Shinobi, Dauthi Voidwalker): no exile segment; the holder sees them as EXILE options when it decides"),
        l("a once-each-turn action was done", V5, "optional once-each-turn actions done: a per-permanent float (rare)"),
        l("a graveyard card was discarded this turn", V5, "mayhem's 'discarded this turn' (CR 702.187b): graveyards are bags (rare)"),
        l("a spell will be exiled instead of put into a graveyard", V5, "where a spell goes as it leaves the stack: the stack's spare float holds X"),
        l("a permanent was exhausted (an exhaust ability)", V5, "Mark::Exhausted: no spare permanent float (one card)"),
        l("a colour was chosen as a permanent entered", V5, "Mark::ChosenColor: no spare permanent float (Painter's Servant's colour is in the descriptor's colours, 58.8)"),
        l("a creature was exerted", V5, "Mark::Exerted (it won't untap next): no spare permanent float (one card)"),
        l("the last normal turn was the other player's", V5, "the extra-turn order (CR 500.7): a global"),
        l("an additional combat phase follows", V5, "additional combat phases (CR 500.8): a global"),
        l("colorless mana for artifacts only", V5, "the artifact-only part of the pool's colorless mana (Mishra's Workshop): a per-player global"),
        l("red mana that gives haste", V5, "the haste part of the pool's red mana (Arena of Glory): a per-player global"),
        l("a card drawn this turn", V5, "cards drawn this turn: a per-player global"),
        l("a noncreature spell cast this turn", V5, "noncreature spells cast this turn: a per-player global (spells cast this turn are encoded, 58.8)"),
        l("gained life this turn", V5, "a per-player global"),
        l("can't gain life", V5, "a per-player global (Screaming Nemesis)"),
        l("a creature attacked this turn", V5, "creatures that attacked this turn (raid; during combat ATTACKING shows them): a per-player global"),
        l("a card drawn in the draw step", V5, "draws in the draw step this turn (Orcish Bowmasters): a per-player global"),
        l("a spell cast this game", V5, "a per-player global"),
        l("may play from their graveyard this turn", V5, "Yawgmoth's Will this turn: a per-player global"),
        l("cards to their graveyard are exiled this turn", V5, "a per-player global"),
        l("the city's blessing", V5, "a per-player global (ascend)"),
        l("protection from everything until their next turn", V5, "The One Ring's protection (CR 702.16j): a per-player global"),
        l("a permanent entered evoked", V5, "no spare permanent float (its sacrifice trigger is on the stack, encoded)"),
        l("a permanent escaped", V5, "no spare permanent float"),
        l("a player was chosen as a permanent entered", V5, "no spare permanent float (True-Name Nemesis, Dauthi Voidwalker)"),
        l("a creature is monstrous", V5, "no spare permanent float"),
        l("a spell's mode is another", V5, "which modes were chosen (their count and the targets are encoded): the stack's spare float holds X"),
        l("a trigger's amount is another", V5, "a triggered ability's event amount ('that much'): no spare stack float"),
        l("a spell was cast for an alternative cost", V5, "the alternative cost (evoke, dash, Force of Will's pitch): no spare stack float"),
        l("a spell was cast from a graveyard with flashback", V5, "how a spell was cast (flashback: exiled as it leaves): no spare stack float"),
        l("a spell was cast with flashback", V5, "as 'a spell was cast from a graveyard with flashback' (Obj::flashback)"),
        l("a spell was cast from another zone", V5, "the zone a spell was cast from: no spare stack float"),
        l("a card exiled for delve", V5, "cards exiled for delve (Murktide Regent's counters): no spare stack float"),
        l("a trigger noted a permanent", V5, "permanents a trigger noted (Dack Fayden's emblem): no spare stack float (rare)"),
        l("divided damage is divided otherwise", V5, "divided damage (CR 601.2d): no spare stack float (rare)"),
        l("a ninjutsu creature will attack a planeswalker", V5, "no spare stack float (rare)"),
        l("a card is spliced onto a spell", V5, "no spare stack float (rare)"),
    ]
}

/// What a player can see of the match around the game (`Game::view_of` is
/// the game's): format, game number, results and who chose and played first,
/// wins and draws, what each player revealed in earlier games, sideboarding.
/// All public: the same for both players (`viewer` is for the signature's
/// symmetry with `view_of`).
pub fn match_view(a: &MtgAz, viewer: PlayerId) -> Vec<u64> {
    let m: &MatchState = &a.m;
    let mut v = vec![m.format as u64, m.game_no as u64, m.draws as u64, m.wins[0] as u64, m.wins[1] as u64];
    v.extend(m.results.iter().map(|&x| x as u64));
    v.extend(m.choosers.iter().map(|&x| x as u64));
    v.extend(m.starters.iter().map(|&x| x as u64));
    for p in 0..2 {
        v.extend(m.earlier[p].iter().copied());
    }
    v.push(m.sb.is_some() as u64);
    let _ = viewer;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_field_has_a_perturbation_and_every_perturbation_a_field() {
        let f = fields();
        let ps = perturbations();
        for x in f.iter().filter(|x| x.class == Class::Public) {
            assert!(ps.iter().any(|p| p.field == x.path), "public field {} has no perturbation", x.path);
        }
        for p in &ps {
            let x = f.iter().find(|x| x.path == p.field).unwrap_or_else(|| panic!("{}: unknown field {}", p.name, p.field));
            assert_eq!(x.class, Class::Public, "{}: {} is not public", p.name, p.field);
        }
        let mut names: Vec<&str> = ps.iter().map(|p| p.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ps.len(), "perturbation names are unique");
        for l in limits() {
            assert!(ps.iter().any(|p| p.name == l.name), "limit {} names no perturbation", l.name);
        }
    }
}
