# The MTG Zero rules engine (`engine` crate)

Status: M4a (Stage 0 core), 2026-10-21. This is the architecture every later card port
follows. DESIGN.md section 3 says *why* (rules-correct, every choice an action, cards are data,
the loop in Rust, Rust tests); this file says *how*. "CR" is the Comprehensive Rules; where the
Python engine (`mtg-bot/src/mtg_bot`) behaves differently, section 14 lists the difference and
the engine follows the CR.

## 1. Shape

```
card_ir (generated serde types)  --lower-->  CardDb (engine CardDefs, static)
                                                  |
Game (all mutable state, Clone)  <-- reads ------+
  pending_decision() / legal_actions() / apply(action, &mut chance_keys)
```

| Module | What |
|---|---|
| `ids.rs` | `PlayerId`, `CardIdx`, `ObjRef`, `Zone`, `Target` |
| `db.rs` | `CardDef`, `AbilityDef`, `CardDb` (build from an IR document, look-ups) |
| `lower/` | IR -> `CardDef`: `printing.rs` (every `CardPrinting` field), `effect.rs` (effects -> ops), `filter.rs` (predicates), `support.rs` (the per-card report) |
| `program.rs` | `Op` (the effect instruction set), `Amount`, `Who`, the program builder |
| `mana.rs` | `ManaCost`, `ManaPool`, colour masks, payment feasibility |
| `game.rs` | `Game` (the state), `Obj`, `Player`, `GameSpec`, setup, zone moves |
| `flow.rs` | the agenda (task stack) runner: turn structure, priority, steps |
| `decision.rs` | `Decision`, `Choice`, `DecisionKind`, the action-id layout |
| `cast.rs` | casting (CR 601), costs, `PayMana` |
| `resolve.rs` | resolution (CR 608) and the effect VM |
| `combat.rs` | CR 506-511 |
| `sba.rs` | CR 704 |
| `triggers.rs` | events, trigger matching, putting triggers on the stack (CR 603) |
| `hidden.rs` | knowledge bits, chance keys, `determinize` |
| `rng.rs` | xoshiro256++ / splitmix64 (the same generators as zero-core) |
| `testkit.rs` | scenario builder and helpers for tests (public; not used by the engine) |

No dependency beyond `card_ir`, `serde`, `serde_json`. No `HashMap` inside `Game` (iteration
order must be deterministic); no threads, callbacks or interior mutability.

## 2. Objects, zones, players and ids

* `PlayerId = u8` (0, 1). Two players only for now; nothing below assumes more except the
  places marked "2p" (APNAP order, "the opponent").
* **`CardIdx(u16)`** indexes `Game.objs`, one slot per physical card for the whole match
  (cards 0..39 belong to player 0's deck, 40..79 to player 1's). Tokens and spell copies will
  append slots; a slot is never reused, so `CardIdx` is stable.
* **`ObjRef { idx, inc }`** names an *object* (CR 109). Each zone change bumps the slot's
  incarnation `inc` (CR 400.7: the object that moved is a new object), so a reference held by
  a target, a trigger or an effect goes stale automatically. `game.live(r)` is the one check.
* `Obj` (per slot): `def` (the `CardDef` id), `owner`, `controller`, `zone`, `inc`,
  `timestamp` (CR 613.7, refreshed on entering the battlefield), `tapped`, `damage`,
  `deathtouched` (CR 704.5h), `control_since` (turn number; summoning sickness, CR 302.6),
  combat flags (`attacking`, `blocking: Option<CardIdx>`, `blocked`, `struck_first`), `known`
  (bit p = player p knows the card's identity; meaningful in hidden zones, section 10).
  Per-object state that later features add (counters, attachments, face-down, modifiers)
  goes here too, and must reset in `move_card` when the object changes zones.
* Zones: per player `library` (index 0 = bottom, last = top), `hand`, `graveyard` (last =
  top); shared `battlefield` (always in timestamp order = the canonical permanent order),
  `exile`, `stack: Vec<StackItem>` (last = top). `Obj.zone` mirrors membership; the fuzzer
  checks both agree and that every card is in exactly one zone.
* **All zone changes go through `move_card(idx, to, position)`**: it removes the card from the
  old list, bumps `inc`, resets per-object state, sets controller = owner outside the
  battlefield and stack, assigns a timestamp on the battlefield, updates knowledge and emits
  the reveal key (section 10) and the zone-change event (section 8). Nothing else edits zone
  lists.
* Characteristics (types, power, toughness, keywords, colours) are read only through
  accessors on `Game` (`power(idx)`, `has_kw(idx, kw)`, ...). In M4a they return the printed
  values; layers (CR 613) will slot in behind these accessors without touching callers.

## 3. Card definitions and the IR

* `CardDb` holds `CardDef`s (static after load) and is shared as `&'static CardDb`
  (`CardDb::leak`): zero-core needs `Game: Clone + Send + Sync + 'static`, and an `Arc` would
  put an atomic refcount on every clone of every search thread.
* A `CardDef` is the *lowered* card: interned name, type and supertype bit sets, interned
  subtypes, colours, `ManaCost`, mana value, power/toughness, a keyword bit set, intrinsic
  mana abilities (from `produces`), the spell ability (instants and sorceries), triggered
  abilities, and later activated and static abilities.
* An **`AbilityDef`** is `{ targets: Vec<TargetReq>, program: Vec<Op> }`. A `TargetReq` is a
  `Filter` plus `min`/`max` counts. An ability is addressed by `AbilityRef { def, slot }`
  (`slot` = spell, trigger i, activated i), which is `Copy` and is all a stack item or a
  frame needs to find its program.
* **Lowering** (`lower/`) turns `card_ir` values into `CardDef`s. Rules:
  1. A card is supported only if the lowering claims every part of it. Every generated node
     struct it accepts is destructured **exhaustively** (no `..`), so a new IR field is a
     compile error until someone decides what it means. Each field is either used, or checked
     to be at its default with `deny(...)`, which records a blocker such as
     `CardPrinting.enters_tapped`.
  2. An unknown category variant records `Effect:CounterThenDelayedManaEqualToMV`,
     `ObjectPredicate:IsSelf`, `Keyword:prowess`, `Event:attacks`, ...
  3. All blockers of a card are collected (not just the first), so the support report
     (`support::report`) lists, per card, everything that stands in the way, and per
     feature how many cards it blocks alone. That is the staging tool for M4b onwards.
  4. Many IR classes may lower to the same ops (`DealDamageAnyTarget`,
     `DealDamageToTargetCreature` -> `Op::DealDamage`). New IR classes should be expressed
     with existing ops whenever the CR semantics match; a new op is added only for a new
     rules action, never for a card.
  5. String-typed IR slots (`unless_pays: "{3}"`) are parsed during lowering; an unparseable
     value is a blocker, never a default.
* Unsupported cards stay out of `CardDb`'s playable set; asking for one by name is an error.

## 4. The agenda: one explicit continuation stack

Everything the engine does between two decisions is a `Task` on `Game.agenda` (a `Vec`, top =
last). `advance()` pops and runs tasks until a decision is pending or the game is over:

```
loop { if pending or over: return; let t = agenda.pop() or Task::NextStep; run(t) }
```

Tasks cover the turn structure (`BeginStep`, `Priority`, `EndStep`), casting stages,
`PayMana`, `Resolve`/`Run(Frame)`/`FinishResolve`, SBAs, putting triggers on the stack, combat
declarations and damage, cleanup discards and mulligans. A task that needs a choice follows the
**ask-or-consume** pattern:

* run with `answer == None`: compute the options, push itself back, set `pending`, return.
  Before asking, a task may only do idempotent work (re-running it must ask the same
  question), because `determinize` re-runs the asker to refresh the options (section 10);
* run with `answer == Some(choice)`: consume it, do the work, push follow-up tasks.

`apply(action)` validates the action against the pending options, stores the matching
`Choice` in `answer` and calls `advance()`. Because the asker is on top of the agenda, it is
the next task to run. The whole continuation (agenda, frames, pending decision) is plain data,
so `Game: Clone` captures a game suspended anywhere, including in the middle of a resolving
spell or a combat damage assignment. There are no callbacks, closures or threads.

`GameSpec.auto_single` (default true): a decision with exactly one option is answered
internally instead of being exposed. zero-core would auto-step it anyway; doing it inside
`apply` saves a `step` round trip.

## 5. Turn structure and priority (CR 500-514, 117)

* Steps: untap, upkeep, draw, main 1, beginning of combat, declare attackers, declare
  blockers, combat damage (a first-strike damage step is inserted when some attacking or
  blocking creature has first or double strike, CR 510.4), end of combat, main 2, end,
  cleanup. Mana pools empty at the end of every step (CR 500.4).
* Turn-based actions: untap (CR 502.3, no priority), draw (CR 504.1; the starting player skips
  the first draw, 103.8a), declare attackers / blockers (508.1, 509.1), combat damage (510),
  remove from combat (511.3), discard to hand size and damage wear-off (514.1-2). No player
  gets priority in untap and cleanup (514.3: unless an SBA or trigger happens, then priority
  and another cleanup step, 514.3a). With no attackers the declare-blockers and damage steps
  are skipped (508.8).
* **Priority.** `turn.priority` and `turn.passes` (consecutive passes). The `Priority` task
  first loops "check SBAs; put waiting triggers on the stack" until neither happens (117.5,
  704.3, 603.3), then asks the priority player. Options: pass, play a land (special action,
  116.2a / 305.2: own main phase, empty stack, land drops left), cast a spell (sorcery timing
  for non-instants without flash: 307.1 / 301.1), later activate an ability.
  * After a player casts, activates or plays a land, **that player receives priority again**
    (117.3c) and `passes` resets.
  * A pass gives priority to the next player (117.3d). When all players have passed in
    succession (117.4): stack empty -> the step ends; otherwise the top object resolves and
    the **active player** then receives priority (117.3b).
* Mana abilities are activated only while paying a cost (601.2g / 117.1d), never as a
  free-standing priority action: floating mana with nothing to spend it on cannot matter.
  Mana already in a pool (Dark Ritual) is spent automatically and first when a cost is paid
  (it empties at the end of the step anyway). Recorded simplification, section 13.
* Game-length caps: `GameSpec.max_turns` (default 60) ends the game as a draw with reason
  `TurnLimit`; an internal step budget turns an engine loop into a panic, not a hang.
* Extra turns (CR 500.7): a stack of players; the most recently created extra turn is taken
  first.
* Setup (CR 103): libraries shuffled from the seed, the starting player chosen from the seed
  (no play/draw choice yet), seven cards each, then the **London mulligan** (103.5):
  in turn order each player who has not kept declares keep or mulligan; each mulliganing
  player shuffles their hand away, draws seven, then puts one card per mulligan taken on the
  bottom (one decision per card; the pick order is the bottom order). Repeat until all keep.
  `GameSpec.mulligans = false` skips it.

## 6. Casting, costs and the stack (CR 601, 602, 405, 608)

* Casting a spell (601.2): the card moves to the stack immediately as a `StackItem { id,
  kind: Spell(card), controller, targets, ... }` (601.2a); targets are chosen one decision per
  target (601.2c; a spell cannot target itself, 115.5; the same object can be chosen once per
  "target" word, 115.3); the total cost is fixed (601.2f); mana abilities are activated and
  the cost paid (601.2g-h, `PayMana`); the spell becomes cast (601.2i, cast triggers).
* A cast is offered only if it can be completed: timing is legal, every required target has
  enough legal candidates, and the cost is payable from the pool plus untapped mana sources.
  So casting never needs to be rewound (601.2 allows rewinding illegal casts; we never start
  one). The same rule applies to activated abilities later.
* **`PayMana`** (601.2g-h). The pool is applied first (coloured pips by colour, then generic).
  The rest is paid by choosing which mana sources to tap, one decision per source: options
  are one representative per class of equivalent sources (same definition and same
  per-object state), restricted to classes whose addition keeps the cost payable, and in
  non-decreasing class order so each *set* of sources is reachable exactly once (no
  permutation branching). When the chosen sources cover the cost the engine taps them, adds
  their mana and pays; colours are assigned by matching, because which permanents are tapped
  is the only thing that matters strategically. Feasibility is Hall's condition over the
  colour classes of the remaining pips (`mana::payable`, at most 64 subsets).
* Targets are re-checked on resolution (608.2b): a target is legal if its object still exists
  (same `ObjRef` incarnation, expected zone) and still matches the `TargetReq` filter. If
  every target of a targeted spell or ability is illegal, it does not resolve and leaves the
  stack ("fizzles"; the card goes to its owner's graveyard). Otherwise the effect skips the
  illegal targets.
* Resolution keeps the item on the stack until it finishes (608.2n): `Resolve` checks targets
  and pushes `FinishResolve { id }` and `Run(Frame)`; `FinishResolve` removes the item by id,
  moves an instant or sorcery to its owner's graveyard if it is still on the stack, puts a
  permanent spell onto the battlefield (608.3), and gives priority to the active player.
* Countering (701.5) removes a spell from the stack to the destination its effect names
  (graveyard; library top for Memory Lapse).

## 7. The effect VM: resumable resolution

An ability's program is a flat `Vec<Op>` with forward jumps (lowered from the IR's tree:
`Sequence` concatenates, conditionals and "unless" become jumps). It runs in a `Frame`:

```
Frame { ability: AbilityRef, pc: u16, source: ObjRef, source_def, controller,
        targets: Vec<(req, Target)>, legal: u64 /* targets legal at resolution start */,
        i: u8 /* per-op loop counter, reset when pc moves */ }
```

`Task::Run(frame)` executes ops from `pc` until the program ends or an op needs a choice. Such
an op uses ask-or-consume: it pushes `Run(frame)` back with the same `pc` (and its loop
counter in `frame.i`) and asks; on resume it consumes the answer and moves on. Ops that need a
sub-process (paying mana in "counter unless its controller pays {3}") push the frame with its
continuation `pc` first and the sub-process task above it. Values an effect must remember
across ops (last known information of a removed target for "its controller gains life equal
to its power") will be frame registers filled by an explicit `Remember` op (M4b). Examples:

| Op | Choice | CR |
|---|---|---|
| `DealDamage { amount, to }` | - | 120 |
| `Draw { who, n }` | - (keys) | 121 |
| `Counter { tgt, dest }` | - | 701.5 |
| `OfferPay { who, cost, if_paid }` | yes/no, then `PayMana` | 118.12 |
| `PutFromHandOnTop { who, n }` | one hand card per iteration | (Brainstorm) |
| `DiscardRandom { who, n }` | - (keys) | 701.9 |
| `AddMana { who, mana }` | - | 106.4 |
| `Destroy { tgt }` / `Exile { tgt }` | - | 701.7 / 406 |
| `ExtraTurn { who }` | - | 500.7 |

`Who` resolves a player (`You`, `TargetPlayer(i)`, `ControllerOfTarget(i)`, ...); `Amount` is
`Fixed(n)` now and grows (`X`, counts, LKI values). Ops reference targets by index into the
ability's `targets`. No op is named after a card.

## 8. Events and triggered abilities (CR 603)

* `move_card` and the other primitive actions call `self.event(Event)`; `Event` records what
  happened with last-known information (definition, controller) so leaves-the-battlefield
  triggers can look back (603.10a). M4a events: enters the battlefield, dies; the matcher is
  generic over `(event kind, trigger filter)` and later kinds plug into the same place.
* A matching trigger becomes a `PendingTrigger { controller, source, source_def, slot }` in
  `Game.waiting`. Nothing is put on the stack until a player would receive priority (603.3).
* `PutTriggers` (run by the `Priority` loop): **APNAP** (603.3b): the active player puts all
  their waiting triggers on the stack in the order they choose (one "which next" decision
  while two or more distinct triggers remain), then the other player; so the non-active
  player's triggers resolve first. Targets are chosen as each goes on the stack (603.3d); a
  trigger with no legal choice for a required target is removed.

## 9. State-based actions (CR 704)

`Sba` runs when a player would receive priority (704.3) and repeats until nothing happens.
One pass collects every applicable action and performs them simultaneously: 704.5a (life <= 0),
704.5b (drew from an empty library since the last check), 704.5f (toughness <= 0), 704.5g
(lethal damage), 704.5h (deathtouch damage), 704.5j (legend rule: the controller **chooses**
which legendary permanent with the same name to keep, one decision per group, before the
simultaneous move), 704.5d (tokens outside the battlefield; tokens come in M4b). Losing ends
the game at once; if all players lose simultaneously the game is a draw (104.4a). Later SBAs
(poison, planeswalker loyalty, auras, +1/+1 and -1/-1 counters) are new arms of the same pass.

## 10. Randomness, chance keys and hidden information

* **The RNG lives in the state** (`Rng`, xoshiro256++ seeded by splitmix64): shuffles, random
  discards, the starting player. Same seed + same actions = same game, bit for bit (the
  fuzzer checks it).
* Library order is materialised (shuffles permute `library`), so a draw is "pop the top".
* **Knowledge.** `Obj.known` has a bit per player that knows the card's identity while it is
  in a hidden zone: a drawn card is known to its owner; a card put from hand on top of the
  library stays known to its owner; a shuffle clears the bits of the library's cards;
  public zones are known to all.
* **Chance keys** (`u64`, pushed to the `chance` vector of `apply`) are emitted whenever a
  transition reveals something: `Draw(player, def)` (private to the drawer),
  `Reveal(player, def)` when a card moves from a hidden to a public zone (casting or playing
  it, discards: this is what tells the tree *which* card an opponent's "hand slot 3" was),
  `RandomDiscard(player, def)`. Layout: kind in bits 56-63, player in bits 48-55, a
  per-event payload (the definition id) in the low bits. Shuffles emit nothing (they reveal
  nothing); the next draw is keyed.
* **Observer masking.** zero-core keys children by chance keys, and the search's tree is the
  root decider's. `determinize(viewer)` records `observer = viewer` in the state; from then
  on a private reveal to another player is emitted with the definition replaced by
  `HIDDEN`, so the viewer's tree does not branch on facts the viewer cannot see. Without an
  observer (real play, tests) keys are complete.
* **`determinize(viewer, rng)`** resamples what the viewer does not know: for each owner, the
  identities (`def`) of that owner's cards in hidden zones (the other player's hand; both
  libraries) whose `known` bit for the viewer is clear are shuffled among themselves; known
  cards (e.g. the two Brainstorm put back) stay in place. It reseeds the state's RNG and sets
  the observer. It never touches public state or the viewer's own hand, so the viewer's
  legal actions are unchanged (tested, and asserted by zero-core every simulation).
  If the pending decision belongs to the *other* player, its options are refreshed by
  re-running the asker (section 4), so they never refer to a stale identity. The viewer's
  own decision needs no refresh because of one rule every new decision kind must keep:
  **a choice of a card in a zone hidden from its decider names the card's definition, not
  its slot** (a library search offers "a Forest", not "card 17"; the decider's hand is never
  resampled for them). Debug builds re-run the asker anyway and assert the options are equal.
  Assumption for now: both decklists are known (open decklists); team drafts will need
  partial pool knowledge (M6).
* `force_chance` (zero-core `chance_k`) is not implemented yet (M5): a forced draw would move
  the requested identity to the top of the unknown part of the library.

## 11. Decisions and the action-id layout

One decision is pending at a time: `Decision { player, kind, options: Vec<Opt> }`, each
`Opt = { action: u16, choice: Choice }`, sorted by `action`, no duplicates, never empty while
the game is running. `legal_actions` copies the ids; `apply` binary-searches them. The ids are
**decision-relative slots** (DESIGN.md 3.4): the same id means "the same kind of thing" for a
network, interpreted under the pending `DecisionKind`. Object slots are canonical and relative
to the decider:

| Range | Ids | Meaning |
|---|---|---|
| `PASS` | 0 | pass priority / no / keep / done / no block / decline |
| `YES` | 1 | yes / take a mulligan / pay |
| `PLAYER` | 2-3 | 2 = the decider, 3 = the opponent |
| `NUMBER` | 4-35 | n = 0..31 (damage amounts; later X, modes) |
| `HAND` | 36-99 | decider's hand slot 0..63 (cast, play, choose, discard, bottom) |
| `MY_PERM` | 100-195 | decider's permanents, slot 0..95 (attack, block with, tap for mana, target) |
| `OPP_PERM` | 196-291 | opponent's permanents, slot 0..95 (target, block this attacker) |
| `STACK` | 292-323 | stack slot 0..31, 0 = top (target spell) |
| `TRIGGER` | 324-339 | decider's waiting triggers, slot 0..15 (order triggers) |
| `LIBRARY` | 340-403 | cards of a searched / looked-at pile, slot 0..63 (M4b) |
| `GRAVEYARD` | 404-531 | decider's 0..63, opponent's 64..127 (M4b) |
| `ACTIVATE` | 532-915 | `MY_PERM` slot * 4 + ability index (M4b) |
| reserved | 916-1023 | alternative costs, modes, ... |

`NUM_ACTIONS = 1024`. Canonical orders: hand = sorted by (definition, card index); permanents
= timestamp order, split by controller; stack = top first. **Equivalent options are offered
once**: hand cards with the same definition (hidden-zone cards have no other state), mana
sources of the same class. Targets are not deduplicated yet (it needs an exact "same
observable state" key over every per-object field; an optimisation for later).

Decisions by kind (M4a): `Priority`, `Mulligan`, `MulliganBottom`, `Target`, `PayMana`,
`DeclareAttackers` (one creature per decision in increasing slot order, `PASS` = done: each
attacking set exactly once), `DeclareBlockers` (one decision per potential blocker in slot
order: which attacker, or `PASS`), `AssignDamage` (amount per blocker in order), `YesNo`,
`ChooseHandCard` (Brainstorm), `Discard` (cleanup), `OrderTriggers`, `LegendRule`.

Mapping to zero-core's `SearchGame` (M5): `to_move` = the pending decision's player;
`legal_actions` = the ids; `step` = `apply`; `result` from `outcome()`; `determinize` as
above; `NUM_ACTIONS = 1024`.

## 12. Combat (CR 506-511)

* Declare attackers (508.1): untapped creatures of the active player that have been under
  their control since the start of their most recent turn (302.6) or have haste, without
  defender; each attacks the defending player (planeswalker and battle attack targets later).
  Attackers without vigilance tap (508.1f).
* Declare blockers (509.1): each untapped creature of the defending player may block one
  attacker it can block: flying needs flying or reach (702.9b); menace needs two or more
  blockers (702.110b; options that would leave an unfixable single blocker are not offered,
  checked by a small search over the remaining blockers).
* Combat damage (510): the attacking player divides each attacker's damage among its blockers
  as they choose (510.1c, current rules: no damage assignment order); with trample, lethal
  damage must be assigned to every blocker before the rest may go to the player (702.19c);
  deathtouch makes 1 damage lethal (702.2c). A blocked creature whose blockers are gone deals
  no damage unless it has trample. All combat damage is dealt simultaneously (510.2);
  lifelink gains life at the same time (702.15b). First strike / double strike: 510.4 and
  702.4 / 702.7 (a creature deals damage in the second step only if it has double strike or
  did not deal first-strike damage).

## 13. Scope of M4a and what is deliberately stubbed

Implemented: setup with London mulligan, the full turn, priority, land play, intrinsic mana
abilities (basic land types and `produces`), casting with mana costs and targets, the stack,
resolution with re-checked targets, the effect VM, ETB and dies triggers with APNAP order and
targets, SBAs listed in section 9, combat with flying, reach, first strike, double strike,
deathtouch, trample, lifelink, vigilance, haste, defender, menace, flash; game end; chance keys;
`determinize`.

Not yet (later milestones; each is a blocker in the support report, never a silent no-op):
layers and continuous effects, replacement and prevention effects, alternative and additional
costs (kicker, flashback, Force of Will), X costs, hybrid and Phyrexian mana, activated
abilities other than intrinsic mana abilities, tokens, counters, planeswalkers and battles,
copies, delayed triggers, hexproof / shroud / protection / indestructible, mana abilities as
free-standing priority actions, the play/draw choice, `force_chance`, `encode`.

Deliberate simplifications and limits (each is small and local to change):

* Mana already in a pool is spent first and automatically; generic pips take colourless
  first, then the most plentiful colour (section 5).
* Events recorded: enters the battlefield and dies. Cast, attack, block, tap and draw events
  get recorded when the first card listens for them (`triggers.rs`, one place).
* Waiting triggers with the same definition and ability are offered once in the ordering
  decision (their order cannot matter while effects ignore their source's per-object state).
* Combat damage amounts use the `NUMBER` range (0..31); a larger amount to divide is an
  assertion failure for now.
* Creatures attack only players; the starting player is chosen from the seed; determinize
  assumes open decklists.
* Targets are not deduplicated (section 11).

## 14. Differences from the Python engine (CR followed)

* Priority after a non-active player casts: Python returns priority to the active player; the
  CR (117.3c) keeps it with the caster.
* Triggers: Python has no APNAP order and no controller ordering choice; here 603.3b with a
  decision.
* Combat damage among several blockers: Python assigns in battlefield order; here the
  attacking player divides it (510.1c).
* Every in-resolution choice (targets, "may", "unless pays", hand-card picks, legend rule,
  mana sources) is a player decision, not "first legal" or "always yes".
* The Python engine's "any land pays one generic" fallback has no counterpart: lands pay only
  through their mana abilities (so `mana_only_from_produces` needs no code here).

## 15. Tests

`cargo test -p engine` runs everything below in about 5 s (debug). Rules tests name the CR
rule they check in the test name. Custom test cards (`tests/common/mod.rs`, "Test Bear",
"Test Striker", ...) are written as card IR and go through the same lowering as cube cards;
they isolate one keyword or trigger each.

| File | What |
|---|---|
| `tests/rules_setup.rs` | 103 setup, 103.5 London mulligan |
| `tests/rules_priority.rs` | 117.3a-d, 117.4, 405.5 (LIFO), 305.2, 307.1 / 702.8 timing, 115.5, 608.2b fizzle, 500.4, 103.8a |
| `tests/rules_sba.rs` | 704.5a, b, f, g (+ 514.2), h, j (legend rule choice), 104.4a, turn cap |
| `tests/rules_combat.rs` | 302.6 / 702.10, 508.1f / 702.3b, 509.1b, 702.110b, 510.1c, 702.19c, 702.4, 702.7, 702.15, 508.8, 506.4 / 702.19e |
| `tests/rules_triggers.rs` | 603.3, 603.3b (APNAP, ordering decision), 603.3d (targets; removal without target), 603.10a |
| `tests/cards.rs` | the cube cards: Mana Leak (pay / decline / cannot pay), Memory Lapse, Counterspell, Spell Pierce, Brainstorm (mid-resolution choices, a clone taken mid-resolution), Ancestral Recall, Dark Ritual + Hymn (pool, keyed random discard), Time Walk, Cut Down / Flame Slash filters, Cogwork Librarian, mana-source classes |
| `tests/hidden.rs` | determinize keeps the viewer's view and actions; known cards stay; observer masking of keys; refreshed decisions of the other player |
| `tests/support.rs` | the support report, the exact supported list, loud failures |
| `tests/fuzz.rs` | random play (below) and replay determinism |
| `tests/perf.rs` | the numbers in section 17 (`--release -- --ignored --nocapture`) |

**Fuzzer.** Uniform random actions in complete games between random 40-card decks (17 lands,
23 spells drawn with repetition from the supported cards and the test cards, so duplicates,
legends and combat keywords are common). After every action: no panic; every card in exactly
the zone list its `zone` says, once (card count conserved); battlefield in timestamp order;
legal actions sorted, unique, non-empty, `< NUM_ACTIONS`; no chance keys left in the state.
Every ~40 actions: two clones given the same action stay identical (state hash and keys), and
`determinize(to_move)` keeps the decider's legal actions and the board, then the resampled
world plays on 30 random actions under the same checks. Every game must end (60-turn cap).
Separately, 30 games are replayed from their seed and action list and must reproduce the
final state hash, the outcome and the chance-key sequence exactly. `FUZZ_GAMES=n` sets the
game count (default 300; 20,000 in release took 28 s, no failure).

## 16. Porting a card (the recipe for M4b onwards)

1. `cargo test -p engine --test support -- --nocapture` lists blocked cards, the features
   that block most cards on their own, and a greedy staging order (the feature completing
   the most cards next). Some blockers are coarse: `CardPrinting.activated_abilities`,
   `static_abilities`, `alternative_costs` deny a whole sub-structure until its lowering
   exists, after which the report names the cost atoms and effects inside instead. At M4a
   the greedy order starts activated abilities (80 cards complete, 31 -> 111), lands that
   enter tapped unless / shock lands (-> 135), loyalty, modal spells, statics.
2. An IR effect class: add an arm to `lower/effect.rs` that destructures every field of its
   node (no `..`), lowers onto existing ops and `deny`s whatever it does not implement. A
   `CardPrinting` flag: remove its `deny` line in `lower/printing.rs` and implement it where
   it belongs (casting, SBAs, combat, ...). A predicate: `lower/filter.rs` + `Filter`.
3. Only if no op expresses it: add an `Op` named after the rules action (never the card) in
   `program.rs`, implement it in `resolve.rs::exec`, cite the CR. An op that needs a choice
   uses ask-or-consume with a `DecisionKind` and the action ranges of section 11; choices of
   hidden cards name definitions (section 10).
4. Tests: a card test in `tests/cards.rs` (drive with `auto_single = false` and
   `drive_until` to stop exactly where you look), a custom IR test card when a rule needs
   isolating, the card in `tests/support.rs::SUPPORTED` and in the fuzz pool
   (`tests/common/mod.rs`). Then `FUZZ_GAMES=20000 cargo test --release -p engine --test fuzz`.

## 17. Measurements

Desktop (i9-13900K, Windows 11, other work running), release build, one thread,
`tests/perf.rs`; random decks as in the fuzzer, mulligans on, `auto_single` on.

| Measure | Value |
|---|---|
| Random play | 1,357 games/s (4,000 games, setup included) |
| Per exposed decision (`apply`, running to the next decision) | 4.4 us (169 decisions, 36 turns per game on average) |
| `Game::clone` (mean over 4,778 mid-game states) | 1.04 us; `size_of::<Game>()` = 560 B + ~10 heap blocks |
| `clone` + `determinize(to_move)` (one search simulation's setup) | 2.4 us |
| `legal_actions` (copies cached ids) | 6 ns |

The clone cost is mostly the ~10 heap allocations (cards, six zone lists, battlefield,
agenda, options) under the Windows system allocator; packing the zones into one array or a
faster allocator would cut it if it ever matters (the network evaluation per simulation will
cost far more). For comparison the old crate measured 0.35 us per clone with copy-on-write
zones but paid for it on first write, and its game loop ran in Python.
