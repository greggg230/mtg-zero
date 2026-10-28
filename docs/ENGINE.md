# The MTG Zero rules engine (`engine` crate)

Status: D1 (constructed play beside the cube: the Legacy Izzet Delver mirror's pinned 60, its two
cards outside the cube in a card table of their own, CR 100.2a decklists in best-of-one, the preset
`delver-mirror`; section 54), 2026-10-28; M8d part 2 (interaction confidence: the riskiest rulings left and the interaction matrix's cells tested, a combo-dense fuzz mode,
seven engine fixes; section 38), 2026-10-26; M8d part 1 (the cube's official rulings as engine tests: 1,607 fetched and triaged, the risky
interaction areas tested, three engine fixes; section 37), 2026-10-26; M7d's IR fixes (the IR of mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: the last 9 cards; 546 supported cube cards, all of
the cube; reflexive triggers with an intervening "if", "this turn" triggered abilities, a spell
shuffling itself into its library, mana abilities with {X}; section 32), 2026-10-25; M7d (the last
cards that needed subsystems: face-down permanents and manifest dread, plot,
an opening-hand action, shadow and void counters, reconfigure, Class levels, a granted target
trigger with a limit, a card name chosen as it enters, a mana ability that taps another
permanent, exert and haste mana, emerge, the top card looked at and cast for life, a granted
sacrifice mana ability used with the land's own; 537 supported cube cards, the 9 others blocked by
IR defects; section 31), 2026-10-24; M7c tail part 3 (face-down exile, splice, kicker on
permanents, reanimating Auras and more; 524 supported cube cards; section 30), 2026-10-24; M7c
tail part 2 (cascade, miracle, warp, boast, ascend, airbend and waterbend, harness,
teamwork, additional combat phases, Yawgmoth's Will, X in activation costs and "X targets", X
life, "triggers only once each turn"; 491 supported cube cards; section 29), 2026-10-24; M7c tail
part 1 (mana abilities with a mana cost or a choice, exhaust, spending
restrictions, converge, a painted colour, incubate, turn skipping, granted leave triggers, a
target linked to another's controller; 467 supported cube cards; section 28), 2026-10-23; M7b
part 3b (spells and abilities on the stack as targets, "becomes the target", ward,
improvise, the initiative and Undercity, abilities activated from graveyards, storm, mobilize,
the cost-order fixes; 438 supported cube cards; section 27), 2026-10-23; M7b part 3 (copies: copy
effects, token copies, copies of spells, entering as a copy; squad, offspring, replicate, spree,
bestow, prepare; 410 supported cube cards; section 26), 2026-10-23; M7b part 2 (cards with more than one face: transforming and modal double-faced cards,
Adventures, Sagas, day and night, a split card; 394 supported cube cards; section 25),
2026-10-23; M7b part 1 (planeswalkers: loyalty abilities, attacking planeswalkers, damage to
planeswalkers, emblems and the command zone; 378 supported cube cards; section 24),
2026-10-22; M7a (threats first: layers and continuous effects, counters, generic triggers,
protection and combat restrictions, alternative costs, X, kicker, delve, casting from
graveyards and exile, linked exile, delayed and reflexive triggers, the monarch; 360
supported cube cards; section 23), 2026-10-22; M5b a token network and stable card identities (section 22); M5a
the zero-core game `mtg` (`SearchGame`, `force_chance`, `encode`, decks, bots, the `mtgz`
package and a first self-play run; section 21); M4b-2 added hidden
information (library searches, looking at libraries and hands, partial knowledge; modal
spells; tokens); M4b-1 activated abilities, costs, mana abilities and lands; M4a built the
Stage 0 core. This is the architecture every later card port follows. DESIGN.md section 3 says *why* (rules-correct, every choice an action, cards are data,
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
| `lower/` | IR -> `CardDef`: `printing.rs` (every `CardPrinting` field), `activated.rs` (activated, mana and loyalty abilities, costs), `effect.rs`, `effect_ext.rs`, `effect_walkers.rs` (effects -> ops), `filter.rs` (predicates, card filters, conditions), `statics.rs` (static abilities, replacement effects), `support.rs` (the per-card report) |
| `program.rs` | `Op` (the effect instruction set), `Amount`, `Who`, the program builder |
| `mana.rs` | `ManaCost` (generic, coloured, hybrid, Phyrexian), `ManaPool`, colour masks, the matching (`payable`) and budgets |
| `game.rs` | `Game` (the state), `Obj`, `Player`, `GameSpec`, setup, zone moves |
| `flow.rs` | the agenda (task stack) runner: turn structure, priority, steps |
| `decision.rs` | `Decision`, `Choice`, `DecisionKind`, the action-id layout |
| `cast.rs` | casting (CR 601), additional costs, targets and filters, priority options |
| `activate.rs` | activated abilities (CR 602): options, activation, non-mana costs, free-standing mana abilities, conditions, "as enters" choices (section 18) |
| `pay.rs` | paying mana (601.2g-h, 605): mana sources, feasibility, `PayMana` (section 18) |
| `resolve.rs` | resolution (CR 608) and the effect VM |
| `resolve_ext.rs`, `resolve_walkers.rs` | the `Ext` instructions of M7a (section 23.8) and M7b (section 24) |
| `combat.rs` | CR 506-511 |
| `companion.rs` | M8a (section 35): the setup after the play-or-draw choice (CR 103), companions (the reveal, the special action), sideboard accessors |
| `sba.rs` | CR 704 |
| `triggers.rs` | events, trigger matching, putting triggers on the stack (CR 603) |
| `hidden.rs` | knowledge (exact bits and partial "clouds"), chance keys, `determinize` (section 10) |
| `rng.rs` | xoshiro256++ / splitmix64 (the same generators as zero-core) |
| `svec.rs` | `SVec`, a small vector of plain values with inline storage (the short lists built for every decision; section 17) |
| `testkit.rs` | scenario builder and helpers for tests (public; not used by the engine) |
| `az/` | the zero-core game `mtg` (section 21): `mod.rs` (`MtgAz`, the spec, the `SearchGame` impl), `encode.rs`, `decks.rs`, `bots.rs` |

No dependency beyond `card_ir`, `serde`, `serde_json`, and zero-core's `zero_search` (the
`SearchGame` trait, for `az/` only; the pinned submodule `third_party/zero-core`). No `HashMap` inside `Game` (iteration
order must be deterministic); no threads, callbacks or interior mutability.

## 2. Objects, zones, players and ids

* `PlayerId = u8` (0, 1). Two players only for now; nothing below assumes more except the
  places marked "2p" (APNAP order, "the opponent").
* **`CardIdx(u16)`** indexes `Game.objs`, one slot per physical card for the whole match
  (cards 0..39 belong to player 0's deck, 40..79 to player 1's). Tokens append slots, and so
  do copies of spells and the copies a prepared permanent makes (M7b part 3, `Game.copy_objs`);
  a slot is never reused, so `CardIdx` is stable. A token or copy that ceased to exist (CR
  704.5d, 704.5e) keeps its slot in `Zone::Nowhere`, in no zone list. Copy effects are kept in
  `Game.copies`, not in `Obj` (section 26.1).
* **`ObjRef { idx, inc }`** names an *object* (CR 109). Each zone change bumps the slot's
  incarnation `inc` (CR 400.7: the object that moved is a new object), so a reference held by
  a target, a trigger or an effect goes stale automatically. `game.live(r)` is the one check.
* `Obj` (per slot): `def` (the `CardDef` id), `owner`, `controller`, `zone`, `inc`,
  `timestamp` (CR 613.7, refreshed on entering the battlefield), `tapped`, `damage`,
  `deathtouched` (CR 704.5h), `control_since` (turn number; summoning sickness, CR 302.6),
  combat flags (`attacking`, `attacks`: the planeswalker it attacks, if any (M7b),
  `blocking: Option<CardIdx>`, `blocked`, `struck_first`), `known`
  (bit p = player p knows the card's identity where it is; meaningful in hidden zones, section
  10).
  Per-object state that later features add (counters, attachments, face-down, modifiers)
  goes here too, and must reset in `move_card` when the object changes zones.
* Zones: per player `library` (index 0 = bottom, last = top), `hand`, `graveyard` (last =
  top); shared `battlefield` (always in timestamp order = the canonical permanent order),
  `exile`, `command` (emblems, in creation order: CR 114, 408; M7b), `stack: Vec<StackItem>`
  (last = top). `Obj.zone` mirrors membership; the fuzzer
  checks both agree and that every card is in exactly one zone.
* **All zone changes go through `move_card(idx, to, position)`**: it removes the card from the
  old list, bumps `inc`, resets per-object state, sets controller = owner outside the
  battlefield and stack, assigns a timestamp on the battlefield, updates knowledge (exact bits
  and clouds) and emits the reveal key (section 10) and the zone-change event (section 8).
  Nothing else edits zone lists. A token is created by `create_token` (a new slot from
  `Zone::Nowhere` through `put_onto_battlefield`, so it enters like any permanent).
* Characteristics (types, power, toughness, keywords, colours) are read only through
  accessors on `Game` (`power(idx)`, `has_kw(idx, kw)`, ...). In M4a they return the printed
  values; layers (CR 613) will slot in behind these accessors without touching callers.

## 3. Card definitions and the IR

* `CardDb` holds `CardDef`s (static after load) and is shared as `&'static CardDb`
  (`CardDb::leak`): zero-core needs `Game: Clone + Send + Sync + 'static`, and an `Arc` would
  put an atomic refcount on every clone of every search thread.
* A `CardDef` is the *lowered* card: interned name, type and supertype bit sets, interned
  subtypes, colours, `ManaCost`, mana value, power/toughness, a keyword bit set, the spell
  ability (instants and sorceries), additional-cost options, triggered abilities, activated
  abilities (`activated`, mana abilities included, at most 4) and abilities activated from
  hand (`hand_abilities`, at most 1), how it enters the battlefield (`enters_tapped`,
  `shock_life`), and "can't be countered". Static abilities come later.
* An **`AbilityDef`** is `{ targets: Vec<TargetReq>, program: Vec<Op> }`. A `TargetReq` is a
  `Filter` plus `min`/`max` counts. An ability is addressed by `AbilityRef { def, slot }`
  (`slot` = spell, trigger i, activated i, hand i), which is `Copy` and is all a stack item or
  a frame needs to find its program. An `ActivatedDef` is a `Cost`, an effect (`Stack`
  program or `Mana` production) and its restrictions (section 18).
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
  4a. When the IR repeats a value (a removal's `target_filter` and its `targets`; a
     `DestroyTargetLand`'s `nonbasic_only` and its filter; the two branches of an
     `IfConditionElse` describing one target), the lowering checks that the copies agree
     and records a blocker if they do not.
  5. String-typed IR slots (`unless_pays: "{3}"`) are parsed during lowering; an unparseable
     value is a blocker, never a default.
* Unsupported cards stay out of `CardDb`'s playable set; asking for one by name is an error.
* Two card tables (section 54): `CardDb::cube()` (the cube IR, compiled in: every cube game,
  generator, pool, draft, fuzz pool and digest) and `CardDb::constructed()` (the cube's cards, the
  same definitions at the same ids, then `data/cards/legacy.ir.json`'s, then their tokens and faces;
  only games with `"constructed": true` use it). Both IR files carry one schema (mtg-bot `cards
  export-ir --pool cube legacy`), so one `card_ir` crate reads both.
* Token definitions (CR 111) are lowered from `CreateToken.token_printing` like any printing
  (the "Token" supertype sets `CardDef.token`), interned by equality and stored after the
  cards' (`CardDb::token_def(i)`); `Op::CreateToken` names one by index. They are not in the
  playable set. A colour indicator (CR 204) adds its colours to the mana cost's.

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
  for non-instants without flash: 307.1 / 301.1), activate an ability (section 18).
  * After a player casts, activates or plays a land, **that player receives priority again**
    (117.3c) and `passes` resets.
  * A pass gives priority to the next player (117.3d). When all players have passed in
    succession (117.4): stack empty -> the step ends; otherwise the top object resolves and
    the **active player** then receives priority (117.3b).
* Mana abilities (of permanents and of cards in hand) are activated while paying a cost
  (601.2g / 605.3a: also a payment during resolution, "counter unless its controller pays"),
  those with a mana cost or a sacrifice of their own included (section 18), and not as
  free-standing priority actions: floating mana with nothing to spend it on is activating the
  ability later in the step (the pool empties between steps). With priority: abilities that may
  be activated "only as an instant" (Lion's Eye Diamond: never while paying) and those that
  sacrifice a source with a triggered ability of its own (Chromatic Star, Wizard's Rockets: also
  while paying; `Game::mana_with_priority`). Mana already in a pool (Dark Ritual) joins the
  payment's virtual pool: the payment spends as much of it as it can, and the player chooses
  which mana stays when that matters (`PayPool`, 106.4, 601.2h). docs/shortcuts.md 13.1, 13.2,
  13.12.
* Game-length caps: `GameSpec.max_turns` (default 60) ends the game as a draw with reason
  `TurnLimit`; an internal step budget turns an engine loop into a panic, not a hang.
* Extra turns (CR 500.7): a stack of players; the most recently created extra turn is taken
  first. `Turn.normal` remembers whose normal turn came last, so after any extra turn the
  normal order goes on (the opponent of that player).
* Setup (CR 103; M8a, section 35.1): the chooser (chosen at random from the seed, or
  `GameSpec.chooser`) chooses who takes the first turn (103.1, `PlayDraw`); companions may be
  revealed (103.2b); the libraries are shuffled (103.3); seven cards each, then the **London
  mulligan** (103.5):
  in turn order each player who has not kept declares keep or mulligan; each mulliganing
  player shuffles their hand away, draws seven, then puts one card per mulligan taken on the
  bottom (one decision per card; the pick order is the bottom order). Repeat until all keep.
  `GameSpec.mulligans = false` skips it.

## 6. Casting, costs and the stack (CR 601, 602, 405, 608)

* Casting a spell (601.2): the card moves to the stack immediately as a `StackItem { id,
  kind: Spell(card), controller, targets, cost_option, modes }` (601.2a); a modal spell's
  modes are chosen (601.2b, section 6a); if it has several
  additional-cost options ("discard a card or pay 3 life"), the caster picks one (601.2b,
  `ChooseCost`, only options that can be paid); targets are chosen one decision per target
  (601.2c; a spell cannot target itself, 115.5; the same object can be chosen once per
  "target" word, 115.3); the total cost is fixed (601.2f); mana abilities are activated and
  the mana paid (601.2g-h, `PayMana`), then the rest of the cost (`PayCosts`: sacrifice and
  discard choices, life); the spell becomes cast (601.2i, cast triggers).
* A cast is offered only if it can be completed: timing is legal, every required target has
  enough legal candidates, and the cost is payable from the pool plus untapped mana sources.
  So casting never needs to be rewound (601.2 allows rewinding illegal casts; we never start
  one). The same rule applies to activated abilities later. The targets are checked the way
  they are chosen, with the card on the stack (601.2a before 601.2c; `Game::as_cast`, on a
  copy of the game, for a spell with targets that is otherwise castable): there it is a spell
  (Emrakul's protection from spells that are one or more colors, 702.16b, keeps it from being
  a target) and it has left its zone (a graveyard's card types for a goyf's power; the hand's
  size for Carnage Interpreter's +2/+2). Until the fix (branch `m7d-targetfix`) the check ran
  with the card where it was, except from a graveyard: a coloured spell whose only candidate
  was Emrakul was offered and the target choice panicked (section 15, the search smoke).
* **`PayMana`** (601.2g-h; full description in section 18). Phyrexian pips first (how many
  of each colour are paid with life). Then the sources, one decision per source: options are
  one representative per class of equivalent sources (same definition and ability), restricted
  to classes whose addition keeps the cost payable, and in non-decreasing class order so each
  *set* of sources is reachable exactly once (no permutation branching). Several-mana sources
  (Sol Ring, Black Lotus) pick their production; their mana and the pool's form a virtual pool,
  spent as far as it can be; single-mana sources are matched to the rest of the pips (which
  colour each pays for cannot matter: all of it is spent). Last, which mana of the virtual pool
  stays when the pools differ (`PayPool`). Feasibility is a matching over the pips' acceptable
  types (`mana::payable`, the pool's units among the candidates), with budgets for life and for
  permanents a later sacrifice needs.
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
  (graveyard; library top for Memory Lapse; hand for Remand; exile for No More Lies). A
  spell that "can't be countered" (CR 101.2) stays on the stack; it can still be targeted.

### 6a. Modal spells and abilities (CR 700.2)

* A modal `AbilityDef` holds `modes: Modes { list, min, max, repeat }` (each mode an
  `AbilityDef` with its own targets and program; the top-level targets and program are
  empty). "Choose one" is 1..1, "one or both" 1..2, "choose three, the same mode more than
  once" 3..3 with `repeat`, "up to one" 0..1.
* Modes are chosen as part of casting, before targets and costs (700.2a, 601.2b), as a
  triggered ability goes on the stack (700.2b, 603.3c), or on activation: `ChooseMode`, one
  decision per mode instance (`NUMBER` + mode index) in non-decreasing order (strictly
  increasing without `repeat`), so each choice of modes is reachable once; `PASS` once `min`
  are chosen. A mode is offered only if its targets can be chosen (601.2c) and the rest can
  still reach `min`; a spell is castable only if enough modes can be chosen.
* The chosen instances are `StackItem.modes`; the item's target requirements are the
  instances' requirements in order, so each instance chooses its own targets (700.2d: the
  same object may be the target of two instances).
* Resolution (608.2b): if every target of the item is illegal, nothing happens; otherwise
  each instance runs as its own frame (`AbilityRef { mode: Some(m), .. }`) with its targets,
  in the order chosen, which is the order printed (608.2c).

## 7. The effect VM: resumable resolution

An ability's program is a flat `Vec<Op>` with forward jumps (lowered from the IR's tree:
`Sequence` concatenates, conditionals and "unless" become jumps). It runs in a `Frame`:

```
Frame { ability: AbilityRef, pc: u16, source: ObjRef, source_def, controller,
        targets: Vec<(req, Target)>, legal: u64 /* targets legal at resolution start */,
        i: u8 /* per-op loop counter, reset when pc moves */,
        lki: Option<(i32, PlayerId)> /* remembered power and controller */ }
```

`Task::Run(frame)` executes ops from `pc` until the program ends or an op needs a choice. Such
an op uses ask-or-consume: it pushes `Run(frame)` back with the same `pc` (and its loop
counter in `frame.i`) and asks; on resume it consumes the answer and moves on. Ops that need a
sub-process (paying mana in "counter unless its controller pays {3}") push the frame with its
continuation `pc` first and the sub-process task above it. Values an effect must remember
across ops (last known information of a removed target for "its controller gains life equal
to its power") live in the frame register `lki: Option<(power, controller)>`, filled by an
explicit `Remember` op before the removal and read through `Amount::LkiPower` and
`Who::LkiController`. Conditionals lower to forward jumps (`JumpUnless` for
`IfConditionElse`, whose two branches share one target requirement; `JumpUnlessTarget` for
"destroy target creature if it has mana value 2 or less"; `May` for `MayDo`). Examples:

| Op | Choice | CR |
|---|---|---|
| `DealDamage { amount, to }` | - | 120 |
| `Draw { who, n }` | - (keys) | 121 |
| `Counter { tgt, dest }` | - | 701.5 |
| `OfferPay { who, cost, if_paid }` | yes/no, then `PayMana` | 118.12 |
| `PutFromHandOnTop { who, n }` | one hand card per iteration | (Brainstorm) |
| `DiscardRandom { who, n }` | - (keys) | 701.9 |
| `AddMana { who, mana }` | - | 106.4 |
| `AddManaChoice { who, prod }` | which production | 106.1b |
| `Destroy { tgt }` / `Exile { tgt }` / `DestroyAll { filter }` | - | 701.7 / 406 |
| `ReturnToHand { tgt }` / `Untap { tgt }` / `PutInLibrary { tgt, depth }` | - | (bounce, untap, Oust) |
| `Discard { who, n }` / `DiscardHand { who }` | one hand card per iteration / - | 701.9 |
| `LoseLife { who, n }` | - | 119.3 |
| `Remember { tgt }` | - (fills `Frame.lki`) | 608.2h |
| `Jump`, `JumpUnless { cond }`, `JumpUnlessTarget { tgt, filter }` | - | conditions on resolution |
| `May { who, skip }` | yes/no | ("you may") |
| `ExtraTurn { who }` | - | 500.7 |
| `DamageAll { amount, filter }` / `DamagePlayer { amount, who }` | - | 120 (each creature; each opponent) |
| `Search { who, filter, dest, reveal, may_fail }` | a definition (`LIBRARY`), or `PASS` | 701.19 (then shuffle, 701.20) |
| `LookTop { who, n, reveal }` | - (keys; fills `Frame.pile`) | 701.18a, 701.42a |
| `PileTo { who, dest, filter, min, max, rest, reveal }` | a pile card per decision, or `PASS` | scry, surveil, "look at the top N" |
| `Shuffle { who }` / `ShuffleIntoLibrary { who, hand, graveyard }` | - | 701.20 (Ponder, Timetwister) |
| `SourceToLibraryTop` | - | (Sensei's Divining Top) |
| `RevealHand { whose, to }` | - (keys) | 701.16 (Duress) / "look at target player's hand" |
| `ChooseDiscard { chooser, whose, filter, optional }` | a card of the seen hand | (Duress, Thoughtseize) |
| `CreateToken { token, n, tapped }` | - | 111.2, 701.6 |
| `Sacrifice { who, filter }` | one of their permanents | 701.17 (edicts) |
| `PutFromHand { who, filter, tapped, optional }` | a hand card, or `PASS` | (Stoneforge Mystic) |

`Frame` also carries `pile` (the cards `LookTop` looked at and `PileTo` has not placed) and a
per-op `floor` (the smallest definition the next pick of a set may take). An op that queues a
task (a land put onto the battlefield whose "as enters" choice must come first, CR 614.12)
suspends the frame below that task.

`Who` resolves a player (`You`, `TargetPlayer(i)`, `ControllerOfTarget(i)`, `LkiController`,
`Active`/`NonActive` for "each player" in APNAP order, CR 101.4); `Amount` is `Fixed(n)` or
`LkiPower` and grows (`X`, counts). Ops reference targets by index into the ability's
`targets`. No op is named after a card.

## 8. Events and triggered abilities (CR 603)

* `move_card` and the other primitive actions call `self.event(Event)`; `Event` records what
  happened with last-known information (definition, controller) so leaves-the-battlefield
  triggers can look back (603.10a). M4a events: enters the battlefield, dies; the matcher is
  generic over `(event kind, trigger filter)` and later kinds plug into the same place.
* A matching trigger becomes a `PendingTrigger { controller, source, source_def, slot }` in
  `Game.waiting`. Nothing is put on the stack until a player would receive priority (603.3).
* **Events and "one or more"** (603.2c, M7e): one instruction of a resolving spell or ability,
  one cost, one simultaneous move (a batch: SBAs, "destroy all", a combat damage step) is one
  event however many objects it touches; `Game::ev_seq` names the current group and a "whenever
  one or more ..." ability triggers once per group (section 33).
* `PutTriggers` (run by the `Priority` loop): **APNAP** (603.3b): the active player puts all
  their waiting triggers on the stack in the order they choose (one "which next" decision
  while two or more distinct triggers remain), then the other player; so the non-active
  player's triggers resolve first. Targets are chosen as each goes on the stack (603.3d); a
  trigger with no legal choice for a required target is removed.

## 9. State-based actions (CR 704)

`Sba` runs when a player would receive priority (704.3) and repeats until nothing happens.
One pass collects every applicable action and performs them simultaneously: 704.5a (life <= 0),
704.5b (drew from an empty library since the last check), 704.5f (toughness <= 0), 704.5g
(lethal damage), 704.5h (deathtouch damage), 704.5i (a planeswalker with loyalty 0 is put into
its owner's graveyard: not destroyed, so indestructible doesn't stop it; M7b), 704.5j (legend
rule: the controller **chooses**
which legendary permanent with the same name to keep, one decision per group, before the
simultaneous move), 704.5d (a token outside the battlefield ceases to exist: to
`Zone::Nowhere`, after a "dies" event could see it in the graveyard). Losing ends
the game at once; if all players lose simultaneously the game is a draw (104.4a). Later SBAs
are new arms of the same pass (M7a added 704.5n and 704.5q, M7b 704.5i, M7b part 2 704.5c
and 704.5s, M7b part 3 704.5p and 702.103f, an unattached bestowed Aura; Auras other than
bestowed ones and battles are not built). A copy of a spell that leaves the stack ceases to exist
at once (704.5e, section 26.4).

## 10. Randomness, chance keys and hidden information

* **The RNG lives in the state** (`Rng`, xoshiro256++ seeded by splitmix64): shuffles, random
  discards, the starting player. Same seed + same actions = same game, bit for bit (the
  fuzzer checks it).
* Library order is materialised (shuffles permute `library`), so a draw is "pop the top".
* **Knowledge, exact.** `Obj.known` has a bit per player who knows the card's identity where
  it is (hidden zones; public zones are known to all). Library positions are physical (the
  `library` list), so a known library card is known *at its place*. Set by: drawing (the
  drawer), looking (the top N of your library; another player's hand, `KEY_LOOK`), revealing
  to everyone (`reveal`: hand reveals, revealed searches, a hand card activated, 602.2a), and
  public -> hidden moves (Memory Lapse, Oust, a bounce, Sensei's Divining Top: everyone saw
  it go there). Cleared by the rearrangements below.
* **Knowledge, partial (`Game.clouds`).** `Cloud { viewer, owner, hand, library, slots, defs }`:
  the viewer knows that the identities `defs` (a multiset) are among those of the owner's
  cards it does not know exactly in the domain: `slots` (library cards), plus the whole hand
  if `hand`, plus the whole library if `library`. A cloud forms when the owner rearranges
  cards privately and the viewer knew some of them (`scramble`): Brainstorm's put-back and
  mulligan bottoms (the hand), a look at the top N (the looker holds the pile), a search (the
  searched card could be any: the library), a shuffle (the library: what was known in it is
  now "somewhere in it", 701.20). The rules keep it exact without ever claiming more:
  * The hand is one pool for anyone but its owner: its cards are not told apart. A card
    leaving it publicly with identity d is matched to a known d in the hand (that card
    becomes unknown: the known d may have been the one) or else to a cloud entry d.
  * A cloud card that is drawn, or taken into the hand, carries the cloud into the hand
    (`hand = true`); a hand card put into the library carries it into `slots`.
  * Learning a card exactly removes its entry (the entry may have been this card); a card
    leaving for a public zone removes its entry; clouds whose domains meet merge; a cloud
    over both the hand and the library says nothing and is dropped.
  * An owner's own knowledge is always exact or nothing (they see their own
    rearrangements; a shuffled library has no order to know), so `viewer != owner`.
  Examples (tests): P0 saw P1's hand, P1 Brainstorms: P0 knows those cards are in P1's hand
  or the two put back. A revealed tutored card, then a fetch land: P0 knows it is in that
  library (never in P1's hand), not where. A scry of a known top card: it is one of the two
  scried cards. The Python engine forgets in these cases (section 14).
* **Chance keys** (`u64`, pushed to the `chance` vector of `apply`) are emitted whenever a
  transition reveals something: `KEY_DRAW(player, def)` (private to the drawer);
  `KEY_REVEAL(owner, def)` for every move from a hidden to a public zone (casting, playing,
  discarding, a searched card put onto the battlefield or into a graveyard, surveil) **even
  if the card was known**: which known card an opponent's "hand slot 3" names can differ
  between sampled worlds; and for an in-place reveal (a hand reveal, a revealed search, a
  hand card activated), each card in definition order; `KEY_LOOK(looker, def)` (private: the
  top N of a library, top first; another hand, in definition order); `KEY_AUTO(player,
  kind << 16 | action)` (public) when the engine answers a single-option decision itself
  (`auto_single`): no search node sees that decision, and that its decider had no
  alternative can depend on hidden cards (no castable instant in hand). Layout: kind in bits
  56-63, player in bits 48-55, payload in the low bits. `KEY_GAME` (public; best-of-three:
  emitted by the match as a later game begins, the decks' and sideboards' sizes, section 36.11).
  Shuffles, random orders and token creation emit nothing (they reveal nothing; the next draw is keyed).
* **Observer masking.** zero-core keys children by chance keys, and the search's tree is the
  root decider's. `determinize(viewer)` records `observer = viewer` in the state; from then
  on a private reveal to another player is emitted with the definition replaced by
  `HIDDEN`, so the viewer's tree does not branch on facts the viewer cannot see. Without an
  observer (real play, tests) keys are complete.
* **`determinize(viewer, rng)`** resamples what the viewer does not know: for each owner, the
  identities (`def`) of that owner's cards in hidden zones (the other player's hand; both
  libraries) not known exactly to the viewer are dealt again among those cards, each
  cloud's identities inside its domain, the rest of the pool over every remaining place
  (uniform over the consistent assignments; known cards stay where they are). It reseeds
  the state's RNG and sets the observer. It never touches public state or the viewer's own
  hand, so the viewer's legal actions are unchanged (tested, and asserted by zero-core
  every simulation). **Pinned cards:** while a payment is in progress (from a cast or
  activation until its mana is paid, and through an "unless pays" payment), the payer's
  mana sources in hand are not resampled for another viewer: the payment may count on
  them. A source already picked is revealed anyway (602.2a), so public. (zero-core only
  determinizes for the decider, who is the payer then; other viewers are tests and
  analysis.)
  If the pending decision belongs to the *other* player, its options are refreshed by
  re-running the asker (section 4), so they never refer to a stale identity. The viewer's
  own decision needs no refresh because of one rule every new decision kind must keep:
  **a choice of a card in a zone hidden from its decider names the card's definition, not
  its slot** (a library search offers `Choice::Def`, "a Forest", at the slot of the first
  Forest in the library sorted by definition: the library's contents are always known to
  its owner, its order never; a looked-at pile or a seen hand names cards the decider knows
  exactly; the decider's hand is never resampled for them). Debug builds re-run the asker
  anyway and assert the options are equal. A second rule: **a decision whose options depend
  on cards hidden from another player is asked even when only "pass" remains** (a pile with
  no matching card, a hand without the card a "put from hand" names), so a refresh for that
  player finds the same decision instead of running on.
  `Game::determinize` assumes both decklists are known (open decklists: best-of-one): it deals
  the owner's hidden cards from the owner's actual deck. **Best-of-three** (M8b part b, section
  36.11) hides each deck within its pool: the match's `determinize` first samples the
  opponent's unknown first picks, deck and sideboard from what the viewer knows
  (`az/sample.rs`), deals the deck's unknown cards to the opponent's unknown cards
  (`Game::redeal`), then runs this one. **Sideboards** (M8a, section 35.5) are hidden from the
  other player but for a revealed companion (public, a card in no zone); with open decklists
  `determinize` keeps them (no card of a sideboard can enter the game but a revealed
  companion), with hidden decks the opponent's is the rest of their sampled pool.
* **Cards exiled face down** (M7c tail part 3, section 30.2): `Zone::FaceDown` is hidden like a
  library. A card moved there keeps its knowledge bits and is known to its owner, who saw it go
  (a look keyed `KEY_LOOK`, private; a hand exiled face down; a hideaway pile); a cloud whose
  domain held it keeps it by its slot; `determinize` deals the face-down cards a viewer doesn't
  know again with the owner's other hidden cards, never for their owner. The view and the
  encoding show a face-down card's identity only to a viewer who knows it, and a player's
  face-down cards as a pile without order (the order a hand was exiled in is not observable).
  Questions about a face-down card (Shelldock Isle's "you may play it") are asked whenever the
  card is there, so whether they are asked says nothing about it.
* **Face-down permanents** (M7d, section 31.2; manifest dread): the slot is on the battlefield
  with the face-down definition (a 2/2 with no text, CR 708.2a), and the card it is is hidden like
  a library card (`Game::face_down_ids`, `hidden_obj`, `ident`): its `known` bits, clouds by
  slot, `determinize` deals it again with its owner's hidden cards for a viewer who doesn't know
  it, never for its controller (who looked at it, 708.5); the view and the encoding show the card
  only to a viewer who knows it (otherwise an unknown 2/2). Turned face up or leaving the
  battlefield it is revealed (708.8-9). Bolas's Citadel's top card (31.1) is known to its
  controller before every decision (a keyed private look).
* **Checks** (`tests/fuzz.rs`, every ~40 actions; matches: both viewers and the hidden pool,
  section 36.11): `determinize(to_move)` keeps the viewer's
  legal actions and its whole observable view (`Game::view_of`: public zones and objects
  named by public place, sizes, life, mana, the turn, its own hand, the cards it knows
  exactly and where, its clouds, the pending decision) and never resamples a known card;
  the clouds of the resampled world still hold; and two worlds sampled for the viewer,
  played with the same actions, stay indistinguishable to it while their keys, next
  decider and legal actions agree (zero-core's child signature). The last check found the
  two leaks fixed in M4b-2 (`KEY_AUTO`, keyed reveals of known cards).
* **`force_chance`** (zero-core `chance_k`, M5a): `Game::force_chance(keys)` queues outcomes;
  every key the engine emits consumes one queued key, in order, so later keys stay aligned
  with their events whatever happened to earlier ones. Three events follow their queued key
  when the world allows it: a draw (`KEY_DRAW`) and a look at the top of a library
  (`KEY_LOOK`, or `KEY_REVEAL` for a revealed look) swap the named identity into the card
  about to be seen from another library card **interchangeable with it for every player**
  (both unknown to everyone, in the same cloud domains: every player's knowledge still
  holds), chosen at random among such cards; a random discard (`KEY_REVEAL` of its owner)
  takes a card of the named identity if the hand holds one. Everything else falls back to
  the world's own outcome: a key masked as `HIDDEN` (another player's private event), reveals
  of chosen cards (casting, a revealed search), `KEY_AUTO`, a known top card, an identity
  that is not there. The emitted keys are always the actual outcome (`force_chance(&[])`
  clears the queue). Measured (tests/az.rs, mid-game states of mixed heuristic / random
  play, the decider's world): a transition with the decider's draws or looks, replayed in
  another world of the same decider, reproduces all its keys in 86% of the cases.

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
| `NUMBER` | 4-35 | n = 0..31 (damage amounts, modes, productions; later X) |
| `HAND` | 36-99 | decider's hand slot 0..63 (cast, play, choose, discard, bottom, put onto the battlefield) |
| `MY_PERM` | 100-195 | decider's permanents, slot 0..95 (attack, block with, sacrifice, target) |
| `OPP_PERM` | 196-291 | opponent's permanents, slot 0..95 (target, block this attacker) |
| `STACK` | 292-323 | stack slot 0..31, 0 = top (target spell) |
| `TRIGGER` | 324-339 | decider's waiting triggers, slot 0..15 (order triggers) |
| `LIBRARY` | 340-403 | slot 0..63 of a list sorted by (definition, card index): a searched library (a definition, at its first slot), a looked-at pile, another player's seen hand |
| `GRAVEYARD` | 404-531 | decider's 0..63, opponent's 64..127 (M4b) |
| `ACTIVATE` | 532-915 | `MY_PERM` slot * 4 + ability index: activate at priority; a mana source while paying |
| `HAND_ABILITY` | 916-979 | the ability of the card in the decider's hand slot 0..63 (cycling; a Spirit Guide while paying) |
| `EXILE` | 980-1011 | at priority: play or cast the k-th card in exile the decider may play (exile order; M7a); `ChooseExiled`: the k-th offered card (M7b) |
| `GY_ABILITY` | 1012-1023 | at priority: activate the ability of the k-th card in the decider's graveyard (bottom first) that has a graveyard ability (M7b part 3b, section 27.5) |

`NUM_ACTIONS = 1024`. Canonical orders: hand, library, pile = sorted by (definition, card
index); permanents = timestamp order, split by controller; stack = top first. **Equivalent
options are offered once**: hand, library and pile cards with the same definition (hidden-zone
cards have no other state), mana sources of the same class; a set of cards (attackers, cards
put into a hand or graveyard from a pile, modes) is picked in canonical order so each set is
reachable once. Targets are not deduplicated yet (it needs an exact "same
observable state" key over every per-object field; an optimisation for later).

Decisions by kind: `Priority`, `Mulligan`, `MulliganBottom`, `Target`, `PayMana` (`ACTIVATE`
ids), `PayPhyrexian` (`NUMBER`: how many pips of a colour are paid with life), `ChooseMana`
(`NUMBER`: which production of a mana ability or effect), `ChooseCost` (`NUMBER`: which
additional cost), `Sacrifice` and `DiscardCost` (one object per decision), `DeclareAttackers`
(one creature per decision in increasing slot order, `PASS` = done: each attacking set exactly
once), `DeclareBlockers` (one decision per potential blocker in slot order: which attacker, or
`PASS`), `AssignDamage` (amount per blocker in order), `YesNo` (pay, "you may", shock lands),
`ChooseHandCard` (Brainstorm, put a card from hand onto the battlefield), `Discard` (cleanup,
discard effects), `OrderTriggers`, `LegendRule`; M4b-2: `SearchLibrary` (`LIBRARY`: a
definition, `Choice::Def`; `PASS` = find nothing, 701.19b), `PutOnTop` / `PutOnBottom` (the
next pile card, in order; `PASS` ends a scry's bottom part), `PutInGraveyard` / `PutInHand`
(a set, in definition order; `PASS` = done when allowed), `ChooseDiscard` (a card of a seen
hand), `ChooseMode` (`NUMBER` = mode index; `PASS` = no more). Activating an ability is
`Choice::Ability(card, slot)`. M7a: `ChooseColor` (`NUMBER` = W U B R G, 5 = colorless),
`ChoosePermanent` (a permanent chosen without targeting, a vote, crew, an Army; `PASS` = no
more), `ChooseGraveyardCard` (`GRAVEYARD` slots: Exhume, a graveyard cost, delve, a milled
creature), `ChooseAltCost` (`NUMBER` 0 = the mana cost, k + 1 = alternative cost k),
`ExileCost` and `BounceCost` (one card per decision), `DivideDamage` (`NUMBER` = the amount
for the next target); X, a counter kind, an energy amount and tap-or-untap use `ChooseMode`;
"top or bottom" is a `YesNo`. Priority also offers graveyard cards (`GRAVEYARD`: flashback,
escape, lands with Crucible of Worlds) and exiled cards with a permission (`EXILE`). M7b:
`AttackTarget` (what the attacker just declared, or just put onto the battlefield attacking,
attacks: `PLAYER` + 1 = the defending player, `OPP_PERM` = a planeswalker they control; asked
only when they control one), `ChooseExiled` (`EXILE` + k: a card in exile an effect picks, Karn's
-1), `NameCard` (`LIBRARY`: a name among the namer's library cards, a definition at its first
slot, `Choice::Def`; `PASS` = a name of no card there; Tamiyo's +1). A loyalty ability is
activated like any other (`ACTIVATE` ids). M7b part 2: `ChooseFace` (`NUMBER` 0 = the card's
front face, 1 = its other face: a modal double-faced card's back face, an Adventure, a split
card's right half; asked after a `HAND`, `GRAVEYARD` or `EXILE` id whose card can be cast or
played with either face, section 25.3). M7b part 3: `NewTarget` (a copy of a spell may get new
targets, 707.10c: one decision per target in order, `PASS` = keep it, else a target id for the
same requirement; section 26.4). M7b part 3b: `ChooseRoom` (`NUMBER` = the room a venture marker
moves into, 309.5a) and `PutOntoBattlefield` (`LIBRARY`: a card of a revealed pile to put onto the
battlefield, Throne of the Dead Three); an ability on the stack is targeted with its `STACK` id;
ward's costs are a `YesNo` (mana, life) or a `Discard` with `PASS` (section 27). M7c tail part 3
(section 30.6) adds no kind: splicing is a `ChooseHandCard` per spliced card (`PASS` = no more),
hideaway's face-down pick a `PutInHand`, "odd or even" a `ChooseMode`. M7d (section 31.6) adds
none either: the opening-hand action is a `ChooseHandCard` with `PASS` and an `ExileCost`,
emerge's artifact a `Sacrifice`, manifest dread's pick a `PutOntoBattlefield`, the chosen name a
`NameCard`; special actions (plot, turning a face-down permanent face up) use activated-ability
ids, the Citadel's top card `LIBRARY` + 0 at priority. M7e part 1 (section 33) adds two:
`ChooseReplacement` (`NUMBER` 1 = shuffled into its owner's library, 2 = exiled, 3 = exiled with a
void counter: the order of replacement effects, 616.1) and `ChooseRoute` (`NUMBER` = a
`cast::route` code: which permission casts a graveyard card, 601.2a-b). M7e part 2 (section 34)
adds `PayPool`. M8a (section 35) adds two: `PlayDraw` (`PLAYER` = the decider takes the first
turn, `PLAYER` + 1 = the opponent does: CR 103.1) and `RevealCompanion` (`LIBRARY` + k = the k-th
companion of the decider's sideboard its starting deck allows, by definition, `Choice::Def`;
`PASS` = none: 103.2b); and one priority action: `act::COMPANION` (= `PLAYER` + 0, "the
decider's" companion, `Choice::Companion`: the special action of 116.2g). M8b part a (section
36.2) adds two, asked by a best-of-three match between its games (never by a `Game`):
`Sideboard` (`LIBRARY` + k = change entry k of the decider's pool list, the pool's cards by
definition then the five basic lands, k after the last entry changed, `Choice::Def`; `PASS` =
done, when the deck is legal) and `SideboardCount` (`NUMBER` + n = the new count of the entry just
chosen: a basic land or a card with several copies). There are 46 decision kinds; the context
block holds 64 since layout v3 (section 22.6).

Mapping to zero-core's `SearchGame` (M5a, section 21): `to_move` = the pending decision's
player; `legal_actions` = the ids; `step` = `apply`; `result` from `outcome()`; `determinize`
as above; `NUM_ACTIONS = 1024`. (M8b: the episode is a match; between the games of a
best-of-three the match's sideboarding decision is the pending one, section 36.)

## 12. Combat (CR 506-511)

* Declare attackers (508.1): untapped creatures of the active player that have been under
  their control since the start of their most recent turn (302.6) or have haste, without
  defender; each attacks the defending player or, when that player controls planeswalkers, the
  one its controller chooses (508.1b, `AttackTarget`, section 24.2; battles later). Attackers
  without vigilance tap (508.1f).
* Declare blockers (509.1): each untapped creature of the defending player may block one
  attacker it can block: flying needs flying or reach (702.9b); menace needs two or more
  blockers (702.110b; options that would leave an unfixable single blocker are not offered,
  checked by a small search over the remaining blockers).
* Combat damage (510): the attacking player divides each attacker's damage among its blockers
  as they choose (510.1c, current rules: no damage assignment order); with trample, lethal
  damage must be assigned to every blocker before the rest may go to the player or
  planeswalker it attacks (702.19b; 702.19c in the numbering the older test names use);
  deathtouch makes 1 damage lethal (702.2c). A blocked creature whose blockers are gone deals
  no damage unless it has trample. An unblocked creature deals its damage to what it attacks,
  and none once the planeswalker it attacked has left combat (510.1b, 506.4c; M7b). All combat
  damage is dealt simultaneously (510.2);
  lifelink gains life at the same time (702.15b). First strike / double strike: 510.4 and
  702.4 / 702.7 (a creature deals damage in the second step only if it has double strike or
  did not deal first-strike damage).

## 13. Scope and what is deliberately stubbed

Implemented (M4a): setup with London mulligan, the full turn, priority, land play, casting
with mana costs and targets, the stack, resolution with re-checked targets, the effect VM, ETB
and dies triggers with APNAP order and targets, SBAs listed in section 9, combat with flying,
reach, first strike, double strike, deathtouch, trample, lifelink, vigilance, haste, defender,
menace, flash; game end; chance keys; `determinize`.

Implemented (M4b-1): activated abilities of permanents and from hand with the cost framework
(mana with hybrid and Phyrexian pips, {T}, sacrifice self or chosen permanents, discard self,
chosen or whole hand, exile self, pay life) and restrictions (sorcery speed, "only if", "only
during your turn", "only as an instant"); mana abilities of every shape in the cube that needs
no choice in its cost; additional costs of spells (601.2b); lands that enter tapped, enter
tapped unless, shock lands, lands with mana life costs, cycling; `IfConditionElse` and
`MayDo`; last known information for removal riders; hexproof, shroud, indestructible, "can't
be countered"; the effects in section 7's table.

Implemented (M4b-2): mana abilities of cards in hand while paying (605.3a); library searches
(701.19) with every destination, reveals and fail-to-find, fetch lands, typecycling; looking
at the top of a library (scry, surveil, reorder, take some, the rest to the bottom or the
graveyard), shuffles; hand reveals and looks, chosen discards; partial knowledge (clouds) and
a determinize that keeps it; modal spells and triggers (700.2) with repeats and "up to one";
tokens (111) from IR token printings, 704.5d; edicts; colour indicators; keyed auto-answers.

Implemented (M7a, section 23): layers 4-7 with static abilities and effects until end of
turn, counters, generic triggers with event subjects and delayed / reflexive triggers,
protection and combat restrictions, equipment, alternative costs (evoke, dash, the Forces),
cost changes, kicker, X, delve, crew, graveyard and counter costs, flashback, escape,
permissions to play exiled cards, lands from graveyards, extra land drops, linked exile,
graveyard targets, coin flips, energy, amass, persist, monstrosity, living weapon, devotion,
buyback, overload, escalate, level up, the monarch, token copies of tokens and of the card
itself.

Implemented (M7b part 1, section 24): planeswalkers (printed loyalty, loyalty abilities once
per turn at sorcery speed, damage to planeswalkers, 704.5i), attacking planeswalkers and
combat damage to them, emblems and the command zone, "loses all abilities", gaining and
exchanging control, replacement effects of static abilities (token doubling, winning instead
of drawing from an empty library), rules modifications (a draw limit, a sorcery-speed lock,
sorceries with flash, a discard and sacrifice shield), triggered mana abilities of lands, die
rolls, casting during resolution, searching for any number of cards, naming a card, ninjutsu,
retrace, "until your next turn".

Implemented (M7b part 2, section 25): faces as definitions of their own; transforming
(701.27, 712.18) and exiling and returning transformed; modal double-faced cards (casting or
playing either face); Adventures; a split card cast as either half; Sagas (lore counters,
chapter abilities, 704.5s); day and night (daybound, nightbound); "whenever this deals
damage"; poison counters; "do this only once each turn"; triggered abilities until your next
turn; activated abilities a permanent gains ("this Saga gains ..."); connive; mayhem; graveyard
spell discounts; divided X damage; "one or more" batch triggers; no maximum hand size.

Implemented (M7b part 3, section 26): copy effects in layer 1 with exceptions (707.2, 707.9),
becoming a copy until end of turn, entering as a copy of a permanent or a graveyard card, token
copies (of permanents now or as they last existed, of the source, "sacrifice it at the
beginning of the next end step"), copies of spells with new targets (707.10), "cast or copy"
triggers, squad, offspring, replicate, spree, bestow, "another target", prepare (722), 704.5p.

Implemented (M7b part 3b, section 27): activated and triggered abilities on the stack as targets
(115.1, 115.4), countering abilities (701.5a), gaining control of a spell and new targets (608.3a,
115.7), "becomes the target" triggers (603.2e), ward with mana, life or discard costs and granted
ward (702.21), "spells you control can't be countered", improvise (702.126), the initiative
(725) and the Undercity dungeon (309, 701.49, 704.5t), goad (701.38), abilities activated from a
graveyard (eternalize, Otharri's return), experience counters, connive N, storm (702.40),
mobilize (702.181), activated-ability cost reductions, "for as long as <the source> remains on
the battlefield"; the total cost of 601.2f with every additional cost and X before reductions,
reductions on alternative costs (118.9d), graveyard counts of cards only (108.2).

Implemented (M7c tail part 1, section 28): mana abilities with a mana cost or a choice in their
cost (activated with priority, 605.3a), exhaust (702.177), mana abilities removing counters,
imprint colours, a choice of fixed productions, "any type the sacrificed land could produce"
(106.7), spending restrictions (106.6) and "that spell can't be countered", "whenever you tap a
creature for mana", "when you play a land" (305.1), converge, "as this enters, choose a color"
and a colour added to every card, spell and permanent, incubate and a double-faced token, turn
skipping (614.10), a taxed play permission (601.2f), exile until the source leaves from a hand
or the battlefield, triggered abilities effects give permanents (earthbend 701.65, Serra
Paragon), a target linked to another target's controller; New in M7c tail part 1 (29): Badgermole
Cub; Balance; Black Widow, Super Spy; Chrome Mox; City of Traitors; Cloak and Dagger, Entwined;
Coalition Relic; Delighted Halfling; Demonic Consultation; Elite Spellbinder; Gemstone Mine;
Goblin Welder; Lim-Dul's Vault; Loot, the Pathfinder; Malcolm, Alluring Scoundrel; Mana Vault;
Mishra's Workshop; Orcish Lumberjack; Painter's Servant; Palantir of Orthanc; Prismatic Ending;
Serra Paragon; Squandered Resources; Sunfall; Sunken Ruins; Tersa Lightshatter; Time Vault;
Transmute Artifact; Ursine Monstrosity.

Implemented (M7c tail part 2, section 29): a set of n sacrificed at once, effects that apply as a
permanent enters, harness, "until the end of your next turn" and "on a later turn" permissions,
ravenous, "this ability triggers only once each turn", linked card types as a static's condition,
an ability discount (Zirda), a graveyard permission for permanent spells only (Lurrus), boast,
the sacrificed permanent's mana value, Yawgmoth's Will (graveyard play and its exile
replacement), cascade (702.85), cards exiled from a library or graveyard as an event, ascend and
the city's blessing (702.131), copies of tokens that entered this turn, mill-then-land, a
permanent's retrace grant, a discount for spells cast from outside the hand, casting from a
graveyard during a resolution, a basic land type chosen as a land enters, X announced for a life
cost, teamwork, additional combat phases (500.8), "attacks for the first time each turn", warp
(702.185), miracle (702.94), airbend and waterbend, {X} in activation costs with "X targets", a cap
on a loop of activations (732.2a). New in M7c tail part 2 (24): Aang, Swift Savior; Bilbo, Thief
in the Night; Bloodbraid Challenger; Broadside Bombardiers; Candelabra of Tawnos; Currency
Converter; Enduring Innocence; Fear of Missing Out; Fire Covenant; Jacked Rabbit; Keen-Eyed
Curator; Laelia, the Blade Reforged; Lurrus of the Dream-Den; Metamorphosis Fanatic; Multiversal
Passage; Ocelot Pride; Pinnacle Emissary; Portal to Phyrexia; Six; The Mind Stone; Thor, God of
Thunder; We Say Thee Nay!; Yawgmoth's Will; Zirda, the Dawnwaker.

Implemented (M7c tail part 3, section 30): X of a spell for "destroy all with mana value X or
less" and "-X/-X", overload's exile with a search per creature, "up to X targets" and "twice X"
tokens, "exile it instead if it would die this turn" on a fought creature, any number of land
plays, reflexive "you may ... when you do" triggers, a target opponent's tokens, a player's
protection from everything (702.16j), "if you cast it", sunburst, "put into a graveyard from
anywhere" (a trigger from the graveyard and a replacement to the library), infect, annihilator,
protection from coloured spells, draw replacements (Hullbreacher, Quantum Riddler), kicker on
permanents (once, multikicker, kicker {X} with a least X, devoid, "if it was kicked" on enter and
cast triggers), splice onto Arcane (702.47), fading, saddle, triggered abilities that function in a
graveyard, "enters from anywhere other than a graveyard or exile", modes chosen once per object,
a parity chosen as it enters, adapt, finality counters, "cards put into a library", attackers
unblocked, Spirits that block only Spirits, cards exiled face down (a hidden exile zone, 30.2),
reanimating Auras and an Aura spell's target (30.4), "sacrifice it at the next cleanup step". New
in M7c tail part 3 (33): "Name Sticker" Goblin; Animate Dead; Batroc the Leaper; Blightsteel
Colossus; Caustic Bronco; Coveted Jewel; Emperor of Bones; Emrakul, the Aeons Torn; Fastbond;
Generous Plunderer; Gollum, Riddle Master; Goryo's Vengeance; Headliner Scarlett; Hullbreacher;
Inti, Seneschal of the Sun; Mawloc; Memory Jar; Necromancy; Parallax Wave; Pentad Prism; Pest
Infestation; Quantum Riddler; Shelldock Isle; Sowing Mycospawn; Sword of the Meek; The One Ring;
Thieving Skydiver; Through the Breach; Toxic Deluge; Vicious Rivalry; Wan Shi Tong, All-Knowing;
Winds of Abandon; Worldspine Wurm.

Implemented with M7d's IR fixes (section 32): reflexive triggered abilities after an action
that isn't optional, with an intervening "if" (603.12, 603.4); "whenever ... this turn"
(603.7b); a combat damage step as one batch for "one or more" triggers; a spell's instruction
shuffling the spell into its owner's library (608.2c); mana abilities with {X} making X mana in
any combination (107.3, 605.3a).

Implemented (M7d, section 31): face-down permanents (CR 708: a 2/2 with no text in every rule and
layer, a hidden identity, turning face up as a special action, revealed as it leaves) and manifest
dread (701.62), special actions (116.2: plot 702.170, turning face up), an opening-hand action
(103.6), shadow (702.28), "exile it with a void counter instead" (614.1a), reconfigure (702.151),
Class levels (716), a granted "becomes the target" triggered ability limited per creature, a card
name chosen as a permanent enters and taxes on that name, a mana ability whose cost taps another
permanent, exert (701.39) and mana that gives haste, emerge (702.119), a random card from a
graveyard and from a hand exiled, a top card known to its controller and cast for life, a granted
sacrifice mana ability used together with the land's own in one payment, a mandatory graveyard
exile on a permanent spell. New in M7d (13): Abhorrent Oculus; Anointed Peacekeeper; Arena of
Glory; Bolas's Citadel; Crabomination; Dauthi Voidwalker; Does Machines; Gemstone Caverns; Lion
Sash; Nadu, Winged Wisdom; Pillage the Bog; Rain of Filth; Urza, Lord High Artificer.

Implemented (M7e part 1, section 33): the documented rules shortcuts removed (docs/shortcuts.md
tracks every one): the monarch's triggered abilities on the stack (724.2) and Okoye's delayed
trigger, "one or more" triggers once per event (603.2c), the order of replacement effects chosen
by the affected object's controller (616.1), Mox Diamond however it would enter, permanents
entering with the counters an effect gives them (614.1c), "enter as a copy" not of a permanent
entering at the same time, a granted escape or retrace with a card's own additional costs, the
permission a graveyard card is cast with chosen by its caster (601.2a-b), Bolas's Citadel casting
any face of the top card, last known information of a damage source (608.2h), saddle without crew
bonuses and teamwork with mana creatures, turning face up after losing all abilities (116.2b),
Necromancy's flash permission, exact converge, attached Equipment timestamps (613.7e), granted
triggered abilities lost with all abilities and sourced at the permanent that has them (113.7).

Supported cube cards: 546, the whole cube (M7d: 537, M7c tail part 3: 524, M7c tail part 2: 491, M7c tail part 1: 467, M7b part 3b: 438, M7b part 3: 410, M7b part 2: 394, M7b part 1: 378, M7a: 360, M4b-2:
191, M4b-1: 133, M4a: 31); creatures 209 by their front faces (M7d: 208, M7c tail part 3: 201, M7c tail part 2: 184, M7c tail part 1: 168, M7b part 3b: 155, M7b part 3: 133, M7b part 2: 125,
M7b part 1: 113, M4b-2: 20); planeswalkers 20 (Jace's, Tamiyo's and Ajani's back faces not counted). New with M7d's IR
fixes (9, section 32): Ajani, Nacatl Pariah; Celestial Colonnade; Chromatic Star; Creeping Tar
Pit; Forth Eorlingas!; Green Sun's Zenith; Minsc & Boo, Timeless Heroes; Strip Mine; Wizard's
Rockets. New in
M7b part 3b (28): Boseiju, Who Endures; Brain Freeze; Caves of Chaos Adventurer; Ertai
Resurrected; Fallen Shinobi; Fanatic of Rhonas; Feywild Caretaker; Forensic Gadgeteer; From the
Catacombs; Hexing Squelcher; Invert Polarity; Kappa Cannoneer; Leovold, Emissary of Trest; Occult
Epiphany; Otawara, Soaring City; Otharri, Suns' Glory; Passageway Seer; Raffine, Scheming Seer;
Seasoned Dungeoneer; Seasoned Pyromancer; Sedgemoor Witch; Sunset Saboteur; Surrak, Elusive
Hunter; The Wondrous Wasp; Tishana's Tidebinder; Undermountain Adventurer; Voice of Victory; White
Plume Adventurer. New in M7b part 3 (16): Chain
Lightning; Chain of Smog; Emeritus of Abundance // Regrowth; Fable of the Mirror-Breaker //
Reflection of Kiki-Jiki; Fractured Identity; Lose Focus; Lutri, the Spellchaser; Phyrexian
Metamorph; Sage of the Skies; Saheeli, Sublime Artificer; Shifting Woodland; Sicarian
Infiltrator; Springheart Nantuko; Superior Spider-Man; Three Steps Ahead; Thundertrap Trainer.
New in M7b part 2 (16): Brazen Borrower // Petty Theft; Cecil, Dark Knight; Concealing
Curtains; Embereth Shieldbreaker // Battle Display; Etali, Primal Conqueror; Jace, Vryn's
Prodigy; Jennifer Walters // The Sensational She-Hulk; Life // Death; Norman Osborn // Green
Goblin; Outland Liberator; Shatterskull Smashing; Sink into Stupor; Tamiyo, Inquisitive
Student; Ulvenwald Oddity; Urza's Saga; Witch Enchanter. New in M7b part 1 (18): Chandra, Torch of Defiance; Comet, Stellar Pup; Dack
Fayden; Elspeth, Storm Slayer; Jace, Wielder of Mysteries; Jace, the Mind Sculptor; Kaito, Bane
of Nightmares; Karn, Scion of Urza; Narset, Parter of Veils; Nissa, Who Shakes the World; Oko,
Thief of Crowns; Tamiyo, Collector of Tales; Teferi, Hero of Dominaria; Teferi, Time Raveler;
Tezzeret, Cruel Captain; The Wandering Emperor; Ugin, Eye of the Storms; Wrenn and Six.
New in M7a (169): Abandoned Air Temple, Adeline, Agent Bishop, Alpha Deathclaw, Aragorn,
Archon of Cruelty, Archon of Emeria, Arwen, Atraxa, Auriok Salvagers, Baleful Mastery, Baloth Prime, Barrowgoyf, Basalt
Monolith, Bejeweled Warg, Bloodtithe Harvester, Boomerang Basics, Bristly Bill, Burst
Lightning, Carnage Interpreter, Chaos Defiler, Clarion Conqueror, Consult the Star Charts,
Collective Brutality, Containment Priest, Cori-Steel Cutter, Corpse Dance, Council's
Judgment, Crucible of Worlds, Cryogen Relic, Damn, Dark Confidant, Daze, Death-Greeter's Champion, Deep-Cavern Bat, Dig Through Time, Dismember,
Displacer Kitten, Dragon's Rage Channeler, Eagles of the North, Echo of Eons, Emry, Endurance,
Esika's Chariot, Exhume, Expressive Iteration, Faerie Mastermind, Faithless Looting, Figure of Destiny, Figure
of Fable, Flash, Flashback, Flickerwisp, Force of Negation, Force of Vigor, Force of Will,
Formidable Speaker, Frantic Search, Fury, Galvanic Discharge, Get Lost, Ghost Vacuum, Giver
of Runes, Glimmer Lens, Goblin Engineer, Golos, Grief, Grim Monolith, Grindstone, Guide of
Souls, Gut, Harvester of Misery, Hexdrinker, Icetill Explorer, Ignoble Hierarch, Iron-Shield Elf, Ivora,
Kaldra Compleat, Knight of Autumn, Kolaghan's Command, Leatherhead, Leyline Binding, Lightning Greaves,
Lingering Souls, Lotus Cobra, Luminarch Aspirant, Magda, Mana Crypt, Mana Drain, Manifold
Key, Mind Twist, Mine Collapse, Mishra's Bauble, Mole Man, Mother of Runes, Mox Diamond,
Nettlecyst, Noble Hierarch, Oath of Druids, Okoye, Oliphaunt, Once Upon a Time, Orcish Bowmasters,
Ouroboroid, Palace Jailer, Phelia, Phlage, Portable Hole, Psychic Frog, Pyrogoyf, Pyrokinesis, Questing
Beast, Ragavan, Raph & Mikey, Ravenous Robots, Reanimate, Reckoner Bankbuster, Restless Vinestalk, Retrofitter Foundry, Robber
of the Rich, Sanguine Evangelist, Screaming Nemesis, Scythecat Cub, Sentinel of the Nameless
City, Sewer-veillance Cam, Shallow Grave, Sheoldred, the Apocalypse, Skateboard, Skullclamp,
Skyclave Apparition, Smuggler's Copter, Snapcaster Mage, Sneak Attack, Snuff Out, Solitude,
Soul-Guide Lantern, Spectacular Spider-Man, Spider-Woman, Staff of the Storyteller,
Subtlety, Super Shredder, Suplex, Sylvan Safekeeper, Tear
Asunder, Territorial Kavu, Thalia, Thassa's Oracle, The Fantasticar, Tidehollow Sculler, Time Spiral,
Titania, Torsten, Touch the Spirit Realm, Traumatic Critique, Treasure Cruise, Troll of
Khazad-dûm, True-Name Nemesis, Underworld Breach, Unearth, Unexpectedly Absent, Upheaval, Uro, Urza's Bauble,
Vaultborn Tyrant, Walking Ballista, Wishclaw Talisman, Witherbloom Apprentice, Wolverine, Woodfall Primus. New in M4b-2 (58): the fetch lands Arid
Mesa, Bloodstained Mire, Flooded Strand, Marsh Flats, Misty Rainforest, Polluted Delta,
Scalding Tarn, Verdant Catacombs, Windswept Heath, Wooded Foothills, Prismatic Vista; the
surveil lands Commercial District, Elegant Parlor, Hedge Maze, Lush Portico, Meticulous
Archive, Raucous Theater, Shadowy Backstreet, Thundering Falls, Undercity Sewers, Underground
Mortuary; Abrade, Consider, Crop Rotation, Demonic Tutor, Duress, Enlightened Tutor, Entomb,
Erode, Expedition Map, Fiery Confluence, Generous Ent, Gitaxian Probe, Imperial Seal,
Inquisition of Kozilek, Legion Extruder, Lórien Revealed, Malevolent Rumble, Mystic
Confluence, Mystical Tutor, Natural Order, Path to Exile, Ponder, Preordain, Pyroblast,
Sensei's Divining Top, Sheoldred's Edict, Spellseeker, Stock Up, Stoneforge Mystic, Thopter
Foundry, Thoughtseize, Thraben Inspector, Timetwister, Tinker, Trinket Mage, Vampiric Tutor,
Worldly Tutor. New in M4b-1 (102): lands Ancient Tomb, Blackcleave
Cliffs, Blazemire Verge, Bleachbone Verge, Blood Crypt, Blooming Marsh, Botanical Sanctum,
Breeding Pool, Concealed Courtyard, Copperline Gorge, Darkslick Shores, Fiery Islet, Gaea's
Cradle, Godless Shrine, Hallowed Fountain, Horizon Canopy, Indatha Triome, Inspiring
Vantage, Jetmir's Garden, Karakas, Ketria Triome, Library of Alexandria, Mana Confluence,
Nurturing Peatland, Overgrown Tomb, Raffine's Tower, Raugrin Triome, Razorverge Thicket,
Sacred Foundry, Savai Triome, Seachrome Coast, Spara's Headquarters, Spirebluff Canal,
Starting Town, Steam Vents, Stomping Ground, Sunbaked Canyon, Temple Garden, Thornspire
Verge, Tolarian Academy, Wasteland, Waterlogged Grove, Watery Grave, Xander's Lounge, Zagoth
Triome, Ziatora's Proving Ground; and Abrupt Decay, Aether Spellbomb, Avacyn's Pilgrim, Birds
of Paradise, Bitter Triumph, Black Lotus, Bone Shards, Cabal Ritual, Cathar Commando, Elvish
Mystic, Elvish Spirit Guide, Exorcise, Fatal Push, Gleemox, Goblin Bombardment, Griselbrand,
Guerrilla Gorilla, Haywire Mite, Lion's Eye Diamond, Llanowar Elves, Loran of the Third Path,
Lotus Petal, Manamorphose, Mind Stone, Mox Amber, Mox Emerald, Mox Jet, Mox Opal, Mox Pearl,
Mox Ruby, Mox Sapphire, No More Lies, Oust, Pyrite Spellbomb, Relic of Sauron, Remand,
Reprieve, Sear, Simian Spirit Guide, Sol Ring, Stern Scolding, Swords to Plowshares, Sylvan
Caryatid, the six Talismans, Time Warp, Unholy Heat, Vindicate, Voltaic Key, Wheel of
Fortune, Wrath of God, Zuran Orb. The exact list is `tests/support.rs::SUPPORTED`.

Not yet (later milestones; each is a blocker in the support report, never a silent no-op; no
cube card is blocked by them: since M7d's IR fixes every cube card is supported, section 32):
face-down permanents other than manifested ones (morph, disguise, cloak; face-down spells),
battles, meld, flip cards, convert, "cast transformed", read ahead, fuse, dungeons other than
Undercity ("venture into the dungeon"), ward that discards at random or that an Aura or Equipment
grants, replacement and prevention effects beyond the ones above, Auras other than bestowed and
reanimating ones (enchant creature, land, artifact), granted triggered abilities other than an
Equipment's and the "becomes the target" one of 31.1, abilities granted to permanents other than
Rain of Filth's mana ability, best-of-three matches and sideboarding between games (M8 phase 2),
cards brought into the game from outside it other than a companion (wishes; none in the cube).

Implemented (D1, section 54): constructed play beside the cube: a second card table with the
legacy cards Murktide Regent and Prismari Charm, 60-card decklists (CR 100.2a, 100.4a) in
best-of-one, the counters for the cards a permanent spell's delve exiled (607.2q), a trigger on
cards leaving a graveyard (603.10a), damage to each of one or two targets.

Implemented (M8a, section 35): sideboards (cards outside the game, hidden), the play-or-draw
choice (CR 103.1; the chooser a spec parameter), companions (702.139: the reveal before the
hands, 103.2b; the {3} special action, 116.2g) with their deck conditions.

Implemented (M7e part 2, section 34): the payment model's shortcuts removed (docs/shortcuts.md):
the player chooses which pool mana pays (106.4, 601.2h; `PayPool`), mana abilities with a mana
cost or a sacrifice of another permanent while paying any cost (605.3a), Pentad Prism once per
counter, a tap and a sacrifice of one land in one payment whatever the tap's condition (601.2g),
mana abilities activated before the costs are paid (a Lotus Petal delved or exiled for escape,
X life after a painland's damage).

Deliberate simplifications and limits (each is small and local to change; every shortcut the
engine documents is tracked in docs/shortcuts.md with its class and status):

* A payment spends as much of the pool's (and several-mana sources') mana as it can; the
  player chooses which (section 5, `PayPool`). Keeping pool mana of a type no untapped source
  makes by tapping a land instead is not offered (docs/shortcuts.md 13.13).
* Mana abilities are not free-standing priority actions (section 5; with priority only Lion's
  Eye Diamond, Chromatic Star and Wizard's Rockets), and a payment activates a minimal set of
  sources: every pick must pay part of the cost, so extra mana is never floated during a payment
  (the excess of a several-mana source stays). Floating mana in response to the loss of its
  source is not modelled (docs/shortcuts.md 13.12).
* The mana of a mana ability with a mana cost never pays another such ability's cost in one
  payment (13.15); Wizard's Rockets helps a payment only when nothing else pays (13.14).
* A cost's source is never one of its own mana sources when the cost taps, sacrifices,
  exiles or discards it (CR would allow "tap this for mana, then sacrifice it" for a cost
  without {T}; no supported card has such a pair).
* A Spirit Guide's mana ability is not offered at priority (like every mana ability, section
  5): floating its mana first cannot matter.
* A searched library offers definitions, not cards: which of two Forests is found cannot
  matter (every search shuffles; no search in the cube leaves the library in order).
* Which modes are chosen is one decision per mode in printed order; "choose one or more"
  with repeats offers each multiset of modes once.
* Events are recorded only for the kinds some permanent listens for (`trig_mask`); section
  23.2 lists the kinds.
* M7a's own simplifications are in section 23.11; M7b's in sections 24.7, 25.10, 26.11 and 27.13;
  M7c's and M7d's in 28.8, 29.6, 30.7, 31.7 and 32.5 (what still applies; docs/shortcuts.md has
  them all, fixed ones included).
* Waiting triggers with the same definition and ability are offered once in the ordering
  decision (their order cannot matter while effects ignore their source's per-object state).
* Combat damage amounts use the `NUMBER` range (0..31); a larger amount to divide offers the
  first 30 amounts, lethal and all (M7a).
* Battles can't be attacked (no battle is supported); determinize assumes open decklists and
  keeps both sideboards (section 35.5). (The starting player is chosen by the chooser since
  M8a.)
* Targets are not deduplicated (section 11), nor are activations of identical permanents
  (two untapped Mind Stones offer two `ACTIVATE` ids).

## 14. Differences from the Python engine (CR followed)

* The starting player (M8a, section 35): Python takes it as a parameter of its game setup
  (`starting_player_idx`); here the chooser decides (CR 103.1, `PlayDraw`). Companions: Python
  parses the condition into `CardPrinting.companion` but plays none (no sideboard, reveal or
  special action; mtg-bot's SEARCH_LIMITATIONS); here 103.2b and 116.2g.
* Priority after a non-active player casts: Python returns priority to the active player; the
  CR (117.3c) keeps it with the caster.
* Triggers: Python has no APNAP order and no controller ordering choice; here 603.3b with a
  decision.
* Combat damage among several blockers: Python assigns in battlefield order; here the
  attacking player divides it (510.1c).
* Every in-resolution choice (targets, "may", "unless pays", hand-card picks, legend rule,
  mana sources) is a player decision, not "first legal" or "always yes".
* The Python engine's "any land pays one generic" fallback has no counterpart: lands pay only
  through their mana abilities. A land with no mana ability in the IR and without
  `mana_only_from_produces` relies on that fallback and is blocked
  (`CardPrinting.mana_only_from_produces(land without a mana ability)`); the only one was Strip
  Mine, whose IR has its "{T}: Add {C}" since mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (section 32).
* Paying life (CR 119.4): a player may pay life equal to their life total (Griselbrand at 7,
  a shock land at 2, Mana Confluence at 1). Python requires strictly more.
* Mana abilities: Python offers explicit mana abilities as actions in sorcery windows and pays
  everything else automatically (`auto_tap_for_cost`, a heuristic). Here every source of a
  payment is a decision, mana abilities are activated while paying (section 18), and
  "activate only as an instant" mana abilities (Lion's Eye Diamond) only with priority.
* The Verges ("{T}: Add {R}. Activate only if you control a Swamp or a Mountain"): Python's
  automatic payment taps lands for their whole `produces` set and ignores the condition; here
  the condition is checked.
* Several-mana sources: Python adds all their mana and pays from the pool; here the player
  picks the production (Black Lotus's colour) when it matters, and a card's mana abilities
  are alternatives (a Talisman never pays {C} and {U} at once).
* "Sacrifice a land": Python excludes the ability's source; the CR does not (no supported
  card is a land with such a cost).
* Extra turns (CR 500.7): after the opponent's extra turn (Time Warp targeting them), the
  normal turn order goes on; M4a's engine skipped their normal turn (fixed in M4b-1, found
  by the Time Warp test).
* Mana abilities of cards in hand (Spirit Guides) are activated while paying any cost,
  including an "unless pays" cost during resolution (CR 605.3a), where the player has no
  priority. M4b-1 allowed them only with priority (wrong for such payments; fixed in M4b-2).
* Knowledge (section 10): Python marks a card known to a player or forgets it
  (`hide_hand_moved_into_library`, `reset_public_knowledge` after a shuffle): after a
  Brainstorm put-back or a shuffle, what the other player knew is gone. Here it becomes
  partial knowledge (the card is in that hand or among those two cards; in that library),
  which is what the player actually knows. Python also keeps per-card knowledge in a hand,
  so a known card can stay "known" after an unknown twin is cast; here the hand is one
  pool for the opponent.
* Hand reveals (Duress, Thoughtseize, Inquisition): Python makes the hand known to the
  caster only; "reveals" shows it to every player (CR 701.16a), as here. "Look at target
  player's hand" (Gitaxian Probe) is private to the looker in both.
* Scry (701.18a): the player chooses any cards for the bottom, in any order, and the order
  of the rest on top. Python's default (`bottom_choice: false`) keeps every card on top
  without a choice, and with `bottom_choice` keeps both groups in their library order.
* Surveil (701.42a) and "look at the top N ... the rest on the bottom in any order" (Stock
  Up): Python keeps the cards left on top or put on the bottom in their library order; here
  the player orders them.
* Modal spells: Python's `ModalChooseOne` chooses the mode when the spell resolves, and lets
  the controller order "one or both" modes; the CR chooses modes on casting (700.2a) or as a
  trigger goes on the stack (700.2b; with no mode chosen it is removed), and the modes are
  performed in the order printed (608.2c), as here. (Python's `ChooseModes` chooses on
  casting.)
* `SearchLibraryForCard.may_fail = false` would make Python find a card when one matches;
  every such search names a quality, which may always be missed (701.19b). No cube card sets
  it.
* M7a: "that creature deals damage equal to its power" (Pyrogoyf): the entering creature is
  the source (its colours, deathtouch, lifelink; as it last existed on the battlefield if it left,
  608.2h, since M7e); Python deals the damage from Pyrogoyf.
* M7a: "nonland permanent" filters: Python's predicate admits instant and sorcery cards
  (in a graveyard, on the stack); here a permanent card type is required.
* M7a: "whenever you cast your second spell each turn" is a condition of the event, not an
  intervening "if" (CR 603.4 applies to "if" clauses only), so it isn't checked again on
  resolution.
* M7a: Leatherhead's "that player controls": the player dealt combat damage (the opponent
  in two-player games).
* M7a: Robber of the Rich lasts while the card is exiled (the Oracle text; since M7d's IR fixes
  the IR's `while_exiled`, 32), as in Python.
* M7b: attacking planeswalkers. Python attacks only players ("attacking a planeswalker isn't
  modeled"); here each attacking creature attacks the player or a planeswalker its
  controller chooses (508.1b), and combat damage, trample and "deals combat damage to a
  player" triggers follow what it attacks (510.1b, 702.19b, 510.3a).
* M7b: neither engine redirects damage from a player to a planeswalker (the redirection rule
  is gone, CR 306.7). A planeswalker that is also a creature (Kaito during his controller's
  turn) gets both results of damage, loyalty removed and damage marked (120.3c, 120.3e), as
  Python's combat damage does.
* M7b: Kaito's "during your turn, as long as Kaito has one or more loyalty counters on him,
  he's a 3/4 Ninja creature and has hexproof" is a static ability whose condition is checked
  all the time (611.3a, layer 4 / 6 / 7b). Python writes until-end-of-turn effects as the turn
  begins and doesn't check the loyalty condition again.
* M7b: Tezzeret, Cruel Captain's -7 emblem: Python defers it (it has no emblems); here every
  emblem of the 18 planeswalkers exists (CR 114).
* M7b part 2: "transform this" (701.27f). Python's `TransformSource` transforms the permanent
  every time it resolves; here an ability of a permanent transforms it only if it hasn't
  transformed since the ability was put on the stack.
* M7b part 2: "do this only once each turn" (603.2h, The Sensational She-Hulk). Python's
  `once_per_turn` gate lets the ability trigger once a turn, whether or not the player then
  did it; here a declined trigger doesn't count, and once the player has done it the ability
  doesn't trigger again that turn.
* M7b part 2: a transforming card's back face has no mana cost here (its mana value is the
  front face's, 712.8e). Python's back-face printing carries the front's mana cost (the IR
  copies it, docs/ir_defects.md 9); the mana value is the same in both.
* M7b part 2: mayhem (702.187b, Green Goblin): Python stamps a card with the turn it was
  discarded; here it is the graveyard object discarded this turn, so a card that left the
  graveyard and came back that turn (a new object, 400.7) has no mayhem.
* M7b part 2: Etali, Primal Conqueror's free casts are one `ChooseExiled` decision per spell,
  in any order (608.2g); Python asks yes or no per card in APNAP order.
* M7b part 2 (a fix to this engine, not a Python difference): a spell whose targets are all
  illegal (608.2b) goes to its owner's graveyard without resolving. M7a's engine finished it
  as if it had resolved: a buyback spell went back to its owner's hand (702.27a applies only as
  the spell resolves).
* M7b part 3: a copy of a spell (707.10). Python's copy keeps only the targets and X
  (`push_spell_copy`); the modes, kicker and squad or offspring payments are dropped (a copy of
  a spree spell resolves its first mode). Here a copy has every choice made for the original:
  modes, X, divided damage, alternative and additional costs, the times paid. Python lets the
  copy's controller change at most one target; here each target in turn (707.10c). A copy whose
  targets all became illegal goes to the graveyard in Python; here it ceases to exist (707.10a).
* M7b part 3: replicate (702.56a) is a triggered ability here ("when you cast this spell, copy it
  for each time its replicate cost was paid"): players can respond before the copies exist, and
  the spell's other cast triggers may resolve first. Python makes the copies while casting, and
  doesn't offer replicate when the spell is cast without paying its mana cost (an additional
  cost may still be paid, 601.2b, 601.2f).
* M7b part 3: "copy it" when the spell is gone (Sage of the Skies, replicate): Python copies only
  a spell still on the stack; here a spell that left the stack is copied as it last existed
  (608.2h; the Lose Focus ruling: a countered spell's replicate copies are still made).
* M7b part 3: Chain of Smog. Python offers the copy only if a player it could target has cards in
  hand (the IR's `needs_target_with_cards_in_hand`, a guard against endless copying); here the
  copy is always offered (the Oracle text), with an engine limit of 31 such copies a turn
  (26.11): copying with an empty hand still triggers "cast or copy" abilities (Witherbloom
  Apprentice).
* M7b part 3: entering as a copy (707.5, 614.1c) applies however the card enters (Exhume, a
  flicker); Python asks only as the spell resolves. A card copying a token isn't a token here
  (token-ness isn't copiable, 707.2); Python's Metamorph copying a token ceases to exist when
  it leaves the battlefield. Copy effects apply in timestamp order (613.7): a Metamorph made a
  copy of something else until end of turn (Saheeli's -2) is the Metamorph's copy again
  afterwards; Python keeps the later copy.
* M7b part 3: token copies. Fractured Identity's token is created like any token here (token
  doublers apply, 614.1a); Python creates it directly. Springheart Nantuko's landfall copies the
  enchanted creature as it is when the ability resolves (if Nantuko is still attached, else an
  Insect), as in Python.
* M7b part 3: bestow (702.103). Python offers a bestow cast without checking that a creature can
  be targeted (it is then cast with no target); here castability is evaluated as the Aura spell
  (702.103d: its target, noncreature spell taxes). On the stack it is an Aura spell, not a
  creature spell (702.103b: no creature-spell triggers). With its target illegal for any reason
  (hexproof, protection, gone) it resolves as a creature (702.103e); Python resolves it as a
  creature only when the target left or stopped being a creature, and otherwise puts it into the
  graveyard. A bestowed Aura whose creature stops being a creature or gains protection becomes
  unattached and an enchantment creature (702.103f, an exception to 704.5m); Python leaves it
  attached.
* M7b part 3: spree (702.172a). Python adds the chosen modes' costs after cost reductions; here
  reductions apply to the total cost (601.2f), so a reduction larger than the mana cost's generic
  part reduces the mode costs (and squad, offspring and replicate costs) too.
* M7b part 3: squad and offspring on a copy of the creature spell (707.10): the copy has the
  times paid; Python's copy has none. ("Another target", 115.3: Python removes every target
  already chosen from every later choice; here only for the requirements that say "another" or
  belong to the same "target" word, so separate "target" words may name the same object.)
* M7c tail part 1 (section 28): the 29 cards follow their Oracle text and the CR where the
  IR's class names are all it says (Balance's APNAP choices, 101.4; Palantir's choice by the
  opponent; Time Vault's skip as a replacement, 614.10; Mana Vault's draw-step "if" checked again
  on resolution, 603.4; Goblin Welder's pair checked at resolution, 608.2b); the Python engine's
  handling of these cards was not compared. Its simplifications are listed in 28.8.
* M7d (section 31): the 13 cards follow their Oracle text and the CR. Their Python handling was
  not compared card by card; where it was read, it agrees on manifest dread's look at the top two
  (one card left: that one is manifested), tags haste mana per mana in the pool as here, and skips
  copies of spells for Dauthi Voidwalker's replacement as here. The faults of 31.5 (an ability's X
  counting its own tapped source, more waiting triggers than the action layout's slots, Rain of
  Filth's exponential feasibility) are engine-side. The simplifications are listed in 31.7.
* M7c tail part 3 (section 30): the 33 cards follow their Oracle text and the CR. Where the
  Python engine's model is known to differ: it gates "if you cast it" on a cast from hand (the
  class's name; here "if you cast it", from anywhere, as its docstring and the Oracle text say),
  it has no hidden exile (here face-down cards are hidden, known to their owner, 406.3), it
  splices nothing it doesn't announce (here 702.47: an announcement with its own cost and
  targets), and its kicker is a printing switch (here a repeatable additional cost read by "if it
  was kicked"). The fixes of 30.5 (an effect's cast permission before flashback, a search for
  several cards by definitions, mana classes by their additional mana) are engine-side. The other
  cards' Python handling was not compared; the simplifications are listed in 30.7.
* M7c tail part 2 (section 29): the 24 cards follow their Oracle text and the CR. Where the Python
  engine's model is known to differ: it has one combat per turn (Fear of Missing Out's additional
  combat phase is 500.8 here), it reads `once_per_turn` as "triggers only once each turn" for
  every card (here only for an effect that isn't "you may", ir_defects 14), it sweeps Laelia's
  exiles once per state-based-action pass (here once per event since M7e: one instruction, cost
  or simultaneous move, 603.2c, section 33), and it fires
  "dies" for a creature an "exile it instead" replacement exiles (fixed here, 29.2). The other
  cards' Python handling was not compared; the simplifications are listed in 29.6.
* M7b part 3b: ward (702.21). Python checks ward only for a spell, as it is cast, and pays it
  automatically when it can (an extra cost: mana by its automatic payment, life, a chosen
  discard; granted life costs summed into one payment). Here ward is a triggered ability: it
  triggers for abilities too, each instance separately, the targeting player chooses whether to
  pay when it resolves, and players may respond to it.
* M7b part 3b: the initiative (725.2) and Undercity (309.4c). Python ventures and resolves the
  room at once; here the initiative's abilities and the room abilities are triggered abilities
  on the stack (their targets chosen as they are put on it), and a dungeon is completed by the
  state-based action of 704.5t. Python's Catacombs Skeleton is colourless; here black (the
  Oracle text).
* M7b part 3b: improvise (702.126). Python taps artifacts automatically to pay; here each
  artifact tapped is a payment decision.
* M7d's IR fixes (32): Ajani, Nacatl Avenger's reflexive trigger chooses its target as it is put
  on the stack and checks its "if" as it triggers (603.12, 603.3d, 603.4); Python chooses the
  target with the loyalty ability and checks the condition on resolution only. Forth Eorlingas!
  triggers once per combat damage step (a first-strike step and the regular one each, 510.4);
  Python resolves both steps without a priority pass between them, so one trigger covers both.
  Wizard's Rockets: one `ChooseMana` decision per mana of its X.
* M7e part 1 (33): the monarch's end-step draw and "combat damage to the monarch" are
  triggered abilities on the stack (724.2), Okoye's attack trigger a delayed one (603.7); "one or
  more" triggers fire once per event (603.2c: one instruction, one cost, one simultaneous move);
  the controller orders replacement effects (616.1: Blightsteel, Yawgmoth's Will, Dauthi
  Voidwalker, flashback); Mox Diamond's replacement applies however it would enter; the caster
  chooses among graveyard permissions (601.2a-b); Necromancy has no flash keyword. Python's
  handling of these was not compared.

## 15. Tests

`cargo test -p engine` runs everything below in about 10 s (debug). Rules tests name the CR
rule they check in the test name. Custom test cards (`tests/common/mod.rs`, "Test Bear",
"Test Striker", ...) are written as card IR and go through the same lowering as cube cards;
they isolate one keyword or trigger each.

| File | What |
|---|---|
| `tests/rules_setup.rs` | 103 setup (the play-or-draw choice before the hands), 103.5 London mulligan |
| `tests/rules_m8a.rs` | M8a (section 35): 103.1 (the chooser chooses who plays first before the shuffles and hands; random, or the spec's), 103.2b (companions offered in turn order before the hands, at most one revealed, a public card outside the game), 702.139a's conditions of Lurrus, Lutri and Zirda (met and broken), 116.2g (the special action: main phase of its turn, empty stack, {3}, once, into the hand), 400.11a (a sideboard hidden from the opponent's view and encoding), `determinize` keeping sideboards and the companion, self-play decks with sideboards and companions, the bot (plays first, reveals, uses it) |
| `tests/rules_priority.rs` | 117.3a-d, 117.4, 405.5 (LIFO), 305.2, 307.1 / 702.8 timing, 115.5, 608.2b fizzle, 500.4, 103.8a |
| `tests/rules_sba.rs` | 704.5a, b, f, g (+ 514.2), h, j (legend rule choice), 104.4a, turn cap |
| `tests/rules_combat.rs` | 302.6 / 702.10, 508.1f / 702.3b, 509.1b, 702.110b, 510.1c, 702.19c, 702.4, 702.7, 702.15, 508.8, 506.4 / 702.19e |
| `tests/rules_triggers.rs` | 603.3, 603.3b (APNAP, ordering decision), 603.3d (targets; removal without target), 603.10a |
| `tests/rules_activated.rs` | 602.2a (on the stack before targets and costs), 602.2 (targets before a sacrifice cost; the source gone while its ability waits), 605 (mana abilities while paying, not at priority), 302.6 (tap abilities and summoning sickness, haste; non-tap and noncreature abilities), 602.5d, 602.5b, 106 (excess mana stays; Black Lotus's colour), 601.2g (activation order: Mox Opal before Black Lotus), a card's mana abilities are alternatives (Talisman damage), 119.4, 107.4f (Phyrexian), 107.4e (hybrid) and a mana choice on resolution, 601.2b (additional-cost options), sacrifice reservations, Lion's Eye Diamond, cycling, "you may", indestructible, hexproof; 605.3a (a Spirit Guide while paying, also an "unless pays" cost during resolution; revealed when picked), a hand mana source leaves the cards a discard cost needs |
| `tests/rules_lands.rs` | 614.1c (enters tapped; fast lands), 614.12 (shock lands, 119.4 at 1 and 2 life), Starting Town, horizon lands, Gaea's Cradle, Ancient Tomb, Wasteland, Karakas, Strip Mine ({C}; destroys a basic land too) |
| `tests/cards.rs` | the cube cards: Mana Leak (pay / decline / cannot pay), Memory Lapse, Counterspell, Spell Pierce, Brainstorm (mid-resolution choices, a clone taken mid-resolution), Ancestral Recall, Dark Ritual + Hymn (pool, keyed random discard), Time Walk, Cut Down / Flame Slash filters, Cogwork Librarian, mana-source classes |
| `tests/cards_m4b1.rs` | Swords to Plowshares (last known power), Fatal Push (resolution check; revolt from a Lotus Petal sacrificed for mana), Unholy Heat (delirium), Cabal Ritual (threshold counted while on the stack), Abrupt Decay vs Counterspell, Oust, Remand / No More Lies, Time Warp on the opponent (500.7), Wheel of Fortune, Relic of Sauron, Zuran Orb, Voltaic Key, Aether Spellbomb, Mox Amber, Loran of the Third Path |
| `tests/rules_library.rs` | 701.19c (must find "a card"; nothing in an empty library), 701.19b (may fail; revealed on top), Vampiric Tutor (known to its owner only), Entomb, fetch lands (land types, a shock land's choice first), Path to Exile (the creature's controller may search; no search, no shuffle), typecycling, a search decision unchanged under its searcher's determinize, 701.20 (a shuffle keeps "in the library", not where), 701.18 scry (looks keyed and masked; bottom and top order), 701.42 surveil (and a surveil land), Ponder, Stock Up (sets in canonical order, no impossible pick), Sensei's Divining Top, a scry of a revealed card (a cloud over the scried cards) |
| `tests/cards_m4b2.rs` | Duress (revealed hand, keys, filter, known until it changes), Thoughtseize, Inquisition, Gitaxian Probe (private look, masked keys), a seen hand partly put back (cloud), 700.2a (modes before targets), a mode without targets, Fiery Confluence (repeats, non-decreasing), Mystic Confluence (a target per instance), 608.2b for modal spells, Pyroblast, 603.3c / 700.2b (modal triggers; no mode chosen), 111 / 704.5d tokens (Clue; a token put into a library), Malevolent Rumble, Thopter Foundry (nontoken), Sheoldred's Edict, Generous Ent, Timetwister |
| `tests/hidden.rs` | determinize keeps the viewer's view and actions; known cards stay; observer masking of keys; refreshed decisions of the other player; an ability activated from hand reveals its card (602.2a); a payment in progress completes in every determinized world (pinned hand sources, then revealed); a pending shock-land choice survives determinize; a Brainstorm after a hand reveal leaves partial knowledge; a card leaving the hand is not told apart from a known twin |
| `tests/rules_layers.rs` | M7a: 613 layers (types, colours, keywords, set P/T in timestamp order, pumps, counters), 611.2c, statics and their conditions, 704.5q, protection 702.16, "can't block" groups, equipment 301.5 / 704.5n, exalted, battle cry, prowess; the manlands Celestial Colonnade and Creeping Tar Pit (colours, Elemental, can't be blocked) |
| `tests/cards_m7a.rs`, `cards_m7a_costs.rs`, `cards_m7a_zones.rs`, `cards_m7a_zones2.rs`, `cards_m7a_threats.rs`, `cards_m7a_threats2.rs`, `cards_m7a_misc.rs`, `cards_m7a_x.rs`, `cards_m7a_more.rs` | M7a cards and rules: equipment triggers, taxes and locks, replacement of uncast creatures, flicker, coin flips, explore, alternative costs and evoke (118.9, 702.74), divided damage (601.2d), linked exile (610.3), delayed triggers (603.7), reflexive triggers (603.12), random bottom order, tokens attacking (508.4), flashback (702.34), escape (702.138), exile permissions, dash (702.109), backup, amass, persist, monstrosity, living weapon, chosen player (614.12), devotion and winning (104.2b), delve, kicker, energy, X (107.3), crew (702.122), extra land drops |
| `tests/rules_walkers.rs` | M7b: 306.5b, 606.3 / 606.4, 606.6 / 704.5i, 115.4 / 120.3c, 704.5j for planeswalkers, 508.1b / 510.1b (attacking a planeswalker; no decision without one), 702.19b (trample over a planeswalker), 506.4c (its planeswalker gone), 510.3a (combat damage to a planeswalker isn't to a player), 508.4 (put onto the battlefield attacking), 114 (an emblem), The Wandering Emperor at instant speed (606.3), 120.3 (a planeswalker creature) |
| `tests/cards_m7b_walkers.rs`, `cards_m7b_walkers2.rs` | M7b: every ability of the 18 planeswalkers (Minsc & Boo on the cube card since M7d's IR fixes); Oko's Elk losing a static ability |
| `tests/cards_m7b_walkers3.rs` | M7b hidden information: the decisions of Tamiyo's naming, Jace's look, a search for any number of cards, Chandra's cast from exile and Karn's opponent choice survive determinize; a retrace cast in progress keeps its land card |
| `tests/cards_m7b_walkers_audit.rs` | the any-target audit (24.9): removal and damage that can hit planeswalkers, "each creature" effects that can't, voting, edicts, Psychic Frog and Questing Beast, Clarion Conqueror, Mox Amber, Subtlety |
| `tests/cards_m7b_tests_lands.rs`, `cards_m7b_tests_mana.rs`, `cards_m7b_tests_creatures.rs`, `cards_m7b_tests_creatures2.rs`, `cards_m7b_tests_spells.rs`, `cards_m7b_tests_edges.rs` | M7b: a test for each of the 118 supported cards no earlier test named (`python tools/untested_cards.py` lists the supported cards no card test names; 0 left), each against its Oracle text: the land cycles table-driven (fetch lands, surveil lands, fast lands, horizon lands, shock lands, duals, Triomes and tri-lands with cycling, Verges), Tolarian Academy, Abandoned Air Temple, Talismans, Moxen, mana creatures, artifacts, creatures and spells; rules edges: 611.2c (Harvester of Misery, Spectacular Spider-Man, Ravenous Robots), 610.3c (Touch the Spirit Realm), 601.2g-h (Crop Rotation, Natural Order, Goblin Engineer sacrificing the permanent that paid), 605.3a (Basalt Monolith paying its own untap), 502.3, 122.1d (stun), 701.16a, 702.34 (granted flashback ends; 702.34a, Reprieve on a flashed-back spell exiles it), 608.2n (Echo of Eons isn't shuffled into its own library), 614.1c (lands put onto the battlefield by Crop Rotation), 111.7 (a flickered token) |
| `tests/rules_faces.rs` | M7b part 2, on custom double-faced test cards (Test Flipper, Test Walker Flip, Test Not Flipper, Test Werewolf): 712.18 (the same object: counters, damage, effects), 701.27f, 701.27c, 306.5b / 704.5i (transformed into a planeswalker), 712.8a (a back face dies as its front card), a transform is public, 702.145b (daybound) |
| `tests/cards_m7b_faces.rs` | M7b part 2, the transforming double-faced cards: Ulvenwald Oddity, Concealing Curtains, Cecil, Jace, Vryn's Prodigy (712.14a; every loyalty ability), Tamiyo, Inquisitive Student (every loyalty ability), Etali (608.2g in any order; 704.5c poison), Outland Liberator (731.2, 702.145b), Jennifer Walters |
| `tests/cards_m7b_faces2.rs` | M7b part 2, MDFCs and Adventures: Shatterskull Smashing (712.12, divided X damage), Sink into Stupor, Witch Enchanter, The Sensational She-Hulk (712.11b, 603.2h), Brazen Borrower (715.3d; countered and fizzled Adventures), Embereth Shieldbreaker; hidden information (a face choice survives determinize, a card on an adventure is public, no face up in a hidden zone) |
| `tests/cards_m7b_faces3.rs` | M7b part 2, Sagas: Urza's Saga (714.2b, 714.3a, 714.3c, latent abilities), 704.5s (not while the last chapter waits), Fable of the Mirror-Breaker (the real card since part 3) |
| `tests/cards_m7b_faces4.rs` | M7b part 2: Norman Osborn // Green Goblin (701.50 connive, mayhem, graveyard discount), Life // Death (709.3), Ajani, Nacatl Pariah (batch triggers, the -4; the 0's reflexive trigger and its intervening "if", 603.12, 603.4), the encoding of faces in every world of the viewer |
| `tests/rules_copies.rs` | M7b part 3, the rules of copies with cube cards: 707.2 (no counters or effects copied), 613.2 (counters after layer 1), 707.4 (no enter trigger), 707.3, 707.8a (a token copy of a transformed permanent), 704.5j (a token copy of a legend), 707.9b (a copy of a copy keeps the exceptions), 608.2h (a token copy of an exiled permanent), 707.10 / 707.12 (a copy of a spell isn't cast; X copied; 707.10a ceases to exist, countered too), 608.3f / 111.13 (a copy of a permanent spell becomes a token, not created); hidden information (a pending copy choice and a copy's new targets survive determinize; the same encoding in every world) |
| `tests/cards_m7b_copies.rs` | M7b part 3: Phyrexian Metamorph (707.5, 707.9b, a legend of the opponent's, copying nothing: 704.5f), Superior Spider-Man (a graveyard card, his name, 704.5j), Saheeli's -2 (until end of turn, "another target" 115.3, 704.5p: a copy of an Equipment that reverts, an Equipment made a copy of a creature), Shifting Woodland (delirium), Reflection of Kiki-Jiki (haste, sacrificed at the end step), Fractured Identity (608.2h, the other player's copy), Springheart Nantuko (bestow 702.103b, e, f; landfall copy or Insect) |
| `tests/cards_m7b_copies2.rs` | M7b part 3: Three Steps Ahead (spree 702.172a, 700.2h payable modes), Sicarian Infiltrator (squad 702.157a), Thundertrap Trainer (offspring 702.175a; 601.2f, a reduction reduces the offspring cost), Chain Lightning (707.10c, a creature's controller is asked), Chain of Smog (the 31-copy limit, 732.2a, 732.3; with Witherbloom Apprentice), Lose Focus (replicate 702.56a), Lutri (cast or not, 603.4), Sage of the Skies (707.10f), Emeritus of Abundance (prepare 722.3: enters prepared, attacking with eight lands, leaving takes the copy; a Metamorph copying it enters prepared, 722.2b) |
| `tests/rules_costs.rs` | M7b part 3b: 601.2f (kicker and X before reductions), 118.9d (a reduction on an alternative cost), 108.2 (a token in a graveyard isn't a card) |
| `tests/rules_stack.rs` | M7b part 3b: 115.1 / 701.5a (an ability targeted and countered), 611.2b (the loss of abilities while Tidebinder remains), 115.5, 605.3b, 608.2b, 603.2e (becoming the target: once per spell, players, spells, copies 707.10c), 101.2, 608.3a, 115.7, Ertai; the stack fields of the encoding |
| `tests/rules_ward.rs` | M7b part 3b: 702.21a (mana, life, discard, abilities, each instance), 119.4, 101.2, granted ward and 613.7; the ward fields of the encoding |
| `tests/cards_m7d.rs` | M7d: the 13 cards against their Oracle text (26 tests, the CR cited in each name: 702.170, 103.6, 106.1, 702.28, 614.1a, 702.151, 716, 603.2e, 601.2f, 605.1a, 302.6, 601.2a, 701.39, 106.6, 702.119, 118.9, 701.21, 601.2g, 601.2b, 701.62, 708.2, 708.5, 701.40b, 708.9); face-down permanents and the Citadel's top card hidden from the opponent in every world it samples, never for their controller; the encoding of a face-down permanent the same in every world of each viewer |
| `tests/cards_m7c_tail3.rs` | M7c tail part 3: the 33 cards against their Oracle text (50 tests, the CR cited in each name: 107.3, 702.96, 701.14, 305.2, 601.2c, 603.12, 702.16j, 603.4, 702.44a, 603.6a, 702.86a, 702.16b, 702.90, 614.1a, 702.33c, 702.33d, 107.4c, 603.7, 702.47, 706, 702.32a, 607.2a, 702.171, 301.5, 700.2, 701.46a, 509.1b, 509.1h, 406.3, 702.75a, 303.4a, 608.3a, 514.3a); face-down cards hidden from the other player in every world it samples, never for their owner; Wizard's Rockets's IR defect; a spell's targets checked with it on the stack (601.2a, 601.2c: Exorcise and Emrakul, 702.16b, the search smoke's find on mtg3's engine; Exorcise and Carnage Interpreter, the hand it left) |
| `tests/cards_m7c_tail2.rs` | M7c tail part 2: the 24 cards against their Oracle text (38 tests, the CR cited in each name: 701.21a, 702.156a, 607.2a, 702.142a, 614.1a, 107.3, 702.85a, 702.131, 702.81a, 601.2f, 305.6, 601.2d, 702.122a, 500.8, 702.185a, 702.94a, 602.2b, 732.2a), the search of Green Sun's Zenith on a custom card, the two IR defects; Bilbo's cast of a flashback card for the cost its offer was checked for |
| `tests/cards_m7c_tail.rs` | M7c tail part 1: the 29 cards against their Oracle text (31 tests, the CR cited in each name: 605.3a, 702.177a, 106.6, 106.7, 106.1b, 101.2, 101.4, 305.1, 502.3, 603.4, 607.2a, 608.2b, 608.2g, 609.4b, 610.3, 613.1e, 614.10, 601.2f, 701.19, 701.20a, 701.53a, 701.65a, 508.1d, 119.4, 118.9a), Chromatic Star's IR defect; the state the run mtg3 crashed in (Malcolm's free cast of Echo of Eons, section 15) |
| `tests/rules_initiative.rs` | M7b part 3b: 725.2 (taking it, the upkeep, combat damage), 309.4-309.5 (rooms, the room choice), 309.7 / 704.5t / 701.49c, goad 701.38, a pending room choice survives determinize; the initiative and dungeon globals |
| `tests/cards_m7b_stack.rs`, `cards_m7b_stack2.rs` | M7b part 3b: the 20 cards against their Oracle text (Tidebinder, Ertai, Invert Polarity, Leovold, Surrak, the ward cards, improvise; the initiative cards; Fanatic of Rhonas, Otharri, Seasoned Pyromancer, the graveyard-ability slots) |
| `tests/cards_m7b_stack3.rs` | M7b part 3b: The Wondrous Wasp (611.2b), Voice of Victory (702.181a), Brain Freeze (702.40a, 707.10), Occult Epiphany (205.2a), Fallen Shinobi (and a ninjutsu payment, 601.2h), Otawara and Boseiju (602.2b), Forensic Gadgeteer |
| `tests/rules_payment.rs` | M7e part 2 (section 34): the payment model, 14 tests, the CR in each name: 106.4 and 601.2h (the pool's mana chosen; a source's mana and the pool's together; converge counting the chosen colours; which {R} pays a creature spell), 605.3a (Loot paying Mana Leak's {3} during resolution; Pentad Prism, Arena of Glory, Wizard's Rockets while paying), 602.2b (no mana ability pays its own cost), 601.2g (Rain of Filth after Ancient Tomb's damaging tap; Squandered Resources after Mana Confluence's life; a Lotus Petal delved and exiled for Uro's escape), 601.2h (X life after the mana abilities) |
| `tests/rules_shortcuts.rs` | M7e part 1 (section 33): the rules shortcuts removed, 27 tests, the CR in each name: 724.2 (the monarch's triggers on the stack, countered by Tidebinder, one per creature), 603.7 (Okoye's delayed trigger), 603.2c ("one or more" once per event: Laelia, Wan Shi Tong, Inti), 614.1c (entering with an effect's counters), 614.12a (Metamorph and Exhume), 608.2h (Pyrogoyf's damage as the creature last existed), 702.171a (saddle without a crew bonus), 601.2h (teamwork tapping mana creatures), 116.2b (turning face up after losing abilities), 702.8a (Necromancy's flash permission), 106.1b (exact converge), 614.1a (Mox Diamond put onto the battlefield), 616.1 (Blightsteel and Yawgmoth's Will, flashback and Dauthi Voidwalker), 601.2b (Breach's escape with Bitter Triumph's own cost), 601.2a (Paragon or Lurrus, flashback or Will, Paragon and a card with another face), 709.3 (the Citadel's split card), 613.7e (an Equipment re-attached), 613.1f (Paragon's granted trigger lost), 113.7 (a granted ward's source) |
| `tests/rules_numbers.rs` | M6 (section 46): unbounded numbers, the mtg5 incident reproduced: 122.1 (Ouroboroid's counters past 65,535 exact, total power past 1.31 million, both encodings finite and within `FEATURE_BOUND`), 510.2 (65,537 combat damage dealt in full), 702.19b (a trampler's lethal damage to a 65,539-toughness blocker, not truncated to 16 bits); all three fail on 8307cb4 |
| `tests/rules_castpay.rs` | M6 (section 49): castability and payment agree, on cube cards: 601.2g and 605.3a (Wizard's Rockets turning mana into colours is not also an artifact tapped for Urza's {U}: no Griselbrand from seven mana), 107.3 (Green Sun's Zenith's X with the Rockets and Urza), 702.126a (nor tapped for improvise: Kappa Cannoneer), 106.6 (Mishra's Workshop's mana never turned into colours: Cryogen Relic), 601.2h (two Lotus Petals for Psychic Frog's graveyard cost of no mana), 702.66a (two Petals delved for Treasure Cruise), 614.1a (no delve of Petals under Dauthi Voidwalker), 702.138a (escape's discard feeds the graveyard: not Blightsteel Colossus), 712.11b (a face-down hideaway card cast with a choice of faces is revealed: worlds `determinize` samples keep it), 707.2 (a copy of Llanowar Elves that is also an artifact for Urza: the Elves offered for {G}); the ten and the agreement fuzz fail on 01fa623; the agreement fuzz: random cube boards, every spell and ability offered at priority followed to the next priority (`CASTPAY_BOARDS`, `CASTPAY_PATHS`, `CASTPAY_FROM`, `CASTPAY_LOG`) |
| `tests/rules_farm.rs` | M6 (section 51): the fuzz farm's first bugs, each state built on cube cards: the hidden pool's deal holds the clouds while the opponent pays (Elvish Spirit Guide, a best-of-three match from a scenario: every world is the sampled deck), 400.3 (an opponent's Lotus Petal is not in your graveyard: no Psychic Frog exile, no delve for Treasure Cruise), 605.3a (Gaea's Cradle animated by Nissa with 28 creatures and Aang's waterbend: checked and paid within 30 s on a thread), 701.25a (a surveiled Blightsteel Colossus shuffles the library after the pile is placed: the decider's worlds and encoding keep the other card), 709.3 (Life // Death from Shelldock Isle revealed before the half is chosen: fixed by 49.6, fails on 075649c), 601.2g-h (delve's cards exiled after the mana abilities: Fanatic of Rhonas with a delirious Dragon's Rage Channeler pays Dig Through Time); five fail on ab02574, all six on 075649c; `pay.rs` unit tests: `covers` on 255 units and an augmenting path, an exhausted search's panic |
| `tests/support.rs` | the support report and its threat view, the exact supported list, loud failures (on an edited IR since every cube card is supported), the IR consistency checks of section 32; `SUPPORT_DEFS=<file>` dumps every lowered definition (`tools/diff_defs.py` compares two dumps by name) |
| `tests/bots.rs` | the heuristic bot (21.4): every answer legal and every game ended (against random, `HeuristicV1` and itself, and half-random play; `--ignored`: 8,000 games with every decision asked, all 39 decision kinds answered; M7e's two kinds have their own answers, not measured that way), the registered `heuristic` beats random, the policy in set situations (a loyalty minus for a creature, else the plus that is not mana; the ultimate; no minus at its own permanents; a fetch land first; equip once, at the creature that attacks; crew with a creature that would not attack; no loop with Basalt Monolith and Sensei's Divining Top; Mind Stone only when flooded, at the opponent's end step; the largest X); `--ignored --nocapture --test-threads=1`: strength against `HeuristicV1` and random by deck subsets, speed, activation counts, a trace (`BOTS_TRACE_SEED`), the cube's activated abilities, the X cards |
| `tests/az.rs` | the zero-core game `mtg` (section 21.8): games end with sane results, the turn cap, clones, `determinize` keeps features and legal actions for both viewers, equal keys mean equal features, `force_chance` replays and stays aligned, encoding slots, decks, spec parsing, heuristic vs random (`AZ_SCALE=k` for more games) |
| `tests/rules_m8b.rs` | M8b part a (section 36): best-of-one is the game of before action for action; the spec's `format` and `pools`; CR 103.1 in later games (the loser chooses; after a drawn game the previous chooser); drawn games count and a match can be drawn; the match ends at two wins or after three games, its result and margin; sideboarding reaches every legal deck exactly once (an exhaustive search of a small list, 100.2b; decks of 5-7 cards and of exactly 6, `PASS` only at a legal size); a pool's deck is sideboarded to exactly 40 cards (section 42: every 40-card deck of generated, evaluation and M8a pools by its canonical path, the two extremes, no 39- or 41-card deck, `PASS` only at 40, the heuristic's answers legal from any point); a spec decklist of another size keeps its size; sideboarded decks are legal (40 cards) and are the next game's cards and sideboard; the heuristic keeps its deck; sideboarding hidden from the other player; `determinize` keeps the decider's actions and encoding in matches; the match block and bags; pools and the deckbuilder |
| `tests/rules_m8b_hidden.rs` | M8b part b (section 36.11): the hidden pool: the opponent's first picks drawn from the prior (never known cards, never the viewer's) and seen ones kept; sampled decks with the public sizes (40 cards, the sampled pool's bounds 40..=40: section 42) holding every card the viewer knows (in games and between games); a deck of 40 basic lands with 31 of a type sampled; the second sideboarder's worlds independent of the first's choice; the companion question asked of every player in best-of-three; the bags (`OPP_POOL`, `OPP_HIDDEN`) and every feature the same in the viewer's worlds; the heuristic's choices the same in its worlds; `force_chance` in matches |
| `tests/rulings_*.rs` | M8d (section 37): one test per official ruling of a cube card, the ruling named in the test (`// ruling: <hash8>`; `docs/rulings.md` tracks every ruling, built by `tools/rulings_doc.py`), the CR in its name. `rulings_copies.rs` (Phyrexian Metamorph, Superior Spider-Man, Saheeli, Shifting Woodland: 707.2, 707.9b, 107.3m, 707.4, 613.1, 302.6, 704.5n), `rulings_replacement.rs` (Blightsteel, Dauthi Voidwalker, Yawgmoth's Will, Hullbreacher, Quantum Riddler, Containment Priest, Suplex: 614, 616.1, 702.90, 700.4), `rulings_protection.rs` (Emrakul, True-Name Nemesis, Mother and Giver of Runes: 702.16, 113.6, 101.2, 702.86, 608.2b), `rulings_stack.rs` (Tidebinder, a countered delayed trigger, spells that can't be countered against Remand, Mana Drain, Subtlety, storm, Lutri's and Chain Lightning's copies, checks on resolution, Teferi, ninjutsu), `rulings_facedown.rs` (manifest dread, Headliner Scarlett: 701.40, 701.62, 708.8), `rulings_faces.rs` (Sink into Stupor and Aang against "can't be countered", a modal double-faced card from the graveyard, daybound, Reflection of Kiki-Jiki's copies, Adventures), `rulings_triggers.rs` (sources gone before their enter abilities resolve, 610.3c, 603.6; Sensei's Top; Goryo's Vengeance; Chandra's emblem; Mana Drain's next main phase; Dack's control), `rulings_walkers.rs` (606.3, 602.2b, Kaito 205.1a), `rulings_mana.rs` (601.2f with alternative costs, 601.2g-h, 119.4, 107.3b, delve, Goblin Engineer); shared helpers in `tests/rulings_util/`. Part 2 (section 38): `rulings_stack2.rs` (divided damage with a target gone: Fury, Pyrokinesis, Fire Covenant, Shatterskull Smashing; Invert Polarity; Lutri's copies of modal, spree, X and divided spells: 601.2d, 608.2b, 115.7, 707.10), `rulings_casts.rs` (Etali's free casts, Bolas's Citadel, Dauthi and a discard, converge on copies and free casts: 608.2g, 107.3b, 118.9a, 601.2f, 614.1a), `rulings_copies2.rs` (Reflection's copies of copies and tokens, the prepare card: 707.9b, 722.3), `rulings_triggers2.rs` (sources gone, Green Sun's Zenith stolen, exile-until one card at a time, magecraft on copies, Leovold, Screaming Nemesis, Coveted Jewel, City of Traitors, flashback exile, library positions: 610.3c, 603.2c, 603.2e, 702.85a, 401.4). Part 3 (section 39): `rulings_replacement2.rs` (ravenous, reconfigure, Archon of Emeria, Elspeth's doubling, an Aura entering without being cast, Narset and Leovold with Hullbreacher, finality counters, bestow: 614, 616.1, 303.4f), `rulings_copies3.rs` (CDAs in every zone, copies of kicked, saddled and dashed objects, token copies' copiable values, Vehicles made creatures, backup, earthbend: 707.2, 707.9, 604.3, 613.4b), `rulings_stack3.rs` (targets gone, priority around resolutions and casts, uncounterable spells, casts during a resolution, storm and replicate, special actions, splice, Bolas's Citadel's top card: 608.2b, 117.3b, 601.2), `rulings_triggers3.rs` (reflexive triggers per token, last known information, delayed returns, cast triggers, draws over a turn, trigger order, intervening ifs, Nadu's new object: 603.12, 608.2h, 603.3b, 603.4, 400.7), `rulings_protection2.rs` (effects that don't target, a reanimated True-Name Nemesis and its Aura, protection from everything, modes needing a target, two wards: 702.16b, 303.4d, 700.2a, 702.21a), `rulings_faces2.rs` (DFC and MDFC characteristics in and out of play, Adventures' characteristics, copies and permissions with Bilbo and Robber of the Rich, preparation cards: 712.8, 712.11, 712.14, 715.3-715.4, 722), `rulings_walkers2.rs` (loyalty abilities after entering mid-combat, Comet's dice: 606.3). Part 4 (section 40): `rulings_zones2.rs` (exile and return as a new object, cards played from exile or a graveyard with their timing, "cast" and lands, escape and retrace again, empty libraries, dual lands' types: 400.7, 601.3, 305.1, 702.138, 702.81, 205.3i), `rulings_combat.rs` (creatures put onto the battlefield attacking, ninjutsu, summoning sickness of lands and Vehicles made creatures, blocks that stand, exert: 508.4, 702.49, 302.6, 509.1h, 701.39b), `rulings_mana2.rs` (mana value under every cost, X outside the stack and in free casts, alternative costs with permissions, flashback, escape, devotion, domain, converge, mana abilities, emerge (also with retrace, 40.9), waterbend, cascade into a split card: 202.3, 107.3b, 118.9, 702.34a, 702.138, 700.5, 106.6, 709.3, 702.119a), `rulings_other.rs` (Ajani's -4, reconfigure, Equipment, control changes, Balance, extra turns, connive and explore, colorless emblems, durations: 701.21a, 702.151b, 301.5, 500.7, 701.50) |
| `tests/interactions_*.rs` | M8d part 2 (section 38, `docs/interactions.md`): pairs of mechanics written in different milestones. `interactions_copies.rs` (copies against layers, face-down permanents, replacement effects on entering, "as this enters" choices, ward: 707.2, 708.2, 614.12, 616.1, 707.5, 702.21a), `interactions_costs.rs` (Bolas's Citadel's life with spree and kicker costs, Thalia's tax on free and Citadel casts, a Lotus Petal under Yawgmoth's Will: 601.2f-h, 614.1a), `interactions_faces.rs` (a split card with two graveyard permissions: 709.3, 601.2b), `interactions_zones.rs` (flicker of face-down, transformed and reanimated permanents and of Leyline Binding, control exchange and summoning sickness, Fractured Identity on a planeswalker, trample over protection: 400.7, 304.4, 610.3, 302.6, 306.5b, 702.19c). Part 3 (section 39.5): the last two cells, a token copy of a nightbound permanent (`interactions_copies.rs`: 707.8a, 702.145f) and True-Name Nemesis's chosen player after Oko's exchange (`interactions_zones.rs`: 702.16j, 110.2) |
| `tests/fuzz.rs` | random play (below), option completion (below) and replay determinism; M8b: random best-of-three matches and their replay, `determinize` for both viewers with the hidden pool's checks; M8d part 2: `FUZZ_POOL=interactions`, combo-dense decks from interaction groups (section 38.5); M6 part 3: `FUZZ_POOL=drafted`, matches of random and `consensus`-drafted 4v4 and 2v2 events as their `mtg` specs (first picks, what each player knows elsewhere), and in every mode a sampled first pick is a candidate of the prior (section 45); M6: every decider's encoding finite and within `FEATURE_BOUND` (games, determinized worlds, matches) and `FUZZ_POOL=exponential`, exponential boards to the turn cap (section 46.5); M6: a fourteenth interaction group, mana abilities and costs (section 49.4); M6: `FUZZ_GROUPS=n` (the first n interaction groups: an older commit's decks for its seeds) and a single match's rerun passing when the match completes (section 51.7) |
| `tests/draft.rs` | the zero-core game `team_draft` (section 41): packs seeded, disjoint and full (the 541 cards drafted or undrafted), seats alternating and every pass to an opponent (round 1 to the right), picks are the pack and illegal actions errors, every pool 45 cards and every deck legal (exactly 40 cards, section 42; at most 31 of a basic, at least the minimum of 22 pool cards, section 47; deck + sideboard = the pool; `consensus` builds `pools::build_deck`'s deck), deckbuilding as picks (section 47: any card not in the deck offered, `DONE` exactly from 22 pool cards on, the extremes: 22 pool cards and 18 basic lands of one type, 40 pool cards ending the cards with every count forced to 0, the same cards in two orders one state and one encoding), every legal deck an end state reached by each order of its pool cards (every path of 6- and 7-card pools, the states of 12- and 15-card pools for several minimums, the 487,676 whole decks of a two-card pool), the Latin-square pairings (12, 4, 1 and 9 matches; three different opponents), the first-picks rule (after the draft a team knows every cell but the opposing openers' first picks), `determinize` keeping the viewer's knowledge, encoding and the decider's options (for the decider and another viewer), equal keys meaning equal features, keys masked for the observer, clones, `force_chance` replaying openings and impossible keys falling back in order, heuristic-played matches resolving 1v1, 2v2 and 4v4 events, the result refused while the matches are pending, `resolve`'s checks, the payload, the encoding's slots, the bots, the spec, the layout file checked in (`MTGZ_WRITE_LAYOUT=1`), the random-actions fuzz (`DRAFT_FUZZ=n`; M6: the decider's encoding and every world's finite and within `FEATURE_BOUND`; section 47: minimums 22, 0, 40 and random, `DONE` offered iff the deck holds the minimum, every deck at least the minimum; section 50.2: `DRAFT_FUZZ_FIRST=f`, events from f on, each on random streams of its own, a failing one printing its rerun), measurements (`--ignored`: `measure`; section 47: deckbuilding decisions per seat, the pool cards of random and `consensus` decks); `tests/test_team_draft.py` (pytest): the game through the bindings, the loud failure at the deferred terminal, the token network on layout `draft_v2` (section 47) |
| `tests/draft_matches.rs` | drafted matches (section 45): the `mtg` spec of an event's match (`pN_first_picks`, `pN_elsewhere`, `seed`) plays the event's match move for move (`match_game`; 32 matches: 4v4 and 2v2, random and `consensus`-built decks, best-of-three and best-of-one), the first-pick prior is exactly the cards the viewer's team never saw (193 in 4v4, 367 in 2v2; the real first picks among them), `determinize` samples first picks from it (never the viewer's knowledge) and keeps the viewer's encoding, the spec refuses contradicting knowledge, specs without the keys are what they were; a position from a draft history is seat 0's information set (its knowledge and encoding the real event's) and a history that does not fit is an error; deckbuilding's simulations divisor; `tests/test_drafted.py` (pytest): the glue (payload -> specs, results -> `resolve`, a malformed result refused), events parked by a draft driver and resolved through a heuristic match driver, the token network's actions by identity on `draft_v1`, the human-pick yardstick on a few drafts, the co-training presets |
| `tests/draft_deferred.rs` | `team_draft` through zero-core's deferred hook (section 43): 2v2 and 1v1 events through the self-play driver with a stand-in network (deferred terminals evaluated as leaves of no options, every event parked, a malformed result refused, resolved with heuristic-played matches, samples with the real outcome and margin), the hooks mapping the inherent methods (1v1-4v4); `--ignored`: `measure_parked_memory` |
| `tests/cards_legacy.rs` | section 54: the legacy cards (constructed play's card table) against their Oracle text, 24 tests (7 of them an independent review's probes: a modal double-faced, a split and an Adventure card counted and seen by their card characteristics, 712.8a / 709.4 / 715.4; Endurance putting a graveyard on the bottom; an opponent's Soul-Guide Lantern triggering the graveyard owner's Regent; Yawgmoth's Will's cast from the graveyard; a Phyrexian Metamorph copy of a Regent, no counters as it enters but the trigger, 707.2; the Charm's target never zero, never one player twice, a planeswalker): Murktide Regent (3/3 flying, delve; a +1/+1 counter for each instant and sorcery card its delve exiled, 607.2q; none when put onto the battlefield uncast or for instants another Regent exiled; a second Regent's delve triggers the first once per instant or sorcery card, the triggers above the spell, 603.10a; an opponent's graveyard and lands don't; Tamiyo's -3 and a flashback cast do; its three rulings: mana value 7 under Mana Drain, at most five cards exiled, delve with Underworld Breach's escape, whose cards aren't "exiled with it"), Prismari Charm (choose one; surveil 2 then draw; 1 damage to each of one or two targets, distinct, any target, one target allowed, 608.2b with one gone; a nonland permanent to its owner's hand, a token ceasing to exist) |
| `tests/constructed.rs` | section 54: the pinned 75 and its provenance (the JSON is the MTGO export), CR 100.2a / 100.4a, the `constructed` spec, the two card tables (the cube's definitions and identities unchanged, the legacy cards at 546-547), 60-card games (the play-or-draw choice, the London mulligan, the turn cap), `determinize` dealing the opponent's unseen cards from their 60 (open decklists), the heuristic in the mirror |
| `tests/measure_delver.rs` | section 54.6's measurements (`--release -- --ignored --nocapture --test-threads=1`, `MEASURE_GAMES`) |
| `tests/perf.rs` | the numbers in section 17 (`--release -- --ignored --nocapture`) |
| `tests/digest.rs` | behaviour digests of a fixed set of games, compared with `tests/data/digests.txt` (below; `--release -- --ignored --nocapture`) |

**Fuzzer.** Uniform random actions in complete games between random 40-card decks (17 lands,
23 other cards drawn with repetition from **every** supported card and the custom test cards,
so a newly supported card is fuzzed without editing a list; duplicates, legends and combat
keywords are common). After every action: no panic; every card in exactly
the zone list its `zone` says, once (card count conserved); battlefield in timestamp order;
legal actions sorted, unique, non-empty, `< NUM_ACTIONS`; no chance keys left in the state.
Every ~40 actions: two clones given the same action stay identical (state hash and keys), and
`determinize(to_move)` keeps the decider's legal actions and its whole observable view
(`view_of`) and never resamples a card the viewer knows, the clouds still hold in the
resampled world, two worlds sampled for the viewer stay indistinguishable to it while their
keys, next decider and legal actions agree (section 10), then the resampled world plays on 30
random actions under the same checks; the same view, known-card and invariant checks run for
the other player's determinize too. After every action the clouds are checked against the
true state (`check_knowledge`). Every game must end (60-turn cap).
Separately, 30 games are replayed from their seed and action list and must reproduce the
final state hash, the outcome and the chance-key sequence exactly. `FUZZ_GAMES=n` sets the
game count (default 300). A failing game prints its seed; `FUZZ_FIRST=s FUZZ_GAMES=1
FUZZ_LOG=1` reruns it with the game log in the panic message. M4b-2: 20,000 games in release,
no failure (see section 17 for the time). M7a: 20,000 games over the 360 cards, no failure.
M7b: 20,000 games over the 378 cards, no failure; on the way it found two faults, both fixed
(24.10). M7b part 2: 20,000 games over the 394 cards, no failure; two faults found on the way,
both fixed (25.9). The check also asserts that no face is up in a hidden zone. M7b part 3:
20,000 games over the 410 cards, no failure; three faults found on the way, all fixed (26.10).
M7b part 3b: 20,000 games over the 438 cards, no failure (102 s); one fault found on the way,
fixed (27.12). M7c tail part 1: 20,000 games over the 467 cards, no failure (70 s); four faults
found on the way, all fixed (28.7). M7c tail part 2: 20,000 games over the 491 cards, no failure
(72 s); four faults found on the way, all fixed (29.5). M7c tail part 3: 20,000 games over the 524
cards, no failure (69-70 s); the faults found on the way are 30.5's and the face-down view's (30.2).
The fuzzer's view, known-card and information-set checks cover the face-down cards (`view_of`
lists them per owner, by identity only where the viewer knows them). M7d: 20,000 games over the
537 cards and the custom cards (Test Manifester puts face-down permanents into many games), no
failure (131 s; 5.6 million options completed); four faults found on the way, all fixed (31.8). The known-card check also compares
a face-down permanent's card (`ident`). M7d's IR fixes: 20,000 games over the 546 cards and the custom
cards, no failure (141 s; 5.6 million options completed); one fault found on the way, fixed (32.6). M7e part 1: 20,000 games over the 546 cards and
the custom cards (three new ones), no failure (143 s; 5,664,330 options completed); one fault
found on the way, fixed: a Mox Diamond set aside while its controller decides (33.2) failed the
zone invariant ("card is nowhere"); the invariant now allows it with its pending task. M6 (section
46): the decider's encoding at every decision, both players' at the checkpoints (the game and every
determinized world) have every value finite and within `encode::FEATURE_BOUND` (256);
`FUZZ_POOL=exponential` drives exponential boards (`GROWTH`'s cards, life 1,000,000,000, attacks one
time in four) to the class-B bounds of section 46.3.

**Option completion** (M7c, branch `m7c-payfix`). Sections 6 and 18 promise that a cast, an
activation or an alternative cost is offered only if it can be completed; the fuzzer checks it
(`check_options_complete`). Every legal action of a decision (at priority all but passing and
playing a land) is applied to a clone and the decisions after it are answered along one random
legal path (a generator of its own: the games are the same with or without the check) until the
next priority decision, which comes after the targets, the announcements, the payment and the
cast triggers (601.2i, 117.3c): nothing may panic or fail. In the game it runs at every decision
but mulligans and combat declarations (at a random half of the frequent priority, `PayMana` and
`Target` decisions: a castable card stays castable over many of them; at every rarer one: an
effect's "cast it?", a cost option, an X, a sacrifice), and at the ~1-in-40 checkpoints above
in each world `determinize` samples for either viewer (what the search explores), every
decision kind. A failure names the seed, the world, the option and the path after it. 20,000
games: 5.6 million options tried, no failure on main (524 cards), 125-137 s in all against 66 s
without the check (the same games). A one-off sweep (not committed) of 30,000 games of the
heuristic bot on self-play's random decks (a quarter of the actions random), every decision
checked, two worlds every eighth, two paths per option (the first options, a random one): 27
million options, no failure either.
It came after the self-play run mtg3 (preset `mtg-3`, engine 4613d0f, tail part 2's 491 cards)
crashed in iteration 3 with "PayMana: no way to pay {2}{U}": in a world the search had
determinized (not a cause: the cards involved were public), Malcolm, Alluring Scoundrel with four
chorus counters had P0 discard Echo of Eons and offered its free cast; the offer was checked for
the free cast (`can_cast_free_now`), but `cast_route` put flashback before the effect's
permission, so the card was cast for its flashback cost {2}{U} with two one-mana sources. Casting
"without paying its mana cost" is the alternative cost the effect gives (608.2g, 118.9), and
flashback is another (118.9a: one at most), so the cast is free and the spell is not exiled
(702.34a). Tail part 3 (b85a3c6) had already put an effect's permission first (30.5), found by
the fuzzer through Bilbo, so main does not crash; on 4613d0f the new check finds the same fault at
seed 4670 within 30 s (Bilbo's cast of Lingering Souls, checked for its mana cost, cast for its
flashback cost {1}{B}), where 20,000 games of plain random play never chose the bad option.
Tests: `cards_m7c_tail.rs` (the crash's state: Malcolm, Echo of Eons, Sylvan Caryatid and Breeding
Pool; it panics on 4613d0f) and `cards_m7c_tail2.rs` (Bilbo, Lingering Souls).
`POOL_EXCLUDE=<file>` (names, one per line) leaves cards out of the random decks, to replay an
earlier milestone's games (section 17). `check_invariants` also checks the fast paths' counts:
`n_static`, and that no slot below `Game::first_token` is a token (M7c, section 17).

**Matches** (M8b, section 36.8; the hidden pool's checks: 36.11): `fuzz_random_matches` plays best-of-three matches through
`MtgAz` with random actions, sideboarding included (`FUZZ_MATCHES=n`, default `FUZZ_GAMES` / 20,
at least 20; `FUZZ_FIRST` as for games), and `same_seed_and_actions_replay_matches_identically`
replays 12.

**The search smoke** (`tools/search_smoke.py`, branch `m7d-targetfix`): the gate before pinning
an engine for a run (21.5). Self-play search reaches states random play doesn't: it tries every
legal option of a decision, in determinized worlds and deep lines, where the fuzzer follows one
random path per option. The smoke plays games with zero-core's Rust driver and search (the
self-play code) with a uniform evaluator (no network, no GPU) and the self-play seat of a run
preset (`--preset mtg-3`: 64 simulations, 16 in three decisions of four, minor decisions 4,
determinized roots, Dirichlet noise, completed-Q records), 400 games per configuration, 8 search
threads, 256 games at a time, over driver seeds and specs (`--specs random,eval,nomull`: the
self-play spec's random decks, the 16 evaluation deck pairs, no mulligans with 18-20 lands). A
configuration plays the same games every time (the driver's streams come from its seed and the
game id). Every game must end; a panic fails it. Each configuration runs in a process of its own:
with zero-core pinned at 71a1718 a search worker's panic can leave the driver waiting for ever
(the panic poisons its chunk's mutex, the thread that lays out the group's batches dies on it, and
a caller already waiting for that group is never woken), so a configuration with no batch for 60
s is reported as `HANG (stalled)` with the panic messages. `python tools/search_smoke.py` (seeds
0-3, specs random and eval: 3,200 games, 1.26 million decisions, 32 million simulations) takes
7.5 minutes on the desktop (52-59 s per configuration; 90-135 s with other work running), the
time to budget before a launch; `python -m mtgz.az.bench search --game mtg --threads 8 --games 400
--sims 64` is one configuration with a plainer seat (driver seed 0, the default spec, no minor
decisions; 65 s; the bench has no seed option and no hang watchdog). M8b: the specs `match` and
`match_eval` (best-of-three matches on random or evaluation pools, three times the step cap); the
default is `--specs random,eval,match` (section 36.8). M6 part 3: the specs `team_draft` (4v4) and
`team_draft_2v2` (`--events` events drafted and built with the seat of `team_draft-4v4`, parked,
their matches played by the heuristic from the glue's `mtg` specs and resolved; section 45.7).
It came after the self-play run mtg3 (preset `mtg-3`, engine 0577e06, 524 cards) panicked in
iteration 15 with "no legal target for a required target" and its process hung. On 0577e06 the
smoke finds that panic in 1 of 23 configurations (9,200 random-deck games; seed 2, every time, in
97 s with the stall): Exorcise cast with Emrakul, the Aeons Torn its only candidate, in a world
determinized for the caster (not a cause: everything involved was public). The cast was checked
with the card in hand, not yet a spell, so Emrakul's protection from spells that are one or more
colors (702.16b) didn't apply; on the stack it did, and no target was left (section 6: a spell's
targets are now checked with it on the stack, `as_cast`). Whether mtg3's own panic was this card
is not known: its message named nothing (it names the spell, the requirement, the turn and the
world now) and its state was not reproduced (36 minutes of self-play like iteration 15's from
iteration 14's network, 3,000 games, found nothing). On the fix (main's 537 cards): 48
configurations (seeds 0-23 random, 0-11 eval, 0-11 nomull: 19,200 games, 7.5 million decisions,
193 million simulations), no failure.

**Behaviour digests** (`tests/digest.rs`, M7c). For a change that must not change behaviour (a
performance change): 2,000 games of uniform random play (seeds 0..2000, random decks of every
supported card as in the fuzzer) and 500 games of `az::bots::Heuristic` against random play
(seeds 0..500, the bot in seat `seed % 2`), each hashed (FNV-1a) by what a player of the engine
sees, never by its representation: per decision the pending decision's kind and player, the legal
action ids, the action taken and the chance keys `apply` emitted; every 10th decision the
decider's encoding (`encode_features`, bit for bit); every 20th decision a determinized clone for
the decider (its decision and legal actions, both players' hand and library identities in order,
five random actions played in it with their legal actions and keys); the outcome, the turn and
the number of decisions. `cargo test --release -p engine --test digest -- --ignored --nocapture`
(about 10 s) compares every game with `tests/data/digests.txt` (first recorded on main at
79ddaf9, combined digest f9b5d7c9db1b2931; re-recorded for the new heuristic, 25d09a0a57ab2d12,
for the 467-card pool of the tail's part 1, 7216a12287fefeaa, and for the 491-card pool of part 2
with its four token kinds registered, 7b815998fc563509, and for the 524-card pool of part 3 before
its three new token kinds are registered, 023fa8874a183eae, and for M7d's 537-card pool with Test
Manifester, 5cbbaf9280a01797, and for the whole cube with M7d's IR fixes, 1f210100db7b442e: the pool deals
other decks, every game differs; and for M7e part 1's rules fixes and its three custom cards,
161108b935c03d14, and for M7e part 2's payment model, b3a102f4d67c5e97, and for M8a's setup (the
play-or-draw decision first, the shuffles after it: every game differs; the digest games have
no sideboards), 076e92a6939c6798, and for M8b's layout additions, the encodings only:
63d5bd84a9f7c19b, and for M8b part b's turning face up (the card shown before its cost is paid,
CR 701.40b: 50 games differ), 646d779580df7517), and names the games that differ. `DIGEST_ENCODING=0` leaves the encodings out and
prints the combined digest alone: compare it before and after a layout change (M8b: 3fb15384404c7428
on both sides, section 36.8; part b: the same without the face-up change, e4783eb5c6705fc3 with it,
section 36.11). `DIGEST_WRITE=1` records
the file instead: only when behaviour is meant to change (a rules fix, a bot change, a new
supported card: the pool deals the random decks), then the digests of the next performance change
are compared with the new file. `DIGEST_RANDOM=n` / `DIGEST_HEURISTIC=n` play fewer games. Every
M7c commit kept all 2,500 digests; the check caught one wrong fast path on the way (section 17).

## 16. Porting a card (the recipe for M4b onwards)

1. `cargo test -p engine --test support -- --nocapture` lists blocked cards, the features
   that block most cards on their own, and a greedy staging order (the feature completing
   the most cards next). `SUPPORT_DUMP=path` also writes every blocked card with all its
   blockers (one per line), the quickest way to see what a feature would unlock. Some
   blockers are coarse: `static_abilities`, `alternative_costs`, `graveyard_activated_abilities`
   deny a whole sub-structure until its lowering exists, after which the report names the
   parts inside instead. Expect the count to drop when a coarse blocker is opened:
   `activated_abilities` was said to complete 80 cards at M4a; opening it showed library
   searches, tokens, protection and counters inside, and 46 of those 80 came through.
   At M4b-1 the greedy order started library search (13 cards alone), surveil (11), fetch
   lands (11), modal spells (6 + 5); opening them gave 58 (section 13). At M4b-2 it starts
   static abilities (4), alternative costs (4), returning cards from graveyards (3), the
   "alternative cost was paid" condition (3), compound trigger predicates (2), pumps until
   end of turn (2); see section 20.
2. An IR effect class: add an arm to `lower/effect.rs` that destructures every field of its
   node (no `..`), lowers onto existing ops and `deny`s whatever it does not implement. A
   `CardPrinting` flag: remove its `deny` line in `lower/printing.rs` and implement it where
   it belongs (casting, SBAs, combat, ...). A predicate: `lower/filter.rs` + `Filter`.
3. Only if no op expresses it: add an `Op` named after the rules action (never the card) in
   `program.rs`, implement it in `resolve.rs::exec`, cite the CR. An op that needs a choice
   uses ask-or-consume with a `DecisionKind` and the action ranges of section 11; choices of
   hidden cards name definitions (section 10). A jump op returns its target from
   `Op::jump_target` so `Builder::patch` can fill it.
3a. An activated ability: the cost fields map in `lower/activated.rs::lower_cost` (each field
   used or denied; "activate only if" fields become `Cond`s); its effect lowers like any
   effect, or, for `is_mana_ability`, to a `ManaProd` (`lower_mana_ability`). A new cost atom
   needs: the `Cost` field, its check in `activate.rs::cost_payable` (can it be paid together
   with the mana?), its payment in `pay_costs` (a decision per object chosen), and, if it can
   take something a mana source also uses, a budget in `pay.rs` (section 18). A new mana
   production needs `pay::productions` and `yield_of`.
3b. A land: `produces` is its "{T}: Add" ability unless an explicit `{T}`-only mana ability
   already makes those colours (section 19); entering rules go through
   `Game::put_onto_battlefield`.
4. Tests: a card test (drive with `auto_single = false` and `drive_until` to stop exactly
   where you look; `quiet_until` in `rules_activated.rs` also declines combat), a custom IR
   test card in `tests/common/mod.rs` when a rule needs isolating (`creature(...)` and
   `spell(...)` build IR JSON), the card in `tests/support.rs::SUPPORTED` (the fuzz pool
   takes every supported card itself). Then `FUZZ_GAMES=20000 cargo test --release -p engine
   --test fuzz`. Check the IR against the oracle text for the cards you port (section 20).

## 17. Measurements

Desktop (i9-13900K, Windows 11, other work running), release build, one thread,
`tests/perf.rs`; random decks of every supported card as in the fuzzer, mulligans on,
`auto_single` on.

| Measure | M7c (438 cards; pinned, below) | M7b part 3b (438 cards; pinned, below) | M7b part 3 (410 cards; pinned, below) | M7b part 2 (394 cards; pinned, below) | M7b (378 cards; pinned, below) | M7a (360 cards) | M4b-2 (191 cards) | M4b-1 (133 cards) | M4a (31 cards) |
|---|---|---|---|---|---|---|---|---|---|
| Random play | 941 games/s best of six, 870 median (main's code the same day: 502, 474) | 545 games/s (main's code, the same day: 592 on its own pool) | 615 games/s (main's code, the same day: 674 on its own pool) | 635 games/s (M7b part 1's code, the same day: 648 on its own pool) | 702 games/s (M7a's code pinned the same way, the same day: 835) | 867 games/s (4,000 games, setup included) | 694 games/s | 750 games/s | 1,357 games/s |
| Per exposed decision (`apply`, running to the next decision) | 4.88 us best of six, 5.29 median (218 decisions per game; main's code the same day: 9.16, 9.70); on M7a's pool 4.61 us against M7a's code 5.67 us and main's 7.06 us (medians 4.95, 5.87, 7.30) | 8.44 us (218 decisions per game); on main's pool 7.73 us against its code's 7.41 us, nearly the same games (911,364 decisions against 911,361) | 7.14 us (228 decisions per game); on main's pool 6.95 us against its code's 6.57 us, nearly the same games (903,244 decisions against 903,215) | 6.97 us (226 decisions per game); on M7b part 1's pool 7.01 us against its code's 6.81 us, the same 906,266 decisions | 6.3 us (227 decisions per game) | 5.2 us (221 decisions, 28 turns per game on average: more threats end games sooner) | 6.2 us (231 decisions, 36 turns per game on average) | 6.5 us (206, 36) | 4.4 us (169, 36) |
| `Game::clone` (mean over mid-game states) | 1.34 us (main's code the same day: 1.43 us); on M7a's pool 1.11 us (M7a's code 1.13 us, main's 1.20 us); 1,240 B | 1.34 us (6,137 states); 1,240 B (main's code the same day: 1.28 us, 1,200 B) | 1.23 us (6,244 states); 1,200 B (main's code the same day: 1.17 us, 1,040 B) | 1.27 us (6,484 states); 1,040 B (part 1's code the same day: 1.24 us) | 1.19 us (6,509 states); 928 B | 1.0 us (6,121 states); `size_of::<Game>()` = 904 B | 1.36 us (6,616 states); 600 B | 1.3 us; 576 B | 1.04 us; 560 B |
| `clone` + `determinize(to_move)` (one search simulation's setup) | 1.72 us (main's code the same day: 2.55 us); on M7a's pool 1.46 us (M7a's code 1.81 us, main's 1.83 us); `determinize` alone 0.86 us (main's 1.23 us, M7a's 1.10 us on its pool) | 2.44 us (on main's pool 2.03 us; main's code the same day: 1.95 us) | 1.80 us (main's code the same day: 1.82 us) | 1.92 us (part 1's code the same day: 1.87 us) | 1.77 us | 1.6 us | 2.3 us | 2.3 us | 2.4 us |
| `legal_actions` (copies cached ids) | 3-4 ns | 3-6 ns | 4-5 ns | 3 ns | 3 ns | 4 ns | 8 ns | 4-9 ns | 6 ns |
| Fuzzer, 20,000 games, release | 67 s, no failure | 102 s, no failure | 93-110 s (the last three runs), no failure | 91-100 s (three runs), no failure | 82-91 s (two runs), no failure | 70 s, no failure | 104 s, no failure (determinize checked for both viewers, information sets) | 60 s | 28 s |

**M7c tail part 1** (467 cards, branch `m7c-tail1`): pinned the same way, interleaved with
main's code (7287cd1) in five rounds. On main's pool (the 29 new cards left out with
`POOL_EXCLUDE`: the same games, 870,209 decisions) 5.42 us per decision median (5.34-5.44)
against main's 5.35 (5.22-5.41), +1.3% (pairs -1.3% to +3.8%), 848 games/s against 859. The
first build was +5%: the creature-mana bonus walked the permanents a second time per
`mana_classes` and Serra Paragon's permission was looked for once per graveyard card; one walk
and a once-per-walk cache took it back (and showed a fault, 28.7). On the full pool: 4.98-5.15
us per decision (849,734 decisions), 914-946 games/s; `Game::clone` 1.13-1.18 us,
`size_of::<Game>()` 1,264 B (`marks`, +24 B); clone + determinize 1.50-1.62 us. Fuzzer 20,000
games: 70 s.

**M7c tail part 2** (491 cards, branch `m7c-tail2`): pinned the same way, interleaved with
main's code (32e5225) in four rounds, with a training run using the GPU and about eight threads
on the machine (one earlier batch of runs came out four times slower and was discarded). On
main's pool (the 24 new cards and `Test Green Sun` left out with `POOL_EXCLUDE`) 5.21-5.40 us per
decision against main's 5.12-5.17, +2% to +5% (872-904 games/s against 910-919); 849,494
decisions against 849,734: a few games differ, by the rules fixes (an "exile it instead"
replacement no longer fires "dies"; at most 31 activations of one ability a turn). The first
build was +7%: Zirda's discount walked the permanents for every ability with a mana cost, Six's
retrace grant for every graveyard card at every priority, ascend at every SBA check; `Game::seen`
(29.2) took them off games without such a card. On the full pool: 5.13-5.16 us per decision
(845,409 decisions), 897-922 games/s; `Game::clone` 1.18-1.24 us (main 1.13-1.17),
`size_of::<Game>()` 1,344 B (+80: the entering effects, the activation counts, the miracle
question, the seen bits, the players' turn effects); clone + determinize 1.55-1.74 us (main
1.40-1.59). Fuzzer 20,000 games: 72 s.

**M7c tail part 3** (524 cards, branch `m7c-tail3`): pinned the same way, interleaved with main's
code (4613d0f) in four rounds, with a training run using the GPU and about eight threads on the
machine. On main's pool (`POOL_EXCLUDE` leaves the 33 new cards out) 5.09-5.42 us per decision
against main's 5.25-5.36, pairs -5.0% to +2.7% (874-930 games/s against 882-901); 845,436
decisions against 845,409: a few games differ, by the fixes of 30.5 (a search for several cards
by definitions, the token cap, an effect's cast permission before flashback, mana classes by
their additional mana). The new rules are looked at only in games with such a card
(`SEEN_DRAW_REPL`, `SEEN_GY_TRIGGER`, `SEEN_RARE_EVENTS`; the face-down list, empty, costs a
clone one empty vector). `Game::clone` 1.18-1.25 us (main 1.16-1.25), `size_of::<Game>()` 1,368
B (+24: the face-down list); clone + determinize 1.55-1.82 us (main 1.51-1.69), `determinize`
alone 0.86-0.88 us (main 0.84-0.89). On the full pool: 5.59-5.67 us per decision (832,306
decisions), 848-860 games/s; `Game::clone` 1.30-1.32 us; clone + determinize 1.72-1.90 us.
Fuzzer 20,000 games: 69-70 s.

**M7d** (537 cards, branch `m7d-last`): pinned the same way, interleaved with main's code (0577e06)
in three rounds, with a training run using the GPU and about eight threads on the machine. On
main's pool (`POOL_EXCLUDE` leaves the 13 new cards and Test Manifester out: the same games,
832,306 decisions both) 5.53-5.60 us per decision against main's 5.42-5.49, pairs +0.9% to +3.3%
(858-870 games/s against 875-887); `Game::clone` 1.24-1.27 us (main 1.27), `size_of::<Game>()`
1,392 B (+24: the face-down identities); clone + determinize 1.77-1.96 us (main 1.66-1.72). The
new rules are looked at only in games with such a card (`SEEN_VOID`, `SEEN_NAMED_TAX`,
`SEEN_TOP`, `SEEN_SAC_MANA`; the knowledge accessors check the face-down list's emptiness
first). On the full pool: 5.56-5.70 us per decision (838,909 decisions), 836-857 games/s;
`Game::clone` 1.28-1.31 us; clone + determinize 1.65-1.79 us. Fuzzer 20,000 games: 131-137 s (with
the option-completion check; 125-137 s on main, section 15).

**M7e part 1** (546 cards, branch `m7e-rules`, the rules shortcuts of section 33): pinned the
same way, interleaved with main's code (7d2ad02) in three rounds, nothing else running. On main's
pool (`POOL_EXCLUDE` leaves the three new custom cards out: 840,190 decisions against main's
842,021, the games differing by the rules fixes) 5.76-5.85 us per decision against main's
5.64-5.69, pairs +1.6% to +3.7% (813-826 games/s against 835-843); `Game::clone` 1.33-1.47 us
(main 1.29-1.36), `size_of::<Game>()` 1,496 B (+104: the event group and its "one or more"
record, the entry group's timestamp, the damage sources' last known information, the rules
objects per definition, the entering counters and flags, the chosen route); clone + determinize
1.70-1.94 us (main 1.71-1.78). `Obj` stays 64 bytes (the entry group is a timestamp on `Game`,
not a field per object). On the full pool: 5.81-5.86 us per decision (842,993 decisions), 809-817
games/s. Fuzzer 20,000 games: 143 s (with the option-completion check).

**M7e part 2** (546 cards, branch `m7e-pay`, the payment model of section 34): pinned the same way,
interleaved with main's code (f89ada4) in five rounds, with a game running on the machine (the
rounds varied by up to 20%). On the same pool (no card added: 834,407 decisions against main's
842,993, the games differing by the payment's choices) 5.94-6.76 us per decision, median 6.18,
against main's 5.50-6.13, median 5.78: +6.9% at the medians (pairs +1% to +12%). The reason is the
payment's new work: each `PayMana` step costs about 1.0 us against 0.84 (measured with timers in
both builds: the virtual pool's spends, the card uses, the needs; 12% of the time in random play),
and every feasibility check carries the pool's units and the converters' ways. The first build was
+9%: `PayMana` grew the task enum to 280 bytes (boxed now), and an empty virtual pool and an
already finished payment took the full path (fast paths now). `Game::clone` 1.27-1.46 us (main
1.27-1.40), `size_of::<Game>()` 1,496 B (unchanged). Fuzzer 20,000 games: 181 s.

**M8a** (546 cards, branch `m8a-companion`, section 35): pinned the same way, interleaved with
main's code (c91f59e) in three rounds, nothing else running. The perf games have no sideboards;
the branch's setup asks the play-or-draw question first and shuffles after it, so it plays other
games (841,664 decisions against main's 834,407): 7.49-8.26 us per decision against main's
6.86-7.19 in that session. To separate code from games, a build of the branch whose setup draws
the same random numbers as main's (shuffles before the chooser, the chooser playing first) plays
exactly main's games (834,407 decisions): 6.47-6.68 us per decision against main's 6.60-6.65,
pairs -2.5% to +1.2%, so the difference is the game set (random play's cost per decision is
heavy-tailed), not the code. `Game::clone` 1.32-1.65 us (main 1.43-1.49), `size_of::<Game>()`
1,568 B (+72: each player's sideboard, an `Arc<[DefId]>`, the companion's slot and identity and
its use); clone + determinize 1.71-2.16 us (main 1.95-2.21). Fuzzer 20,000 games: 160 s with
other load (a quarter of the games with generated decks, sideboards and companions).

**M8b part b** (546 cards, branch `m8b-hidden`, section 36.11): `Game` holds the known pools (two
`Option<Arc<[DefId]>>`). `tests/perf.rs` `match_determinize` (unpinned): clone +
`determinize` through `MtgAz`: best-of-one 1.61-1.79 us, best-of-three 6.71-6.72 us (the hidden
pool sampled: ~5 us per world; section 36.11). The best-of-one game is unchanged but for turning
a face-down card face up (section 36.11).

M7b's column is measured with the process pinned to one logical CPU of a performance core
(`cmd /c start /b /wait /affinity 0x4 perf-....exe --ignored --nocapture`), best of three:
unpinned runs on this hybrid CPU varied by up to 50% with other work running (the earlier
columns are unpinned). On M7a's card pool (the planeswalkers left out of the random decks:
nearly the same games, 881,927 decisions against 885,439) M7b's code takes 5.98 us per
decision against M7a's 5.41 us, pinned the same day: +10% for the checks the new rules add.
The rest of the step to 6.3 us is the games with planeswalkers themselves. Those checks were
measured one at a time and taken off the hot paths where they could be (from +24%): the
static abilities of emblems walked by index instead of a chained iterator, a permanent's own
type-changing statics looked for only while one is on the battlefield (`n_self_types`), the
SBA test reading a permanent's types once, Teferi's lock looked for only for a card it could
stop, `Obj` back to 64 bytes.

M7b part 2's column is measured the same way, best of four, interleaved with M7b part 1's
code (main at dba496d) on the same machine the same day; this machine measured part 1's code at
6.81-6.94 us per decision that day (6.3 us the day before), so only same-day numbers compare.
On part 1's card pool (`POOL_EXCLUDE` leaves the 16 new cards and the custom face test cards
out of the random decks: exactly the same games, 906,266 decisions both) part 2's code takes
7.01 us per decision against 6.81 us, about +3%. The face checks first cost about +5%; four of
them were taken off the hot paths: a card's other-face options are computed only for a card
that has another face (`CardDef::other_face`), the Saga SBA looks only at enchantments, the
spell locks (Archon of Emeria, Jennifer Walters) are checked only when a spell could be cast,
and a face's timing is checked before the game is copied to evaluate it. A stubbed build
without the face checks matched part 1's time, so the rest is those checks (the lore counter
turn-based action, "deals damage" events, the day and night checks). `Obj` stays 64 bytes;
`Game` grew from 928 to 1,040 bytes (four vectors: optional actions done this turn, cards
discarded this turn, temporary triggers, spells exiled instead of going to a graveyard).

M7b part 3's column is measured the same way, interleaved with main's code (3ab5fb5) and best of
four. On main's card pool (`POOL_EXCLUDE` leaves the 16 new cards out) the branch takes 3-7% more
per decision in four interleaved sets the same day (6.95 against 6.57 us, 7.35 against 6.85,
7.44 against 7.22, 7.65 against 7.38: the machine drifted by 10% over the day, so only pairs
compare). The games are nearly the same (29 more decisions in 4,000 games: the rule changes reach
a few of main's games). Builds with one change stubbed out each locate the cost: without the copy
lookups in the characteristic accessors (`types_of`, `keywords_of`, `pt`, `has_subtype`, `name`,
`own_def`, `is_token`: one length check each while no copy effect exists) the branch was within 1%
of main; reverting the state-based 704.5p and bestow checks, or the last-known copiable values
recorded as permanents leave, made no measurable difference; moving the copy search out of line
(a cold function) didn't either. `Game` grew from 1,040 to 1,200 bytes (copy effects, copy
slots, last known copiable values and spells, prepared permanents, the entering copy's state).

M7b part 3b's column is measured the same way, interleaved with main's code (cffdc34) and best of
four. On main's card pool (`POOL_EXCLUDE` leaves the 28 new cards and the two new custom test cards
out) the branch takes about 4% more per decision (7.73 against 7.41 us best of four; the four
same-round pairs +1%, +6%, +7%, +4%); the games are nearly the same (3 more decisions in 4,000
games: the cost-order fixes and the ninjutsu reservation reach a few of main's games). The cost was
not located build by build as in part 3; the checks the part adds to every priority decision are
the candidates: the graveyard-ability scan, the stack-ability candidates, `Game::total_mana`'s
additional costs before reductions, the token test in graveyard counts, and the larger `EvCtx`
(the targeting item) that frames, stack items and waiting triggers copy. `Game` grew from 1,200
to 1,240 bytes (the initiative, the dungeons, the rules slot, `choosing_for`). On the full pool
the games are shorter (218 decisions against 228) and each decision costs more (8.44 us): the new
cards' decisions (ward payments, rooms, improvise) and their triggers.

A decision costs more because every priority decision computes the mana sources and, for
each card in hand and each ability, whether its cost can be paid (a matching with budgets),
and payments are decisions now. Most of the remaining time was small allocations under the
Windows system allocator (mana classes, candidate lists, SBA groups); M7c removed most of them
(below).

M7c's column (branch `m7c-perf`) recovers the per-decision speed M7b lost, with behaviour
unchanged (the digests of section 15 identical after every commit). **Method**: as above, the
process pinned to logical CPU 2 (a performance core), M7a's code (98b9463), main's (79ddaf9) and
the branch built side by side and run interleaved, six rounds, best and median reported. M7a's
pool is main's less the 85 names (cards and custom test cards) M7a lacks (`POOL_EXCLUDE`; what remains
is exactly M7a's pool in the same order, so the games are nearly the same: 882,060 decisions
against M7a's 885,439). The bench script waits while a `rustc` or `cargo` runs: another agent's
builds on the machine moved single runs by up to 50%. **Before**, the same day: main's code took
7.06 us per decision on M7a's pool against M7a's 5.67 (+23% to +28% in the six same-round
pairs; the M7b parts' own measurements summed to about +20%), and 9.16 us on the full pool.
**After**: 4.61 us on M7a's pool, 12-19% less than M7a's code in every round; 4.88 us on the
full pool, 45-47% less than main's code in every round. `Game::clone` and clone + determinize
are below M7a's code on its pool; `size_of::<Game>()` stays 1,240 bytes.

**Where the time went.** A sampling profile (a test-only tool, not committed: a thread
suspends the playing thread every 100 us, unwinds its stack with `RtlVirtualUnwind` and
symbolizes the frames with dbghelp, inline frames included, to function and line) and an
allocation-site count. On M7a's pool main spent 49% of a decision building priority options
(mana sources 13%, activated abilities 10%, castability from hand 9%, other zones 6%, hand slots
3%), 16% in the SBA check and 13% in payment decisions. Heap allocation and freeing took about a
quarter of the time: 49 allocations per decision (M7a's code: 48.6, so the count had not
regressed, but each round trip costs tens of nanoseconds under the Windows heap, and some runs
hit a slow case where one allocation site took 27% of the time: part of the run-to-run noise of
this section). The M7b additions that cost most: the "can't be activated" locks read by
walking the battlefield once per mana source (the walk itself older, but each step now through
the face- and copy-aware `types_of` and `abilities`), the 704.5d check walking every slot
through the copy-aware `is_token`, the escape granter looked for once per graveyard card, the
attachment and counter-pair lists built inside the "is any SBA possible" test, and power and
toughness walking the static abilities twice. A clone copied `Vec<Obj>` field by field (37% of
a clone).

**The fixes**, one commit each, each measured interleaved with the commit before it (best of
three or four rounds; per decision on M7a's pool unless noted):

| Fix | Effect |
|---|---|
| `Obj` is `Copy` (`Vec<Obj>` clones as one copy) | clone 1.32 -> 1.19 us |
| `determinize`: the unknown cards' list sized once | clone + determinize 1.78 -> 1.57 us (full pool 2.42 -> 1.96) |
| Payment feasibility without heap allocations: a class's sources, the candidate units and a card's ways inline (`svec::SVec`), a card's own sources left out by a filter instead of a copy of the classes | 49.0 -> 22.6 allocations per decision; 6.83 -> 6.16 us |
| Hand slots, the sorted hand and the hand-knowledge canonicalisation inline; the priority options sized once | 22.6 -> 14.9 allocations; 6.42 -> 6.10 us |
| SBA check: 704.5d looks only from the first slot made as a token or copy (`Game::first_token`: cards are never tokens), "is any SBA possible" builds no lists, power and toughness walk the statics once | 5.84 -> 5.69 us (pairs -2% to -10%) |
| The battlefield's activation locks read once per `mana_classes` (their union; a source's types only when there is a lock); the escape granter once per graveyard | 5.63 -> 4.80 us (the escape granter alone, measured later against the final code: -1% to -4% per round on the full pool, -2% to +2% on M7a's) |
| `determinize`: the unknown cards and identities inline | clone + determinize on the full pool 2.04 -> 1.61 us; `determinize` alone unchanged (the gain is fewer allocator round trips next to the clone's) |
| A cheap necessary test before building payment candidates (enough mana units for the non-Phyrexian pips: the pool plus every source at its largest production); the locks once per activation pass | 5.07 -> 4.91 us (full pool 5.43 -> 5.34) |

Tried and dropped: the static abilities' sources found once per SBA check (within noise);
moving rarely used `Game` fields behind one box (25 empty vectors clone and drop in 24 ns, 2% of
a clone: not worth the churn). **What is left** (full pool): priority options 46% (mana sources
12%, activated abilities 10%, castability from hand 9%, other zones 6%), the SBA check 14%
(power and toughness 5%), payment decisions 12%; 14.8 allocations per decision (the options, the
mana classes, several-mana yields) and 12.9 per clone (one per non-empty vector); 9% of the time
is memory moves and copies, the largest part (3%) the 232-byte `Task` (a `Frame` and a `PayMana`
inline) moved on every agenda push and pop: boxing those two would trade moves for allocations.
A faster allocator
(mimalloc) would likely cut the rest of the allocation cost, but it belongs to the binary, not
the engine.

**The allocator (M7c).** The Python module `mtgz._engine` (crate `py`, the binary that runs
self-play, evaluation and the search) uses mimalloc as its global allocator (`py/src/lib.rs`;
crate `mimalloc` 0.1, built from C by `cc` with MSVC here, clang on a Mac). The engine crate and
its tests keep the system allocator. `python -m mtgz.az.bench search --game mtg --threads 4
--games 25 --sims 64` (zero-core's Rust driver and search with a uniform evaluator), three rounds
interleaved with the same code on the Windows heap, the same games:

| | Windows heap | mimalloc |
|---|---|---|
| search time per simulation and thread | 29.7 / 32.5 / 34.5 us | 18.1 / 18.0 / 18.4 us |
| simulations per second (the driver, Python included) | 18,296 / 16,798 / 15,338 | 24,573 / 24,687 / 24,003 |
| leaves per second | 11,664 / 10,709 / 9,778 | 15,665 / 15,739 / 15,302 |
| wall time | 168 / 183 / 201 s | 125 / 125 / 128 s |

The search's own time falls by 40-48% and stops drifting (the Windows heap slowed by 16% over
the three rounds; mimalloc stayed within 2%): the clones, determinizations and options of four
search threads allocate concurrently, where the Windows heap contends and degrades. With a
network the evaluator's share grows, so self-play gains less than this in wall time.

## 18. Costs, activated abilities and mana abilities (CR 118, 602, 605)

**Costs.** A `Cost` is the mana (`ManaCost`: generic, coloured, two-colour hybrid, Phyrexian)
plus the other parts the cube uses: {T}, pay life, sacrifice this, sacrifice n permanents
matching a filter ("another" or not), discard this (cycling), discard n chosen cards,
discard your hand, exile this. An activated ability's "activate only if" gates (on the cost
struct in the IR: Mox Opal's three artifacts, the Verges' land types, a creature with power
4) and `activate_only_if` become `Cond`s. Spells carry additional-cost options (each a
`Cost` without mana; the caster pays exactly one).

**What can be activated (CR 602.5, 117.1b).** At priority: every activated ability of a
permanent the player controls, except mana abilities, and every ability of a card in their
hand, if: the timing allows ("activate only as a sorcery": own main phase, empty stack),
the restrictions hold, every part of the cost can be paid together (`cost_payable`: life at
least the payment, CR 119.4; enough sacrifice candidates; enough other cards to discard; the
mana with the budgets below), and every target requirement has enough candidates. As with
casting, an activation is offered only if it can be completed, so nothing is ever rewound.
{T} in the cost needs an untapped source and, for a creature, no summoning sickness (CR
302.6) unless it has haste; abilities without {T} and noncreature permanents' {T} abilities
can be used at once.

**Activation (CR 602.2).** `Choice::Ability(card, slot)` at priority: a card in hand is
revealed (602.2a: key, known to all); the ability goes on the stack as a
`StackKind::Ability { ability: AbilityRef { def, slot: Activated(i) | Hand(i) }, source }`;
targets are chosen (601.2c, so a sacrifice cost's permanent can be targeted and the ability
then has an illegal target); the mana is paid (`ActPay` -> `PayMana`), then the rest
(`PayCosts`: one `Sacrifice` decision per permanent, all sacrificed together; one
`DiscardCost` decision per card, each discarded at once so a chosen card is never hidden
while another decision waits; then {T}, life, discard hand, and the source's own sacrifice,
exile or discard). The activator then receives priority (117.3c). The ability resolves like a
triggered one, with its source's last known information if the source is gone (113.7a).

**Mana abilities (CR 605).** An `ActivatedDef` whose effect is `Mana(ManaAbility { prod,
damage_to_you })`. Productions (`ManaProd`): one mana of a mask (a land, Birds), fixed amounts
(Sol Ring), n of one chosen type (Black Lotus), n in any combination (Relic of Sauron), one per
permanent you control matching a filter (Gaea's Cradle), one of the colours among your
permanents matching a filter (Mox Amber). Costs a mana ability may have while paying: {T}, life, sacrifice or exile itself, a counter
removed (Pentad Prism without {T}: once per counter), one mana or {X} of its own (a need of the
same payment), a permanent sacrificed (Squandered Resources, Orcish Lumberjack: a source on each
permanent it may sacrifice). A source's use of its card is recorded (`Class::res`: tap,
sacrifice); a card's sources are exclusive alternatives unless one taps and the other sacrifices
it (tap it for mana, then sacrifice it, 601.2g). Where they are activated:

* mana abilities of permanents and of cards in hand (Spirit Guides): only while paying a
  cost (`PayMana`), whenever a cost is paid (casting, activating, "unless pays" during
  resolution: CR 605.3a). A hand source is one class per definition after the permanents
  (`HAND_ABILITY` ids); picking it reveals the card at once (602.2a), and while a payment is
  in progress the payer's hand sources are pinned against another viewer's `determinize`
  (section 10), so a payment never counts on a card that could be resampled away. A card
  cast from hand never pays for itself (it is on the stack). A hand source spends a card a
  later discard cost needs, so it counts against that reservation (`Keep.discard`);
* "activate only as an instant" ones (Lion's Eye Diamond): only with priority, never while
  paying (its ruling); costs, then a `ChooseMana` decision, mana to the pool, priority stays.
  Chromatic Star and Wizard's Rockets (a triggered ability as they go) are offered with priority
  as well (`Game::mana_with_priority`); every other mana ability only while paying.

**`PayMana` in detail.** State: the remaining cost, the picks so far, `Keep` (what later parts
of the cost need: the excluded source, life, a sacrifice or discard reservation; at most one
of the two per cost, the lowering checks). Stages:

1. Phyrexian pips: per colour, one `PayPhyrexian` decision "how many of these k pips are
   paid with 2 life" (0..k, only feasible counts; one decision per colour, not per pip, so
   no permutation branching). The life is paid at the end (601.2h comes after 601.2g).
2. Forced sources (`PayMana::forced`: a Lotus Petal delve exiles, sources an "exile n cards
   from your graveyard" cost needs) are picked first, their mana's type chosen.
3. Sources, one decision per pick, classes in non-decreasing order: several-mana classes
   first (Wizard's Rockets among them, offered when nothing else pays), then single-mana ones
   (canonical battlefield order within each). Picking a several-mana source asks for its
   production (`ChooseMana`, only productions the payment spends some of, leaving the rest
   payable); its mana joins the virtual pool (the player's pool and the several-mana sources'
   mana). A source with a mana cost of its own adds that cost to the payment as a `NOT_MADE`
   need; its mana is `MADE` and never pays such a need. `PayMana::vm` is the most of the
   virtual pool the payment can spend with single-mana sources paying the rest (`most_spent`,
   every spend of that size tried; `None` while several-mana sources must still be picked).
   Single-mana sources are picked as a set (a tap and a sacrifice of one card both, the tap
   first); when they and `vm` mana cover the cost and needs exactly, the sources are done.
4. `PayPool` (CR 106.4, 601.2h): the distinct pools the payment can leave (`pool_rems`: two
   units of one type and tag are one; artifact-only {C} spent before other {C}, haste {R}
   after other {R} except on a creature spell); asked when there are several, `NUMBER` = the
   k-th (the first keeps the most types).
5. Nothing has been activated yet: every pick is checked against the state at the start of
   the payment. At the end everything is activated in an order the rules allow (601.2g lets
   the player choose): sources that keep their permanent first (so "activate only if you
   control three artifacts" still holds for Mox Opal when Black Lotus is sacrificed), life
   payments before damage (Mana Confluence before Ancient Tomb), sacrifices last; the pool
   is left as chosen; delve's forced sources are exiled from the graveyard; then the
   Phyrexian life and the cost's life after the mana (X life, the Citadel's,
   `PayMana::life_after`).

Feasibility (`can_pay_with`, `coverable`) is exact: a matching of the pips' acceptable types
(`payable`, with the chosen units forced in), with budgets (`payable_within`): life (CR
119.4; when life is paid later in the same cost, the reserved amount and the damage mana
abilities deal are counted, since that damage comes first), and permanents a later sacrifice
needs (a Lotus Petal cannot pay for Tinker's mana if it is the only artifact to sacrifice), or
hand cards a later discard needs (a Spirit Guide cannot pay if it is the card to discard).
Each card contributes at most one way (its mana abilities are alternatives; a tap way and a
sacrifice way also together); a way whose activation takes budget is all of its mana or none
(Black Lotus's three mana come from one activation); an ability with a mana cost adds its need
with its way. Phyrexian splits and "n of one chosen type" productions are tried one by one.
The pool's mana is among the candidates (any of it may pay); with Wizard's Rockets usable, a
cost unpayable otherwise is tried with every unit of any colour (`Cands::convert`). Delve
spells count each mana source that sacrifices itself and is a card as its mana and one more
generic mana (`Game::delve_classes`), and "exile n cards from your graveyard" counts such
sources toward n (`exile_short`).

## 19. Lands (CR 305, 614)

* **Mana.** A land's mana abilities are its explicit mana abilities plus, for the colours of
  the IR's `produces` that no explicit ability with exactly {T} as its cost already makes, an
  intrinsic "{T}: Add one mana of those colours" (basic land types, CR 305.6; the rules text
  of other lands; with `produces_life_cost`, "{T}, Pay 1 life", the horizon lands). It comes
  first in `activated`, so on a dual land and on Karakas ability 0 is the mana ability.
  `produces` is a summary when an explicit ability covers it (Wasteland's {C}, the Verges);
  a land with neither relies on Python's generic fallback and is blocked (section 14).
* **Entering tapped** (614.1c): `EntersTapped::Always` (Triomes), `Unless(cond)` (fast lands:
  "two or fewer other lands", evaluated before the land moves, so it counts the others;
  Starting Town: "your first, second or third turn", `Player.turns`). Applied in
  `put_onto_battlefield`, so it holds however the permanent enters.
* **Shock lands** (614.12): "As it enters, you may pay 2 life. If you don't, it enters
  tapped." The land is put onto the battlefield and an `AsEnters` task asks its controller
  (`YesNo`) before anything else happens; nothing can observe the order, and the land is
  public while the question is open (so `determinize` cannot touch it). At less than 2 life
  only "no" is offered (119.4).
* **Cycling** (702.29): a hand ability ("{3}, discard this: draw a card"), activated from
  hand at instant speed with `HAND_ABILITY` ids.

## 20. Findings for the IR and the next milestone

* IR defect: Strip Mine has no "{T}: Add {C}" (it relied on the Python fallback); blocked
  until mtg-bot exports it (fixed: mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca, section 32).
* M7a IR findings (section 23.10): Celestial Colonnade and Creeping Tar Pit lack the colours
  their animation sets; Robber of the Rich's permission duration; Get Lost's Map token (the
  engine now builds Map from the printing other cards carry).
* M7b IR finding (24.8): Minsc & Boo, Timeless Heroes's Boo token printing has no colour
  ("a legendary 1/1 red Hamster creature token"; no `color_indicator`, so Boo would be
  colourless). The card stays blocked (`IR:CreateToken.token_printing.color_indicator(Boo is
  red)`, from `lower::IR_DEFECTS`) until mtg-bot fixes the IR; its tests run on a corrected
  copy. (Fixed by mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca, section 32.)
* IR gap: Get Lost's rider (`ControllerCreatesTokens { token_name: "Map" }`) names a token
  the IR does not describe (Python looks it up in `TOKEN_PRINTINGS`); the IR should carry the
  printing, as `CreateToken.token_printing` does (Map also needs explore).
* M7b: `docs/ir_defects.md` lists the IR defects for mtg-bot's parser, each with the Oracle
  text, what the IR says and should say, the parser function that produces it, a suggested fix
  and the engine-side status. Confirmed: Strip Mine's mana ability; Colonnade and Tar Pit's
  colours and creature type (`_try_build_manland`); Get Lost's Map printing; Robber of the
  Rich's duration; `{C}` folded into generic mana (the IR's `ManaCost` has no colorless pip:
  Sowing Mycospawn's "Kicker {1}{C}" reads `{2}`); target specs set in `__post_init__` and not
  exported (Thieving Skydiver; six more classes, re-derived here for Mishra's Bauble, Urza's
  Bauble, Grindstone, Displacer Kitten). Suspected: an Aura's bonus in `equipment_bonuses`
  (Animate Dead), Necromancy's "as though it had flash" as the flash keyword, transform back
  faces carrying the front face's mana cost, sentinels (Fastbond's 1000000, Pest Infestation's
  `count_max: 0`, Wizard's Rockets' `amount: 0`), target requirements that exist only in Python
  methods. The 118 cards first tested in M7b showed no IR defect and no engine bug.
* M7b part 2 IR findings (25.11, docs/ir_defects.md 9, 13, 14): Ajani, Nacatl Avenger's 0
  flattens a reflexive trigger into an `IfCondition` (confirmed; the card is blocked by
  `lower::IR_DEFECTS` and tested on a copy; fixed by mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca, section 32); transforming back faces repeat the front's mana
  cost (suspected; the lowering reads it as none); `TriggeredAbility.once_per_turn` stands for
  both "do this only once each turn" (603.2h, She-Hulk) and "this ability triggers only once
  each turn" (Enduring Innocence), told apart here by the shape of the effect (suspected).
  The modal double-faced cards that can also transform (Jennifer Walters, Norman Osborn)
  describe their back face twice, as `mdfc_back_face` and `transform_back_face`; the two agree
  (the lowering checks it). Apart from these, the 16 cards' IR matches their Oracle text.
* M7b part 3 (26.12): the IR of the 16 copy cards matches their Oracle text; no new defect.
  Two more effect classes carry their targets only in Python (Shifting Woodland's graveyard
  card, Lutri's spell: docs/ir_defects.md 11, re-derived in the lowering), and Chain of Smog's
  `needs_target_with_cards_in_hand` is a Python behaviour switch (section 14). The repeatable
  and alternative costs new here (squad, offspring, bestow, Springheart Nantuko's {1}{G}) are
  checked against their `*_str` twins (replicate's cost is a string only).
* M7b part 3b (27.14): the IR of the 28 cards matches their Oracle text; no new defect. The IR
  leaves rules to code (Undercity's rooms, the initiative's abilities, mobilize's and storm's
  abilities), and Fallen Shinobi's `TargetPlayerExilesTopNYouMayPlayThem` has no target (its
  "that player" is the damaged player). Left with one blocker near this part: Tersa Lightshatter
  (a random graveyard card), Celestial Colonnade and Creeping Tar Pit (IR), the mana abilities
  with a mana cost (Chromatic Star, Loot), spend restrictions (Mishra's Workshop), Balance.
* M7d's IR fixes (section 32): the IR of mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca fixes docs/ir_defects.md 1, 2, 4,
  12, 13 and 15-18; every cube card is supported and `lower::IR_DEFECTS` is empty. Still in the
  IR, read as before: entries 3, 5-11 and 14 (Get Lost's Map, the {C} kicker, the targets set in
  `__post_init__`, Animate Dead's bonus, Necromancy's flash, transform back faces' mana cost, the
  sentinels: Wizard's Rockets' `amount: 0` too, targets only in code, `once_per_turn`).
* Lowering: the `*_str` cost twins (`mana_cost_str`, `kicker_cost_str`, `mana_str`, ...) are
  ignored as display forms; checking them against the structured cost (rule 4a, section 3) would
  have caught the lost `{C}`.
* IR naming: `EachOpponentSacrificesCreature` means "a nontoken creature" (Sheoldred's
  Edict; Python agrees); `Scry.bottom_choice` and `SearchLibraryForCard.may_fail` are Python
  behaviour switches the engine ignores in favour of the CR (section 14).
* M4b-1's estimate for these features was about 195 cards; M4b-2 supports 191 (with
  Timetwister and Stoneforge Mystic, which fell out of the same ops). Left behind with one
  blocker in this area: Torsten, Founder of Benalia (and Once Upon a Time, Atraxa) put cards
  on the bottom "in a random order", where the owner knows a set of cards but not their
  order (owner-side partial knowledge, not built); Ertai Resurrected (targets an ability on
  the stack); Kolaghan's Command (a graveyard target); Collective Brutality (escalate, a
  pump); Three Steps Ahead (spree, a token copy); Suplex (a replacement); Knight of Autumn
  (counters); Get Lost (above); Mishra's and Urza's Baubles (delayed draw); Demonic
  Consultation (naming a card).
* Next by the greedy order (section 16): static abilities, alternative costs (Force of Will,
  Daze), returning cards from graveyards, the "alternative cost was paid" condition.
* For M5 (the zero-core binding): `encode` must describe what each slot of the pending
  decision means (the hand, library-search, pile and seen-hand lists are sorted by
  definition, so a slot's card depends on the list), the public state by canonical places
  (as `view_of` does), the viewer's exact knowledge and clouds; `force_chance` must replay a
  key in another world (a draw or a look: move the requested identity into place among the
  viewer's unknown cards, consistent with the clouds; a random discard: pick that identity);
  `KEY_AUTO` keys mean single-option decisions of other players split search nodes (the
  alternative is to expose every decision, which zero-core would step anyway). Done in M5a
  (section 21).

## 21. The zero-core game `mtg` (M5a)

`engine/src/az` makes the engine a zero-core `SearchGame` named `mtg`: one game of Magic
between two 40-card decks of supported cards. The repository is laid out per zero-core's
docs/new_game.md: zero-core pinned as the git submodule `third_party/zero-core` (docs/pinning.md
there; the pin is the staged gitlink, `tools/build_py.sh` refuses another core), the binding
crate `py/` (`zero_py::engine_module!` -> Python module `mtgz._engine` with the games `toy` and
`mtg`), the package `mtgz` (`mtgz/az/__init__.py` registers the `GamePackage`: presets, the
default spec, dashboard port 5340, hub id `mtg_zero`; `mtgz.az.<module>` are shims of the core's
`zero.<module>`), the run tools (`tools/{supervise,kill_run,pause_run,fork_run,launch_hidden}.py`)
and the Python tests (`tests/`).

### 21.1 The `SearchGame` mapping

| zero-core | `MtgAz` |
|---|---|
| `NUM_ACTIONS` | 1024 (section 11: decision-relative ids) |
| `NUM_FEATURES` | 13,532 (21.2) |
| players | 2 |
| `parse_spec` / `new_game(spec, 2, seed)` | `MtgSpec` (below); the game seed draws the decks and sideboards (21.3, 35.7), then `Game::with_sideboards` (the chooser; the first decision is its play-or-draw choice, section 35.1) |
| `to_move`, `legal_actions`, `step` | the pending decision's player, its ids, `Game::apply` with its chance keys (section 10) |
| `force_chance` | `Game::force_chance` (section 10) |
| `determinize(viewer, rng)` | `Game::determinize` with an engine RNG seeded from zero-core's |
| `result` | 1 / 0 for a win; 0.5 each for a draw (both players lost at once, or the turn cap) |
| `margin(seat)` | (life of the seat - life of the opponent, each floored at 0) / 20, clamped to [-1, 1] |
| `score`, `rounds` | life (reports), the turn number |
| `encode(viewer)` | `Game::encode_features` (21.2) |
| bots | `random` (zero-core), `heuristic` (21.4) |

The spec (JSON, every key optional): `{"max_turns": 60, "mulligans": true, "decks": "random",
"lands": [16, 17], "eval_decks": 16, "eval_seed": 1}`. `max_turns` counts both players' turns
(a draw after it; 60 = the engine default: ~3% of heuristic-vs-heuristic games and ~1% of
heuristic-vs-random games reach it, against ~20% at 40); `decks` is `"random"` (two new decks
per game), `"eval"` (a pair of the fixed evaluation set) or `{"p0": [names], "p1": [names]}`
(M8a: optionally `"p0_sideboard"`, `"p1_sideboard"`; random and evaluation decks come with
generated sideboards, 35.7); M8a's `"chooser": 0 | 1` fixes the seat that chooses who takes the
first turn (default: chosen at random each game, CR 103.1). Constructed (section 54):
`"constructed": true` plays the `decks` lists with the card table of constructed play
(`CardDb::constructed`: the cube's cards and the legacy ones) under CR 100.2a / 100.4a (at least 60
cards, four of a card but basic lands over deck and sideboard, a sideboard of at most 15), best-of-one
only. M8b (section 36): `"format": "bo1"`
(the default: one game) or `"bo3"` (the episode is a best-of-three match with sideboarding; its
result is the match's), `"pools": true | false` (random and evaluation decks built by the
deckbuilder from generated 45-card pools, the rest of the pool the sideboard; default true for
`"bo3"`). Unknown keys are errors. The cube IR is compiled into the binary (`db::CUBE_IR`), so the Python
module does not depend on where it was built.

### 21.2 `encode`: layout v1 (13,532 floats)

(M5a's layout. Layout v2, 22.2, keeps these blocks but gives every card a stable identity:
the card code, the bags and the context's card no longer use the card table's index.)

A fixed-width vector for one viewer ("me"), with no hidden information: everything in it is
public or the viewer's own knowledge (its hand, the cards it knows exactly and where, its
clouds, its own decision's options), so it is the same in every world `determinize(viewer)` can
produce (tested for both viewers, 21.8). It is large and simple on purpose; M5b replaces it with
a network that reads the board card by card.

**Card code** (48 floats): the definition's index in the card table (`CardDb` order: the 191
supported cards by name, then the 30 token definitions; capacity `D` = 256) as two one-hots
(index / 16 and index % 16: 32 floats) plus a descriptor (16: artifact, creature, enchantment,
instant, land, sorcery; the five colours; mana value / 8, printed power / 8, printed toughness
/ 8; token; legendary). The card table is the layout's: a change to the supported cards changes
the indices (checkpoints belong to one card table).

| Block | Offset | Floats | Content |
|---|---:|---:|---|
| globals | 0 | 128 | turn / 50; step one-hot (14); me active, me priority, passes, I started, turns left / 50, stack size, waiting triggers, extra turns, over; per player (me, then the opponent, 40 each): life / 20, hand, library, graveyard, exile sizes, lands played, mana pool (6), mulligans, kept, drew from an empty library, lost, turns, a permanent left (revolt), permanents, creatures, lands, untapped lands, total power and toughness, creatures ready to attack, artifacts, tokens, extra turns |
| context | 128 | 384 | the pending decision: `DecisionKind` one-hot (32); the decider is me; the asking task (20: priority, mulligan, bottom, attackers, blockers, damage, discard, targets, mana payment, a resolving effect, triggers, SBA, costs, cost option, modes, mana choice, as-enters, ...); the card behind it as a full one-hot (256: the spell being cast, the ability resolving or being paid for, the land entering, the attacker dividing damage) with its ability slot (spell, trigger, activated, from hand; index; mode); the resolving op (37 kinds) and program counter; the mana still to pay (generic, W U B R G C, hybrid, Phyrexian); damage left to divide; the target requirement; the next attacker slot; the blocker index; bottoms left; the looked-at pile's size; my number of options; modes chosen; cost cards picked |
| bags | 512 | 13 x 256 | counts per card-table index (0.25 per card): my hand, my battlefield, the opponent's battlefield, the two graveyards, the cards each player owns in exile, my library (its contents, which its owner knows), the opponent's hidden pool (hand and library cards I do not know exactly: open decklists), the opponent's hand cards I know exactly, the identities my clouds place in the opponent's hand, my library cards I know, the opponent's library cards I know exactly plus the identities my clouds place there |
| tops | 3840 | 2 x 3 x 50 | the top three positions of my library, then the opponent's: present, known to me, card code |
| my permanents | 4140 | 32 x 88 | `MY_PERM` slots 0..31 (timestamp order): card code + 40 status floats: present, tapped, damage / 8, power / 8, toughness / 8, summoning sick (no haste), attacking, blocked, blocking, the blocked attacker's slot, struck first, the blocker this `DeclareBlockers` decision is about, the attacker and the blocker of this `AssignDamage` decision, its slot id is an option of my decision, `ACTIVATE` options for abilities 0..3, targeted by a stack object, blockers on it / 4, 15 keywords, token, entered this turn, controlled by a non-owner |
| opponent permanents | 6956 | 32 x 88 | `OPP_PERM` slots 0..31, the same |
| hand | 9772 | 16 x 56 | `HAND` slots 0..15 = my hand sorted by (definition, card): card code, present, copies in hand, `HAND` and `HAND_ABILITY` options, known to the opponent, a land |
| stack | 10668 | 8 x 72 | `STACK` slots 0..7, top first: the source's card code, present, spell / activated / triggered, controller is me, modes, cost option, targets, and the first two targets (me, the opponent, my permanent's slot, an opponent permanent's slot, a stack slot) |
| triggers | 11244 | 4 x 52 | `TRIGGER` slots 0..3 = my waiting triggers: card code, present, ability index, option |
| list | 11452 | 40 x 52 | `LIBRARY` slots 0..39 of my own decision: the searched library, the looked-at pile or the seen hand, sorted by definition (section 11): card code, present, option, copies in the list |

Slots beyond the encoded ones (permanents 32..95, hand 16..63, stack 8..31, list 40..63) can
still be chosen (the policy head covers all 1024 ids); the bags count every card. Options are
encoded only when the viewer decides (its own legal actions, which `determinize` keeps), and so
is the list block (another player's pile or search is hidden from the viewer). The `NUMBER`,
`YES`, `PASS` and `PLAYER` ids mean what the context says. Not encoded yet: counters (the engine
has none), the graveyard order, which permanent a pending trigger came from. Encoding costs
about as much as the rest of a simulation: the Rust search runs ~41 us per simulation per
thread on `mtg` with a constant evaluator (`python -m mtgz.az.bench search`, 8 threads: 77k
simulations/s) against 1.65 us on the toy.

### 21.3 Decks

`decks::generate` (seeded by the game seed): two colours at random; 23-24 singleton spells of
those colours or colourless, weighted to creatures (3), interaction (2: damage, removal,
counters, discard, edicts, token makers), other coloured cards (1) and colourless ones (0.5),
with at most 3 spells of mana value 5+ and 4 of mana value 4; 16-17 lands: each supported
nonbasic land that makes or fetches exactly the two colours (p 0.9), one of them only (0.5,
Karakas; a fetch with one land type on colour), both among more (0.4, a triome), any colour
(0.3, Mana Confluence, Prismatic Vista, Starting Town), colourless utility (0.15: Ancient Tomb,
Wasteland, Library of Alexandria, Tolarian Academy, Gaea's Cradle), at most 9 of them; then
basics split by the spells' coloured pips (at least a third each). `decks::check` validates a
deck (40 cards, supported non-token cards, singleton but basics, 16-17 lands, at most two
colours of spells). Only the supported cards are used, so the pool grows with the engine.
M8a (section 35.7): `decks::generate_pool` adds a sideboard of seven cards to each deck and
builds one deck in four for a companion; `MtgSpec::pools(seed)` gives decks and sideboards
(`decks(seed)` its decks), `decks::eval_pools` the evaluation set's.

**The evaluation set**: `decks::eval_pair(gen, seed, i)` = two decks generated from a fixed
seed per pair; spec `"decks": "eval"` picks pair `split64(game seed) % eval_decks`. zero-core
plays self-play and evaluation with one spec, so `mtgz.az.run` patches the loop
(`mtgz.az.evaldecks`): every driver created without recording samples (the matches against
the yardstick and random, the Elo matches, the random-vs-yardstick reference) gets the spec
with `"decks": "eval"`; self-play keeps new random decks every game. The run's `spec` names
both (`{"max_turns": 60, "eval_decks": 16, "eval_seed": 1}`).

### 21.4 Bots

* `random` (zero-core's `RandomBot`): uniform over the legal ids.
* `heuristic` (`az/bots.rs`; the Elo anchor of the runs, 1000): plays a land; casts the most
  expensive spell it can afford in its main phases, skipping a targeted spell without a target
  on the right side for each required target (an opposing one where the effect harms, its own
  where it helps) and a sweeper that would hit more of its own creatures; announces the largest
  X it can pay; counters an opponent's spell when it can; burns the opponent out when the
  damage is lethal; activates abilities (below: loyalty abilities, fetch lands, equip, crew,
  removal, upgrades, card draw and other sinks); attacks with each creature that no untapped
  opposing creature can kill and survive (all of them when the opponent has no untapped
  creature); blocks when the blocker survives or trades for a creature of at least its mana
  value, chump-blocks when the unblocked damage would be lethal; assigns lethal damage in
  order; aims each target by what the effect does to it (the first op of the program that uses
  the requirement, `aim_in`; else whether any effect of the ability harms): harmful effects at
  the opponent's most valuable permanent they remove (or the player), helpful ones at its own
  most valuable permanent (an Equipment at a creature that can attack now; from a graveyard,
  the costliest card); keeps hands of 2-5 lands (one mulligan at most); gives up lands when
  flooded, else its most expensive card; sacrifices, or returns as a cost, its least valuable
  permanent; pays with the first source offered, except the source of the ability being paid
  for (a manland tapped for its own mana could not attack). It reads only public information
  and its own cards. A yardstick, not a goal.

**Activated abilities (M7c).** At priority the bot reads each offered ability's kind from its
definition, the effect's ops and the cost, never from the card's name (`classify`), and
decides by the kind, the step and the board. "Worth" is `value` (mana value x 2; a creature
adds 2 + power x 2 + toughness; a land 3; a token 2 less: a 2/2 for two is worth 12).

| Kind (from the ops) | When | If |
|---|---|---|
| Loyalty ability | its main phases, before spells | the ultimate (the most negative ability, -5 or less) whenever it can be paid; else a minus or zero that removes an opposing permanent worth 7 or more, if the planeswalker keeps 1 loyalty or the target is worth 15; else a plus aimed at an opposing permanent worth 8; else the biggest plus (one that only adds mana last), else a zero, else a minus that does something (draw, tokens, recursion, an upgrade) and leaves 1 loyalty |
| Fetch (search for a land onto the battlefield) | its main phases and the opponent's end step, before spells | 2 life left after paying |
| Land tutor (landcycling, Expedition Map) | its main phase before its land drop (before spells), or the opponent's end step | no land in hand, fewer than 6 lands |
| Ninjutsu | its declare-blockers step | an unblocked attacker of lower mana value |
| Lethal damage | its main phases and declare-blockers step, the opponent's end step | one activation, or all its cost allows (the counters on the source, the permanents it can sacrifice), deals the opponent's life; the rest of the turn its damage goes to the player |
| Removal (destroy, exile, damage, bounce, steal, -X/-X, "loses all abilities") | its main phases or the opponent's end step, after spells | an opposing target worth 7 that the effect removes (`effect_value`: lethal damage, not indestructible; a bounce counts half); a creature sacrificed, or the last counter removed from one, only for a target worth more |
| Equip | its main phases | the Equipment is not attached to a creature it controls |
| Crew | its first main phase | the Vehicle can attack and no blocker would eat it, and creatures that would not attack (summoning sick, no power, or eaten by a blocker) have the power |
| Manland ("becomes a creature until end of turn") | its first main phase | untapped and not summoning sick |
| Upgrade (+1/+1 and level counters, monstrosity, transform, "becomes" with no end, explore) | its main phases; at instant speed also the opponent's end step | it changes something (not a second monstrosity, not "becomes 4/4" on a 4/4, not "if it's a Spirit" on a non-Spirit); no counters removed |
| Put a card from hand onto the battlefield | its main phases or the opponent's end step; sacrificed at end of turn (Sneak Attack): its first main phase | a card in hand it applies to (Sneak Attack: one costing more than its lands) |
| Tokens; recursion from the graveyard | the opponent's end step or its second main phase (sorcery speed: its main phases); tokens sacrificed at end of turn: its first main phase | |
| Card draw (more draws for it than for the opponent, look and keep, exile and play, tutors) | the opponent's end step (sorcery speed, or "play them this turn": its main phases) | |
| Cycling | the opponent's end step or its second main phase | stuck: a land card with 5 lands in play and 2 in hand; a spell costing more than its lands + 2 |
| Mill a player; gain life | the opponent's end step | life: 10 or less |
| Never | | untapping (Basalt Monolith, Voltaic Key), protection, library rearranging (Sensei's Divining Top), symmetric draws (Faerie Mastermind, Loran), mana abilities at priority (Lion's Eye Diamond), "becomes a copy", graveyard hate, temporary pumps, counterspells, giving the source away (Wishclaw Talisman) |

Costs, for every kind but loyalty: life only with 8 left (2 for a fetch); a discard only when
flooded (6 lands, or 5 and a land in hand); a permanent other than the source sacrificed only
if it is a token (except for removal worth more, or lethal damage); the source sacrificed, if
it is a land or a nontoken mana rock (Horizon Canopy, Mind Stone), only when flooded, if it is
a creature only for removal. Order at priority: a land; a counterspell against an opponent's
spell; a lethal burn spell; tier 2 (loyalty, fetches, a land tutor before the land drop,
ninjutsu, lethal damage) by score; the most expensive spell worth casting; then tier 1, the
highest score first (removal 500 + the target's worth, equip 400, crew 380, manland 370,
put-in 350, upgrade 300 + its mana, tokens 250, recursion 220, draw 200, land tutor 180,
cycling 150, mill 100, life 50).

**Caps.** An ability is activated at most once a turn (a loyalty ability 3 times, for Comet's
extra activations; one whose cost sacrifices another permanent, removes counters or discards,
20 times), and a player at most 40 times a turn (`cap`, `TURN_CAP`; the counts reset each turn,
and when the game's decision count drops: a new game). Untap abilities are never used and no
mana is made at priority but a loyalty ability's (once a turn), so mana-costed activations are
bounded too: every game ends (`tests/bots.rs`, 8,000 games with every decision asked).

Decision kinds whose handling changed in M7c: `Priority` (activations), `Target` (per-requirement
aim; graveyard cards by mana value; an Equipment's creature), `ChooseMode` under X (the largest;
other modes, the times a repeatable cost is paid, still the first), `PayMana` (not the paid
ability's source), `ChoosePermanent` while paying a cost (crew, "tap an untapped Rebel": `PASS`
once allowed, else a summoning-sick creature, else the smallest), `BounceCost` (the least
valuable; was the first offered). `az::bots::HeuristicV1` (module `v1`) is the policy before
M7c, unchanged, kept to measure against.

Measured in M7c (`cargo test --release -p engine --test bots -- --ignored --nocapture
--test-threads=1`; `BOTS_GAMES=2000` deck pairs from the game seeds, each pair played with both
seatings, scores +- 1 standard error over pairs; the 438-card pool, `max_turns` 60):

| Match | Score of the first | Games | Draws |
|---|---:|---:|---:|
| heuristic vs `HeuristicV1` | 62.9% +- 0.6 | 4,000 | 0 |
| ... a deck has a planeswalker | 63.4% +- 0.6 | 3,288 | 0 |
| ... no planeswalker | 60.4% +- 1.3 | 712 | 0 |
| ... 8-11 nonland cards with activated abilities in the two decks | 61.1% +- 1.5 | 588 | 0 |
| ... 12 or more | 63.3% +- 0.6 | 3,378 | 0 |
| heuristic vs random | 95.8% +- 0.3 | 4,000 | 0 |
| `HeuristicV1` vs random | 91.3% +- 0.4 | 4,000 | 1 |

62.9% is about +92 Elo: the anchor of the runs (1000) is a stronger player from M7c on, so scores
against the heuristic before M7c (sections 21.6, 22.4) and after are not comparable.

Every deck pair has nonland cards with activated abilities (7 or fewer in 34 games). Where the
gain comes from (the same 4,000 games against `HeuristicV1`, abilities switched off by kind in
a local experiment): no activations at all (the targeting, X and cost changes only) 53.1% +-
0.3; loyalty abilities only 55.0% +- 0.4; fetch lands only 56.0% +- 0.5; all but loyalty
abilities 60.9% +- 0.5; all but fetch lands 59.1% +- 0.5.

Speed (3,000 games each, one thread, decisions with two or more options): `HeuristicV1` vs
itself 1,166 games/s (15.4 turns, 252 decisions per game, about 0.1 us per bot decision);
the heuristic vs itself 1,178 games/s (14.4 turns, 186 decisions, about 0.3 us); heuristic vs random 1,380
games/s, `HeuristicV1` vs random 1,189. The bots take 2-7% of the time; the games are shorter, so
evaluation games are not slower.

As a yardstick it still has blind spots: it activates nothing in response or during combat
(no protection from Mother of Runes, no pump or removal on an attacker, no sacrifice of a
creature about to die), holds no mana for instants (its sinks use the opponent's end step),
crews and animates manlands only before its own combat (never to block), uses a sacrifice
outlet only on tokens or for lethal damage, takes an ultimate as soon as it can be paid (even
one that leaves the planeswalker at 0), and its attacks and blocks are unchanged (aggressive,
one blocker per attacker).

Measured in M5a (`cargo run --release -p engine --example az_probe -- <a> <b> <games> <spec>`,
1000 games each, seats alternating, `max_turns` 60; the 191-card pool, before M7c):

| Match | Score of the first | Draws | Turns / game | Decisions / game | Games / s (1 thread) |
|---|---:|---:|---:|---:|---:|
| heuristic vs random (random decks) | 91.8% | 1.1% | 27.8 | 344 | 466 |
| heuristic vs random (evaluation set) | 93.2% | 1.2% | 26.2 | 333 | 488 |
| heuristic vs heuristic | 50.6% | 2.5% | 29.4 | 547 | 307 |
| random vs random | 50.8% | 3.3% | 39.0 | 250 | 532 |

A decision here is one with at least two options (the engine answers single-option decisions
itself, section 4): many are priority passes and one-source-at-a-time mana payments.

### 21.5 Running

```
bash tools/build_py.sh                                          # mtgz/_engine.pyd (inits the pinned core)
python -m mtgz.az.run --run runs/x --preset mtg-smoke           # 3 tiny iterations, ~1 minute
python -m mtgz.az.dashboard --host 0.0.0.0                       # port 5340 (hub mtg_zero)
```

Live runs go in `~/mtg-zero/runs/<run>` and run from a pinned worktree (zero-core docs/az.md 1):
`git -C ~/mtg-zero worktree add --detach ~/mtg-zero-wt/run-<sha> <sha>`, then
`CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/run-<sha> bash tools/build_py.sh` there, then the
search smoke there as the gate (`python tools/search_smoke.py`, section 15: no failure; 7.5
minutes), then
`python tools/launch_hidden.py <run>/supervisor_console.txt python tools/supervise.py <run>
--preset mtg-1`. While the pinned core commit is not on GitHub, the submodule clone of a new
worktree cannot fetch it: `git -C third_party/zero-core fetch ~/zero-core main && git -C
third_party/zero-core checkout <pin>` first. Pause / resume / stop as in every Zero project
(PAUSE and STOP files, `tools/pause_run.py`, the dashboards). zerodash lists the runs
(`zero-core/dashboard/projects.json`, project `mtg`). CPU work on the MacBook (`tools/mac.sh`) and a
run's evaluations there (`--eval-remote mac`): section 44.

Presets: `mtg-smoke` (CPU, a 32 x 1 network, 16 games per iteration at 8 / 2 sims, 8 games
against the heuristic) and `mtg-1` (the first run's settings, 21.6).

### 21.6 The first self-play run (`runs/mtg1`)

`runs/mtg1` (2026-10-22 01:47-03:43, 1 h 56 min; stopped with a STOP file after iteration 9,
left stopped): preset `mtg-1` from the pinned worktree `~/mtg-zero-wt/run-82767fd` (core
7225a65), tabula rasa, a 256 x 3 MLP (4.2M parameters), CPU only (8 search threads, 6 torch
threads; the GPU was busy with two Kingdom Builder Zero runs, which were not touched), rolling
self-play with 384 games in flight, 32 / 8 simulations (p_full 0.25), visit-count targets,
chance_k 0, value_q 0.5 (warm-up 3), evaluation at 32 simulations on the 16 fixed deck pairs.
After iteration 2 the run was restarted from its checkpoint with 800 self-play games per
iteration (was 400) and 20-game Elo matches (was 40): the Elo matches (both seats searching,
few games in flight: small CPU batches) took 3 minutes each and grew to 4 per iteration.

| Iter | Self-play games (cum.) | Samples | Games / h | Leaves / s | Decisions / game | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100) | vs random (50) | Elo (final fit) | Minutes |
|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|
| 0 | 0 | | | | | | | 8.5% +-2.8 | 59% | 436 +-34 | 2.0 |
| 1 | 400 | 18,008 | 10,178 | 11,584 | 180 | 0.90 | 0.991 / 0.393 / 0.90 | 7.0% +-2.5 | 78% | 542 +-31 | 5.9 |
| 2 | 800 | 21,528 | 16,847 | 14,417 | 214 | 0.90 | 0.982 / 0.462 / 0.95 | 8.5% +-2.8 | 83% | 567 +-31 | 9.8 |
| 3 | 1,600 | 44,940 | 10,347 | 11,767 | 224 | 0.91 | 0.980 / 0.483 / 0.95 | 14.5% +-3.5 | 82% | 594 +-32 | 11.1 |
| 4 | 2,400 | 50,862 | 15,901 | 15,483 | 256 | 0.91 | 0.982 / 0.483 / 0.95 | 10.5% +-3.1 | 79% | 561 +-31 | 10.9 |
| 5 | 3,200 | 53,991 | 10,947 | 11,937 | 270 | 0.92 | 0.983 / 0.478 / 0.94 | 8.5% +-2.8 | 87% | 632 +-30 | 13.1 |
| 6 | 4,000 | 56,111 | 11,480 | 12,384 | 282 | 0.91 | 0.982 / 0.473 / 0.94 | 4.0% +-2.0 | 85% | 630 +-31 | 12.9 |
| 7 | 4,800 | 59,146 | 14,645 | 16,570 | 295 | 0.91 | 0.978 / 0.477 / 0.93 | 14.5% +-3.5 | 89% | 687 +-30 | 14.7 |
| 8 | 5,600 | 60,500 | 13,781 | 16,320 | 302 | 0.91 | 0.984 / 0.476 / 0.92 | 10.0% +-3.0 | 88% | 674 +-31 | 17.5 |
| 9 | 6,400 | 61,748 | 12,080 | 14,322 | 308 | 0.90 | 0.986 / 0.479 / 0.90 | 6.5% +-2.5 | 98% | 668 +-32 | 17.3 |

The heuristic is the anchor (1000); random rates 324 (5/100 against the heuristic). Games per
hour and leaves per second are the self-play phase's (iterations 2, 4 and 7 started with games
already in flight). A sample is 27 KB (float16), ~1.6 GB of shards per 800-game iteration.

Readout:

* **The network beats random from the first iteration and keeps improving against it**: 59%
  (iteration 0, the untrained net with search) -> 78% -> ... -> 88-89% -> 98% at iteration 9.
  The final Elo fit rises from 436 (iteration 0) to 668-687 (iterations 7-9), +230-250, all of
  it measured against random and earlier checkpoints.
* **Against the heuristic it is flat**: 4-14.5% per iteration (mean 9.3% over iterations 1-9, no
  trend over 6,400 games), against the heuristic's own 92-93% vs random. Not yet a success on that
  yardstick; an honest flat result.
* **Why, as far as the metrics show**: (1) the policy targets are nearly uniform (normalised
  entropy H(pi) / ln(options) 0.90-0.92 throughout; policy loss = entropy = 0.98, unchanged
  from iteration 1): at 32 simulations over ~3 options PUCT's visits stay at the prior, so the
  policy has nothing to learn from (the visit-target plateau zero-core docs/az.md 9 describes;
  Glass Road and Kingdom Builder moved to completed-Q targets for it). (2) The value head
  memorises games: on the iteration it was trained on, checkpoint 8 calls the winner 94% of
  the time (cross-entropy 0.35), on the next, unseen iteration 72% (0.60); checkpoint 3: 98%
  vs 67%. Every game has its own random decks and the bags fingerprint them (Kingdom Builder's
  random boards did the same). (3) Self-play games got longer (180 -> 308 searched decisions,
  35 -> 43 turns): the net passes more than the heuristic, which casts and attacks every turn.

### 21.7 For M5b and later

* **The network** (M5b, in zero-core): a network that reads the board card by card (a set /
  transformer encoder over objects: each permanent, hand card, stack object, list card and the
  context as a token with its card embedding and status), with a policy head that scores the
  action ids through the objects they refer to (slot -> token). The fixed-width layout above
  is what such a network replaces: 13,532 dense floats (27 KB per float16 sample, ~0.5 GB of
  shards per 400-game iteration) of which a few hundred are non-zero, slots cut at 32 / 16 / 8
  / 40, and per-slot one-hots that must be learned slot by slot. Dominion Zero's token encoder
  is the starting point (DESIGN.md 5). The protocol then needs variable-length (or padded)
  object lists in the leaf batches and the shards, and a sparse feed.
* **The next run** (measured in 21.6): completed-Q policy targets (`policy_target='cq'`, the
  visit targets stayed at H/ln n 0.90), more simulations per searched decision, and against
  the value memorisation `value_q` near 1 and fewer reuses (Kingdom Builder's fix: value_q 0.9,
  reuse 2), or more distinct decks per sample. The fingerprint is structural (every game has
  its own decks and the bags show them), so a card-by-card network will meet it too.
* **zero-core** (worked around here, for the core):
  * an evaluation spec (a run setting the loop uses for every evaluation match) would replace
    `mtgz.az.evaldecks`' patch of `Run.driver`;
  * evaluation and Elo matches dominate a CPU iteration (two network seats at 32 sims with 20-40
    games in flight batch badly on CPU): an evaluation cadence or cheaper Elo for CPU runs;
  * the pinned commit (zero-core `main` 7225a65) is not on GitHub yet: push it so worktrees can
    init the submodule without the local fetch (21.5).
* **Search cost**: ~180-550 decisions with two or more options per game, many of them priority
  passes and one-source-at-a-time mana payments; an engine-level shortcut (a mana payment as
  one decision among a few canonical tappings; skipping priority stops where only "pass" makes
  sense) would cut the simulations per game several fold. The encoding is ~40 us of the Rust
  side per simulation.
* **force_chance** falls back for looks at another player's hand and hand reveals (the forced
  identities would have to be dealt into the hand consistently with the clouds); these are
  rare and keyed, so they only cost search-tree sharing.

### 21.8 Tests

`engine/tests/az.rs` (`cargo test -p engine --test az`; `AZ_SCALE=k` plays k times as many
games): random and mixed heuristic / random games end with results summing to 1 (draws split,
the turn cap a draw), illegal actions are errors; clones continue identically; `determinize`
keeps the features of the viewer and the decider's legal actions, for the decider and the
other player (22k checks at `AZ_SCALE=15`, all identical); two worlds of the decider played
with the same actions have the same features while their keys, next decider and legal actions
agree (zero-core's child signature; 22k comparisons); `force_chance` reproduces a transition's
draws and looks in another world (86%) and keeps the world valid, an impossible key falls back
and the next queued key still lands on its own event; the encoding's hand, permanent and list
slots hold the cards behind the `HAND`, `MY_PERM` / `OPP_PERM` and `LIBRARY` ids and their
option flags match the legal actions; decks are legal, seeded and cover the colour pairs; the
evaluation set is fixed; the spec parser; the heuristic beats random (> 80% over 120 games)
and always answers a legal id (the bot's own tests: `tests/bots.rs`, section 15). `tests/` (pytest): the engine module registers `toy` and `mtg`,
the game through the bindings (determinize keeps the features), the bots in the Rust driver,
self-play records valid samples, the presets, the evaluation spec and the driver patch, the
commands through the shims (zero-core's loop at toy scale: resume, STOP, PAUSE, supervisor,
kill, fork, dashboard, h2h, targets), and one iteration of `mtg-smoke`. zero-core's cross-game
smoke runs all of it plus the 3-iteration `mtg-smoke` run: `SKIP_CORE=1 bash
third_party/zero-core/tools/smoke_all.sh ~/mtg-zero` (GREEN 2026-10-22: build 115 s, cargo test
in release 397 s, pytest 18 tests 65 s, smoke run 69 s).

## 22. A network that reads the board card by card, and the second run (M5b)

### 22.1 What changed

**zero-core** (`main` @ 71a1718, pushed; its docs/az.md 10 and docs/search.md 1, 3):

* arch `tokens` (`zero.net.TokenNet`): a transformer over the game's objects, read from the
  same fixed-width features through a layout descriptor the game package registers
  (`GamePackage.tokens`). The `SearchGame` trait, the driver, the leaf batches and the shards
  are unchanged, so the MLP and the token network train on the same data and can be compared
  one for one.
* `SearchGame::minor_decision` + run setting `sims_minor`: a self-play seat searches the game's
  minor decisions with that many simulations and never records them.
* run settings `eval_spec` (and `GamePackage.eval_specs`: a game's evaluation spec as a
  function of the self-play spec), `elo_opponents` / `elo_sims` / `elo_together` (cheaper Elo),
  `infer_workers` (CPU inference on several Python threads), `heads` / `ff`. Every default
  keeps the old behaviour: the core's tests (68) and Rust tests, the golden self-play digests,
  and `tools/check_checkpoint_compat.py` for mtg1's iteration-9 checkpoint (visits and cq
  records, results and a match against random identical on the old and the new checkout).

**MTG Zero** (branch `m5b-run2`):

* The runs of 22.4 used layout v1 (21.2) unchanged; layout v2 (22.2) came after them.
  `encode::layout_json` describes the encoding for the token network, checked in as
  `mtgz/az/layout_v4.json` (22.7, M8a; v3 until then, 22.6; v2 at commit 98b9463, v1: `layout_v1.json` at commit 531070a; `cargo test -p engine
  --test az layout_json_is_checked_in`; `MTGZ_WRITE_LAYOUT=1` rewrites it):

  | Segment | Slots x floats | Present flag | Identity | Actions scored from the slot's token |
  |---|---|---|---|---|
  | context | 1 x 384 (v2: 192, v3: 256) | always | the decision's card (v1: one-hot of 256; v2: card code) | |
  | tops | 6 x 50 | known to the viewer | card code | |
  | my permanents | 32 x 88 | present | card code | `MY_PERM` + s, `ACTIVATE` + 4 s + i (i = 0..3) |
  | opponent permanents | 32 x 88 | present | card code | `OPP_PERM` + s |
  | hand | 16 x 56 | present | card code | `HAND` + s, `HAND_ABILITY` + s |
  | stack | 8 x 72 | present | card code | `STACK` + s |
  | triggers | 4 x 52 | present | card code | `TRIGGER` + s |
  | list | 40 x 52 | present | card code | `LIBRARY` + s |
  | bags | 13 x 256 (v2: 13 x 1024) | | multisets of identities (pooled into the global token) | |

  The 128 globals are the global token's input; `PASS`, `YES`, `PLAYER`, `NUMBER`, `GRAVEYARD`
  and the slots beyond the encoded ones get their logits from the pooled state. The card
  identity is embedded once (256 + 1 rows, shared by every segment and the bags); a token is
  that embedding + a linear map of the slot's 50-88 floats (status, keywords, options) + a
  (segment, slot) embedding (v1: 256 + 1 identities; v2: 1024 + 1). MTG positions hold 21
  tokens on average (p99 39) of the 153 slots.
* Minor decisions: mana payments (`PayMana`, which source to tap next): 14% of the searched
  decisions of mtg1's games.
* The evaluation deck set goes through `GamePackage.eval_specs` (M5a's patch of `Run.driver` is
  gone; any spec's evaluation form, as before).
* Presets `mtg-2` (22.4) and `mtg-2-mlp` (the same with mtg-1's MLP: the control).
* The token network's policy starts uniform (zero-core d89202f: its policy output layers are
  zero at first; the value head keeps its random init), and mtg2 runs with seed 1. Three false
  starts (stopped, their data deleted; logs, metrics and checkpoints kept) showed why:
  `runs/mtg2a` (random output layers) began with 55% of the prior on `PASS` where 25% is
  uniform; its self-play games ran into the turn cap (58.8 turns of 60: draws, no signal) and its
  first checkpoint lost 100/100 to the heuristic. `runs/mtg2b` (uniform policy, seed 0) still
  favoured `PASS`: the cq targets of its first iteration put 49% on it against 36% uniform, its
  first checkpoint 47%, and it lost 99/100. `runs/mtg2n` (policy and value heads at zero,
  3be6d11) made every search a tie, and the search's tie rule (the first maximum) plays the first
  option, `PASS`: 3 self-play games finished in 15 minutes. Short self-play probes with
  untrained token networks (16 games, 32 sims, cq targets: the share of `PASS` in the targets at
  priority decisions that offer a hand card) show a coin flip of the random value head, not the
  architecture: seed 0 49% vs 36% uniform, seed 1 33% vs 40%; the
  MLP's seed 0 (the control) 25% vs 40%.

### 22.2 Stable card identities: layout v2 (23,324 floats)

Layout v1 coded a card by its index in the card table (`CardDb`: the supported cards in IR
order, then the tokens in the order lowering meets them), capacity 256: every card ported
renumbers the cards after it (a checkpoint then reads every card as another one), and the
cube's 546 cards do not fit. Layout v2 (`az/identity.rs`, `az/encode.rs`) codes the card's
**identity**, which does not depend on what the engine supports:

* a cube card: its position in the cube's IR card list (`data/cards/powerlsv.ir.json`, all 546
  cards of the PowerLSV cube, supported or not, sorted by name, basic lands included): 0..545;
  capacity 768;
* a token: 768 + its line in the token registry `engine/src/az/tokens.txt` (30 lines today: one
  key per token kind = name, power / toughness, colours, card types, subtypes, keywords, e.g.
  `Thopter 1/1 C ac Thopter flying`). New tokens are appended, never inserted: when ported cards
  create new tokens, `cargo test -p engine --test az token_registry` fails until
  `MTGZ_WRITE_TOKENS=1` appends their keys (two token kinds with the same key would share an
  identity; the test checks the keys are distinct);
* a legacy card (ENGINE.md 54: a card outside the cube that constructed decks play, in
  `data/cards/legacy.ir.json` and only in the card table of constructed play,
  `CardDb::constructed`): 546 + its line in the legacy registry `engine/src/az/legacy_cards.txt`
  (one card name per line: `Murktide Regent` 546, `Prismari Charm` 547), in the free range below
  `CUBE_CAP` (546..767: 222 identities), so layout v4 is unchanged. Append-only like the token
  registry, never sorted (the legacy IR is sorted by name, so its positions would move as cards are
  added): `cargo test -p engine --test az legacy_registry_and_identities` fails while a legacy card
  has no line, a line is a cube card or repeats; the cube's identities are the same in both card
  tables;
* 1023: anything else (a card outside the cube list, an unregistered token: never in a game
  the tests accept).

Capacity 1024 identities: the card code's identity part is three one-hots of 16, 8 and 8
(index = 64 a + 8 b + c; 32 floats, as v1's two one-hots of 16, so every slot keeps its width),
the bags are 13 x 1024 counts, and the context block carries the decision's card as a card
code (192 floats instead of 384 with a 256-wide one-hot). Everything else is v1's. A
checkpoint of layout v2 stays valid as cards are ported: a newly supported card appears under
the identity it always had, and its descriptor floats (types, colours, mana value, size) are
read the same way; its embedding (token network) or input weights (MLP) start untrained. A
change to the cube list itself (another IR card order) or to the capacity is a new layout.
Layout v1 checkpoints (runs/mtg1, mtg2, mtg2c) do not load into v2 (other widths): they run
from their pinned worktrees. Tests: `encoding_slots_name_the_action_slots` (the slots' codes
are the cards' identities), `token_registry_and_stable_identities` (every token registered,
cube cards at their IR position, identities distinct, and a card table of a few cards gives
them the same identities as the full one), `layout_json_is_checked_in`.

### 22.3 Against the memorised values (evidence first)

What fingerprints a game is not the decklists alone, and the decks cannot vary more than they
do (every self-play game has two new random decks). Measured offline on runs/mtg1's data: a fresh
256 x 3 MLP's value head trained on iterations 6-8 (175,757 samples), tested on iteration 9's
unseen games (61,748 samples), cross-entropy and accuracy against the game result z:

| Training | Train CE / acc. | Next-iteration CE / acc. |
|---|---:|---:|
| target z, 4 epochs (mtg1's reuse 4) | 0.123 / 97.0% | 1.385 / 65.5% |
| target z, 1 epoch | 0.255 / 87.9% | 0.910 / 65.3% |
| target 0.1 z + 0.9 search value, 4 epochs | 0.508 / 73.3% | **0.602 / 66.3%** |
| target z, 4 epochs, the decklist bags zeroed (my library, the opponent's hidden pool, known library cards) | 0.166 / 90.0% | 1.332 / 62.4% |
| 0.9 search value, 1 epoch, decklist bags zeroed | 0.542 / 70.7% | 0.629 / 63.0% |

* Without the decklist bags the network still memorises (90% on its training games): the
  whole position identifies a game, not just the decks. The bags help on unseen games (+3
  points), so they stay.
* A value target that varies within a game (`value_q` 0.9) removes the memorisation (train
  and next-iteration CE 0.51 / 0.60) and gives the best next-iteration CE and accuracy; fewer
  passes over the data help less. Hence `value_q` 0.9 (warm-up 2 iterations) and `reuse` 2;
  the decks stay as they are.

The same data through the token network (96 x 3, 600 steps at lr 1e-3 on iterations 7-8, the
loop's losses with value_q 0.9; tested on iteration 9): policy CE 1.005 (target entropy 0.934),
value CE vs z 0.585, accuracy 67.5%, against the MLP's 1.012 / 0.600 / 65.8% - it learns from
the same samples at least as well.

### 22.4 The second run (`runs/mtg2`) and its control (`runs/mtg2c`)

Preset `mtg-2` (`runs/mtg2`, seed 1, from the pinned worktree `~/mtg-zero-wt/run-531070a`, core
c0d1a3c, layout v1) and its control `mtg-2-mlp` (`runs/mtg2c`, seed 0, from `run-ca74b0f`, core
cc86dee: the MLP's code is the same in both cores). The same settings except the network:

* network: tokens 96 x 3 (4 heads, feed-forward 192; 0.76M parameters) vs the MLP 256 x 3 of
  mtg1 (4.19M); lr 1e-3, batch 512, `reuse` 2, window 6 (growing), value_q 0.9 (warm-up 2:
  0.45 at iteration 1);
* self-play: rolling, 768 games in flight, 600 finished games per iteration, 64 / 16 simulations
  (`p_full` 0.25; mtg1: 32 / 8), mana payments at 4 simulations and never recorded
  (`sims_minor`), completed-Q targets (c_visit 50, c_scale 1), temperature for the first 60
  searched decisions;
* CPU only: 6 search threads, 4 inference threads x 2 torch threads (`infer_workers`, 4
  groups). The GPU ran two Kingdom Builder runs (100% busy), whose self-play workers also took
  ~19 of the 32 logical CPUs throughout (measured: the machine at 31.8 / 32);
* evaluation as mtg1 (100 games against the heuristic, 50 against random, 32 simulations, the 16
  evaluation deck pairs), Elo matches against the 2 previous checkpoints in one driver (20
  games each). mtg2's Elo matches took 12 minutes for one 20-game match (two token networks on
  tiny batches), so after iteration 1 its config was changed to `elo_games` 0 (paused and resumed
  during iteration 2's evaluation): its Elo rests on the heuristic and random results and the
  1-vs-0 match.

Wall clock (2026-10-22): mtg2 11:32-15:07 (3 h 35 min, 5 iterations; stopped with a STOP file
and left stopped); mtg2c 08:34-11:38 (3 h 4 min, 10 iterations; stopped after iteration 10 to
give mtg2 the CPU, left stopped). The control ran next to the token network's false starts
(22.1: runs/mtg2a, mtg2b, mtg2n, 08:34-11:31, their data deleted); mtg2 had no other MTG run
beside it after 11:38. Both shared the machine with the two Kingdom Builder runs. Afterwards
only the last two iterations' shards were kept (mtg2 2.6 GB, mtg2c 2.8 GB on C:).

**mtg2** (token network):

| Iter | Games (cum.) | Samples | Leaves / s | Decisions / game | Minor / game | Turns | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100) | vs random (50) | Elo (final fit) | Minutes (sp / train / eval / elo) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|
| 0 | 0 |  |  |  |  |  |  |  | 4.0% +-2.0 | 54% | 334 +-53 | 0.0 / 0.0 / 10.1 / 0.0 |
| 1 | 600 | 31,458 | 5,380 | 258 | 81 | 37.3 | 0.73 | 0.940 / 0.626 / 0.77 | 6.0% +-2.4 | 84% | 561 +-48 | 20.4 / 1.4 / 12.7 / 11.7 |
| 2 | 1,200 | 43,124 | 4,588 | 341 | 59 | 41.9 | 0.63 | 0.899 / 0.675 / 0.79 | 3.0% +-1.7 | 66% | 403 +-56 | 22.4 / 2.2 / 9.2 / 0.0 |
| 3 | 1,800 | 40,131 | 4,291 | 300 | 62 | 40.3 | 0.74 | 0.898 / 0.664 / 0.73 | 3.5% +-1.8 | 75% | 464 +-54 | 34.0 / 1.6 / 10.5 / 0.0 |
| 4 | 2,400 | 48,805 | 3,284 | 362 | 37 | 43.5 | 0.72 | 0.901 / 0.664 / 0.72 | 2.0% +-1.4 | 79% | 471 +-54 | 27.6 / 2.2 / 8.9 / 0.0 |
| 5 | 3,000 | 50,435 | 3,810 | 372 | 31 | 43.6 | 0.68 | 0.897 / 0.668 / 0.69 | 5.5% +-2.3 | 81% | 527 +-52 | 26.9 / 2.7 / 10.3 / 0.0 |

**mtg2c** (the MLP control):

| Iter | Games (cum.) | Samples | Leaves / s | Decisions / game | Minor / game | Turns | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100) | vs random (50) | Elo (final fit) | Minutes (sp / train / eval / elo) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|
| 0 | 0 |  |  |  |  |  |  |  | 8.5% +-2.8 | 59% | 500 +-38 | 0.0 / 0.0 / 2.7 / 0.0 |
| 1 | 600 | 19,936 | 8,859 | 173 | 66 | 34.7 | 0.64 | 0.877 / 0.617 / 0.79 | 11.5% +-3.2 | 66% | 593 +-34 | 7.8 / 0.5 / 9.9 / 2.7 |
| 2 | 1,200 | 24,051 | 12,300 | 210 | 58 | 37.5 | 0.57 | 0.876 / 0.678 / 0.78 | 17.0% +-3.8 | 71% | 636 +-32 | 4.3 / 0.5 / 4.5 / 5.6 |
| 3 | 1,800 | 29,142 | 17,613 | 242 | 43 | 40.8 | 0.50 | 0.866 / 0.660 / 0.73 | 12.5% +-3.3 | 80% | 612 +-32 | 3.6 / 0.4 / 3.7 / 6.9 |
| 4 | 2,400 | 34,273 | 11,481 | 266 | 34 | 43.6 | 0.51 | 0.850 / 0.660 / 0.72 | 10.5% +-3.1 | 60% | 583 +-32 | 6.3 / 0.6 / 2.1 / 3.9 |
| 5 | 3,000 | 37,423 | 12,316 | 285 | 35 | 46.3 | 0.49 | 0.844 / 0.662 / 0.69 | 12.0% +-3.2 | 69% | 605 +-32 | 5.5 / 0.8 / 5.7 / 4.6 |
| 6 | 3,600 | 35,498 | 10,383 | 273 | 36 | 43.4 | 0.52 | 0.835 / 0.661 / 0.70 | 5.5% +-2.3 | 75% | 624 +-32 | 6.9 / 1.0 / 6.3 / 6.0 |
| 7 | 4,200 | 41,752 | 12,986 | 311 | 27 | 46.0 | 0.55 | 0.850 / 0.663 / 0.69 | 9.0% +-2.9 | 92% | 668 +-31 | 6.9 / 0.9 / 4.2 / 5.0 |
| 8 | 4,800 | 47,371 | 14,010 | 343 | 27 | 47.6 | 0.55 | 0.840 / 0.663 / 0.69 | 6.0% +-2.4 | 84% | 626 +-32 | 6.3 / 1.2 / 8.6 / 5.5 |
| 9 | 5,400 | 47,976 | 13,484 | 345 | 24 | 46.8 | 0.55 | 0.841 / 0.665 / 0.68 | 10.5% +-3.1 | 75% | 653 +-33 | 7.1 / 1.4 / 7.5 / 6.3 |
| 10 | 6,000 | 52,223 | 11,781 | 371 | 21 | 47.4 | 0.54 | 0.831 / 0.665 / 0.68 | 6.0% +-2.4 | 88% | 669 +-35 | 9.0 / 1.4 / 4.7 / 5.7 |

Minutes per phase: self-play / training / evaluation (vs the heuristic and random) / Elo matches. Leaves per second are the self-play phase's (the first iteration fills the 768 games in flight).

The heuristic is each run's anchor (1000); Elo fits are per run (random: 289 in mtg2's fit, 429
in mtg2c's), so compare Elo within a run and the runs through the head-to-head matches below.

**Head to head** (`python -m mtgz.az.h2h`, 32 simulations, chance_k 0, random decks, seats
alternating; score of the token network, +- 1 se):

| Token network | MLP | Games | Score of the token network | Elo difference |
|---|---|---:|---:|---:|
| iteration 3 (1,800 games) | iteration 3 (1,800 games) | 60 | 65.8% +-6.1 | +114 |
| iteration 5 (3,000 games) | iteration 5 (3,000 games) | 100 | 57.0% +-5.0 | +49 |
| iteration 5 (3,000 games) | iteration 10 (6,000 games) | 100 | 36.5% +-4.8 | -96 |

**Value generalisation** (`value_gen`: checkpoint k's value head on the samples of its own
iteration k and of the next iteration's unseen games; cross-entropy / accuracy against the
result z), against mtg1 (checkpoint 8: 94% / 72%, checkpoint 3: 98% / 67%):

| Checkpoint | mtg2 (tokens) own | mtg2 next | mtg2c (MLP) own | mtg2c next |
|---:|---|---|---|---|
| 1 | 0.396 / 87.3% | 0.592 / 68.3% | 0.345 / 93.3% | 0.626 / 65.4% |
| 2 | 0.592 / 74.9% | 0.655 / 61.4% | 0.591 / 74.7% | 0.646 / 62.8% |
| 3 | 0.601 / 71.6% | 0.629 / 66.0% | 0.581 / 72.2% | 0.640 / 62.8% |
| 4 | 0.612 / 67.8% | 0.649 / 61.3% | 0.591 / 69.9% | 0.661 / 60.2% |
| 5 | | | 0.618 / 68.6% | 0.642 / 62.7% |
| 9 | | | 0.618 / 66.3% | 0.638 / 62.6% |

(Checkpoint 1 still had value_q 0.45; from iteration 2 on value_q 0.9.)

**Both networks on the same held-out games** (the control's iteration 7, unseen by both;
`cross_eval`): token checkpoint 3: policy CE 1.011, value CE 0.621, accuracy 67.3%; MLP
checkpoint 3: 1.005 / 0.649 / 61.8%; MLP checkpoint 6: 0.947 / 0.641 / 65.3% (the targets'
entropy 0.588). Per decision kind the two checkpoint-3 policies are within 0.02 of each other
(priority 0.968 / 0.966, targets 1.089 / 1.095, attackers 0.827 / 0.831).

Readout:

* **Against the heuristic neither network improves** in these iterations: the MLP stays at
  5.5-17% (mean 10.1% over iterations 1-10, as mtg1's 9.3%), the token network at 2-6% (mean 4.0%
  over 1-5), losing faster (22-25 turns, margin -20: the heuristic wins at full life). Against
  random both win more (MLP 59% -> 75-92%, tokens 54% -> 75-84%).
* **Head to head the token network is the stronger player** at equal iterations (57-66% against the MLP of the same iteration; the MLP of twice the games, which the CPU gave it in less time, wins 63.5%), and
  its value head generalises at least as well (next-iteration accuracy 61-68% vs 60-65%; on the
  same unseen games 67% vs 62%). The two yardsticks disagree: the heuristic (aggressive, casts
  and attacks every turn) beats the token network more often than the MLP although the token
  network beats the MLP. Blocking is the likely gap: blocks are rare in self-play (130 of 20,000
  recorded decisions of mtg2's iteration 4) and the token network's prior on them is still
  uniform (47% on no block, 48% uniform), while the heuristic attacks every turn.
* **The memorised values are gone** with value_q 0.9 and reuse 2 in both runs: the gap between a
  checkpoint's own games and the next iteration's is 4-12 accuracy points (mtg1: 22-31).
* **The completed-Q targets are sharper** than mtg1's visit targets (H / ln n 0.49-0.74 vs 0.90),
  and the policy loss now falls below the target entropy (MLP 0.877 -> 0.831).
* **Cost**: the token network self-played at 3.3-5.4k leaves/s against the MLP's 8.9-17.6k on
  the same machine and settings (2-3x slower), its evaluation took 9-13 minutes per iteration
  against the MLP's 2-10, so it got 5 iterations (3,000 games) in 3 h 35 min where the MLP got
  10 (6,000 games) in 3 h 4 min. Mana payments (minor) were 21-81 of 173-372 searched decisions
  per game.
* **Self-play games get longer** in both runs (MLP 173 -> 371 decisions, 35 -> 47 turns; tokens
  258 -> 372, 37 -> 44): as in mtg1, the networks pass more than the heuristic.

### 22.5 Next

* **A yardstick that does not disagree with head to head**: the heuristic alone measures one
  style; add Elo matches against fixed earlier checkpoints of both runs (cheap now with
  `elo_together`), or a second, defensive bot.
* **Blocking**: blocks are rare in self-play and pointer logits for "block this attacker with
  the asked blocker" need the attacker's token to attend to the blocker's: give the
  `DeclareBlockers` / `DeclareAttackers` decisions their own context (the asked blocker's slot
  as a pointer query), and check per-kind policy and value calibration after each iteration.
* **Compute**: the token network is CPU-bound at ~4k leaves/s with the machine shared; the GPU
  (bfloat16, CUDA graphs as GridNet has) or a Rust / ONNX inference path would remove the
  per-op Python overhead. Evaluation should batch its matches (vs the yardstick, vs random and
  Elo in one driver) - 40% of mtg2's iteration time was evaluation on tiny batches.
* **Search cost**: per game 170-370 searched decisions remain; priority stops where only passing
  makes sense (always-available activations such as Sensei's Divining Top, Zuran Orb, Voltaic
  Key: 85% of the activation-only priority decisions of mtg1's data) are the next minor
  decisions to try.
* **Layout v2** (22.2) for every new run; mtg2 / mtg2c checkpoints are layout v1.

### 22.6 Layout v3: room for 64 decision kinds (M7b)

Layout v2's context block (192 floats) had one-hot room for exactly 32 decision kinds, 20 asking
tasks and 40 effect ops, and M7a used 32, 19 and 38. Layout v3 (23,388 floats) widens the context
block to 256: 64 decision kinds, 32 task codes, 64 op codes (`encode::ctx::{KINDS, TASKS, OPS}`;
`encode_context` asserts each code is in range instead of writing into the next field); the rest
of the block (decider, the card behind the decision, cost, counters, ...) keeps its order after
them, with 19 spare floats. Every segment after the context moves by 64 floats; the identities,
the bags and every other width are v2's (the `cards` list of `layout_v3.json` is v2's exactly).
Checkpoints of layout v2 (no run was made with M7a's cards) do not load into v3. v3 is not frozen
until the M7b card batch ends: while no run uses it, the batch may widen it further (the version
stays 3); it is frozen at the first run.

M7b part 1 widened it (still version 3) to 26,500 floats for planeswalkers (section 24.6): the
card code from 48 to 52 floats (planeswalker, printed loyalty / 8, emblem, one spare), so every
slot that holds a card code grows by 4 (tops 54, hand 60, stack 76, triggers and list 56); the
permanent status from 40 to 48 floats (88 to 100 a slot); two more bags (15: each player's
emblems); two globals per player in the spare floats. `mtgz/az/layout_v3.json` and the token
registry were regenerated on merge (`MTGZ_WRITE_LAYOUT=1` and `MTGZ_WRITE_TOKENS=1 cargo test -p
engine --test az ...`).

M7b part 2 (faces, section 25.8) adds fields only in spare floats (still 26,500): the card
code's spare float (a face other than the front is up), permanent status 45-46 (lore counters,
back face up), hand status 6-7 (which face the slot's `HAND` id casts or plays), task code 20
and four context floats (`ChooseFace` and its mask; 11 context floats stay spare), four
globals per player (poison, cards on an adventure, temporary triggered abilities, no maximum
hand size) and two game globals (day, night). The `cards` list gains the 16 new cards (faces
share their card's identity and are skipped); no new token kind. `layout_v3.json` was
regenerated on merge.

M7b part 3 (copies, section 26.9) widens it to 29,092 floats (still version 3): the permanent
status from 48 to 88 floats (100 to 140 a slot: a copy and the identity of what it copies,
what it is attached to, prepared; four spare), the stack status from 24 to 28 (76 to 80 a
slot: a copy of a spell, the times an additional cost was paid, bestowed; one spare); task
codes 21-23. A permanent's identity is what it is without copy effects, its descriptor the
copied face. `layout_v3.json` and the token registry (one new kind, `Insect 1/1 G c Insect`)
were regenerated on merge; a token copy takes the copied card's identity, so registry line 807
(Vaultborn Tyrant's token copy of itself) is unused from here on.

M7b part 3b (section 27.10) widens it to 29,860 floats (still version 3): the permanent status
from 88 to 96 floats (148 a slot: ward and its costs, lost all abilities, goaded, exiled if it
would leave), the stack status from 28 to 32 (84 a slot: targeted, can't be countered, the
ability's index, a graveyard card's ability), a new segment `graveyard_abilities` (4 x 56, at the
end, scoring the `GY_ABILITY` ids), the initiative, dungeon and experience globals in spare
floats, the context's ability-slot kind from 4 to 5 floats (the fields after it move by one).
The descriptor of a copy shows its exceptions. `layout_v3.json` and the token registry (four new
keys: `Undercity rules`, `The initiative rules`, `Elemental 1/1 R c Elemental`, `Warrior 1/1 R c
Warrior`) were regenerated on merge.

**Frozen** at the end of the M7b card batch (2026-10-23, the merge of `m7b-stack`): 29,860
floats, 9 segments, 15 bags, 1024 identities. At every step of the batch no card identity moved
(newly supported cards and new registry keys filled empty identities). From here on a change to
any width, offset, segment or identity is layout v4; newly supported cards and appended token
keys keep v3 (22.2).

### 22.7 Layout v4 (M8a)

Started by M8a (section 35.6): 29,972 floats, version 4, `mtgz/az/layout_v4.json` (renamed from
`layout_v3.json`). v3's blocks keep their offsets and widths; two segments are appended
(`my_companion`, `opponent_companion`: 56 floats each, the card code plus revealed, action
available, action legal now, in the hand; `my_companion` scores `act::COMPANION`), and v4's
globals use v3's spare floats (global 124: the viewer chose who plays first; per player 38-39:
sideboard cards still outside the game, a companion whose action is available). Two decision
kinds (44 of 64) and two task codes (24, 25). v4 stays open until phase 2 of M8 ends (the
best-of-three match adds its own fields); it is frozen at the first run of layout v4.
Checkpoints of layout v3 do not load into v4.

M8b part a (section 36.7): 33,652 floats. The list block grows in place to 64 slots of 60 floats
(every `LIBRARY` id; sideboarding's pool list), two bags join the bag block (17: the opponent's
pool, the cards they revealed in earlier games), and the match block (32 globals) is appended; the
blocks after the list and the bags move. Two decision kinds (46 of 64), one task code (26,
sideboarding).

**Frozen** at the end of M8b (2026-10-26, the merge of `m8b-hidden`): 33,652 floats, 11
segments, 17 bags, 1024 identities; part b changed no width (its hidden-pool model fills the
same bags from what the viewer knows). The first run of layout v4 is `runs/mtg4` (preset
`mtg-4`, best-of-three matches). From here a change to a width, offset, segment or identity is
layout v5; newly appended token keys keep v4.

### 22.8 The third and fourth runs (`runs/mtg3`, `runs/mtg4`)

**Settings.** Preset `mtg-3` (`runs/mtg3`, seed 1) is the first run on the GPU: tokens 128 x 4
(4 heads, feed-forward 256), inference and training on CUDA (one inference worker, 2 groups, 8
search threads), 768 games in flight, 600 finished games per iteration, 64 / 16 simulations
(`p_full` 0.25), mana payments at 4 simulations (`sims_minor`), completed-Q targets, value_q 0.9
(warm-up 2), lr 1e-3, batch 512, reuse 2, window 6 (growing), a 12 GB window budget; best-of-one
games on random decks of the supported cards. Evaluation: 100 games against the heuristic (M7c's,
which activates abilities, 21.4; about +92 Elo against the bot of mtg1 and mtg2) and 50 against
random, 32 simulations, the 16 evaluation deck pairs; Elo matches against the 2 previous
checkpoints in one driver (`elo_together`, 20 games each). Preset `mtg-4` (`runs/mtg4`, seed 1)
is `mtg-3` with best-of-three matches (section 36: sideboarding, seeded 45-card pools through the
deckbuilder, the hidden-pool model) as the episode, layout v4 and `max_steps` 60,000; evaluation
and Elo play matches.

**mtg3's engine changed under it** (pinned worktrees, each time resumed from a checkpoint):

| From | Engine | Cards | Why |
|---|---|---:|---|
| iteration 0 (10-24 11:33) | run-32e5225 | 467 | launch |
| iteration 2's evaluation (14:15) | run-4613d0f | 491 | evaluation games looped on activations (iteration 0's evaluation took 85 minutes) |
| iteration 3 (18:03) | run-0577e06 | 524 | a self-play panic, Malcolm's free cast of a flashback card (crash loop 14:36-17:04; section 15) |
| iteration 15 (10-25 02:11) | run-7d2ad02 | 546 | a search thread panicked at 20:53 ("no legal target": targets checked with the card in hand, 601.2a / 601.2c) and the process hung on zero-core's poisoned lock until found |

It finished its 40 iterations (24,002 games) at 07:38 on 10-25: about 11 hours of running in 20
of wall clock, about 12 minutes per iteration (self-play 3, training 0.4, evaluation 3-5, Elo
4-7).

**mtg3** (best-of-one games; selected iterations, every one in `runs/mtg3/metrics.jsonl`):

| Iter | Games (cum.) | Samples | Leaves / s | Decisions / game | Minor / game | Turns | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100) | vs random (50) | Elo (final fit) | Minutes (sp / train / eval / elo) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|
| 0 | 0 |  |  |  |  |  |  |  | 9.0% +-2.9 | 49% | 564 +-68 | 0.0 / 0.0 / 85.4 / 0.0 |
| 1 | 600 | 19,022 | 12,241 | 143 | 33 | 22.2 | 0.76 | 1.061 / 0.627 / 0.76 | 16.0% +-3.7 | 50% | 555 +-65 | 6.3 / 0.3 / 4.0 / 6.1 |
| 2 | 1,200 | 25,315 | 12,317 | 187 | 15 | 25.5 | 0.71 | 1.044 / 0.679 / 0.85 | 11.0% +-3.1 | 74% | 669 +-58 | 3.9 / 0.3 / 4.1 / 14.1 |
| 3 | 1,800 | 13,909 | 12,917 | 106 | 22 | 18.5 | 0.79 | 1.021 / 0.667 / 0.87 | 28.0% +-4.5 | 72% | 754 +-56 | 4.0 / 0.2 / 3.8 / 6.7 |
| 4 | 2,400 | 17,932 | 12,744 | 133 | 15 | 20.6 | 0.76 | 1.006 / 0.664 / 0.88 | 13.0% +-3.4 | 78% | 713 +-56 | 2.8 / 0.2 / 3.0 / 3.6 |
| 5 | 3,000 | 18,730 | 12,734 | 138 | 14 | 20.2 | 0.73 | 0.981 / 0.665 / 0.88 | 22.0% +-4.1 | 74% | 776 +-54 | 2.9 / 0.2 / 4.1 / 7.1 |
| 10 | 6,002 | 18,811 | 12,733 | 141 | 16 | 19.3 | 0.67 | 0.911 / 0.658 / 0.84 | 29.0% +-4.5 | 88% | 861 +-52 | 3.0 / 0.4 / 3.6 / 4.1 |
| 15 | 9,002 | 13,086 | 12,466 | 98 | 18 | 15.7 | 0.57 | 0.839 / 0.643 / 0.84 | 38.0% +-4.9 | 90% | 868 +-52 | 3.9 / 0.4 / 3.8 / 5.3 |
| 20 | 12,002 | 20,148 | 12,388 | 146 | 12 | 18.1 | 0.57 | 0.809 / 0.626 / 0.85 | 38.0% +-4.9 | 96% | 907 +-51 | 3.0 / 0.4 / 3.2 / 5.5 |
| 25 | 15,002 | 19,889 | 12,368 | 145 | 12 | 17.7 | 0.52 | 0.786 / 0.613 / 0.84 | 52.0% +-5.0 | 100% | 1018 +-51 | 2.9 / 0.4 / 3.1 / 5.9 |
| 30 | 18,002 | 19,762 | 12,493 | 143 | 11 | 17.3 | 0.50 | 0.754 / 0.605 / 0.84 | 41.0% +-4.9 | 94% | 979 +-51 | 2.8 / 0.4 / 4.0 / 6.0 |
| 35 | 21,002 | 18,692 | 12,472 | 137 | 12 | 16.9 | 0.47 | 0.727 / 0.589 / 0.84 | 62.0% +-4.9 | 98% | 1058 +-52 | 2.9 / 0.4 / 2.9 / 7.3 |
| 40 | 24,002 | 18,910 | 12,392 | 139 | 11 | 16.5 | 0.44 | 0.691 / 0.569 / 0.83 | 52.0% +-5.0 | 100% | 1018 +-57 | 2.7 / 0.4 / 4.3 / 4.7 |

Means per ten iterations, against the heuristic / random: 1-10 22.6% / 78.0%, 11-20 33.4% /
92.4%, 21-30 44.8% / 95.8%, 31-40 51.3% / 97.2%. Random vs the heuristic: 6 / 100. Elo
(heuristic 1000, +- 1.96 se; win rates +- 1 se): random 484 in mtg3's fit, the best checkpoint
iteration 36 (1070).

**mtg4** (best-of-three matches; launched 10-26 01:59 from run-f3637c4; stalled in iteration 3
on a hidden-pool sampler panic, fixed in 4fa92e4 (36.11) and resumed from iteration 2 at 05:14;
stopped after iteration 6 and resumed at 09:09 on run-2f3c1c0, zero-core c1facf0 (a panicking
game fails the run) with `MTGZ_SUPERVISE_STALL=3600`):

| Iter | Matches (cum.) | Samples | Leaves / s | Decisions / match | Minor / match | Turns / match | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100 matches) | vs random (50 matches) | Elo (final fit) | Minutes (sp / train / eval / elo) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|
| 0 | 0 |  |  |  |  |  |  |  | 1.0% +-1.0 | 28% | 217 +-102 | 0.0 / 0.0 / 7.8 / 0.0 |
| 1 | 600 | 47,721 | 10,996 | 349 | 61 | 44.7 | 0.65 | 1.031 / 0.570 / 0.91 | 2.0% +-1.4 | 10% | 11 +-109 | 15.6 / 0.5 / 7.3 / 15.5 |
| 2 | 1,200 | 66,966 | 10,844 | 479 | 23 | 58.7 | 0.63 | 1.015 / 0.669 / 0.91 | 3.0% +-1.7 | 10% | 64 +-100 | 15.2 / 0.9 / 9.4 / 25.1 |
| 3 | 1,800 | 57,342 | 10,829 | 395 | 29 | 47.1 | 0.70 | 1.028 / 0.657 / 0.89 | 2.0% +-1.4 | 23% | 278 +-88 | 20.4 / 0.8 / 10.5 / 25.9 |
| 4 | 2,400 | 79,053 | 10,691 | 546 | 23 | 58.6 | 0.68 | 1.013 / 0.658 / 0.89 | 4.0% +-2.0 | 50% | 362 +-83 | 14.5 / 1.2 / 11.0 / 28.3 |
| 5 | 3,000 | 82,413 | 10,858 | 572 | 21 | 60.2 | 0.69 | 0.989 / 0.663 / 0.84 | 2.0% +-1.4 | 55% | 414 +-79 | 13.5 / 1.3 / 9.5 / 37.7 |
| 6 | 3,600 | 79,331 | 9,418 | 559 | 26 | 59.7 | 0.71 | 0.981 / 0.664 / 0.81 | 4.0% +-2.0 | 70% | 443 +-78 | 14.8 / 1.4 / 17.5 / 26.6 |
| 7 | 4,200 | 55,286 | 10,781 | 388 | 32 | 47.1 | 0.73 | 0.975 / 0.665 / 0.80 | 3.0% +-1.7 | 54% | 481 +-76 | 19.8 / 1.3 / 11.8 / 30.5 |
| 8 | 4,800 | 74,578 | 10,971 | 520 | 24 | 56.1 | 0.72 | 0.968 / 0.666 / 0.81 | 4.0% +-2.0 | 66% | 452 +-76 | 11.6 / 2.0 / 8.5 / 19.8 |
| 9 | 5,400 | 71,295 | 11,145 | 501 | 22 | 56.5 | 0.72 | 0.964 / 0.669 / 0.79 | 7.0% +-2.5 | 64% | 587 +-75 | 12.2 / 1.4 / 10.2 / 23.1 |
| 10 | 6,000 | 72,302 | 10,979 | 507 | 23 | 55.6 | 0.72 | 0.961 / 0.669 / 0.80 | 8.0% +-2.7 | 81% | 584 +-78 | 11.9 / 2.2 / 18.7 / 24.3 |

(Fit after iteration 10, 10-26 12:39; random 390 in mtg4's fit; random vs the heuristic 1 / 100
matches. Resumed at 12:40 with `eval_every` 2, below.)

**mtg4, iterations 11-40.** From iteration 11 `eval_every` 2: the yardsticks and Elo on even
iterations only. Re-pinned twice, each time stopped after an iteration and resumed from its
checkpoint with the same settings: iteration 13 onwards (10-26 15:34) on run-48ccbb9 (M8d part
2's 7 engine fixes, 38), iteration 21 onwards (19:33) on run-739abd8 (part 3's 10, 39). It
finished its 40 iterations at 05:08 on 10-27, 27 hours after launch, but it collapsed at
iteration 30 (below). Evaluated iterations:

| Iter | Matches (cum.) | Samples | Leaves / s | Decisions / match | Minor / match | Turns / match | Target H/ln n | Policy / value loss, value acc. | vs heuristic (100 matches) | vs random (50 matches) | Elo (final fit) | Minutes (sp / train / eval / elo) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|
| 12 | 7,200 | 69,829 | 8,413 | 485 | 21 | 54.1 | 0.71 | 0.959 / 0.671 / 0.83 | 8.0% +-2.7 | 61% | 552 +-63 | 15.6 / 1.5 / 9.1 / 22.7 |
| 14 | 8,400 | 63,905 | 10,603 | 446 | 22 | 51.8 | 0.71 | 0.956 / 0.672 / 0.84 | 15.0% +-3.6 | 70% | 664 +-60 | 10.6 / 1.5 / 10.8 / 25.2 |
| 16 | 9,600 | 63,781 | 10,861 | 447 | 20 | 50.2 | 0.70 | 0.947 / 0.669 / 0.86 | 14.0% +-3.5 | 86% | 687 +-59 | 10.2 / 1.4 / 9.0 / 16.8 |
| 18 | 10,800 | 55,581 | 7,053 | 391 | 22 | 46.5 | 0.70 | 0.934 / 0.667 / 0.87 | 17.0% +-3.8 | 91% | 736 +-57 | 14.6 / 1.3 / 10.0 / 27.5 |
| 20 | 12,000 | 53,596 | 6,806 | 377 | 21 | 45.8 | 0.70 | 0.920 / 0.664 / 0.88 | 12.0% +-3.2 | 88% | 713 +-57 | 13.8 / 1.2 / 9.3 / 12.3 |
| 22 | 13,200 | 51,339 | 11,202 | 361 | 18 | 44.1 | 0.69 | 0.904 / 0.660 / 0.89 | 23.0% +-4.2 | 88% | 757 +-56 | 8.2 / 1.6 / 8.1 / 16.6 |
| 24 | 14,400 | 50,346 | 11,174 | 356 | 21 | 45.1 | 0.66 | 0.891 / 0.657 / 0.88 | 16.0% +-3.7 | 92% | 790 +-55 | 8.4 / 1.1 / 9.0 / 10.6 |
| 26 | 15,600 | 46,355 | 11,169 | 330 | 21 | 42.6 | 0.66 | 0.875 / 0.653 / 0.87 | 28.0% +-4.5 | 86% | 775 +-58 | 8.2 / 1.5 / 8.3 / 11.5 |
| 28 | 16,801 | 50,396 | 11,189 | 358 | 22 | 43.2 | 0.65 | 0.861 / 0.651 / 0.86 | 29.0% +-4.5 | 98% | 857 +-60 | 8.4 / 1.2 / 9.0 / 11.9 |
| 30 | 18,001 | 53,535 | 6,763 | 380 | 23 | 44.8 | 0.66 | NaN / NaN / 0.53 | 2.0% +-1.4 | 4% | -31 +-122 | 15.8 / 1.2 / 5.1 / 19.0 |
| 32 | 19,201 | 130,369 | 12,439 | 872 | 0 | 173.6 | 0.05 | NaN / NaN / 0.67 | 2.0% +-1.4 | 0% | -76 +-117 | 17.1 / 3.1 / 6.9 / 18.5 |
| 34 | 20,401 | 117,369 | 11,845 | 788 | 0 | 180.0 | 0.00 | NaN / NaN / 0.82 | 2.0% +-1.4 | 0% | -81 +-112 | 17.9 / 2.8 / 6.0 / 23.0 |
| 36 | 21,601 | 116,983 | 12,021 | 777 | 0 | 180.0 | 0.00 | NaN / NaN / 0.92 | 1.0% +-1.0 | 4% | -90 +-113 | 17.3 / 2.6 / 6.6 / 23.1 |
| 38 | 22,801 | 115,850 | 12,018 | 774 | 0 | 180.0 | 0.00 | NaN / NaN / 1.00 | 1.0% +-1.0 | 0% | -108 +-121 | 18.1 / 2.7 / 5.4 / 24.7 |
| 40 | 24,001 | 111,117 | 11,939 | 739 | 0 | 180.0 | 0.00 | NaN / NaN / 1.00 | 0.0% +-0.0 | 3% | -111 +-132 | 18.0 / 2.5 / 5.0 / 21.3 |

(Fit after iteration 40; random 431 in it. The rows of iterations 0-10 above keep the fit after
iteration 10. Iteration 11 played no yardsticks and spent 100 minutes on the one-time Elo
catch-up. Iteration 30's self-play is still checkpoint 29's (clean) and its training is the step
that went NaN; from iteration 31 on every number comes from a NaN network.)

Readout:

* **mtg3 is the first run that improves against the heuristic**: 9% at iteration 0 to 51% over
  iterations 31-40 (even with M7c's heuristic; Elo 962-1070), and 97% against random. The
  policy loss falls throughout (1.06 -> 0.69) with sharper targets (H / ln n 0.76 -> 0.44); the
  gain per ten iterations shrinks (+11, +11, +7 points) but hasn't stopped. Games get shorter
  (22 -> 16.5 turns at ~140 searched decisions).
* **What changed from mtg2** (flat at 2-6%): the GPU (12.4k leaves/s against 3-5k, so 40
  iterations instead of 5), a larger network (128 x 4 against 96 x 3), a pool with real threats
  (467-546 cards against 191), a stronger yardstick. The runs weren't built as an ablation, so
  which of these mattered isn't measured.
* **mtg4 after 10 iterations is at 1-8% against the heuristic in matches.** If the games of a
  match were independent, a match win rate m = p^2 (3 - 2p) for a game win rate p, so 4-8% of
  matches is p ~ 12-17% per game: where mtg3 was in iterations 0-2, not 3-9 (13-29%). The two
  aren't directly comparable: mtg4's decks come from pools through the deckbuilder on both sides,
  the network sideboards (the heuristic keeps its deck), its layout is v4 and its network fresh.
* **Against random mtg4 first got worse** (28% at iteration 0, 10% at iterations 1-2) and then
  better (50-81% from iteration 4). Unmeasured guess: early sideboarding from a near-uniform prior
  (the 60-card deck of 34 basic lands that stalled iteration 3 came from it, 36.11).
* **A match costs about 3.6 of mtg3's games** in searched decisions (~500 and ~56 turns per match
  against ~140 and ~17 per game); 600 matches take 12-20 minutes at ~10.9k leaves/s, each
  iteration ~3.8x mtg3's samples. **Evaluation took 70% of mtg4's iterations** (heuristic 3-9
  minutes, random 7-10, Elo 15-38 for its 40 matches on small batches), so from iteration 11 it
  runs with `eval_every` 2 (the user's decision, 10-26; resumed 12:40): the yardsticks and Elo
  every second iteration, Elo against checkpoints k-2 and k-4. Iteration 11's Elo phase first
  plays the pairs this spacing lacks (4-0, 6-2, 8-4, 10-6), once.
* **mtg4 kept improving until it collapsed.** Against the heuristic, the evaluated iterations
  average 13.2% of matches over 12-20 and 24.0% over 22-28, peaking at 29% at iteration 28 (the
  best checkpoint: Elo 857 in the final fit, random 431); against random 79% and 91% (98% at
  28). By the formula above, 24-29% of matches is p ~ 32-36% per game, close to mtg3's 33% over
  its iterations 11-20 (same caveats). The policy loss fell throughout (0.962 at 11 to 0.861 at
  29), the targets sharpened (H/ln n 0.73 -> 0.66) and matches got shorter (54 -> 43 turns, 485
  -> 358 searched decisions).
* **Cost with `eval_every` 2**: from iteration 21 a pair of iterations took about 40 minutes (the
  odd one 9-14: self-play 8, training 1-2; the even one 29-35, of which yardsticks 8-9 and Elo
  11-17), so evaluation still took about half. Self-play fell to 6.3-8.4k leaves/s (from ~11k) at
  iterations 12 and 18-20 while the re-pins' search smokes ran beside it.
* **mtg4 went NaN at iteration 30** (10-27): one training step returned NaN losses (bfloat16
  autocast on CUDA, grad clip 5; the trigger is unknown, that iteration's shard had been rotated
  out), `iter_0030.pt` was saved with all 93 float tensors NaN (`iter_0029.pt` clean, max |w|
  1.23) and the run went on for 10 iterations of turn-capped games, NaN targets and drawn Elo
  matches: from iteration 31 self-play matches grew to 133.9 and then 180 turns (the cap, from
  iteration 33), with no minor decisions and the target entropy at 0, the samples per iteration
  doubled (~115k: ~46 GB of shards for iterations 35-40) and the value-accuracy column climbed to
  1.00 on NaN outputs (meaningless). The watchdog saw only a clean exit. From zero-core 681ba8d (pinned in 83cc116) the non-finite guard (zero-core
  docs/az.md 4.1) fails the run instead: a NaN / inf in a shard's x / pi / z / q / sc after the
  float16 cast (written: quarantined to `<run>/debug/*.bad.npz`; loaded: the window refuses the
  shard), in a training step's losses or gradient norm (before `optimizer.step()`; the batch and
  its shard rows go to `<run>/debug/nonfinite_iter_NNNN_step_SSSS.pt`) or in a checkpoint is a
  `NonFinite` crash (exit 1, `run aborted: NonFinite: ...` in log.txt); the supervisor restarts
  it like any crash, so a deterministic cause repeats once per replayed iteration. Checked on
  `mtg-smoke` (an injected 1e6 feature, a NaN value logit: both fail with the message; no
  injection: the iteration completes, CPU and CUDA training). Cost on a real shard (65,536 rows x
  33,652 features, 4.4 GB): about 0.7 s per write and per load.

## 23. Threats first: layers, counters, generic triggers, other zones (M7a)

M7a grew the supported cube from 191 to 360 cards, creatures from 20 to 113 (threats, i.e.
creatures, Vehicles, Equipment, manlands, token makers and burn, from 37 to 139). The work
went feature by feature in the order the threat view of the support report (23.9) gave:
threats unlocked per unit of work. Every card's IR was read against its Oracle text; a
mismatch is a blocker, not a guess (23.10).

### 23.1 Continuous effects and layers (CR 611, 613; `layers.rs`)

Characteristics are read through accessors (`types_of`, `has_subtype`, `colors_of`,
`keywords_of`, `protection_of`, `flags_of`, `pt`) that apply, in layer order: printed values
(or a characteristic-defining ability, 7a: Tarmogoyf-style counts, "power equal to the
number of creatures you control", domain), then type-changing (4), colour (5), ability (6:
keywords, protection, rules flags such as "can't block", "can't be blocked", "attacks each
combat if able"), base power/toughness setting in timestamp order (7b), then pumps, counters
and per-count bonuses summed (7c). Two sources feed them:

* **Static abilities** (`StaticDef { scope, cond, mods }` on the card): `This`, `Equipped`
  (301.5) or `Matching { filter, other }` (types, subtypes, colours, controller, token-ness
  relative to the source's controller), with an optional condition ("as long as you have one
  or fewer cards in hand"). Re-evaluated continuously (611.3a). A fast path skips everything
  when no static or effect exists.
* **Effects from resolved spells and abilities** (`Game.effects`: `Effect { ts, until, on,
  m, controller }`, `until` = end of turn or forever). Effects on objects are locked in at
  resolution (611.2c: `Affected::Obj(ObjRef)`, ending when the object changes zones); rules
  effects on a group ("creatures target player controls can't block this turn") apply to
  whatever matches while they last (`Affected::Group`).

**Counters** (CR 122) live in `Game.counters` per object: +1/+1, -1/-1 (they annihilate,
704.5q), keyword counters (flying, hexproof, lifelink, indestructible, ...), stun (122.1d:
removed instead of untapping) and named counters (charge, ...). They leave with the object.

### 23.2 Generic triggers (CR 603; `triggers.rs`)

An `Event { kind, a, b, amount }` names its subjects (`Slot`: obj, attacker, blocker, source,
target, spell, controller, player, card). Kinds: enters, dies, leaves the battlefield,
attacks, attacks alone, you attack, blocks, deals combat damage (to a player), is dealt
damage, is cast, draws, discards, becomes tapped, is sacrificed, becomes monstrous, the
beginnings of upkeep, draw step, precombat main, combat and end step. A trigger's
predicate is a tree (`TrigPred`: `IsSelf(slot)`, `Subject(slot, filter)`, `Controlled(slot,
you)`, `FormerHost` for "equipped creature dies", `HostAttacksWithAnother`, `Source(filter)`,
`If(cond)` = an intervening "if" checked again on resolution (603.4), `State(cond)` = part of
the event ("your second spell each turn"), and/or/not). Leave-the-battlefield triggers look
back (603.10a): the events are recorded before the object moves, and a simultaneous move
(SBA deaths, "destroy all", sacrificed costs) shows each leaver the others
(`begin_batch`/`end_batch`). A triggered ability remembers its event (`EvCtx`: "that
player", "that creature", "that much") through resolution. Keyword triggers are synthesized
at lowering: exalted, battle cry, prowess, persist.

**Delayed triggered abilities** (603.7; `Game.delayed`): an effect creates "at the beginning
of the next end step / next upkeep / your next main phase" abilities about an object or a
number (Flickerwisp's return, Sneak Attack's sacrifice, dash, Shallow Grave, Mana Drain's
mana, the Baubles' draw). **Reflexive** triggers (603.12: "you may remove a counter. When you
do, ...") trigger at once and choose their targets as they go on the stack.

### 23.3 Combat keywords and restrictions

Protection (702.16: damage prevented, not blocked by, not targeted by, not enchanted or
equipped by; from colours, card types, everything, each opponent, the chosen player),
"can't block", "can't be blocked", "can't be blocked except by two or more" generalized to
n blockers, "can block only creatures with flying", "can't be blocked by <filter>", "attacks
each combat if able" (508.1d: declared before the free choices), exalted and battle cry.
Tokens can enter tapped and attacking (508.4: not declared, so no attack triggers; only for
the attacking player during combat). "Combat damage that would be dealt by creatures you
control can't be prevented" overrides protection's prevention.

### 23.4 Costs

* **Alternative costs** (118.9): `AltCost { mana, cost, turn, only_if, evoke, dash }` - Force
  of Will and the other Forces, Daze (return an Island), Snuff Out, Fury and Solitude
  (evoke, 702.74: the permanent knows; its "when evoked" sacrifice), dash (702.109: haste and
  a delayed return), "if this is the first spell you've cast this game". `ChooseAltCost`
  (`NUMBER` 0 = the mana cost, k + 1 = alternative k) asks only when more than one is payable.
  Cost increases apply to them (601.2f).
* **Cost changes** (601.2f): increases ("noncreature spells cost {1} more"), then reductions
  (per basic land type, per controlled permanent), generic only.
* **Additional costs and kicker** (702.33): a kicked spell's effect and targets follow the
  option like a mode (the two branches of "if this spell was kicked" are the modes; no mode
  decision).
* **X** (107.3): announced at 601.2b (`NUMBER`, from 0 to what the caster can pay, 0 when
  cast without paying its mana cost), paid as generic mana, read by `Amount::X`, counted in
  the mana value on the stack (202.3e), "enters with X +1/+1 counters".
* **Delve** (702.66): graveyard cards exiled while paying, one decision per card in
  increasing slot order, never leaving the rest unpayable (nor taking what escape exiles).
* **Buyback** (702.27): an optional additional cost; the spell goes to hand as it resolves.
  **Overload** (702.96): the alternative cost picks the "each" effect, a mode given by the
  cost like kicker. **Escalate** (702.120): each mode beyond the first needs a card to
  discard (and the mana payable while keeping it); the discards follow the mana.
* **Other cost parts**: exile a card of a kind from your hand, return permanents to hand,
  exile n cards from your graveyard (a set: increasing positions), remove counters from the
  source (by kind or name), crew n (702.122: untapped creatures other than the Vehicle, one
  per decision in slot order, `PASS` once their power is enough), divided damage announced
  with the targets (601.2d, `DivideDamage`).

### 23.5 Casting and playing from other zones (601.2a)

`cast_route` says how a card may be cast from where it is: from hand; from its owner's
graveyard with **flashback** (702.34; printed, or granted for its mana cost by Snapcaster
Mage or Flashback; the card is exiled whenever it would leave the stack) or **escape**
(702.138; the permanent knows it escaped; Underworld Breach gives every nonland card escape
for its mana cost and three other cards); from exile or a graveyard with a **permission**
(`Game.permissions`: Golos, Ragavan, Robber of the Rich, Expressive Iteration, Emry; this turn or
while exiled, free, "mana as though of any colour", "during any turn you attacked with a
Rogue"). Lands may be played from exile with a permission or from the graveyard ("you may
play lands from your graveyard"); additional land drops (305.2) count. Priority ids: a
graveyard card is its `GRAVEYARD` slot; an exiled card with a permission is `EXILE` + k, the
k-th such card in exile order (980-1011, section 11). A card cast this way is its own source
for protection and never its own graveyard target. "Without paying its mana cost" is an
alternative cost; flashback, escape and permissions don't combine with another one (118.9a).

### 23.5a The monarch, level up, token copies

* **The monarch** (724): `Game.monarch`; becoming it, "until an opponent becomes the monarch"
  exile (Palace Jailer), "if you're the monarch". The end-step draw and "whenever a creature deals
  combat damage to the monarch, its controller becomes the monarch" are the monarch's inherent
  triggered abilities (724.2, since M7e): sourceless, controlled by the player who is the monarch
  as they trigger, on the stack (Tishana's Tidebinder can counter them): the triggers of a rules
  definition "The monarch" (as the initiative's, 27.4; `dungeon.rs::monarch_trigger`), one per
  creature that dealt the damage.
* **Level up** (711): level counters pick the base P/T (`PtCda::Leveler`) and the statics
  (`Cond::SourceCountersIn`, read against the static's own source).
* **Token copies**: of a token (its definition, 707.2); of the card itself ("except it's an
  artifact", 707.9b): a placeholder token index the lowering fills with the card's own
  definition once the card is lowered (Vaultborn Tyrant). M7b part 3 replaced both with the
  general token copy (section 26.2): a copy effect with the exception, read as the card last
  existed.
* **Counters as they last existed** (608.2h): `Game.lki_counters` keeps a leaving
  permanent's counters for the rest of the turn ("if there are no charge counters on it").

### 23.6 Linked and delayed exile (607.2a, 610.3)

`Game.links`: cards exiled with a permanent. "Until this leaves the battlefield" (Portable
Hole, Leyline Binding, Deep-Cavern Bat, Tidehollow Sculler) returns them when it leaves
(610.3a; exiles nothing if it already left, 610.3c). Untimed links serve "the exiled card"
later: Skyclave Apparition's token, Ghost Vacuum's return. Flickerwisp exiles and creates a
delayed return about the exiled card (a new object, 400.7).

### 23.7 New hidden-information cases (section 10)

* **Random order** ("on the bottom in a random order": Torsten, Endurance, Thassa's Oracle,
  Atraxa): nobody knows the order. The owner forgets the cards (an owner's knowledge is
  exact or nothing); what another player knew of them becomes a cloud over that library
  (the owner's determinize keeps library cards in the library, so it stays true); a cloud
  naming one of those slots is widened to the library.
* **Looking at another player's library card that its owner doesn't know** (Mishra's
  Bauble): the look is keyed for the looker but not remembered; the owner's determinize may
  change that card, and the looker's clouds could not stay consistent with it. A random hand
  card seen (Urza's Bauble) is remembered (its owner knows it). Cards cast from exile or a
  graveyard are public.
* A coin flip (Mana Crypt) is a public chance key (`KEY_COIN`) and can be forced.
* Which of two identical hand cards another player "knows" is canonicalized after every
  action (a card leaving the hand is not told apart from a known twin).

### 23.8 Effects added (the `Ext` instructions)

`Op::Ext(Ext)` holds the M7a instructions (`resolve_ext.rs`), so `az/encode.rs`'s
exhaustive `op_code` needs a single arm. Among them: apply mods / rules effects, protection
of a chosen colour, counters (put, double, "the second time this resolved this turn"),
tap / untap / tap-or-untap, attach (equip), fight, mill, explore, choose permanents without
targeting (Chaos Defiler, "untap up to three lands"), flicker, coin flips, graveyard returns
(Reanimate, Exhume's each-player choice, Unearth), divided damage, the evoke sacrifice,
linked exile, delayed and reflexive triggers, amass, monstrosity, living weapon / For
Mirrodin!, damage dealt by the event's object (Pyrogoyf) with its last known power,
"can't gain life for the rest of the game", energy, will of the council, devotion, winning
the game (104.2b), the Oath, Upheaval, Flash, Grindstone, Atraxa's one card per type (a
bipartite matching of chosen cards to card types).

### 23.9 The threat view of the support report

`cargo test -p engine --test support threat_report -- --nocapture` classifies every cube
card as a threat (creature; Vehicle; Equipment; manland; token maker; burn) or not, and ranks
features by the threats they block alone and in total, with a greedy staging over threats.
At the end of M7a: threats 139 supported (113 creatures), 107 blocked (96 creatures).

### 23.10 Blocked by the IR, not by the engine

* Celestial Colonnade and Creeping Tar Pit: `BecomeLandCreatureUEOT` lacks the animated
  creature's colours ("a 4/4 white and blue Elemental"); a coloured land without
  `set_colors` is a blocker rather than a colourless creature.
* Strip Mine has no "{T}: Add {C}" (section 20).
* Robber of the Rich's permission keeps the default duration ("this turn") in the IR; the
  Oracle text ("during any turn you attacked with a Rogue") makes it last while the card is
  exiled, as Python does; the lowering follows the Oracle text.
* All three fixed by mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (section 32): the manlands' colours and Elemental, Strip
  Mine's `produces: [C]`, Robber's `while_exiled` (now read: a `this_turn` duration with the
  Rogue requirement is a blocker).

### 23.11 Simplifications

What still applies (docs/shortcuts.md tracks them all; M7e fixed a Pyrogoyf-like damage source's
last known information, an attached Equipment's timestamp, Mox Diamond however it enters, a
granted escape with a card's own additional costs and the monarch's triggered abilities):

* Untap-up-to effects offer only tapped permanents (choosing an untapped one does nothing).
* Combat damage to divide among blockers above 31 offers the first 30 amounts, lethal and
  all (the `NUMBER` range).
* A graveyard card's castability is checked on a clone with the card on the stack (leaving
  the graveyard can change a goyf's power and so what is targetable).
* "True-Name Nemesis": choosing yourself is offered (a `Target` decision with `PLAYER` ids;
  dominated, but legal).
* The tap-or-untap choice, a counter kind to remove, an energy amount and X use `ChooseMode`
  with `NUMBER` ids; DecisionKind stayed at 32 kinds (the context block of layout v2; v3 has 64), so "top
  or bottom" is a `YesNo` (`YES` = top).

## 24. Planeswalkers (M7b part 1)

M7b part 1 grew the supported cube from 360 to 378 cards: 18 of the cube's 20 planeswalkers
and the rules they need, which also reach cards already supported (damage to "any target",
"creature or planeswalker" removal, "combat damage to a player"). Saheeli, Sublime Artificer
needed copy effects (`Effect:TargetBecomesCopyOfAnotherTargetUEOT`; supported in M7b part 3,
section 26); Minsc & Boo, Timeless Heroes is blocked by its IR (24.8; supported since M7d's IR
fixes, 32). Every card's IR was read against its Oracle text.

### 24.1 Loyalty and loyalty abilities (CR 306, 606)

* `CardDef.loyalty` is the printed loyalty (306.5a); a planeswalker enters with that many
  loyalty counters (`Counter::Loyalty`, 306.5b), and its loyalty is their number (306.5c). The
  lowering takes `CardPrinting.loyalty` only for a planeswalker; a planeswalker without one,
  or a loyalty on another card, is a blocker.
* Every printed activated ability of a planeswalker on the battlefield is a loyalty ability
  (`ActivatedDef.loyalty = Some(change)`, 606.2). The IR's convention: `ActivationCost.loyalty`
  is the loyalty removed (+1 is written -1), and a [0] cost is written as no loyalty cost at
  all; the lowering flips the sign, requires `sorcery_speed` (606.3) and refuses
  `is_mana_ability` (605.1a: a loyalty ability is never a mana ability; Chandra's "+1: Add
  {R}{R}" and Ugin's "0: Add {C}{C}{C}" use the stack) and `loyalty_x` (no cube card has X).
* Activation (606.3): the controller's main phase, an empty stack, and no loyalty ability of
  that permanent activated this turn (`Obj.pw_turn` / `pw_used`; a permanent that left and
  came back is a new object). The Wandering Emperor: "as long as she entered this turn, any
  time you could cast an instant" (`Obj.entered`). Comet: "you may activate Comet's loyalty
  ability two more times this turn" (`pw_extra`).
* The cost (606.4) puts on or removes the loyalty counters as the ability is activated; a
  negative cost needs that many counters (606.6).
* Damage to a planeswalker removes that many loyalty counters (120.3c, 306.8); a planeswalker
  that is also a creature (Kaito during his controller's turn) also has the damage marked
  (120.3e). A planeswalker with loyalty 0 is put into its owner's graveyard (704.5i, 306.9).
  Damage is never redirected from a player to a planeswalker (306.7: the old rule is gone).
* The legend rule (704.5j) applies to them like any legendary permanent.

### 24.2 Attacking planeswalkers (CR 506.3, 508.1b, 510.1b)

* `Obj.attacks: Option<ObjRef>`: `None` = the defending player, `Some(r)` = that planeswalker.
  `Game::attack_dest(c)` is what the creature attacks now: the player; the planeswalker while
  it is on the battlefield, still a planeswalker and still controlled by the defending player;
  else nothing, the planeswalker having been removed from combat (506.4c: the creature is still
  attacking, may be blocked, and deals no combat damage if unblocked).
* Declaring (508.1b): right after each attacker is chosen, if the defending player controls a
  planeswalker, an `AttackTarget` decision for that creature; otherwise no decision (games
  without planeswalkers are unchanged). Creatures that attack each combat if able (508.1d) get
  theirs first. Creatures put onto the battlefield attacking (508.4: Adeline's tokens, "create
  ... tapped and attacking", the creature card Raph & Mikey reveal) are asked too; ninjutsu's
  creature attacks what the returned creature attacked (702.49c).
* Combat damage (510.1b): an unblocked creature deals its damage to what it attacks; trample's
  excess goes there too (702.19b); with nothing to attack, everything goes to the blockers and
  none to the player (702.19f; no cube card has trample over planeswalkers).
* Triggers: "deals combat damage to a player" fires for players only (510.3a: Ragavan; combat
  damage to the monarch); "to a player or planeswalker" (Psychic Frog) fires for both
  (`DealsCombatDamage` events carry the planeswalker as their second subject); "whenever ...
  attacks" fires whatever it attacks. Okoye's "whenever that creature attacks the monarch this
  turn" (a delayed triggered ability bound to the creature since M7e, `TrigPred::AttacksMonarch`)
  fires only for a creature attacking the player.

### 24.3 Emblems and the command zone (CR 114, 408)

* `Game.command` (`Zone::Command`) holds emblems. An emblem is an object whose definition
  (`CardDef.emblem`, named "<card> emblem", stored with the tokens) has only abilities (114.3:
  no types, colours or mana value); it is owned and controlled by the player who got it
  (114.2) and never leaves. `Ext::CreateEmblem` makes one; the lowering builds the definition
  from `CreateEmblemWithAbilities` (triggered and static abilities, lowered like a card's).
* Its abilities function in the command zone (114.4, 113.6p): triggers observe events from
  there and statics apply from there. The cube's: Chandra's (whenever you cast a spell, 5
  damage to any target), Dack's (whenever you cast a spell that targets permanents, gain
  control of them), Kaito's (Ninjas you control get +1/+1), Nissa's (lands you control have
  indestructible), Teferi, Hero of Dominaria's (whenever you draw a card, exile target
  permanent an opponent controls), Tezzeret's (at the beginning of combat on your turn, three
  +1/+1 counters on target artifact you control; a noncreature one becomes a 0/0 Robot
  artifact creature), Wrenn and Six's (instant and sorcery cards in your graveyard have
  retrace).
* Emblems are public: `view_of` and the state fingerprint include them, determinize leaves
  them alone, the encoding has a bag of them per player (24.6).

### 24.4 Other rules added

* **"Loses all abilities"** (613.1f, Oko's +1): `Mod::LoseAllAbilities`; the object loses its
  printed abilities and those granted before it (613.7), keeps later grants. Every reader of a
  permanent's rules flags, triggered, activated, static and mana abilities goes through
  `Game::abilities(c)`, which returns an empty definition for such a permanent (a fast path,
  `any_loss`, skips the check when no effect removes abilities). **"Becomes a green Elk
  creature with base power and toughness 3/3"**: `Mod::BecomeTypes` sets the card types and
  creature types (205.1a-b: a Food stops being an artifact), with a colour and base P/T.
* **A permanent's own type-changing static** (Kaito: "during your turn, as long as Kaito has
  one or more loyalty counters on him, he's a 3/4 Ninja creature and has hexproof"):
  `Scope::This` with `Cond::All[YourTurn, SourceCountersIn(Loyalty, 1..)]`, read by
  `types_of` and `has_subtype` without recursion.
* **Control** (613.1b): gaining control for good (Dack's -2), exchanging control (Oko's -5;
  701.12a-b: nothing happens unless both are still there and controlled by different players),
  gaining control of the permanents a spell targets (Dack's emblem: noted as the trigger goes
  on the stack, so a countered spell doesn't stop it). A permanent that changes control leaves
  combat (506.4) and is summoning sick (302.6).
* **Replacement effects of static abilities** (614.1a): Elspeth's "twice that many of those
  tokens" (`token_factor`, applied by `create_tokens` to every token effect; two Elspeths make
  four times as many), Jace's "if you would draw a card while your library has no cards in it,
  you win the game instead" (104.2b).
* **Rules changes of static abilities**: Narset's "each opponent can't draw more than one card
  each turn" (a later draw does nothing), Teferi, Time Raveler's "each opponent can cast spells
  only any time they could cast a sorcery" (`cast_timing_ok`) and his "until your next turn, you
  may cast sorcery spells as though they had flash" (`Player.flash_sorceries`, ends as that
  player's next turn begins, 611.2b; `Until::YourNextTurn` for effects, Elspeth's flying),
  Tamiyo's "spells and abilities your opponents control can't cause you to discard cards or
  sacrifice permanents" (discard and sacrifice instructions controlled by an opponent skip that
  player; costs are paid by their own controller and are not affected).
* **Nissa's "whenever you tap a Forest for mana, add an additional {G}"** (a triggered mana
  ability, 605.1b, which resolves at once): the Forest's mana ability yields one more {G}.
* **Die rolls** (706, Comet): `Ext::RollDie` with a public chance key `KEY_DIE` (payload = the
  result); `force_chance` can choose it.
* **Casting during resolution** (608.2g; Chandra's +1, Ugin's -11 through a permission): the
  player casts the exiled card while the effect resolves, ignoring its card type's timing
  (not Teferi's lock: the stack isn't empty), through the normal casting process (a one-time
  permission, then `begin_cast`); the rest of the effect waits for the cast.
* **"Search for any number of cards"** (Nissa's -8, Ugin's -11): one definition per decision
  in non-decreasing order (each set of cards once), `PASS` = done.
* **Naming a card** (Tamiyo's +1): `NameCard`, 24.5.
* **Ninjutsu** (702.49, Kaito): a hand ability whose cost includes returning an unblocked
  attacking creature you control to its owner's hand (`Filter::Unblocked`, true once blockers
  are declared, 509.1h); the card enters tapped and attacking.
* **Retrace** (702.81, Wrenn and Six's emblem): an instant or sorcery card cast from its
  owner's graveyard by discarding a land card in addition to its other costs
  (`CastFrom::Retrace`, `GRAVEYARD` ids).
* **"When you cast this spell"** (113.6, Ugin): while a permanent spell is being cast, only
  its "this spell" triggered abilities function; "whenever you cast a colorless spell"
  functions on the battlefield only. (M7a's engine triggered both from the stack; no supported
  card had both kinds before Ugin.)
* Also: "put into its owner's library third from the top" (Teferi, Hero of Dominaria), "exile
  all cards from target player's library, then that player shuffles their hand into their
  library" (Jace, the Mind Sculptor), a revealed pair of which an opponent chooses one and the
  other is exiled with a silver counter, and later "a card you own with a silver counter on it
  from exile" (Karn), "draw a card for each opponent who lost life this turn"
  (`Player.lost_life`), "deals damage equal to the number of loyalty counters on him to a
  creature or player" (chosen as it resolves, not targeted: 115.10), "sacrifice a creature;
  when you do" (a reflexive trigger, Minsc & Boo), "at the beginning of the next end step,
  untap up to two lands".

### 24.5 Decisions

Three new kinds (35 in all; the context block holds 64):

* `AttackTarget`: what the creature just declared as an attacker (or just put onto the
  battlefield attacking) attacks: `PLAYER` + 1 = the defending player, `OPP_PERM` = one of
  their planeswalkers. Only when they control one.
* `ChooseExiled`: a card in exile an effect picks, `EXILE` + k = the k-th offered card in
  exile order (Karn's -1).
* `NameCard`: Tamiyo's "choose a nonland card name". `LIBRARY` = a nonland definition in the
  namer's library (sorted by definition, `Choice::Def`), `PASS` = any other name. Only cards of
  that library can be revealed, so every other name has the same result; open decklists make
  the library's contents (not their order) known to its owner.

Loyalty abilities are `ACTIVATE` ids like other abilities. Comet's "to a creature or player"
is a `Target` decision (not targeting); an opponent's choice between Karn's revealed cards is a
`PutInHand` of that opponent; Jace's "you may put that card on the bottom" is a `YesNo`; the
searches for any number of cards are `SearchLibrary`; "untap up to two lands" is
`ChoosePermanent`; the heuristic bot (`az/bots.rs`) attacks a planeswalker its attacker's power
kills, else the player, counts only creatures attacking it when it blocks, and since M7c uses
its own planeswalkers' loyalty abilities every turn they are available (the ultimate when it
can be paid, a minus that removes a worthwhile opposing permanent, else a plus; 21.4).

### 24.6 Encoding (layout v3, 26,500 floats)

* Card code (52): + planeswalker, printed loyalty / 8, emblem (one spare float).
* Permanent status (48): + loyalty / 8, a loyalty ability may still be activated this turn,
  attacking a planeswalker and that planeswalker's slot ((slot + 1) / 32), creatures attacking
  this planeswalker / 4, the attacker the pending `AttackTarget` decision is about.
* Bags (15): + `MY_EMBLEMS`, `OPP_EMBLEMS` (by the identity of the card that made the emblem:
  an emblem definition shares its card's identity, `identity.rs`; the token registry and the
  layout's card list skip emblems).
* Globals, per player: lost life this turn, may cast sorceries as though they had flash.
* Context: task code 19 = `AttackTarget` (its card = the attacker); `NameCard`'s list block is
  the namer's library sorted by definition (as for `SearchLibrary`).
* One new token kind to register (`tokens.txt`): `Squirrel 1/1 G c Squirrel` (Comet). The
  other tokens these cards make already have lines: Soldier (Elspeth), Construct 0/0 (Karn;
  the key has no static abilities), Food (Oko), Samurai (The Wandering Emperor). Boo's line
  (`Boo 1/1 C c Hamster trample haste`, colourless as the IR has it) will need a red one once
  the IR is fixed.

### 24.7 Simplifications

* `AttackTarget` is asked right after each attacker is chosen, not after the whole set
  (508.1a-b): no attack restriction or requirement in the cube depends on what a creature
  attacks, so the legal sets are the same.
* Naming a card offers the names of the namer's library cards and one "other name" (above).
* Nissa's extra mana is part of the Forest's mana ability, not a separate triggered mana
  ability (payments never float mana, so nothing can be done between the two).
* A token creation makes at most 64 tokens after doubling.
* (Retrace with a card's own additional costs is offered since M7e: one cost of both,
  `CardDb::merged_cost`; docs/shortcuts.md.)
* Until M7c the heuristic bot activated no abilities at priority (it played lands and cast
  spells), so it never used a loyalty ability and its planeswalkers only drew attacks; since
  M7c it uses them (21.4).

### 24.8 Blocked by the IR

* Minsc & Boo, Timeless Heroes: the IR's Boo token printing has no colour indicator (the
  Oracle text makes Boo red), so the token would be colourless. `lower::IR_DEFECTS` lists it;
  the card's tests run on a corrected copy of its IR built in the test
  (`cards_m7b_walkers2.rs::minsc_db`), which exercises everything else of the card (Boo's
  creation on entering and each upkeep, the +1, the -2's reflexive damage and the Hamster
  draw). Fixed by mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (Boo is red): the tests now play the cube card (section 32).
* Not a defect but a convention the lowering relies on: loyalty costs are written as the
  loyalty removed, and [0] as no cost (24.1).

### 24.9 The any-target audit

Every supported card whose Oracle text says "any target", "planeswalker", "permanent" or "a
creature or player" (64 cards) was checked against its IR's filters: every filter already
named planeswalkers where the Oracle text does, so the defects were in the engine, all fixed:
damage to a planeswalker did nothing (`deal_damage` ignored non-creatures), and "deals combat
damage to a player or planeswalker" (Psychic Frog) never saw a planeswalker. The cards: damage
to any target or to creatures and planeswalkers (Burst Lightning, Chandra's emblem, Fury,
Galvanic Discharge, Goblin Bombardment, Kolaghan's Command, Legion Extruder, Lightning Bolt,
Mine Collapse, Minsc & Boo, Orcish Bowmasters, Phlage, Pyrite Spellbomb, Pyrogoyf, Screaming
Nemesis, Sear, Traumatic Critique, Unholy Heat, Walking Ballista, Wrenn and Six); destroy,
exile, flicker, tuck or bounce a creature or planeswalker or a permanent (Abrupt Decay, Alpha
Deathclaw, Baleful Mastery, Bitter Triumph, Bone Shards, Boomerang Basics, Chaos Defiler,
Council's Judgment, Displacer Kitten, Erode, Flickerwisp, Get Lost, Leyline Binding, Phelia,
Portable Hole, Pyroblast, Skyclave Apparition, Tear Asunder, Teferi, Hero of Dominaria,
Unexpectedly Absent, Upheaval, Ugin, Vindicate, Woodfall Primus); edicts (Archon of Cruelty,
Sheoldred's Edict); combat (Adeline, Psychic Frog, Questing Beast); rules that name
planeswalkers (Clarion Conqueror, Mox Amber, Subtlety, Dack Fayden's emblem, Tamiyo's shield;
Comet's "a creature or player", no planeswalker, as printed); and cards where "permanent"
raises no planeswalker question (Atraxa, Baloth Prime, Cryogen Relic, Fatal Push, Formidable
Speaker, Malevolent Rumble, Skateboard, Super Shredder, Thassa's Oracle). "Each creature"
effects (Fiery Confluence's mode) spare planeswalkers; "each opponent" damage never reaches
them. `cards_m7b_walkers_audit.rs` tests one card of each kind.

### 24.10 Hidden information and fuzzing

* The die roll is a public chance key; Karn's and Tamiyo's reveals are public; Jace's look at
  the top of a library is keyed for the looker and remembered only where its owner knows the
  card too (23.7); the cards a search for any number of cards takes are private until
  revealed; Jace, the Mind Sculptor's -12 exiles the library face up, then shuffles the hand
  in with Timetwister's instruction (what the other player knew of that hand becomes "in that
  library", section 10); Narset's rest goes to the bottom in a random order (23.7).
* The fuzzer found two faults on the way, both fixed: a check-only one (`view_of` placed a
  link or permission whose card had moved on, `testkit::live_place`), and a retrace cast in
  progress whose land card the other player's determinize resampled away (the cost then had
  nothing to discard): `exile_filters_in_progress` now keeps retrace's land card in that hand,
  like the other hand cards a cost in progress may still take.

## 25. Cards with more than one face (M7b part 2)

M7b part 2 grew the supported cube from 378 to 394 cards: transforming double-faced cards,
modal double-faced cards (MDFCs), Adventures, Sagas, day and night, and a split card. The 16:
Brazen Borrower // Petty Theft; Cecil, Dark Knight; Concealing Curtains; Embereth Shieldbreaker
// Battle Display; Etali, Primal Conqueror; Jace, Vryn's Prodigy; Jennifer Walters // The
Sensational She-Hulk; Life // Death; Norman Osborn // Green Goblin; Outland Liberator;
Shatterskull Smashing; Sink into Stupor; Tamiyo, Inquisitive Student; Ulvenwald Oddity; Urza's
Saga; Witch Enchanter. Every card's IR, each face, was read against its Oracle text. Four cards
with faces stay blocked (25.11).

Rule numbers are those of the Comprehensive Rules effective 2026-09-25: transform is 701.27
(701.28 is convert), day and night 731. Older documents (and the M7b plan) say 701.28 and 726
for the same rules; tests and comments cite the current numbers.

### 25.1 A face is a definition

* Each face (a double-faced card's back face, an Adventure, a half of a split card) is lowered
  like a printing into a `CardDef` of its own: `lower_card` returns the card and its faces,
  and `CardDb::from_cards` stores the faces after the tokens. A face knows its card
  (`CardDef.front`); the card names its faces (`back`, `modal` for an MDFC, `adventure`,
  `halves`); a transforming card's back face has the front as its `back`, so transforming is
  the same both ways. `CardDb::is_face`, `CardDef::card_def`, `CardDef::other_face`.
* An object's `def` is the face that is up. Outside the battlefield and the stack a card has
  only its front face's characteristics (712.8a, 715.4, 709.4): `move_card` turns it back as it
  goes to any other zone (what left keeps its face as last known information). A spell that
  becomes a permanent keeps the face it had on the stack (712.13). A card put onto the
  battlefield from elsewhere comes front face up (712.14) unless the effect says "transformed"
  (712.14a; `Game.entering_face`), it is daybound and it is night (702.145b), or it is an MDFC's
  land face being played (712.12).
* A transforming card's back face has no mana cost; its mana value is the front face's mana
  value (712.8e; `CardDef.front_mv`). Its colours come from its colour indicator
  (`face_colors`, required on such a face). An MDFC's back face has a mana cost of its own
  (712.8f). A split card has its halves combined everywhere but the stack (709.4: the lowering
  checks that the IR's card-level cost and types are the sum and the union of the halves').
* A face has its card's identity (`identity.rs`): the token registry and the layout's card
  list skip faces, and the card code's descriptor shows the face that is up (25.8). Hidden-zone
  keys and knowledge use the card, not the face.
* **The back face's mana cost** (docs/ir_defects.md 9). The IR repeats the front face's mana
  cost on a transforming card's back face (with an empty `mana_cost_str`). The lowering reads
  exactly that shape, or an empty cost, as "no mana cost" and records the front's mana value
  in `front_mv`; any other back-face cost is a blocker
  (`CardPrinting.transform_back_face.mana_cost(not the front face's)`). No card can cast a
  transforming card's back face, so the mana cost itself is the only difference; the mana
  value is the same either way.

### 25.2 Transforming (CR 701.27, 712.18)

* `Game::transform` turns the other face up. The permanent stays the same object (712.18):
  counters, damage, attachments, tapped and combat status, summoning sickness and every effect
  on it remain. Nothing happens to a token or a single-faced permanent (701.27c), when the
  other face is an instant or sorcery face (701.27d, 712.10), or to a daybound or nightbound
  permanent except through day and night (702.145b, e). A permanent that transforms into a
  planeswalker gets no loyalty counters (306.5b is about entering), so with no other loyalty it
  is put into the graveyard by 704.5i (a custom test card; no cube card does it).
* 701.27f: an activated or triggered ability of a permanent transforms it only if it hasn't
  transformed since the ability was put on the stack. `Obj.flips` counts its transformations;
  `StackItem.flips` and `Frame.flips` keep the count from when the ability was put on the
  stack (`Ext::TransformSource`).
* "Exile it, then return it to the battlefield transformed under its owner's control"
  (Jace, Vryn's Prodigy; Tamiyo, Inquisitive Student; Ajani: `Ext::ExileReturnTransformed`): a new object with its back face up (712.14a); a
  planeswalker back face enters with its printed loyalty. A token, or a card that isn't
  double-faced, stays in exile.
* "When this creature transforms into Revealing Eye" (701.27e): a `Transforms` event.

### 25.3 Casting and playing a face (CR 709.3, 712.11-12, 715.3)

* At priority, a card whose other face can be cast or played (an MDFC's back face, an
  Adventure, a split card's right half) gets one option on its `HAND`, `GRAVEYARD` or `EXILE`
  id: `Cast` / `Play` for its front face, `CastFace` / `PlayFace` for the other face, or, when
  both faces can be used, `Faces(card, mask)` followed by a `ChooseFace` decision (`NUMBER`
  0 = the front face, 1 = the other). The mask bits (`FACE_CAST`, `FACE_PLAY`, `OTHER_CAST`,
  `OTHER_PLAY`) say what each face does; the task keeps them, so a refreshed decision asks the
  same question. A split card is always cast as one of its halves (709.3: `Cast` = the left
  half).
* Only the face to be cast is evaluated (712.11c, 715.3a, 709.3a): its timing, cost,
  additional costs and targets come from that face (`face_castable` evaluates them on a copy
  of the game with that face up). How the card may be cast from where it is comes from the card
  as it is there (its front: Wrenn and Six's retrace for "instant and sorcery cards" doesn't
  reach an adventurer creature card; a fuzz find, 25.9).
* Playing an MDFC's land face is a land play like any other: it uses the land drop, enters
  with that face up (712.12), and its "as this enters" choice (pay 3 life or enter tapped)
  is the face's.
* An Adventure that resolves is exiled instead of going to the graveyard (715.3d), with a
  permission (`PermKind::Adventure`, no time limit) to cast the card from exile, but not as an
  Adventure. A countered Adventure, or one whose targets are all illegal (608.2b: it doesn't
  resolve), goes to its owner's graveyard. A flashback Adventure would be exiled by flashback's
  rule without the permission (no card does it).
* Two spell locks now check every face: Archon of Emeria's "one spell each turn" and Jennifer
  Walters' "your opponents can't cast spells during your turn" (`may_cast_spells`); a locked
  player keeps the land faces.

### 25.4 Sagas (CR 714)

* `CardDef.chapters` is the final chapter number (714.2d); chapter abilities are triggered
  abilities on a `Chapter` event (714.2b: one event per chapter number the lore counters
  reach, so two counters at once trigger two chapters, in order).
* A Saga enters with a lore counter (714.3a, a replacement: its chapter I triggers); as its
  controller's precombat main phase begins, they put a lore counter on each Saga they control
  (714.3c, a turn-based action before the "beginning of precombat main phase" triggers).
* 704.5s: a Saga with at least its final chapter number of lore counters is sacrificed once
  no chapter ability of it is waiting to be put on the stack or on the stack. It is a
  sacrifice ("whenever you sacrifice" sees it), and a Saga that lost all its abilities isn't
  sacrificed.
* Urza's Saga's chapters I and II make it gain activated abilities ("{T}: Add {C}", "{2},
  {T}: Create a 0/0 Construct ..."): the lowering stores them as *latent* activated abilities
  of the card (`CardDef.latent`, the last ones in `activated`), and the chapter abilities
  create an effect (`Mod::GainLatent(k)`) that switches one on while the Saga stays. Chapter
  III's "an artifact card with mana cost {0} or {1}" is `Filter::ManaCostIn`.

### 25.5 Day and night (CR 731, 702.145)

* `Game.day`: `None` (the game starts with neither), `Some(true)` day, `Some(false)` night.
* When a daybound permanent appears and it is neither, it becomes day (702.145d); a nightbound
  one, with no daybound one, makes it night (702.145g). As it becomes night, daybound
  permanents transform; as it becomes day, nightbound ones do (702.145c, f; at once, not a
  state-based action). A daybound card put onto the battlefield at night enters transformed
  (702.145b).
* As the untap step's second part (731.2), if it is day and the previous turn's active player
  cast no spells that turn, it becomes night; if it is night and they cast two or more, it
  becomes day.
* Daybound and nightbound permanents transform only through day and night (702.145b, e; the
  custom Test Werewolf has a transform ability that does nothing).

### 25.6 Other rules and effects added

* **"Whenever this deals damage"** (Cecil): a `DealsDamage` event per damage dealt by an
  effect, and one per source for all the combat damage it deals in a step (510.2: dealt at
  once). Cecil's "then if your life total is less than or equal to half your starting life
  total" is `Cond::LifeAtMostHalfStart`.
* **Poison** (122.1f, 704.5c): `Player.poison`; ten or more lose the game. Etali, Primal
  Sickness: "they get that many poison counters" (`Ext::Poison`).
* **"You may ... Do this only once each turn"** (603.2h, The Sensational She-Hulk):
  `Ext::OnceMay` asks like `Op::May`; once the player does it, the ability doesn't trigger
  again that turn (`Game.done_once`). A declined trigger doesn't count.
* **"Until your next turn, whenever ..."** (Tamiyo, Seasoned Scholar's +2): a temporary
  triggered ability of its controller (`Game.temp_triggers`, `TempTrigger`), ending as their
  next turn begins (611.2b). Its definition is among the card's delayed abilities.
* **Casting a graveyard card this turn, exiled instead of going to a graveyard** (Jace,
  Telepath Unbound's -3): `PermKind::CastThenExile` and `Game.gy_exile` (the spell, wherever it
  would go to a graveyard from the stack, goes to exile; a countered spell returned to hand
  isn't exiled).
* **Etali's "each player exiles cards from the top of their library until they exile a nonland
  card; you may cast any number of spells from among the nonland cards exiled this way without
  paying their mana costs"**: the exiles first (each card revealed), then a `ChooseExiled`
  decision per spell (`PASS` = no more), so they are cast in any order, each at once while the
  rest of the effect waits (608.2g, as Chandra's +1, 24.4).
* **Connive** (701.50, Norman Osborn), **mayhem** (702.187b, Green Goblin's "each nonland card
  in your graveyard has mayhem"; `CastFrom::Mayhem`, `Game.discarded` holds the graveyard
  objects discarded this turn) and **"spells you cast from your graveyard cost {2} less"**
  (`CardDef.gy_spell_discount`; `StackItem.origin` records the zone a spell was cast from).
* **Divided X damage** (Shatterskull Smashing: "X damage divided as you choose among up to two
  target creatures and/or planeswalkers; if X is 6 or more, twice X damage instead"):
  `DivAmount::X { double_at }`; at most as many targets as damage.
* **"Whenever one or more other Cats you control die"** (Ajani): a batch trigger
  (`TrigPred::Batch`) triggers once per simultaneous event. **"Each opponent chooses an
  artifact, a creature, an enchantment and a planeswalker from among the nonland permanents
  they control, then sacrifices the rest"** (Ajani's -4, `Ext::KeepOneOfEachType`). **"If you
  control a red permanent other than Ajani"** (`Cond::ControlOtherOfColor`). Ajani was blocked
  by its IR (25.11) and tested on a copy; supported since M7d's IR fixes (section 32).
* Also: "draw cards equal to half the number of cards in your library, rounded up"
  (`Amount::HalfLibraryUp`), an emblem "you have no maximum hand size" (402.2;
  `CardDef.no_max_hand`), "discard up to N cards, then draw that many" (`Ext::DiscardUpToThenDraw`),
  "that player draws a card" only if a card was discarded (`Amount::Remembered`), "target
  opponent mills five" from an emblem, "lands you control become 1/1 creatures until end of
  turn" (Life), a +1/+1 counter on each permanent of a kind, "-1/-0 until your next turn"
  (Jace's +1), "return target instant or sorcery card; if it's green, add one mana of any
  colour" (Tamiyo's -3), the third card drawn in a turn (Tamiyo).
* **A spell whose targets are all illegal** (608.2b) is put into its owner's graveyard without
  resolving. M7a's engine sent it through the resolution's last step, which put a buyback
  spell into its owner's hand (702.27a applies as the spell resolves) and would have put an
  Adventure into exile.

### 25.7 Decisions and the bot

One new kind (36 in all): `ChooseFace` (`NUMBER` 0 = the front face, 1 = the other face),
asked after a priority id whose card can be cast or played with either face. The priority
choices `CastFace`, `PlayFace` and `Faces` use the existing `HAND`, `GRAVEYARD` and `EXILE`
ids. Everything else uses existing kinds: Etali's casts are `ChooseExiled` (`PASS` = no
more), She-Hulk's "you may" is a `YesNo`, Ajani's -4 keeps are `ChoosePermanent`, connive's
discard and "discard up to" are `Discard` (`PASS` = stop, for "up to"). The heuristic bot
(`az/bots.rs`) plays a land face when it has played no land this turn and otherwise casts the
costlier face; it counts land faces as lands and casts faces by the same rule as cards.

### 25.8 Encoding (layout v3, unchanged width, 26,500 floats)

Every new field uses spare floats of layout v3; no width changed.

* Card code (52): float 51 (the spare one) = a face other than the front is up (a transformed
  permanent, an MDFC's back face, an Adventure or a split half on the stack). The identity
  digits are the card's; the descriptor (types, colours, mana value, P/T, loyalty) is the
  face's.
* Permanent status (48): `perm::LORE` (45) = lore counters / 4; `perm::BACK_FACE` (46) = a face
  other than the front is up.
* Hand slots (60): status floats 6 and 7 = what the slot's `HAND` id does for the viewer
  deciding now: casts or plays the front face, the other face (both: a face choice follows).
* Context: task code 20 = `ChooseFace` (its card = the card); `ctx::FACES` (4 floats) = the
  mask bits (front cast, front played, other face cast, other face played). 11 context floats
  stay spare.
* Globals per player: poison / 10, cards on an adventure / 4, temporary triggered abilities /
  2, no maximum hand size; and day, night (globals 104, 105).
* No new token kind (the Construct, Clue and Cat Warrior lines already exist). The `cards` list
  of `layout_v3.json` gains the 16 newly supported cards (faces are skipped), so the file must
  be regenerated (`MTGZ_WRITE_LAYOUT=1`; not run on this branch).

### 25.9 Hidden information and fuzzing

* A face choice from a hand: the card is revealed when its `Faces` id is chosen (it becomes
  public a decision later anyway, cast or played), so the refreshed `ChooseFace` decision of a
  determinized world finds the same card (`choosing_a_face_reveals_the_card_and_survives_
  determinize`). `CastFace` and `PlayFace` move the card at once.
* A card on an adventure is in exile face up with a public permission; its twin in the hand
  stays hidden (`a_card_on_an_adventure_is_public_and_its_hand_twin_is_not_cr_715`).
* No face is up in a hidden zone: `testkit::check_invariants` checks it after every fuzz
  action, with the static-ability counters (`n_static`, `n_self_types`) that `set_face` keeps
  right.
* Etali's exiles are revealed one by one (`KEY_REVEAL`); a transform is public; Jace's -9
  emblem mills face up; Tamiyo's -7 draws are private like any draw.
* The encoding of faces is the same in every world of the viewer
  (`the_encoding_of_faces_is_the_same_in_every_world_of_the_viewer`).
* The fuzzer found two faults on the way, both fixed: an adventurer creature card in a
  graveyard was castable as an Adventure through Wrenn and Six's retrace (the route came from
  the face, not the card in its zone), and an MDFC played as its land face was keyed and
  forgotten by its face definition, not its card (the other player's cloud kept the card in
  that hand).

### 25.10 Simplifications

(All still apply; docs/shortcuts.md.)

* `face_castable` evaluates a face on a clone of the game (its timing first, without one): the
  cast checks read the card's definition throughout. Correct, and only for cards with another
  face.
* "Do this only once each turn" is built for the "you may" form only; "this ability triggers
  only once each turn" (Enduring Innocence) is another rule and a blocker
  (`TriggeredAbility.once_per_turn(not "you may ... only once each turn")`).
* Mayhem is tied to the graveyard object discarded this turn: a card that left the graveyard
  and came back is a new object (400.7) and has no mayhem.
* Split cards: only Life // Death (no fuse, no split permanent). Meld, flip cards, convert,
  "cast transformed" and read ahead are not built (prepare is, in part 3: 26.7).
* Latent activated abilities: at most four activated abilities per permanent in all (the
  `ACTIVATE` slots), checked by the lowering.

### 25.11 Blocked

* Fable of the Mirror-Breaker // Reflection of Kiki-Jiki: Reflection's "create a token that's a
  copy of another target nonlegendary creature you control" (token copies of nontoken
  permanents). Supported in part 3 (26.2); `cards_m7b_faces3.rs` now plays the real card.
* Ajani, Nacatl Pariah // Ajani, Nacatl Avenger: its IR flattens the 0's reflexive trigger
  ("When you do, if you control a red permanent other than Ajani, he deals damage ... to any
  target", 603.12) into an `IfCondition` of the loyalty ability, whose target would be chosen
  on activation. Blocked by `lower::IR_DEFECTS`
  (`IR:IfCondition(a reflexive trigger flattened: Ajani's 0)`, docs/ir_defects.md 13); every
  ability is tested on a renamed copy of its IR (`cards_m7b_faces4.rs`). Supported since M7d's
  IR fixes (`DoThenReflexive`, section 32); the tests play the cube card.
* Aang, Swift Savior // Aang and La, Ocean's Fury: waterbend (`ActivationCost.waterbend`, a
  cost that taps artifacts and creatures for mana) and airbend (`Effect:Airbend`).
* Emeritus of Abundance // Regrowth: prepare (`CardPrinting.enters_prepared`,
  `CardPrinting.prepare_spell`, `Effect:SourceBecomesPrepared`), a copy of the prepared spell.
  Supported in part 3 (26.7).

## 26. Copies (M7b part 3)

M7b part 3 grew the supported cube from 394 to 410 cards with copies (CR 707) and the cube cards
that need them: permanents that enter as copies (Phyrexian Metamorph, Superior Spider-Man) or
become copies (Saheeli, Sublime Artificer; Shifting Woodland), token copies (Fable of the
Mirror-Breaker's Reflection of Kiki-Jiki, Fractured Identity, Three Steps Ahead, Springheart
Nantuko, Sicarian Infiltrator, Thundertrap Trainer), copies of spells (Chain Lightning, Chain of
Smog, Lose Focus, Lutri, the Spellchaser, Sage of the Skies) and a preparation card (Emeritus of
Abundance // Regrowth, CR 722). On the way: squad, offspring, replicate, spree, bestow,
"another target", prepare. Esika's Chariot and Vaultborn Tyrant moved to the general token copy.
Every card's IR was read against its Oracle text and its Gatherer rulings; none is blocked by its
IR (26.12). The code is `copies.rs`, its lowering `lower/effect_copies.rs`.

### 26.1 Copiable values and copy effects (CR 707.2, 613.2)

* A permanent's `Obj.def` is the definition whose copiable values it has, as for a face (25.1):
  a copy effect turns the copied face up, so every rule that reads the definition (abilities,
  triggers, statics, mana abilities, costs, types) reads the copy, and layers 2-7 apply on top
  (613.2: copy effects are layer 1a).
* `Game.copies` holds the copy effects (`CopyFx { obj, ts, until, vals, own }`, timestamp
  order). Each copy effect sets all copiable values, so the latest on an object wins; it
  remembers `own`, the definition the object has without copy effects: what it reverts to, and
  what it is (its identity, 26.8). `Game::copiable(c)` is the latest effect's values, else the
  face up (a bestowed Aura's card: being bestowed is not a copy effect, 702.103b).
* `CopyVals { def, exc }`: the face copied and the copy's exceptions (`Except`, 707.9a-b): card
  types and subtypes "in addition", keywords ("except it has haste"), base power and toughness
  ("except it's 1/1"; then no characteristic-defining ability sets them, 707.9d) and a name
  ("his name is Superior Spider-Man"). The exceptions are copiable values (707.9b): a copy of a
  copy has them, and its own on top (`Except::over`; 707.3). The accessors `types_of`,
  `has_subtype`, `keywords_of`, `pt` and `name` read them.
* Becoming a copy (707.4) keeps the object: counters, damage, effects, attachments and
  summoning sickness stay; no enter or leave ability triggers. "Until end of turn" copy effects
  end in the cleanup step with the others (514.2); every copy effect ends when its object
  changes zones (400.7): `move_card` gives it its own definition back after the leave events and
  the look-back (603.10a) saw the copy.
* Last known information (608.2h): as a permanent leaves the battlefield its copiable values are
  recorded (`Game.lki_copy`, this turn): "create a token that's a copy of it" after it left
  (Vaultborn Tyrant, squad, offspring) and Fractured Identity (exile, then copy) read them.
* Token-ness is not a copiable value: `Game::is_token(c)` is the object's (its own definition is a
  token's, or it is a slot made as a copy); a card copying a token isn't a token.
* Double-faced permanents (707.8): a copy copies the face up. A token created as a copy of one is
  a double-faced token (707.8a): it can transform (its copy effect follows the face). A card
  that is copying a double-faced permanent doesn't transform (it isn't a double-faced card,
  701.27c).
* 704.5p: an attached permanent that is a creature, or that is no longer an Equipment (or a
  bestowed Aura), becomes unattached: a Saheeli copy of an Equipment that reverts, an Equipment
  made a copy of a creature.

### 26.2 Token copies (CR 707.2, 111)

* `Game::create_token_copies(vals, controller, n)`: n new slots (`Game.copy_objs`: slots made
  as copies, not cards), each with the copied face as its definition and a copy effect with the
  exceptions for as long as it exists, in place before it is seen entering (so "enters"
  triggers, "enters tapped" and its own enter replacements see the copy, 707.5). The token
  multipliers apply (614.1a); no token copy of an instant or sorcery card is created (111.5).
* `Ext::CreateTokenCopy { of, who, n, exc }`, `of` = a target (a permanent now, or as it last
  existed: Fractured Identity), the source (as it last existed: squad, offspring, Vaultborn
  Tyrant), the source's enchanted creature (Springheart Nantuko). Reflection of Kiki-Jiki's
  "sacrifice it at the beginning of the next end step" is a delayed ability about each token
  made (`About::EachMade`: two with a token doubler).
* Fractured Identity's "each player other than its controller" (2p: `Who::NotLkiController`).
* "Whenever you create one or more creature tokens" is for the player who created them now
  (Fractured Identity: the other player).

### 26.3 Entering as a copy (CR 707.5, 707.9, 614.1c)

* `CardDef.enters_as_copy` (the IR's `enters_as_copy_*`): what it may copy (permanents on the
  battlefield matching a filter, other than itself, or cards in any graveyard), the exceptions,
  "keeps its name", and "when you do, exile that card" (a triggered ability linked to the copy
  effect: 603.11, 707.9g).
* `put_onto_battlefield` moves such a card with its `Enters` event held back
  (`Game.defer_enters`) and pushes `Task::EnterAsCopy`: its controller chooses
  (`ChoosePermanent` / `ChooseGraveyardCard`, `PASS` = nothing); the copy effect applies (no
  duration); then what it entered as enters: "enters tapped", "enters with" counters, a
  planeswalker's loyalty, "as this enters" choices (707.5, 707.6; X is 0), and its `Enters`
  event. Superior Spider-Man's "when you do, exile that card" waits as a triggered ability about
  the chosen card.
* Phyrexian Metamorph that copies nothing is a 0/0 and dies (704.5f).

### 26.4 Copies of spells (CR 707.10)

* `Game::put_spell_copy`: a new slot on the stack (`copy_objs`), owned and controlled by the
  player who copies, with the original's definition (the face cast) and its item's targets,
  modes, X, divided damage, alternative and additional costs (707.10; the times paid, 26.5).
  It isn't cast: no spell is counted, "whenever you cast" doesn't see it; an `IsCast` event
  marked as a copy (`copies::COPIED`) triggers only "cast or copy" abilities (`TrigPred::OrCopy`,
  Witherbloom Apprentice's magecraft).
* `Ext::CopySpell { of, who, n, may, new_targets }`: this spell (Chain Lightning, Chain of
  Smog), the triggered ability's spell (Sage of the Skies, replicate) or a target spell (Lutri).
  A spell that left the stack before its "copy it" ability resolves is copied as it last existed
  (608.2h; `Game.spell_lki`, kept while an ability about it waits: the Lose Focus ruling).
* New targets (707.10c): `Task::CopyTargets` asks the copy's controller, target by target in
  order (`NewTarget`): `PASS` keeps the target (even an illegal one), else a legal target for
  that requirement (not one chosen for the same "target" word, 115.3, nor for an "another"
  requirement). Not asked when no other target is legal.
* A copy of a spell leaving the stack ceases to exist (707.10a, 704.5e) at once (`move_card`
  sends it to `Zone::Nowhere`). A copy of a permanent spell becomes a token as it resolves
  (707.10f, 608.3f); it isn't created (111.13: no token doubling, no "whenever you create").
* Chain Lightning: the damaged player, or the damaged permanent's controller
  (`Who::TargetOrItsController`), may pay {R}{R} (`OfferPay`), then may copy it (`YesNo`).
* "May copy this spell" is offered at most 31 times a turn (`copies::MAY_COPY_CAP`,
  `Game.may_copies`): Chain of Smog's copies cost nothing, so players who always copy (the
  heuristic bot) would loop forever within a turn; the CR repeats a loop of optional actions a
  chosen number of times (732.2a) and breaks one that comes back to the same game state
  (732.3). 31 keeps Chain of Smog with Witherbloom Apprentice (a drain per copy)
  lethal.

### 26.5 Additional costs paid a number of times: squad, offspring, replicate (CR 601.2b)

* `CardDef.times_cost` (`TimesCost { mana, max }`): squad (702.157a, any number), offspring
  (702.175a, once), replicate (702.56a, any number). `Task::ChooseTimes` announces the number
  after the modes and before X (`ChooseMode`, `NUMBER` = times, from 0 to what the caster can
  pay), `StackItem.times` keeps it, the payment adds it.
* The permanent's `Enters` event carries a note (`Game::put_onto_battlefield`: bit 0 = it was
  cast, the rest = the times paid): "if you cast it" (Lutri, `TrigPred::EnteredCast`), "if its
  squad / offspring cost was paid" (`TrigPred::EnteredPaid`) and "that many" (`Amount::
  EventTimes`) read it. A copy of a spell keeps the times paid (707.10) but wasn't cast.
* Replicate's triggered ability is synthesized by the lowering ("when you cast this spell, if a
  replicate cost was paid, copy it for each time"; `TrigPred::SpellPaid`, `Amount::
  EventSpellTimes`).

### 26.6 Spree, bestow, "another target"

* **Spree** (702.172a, 700.2h): `Modes.costs`, a mana cost per mode. A mode is offered only if its
  cost can be paid with the others chosen; the spell is castable if some mode whose targets can
  be chosen is payable; the chosen modes' costs are added to the cost (also when the spell is cast
  without paying its mana cost).
* **The total cost** (601.2f) is the mana cost plus these additional costs (spree modes, squad,
  offspring, replicate) and increases, less reductions: a reduction larger than the generic part
  of the mana cost reduces them too (`Game::route_mana_plus`; Green Goblin's discount on a
  Thundertrap Trainer cast with offspring). Kicker and the other additional cost options of
  M4b-1 and M7a were still added after reductions here; fixed in part 3b (27.6): Tear Asunder
  kicked, cast with mayhem under Green Goblin's {2} discount, costs {G}{B}.
* **Bestow** (702.103): an alternative cost (`AltCost.bestow`) and a bestowed face lowered from
  the card (`CardDef.bestow`): an Aura enchantment with enchant creature (no creature types, power
  or toughness), "enchanted creature gets +p/+t" as a static of `Scope::Equipped`, the card's
  other abilities. Castability is evaluated as that Aura spell (702.103d: noncreature spell
  taxes apply, a creature to target; on a copy of the game with the face up). Cast bestowed, the
  face is turned up and its target chosen; as it resolves, an illegal target makes it a creature
  spell again (702.103e, 608.3b), else it enters attached (608.3c). Unattached, or attached to
  an illegal object (no longer a creature, protection from it), it becomes unattached and ceases
  to be bestowed: the enchantment creature again (702.103f, an exception to 704.5m; checked
  with the state-based actions).
* **"Another target"** (115.3): `TargetReq.distinct` (the IR's `distinct_from`): its targets must
  differ from those of the named requirements. Castability searches for distinct choices; a
  choice that would leave a later requirement without a target isn't offered (Saheeli's -2 with
  one artifact creature is not offered at all).

### 26.7 Prepare (CR 722)

* A preparation card's prepare spell is a face of its own (`CardDef.prepare`, an instant or
  sorcery; the card's identity). "Enters prepared" (`CardDef.enters_prepared`) and "becomes
  prepared" (`Ext::BecomePrepared`) make a permanent with a prepare spell prepared, once
  (722.3a): its controller creates a copy of the card in exile with only the prepare spell's
  characteristics (722.3c; a `copy_objs` slot), listed in `Game.prepared` with its permanent.
  The prepare spell is a copiable value (722.2b): a Phyrexian Metamorph copying Emeritus enters
  prepared.
* The prepared permanent's current controller may cast the copy (for its mana cost,
  `PermKind::Prepared`, an `EXILE` id) while it stays prepared on the battlefield; as the copy is
  cast the permanent is no longer prepared. The copy is the exception to 704.5e while its
  permanent stays prepared; otherwise it ceases to exist. Being prepared is not a copiable
  value.
* Emeritus of Abundance // Regrowth: "This creature enters prepared. Whenever this creature
  attacks, if you control eight or more lands, this creature becomes prepared." Regrowth: "Return
  target card from your graveyard to your hand."

### 26.8 Decisions, identities and the bot

* One new kind (37 in all): `NewTarget` (707.10c). The rest use existing kinds: what to enter as a
  copy of is `ChoosePermanent` or `ChooseGraveyardCard` (`PASS` = nothing); the times paid is a
  `ChooseMode` (`NUMBER`), like X; "may copy" and Chain Lightning's {R}{R} are `YesNo`; bestow is
  `ChooseAltCost` (`NUMBER` 1); the prepare copy is cast with its `EXILE` id.
* Identity (22.2): a permanent with copy effects keeps the identity of what it is (`own`: the
  Metamorph card); a token copy takes the identity of the card it copies (its definition is that
  card's face), as faces and emblems take their card's; a copy of a spell, the copied card's; a
  copy of a token, the token kind's. Token copies need no registry line of their own: the
  registry keeps one line per token kind defined by a token printing. (Vaultborn Tyrant's token
  copy was a token definition of its own, `Vaultborn Tyrant 6/6 G ac Dinosaur trample`; it is now
  a token copy of the card, and the line is unused.)
* The heuristic bot enters as a copy of the most valuable permanent (or the costliest card),
  keeps a copy's targets (the first option, `PASS`), pays no repeatable cost (`NUMBER` 0), says
  yes to "may copy".

### 26.9 Encoding (layout v3, 29,092 floats)

* Card code of a permanent: the identity digits are what it is (`own_def`), the descriptor the
  copiable values it has (the copied face); float 46 (token) is `is_token`.
* Permanent status 48 -> 88 floats (`PERM_W` 100 -> 140): `perm::COPY` (47, a copy effect
  applies), `perm::COPY_OF` (48-79, the identity digits of what it copies), `perm::ATTACHED`,
  `ATTACHED_SLOT`, `ATTACHED_OPP` (80-82: what an Equipment or a bestowed Aura is attached to;
  M7a's Equipment had no such field), `perm::PREPARED` (83); 84-87 spare.
* Stack status 24 -> 28 floats (`STACK_W` 76 -> 80): `stack::COPY` (a copy of a spell),
  `stack::TIMES` (times paid / 4), `stack::BESTOWED`; one spare.
* Context: task codes 21 (`EnterAsCopy`, its card = what it is), 22 (`CopyTargets`, its card = the
  copy's; `ctx::REQ` = the target's position / 4), 23 (`ChooseTimes`).
* Bags: a permanent counts under what it is (`own_def`).
* New token kind to register: `Insect 1/1 G c Insect` (Springheart Nantuko). Servo, Goblin
  Shaman and Treasure have lines.
* `mtgz/az/layout_v3.json` must be regenerated (`MTGZ_WRITE_LAYOUT=1`) and the token registry
  appended (`MTGZ_WRITE_TOKENS=1`); `tests/az.rs` expects 29,092 features and checks a copy's
  `COPY_OF` digits (not run on this branch).

### 26.10 Hidden information and fuzzing

* Everything copied is public (battlefield, stack, graveyard cards); the copy choices are
  between public objects; spell copies never enter a hidden zone, and token copies moved to one
  are known to all (as every token). `determinize` keeps copies and a pending "enter as a copy"
  choice, and their encoding is the same in every world of either viewer
  (`rules_copies.rs::copies_are_public_and_survive_determinize`); so does a copy of a spell whose
  new targets are being chosen while both hands are hidden
  (`a_copy_of_a_spell_choosing_new_targets_survives_determinize_cr_707_10c`). `view_of` names a
  prepared permanent's copy by the permanent's place.
* The fuzzer found three faults on the way, all fixed: bestow's castability ignored the tax on
  noncreature spells (Thalia; now evaluated as the Aura spell), `view_of` hashed a prepared
  permanent's slot index (a check-only fault), and a player could control more than the 96
  permanents the action layout has slots for (Retrofitter Foundry with a token doubler: an old
  limit the new pool reached, 26.11).

### 26.11 Simplifications

* A copy of a spell leaving the stack ceases to exist at once, not at the next state-based check
  (nothing can see it in the other zone first).
* The "enter as a copy" choice is made right after the card moves (its `Enters` event and enter
  replacements wait for it, so nothing can see the difference). (A permanent entering at the same
  time, Exhume's other creature, can't be chosen since M7e: `Game::group_ts`.)
* A double-faced card that is copying something doesn't transform.
* "Another target" requirements choose at most one target each (the lowering checks).
* A copy effect's exceptions hold at most four subtypes.
* A player controls at most 96 permanents (the `MY_PERM` / `OPP_PERM` slots): token creations
  beyond that create fewer tokens.
* At most 31 copies a turn by "may copy this spell" effects (26.4; a loop of optional actions,
  732.2a, is cut there rather than asked as a number).
* (Kicker's and the other M7a optional additional costs were added after cost reductions; fixed
  in part 3b, 27.6.)
* Chain of Smog's IR flag `needs_target_with_cards_in_hand` (a Python shortcut) is ignored: the
  copy is always offered (section 14).

### 26.12 IR

* The 16 cards' IR matches their Oracle text, with a few things the IR leaves to code: Shifting
  Woodland's "target permanent card in your graveyard" and Lutri's "target instant or sorcery
  spell you control" have no target specification (re-derived in the lowering, as in
  docs/ir_defects.md 11); Chain of Smog's "that player" is the spell's target player (found
  among the lowered targets); Lutri's companion ability isn't in the IR (it works from outside
  the game, before it starts: these games have no sideboard; here Lutri is a flash creature).
  The `*_cost_str` twins of squad, offspring, bestow and Springheart's payment are checked
  against the structured costs (section 3, rule 4a).

## 27. The stack as a target, ward, the initiative, graveyard abilities (M7b part 3b)

M7b part 3b grew the supported cube from 410 to 438 cards, ending the M7b card batch. Twenty
cards came with four features: spells and abilities on the stack as targets and "becomes the
target" (Tishana's Tidebinder, Ertai Resurrected, Invert Polarity, Leovold, Emissary of Trest,
Surrak, Elusive Hunter), ward and improvise (Sedgemoor Witch, Kappa Cannoneer, Raffine, Scheming
Seer, Hexing Squelcher, Sunset Saboteur), the initiative and the Undercity dungeon (Feywild
Caretaker, Passageway Seer, White Plume Adventurer, Caves of Chaos Adventurer, Seasoned
Dungeoneer, Undermountain Adventurer, From the Catacombs) and abilities activated from a
graveyard (Fanatic of Rhonas, Otharri, Suns' Glory, Seasoned Pyromancer). Eight more needed one
small feature each afterwards (27.9). Three faults in the cost rules were fixed first, each with a
failing test (27.6). Every card's IR was read against its Oracle text; none is blocked by its IR
(27.14). The code is `stack_ext.rs` and `dungeon.rs`, the lowering `lower/effect_stack.rs`.

### 27.1 Spells and abilities on the stack as targets (CR 115.1, 115.4, 115.5)

* A spell is targeted as its card (`Target::Obj`, as before). An activated or triggered ability on
  the stack is `Target::Stack(id)`, its stack item's id. `Filter::Ability(mask)` (activated,
  triggered; the IR's `StackObjectKind`) names abilities; "target spell, activated ability, or
  triggered ability" is an `Or` of a spell filter and it. A target id is the item's `STACK` slot.
* Candidates are the stack top first; abilities are listed only for a filter that names them
  (`cast::wants_ability`). An ability is never its own target (115.5): `choose_targets` leaves
  out its own id, and `Game.choosing_for` holds the id while its modes are chosen, so Ertai's
  "counter target ... triggered ability" mode isn't offered for itself alone (found by a card
  test). Mana abilities never use the stack (605.3b) and can't be targeted. An ability of a card
  in a graveyard is an activated ability; the initiative's and Undercity's abilities are
  triggered abilities (27.4).
* A targeted ability that left the stack is an illegal target (608.2b).

### 27.2 Countering abilities, gaining control of a spell, becoming the target

* `Op::Counter` counters a spell or an ability (`Game::counter`, 701.5a): a spell goes to its
  owner's graveyard unless it can't be countered (`spell_uncounterable`: "this spell can't be
  countered", Surrak, Hexing Squelcher; or its controller controls a permanent with "spells you
  control can't be countered", Hexing Squelcher, 101.2); an ability is removed from the stack.
* Tishana's Tidebinder (`Ext::SourceOfTargetLosesAbilities`): if the countered ability's source
  is still the same artifact, creature or planeswalker on the battlefield, it loses all abilities
  (`Mod::LoseAllAbilities`) with the duration `Until::WhileOnBattlefield(Tidebinder)`, removed
  when Tidebinder leaves (611.2b; nothing if Tidebinder had already left). The Wondrous Wasp's
  "for as long as" uses the same duration (27.9).
* Ertai Resurrected's "its controller draws a card" remembers the target's controller before the
  effect (608.2h: a countered spell's or destroyed creature's controller still draws).
* Invert Polarity: a coin flip; won, `Ext::GainControlOfSpell` makes the spell (and its card)
  the flipper's (608.3a: a permanent spell enters under its new controller), who may then choose
  new targets one by one (`Task::CopyTargets` with `copy` false, 115.7); lost, it is countered.
* "Becomes the target" (603.2e): `EventKind::BecomesTarget`, one event per player or object as
  it becomes a target of a spell or ability, with the targeting item as `Subj::Item(id)`; the
  event's player ("that player", ward's payer) is that item's controller. A player or permanent
  is the event's `Slot::Target`, a spell its `Slot::TargetSpell` (Surrak's "a creature spell you
  control"). The events come as targets are chosen (601.2c, 602.2b, 603.3d), once per object for
  one spell even if two of its targets name it, as a stolen spell's new targets are chosen
  (115.7), and when a copy of a spell is put on the stack (its targets become targets then,
  707.10c).
* Leovold ("you or a permanent you control becomes the target of a spell or ability an opponent
  controls, you may draw a card") and Surrak (a creature or creature spell you control) are
  generic triggers on the new event; the opponent test is on the item's controller.

### 27.3 Ward (CR 702.21)

* `CardDef.ward` (`WardCost`: `Mana`, `Life(n)`, `Discard`). The lowering synthesizes the
  triggered ability (`effect_stack::ward_trigger`): "whenever this permanent becomes the target
  of a spell or ability an opponent controls, counter that spell or ability unless that player
  pays <cost>". The cost is offered to the targeting item's controller: mana with `OfferPay`
  (`YesNo`), life with `Ext::OfferPayOther` (`YesNo`, `YES` only with at least that much life,
  119.4), a card with `Ext::OfferPayOther` too (one `Discard` decision: a hand card pays, `PASS`
  declines). Unpaid, `Ext::CounterEventItem` counters the event's item if it is still on the
  stack (a spell that can't be countered stays, 101.2).
* Each instance triggers separately: a Hexing Squelcher with its own ward and another
  Squelcher's has two triggers.
* Granted ward (Hexing Squelcher: "other creatures you control have 'Ward—Pay 2 life'",
  `CardDef.grants_ward`): the trigger is the granting card's definition's, about a target that is
  another creature its controller controls (`TrigPred::Granted(Slot::Target)`: one that didn't
  lose all abilities later than the grant's timestamp, 613.7; Oko's Elk has no ward). Its source
  is that creature (113.7, since M7e: the object that has the ability), so Tishana's Tidebinder
  countering it makes the creature lose its abilities.
* Improvise (702.126, Kappa Cannoneer): `CardDef.improvise`; while its cost is paid, each
  untapped artifact the payer controls may be tapped for {1} of generic mana (`PayMana.
  improvise`; the artifact's `MY_PERM` id with `Choice::Card`, marked by the `IMPROVISE` slot;
  castability counts them as sources of one generic mana). An artifact with a mana ability can
  pay either way, not both; improvise artifacts are tapped last as the payment completes.

### 27.4 The initiative and Undercity (CR 725, 309, 701.49)

* `Game.initiative: Option<PlayerId>`. Taking it (`Ext::TakeInitiative`, 725.1) triggers the
  initiative's first inherent ability; the others trigger at the upkeep of the player who has it
  and when one or more creatures a player controls deal combat damage to that player (once per
  attacking player). They have no source (725.2): they are the triggers of a rules definition
  named "The initiative" (`CardDef.rules`), put on the stack with a slot in no zone as their
  source (`Game.rules_srcs`, one per rules definition: the monarch's too since M7e), controlled by
  the player they name.
* A player's dungeon is an object in the command zone whose definition is "Undercity"
  (`Game.dungeons[p] = Dungeon { obj, room, completed }`). Venturing into Undercity
  (`Ext::Venture`, 701.49a-c): with no dungeon, Undercity is put into the command zone with the
  marker on Secret Entrance; else the marker moves to a room an arrow leads to (`ChooseRoom`,
  `NUMBER` = the room's index in `dungeon::ROOMS`, asked when there are two). Entering a room
  triggers its ability (309.4c; Undercity's triggers, by room index): Secret Entrance (search for
  a basic land, to hand), Forge (two +1/+1 counters on target creature), Lost Well (scry 2), Trap!
  (target player loses 5 life), Arena (goad target creature), Stash (a Treasure), Archives (draw),
  Catacombs (a 4/1 black Skeleton with menace), Throne of the Dead Three (reveal the top ten, put a
  creature card onto the battlefield with three +1/+1 counters, hexproof until your next turn,
  shuffle: `Ext::PileToBattlefield`, `PutOntoBattlefield`).
* Completing (309.7, 704.5t): a dungeon whose marker is on its bottommost room, with no room
  ability of it waiting or on the stack, is removed by the state-based actions and its owner has
  completed it (`completed`, `Cond::CompletedDungeon`: White Plume Adventurer, Caves of Chaos
  Adventurer, Undermountain Adventurer). The next venture starts a new Undercity.
* Both rules definitions (and the Treasure and Skeleton tokens) are built by the lowering when a
  card takes the initiative and stored with the tokens (`lower/effect_stack.rs::initiative_defs`;
  `dungeon.rs` finds them by name).
* Goad (701.38, Arena): `Mod::Goad` until the goading player's next turn; the creature attacks
  each combat if able (`must_attack`) and a player other than the goading player if able: in two
  players, a creature its own controller goaded attacks the opponent (not a planeswalker); one
  the opponent goaded attacks as usual.
* "If you have the initiative" (Feywild Caretaker, Passageway Seer) is `Cond::HasInitiative`, an
  intervening "if" (603.4).

### 27.5 Abilities activated from a graveyard (CR 602.2, 113.6)

* `CardDef.gy_abilities` (at most one per card, the lowering checks): `lower_activated` with
  `From::Graveyard`. Priority offers it as `GY_ABILITY + k` (ids 1012-1023, the old reserved
  range): the k-th card in the player's graveyard, bottom first, that has one (counted whether or
  not it can be activated now), `Choice::Ability(card, AbilitySlot::Graveyard(0))`.
* Costs: "exile this card from your graveyard" (`exile_self`, eternalize, Seasoned
  Pyromancer); "tap an untapped Rebel you control" (`Cost.tap_untapped`, one `ChoosePermanent`
  per permanent after the mana).
* Fanatic of Rhonas: eternalize (702.129a) is a token copy of the card as it last existed in the
  graveyard (`CopyOf::SourceCard`), except a 4/4 black Zombie with no mana cost (`Except.colors`,
  `Except.no_cost`: mana value 0); its ferocious mana ability is `ManaProd::IfElse`.
* Otharri: experience counters (`Player.experience`, `Ext::GetExperience`,
  `Amount::Experience`), Rebels created tapped and attacking per counter (`Ext::CreateTokensN`),
  and "return this card from your graveyard to the battlefield tapped"
  (`Ext::SourceFromGraveyard`).
* Seasoned Pyromancer: "discard two cards, then draw two cards; for each nonland card discarded
  this way, create a 1/1 red Elemental" (`Ext::DiscardThenDraw`), and from the graveyard two
  Elementals.

### 27.6 Cost rules fixed (CR 601.2f, 118.9d, 108.2)

Three faults, each found with a failing test first (`tests/rules_costs.rs`):

* **The total cost** (601.2f) is the mana cost or the alternative cost, plus every additional
  cost announced (kicker, buyback and M7a's other options, spree, squad, offspring, replicate)
  and X, plus increases, less reductions. Kicker, the other additional-cost options and X were
  added after the reductions: kicked Tear Asunder cast with mayhem under Green Goblin's {2}
  discount cost {1}{G}{B} instead of {G}{B}, and a Walking Ballista cast from the graveyard so
  paid {2} too much. `Game::total_mana` is now the one total.
* **Reductions apply to an alternative cost** (601.2f, 118.9d): an alternative cost took the
  increases but no reductions. Green Goblin's discount now reduces a flashback or escape cost
  from the graveyard, and an affinity-like reduction an alternative cost (the custom "Test
  Discount Alt"; the lowering's blocker for reductions with alternative costs is gone).
* **A token in a graveyard isn't a card** (108.2, 111.1): between a token dying and 704.5d it is
  in a graveyard, and threshold, delirium, the Lhurgoyfs' card types, delve, escape's exiled
  cards and graveyard choices counted it. They count cards only now (`Game::graveyard_cards`).
  Supported cards whose results change: Barrowgoyf and Pyrogoyf (their toughness in the same
  state-based check), Dragon's Rage Channeler, Unholy Heat, Shifting Woodland, Cabal Ritual,
  Treasure Cruise, Dig Through Time, the escape cards.

### 27.7 Other rules and effects added

* Connive N (701.50, Raffine: X = the attacking creatures, `Amount::Attacking`):
  `Ext::Connive { on, n }`.
* From the Catacombs: "with a corpse counter on it" and "if that creature would leave the
  battlefield, exile it instead" (`Mod::ExileIfLeaves`, a replacement, 614.1a); its escape cost
  exiles five other cards.
* Caves of Chaos Adventurer's "if you've completed a dungeon, you may play that card this turn
  without paying its mana cost; otherwise, you may play that card this turn" (checked as it
  resolves); Seasoned Dungeoneer's protection from creatures and explore; Undermountain
  Adventurer's {G}{G} or six {G} (`ManaProd::IfElse`); White Plume Adventurer's untap at each
  opponent's upkeep.
* `Op::Ext` gains `SourceOfTargetLosesAbilities`, `GainControlOfSpell`, `CounterEventItem`,
  `OfferPayOther`, `TakeInitiative`, `Venture`, `PileToBattlefield`, `GetExperience`,
  `CreateTokensN`, `DiscardThenDraw`, `SourceFromGraveyard`; `Connive` takes a target and an
  amount. Each is a rules action (section 16, rule 3).

### 27.8 Decisions, identities and the bot

* Two new kinds (39 in all): `ChooseRoom` (`NUMBER` = room) and `PutOntoBattlefield` (Throne of
  the Dead Three: `LIBRARY` = the card's slot in the revealed cards sorted by definition, one id
  per definition). Ward's payments use `YesNo` and `Discard` (with `PASS`); graveyard abilities
  `GY_ABILITY`; improvise `PayMana` with `MY_PERM` ids; an ability target a `STACK` id.
* Identities (22.2): the rules definitions are in the token registry as `Undercity rules` and
  `The initiative rules` (`identity::token_key`; `tests/az.rs` treats `d.token || d.rules`
  alike). New token kinds: `Elemental 1/1 R c Elemental` (Seasoned Pyromancer), `Warrior 1/1 R c
  Warrior` (mobilize). Treasure, Skeleton, Rebel, Faerie Dragon, Spirit and Clue have lines; an
  eternalized Fanatic is a token copy of the card (its identity).
* The heuristic bot puts the most expensive card onto the battlefield (`PutOntoBattlefield`),
  takes the first room, pays ward when it can (`YES`; for a discard, a card of its choosing), and treats an
  ability target as its controller's (`Heuristic::target_player`).

### 27.9 More cards needing one feature

After the 20 cards the support report's one-feature list gave eight more:

* The Wondrous Wasp: "tap up to one target creature. It loses all abilities for as long as The
  Wondrous Wasp remains on the battlefield." `Until::WhileSource`, a lowered duration that an op
  turns into `WhileOnBattlefield(source)` as it resolves, or into nothing if the source already
  left (611.2b: "If the 'for as long as' duration never starts, the effect does nothing").
* Voice of Victory: mobilize 2 (702.181a): two 1/1 red Warriors tapped and attacking, each
  sacrificed at the beginning of the next end step (a delayed ability about each token).
* Brain Freeze: storm (702.40a), a cast trigger that copies the spell for each spell cast before
  it this turn, by any player, with new targets (`Amount::EventAmount`: the `IsCast` event's
  amount is that number, counted as the spell becomes cast; `copies::COPIED`, the mark of a
  copy's event, moved to `u16::MAX`). Copies aren't cast and don't count.
* Occult Epiphany: draw X, discard X, a Spirit for each card type among the discarded cards
  (`Ext::DiscardThenDraw { types }`, 205.2a).
* Fallen Shinobi: "that player exiles the top two cards of their library. Until end of turn, you
  may play those cards without paying their mana costs" (`Ext::ExileTopWithPermission`, the
  event's player). Its ninjutsu exposed an older fault (27.12).
* Otawara, Soaring City and Boseiju, Who Endures: "this ability costs {1} less to activate for
  each legendary creature you control" (`ActivatedDef.discounts`, `Game::act_mana`; generic mana
  only, 602.2b). Boseiju's "that player may search their library for a land card with a basic
  land type, put it onto the battlefield" is the destroyed permanent's controller's search
  (remembered first, 608.2h).
* Forensic Gadgeteer: "activated abilities of artifacts you control cost {1} less to activate.
  This effect can't reduce the mana in that cost to less than one mana"
  (`CardDef.artifact_ability_discount`).

### 27.10 Encoding (layout v3, 29,860 floats)

* Globals: per player (viewer first), in the spare floats 34-37: has the initiative, has a
  dungeon, dungeons completed / 2, experience counters / 4; game globals 106-123: the venture
  marker's room as a one-hot, nine per player (`encode::glob`).
* Permanent status 88 -> 96 floats (`PERM_W` 140 -> 148): ward instances / 2, the largest ward
  mana cost / 4, the largest ward life cost / 4, a discard ward (84-87; own and granted, as the
  triggers see them), lost all abilities (88), goaded (89), exiled if it would leave (90); five
  spare.
* Stack status 28 -> 32 floats (`STACK_W` 80 -> 84): targeted by another stack item (27), a
  spell that can't be countered (28), an ability's index ((index + 1) / 8: which of its card's
  triggered or activated abilities, a ward trigger among them; 29), an ability of a card in a
  graveyard (30); one spare. A target that is an ability is its stack slot (target floats 6-7,
  as a spell's).
* A new segment `graveyard_abilities` (4 slots x 56, at the end): the viewer's graveyard cards
  with a graveyard ability in `GY_ABILITY` order (card code, present, its id is an option);
  `layout_json` scores `GY_ABILITY + k` from slot k (ids 4-11 fall back to the generic logits, as
  the `HAND` ids beyond the 16 slots do).
* The card code's descriptor of a permanent shows a copy's exceptions (types added, colours, no
  mana cost, base power and toughness: an eternalized Fanatic is a black 4/4 of mana value 0).
* Context: `ctx::SLOT_KIND` widened from 4 to 5 floats (a graveyard ability), so the fields after
  it move by one (`ctx::END` 245 -> 246, ten spare). No new task code; every new instruction is an `Ext` op.
* Bags: dungeons are not counted among the emblems.
* `mtgz/az/layout_v3.json` and the token registry must be regenerated (`MTGZ_WRITE_LAYOUT=1`,
  `MTGZ_WRITE_TOKENS=1`: four new keys, 27.8); `tests/az.rs` expects 29,860 features and checks
  the graveyard-ability slots (not run on this branch). The rules tests check the new fields
  (`rules_ward.rs`, `rules_stack.rs`, `rules_initiative.rs`, Fanatic of Rhonas).

### 27.11 Hidden information

* The initiative, dungeons, rooms, experience and every stack target are public; `view_of` and
  the fingerprint include them (an `Until::WhileOnBattlefield` duration and an ability target
  are named by place). A pending room choice survives `determinize` for either viewer with the
  same encoding (`rules_initiative.rs`). Ward's discard is from the payer's own hand; Throne of
  the Dead Three reveals the ten cards; Fallen Shinobi's exiled cards are revealed.

### 27.12 Fuzzing

20,000 games over the 438 cards: see section 15. The first run found one fault, older than this
part: a cost that returns a permanent to its owner's hand (ninjutsu's "return an unblocked
attacker you control") didn't reserve it from the mana payment, so an attacking Eldrazi Spawn
could be sacrificed for {C} and then the cost had nothing to return (Kaito's ninjutsu had the
same fault). `Keep.sac` now reserves the permanents a "return" cost needs as it does for a
sacrifice (`tests/cards_m7b_stack3.rs::ninjutsu_cant_pay_its_mana_with_the_attacker_it_returns_cr_601_2h`).

### 27.13 Simplifications

What still applies (docs/shortcuts.md; M7e fixed granted ward's source and "with a counter on
it" as the permanent enters):

* White Plume Adventurer's "untap a creature you control" offers only tapped creatures.
* A "tap an untapped Rebel you control" cost counts, for castability, only Rebels with no mana
  ability (one could tap for mana first); no Rebel in the cube has one.
* One graveyard ability per card; `GY_ABILITY` names the first 12 such cards of a graveyard.
* Undercity is the only dungeon ("venture into the dungeon" isn't supported; no supported card
  has it).
* The initiative's abilities are controlled by the player they name and have a sourceless slot
  as their source (725.2 says they have none).

### 27.14 IR

* The 28 cards' IR matches their Oracle text. Things the IR leaves to code: Undercity's rooms
  and the initiative's abilities (rules, CR 309.4 and 725.2, built by the lowering from the
  dungeon card), mobilize's and storm's abilities (keywords, 702.181a and 702.40a), Fallen
  Shinobi's "that player" (the class is named `TargetPlayer...` but has no target: the damaged
  player, as Python passes it). `TargetConnives` repeats "attacking" in `attacking_only` and in
  its target filter (the lowering checks they agree). `CardPrinting.ward_discard_random` has no
  cube card and is a blocker. docs/ir_defects.md has the details; no new defect.

## 28. The single-card tail, part 1 (M7c)

Branch `m7c-tail1`, from main at 7287cd1 (438 cards): the cards that needed one feature each,
and the mana cards that needed two. 29 cards came through, 467 supported; four stay blocked
(28.9). Code: `lower/effect_tail.rs` (lowering), `resolve_tail.rs` (the new `Ext` instructions),
the payment changes in `pay.rs` / `activate.rs` / `mana.rs`; tests `tests/cards_m7c_tail.rs`
(30 tests, CR cited in each name).

### 28.1 Mana abilities with a mana cost or a choice (CR 605.3a)

(M7e part 2, section 34: these are payment sources now, a need of the same payment for their
mana cost and a source on each permanent for a sacrifice of another; the priority path below
remains for Lion's Eye Diamond, Chromatic Star and Wizard's Rockets.)

Section 18's payment activates mana abilities while paying, and only those whose cost has no
choice and no mana part (a payment inside a payment). A mana ability whose cost has a mana part
(Chromatic Star, Sunken Ruins' {U/B}, Loot's {G}), a permanent to choose (Orcish Lumberjack's
Forest, Squandered Resources' land), whose mana depends on what its cost sacrifices
(Squandered), or that can be activated only once (exhaust) is now lowered as activated **with
priority only**, like Lion's Eye Diamond (`ActivatedDef::instant_only`, set by the lowering):
605.3a lets a player activate a mana ability whenever they have priority, and its mana then
waits in the pool for the spell or ability it pays. The activation (`Task::ManaNow`, then
`Task::ManaNowCosts`) pays the mana part first (`PayMana` with the source excluded, 601.2g-h by
602.2b), then asks for the permanent to sacrifice (`Sacrifice`, one decision), pays {T}, life,
counters and the source's own costs, then asks the production (`ChooseMana`). "One mana of any
type the sacrificed land could produce" reads the land's mana abilities as it is sacrificed
(106.7: the union of the types its productions make now; Wasteland: {C} only, no question;
a land that makes none: nothing).

* **Exhaust** (702.177a; Loot, the Pathfinder): `ActivatedDef::exhaust`; activating it records
  `Mark::Exhausted(i)` on the object (a new object has its own), and an exhausted ability is not
  offered again.
* **Counters as a mana cost** (Gemstone Mine): a mana ability may remove a counter from its
  source (`Cost::remove_counters`, one): it is a payment source only with a counter there, and
  "if there are no mining counters on this land, sacrifice it" (`ManaAbility::sac_if_no`) runs
  as part of the ability (605.3b); the last counter's activation counts as sacrificing its
  source for the budgets and the activation order of section 18.
* **Productions**: `ManaProd::Linked` (one mana of a colour among the cards exiled with the
  source, 607.2a: Chrome Mox's imprint, read from `Game::links`), `ManaProd::OneOf` (Sunken
  Ruins: {U}{U}, {U}{B} or {B}{B}), `ManaProd::SacrificedLand`. `pay::productions` takes the
  source.
* **"Whenever you tap a creature for mana, add an additional {G}"** (Badgermole Cub; a
  triggered mana ability, 605.1b): `CardDef::extra_mana_creature`, added to the yield of every
  creature source whose ability has {T} (the land version's walk, one walk for both) and to a
  priority mana ability's productions.

### 28.2 Spending restrictions (CR 106.6)

"Spend this mana only to cast artifact spells" (Mishra's Workshop) and "... only to cast a
legendary spell, and that spell can't be countered" (Delighted Halfling): `ManaAbility::only`
(`SPEND_ARTIFACT`, `SPEND_LEGENDARY`) and `::uncounterable`. A payment says what it allows:
`Keep::allow` (the restrictions casting this spell satisfies, `Game::spend_allow`) and
`Keep::spell`. Mana classes carry `only`; candidates leave out the classes a payment may not use
(`pay::cands`, only when such a class exists), and the `PayMana` options skip them. What a pool
can hold is limited by the lowering to what the engine represents: several colorless mana for
artifact spells (the Workshop: what its {C}{C}{C} leaves stays in the pool as artifact-only
mana, `ManaPool::art`, spent first on an artifact spell and never on anything else), or one
mana spent at once. Mana of a Halfling spent on a spell marks it uncounterable
(`Mark::Uncounterable`, read by `spell_uncounterable`; read before the sources are activated).
Every payability check of a spell (from hand, other zones, the mana-cost choice against an
alternative cost, X, delve, spree, replicate) uses what the spell allows, so a cast is offered
exactly when its payment can finish; activations and "unless pays" never use restricted mana.

### 28.3 Rare per-object facts: `Game::marks`

One vector (empty in nearly every game: a clone copies one empty vector) holds the tail's facts
about objects, each stale with its object (CR 400.7; dead ones are dropped as a turn begins):
`Exhausted(i)`, `ConvergePending` / `Converge(n)` (the colours of mana spent on a spell),
`ChosenColor(c)` (Painter's Servant), `Uncounterable`, `LeaveTrigger { def, slot, exile }` (a
triggered ability an effect gave the permanent), `GrantOnEnter` (a spell cast with Serra
Paragon's permission), `UsedTurn(t)` ("once during each of your turns").

**Triggered abilities an effect gives a permanent** ("when it dies [or is exiled]": earthbend,
Serra Paragon): `Mark::LeaveTrigger` names a delayed ability of the granting card (603.7-style
definition, `delayed_base + k`); `move_card` fires it as the permanent leaves the battlefield for
a graveyard (or exile), about the card where it went (603.10a: it looks back), controlled by the
permanent's controller.

### 28.4 The cards

| Card | What it needed | Decisions |
|---|---|---|
| Mana Vault | `OptionalManaCostThenEffect` ("you may pay {4}. If you do, untap it": `OfferPay`); "at the beginning of your draw step, if it is tapped" as an intervening if checked again on resolution (603.4: an "if" about the source of a beginning-of-step trigger adds `JumpUnlessSource`) | `YesNo`, `PayMana` |
| Coalition Relic | charge counters; `RemoveAllCountersAddMana` (one mana of any colour per counter, 106.4) | `ChooseColor` per mana, non-decreasing |
| Chrome Mox | imprint (`ExileFromHandLinked` from one's own hand); `ManaProd::Linked` | `ChooseDiscard` (own hand) |
| City of Traitors | `EventKind::PlaysLand` (305.1: played, not put; subjects the land and the player), `Slot::Land` | - |
| Gemstone Mine | 28.1 | - |
| Sunken Ruins, Loot, Orcish Lumberjack, Squandered Resources | 28.1 (Loot's other two exhaust abilities are ordinary) | `PayMana`, `Sacrifice`, `ChooseMana` |
| Mishra's Workshop, Delighted Halfling | 28.2 | - |
| Badgermole Cub | earthbend (701.65a: the land becomes a 0/0 creature with haste, `Until::Forever`, +1/+1 counters, and gains "when it dies or is exiled, return it to the battlefield tapped", 28.3); 28.1 | `Target` |
| Palantir of Orthanc | `LetDrawElseMill`: the target opponent decides (101.4), else mill X (the source's counters as it last existed) and they lose the milled mana value | `YesNo` (the opponent's) |
| Tersa Lightshatter | `ExileRandomGraveyardPlayable` (a public chance key, `KEY_DIE`) with a play permission this turn | - |
| Demonic Consultation | `NameExileRevealUntil` (24.5's naming: `PASS` names a card not in the library, exiling it all) | `NameCard` |
| Balance | `Balance`: lands, then hands, then creatures; each player above the fewest chooses what goes (APNAP), permanents as a set in slot order, sacrificed at once; cards discarded one at a time; a player shielded from the controller's effects keeps everything | `Sacrifice`, `Discard` |
| Ursine Monstrosity | `MillAttackPump`; `Mod::MustAttackPlayer` (attacks that player if able, never one of their planeswalkers; two players: the random opponent is the opponent) | - |
| Transmute Artifact | `SacrificeTutorPayDifference` | `Sacrifice`, `SearchLibrary`, `YesNo`, `PayMana` |
| Lim-Dul's Vault | `LookTopRepeatPayLife` then the pile on top (`PileTo`) | `YesNo` per round (life at least 1, 119.4), `PutOnTop` |
| Malcolm, Alluring Scoundrel | `LootMayCastFree`: a free cast of the discarded card from the graveyard (608.2g; a graveyard permission's "without paying" now counts) | `Discard`, `YesNo` |
| Time Vault | `CardDef::skip_turn_untap`: as a turn of its controller would begin while it is tapped, "skip that turn" replaces it (614.10; the next turn is computed as usual) and untaps it | `YesNo` (`Task::MaySkipTurn`) |
| Elite Spellbinder | `ExileFromHandTaxed`: `PermKind::Taxed(n)` (its owner may play it while it stays exiled) and `CastFrom::Taxed(n)` (601.2f: {n} more) | `ChooseDiscard` (the seen hand) |
| Cloak and Dagger, Entwined | target opponent and up to one creature they control; `ExileHandCardOrTargetUntil` (a nonland card of the revealed hand or that creature, until it leaves, 610.3) | `Target`, `ChooseDiscard` |
| Black Widow, Super Spy | `ExileUntilNonlandCounterOrCast`: the counter, or a permission this turn with mana of any type (609.4b, `any_mana`) | `YesNo` |
| Prismatic Ending | converge: `Filter::MvAtMostConverge`, `CardDef::converge`; the payment records the colours (28.8) | X, `Target`, `PayMana` |
| Painter's Servant | "as it enters, choose a color" (`ChooseColor`, `Mark::ChosenColor`); every card, spell and permanent (not an emblem) has that colour too (`colors_of`, and a spell's damage source) | `ChooseColor` |
| Sunfall | `ExileAllCreaturesIncubate`; the Incubator token is a double-faced token (its back face, Phyrexian Token, lowered with Sunfall's faces and placed by `CardDb::from_cards` with the token as its front); a token transforms when it is double-faced | - |
| Serra Paragon | `CardDef::gy_play_mv`: once during each of its controller's turns (per Paragon, `Mark::UsedTurn`) a land played or a permanent spell of mana value 3 or less cast from their graveyard (`CastFrom::Paragon`), which gains its trigger (28.3) | - |
| Goblin Welder | `TargetReq::pair`: the second target is a card owned by the controller of the first; offered only when a pair exists, the first target only if it can be paired; `SwapWithGraveyardCard` checks the pair again (608.2b) and sacrifices and returns in one batch | `Target` x2 |

### 28.5 Decisions, tasks, the bot

No new decision kind (39 still). New tasks: `ManaNowCosts` (asks `Sacrifice`; task code 12, as
`PayCosts`) and `MaySkipTurn` (asks `YesNo`; task code 16, as `AsEnters`); every new `Ext` op is
op code 37. The heuristic bot stays legal for every decision; it declines Time Vault's skip and
Lim-Dul's Vault's life payments (it answered yes to every `YesNo`: it would skip its own turns
and pay life down to 0) and still never activates mana abilities at priority. Measured after
(`strength_against_the_old_policy_and_random`, 1,000 games each): heuristic vs random 95.6% +-
0.6, vs `HeuristicV1` 64.3% +- 1.1 (section 21.4: 95.8, 62.9 on main's pool).
`tests/bots.rs::registered_bot_beats_random` now plays 200 games: the new pool gave 0.833 on its
first 60 (one game under 0.85) and 0.895 over 200.

### 28.6 Encoding (layout v3, unchanged)

Nothing in `az/encode.rs` changed: the new cards fill their identities. New token kind to
register: the Incubator (an artifact, no power and toughness; its back face has the Incubator's
identity, as a face has its front's). Not encoded (no spare floats were used): `Game::marks`
(an exhaust used, a chosen colour, a converge count, a granted leave trigger, a Paragon used this
turn) and `ManaPool::art` (the pool's {C} count includes it).

### 28.7 Hidden information and fuzzing

Demonic Consultation reveals every card it exiles or takes (face up); Lim-Dul's Vault's piles
are looks (`look_top`), and the end puts the rest of the library on the bottom in a random
order nobody knows (`put_on_bottom_randomly`: the shuffle, with the pile left on top); Balance's
discards happen one at a time; Elite Spellbinder and Cloak and Dagger choose from a hand the
chooser has seen (a `LIBRARY` slot of it); Tersa's random card is a public chance key.
Transmute Artifact's found card waits, while the payment is decided, revealed on top of the
library already shuffled (the shuffle comes early: nothing in between touches the library): the
fuzzer's information-set check caught the first version, which revealed it at its library
position (a position that differs between sampled worlds under the same keys).

Fuzzing (20,000 games over the 467 cards): no failure (70 s). Faults found on the way, all fixed:
that leak; the "can't be countered" check read the sources' abilities after they were activated (a source gone by then); a
spell with an alternative cost offered as castable with restricted mana while the mana-cost
choice ignored it ("ChooseAltCost with no options"); a Paragon permission looked up after the
card moved to the stack. The speed measurement (17) caught one more: a card of mana value 0 was
castable from a graveyard with no Serra Paragon (same games on main's pool after the fix).

### 28.8 Simplifications (differences from the CR)

What still applies (docs/shortcuts.md; M7e fixed converge's upper bound, Serra Paragon's routes
and faces and the granted leave trigger after "loses all abilities"):

* (Mana abilities with a mana cost or a choice in their cost: while paying since M7e part 2,
  section 34.)
* Converge counts the colours the payment could show: the pool's and several-mana sources' mana
  as the player chose to spend it (`PayPool`), and for single-mana sources the most colours an
  exact payment by them can have, exactly for any number of sources (a player who taps a dual
  land chooses its colour, 106.1b; for Prismatic Ending and Pentad Prism more is never worse).
* Lim-Dul's Vault puts each looked-at five on the bottom in the order looked at (an order the
  player could choose; the library is shuffled at the end, so only a loop through the whole
  library could tell).
* Painter's Servant's colour is added before any "becomes [colour]" effect regardless of
  timestamps (the cube has no such effect).
* Goblin Welder: if "that player" is shielded from the controller's effects, the artifact isn't
  sacrificed but the card still returns.

### 28.9 Still blocked

* **Chromatic Star**: an IR defect (docs/ir_defects.md 15): its "put into a graveyard from the
  battlefield" trigger is `leaves_battlefield` in the IR (it would draw when exiled or bounced).
  Its mana ability ({1}, {T}, sacrifice) lowers. Supported since M7d's IR fixes (`dies`, 32).
* **Memory Jar** (`Effect:EachPlayerExilesHandDrawsNReturnsLater`) and **Headliner Scarlett**
  (`ExileTopCardsWithPlayPermission.face_down`): face-down exile is a hidden zone the engine
  doesn't have (section 10's knowledge, `determinize`, the view and the encoding all treat exile
  as public).
* **Rain of Filth** (`Effect:GrantAbilityToPermanentsYouControlUEOT`): "lands you control gain
  'Sacrifice this land: Add {B}'" grants an ability of another card, usable together with the
  land's own mana ability in one payment (tap it, then sacrifice it); section 18's payment takes
  one ability per card.

## 29. The single-card tail, part 2 (M7c)

Branch `m7c-tail2`, from main at 32e5225 (467 cards): the cards that needed two features each. 24
came through, 491 supported (184 creatures); six stay blocked (29.8). Code: `lower/effect_tail2.rs`
(lowering), `resolve_tail2.rs` (the new `Ext` instructions, cascade, miracle, the permission turn
bounds), changes in `cast.rs`, `activate.rs`, `pay.rs`, `triggers.rs`, `flow.rs`, `game.rs`;
tests `tests/cards_m7c_tail2.rs` (37 tests, the CR cited in each name).

### 29.1 The cards

| Card | What it needed | Decisions |
|---|---|---|
| Portal to Phyrexia | `SacrificeN`: each opponent chooses three creatures (a set, increasing slot order, always leaving enough; all of them, no decision, with three or fewer), sacrificed at once (701.21a); `FromGraveyardAs`: "put target creature card from a graveyard onto the battlefield under your control. It's a Phyrexian in addition" (the subtype applies as it enters, `Game::entering_mods`) | `Sacrifice`, `Target` |
| The Mind Stone | harness (`Mark::Harnessed`, `Filter::Harnessed`); its infinity trigger exists only once harnessed (no intervening "if": the trigger resolves without its source, 113.7a) | `Target` |
| Lurrus of the Dream-Den | Serra Paragon's permission (28.4) without lands (`CardDef::gy_play_land`) and without a granted trigger (`NO_GRANT`); companion not needed | - |
| Zirda, the Dawnwaker | `CardDef::ability_discount`: every non-mana ability its controller activates costs {2} less, at least one mana left (602.2b, 601.2f) | - |
| Thor, God of Thunder | `ExilePlayUntilNextTurn` (a permission "until the end of your next turn": `PERM_UNTIL_TURNS`, the player's turn count it ends with); `Amount::EventSpellMv` ("that spell's mana value", with its X on the stack, else as it last existed, 608.2h) | `Target` |
| Jacked Rabbit | ravenous (702.156a): the spell's X in the `Enters` note (bits 9-15), `TrigPred::EnteredXAtLeast`; tokens equal to its power (`CreateTokensN` with `SourcePower`) | X |
| Enduring Innocence | "this ability triggers only once each turn" (`TrigPred::Once`: once it triggers, `done_once` holds it for the turn; ir_defects 14); "return it ... It's an enchantment" (`ReturnEventCardAs`: `BecomeTypes` as it enters, its creature types gone, 205.3d) | - |
| Keen-Eyed Curator | the linked exile of a graveyard card; `Cond::LinkedTypesAtLeast` (card types among the cards exiled with the source, a static's condition, 607.2a) | `Target` |
| Currency Converter | `ExileEventCardLinked` ("you may exile that card from your graveyard"); `LinkedToGraveyardThenToken` (a linked card to its owner's graveyard, then a Treasure or a 2/2 Rogue) | `YesNo`, `ChooseExiled` |
| Broadside Bombardiers | boast (702.142a: `ActivatedDef::boast`, `Mark::Boasted`, "attacked this turn" from `attacked_with`); `Amount::CostMvPlus` (the sacrificed permanent's mana value, kept on the stack item as it is paid) | `Target`, `Sacrifice` |
| Yawgmoth's Will | `Player::gy_play_turn` (lands and spells from the graveyard this turn, a cast route for its mana cost); `Player::gy_exile_turn` (614.1a: a card, not a token, put into that player's graveyard goes to exile; it exiles the Will itself) | - |
| Bloodbraid Challenger | cascade (702.85a; keyword and trigger checked to agree): exiled cards revealed one at a time, the hit cast free (608.2g), the rest on the bottom in a random order after the cast | `YesNo` |
| Laelia, the Blade Reforged | `EventKind::ExiledFromLibGy` (a card into exile from its owner's library or graveyard), "one or more": once per event (since M7e, 603.2c) | - |
| Ocelot Pride | ascend (702.131: `Player::city_blessing`, set by the SBA check once a player with an ascend permanent controls ten permanents; `Cond::CityBlessing`); `CopyTokensEnteredThisTurn` | `Target` |
| Six | `MillThenLandToHand`; a permanent's retrace grant (`CardDef::grants_retrace` on a permanent: during its controller's turn) | `ChooseGraveyardCard`, `Discard` |
| Bilbo, Thief in the Night | `CardDef::outside_hand_discount` (601.2f: a card not in hand, or a spell cast from elsewhere); `MayCastFromGraveyardNow` (cast during the resolution paying its costs, 608.2g; an instant or sorcery with `CastThenExile`) | `ChooseGraveyardCard` |
| Multiversal Passage | "as this land enters, choose a basic land type" (`ChooseColor`: W U B R G = Plains ... Forest, `Mark::ChosenColor`), then the shock-land life question; the type as a forever effect on the object, its mana ability `ManaProd::Chosen` (305.6) | `ChooseColor`, `YesNo` |
| Fire Covenant | `CardDef::x_life`: X announced (601.2b, at most the life the mana payment leaves, 119.4), paid as life with the mana; X damage divided among any number of target creatures (at most X targets) | X, `Target`, divide |
| We Say Thee Nay! | teamwork n as kicker's modes (the cost option picks "if cast using teamwork"): crew's cost (702.122a), paid before the mana since M7e so creatures with mana abilities can be tapped (`crew_mana_ok`) | `ChooseCost`, `ChoosePermanent` |
| Fear of Missing Out | `Turn::extra_combats` (500.8: after this combat phase another one); `TrigPred::FirstAttack` (declared as an attacker once this turn) | `Target` |
| Pinnacle Emissary | warp (702.185a): an alternative cost from hand whose delayed ability (dash's plumbing, no haste) exiles the permanent at the next end step with a permission from a later turn on (`PERM_LATER`) | `ChooseAltCost` |
| Metamorphosis Fanatic | miracle (702.94a): on every first draw of a turn by a player whose deck has a miracle card, `Task::MiracleReveal` before the next priority (only "no" when the card has no miracle, so the question says nothing); revealed, its trigger (`MiracleCast`) may cast it for its miracle cost (`CastFrom::Miracle`) | `YesNo` x2 |
| Aang, Swift Savior | airbend: a creature or a spell exiled, its owner may cast it for {2} rather than its mana cost (`PermKind::Airbent`, `CastFrom::Airbent`); waterbend (`Cost::waterbend`, `PayMana::waterbend`: improvise with creatures too) | `Target`, `PayMana` |
| Candelabra of Tawnos | {X} in an activation cost (`ActivatedDef::x`; X announced, 602.2b, at most what can be paid and no more than the targets there are); exactly X targets (`X_TARGETS`) | X, `Target` |

### 29.2 Rules added on the way

* **Exile replacements before the events.** "If it would die, exile it instead" and "if it would
  leave the battlefield, exile it instead" were applied after the "dies" event fired (a creature
  exiled by Suplex triggered "when this dies"); they, and Yawgmoth's Will's, are now decided first.
* **Entering with effects** (`Game::entering_mods`): effects that apply to a permanent as it
  enters (614.1c-like), in place before its `Enters` event.
* **Permission turn bounds**: `Permission::turn` is exact, or `PERM_UNTIL_TURNS | k` ("until the end
  of your next turn": through the player's k-th turn), or `PERM_LATER | t` ("on a later turn").
* **A loop of optional actions** (732.2a): at most 31 activations of one permanent's non-mana
  ability a turn (`ACTIVATION_CAP`, as `MAY_COPY_CAP`): Zirda with Basalt Monolith makes unbounded
  mana, and random play never ended a game (the fuzzer).
* **Fast paths** (`Game::seen`): the tail's rare rules (ascend, the ability and spell discounts, a
  permanent's retrace grant, the exile watch, miracle per owner) are looked at only in games with
  such a card (set as objects are made; a copy copies a card of the game).

### 29.3 Decisions, tasks, the bot

No new decision kind (39 still). New task: `MiracleReveal` (asks `YesNo`; task code 16, as
`AsEnters`); every new `Ext` op is op code 37. The heuristic bot stays legal for every decision;
it pays X life for what the opponent's creatures need, at most half its life (Fire Covenant: it
would pay all of it), and announces X = 0 for an ability (Candelabra).

### 29.4 Encoding (layout v3, unchanged)

Nothing in `az/encode.rs` changed; the new cards fill their identities. New token kinds to
register: Cat (1/1 white, Ocelot Pride), Drone (1/1 colorless artifact creature, flying, blocks
only fliers; Pinnacle Emissary), Rabbit (1/1 white, Jacked Rabbit), Rogue (2/2 black, Currency
Converter); the Treasure is registered already. Not encoded (no spare floats used): harnessed,
boasted, the chosen basic land type (`marks`), the city's blessing, Yawgmoth's Will's turn
effects, additional combat phases, activation counts, a permission's kind and turn bound (airbent,
warp, until your next turn).

### 29.5 Hidden information and fuzzing

Cascade reveals each card as it is exiled (a public move keyed `KEY_REVEAL`, `force_library_card`
as for Demonic Consultation). Miracle's question is asked of a player on every first draw of a
turn if their deck has a miracle card (open decklists, section 10), with only "no" unless the
drawn card has miracle: whether it is asked depends on the deck, not the card; a revealed card
becomes known to both. Six mills face up; Laelia's exiles are public.

Fuzzing (20,000 games over the 491 cards): no failure (72 s). Faults found on the way, all fixed:
cascade's pending question, re-run by a refresh for the other player, acted instead of asking
again (the information-set check); Fire Covenant's X took life a painland's mana needed; teamwork's
taps and the mana payment both wanted a mana creature; Zirda with Basalt Monolith never ended a
random game (29.2).

### 29.6 Simplifications (differences from the CR)

What still applies (docs/shortcuts.md; M7e fixed Laelia's "one or more", the choice between
Lurrus and Serra Paragon, and teamwork's creatures and crew bonus):

* "After this phase, there is an additional combat phase" works during a combat phase (the only
  place the cube's card makes it).
* Miracle's reveal is asked before the next priority after the draw, not at the instant of the
  draw (nothing a player could do comes in between).
* At most 31 activations of one permanent's non-mana ability a turn (29.2).
* Thor's "until the end of your next turn" and warp's "a later turn" are counted in the player's
  turns and the turn number; extra turns are counted as turns.

### 29.7 IR

Two new defects (docs/ir_defects.md 16, 17): Forth Eorlingas! writes "one or more ... this turn"
as `once_per_turn`, which is "this ability triggers only once each turn" (it would not trigger in
a second combat damage step); Green Sun's Zenith writes "shuffle it into its owner's library" as
a replacement from anywhere (countered, discarded or milled it would go to the library). Both are
blocked (`lower::IR_DEFECTS`). Green Sun's Zenith's search is built and tested on a custom card
(`Test Green Sun`). Both fixed by mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca and supported (section 32; the custom card is
gone). The other 24 cards' IR matches their Oracle text; Laelia's placeholder event
and the doubled cascade are noted under "Checked and not defects".

### 29.8 Still blocked

* **Forth Eorlingas!**, **Green Sun's Zenith**: their IR (29.7; supported since M7d's IR fixes,
  32).
* **Abhorrent Oculus** (`Effect:ManifestDread`): manifest dread puts a card onto the battlefield
  face down as a 2/2 (708): face-down permanents need hidden identities on the battlefield (the
  determinization would resample permanents), characteristics overridden in every layer and
  turning face up; not built. Its "exile six cards from your graveyard" additional cost is not
  lowered either.
* **Shelldock Isle** (hideaway 702.75), **Memory Jar**, **Headliner Scarlett**: face-down exile is
  a hidden zone the engine doesn't have (section 10's knowledge, `determinize`, the view and the
  encoding all treat exile as public); a zone for it, with its clouds and play permissions, is the
  work they need together.
* Rain of Filth (28.9) and the cards with three or more blockers (part 3).

## 30. The single-card tail, part 3 (M7c)

Branch `m7c-tail3`, from main at 4613d0f (491 cards): the cards that needed three or more
features, and the face-down cards. 33 came through, 524 supported; 12 of the cards in scope stay
blocked (30.8). Code: `lower/effect_tail3.rs` (lowering), `resolve_tail3.rs` (the new `Ext`
instructions, splice, the face-down plays, the reanimating Auras), the face-down zone in
`ids.rs` / `game.rs` / `hidden.rs`, changes in `cast.rs`, `pay.rs`, `triggers.rs`, `combat.rs`,
`sba.rs`, `printing.rs`, `statics.rs`, `trigger.rs`; tests `tests/cards_m7c_tail3.rs` (48 tests,
the CR cited in each name). The work went shared features first (X life, reflexive triggers,
"from anywhere", draw replacements, kicker on permanents, splice), then the face-down zone, then
the reanimating Auras.

### 30.1 The cards

| Card | What it needed | Decisions |
|---|---|---|
| Vicious Rivalry | "destroy all ... with mana value X or less" (`Filter::MvAtMostX` on `DestroyAll`; X the life paid, 29.1's `x_life`) | X |
| Toxic Deluge | "all creatures get -X/-X" (`Ext::ApplyPtX`, X locked in, 608.2h) | X |
| Winds of Abandon | overload's "exile each": `Ext::ExileAll` with "for each creature exiled this way, its controller searches for a basic land" (up to that many, tapped, then a shuffle; APNAP) | `ChooseAltCost`, `SearchLibrary` |
| Mawloc | ravenous (29.1) and "if that creature would die this turn, exile it instead" (`Mod::ExileIfDies` on the fought target, whether or not the fight happened) | X, `Target` |
| Fastbond | "any number of lands" (the IR's sentinel 1,000,000 read as `ANY_LANDS`, ir_defects 10); "if it wasn't the first land you played this turn" (`Cond::LandsPlayedAtLeast`, an intervening "if") | - |
| Pest Infestation | "up to X targets" (`X_UP_TO`: the IR's `count_from_x` with its zero counts, read only in that exact shape on a spell with X, ir_defects 10); "twice X tokens" (`Amount::XTimes`) | X, `Target` |
| Generous Plunderer | "you may <do>. When you do, <reflexive>" (`MayDoThenReflexive`, 603.12); "target opponent creates a tapped Treasure" (`Ext::TokensFor`); damage to the defending player by the artifacts they control (`Amount::CountTypesOpp`) | `YesNo`, `Target` |
| Inti, Seneschal of the Sun | the same reflexive form with "you may discard a card"; "whenever you discard one or more cards" (the IR's `discards_card` as `CardDiscarded`, batched); "until your next end step" (`PERM_UNTIL_END_STEP`) | `Discard`, `Target` |
| The One Ring | "if you cast it" (the IR's `ConditionalIfCastFromHand`, read as its docstring's "if you cast it": `TrigPred::EnteredCast`); a player's protection from everything until their next turn (702.16j: `Player::prot_until`; not targetable, damage prevented); burden counters | - |
| Pentad Prism | sunburst (702.44a: the converge record, `CardDef::sunburst`, charge counters or +1/+1 on a creature); a mana ability that removes a counter and doesn't tap: activated with priority (28.1's route: it could be activated again within one payment) | `ChooseMana` |
| Worldspine Wurm, Emrakul, the Aeons Torn | "when this is put into a graveyard from anywhere" (`EventKind::ToGraveyard`: the card's own ability from the graveyard, its owner controls it, 603.6a); shuffling the source or the owner's graveyard in; Emrakul's annihilator (keyword and trigger checked to agree), protection from coloured spells (`Prot::colored_spells`, `DamageSource::spell`), the cast trigger's extra turn | `Sacrifice` (annihilator) |
| Blightsteel Colossus | infect (702.90b-c: poison counters, -1/-1 counters; `Kw::Infect`); "if it would be put into a graveyard from anywhere, reveal it and shuffle it into its owner's library instead" (`CardDef::gy_to_library`, applied in `move_card`; from a library it stays there; with other replacements in the order its controller chooses, 616.1, since M7e) | - |
| Hullbreacher | "if an opponent would draw a card except the first one they draw in each of their draw steps, instead you create a Treasure" (`CardDef::opp_draw_token`) | - |
| Quantum Riddler | "as long as you have one or fewer cards in hand, if you would draw one or more cards, you draw that many plus one" (`CardDef::draw_plus`, per draw instruction: `Game::draw_cards`); warp (29.1) | - |
| Batroc the Leaper | multikicker (702.33c: the repeatable cost, `TimesCost`); counters per kick; "up to X targets, X the times kicked" (`KICKS_UP_TO`, read from the enter note) | `ChooseMode`, `Target` |
| Sowing Mycospawn | kicker on a permanent (once), "if it was kicked" on two cast triggers (`TrigPred::SpellPaid`), devoid (702.114a); its kicker's {C} read from `kicker_cost_str` when the structured cost agrees otherwise (ir_defects 5) | `ChooseMode`, `SearchLibrary`, `Target` |
| Thieving Skydiver | kicker {X} with "X can't be 0" (`TimesCost::x`, `CardDef::x_min`: X announced after the kick, 601.2b); "target artifact with mana value X or less" (`Filter::MvAtMostSourceX`, `Mark::KickerX`; the IR has no target, ir_defects 6: the Oracle text's); gain control, attach if an Equipment | `ChooseMode` x2, `Target` |
| Goryo's Vengeance, Through the Breach | splice onto Arcane (702.47, 30.3); Goryo's haste and end-step exile | `ChooseHandCard`, `Target` |
| "Name Sticker" Goblin | "enters from anywhere other than a graveyard or exile" (enter note bit 15); "creatures named ..." (`Filter::Named`); the die roll's gate checked on resolution | - |
| Parallax Wave | fading (702.32a); exile linked to the source; "each player returns ... all cards they own exiled with it" | `Target` |
| Caustic Bronco | saddle (702.171a: crew's cost, other creatures, as a sorcery; `Mark::Saddled` until end of turn); "if this creature isn't saddled" on resolution | `ChoosePermanent` |
| Sword of the Meek | a triggered ability that functions in the graveyard (`TrigPred::InGraveyard`, `CardDef::gy_triggers`, `SEEN_GY_TRIGGER`; its owner controls it); "a 1/1 creature" (`Filter::PtIs`) | `YesNo` |
| Gollum, Riddle Master | odd or even chosen as it enters (`Mark::Parity`); "a spell with mana value of the chosen quality"; "choose one that hasn't been chosen" (`Modes::once_each`, `Mark::ModesUsed`; with none left the trigger is removed, 700.2b) | `ChooseMode` |
| Emperor of Bones | adapt (701.46a); "whenever one or more +1/+1 counters are put on this" (`EventKind::CountersAdded`); a finality counter (122.1i: exiled instead of put into a graveyard, `CardDb::finality`); a linked creature card reanimated with haste and an end-step sacrifice | `Target`, `ChooseExiled` |
| Wan Shi Tong, All-Knowing | "its owner puts it second from the top or on the bottom" (the owner's `YesNo`); "whenever one or more cards are put into a library from anywhere" (`EventKind::CardsPutIntoLibrary`, once per event since M7e); Spirits that block and are blocked only by Spirits (`CardDef::blocks_only_subtype`) | `Target`, `YesNo` |
| Coveted Jewel | "whenever one or more creatures an opponent controls attack you and aren't blocked" (`EventKind::AttackersUnblocked`, `Slot::Defender`); that player draws three, gains control of it, untaps it | - |
| Headliner Scarlett, Memory Jar, Shelldock Isle | cards exiled face down (30.2) | `PutInHand` (hideaway), `YesNo` (Shelldock) |
| Animate Dead, Necromancy | reanimating Auras (30.4) | `Target` |

### 30.2 Face-down exile

`Zone::FaceDown` is the exile zone for cards exiled face down (CR 406.3): a hidden zone like a
library (`Zone::is_hidden`), one list (`Game::face_down`, both players', in exile order). The
knowledge rules of section 10 carry over: a card moved there keeps its `known` bits (hidden to
hidden), and its owner, who saw it go, knows it (Scarlett's look is keyed `KEY_LOOK`, private;
Memory Jar's cards come from the hand; hideaway's from a looked-at pile). A cloud whose domain
held the card keeps it by its slot (`knowledge_moves`), so "that card is among these" survives
the move and the return to the hand. `determinize` deals the face-down cards a viewer doesn't
know again with the owner's other hidden cards (`unknown_to_in`), never for their owner. The
view (`view_of`) shows each owner's face-down pile as a count and the identities the viewer
knows, without order (the fuzzer found that the order of a hand exiled face down told which
known card was where). The encoding puts a face-down card in `MY_EXILE` / `OPP_EXILE` by
identity only for a viewer who knows it, and the opponent's unknown ones in `OPP_HIDDEN` (the
multiset of identities the viewer doesn't know, the same in every world it samples); the exile
count of the globals includes them. Nothing in `az/encode.rs`'s layout changed.

Playing a face-down card: a permission (Scarlett's "this turn") lets its owner play it with the
`EXILE` slots, after the face-up cards; the play reveals it (hidden to public). Shelldock Isle's
card is linked to it (607.2a), hideaway's pile picks it with a `PutInHand` decision (a card
taken; no new decision kind), and its ability asks the `YesNo` whenever the card is there and a
library has twenty or fewer cards ("yes" only if it can be played now: a land on its
controller's turn with a land play left; a spell castable without paying its mana cost), so the
question says nothing about the hidden card. Memory Jar links each player's exiled hand to the
Jar; the delayed ability discards both hands and returns the linked cards.

Face-down permanents (Abhorrent Oculus's manifest dread, CR 708) are not built: a hidden identity
on the battlefield would need `determinize` over permanents, 2/2 colourless nameless
characteristics in every layer accessor and the encoding, and turning face up; they would cost
more than this whole part (30.8).

### 30.3 Splice onto Arcane (CR 702.47)

Casting a spell with a subtype that some card of its caster's deck splices onto asks
`Task::Splice` (task code 23, as `ChooseTimes`) after the other announcements and before the
targets: one `ChooseHandCard` decision per spliced card (a hand slot, `PASS` = no more), asked
whenever the deck has such a card (open decklists: the question says nothing about the hand). A
card is offered while a copy of it isn't spliced yet, its splice cost can be paid with the rest
(`extra_costs` adds it, 601.2f) and its targets exist. The spliced card is revealed and stays in
the hand; `StackItem::spliced` records its definition. Its targets follow the spell's own
(`resolve_tail3::item_reqs`, used by target choice, resolution and copies), and as the spell
resolves its program runs after the spell's, with the spell as its source and the spliced card's
definition as the frame's (its delayed abilities are its own: Goryo's end-step exile).

### 30.4 Reanimating Auras

Animate Dead is an Aura spell whose target is a creature card in a graveyard (303.4a: the IR's
`aura_enchant_type`, `CardDef::aura_spell`); with an illegal target it doesn't resolve (608.3a),
else it enters attached to that card (608.3c, as a bestowed Aura). Its enter trigger returns the
card it enchants under its controller's control and attaches to it (`Ext::ReanimateAttach`,
`Mark::Reanimated`); Necromancy's targets a creature card in a graveyard as it triggers and first
becomes an Aura (layer 4). Both check "if it's on the battlefield" as they resolve. A reanimating
Aura that is an Aura is legal attached to the creature card it entered on (in a graveyard) or to
the creature it put onto the battlefield; anything else, or nothing, puts it into its owner's
graveyard (704.5m, `sba.rs::bad_reanimating_aura`). Leaving, it makes that creature's controller
sacrifice it. Animate Dead's -1/-0 is the IR's `equipment_bonuses` on an Aura (ir_defects 7: the
same `attached` applies it). Necromancy's "as though it had flash" is a permission since M7e
(`CardDef::flash_permission`; the IR writes the flash keyword, ir_defects 8); cast when a sorcery couldn't have been (not its controller's
main phase with an empty stack, `Mark::NotSorceryTiming`), it is sacrificed at the beginning of
the next cleanup step (`DelayedWhen::NextCleanup`, fired as the cleanup step begins: 514.3a gives
priority and another cleanup).

### 30.5 Rules added and fixed on the way

* `Lowerer::spell_x`: the printing's spell has an X (mana cost or "pay X life"); "X" effects
  check it.
* The enter note's X is bits 9-14 (6 bits: X is at most 31 here); bit 15 says the permanent came
  from a graveyard or exile.
* Kicker on a permanent is a repeatable additional cost (`TimesCost`, once; multikicker any
  number of times; kicker {X} with `x`), so the enter note's "times" and the spell's are "if it
  was kicked" (`EnteredPaid`, `SpellPaid`).
* `trig_mask` is a `u64` (33 event kinds); `Game::seen` a `u16` (`SEEN_DRAW_REPL`,
  `SEEN_GY_TRIGGER`, `SEEN_RARE_EVENTS`: the draw replacements, graveyard triggers and the three
  new events are looked at only in games with such a card).
* Fixes the fuzzer found in older code, exposed by the new pool's decks: a permission an effect
  gives a graveyard card ("you may cast it now", Bilbo, Malcolm) is its route before its
  flashback (`cast_route`; the effect offered it for its mana cost); more waiting triggers than
  `TRIGGER` slots (a board of tokens dying) put the first of each interchangeable group first;
  tokens are capped so every card a player owns can still enter (a board of tokens, then a
  spell, overran the 96 permanent slots); a search for several cards (Nissa's, Ugin's) picks
  definitions, taking the cards at its end (`Frame::picked_defs`: the searcher's own worlds may
  deal the library again in between); mana classes of one definition stay apart when their
  additional mana differs (an animated Urza's Saga with Badgermole Cub is a creature, its twin
  isn't); a card surveiled into a graveyard that Blightsteel shuffles in instead is not known at
  its new place.

### 30.6 Decisions, tasks, encoding, the bot

No new decision kind (39 still). New task: `Splice` (task code 23). New contexts for existing
kinds: hideaway's pile pick (`PutInHand`), the splice choice (`ChooseHandCard`), the parity
(`ChooseMode`), kicker's times and X (`ChooseMode`), the owner's "second from the top" and
Shelldock's play (`YesNo`). Every new `Ext` op is op code 37. Layout v3 is unchanged; not encoded
(no spare floats used): a player's protection from everything, splices, saddled, the modes
Gollum chose and its parity, a kicker's X, a reanimating Aura's creature, the new named counters
(burden, fade, charge, finality: the layout encodes loyalty and lore counters, +1/+1 and -1/-1
through power and toughness), `Kw::Infect` (not in the layout's keyword list: a card's printed
keywords come with its identity). New token kinds to register: Pest (1/1 black and
green, "when this dies, you gain 1 life"), Wurm (5/5 green, trample), Spirit (1/1 colourless,
blocks and is blocked only by Spirits); the Treasure is registered already.

The heuristic bot stays legal for every decision (the long test: 4,000 games, all 39 kinds). "Pay
X life" is chosen by what the spell does: the toughest opposing creature for -X/-X (Toxic Deluge),
the costliest opposing artifact or creature for "mana value X or less" (Vicious Rivalry), the
toughness together for divided damage (Fire Covenant), at most half its life.

### 30.7 Simplifications (differences from the CR)

What still applies (docs/shortcuts.md; M7e fixed Blightsteel's replacement order, the "one or
more" library and discard triggers, saddle's crew bonus and Necromancy's flash):

* Shelldock Isle plays a land only on its controller's turn with a land play left.
* "Until your next end step" ends as that end step begins, and "you may play that card" is
  checked by the controller's turn count, extra turns included.
* Splice offers each spliced card once per copy in hand, never the spell itself; splicing a card
  with a "distinct targets" requirement is not modelled (none in the cube).
* A reanimating Aura's attachment legality doesn't model "enchant creature put onto the
  battlefield with this Aura" beyond the one creature it reanimated.

### 30.8 Still blocked

* **Abhorrent Oculus**: face-down permanents (manifest dread, CR 708, 701.62), 30.2; its "exile
  six cards from your graveyard" additional cost is not lowered either.
* **Urza, Lord High Artificer** (`ActivationCost.tap_permanent_type`): a mana ability whose cost
  taps another permanent (section 18's payment taps sources, not costs of sources), and
  "shuffle, exile the top card, play it free".
* **Bolas's Citadel** (`grants_look_at_top_of_library`, `grants_play_from_top_paying_life`,
  ten nonland permanents sacrificed): a top card known to its owner at all times and plays from
  the library paying life.
* **Dauthi Voidwalker** (shadow, `void_exile_replacement`): opponents' cards exiled with void
  counters instead of going to a graveyard, and a free play of one of them.
* **Lion Sash** (reconfigure, 702.151; an Equipment bonus per own counter).
* **Does Machines** (Class levels, 716).
* **Nadu, Winged Wisdom** (a granted "becomes the target" trigger limited to twice a turn per
  creature).
* **Anointed Peacekeeper** (a card name chosen as it enters, taxes by name).
* **Gemstone Caverns** (an opening-hand action, 103.6).
* **Pillage the Bog** (plot, 702.170: a special action from hand).
* **Arena of Glory** (exert as a cost; mana that gives haste to the creature spell it is spent
  on).
* **Crabomination** (emerge, 702.119; a random card from a graveyard and from a hand exiled, one
  cast free).
* **Wizard's Rockets**: an IR defect (30.9).
* Not in scope: Ajani, Celestial Colonnade, Creeping Tar Pit, Minsc & Boo, Strip Mine, Chromatic
  Star, Forth Eorlingas!, Green Sun's Zenith (IR defects), Rain of Filth (28.9).
* (All supported since: M7d, section 31; the IR defects, section 32.)

### 30.9 IR

One new defect (docs/ir_defects.md 18): Wizard's Rockets's "when this artifact is put into a
graveyard from the battlefield" is `leaves_battlefield` in the IR, as Chromatic Star's (15);
blocked (`lower::IR_DEFECTS`). Handled faithfully with consistency checks: Fastbond's and Pest
Infestation's sentinels (10), Sowing Mycospawn's {C} (5, from the string), Thieving Skydiver's
and Wan Shi Tong's missing targets (6, the Oracle text's), Animate Dead's Aura bonus (7),
Necromancy's flash (8). The other cards' IR matches their Oracle text; notes under "Checked and
not defects".

### 30.10 Measurements and fuzzing

Fuzzing (20,000 games over the 524 cards): no failure (69-70 s); the faults it found on the way are
30.5's and the face-down view's (30.2). Speed (pinned to logical CPU 2, `tests/perf.rs`,
interleaved with main's code 4613d0f in four rounds, a training run using the GPU and about eight
threads): on main's pool (`POOL_EXCLUDE` leaves the 33 new cards out) 5.09-5.42 us per decision
against main's 5.25-5.36 (874-930 games/s against 882-901; pairs -5.0% to +2.7%), 845,436 decisions
against 845,409 (a few games differ by 30.5's fixes); `Game::clone` 1.18-1.25 us against 1.16-1.25,
`size_of::<Game>()` 1,368 B (+24: the face-down list); clone + determinize 1.55-1.82 us against
1.51-1.69. On the full pool: 5.59-5.67 us per decision (832,306 decisions), 848-860 games/s;
`Game::clone` 1.30-1.32 us; clone + determinize 1.72-1.90 us.

## 31. The last cards that needed subsystems (M7d)

Branch `m7d-last`, from main at 0577e06 (524 cards): the 13 cube cards left that each needed an
engine subsystem. All 13 came through: 537 supported (208 creatures by their front faces); the 9
cards still blocked are IR defects (31.9). Code: `lower/effect_m7d.rs` (lowering),
`resolve_m7d.rs` (the `Ext::M7d` instructions, special actions, face-down permanents, the opening-
hand action, emerge, the Citadel's top card, Rain of Filth's sources), changes in `pay.rs`,
`hidden.rs`, `game.rs`, `cast.rs`, `activate.rs`, `flow.rs`, `layers.rs`, `triggers.rs`,
`lower/printing.rs`, `lower/activated.rs`, `lower/statics.rs`, `lower/trigger.rs`,
`az/encode.rs`, `az/bots.rs`; tests `tests/cards_m7d.rs` (26 tests, the CR cited in each name) and
the custom card Test Manifester ("when this enters, manifest dread": the fuzzer meets face-down
permanents in many games). The work went from the cheapest card to the largest: plot, the
opening-hand action, shadow and the void replacement, reconfigure, Class levels, Nadu, the chosen
name, Urza, exert, emerge, the Citadel, Rain of Filth, face-down permanents.

### 31.1 The cards

| Card | What it needed | Decisions |
|---|---|---|
| Pillage the Bog | plot (702.170a): a special action from hand (116.2k), offered as the card's hand ability (`ActivatedDef::special`, `HAND_ABILITY` id; no stack: `Task::Special` after its mana); exiled with a permission to cast it without paying its mana cost on a later turn (`PERM_LATER`; a sorcery's timing, the lowering checks it is one); "twice the number of lands you control" (`Formula:Scaled`, `Amount::CountTypesTimes`) | `PayMana`, `PutInHand` |
| Gemstone Caverns | an opening-hand action (103.6): `Task::Opening` after the mulligans, the starting player first; asked whenever the player's deck has such a card (open decklists), `ChooseHandCard` with `PASS`; then the luck counter and `ExileCost` for the card exiled from hand; `ManaProd::IfCounter` ({C}, or any colour with a luck counter, as the ability resolves; `produces_with_counter` checked against it) | `ChooseHandCard`, `ExileCost` |
| Dauthi Voidwalker | shadow (702.28b, `Kw::Shadow`); "if a card would be put into an opponent's graveyard from anywhere, instead exile it with a void counter on it" (614.1a in `move_card`: a card, not a token or a copy of a spell; decided before the events, so it doesn't die; with Yawgmoth's Will's or flashback's in the order the card's controller chooses since M7e, 616.1; a Voidwalker leaving in the same simultaneous move still applies; `SEEN_VOID`); `M7d::ExiledWithCounterPlayFree` (a free play permission this turn) | `ChooseExiled` |
| Lion Sash | reconfigure (702.151): `CardDef::reconfigure` (attached, not a creature: `types_of`, 702.151b; it may equip while a creature, 301.5c), the two abilities checked against `reconfigure_cost`, unattach only while attached (`Cond::SourceAttached`; `activation_ready` now gives "activate only if" its source); `Mod::PtPerSourceCounter`; `M7d::ExileTargetCounterIfPermanent` | `Target` |
| Does Machines | Class levels (716): each level's printing is a face (`Face::Level`, `CardDef::levels`, `class_level`) with the Class's abilities at that level; gaining a level turns that face up (`set_face`: the same object, as a transform); `EventKind::ClassLevel`, `TrigPred::Level`; `M7d::BecomeCreatureIfNot` (a 0/0 Robot creature in addition, forever, if it isn't one) | `Target` |
| Nadu, Winged Wisdom | a granted "whenever this creature becomes the target of a spell or ability" triggered ability: the granter's definition's trigger (its source the creature since M7e, 113.7) for its controller's creatures that have the granted abilities (`TrigPred::Granted`, as granted ward, 27.3); "this ability triggers only twice each turn", per creature (`TrigPred::TimesPerSubject`, `Mark::Triggered`); `M7d::RevealTopLandElseHand` | - |
| Anointed Peacekeeper | "as this enters, look at an opponent's hand, then choose any card name" (`AsEnters`: the look once, `Mark::LookedHand`, then `NameCard` over the names of the opponent's cards, open decklists: the same in every world; `PASS` = a name of no such card; `Mark::ChosenName`); "spells your opponents cast with the chosen name cost {2} more" (`cost_with`, 601.2f) and "activated abilities of sources with the chosen name cost {2} more to activate unless they're mana abilities" (`act_mana_x`, anyone's, with the increases; a special action isn't an ability); `SEEN_NAMED_TAX` | `NameCard` |
| Urza, Lord High Artificer | a mana ability whose cost taps another permanent ("tap an untapped artifact you control: add {U}"; the IR's `tap_permanent_you_control`): each untapped artifact is a source of {U} while paying (`pay::TAP_FOR`, its `MY_PERM` id; 31.4); `M7d::ShuffleExileTopPlayFree` (a free play permission this turn); the Construct token (registered) | `PayMana` |
| Arena of Glory | exert as a cost (701.39: `Cost::exert`, `Mark::Exerted`; its controller's next untap step skips it); mana that gives haste (`ManaAbility::haste`, `ManaPool::haste`: its {R}{R}, made with priority (28.1), is spent first on a creature spell and last on anything else; `Mark::HasteMana` gives the permanent haste until end of turn) | - |
| Crabomination | emerge (702.119): `AltCost::emerge`; `Task::Emerge` chooses the artifact before the cost is determined (601.2f; `StackItem::noted`), the cost is reduced by its mana value and leaves out a source that sacrifices it, and it is sacrificed after the mana (601.2h); `M7d::OppExilesThreeMayCastOne`: the top card (keyed, forced), a random graveyard card (a public chance key), a random hand card (the forced hand pick of a random discard), then a free cast of one of them (608.2g) | `ChooseAltCost`, `Sacrifice`, `ChooseExiled` (`PASS` = none) |
| Rain of Filth | `Mod::GainSacMana` on the lands its controller controls (locked in, 611.2c); `pay::SAC_FOR`: the granted ability used together with the land's own (31.4) | `PayMana`, `ChooseMana` |
| Bolas's Citadel | "you may look at the top card of your library any time" (`Game::look_tops` before every decision: a private look, keyed and forced, `SEEN_TOP`); "play lands and cast spells from the top of your library ... pay life equal to its mana value rather than pay its mana cost" (`CastFrom::LibraryTop`: an alternative cost, X 0, cost increases still apply; `LIBRARY` + 0 at priority); "sacrifice ten nonland permanents" (the IR range raised to 10) | `Sacrifice` x10 |
| Abhorrent Oculus | face-down permanents (31.2) and manifest dread (701.62a: `M7d::ManifestDread`, a look at the top two, the face-down pick, the other into the graveyard; one card left: that one); "as an additional cost, exile six cards from your graveyard" (a mandatory additional cost, the only one allowed on a permanent card) | `PutOntoBattlefield`, `ChooseGraveyardCard` |

### 31.2 Face-down permanents (CR 708)

A face-down permanent's slot has the database's face-down definition (`CardDb::face_down_def`,
appended after every card, token and face): a 2/2 creature with no name, colour, mana cost,
subtypes or abilities (708.2a). Every rule and every layer reads it like any definition: its
power and toughness with counters and pumps, combat, targeting, copies of it (707.2: a 2/2 with
no name), "a creature entered", the legend rule (no name), Containment Priest's replacement (it
isn't cast). The card it is waits in `Game::face_down_ids` (slot, definition).

Its identity is hidden like a library card's (section 10): `Game::hidden_obj` (a hidden zone or a
face-down permanent), `knows` (its `Obj::known` bits keep their meaning on the battlefield),
`ident` / `set_ident` (the card it is; what knowledge and `determinize` read and write). Entering
face down is a move between hidden places (`knowledge_moves`: a cloud whose domain held the card
keeps it by its slot, as for face-down exile, 30.2; no reveal key); `learn`, `reveal`, `scramble`,
`tidy_clouds`, `domain` and `check_knowledge` treat the slot as hidden; `determinize` deals a
face-down permanent the viewer doesn't know again with its owner's other hidden cards
(`unknown_to_in`), never for a player who knows it. Its controller knows it (708.5: the look of
manifest dread, and `change_control` gives the new controller a private look). The view
(`view_of`) adds, after the permanent, the card it is for a viewer who knows it; the encoding
gives the permanent's card code the card's identity for a viewer who knows it and the face-down
definition's (`UNKNOWN`, a 2/2 creature) otherwise, and puts the opponent's unknown face-down
permanents in `OPP_HIDDEN` (`perm_identity`). Nothing in layout v3 changed.

Turning it face up (708.7, 701.40b: a manifested creature card, for its mana cost; X 0) is a
special action of its controller any time they have priority: the face-down definition's one
activated entry (`Special::TurnFaceUp`, `ACTIVATE` + slot * 4), offered when the cost can be paid
(only its controller knows whether, so only its controller's own decisions depend on it). It is
the same permanent (708.8: `set_face`, counters, damage and effects stay; no enter abilities); its
identity becomes public (`knowledge_leaves`, a public reveal key). Leaving the battlefield (708.9)
it leaves as a face-down 2/2 (its leave events and last known information see that, so its card's
"when this dies" doesn't trigger), then is revealed and is its card in the new zone
(`move_card`, `Game::unface`, with the knowledge bits it had: the fuzzer found the first version
reading them after the move had reset them).

### 31.3 Special actions (CR 116.2)

`ActivatedDef::special` marks a special action the engine offers as an activated ability: plot
(a hand ability) and turning a face-down permanent face up (an ability of the face-down
definition). `begin_activate` pays its mana (`special_mana`: plot's cost, or the face-down card's
mana cost) and then `Task::Special` does it at once, without the stack; the player keeps
priority. It isn't an ability: Zirda's and Anointed Peacekeeper's ability costs don't apply,
"activated abilities can't be activated" locks don't stop turning a card face up, and it isn't
counted against the 31-activations cap of 29.2.

### 31.4 Payments: another permanent's tap, two abilities of one land

Section 18's payment lets each card contribute one way (its mana abilities tap, sacrifice or
exile it, so they are alternatives). Two kinds of sources join it:

* **Urza** (`pay::TAP_FOR`): each untapped artifact its controller controls is a source of {U}
  while paying, picked by its `MY_PERM` id; activating it taps the artifact (not "tapped for
  mana", 106.12a: no Badgermole bonus). An artifact with its own mana ability has both as
  alternatives (both tap it); improvise doesn't offer an artifact Urza can tap (the same tap, and
  {U} pays for {1} too). `has_mana_ability` counts an artifact Urza can tap, so the costs that tap
  creatures besides the mana (teamwork, "tap an untapped <type>") never compete with the payment.
* **Rain of Filth** (`pay::SAC_FOR`): the question was whether one source may contribute two
  abilities whose costs don't conflict. It may (601.2g lets a player tap a land for mana and then
  sacrifice it for {B} in one payment), and the one-way-per-card rule stays: the land's ways are
  its own {T} ability, the granted sacrifice alone, or both, and "both" is one more way of the
  card. The granted ability is one class per land: a several-mana class whose units are its own
  {T} mana and the sacrifice's {B} (its productions: {B}, or one of its {T} mana plus {B}; the
  excess stays in the pool as for any several-mana source), or for a tapped land a one-mana class;
  its id is `ACTIVATE` + slot * 4 + the land's own number of abilities. Activating it activates the
  {T} ability first, then sacrifices the land. Only the land's {T} abilities of one mana without a
  life, damage or spending restriction are combined (31.7). (M7e part 2, section 34: the granted
  sacrifice is a one-mana source of its own, `Class::res` = sacrifice, combined with any tap
  ability of the land, conditioned ones too; Squandered Resources and Orcish Lumberjack the same
  way.)

The feasibility search (`cands`) branched on every card with several ways; a board of Rain lands
made it exponential (seconds per decision, found by the fuzzer's timing). A card whose ways use no
budget and one of which makes every other's units is now that way's units alone (more units never
make a cost unpayable), so the search doesn't branch on it.

### 31.5 Rules added and fixed on the way

* **An activated ability's X counted its own source** (`choose_x`): with Urza, the Candelabra of
  Tawnos (an artifact) could be tapped for {U} while its own {T} was the cost; the X is now
  computed without the source when the cost taps it (the option-completion check found it, seed
  1012).
* **More distinct waiting triggers than `TRIGGER` slots** (seventeen "whenever you discard" triggers
  as a player discarded at cleanup; a crash in random play, found by the speed measurement): the
  first sixteen are offered, the others come next.
* `activation_ready` evaluates "activate only if" with its source (`cond_holds_from`).
* An additional cost on a permanent card is allowed for the graveyard exile (Oculus).
* The `Game::marks` facts added: `Triggered`, `LookedHand`, `ChosenName`, `Exerted`, `HasteMana`.
* `Game::seen` bits added: `SEEN_VOID`, `SEEN_NAMED_TAX`, `SEEN_TOP`, `SEEN_SAC_MANA` (the new
  rules are looked at only in games with such a card).

### 31.6 Decisions, tasks, encoding, the bot

No new decision kind (39 still). New contexts for existing kinds: the opening-hand action
(`ChooseHandCard` with `PASS`, then `ExileCost`), emerge's artifact (`Sacrifice`, before the
mana), Dauthi's card (`ChooseExiled`) and Crabomination's free spell (`ChooseExiled` with `PASS`),
the chosen name (`NameCard`), manifest dread's pick (`PutOntoBattlefield`), Rain's production
(`ChooseMana`). New action meanings: plot is the card's `HAND_ABILITY` id; the Citadel's top card
is `LIBRARY` + 0 at priority; turning a face-down permanent face up is its `ACTIVATE` + 0; Rain's
ability `ACTIVATE` + its number of abilities; an artifact Urza taps its `MY_PERM` id. New tasks:
`Special` (asks nothing), `Opening` (task code 16, as `AsEnters`), `Emerge` (12, as `PayCosts`);
every new instruction is `Ext::M7d` (op code 37).

Layout v3 is unchanged (31.2 for face-down permanents). Not encoded (no spare floats used): a
pool's haste mana (the pool's {R} count includes it), exerted, a Class's level (its face's
descriptor shows a face other than the front is up), the chosen name, a plotted card's permission
(it is in exile), the void counters (named counters), Nadu's trigger counts, Rain's granted
ability (its `ACTIVATE` option is encoded while paying). New token kinds to register: none (Urza's
Construct, `Construct 0/0 C ac Construct`, is registered).

The heuristic bot stays legal for every decision (the long test: 4,000 games, all 39 kinds). It
plots a card it doesn't cast in its second main phase, turns a face-down creature face up in its
main phase when the card is bigger than a 2/2, gains Class levels in its main phases, reconfigures
Lion Sash only with two or more counters on it, begins the game with Gemstone Caverns and exiles
its worst card, names the opponent's costliest card for Anointed Peacekeeper, casts the costliest
of Crabomination's cards, and casts from the top of its library with the Citadel only below half
its life. Measured (`strength_against_the_old_policy_and_random`, 2,000 games each): heuristic vs
random 94.8% +- 0.5, vs `HeuristicV1` 61.6% +- 0.8.

### 31.7 Simplifications (differences from the CR)

What still applies (docs/shortcuts.md; M7e fixed the Citadel's faces, turning face up after a
loss of all abilities and Dauthi Voidwalker's replacement order):

* (Arena of Glory's mana while paying, the player choosing which {R} pays a creature spell, and
  Rain of Filth with a land's conditioned {T} abilities: M7e part 2, section 34.) Rain of
  Filth's granted ability is used while paying only (docs/shortcuts.md 13.12); a land with four
  abilities of its own doesn't get its id (none in the cube).
* Emerge's artifact is chosen right after the alternative cost, before modes, X and targets (the CR
  determines it with the total cost, 601.2f; the cube's emerge card has no spell targets).
* Bolas's Citadel's top card is known before every decision, not between two of them.
* Anointed Peacekeeper names among its opponent's cards (any card name in the CR: a name of its
  controller's own cards would tax only that player's own abilities).
* Nadu's limit is counted per creature and granting definition (two Nadus would share one count;
  each grants its own ability in the CR; Nadu is legendary).
* A copy of a Class doesn't gain levels; morph, disguise and cloak are not built; a face-down
  spell can't be cast.
* More distinct waiting triggers than 16 offer the first 16 for the next (31.5).

### 31.8 Hidden information and fuzzing

Face-down permanents (31.2): hidden from the opponent in every world it samples, never resampled
for their controller (tests: the view, the encoding and the identity over 40 worlds of each
player). The Citadel's top card is known to its controller only (a test over 30 worlds). Anointed
Peacekeeper's look is private (`KEY_LOOK` per card); Crabomination's random graveyard card is a
public chance key and its random hand card the keyed reveal of a random discard; manifest dread's
look is private and the card put into the graveyard is revealed. The fuzzer's known-card check
also compares `ident`.

Fuzzing (20,000 games over the 537 cards and the custom cards with Test Manifester): no failure
(131-137 s; 5,624,843 options completed, 6.3 million decisions after them). Faults found on the way, all fixed: the face-down knowledge bits read after the move
(a cloud lost its card; seed 2150); an activated ability's X counting its source (seed 1012);
Rain of Filth's exponential feasibility (one game took 31 s: seed 1784); seventeen waiting
triggers (31.5). Behaviour digests re-recorded for the new pool (section 15: 5cbbaf9280a01797).

### 31.9 IR

No new defect. All 13 cards' IR matches their Oracle text; representational notes in
docs/ir_defects.md ("Checked and not defects"). Still blocked, all by IR defects (`lower::IR_DEFECTS`,
not in scope): Ajani, Nacatl Pariah; Celestial Colonnade; Creeping Tar Pit; Minsc & Boo; Strip
Mine; Chromatic Star; Forth Eorlingas!; Green Sun's Zenith; Wizard's Rockets. All nine supported
with the IR of mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (section 32).

### 31.10 Measurements

Speed (pinned to logical CPU 2, `tests/perf.rs`, interleaved with main's code 0577e06 in three
rounds, a training run using the GPU and about eight threads on the machine): on main's pool
(`POOL_EXCLUDE` leaves the 13 new cards and Test Manifester out: the same games, 832,306 decisions
both) 5.53-5.60 us per decision against main's 5.42-5.49 (858-870 games/s against 875-887; pairs
+0.9% to +3.3%); `Game::clone` 1.24-1.27 us against 1.27; `size_of::<Game>()` 1,392 B (+24: the
face-down identities); clone + determinize 1.77-1.96 us against 1.66-1.72. On the full pool:
5.56-5.70 us per decision (838,909 decisions), 836-857 games/s; `Game::clone` 1.28-1.31 us; clone +
determinize 1.65-1.79 us. The new rules are looked at only in games with such a card (31.5's
`SEEN_*` bits); the knowledge accessors check the face-down list's emptiness first.

## 32. The IR fixes: the last nine cards (M7d)

Branch `m7d-ir`, from main at 6d6760c (537 cards). The cube's IR is now the export of mtg-bot
`feature/ir-fixes-mtgzero` 8ea9fcca (schema `66ba3e33fe5b`; two new classes, `DoThenReflexive`
and `ShuffleThisSpellIntoOwnersLibrary`; docs/card_ir.md), which fixes docs/ir_defects.md 1, 2,
4, 12, 13 and 15-18. All 546 cube cards are supported (209 creatures by their front faces, 20
planeswalkers); `lower::IR_DEFECTS` is empty. Code: `lower/effect_m7d.rs` (the new effect classes,
`check_self_shuffle`), `lower/activated.rs` (X mana), `resolve_m7d.rs`, `activate.rs`
(`mana_now`), `combat.rs`, `triggers.rs`.

### 32.1 The cards

| Card | What it needed | Decisions |
|---|---|---|
| Strip Mine | "{T}: Add {C}" is the IR's `produces: [C]` with `mana_only_from_produces` (19); its "destroy target land" a basic one too | `Target` |
| Celestial Colonnade, Creeping Tar Pit | the animation's colours and Elemental (`set_colors`, `granted_subtype`, already lowered for Restless Vinestalk: layers 4 and 5); Tar Pit's "can't be blocked this turn" | - |
| Minsc & Boo, Timeless Heroes | Boo is red (its colour indicator, CR 204): a new token definition | as before (24) |
| Ajani, Nacatl Pariah // Ajani, Nacatl Avenger | the 0's `DoThenReflexive`: the Cat, then a reflexive triggered ability (603.12) whose intervening "if" (603.4) is checked as it triggers and as it resolves, for its source ("other than Ajani") | `Target` as it is put on the stack |
| Chromatic Star | "put into a graveyard from the battlefield" is the `dies` event of an artifact (603.10a: it looks back) | `PayMana`, `ChooseMana` |
| Wizard's Rockets | the same event; "{X}, {T}, Sacrifice this artifact: Add X mana in any combination of colors" (107.3, 605.3a) | `ChooseMode` (X), `PayMana`, a `ChooseMana` per mana |
| Forth Eorlingas! | "whenever ... this turn" (`CreateThisTurnTriggeredAbility`): a delayed triggered ability with a duration (603.7b); "one or more creatures ... deal combat damage" once per combat damage step | `ChooseMode` (X) |
| Green Sun's Zenith | its last instruction moves the resolving spell into its owner's library and shuffles it (608.2c; then 608.2n has nothing to put into a graveyard); countered, it goes to the graveyard (701.6a) | `ChooseMode` (X), `SearchLibrary` |
| Robber of the Rich (supported since M7a) | reads the IR's `while_exiled` (a `this_turn` duration with the Rogue requirement is a blocker); its lowered definition is unchanged | - |

### 32.2 Rules added on the way

* **Reflexive triggered abilities with an intervening "if"** (603.12, 603.4):
  `Lowerer::delayed_ability_if` gives the delayed ability a `TrigPred::If`; `Ext::Reflexive`
  puts it among the waiting triggers only if the condition holds (for the ability's source), and
  on resolution `Game::intervening_holds` checks it again, now also for the source (`ControlOtherOfColor`
  needs it: "other than Ajani"). A printed trigger's `TrigPred::If` is evaluated for its source
  too. No other card's intervening "if" reads its source (32.4: no other definition and no other
  game changed by it).
* **"Whenever ... this turn"** (603.7b): `M7d::TriggerThisTurn` gives the controller a
  `TempTrigger` with `this_turn` (the "until your next turn" list of 25.6); it is removed as the
  next turn begins. Its source is the spell card, which has left the stack: the ability is the
  definition's (113.7a).
* **"One or more creatures deal combat damage"**: the `DealsCombatDamage` events of one combat
  damage step form a batch (`begin_batch` / `end_batch` in `combat.rs`, 510.2), so a
  `TrigPred::Batch` ability triggers once per step; a first-strike step and the regular one each
  (510.4). Temporary abilities are batched like printed ones. No other supported card has a batch
  trigger on combat damage.
* **A spell's own instruction moving it** (608.2c): `M7d::ShuffleThisSpellIntoLibrary` moves the
  resolving spell from the stack into its owner's library and shuffles (a flashback spell would
  be exiled instead, 702.34a, `move_card`); the lowering requires it to be the last instruction of
  an instant or sorcery (`check_self_shuffle`).
* **Mana abilities with {X}** (107.3, 602.2b, 605.3a): `ManaProd::ComboX`, with priority only
  (28.1): X is announced first (`Task::ManaNow` asks `ChooseMode`: as much as the player can pay
  without the source, like an activated ability's X, 29), then the {X} is paid, the source tapped
  and sacrificed, and each of the X mana is a `ChooseMana` among the five colours. The lowering
  requires the cost's {X} and the production's X together, and reads the IR's `amount: 0` beside
  `amount_is_x` (ir_defects 10) only in that shape. Never a source while paying another cost.

### 32.3 Decisions, tasks, encoding, the bot

No new decision kind. `Task::ManaNow` now asks X (task code 18 of the context block, "other");
`Task::ManaNowCosts` carries X. Layout v3 is unchanged (29,860 floats): the new instructions are
`Ext` ops (one op kind), the "this turn" abilities count among a player's temporary triggers
(`o[32]`), no width or identity changed. The red Boo is a new token definition (the colourless
one is gone): its registry line is needed (Boo, red). The heuristic bots: a mana ability's X is the
first option (X = 0: Wizard's Rockets sacrificed for its draw), a reflexive trigger's target as any
trigger's.

### 32.4 The lowered definitions

`SUPPORT_DEFS=<file> cargo test --release -p engine --test support cube_support_report` writes
every definition (cards, tokens, faces) with the interned subtypes; `python tools/diff_defs.py
old new [--show]` compares two dumps by name (token, face and subtype ids move when another card
becomes supported). Main (6d6760c: its IR and lowering) against this branch: 10 definitions added
(the 9 cards and Ajani, Nacatl Avenger's face), 1 changed (the Boo token: colourless to red), 618
unchanged, among them Robber of the Rich's.

### 32.5 Simplifications (differences from the CR)

(Both still apply; docs/shortcuts.md.)

* Wizard's Rockets' X mana with priority: one `ChooseMana` decision per mana (the combinations are
  not offered as one choice); the bot announces X = 0. While paying (M7e part 2) its X is the
  mana it turns into colours, never asked.
* A "this turn" triggered ability is removed when the next turn begins (nothing can trigger it
  between the end of the cleanup step and then).

### 32.6 Tests, fuzzing, digests

Card tests for the nine cards on the cube IR (the copies and the custom card they replaced are
gone): `rules_lands.rs` (Strip Mine), `rules_layers.rs` (the manlands), `cards_m7b_walkers2.rs`
(Minsc & Boo), `cards_m7b_faces4.rs` (Ajani: the 0 asks no target on activation, its reflexive
trigger asks one as it goes on the stack, does nothing without another red permanent and when
that permanent is gone before it resolves), `cards_m7c_tail.rs` (Chromatic Star: draws when
sacrificed, not when exiled), `cards_m7c_tail2.rs` (Green Sun's Zenith shuffled in, to the
graveyard when countered; Forth Eorlingas!: a trigger in the first-strike step and one in the
regular step for two Knights at once, none the next turn), `cards_m7c_tail3.rs` (Wizard's Rockets:
enters tapped, X = 0..2 with two lands, one colour per mana, the draw). `support.rs`: the exact
supported list (546), loud failures on an edited IR (Strip Mine without its mana), the IR checks
of this section (Robber's `this_turn`, Green Sun's Zenith's shuffle not last, "add X mana" without
{X}). `tools/untested_cards.py`: 0 untested.

`cargo test --release -p engine --no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden
--test support --test smoke --test 'bots*'`: 835 tests pass. Fuzzing (20,000 games over the 546
cards and the custom cards): no failure (141 s; 5,630,719 options completed, 6.2 million decisions
after them); one fault found on the way, outside these cards and fixed: a coloured spell was
offered when Emrakul, with protection from coloured spells, was its only possible target (the
cast check took the card in hand for its source, not a spell; `cast::Ctx::spell`, seed 7209).
`hidden.rs` compares a player's cards by identity (a face-down permanent's card, 31.2, may move to
another of the player's hidden cards in a determinized world; the new pool's random decks put
Test Manifester's face-down cards into its games). Behaviour digests re-recorded for the 546-card
pool (section 15: 1f210100db7b442e), before the red Boo's token kind is registered.

## 33. The rules shortcuts removed (M7e part 1)

Branch `m7e-rules` (from main at 7d2ad02): every place the engine knowingly departed from the CR
in triggers, replacement effects, casting routes and a few small rules, made rules-correct; the
whole tracking table is `docs/shortcuts.md` (every item of section 13's list and of 23.11, 24.7,
25.10, 26.11, 27.13, 28.8, 29.6, 30.7, 31.7 and 32.5, with its class and status). The payment
model (the pool's mana choice, mana abilities with a cost during a payment) is the next part's
and was left alone. The action-id layout, `NUM_ACTIONS` and layout v3 are unchanged; the token
registry gained "The monarch rules" (appended).

### 33.1 Triggers on the stack

* **The monarch** (724.2): its two inherent triggered abilities ("at the beginning of the
  monarch's end step, that player draws a card"; "whenever a creature deals combat damage to the
  monarch, its controller becomes the monarch") are the triggers of a rules definition "The
  monarch" (`lower/effect_stack.rs::monarch_def`, built for the cards that make a monarch),
  sourceless (a slot in no zone, `Game::rules_srcs`, as the initiative's, 27.4), controlled by the
  player who is the monarch as they trigger, one per creature dealing the damage
  (`dungeon.rs::monarch_trigger`); "its controller" is read as it resolves (its last known
  information if the creature left: `Ext::EventControllerBecomesMonarch`). Tishana's Tidebinder
  can counter them.
* **Okoye's** "whenever that creature attacks the monarch this turn, it gains double strike and
  trample until end of turn" is a delayed triggered ability with a duration (603.7b) about that
  creature (603.7c): `M7d::TriggerThisTurnFor` gives the controller a temporary trigger bound to
  the object (`TempTrigger::obj`), event `Attacks`, `TrigPred::AttacksMonarch`.
* **Event groups** (603.2c, "one or more"): `Game::ev_seq` is bumped as an instruction of a
  resolving spell or ability ends (`run_frame`), as a resolution begins, as a batch (SBAs,
  "destroy all", a sacrifice cost, a combat damage step) begins or ends and as a cost starts
  being paid. A "whenever one or more ..." ability (Laelia's exiles, Wan Shi Tong's "cards put
  into a library", Inti's discards, Ajani's Cats, Forth Eorlingas!'s combat damage) triggers once
  per group (`Game::batch_once`); one instruction's decisions (Brainstorm's put-backs, a discard
  one card at a time) stay one event, two instructions of one resolution are two.

### 33.2 Replacement effects

* **616.1**: the replacement effects of where a card goes (Blightsteel's "shuffle it into its
  owner's library", Yawgmoth's Will's "exile that card instead", Dauthi Voidwalker's "exile it with
  a void counter", "if it would die / leave the battlefield, exile it instead", a finality counter,
  flashback's exile, a permission's "exile it instead") are applied in the order the affected
  object's controller (its owner if it has none) chooses, each at most once, until none applies
  (616.1f: Blightsteel's library then meets "exile it instead of putting it anywhere else").
  `Game::replaced_dest` walks every order; one result applies at once; several make the card go
  to exile for now (its arrival events held back) and `Task::ChooseReplacement` asks right after
  the move (`ChooseReplacement`: `NUMBER` 1 library, 2 exile, 3 exile with a void counter). None
  of them is a self-replacement effect (616.1a).
* **Mox Diamond** however it would enter (614.1a): put onto the battlefield by an effect, the card
  waits revealed in no zone (`Zone::Nowhere`, the invariants allow it only with its task) while
  its would-be controller answers a `Discard` (a land card, or `PASS`:
  `Task::EnterByDiscardAny`); then it enters, or goes to its owner's graveyard with that
  destination's replacements. Cast, the choice is still made as it resolves.
* **Entering with counters and tapped** (614.1c, 122.6): the counters the effect gives (From the
  Catacombs, Throne of the Dead Three, persist, the opening-hand luck counter:
  `Game::entering_counters`), "tapped" (`entering_tapped`: tokens created tapped, a reanimation
  put tapped) and the card's own "enters with" counters, loyalty and lore counter are on it before
  its `Enters` event (`put_onto_battlefield` holds the event back for every permanent now).
* **Entering as a copy** (707.5, 614.12a): not of a permanent entering at the same time (Exhume's
  other creature) or since: `Game::group_ts` keeps the first timestamp of the current event's
  entries.

### 33.3 Casting routes

* **A granted escape or retrace with a card's own additional costs** (601.2b, 601.2f-h): the
  route's cost and each option are one cost, built when the database is (`CardDb::merged_cost`,
  `db.rs::merge_costs`: `None` for costs one `Cost` can't say, two different discards).
  Underworld Breach casts Bitter Triumph paying "exile three other cards" and "discard a card or
  pay 3 life".
* **The permission a graveyard card is cast with** (601.2a-b): `Game::gy_routes_esc` lists the
  routes (`cast::route`: flashback, Yawgmoth's Will, escape, retrace, mayhem, Serra Paragon,
  Lurrus; an effect's permission for the card alone); a card is offered at priority if any route
  can complete, and `Task::ChooseRoute` (`ChooseRoute`, `NUMBER` = route code) asks when several
  can. Serra Paragon casts a card with another face (its permanent face: an Adventure creature,
  an MDFC's permanent front). Two permanents of one kind are offered once.
* **Bolas's Citadel** offers every face of the top card (`top_options`: either half of a split
  card, an MDFC's or an Adventure's other face, an MDFC's land face to play; `ChooseFace` when two
  can), for life equal to that face's mana value.

### 33.4 Small rules

* A damage source that left the battlefield deals damage as it last existed there (608.2h,
  113.7a: `Game::lki_damage`, kept only when `SEEN_LKI_DAMAGE`).
* Saddle and teamwork don't count "crews Vehicles as though its power were greater"
  (`Cost::saddle`; crew only). Teamwork's taps are paid before the spell's mana (601.2h: any
  order) and every choice leaves the mana payable without the creatures tapped (`crew_mana_ok`,
  every set of creatures with mana abilities tried; `pay::classes_without`), so creatures with
  mana abilities can be tapped for it.
* Turning a face-down permanent face up is a special action (116.2b, 708.7): offered after the
  permanent lost all abilities.
* Necromancy's "as though it had flash" is a permission (`CardDef::flash_permission`; 702.8a:
  Necromancy has no flash).
* Converge is exact for any number of sources (`pay.rs::most_colors_matching`: a matching per
  colour set beyond seven sources).
* An Aura or Equipment becoming attached gets a new timestamp (613.7e,
  `Game::attached_timestamp`; it moves to the end of the battlefield).
* A trigger an effect gave a permanent (Serra Paragon's, earthbend's) is gone once the permanent
  lost all abilities (613.1f), and a granted triggered ability's source is the permanent that has
  it (113.7: granted ward, Nadu's; Tidebinder then affects that permanent).

### 33.5 Decisions, tasks, the bot, encoding

Two decision kinds (section 11): `ChooseReplacement` and `ChooseRoute` (41 of 64). Tasks:
`EnterByDiscardAny`, `ChooseReplacement`, `ChooseRoute` (the context block's task code "other").
The heuristic bots answer both new kinds (`az/bots.rs`: a card its chooser owns back into the
library rather than exiled and without a void counter, the reverse for an opponent's; a permanent
escapes if it can, else a permission that costs nothing later). Layout v3 is unchanged: no width,
offset or identity changes (the token registry gained a line, "The monarch rules", for the new
rules definition's identity).

### 33.6 Tests, fuzzing, smoke, digests

`tests/rules_shortcuts.rs`: 27 tests, each naming the CR rule it checks; 26 failed on main's
engine (7d2ad02, with only the test kit's `monarch` builder and the two decision kinds added to
compile them); the other (Okoye's trigger resolving before damage) checks the result is the same.
Custom cards for them: Test Twice Exile, Test Twice Oust (two events in one resolution), Test
Pilot (a crew bonus). Two earlier card tests changed with the rules (the monarch's draw and
Okoye's keywords now resolve first).

`cargo test --release -p engine --no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden
--test support --test smoke --test 'bots*'`: 863 tests pass. Fuzzing (`FUZZ_GAMES=20000`, the
option-completion check on): no failure, 143 s, 5,664,330 options completed; one fault found on
the way and fixed (section 15). The search smoke (`python tools/search_smoke.py`, seeds 0-3,
random and eval specs): 0 failed of 8 configurations (3,200 games, 1.31 million decisions, 33.8
million simulations, 466 s). Speed: section 17 (+1.6% to +3.7% per decision on main's pool).
Behaviour digests re-recorded (`DIGEST_WRITE=1 cargo test --release -p engine --test digest --
--ignored`, the branch's last commit): combined digest 161108b935c03d14 (section 15).

## 34. The payment model (M7e part 2)

Branch `m7e-pay` (from main at f89ada4): the places where paying mana departed from the CR (106,
117, 601.2f-h, 605), made rules-correct where the difference shows in play, without options that
are equivalent or dominated (docs/shortcuts.md 13.1, 13.2, 28.8.1, 30.7.4, 31.7.1, 31.7.2 and the
converge note of 28.8.2 fixed; 13.12-13.17 what stays, with reasons). The action-id layout,
`NUM_ACTIONS` and layout v3 are unchanged.

### 34.1 The virtual pool and the pool's choice (106.4, 601.2h)

The player's pool and the mana of the several-mana sources picked so far form the payment's
virtual pool (`Game::vgroups`; artifact-only {C} only for an artifact spell). `PayMana::vm` is the
most of it the payment can spend with single-mana sources paying the rest (`most_spent`: every
spend of that size tried with the forced units in the matching; `None` while several-mana sources
must still be picked); the single-mana picks stop when they and `vm` mana cover the cost exactly.
Then `pool_rems` lists the pools the payment can leave, each once (two units of one type and tag
are the same), artifact-only {C} spent before other {C} and haste {R} after other {R} except on a
creature spell (never worse for the player); `PayPool` asks when there are several (`NUMBER` + k;
the first keeps the most types). The pool's mana is a candidate unit in every feasibility check
(no greedy assignment: a hybrid pip can no longer hide a payable cost). Converge reads the colours
of the pool's mana chosen. Kept: the payment spends as much of the virtual pool as it can
(13.13), and no mana is floated with priority (13.12: the reasons).

### 34.2 Mana abilities with a cost, while paying (605.3a)

* **A one-mana cost** (Sunken Ruins' {U/B}, Loot's {G}, Arena of Glory's {R}, Chromatic Star's
  {1}): the ability is a several-mana source whose cost is one more need of the same payment
  (`Class::need`, `PayMana::marked`), `NOT_MADE`; its own mana is `MADE` (`mana.rs::fits`: such
  mana never pays such a need, so nothing pays for itself; it never pays another such ability's
  cost either, 13.15). The matching handles the needs with the pips; the fast path (no needs) is
  unchanged (`best_chosen::<false>`).
* **Pentad Prism** (a counter, no {T}): one-mana source usable once per counter in one payment
  (`Class::reuse`).
* **Wizard's Rockets** ({X}, {T}, sacrifice: X mana in any combination of colours): picked, every
  unit of the payment may be of any colour (`PayMana::convert`, `Cands::convert`): its X is the
  mana it turns into colours, never asked. Offered only when nothing else pays (13.14); with
  priority too, for its draw. (Section 49: its activation rules out its card's other sources, Urza's
  tap and improvise, and it never turns mana with a spending restriction into colours.)
* **Squandered Resources, Orcish Lumberjack** (sacrifice another permanent): sources on each
  permanent they may sacrifice (`SQ_FOR`: one mana of a type the land could produce, 106.7;
  `LJ_FOR`: three of red and/or green, the Lumberjack tapped, `Class::share` and `Budget::tap`
  limiting them to the Lumberjacks there are). Ids: the permanent's `ACTIVATE` slots after its own
  abilities, with Rain of Filth's (`Class::slot_id`; none past the fourth).
* **Where**: any payment, an "unless pays" cost during resolution included. With priority only
  Lion's Eye Diamond; Chromatic Star and Wizard's Rockets (a triggered ability as their source
  goes) also with priority (`Game::mana_with_priority`). Exhaust (Loot) is marked as the payment
  activates it.

### 34.3 A tap and a sacrifice of one card (601.2g)

A source's use of its card is recorded (`Class::res`: tap, sacrifice; both for most mana
abilities, none for Pentad Prism's counters). A card's sources are alternatives unless one taps
and the other sacrifices it: in `cands` each card's tap way, sacrifice way and both together are
its alternatives; in the payment both may be picked (the tap is activated first). Rain of Filth's
granted sacrifice (`SAC_FOR`) is now a one-mana source of its own, combined with any tap ability
of the land: a painland's damage, Mana Confluence's life, Mishra's Workshop's restricted mana.

### 34.4 Activation before the costs are paid (601.2g-h)

* **Delve**: a mana source that sacrifices itself and is a card (Lotus Petal) may be chosen in
  the delve decision (`ChooseGraveyardCard`, its `MY_PERM` id; before the graveyard cards, in
  canonical order): it counts as one card delved (`Mark::DelveLate`), the payment activates it
  first (`PayMana::forced`, its mana's type chosen, `ChooseMana`), and the card is exiled from the
  graveyard at the end of the payment. Castability counts each such source as its mana and one
  more generic mana (`Game::delve_classes`).
* **"Exile n cards from your graveyard"** (escape, Abhorrent Oculus): such sources count toward n
  when the graveyard is short (`exile_short`, `exile_sources`); the payment activates them first,
  and the cost's exile (after the mana) can take them.
* **Life after the mana**: "pay X life" and Bolas's Citadel's life are paid at the end of the
  payment (`PayMana::life_after`, reserved in `keep.life`), after the mana abilities' damage.
* Kept: a several-mana source that sacrifices itself (Black Lotus) is not one of those cards in
  the payment it helps (13.16).

### 34.5 Decisions, tasks, the bot, encoding

One decision kind: `PayPool` (41; 42 of 64), a minor decision like `PayMana` (`mtg` game's
`minor_decision`). New action meanings: a mana source's `MY_PERM` id in a delve decision; the
sacrifice sources' `ACTIVATE` ids after a permanent's own abilities. `Task::PayMana` holds a
`Box<PayMana>` (the task enum stays small: `PayMana` grew to 280 bytes). `Mark::DelveLate`. The
heuristic bots answer `PayPool` with its first option (the pool keeping the most types) and are
legal for the rest (the first option elsewhere). Layout v3 unchanged: no width, offset or identity
change (`az/encode.rs` untouched); the payment context shows the whole cost being paid.

### 34.6 Tests, fuzzing, smoke, digests, speed

`tests/rules_payment.rs`: 14 tests, each naming the CR rule it checks; all 14 failed on main's
engine (f89ada4, with only the test kit's `pool` builder and the `PayPool` kind added to compile
them). Earlier card tests rewritten for the new model: Sunken Ruins, Loot, Orcish Lumberjack,
Squandered Resources, Chromatic Star (`cards_m7c_tail.rs`), Pentad Prism (`cards_m7c_tail3.rs`),
Arena of Glory and Rain of Filth (`cards_m7d.rs`).

`cargo test --release -p engine --no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden
--test support --test smoke --test 'bots*'`: 877 tests pass. Fuzzing (`FUZZ_GAMES=20000`, the
option-completion check on): no failure, 181 s. Faults found on the way, all fixed: Wizard's
Rockets' colours counted in the pool's spend without Rockets picked (a payment ended unpayable);
an unpicked converter's need counted in the pool's spend; the `PayPool` answer lost when the task
was asked again; a second delve source left for later in the payment. The search smoke
(`python tools/search_smoke.py`, seeds 0-3, random and eval specs): 0 failed of 8 configurations
(3,200 games, 1.29 million decisions, 33.0 million simulations, 601 s on a machine with other
load). Payment decisions per game in random play (4,000 games, main's pool): `PayMana` 31.90 ->
32.16, `PayPool` 0 -> 0.11, `ChooseMana` 1.91 -> 0.97 (the priority mana abilities and Rain's
productions gone), `PayPhyrexian` 0.08 -> 0.09; 33.9 -> 33.3 in all. Speed: section 17. Behaviour
digests re-recorded (`DIGEST_WRITE=1 cargo test --release -p engine --test digest -- --ignored`,
the branch's last commit): combined digest b3a102f4d67c5e97 (section 15).

## 35. Sideboards, the play-or-draw choice, companions (M8a)

Branch `m8a-companion` (from main at c91f59e): phase 1 of "matches and sideboarding" (DESIGN.md
0, M8), the foundations inside one game: a sideboard per player, the play-or-draw choice and
companion. The user's phase-2 decisions (a best-of-three match as the `mtg` episode, sideboarding
between games, each player knowing the opponent's pool except their first picks) are recorded
in DESIGN.md; phase 1 keeps one game per episode and open decklists. Layout v4 starts here
(35.6). `NUM_ACTIONS` and the action ranges are unchanged.

### 35.1 The setup (CR 103)

`Game::with_sideboards(db, spec, decks, sideboards, seed)` (`Game::new`: no sideboards) puts the
decks in the libraries, sets the sideboards aside (103.2a: what is left is the starting deck),
determines the chooser and runs to the first decision, the chooser's `PlayDraw` (103.1). The rest
of the setup follows the answer, in the CR's order:

| Rule | Step | How |
|---|---|---|
| 103.1 | the chooser chooses who takes the first turn | `GameSpec.chooser`: `None` = a player chosen at random from the seed (the first game of a match), `Some(p)` = `p` (a later game: the loser of the previous one, phase 2). `Turn::chooser`; `Task::PlayDraw`, `DecisionKind::PlayDraw`: `PLAYER` = the chooser plays first, `PLAYER` + 1 = it draws first |
| 103.2a, b | sideboards set aside; companions | `Task::RevealCompanion`, the starting player first, then the other (101.4: turn order) |
| 103.3 | each player shuffles | `Task::DrawHands` |
| 103.5 | opening hands, then mulligans | `Task::DrawHands` (the starting player first), the London mulligan as before |
| 103.6 | opening-hand actions | `Task::Opening`, as before (Gemstone Caverns) |
| 103.8 | the first turn; the starting player skips its draw (103.8a) | as before |

The die roll is part of the initial state (no transition reveals it; `Turn::chooser` is public).
The opening hands are now drawn by the first `apply` (the chooser's answer) and keyed like every
draw (`KEY_DRAW`, private to the drawer); before M8a they were drawn in `Game::new` and the
setup's keys dropped. The heuristic bot plays first (35.4).

### 35.2 Sideboards (CR 100.4, 400.11)

`Player::sideboard` (`Arc<[DefId]>`, sorted; a clone copies a pointer) holds the cards outside
the game (400.11a; in limited the rest of the player's pool, 100.4b; the generated decks have
seven, 35.7). They are not objects: nothing in a game reaches a sideboard card except a
revealed companion. `Game::sideboard_size(p)` counts the cards still outside the game (a
companion put into the hand is in it, 400.11b). Sideboarding between games is phase 2.

### 35.3 Companion (CR 702.139)

"Companion — [Condition]" (702.139a) is now in the card IR (mtg-bot `feature/companion-ir`,
docs/card_ir.md): `CardPrinting.companion`, a `Companion` holding a `DeckCondition`:
`EachCardInStartingDeck { cards, must }` (Lurrus of the Dream-Den: `IsPermanentCard` /
`HasManaValueAtMost(2)`; Zirda, the Dawnwaker: `IsPermanentCard` / `HasActivatedAbility`) or
`StartingDeckDifferentNames { cards }` (Lutri, the Spellchaser: `ObjectNot(IsLand)`). The lowering
(`lower::filter::lower_companion`) makes `CardDef::companion: Option<DeckCond>` (`DeckCond::Each`,
`DeckCond::DifferentNames`) whose filters must read only a card's own characteristics
(`db::deck_filter_ok`; anything needing a game, a controller or counters, is unsupported, never
ignored). Two filters are new: `IsPermanentCard` is `Filter::Type(PERMANENT)` (110.4a: artifact,
battle, creature, enchantment, land, planeswalker), `Filter::HasActivatedAbility` a card's printed
activated abilities (`CardDef::has_activated_ability`: mana abilities, the intrinsic ones of basic
land types included, 305.6; loyalty abilities, 606; equip, reconfigure, Class levels; from hand
and from a graveyard; not special actions, plot and turning face up, 116.2b/k; not latent
abilities, which only an effect gives).

**The condition** is checked exactly against the starting deck (702.139b: the deck after the
sideboard is set aside) when the reveal is asked: the player's library, all of it, before the
shuffle and the draws (`CardDb::deck_satisfies`, over `CardDb::def_matches`: a card outside the
battlefield and the stack has its front face's characteristics, 712.8a). Lurrus: every permanent
card has mana value 2 or less (a land's is 0; instants and sorceries don't count). Lutri: no two
nonland cards share a name (basic lands may repeat). Zirda: every permanent card has an activated
ability (a basic land has one).

**The reveal** (103.2b): `Task::RevealCompanion` asks a player who has a candidate (a sideboard
card with companion whose condition the starting deck fulfills): `PASS` or `LIBRARY` + k (the
k-th candidate by definition, `Choice::Def`); at most one is revealed. The card becomes an object
in no zone (`Zone::Nowhere`, as Mox Diamond's waiting card), known to both players,
`Player::companion`; a public key `KEY_REVEAL(owner, def)`.

**The special action** (116.2g, 702.139a): at priority, during a main phase of the player's turn
with the stack empty, if they revealed a companion still outside the game, haven't taken the
action this game and can pay {3}: `act::COMPANION` (= `PLAYER` + 0, `Choice::Companion`). It pays
{3} (`COMPANION_COST`, a `PayMana`; Zirda's reduction doesn't apply: a special action isn't an
ability) and `Task::CompanionToHand` puts the card into its owner's hand, known to both (it was
public); it stays in the game (702.139c); the player keeps priority (116.3). Restrictions on
casting don't apply (it isn't a spell).

### 35.4 Decisions, tasks, choices, the bot

Two decision kinds: `PlayDraw` (42) and `RevealCompanion` (43): 44 of the context block's 64.
One choice: `Choice::Companion`. Tasks: `PlayDraw`, `RevealCompanion` (task codes 24, 25),
`DrawHands`, `CompanionToHand` (never asking). The heuristic bot plays first (in these 40-card
decks the first land drop and the tempo are worth more than a card), reveals a companion whenever
it can (the first candidate), and takes the special action in its main phase when it casts
nothing else (before its tier-1 abilities). `HeuristicV1` (the frozen older policy) answers the
new kinds with its first legal option (play first, no reveal) and never takes the action.

### 35.5 Hidden information

A sideboard is its owner's: `view_of(viewer)` holds the viewer's sideboard, the other player's
sideboard size, both revealed companions and whether their action was taken, the chooser and the
starting player; the encoding shows the opponent's sideboard only as its size (tested: two games
that differ only in an unrevealed sideboard card look the same to the opponent, view and
encoding). `determinize` keeps both sideboards: under phase 1's information model (open
decklists) no card of a sideboard can enter the game but a revealed companion, which is public,
so no transition depends on the hidden part. The other player's search does see whether a
player is asked the reveal (whether they could reveal a companion), a moment before they reveal
or decline; kept, as it says nothing about the game's hidden cards. Phase 2 replaces open
decklists by the pool knowledge of DESIGN.md 0, and `determinize` will then deal the opponent's
deck and sideboard from the unseen part of their pool.

### 35.6 Encoding: layout v4 (29,972 floats)

Layout v3 (frozen, 22.6) plus, at the end, two companion segments, and v4's globals in spare
floats:

| Field | Where | What |
|---|---|---|
| `my_companion`, `opponent_companion` | `O_COMP` (29,860), 2 x 56 (`COMP_W`) | the viewer's, then the opponent's revealed companion: card code (the identity), revealed (`comp::PRESENT`, the segment's present flag), its special action still available (`AVAILABLE`: outside the game, not taken), the action is an option of the viewer's decision now (`LEGAL`), put into the hand (`IN_GAME`). `my_companion` scores `act::COMPANION`; `opponent_companion` scores nothing |
| `glob::CHOOSER` | global 124 | the viewer chose who takes the first turn (global 18: the viewer is the starting player) |
| `glob::SIDEBOARD` | per player 38 | the cards of the player's sideboard still outside the game / 15 |
| `glob::COMPANION` | per player 39 | a revealed companion whose special action is available |

The `RevealCompanion` decision lists its candidates in the `list` block (`LIBRARY` slots). The
layout file is `mtgz/az/layout_v4.json` (`"version": 4`; `layout_v3.json` renamed, references in
`mtgz/az/__init__.py`, `engine/tests/az.rs`, `tests/test_mtg_az.py`). Layout v4 stays open until
phase 2 ends (the match unit will add its fields); it is frozen at the first run. No new token
kind (no token definition changed). On merge: `MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test az
layout` regenerates the file (this branch only bumped its version).

### 35.7 Decks: sideboards and companions

`decks::generate_pool(db, rng, side, gen)` -> (deck, sideboard): the deck is `generate`'s from
`rng` (the same deck for the same `rng` unless it is built for a companion); `side` decides the
rest. One deck in four is built for a companion its colours can cast (`castable_with`: every
coloured pip, a hybrid pip by either colour), chosen at random: the candidate spells and nonbasic
lands are only those that keep the condition (`fits`), and the companion goes to the sideboard if
the finished deck passes `check` and fulfills the condition (else the deck is generated as usual).
Otherwise, half the time, a companion the deck happens to fulfill and can cast (Lutri, for any
singleton deck with blue or red) goes to the sideboard. The sideboard is then filled to
`decks::SIDEBOARD` = 7 with random spells of the deck's colours not in it (singleton, sorted).
`MtgSpec::pools(seed)` uses `generate_pool` for random decks (the `side` generator from the seed
too: seeded, deterministic) and `decks::eval_pools` for the evaluation set; `MtgSpec::decks` is
`pools`' decks. In the first 400 seeds' 800 random decks Lurrus is in 82 sideboards, Lutri in
261, Zirda in 54, each with a deck that fulfills it (tests/rules_m8a.rs). The fuzzer plays one
game in four with `generate_pool`'s decks and the others with random decks and seven random
sideboard cards.

### 35.8 Tests, fuzzing, smoke, digests, speed

`tests/rules_m8a.rs`: 11 tests, each naming the CR rule it checks: 103.1 (the chooser decides
before the shuffles and hands, both players get to choose over seeds, the chooser as a spec
parameter), 103.2b (candidates offered in turn order before the hands, at most one revealed, a
declined companion stays in the sideboard), 702.139a's three conditions met and broken (Lurrus:
a mana value 3 permanent breaks it, a mana value 5 instant doesn't; Lutri: two Lightning Bolts
break it, forty basics don't; Zirda: Thalia and Ragavan break it, basic lands don't), 116.2g (not
in the upkeep, not on the opponent's turn, not with a spell on the stack, not without {3}; once;
into the hand, known to both; no stack), 400.11a (the sideboard hidden from the opponent's view
and encoding; a revealed companion visible to both), `determinize` keeping sideboards and the
companion, the generated decks, the heuristic bot (plays first, reveals, takes the action). They
use the new API, so they cannot run against main's engine. Tests adapted to the new first
decision: `rules_setup.rs` (2), `rules_priority.rs` (103.8a), `rules_sba.rs` (the turn cap),
`cards_m7d.rs` (Gemstone Caverns), with `Game::play_first` in the test kit; the kit's
`Scenario::sideboard` / `companion` build positions; `check_invariants` accepts a revealed
companion in no zone.

`cargo test --release -p engine --no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden
--test support --test smoke --test 'bots*'`: 888 tests pass (main's 877 and the 11 new), 7
ignored. Fuzzing (`FUZZ_GAMES=20000`, the option-completion check on): no failure, 160 s with other
load; companions revealed in 2,641 games, put into a hand 2,282 times (the fuzzer asserts at least
one special action in 200 games or more). Fault found on the way and fixed: the view and the
encoding read a used companion's identity from its slot, which a shuffle (Timetwister) can hide
again (`Player::companion_def` keeps the revealed identity). The search smoke (`python
tools/search_smoke.py`, seeds 0-3, random and eval specs, after `bash tools/build_py.sh`): 0 failed
of 8 configurations (3,200 games, 1,365,009 decisions, 35.0 million simulations, 548 s). Behaviour
digests re-recorded (`DIGEST_WRITE=1 cargo test --release -p engine --test digest -- --ignored`,
the branch's last commit): combined digest 076e92a6939c6798 (section 15). Speed: section 17.

### 35.9 What phase 1 leaves

* One game per episode, no sideboarding, open decklists; `determinize` keeps sideboards, and the
  other player's search sees whether a player is asked the reveal (35.5). Phase 2 (DESIGN.md 0).
* The die roll is part of the initial state (setup), not a chance key: no transition reveals it.
* Companion is the only way into a game from outside it (no wish in the cube); the sideboard has
  no other use in a game.
* The heuristic bot always plays first (not deck- or matchup-dependent).
* Layout v4's file: this branch renamed `layout_v3.json` and set `"version": 4`; the content
  (offsets, segments) is regenerated on merge with the `az` test (not run here).

## 36. Matches and sideboarding (M8b)

Branch `m8b-match` (from main at 536f659): phase 2 part a of "matches and sideboarding" (DESIGN.md
0, M8). The `mtg` episode becomes a match: best-of-one (exactly the game of M5a-M8a) or
best-of-three with sideboarding between games, whose result is the match's; seeded card pools and
a deckbuilder for self-play and evaluation; the encoding's match additions (layout v4, still
open). Part a kept phase 1's information model (open decklists); 36.10 lists what part b had to
change for the hidden pool, 36.11 is the information model as built (part b, branch
`m8b-hidden`): in best-of-three each player knows the opponent's pool but their first picks, and
their deck is hidden within it. `NUM_ACTIONS` and the action ranges are unchanged.

### 36.1 The match

`MtgAz` (engine/src/az) is `{ g: Game, m: MatchState }` (`az/matches.rs`): the current (or last)
game and the rest of the match. The spec's `"format"` is `"bo1"` (the default: one game; the
existing presets, tests and digests) or `"bo3"`.

| What | Rule | How |
|---|---|---|
| who chooses who takes the first turn | CR 103.1: in the first game of a match the players may use any agreed method; in a match of several games the loser of the previous game chooses; if the previous game was a draw, the player who made the choice in it chooses again | game 1: `GameSpec.chooser` = the spec's `chooser` or a player chosen at random (M8a); a later game: `Some(loser)`, after a draw `Some(previous chooser)` |
| when the match ends | the user's decision (DESIGN.md 0): a player has won two games, or three games have been played; a drawn game counts as a game; a match can be a draw | `MatchState::end_game` after every game; `wins`, `draws`, `results` (per game: 1 + winner, 3 = drawn) |
| the result | | 1 / 0 to the player with more game wins, 0.5 each on equal wins (1-1-1, 0-0 with three draws, 1-0 with two draws is a win) |
| between games | CR 100.4 (the sideboard), 100.4b (limited: the rest of the pool), 100.2b (a limited deck has at least 40 cards; any number of basic lands); a deck built from a pool is exactly 40 cards (the user's format restriction, section 42) | sideboarding (36.2), then the next game with the chosen decks and the rest of the pools as sideboards, chooser as above |

The Magic Tournament Rules play on after a drawn game until a player has two wins or the round's
time runs out (then the player with more game wins wins); stopping after three games is the
user's choice for self-play (checked against MTR 2.1 and 2.2 in 36.12). Companion reveals (103.2b)
and the play-or-draw choice happen in every game (M8a); the companion's "once per game" special
action is per game.

The search sees one episode: the last decision of a game leads straight to the first sideboarding
decision (or the end of the match), the last sideboarding decision to the next game's play-or-draw
decision. No chance key marks a game boundary: a game's end follows from public facts, and the
next game's setup reveals nothing (its shuffles come after the play-or-draw choice; its draws are
keyed as before). (Part b: with hidden decks the next game's setup shows the decks' and
sideboards' sizes, which the hidden sideboarding decided: a public `KEY_GAME`, 36.11.) The next game's seed comes from the match's RNG, which `determinize` reseeds (a
future game's shuffles are not known in the search's worlds).

### 36.2 Sideboarding (`Sideboard`, `SideboardCount`)

Each player's **pool list** (`MatchPool::list`): the pool's cards (deck and sideboard, basic lands
excluded) as (definition, copies) sorted by definition, then the five basic lands (W U B R G, any
number). A deck is a count per entry. Players sideboard one after the other, the next game's
chooser first (the first one's choice hidden from the second, and resampled by the second's
`determinize`: 36.11); each player's decisions:

* `Sideboard`: `PASS` = done (offered when the deck has `min_deck..=max_deck` cards: exactly 40
  for a pool's deck, section 42), or `LIBRARY`
  + k = change entry k, for k after the last entry changed (increasing list order). A single card
  (one copy in the pool) goes in or out at once; a basic land or a card with several copies is
  followed by
* `SideboardCount`: `NUMBER` + n = its new count (n other than its count now, 0..31).

An option is offered only when a legal deck stays reachable (the entries after it may still keep
their counts or take any count up to their copies), so no sequence dead-ends. **Every legal deck is
reachable in exactly one way**: the entries it changes, in list order, with their new counts, then
`PASS` (an entry is changed at most once, to a different count). Keeping the deck is one decision;
a swap of two single cards three; a card for basic lands three or four; basic lands of one type
for another four. A sideboarding is at most 1 + (single entries) + 2 x (other entries) decisions
(k increases every time), and a random player made ~7 per sideboarding with decks of 40-60 cards
(fuzzing, 36.8), ~7.6 at exactly 40 (section 42). Bounds (section 42, 2026-10-27): a deck keeps
the first deck's size, `MatchPool::min_deck` = `max_deck`: exactly 40 for every deck built from a
pool (the deckbuilder's, M8a's generated decks, a drafted one), a spec decklist's own size in
tests (until then: at least 40, 100.2b, or the first deck's size when smaller, and at most 60 or
the first deck's size when larger); a count at most 31 (`NUMBER`'s range); a pool list has at
most 64 entries (`LIBRARY`'s range: 59 different cards and the basics; a bigger pool is an error
in best-of-three). Test: `cr_100_2b_sideboarding_reaches_every_legal_deck_exactly_once` searches
every decision sequence of a small list (three cards, one with two copies, two basics) and finds
each legal deck once, from three starting decks: the 180 decks of 5..7 cards, and with bounds
6..=6 the 60 decks of exactly 6.

Both decks chosen, the next game begins (`Game::with_sideboards`: the decks, the pools' other
cards as sideboards, `chooser`). The companion is revealed from the new sideboard if the new deck
fulfills it.

### 36.3 What carries over

`MatchState`: the format, the game spec, the pools (`Arc`), each player's current deck (counts),
the game number, per game the result, who chose and who started, wins and draws, the turns of
earlier games (`rounds`), the cards each player **revealed** in the current and the earlier games
(a bit per identity: every `KEY_REVEAL` key a step emits, i.e. every card made public, from hand,
library, sideboard or exile face down; `MtgAz::step` reads them in best-of-three), the
sideboarding in progress and the match's RNG. Clones copy pointers and ~600 bytes.

### 36.4 Pools and the deckbuilder (`az/pools.rs`)

Before team drafting exists, a match's pools are generated (`"pools": true`, the default for
`"bo3"`; `"pools": false` keeps M8a's generated decks with 7-card sideboards):

* `random_pool` (45 singleton cards of the 546 supported, seeded): two main colours; 27 spells of
  the main colours (weighted to creatures 3, interaction 2, other 1), 4 colourless spells, 5 nonbasic lands (weighted to the pair's duals,
  then lands of one colour, triomes and any-colour lands, colourless utility), then off-colour
  spells, half of them of a splash colour. Three of its main-colour spells are marked as the
  player's **first picks** (data only in this part: part b hides them from the opponent).
* `build_deck` (deterministic): the colour pair whose 23 best playable spells (colours within the
  pair, every pip payable) weigh most; 23 spells by weight then mana value, at most 3 of mana value
  5+ and 4 of 4 (lifted when fewer fit); companions stay in the sideboard; lands: the pool's
  nonbasic lands that make or fetch a colour of the pair and at most one colourless utility land
  (up to 9), then basics split by the coloured pips (at least a third each), 40 cards. The rest of
  the pool is the sideboard (~22 cards).
* The evaluation pools: `eval_pools(seed, i)`, fixed per evaluation seed (pair i = the game seed's
  `split64 % eval_decks`, as the evaluation decks). The evaluation spec keeps the format, so the
  yardstick, random and Elo games of a best-of-three run are matches.

Measured (tests/rules_m8b.rs, 200 pools): every pool 45 cards, 3 first picks among them; 35
pools with a companion (as ordinary main-colour or off-colour picks); none without 23 playable
spells; every deck passes `decks::check` (40 cards, 17 lands, two colours of spells, singleton
but basics).

Companions (2026-10-26, the user's call): until then one pool in four was also built around a
companion its main colours could cast, which runs/mtg4 used. Over 40,000 pools (20,000 matches) a
pool held a companion 38.4% of the time and could use it (the deck fulfills it and can cast it)
13.5%, every time Lutri (a singleton deck always fulfills it; the deckbuilder never builds for
Lurrus or Zirda); a uniform 45-card share of the cube would hold each companion ~8.2% of the time,
one of the three ~23%. Without the insert: 19.1% hold one (each 6.6-6.8%), 5.8% can use it
(Lutri); 35.6% of matches have one in some pool. The insert drew from the pool's random stream,
so every generated pool changed with it, the evaluation pools too: a run after this one doesn't
play mtg4's evaluation pools. M8a's best-of-one decks (`decks::generate_pool`, 35.7) still build
one deck in four for a companion. Team drafting (M6) replaces generated pools.

### 36.5 Bots

The heuristic keeps its deck (`bots::sideboard`: `PASS`, always legal at a player's first
sideboarding decision since the last game's deck is legal, 40 cards; the first legal option
otherwise, which ends at a legal deck since every option completes),
plays first and reveals companions as in M8a. zero-core's `random` picks uniformly among the
options (a random walk over the list, ending since k increases).

### 36.6 The `SearchGame` mapping

| zero-core | best-of-one | best-of-three |
|---|---|---|
| `to_move`, `legal_actions`, `step` | the game's | the sideboarding decision's between games, else the game's; a game's end starts sideboarding or ends the match |
| `result` | the game's (M5a) | the match's (36.1) |
| `margin(seat)` | the life difference / 20 | 0.75 x (game-win difference / 2) + 0.25 x the current (at the end the last) game's life margin |
| `score`, `rounds` | life, turns | game wins, the turns of all games |
| `determinize` | the game's (the match's RNG reseeded from the engine RNG after it: zero-core's stream is unchanged) | the game's, not while sideboarding (the last game is over); the match's RNG |
| `encode` | the game's + the match block and bags | the same; while sideboarding 36.7 |
| `minor_decision` | paying mana | the same; sideboarding is not minor |

### 36.7 Encoding: layout v4 additions (33,652 floats)

| Field | Where | What |
|---|---|---|
| list block | `O_LIST`, now 64 x 60 (was 40 x 56) | every `LIBRARY` id has a slot; four more floats (`list::*`). A sideboarding decision (the decider only) lists its pool list: card code, present, its `LIBRARY` id an option (`OPTION`), copies in the pool / 4 (`COPIES`; 0 for a basic land), copies in the deck being built / 4 (`IN_DECK`), in the last game's deck / 4 (`AT_START`), a first pick of the viewer's (`FIRST_PICK`), a basic land (`BASIC`), at or before the last entry changed (`DECIDED`). The slot scores `LIBRARY` + k (the list segment) |
| `bag::OPP_POOL` | bag 15 | the opponent's pool as the viewer knows it (open decklists: all of it; part b: but the first picks) |
| `bag::OPP_REVEALED` | bag 16 | the identities the opponent revealed in earlier games of the match |
| match block | `O_MATCH` (33,620), 32 floats, globals (`mtch::*`) | best-of-three; game number / 3; game wins / 2 (mine, the opponent's), draws / 3; sideboarding now, I sideboard first (I choose in the next game), I have chosen my deck; my deck's size / 40 (being built while I sideboard), the opponent's last deck's size / 40; per game played: I won / lost / drawn, I took the first turn, I chose who did; the entry a `SideboardCount` is about and the last entry changed ((k + 1) / 64) |
| context while sideboarding | context block | the kind (`Sideboard` 44, `SideboardCount` 45), the decider is the viewer, the asking task code 26 (sideboarding), the number of options; `SideboardCount`: the entry's card code |

While sideboarding the game blocks are empty (the last game is over): the encoding is the match
block, the match bags, the context and, for the decider, the list. The other player's deck being
built and, for the second player, the first player's chosen deck are not encoded (tested). The
layout file `mtgz/az/layout_v4.json` is regenerated by the main session (`MTGZ_WRITE_LAYOUT=1
cargo test -p engine --test az layout`; not run on this branch, so the file is stale here and
`tests/test_mtg_az.py::test_layout_and_token_network_read_the_encoding` fails until then).
`op_code` and `task_code` are unchanged (no new task or op).

### 36.8 Tests, fuzzing, smoke, digests, speed

`tests/rules_m8b.rs` (12 tests, section 15's table). `cargo test --release -p engine
--no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden --test support --test smoke
--test 'bots*'`: 900 pass (M8a's 888 and the 12 new), 7 ignored.

Fuzzing: `fuzz_random_matches` plays best-of-three matches with random actions (random pools,
the evaluation pools, M8a's decks with 7-card sideboards, 12-turn games: one match in four each),
checking the legal actions, the game invariants after every action, every ~40 actions a clone
and `determinize` for the decider (actions, encoding, `view_of` in a game), every later game's
decks (in range, within the pool) and chooser (103.1), and the match's end;
`same_seed_and_actions_replay_matches_identically` replays 12. `FUZZ_GAMES=20000 cargo test --release -p engine --test fuzz`: no failure, 146 s. The
20,000 games as before (companions revealed in 2,641 games, put into a hand 2,282 times: M8a's
numbers, the games are unchanged) and 1,000 matches: 2,562 games, 562 matches of three games, 748
drawn games (mostly the 12-turn matches), 410.7 decisions per match, 20,563 sideboarding decisions
(~7 per player's sideboarding), 3,058 decks changed.

Search smoke: `tools/search_smoke.py` has the specs `match` and `match_eval` (best-of-three on
random or evaluation pools, three times the step cap) and runs `random,eval,match` by default.
`python tools/search_smoke.py` (seeds 0-3, specs random, eval, match; after `bash tools/build_py.sh`):
0 failed of 12 configurations, 1,163 s (random and eval 60-71 s each, as before; match 150-159 s):
1,600 matches with 1,546,770 decisions (967 per match, against 411 per game on the random spec)
and 41.0 million simulations. `--specs match_eval --seeds 0-1`: 0 failed of 2 (800 matches,
754,404 decisions: 943 per match), 302 s.

Behaviour digests: the layout grew, and the digest hashes the encoding bit for bit, so every
game's digest changed; `DIGEST_ENCODING=0` (new) leaves the encodings out: on main's code
(536f659 + the option) and on this branch the 2,500 games give the same combined digest,
3fb15384404c7428 (decisions, legal actions, keys, determinized worlds and outcomes unchanged: the
`Game` is untouched). Re-recorded with the encodings (`DIGEST_WRITE=1`, this branch's last code commit): combined digest
63d5bd84a9f7c19b.

Speed (`cargo test --release -p engine --test perf match_layer -- --ignored --nocapture`, the
desktop, idle): best-of-one, 93,472 decisions (300 heuristic-against-random games, replayed twice in
alternating order): 12.64 us per decision for the bare `Game` (legal actions, clone, encode, apply:
M8a's `MtgAz`) and 12.88 us through `MtgAz` (+1.9%: the match block and the opponent's pool bag in
every encoding, the match state in every clone); both include the larger vector (33,652 floats,
+12%). Best-of-three: heuristic against heuristic 2.50 games and 379 decisions per match; random
against random 2.45 games and 457.

### 36.9 Python

`mtgz/az/__init__.py`: `SPEC_BO3` and the preset `mtg-4` = `mtg-3` with the best-of-three spec,
its evaluation spec (the evaluation pools, matches) and `max_steps` 60,000 (a match is up to three
games). Elo matches use the evaluation spec: matches. `tests/test_mtg_az.py`: the feature count,
a match through the bindings and the driver (the heuristic beats random), the presets. `temp_moves`
(60) counts the match's searched decisions, so in a match it covers game 1's opening only (kept as
mtg-3's; to revisit with the run). `python -m pytest -q tests` (after `bash tools/build_py.sh`):
19 pass; `test_layout_and_token_network_read_the_encoding` fails until `layout_v4.json` is
regenerated (29,972 != 33,652), and passes with `encode::layout_json`'s output (dumped to a
scratch file, not checked in).

### 36.10 What part b must change (the hidden pool)

(Part a's list, kept as written; all of it is done in part b, 36.11: 1-3 by `az/sample.rs`, the
companion question asked of every player; 4 by the bags; 5 by the prior; 6 checked and measured;
7 checked by a test.)

Part b replaces open decklists by DESIGN.md 0's knowledge: each player knows the opponent's pool
but their first picks, and the opponent's current deck is hidden within it. Every place that
assumes open decklists:

1. `Game::determinize` deals the owner's hidden cards (hand, library, face-down) from the owner's
   **actual** deck (the multiset is known). Part b: the viewer's world samples the opponent's
   first picks (cards the viewer has not seen), then their deck and sideboard from that pool,
   consistent with what the opponent revealed in this game (in the deck) and the deck's size
   (public), then deals hand and library from it.
2. `MtgAz::determinize` while sideboarding keeps the first sideboarder's chosen deck: the second
   sideboarder's tree is conditioned on it. Part b resamples it from the pool (zero-core's
   simultaneous-choice pattern, Glass Road Zero docs/search.md 9.3); the encoding already hides it.
3. The sideboards: `determinize` keeps both (35.5); part b deals the opponent's from their sampled
   pool. The other player's search sees whether a player is asked to reveal a companion (whether
   their deck fulfills one, 35.5): with hidden decks that says something about the deck; decide
   whether to ask every player with a companion in the pool.
4. The encoding: `bag::OPP_HIDDEN` (the opponent's hand and library cards the viewer doesn't know,
   by identity) reads the opponent's deck: it must become what the viewer can know (e.g. the pool's
   candidates, or nothing); `bag::OPP_POOL` must leave out the first picks; `mtch::OPP_DECK` (the
   deck's size) stays public. The `az` test that features are invariant under `determinize` must
   then hold with sampled decks.
5. `MatchPool::first_picks` (3 per generated pool) is the data part b hides; `list::FIRST_PICK`
   shows the viewer its own. Real first picks come from `team_draft`.
6. `force_chance`'s draw forcing swaps identities among the library cards "interchangeable for
   every player": still right within a sampled deck.
7. The heuristic bot reads the true `Game`: check that none of its choices reads the opponent's
   hidden cards before the deck is hidden.

### 36.11 The hidden pool (M8b part b)

Branch `m8b-hidden` (from main at fe72487): DESIGN.md 0's knowledge replaces open decklists in
best-of-three. Each player knows the opponent's pool but their first picks (one per pack: in a
team draft the only picks no other drafter sees); the opponent's current deck (which pool cards,
plus basic lands) is hidden within that pool. Best-of-one keeps open decklists: it is M5a-M8a's
game, bit for bit (`GameSpec::hidden_decks` is false; the digests are unchanged, below).

**What the viewer knows of the opponent** (`az/sample.rs`):

| | known to the viewer | hidden from it |
|---|---|---|
| their pool | every card but the first picks; the number of first picks (3 per generated pool, none for spec decks); a first pick once seen: revealed in an earlier game (`MatchState::earlier`), or known in the current game (on a public zone, looked at, an identity of a cloud, revealed this game, a revealed companion) | the unseen first picks |
| their deck in a game | its size (their cards in the game); the sideboard's size (public, `glob::SIDEBOARD`), so the number of pool cards in the deck (pool size less sideboard size) and of basic lands; their deck cards it knows exactly; its clouds' identities; what they revealed this game (`MatchState::seen`: in the deck wherever it is now); a revealed companion (in the sideboard; the deck fulfills its condition) | which pool cards, which basic lands, the order and the hand |
| between games | the last deck's size and what the last game showed (the game's final state, `seen` kept until the next game begins) | the last deck; for the second sideboarder, the first one's chosen deck; the other player's sideboarding in progress |

The viewer's own pool, deck, sideboard and hand are never resampled, nor any card it knows
exactly.

**`MtgAz::determinize(viewer)` in best-of-three** (`sample_hidden_pool`, then
`Game::determinize` as before; not when the match is over), for the opponent `o`:

1. **First picks.** The seen ones stay. The others are drawn without replacement from the prior
   `FirstPickPrior` (one per player, fixed per match, built from what the other player knows): the
   supported cards but basic lands and tokens that are neither in the known part of `o`'s pool
   nor in the viewer's own pool (the cube is singleton: a card known to be elsewhere is not a
   first pick), weighted as the generator weighs spells (creatures 3, interaction 2, other
   coloured cards 1, colourless 0.5) and x4 when playable in the colours the deckbuilder would
   choose for the known part of the pool (`pools::best_pair`; a land: when it makes or fetches
   one of them). A heuristic prior close to how the generator marks first picks (three of the
   pool's main-colour spells); to be replaced by one learned from drafts once team drafting
   exists. The sampled pool is the known part plus the seen and drawn first picks (marked as
   first picks), with the real pool's deck bounds (40..=40 for a pool's deck, section 42).
2. **The deck.** It holds the required cards (their deck cards the viewer knows exactly, by their
   own card: not a copy effect's identity, not a face; its clouds' identities; what they
   revealed this game; a revealed companion is not a deck card), has the public size N and
   M = pool cards - sideboard size pool cards; the other M - (required pool cards) pool cards are
   drawn by weight per copy: the generator's weight, x4 on the deck's colours (the prior's pair
   and the colours of the required spells and basic lands), x3 for a card revealed in an earlier
   game. A revealed companion stays in the sideboard and every other card passes its condition
   (Lurrus, Zirda: card by card; Lutri: one copy per name). The N - M basic lands: the required
   ones, the rest uniformly among the basic land types of the deck's colours that are below the
   count cap (31, `MAX_COUNT`), then among any type below it (runs/mtg4 stalled before this on a
   60-card deck of 34 basic lands in one colour pair, legal then; since section 42 a pool's deck
   is 40 cards, whose basic lands always fit in two colours, and
   `a_deck_of_basic_lands_up_to_the_cap_is_sampled` stuffs 40 basic lands, 31 of a type). (If no deck fits,
   which needs first picks crowding out the cards a revealed companion allows, the first picks
   are drawn again among cards the condition allows, then the condition is dropped; the fuzzer
   checks every sampled deck against a revealed companion and never saw it broken.)
3. **The game.** The deck's cards the viewer does not know (and any non-deck card of `o`'s it
   lost track of: a companion put into the hand and shuffled away) are dealt to `o`'s cards the
   viewer does not know (`Game::redeal`: their identities; the sideboard = the rest of the pool;
   the rare-rule bits `seen` learn new identities), then `Game::determinize` shuffles them among
   those cards as before (clouds in their domains, pinned cards kept). `MatchState::pools[o]` and `decks[o]` become the sampled
   pool and deck, so the rest of the match in that world (sideboarding, later games) plays with
   them.
4. **Between games** the same is done from the last game's final state (nothing is dealt into
   the finished game). The first sideboarder's chosen deck, hidden from the second
   (zero-core's simultaneous-choice pattern, Glass Road Zero docs/search.md 9.3), becomes the
   sampled last deck (the prior: kept; the heuristic always keeps, the network's sideboarding
   is to be seen): the second sideboarder's tree is no longer conditioned on the real choice.
   When the opponent is the one sideboarding (another viewer: tests, analysis), their
   sideboarding in progress restarts from the sampled last deck.

The real first picks and deck are read only through what the viewer knows: the pool less its
first picks is the viewer's by DESIGN.md 0, a seen first pick is a card the viewer knows.
`tests/rules_m8b_hidden.rs` checks this directly for the second sideboarder: two matches that
differ only in the first sideboarder's choice give the second the same worlds.

**Questions whose asking depended on a deck.**

* The companion reveal (103.2b): in best-of-three every player is asked in every game, `PASS`
  only when their deck fulfills no sideboard companion, and never answered internally (no
  `KEY_AUTO`): whether a player is asked, and with how many options, says nothing (a question
  asked when the pool holds a companion would tell when the only one is a first pick).
  Best-of-one asks as M8a (only with a candidate). Cost: two decisions per game (one option most
  of the time: zero-core plays it without a search).
* "Look at an opponent's hand, then choose a card name" (`NameCard`, Anointed Peacekeeper): the
  names were those of the opponent's cards (open decklists). With hidden decks: the names of
  their pool the chooser knows (`Game::known_pools`, set by the match at each game's start: the
  pool less the first picks not revealed in an earlier game, and the basic lands), of their cards
  the chooser knows in the game and of its clouds' identities; the same in every world of the
  chooser's.
* Three questions were asked because a player's deck has a card: the miracle reveal (on every
  first draw, a deck with a miracle card: Metamorphosis Fanatic), Gemstone Caverns' opening
  action, splice (a deck with a card that splices onto the spell's subtype: Goryo's Vengeance,
  Through the Breach). Asked so, they tell the opponent what the deck holds (who decides next is
  public to zero-core: a node's signature), and a world whose sampled deck differs runs past the
  real decision (the match fuzz found it: a refresh of the opponent's opening question ran on to
  the next decision). With hidden decks they are asked when the part of the player's pool the
  other player knows has such a card (`Game::deck_question` over `known_pools`), whatever the
  deck holds: the same in every world. The cost: a first pick the opponent has not seen before
  the game does not enable them (a Gemstone Caverns or Metamorphosis Fanatic first-picked and
  unseen gives no opening action or miracle in that game); one card each, rare.

**Keys.** A later game's setup now shows something the hidden sideboarding decided: each
player's deck and sideboard sizes. The match emits a public key `KEY_GAME` (kind 7, player 0,
payload `deck0 | deck1 << 8 | side0 << 16 | side1 << 24`) as the game begins, so two worlds of a
viewer whose opponent built decks of different sizes are told apart (the match fuzz found two
such worlds with equal keys and different encodings); it replaces 36.1's "no chance key marks a
game boundary" for best-of-three. The play-or-draw choice and its chooser are public as before.

**A leak in the game the match fuzz found** (best-of-one too): turning a manifested card face up
(116.2b) paid its mana cost before the card was revealed, so the payment showed the hidden card's
cost (the context block's cost) while the worlds still agreed on every key. CR 701.40b: "show all
players that the card representing that permanent is a creature card and what that card's mana
cost is, pay that cost, then turn the permanent face up": the card is now revealed when the
special action begins (`reveal`: known to both, a public `KEY_REVEAL`), before the payment, and
again as it turns face up (as a hand card activated, 602.2a, then cast). This changes best-of-one
games that turn a face-down card face up: 50 of the 2,500 digest games (below).

**Encoding** (layout v4 unchanged: 33,652 floats; best-of-one encodings unchanged):

* `bag::OPP_POOL`: the known part of the opponent's pool (no first pick not revealed in an
  earlier game).
* `bag::OPP_HIDDEN`, in a game: the candidates, the known part less their cards the viewer knows
  in the game (exactly or by its clouds) and a revealed companion still outside; basic lands left
  out. `Game::encode_features` no longer reads the opponent's unknown cards when
  `hidden_decks` (the match writes the bag).
* While sideboarding, the other player's context shows the kind `Sideboard` whatever the
  decider's decision (a `SideboardCount` would tell that a basic land or a card with copies was
  chosen).

The `az` feature-invariance test runs on best-of-one states; the same check on best-of-three
states (both viewers, sideboarding included) is in `tests/rules_m8b_hidden.rs` and in the match
fuzz (`tests/az.rs` is not run on this branch).

**Pools.** `pools::random_pools` makes the second pool from the cards the first does not hold
(`random_pool_without`; the cube is singleton): a first pick is never in the other player's pool,
as the prior assumes. The first pool, `random_pool` and the deckbuilder are unchanged; a match's
second pool and so the evaluation pools' second seats change (best-of-three only; nothing is
trained on them yet).

**`force_chance`.** Unchanged rule: a forced draw or look swaps the named identity in from a
library card interchangeable with the one seen for every player, which stays right within a
sampled deck. A forced identity the world's deck lacks falls back to the world's outcome: a swap
between deck and sideboard would change what the owner knows of their own library (an owner
knows its library's contents), so it is not done. The decider's own draws come from its real
deck, so its keys replay as in best-of-one: 334 of 378 forced transitions with the decider's
draws or looks reproduced all their keys (88%; `force_chance_replays_the_deciders_draws_in_matches`).

**The heuristic bot.** It reads the true `Game`; no read of the opponent's hidden cards was found
(its reads of the opponent are life, permanents and creatures; options name cards the decider
knows). Test: at 5,382 decisions of best-of-three matches its choice is the same in three worlds
of the decider (the opponent's first picks, deck, hand, library and sideboard resampled, its own
library order too).

**Tests** (`tests/rules_m8b_hidden.rs`, 7): the first picks drawn from the prior (never in the
known part or the viewer's pool, mostly in the prior's colours, varying; the real ones kept in at
most one of 240 worlds) and seen ones kept (2,115 worlds); sampled decks with the public sizes
holding every card the viewer knows, in games and between games, other than the real deck in
every world (2,970, 90 of them between games); the second sideboarder blind to the first's choice; the companion question
asked of everyone (240 questions in 120 games, 204 with `PASS` only) and every sampled deck
fulfilling a revealed companion (210 worlds); the bags (no unseen first pick, `OPP_HIDDEN` within
`OPP_POOL`, every feature the same in the viewer's worlds: 3,052 worlds, both viewers); the bot;
`force_chance`. `tests/rules_m8b.rs`: `OPP_POOL` is the known part.

**Fuzzing.** The match fuzz now checks, every ~40 actions, `determinize` for both viewers (in
games and while sideboarding): the decider's actions (or the decider kept, for the other viewer),
the viewer's whole encoding, the viewer's pool, deck and sideboard untouched, the known part of
the opponent's pool kept and as many first picks (none in the viewer's pool), the deck's size and
copies, a revealed companion fulfilled, this game's reveals in the deck, the world's cards equal
to the sampled deck and its sideboard to the rest of the pool, the sideboard's size, the view
(`view_of`), no known card resampled, the invariants and the clouds (`check_knowledge`); then 30
random actions in the world (across game ends and sideboarding); and for the decider, one time in
four, two worlds played with the same actions stay indistinguishable while their keys, next
decider and legal actions agree (zero-core's child signature).
`FUZZ_GAMES=20000 cargo test --release -p engine --test fuzz`: no failure, 165 s; the 20,000 games
as before and 1,000 matches (2,579 games, 579 matches of three, 754 drawn games, 418.6 decisions
per match, 20,721 sideboarding decisions, 3,099 decks changed) with 20,970 sampled worlds: the
first picks other than the real ones in 11,934 (the rest: M8a's decks without first picks, or all
first picks seen), the deck other than the real one in 20,945. Also clean: `FUZZ_FIRST=2000
FUZZ_MATCHES=6000` (124,544 worlds). Found and fixed on the way: a later game's deck sizes unkeyed
(`KEY_GAME`), turning face up paid before the reveal, the opening question asked by the deck, the
sampled deck checked by its slots (a companion the viewer lost track of may be dealt to a deck
card's slot: the check counts the player's cards and the companion).

`cargo test --release -p engine --no-fail-fast --lib --test 'cards*' --test 'rules*' --test hidden
--test support --test smoke --test 'bots*'`: 907 pass (part a's 900 and the 7 new), 7 ignored.
`bash tools/build_py.sh`, `python -m pytest -q tests`: 20 pass (the layout test included: the
layout is unchanged). Search smoke (`python tools/search_smoke.py`, seeds 0-3, specs random, eval,
match): 0 failed of 12 configurations, 1,189 s (random 64-66 s, eval 67-71 s, match 155-164 s:
1,600 matches, 1,499,267 decisions, 39.8 million simulations); `--specs match_eval --seeds 0-1`:
0 failed of 2 (800 matches, 739,587 decisions), 310 s.

Behaviour digests (`cargo test --release -p engine --test digest -- --ignored`): best-of-one is
unchanged but for the face-up reveal: without that change the 2,500 games give part a's
digests exactly (`DIGEST_ENCODING=0` 3fb15384404c7428; with the encodings 63d5bd84a9f7c19b, every
game equal to the recorded file); with it 50 games differ (`DIGEST_ENCODING=0` e4783eb5c6705fc3).
Re-recorded (`DIGEST_WRITE=1`, this branch's last code commit): combined digest 646d779580df7517.

**Speed** (`cargo test --release -p engine --test perf match_determinize -- --ignored
--nocapture`, the desktop, unpinned, two runs): clone + `determinize(to_move)` through `MtgAz`,
states of heuristic-against-random play one every ~7 decisions: best-of-one 1.61-1.79 us (2,745
states; clone 1.11-1.20 us), best-of-three 6.71-6.72 us (4,599 states, games and sideboarding;
clone 1.17-1.20 us): the sampling adds ~5 us per world (the first picks, the deck's weighted
draws, the sampled pool and deck built, the deal), against a simulation's encoding (~10 us) and
network evaluation. Two costs found on the way and cached per card definition (the `seen` bits of
a new identity, the generator's weights and colours): 9.2 -> 6.7 us.

### 36.12 Drawn games in the Magic Tournament Rules; temperature in a match

**Drawn games.** MTR 2.1 (Match Structure, read 2026-10-25 at blogs.magicjudges.org/rules/mtr2-1):
"A match consists of a series of games that are played until one side has won a set number of
games, usually two. Drawn games do not count toward this goal. If the round ends before a player
has won the required number of games, the winner of the match is the player who has won the most
games at that point" (equal game wins: a drawn match). MTR 2.2 (Play/Draw Rule): "If the previous
game was a draw, the player who decided to play or draw at the beginning of the drawn game
chooses." Implemented (36.1): a match ends at two wins or after three games, a drawn game counts
as one of the three, the result is by game wins (equal: a draw), and after a drawn game its
chooser chooses again. The MTR's match plays on after a draw until two wins or the round's time;
the fixed three games stand in for the time limit (a self-play match has no clock), and the
result rule is the MTR's end-of-round rule. The play/draw rule is the MTR's. Kept: not clearly
wrong for a fixed-length self-play match.

**`temp_moves` for `mtg-4`.** zero-core samples the move by visit count for the first
`temp_moves` searched decisions of an episode (`slot.decisions`, every `decide` of the slot,
both seats, minor payment decisions included; reset only at a new episode), then plays the most
visited move; root Dirichlet noise (`alpha`, `eps`) applies at every searched decision of
self-play. In a match the 60 of `mtg-3` cover game 1's mulligans and first turns only; games 2
and 3 and the sideboarding are played greedily. No setting of the pinned zero-core covers each
game's opening without also covering most of game 1 (a game is ~400 decisions): a larger value
puts temperature on game 1's whole middle (noisier value targets), a smaller one less on game 1.
Proposal: keep `temp_moves` 60 for `mtg-4`. Later games and sideboarding still vary: new
shuffles, decks that differ after sideboarding, root noise at every searched decision (eps
0.25), and the completed-Q policy target (`cq`) trains every option of a searched decision, not
only the one played. To watch in the first run: the share of sideboardings that change the deck
and the spread of game-2 openings; if sideboarding collapses to "keep", raise `eps` for the run
rather than `temp_moves`, or add a zero-core option that counts temperature decisions per game
(a `SearchGame` hook marking a new game; a zero-core change, not made here).

## 37. The cube's official rulings as tests (M8d part 1)

Every cube card has its own Oracle-text test (sections 23-32); what those don't reach is how
cards behave together. The official rulings of the cube's cards are the cheapest list of such
interactions written by the rules managers, so M8d turns them into engine tests.

**37.1 Data.** `tools/fetch_rulings.py` fetches each card's `rulings_uri` (Scryfall; cached per
card in `data/rulings/.cache/`, gitignored; ~8 requests/s with a User-Agent) into
`data/rulings/powerlsv.rulings.json`: 546 cards, 1,607 rulings (1,602 WotC, 5 Scryfall), 1,260
distinct texts (boilerplate repeats across cards: shock lands, flashback, the initiative); 171
cards have none. A ruling is named by `hash8`, the first 8 hex digits of the SHA-1 of its text.

**37.2 Triage.** `data/rulings/triage.tsv`, one row per distinct text: class **a** (testable
here: two players, the cube's cards), **b** (not applicable: multiplayer, Commander, tournament
and deck construction, printing and wording, a bare definition) or **c** (a mechanic or
situation no cube card reaches), with an area (copies, replacement, protection, facedown, dfc,
walkers, stack, triggers, mana, combat, zones, other) or a reason. The first pass was made by a
model with that rubric; the b and c rows were reviewed (one b, "to play a card", is tested and
now a). Class a is generous: 1,193 distinct texts (1,507 rulings), many of them restating what
the Oracle tests already check; part 2 should narrow it as it goes. `tools/rulings_doc.py` joins
the rulings, the triage and the `// ruling: <hash8>` comments of the tests into `docs/rulings.md`
(per card, per ruling: class, area or reason, test or status; the coverage table), and fails on
a comment naming an unknown ruling; `#[ignore = "RULING: ..."]` marks a deferred one.

**37.3 Tests.** 97 new tests in nine `tests/rulings_*.rs` files (section 15), one or a few
rulings each, written first for the risky interactions (copies and layers, replacement effects
and their order, protection and targeting, face-down cards, double-faced cards, planeswalkers,
the stack, triggers whose source is gone, costs); 15 existing tests that already checked a
ruling now name it. Coverage after part 1: 130 rulings (126 distinct texts) tested, all passing,
none deferred; 1,377 class-a rulings (1,067 distinct) untested. By area (distinct, tested/class
a): facedown 10/17, walkers 4/10, protection 6/29, replacement 15/70, copies 16/110, stack
17/133, triggers 18/166, mana 17/213, dfc 6/72, zones 8/142, combat 5/65, other 4/166.

**37.4 What the rulings found: three engine faults, fixed.**

* *Containment Priest entering with other creatures* (ruling e9e31107; test
  `rulings_replacement::containment_priest_entering_with_other_creatures_doesnt_exile_them_or_itself_cr_614_12`).
  Exhume returning the Priest and the opponent's creature exiled that creature: the "nontoken
  creature that wasn't cast" replacement looked at every Priest on the battlefield, including one
  put there by the same event a moment before. A permanent's static ability applies only from the
  battlefield (CR 113.6), so a Priest entering at the same time doesn't see the others (614.12;
  the rule of Phyrexian Metamorph's choice, `EnterAsCopy`): the check now skips a Priest whose
  timestamp is from the current event group (`group_ts`).
* *One draw under Hullbreacher and Quantum Riddler* (ruling 60f3ab6b; test
  `rulings_replacement::hullbreacher_and_quantum_riddler_on_one_draw_the_drawer_gets_hullbreacher_first_cr_616_1`).
  Both replace the same single draw and the drawing player chooses the order (CR 616.1). The
  engine always applied Quantum Riddler first ("draw two"), then Hullbreacher to each: two
  Treasures for the opponent. Hullbreacher first leaves no draw for the Riddler: one Treasure.
  Neither order draws a card, so the engine now takes Hullbreacher first for the drawer (the
  other order only gives the opponent a Treasure more; no cube card rewards that), without a
  decision. For more than one card the Riddler applies to the instruction first, then
  Hullbreacher to each draw (rulings 5a48fcc2, d9d5b8ea; unchanged, tested).
* *Kaito, Bane of Nightmares on his turn* (ruling f0224bc7; tests
  `rulings_walkers::kaito_as_a_creature_is_dealt_damage_not_loyalty_loss_cr_306_8` and
  `rules_walkers::cr_205_1a_kaito_is_a_creature_not_a_planeswalker_on_his_turn_and_damage_is_marked`).
  "During your turn, as long as Kaito has one or more loyalty counters on him, he's a 3/4 Ninja
  creature" was lowered as a creature *in addition to* a planeswalker, so damage to him removed
  loyalty (and `rules_walkers.rs` pinned that). Without "in addition to its other types" the card
  types and subtypes become exactly these (CR 205.1a-b; the ruling: he stops being a
  planeswalker): `Mod::Types { replace: true }` on his own static, and `types_of` /
  `has_subtype` honour `replace` for a permanent's own type-changing static. Damage to him on his
  turn is marked (120.3e); he keeps his loyalty abilities (606.3 doesn't ask for the type).

**37.5 What held.** Everything else tested passed on the engine as it was, including: copies of
tokens, of animated manlands (the animation isn't copied), of a Saheeli copy (the artifact
exception is copiable); X as 0 for Metamorph, Spider-Man, Shifting Woodland, Reflection and
Reanimate; Equipment that becomes or stops being one; replacement order chosen by the affected
player; infect against indestructible, planeswalkers and protection from colorless; Dauthi's
free casts paying additional costs and following timing; Yawgmoth's Will on costs and on itself;
Containment Priest exiling a manifested card; Emrakul targeted on the stack by a counterspell,
destroyed by a coloured permanent's trigger, returned by Sink into Stupor, exiled by Aang, put
into its library by Subtlety, still giving Mana Drain its 15 mana; annihilator taking the
attacked planeswalker; a countered "next end step" trigger never returning Memory Jar's cards;
Tidebinder leaving before its trigger resolves; storm counting countered spells, and a countered
storm trigger; Lutri's copy not being cast; Chain Lightning's copy controlled by its copier;
Thalia's tax on alternative costs, and Daze returning the Island it tapped for that tax (CR
601.2g-h); manifest dread with one or no card; turning a face-down permanent up (no enter
trigger, still tapped and attacking, Equipment stays); Headliner Scarlett's card after she
leaves; daybound at night.

**Checks.** Every engine test binary but `az` (73 binaries, 1,005 passed, 7 ignored as before);
20,000 fuzz games over the 546 cards and the custom cards, no failure (196 s; matches and replays
too); `tools/build_py.sh` and `tools/search_smoke.py --specs random,match --seeds 0-1` (4 x 400
games, 0 failed, 592 s); the digests re-recorded (the three fixes change some games).

**37.6 Deferred, IR.** Nothing deferred: no test is ignored. No ruling showed a wrong IR (Kaito's
`creature_during_your_turn` says nothing about "in addition to"; the reading was the lowering's,
`docs/ir_defects.md`).

**37.7 Part 2 starts here.** `docs/rulings.md`'s `untested` rows, by area (the counts above),
first the risky ones left: divided damage with a target gone illegal (Fury, Pyrokinesis, Fire
Covenant, Shatterskull Smashing: 45abd910, 17a878c0, 7afb4bdb, d70bd868), copies of modal and
split spells (Lutri 23cf3168, 7be1b615; Three Steps Ahead d4f44dfc; Mystic Confluence 3ef23eda),
Invert Polarity's variable targets (747157aa, c3434e5c), Reflection of Kiki-Jiki's copies of
copies (b30727cf, 3f6b90f2), Dauthi and a discarded card (8d9db447), Etali's and Bolas's
Citadel's casts, converge on copies and free casts (Prismatic Ending 41553eae, fc8abf83), the
prepare cards (Emeritus), then triggers, zones and mana by number. Then the interaction matrix
and combo-dense fuzz decks (the M8d plan).
Part 2 (section 38) tested this list (everything in it held) and the riskiest rulings about
triggers, zones and mana; coverage after part 2: 218 rulings (203 distinct texts) tested, none deferred.
Part 3 (section 39) narrowed the triage and tested the replacement, copies, stack, triggers,
protection, dfc and walkers areas: 913 rulings (726 distinct texts) tested, 313 (251 distinct)
untested, in zones, combat, mana and other (39.1). Part 4 (section 40) tested those four areas;
coverage now: 1,225 rulings (976 distinct texts) tested, none deferred, none untested (40.1).

## 38. Interaction confidence (M8d part 2)

Part 1 (section 37) turned the cube's official rulings into tests; part 2 tests how cards behave
together where two subsystems written in different milestones meet: the risky rulings 37.7
left, more rulings about triggers, zones and mana, a matrix of mechanic pairs
(`docs/interactions.md`) with tests for its riskiest cells, and a fuzz mode whose decks are
built from cards that interact. Branch `m8d-interact`.

**38.1 Tests.** 77 new tests in eight files (section 15), every one with the CR in its name and,
where it checks an official ruling, the ruling in a `// ruling:` comment:

| File | Tests | What |
|---|---:|---|
| `tests/rulings_stack2.rs` | 13 | damage divided as a spell is cast or a trigger is put on the stack, a target gone illegal (Fury, Pyrokinesis, Fire Covenant, Shatterskull Smashing: 601.2d, 603.3d, 608.2b); Invert Polarity changing the targets of a divided spell but not their number or the division (115.7); Lutri's copies of modal (Fiery and Mystic Confluence), spree (Three Steps Ahead), X and divided (Fire Covenant) spells and of a spell without targets (707.10) |
| `tests/rulings_casts.rs` | 14 | Etali's free casts in its trigger's resolution (a later spell targeting an earlier one but not the permanent it will be, a sorcery with a spell on the stack, X = 0, no alternative cost but mandatory additional costs, the cards left in exile: 608.2g, 107.3b, 118.9a, 601.2f); Bolas's Citadel (X = 0, no other alternative cost, the land drop, timing); Dauthi Voidwalker and a discard (Inti, Occult Epiphany); converge counting no colours on a copy and on a free cast (Prismatic Ending) |
| `tests/rulings_copies2.rs` | 9 | Reflection of Kiki-Jiki copying a Metamorph copy (707.9b) and a token (707.2), no counters or attachments, the token's own enter trigger; Emeritus of Abundance prepared once, being prepared not copiable (Saheeli's -2), kept through Oko's Elk, the land count checked again on resolution (603.4), the Regrowth copy cast by its new controller after Oko's exchange (722.3c) |
| `tests/rulings_triggers2.rs` | 19 | sources gone before their abilities resolve (Palace Jailer's monarch return, Leyline Binding, Animate Dead, Flickerwisp, Shallow Grave: 610.3c, 603.7, 608.2b); Palace Jailer exiling a token (111.7); Uro reanimated; Green Sun's Zenith stolen by Invert Polarity and countered; "exile until" one card at a time (Laelia and cascade); magecraft for copies; Leovold once per target; Screaming Nemesis blocked by two creatures; Coveted Jewel and a planeswalker; City of Traitors and a fetched land; Celestial Colonnade becoming a creature; flashback exile after Counterspell and Remand; Oust and Unexpectedly Absent's library positions |
| `tests/interactions_copies.rs` | 6 | a copy of Oko's Elk (layer 1 against 4-7), a copy and a token copy of a face-down creature (708.2), of True-Name Nemesis (its own choice, 707.5); Containment Priest and a reanimated Metamorph entering as a copy of an artifact (614.12, 616.1); a spell copy retargeted at a ward creature (707.10c, 702.21a) |
| `tests/interactions_costs.rs` | 5 | Bolas's Citadel's life kept when a spree mode or a kicker is offered (601.2g-h); Thalia's tax on Etali's free casts and on the Citadel's casts (601.2f); a Lotus Petal exiled by Yawgmoth's Will as a graveyard exile cost (614.1a) |
| `tests/interactions_faces.rs` | 1 | a split card cast from a graveyard with two permissions (709.3, 601.2b) |
| `tests/interactions_zones.rs` | 10 | Flickerwisp on a manifested creature, a manifested instant, a transformed card, an MDFC's land face (ruling 47775ed7) and a reanimated creature (400.7, 708.9, 712.14, 304.4, 307.4, 704.5m); Leyline Binding flickered (610.3); an exchanged creature's summoning sickness and "its controller" (302.6); Fractured Identity on a planeswalker (306.5b) and on a face-down permanent; trample over a blocker with protection (702.19c) |

Four existing tests now name rulings they already checked (Bolas's Citadel's sacrifice and top
card, Emeritus's copy in exile, Nadu's per-creature limit). Rulings coverage (`docs/rulings.md`):
218 rulings tested (203 distinct texts; part 1: 130, 126), none deferred; by area (distinct,
tested/class a): mana 26/213, triggers 42/166, zones 23/142, stack 35/133, copies 23/110, dfc
8/72, replacement 15/70, combat 5/65, protection 6/29, facedown 10/17, walkers 4/10, other 6/166.
The triage wasn't narrowed (no class-a row was reclassified). (Part 3 narrowed it and tested seven areas: 39.1; part 4 the last four: 40.1.)

**38.2 What broke, and why** (each fix has a test that failed before it, checked):

1. *"Exile until" exiled its cards as one event.* Cascade (Bloodbraid Challenger), Etali's
   trigger and the exile-until-nonland effect moved every card to exile within one event
   group, so Laelia ("whenever one or more cards are put into exile from your library")
   triggered once. Ruling 1178d637: the cards are exiled one at a time, a trigger each (CR
   603.2c). Each card is now its own event (`new_event` before each move). Test
   `rulings_triggers2::laelia_cascade_exiles_one_card_at_a_time_one_trigger_each_cr_702_85a_603_2c`.
2. *Combat damage from several sources was several "is dealt damage" events.* Screaming
   Nemesis blocked by two Bears triggered twice (2 and 2). Ruling 66fc62ef: damage dealt at
   once (CR 510.2) by several sources triggers it once, for the total. The combat damage step
   now emits one dealt-damage event per recipient with its total (`combat.rs`). Test
   `rulings_triggers2::screaming_nemesis_dealt_damage_by_two_blockers_triggers_once_cr_603_2c_510_2`.
3. *A split card cast from a graveyard with two permissions panicked* ("a castable card"):
   Life // Death with Yawgmoth's Will and Snapcaster Mage's flashback. The permissions were
   checked against the whole split card (no route castable) while the offer had checked the
   half; a split card is cast as one of its halves (CR 709.3), now chosen before the
   permissions. Found by the interaction fuzz (seed 35744). Test
   `interactions_faces::a_split_card_with_two_graveyard_permissions_casts_its_left_half_cr_709_3_601_2b`.
4. *Bolas's Citadel's life wasn't kept when an additional mana cost was offered.* A spell cast
   from the top pays life equal to its mana value after its mana (CR 601.2g-h). The offers of
   spree modes, squad / offspring / replicate counts, kicker X, X and splice (`can_pay_spell`)
   and of cost options such as kicker (`cast_option`, `options_payable`, with `with_life`) didn't
   keep that life, so at low life Three Steps Ahead's +{3} and Burst Lightning's kicker were
   offered when they needed Mana Confluence's life too, and the payment panicked. A new
   `Game::life_later` (X life and the Citadel's) is kept by these checks and used by the
   payment. Found by the interaction fuzz (seeds 40409, 92590). Tests
   `interactions_costs::citadel_spree_mode_costs_keep_the_life_the_citadel_takes_cr_601_2h_702_172a`,
   `::citadel_kicker_keeps_the_life_the_citadel_takes_cr_601_2h_702_33a`.
5. *Containment Priest exiled a Metamorph entering as a copy of a noncreature.* Phyrexian
   Metamorph returned by Reanimate was exiled before its copy choice. "You may have this enter
   as a copy" is a replacement effect too: its controller applies it first (616.1) and the
   Priest's applies only if what would enter is still a creature (614.12). The Priest's check
   for a card entering as a copy now waits for the copy choice (`Mark::PriestPending`) and
   exiles it then if it is still a nontoken creature, without leave events (it never entered:
   `Game::quiet_leave`). No decision for the order: the other order's outcome (exiled) is
   reached by copying a creature or nothing (`docs/shortcuts.md` 38.1). Test
   `interactions_copies::containment_priest_and_a_reanimated_metamorph_entering_as_an_artifact_cr_614_12_616_1`.
6. *An instant or sorcery card could be put onto the battlefield.* Flickerwisp returning a
   manifested Lightning Bolt made it an instant permanent. CR 304.4, 307.4: it stays in its
   previous zone (exile). `put_onto_battlefield` now leaves such a card where it is (unless it
   enters face down, or as a land face an effect names); the same holds for a modal double-faced
   card returned front face up (ruling 47775ed7: Shatterskull, the Hammer Pass flickered, or
   returned by Badgermole Cub's earthbend, stays where it was). Tests
   `interactions_zones::a_flickered_face_down_instant_cant_return_and_stays_exiled_cr_708_9_400_4a`,
   `::a_flickered_mdfc_land_face_returns_front_face_up_and_a_sorcery_cant_enter_cr_712_14_307_4`.
7. *A Lotus Petal exiled by Yawgmoth's Will counted toward a graveyard exile cost.* Since M7e a
   self-sacrificing mana source counts as a graveyard card for delve and "exile n cards from
   your graveyard" (it is sacrificed while paying, 601.2g, before 601.2h); under Yawgmoth's Will
   (or an opponent's Dauthi Voidwalker) it is exiled instead (614.1a), so Abhorrent Oculus was
   offered with five cards and the Petal, and its exile cost had no card to choose (panic). Only
   sources whose move to the graveyard isn't replaced count now (`delve_sources`). Found by the
   interaction fuzz (seed 79031). Test
   `interactions_costs::a_lotus_petal_exiled_by_yawgmoths_will_cant_feed_a_graveyard_exile_cost_cr_614_1a_601_2g`.

**38.3 What held.** Everything else passed on the engine as it was: divided damage keeping its
division when a target goes (and doesn't resolve when all do), at least 1 per target, X = 0 with
no target; Invert Polarity's steal keeping the number of targets and the division; copies of
spells keeping modes, X, the division and the costs paid, with new targets; Etali's casts
(ordering, timing, X, alternative and additional costs, exile); the Citadel's X, alternative
costs, land drop and timing; Dauthi's exile still a discard; converge 0 on copies and free casts;
copies of copies, of tokens, of an Elk, of a face-down creature, of True-Name Nemesis; the
prepare rulings; sources gone before their abilities resolve; Green Sun's Zenith cast by its
owner's opponent (searching their library, shuffled into the owner's); magecraft on copies;
Leovold once per target; Coveted Jewel and planeswalkers; City of Traitors; Colonnade; flashback
exile; Oust and Unexpectedly Absent; Thalia's tax on free and Citadel casts; flicker of
face-down, transformed and reanimated creatures and of Leyline Binding; Oko's exchange and
summoning sickness; Fractured Identity on a planeswalker; trample over protection.

**38.4 The interaction matrix.** `docs/interactions.md`: 16 mechanics (copies, layers,
replacement effects, protection and ward, face-down, faces, planeswalkers, the stack, sources
gone and last known information, the new object, costs, mana abilities while paying, combat,
state-based actions, control changes, day and night), every reachable pair with its tests
(earlier milestones' and part 2's) or `n/a` with the reason. Left `untested`: a token copy of a
daybound card at night (whether such a token is double-faced, CR 707.8a and 712.10, wants a
careful reading first) and True-Name Nemesis's chosen player after a control change. (Both
tested in part 3, 39.5: no cell is left untested.)

**38.5 The combo-dense fuzz mode.** `FUZZ_POOL=interactions` (`tests/fuzz.rs`): thirteen groups
of cube cards (`GROUPS`; fourteen since section 49.4), one per family of the matrix's cells:
copies and enter/leave abilities; flicker and linked exile; reanimation and replacement effects;
face-down cards; double-faced cards, Adventures and split cards; counterspells and copies of
spells; control
changes; taxes, alternative and additional costs and free casts; damage, protection, deathtouch,
infect and divided damage; planeswalkers; last known information, triggers and state-based
actions; combat keywords; day and night. A deck takes 17 random lands and 23 spells from two or
three groups (one in eight from all the groups); a sideboard of seven from its groups. Single
games use it for every seed, matches (best-of-three on the two lists, open decklists, one in four
with a 12-turn cap) too; every check of the default mode runs (options completed, clones,
determinize and information sets, invariants, replays). The decision kinds asked are counted
(per 1,000 decisions, interaction mode against the default, 20,000 games each):
NewTarget 0.27 -> 0.35, DivideDamage 0.22 -> 0.79, ChooseExiled 0.35 -> 0.92,
ChoosePermanent 7.54 -> 15.60, OrderTriggers 4.63 -> 7.74, ChooseAltCost 1.24 -> 1.61, Target
53.81 -> 59.78 (Sacrifice 3.90 -> 2.78, ChooseGraveyardCard 2.70 -> 2.33, ChooseMode 7.73 ->
7.35, YesNo 22.06 -> 21.97): copy choices and simultaneous triggers twice as often, divided
damage and cards cast from exile three times. An unknown `FUZZ_POOL` fails; unset, the default
mode is unchanged. Three of the seven faults above were found by this mode (3; 4, at two seeds;
7), each in code the default fuzzer had passed 20,000 games on since it was written. Over the
session the mode played 132,591 games (seeds 0-132,590: a run stopped at a failing seed, the
fault was fixed, the run resumed after it, and the four failing seeds pass now) and 12,000
matches.

**38.6 Checks.** Every engine test binary but `az` (84 binaries: 1,086 passed (part 1's 1,005, the 77 new, fuzz's 4), 11 ignored (part 1's 7, `perf`'s 3 and `digest`'s measurement runs)); `FUZZ_GAMES=20000` in the
default mode: no failure, 163 s (20,000 games, 214.0 decisions per game, 5.65 million options
completed; 1,000 matches, 2,574 games, 20,886 sampled worlds); `FUZZ_POOL=interactions
FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure, 187 s (20,000 games, 206.2 decisions per game,
5.43 million options completed; 2,000 matches, 5,157 games, 1,157 of three, 41,865 sideboarding
decisions, 44,630 sampled worlds); `bash tools/build_py.sh` and `python tools/search_smoke.py
--specs random,match --seeds 0-1`: 0 failed of 4 configurations, 524 s (random 78-82 s; match 169-185 s: 800 games, 747,786 decisions, 19.8 million simulations); `python tools/rulings_doc.py --check` passes,
`python tools/untested_cards.py`: 0 of 546.

**38.7 Digests and speed.** Re-recorded (`DIGEST_WRITE=1`): 13 of the 2,500 games change, each
explained by a fix: 11 hold Screaming Nemesis or Jennifer Walters (one dealt-damage event per
recipient in combat) or Laelia with cascade or Etali (exile-until one card at a time); r 758
(Badgermole Cub's earthbend returning Shatterskull, the Hammer Pass, whose front face can't enter)
and r 1243 (Parallax Wave returning a manifested Force of Will) by fix 6. Combined digest
e80db1ef6fad15cb. Speed: the fixes add work only where they apply (a combat damage step's
recipients, an exile-until loop's event per card, a type check per card put onto the battlefield,
the casting choices' life reserve); `Game` grows by one `bool` (`quiet_leave`; the Priest's
pending check is a `Mark`). `tests/perf.rs` `measure`, this branch against its base 7a65ad8 interleaved
in two rounds (the desktop, unpinned, the same day): 6.77 and 6.68 us per decision against 6.69
and 6.72 (702 and 712 games/s against 710 and 707); `Game::clone` 1.34 and 1.24 us against 1.26
and 1.26; clone + `determinize` 1.83 and 1.74 us against 1.74 and 1.77; `size_of::<Game>()` 1,600
B both: no change beyond the noise.

## 39. The remaining rulings (M8d part 3)

Parts 1 and 2 (sections 37, 38) tested the riskiest official rulings and the interaction matrix;
part 3 narrows the triage, tests what is left area by area (replacement, copies, stack,
triggers, protection, dfc, zones, combat, facedown, walkers, mana, other), and tests the
matrix's last two cells. Branch `m8d-rulings2` (from 48ccbb9).

**39.1 Narrowing the triage.** Every class-a text untested after part 2 (990 distinct) was
decided once: *already tested* (an existing test checks it: the test now names it with a
`// ruling:` comment; three tests got one more assertion: Stoneforge Mystic's Equipment enters
unattached, Life's lands keep their subtypes and mana abilities, Lion Sash loses its creature
type while attached), *reclassified b* (a definition, multiplayer, tournament, printing) or *c*
(a situation no cube card reaches), each with its reason in `triage.tsv`, or *to test*. Two more
were reclassified while testing (a3e958c5 c: Hexdrinker's level up under Oko's Elk can't happen;
b1e42bd2 b: Sword of the Meek's trigger event is a definition). `tools/rulings_doc.py` splits
the tested rulings into those a rulings suite tests (`rulings_*`, `interactions_*`) and those
only an earlier Oracle or rules test names.

Distinct texts per area (class a before part 3; of the untested: already tested, reclassified
b, reclassified c, to test; of those to test: tested in step 2, left; class a and tested after):

| Area | a before | tested before | untested | already tested | -> b | -> c | to test | tested now | left | a after | tested after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| replacement | 70 | 15 | 55 | 12 | 1 | 10 | 32 | 32 | 0 | 59 | 59 |
| copies | 110 | 23 | 87 | 12 | 1 | 12 | 62 | 61 | 0 (1 -> c) | 96 | 96 |
| stack | 133 | 35 | 98 | 10 | 3 | 4 | 81 | 81 | 0 | 126 | 126 |
| triggers | 166 | 42 | 124 | 18 | 4 | 22 | 80 | 79 | 0 (1 -> b) | 139 | 139 |
| protection | 29 | 6 | 23 | 2 | 0 | 2 | 19 | 19 | 0 | 27 | 27 |
| dfc | 72 | 8 | 64 | 17 | 0 | 7 | 40 | 40 | 0 | 65 | 65 |
| zones | 142 | 23 | 119 | 21 | 8 | 11 | 79 | 9 | 70 | 123 | 53 |
| combat | 65 | 5 | 60 | 6 | 3 | 10 | 41 | 3 | 38 | 52 | 14 |
| facedown | 17 | 10 | 7 | 3 | 1 | 3 | 0 | 0 | 0 | 13 | 13 |
| walkers | 10 | 4 | 6 | 2 | 0 | 1 | 3 | 3 | 0 | 9 | 9 |
| mana | 213 | 26 | 187 | 43 | 23 | 17 | 104 | 9 | 95 | 173 | 78 |
| other | 166 | 6 | 160 | 35 | 44 | 27 | 54 | 6 | 48 | 95 | 47 |
| **total** | **1,193** | **203** | **990** | **181** | **88** | **126** | **595** | **342** | **251** | **977** | **726** |

("Tested now" in zones, combat, mana and other: rulings a test of another area checks too.)
`docs/rulings.md` after part 3: 1,607 rulings (1,260 distinct): a 1,226, b 199, c 182; tested
913 (726 distinct), deferred 0, untested 313 (251 distinct).

**39.2 Tests.** 122 new tests (120 in seven new files, two in part 2's interaction files),
each with the CR in its name and its rulings in `// ruling:` comments, on real cube cards:

| File | Tests | What |
|---|---:|---|
| `tests/rulings_replacement2.rs` | 21 | ravenous and copies, reconfigure and Equipment made creatures, Archon of Emeria against fast and shock lands, Elspeth's doubling with create-and-attach and tapped-and-attacking tokens, Portable Hole and Animate Dead, Narset / Leovold with Hullbreacher, Memory Lapse on flashback, finality counters, Necromancy, bestow, Sword of the Meek, Mawloc (614, 616.1, 303.4f) |
| `tests/rulings_copies3.rs` | 24 | Guide of Souls and energy, Ocelot Pride, characteristic-defining abilities in every zone, copies of kicked, saddled and dashed permanents and spells, token copies' copiable values, Vehicles made creatures, backup, Oko, earthbend, Painter's Servant, Figure of Destiny (707.2, 707.9, 604.3, 613.4b) |
| `tests/rulings_stack3.rs` | 21 | targets gone (608.2b), no priority inside a resolution or a cast, the active player first after one (117.3b), uncounterable spells, casts during a resolution, storm and replicate, special actions, splice, Pyroblast redirected, Bolas's Citadel's new top card (601.2) |
| `tests/rulings_triggers3.rs` | 30 | reflexive triggers per token (603.12), last known information (608.2h), delayed returns, cast triggers of spells that left, draws counted over a turn, simultaneous triggers ordered (603.3b), intervening ifs (603.4), Nadu's new object (400.7), persist APNAP |
| `tests/rulings_protection2.rs` | 7 | effects that don't target against protection from a player (702.16b: Balance, overload, Council's Judgment), a reanimated True-Name Nemesis protected from its Aura's controller (303.4d), protection from everything and from instants, The One Ring, modes needing a target (700.2a), two wards (702.21a), channel needing a target |
| `tests/rulings_faces2.rs` | 16 | transforming DFCs' characteristics off the battlefield, colour indicators, the front's mana value (712.8, Bombardiers' boast), MDFC mana value and legality face by face, put onto the battlefield front face up (712.11, 712.14), Spellbinder's exiled MDFC played as a land, Sunfall's Incubator for 0, Touch the Spirit Realm returning a DFC, Adventures (715.3-715.4: characteristics on the stack only, copies, other exiles, Bilbo, Robber of the Rich), preparation cards (722) |
| `tests/rulings_walkers2.rs` | 1 | loyalty abilities of a planeswalker transformed mid-combat wait for a main phase (606.3); Comet, Stellar Pup's dice at one loyalty and with nothing to return |
| `tests/interactions_copies.rs` | +1 | matrix cell A x P (39.5) |
| `tests/interactions_zones.rs` | +1 | matrix cell D x O (39.5) |

The files are 83 to 1,483 lines. Every test passes; none is ignored.

**39.3 What broke, and why** (ten faults; each fix has a test or assertion that failed before
it, checked):

1. *Lion Sash attached was still a Cat* (ruling cff2c1c6). A reconfigure permanent attached
   to a creature isn't a creature (702.151b), so it has no creature types (205.3d):
   `has_subtype` keeps only Equipment for it. Assertion in
   `cards_m7d::lion_sash_grows_and_reconfigures_cr702_151`.
2. *Animate Dead put onto the battlefield without being cast* (Portable Hole's return, Leyline
   Binding, Flickerwisp, Phelia) entered attached to nothing and went to the graveyard. CR
   303.4f-g: as it enters its controller chooses a creature card in a graveyard for it to
   enchant (`Task::EnterAura`, a decision when there are several); with none it stays where it
   is. Test `rulings_replacement2::portable_hole_returning_animate_dead_lets_its_owner_choose_what_it_enchants_cr_303_4f`.
3. *A characteristic-defining ability only on the battlefield.* `pt()` gave Adeline on the stack
   her printed 0/4 (Stern Scolding countered her with three creatures in play), the goyfs in
   hand too. CR 604.3: a CDA works in every zone. Test
   `rulings_copies3::power_defining_abilities_work_in_every_zone_and_count_types_not_cards_cr_604_3`.
4. *Tezzeret's emblem replaced subtypes* (ruling 95070e2f): "becomes a 0/0 Robot artifact
   creature" made Smuggler's Copter stop being a Vehicle. "Becomes an artifact creature" keeps
   every prior type and subtype (205.1b): `Mod::Types { replace: false }`. Test
   `rulings_copies3::does_machines_and_tezzerets_emblem_make_a_copter_a_0_0_robot_that_crewing_doesnt_restore_cr_613_4b`.
5. *Bolas's Citadel's new top card was seen mid-cast* (ruling 2012f8f3): `look_tops` ran before
   each decision of a cast from the top; the player sees the new top card only once the cast is
   done (601.2). `look_tops` skips a player with a cast in progress (`Task::CastDone` waiting).
   Test `rulings_stack3::bolass_citadel_the_new_top_card_is_unseen_until_the_cast_is_done_cr_601_2`.
6. *A reflexive trigger once for several tokens* (ruling 47eed49e): Ajani, Nacatl Avenger's 0
   doubled by Elspeth made two Cat Warriors but triggered once; "create a token. When you do"
   triggers per token made (603.12): `Ext::Reflexive { per_token }`. Test
   `rulings_triggers3::a_reflexive_trigger_chooses_its_target_when_put_on_the_stack_and_can_be_answered_cr_603_12`.
7. *"Its power" with the source gone used the printed power* (Jacked Rabbit, Ouroboroid): CR
   608.2h, its power as it last existed (`Amount::SourcePower` reads `lki_power`). Test
   `rulings_triggers3::abilities_whose_source_left_use_its_last_known_values_cr_608_2h`.
8. *Caustic Bronco gone was unsaddled* (ruling ed19c884): saddled as it last existed (the
   `Saddled` mark of its old object). Same test.
9. *A new Nadu counted the old one's triggers* (ruling 96e9a886): "only twice each turn" was
   counted per creature and granting card, so a Nadu flickered by Flickerwisp granted nothing
   new that turn. The new object's ability is a new one (400.7): `Mark::Triggered` keeps the
   granting object's incarnation. Test
   `rulings_triggers3::a_new_nadu_grants_a_new_ability_whose_two_triggers_count_anew_cr_400_7_603_2`.
10. *Bilbo couldn't cast an Adventure* (ruling d26ce62d). His attack trigger ("cast an artifact,
    instant, or sorcery spell from your graveyard") filtered graveyard cards by their front
    characteristics, so Brazen Borrower's Petty Theft (an instant spell) was never offered. Cast
    as an Adventure, only the Adventure's characteristics are evaluated (715.3a-b):
    `MayCastFromGraveyardNow` also offers a card whose other spell face matches and can be cast
    now (`gy_spell_face_now`), asks the face when both can (`Task::ChooseFace`), and its "exile
    instants and sorceries instead" rider looks at the spell's type after the cast (the face it
    was cast with). Resolved, the Adventure is exiled on its adventure (its controller may cast
    the creature later); countered, Bilbo's replacement exiles it without that permission. Test
    `rulings_faces2::bilbo_casts_an_adventure_from_the_graveyard_resolved_it_may_be_cast_later_countered_not_cr_715_4`.

Test expectations corrected (the engine was right): Malcolm's free cast needs four counters as
it leaves; Crucible of Worlds may play Soporific Springs from a graveyard (playing an MDFC
considers the face played, 712.11b; "a land card" effects see only the front, ruling ab202931);
Tamiyo, Seasoned Scholar is blue and green. No existing test was wrong.

**39.4 What held.** Everything else: the replacement, copies, stack, triggers, protection, dfc
and walkers rulings above, among them protection from a player against effects that don't
target, The One Ring's protection and Sheoldred's Edict, Hexdrinker's protections, two wards
with one unpaid, Abrupt Decay against ward, modes that need a target, transforming and modal
DFCs in and out of play, Spellbinder and an MDFC, Touch the Spirit Realm's return front face up,
an Adventure's copy ceasing to exist, Robber of the Rich's caster casting the creature, the
preparation cards, planeswalkers that enter mid-combat, Comet's dice.

**39.5 The interaction matrix's last two cells** (`docs/interactions.md`, now none untested):

* *A x P, a token copy of a daybound card at night.* Reading of the current CR (2026-09-25):
  707.8a makes a token copy of a double-faced permanent a double-faced token with both faces,
  entering with the copied permanent's face up; 712.9 lets a permanent represented by a
  double-faced token transform; nightbound transforms it as it becomes day (702.145e-f).
  702.145b's "if it is night ... enters transformed" needs a double-faced *card*, but at night a
  daybound permanent is never front face up to be copied (702.145c), so a copy made at night
  enters back face up by 707.8a anyway. (712.10 in this CR is about transforming into an
  instant or sorcery face; no cube card reaches it.) Test
  `interactions_copies::a_token_copy_of_a_nightbound_permanent_is_double_faced_and_turns_at_day_cr_707_8a_702_145f`
  (Reflection of Kiki-Jiki copies Frenzied Trapbreaker in the end step of a night turn with two
  spells cast; at the next untap it becomes day and the token turns to Outland Liberator; also
  ruling efbce0fd). Held.
* *D x O, True-Name Nemesis's chosen player after a control change.* The choice is made as it
  enters (614.12) and stays with the object (only a new object chooses anew, 400.7): given to
  the chosen player by Oko's exchange, it has protection from its own controller. Test
  `interactions_zones::true_name_nemesis_keeps_its_chosen_player_after_a_control_change_cr_702_16j_110_2`.
  Held.

**39.6 Shortcuts.** One new class-C row in `docs/shortcuts.md` (39.1): an Adventure cast
through Bilbo and resolved is exiled on its adventure without asking which of the two
replacement effects applies (616.1); the other order exiles it without the permission, a
dominated outcome.

**39.7 Deferred, IR, areas left.** Nothing deferred (no `#[ignore = "RULING: ..."]`); no IR
defect (`docs/ir_defects.md` unchanged). Not finished, for usage reasons (the user's pacing
rule: stop after a completed area): zones (70 distinct texts left), combat (38), mana (95),
other (48), 251 in all, listed as `untested` in `docs/rulings.md`. Facedown had nothing left;
walkers was done out of order (its three rulings' test was written alongside protection's). (Part 4,
section 40, tested the four areas left.)

**39.8 Digests and speed.** Re-recorded (`DIGEST_WRITE=1`): 11 of the 2,500 games change,
attributed by re-recording the digests at each fix's commit (in a scratch worktree): r 172, 239,
737, 1041, 1180, 1406, 1991, h 18 and 409 by Bolas's Citadel's new top card being learned after
the cast instead of during it (fix 5: the Citadel is the cube's one "look at the top card any
time" card; the hidden-information parts of the digest change); r 1188 by fixes 6-8 (one commit);
r 1949 by Bilbo casting an Adventure (fix 10). Fixes 1-4 and 9 change no recorded game. Combined
digest 7721970fa2b16be2. Speed: the fixes add work only where they apply (an Aura entering
uncast, a reconfigure permanent's subtypes, a card with a characteristic-defining ability off
the battlefield, Nadu's marks, a Citadel's look during a cast, Bilbo's offer); `size_of::<Game>()`
1,600 B both. `tests/perf.rs` `measure`, this branch against its base 48ccbb9, binaries built
once and run interleaved (the desktop, unpinned, a training run on the machine): the first
build was 1-5% slower per decision (7.14-7.70 us against 6.67-7.30): `pt()` had a second
`cda_pt` call site for cards off the battlefield; with one call site, six rounds give 7.25-7.35
us against 7.09-7.30 (mean +1.2%, pairs +0.3% to +3%, the base's own rounds spreading 3%);
`Game::clone` 1.47-1.51 us against 1.49 (the same); clone + `determinize` 2.00-2.12 us against
1.94-2.08.

**39.9 Checks.** Every engine test binary but `az` (91 binaries: 1,208 passed (part 2's 1,086
and the 122 new), 11 ignored (as before: part 1's 7, `perf`'s 3, `digest`'s measurement run));
`FUZZ_GAMES=20000` in the default mode: no failure, 156 s (20,000 games, 214.0 decisions per
game, 5.65 million options completed; 1,000 matches, 2,576 games, 20,916 sampled worlds);
`FUZZ_POOL=interactions FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure, 183 s (20,000 games,
206.1 decisions per game, 5.42 million options completed; 2,000 matches, 5,157 games, 1,157 of
three, 41,867 sideboarding decisions, 44,614 sampled worlds); `bash tools/build_py.sh` and
`python tools/search_smoke.py --specs random,match --seeds 0-1`: 0 failed of 4 configurations,
517 s (random 72-75 s; match 169-183 s: 800 games, 748,048 decisions, 19.8 million
simulations); `python tools/rulings_doc.py --check` passes; `python tools/untested_cards.py`: 0
of 546.

## 40. The last rulings (M8d part 4)

Part 3 (section 39) left 251 distinct class-a texts untested, in four areas: zones, combat, mana
and other. Part 4 decides and tests every one of them, area by area in that order, the way part 3
did (39.1-39.2): an existing test that checks the point gets a `// ruling:` comment, a ruling no
cube card can reach is reclassified with its reason in `triage.tsv`, the rest get new tests on real
cube cards. Branch `m8d-rulings3` (from 1f6ccf4).

**40.1 Triage.** Distinct texts per area (class a before part 4; of the untested: already tested,
reclassified b, reclassified c, to test; of those to test: tested now, left; class a and tested
after):

| Area | a before | tested before | untested | already tested | -> b | -> c | to test | tested now | left | a after | tested after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zones | 123 | 53 | 70 | 5 | 0 | 0 | 65 | 65 | 0 | 123 | 123 |
| combat | 52 | 14 | 38 | 1 | 0 | 0 | 37 | 37 | 0 | 52 | 52 |
| mana | 173 | 78 | 95 | 1 | 0 | 0 | 94 | 94 | 0 | 173 | 173 |
| other | 95 | 47 | 48 | 1 | 1 | 0 | 46 | 46 | 0 | 94 | 94 |
| **total** | **443** | **192** | **251** | **8** | **1** | **0** | **242** | **242** | **0** | **442** | **442** |

Already tested, by an earlier Oracle test (four of them with an assertion added): Demonic
Consultation's empty library loses only on the next draw (d81b7805, assertion added), Golos's and
Fallen Shinobi's cards not played stay exiled (07bd45b4, 2826b2c6, assertions added), Chandra's
exiled land can't be played (dcd6db69, assertion added), Memory Jar's face-down exile (a16cbfa9),
Kaito's ninjutsu attacking what the returned creature attacked (5a72d756), Mox Amber with no
legend (91e69158), Figure of Fable's abilities checked on resolution (bc6af43b). Reclassified: one
(03b4df6c b, multiplayer: Chaos Defiler picks at random among the permanents chosen for each
opponent; with one opponent one permanent is chosen). 909ac1ac (boast once a turn with extra
combats) looked unreachable at first, but Fear of Missing Out adds a combat phase: tested. One mana
ruling (3a81a6c3, any creature crews) is tested in the combat file.
`docs/rulings.md` after part 4: 1,607 rulings (1,260 distinct): a 1,225, b 200, c 182; tested
1,225 (976 distinct), deferred 0, untested 0. Every class-a ruling of the cube is tested.

**40.2 Tests.** 120 new tests in four new files, each with the CR in its name and its rulings in
`// ruling:` comments, on real cube cards:

| File | Tests | What |
|---|---:|---|
| `tests/rulings_zones2.rs` | 35 | exile that returns a new object (Auras to the graveyard, Equipment unattached, counters gone: Palace Jailer, Portable Hole, Leyline Binding, Touch the Spirit Realm, Flickerwisp, Phelia, Displacer Kitten; 400.7); cards played from exile or a graveyard with their timing and costs (Laelia, Robber, Ragavan, Fallen Shinobi, Tersa, Urza, Golos, Ugin, Expressive Iteration, Inti, Emry, Lurrus, Crucible, Icetill Explorer, Elite Spellbinder, Adventures; 601.3, 307.1, 305.2); "cast" plays no land (305.1); permissions that end, new objects (Jace, Robber, Laelia, Loot's exhaust); escape and retrace again (702.138, 702.81); face-up exile (406.3); declined searches, empty libraries (Path to Exile, Jace, Demonic Consultation, Ursine, Ragavan, Lim-Dul's Vault, Expressive Iteration); a token with persist; Uro's land; Atraxa; dual lands' basic types (205.3i); Ajani's -4 and a land creature, a single-faced copy of Ajani (701.27c) |
| `tests/rulings_combat.rs` | 19 | creatures put onto the battlefield attacking never declared (508.4: Voice of Victory, Adeline, Gut, Raph & Mikey, ninjutsu; Tamiyo's +2, exalted, boast), each choosing what it attacks; ninjutsu's timing and target (702.49); control since the turn began for lands and Vehicles made creatures, crew timing (302.6, 301.7); haste lost; "attacks if able" (508.1d); blocks that stand (509.1h); an attacking Colonnade tapped for mana; dash; Robber and a planeswalker; exalted; an Equipment untapped; saddled for the turn; a repeated damage mode (Screaming Nemesis); exert expiring; boast with Fear of Missing Out's second combat |
| `tests/rulings_mana2.rs` | 33 | a spell's mana value under every kind of cost (Mana Drain on evoke, overload, flashback, kicker, escape, delve, Phyrexian life, emerge, domain, affinity, miracle, escalate, spree; 202.3, 601.2f); X 0 outside the stack and in free casts (107.3b), Lurrus's mana value limit with X; free casts and alternative costs (118.9a: Golos, Bloodbraid, Laelia, Chandra, Elite Spellbinder, Ragavan's dash, also with retrace); flashback (Snapcaster's and printed, kicker, X, delve, Tinker), Thalia on alternative costs, escape with a discard or sacrifice, Death escaped; devotion, domain, converge; Chrome Mox, Mox Amber, Squandered Resources, Thopter Foundry, Mishra's Workshop, Urza's Saga, Emry's affinity, Rain of Filth, Arwen, values counted once (608.2h), a token's mana value; emerge's reduction; Arena of Glory's haste; waterbend; Transmute Artifact; cascade into a split card |
| `tests/rulings_other.rs` | 33 | Ajani's -4 by card type; Lion Sash attached by another effect, tapped, without abilities (702.151b); Equipment left behind and moved (Glimmer Lens, Nettlecyst), Lightning Greaves's shroud; Anointed Peacekeeper's tax on a graveyard cast; Archon of Emeria over the whole turn; explore and connive with nothing (701.44, 701.50); Oust's controller, Oko's exchange and Dack's theft with attachments, Thieving Skydiver's control; Balance's order; Narset; Frantic Search; extra turns' order (500.7); a tapped Iron-Shield Elf; Corpse Dance's haste; Inquisition on oneself, Thoughtseize, Gitaxian Probe; Wishclaw without counters; Seasoned Pyromancer; Chandra's colorless emblem; flurry; Sneak Attack; Questing Beast; Fastbond; Walking Ballista; Candelabra; Grindstone; Kaito's "lost life"; Kolaghan's Command's order; Lurrus and another permission; Shelldock Isle; Thassa's Oracle at devotion 0; Gut made an artifact |

The files are 828 to 1,725 lines. Every test passes; none is ignored.

**40.3 What broke, and why** (eight faults; each fix has a test that failed before it, checked):

1. *Lurrus and Serra Paragon allowed any X* (ruling 90a8974d). "Cast a permanent spell with mana
   value 2 or less" checked the card in the graveyard (X is 0 there), then let Walking Ballista
   announce X = 2: a spell with mana value 4. X counts on the stack (CR 202.3e): a spell cast with
   such a permission is marked (`Mark::MvCap`) and `choose_x` offers no X past the limit. Test
   `rulings_mana2::lurrus_uses_the_x_chosen_for_the_spells_mana_value_cr_107_3_202_3e`.
2. *Alternative costs only from hand* (rulings eb35fd18, 9950a418). A card cast from exile or a
   graveyard with a permission to cast it for its costs (Laelia, Chandra, Robber, Elite
   Spellbinder, Yawgmoth's Will, Lurrus and Serra Paragon, retrace) was never offered its own
   alternative costs: Laelia's Force of Will couldn't be cast for its pitch cost, Chandra's
   Crabomination not for emerge. CR 118.9: such a permission allows one (flashback, escape,
   mayhem, miracle, airbending, Bolas's Citadel's life and "without paying its mana cost" are
   alternative costs themselves and allow none, 118.9a; warp only from hand). `Game::alt_allowed`
   names the casts that allow one; the offer (`castable_via`, `route_payable`), `cast_alt` and the
   payment take the cast's own adjustments (Elite Spellbinder's {2} more, Robber's mana of any
   colour: `alt_mana_via`); with retrace, the land discard and the alternative cost's rest are one
   cost (`CardDb::merged_cost`, `ALT_OPTION`; one that takes another hand card can't be: 40.5).
   Tests
   `rulings_mana2::cards_cast_from_exile_with_a_permission_may_use_alternative_costs_and_pay_additional_ones_cr_118_9_601_2b`,
   `::an_alternative_cost_with_retrace_or_with_elite_spellbinders_tax_cr_118_9_702_81a_601_2f`
   (Ragavan dashed with Six's retrace; Force of Will exiled by Elite Spellbinder, its pitch cost and
   the {2}).
3. *Escape couldn't exile the cards its own costs put in the graveyard* (ruling bea302d9). Under
   Underworld Breach, Bitter Triumph with two other cards in the graveyard and a card to discard
   wasn't offered: the discarded card can be exiled for escape (601.2h, the costs in any order).
   The check counts the cost's discards and its nontoken sacrifices (each card once, besides the
   mana sources that sacrifice themselves); the sacrifices of such a cost are made before the
   graveyard cards are chosen, and a sacrifice choice keeps the exile payable. Test
   `rulings_mana2::escape_exiles_the_card_discarded_for_its_additional_cost_and_casts_a_split_half_cr_702_138_709_3`
   (Bitter Triumph's discard, Tinker's sacrifice).
4. *Only the printed flashback cost* (ruling a92b37a5). Snapcaster Mage on Faithless Looting
   gives flashback for {R}; the engine used the printed {2}{R}. A card with two flashback costs may
   be cast for either (702.34a): a route `FLASHBACK_MANA` (`CastFrom::FlashbackMana`), the player's
   `ChooseRoute` when both can be paid. Test
   `rulings_mana2::flashback_costs_snapcasters_and_printed_x_kicker_no_overload_additional_costs_cr_702_34a_118_9`.
5. *A latent mana ability counted for "mana that land could produce"* (ruling a755e0f5). Urza's
   Saga before its chapter I has no mana ability, but Squandered Resources' sacrifice of it made
   {C}: `could_produce` read the card's latent abilities too (106.7: the abilities it has). Test
   `rulings_mana2::squandered_resources_gets_no_mana_from_a_land_that_makes_none_cr_605_1a`.
6. *A cast during a resolution was always the front face* (ruling 6187a75b). Bloodbraid
   Challenger cascading into Life // Death cast Life; either half may be cast (709.3), and an
   adventurer card may be cast as its Adventure (715.3; ruling 5d19b09e). The casts an effect
   allows during its resolution (cascade, Etali, Crabomination, Malcolm, Shelldock Isle, Chandra's +1)
   ask `ChooseFace` when both faces can be cast (`faces_castable_now`,
   `begin_cast_either_face`). Test
   `rulings_mana2::cascade_sees_a_split_cards_combined_mana_value_and_casts_one_half_cr_709_4_702_85a`.
7. *Thieving Skydiver couldn't attach Lion Sash* (ruling 35466139). "If that artifact is an
   Equipment, attach it": an Equipment that is a creature can't equip unless it has reconfigure
   (301.5c); the check left reconfigure out. Attached, Lion Sash isn't a creature (702.151b). Test
   `rulings_other::lion_sash_attached_by_another_effect_or_while_tapped_isnt_a_creature_cr_702_151b`.
8. *Gut couldn't sacrifice itself as an artifact* (ruling 06cb2c7b): the IR puts "another" on
   "creature or an artifact" as a whole (`docs/ir_defects.md` 19); the lowering now scopes it to
   the first noun phrase when the label says "... or a(n) ...". Test
   `rulings_other::gut_made_an_artifact_may_sacrifice_itself_to_its_own_ability_cr_701_21a`.

Test expectations corrected (the engine was right): Grief's mana value is 4; Golos is an artifact
Tinker may sacrifice; Aang taps himself to pay his own waterbend; a Phyrexian Metamorph put onto
the battlefield without copying anything is a 0/0 (given counters in the tests); Laelia's combat
damage. No existing test was wrong.

**40.4 What held.** Everything else: the new object of every exile-and-return (Auras, Equipment,
counters); timing of cards played from exile or a graveyard and "cast" against lands; escape and
retrace again; empty libraries and declined searches; creatures put onto the battlefield attacking
(no attack triggers, no boast, exalted unaffected, each choosing its target), ninjutsu's timing and
regular-step damage, summoning sickness of lands and Vehicles made creatures, blocks that stand,
exert expiring; mana value under every alternative and additional cost (read by Mana Drain), X 0
outside the stack and in free casts, devotion (hybrid and Phyrexian symbols, counted on
resolution), domain, converge with a cost increase, the Workshop's spending restriction, emerge's
printed mana value, Arena of Glory's haste; Lion Sash losing its abilities while attached (it stays
a noncreature), Balance's order, extra turns' order, Narset, Chandra's colorless emblem, Sneak
Attack and a new object, Kolaghan's Command's printed order. Memory Jar's cards are exiled face down
and unplayable; their owner still knows them (the engine's model of a face-down exile its owner
put there, ENGINE.md 30).

**40.5 Shortcuts.** One new row in `docs/shortcuts.md`, 40.1 (class B, reachable): retrace with an
alternative cost whose rest takes a card from hand or returns a permanent (evoke, pitch costs, Daze)
isn't offered, as retrace's land discard and that cost can't be one cost in the payment's model (one
reservation of hand cards or permanents per cost, as 24.7.5); the card can still be retraced for its
mana cost, and retrace with dash, overload or emerge is offered. (Squandered Resources sacrificing a
land for no mana is reachable only as a free-standing mana activation, which shortcut 13.12 already
covers; the ruling's point, no mana, is tested.)

**40.6 Deferred, IR.** Nothing deferred (no `#[ignore = "RULING: ..."]`). One IR defect, entry 19
in `docs/ir_defects.md` (Gut's "another"), worked around in the lowering. Narset's ruling a549c65a
also speaks of optional draws of several cards; no cube card has one, so its test covers the forced
draw.

**40.7 Digests and speed.** Re-recorded (`DIGEST_WRITE=1`, in the mana commit): 106 of the 2,500
games change, attributed by recording the digests after each fix in turn (a scratch worktree at the
combat commit, its own target dir, the fixes applied one at a time in the order of 40.3): fix 1
(Lurrus's limit with X) r 626 and 752; fix 2 (alternative costs with a permission) 101 games, h 2,
9, 18, 51, 81, 131, 186, 189, 197, 223, 277, 299, 451, 490 and 87 random games (r 626 again among
them); fix 3 (escape and its own costs' cards) r 1373; fix 4 (two flashback costs) r 787 and 956;
fix 6 (either face during a resolution) r 1925. Fixes 5, 7 and 8 change no recorded game. Combined
digest 7721970fa2b16be2 -> 12642107250032a7. Speed: the fixes add work where they apply (a mark for
a spell cast with Lurrus's or Serra Paragon's permission; the alternative costs of a card cast from
exile or a graveyard with a permission, checked only for cards that have one; an escape cost's
discards and sacrifices; one more graveyard route for a card with two flashback costs; a face check
for casts during a resolution); `size_of::<Game>()` 1,600 B both. `tests/perf.rs` `measure`, this
branch against its base (1f6ccf4's engine, built in the scratch worktree), binaries built once and
run interleaved in three rounds (the desktop, unpinned, other runs on the machine): 6.50, 6.63 and
6.63 us per decision against 6.51, 6.52 and 6.66 (731, 716, 717 games/s against 730, 729, 714; mean
+0.4%, within the base's own spread of 2%); `Game::clone` 1.21-1.25 us against 1.21-1.26; clone +
`determinize` 1.59-1.71 us against 1.59-1.68.

**40.8 Checks.** Every engine test binary but `az` (95 binaries: 1,328 passed (part 3's 1,208 and
the 120 new), 11 ignored (as before: part 1's 7, `perf`'s 3, `digest`'s measurement run)), also at
the mana commit; `FUZZ_GAMES=20000` in the default mode: no failure, 177 s (20,000 games, 214.0
decisions per game, 5.65 million options completed; 1,000 matches, 2,575 games, 20,824 sampled
worlds); `FUZZ_POOL=interactions FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure, 204 s (20,000
games, 206.2 decisions per game, 5.43 million options completed; 2,000 matches, 5,157 games, 1,157
of three, 41,851 sideboarding decisions, 44,694 sampled worlds); `bash tools/build_py.sh` and
`python tools/search_smoke.py --specs random,match --seeds 0-1`: 0 failed of 4 configurations, 522 s
(random 71-72 s; match 178-192 s: 800 games, 765,206 decisions, 20.3 million simulations); `python
tools/rulings_doc.py --check` passes; `python tools/untested_cards.py`: 0 of 546; `python -m pytest
-q tests`: 20 passed.

**40.9 Addendum (M6): retrace with emerge, and an audit of the cast's total cost.** Branch
`m6-emergefix` (from `m6-deck40`). The match fuzz on main 0054b32 stopped at match 1202 (42.7):
in game 2, turn 22, P0 cast Crabomination from the graveyard with Six's retrace for its emerge
cost, Talisman of Curiosity sacrificed, with only The One Ring in hand; after the mana, the
payment asked for retrace's land card ("DiscardCost with no options"). Fix 2 (40.3) made retrace's
land discard and an alternative cost's rest one cost (`alt_rest`), and `alt_payable` computed it,
but its emerge branch returned whether some artifact could be sacrificed with the reduced mana
payable (`emerge_options`) without looking at it. The total cost is the emerge cost less the
sacrificed permanent's mana value, and a land card discarded (CR 702.81a: retrace adds a cost;
702.119a; 601.2f-h). *Fix 9:* `emerge_options` (the offer, and the `Sacrifice` decision of
`Task::Emerge`) takes `alt_rest`'s non-mana parts and keeps what they need from the mana payment
(`keep_for`; emerge's own rest is empty, so the sacrifice chosen doesn't change them). Test
`rulings_mana2::retrace_with_emerge_needs_the_land_card_to_discard_as_well_cr_702_81a_702_119a_601_2f`
(failed before the fix): with only The One Ring in hand Crabomination isn't castable; with a
Forest too it is cast for emerge, the Forest discarded, Golos sacrificed, {B}{B} paid.

*The audit.* Every other way to cast an alternative cost takes its whole rest: `alt_payable`'s
default path (`cost_payable` with `alt_rest`: a pitch card, life, a permanent returned or
sacrificed, with retrace's discard merged), its bestow path (the same on the bestowed face), and
the payment (`cast_pay`, `item_cost`: `alt_rest`); `cast_alt` offers the mana cost by
`route_payable` (`normal_payable` from hand) and each alternative cost by `alt_payable`;
`castable_via`, `route_payable` and `options_payable` take the route's cost with the card's own
additional costs (merged for retrace and a granted escape), with the cast's adjustments
(`alt_mana_via`: increases, Elite Spellbinder's tax, Robber's mana of any colour). From hand, with
a permission, with Lurrus's or Serra Paragon's, taxed, and retraced, nothing else is left out for
the cube's cards (checked also on Springheart Nantuko bestowed, Snuff Out and Ragavan's dash
retraced with and without a land card). Two shapes no cube card has would leave a cost out, and
are now lowering blockers next to the existing one for an alternative cost with additional costs
(kicker, buyback, options), which the engine also doesn't pay together: an alternative cost with
spree's mode costs (702.172a: the offer of an alternative cost skips spree's "at least one mode"),
and a printed escape cost with additional costs (`route_extra` gives the escape's exile for every
option). Test `support::costs_the_cast_cant_pay_together_are_blockers_cr_601_2b_601_2f` (Three
Steps Ahead with an alternative cost and From the Catacombs with a discard were supported before;
Snuff Out with a discard is the existing blocker). Shortcut 40.1's row says when retrace with
emerge is offered.

*Digests and checks.* Behaviour digests unchanged (`cargo test --release -p engine --test digest --
--ignored`: combined 12642107250032a7; no recorded game offers emerge with retrace without a land
card, and the blockers change no cube or custom card). Every engine test binary but `az` (96
binaries): 1,355 passed (42.6's 1,353 and the 2 new), 0 failed, 12 ignored. `FUZZ_GAMES=20000
FUZZ_MATCHES=2000`: no failure, 148 s (42.6's counts: 20,000 games, 5,651,655 options completed;
2,000 matches, 5,139 games, 1,139 of three, 47,810 sideboarding decisions, 42,766 sampled worlds);
`FUZZ_POOL=interactions FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure, 171 s (20,000 games,
5,425,503 options completed; 2,000 matches, 5,127 games, 1,127 of three, 43,030 sideboarding
decisions, 44,548 sampled worlds). Main 0054b32 with fix 9 alone (a scratch worktree with its own
target dir, removed): `FUZZ_FIRST=1202 FUZZ_MATCHES=1` passes, and `FUZZ_GAMES=20000
FUZZ_MATCHES=2000`: no failure, 210 s (2,000 matches, 5,163 games, 1,163 of three, 41,920
sideboarding decisions, 41,666 sampled worlds; 20,000 games, 5,651,655 options completed). `bash
tools/build_py.sh`, `python -m pytest -q tests`: 24 passed. `python tools/search_smoke.py --specs
random,match --seeds 0-1`: 0 failed of 4 configurations, 546 s (random 75-78 s: 800 games, 328,537
decisions, 8.4 million simulations; match 191-195 s: 800 games, 766,218 decisions, 20.3 million
simulations). `python tools/rulings_doc.py --check` passes; `python tools/untested_cards.py`: 0 of
546.

## 41. Team draft (M6 part 1)

Branch `m6-team-draft` (from main at 1f6ccf4): the design of M6, team cube drafting end to end
(DESIGN.md 2.3, 5.1, 7.1: the user's decisions of 2026-10-27, full co-training, 4v4 PowerLSV,
learned deckbuilding), and its first part: the zero-core game `team_draft` in Rust. zero-core is
unchanged (its deferred hook and the co-training loop are part 2); the `mtg` game is unchanged
(layout v4, behaviour digests 7721970fa2b16be2).

### 41.1 What was built

| File | What |
|---|---|
| `engine/src/az/draft.rs` | `TeamDraftAz` (the game), `DraftSpec`, `Event` (seats, packs, passes), `Build` (one seat's deckbuilding), `MatchSetup` / `MatchPlayer` (the deferred matches), `EventOutcome`; `determinize` (`redraw`), keys, `force_chance`, invariants |
| `engine/src/az/draft_encode.rs` | layout `draft_v1` (`encode`, `layout_json`) |
| `engine/src/az/draft_bots.rs` | the `consensus` drafter, the M1 pick strength, the test-only heuristic-played matches (`play_matches_heuristic`, `resolve_with_heuristic`, `match_game`) |
| `py/src/lib.rs` | `"team_draft" => engine::az::draft::TeamDraftAz` next to `mtg` |
| `mtgz/az/__init__.py`, `mtgz/az/layout_draft_v1.json` | the layout registered for the token network (`tokens['team_draft']`), the default spec `{}`; no preset (its matches need part 2) |
| `engine/tests/draft.rs`, `tests/test_team_draft.py` | 41.6 |

**The event** (DESIGN.md 2.3.1). Players = `2T` seats (1v1 to 4v4; `MIN_PLAYERS` 2, `MAX_PLAYERS`
8), seat `s` in team `s % 2`. The draftable cube is the cube IR's 546 cards less the five basic
lands, 541, each looked up by name in the engine's card table (`draft::cube()`: an unsupported
card is an error; the identity of each card is checked against `identity.rs`). The game seed
shuffles the 541; pack `p` = round `p / 2T`, opener `p % 2T`, is the next `pack_size` cards
(sorted), the rest is never drafted (`rest`). `Event::seat_at(p, t)` / `pack_at(s, r, t)`: round
0 and 2 pass left (to `s + 1`), round 1 right. The spec: `{"rounds", "packs" (1-3), "pack_size"
(2-15), "max_turns", "format"}` (the last two the matches'); unknown keys are errors; `rounds`
defaults to `min(T, 3)` and is at most `T`.

**A pack is its removal order**: `order[p * size + t]` is the card taken at pick `t`; from
`taken[p]` on, the cards still in the pack, kept sorted (a pick rotates the chosen card to
position `taken`). A seat's pool, first picks and every view follow from it; a pick is legal iff
the card is in the pack, so every arrangement of the array is a legal draft history.

**Decisions.** A pick: the identities in the decider's pack (`0..767`; ENGINE.md 22.2). Seats
pick in order `0..2T-1` within a pick round; after the last one the packs pass. A pack's last
card is a single option (zero-core plays it without a search). After the draft each seat in turn
builds: for each pool card in its **build order** (spells by colour group W, U, B, R, G,
multicolour, colourless, then mana value, then identity; then lands by identity) its identity (in
the deck) or `OUT` = 768 (in the sideboard); then the Plains, Island, Swamp, Mountain and Forest
counts, `COUNT + n` (800 + n), each limited to keep a deck of exactly 40 cards reachable with at
most 31 of a type (`Build::count_range`; section 42: until then 40-60 cards); once the deck holds
40 pool cards the rest of the pool is forced `OUT` (a single option, `Build::room`), and the
Forest count is always forced. Every legal deck is reachable once; a seat's build is 50
decisions, single options included. After the last seat's Forest count the event is at its
**deferred terminal**: `to_move()` is `None`, `deferred()` is true, `matches()` / `deferred_payload()` give the
matches, `resolve(results)` / `resolve_json` take them (each match's A and B result: 1/0, 0/1 or
0.5/0.5) and set the outcome: per seat the team's 1 / 0 / 0.5 divided by `T` (zero-core's result
vector sums to 1), margin = (team match wins - opponent match wins) / matches, score = the team's
match wins. **`result()` and `margin()` panic while the matches are pending**: zero-core (before
part 2) would read the terminal as a real one, so a driver or a search that reaches it stops
with "search worker panicked: team_draft: the event's matches have not been played (a deferred
result ...)" (tested) instead of training on a made-up value.

**The matches** (`matches()`): the rotations of the Latin square (`rotations`: `rounds` distinct
of the `T`, drawn from the game seed; hidden: nothing encodes them, `determinize` redraws them),
round by round, A player `i` (seat `2i`) against B player `(i + d) mod T` (seat `2((i + d) mod
T) + 1`), the A player as `mtg` seat 0; per player its deck and sideboard (definitions, basic
lands repeated), its 3 first picks and `elsewhere` (what its team saw that is not in the
opponent's known pool: 4v4 306 cards); the match seed from the event. `deferred_payload()` is
the same as JSON with card names.

### 41.2 The information model

`TeamDraftAz::knowing(viewer)`: the seats whose observations the viewer has, itself during the
draft and its team after it. `views(p, mask)`: the picks at which those seats held pack `p` (a
pack in the current round only up to the current pick: every seat sees its pack when the pick
round starts, before it picks). `knowledge(mask)`: per view, the pack's contents then and the
pick made then: what the viewer knows, canonically. `known_cells(mask)`: the cells whose card is
known exactly (a view's own pick, the one cell between two views two picks apart, a pack's last
cell after a view at the second-to-last pick); after the draft, for a team, every cell but the
opposing openers' first picks (tested for 1v1, 2v2, 4v4: the first-picks rule, DESIGN.md 2.3.2).

**`determinize(viewer)`** (`redraw`): the cells before the knowing seats' first view of a pack and
every cell of a pack they have not seen, with the undrafted cards, get the unseen cards again (a
partial Fisher-Yates over those cards); the cells between two views and after the last view are
permuted (which seat took which card, which cards are still there); the unpicked parts are
sorted again. After the draft, each opponent's build order follows its redrawn pool; an opponent
who has built gets the heuristic deckbuilder's deck of the part of its pool the viewer knows
(`heuristic_deck`: `pools::build_deck` of the pool less the first picks, memoised per thread: the
same in every world, so nothing of the real deck enters a world; its redrawn first picks go to the
sideboard; nothing the viewer sees depends on that deck), one building now restarts (only another
viewer than the decider sees that state), one not yet building has its order. The pairings and the match seed are redrawn; the viewer becomes the
observer. The viewer's knowledge, its encoding, `to_move` and, for the decider, the legal
actions never change (tested at every check). The redraw is uniform over the unseen cards (an
opener's first pick is, in such a world, an average card: DESIGN.md 2.3.10 question 10).

**Keys.** `KEY_SEE(seat, identity)` (kind 16): when the packs reach their seats (at every pick
round, the openings included), one per card of each seat's pack, sorted; `KEY_TEAM(team, pack
<< 24 | pick << 16 | identity)` (kind 17): at the end of the draft, every cell the team knows
(4v4: 348 per team). After `determinize(viewer)` a seat's or team's private keys are one key
with the payload `HIDDEN` (the counts are public: pack sizes, seating), as the `mtg` game masks
private reveals (section 10). Deckbuilding and the pairings emit no key (a deck choice is an
action on the path; the encoding shows a viewer only its team's decks). Real play (no observer)
emits every key: 112-120 per pick round in 4v4, 696 at the draft's end.

**`force_chance`**: queued keys are consumed one per emitted key, in order. An opening
(`KEY_SEE` at pick 0) takes the forced cards for a seat's new pack before any key is emitted:
a forced card is swapped in from a place nobody has seen (the undrafted cards, a pack of this or
a later round) or, for the observer, from any cell it has not seen; the card swapped out is the
pack's smallest card that is not forced. A card the world cannot give (one the observer has seen)
falls back. Arrivals of packs in flight (a revelation of other seats' picks), team keys and
`HIDDEN` keys fall back. Measured: the observer's own opening replays in every forced world (80
of 80); whole transitions also show the last card of a pack in flight, so 23 of 80 transitions
reproduce all their keys.

### 41.3 Encoding: layout `draft_v1` (14,488 floats)

| Block | Offset | Floats | What |
|---|---:|---:|---|
| globals (`gl::*`) | 0 | 64 | phase (draft, deckbuilding, deferred terminal, resolved); round (one-hot 3), pick / 15, picks left / 15, left / right; team size (one-hot 4); my index in my team (one-hot 4), my team is B; I, a teammate or an opponent decides; my pool / 45, my pack / 15; my pool's nonland cards by colour and colourless / 15; my deck so far: coloured pips / 20, lands / 20, curve 0..7+ / 10, size / 40, basics / 20; deciding a card, the basic type (one-hot 5); options / 32; build position / 45; seats built / 8; packs / 3, pack size / 15, rounds / 3 |
| pack | 64 | 15 x 68 | my current pack, sorted: card code (64), present, an option of my decision, my view number of this pack / 4, spare |
| pool | 1,084 | 45 x 76 | my picks in pick order: card code, present, round (one-hot 3), pick / 15, a first pick, in the deck, out, the card decided now, an option now, build position / 45, a companion the deck so far fulfills, spare |
| bags (`bag::*`) | 4,504 | 13 x 768 | identities x 0.25: taken by others between two of my views, passed with their fate unknown (after my last view), wheeled (there again at my next view); after the draft my teammates' pools (seat order after me) and decks (the cards in, the basics), the opponents' pools less their first picks (seat order from my left) |

The card code: the identity's digits (16, 8, 8; vocabulary 1024, `mtg`'s) and a 32-float
descriptor from the card's definition (types with planeswalker, colours, colourless nonland,
mana value / 8, coloured pips / 3 counting hybrid and Phyrexian, hybrid-or-Phyrexian, power and
toughness / 8, legendary, companion, the colours a land makes or fetches and colourless mana, a
fetch land, interaction: `decks::interacts`). `layout_draft_v1.json`: version 1, the two segments
(present flag at 64, identity at 0, no `[base, stride]` actions, `id_actions: 0`), one bag entry
of 13 x 768, `cards` (the name of each identity). `id_actions` (a slot scores the action of its
card's identity: the pick, or putting the card in the deck) is part 2's `TokenNet` addition; the
pinned core ignores the key, so today every action is scored from the pooled state (tested: the
token network reads the layout on real positions and answers a driver's leaf batch). Section 47
replaced it with `draft_v2` (14,492 floats) for deckbuilding as picks.

### 41.4 The bots and the heuristic-played matches

`consensus` (`bot_names()`): the legal card with the highest M1 human pick strength
(`cubecobra_bt_strength` of `data/powerlsv_card_values.json`, compiled in; every draftable card
has one; ties: the lowest identity), and the heuristic deckbuilder's deck (`heuristic_build`; the
count nearest the plan's among the legal ones). A yardstick only (DESIGN.md 0). `random` is
zero-core's (random picks and a random legal deck).

`draft_bots::play_matches_heuristic(event, seed)` builds each match as an `mtg` match
(`match_game`: `Game::with_sideboards` + `MatchState::new` with the drafted decks, sideboards and
the real first picks; best-of-three hides the decks and the first picks as ENGINE.md 36.11) and
plays it with the heuristic bot on both sides (`bots::sideboard` between games), synchronously;
`resolve_with_heuristic` resolves the event with it. For tests and possibly a cheap yardstick
(DESIGN.md 2.3.8), never the training path. It needs no change to `mtg`: the part-3 spec keys
(`pN_first_picks`, `pN_elsewhere`, DESIGN.md 2.3.6) replace it in training.

### 41.5 Measurements

`cargo test --release -p engine --test draft measure -- --ignored --nocapture --test-threads=1`
(one thread; the desktop loaded by mtg5 and another agent's builds: two runs, their range):

| | 4v4 | 2v2 |
|---|---:|---:|
| random-action events per second | 9,400-12,800 | 25,300-26,600 |
| actions per event | 760 (360 picks, 400 deckbuilding) | 380 |
| searched picks / deckbuilding decisions per event (two options or more) | 336 / 395.4 (387.0 at exactly 40 cards, section 42) | 168 / 197.7 (193.5) |
| `encode` (14,488 floats; the caller's zeroing not counted) | 3.5-4.6 us | 2.6-3.9 us |
| clone | 0.27-0.36 us | 0.23-0.25 us |
| clone + `determinize`, draft states | 4.1 us | 3.7 us |
| clone + `determinize`, deckbuilding states | 11.1 us (33 us before the opponents' decks were memoised) | 10.1 us |
| the 12 (4) matches played by the heuristic, random drafts | 0.023-0.035 s per event, 30.2 games | 0.008-0.009 s, 10.2 games |

An action costs ~0.1 us (an `mtg` decision 7-13 us), so the draft's search cost is the network's:
the budget of DESIGN.md 2.3.9 counts leaves. A searched pick has 2-15 options (8.5 on average), a
deckbuilding decision 2 (cards) or up to 32 (basic counts).

### 41.6 Tests

`engine/tests/draft.rs` (22 tests and `measure`; 23 since section 42; section 15's row):
`packs_are_seeded_disjoint_and_full`, `seats_alternate_and_every_pass_goes_to_an_opponent`,
`picks_are_the_pack_and_illegal_actions_are_errors`, `every_pool_has_45_cards_and_decks_are_legal`
(and `consensus` builds `pools::build_deck`'s deck), `deckbuilding_reaches_every_legal_deck_shape...`
(40 basic lands; 60 cards with 31 of a basic; replaced in section 42 by
`deckbuilding_makes_exactly_40_cards_from_either_extreme` and
`every_40_card_deck_is_reachable_exactly_once`), `pairings_are_rotations_of_a_latin_square` (40
events; 1v1, 2v2, 3v3, `rounds`), `after_the_draft_a_team_knows_every_card_but_the_opposing_first_picks`
(1v1, 2v2, 4v4: the known cells, the team's cards, the `KEY_TEAM` count; during the draft a seat
knows only its own picks exactly), `determinize_keeps_what_the_viewer_knows` (684 checks at the
default scale, 3,164 at `DRAFT_SCALE=5`: the decider and a random other viewer; knowledge, picks,
encoding, `to_move`, the decider's options, teammates' pools and builds; every world redraws some
pool), `equal_keys_mean_equal_features` (5,267 comparisons; 29,147 at scale 5),
`keys_are_masked_for_the_observer`, `clone_continues_identically`,
`force_chance_replays_the_viewers_openings` (80 of 80, 400 of 400 at scale 5; whole transitions 23
and 116), `forced_keys_are_consumed_in_order_and_impossible_ones_fall_back`,
`the_heuristic_matches_resolve_an_event` (1v1, 2v2, 4v4: consensus team A against random team B
won 10-2, 4-0, 1-0), `the_result_is_unknown_while_the_matches_are_pending`,
`resolve_checks_its_results`, `the_payload_describes_every_match`,
`encoding_slots_hold_the_pack_and_the_picks`, `the_bots`, `spec_parsing`,
`layout_json_is_checked_in` (`MTGZ_WRITE_LAYOUT=1` rewrites `mtgz/az/layout_draft_v1.json`),
`fuzz_random_events` (`DRAFT_FUZZ=n`, default 200: 4v4, 2v2, 3v3 and 1v1 events of random actions;
the legal actions sorted and in range, the invariants after every action (every card once,
`taken` per pack, sorted packs, builds and decks); every ~40 actions `determinize` for the decider
and a random viewer (knowledge, encoding, `to_move`, options, invariants; one world in eight
played to its terminal); at one decision of every 8th event every option completes the event;
every event resolved, every 50th with heuristic-played matches).

`tests/test_team_draft.py` (pytest, 4): the game's info and spec errors; a 4v4 and a 2v2 event
through the bindings (actions sorted, features finite, `determinize` keeping the decider's options
and features, 760 / 380 actions); the result refused at the deferred terminal; a driver that reaches
it fails ("search worker panicked: team_draft: the event's matches have not been played ...");
zero-core's token network reading `layout_draft_v1.json` (the pack slots' identities are the pick
actions) and answering a 8-seat driver's leaf batch. `tests/test_mtg_az.py`: the module registers
`toy`, `mtg` and `team_draft`.

### 41.7 Checks

Every engine test binary but `az` (`cargo test --release -p engine --no-fail-fast --lib --test
<every file of engine/tests but az>`; 92 binaries: the library and 91 test files): 1,230 passed
(main's 1,208 and the 22 new), 0 failed, 12 ignored (main's 11 and `measure`). Behaviour digests
(`cargo test --release -p engine --test digest -- --ignored`): combined digest 7721970fa2b16be2,
unchanged (the `mtg` game is untouched: no file of its path changed but the module list in
`az/mod.rs`). `bash tools/build_py.sh` (games `toy`, `mtg`, `team_draft`) and `python -m pytest -q
tests`: 24 passed (main's 20 and the 4 of `test_team_draft.py`). The draft fuzz at scale
(`DRAFT_FUZZ=20000`): no failure, 101 s (one thread): 20,000 events (4v4, 2v2, 3v3, 1v1), 11.4
million actions, 5.04 million searched picks and 5.93 million searched deckbuilding decisions,
573,334 determinized worlds checked, 13,893 offered options played to the end of their event,
1,866 matches played by the heuristic (every 50th event), team A won 8,163, lost 8,136, drew
3,701 (random results elsewhere). `DRAFT_SCALE=5`: the sampled-state tests pass (above). The
search smoke was not run: nothing on the `mtg` path changed.

### 41.8 Open questions and what is left

* The ten open questions of DESIGN.md 2.3.10 (deckbuilding in `team_draft` or as game-1
  sideboarding in `mtg`; the first-picks reading; team values and `team_size`; the margin; 2v2
  rounds; the first-pick prior of drafted matches; the play network's start; throughput against
  signal; the draft evaluation's play agent; the uniform determinization).
* Part 2 (zero-core): `deferred` / `deferred_payload` / `resolve` / `team_size` in the trait
  (today inherent methods of `TeamDraftAz`), the search leaf at a deferred terminal, the driver's
  parking and queued setups, `TokenNet`'s `id_actions`, the co-training loop.
* Part 3: the `mtg` spec keys for drafted matches, the presets, the yardsticks, the search smoke
  for `team_draft`.
* Limitations kept: forced keys replay openings only (arrivals in flight and team keys fall back,
  as revelations); the opponents' decks in a viewer's world are the heuristic deckbuilder's.

## 42. Forty-card decks (M6)

Branch `m6-deck40` (from main at 0054b32). **The rule** (the user's decision of 2026-10-27): a
limited deck built from a pool is always exactly 40 cards. CR 100.2b makes 40 only the minimum of
a limited deck (with any number of basic lands added, 100.4b), so exactly 40 is a format
restriction of ours, chosen by the user, not a rule of the CR; a 40-card deck meets the CR's
minimum. It applies wherever a deck is built from a pool: `team_draft`'s deckbuilding (41) and
the `mtg` game's sideboarding between the games of a best-of-three (36.2). Layout v4 (frozen,
33,652 floats), `NUM_ACTIONS` and every action id are unchanged, and so is layout `draft_v1`
(14,488 floats; `layout_json_is_checked_in` passes without rewriting the file): the change only
restricts which options are offered.

### 42.1 `team_draft` (`engine/src/az/draft.rs`)

| What | Before | Now |
|---|---|---|
| the bound | `DECK_MIN..=DECK_MAX` = 40..=60 (`matches::MIN_DECK`, `MAX_DECK`) | `DECK_SIZE` = 40 (`decks::DECK_SIZE`) |
| a pool card (build order) | its identity (in) or `OUT`, always both | both while the deck holds fewer than 40 pool cards (`Build::room`); then `[OUT]`, a single option (zero-core plays it without a search, as a pack's last card); an in for a full deck is an error |
| a basic count (`Build::count_range`) | lo = max(0, 40 - before - 31 x types after), hi = min(31, 60 - before) | hi = min(31, 40 - before): the Forest count is always forced (lo = hi), and once the deck holds 40 cards every later count is forced to 0 |
| the invariants | a finished deck has 40-60 cards | at most 40 pool cards and 40 cards at any time, a finished deck exactly 40 |

**Every legal deck is reachable in exactly one way**: a legal deck is a set of at most 40 pool
cards plus basic lands making 40, at most 31 of a type; its path puts its cards in and the others
out (an in is offered while fewer than 40 are in, and its cards are at most 40) and then takes its
counts, each within the range (the types after a count can hold the rest: 40 - before - (its
count) <= 31 x types after; and the count is at most 40 - before). Tested exhaustively on a
two-card pool (all 487,676 decks once, none other) and at the extremes of a 45-card pool.

**Decision counts.** A seat still makes 50 decisions (45 pool cards, 5 counts; single options
included), a 4v4 event 400 and 760 actions in all. Searched decisions (two options or more) in
random events (`measure`): 4v4 387.0 deckbuilding decisions per event (395.4 before: the Forest
count is now always forced, 8 per event), 2v2 193.5 (197.7); picks unchanged (336, 168). In the
draft fuzz (`DRAFT_FUZZ=20000`) 5,804,002 searched deckbuilding decisions (5.93 million before).
A random seat rarely fills its deck with pool cards (22.5 in on average), so the forced `OUT`
appears mostly with built decks: a deck of 40 pool cards from a 45-card pool forces the last five.

**Bots and worlds.** `consensus` builds `pools::build_deck`'s deck, 23 spells and 17 lands = 40
by construction (`decks::DECK_SIZE`; `pools_and_the_deckbuilder` checks 200 pools), so its plan is
always legal; it now checks that its planned card is offered. In a viewer's world an opponent who
has built gets the heuristic deck of its known pool (41.2): 40 cards too. zero-core's `random`
picks among the offered options, so its decks are 40.

### 42.2 Sideboarding in `mtg` (`engine/src/az/matches.rs`)

`MatchPool::new` sets `min_deck` = `max_deck` = the first deck's size (before: 40, or the first
deck's size when smaller, to 60, or the first deck's size when larger). Every first deck built
from a pool has 40 cards: the deckbuilder's (`pools::build_deck`, generated and evaluation pools,
36.4), M8a's generated decks with their 7-card sideboards (`decks::generate_pool`), and a drafted
deck (42.1). So for every deck from a pool:

* `PASS` (done) is offered only when the deck holds 40 cards; at a player's first sideboarding
  decision the last game's deck is 40, so keeping it is one decision, as before.
* Every swap path back to 40 stays reachable, by the existing range rule (36.2): an entry is
  offered only when a deck in bounds stays reachable with the later entries free, and the later
  entries (each 0..=its copies, basic lands 0..=31) can sum to any number up to their total. A
  swap of two single cards is three decisions, a card for basic lands three or four, basic lands
  of one type for another four. Every legal deck is still reached in exactly one way.
* A spec decklist of tests (a fixed decklist, `"decks": {"p0": ...}`) of another size keeps its
  own size exactly (as 36.2's "or the first deck's size" kept it within the bounds); a drafted
  deck passed as a spec (DESIGN.md 2.3.6) is 40 and stays 40.

`matches::MIN_DECK` and `MAX_DECK` are gone (60 was an engine bound, no rule). The heuristic bot
(`bots::sideboard`) keeps its deck (`PASS` at the first decision, legal since the deck is 40) and
answers any other sideboarding decision with its first legal option, which completes at 40.

**Sideboarding decisions per match** (`fuzz_random_matches`, random players, `FUZZ_GAMES=20000
FUZZ_MATCHES=2000`): main 41,895 decisions in 1,999 matches (match 1202 fails on main, 42.7),
6,322 sideboardings: 6.63 per sideboarding, 20.96 per match; this branch 47,810 in 2,000
matches, 6,278 sideboardings: 7.62 per sideboarding, 23.9 per match. A random walk can stop only
at 40 cards now, so it changes more entries before it stops (decks changed: 6,195 and 6,142 of
the sideboardings). `FUZZ_POOL=interactions` (decks with copies of a card): main 41,851 in 2,000
matches, 6,314 sideboardings: 6.63 per sideboarding, 20.9 per match; this branch 43,030 in 2,000
matches, 6,254 sideboardings: 6.88 per sideboarding, 21.5 per match. The heuristic's sideboarding
is one decision (`PASS`), as before; zero-core searches the network's.

### 42.3 The hidden pool (`engine/src/az/sample.rs`, ENGINE.md 36.11)

The sampled opponent deck has the public size N, which is the real deck's (40), and the sampled
pool takes the real pool's bounds (40..=40), so sideboarding in a world keeps 40 as in the real
match; between games the first sideboarder's deck, hidden from the second, becomes the sampled
last deck (40). The basic lands' fallback (a type at the count cap of 31, then any type; added
when runs/mtg4 built a 60-card deck of 34 basic lands) stays for spec decklists: a 40-card deck's
basic lands always fit in the deck's two colours (62 >= 40). Tested: every sampled deck in
`sampled_decks_hold_what_the_viewer_knows` is 40 with bounds 40..=40 (and the first sideboarder's
sampled choice 40); `a_deck_of_basic_lands_up_to_the_cap_is_sampled` now stuffs 40 basic lands, 31
of a type, and every world of the other player holds a deck of the real size.

**Found on the way** (pre-existing, fixed in its own commit): the interaction fuzz (match 1454
on this branch) reached a world with no deck, "no deck of 40 cards (36 from the pool) holds what
P1 knows of P0's". While P0 cast Force of Will (whose alternative cost exiles a blue card from
hand), P0's blue hand cards were pinned (kept by `determinize`, so known exactly for the pass), and one of
them, Spell Pierce, was also an identity of P1's cloud over P0's hand: `sample::knowledge` added
it twice to the cards the deck must hold (known, and the cloud's identities), more copies than
the pool has. `Game::determinize` already took a pinned card's identity out of its cloud's needs
(`resample_with_clouds`); that computation is now `Game::cloud_needs`, shared by the deal and by
`sample::knowledge`, so the sampled deck holds exactly what the deal will place. Main's random
path does not reach such a state in 2,000 interaction matches; the digests are unchanged (the
deal is the same code).

### 42.4 Companions (ENGINE.md 35)

A companion is outside the deck: it stays in the sideboard (the rest of the pool), is revealed
when the 40-card deck fulfills its condition, and, put into the hand by its special action, is in
the game without being a deck card (the deck's counts stay 40; `sample::game_cards` lists it
apart). Nothing changes: a companion never counts toward the 40, and in `team_draft` a companion
put in the deck is an ordinary card, one of the 40.

### 42.5 Tests

* `engine/tests/draft.rs` (23 tests and `measure`): `every_pool_has_45_cards_and_decks_are_legal`
  (every deck exactly 40); `deckbuilding_makes_exactly_40_cards_from_either_extreme` (45-card pools:
  every card out and the smallest counts, 0 0 0 9 31, or the largest, 31 9 0 0 0, with the Forest
  count forced; every card in: the last five forced `OUT`, every count forced 0; three out then
  every card in: the last two forced; a card into a full deck an error), replacing
  `deckbuilding_reaches_every_legal_deck_shape...`; `every_40_card_deck_is_reachable_exactly_once`
  (a two-card pool: 487,676 decks, each once, 535,296 decisions of which 490,618 single options).
* `engine/tests/rules_m8b.rs` (14): `cr_100_2b_sideboarding_reaches_every_legal_deck_exactly_once`
  also with exact bounds (6..=6: the 60 decks of 6 cards from three starts) and `PASS` only at a
  legal size; `a_pool_deck_is_sideboarded_to_exactly_40_cards` (random, evaluation and M8a specs,
  8 matches each, both players: bounds 40..=40; 24 targets each, random 40-card decks and the
  extremes, 40 pool cards and no basic land or 31 + 9 basic lands, reached by their canonical
  path; one card more or fewer reached by none; 960 random walks, half finished by the heuristic
  from a random point, `PASS` offered exactly at 40, every walk ending at 40: 7.9 decisions per
  walk); `a_spec_decklist_keeps_its_size` (36 and 44 cards); `sideboarded_decks_are_legal...`
  (40 cards).
* `engine/tests/rules_m8b_hidden.rs`: sampled decks 40 (above); the basic-land stuffing.
* `engine/tests/fuzz.rs`: every later game's decks are 40 with bounds 40..=40.

### 42.6 Checks

Every engine test binary but `az` (`cargo test --release -p engine --no-fail-fast --lib --test
<every file of engine/tests but az>`; 96 binaries: the library and 95 test files): 1,353 passed
(main's 1,350 and 3 new: `draft.rs` 22 -> 23, `rules_m8b.rs` 12 -> 14), 0 failed, 12 ignored (as
before). Behaviour digests (`cargo test --release -p engine --test digest -- --ignored`): combined
digest 12642107250032a7, main's (best-of-one is untouched, and the deal shared in 42.3 is the same
code). `FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure, 267 s (20,000 games, 214.0 decisions per
game, 5,651,655 options completed: the games are main's; 2,000 matches, 5,139 games, 1,139 of
three, 1,491 drawn, 426.4 decisions per match, 47,810 sideboarding decisions, 6,142 decks changed,
42,766 sampled worlds). `FUZZ_POOL=interactions FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: no failure,
310 s (20,000 games, 206.2 decisions per game, 5,425,503 options completed; 2,000 matches, 5,127
games, 1,127 of three, 43,030 sideboarding decisions, 44,548 sampled worlds). `DRAFT_FUZZ=20000`:
no failure, 137 s (20,000 events, 11.4 million actions, 5,040,000 searched picks and 5,804,002
searched deckbuilding decisions, 573,334 worlds, 12,534 offered options played to the end of their
event, 1,866 heuristic-played matches). `bash tools/build_py.sh`, `python -m pytest -q tests`: 24
passed. `python tools/search_smoke.py --specs match,match_eval --seeds 0-1`: 0 failed
of 4 configurations, 1,056 s (match 307-381 s: 800 matches, 766,218 decisions, 20.3 million
simulations; match_eval 172-184 s: 800 matches, 723,239 decisions, 19.1 million simulations).
(Times on the desktop loaded by runs/mtg5 and other builds.)

### 42.7 Left and uncertain

* A spec decklist of another size than 40 keeps exactly its size (the reading of "keep their own
  size" taken here); a range between its size and 40 would be the other reading.
* Main (0054b32) fails `FUZZ_GAMES=20000 FUZZ_MATCHES=2000` in the default mode at match 1202
  (`FUZZ_FIRST=1202 FUZZ_MATCHES=1`; past the 1,000 matches `FUZZ_GAMES=20000` alone plays): in
  game 2, turn 22, P0 casts Crabomination for emerge, and after its mana `PayCosts` asks a
  `DiscardCost` with no options (`flow.rs`: "DiscardCost with no options"): the cast's cost
  (`item_cost`) holds a discard of one land card (`Filter::Type(LAND)`), which Crabomination's
  costs don't have, with only The One Ring in hand. A game-engine fault (a cost mix-up to
  investigate), not fixed here; this branch's random sideboarding differs from game 2 on, so its
  fuzz does not reach that state. Fixed by 40.9 (retrace's land discard with an emerge cost).
* Section 36's measurements (36.2's "~7 per sideboarding", 36.8, 36.11) and section 41's (41.5,
  41.7) were taken with 40-60-card decks and are kept as written, with this section's numbers next
  to them where they changed.

## 43. zero-core's deferred results (M6 part 2)

Branch `m6-corepin` (from main at 0054b32): zero-core pinned at `feat/deferred-cotrain`
(e6166c3, four commits on zero-core main 681ba8d: zero-core docs/search.md 1.1, 2.8-2.9,
3.2, 5.1 and docs/az.md 11), and `team_draft`'s trait hooks (DESIGN.md 5.1, 7.1 part 2). Section
42 is the 40-card change's (branch m6-deck40). The `mtg` path is unchanged (digests below).

### 43.1 What was built

**In zero-core** (generic: nothing names MTG):

| Piece | What |
|---|---|
| `SearchGame` | `deferred()` (over, the result from outside), `deferred_payload() -> Result<String, String>` (what decides it, JSON), `resolve(&str)` (the episodes' results, in order; a malformed result is an error), `team_size()` (seat `s` in team `s % (n / T)`; `result()` = the team's outcome / T); defaults: never deferred, teams of one |
| Search (`tree.rs`, `mcts.py`) | a deferred terminal is a no-option node of the tree's viewer: a leaf the network evaluates (`encode(viewer)`, an empty option segment: value and margin, no priors), its utility and win vector stored on the node and backed up on every later visit; `result()` never called there. Utilities are `T x win` (+ the margin term): Q in [0, 1] |
| Driver | a game at a deferred terminal is parked (records with float16 features, root values); `take_deferred()` -> `[(gid, payload)]`, `resolve(gid, results)` emits its row and samples exactly as a finished game's (`z` = `result()`, `q` = root values, `sc` = `margin()`), `pending`, `parked_game(gid)`; rolling quotas count resolved games; `enqueue(tag, spec)` (the next new game takes that spec, its row carries the tag), `queue_only` (rolling drivers that start only queued setups) |
| Bindings | `Game.deferred`, `.team_size`, `.deferred_payload()`, `.resolve(...)`, `.result()` / `.margin()` refused (ValueError) at a deferred terminal; `SearchDriver(..., queue_only=)`, `.take_deferred()`, `.resolve()`, `.pending`, `.enqueue()`, `.queued`, rows with `tag`, stats `parked`, `parked_bytes`, `deferred`, `resolved`, `queued` |
| The run (`zero.cotrain`) | settings `resolver` (the resolver game), `resolver_cfg` (its settings, nested), `resolver_init`, `resolver_conc`, `pending_max`, `draft_eval_net_every`; `<run>/resolver/`; two rolling drivers on two threads and a coordinator (parked events -> queued matches tagged `gid/k` -> results by tag -> `resolve`), an iteration = `games` events resolved, backpressure; both nets trained, the non-finite guard on both before either is saved (`NonFinite` names the net); evaluation and Elo of both (the draft net's matches are team events whose matches the resolver's yardstick, or every `draft_eval_net_every` its network, plays); resume, PAUSE, STOP, `fork_run` with both nets, `resolver_init` from a single-game checkpoint; progress `{iter, phase, events_resolved, target, drafting, pending_events, matches_queued, matches_in_flight, matches_done}` |
| The package glue | `GamePackage.deferred`: a game's `episodes(payload)` -> resolver specs and `results(payload, rows)` -> the `resolve` JSON (defaults: the payload's `"episodes"`, `[{"result", "scores"}, ...]`) |
| Toy team game | `toy_team`: 1v1 / 2v2 picks of food cards, its deferred result decided by toy matches (`{"food": [a, b]}`, a new optional toy spec key); the core's tests and preset `toy-cotrain-smoke` |

**Here**: `TeamDraftAz`'s `SearchGame` impl maps the trait hooks onto part 1's inherent methods
(`deferred`, `deferred_payload`, `resolve_json`, `ev.t`): 19 lines in `engine/src/az/draft.rs`,
nothing else of the file changed (the panic of `result()` / `margin()` at the deferred terminal
stays: nothing in the core reads them there now). So the search now evaluates the end of
deckbuilding as a deferred leaf (the draft network's estimate of the event from the root
viewer's encoding) and the self-play driver parks the event until its matches are played.
`engine/tests/draft_deferred.rs` (new): events of 2v2 (one pack of 6) and 1v1 (three packs of
15) through zero-core's driver with self-play seats and a stand-in network: the searches
evaluate deferred terminals (rows of no options), every event parks (no row, no sample before
its matches), a malformed result is refused and the event stays parked, the matches played by
the heuristic (`draft_bots::play_matches_heuristic`) resolve it, and the emitted samples carry
the real outcome (`z` = the resolved event's `result()` per relative seat, `sc` = its
`margin()`); the hooks map to the inherent methods (1v1-4v4); `measure_parked_memory`
(`--ignored`). `tests/test_team_draft.py`: the event through the bindings is `deferred` with
`team_size` T and its payload's matches (12 / 4), the result refused with zero-core's
ValueError; a driver parks the event, refuses a malformed result and emits its row once
resolved (instead of part 1's loud failure). `mtgz/az/__init__.py`: comments.

### 43.2 Deviations from DESIGN.md 5.1 (the smallest sound changes)

* `deferred_payload()` returns `Result<String, String>` (5.1: `String`): `team_draft`'s is
  fallible (the cube table), and an error then stops the driver naming the game instead of a
  panic.
* The core never parses the payload: the run's game package turns it into resolver specs and the
  episodes' rows back into the `resolve` JSON (`GamePackage.deferred`; the defaults read
  `"episodes"` and send `[{"result": [...], "scores": [...]}, ...]`, which `resolve_json` takes).
  `team_draft`'s part-1 payload lists `"matches"` (decks, sideboards, first picks, elsewhere,
  seeds), not `mtg` specs: part 3 adds the mapping (with the spec keys of 2.3.6).
* The phase quota is exact: when more events are ready than the quota needs, the rest wait for
  the next iteration (each iteration resolves exactly `games` events).
* `pending_max` counts resolver episodes (matches) queued or playing, 0 = 2 x `resolver_conc`
  (2.3.9's "matches queued up to 2 x 768").
* The draft net's evaluation with play-network matches (`eval_yard_net`, every
  `draft_eval_net_every` evaluated iteration, `eval_games` events against the yardstick) is kept
  out of its Elo fit: the Elo pool (`elo/`) is heuristic-played (one play agent, 2.3.8). The
  anchor is the run's `yardstick` setting (part 3: `consensus`); external drafters are
  `ext:<name>` players (`GamePackage.externals`).
* With `resolver_init` the play network's architecture is the checkpoint's (widened if the game
  grew; another game's checkpoint is refused). One player count per game (`players`);
  `eval_remote` is refused with a resolver.
* The Python reference driver (`play.PyDriver`) does not park: it raises at a deferred terminal
  (the Rust driver is the one that parks). `TokenNet`'s `id_actions` is not in part 2 (7.1 lists
  it in part 3; 41.8 had it in part 2).

### 43.3 Measurements

* **Parked memory** (`cargo test --release -p engine --test draft_deferred -- --ignored
  --nocapture`; self-play seats with mtg-4's playout cap, p_full 0.25; the records' features in
  float16): **5.24 MB per 4v4 event** (DESIGN.md 2.3.9 estimated ~5 MB: ~185 records of 14,488
  features), 2.74 MB per 2v2 event, 1.41 MB per 1v1 event. 64 events waiting for their matches
  (2.3.9's "in flight") hold ~335 MB.
* **The coordinator and the GPU** (measured in zero-core on its toy co-training, its docs/az.md
  11.4: 300 `toy_team` events and ~1,200 toy matches per iteration): from an event's last match
  result to its `resolve` 0.11 ms on average (max 2.4 ms); from parking to `resolve` 0.8 s on
  average (the matches' play); the match driver's leaves/s with the draft driver beside it, each
  with its own evaluator on the same GPU, 30.5k / 27.5k against 27.8k / 31.6k alone (no
  measurable interference); the draft driver waited for backpressure most of the time (the
  matches are the bottleneck, as 2.3.9 expects). Not measured here: real draft and play networks
  on the GPU (part 3's smoke run).
* A team_draft event through the driver with a stand-in network (4 simulations, two threads, the
  heuristic matches included): 4 events of 4v4 in 0.4 s, of 2v2 in 0.2 s.

### 43.4 Checks

**zero-core** (`feat/deferred-cotrain`): `cargo test --release` 43 passed (the search crate's 40:
main's 30 and 10 new; webnet's 3); `python -m pytest tests` 101 passed (main's 82 and 19 new:
`test_deferred.py` 8, `test_cotrain.py` 11; the non-finite guard's 15 among the 82). Golden
digests unchanged: the crate's `visits_records_unchanged` / `minor_decisions...` (toy records
347, 8501300995519660688), `check_checkpoint_compat` against zero-core main 681ba8d on the toy
(random and uniform evaluators) and on Glass Road (Glass Road Zero c1f56dc built with each core:
`{"intro": false}`, random and uniform evaluators: 607 / 622 samples, visits and cq records,
results and the match against random identical); toy-smoke single-game runs (rolling; classic
with margin_w 0.2, chance_k 2, cq) give the same metrics, checkpoints, optimizer state and shards
byte for byte as 681ba8d. `SMOKE_TARGET=cotrain bash tools/smoke_all.sh --with-this-core`
(Glass Road, Caverna, Kingdom Builder, MTG Zero at their HEADs with this core): **GREEN**, 19
steps: core cargo test (43), pytest (101); Glass Road cargo test 46, pytest 33, `glassroad-smoke`
4 metrics rows; Caverna 47, 32, `caverna-smoke` 4 rows; Kingdom Builder 105 (4 ignored), 45,
`kingdombuilder-smoke` 4 rows; MTG Zero (main) cargo test 1,369 (every binary, `az` included; 12
ignored), pytest 24, `mtg-smoke` 4 rows.

**Here** (`m6-corepin`): every engine test binary but `az` (`cargo test --release -p engine
--no-fail-fast --lib --test <each file of engine/tests but az>`, 97 binaries): 1,352 passed
(main's 1,350 and the 2 of `draft_deferred.rs`), 0 failed, 13 ignored (main's 12 and
`measure_parked_memory`). Behaviour digests (`cargo test --release -p engine --test digest --
--ignored`): combined digest **12642107250032a7**, main's (the `mtg` path is unchanged). `bash
tools/build_py.sh` and `python -m pytest -q tests`: 24 passed (main's 24; one test replaced:
the driver now parks the event instead of failing). `python tools/search_smoke.py --specs
random,match --seeds 0-1`: 0 failed of 4 configurations, 954 s on the loaded desktop (random
110-111 s, 800 games, 328,537 decisions; match 360-361 s: 800 matches, 765,206 decisions, 20.3
million simulations - the same match decisions and simulations as section 40.8's run: the core's
search plays `mtg` as before). The draft fuzz at scale (`DRAFT_FUZZ=20000`): no failure, 134 s,
identical to 41.7 (20,000 events, 11.4 million actions, 573,334 worlds, 13,893 options
completed, 1,866 heuristic matches, team A 8,163 / 8,136 / 3,701).
`check_checkpoint_compat` for an `mtg` checkpoint (DESIGN.md 5.1's guard rail): runs/mtg5's
iteration 8 (a copy) on main with zero-core 681ba8d against this branch (4 self-play games at 16
/ 4 simulations, policy targets visits and cq, 2 matches against random): identical (378 samples,
records, results).

### 43.5 Left for part 3

* The `mtg` side of drafted matches: the spec keys (`pN_first_picks`, `pN_elsewhere`; 2.3.6) and
  `team_draft`'s package glue (`GamePackage.deferred`: its payload's matches -> `mtg` specs with
  their seeds; the results' default format already fits `resolve_json`); a test that a drafted
  match spec plays the same match as `draft_bots::match_game`.
* `TokenNet` actions by identity (`id_actions`, zero-core).
* The co-training presets (a 2v2 smoke, the 4v4 run: `resolver` 'mtg', `resolver_cfg` mtg-4's
  settings, `resolver_init` mtg5's best, `yardstick` 'consensus', `draft_eval_net_every` 4,
  `pending_max`, the draft net's search settings of 2.3.9), the draft yardsticks (held-out
  human-pick agreement), the search smoke for `team_draft`, a smoke run on the GPU (the toy
  measured no interference between the two drivers' evaluators; MTG's draft leaves are ~2x an
  `mtg` leaf).
* zero-core tools that play matches outside the run (`zero.h2h`, `zero.replay`, `zero.targets`,
  `zero.bench`) do not resolve deferred games; the dashboard does not show `<run>/resolver/` yet
  (it does since section 45.8; the other items of this list are section 45's).

## 44. The Mac: offload and remote evaluation

Branch `m6-mac` (from eeb6291; Python, tools and docs only: the engine is eeb6291's). The MacBook
(Apple M4 Max: 12 performance and 4 efficiency cores, 64 GB, no CUDA; `greggkeithley@100.95.248.116`
on the tailnet, key auth) is a second machine for MTG Zero in two ways: CPU-only work (cargo tests,
fuzz runs, search smokes, pytest) through `tools/mac.sh`, and a run's evaluations (the yardstick and
random matches, the Elo matches) through zero-core's `eval_remote`, while the desktop keeps
self-playing. Dominion Zero's `tools/mac.sh` and Kingdom Builder Zero's remote evaluation are the
patterns.

### 44.1 `tools/mac.sh`

`bash tools/mac.sh <command>` runs the command on the Mac against this checkout's **committed** HEAD
(uncommitted changes are not sent; a warning says so): it pushes HEAD to the Mac's bare repo
`~/mtg-zero.git` (git remote `mac`) as `refs/heads/<branch>`, checks the commit out (detached) in
`~/mtg-zero-wt/<branch>` (a worktree of the clone `~/mtg-zero`; main in `~/mtg-zero` itself), runs
`git submodule update --init` there, and runs the command with Rust (`~/.cargo/bin`), the venv
`~/.venvs/dominion-zero` (Python 3.13, torch 2.14 CPU/MPS, numpy, pytest, flask, psutil: everything
mtgz needs, shared with Dominion's and Kingdom Builder's remote work) first on PATH,
`CARGO_TARGET_DIR=~/cargo-target/mtg-zero/<branch>` (one per checkout, never shared) and
`CARGO_BUILD_JOBS=12`. The command travels base64-encoded (quotes, pipes and semicolons arrive
intact). `MTGZ_MAC=user@host` names another Mac.

* **zero-core**: the Mac cannot fetch the core from GitHub (the repository is private and the Mac's
  `gh` token is invalid), so mac.sh pushes the checkout's pinned core commit from
  `third_party/zero-core` to a mirror on the Mac, `~/mtg-zero-core.git` (`refs/heads/pin-<sha>`), and
  the clone's `submodule.third_party/zero-core.url` points at it (a local-path submodule needs
  `git -c protocol.file.allow=always`, which mac.sh passes). A pin not on GitHub yet works the same.
* **The PyO3 module on macOS** leaves libpython's symbols to the loader: mac.sh exports
  `RUSTFLAGS="-C link-arg=-undefined -C link-arg=dynamic_lookup"` for every command (Dominion has it in
  a build.rs; here it is outside the repo, so any commit builds), so the builds in a target dir never
  thrash between `tools/build_py.sh` and `cargo test`. Cargo by hand in those checkouts needs the
  same RUSTFLAGS, or it rebuilds. zero-core's `build_engine.sh` installs the dylib as
  `mtgz/_engine.so`.
* **`--eval-checkout`** (no command): the checkout a run's remote evaluation uses (44.3):
  `~/mtg-zero-wt/run-<sha12>` at exactly this HEAD, refused if the working tree has uncommitted
  changes (tracked files or the submodule), its engine built (`tools/build_py.sh`, target dir
  `~/cargo-target/mtg-zero/run-<sha12>`, ~40 s) and stamped with the commit (`mtgz-engine-commit` in
  the worktree's git dir), then checked (`python -m mtgz.az.remote check <sha>`).

```
bash tools/mac.sh cargo test --release --no-fail-fast
bash tools/mac.sh 'FUZZ_GAMES=20000 FUZZ_MATCHES=2000 cargo test --release -p engine --test fuzz -- --nocapture'
bash tools/mac.sh 'bash tools/build_py.sh && python -m pytest -q tests'
bash tools/mac.sh 'bash tools/build_py.sh && python tools/search_smoke.py --specs random,match --seeds 0-1'
bash tools/mac.sh --eval-checkout          # in a run's checkout, before it starts with eval_remote mac
```

On the Mac (all new; nothing of the other projects touched): `~/mtg-zero.git`, `~/mtg-zero-core.git`,
`~/mtg-zero` (clone, no checkout of its own until main is used), `~/mtg-zero-wt/<branch>` and
`run-<sha12>`, `~/cargo-target/mtg-zero/<branch>` (4.1 GB for a branch after the full test suite,
0.4 GB for an evaluation checkout), the remote workdir `~/mtgz-remote` (empty until a run uses it).

### 44.2 Bit-identity at eeb6291

`bash tools/mac.sh ...` at eeb6291 (the Mac otherwise idle; desktop numbers from 40.9 and 42.6,
the desktop loaded by mtg5):

| Check | Mac | Desktop |
|---|---|---|
| `bash tools/build_py.sh`, empty target dir | 39 s | 151 s (today) |
| digests (`cargo test --release -p engine --test digest -- --ignored --nocapture`) | combined **12642107250032a7**; the 2,000 random and 500 heuristic games in 9.0 s | 12642107250032a7 |
| `cargo test --release --no-fail-fast` (az included) | 1,374 passed, 0 failed, 12 ignored, 337 s with the build of every test binary: the 96 engine binaries but az 1,355 passed and 12 ignored, `az` 14, card_ir 5 | 96 binaries but az: 1,355 passed, 12 ignored |
| `python -m pytest -q tests` | 24 passed, 14 s (28 at this branch's head) | 24 passed |
| `FUZZ_GAMES=20000 FUZZ_MATCHES=2000` | no failure, 111 s (108 s again): 20,000 games, 214.0 decisions per game, 5,651,655 options completed; 2,000 matches, 5,139 games, 1,139 of three, 1,491 drawn, 426.4 decisions per match, 47,810 sideboarding decisions, 6,142 decks changed, 42,766 sampled worlds | the same counts, 148 s |
| `python tools/search_smoke.py --specs random,match --seeds 0-1` | 0 failed of 4, 121 s (random 17.1 s each: 328,537 decisions, 8,432,848 simulations; match 41.4-42.4 s: 766,218 decisions, 20,295,468 simulations) | 0 failed, 546 s (random 75-78 s, match 191-195 s; the same decisions and simulations) |

The engine (Rust: games, bots, the search with a uniform evaluator) is bit-identical on the Mac; it
builds and smokes 3.9 and 4.5 times faster than the loaded desktop, fuzzes 1.3 times faster.

### 44.3 Remote evaluation: exactly the run's code on the Mac

zero-core's `eval_remote` (`zero/remote.py`, pinned 681ba8d): the run queues each evaluated
checkpoint and keeps training; a thread ships the checkpoints an evaluation needs to the remote's
mirror of the run (`<workdir>/<run name>`: config.json with the remote's overrides, the run's
`elo/results.json` and `metrics.jsonl` once), runs `python -m mtgz.az.remote eval <mirror> <iter>` in
the remote's `repo` over ssh, appends the result to the run's `evals.jsonl` and copies the mirror's
`elo/` and `models/index.json` back (the remote owns the run's Elo while `eval_remote` is on); a
failure is logged and retried every 5 minutes. The game dashboard (`python -m mtgz.az.dashboard`)
merges `evals.jsonl` into the iterations' rows.

A remote's `repo` is one fixed path in the core's spec, but a run's code is the commit of its
pinned worktree, and runs are re-pinned. MTG Zero's remote (`mtgz/az/mac.py`, `RunCommitRemote`, the
package's `remotes={'mac': ...}`) ties the Mac's checkout to the commit of the run's checkout:

* The spec resolves when zero.remote reads it (`remote_spec`, when the run starts its
  `RemoteEval`), not at import: the run's checkout must be clean (`git status --porcelain
  --untracked-files=no`, the submodule included), else **SystemExit** (no commit holds its code);
  `repo` is `~/mtg-zero-wt/run-<sha12>` of its HEAD, and `env` carries the full commit
  (`MTGZ_REMOTE_COMMIT`).
* Preflight: the run checks the Mac's checkout over ssh (`python -m mtgz.az.remote check` there);
  missing, at another commit, changed, or without its engine stamp: **SystemExit** with the reason
  and the command that prepares it (`bash tools/mac.sh --eval-checkout` in the run's checkout). The
  Mac unreachable (ssh exit 255, a timeout): a warning on stderr, and the run starts (the core
  retries the evaluations).
* Remote side: `mtgz/az/remote.py` (the shim `python -m mtgz.az.remote`, as in the other games)
  checks the checkout before every evaluation (`verify_checkout`: HEAD = `MTGZ_REMOTE_COMMIT`, nothing
  changed, the engine stamp = HEAD) and refuses otherwise (exit 1 with the reason, which the run's
  log shows every 5 minutes until the checkout is prepared; then the queued evaluations run). An
  evaluation never runs with other code than the run's.

The Mac's settings (`overrides`, on top of the run's config): `infer_device` cpu, `train_device`
cpu, `threads` 12, `groups` 2, `infer_workers` 3, and `OMP_NUM_THREADS=4` in `env` (torch threads:
the remote side of the core does not apply `torch_threads`); python
`~/.venvs/dominion-zero/bin/python`, workdir `~/mtgz-remote`. Tests: `tests/test_mac_remote.py` (the
spec is the checkout's commit through `zero.remote.remote_spec`, resolved lazily, refused when
dirty; the remote side refuses another commit, a change, a missing or stale stamp, an unset
commit; a co-training run's resolver mirror is checked and evaluated the same way, section 48).
End to end on the desktop: `python -m mtgz.az.run --run <scratch>/e2e-mac --preset mtg-smoke
--eval-remote mac` (26 s): the preflight passed, iterations 0-3 were evaluated on the Mac (3.5-5.1 s
each, shipping included), `evals.jsonl` got their rows (host mac: vs the heuristic, vs random, Elo),
`metrics.jsonl` has none, `zero.dashboard._metrics` merges them into the four rows, `elo/` and
`models/index.json` came back. Refusals checked by hand: a missing Mac checkout stops the run at
start with the command to run; an unreachable host warns and goes on; the remote shim refuses a
checkout at another commit and an unset commit. Co-training runs: the play network's evaluation on the
Mac, mirror `~/mtgz-remote/<run>.resolver` (section 48).

### 44.4 Measurements: mtg5's iteration 8

A scratch copy of `runs/mtg5` (config.json, checkpoints 4, 6 and 8, `metrics.jsonl` up to iteration
7 and `elo/results.json` without iteration 8's pairs) evaluated with preset mtg-4's evaluation
settings: 100 best-of-three matches against the heuristic and 50 against random at 32 simulations
on the evaluation pools, then the Elo matches against iterations 6 and 4 together (`elo_together`,
20 each). Single phases with `Run.play_match` / `play_matches` and run-setting overrides (the Mac
otherwise idle):

| Phase | Settings | Mac |
|---|---|---:|
| vs heuristic | CPU, 12 search threads, groups 2, 1 inference worker, torch 12 threads (its default) | 113 s |
| vs heuristic | **MPS**, otherwise the same | 234 s |
| vs heuristic | CPU, 2 workers x 6 threads / 4 x 3 with groups 4 / 1 x 4 / 8 search threads, 4 x 2, groups 4 | 93 / 93 / 89 / 93 s |
| Elo | CPU, 1 worker, torch 12 threads | 575 s |
| Elo | CPU, 1 worker x 4 threads | 365 s |
| Elo | CPU, **3 workers x 4 threads**, groups 2 | **325 s** |
| Elo | CPU, 4 workers x 3 threads, groups 4 | 740 s |

Inference dominates (batches of ~13 leaves vs the heuristic, ~6 in the Elo matches: 197,315
network calls for 1.2 million leaves), so MPS's per-call cost doubles the time and CPU inference with
a few small thread teams in parallel is fastest. Then one full evaluation through the run's own path
(`zero.remote.RemoteEval` from the desktop, the settings of 44.3, the checkout `run-a51dd686f134`):

| Phase | Mac | Desktop (mtg5, CUDA inference, loaded by self-play) |
|---|---:|---:|
| vs heuristic (100 matches) | 85 s | 7.2-8.4 min |
| vs random (50) | 146 s | 11.6-12.7 min |
| Elo (2 x 20) | 320 s | 48.6 min (the first attempt was killed by the stall watchdog after 60 minutes without progress) |
| all, shipping included | **560 s (9.3 min)** | ~68 min |

Results against the desktop's: vs the heuristic 1/100, margin -1.89 (the desktop 1/100 twice, -1.89;
average rounds 28.99 vs 28.88); vs random 22/50 (the desktop 22/50 twice; margin -0.38 vs -0.44); Elo
matches 11.5/20 and 12.0/20 against iterations 6 and 4 (the desktop 13.0 and 13.5); fitted Elo 161
+-189 (the desktop 179 +-189). Every Mac setting (CPU or MPS, workers, groups, threads) played the
same games, and the evaluation repeats exactly on the Mac. The arithmetic differs at float32
rounding: on 60 recorded leaf batches of these matches, the network's priors differ by at most
3.6e-7 between the Mac's CPU and MPS, 4.2e-7 against the desktop's CPU and 4.8e-7 against its CUDA
(values 1.2e-7; batch against one leaf at a time 3e-7), and the search turns such near-ties into
other moves now and then: the Mac's games are not the desktop's, its results are the same within
noise.

### 44.5 Turning it on for a run (mtg5)

A run can use `eval_remote mac` only from a commit that has this code (mtg5's eeb6291 has no
`remotes`), so the switch is a re-pin to this branch's head (or main fast-forwarded to it: the
engine is eeb6291's, so its search-smoke gate holds), at an iteration boundary:

1. Pinned worktree of the commit `<S>`: `git -C ~/mtg-zero worktree add --detach
   ~/mtg-zero-wt/run-<s7> <S>`, then there `CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/run-<s7> bash
   tools/build_py.sh`.
2. There: `bash tools/mac.sh --eval-checkout` (the Mac's `~/mtg-zero-wt/run-<s12>`, ~40 s).
3. Stop mtg5 cleanly after an iteration: `touch runs/mtg5/STOP` while an odd iteration (not
   evaluated: `eval_every` 2) self-plays; the run exits after its training and the supervisor with it.
4. `rm runs/mtg5/STOP`, and from the new worktree: `python tools/launch_hidden.py
   <run>/supervisor_console.txt python tools/supervise.py <run> --preset mtg-4 --seed 1 --eval-every 2
   --eval-remote mac` (`<run>` = `C:/Users/Gregg Keithley/mtg-zero/runs/mtg5`). The log says `evaluations on the
   remote 'mac' (greggkeithley@100.95.248.116)`; the first evaluated iteration creates
   `~/mtgz-remote/mtg5` and logs `iter N vs heuristic (remote mac, ...s)` and `iter N elo ... (remote
   mac)`; `runs/mtg5/evals.jsonl` holds the results. `--eval-remote ''` turns it off (from then the
   run evaluates here again; `elo/` is the Mac's last).

Every later re-pin repeats steps 1, 2 and 4: a run started at a commit whose Mac checkout is missing
stops at start with the command (44.3).

### 44.6 Limits

* zero-core's co-training (`zero/cotrain.py`, core e6166c3, M6 part 2) refused `eval_remote`
  ("eval_remote is not supported in co-training runs"). Since zero-core f8adf48 (section 48) a
  co-training run's play network (the resolver) is evaluated on a remote with `resolver_cfg`'s
  `eval_remote` (`team_draft-4v4`: the Mac); the draft network's evaluation stays on the desktop.
* zerodash (`zero-core/dashboard`) reads `metrics.jsonl` and `elo/ratings.json` only: with
  `eval_remote` its Elo is current (ratings.json comes back) but its win rates vs the yardstick and
  random stop at the last local evaluation; the game dashboard merges `evals.jsonl`. `models/best.pt`
  is not copied back (index.json is). Core changes, not made here.
* The Mac shares the venv `~/.venvs/dominion-zero`: a torch upgrade there changes its arithmetic (at
  rounding level, 44.4); the checkout check covers the code, not the venv. Other projects' remote
  evaluations on the Mac (`~/kbz-remote`, `~/domz-remote`) have their own locks and would share its
  cores.
* The remote's host and paths are fixed in `mtgz/az/__init__.py` (`MTGZ_MAC` is mac.sh's only).
* The Mac's results are not bit-identical to the desktop's for network players (44.4), so a run's
  evaluations before and after the switch differ by the usual noise only.

## 45. Team drafting end to end (M6 part 3)

Branch `m6-part3` (from main at 5ce8057), zero-core pinned at `feat/m6-part3` (zero-core main
e6166c3 + five commits: its docs/search.md 1 and 3, docs/az.md 5.1, 10 and 11.3a). The
integration of DESIGN.md 7.1 part 3 with the user's decisions of 2026-10-27 (DESIGN.md 2.3.10:
full co-training, a 4v4 PowerLSV pod, learned deckbuilding in `team_draft` with exactly 40
cards, the post-draft knowledge at every match, teammates sharing decks, one deck per player for
all its matches, the margin over the team's matches, both rotations in 2v2, the heuristic
first-pick prior, the play network from mtg5's best checkpoint, mtg-4's match search, draft
evaluation with heuristic-played matches every `eval_every` and play-network matches every
`draft_eval_net_every`, the uniform determinization, dashboard support). Section 44 is branch
m6-mac's. The `mtg` path's behaviour is unchanged: digests 12642107250032a7, layout v4 (33,652
floats) and `draft_v1` (14,488) unchanged, runs/mtg5's checkpoints play identically (45.9).

### 45.1 What was built

| Piece | Where | What |
|---|---|---|
| Drafted matches | `engine/src/az/mod.rs`, `matches.rs`, `sample.rs` | the `mtg` spec keys `p0_first_picks` / `p1_first_picks`, `p0_elsewhere` / `p1_elsewhere` (in `decks`) and `seed` (45.2); `DeckMode::Drafted`; `Seat::elsewhere`; `FirstPickPrior::new(.., elsewhere)` |
| The spec of an event's match | `engine/src/az/draft_bots.rs` | `match_spec(setup, max_turns, format)`: the JSON the glue builds; `match_game` passes `elsewhere` too |
| The deferred glue | `mtgz/az/drafted.py` | `GamePackage.deferred['team_draft']`: payload -> `mtg` specs, match rows -> `resolve` JSON, a malformed result refused (45.3) |
| Actions by identity | zero-core `python/zero/net.py` | `TokenNet`'s `id_actions` (45.4) |
| Deckbuilding's simulations | zero-core `SearchGame::sims_divisor`; `team_draft`'s spec key `build_sims_div` | deckbuilding at the seat's simulations / 2 (45.4) |
| A position from a draft history | `engine/src/az/draft.rs` | `team_draft`'s spec key `history`, `TeamDraftAz::from_history` (the human-pick yardstick's positions, 45.5) |
| The presets | `mtgz/az/__init__.py` | `team_draft-smoke` (2v2, CPU), `team_draft-gpu-smoke`, `team_draft-4v4` (45.6) |
| The yardsticks | `mtgz/az/human_picks.py`, `tools/data/08_heldout_drafts.py`; zero-core `GamePackage.evals`, `Run.extra_evals` | `consensus` (the anchor) and `random` teams, the draft Elo, play-net-played evaluation (zero-core's co-training, configured), the agreement with held-out human picks (45.5) |
| The search smoke | `tools/search_smoke.py` | specs `team_draft` (4v4) and `team_draft_2v2` (45.7) |
| The dashboard | zero-core `python/zero/dashboard.py` | co-training runs: a tab per network, the co-training progress and tiles, the other yardsticks (45.8) |
| Tests | `engine/tests/draft_matches.rs` (7), `engine/tests/fuzz.rs` (`FUZZ_POOL=drafted`), `tests/test_drafted.py` (6); zero-core: `driver::tests::light_decisions_divide_the_simulations_and_are_recorded`, `test_net_data.py` (2), `test_cotrain.py` (2) | 45.9 |

### 45.2 Drafted matches in `mtg` (DESIGN.md 2.3.6)

A match of a `team_draft` event is an `mtg` spec:

```json
{"max_turns": 60, "format": "bo3", "seed": <the event's match seed>,
 "decks": {"p0": [...], "p0_sideboard": [...], "p0_first_picks": [...], "p0_elsewhere": [...],
           "p1": [...], "p1_sideboard": [...], "p1_first_picks": [...], "p1_elsewhere": [...]}}
```

* `pN` / `pN_sideboard`: player N's built deck (exactly 40 cards, basic lands repeated) and the
  rest of its pool (CR 100.4b); the A player (an even seat) is `mtg` seat 0, as in
  `TeamDraftAz::matches`.
* `pN_first_picks`: its real first picks (one per pack), `MatchPool::first_picks`: the other
  player does not know them until it sees them, exactly as generated pools' (36.11): the bags,
  `known_pools`, the deck questions and `determinize` read only the known part.
* `pN_elsewhere`: the cards player N knows are elsewhere: its team saw them in the draft (its
  own and its teammates' pools, the other opponents' known cards) and they are not in this
  opponent's pool. They leave N's prior of the opponent's unseen first picks
  (`FirstPickPrior::new(db, pools, owner, elsewhere)`): its candidates were the supported cards
  outside the known part of the opponent's pool and outside the viewer's own pool (~454 for a
  45-card pool); a drafted match's are exactly the cards the viewer's team never saw: **193** in
  4v4 (the opposing team's 12 first picks and the 181 undrafted cards), 367 in 2v2 (6 + 361).
  The weights stay the engine heuristic's (the user's decision, 2.3.10 question 6). With these
  two keys the knowledge at a match is the post-draft knowledge (the user's reading: nothing
  another match reveals is carried over).
* `seed`: the match's seed replaces the driver's game seed (`new_game`), so the spec plays the
  event's own match: `MtgAz::new_game(spec)` is `draft_bots::match_game(setup)` (the same game
  fingerprint, pools, decks, priors and encodings for both viewers, and heuristic play gives the
  same keys, results and step counts; tested on 32 matches of 4v4 and 2v2 events, random and
  `consensus`-built decks, best-of-three and best-of-one).
* **The spec checks its knowledge**: a first pick outside its player's pool (deck or
  sideboard), named twice or a basic land; a card "elsewhere" that is a basic land or in the
  other player's pool (a first pick of theirs would leave the real world outside the viewer's
  information set; a known card of theirs is a contradiction): errors that name the card.
* **Unchanged**: without the four knowledge keys the `decks` object is `Lists` or
  `ListsWithSideboards` as before, and without `seed` the driver's seed is used: the generated
  pools, the evaluation pools, best-of-one and every existing spec are what they were (the
  digests, 45.9). Decks of up to 31 of a basic, exactly 40 cards: `MatchPool::new` and
  sideboarding as section 42.
* `match_game` (part 1's test utility) now passes `elsewhere` too (the heuristic reads the real
  game: its play is unchanged).

### 45.3 The deferred glue (`mtgz/az/drafted.py`; zero-core's `GamePackage.deferred`)

zero-core parses no payload (43.2). `episodes(payload)` turns `TeamDraftAz::deferred_payload()`
(`{"event": {seats, team_size, rounds, max_turns, format}, "matches": [{round, seats, seed,
players: [{seat, deck, sideboard, first_picks, elsewhere}, x2]}, ...]}`) into one spec per
match (45.2), in the payload's order; the coordinator queues them tagged `<gid>/<k>`.
`results(payload, rows)` turns the matches' result rows, in order, into `[{"result": [a, b],
"scores": [wins a, wins b]}, ...]` for `resolve` and **refuses a malformed result**
(ValueError): another number of rows than of matches, a row that is not a two-player match, a
result other than 1/0, 0/1 or 0.5/0.5; the engine's `resolve` refuses it again and the event
stays at its deferred terminal (tested). A match cut at the driver's `max_steps` arrives as a
draw (0.5/0.5, `over` false) and counts as one. In a run the coordinator's exception reaches the
main thread and the run aborts (zero-core 11.2): a malformed result never becomes a made-up
outcome. The engine's `draft_bots::match_spec` builds the same spec from the event (tests, the
fuzz).

### 45.4 Actions by identity, deckbuilding's budget

**`TokenNet`'s `id_actions`** (zero-core docs/az.md 10; DESIGN.md 2.3.5): layout `draft_v1`'s
two segments (the pack's 15 slots, the pool's 45) carry `id_actions: 0`: a present slot also
scores action `0 + its identity`, one pointer column per segment, from its token conditioned on
the pooled state. A pick (the pack's identities) is scored by the pack slots, putting the card
being decided into the deck (its identity) by its pool slot; `OUT` and the counts keep the
generic logits. A pack card and a pool card are never the same identity (a singleton cube), so
no action is scored twice. Layouts without the key (layout v4) build exactly the network they
did (no identity column, the normalised layout stored in checkpoints unchanged): runs/mtg5's
iteration 10 plays identically (45.9).

**Deckbuilding at half a pick's simulations** (DESIGN.md 2.3.9: picks 64 / 16, deckbuilding
32 / 8): zero-core's new `SearchGame::sims_divisor` (default 1) divides the seat's drawn budget
of the pending decision (at least one simulation; recorded as usual, every network seat);
`team_draft` returns its spec's `build_sims_div` (1-16, default 1) during deckbuilding, and the
run presets set 2. Measured in the search smoke (4v4, uniform evaluator, 45.9): 33,785 searched
decisions of 48 events took 697,792 simulations, 20.7 per decision (picks 28 on average at p_full
0.25, deckbuilding decisions 14): ~14,600 per event, DESIGN.md 2.3.9's 14,900.

### 45.5 The draft yardsticks (DESIGN.md 2.3.8)

* **`consensus`** (the anchor, Elo 1000: the run's `yardstick`) and **`random`** teams: events
  whose team seats alternate between the draft checkpoint and the bot (zero-core's
  `CoRun.event_match`), every evaluated iteration (`eval_every` 1), the matches played by the
  heuristic (the resolver's yardstick: one play agent for the whole Elo pool); `eval_games` 32
  events against `consensus`, `eval_random_games` 16 against `random`, `ref_games` 32 reference
  events `random` against `consensus`.
* **Draft Elo**: each checkpoint against its checkpoints 1, 2, 4 and 8 back (`elo_opponents`
  4), `elo_games` 16 events each, heuristic-played; Bradley-Terry with `consensus` at 1000
  (`elo/`).
* **Play-network matches** every 4th iteration (`draft_eval_net_every` 4: `eval_yard_net`, 32
  events against `consensus`, the matches played by the play network of the same iteration at
  its `eval_sims`), logged in metrics, outside the Elo fit (43.2).
* **Agreement with held-out human picks** (a logged curiosity, never optimised: DESIGN.md 0).
  `tools/data/08_heldout_drafts.py` (the M1 venv: pyarrow) keeps the CubeCobra test split's
  drafts (docs/data_m1.md 5: held out by draft) that are one seat's whole 3 x 15 draft
  consistent with an 8-seat pod (a pack seen again eight picks later holds only cards of its
  earlier view less the card taken, and no card in two packs), 2,585 of 3,150, and writes a seeded sample of 400 to
  `E:\mtg-zero\data\splits\cubecobra_heldout_8seat.json` (raw human picks: never committed).
  `mtgz/az/human_picks.py` (`GamePackage.evals['team_draft']`, run on every evaluated
  iteration of a 4v4 run; skipped, logged, for other pods or without the file) takes the first
  100 drafts (`MTGZ_HUMAN_PICKS_DRAFTS`): the drafts mix cube versions (76% of their pack cards
  are current cards, matched by name, face name and with the accents dropped on both sides); a
  card not in the current cube is replaced, consistently within the draft, by a random current
  card the draft does not hold (seeded per draft: the same positions every iteration). Each pick
  whose card is current, with at least two current cards in the pack, is a position: the spec
  key `history` (45.1: seat 0's packs at its picks and its picks; `TeamDraftAz::from_history`
  builds the event with seat 0's views exactly, its picks in place, the cards between two views
  of a pack in a random order, every other cell a random card none of the views holds, every
  seat at the same pick; tested: the encoding and the knowledge of seat 0 equal the real event's
  at 0, 5, 9, 23 and 44 picks in 4v4, 13 and 31 in 2v2, 40 in 1v1; a history that does not fit
  the event is an error). The network's choice is its policy's top card among the pack's current
  cards (no search: the network's own judgement), the human's among the same cards. Logged:
  `extras.human_picks` = {value (top-1 agreement), n, drafts, se, refs: {random, strongest card}}
  (on the same positions; the strongest card by M1's pick strength, fit on the whole corpus, so
  an optimistic reference). 100 drafts: 3,603 positions; ~1 s per evaluation on the GPU once the
  positions are built (cached per process). Values: 45.10.

### 45.6 The presets (`mtgz/az/__init__.py`)

| | `team_draft-4v4` (the run) | `team_draft-gpu-smoke` | `team_draft-smoke` (CPU) |
|---|---|---|---|
| pod, events / iteration, in flight | 4v4 (players [8]), 48, 64 | 4v4, 8, 16 | 2v2 ([4]), 4, 4 |
| draft search | picks 64 / 16, deckbuilding 32 / 8 (`build_sims_div` 2), p_full 0.25, cq targets, `temp_moves` 120 | as the run | 8 / 2, deckbuilding 4 / 1 |
| draft network | tokens 128 x 4 (heads 4, ff 256; actions by identity), batch 512, reuse 4, window 8 (min 20,000 samples), value_q 0.5 (warm-up 5), RAM 6 GB, CUDA | as the run (min window 0) | tokens 32 x 1 |
| play network (`resolver_cfg`) | mtg-4 (64 / 16, minor 4, 128 x 4, cq, value_q 0.9), evaluated every 2nd iteration on the fixed evaluation pools **on the Mac** (`eval_remote` mac, section 48); `resolver_init` the mtg5 checkpoint chosen at launch | mtg-4, 96 in flight, small evaluations (on the Mac, 48) | mtg-smoke, best-of-three, tokens 32 x 1, minor 2 |
| matches in flight, backpressure | 768, 2 x 768 (`pending_max` 0) | 96, 192 | 16, 32 |
| draft evaluation | `consensus` 32 events, random 16, Elo 4 x 16, reference 32 (heuristic-played, every iteration); `eval_yard_net` every 4th; human picks | 4, 2, 2 x 2, 4; `eval_yard_net` every 2nd | 4, 2, 2, 4; every 2nd |

`temp_moves` 120: zero-core counts an episode's searched decisions over all seats, so 120 is the
first pack's 15 pick rounds of a 4v4 (mtg-4's 60 would be 7.5); root noise at every full search.
The draft network's value target blends the event's outcome with the root search's estimate as
the default (value_q 0.5 after 5 iterations): the search's leaves at the deferred terminal are
the network's own estimate, so the outcome carries the signal.

### 45.7 The search smoke for `team_draft` (`tools/search_smoke.py`)

Specs `team_draft` (4v4) and `team_draft_2v2`: `--events` (default 24) events per seed with the
self-play seat of `team_draft-4v4` (picks 64 / 16, deckbuilding 32 / 8, p_full 0.25, cq,
temperature) and a uniform evaluator through zero-core's driver, which parks every event at its
deferred terminal; the coordinator of zero-core's co-training (`zero.cotrain.run_pair`) queues
each parked event's matches (the glue's specs) into a second, `queue_only` driver of heuristic
seats and resolves the event with their results. Every event must resolve (none left parked)
with a sane outcome (each seat `1/T`, `0` or `1/2T`, the team result the one its match wins
give, at most 12 or 4 match wins), samples emitted, every match ended; a configuration with no
batch for `--stall` seconds is a hang (a worker's panic). Reported: events, team A's wins and
draws, matches, searched draft decisions, samples, simulations, events parked, the parked
records' peak.

### 45.8 The dashboard (zero-core `zero.dashboard`, `python -m mtgz.az.dashboard`, port 5340)

Generic (zero-core docs/az.md 5.1, 11.3a): for a run with a `resolver/` directory
`/api/run/<name>` adds `resolver` (its config, metrics, Elo results and ratings), and the page
shows a tab per network (`team_draft network`, `mtg network (resolver)`), each the single-game
view of that network: its Elo by iteration and head-to-head table, win rates against its
yardstick and random (the draft network's also against `consensus` with the matches played by
the play network, `eval_yard_net`), losses, throughput, minutes per iteration, the table; the
status line shows the co-training progress (`Iteration i: self-play e/48 events resolved · d
drafting · p waiting for their matches · matches q queued, f in flight, n done`), and tiles the
other network's newest Elo, matches per hour and the event latency (park to resolve, pending
events, the parked records' peak). `extras` (the human-pick agreement) are charted in "Other
yardsticks" with their references; a failed one is named in the caption. A single-game run renders
exactly as before: rendered headless (Playwright, its requests answered by the Flask test client:
no port) for a copy of runs/mtg5's metrics, Elo and log, the old dashboard (zero-core e6166c3)
and the new one give the same page DOM (the new page's two hidden elements aside) and the same
screenshot but the "last log line N s ago" clock; no script error, no horizontal scroll at phone
width, for the single-game run and both tabs of the smoke runs. The live dashboard (:5340,
main's checkout) shows co-training runs once main pins this core.

### 45.9 Checks

* **Engine tests**: every test binary but `az` (`cargo test --release -p engine --no-fail-fast
  --lib --test <each file of engine/tests but az>`, 98 binaries: the library and 97 files):
  **1,364 passed, 0 failed, 13 ignored** (the 7 of the new `draft_matches.rs`; ignored as
  before).
* **Behaviour digests** (`cargo test --release -p engine --test digest -- --ignored`): combined
  digest **12642107250032a7**, main's (2,000 random and 500 heuristic games, 16.3 s).
* **Fuzz** (`FUZZ_GAMES=20000 FUZZ_MATCHES=2000`): no failure, 284 s; identical to section
  42.6 (20,000 games, 214.0 decisions per game, 5,651,655 options completed; 2,000 matches, 5,139
  games, 1,139 of three, 1,491 drawn, 426.4 decisions per match, 47,810 sideboarding decisions,
  6,142 decks changed, 42,766 sampled worlds, first picks resampled in 23,947) - the new check
  (a sampled first pick is a candidate of the prior) held in every world.
  `FUZZ_POOL=interactions`: no failure, 342 s, identical to 42.6 (20,000 games, 206.2 decisions,
  5,425,503 options; 2,000 matches, 5,127 games, 43,030 sideboarding decisions, 44,548 worlds).
  **`FUZZ_POOL=drafted`** (the matches of 2,000 drafted events: 4v4 and 2v2, random and
  `consensus` team A, one in eight with 12-turn games): no failure, 27 s; 5,015 games (1,015
  matches of three, 768 drawn games), 508.0 decisions per match, 46,741 sideboarding decisions,
  5,919 decks changed, 50,506 sampled worlds, first picks resampled in 46,564 (the rest: first
  picks seen), decks in 50,385. `DRAFT_FUZZ=20000`: no failure, 119 s, the counts of 42.6
  (20,000 events, 11.4 million actions, 5,040,000 searched picks, 5,804,002 searched deckbuilding
  decisions, 573,334 worlds, 12,534 options completed, 1,866 heuristic-played matches; team A
  8,066 / 8,155 / 3,779).
* **Python** (`bash tools/build_py.sh`, `python -m pytest -q tests`): **30 passed** (main's 24
  and the 6 of `test_drafted.py`), 69 s.
* **Search smoke**: `python tools/search_smoke.py --specs random,match --seeds 0-1`: **0 failed
  of 4**, 734 s (random 103-107 s, 800 games, 328,537 decisions: part 2's; match 253-260 s, 800
  matches, 766,218 decisions, 20.3 million simulations: section 42.6's). `--specs
  team_draft,team_draft_2v2 --seeds 0-1` (24 events each): **0 failed of 4**, 22 s: 4v4 48 events,
  576 matches, 33,785 searched draft decisions (704 per event), 697,792 simulations (20.7 per
  decision), 8,398 samples (175 per event), parked records at most 11.6-16.5 MB (two or three
  events at a time); 2v2 48 events, 192 matches, 16,889 decisions, 4,216 samples, at most 7.7 MB;
  every event resolved with a sane outcome (team A won 16, drew 7 of 48 in 4v4, 22 and 14 in
  2v2: the uniform evaluator's drafts against each other), every match ended.
* **`check_checkpoint_compat`** (zero-core's tool) on a copy of runs/mtg5's iteration 10: main's
  checkout (m6-corepin at 5ce8057, zero-core e6166c3) against this branch, best-of-three spec, 4
  self-play matches at 16 / 4 simulations, policy targets visits and cq, 2 matches against random:
  **identical** (542 samples each, records 9451e8d0d1824754 / 16280d8b0ee4a21a, results
  48342af1b2fab28f, the match 1d9e9da9908a8244).
* **zero-core** (`feat/m6-part3`): `cargo test --release` 44 passed (41 + webnet 3: the driver's
  new `light_decisions_divide_the_simulations_and_are_recorded`); `python -m pytest tests` 105
  passed (101 and 4 new). `SMOKE_TARGET=m6part3 bash tools/smoke_all.sh --with-this-core` (Glass
  Road c1f56dc, Caverna a6fad73, Kingdom Builder 4202a4a, MTG Zero main 5ce8057, each at its HEAD
  with this core d68778e): **GREEN**, 19 steps: core cargo test 44, pytest 105; Glass Road cargo
  test 46, pytest 33, `glassroad-smoke` 4 rows; Caverna 47, 32, `caverna-smoke` 4 rows; Kingdom
  Builder 105 (4 ignored), 45, `kingdombuilder-smoke` 4 rows; MTG Zero cargo test 1,376 (every
  binary, `az` included as smoke_all runs it; 13 ignored), pytest 24, `mtg-smoke` 4 rows.
* **The dashboard**: 45.8 (the same DOM for a single-game run; no script error).

### 45.10 The smoke runs (scratch run directories; the desktop shared with runs/mtg5 on the GPU and other projects' runs)

**CPU, 2v2** (`python -m mtgz.az.run --run <scratch>/td_cpu --preset team_draft-smoke --iters 3`,
CUDA hidden): iteration 0 evaluated both nets (the draft network 0/4 against `consensus`, 1 of 2
against random, 3 of 4 against `consensus` with play-net matches; the human-pick yardstick
skipped: a 2v2 run); iteration 1 drafted, parked and resolved 4 events (22 matches played by the
play network, both nets trained and evaluated); **killed** (`tools/kill_run.py`) during iteration
2's self-play (1 event resolved, 10 parked, 16 matches in flight) and restarted with the same
command: "starting after iter 1", iteration 2 replayed from its start (both data directories
cleared), then iteration 3; `done`, no debug directory. Per iteration (tiny networks, 8 / 2
simulations): 25-40 s of self-play, 4 events and 21-24 matches (360-580 events and 2,160-3,050
matches per hour), 345-368 draft samples, 2,096-4,237 match samples; draft leaves 15-24% of the
leaves (27-39% as leaf-equivalents); parked records 28-32 MB at most (10-11 events waiting);
event latency (park to resolve) 25-34 s; the coordinator's delay from the last match to `resolve`
1.6-7.5 ms; the draft driver waited for backpressure 18-20 s of each phase (the matches are the
bottleneck, as designed). The dashboard showed both tabs (45.8).

**GPU, 4v4** (`--preset team_draft-gpu-smoke --resolver-init <a copy of runs/mtg5's
iteration 10>`: the run's networks and search at 8 events per iteration, 16 events and 96 matches
in flight, backpressure at 192): iteration 0 = a fresh draft network and mtg5's iteration 10 as
the play network ("resolver iteration 0 = ...mtg5_iter_0010.pt (tokens, 33652 features, 1024
actions)"); its evaluation: the play network 0/8 against the heuristic, 3/4 against random; the
draft network 0/4 against `consensus` (margin -6.5 matches of 12), 0/2 against random, 1/4
against `consensus` with play-net matches (40 min: 48 matches at 32 simulations, 48 in flight);
human picks 15.1% (random 17.8%, strongest card 39.7%: an untrained network). Iteration 1
(self-play 2,046 s, while the engine fuzz and search smokes and smoke_all's builds ran on the
CPU):

| | draft (`team_draft`, 4v4) | matches (`mtg`, the play network) |
|---|---:|---:|
| resolved / played | 8 events (28 more parked, 16 drafting) | 241 matches (2.5 games, 610 decisions, 33 minor each) |
| per hour | 14 events resolved (the quota of 8 per phase; drafting ran ahead, backpressure) | 424 matches |
| leaves, leaves/s, mean batch | 703,092, 344, 8.0 | 5,108,844, 2,497, 47.9 |
| leaves per unit | ~16,000 per event drafted (~44 drafted; the search smoke: 14,600) | 21,200 per match (runs/mtg5: 14,700-24,800) |
| samples | 1,436 (180 per event) | 35,187 (146 per match) |
| GPU split (leaves; x2 for a draft leaf's tokens) | 12.1% (21.6%) in this phase; at steady state (14,600 per event against 12 x 21,200) 5.4% (10.3%) | |
| parked records, event latency | 151.6 MB at most (28 events, 5.4 MB each), park to resolve 1,303 s mean / 1,811 s max; the coordinator 10.6 / 18.9 ms; backpressure 1,144 s of 2,046 | |

**Killed** (`tools/kill_run.py`) 1 min into iteration 2's self-play (1 event resolved, 28
parked, 96 matches in flight) and restarted with the same command and `--iters 2
--draft-eval-net-every 0` (to skip a second 40-minute play-net evaluation of the draft): "starting
after iter 1", iteration 2 replayed from its start (its data directories cleared, the events and
matches in flight dropped). Iteration 2 (self-play 2,047 s, the CPU quieter, the GPU still
shared): 8 events resolved (24 parked), 1,410 draft samples, 641,184 draft leaves (313 per second,
batches of 8); 226 matches (471 decisions, 29 minor each; 397 per hour), 25,693 samples,
3,889,971 leaves (1,900 per second, batches of 47.7: 17,200 per match); GPU split 14.1% of the
leaves (24.8% as leaf-equivalents) in this phase; parked records 144.1 MB at most (24 events);
event latency 1,262 s mean / 1,707 s max, the coordinator 9.3 / 22.0 ms, backpressure 1,015 s.
Training: the draft net 50 steps (31-37 s), the play net 101-138 steps (24-38 s; windows of
1,436-2,846 and 35,187-60,880 samples). Evaluations of iterations 1 and 2: the play network 0/8
against the heuristic, 3/4 and 1.5/4 against random (its evaluation and Elo 16-31 min with 4-8
matches in flight); the draft network 0/4 against `consensus` (margin -11.5 of 12) and 0/2
against random (-11.0), the draft Elo 2/2 for iteration 2 against 1 and 0/2 against 0 (its
evaluation and Elo 8.5-10 min); human picks 19.5% and 18.9% (iteration 2 on the regenerated
3,603 positions: random 17.8%, strongest card 41.1%; none failed). Both networks trained and
evaluated each iteration; `done`, no debug directory, no non-finite value.

**What the smokes' draft networks learned: deckbuilding collapsed to an extreme.** The GPU
smoke's iteration-2 draft network, choosing by its policy, puts every pool card `OUT` and builds
40 basic lands (31 Plains); its searched evaluation lost every match to `consensus` and to random
drafters. Its iteration-1 training data (drafted by the fresh network with search) already leaned
that way: on 709 card decisions the completed-Q target put on average 52.4% on `OUT`, and above
50% in 90.8% of them (iteration 2: 52.9%, 85.4%); the fresh value head's random preference
between two near-identical children decides every such target the same way, and `OUT` is one
generic logit shared by every card decision (the card's own action is its slot's pointer logit),
so a small consistent bias becomes the policy. The CPU smoke swung the other way (iteration 1
30-39 basic lands, iterations 2-3 every card in, 0-4 basic lands). In self-play every seat is the
same network, so a shared degenerate deck would be a fixed point: decks of 40 basic lands on both
sides draw every match and give no signal to leave it; nothing explores deckbuilding but the
root noise (`temp_moves` 120 ends before the first deckbuilding decision). The smoke's self-play
had not reached it (its searched decks still decided matches: 5.4 match wins per team and event
in iteration 2, 26% of iteration 1's samples from drawn events, none in iteration 2), but its
evaluation had. Two iterations of 8 events say little about a 48-event run's learning; this is
the first thing to watch (45.12).


**Projection for `team_draft-4v4`** (48 events per iteration). An event is ~14,600 draft leaves
(x2: ~29,000 leaf-equivalents) and 12 matches of 17,200-21,200 leaves: ~235,000-284,000
leaf-equivalents. With
768 matches and 64 events in flight the batches are runs/mtg5's (384 leaves), whose self-play
measured 6,600-11,400 leaves/s on this GPU (iterations 1-10, the GPU shared with the other
projects): **84-175 events (1,000-2,100 matches) per hour of self-play**, so **16-34 min of
self-play per iteration** (DESIGN.md 2.3.9: 145-175 events per hour at 11.4k leaves/s; the matches
grew from 17k to 21k leaves). The smoke's own 344 + 2,497 leaves/s came from 16 events and 96
matches in flight on a GPU busy with runs/mtg5 (batches of 8 and 48); it is not the run's rate.
Training both nets: ~2-3 min (the play net ~84k samples per iteration, mtg5's 1.5-3 min; the
draft net ~8.6k). Evaluation: the draft network every iteration (32 + 16 events against
`consensus` and random, 4 x 16 Elo events, heuristic-played: ~112 events whose 4 or 8 searching seats make ~19,000-38,000 leaf-equivalents each, ~5-10 min; in the smoke, 4 + 2 events took 4 min with 4 and 2 events in flight), the play network every
2nd iteration as mtg-4 (runs/mtg5: 28-69 min of evaluation and Elo per evaluated iteration), the
play-net matches of the draft evaluation every 4th (32 events, 384 matches at 32 simulations:
~20-40 min). **An iteration: ~42-91 min, ~65 on average (0.9 per hour; 100 iterations ~4.5
days)**; self-play alone ~84-175 events per hour, the whole loop ~45 events per hour. The GPU
split at that scale: the draft ~5-6% of the leaves (10-12% as leaf-equivalents; 2.3.9: 11-13%);
parked records ~5.4 MB per event, ~350 MB for 64 events waiting. **With the play network's
evaluation on the Mac** (section 48, the preset since then): ~28-57 min, ~42 on average (48.4).

### 45.11 Deviations from the design (DESIGN.md 2.3, 5.1, 7.1)

* **`"seed"` in the `mtg` spec** (2.3.6 named two keys): the event's match seed must reach the
  match, and zero-core's queued setups take a spec, not a seed. With it a drafted spec is the
  event's match exactly (tested move for move); without it the driver's seed is used as before.
* **`SearchGame::sims_divisor`** (zero-core; 2.3.9 gave deckbuilding 32 / 8 without saying how):
  the core had one budget per seat (`sims_minor` aside, which is never recorded). A game hook was
  the smallest generic change: `team_draft`'s spec key `build_sims_div` (default 1, the presets 2).
  It applies to every network seat, so the draft network's evaluation searches deckbuilding at
  `eval_sims` / 2 too.
* **The human-pick positions** (2.3.8 said "agreement with held-out human picks"): built from
  whole held-out drafts as 4v4 positions of seat 0 (a new `team_draft` spec key, `history`),
  with cards of older cube versions replaced by random current ones and the choice restricted to
  the pack's current cards; the network's policy without search; skipped for other pods than
  4v4. The data stays on E: (raw human picks). A failing evaluation of this kind is logged and
  recorded, not fatal (zero-core's `Run.extra_evals`): a curiosity must not stop a long run.
* **`temp_moves` 120** for the draft (2.3.9 did not set it): zero-core counts an event's searched
  decisions over all eight seats; 120 covers the first pack.
* **The preset keeps the play network's evaluation as mtg-4's** (`eval_every` 2 with mtg-4's 100
  / 50 / 20 x 2 matches): 2.3.9 budgeted ~27 min for it; runs/mtg5 measured 28-69 min of
  evaluation and Elo per evaluated iteration (iterations 2-10), the largest cost after self-play
  (45.10). Not changed (the user's "mtg-4's settings"); an option for the launch (45.12). Since
  section 48 it runs on the Mac (`resolver_cfg` `eval_remote` 'mac'), off the desktop's iteration.
* Unchanged from part 2 and kept: the draft network's play-net evaluation (`eval_yard_net`,
  `eval_games` events) stays out of its Elo fit; the coordinator raises on a malformed result.

### 45.12 Open issues

* **Deckbuilding can collapse to a shared degenerate deck** (45.10: both smokes' draft networks
  swung to all-`OUT` or all-in within 1-2 iterations; all-basic-land decks on both sides would draw
  every match, a fixed point without signal). Watch the draft network's decks in the first
  iterations (the evaluation against random drafters and `consensus` shows it: margins near -12
  of 12). Levers, none applied here: temperature over the whole event in self-play
  (`--temp-moves 800`: an event has ~723 searched decisions, so deckbuilding is sampled by visit
  count and decks stay diverse within a team, at the cost of noisier picks), or a zero-core hook
  for temperature per decision class (deckbuilding sampled, picks greedy after the first pack),
  or a policy that scores `OUT` per card (a layout change: the decided card's slot scoring `OUT`).
  The user chose deckbuilding as picks: section 47.

* **The play network's start**: runs/mtg5's iteration 10 (the copy used here) lost 0/8 to the
  heuristic and won 3/4 against random at 32 simulations; the matches of the first draft
  iterations reward what a weak player can pilot (DESIGN.md 2.2's stated risk). The checkpoint
  is the user's choice at launch.
* **Iteration time** (45.10): the projection is ~42-91 min per iteration (~65 on average) with
  mtg-4's evaluation of the play network every 2nd iteration, against 2.3.9's 35-40 min. Options: the play network's
  `eval_every` 4 (`--resolver-cfg '{"preset": "mtg-4", "eval_every": 4}'`), fewer Elo games, or
  `elo_sims` lower; the draft side is cheap. **Taken (the user's decision of 2026-10-27): the play
  network's evaluation on the Mac, ~42 min per iteration on average (section 48.4).**
* The minutes-per-iteration chart of the dashboard shows self-play, training, evaluation and Elo,
  not the draft network's play-net evaluation phase (`eval_net_s`, in metrics and the table's
  minutes); the dashboard on :5340 runs main's core until main pins this one.
* The human-pick agreement uses 100 of the 2,585 eligible held-out drafts (3,603 positions, SE
  ~0.8 points); the strongest-card reference is fit on the whole corpus (optimistic). The first
  extraction kept 11 drafts with a repeated card (a card in two packs, 32 positions failed in the
  GPU smoke's iteration 0); the tool now drops them.
* zero-core tools that play matches outside a run (`zero.h2h`, `zero.replay`, `zero.targets`,
  `zero.bench`) still do not resolve deferred games (43.5).
* The uniform determinization (2.3.10 question 10, kept) and the heuristic first-pick prior
  (question 6, kept): both are options for later, with the run's own drafts as data.

## 46. Unbounded numbers (M6)

Branch `m6-bounds` (from main 8307cb4; zero-core pinned at e6166c3, unchanged). Magic has no maximum
for counters, life, power, toughness or damage (CR 107.1, 122.1, 119, 120.3, 208), and the cube has
cards that grow a board exponentially in a 60-turn game: Ouroboroid (at the beginning of combat, X
+1/+1 counters on each creature you control, X its power: everything doubles every turn), Bristly
Bill ("double the number of +1/+1 counters on each creature you control", any number of times a
turn), Scythecat Cub (doubles on its second resolution each turn), Jacked Rabbit (tokens equal to its
power), Elspeth, Storm Slayer (twice that many tokens). The engine kept those numbers in 16 bits and
the encoding divided them by small constants, so an exponential board became wrong numbers in the
engine and infinities in the network's input.

### 46.1 The incident

Run mtg5 (preset `mtg-4`, best-of-three) aborted on 2026-10-27 at 19:59 in iteration 14: zero-core's
non-finite guard (`python/zero/finite.py`, core docs/az.md 4.1) refused a self-play shard, "x: 67
row(s) [55010 ...], column(s) [46, 47, 86, 87] (134 +inf)". Those are the per-player globals
`o[22]` (total power / 20) and `o[23]` (total toughness / 20) of both sides (`g[24 + 40 * side]`).
The shard stores x as float16 (largest finite value 65,504), so a total power above 1,310,080 became
+inf. Without the guard (run mtg4, iteration 30) an infinity like this most likely made the NaN
training step that collapsed that run.

The bad shard (`runs/mtg5/debug/iter_0014_r00_000.bad.npz`, 4.4 GB, uncompressed, read as a memory
map) decodes with layout v4 (`mtgz/az/layout_v4.json`: the permanent slots' identity digits and
their P/T status): one game of a best-of-three match, its rows 54,817 to 55,076, both seats, drawn (z =
0.5, 0.5) at the 60-turn cap. One player's board: Ouroboroid, Baloth Prime and up to 15 of its Beast
tokens, Badgermole Cub, Hexdrinker, Vaultborn Tyrant, Sylvan Safekeeper, Sylvan Caryatid, Surrak,
Elusive Hunter, Sentinel of the Nameless City, Screaming Nemesis, Flametongue Kavu, Goblin Engineer,
Ignoble Hierarch, Robber of the Rich, "Name Sticker" Goblin, Cogwork Librarian: 28 to 32 creatures,
nearly all at 65,535-65,539 power (the u16 counter count saturated), total power 1,842 at turn 40,
846,720 at turn 56 and past 1.31 million from row 55,010 (turn 56) to the end. The other player:
Quantum Riddler, Spectacular Spider-Man, Thor, God of Thunder and 14-17 lands, at 12 then 7 life,
never attacked to death: an iteration-14 network that doesn't attack with a board of 30 creatures
of 65,535 power.

### 46.2 The bugs

* **Counts of counters were `u16`** (`Game::counters`, `lki_counters`, `entering_counters`), added
  with `saturating_add`: past 65,535 a creature's +1/+1 counters stopped (a silent saturation:
  Ouroboroid's 65,000 + 65,001 counters left 65,535).
* **Casts that truncate**: a blocker's lethal damage for a trampler's assignment was
  `toughness as u16` (65,539 toughness: lethal 3, so 65,536 damage trampled over a wall it should
  have died to); poison from an amount was `amount as u16` (Etali, Primal Sickness dealing 65,536
  combat damage gave no poison); the heuristic bot's "pay X life" took `x as u16` (65,536: X = 0).
* **Clamps at 65,535**: combat damage (a 65,537-power creature dealt 65,535), marked damage, every
  event amount ("that much": Cecil's life loss, Etali's poison, Screaming Nemesis's damage),
  Ouroboroid's X and "remove n counters".
* **Unchecked arithmetic** (wraps in release builds; most of it out of reach while counters stopped at
  65,535, all of it in reach once they don't): life totals (`life -= amount`: two unblocked creatures
  of 2^31 - 1 power take a 20-life player back to a positive life), crew, saddle and teamwork power
  sums, Cut Down's "total power and toughness 5 or less" (power + toughness wraps negative: a huge
  creature becomes a legal target), "life at most half the starting life" (life x 2), the mana pool
  (`amt += n`, and its `total()` summed six `u8`s), the bot's sums of power and its scores, the
  damage already assigned to a blocker (`u16` sums).
* **A list the size of an amount**: the damage assignment built `(0..=left).collect()` (65,536
  options for 65,535 damage; with wider types a 2^31-element vector).
* **The encoding** divided every number by a constant (P/T / 8, life and total power / 20, counters /
  4 or / 8) with no bound: 1.37 million total power is 68,280, +inf as float16.

### 46.3 The engine's numbers

`engine/src/num.rs` names the types and the saturating conversions; every conversion between them
goes through it (never a bare `as` that could truncate). Each quantity is exact up to its bound and
saturates there (class B, docs/shortcuts.md 46.1-46.6):

| Quantity | Type | Exact up to | Rules |
|---|---|---|---|
| Counters of one kind on an object (+1/+1, -1/-1, loyalty, lore, charge, level, keyword, stun, named) | `num::Count` = `u32` | 4,294,967,295 | CR 122.1, 122.6, 306.5c, 714.3 |
| A player's poison, energy, experience counters | `Count` | 4,294,967,295 | CR 122.1, 122.1f, 704.5c |
| Power and toughness | `i32`, layer 7 summed in `i64` | +-2,147,483,647 | CR 208, 613.4 |
| Life totals; life gained, lost or paid; damage dealt; an effect's amount | `i32` | +-2,147,483,647 | CR 119.3, 120.3 |
| Marked damage, combat damage assigned, damage left to assign, an event's amount | `u32` | at least any amount | CR 120.3e, 510.1, 603.7c |
| Mana of one type in a pool, and in a payment's virtual pool | `u8` | 255 | CR 106.4 |
| Turn numbers (in objects: `entered`, loyalty turns, marks) | `u16` | 65,535 (`GameSpec::max_turns` checked) | engine cap |

`num::count` turns an amount into counters (negative: 0, CR 107.1b), `num::count_amount` counters
into an amount ("X is the number of counters", saturating), `num::amount` a wide sum into an amount,
`num::unsigned` / `num::signed` amounts into and out of marked damage and events. The amount bound is
symmetric (`-AMOUNT_MAX..=AMOUNT_MAX`), so a negation never overflows.

Every change, with its rules basis:

* `layers.rs`: `pt` sums its modifications (effects, statics, "+1/+1 for each", per-counter
  bonuses, +1/+1 and -1/-1 counters) in `i64` and saturates the result (613.4); `counters_on`,
  `counters_of`, `add_counters` (saturating), `remove_counters` take and return `Count`; a leveler's
  ranges compare `Count`s (711).
* `game.rs`: `Obj::damage` `u32`; `Player::{energy, poison, experience}` `Count`; `combat_dmg`
  amounts `u32`; the counter lists `Count`; `gain_life` / `lose_life` / damage to a player through
  `sub_life` (saturating, 119.3, 120.3a); damage to a permanent marks `num::unsigned(amount)`
  (saturating), removes that many loyalty counters (120.3c) or puts that many -1/-1 counters (infect,
  702.90c); poison from infect saturating (702.90b); event amounts `u32`; a planeswalker's printed
  loyalty through `num::count` (306.5b); `Game::with_sideboards` refuses `max_turns` above 65,535.
* `combat.rs`: combat damage is the creature's power in full (510.1a, 510.2: no 65,535 clamp); the
  recipients' and sources' totals saturate; `lethal_needed` is `u32` with saturating sums and the
  toughness converted by `num::unsigned` (510.1c-d, 702.19b); `assign_damage` offers the amounts as a
  range `lo..=hi` (the same options as before: 0..=29 exactly, the blocker's lethal damage, all the
  rest), never a vector of `left` elements.
* `triggers.rs`: `Event::amount`, `EvCtx::amount` `u32` (chapters, class levels, the enter note's
  bits compare as `u32`); `copies::COPIED` `u32::MAX`.
* `resolve.rs`: `Amount::EventAmount`, `SourceCounters` (its last known counters summed in `i64`),
  `Experience`, `CostMvPlus` saturating; `Op::AddMana` through `ManaPool::add`.
* `resolve_ext.rs`, `resolve_walkers.rs`, `faces.rs`, `stack_ext.rs`, `resolve_m7d.rs`,
  `resolve_tail.rs`, `activate.rs`, `pay.rs`: every count put or removed through `num::count` or
  `as Count` (Ouroboroid's and Walking Ballista's amounts, "remove n", connive, energy, experience,
  loyalty costs 606.4, entering counters 614.1c, incubate's exiled count, Gemstone Mine's and Pentad
  Prism's counters); Etali's poison saturating (122.1f); the delayed "that much" (`About::Remembered`)
  `num::unsigned`; "+1/+1 for each" (`ApplyPtPerCount`: signs times a count, its +-999 clamp dropped:
  exact).
* `activate.rs`: crew, saddle and teamwork power summed in `i64` (702.122a, 702.171a, 601.2h);
  "life at most half the starting life" in `i64`.
* `cast.rs`: Cut Down's total power and toughness in `i64` (208.1, 115.1); storm's event amount `u32`.
* `mana.rs`, `pay.rs`: `ManaPool::add` saturates at 255 a type and `total()` sums wide; the payment's
  virtual pool (`vgroups`, `pool_rems`, `pick_multi`, `finish_payment`) saturates the same way; the
  payment's life budget subtracts saturating.
* `resolve_tail.rs`: Coalition Relic's mana (one per charge counter removed, one colour choice each,
  counted by the frame's `u8`) is at most 254 (else the count would wrap and never end).
* `sba.rs`: lethal damage compares `damage` and toughness in `i64` (704.5g); a Saga's chapters as
  `Count`.
* `az/bots.rs`: the heuristic's power, toughness, damage and loyalty arithmetic in `i64` (whether a
  creature kills another, the incoming damage, the value of a permanent, clamped to +-2^30 so its
  bonuses can't overflow), the counters per use as `Count`, "pay X life" clamped to the `NUMBER` range
  before its `u16`, the damage assignment's `NUMBER + lethal` saturating.
* `testkit.rs`: `Scenario::counters` takes a `Count`.

Sizes: `size_of::<Game>()` 1,600 bytes before and after; `Obj` 64 -> 68 bytes (marked damage),
`PendingTrigger` 28 -> 32 and `EvCtx` 20 -> 24 (the event amount), `Player` 184 (the wider counters
fit its padding).

### 46.4 Bounded features

Every value `encode` writes is finite and at most `encode::FEATURE_BOUND` = 256 in magnitude,
whatever the game state, and every value of a normal state is exactly as before.

* **The transform** (`encode::squash`): `v` for |v| <= `SQUASH_FROM` = 128; beyond,
  `sign(v) (128 + ln(1 + |v| - 128))`: continuous, slope 1 at the threshold, monotone. 2^31 power / 8
  is 147.4; all of 192 creatures' 2^31 power / 20 is 151.7; f32::MAX 216.7; an infinity (never
  written) 256. A NaN stays NaN (a bug, never hidden: the fuzzer and zero-core's guard refuse it).
* **The threshold** is above every state of the behaviour digests: their largest value is 68.1 (a
  545/545 creature of random play, P/T / 8, digest game r 424). A first threshold of 16 changed
  three of the 2,500 digest games (r 103, 117, 424: P/T / 8 and total power / 20 between 16 and 68;
  their behaviour without the encodings, `DIGEST_ENCODING=0`, was the same, fa5e570743119dba on
  8307cb4 and on the branch), so it was raised before anything was re-recorded.
  128 keeps a 1,024-power creature, 2,560 life or 2,560 total power exact.
* **Where**: the numbers the game leaves unbounded go through `squash`: per permanent damage, power,
  toughness (computed once, `pt`), loyalty, lore counters, blockers, attackers of a planeswalker; the
  globals' turn, turns left, stack, waiting triggers and extra turns, and per player life, hand,
  library, graveyard and exile sizes, turns, permanents, creatures, lands, untapped lands, total power
  and toughness (summed in `i64`), ready creatures, artifacts, tokens, extra turns, poison, adventure
  cards, temporary triggers, experience; the context's program counter, pile and damage left to
  assign. Everything else is bounded by its type or an engine cap: one-hots and flags; a `u8` field
  over a constant (mana in the pool / 4, X, times, modes, costs: at most 63.75; lands played this turn
  as it is: 255); a slot over 32 or 8; a bag cell is 0.25 a card of one identity in one zone (at
  most 24: the 96-permanent cap of a side; a hidden zone holds fewer cards); the match block (games,
  wins, deck sizes); the team draft's encoding (`draft_encode.rs`: fixed event sizes, 45 picks, 40
  cards a deck: at most 10).
* **Cost**: a compare per squashed value (an earlier version squashed the whole vector after
  encoding: a 33,652-float scan, +4.5 us per decision; dropped).

Layout v4's dimensions and action ids are unchanged (33,652 floats); `mtgz/az/layout_v4.json` is
unchanged.

### 46.5 Invariants

* `tests/fuzz.rs`: at every decision the decider's encoding (`check_encoding`), at the ~1-in-40
  checkpoints both players' encodings of the game and of every world `determinize` samples, and in
  matches `MtgAz`'s encoding of the decider at every decision and of every determinized world: every
  value finite and within `FEATURE_BOUND`. It prints the encodings checked and those with a value
  past the threshold.
* `FUZZ_POOL=exponential`: each player's 23 spells from `GROWTH` (36 cube cards: Ouroboroid, Bristly
  Bill, Scythecat Cub, Jacked Rabbit, Elspeth, Storm Slayer, Baloth Prime, Badgermole Cub, Hexdrinker,
  Walking Ballista, Luminarch Aspirant, Guide of Souls, Ivora, Ajani, Forth Eorlingas!, Pest
  Infestation, Sentinel of the Nameless City, Vaultborn Tyrant, Fanatic of Rhonas, Esika's Chariot,
  Thundertrap Trainer, Sylvan Safekeeper, Surrak; the payoffs Swords to Plowshares, Solitude, Minsc &
  Boo, Etali, Pyrogoyf, Batroc, Cecil, Coalition Relic, Griselbrand, Ocelot Pride, Deep-Cavern Bat,
  Passageway Seer, The One Ring, Tezzeret), 12 Forests and 5 random lands; life 1,000,000,000 (so
  combat with huge creatures happens and life saturates); attackers declared one time in four, so
  most boards grow until the turn cap. It prints the largest power, counter count, marked damage and
  life reached and, from 100 games, fails without a board past 65,535 power. Every check of the
  default mode runs (invariants, clones, `determinize`, option completion). 100 games reach both
  class-B bounds: power 2,147,483,647, counters 4,294,967,295, life 2,147,483,647 (marked damage
  3,145,636).
* `tests/draft.rs` (`DRAFT_FUZZ`): the decider's encoding at every action, and every world's.
* `tests/rules_numbers.rs` (the reproduction): Ouroboroid's trigger on a 65,000-counter creature and
  twenty bears (counters 130,001 and 65,001, total power 1,430,062, a turn later 260,003 and 195,003;
  both encodings bounded), 65,537 combat damage to a 65,537-life player, and a 65,539/65,539
  trampler blocked by a 65,535/65,539 wall (one assignment: all to the wall). On 8307cb4 all three
  fail: counters 65,535 (not 130,001), life 2 (not 0), the assignment offers 3..=29 and "all the
  rest" (lethal 65,539 truncated to 3); the encoding of that board there has 44 features past 64 and
  +inf as float16 at exactly the incident's columns 46, 47 (viewer P0) and 86, 87 (P1).

### 46.6 Measurements

* **The reproduction** (`tests/rules_numbers.rs`) on 8307cb4 (a scratch worktree with its own target
  dir): 3 of 3 fail as in 46.5; that board's encoding: total power 1,365,596, 44 features past 64,
  float16 +inf at 46, 47 (viewer P0) and 86, 87 (P1). On the branch: 3 of 3 pass.
* **Every engine test binary** on the Mac (`cargo test --release -p engine --no-fail-fast`, `az`
  included; 99 binaries and the doctests): 1,375 passed, 0 failed, 13 ignored.
* **Behaviour digests**: combined 12642107250032a7, main's, on the desktop and the Mac (every game
  equal); `DIGEST_ENCODING=0` fa5e570743119dba on 8307cb4 and on the branch.
* **Fuzz** (the Mac): default `FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: 20,000 games (wins 9,991 /
  9,972, 37 draws, 27.0 turns, 214.0 decisions on average), 5,651,655 options completed, 2,000
  matches (5,139 games), 6,033,392 encodings checked, 549 of them with a value past 128 (random
  play's largest creature: 10,368 power, Ouroboroid's doublings), no failure (216 s, the four tests
  in parallel).
  `FUZZ_POOL=interactions`: 20,000 games (10,005 / 9,940, 55 draws), 5,425,503 options, 2,000
  matches (5,127 games), 5,882,094 encodings, 167 past 128, no failure (244 s).
  `FUZZ_POOL=exponential FUZZ_GAMES=5000`: 5,000 games (2,117 / 2,122, 761 draws; 42.5 turns, 397.3
  decisions), 3,862,285 options completed, largest power 2,147,483,647, counters 4,294,967,295,
  marked damage 306,962,933, life 2,147,483,647 (both bounds reached, nothing wraps), 2,384,614
  encodings checked (every one squashed: life starts at 10^9), no failure (889 s: option completion
  on boards of saturated creatures). `DRAFT_FUZZ=20000`: 20,000 events, 11,400,000 actions (every
  decider's encoding checked), 573,334 worlds, 12,534 options completed, 1,866 heuristic-played
  matches, no failure (150 s).
* **Python** (the Mac): `bash tools/build_py.sh`, `python -m pytest -q tests`: 28 passed;
  `python tools/search_smoke.py --specs random,match --seeds 0-1`: 4 of 4 configurations ok (1,600
  games, 1,094,755 decisions, 28.7 million simulations, 118 s).
* **Size**: `size_of::<Game>()` 1,600 bytes on 8307cb4 and the branch.
* **Speed** (the desktop while mtg5 self-plays; `tests/perf.rs` built once at 8307cb4, in its own
  target dir, and once on the branch, run interleaved): `measure`, five runs each: random play
  7.05-7.62 us per decision on 8307cb4 and 7.05-7.81 on the branch (841,706 decisions: the same
  games), clone 1,387-1,822 ns and 1,335-1,628 ns, clone + `determinize` 1,863-2,399 ns and
  1,836-2,424 ns, `determinize` alone 1,222-1,395 ns and 1,137-1,345 ns, `legal_actions` 3 ns and
  3-6 ns.
  `match_layer` (legal actions, clone, encode, step; 93,304 decisions), six runs each: `Game` 14.12-
  14.79 us (mean 14.35) and 13.98-14.96 us (14.42), `MtgAz` mean 14.65 and 14.41 us: no change beyond
  the noise. (The first version squashed the whole vector after encoding, a 33,652-float scan: 18.9-
  19.8 us against 14.4-15.3, +4.5 us; replaced by the squash at the unbounded sites, with power and
  toughness computed once per permanent.)

### 46.7 Limits and what is left

* The class-B bounds above (shortcuts.md 46.1-46.6). The exponential fuzz reaches them: a board past
  2^31 power, or 2^32 counters, is exact no more (two saturated creatures blocking each other both
  die; counters doubled at the bound stay there); nothing wraps, and the encoding of such a state is
  the squash of the saturated numbers.
* Power and toughness saturate below the counter counts (2^31 - 1 against 2^32 - 1): a creature with
  more than 2^31 +1/+1 counters has power 2^31 - 1 (a wider P/T, `i64`, would carry `Obj::lki_power`
  and every amount and life total with it; not needed below 2^31).
* Mana of one type above 255 in a pool saturates (the payment model's `u8` arithmetic); the cube
  reaches it only with two Gaea's Cradle-sized sources of one type in one step.
* The objects of a game are at most 65,534 (`CardIdx` is `u16`; a token or copy takes a new slot):
  `add_card` panics past it (loud).
* Self-play shards written before this branch can hold +inf rows of such boards; the guard refuses
  them (the bad shard stays in `runs/mtg5/debug`). A run continues on this engine with its network
  as it is: layout v4 is unchanged and every normal state encodes bit for bit as before; only states
  past the threshold (0.009% of the default fuzz's encodings) read differently.

## 47. Deckbuilding as picks (M6)

Branch `m6-deckpick` (from 01fa623: main 2b27c84 with M6 parts 1-3 and branch m6-bounds,
section 46). The user's decision of 2026-10-27 after part 3's smoke runs (45.10, 45.12): each
deckbuilding decision **chooses the next pool card to add to the deck**, scored by its card
identity like a draft pick, or `DONE`; after `DONE`, or when the deck is full, the basic lands;
exactly 40 cards; temperature sampling through deckbuilding. zero-core is unchanged (pinned
d68778e); the `mtg` game is unchanged (layout v4, behaviour digests 12642107250032a7); layout
`draft_v1` became `draft_v2` (14,492 floats). DESIGN.md 2.3.3 describes the result.

### 47.1 The problem

Part 3's deckbuilding (41.1, 42.1) was a binary decision per pool card in a build order: the card's
identity (in) or `OUT` = 768, then five basic land counts. A card's "in" was its pool slot's pointer
logit, but `OUT` was one generic logit shared by every card decision of every seat: a small
consistent bias of a fresh network (its value head's random preference between two near-identical
children decides every completed-Q target the same way) flipped every card the same way. Both
smokes swung to an extreme within two iterations: the GPU smoke's draft network put every card
`OUT` and built 40 basic lands (31 Plains) and lost every evaluation match; the CPU smoke put every
card in (0-4 basic lands). In self-play every seat is the same network, so every seat builds the
same broken deck; two decks of 40 basic lands draw every match: a fixed point without signal.

### 47.2 The design

| | Part 3 (41.1, 42.1) | Now |
|---|---|---|
| a deckbuilding decision | the next card of the build order: its identity (in) or `OUT` (768) | **any pool card not in the deck** (its identity: the pool slot's pointer logit, `id_actions`), or **`DONE`** (768, a generic logit), offered once the deck holds `deck_min` pool cards |
| the cards end | after the 45th card (a full deck forced the rest `OUT`) | `DONE`, a full deck (40 pool cards), or the whole pool in the deck (small pools): no `DONE` asked then |
| the basic lands | five counts (`COUNT + n`; Plains .. Forest; at most 31 of a type; the Forest count forced) | unchanged (`Build::count_range`) |
| a legal deck | 0-40 pool cards and basic lands making 40 | **22-40 pool cards** (`deck_min`) and basic lands making 40, at most 31 of a type |
| paths to a deck | one | one per order of its pool cards (k!), one state |
| decisions per seat | 50 (45 cards, 5 counts) | 28-45 (22-40 cards, `DONE` unless the deck is full, 5 counts) |
| options of a card decision | 2 | up to 45 (the cards not in the deck, `DONE`) |

**The minimum (`deck_min`, a spec key: 0-40, default 22, at most the pool).** `DONE` is offered
once the deck holds 22 pool cards. (1) `DONE` is one generic logit, as `OUT` was: without a floor,
a bias toward it ends every deck at its first decision, 40 basic lands, the GPU smoke's fixed point;
now it is out of reach. (2) 22 pool cards means at most 18 basic lands, the most lands a 40-card
deck plays (16-18; the pool's nonbasic lands are pool cards), so no sensible deck is excluded. (3)
It is a format restriction of ours, like exactly 40 cards (CR 100.2b makes 40 a minimum and allows
any number of basic lands), and for `team_draft`'s deckbuilding only: the `mtg` game's sideboarding
(unchanged, layout v4 frozen) may go below it in games 2 and 3. Without it a uniform builder's deck
would hold 0-40 pool cards uniformly (one in five with 32 or more basic lands).

**Order and canonical form: any order.** A deck is a set: the state (`Build::inn`) is the set of
cards in, so the encoding, the legal actions and every later decision are the same whatever order
the cards came in (tested: the same 22 cards in two orders give equal builds, encodings and
options). Every legal deck is one end state, reached by the k! orders of its k pool cards. The
search does not merge them: zero-core's tree has no transpositions (a node table keyed by a
game-provided state hash would be a zero-core change, not made, 47.9), so below the root two
orders of the same cards are two nodes, a state evaluated twice: wasted simulations, not a bias
(each node is the same state's value; the root's options, the recorded decision, are distinct
cards). A simulation reaches a transposition only at depth 2 or more of a deckbuilding search of 32
or 8 simulations over up to 45 options.

**The canonical order was built first and dropped** (commit 3a73e39, replaced by d3a8fa6): each
card added had to come after the last one added in a build order, so each deck had exactly one
path (the user's first example). Measured, it cannot learn the deck's size:

* A card chosen passes over every card before it for good, and with the minimum the cards after a
  window are kept for it: a builder may pass over at most `pool - deck_min` = 23 cards. An
  untrained chooser (uniform priors, Dirichlet noise at full searches, sampling by visit count)
  jumps about half the window per choice and has no room left after ~4 choices; the rest of the
  order is then forced (single options) and the order's last card ends the cards. Every such deck
  has exactly the minimum of pool cards, and `DONE` is never offered, so the deck's size cannot be
  learned until the network first learns a positional rule (add the earliest wanted card, not the
  best one). Without a minimum the walk's deck has H(46) - 1 = 3.4 pool cards (36 basic lands;
  simulated, 3.41 over 20,000 walks).
* The draft fuzz (`DRAFT_FUZZ=20000`): 30,796 random decks at the default minimum, every one
  exactly 22 pool cards, `DONE` offered at none of their card choices. `measure`: a random seat
  made 27.0 deckbuilding decisions, 8.1 of them with two options or more.
* With the build order of part 3 (colour groups, then lands) the forced tail was the pool's lands,
  colourless and gold cards; sorted by identity (alphabetical) it was any cards: the order was by
  identity in that version.
* The smokes (47.8): every self-play deck of the CPU smoke from iteration 3 on, and all 152 of the
  GPU smoke's iteration 1, held exactly 22 pool cards and 18 basic lands; `DONE` was chosen in 5%
  of the CPU smoke's iteration-1 decks and never after.

In any order an untrained builder's deck holds 22-40 pool cards (32.9 on average in random play:
`measure`), `DONE` is offered at every decision from the minimum on, and a card is chosen for
itself: the cards compete head to head, as the user asked.

**Decisions and search cost** (`measure`, the desktop): a random seat makes 38.6 deckbuilding
decisions (35.8 with two options or more) and holds 32.9 pool cards (22-40); `consensus` 31.8
decisions (30.3), 25.8 pool cards. A random 4v4 event is 669 actions with 287 searched
deckbuilding decisions (part 3: 760 and 387); with `consensus`-built decks 243. At deckbuilding's
32 / 8 simulations (p_full 0.25, mean 14) that is ~3,400 leaves per event (part 3: ~5,400), the
picks' 9,400 unchanged.

### 47.3 Temperature through deckbuilding

The draft presets (`team_draft-4v4`, `team_draft-gpu-smoke`, `team_draft-smoke`) set `temp_moves`
1000. zero-core's driver samples the move by visit count while the episode's searched decisions
(every seat's, from the event's first pick: `driver/worker.rs` `decide`) are fewer than
`temp_moves`; a 4v4 event has at most 336 + 8 x 45 = 696 searched decisions, a 2v2 event 348, so
every decision of an event is sampled in self-play, deckbuilding included (part 3's 120 ended
before it). The picks come first, so covering deckbuilding means covering the draft: every pick is
sampled too, at the cost of noisier drafts. "Early on": a relaunch with `--temp-moves 120`
(command-line settings override the run's `config.json`) returns to the first pack only once the
decks settle. Temperature per decision class (deckbuilding sampled, the picks greedy after the
first pack) or a schedule by iteration needs zero-core (47.9).

### 47.4 Layout `draft_v2` (14,492 floats)

| | `draft_v1` | `draft_v2` |
|---|---|---|
| globals | 64 floats | 68: `gl::DONE_OK` (64: `DONE` is one of my options), `POOL_IN` (65: my deck's pool cards / 40), `NEED` (66: pool cards still needed before `DONE` / 40), `DECK_MIN` (67: the minimum / 40); `gl::CARDS_CHOSEN` (59) replaces the build position; `BUILD_CARD` (52): I choose a card |
| offsets | pack 64, pool 1,084, bags 4,504 | pack 68, pool 1,088, bags 4,508 |
| pool slot (after the card code) | in, out (decided), the card decided now, an option, its build-order position | `IN` (in the deck), `OUT` (not in once the cards are chosen), `AVAILABLE` (8: not in while I choose), `OPTION` (adding it is an option now), `BUILD_POS` (10: its rank in the pool by identity / 45) |
| actions | a pool slot scores putting its card in (`id_actions: 0`); `OUT` and the counts generic | a pool slot scores adding its card (`id_actions: 0`); `DONE` and the counts generic |

`mtgz/az/layout_draft_v2.json` (version 2; `MTGZ_WRITE_LAYOUT=1` rewrites it; `draft_v1`'s file is
gone: no run used it), loaded by `mtgz/az/__init__.py`. A teammate's deck bag holds every card in
its deck so far. Every value stays within the event's sizes (the fuzz checks `FEATURE_BOUND`).

### 47.5 Bots and the information model

`consensus` adds its plan's cards (the heuristic deckbuilder's deck: 23 spells and its
lands, at least 22 pool cards from a 45-card pool) by identity, then `DONE`, then the counts
nearest the plan's: still `pools::build_deck`'s deck (tested). `random` is zero-core's uniform
bot, as before (41.4): random picks and a random legal deck, now 22-40 pool cards (it never used
the heuristic deckbuilder; zero-core's `make_bot` answers `random` before the game's bots). The
heuristic-played matches, the deferred glue (the payload's decks and sideboards) and the
human-pick yardstick (picks only) are unchanged.

**The information model is unchanged**: deckbuilding emits no key; `determinize` keeps the
viewer's and its teammates' builds, gives an opponent who has built the heuristic deck of its
known pool and restarts one building now (41.2); the decider's options never change (tested at
every check and in the fuzz). `check_invariants`: the deck's bits inside the pool, the counts after
the cards, room while choosing, a deck the seat built at least the minimum (a world's redrawn
opponent deck, the heuristic's stand-in, excepted).

### 47.6 Tests

* `engine/tests/draft.rs` (24, `measure` among them): `deckbuilding_adds_any_card_until_done`
  (45-card pools of a 2v2 event: every card not in the deck offered, `DONE` an error before 22 pool
  cards and offered from then on, a card already in the deck an error; the 22 smallest cards then
  `DONE` and the smallest counts: 22 pool cards and 18 Forests; the 22 largest and the largest
  counts: 18 Plains, the other counts forced; every card: the 40th ends the cards, no `DONE` asked,
  every count forced to 0, five cards in the sideboard; 22 cards in a shuffled order and in its
  reverse: equal builds, encodings and options); `every_legal_deck_is_reachable_and_one_state`
  (every path of the card choices of 6-card pools, minimums 0, 1, 4, 6: 64, 63, 22 and 1 card sets,
  1,957, 1,956, 1,800 and 720 paths, each set ending exactly k! of them; a 7-card pool, minimum 3:
  99 sets, 13,650 paths; the states with the orders merged of 12-card pools, minimums 0, 5, 12:
  4,096, 3,302 and 1 end states, and a 15-card pool, minimum 3: 32,647, each set of at least the
  minimum once and no other; whole decks of a two-card pool, minimums 0, 1, 2: 487,676, 354,400 and
  110,880 decks (cards and five counts), each reached by its k! paths; `DONE` offered exactly from
  the minimum at every decision); `encoding_slots_hold_the_pack_and_the_picks` (while choosing:
  every card not in the deck `AVAILABLE` and an `OPTION`, the cards in `IN`, none `OUT`; `DONE_OK`,
  `NEED`, `POOL_IN`); `spec_parsing` (`deck_min`, at most the pool, 41 and -1 refused; 14,492
  features; `DONE` 768); the fuzz (`DRAFT_FUZZ`: the minimum 22, 0, 40 or random by event, `DONE`
  offered exactly when the deck holds the minimum at every card choice, every finished deck at least
  the minimum and 40 cards; since 8d7dbaf its heuristic-played matches are played one at a time,
  `draft_bots::play_match_heuristic`, and a match that panics in the `mtg` engine is recorded with
  its spec, the event resolved with random results and the fuzz goes on, failing at the end with
  every such match listed, so one engine fault no longer hides the other 19,999 events);
  `every_pool_has_45_cards_and_decks_are_legal` (at least 22 pool cards; `consensus` builds
  `pools::build_deck`'s deck); `measure` (deckbuilding decisions per seat, the pool cards of random
  and `consensus` decks). The information-model tests are unchanged and pass.
* `tests/test_team_draft.py`: 14,492 features, `deck_min` 41 refused, 28-45 deckbuilding decisions
  per seat, every deck of the payload 40 cards with at least 22 pool cards and deck + sideboard =
  the pool; the token network on `draft_v2`. `tests/test_drafted.py`: the pool slots score every
  card a builder may add, `DONE` generic.

### 47.7 Checks

At d3a8fa6 (the design) and 8d7dbaf (the fuzz's record-and-continue; the docs commit changes
nothing else), the Mac running the committed HEAD (`tools/mac.sh`, bit-identical to the desktop,
section 44) unless the desktop is named:

* **Every engine test binary** (`cargo test --release -p engine --no-fail-fast`, `az` included:
  the library, the 99 test files, the doctests): **1,382 passed, 0 failed, 13 ignored** at
  d3a8fa6 (363 s) and at 8d7dbaf; the same at 3a73e39 (370 s).
* **Behaviour digests** (`--test digest -- --ignored`): combined **12642107250032a7**, unchanged
  (the Mac at 3a73e39 and d3a8fa6, 8.8 s; the desktop, 13.0 s).
* **`DRAFT_FUZZ=20000`**: **fails: one of the 1,866 heuristic-played matches panicked in the `mtg`
  engine, the pre-existing `PayMana` fault of 47.10 (case 2), not a deckbuilding regression**;
  every check of the draft held (the desktop at 8d7dbaf, 208 s): 20,000 events
  (4v4, 2v2, 3v3, 1v1; the minimum 22, 0, 40 or random), 9,871,224 actions, 5,040,000 searched
  picks and 4,087,506 searched deckbuilding decisions (part 3: 11.4 million actions, 5,804,002),
  497,122 determinized worlds, 37,488 offered options played to the end of their event, team A
  8,080 / 8,188 / 3,732; the 30,800 random decks at the default minimum held 32.84 pool cards
  (33.59 card choices per seat, 11.59 of them with `DONE` offered), the 89,200 others 31.28. The
  Mac at d3a8fa6, before the fuzz recorded such panics, stopped at the same match (52 s). At
  3a73e39 (build order): no failure, 96 s, 8,586,574 actions, 804,331 searched deckbuilding
  decisions, 431,358 worlds, 14,415 options completed; its 30,796 random decks at the default
  minimum all exactly 22 pool cards.
* **`FUZZ_POOL=drafted FUZZ_MATCHES=2000`** (`--test fuzz`): no failure, 41 s: 2,000 drafted
  matches, 4,984 games (984 matches of three, 761 drawn games), 493.5 decisions per match, 43,548
  sideboarding decisions, 5,845 decks changed, 49,086 sampled worlds (first picks resampled in
  42,272, decks in 48,955); the default games 300, 209.7 decisions, 80,016 options completed
  (part 3, 45.9: 5,015 games, 508.0 decisions, 46,741 sideboarding decisions; at 3a73e39: 5,060,
  513.0, 46,482).
* **Python** (`bash tools/build_py.sh`, `python -m pytest -q tests`): the desktop **34 passed**
  (36 s); the Mac 33 passed, 1 skipped (the human-pick test: no held-out drafts there).
* **Search smoke** (`python tools/search_smoke.py --specs team_draft,team_draft_2v2,random,match
  --seeds 0-1`, 24 events per draft configuration): **0 failed of 8**, 126 s. 4v4: 48 events, 576
  matches, 31,120 searched draft decisions (648 per event; part 3: 704), 660,152 simulations (13,750
  per event; part 3: 14,600), 7,750 samples, parked records at most 10.6 MB; 2v2: 48 events, 192
  matches, 15,528 decisions, 3,850 samples, at most 5.5 MB; every event resolved with a sane
  outcome, every match ended; random 800 games, 328,537 decisions, 8.4 million simulations; match
  800 matches, 766,218 decisions, 20.3 million simulations (part 3's numbers: the `mtg` path is
  unchanged). At 3a73e39 (build order) the same command failed 1 of 8: `seed 0 team_draft_2v2`, a
  drafted match panicked in the `mtg` engine: the pre-existing `PayMana` fault of 47.10 (case 1),
  not a deckbuilding regression.
* **`measure`** (`--test draft measure -- --ignored`; the desktop above, the Mac: 9,864 random 4v4
  events per second, 0.15 us per action, `encode` 2.5 us, clone + `determinize` of deckbuilding
  states 3.9 us).

### 47.8 The smoke runs (scratch run directories under `E:\mtg-zero-scratch\deckpick`)

Part 3's two presets, in scratch run directories: `team_draft-smoke` (2v2 on the CPU, tiny
networks, picks 8 / 2 simulations, deckbuilding 4 / 1) and `team_draft-gpu-smoke` (4v4, the
run's 128 x 4 network and search: picks 64 / 16, deckbuilding 32 / 8, 8 events per iteration,
16 drafting, 96 matches in flight, the play network from a copy of runs/mtg5's `best.pt`,
iteration 10 at the copy, part 3's play network; its own evaluations cut to 2 matches,
`--draft-eval-net-every 0`), both with `temp_moves` 1000. The GPU was free (runs/mtg5 stopped at
its iteration 15; nvidia-smi: another project's model server only). A wrapper of `mtgz.az.run`
(scratch, not committed: `run_decklog.py`) logged every parked event's decks with the phase that
drafted it; `deckstats.py` reads them. Per self-play iteration: the decks of the events parked
during it (every seat; rolling self-play drafts ahead, so more events than the 4 or 8 resolved),
their pool cards (= cards added before `DONE`), spells and nonbasic lands among them, basic
lands (and their types), all lands, the colours of their nonland cards (a colour with at least
two), `DONE` chosen (the cards ended before 40 pool cards), distinct decks. "Checkpoint" rows:
each iteration's network building by itself at its evaluation settings (8 events of 4 seats,
4 simulations, deckbuilding at 2, no noise, no temperature: `ckpt_decks.py`).

**Build order (the canonical form, 3a73e39), CPU 2v2, 6 iterations** (all done):

| iteration | decks | pool cards | spells / nonbasic lands | basic lands (types) | lands | colours | `DONE` chosen | checkpoint's decks |
|---:|---:|---|---|---|---:|---:|---:|---|
| 1 | 56 | 22.4 (22-28) | 17.9 / 4.5 | 17.6 (2.2) | 22.1 | 4.3 | 5% | 22.0, 18 basic lands (iteration 0's: 40 pool cards, its ties) |
| 2 | 16 | 22.7 (22-30) | 18.9 / 3.8 | 17.3 (3.4) | 21.1 | 4.3 | 0% | 22.0 |
| 3 | 16 | 22.0 (all) | 18.2 / 3.8 | 18.0 (4.6) | 21.8 | 4.5 | 0% | 22.0 |
| 4 | 12 | 22.0 (all) | 18.4 / 3.6 | 18.0 (4.4) | 21.6 | 4.6 | 0% | 22.0 |
| 5 | 20 | 22.0 (all) | 17.8 / 4.2 | 18.0 (2.4) | 22.2 | 4.3 | 0% | |
| 6 | 20 | 22.0 (all) | 16.9 / 5.0 | 18.0 (4.2) | 23.1 | 4.2 | 0% | |

The draft network against `consensus` (4 events, heuristic-played matches) won 0, 0, 1, 0.5, 0.5,
2 and 1 of 4 at iterations 0-6, against random 0, 1, 2, 0.5, 1, 2 and 1 of 2. Every deck was the
minimum with the order's tail from iteration 3 on: pinned (47.2). (Iterations 5-6's checkpoints
were not measured: the engine had moved to any order by then.)

**Any order (d3a8fa6), CPU 2v2, 6 iterations** (all done; the same binary but for a then
temporary spec switch, the final code's behaviour):

| iteration | decks | pool cards | spells / nonbasic lands | basic lands (types) | lands | colours | `DONE` chosen | checkpoint's decks |
|---:|---:|---|---|---|---:|---:|---:|---|
| 1 | 48 | 38.3 (23-40; 36 of 48 at 40) | 31.9 / 6.4 | 1.7 (0.4) | 8.1 | 5.0 | 25% | 24.7 pool cards, `DONE` 97%, 16.7 lands (iteration 0's: 40, its ties) |
| 2 | 24 | 25.6 (22-40) | 23.2 / 2.4 | 14.4 (1.7) | 16.8 | 4.7 | 96% | 22.0, 18 basic lands of 5 types |
| 3 | 12 | 24.2 (22-40) | 20.7 / 3.6 | 15.8 (3.8) | 19.3 | 4.5 | 92% | 22.0 (3.1 types) |
| 4 | 24 | 22.2 (22-23) | 19.9 / 2.2 | 17.8 (3.4) | 20.1 | 4.4 | 100% | 22.0 (3.0 types) |
| 5 | 12 | 22.2 (22-24) | 18.8 / 3.4 | 17.8 (2.7) | 21.2 | 4.5 | 100% | 22.0 (5 types) |
| 6 | 8 | 22.2 (22-23) | 18.5 / 3.8 | 17.8 (3.4) | 21.5 | 4.2 | 100% | 22.0, 18 basic lands of one type |

Against `consensus` 1, 1, 1, 0.5, 0, 3 and 1.5 of 4 at iterations 0-6, against random 0, 0, 0, 0,
0.5, 1.5 and 0 of 2. Every deck distinct in every iteration.

**Any order (d3a8fa6), GPU 4v4, 3 iterations** (`--iters 3 --draft-eval-net-every 0`, the play
network's own evaluation cut to 2 matches; the checkpoint rows at the run's evaluation settings:
4 events of 8 seats, 32 simulations, deckbuilding at 16, on the GPU beside the run):

| iteration | decks | pool cards | spells / nonbasic lands | basic lands (types) | lands | colours | `DONE` chosen | checkpoint's decks |
|---:|---:|---|---|---|---:|---:|---:|---|
| 1 | 256 | 37.2 (22-40; 149 at 40) | 30.9 / 6.3 | 2.8 (0.8) | 9.1 | 4.9 | 42% | 22.0 (all), 18 basic lands (2.2 types), `DONE` 100% (iteration 0's, the fresh network: 31.0, all: its ties) |
| 2 | 64 | 22.7 (22-30) | 18.3 / 4.4 | 17.3 (2.7) | 21.7 | 4.5 | 100% | 22.0 (all), 18 basic lands of 2 types |
| 3 | 56 | 23.4 (22-38) | 19.1 / 4.3 | 16.6 (2.7) | 20.9 | 4.7 | 100% | 22.0 (all), 18 basic lands of 2 types, 21.0 lands |

Self-play per iteration (the draft; the matches by the play network): iteration 1 resolved 8 events
in 1,648 s (640.6 searched draft decisions per event, 1,229 samples, 447,744 draft leaves at 272 per
second; 183 matches, 400 per hour, 29,993 play samples), iteration 2 in 774 s (646.5, 1,324 samples;
102 matches, 475 per hour), iteration 3 in 361 s (652.2, 1,233 samples; 83 matches, 827 per hour:
the decks of 22 pool cards end their matches sooner); the draft's completed-Q targets over 16.8-17.1
options, normalised entropy 0.93; no failure, `done`. The draft network against `consensus` (4
events, heuristic-played matches) won 0, 1.5, 0.5 and 0 of 4 at iterations 0-3 (margins -2.5, -1.5,
-4.5 and -6.0 of 12), against random 1, 1, 0.5 and 0 of 2; the draft Elo 655, 849, 720 and 646 (the
best iteration 1).

**Build order (3a73e39), GPU 4v4** (stopped during iteration 1's self-play, once the CPU smoke
had shown the pin, to give the GPU to the final design): iteration 0's evaluation 0 of 4 against
`consensus`, 2 of 2 against random; iteration 1's self-play (the fresh network with search and
temperature) parked 19 events: **all 152 decks exactly 22 pool cards and 18 basic lands, `DONE`
chosen in none** (spells 17.7, nonbasic lands 4.3, lands 22.3, colours 4.4).

**Judgement: the collapse of part 3 is gone, but deckbuilding still swings to an extreme, now in
its size.** Gone: no deck of 40 basic lands can be built (the minimum), no single logit decides
every card (each card is its own pointer logit), and every deck of every iteration of both smokes
is distinct, with varied cards. Not gone: in both smokes the deck's size swung within one
iteration, from the fresh network's 37-38 pool cards (8-9 lands: its tied priors and its fast
searches without noise add the first card of the list, the smallest identity, before `DONE`, the
last) to `DONE` as soon as it is offered: every checkpoint from iteration 1 or 2 on builds
exactly 22 pool cards and 18 basic lands (of one or two types), and self-play decks sit at 22-24
with `DONE` chosen in 100%. `DONE` is the one generic logit left, and it swung as `OUT` did; the
floor caught it at a playable deck (~19 spells, ~21 lands) instead of 40 basic lands, so the
matches keep a signal, but the size is no longer explored, and the GPU smoke's iteration-3 network
lost all its evaluation events (0 of 4 against `consensus`, margin -6.0 of 12; 0 of 2 against
random, whose decks hold 33 pool cards). The colours were not learned in 3-6 iterations (4-5
colours with two cards or more in every iteration). Whether 18 basic lands is the weak play
network's own best answer for five-colour piles (it plays the self-play matches; the evaluation is
heuristic-played: DESIGN.md 2.2's stated risk) or a bias of the value head was not separated here.
Build order would not have learned the size at all (pinned at the minimum from the first iteration,
`DONE` never offered).

### 47.9 zero-core

No change is needed (pinned d68778e). Possible hooks, not made (zero-core is being changed
elsewhere):

* **Temperature per decision class**: a `SearchGame` hook (e.g. `sample_move() -> Option<bool>`)
  that `decide` consults before `temp_moves`, so deckbuilding is sampled for a whole run while the
  picks follow `temp_moves` (today 1000 samples every pick too).
* **A temperature schedule** by iteration in `zero.run` (e.g. `temp_moves` after iteration N), so
  "early on" ends by itself instead of by a relaunch.
* **Transpositions**: one node per state in the search (a table keyed by a game-provided state
  hash), so the orders of the same deck share their statistics below the root; not needed at
  deckbuilding's budget.
* **A game's own `random`**: `make_bot` answers `random` before `SearchGame::make_bot`, and
  `zero.run` plays its random yardstick as the bot named `random`; a `random` drafter that builds
  with the heuristic deckbuilder needs one of the two to change.

### 47.10 Open issues

* **An `mtg` engine fault the new decks reach** (not fixed here: the `mtg` game is frozen in this
  branch): the engine offers a cast its payment cannot pay, and `PayMana` panics (`pay.rs:1569`,
  "PayMana: no way to pay ..."). Two cases: (1) at 3a73e39 (build order), the search smoke's `seed 0
  team_draft_2v2`, drafted match `8/3`: P0 casts Green Sun's Zenith for X = 10 ({10}{G}); (2) at
  d3a8fa6 and 8d7dbaf, `DRAFT_FUZZ=20000`'s event 8657, match 0: P1 casts {4}{B}{B}{B}{B}
  (Griselbrand, in its deck). Both decks hold Wizard's Rockets ({X}, sacrifice it: X mana in any
  combination of colours), and both panics' payment classes hold its definition (def 532: a
  sacrifice-itself converting class with no fixed production): a lead (the castability check
  counting what the payment cannot use), not a diagnosis. Both match specs
  (`E:\mtg-zero-scratch\deckpick\paymana_gsz_spec.json`, `paymana_bbbb_spec.json`) panic with every
  driver seed when heuristic seats play them, and identically on 01fa623's engine (its run-01fa623
  checkout): the fault is part 3's. Both decks are random drafts' decks of a kind part 3's binary
  deckbuilding rarely built; the engine is part 3's, unchanged. In a co-training run such a panic in
  a match stops the run (a search worker's panic aborts zero-core's driver), so it is to be fixed
  before the first run, in a branch of its own (the behaviour digests may change).
* **The deck's size swung to the floor** (47.8): `DONE` is still one generic logit, and in both
  smokes the network chose it as soon as it was offered from its first or second trained
  iteration on (every checkpoint's deck 22 pool cards and 18 basic lands; self-play 22-24). The
  floor makes that a playable deck instead of 40 basic lands, but the size is then no longer
  explored: temperature samples `DONE` by its visits, and the value network sees few decks of
  other sizes. Not separated: the weak play network's own preference (it plays the self-play
  matches) from a value-head bias. To watch in the run's first iterations (the decks' pool cards and
  lands: the evaluation decks against `consensus` show them); levers: `deck_min` 23 (the
  textbook 23 spells and 17 basic lands as the floor, a spec key), temperature per decision class
  (47.9), or more events per iteration than the smokes' 4 and 8.
* **The basic land counts** are generic logits too (`COUNT + n`, unchanged): the CPU smoke's
  iteration-6 network put all 18 basic lands of its evaluation decks into one type. The counts read
  the deck's coloured pips (`gl::DECK_PIPS`), so a trained network can follow them; to watch.
* **Colours**: every smoke deck had cards of 4-5 colours (two or more cards each): no colour
  learning in 3-6 iterations of 4-8 events. The card choices are the pointer logits now, so a
  network that learns its colours adds those cards directly.
* **`random`'s decks changed**: zero-core's uniform bot builds 22-40 pool cards (32.9 on average,
  `measure`) where part 3's built about 22.5; the draft Elo's random reference is not comparable
  with part 3's. It never used the heuristic deckbuilder (only `consensus` does; 47.9 on making
  it).
* **Temperature covers the picks too** (47.3): every pick of an event is sampled in self-play
  while `temp_moves` is 1000; lower it on a relaunch once the decks settle, or make it per decision
  class in zero-core (47.9).
* The canonical-order version (3a73e39) stays in the branch's history: its tests were replaced by
  d3a8fa6's.

## 48. The play network's evaluation on the Mac (co-training runs, M6)

Branch `m6-cotrain-remote` (from 01fa623: main 2b27c84 + m6-bounds; presets, tests and docs only: the
engine is 01fa623's), zero-core pinned at `feat/cotrain-remote` af44741 (core main d68778e + f8adf48,
the code, and af44741, its docs/az.md 5.2 and 11.5). The user's decision of 2026-10-27: in co-training
runs the MacBook evaluates the play network (`mtg`, the resolver), as single-game runs already could
(section 44); the draft network's evaluation stays on the desktop. Why: the play network's evaluation
with mtg-4's settings every 2nd iteration was the largest cost of the 4v4 run after self-play (45.10:
28-69 min on the desktop per evaluated iteration, ~65 min per iteration on average against 2.3.9's
35-40), and the Mac plays it in ~9 minutes (44.4) while the desktop goes on.

### 48.1 What was built

* **zero-core** (its docs/az.md 11.5): `eval_remote` in `resolver_cfg` - the resolver's own setting,
  meaning there what it means for a single-game run (no new run setting) - gives the resolver's
  `Run` a `zero.remote.RemoteEval`: each evaluated iteration of the play network (`eval_every`) is
  queued and the run goes on; the remote's thread ships the checkpoints to the mirror
  **`~/mtgz-remote/<run name>.resolver`** (a sibling of single-game mirrors such as `mtg5`: the same
  lock, one evaluation at a time in `~/mtgz-remote`, and the same `overrides.json`), runs `python -m
  mtgz.az.remote eval` there, appends the result to **`<run>/resolver/evals.jsonl`** and copies the
  mirror's Elo back to `<run>/resolver/elo/` and `resolver/models/index.json`. The resolver's rows in
  `resolver/metrics.jsonl` carry no evaluation; the log's lines are `[resolver] ... (remote mac, Ns)`.
  A restart queues the play network's evaluations without a result again; an unreachable Mac is
  retried every 5 minutes on that thread while both networks train and the draft network is
  evaluated; at the end of its iterations the run waits for the queue (progress phase `remote
  evaluations`; PAUSE stops the wait). The run's top-level `eval_remote` would be the draft network's
  and is refused (its events' matches need the run's resolver drivers); an unknown remote name is
  refused at start.
* **The presets** (`mtgz/az/__init__.py`): `team_draft-4v4` and `team_draft-gpu-smoke` add
  `'eval_remote': 'mac'` to their `resolver_cfg`; `team_draft-smoke` (the CPU smoke the tests run)
  evaluates everything on the desktop (48.2 adds the key on its command line).
* **The Mac's side is unchanged**, and covers co-training as it is:
  * `RunCommitRemote` (`mtgz/az/mac.py`) resolves to the commit of the co-training run's checkout:
    refused at start if the checkout has uncommitted changes, `repo` = `~/mtg-zero-wt/run-<sha12>`,
    preflighted over ssh (missing, another commit, changed or unstamped: refused at start with the
    command that prepares it).
  * `bash tools/mac.sh --eval-checkout` builds the whole engine there (`mtg` and `team_draft`); the
    resolver's evaluation uses `mtg`.
  * A resolver mirror's config.json is the resolver's settings: a single-game `mtg` run's (mtg-4's,
    with the Mac's overrides). `python -m mtgz.az.remote eval` checks the checkout (HEAD =
    `MTGZ_REMOTE_COMMIT`, nothing changed, the engine stamp) before evaluating any mirror, so the
    commit check applies to the play network's evaluations exactly as to mtg5's.
* **Tests**: `tests/test_mac_remote.py::test_a_resolver_mirror_is_evaluated_after_the_same_check`
  (team_draft-smoke's resolver mirror: `python -m mtgz.az.remote eval` refuses it at another commit
  before evaluating or locking anything; the remote side then evaluates it: the heuristic, random,
  Elo); `tests/test_drafted.py::test_cotraining_presets` (the three presets' remotes). zero-core's
  `tests/test_cotrain.py`: the toy co-training run with the resolver's remote `local`, a restart with
  evaluations pending, an unreachable remote (its docs/az.md 11.5).

### 48.2 End to end: the Mac for real (`team_draft-smoke`, 2v2, CPU)

At 9307c09 (committed), `bash tools/mac.sh --eval-checkout` (41 s: `~/mtg-zero-wt/run-9307c09d6350`,
engine built in 38 s, checked), then in a scratch directory, CUDA hidden, the desktop shared with
other projects' runs:

```
python -m mtgz.az.run --run E:/mtg-zero-scratch/cotrain-remote/td_mac --preset team_draft-smoke --iters 3 \
  --resolver-cfg '<team_draft-smoke resolver_cfg + "eval_remote": "mac">'
```

The log: `[resolver] evaluations on the remote 'mac' (greggkeithley@100.95.248.116, mirror
/Users/greggkeithley/mtgz-remote/td_mac.resolver)`; the preflight passed. Iterations 0-3, 3 min 2 s,
`done`:

| | where | iteration 0 | 1 | 2 | 3 |
|---|---|---:|---:|---:|---:|
| play network: vs the heuristic (4 matches), random (2), Elo | **Mac** (the mirror's phases: evaluation, Elo) | 0.9 s, 0.0 s | 1.5, 4.0 | 1.4, 4.7 | 1.3, 4.6 |
| the same, wall clock from the queue (shipping, ssh, the checkout check) | Mac | 4.3 s | 7.7 | 8.5 | 8.2 |
| draft network: vs `consensus` (4 events), random (2) | desktop | 3.6 s | 3.6 | 4.9 | 4.0 |
| draft network: vs `consensus`, play-network matches (`eval_yard_net`, every 2nd) | desktop | 18.9 s | - | 36.6 | - |
| draft network: Elo | desktop | 0.5 s | 3.0 | 9.5 | 6.3 |
| iteration (self-play + training + the desktop's evaluations) | desktop | 23 s | 46 | 84 | 29 |

* `resolver/evals.jsonl`: four rows, `host` mac (vs the heuristic 0/4 each, random 0, 0, 0.5, 1 of 2;
  Elo -99, -43, 358, 399); `resolver/metrics.jsonl`: four rows without an evaluation;
  `resolver/elo/` (ratings of iterations 0-3, random, the heuristic) and `resolver/models/index.json`
  came back. The draft network's rows in `metrics.jsonl` have their evaluations, Elo and
  `eval_yard_net` (iterations 0 and 2), all played on the desktop; no `evals.jsonl` at the run root.
* The Mac's mirror `~/mtgz-remote/td_mac.resolver`: config.json (`mtg`, mtg-smoke's network and
  evaluation settings, the Mac's overrides), checkpoints 0-3, its metrics and Elo, next to `mtg5`.
* The dashboard (`zero.dashboard`, `/api/run/td_mac`): the play network's tab has the Mac's rows
  merged (win rates, Elo by iteration) and its Elo table from the copied-back `resolver/elo/`.
* **A local baseline** (the same command without the remote, right after, the desktop busier):
  3 iterations in 8 min 31 s; the play network's evaluation on the desktop 2.6-9.6 s and its Elo
  0-74 s per iteration (16.9, 59.8 and 74.0 s for iterations 1-3: the Elo matches grow with the
  checkpoints played), all of it on the iteration's critical path. The two runs' self-play differs
  (co-training self-play is not deterministic: two drivers on two threads), so only the phases
  compare.
* **Refusals in the co-training path**: a run started from this worktree with an uncommitted change
  stopped at start ("eval_remote: the run's checkout ... has uncommitted changes, so no remote
  checkout can hold its code"); a run started at a commit whose Mac checkout does not exist stopped
  at start with the command that prepares it (48.5).

### 48.3 Turning it on for the 4v4 run

As for mtg5 (44.5), from the run's pinned worktree at a commit with this branch's code:

1. `git -C ~/mtg-zero worktree add --detach ~/mtg-zero-wt/run-<s7> <S>`, there
   `CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/run-<s7> bash tools/build_py.sh`.
2. There: `bash tools/mac.sh --eval-checkout` (~40 s: the Mac's `~/mtg-zero-wt/run-<s12>`).
3. `python tools/launch_hidden.py <run>/supervisor_console.txt python tools/supervise.py <run>
   --preset team_draft-4v4 --resolver-init <the mtg5 checkpoint>`: the preset's `resolver_cfg` has
   `eval_remote` 'mac'. The log says `[resolver] evaluations on the remote 'mac' (..., mirror
   /Users/greggkeithley/mtgz-remote/<run>.resolver)`; each evaluated iteration of the play network
   logs `[resolver] iter N vs heuristic (remote mac, ...s)` and `[resolver] iter N elo ... (remote
   mac)`; `<run>/resolver/evals.jsonl` holds the results.
4. Off (or on, for a run started without it), at a restart: `--resolver-cfg` with the whole dict (a
   flag replaces the saved one), e.g. `--resolver-cfg '{"preset": "mtg-4", "eval_every": 2,
   "eval_remote": ""}'`: from then the play network is evaluated on the desktop again, from its
   `resolver/elo/` as the Mac last left it. Every re-pin repeats steps 1-2.

### 48.4 The projected 4v4 iteration

45.10's projection per iteration: self-play 16-34 min, training 2-3, the draft network's evaluation
5-10 (heuristic-played events, every iteration), the play network's 28-69 every 2nd (runs/mtg5 on the
desktop), the draft network's play-network events (`eval_yard_net`) 20-40 every 4th: 42-91 min, ~65
on average. With the play network's evaluation on the Mac, the desktop's iteration is self-play,
training and the draft network's evaluation:

| | desktop per iteration | average |
|---|---:|---:|
| iterations 1, 2, 3 of 4 (no `eval_yard_net`) | 23-47 min | ~35 |
| every 4th (`eval_yard_net`) | 43-87 min | ~65 |
| **average** | **28-57 min** | **~42 (1.4 iterations per hour; 100 iterations ~2.9 days, against ~4.5)** |

The Mac's share: the play network's evaluation (mtg-4's: 100 matches against the heuristic, 50
against random, the Elo matches, ~9.3 min with shipping on the idle Mac, 44.4) every 2nd iteration,
~85 min apart: busy ~11% of the time, so it never falls behind; each result lands ~10 min after its
checkpoint, later by up to one evaluation when another mtgz run's (mtg5's) holds the lock in
`~/mtgz-remote`, or by the shared cores when other projects evaluate there (`~/kbz-remote`). The
largest evaluation left on the desktop is `eval_yard_net` (48.6).

### 48.5 Checks

At 9307c09 (the engine is 01fa623's: this branch changes presets, tests and docs, and the core's
Rust is unchanged):

* **Engine tests on the Mac** (`bash tools/mac.sh 'cargo test --release --no-fail-fast'`: every test
  binary of the workspace, `az` included; the desktop runs no `az`): **1,387 passed, 0 failed, 13
  ignored** (`az` 14 passed, card_ir 5), 6 min 28 s with the builds.
* **Behaviour digests** (Mac, `cargo test --release -p engine --test digest -- --ignored`): combined
  digest **12642107250032a7** (2,000 random and 500 heuristic games in 9.0 s).
* **Python** (desktop, `bash tools/build_py.sh`, 2 min 28 s; `python -m pytest -q tests`): **35
  passed** (01fa623's 34 and the new resolver-mirror test), 74 s.
* **Search smoke** (Mac, `python tools/search_smoke.py --specs random,match,team_draft --seeds 0-1`):
  **0 failed of 6**, 123 s: random 400 games each (161,770 and 166,767 decisions), match 400 each
  (385,154 and 381,064 decisions, 10.2 and 10.1 million simulations), team_draft 24 events each, all
  resolved (team A won 6 and 10, drew 3 and 4), 288 matches each, 16,874 and 16,911 draft
  decisions, parked records at most 11.55 and 16.52 MB.
* **Refusals at start** in the co-training path (48.2): an uncommitted change in the run's checkout;
  a commit without its Mac checkout (this section's docs commit 049564a, which has no `--eval-checkout`): stopped at start, "eval_remote:
  greggkeithley@100.95.248.116:/Users/greggkeithley/mtg-zero-wt/run-049564aa0bfa is not the run's
  code (049564aa0bfa): zsh:cd:1: no such file or directory: ..." and "prepare it: in <checkout>: bash
  tools/mac.sh --eval-checkout".
* **zero-core** (`feat/cotrain-remote` af44741): `cargo test --release` 44 passed (unchanged Rust);
  `python -m pytest tests` **110 passed** (105 and 5 new: the resolver's remote `local`, a restart
  with evaluations pending, an unreachable remote, the run's own `eval_remote` and an unknown name
  refused), 296 s.
  `SMOKE_TARGET=cotrainremote bash tools/smoke_all.sh --with-this-core` (Glass Road c1f56dc,
  Caverna a6fad73, Kingdom Builder 4202a4a, MTG Zero main 2b27c84, each at its HEAD with core
  af44741): **GREEN**, 19 steps, 42 min: core cargo test 44, pytest 110; Glass Road cargo test 46,
  pytest 33, `glassroad-smoke` 4 rows; Caverna 47, 32, `caverna-smoke` 4 rows; Kingdom Builder 105
  (4 ignored), 45, `kingdombuilder-smoke` 4 rows; MTG Zero cargo test 1,387 (every binary, `az`
  included as smoke_all runs it; 13 ignored), pytest 34, `mtg-smoke` 4 rows.

### 48.6 Limits and open issues

* **`eval_yard_net` stays on the desktop** (the user's decision: the draft network's evaluation is
  the desktop's). It could go to the Mac at moderate cost - a second remote command rebuilding
  zero-core's `CoRun.event_match` from a mirror holding both networks' checkpoints and the run's
  settings, the commit check and the lock as they are - and it is now the largest evaluation of the
  desktop's iteration (32 events, 384 play-network matches at 32 simulations every 4th iteration,
  20-40 min: ~5-10 min of the ~42 average). Not done; the other lever is `draft_eval_net_every` 8.
* The Mac's checkout check covers the code, not the shared venv `~/.venvs/dominion-zero` (44.6); the
  Mac's results are those of its arithmetic (44.4), so the play network's Elo differs from what the
  desktop would fit by the usual noise (48.2's local baseline: iteration 2 at 203 here against 358
  there, on other self-play).
* `models/best.pt` of the play network is not copied back (index.json is); zerodash (zero-core's
  `dashboard/`) reads `metrics.jsonl` and `elo/` only (44.6): the game dashboard merges
  `resolver/evals.jsonl`.
* A run that finishes its iterations waits for the play network's last evaluations (at most 6
  hours) unless STOP exists; PAUSE ends the wait (the restart queues them again).
* The scratch mirror `~/mtgz-remote/td_mac.resolver` was removed after 48.2; the checkout
  `~/mtg-zero-wt/run-9307c09d6350` and its target dir (0.4 GB) stay, as other run checkouts do.

## 49. Castability and payment agree (M6)

Branch `m6-payfix` (from main 01fa623; zero-core pinned at d68778e, unchanged). A spell or ability
is offered only if it can be completed (sections 6 and 18): the castability checks (`can_pay_with`
over `pay::cands`, `cost_payable`, the X maxima) must count exactly what the payment (`PayMana`,
`PayCosts`, delve) can then do, or an offered option ends in a decision without options, a panic
that fails a self-play run (zero-core makes a panic fail the run). Two drafted matches did:
`PayMana: no way to pay {4}{B}{B}{B}{B}` (Griselbrand) and `PayMana: no way to pay {10}{G}` (Green
Sun's Zenith, X = 10). Both are one fault, Wizard's Rockets counted twice. The audit and an
agreement fuzz written for it (49.4) found six more, all fixed here: five where the payment could not
do what castability counted, one where both counted mana the rules don't give (Mishra's Workshop's
turned into colours); and the checks found a seventh, in hidden information (49.6).

### 49.1 The two reproductions

* **Griselbrand** (the drafted match `e8657`, heuristic seats, every driver seed): P1's mana classes
  in the panic were Wizard's Rockets (def 532: `{X}, {T}, sacrifice this artifact: add X mana in any
  combination of colors`; `convert`, `sac_self`, `res` TAP | SAC, the production `[0; 6]` because
  its X is the mana it turns into colours, section 34.2), Fiery Islet, Mox Jet, Creeping Tar Pit,
  Savai Triome, Sacred Foundry, and Urza, Lord High Artificer's "tap an untapped artifact you
  control: add {U}" on four artifacts (`TAP_FOR` = `Trigger(255)`, mask {U}), the Rockets among them.
  Eight cards, three of them black. Castability (`cands`) kept the converter apart
  (`Cands::convert`) and put the Rockets' card's other way, tapped for Urza's {U}, among the units;
  `payable_converted` turned every unit into any colour, the Rockets' own {U} included: eight units,
  {4}{B}{B}{B}{B} payable, the cast offered. The payment: picking the Rockets uses its card (TAP |
  SAC: `used_res`), so its `TAP_FOR` source is gone, seven units; `convert_ok` false, no single-mana
  source completes the cost, no option: the panic.
* **Green Sun's Zenith** (`search_smoke --specs team_draft_2v2` on a branch with other
  deckbuilding; replayed on main with the spec): P0 with the Rockets, Urza, two Talismans, an Island
  and Plains, no green source. X was chosen up to what castability counted (`can_pay_spell` for each
  X, CR 107.3, 601.2f): 10 with the Rockets' {U} and its colours, 9 in truth, and {10}{G} had no
  payment. The same fault: the X maxima are exact once `can_pay` is.

The deckpick agent's lead (the Rockets, def 532, in both decks; how castability models a mana
ability with {X}) pointed at the same class: the {X} model itself (section 34.2: picked, the
payment's mana may be of any colour) is right; what was wrong is that the Rockets' activation, which
taps and sacrifices it, did not rule out its card's other uses.

### 49.2 The fixes, with their rules

* **A converter's card is used up** (CR 601.2g, 605.3a; 702.126a for improvise). `Cands::convert`
  lists every usable converter with the index in `Cands::alts` of its card's other ways (Urza's
  tapped artifact, improvise, any other source on that card: they are one alternative of their own
  there, never folded into `fixed`); `cands_payable_c` converts without them
  (`payable_converted(.., skip)`). The payment was already right (`used_res`). Covers Urza's
  `TAP_FOR`, an improvise spell (Kappa Cannoneer: the Rockets tapped for improvise or activated, not
  both) and waterbend.
* **Mana with a spending restriction is not converted** (CR 106.6): Mishra's Workshop's "spend
  this mana only to cast artifact spells" can't pay the Rockets' {X} (activating a mana ability is
  not casting an artifact spell), so the Rockets doesn't turn it into colours. `mana::RESTRICTED`
  marks such a unit (bit 7, which on a need is `NOT_MADE`; `fits` reads a unit's types and `MADE`
  only, and a need without `NOT_MADE` never has the bit); `convert` leaves it. It is set on the
  candidates of a class with `only` (`cands`), the pool's artifact-only {C} when the payment may
  spend it (`add_pool`), the virtual pool's (a kind + 16 in `vgroups`, the Workshop's production in
  `feasible_prods`) and converge's units. `pool_rems` keeps "artifact-only {C} spent before other
  {C}" as the most of it among the spends of the same mana; without the Rockets every split is
  feasible, so the pools left and their order are as before. Delighted Halfling's restricted mana is
  any colour already. Digest game h 411 (below) is this case in random play.
* **Sources a graveyard cost needs, with no mana to pay** (CR 601.2g-h): `pay_mana`'s early finish
  ("paid by single-mana sources alone") waits until `PayMana::forced` is empty. Psychic Frog's
  "exile three cards from your graveyard" costs no mana; with one card in the graveyard and two
  Lotus Petals, the payment activated the first Petal, found nothing left to pay and finished
  without the second: the exile had two cards for three (`ChooseGraveyardCard` without options).
* **Delve's mana sources** (CR 702.66a, 601.2g-h; 614.1a): the delve decision offers a source that
  sacrifices itself (Lotus Petal, Gemstone Mine on its last counter) when the rest is payable with
  the graveyard's cards *and the sources after it* (castability, `delve_classes`, counts each such
  source as its mana and one more generic mana; the decision counted the graveyard only, so with
  two such sources and too few cards it offered neither, only "stop", and the payment had no way).
  `delve_classes` now counts only a card that reaches the graveyard (`feeds_graveyard`, as
  `delve_sources` and the decision do: not under Yawgmoth's Will or an opponent's Dauthi
  Voidwalker, "exile it instead").
* **Escape's graveyard fed by a discard** (CR 702.138a, 601.2h, 614.1a; ruling bea302d9):
  `discards_feed` counts the hand cards the cost may discard that would go to the graveyard (it read
  the first card in hand: with Blightsteel Colossus, shuffled into its library instead, first the
  escape wasn't offered; with another card first, the Colossus was then offered as the discard, a
  dead end); the `DiscardCost` decision offers only such cards while the graveyard would be short
  by as many cards as discards are left (as the sacrifice decision already did).
* **One source of a kind, one that works** (CR 707.2, 601.2g): the payment offers one source per
  class (same definition and ability: the canonical choice, no permutations). A class's members
  can differ in their cards' other sources or reservations: Saheeli's copy of Llanowar Elves on Mox
  Sapphire has the Elves' "{T}: add {G}" and is an artifact as well, which Urza's ability taps for
  {U}. With the copy first in the class, the copy, the Elves and a Mountain had no option for Oko
  ({1}{U}{G}): the copy picked for {G} leaves no {U}, and the classes after it can't make {G}
  (castability, card by card, was right). The payment now offers the first free member whose pick
  leaves the cost payable, each kind of member (its reservation and the other classes its card is
  in) tried once, so a class of plain lands costs nothing more.

### 49.3 The audit

The class model was read for every way castability can count more than the payment can use, and
the agreement fuzz (49.4) checks it on random boards:

* **Sources that yield nothing.** Only the Rockets has a production of no mana (`[0; 6]`, units
  `Some([])`): `might_pay` counts it as 0 and `cands` as a converter only. Other sources that could
  make nothing make no class: `PerPermanent` with no matching permanent, `Linked`, `Chosen`,
  `IfCounter` and `ColorAmong` without a colour (`yield_of` returns `None`); `Fixed` and `IfElse`
  always make some mana (the lowering). A production of nothing is never offered (`feasible_prods`
  wants the virtual pool's spend to grow). Nothing to fix.
* **Conversion.** The used-up card and the restricted mana (49.2). The converter's own budget (a
  sacrifice reservation: Tinker's artifact) is charged on both sides (`reserved`); `MADE` units keep
  `MADE` when converted on both sides (13.15); the Rockets is offered only when nothing else pays
  (13.14) and castability tries without it first: the same set.
* **Triggered mana.** Land-subtype bonuses (Nissa, Who Shakes the World) and the creature bonus
  (Badgermole Cub) are in the class's yield (`yield_with`; the Lumberjack's in `sac_classes`), read
  by both sides; a permanent tapped for Urza's ability gets no bonus on either side (not "tapped for
  mana", CR 106.12a). Nothing to fix.
* **`allow` and `keep`.** Restricted classes are filtered by `keep.allow` in `cands` and in the
  payment's options; hand castability takes the `allow` path whenever a restricted class or
  artifact-only pool mana exists (else the plain candidates, allow 0, the same sources); `exclude`,
  the sacrifice, discard and exile reservations come from `keep_for` on both sides; the X maxima use
  the cast's keep (allow, life paid later), abilities' X the ability's keep and exclusion. Found:
  the escape discard's (49.2).
* **Pooled mana.** The pool's units in castability (`add_pool`) and payment (`vgroups`) agree (the
  artifact-only {C} held unless the payment may spend it, now also never converted).
* **X maxima.** Spells (`can_pay_spell` per X, kicker {X}, "pay X life"), abilities (`choose_x`):
  exact as soon as `can_pay` is; the Zenith's X was the Rockets fault.
* **Graveyard costs.** Delve, escape and "exile n cards from your graveyard" with self-sacrificing
  mana sources: four faults (49.2), three found by the fuzz, the Dauthi Voidwalker one by reading.
* **Classes.** A class's members (same definition and ability) are alike for their own mana, not
  always for their cards' other sources: a copy (49.2), found by the fuzz.

### 49.4 The agreement fuzz

`tests/rules_castpay.rs`, `castability_and_payment_agree_on_random_cube_boards`: random boards of
the cube (P0: one to eight lands, up to four permanents with a mana ability or a mana bonus, the
Rockets and Urza one time in four each, the counters a mana ability removes, sometimes another
permanent such as a tax or a lock; one to three cards in hand and sometimes a mana permanent, a
graveyard half the time, mana in the pool one time in four, a low life one time in four; P1:
permanents, lands, a hand to respond with). At P0's priority every spell and ability offered is
followed six times to the next priority (always the first option, always the last: the largest X,
the last source; four random paths, the largest X one time in two); then a random action and the
stack resolved (each player passing three times in four, else acting at random: P1's responses,
"unless pays" costs), three rounds per board. A panic anywhere is a failure, reported with the
board, the actions before it and the path (`CASTPAY_LOG=1`: the game's log before the panicking
step). `CASTPAY_BOARDS` (default 4,000, 2.4 s), `CASTPAY_PATHS` (6), `CASTPAY_FROM` (replay a
board). On main 01fa623: 19 failures in the default 4,000 boards, 100 in 20,000 (the twelve the
failure lists: the Rockets with Urza). On the branch the fuzz found four of 49.2's faults in turn,
each fixed before the next run: board 85,480 (Psychic Frog and two Petals), 226,634 (escape and
Blightsteel Colossus), 980,602 (delve with Gemstone Mine and a Petal), 2,505,552 (the copy of
Llanowar Elves). At the branch's last code: 0 failures in 4,400,000 boards (49.7).

`tests/fuzz.rs`: a fourteenth interaction group, "mana abilities x costs" (the Rockets, Urza, Kappa
Cannoneer, Mishra's Workshop, Delighted Halfling, Lotus Petal, Black Lotus, Lion's Eye Diamond,
Psychic Frog, Abhorrent Oculus, Treasure Cruise, Dig Through Time, Uro, Green Sun's Zenith, Walking
Ballista, Griselbrand, Cryogen Relic, Esika's Chariot, Sunken Ruins, Chromatic Star, Pentad Prism,
Gemstone Mine, Squandered Resources, Orcish Lumberjack, Arena of Glory, Rain of Filth, Badgermole
Cub, Nissa, Sol Ring, Mana Vault, Grim and Basalt Monolith, Talisman of Dominance, the Spirit Guides,
Mana Confluence, Ancient Tomb, Loot, Gaea's Cradle, Coalition Relic, Mox Opal, Mox Amber, Aang's
waterbend, Tinker, Thalia, Bolas's Citadel), so `FUZZ_POOL=interactions`' option completion
(`check_options_complete`: every option of a sampled decision followed to the next priority, a
panic on the way a failure) meets these boards in games and matches too.

### 49.5 Tests

`tests/rules_castpay.rs`, every test on real cube cards; all eleven fail on main 01fa623 (built
with the test file alone), all pass on the branch:

* `cr_601_2g_wizards_rockets_converting_is_not_also_tapped_for_urza`: the Rockets, Urza, three
  Swamps and four Islands: {3}{B}{B}{B}{B} payable, {4}{B}{B}{B}{B} not, no Griselbrand; {5}{B}{B}{B}
  payable (the Rockets an artifact for Urza); a fifth Island: cast, every payment path completes,
  the Rockets sacrificed.
* `cr_107_3_x_is_at_most_what_the_payment_can_pay_with_wizards_rockets`: the Rockets, Urza, three
  Plains: Green Sun's Zenith's X is 0 to 2 (main offered 3), every path of every X completes.
* `cr_702_126a_wizards_rockets_converting_is_not_also_tapped_for_improvise`: the Rockets and five
  Mountains, no Kappa Cannoneer; six Mountains: cast and paid.
* `cr_106_6_wizards_rockets_does_not_turn_mishras_workshop_mana_into_colours`: the Workshop and the
  Rockets: no Cryogen Relic ({1}{U}); with a Mountain the Rockets turns its mana blue, the Workshop
  pays {1}, two artifact-only {C} stay in the pool.
* `cr_601_2h_two_lotus_petals_for_psychic_frogs_graveyard_cost`: the Frog's "exile three" with one
  graveyard card and two Petals: every path completes, both Petals and the card exiled, two mana in
  the pool; one Petal: not offered.
* `cr_702_66a_two_lotus_petals_delved_for_treasure_cruise`: four Islands, two Petals, an empty
  graveyard: Treasure Cruise cast, both Petals delved.
* `cr_614_1a_no_delve_for_lotus_petals_exiled_by_dauthi_voidwalker`: the same board under P1's
  Dauthi Voidwalker: not offered.
* `cr_702_138a_escape_discard_must_feed_the_graveyard`: Underworld Breach, two other graveyard
  cards, Bone Shards escaped with "discard a card": the discard is the Island, never Blightsteel
  Colossus, whichever is first in hand; the Colossus alone: no escape.
* `cr_707_2_a_copy_that_is_also_an_artifact_for_urza_is_not_the_one_offered_for_its_green`: Mox
  Sapphire a copy of Llanowar Elves (Saheeli's -2), the Elves, Urza and a Mountain: Oko cast, every
  payment path completes (main: `PayMana: no way to pay {1}{U}{G}`).
* `cr_712_11b_a_face_down_card_cast_with_a_choice_of_faces_is_revealed` (49.6).
* `castability_and_payment_agree_on_random_cube_boards` (49.4).

### 49.6 Found by the checks

`FUZZ_POOL=interactions` with the new group dealt new decks and failed at seed 7165, a fault in
hidden information, not in payment, that 01fa623's engine has as well: P1 played Shelldock Isle's
hideaway card (exiled face down), Brazen Borrower // Petty Theft, both faces castable, so a
`ChooseFace` decision came first (`begin_cast_either_face`, a cast during a resolution); in a world
`determinize` sampled for P0 the face-down card was dealt again, a card without another face, and
the other face's option panicked ("a card with another face"). At priority such a card is revealed
before the choice (`Choice::Faces`: it is public either way, CR 712.11b, 715.3, 601.2a); during a
resolution it now is too. Test `cr_712_11b_...` (fails on 01fa623 with that panic). Digest game r
1925 is the same play (the reveal's chance key as the Borrower leaves the Isle).

### 49.7 Checks

At 567b2c3, the branch's last code (the documentation commit after it changes none):

* **The reproductions** (the drafted specs replayed with heuristic seats over driver seeds, the
  search smoke's driver; `mtgz/_engine` built at 567b2c3 on the desktop): the Griselbrand match
  `e8657`, seeds 0-49: 50 complete, 0 panics (P1 wins it 2-1, every seed alike); the Green Sun's
  Zenith spec, seeds 0-49: 50 complete, 0 panics (P1 wins 2-0). On 01fa623 every seed tried
  panicked (e8657 seeds 9-10, the Zenith spec seeds 0-5).
* **Every engine test binary**: the Mac (`cargo test --release -p engine --no-fail-fast`, `az`
  included; 101 binaries and the doctests): 1,393 passed, 0 failed, 13 ignored (434 s). The desktop
  without `az` (the library and the 99 other test binaries): 1,379 passed, 0 failed, 13 ignored.
* **Behaviour digests**: two games re-recorded, each attributed by playing it on both sides: h 411
  (in 1b1dca5: P1 cast Sewer-veillance Cam, {U}, an artifact, on turn 3 with Mishra's Workshop's
  {C} turned blue by Wizard's Rockets, the illegal payment 49.2 removes: the Cam is not offered
  there) and r 1925 (in 790107e: the reveal's chance key as Brazen Borrower is cast from Shelldock
  Isle, 49.6; the same actions). Combined digest 12642107250032a7 (01fa623) -> de6fa2e70663316c
  (1b1dca5) -> 1ed7e08dd5078f55 (790107e; 567b2c3 changed none), on the desktop and the Mac.
* **Fuzz** (the Mac): default `FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: 20,000 games (wins 9,991 /
  9,972, 37 draws), 5,651,654 options completed (5,651,655 on 01fa623: one option no longer offered),
  2,000 matches (5,139 games), 6,033,392 encodings checked, no failure (254 s).
  `FUZZ_POOL=interactions` (fourteen groups): 20,000 games (9,888 / 10,063, 49 draws), 5,552,281
  options completed, 2,000 matches (5,117 games), 6,140,401 encodings, no failure (306 s; at 1b1dca5
  it stopped at seed 7165, 49.6). `FUZZ_POOL=drafted`: 2,000 drafted matches (5,015 games), 50,506
  worlds, and the default games, no failure (256 s). `FUZZ_POOL=exponential FUZZ_GAMES=5000
  FUZZ_MATCHES=2000`: 5,000 games (2,117 / 2,122, 761 draws), 3,862,285 options completed, largest
  power 2,147,483,647, counters 4,294,967,295, life 2,147,483,647, no failure (1,112 s).
  `DRAFT_FUZZ=20000`: 20,000 events, 11,400,000 actions, 573,334 worlds, 12,534 options completed,
  1,866 heuristic-played matches, no match panic recorded, no failure (175 s).
* **The agreement fuzz** (49.4): boards 0 to 3,199,999 on the Mac (eight processes, 178 s) and
  3,200,000 to 4,399,999 on the desktop (four): 4,400,000 boards, 10,216,228 priority decisions,
  31,197,142 options (14,406,163 of them casts), 187,182,852 paths, 0 failures.
* **Python**: `bash tools/build_py.sh`, `python -m pytest -q tests`: 34 passed on the desktop; 33
  passed, 1 skipped on the Mac (no CUDA).
* **Search smoke** (the Mac): `python tools/search_smoke.py --specs
  random,match,team_draft,team_draft_2v2 --seeds 0-3`: 0 failed of 16 configurations (3,200 games,
  2,176,739 decisions, 57.1 million simulations; 192 team-draft events, 1,536 matches, 101,409 draft
  decisions; 306 s).
* **Speed** (the payment's options and candidates changed; the desktop, `tests/perf.rs` `measure`
  built once at 01fa623 and once at 567b2c3, one target dir, run interleaved, five runs each): random
  play 6.58-6.61 us per decision on 01fa623 and 6.51-6.54 on the branch (841,706 decisions: the same
  games), clone 1,202-1,225 ns and 1,208-1,249 ns, clone + `determinize` 1,601-1,640 ns and
  1,619-1,653 ns, `determinize` alone 991-1,002 ns and 937-962 ns, `legal_actions` 5-6 ns and 5 ns:
  no change beyond the noise. `size_of::<Game>()` 1,600 bytes on both.

### 49.8 Limits

* The fuzz finds over-counting (an option offered that can't be completed), not under-counting (a
  cost the rules let a player pay that the engine doesn't offer): there is no second payment model
  to compare with. The known under-counts are the shortcuts' (docs/shortcuts.md 13.12-13.17).
* Only P0's main-phase priority is checked option by option; instants on the opponent's turn and in
  combat go through the same castability code and are covered by `FUZZ_POOL=interactions`' option
  completion.

## 50. Keeping the Mac busy (M6)

Branch `m6-macuse` (from main at 94d43c1, rebuilt on main 075649c once deckbuilding as picks was
merged; tools, one test-harness change and docs: the engine is main's, digests 12642107250032a7).
Beyond its remote evaluations (sections 44 and 48) the MacBook (M4 Max, 12 performance and 4
efficiency cores, 64 GB) was mostly idle. Three uses: a measurement the M6 design needed (how noisy
one team-draft event's outcome is, and what that says about matches per event and the draft's
training targets, 50.1), a fuzz farm that never stops (50.2), and the design of remote self-play,
with the Mac's self-play throughput measured (50.3).

**The rule for everything here: remote evaluations come first.** zero-core's remote side takes
one evaluation at a time per workdir with a lock directory (`zero/remote.py`: `~/mtgz-remote/.eval.lock`,
its `owner` file "<pid> <time>"). `tools/mac_backoff.py` holds the rule: a job runs at nice 19 in a
process group of its own and is **stopped (SIGSTOP) while a lock is held by a live owner**, then
continued (SIGCONT); the locks watched are mtgz's and the other zero-core games' remotes on the Mac
(`~/kbz-remote`, `~/domz-remote`: their evaluations share the cores too), or `$MTGZ_BACKOFF_LOCKS`.
A lock whose owner died does not count (zero-core removes it at the next evaluation); one without its
owner file yet does (being created). As a wrapper: `python tools/mac_backoff.py -- <command>`; the
fuzz farm uses its `Governor` (SIGSTOP / SIGCONT of the jobs' groups), the noise study the cooperative
form (it stops asking for leaf batches, so its search threads idle). A stopped job's wall clock
runs on: a job that times itself counts only its running time. Checked live (a 75-second lock in
`~/mtgz-remote` owned by a `sleep`): the farm's two fuzz jobs were stopped 4 s after it appeared
(state `T`, 0% CPU) and continued when its owner died; the study's driver idled at 0% CPU and went
on. `tools/mac.sh --pin NAME` (new): a checkout `~/mtg-zero-wt/NAME-<sha12>` at exactly this commit
with its engine built (its own target dir), which later `mac.sh` commands never check anything out
in: a long job started there keeps its code and its engine module while the branch's own checkout
moves on.

### 50.1 Match noise: how many matches per event?

The question (the user's): does it make sense to play more than 3 matches per player per draft for
a clearer signal? A team-draft event's outcome (ENGINE.md 41, 45: 12 best-of-three matches, 3 per
player, 3 of the 4 Latin-square rotations; per seat the team result 1 / 0 / 0.5, the margin = (team
match wins - opponent match wins) / 12) mixes the real difference between the two teams' decks with
the luck of 12 matches. How much of each?

**Method** (`tools/match_noise.py`; data `~/mtgz-noise/` on the Mac, the summary copied to
`E:/mtg-zero-scratch/noise/`):

* **Events**: 60 drafted 4v4 events (8 seats, 3 packs of 15, decks built: exactly zero-core's
  driver with bot seats, parked at the deferred terminal, their payloads kept) in two regimes:
  every seat a `random` drafter (random picks, a random legal 40-card deck: close to an untrained
  draft network) or a `consensus` drafter (the M1 pick strength and the heuristic deckbuilder:
  closer to a trained one). Driver seeds 1000 and 1001. Drafted at 94d43c1, before deckbuilding
  as picks (section 47): a random drafter's deck is the old random deckbuilding's (each pool card in
  or out, then random basic counts); `consensus`'s is the heuristic deckbuilder's either way.
* **Replays**: each event's 12 matches, as `mtgz.az.drafted` makes them (the co-training run's
  specs), played K times with different match seeds (replay 0 the event's own seed, replay k a
  splitmix of it: other shuffles) and search seeds, in a queue_only rolling driver (the resolver's
  of a co-training run). Players: **the play network** (runs/mtg5's iteration 14, 128 x 4 tokens,
  preset mtg-4; copied to the Mac) **with mtg-4's self-play search** (64 / 16 simulations, p_full
  0.25, minor decisions at 4, temperature for 60 decisions, root noise, completed-Q: exactly what a
  co-training run's resolver plays), K = 4; as contrasts the heuristic bot on both sides, K = 16,
  and the network with the same search less its temperature and root noise (`net-greedy`: the first
  30 events of each regime, K = 2). 5,760, 23,040 and 1,440 match replays. Replays are played
  replay by replay, so a study still running has whole replays (two already give every estimate).
  The study ran from pinned checkouts of this branch before its rebuild on 075649c
  (`~/mtg-zero-wt/noise-5843007ef161` and, for `net-greedy`, `noise-e6b8c262a790`: `mac.sh --pin`;
  the engine of 94d43c1, whose `mtg` path main has not changed since: the digests; their play path
  is the final tool's, which added the contrasts' players and the analysis' refinements); the
  analysis is the final tool's (`tools/match_noise.py analyze` reads the results files).
* **The numbers**, per player and regime, with x = +1 / 0 / -1 a match's result for team A:
  one-way random effects (method of moments) at three levels: the **luck** per match sigma^2 (the
  variance of x across replays of the same match), the **between-event** variance tau^2 of an
  event's true per-match mean (the 12 matches' replay means, less the luck they still hold), and
  the **within-event** spread omega^2 of the matches' true means (which player meets which), split
  by a two-way additive fit per event (A's players' strengths less B's) into the players' strengths
  and the pairing interactions. Then for each event label - **D** (A's match wins - B's), the
  **margin** D / 12, the **team result** T (1 / 0.5 / 0) and each player's **own record** (its 3
  matches' wins - losses) - its luck variance across replays, its true variance across events and
  its **reliability** (true / (true + luck): the share of the label's variance that a replay
  reproduces; 1 is a noiseless label), and how often two replays of an event give different team
  results. Bootstrap over events (300 resamples): 90% intervals. **Projections** to m matches per
  event (m / 4 rounds of Latin-square rotations; 16 = the full round robin, 4 per player; 24 = 6
  rounds): the margin's reliability is m / (m + sigma^2 / tau^2) (Spearman-Brown: the matches'
  luck is independent, checked: D's luck variance is 12 x the per-match luck within noise); the team
  result's comes from a model (each event's true mean moment-matched to tau^2, its players'
  strengths and interactions drawn with the measured variances, fixed for the event; each match
  won, lost or drawn; D's exact distribution by convolution), which at m = 12 reproduces the
  measured reliability within its interval. **What one replay's label tells** about the seat's true
  own record (its expected wins - losses given the decks) and about its team's true margin: squared
  correlations from pairs of different replays of an event (independent luck: Cov(label in replay
  k, own record in replay k') estimates Cov(label, true own record)). **The noise-optimal matches
  per event** (the two-stage sampling optimum: a budget spent on N events of d + m
  match-equivalents, the error of what the events teach ~ (tau^2 + sigma^2 / m) / N, minimal at
  **m* = sqrt(d sigma^2 / tau^2)**), with the draft's cost d in match-equivalents: 45.10's
  measurements put an event's draft at ~12% of its GPU at 12 matches, d ~ 1.5.
* **Checked on synthetic data** first (400 events x 4 replays with known truth: tau^2 0.070, omega^2
  0.090, luck 0.798 -> estimated 0.065, 0.091, 0.793; margin reliability 0.479 direct against 0.495
  from the components; team result 0.369 against the model's 0.352).
* **Engine panics**: zero-core's driver fails on a search worker's panic (the payment panics of
  m6-payfix: `PayMana: no way to pay {1}{R}`, `{5}{U}`, `{7}{U}`, `{6}{U}{U}` in these drafted
  matches); the study records the match the error names (`results/<player>.failed.jsonl`), leaves it
  out and starts a new driver for the rest. 5 of 23,040 heuristic match replays (deterministic:
  the same match fails again), none of the 5,760 with the network.

**Results** (60 events per regime; 90% intervals in brackets):

| | network, random drafters | network, consensus drafters | heuristic, random drafters | heuristic, consensus drafters |
|---|---:|---:|---:|---:|
| events x replays (match replays) | 60 x 4.00 (2,880) | 60 x 4.00 (2,880) | 60 x 15.97 (11,496) | 60 x 15.95 (11,484) |
| team A per match: mean x, draws | +0.031, 0.052 | -0.061, 0.033 | +0.018, 0.000 | -0.064, 0.000 |
| luck per match sigma^2 | 0.798 | 0.849 | 0.686 | 0.766 |
| between events tau^2 (true per-match mean) | 0.0145 | 0.0239 | 0.0651 | 0.0585 |
| within an event omega^2 = strengths + interactions | 0.135 = 0.114 + 0.022 | 0.090 = 0.082 + 0.008 | 0.249 = 0.236 + 0.013 | 0.172 = 0.159 + 0.012 |
| a match's P(A wins): quartiles of the true values | 0.36 / 0.49 / 0.62 | 0.34 / 0.45 / 0.57 | 0.32 / 0.51 / 0.70 | 0.31 / 0.47 / 0.63 |
| **sigma^2 / tau^2** | **54.84** [36.94, 116.87] | **35.54** [23.96, 68.58] | **10.53** [8.16, 14.35] | **13.11** [10.19, 18.31] |
| **m\* (d = 1 / 1.5 / 3)** | 7.41 / **9.07** [7.44, 13.24] / 12.83 | 5.96 / **7.30** [5.99, 10.14] / 10.33 | 3.25 / **3.97** [3.50, 4.64] / 5.62 | 3.62 / **4.43** [3.91, 5.24] / 6.27 |
| 12 matches: margin reliability | 0.17 [0.07, 0.26] | 0.27 [0.14, 0.37] | 0.52 [0.44, 0.59] | 0.49 [0.41, 0.55] |
| 12 matches: team result reliability | 0.14 [0.04, 0.24] | 0.18 [0.06, 0.30] | 0.44 [0.36, 0.51] | 0.41 [0.34, 0.49] |
| 12 matches: two replays' team results differ | 0.57 | 0.53 | 0.44 | 0.46 |
| own record (3 matches) reliability | 0.23 | 0.20 | 0.46 | 0.38 |
| D's luck variance (12 x the per-match luck) | 9.83 (9.57) | 9.62 (10.19) | 8.59 (8.23) | 8.88 (9.19) |
| 4 matches: margin / team result reliability (model) | 0.07 / 0.07 | 0.10 / 0.09 | 0.28 / 0.23 | 0.23 / 0.21 |
| 8 matches: margin / team result reliability (model) | 0.13 / 0.11 | 0.18 / 0.15 | 0.43 / 0.34 | 0.38 / 0.31 |
| 12 matches: margin / team result reliability (model) | 0.18 / 0.14 | 0.25 / 0.19 | 0.53 / 0.41 | 0.48 / 0.38 |
| 16 matches: margin / team result reliability (model) | 0.23 / 0.17 | 0.31 / 0.23 | 0.60 / 0.47 | 0.55 / 0.43 |
| 24 matches: margin / team result reliability (model) | 0.30 / 0.23 | 0.40 / 0.30 | 0.69 / 0.54 | 0.65 / 0.50 |
| 48 matches: margin / team result reliability (model) | 0.47 / 0.35 | 0.57 / 0.42 | 0.82 / 0.66 | 0.79 / 0.62 |
| one replay's team result: r^2 with the seat's true own record / the team's true margin | 0.03 / 0.15 | 0.08 / 0.21 | 0.14 / 0.43 | 0.17 / 0.42 |
| one replay's margin: r^2 with the seat's true own record / the team's true margin | 0.03 / 0.18 | 0.11 / 0.30 | 0.17 / 0.52 | 0.20 / 0.50 |
| one replay's own record: r^2 with the seat's true own record / the team's true margin | 0.23 / 0.04 | 0.20 / 0.08 | 0.46 / 0.15 | 0.38 / 0.15 |

* **The network's labels are very noisy.** With the play network a match is mostly luck: sigma^2
  0.80-0.85 per match (of at most ~1), against a between-event signal tau^2 of 0.015-0.024, so
  sigma^2 / tau^2 = 36-55 (the heuristic: 10-13). A 12-match event's team result has a
  reliability of **0.14-0.18** (the heuristic 0.41-0.44): replayed with other seeds, the same event
  gives another team result **53-57%** of the time. The margin does better (0.17-0.27) but not
  well. The matches are independent (D's luck variance is 12 x the per-match luck within noise).
  The network is weak (mtg5's iteration 14: 5% against the heuristic): it turns the decks'
  differences into wins far less than the heuristic does (tau^2 2.4-4.5 times smaller), while the
  luck per match is about the same. A stronger play network should move the labels towards the
  heuristic's numbers (an expectation: the heuristic shows what a player that converts deck quality
  better gives).
* **Not the self-play's own randomness**: the same search without temperature or root noise
  (`net-greedy`, 30 events per regime x 2 replays: wide intervals, the per-match luck is the
  usable number) has a per-match luck of 0.74-0.78 against 0.80-0.85: temperature and noise add
  ~8%; the rest is the game (shuffles, the hidden information) and the network's play. Playing
  the drafted matches greedily would not make the labels clean (and would take the play
  network's exploration away).
* **Within an event the players differ far more than the teams do**: omega^2 (0.09-0.14 for the
  network, 0.17-0.25 for the heuristic) is 3-9 times tau^2, and 84-95% of it is the players'
  strengths (additive in a two-way fit), the rest pairing interactions. A whole round of a
  Latin-square rotation meets every player once, so the strengths cancel in the event's total:
  they make its spread smaller than independent matches' would (the team result's model needs
  them), not its mean. The true P(team A wins a match) has quartiles 0.34-0.36 / 0.45-0.49 /
  0.57-0.62 for the network (0.31-0.32 / 0.47-0.51 / 0.63-0.70 for the heuristic).
* **What each label tells** (one replay, squared correlation; network / heuristic): about the
  team's true margin, the margin 0.18-0.30 / 0.50-0.52, the team result 0.15-0.21 / 0.42-0.43, the
  own record 0.04-0.08 / 0.15; about the seat's true own record, the own record 0.20-0.23 /
  0.38-0.46, the team result 0.03-0.08 / 0.14-0.17. The team's match difference is exactly the sum
  of its players' own records (checked on every replay), so the own records split the margin
  between the seats.

**Recommendation: matches per event: keep 12 (3 per player); do not go to 16 or 24.** So the
answer to the question is no: more matches per player make each label clearer, but not by enough
to pay for themselves. The compute
to reach a given precision, (1 + (sigma^2 / tau^2) / m)(d + m) with d = 1.5, relative to 12 matches:

| matches per event | 4 | 8 | 12 | 16 | 24 | 48 |
|---|---:|---:|---:|---:|---:|---:|
| network, random drafters | 1.08 | 0.99 | 1 | 1.03 | 1.11 | 1.41 |
| network, consensus drafters | 1.02 | 0.97 | 1 | 1.05 | 1.18 | 1.61 |
| heuristic, random drafters | 0.79 | 0.87 | 1 | 1.14 | 1.45 | 2.38 |
| heuristic, consensus drafters | 0.83 | 0.89 | 1 | 1.13 | 1.40 | 2.23 |

With today's play network 8 to 16 matches cost the same within 5% (m* = 7-9): the extra matches
of a full round robin (16) buy +23-26% margin reliability (0.18 -> 0.23, 0.25 -> 0.31) for 33% more
matches, 24 buy +60-70% for twice the matches: the same compute spent on more events teaches the
draft network as much or more. As the play network improves towards the heuristic's numbers, m*
falls to ~4 and fewer matches per event (8, then 4) become the better buy, not more. Two things
weigh against cutting below 12 now: the matches are also the play network's training data (in
co-training they are not wasted: the formula charges them fully to the draft), and m* is for the
draft as a whole: a decision whose deck is already fixed (deckbuilding) gains from every extra
match. So: 12 now; revisit 8 when the play network beats the heuristic.

**Recommendation: training targets.** The margin carries more information than the team result
on every count (more reliable, 0.17-0.27 vs 0.14-0.18 at 12 matches; more correlated with the
team's true strength, 0.18-0.30 vs 0.15-0.21): the team result throws away how much a team won by.
Two changes are worth trying, both design decisions for the user (nothing changed here):
1. **Value target from the margin**, not only the win / draw / loss: today the draft network's
   value head learns the team result (blended with the search's root value, value_q 0.5) and the
   margin only through its margin head (margin_loss_w 0.5); a value target that is a smooth
   function of the margin (or a larger weight on the margin head) uses the better label.
2. **Own records for deckbuilding.** A seat's deck plays only its own 3 matches, and the team's
   match difference is exactly the sum of its players' own records: the own record is the margin's
   share that the seat's own deck decides, and it tells 2.4-8.6 times more about the seat's own deck
   than the team result does (0.20-0.23 vs 0.03-0.08 for the network; 2.2-3.3 times for the
   heuristic). For deckbuilding decisions
   (the deck is the seat's alone) the own record is the right credit; picks also shape what the
   opponents get (every pass goes to an opponent), so they keep the team's margin. zero-core would
   need a per-decision-class target (a game hook naming each record's target), the analogue of
   `sims_divisor`.

### 50.2 The fuzz farm (`tools/fuzz_farm.py`, `tools/fuzz_farm.sh`, `tools/fuzz_farm_status.sh`)

The Mac fuzzes the latest main for ever, at nice 19, in the time the remote evaluations leave.

**Where.** `~/mtgz-fuzzfarm/` on the Mac: `bin/` (the farm's code: `fuzz_farm.py` and `mac_backoff.py`
copied there by `fuzz_farm.sh install`, so the farm never runs code it checks out), `src/` (a clone of
the bare repo `~/mtg-zero.git`, checked out at main's head, zero-core from the mirror
`~/mtg-zero-core.git`), the target dir `~/cargo-target/mtg-zero/fuzzfarm` (its own), `state.json`
(the cursors, rates and totals), `status.json`, `failures/`, `logs/` (`farm.log`, `rounds.jsonl`,
each mode's last job output in `jobs/`, the builds'), `config.json` (optional overrides, read every
round), the control files `PAUSE` and `STOP`.

**Main on the Mac.** The farm fetches `refs/heads/main` of `~/mtg-zero.git` at every round. The
desktop puts main there with `git push mac main` (done at 94d43c1, and at 075649c once deckbuilding
as picks was merged: the farm rebuilt and went on at its next round); `bash tools/mac.sh
<anything>` from main's checkout also pushes it and, in addition, zero-core's pinned commit to
the Mac's mirror, which a main that moves its pin needs (the farm's `git submodule update` fails
otherwise: recorded as a build failure, and the farm goes on with its last build).

**A round.** Fetch main; if it moved, check it out and build it (`tools/build_py.sh` for the
search smoke, `cargo test --release -p engine --test fuzz --test draft --no-run` for the fuzz
binaries, whose paths it keeps; a failed build is recorded as a failure, and the farm stays on its
last build); then one chunk of every mode, `jobs` at a time (default 4), each a process group of
its own at nice 19:

| mode | command | seeds | unit |
|---|---|---|---|
| `games` | the fuzz binary's `fuzz_random_games` | `FUZZ_FIRST`, `FUZZ_GAMES` | games |
| `matches` | `fuzz_random_matches` | `FUZZ_FIRST`, `FUZZ_MATCHES` | matches |
| `interactions-games`, `interactions-matches` | the same, `FUZZ_POOL=interactions` | the same | |
| `drafted-matches` | `fuzz_random_matches`, `FUZZ_POOL=drafted` (the drafted mode's games are the default mode's) | the same | matches |
| `exponential-games` | `fuzz_random_games`, `FUZZ_POOL=exponential` (its matches are the default mode's) | the same | games |
| `draft-events` | the draft binary's `fuzz_random_events` | `DRAFT_FUZZ_FIRST`, `DRAFT_FUZZ` | events |
| `smoke-team_draft`, `smoke-team_draft_2v2`, `smoke-match`, `smoke-random` | `python tools/search_smoke.py --specs <spec> --seeds a-b --threads 4` | driver seeds | configurations (400 games or matches, or 24 events and their matches, each) |

**Seeds.** Every mode has a cursor (`state.json`) that only moves forward: the fuzz modes start at
10,000,000 (past every seed run by hand: the fuzz's defaults are 0..n, the replay tests 1000-1030 and
2000-2012), the smokes at 1000 (the gate uses 0-15). A chunk is `[cursor, cursor + n)` with `n` the
mode's measured rate times `chunk_s` (600 s; at least 1,000 games, 100 exponential games, 50 matches
or events, 1 smoke seed); the cursor moves to its end when it is done. How each mode takes an offset:
`fuzz_random_games` and `fuzz_random_matches` read `FUZZ_FIRST` (they always did: each seed's game or
match is built from the seed alone, so a seed is the same game in any chunk); the search smoke's
`--seeds` are driver seeds; the draft fuzz had none: its events were `0..n` on one random stream, so
**`DRAFT_FUZZ_FIRST=f`** (engine/tests/draft.rs, the only test change) plays events `f..f+n`, each on
random streams of its own seeded by its number, and a failing event prints `event e failed (rerun:
DRAFT_FUZZ_FIRST=e DRAFT_FUZZ=1)`; unset, the fuzz is exactly what it was (`DRAFT_FUZZ=20000`:
section 47.7's counts to the action, its `PayMana` match at event 8657 included; the digests are
untouched: no engine file changed). While main has no
`DRAFT_FUZZ_FIRST` (until this branch is merged) the farm skips `draft-events` and says so in its status.

**Failures.** A fuzz chunk stops at its first failing seed; the farm reads the seed from the
test's own rerun line (`seed s failed (rerun: ...)`, `match s failed ...`, `event s failed ...`),
records the failure and queues the rest of the chunk from `s + 1` (at least the mode's minimum, so a
statistical assertion over the chunk keeps its meaning). A crash without unwinding (an abort, a stack
overflow: no rerun line) is bisected: the chunk's halves are rerun until the crashing seed is alone.
A chunk that does not end is killed after 5 times its expected running time (at least an hour, the
stopped time not counted) and bisected the same way, each half killed after 4 times its expected
time plus 2 minutes, down to the seed that hangs (bisection replays seeds: the only time the farm
does). A panic outside a seed (an assertion over the whole chunk) is recorded for the chunk. The
draft fuzz goes on past a heuristic-played match that panics in the `mtg` engine and lists them at
the end (section 47); the farm records each listed match as a failure of its own (its event alone
the reproduction, with the match's spec). A search-smoke configuration that fails or hangs is
recorded with its seed; the smoke's own watchdog is off (`--stall 1e7 --timeout 1e6`: a stopped
process would trip it) and the farm's limit replaces it. Each failure is a file
`failures/<id>.json`, the id the hash of its panic site and the first 120 characters of its
message's first line with the digits masked (so the same bug at other seeds is one failure; a panic
that dumps its state, as `PayMana`'s, differs by board after that): the kind (panic, crash, hang, build),
the site, the message (40 lines), the modes, the count, the first and last time, up to 50 seeds with
their commits, the reproduction (the commit, the environment and the cargo or smoke command) and the
job's output the first three times (`<id>-<n>.log`). Failures matching `expected` (config; default
`PayMana`, `Griselbrand`, `Green Sun's Zenith`: the payment panics branch m6-payfix fixes) are marked
expected and counted like the others; nothing stops the farm.

**Back-off.** The farm's governor (tools/mac_backoff.py) stops every running job (SIGSTOP to its
process group) within 5 s of an evaluation taking `~/mtgz-remote/.eval.lock` (or the other games'
locks, or `PAUSE`), starts no new job meanwhile, and continues them (SIGCONT) when it is gone;
the jobs' running time excludes the stopped time (rates and the hang limit). The builds run under
the same governor.

**Status.** `status.json` (every 10 s): the state (running, backing off and why, building,
paused), main's commit and subject, the round, the farm's hours, the back-offs, the running jobs,
per mode what it fuzzed in total, per hour of its running time and per hour of the farm, its
cursor and last chunk, and every failure. `bash tools/fuzz_farm_status.sh` (desktop) prints it over
ssh and marks the failures first seen since the last check NEW (`--all`: every one; `--json`).

**Lifetime.** A user LaunchAgent `com.mtgzero.fuzzfarm` (`~/Library/LaunchAgents/`, nothing else
there touched): `RunAtLoad`, `KeepAlive` unless it exited cleanly (a crash restarts it after 60 s;
`STOP` ends it for good), `Nice` 19, low-priority IO; it starts at login, so after a reboot it runs
once the user logs in (the Mac has no automatic login: as for its other user agents, the mtg-bot
worker and the planner) and it ends at logout (a LaunchDaemon would survive both but needs root: not
done). A restarted farm kills the job groups its previous
life left (`jobs.json`) and goes on from its cursors (a chunk interrupted midway is replayed from its
start: never counted as done).

**Running since 2026-10-28 02:16** (the Mac's clock; installed from this branch, reinstalled with
the fixes of its first hours): two jobs at a time while the noise study ran, four since 04:29;
main 94d43c1, then 075649c from its next round. Throughput per **running hour of one job** (one
core; a search smoke's four threads), from its first ~2.5 hours with the Mac shared (the noise
study, other agents' cargo tests, td1's first evaluations):

| mode | per running hour |
|---|---:|
| games | 141,000 games |
| matches | 86,000 matches |
| interactions games / matches | 124,000 games / 79,000 matches |
| drafted matches | 70,000 matches |
| exponential games | under ~27,000 (a chunk of 8,374 ran past 1,100 s) |
| draft events (this branch, `once`: 300 events in 3.4 s) | ~320,000 events |
| smoke `team_draft` (4v4) | 530 configurations: 12,700 events and their 152,000 heuristic-played matches |
| smoke `team_draft_2v2` | 790 configurations: 19,000 events, 76,000 matches |
| smoke `random` | 108 configurations: 43,000 games |

A round gives every mode one chunk of ~600 s of running time, so with 4 jobs and 10 modes (the
draft fuzz skipped on main) each mode runs about a third of the time: ~45,000 games, ~27,000
matches per hour of the farm in each of the default and interactions modes, and so on (the status's
"per farm h" column, which also counts the hours at 2 jobs and the back-offs).

**Failures found** (its first hours; every one with its reproduction in `failures/`):

* **Unexpected: the hidden pool's determinization in best-of-three matches** (`engine/tests/fuzz.rs:861`,
  `check_hidden_pool`): in a world determinized for P1, P0's cards hold **Elvish Spirit Guide twice and
  not Jennifer Walters // The Sensational She-Hulk**, while the sampled deck has one of each (the
  world's cards are not the sampled deck). Matches 10009649 (default mode, at 94d43c1) and 10009634
  (`FUZZ_POOL=interactions`, at 075649c); deterministic: `FUZZ_FIRST=10009649 FUZZ_MATCHES=1 cargo
  test --release -p engine --test fuzz fuzz_random_matches -- --exact --nocapture` (reproduced at
  075649c). Not a payment panic; determinize's resampling of the opponent's deck (ENGINE.md 36.11)
  with a double-faced card and a card exiled from the hand as a cost are the suspects. Untouched here.
  (Fixed in section 51.1: the deal ignored a cloud while the opponent paid.)
* **Unexpected: `ChooseGraveyardCard with no options`** (`engine/src/flow.rs:208`, from
  `activate::pay_costs`: an activated ability whose cost takes a graveyard card was offered with
  nothing to take), drafted match 10001565 at 075649c: `FUZZ_POOL=drafted FUZZ_FIRST=10001565
  FUZZ_MATCHES=1 ...`. The castability-versus-payment family m6-payfix works on (its new
  interaction group has graveyard costs), but not a `PayMana` panic: worth checking against its fix.
* **A payment search that does not end** (the games chunk from 10,044,190 at 075649c): one game sat
  for over 50 minutes of running time in `engine::pay::covers::go` (recursive, under
  `can_pay_without` -> `phyrexian_feasible` -> `cands`, during the fuzz's option completion);
  bisected by hand on the Mac to **game 10049522** (12-way splits: every other part of the chunk
  passed in ~28 s per 2,355 games; this seed alone runs for minutes, where a game takes ~12 ms):
  `FUZZ_FIRST=10049522 FUZZ_GAMES=1 FUZZ_LOG=1 cargo test --release -p engine --test fuzz
  fuzz_random_games -- --exact --nocapture` does not return. A search worker meeting such a state
  in a run stalls its group (only the supervisor's stall restart gets out). The farm, which
  bisects hung chunks itself since its last install, will record it at its next pass over the chunk.
* **Unexpected: `determinize changed the encoding of P1 (mover P1) at 29444: 0 -> 1`**
  (`engine/tests/fuzz.rs:999`): a world determinized for the decider of a best-of-three match does not
  encode as the real game for it (an information leak, or a resampled card the decider knows), match
  10036882 with `FUZZ_POOL=interactions` at 075649c: `FUZZ_POOL=interactions FUZZ_FIRST=10036882
  FUZZ_MATCHES=1 cargo test --release -p engine --test fuzz fuzz_random_matches -- --exact --nocapture`.
* **Unexpected: `a card with another face`** (`engine/src/faces.rs`, `other_face(card).expect(..)` in
  `begin_cast_either_face`: a card offered as castable by its other face has none), game 10070547
  with `FUZZ_POOL=interactions` at 075649c: `FUZZ_POOL=interactions FUZZ_FIRST=10070547 FUZZ_GAMES=1
  FUZZ_LOG=1 cargo test --release -p engine --test fuzz fuzz_random_games -- --exact --nocapture`.
* **Expected (`PayMana: no way to pay ...`, `engine/src/pay.rs:1569`, m6-payfix)**: `{3}{U}{U}` (games
  10013181), `{5}{U}` (interactions games 10019015 and matches 10018076), `{1}{B}` (the `team_draft`
  search smoke, seed 1000: a co-training run's own path). The noise study's heuristic-played drafted
  matches hit `{1}{R}`, `{5}{U}`, `{7}{U}` and `{6}{U}{U}` (5 matches, specs in
  `~/mtgz-noise/failed_specs.jsonl`), and section 47's draft fuzz Griselbrand's `{4}{B}{B}{B}{B}`.

Section 51 settles every one of these: the hidden pool's deal, the opponent's Lotus Petal, the
payment search and the surveil's shuffle were live on ab02574 and are fixed there; "a card with
another face", `{5}{U}` and `{1}{B}` were fixed by ab02574 (section 49); `{3}{U}{U}` was a new case
(delve before the mana abilities), fixed there too.

**Checking it from the desktop**: `bash tools/fuzz_farm_status.sh` (the state, per mode the totals
and rates, every failure with its reproduction, NEW since the last check; the noise study's progress
line), `--json`, `--noise` (the study's summary). Control: `bash tools/mac.sh 'bash tools/fuzz_farm.sh
pause|resume|stop|start|install|uninstall'`; jobs at a time: `~/mtgz-fuzzfarm/config.json`
(`{"jobs": 4, "chunk_s": 600}` now; read every round).

### 50.3 The Mac as a self-play worker (the design; built in section 52, with its deviations in 52.2)

**The measurement** (`python tools/match_noise.py bench --model <mtg5 iter 14> --seconds 150 ...`:
mtg-4's self-play, generated best-of-three pools, the 128 x 4 token network with CPU inference, a
rolling driver recording its samples, leaves per second over the phase; 150 s each from a cold start,
the Mac shared with the noise study's heuristic phase or other agents' cargo tests, as noted):

| matches in flight | search threads | groups (batch) | inference workers x torch threads | leaves/s |
|---:|---:|---:|---:|---:|
| 128 | 12 | 2 (64) | 3 x 4 (the remote evaluation's settings) | 7,942 * |
| 256 | 12 | 2 (128) | 3 x 4 | 10,669 * |
| 256 | 14 | 2 (128) | 2 x 6 | 10,809 * |
| 256 | 12 | 4 (64) | 4 x 3 | 12,956 * |
| 512 | 12 | 2 (256) | 3 x 4 | 13,158 * |
| 384 | 12 | 3 (128) | 3 x 4 | 14,804 * / 10,931 ** |
| 512 | 12 | 4 (128) | 4 x 3 | 17,474 |
| 768 | 12 | 6 (128) | 6 x 2 | 20,262 |
| **768** | **12** | **4 (192)** | **4 x 3** | **20,748** |
| 768 | 16 | 6 (128) | 6 x 2 | 18,737 ** |
| 1,024 | 12 | 8 (128) | 8 x 2 | 13,906 ** |

(* nice 19 with m6-payfix's cargo tests running at normal priority; ** the Mac's load at 12-17 from
other jobs; the rest at normal priority, load 4-11.) The CPU is far from saturated (4-6 cores busy by
the process's CPU time): the driver is bound by batches in flight, so more groups with an inference
worker each scale until ~6 batches.

**Capacity** (`~/mtgz-noise/capbench.sh`: 1, 2 and 3 bench processes at once, each the best
setting above, 240 s, at nice 19 with the farm paused; load 4-11): **one process
19,390 leaves/s; two 33,036 (16,499 + 16,537); three 37,067 (3 x ~12,350)**. One driver process is
bound by its own loop (the Python thread that moves the batches), not by the cores: a second
process nearly doubles the total, a third adds 12%. Sustained over hours (the noise study's ~2 h
of network replays at nice 19, sharing the Mac with the fuzz farm, other agents' cargo tests and,
for its last hour, a second study process): 14,000-16,700 leaves/s per process.

**Against the desktop**: runs/mtg5's self-play (the same network size and search, 768 matches in
flight, CUDA inference, the GPU shared with other projects) measured 6,268-11,385 leaves/s over
iterations 1-15 (7,877 at iteration 14), 943-2,790 matches per hour (13,300-24,800 leaves per
match). **The Mac self-plays several times faster than the desktop**: at 19,000-24,300 leaves per
match (the noise study's 18,975 per drafted match over all 5,760; runs/mtg5's iteration 14
self-play 24,305), one process plays **2,900-3,700 matches per hour** and two **4,900-6,300**,
against the desktop's 943-2,790 (1,167 at iteration 14): 33,000-37,000 leaves/s against
6,300-11,400, **3 to 6 times the desktop's self-play** (4-5 times its rate at iteration 14). Its
evaluations (section 44) already showed the shape (inference in small batches: CPU, not MPS).

**Single-game runs: remote self-play shards.** A run setting `selfplay_remote` naming a game
package remote (the `remotes` of `eval_remote`: host, the run commit's checkout, python, workdir,
the Mac's overrides) gives the run a second producer of iteration `i`'s samples:

1. After training writes `models/iter_{i-1}.pt`, a `RemoteSelfPlay` thread of the run (zero.remote's
   transport) ships it (6 MB) to the mirror `<workdir>/<run>.selfplay/` and starts `python -m
   mtgz.az.remote selfplay <mirror> <i>` over ssh: `Run.selfplay` with the remote's overrides (CPU
   inference, the measured concurrency, groups and workers), a rolling driver with no quota of its
   own, shards written **compressed** (`np.savez_compressed`) as `data/iter_i/m00_*.npz` in the
   mirror, through the same `ShardWriter` and its non-finite check.
2. The desktop self-plays its quota as now. When its quota is done it tells the remote to stop
   (a file in the mirror, or closing the session); the remote ends its phase (`end_phase`: the
   matches in flight are dropped, as zero-core drops a phase's unfinished games), closes its shards
   and prints one result line (matches, samples, leaves, seconds). The thread copies the shards into
   the run's `data/iter_i/` beside `r00_*` before training starts, with a deadline (say 60 s after
   the stop): shards not back by then are dropped and logged, so a slow, busy or offline Mac never
   delays an iteration.
3. Training reads the union (`window_shards` lists every shard of an iteration directory; shards
   are self-describing npz, compressed or not); the steps follow the new samples (`reuse`), and
   `metrics.jsonl` gets `selfplay.remote` = {matches, samples, leaves, leaves_per_s, seconds,
   dropped}. Every sample already carries `net` (its checkpoint): `i - 1` on both machines, so the
   data is on-policy.

The quota stays the desktop's: the Mac adds a time-boxed share (whatever it finishes while the
desktop plays its quota), so an iteration never waits for it, and an iteration with the Mac busy or
asleep is exactly today's.

**Co-training (team_draft-4v4): the Mac plays a share of the queued resolver matches.** The
coordinator (zero/cotrain.py) turns each parked event into 12 queued `mtg` specs and takes their
result rows back by tag, from any driver; a remote match worker is a second consumer of its queue:

1. `resolver_cfg['selfplay_remote']`: a `RemoteMatches` thread keeps one ssh session to `python -m
   mtgz.az.remote matches <mirror>` (the play network's checkpoint i-1 shipped at each iteration's
   start), feeding it specs as JSON lines - at most `remote_inflight` (~768, the Mac's best
   concurrency) outstanding - taken from `Coordinator.to_queue` alongside the desktop's resolver
   driver; the remote side is a queue_only rolling driver (the measured settings) that answers each
   finished match with its result row (by tag) and writes its samples to compressed `m00` shards.
2. The rows come back on the session's stdout and go into the coordinator as the local driver's do
   (an event resolves when its 12 rows are in, from either machine); the shards come back at the
   iteration's end with the deadline of the single-game design.
3. A match the Mac has not finished when the phase ends, or when the session breaks, is requeued
   locally (the coordinator knows its tag): never lost, at worst played twice.

**Costs.** zero-core: the remote `selfplay` and `matches` commands (`Run.selfplay` and the
resolver's driver reused; ~250 lines over zero.remote's transport, mirror, checks), the
`RemoteSelfPlay` / `RemoteMatches` threads with the stop, the deadline and the requeue (~300 lines:
the co-training queue consumer is the delicate part), compressed shards in `ShardWriter` (a flag),
metrics and the dashboard's columns; mtgz: the remote's self-play overrides. Tests: a `local`
remote end to end (as `eval_remote`'s), a remote killed mid-phase, a missed deadline, a broken
session's requeue. About two days with the core's smoke. **Bandwidth**: a sample is 67 KB (33,652
features as float16; measured 1.0% non-zero) and a match ~140 samples, ~9.5 MB; the Mac's measured
rate to the desktop is only **7.6 MB/s** (scp over the tailnet's direct LAN path), so uncompressed
shards at the Mac's rate (35-60 GB per hour of its self-play) would saturate it; zlib level 1
compresses them **58x** at 854 MB/s (measured on a runs/mtg5 shard), ~165 KB per match: shipping
is then negligible.

**Risks.**
* *Staleness*: none with checkpoint i-1 shipped at the iteration's start (the Mac starts a few seconds
  after the desktop). The simpler alternative - the Mac always one checkpoint behind, never waiting -
  is off-policy by one iteration (the window already mixes 6-8 iterations); `net` in the samples
  makes either visible.
* *Determinism*: self-play is not reproducible today (rolling drivers, threads); the Mac's share
  depends on timing and its arithmetic differs from the desktop's at float32 rounding (44.4), so a
  run's data depends on the Mac's load. Evaluations stay comparable (fixed pools, the same players).
* *The lock with remote evaluation*: evaluations first. Remote self-play backs off (stops) while
  `~/mtgz-remote/.eval.lock` is held (tools/mac_backoff.py's rule): the play network's evaluation
  every 2nd iteration takes ~9 minutes there (44.4), so the Mac's share shrinks by that much on those
  iterations. The fuzz farm and studies must yield to remote self-play too (it would take a lock of
  its own that `mac_backoff` honours), or both lose.
* *Version and venv skew*: the remote checkout must be the run's commit (`RunCommitRemote`'s check,
  reused as is); a torch upgrade in the shared venv changes rounding only.
* *Data integrity*: the shards pass the non-finite guard on write (Mac) and on load (desktop); a
  shard is copied under a temporary name and renamed, so a torn copy is a missing shard.
* *Training*: more samples per iteration mean proportionally more steps at the same `reuse` (the
  desktop's training, 2-3 minutes today, would grow with the samples) or a smaller `reuse`.

**Expected gain.** Self-play is most of an iteration on the desktop; the Mac's 2-3 driver
processes would add 3-5 times the desktop's leaves per second (less ~11% for its own evaluations,
48.4). **Single-game runs** (runs/mtg5: 600 matches per iteration in 13-38 minutes of self-play,
training 2-3 minutes, the evaluations already on the Mac): a desktop quota of ~150-200 matches with
the Mac's share on top, or the same 600 matches in ~4-10 minutes: iterations ~2-3 times faster.
**Co-training** (team_draft-4v4, 48.4: ~42 minutes per iteration on average, self-play 16-34 of
them, the draft network's training and evaluation the rest): the Mac playing ~3/4 of the queued
matches would bring self-play to ~5-12 minutes and the iteration to ~20-30 (~1.5-2 times the
iterations per day; the draft network's evaluation, now the largest desktop phase after self-play,
then matters most, 48.6). Worth building after the 4v4 run shows that its draft network learns
(45.12): it does not change what a run computes, only how fast. A cheaper first step: the
desktop's own self-play may be CPU-bound too (8 search threads, batches in flight), which a
measurement like 50.3's on the desktop would show.

### 50.4 Checks

* The draft fuzz, rebuilt on 075649c (Mac): `DRAFT_FUZZ=20000` (the default mode) is **identical** to
  section 47.7's run to the count (20,000 events, 9,871,224 actions, 5,040,000 searched picks,
  4,087,506 searched deckbuilding decisions, 497,122 worlds, 37,488 options completed, team A 8,080 /
  8,188 / 3,732, the 30,800 and 89,200 random decks' averages) and fails as it does, at the `PayMana`
  panic of event 8657's match 0; on 94d43c1 likewise identical to 45.9's. `DRAFT_FUZZ_FIRST=10000000
  DRAFT_FUZZ=2000`: no failure, 988,134 actions, 14 s. `cargo test --release -p engine --test draft`:
  23 passed, 1 ignored (`measure`).
* Behaviour digests (Mac, 94d43c1 + this branch's first commit): combined **12642107250032a7** (no
  engine file changed).
* `python -m pytest -q tests` (Mac): 34 passed, 1 skipped (CUDA), as main.
* The farm, through its own job path (`fuzz_farm.py once` for every mode, at 94d43c1; then
  `draft-events` against this branch in a throwaway farm directory and target dir, removed after):
  every mode ran and parsed; an injected failing mode (`FUZZ_POOL=bogus`: a panic in every seed) was
  recorded once with its site, message, seeds and reproduction and its chunk continued from the next
  seed (restarts capped); the parsers on synthetic output (panic lines with and without a thread id,
  rerun lines, the search smoke's lines, the draft fuzz's listed match panics). Live: its back-off
  (above; and for td1's first real evaluation of the play network at 04:10, 165-175 s), its builds
  of 94d43c1 and 075649c (122 and 190 s), its restarts (`install` five times: no orphaned job), the
  failures of 50.2.
* The analysis on synthetic data (50.1); its team-result model against the measured 12-match
  reliabilities (0.38 / 0.41 against 0.41 / 0.44 for the heuristic; 0.19 / 0.14 against 0.18 / 0.14
  for the network).

### 50.5 Limits and open issues

* **The noise study's players**: one play network (mtg5's iteration 14, weak: 5% of its matches
  against the heuristic, 44.4's evaluation) with self-play search, and the heuristic. A stronger
  network may convert deck quality into wins more reliably (less luck per match) or less (it
  exploits more, and its opponent too); repeat the study with the co-training run's play network
  at some iteration (`tools/match_noise.py play --player net --model <checkpoint>` on the same events,
  after moving the old results aside: ~4 hours on the Mac). The drafters are bots (random and
  `consensus`); a draft network's events will sit between them or beyond `consensus`.
* **The projections beyond 12 matches are models**: the margin's assumes the extra matches' luck is
  independent (checked: D's luck variance is 12 x the per-match luck within noise) and their pairings
  balanced (whole rounds of rotations); the team result's assumes normal player strengths and
  interactions with the measured variances (it reproduces the 12-match reliability within its
  interval). m* uses one d for the whole draft; for deckbuilding decisions, whose deck is fixed, the
  event-level variation is no longer noise and more matches always help.
* **Engine panics in drafted matches**: the payment panics (m6-payfix) fail zero-core's driver; the
  study leaves those match replays out (5 of 23,040 heuristic, 0 of 5,760 network). A co-training
  run hitting one stops (panics fail the run, zero-core 681ba8d), so the fix matters for the 4v4 run:
  the five heuristic-played failures reproduce deterministically (`tools/match_noise.py repro --tag
  <tag> --player heuristic`; their specs in `~/mtgz-noise/failed_specs.jsonl`).
* **The fuzz farm** runs at nice 19 beside everything else on the Mac (the other agents' `mac.sh`
  tests at normal priority take the cores first); its throughput varies with them. It fuzzes main
  only: branches are fuzzed by hand (`mac.sh`). `draft-events` waits for this branch on main. It ends
  at logout and starts at login (no LaunchDaemon). Its search smokes use `--threads 4`; their
  per-configuration counts (400 games, 24 events) are the gate's.
* **Remote self-play** (50.3) was a design here; section 52 built it. The Mac's throughput was measured
  from cold starts of a few minutes and one long steady phase; a run's Mac share also depends on the
  other projects' remote evaluations there (`~/kbz-remote`).
* On the Mac: `~/mtgz-noise/` (the study's events, results, summary; ~60 MB), the pinned checkout
  `~/mtg-zero-wt/noise-5843007ef161` and its target dir, `~/mtgz-fuzzfarm/` and
  `~/cargo-target/mtg-zero/fuzzfarm` (~3 GB), the LaunchAgent plist; `~/mtgz-remote` untouched.

## 51. The fuzz farm's first bugs (M6)

Branch `m6-farmfix` (from 692a52c: main ab02574 and branch m6-macuse, the farm's tools and
`DRAFT_FUZZ_FIRST`; zero-core af44741, unchanged). The fuzz farm (section 50.2) found six kinds of
failure in its first hours, on main 94d43c1 and 075649c. Re-run on ab02574, the engine of the live
4v4 co-training run td1: three reproduced at their seeds, a fourth (a `PayMana` panic) as well, and
two passed only because ab02574's fourteenth interaction group deals other decks for the same
seeds. Each is below with its cards, its root cause, the fix and its rule, and a test that builds
the state on real cube cards (`tests/rules_farm.rs`; every test fails on 075649c, and on ab02574
unless said otherwise). One commit per fix:

| | farm failure (reproduction) | cards | live at ab02574 | fix |
|---|---|---|---|---|
| 51.1 | the world's cards are not the sampled deck (match 10009649; on ab02574 also drafted match 10043680) and a cloud's card out of its domain (interactions match 10039110 on ab02574) | Elvish Spirit Guide twice, no Jennifer Walters // The Sensational She-Hulk; Force of Vigor, Fury | yes (silent in a run) | 1759983 |
| 51.2 | `ChooseGraveyardCard with no options` (drafted match 10001565) | Crabomination, Lotus Petal, Psychic Frog | yes (a panic) | 2f2b607 |
| 51.3 | a payment search that never ends (game 10049522) | Gaea's Cradle, Nissa, Who Shakes the World, Aang | yes (a hang) | 819f44e |
| 51.4 | determinize changes the decider's encoding (interactions match 10036882) | Kaito, Bane of Nightmares, Blightsteel Colossus | yes (silent) | 83e12e1 |
| 51.5 | "a card with another face" (interactions game 10070547) | Shelldock Isle, Life // Death | no: fixed by 49.6 | - |
| 51.6 | `PayMana: no way to pay {3}{U}{U}` (game 10013181) | Dig Through Time, Fanatic of Rhonas, Dragon's Rage Channeler | yes (a panic) | df83060 |
| 51.6 | `{5}{U}`, `{1}{B}` | Wizard's Rockets with improvise, with Urza | no: fixed by 49.2 | - |

"Live" is measured, not inferred: each test of `tests/rules_farm.rs` was run on 692a52c (ab02574's
engine), and the farm's interactions seeds were dealt their 075649c decks again on it with
`FUZZ_GROUPS=13` (51.7).

### 51.1 The hidden pool's deal against a cloud

**The failure.** Match 10009649 (default mode; also 10048520, `FUZZ_POOL=interactions` 10009634 and
drafted 10036613): in a world `determinize` sampled for P1, P0's cards held Elvish Spirit Guide twice
and not Jennifer Walters // The Sensational She-Hulk, though the sampled deck had one of each. In a run
nothing checks this: the search simply plays worlds that are not the deck it sampled. While this
branch was being checked the farm, now on ab02574, recorded two more of the same fault: drafted match
10043680 (P1 cast Force of Vigor free from Urza's exile; in P0's world P1 held Green Sun's Zenith twice
and one Island too few) and interactions match 10039110 (P1 paid for Fury; P0's cloud over P1's hand
and one library slot lists Horizon Canopy and Watery Grave, and in P0's world the Grave was outside
that domain: `check_knowledge` failed).

**Root cause.** A best-of-three world is made in two deals (sections 36.11, 10). The hidden pool's
(`MtgAz::sample_hidden_pool`) samples the opponent's deck and gives its cards to the slots the
viewer does not know (`Game::redeal`): in order, the hand's slots first, ignoring the clouds (what
the viewer knows is somewhere in a zone: here, after a shuffle, that the Guide was in P0's library).
`Game::determinize` follows and deals the slots again so each cloud's cards land in its domain, but
it **pins** the hand mana sources of a player who is paying (their payment may count on them,
section 10), and it computes the pins from the world's identities. P0 was paying for Shifting
Woodland's ability; the first deal had put the Guide in a hand slot (the lowest card definition,
the first slot); the second pinned it there, so the cloud's Guide was missing from what it shared
out, was dealt again from the cloud's list anyway, and one card of the deck (Jennifer Walters) was
never dealt. The `debug_assert` that would have caught the missing card is off in release. The
other pins do the same: the hand cards a cost of the spell being cast may exile (Force of Vigor's
"exile a green card from your hand": the cloud's Green Sun's Zenith dealt to P1's hand and pinned;
Fury's evoke, a red card: Chandra, Torch of Defiance pinned in the hand, which left the cloud over
the hand and a library slot room for one of its two lands, `cloud_needs` truncating the other).

**Fix.** The hidden pool's deal holds the clouds itself: `Game::redeal(viewer, owner, slots, pinned,
...)` deals the sampled cards with `resample_with_clouds` (each cloud's cards among the slots of its
domain, the payment's real pins kept out), so whatever `determinize` pins afterwards, every cloud's
card is still in its domain's pool. `resample_with_clouds` now asserts that each cloud's card is
among the cards it deals (a panic, never a world with a card twice). Hidden information, ENGINE.md
36.11 and 10: a sampled world is the sampled deck exactly and agrees with what the viewer knows.

**Why the fix holds for every pin.** With each cloud's cards in its domain before
`determinize`, a pinned slot of a domain holds either one of the cloud's cards (`cloud_needs` takes
it off the cloud's list) or another card (one slot less of room); so a cloud's list never exceeds its
unpinned slots, and every card on it is among the unpinned cards dealt.

**Tests.** Two cases of one helper, a best-of-three match built from a scenario
(`MatchState::new` with the game's 40 + 40 cards): a card in P0's graveyard, P0 discards Emrakul,
the Aeons Torn to Psychic Frog, Emrakul's trigger shuffles the graveyard into the library (a cloud of
P1's over P0's library), P0 casts a spell and pays; every card of P0's that P1 does not know sorts
after the graveyard card. `hidden_pool_deal_keeps_a_clouds_card_in_its_domain_while_its_owner_pays`
(Elvish Spirit Guide; Lightning Bolt) and
`hidden_pool_deal_keeps_a_clouds_card_in_its_domain_while_an_evoke_cost_pins_the_hand` (Chain
Lightning; Fury's mana cost paid, its evoke's red card pinned). In 64 worlds sampled for P1: P0's
cards are the sampled deck, the card once and in the library, knowledge and invariants hold. On
ab02574 both fail at world 0 (the card twice). The farm's four seeds of 075649c and two of ab02574
pass on the branch; the two of ab02574 fail on 692a52c and pass from 1759983, this fix alone.

### 51.2 Another player's Lotus Petal is not in your graveyard

**The failure.** Drafted match 10001565 (and default match 10040102): `ChooseGraveyardCard with no
options`, from `activate::pay_costs`. P1's Crabomination exiled P0's Lotus Petal from P0's graveyard
and P1 cast it; P1 then activated Psychic Frog's "exile three cards from your graveyard" with two
cards in its graveyard and the Petal: the Petal paid nothing towards the cost, the exile found two
cards for three.

**Root cause and rule.** CR 400.3: a card that would go to a graveyard other than its owner's goes to
its owner's. The Petal is P0's: sacrificed for P1's mana it is a card in P0's graveyard, not P1's.
Castability (`cost_payable`: `exile_short`'s shortfall met by `delve_sources`, the sources activated
first so they are in the graveyard, section 49.2) and delve (`delve_sources`, `delve_classes`)
counted every self-sacrificing mana source P1 controlled; `feeds_graveyard` (sacrifices for such a
cost) asked only whether the card reaches *a* graveyard.

**Fix.** `feeds_graveyard(p, c)` is true only for `p`'s own nontoken card that reaches the
graveyard; `delve_sources(p)` and `delve_classes(p, ..)` use it. Test
`cr_400_3_an_opponents_lotus_petal_does_not_feed_your_graveyard_costs`: P0 takes P1's Petal with
Dack Fayden's -2; Psychic Frog's ability is not offered with two graveyard cards (with P0's own Petal
it is, and every payment path completes); Treasure Cruise is not offered with two Islands, the
Petal and four graveyard cards (seven; with P0's own Petal delved, eight: offered, every path
completes). On ab02574 the Frog's ability is offered.

### 51.3 A payment search that never ended, and a bound on the searches

**The failure.** Game 10049522 sat for over 50 minutes in `pay::covers::go` (section 50.2). A
self-play worker meeting such a board hangs its run until the stall watchdog (2 hours for td1).

**Root cause.** `cands` folds a card's ways to pay into one when one way makes every other's mana
(`covers`: can the units of way A make the units of way B, each by its own unit?). It assumed ways of
at most four units and kept the units already used in eight bits (`used: u8`, `1 << i`). P1's Gaea's
Cradle, animated by Nissa, Who Shakes the World's +1 (a creature), had two ways: its own "{T}: add
{G} for each creature you control" (28 {G}) and a tap for Aang's waterbend (creatures and artifacts
pay {1} each, like improvise). `covers` compares the 28-unit way with itself: past the eighth unit
every bit is taken (in release a shift past 7 wraps), the search fails at depth 8 and backtracks
through the permutations of 28 units, about 10^9 nodes of 28 steps each, per call, many calls per
priority decision. The answer was not wrong (no fold), only never reached.

**Fix.** `covers` is a bipartite matching by augmenting paths (Kuhn), polynomial for any number of
units (at most 255 per way). And every exponential feasibility search of the payment is bounded: a
node budget `SEARCH_NODES` = 16,777,216 per search for `ways_payable` (the cards with several ways),
`payable_within_n` (the budgeted units) and `most_spent_from` (the virtual pool's spends); past it the
search panics with its inputs printed (cost, needs, chosen units, fixed units, every card's ways,
budget: that search's reproduction, run alone), never an answer it did not finish. A panic fails a
self-play run at once (zero-core 681ba8d) instead of stalling it for two hours. The most nodes one
search visited, measured with an instrumented build on the Mac (5,000 default and 5,000
interactions games, 500 default, interactions and drafted matches, 500 exponential games):
`ways_payable` 20,103 (a default match), `most_spent_from` 432, `payable_within_n` 92: the bound is
834 times the largest. Rules: CR 601.2g-h and 605.3a (mana abilities activated while paying,
each source once), nothing changes in what is payable.

**Tests.** `cr_605_3a_gaeas_cradle_with_many_creatures_tapped_for_waterbend_is_checked_at_once`:
Gaea's Cradle, Nissa, Aang and 26 Llanowar Elves; Nissa's +1 animates the Cradle (28 creatures);
Aang's waterbend {8} is offered and every payment path completes, all within 30 s on a thread
against a clock (milliseconds on the branch; on ab02574 and 075649c it times out). Unit tests in
`pay.rs`: `covers` on 255 units, an augmenting path, a colour none of the units makes; a search of
21 cards with two ways each against {W} (3^21 paths) panics with "a feasibility search visited".

**Speed.** The old `covers` was also slow below a hang: random play is 13-15% faster on the branch
(51.8), all of it from `covers` (the branch with the old `covers` put back: 7.28-7.32 us per decision,
as 692a52c).

### 51.4 A surveil's replacement shuffle

**The failure.** `FUZZ_POOL=interactions` match 10036882 at 075649c: `determinize changed the
encoding of P1 (mover P1) at 29444`, the first float of the decision's card list (`O_LIST`). P1 had
surveiled 2 with Kaito, Bane of Nightmares' 0 (Blightsteel Colossus and Screaming Nemesis on top),
put the Colossus into the graveyard first and was asked about the Nemesis; in a world sampled for
P1 the Nemesis' slot held another card.

**Root cause and rule.** CR 701.25a: to surveil is to put any number of the looked-at cards into the
graveyard and the rest on top, one action; the Colossus' replacement ("if it would be put into a
graveyard from anywhere, reveal it and shuffle it into its owner's library instead", CR 614.1a)
modifies that action. The engine asks the cards one by one and moved the Colossus at once: its
shuffle shuffled the library with the Nemesis still in it, still to be placed. The shuffle made P1
forget the Nemesis' slot (an owner's knowledge of its library is exact or nothing), so P1's own
world dealt it again, and the decision, which names the slot, showed P1 a different card.

**Fix.** While a looked-at pile's cards are put (`Op::PileTo`), a replacement's shuffle of their
library waits (`Game::pile_shuffles`) until the pile is placed: the pile empty, or the next op not a
`PileTo` (`Game::flush_pile_shuffles`, after each op of `run_frame`). The final state is the one
action's: the cards to the graveyard, the rest on top, the Colossus shuffled in with them. Test
`cr_701_25a_a_surveiled_blightsteel_colossus_shuffles_after_the_pile_is_placed`: after the Colossus,
P0 still knows the Nemesis; in 32 worlds sampled for P0 it keeps its identity and P0's encoding is the
game's; then with the Nemesis on top (or into the graveyard) the library is shuffled after, the
Colossus in it. On ab02574: P0 no longer knows the Nemesis. The same seed with its 075649c decks
(`FUZZ_GROUPS=13`) fails on ab02574 and passes on the branch.

### 51.5 A card with another face: fixed on ab02574

`FUZZ_POOL=interactions` game 10070547 at 075649c: `a card with another face` (`faces.rs`,
`choose_face`): P1 played its Shelldock Isle hideaway card, Life // Death (a split card, CR 709.3),
both halves castable, so the half was asked first; in a world sampled for P0 the face-down card was
dealt again, a card without halves. It is section 49.6's fault (there with Brazen Borrower, an
Adventure card): ab02574 reveals a card cast during a resolution with a choice of faces before the
choice (CR 712.11b, 601.2a: public either way). The farm's seed passes on ab02574 with its 075649c
decks (`FUZZ_GROUPS=13`). Test `cr_709_3_a_split_card_played_from_shelldock_isle_is_revealed_before_its_half_is_chosen`
(the farm's case: a split card): passes on ab02574, fails on 075649c with the farm's panic.

### 51.6 The `PayMana` panics; delve after the mana abilities

The farm recorded three `PayMana: no way to pay` panics before ab02574 existed:

* **`{5}{U}`** (interactions game 10019015, match 10018076 at 075649c): Wizard's Rockets and an
  improvise spell, the Rockets counted once tapped for improvise and once converted: section 49.2's
  first fault. Both seeds pass on ab02574 with their 075649c decks (`FUZZ_GROUPS=13`).
* **`{1}{B}`** (`search_smoke --specs team_draft --seeds 1000`, game 199 of driver seed 1001 at
  075649c): the Rockets with Urza's tapped artifacts, 49.1's fault. Seed 1000 passes on ab02574
  (24 events, 288 matches).
* **`{3}{U}{U}`** (game 10013181 at 94d43c1, the default mode, same decks on ab02574): **still a panic
  on ab02574**, a new castability-versus-payment case. P1 cast Dig Through Time ({6}{U}{U}, delve)
  with Underground Mortuary, Botanical Sanctum, Talisman of Creativity, Fanatic of Rhonas ("{T}: add
  {G}{G}{G}{G}; activate only if you control a creature with power 4 or greater") and Dragon's Rage
  Channeler, a 4/4 with delirium (four card types in the graveyard, a creature card, Mawloc, among
  them). Castability was right: the Fanatic's {G}{G}{G}{G} and up to six delved cards. The payment
  exiled delve's cards first, as each was chosen: with Mawloc exiled delirium was gone, the Channeler
  a 2/2, the Fanatic's ability off, and {3}{U}{U} had four mana.

**Rule and fix.** CR 601.2g-h, 702.66a-b: delve isn't an alternative or additional cost; its
exiled cards pay for generic mana as the total cost is paid (601.2h), after the mana abilities are
activated (601.2g). So the Fanatic is tapped with the graveyard whole. The engine now chooses
delve's cards in the delve decisions (each `Mark::DelveExile`, kept in place; the next picks go up
the graveyard above them) and exiles them as the payment ends (`exile_delved`, before the
self-sacrificing sources delve exiles after their mana, `PayMana::delve_late`, as before;
a cost with no mana left exiles them at once). `exile_short` (a graveyard cost after the mana,
escape's) leaves out the cards a delve exiles first. Castability (section 49) already counted
this way. Test `cr_601_2g_mana_abilities_are_activated_before_delve_exiles_the_graveyard`: the
Fanatic, the Channeler (a +1/+1 counter, delirium from Llanowar Elves, Island, Lightning Bolt and
Gitaxian Probe), Botanical Sanctum, Talisman of Creativity: Dig Through Time is offered and every
path completes (on ab02574 the first path, the Elves delved first, panics: `no way to pay
{5}{U}{U}`); on that path the Elves and one card are exiled after the Fanatic is tapped.

The noise study's five heuristic-played panics (section 50.5: `{1}{R}`, `{5}{U}`, `{7}{U}`,
`{6}{U}{U}`) were replayed on the branch (51.8).

### 51.7 The fuzz harness

* `FUZZ_GROUPS=n` (`tests/fuzz.rs`): `FUZZ_POOL=interactions` deals from the first `n` groups only.
  The farm records a failure with its commit's seeds; when a group is added, the same seed deals
  other decks. `FUZZ_GROUPS=13` deals 075649c's decks on ab02574 and later: how 51.4-51.6's
  interactions seeds were checked against ab02574 (they are the same games up to the failure:
  the same panics on 692a52c where the bug is live).
* A single match's rerun (`FUZZ_MATCHES=1`, the farm's reproductions) failed its chunk's statistical
  assertion (a sideboarding, a changed deck and a match of three among the matches) unless the match
  went to three games; it now applies from 20 matches, as the hidden-pool one did.

### 51.8 Checks

At 8e1211f, the branch's last engine code (f7f61be adds a test; the documentation commit changes none):

* **Every engine test binary** (the Mac, `cargo test --release -p engine --no-fail-fast`, `az`
  included; the library, 101 test binaries and the doctests): 1,401 passed, 0 failed, 13 ignored
  (section 49's 1,393, the six tests of `tests/rules_farm.rs` then and `pay.rs`'s two unit tests);
  `tests/rules_farm.rs` at f7f61be (its seventh test): 7 passed on the desktop and the Mac. The
  desktop (td1 live: not the whole suite): the library's `pay` unit tests, `rules_farm`,
  `rules_castpay`, the digests and the farm's seeds.
* **Behaviour digests**: re-recorded in 83e12e1 (r 476, 51.4) and df83060 (158 games, 51.6): combined
  1ed7e08dd5078f55 (main) -> 4fb5cc3ca4e7dc22 -> **804002969bdd0938**, on the desktop and the Mac.
  Attributed by building the branch without each fix: without 51.6's and 51.4's the digests are
  main's exactly (51.1-51.3 change no recorded game); without 51.6's alone only r 476 differs. Every
  one of the 159 changed games plays the same cards to the same end on both sides (the chosen
  options by card, the outcome, the turn, the decision count): 158 cast a delve spell for two or
  more cards (the later picks' action ids are places in a graveyard that keeps the chosen cards,
  and the graveyard in the encodings until the payment ends), r 476 is Tamiyo, Collector of Tales'
  reveal and Kaito's surveil with Blightsteel Colossus (the library shuffled after the pile: its
  order, and so the later hashed hidden zones, differ).
* **Fuzz** (the Mac, the farm paused, the five runs at once at nice 10): the default mode
  `FUZZ_GAMES=20000 FUZZ_MATCHES=2000`: 20,000 games (wins 9,993 / 9,970, 37 draws), 5,651,735
  options completed, 2,000 matches (5,138 games, 1,138 of three), 42,768 hidden-pool worlds,
  6,033,684 encodings checked, no failure (228 s). `FUZZ_POOL=interactions` (fourteen groups): 20,000
  games (9,888 / 10,063, 49 draws), 5,551,982 options, 2,000 matches (5,116 games), 45,986 worlds,
  6,139,919 encodings, no failure (266 s). `FUZZ_POOL=drafted`: 2,000 drafted matches (4,983 games),
  48,996 worlds, no failure (43 s). `FUZZ_POOL=exponential FUZZ_GAMES=5000`: 5,000 games (2,117 /
  2,122, 761 draws), 3,862,285 options, largest power 2,147,483,647, counters 4,294,967,295, life
  2,147,483,647, no failure (977 s). `DRAFT_FUZZ=20000`: 20,000 events, 9,871,224 actions, 5,040,000
  searched picks, 4,087,506 searched deckbuilding decisions, 497,122 worlds, 37,488 options completed,
  1,866 heuristic-played matches (none panicked), team A 8,080 / 8,188 / 3,732, no failure (125 s):
  section 50.4's counts exactly, without event 8657's `PayMana` (fixed by ab02574).
* **The farm's failing seeds**, all passing on the branch (the Mac): matches 10009649 and 10048520,
  interactions match 10009634 (`FUZZ_GROUPS=13`), drafted 10036613 (51.1); drafted 10001565, match
  10040102 (51.2); game 10049522 in 0.19 s, and the 1,000 games around it from 10,049,000 in 13 s
  (51.3); interactions match 10036882 and game 10070547 (`FUZZ_GROUPS=13`, 51.4-51.5);
  game 10013181, interactions game 10019015 and match 10018076 (`FUZZ_GROUPS=13`, 51.6); the
  `team_draft` smoke's seed 1000. The two of ab02574 (drafted 10043680, interactions 10039110) on the
  desktop: pass on the branch and from 1759983, fail on 692a52c.
* **The agreement fuzz** (49.4; the Mac, eight processes of 400,000 boards from 0): 3,200,000 boards,
  7,429,517 priority decisions of P0, 22,688,675 options (10,476,766 of them casts), 136,132,050 paths,
  0 failures (172 s).
* **Python**: `bash tools/build_py.sh`, `python -m pytest -q tests`: 35 passed on the desktop; 34
  passed, 1 skipped on the Mac (no CUDA).
* **Search smoke** (the Mac): `python tools/search_smoke.py --specs random,match,team_draft,team_draft_2v2
  --seeds 0-3`: 0 failed of 16 configurations (3,200 games and 2,173,758 decisions; 192 team-draft
  events and their 1,536 matches, 93,293 draft decisions; 255 s).
* **The noise study's five panics** (`tools/match_noise.py repro --tag <tag> --player heuristic`,
  section 50.5): every match completes on the branch, and on 692a52c with the same results: fixed by
  ab02574.
* **Speed** (the desktop with td1 live; `tests/perf.rs` `measure` built at 692a52c and at 8e1211f, run
  interleaved, five runs each): random play 7.26-7.54 us per decision on 692a52c and 6.25-6.31 on the
  branch (841,706 decisions: the same games); clone 1,490-2,094 ns (one outlier; else to 1,504) and
  1,451-1,513 ns; clone + `determinize` 1,978-2,587 and 1,893-2,196 ns; `determinize` alone
  1,105-1,460 and 1,170-1,543 ns; `legal_actions` 5-8 and 3-10 ns; `size_of::<Game>()` 1,600 bytes on
  both (the two new fields fit in padding). The branch with the old `covers` put back: 7.28 and 7.32
  us per decision: the speed-up is `covers`'.
* **The search bound's headroom** (51.3): an instrumented build (a maximum per search kind, not
  committed) on the Mac.

### 51.9 Limits

* `ways_payable` is exponential in the cards with several ways to pay (Talismans, painlands, an
  artifact tapped for Urza or improvise ...): 20,103 nodes on one default match board, about 15 such
  cards. The bound turns a pathological board into a loud failure (with the search's inputs), not a
  fast search; identical cards (several Talismans of one kind) could be searched as a multiset if a
  board ever comes near it.
* The surveil fix covers looked-at piles (`Op::PileTo`); a replacement's shuffle that needs a
  decision of its own (Blightsteel Colossus under Dauthi Voidwalker: library or exile) still
  shuffles when that choice is answered, after the pile's op has moved on. Milling (no pile) shuffles
  at once, as before: the milled cards are chosen before, by slot.
* The hidden pool's fix makes the first deal respect the clouds; a world's later `Game::determinize`
  still computes the payer's pins from the world's identities (a hand mana source sampled into the
  hand is pinned): consistent now that every cloud's card is in its domain before it.
* `FUZZ_GROUPS` reproduces an older commit's interactions decks only while the groups before `n` are
  unchanged (groups are only appended).

## 52. Remote self-play: the Mac self-plays for a run (M6)

Branch `m6-remoteselfplay` (from main 692a52c; tools, presets, tests and docs: the engine is main's),
zero-core pinned at `feat/remote-selfplay` (from core main af44741; its docs/az.md 5.3 and 11.6). The
user's decision of 2026-10-28: build 50.3's design. Why: the MacBook (M4 Max, CPU inference of the
128 x 4 token network) self-plays at 19.4k / 33.0k / 37.1k leaves/s with 1 / 2 / 3 driver processes
(50.3) where the desktop's GPU self-play does 6.3-11.4k, and the 4v4 co-training run td1 spends most
of its iteration in self-play waiting for its matches (td1's iterations 2-3: self-play 843 and 865 s
of 1,666 and 1,683, the draft network waiting 586 and 628 s of it for backpressure: the play
network's matches are the bottleneck).

### 52.1 What was built

* **zero-core** (`zero/remoteplay.py`; its docs/az.md 5.3, 11.6): run setting **`selfplay_remote`**
  (a remote of the game package, the same `remotes` entry as `eval_remote`) and, for a co-training
  run, the resolver's **`resolver_cfg['selfplay_remote']`**; `selfplay_remote_procs` (0 = the
  remote's `procs`), `selfplay_remote_wait` (30 s), `selfplay_remote_timeout` (3,600 s).
  * The run keeps `procs` **persistent workers** on the Mac, each one ssh session to `python -m
    mtgz.az.remote selfplay <mirror> <K>` (stdin: control lines; stdout: events), its rolling driver
    alive across the run's iterations. As soon as training has written checkpoint i-1 the run ships
    it to the mirror and tells the workers `phase i` (a new search epoch: the checkpoint the desktop's
    next phase will use), so the Mac plays through the desktop's evaluation phase; when the desktop
    has played its quota it tells them `end i`, they end their phase (games in flight carry over, as
    the desktop's rolling driver's do) and flush their last shard. The workers idle only while the
    desktop trains.
  * **Shards**: `data/iter_i/m<KK>-<sid>_SSS.npz` (8,192 samples, zlib level 1 on a background
    thread), announced on stdout as soon as written, copied by the run's fetch threads (one per
    worker) under a temporary name, checked on arrival (the non-finite guard, the run's game,
    features and actions) and renamed into the run's `data/iter_i/` (`resolver/data/iter_i/` for
    co-training) beside its `r00` shards; the Mac's copy is deleted. The run waits for a phase's
    last shards at most `selfplay_remote_wait` s; later ones are dropped, never put into another
    iteration (same checkpoint only). A non-finite shard is quarantined to `<run>/debug/` and the run
    fails (NonFinite) before training, as for a local one.
  * **Co-training**: the remote is a second consumer of the coordinator's queue: the local resolver
    driver keeps at most max(16, resolver_conc / 4) matches queued in its driver, the remote takes up
    to conc x 1.25 (960) outstanding per worker, oldest first; rows come back by tag into the
    coordinator (an event resolves when its 12 rows are in, from either machine), samples as shards.
    A worker that dies or stalls has its outstanding matches requeued at the front at once; a match
    not finished within `selfplay_remote_timeout` s of its worker's running time is requeued too, and
    the second copy's row is dropped (at worst played twice). The quota (events resolved) and the
    events' resolution are the coordinator's as before. `pending_max` 0 = 2 x resolver_conc + the
    remote's room.
  * **Robustness**: everything ssh happens on the run's threads; the main thread only starts and ends
    phases (the end bounded). A session that fails (the Mac asleep, unreachable, its checkout refused)
    is started again after 60 s (logged the first three times in a row, then every 10 minutes); one
    silent for 300 s is killed; the workers exit on `quit`, on stdin closing (the run gone) or after
    180 s without a control line (the run pings every 20 s). PAUSE, STOP, a crash and the end of the
    iterations close them. A restart replays the interrupted iteration (its data directory cleared,
    as before) with new sessions; a new worker for a slot stops the same run's previous one if it is
    still alive. Nothing writes into the run's directories except the fetched shards (and the
    quarantined ones in `debug/`).
  * **Remote evaluation first**: a worker watches `~/mtgz-remote/.eval.lock` (the remote's own
    workdir's lock, always) and the remote's `selfplay.locks` (mtgz: `~/kbz-remote/.eval.lock`,
    `~/domz-remote/.eval.lock`); while one is held by a live process it stops asking for leaf batches
    (`backoff` 'stop', the default), or with `backoff` 'background' plays on under the Darwin
    background policy (52.4). A phase holds a **busy marker** `~/mtgz-selfplay/.busy-<mirror>-w<K>`
    (owner "<pid> <time>").
  * Also in the core: the training window subsamples each shard as it reads it (the same rows, a
    peak of the window plus one shard instead of every shard: td1's resolver window at full size is
    ~31 GB of shards before the Mac's data; 3 times that would not fit the desktop's 68 GB), compressed
    shards (`data.savez_level`), metrics `selfplay.remote` and progress.json's `remote` /
    `matches_remote`, the dashboard's status line, `zero.remote._alive` without `os.kill(pid, 0)` on
    Windows (it terminates the process there).
* **mtgz**: `RunCommitRemote` carries the spec's `selfplay` dict, resolved with the rest (the Mac's
  checkout of exactly the run's commit; `python -m mtgz.az.remote selfplay` runs `verify_checkout`
  first, as `eval` does). The remote 'mac' (`mtgz/az/__init__.py`): `selfplay = {workdir
  ~/mtgz-selfplay, procs 2, nice 5, locks [kbz, domz evaluation locks], env OMP_NUM_THREADS=3,
  overrides {infer_device cpu, conc 768, threads 12, groups 4, infer_workers 4, torch_threads 3,
  shard_rows 8192}}` (50.3's best setting; 2 processes: 33k of 3's 37k leaves/s with room left on the
  Mac). Presets, opt-in: **`mtg-4-mac`** (mtg-4 with a desktop quota of 200 matches and
  `selfplay_remote` 'mac': 50.3's single-game plan) and **`team_draft-4v4-mac`** (team_draft-4v4 with
  the play network's matches on the Mac too); no existing preset changed. `tools/mac_backoff.py`'s
  default locks add `~/mtgz-selfplay/.busy-*` (glob patterns allowed): the fuzz farm and the studies
  stop while a run's self-play phase runs on the Mac (52.4).
* **Tests**: zero-core `tests/test_remoteplay.py` (17: the remote 'local'), `tests/test_net_data.py`
  (the streamed window); mtgz `tests/test_mac_remote.py` (the spec's `selfplay`, the Mac's settings,
  a worker refusing other code, a worker playing `mtg` self-play and queued best-of-three matches
  through the core's remote side, the back-off's glob), `tests/test_drafted.py` (the presets).

### 52.2 Deviations from 50.3

* **Persistent workers, not a process per iteration.** 768 matches in flight per process at ~1
  match per second take ~13 minutes to finish their first wave; a fresh driver every iteration
  would spend the first quarter of an iteration finishing nothing and drop ~half a wave at its end.
  The workers keep their driver across iterations, exactly as the desktop's rolling self-play does:
  a phase boundary is a new search epoch with the new checkpoint, games in flight carry over (so a
  shard of iteration i holds checkpoint i-1's decisions and the carried-over games' earlier ones,
  like the desktop's own `r00` shards).
* **The head start is 50.3's**: the remote's phase i starts "after training writes models/iter_{i-1}.pt",
  i.e. before the desktop evaluates that checkpoint (td1: 13-23 minutes of evaluation per iteration,
  plus `eval_yard_net` every 4th), not when the desktop's own phase i starts. The first build started
  it with the desktop's phase, which left the Mac idle through every evaluation (its duty cycle in
  td1 would have been ~25%); changed after the single-game end-to-end run, before the co-training
  one (zero-core aee1a2f). Still on-policy: the checkpoint is the one the desktop's phase i uses.
  `lead_s` in the metrics is the head start.
* **Shards stream back during the phase** (each announced and fetched as soon as written), not
  only at its end: the end waits for the last shard per worker. Default deadline 30 s (50.3: 60).
* **Staleness: the same phase only.** A shard is accepted only into the iteration whose phase
  produced it, and only until the deadline. 50.3's "simpler alternative" (the Mac one checkpoint
  behind, never waiting) is not built.
* **Co-training's split** is a shared queue with two consumers (the local driver's queue capped,
  the remote's outstanding capped), not 50.3's fixed share: each machine takes work as it has
  room. Timeouts count the worker's running time (not wall time), so the desktop's evaluation
  phases and the Mac's back-offs do not requeue matches.
* **Back-off**: 'stop' as 50.1's rule (the default), plus an optional 'background' mode (macOS's
  background policy) built for the long evaluation every 2nd iteration and measured not worth it
  (52.4: 2-5% of the workers' speed). **The fuzz farm**: 50.3's lock "of its
  own" is the busy marker, honoured by `tools/mac_backoff.py`; the farm runs code installed from
  main, so it yields once main has this branch and the farm is reinstalled (52.4 measures what it
  costs until then).
* **Mirrors** in `~/mtgz-selfplay/<run>` (single-game) and `~/mtgz-selfplay/<run>.resolver`
  (co-training), not in `~/mtgz-remote` (the evaluations' workdir and lock stay as they were).
* **Training** follows the new samples (`reuse` unchanged: more samples, more steps), as 50.3
  said; the window needed the streamed load (52.1) to stay inside the desktop's RAM.

### 52.3 End to end with the Mac (`mtg-smoke`, `team_draft-smoke`)

From this worktree (clean, committed), `bash tools/mac.sh --eval-checkout` (~40 s:
`~/mtg-zero-wt/run-<sha12>`), the mirrors in a test workdir (`ZERO_SELFPLAY_WORKDIR=/Users/greggkeithley/mtgz-selfplay-test`,
with `MSYS2_ENV_CONV_EXCL=ZERO_SELFPLAY_WORKDIR`: Git Bash rewrites a POSIX path in the environment of a
Windows process, which the first attempt showed - its worker could not find its mirror, and ssh had
created `~/C:/Program Files/Git/...` on the Mac, removed; such a workdir is now refused at start),
CUDA hidden, the desktop shared with td1; each run 3 iterations in `E:/mtg-zero-scratch/remote-selfplay/`
and the same without the Mac as the baseline:

```
python -m mtgz.az.run --run <scratch>/sg_mac --preset mtg-smoke --iters 3 --games 64 --selfplay-remote mac
python -m mtgz.az.run --run <scratch>/td_mac --preset team_draft-smoke --iters 3 --games 8 \
  --resolver-cfg '<team_draft-smoke resolver_cfg + "selfplay_remote": "mac">'
```

**`mtg-smoke`, single games, 64 per iteration** (the run at a9d5bfc, before the head start: the first shards fetched one at
a time and the local phase's seconds still counting the wait - both fixed since, 7e90ded):

| | iteration 1 | 2 | 3 |
|---|---:|---:|---:|
| desktop: games, samples (the same quota with or without the Mac) | 64, 3,755 | 64, 3,553 | 64, 3,107 |
| **Mac: games, samples** | **949, 30,623** | **1,478, 72,046** | **951, 49,723** |
| Mac: shards, MB shipped (compressed: x 58.2-61.7 against the desktop's npz) | 4, 33.4 | 10, 82.3 | 7, 57.5 |
| Mac: its phase, leaves/s (2 workers, the tiny smoke network: ~114k each) | 4.8 s, 228,156 | 5.2 s, 210,025 | 3.5 s, 198,490 |
| the desktop's wait for the Mac's last shards (copies: 2.5-3 s each, one at a time then) | 10.7 s | 27.2 s | 16.3 s |
| training: steps, seconds (the samples of both) | 1,075, 36 s | 2,363, 77 s | 1,651, 60 s |
| **iteration** with the Mac / without (baseline: 118-123 steps, 4-5 s training) | **58 s / 14 s** | **115 s / 14 s** | **87 s / 15 s** |

The workers came up 3 s into iteration 1 (ssh, Python, the engine), got their checkpoint and played
3.5-5 s of each of the desktop's 5-9 s phases (no head start in this run); at this scale (a 32 x 1 network) the Mac plays
15-20 times the desktop's samples in that time, and the iteration grows with the training on them
(3 iterations: 4 min 26 s against 47 s). The quota stays the desktop's (52.2): a single-game run with
the Mac gets more data per iteration, not shorter iterations, unless its quota is lowered
(`mtg-4-mac`). On the Mac after the run: no worker, no slot lock, no busy marker; the mirror's
fetched shards deleted.

**`team_draft-smoke`, co-training, 8 events (32 matches) per iteration** (at e428eba, the head start
included; the Mac's 2 workers took matches only once the desktop's 16 slots and 16-long queue were
full, so a few dozen at a time - hence its low leaves/s here):

| | iteration 1 | 2 | 3 |
|---|---:|---:|---:|
| events resolved (the quota) | 8 | 8 | 8 |
| matches back: desktop + **Mac** | 16 + **38** | 1 + **62** | 9 + **10** |
| Mac: samples, MB shipped | 7,497, 8.4 | 12,607, 14.4 | 949, 1.0 |
| Mac: head start before the desktop's phase (`lead_s`), its phase | 0 s, 36 s | 15 s, 15 s | 56 s, 67 s |
| Mac: leaves/s, matches still outstanding at the end | 6,206, 62 | 4,291, 0 | 759, 33 |
| the desktop's self-play phase with the Mac / without | 42 s / 41 s | **3 s / 617 s** | 12 s / 34 s |
| the end's wait for the Mac's shards | 2.8 s | 2.7 s | 0.7 s |
| **iteration** with the Mac / without | **73 s / 66 s** | **80 s / 675 s** | **46 s / 69 s** |

3 iterations: **4 min 5 s with the Mac, 14 min 5 s without**. No match was requeued, no duplicate. In
iterations 2 and 3 the Mac played through the desktop's evaluation (15 and 56 s, the latter with
`eval_yard_net`) and the events it completed were resolved in the first seconds of the desktop's
phase. The baseline's iteration 2 lost ~9.5 minutes to an idle stretch that is not this branch's: the
same run at main 692a52c (zero-core af44741) stops at exactly the same point (6 of 8 events
resolved, 16 matches in flight, 29-30 done; its resolver thread waiting in the driver's
`next_batch`, the draft network on backpressure, the process idle) and its iteration 2's self-play
takes 649 s (the resolver at 129 leaves/s, 637 s of backpressure; iteration 1 identical to this
branch's to the sample: 33 matches, 6,019 samples, 189 steps) - with the Mac, those matches ran on
the Mac's drivers (52.8).

### 52.4 The Mac with the play network: throughput, the farm, the back-off modes

The workers' own measurement (`bgx.py`, `bgx2.py` in `~/mtgz-selfplay-test/`: two workers with the
Mac's settings run directly - `zero.remote.main(['selfplay', ...])` - on mtg-4's self-play with
runs/mtg5's iteration 14, td1's starting play network; 90 s windows after a 70 s warm-up; leaves
from the workers' status lines; the Mac shared with other agents' jobs, load 3-15):

| | leaves/s (2 workers) |
|---|---:|
| alone | **31,108** (re-test: 30,765; 50.3's two bench processes: 33,036) |
| beside 4 nice-19 CPU burners (the fuzz farm's footprint: 4 single-threaded jobs) | 28,034 (**-9.9%**) |
| `backoff` 'background' while a test lock is held, beside an evaluation-like job (`match_noise.py bench`, the evaluation's settings: 12 threads, 2 groups, 3 x 4 inference, 128 matches, normal priority) | 736 (2.4%) |
| 'background' with the lock held and nothing else of ours running (re-test) | 1,654 (5.4%) |
| after the lock is gone (re-test: the workers logged "out of the background after 74-77 s") | 31,836 (all of it back) |

| the evaluation-like job | leaves/s |
|---|---:|
| alone (the workers stopped) | 8,247 |
| beside the two workers in 'background' | 8,275 (the same) |

* **Throughput**: two workers deliver 31k leaves/s with td1's play network, as 50.3 measured with
  its bench (the same driver loop). At td1's 18,254 leaves per drafted match (its iteration 3:
  9,090,702 leaves for 498 matches) that is **1.70 matches per second**, against the desktop's 0.58
  (td1: 498 matches in 865 s): 2.9 times the desktop's resolver.
* **The fuzz farm costs ~10%** at nice 19 beside the workers' nice 5 (the burners' measurement):
  nice alone is not enough, so the farm yields - `tools/mac_backoff.py` stops its jobs while a busy
  marker is held - once it runs code with this change (a reinstall from main). During this work the
  farm was paused by m6-farmfix (its `PAUSE`, untouched), so nothing of it ran beside the workers.
* **'background' does not slow the evaluation** (8,275 against 8,247 leaves/s) **and leaves the workers
  2-5%** of their speed (the Darwin background band on the efficiency cores loses to anything else);
  they recover fully when the lock goes. 'stop' stays the default: the 2-5% is not worth a mode
  whose throughput depends on what else runs. (A first attempt kept the workers in the background
  after the lock's holder was killed: an unreaped child still answers `kill(pid, 0)`; zombie owners
  now count as dead, `lock_held`.)
* **When the play network's evaluation runs** on the Mac (every 2nd iteration; td1's: 410-419 s,
  44.4's idle Mac: 560 s): with the early start (52.2) the remote's phase overlaps the desktop's
  evaluation phase, which is when the Mac's own evaluation of the same checkpoint runs (td1 queues it
  right after training): the workers stop for those ~7 minutes and play the rest of the desktop's
  13-23 minute evaluation phase and its self-play.

### 52.5 td1 with the Mac: the projected iteration

**td1 today** (its metrics and log; core af44741, the play network's evaluation already on the Mac):

| iteration | self-play (matches; the draft network waiting on backpressure) | training | the draft network: vs `consensus` + random | its Elo matches | `eval_yard_net` | iteration |
|---|---:|---:|---:|---:|---:|---:|
| 2 | 843 s (531; 586 s waiting) | 89 s | 152 s | 581 s (2 opponents) | - | 1,666 s (27.8 min) |
| 3 | 865 s (498; 628 s) | 73 s | 155 s | 590 s (2) | - | 1,683 s (28.1 min) |
| 4 | 1,214 s (664) | 96 s | ~150 s | (3 opponents) | 5,275 s (88 min; iteration 0: 3,073 s) | |

The play network's matches are the self-play phase's bottleneck: the desktop plays 0.55-0.61 matches
per second (10.2-10.5k leaves/s at 18,254 leaves per match) and the draft network, which drafts 48
events in ~240-260 s of its own time, waits for them 70% of the phase.

**With the Mac** (52.4: two workers, 31.1k leaves/s = 1.70 matches/s; 2.9 times the desktop's
resolver, 3.9 times together):

* **Self-play**: 576 matches per iteration take 2.28 matches/s -> ~4.2 min of both machines; and the
  Mac's head start (52.2) adds the desktop's whole evaluation phase (13-23 min, ~7 min of it stopped
  for its own evaluation of the play network every 2nd iteration: td1's take 410-419 s there) -
  far more than an iteration needs. So the matches stop being the bottleneck and **the phase
  becomes the draft network's drafting, ~4-4.5 min** (48 events at td1's measured rate), against
  14-20 min today. (The co-training smoke of 52.3 shows the effect at small scale: with the head
  start, the desktop's phases of iterations 2-3 took 3 and 12 s.)
* **Training**: unchanged (~1.2-1.6 min): an iteration still resolves 48 events, ~576 matches, about
  the same samples; the Mac plays a share of them rather than more of them.
* **The draft network's evaluation, unchanged and now the largest part**: vs `consensus` and random
  ~2.5 min; its Elo matches ~5 min per opponent (td1: 2 opponents ~10 min at iterations 2-3; 3 at
  4-7, ~15 min; 4 from iteration 8, ~20 min); `eval_yard_net` every 4th iteration, 51-88 min.

| | today | with the Mac | |
|---|---:|---:|---:|
| an iteration like 2-3 (Elo vs 2) | 28 min | **~18 min** | 1.5x |
| iterations 5-7 (Elo vs 3) | ~33 min | **~23 min** | 1.4x |
| iterations 9-11, 13-15 ... (Elo vs 4) | ~38 min | **~28 min** | 1.35x |
| every 4th (+ `eval_yard_net`, 51-88 min) | ~89-126 min | ~79-116 min | |
| **average from iteration 8 on** (`eval_yard_net` 51-88 min) | **~51-60 min** | **~41-50 min** | **~1.2-1.25x** |

So td1's ordinary iterations go from 28-38 to ~18-28 minutes (50.3's "20-30" for the self-play-bound
iteration), but its average, with `eval_yard_net` every 4th, only from ~51-60 to ~41-50: the Mac saves
~10 minutes of self-play per iteration, and the draft network's evaluation on the desktop (its Elo
matches, `eval_yard_net`) becomes ~60-70% of an iteration. The next levers are there, not in
self-play: `elo_opponents` 2-3 or fewer `elo_games` for the draft network, `draft_eval_net_every` 8,
`eval_yard_net` on the Mac (48.6's idea; the remote matches workers of this section could play its
episodes with the play network at the evaluation settings - a non-recording worker mode - not done);
and with most matches on the Mac, a smaller desktop `resolver_conc` (e.g. 256) would leave the GPU to
the drafting and the evaluations.

### 52.6 Turning it on for td1 (at a restart)

A run can use `selfplay_remote` only from a commit that has this code on both machines (td1's pinned
commit has none of it), so the switch is a re-pin, as for 48.3:

1. This branch on main (the user's merge; main has m6-farmfix's engine fixes and its section 51 since
   433ff28: both branches append a section at the end of this file, a conflict of order only), a
   pinned worktree of the commit `<S>`: `git -C ~/mtg-zero worktree add --detach ~/mtg-zero-wt/run-<s7> <S>`,
   there `CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/run-<s7> bash tools/build_py.sh`.
2. There: `bash tools/mac.sh --eval-checkout` (the Mac's `~/mtg-zero-wt/run-<s12>`, ~40 s: the same
   checkout serves the play network's evaluations and the self-play workers).
3. Stop td1 cleanly after an iteration: `touch runs/td1/STOP`; the run exits after the current
   iteration and the supervisor with it (a play-network evaluation still pending on the Mac is
   queued again at the restart, 48.1).
4. `rm runs/td1/STOP`, and from the new worktree:

   ```
   python tools/launch_hidden.py "C:/Users/Gregg Keithley/mtg-zero/runs/td1/supervisor_console.txt" \
     python tools/supervise.py "C:/Users/Gregg Keithley/mtg-zero/runs/td1" --preset team_draft-4v4 \
     --resolver-cfg '{"preset": "mtg-4", "eval_every": 2, "eval_remote": "mac", "selfplay_remote": "mac"}'
   ```

   (`--resolver-cfg` replaces the saved dict: the whole of td1's, plus `selfplay_remote`.) The log
   says `[resolver] episodes also on the remote 'mac' (greggkeithley@100.95.248.116, 2 worker(s),
   mirror /Users/greggkeithley/mtgz-selfplay/td1.resolver)`, within seconds `[resolver] remote
   self-play mac: worker 0 up (pid ... on Greggs-MacBook-Pro.local, ...)`, and after each self-play
   phase `[resolver] iter N remote self-play: 2 worker(s), ... games, ... samples in ... shards
   (... MB), ... leaves/s ...; matches ... back, ... assigned, ... outstanding, ...`; the primary's
   `cotrain` row gets `episodes_remote`, the resolver's metrics `selfplay.remote`; progress.json
   `matches_remote` (the dashboard's status line: `remote mac: playing, 2 worker(s), ...`). Its
   `pending_max` (0) becomes 2 x 768 + 2 x 960 = 3,456 by itself.
5. Optional: `"selfplay_remote_procs": 3` in the same dict for 37k instead of 33k leaves/s (50.3:
   the Mac's last 12%, leaving less room for the other agents' jobs there). Not recommended: the
   Mac's `backoff` 'background' (52.4: 2-5% of the speed while an evaluation runs).
6. The fuzz farm yields to the busy markers once reinstalled from a main with this branch (`bash
   tools/mac.sh 'bash tools/fuzz_farm.sh install'` in main's checkout; it keeps its state and
   cursors).

Off again at a restart: the same `--resolver-cfg` without `"selfplay_remote"` (or with `""`); the
iteration then is exactly today's. A single-game run: `--selfplay-remote mac` (`''` to stop), and
`--games` lower than before (the Mac adds its share to the desktop's quota, it does not replace it:
`mtg-4-mac` has 200).

### 52.7 Checks

At e428eba, zero-core aee1a2f (the code; the later commits pin the core's test and docs commits
only; the engine is main 692a52c's: this branch changes presets, `mtgz/az/mac.py`, a tool, tests and
docs; the core's Rust is af44741's):

* **Engine tests on the Mac** (`bash tools/mac.sh 'cargo test --release --no-fail-fast'`: every test
  binary of the workspace, `az` included; the desktop runs no `az`): **1,398 passed, 0 failed, 13
  ignored** (105 result blocks; `az` 14 passed), the build 5 min 43 s, the Mac shared with the
  resumed fuzz farm.
* **Behaviour digests** (Mac): combined digest **1ed7e08dd5078f55** (main's; 2,000 random and 500
  heuristic games in 9.1 s).
* **Python**: `bash tools/build_py.sh` (desktop 1 min 56 s; Mac); `python -m pytest -q tests`: desktop
  **40 passed** (692a52c's 35 and the 5 new), Mac 39 passed, 1 skipped (CUDA).
* **Search smoke** (Mac, `python tools/search_smoke.py --specs random,match,team_draft --seeds 0-1`):
  **0 failed of 6**, 126 s: random 400 games each (161,770 and 166,767 decisions), match 400 each
  (385,154 and 381,064 decisions, 10.2 and 10.1 million simulations), team_draft 24 events each, all
  resolved (team A won 9 and 11, drew 6 and 7), 288 matches each, 15,581 and 15,539 draft
  decisions, parked records at most 10.64 and 12.89 MB.
* **zero-core** (`feat/remote-selfplay`): `cargo test --release` **44 passed** (the Rust unchanged);
  `python -m pytest tests` **128 passed** (110 and 18 new: 17 in `tests/test_remoteplay.py`, the
  streamed window). On the Mac (through this checkout's submodule, its own target dir):
  `tests/test_remoteplay.py` and `tests/test_net_data.py` **39 passed, 1 skipped** (CUDA) - the
  darwin paths ('background' set and cleared, POSIX liveness, niceness); a first run there failed
  one test's timing assumption (progress.json is written every 2 s, the toy's phases there are
  shorter), fixed in the test (5cd9c36: no code changed).
  `SMOKE_TARGET=remoteselfplay bash tools/smoke_all.sh --with-this-core` (the games at their main
  checkouts' HEADs, each in a temporary worktree with the core moved to this branch: 000e243 for
  the first three, 5cd9c36 - the same code - for MTG Zero, which the test fix reached mid-run):
  **GREEN**, 19 steps, 37 min: core cargo test 44, pytest 128; Glass Road c1f56dc cargo test 46,
  pytest 33, `glassroad-smoke` 4 rows; Caverna a6fad73 47, 32, `caverna-smoke` 4 rows; Kingdom
  Builder 4202a4a 105 (4 ignored), 45, `kingdombuilder-smoke` 4 rows; MTG Zero main 433ff28
  (m6-farmfix's engine fixes; core af44741 pinned there) cargo test 1,407 (every binary, `az`
  included; 13 ignored), pytest 35, `mtg-smoke` 4 rows.
* **The remote's failure paths** (zero-core's tests, the remote 'local'): an unreachable host (ssh to
  `.invalid`: every start retried and logged, the run trains on its own samples), a worker killed
  mid-phase (its episodes requeued, a new worker), a slow worker (episodes requeued on the timeout,
  duplicates dropped, quotas exact), shards later than the deadline (dropped), a damaged shard
  (quarantined, NonFinite before training), an evaluation lock held (the worker stops; macOS's
  'background' mode on the Mac, 52.4), PAUSE mid-phase (exit 75, no worker left, the replayed
  iteration without the old session's shards), a remote workdir that is not an absolute path
  (refused at start).

### 52.8 Limits and open issues

* **Not reproducible**, as 50.3 said: what the Mac contributes to an iteration depends on timing
  (when its sessions started, how long the desktop's phase lasted, its back-offs), and the Mac's
  CPU arithmetic differs from the desktop's CUDA at float rounding (44.4: other games then). A
  worker's games are reproducible per session (its driver's seed is `seed x 7919 + i0 + 7,777,777 x
  (K + 1)`); every sample carries its search's `net`, every remote shard's meta its worker and
  session; the games themselves are not recorded (nor are the desktop's).
* **Single-game runs: more data, not shorter iterations**, unless the desktop's quota is lowered
  (the quota stays the desktop's by design; `mtg-4-mac` has 200 matches): with the same quota an
  iteration is as long as before plus its training on several times the samples (52.3).
* **Co-training's split favours the desktop at a phase's start**: the local driver's free slots take
  queued matches first (its queue is capped, its slots are not). In td1 the queue is long (the
  backpressure lets 3,456 matches be outstanding), so both machines stay busy; a small run (a few
  events per iteration, 52.3) can leave the Mac little.
* **An event's latency**: an event resolves when its last match does, on whichever machine; matches
  that time out (3,600 s of the worker's running time) are played again here. A Mac match that
  hangs (an engine bug such as 50.2's payment search at game 10049522) stalls its worker's group:
  the worker's session is killed after 300 s of silence and its matches are requeued here, where the
  same spec would hang the desktop's driver too (as it would have without the Mac).
* **The fuzz farm** yields only after a reinstall from a main with this branch; until then it runs
  at nice 19 beside the workers' nice 5 (52.4: what that costs). It was paused by m6-farmfix during
  this work (its `PAUSE`), untouched here.
* **Two runs self-playing on the Mac** at once share its cores (no lock between self-play runs);
  other projects' remote evaluations (`~/kbz-remote`, `~/domz-remote`) stop the workers (their
  locks), other projects' remote self-play would not.
* **A pre-existing idle stretch in co-training self-play** (not this branch's; found by its baseline,
  52.3): `python -m mtgz.az.run --preset team_draft-smoke --iters 3 --games 8` (CPU) idles ~9.5
  minutes in iteration 2's self-play - 6 of 8 events resolved, 16 matches in flight, the resolver
  thread waiting in the driver's `next_batch`, the draft network on backpressure, the process at
  ~0% CPU - then goes on; the same at main 692a52c with zero-core af44741 (649 s against 617 s, at
  the same counts; the resolver's rolling stats are those of the iterations that do not stall). Not
  investigated (the core's Rust driver: a wait inside `next_batch` with games in flight). Related
  or not: td1's iteration-4 `eval_yard_net` took 88 minutes with its process nearly idle for long
  stretches (07:31-08:59; iteration 0's: 51 min); td1 was not touched.
* zerodash (zero-core's dashboard) shows nothing of the remote yet; the game dashboard's status
  line does (progress.json), its throughput charts show the desktop's leaves/s only.
* On the Mac: `~/mtgz-selfplay-test/` (this section's mirrors, the experiments' scripts and logs, the
  checks' logs and the mtg5 checkpoint copy; ~40 MB), the checkout `~/mtg-zero-wt/run-e428ebadcdca`
  and its target dir (0.4 GB; `run-a9d5bfcf7c89` of the first runs removed),
  `~/mtg-zero-wt/m6-remoteselfplay` and its target dir (the checks, 4.2 GB),
  `~/cargo-target/zero-core/remote-selfplay` (the core's toy module for its tests there, 133 MB);
  `~/mtgz-remote` untouched; `~/mtgz-selfplay` does not exist until a run uses it.

## 54. Constructed play: the Legacy Izzet Delver mirror (D1)

Branch `delver-d1` (from main at b11952b) with mtg-bot branch `feature/legacy-delver-ir` (from
`feature/companion-ir` 62210e89). The side project (DESIGN.md 8, the user's decisions of
2026-10-28): AlphaZero self-play on one constructed matchup, the Legacy Izzet Delver mirror, both
seats the pinned 75 "Izzet Delver" by i_b_TRUE (Legacy Challenge 32, 2026-10-23, 1st place 8-1,
MTGO; https://www.mtggoldfish.com/deck/8016541); run 1 best-of-one with the maindeck only, run 2
best-of-three with sideboarding; runs under `runs/delver1`. D1 builds everything up to the run and
stops. Scope (the user, 2026-10-28, during D1): only the two maindeck cards outside the cube
(Murktide Regent, Prismari Charm); the five sideboard cards outside it wait (54.8). Layout v4,
`NUM_ACTIONS` and every action id are unchanged; the cube's behaviour is unchanged (the digests,
54.7).

### 54.1 What was built

| Part | Where | What |
|---|---|---|
| The deck | `data/decks/izzet_delver_8016541.json`, `.txt` | the 75 with its provenance (source, download URL, event, date, player, result, platform); the MTGO export verbatim next to it (`export_names` maps its two names that differ: Tamiyo by her front face, "Rough/Tumble") |
| Loaders | `engine/src/az/constructed.rs` (`Decklist`), `mtgz/az/decklists.py` | the JSON (compiled into the engine), the export checked against it (tests), the mirror's spec |
| The cards | mtg-bot pool `legacy` (its `data/legacy_delver.json`: Scryfall records, no basic lands); `data/cards/legacy.ir.json` | the IR of the two cards, encoded with the cube's schema (54.2) |
| A second card table | `CardDb::constructed()` (`db.rs`) | the cube's 546 cards (the same definitions at the same ids as `CardDb::cube()`), then the legacy cards, then the tokens and faces; every legacy card must be supported (a loud failure otherwise) |
| Identities | `engine/src/az/legacy_cards.txt` (`identity::legacy_registry`, `LEGACY_BASE` = 546) | 546 + the line: Murktide Regent 546, Prismari Charm 547 (22.2); append-only, in the free range below `CUBE_CAP` (layout v4 unchanged). The cube list must stay at 546 cards: `identities` asserts it fits below `LEGACY_BASE`, so a card added to the cube list fails loudly there; acceptable, since another cube list is a new layout anyway (22.2), which then moves `LEGACY_BASE` above it |
| Rules | `cast.rs`, `pay.rs`, `resolve.rs`, `game.rs`, `triggers.rs`, `lower/` | 54.3 |
| The spec | `MtgSpec.constructed` (`"constructed": true`) | `CardDb::constructed`, CR 100.2a / 100.4a decklists, best-of-one only (54.4) |
| The presets | `delver-mirror`, `delver-smoke` (`mtgz/az/__init__.py`) | 54.6 |
| Gates | `tests/cards_legacy.rs`, `tests/constructed.rs`, `az.rs` `legacy_registry_and_identities`, `tests/measure_delver.rs`, `FUZZ_POOL=delver`, the search smoke's `delver` spec, `tests/test_delver.py`, the rulings | 54.7 |

### 54.2 The IR: two cards, one schema

Prismari Charm parsed already (mtg-bot claims it in its roto pool): `ModalChooseOne` of
`Sequence(Surveil 2, DrawCards 1)`, `DealDamageToEachTarget` (amount 1, one `TargetSpec` of 1..2 any
targets) and `Bounce` (target nonland permanent). Murktide Regent did not: three of its four lines
failed. mtg-bot's `cards/parser/legacy_creatures.py` (parameterized recognizers, hooked into the
creature builder before the U/B bucket's) adds `Delve` on a creature (`has_delve`); "This creature
enters with a +1/+1 counter on it for each <types> card exiled with it"
(`CardPrinting.enters_with_counter_per_card_exiled_with_it`, an object predicate:
`ObjectOr(HasPrintedCardType Instant, HasPrintedCardType Sorcery)`); "Whenever a <types> card leaves
your graveyard, put a +1/+1 counter on this creature" (`Event.LEAVES_GRAVEYARD`, subjects `player` =
the graveyard's owner and `card`; the predicate `Subject(card, ...) & Controlled(player)`). The Python
engine plays neither ability (data for this engine, as companion was: a permanent entering with
either raises `NotImplementedError`; mtg-bot SEARCH_LIMITATIONS, "Legacy pool" 1). The pool `legacy`
is in no other pool; the cube, roto, EoE and `all` pools' claimed printings and IR are unchanged
(compared card by card with the branch point).

**One schema for both files.** mtg-bot builds a schema from the printings it encodes, so a
legacy-only export would carry another schema than the cube's, and the `card_ir` crate (generated
from one schema, checking every document's hash) could not read both. `mtg_bot cards export-ir --pool
cube legacy` builds one schema over both pools' printings and writes `powerlsv.ir.json`,
`legacy.ir.json` and `powerlsv.schema.json`: schema `181ba4b8a1e7...` (before `7100f8436556...`:
one class more, `DealDamageToEachTarget`, one field, one `Event` member). `powerlsv.ir.json`'s 546
cards are byte for byte the cube-only export's (the card list and order unchanged; only
`schema_sha256` differs); `card_ir/src/generated.rs` regenerated (25 lines). The lowering's
exhaustive destructuring made the new field a compile error until it was lowered.

### 54.3 The rules

| Text | Rule | How |
|---|---|---|
| Delve on a creature spell | 702.66 | the existing delve (18, 51) of any spell with `CardDef::delve`: at most the generic part of the total cost, cards only, exiled after the mana abilities |
| "enters with a +1/+1 counter on it for each instant and sorcery card exiled with it" | 607.2q (the cards exiled to pay the cost of the spell that became the permanent: linked to an ability *printed on it* that exiles cards while paying), 614.1c | `CardDef::counters_per_exiled` (a filter on a card's own characteristics, `deck_filter_ok`). As delve exiles a card for such a spell, `Mark::ExiledToPay(card)` goes on the spell (`record_exiled_to_pay`: graveyard picks and delve's mana sources alike); as the spell resolves into a permanent, the marked cards still in exile as the same objects (400.7) that match the filter are counted (`exiled_to_pay_count`) and it enters with that many (`Game::entering_exiled`, `entering`). Not cast (reanimated, put onto the battlefield): none. A copy of the spell: none (707.10: no cost was paid for it). A card with its own escape (printed too) would be a blocker, not a silent zero |
| "Whenever an instant or sorcery card leaves your graveyard, put a +1/+1 counter on this creature" | 603.10a (abilities that trigger when a card leaves a graveyard look back in time), 603.2 | `EventKind::LeavesGraveyard` (subjects: the owner, the card), recorded in `move_card` before the card moves, whatever zone it goes to (exile, hand, library, stack, battlefield), cards only (108.2), and only in games with such a card (`SEEN_GY_LEAVE`); a `Subject` predicate on it reads the card in the graveyard (`card_matches`), where other events read permanents and spells only. One trigger per card (not "one or more") |
| "deals 1 damage to each of one or two targets" | 115.3 (distinct), 608.2b | `DealDamageToEachTarget` lowers to `Op::DealDamage` over one requirement of 1..2 |

Readings taken (the ambiguities resolved):

* **A Regent's own delve doesn't trigger it**: the cards leave the graveyard while it is a spell
  (601.2h); its triggered ability works on the battlefield (113.6). A second Regent's delve triggers
  one already on the battlefield once per instant or sorcery card, the triggers going on the stack
  above the second Regent as it becomes cast (603.3).
* **"Exiled with it" is the spell's delve only**: Underworld Breach's escape (an alternative cost
  granted to the card, not printed on it) exiles cards to pay its cost too, but 607.2q links only an
  ability printed on the permanent spell, so those cards don't count (tested). Instants exiled by
  another Regent's delve are in exile but weren't exiled with this one (tested).
* **A card that leaves exile before the Regent resolves** isn't counted (a new object, 400.7); no card
  of the mirror can do that.
* **"Your graveyard"** is the graveyard of the trigger's controller: a card is in its owner's
  graveyard (404.2), so the owner is compared with the controller (a stolen Regent would count its
  new controller's).
* **"Instant and sorcery card"** reads each card's printed types (it is in exile, or in the
  graveyard): `HasPrintedCardType`, as the cube's graveyard filters.

### 54.4 Constructed decks and the spec

`"constructed": true` (read before the other keys: it picks the card table the names are looked up
in) needs explicit decklists (`decks: {"p0": [...], "p1": [...]}`, sideboards optional) and checks CR
100.2a and 100.4a (`az::constructed::check`): at least 60 cards (`MIN_DECK`; more is legal), at most
four of a card other than basic lands (supertype basic and a land) over the deck and the sideboard
together (`MAX_COPIES`), a sideboard of at most 15 (`MAX_SIDEBOARD`), cards only (no token, no face),
and **at most 64 cards** (`MAX_DECK`, the engine's limit, not the CR's: every card of a deck may end up
in one zone, and a hand, a library list and a graveyard have 64 action slots each, section 11:
`act::HAND_N`, `LIBRARY_N`, `GRAVEYARD_N`, the bound derived from them; a 65-card deck is refused at
the spec with that reason rather than reaching an out-of-range slot mid-game).
Best-of-three is an error (constructed sideboarding is not built, 54.8), so are pools and random,
evaluation and drafted decks (limited's). Without the key every spec is what it was: the cube's card
table (in which "Murktide Regent" is an unknown card) and limited's 40 (section 42), untouched. The
evaluation spec of a decklist spec is the spec (`eval_json`, `mtgz.az.evaldecks.eval_spec`), so the
yardstick, random and Elo games play the mirror too.

**60-card games needed nothing new**: `Game::with_sideboards` took decks of any size (the card slots
are 0..59 and 60..119); the play-or-draw choice (103.1), the London mulligan (103.5) and the turn cap
(a draw after `max_turns`) are tested with the 60s (`constructed.rs`); the action ranges fit (a
graveyard holds at most its owner's 60 cards, under the 64 `GRAVEYARD` slots; the library, hand and
pile lists under 64); the encoding's library size / 40 squashes as it did.

### 54.5 Hidden information: open decklists

Best-of-one keeps open decklists (section 10): each player knows both 75s, the cards in play are the
60s; hidden are the hands, library order and face-down cards. `Game::determinize` deals the
opponent's hidden cards again from their own cards, so in every world the opponent's 60 card slots
hold exactly their decklist, the multiset of their hidden cards is the decklist minus every card of
theirs the viewer has seen (public zones, cards known exactly, clouds), what the viewer knows stays,
and its encoding and legal actions don't change. Tested on 408 worlds of 12 heuristic games
(`determinize_samples_the_opponents_unseen_cards_from_their_decklist`: the opponent's hand differs
from the real one in 262) and by `FUZZ_POOL=delver`'s checks (54.7). Sideboards stay outside the game
(the preset passes none).

### 54.6 The preset and its measurements

**`delver-mirror`** (`mtgz/az/__init__.py`) is `mtg-3` (22.8: the last best-of-one run) on the
mirror's spec (`decklists.mirror_spec`: `{"max_turns": 60, "mulligans": true, "constructed": true,
"decks": {"p0": <the 60>, "p1": <the 60>}}`, no sideboards), with `eval_spec` the same spec: the token
network 128 x 4 (4 heads, feed-forward 256) on layout v4, CUDA inference (one worker, 2 groups, 8
search threads) and training, 768 games in flight, 600 games an iteration, 64 / 16 simulations
(`p_full` 0.25), mana payments at 4 (`sims_minor`), completed-Q targets, value_q 0.9, lr 1e-3, batch
512, reuse 2, window 6 (growing), a 12 GB window budget, `max_steps` 20,000; the yardsticks the M7c
heuristic (21.4: the Elo anchor, 1000) playing the same deck, 100 games at 32 simulations, and random,
50; Elo against the 2 previous checkpoints, 20 games each, in one driver. **`delver-smoke`**:
`mtg-smoke` on that spec (CPU, a 32 x 1 network, 3 tiny iterations).

Measured on the desktop (i9-13900K; shared: until 13:00 with td1 on the GPU and its CPU, then with
the user's game and other sessions), 2026-10-28.

**The engine and the heuristic** (`cargo test --release -p engine --test measure_delver -- --ignored
--nocapture --test-threads=1`, 2,000 games per pairing, one thread; the seed's chooser decides, the
heuristic always plays first, 35.4; decisions = decisions with two options or more):

| Pairing | Turns / game | Decisions / game | Hands mulliganed | First player's score | Draws | Games / s | Decisions / s |
|---|---:|---:|---:|---:|---:|---:|---:|
| heuristic vs heuristic | 15.8 | 299.1 | 29.0% | **48.6% +- 1.1** | 0 | 689 | 206k |
| heuristic vs random (heuristic seat 0: 95.7%) | 16.0 | 206.8 | 40.7% | 72.5% | 0 | 966 | 200k |
| random vs heuristic (heuristic seat 1: 95.0%) | 15.9 | 203.3 | 40.7% | 73.3% | 0 | 1,053 | 214k |
| random vs random | 26.8 | 183.0 | 51.5% | 48.3% | 3 (the turn cap) | 819 | 150k |

The cube's heuristic-vs-heuristic games (21.4, 438-card pool) were 14.4 turns and 186 decisions: the
mirror's are a little longer and have 60% more decisions (cantrips, fetch lands, Force of Will and
Daze, Brainstorm's put-backs, delve). The heuristic beats random 95.0-95.7% in the mirror (95.8% on
the cube's decks). The first player's 48.6% is a sanity number of the yardstick, not of the matchup:
the heuristic mulligans any hand outside 2-5 lands (29% of its hands, one mulligan at most) and holds
no mana for instants (21.4).

**Search** (`python tools/search_smoke.py --specs delver --seeds 0-3 --preset delver-mirror`: the
preset's self-play seat, a uniform evaluator, 8 threads, 256 games in flight, 400 games per seed):

| | per seed | per game |
|---|---|---|
| decisions searched | 114,339-116,708 | 289 |
| simulations | 3.15-3.23 million | 8,000 (27.6 per decision: 64 x 0.25 + 16 x 0.75, payments at 4) |
| time | 62.6-94.7 s | |
| throughput (search + engine, no network) | 1,220-1,860 decisions/s, 34-51k simulations/s | |

(The same smoke's cube games, seed 0 random: 404 decisions and 10,400 simulations per game.) An
untrained network plays like the uniform evaluator; trained, games shorten (mtg3's searched decisions
per game were 143-187 in its first iterations, ~140 from iteration 10 on).

**With the network** (`python -m mtgz.az.bench selfplay`, the preset's network untrained, 1.5 million
parameters, 64 / 16 simulations):

* CPU inference (8 torch threads, 32 games in flight, batches of 8.8): 889 leaves/s, **486 games per
  hour**: CPU inference is not an option at this size.
* CUDA beside td1 (td1 at 87-91% GPU): 128 games at 128 in flight did not finish in 10 minutes (under
  ~1.7k leaves/s); stopped there to leave td1 its GPU. The GPU alone was not measured (td1, then the
  user's game held it). **Estimate on the GPU alone**: the same network on layout v4 at 768 in flight
  evaluates 10.2-10.5k leaves/s on this GPU (td1's play network, 52.5; mtg3's 12.4k on layout v3), so
  at 8,000 simulations a game **~4,600-5,600 games per hour**, an iteration's 600 games in **~6.5-8
  minutes** of self-play, less as games shorten.

**A projected iteration on the GPU alone** (mtg3's phases scaled by the mirror's 289 against mtg3's
~190 searched decisions per game early on, x 1.5): self-play 6.5-8 min, training ~0.5 min, the
yardsticks 4.5-6.5 min, Elo 6-10 min: **~18-25 minutes**, 40 iterations in ~12-17 hours.

### 54.7 Tests, fuzzing, smoke, digests

* **Rust** (`cargo test --release -p engine -p card_ir --no-fail-fast`, every binary including `az`;
  on the Mac at 02a10e4, bit-identical to the desktop, section 44): **1,432 passed, 0 failed, 14
  ignored** (and after the review's additions, 54.10: 1,439). New: `cards_legacy.rs` 24 (17 and the
  review's 7), `constructed.rs` 7, `az.rs` 1 (15 there), `measure_delver.rs` 1 ignored. (A desktop run before that had one failure, `the_heuristic_plays_the_mirror`'s floor of 17
  random losses in 20 games, too tight for 20 games: 16 once Murktide's trigger worked; the floor is 13
  now, 54.6 has the 2,000-game score.) `python -m pytest tools/codegen` (generated.rs current): pass.
* **Behaviour digests** (`cargo test --release -p engine --test digest -- --ignored --nocapture`):
  combined **804002969bdd0938 before (b11952b) and after** (every one of the 2,500 games equal to
  `tests/data/digests.txt`): the cube's card table, its generators and its games are untouched.
* **`FUZZ_POOL=delver FUZZ_GAMES=20000`** (on the Mac, `tools/mac.sh`, at 0fae496): no failure, 154.7
  s; 20,000 games of the two 60s (wins 9,923 / 10,067, 10 drawn at the turn cap), 26.6 turns and
  181.7 decisions per game, the longest game 612 actions (the cap is 100,000), 4,399,041 options
  completed (5.0 million decisions after them); **Murktide Regent cast 16,428 times, its
  leave-the-graveyard trigger put on the stack 1,421 times, Prismari Charm cast 11,765 times** (the
  mode asserts all three in 200 games or more). Every check of the default mode: invariants, clones,
  `determinize` for both viewers (view, known cards, clouds, information sets), the encoding finite
  and bounded, every option completed in the game and in the sampled worlds.
* **The default fuzz** (`FUZZ_GAMES=20000 FUZZ_MATCHES=2000`, the Mac at 0fae496): no failure, 226 s;
  20,000 games (wins 9,993 / 9,970, 37 draws), 214.0 decisions per game, 5,651,735 options completed,
  2,000 matches (5,138 games, 1,138 of three, 1,491 drawn), 47,803 sideboarding decisions, 42,768
  hidden-pool worlds: **every number main's of 51.8**, the same games (the cube's fuzz pool is the cube
  IR's and the custom test cards').
* **The search smoke**: `--specs delver --seeds 0-3 --preset delver-mirror` 0 failed of 4 (1,600 games,
  462,661 decisions, 12.8 million simulations, 333 s); the default (`python tools/search_smoke.py`:
  `random,eval,match`, seeds 0-3, the mtg-3 seat) 0 failed of 12 (4,800 games and matches: random
  658,117 decisions and 16.9 million simulations, eval 704,510 and 18.0 million, match 1,515,641 and
  40.2 million; 1,373 s with the desktop shared).
* **The run loop** (`python -m mtgz.az.run --run E:/mtg-zero-scratch/delver-d1/smoke --preset
  delver-smoke`, CPU, 24 s): three tiny iterations of the mirror end to end, self-play (16 games,
  239-251 decisions and ~60 samples per game, 67 KB per sample), training, the heuristic (1-2 of 8)
  and random yardsticks, Elo; no failure. (A scratch directory, not a run.)
* **pytest** (`bash tools/build_py.sh`, `python -m pytest -q tests`): 45 passed (5 new in
  `tests/test_delver.py`).
* **Rulings**: `data/rulings/legacy.rulings.json` (`tools/fetch_rulings.py --out`): Murktide Regent's
  three (two new texts, `87078369` mana value 7 and `05ce86bd` at most five cards, triaged a/mana; the
  third, `0a2cf1a9`, the cube's delve cards' too), Prismari Charm none; all three tested
  (`cards_legacy.rs`); `docs/rulings.md` regenerated (1,610 rulings, 1,228 class a, every one tested).
* **mtg-bot** (`feature/legacy-delver-ir`): its new and touched tests pass (`test_legacy_creatures.py`
  15, `test_card_ir.py` with the legacy pool, `test_pools.py`, `test_fetch_cube.py`: 63 + 54); the full
  suite (`pytest -n 6 --no-cov`): 21,089 passed, 52 failed, of which 50 fail at the branch point
  62210e89 too (Rust-engine parity and coverage-doc staleness tests) and 2 were new: the pool
  registry's shape (fixed: `legacy` registered) and a spacecraft test that passes alone (an xdist
  ordering flake).

### 54.8 Limits, and what best-of-three needs

* **The five sideboard cards outside the cube are not supported**: Consign to Memory, Hydroblast,
  Meltdown, Price of Progress, Rough // Tumble (`Decklist::unsupported_sideboard`; the deck file keeps
  the full 75). mtg-bot's parser already claims four of them in its roto pool (all but Rough //
  Tumble); each needs its IR appended to the legacy pool and registry (identities 548..), its
  lowering and its tests: Consign to Memory (replicate, 26.5; "counter target triggered ability or
  colorless spell": countering abilities, 27.2, and a colorless spell filter), Hydroblast (Pyroblast's
  shape with red: the target any spell or permanent, the colour checked on resolution), Meltdown ({X}:
  "destroy each artifact with mana value X or less", 30), Price of Progress (damage to each player by
  the nonbasic lands they control, counted on resolution), Rough // Tumble (a split card, 25 and 709:
  2 damage to each creature without flying, 6 to each with flying).
* **Constructed best-of-three** (`"constructed"` with `"format": "bo3"` is refused) needs: sideboarding
  bounds of constructed play in `MatchPool::new` (today a spec decklist keeps its size exactly, 42.2):
  a deck of 60..=75 cards from the 75 (at least 60, 100.2a; the sideboard the rest, at most 15, 100.4a;
  the four-card limit holds by construction since the pool is the legal 75); the pool list fits (29
  distinct names of 64 `LIBRARY` entries; counts at most 4 of 31); the information model (36.11) with
  open 75s: no first picks, both pools known, the opponent's chosen deck hidden between games
  (`sample.rs` dealing a deck of the public size from the known 75, its knowledge kept); the
  heuristic keeps its deck (`PASS`); the fuzzer's matches and the search smoke's match spec for the
  mirror; the five cards above for the sideboard to mean anything.
* **The yardstick's limits in this deck** (54.6): the heuristic mulligans by land count only, always
  plays first, holds no mana for instants.
* **Not measured**: the mirror on the GPU alone (54.6's estimate); the speed of the engine change
  (`tests/perf.rs`): the cube's hot paths only gained a `seen` test in `move_card` and a match arm in
  `TrigPred::Subject`.
* **At most 64 cards in a deck** (54.4: the action slots), and the cube list at 546 (54.1: the legacy
  identities start there).
* No rules shortcut was added (docs/shortcuts.md unchanged).

### 54.9 A run configuration for `runs/delver1`

(At ~60 samples per game, 54.7's run-loop check, an iteration is ~36k samples, ~2.4 GB: the 12 GB
window budget holds five iterations of the window's six; mtg3 had ~19k samples an iteration.)


**`delver-mirror` as it is, on the GPU, when td1 is not using it** (td1 was paused on 10-28 during
D1). From a pinned worktree of the merged commit (21.5): `bash tools/build_py.sh`, the gate
`python tools/search_smoke.py --specs random,eval,match,delver --preset delver-mirror`, then
`python tools/launch_hidden.py runs/delver1/supervisor_console.txt python tools/supervise.py
runs/delver1 --preset delver-mirror`. Expected ~18-25 minutes an iteration (54.6), a fresh 128 x 4
network (no checkpoint of the cube's runs: they never saw the legacy identities 546-547, and their
priors are the cube's).

* **Beside td1 on the GPU: not recommended**: a mirror self-play bench got under 1.7k leaves/s next
  to td1 (54.6), ~6 times slower, and would slow td1 as much. If both must run, give the mirror the
  Mac: `selfplay_remote='mac'` (section 52: two workers reached 31.1k leaves/s with td1's 128 x 4
  play network, 3 times the desktop) while td1 doesn't use it (td1's remote self-play and the mirror's
  can't share the Mac's cores well, 52.8), the desktop keeping training and evaluation; and
  `eval_every` 2 (mtg4's choice, 22.8) to halve the evaluation's share.
* Keep mtg-3's search and network settings for the first run: they were the last best-of-one run that
  improved steadily (22.8); the one matchup may need fewer games per iteration to move (300 would
  halve the self-play phase), a choice to make after a few iterations' metrics.
