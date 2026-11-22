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
//!    leaves out, each with its reason: by design, one (the graveyard's order
//!    no rule reads: the user's choice). Layout v4's 42 gaps (ENGINE.md
//!    58.10) are layout v5's fixes (60), v6's and v7's theirs (100.5, 124.2,
//!    125: since v7 every object is a token, so the perturbations change any
//!    object, and there is one perturbation per `Mark` variant,
//!    `MARK_PERTURBATIONS`): anything else that fails is a finding.

use crate::az::matches::{Format, MatchState};
use crate::az::MtgAz;
use crate::db::{types, AbilitySlot, Counter, Kw, KwSet, Mod, Prot};
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
        agenda: Public "the engine's continuation stack: what a player sees of it is the pending decision (the context block) and its tasks' picks in progress (the innermost holder's: the head's pick floats and links tokens; an enclosing task's: links tokens with their depth, ENGINE.md 142.10)",
        pending: Derived "the pending decision, computed from the agenda: its kind and decider are the context block's, its options the decider's",
        answer: Derived "the answer being applied: always empty between apply calls",
        page: Derived "the page of the pending decision when its permanents page (ENGINE.md 100.4.8): part of the agenda's continuation; the decider's options and context floats show it",
        waiting: Public "triggered abilities waiting to be put on the stack",
        extra_turns: Public "extra turns to come",
        combat_dmg: Derived "combat damage assigned so far in a damage step, read by the assigning player's next AssignDamage decision (the context's damage left)",
        next_ts: Derived "a counter for new timestamps (their order is the battlefield's)",
        next_stack_id: Derived "a counter for new stack ids",
        outcome: Public "the game's result",
        clouds: Hidden "a player's partial knowledge of hidden cards (ENGINE.md 10): the viewer's own, leak-tested",
        sightings_lost: Derived "a test's flag: the state was changed by hand, the sightings dropped (ENGINE.md 140.9)",
        ff: Derived "the forced find's state (ENGINE.md 180, a self-play training aid): its seed from the game's and a record of the spell's resolutions (public events), the same in every world; deliberately never encoded - neither player is told a resolution was forced (the user's decision)",
        sightings: Hidden "each player's sightings of the other's cards, in order, each blind or a search's find (ENGINE.md 140.9): the viewer's own knowledge, the same in all its worlds; read only by the hidden deck's deal",
        sight_at: Derived "the sightings' clock (ENGINE.md 140.9)",
        found: Hidden "the real game's record of its searches' finds for the sightings' order and weight (ENGINE.md 140.9), read only by the hidden deck's deal (`Game::sighted`); not the public fact (`finds_told`, ENGINE.md 223): a Doomsday pile's finds stay in it mixed with the graveyard cards the other player knew, whose places it no longer tells from theirs, and it lists them in the order of the searches - a pile's in its cards' definition order",
        finds_told: Public "which hidden cards everyone tells apart as their owner's search's finds (ENGINE.md 140.9, 223: a tutor's card put on top, a Doomsday pile of finds only, a find exiled face down, until seen, put into a hand or mixed with a card that is not one): only the set of their places is public, so the rows (layout v7's found tokens, ENGINE.md 142.12) and the view list them by place",
        tokens: Derived "a fast path: tokens created so far",
        first_token: Derived "a fast path: the first slot made as a token or a copy",
        batching: Derived "transient: a simultaneous move in progress",
        releasing: Derived "transient: during one simultaneous move",
        gy_arrivals: Derived "transient: during one simultaneous move (its cards put into graveyards, matched at its end)",
        ev_seq: Derived "event-group bookkeeping",
        batch_fired: Derived "event-group bookkeeping (\"one or more\" triggers once per group)",
        entering_cast: Derived "transient: set and cleared around one move",
        entering_evoked: Derived "transient: set and cleared around one move",
        observer: Derived "which world determinize made, for its key masking",
        keys: Derived "chance keys of the current apply: always empty between calls",
        forced: Derived "the search's forced chance outcomes",
        forced_at: Derived "the search's forced chance outcomes",
        decisions: Derived "a count of decisions answered (statistics)",
        no_auto: Derived "a flag of determinize's refresh",
        oracle: Derived "a flag of the payment search's copies",
        oracle_dead: Derived "a flag of the payment search's copies",
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
        leaving_to: Derived "transient: set and cleared around one leave event",
        leaving_choice: Derived "transient: set and cleared around one leave event",
        leaving_card: Derived "transient: set and cleared around one leave event",
        resolving_adventure: Derived "transient: set and cleared around one move",
        ts_group: Derived "event-group bookkeeping (CR 613.7m: which timestamps were given together)",
        held: Derived "the triggered abilities a pending 616.1 choice releases (ChooseReplacement, the decision's context): what its answers would trigger follows from the public event",
        entering_counters: Derived "transient: set and cleared around one move",
        entering_tapped: Derived "transient: set and cleared around one move",
        discard_entered: Derived "transient: Mox Diamond's answer, consumed by its entering",
        repl_decided: Derived "transient: a replacement choice made for the next move",
        route_choice: Derived "transient: the route of the cast about to begin (its decision's context)",
        cast_lock: Public "the total cost of a spell being cast, locked at its first mana ability during the cast (CR 601.2f-g): the cost floats of its 601.2g step's and delve's decisions (ENGINE.md 230)",
        pool_wild: Derived "transient: the payment search's copies only, (0, 0) in every state a decision is asked in (ENGINE.md 230.4)",
        entering_dash: Derived "transient: set and cleared around one move",
        entering_warp: Derived "transient: set and cleared around one move",
        miracle_cast: Derived "transient: the card a miracle trigger is casting (its decisions' context)",
        entering_face: Derived "transient: set and cleared around one move",
        day: Public "day or night (CR 731)",
        done_once: Public "optional \"only once each turn\" actions done this turn",
        discarded: Public "cards discarded this turn (mayhem, CR 702.187b)",
        void_turn: Public "void: a nonland permanent left the battlefield or a spell was warped this turn (ENGINE.md 99.4)",
        temp_triggers: Public "triggered abilities a player has until their next turn",
        gy_exile: Public "spells that are exiled instead of being put into a graveyard",
        copies: Public "copy effects on permanents (CR 707)",
        next_turn_controller: Public "who will control each player during their next turn (CR 722.1; ENGINE.md 99.14)",
        turn_controller: Public "who controls the active player during this turn (CR 722.1; ENGINE.md 99.14)",
        copy_objs: Derived "which slots are copies (the copy flags of permanents and spells)",
        lki_copy: Derived "last known information (CR 608.2h)",
        lki_attached: Derived "last known information (CR 608.2h)",
        lki_damage: Derived "last known information (CR 608.2h)",
        lki_toughness: Derived "last known information (CR 608.2h)",
        ref_lki: Derived "last known identities of gone objects (CR 400.7, 608.2h): what a stale reference shows, through the references the encoding writes (ENGINE.md 156)",
        lki_power: Derived "last known information (CR 608.2h)",
        loyalty: Public "loyalty abilities activated this turn (CR 606.3)",
        pool_restr: Public "the part of a pool's mana with a spending restriction other than the Workshop's (CR 106.6; ENGINE.md 99.12)",
        moon_game: Derived "fast path: a card of the game says nonbasic lands are Mountains",
        lands_entered: Public "the lands that entered the battlefield under each player's control this turn (Bioengineered Future; ENGINE.md 99.12)",
        spell_lki: Derived "last known information (CR 608.2h)",
        entering_copy: Derived "transient: set and cleared around one move",
        entering_times: Derived "transient: set and cleared around one move",
        enter_note: Derived "transient: set and cleared around one move",
        defer_enters: Derived "transient: an enters event held back for one choice",
        entries: Derived "the Enters events of a simultaneous entry, held while the choices made as its permanents enter are asked (their decisions' context): what they trigger follows from the public entry",
        pile_moving: Derived "transient: between ops",
        pile_shuffles: Derived "transient: between ops",
        prepared: Public "prepared permanents and their prepare spell copies (CR 722.3)",
        loop_taken: Derived "records of this turn's loop actions (loops.rs; CR 732.2a, MTR 4.4): hashes of views of the state as their takers' deciders saw it, with the activations' decisions (action ids offered and taken), which prune a completed action only at an identical view of that decider (determinize keeps the viewer's own)",
        loop_open: Derived "a loop action's activation in progress: its record so far (the hash of its taker's decider's view, its decisions' action ids), dropped by determinize for any other viewer",
        seen: Derived "fast paths: rare rules some card of the game has",
        initiative: Public "the initiative (CR 725)",
        dungeons: Derived "each player's dungeon: its fields are classified under Dungeon",
        rules_srcs: Derived "objects in no zone naming the initiative's and the monarch's triggers' source",
        choosing_for: Derived "transient: the item whose modes are chosen (0 between tasks)",
        marks: Public "rare per-object facts (exhausted, a chosen colour, exerted, can't be countered, ...)",
        w3: Derived "EoE workstream W3's state: its fields are classified under W3State",
        w6: Derived "EoE workstream W6's state: its fields are classified under W6State",
        known_pools: Derived "with hidden decks, the pool names each player knows of the other's: set by the match, fixed for the game",
        log: Derived "the human-readable log (tests and the viewer)",
        peeks: Hidden "which entries of a player's library cloud name a card its owner never saw (ENGINE.md 192): the viewer's own knowledge, redrawn in the owner's worlds",
        log_mark: Derived "the log's length when the last answer began to run: the spectator view's start (ENGINE.md 191)",
        hand_looks: Derived "the public looks at a card of another player's hand the last apply made (ENGINE.md 206): for the fuzz's private-key check, nothing in play reads it",
    });
    {
        use crate::eoe_w3::W3State;
        classify!(v, W3State {
            no_regen: Derived "transient: set and cleared around one destruction (\"it can't be regenerated\")",
            granted: Derived "a fast path: some permanent may have a granted triggered ability (Game.marks)",
            seen: Derived "fast paths: rare rules some card of the game has (as Game.seen)",
            lki_host: Derived "last known information (CR 608.2h): what a gone Aura enchanted",
            lki_loss: Derived "last known information (CR 603.10a): when a gone permanent lost its abilities",
            entering_spent: Derived "transient: set and taken around one move",
            phased: Public "phased-out permanents (CR 702.26b): not encoded (a limit)",
            indirect: Public "which phased-out permanents phased out with what they are attached to (CR 702.26e)",
        });
    }
    {
        use crate::eoe_w6::W6State;
        classify!(v, W6State {
            seen: Derived "fast paths: rare rules some card of the game has (as Game.seen)",
            entering: Derived "transient: set and cleared around one permanent's entry",
            actor: Derived "transient: set and restored around one op",
            control: Public "the control-changing effects behind permanents' controllers (CR 613.1b; EoE W6: games with an Aura's control effect): which ends when",
            syncing: Derived "transient: set while the controllers are worked out",
            tokens_turn: Public "the turn each player last created tokens (CR 111.2; EoE W6: Moonlit Meditation's 'the first time ... each turn')",
            token_question: Derived "transient: set by a token creation, taken as its instruction returns",
            token_asking: Derived "the pending decision is the token replacement's question (Game.pending, public)",
            token_answer: Derived "transient: set and cleared around one op",
            token_frame: Derived "transient: set and cleared around one op",
            deferred_since: Derived "transient: the entering permanents waiting for their devour choice (Game.agenda), with their entries' first timestamps",
        });
    }
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
        life_gained: Public "how much life gained this turn",
        artifact_entered: Public "an artifact entered the battlefield under their control this turn (ENGINE.md 99.11)",
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
        uncounterable_next: Public "\"the next spell you cast this turn can't be countered\" effects waiting (Mistrise Village; ENGINE.md 123)",
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
        struck_first: Public "had first strike or double strike as this combat's first combat damage step began (CR 510.4; ENGINE.md 151.5)",
        known: Public "which players know a hidden card: who knows is public (a look, a reveal, a put-back, a shuffle are seen; ENGINE.md 72), what it is is not (leak-tested, ENGINE.md 10)",
        attached: Public "what it is attached to (CR 301.5, 303.4)",
        evoked: Public "it entered evoked (CR 702.74a)",
        flashback: Public "a spell cast with flashback: exiled when it leaves the stack (CR 702.34a)",
        escaped: Public "it entered as a spell cast with escape (CR 702.138c)",
        chosen_player: Public "the player chosen as it entered (CR 614.12)",
        monstrous: Public "CR 701.37b",
        attacks: Public "what it attacks (a planeswalker, CR 506.3)",
        entered: Public "the turn it entered (\"entered this turn\")",
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
        after_combat: Public "where the turn goes on after combat phases added to another phase (CR 500.8)",
        chooser: Public "who chose who plays first (CR 103.1)",
        had_attackers: Public "a creature was declared as an attacker or put onto the battlefield attacking in this declare attackers step (CR 508.8; ENGINE.md 151.4)",
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
        earlier_by_game: Public "the same by game (ENGINE.md 142.8: game 2's reveals apart from game 1's, the my_revealed / opp_revealed tokens' game bits)",
        sb: Derived "sideboarding between games: encoded by encode_sideboarding, not a game position",
        rng: Hidden "seeds the later games: determinize reseeds it",
        priors: Hidden "the prior of each player's unknown first picks (ENGINE.md 36.11)",
        deck_priors: Derived "hidden decklists: the prior of each player's deck (ENGINE.md 102), a setting of the match, the same in every world",
        deal: Derived "hidden decklists: the K-sample deal's setting and memo (ENGINE.md 102.8), a function of the match's salt and what the viewer knows",
        choice: Hidden "the sealed deck choice before the game (ENGINE.md 107.2): each seat's candidates and choice are its own, encoded by encode_deck_choice (who decides, the number of candidates); determinize deals the other seat's",
        how: Derived "how each sealed deck was made (ENGINE.md 107): the game log's, never encoded; None in a world for the other seat",
        constructed_bo3: Derived "a constructed best-of-three (ENGINE.md 120): the match's format setting, the same in every world",
        game_episodes: Derived "the spec's one episode per game (ENGINE.md 121): a setting of the match, the same in every world",
        between: Derived "one episode per game (ENGINE.md 121): a game ended, the match did not - a terminal state, never encoded for a decision",
        keep: Hidden "the seats that keep their decks at every sideboarding (ENGINE.md 130, an evaluation yardstick): a seat's own policy, off in the other player's worlds (on in them with keep_known)",
        keep_known: Derived "the yardstick's informed arm (ENGINE.md 141): a setting of the match, the same in every world, never encoded - the other seat's worlds deal a keeper its real deck (a prior that knows its policy)",
        post_board: Derived "game 1 was not played (ENGINE.md 130, start_game 2): a setting of the match, the same in every world, never encoded - a real game 2 encodes as a synthetic one",
        dealt: Public "the dealt seat (ENGINE.md 154, dealt_decks): a rule of the match both players know, the same in every world, encoded per player (g6::DEALT)",
        dealt_both: Derived "both seats are dealt (ENGINE.md 168.1, dealt_decks per_seat): a setting of the match from its spec and seed, the same in every world; it reaches the encoding through the per-player g6::DEALT floats, which the dealt seat's perturbation checks (and that perturbation turns a both-dealt match into one dealt seat)",
        table: Hidden "the seats that sideboard by the choice table (ENGINE.md 168.3): a seat's own policy, as keep; never encoded, the other player's worlds deal its 60 from the constructed prior",
        glog: Derived "the live match's game log record (ENGINE.md 168.2): never read by the rules, the encodings or the search; a clone carries none",
        dealt_seed: Hidden "seeds the dealt seat's later deals (ENGINE.md 154): chance not yet happened, determinize reseeds it in every world",
        test_panic: Derived "a test fault (ENGINE.md 172, MTGZ_TEST_PANIC): None in every real match; never read by the rules, the encodings or the search",
        pairing: Derived "a metagame match's pairing (ENGINE.md 176): which of the spec's 75s each seat plays, from the spec and the match's seed, the same in every world; never read by the rules, the encodings or the search - the 75s it names are the pools, public (OPP_POOL)",
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

impl Perturbation {
    /// `apply` and, when it changed the state (by hand, outside the rules), drop the sightings that no longer
    /// describe it (`Game::lose_sightings`, ENGINE.md 140.9). The chance keys a change by hand emitted belong to no
    /// transition: the buffer is empty between transitions (`keys`, derived above), as `refresh_pending`'s check
    /// "refreshing a decision revealed something" reads it (the turn controlled by hand, `sync_controller_view`'s
    /// looks: ENGINE.md 210).
    pub fn run(&self, a: &mut MtgAz, r: &mut Rng) -> Option<String> {
        let out = (self.apply)(a, r);
        if out.is_some() {
            a.g.lose_sightings();
        }
        a.g.keys.clear();
        out
    }
}

/// A public change the encoding leaves out (`name`: the perturbation's), and
/// why: by design, or a gap that only a new layout can close.
pub struct Limit {
    pub name: &'static str,
    pub kind: LimitKind,
    pub why: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// A deliberate limit of the encoding (ENGINE.md 58.9).
    Design,
    /// A genuine gap the layout has no room for, for its next version (layout
    /// v4's 42, ENGINE.md 58.10, are encoded by layout v5, 60; the EoE port's
    /// and workstream C's by layout v6, 100.5). The procedure for a new one:
    /// ENGINE.md 100.5.
    NextLayout,
}

fn pick<T: Copy>(v: &[T], r: &mut Rng) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[r.below(v.len() as u32) as usize])
    }
}

/// The permanents (each player's, timestamp order: every one encoded since
/// layout v7, past slot 31 as a tail token, ENGINE.md 125), but face-down
/// ones (their card is hidden).
fn slots(g: &Game) -> Vec<CardIdx> {
    let mut v: Vec<CardIdx> = g.permanents_of(0).collect();
    v.extend(g.permanents_of(1));
    v.retain(|&c| g.objs[c.i()].def != g.db.face_down_def());
    v
}

fn slot_creatures(g: &Game) -> Vec<CardIdx> {
    slots(g).into_iter().filter(|&c| g.is_creature(c)).collect()
}

/// The stack items (every one encoded since layout v7, past the top 8 as tail
/// tokens, ENGINE.md 125), as indices into `Game::stack`, top first.
fn stack_slots(g: &Game) -> Vec<usize> {
    (0..g.stack.len()).rev().collect()
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

/// The first game played whose chooser and starter the match block encodes: game 1, but in a
/// constructed best-of-three game 2 (ENGINE.md 130: game 1's are not encoded); None before it.
fn encoded_game(a: &MtgAz) -> Option<usize> {
    let i = a.m.constructed_bo3 as usize;
    (a.m.results[i] != 0).then_some(i)
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
    o.inc = o.inc.checked_add(1).expect("an incarnation below u32::MAX");
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
fn characteristics(g: &Game, c: CardIdx) -> (u16, u8, (crate::num::Wide, crate::num::Wide), u32, crate::layers::Flags, crate::db::Prot, bool) {
    (g.types_of(c), g.colors_of(c), g.pt(c), g.keywords_of(c).0, g.flags_of(c), g.protection_of(c), g.lost_abilities(c))
}

/// An until-end-of-turn effect `m` on permanent `c`.
fn effect(g: &mut Game, c: CardIdx, m: Mod) -> Option<String> {
    let p = g.objs[c.i()].controller;
    effect_until(g, c, m, Until::EndOfTurn, p)
}

/// An effect `m` on permanent `c` lasting `until`, controlled by `controller` ("you" of "until your
/// next turn").
fn effect_until(g: &mut Game, c: CardIdx, m: Mod, until: Until, controller: PlayerId) -> Option<String> {
    let r = g.obj_ref(c);
    let what = format!("{m:?} on {} ({until:?}, P{controller}'s)", g.name(c));
    g.add_effect(Affected::Obj(r), m, until, controller);
    Some(what)
}

/// "For as long as <another permanent> remains on the battlefield" (CR 611.2b; Tishana's
/// Tidebinder): the first permanent other than `c`.
fn while_other(g: &Game, c: CardIdx) -> Option<Until> {
    let o = slots(g).into_iter().find(|&x| x != c)?;
    Some(Until::WhileOnBattlefield(g.obj_ref(o)))
}

/// A creature without protection (the gate's protection family, ENGINE.md 142: every member on
/// one candidate list) gains protection `prot` until end of turn.
fn protection(a: &mut MtgAz, r: &mut Rng, prot: Prot) -> Option<String> {
    let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.protection_of(c).is_empty()).collect();
    let c = pick(&cs, r)?;
    effect(&mut a.g, c, Mod::Protection(prot))
}

/// A counter of kind `k` on permanent `c`, by hand (no events). A +1/+1 counter on a permanent
/// with a -1/-1 counter removes one instead, and the other way (the state-based action of CR
/// 704.5q: a decision never sees both on one permanent; ENGINE.md 142).
fn counter(g: &mut Game, c: CardIdx, k: Counter) -> Option<String> {
    let other = match k {
        Counter::P1P1 => Some(Counter::M1M1),
        Counter::M1M1 => Some(Counter::P1P1),
        _ => None,
    };
    if let Some(i) = other.and_then(|o| g.counters.iter().position(|x| x.0 == c && x.1 == o && x.2 > 0)) {
        g.counters[i].2 -= 1;
        if g.counters[i].2 == 0 {
            g.counters.remove(i);
        }
        return Some(format!("a {k:?} counter on {} (one {:?} counter fewer, CR 704.5q)", g.name(c), other.unwrap()));
    }
    match g.counters.iter_mut().find(|x| x.0 == c && x.1 == k) {
        Some(x) => x.2 += 1,
        None => g.counters.push((c, k, 1)),
    }
    Some(format!("a {k:?} counter on {}", g.name(c)))
}

/// A phased-out permanent (CR 702.26b; EoE W3), with the definition and
/// owner of the first permanent in the encoded slots; `indirect`: it phased
/// out with what it is attached to (702.26e).
fn phased_out(g: &mut Game, indirect: bool) -> Option<String> {
    let c = *slots(g).first()?;
    let (def, owner) = (g.objs[c.i()].def, g.objs[c.i()].owner);
    let x = g.add_card(def, owner, Zone::PhasedOut);
    g.objs[x.i()].zone = Zone::PhasedOut;
    g.w3.phased.push(x);
    if indirect {
        g.w3.indirect.push(x);
    }
    Some(format!("a phased-out {}", g.name(x)))
}

fn mark(g: &mut Game, c: CardIdx, m: crate::game::Mark) -> Option<String> {
    let r = g.obj_ref(c);
    if g.marks.contains(&(r, m)) {
        return None;
    }
    g.marks.push((r, m));
    Some(format!("{m:?} on {}", g.name(c)))
}

/// The mana spent to cast `c` made another amount (CR 601.2h: an object has one amount, its one
/// `ManaSpent` mark, which the rules read: `mana_spent_of`): the mark set to `n` (`n + 1` where it
/// was `n`), or added. (A second mark would change the view and no fact the rules read: on the
/// EoE table every spell has one, `W6Db::mana_spent_all`; ENGINE.md 105.)
fn mana_spent(g: &mut Game, c: CardIdx, n: u32) -> Option<String> {
    use crate::game::Mark;
    let r = g.obj_ref(c);
    match g.marks.iter().position(|&(o, m)| o == r && matches!(m, Mark::ManaSpent(_))) {
        Some(i) => {
            let Mark::ManaSpent(old) = g.marks[i].1 else { unreachable!() };
            let new = if old == n { n + 1 } else { n };
            g.marks[i].1 = Mark::ManaSpent(new);
            Some(format!("ManaSpent {old} -> {new} on {}", g.name(c)))
        }
        None => mark(g, c, Mark::ManaSpent(n)),
    }
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
                    // (A group's rules effect ends whatever it applies to now: it binds the
                    // permanents that match later too, CR 611.2c; ENGINE.md 142.)
                    Affected::Group(_) => !g.effects.iter().enumerate().any(|(j, e)| j != i && e.on == g.effects[i].on && e.m == g.effects[i].m),
                })
                .collect();
            let i = pick(&live, r)?;
            let Affected::Obj(o) = a.g.effects[i].on.clone() else {
                let e = a.g.effects.remove(i);
                a.g.effects_changed();
                return Some(format!("{:?} on {:?}", e.m, e.on));
            };
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
            // (One that can block now, as "can't be blocked" below: a creature that already can't
            // block would not change; ENGINE.md 102.7 found one in a hidden-decklist position.)
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.flags_of(c).cant_block).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::CantBlock)
        }),
        p!("a creature can't be blocked", "Game.effects", |a, r| {
            // (One that can be blocked now: another such effect, or its own
            // ability, would make this one change nothing.)
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.flags_of(c).cant_be_blocked).collect();
            let c = pick(&cs, r)?;
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
            // (One without protection: the protection family's, ENGINE.md 142.)
            protection(a, r, Prot { colors: 1 << 3, ..Default::default() })
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
                turn: a.g.turn.number,
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
            // (A card its owner may not play yet: another permission to play
            // a card they may play changes nothing they may do.)
            let cs: Vec<CardIdx> = a.g.exile.iter().copied().filter(|&c| a.g.exile_permission(a.g.objs[c.i()].owner, c).is_none()).collect();
            let c = pick(&cs, r)?;
            let card = a.g.obj_ref(c);
            let player = a.g.objs[c.i()].owner;
            let kind = crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, only_if: crate::game::PlayIf::Always };
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
        // The total cost of the spell a casting decision is about, locked (CR 601.2f-g; ENGINE.md 230): one more
        // generic mana, read by the decision's cost floats (its 601.2g step's, delve's, convoke's).
        p!("the total cost of the spell being paid for is locked one higher", "Game.cast_lock", |a, _r| {
            let id = match a.g.agenda.last()? {
                crate::flow::Task::CastMana { id, .. } | crate::flow::Task::Delve { id } | crate::flow::Task::ConvokePay { id, .. } => *id,
                _ => return None,
            };
            let i = a.g.item_index(id)?;
            let StackKind::Spell { card } = a.g.stack[i].kind else { return None };
            let mut cost = a.g.total_mana(card, &a.g.stack[i]);
            cost.generic += 1;
            a.g.cast_lock.retain(|x| x.0 != id);
            a.g.cast_lock.push((id, cost));
            Some(format!("{} costs {{1}} more", a.g.name(card)))
        }),
        p!("a permanent becomes a copy of another", "Game.copies", |a, r| {
            let ss = slots(&a.g);
            let c = pick(&ss, r)?;
            let others: Vec<DefId> = ss.iter().map(|&x| a.g.objs[x.i()].def).filter(|&d| d != a.g.objs[c.i()].def).collect();
            let d = pick(&others, r)?;
            a.g.add_copy_effect(c, crate::copies::CopyVals { def: d, exc: Default::default(), dfc: true }, Until::EndOfTurn);
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
        // EoE W4 (ENGINE.md 99.14): controlling another player (CR 722). (No
        // draw from the gate's random stream.)
        p!("a player will control the other's next turn", "Game.next_turn_controller", |a, _r| {
            let q = opp(a.g.turn.active);
            let x = &mut a.g.next_turn_controller[q as usize];
            *x = if x.is_some() { None } else { Some(a.g.turn.active) };
            Some(format!("P{q}: {:?}", a.g.next_turn_controller[q as usize]))
        }),
        // (With what CR 722.3 makes of it at once: the controller sees the
        // controlled player's hand, `sync_controller_view`, as at the end of
        // every transition of a controlled turn; ENGINE.md 100.5.)
        p!("the active player is controlled this turn", "Game.turn_controller", |a, _r| {
            let q = opp(a.g.turn.active);
            a.g.turn_controller = if a.g.turn_controller.is_some() { None } else { Some(q) };
            a.g.sync_controller_view();
            Some(format!("{:?}", a.g.turn_controller))
        }),
        // (Rare: mana in a pool at a sampled decision; draws nothing from
        // the shared stream: the first player with mana, its first type.)
        p!(rare "mana in a pool is restricted to activating abilities of lands", "Game.pool_restr", |a, _| {
            let (p, t) = (0..2u8).find_map(|p| {
                let pool = &a.g.players[p as usize].pool;
                let held = |t: usize| a.g.pool_restr.iter().filter(|k| k.0 == p && k.1 as usize == t).map(|k| k.3).sum::<u8>() + if t == 5 { pool.art } else { 0 };
                (0..6).find(|&t| pool.amt[t] > held(t)).map(|t| (p, t))
            })?;
            a.g.pool_restr.push((p, t as u8, crate::db::SPEND_LAND_ABILITY, 1));
            Some(format!("P{p}: one mana of type {t}"))
        }),
        // ENGINE.md 124.2 (Cavern of Souls). (Rare as the one above, and
        // drawing nothing from the shared stream: one Merfolk mana with the
        // rider, of a colour: such mana is one mana of any colour.)
        p!(rare "mana in a pool is restricted to creature spells of a chosen type", "Game.pool_restr", |a, _| {
            let (p, t) = (0..2u8).find_map(|p| {
                let pool = &a.g.players[p as usize].pool;
                let held = |t: usize| a.g.pool_restr.iter().filter(|k| k.0 == p && k.1 as usize == t).map(|k| k.3).sum::<u8>();
                (0..5).find(|&t| pool.amt[t] > held(t) && (t != crate::mana::R || pool.amt[t] - held(t) > pool.haste)).map(|t| (p, t))
            })?;
            let merfolk = crate::doomsday_b::CREATURE_TYPES_CR.iter().position(|&n| n == "Merfolk").expect("Merfolk") as u16;
            let only = crate::doomsday_b::with_type(crate::db::MANA_UNCOUNTERABLE, Some(merfolk));
            a.g.pool_restr.push((p, t as u8, only, 1));
            Some(format!("P{p}: one mana of type {t}, Merfolk spells only"))
        }),
        // (Draws nothing from the shared stream: the active player's count.)
        p!("a land entered the battlefield this turn", "Game.lands_entered", |a, _| {
            let (t, p) = (a.g.turn.number, a.g.turn.active as usize);
            if a.g.lands_entered.0 != t {
                a.g.lands_entered = (t, [0; 2]);
            }
            a.g.lands_entered.1[p] += 1;
            Some(format!("P{p}: {}", a.g.lands_entered.1[p]))
        }),
        p!("void holds (a nonland permanent left the battlefield this turn)", "Game.void_turn", |a, _| {
            let t = a.g.turn.number;
            a.g.void_turn = if a.g.void_turn == t { 0 } else { t };
            Some(format!("void turn {}", a.g.void_turn))
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
        // ENGINE.md 124.2 (Cavern of Souls).
        p!("a creature type was chosen as a permanent entered", "Game.marks", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let t = r.below(crate::doomsday_b::CREATURE_TYPES_CR.len() as u32) as u16;
            if a.g.chosen_creature_type(c) == Some(t) {
                return None;
            }
            a.g.set_chosen_creature_type(c, t);
            Some(format!("{} chose {}", a.g.name(c), crate::doomsday_b::CREATURE_TYPES_CR[t as usize]))
        }),
        p!("a creature was exerted", "Game.marks", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            mark(&mut a.g, c, crate::game::Mark::Exerted)
        }),
        // EoE's "if that creature was cast for its warp cost" (ENGINE.md
        // 99.8). (No draw from the gate's random stream: a draw would shift
        // every later perturbation's, as lane D found; the first creature in
        // the encoded slots.)
        p!("a creature was cast for its warp cost", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::WarpCast)
        }),
        // EoE W3 (ENGINE.md 99.13): no draw from the gate's random stream
        // (the first object in the encoded slots, as the cross-lane pass's).
        p!("a creature has a regeneration shield", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::RegenShield)
        }),
        // ENGINE.md 100.5: a triggered ability an effect gave a permanent
        // (`Mark::Granted`, CR 613.1f; EoE W3). (The first permanent in the
        // encoded slots whose definition has one - slot 0 names it: a state the
        // engine can reach, its searches resolving events through it, ENGINE.md
        // 228; no draw from the gate's random stream.)
        p!("a permanent gained a triggered ability", "Game.marks", |a, _r| {
            let c = *slots(&a.g).iter().find(|&&c| !a.g.def_of(c).triggers.is_empty())?;
            let (def, ts) = (a.g.objs[c.i()].def, a.g.next_ts);
            a.g.w3.granted = true;
            mark(&mut a.g, c, crate::game::Mark::Granted { def, slot: 0, this_turn: true, ts })
        }),
        p!("the mana spent to cast a permanent", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mana_spent(&mut a.g, c, 3)
        }),
        p!("a permanent is phased out", "W3State.phased", |a, _r| phased_out(&mut a.g, false)),
        p!("a phased-out permanent phased out indirectly", "W3State.indirect", |a, _r| phased_out(&mut a.g, true)),
        // EoE W4 (ENGINE.md 99.14): a copy's "except it's legendary" (CR
        // 707.9b; Adagia's token copies). On a copy effect that is already
        // there (adding one would set the encoded copy flag too); no draw
        // from the gate's random stream. Rare: the gate's positions hold a
        // copy effect only when its heuristic games made one (11 of the
        // drafted game's positions with the M7c heuristic, none since
        // ENGINE.md 103), as its sibling "a copy has an ability".
        p!(rare "a copy is legendary by its exception", "Game.copies", |a, _r| {
            let ss = slots(&a.g);
            let db = a.g.db;
            let legendary = crate::db::supertypes::LEGENDARY;
            let fx = a
                .g
                .copies
                .iter_mut()
                .find(|x| ss.contains(&x.obj.idx) && (x.vals.exc.supers | db.def(x.vals.def).supertypes) & legendary == 0)?;
            fx.vals.exc.supers |= legendary;
            Some(db.def(fx.vals.def).name.clone())
        }),
        p!("a spell can't be countered", "Game.marks", |a, r| {
            // (Not a spell that already can't be: Abrupt Decay, ENGINE.md 86.5.)
            let spells: Vec<(usize, CardIdx)> = stack_spells(&a.g).into_iter().filter(|&(_, c)| !a.g.spell_uncounterable(c)).collect();
            let (_, c) = pick(&spells, r)?;
            mark(&mut a.g, c, crate::game::Mark::Uncounterable)
        }),
        // A graveyard card is chosen for delve (CR 702.66a, 601.2h): a graveyard card chosen,
        // still in the graveyard until the payment ends (ENGINE.md 51.6, 65).
        p!("a graveyard card is chosen for delve", "Game.marks", |a, r| {
            let cards: Vec<CardIdx> = (0..2).flat_map(|p| a.g.players[p].graveyard.clone()).filter(|&c| !a.g.is_token(c)).collect();
            let c = pick(&cards, r)?;
            mark(&mut a.g, c, crate::game::Mark::DelveExile)
        }),
        // ---- Game.marks: every other variant (layout v7, ENGINE.md 125: the gate's G1, one
        // perturbation per `Mark` variant; `MARK_PERTURBATIONS` lists them). A permanent's in its
        // slot (`perm::*`), a spell's in its stack slot (`stack::*`), a relation to another object
        // or an ability a `links` token. No draw from the gate's random stream: the first object
        // that can take it.
        p!("a permanent entered with a big kicker count and X", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::Entered { times: 300, x: 70 })
        }),
        p!("Containment Priest's replacement waits for a copy choice", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::PriestPending)
        }),
        p!("a spell's converge colours spent so far", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::ConvergePending(0b00101))
        }),
        p!("a spell's converge colours spent", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::Converge(0b10010))
        }),
        p!("a permanent has a granted leave-the-battlefield trigger", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            let def = a.g.objs[c.i()].def;
            mark(&mut a.g, c, crate::game::Mark::LeaveTrigger { def, slot: 0, exile: false })
        }),
        p!("a spell will give its permanent a trigger as it enters", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            let def = a.g.objs[c.i()].def;
            mark(&mut a.g, c, crate::game::Mark::GrantOnEnter { def, slot: 0 })
        }),
        p!("a permanent's once-each-turn permission was used this turn", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            let t = a.g.turn.number as u16;
            mark(&mut a.g, c, crate::game::Mark::UsedTurn(t))
        }),
        p!("a spell's X is capped by a permission's mana value", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::MvCap(3))
        }),
        p!("a permanent was harnessed", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::Harnessed)
        }),
        p!("a permanent boasted this turn", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            let t = a.g.turn.number as u16;
            mark(&mut a.g, c, crate::game::Mark::Boasted(t))
        }),
        p!("a permanent entered kicked with an X", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::KickerX(5))
        }),
        p!("a creature became saddled this turn", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            let t = a.g.turn.number as u16;
            mark(&mut a.g, c, crate::game::Mark::Saddled(t))
        }),
        p!("a permanent's once-chosen modes", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::ModesUsed(0b101))
        }),
        p!("odd or even was chosen as a permanent entered", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::Parity(1))
        }),
        p!("an Aura reanimated a creature", "Game.marks", |a, _r| {
            let ss = slots(&a.g);
            let (&c, &other) = (ss.first()?, ss.get(1)?);
            let o = a.g.obj_ref(other);
            mark(&mut a.g, c, crate::game::Mark::Reanimated(o))
        }),
        p!("a spell was cast when a sorcery couldn't have been", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::NotSorceryTiming)
        }),
        p!("a granted trigger triggered this turn for a permanent", "Game.marks", |a, _r| {
            let ss = slots(&a.g);
            let (&c, &src) = (ss.first()?, ss.last()?);
            let (def, turn, src) = (a.g.objs[src.i()].def, a.g.turn.number as u16, a.g.obj_ref(src));
            mark(&mut a.g, c, crate::game::Mark::Triggered { def, slot: 0, turn, n: 1, src })
        }),
        p!("a permanent looked at the opponent's hand as it entered", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::LookedHand)
        }),
        p!("a card name was chosen as a permanent entered", "Game.marks", |a, r| {
            let ss = slots(&a.g);
            let c = *ss.first()?;
            // (A card of the game's, or a name of none: `NO_NAME`, ENGINE.md 125.)
            let d = if r.below(4) == 0 { crate::game::NO_NAME } else { a.g.objs[pick(&ss, r)?.i()].def };
            if a.g.chosen_name(a.g.obj_ref(c)) == Some(d) {
                return None;
            }
            a.g.marks.retain(|(o, m)| !(o.idx == c && matches!(m, crate::game::Mark::ChosenName(_))));
            mark(&mut a.g, c, crate::game::Mark::ChosenName(d))
        }),
        p!("mana that gives haste was spent on a spell", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::HasteMana)
        }),
        p!("a mana source is chosen to be exiled by delve", "Game.marks", |a, _r| {
            let c = *slots(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::DelveLate)
        }),
        p!("a hand card is promised to a delve", "Game.marks", |a, r| {
            let p = player(r);
            let c = *a.g.players[p as usize].hand.first()?;
            // (A promised card is revealed: the cost discards it, ENGINE.md 100.4.9.)
            a.g.learn(opp(p), c);
            mark(&mut a.g, c, crate::game::Mark::DelvePromised)
        }),
        p!("a spell's delve exiled a card to pay", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            let e = *a.g.exile.first()?;
            let o = a.g.obj_ref(e);
            mark(&mut a.g, c, crate::game::Mark::ExiledToPay(o))
        }),
        p!("a permanent was dealt damage this turn", "Game.marks", |a, _r| {
            let c = *slot_creatures(&a.g).first()?;
            let t = a.g.turn.number as u16;
            if a.g.marks.iter().any(|&(o, m)| o.idx == c && matches!(m, crate::game::Mark::DealtDamage(_))) {
                return None;
            }
            mark(&mut a.g, c, crate::game::Mark::DealtDamage(t))
        }),
        p!("a spell was cast from its caster's hand free", "Game.marks", |a, _r| {
            let (_, c) = *stack_spells(&a.g).first()?;
            mark(&mut a.g, c, crate::game::Mark::FreeFromHand)
        }),
        // ---- Layout v7's other new state (ENGINE.md 125) ----
        p!("two creature cards of a graveyard swap places", "Player.graveyard", |a, r| {
            // (The order a positional reader reads, Shallow Grave's: `graveyard_order` tokens; a
            // table without one has nothing to change.)
            let readers = crate::lower::graveyard_order::readers(a.g.db);
            let f = &readers.first()?.filter;
            let p = player(r) as usize;
            let gy = &a.g.players[p].graveyard;
            let m: Vec<usize> = (0..gy.len()).filter(|&i| crate::lower::graveyard_order::matches(a.g.db, f, a.g.def_of(gy[i]))).collect();
            let pairs: Vec<(usize, usize)> = m
                .iter()
                .flat_map(|&i| m.iter().filter(move |&&j| j > i).map(move |&j| (i, j)))
                .filter(|&(i, j)| a.g.objs[gy[i].i()].def != a.g.objs[gy[j].i()].def)
                .collect();
            let (i, j) = pick(&pairs, r)?;
            a.g.players[p].graveyard.swap(i, j);
            Some(format!("P{p}'s {i} and {j}"))
        }),
        p!("a stack ability's source is another permanent", "StackItem.kind", |a, _r| {
            let g = &mut a.g;
            let i = stack_slots(g).into_iter().find(|&i| matches!(g.stack[i].kind, StackKind::Ability { source, .. } if g.live(source) && g.objs[source.idx.i()].zone == Zone::Battlefield))?;
            let StackKind::Ability { source, .. } = g.stack[i].kind else { return None };
            let other = g.battlefield.iter().copied().find(|&c| c != source.idx && g.objs[c.i()].def == g.objs[source.idx.i()].def).or_else(|| g.battlefield.iter().copied().find(|&c| c != source.idx))?;
            let o = g.obj_ref(other);
            if let StackKind::Ability { source, .. } = &mut g.stack[i].kind {
                *source = o;
            }
            Some(g.name(other).to_string())
        }),
        p!("a trigger's event names another object", "StackItem.ev", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let o = a.g.obj_ref(c);
            item(a, r, move |s| {
                if !matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Trigger(_))) || s.ev.obj == Some(o) {
                    return None;
                }
                s.ev.obj = Some(o);
                Some("another event object".into())
            })
        }),
        p!("a trigger's event names the other player", "StackItem.ev", |a, r| item(a, r, |s| {
            if !matches!(s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, AbilitySlot::Trigger(_))) {
                return None;
            }
            s.ev.player = Some(s.ev.player.map_or(0, opp));
            Some(format!("{:?}", s.ev.player))
        })),
        p!(rare "a stack item's third target is another", "StackItem.targets", |a, _r| {
            let g = &mut a.g;
            let i = stack_slots(g).into_iter().find(|&i| g.stack[i].targets.len() >= 3)?;
            let t = match g.stack[i].targets[2].1 {
                Target::Player(p) => Target::Player(opp(p)),
                Target::Obj(o) => {
                    let other = g.battlefield.iter().copied().find(|&c| c != o.idx)?;
                    Target::Obj(g.obj_ref(other))
                }
                Target::Stack(_) => return None,
            };
            g.stack[i].targets[2].1 = t;
            Some("the third target".into())
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
        // (While a creature is attacking, the record adds nothing: it was
        // declared or put onto the battlefield attacking.)
        p!("the declare blockers step follows an attacker that left combat", "Turn.had_attackers", |a, _| {
            if a.g.battlefield.iter().any(|&c| a.g.objs[c.i()].attacking) {
                return None;
            }
            a.g.turn.had_attackers = !a.g.turn.had_attackers;
            Some(format!("{}", a.g.turn.had_attackers))
        }),
        p!("an additional combat phase follows", "Turn.extra_combats", |a, _| {
            a.g.turn.extra_combats += 1;
            Some(format!("{}", a.g.turn.extra_combats))
        }),
        p!("the turn goes on with another phase after its combat phases", "Turn.after_combat", |a, _| {
            use crate::game::Step;
            let combat = matches!(
                a.g.turn.step,
                Step::BeginCombat | Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage | Step::EndCombat
            );
            if !combat {
                return None;
            }
            a.g.turn.after_combat = if a.g.turn.after_combat == Step::Main2 { Step::End } else { Step::Main2 };
            Some(format!("{:?}", a.g.turn.after_combat))
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
        // (Rare: it needs {C} in a pool at a sampled decision, as haste needs {R}; ENGINE.md 86.5.)
        p!(rare "colorless mana for artifacts only", "ManaPool.art", |a, r| pl(a, r, |p| {
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
        // (Player 0 always: no draw from the gate's random stream, which the
        // other perturbations' positions depend on.)
        p!("more life gained this turn", "Player.life_gained", |a, _r| {
            let p = &mut a.g.players[0];
            p.life_gained += 1;
            Some(format!("P0: {}", p.life_gained))
        }),
        p!("can't gain life", "Player.cant_gain_life", |a, r| pl(a, r, |p| flip(&mut p.cant_gain_life))),
        // (Player 0 always: no draw from the gate's random stream, which the
        // other perturbations' positions depend on.)
        p!("an artifact entered under a player's control this turn", "Player.artifact_entered", |a, _r| {
            let p = &mut a.g.players[0];
            flip(&mut p.artifact_entered).map(|s| format!("P0: {s}"))
        }),
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
        // Who knows a hidden card (ENGINE.md 72; `learn` keeps the clouds
        // right). The other player's look at a library card its owner knows,
        // and (ENGINE.md 192: a Mishra's Bauble or Jace's +2 at a card its
        // owner doesn't know, remembered since) at one its owner doesn't.
        p!("the owner learns the top card of their library", "Obj.known", |a, r| {
            let p = player(r);
            let c = *a.g.players[p as usize].library.last()?;
            if a.g.knows(p, c) {
                return None;
            }
            a.g.learn(p, c);
            Some(format!("P{p}"))
        }),
        p!("the other player learns the top card of a library", "Obj.known", |a, r| {
            let p = player(r);
            let c = *a.g.players[p as usize].library.last()?;
            if a.g.knows(opp(p), c) {
                return None;
            }
            a.g.learn(p, c);
            a.g.learn(opp(p), c);
            Some(format!("P{} learns P{p}'s", opp(p)))
        }),
        p!("the other player learns the top card of a library its owner doesn't know", "Obj.known", |a, r| {
            let p = player(r);
            let c = *a.g.players[p as usize].library.last()?;
            if a.g.knows(opp(p), c) || a.g.knows(p, c) {
                return None;
            }
            a.g.learn(opp(p), c);
            Some(format!("P{} learns P{p}'s", opp(p)))
        }),
        p!("the owner learns a library card below the top three", "Obj.known", |a, r| {
            let p = player(r);
            let lib = &a.g.players[p as usize].library;
            let deep: Vec<(usize, CardIdx)> =
                lib.iter().rev().copied().enumerate().skip(crate::az::encode::TOPS).filter(|&(_, c)| !a.g.knows(p, c)).collect();
            let (k, c) = pick(&deep, r)?;
            a.g.learn(p, c);
            Some(format!("P{p}: {k} from the top"))
        }),
        p!("the other player learns a hand card", "Obj.known", |a, r| {
            let p = player(r);
            let q = opp(p);
            // (The cards of the encoded `HAND` slots; past them below.)
            let hs: Vec<CardIdx> = a.g.hand_sorted(p).into_iter().take(crate::az::encode::HANDS).filter(|&c| !a.g.knows(q, c)).collect();
            let c = pick(&hs, r)?;
            a.g.learn(q, c);
            Some(format!("P{q} learns P{p}'s {}", a.g.name(c)))
        }),
        p!(rare "the other player learns a hand card past the 16th", "Obj.known", |a, r| {
            let p = player(r);
            let q = opp(p);
            let hs: Vec<CardIdx> = a.g.hand_sorted(p).into_iter().skip(crate::az::encode::HANDS).filter(|&c| !a.g.knows(q, c)).collect();
            let c = pick(&hs, r)?;
            a.g.learn(q, c);
            Some(format!("P{q} learns P{p}'s {}", a.g.name(c)))
        }),
        // Layout v7 (ENGINE.md 125.7): who knows a face-down card (`face_down` tokens; a card
        // exiled face down, a face-down permanent).
        p!(rare "the other player learns a face-down exiled card", "Obj.known", |a, r| {
            let fd: Vec<CardIdx> = a.g.face_down.iter().copied().filter(|&c| !a.g.knows(opp(a.g.objs[c.i()].owner), c)).collect();
            let c = pick(&fd, r)?;
            let q = opp(a.g.objs[c.i()].owner);
            a.g.learn(q, c);
            Some(format!("P{q} learns {}", a.g.name(c)))
        }),
        p!(rare "the other player learns a face-down permanent", "Obj.known", |a, r| {
            let fd: Vec<CardIdx> =
                a.g.face_down_ids.iter().map(|x| x.0).filter(|&c| !a.g.knows(opp(a.g.objs[c.i()].owner), c)).collect();
            let c = pick(&fd, r)?;
            let q = opp(a.g.objs[c.i()].owner);
            a.g.learn(q, c);
            Some(format!("P{q} learns a face-down permanent"))
        }),
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
        p!("a creature had first strike as the first combat damage step began", "Obj.struck_first", |a, r| {
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
        p!("a planeswalker's loyalty ability was activated this turn", "Game.loyalty", |a, r| {
            let t = a.g.turn.number as u16;
            let cs: Vec<CardIdx> = slots(&a.g)
                .into_iter()
                .filter(|&c| a.g.is_type(c, types::PLANESWALKER) && !(a.g.loyalty_of(c).turn == t && a.g.loyalty_of(c).used > 0))
                .collect();
            let c = pick(&cs, r)?;
            let o = a.g.loyalty_mut(c);
            o.turn = t;
            o.used = 1;
            Some(a.g.name(c).to_string())
        }),
        p!("a planeswalker's activations are another turn's", "Game.loyalty", |a, r| {
            let t = a.g.turn.number as u16;
            let cs: Vec<CardIdx> = slots(&a.g)
                .into_iter()
                .filter(|&c| a.g.is_type(c, types::PLANESWALKER) && a.g.loyalty_of(c).turn == t && a.g.loyalty_of(c).used > 0)
                .collect();
            let c = pick(&cs, r)?;
            // (Another turn: the previous one, or the next before the first.)
            a.g.loyalty_mut(c).turn = if t == 0 { 1 } else { t - 1 };
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
        // EoE W4 (ENGINE.md 99.14): a target in exile (Blade of the Swarm's
        // trigger), another exiled card. (No draw from the gate's random
        // stream: the first encoded item with an exiled target.)
        p!(rare "a stack item targets another exiled card", "StackItem.targets", |a, _r| {
            let g = &mut a.g;
            let exiled = |g: &Game, t: Target| matches!(t, Target::Obj(o) if g.live(o) && g.objs[o.idx.i()].zone == Zone::Exile);
            let i = stack_slots(g).into_iter().find(|&i| g.stack[i].targets.iter().any(|&(_, t)| exiled(g, t)))?;
            let k = g.stack[i].targets.iter().position(|&(_, t)| exiled(g, t))?;
            let Target::Obj(cur) = g.stack[i].targets[k].1 else { return None };
            let other = g.exile.iter().copied().find(|&c| c != cur.idx)?;
            g.stack[i].targets[k].1 = Target::Obj(g.obj_ref(other));
            Some(g.name(other).to_string())
        }),
        // EoE W4 (ENGINE.md 99.14): which object a stack item noted (Close
        // Encounter's chosen creature or card, emerge's permanent, station's
        // tapped creature): another permanent. (No draw from the gate's
        // random stream.)
        p!(rare "a stack item noted another object", "StackItem.noted", |a, _r| {
            let g = &mut a.g;
            let i = stack_slots(g).into_iter().find(|&i| !g.stack[i].noted.is_empty())?;
            let other = g.battlefield.iter().copied().find(|&c| !g.stack[i].noted.iter().any(|n| n.idx == c))?;
            g.stack[i].noted[0] = g.obj_ref(other);
            Some(g.name(other).to_string())
        }),
        // EoE W5 (ENGINE.md 99.15): which permanent gave the alternative cost
        // a spell was cast for (`Mark::AltGranted`): another permanent. (No
        // draw from the gate's random stream.)
        p!(rare "a spell's granted alternative cost came from another permanent", "Game.marks", |a, _r| {
            let g = &mut a.g;
            let spells = stack_spells(g);
            let k = g.marks.iter().position(|&(o, m)| {
                matches!(m, crate::game::Mark::AltGranted { .. }) && spells.iter().any(|&(_, c)| g.obj_ref(c) == o)
            })?;
            let (o, crate::game::Mark::AltGranted { granter, def, grant }) = g.marks[k] else { return None };
            let other = g.battlefield.iter().copied().find(|&c| c != granter.idx)?;
            g.marks[k] = (o, crate::game::Mark::AltGranted { granter: g.obj_ref(other), def, grant });
            Some(g.name(other).to_string())
        }),
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
        p!("a spell was cast for an alternative cost", "StackItem.alt", |a, r| {
            // (A spell whose card has an alternative cost, or cast for one:
            // the state stays one a game reaches, so a pending payment can be
            // recomputed.)
            let v: Vec<usize> = stack_spells(&a.g).into_iter().filter(|&(i, c)| a.g.stack[i].alt != 0 || !a.g.def_of(c).alt_costs.is_empty()).map(|(i, _)| i).collect();
            let i = pick(&v, r)?;
            a.g.stack[i].alt ^= 1;
            Some(format!("stack item {} from the top: alt {}", a.g.stack.len() - 1 - i, a.g.stack[i].alt))
        }),
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
            // (Of a game played: the current game's is the game's chooser. A constructed
            // best-of-three's game 1's is not encoded, ENGINE.md 130: its game 2's, once played.)
            let i = encoded_game(a)?;
            a.m.choosers[i] = opp(a.m.choosers[i]);
            Some(format!("game {}", i + 1))
        }),
        p!(rare "another played first", "MatchState.starters", |a, _| {
            let i = encoded_game(a)?;
            a.m.starters[i] = opp(a.m.starters[i]);
            Some(format!("game {}", i + 1))
        }),
        p!(rare "a constructed best-of-three's game 1 had another chooser and starter", "MatchState.choosers", |a, _| {
            // (ENGINE.md 130: not encoded, a limit by design)
            if !a.m.constructed_bo3 || a.m.results[0] == 0 {
                return None;
            }
            a.m.choosers[0] = opp(a.m.choosers[0]);
            a.m.starters[0] = opp(a.m.starters[0]);
            Some("game 1".into())
        }),
        p!("a game win", "MatchState.wins", |a, r| {
            let p = player(r) as usize;
            a.m.wins[p] += 1;
            Some(format!("P{p}"))
        }),
        p!("a player's decks in the match are dealt", "MatchState.dealt", |a, r| {
            // (ENGINE.md 154: the match's dealt seat, which the encoding shows for both players; a match with one
            // already: the other seat instead)
            // (both dealt, ENGINE.md 168.1: one of them only)
            if a.m.dealt_both {
                a.m.dealt_both = false;
                a.m.dealt = Some(player(r));
                return Some(format!("P{} only", player(r)));
            }
            let p = match a.m.dealt {
                Some(d) => opp(d),
                None => player(r),
            };
            a.m.dealt = Some(p);
            Some(format!("P{p}"))
        }),
        p!("a drawn game", "MatchState.draws", |a, _| {
            a.m.draws += 1;
            Some(format!("{}", a.m.draws))
        }),
        // ENGINE.md 142.8 (the user's decision of 2026-11-13: how a player boarded game 2 is an input
        // for game 3): an earlier reveal made in another earlier game - a reveal of game 2 one of game
        // 1 (a cube best-of-three's game 3; a constructed one keeps game 2's only, ENGINE.md 130).
        p!(rare "an earlier game's reveal was in another earlier game", "MatchState.earlier_by_game", |a, r| {
            let p = player(r) as usize;
            let kept: Vec<usize> = (0..3).filter(|&g| a.m.results[g] != 0 && !(a.m.constructed_bo3 && g == 0)).collect();
            let bg = a.m.earlier_by_game[p];
            let from: Vec<(usize, usize)> =
                kept.iter().flat_map(|&g| (0..16 * 64).filter(move |&i| bg[g][i >> 6] & (1u64 << (i & 63)) != 0).map(move |i| (g, i))).collect();
            let (g, i) = pick(&from, r)?;
            let to = *kept.iter().find(|&&h| h != g && bg[h][i >> 6] & (1u64 << (i & 63)) == 0)?;
            a.m.earlier_by_game[p][g][i >> 6] &= !(1u64 << (i & 63));
            a.m.earlier_by_game[p][to][i >> 6] |= 1u64 << (i & 63);
            Some(format!("P{p}: game {} -> game {}", g + 1, to + 1))
        }),
        p!("an identity revealed in an earlier game", "MatchState.earlier", |a, r| {
            let p = player(r) as usize;
            // (A card of the player's pool not revealed yet: a reveal is of a pool's card, as the
            // worlds' samplers keep them; ENGINE.md 36.11, 125.10.)
            let ids = crate::az::identity::identities(a.g.db);
            let pool = &a.m.pools[p];
            let cand: Vec<usize> = pool.list[..pool.n_cards]
                .iter()
                .map(|e| ids[e.0 as usize] as usize)
                .filter(|&i| a.m.earlier[p][i >> 6] & (1u64 << (i & 63)) == 0)
                .collect();
            let i = pick(&cand, r)?;
            a.m.earlier[p][i >> 6] |= 1u64 << (i & 63);
            // (in the last game `earlier` keeps, or the current game's predecessor: ENGINE.md 142.8)
            let g = (0..3).rev().find(|&g| a.m.results[g] != 0 && !(a.m.constructed_bo3 && g == 0)).unwrap_or(0);
            a.m.earlier_by_game[p][g][i >> 6] |= 1u64 << (i & 63);
            Some(format!("P{p}"))
        }),
        // EoE W6 (ENGINE.md 99.16): no draw from the gate's random stream.
        // A recorded control-changing effect ("until end of turn") on the
        // first permanent in the encoded slots, under the control it has (so
        // nothing else changes now: what changes is who controls it once the
        // turn ends).
        p!("a control effect ends at the end of the turn", "W6State.control", |a, _r| {
            let c = *slots(&a.g).first()?;
            let r = a.g.obj_ref(c);
            let to = a.g.objs[c.i()].controller;
            // (Recorded control effects: a game with such a card, ENGINE.md
            // 100.5.1.)
            a.g.w6.seen |= crate::eoe_w6::SEEN_W6_CONTROL;
            a.g.w6.control.push(crate::eoe_w6::ControlFx { obj: r, to, ts: u32::MAX, eot: true });
            Some(a.g.name(c).to_string())
        }),
        // The amount of mana spent to cast a spell on the stack (Unravel's
        // condition; W3's perturbation covers permanents).
        p!(rare "the mana spent to cast a spell", "Game.marks", |a, _r| {
            let &(_, c) = stack_spells(&a.g).first()?;
            mana_spent(&mut a.g, c, 1)
        }),
        // A copy effect's "except it has this ability" (Thespian's Stage):
        // its definition made with the ability (only in a table with one).
        p!(rare "a copy has an ability by its exception", "Game.copies", |a, _r| {
            let ss = slots(&a.g);
            let db = a.g.db;
            let k = a.g.copies.iter().position(|x| ss.contains(&x.obj.idx) && db.w6.kept.iter().any(|y| y.0 .0 == x.vals.def))?;
            let d = a.g.copies[k].vals.def;
            let m = db.w6.kept.iter().find(|y| y.0 .0 == d)?.1;
            a.g.copies[k].vals.def = m;
            let c = a.g.copies[k].obj.idx;
            a.g.objs[c.i()].def = m;
            Some(db.def(d).name.clone())
        }),
        // Player 0 created tokens this turn (Moonlit Meditation's "the
        // first time ... each turn").
        p!("a player created tokens this turn", "W6State.tokens_turn", |a, _r| {
            let t = a.g.turn.number as u16;
            if a.g.w6.tokens_turn[0] == t {
                return None;
            }
            a.g.w6.tokens_turn[0] = t;
            Some("P0".to_string())
        }),
        // ENGINE.md 123 (Mistrise Village). (No draw from the gate's random
        // stream: the active player.)
        p!("a player's next spell this turn can't be countered", "Player.uncounterable_next", |a, _r| {
            let p = a.g.turn.active as usize;
            a.g.players[p].uncounterable_next += 1;
            Some(format!("P{p}: {}", a.g.players[p].uncounterable_next))
        }),
        // ---- ENGINE.md 142: the pairwise gate's perturbations ----
        // Families on one candidate list each (the pairwise pass draws every perturbation of a
        // position from one stream, so the members of a family act on the same object): which
        // protection, the sources of a creature's power and toughness (counters, effects and
        // their durations), counter kinds. Then one perturbation per `Mod` variant the list above
        // lacked (`MOD_PERTURBATIONS`, `mod_variant` an exhaustive match), group (rules) effects,
        // and an effect's duration.
        p!("a creature gains protection from white", "Game.effects", |a, r| protection(a, r, Prot { colors: 1 << 0, ..Default::default() })),
        p!("a creature gains protection from blue", "Game.effects", |a, r| protection(a, r, Prot { colors: 1 << 1, ..Default::default() })),
        p!("a creature gains protection from black", "Game.effects", |a, r| protection(a, r, Prot { colors: 1 << 2, ..Default::default() })),
        p!("a creature gains protection from green", "Game.effects", |a, r| protection(a, r, Prot { colors: 1 << 4, ..Default::default() })),
        p!("a creature gains protection from colorless", "Game.effects", |a, r| protection(a, r, Prot { colorless: true, ..Default::default() })),
        p!("a creature gains protection from everything", "Game.effects", |a, r| protection(a, r, Prot { everything: true, ..Default::default() })),
        p!("a creature gains protection from creatures", "Game.effects", |a, r| protection(a, r, Prot { types: types::CREATURE, ..Default::default() })),
        p!("a creature gains protection from its controller's opponents", "Game.effects", |a, r| protection(a, r, Prot { opponents: true, ..Default::default() })),
        p!("a creature gains protection from coloured spells", "Game.effects", |a, r| protection(a, r, Prot { colored_spells: true, ..Default::default() })),
        p!("+1/+1 until its controller's next turn", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::Pt(1, 1), Until::YourNextTurn, p)
        }),
        p!("+1/+1 until the other player's next turn", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            let p = opp(a.g.objs[c.i()].controller);
            effect_until(&mut a.g, c, Mod::Pt(1, 1), Until::YourNextTurn, p)
        }),
        p!("+1/+1 with no duration", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::Pt(1, 1), Until::Forever, p)
        }),
        p!("+1/+1 for as long as another permanent remains on the battlefield", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            let w = while_other(&a.g, c)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::Pt(1, 1), w, p)
        }),
        p!("a permanent loses all abilities for as long as another permanent remains on the battlefield", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.lost_abilities(c)).collect();
            let c = pick(&cs, r)?;
            let w = while_other(&a.g, c)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::LoseAllAbilities, w, p)
        }),
        p!("a permanent loses all abilities with no duration", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| !a.g.lost_abilities(c)).collect();
            let c = pick(&cs, r)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::LoseAllAbilities, Until::Forever, p)
        }),
        p!("a creature can't block with no duration", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| !a.g.flags_of(c).cant_block).collect();
            let c = pick(&cs, r)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::CantBlock, Until::Forever, p)
        }),
        p!("a land becomes a 3/3 creature with no duration", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::LAND) && !a.g.is_creature(c)).collect();
            let c = pick(&cs, r)?;
            let p = a.g.objs[c.i()].controller;
            effect_until(&mut a.g, c, Mod::Types { add: types::CREATURE, subtypes: Vec::new(), replace: false }, Until::Forever, p)?;
            effect_until(&mut a.g, c, Mod::SetPt(3, 3), Until::Forever, p)
        }),
        p!("an effect's duration is another", "Game.effects", |a, r| {
            // (An effect from the position on a permanent or a group; its duration made another
            // one: until end of turn <-> no duration, until a player's next turn or while an object
            // remains -> until end of turn.)
            let g = &a.g;
            let live: Vec<usize> = (0..g.effects.len())
                .filter(|&i| match &g.effects[i].on {
                    Affected::Obj(o) => g.live(*o) && g.objs[o.idx.i()].zone == Zone::Battlefield,
                    Affected::Group(_) => true,
                })
                .collect();
            let i = pick(&live, r)?;
            let e = &mut a.g.effects[i];
            let old = e.until;
            e.until = if old == Until::EndOfTurn { Until::Forever } else { Until::EndOfTurn };
            let what = format!("{:?}: {old:?} -> {:?}", e.m, e.until);
            a.g.effects_changed();
            Some(what)
        }),
        p!("another named counter", "Game.counters", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            counter(&mut a.g, c, Counter::Named(1))
        }),
        p!("two named counters", "Game.counters", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            counter(&mut a.g, c, Counter::Named(0))?;
            counter(&mut a.g, c, Counter::Named(0))
        }),
        p!("a stun counter and a named counter", "Game.counters", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            counter(&mut a.g, c, Counter::Stun)?;
            counter(&mut a.g, c, Counter::Named(0))
        }),
        p!("a land gains \"sacrifice it: add {B}\"", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::LAND)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::GainSacMana(1 << 2))
        }),
        p!("a creature has all creature types", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::AllCreatureTypes)
        }),
        p!("a land becomes a 2/2 creature and nothing else", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.is_type(c, types::LAND) && !a.g.is_creature(c)).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::BecomeTypes { types: types::CREATURE, subtypes: Vec::new() })?;
            effect(&mut a.g, c, Mod::SetPt(2, 2))
        }),
        p!(rare "a permanent gains a latent ability of its card", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slots(&a.g).into_iter().filter(|&c| a.g.def_of(c).latent > 0).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::GainLatent(0))
        }),
        p!("a creature's power and toughness are switched", "Game.effects", |a, r| {
            let cs: Vec<CardIdx> = slot_creatures(&a.g).into_iter().filter(|&c| a.g.pt(c).0 != a.g.pt(c).1).collect();
            let c = pick(&cs, r)?;
            effect(&mut a.g, c, Mod::SwitchPt)
        }),
        p!("a player gained control of a permanent until end of turn", "Game.effects", |a, r| {
            let c = pick(&slots(&a.g), r)?;
            let prev = a.g.objs[c.i()].controller;
            let to = opp(prev);
            let turn = a.g.turn.number;
            let o = &mut a.g.objs[c.i()];
            o.controller = to;
            o.control_since = turn;
            effect_until(&mut a.g, c, Mod::ControlFrom(prev), Until::EndOfTurn, to)
        }),
        p!("a creature attacks the other player this combat if able", "Game.effects", |a, r| {
            let c = pick(&slot_creatures(&a.g), r)?;
            effect(&mut a.g, c, Mod::MustAttackPlayer)
        }),
        p!("creatures a player controls can't block this turn (a rules effect)", "Game.effects", |a, r| {
            let p = player(r);
            let f = crate::db::Filter::And(Box::new(crate::db::Filter::Type(types::CREATURE)), Box::new(crate::db::Filter::Controller(true)));
            a.g.add_effect(Affected::Group(f), Mod::CantBlock, Until::EndOfTurn, p);
            Some(format!("P{p}'s creatures"))
        }),
        p!("combat damage is prevented this turn (a rules effect)", "Game.effects", |a, r| {
            let p = player(r);
            a.g.add_effect(Affected::Group(crate::db::Filter::Type(types::CREATURE)), Mod::PreventCombatDamage, Until::EndOfTurn, p);
            Some(format!("by P{p}"))
        }),
        // ENGINE.md 142.8's run found a framed row's gap (`known_gaps`): the controlled player's
        // knowledge of its controller's hand. (Its own perturbation, so the gate meets it on every run:
        // the turn controlled as "the active player is controlled this turn" does, then the controlled
        // player learns a hand card of its controller.)
        p!("the controlled player learns a hand card of its controller", "Obj.known", |a, r| {
            if a.g.turn_controller.is_some() {
                return None;
            }
            let (p, q) = (a.g.turn.active, opp(a.g.turn.active));
            a.g.turn_controller = Some(q);
            a.g.sync_controller_view();
            let hs: Vec<CardIdx> = a.g.hand_sorted(q).into_iter().filter(|&c| !a.g.knows(p, c)).collect();
            let c = pick(&hs, r)?;
            a.g.learn(p, c);
            Some(format!("P{q} controls P{p}'s turn; P{p} learns P{q}'s {}", a.g.name(c)))
        }),
        // (ENGINE.md 123's "a card name was chosen as a permanent entered" - Disruptor Flute, Anointed
        // Peacekeeper - is the `Mark` perturbation above, layout v7's: ENGINE.md 125.)
        // The cards found by their owner's library search that everyone tells apart (ENGINE.md 140.9, 142.12,
        // 223): a tutor's card put on top of the library.
        p!("the top library card is its owner's search's find", "Game.finds_told", |a, r| {
            let p = player(r);
            let &c = a.g.players[p as usize].library.last()?;
            // (a find the other player has seen is one no longer: `Game::learn`)
            if a.g.finds_told.contains(&c) || a.g.knows(opp(p), c) {
                return None;
            }
            a.g.finds_told.push(c);
            Some(format!("P{p}'s top library card"))
        }),
        p!(rare "a search's find is no longer told apart", "Game.finds_told", |a, r| {
            let k = r.below(a.g.finds_told.len().max(1) as u32) as usize;
            if k >= a.g.finds_told.len() {
                return None;
            }
            let c = a.g.finds_told.remove(k);
            Some(format!("P{}'s find {k}", a.g.objs[c.i()].owner))
        }),
        // The picks in progress (ENGINE.md 125, 142.10, the Fable review's X3): one more pick - a card
        // the pending decision offers, the same one (one stream) - made by the asking task, which
        // holds picks already, or held by the task around it: which task holds a pick is public (the
        // head's pick floats are the innermost holder's; an enclosing task's are links tokens with
        // their depth). (An offered card keeps the asking task's state one its next decision reads:
        // the hidden-information check recomputes it for the player who doesn't decide.)
        p!(rare "the asking task holding picks picks one more offered card", "Game.agenda", |a, r| {
            let ts = pick_tasks(&a.g);
            let &(i, holds) = ts.first()?;
            if !holds || i + 1 != a.g.agenda.len() {
                return None;
            }
            let c = offered_unpicked(&a.g, r)?;
            task_picked(&mut a.g.agenda[i]).push(c);
            Some(format!("agenda task {i} picks {}", a.g.name(c)))
        }),
        p!(rare "an enclosing task holds a pick in progress", "Game.agenda", |a, r| {
            let ts = pick_tasks(&a.g);
            let h = ts.iter().position(|t| t.1)?;
            let i = ts.get(h + 1)?.0;
            let c = offered_unpicked(&a.g, r)?;
            task_picked(&mut a.g.agenda[i]).push(c);
            Some(format!("agenda task {i}, around task {}, picks {}", ts[h].0, a.g.name(c)))
        }),
    ]
}

/// The agenda's tasks that can hold picks in progress (`tail_encode::agenda_picks`), innermost
/// first: (agenda index, holds picks now).
fn pick_tasks(g: &Game) -> Vec<(usize, bool)> {
    use crate::flow::Task;
    g.agenda
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(i, t)| match t {
            Task::Run(f) => Some((i, !f.picked.is_empty() || !f.picked_defs.is_empty())),
            Task::PayCosts { picked, .. } | Task::ConvokePay { picked, .. } | Task::OrderTimestamps { picked, .. } | Task::Devour { picked, .. } => Some((i, !picked.is_empty())),
            _ => None,
        })
        .collect()
}

/// The objects a task of `pick_tasks` has picked so far.
fn task_picked(t: &mut crate::flow::Task) -> &mut Vec<CardIdx> {
    use crate::flow::Task;
    match t {
        Task::Run(f) => &mut f.picked,
        Task::PayCosts { picked, .. } | Task::ConvokePay { picked, .. } | Task::OrderTimestamps { picked, .. } | Task::Devour { picked, .. } => picked,
        _ => unreachable!("not a task holding picks"),
    }
}

/// A card the pending decision offers (`Choice::Card`) that no task has picked.
fn offered_unpicked(g: &Game, r: &mut Rng) -> Option<CardIdx> {
    let d = g.pending.as_ref()?;
    let p = crate::az::tail_encode::agenda_picks(g, 0);
    let picked: Vec<CardIdx> = p.objs.iter().chain(p.outer.iter().flat_map(|o| o.objs.iter())).map(|x| x.0).collect();
    let mut cand: Vec<CardIdx> = d
        .options
        .iter()
        .filter_map(|o| match o.choice {
            crate::decision::Choice::Card(c) if !picked.contains(&c) => Some(c),
            _ => None,
        })
        .collect();
    cand.dedup();
    pick(&cand, r)
}

/// The gate's paired check of a chosen name's face (ENGINE.md 125.11): the first permanent in the
/// encoded slots names the table's first split card's left half (`left`) or its right half - two
/// names (CR 709.4), so the two states differ in both views and both encodings (`tests/az.rs`, the
/// gate). None without a permanent or a split card.
pub fn name_a_split_half(a: &mut MtgAz, left: bool) -> Option<String> {
    let db = a.g.db;
    let (l, r) = (0..db.num_cards() as DefId).find_map(|d| db.def(d).halves)?;
    let c = *slots(&a.g).first()?;
    a.g.marks.retain(|(o, m)| !(o.idx == c && matches!(m, crate::game::Mark::ChosenName(_))));
    mark(&mut a.g, c, crate::game::Mark::ChosenName(if left { l } else { r }))
}

/// The perturbations whose public change the encoding leaves out (ENGINE.md
/// 58.9: by design), each with its reason: since layout v7 (ENGINE.md 125.7)
/// the graveyard's order no rule reads, the user's choice (D5), and (ENGINE.md
/// 130) a constructed best-of-three's game-1 chooser and starter, which the
/// match block leaves out so that a real game 2 encodes as a post-sideboard
/// start's; the face-down exiled card and the earlier games' reveals, limits
/// until v7, are encoded (`face_down`, `my_revealed`). Layout v5 (ENGINE.md 60) encodes
/// the 42 gaps layout v4 had no room for (58.10), layout v6 (ENGINE.md
/// 100.5) the gaps the Edge of Eternities port and workstream C added after
/// v5 froze, and two facts cards read that were limits by design
/// (`V6_FIXED`); the graveyard's order, which Priority's `GRAVEYARD` ids
/// depended on, no id depends on since its casts became one option and a
/// list choice (ENGINE.md 100.5). No gap is left. How to add a gap later:
/// ENGINE.md 100.5.
pub fn limits() -> Vec<Limit> {
    #[allow(unused_imports)]
    use LimitKind::{Design, NextLayout};
    let l = |name, kind, why| Limit { name, kind, why };
    vec![
        l("two graveyard cards swap places", Design, "the user's choice (D5, ENGINE.md 125): graveyards are bags of identities (counts), not ordered slots (ENGINE.md 22.2); the only rules that read the order are the table's positional readers (Shallow Grave's \"the top creature card\"), whose cards' order is the `graveyard_order` tokens (R17 checks the table); no action id depends on the order: a graveyard-card choice or target is a list id whose token is its card, the graveyards in order in the list block while it is pending (ENGINE.md 65.10), and Priority's casts and plays from the graveyard are one option, GRAVEYARD + 0, then that list's choice (ENGINE.md 100.5)"),
        l("a constructed best-of-three's game 1 had another chooser and starter", Design, "with both 75s public, game 1 played with the registered mains and each game labelled by its own result, who chose and who played first in game 1 tells nothing about the decks or the game being played; a match that starts at game 2 (start_game 2) has none, and a real game 2 must encode as a synthetic one (ENGINE.md 130): the match block leaves them out; game 1's result is encoded (the score)"),
    ]
}

/// The perturbations of fields hidden decklists hide from the other player
/// (ENGINE.md 102: public in every other format): with hidden decklists the
/// gate requires each to reach its owner's view and encoding and neither of
/// the other player's (`tests/az.rs`, `public_state_reaches_the_encoding`).
/// The sideboard's size tells the number of basic lands in the deck.
pub const HIDDEN_BY_DECKLISTS: [&str; 1] = ["a sideboard card fewer"];

/// The perturbations layout v6 encodes (ENGINE.md 100.5): the next-layout
/// gaps of the Edge of Eternities port (ENGINE.md 99) and workstream C
/// (100.3), two facts cards read that were limits by design (the life gained
/// this turn, a permanent that entered this turn) and the granted triggered
/// abilities (W3, no perturbation before). Its tests read them back and check
/// that they hide nothing (`tests/az.rs`).
pub const V6_FIXED: [&str; 26] = [
    "void holds (a nonland permanent left the battlefield this turn)",
    "more life gained this turn",
    "a creature was cast for its warp cost",
    "a creature has a regeneration shield",
    "the mana spent to cast a permanent",
    "a permanent is phased out",
    "a phased-out permanent phased out indirectly",
    "a copy is legendary by its exception",
    "a stack item targets another exiled card",
    "a stack item noted another object",
    "a player will control the other's next turn",
    "the active player is controlled this turn",
    "a spell's granted alternative cost came from another permanent",
    "a permanent entered this turn",
    "the other player learns a hand card past the 16th",
    "the turn goes on with another phase after its combat phases",
    "an artifact entered under a player's control this turn",
    "a permanent gained a triggered ability",
    // ENGINE.md 100.5.1: W2's and W6's.
    "mana in a pool is restricted to activating abilities of lands",
    "a land entered the battlefield this turn",
    "a control effect ends at the end of the turn",
    "a copy has an ability by its exception",
    "a player created tokens this turn",
    "the mana spent to cast a spell",
    // ENGINE.md 124.2: lane B of the Legacy Doomsday deck's (perm 133-141).
    "a creature type was chosen as a permanent entered",
    // ENGINE.md 123 (Mistrise Village, g6 float 8).
    "a player's next spell this turn can't be countered",
];

/// The perturbations layout v7 encodes (ENGINE.md 124.2: its block of
/// globals, the pool's mana of a chosen creature type by colour and type,
/// exact for any number of types). Its tests read them back and check that
/// they hide nothing (`tests/az.rs`, `tests/cards_doomsday_b.rs`).
///
/// ENGINE.md 125 (layout v7's variable-length rows): every `Game.marks`
/// variant the encoding missed (the gate's G1: one perturbation per variant,
/// `MARK_PERTURBATIONS`), the library cards someone knows below the top three
/// (`library_known`), a positional graveyard reader's order (Shallow Grave:
/// `graveyard_order`), a stack ability's source, a trigger's event, a third
/// target; and (ENGINE.md 125.7) the two limits by design until v7 - a card
/// exiled face down and who knows it (`face_down`, face-down permanents too),
/// the identities the viewer revealed in earlier games (`my_revealed`).
pub const V7_FIXED: [&str; 41] = [
    "mana in a pool is restricted to creature spells of a chosen type",
    "a permanent entered with a big kicker count and X",
    "Containment Priest's replacement waits for a copy choice",
    "a spell's converge colours spent so far",
    "a spell's converge colours spent",
    "a permanent has a granted leave-the-battlefield trigger",
    "a spell will give its permanent a trigger as it enters",
    "a permanent's once-each-turn permission was used this turn",
    "a spell's X is capped by a permission's mana value",
    "a permanent was harnessed",
    "a permanent boasted this turn",
    "a permanent entered kicked with an X",
    "a creature became saddled this turn",
    "a permanent's once-chosen modes",
    "odd or even was chosen as a permanent entered",
    "an Aura reanimated a creature",
    "a spell was cast when a sorcery couldn't have been",
    "a granted trigger triggered this turn for a permanent",
    "a permanent looked at the opponent's hand as it entered",
    "a card name was chosen as a permanent entered",
    "mana that gives haste was spent on a spell",
    "a mana source is chosen to be exiled by delve",
    "a hand card is promised to a delve",
    "a spell's delve exiled a card to pay",
    "a permanent was dealt damage this turn",
    "a spell was cast from its caster's hand free",
    "the owner learns a library card below the top three",
    "two creature cards of a graveyard swap places",
    "a stack ability's source is another permanent",
    "a trigger's event names another object",
    "a trigger's event names the other player",
    "a stack item's third target is another",
    "an exiled card is turned face down",
    "the other player learns a face-down exiled card",
    "the other player learns a face-down permanent",
    "an identity revealed in an earlier game",
    // ENGINE.md 142.8: the game an earlier reveal was made in (`opp_revealed` / `my_revealed`'s game bits).
    "an earlier game's reveal was in another earlier game",
    // ENGINE.md 125's picks in progress (layout v6 has none), 142.10's enclosing task's.
    "the asking task holding picks picks one more offered card",
    "an enclosing task holds a pick in progress",
    // ENGINE.md 142.12: the finds the other player tells apart (`found`).
    "the top library card is its owner's search's find",
    "a search's find is no longer told apart",
];

/// One perturbation per `Mark` variant (ENGINE.md 125, the gate's G1): the
/// variant, the perturbation that makes it. `tests/v7_tails.rs` checks that
/// every variant has one (`mark_variant`, an exhaustive match) and the gate
/// that each reaches the encoding.
pub const MARK_PERTURBATIONS: [(&str, &str); 36] = [
    ("Entered", "a permanent entered with a big kicker count and X"),
    ("PriestPending", "Containment Priest's replacement waits for a copy choice"),
    ("Exhausted", "a permanent was exhausted (an exhaust ability)"),
    ("ConvergePending", "a spell's converge colours spent so far"),
    ("Converge", "a spell's converge colours spent"),
    ("ChosenColor", "a colour was chosen as a permanent entered"),
    ("ChosenCreatureType", "a creature type was chosen as a permanent entered"),
    ("Uncounterable", "a spell can't be countered"),
    ("LeaveTrigger", "a permanent has a granted leave-the-battlefield trigger"),
    ("GrantOnEnter", "a spell will give its permanent a trigger as it enters"),
    ("UsedTurn", "a permanent's once-each-turn permission was used this turn"),
    ("MvCap", "a spell's X is capped by a permission's mana value"),
    ("Harnessed", "a permanent was harnessed"),
    ("Boasted", "a permanent boasted this turn"),
    ("KickerX", "a permanent entered kicked with an X"),
    ("Saddled", "a creature became saddled this turn"),
    ("ModesUsed", "a permanent's once-chosen modes"),
    ("Parity", "odd or even was chosen as a permanent entered"),
    ("Reanimated", "an Aura reanimated a creature"),
    ("NotSorceryTiming", "a spell was cast when a sorcery couldn't have been"),
    ("Triggered", "a granted trigger triggered this turn for a permanent"),
    ("LookedHand", "a permanent looked at the opponent's hand as it entered"),
    ("ChosenName", "a card name was chosen as a permanent entered"),
    ("Exerted", "a creature was exerted"),
    ("HasteMana", "mana that gives haste was spent on a spell"),
    ("DelveLate", "a mana source is chosen to be exiled by delve"),
    ("DelveExile", "a graveyard card is chosen for delve"),
    ("DelvePromised", "a hand card is promised to a delve"),
    ("ExiledToPay", "a spell's delve exiled a card to pay"),
    ("DealtDamage", "a permanent was dealt damage this turn"),
    ("Granted", "a permanent gained a triggered ability"),
    ("WarpCast", "a creature was cast for its warp cost"),
    ("RegenShield", "a creature has a regeneration shield"),
    ("ManaSpent", "the mana spent to cast a permanent"),
    ("AltGranted", "a spell's granted alternative cost came from another permanent"),
    ("FreeFromHand", "a spell was cast from its caster's hand free"),
];

/// The variant's name of a mark (`MARK_PERTURBATIONS`): an exhaustive match,
/// so a new variant is a compile error here until it has a perturbation and an
/// encoding.
pub fn mark_variant(m: &crate::game::Mark) -> &'static str {
    use crate::game::Mark as M;
    match m {
        M::Entered { .. } => "Entered",
        M::PriestPending => "PriestPending",
        M::Exhausted(_) => "Exhausted",
        M::ConvergePending(_) => "ConvergePending",
        M::Converge(_) => "Converge",
        M::ChosenColor(_) => "ChosenColor",
        M::ChosenCreatureType(_) => "ChosenCreatureType",
        M::Uncounterable => "Uncounterable",
        M::LeaveTrigger { .. } => "LeaveTrigger",
        M::GrantOnEnter { .. } => "GrantOnEnter",
        M::UsedTurn(_) => "UsedTurn",
        M::MvCap(_) => "MvCap",
        M::Harnessed => "Harnessed",
        M::Boasted(_) => "Boasted",
        M::KickerX(_) => "KickerX",
        M::Saddled(_) => "Saddled",
        M::ModesUsed(_) => "ModesUsed",
        M::Parity(_) => "Parity",
        M::Reanimated(_) => "Reanimated",
        M::NotSorceryTiming => "NotSorceryTiming",
        M::Triggered { .. } => "Triggered",
        M::LookedHand => "LookedHand",
        M::ChosenName(_) => "ChosenName",
        M::Exerted => "Exerted",
        M::HasteMana => "HasteMana",
        M::DelveLate => "DelveLate",
        M::DelveExile => "DelveExile",
        M::DelvePromised => "DelvePromised",
        M::ExiledToPay(_) => "ExiledToPay",
        M::DealtDamage(_) => "DealtDamage",
        M::Granted { .. } => "Granted",
        M::WarpCast => "WarpCast",
        M::RegenShield => "RegenShield",
        M::ManaSpent(_) => "ManaSpent",
        M::AltGranted { .. } => "AltGranted",
        M::FreeFromHand => "FreeFromHand",
    }
}


/// One perturbation per `Mod` variant a continuous effect from a resolved spell or ability can
/// have (ENGINE.md 142; the Fable review's B4: four had none): the variant, the perturbation that
/// makes it. `mod_variant` is an exhaustive match, so a new variant is a compile error until it is
/// here or in [`STATIC_ONLY_MODS`]; `tests/az.rs` checks the table.
pub const MOD_PERTURBATIONS: [(&str, &str); 21] = [
    ("Types", "a permanent becomes an artifact too"),
    ("Colors", "a permanent's colour becomes another"),
    ("Keywords", "a creature gains flying"),
    ("Protection", "a creature gains protection from red"),
    ("SetPt", "base power and toughness 7/7"),
    ("Pt", "+1/+1 until end of turn"),
    ("CantBlock", "a creature can't block"),
    ("CantBeBlocked", "a creature can't be blocked"),
    ("MustAttack", "a creature attacks each combat"),
    ("ExileIfDies", "exiled if it would die"),
    ("GainSacMana", "a land gains \"sacrifice it: add {B}\""),
    ("LoseAllAbilities", "a permanent loses all abilities"),
    ("AllCreatureTypes", "a creature has all creature types"),
    ("BecomeTypes", "a land becomes a 2/2 creature and nothing else"),
    ("GainLatent", "a permanent gains a latent ability of its card"),
    ("SwitchPt", "a creature's power and toughness are switched"),
    ("Goad", "a creature is goaded"),
    ("ExileIfLeaves", "exiled if it would leave"),
    ("ControlFrom", "a player gained control of a permanent until end of turn"),
    // (Darkness's "prevent all combat damage that would be dealt this turn": a group's rules
    // effect, the only kind the lowering makes, lower/effect_eoe_d.rs.)
    ("PreventCombatDamage", "combat damage is prevented this turn (a rules effect)"),
    ("MustAttackPlayer", "a creature attacks the other player this combat if able"),
];

/// The `Mod` variants only a static ability has (ENGINE.md 142): the lowering builds them inside a
/// `StaticDef` only, and `Game::add_effect` refuses them loudly (`Mod::static_only`). A static
/// ability is its source's printed text (its identity, encoded) applied to the encoded board, so
/// what it does is a function of the row.
pub const STATIC_ONLY_MODS: [(&str, &str); 5] = [
    ("PtPer", "an Equipment's or a static ability's \"+p/+t for each\" (lower/statics.rs, lower/printing.rs)"),
    ("PtPerSourceCounter", "\"+1/+1 for each +1/+1 counter on this Equipment\" (lower/printing.rs)"),
    ("PtPerGraveyardCards", "Dark Matter Manipulator's \"+2/+0 for every seven cards in your graveyard\" (lower/statics.rs; the graveyard is encoded)"),
    ("AttackDespiteDefender", "\"can attack as though it didn't have defender\" (Mechan Shieldmate; lower/statics.rs)"),
    ("PowerPlusGreatestMv", "Emissary Escort's \"+X/+0\" (lower/eoe_a.rs)"),
];

/// The variant's name of a `Mod` ([`MOD_PERTURBATIONS`], [`STATIC_ONLY_MODS`]): an exhaustive
/// match.
pub fn mod_variant(m: &Mod) -> &'static str {
    match m {
        Mod::Types { .. } => "Types",
        Mod::Colors(_) => "Colors",
        Mod::Keywords(_) => "Keywords",
        Mod::Protection(_) => "Protection",
        Mod::SetPt(..) => "SetPt",
        Mod::Pt(..) => "Pt",
        Mod::CantBlock => "CantBlock",
        Mod::CantBeBlocked => "CantBeBlocked",
        Mod::MustAttack => "MustAttack",
        Mod::ExileIfDies => "ExileIfDies",
        Mod::PtPer { .. } => "PtPer",
        Mod::PtPerSourceCounter { .. } => "PtPerSourceCounter",
        Mod::PtPerGraveyardCards { .. } => "PtPerGraveyardCards",
        Mod::GainSacMana(_) => "GainSacMana",
        Mod::LoseAllAbilities => "LoseAllAbilities",
        Mod::AttackDespiteDefender => "AttackDespiteDefender",
        Mod::AllCreatureTypes => "AllCreatureTypes",
        Mod::BecomeTypes { .. } => "BecomeTypes",
        Mod::GainLatent(_) => "GainLatent",
        Mod::SwitchPt => "SwitchPt",
        Mod::Goad => "Goad",
        Mod::ExileIfLeaves => "ExileIfLeaves",
        Mod::ControlFrom(_) => "ControlFrom",
        Mod::PreventCombatDamage => "PreventCombatDamage",
        Mod::MustAttackPlayer => "MustAttackPlayer",
        Mod::PowerPlusGreatestMv { .. } => "PowerPlusGreatestMv",
    }
}

/// A `Mod` of each variant (the tests' check that every variant is classified).
pub fn mod_samples() -> Vec<Mod> {
    use crate::db::Filter;
    vec![
        Mod::Types { add: 0, subtypes: Vec::new(), replace: false },
        Mod::Colors(1),
        Mod::Keywords(KwSet::default()),
        Mod::Protection(Prot::default()),
        Mod::SetPt(1, 1),
        Mod::Pt(1, 1),
        Mod::CantBlock,
        Mod::CantBeBlocked,
        Mod::MustAttack,
        Mod::ExileIfDies,
        Mod::PtPer { filter: Filter::Any, p: 1, t: 1 },
        Mod::PtPerSourceCounter { counter: Counter::P1P1, p: 1, t: 1 },
        Mod::PtPerGraveyardCards { filter: Filter::Any, per: 7, p: 2, t: 0 },
        Mod::GainSacMana(1),
        Mod::LoseAllAbilities,
        Mod::AttackDespiteDefender,
        Mod::AllCreatureTypes,
        Mod::BecomeTypes { types: 0, subtypes: Vec::new() },
        Mod::GainLatent(0),
        Mod::SwitchPt,
        Mod::Goad,
        Mod::ExileIfLeaves,
        Mod::ControlFrom(0),
        Mod::PreventCombatDamage,
        Mod::MustAttackPlayer,
        Mod::PowerPlusGreatestMv { types: 0 },
    ]
}

/// The pseudo-perturbation of the pairwise pass: the position as it is (ENGINE.md 142).
pub const BASE: &str = "(the position as it is)";

/// The layouts a known gap is a gap of (`KnownGap::layouts`): layout v7 (the `mtg` game) and
/// layout v6 (the `mtg_eoe` game, checked on the gate's EoE positions).
pub const V7: u8 = 1;
pub const V6: u8 = 2;

/// A public-state gap the encoding has, found by the gate and waiting on a decision (ENGINE.md
/// 142; `E:/mtg-zero-scratch/fable-fix/encoding-options.md`, the section `decision` names). The gate
/// fails on every gap no entry covers, and on an entry that covers nothing the gate finds (a stale
/// entry: the encoding was fixed, remove it) - this list goes to empty when the encoding is fixed.
///
/// An entry names perturbations two ways:
/// * `alone`: a perturbation whose change the layout leaves out entirely (the position's row is
///   unchanged; the single-perturbation pass accepts exactly these);
/// * `alike`: perturbations whose changes the layout reads only through what they share (one sum,
///   one flag, the resulting power, the present characteristics an effect gives): two of them can
///   encode alike, and a set of them can encode like none of them.
///
/// [`covered`] says when two states' equal rows are such a gap.
pub struct KnownGap {
    pub name: &'static str,
    pub decision: &'static str,
    pub layouts: u8,
    pub alone: Vec<&'static str>,
    pub alike: Vec<&'static str>,
}

/// Do known gaps cover the equal rows (for `layout`) of two states whose views differ, the position
/// perturbed by the perturbations `a` and by `b` (ENGINE.md 142)? The perturbations of one state and
/// not the other are each assigned to an entry of the layout - as an `alone` perturbation (it
/// changes no row), or to the entry's `alike` group; each entry's group must hold perturbations of
/// both states, or two or more of one state, or one of one state while a perturbation both states
/// share is in the entry's `alike` too (a set of alike changes can encode like a subset of itself).
/// Returns the entries one such assignment uses (credited by the gate), or None.
pub fn covered<'s>(gaps: &[KnownGap], layout: u8, a: &[&'s str], b: &[&'s str]) -> Option<Vec<usize>> {
    let es: Vec<usize> = (0..gaps.len()).filter(|&i| gaps[i].layouts & layout != 0).collect();
    let common: Vec<&str> = a.iter().copied().filter(|x| b.contains(x)).collect();
    let diff: Vec<(&str, usize)> = a.iter().filter(|x| !b.contains(x)).map(|&x| (x, 0)).chain(b.iter().filter(|x| !a.contains(x)).map(|&x| (x, 1))).collect();
    if diff.is_empty() {
        return None;
    }
    // each differing perturbation's options: (entry, alone?)
    let mut opts: Vec<Vec<(usize, bool)>> = Vec::new();
    for &(x, _) in &diff {
        let o: Vec<(usize, bool)> = es
            .iter()
            .flat_map(|&i| {
                let g = &gaps[i];
                [g.alone.iter().any(|y| *y == x).then_some((i, true)), g.alike.iter().any(|y| *y == x).then_some((i, false))]
            })
            .flatten()
            .collect();
        if o.is_empty() {
            return None;
        }
        opts.push(o);
    }
    let mut pick = vec![0usize; diff.len()];
    loop {
        // the groups of this assignment: entry -> (from a, from b)
        let mut groups: Vec<(usize, usize, usize)> = Vec::new();
        let mut used: Vec<usize> = Vec::new();
        for (k, &(_, side)) in diff.iter().enumerate() {
            let (i, alone) = opts[k][pick[k]];
            if !used.contains(&i) {
                used.push(i);
            }
            if alone {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == i) {
                Some(g) => {
                    if side == 0 {
                        g.1 += 1
                    } else {
                        g.2 += 1
                    }
                }
                None => groups.push((i, (side == 0) as usize, (side == 1) as usize)),
            }
        }
        let ok = groups.iter().all(|&(i, na, nb)| (na > 0 && nb > 0) || na + nb >= 2 || common.iter().any(|c| gaps[i].alike.iter().any(|y| y == c)));
        if ok {
            return Some(used);
        }
        // the next assignment
        let mut k = 0;
        loop {
            if k == diff.len() {
                return None;
            }
            pick[k] += 1;
            if pick[k] < opts[k].len() {
                break;
            }
            pick[k] = 0;
            k += 1;
        }
    }
}

/// The perturbations the pairwise pass never combines with another (ENGINE.md 142): each moves an
/// object (a new object, CR 400.7), changes the turn, the step, the active player or the game, or
/// ends the game, or acts on another effect, so the other perturbation's change can become inert
/// (an effect, a mark or a link of an object that left; a "this turn" fact of another turn; the
/// effect it added ended) while it stays in the view. Each is still compared alone with every
/// state.
pub const COMBO_EXCLUDED: [&str; 22] = [
    "a permanent leaves the battlefield",
    "a graveyard card is exiled",
    "an exiled card is turned face down",
    "a hand card is discarded",
    "a library card leaves the game",
    "a revealed companion is put into its owner's hand",
    "a permanent is another card",
    "the turn number + 1",
    "another step",
    "the other player is active",
    // (a player's turns begun: "until their next turn" facts read it, ENGINE.md 142.8's run)
    "a turn begun",
    "the game ends",
    "lost",
    "the next game of the match",
    "an effect ends",
    "an effect's duration is another",
    // (ENGINE.md 142.10: the picks in progress, on the gate's positions with picks; out of the
    // partner draw, so that every position's partners are as before.)
    "the asking task holding picks picks one more offered card",
    "an enclosing task holds a pick in progress",
    // (ENGINE.md 142.12: the finds, added after the partner draws were set - kept out so that every
    // position's partners are as before.)
    "the top library card is its owner's search's find",
    "a search's find is no longer told apart",
    // (ENGINE.md 192: a look at a library card its owner doesn't know, remembered since - added after the partner
    // draws were set, kept out so that every position's partners are as before.)
    "the other player learns the top card of a library its owner doesn't know",
    // (ENGINE.md 151.4: CR 508.8's record. A partner that makes a creature attack makes it inert -
    // while one is attacking, its attacking flag says the steps follow - while the view keeps it; and
    // added after the partner draws were set, so every position's partners are as before. Compared
    // alone with every state: `public_states_encode_apart` checks that it reaches states.)
    "the declare blockers step follows an attacker that left combat",
];

/// The known gaps (`KnownGap`; ENGINE.md 142.2, 142.7): what the pairwise gate finds. Layout v7 has
/// none since 142.7 (its `effects`, `counters` and `numbers` tokens closed the ten it found); layout
/// v6 - the `mtg_eoe` game, which stays on it until EoE run 2 resumes, the user's call - keeps them
/// all, each scoped to layout v6.
pub fn known_gaps() -> Vec<KnownGap> {
    let ps = perturbations();
    let group = ["combat damage is prevented this turn (a rules effect)", "creatures a player controls can't block this turn (a rules effect)"];
    // every perturbation that adds a continuous effect to an object
    let effects: Vec<&'static str> = ps.iter().filter(|p| p.field == "Game.effects" && !group.contains(&p.name)).map(|p| p.name).collect();
    let protection: Vec<&'static str> = ps.iter().filter(|p| p.name.starts_with("a creature gains protection from ")).map(|p| p.name).collect();
    // (A hand card promised to a delve is revealed too, so its v6 row is a learned card's: `alike`.)
    let v7_facts: Vec<&'static str> = V7_FIXED.iter().copied().filter(|&x| x != "a hand card is promised to a delve").collect();
    vec![
        KnownGap {
            name: "layout v6 (mtg_eoe): which protection a permanent has (perm::PROTECTED is one any-of float)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec![],
            alike: protection,
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): power and toughness by source (+1/+1 and -1/-1 counters, P/T effects: one P/T)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec![],
            alike: vec![
                "+1/+1 until end of turn",
                "+1/+1 until its controller's next turn",
                "+1/+1 until the other player's next turn",
                "+1/+1 with no duration",
                "+1/+1 for as long as another permanent remains on the battlefield",
                "base power and toughness 7/7",
                "a creature's power and toughness are switched",
                "a +1/+1 counter on a creature",
                "a -1/-1 counter on a creature",
                "an effect ends",
            ],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): counter kinds (stun, keyword and named counters one sum; a keyword counter's keyword an effect's)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec![],
            alike: vec![
                "a stun counter",
                "a named counter (charge, oil, ...)",
                "another named counter",
                "two named counters",
                "a stun counter and a named counter",
                "a flying counter on a creature",
                "a creature gains flying",
            ],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): a granted \"sacrifice: add mana\" ability (Mod::GainSacMana) has no float",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec!["a land gains \"sacrifice it: add {B}\""],
            alike: vec![],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): must-attack effects are one count (perm::MUST_ATTACK)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec![],
            alike: vec!["a creature attacks each combat", "a creature attacks the other player this combat if able"],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): subtype effects are one count (perm::SUBTYPES)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: vec![],
            alike: vec!["a creature gains a creature type", "a creature has all creature types"],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): group (rules) effects reach the row only through the permanents they affect now",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            alone: group.to_vec(),
            alike: vec![group[1], "a creature can't block", "a creature can't block with no duration"],
        },
        // Found by the pairwise gate on 2026-11-13 (ENGINE.md 142.8's run), in layouts v7 and v6 alike (it
        // predates both: ENGINE.md 100.5's frame): a decision for the user.
        KnownGap {
            name: "a framed row (CR 722: the viewer controls the other player's turn) lacks which of the viewer's own hand cards the controlled player knows",
            decision: "encoding-options.md 2.9 (new, 2026-11-13)",
            layouts: V7 | V6,
            alone: vec![],
            alike: vec![
                "the other player learns a hand card",
                "the active player is controlled this turn",
                "the controlled player learns a hand card of its controller",
            ],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe, EoE run 2) lacks layout v7's facts (V7_FIXED)",
            decision: "encoding-options.md 2.8 (Fable review I3: mtg_eoe onto a layout with v7's facts)",
            layouts: V6,
            alone: v7_facts,
            // (A hand card promised to a delve is revealed too: its v6 row is a learned card's.)
            alike: vec!["a hand card is promised to a delve", "the other player learns a hand card"],
        },
        KnownGap {
            name: "layout v6 (mtg_eoe): continuous effects, only the characteristics they give now (no effect, duration, controller or timestamp)",
            decision: "encoding-options.md 2.8: mtg_eoe stays on layout v6 until EoE run 2 resumes (the user's call); layout v7 encodes it since ENGINE.md 142.7",
            layouts: V6,
            // (and a latent ability an effect gave, `Mod::GainLatent`: rare, none of the gate's EoE positions has one)
            alone: vec!["an effect's duration is another", "a permanent gains a latent ability of its card"],
            alike: effects.into_iter().chain(["an effect ends"]).collect(),
        },
    ]
}

/// Layout v6's gap (ENGINE.md 124.2): a pool's mana restricted to creature
/// spells of a chosen type (Cavern of Souls); layout v7 encodes it (its block,
/// `V7_FIXED`), and the layout v6 game `mtg_eoe` refuses a card table that
/// makes such mana (`az::needs_layout_v7`).
pub const CHOSEN_TYPE_MANA_GAP: &str = "mana in a pool is restricted to creature spells of a chosen type";

/// Layout v6's gap (ENGINE.md 123): the card name a permanent chose as it
/// entered (`Mark::ChosenName`); layout v7 encodes it (its chosen-name floats,
/// `V7_FIXED`), and the layout v6 game `mtg_eoe` refuses a card table holding a
/// card that chooses a name the plain way (`az::needs_layout_v7`).
pub const CHOSEN_NAME_GAP: &str = "a card name was chosen as a permanent entered";

/// The perturbations of layout v4's 42 gaps (ENGINE.md 58.10), which layout
/// v5 encodes (ENGINE.md 60.3): its tests read them back and check that they
/// hide nothing (`tests/az.rs`).
pub const V5_FIXED: [&str; 42] = [
    "a creature attacks each combat",
    "a creature gains a creature type",
    "exiled if it would die",
    "a delayed trigger waits",
    "an ability resolved this turn",
    "an exiled card may be played this turn",
    "a once-each-turn action was done",
    "a graveyard card was discarded this turn",
    "a spell will be exiled instead of put into a graveyard",
    "a permanent was exhausted (an exhaust ability)",
    "a colour was chosen as a permanent entered",
    "a creature was exerted",
    "the last normal turn was the other player's",
    "an additional combat phase follows",
    "colorless mana for artifacts only",
    "red mana that gives haste",
    "a card drawn this turn",
    "a noncreature spell cast this turn",
    "gained life this turn",
    "can't gain life",
    "a creature attacked this turn",
    "a card drawn in the draw step",
    "a spell cast this game",
    "may play from their graveyard this turn",
    "cards to their graveyard are exiled this turn",
    "the city's blessing",
    "protection from everything until their next turn",
    "a permanent entered evoked",
    "a permanent escaped",
    "a player was chosen as a permanent entered",
    "a creature is monstrous",
    "a spell's mode is another",
    "a trigger's amount is another",
    "a spell was cast for an alternative cost",
    "a spell was cast from a graveyard with flashback",
    "a spell was cast with flashback",
    "a spell was cast from another zone",
    "a card exiled for delve",
    "a trigger noted a permanent",
    "divided damage is divided otherwise",
    "a ninjutsu creature will attack a planeswalker",
    "a card is spliced onto a spell",
];

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
        // (by game: ENGINE.md 142.8)
        for g in 0..3 {
            v.extend(m.earlier_by_game[p][g].iter().copied());
        }
    }
    v.push(m.sb.is_some() as u64);
    v.push(m.dealt.map_or(9, |d| d as u64));
    // (ENGINE.md 168.1: both seats dealt)
    if m.dealt_both {
        v.push(0xD2);
    }
    let _ = viewer;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ENGINE.md 142: the known-gap rule (`covered`) on hand-made pairs of states.
    #[test]
    fn known_gaps_cover_what_they_name_and_nothing_else() {
        let k = known_gaps();
        let c = |l: u8, a: &[&'static str], b: &[&'static str]| covered(&k, l, a, b).is_some();
        let (red, black) = ("a creature gains protection from red", "a creature gains protection from black");
        let (pump, m1) = ("+1/+1 until end of turn", "a -1/-1 counter on a creature");
        let sac = "a land gains \"sacrifice it: add {B}\"";
        let (group, cant) = ("creatures a player controls can't block this turn (a rules effect)", "a creature can't block");
        // (Layout v7 has no known gap since ENGINE.md 142.7; layout v6, mtg_eoe's, keeps them.)
        assert!(c(V6, &[red], &[black]) && !c(V7, &[red], &[black]), "two protections alike in v6 only");
        assert!(!c(V6, &[], &[red]), "a protection alone is encoded");
        assert!(c(V6, &[], &[pump, m1]), "a pump and a -1/-1 counter cancel");
        assert!(c(V6, &[red, pump], &[black, "+1/+1 with no duration"]), "two gaps at once");
        assert!(c(V6, &[], &[sac]) && c(V6, &[pump], &[pump, sac]), "an alone perturbation changes no row");
        assert!(c(V6, &[group], &[group, cant]), "a creature's can't-block under a group's");
        assert!(!c(V6, &[], &[cant]), "a can't-block alone is encoded");
        assert!(!c(V6, &["life + 1"], &["a stun counter"]), "unrelated changes are no known gap");
        assert!(!c(V6, &[red], &[red]), "the same perturbations: no difference to cover");
        assert!(c(V6, &[], &["a permanent was harnessed"]) && !c(V7, &[], &["a permanent was harnessed"]), "a v7 fact: a v6 gap only");
        // (layout v7's one known gap: the framed row's, found 2026-11-13, a decision of its own)
        assert!(k.iter().filter(|g| g.layouts & V7 != 0).count() == 1, "layout v7's known gaps");
    }

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
        for n in V5_FIXED.iter().chain(V6_FIXED.iter()).chain(V7_FIXED.iter()) {
            assert!(ps.iter().any(|p| p.name == *n), "V5_FIXED / V6_FIXED: {n} names no perturbation");
            assert!(!limits().iter().any(|l| l.name == *n), "V5_FIXED / V6_FIXED: {n} is still a limit");
        }
    }
}
