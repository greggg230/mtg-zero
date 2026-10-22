# The MTG Zero rules engine (`engine` crate)

Status: M7a (threats first: layers and continuous effects, counters, generic triggers,
protection and combat restrictions, alternative costs, X, kicker, delve, casting from
graveyards and exile, linked exile, delayed and reflexive triggers; 340 supported cube cards;
section 23), 2026-10-22; M5b a token network and stable card identities (section 22); M5a
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
| `lower/` | IR -> `CardDef`: `printing.rs` (every `CardPrinting` field), `activated.rs` (activated and mana abilities, costs), `effect.rs` (effects -> ops), `filter.rs` (predicates, card filters, conditions), `support.rs` (the per-card report) |
| `program.rs` | `Op` (the effect instruction set), `Amount`, `Who`, the program builder |
| `mana.rs` | `ManaCost` (generic, coloured, hybrid, Phyrexian), `ManaPool`, colour masks, the matching (`payable`) and budgets |
| `game.rs` | `Game` (the state), `Obj`, `Player`, `GameSpec`, setup, zone moves |
| `flow.rs` | the agenda (task stack) runner: turn structure, priority, steps |
| `decision.rs` | `Decision`, `Choice`, `DecisionKind`, the action-id layout |
| `cast.rs` | casting (CR 601), additional costs, targets and filters, priority options |
| `activate.rs` | activated abilities (CR 602): options, activation, non-mana costs, free-standing mana abilities, conditions, "as enters" choices (section 18) |
| `pay.rs` | paying mana (601.2g-h, 605): mana sources, feasibility, `PayMana` (section 18) |
| `resolve.rs` | resolution (CR 608) and the effect VM |
| `combat.rs` | CR 506-511 |
| `sba.rs` | CR 704 |
| `triggers.rs` | events, trigger matching, putting triggers on the stack (CR 603) |
| `hidden.rs` | knowledge (exact bits and partial "clouds"), chance keys, `determinize` (section 10) |
| `rng.rs` | xoshiro256++ / splitmix64 (the same generators as zero-core) |
| `testkit.rs` | scenario builder and helpers for tests (public; not used by the engine) |
| `az/` | the zero-core game `mtg` (section 21): `mod.rs` (`MtgAz`, the spec, the `SearchGame` impl), `encode.rs`, `decks.rs`, `bots.rs` |

No dependency beyond `card_ir`, `serde`, `serde_json`, and zero-core's `zero_search` (the
`SearchGame` trait, for `az/` only; the pinned submodule `third_party/zero-core`). No `HashMap` inside `Game` (iteration
order must be deterministic); no threads, callbacks or interior mutability.

## 2. Objects, zones, players and ids

* `PlayerId = u8` (0, 1). Two players only for now; nothing below assumes more except the
  places marked "2p" (APNAP order, "the opponent").
* **`CardIdx(u16)`** indexes `Game.objs`, one slot per physical card for the whole match
  (cards 0..39 belong to player 0's deck, 40..79 to player 1's). Tokens append slots (spell
  copies will too); a slot is never reused, so `CardIdx` is stable. A token that ceased to
  exist (CR 704.5d) keeps its slot in `Zone::Nowhere`, in no zone list.
* **`ObjRef { idx, inc }`** names an *object* (CR 109). Each zone change bumps the slot's
  incarnation `inc` (CR 400.7: the object that moved is a new object), so a reference held by
  a target, a trigger or an effect goes stale automatically. `game.live(r)` is the one check.
* `Obj` (per slot): `def` (the `CardDef` id), `owner`, `controller`, `zone`, `inc`,
  `timestamp` (CR 613.7, refreshed on entering the battlefield), `tapped`, `damage`,
  `deathtouched` (CR 704.5h), `control_since` (turn number; summoning sickness, CR 302.6),
  combat flags (`attacking`, `blocking: Option<CardIdx>`, `blocked`, `struck_first`), `known`
  (bit p = player p knows the card's identity where it is; meaningful in hidden zones, section
  10).
  Per-object state that later features add (counters, attachments, face-down, modifiers)
  goes here too, and must reset in `move_card` when the object changes zones.
* Zones: per player `library` (index 0 = bottom, last = top), `hand`, `graveyard` (last =
  top); shared `battlefield` (always in timestamp order = the canonical permanent order),
  `exile`, `stack: Vec<StackItem>` (last = top). `Obj.zone` mirrors membership; the fuzzer
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
* Mana abilities (of permanents and of cards in hand) are activated only while paying a cost
  (601.2g / 605.3a: also a payment during resolution, "counter unless its controller pays"),
  never as a free-standing priority action: floating mana with nothing to spend it on cannot
  matter. The exception is abilities that may be activated "only as an instant" (Lion's Eye
  Diamond: never while paying). Mana already in a pool (Dark Ritual) is spent automatically
  and first when a cost is paid (it empties at the end of the step anyway). Recorded
  simplification, section 13.
* Game-length caps: `GameSpec.max_turns` (default 60) ends the game as a draw with reason
  `TurnLimit`; an internal step budget turns an engine loop into a panic, not a hang.
* Extra turns (CR 500.7): a stack of players; the most recently created extra turn is taken
  first. `Turn.normal` remembers whose normal turn came last, so after any extra turn the
  normal order goes on (the opponent of that player).
* Setup (CR 103): libraries shuffled from the seed, the starting player chosen from the seed
  (no play/draw choice yet), seven cards each, then the **London mulligan** (103.5):
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
  one). The same rule applies to activated abilities later.
* **`PayMana`** (601.2g-h; full description in section 18). Phyrexian pips first (how many
  of each colour are paid with life), then the pool (coloured pips by colour, hybrid pips,
  then generic). The rest is paid by choosing which mana abilities to activate, one decision
  per source: options are one representative per class of equivalent sources (same
  definition and ability), restricted to classes whose addition keeps the cost payable, and
  in non-decreasing class order so each *set* of sources is reachable exactly once (no
  permutation branching). Single-mana sources are matched to the pips (which colour each
  pays for cannot matter: all of it is spent); several-mana sources (Sol Ring, Black Lotus)
  pick their production, and the excess stays in the pool. Feasibility is a matching over
  the pips' acceptable types (`mana::payable`), with budgets for life and for permanents a
  later sacrifice needs.
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
simultaneous move), 704.5d (a token outside the battlefield ceases to exist: to
`Zone::Nowhere`, after a "dies" event could see it in the graveyard). Losing ends
the game at once; if all players lose simultaneously the game is a draw (104.4a). Later SBAs
(poison, planeswalker loyalty, auras, +1/+1 and -1/-1 counters) are new arms of the same pass.

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
  56-63, player in bits 48-55, payload in the low bits. Shuffles, random orders and token
  creation emit nothing (they reveal nothing; the next draw is keyed).
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
  Assumption for now: both decklists are known (open decklists); team drafts will need
  partial pool knowledge (M6).
* **Checks** (`tests/fuzz.rs`, every ~40 actions): `determinize(to_move)` keeps the viewer's
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
| `EXILE` | 980-1011 | at priority: play or cast the k-th card in exile the decider may play (exile order; M7a) |
| reserved | 1012-1023 | |

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
escape, lands with Crucible of Worlds) and exiled cards with a permission (`EXILE`). There
are 32 decision kinds, the context block's capacity (section 22).

Mapping to zero-core's `SearchGame` (M5a, section 21): `to_move` = the pending decision's
player; `legal_actions` = the ids; `step` = `apply`; `result` from `outcome()`; `determinize`
as above; `NUM_ACTIONS = 1024`.

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
graveyard targets, coin flips, energy, amass, persist, monstrosity, living weapon, devotion.

Supported cube cards: 340 (M4b-2: 191, M4b-1: 133, M4a: 31); creatures 104 (M4b-2: 20).
New in M7a (149): Abandoned Air Temple, Adeline, Agent Bishop, Alpha Deathclaw, Archon of
Cruelty, Arwen, Atraxa, Auriok Salvagers, Baleful Mastery, Baloth Prime, Barrowgoyf, Basalt
Monolith, Bejeweled Warg, Bloodtithe Harvester, Boomerang Basics, Bristly Bill, Burst
Lightning, Carnage Interpreter, Chaos Defiler, Clarion Conqueror, Consult the Star Charts,
Containment Priest, Cori-Steel Cutter, Council's Judgment, Crucible of Worlds, Cryogen Relic,
Dark Confidant, Daze, Death-Greeter's Champion, Deep-Cavern Bat, Dig Through Time, Dismember,
Displacer Kitten, Dragon's Rage Channeler, Eagles of the North, Echo of Eons, Endurance,
Exhume, Expressive Iteration, Faerie Mastermind, Faithless Looting, Figure of Destiny, Figure
of Fable, Flash, Flashback, Flickerwisp, Force of Negation, Force of Vigor, Force of Will,
Formidable Speaker, Frantic Search, Fury, Galvanic Discharge, Get Lost, Ghost Vacuum, Giver
of Runes, Glimmer Lens, Goblin Engineer, Golos, Grief, Grim Monolith, Grindstone, Guide of
Souls, Gut, Harvester of Misery, Icetill Explorer, Ignoble Hierarch, Iron-Shield Elf, Ivora,
Knight of Autumn, Kolaghan's Command, Leatherhead, Leyline Binding, Lightning Greaves,
Lingering Souls, Lotus Cobra, Luminarch Aspirant, Magda, Mana Crypt, Mana Drain, Manifold
Key, Mind Twist, Mine Collapse, Mishra's Bauble, Mole Man, Mother of Runes, Mox Diamond,
Nettlecyst, Noble Hierarch, Oath of Druids, Oliphaunt, Once Upon a Time, Orcish Bowmasters,
Ouroboroid, Phelia, Phlage, Portable Hole, Psychic Frog, Pyrogoyf, Pyrokinesis, Questing
Beast, Ragavan, Ravenous Robots, Reanimate, Restless Vinestalk, Retrofitter Foundry, Robber
of the Rich, Sanguine Evangelist, Screaming Nemesis, Scythecat Cub, Sentinel of the Nameless
City, Sewer-veillance Cam, Shallow Grave, Sheoldred, the Apocalypse, Skateboard, Skullclamp,
Skyclave Apparition, Smuggler's Copter, Snapcaster Mage, Sneak Attack, Snuff Out, Solitude,
Soul-Guide Lantern, Spectacular Spider-Man, Subtlety, Super Shredder, Sylvan Safekeeper, Tear
Asunder, Territorial Kavu, Thalia, Thassa's Oracle, The Fantasticar, Tidehollow Sculler,
Titania, Torsten, Touch the Spirit Realm, Traumatic Critique, Treasure Cruise, Troll of
Khazad-dûm, True-Name Nemesis, Unearth, Unexpectedly Absent, Upheaval, Uro, Urza's Bauble,
Walking Ballista, Witherbloom Apprentice, Wolverine, Woodfall Primus. New in M4b-2 (58): the fetch lands Arid
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

Not yet (later milestones; each is a blocker in the support report, never a silent no-op):
planeswalkers and loyalty (and attacking them), battles, copies (of spells, token copies,
"enters as a copy"), transform and double-faced cards, adventures, ward, targeting abilities
on the stack, the initiative and the monarch, sagas, replacement and prevention effects beyond
the ones above, escalate and spree, multikicker, mana with spend restrictions, mana abilities
whose cost has a choice or a mana part (Orcish Lumberjack, Sunken Ruins, Chromatic Star),
granted triggered abilities (Kaldra Compleat), activated abilities in graveyards, naming a
card, "activate only once" / exhaust / boast, the play/draw choice.

Deliberate simplifications and limits (each is small and local to change):

* Mana already in a pool is spent first and automatically: coloured pips from their colour,
  hybrid pips from the more plentiful of their colours, generic pips colourless first, then
  the most plentiful colour (section 5). Which pool mana pays a generic pip can matter for
  what is left later in the step; a hybrid pip paid greedily can, in rare pools, make the
  rest look unpayable (the payment then just offers fewer options; nothing illegal happens).
* Mana abilities are not free-standing priority actions (section 5), and a payment
  activates a minimal set of sources: every pick must pay part of the cost, so extra mana
  is never floated during a payment (the excess of a several-mana source stays).
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
* M7a's own simplifications are in section 23.11.
* Waiting triggers with the same definition and ability are offered once in the ordering
  decision (their order cannot matter while effects ignore their source's per-object state).
* Combat damage amounts use the `NUMBER` range (0..31); a larger amount to divide offers the
  first 30 amounts, lethal and all (M7a).
* Creatures attack only players; the starting player is chosen from the seed; determinize
  assumes open decklists.
* Targets are not deduplicated (section 11), nor are activations of identical permanents
  (two untapped Mind Stones offer two `ACTIVATE` ids).

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
  through their mana abilities. A land with no mana ability in the IR and without
  `mana_only_from_produces` relies on that fallback; the only one is Strip Mine (its IR lacks
  "{T}: Add {C}", an IR defect to fix in mtg-bot), which is therefore blocked
  (`CardPrinting.mana_only_from_produces(land without a mana ability)`).
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
  the source (its colours, deathtouch, lifelink); Python deals the damage from Pyrogoyf.
* M7a: "nonland permanent" filters: Python's predicate admits instant and sorcery cards
  (in a graveyard, on the stack); here a permanent card type is required.
* M7a: "whenever you cast your second spell each turn" is a condition of the event, not an
  intervening "if" (CR 603.4 applies to "if" clauses only), so it isn't checked again on
  resolution.
* M7a: Leatherhead's "that player controls": the player dealt combat damage (the opponent
  in two-player games).
* M7a: Robber of the Rich lasts while the card is exiled (the Oracle text; the IR's default
  "this turn", 23.10), as in Python.

## 15. Tests

`cargo test -p engine` runs everything below in about 10 s (debug). Rules tests name the CR
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
| `tests/rules_activated.rs` | 602.2a (on the stack before targets and costs), 602.2 (targets before a sacrifice cost; the source gone while its ability waits), 605 (mana abilities while paying, not at priority), 302.6 (tap abilities and summoning sickness, haste; non-tap and noncreature abilities), 602.5d, 602.5b, 106 (excess mana stays; Black Lotus's colour), 601.2g (activation order: Mox Opal before Black Lotus), a card's mana abilities are alternatives (Talisman damage), 119.4, 107.4f (Phyrexian), 107.4e (hybrid) and a mana choice on resolution, 601.2b (additional-cost options), sacrifice reservations, Lion's Eye Diamond, cycling, "you may", indestructible, hexproof; 605.3a (a Spirit Guide while paying, also an "unless pays" cost during resolution; revealed when picked), a hand mana source leaves the cards a discard cost needs |
| `tests/rules_lands.rs` | 614.1c (enters tapped; fast lands), 614.12 (shock lands, 119.4 at 1 and 2 life), Starting Town, horizon lands, Gaea's Cradle, Ancient Tomb, Wasteland, Karakas, Strip Mine blocked |
| `tests/cards.rs` | the cube cards: Mana Leak (pay / decline / cannot pay), Memory Lapse, Counterspell, Spell Pierce, Brainstorm (mid-resolution choices, a clone taken mid-resolution), Ancestral Recall, Dark Ritual + Hymn (pool, keyed random discard), Time Walk, Cut Down / Flame Slash filters, Cogwork Librarian, mana-source classes |
| `tests/cards_m4b1.rs` | Swords to Plowshares (last known power), Fatal Push (resolution check; revolt from a Lotus Petal sacrificed for mana), Unholy Heat (delirium), Cabal Ritual (threshold counted while on the stack), Abrupt Decay vs Counterspell, Oust, Remand / No More Lies, Time Warp on the opponent (500.7), Wheel of Fortune, Relic of Sauron, Zuran Orb, Voltaic Key, Aether Spellbomb, Mox Amber, Loran of the Third Path |
| `tests/rules_library.rs` | 701.19c (must find "a card"; nothing in an empty library), 701.19b (may fail; revealed on top), Vampiric Tutor (known to its owner only), Entomb, fetch lands (land types, a shock land's choice first), Path to Exile (the creature's controller may search; no search, no shuffle), typecycling, a search decision unchanged under its searcher's determinize, 701.20 (a shuffle keeps "in the library", not where), 701.18 scry (looks keyed and masked; bottom and top order), 701.42 surveil (and a surveil land), Ponder, Stock Up (sets in canonical order, no impossible pick), Sensei's Divining Top, a scry of a revealed card (a cloud over the scried cards) |
| `tests/cards_m4b2.rs` | Duress (revealed hand, keys, filter, known until it changes), Thoughtseize, Inquisition, Gitaxian Probe (private look, masked keys), a seen hand partly put back (cloud), 700.2a (modes before targets), a mode without targets, Fiery Confluence (repeats, non-decreasing), Mystic Confluence (a target per instance), 608.2b for modal spells, Pyroblast, 603.3c / 700.2b (modal triggers; no mode chosen), 111 / 704.5d tokens (Clue; a token put into a library), Malevolent Rumble, Thopter Foundry (nontoken), Sheoldred's Edict, Generous Ent, Timetwister |
| `tests/hidden.rs` | determinize keeps the viewer's view and actions; known cards stay; observer masking of keys; refreshed decisions of the other player; an ability activated from hand reveals its card (602.2a); a payment in progress completes in every determinized world (pinned hand sources, then revealed); a pending shock-land choice survives determinize; a Brainstorm after a hand reveal leaves partial knowledge; a card leaving the hand is not told apart from a known twin |
| `tests/rules_layers.rs` | M7a: 613 layers (types, colours, keywords, set P/T in timestamp order, pumps, counters), 611.2c, statics and their conditions, 704.5q, protection 702.16, "can't block" groups, equipment 301.5 / 704.5n, exalted, battle cry, prowess |
| `tests/cards_m7a.rs`, `cards_m7a_costs.rs`, `cards_m7a_zones.rs`, `cards_m7a_zones2.rs`, `cards_m7a_threats.rs`, `cards_m7a_threats2.rs`, `cards_m7a_misc.rs`, `cards_m7a_x.rs`, `cards_m7a_more.rs` | M7a cards and rules: equipment triggers, taxes and locks, replacement of uncast creatures, flicker, coin flips, explore, alternative costs and evoke (118.9, 702.74), divided damage (601.2d), linked exile (610.3), delayed triggers (603.7), reflexive triggers (603.12), random bottom order, tokens attacking (508.4), flashback (702.34), escape (702.138), exile permissions, dash (702.109), backup, amass, persist, monstrosity, living weapon, chosen player (614.12), devotion and winning (104.2b), delve, kicker, energy, X (107.3), crew (702.122), extra land drops |
| `tests/support.rs` | the support report and its threat view, the exact supported list, loud failures |
| `tests/az.rs` | the zero-core game `mtg` (section 21.8): games end with sane results, the turn cap, clones, `determinize` keeps features and legal actions for both viewers, equal keys mean equal features, `force_chance` replays and stays aligned, encoding slots, decks, spec parsing, heuristic vs random (`AZ_SCALE=k` for more games) |
| `tests/fuzz.rs` | random play (below) and replay determinism |
| `tests/perf.rs` | the numbers in section 17 (`--release -- --ignored --nocapture`) |

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
no failure (see section 17 for the time). M7a: 20,000 games over the 340 cards, no failure.

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

| Measure | M7a (340 cards) | M4b-2 (191 cards) | M4b-1 (133 cards) | M4a (31 cards) |
|---|---|---|---|---|
| Random play | 905 games/s (4,000 games, setup included) | 694 games/s | 750 games/s | 1,357 games/s |
| Per exposed decision (`apply`, running to the next decision) | 5.0 us (220 decisions, 29 turns per game on average: more threats end games sooner) | 6.2 us (231 decisions, 36 turns per game on average) | 6.5 us (206, 36) | 4.4 us (169, 36) |
| `Game::clone` (mean over mid-game states) | 0.98 us (6,160 states); `size_of::<Game>()` = 880 B | 1.36 us (6,616 states); 600 B | 1.3 us; 576 B | 1.04 us; 560 B |
| `clone` + `determinize(to_move)` (one search simulation's setup) | 1.6 us | 2.3 us | 2.3 us | 2.4 us |
| `legal_actions` (copies cached ids) | 4 ns | 8 ns | 4-9 ns | 6 ns |
| Fuzzer, 20,000 games, release | 79 s, no failure | 104 s, no failure (determinize checked for both viewers, information sets) | 60 s | 28 s |

A decision costs more because every priority decision computes the mana sources and, for
each card in hand and each ability, whether its cost can be paid (a matching with budgets),
and payments are decisions now. Most of the remaining time is small allocations under the
Windows system allocator (mana classes, candidate lists, SBA groups); the matching itself is
allocation-free. Caching the sources between decisions, or a faster allocator, would cut
it if it ever matters (the network evaluation per simulation will cost far more).

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
permanents matching a filter (Mox Amber). Costs a mana ability may have while paying: {T},
life, sacrifice or exile itself (no choices, no mana part: those are blockers). Every mana
ability taps, sacrifices or exiles its source, so a card's mana abilities are exclusive
alternatives (the lowering checks). Where they are activated:

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

**`PayMana` in detail.** State: the remaining cost, the picks so far, `Keep` (what later parts
of the cost need: the excluded source, life, a sacrifice or discard reservation; at most one
of the two per cost, the lowering checks). Stages:

1. Phyrexian pips: per colour, one `PayPhyrexian` decision "how many of these k pips are
   paid with 2 life" (0..k, only feasible counts; one decision per colour, not per pip, so
   no permutation branching). The life is paid at the end (601.2h comes after 601.2g).
2. The pool pays what it can (section 5).
3. Sources, one decision per pick, classes in non-decreasing order: several-mana classes
   first, then single-mana ones (canonical battlefield order within each). Picking a
   several-mana source asks for its production (`ChooseMana`, only productions that pay part
   of the cost and leave the rest payable); its mana pays what it can at once and the rest
   is remembered to stay in the pool. Single-mana sources are picked as a set; when they
   cover the rest exactly, the payment ends.
4. Nothing has been activated yet: every pick is checked against the state at the start of
   the payment. At the end everything is activated in an order the rules allow (601.2g lets
   the player choose): sources that keep their permanent first (so "activate only if you
   control three artifacts" still holds for Mox Opal when Black Lotus is sacrificed), life
   payments before damage (Mana Confluence before Ancient Tomb), sacrifices last; then the
   Phyrexian life.

Feasibility (`can_pay_with`, `coverable`) is exact: a matching of the pips' acceptable types
(`payable`, with the chosen units forced in), with budgets (`payable_within`): life (CR
119.4; when life is paid later in the same cost, the reserved amount and the damage mana
abilities deal are counted, since that damage comes first), and permanents a later sacrifice
needs (a Lotus Petal cannot pay for Tinker's mana if it is the only artifact to sacrifice), or
hand cards a later discard needs (a Spirit Guide cannot pay if it is the card to discard).
Each card contributes at most one way (its mana abilities are alternatives); a way whose
activation takes budget is all of its mana or none (Black Lotus's three mana come from one
activation). Phyrexian splits and "n of one chosen type" productions are tried one by one.

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
  until mtg-bot exports it.
* M7a IR findings (section 23.10): Celestial Colonnade and Creeping Tar Pit lack the colours
  their animation sets; Robber of the Rich's permission duration; Get Lost's Map token (the
  engine now builds Map from the printing other cards carry).
* IR gap: Get Lost's rider (`ControllerCreatesTokens { token_name: "Map" }`) names a token
  the IR does not describe (Python looks it up in `TOKEN_PRINTINGS`); the IR should carry the
  printing, as `CreateToken.token_printing` does (Map also needs explore).
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
| `parse_spec` / `new_game(spec, 2, seed)` | `MtgSpec` (below); the game seed draws the decks (21.3), then `Game::new` (shuffles, starting player, mulligans) |
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
per game), `"eval"` (a pair of the fixed evaluation set) or `{"p0": [names], "p1": [names]}`.
Unknown keys are errors. The cube IR is compiled into the binary (`db::CUBE_IR`), so the Python
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
  expensive spell it can afford in its main phases, skipping harmful targeted spells without
  an opposing target and a sweeper that would hit more of its own creatures; counters an
  opponent's spell when it can; burns the opponent out when the damage is lethal; attacks
  with each creature that no untapped opposing creature can kill and survive (all of them
  when the opponent has no untapped creature); blocks when the blocker survives or trades
  for a creature of at least its mana value, chump-blocks when the unblocked damage would be
  lethal; assigns lethal damage in order; aims harmful effects at the opponent's most
  valuable permanent it removes (or the player) and helpful ones at its own; keeps hands of
  2-5 lands (one mulligan at most); gives up lands when flooded, else its most expensive
  card; pays with the first source offered; never activates abilities. It reads only public
  information and its own cards. A yardstick, not a goal.

Measured (`cargo run --release -p engine --example az_probe -- <a> <b> <games> <spec>`, 1000
games each, seats alternating, `max_turns` 60):

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
`CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/run-<sha> bash tools/build_py.sh` there, then
`python tools/launch_hidden.py <run>/supervisor_console.txt python tools/supervise.py <run>
--preset mtg-1`. While the pinned core commit is not on GitHub, the submodule clone of a new
worktree cannot fetch it: `git -C third_party/zero-core fetch ~/zero-core main && git -C
third_party/zero-core checkout <pin>` first. Pause / resume / stop as in every Zero project
(PAUSE and STOP files, `tools/pause_run.py`, the dashboards). zerodash lists the runs
(`zero-core/dashboard/projects.json`, project `mtg`).

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
and always answers a legal id. `tests/` (pytest): the engine module registers `toy` and `mtg`,
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
  `mtgz/az/layout_v2.json` (v1: `layout_v1.json` at commit 531070a; `cargo test -p engine
  --test az layout_json_is_checked_in`; `MTGZ_WRITE_LAYOUT=1` rewrites it):

  | Segment | Slots x floats | Present flag | Identity | Actions scored from the slot's token |
  |---|---|---|---|---|
  | context | 1 x 384 (v2: 192) | always | the decision's card (v1: one-hot of 256; v2: card code) | |
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

## 23. Threats first: layers, counters, generic triggers, other zones (M7a)

M7a grew the supported cube from 191 to 340 cards, creatures from 20 to 104 (threats, i.e.
creatures, Vehicles, Equipment, manlands, token makers and burn, from 37 to 125). The work
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
  increasing slot order, never leaving the rest unpayable.
* **Other cost parts**: exile a card of a kind from your hand, return permanents to hand,
  exile n cards from your graveyard (a set: increasing positions), remove counters from the
  source (by kind or name), crew n (702.122: untapped creatures other than the Vehicle, one
  per decision in slot order, `PASS` once their power is enough), divided damage announced
  with the targets (601.2d, `DivideDamage`).

### 23.5 Casting and playing from other zones (601.2a)

`cast_route` says how a card may be cast from where it is: from hand; from its owner's
graveyard with **flashback** (702.34; printed, or granted for its mana cost by Snapcaster
Mage or Flashback; the card is exiled whenever it would leave the stack) or **escape**
(702.138; the permanent knows it escaped); from exile with a **permission**
(`Game.permissions`: Golos, Ragavan, Robber of the Rich, Expressive Iteration; this turn or
while exiled, free, "mana as though of any colour", "during any turn you attacked with a
Rogue"). Lands may be played from exile with a permission or from the graveyard ("you may
play lands from your graveyard"); additional land drops (305.2) count. Priority ids: a
graveyard card is its `GRAVEYARD` slot; an exiled card with a permission is `EXILE` + k, the
k-th such card in exile order (980-1011, section 11). A card cast this way is its own source
for protection and never its own graveyard target. "Without paying its mana cost" is an
alternative cost; flashback, escape and permissions don't combine with another one (118.9a).

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
At the end of M7a: threats 125 supported (104 creatures), 121 blocked (105 creatures).

### 23.10 Blocked by the IR, not by the engine

* Celestial Colonnade and Creeping Tar Pit: `BecomeLandCreatureUEOT` lacks the animated
  creature's colours ("a 4/4 white and blue Elemental"); a coloured land without
  `set_colors` is a blocker rather than a colourless creature.
* Strip Mine has no "{T}: Add {C}" (section 20).
* Robber of the Rich's permission keeps the default duration ("this turn") in the IR; the
  Oracle text ("during any turn you attacked with a Rogue") makes it last while the card is
  exiled, as Python does; the lowering follows the Oracle text.

### 23.11 Simplifications

* A Pyrogoyf-like "that creature deals damage" whose creature left uses its last known
  power and its characteristics as a card (not as it last existed on the battlefield).
* Untap-up-to effects offer only tapped permanents (choosing an untapped one does nothing).
* Combat damage to divide among blockers above 31 offers the first 30 amounts, lethal and
  all (the `NUMBER` range).
* An Equipment's timestamp is not refreshed when it is re-attached (no layer-dependent
  interaction in the pool).
* Mox Diamond's "if this would enter" replacement applies as the spell resolves (no other
  supported effect puts it onto the battlefield).
* Escape granted by Underworld Breach isn't offered for a card with its own additional
  costs (Bitter Triumph): the two costs would have to be paid as one.
* "True-Name Nemesis": choosing yourself is offered (a `Target` decision with `PLAYER` ids;
  dominated, but legal).
* The tap-or-untap choice, a counter kind to remove, an energy amount and X use `ChooseMode`
  with `NUMBER` ids; DecisionKind stays at 32 kinds (the network's context block), so "top
  or bottom" is a `YesNo` (`YES` = top).
