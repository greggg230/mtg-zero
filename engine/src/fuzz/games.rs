//! The random-play fuzzer of `engine/tests/fuzz.rs` as library code (moved here for zero-farm's
//! worker, docs/ENGINE.md 114.1; the tests `fuzz_random_games` / `fuzz_random_matches` call
//! [`random_games`] / [`random_matches`], the farm worker the same functions, so the same seeds play
//! the same games and print the same lines). Settings are read through [`super::env::var`] (the
//! process environment, or the farm job's settings).
//!
//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, checking after every action that nothing panics, the
//! structural invariants hold (every card in exactly one zone), legal
//! actions are sorted, unique and non-empty, and the game terminates; that
//! the same seed and actions replay bit for bit; that a clone continues
//! identically; that `determinize` keeps the decider's view; and that every
//! option of a sampled decision can be completed, in the game and in the
//! worlds `determinize` samples.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300).
//! `FUZZ_SKIP=s1,s2,...` leaves those seeds out (games and matches); the
//! games' run prints the longest turn (decisions in one turn: loops of
//! optional actions have no cap, `engine/src/loops.rs`).
//!
//! Matches (M8b, ENGINE.md 36.8): `fuzz_random_matches` plays best-of-three
//! matches through `MtgAz` with random actions (sideboarding too), checking
//! the legal actions, the game invariants, clones, `determinize` (the
//! decider's actions, view and encoding), the decks of every later game
//! (legal, from the pool), the chooser of every later game (CR 103.1) and
//! the match's end; `FUZZ_MATCHES=n` (default `FUZZ_GAMES` / 20, at least
//! 20). M8b part b (ENGINE.md 36.11): `determinize` for both viewers, in
//! games and while sideboarding, samples the opponent's hidden pool and deck
//! consistently with what the viewer knows (`check_hidden_pool`), keeps the
//! viewer's encoding, and two sampled worlds stay indistinguishable to the
//! decider while zero-core would keep them in one node.
//!
//! `FUZZ_POOL=interactions` (M8d part 2, ENGINE.md 38.5): combo-dense decks.
//! Each player's 23 spells come from two or three of the interaction groups
//! below (`GROUPS`: cards that interact under the CR, one group per family
//! of cells of `docs/interactions.md`: copy effects with enter and leave
//! abilities, flicker and linked exile, reanimation with replacement
//! effects, face-down cards, double-faced cards, counterspells and copies of
//! spells, control changes, taxes and free casts, damage and protection,
//! planeswalkers, last known information and state-based actions, combat
//! keywords, day and night), one in eight from all the groups; 17 random
//! lands; a sideboard of seven from the same groups. Games and matches
//! (best-of-three on those lists, open decklists) both use it, with every
//! check of the default mode. The decision kinds asked are counted and
//! printed (interaction decisions per 1,000, against the default mode's).
//! `FUZZ_GROUPS=n` deals from the first `n` groups only: a failure recorded
//! before a group was added gets its decks again (ENGINE.md 51).
//! `FUZZ_POOL=drafted` (M6 part 3, ENGINE.md 45): the matches are drafted
//! ones: a `team_draft` event (4v4 or 2v2; random picks and decks, or team A
//! drafting and building as `consensus`) and one of its matches as the `mtg`
//! spec a co-training run plays (`draft_bots::match_spec`: the drafted
//! decks, the rest of the pools as sideboards, the real first picks, what
//! each player knows is elsewhere, the match seed), one match in eight with
//! 12-turn games; every check of the matches' default mode (the games are
//! the default mode's). Every mode also checks that a sampled first pick is
//! a candidate of the viewer's prior (in a drafted match: a card its team
//! never saw).
//! `FUZZ_POOL=delver` (ENGINE.md 54): the Legacy Izzet Delver mirror, both
//! players the pinned 60 (`az::constructed`, no sideboard) with the card
//! table of constructed play (`CardDb::constructed`: Murktide Regent and
//! Prismari Charm with the cube's cards); every check of the default mode,
//! and with 200 games or more Murktide Regent and Prismari Charm must have been cast, with
//! `DELVER_TRIGGER_GAMES` (2,000) or more Murktide's leave-the-graveyard trigger put on the stack
//! (ENGINE.md 209: rare, so a run or a farm chunk of fewer games may miss it by chance). Its
//! matches (ENGINE.md 120) are constructed
//! best-of-three: both seats the pinned main and the sideboard cards of the 75 this checkout
//! supports (one match in four P1 another 75 of supported cards), one match in four with 12-turn
//! games; every check of the default matches but the
//! hidden pool's, and instead at each checkpoint, for both viewers, the constructed world
//! (`check_constructed_world`: the 75 public and unchanged, the opponent's deck a legal 60 of it -
//! in game 1 the registered main, from game 2 on dealt, holding every card the viewer saw this game -,
//! the world's cards that deck, the first sideboarder's hidden choice a legal 60, the other
//! player's sideboarding restarted), a deck-tilted world too (`FUZZ_DECK_TILT=t`'s tilt, default 1;
//! a random prediction from its own generator: the deck one of the K cached decks), and every later
//! game's decks legal 60s of the 75. Then as many post-sideboard matches (ENGINE.md 130: the same
//! seeds' specs with `"start_game": 2` and one episode per game, one in three a seat keeping its deck)
//! with the same checks, game 1's loser sideboarding first, a keeper's only option `PASS`.
//! `FUZZ_POOL=delver_dealt` (ENGINE.md 154): `FUZZ_POOL=delver` with dealt decks in every match spec
//! (`"dealt_decks"`, by seed: p 0.75 with swaps [1, 8], p 1 with [0, 12], p 0.5 with [3, 3]; the
//! post-sideboard matches without a keeper, which dealt decks refuse) and its checks: the dealt seat never
//! decides at a sideboarding, each later game's dealt 60 has swaps in the range against its registered main,
//! every world keeps the dealt seat and deals its 60 within the range (the information-set walks cover the
//! deal's keys: hidden for the other player, its encodings equal across worlds that dealt different 60s).
//! `FUZZ_POOL=delver_dealt2` (ENGINE.md 168.4): `delver_dealt` with each seat dealt independently (`"per_seat"`:
//! by seed p 0.85 with swaps [0, 15] - the pb3 preset's, [0, 12] against `OTHER_75`'s 12-card sideboard -, p 1 with
//! [0, 12], p 0.5 with [2, 6]), so a match has none, one or two dealt seats, and in every other match a choice table
//! (`"sideboard_table"`, ENGINE.md 168.3: random legal 60s of the Delver 75 for its 24 contexts) whose seats - the
//! Delver 75's, not dealt - always play the table's action at their sideboarding decisions (as zero-core's driver
//! does) and reach exactly the table's 60 of their context.
//! `FUZZ_POOL=dd_a` (ENGINE.md 124.1): Legacy Doomsday, lane A's cards: the
//! Doomsday 60 (MTGGoldfish 8027957) with the cards of the other lanes
//! swapped for more of lane A's (`DD_A_DECK`), against the Delver 60 in
//! either seat, one game in three a Doomsday mirror; constructed play's card
//! table; every check of the default mode, and with 200 games or more
//! Doomsday, Pact of Negation, Personal Tutor and Flow State must have been
//! cast and a Pact trigger put on the stack. Games only (best-of-one).
//! `FUZZ_POOL=dd_b` (ENGINE.md 124.2): lane B of the Legacy Doomsday deck, constructed play's
//! table: one player the Doomsday 60 (MTGGoldfish 8027957) with its cards of other lanes swapped
//! for lane B's and supported ones (`DD_B_DECK`: four Misdirection, Flusterstorm and Cavern of
//! Souls, Orcish Bowmasters for a creature type random play names), against the Delver 60; seats
//! alternate by seed. Every check of the default mode, and
//! with 200 games or more each of the three cards must have been played (Misdirection cast and
//! changing a target, Flusterstorm cast and copied, a Cavern's type chosen and its mana spent on
//! a spell that then can't be countered). Games only (best-of-one).
//! `FUZZ_POOL=dd_c` (ENGINE.md 124.3): lane C of the Legacy Doomsday deck: the Doomsday 60 with
//! its cards of other lanes swapped for lane C's and supported ones (`DOOMSDAY_C`: four Edge of
//! Autumn, Street Wraith and Leyline of the Void), against the Delver 60 in either seat, one game
//! in three the mirror; constructed play's table; every check of the default mode, and with 300
//! games or more lane C's cards must have been cast, cycled and begun on the battlefield, a
//! swampwalker never offered to a blocker whose player controls a Swamp. Games only.
//! `FUZZ_POOL=doomsday` (ENGINE.md 124.4): the real Legacy Doomsday 60 (`data/decks/
//! doomsday_8027957.json`, `Decklist::legacy_doomsday`; its main deck, no sideboard) against the
//! Delver 60 in either seat, one game in three the Doomsday mirror; constructed play's table;
//! every check of the default mode, and with 200 games or more the deck's legacy cards must have
//! been played (Doomsday, Pact of Negation and its trigger, Personal Tutor, Flow State,
//! Misdirection and a new target, a Cavern's type chosen, Edge of Autumn and Street Wraith
//! cycled; Murktide Regent and Prismari Charm on the Delver side), with 2,000 or more Edge of
//! Autumn and Street Wraith cast too. Games only (best-of-one).
//! `FUZZ_POOL=sneak_show` (ENGINE.md 123): the real Legacy Sneak and Show 60 (MTGGoldfish
//! 8014220, `data/decks/sneak_show_8014220.json`; its main deck, no sideboard) against the Delver
//! 60 in either seat, one game in three the Sneak and Show mirror. `FUZZ_POOL=sns` (ENGINE.md
//! 123): the lane's six cards four each (`SNS_DECK`: Show and Tell, Omniscience, Mistrise Village,
//! Brotherhood's End, Disruptor Flute, Magus of the Moon) with the deck's creatures, Sneak
//! Attack, Force of Will and mana, the same pairings. Constructed play's card table; every check
//! of the default mode, and with 200 games or more (`sns`) each of the six must have been cast or
//! activated, a spell cast without paying its mana cost, a spell made uncounterable and a name
//! chosen (`sneak_show`: Show and Tell and Mistrise Village seen, a free cast, a spell made
//! uncounterable). Games only (best-of-one).
//! `FUZZ_POOL=hogaak_a` (ENGINE.md 220.1): lane A of the Legacy Hogaak Tempo deck: the deck's 60 with
//! its cards of the other lanes swapped for supported graveyard and creature enablers
//! (`HOGAAK_A_DECK`: Hogaak, Arisen Necropolis four, Orcish Bowmasters, Kaito, Brazen Borrower, the
//! deck's interaction and mana; Mishra's Bauble, Street Wraith, Entomb, Dark Confidant, Deep-Cavern
//! Bat, Barrowgoyf for Stitcher's Supplier, Mockingbird, Dark Matter Manipulator, Moonshadow and
//! Dryad Arbor), against the Delver 60 in either seat, one game in three the mirror. Constructed
//! play's table; every check of the default mode, and with 200 games or more Hogaak must have been
//! cast from a hand and from a graveyard, paid by convoke, by delve and by both in one payment.
//! Games only (best-of-one).
//! `FUZZ_POOL=hogaak_b` (ENGINE.md 220.2): Legacy Hogaak Tempo's lane B, Mockingbird and Massacre
//! four each (`HOGAAK_B_DECK`) with copy targets of mana value 0 to 7 (Orcish Bowmasters and its Orc
//! Army tokens, Brazen Borrower, Kaito, Concealing Curtains and its back face, Walking Ballista,
//! True-Name Nemesis, Phyrexian Metamorph, Murktide Regent), black and blue mana with Underground Sea
//! and Plains-typed lands (Scrubland, Tundra, a Plains), against the Delver 60 in either seat, one
//! game in three the mirror (where an opponent controls a Plains: Massacre's free cast). Constructed
//! play's card table; every check of the default mode, and with 200 games or more Mockingbird cast,
//! a copy made, a copy declined, Massacre cast for free and Massacre resolving. Games only.
//! `FUZZ_POOL=names` (ENGINE.md 127): "choose a card name" with every kind of face. Each seat a
//! random 60 of constructed play's table: Anointed Peacekeeper and Disruptor Flute four each, eight
//! of the table's cards with more than one face (split cards, double-faced cards, adventurer and
//! preparation cards) three each, 28 basic lands. Every check of the default mode; with
//! `FUZZ_HIDDEN_DECKS=1` the hidden-decklist checks too (the names are then the table's). A name
//! question with a next page takes it three times in four (the table's faces and tokens are on its
//! last pages). With 200 games or more each card must have chosen a name, and the Peacekeeper each
//! kind of name: a card of one face, a back face, an Adventure, a split card's half, and `PASS`.
//! `FUZZ_POOL=delver_sb` (ENGINE.md 122): the Delver 75's five sideboard cards outside the cube
//! in the Delver 60 (`DELVER_SB_DECK`: Consign to Memory, Hydroblast, Price of Progress and Rough
//! // Tumble three each, Meltdown two, for Daze, Ponder, Unholy Heat, Prismari Charm and three
//! Brainstorms) against the Delver 60 in either seat, the sideboarded mirror, or the Sneak and
//! Show 60 (colourless spells, Emrakul's cast trigger), by seed. Constructed play's card table;
//! every check of the default mode, and with 200 games or more each of the five must have been
//! cast and a Consign to Memory aimed at a triggered ability. Games only (best-of-one).
//! `FUZZ_POOL=hogaak_c` (ENGINE.md 220.3): lane C of the Legacy Hogaak Tempo deck (MTGGoldfish 8027954): its 60
//! with the other lanes' cards (Hogaak, Mockingbird) out and lane C's four each (`HOGAAK_C_DECK`: Stitcher's
//! Supplier, Dark Matter Manipulator, Moonshadow, Dryad Arbor) beside the deck's fetch lands, Wasteland, Orcish
//! Bowmasters, Thoughtseize, Force of Will, Daze and Fatal Push, against the Delver 60 in either seat, one game in three
//! the mirror; constructed play's table; every check of the default mode, and with 200 games or more each of the four
//! must have been cast (Dryad Arbor played as a land), a Moonshadow -1/-1 counter removed by its trigger, a Dark
//! Matter Manipulator at +2/+0 or more, and a Dryad Arbor attacking and tapped for mana. Games only (best-of-one).
//! `FUZZ_POOL=lands_b` (ENGINE.md 270.2): lane B of the Legacy Lands deck (MTGGoldfish 8027955): its 61 with the
//! other lanes' cards (Dark Depths, Thespian's Stage, Ghost Quarter, Maze of Ith, Bojuka Bog, The Tabernacle at
//! Pendrell Vale, Pithing Needle, Blast Zone) out and lane B's four each (`LANDS_B_DECK`: Life from the Loam,
//! Exploration, Carnivorous Cultivator // Enroot, Yavimaya, Cradle of Growth) beside the deck's supported cards, more
//! Forests and sideboard cards (Force of Vigor, Dismember), against the Delver 60 in either seat, one game
//! in three the mirror; constructed play's table; every check of the default mode, and with 200 games or more each of
//! the four cast or played, a draw dredged and a dredge declined, a second land played in a turn, Enroot cast, the
//! Cultivator's combat trigger, and a land a Forest by Yavimaya alone (`LANDS_B_SEEN`). Games only (best-of-one).
//! `FUZZ_POOL=hogaak` (ENGINE.md 220.4): the Legacy Hogaak Tempo deck (MTGGoldfish 8027954,
//! `data/decks/hogaak_tempo_8027954.json`, `Decklist::legacy_hogaak`). Games: its real 60 against the real Delver,
//! Doomsday and Sneak and Show 60s in either seat, one game in four the mirror (`seed % 8`), best-of-one, constructed
//! play's table, every check of the default mode; with 200 games or more Hogaak must have been cast from a graveyard
//! paid by convoke and delve together, a Mockingbird must have entered as a copy, a Moonshadow -1/-1 counter must have
//! been removed, a Dark Matter Manipulator seen at +2/+0 or more, a Dryad Arbor played and a Stitcher's Supplier
//! trigger resolved (its mill). Matches: constructed best-of-three between the real 75s, the Hogaak 75 against each
//! of the three in either seat and its mirror twice (`HOGAAK_PAIRS`, `seed % 8`; one match in four 12-turn games),
//! then post-sideboard matches of its mirror, every check of `FUZZ_POOL=delver`'s matches; with 500 matches or more a
//! Massacre (a sideboard card) must have resolved.
//! `FUZZ_POOL=cross` (ENGINE.md 148): the deck pairs on main no other pool plays. Games: the real
//! Legacy Doomsday 60 against the real Sneak and Show 60 (seats by seed), constructed play's table,
//! every check of the default mode, and with 200 games or more Doomsday and Show and Tell must have
//! been cast. Matches: constructed best-of-three between the real 75s (`data/decks/`: each main and
//! the sideboard cards this checkout supports, the rest named in the output) of every pair with
//! Doomsday, Sneak and Show or Hogaak - Doomsday v Sneak and Show, Doomsday v Delver, Sneak and Show v
//! Delver, Hogaak v Delver, Hogaak v Doomsday, Hogaak v Sneak and Show, each in both seats, and the
//! three mirrors (`CROSS_PAIRS`, by `seed % 15`; the Hogaak pairs since ENGINE.md 220.4), one match
//! in four 12-turn games - then as many post-sideboard matches, every check of `FUZZ_POOL=delver`'s
//! matches.
//! `FUZZ_POOL=meta` (ENGINE.md 176): the metagame spec of the three real 75s (Delver, Doomsday, Sneak and Show; a
//! `"metagame"` spec, uniform over the 9 ordered pairings), each game's pairing the engine's draw from its seed. Games:
//! the best-of-one metagame's game of the seed (the pairing's mains) with every check of the default mode; matches:
//! its constructed best-of-three (the 75s; odd seeds one episode per game, one match in four 12-turn games) with every
//! check of `FUZZ_POOL=delver`'s matches, then post-sideboard matches of the mirrors-only metagame (`"weights"` on the
//! diagonal, `"start_game": 2`; no keeper: the metagame refuses `keep_deck`). Before each game and match its pairing
//! check (`meta_lockstep`): the metagame spec's game and the game of its pairing's fixed spec (`Metagame::pair_json`)
//! played in lockstep from the seed by random actions - the same seats, decider, legal actions and chance keys at
//! every step, both viewers' whole layout v7 rows (every third step), and every eighth step both viewers' worlds
//! (`determinize`: the same pools, decks, hand and library identities, the viewer's row; in a best-of-one game the
//! world's cards of the opponent are exactly its pairing's main), the record (`record_setup`: the fixed game's decks
//! and the pairing) and the result: everything that depends on a deck is this game's pairing's. The worlds of the
//! matches' checks keep the pairing (`check_constructed_world`). With 200 games (90 matches, 20 post-sideboard
//! matches) or more every cell (the post-sideboard matches: every mirror) must have been played.
//!
//! `FUZZ_POOL=meta_pair` (ENGINE.md 240): `meta` with a stew specialist's sideboarding settings in its matches - each
//! seat dealt from its own 75 (`"dealt_decks"` per seat: by seed p 0.85 with swaps [0, 15], p 1 with [0, 15], or p 0.5
//! with [2, 6]), a choice table per deck in two matches of three (each deck's own: random legal 60s of its 75, in its
//! metagame entry, so it follows the deck to its seat), and the forced find (Doomsday for Thassa's Oracle at p 1, 0.5
//! or 0.25 by seed) - in both its whole matches and its mirrors' post-sideboard matches; the games are `meta`'s. Every
//! check of `meta`'s matches, the lockstep with each pairing's fixed match included (now with the dealt seats, the
//! seats' tables - played as teacher decisions - and the forced find passed through `Metagame::pair_json`), every
//! dealt 60 within the range of its own 75, every table seat's 60 its own deck's table's for its context; with 100
//! matches or more table decisions, dealt 60s and Doomsday resolutions (the forced find's records) must have been seen.
//!
//! `FUZZ_POOL=meta_wheel` (ENGINE.md 180): the forced find. Games only: the best-of-one metagame of the three real mains
//! with a Doomsday deck in every game and `"forced_find"` (Doomsday, Thassa's Oracle; p 1, 0.5 or 0.25 by seed,
//! `unless_in_hand` false one seed in eight). Each game is played twice: by the default mode's loop on its `Game` (the
//! forced find in its `GameSpec`; every check of the default mode, the information-set walks and the replay among them),
//! and first by `wheel_walk` on the spec's `MtgAz` game: both viewers' rows equal those of the same state without the
//! option (every third step and at every pick), every world of either viewer keeps the draw and the records at a pick,
//! the decider's worlds take the real pick to the same keys and records, no pile decision with one option reaches the
//! player, each record's class agrees with its pile, its draw and (where the library is the pile) the library, and the
//! game replays to the same state and records; zero-core's value-only decisions (`value_only_decision`, ENGINE.md
//! 180.12) are exactly a drawn resolution's decisions (all of a resolution's alike, as its record says; none outside a
//! resolving Doomsday). With 200 games or more forced and missed resolutions, value-only decisions and resolution
//! decisions with their policy must have been seen and worlds, rows and libraries compared.
//!
//! The information-set walks (ENGINE.md 148): two worlds `determinize` deals for a viewer, played on
//! the same random actions while their keys, next decider and legal actions agree (zero-core's child
//! signature), must agree on what the viewer sees: in games with open decklists the view and the
//! viewer's whole layout v7 row (head, tail tokens, pointer overrides), for both viewers; in matches
//! and hidden-decklist games the viewer's encoding. Equal keys also mean the game (in matches: the
//! match and the episode) is over in both worlds or in neither. Each kind of walk counts the states it
//! compared after a common action and prints them; the games' run (200 games or more) and the
//! matches' (20 or more) need some.
//! `FUZZ_POOL=eoe` (ENGINE.md 99): Edge of Eternities sealed, the set's own
//! card table (`CardDb::eoe`): random 40-card decks of its supported cards
//! (17 lands, 23 spells; games only, no matches).
//! `FUZZ_POOL=sealed` (ENGINE.md 101.6): Edge of Eternities sealed decks as the `mtg` spec's
//! `"sealed"` key makes them: each game's two pools generated from its seed (the `arena`
//! collation for even seeds, `paper` for odd ones), built by the randomised builder (40 cards,
//! the rest of the pool the sideboard), on the EoE card table; every check of the default mode.
//! A pool holding a card this checkout doesn't support (a loud error of the spec, never a dropped
//! card) skips its game, counted and named at the end. Games only (sealed is best-of-one).
//! `FUZZ_BEST_SHARE=p` (ENGINE.md 107.1): the sealed spec's mixed field, `"best_share": p` (each
//! seat's deck the builder's best guess with probability p; with `FUZZ_HIDDEN_DECKS=1` the prior
//! samples the same mixture). Unset: the spec without the key, unchanged.
//! `FUZZ_DECK_CHOICE=k` (with `FUZZ_POOL=sealed FUZZ_HIDDEN_DECKS=1`; ENGINE.md 107.2): each game is the sealed
//! spec's with `"deck_choice": {"k": k, "explore": 0.1}`, made by `MtgAz::new`: its pre-game deck choice is played
//! first - at each choice, worlds of the decider and of the other player (the decision, its options and the
//! viewer's whole encoding as in the real game; in the decider's world the other seat chose a deck of 40 cards
//! of its prior's support; one choice in the world starts the game, 30 random moves on), then a random choice -
//! and the game goes on with every check of the hidden mode.
//! `FUZZ_DECK_CHOICE_BY=<file>` (with `FUZZ_DECK_CHOICE=k`; ENGINE.md 112): the deck choice by an evaluator instead -
//! `"by": "evaluator", "evaluator": <file>` (an exported `mtgz.sealed.scorer` file): no decision; each seat's deck is
//! checked to be its explore pick or the evaluator's best candidate (recomputed here from the spec's candidates and the
//! scorer), with the evaluator's sha256 and score in `how`, and the opponent's worlds are dealt from each seat's own
//! prior, which chooses as the seat does (`SealedPrior::of_seat`, named `sealed+evaluator`). It prints the chosen and
//! exploring seats.
//! An unknown `FUZZ_POOL` value fails. Unset: the default mode, unchanged.
//!
//! Layout v7's rows (ENGINE.md 125), at every decision of a game: the decider's whole row (head,
//! tail, pointer overrides) is bounded, its head is `encode_features`' when it has no tail, its v6
//! projection is layout v6's encoding (`mtg_eoe`'s path) but on a page v6 showed as a window, and
//! every option of the decision points at the token of the object it names
//! (`tail_encode::check_pointers`: static slots, tail tokens, overrides on pages and for exile
//! offers); rows past `max_row_tokens` are counted. `FUZZ_CAP_CENSUS=1`: per pool, both viewers'
//! rows at every decision, how often each capped segment passes its head slots, the rows with each
//! tail-only segment's tokens, the tokens per row (layout v6's present tokens and v7's total; mean,
//! p99, max), as JSON (`FUZZ_CAP_CENSUS_OUT`, default `fuzz_cap_census_<pool>.json`).
//!
//! `FUZZ_HIDDEN_DECKS=1` (ENGINE.md 102): the mode's games (the default decks, `FUZZ_POOL=eoe`'s or
//! `FUZZ_POOL=sealed`'s; refused with `sneak_show`, `sns` and `delver_sb`, whose Show and Tell pick the
//! table's prior cannot condition a deck on, ENGINE.md 129.3) with hidden decklists, played through `MtgAz` (`MtgAz::from_seats`, the
//! table's `TablePrior` for both decks; with `FUZZ_POOL=sealed` the spec's `SealedPrior`, 102.6): every check of a game (the decider's encoding through `MtgAz::encode`) and at
//! each checkpoint `MtgAz::determinize` for both viewers - the decider's actions, the viewer's
//! view and whole encoding as in the real game, no known card dealt again, the opponent's sampled
//! deck of the public size holding every card the viewer saw and equal to the match's, the
//! viewer's own cards, invariants and clouds, every option completes, the information-set check
//! (two worlds, the same actions), 30 moves on. Random play on the base loops on free
//! activations (ENGINE.md 100.5.1, seed 1024: fixed by the board-reading loop rule on
//! `eoe-f7-numbers`, not on this branch), so past 500 decisions in one turn the random player
//! passes priority whenever it may (counted). Games only; it prints the worlds and those with
//! another deck than the real one (with 200 games or more, most must).
//!
//! `FUZZ_BELIEF=t` (ENGINE.md 69): at every checkpoint also the belief-weighted deal
//! (`Game::determinize_weighted`, `MtgAz::determinize_belief`) for both viewers, with random weights
//! sharpened by the tilt t (clip 1e6): every check of `determinize`'s world (the decider's actions and
//! view, known cards, invariants, clouds, every owner's cards, both encodings bounded, every option
//! completes, 30 moves on), and at the end the deal's concentration: the most favoured identity of the
//! opponent's hidden pool is dealt into the hand more than uniformly. Its own generator: the games are
//! the same with or without it.
//!
//! `FUZZ_DECK_TILT=t` (ENGINE.md 106; with `FUZZ_HIDDEN_DECKS=1`): each checkpoint's hidden-decklist
//! worlds are dealt by `MtgAz::determinize_with` with a random prediction of both targets (weights
//! (1..1000)^3, normalized), the deck's tilt t (`belief_tilts.deck`) and on every other check the hand's
//! too, clip 1e6 - every check of the mode's world (actions, view, whole encoding, known cards, the
//! deck's size and shown cards, the match's deck, own cards, invariants, every option completes, the
//! information-set check, 30 moves on), plus: the world's deck is one of the K cached decks, and at the
//! end the dealt decks' probability against the uniform pick's (K x p of the dealt deck, averaged: 1
//! untilted, K / ESS tilted). The prediction from its own generator: the games are the same.
//!
//! `FUZZ_CHANCE=n` (ENGINE.md 74): one decision in `n` (its own generator) runs a chance walk,
//! what zero-core's search does at `chance_k` > 0: two worlds of the decider (each dealt as a
//! simulation's, uniform or by random weights) walk one random path while they are at one node of
//! the tree, one taking each step freely and the other replaying its keys (`force_chance`, a
//! search transition: single-option decisions answered), the roles swapping at random; a replaying
//! world that leaves the node plays on alone for 20 steps. Every world's invariants and knowledge
//! hold after every step, the legal actions stay sorted and unique, every option of a replaying
//! world's decision completes (always when the force changed its outcome), and a panic is reported
//! with the walk's path and forced keys. It prints the walks, their steps, the replays whose
//! outcome the force changed and the walks split by the keys; with 100 games or more some replay
//! must have taken a forced outcome. Unset: none (the games are the same either way).
//!
//! Unbounded numbers (M6, ENGINE.md 46): at every decision the decider's
//! encoding (and at the ~1-in-40 checkpoints - ~1-in-4 under the `fuzz` Cargo profile,
//! `checkpoint_every`, ENGINE.md 70 - both players' encodings, in
//! the game and in every world `determinize` samples) has every value finite
//! and within `encode::FEATURE_BOUND` (256); matches check `MtgAz`'s
//! encoding the same way. `FUZZ_POOL=exponential`: exponential boards. Each
//! player's 23 spells come from `GROWTH` (counter and token doublers,
//! Ouroboroid, Bristly Bill, Scythecat Cub, Jacked Rabbit, Elspeth's token
//! doubling, and the cards that turn a creature's size into damage, life or
//! poison), 12 Forests and 5 random lands; every life total starts at
//! 10^18 (near the end of an amount's exact range, `i64::MAX`) and attacks
//! are declared one time in four, so most games run to the 60-turn cap with
//! growing boards, or end aborted when a number the game keeps passes its
//! exact range (ENGINE.md 100.7: a legitimate end, with no result). Every
//! mode prints the largest power, counter count, marked damage and life
//! reached and the aborted games; with 100 games or more the exponential
//! mode must have reached a board past `u32::MAX` power (exact: the engine's
//! numbers were 32-bit before F7).

use crate::db::CardDb;
use crate::rng::Rng;
use crate::game::EndReason;
use crate::{Choice, DecisionKind, Game, GameSpec, Outcome, NUM_ACTIONS};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use super::{Hooks, Run, SeedOutcome, Totals};
use crate::rules_check::{MatchCheck, RulesCheck};

thread_local! {
    /// The rules checks' counts of this test (ENGINE.md 150; each test runs on a thread of its
    /// own): each game's, match's and world's `RulesCheck` adds its own as it ends.
    static RULES: std::cell::RefCell<crate::rules_check::Stats> = std::cell::RefCell::new(Default::default());
}

fn rules_add(s: &crate::rules_check::Stats) {
    RULES.with(|r| r.borrow_mut().add(s));
}

fn rules_report() -> String {
    format!("rules checks (ENGINE.md 150, CR {}): {}", crate::rules_check::CR_EDITION, RULES.with(|r| *r.borrow()))
}

/// A game's last log lines (for a rules check's failure).
fn log_tail(g: &Game, n: usize) -> String {
    g.log_lines().iter().rev().take(n).rev().cloned().collect::<Vec<_>>().join("\n")
}

thread_local! {
    /// The rules checks' violations of this test (ENGINE.md 150): (rule, where and what, the
    /// game's log). A violation is recorded and its game checked no further (`RulesCheck` stops);
    /// the test plays on and fails at its end (`rules_verdict`), so one run counts every
    /// violation of its seeds.
    static RULES_FAILS: std::cell::RefCell<Vec<(&'static str, String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The rules checks' violations recorded on this thread and not yet reported, each "rule: where and what", taken
/// (tests that play a seed with `play` / `play_hidden`, whose violations wait for the run's verdict: ENGINE.md 209).
pub fn take_rules_violations() -> Vec<String> {
    RULES_FAILS.with(|f| std::mem::take(&mut *f.borrow_mut())).into_iter().map(|(r, w, _)| format!("{r}: {w}")).collect()
}

/// The rules checks' counts added on this thread so far (`rules_add`).
pub fn rules_stats() -> crate::rules_check::Stats {
    RULES.with(|r| *r.borrow())
}

fn rules_fail(e: crate::rules_check::Violation, what: String, log: String) {
    RULES_FAILS.with(|f| f.borrow_mut().push((e.rule, format!("{what}: {e}"), log)));
}

/// The rules checks of a step (ENGINE.md 150): a violation is recorded with `what` and the
/// game's log.
fn rules_step(rc: &mut RulesCheck, g: &Game, what: &dyn Fn() -> String) {
    if let Err(e) = rc.step(g) {
        rules_fail(e, what(), log_tail(g, 60));
    }
}

/// The rules checks' violations by rule (each one's first line), and the test's verdict: it fails
/// if any rule outside `FUZZ_RULES_ALLOW` (comma-separated rule numbers, as the violations name
/// them; `*`: every rule, so a campaign's chunk plays to its end and lists them) was violated.
fn rules_verdict(test: &str) {
    let allow: Vec<String> = super::env::var("FUZZ_RULES_ALLOW").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let fails = RULES_FAILS.with(|f| std::mem::take(&mut *f.borrow_mut()));
    if fails.is_empty() {
        println!("{test}: no rules check violated");
        return;
    }
    let mut by: Vec<(&'static str, Vec<&(&'static str, String, String)>)> = Vec::new();
    for f in &fails {
        match by.iter_mut().find(|x| x.0 == f.0) {
            Some(x) => x.1.push(f),
            None => by.push((f.0, vec![f])),
        }
    }
    let mut failing = Vec::new();
    for (rule, list) in &by {
        let allowed = allow.iter().any(|a| a == rule || a == "*");
        println!("{test}: CR {rule} violated {} times{}; first: {}", list.len(), if allowed { " (allowed: FUZZ_RULES_ALLOW)" } else { "" }, list[0].1);
        for x in list.iter().skip(1) {
            println!("    also: {}", x.1.lines().next().unwrap_or(""));
        }
        println!("    its log:\n{}", list[0].2);
        if !allowed {
            failing.push(format!("CR {rule} x{}", list.len()));
        }
    }
    assert!(failing.is_empty(), "{test}: rules checks violated: {}", failing.join(", "));
}

/// [`rules_step`] over a match's games.
fn rules_match_step(mc: &mut MatchCheck, a: &crate::az::MtgAz, what: &dyn Fn() -> String) {
    if let Err(e) = mc.step(a) {
        rules_fail(e, what(), log_tail(&a.g, 60));
    }
}

/// The interaction groups of `FUZZ_POOL=interactions` (ENGINE.md 38.5; cube
/// cards only, so matches can use them; a misspelt name fails loudly).
const GROUPS: &[(&str, &[&str])] = &[
    ("copies x enter and leave abilities", &[
        "Phyrexian Metamorph", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Saheeli, Sublime Artificer",
        "Superior Spider-Man", "Fractured Identity", "Springheart Nantuko", "Esika's Chariot", "Vaultborn Tyrant",
        "Thundertrap Trainer", "Sicarian Infiltrator", "Archon of Cruelty", "Grief", "Solitude", "Fury", "Endurance",
        "Subtlety", "Orcish Bowmasters", "Haywire Mite", "Skyclave Apparition", "Flametongue Kavu", "Baleful Strix",
        "Thraben Inspector", "Shifting Woodland", "Seasoned Pyromancer", "Enduring Innocence", "Woodfall Primus",
        "Three Steps Ahead", "Emeritus of Abundance // Regrowth", "Sage of the Skies",
    ]),
    ("flicker x linked exile, the new object", &[
        "Flickerwisp", "Phelia, Exuberant Shepherd", "Palace Jailer", "Leyline Binding", "Portable Hole",
        "Touch the Spirit Realm", "Displacer Kitten", "Skyclave Apparition", "Oust", "Elite Spellbinder",
        "Tidehollow Sculler", "Parallax Wave", "Containment Priest", "Unexpectedly Absent", "Swords to Plowshares",
        "Path to Exile", "Get Lost", "Council's Judgment", "Brazen Borrower // Petty Theft", "Memory Jar",
        "Phyrexian Metamorph", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Solitude", "Grief",
    ]),
    ("reanimation x replacement effects", &[
        "Reanimate", "Animate Dead", "Exhume", "Necromancy", "Shallow Grave", "Corpse Dance", "Goryo's Vengeance",
        "Unearth", "Entomb", "Faithless Looting", "Frantic Search", "Containment Priest", "Dauthi Voidwalker",
        "Archon of Cruelty", "Griselbrand", "Atraxa, Grand Unifier", "Uro, Titan of Nature's Wrath",
        "Phlage, Titan of Fire's Fury", "Woodfall Primus", "Emrakul, the Aeons Torn", "Sneak Attack",
        "Through the Breach", "Blightsteel Colossus", "Worldspine Wurm", "Yawgmoth's Will", "Underworld Breach",
        "Hullbreacher", "Quantum Riddler", "Sword of the Meek", "Thopter Foundry",
    ]),
    ("face-down x copies, flicker, zones", &[
        "Abhorrent Oculus", "Cloak and Dagger, Entwined", "Headliner Scarlett", "Shelldock Isle", "Memory Jar",
        "Phyrexian Metamorph", "Containment Priest", "Flickerwisp", "Oust", "Swords to Plowshares",
        "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Saheeli, Sublime Artificer", "Oko, Thief of Crowns",
        "Tishana's Tidebinder", "Displacer Kitten", "Fractured Identity", "Dauthi Voidwalker", "Skyclave Apparition",
    ]),
    ("double-faced cards, Adventures x copies, zones, mana value", &[
        "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Concealing Curtains // Revealing Eye",
        "Cecil, Dark Knight // Cecil, Redeemed Paladin", "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
        "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar", "Etali, Primal Conqueror // Etali, Primal Sickness",
        "Outland Liberator // Frenzied Trapbreaker", "Ulvenwald Oddity // Ulvenwald Behemoth",
        "Jennifer Walters // The Sensational She-Hulk", "Shatterskull Smashing // Shatterskull, the Hammer Pass",
        "Sink into Stupor // Soporific Springs", "Witch Enchanter // Witch-Blessed Meadow", "Brazen Borrower // Petty Theft",
        "Embereth Shieldbreaker // Battle Display", "Emeritus of Abundance // Regrowth", "Norman Osborn // Green Goblin",
        "Life // Death", "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger", "Aang, Swift Savior // Aang and La, Ocean's Fury",
        "Phyrexian Metamorph", "Flickerwisp", "Oust", "Displacer Kitten", "Phelia, Exuberant Shepherd",
        "Saheeli, Sublime Artificer", "Cut Down", "Abrupt Decay", "Fatal Push",
    ]),
    ("counterspells x copies of spells, split, modal, X", &[
        "Counterspell", "Mana Leak", "Memory Lapse", "Remand", "Force of Will", "Force of Negation", "Daze",
        "Mana Drain", "Spell Pierce", "No More Lies", "Stern Scolding", "We Say Thee Nay!", "Lutri, the Spellchaser",
        "Chain Lightning", "Three Steps Ahead", "Mystic Confluence", "Fiery Confluence", "Invert Polarity",
        "Tishana's Tidebinder", "Ertai Resurrected", "Subtlety", "Witherbloom Apprentice", "Sedgemoor Witch",
        "Lose Focus", "Brain Freeze", "Chain of Smog", "Pyroblast", "Kolaghan's Command", "Collective Brutality",
        "Snapcaster Mage", "Flashback", "Fire Covenant", "Pyrokinesis", "Life // Death", "Prismatic Ending",
        "Green Sun's Zenith", "Walking Ballista",
    ]),
    ("control changes x you and its controller", &[
        "Fractured Identity", "Oko, Thief of Crowns", "Dack Fayden", "Thieving Skydiver", "Invert Polarity",
        "Coveted Jewel", "Palace Jailer", "Forth Eorlingas!", "Aragorn, King of Gondor", "Emeritus of Abundance // Regrowth",
        "Hullbreacher", "Ragavan, Nimble Pilferer", "Sol Ring", "Mana Vault", "Lightning Greaves", "Skullclamp",
        "Smuggler's Copter", "Esika's Chariot", "Bolas's Citadel", "Phyrexian Metamorph",
    ]),
    ("taxes, alternative and additional costs x free casts", &[
        "Thalia, Guardian of Thraben", "Etali, Primal Conqueror // Etali, Primal Sickness", "Bolas's Citadel",
        "Dauthi Voidwalker", "Flash", "Force of Will", "Daze", "Solitude", "Fury", "Subtlety", "Grief", "Snapcaster Mage",
        "Yawgmoth's Will", "Underworld Breach", "Lurrus of the Dream-Den", "Zirda, the Dawnwaker", "Mishra's Workshop",
        "Arena of Glory", "Bloodbraid Challenger", "Prismatic Ending", "Walking Ballista", "Malcolm, Alluring Scoundrel",
        "Through the Breach", "Sneak Attack", "Gitaxian Probe", "Dismember", "Mana Tithe", "Spell Pierce",
        "Delighted Halfling", "Lotus Petal", "Black Lotus", "Chrome Mox", "Mox Diamond", "Pillage the Bog",
        "Fallen Shinobi", "Robber of the Rich", "Tersa Lightshatter", "Laelia, the Blade Reforged", "Golos, Tireless Pilgrim",
        "Abhorrent Oculus", "Fire Covenant", "Toxic Deluge",
    ]),
    ("damage x protection, deathtouch, infect, divided damage", &[
        "Giver of Runes", "Mother of Runes", "True-Name Nemesis", "Sylvan Safekeeper", "Lightning Greaves",
        "Emrakul, the Aeons Torn", "Fury", "Pyrokinesis", "Fire Covenant", "Walking Ballista", "Lightning Bolt",
        "Chain Lightning", "Burst Lightning", "Galvanic Discharge", "Sheoldred, the Apocalypse", "Screaming Nemesis",
        "Blightsteel Colossus", "Goblin Bombardment", "Kappa Cannoneer", "Hexdrinker", "Pyrogoyf", "Fiery Confluence",
        "Wizard's Rockets", "Kolaghan's Command", "Suplex", "Skullclamp", "Bone Shards",
        "Shatterskull Smashing // Shatterskull, the Hammer Pass", "Questing Beast", "Kaldra Compleat",
    ]),
    ("planeswalkers x damage, copies, control", &[
        "Oko, Thief of Crowns", "Dack Fayden", "Jace, the Mind Sculptor", "Teferi, Hero of Dominaria", "Teferi, Time Raveler",
        "Chandra, Torch of Defiance", "Karn, Scion of Urza", "Narset, Parter of Veils", "Nissa, Who Shakes the World",
        "Wrenn and Six", "Minsc & Boo, Timeless Heroes", "Tamiyo, Collector of Tales", "The Wandering Emperor",
        "Elspeth, Storm Slayer", "Ugin, Eye of the Storms", "Tezzeret, Cruel Captain", "Kaito, Bane of Nightmares",
        "Saheeli, Sublime Artificer", "Fury", "Lightning Bolt", "Questing Beast", "Phyrexian Metamorph",
        "Fractured Identity", "Clarion Conqueror", "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar",
        "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
    ]),
    ("last known information, triggers and state-based actions", &[
        "Pyrogoyf", "Jacked Rabbit", "Ouroboroid", "Orcish Bowmasters", "Woodfall Primus", "Sword of the Meek",
        "Thopter Foundry", "Skullclamp", "Laelia, the Blade Reforged", "Nadu, Winged Wisdom", "Hullbreacher",
        "Quantum Riddler", "Sheoldred, the Apocalypse", "Leovold, Emissary of Trest", "Wan Shi Tong, All-Knowing",
        "Inti, Seneschal of the Sun", "Currency Converter", "Ivora, Insatiable Heir", "Bloodtithe Harvester",
        "Carnage Interpreter", "Forensic Gadgeteer", "Generous Plunderer", "Screaming Nemesis", "Walking Ballista",
        "Toxic Deluge", "Balance", "Wrath of God", "Damn", "Sunfall", "Goblin Bombardment", "Swords to Plowshares",
        "Fatal Push", "Tishana's Tidebinder",
    ]),
    ("combat: first and double strike, trample, deathtouch, protection", &[
        "Questing Beast", "True-Name Nemesis", "Ragavan, Nimble Pilferer", "Adeline, Resplendent Cathar",
        "Mother of Runes", "Giver of Runes", "Kaldra Compleat", "Blightsteel Colossus", "Swords to Plowshares",
        "Lightning Greaves", "Skullclamp", "Nettlecyst", "Kappa Cannoneer", "Hexdrinker", "Figure of Destiny",
        "Smuggler's Copter", "Esika's Chariot", "Dauthi Voidwalker", "Dragon's Rage Channeler", "Gut, True Soul Zealot",
        "Okoye, Mighty and Adored", "Surrak, Elusive Hunter", "Coveted Jewel", "Thalia, Guardian of Thraben",
        "Archon of Emeria", "Ocelot Pride", "Guide of Souls", "Luminarch Aspirant",
    ]),
    ("day and night, transform x copies", &[
        "Outland Liberator // Frenzied Trapbreaker", "Ulvenwald Oddity // Ulvenwald Behemoth",
        "Concealing Curtains // Revealing Eye", "Cecil, Dark Knight // Cecil, Redeemed Paladin",
        "Jace, Vryn's Prodigy // Jace, Telepath Unbound", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
        "Phyrexian Metamorph", "Saheeli, Sublime Artificer", "Flickerwisp", "Oust", "Swords to Plowshares",
        "Etali, Primal Conqueror // Etali, Primal Sickness", "Brainstorm", "Ponder", "Lightning Bolt",
    ]),
    // (ENGINE.md 49: castability and payment agree.)
    ("mana abilities x costs: converters, tapped artifacts, improvise, restricted mana, graveyard costs, X", &[
        "Wizard's Rockets", "Urza, Lord High Artificer", "Kappa Cannoneer", "Mishra's Workshop", "Delighted Halfling",
        "Lotus Petal", "Black Lotus", "Lion's Eye Diamond", "Psychic Frog", "Abhorrent Oculus", "Treasure Cruise",
        "Dig Through Time", "Uro, Titan of Nature's Wrath", "Green Sun's Zenith", "Walking Ballista", "Griselbrand",
        "Cryogen Relic", "Esika's Chariot", "Sunken Ruins", "Chromatic Star", "Pentad Prism", "Gemstone Mine",
        "Squandered Resources", "Orcish Lumberjack", "Arena of Glory", "Rain of Filth", "Badgermole Cub",
        "Nissa, Who Shakes the World", "Sol Ring", "Mana Vault", "Grim Monolith", "Basalt Monolith",
        "Talisman of Dominance", "Elvish Spirit Guide", "Simian Spirit Guide", "Mana Confluence", "Ancient Tomb",
        "Loot, the Pathfinder", "Gaea's Cradle", "Coalition Relic", "Mox Opal", "Mox Amber",
        "Aang, Swift Savior // Aang and La, Ocean's Fury", "Tinker", "Thalia, Guardian of Thraben", "Bolas's Citadel",
    ]),
];

/// The cards of `FUZZ_POOL=exponential` (ENGINE.md 46): what grows a board
/// without bound (counters on each creature equal to a power, doubling
/// counters, tokens equal to a power, token doubling, counters each turn) and
/// what turns a creature's size into damage, life, poison or mana.
const GROWTH: &[&str] = &[
    "Ouroboroid", "Bristly Bill, Spine Sower", "Scythecat Cub", "Jacked Rabbit", "Elspeth, Storm Slayer", "Baloth Prime",
    "Badgermole Cub", "Hexdrinker", "Walking Ballista", "Luminarch Aspirant", "Guide of Souls", "Ivora, Insatiable Heir",
    "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger", "Forth Eorlingas!", "Pest Infestation", "Sentinel of the Nameless City",
    "Vaultborn Tyrant", "Fanatic of Rhonas", "Esika's Chariot", "Thundertrap Trainer", "Sylvan Safekeeper", "Surrak, Elusive Hunter",
    "Swords to Plowshares", "Solitude", "Minsc & Boo, Timeless Heroes", "Etali, Primal Conqueror // Etali, Primal Sickness",
    "Pyrogoyf", "Batroc the Leaper", "Cecil, Dark Knight // Cecil, Redeemed Paladin", "Coalition Relic", "Griselbrand",
    "Ocelot Pride", "Deep-Cavern Bat", "Passageway Seer", "The One Ring", "Tezzeret, Cruel Captain",
];

/// `FUZZ_POOL=exponential`: every player's starting life (near the end of an
/// amount's exact range, `num::AMOUNT_MAX` = `i64::MAX`: life gained past it
/// ends the game aborted, ENGINE.md 100.7).
const EXP_LIFE: i64 = 1_000_000_000_000_000_000;

/// `FUZZ_POOL`: unset (the default decks), `interactions`, `exponential` or `drafted`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PoolMode {
    Default,
    Interactions,
    Exponential,
    Drafted,
    Delver,
    DdB,
    Eoe,
    Sealed,
    DdA,
    /// `FUZZ_POOL=dd_c` (ENGINE.md 124.3): the Legacy Doomsday 60 with lane C's
    /// cards (Edge of Autumn, Street Wraith, Leyline of the Void) against the
    /// Delver 60 and itself.
    DoomsdayC,
    /// `FUZZ_POOL=doomsday` (ENGINE.md 124.4): the real Legacy Doomsday 60
    /// against the Delver 60 and itself.
    Doomsday,
    /// `FUZZ_POOL=sneak_show` (ENGINE.md 123): the real Legacy Sneak and Show
    /// 60 against the Delver 60 and itself.
    SneakShow,
    /// `FUZZ_POOL=sns` (ENGINE.md 123): the lane's six cards four each.
    Sns,
    /// `FUZZ_POOL=hogaak_a` (ENGINE.md 220.1): Hogaak, Arisen Necropolis four
    /// with the Hogaak deck's supported cards and enablers.
    HogaakA,
    /// `FUZZ_POOL=hogaak_b` (ENGINE.md 220.2): Mockingbird and Massacre four
    /// each against the Delver 60 and the mirror.
    HogaakB,
    /// `FUZZ_POOL=names` (ENGINE.md 127): Anointed Peacekeeper and Disruptor
    /// Flute with the table's cards of more than one face.
    Names,
    /// `FUZZ_POOL=delver_sb` (ENGINE.md 122): the Delver sideboard's five
    /// cards in the Delver 60.
    DelverSb,
    /// `FUZZ_POOL=hogaak_c` (ENGINE.md 220.3): the Legacy Hogaak Tempo deck's
    /// lane C cards four each against the Delver 60 and itself.
    HogaakC,
    /// `FUZZ_POOL=hogaak` (ENGINE.md 220.4): the Legacy Hogaak Tempo deck's real
    /// 60 against the Delver, Doomsday and Sneak and Show 60s and its mirror;
    /// best-of-three between its 75 and theirs.
    Hogaak,
    /// `FUZZ_POOL=lands_b` (ENGINE.md 270.2): the Legacy Lands deck's lane B
    /// cards four each against the Delver 60 and itself.
    LandsB,
    /// `FUZZ_POOL=cross` (ENGINE.md 148): the deck pairs no other pool plays -
    /// Doomsday against Sneak and Show, and best-of-three between the real 75s.
    Cross,
    /// `FUZZ_POOL=delver_dealt` (ENGINE.md 154): `delver` with dealt decks.
    DelverDealt,
    /// `FUZZ_POOL=delver_dealt2` (ENGINE.md 168.4): each seat dealt independently, and choice tables.
    DelverDealtPerSeat,
    /// `FUZZ_POOL=meta` (ENGINE.md 176): the metagame spec of the three real 75s.
    Meta,
    /// `FUZZ_POOL=meta_wheel` (ENGINE.md 180): the best-of-one metagame's Doomsday games with the forced find.
    MetaWheel,
    /// `FUZZ_POOL=meta_pair` (ENGINE.md 240): `meta` with per-seat dealt decks, per-deck tables and the forced find.
    MetaPair,
}

thread_local! {
    /// A test's own pool for its thread, over `FUZZ_POOL` (`enclosing_picks_of_etali_s_casts_are_encoded`).
    pub static POOL_OVERRIDE: std::cell::Cell<Option<PoolMode>> = const { std::cell::Cell::new(None) };
}

pub fn pool_mode() -> PoolMode {
    if let Some(m) = POOL_OVERRIDE.with(|c| c.get()) {
        return m;
    }
    match super::env::var("FUZZ_POOL").as_deref() {
        Err(_) | Ok("") => PoolMode::Default,
        Ok("interactions") => PoolMode::Interactions,
        Ok("exponential") => PoolMode::Exponential,
        Ok("drafted") => PoolMode::Drafted,
        Ok("delver") => PoolMode::Delver,
        Ok("dd_b") => PoolMode::DdB,
        Ok("eoe") => PoolMode::Eoe,
        Ok("sealed") => PoolMode::Sealed,
        Ok("dd_a") => PoolMode::DdA,
        Ok("dd_c") => PoolMode::DoomsdayC,
        Ok("doomsday") => PoolMode::Doomsday,
        Ok("sneak_show") => PoolMode::SneakShow,
        Ok("sns") => PoolMode::Sns,
        Ok("hogaak_a") => PoolMode::HogaakA,
        Ok("hogaak_b") => PoolMode::HogaakB,
        Ok("names") => PoolMode::Names,
        Ok("delver_sb") => PoolMode::DelverSb,
        Ok("hogaak_c") => PoolMode::HogaakC,
        Ok("hogaak") => PoolMode::Hogaak,
        Ok("lands_b") => PoolMode::LandsB,
        Ok("cross") => PoolMode::Cross,
        Ok("delver_dealt") => PoolMode::DelverDealt,
        Ok("delver_dealt2") => PoolMode::DelverDealtPerSeat,
        Ok("meta") => PoolMode::Meta,
        Ok("meta_wheel") => PoolMode::MetaWheel,
        Ok("meta_pair") => PoolMode::MetaPair,
        Ok(other) => panic!("FUZZ_POOL={other}: unknown (unset, \"interactions\", \"exponential\", \"drafted\", \"delver\", \"delver_dealt\", \"delver_dealt2\", \"dd_a\", \"dd_b\", \"dd_c\", \"doomsday\", \"sneak_show\", \"sns\", \"hogaak_a\", \"hogaak_b\", \"hogaak_c\", \"hogaak\", \"lands_b\", \"names\", \"delver_sb\", \"cross\", \"meta\", \"meta_pair\", \"meta_wheel\", \"eoe\" or \"sealed\")"),
    }
}

fn interactions_mode() -> bool {
    pool_mode() == PoolMode::Interactions
}

fn exponential_mode() -> bool {
    pool_mode() == PoolMode::Exponential
}

/// `FUZZ_POOL=delver`, or `delver_dealt` / `delver_dealt2` (its matches with dealt decks, ENGINE.md 154, 168.4).
pub fn delver_mode() -> bool {
    matches!(pool_mode(), PoolMode::Delver | PoolMode::DelverDealt | PoolMode::DelverDealtPerSeat)
}

/// `FUZZ_POOL=delver_dealt` or `delver_dealt2` (ENGINE.md 154, 168.4).
fn dealt_mode() -> bool {
    matches!(pool_mode(), PoolMode::DelverDealt | PoolMode::DelverDealtPerSeat)
}

/// `FUZZ_POOL=delver_dealt2` (ENGINE.md 168.4).
fn per_seat_mode() -> bool {
    pool_mode() == PoolMode::DelverDealtPerSeat
}

/// `FUZZ_POOL=delver_dealt2`'s choice table of match `seed` (ENGINE.md 168.4): the Delver 75 (`main`, `side`: names)
/// as its list, each of the 24 contexts a random legal 60 - the main with 0..=15 random one-for-one swaps.
fn fuzz_table(seed: u64, main: &[String], side: &[String]) -> serde_json::Value {
    let mut names: Vec<String> = Vec::new();
    let mut copies: Vec<u8> = Vec::new();
    let mut base: Vec<u8> = Vec::new();
    for (i, n) in main.iter().chain(side).enumerate() {
        let k = match names.iter().position(|x| x == n) {
            Some(k) => k,
            None => {
                names.push(n.clone());
                copies.push(0);
                base.push(0);
                names.len() - 1
            }
        };
        copies[k] += 1;
        if i < main.len() {
            base[k] += 1;
        }
    }
    let mut r = Rng::new(seed ^ 0x7AB1_E000_0168);
    let mut choices = Vec::new();
    for game in 2..=3u8 {
        for first in 0..2u8 {
            for lead in -1..=1i8 {
                for od in 0..2u8 {
                    let mut c = base.clone();
                    for _ in 0..r.below(16) {
                        let out: Vec<usize> = (0..c.len()).filter(|&k| c[k] > 0).collect();
                        let inn: Vec<usize> = (0..c.len()).filter(|&k| c[k] < copies[k]).collect();
                        let (o, i) = (out[r.below(out.len() as u32) as usize], inn[r.below(inn.len() as u32) as usize]);
                        if o != i {
                            c[o] -= 1;
                            c[i] += 1;
                        }
                    }
                    choices.push(serde_json::json!([game, first, lead, od, c]));
                }
            }
        }
    }
    let list: Vec<serde_json::Value> = names.iter().zip(&copies).map(|(n, k)| serde_json::json!([n, k])).collect();
    serde_json::json!({"version": format!("fuzz{seed}"), "list": list, "choices": choices})
}

/// `FUZZ_POOL=delver_dealt`'s dealt decks of match `seed` (ENGINE.md 154): by seed, p 0.75 with swaps [1, 8] (the
/// preset's range), p 1 with [0, 12] (P1's other 75 has 12 sideboard cards), p 0.5 with [3, 3].
fn dealt_of(seed: u64) -> serde_json::Value {
    if per_seat_mode() {
        // (ENGINE.md 168.4: each seat independently; [0, 12] where a seat plays OTHER_75's 12-card sideboard)
        let most = if seed % 4 == 1 { 12 } else { 15 };
        return match (seed / 4) % 3 {
            0 => serde_json::json!({"p": 0.85, "swaps": [0, most], "per_seat": true}),
            1 => serde_json::json!({"p": 1.0, "swaps": [0, 12], "per_seat": true}),
            _ => serde_json::json!({"p": 0.5, "swaps": [2, 6], "per_seat": true}),
        };
    }
    match (seed / 4) % 3 {
        0 => serde_json::json!({"p": 0.75, "swaps": [1, 8]}),
        1 => serde_json::json!({"p": 1.0, "swaps": [0, 12]}),
        _ => serde_json::json!({"p": 0.5, "swaps": [3, 3]}),
    }
}

/// The swaps of `deck` against `main` (card ids): its cards beyond the main's copies.
fn swaps_vs(main: &[u16], deck: &[u16]) -> usize {
    let mut m = main.to_vec();
    m.sort_unstable();
    deck.iter()
        .filter(|d| match m.binary_search(d) {
            Ok(i) => {
                m.remove(i);
                false
            }
            Err(_) => true,
        })
        .count()
}

/// A dealt match's checks (ENGINE.md 154): the registered main of seat `p` of spec `sp` in match `seed` (a metagame's:
/// the seat's pairing deck's, ENGINE.md 240), and the spec's swap range.
fn dealt_main(sp: &crate::az::MtgSpec, p: usize, seed: u64) -> (Vec<u16>, (usize, usize)) {
    let dd = sp.dealt_decks.expect("a dealt spec");
    let main = match &sp.decks {
        crate::az::DeckMode::ListsWithSideboards(x) => x[p].0.clone(),
        crate::az::DeckMode::Lists(a, b) => if p == 0 { a.clone() } else { b.clone() },
        crate::az::DeckMode::Metagame(_) => sp.seats(seed)[p].deck.clone(),
        _ => panic!("a dealt spec plays decklists"),
    };
    (main, (dd.swaps.0 as usize, dd.swaps.1 as usize))
}

/// `FUZZ_POOL=dd_a` (ENGINE.md 124.1).
fn dd_a_mode() -> bool {
    pool_mode() == PoolMode::DdA
}

/// `FUZZ_POOL=dd_b` (ENGINE.md 124.2).
fn dd_b_mode() -> bool {
    pool_mode() == PoolMode::DdB
}

/// Constructed play's table (`FUZZ_POOL=delver`, `dd_a`, `dd_b`, `dd_c`,
/// `doomsday`, `sneak_show`, `sns`, `hogaak_a`, `hogaak_b`, `hogaak_c`, `hogaak`, `lands_b`, `names`, `delver_sb`,
/// `cross` or `meta`).
pub fn constructed_mode() -> bool {
    delver_mode()
        || dd_a_mode()
        || dd_b_mode()
        || ddc_mode()
        || doomsday_mode()
        || sns_mode()
        || hogaak_a_mode()
        || hogaak_b_mode()
        || hogaak_c_mode()
        || hogaak_mode()
        || lands_b_mode()
        || names_mode()
        || delver_sb_mode()
        || cross_mode()
        || meta_mode()
        || wheel_mode()
}

/// `FUZZ_POOL=meta_wheel` (ENGINE.md 180).
pub fn wheel_mode() -> bool {
    pool_mode() == PoolMode::MetaWheel
}

/// `FUZZ_POOL=meta_wheel`'s spec of game `seed` (JSON): the best-of-one metagame of the three real mains with a
/// Doomsday deck in every game (the cells with Doomsday weight 1, the others 0) and the forced find of Doomsday for
/// Thassa's Oracle at p 1, 0.5 or 0.25 by seed (`unless_in_hand` false one seed in eight).
fn wheel_json(seed: u64) -> String {
    let p = [1.0, 0.5, 0.25][(seed % 3) as usize];
    let ff = serde_json::json!({"spell": "Doomsday", "card": "Thassa's Oracle", "p": p, "unless_in_hand": seed % 8 != 7});
    let mut v: serde_json::Value = serde_json::from_str(&meta_json(false, false, serde_json::json!({"forced_find": ff}))).unwrap();
    v["metagame"]["weights"] = serde_json::json!([[0, 1, 0], [1, 1, 1], [0, 1, 0]]);
    v.to_string()
}

/// `FUZZ_POOL=meta_wheel`'s counts of one game (`wheel_walk`).
#[derive(Default)]
struct WheelSeen {
    /// Resolutions by class (`FfClass` order: voluntary, forced, in hand, not findable, missed).
    classes: [u64; 5],
    /// Worlds whose draw and records were compared at a pick.
    worlds: u64,
    /// The decider's worlds stepped with the real pick (keys and records compared).
    stepped: u64,
    /// Rows compared with the same state without the option.
    rows: u64,
    /// Records whose pile was compared with the caster's library.
    libraries: u64,
    /// Decisions of a resolving Doomsday (its frame asking) that zero-core records without their policy target
    /// (`value_only_decision`, ENGINE.md 180.12: a drawn resolution's), and those it records as usual.
    value_only: u64,
    pile_free: u64,
}

/// `FUZZ_POOL=meta_wheel`'s walk of game `seed` (module docs, ENGINE.md 180): the metagame spec's game with the forced
/// find (`MtgAz`, as self-play makes it), played by random actions from the seed; every third step and at every pick
/// both viewers' rows equal those of the same state without the option; at every pick of a resolving Doomsday every
/// world of either viewer keeps the draw and the records, and the decider's worlds take the real pick to the same keys
/// and records; every new record's class agrees with its pile and draw (a forced or voluntary pile holds the Oracle,
/// nothing else does; forced only when drawn, missed only when not) and, where the library is the pile, with the
/// library; at the end the seed and actions replay to the same state and records.
fn wheel_walk(seed: u64) -> WheelSeen {
    use crate::az::tail_encode::Row;
    use crate::az::{MtgAz, MtgSpec};
    use crate::forced_find::FfClass;
    use zero_search::game::SearchGame;
    let spec = MtgSpec::parse(&wheel_json(seed)).unwrap_or_else(|e| panic!("seed {seed}: the wheel spec: {e}"));
    let ff = spec.forced_find.expect("the forced find");
    let mut a = MtgAz::new_game(&spec, 2, seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    let mut seen = WheelSeen::default();
    let mut rng = Rng::new(seed ^ 0xF0_4CED_0180_1);
    let (mut legal, mut keys, mut wkeys, mut actions) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut step = 0u64;
    // (ENGINE.md 180.12: the hook's answers at the running resolution's decisions, and what its record says they must be)
    let (mut frame_vo, mut expect): (Vec<bool>, Option<bool>) = (Vec::new(), None);
    let flush = |frame_vo: &mut Vec<bool>, expect: &mut Option<bool>, seen: &mut WheelSeen, what: &dyn Fn() -> String| {
        if frame_vo.is_empty() {
            return;
        }
        let e = expect.take().unwrap_or_else(|| panic!("{}: a resolution's decisions without its record", what()));
        assert!(frame_vo.iter().all(|&v| v == e), "{}: the resolution's decisions recorded value-only {frame_vo:?}, its record says {e}", what());
        if e {
            seen.value_only += frame_vo.len() as u64;
        } else {
            seen.pile_free += frame_vo.len() as u64;
        }
        frame_vo.clear();
    };
    let strip_rows = |a: &MtgAz, seen: &mut WheelSeen, what: &dyn Fn() -> String| {
        let mut s = a.clone();
        s.g.strip_forced_find();
        for v in 0..2 {
            assert!(Row::of_az_v8(a, v) == Row::of_az_v8(&s, v), "{}: P{v}'s row shows the forced find", what());
        }
        seen.rows += 1;
    };
    while let Some(p) = a.to_move() {
        let what = || format!("seed {seed} (FUZZ_POOL=meta_wheel) step {step}");
        a.legal_actions(&mut legal);
        let act = legal[rng.below(legal.len() as u32) as usize];
        // a pick of a resolving Doomsday's pile (its controller deciding a library or graveyard choice)
        let pick = a.g.pending_decision().is_some_and(|d| {
            matches!(d.kind, crate::DecisionKind::SearchLibrary | crate::DecisionKind::ChooseGraveyardCard)
                && a.g.stack().last().is_some_and(|s| s.controller == d.player && matches!(s.kind, crate::game::StackKind::Spell { card } if a.g.obj(card).def == ff.spell))
        });
        if step % 3 == 0 || pick {
            strip_rows(&a, &mut seen, &what);
        }
        // zero-core's value-only decisions (ENGINE.md 180.12): only a resolving Doomsday's (its frame asking), and all of
        // one resolution's the same - drawn, the pile holding the Oracle, none in the hand (the record's; checked at the
        // resolution's end)
        let vo = a.value_only_decision();
        let in_frame = matches!(a.g.agenda.last(), Some(crate::flow::Task::Run(f)) if f.source_def == ff.spell);
        assert!(!vo || in_frame, "{}: a value-only decision outside the spell's resolution", what());
        if in_frame {
            frame_vo.push(vo);
        } else {
            flush(&mut frame_vo, &mut expect, &mut seen, &what);
        }
        let before = a.g.forced_finds().len();
        keys.clear();
        let real_after = {
            let mut r = a.clone();
            r.step(act, &mut keys).unwrap_or_else(|e| panic!("{}: {e}", what()));
            r
        };
        if pick {
            assert!(legal.len() >= 2, "{}: a pile decision with one option reached the player (auto_single answers it)", what());
            for viewer in 0..2usize {
                for _ in 0..2 {
                    let mut w = a.clone();
                    w.determinize(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()));
                    assert_eq!(w.g.ff_draw(), a.g.ff_draw(), "{}: P{viewer}'s world draws otherwise", what());
                    assert_eq!(w.g.forced_finds(), a.g.forced_finds(), "{}: P{viewer}'s world's records", what());
                    strip_rows(&w, &mut seen, &what);
                    seen.worlds += 1;
                    if viewer == p {
                        let mut wl = Vec::new();
                        w.legal_actions(&mut wl);
                        assert_eq!(wl, legal, "{}: the decider's world's options", what());
                        wkeys.clear();
                        w.step(act, &mut wkeys).unwrap_or_else(|e| panic!("{}: the world's step: {e}", what()));
                        assert_eq!(wkeys, keys, "{}: the decider's world's keys (the forced answer's among them)", what());
                        let (wr, rr) = (w.g.forced_finds(), real_after.g.forced_finds());
                        assert_eq!(wr.len(), rr.len(), "{}: the decider's world's records", what());
                        for (x, y) in wr.iter().zip(rr) {
                            assert_eq!((x.player, x.drawn, x.class, &x.pile), (y.player, y.drawn, y.class, &y.pile), "{}: the decider's world's record", what());
                        }
                        seen.stepped += 1;
                    }
                }
            }
        }
        a = real_after;
        actions.push(act);
        for r in &a.g.forced_finds()[before..] {
            let held = r.pile.contains(&ff.card);
            assert_eq!(held, matches!(r.class, FfClass::Voluntary | FfClass::Forced), "{}: {r:?} against its pile", what());
            assert!(r.class != FfClass::Forced || r.drawn, "{}: forced without the draw: {r:?}", what());
            assert!(r.class != FfClass::Missed || !r.drawn, "{}: a drawn resolution missed the card: {r:?}", what());
            assert!(r.class != FfClass::InHand || ff.unless_in_hand, "{}: {r:?} with unless_in_hand false", what());
            if r.class == FfClass::InHand {
                assert!(a.g.player(r.player).hand.iter().any(|&c| a.g.obj(c).def == ff.card), "{}: in hand without the card in hand", what());
            }
            let lib = &a.g.player(r.player).library;
            if lib.len() == r.pile.len() && before + 1 == a.g.forced_finds().len() {
                let mut l: Vec<u16> = lib.iter().map(|&c| a.g.obj(c).def).collect();
                l.sort_unstable();
                assert_eq!(&l, &r.pile, "{}: the library is the record's pile", what());
                seen.libraries += 1;
            }
            let k = [FfClass::Voluntary, FfClass::Forced, FfClass::InHand, FfClass::NotFindable, FfClass::Missed].iter().position(|&c| c == r.class).unwrap();
            seen.classes[k] += 1;
            let in_hand = a.g.player(r.player).hand.iter().any(|&c| a.g.obj(c).def == ff.card);
            expect = Some(r.drawn && matches!(r.class, FfClass::Voluntary | FfClass::Forced) && !(ff.unless_in_hand && in_hand));
        }
        step += 1;
        assert!(step < 300_000, "seed {seed}: the wheel game does not end");
    }
    strip_rows(&a, &mut seen, &|| format!("seed {seed} (FUZZ_POOL=meta_wheel) the end"));
    flush(&mut frame_vo, &mut expect, &mut seen, &|| format!("seed {seed} (FUZZ_POOL=meta_wheel) the end"));
    // the seed and actions replay to the same state and records
    let mut b = MtgAz::new_game(&spec, 2, seed).unwrap();
    for &x in &actions {
        keys.clear();
        b.step(x, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: the replay: {e}"));
    }
    assert_eq!(b.g.forced_finds(), a.g.forced_finds(), "seed {seed}: the replay's records");
    assert_eq!(b.export_state(), a.export_state(), "seed {seed}: the replay's state");
    assert_eq!(b.result(), a.result(), "seed {seed}: the replay's result");
    seen
}

/// `FUZZ_POOL=meta` (ENGINE.md 176), or `meta_pair` (ENGINE.md 240: its matches with the per-deck settings).
pub fn meta_mode() -> bool {
    matches!(pool_mode(), PoolMode::Meta | PoolMode::MetaPair)
}

/// `FUZZ_POOL=meta_pair` (ENGINE.md 240).
pub fn pair_mode() -> bool {
    pool_mode() == PoolMode::MetaPair
}

/// `FUZZ_POOL=meta_pair`'s per-deck settings of match `seed` merged into `v` (a best-of-three metagame spec value): each
/// seat dealt from its own 75, a choice table per deck in two matches of three (random legal 60s of the deck's own 75:
/// `fuzz_table`), the forced find (module docs).
fn pair_settings(seed: u64, v: &mut serde_json::Value) {
    v["dealt_decks"] = match seed % 3 {
        0 => serde_json::json!({"p": 0.85, "swaps": [0, 15], "per_seat": true}),
        1 => serde_json::json!({"p": 1.0, "swaps": [0, 15], "per_seat": true}),
        _ => serde_json::json!({"p": 0.5, "swaps": [2, 6], "per_seat": true}),
    };
    let p = [1.0, 0.5, 0.25][((seed / 3) % 3) as usize];
    v["forced_find"] = serde_json::json!({"spell": "Doomsday", "card": "Thassa's Oracle", "p": p});
    if seed % 3 != 2 {
        for (k, d) in v["metagame"]["decks"].as_array_mut().unwrap().iter_mut().enumerate() {
            let names = |x: &serde_json::Value| -> Vec<String> { x.as_array().map_or(Vec::new(), |a| a.iter().map(|n| n.as_str().unwrap().to_string()).collect()) };
            let (main, side) = (names(&d["main"]), names(&d["sideboard"]));
            d["table"] = fuzz_table(seed ^ (k as u64) << 40, &main, &side);
        }
    }
}

/// `FUZZ_POOL=meta`'s metagame spec (JSON): the three real 75s (`real_75`, named by their files) - best-of-one their
/// mains, best-of-three (`bo3`) the 75s -, uniform, or with `mirrors` only the three mirrors (the post-sideboard
/// matches'); `extra` merged in.
fn meta_json(bo3: bool, mirrors: bool, extra: serde_json::Value) -> String {
    const FILES: [&str; 3] = ["izzet_delver_8016541", "doomsday_8027957", "sneak_show_8014220"];
    let decks: Vec<serde_json::Value> = (0..3)
        .map(|k| {
            let (m, s) = bo3_lists(&real_75(k));
            let mut e = serde_json::json!({"name": FILES[k], "main": m});
            if bo3 {
                e["sideboard"] = serde_json::json!(s);
            }
            e
        })
        .collect();
    let mut mg = serde_json::json!({"decks": decks});
    if mirrors {
        mg["weights"] = serde_json::json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]]);
    }
    let mut v = serde_json::json!({"max_turns": 60, "constructed": true, "metagame": mg});
    if bo3 {
        v["format"] = serde_json::json!("bo3");
    }
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v.to_string()
}

/// `FUZZ_POOL=meta`'s match `seed` (JSON): the best-of-three metagame, odd seeds one episode per game, one match in four
/// (by `seed / 2`) 12-turn games; `post`: the mirrors' post-sideboard matches (`"start_game": 2`).
fn meta_match_json(seed: u64, post: bool) -> String {
    let mut extra = serde_json::json!({});
    if post {
        extra = serde_json::json!({"start_game": 2, "game_episodes": true});
    } else if seed % 2 == 1 {
        extra["game_episodes"] = serde_json::json!(true);
    }
    if (seed / 2) % 4 == 3 {
        extra["max_turns"] = serde_json::json!(12);
    }
    let json = meta_json(true, post, extra);
    if !pair_mode() {
        return json;
    }
    // (`FUZZ_POOL=meta_pair`, ENGINE.md 240: the stew specialists' per-deck settings)
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    pair_settings(seed, &mut v);
    v.to_string()
}

/// A metagame game's (match's) pairing check (module docs, ENGINE.md 176): the game of `meta` (a metagame spec) with
/// seed `seed` and the game of its pairing's fixed spec played in lockstep by random actions from the seed. Its
/// pairing (p0's deck, p1's deck), the steps compared and the worlds compared.
fn meta_lockstep(meta: &str, seed: u64) -> ([u8; 2], u64, u64) {
    use crate::az::metagame::Metagame;
    use crate::az::tail_encode::Row;
    use crate::az::{MtgAz, MtgSpec};
    use zero_search::game::{Next, SearchGame};
    let ms = MtgSpec::parse(meta).unwrap_or_else(|e| panic!("seed {seed}: the metagame spec: {e}"));
    let mut a = MtgAz::new_game(&ms, 2, seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    let pairing = a.m.pairing.clone().unwrap_or_else(|| panic!("seed {seed}: a metagame game without its pairing"));
    assert_eq!(Some(&pairing), ms.pairing(seed).as_ref(), "seed {seed}: the game's pairing is the seed's");
    let fixed = Metagame::pair_json(meta, pairing.decks).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    let fs = MtgSpec::parse(&fixed).unwrap_or_else(|e| panic!("seed {seed}: the pairing's fixed spec: {e}"));
    assert_eq!(ms.pools(seed), fs.pools(seed), "seed {seed}: the seats are the pairing's lists");
    let mut b = MtgAz::new_game(&fs, 2, seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    assert_eq!(b.m.pairing, None, "seed {seed}: a fixed spec's game has no pairing");
    // (ENGINE.md 240: the per-deck settings follow the pairing - each seat's dealt flag and choice table)
    assert_eq!([a.m.is_dealt(0), a.m.is_dealt(1)], [b.m.is_dealt(0), b.m.is_dealt(1)], "seed {seed}: the dealt seats");
    assert_eq!(a.m.table, b.m.table, "seed {seed}: the seats' choice tables");
    assert_eq!(a.m.pools, b.m.pools, "seed {seed}: the match's pools are the pairing's 75s");
    let (ra, rb): (serde_json::Value, serde_json::Value) = (serde_json::from_str(&a.record_setup()).unwrap(), serde_json::from_str(&b.record_setup()).unwrap());
    assert_eq!((&ra["decks"], &ra["pairing"]), (&rb["decks"], &pairing.json()), "seed {seed}: the record");
    let bo1 = a.m.format == crate::az::Format::Bo1;
    let mains: Vec<Vec<u16>> = ms
        .pools(seed)
        .into_iter()
        .map(|(mut d, _)| {
            d.sort_unstable();
            d
        })
        .collect();
    let mut rng = Rng::new(seed ^ 0x3E7A_0176_F022);
    let (mut la, mut lb, mut ka, mut kb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut step, mut worlds) = (0u64, 0u64);
    let what = |step: u64| format!("seed {seed} ({} against {}) step {step}", pairing.names[0], pairing.names[1]);
    loop {
        if step % 3 == 0 {
            for v in 0..2 {
                assert!(Row::of_az_v8(&a, v) == Row::of_az_v8(&b, v), "{}: P{v}'s row differs from the pairing's fixed game", what(step));
            }
        }
        let Some(p) = a.to_move() else {
            assert_eq!(b.to_move(), None, "{}: one game over", what(step));
            assert_eq!(a.result(), b.result(), "{}: the result", what(step));
            if !a.chained() {
                break;
            }
            match (a.next_episode(None).unwrap(), b.next_episode(None).unwrap()) {
                (Next::Continue(x), Next::Continue(y)) => {
                    assert_eq!(x.m.pairing, a.m.pairing, "{}: the next episode keeps the pairing", what(step));
                    a = x;
                    b = y;
                    continue;
                }
                (Next::Over(x), Next::Over(y)) => {
                    assert_eq!(x, y, "{}: the match's result", what(step));
                    break;
                }
                _ => panic!("{}: one match over, the other not", what(step)),
            }
        };
        assert_eq!(b.to_move(), Some(p), "{}: the decider", what(step));
        if step % 8 == 5 {
            for viewer in 0..2usize {
                let (mut da, mut db) = (a.clone(), b.clone());
                let k = rng.next_u64();
                da.determinize(viewer, &mut zero_search::rng::Rng::new(k));
                db.determinize(viewer, &mut zero_search::rng::Rng::new(k));
                let w = || format!("{}: P{viewer}'s world", what(step));
                assert_eq!(da.m.pairing, a.m.pairing, "{}: the pairing", w());
                assert_eq!((&da.m.pools, &da.m.decks), (&db.m.pools, &db.m.decks), "{}: the pools and decks", w());
                for q in 0..2u8 {
                    let ids = |g: &Game| -> Vec<u16> {
                        let pl = g.player(q);
                        pl.hand.iter().chain(&pl.library).map(|&c| g.obj(c).def).collect()
                    };
                    assert_eq!(ids(&da.g), ids(&db.g), "{}: P{q}'s hand and library", w());
                }
                if bo1 && da.m.sb.is_none() {
                    let o = 1 - viewer;
                    assert_eq!(hd_deck(&da, o as u8), mains[o], "{}: the opponent's cards are its pairing's main", w());
                }
                assert!(Row::of_az_v8(&da, viewer) == Row::of_az_v8(&db, viewer), "{}: the viewer's row", w());
                worlds += 1;
            }
        }
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        assert_eq!(la, lb, "{}: the legal actions", what(step));
        // (ENGINE.md 240: a seat's choice table, the same in both games, played as a teacher decision; every other
        // decision at random - no table, no change: `meta`'s draws as before)
        let taught = a.table_action();
        assert_eq!(taught, b.table_action(), "{}: the table's action", what(step));
        let act = taught.unwrap_or_else(|| la[rng.below(la.len() as u32) as usize]);
        ka.clear();
        kb.clear();
        a.step(act, &mut ka).unwrap_or_else(|e| panic!("{}: {e}", what(step)));
        b.step(act, &mut kb).unwrap_or_else(|e| panic!("{}: {e}", what(step)));
        assert_eq!(ka, kb, "{}: the chance keys", what(step));
        step += 1;
        assert!(step < 300_000, "{}: the game does not end", what(step));
    }
    (pairing.decks, step, worlds)
}

/// `FUZZ_POOL=meta`'s per-cell counts of a run (`meta_g_0..8` games, `meta_m_` matches, `meta_p_` post-sideboard
/// matches) and the lockstep's steps and worlds, as `Totals` keys.
fn meta_totals(t: &mut Totals, kind: &str, cells: &[u64; 9], steps: u64, worlds: u64) {
    for (i, &c) in cells.iter().enumerate() {
        t.set(&format!("meta_{kind}_{i}"), c);
    }
    t.set(&format!("meta_{kind}_steps"), steps);
    t.set(&format!("meta_{kind}_worlds"), worlds);
}

/// `FUZZ_POOL=meta`'s end-of-run check of one kind (`least` seeds or more): every cell of positive weight played (the
/// mirrors only: `mirrors`), steps and worlds compared.
fn meta_check(t: &Totals, kind: &str, n: u64, least: u64, mirrors: bool) {
    if !meta_mode() || n < least {
        return;
    }
    let cells: Vec<u64> = (0..9).map(|i| t.get(&format!("meta_{kind}_{i}"))).collect();
    for (i, &c) in cells.iter().enumerate() {
        let mirror = i / 3 == i % 3;
        if mirrors && !mirror {
            assert_eq!(c, 0, "FUZZ_POOL=meta {kind}: the mirrors-only metagame played cell {i}");
        } else {
            assert!(c > 0, "FUZZ_POOL=meta {kind}: cell {i} (p0 {}, p1 {}) never played in {n} seeds: {cells:?}", i / 3, i % 3);
        }
    }
    assert!(t.get(&format!("meta_{kind}_steps")) > 0 && t.get(&format!("meta_{kind}_worlds")) > 0, "FUZZ_POOL=meta {kind}: no lockstep step or world compared");
}

/// `FUZZ_POOL=cross` (ENGINE.md 148).
fn cross_mode() -> bool {
    pool_mode() == PoolMode::Cross
}

/// Constructed best-of-three matches: `FUZZ_POOL=delver`'s (ENGINE.md 120), `cross`'s (ENGINE.md 148), `meta`'s
/// (ENGINE.md 176) or `hogaak`'s (ENGINE.md 220.4).
pub fn constructed_bo3_mode() -> bool {
    delver_mode() || cross_mode() || meta_mode() || hogaak_mode()
}

/// The real 75s on main (ENGINE.md 148): the Izzet Delver, Legacy Doomsday, Sneak and Show and (ENGINE.md 220.4)
/// Legacy Hogaak Tempo lists.
fn real_75(k: usize) -> crate::az::constructed::Decklist {
    use crate::az::constructed::Decklist;
    match k {
        0 => Decklist::izzet_delver(),
        1 => Decklist::legacy_doomsday(),
        2 => Decklist::parse(include_str!("../../../data/decks/sneak_show_8014220.json")).unwrap(),
        _ => Decklist::legacy_hogaak(),
    }
}

const REAL_75_NAMES: [&str; 4] = ["Delver", "Doomsday", "Sneak and Show", "Hogaak"];

/// A 75 as a constructed best-of-three spec takes it: the main's names and the sideboard cards
/// this checkout supports (as `FUZZ_POOL=delver`'s matches; the rest left out, named by `cross_75_report`).
fn bo3_lists(d: &crate::az::constructed::Decklist) -> (Vec<String>, Vec<String>) {
    let db = CardDb::constructed();
    let side = d.sideboard.iter().filter(|(_, n)| db.id(n).is_some()).flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    (d.main_names(), side)
}

/// `FUZZ_POOL=cross`'s pairings of the real 75s for matches (`real_75`): every pair with Doomsday, Sneak and Show or
/// (ENGINE.md 220.4) Hogaak, both seats, and their mirrors - none is `FUZZ_POOL=delver`'s best-of-three.
const CROSS_PAIRS: [(usize, usize); 15] =
    [(1, 2), (2, 1), (1, 0), (0, 1), (2, 0), (0, 2), (1, 1), (2, 2), (3, 0), (0, 3), (3, 1), (1, 3), (3, 2), (2, 3), (3, 3)];

/// `FUZZ_POOL=hogaak`'s pairings of the real 75s for matches (ENGINE.md 220.4): the Hogaak 75 against each other 75
/// in either seat, and its mirror twice (one match in four, as its games).
const HOGAAK_PAIRS: [(usize, usize); 8] = [(3, 0), (0, 3), (3, 1), (1, 3), (3, 2), (2, 3), (3, 3), (3, 3)];

/// `FUZZ_POOL=cross`'s match `seed` (ENGINE.md 148): constructed best-of-three between `CROSS_PAIRS[seed % 15]`'s
/// 75s (`FUZZ_POOL=hogaak`: `HOGAAK_PAIRS[seed % 8]`'s), one match in four (by `seed / pairs`) 12-turn games; `extra`
/// merged into the spec (the post-sideboard matches').
fn cross_bo3_spec_with(seed: u64, extra: serde_json::Value) -> crate::az::MtgSpec {
    let pairs: &[(usize, usize)] = if hogaak_mode() { &HOGAAK_PAIRS } else { &CROSS_PAIRS };
    let (p0, p1) = pairs[(seed % pairs.len() as u64) as usize];
    // (A post-sideboard match is a mirror, ENGINE.md 147.4: `"start_game": 2` draws game 1 as a fair coin, exact for a
    // mirror only; the pair's first 75 in both seats - `FUZZ_POOL=hogaak`'s, the Hogaak 75's.)
    let post = extra.get("start_game").and_then(|x| x.as_u64()) == Some(2);
    let p0 = if post && hogaak_mode() { 3 } else { p0 };
    let p1 = if post { p0 } else { p1 };
    let ((m0, s0), (m1, s1)) = (bo3_lists(&real_75(p0)), bo3_lists(&real_75(p1)));
    let max_turns = if (seed / pairs.len() as u64) % 4 == 3 { 12 } else { 60 };
    let mut v = serde_json::json!({"max_turns": max_turns, "constructed": true, "format": "bo3",
        "decks": {"p0": m0, "p1": m1, "p0_sideboard": s0, "p1_sideboard": s1}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    crate::az::MtgSpec::parse(&v.to_string()).unwrap_or_else(|e| panic!("match {seed}: {} against {}: {e}", REAL_75_NAMES[p0], REAL_75_NAMES[p1]))
}

/// The sideboard cards of the real 75s this checkout doesn't support (left out of `cross`'s 75s).
fn cross_75_report() -> String {
    let db = CardDb::constructed();
    (0..REAL_75_NAMES.len())
        .map(|k| {
            let d = real_75(k);
            let out: Vec<String> = d.sideboard.iter().filter(|(_, n)| db.id(n).is_none()).map(|(k, n)| format!("{k} {n}")).collect();
            format!("{}: {}", REAL_75_NAMES[k], if out.is_empty() { "the whole 75".to_string() } else { format!("without {}", out.join(", ")) })
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// `FUZZ_POOL=cross`'s games: spells of both decks seen on the stack (each stack item once).
static CROSS_SEEN: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
const CROSS_SPELLS: [&str; 5] = ["Doomsday", "Thassa's Oracle", "Show and Tell", "Sneak Attack", "Omniscience"];

fn count_cross(g: &Game, seen: &mut Vec<u32>) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        if let crate::game::StackKind::Spell { card } = it.kind {
            if let Some(k) = CROSS_SPELLS.iter().position(|&n| n == g.name(card)) {
                CROSS_SEEN[k].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// `FUZZ_POOL=names` (ENGINE.md 127).
fn names_mode() -> bool {
    pool_mode() == PoolMode::Names
}

/// A deck of `FUZZ_POOL=names` (ENGINE.md 127): Anointed Peacekeeper and
/// Disruptor Flute four each, eight random cards of the table with more than
/// one face three each, 28 basic lands.
fn names_deck(rng: &mut Rng) -> Vec<u16> {
    let db = CardDb::constructed();
    let mut multi: Vec<u16> = (0..db.num_cards() as u16)
        .filter(|&d| {
            let dd = db.def(d);
            !dd.token && (dd.halves.is_some() || dd.back.is_some() || dd.adventure.is_some() || dd.prepare.is_some())
        })
        .collect();
    assert!(multi.len() >= 8, "the table's cards with more than one face: {}", multi.len());
    let mut list: Vec<(u32, String)> = vec![(4, "Anointed Peacekeeper".into()), (4, "Disruptor Flute".into())];
    for _ in 0..8 {
        let d = multi.swap_remove(rng.below(multi.len() as u32) as usize);
        list.push((3, db.def(d).name.clone()));
    }
    list.extend([(8, "Plains"), (5, "Island"), (5, "Swamp"), (5, "Mountain"), (5, "Forest")].map(|(n, s)| (n, s.to_string())));
    let deck = crate::az::constructed::Decklist::ids(db, &list).unwrap();
    crate::az::constructed::check(db, &deck, &[]).unwrap_or_else(|e| panic!("a names deck: {e}"));
    deck
}

/// `FUZZ_POOL=names`: the names chosen, per card (0 Anointed Peacekeeper, 1
/// Disruptor Flute) and kind (`NAME_KINDS`), and the spells cast whose name
/// was chosen by a permanent that taxes them.
static NAMES_CHOSEN: [[AtomicU64; 8]; 2] = [const { [const { AtomicU64::new(0) }; 8] }; 2];
static NAMES_TAXED: AtomicU64 = AtomicU64::new(0);
const NAME_KINDS: [&str; 8] =
    ["a card of one face", "a front face", "a back face", "an Adventure", "a split card's half", "a prepare spell", "a token", "PASS"];

/// `FUZZ_POOL=names`: at a `NameCard` decision with a next page, its "next
/// page" three times in four, so the last pages of a long list (hidden
/// decks: the table's names, 9 pages and more, the faces and tokens on the
/// last) are reached; uniform play would stay on the first.
fn names_next_page(g: &Game, rng: &mut Rng) -> Option<u16> {
    if !names_mode() || g.kind() != Some(DecisionKind::NameCard) {
        return None;
    }
    let next = g.pending_decision()?.options.iter().find(|o| matches!(o.choice, crate::Choice::Page(_)))?.action;
    (rng.below(4) != 0).then_some(next)
}

/// The kind of name definition `d` is (`NAME_KINDS`).
fn name_kind(db: &CardDb, d: u16) -> usize {
    if d == u16::MAX {
        return 7;
    }
    let dd = db.def(d);
    if dd.token {
        return 6;
    }
    match dd.front {
        Some(f) => {
            let card = db.def(f);
            if card.halves.is_some_and(|(l, r)| d == l || d == r) {
                4
            } else if card.back == Some(d) {
                2
            } else if card.adventure == Some(d) {
                3
            } else if card.prepare == Some(d) {
                5
            } else {
                panic!("a name of an unknown face: {}", dd.name)
            }
        }
        None if dd.back.is_some() || dd.adventure.is_some() || dd.prepare.is_some() => 1,
        None => 0,
    }
}

/// `FUZZ_POOL=names`: the names chosen as a Peacekeeper or a Flute entered
/// (each permanent once), and the spells newly on the stack that a chosen
/// name taxes (a Flute's: anyone's; a Peacekeeper's: its controller's
/// opponents'; the name compared as the engine does, a split card's halves).
fn count_names(g: &Game, named: &mut Vec<crate::ids::ObjRef>, seen: &mut Vec<u32>) {
    let db = g.db();
    let face = |d: u16| crate::sneak_show::face_name(db.def(d));
    let mut chosen: Vec<(u8, bool, &str)> = Vec::new();
    for &c in g.battlefield() {
        let card = match g.name(c) {
            "Anointed Peacekeeper" => 0,
            "Disruptor Flute" => 1,
            _ => continue,
        };
        let Some(d) = g.marks_of(c).iter().find_map(|m| match m {
            crate::game::Mark::ChosenName(d) => Some(*d),
            _ => None,
        }) else {
            continue;
        };
        let r = g.obj_ref(c);
        if !named.contains(&r) {
            named.push(r);
            NAMES_CHOSEN[card][name_kind(db, d)].fetch_add(1, Ordering::Relaxed);
        }
        if d != u16::MAX {
            chosen.push((g.obj(c).controller, card == 1, face(d)));
        }
    }
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        if let crate::game::StackKind::Spell { card } = it.kind {
            let dd = g.def_of(card);
            let names: Vec<&str> = match dd.halves {
                Some((l, r)) => vec![face(l), face(r)],
                None => vec![crate::sneak_show::face_name(dd)],
            };
            if chosen.iter().any(|&(p, everyone, n)| (everyone || p != it.controller) && names.contains(&n)) {
                NAMES_TAXED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// `FUZZ_POOL=delver_sb` (ENGINE.md 122).
fn delver_sb_mode() -> bool {
    pool_mode() == PoolMode::DelverSb
}

/// The Delver 60 of `FUZZ_POOL=delver_sb` (ENGINE.md 122): the Delver 75's five sideboard cards
/// outside the cube (three each, Meltdown two) in for Daze, Ponder, Unholy Heat, Prismari Charm
/// and three Brainstorms.
const DELVER_SB_DECK: &[(u32, &str)] = &[
    (3, "Consign to Memory"),
    (3, "Hydroblast"),
    (2, "Meltdown"),
    (3, "Price of Progress"),
    (3, "Rough // Tumble"),
    (1, "Brainstorm"),
    (3, "Cori-Steel Cutter"),
    (4, "Dragon's Rage Channeler"),
    (2, "Flooded Strand"),
    (4, "Force of Will"),
    (1, "Island"),
    (4, "Lightning Bolt"),
    (4, "Mishra's Bauble"),
    (2, "Misty Rainforest"),
    (2, "Murktide Regent"),
    (3, "Polluted Delta"),
    (2, "Scalding Tarn"),
    (4, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"),
    (1, "Thundering Falls"),
    (1, "Urza's Bauble"),
    (4, "Volcanic Island"),
    (4, "Wasteland"),
];

fn delver_sb_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = DELVER_SB_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("DELVER_SB_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=delver_sb`: the five cards' spells seen on the stack (each stack item once, by its
/// id; Rough // Tumble by the half cast, copies of Consign to Memory too), and Consign to Memory
/// spells aimed at a triggered ability.
static DELVER_SB_SEEN: [AtomicU64; 6] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static DELVER_SB_AT_ABILITY: AtomicU64 = AtomicU64::new(0);
const DELVER_SB_SPELLS: [&str; 6] = ["Consign to Memory", "Hydroblast", "Meltdown", "Price of Progress", "Rough", "Tumble"];

fn count_delver_sb(g: &Game, seen: &mut Vec<u32>) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        if let crate::game::StackKind::Spell { card } = it.kind {
            if let Some(k) = DELVER_SB_SPELLS.iter().position(|&n| n == g.name(card)) {
                DELVER_SB_SEEN[k].fetch_add(1, Ordering::Relaxed);
                if k == 0 && it.targets.iter().any(|&(_, t)| matches!(t, crate::Target::Stack(_))) {
                    DELVER_SB_AT_ABILITY.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// `FUZZ_POOL=hogaak_c` (ENGINE.md 220.3).
fn hogaak_c_mode() -> bool {
    pool_mode() == PoolMode::HogaakC
}

/// The deck of `FUZZ_POOL=hogaak_c` (ENGINE.md 220.3): the Legacy Hogaak Tempo 60 (MTGGoldfish 8027954) with the
/// other lanes' Hogaak (4) and Mockingbird (3) out, Dryad Arbor 1 -> 4, a third Thoughtseize, a second Brazen
/// Borrower and two Fatal Push (the sideboard's) in: lane C's four cards four each.
const HOGAAK_C_DECK: &[(u32, &str)] = &[
    (4, "Stitcher's Supplier"),
    (4, "Dark Matter Manipulator"),
    (4, "Moonshadow"),
    (4, "Dryad Arbor"),
    (3, "Thoughtseize"),
    (2, "Kaito, Bane of Nightmares"),
    (4, "Force of Will"),
    (3, "Daze"),
    (2, "Fatal Push"),
    (3, "Orcish Bowmasters"),
    (2, "Brazen Borrower // Petty Theft"),
    (3, "Ponder"),
    (4, "Brainstorm"),
    (4, "Underground Sea"),
    (1, "Swamp"),
    (1, "Undercity Sewers"),
    (4, "Misty Rainforest"),
    (4, "Verdant Catacombs"),
    (4, "Wasteland"),
];

fn hogaak_c_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = HOGAAK_C_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("HOGAAK_C_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=hogaak_c`'s counters: lane C's spells seen on the stack (Stitcher's Supplier, Dark Matter
/// Manipulator, Moonshadow; each stack item once, by its id) and Dryad Arbor played as a land; Moonshadow's triggers
/// put on the stack and its -1/-1 counters removed (a counter count that went down on the same object: its trigger,
/// or a +1/+1 counter's annihilation, CR 704.5q); Dark Matter Manipulators (each object once) seen at +2/+0 or more
/// (seven cards or more in its controller's graveyard); Dryad Arbors (each object once per turn) attacking, and
/// chosen as a mana source while paying or floating mana.
static HOGAAK_C_SEEN: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static HOGAAK_C_MOON_TRIGGERS: AtomicU64 = AtomicU64::new(0);
static HOGAAK_C_MOON_REMOVED: AtomicU64 = AtomicU64::new(0);
static HOGAAK_C_DMM_BIG: AtomicU64 = AtomicU64::new(0);
static HOGAAK_C_ARBOR_ATTACKS: AtomicU64 = AtomicU64::new(0);
static HOGAAK_C_ARBOR_MANA: AtomicU64 = AtomicU64::new(0);
const HOGAAK_C_CARDS: [&str; 4] = ["Stitcher's Supplier", "Dark Matter Manipulator", "Moonshadow", "Dryad Arbor"];

/// One game's `FUZZ_POOL=hogaak_c` bookkeeping: the stack items counted, each Moonshadow's last -1/-1 count, the
/// Manipulators counted, the Arbors' (object, turn) attacks counted.
#[derive(Default)]
struct HogaakCSeen {
    items: Vec<u32>,
    moon: Vec<(crate::ids::ObjRef, u64)>,
    dmm: Vec<crate::ids::ObjRef>,
    attacks: Vec<(crate::ids::ObjRef, u32)>,
}

/// Before action `a` is applied: Dryad Arbor played (`Choice::Play`) or chosen as a mana source.
fn count_hogaak_c_choice(g: &Game, a: u16) {
    let Some(d) = g.pending_decision() else { return };
    let Some(o) = d.options.iter().find(|o| o.action == a) else { return };
    match (d.kind, o.choice) {
        (DecisionKind::Priority, Choice::Play(c)) if g.name(c) == "Dryad Arbor" => {
            HOGAAK_C_SEEN[3].fetch_add(1, Ordering::Relaxed);
        }
        (DecisionKind::PayMana | DecisionKind::ActivateMana, Choice::Card(c) | Choice::Ability(c, _)) if g.name(c) == "Dryad Arbor" => {
            HOGAAK_C_ARBOR_MANA.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

fn count_hogaak_c(g: &Game, s: &mut HogaakCSeen) {
    for it in g.stack() {
        if s.items.contains(&it.id) {
            continue;
        }
        s.items.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => {
                if let Some(k) = HOGAAK_C_CARDS[..3].iter().position(|&n| n == g.name(card)) {
                    HOGAAK_C_SEEN[k].fetch_add(1, Ordering::Relaxed);
                }
            }
            crate::game::StackKind::Ability { ability, .. } if g.db().def(ability.def).name == "Moonshadow" => {
                HOGAAK_C_MOON_TRIGGERS.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    let turn = g.turn().number;
    for &c in g.battlefield() {
        let r = g.obj_ref(c);
        match g.name(c) {
            "Moonshadow" => {
                let n = g.counters_on(c, crate::db::Counter::M1M1) as u64;
                match s.moon.iter_mut().find(|x| x.0 == r) {
                    Some(x) => {
                        if n < x.1 {
                            HOGAAK_C_MOON_REMOVED.fetch_add(x.1 - n, Ordering::Relaxed);
                        }
                        x.1 = n;
                    }
                    None => s.moon.push((r, n)),
                }
            }
            "Dark Matter Manipulator" => {
                let p = g.obj(c).controller;
                if !s.dmm.contains(&r) && g.graveyard_cards(p).count() >= 7 {
                    s.dmm.push(r);
                    HOGAAK_C_DMM_BIG.fetch_add(1, Ordering::Relaxed);
                }
            }
            "Dryad Arbor" if g.obj(c).attacking && !s.attacks.contains(&(r, turn)) => {
                s.attacks.push((r, turn));
                HOGAAK_C_ARBOR_ATTACKS.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// `FUZZ_POOL=lands_b` (ENGINE.md 270.2).
fn lands_b_mode() -> bool {
    pool_mode() == PoolMode::LandsB
}

/// The deck of `FUZZ_POOL=lands_b` (ENGINE.md 270.2): the Legacy Lands 61 (MTGGoldfish 8027955) with the other
/// lanes' cards out (Dark Depths 3, Thespian's Stage 3, Ghost Quarter, Maze of Ith, Bojuka Bog, The Tabernacle at
/// Pendrell Vale, Pithing Needle, Blast Zone), Life from the Loam 3 -> 4 and Yavimaya 3 -> 4, six more Forests and
/// the sideboard's Force of Vigor 2 and a Dismember in: lane B's four cards four each, 60 cards. (Not the sideboard's
/// Endurance: against the Delver 60 its random bottom order under Murktide Regent's triggers is a hidden-information
/// finding outside these cards, ENGINE.md 270.2.7.)
const LANDS_B_DECK: &[(u32, &str)] = &[
    (4, "Life from the Loam"),
    (4, "Exploration"),
    (4, "Carnivorous Cultivator // Enroot"),
    (4, "Yavimaya, Cradle of Growth"),
    (1, "Otawara, Soaring City"),
    (9, "Forest"),
    (2, "Disruptor Flute"),
    (1, "Icetill Explorer"),
    (1, "Karakas"),
    (4, "Wasteland"),
    (4, "Crop Rotation"),
    (1, "Ancient Tomb"),
    (1, "Skateboard"),
    (4, "Malevolent Rumble"),
    (4, "Mox Diamond"),
    (4, "Urza's Saga"),
    (4, "Boseiju, Who Endures"),
    (1, "Expedition Map"),
    (2, "Force of Vigor"),
    (1, "Dismember"),
];

fn lands_b_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = LANDS_B_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("LANDS_B_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=lands_b`'s counters (`LANDS_B_KINDS`): spells seen on the stack (each stack item once, by its id), a
/// dredge question answered by dredging or by drawing (`Task::Dredge`, before the answer is applied), a land played
/// as the second or later of a turn, Yavimaya played, the Cultivator's combat-damage trigger put on the stack, and a
/// land that is a Forest by Yavimaya alone (no printed Forest type; each object once a turn).
static LANDS_B_SEEN: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
const LANDS_B_KINDS: [&str; 10] = [
    "Life from the Loam cast",
    "draws dredged",
    "dredges declined",
    "Exploration cast",
    "second lands of a turn played",
    "Carnivorous Cultivator cast",
    "Enroot cast",
    "Cultivator combat triggers",
    "Yavimaya played",
    "lands a Forest by Yavimaya",
];

/// One game's `FUZZ_POOL=lands_b` bookkeeping: the stack items counted, the (land, turn) Forests counted.
#[derive(Default)]
struct LandsBSeen {
    items: Vec<u32>,
    forests: Vec<(crate::ids::ObjRef, u32)>,
}

/// Before action `a` is applied: a dredge question's answer, a land played.
fn count_lands_b_choice(g: &Game, a: u16) {
    let Some(d) = g.pending_decision() else { return };
    let Some(o) = d.options.iter().find(|o| o.action == a) else { return };
    let dredge = matches!(g.agenda.last(), Some(crate::flow::Task::Dredge { .. }));
    match (d.kind, o.choice) {
        (DecisionKind::ChooseGraveyardCard, Choice::Card(_)) if dredge => {
            LANDS_B_SEEN[1].fetch_add(1, Ordering::Relaxed);
        }
        (DecisionKind::ChooseGraveyardCard, Choice::Pass) if dredge => {
            LANDS_B_SEEN[2].fetch_add(1, Ordering::Relaxed);
        }
        (DecisionKind::Priority | DecisionKind::ChooseGraveyardCard, Choice::Play(c) | Choice::PlayFace(c)) => {
            if g.player(d.player).lands_played >= 1 {
                LANDS_B_SEEN[4].fetch_add(1, Ordering::Relaxed);
            }
            if g.name(c) == "Yavimaya, Cradle of Growth" {
                LANDS_B_SEEN[8].fetch_add(1, Ordering::Relaxed);
            }
        }
        _ => {}
    }
}

fn count_lands_b(g: &Game, s: &mut LandsBSeen) {
    for it in g.stack() {
        if s.items.contains(&it.id) {
            continue;
        }
        s.items.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => {
                let k = match g.name(card) {
                    "Life from the Loam" => 0,
                    "Exploration" => 3,
                    "Carnivorous Cultivator" | "Carnivorous Cultivator // Enroot" => 5,
                    "Enroot" => 6,
                    _ => continue,
                };
                LANDS_B_SEEN[k].fetch_add(1, Ordering::Relaxed);
            }
            crate::game::StackKind::Ability { ability, .. } if g.db().def(ability.def).name == "Carnivorous Cultivator // Enroot" => {
                LANDS_B_SEEN[7].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    let Some(forest) = g.db().subtype_id("Forest") else { return };
    let turn = g.turn().number;
    for &c in g.battlefield() {
        if g.is_type(c, crate::db::types::LAND) && !g.def_of(c).subtypes.contains(&forest) && g.has_subtype(c, forest) {
            let r = g.obj_ref(c);
            if !s.forests.contains(&(r, turn)) {
                s.forests.push((r, turn));
                LANDS_B_SEEN[9].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// `FUZZ_POOL=hogaak` (ENGINE.md 220.4).
fn hogaak_mode() -> bool {
    pool_mode() == PoolMode::Hogaak
}

/// The real Legacy Hogaak Tempo 60 (`data/decks/hogaak_tempo_8027954.json`'s main deck, ENGINE.md 220.4).
fn hogaak_deck() -> Vec<u16> {
    let d = crate::az::constructed::Decklist::legacy_hogaak();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("the Hogaak 60: {e}"));
    deck
}

/// `FUZZ_POOL=hogaak`'s decks of game `seed` (ENGINE.md 220.4): the Hogaak 60 against the Delver, Doomsday and Sneak
/// and Show 60s in either seat, one game in four the mirror.
fn hogaak_decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    let h = hogaak_deck();
    let other = match (seed % 8) / 2 {
        0 => delver_deck(),
        1 => doomsday_deck(),
        2 => sneak_show_deck(),
        _ => h.clone(),
    };
    if seed % 2 == 0 {
        [(h, Vec::new()), (other, Vec::new())]
    } else {
        [(other, Vec::new()), (h, Vec::new())]
    }
}

/// `FUZZ_POOL=hogaak`'s own counters (ENGINE.md 220.4; the lanes' counters - `HOGAAK_A_SEEN`, `HOGAAK_B_COUNTS`,
/// `HOGAAK_C_*` - count the rest in its games too): a Hogaak spell cast from a graveyard paid by convoke and delve
/// together (counted with `HOGAAK_A_SEEN`, in every pool), a Stitcher's Supplier trigger put on the stack and one
/// resolved (each stack item once; resolved: on top of the stack at a priority pass, gone after it).
static HOGAAK_SEEN: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
const HOGAAK_KINDS: [&str; 3] = ["Hogaak cast from a graveyard by convoke and delve", "Stitcher's Supplier triggers", "Stitcher's Supplier triggers resolved"];
/// `FUZZ_POOL=hogaak`'s matches (ENGINE.md 220.4): Massacre spells (the sideboard card) resolved (as
/// `hogaak_b_massacre_on_top`) and cast (each stack item of each game once), in the main matches (0, 2) and in the
/// post-sideboard ones (1, 3).
static HOGAAK_MASSACRE_MATCHES: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// The stack id of a Stitcher's Supplier trigger on top of the stack that action `a` (a priority pass) may resolve
/// (`FUZZ_POOL=hogaak`).
fn hogaak_stitcher_on_top(g: &Game, a: u16) -> Option<u32> {
    if g.kind() != Some(DecisionKind::Priority) || a != crate::act::PASS {
        return None;
    }
    let it = g.stack().last()?;
    match it.kind {
        crate::game::StackKind::Ability { ability, .. } if g.db().def(ability.def).name == "Stitcher's Supplier" => Some(it.id),
        _ => None,
    }
}

/// `FUZZ_POOL=hogaak`'s Stitcher's Supplier counts (`HOGAAK_SEEN` 1 and 2); `top` the trigger on top before the action.
fn count_hogaak_stitcher(g: &Game, top: Option<u32>, seen: &mut Vec<u32>) {
    if let Some(id) = top {
        if !g.stack().iter().any(|s| s.id == id) {
            HOGAAK_SEEN[2].fetch_add(1, Ordering::Relaxed);
        }
    }
    for it in g.stack() {
        if let crate::game::StackKind::Ability { ability, .. } = it.kind {
            if g.db().def(ability.def).name == "Stitcher's Supplier" && !seen.contains(&it.id) {
                seen.push(it.id);
                HOGAAK_SEEN[1].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// `FUZZ_POOL=sneak_show` or `sns` (ENGINE.md 123).
fn sns_mode() -> bool {
    matches!(pool_mode(), PoolMode::SneakShow | PoolMode::Sns)
}

/// The real Sneak and Show 60 of `FUZZ_POOL=sneak_show` (ENGINE.md 123):
/// `data/decks/sneak_show_8014220.json`'s main deck (read here:
/// `az/constructed.rs` is the best-of-three lane's to change).
fn sneak_show_deck() -> Vec<u16> {
    let d = crate::az::constructed::Decklist::parse(include_str!("../../../data/decks/sneak_show_8014220.json")).unwrap();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("the Sneak and Show 60: {e}"));
    deck
}

/// The lane's deck of `FUZZ_POOL=sns` (ENGINE.md 123): the six cards four each.
const SNS_DECK: &[(u32, &str)] = &[
    (4, "Show and Tell"),
    (4, "Omniscience"),
    (4, "Mistrise Village"),
    (4, "Brotherhood's End"),
    (4, "Disruptor Flute"),
    (4, "Magus of the Moon"),
    (3, "Emrakul, the Aeons Torn"),
    (2, "Griselbrand"),
    (2, "Atraxa, Grand Unifier"),
    (2, "Sneak Attack"),
    (4, "Force of Will"),
    (4, "Brainstorm"),
    (4, "Lotus Petal"),
    (3, "Volcanic Island"),
    (4, "Scalding Tarn"),
    (2, "Ancient Tomb"),
    (2, "City of Traitors"),
    (2, "Island"),
    (2, "Mountain"),
];

fn sns_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = SNS_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("SNS_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=sneak_show` / `sns`: the lane's cards seen on the stack (spells,
/// Mistrise Village's ability; each stack item once, by its id), spells cast
/// without paying their mana costs, spells made uncounterable, names chosen.
static SNS_SEEN: [AtomicU64; 6] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static SNS_FREE: AtomicU64 = AtomicU64::new(0);
static SNS_UNCOUNTERABLE: AtomicU64 = AtomicU64::new(0);
static SNS_NAMED: AtomicU64 = AtomicU64::new(0);
const SNS_CARDS: [&str; 6] = ["Show and Tell", "Omniscience", "Mistrise Village", "Brotherhood's End", "Disruptor Flute", "Magus of the Moon"];

fn count_sns(g: &Game, seen: &mut Vec<u32>) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => {
                if let Some(k) = SNS_CARDS.iter().position(|&n| n == g.name(card)) {
                    SNS_SEEN[k].fetch_add(1, Ordering::Relaxed);
                }
                if matches!(it.from, crate::game::CastFrom::Permission { free: true, .. }) {
                    SNS_FREE.fetch_add(1, Ordering::Relaxed);
                }
                if g.marks_of(card).contains(&crate::game::Mark::Uncounterable) {
                    SNS_UNCOUNTERABLE.fetch_add(1, Ordering::Relaxed);
                }
            }
            crate::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Mistrise Village" => {
                SNS_SEEN[2].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// `FUZZ_POOL=sneak_show` / `sns`: names chosen as a Flute entered (each
/// permanent once).
fn count_sns_names(g: &Game, named: &mut Vec<crate::ids::ObjRef>) {
    for &c in g.battlefield() {
        let r = g.obj_ref(c);
        if !named.contains(&r) && g.marks_of(c).iter().any(|m| matches!(m, crate::game::Mark::ChosenName(_))) {
            named.push(r);
            SNS_NAMED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `FUZZ_POOL=hogaak_a` (ENGINE.md 220.1).
fn hogaak_a_mode() -> bool {
    pool_mode() == PoolMode::HogaakA
}

/// The lane's deck of `FUZZ_POOL=hogaak_a` (ENGINE.md 220.1): MTGGoldfish's Legacy Hogaak Tempo
/// (deck 8027957's archetype list, 8027954) with its cards of the other lanes (Stitcher's
/// Supplier 4, Mockingbird 3, Dark Matter Manipulator 4, Moonshadow 4, Dryad Arbor 1: 16) swapped
/// for supported graveyard fillers and cheap black creatures.
const HOGAAK_A_DECK: &[(u32, &str)] = &[
    (4, "Hogaak, Arisen Necropolis"),
    (3, "Orcish Bowmasters"),
    (2, "Kaito, Bane of Nightmares"),
    (1, "Brazen Borrower // Petty Theft"),
    (2, "Thoughtseize"),
    (4, "Force of Will"),
    (3, "Ponder"),
    (4, "Brainstorm"),
    (3, "Daze"),
    (4, "Wasteland"),
    (1, "Swamp"),
    (4, "Underground Sea"),
    (4, "Misty Rainforest"),
    (4, "Verdant Catacombs"),
    (1, "Undercity Sewers"),
    (4, "Mishra's Bauble"),
    (4, "Street Wraith"),
    (2, "Entomb"),
    (2, "Dark Confidant"),
    (2, "Deep-Cavern Bat"),
    (2, "Barrowgoyf"),
];

fn hogaak_a_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = HOGAAK_A_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("HOGAAK_A_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=hogaak_a`: Hogaak spells once cast (601.2i: at the priority after their cast, each
/// stack item once): cast from a hand, from a graveyard; paid by convoke (its total cost less the
/// cards delved is not zero: no mana pays it, so its convoke did; read at that priority, the cost
/// the payment paid), by delve (cards delved), by both.
static HOGAAK_A_SEEN: [AtomicU64; 5] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
const HOGAAK_A_KINDS: [&str; 5] = ["cast from a hand", "cast from a graveyard", "paid by convoke", "paid by delve", "paid by both"];

/// `FUZZ_POOL=hogaak_a`'s counts (`HOGAAK_A_SEEN`), `seen` the stack ids counted.
fn count_hogaak_a(g: &Game, seen: &mut Vec<u32>) {
    if g.kind() != Some(DecisionKind::Priority) {
        return;
    }
    for it in g.stack() {
        let crate::game::StackKind::Spell { card } = it.kind else { continue };
        if seen.contains(&it.id) || !g.def_of(card).casting.no_mana {
            continue;
        }
        seen.push(it.id);
        let conv = !g.total_mana(card, it).less_generic(it.delved as u64).is_zero();
        let delve = it.delved > 0;
        if it.origin == crate::ids::Zone::Graveyard && conv && delve {
            HOGAAK_SEEN[0].fetch_add(1, Ordering::Relaxed);
        }
        for (k, hit) in [it.origin == crate::ids::Zone::Hand, it.origin == crate::ids::Zone::Graveyard, conv, delve, conv && delve].into_iter().enumerate() {
            if hit {
                HOGAAK_A_SEEN[k].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// `FUZZ_POOL=hogaak_b` (ENGINE.md 220.2).
fn hogaak_b_mode() -> bool {
    pool_mode() == PoolMode::HogaakB
}

/// The lane's deck of `FUZZ_POOL=hogaak_b` (ENGINE.md 220.2): Mockingbird and Massacre four each;
/// copy targets of mana value 0 to 7 (Orcish Bowmasters and its Orc Army tokens, Brazen Borrower,
/// Kaito, Concealing Curtains and its back face, Walking Ballista - 107.3m -, True-Name Nemesis,
/// Phyrexian Metamorph - copies of copies -, Murktide Regent); the Hogaak deck's blue and black
/// spells; Underground Sea, basic Swamps and Plains-typed lands (Scrubland, Tundra, a Plains), so in
/// the mirror an opponent controls a Plains.
const HOGAAK_B_DECK: &[(u32, &str)] = &[
    (4, "Mockingbird"),
    (4, "Massacre"),
    (4, "Orcish Bowmasters"),
    (2, "Brazen Borrower // Petty Theft"),
    (2, "Kaito, Bane of Nightmares"),
    (2, "Concealing Curtains // Revealing Eye"),
    (2, "Walking Ballista"),
    (1, "True-Name Nemesis"),
    (1, "Phyrexian Metamorph"),
    (1, "Murktide Regent"),
    (4, "Force of Will"),
    (4, "Brainstorm"),
    (3, "Ponder"),
    (3, "Daze"),
    (2, "Thoughtseize"),
    (4, "Underground Sea"),
    (2, "Scrubland"),
    (2, "Tundra"),
    (2, "Swamp"),
    (2, "Island"),
    (1, "Plains"),
    (3, "Verdant Catacombs"),
    (3, "Polluted Delta"),
    (2, "Wasteland"),
];

fn hogaak_b_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = HOGAAK_B_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("HOGAAK_B_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=hogaak_b`'s counts (ENGINE.md 220.2): Mockingbird cast, a copy made, a copy declined,
/// Massacre cast for free (its alternative cost), Massacre resolving; Massacre cast, a copy of a
/// back face, a Mockingbird entering as Walking Ballista with counters (107.3m).
static HOGAAK_B_COUNTS: [AtomicU64; 8] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
const HOGAAK_B_NAMES: [&str; 8] = [
    "Mockingbird cast",
    "copies made",
    "copies declined",
    "Massacre cast for free",
    "Massacre resolved",
    "Massacre cast",
    "copies of a back face",
    "Mockingbird as Walking Ballista with counters",
];

/// The stack id of a Massacre spell on top of the stack that action `a` (priority's pass) may
/// resolve (`FUZZ_POOL=hogaak_b`): it resolved if it is gone after the action (only a resolution
/// takes the top item away on a pass; a counterspell resolves from the top itself).
fn hogaak_b_massacre_on_top(g: &Game, a: u16) -> Option<u32> {
    if g.kind() != Some(crate::DecisionKind::Priority) || a != crate::act::PASS {
        return None;
    }
    let it = g.stack().last()?;
    match it.kind {
        crate::game::StackKind::Spell { card } if g.db().def(g.own_def(card)).name == "Massacre" => Some(it.id),
        _ => None,
    }
}

fn count_hogaak_b(g: &Game, massacre_top: Option<u32>, seen: &mut Vec<u32>, mbs: &mut Vec<crate::ids::ObjRef>) {
    let add = |k: usize| HOGAAK_B_COUNTS[k].fetch_add(1, Ordering::Relaxed);
    if let Some(id) = massacre_top {
        if !g.stack().iter().any(|s| s.id == id) {
            add(4);
        }
    }
    for it in g.stack() {
        let crate::game::StackKind::Spell { card } = it.kind else { continue };
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        match g.db().def(g.own_def(card)).name.as_str() {
            "Mockingbird" => {
                add(0);
            }
            "Massacre" => {
                add(5);
                if it.alt == 1 {
                    add(3);
                }
            }
            _ => {}
        }
    }
    let mb = g.db().id("Mockingbird");
    for &c in g.battlefield() {
        if Some(g.own_def(c)) != mb {
            continue;
        }
        let r = g.obj_ref(c);
        if mbs.contains(&r) {
            continue;
        }
        // (Its copy choice still pending: counted once it is made.)
        if g.agenda.iter().any(|t| matches!(t, crate::flow::Task::EnterAsCopy { card, .. } if *card == c)) {
            continue;
        }
        mbs.push(r);
        if !g.is_copy(c) {
            add(2);
            continue;
        }
        add(1);
        if g.db().def(g.copiable(c).def).front_mv.is_some() {
            add(6);
        }
        if g.db().def(g.copiable(c).def).name == "Walking Ballista" && g.counters_on(c, crate::db::Counter::P1P1) > 0 {
            add(7);
        }
    }
}

/// The Doomsday 60 (MTGGoldfish 8027957, E:/mtg-zero-scratch/doomsday) with its cards of other
/// lanes (Doomsday, Pact of Negation, Personal Tutor, Flow State, Edge of Autumn, Street Wraith:
/// 12) swapped for Flusterstorm 4, Misdirection 2 more, Cavern of Souls 3 more and Orcish
/// Bowmasters 3 (`FUZZ_POOL=dd_b`). (An Orc Archer: random play's creature-type choices are
/// mostly the first 31 types, the first decision's own numbers, Archer among them, so a Cavern's
/// coloured mana casts a creature spell of its type in random games.)
const DD_B_DECK: &[(u32, &str)] = &[
    (1, "Swamp"),
    (1, "Flooded Strand"),
    (2, "Thoughtseize"),
    (4, "Misdirection"),
    (4, "Force of Will"),
    (4, "Underground Sea"),
    (1, "Misty Rainforest"),
    (4, "Lotus Petal"),
    (1, "Scalding Tarn"),
    (4, "Ponder"),
    (2, "Thassa's Oracle"),
    (2, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"),
    (1, "Consider"),
    (1, "Island"),
    (4, "Cavern of Souls"),
    (4, "Dark Ritual"),
    (1, "Verdant Catacombs"),
    (1, "Undercity Sewers"),
    (4, "Brainstorm"),
    (4, "Polluted Delta"),
    (1, "Lion's Eye Diamond"),
    (2, "Daze"),
    (4, "Flusterstorm"),
    (3, "Orcish Bowmasters"),
];

fn dd_b_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = DD_B_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let d = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    assert_eq!(d.len(), 60);
    d
}

/// `FUZZ_POOL=dd_b`: lane B's cards played (each stack item and permanent once).
static DD_B_MISDIRECTIONS: AtomicU64 = AtomicU64::new(0);
static DD_B_RETARGETS: AtomicU64 = AtomicU64::new(0);
static DD_B_FLUSTERSTORMS: AtomicU64 = AtomicU64::new(0);
static DD_B_COPIES: AtomicU64 = AtomicU64::new(0);
static DD_B_CAVERN_TYPES: AtomicU64 = AtomicU64::new(0);
static DD_B_TYPE_MANA: AtomicU64 = AtomicU64::new(0);
static DD_B_UNCOUNTERABLE: AtomicU64 = AtomicU64::new(0);

/// `FUZZ_POOL=dd_b`: count what the game shows now of lane B's cards (`seen`: stack item ids,
/// uncounterable spells' items and permanents counted already). A Misdirection's change is its
/// `NewTarget` decision as it resolves (on top of the stack; a storm copy's new targets are asked
/// as its trigger resolves).
fn count_dd_b(g: &Game, seen: &mut Vec<(u8, u32)>) {
    use crate::game::StackKind;
    if g.kind() == Some(crate::DecisionKind::NewTarget) {
        if let Some(StackKind::Spell { card }) = g.stack().last().map(|s| s.kind) {
            if g.name(card) == "Misdirection" {
                DD_B_RETARGETS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    for it in g.stack() {
        let StackKind::Spell { card } = it.kind else { continue };
        if !seen.contains(&(2, it.id)) && g.spell_cant_be_countered(card) && g.db().def(g.obj(card).def).is(crate::db::types::CREATURE) {
            seen.push((2, it.id));
            DD_B_UNCOUNTERABLE.fetch_add(1, Ordering::Relaxed);
        }
        if seen.contains(&(0, it.id)) {
            continue;
        }
        seen.push((0, it.id));
        match g.name(card) {
            "Misdirection" => DD_B_MISDIRECTIONS.fetch_add(1, Ordering::Relaxed),
            "Flusterstorm" if g.is_copy_obj(card) => DD_B_COPIES.fetch_add(1, Ordering::Relaxed),
            "Flusterstorm" => DD_B_FLUSTERSTORMS.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }
    for c in g.permanents_of(0).chain(g.permanents_of(1)) {
        if g.chosen_creature_type(c).is_some() && !seen.contains(&(1, c.get())) {
            seen.push((1, c.get()));
            DD_B_CAVERN_TYPES.fetch_add(1, Ordering::Relaxed);
        }
    }
    for p in 0..2 {
        if g.restricted_mana(p).iter().any(|k| k.1 & crate::db::SPEND_CHOSEN_CREATURE_TYPE != 0) {
            DD_B_TYPE_MANA.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn eoe_mode() -> bool {
    pool_mode() == PoolMode::Eoe
}

fn ddc_mode() -> bool {
    pool_mode() == PoolMode::DoomsdayC
}

/// `FUZZ_POOL=doomsday` (ENGINE.md 124.4).
fn doomsday_mode() -> bool {
    pool_mode() == PoolMode::Doomsday
}

pub fn sealed_mode() -> bool {
    pool_mode() == PoolMode::Sealed
}

/// The sealed spec of game `seed` (`FUZZ_POOL=sealed`): arena pools for even seeds, paper for odd;
/// with `FUZZ_BEST_SHARE=p` the mixed field's `"best_share": p` (ENGINE.md 107.1).
fn sealed_spec(seed: u64) -> &'static crate::az::MtgSpec {
    static SPECS: std::sync::OnceLock<[crate::az::MtgSpec; 2]> = std::sync::OnceLock::new();
    let specs = SPECS.get_or_init(|| {
        let share = match super::env::var("FUZZ_BEST_SHARE") {
            Ok(v) => format!(r#", "best_share": {}"#, v.parse::<f64>().expect("FUZZ_BEST_SHARE: a probability")),
            Err(_) => String::new(),
        };
        ["arena", "paper"].map(|c| crate::az::MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "{c}"{share}}}}}"#)).unwrap())
    });
    &specs[(seed % 2) as usize]
}

/// The decks and sideboards of sealed game `seed`, or why it has none (a pool holding a card
/// this checkout doesn't support).
fn sealed_decks(seed: u64) -> Result<[(Vec<u16>, Vec<u16>); 2], String> {
    Ok(sealed_spec(seed).try_seats(seed)?.map(|s| (s.deck, s.sideboard)))
}

/// `FUZZ_POOL=sealed`: game `seed` can't be dealt (its error), else None.
pub fn sealed_unplayable(seed: u64) -> Option<String> {
    if !sealed_mode() {
        return None;
    }
    sealed_decks(seed).err()
}

/// The supported cards of the Edge of Eternities table (`FUZZ_POOL=eoe`):
/// its lands (the basics among them) and its other cards.
fn eoe_pool() -> &'static (Vec<u16>, Vec<u16>) {
    static POOL: std::sync::OnceLock<(Vec<u16>, Vec<u16>)> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let db = CardDb::eoe();
        let (mut lands, mut spells) = (Vec::new(), Vec::new());
        for name in db.supported() {
            let id = db.id(name).unwrap();
            if db.def(id).is(crate::db::types::LAND) {
                lands.push(id);
            } else {
                spells.push(id);
            }
        }
        assert!(!lands.is_empty() && !spells.is_empty(), "FUZZ_POOL=eoe: no supported cards");
        (lands, spells)
    })
}

/// A random Edge of Eternities deck (`FUZZ_POOL=eoe`): 17 lands, 23 spells.
fn eoe_deck(rng: &mut Rng) -> Vec<u16> {
    let (lands, spells) = eoe_pool();
    let mut d: Vec<u16> = (0..17).map(|_| lands[rng.below(lands.len() as u32) as usize]).collect();
    d.extend((0..23).map(|_| spells[rng.below(spells.len() as u32) as usize]));
    d
}

/// The clone/determinize checkpoint's sampling denominator (module docs; `check_clone`,
/// `check_determinize`): 1-in-40 normally, 1-in-4 under the `fuzz` Cargo profile
/// (`debug-assertions = true`, root Cargo.toml): that profile's whole point is paying for extra
/// checking while the farm fuzzes, so the sampled determinize check runs far more often there
/// instead of at the release rate every other build (including plain `cargo test --release`) gets.
fn checkpoint_every() -> u32 {
    if let Some(k) = CHECKPOINT_EVERY_OVERRIDE.with(|c| c.get()) {
        return k;
    }
    if cfg!(debug_assertions) { 4 } else { 40 }
}

thread_local! {
    /// `checkpoint_every`'s value on this thread instead of the profile's (None: the profile's). For tests that replay
    /// a seed found under the `fuzz` profile in another profile: the denominator is drawn from the match's own random
    /// stream, so a seed is the same match only at the same denominator (`a_look_at_the_viewers_hand_in_a_step_that_
    /// begins_the_next_game`, ENGINE.md 173).
    pub static CHECKPOINT_EVERY_OVERRIDE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// `FUZZ_BELIEF=t`: the tilt of the belief-weighted deal's checks (ENGINE.md 69); unset: none.
fn belief_tilt() -> Option<f64> {
    super::env::var("FUZZ_BELIEF").ok().map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("FUZZ_BELIEF={s}: a tilt (a number > 0)"))).inspect(|&t| {
        assert!(t > 0.0 && t.is_finite(), "FUZZ_BELIEF={t}: a tilt > 0");
    })
}

/// `FUZZ_DECK_TILT=t`: the deck tilt of the hidden-decklist worlds (ENGINE.md 106); unset: none.
fn deck_tilt_fuzz() -> Option<f64> {
    super::env::var("FUZZ_DECK_TILT").ok().map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("FUZZ_DECK_TILT={s}: a tilt (a number > 0)"))).inspect(|&t| {
        assert!(t > 0.0 && t.is_finite(), "FUZZ_DECK_TILT={t}: a tilt > 0");
        assert!(hidden_decks_mode(), "FUZZ_DECK_TILT needs FUZZ_HIDDEN_DECKS=1 (the deck tilt deals hidden decklists)");
    })
}

/// Deck-tilted worlds dealt from the K cached decks, and K x the dealt deck's probability summed (x 1e6).
static DT_WORLDS: AtomicU64 = AtomicU64::new(0);
static DT_RATIO: AtomicU64 = AtomicU64::new(0);

/// The weighted deals checked, and the most favoured identity's dealt copies in the opponent's
/// unknown hand slots against the uniform deal's expected copies (x 1000, summed).
static BELIEF_WORLDS: AtomicU64 = AtomicU64::new(0);
static BELIEF_DEALT: AtomicU64 = AtomicU64::new(0);
static BELIEF_UNIFORM: AtomicU64 = AtomicU64::new(0);

/// Random per-identity weights for a weighted deal: exp(tilt x a standard normal-ish), clipped to
/// [1e-6, 1e6] (the extreme of `belief_weight`'s clip).
fn random_weights(rng: &mut Rng, tilt: f64) -> Vec<f64> {
    (0..crate::az::identity::D)
        .map(|_| {
            let z: f64 = (0..4).map(|_| rng.below(1 << 20) as f64 / (1u64 << 20) as f64).sum::<f64>() - 2.0;
            (tilt * 1.7 * z).exp().clamp(1e-6, 1e6)
        })
        .collect()
}

/// The checks of `check_determinize` on worlds dealt by random weights (`FUZZ_BELIEF`), for both
/// viewers; its own generator (the game's actions are unchanged).
fn check_determinize_weighted(g: &Game, seed: u64, step: usize, tilt: f64) {
    let mut rng = Rng::new(seed ^ (step as u64).wrapping_mul(0xBE11_EF5E_ED00_0001) ^ 0x7177);
    let mover = g.to_move().unwrap();
    let ids = crate::az::identity::identities(g.db());
    let cards = |g: &Game, p: u8| -> Vec<u16> {
        let mut v: Vec<u16> = g.objs().iter().enumerate().filter(|(_, o)| o.owner == p).map(|(i, _)| g.ident(crate::CardIdx::new(i as u32))).collect();
        v.sort();
        v
    };
    for viewer in [mover, 1 - mover] {
        let w = random_weights(&mut rng, tilt);
        let mut d = g.clone();
        d.determinize_weighted(viewer, &mut rng, &|x| w[ids[x as usize] as usize]);
        let what = format!("seed {seed} step {step}: a world dealt by weights for P{viewer} (mover P{mover})");
        // the cards a payment in progress pins stay where they are, as determinize keeps them
        for c in g.pinned_for(viewer) {
            assert_eq!(g.ident(c), d.ident(c), "{what}: a pinned card was dealt again");
        }
        if viewer == mover {
            let (mut l1, mut l2) = (Vec::new(), Vec::new());
            g.legal_actions(&mut l1);
            d.legal_actions(&mut l2);
            assert_eq!(l1, l2, "{what}: the decider's actions changed");
        } else {
            assert_eq!(d.to_move(), Some(mover), "{what}: a refresh ran past the decision");
        }
        assert_eq!(g.view_of(viewer), d.view_of(viewer), "{what}: the view changed");
        for (i, o) in g.objs().iter().enumerate() {
            let c = crate::CardIdx::new(i as u32);
            if g.knows(viewer, c) {
                assert_eq!((o.def, g.ident(c)), (d.obj(c).def, d.ident(c)), "{what}: a known card was dealt again");
            }
        }
        for p in 0..2u8 {
            assert_eq!(cards(g, p), cards(&d, p), "{what}: P{p}'s cards");
            assert_eq!(g.player(p).hand.len(), d.player(p).hand.len(), "{what}: P{p}'s hand size");
        }
        d.check_invariants().unwrap_or_else(|e| panic!("{what}: {e}"));
        d.check_knowledge().unwrap_or_else(|e| panic!("{what}: {e}"));
        let mut buf = Vec::new();
        for v in [0u8, 1] {
            check_encoding(&d, v, &mut buf, &|| what.clone());
        }
        check_options_complete(&d, seed, &what);
        // the concentration: the most favoured identity of the opponent's hidden pool, dealt copies
        // into its unknown hand slots against the uniform deal's m h / H
        let o = 1 - viewer;
        let pool = g.unknown_to(viewer, o);
        let unknown_hand: Vec<crate::CardIdx> = g.player(o).hand.iter().copied().filter(|&c| !g.knows(viewer, c)).collect();
        if !pool.is_empty() && !unknown_hand.is_empty() {
            let best = pool.iter().map(|&c| g.ident(c)).max_by(|&a, &b| w[ids[a as usize] as usize].total_cmp(&w[ids[b as usize] as usize]).then(b.cmp(&a))).unwrap();
            let h = pool.iter().filter(|&&c| g.ident(c) == best).count() as f64;
            let dealt = unknown_hand.iter().filter(|&&c| d.ident(c) == best).count() as f64;
            BELIEF_WORLDS.fetch_add(1, Ordering::Relaxed);
            BELIEF_DEALT.fetch_add((dealt * 1000.0) as u64, Ordering::Relaxed);
            BELIEF_UNIFORM.fetch_add((unknown_hand.len() as f64 * h / pool.len() as f64 * 1000.0) as u64, Ordering::Relaxed);
        }
        let (mut legal, mut keys) = (Vec::new(), Vec::new());
        let mut rc = RulesCheck::new(&d);
        rules_step(&mut rc, &d, &|| what.clone());
        for _ in 0..30 {
            if d.is_over() {
                break;
            }
            d.legal_actions(&mut legal);
            let act = legal[rng.below(legal.len() as u32) as usize];
            let at = d.describe_action(act).unwrap_or_else(|| format!("action {act}"));
            // (an engine panic in the world's own play: said with the world and the action)
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.apply(act, &mut keys))) {
                Ok(r) => r.unwrap_or_else(|e| panic!("{what}: {e}")),
                Err(_) => panic!("{what}: its play panicked at {at}; its log:
{}", d.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
")),
            }
            d.check_invariants().unwrap_or_else(|e| panic!("{what}: {e}"));
            rules_step(&mut rc, &d, &|| format!("{what}, played on"));
        }
        rules_add(&rc.stats);
    }
}

/// `FUZZ_CHANCE=n` (ENGINE.md 74): one decision in `n` (its own generator) runs a chance walk;
/// unset: none.
fn chance_every() -> Option<u32> {
    super::env::var("FUZZ_CHANCE").ok().map(|s| {
        let n: u32 = s.parse().unwrap_or_else(|_| panic!("FUZZ_CHANCE={s}: one decision in n (a number >= 1)"));
        assert!(n >= 1, "FUZZ_CHANCE={n}: n >= 1");
        n
    })
}

/// Steps of one chance walk at most (`check_forced_chance`).
const CHANCE_DEPTH: usize = 40;

/// Chance walks run, their steps with both worlds at one node, the forced replays whose
/// outcome the force changed (the replaying world, unforced, would have had other keys) and the
/// walks that ended because the replay's keys differed (a different child).
static CHANCE_WALKS: AtomicU64 = AtomicU64::new(0);
static CHANCE_STEPS: AtomicU64 = AtomicU64::new(0);
static CHANCE_FORCED: AtomicU64 = AtomicU64::new(0);
static CHANCE_SPLIT: AtomicU64 = AtomicU64::new(0);

/// zero-core's search transition (`tree::transition`): `act`, then every single-option decision
/// answered, the whole transition's chance keys returned; `forced`: the outcomes it replays
/// (`force_chance` once before, cleared after, as the search does).
fn chance_transition(g: &mut Game, act: u16, forced: Option<&[u64]>) -> Result<Vec<u64>, String> {
    let mut keys = Vec::new();
    let mut legal = Vec::new();
    if let Some(f) = forced {
        g.force_chance(f);
    }
    let r = (|| {
        g.apply(act, &mut keys)?;
        while !g.is_over() {
            g.legal_actions(&mut legal);
            if legal.len() != 1 {
                break;
            }
            g.apply(legal[0], &mut keys)?;
        }
        Ok(())
    })();
    if forced.is_some() {
        g.force_chance(&[]);
    }
    r.map(|()| keys)
}

/// Steps a chance walk's world plays on alone once it leaves the other's node.
const CHANCE_SOLO: usize = 20;

/// The message of a caught panic.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// One search transition of a chance walk's world, its invariants and knowledge checked after
/// it; a failure or an engine panic is reported with the walk (`at`, the path, the forced keys).
fn chance_step(w: &mut Game, rc: &mut RulesCheck, act: u16, forced: Option<&[u64]>, at: &str, path: &[String]) -> Vec<u64> {
    if forced.is_some() {
        rc.forced_chance(w);
    }
    let k = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| chance_transition(w, act, forced))) {
        Ok(Ok(k)) => k,
        Ok(Err(e)) => panic!("{at}: {e}\npath: {path:?}\nforced: {forced:?}"),
        Err(p) => panic!(
            "{at}: panicked: {}\npath: {path:?}\nforced: {forced:?}\n{}",
            panic_text(&*p),
            w.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
        ),
    };
    w.check_invariants().unwrap_or_else(|e| panic!("{at}: {e}\npath: {path:?}\nforced: {forced:?}"));
    w.check_knowledge().unwrap_or_else(|e| panic!("{at}: {e}\npath: {path:?}\nforced: {forced:?}"));
    rules_step(rc, w, &|| format!("{at}\npath: {path:?}\nforced: {forced:?}"));
    k
}

/// What zero-core's search does at `chance_k` > 0 (ENGINE.md 74): every simulation deals its own
/// world for the decider (uniform or, one time in two, by random weights: the belief-weighted
/// search), and an edge that has recorded `chance_k` outcomes replays one of them
/// (`force_chance`) in the world walking it. Two worlds of the decider walk the same random
/// path while they are at one node of the tree (the same next decider and legal actions, equal
/// keys so far); at each step one of them takes the action freely and the other replays its
/// keys (a search transition: single-option decisions answered, `chance_transition`), then
/// they may swap. When the replay's keys or the next legal actions differ, the replaying world
/// is in another node, and it plays on alone (`CHANCE_SOLO` random steps), as the search goes
/// on below its own child. Each world's invariants, knowledge and legal actions hold after
/// every step, every option of a replaying world's decision completes (whenever the force
/// changed its outcome, else one time in four), and an engine panic is reported with the walk.
/// Its own generator: the game is the same with or without it.
fn check_forced_chance(g: &Game, seed: u64, step: usize) {
    let mut rng = Rng::new(seed ^ (step as u64).wrapping_mul(0xC4A1_1CE5_0000_0001) ^ 0xC4A7);
    let mover = g.to_move().unwrap();
    let ids = crate::az::identity::identities(g.db());
    let deal = |rng: &mut Rng| -> Game {
        let mut d = g.clone();
        if rng.below(2) == 0 {
            d.determinize(mover, rng);
        } else {
            let w = random_weights(rng, 1.0);
            d.determinize_weighted(mover, rng, &|x| w[ids[x as usize] as usize]);
        }
        d
    };
    let mut free = deal(&mut rng);
    let mut replay = deal(&mut rng);
    let (mut rcf, mut rcr) = (RulesCheck::new(&free), RulesCheck::new(&replay));
    CHANCE_WALKS.fetch_add(1, Ordering::Relaxed);
    let (mut lf, mut lr) = (Vec::new(), Vec::new());
    let mut path: Vec<String> = Vec::new();
    let mut split = false;
    for depth in 0..CHANCE_DEPTH {
        if free.is_over() || replay.is_over() {
            return;
        }
        free.legal_actions(&mut lf);
        replay.legal_actions(&mut lr);
        assert!(lr.windows(2).all(|w| w[0] < w[1]) && !lr.is_empty(), "seed {seed} step {step}: chance walk {depth}: actions {lr:?}\npath: {path:?}");
        if free.to_move() != replay.to_move() || lf != lr {
            split = true; // different nodes of the tree (`replay`: the world forced last)
            break;
        }
        if depth > 0 && rng.below(2) == 0 {
            std::mem::swap(&mut free, &mut replay);
            std::mem::swap(&mut rcf, &mut rcr);
        }
        let act = lf[rng.below(lf.len() as u32) as usize];
        let at = |how: &str| format!("seed {seed} step {step}: chance walk (mover P{mover}) at {depth}, {how}, action {act}");
        path.push(format!("{act} = {}", free.describe_action(act).unwrap_or_default()));
        let kf = chance_step(&mut free, &mut rcf, act, None, &at("the free world"), &path);
        let unforced = chance_step(&mut replay.clone(), &mut rcr.clone(), act, None, &at("the replaying world unforced"), &path);
        let kr = chance_step(&mut replay, &mut rcr, act, Some(&kf), &at("the replaying world"), &path);
        CHANCE_STEPS.fetch_add(1, Ordering::Relaxed);
        let changed = unforced != kf && kr == kf;
        if changed {
            CHANCE_FORCED.fetch_add(1, Ordering::Relaxed);
        }
        if !replay.is_over() && (changed || rng.below(4) == 0) {
            check_options_complete(&replay, seed, &format!("a chance walk's replaying world (seed {seed} step {step}, depth {depth}, path {path:?})"));
        }
        if kr != kf {
            CHANCE_SPLIT.fetch_add(1, Ordering::Relaxed);
            split = true; // a different child
            break;
        }
    }
    if !split {
        rules_add(&rcf.stats);
        rules_add(&rcr.stats);
        return;
    }
    // The replaying world below its own node.
    for solo in 0..CHANCE_SOLO {
        if replay.is_over() {
            return;
        }
        replay.legal_actions(&mut lr);
        let act = lr[rng.below(lr.len() as u32) as usize];
        path.push(format!("(alone) {act} = {}", replay.describe_action(act).unwrap_or_default()));
        let at = format!("seed {seed} step {step}: chance walk (mover P{mover}), the replaying world alone at {solo}, action {act}");
        chance_step(&mut replay, &mut rcr, act, None, &at, &path);
    }
    rules_add(&rcf.stats);
    rules_add(&rcr.stats);
}

/// The card table of the mode's games: the cube's and the custom test cards,
/// or constructed play's (`FUZZ_POOL=delver`).
fn game_db() -> &'static CardDb {
    if constructed_mode() {
        CardDb::constructed()
    } else if eoe_mode() || sealed_mode() {
        CardDb::eoe()
    } else {
        super::cards::db()
    }
}

/// The pinned Delver 60 (`FUZZ_POOL=delver`).
fn delver_deck() -> Vec<u16> {
    let d = crate::az::constructed::Decklist::izzet_delver();
    crate::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap()
}

/// The Doomsday 60 of `FUZZ_POOL=dd_a` (ENGINE.md 124.1): MTGGoldfish's
/// "Legacy Doomsday" (deck 8027957), its seven cards of the other lanes
/// (Edge of Autumn, Misdirection, Street Wraith x2 each, Cavern of Souls)
/// swapped for two Pacts of Negation, two Flow States, two Personal Tutors
/// and an Island.
const DD_A_DECK: &[(u32, &str)] = &[
    (4, "Doomsday"),
    (3, "Pact of Negation"),
    (4, "Personal Tutor"),
    (3, "Flow State"),
    (2, "Thoughtseize"),
    (4, "Force of Will"),
    (4, "Lotus Petal"),
    (4, "Ponder"),
    (2, "Thassa's Oracle"),
    (2, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"),
    (1, "Consider"),
    (4, "Dark Ritual"),
    (4, "Brainstorm"),
    (1, "Lion's Eye Diamond"),
    (2, "Daze"),
    (1, "Swamp"),
    (2, "Island"),
    (4, "Underground Sea"),
    (1, "Undercity Sewers"),
    (1, "Flooded Strand"),
    (1, "Misty Rainforest"),
    (1, "Scalding Tarn"),
    (1, "Verdant Catacombs"),
    (4, "Polluted Delta"),
];

fn dd_a_deck() -> Vec<u16> {
    let list: Vec<(u32, String)> = DD_A_DECK.iter().map(|&(n, s)| (n, s.to_string())).collect();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("DD_A_DECK: {e}"));
    deck
}

/// `FUZZ_POOL=dd_a`: lane A's spells and Pact of Negation's triggers seen on
/// the stack after an action (each stack item once, by its id; a spell cast
/// and resolved within one action, every other decision answered by
/// `auto_single`, isn't seen: a Pact's trigger can outnumber its casts).
static DD_A_CASTS: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static DD_A_PACT_TRIGGERS: AtomicU64 = AtomicU64::new(0);
const DD_A_CARDS: [&str; 4] = ["Doomsday", "Pact of Negation", "Personal Tutor", "Flow State"];

fn count_dd_a(g: &Game, seen: &mut Vec<u32>) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => {
                if let Some(k) = DD_A_CARDS.iter().position(|&n| n == g.name(card)) {
                    DD_A_CASTS[k].fetch_add(1, Ordering::Relaxed);
                }
            }
            crate::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Pact of Negation" => {
                DD_A_PACT_TRIGGERS.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// `FUZZ_POOL=dd_c` (ENGINE.md 124.3): the Legacy Doomsday 60 (MTGGoldfish
/// archetype "Legacy Doomsday", deck 8027957) with the cards no branch
/// supports yet swapped out: Doomsday x4 -> Leyline of the Void x4 (from the
/// sideboard: on the battlefield in many openings), Misdirection x2 -> Street
/// Wraith (4 in all), Personal Tutor x2 -> Edge of Autumn (4 in all), Flow
/// State -> Duress, Pact of Negation -> Fatal Push (both from the sideboard),
/// Cavern of Souls -> Forest (green for Edge of Autumn's spell, fetchable).
const DOOMSDAY_C: &[(u8, &str)] = &[
    (1, "Swamp"),
    (1, "Forest"),
    (1, "Island"),
    (4, "Underground Sea"),
    (1, "Undercity Sewers"),
    (1, "Flooded Strand"),
    (1, "Misty Rainforest"),
    (1, "Scalding Tarn"),
    (1, "Verdant Catacombs"),
    (4, "Polluted Delta"),
    (4, "Lotus Petal"),
    (1, "Lion's Eye Diamond"),
    (4, "Dark Ritual"),
    (4, "Brainstorm"),
    (4, "Ponder"),
    (1, "Consider"),
    (4, "Force of Will"),
    (2, "Daze"),
    (2, "Thoughtseize"),
    (1, "Duress"),
    (1, "Fatal Push"),
    (2, "Thassa's Oracle"),
    (2, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"),
    (4, "Edge of Autumn"),
    (4, "Street Wraith"),
    (4, "Leyline of the Void"),
];

fn doomsday_c_deck() -> Vec<u16> {
    let db = CardDb::constructed();
    let d: Vec<u16> =
        DOOMSDAY_C.iter().flat_map(|&(n, name)| std::iter::repeat_n(db.expect_id(name).unwrap_or_else(|e| panic!("{e}")), n as usize)).collect();
    assert_eq!(d.len(), 60);
    d
}

/// `FUZZ_POOL=dd_c`: game `seed`'s decks: Doomsday against Delver (by `seed %
/// 3`: 0 Doomsday in seat 0, 1 in seat 1) and the Doomsday mirror (2).
fn ddc_decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    match seed % 3 {
        0 => [(doomsday_c_deck(), Vec::new()), (delver_deck(), Vec::new())],
        1 => [(delver_deck(), Vec::new()), (doomsday_c_deck(), Vec::new())],
        _ => [(doomsday_c_deck(), Vec::new()), (doomsday_c_deck(), Vec::new())],
    }
}

/// `FUZZ_POOL=dd_c`: lane C's cards cast and cycled (stack items, each once),
/// games with a Leyline of the Void on the battlefield as the first turn
/// begins (the opening-hand action) and at any time, games with a card of a
/// player in exile while an opponent's Leyline is on the battlefield, and
/// declare-blockers decisions facing an attacking swampwalker whose decider
/// controls a Swamp (checked: the swampwalker is never offered).
static DDC_EDGE_CAST: AtomicU64 = AtomicU64::new(0);
static DDC_EDGE_CYCLED: AtomicU64 = AtomicU64::new(0);
static DDC_WRAITH_CAST: AtomicU64 = AtomicU64::new(0);
static DDC_WRAITH_CYCLED: AtomicU64 = AtomicU64::new(0);
static DDC_LEYLINE_OPENING: AtomicU64 = AtomicU64::new(0);
static DDC_LEYLINE_GAMES: AtomicU64 = AtomicU64::new(0);
static DDC_LEYLINE_EXILE_GAMES: AtomicU64 = AtomicU64::new(0);
static DDC_SWAMPWALK_ATTACKS: AtomicU64 = AtomicU64::new(0);

/// Per game (`FUZZ_POOL=dd_c`): what `count_ddc` has counted.
#[derive(Default)]
struct DdcSeen {
    opening_checked: bool,
    leyline: bool,
    leyline_exile: bool,
}

fn count_ddc(g: &Game, seen: &mut Vec<u32>, s: &mut DdcSeen) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => match g.name(card) {
                "Edge of Autumn" => DDC_EDGE_CAST.fetch_add(1, Ordering::Relaxed),
                "Street Wraith" => DDC_WRAITH_CAST.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            },
            crate::game::StackKind::Ability { source, .. } => match g.db().def(g.obj(source.idx).def).name.as_str() {
                "Edge of Autumn" => DDC_EDGE_CYCLED.fetch_add(1, Ordering::Relaxed),
                "Street Wraith" => DDC_WRAITH_CYCLED.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            },
        };
    }
    let leylines: Vec<u8> = g.battlefield().iter().filter(|&&c| g.name(c) == "Leyline of the Void").map(|&c| g.obj(c).controller).collect();
    if !s.opening_checked && g.turn().number >= 1 {
        s.opening_checked = true;
        if !leylines.is_empty() {
            DDC_LEYLINE_OPENING.fetch_add(1, Ordering::Relaxed);
        }
    }
    if !leylines.is_empty() && !s.leyline {
        s.leyline = true;
        DDC_LEYLINE_GAMES.fetch_add(1, Ordering::Relaxed);
    }
    if !s.leyline_exile && leylines.iter().any(|&q| g.exile_zone().iter().any(|&c| g.obj(c).owner != q)) {
        s.leyline_exile = true;
        DDC_LEYLINE_EXILE_GAMES.fetch_add(1, Ordering::Relaxed);
    }
    // Swampwalk (CR 702.14c): a blocker of a defending player who controls a
    // land with the land type Swamp is never offered an attacking swampwalker.
    if g.kind() == Some(DecisionKind::DeclareBlockers) {
        let d = g.pending_decision().unwrap().player;
        let swamp = g.db().subtype_id("Swamp");
        let has_swamp = swamp.is_some_and(|s| {
            g.battlefield().iter().any(|&c| g.obj(c).controller == d && g.types_of(c) & crate::db::types::LAND != 0 && g.has_subtype(c, s))
        });
        for &c in g.battlefield() {
            if has_swamp && g.obj(c).attacking && g.has_kw(c, crate::db::Kw::Swampwalk) {
                DDC_SWAMPWALK_ATTACKS.fetch_add(1, Ordering::Relaxed);
                assert!(!g.has_option(Choice::Card(c)), "a block of the swampwalking {} is offered to a player with a Swamp", g.name(c));
            }
        }
    }
}

/// `FUZZ_POOL=doomsday` (ENGINE.md 124.4): the real Legacy Doomsday 60 (the
/// main deck of `data/decks/doomsday_8027957.json`; best-of-one, no
/// sideboard).
fn doomsday_deck() -> Vec<u16> {
    let d = crate::az::constructed::Decklist::legacy_doomsday();
    let deck = crate::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap();
    crate::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("the Doomsday 60: {e}"));
    deck
}

/// `FUZZ_POOL=doomsday`: game `seed`'s decks: the Doomsday 60 against the
/// Delver 60 (by `seed % 3`: 0 Doomsday in seat 0, 1 in seat 1) and the
/// Doomsday mirror (2).
fn doomsday_decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    match seed % 3 {
        0 => [(doomsday_deck(), Vec::new()), (delver_deck(), Vec::new())],
        1 => [(delver_deck(), Vec::new()), (doomsday_deck(), Vec::new())],
        _ => [(doomsday_deck(), Vec::new()), (doomsday_deck(), Vec::new())],
    }
}

/// `FUZZ_POOL=doomsday`: the legacy cards' spells (and Thassa's Oracle) seen
/// on the stack after an action, each stack item once by its id (a spell
/// cast and resolved within one action, every other decision answered by
/// `auto_single`, isn't seen); their abilities likewise (Pact of Negation's
/// trigger, Edge of Autumn's and Street Wraith's cycling, Murktide Regent's
/// trigger); Misdirection's new-target decisions; Caverns' creature types
/// chosen (each permanent once); creature spells that can't be countered.
const DOOMSDAY_SPELLS: [&str; 10] = [
    "Doomsday",
    "Pact of Negation",
    "Personal Tutor",
    "Flow State",
    "Misdirection",
    "Edge of Autumn",
    "Street Wraith",
    "Thassa's Oracle",
    "Murktide Regent",
    "Prismari Charm",
];
static DOOMSDAY_CASTS: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
const DOOMSDAY_ABILITIES: [&str; 4] = ["Pact of Negation", "Edge of Autumn", "Street Wraith", "Murktide Regent"];
static DOOMSDAY_ABILITY_COUNTS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static DOOMSDAY_RETARGETS: AtomicU64 = AtomicU64::new(0);
static DOOMSDAY_CAVERN_TYPES: AtomicU64 = AtomicU64::new(0);
static DOOMSDAY_UNCOUNTERABLE: AtomicU64 = AtomicU64::new(0);

/// Per game (`FUZZ_POOL=doomsday`): the stack items, uncounterable creature
/// spells' items and Caverns counted already.
#[derive(Default)]
struct DoomsdaySeen {
    stack: Vec<u32>,
    uncounterable: Vec<u32>,
    caverns: Vec<u32>,
}

fn count_doomsday(g: &Game, s: &mut DoomsdaySeen) {
    use crate::game::StackKind;
    if g.kind() == Some(DecisionKind::NewTarget) {
        if let Some(StackKind::Spell { card }) = g.stack().last().map(|it| it.kind) {
            if g.name(card) == "Misdirection" {
                DOOMSDAY_RETARGETS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    for it in g.stack() {
        if let StackKind::Spell { card } = it.kind {
            if !s.uncounterable.contains(&it.id) && g.spell_cant_be_countered(card) && g.db().def(g.obj(card).def).is(crate::db::types::CREATURE) {
                s.uncounterable.push(it.id);
                DOOMSDAY_UNCOUNTERABLE.fetch_add(1, Ordering::Relaxed);
            }
        }
        if s.stack.contains(&it.id) {
            continue;
        }
        s.stack.push(it.id);
        match it.kind {
            StackKind::Spell { card } => {
                if let Some(k) = DOOMSDAY_SPELLS.iter().position(|&n| n == g.name(card)) {
                    DOOMSDAY_CASTS[k].fetch_add(1, Ordering::Relaxed);
                }
            }
            StackKind::Ability { source, .. } => {
                let name = g.db().def(g.obj(source.idx).def).name.as_str();
                if let Some(k) = DOOMSDAY_ABILITIES.iter().position(|&n| n == name) {
                    DOOMSDAY_ABILITY_COUNTS[k].fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    for c in g.permanents_of(0).chain(g.permanents_of(1)) {
        if g.chosen_creature_type(c).is_some() && !s.caverns.contains(&c.get()) {
            s.caverns.push(c.get());
            DOOMSDAY_CAVERN_TYPES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `FUZZ_POOL=delver` (and `delver_dealt`, `delver_dealt2`): Murktide Regents and Prismari Charms cast, and
/// leave-the-graveyard triggers of the Regents put on the stack - each seen on the stack after a step
/// (`count_delver`; one that comes and goes inside one step, every priority in it answered by the engine,
/// `auto_single`, is not seen).
static DELVER_MURKTIDES: AtomicU64 = AtomicU64::new(0);
static DELVER_CHARMS: AtomicU64 = AtomicU64::new(0);
static DELVER_MURK_TRIGGERS: AtomicU64 = AtomicU64::new(0);
/// The most actions any game took (the fuzzer's cap is 100,000).
static MAX_GAME_STEPS: AtomicU64 = AtomicU64::new(0);
/// Decisions of a controlled player made by their controller (CR 722.4;
/// ENGINE.md 100.5: encoded in the controlled seat).
static CONTROLLED: AtomicU64 = AtomicU64::new(0);

/// The most decisions in one turn of a game (loops of optional actions have
/// no cap, `engine/src/loops.rs`): (decisions, seed, turn).
static LONGEST_TURN: std::sync::Mutex<(u64, u64, u32)> = std::sync::Mutex::new((0, 0, 0));
/// Turns with more than `LONG_TURN` decisions.
static LONG_TURNS: AtomicU64 = AtomicU64::new(0);
const LONG_TURN: u64 = 1000;

/// A turn of game `seed` took `n` decisions.
fn record_turn(seed: u64, turn: u32, n: u64) {
    if n > LONG_TURN {
        LONG_TURNS.fetch_add(1, Ordering::Relaxed);
    }
    let mut l = LONGEST_TURN.lock().unwrap();
    if n > l.0 {
        *l = (n, seed, turn);
    }
}

fn longest_turn_report() -> String {
    let l = *LONGEST_TURN.lock().unwrap();
    format!("longest turn: {} decisions (seed {}, turn {}); turns past {LONG_TURN} decisions: {}", l.0, l.1, l.2, LONG_TURNS.load(Ordering::Relaxed))
}

/// `FUZZ_SKIP=s1,s2,...`: seeds the game and match loops leave out (a known
/// failure of another workstream's).
fn skipped(seed: u64) -> bool {
    static SKIP: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    SKIP.get_or_init(|| {
        super::env::var("FUZZ_SKIP")
            .map(|s| s.split(',').filter(|x| !x.trim().is_empty()).map(|x| x.trim().parse().unwrap_or_else(|_| panic!("FUZZ_SKIP={s}: seeds"))).collect())
            .unwrap_or_default()
    })
    .contains(&seed)
}

/// `FUZZ_POOL=delver`: count the Regents and Charms cast and the Regents'
/// triggers put on the stack (each stack item once, by its id).
fn count_delver(g: &Game, seen: &mut Vec<u32>) {
    for it in g.stack() {
        if seen.contains(&it.id) {
            continue;
        }
        seen.push(it.id);
        match it.kind {
            crate::game::StackKind::Spell { card } => match g.name(card) {
                "Murktide Regent" => DELVER_MURKTIDES.fetch_add(1, Ordering::Relaxed),
                "Prismari Charm" => DELVER_CHARMS.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            },
            crate::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Murktide Regent" => {
                DELVER_MURK_TRIGGERS.fetch_add(1, Ordering::Relaxed)
            }
            _ => 0,
        };
    }
}

/// An exponential deck (40 cards: 12 Forests, 5 random lands, 23 from
/// `GROWTH`) and a sideboard of seven from `GROWTH`.
fn exponential_deck(db: &CardDb, rng: &mut Rng) -> (Vec<u16>, Vec<u16>) {
    let growth: Vec<u16> = GROWTH.iter().map(|n| db.expect_id(n).unwrap_or_else(|e| panic!("GROWTH card: {e}"))).collect();
    let (lands, _) = super::cards::fuzz_pool();
    let forest = db.expect_id("Forest").unwrap();
    let mut deck: Vec<u16> = vec![forest; 12];
    deck.extend((0..5).map(|_| lands[rng.below(lands.len() as u32) as usize]));
    deck.extend((0..23).map(|_| growth[rng.below(growth.len() as u32) as usize]));
    let side = (0..7).map(|_| growth[rng.below(growth.len() as u32) as usize]).collect();
    (deck, side)
}

/// The largest numbers the fuzzed games reached (ENGINE.md 46; printed).
static MAX_POWER: AtomicI64 = AtomicI64::new(0);
static MAX_COUNTERS: AtomicI64 = AtomicI64::new(0);
static MAX_DAMAGE: AtomicI64 = AtomicI64::new(0);
static MAX_LIFE: AtomicI64 = AtomicI64::new(0);
/// Encodings checked (`check_bounded`), and those the squash changed (some
/// value beyond `encode::SQUASH_FROM`).
static ENCODINGS: AtomicU64 = AtomicU64::new(0);
static SQUASHED: AtomicU64 = AtomicU64::new(0);

fn record_extremes(g: &Game) {
    // (A power is a derived `i128`: past `i64::MAX` it is recorded as
    // `i64::MAX`, "at least". Counts, marked damage and life totals are
    // within it: the engine keeps them exact or aborts.)
    let big = |v: u128| i64::try_from(v).unwrap_or(i64::MAX);
    for &c in g.battlefield() {
        MAX_POWER.fetch_max(big(g.power(c).unsigned_abs()), Ordering::Relaxed);
        MAX_DAMAGE.fetch_max(big(g.obj(c).damage as u128), Ordering::Relaxed);
        for (_, n) in g.counters_of(c) {
            MAX_COUNTERS.fetch_max(big(n as u128), Ordering::Relaxed);
        }
    }
    for p in 0..2 {
        MAX_LIFE.fetch_max(big(g.life(p).unsigned_abs() as u128), Ordering::Relaxed);
    }
}

fn extremes_report() -> String {
    format!(
        "largest power {}, counters {}, marked damage {}, |life| {}; {} encodings checked, {} with a value past {} (squashed)",
        MAX_POWER.load(Ordering::Relaxed),
        MAX_COUNTERS.load(Ordering::Relaxed),
        MAX_DAMAGE.load(Ordering::Relaxed),
        MAX_LIFE.load(Ordering::Relaxed),
        ENCODINGS.load(Ordering::Relaxed),
        SQUASHED.load(Ordering::Relaxed),
        crate::az::encode::SQUASH_FROM
    )
}

/// Every value of an encoding is finite and within the bound (ENGINE.md 46).
fn check_bounded(x: &[f32], what: &dyn Fn() -> String) {
    use crate::az::encode::{FEATURE_BOUND, SQUASH_FROM};
    ENCODINGS.fetch_add(1, Ordering::Relaxed);
    let mut squashed = false;
    for (i, &v) in x.iter().enumerate() {
        assert!(v.is_finite() && v.abs() <= FEATURE_BOUND, "{}: feature {i} is {v} (bound {FEATURE_BOUND})", what());
        squashed |= v.abs() > SQUASH_FROM;
    }
    SQUASHED.fetch_add(squashed as u64, Ordering::Relaxed);
}

/// Graveyard-list decisions checked, the longest list, the most cards
/// offered from both graveyards (ENGINE.md 65.10: the list then holds the
/// offered cards only; more than 64 fails loudly).
static GY_LISTS: AtomicU64 = AtomicU64::new(0);
static GY_LIST_MAX: AtomicU64 = AtomicU64::new(0);
static GY_LIST_BOTH_MAX: AtomicU64 = AtomicU64::new(0);

/// The pending decision's graveyard list (ENGINE.md 65.10) in the decider's
/// row: each option naming a card in a graveyard is `LIBRARY` + k, the card at
/// the page's first position + k of the list (layout v7: absolute positions,
/// ENGINE.md 125; the offered cards' graveyard whole, in order; offered cards
/// in both: those only, the decider's first), and the list token there is that
/// card (its identity, present, an option, whose graveyard); no option of any
/// decision but Priority keeps a `GRAVEYARD` id.
fn check_graveyard_list(g: &Game, row: &crate::az::tail_encode::Row, what: &dyn Fn() -> String) {
    use crate::act;
    use crate::az::encode::{list, CODE};
    use crate::az::tail_encode::seg;
    use crate::{Target, Zone};
    let Some(d) = g.pending_decision() else { return };
    if d.kind != DecisionKind::Priority {
        assert!(
            !d.options.iter().any(|o| (act::GRAVEYARD..act::GRAVEYARD + 2 * act::GRAVEYARD_N).contains(&o.action)),
            "{}: {:?} keeps a GRAVEYARD id",
            what(),
            d.kind
        );
    }
    // (On a page of a paged list, the page's cards at their slots: ENGINE.md 100.2.)
    let slots = g.decision_graveyard_list(d.player, d);
    let gy: Vec<crate::CardIdx> = slots.iter().flatten().copied().collect();
    if gy.is_empty() {
        return;
    }
    let first = d.range_first(act::LIBRARY, act::LIBRARY_N);
    GY_LISTS.fetch_add(1, Ordering::Relaxed);
    GY_LIST_MAX.fetch_max(gy.len() as u64, Ordering::Relaxed);
    if gy.iter().any(|&c| g.obj(c).owner == d.player) && gy.iter().any(|&c| g.obj(c).owner != d.player) {
        GY_LIST_BOTH_MAX.fetch_max(gy.len() as u64, Ordering::Relaxed);
        assert!(
            gy.iter().all(|&c| d.options.iter().any(|o| o.action != act::PASS && matches!(o.choice, Choice::Card(x) if x == c) || matches!(o.choice, Choice::Target(Target::Obj(r)) if r.idx == c))),
            "{}: a list over both graveyards holds the offered cards only",
            what()
        );
    }
    let ids = crate::az::identity::identities(g.db());
    let mut named = 0;
    for o in &d.options {
        let c = match o.choice {
            Choice::Card(i) => i,
            Choice::Target(Target::Obj(r)) => r.idx,
            // (A cast or play from the graveyard, ENGINE.md 100.5.)
            Choice::Cast(i) | Choice::Play(i) | Choice::CastFace(i) | Choice::PlayFace(i) | Choice::Faces(i, _) => i,
            _ => continue,
        };
        if g.obj(c).zone != Zone::Graveyard {
            continue;
        }
        let k = slots.iter().position(|&y| y == Some(c)).unwrap_or_else(|| panic!("{}: {} not in its graveyard list", what(), g.name(c)));
        assert_eq!(o.action, act::LIBRARY + k as u16, "{}: {:?} {} is list slot {k}", what(), d.kind, g.name(c));
        let pos = first + k as u32;
        let x = row.token(seg::LIST, pos).unwrap_or_else(|| panic!("{}: {:?} {}: no list token at {pos}", what(), d.kind, g.name(c)));
        assert_eq!(crate::az::tail_encode::code_identity(&x[..CODE]), Some(ids[g.obj(c).def as usize] as usize), "{}: {:?} position {pos}'s token isn't {}", what(), d.kind, g.name(c));
        assert_eq!((x[CODE + list::PRESENT], x[CODE + list::OPTION]), (1.0, 1.0), "{}: {:?} position {pos}", what(), d.kind);
        assert_eq!(x[CODE + list::THEIRS], (g.obj(c).owner != d.player) as u8 as f32, "{}: position {pos}", what());
        named += 1;
    }
    assert!(named > 0, "{}: a graveyard list without a graveyard option", what());
}

/// Rows read back (`FUZZ_CAP_CENSUS` and the invariants): every option's pointer checked,
/// the v6 projections compared, the rows past the limit.
static POINTERS: AtomicU64 = AtomicU64::new(0);
static V6_SAME: AtomicU64 = AtomicU64::new(0);
static V6_PAGED: AtomicU64 = AtomicU64::new(0);
static ROWS_OVER_LIMIT: AtomicU64 = AtomicU64::new(0);
static MAX_CAPACITY: AtomicU64 = AtomicU64::new(0);

/// Is the pending decision a page layout v6 showed as a window (a range page, or a paged list's
/// first page whose slot 63 v6 left empty)? There the v6 projection of the v7 head differs from
/// v6's encoding by design (ENGINE.md 125).
fn v6_window(g: &Game) -> bool {
    let Some(d) = g.pending_decision() else { return false };
    match d.stage {
        crate::decision::Stage::Paged { window } if window > 0 => true,
        crate::decision::Stage::Paged { .. } => d.options.iter().any(|o| o.action == crate::act::LIBRARY + crate::act::LIBRARY_N - 1),
        _ => false,
    }
}

/// Layout v7's encoding invariants on `viewer`'s row of `g` (ENGINE.md 125): every value finite
/// and bounded (head and tail); the head is `encode_features`' when the row has no tail (a row with
/// one is refused there: R1, `tests/v7_tails.rs`); the head's v6 projection is layout v6's encoding
/// (`mtg_eoe`'s path) but on a page v6 showed as a window; every option of the viewer's decision
/// points at its object's token (`check_pointers`); the row's capacity within the limit (counted:
/// zero-core's driver aborts such a game); the census (`FUZZ_CAP_CENSUS`).
fn check_row(g: &Game, viewer: u8, row: &crate::az::tail_encode::Row, what: &dyn Fn() -> String) {
    use crate::az::encode::{v6_projection, NUM_FEATURES_V6};
    use crate::az::tail_encode::seg;
    let vals: Vec<f32> = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).copied().collect();
    check_bounded(&vals, &|| format!("{} (viewer P{viewer})", what()));
    // Layout v8's row (ENGINE.md 236) is layout v7's plus its own segments' tokens and the overrides into
    // `pay_options`: its v7 part is the v7 row exactly (a v7 network widened onto v8 reads it alike).
    let r7 = crate::az::tail_encode::Row::of(g, viewer);
    let tail7: Vec<_> = row.tail.iter().filter(|t| (t.0 as usize) < seg::N).cloned().collect();
    let ov7: Vec<(u32, i64)> = row.ov.iter().copied().filter(|&(_, t)| t < 0 || zero_search::tail::unpack_tok(t).0 != seg::PAY_OPTIONS).collect();
    assert!(r7.head == row.head && r7.tail == tail7 && r7.ov == ov7, "{}: layout v8's row is not layout v7's plus v8's segments (viewer P{viewer})", what());
    V8_TOKENS.fetch_add(row.tail.len() as u64 - tail7.len() as u64, Ordering::Relaxed);
    if r7.tail.is_empty() {
        let mut h = vec![0.0f32; crate::az::NUM_FEATURES];
        g.encode_features(viewer, &mut h);
        assert!(h.iter().zip(&row.head).all(|(a, b)| a.to_bits() == b.to_bits()), "{}: encode_features is not encode_row's head (viewer P{viewer})", what());
    }
    static P6: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
    let p = P6.get_or_init(v6_projection);
    let mut v6 = vec![0.0f32; NUM_FEATURES_V6];
    g.encode_features_v6_check(viewer, &mut v6);
    if p.iter().zip(&v6).all(|(&i, &y)| row.head[i].to_bits() == y.to_bits()) {
        V6_SAME.fetch_add(1, Ordering::Relaxed);
    } else {
        let i = p.iter().zip(&v6).position(|(&i, &y)| row.head[i].to_bits() != y.to_bits()).unwrap();
        assert!(v6_window(g), "{}: the v6 projection of the v7 head differs from layout v6's encoding at v6 feature {i} outside a page (viewer P{viewer})", what());
        V6_PAGED.fetch_add(1, Ordering::Relaxed);
    }
    let n = crate::az::tail_encode::check_pointers_layout(g, viewer, row, true).unwrap_or_else(|e| panic!("{}: {e} (viewer P{viewer})", what()));
    POINTERS.fetch_add(n as u64, Ordering::Relaxed);
    let cap = row.capacity() as u64;
    MAX_CAPACITY.fetch_max(cap, Ordering::Relaxed);
    if cap > crate::az::tail_encode::MAX_ROW_TOKENS as u64 {
        ROWS_OVER_LIMIT.fetch_add(1, Ordering::Relaxed);
    }
    census::record(g, viewer, row);
}

/// `viewer`'s whole row of `g` (`buf`: the row as one vector, `Row::flat`, the head first), checked
/// (`check_row`).
fn check_encoding(g: &Game, viewer: u8, buf: &mut Vec<f32>, what: &dyn Fn() -> String) {
    let row = crate::az::tail_encode::Row::of_v8(g, viewer);
    check_row(g, viewer, &row, what);
    *buf = row.flat();
}

/// Layout v8's own tokens in the rows checked (ENGINE.md 236).
static V8_TOKENS: AtomicU64 = AtomicU64::new(0);

/// The rows' report: pointers checked, v6 projections, the limit.
fn rows_report() -> String {
    format!(
        "rows: {} options' pointers checked; layout v8's own tokens {}; v6 projection equal on {} rows, {} on pages v6 showed as a window; the most tokens in a row {} (limit {}), {} rows past it",
        POINTERS.load(Ordering::Relaxed),
        V8_TOKENS.load(Ordering::Relaxed),
        V6_SAME.load(Ordering::Relaxed),
        V6_PAGED.load(Ordering::Relaxed),
        MAX_CAPACITY.load(Ordering::Relaxed),
        crate::az::tail_encode::MAX_ROW_TOKENS,
        ROWS_OVER_LIMIT.load(Ordering::Relaxed)
    )
}

/// `FUZZ_CAP_CENSUS=1` (ENGINE.md 125; API 10.6): at every decision both viewers' rows, per pool:
/// per capped segment the rows past its head slots and the most objects; the rows with tokens of
/// each tail-only segment (1 and 2+ for exile offers), opponent triggers, knowledge at depth >= 3,
/// stack items with 3+ targets, graveyard-order tokens; the tokens per row (the head's present
/// tokens - layout v6's count - and v7's total) mean, p99, max. Written as JSON to
/// `FUZZ_CAP_CENSUS_OUT` (default `target/fuzz_cap_census_<pool>.json`) and printed.
mod census {
    use super::*;
    use crate::az::tail_encode::{seg, Row, SEG_HEAD};

    #[derive(Default)]
    pub struct Census {
        pub rows: u64,
        pub past_head: [u64; seg::N],
        pub max_objects: [u64; seg::N],
        pub with_tokens: [u64; seg::N],
        pub exile_offers_2: u64,
        pub targets_3: u64,
        pub tokens_v6: Vec<u32>,
        pub tokens_v7: Vec<u32>,
        pub tail_tokens: u64,
    }

    pub static C: std::sync::Mutex<Option<Census>> = std::sync::Mutex::new(None);

    pub fn on() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| crate::fuzz::env::var("FUZZ_CAP_CENSUS").is_ok_and(|v| v == "1"))
    }

    pub fn record(g: &Game, _viewer: u8, row: &Row) {
        if !on() {
            return;
        }
        let mut objs = [0u64; seg::N];
        for sg in 0..seg::N as u16 {
            objs[sg as usize] = row.tokens(sg).iter().map(|t| t.0 as u64 + 1).max().unwrap_or(0);
        }
        let present_head: u32 = (0..seg::N as u16).map(|sg| row.tokens(sg).iter().filter(|t| (t.0 as usize) < SEG_HEAD[sg as usize]).count() as u32).sum();
        let targets_3 = g.stack().iter().any(|s| s.targets.len() >= 3);
        let mut c = C.lock().unwrap();
        let c = c.get_or_insert_with(Census::default);
        c.rows += 1;
        for i in 0..seg::N {
            if SEG_HEAD[i] > 0 && objs[i] > SEG_HEAD[i] as u64 {
                c.past_head[i] += 1;
            }
            c.max_objects[i] = c.max_objects[i].max(objs[i]);
            if row.tail.iter().any(|t| t.0 as usize == i) {
                c.with_tokens[i] += 1;
            }
        }
        if row.tail.iter().filter(|t| t.0 == seg::EXILE_OFFERED).count() >= 2 {
            c.exile_offers_2 += 1;
        }
        c.targets_3 += targets_3 as u64;
        c.tokens_v6.push(1 + present_head);
        c.tokens_v7.push(1 + present_head + row.tail.len() as u32);
        c.tail_tokens += row.tail.len() as u64;
    }

    fn stats(v: &mut [u32]) -> serde_json::Value {
        if v.is_empty() {
            return serde_json::json!(null);
        }
        v.sort_unstable();
        let mean = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
        let p99 = v[((v.len() as f64 * 0.99) as usize).min(v.len() - 1)];
        serde_json::json!({"mean": (mean * 100.0).round() / 100.0, "p99": p99, "max": v[v.len() - 1]})
    }

    /// The census as JSON (`pool`: the `FUZZ_POOL`), written and returned.
    pub fn report(pool: &str, games: u64) -> Option<String> {
        if !on() {
            return None;
        }
        let mut g = C.lock().unwrap();
        let c = g.get_or_insert_with(Census::default);
        let names = ["context", "tops", "my_permanents", "opponent_permanents", "hand", "stack", "triggers", "list", "graveyard_abilities", "my_companion", "opponent_companion", "opponent_triggers", "delayed", "links", "exile_offered", "library_known", "stack_relations", "graveyard_order", "face_down", "my_revealed"];
        let share = |n: u64| if c.rows == 0 { 0.0 } else { (n as f64 / c.rows as f64 * 1e6).round() / 1e4 };
        let mut capped = serde_json::Map::new();
        let mut tail_only = serde_json::Map::new();
        for i in 0..seg::N {
            if SEG_HEAD[i] > 0 && ![seg::CONTEXT as usize, seg::TOPS as usize, seg::MY_COMP as usize, seg::OPP_COMP as usize].contains(&i) {
                capped.insert(names[i].into(), serde_json::json!({"head": SEG_HEAD[i], "rows_past_head": c.past_head[i], "share_pct": share(c.past_head[i]), "max_objects": c.max_objects[i]}));
            } else if SEG_HEAD[i] == 0 {
                tail_only.insert(names[i].into(), serde_json::json!({"rows_with_tokens": c.with_tokens[i], "share_pct": share(c.with_tokens[i]), "max_tokens": c.max_objects[i]}));
            }
        }
        let v = serde_json::json!({
            "pool": pool, "games": games, "rows": c.rows, "viewers": "both, every decision",
            "capped_segments": capped,
            "tail_only_segments": tail_only,
            "exile_offers_2_plus": {"rows": c.exile_offers_2, "share_pct": share(c.exile_offers_2)},
            "stack_item_with_3_plus_targets": {"rows": c.targets_3, "share_pct": share(c.targets_3)},
            "tail_tokens_total": c.tail_tokens,
            "tokens_per_row_v6_present": stats(&mut c.tokens_v6),
            "tokens_per_row_v7_total": stats(&mut c.tokens_v7),
        });
        let s = serde_json::to_string_pretty(&v).unwrap();
        let path = crate::fuzz::env::var("FUZZ_CAP_CENSUS_OUT").unwrap_or_else(|_| format!("fuzz_cap_census_{pool}.json"));
        std::fs::write(&path, &s).unwrap_or_else(|e| panic!("FUZZ_CAP_CENSUS_OUT {path}: {e}"));
        Some(s)
    }
}

/// The spec of drafted match `seed` (`FUZZ_POOL=drafted`): a 4v4 or 2v2 event
/// (random, or team A as `consensus`) and one of its matches.
fn drafted_spec(seed: u64) -> crate::az::MtgSpec {
    use crate::az::draft::{DraftSpec, TeamDraftAz};
    use crate::az::draft_bots::{match_spec, Consensus};
    use zero_search::game::{Bot, SearchGame};
    let players = if seed % 2 == 0 { 8 } else { 4 };
    let consensus = seed % 4 >= 2;
    let mut ev = TeamDraftAz::new(&DraftSpec::default(), players, seed ^ 0xD2AF_7ED0).unwrap();
    let mut rng = Rng::new(seed ^ 0xD2AF_7ED1);
    let mut bot = Consensus::default();
    let (mut legal, mut keys) = (Vec::new(), Vec::new());
    while let Some(p) = ev.to_move() {
        ev.legal_actions(&mut legal);
        let a = if consensus && p % 2 == 0 { bot.choose(&ev, &legal) } else { legal[rng.below(legal.len() as u32) as usize] };
        ev.step(a, &mut keys).unwrap();
    }
    let ms = ev.matches().unwrap();
    let setup = &ms[(seed / 4) as usize % ms.len()];
    let turns = if seed % 8 == 7 { 12 } else { 60 };
    crate::az::MtgSpec::parse(&match_spec(setup, turns, crate::az::Format::Bo3)).unwrap()
}

/// The groups' cards as ids of `db`, and `db`'s supported lands.
/// `FUZZ_GROUPS=n`: the first `n` groups only, so a failure recorded before
/// a group was added deals its decks again (the farm's interactions seeds
/// at 075649c: `FUZZ_GROUPS=13`; ENGINE.md 51).
fn interaction_pool(db: &CardDb) -> (Vec<Vec<u16>>, Vec<u16>) {
    let n: usize = super::env::var("FUZZ_GROUPS").ok().map(|s| s.parse().expect("FUZZ_GROUPS: a number of groups")).unwrap_or(GROUPS.len());
    assert!((1..=GROUPS.len()).contains(&n), "FUZZ_GROUPS: 1 to {}", GROUPS.len());
    let groups = GROUPS[..n]
        .iter()
        .map(|(_, names)| names.iter().map(|n| db.expect_id(n).unwrap_or_else(|e| panic!("FUZZ_POOL group card: {e}"))).collect())
        .collect();
    let lands = db.supported().map(|n| db.id(n).unwrap()).filter(|&id| db.def(id).is(crate::db::types::LAND)).collect();
    (groups, lands)
}

/// A combo-dense deck (40 cards: 17 random lands; 23 spells, each from two
/// or three interaction groups, or one time in eight from all of them) and
/// a sideboard of seven from the same groups.
fn interaction_deck(db: &CardDb, rng: &mut Rng) -> (Vec<u16>, Vec<u16>) {
    let (groups, lands) = interaction_pool(db);
    let k = 2 + rng.below(2) as usize;
    let mut chosen: Vec<usize> = Vec::new();
    while chosen.len() < k {
        let i = rng.below(groups.len() as u32) as usize;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    let cards: Vec<u16> = chosen.iter().flat_map(|&i| groups[i].iter().copied()).collect();
    let all: Vec<u16> = groups.iter().flatten().copied().collect();
    let mut deck: Vec<u16> = (0..17).map(|_| lands[rng.below(lands.len() as u32) as usize]).collect();
    for _ in 0..23 {
        let from = if rng.below(8) == 0 { &all } else { &cards };
        deck.push(from[rng.below(from.len() as u32) as usize]);
    }
    let side = (0..7).map(|_| cards[rng.below(cards.len() as u32) as usize]).collect();
    (deck, side)
}

/// Decisions asked, by kind (printed: what the interaction mode changes).
static KINDS: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];

fn count_kind(k: Option<DecisionKind>) {
    if let Some(k) = k {
        KINDS[(k as usize).min(63)].fetch_add(1, Ordering::Relaxed);
    }
}

fn kinds_report() -> String {
    let total: u64 = KINDS.iter().map(|k| k.load(Ordering::Relaxed)).sum();
    let names = [
        DecisionKind::NewTarget,
        DecisionKind::DivideDamage,
        DecisionKind::ChooseExiled,
        DecisionKind::ChoosePermanent,
        DecisionKind::ChooseGraveyardCard,
        DecisionKind::ChooseAltCost,
        DecisionKind::ChooseMode,
        DecisionKind::OrderTriggers,
        DecisionKind::YesNo,
        DecisionKind::Sacrifice,
        DecisionKind::Target,
    ];
    let per = |k: DecisionKind| KINDS[(k as usize).min(63)].load(Ordering::Relaxed) as f64 * 1000.0 / total.max(1) as f64;
    names.iter().map(|&k| format!("{k:?} {:.2}", per(k))).collect::<Vec<_>>().join(", ") + " (per 1,000 decisions)"
}

/// One game played (`play`, `play_hidden`).
pub struct Played {
    pub actions: Vec<u16>,
    pub keys: Vec<u64>,
    pub fingerprint: u64,
    pub outcome: Outcome,
    pub turns: u32,
    /// Companions revealed, and put into a hand (ENGINE.md 35).
    pub companions: u32,
    pub companions_used: u32,
}

/// The decks and sideboards of game `seed` (ENGINE.md 35): one game in four
/// has the self-play generator's decks (two colours, singleton, a sideboard,
/// often a companion, a deck built for it); the others random decks with a
/// sideboard of seven random cards.
fn decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    let db = super::cards::db();
    let mut rng = Rng::new(seed ^ 0xDEC5);
    let mut side = Rng::new(seed ^ 0x51DE);
    if interactions_mode() {
        return [interaction_deck(db, &mut rng), interaction_deck(db, &mut rng)];
    }
    if exponential_mode() {
        return [exponential_deck(db, &mut rng), exponential_deck(db, &mut rng)];
    }
    if delver_mode() {
        return [(delver_deck(), Vec::new()), (delver_deck(), Vec::new())];
    }
    if dd_a_mode() {
        // Doomsday against Delver in either seat; one game in three the
        // Doomsday mirror.
        return match seed % 3 {
            0 => [(dd_a_deck(), Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (dd_a_deck(), Vec::new())],
            _ => [(dd_a_deck(), Vec::new()), (dd_a_deck(), Vec::new())],
        };
    }
    if dd_b_mode() {
        let (dd, delver) = ((dd_b_deck(), Vec::new()), (delver_deck(), Vec::new()));
        return if seed % 2 == 0 { [dd, delver] } else { [delver, dd] };
    }
    if ddc_mode() {
        return ddc_decks(seed);
    }
    if doomsday_mode() {
        return doomsday_decks(seed);
    }
    if sns_mode() {
        // Against Delver in either seat; one game in three the mirror.
        let mine = if pool_mode() == PoolMode::Sns { sns_deck() } else { sneak_show_deck() };
        return match seed % 3 {
            0 => [(mine, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (mine, Vec::new())],
            _ => [(mine.clone(), Vec::new()), (mine, Vec::new())],
        };
    }
    if hogaak_a_mode() {
        // Against Delver in either seat; one game in three the mirror.
        let mine = hogaak_a_deck();
        return match seed % 3 {
            0 => [(mine, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (mine, Vec::new())],
            _ => [(mine.clone(), Vec::new()), (mine, Vec::new())],
        };
    }
    if hogaak_b_mode() {
        // Against Delver in either seat; one game in three the mirror.
        let mine = hogaak_b_deck();
        return match seed % 3 {
            0 => [(mine, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (mine, Vec::new())],
            _ => [(mine.clone(), Vec::new()), (mine, Vec::new())],
        };
    }
    if names_mode() {
        return [(names_deck(&mut rng), Vec::new()), (names_deck(&mut rng), Vec::new())];
    }
    if meta_mode() {
        // (ENGINE.md 176) The best-of-one metagame's game of this seed: its pairing's mains.
        let sp = crate::az::MtgSpec::parse(&meta_json(false, false, serde_json::json!({}))).unwrap();
        return sp.pools(seed);
    }
    if wheel_mode() {
        // (ENGINE.md 180) The wheel spec's game of this seed: its pairing's mains (a Doomsday deck in every game).
        let sp = crate::az::MtgSpec::parse(&wheel_json(seed)).unwrap();
        return sp.pools(seed);
    }
    if cross_mode() {
        // (ENGINE.md 148) The Doomsday 60 against the Sneak and Show 60, seats by seed.
        let (dd, sns) = ((doomsday_deck(), Vec::new()), (sneak_show_deck(), Vec::new()));
        return if seed % 2 == 0 { [dd, sns] } else { [sns, dd] };
    }
    if hogaak_mode() {
        return hogaak_decks(seed);
    }
    if lands_b_mode() {
        // Against Delver in either seat; one game in three the mirror.
        let mine = lands_b_deck();
        return match seed % 3 {
            0 => [(mine, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (mine, Vec::new())],
            _ => [(mine.clone(), Vec::new()), (mine, Vec::new())],
        };
    }
    if hogaak_c_mode() {
        // Against Delver in either seat; one game in three the mirror.
        let mine = hogaak_c_deck();
        return match seed % 3 {
            0 => [(mine, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (mine, Vec::new())],
            _ => [(mine.clone(), Vec::new()), (mine, Vec::new())],
        };
    }
    if delver_sb_mode() {
        // Against Delver in either seat, the sideboarded mirror, or Sneak and Show.
        let sb = delver_sb_deck();
        return match seed % 4 {
            0 => [(sb, Vec::new()), (delver_deck(), Vec::new())],
            1 => [(delver_deck(), Vec::new()), (sb, Vec::new())],
            2 => [(sb.clone(), Vec::new()), (sb, Vec::new())],
            _ => [(sb, Vec::new()), (sneak_show_deck(), Vec::new())],
        };
    }
    if eoe_mode() {
        return [(eoe_deck(&mut rng), Vec::new()), (eoe_deck(&mut rng), Vec::new())];
    }
    if sealed_mode() {
        return sealed_decks(seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
    if seed % 4 == 3 {
        let gen = crate::az::decks::DeckGen::default();
        return [
            crate::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
            crate::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
        ];
    }
    let (_, spells) = super::cards::fuzz_pool();
    let sideboard = |side: &mut Rng| -> Vec<u16> { (0..7).map(|_| spells[side.below(spells.len() as u32) as usize]).collect() };
    let d0 = super::cards::random_deck(db, &mut rng);
    let d1 = super::cards::random_deck(db, &mut rng);
    [(d0, sideboard(&mut side)), (d1, sideboard(&mut side))]
}

pub fn new_game(seed: u64) -> Game {
    let [(d0, s0), (d1, s1)] = decks(seed);
    // `FUZZ_LOG=1` keeps the game log, so a panic message shows the game.
    let log = super::env::var("FUZZ_LOG").is_ok();
    let starting_life = if exponential_mode() { EXP_LIFE } else { GameSpec::default().starting_life };
    // (`FUZZ_POOL=meta_wheel`, ENGINE.md 180: the game plays with the seed's forced find, every check of the default mode)
    let forced_find = wheel_mode().then(|| crate::az::MtgSpec::parse(&wheel_json(seed)).unwrap().forced_find).flatten();
    Game::with_sideboards(game_db(), GameSpec { log, starting_life, forced_find, ..GameSpec::default() }, [&d0, &d1], [&s0, &s1], seed).unwrap()
}

/// Play one game with random actions; run the extra checks along the way.
pub fn play(seed: u64, checks: bool) -> Played {
    let mut g = new_game(seed);
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
    // (The completion check's sampling: its own generator, so the game's
    // actions are the same with or without it.)
    let mut crng = Rng::new(seed ^ 0xC0_4D_1E7E);
    // (`FUZZ_CHANCE`'s sampling: its own generator too.)
    let mut chrng = Rng::new(seed ^ 0xC4A_5CE5);
    let chance = chance_every();
    let mut legal = Vec::new();
    let mut actions = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0;
    let exp = exponential_mode();
    let mut stack_seen: Vec<u32> = Vec::new();
    // (`FUZZ_POOL=dd_b`: lane B's counts, ENGINE.md 124.2.)
    let mut dd_b_seen = Vec::new();
    let mut ddc_seen = DdcSeen::default();
    // (`FUZZ_POOL=doomsday`'s counts, ENGINE.md 124.4.)
    let mut doomsday_seen = DoomsdaySeen::default();
    // (`FUZZ_POOL=sneak_show` / `sns`: the Flutes that chose a name, ENGINE.md 123.)
    let mut sns_named: Vec<crate::ids::ObjRef> = Vec::new();
    // (`FUZZ_POOL=hogaak_a`: the Hogaaks counted, ENGINE.md 220.1.)
    let mut hogaak_seen: Vec<u32> = Vec::new();
    // (`FUZZ_POOL=hogaak_b`: the Mockingbirds counted, ENGINE.md 220.2.)
    let mut hogaak_b_seen: Vec<crate::ids::ObjRef> = Vec::new();
    // (`FUZZ_POOL=names`: the namers counted and the stack items seen, ENGINE.md 127.)
    let (mut names_named, mut names_seen): (Vec<crate::ids::ObjRef>, Vec<u32>) = (Vec::new(), Vec::new());
    // (`FUZZ_POOL=hogaak_c`'s counts, ENGINE.md 220.3.)
    let mut hogaak_c_seen = HogaakCSeen::default();
    // (`FUZZ_POOL=hogaak`'s Stitcher's Supplier triggers counted, ENGINE.md 220.4.)
    let mut hogaak_stitchers: Vec<u32> = Vec::new();
    // (`FUZZ_POOL=lands_b`'s counts, ENGINE.md 270.2.)
    let mut lands_b_seen = LandsBSeen::default();
    let (mut turn, mut turn_steps) = (g.turn().number, 0u64);
    // The rules checks (ENGINE.md 150), after every step.
    let mut rc = checks.then(|| RulesCheck::new(&g));
    if let Some(rc) = rc.as_mut() {
        rules_step(rc, &g, &|| format!("seed {seed} (its first state)"));
    }
    while !g.is_over() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        // (Exponential boards: attackers are declared one time in four, so
        // the boards grow through the 60 turns.)
        let a = if exp && g.kind() == Some(DecisionKind::DeclareAttackers) && rng.below(4) != 0 && legal.contains(&crate::act::PASS) {
            crate::act::PASS
        } else if let Some(next) = names_next_page(&g, &mut rng) {
            next
        } else {
            legal[rng.below(legal.len() as u32) as usize]
        };
        if checks {
            count_kind(g.kind());
            // The decider's encoding is finite and bounded (ENGINE.md 46);
            // a graveyard card's option points at its list token (ENGINE.md
            // 65.10).
            // (Layout v7, ENGINE.md 125: the whole row, every option's pointer.)
            let mover = g.to_move().unwrap();
            let row = crate::az::tail_encode::Row::of_v8(&g, mover);
            check_row(&g, mover, &row, &|| format!("seed {seed} step {steps}"));
            if census::on() {
                let other = crate::az::tail_encode::Row::of_v8(&g, 1 - mover);
                census::record(&g, 1 - mover, &other);
            }
            check_graveyard_list(&g, &row, &|| format!("seed {seed} step {steps}"));
            if g.pending_decision().is_some_and(|d| g.to_move() != Some(d.player)) {
                CONTROLLED.fetch_add(1, Ordering::Relaxed);
            }
            record_extremes(&g);
        }
        if checks && rng.below(checkpoint_every()) == 0 {
            check_clone(&g, a, seed);
            check_determinize(&g, seed, &mut rng);
            if let Some(t) = belief_tilt() {
                check_determinize_weighted(&g, seed, steps, t);
            }
        }
        if checks && complete_now(&g, &mut crng) {
            check_options_complete(&g, seed, "the game");
        }
        if let Some(n) = chance.filter(|_| checks) {
            if chrng.below(n) == 0 {
                check_forced_chance(&g, seed, steps);
            }
        }
        let massacre_top = if hogaak_b_mode() || hogaak_mode() { hogaak_b_massacre_on_top(&g, a) } else { None };
        let stitcher_top = if hogaak_mode() { hogaak_stitcher_on_top(&g, a) } else { None };
        if hogaak_c_mode() || hogaak_mode() {
            count_hogaak_c_choice(&g, a);
        }
        if lands_b_mode() {
            count_lands_b_choice(&g, a);
        }
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        count_dropped_loop_records(&g, false);
        g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}
{}", g.log_lines().iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("
")));
        if let Some(rc) = rc.as_mut() {
            rules_step(rc, &g, &|| format!("seed {seed} step {steps} (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1)"));
        }
        actions.push(a);
        steps += 1;
        if g.turn().number != turn {
            record_turn(seed, turn, turn_steps);
            (turn, turn_steps) = (g.turn().number, 0);
        }
        turn_steps += 1;
        if turn_steps == LONG_TURN && super::env::var("FUZZ_LOG").is_ok() {
            // (A long turn's log, to tell what it repeats.)
            let log = g.log_lines();
            println!("seed {seed} turn {turn}: {LONG_TURN} decisions; its log's last lines:\n{}", log[log.len().saturating_sub(80)..].join("\n"));
        }
        if delver_mode() && !g.stack().is_empty() {
            count_delver(&g, &mut stack_seen);
        }
        if dd_a_mode() && !g.stack().is_empty() {
            count_dd_a(&g, &mut stack_seen);
        }
        if dd_b_mode() {
            count_dd_b(&g, &mut dd_b_seen);
        }
        if ddc_mode() {
            count_ddc(&g, &mut stack_seen, &mut ddc_seen);
        }
        if doomsday_mode() {
            count_doomsday(&g, &mut doomsday_seen);
        }
        if sns_mode() {
            if !g.stack().is_empty() {
                count_sns(&g, &mut stack_seen);
            }
            count_sns_names(&g, &mut sns_named);
        }
        if (hogaak_a_mode() || hogaak_mode()) && !g.stack().is_empty() {
            count_hogaak_a(&g, &mut hogaak_seen);
        }
        if hogaak_b_mode() || hogaak_mode() {
            count_hogaak_b(&g, massacre_top, &mut stack_seen, &mut hogaak_b_seen);
        }
        if hogaak_mode() {
            count_hogaak_stitcher(&g, stitcher_top, &mut hogaak_stitchers);
        }
        if names_mode() {
            count_names(&g, &mut names_named, &mut names_seen);
        }
        if delver_sb_mode() && !g.stack().is_empty() {
            count_delver_sb(&g, &mut stack_seen);
        }
        if hogaak_c_mode() || hogaak_mode() {
            count_hogaak_c(&g, &mut hogaak_c_seen);
        }
        if lands_b_mode() {
            count_lands_b(&g, &mut lands_b_seen);
        }
        if cross_mode() && !g.stack().is_empty() {
            count_cross(&g, &mut stack_seen);
        }
        assert!(steps < 100_000, "seed {seed}: game does not end
{}", g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
"));
    }
    MAX_GAME_STEPS.fetch_max(steps as u64, Ordering::Relaxed);
    record_turn(seed, turn, turn_steps);
    if let Some(rc) = &rc {
        rules_add(&rc.stats);
    }
    let companions = (0..2).filter(|&p| g.player(p).companion.is_some()).count() as u32;
    let companions_used = (0..2).filter(|&p| g.player(p).companion_used).count() as u32;
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number, companions, companions_used }
}

/// A clone continues exactly like the original.
fn check_clone(g: &Game, a: u16, seed: u64) {
    let mut x = g.clone();
    let mut y = g.clone();
    let (mut kx, mut ky) = (Vec::new(), Vec::new());
    x.apply(a, &mut kx).unwrap();
    y.apply(a, &mut ky).unwrap();
    assert_eq!(x.fingerprint(), y.fingerprint(), "seed {seed}: clones diverged");
    assert_eq!(kx, ky);
}

/// `determinize(to_move)` keeps the decider's legal actions and the public
/// state; the resampled world keeps playing without breaking invariants.
fn check_determinize(g: &Game, seed: u64, rng: &mut Rng) {
    let mover = g.to_move().unwrap();
    // The decider (what the search does) and the other player (tests and
    // analysis: its determinize refreshes the decider's decision).
    for viewer in [mover, 1 - mover] {
        let mut d = g.clone();
        d.determinize(viewer, rng);
        if viewer == mover {
            let (mut l1, mut l2) = (Vec::new(), Vec::new());
            g.legal_actions(&mut l1);
            d.legal_actions(&mut l2);
            assert_eq!(l1, l2, "seed {seed}: determinize changed the decider's actions");
        } else {
            assert_eq!(d.to_move(), Some(mover), "seed {seed}: a refresh ran past the decision");
        }
        // Nothing the viewer can observe changes; no card it knows exactly
        // is resampled.
        assert_eq!(
            g.view_of(viewer),
            d.view_of(viewer),
            "seed {seed}: determinize changed the view of P{viewer} (mover P{mover})
{}",
            g.log_lines().iter().rev().take(20).cloned().collect::<Vec<_>>().join("
")
        );
        for (i, o) in g.objs().iter().enumerate() {
            let c = crate::CardIdx::new(i as u32);
            if g.knows(viewer, c) {
                assert_eq!(o.def, d.obj(c).def, "seed {seed}: a card P{viewer} knows was resampled");
                assert_eq!(g.ident(c), d.ident(c), "seed {seed}: a face-down card P{viewer} knows was resampled");
            }
        }
        d.check_invariants().unwrap();
        // Both players' encodings of the world (and of the game) are bounded.
        let mut buf = Vec::new();
        for v in [0u8, 1] {
            check_encoding(&d, v, &mut buf, &|| format!("seed {seed}: a world determinized for P{viewer}"));
            check_encoding(g, v, &mut buf, &|| format!("seed {seed}: a checkpoint"));
        }
        // What the search does in that world: try every option.
        check_options_complete(&d, seed, &format!("a world determinized for P{viewer} (mover P{mover})"));
        // The information-set walk for both viewers (ENGINE.md 148: the decider's only before); the other
        // viewer's from a generator of its own, so the game's actions are the same as before.
        if viewer == mover {
            check_information_set(g, viewer, seed, rng);
        } else {
            check_information_set(g, viewer, seed, &mut Rng::new(seed ^ (g.decisions() as u64).wrapping_mul(0x9E37_79B9) ^ 0x15E7_0BE5));
        }
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        // (The world's own rules checks, ENGINE.md 150: its cards are the world's.)
        let mut rc = RulesCheck::new(&d);
        rules_step(&mut rc, &d, &|| format!("seed {seed}: a world determinized for P{viewer}"));
        for _ in 0..30 {
            if d.is_over() {
                break;
            }
            d.legal_actions(&mut legal);
            keys.clear();
            // (the viewer's hand before the step: a look of the step at it may see a card that leaves it later in the
            // step, ENGINE.md 175)
            let before = game_viewer_hand(&d, viewer);
            d.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
            PK_GAME_KEYS.fetch_add(
                check_private_keys_masked_since(&keys, viewer, &d, &before, &|| format!("seed {seed}: a world determinized for P{viewer}")),
                Ordering::Relaxed,
            );
            d.check_invariants().unwrap_or_else(|e| {
                panic!(
                    "seed {seed} (determinized for P{viewer}): {e}\n{}",
                    d.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
                )
            });
            rules_step(&mut rc, &d, &|| format!("seed {seed}: a world determinized for P{viewer}, played on"));
        }
        rules_add(&rc.stats);
    }
}

/// Decisions followed after an option before giving up on reaching priority.
const COMPLETE_STEPS: usize = 64;

/// Options the completion check tried, and the decisions it answered after
/// them (reported by `fuzz_random_games`).
static OPTIONS_TRIED: AtomicU64 = AtomicU64::new(0);
static OPTION_STEPS: AtomicU64 = AtomicU64::new(0);

/// Whether the completion check runs at this decision of the game: not at
/// mulligans and combat declarations (no cast or activation starts there), at
/// half of the frequent priority, payment and target decisions (a castable
/// card stays castable over many of them), at every other one (the rare
/// ones: an effect's "cast it?", a cost option, an X, a sacrifice, ...).
fn complete_now(g: &Game, rng: &mut Rng) -> bool {
    use DecisionKind as K;
    match g.kind() {
        None | Some(K::Mulligan | K::MulliganBottom | K::DeclareAttackers | K::DeclareBlockers | K::AssignDamage | K::AttackTarget) => {
            false
        }
        Some(K::Priority | K::PayMana | K::Target) => rng.below(2) == 0,
        Some(_) => true,
    }
}

/// Every option of the pending decision can be completed (ENGINE.md 6, 18: a
/// cast, an activation or an alternative cost is offered only if it can be
/// completed, so nothing is ever rewound): each legal action, applied to a
/// clone, then the decisions after it answered along one legal path (random,
/// from a generator of its own: the game's sequence is unchanged) until the
/// next priority decision, never panics or fails. The payment's own
/// decisions (targets, X, the cost option, mana sources, sacrifices,
/// discards) all come before the caster's next priority (CR 601.2i, 117.3c).
/// At priority, passing and playing a land are left out (no cost to
/// complete).
fn check_options_complete(g: &Game, seed: u64, world: &str) {
    let Some(d) = g.pending_decision() else { return };
    if d.options.len() < 2 {
        return; // (the game itself takes the one option)
    }
    let priority = d.kind == DecisionKind::Priority;
    for o in &d.options {
        if priority && matches!(o.choice, Choice::Pass | Choice::Play(_) | Choice::PlayFace(_)) {
            continue;
        }
        let a = o.action;
        let path_seed = seed ^ (g.decisions() as u64).wrapping_mul(0x9E37_79B9) ^ ((a as u64) << 40);
        // The option, then one random legal path to the next priority;
        // `describe` keeps the path's descriptions (for a failure).
        let follow = |describe: &mut Option<Vec<String>>| -> Result<(), String> {
            let mut prng = Rng::new(path_seed);
            let mut x = g.clone();
            let (mut keys, mut l) = (Vec::new(), Vec::new());
            x.apply(a, &mut keys)?;
            for _ in 0..COMPLETE_STEPS {
                if x.is_over() || x.kind() == Some(DecisionKind::Priority) {
                    break;
                }
                x.legal_actions(&mut l);
                let b = l[prng.below(l.len() as u32) as usize];
                if let Some(p) = describe {
                    p.push(format!("{b} = {}", x.describe_action(b).unwrap_or_default()));
                }
                OPTION_STEPS.fetch_add(1, Ordering::Relaxed);
                x.apply(b, &mut keys)?;
            }
            Ok(())
        };
        OPTIONS_TRIED.fetch_add(1, Ordering::Relaxed);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut None)));
        let err = match r {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(p) => p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
        };
        // The same path again, described.
        let mut path = Some(Vec::new());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut path)));
        panic!(
            "seed {seed}: in {world}, option {a} ({}) of {:?} for P{} can't be completed: {err}\npath after it: {:?}\noptions: {:?}\n{}",
            g.describe_action(a).unwrap_or_default(),
            d.kind,
            d.player,
            path.unwrap_or_default(),
            g.options_desc(),
            g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

/// The open-decklist information-set walks (`check_information_set`, ENGINE.md 148): walks begun, and
/// the states compared after at least one common action (an equal view and an equal whole row of the
/// viewer). The games' run prints both and, with 200 games or more, needs compared states: a walk that
/// ended at its first action every time would pass silently otherwise.
static IS_WALKS: AtomicU64 = AtomicU64::new(0);
/// The near-tie merge key's dropped loop records (ENGINE.md 233, `Game::check_dropped_loop_records`): records checked
/// at every step of every game and match, and the times one's loop hash came back (none allowed).
static LOOPDROP_CHECKED: AtomicU64 = AtomicU64::new(0);
static LOOPDROP_RECUR: AtomicU64 = AtomicU64::new(0);
static MATCH_LOOPDROP_CHECKED: AtomicU64 = AtomicU64::new(0);
static MATCH_LOOPDROP_RECUR: AtomicU64 = AtomicU64::new(0);

/// Count `g`'s dropped loop records (ENGINE.md 233) after a step.
fn count_dropped_loop_records(g: &Game, matches: bool) {
    let (n, bad) = g.check_dropped_loop_records();
    if n > 0 {
        let (c, r) = if matches { (&MATCH_LOOPDROP_CHECKED, &MATCH_LOOPDROP_RECUR) } else { (&LOOPDROP_CHECKED, &LOOPDROP_RECUR) };
        c.fetch_add(n, Ordering::Relaxed);
        r.fetch_add(bad, Ordering::Relaxed);
    }
}

/// The end-of-run check of the dropped loop records (ENGINE.md 233): none came back.
fn check_dropped_loops(kind: &str, checked: u64, recur: u64) {
    println!("{kind}: loop records the merge key drops (their source sacrificed): {checked} checked after steps, {recur} recurrences");
    assert_eq!(recur, 0, "{kind}: a dropped loop record's hash came back {recur} times in {checked} checks (ENGINE.md 233)");
}
static IS_STEPS: AtomicU64 = AtomicU64::new(0);
/// ENGINE.md 160, 161: two worlds of `viewer`'s whose keys of one step differ only by the other player's forced answers
/// (`KEY_AUTO`: that player had one option, which may read its hidden cards) while everything else zero-core's child
/// signature reads is equal and the viewer sees them the same (neither over, the same next decider and legal actions,
/// equal views): a key alone told apart worlds the viewer cannot tell apart - the search would split its nodes by the
/// other player's hidden cards. A forced answer that shows (a pass of priority, a permanent chosen) leaves the views
/// different. 160 failed this for `ActivateMana`'s keys and counted the other kinds (default 20k: `ExileCost` 26,
/// `PayMana` 38, `ChooseHandCard` 4; a hand slot or a hand card's ability by its slot); 161 masks such places in the
/// observer's worlds (`Game::forced_answer_seen`) and fails every kind. Next legal actions that differ split the worlds
/// whatever the keys (another player's options may read its hidden cards, as the walk's own check allows): e.g. default
/// seed 9273, a payment's one source, a land, forced in one world while the other offers that land and a Spirit Guide
/// in hand - no key can merge those (161.2). The description, when so.
fn forced_answer_split(ka: &[u64], kb: &[u64], a: &Game, b: &Game, viewer: u8) -> Option<String> {
    use crate::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_AUTO};
    let other_auto = |k: u64| key_kind(k) == KEY_AUTO && key_player(k) != viewer;
    let strip = |k: &[u64]| k.iter().copied().filter(|&x| !other_auto(x)).collect::<Vec<_>>();
    let legal = |g: &Game| {
        let mut v = Vec::new();
        g.legal_actions(&mut v);
        v
    };
    if ka == kb || strip(ka) != strip(kb) || a.is_over() || b.is_over() || a.to_move() != b.to_move() || legal(a) != legal(b) || a.view_of(viewer) != b.view_of(viewer) {
        return None;
    }
    let desc = |k: &[u64]| {
        k.iter()
            .copied()
            .filter(|&x| other_auto(x))
            .map(|x| match key_payload(x) {
                HIDDEN => "HIDDEN".to_string(),
                p => format!("{} kind {} action {}", key_player(x), p >> 16, p & 0xFFFF),
            })
            .collect::<Vec<_>>()
    };
    Some(format!(
        "keyed apart only by the other player's forced answers ({:?} / {:?}), with nothing the viewer P{viewer} sees between the worlds",
        desc(ka),
        desc(kb)
    ))
}

/// The first difference between two layout v7 rows (head, tail tokens, pointer overrides), bit for
/// bit; None when they are equal.
fn row_difference(x: &crate::az::tail_encode::Row, y: &crate::az::tail_encode::Row) -> Option<String> {
    if let Some(i) = (0..x.head.len()).find(|&i| x.head[i].to_bits() != y.head[i].to_bits()) {
        return Some(format!("head float {i}: {} / {}", x.head[i], y.head[i]));
    }
    let bits = |t: &[(u16, u32, Vec<f32>)]| -> Vec<(u16, u32, Vec<u32>)> { t.iter().map(|(g, s, v)| (*g, *s, v.iter().map(|f| f.to_bits()).collect())).collect() };
    let (bx, by) = (bits(&x.tail), bits(&y.tail));
    if bx != by {
        // (the first token that differs: its segment and slot in each row, and its first differing float)
        let k = (0..bx.len().min(by.len())).find(|&k| bx[k] != by[k]).unwrap_or(bx.len().min(by.len()));
        let name = |t: Option<&(u16, u32, Vec<u32>)>| {
            t.map_or("-".to_string(), |(g, s, _)| {
                let seg = crate::az::tail_encode::TAILS.iter().find(|ts| ts.seg == *g).map_or("?", |ts| ts.name);
                format!("{seg} slot {s}")
            })
        };
        let f = match (bx.get(k), by.get(k)) {
            (Some(a), Some(b)) if a.0 == b.0 && a.1 == b.1 => {
                let j = (0..a.2.len()).find(|&j| a.2[j] != b.2[j]).unwrap_or(0);
                format!(", float {j}: {} / {}", f32::from_bits(a.2[j]), f32::from_bits(b.2[j]))
            }
            _ => String::new(),
        };
        return Some(format!("tail tokens: {} / {}; the first differing, token {k}: {} / {}{f}", bx.len(), by.len(), name(bx.get(k)), name(by.get(k))));
    }
    if x.ov != y.ov {
        return Some(format!("pointer overrides: {:?} / {:?}", x.ov, y.ov));
    }
    None
}

/// Every revealed fact is keyed: two worlds `determinize(viewer)` samples,
/// played with the same actions, stay indistinguishable to the viewer as
/// long as the search would keep them in one node: zero-core keys children
/// by (chance keys, next decider, next legal actions), so equal keys, an
/// equal next decider and equal legal actions must mean an equal view and an
/// equal whole row of the viewer (layout v7: head, tail tokens, pointer
/// overrides; ENGINE.md 148 - the view alone was compared before, and 128's
/// leak showed in the encoding only). The compared states are counted
/// (`IS_STEPS`).
fn check_information_set(g: &Game, viewer: u8, seed: u64, rng: &mut Rng) {
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    IS_WALKS.fetch_add(1, Ordering::Relaxed);
    // (A failure's message: the walk's actions as each world describes them, both worlds' stack references and logs.)
    let mut path: Vec<String> = Vec::new();
    let tell = |a: &Game, b: &Game, path: &[String]| {
        let log = |x: &Game| x.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("\n");
        format!(
            "the checkpoint's decider P{}, the walk's viewer P{viewer}; decisions {:?} / {:?}\nthe walk's actions: {path:?}\nstack references: {} / {}\n{}\n----\n{}",
            g.to_move().map_or(9, |p| p),
            a.pending_decision(),
            b.pending_decision(),
            stack_refs(a, viewer),
            stack_refs(b, viewer),
            log(a),
            log(b)
        )
    };
    for step in 0..25 {
        // Equal keys: the game is over in both worlds or in neither (its end is public; ENGINE.md
        // 148 - one world over used to end the walk silently, as a differing decider).
        if a.is_over() != b.is_over() {
            panic!("seed {seed} step {step}: same keys, the game over in one world only ({:?} / {:?}); {}", a.outcome(), b.outcome(), tell(&a, &b, &path));
        }
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            // Another player's options may depend on its hidden cards (and a
            // decision with one option is answered internally, so who
            // decides next may differ too): the search keeps such worlds in
            // different nodes. The viewer's own options never differ.
            assert!(
                !(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)),
                "seed {seed} step {step}: the viewer's options differ: {:?} / {:?}",
                a.options_desc(),
                b.options_desc()
            );
            return;
        }
        let (va, vb) = (a.view_of(viewer), b.view_of(viewer));
        if va != vb {
            let i = (0..va.len().min(vb.len())).find(|&i| va[i] != vb[i]).unwrap_or(va.len().min(vb.len()));
            panic!(
                "seed {seed} step {step}: same keys, different views (first at {i} of {} / {}: {:?} / {:?}); {}",
                va.len(),
                vb.len(),
                va.get(i),
                vb.get(i),
                tell(&a, &b, &path)
            );
        }
        if a.is_over() {
            assert_eq!(a.outcome(), b.outcome(), "seed {seed} step {step}: same keys, different outcomes");
            return;
        }
        let (ra, rb) = (crate::az::tail_encode::Row::of_v8(&a, viewer), crate::az::tail_encode::Row::of_v8(&b, viewer));
        if let Some(d) = row_difference(&ra, &rb) {
            panic!("seed {seed} step {step}: same keys, different rows of P{viewer} ({d}); {}", tell(&a, &b, &path));
        }
        if step > 0 {
            IS_STEPS.fetch_add(1, Ordering::Relaxed);
        }
        let act = la[rng.below(la.len() as u32) as usize];
        path.push(format!("{act}: {} / {}", a.describe_action(act).unwrap_or_default(), b.describe_action(act).unwrap_or_default()));
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.apply(act, &mut ka).unwrap();
        b.apply(act, &mut kb).unwrap();
        if ka != kb {
            // Keys that tell the worlds apart only by the other player's answers the game made itself (`KEY_AUTO`:
            // that player had a single option, which may read its hidden cards) must mark something the viewer sees
            // (ENGINE.md 160, 161): no forced answer may split worlds the viewer sees the same.
            if let Some(why) = forced_answer_split(&ka, &kb, &a, &b, viewer) {
                panic!("seed {seed} step {step}: {why}; {}", tell(&a, &b, &path));
            }
            return; // different outcomes, told apart by the keys
        }
    }
}

/// The stack's items with the objects they refer to (an ability's source, the event's objects): each
/// reference's slot and incarnation, whether it is live, the slot's zone and card, and whether `viewer`
/// knows the card (a failure's message, ENGINE.md 148).
fn stack_refs(g: &Game, viewer: u8) -> String {
    let r = |o: crate::ids::ObjRef| {
        let ob = g.obj(o.idx);
        format!("#{}.{} live {} {:?} {} known {}", o.idx.i(), o.inc, g.live(o), ob.zone, g.db().def(ob.def).name, g.knows(viewer, o.idx))
    };
    g.stack()
        .iter()
        .map(|s| {
            let what = match s.kind {
                crate::game::StackKind::Ability { source, .. } => format!("ability of {}", r(source)),
                crate::game::StackKind::Spell { card } => format!("spell {}", g.name(card)),
            };
            format!("[{what}; event objects {} / {}]", s.ev.obj.map_or("-".to_string(), r), s.ev.obj2.map_or("-".to_string(), r))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `fuzz_random_games` (tests/fuzz.rs; the farm's `*-games` modes): seeds `first..first + n` of the mode
/// the `FUZZ_*` settings choose (module docs), every check above, then the chunk's report and its checks
/// over the whole chunk. A seed whose game panics ends the run there ([`Run::Failed`]: the report is
/// not printed, as when the test stopped at it); `hooks` sees every seed (and may stop the run before
/// one).
pub fn random_games(first: u64, n: u64, hooks: &mut dyn Hooks) -> Run {
    let (mut wins, mut draws, mut aborted, mut turns, mut decisions) = ([0u32; 2], 0u32, 0u32, 0u64, 0u64);
    let (mut companions, mut companions_used) = (0u32, 0u32);
    let mut skips = 0u64;
    let mut unplayable: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let (mut meta_cells, mut meta_steps, mut meta_worlds) = ([0u64; 9], 0u64, 0u64);
    let mut wheel = WheelSeen::default();
    for seed in first..first + n {
        if !hooks.before(seed) {
            return Run::Stopped { at: seed };
        }
        if skipped(seed) {
            skips += 1;
            hooks.passed(seed, &SeedOutcome::not_played("skipped (FUZZ_SKIP)"));
            continue;
        }
        if let Some(e) = sealed_unplayable(seed) {
            // (The error names the card: "... <name> is not supported: <blockers>".)
            let card = e.split(": ").find(|x| x.ends_with(" is not supported")).map_or(e.clone(), |x| x.trim_end_matches(" is not supported").to_string());
            *unplayable.entry(card).or_insert(0) += 1;
            hooks.passed(seed, &SeedOutcome::not_played("unplayable (a card this checkout doesn't support)"));
            continue;
        }
        // (`FUZZ_POOL=meta`, ENGINE.md 176: the game's pairing check first)
        let p = match std::panic::catch_unwind(|| {
            let m = meta_mode().then(|| meta_lockstep(&meta_json(false, false, serde_json::json!({})), seed));
            // (`FUZZ_POOL=meta_wheel`, ENGINE.md 180: the forced find's walk first)
            let w = wheel_mode().then(|| wheel_walk(seed));
            (if hidden_decks_mode() { play_hidden(seed) } else { play(seed, true) }, m, w)
        }) {
            Ok((p, m, w)) => {
                if let Some(([i, j], st, w)) = m {
                    meta_cells[(i * 3 + j) as usize] += 1;
                    meta_steps += st;
                    meta_worlds += w;
                }
                if let Some(w) = w {
                    for k in 0..5 {
                        wheel.classes[k] += w.classes[k];
                    }
                    wheel.worlds += w.worlds;
                    wheel.stepped += w.stepped;
                    wheel.rows += w.rows;
                    wheel.libraries += w.libraries;
                    wheel.value_only += w.value_only;
                    wheel.pile_free += w.pile_free;
                }
                p
            }
            Err(payload) => return Run::Failed { seed, payload },
        };
        // (An aborted game, ENGINE.md 100.7: a legitimate end, with no
        // result and no winner; not a draw.)
        match (p.outcome.reason, p.outcome.winner) {
            (EndReason::Aborted, w) => {
                assert_eq!(w, None, "seed {seed}: an aborted game has no winner");
                aborted += 1;
            }
            (_, Some(w)) => wins[w as usize] += 1,
            (_, None) => draws += 1,
        }
        turns += p.turns as u64;
        decisions += p.actions.len() as u64;
        companions += p.companions;
        companions_used += p.companions_used;
        hooks.passed(seed, &SeedOutcome { fingerprint: p.fingerprint, result: format!("{:?}", p.outcome), turns: p.turns as u64, decisions: p.actions.len() as u64 });
    }
    println!(
        "{n} games: wins {wins:?}, draws {draws}, aborted {aborted}, mean turns {:.1}, mean decisions {:.1}; {} options completed ({} decisions after them)",
        turns as f64 / n as f64,
        decisions as f64 / n as f64,
        OPTIONS_TRIED.load(Ordering::Relaxed),
        OPTION_STEPS.load(Ordering::Relaxed)
    );
    println!("companions revealed {companions}, put into a hand {companions_used}");
    println!("decisions{}: {}", if interactions_mode() { " (FUZZ_POOL=interactions)" } else { "" }, kinds_report());
    println!("numbers{}: {}", if exponential_mode() { " (FUZZ_POOL=exponential)" } else { "" }, extremes_report());
    // Layout v7's rows (ENGINE.md 125): pointers, the v6 projection, the limit; the census.
    println!("{}", rows_report());
    if let Some(c) = census::report(&super::env::var("FUZZ_POOL").unwrap_or_else(|_| "cube".into()), n) {
        println!("FUZZ_CAP_CENSUS:\n{c}");
    }
    println!("longest game: {} actions (cap 100,000)", MAX_GAME_STEPS.load(Ordering::Relaxed));
    println!("{}", rules_report());
    rules_verdict("fuzz_random_games");
    println!("controlled decisions (CR 722.4, encoded in the controlled seat: ENGINE.md 100.5): {}", CONTROLLED.load(Ordering::Relaxed));
    println!("{}; seeds skipped (FUZZ_SKIP): {skips}", longest_turn_report());
    // (ENGINE.md 141; the match fuzz's counts run in parallel with this one: read them there)
    println!("private keys of the other player masked in the game fuzz's worlds: {}; {}", PK_GAME_KEYS.load(Ordering::Relaxed), hand_looks_report());
    if hidden_decks_mode() {
        let (w, x, st, lt) =
            (HD_WORLDS.load(Ordering::Relaxed), HD_OTHER_DECK.load(Ordering::Relaxed), HD_STEPS.load(Ordering::Relaxed), HD_LONG_TURNS.load(Ordering::Relaxed));
        println!("FUZZ_HIDDEN_DECKS: {w} hidden-decklist worlds, {x} with another deck than the real one; {st} information-set steps in step; {lt} turns past {HD_LONG_TURN} decisions");
    }
    if let Some(k) = deck_choice_fuzz().filter(|_| sealed_mode() && hidden_decks_mode()) {
        let (c, w) = (DC_CHOICES.load(Ordering::Relaxed), DC_WORLDS.load(Ordering::Relaxed));
        println!("FUZZ_DECK_CHOICE={k}: {c} deck choices played, {w} worlds checked during them");
        if let Ok(path) = super::env::var("FUZZ_DECK_CHOICE_BY") {
            let (ch, ex) = (DC_EVAL_CHOSEN.load(Ordering::Relaxed), DC_EVAL_EXPLORE.load(Ordering::Relaxed));
            println!("FUZZ_DECK_CHOICE_BY={path}: {ch} seats chose by the evaluator, {ex} explored");
        }
    }
    if let Some(t) = deck_tilt_fuzz() {
        let (w, r) = (DT_WORLDS.load(Ordering::Relaxed), DT_RATIO.load(Ordering::Relaxed));
        let mean = r as f64 / 1e6 / w.max(1) as f64;
        println!("FUZZ_DECK_TILT={t}: {w} deck-tilted worlds from the K cached decks; K x the dealt deck's probability {mean:.3} on average (uniform 1)");
    }
    if let Some(t) = belief_tilt() {
        let (w, d, u) = (BELIEF_WORLDS.load(Ordering::Relaxed), BELIEF_DEALT.load(Ordering::Relaxed), BELIEF_UNIFORM.load(Ordering::Relaxed));
        println!(
            "FUZZ_BELIEF={t}: {w} weighted worlds with unknown hand cards; the favoured identity dealt {:.3} copies per world, the uniform deal's {:.3}",
            d as f64 / 1000.0 / w.max(1) as f64,
            u as f64 / 1000.0 / w.max(1) as f64
        );
    }
    println!(
        "graveyard lists (ENGINE.md 65.10): {} decisions checked, longest {} (cards offered from both graveyards: at most {}; the limit 64)",
        GY_LISTS.load(Ordering::Relaxed),
        GY_LIST_MAX.load(Ordering::Relaxed),
        GY_LIST_BOTH_MAX.load(Ordering::Relaxed)
    );
    // The information-set walks compared states (ENGINE.md 148): open decklists' (`check_information_set`) or
    // hidden decklists' (`check_hidden_information_set`).
    let (isw, iss, hds) = (IS_WALKS.load(Ordering::Relaxed), IS_STEPS.load(Ordering::Relaxed), HD_STEPS.load(Ordering::Relaxed));
    println!("information sets (ENGINE.md 148): {isw} open-decklist walks, {iss} states compared after a common action (the view and the viewer's whole row); {hds} hidden-decklist steps");
    if let Some(c) = chance_every() {
        let (w, s, f, x) = (
            CHANCE_WALKS.load(Ordering::Relaxed),
            CHANCE_STEPS.load(Ordering::Relaxed),
            CHANCE_FORCED.load(Ordering::Relaxed),
            CHANCE_SPLIT.load(Ordering::Relaxed),
        );
        println!("FUZZ_CHANCE={c}: {w} chance walks, {s} steps at one node, {f} replays whose outcome the force changed, {x} walks split by the keys");
    }
    if sealed_mode() {
        let total: u64 = unplayable.values().sum();
        println!("FUZZ_POOL=sealed: {} games played; {total} games skipped, a pool holding a card this checkout doesn't support: {unplayable:?}", n - total - skips);
        // the mixed field (ENGINE.md 107.1): how many seats played the best guess
        if let Ok(v) = super::env::var("FUZZ_BEST_SHARE") {
            let p: f64 = v.parse().expect("FUZZ_BEST_SHARE: a probability");
            let best = (first..first + n).map(|s| (0..2).filter(|&q| crate::az::sealed::best_seat(s, q, p)).count()).sum::<usize>();
            println!("FUZZ_BEST_SHARE={p}: {best} of {} seats the builder's best guess", 2 * n);
        }
    }
    if dd_b_mode() {
        let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let (m, r, f, c, t, k, u) = (
            l(&DD_B_MISDIRECTIONS),
            l(&DD_B_RETARGETS),
            l(&DD_B_FLUSTERSTORMS),
            l(&DD_B_COPIES),
            l(&DD_B_CAVERN_TYPES),
            l(&DD_B_TYPE_MANA),
            l(&DD_B_UNCOUNTERABLE),
        );
        println!(
            "FUZZ_POOL=dd_b: Misdirection cast {m}, its new-target decisions {r}; Flusterstorm cast {f}, copies {c}; Cavern types chosen {t}, decisions with chosen-type mana floating {k}, uncounterable creature spells {u}"
        );
    }
    if delver_mode() {
        let (m, t, c) =
            (DELVER_MURKTIDES.load(Ordering::Relaxed), DELVER_MURK_TRIGGERS.load(Ordering::Relaxed), DELVER_CHARMS.load(Ordering::Relaxed));
        println!("FUZZ_POOL=delver: Murktide Regent cast {m} times, its leave-the-graveyard triggers {t}, Prismari Charm cast {c} times");
    }
    if dd_a_mode() {
        let casts: Vec<u64> = DD_A_CASTS.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let pacts = DD_A_PACT_TRIGGERS.load(Ordering::Relaxed);
        println!(
            "FUZZ_POOL=dd_a: seen on the stack {}; Pact of Negation's triggers {pacts}",
            DD_A_CARDS.iter().zip(&casts).map(|(n, c)| format!("{n} {c} times")).collect::<Vec<_>>().join(", ")
        );
    }
    if ddc_mode() {
        let ld = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let (ec, ey, wc, wy) = (ld(&DDC_EDGE_CAST), ld(&DDC_EDGE_CYCLED), ld(&DDC_WRAITH_CAST), ld(&DDC_WRAITH_CYCLED));
        let (lo, lg, le, sw) = (ld(&DDC_LEYLINE_OPENING), ld(&DDC_LEYLINE_GAMES), ld(&DDC_LEYLINE_EXILE_GAMES), ld(&DDC_SWAMPWALK_ATTACKS));
        println!(
            "FUZZ_POOL=dd_c: Edge of Autumn cast {ec}, cycled {ey}; Street Wraith cast {wc}, cycled {wy}; blockers asked facing a swampwalker and controlling a Swamp {sw} (never offered it); \
             Leyline of the Void on the battlefield as the first turn began in {lo} games, at some time in {lg}, with a card of its controller's opponent in exile then in {le}"
        );
        // (A swampwalker facing a Swamp is rare in random play, a few times in
        // 2,000 games: not asserted; the card tests cover it.)
    }
    if doomsday_mode() {
        let casts: Vec<u64> = DOOMSDAY_CASTS.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let abil: Vec<u64> = DOOMSDAY_ABILITY_COUNTS.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let (r, t, u) =
            (DOOMSDAY_RETARGETS.load(Ordering::Relaxed), DOOMSDAY_CAVERN_TYPES.load(Ordering::Relaxed), DOOMSDAY_UNCOUNTERABLE.load(Ordering::Relaxed));
        println!(
            "FUZZ_POOL=doomsday: seen on the stack {}; abilities: Pact of Negation's trigger {}, Edge of Autumn cycled {}, Street Wraith cycled {}, Murktide Regent's trigger {}; Misdirection's new-target decisions {r}; Caverns' types chosen {t}; uncounterable creature spells {u}",
            DOOMSDAY_SPELLS.iter().zip(&casts).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", "),
            abil[0],
            abil[1],
            abil[2],
            abil[3]
        );
    }
    if names_mode() {
        let chosen: Vec<Vec<u64>> = NAMES_CHOSEN.iter().map(|k| k.iter().map(|x| x.load(Ordering::Relaxed)).collect()).collect();
        let taxed = NAMES_TAXED.load(Ordering::Relaxed);
        for (card, c) in ["Anointed Peacekeeper", "Disruptor Flute"].iter().zip(&chosen) {
            println!(
                "FUZZ_POOL=names: {card} named {}",
                NAME_KINDS.iter().zip(c).map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
            );
        }
        println!("FUZZ_POOL=names: spells cast with a name chosen against them: {taxed}");
    }
    if sns_mode() {
        let seen: Vec<u64> = SNS_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let (free, unc, named) = (SNS_FREE.load(Ordering::Relaxed), SNS_UNCOUNTERABLE.load(Ordering::Relaxed), SNS_NAMED.load(Ordering::Relaxed));
        println!(
            "FUZZ_POOL={}: seen on the stack {}; spells cast without paying their mana costs {free}, made uncounterable {unc}; names chosen {named}",
            if pool_mode() == PoolMode::Sns { "sns" } else { "sneak_show" },
            SNS_CARDS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")
        );
    }
    if hogaak_a_mode() {
        let seen: Vec<u64> = HOGAAK_A_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        println!("FUZZ_POOL=hogaak_a: Hogaak, Arisen Necropolis {}", HOGAAK_A_KINDS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", "));
    }
    if hogaak_b_mode() {
        let n: Vec<u64> = HOGAAK_B_COUNTS.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        println!(
            "FUZZ_POOL=hogaak_b: {}",
            HOGAAK_B_NAMES.iter().zip(&n).map(|(k, c)| format!("{k} {c}")).collect::<Vec<_>>().join(", ")
        );
    }
    if hogaak_c_mode() {
        let seen: Vec<u64> = HOGAAK_C_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let l = |x: &AtomicU64| x.load(Ordering::Relaxed);
        println!(
            "FUZZ_POOL=hogaak_c: cast {} (Dryad Arbor played as a land); Moonshadow triggers {}, -1/-1 counters removed {}; Dark Matter Manipulators at +2/+0 or more {}; Dryad Arbor attacking {}, tapped for mana {}",
            HOGAAK_C_CARDS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", "),
            l(&HOGAAK_C_MOON_TRIGGERS),
            l(&HOGAAK_C_MOON_REMOVED),
            l(&HOGAAK_C_DMM_BIG),
            l(&HOGAAK_C_ARBOR_ATTACKS),
            l(&HOGAAK_C_ARBOR_MANA)
        );
    }
    if lands_b_mode() {
        println!(
            "FUZZ_POOL=lands_b (ENGINE.md 270.2): {}",
            LANDS_B_KINDS.iter().zip(&LANDS_B_SEEN).map(|(n, c)| format!("{n} {}", c.load(Ordering::Relaxed))).collect::<Vec<_>>().join(", ")
        );
    }
    if hogaak_mode() {
        let l = |x: &AtomicU64| x.load(Ordering::Relaxed);
        println!(
            "FUZZ_POOL=hogaak (ENGINE.md 220.4): Hogaak, Arisen Necropolis {}; {}; Mockingbird {}; Massacre {}; Stitcher's Supplier, Dark Matter Manipulator, Moonshadow cast {}, Dryad Arbor played {}; Moonshadow triggers {}, -1/-1 counters removed {}; Dark Matter Manipulators at +2/+0 or more {}; Dryad Arbor attacking {}, tapped for mana {}",
            HOGAAK_A_KINDS.iter().zip(&HOGAAK_A_SEEN).map(|(n, c)| format!("{n} {}", l(c))).collect::<Vec<_>>().join(", "),
            HOGAAK_KINDS.iter().zip(&HOGAAK_SEEN).map(|(n, c)| format!("{n} {}", l(c))).collect::<Vec<_>>().join(", "),
            HOGAAK_B_NAMES[..3].iter().zip(&HOGAAK_B_COUNTS).map(|(n, c)| format!("{n} {}", l(c))).collect::<Vec<_>>().join(", "),
            HOGAAK_B_NAMES[3..6].iter().zip(&HOGAAK_B_COUNTS[3..6]).map(|(n, c)| format!("{n} {}", l(c))).collect::<Vec<_>>().join(", "),
            HOGAAK_C_SEEN[..3].iter().map(l).map(|c| c.to_string()).collect::<Vec<_>>().join(" / "),
            l(&HOGAAK_C_SEEN[3]),
            l(&HOGAAK_C_MOON_TRIGGERS),
            l(&HOGAAK_C_MOON_REMOVED),
            l(&HOGAAK_C_DMM_BIG),
            l(&HOGAAK_C_ARBOR_ATTACKS),
            l(&HOGAAK_C_ARBOR_MANA)
        );
    }
    if delver_sb_mode() {
        let seen: Vec<u64> = DELVER_SB_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let at = DELVER_SB_AT_ABILITY.load(Ordering::Relaxed);
        println!(
            "FUZZ_POOL=delver_sb: seen on the stack {}; Consign to Memory aimed at a triggered ability {at}",
            DELVER_SB_SPELLS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")
        );
    }
    if cross_mode() {
        let seen: Vec<u64> = CROSS_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        println!(
            "FUZZ_POOL=cross: Doomsday against Sneak and Show; seen on the stack {}",
            CROSS_SPELLS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")
        );
    }
    // the end-of-run checks over the chunk's counters (ENGINE.md 167: a shard's are left to the merged totals)
    if meta_mode() {
        println!("FUZZ_POOL=meta (ENGINE.md 176): games per cell (p0 x p1) {meta_cells:?}; lockstep with the pairings' fixed games: {meta_steps} steps, {meta_worlds} worlds");
    }
    let mut t = games_totals(companions_used as u64);
    meta_totals(&mut t, "g", &meta_cells, meta_steps, meta_worlds);
    if wheel_mode() {
        let c = wheel.classes;
        println!(
            "FUZZ_POOL=meta_wheel (ENGINE.md 180): Doomsday resolutions voluntary {} forced {} in hand {} not findable {} missed {}; worlds compared {}, the decider's worlds stepped {}, rows without the option compared {}, libraries checked {}; resolution decisions value-only {} (180.12), with their policy {}",
            c[0], c[1], c[2], c[3], c[4], wheel.worlds, wheel.stepped, wheel.rows, wheel.libraries, wheel.value_only, wheel.pile_free
        );
        for (k, name) in ["voluntary", "forced", "in_hand", "not_findable", "missed"].iter().enumerate() {
            t.set(&format!("wheel_{name}"), c[k]);
        }
        t.set("wheel_worlds", wheel.worlds);
        t.set("wheel_stepped", wheel.stepped);
        t.set("wheel_rows", wheel.rows);
        t.set("wheel_libraries", wheel.libraries);
        t.set("wheel_value_only", wheel.value_only);
        t.set("wheel_pile_free", wheel.pile_free);
    }
    end_of_run("games", first, n, &t, check_games);
    Run::Done
}

/// A chunk's end (ENGINE.md 167): its counters as one `FUZZ_TOTALS {"kind", "first", "n", "totals"}` line (what
/// `tools/fuzz_shards.py` sums over shards), then its end-of-run checks over `n` seeds - unless `FUZZ_SHARD=1` (one
/// shard of a sharded run: a small shard must not dodge a check that needs 200 games, nor pass one alone; the merged
/// totals are checked over the whole run's seeds by [`check_merged`]).
fn end_of_run(kind: &str, first: u64, n: u64, t: &Totals, check: fn(u64, &Totals)) {
    println!("FUZZ_TOTALS {}", serde_json::json!({"kind": kind, "first": first, "n": n, "totals": t.0}));
    if super::env::var("FUZZ_SHARD").is_ok_and(|v| v == "1") {
        println!("FUZZ_SHARD=1: the end-of-run checks of these {kind} are the merged totals' (tools/fuzz_shards.py)");
    } else {
        check(n, t);
    }
}

/// The merged totals of a sharded run (ENGINE.md 167; `tools/fuzz_shards.py`'s JSON: `{"games" | "matches" | "post":
/// {"n": seeds, "totals": {...}}}`, each kind its shards' counters summed, `max_` ones their max) through the same
/// end-of-run checks as one run of those seeds (a failure panics with the check's message) -> one line per kind checked.
pub fn check_merged(text: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("merged totals: {e}"))?;
    let mut out = Vec::new();
    for (kind, check) in [("games", check_games as fn(u64, &Totals)), ("matches", check_matches), ("post", check_post)] {
        let Some(k) = v.get(kind) else { continue };
        let n = k["n"].as_u64().ok_or_else(|| format!("merged totals: {kind} without n"))?;
        let t = Totals::from_json(&k["totals"]).map_err(|e| format!("merged totals: {kind}: {e}"))?;
        check(n, &t);
        out.push(format!("fuzz totals: {kind} checked over {n} seeds ({} counters, merged shards)", t.0.len()));
    }
    if out.is_empty() {
        return Err("merged totals: no games, matches or post kind".into());
    }
    Ok(out)
}

/// The counters the games' end-of-run checks read ([`check_games`]): this process's (one chunk of games).
fn games_totals(companions_used: u64) -> Totals {
    let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
    let mut t = Totals::default();
    for (k, v) in [
        ("companions_used", companions_used),
        ("max_power", MAX_POWER.load(Ordering::Relaxed).max(0) as u64),
        ("hd_worlds", l(&HD_WORLDS)),
        ("hd_other_deck", l(&HD_OTHER_DECK)),
        ("hd_steps", l(&HD_STEPS)),
        ("dc_choices", l(&DC_CHOICES)),
        ("dc_worlds", l(&DC_WORLDS)),
        ("dc_eval_chosen", l(&DC_EVAL_CHOSEN)),
        ("dt_worlds", l(&DT_WORLDS)),
        ("dt_ratio", l(&DT_RATIO)),
        ("belief_worlds", l(&BELIEF_WORLDS)),
        ("belief_dealt", l(&BELIEF_DEALT)),
        ("belief_uniform", l(&BELIEF_UNIFORM)),
        ("gy_lists", l(&GY_LISTS)),
        ("is_walks", l(&IS_WALKS)),
        ("loopdrop_checked", l(&LOOPDROP_CHECKED)),
        ("loopdrop_recur", l(&LOOPDROP_RECUR)),
        ("is_steps", l(&IS_STEPS)),
        ("chance_walks", l(&CHANCE_WALKS)),
        ("chance_steps", l(&CHANCE_STEPS)),
        ("chance_forced", l(&CHANCE_FORCED)),
        ("dd_b_misdirections", l(&DD_B_MISDIRECTIONS)),
        ("dd_b_retargets", l(&DD_B_RETARGETS)),
        ("dd_b_flusterstorms", l(&DD_B_FLUSTERSTORMS)),
        ("dd_b_copies", l(&DD_B_COPIES)),
        ("dd_b_cavern_types", l(&DD_B_CAVERN_TYPES)),
        ("dd_b_uncounterable", l(&DD_B_UNCOUNTERABLE)),
        ("delver_murktides", l(&DELVER_MURKTIDES)),
        ("delver_murk_triggers", l(&DELVER_MURK_TRIGGERS)),
        ("delver_charms", l(&DELVER_CHARMS)),
        ("dd_a_pacts", l(&DD_A_PACT_TRIGGERS)),
        ("ddc_edge_cast", l(&DDC_EDGE_CAST)),
        ("ddc_edge_cycled", l(&DDC_EDGE_CYCLED)),
        ("ddc_wraith_cast", l(&DDC_WRAITH_CAST)),
        ("ddc_wraith_cycled", l(&DDC_WRAITH_CYCLED)),
        ("ddc_leyline_opening", l(&DDC_LEYLINE_OPENING)),
        ("ddc_leyline_games", l(&DDC_LEYLINE_GAMES)),
        ("ddc_leyline_exile_games", l(&DDC_LEYLINE_EXILE_GAMES)),
        ("doomsday_retargets", l(&DOOMSDAY_RETARGETS)),
        ("doomsday_cavern_types", l(&DOOMSDAY_CAVERN_TYPES)),
        ("names_taxed", l(&NAMES_TAXED)),
        ("sns_free", l(&SNS_FREE)),
        ("sns_uncounterable", l(&SNS_UNCOUNTERABLE)),
        ("sns_named", l(&SNS_NAMED)),
        ("delver_sb_at_ability", l(&DELVER_SB_AT_ABILITY)),
        ("hogaak_c_moon_triggers", l(&HOGAAK_C_MOON_TRIGGERS)),
        ("hogaak_c_moon_removed", l(&HOGAAK_C_MOON_REMOVED)),
        ("hogaak_c_dmm_big", l(&HOGAAK_C_DMM_BIG)),
        ("hogaak_c_arbor_attacks", l(&HOGAAK_C_ARBOR_ATTACKS)),
        ("hogaak_c_arbor_mana", l(&HOGAAK_C_ARBOR_MANA)),
    ] {
        t.set(k, v);
    }
    let arrays: [(&str, &[AtomicU64]); 11] = [
        ("dd_a_cast", &DD_A_CASTS),
        ("doomsday_cast", &DOOMSDAY_CASTS),
        ("doomsday_ability", &DOOMSDAY_ABILITY_COUNTS),
        ("sns_seen", &SNS_SEEN),
        ("delver_sb_seen", &DELVER_SB_SEEN),
        ("cross_seen", &CROSS_SEEN),
        ("hogaak_a_seen", &HOGAAK_A_SEEN),
        ("hogaak_b", &HOGAAK_B_COUNTS),
        ("hogaak_c_seen", &HOGAAK_C_SEEN),
        ("hogaak_seen", &HOGAAK_SEEN),
        ("lands_b_seen", &LANDS_B_SEEN),
    ];
    for (name, a) in arrays {
        for (i, x) in a.iter().enumerate() {
            t.set(&format!("{name}_{i}"), l(x));
        }
    }
    for (q, row) in NAMES_CHOSEN.iter().enumerate() {
        for (k, x) in row.iter().enumerate() {
            t.set(&format!("names_chosen_{q}_{k}"), l(x));
        }
    }
    t
}

/// `t`'s array counter `name_0, name_1, ...` (`len` of them).
fn totals_vec(t: &Totals, name: &str, len: usize) -> Vec<u64> {
    (0..len).map(|i| t.get(&format!("{name}_{i}"))).collect()
}

/// The least games over which a Delver pool's run must have seen Murktide Regent's leave-the-graveyard trigger on the
/// stack (ENGINE.md 209). The games of `delver`, `delver_dealt` and `delver_dealt2` are the same (the pinned 60s; the
/// dealt decks are their matches'): over seeds 0..16,000 a Regent cast 7,794 times and a Charm 11,697 (at least 98 and 159
/// in every run of 250 games), the trigger seen 280 times, clustered, and in 3 of the 64 runs of 250 not at all - about
/// one game in 80 has one. Farm chunks of 239, 319 and 325 games missed it by chance and failed. Over 2,000 games a miss
/// has a probability of about e^-24 (under 1e-7 even at the 95% bound of 3 in 64).
pub const DELVER_TRIGGER_GAMES: u64 = 2000;

/// The games' end-of-run checks over `n` seeds' counters `t` (one chunk's, or a sharded run's merged totals: ENGINE.md
/// 165): what the mode must have exercised, each over its own least number of games (200 for most).
pub fn check_games(n: u64, t: &Totals) {
    check_dropped_loops("games", t.get("loopdrop_checked"), t.get("loopdrop_recur"));
    meta_check(t, "g", n, 200, false);
    // (`FUZZ_POOL=meta_wheel`, ENGINE.md 180: forced and missed resolutions seen, worlds and rows compared)
    if wheel_mode() && n >= 200 {
        for k in ["wheel_forced", "wheel_missed", "wheel_worlds", "wheel_stepped", "wheel_rows", "wheel_libraries", "wheel_value_only", "wheel_pile_free"] {
            assert!(t.get(k) > 0, "FUZZ_POOL=meta_wheel: {k} is 0 in {n} games");
        }
    }
    let g = |k: &str| t.get(k);
    if n >= 200 && pool_mode() == PoolMode::Default {
        assert!(g("companions_used") > 0, "no companion special action in {n} games");
    }
    if n >= 100 && exponential_mode() {
        assert!(g("max_power") > u32::MAX as u64, "no board past u32::MAX power: the most {}", g("max_power"));
    }
    if hidden_decks_mode() && n >= 200 {
        let (w, x) = (g("hd_worlds"), g("hd_other_deck"));
        assert!(w > 0 && x * 10 > w * 9, "FUZZ_HIDDEN_DECKS: {x} of {w} worlds with another deck");
    }
    if deck_choice_fuzz().filter(|_| sealed_mode() && hidden_decks_mode()).is_some() && n >= 100 {
        let (c, w, ch) = (g("dc_choices"), g("dc_worlds"), g("dc_eval_chosen"));
        if super::env::var("FUZZ_DECK_CHOICE_BY").is_ok() {
            assert!(ch > 0 && c == 0, "FUZZ_DECK_CHOICE_BY: {ch} seats chose, {c} decisions (the evaluator's choice is none)");
        } else {
            assert!(c > 0 && w > 0, "FUZZ_DECK_CHOICE: no deck choice was played");
        }
    }
    if deck_tilt_fuzz().is_some() {
        let w = g("dt_worlds");
        let mean = g("dt_ratio") as f64 / 1e6 / w.max(1) as f64;
        if w >= 100 {
            assert!(mean > 1.5, "FUZZ_DECK_TILT: the deck tilt does not concentrate the deal ({mean:.3})");
        }
    }
    if belief_tilt().is_some() {
        let (w, d, u) = (g("belief_worlds"), g("belief_dealt"), g("belief_uniform"));
        if w >= 100 {
            assert!(d > 2 * u, "FUZZ_BELIEF: the weighted deal does not concentrate ({d} vs uniform {u})");
        }
    }
    if n >= 200 {
        assert!(g("gy_lists") > 0, "no graveyard-list decision in {n} games");
        let (isw, iss, hds) = (g("is_walks"), g("is_steps"), g("hd_steps"));
        if hidden_decks_mode() {
            assert!(hds > 0, "FUZZ_HIDDEN_DECKS: no information-set walk compared a state after a common action in {n} games");
        } else {
            assert!(isw > 0 && iss > 0, "no information-set walk compared a state after a common action in {n} games ({isw} walks)");
        }
    }
    if chance_every().is_some() && n >= 100 {
        let (w, s, f) = (g("chance_walks"), g("chance_steps"), g("chance_forced"));
        assert!(f > 0, "FUZZ_CHANCE: no replay took a forced outcome ({w} walks, {s} steps)");
    }
    if dd_b_mode() && n >= 200 {
        let (m, r, f, c, ty, u) = (g("dd_b_misdirections"), g("dd_b_retargets"), g("dd_b_flusterstorms"), g("dd_b_copies"), g("dd_b_cavern_types"), g("dd_b_uncounterable"));
        assert!(m > 0 && r > 0 && f > 0 && c > 0 && ty > 0 && u > 0, "lane B's cards weren't played: {m} {r} {f} {c} {ty} {u}");
    }
    if delver_mode() && n >= 200 {
        let (m, tr, c) = (g("delver_murktides"), g("delver_murk_triggers"), g("delver_charms"));
        assert!(m > 0 && c > 0, "the legacy cards weren't played: {m} {tr} {c}");
        if n >= DELVER_TRIGGER_GAMES {
            assert!(tr > 0, "Murktide Regent's leave-the-graveyard trigger never seen on the stack in {n} games ({m} Regents cast)");
        }
    }
    if dd_a_mode() && n >= 200 {
        let casts = totals_vec(t, "dd_a_cast", DD_A_CASTS.len());
        let pacts = g("dd_a_pacts");
        assert!(casts.iter().all(|&c| c > 0) && pacts > 0, "lane A's cards weren't played: {casts:?} {pacts}");
    }
    if ddc_mode() && n >= 300 {
        let (ec, ey, wc, wy) = (g("ddc_edge_cast"), g("ddc_edge_cycled"), g("ddc_wraith_cast"), g("ddc_wraith_cycled"));
        let (lo, lg, le) = (g("ddc_leyline_opening"), g("ddc_leyline_games"), g("ddc_leyline_exile_games"));
        assert!(ec > 0 && ey > 0 && wc > 0 && wy > 0 && lo > 0 && lg > 0 && le > 0, "lane C's cards weren't played");
    }
    if doomsday_mode() {
        let casts = totals_vec(t, "doomsday_cast", DOOMSDAY_CASTS.len());
        let abil = totals_vec(t, "doomsday_ability", DOOMSDAY_ABILITY_COUNTS.len());
        let (r, ty) = (g("doomsday_retargets"), g("doomsday_cavern_types"));
        let cast = |name: &str| casts[DOOMSDAY_SPELLS.iter().position(|&n| n == name).unwrap()];
        if n >= 200 {
            let main = ["Doomsday", "Pact of Negation", "Personal Tutor", "Flow State", "Misdirection", "Thassa's Oracle", "Murktide Regent", "Prismari Charm"];
            assert!(main.iter().all(|&m| cast(m) > 0), "the decks' cards weren't played: {casts:?}");
            assert!(abil[0] > 0 && abil[1] > 0 && abil[2] > 0 && r > 0 && ty > 0, "the legacy cards' abilities weren't used: {abil:?} {r} {ty}");
        }
        // (Edge of Autumn and Street Wraith are cycled far more often than
        // cast: their casts are asserted over 2,000 games.)
        if n >= 2000 {
            assert!(cast("Edge of Autumn") > 0 && cast("Street Wraith") > 0, "Edge of Autumn or Street Wraith never cast: {casts:?}");
        }
    }
    if names_mode() && n >= 200 {
        let chosen: Vec<Vec<u64>> = (0..NAMES_CHOSEN.len()).map(|q| totals_vec(t, &format!("names_chosen_{q}"), NAMES_CHOSEN[q].len())).collect();
        let taxed = g("names_taxed");
        assert!([0, 2, 3, 4, 7].iter().all(|&k| chosen[0][k] > 0), "Anointed Peacekeeper's kinds of names: {chosen:?}");
        assert!(chosen[1].iter().sum::<u64>() > 0 && taxed > 0, "Disruptor Flute's names, taxed spells: {chosen:?} {taxed}");
    }
    if sns_mode() && n >= 200 {
        let seen = totals_vec(t, "sns_seen", SNS_SEEN.len());
        let (free, unc, named) = (g("sns_free"), g("sns_uncounterable"), g("sns_named"));
        if pool_mode() == PoolMode::Sns {
            assert!(seen.iter().all(|&c| c > 0) && free > 0 && unc > 0 && named > 0, "the lane's cards weren't played: {seen:?} {free} {unc} {named}");
        }
        // (The deck's one Omniscience, ten mana, mostly enters by Show and Tell: its free casts
        // are what is required.)
        if pool_mode() == PoolMode::SneakShow {
            assert!(seen[0] > 0 && seen[2] > 0 && free > 0 && unc > 0, "the deck's lane cards weren't played: {seen:?} {free} {unc}");
        }
    }
    if hogaak_a_mode() && n >= 200 {
        let seen = totals_vec(t, "hogaak_a_seen", HOGAAK_A_SEEN.len());
        assert!(seen.iter().all(|&c| c > 0), "Hogaak's casts and payments: {:?}", HOGAAK_A_KINDS.iter().zip(&seen).collect::<Vec<_>>());
    }
    if hogaak_b_mode() && n >= 200 {
        let c = totals_vec(t, "hogaak_b", HOGAAK_B_COUNTS.len());
        assert!(c[..5].iter().all(|&x| x > 0), "the lane's cards weren't played: {:?}", HOGAAK_B_NAMES.iter().zip(&c).collect::<Vec<_>>());
    }
    if hogaak_c_mode() && n >= 200 {
        let seen = totals_vec(t, "hogaak_c_seen", HOGAAK_C_SEEN.len());
        let (trig, removed, big, attacks, mana) =
            (g("hogaak_c_moon_triggers"), g("hogaak_c_moon_removed"), g("hogaak_c_dmm_big"), g("hogaak_c_arbor_attacks"), g("hogaak_c_arbor_mana"));
        assert!(
            seen.iter().all(|&c| c > 0) && trig > 0 && removed > 0 && big > 0 && attacks > 0 && mana > 0,
            "lane C's cards weren't played: {seen:?} {trig} {removed} {big} {attacks} {mana}"
        );
    }
    if lands_b_mode() && n >= 200 {
        let seen = totals_vec(t, "lands_b_seen", LANDS_B_SEEN.len());
        assert!(seen.iter().all(|&c| c > 0), "lane B's cards weren't played: {:?}", LANDS_B_KINDS.iter().zip(&seen).collect::<Vec<_>>());
    }
    if hogaak_mode() && n >= 200 {
        // (ENGINE.md 220.4: the seven cards; Massacre, a sideboard card, is the matches')
        let own = totals_vec(t, "hogaak_seen", HOGAAK_SEEN.len());
        let b = totals_vec(t, "hogaak_b", HOGAAK_B_COUNTS.len());
        let c = totals_vec(t, "hogaak_c_seen", HOGAAK_C_SEEN.len());
        let (removed, big) = (g("hogaak_c_moon_removed"), g("hogaak_c_dmm_big"));
        let need = [
            ("Hogaak cast from a graveyard by convoke and delve", own[0]),
            ("a Mockingbird copy", b[1]),
            ("Moonshadow -1/-1 counters removed", removed),
            ("Dark Matter Manipulator at +2/+0", big),
            ("Dryad Arbor played", c[3]),
            ("Stitcher's Supplier triggers resolved", own[2]),
        ];
        assert!(need.iter().all(|x| x.1 > 0), "FUZZ_POOL=hogaak: the deck's cards weren't played: {need:?}");
    }
    if delver_sb_mode() && n >= 200 {
        let seen = totals_vec(t, "delver_sb_seen", DELVER_SB_SEEN.len());
        let at = g("delver_sb_at_ability");
        assert!(seen[..5].iter().all(|&c| c > 0) && at > 0, "the five cards weren't played: {seen:?} {at}");
    }
    if cross_mode() && n >= 200 {
        let seen = totals_vec(t, "cross_seen", CROSS_SEEN.len());
        assert!(seen[0] > 0 && seen[2] > 0, "Doomsday or Show and Tell never cast: {seen:?}");
    }
}

/// The matches' end-of-run checks over `n` matches' counters `t` (ENGINE.md 167; each needs 20 matches).
pub fn check_matches(n: u64, t: &Totals) {
    check_dropped_loops("matches", t.get("match_loopdrop_checked"), t.get("match_loopdrop_recur"));
    meta_check(t, "m", n, 90, false);
    if n < 20 {
        return;
    }
    let g = |k: &str| t.get(k);
    if hogaak_mode() && n >= 500 {
        // (ENGINE.md 220.4: Massacre is a sideboard card of the Hogaak 75: random sideboarding and random play cast
        // it about once in 35 matches and resolve it about once in 70)
        assert!(g("hogaak_massacre") > 0, "FUZZ_POOL=hogaak: no Massacre resolved in {n} matches");
    }
    if dealt_mode() || pair_mode() {
        assert!(g("dealt_matches") > 0 && g("dealt_decks") > 0, "no dealt match");
    }
    if per_seat_mode() || pair_mode() {
        assert!(g("table_decisions") > 0, "no choice table decision (ENGINE.md 168.3)");
    }
    if pair_mode() && n >= 100 {
        // (ENGINE.md 240: the forced find beside the dealt decks in best-of-three)
        assert!(g("ff_res") > 0 && g("ff_forced") > 0, "FUZZ_POOL=meta_pair: no Doomsday resolution forced in {n} matches ({} resolutions)", g("ff_res"));
    }
    assert!(g("pk_new_games") > 0 && g("pk_new_game_keys") > 0, "the private-key check reached no game a world started (ENGINE.md 141)");
    assert!(g("sb") > 0 && g("changed") > 0 && g("three") > 0, "no sideboarding, changed deck or match of three");
    let (w, s) = (g("match_is_walks"), g("match_is_steps"));
    assert!(s > 0, "no match information-set walk compared a state after a common action ({w} walks)");
    if constructed_bo3_mode() {
        assert!(g("hidden_changed") * 2 > g("hidden") && g("choices_dealt") > 0 && g("tilted") > 0, "the constructed 60s are rarely dealt");
    } else {
        // (Interaction lists have no first picks: open decklists.)
        assert!(
            interactions_mode() || g("first_picks_changed") > g("worlds") / 4 && g("decks_changed") > g("worlds") / 4,
            "the hidden pool is rarely resampled"
        );
    }
}

/// The post-sideboard matches' end-of-run checks over `n` matches' counters `t` (ENGINE.md 167; 20 matches).
pub fn check_post(n: u64, t: &Totals) {
    if n < 20 {
        return;
    }
    let g = |k: &str| t.get(k);
    if dealt_mode() || pair_mode() {
        assert!(g("dealt_matches") > 0 && g("dealt_decks") > 0, "post-sideboard matches: no dealt match");
    }
    if per_seat_mode() || pair_mode() {
        assert!(g("table_decisions") > 0, "post-sideboard matches: no choice table decision (ENGINE.md 168.3)");
    }
    meta_check(t, "p", n, 20, true);
    // (`FUZZ_POOL=meta` has no keeper: the metagame refuses keep_deck, ENGINE.md 176)
    assert!(
        g("sb") > 0 && g("changed") > 0 && g("three") > 0 && (meta_mode() || g("kept_or_dealt") > 0) && g("choices_dealt") > 0 && g("tilted") > 0,
        "post-sideboard matches: no sideboarding, changed deck, match of three or keeper"
    );
    assert!(g("pk_new_games") > 0, "post-sideboard matches: the private-key check reached no game a world started (ENGINE.md 141)");
}

/// The spec of fuzzed match `seed`: random pools (bo3's default), the
/// evaluation pools, M8a's generated decks with 7-card sideboards, or short
/// games (drawn games: the turn cap).
pub fn match_spec(seed: u64) -> crate::az::MtgSpec {
    if pool_mode() == PoolMode::Drafted {
        return drafted_spec(seed);
    }
    if delver_mode() {
        return delver_bo3_spec(seed);
    }
    if cross_mode() || hogaak_mode() {
        return cross_bo3_spec_with(seed, serde_json::json!({}));
    }
    if meta_mode() {
        // (ENGINE.md 176: the best-of-three metagame; each match's pairing the engine's draw from its seed)
        return crate::az::MtgSpec::parse(&meta_match_json(seed, false)).unwrap();
    }
    if interactions_mode() {
        // Combo-dense lists (cube cards), best-of-three, open decklists;
        // one match in four short.
        let db = CardDb::cube();
        let mut rng = Rng::new(seed ^ 0x1A7E_DEC5);
        let p = [interaction_deck(db, &mut rng), interaction_deck(db, &mut rng)];
        let mut s = crate::az::MtgSpec::parse(r#"{"format": "bo3", "pools": false}"#).unwrap();
        s.decks = crate::az::DeckMode::ListsWithSideboards(p);
        if seed % 4 == 3 {
            s.max_turns = 12;
        }
        return s;
    }
    let json = match seed % 4 {
        0 => r#"{"format": "bo3"}"#,
        1 => r#"{"format": "bo3", "decks": "eval"}"#,
        2 => r#"{"format": "bo3", "pools": false}"#,
        _ => r#"{"format": "bo3", "max_turns": 12}"#,
    };
    crate::az::MtgSpec::parse(json).unwrap()
}

/// `FUZZ_POOL=delver`'s matches (ENGINE.md 120): constructed best-of-three, both seats the pinned
/// main and the sideboard cards of the 75 this checkout supports (all 15 since ENGINE.md 122);
/// one match in four P1 plays another 75 of supported cards (`OTHER_75`: the format takes any two
/// 75s), one in four 12-turn games (drawn games).
fn delver_bo3_spec(seed: u64) -> crate::az::MtgSpec {
    delver_bo3_spec_with(seed, serde_json::json!({}), false)
}

/// `FUZZ_POOL=delver`'s post-sideboard matches (ENGINE.md 130): `delver_bo3_spec`'s with `"start_game": 2` and one
/// episode per game; one match in three a seat keeps its deck (`"keep_deck"`, the yardstick's). Mirrors only (the
/// spec refuses `"start_game": 2` for two different 75s, ENGINE.md 147.4): `delver_bo3_spec`'s one match in four
/// with `OTHER_75` plays it in both seats.
pub fn delver_post_spec(seed: u64) -> crate::az::MtgSpec {
    if meta_mode() {
        // (ENGINE.md 176: the mirrors-only metagame, post-sideboard; no keeper - the metagame refuses keep_deck)
        return crate::az::MtgSpec::parse(&meta_match_json(seed, true)).unwrap();
    }
    let mut extra = serde_json::json!({"start_game": 2, "game_episodes": true});
    // (dealt decks refuse a keeper: ENGINE.md 154)
    if seed % 3 == 2 && !dealt_mode() {
        extra["keep_deck"] = serde_json::json!([(seed / 3) % 2]);
    }
    if cross_mode() || hogaak_mode() {
        // (`FUZZ_POOL=cross`, ENGINE.md 148, and `hogaak`, 220.4: their pairings of the real 75s; post-sideboard, their
        // mirrors, 147.4)
        return cross_bo3_spec_with(seed, extra);
    }
    delver_bo3_spec_with(seed, extra, true)
}

fn delver_bo3_spec_with(seed: u64, extra: serde_json::Value, mirror: bool) -> crate::az::MtgSpec {
    let d = crate::az::constructed::Decklist::izzet_delver();
    let db = CardDb::constructed();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().filter(|(_, n)| db.id(n).is_some()).flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let expand = |l: &[(u32, &str)]| -> Vec<String> { l.iter().flat_map(|&(k, n)| std::iter::repeat_n(n.to_string(), k as usize)).collect() };
    let (main1, side1) = if seed % 4 == 1 { (expand(&OTHER_75.0), expand(&OTHER_75.1)) } else { (main.clone(), side.clone()) };
    let (main, side) = if mirror && seed % 4 == 1 { (main1.clone(), side1.clone()) } else { (main, side) };
    let max_turns = if seed % 4 == 3 { 12 } else { 60 };
    let mut v = serde_json::json!({"max_turns": max_turns, "constructed": true, "format": "bo3",
        "decks": {"p0": main, "p1": main1, "p0_sideboard": side, "p1_sideboard": side1}});
    if dealt_mode() {
        v["dealt_decks"] = dealt_of(seed);
    }
    if per_seat_mode() && seed % 2 == 0 {
        // (ENGINE.md 168.4: a choice table of the Delver 75 - OTHER_75's seats are not its seats)
        let d = crate::az::constructed::Decklist::izzet_delver();
        let side: Vec<String> = d.sideboard.iter().filter(|(_, n)| db.id(n).is_some()).flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
        v["sideboard_table"] = fuzz_table(seed, &d.main_names(), &side);
    }
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    crate::az::MtgSpec::parse(&v.to_string()).unwrap()
}

/// Another constructed 75 of supported cards (`FUZZ_POOL=delver`'s matches; `tests/constructed_bo3.rs`'s
/// second deck): basic lands past four copies (no free basic land entry), a 12-card sideboard.
#[allow(clippy::type_complexity)]
const OTHER_75: ([(u32, &str); 15], [(u32, &str); 6]) = (
    [
        (4, "Lightning Bolt"),
        (4, "Brainstorm"),
        (4, "Ponder"),
        (4, "Counterspell"),
        (4, "Spell Pierce"),
        (2, "Fiery Confluence"),
        (4, "Force of Will"),
        (4, "Dragon's Rage Channeler"),
        (2, "Murktide Regent"),
        (4, "Volcanic Island"),
        (4, "Scalding Tarn"),
        (8, "Island"),
        (6, "Mountain"),
        (4, "Mishra's Bauble"),
        (2, "Unholy Heat"),
    ],
    [(3, "Pyroblast"), (3, "Force of Negation"), (1, "Karakas"), (2, "Unholy Heat"), (2, "Fiery Confluence"), (1, "Mountain")],
);

/// One match played (`play_match`).
pub struct PlayedMatch {
    /// The match ended aborted (ENGINE.md 100.7).
    pub aborted: bool,
    pub actions: Vec<u16>,
    pub keys: Vec<u64>,
    pub fingerprint: u64,
    pub results: [u8; 3],
    pub games: u8,
    pub sideboard_decisions: u32,
    /// Sideboardings finished (each player's `PASS` between games).
    pub sideboardings: u32,
    pub changed_decks: u32,
    /// Sideboarding decisions of a seat that keeps its deck (`"keep_deck"`, ENGINE.md 130).
    pub keeper_decisions: u32,
    /// Dealt decks (ENGINE.md 154): the match had a dealt seat; its dealt 60s checked (one a later game).
    pub dealt: bool,
    pub dealt_decks: u32,
    /// Choice table decisions played (ENGINE.md 168.3, `FUZZ_POOL=delver_dealt2`).
    pub table_decisions: u32,
}

/// Counts of the hidden-pool checks (ENGINE.md 36.11).
#[derive(Default)]
pub struct HiddenStats {
    worlds: u64,
    first_picks_changed: u64,
    decks_changed: u64,
    /// Constructed best-of-three (ENGINE.md 120): worlds where the opponent's deck is hidden (games
    /// 2-3, between games after game 2), those with another 60 than the real one, the first
    /// sideboarder's hidden choices dealt, and the deck-tilted worlds.
    hidden: u64,
    hidden_changed: u64,
    choices_dealt: u64,
    tilted: u64,
}

/// A world `determinize(viewer)` sampled (`d`) from match `g`: the viewer's
/// view and own cards unchanged, no card it knows resampled; the opponent's
/// pool its known part plus as many first picks; their deck a legal deck of
/// that pool with the public sizes, holding every card of theirs the viewer
/// knows, fulfilling a revealed companion, and in the game the world's
/// cards; the clouds hold.
fn check_hidden_pool(g: &crate::az::MtgAz, d: &crate::az::MtgAz, viewer: u8, seed: u64, stats: &mut HiddenStats) {
    use crate::az::sample;
    let o = 1 - viewer;
    let (v, ou) = (viewer as usize, o as usize);
    assert_eq!(g.m.pools[v], d.m.pools[v], "match {seed}: the viewer's pool was resampled");
    assert_eq!(g.m.decks[v], d.m.decks[v], "match {seed}: the viewer's deck was resampled");
    assert_eq!(g.g.player(viewer).sideboard, d.g.player(viewer).sideboard, "match {seed}: the viewer's sideboard was resampled");
    let ids = crate::az::identity::identities(g.g.db());
    let (rp, sp) = (&g.m.pools[ou], &d.m.pools[ou]);
    let known = |m: &crate::az::MatchState, pool: &crate::az::matches::MatchPool| -> Vec<(u16, u8)> {
        pool.list[..pool.n_cards].iter().copied().filter(|e| sample::known_before(m, o, ids, e.0)).collect()
    };
    assert_eq!(known(&g.m, rp), known(&d.m, sp), "match {seed}: the known part of P{o}'s pool changed");
    assert_eq!(rp.first_picks.len(), sp.first_picks.len());
    assert_eq!(rp.list.len(), sp.list.len());
    assert!(sp.list[..sp.n_cards].windows(2).all(|w| w[0].0 < w[1].0), "match {seed}: sampled pool not sorted");
    assert!(sp.first_picks.iter().all(|f| sp.list.iter().any(|e| e.0 == *f)));
    assert!(sp.first_picks.iter().all(|f| !g.m.pools[v].list.iter().any(|e| e.0 == *f)), "match {seed}: a first pick in the viewer's pool");
    if let Some(pri) = &g.m.priors {
        // a sampled first pick is a candidate of the prior (a real one kept: seen)
        assert!(
            sp.first_picks.iter().all(|f| pri[ou].cands.contains(f) || rp.first_picks.contains(f)),
            "match {seed}: a sampled first pick outside the prior"
        );
    }
    let size = |c: &[u8]| c.iter().map(|&n| n as usize).sum::<usize>();
    assert_eq!(size(&g.m.decks[ou]), size(&d.m.decks[ou]), "match {seed}: deck size");
    for (k, &(_, n)) in sp.list.iter().enumerate() {
        assert!(d.m.decks[ou][k] <= n, "match {seed}: more copies than the pool has");
    }
    let deck = sp.deck(&d.m.decks[ou]);
    if let Some(c) = d.g.player(o).companion_def {
        if let Some(cond) = &g.g.db().def(c).companion {
            assert!(g.g.db().deck_satisfies(cond, &deck), "match {seed}: the sampled deck breaks the revealed companion");
        }
    }
    // What they revealed in this game (between games: the last) is in it.
    let comp = g.g.player(o).companion_def;
    for &(dd, _) in &sp.list {
        let i = ids[dd as usize] as usize;
        if g.m.seen[ou][i >> 6] & (1 << (i & 63)) != 0 && Some(dd) != comp {
            assert!(deck.contains(&dd), "match {seed}: a card P{o} revealed this game is not in the sampled deck");
        }
    }
    stats.worlds += 1;
    stats.first_picks_changed += (rp.first_picks != sp.first_picks) as u64;
    stats.decks_changed += (rp.deck(&g.m.decks[ou]) != deck) as u64;
    if d.sideboarding().is_some() {
        return;
    }
    let mut want = deck.clone();
    if let Some(c) = d.g.player(o).companion_def {
        want.push(c);
        want.sort_unstable();
    }
    assert_eq!(sample::game_cards(d, o), want, "match {seed}: the world's cards of P{o} are not the sampled deck (and companion)");
    assert_eq!(d.g.player(o).sideboard.as_ref(), sp.sideboard(&d.m.decks[ou]).as_slice(), "match {seed}: the sampled sideboard");
    assert_eq!(g.g.sideboard_size(o), d.g.sideboard_size(o));
    assert_eq!(g.g.view_of(viewer), d.g.view_of(viewer), "match {seed}: determinize changed the view of P{viewer}");
    for (i, x) in g.g.objs().iter().enumerate() {
        let c = crate::CardIdx::new(i as u32);
        if g.g.knows(viewer, c) {
            assert_eq!((x.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "match {seed}: a card P{viewer} knows was resampled");
        }
    }
    d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
    d.g.check_knowledge().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
}

/// Does sorted multiset `a` hold sorted multiset `b`?
fn holds_all(a: &[u16], b: &[u16]) -> bool {
    let mut left = a.to_vec();
    b.iter().all(|x| match left.binary_search(x) {
        Ok(i) => {
            left.remove(i);
            true
        }
        Err(_) => false,
    })
}

/// A legal 60 of player `o`'s 75 in match `m` (counts per entry of its constructed pool list).
fn check_legal_60(m: &crate::az::MatchState, o: usize, counts: &[u8], what: &dyn Fn() -> String) {
    let pool = &m.pools[o];
    let n: usize = counts.iter().map(|&c| c as usize).sum();
    assert_eq!(counts.len(), pool.list.len(), "{}: counts of another list", what());
    assert_eq!(n, 60, "{}: a deck of {n} cards", what());
    assert!(counts.iter().zip(&pool.list).all(|(&c, e)| c <= e.1), "{}: more copies than the 75 has", what());
}

/// A world `determinize(viewer)` dealt (`d`) from a constructed best-of-three `g` (ENGINE.md 120):
/// the viewer's pool, decks and sideboarding untouched; the opponent's 75 public and unchanged (the
/// same list); their deck a legal 60 of it - the registered main in game 1 (open), dealt from game
/// 2 on, holding every card the viewer has seen this game (between games: at the last game's end);
/// in a game the world's cards that deck and the sideboard the rest of the 75, the view and every
/// known card unchanged, the invariants and the clouds; between games the first sideboarder's hidden
/// choice a legal 60 and the other player's sideboarding restarted from the world's last deck.
fn check_constructed_world(g: &crate::az::MtgAz, d: &crate::az::MtgAz, viewer: u8, seed: u64, stats: &mut HiddenStats) {
    let o = 1 - viewer;
    let (v, ou) = (viewer as usize, o as usize);
    let what = || format!("match {seed} game {}{}: a constructed world of P{viewer}", g.m.game_no, if g.sideboarding().is_some() { " (between games)" } else { "" });
    assert_eq!(g.m.pools[v], d.m.pools[v], "{}: the viewer's pool", what());
    assert_eq!(g.m.decks[v], d.m.decks[v], "{}: the viewer's deck", what());
    assert_eq!(g.g.player(viewer).sideboard, d.g.player(viewer).sideboard, "{}: the viewer's sideboard", what());
    assert_eq!(g.m.pools[ou].list, d.m.pools[ou].list, "{}: the opponent's 75 is public", what());
    // (ENGINE.md 176: a metagame match's pairing is public, the same in every world; None in every other match)
    assert_eq!(g.m.pairing, d.m.pairing, "{}: a world changed the metagame pairing", what());
    let pool = &d.m.pools[ou];
    assert_eq!((pool.n_cards, pool.min_deck, pool.max_deck), (pool.list.len(), 60, 60), "{}: the constructed pool", what());
    check_legal_60(&d.m, ou, &d.m.decks[ou], &what);
    let deck = pool.deck(&d.m.decks[ou]);
    let real = g.m.pools[ou].deck(&g.m.decks[ou]);
    stats.worlds += 1;
    if g.deck_hidden(o) {
        stats.hidden += 1;
        stats.hidden_changed += (deck != real) as u64;
    } else {
        assert_eq!(deck, real, "{}: game 1's deck is the registered main (open)", what());
    }
    let shown = hd_shown(g, viewer);
    assert!(holds_all(&deck, &shown), "{}: the dealt 60 lacks a card the viewer saw this game", what());
    // dealt decks (ENGINE.md 154): public - every world keeps the dealt seat; a dealt opponent's hidden 60s (the deck,
    // between games its dealt next one) within the spec's swap range, from its dealt prior
    assert_eq!((d.m.dealt, d.m.dealt_both), (g.m.dealt, g.m.dealt_both), "{}: a world changed the dealt seats", what());
    if g.m.is_dealt(o) {
        if let Some((main, (lo, hi))) = DEALT_SPEC.with(|s| s.borrow().as_ref().map(|sp| dealt_main(sp, ou, seed))) {
            if g.deck_hidden(o) {
                assert!((lo..=hi).contains(&swaps_vs(&main, &deck)), "{}: a dealt 60 of {} swaps", what(), swaps_vs(&main, &deck));
            }
            if let Some(x) = d.sideboarding().and_then(|sb| sb.done[ou].as_ref()) {
                let nd = pool.deck(x);
                assert!((lo..=hi).contains(&swaps_vs(&main, &nd)), "{}: a dealt next 60 of {} swaps", what(), swaps_vs(&main, &nd));
            }
        }
    }
    if let Some(sb) = d.sideboarding() {
        let rsb = g.sideboarding().unwrap();
        if let Some(x) = &sb.done[ou] {
            check_legal_60(&d.m, ou, x, &what);
            stats.choices_dealt += 1;
        }
        if let Some(x) = &sb.done[v] {
            assert_eq!(Some(x), rsb.done[v].as_ref(), "{}: the viewer's own choice", what());
        }
        if sb.decider() == o {
            assert_eq!(sb.counts, d.m.decks[ou].to_vec(), "{}: the opponent's sideboarding not restarted from the world's last deck", what());
            assert_eq!((sb.last, sb.asking), (None, None));
        } else {
            assert_eq!((&sb.counts, sb.last, sb.asking), (&rsb.counts, rsb.last, rsb.asking), "{}: the viewer's own sideboarding", what());
        }
        return;
    }
    assert_eq!(hd_deck(d, o), deck, "{}: the world's cards of the opponent are not the dealt 60", what());
    assert_eq!(d.g.player(o).sideboard.as_ref(), pool.sideboard(&d.m.decks[ou]).as_slice(), "{}: the sideboard is the rest of the 75", what());
    assert_eq!(g.g.sideboard_size(o), d.g.sideboard_size(o));
    assert_eq!(hd_deck(d, viewer), hd_deck(g, viewer), "{}: the viewer's own cards", what());
    assert_eq!(g.g.view_of(viewer), d.g.view_of(viewer), "{}: the view", what());
    for (i, x) in g.g.objs().iter().enumerate() {
        let c = crate::CardIdx::new(i as u32);
        if g.g.knows(viewer, c) {
            assert_eq!((x.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "{}: a card the viewer knows was dealt again", what());
        }
    }
    d.g.check_invariants().unwrap_or_else(|e| panic!("{}: {e}", what()));
    d.g.check_knowledge().unwrap_or_else(|e| panic!("{}: {e}", what()));
}

/// `FUZZ_POOL=delver`'s matches (ENGINE.md 120): a deck-tilted world of `viewer` (`determinize_with`,
/// the deck's tilt `FUZZ_DECK_TILT` or 1, the hand's on every other check, clip 1e6, a random prediction
/// of both targets from its own generator: the match plays on unchanged): the decider's actions and the
/// viewer's encoding as in the real match, every check of `check_constructed_world`, and where the deck
/// is hidden in a game the world's deck one of the K cached decks.
fn check_constructed_tilted(g: &crate::az::MtgAz, viewer: u8, seed: u64, step: usize, stats: &mut HiddenStats) {
    use zero_search::game::SearchGame;
    let t = super::env::var("FUZZ_DECK_TILT").ok().map_or(1.0, |s| s.parse::<f64>().unwrap_or_else(|_| panic!("FUZZ_DECK_TILT={s}: a tilt")));
    let mut pr = Rng::new(seed ^ (step as u64).wrapping_mul(0x9E37_79B9) ^ (viewer as u64) << 40 ^ 0xDEC7);
    let mut seg = || {
        let mut p: Vec<f32> = (0..crate::az::identity::D).map(|_| ((pr.below(1000) + 1) as f32).powi(3)).collect();
        let sum: f32 = p.iter().sum();
        p.iter_mut().for_each(|x| *x /= sum);
        p
    };
    let mut pred = seg();
    pred.extend(seg());
    let hand = if pr.below(2) == 0 { t } else { 0.0 };
    let b = zero_search::game::Belief { pred, tilt: hand, clip: 1e6, tilts: vec![0.0, t] };
    let cached = {
        let dt = crate::az::hidden_decks::DeckTilt { p: crate::az::belief::deck_segment(&b.pred), tilt: t, clip: b.clip };
        g.deck_tilt_of(viewer, dt)
    };
    let mut d = g.clone();
    d.determinize_with(viewer as usize, &mut zero_search::rng::Rng::new(pr.next_u64()), &b);
    let mover = g.to_move().unwrap();
    let (mut l0, mut l1) = (Vec::new(), Vec::new());
    g.legal_actions(&mut l0);
    d.legal_actions(&mut l1);
    if viewer as usize == mover {
        assert_eq!(l0, l1, "match {seed}: a deck-tilted world changed the decider's actions");
    } else {
        assert_eq!(d.to_move(), Some(mover));
    }
    let (mut x0, mut x1) = (Vec::new(), Vec::new());
    az_encode(g, viewer, &mut x0);
    az_encode(&d, viewer, &mut x1);
    if let Some(i) = (0..x0.len()).find(|&i| x0[i] != x1[i]) {
        panic!(
            "match {seed}: a deck-tilted world changed the encoding of P{viewer} at {i} ({} / {}); decision {:?}; stack references {} / {}",
            x0[i],
            x1[i],
            g.pending_decision(),
            stack_refs(&g.g, viewer),
            stack_refs(&d.g, viewer)
        );
    }
    check_constructed_world(g, &d, viewer, seed, &mut HiddenStats::default());
    if let Some((decks, _)) = &cached {
        let world = hd_deck(&d, 1 - viewer);
        assert!(decks.iter().any(|x| x.deck == world), "match {seed}: the deck-tilted world's deck is not one of the K cached decks");
        stats.tilted += 1;
    }
}

/// Two worlds sampled for the decider, played with the same actions, give it
/// the same encoding while their keys, next decider and legal actions agree
/// (zero-core's child signature), across games and sideboarding.
fn check_match_information_set(g: &crate::az::MtgAz, viewer: usize, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut zero_search::rng::Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    let (mut xa, mut xb): (Vec<f32>, Vec<f32>);
    MATCH_IS_WALKS.fetch_add(1, Ordering::Relaxed);
    for step in 0..25 {
        // (or a game's end with one episode per game, ENGINE.md 121: the episode is over) Equal keys: the match,
        // and the episode, over in both worlds or in neither (ENGINE.md 148: one world over returned silently).
        let (ea, eb) = ((a.match_over(), a.to_move().is_none()), (b.match_over(), b.to_move().is_none()));
        if ea != eb {
            panic!(
                "match {seed} step {step}: same keys, (match over, episode over) {ea:?} in one world of P{viewer}, {eb:?} in the other; game {} / {}, results {:?} / {:?}
{}
----
{}",
                a.m.game_no,
                b.m.game_no,
                a.m.results,
                b.m.results,
                stack_names(&a.g),
                stack_names(&b.g)
            );
        }
        if ea.0 || ea.1 {
            return;
        }
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            assert!(!(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)), "match {seed} step {step}: the viewer's options differ");
            return;
        }
        let (ra, rb) = (crate::az::tail_encode::Row::of_az_v8(&a, viewer), crate::az::tail_encode::Row::of_az_v8(&b, viewer));
        xa = ra.flat();
        xb = rb.flat();
        if let Some(i) = (0..xa.len().min(xb.len())).find(|&i| xa[i] != xb[i]).or((xa.len() != xb.len()).then(|| xa.len().min(xb.len()))) {
            panic!(
                "match {seed} step {step}: same keys, different encodings of P{viewer} at {i} ({} / {}: {} / {}); decisions {:?} / {:?}; game {} sb {}
{}
----
{}",
                xa.get(i).copied().unwrap_or(f32::NAN),
                xb.get(i).copied().unwrap_or(f32::NAN),
                ra.locate(i),
                rb.locate(i),
                a.pending_decision(),
                b.pending_decision(),
                a.m.game_no,
                a.sideboarding().is_some(),
                stack_names(&a.g),
                stack_names(&b.g)
            );
        }
        // The search's decision class of the node (zero-core's signature with `ANSWERS`): the viewer's worlds see
        // the other player's mana activations with priority as that player holding priority (ENGINE.md 160).
        assert_eq!(a.decision_tag(), b.decision_tag(), "match {seed} step {step}: same keys, different decision classes for P{viewer}; decisions {:?} / {:?}", a.pending_decision(), b.pending_decision());
        if step > 0 {
            MATCH_IS_STEPS.fetch_add(1, Ordering::Relaxed);
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.step(act, &mut ka).unwrap();
        b.step(act, &mut kb).unwrap();
        if ka != kb {
            // (Within one game, ENGINE.md 161: no forced answer of the other player splits worlds the viewer sees the
            // same.)
            if a.sideboarding().is_none() && b.sideboarding().is_none() && a.m.game_no == b.m.game_no {
                if let Some(why) = forced_answer_split(&ka, &kb, &a.g, &b.g, viewer as u8) {
                    panic!("match {seed} step {step}: {why}; decisions {:?} / {:?}\n{}\n----\n{}", a.pending_decision(), b.pending_decision(), stack_names(&a.g), stack_names(&b.g));
                }
            }
            return;
        }
    }
}

/// The matches' information-set walks (`check_match_information_set`, ENGINE.md 148): walks begun and the states
/// compared after at least one common action; the matches' run prints them and, over 20 matches or more, needs
/// compared states.
static MATCH_IS_WALKS: AtomicU64 = AtomicU64::new(0);
static MATCH_IS_STEPS: AtomicU64 = AtomicU64::new(0);

// ---- Every world masks the other player's private keys (ENGINE.md 141; the Fable review's I3) ----

/// The other player's private keys (draws and looks) checked in the worlds the fuzz walks: of the game fuzz's
/// worlds (`check_determinize`, the hidden-decklist worlds) and of the match fuzz's (`play_match_spec`,
/// `check_world_private_keys`); of those, the ones emitted in a game the world itself started (after the
/// sideboarding or the deck choice it was dealt in, or past the end of the game it was dealt in: ENGINE.md
/// 141's H2), and the worlds that started one; and the looks at a random card of the viewer's own hand (Urza's
/// Bauble), the one look of another player that carries the card by design.
static PK_GAME_KEYS: AtomicU64 = AtomicU64::new(0);
static PK_MATCH_KEYS: AtomicU64 = AtomicU64::new(0);
static PK_NEW_GAME_KEYS: AtomicU64 = AtomicU64::new(0);
/// The forced find's resolutions in matches (ENGINE.md 240, `FUZZ_POOL=meta_pair`): all, forced, forced in a game 2 or 3.
static PAIR_FF_RES: AtomicU64 = AtomicU64::new(0);
static PAIR_FF_FORCED: AtomicU64 = AtomicU64::new(0);
static PAIR_FF_FORCED_LATER: AtomicU64 = AtomicU64::new(0);
static PK_NEW_GAMES: AtomicU64 = AtomicU64::new(0);
static PK_HAND_LOOKS: AtomicU64 = AtomicU64::new(0);
/// Of `PK_HAND_LOOKS`, the looks in a step that ended the game and began the next (both seats dealt, ENGINE.md 168.1,
/// 173): the card seen is the game's that ended.
pub static PK_ENDED_GAME_HAND_LOOKS: AtomicU64 = AtomicU64::new(0);
/// Of `PK_HAND_LOOKS`, the looks at a card that changed zones later in the same step (a hand discarded by Memory Jar's
/// delayed trigger at the end step the step ran into, ENGINE.md 175; a hand shuffled away by Timetwister and the card
/// drawn back, 206).
pub static PK_LEFT_HAND_LOOKS: AtomicU64 = AtomicU64::new(0);
/// Of `PK_HAND_LOOKS`, the looks at a card that was not in the viewer's hand when the step began: it came into the hand
/// in the step (drawn after Memory Jar exiled the hand, returned by Daze's alternative cost, ...; ENGINE.md 206).
pub static PK_ENTERED_HAND_LOOKS: AtomicU64 = AtomicU64::new(0);
/// The dealt seat's deal keys (`KEY_DEALT`, ENGINE.md 154.2) checked `HIDDEN` in the other player's worlds.
static PK_DEALT_KEYS: AtomicU64 = AtomicU64::new(0);

/// In a world determinized for `viewer`, the keys `keys` of one step (from its first key: the walks clear the vector
/// before each step; `after`: the world's game right after it; `before`: the viewer's hand before it, `viewer_hand` /
/// `game_viewer_hand`): every draw of another player (`KEY_DRAW`) carries `HIDDEN`, and so does every look (`KEY_LOOK`)
/// of another player (`Game::emit_private` masks them for the world's observer) - a key with the card's identity would
/// split the viewer's tree by a card it cannot see (zero-core docs/search.md 3.16) - but the one look that is public by
/// design: another player's look at a card at random of the viewer's own hand (`Ext::LookAtRandomHandCard`, Urza's
/// Bauble: its owner sees which card was seen). A dealt seat's deal (`KEY_DEALT`, ENGINE.md 154.2: its 60's counts,
/// the dealt seat's private chance) carries `HIDDEN` for every other observer, with no exception. A told-apart card's
/// key (`KEY_TOLD`, ENGINE.md 156: its card index, which every player sees) is public: not a private key, not checked
/// here. Returns how many keys of the other player it checked.
///
/// The public look is recognised by the step's own record of it (`Game::hand_looks`, ENGINE.md 206): the key at its
/// index is that player's look at a card of the viewer's hand with that identity, or the key is a leak. (Through 173 and
/// 175 the check guessed the hand the look saw from the hands before and after the step; a step can change the hand
/// around the look in more ways than those guesses - a card drawn or returned to the hand in the step and looked at,
/// a card looked at and then shuffled away and drawn back - and every such guess failed on a key that was the viewer's
/// own card: 173, 175, 206.) Where the card has not changed zones since the look (its incarnation is the look's) the
/// state shows the rest: it is still in the viewer's hand, and the looker knows it - or the card of that identity in
/// the hand it knew already (the look's twin rule). A step that ends a game and begins the next (both seats dealt,
/// ENGINE.md 168.1) leaves `after` a new game; its keys before the new game's `KEY_GAME` and the looks among them are
/// the game's that ended (ENGINE.md 173), whose cards are gone.
fn check_private_keys_masked_since(keys: &[u64], viewer: u8, after: &Game, before: &[(crate::CardIdx, u64)], what: &dyn Fn() -> String) -> u64 {
    use crate::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT, KEY_DRAW, KEY_GAME, KEY_LOOK};
    // (the step's keys before this index are of the game it ended; none when it began no game)
    let new_game_at = keys.iter().rposition(|&k| key_kind(k) == KEY_GAME).unwrap_or(0);
    let hand = &after.player(viewer).hand;
    let mut n = 0;
    for (i, &k) in keys.iter().enumerate() {
        let (kind, p, d) = (key_kind(k), key_player(k), key_payload(k));
        if !matches!(kind, KEY_DRAW | KEY_LOOK | KEY_DEALT) || p == viewer {
            continue;
        }
        n += 1;
        if kind == KEY_DEALT {
            assert_eq!(d, HIDDEN, "{}: P{p}'s deal (KEY_DEALT) carries its 60's counts in a world of P{viewer} (ENGINE.md 154.2)", what());
            PK_DEALT_KEYS.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        if d == HIDDEN {
            continue;
        }
        let look = (kind == KEY_LOOK).then(|| after.hand_looks().iter().find(|l| l.key as usize == i)).flatten();
        let Some(look) = look.filter(|l| l.looker == p && l.owner == viewer && l.def as u64 == d) else {
            panic!(
                "{}: P{p}'s private key (kind {kind}) carries the card's identity ({d}) in a world of P{viewer}, and it is no look at a card of P{viewer}'s hand (ENGINE.md 141, 206)",
                what()
            );
        };
        PK_HAND_LOOKS.fetch_add(1, Ordering::Relaxed);
        if !before.iter().any(|&(c, _)| c == look.card) {
            PK_ENTERED_HAND_LOOKS.fetch_add(1, Ordering::Relaxed);
        }
        if i < new_game_at {
            PK_ENDED_GAME_HAND_LOOKS.fetch_add(1, Ordering::Relaxed);
        } else if after.obj(look.card).inc != look.inc {
            PK_LEFT_HAND_LOOKS.fetch_add(1, Ordering::Relaxed);
        } else {
            let known = hand.iter().any(|&c| after.obj(c).def == look.def && after.knows(p, c));
            assert!(
                hand.contains(&look.card) && known,
                "{}: P{p}'s look at a card of P{viewer}'s hand ({d}, card {}): the card has not moved since, but it is {} (ENGINE.md 141, 206)",
                what(),
                look.card.get(),
                if hand.contains(&look.card) { "a card of that identity the looker does not know in the hand" } else { "not in the hand" }
            );
        }
    }
    n
}

/// The cards of `viewer`'s hand in game `g` with their identities, for `check_private_keys_masked_since`'s `before`,
/// read before a step.
fn game_viewer_hand(g: &Game, viewer: u8) -> Vec<(crate::CardIdx, u64)> {
    g.player(viewer).hand.iter().map(|&c| (c, g.obj(c).def as u64)).collect()
}

/// `game_viewer_hand` of match state `w` (none between games: no game card is in play).
fn viewer_hand(w: &crate::az::MtgAz, viewer: u8) -> Vec<(crate::CardIdx, u64)> {
    if w.sideboarding().is_some() || w.deck_choice().is_some() {
        return Vec::new();
    }
    game_viewer_hand(&w.g, viewer)
}

/// A world of match `g` determinized for `viewer`, walked as the search walks it into the game the world plays
/// next: from a deck choice or a sideboarding root, the choice or the sideboarding played to its end (`PASS` with
/// probability 1/2 where it is legal, else a random option), then up to 60 actions of the game it starts; from a
/// game's root up to 60 actions, and when the walk passes the game's end (a match without one episode per game)
/// the sideboarding and 60 actions of the next game. Every step's private keys of the other player checked
/// (`check_private_keys_masked_since`). Its own generator: the match's random stream is untouched.
fn check_world_private_keys(g: &crate::az::MtgAz, viewer: usize, seed: u64, step: usize) {
    use zero_search::game::SearchGame;
    let mut rng = Rng::new(seed ^ (step as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (viewer as u64) << 56 ^ 0x0B5E_4FE4);
    let mut w = g.clone();
    w.determinize(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()));
    let between = |x: &crate::az::MtgAz| x.sideboarding().is_some() || x.deck_choice().is_some();
    let root = if g.deck_choice().is_some() { " (the deck choice)" } else if g.sideboarding().is_some() { " (sideboarding)" } else { "" };
    let what = || format!("match {seed} step {step} game {}{root}: a world of P{viewer}", g.m.game_no);
    let (mut legal, mut keys) = (Vec::new(), Vec::new());
    // (`started`: the world is in a game it started; `since`: its actions there)
    let (mut started, mut since, mut n) = (false, 0usize, 0usize);
    while n < 600 && (if started { since < 60 } else { between(&w) || n < 60 }) {
        if w.to_move().is_none() {
            break;
        }
        w.legal_actions(&mut legal);
        let before = between(&w);
        let a = if before && legal[0] == crate::act::PASS && rng.below(2) == 0 { crate::act::PASS } else { legal[rng.below(legal.len() as u32) as usize] };
        keys.clear();
        // (the viewer's hand before the step: the step may end the game and begin the next, 168.1 - a look of that step
        // at a card of this hand is checked against it, ENGINE.md 173)
        let known = viewer_hand(&w, viewer as u8);
        let g0 = w.m.game_no;
        w.step(a, &mut keys).unwrap_or_else(|e| panic!("{}: {e}", what()));
        // (a new game: after a sideboarding, or - both seats dealt, ENGINE.md 168.1 - from the last game's end in one step)
        if (before && !between(&w) || !started && w.m.game_no != g0) && !w.match_over() {
            started = true;
            PK_NEW_GAMES.fetch_add(1, Ordering::Relaxed);
        } else if started {
            since += 1;
        }
        let k = check_private_keys_masked_since(&keys, viewer as u8, &w.g, &known, &|| format!("{}{}", what(), if started { " (in the game it started)" } else { "" }));
        PK_MATCH_KEYS.fetch_add(k, Ordering::Relaxed);
        if started {
            PK_NEW_GAME_KEYS.fetch_add(k, Ordering::Relaxed);
        }
        n += 1;
    }
}

/// The private-key check's counts (ENGINE.md 141).
fn private_keys_report() -> String {
    format!(
        "private keys of the other player masked in every world: {} in the game fuzz's worlds, {} in the match fuzz's, {} of them in {} games a world started; {}; {} deal keys of a dealt seat (ENGINE.md 154)",
        PK_GAME_KEYS.load(Ordering::Relaxed),
        PK_MATCH_KEYS.load(Ordering::Relaxed),
        PK_NEW_GAME_KEYS.load(Ordering::Relaxed),
        PK_NEW_GAMES.load(Ordering::Relaxed),
        hand_looks_report(),
        PK_DEALT_KEYS.load(Ordering::Relaxed)
    )
}

/// The looks at a card of the viewer's hand the private-key check recognised (ENGINE.md 141.2, 206), of every walk
/// (the game fuzz's and the match fuzz's share the counters when they run in one process).
fn hand_looks_report() -> String {
    format!(
        "{} looks at a card of the viewer's hand (public by design), {} of them in a step that began the next game (ENGINE.md 173), {} at a card that changed zones later in the step (ENGINE.md 175, 206), {} at a card that came into the hand in the step (206)",
        PK_HAND_LOOKS.load(Ordering::Relaxed),
        PK_ENDED_GAME_HAND_LOOKS.load(Ordering::Relaxed),
        PK_LEFT_HAND_LOOKS.load(Ordering::Relaxed),
        PK_ENTERED_HAND_LOOKS.load(Ordering::Relaxed)
    )
}

fn stack_names(g: &Game) -> String {
    let db = g.db();
    let mut v: Vec<String> = g
        .stack()
        .iter()
        .map(|it| match it.kind {
            crate::game::StackKind::Spell { card } => format!("spell {} (own {})", db.def(g.obj(card).def).name, db.def(g.own_def(card)).name),
            crate::game::StackKind::Ability { source, .. } => format!("ability of {}", db.def(g.obj(source.idx).def).name),
        })
        .collect();
    for p in 0..2u8 {
        v.push(format!("P{p} battlefield: {:?}", g.objs().iter().filter(|o| o.zone == crate::Zone::Battlefield && o.controller == p).map(|o| db.def(o.def).name.clone()).collect::<Vec<_>>()));
    }
    v.join("
")
}

pub fn play_match(seed: u64, checks: bool) -> PlayedMatch {
    play_match_stats(seed, checks, &mut HiddenStats::default())
}

fn play_match_stats(seed: u64, checks: bool, stats: &mut HiddenStats) -> PlayedMatch {
    play_match_spec(seed, &match_spec(seed), checks, stats)
}

/// The state after a game's end with one episode per game (ENGINE.md 121): the next game's episode (its
/// sideboarding first); None at the match's end.
fn next_episode(g: &crate::az::MtgAz, seed: u64) -> Option<crate::az::MtgAz> {
    use zero_search::game::SearchGame;
    if !g.m.between {
        return None;
    }
    match g.next_episode(None).unwrap_or_else(|e| panic!("match {seed}: {e}")) {
        zero_search::game::Next::Continue(n) => Some(n),
        zero_search::game::Next::Over(_) => panic!("match {seed}: between games, but next_episode ended the match"),
    }
}

thread_local! {
    /// The spec of the match being played when it deals decks (ENGINE.md 154), for `check_constructed_world`.
    static DEALT_SPEC: std::cell::RefCell<Option<crate::az::MtgSpec>> = const { std::cell::RefCell::new(None) };
}

pub fn play_match_spec(seed: u64, sp: &crate::az::MtgSpec, checks: bool, stats: &mut HiddenStats) -> PlayedMatch {
    use crate::az::MtgAz;
    use zero_search::game::SearchGame;
    let mut g = MtgAz::new_game(sp, 2, seed).unwrap_or_else(|e| panic!("match {seed}: {e}"));
    // (ENGINE.md 176: a metagame match's pairing is its seed's; None for every other spec)
    assert_eq!(g.m.pairing, sp.pairing(seed), "match {seed}: the metagame pairing");
    DEALT_SPEC.with(|s| *s.borrow_mut() = sp.dealt_decks.map(|_| sp.clone()));
    let mut dealt_decks = 0u32;
    if let Some(dd) = sp.dealt_decks {
        if dd.per_seat {
            // (ENGINE.md 168.1)
            let ds = crate::az::matches::dealt_seats(seed, dd.p.0);
            assert_eq!([g.m.is_dealt(0), g.m.is_dealt(1)], ds, "match {seed}: the dealt seats are the seed's");
            assert_eq!(g.m.dealt_both, ds[0] && ds[1]);
        } else {
            assert_eq!(g.m.dealt, crate::az::matches::dealt_seat(seed, dd.p.0), "match {seed}: the dealt seat is the seed's");
        }
    } else {
        assert!(!g.m.any_dealt());
    }
    // (ENGINE.md 168.3: the choice table's seats are those whose 75 is its list; ENGINE.md 240: each seat's own table,
    // a metagame seat its pairing deck's)
    for p in 0..2 {
        let want = match (&sp.sideboard_table, &sp.decks) {
            (Some(t), _) => t.fits(&g.m.pools[p]).then(|| t.clone()),
            (None, crate::az::DeckMode::Metagame(mg)) => mg.decks[g.m.pairing.as_ref().unwrap().decks[p] as usize].table.clone(),
            (None, _) => sp.seat_tables[p].clone(),
        };
        assert_eq!(g.m.table[p], want, "match {seed}: P{p}'s choice table");
        if let Some(t) = &g.m.table[p] {
            assert!(t.fits(&g.m.pools[p]), "match {seed}: P{p}'s table is not of its 75");
        }
    }
    // (A post-sideboard match, ENGINE.md 130, starts at the sideboarding before game 2: game 1 drawn, never a
    // draw, its loser sideboarding first (CR 103.1). A dealt loser (ENGINE.md 154.1) is dealt at once - its 60
    // done, no decision of its own - so the winner is the first decider; otherwise the loser decides, nothing done.)
    if sp.start_game == 2 && g.m.dealt_both {
        // (both seats dealt, ENGINE.md 168.1: nobody decides - game 2 has begun with both deals)
        assert!(g.m.post_board && g.m.game_no == 2 && g.sideboarding().is_none() && matches!(g.m.results[0], 1 | 2), "match {seed}: both dealt: game 2 at once");
    } else if sp.start_game == 2 {
        assert!(g.m.post_board && g.m.game_no == 1 && matches!(g.m.results[0], 1 | 2), "match {seed}: a post-sideboard start");
        let loser = 2 - g.m.results[0];
        let sb = g.sideboarding().unwrap_or_else(|| panic!("match {seed}: a post-sideboard start without its sideboarding"));
        assert_eq!(sb.order, [loser, 1 - loser], "match {seed}: game 1's loser sideboards first");
        if g.m.is_dealt(loser) {
            assert!(
                sb.turn == 1 && sb.done[loser as usize].is_some() && sb.done[1 - loser as usize].is_none(),
                "match {seed}: game 1's loser P{loser} is the dealt seat: dealt at once (its 60 done), the winner deciding (turn {}, done {:?})",
                sb.turn,
                sb.done.iter().map(|d| d.is_some()).collect::<Vec<_>>()
            );
            assert_eq!(sb.decider(), 1 - loser, "match {seed}: game 1's loser is dealt: the winner decides first");
        } else {
            assert!(
                sb.turn == 0 && sb.done.iter().all(|d| d.is_none()),
                "match {seed}: game 1's loser P{loser} decides first, nothing done yet (turn {}, done {:?})",
                sb.turn,
                sb.done.iter().map(|d| d.is_some()).collect::<Vec<_>>()
            );
            assert_eq!(sb.decider(), loser, "match {seed}: game 1's loser sideboards first");
        }
    }
    let mut keeper_decisions = 0u32;
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 5);
    let (mut legal, mut legal2, mut keys, mut all_keys, mut actions) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut x1: Vec<f32>;
    let mut x2: Vec<f32>;
    let (mut sideboard_decisions, mut changed_decks, mut sideboardings) = (0u32, 0u32, 0u32);
    let mut decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
    let mut game_no = 1;
    // (ENGINE.md 168.3: a choice table seat's target of the sideboarding in progress, checked when the game begins)
    let mut table_target: [Option<std::sync::Arc<[u8]>>; 2] = [None, None];
    let mut table_decisions = 0u32;
    // (`FUZZ_POOL=hogaak`'s Massacre spells counted: game, stack id; ENGINE.md 220.4)
    let mut massacres_seen: Vec<(u8, u32)> = Vec::new();
    // The rules checks of each game (ENGINE.md 150), after every step.
    let mut mc = MatchCheck::new();
    if checks {
        rules_match_step(&mut mc, &g, &|| format!("match {seed} (its first state)"));
    }
    loop {
        // (one episode per game, ENGINE.md 121: a game's end is the episode's; the match goes on)
        let Some(p) = g.to_move() else {
            match next_episode(&g, seed) {
                Some(n) => {
                    g = n;
                    continue;
                }
                None => break,
            }
        };
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "match {seed}: no legal action");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "match {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        let sb = g.sideboarding().is_some();
        sideboard_decisions += sb as u32;
        // the dealt seat never decides at a sideboarding (ENGINE.md 154)
        assert!(!(sb && g.m.is_dealt(p as u8)), "match {seed}: the dealt seat P{p} decides at a sideboarding");
        // a choice table's seat (ENGINE.md 168.3): the table's action, legal, at every decision of its sideboarding (and
        // nowhere else); it plays it, as zero-core's driver does
        let taught = g.table_action();
        assert_eq!(taught.is_some(), sb && g.m.table[p].is_some(), "match {seed}: P{p}'s table action");
        if let Some(t) = taught {
            assert!(legal.contains(&t), "match {seed}: the table's action {t} is not legal at {legal:?}");
            let s = g.sideboarding().unwrap();
            let lead = g.m.wins[p] as i8 - g.m.wins[1 - p] as i8;
            let want = g.m.table[p].as_ref().unwrap().choice(g.m.game_no + 1, s.turn == 0, lead, g.m.is_dealt(1 - p as u8)).clone();
            assert!(table_target[p].as_ref().is_none_or(|x| *x == want), "match {seed}: P{p}'s table target moved in a sideboarding");
            table_target[p] = Some(want);
            table_decisions += 1;
        }
        // a seat that keeps its deck (ENGINE.md 130): PASS, its only option
        if sb && g.m.keep[p] {
            assert_eq!(legal, [crate::act::PASS], "match {seed}: P{p} keeps its deck");
            keeper_decisions += 1;
        }
        let a = taught.unwrap_or_else(|| legal[rng.below(legal.len() as u32) as usize]);
        sideboardings += (sb && a == crate::act::PASS) as u32;
        if checks {
            // The decider's encoding is finite and bounded (ENGINE.md 46).
            az_row_checked(&g, p, &|| format!("match {seed} step {}", actions.len()));
        }
        if checks && rng.below(checkpoint_every()) == 0 {
            // A clone continues identically.
            let (mut c1, mut c2) = (g.clone(), g.clone());
            let (mut k1, mut k2) = (Vec::new(), Vec::new());
            c1.step(a, &mut k1).unwrap();
            c2.step(a, &mut k2).unwrap();
            assert_eq!((c1.g.fingerprint(), &k1, c1.m.game_no), (c2.g.fingerprint(), &k2, c2.m.game_no), "match {seed}: clones diverged");
            // determinize keeps the decider's actions, both viewers' view
            // and encoding, and samples the opponent's hidden pool and deck
            // consistently (ENGINE.md 36.11).
            for viewer in [p, 1 - p] {
                let mut d = g.clone();
                d.determinize(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()));
                d.legal_actions(&mut legal2);
                if viewer == p {
                    assert_eq!(legal, legal2, "match {seed}: determinize changed the decider's actions");
                } else {
                    assert_eq!(d.to_move(), Some(p), "match {seed}: determinize changed the decider: {:?} -> {:?}", g.pending_decision(), d.pending_decision());
                }
                x1 = crate::az::tail_encode::Row::of_az_v8(&g, viewer).flat();
                x2 = az_row_checked(&d, viewer, &|| format!("match {seed}: a world determinized for P{viewer}"));
                if let Some(i) = (0..x1.len()).find(|&i| x1[i] != x2[i]) {
                    panic!("match {seed}: determinize changed the encoding of P{viewer} (mover P{p}) at {i}: {} -> {}", x1[i], x2[i]);
                }
                if g.m.constructed_bo3 {
                    check_constructed_world(&g, &d, viewer as u8, seed, stats);
                    check_constructed_tilted(&g, viewer as u8, seed, actions.len(), stats);
                } else {
                    check_hidden_pool(&g, &d, viewer as u8, seed, stats);
                }
                if viewer == p && rng.below(4) == 0 {
                    check_match_information_set(&g, viewer, seed, &mut rng);
                }
                if let Some(t) = belief_tilt() {
                    // the belief-weighted deal (ENGINE.md 69): the hidden pool sampled first, then the
                    // hand by the weights; every check of the uniform world's (its own generator)
                    let mut brng = Rng::new(seed ^ (actions.len() as u64).wrapping_mul(0x9E37) ^ viewer as u64);
                    let pred = random_weights(&mut brng, 1.0).iter().map(|&x| x as f32).collect::<Vec<f32>>();
                    let s: f32 = pred.iter().sum();
                    let b = zero_search::game::Belief { pred: pred.iter().map(|x| x / s).collect(), tilt: t, clip: 1e6, tilts: Vec::new() };
                    let mut dw = g.clone();
                    dw.determinize_belief(viewer, &mut zero_search::rng::Rng::new(brng.next_u64()), &b);
                    dw.legal_actions(&mut legal2);
                    if viewer == p {
                        assert_eq!(legal, legal2, "match {seed}: a weighted deal changed the decider's actions");
                    }
                    x2 = crate::az::tail_encode::Row::of_az_v8(&dw, viewer).flat();
                    if let Some(i) = (0..x1.len()).find(|&i| x1[i] != x2[i]) {
                        panic!("match {seed}: a weighted deal changed the encoding of P{viewer} at {i}");
                    }
                    if g.m.constructed_bo3 {
                        check_constructed_world(&g, &dw, viewer as u8, seed, &mut HiddenStats::default());
                    } else {
                        check_hidden_pool(&g, &dw, viewer as u8, seed, &mut HiddenStats::default());
                    }
                }
                // The world plays on.
                let mut kd = Vec::new();
                let mut wc = MatchCheck::new();
                rules_match_step(&mut wc, &d, &|| format!("match {seed} (determinized for P{viewer})"));
                for _ in 0..30 {
                    if d.to_move().is_none() {
                        break;
                    }
                    d.legal_actions(&mut legal2);
                    let a2 = legal2[rng.below(legal2.len() as u32) as usize];
                    kd.clear();
                    // (the viewer's hand before the step: the step may end the game and begin the next - both seats
                    // dealt, ENGINE.md 168.1 - and a look of that step at a card of this hand is checked against it, 173)
                    let known = viewer_hand(&d, viewer as u8);
                    d.step(a2, &mut kd).unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    PK_MATCH_KEYS.fetch_add(
                        check_private_keys_masked_since(&kd, viewer as u8, &d.g, &known, &|| format!("match {seed}: a world determinized for P{viewer}")),
                        Ordering::Relaxed,
                    );
                    if d.sideboarding().is_none() && !d.match_over() {
                        d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    }
                    rules_match_step(&mut wc, &d, &|| format!("match {seed} (determinized for P{viewer}, played on)"));
                }
                rules_add(&wc.total());
                check_world_private_keys(&g, viewer, seed, actions.len());
            }
        }
        // (`FUZZ_POOL=hogaak`, ENGINE.md 220.4: a Massacre on top of the stack resolved by this pass, the game going on)
        let massacre_top = (hogaak_mode() && g.sideboarding().is_none()).then(|| hogaak_b_massacre_on_top(&g.g, a)).flatten().map(|id| (id, g.m.game_no));
        // (the forced find in best-of-three, ENGINE.md 240: its records of the game's resolutions, each counted once)
        let (ff_game, ff_seen) = (g.m.game_no, g.g.forced_finds().len());
        g.step(a, &mut keys).unwrap_or_else(|e| panic!("match {seed}: {e}"));
        if g.m.game_no == ff_game {
            for r in &g.g.forced_finds()[ff_seen.min(g.g.forced_finds().len())..] {
                PAIR_FF_RES.fetch_add(1, Ordering::Relaxed);
                if r.class == crate::forced_find::FfClass::Forced {
                    PAIR_FF_FORCED.fetch_add(1, Ordering::Relaxed);
                    if ff_game >= 2 {
                        PAIR_FF_FORCED_LATER.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        count_dropped_loop_records(&g.g, true);
        if let Some((id, game)) = massacre_top {
            if g.m.game_no == game && g.sideboarding().is_none() && !g.g.stack().iter().any(|s| s.id == id) {
                HOGAAK_MASSACRE_MATCHES[usize::from(sp.start_game == 2)].fetch_add(1, Ordering::Relaxed);
            }
        }
        if hogaak_mode() && g.sideboarding().is_none() {
            for it in g.g.stack() {
                if let crate::game::StackKind::Spell { card } = it.kind {
                    if g.g.name(card) == "Massacre" && !massacres_seen.contains(&(g.m.game_no, it.id)) {
                        massacres_seen.push((g.m.game_no, it.id));
                        HOGAAK_MASSACRE_MATCHES[2 + usize::from(sp.start_game == 2)].fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        all_keys.append(&mut keys);
        actions.push(a);
        if g.sideboarding().is_none() && !g.match_over() {
            g.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} step {}: {e}", actions.len()));
        }
        if checks {
            rules_match_step(&mut mc, &g, &|| format!("match {seed} step {} (rerun: FUZZ_FIRST={seed} FUZZ_MATCHES=1 FUZZ_LOG=1)", actions.len()));
        }
        if g.m.game_no != game_no && g.sideboarding().is_none() {
            // A later game: its decks are legal and from the pools; its
            // chooser is the previous game's loser, or after a draw its
            // chooser (CR 103.1).
            game_no = g.m.game_no;
            let i = (game_no - 2) as usize;
            let want = match g.m.results[i] {
                3 => g.m.choosers[i],
                w => 1 - (w - 1),
            };
            assert_eq!(g.g.turn().chooser, want, "match {seed}: the chooser of game {game_no}");
            for q in 0..2u8 {
                let pool = &g.m.pools[q as usize];
                let deck = g.m.deck(q);
                // (Every fuzzed deck has 40 cards, and a pool's deck keeps
                // them: ENGINE.md 42; a constructed best-of-three's 60, ENGINE.md 120.)
                let size = if g.m.constructed_bo3 { 60 } else { 40 };
                assert_eq!((pool.min_deck, pool.max_deck, deck.len()), (size, size, size), "match {seed}: {} cards", deck.len());
                if g.m.constructed_bo3 {
                    check_legal_60(&g.m, q as usize, &g.m.decks[q as usize], &|| format!("match {seed} game {game_no}: P{q}'s deck"));
                    assert_eq!(g.g.sideboard_size(q), pool.list.iter().map(|e| e.1 as usize).sum::<usize>() - 60);
                }
                for (k, &(_, n)) in pool.list.iter().enumerate() {
                    assert!(g.m.decks[q as usize][k] <= n.max(decks[q as usize][k]));
                }
                // a choice table's 60 (ENGINE.md 168.3): exactly its context's
                if let Some(t) = table_target[q as usize].take() {
                    assert_eq!(&g.m.decks[q as usize][..], &t[..], "match {seed} game {game_no}: P{q} followed the table to another 60");
                }
                // a dealt 60 (ENGINE.md 154): its swaps against the registered main within the spec's range
                if g.m.is_dealt(q) {
                    let (main, (lo, hi)) = dealt_main(sp, q as usize, seed);
                    let n = swaps_vs(&main, &deck);
                    assert!((lo..=hi).contains(&n), "match {seed} game {game_no}: P{q}'s dealt 60 of {n} swaps (range {lo}..={hi})");
                    dealt_decks += 1;
                }
                changed_decks += (g.m.decks[q as usize] != decks[q as usize]) as u32;
            }
            decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        }
        assert!(actions.len() < 300_000, "match {seed}: does not end");
    }
    rules_add(&mc.total());
    let m = &g.m;
    assert!(g.match_over());
    // (An aborted game aborts the match, ENGINE.md 100.7: no result.)
    let aborted = g.match_aborted();
    if !aborted {
        assert!(m.wins.contains(&2) || m.game_no == 3, "match {seed}: ended early: {:?}", m.results);
        assert_eq!(m.wins[0] + m.wins[1] + m.draws, m.game_no);
        let r = g.result();
        assert!((r[0] + r[1] - 1.0).abs() < 1e-6);
    }
    PlayedMatch {
        aborted,
        actions,
        keys: all_keys,
        fingerprint: g.g.fingerprint(),
        results: m.results,
        games: m.game_no,
        sideboard_decisions,
        sideboardings,
        changed_decks,
        keeper_decisions,
        dealt: m.any_dealt(),
        dealt_decks,
        table_decisions,
    }
}

/// Why the matches of the mode the `FUZZ_*` settings choose are not played (a games-only pool), as
/// `random_matches` prints it; None: they are.
pub fn matches_skipped() -> Option<&'static str> {
    if constructed_mode() && !constructed_bo3_mode() {
        return Some("FUZZ_POOL=dd_a / dd_b / dd_c / doomsday / sneak_show / sns / hogaak_a / hogaak_b / hogaak_c / lands_b / names / delver_sb / meta_wheel: games only (constructed best-of-three is FUZZ_POOL=delver's, cross's and meta's)");
    }
    if eoe_mode() {
        return Some("FUZZ_POOL=eoe: games only (sealed matches come with milestone S2)");
    }
    if sealed_mode() {
        return Some("FUZZ_POOL=sealed: games only (sealed is best-of-one: ENGINE.md 101)");
    }
    if hidden_decks_mode() {
        return Some("FUZZ_HIDDEN_DECKS: games only (hidden decklists are best-of-one)");
    }
    None
}

/// `fuzz_random_matches` (tests/fuzz.rs; the farm's `*-matches` modes): matches `first..first + n`
/// (module docs), then the chunk's report and checks; a games-only pool prints why and plays none. A
/// match that panics ends the run there ([`Run::Failed`]), as in [`random_games`].
pub fn random_matches(first: u64, n: u64, hooks: &mut dyn Hooks) -> Run {
    if let Some(why) = matches_skipped() {
        println!("{why}");
        // (a sharded run's positive evidence that this pool plays no matches: ENGINE.md 167)
        println!("FUZZ_TOTALS {}", serde_json::json!({"kind": "matches", "first": first, "n": n, "skipped": why}));
        return Run::Done;
    }
    let (mut games_played, mut draws, mut decisions, mut sb, mut changed, mut three) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut sideboardings = 0u64;
    let mut aborted = 0u64;
    let (mut dealt_matches, mut dealt_decks, mut table_decisions) = (0u64, 0u64, 0u64);
    let mut stats = HiddenStats::default();
    let (mut meta_cells, mut meta_steps, mut meta_worlds) = ([0u64; 9], 0u64, 0u64);
    for seed in first..first + n {
        if !hooks.before(seed) {
            return Run::Stopped { at: seed };
        }
        if skipped(seed) {
            hooks.passed(seed, &SeedOutcome::not_played("skipped (FUZZ_SKIP)"));
            continue;
        }
        // (`FUZZ_POOL=meta`, ENGINE.md 176: the match's pairing check first)
        let p = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let m = meta_mode().then(|| meta_lockstep(&meta_match_json(seed, false), seed));
            (play_match_stats(seed, true, &mut stats), m)
        })) {
            Ok((p, m)) => {
                if let Some(([i, j], st, w)) = m {
                    meta_cells[(i * 3 + j) as usize] += 1;
                    meta_steps += st;
                    meta_worlds += w;
                }
                p
            }
            Err(payload) => return Run::Failed { seed, payload },
        };
        games_played += p.games as u64;
        draws += p.results.iter().filter(|&&r| r == 3).count() as u64;
        decisions += p.actions.len() as u64;
        sb += p.sideboard_decisions as u64;
        sideboardings += p.sideboardings as u64;
        changed += p.changed_decks as u64;
        three += (p.games == 3) as u64;
        aborted += p.aborted as u64;
        dealt_matches += p.dealt as u64;
        table_decisions += p.table_decisions as u64;
        dealt_decks += p.dealt_decks as u64;
        hooks.passed(seed, &SeedOutcome { fingerprint: p.fingerprint, result: format!("{:?}", p.results), turns: p.games as u64, decisions: p.actions.len() as u64 });
    }
    if dealt_mode() || pair_mode() {
        println!("dealt decks (ENGINE.md 154): {dealt_matches} of {n} matches with a dealt seat, {dealt_decks} dealt 60s checked");
        if per_seat_mode() || pair_mode() {
            println!("choice tables (ENGINE.md 168.3): {table_decisions} table decisions played, each table 60 reached");
        }
    }
    if pair_mode() {
        println!(
            "FUZZ_POOL=meta_pair (ENGINE.md 240): the forced find in best-of-three - {} Doomsday resolutions, {} forced ({} in a game 2 or 3)",
            PAIR_FF_RES.load(Ordering::Relaxed),
            PAIR_FF_FORCED.load(Ordering::Relaxed),
            PAIR_FF_FORCED_LATER.load(Ordering::Relaxed)
        );
    }
    println!(
        "{n} matches: {games_played} games ({three} matches of three, {draws} drawn games, {aborted} aborted matches), mean decisions {:.1}, \
         sideboarding decisions {sb}, decks changed {changed}; hidden pools: {} worlds, first picks resampled in {}, \
         decks in {}",
        decisions as f64 / n as f64,
        stats.worlds,
        stats.first_picks_changed,
        stats.decks_changed
    );
    println!(
        "sideboardings {sideboardings} ({:.2} decisions per sideboarding)",
        sb as f64 / sideboardings.max(1) as f64
    );
    println!("{}", rules_report());
    rules_verdict("fuzz_random_matches");
    // (ENGINE.md 141: the worlds dealt while sideboarding play their next game with the observer)
    println!("{}", private_keys_report());
    // The information-set walks compared states (ENGINE.md 148).
    let (w, s) = (MATCH_IS_WALKS.load(Ordering::Relaxed), MATCH_IS_STEPS.load(Ordering::Relaxed));
    println!("match information sets (ENGINE.md 148): {w} walks, {s} states compared after a common action");
    if cross_mode() || hogaak_mode() {
        let ps: &[(usize, usize)] = if hogaak_mode() { &HOGAAK_PAIRS } else { &CROSS_PAIRS };
        let pairs: Vec<String> = ps.iter().map(|&(a, b)| format!("{} v {}", REAL_75_NAMES[a], REAL_75_NAMES[b])).collect();
        println!("FUZZ_POOL={} matches: {} by seed % {}; the 75s: {}", if hogaak_mode() { "hogaak" } else { "cross" }, pairs.join(", "), ps.len(), cross_75_report());
    }
    if hogaak_mode() {
        let l = |k: usize| HOGAAK_MASSACRE_MATCHES[k].load(Ordering::Relaxed);
        println!("FUZZ_POOL=hogaak matches: Massacre cast {}, resolved {}", l(2), l(0));
    }
    if constructed_bo3_mode() {
        // Constructed best-of-three (ENGINE.md 120): no first picks; the later games' decks hidden.
        println!(
            "constructed: {} worlds, the opponent's 60 hidden in {} ({} with another 60 than the real one), {} first sideboarders' choices dealt, {} deck-tilted worlds from the K cached decks",
            stats.worlds, stats.hidden, stats.hidden_changed, stats.choices_dealt, stats.tilted
        );
    }
    // the end-of-run checks over the chunk's counters (over a chunk of matches; a single match's rerun, `FUZZ_MATCHES=1`,
    // passes when the match completes: ENGINE.md 51; a shard's are left to the merged totals: ENGINE.md 167)
    let mut t = Totals::default();
    for (k, v) in [
        ("dealt_matches", dealt_matches),
        ("dealt_decks", dealt_decks),
        ("table_decisions", table_decisions),
        ("pk_new_games", PK_NEW_GAMES.load(Ordering::Relaxed)),
        ("pk_new_game_keys", PK_NEW_GAME_KEYS.load(Ordering::Relaxed)),
        ("ff_res", PAIR_FF_RES.load(Ordering::Relaxed)),
        ("ff_forced", PAIR_FF_FORCED.load(Ordering::Relaxed)),
        ("ff_forced_later", PAIR_FF_FORCED_LATER.load(Ordering::Relaxed)),
        ("sb", sb),
        ("changed", changed),
        ("three", three),
        ("match_is_walks", w),
        ("match_is_steps", s),
        ("match_loopdrop_checked", MATCH_LOOPDROP_CHECKED.load(Ordering::Relaxed)),
        ("match_loopdrop_recur", MATCH_LOOPDROP_RECUR.load(Ordering::Relaxed)),
        ("worlds", stats.worlds),
        ("first_picks_changed", stats.first_picks_changed),
        ("decks_changed", stats.decks_changed),
        ("hidden", stats.hidden),
        ("hidden_changed", stats.hidden_changed),
        ("choices_dealt", stats.choices_dealt),
        ("tilted", stats.tilted),
        ("hogaak_massacre", HOGAAK_MASSACRE_MATCHES[0].load(Ordering::Relaxed)),
    ] {
        t.set(k, v);
    }
    if meta_mode() {
        println!("FUZZ_POOL=meta (ENGINE.md 176): matches per cell (p0 x p1) {meta_cells:?}; lockstep with the pairings' fixed matches: {meta_steps} steps, {meta_worlds} worlds");
    }
    meta_totals(&mut t, "m", &meta_cells, meta_steps, meta_worlds);
    end_of_run("matches", first, n, &t, check_matches);
    if constructed_bo3_mode() {
        return post_sideboard_matches(first, n, hooks);
    }
    Run::Done
}

/// `FUZZ_POOL=delver`'s post-sideboard matches (ENGINE.md 130), the same seeds as its matches: `"start_game": 2`
/// with one episode per game, one match in three a seat keeping its deck; every check of `play_match_spec` (the
/// constructed worlds, the deck-tilted deals, the information sets, the later games' decks and choosers). With
/// `FUZZ_POOL=cross` (ENGINE.md 148) its matches' pairings of the real 75s, the same way.
/// (The farm: a match that panics ends the run there, [`Run::Failed`]; [`Hooks::before`] is asked before each match
/// and may stop the run, [`Run::Stopped`]; the matches before passed these seeds, so none is passed again.)
fn post_sideboard_matches(first: u64, n: u64, hooks: &mut dyn Hooks) -> Run {
    let (mut games, mut draws, mut decisions, mut sb, mut changed, mut three, mut aborted, mut kept) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut dealt_matches, mut dealt_decks, mut table_decisions) = (0u64, 0u64, 0u64);
    let mut stats = HiddenStats::default();
    let (mut meta_cells, mut meta_steps, mut meta_worlds) = ([0u64; 9], 0u64, 0u64);
    let new_games0 = PK_NEW_GAMES.load(Ordering::Relaxed);
    for seed in first..first + n {
        if !hooks.before(seed) {
            return Run::Stopped { at: seed };
        }
        if skipped(seed) {
            continue;
        }
        let sp = delver_post_spec(seed);
        // (`FUZZ_POOL=meta`, ENGINE.md 176: the match's pairing check first)
        let p = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let m = meta_mode().then(|| meta_lockstep(&meta_match_json(seed, true), seed));
            (play_match_spec(seed, &sp, true, &mut stats), m)
        })) {
            Ok((p, m)) => {
                if let Some(([i, j], st, w)) = m {
                    meta_cells[(i * 3 + j) as usize] += 1;
                    meta_steps += st;
                    meta_worlds += w;
                }
                p
            }
            Err(payload) => return Run::Failed { seed, payload },
        };
        // (game 1 is not played: the games after it)
        games += p.games as u64 - 1;
        draws += p.results[1..].iter().filter(|&&r| r == 3).count() as u64;
        decisions += p.actions.len() as u64;
        sb += p.sideboard_decisions as u64;
        changed += p.changed_decks as u64;
        three += (p.games == 3) as u64;
        aborted += p.aborted as u64;
        kept += p.keeper_decisions as u64;
        dealt_matches += p.dealt as u64;
        table_decisions += p.table_decisions as u64;
        dealt_decks += p.dealt_decks as u64;
    }
    if dealt_mode() || pair_mode() {
        // (dealt decks refuse a keeper, ENGINE.md 154: the dealt seats instead)
        println!("post-sideboard dealt decks: {dealt_matches} of {n} matches with a dealt seat, {dealt_decks} dealt 60s checked");
        if per_seat_mode() || pair_mode() {
            println!("post-sideboard choice tables (ENGINE.md 168.3): {table_decisions} table decisions played, each table 60 reached");
        }
    }
    // (a keeper's decisions, or with dealt decks - which refuse a keeper - the dealt 60s)
    let kept_or_dealt = if dealt_mode() { dealt_decks } else { kept };
    println!(
        "{n} post-sideboard matches (start_game 2): {games} games played ({three} matches of three, {draws} drawn games, {aborted} aborted), mean decisions {:.1}, \
         sideboarding decisions {sb} ({kept} of a seat that keeps its deck), decks changed {changed}; constructed: {} worlds, the opponent's 60 hidden in {} \
         ({} with another 60 than the real one), {} first sideboarders' choices dealt, {} deck-tilted worlds",
        decisions as f64 / n as f64,
        stats.worlds,
        stats.hidden,
        stats.hidden_changed,
        stats.choices_dealt,
        stats.tilted
    );
    println!("{} (with the matches before)", rules_report());
    rules_verdict("fuzz_post_sideboard_matches");
    // (ENGINE.md 141: every game 2 and 3 of these matches starts after a sideboarding, the worlds' too)
    println!("{} (cumulative)", private_keys_report());
    if hogaak_mode() {
        let l = |k: usize| HOGAAK_MASSACRE_MATCHES[k].load(Ordering::Relaxed);
        println!("FUZZ_POOL=hogaak post-sideboard matches: Massacre cast {}, resolved {}", l(3), l(1));
    }
    let mut t = Totals::default();
    for (k, v) in [
        ("dealt_matches", dealt_matches),
        ("dealt_decks", dealt_decks),
        ("table_decisions", table_decisions),
        ("sb", sb),
        ("changed", changed),
        ("three", three),
        ("kept_or_dealt", kept_or_dealt),
        ("choices_dealt", stats.choices_dealt),
        ("tilted", stats.tilted),
        ("pk_new_games", PK_NEW_GAMES.load(Ordering::Relaxed) - new_games0),
    ] {
        t.set(k, v);
    }
    if meta_mode() {
        println!("FUZZ_POOL=meta (ENGINE.md 176): post-sideboard matches per cell (the mirrors) {meta_cells:?}; lockstep: {meta_steps} steps, {meta_worlds} worlds");
    }
    meta_totals(&mut t, "p", &meta_cells, meta_steps, meta_worlds);
    end_of_run("post", first, n, &t, check_post);
    Run::Done
}

/// Replays match `seed` of `sp` from its actions (`next_episode` at each game's end with one episode per game):
/// its fingerprint, results and chance keys.
pub fn replay_match(seed: u64, sp: &crate::az::MtgSpec, actions: &[u16]) -> (u64, [u8; 3], Vec<u64>) {
    use crate::az::MtgAz;
    use zero_search::game::SearchGame;
    let mut g = MtgAz::new_game(sp, 2, seed).unwrap();
    let mut keys = Vec::new();
    for &x in actions {
        while g.to_move().is_none() {
            g = next_episode(&g, seed).expect("the match goes on");
        }
        g.step(x, &mut keys).unwrap();
    }
    (g.g.fingerprint(), g.m.results, keys)
}

// ---- FUZZ_HIDDEN_DECKS=1: hidden decklists (ENGINE.md 102) ----

/// `FUZZ_HIDDEN_DECKS=1` (module docs): the games are played through `MtgAz` with hidden
/// decklists.
pub fn hidden_decks_mode() -> bool {
    super::env::var("FUZZ_HIDDEN_DECKS").is_ok_and(|s| s == "1")
}

/// Hidden-decklist worlds checked, those whose opponent deck is not the real one, steps the
/// information-set check took in step, turns that reached the long-turn rule.
static HD_WORLDS: AtomicU64 = AtomicU64::new(0);
static HD_OTHER_DECK: AtomicU64 = AtomicU64::new(0);
static HD_STEPS: AtomicU64 = AtomicU64::new(0);
static HD_LONG_TURNS: AtomicU64 = AtomicU64::new(0);
/// `FUZZ_DECK_CHOICE` (ENGINE.md 107.2): deck choices played, worlds checked during them.
static DC_CHOICES: AtomicU64 = AtomicU64::new(0);
/// `FUZZ_DECK_CHOICE_BY`: seats whose deck the evaluator chose, and exploring seats.
static DC_EVAL_CHOSEN: AtomicU64 = AtomicU64::new(0);
static DC_EVAL_EXPLORE: AtomicU64 = AtomicU64::new(0);
static DC_WORLDS: AtomicU64 = AtomicU64::new(0);

/// `FUZZ_DECK_CHOICE=k`: the candidates per seat (module docs), or None.
fn deck_choice_fuzz() -> Option<u8> {
    super::env::var("FUZZ_DECK_CHOICE").ok().map(|v| v.parse().expect("FUZZ_DECK_CHOICE: the candidates per seat (2..=32)"))
}

/// The deck-choice spec of game `seed` (`FUZZ_DECK_CHOICE`): `sealed_spec`'s with the choice.
fn choice_spec(seed: u64, k: u8) -> &'static crate::az::MtgSpec {
    static SPECS: std::sync::OnceLock<[crate::az::MtgSpec; 2]> = std::sync::OnceLock::new();
    let specs = SPECS.get_or_init(|| {
        [0u64, 1].map(|s| {
            let mut v: serde_json::Value = serde_json::from_str(&format!(r#"{{"sealed": {}}}"#, sealed_spec(s).sealed.as_deref().unwrap().to_json())).unwrap();
            v["sealed"]["deck_choice"] = serde_json::json!({"k": k, "explore": 0.1});
            // (FUZZ_DECK_CHOICE_BY, ENGINE.md 112: the deck choice by an evaluator file)
            if let Ok(path) = super::env::var("FUZZ_DECK_CHOICE_BY") {
                v["sealed"]["deck_choice"]["by"] = serde_json::json!("evaluator");
                v["sealed"]["deck_choice"]["evaluator"] = serde_json::json!(path);
            }
            crate::az::MtgSpec::parse(&v.to_string()).unwrap()
        })
    });
    &specs[(seed % 2) as usize]
}

/// `FUZZ_DECK_CHOICE_BY` (module docs): each built seat's deck is its explore pick or the evaluator's best
/// candidate, recomputed from the spec; its prior chooses as it does. Nothing without the evaluator.
fn check_evaluator_choice(a: &crate::az::MtgAz, spec: &crate::az::MtgSpec, seed: u64) {
    let sl = spec.sealed.as_deref().unwrap();
    let Some(dc) = sl.deck_choice.as_ref() else { return };
    let Some(e) = dc.evaluator() else { return };
    let ss = spec.sealed_seed(seed);
    assert!(a.deck_choice().is_none(), "seed {seed}: the deck choice by an evaluator is no decision");
    for p in 0..2u8 {
        let (cands, _) = sl.candidates(ss, p).unwrap().expect("a built seat's candidates");
        let how = a.m.how[p as usize].expect("how the deck was made");
        let deck = a.m.deck(p);
        match crate::az::sealed::explore_pick(ss, p, dc.explore.0, dc.k) {
            Some(j) => {
                assert_eq!((how.mode, how.candidate, how.evaluator), (crate::az::sealed::Mode::Explore, Some(j), None), "seed {seed}: P{p} explores");
                assert_eq!(deck, cands[j as usize].deck, "seed {seed}: P{p}'s explore pick");
                DC_EVAL_EXPLORE.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                let scores: Vec<f64> = cands.iter().map(|c| e.scorer.score(&c.deck).unwrap()).collect();
                let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let j = scores.iter().position(|&x| x == best).unwrap();
                assert_eq!((how.mode, how.candidate, how.evaluator), (crate::az::sealed::Mode::Chosen, Some(j as u8), Some(e.scorer.sha256)), "seed {seed}: P{p} chose");
                assert_eq!(how.score.map(f64::from_bits), Some(best), "seed {seed}: P{p}'s score");
                assert_eq!(deck, cands[j].deck, "seed {seed}: P{p}'s chosen deck");
                DC_EVAL_CHOSEN.fetch_add(1, Ordering::Relaxed);
            }
        }
        let prior = a.m.deck_priors[p as usize].as_ref().expect("a prior");
        assert_eq!(prior.name(), "sealed+evaluator", "seed {seed}: P{p}'s prior chooses as it does");
    }
}

/// The deck choice of a `FUZZ_DECK_CHOICE` game (module docs), played until the game begins.
fn play_deck_choice(a: &mut crate::az::MtgAz, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let mut zr = zero_search::rng::Rng::new(seed ^ 0xDEC0_C401);
    let (mut b0, mut b1) = (Vec::new(), Vec::new());
    let mut keys = Vec::new();
    while a.deck_choice().is_some() {
        assert!(a.value_choice(), "seed {seed}: a deck choice is a value choice");
        let p = a.to_move().unwrap() as u8;
        let legal = az_legal(a);
        assert!(legal.len() >= 2 && legal.iter().enumerate().all(|(j, &x)| x == crate::act::NUMBER + j as u16), "seed {seed}: the choice's options {legal:?}");
        for viewer in [p, 1 - p] {
            for _ in 0..2 {
                let mut w = a.clone();
                w.determinize(viewer as usize, &mut zr);
                assert_eq!((w.to_move(), az_legal(&w)), (Some(p as usize), legal.clone()), "seed {seed}: a world of P{viewer} changed the choice");
                az_encode(a, viewer, &mut b0);
                az_encode(&w, viewer, &mut b1);
                assert!(b0 == b1, "seed {seed}: a world of P{viewer} changed its encoding during the deck choice");
                let o = 1 - viewer;
                assert_eq!(w.m.deck(o).len(), 40, "seed {seed}: the dealt deck");
                assert!(w.m.how[o as usize].is_none(), "seed {seed}: the world knows how P{o}'s deck was made");
                if viewer == p {
                    let c = w.deck_choice().unwrap();
                    assert_eq!(c.n, c.turn + 1, "seed {seed}: in the decider's world the other seat has chosen");
                    let x = legal[rng.below(legal.len() as u32) as usize];
                    w.step(x, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
                    assert!(w.deck_choice().is_none(), "seed {seed}: the world's game begins with the decider's choice");
                    w.g.check_invariants().unwrap_or_else(|e| panic!("seed {seed}: the world's game: {e}"));
                    let mut wc = MatchCheck::new();
                    rules_match_step(&mut wc, &w, &|| format!("seed {seed}: a deck choice's world"));
                    for _ in 0..30 {
                        if w.to_move().is_none() {
                            break;
                        }
                        let l = az_legal(&w);
                        w.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
                        rules_match_step(&mut wc, &w, &|| format!("seed {seed}: a deck choice's world, played on"));
                    }
                    rules_add(&wc.total());
                }
                DC_WORLDS.fetch_add(1, Ordering::Relaxed);
            }
            check_world_private_keys(a, viewer as usize, seed, DC_CHOICES.load(Ordering::Relaxed) as usize);
        }
        let x = legal[rng.below(legal.len() as u32) as usize];
        a.step(x, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        DC_CHOICES.fetch_add(1, Ordering::Relaxed);
    }
}

/// `FUZZ_HIDDEN_DECKS`: past this many decisions in one turn the random player passes priority
/// whenever it may (module docs: random play loops on the base's free activations).
const HD_LONG_TURN: u64 = 500;

/// `viewer`'s whole row of match `a` as one vector (`Row::flat`; layout v7, ENGINE.md 125).
fn az_encode(a: &crate::az::MtgAz, viewer: u8, buf: &mut Vec<f32>) {
    *buf = crate::az::tail_encode::Row::of_az_v8(a, viewer as usize).flat();
}

/// `viewer`'s whole row of match `a` as one vector, its values bounded; in a game (not between
/// the games of a match, not the deck choice) the game's row checked (`check_row`).
fn az_row_checked(a: &crate::az::MtgAz, viewer: usize, what: &dyn Fn() -> String) -> Vec<f32> {
    let row = crate::az::tail_encode::Row::of_az_v8(a, viewer);
    let vals: Vec<f32> = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).copied().collect();
    check_bounded(&vals, &|| format!("{} (viewer P{viewer})", what()));
    if a.sideboarding().is_none() && a.deck_choice().is_none() && a.g.pending_decision().is_some() {
        let r = crate::az::tail_encode::Row::of_v8(&a.g, viewer as u8);
        check_row(&a.g, viewer as u8, &r, what);
    }
    row.flat()
}

fn az_legal(a: &crate::az::MtgAz) -> Vec<u16> {
    use zero_search::game::SearchGame;
    let mut l = Vec::new();
    a.legal_actions(&mut l);
    l
}

/// Player `p`'s starting deck's cards (sorted card definitions): their game's cards (`sample::game_cards`: the
/// deck's objects, the two decks first, P0's then P1's, and a revealed companion's) less one copy of the revealed
/// companion. (A companion put into the hand and shuffled away is one of its owner's hidden cards: a deal - the
/// owner's own `determinize`, the other player's redeal of the extras with the deck - may give its identity to a
/// deck object and a deck card's to the companion's object: which of the hidden cards it is, is hidden (fixing it to
/// its object would show where it lies); the deck is the multiset either way. ENGINE.md 140.10: comparing the deck's objects alone failed there.)
fn hd_deck(a: &crate::az::MtgAz, p: u8) -> Vec<u16> {
    let mut v = crate::az::sample::game_cards(a, p);
    if let Some(c) = a.g.player(p).companion_def {
        let i = v.binary_search(&c).unwrap_or_else(|_| panic!("P{p}'s companion is not among their game's cards"));
        v.remove(i);
    }
    v
}

/// The cards `viewer` has seen of the other player's deck (deck cards it knows exactly, its
/// clouds' identities), sorted: every world's deck holds them.
fn hd_shown(a: &crate::az::MtgAz, viewer: u8) -> Vec<u16> {
    let o = 1 - viewer;
    let g = &a.g;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> =
        r.clone().map(|i| crate::CardIdx::new(i as u32)).filter(|&c| g.knows(viewer, c)).map(|c| crate::az::sample::card_of(g, c)).collect();
    for k in g.clouds().iter().filter(|k| k.viewer == viewer && k.owner == o) {
        v.extend(k.defs.iter().map(|&d| g.db().def(d).card_def(d)));
    }
    v.sort_unstable();
    // (A cloud's identity may be a card of theirs outside their deck, hidden from the viewer: a
    // token in a hidden zone, a companion put into the hand. Not a deck card. Hidden wherever it
    // is: in a hidden zone, or a face-down permanent or spell (CR 708.2) - `Game::knows` is false
    // for those alone, when the viewer does not know them. ENGINE.md 234: the hidden zones listed
    // here before missed a companion shuffled away and then manifested face down.)
    for (i, ob) in g.objs().iter().enumerate() {
        let c = crate::CardIdx::new(i as u32);
        if ob.owner == o && !r.contains(&i) && !g.knows(viewer, c) {
            if let Ok(at) = v.binary_search(&crate::az::sample::card_of(g, c)) {
                v.remove(at);
            }
        }
    }
    v
}

/// The first card of sorted multiset `cards` that sorted multiset `deck` does not hold (a card twice in `cards`,
/// once in `deck`: its second copy), or None.
fn hd_lacks(deck: &[u16], cards: &[u16]) -> Option<u16> {
    let mut left = deck.to_vec();
    cards.iter().copied().find(|x| match left.binary_search(x) {
        Ok(i) => {
            left.remove(i);
            false
        }
        Err(_) => true,
    })
}

/// One game of `FUZZ_HIDDEN_DECKS` (module docs): the mode's decks, played through `MtgAz` with
/// hidden decklists (the table's `TablePrior` for both decks), with the checks of `play` and, at
/// each checkpoint, `check_determinize_hidden`.
pub fn play_hidden(seed: u64) -> Played {
    use zero_search::game::SearchGame;
    // (Show and Tell's face-down pick reaches a hidden deck's request, which only the constructed prior
    // conditions on: the table's refuses it, ENGINE.md 129.3. Its decks play constructed best-of-three.)
    // (`FUZZ_POOL=meta`: the metagame refuses hidden decklists, ENGINE.md 176)
    assert!(!meta_mode(), "FUZZ_HIDDEN_DECKS=1 with FUZZ_POOL=meta: the metagame's 75s are open decklists (the spec refuses hidden_decklists, ENGINE.md 176)");
    assert!(
        !(sns_mode() || delver_sb_mode() || cross_mode() || hogaak_mode()),
        "FUZZ_HIDDEN_DECKS=1 with a pool that casts Show and Tell (sneak_show, sns, delver_sb, cross, hogaak): the table's prior cannot condition a deck on its face-down pick (ENGINE.md 129.3)"
    );
    let [(d0, s0), (d1, s1)] = decks(seed);
    let log = super::env::var("FUZZ_LOG").is_ok();
    let gspec = GameSpec { log, hidden_decks: true, hidden_decklists: true, ..GameSpec::default() };
    let seat = |deck: Vec<u16>, sideboard: Vec<u16>| crate::az::Seat { deck, sideboard, first_picks: Vec::new(), elsewhere: Vec::new() };
    // (FUZZ_POOL=sealed: the sealed prior of the game's spec, ENGINE.md 102.6; else the table's stub.)
    let prior: std::sync::Arc<dyn crate::az::hidden_decks::DeckPrior> = match sealed_spec(seed).sealed.as_deref().filter(|_| sealed_mode()) {
        Some(sl) => std::sync::Arc::new(crate::az::hidden_decks::SealedPrior::of_spec(sl)),
        None => crate::az::hidden_decks::TablePrior::of(game_db()),
    };
    // (FUZZ_DECK_CHOICE, ENGINE.md 107.2: the spec's game with its deck choice, made by `MtgAz::new`)
    let choice = deck_choice_fuzz().filter(|_| sealed_mode());
    let mut a = match choice {
        Some(k) => crate::az::MtgAz::new(choice_spec(seed, k), seed).unwrap_or_else(|e| panic!("seed {seed}: {e}")),
        None => crate::az::MtgAz::from_seats(game_db(), gspec, crate::az::Format::Bo1, &[seat(d0, s0), seat(d1, s1)], seed, [Some(prior.clone()), Some(prior)])
            .unwrap_or_else(|e| panic!("seed {seed}: {e}")),
    };
    let _ = log;
    // (The K-sample deal, ENGINE.md 102.8: `FUZZ_DEAL_SAMPLES=k`, default the spec's 32; 0 = a
    // fresh sample of the prior at every deal.)
    a.m.deal.k = super::env::var("FUZZ_DEAL_SAMPLES").ok().map_or(32, |v| v.parse().expect("FUZZ_DEAL_SAMPLES: a number"));
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
    if let Some(k) = choice {
        check_evaluator_choice(&a, choice_spec(seed, k), seed);
        play_deck_choice(&mut a, seed, &mut rng);
    }
    let mut crng = Rng::new(seed ^ 0xC0_4D_1E7E);
    let (mut actions, mut keys) = (Vec::new(), Vec::new());
    let (mut names_named, mut names_seen): (Vec<crate::ids::ObjRef>, Vec<u32>) = (Vec::new(), Vec::new());
    // (`FUZZ_POOL=lands_b`'s counts with hidden decklists too, ENGINE.md 270.2.)
    let mut lands_b_seen = LandsBSeen::default();
    let mut steps = 0usize;
    let mut mc = MatchCheck::new();
    rules_match_step(&mut mc, &a, &|| format!("seed {seed} (its first state)"));
    let (mut turn, mut turn_steps) = (a.g.turn().number, 0u64);
    while !a.g.is_over() {
        let legal = az_legal(&a);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        let act = if turn_steps >= HD_LONG_TURN && legal.contains(&crate::act::PASS) {
            if turn_steps == HD_LONG_TURN {
                HD_LONG_TURNS.fetch_add(1, Ordering::Relaxed);
            }
            crate::act::PASS
        } else if let Some(next) = names_next_page(&a.g, &mut rng) {
            next
        } else {
            legal[rng.below(legal.len() as u32) as usize]
        };
        count_kind(a.g.kind());
        let mover = a.to_move().unwrap() as u8;
        az_row_checked(&a, mover as usize, &|| format!("seed {seed} step {steps}"));
        if a.deck_choice().is_none() {
            let row = crate::az::tail_encode::Row::of_v8(&a.g, mover);
            check_graveyard_list(&a.g, &row, &|| format!("seed {seed} step {steps}"));
        }
        if rng.below(checkpoint_every()) == 0 {
            check_clone(&a.g, act, seed);
            check_determinize_hidden(&a, seed, &mut rng);
        }
        if complete_now(&a.g, &mut crng) {
            check_options_complete(&a.g, seed, "the game");
        }
        if lands_b_mode() {
            count_lands_b_choice(&a.g, act);
        }
        a.step(act, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        count_dropped_loop_records(&a.g, false);
        a.g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}"));
        a.g.check_knowledge().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}"));
        rules_match_step(&mut mc, &a, &|| format!("seed {seed} step {steps} (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1)"));
        if names_mode() {
            count_names(&a.g, &mut names_named, &mut names_seen);
        }
        if lands_b_mode() {
            count_lands_b(&a.g, &mut lands_b_seen);
        }
        actions.push(act);
        steps += 1;
        if a.g.turn().number != turn {
            record_turn(seed, turn, turn_steps);
            (turn, turn_steps) = (a.g.turn().number, 0);
        }
        turn_steps += 1;
        assert!(steps < 100_000, "seed {seed}: game does not end");
    }
    MAX_GAME_STEPS.fetch_max(steps as u64, Ordering::Relaxed);
    record_turn(seed, turn, turn_steps);
    rules_add(&mc.total());
    let g = &a.g;
    let companions = (0..2).filter(|&p| g.player(p).companion.is_some()).count() as u32;
    let companions_used = (0..2).filter(|&p| g.player(p).companion_used).count() as u32;
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number, companions, companions_used }
}

/// `MtgAz::determinize` with hidden decklists, for the decider and the other player (ENGINE.md
/// 102.3): the decider's actions (the other player: the decider still to move), the viewer's view
/// and whole encoding as in the real game, no card it knows dealt again, the opponent's sampled
/// deck of the public size holding every card the viewer has seen and equal to the match's deck of
/// theirs (the deck's cards and a revealed companion's, wherever they are: ENGINE.md 152.5), the
/// viewer's own cards untouched (likewise), the invariants and the clouds; every option of the
/// world's decision completes; for the decider, two worlds played with the same actions stay
/// indistinguishable while their keys, next decider and legal actions agree; then 30 random
/// actions in the world.
fn check_determinize_hidden(a: &crate::az::MtgAz, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let mover = a.to_move().unwrap() as u8;
    let (mut b0, mut b1) = (Vec::new(), Vec::new());
    for viewer in [mover, 1 - mover] {
        let o = 1 - viewer;
        let shown = hd_shown(a, viewer);
        let (real, mine) = (hd_deck(a, o), hd_deck(a, viewer));
        // (The game itself is one of the viewer's worlds: its deck holds every card shown, or the shown cards are
        // wrong, not a world - ENGINE.md 234.)
        if let Some(x) = hd_lacks(&real, &shown) {
            panic!("seed {seed}: the game's own deck of P{o} lacks a {} P{viewer} was shown (`hd_shown`)", a.g.db().def(x).name);
        }
        let mut d = a.clone();
        let s = rng.next_u64();
        // (FUZZ_DECK_TILT: the deck-tilted deal, ENGINE.md 106; the prediction from its own generator)
        let tilted = deck_tilt_fuzz().map(|t| {
            let mut pr = Rng::new(s ^ 0xDEC_7117);
            let mut seg = || {
                let mut p: Vec<f32> = (0..crate::az::identity::D).map(|_| ((pr.below(1000) + 1) as f32).powi(3)).collect();
                let sum: f32 = p.iter().sum();
                p.iter_mut().for_each(|x| *x /= sum);
                p
            };
            let mut pred = seg();
            pred.extend(seg());
            let hand = if pr.below(2) == 0 { t } else { 0.0 };
            zero_search::game::Belief { pred, tilt: hand, clip: 1e6, tilts: vec![0.0, t] }
        });
        let cached = tilted.as_ref().and_then(|b| {
            let t = crate::az::hidden_decks::DeckTilt { p: crate::az::belief::deck_segment(&b.pred), tilt: b.tilts[1], clip: b.clip };
            a.deck_tilt_of(viewer, t)
        });
        match &tilted {
            Some(b) => d.determinize_with(viewer as usize, &mut zero_search::rng::Rng::new(s), b),
            None => d.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s)),
        }
        let what = || format!("seed {seed}: a hidden-decklist world for P{viewer} (mover P{mover}){}", if tilted.is_some() { ", deck-tilted" } else { "" });
        if let Some((decks, td)) = &cached {
            let world = hd_deck(&d, o);
            let q: f64 = decks.iter().zip(&td.probs).filter(|(x, _)| x.deck == world).map(|(_, q)| q).sum();
            assert!(decks.iter().any(|x| x.deck == world), "{}: the world's deck is not one of the K cached decks", what());
            DT_WORLDS.fetch_add(1, Ordering::Relaxed);
            DT_RATIO.fetch_add((q * decks.len() as f64 * 1e6) as u64, Ordering::Relaxed);
        }
        if viewer == mover {
            assert_eq!(az_legal(&d), az_legal(a), "{}: the decider's actions", what());
        } else {
            assert_eq!(d.to_move(), Some(mover as usize), "{}: a refresh ran past the decision", what());
        }
        assert_eq!(a.g.view_of(viewer), d.g.view_of(viewer), "{}: the view", what());
        az_encode(a, viewer, &mut b0);
        az_encode(&d, viewer, &mut b1);
        if let Some(i) = (0..b0.len()).find(|&i| b0[i] != b1[i]) {
            let diff: Vec<usize> = (0..b0.len()).filter(|&i| b0[i] != b1[i]).collect();
            panic!(
                "{}: the viewer's feature {i} differs ({} / {}); all {diff:?}\nreal: {:?} {:?}\n  agenda {}\nworld: {:?} {:?}\n  agenda {}\n{}",
                what(),
                b0[i],
                b1[i],
                a.g.kind(),
                a.g.options_desc(),
                a.g.agenda_tail(4),
                d.g.kind(),
                d.g.options_desc(),
                d.g.agenda_tail(4),
                a.g.log_lines().iter().rev().take(30).rev().cloned().collect::<Vec<_>>().join("
")
            );
        }
        for (i, ob) in a.g.objs().iter().enumerate() {
            let c = crate::CardIdx::new(i as u32);
            if a.g.knows(viewer, c) {
                assert_eq!((ob.def, a.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "{}: a card the viewer knows was dealt again", what());
            }
        }
        d.g.check_invariants().unwrap_or_else(|e| panic!("{}: {e}", what()));
        d.g.check_knowledge().unwrap_or_else(|e| panic!("{}: {e}", what()));
        // (The deck as dealt, whichever object holds a shuffled-away companion: `hd_deck`, ENGINE.md 140.10 - the
        // one form of farm-fuzz's 152.5 fix and A1's, A1's kept at the integration, 157.)
        let world = hd_deck(&d, o);
        assert_eq!(world.len(), real.len(), "{}: the deck's size", what());
        if let Some(x) = hd_lacks(&world, &shown) {
            panic!("{}: the sampled deck lacks a shown {}", what(), a.g.db().def(x).name);
        }
        assert_eq!(d.m.deck(o), world, "{}: the match's deck of the opponent", what());
        assert_eq!(hd_deck(&d, viewer), mine, "{}: the viewer's own cards", what());
        for v in [0u8, 1] {
            b1 = az_row_checked(&d, v as usize, &what);
        }
        HD_WORLDS.fetch_add(1, Ordering::Relaxed);
        HD_OTHER_DECK.fetch_add((world != real) as u64, Ordering::Relaxed);
        check_options_complete(&d.g, seed, &what());
        if viewer == mover {
            check_hidden_information_set(a, viewer, seed, rng);
        }
        let mut keys = Vec::new();
        let mut wc = MatchCheck::new();
        rules_match_step(&mut wc, &d, &what);
        for _ in 0..30 {
            if d.to_move().is_none() {
                break;
            }
            let l = az_legal(&d);
            keys.clear();
            // (the viewer's hand before the step, ENGINE.md 175)
            let before = viewer_hand(&d, viewer);
            d.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap_or_else(|e| panic!("{}: {e}", what()));
            PK_GAME_KEYS.fetch_add(check_private_keys_masked_since(&keys, viewer, &d.g, &before, &what), Ordering::Relaxed);
            d.g.check_invariants().unwrap_or_else(|e| panic!("{} (played on): {e}", what()));
            rules_match_step(&mut wc, &d, &|| format!("{} (played on)", what()));
        }
        rules_add(&wc.total());
    }
}

/// zero-core's child signature on hidden-decklist worlds (`check_information_set`'s, through
/// `MtgAz`): equal keys, next decider and legal actions mean an equal view and encoding.
fn check_hidden_information_set(a: &crate::az::MtgAz, viewer: u8, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let s = rng.next_u64();
    let mut x = a.clone();
    x.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s));
    let mut y = a.clone();
    y.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
    let (mut bx, mut by) = (Vec::new(), Vec::new());
    for step in 0..25 {
        // (equal keys: the game over in both worlds or in neither, ENGINE.md 148)
        assert_eq!(x.to_move().is_none(), y.to_move().is_none(), "seed {seed} step {step}: same keys, the game over in one world of P{viewer} only");
        let (lx, ly) = (az_legal(&x), az_legal(&y));
        if x.to_move() != y.to_move() || lx != ly {
            let v = Some(viewer as usize);
            assert!(!(x.to_move() == v && y.to_move() == v), "seed {seed} step {step}: the viewer's options differ between its worlds");
            return;
        }
        assert_eq!(x.g.view_of(viewer), y.g.view_of(viewer), "seed {seed} step {step}: same keys, different views");
        az_encode(&x, viewer, &mut bx);
        az_encode(&y, viewer, &mut by);
        assert!(bx == by, "seed {seed} step {step}: same keys, different encodings");
        if x.to_move().is_none() {
            return;
        }
        let act = lx[rng.below(lx.len() as u32) as usize];
        let (mut kx, mut ky) = (Vec::new(), Vec::new());
        x.step(act, &mut kx).unwrap();
        y.step(act, &mut ky).unwrap();
        HD_STEPS.fetch_add(1, Ordering::Relaxed);
        if kx != ky {
            // (ENGINE.md 161: no forced answer of the other player splits worlds the viewer sees the same.)
            if x.sideboarding().is_none() && y.sideboarding().is_none() && x.m.game_no == y.m.game_no {
                if let Some(why) = forced_answer_split(&kx, &ky, &x.g, &y.g, viewer) {
                    panic!("hidden-decklist seed {seed} step {step}: {why}; decisions {:?} / {:?}", x.pending_decision(), y.pending_decision());
                }
            }
            return;
        }
    }
}

#[cfg(test)]
mod hidden_shown_tests {
    //! ENGINE.md 234: the fuzz's cards shown of a hidden deck (`hd_shown`) and a revealed companion that went
    //! into its owner's hand, back into the library, and onto the battlefield face down (the fuzz steward's
    //! queue 2eeffcef33d5 / 88f4716ce1e7: seed 100,200,667 of `FUZZ_HIDDEN_DECKS=1`, a Test Manifester's manifest).
    use super::*;
    use crate::{CardIdx, Zone};
    use zero_search::game::SearchGame;

    const LUTRI: &str = "Lutri, the Spellchaser";

    /// A hidden-decklist match of the default pool's table (as `play_hidden` makes it): P0 40 Islands with Lutri,
    /// the Spellchaser revealed as its companion (CR 103.2b; every nonland card of an all-land deck has a different
    /// name), P1 40 Islands; played to P0's first priority, its other choices the first option.
    fn match_to_priority(seed: u64) -> crate::az::MtgAz {
        let db = super::super::cards::db();
        let (island, lutri) = (db.expect_id("Island").unwrap(), db.expect_id(LUTRI).unwrap());
        let gspec = GameSpec { hidden_decks: true, hidden_decklists: true, ..GameSpec::default() };
        let seat = |sideboard: Vec<u16>| crate::az::Seat { deck: vec![island; 40], sideboard, first_picks: Vec::new(), elsewhere: Vec::new() };
        let prior: std::sync::Arc<dyn crate::az::hidden_decks::DeckPrior> = crate::az::hidden_decks::TablePrior::of(db);
        let mut a = crate::az::MtgAz::from_seats(db, gspec, crate::az::Format::Bo1, &[seat(vec![lutri]), seat(Vec::new())], seed, [Some(prior.clone()), Some(prior)])
            .unwrap();
        a.m.deal.k = 32;
        let mut keys = Vec::new();
        for _ in 0..200 {
            let d = a.g.pending_decision().expect("the game runs");
            if d.kind == DecisionKind::Priority && d.player == 0 {
                break;
            }
            let act = match d.kind {
                DecisionKind::RevealCompanion if d.player == 0 => {
                    d.options.iter().find(|o| o.choice == Choice::Def(lutri)).expect("Lutri is a companion of an all-land deck").action
                }
                _ => d.options[0].action,
            };
            a.step(act, &mut keys).unwrap();
        }
        assert_eq!((a.g.pending_decision().map(|d| (d.kind, d.player))), Some((DecisionKind::Priority, 0)), "P0's priority");
        assert_eq!(a.g.player(0).companion_def, Some(lutri), "P0 revealed Lutri");
        a
    }

    /// P0's Lutri into the hand (the companion special action, CR 116.2g / 702.139c, known to both players), on
    /// top of the library and the library shuffled (P1 loses track of it: a cloud of P1's), then manifested face
    /// down from the library (CR 701.40a, 708.2: a face-down permanent P1 doesn't know) - the engine's own steps
    /// for each (`companion_to_hand`, `move_card`, `shuffle_library`, manifest's `put_onto_battlefield` entering
    /// face down), the decision then asked again.
    fn lutri_manifested(a: &mut crate::az::MtgAz) -> CardIdx {
        let c = a.g.player(0).companion.expect("a companion");
        a.g.companion_to_hand(0);
        a.g.move_card(c, Zone::Library, crate::game::Pos::Top);
        a.g.shuffle_library(0);
        a.g.learn(0, c);
        a.g.entering_face_down = true;
        a.g.put_onto_battlefield(c, 0);
        a.g.entering_face_down = false;
        a.g.keys.clear();
        a.g.refresh_pending();
        a.g.check_invariants().unwrap();
        a.g.check_knowledge().unwrap();
        c
    }

    /// The state: the companion's object (outside P0's deck's objects) a face-down permanent P1 doesn't know, its
    /// identity Lutri in P1's cloud of P0's cards. It is no card of P0's deck - the game's own deck lacks it -,
    /// so the cards shown to P1 of P0's deck exclude it, and every check of a hidden-decklist world passes (it
    /// failed "the sampled deck lacks a shown Lutri, the Spellchaser" in every world: `hd_shown` dropped a
    /// non-deck card's identity from the clouds in a hidden zone only, not on a face-down permanent).
    #[test]
    fn a_companion_manifested_face_down_is_not_a_shown_deck_card() {
        let db = super::super::cards::db();
        let lutri = db.expect_id(LUTRI).unwrap();
        for seed in [1u64, 2, 3] {
            let mut a = match_to_priority(seed);
            let c = lutri_manifested(&mut a);
            let g = &a.g;
            assert_eq!(g.obj(c).zone, Zone::Battlefield, "the companion is a permanent");
            assert!(g.hidden_obj(c) && !g.knows(1, c), "face down, P1 doesn't know it");
            assert_eq!(crate::az::sample::card_of(g, c), lutri);
            assert!(g.clouds().iter().any(|k| k.viewer == 1 && k.owner == 0 && k.defs.contains(&lutri)), "P1's cloud of P0's cards holds Lutri");
            assert!(!hd_deck(&a, 0).contains(&lutri), "Lutri is no card of P0's deck");
            assert!(!hd_shown(&a, 1).contains(&lutri), "seed {seed}: Lutri shown to P1 as a card of P0's deck");
            assert_eq!(hd_lacks(&hd_deck(&a, 0), &hd_shown(&a, 1)), None);
            assert_eq!(a.to_move(), Some(0));
            for s in 0..6u64 {
                check_determinize_hidden(&a, seed, &mut Rng::new(s));
            }
        }
    }
}
