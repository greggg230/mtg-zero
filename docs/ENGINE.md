# The MTG Zero rules engine (`engine` crate)

Status: M4b-1 (activated abilities, costs, mana abilities, lands), 2026-10-21; M4a built the
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
* Mana abilities of permanents are activated only while paying a cost (601.2g / 117.1d),
  never as a free-standing priority action: floating mana with nothing to spend it on cannot
  matter. The exceptions are abilities that may be activated "only as an instant" (Lion's
  Eye Diamond: never while paying) and mana abilities of cards in hand (Spirit Guides: only
  with priority, so a payment never waits on a hidden card; section 18). Mana already in a
  pool (Dark Ritual) is spent automatically and first when a cost is paid (it empties at the
  end of the step anyway). Recorded simplification, section 13.
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
  kind: Spell(card), controller, targets, cost_option }` (601.2a); if it has several
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
| `MY_PERM` | 100-195 | decider's permanents, slot 0..95 (attack, block with, sacrifice, target) |
| `OPP_PERM` | 196-291 | opponent's permanents, slot 0..95 (target, block this attacker) |
| `STACK` | 292-323 | stack slot 0..31, 0 = top (target spell) |
| `TRIGGER` | 324-339 | decider's waiting triggers, slot 0..15 (order triggers) |
| `LIBRARY` | 340-403 | cards of a searched / looked-at pile, slot 0..63 (M4b) |
| `GRAVEYARD` | 404-531 | decider's 0..63, opponent's 64..127 (M4b) |
| `ACTIVATE` | 532-915 | `MY_PERM` slot * 4 + ability index: activate at priority; a mana source while paying |
| `HAND_ABILITY` | 916-979 | the ability of the card in the decider's hand slot 0..63 (cycling, Spirit Guides) |
| reserved | 980-1023 | alternative costs, modes, ... |

`NUM_ACTIONS = 1024`. Canonical orders: hand = sorted by (definition, card index); permanents
= timestamp order, split by controller; stack = top first. **Equivalent options are offered
once**: hand cards with the same definition (hidden-zone cards have no other state), mana
sources of the same class. Targets are not deduplicated yet (it needs an exact "same
observable state" key over every per-object field; an optimisation for later).

Decisions by kind: `Priority`, `Mulligan`, `MulliganBottom`, `Target`, `PayMana` (`ACTIVATE`
ids), `PayPhyrexian` (`NUMBER`: how many pips of a colour are paid with life), `ChooseMana`
(`NUMBER`: which production of a mana ability or effect), `ChooseCost` (`NUMBER`: which
additional cost), `Sacrifice` and `DiscardCost` (one object per decision), `DeclareAttackers`
(one creature per decision in increasing slot order, `PASS` = done: each attacking set exactly
once), `DeclareBlockers` (one decision per potential blocker in slot order: which attacker, or
`PASS`), `AssignDamage` (amount per blocker in order), `YesNo` (pay, "you may", shock lands),
`ChooseHandCard` (Brainstorm), `Discard` (cleanup, discard effects), `OrderTriggers`,
`LegendRule`. Activating an ability is `Choice::Ability(card, slot)`.

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

Supported cube cards: 133 (M4a: 31). New in M4b-1 (102): lands Ancient Tomb, Blackcleave
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
layers and continuous effects ("until end of turn" pumps, keyword grants, manlands),
replacement and prevention effects beyond entering tapped, alternative costs (kicker,
flashback, Force of Will, evoke), X costs, cost reductions, tokens, counters (and so loyalty,
planeswalkers and battles), copies, delayed triggers, protection and ward, library search,
surveil / scry / looking at libraries or hands, modal spells, graveyard targets, "activate
only once" / exhaust / boast, mana abilities whose cost has a choice or a mana part (Orcish
Lumberjack, Sunken Ruins, Loot), spend restrictions on mana, the play/draw choice,
`force_chance`, `encode`.

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
* Mana abilities of cards in hand (Spirit Guides) are activated with priority only, never
  while paying; nothing is lost because activating a mana ability does not pass priority.
* Events recorded: enters the battlefield and dies. Cast, attack, block, tap and draw events
  get recorded when the first card listens for them (`triggers.rs`, one place).
* Waiting triggers with the same definition and ability are offered once in the ordering
  decision (their order cannot matter while effects ignore their source's per-object state).
* Combat damage amounts use the `NUMBER` range (0..31); a larger amount to divide is an
  assertion failure for now.
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
| `tests/rules_activated.rs` | 602.2a (on the stack before targets and costs), 602.2 (targets before a sacrifice cost; the source gone while its ability waits), 605 (mana abilities while paying, not at priority), 302.6 (tap abilities and summoning sickness, haste; non-tap and noncreature abilities), 602.5d, 602.5b, 106 (excess mana stays; Black Lotus's colour), 601.2g (activation order: Mox Opal before Black Lotus), a card's mana abilities are alternatives (Talisman damage), 119.4, 107.4f (Phyrexian), 107.4e (hybrid) and a mana choice on resolution, 601.2b (additional-cost options), sacrifice reservations, Spirit Guides, Lion's Eye Diamond, cycling, "you may", indestructible, hexproof |
| `tests/rules_lands.rs` | 614.1c (enters tapped; fast lands), 614.12 (shock lands, 119.4 at 1 and 2 life), Starting Town, horizon lands, Gaea's Cradle, Ancient Tomb, Wasteland, Karakas, Strip Mine blocked |
| `tests/cards.rs` | the cube cards: Mana Leak (pay / decline / cannot pay), Memory Lapse, Counterspell, Spell Pierce, Brainstorm (mid-resolution choices, a clone taken mid-resolution), Ancestral Recall, Dark Ritual + Hymn (pool, keyed random discard), Time Walk, Cut Down / Flame Slash filters, Cogwork Librarian, mana-source classes |
| `tests/cards_m4b1.rs` | Swords to Plowshares (last known power), Fatal Push (resolution check; revolt from a Lotus Petal sacrificed for mana), Unholy Heat (delirium), Cabal Ritual (threshold counted while on the stack), Abrupt Decay vs Counterspell, Oust, Remand / No More Lies, Time Warp on the opponent (500.7), Wheel of Fortune, Relic of Sauron, Zuran Orb, Voltaic Key, Aether Spellbomb, Mox Amber, Loran of the Third Path |
| `tests/hidden.rs` | determinize keeps the viewer's view and actions; known cards stay; observer masking of keys; refreshed decisions of the other player; an ability activated from hand reveals its card (602.2a); a pending payment completes in every determinized world; a pending shock-land choice survives determinize |
| `tests/support.rs` | the support report, the exact supported list, loud failures |
| `tests/fuzz.rs` | random play (below) and replay determinism |
| `tests/perf.rs` | the numbers in section 17 (`--release -- --ignored --nocapture`) |

**Fuzzer.** Uniform random actions in complete games between random 40-card decks (17 lands,
23 other cards drawn with repetition from **every** supported card and the custom test cards,
so a newly supported card is fuzzed without editing a list; duplicates, legends and combat
keywords are common). After every action: no panic; every card in exactly
the zone list its `zone` says, once (card count conserved); battlefield in timestamp order;
legal actions sorted, unique, non-empty, `< NUM_ACTIONS`; no chance keys left in the state.
Every ~40 actions: two clones given the same action stay identical (state hash and keys), and
`determinize(to_move)` keeps the decider's legal actions and the board, then the resampled
world plays on 30 random actions under the same checks. Every game must end (60-turn cap).
Separately, 30 games are replayed from their seed and action list and must reproduce the
final state hash, the outcome and the chance-key sequence exactly. `FUZZ_GAMES=n` sets the
game count (default 300). A failing game prints its seed; `FUZZ_FIRST=s FUZZ_GAMES=1
FUZZ_LOG=1` reruns it with the game log in the panic message. M4b-1: 20,000 games in release,
no failure (see section 17 for the time).

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
   At M4b-1 the greedy order starts library search (13 cards alone), surveil (11), fetch
   lands (11), modal spells (6 + 5); see section 20.
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

| Measure | M4b-1 (133 cards) | M4a (31 cards) |
|---|---|---|
| Random play | 750 games/s (4,000 games, setup included) | 1,357 games/s |
| Per exposed decision (`apply`, running to the next decision) | 6.5 us (206 decisions, 36 turns per game on average) | 4.4 us (169, 36) |
| `Game::clone` (mean over 5,777 mid-game states) | 1.3 us; `size_of::<Game>()` = 576 B | 1.04 us; 560 B |
| `clone` + `determinize(to_move)` (one search simulation's setup) | 2.3 us | 2.4 us |
| `legal_actions` (copies cached ids) | 4-9 ns | 6 ns |
| Fuzzer, 20,000 games, release | 60 s, no failure | 28 s |

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

* mana abilities of permanents: only while paying a cost (`PayMana`);
* "activate only as an instant" ones (Lion's Eye Diamond): only with priority, never while
  paying (its ruling); costs, then a `ChooseMana` decision, mana to the pool, priority stays;
* mana abilities of cards in hand (Spirit Guides): only with priority, the same way. A
  payment therefore never waits on a card hidden in hand, which the opponent's
  `determinize` could resample into something else (a test found exactly that). Nothing is
  lost: activating a mana ability does not pass priority.

**`PayMana` in detail.** State: the remaining cost, the picks so far, `Keep` (what later parts
of the cost need: the excluded source, life, a sacrifice reservation). Stages:

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
needs (a Lotus Petal cannot pay for Tinker's mana if it is the only artifact to sacrifice).
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
* Unsupported cards stay out of `CardDb`'s playable set with every blocker named; the M4b-2
  candidates by the dump (upper bounds; nested blockers can surface): library search
  (`SearchLibrary`, fetch lands, Path to Exile's rider: 133 -> about 170), library looks
  (scry, surveil lands, look at the top: -> 174), hand reveal and discard (Duress,
  Thoughtseize, Gitaxian Probe: -> 178), modal spells (-> 189), tokens (-> 195). All of
  these are hidden-information features: searches name definitions (section 10), revealed
  cards set knowledge bits, library positions become known to a player.
