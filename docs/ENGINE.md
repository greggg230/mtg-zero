# The MTG Zero rules engine (`engine` crate)

Status: M8d part 2 (interaction confidence: the riskiest rulings left and the interaction matrix's cells tested, a combo-dense fuzz mode,
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
| `tests/support.rs` | the support report and its threat view, the exact supported list, loud failures (on an edited IR since every cube card is supported), the IR consistency checks of section 32; `SUPPORT_DEFS=<file>` dumps every lowered definition (`tools/diff_defs.py` compares two dumps by name) |
| `tests/bots.rs` | the heuristic bot (21.4): every answer legal and every game ended (against random, `HeuristicV1` and itself, and half-random play; `--ignored`: 8,000 games with every decision asked, all 39 decision kinds answered; M7e's two kinds have their own answers, not measured that way), the registered `heuristic` beats random, the policy in set situations (a loyalty minus for a creature, else the plus that is not mana; the ultimate; no minus at its own permanents; a fetch land first; equip once, at the creature that attacks; crew with a creature that would not attack; no loop with Basalt Monolith and Sensei's Divining Top; Mind Stone only when flooded, at the opponent's end step; the largest X); `--ignored --nocapture --test-threads=1`: strength against `HeuristicV1` and random by deck subsets, speed, activation counts, a trace (`BOTS_TRACE_SEED`), the cube's activated abilities, the X cards |
| `tests/az.rs` | the zero-core game `mtg` (section 21.8): games end with sane results, the turn cap, clones, `determinize` keeps features and legal actions for both viewers, equal keys mean equal features, `force_chance` replays and stays aligned, encoding slots, decks, spec parsing, heuristic vs random (`AZ_SCALE=k` for more games) |
| `tests/rules_m8b.rs` | M8b part a (section 36): best-of-one is the game of before action for action; the spec's `format` and `pools`; CR 103.1 in later games (the loser chooses; after a drawn game the previous chooser); drawn games count and a match can be drawn; the match ends at two wins or after three games, its result and margin; sideboarding reaches every legal deck exactly once (an exhaustive search of a small list, 100.2b; decks of 5-7 cards and of exactly 6, `PASS` only at a legal size); a pool's deck is sideboarded to exactly 40 cards (section 42: every 40-card deck of generated, evaluation and M8a pools by its canonical path, the two extremes, no 39- or 41-card deck, `PASS` only at 40, the heuristic's answers legal from any point); a spec decklist of another size keeps its size; sideboarded decks are legal (40 cards) and are the next game's cards and sideboard; the heuristic keeps its deck; sideboarding hidden from the other player; `determinize` keeps the decider's actions and encoding in matches; the match block and bags; pools and the deckbuilder |
| `tests/rules_m8b_hidden.rs` | M8b part b (section 36.11): the hidden pool: the opponent's first picks drawn from the prior (never known cards, never the viewer's) and seen ones kept; sampled decks with the public sizes (40 cards, the sampled pool's bounds 40..=40: section 42) holding every card the viewer knows (in games and between games); a deck of 40 basic lands with 31 of a type sampled; the second sideboarder's worlds independent of the first's choice; the companion question asked of every player in best-of-three; the bags (`OPP_POOL`, `OPP_HIDDEN`) and every feature the same in the viewer's worlds; the heuristic's choices the same in its worlds; `force_chance` in matches |
| `tests/rulings_*.rs` | M8d (section 37): one test per official ruling of a cube card, the ruling named in the test (`// ruling: <hash8>`; `docs/rulings.md` tracks every ruling, built by `tools/rulings_doc.py`), the CR in its name. `rulings_copies.rs` (Phyrexian Metamorph, Superior Spider-Man, Saheeli, Shifting Woodland: 707.2, 707.9b, 107.3m, 707.4, 613.1, 302.6, 704.5n), `rulings_replacement.rs` (Blightsteel, Dauthi Voidwalker, Yawgmoth's Will, Hullbreacher, Quantum Riddler, Containment Priest, Suplex: 614, 616.1, 702.90, 700.4), `rulings_protection.rs` (Emrakul, True-Name Nemesis, Mother and Giver of Runes: 702.16, 113.6, 101.2, 702.86, 608.2b), `rulings_stack.rs` (Tidebinder, a countered delayed trigger, spells that can't be countered against Remand, Mana Drain, Subtlety, storm, Lutri's and Chain Lightning's copies, checks on resolution, Teferi, ninjutsu), `rulings_facedown.rs` (manifest dread, Headliner Scarlett: 701.40, 701.62, 708.8), `rulings_faces.rs` (Sink into Stupor and Aang against "can't be countered", a modal double-faced card from the graveyard, daybound, Reflection of Kiki-Jiki's copies, Adventures), `rulings_triggers.rs` (sources gone before their enter abilities resolve, 610.3c, 603.6; Sensei's Top; Goryo's Vengeance; Chandra's emblem; Mana Drain's next main phase; Dack's control), `rulings_walkers.rs` (606.3, 602.2b, Kaito 205.1a), `rulings_mana.rs` (601.2f with alternative costs, 601.2g-h, 119.4, 107.3b, delve, Goblin Engineer); shared helpers in `tests/rulings_util/`. Part 2 (section 38): `rulings_stack2.rs` (divided damage with a target gone: Fury, Pyrokinesis, Fire Covenant, Shatterskull Smashing; Invert Polarity; Lutri's copies of modal, spree, X and divided spells: 601.2d, 608.2b, 115.7, 707.10), `rulings_casts.rs` (Etali's free casts, Bolas's Citadel, Dauthi and a discard, converge on copies and free casts: 608.2g, 107.3b, 118.9a, 601.2f, 614.1a), `rulings_copies2.rs` (Reflection's copies of copies and tokens, the prepare card: 707.9b, 722.3), `rulings_triggers2.rs` (sources gone, Green Sun's Zenith stolen, exile-until one card at a time, magecraft on copies, Leovold, Screaming Nemesis, Coveted Jewel, City of Traitors, flashback exile, library positions: 610.3c, 603.2c, 603.2e, 702.85a, 401.4). Part 3 (section 39): `rulings_replacement2.rs` (ravenous, reconfigure, Archon of Emeria, Elspeth's doubling, an Aura entering without being cast, Narset and Leovold with Hullbreacher, finality counters, bestow: 614, 616.1, 303.4f), `rulings_copies3.rs` (CDAs in every zone, copies of kicked, saddled and dashed objects, token copies' copiable values, Vehicles made creatures, backup, earthbend: 707.2, 707.9, 604.3, 613.4b), `rulings_stack3.rs` (targets gone, priority around resolutions and casts, uncounterable spells, casts during a resolution, storm and replicate, special actions, splice, Bolas's Citadel's top card: 608.2b, 117.3b, 601.2), `rulings_triggers3.rs` (reflexive triggers per token, last known information, delayed returns, cast triggers, draws over a turn, trigger order, intervening ifs, Nadu's new object: 603.12, 608.2h, 603.3b, 603.4, 400.7), `rulings_protection2.rs` (effects that don't target, a reanimated True-Name Nemesis and its Aura, protection from everything, modes needing a target, two wards: 702.16b, 303.4d, 700.2a, 702.21a), `rulings_faces2.rs` (DFC and MDFC characteristics in and out of play, Adventures' characteristics, copies and permissions with Bilbo and Robber of the Rich, preparation cards: 712.8, 712.11, 712.14, 715.3-715.4, 722), `rulings_walkers2.rs` (loyalty abilities after entering mid-combat, Comet's dice: 606.3). Part 4 (section 40): `rulings_zones2.rs` (exile and return as a new object, cards played from exile or a graveyard with their timing, "cast" and lands, escape and retrace again, empty libraries, dual lands' types: 400.7, 601.3, 305.1, 702.138, 702.81, 205.3i), `rulings_combat.rs` (creatures put onto the battlefield attacking, ninjutsu, summoning sickness of lands and Vehicles made creatures, blocks that stand, exert: 508.4, 702.49, 302.6, 509.1h, 701.39b), `rulings_mana2.rs` (mana value under every cost, X outside the stack and in free casts, alternative costs with permissions, flashback, escape, devotion, domain, converge, mana abilities, emerge (also with retrace, 40.9), waterbend, cascade into a split card: 202.3, 107.3b, 118.9, 702.34a, 702.138, 700.5, 106.6, 709.3, 702.119a), `rulings_other.rs` (Ajani's -4, reconfigure, Equipment, control changes, Balance, extra turns, connive and explore, colorless emblems, durations: 701.21a, 702.151b, 301.5, 500.7, 701.50) |
| `tests/interactions_*.rs` | M8d part 2 (section 38, `docs/interactions.md`): pairs of mechanics written in different milestones. `interactions_copies.rs` (copies against layers, face-down permanents, replacement effects on entering, "as this enters" choices, ward: 707.2, 708.2, 614.12, 616.1, 707.5, 702.21a), `interactions_costs.rs` (Bolas's Citadel's life with spree and kicker costs, Thalia's tax on free and Citadel casts, a Lotus Petal under Yawgmoth's Will: 601.2f-h, 614.1a), `interactions_faces.rs` (a split card with two graveyard permissions: 709.3, 601.2b), `interactions_zones.rs` (flicker of face-down, transformed and reanimated permanents and of Leyline Binding, control exchange and summoning sickness, Fractured Identity on a planeswalker, trample over protection: 400.7, 304.4, 610.3, 302.6, 306.5b, 702.19c). Part 3 (section 39.5): the last two cells, a token copy of a nightbound permanent (`interactions_copies.rs`: 707.8a, 702.145f) and True-Name Nemesis's chosen player after Oko's exchange (`interactions_zones.rs`: 702.16j, 110.2) |
| `tests/fuzz.rs` | random play (below), option completion (below) and replay determinism; M8b: random best-of-three matches and their replay, `determinize` for both viewers with the hidden pool's checks; M8d part 2: `FUZZ_POOL=interactions`, combo-dense decks from interaction groups (section 38.5); M6 part 3: `FUZZ_POOL=drafted`, matches of random and `consensus`-drafted 4v4 and 2v2 events as their `mtg` specs (first picks, what each player knows elsewhere), and in every mode a sampled first pick is a candidate of the prior (section 45); M6: every decider's encoding finite and within `FEATURE_BOUND` (games, determinized worlds, matches) and `FUZZ_POOL=exponential`, exponential boards to the turn cap (section 46.5) |
| `tests/draft.rs` | the zero-core game `team_draft` (section 41): packs seeded, disjoint and full (the 541 cards drafted or undrafted), seats alternating and every pass to an opponent (round 1 to the right), picks are the pack and illegal actions errors, every pool 45 cards and every deck legal (exactly 40 cards, section 42; at most 31 of a basic, deck + sideboard = the pool; `consensus` builds `pools::build_deck`'s deck), both extremes reachable (40 basic lands, 31 of a type; 40 pool cards with the rest of the pool forced out and every count forced to 0; a card into a full deck an error), every 40-card deck of a two-card pool reachable exactly once (487,676 decks), the Latin-square pairings (12, 4, 1 and 9 matches; three different opponents), the first-picks rule (after the draft a team knows every cell but the opposing openers' first picks), `determinize` keeping the viewer's knowledge, encoding and the decider's options (for the decider and another viewer), equal keys meaning equal features, keys masked for the observer, clones, `force_chance` replaying openings and impossible keys falling back in order, heuristic-played matches resolving 1v1, 2v2 and 4v4 events, the result refused while the matches are pending, `resolve`'s checks, the payload, the encoding's slots, the bots, the spec, the layout file checked in (`MTGZ_WRITE_LAYOUT=1`), the random-actions fuzz (`DRAFT_FUZZ=n`; M6: the decider's encoding and every world's finite and within `FEATURE_BOUND`), measurements (`--ignored`: `measure`); `tests/test_team_draft.py` (pytest): the game through the bindings, the loud failure at the deferred terminal, the token network on layout `draft_v1` |
| `tests/draft_matches.rs` | drafted matches (section 45): the `mtg` spec of an event's match (`pN_first_picks`, `pN_elsewhere`, `seed`) plays the event's match move for move (`match_game`; 32 matches: 4v4 and 2v2, random and `consensus`-built decks, best-of-three and best-of-one), the first-pick prior is exactly the cards the viewer's team never saw (193 in 4v4, 367 in 2v2; the real first picks among them), `determinize` samples first picks from it (never the viewer's knowledge) and keeps the viewer's encoding, the spec refuses contradicting knowledge, specs without the keys are what they were; a position from a draft history is seat 0's information set (its knowledge and encoding the real event's) and a history that does not fit is an error; deckbuilding's simulations divisor; `tests/test_drafted.py` (pytest): the glue (payload -> specs, results -> `resolve`, a malformed result refused), events parked by a draft driver and resolved through a heuristic match driver, the token network's actions by identity on `draft_v1`, the human-pick yardstick on a few drafts, the co-training presets |
| `tests/draft_deferred.rs` | `team_draft` through zero-core's deferred hook (section 43): 2v2 and 1v1 events through the self-play driver with a stand-in network (deferred terminals evaluated as leaves of no options, every event parked, a malformed result refused, resolved with heuristic-played matches, samples with the real outcome and margin), the hooks mapping the inherent methods (1v1-4v4); `--ignored`: `measure_parked_memory` |
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
first turn (default: chosen at random each game, CR 103.1). M8b (section 36): `"format": "bo1"`
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
  priority too, for its draw.
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
of cube cards (`GROUPS`), one per family of the matrix's cells: copies and enter/leave
abilities; flicker and linked exile; reanimation and replacement effects; face-down cards;
double-faced cards, Adventures and split cards; counterspells and copies of spells; control
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
token network reads the layout on real positions and answers a driver's leaf batch).

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
commit). End to end on the desktop: `python -m mtgz.az.run --run <scratch>/e2e-mac --preset mtg-smoke
--eval-remote mac` (26 s): the preflight passed, iterations 0-3 were evaluated on the Mac (3.5-5.1 s
each, shipping included), `evals.jsonl` got their rows (host mac: vs the heuristic, vs random, Elo),
`metrics.jsonl` has none, `zero.dashboard._metrics` merges them into the four rows, `elo/` and
`models/index.json` came back. Refusals checked by hand: a missing Mac checkout stops the run at
start with the command to run; an unreachable host warns and goes on; the remote shim refuses a
checkout at another commit and an unset commit.

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

* zero-core's co-training (`zero/cotrain.py`, core e6166c3, M6 part 2) refuses `eval_remote` for now
  ("eval_remote is not supported in co-training runs"): co-trained team-draft runs evaluate on the
  desktop until the core supports it.
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
| play network (`resolver_cfg`) | mtg-4 (64 / 16, minor 4, 128 x 4, cq, value_q 0.9), evaluated every 2nd iteration on the fixed evaluation pools; `resolver_init` the mtg5 checkpoint chosen at launch | mtg-4, 96 in flight, small evaluations | mtg-smoke, best-of-three, tokens 32 x 1, minor 2 |
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
parked records ~5.4 MB per event, ~350 MB for 64 events waiting.

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
  (45.10). Not changed (the user's "mtg-4's settings"); an option for the launch (45.12).
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

* **The play network's start**: runs/mtg5's iteration 10 (the copy used here) lost 0/8 to the
  heuristic and won 3/4 against random at 32 simulations; the matches of the first draft
  iterations reward what a weak player can pilot (DESIGN.md 2.2's stated risk). The checkpoint
  is the user's choice at launch.
* **Iteration time** (45.10): the projection is ~42-91 min per iteration (~65 on average) with
  mtg-4's evaluation of the play network every 2nd iteration, against 2.3.9's 35-40 min. Options: the play network's
  `eval_every` 4 (`--resolver-cfg '{"preset": "mtg-4", "eval_every": 4}'`), fewer Elo games, or
  `elo_sims` lower; the draft side is cheap.
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
