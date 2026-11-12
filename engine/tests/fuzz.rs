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
//! and with 200 games or more Murktide Regent must have been cast and its
//! leave-the-graveyard trigger resolved. Games only (best-of-one).
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
//! `FUZZ_POOL=names` (ENGINE.md 127): "choose a card name" with every kind of face. Each seat a
//! random 60 of constructed play's table: Anointed Peacekeeper and Disruptor Flute four each, eight
//! of the table's cards with more than one face (split cards, double-faced cards, adventurer and
//! preparation cards) three each, 28 basic lands. Every check of the default mode; with
//! `FUZZ_HIDDEN_DECKS=1` the hidden-decklist checks too (the names are then the table's). A name
//! question with a next page takes it three times in four (the table's faces and tokens are on its
//! last pages). With 200 games or more each card must have chosen a name, and the Peacekeeper each
//! kind of name: a card of one face, a back face, an Adventure, a split card's half, and `PASS`.
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
//! `FUZZ_POOL=sealed`'s) with hidden decklists, played through `MtgAz` (`MtgAz::from_seats`, the
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

mod common;

use engine::db::CardDb;
use engine::rng::Rng;
use engine::game::EndReason;
use engine::{Choice, DecisionKind, Game, GameSpec, Outcome, NUM_ACTIONS};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

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
enum PoolMode {
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
    /// `FUZZ_POOL=names` (ENGINE.md 127): Anointed Peacekeeper and Disruptor
    /// Flute with the table's cards of more than one face.
    Names,
}

fn pool_mode() -> PoolMode {
    match std::env::var("FUZZ_POOL").as_deref() {
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
        Ok("names") => PoolMode::Names,
        Ok(other) => panic!("FUZZ_POOL={other}: unknown (unset, \"interactions\", \"exponential\", \"drafted\", \"delver\", \"dd_a\", \"dd_b\", \"dd_c\", \"doomsday\", \"sneak_show\", \"sns\", \"names\", \"eoe\" or \"sealed\")"),
    }
}

fn interactions_mode() -> bool {
    pool_mode() == PoolMode::Interactions
}

fn exponential_mode() -> bool {
    pool_mode() == PoolMode::Exponential
}

fn delver_mode() -> bool {
    pool_mode() == PoolMode::Delver
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
/// `doomsday`, `sneak_show`, `sns` or `names`).
fn constructed_mode() -> bool {
    delver_mode() || dd_a_mode() || dd_b_mode() || ddc_mode() || doomsday_mode() || sns_mode() || names_mode()
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
    let deck = engine::az::constructed::Decklist::ids(db, &list).unwrap();
    engine::az::constructed::check(db, &deck, &[]).unwrap_or_else(|e| panic!("a names deck: {e}"));
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
    let next = g.pending_decision()?.options.iter().find(|o| matches!(o.choice, engine::Choice::Page(_)))?.action;
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
fn count_names(g: &Game, named: &mut Vec<engine::ids::ObjRef>, seen: &mut Vec<u32>) {
    let db = g.db();
    let face = |d: u16| engine::sneak_show::face_name(db.def(d));
    let mut chosen: Vec<(u8, bool, &str)> = Vec::new();
    for &c in g.battlefield() {
        let card = match g.name(c) {
            "Anointed Peacekeeper" => 0,
            "Disruptor Flute" => 1,
            _ => continue,
        };
        let Some(d) = g.marks_of(c).iter().find_map(|m| match m {
            engine::game::Mark::ChosenName(d) => Some(*d),
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
        if let engine::game::StackKind::Spell { card } = it.kind {
            let dd = g.def_of(card);
            let names: Vec<&str> = match dd.halves {
                Some((l, r)) => vec![face(l), face(r)],
                None => vec![engine::sneak_show::face_name(dd)],
            };
            if chosen.iter().any(|&(p, everyone, n)| (everyone || p != it.controller) && names.contains(&n)) {
                NAMES_TAXED.fetch_add(1, Ordering::Relaxed);
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
    let d = engine::az::constructed::Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap();
    let deck = engine::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap();
    engine::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("the Sneak and Show 60: {e}"));
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
    let deck = engine::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    engine::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("SNS_DECK: {e}"));
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
            engine::game::StackKind::Spell { card } => {
                if let Some(k) = SNS_CARDS.iter().position(|&n| n == g.name(card)) {
                    SNS_SEEN[k].fetch_add(1, Ordering::Relaxed);
                }
                if matches!(it.from, engine::game::CastFrom::Permission { free: true, .. }) {
                    SNS_FREE.fetch_add(1, Ordering::Relaxed);
                }
                if g.marks_of(card).contains(&engine::game::Mark::Uncounterable) {
                    SNS_UNCOUNTERABLE.fetch_add(1, Ordering::Relaxed);
                }
            }
            engine::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Mistrise Village" => {
                SNS_SEEN[2].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// `FUZZ_POOL=sneak_show` / `sns`: names chosen as a Flute entered (each
/// permanent once).
fn count_sns_names(g: &Game, named: &mut Vec<engine::ids::ObjRef>) {
    for &c in g.battlefield() {
        let r = g.obj_ref(c);
        if !named.contains(&r) && g.marks_of(c).iter().any(|m| matches!(m, engine::game::Mark::ChosenName(_))) {
            named.push(r);
            SNS_NAMED.fetch_add(1, Ordering::Relaxed);
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
    let d = engine::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
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
    use engine::game::StackKind;
    if g.kind() == Some(engine::DecisionKind::NewTarget) {
        if let Some(StackKind::Spell { card }) = g.stack().last().map(|s| s.kind) {
            if g.name(card) == "Misdirection" {
                DD_B_RETARGETS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    for it in g.stack() {
        let StackKind::Spell { card } = it.kind else { continue };
        if !seen.contains(&(2, it.id)) && g.spell_cant_be_countered(card) && g.db().def(g.obj(card).def).is(engine::db::types::CREATURE) {
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
        if g.restricted_mana(p).iter().any(|k| k.1 & engine::db::SPEND_CHOSEN_CREATURE_TYPE != 0) {
            DD_B_TYPE_MANA.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn eoe_mode() -> bool {
    pool_mode() == PoolMode::Eoe
}

fn ddc_mode() -> bool {
    pool_mode() == PoolMode::DoomsdayC
}

/// `FUZZ_POOL=doomsday` (ENGINE.md 124.4).
fn doomsday_mode() -> bool {
    pool_mode() == PoolMode::Doomsday
}

fn sealed_mode() -> bool {
    pool_mode() == PoolMode::Sealed
}

/// The sealed spec of game `seed` (`FUZZ_POOL=sealed`): arena pools for even seeds, paper for odd;
/// with `FUZZ_BEST_SHARE=p` the mixed field's `"best_share": p` (ENGINE.md 107.1).
fn sealed_spec(seed: u64) -> &'static engine::az::MtgSpec {
    static SPECS: std::sync::OnceLock<[engine::az::MtgSpec; 2]> = std::sync::OnceLock::new();
    let specs = SPECS.get_or_init(|| {
        let share = match std::env::var("FUZZ_BEST_SHARE") {
            Ok(v) => format!(r#", "best_share": {}"#, v.parse::<f64>().expect("FUZZ_BEST_SHARE: a probability")),
            Err(_) => String::new(),
        };
        ["arena", "paper"].map(|c| engine::az::MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "{c}"{share}}}}}"#)).unwrap())
    });
    &specs[(seed % 2) as usize]
}

/// The decks and sideboards of sealed game `seed`, or why it has none (a pool holding a card
/// this checkout doesn't support).
fn sealed_decks(seed: u64) -> Result<[(Vec<u16>, Vec<u16>); 2], String> {
    Ok(sealed_spec(seed).try_seats(seed)?.map(|s| (s.deck, s.sideboard)))
}

/// `FUZZ_POOL=sealed`: game `seed` can't be dealt (its error), else None.
fn sealed_unplayable(seed: u64) -> Option<String> {
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
            if db.def(id).is(engine::db::types::LAND) {
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
    if cfg!(debug_assertions) { 4 } else { 40 }
}

/// `FUZZ_BELIEF=t`: the tilt of the belief-weighted deal's checks (ENGINE.md 69); unset: none.
fn belief_tilt() -> Option<f64> {
    std::env::var("FUZZ_BELIEF").ok().map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("FUZZ_BELIEF={s}: a tilt (a number > 0)"))).inspect(|&t| {
        assert!(t > 0.0 && t.is_finite(), "FUZZ_BELIEF={t}: a tilt > 0");
    })
}

/// `FUZZ_DECK_TILT=t`: the deck tilt of the hidden-decklist worlds (ENGINE.md 106); unset: none.
fn deck_tilt_fuzz() -> Option<f64> {
    std::env::var("FUZZ_DECK_TILT").ok().map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("FUZZ_DECK_TILT={s}: a tilt (a number > 0)"))).inspect(|&t| {
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
    (0..engine::az::identity::D)
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
    let ids = engine::az::identity::identities(g.db());
    let cards = |g: &Game, p: u8| -> Vec<u16> {
        let mut v: Vec<u16> = g.objs().iter().enumerate().filter(|(_, o)| o.owner == p).map(|(i, _)| g.ident(engine::CardIdx::new(i as u32))).collect();
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
            let c = engine::CardIdx::new(i as u32);
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
        let unknown_hand: Vec<engine::CardIdx> = g.player(o).hand.iter().copied().filter(|&c| !g.knows(viewer, c)).collect();
        if !pool.is_empty() && !unknown_hand.is_empty() {
            let best = pool.iter().map(|&c| g.ident(c)).max_by(|&a, &b| w[ids[a as usize] as usize].total_cmp(&w[ids[b as usize] as usize]).then(b.cmp(&a))).unwrap();
            let h = pool.iter().filter(|&&c| g.ident(c) == best).count() as f64;
            let dealt = unknown_hand.iter().filter(|&&c| d.ident(c) == best).count() as f64;
            BELIEF_WORLDS.fetch_add(1, Ordering::Relaxed);
            BELIEF_DEALT.fetch_add((dealt * 1000.0) as u64, Ordering::Relaxed);
            BELIEF_UNIFORM.fetch_add((unknown_hand.len() as f64 * h / pool.len() as f64 * 1000.0) as u64, Ordering::Relaxed);
        }
        let (mut legal, mut keys) = (Vec::new(), Vec::new());
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
        }
    }
}

/// `FUZZ_CHANCE=n` (ENGINE.md 74): one decision in `n` (its own generator) runs a chance walk;
/// unset: none.
fn chance_every() -> Option<u32> {
    std::env::var("FUZZ_CHANCE").ok().map(|s| {
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
fn chance_step(w: &mut Game, act: u16, forced: Option<&[u64]>, at: &str, path: &[String]) -> Vec<u64> {
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
    let ids = engine::az::identity::identities(g.db());
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
        }
        let act = lf[rng.below(lf.len() as u32) as usize];
        let at = |how: &str| format!("seed {seed} step {step}: chance walk (mover P{mover}) at {depth}, {how}, action {act}");
        path.push(format!("{act} = {}", free.describe_action(act).unwrap_or_default()));
        let kf = chance_step(&mut free, act, None, &at("the free world"), &path);
        let unforced = chance_step(&mut replay.clone(), act, None, &at("the replaying world unforced"), &path);
        let kr = chance_step(&mut replay, act, Some(&kf), &at("the replaying world"), &path);
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
        chance_step(&mut replay, act, None, &at, &path);
    }
}

/// The card table of the mode's games: the cube's and the custom test cards,
/// or constructed play's (`FUZZ_POOL=delver`).
fn game_db() -> &'static CardDb {
    if constructed_mode() {
        CardDb::constructed()
    } else if eoe_mode() || sealed_mode() {
        CardDb::eoe()
    } else {
        common::db()
    }
}

/// The pinned Delver 60 (`FUZZ_POOL=delver`).
fn delver_deck() -> Vec<u16> {
    let d = engine::az::constructed::Decklist::izzet_delver();
    engine::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap()
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
    let deck = engine::az::constructed::Decklist::ids(CardDb::constructed(), &list).unwrap();
    engine::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("DD_A_DECK: {e}"));
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
            engine::game::StackKind::Spell { card } => {
                if let Some(k) = DD_A_CARDS.iter().position(|&n| n == g.name(card)) {
                    DD_A_CASTS[k].fetch_add(1, Ordering::Relaxed);
                }
            }
            engine::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Pact of Negation" => {
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
            engine::game::StackKind::Spell { card } => match g.name(card) {
                "Edge of Autumn" => DDC_EDGE_CAST.fetch_add(1, Ordering::Relaxed),
                "Street Wraith" => DDC_WRAITH_CAST.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            },
            engine::game::StackKind::Ability { source, .. } => match g.db().def(g.obj(source.idx).def).name.as_str() {
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
            g.battlefield().iter().any(|&c| g.obj(c).controller == d && g.types_of(c) & engine::db::types::LAND != 0 && g.has_subtype(c, s))
        });
        for &c in g.battlefield() {
            if has_swamp && g.obj(c).attacking && g.has_kw(c, engine::db::Kw::Swampwalk) {
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
    let d = engine::az::constructed::Decklist::legacy_doomsday();
    let deck = engine::az::constructed::Decklist::ids(CardDb::constructed(), &d.main).unwrap();
    engine::az::constructed::check(CardDb::constructed(), &deck, &[]).unwrap_or_else(|e| panic!("the Doomsday 60: {e}"));
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
    use engine::game::StackKind;
    if g.kind() == Some(DecisionKind::NewTarget) {
        if let Some(StackKind::Spell { card }) = g.stack().last().map(|it| it.kind) {
            if g.name(card) == "Misdirection" {
                DOOMSDAY_RETARGETS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    for it in g.stack() {
        if let StackKind::Spell { card } = it.kind {
            if !s.uncounterable.contains(&it.id) && g.spell_cant_be_countered(card) && g.db().def(g.obj(card).def).is(engine::db::types::CREATURE) {
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

/// `FUZZ_POOL=delver`: Murktide Regents that resolved, and leave-the-graveyard
/// triggers of theirs that resolved (counted from the game logs' counters).
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
        std::env::var("FUZZ_SKIP")
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
            engine::game::StackKind::Spell { card } => match g.name(card) {
                "Murktide Regent" => DELVER_MURKTIDES.fetch_add(1, Ordering::Relaxed),
                "Prismari Charm" => DELVER_CHARMS.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            },
            engine::game::StackKind::Ability { source, .. } if g.db().def(g.obj(source.idx).def).name == "Murktide Regent" => {
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
    let (lands, _) = common::fuzz_pool();
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
        engine::az::encode::SQUASH_FROM
    )
}

/// Every value of an encoding is finite and within the bound (ENGINE.md 46).
fn check_bounded(x: &[f32], what: &dyn Fn() -> String) {
    use engine::az::encode::{FEATURE_BOUND, SQUASH_FROM};
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
fn check_graveyard_list(g: &Game, row: &engine::az::tail_encode::Row, what: &dyn Fn() -> String) {
    use engine::act;
    use engine::az::encode::{list, CODE};
    use engine::az::tail_encode::seg;
    use engine::{Target, Zone};
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
    let gy: Vec<engine::CardIdx> = slots.iter().flatten().copied().collect();
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
    let ids = engine::az::identity::identities(g.db());
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
        assert_eq!(engine::az::tail_encode::code_identity(&x[..CODE]), Some(ids[g.obj(c).def as usize] as usize), "{}: {:?} position {pos}'s token isn't {}", what(), d.kind, g.name(c));
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
        engine::decision::Stage::Paged { window } if window > 0 => true,
        engine::decision::Stage::Paged { .. } => d.options.iter().any(|o| o.action == engine::act::LIBRARY + engine::act::LIBRARY_N - 1),
        _ => false,
    }
}

/// Layout v7's encoding invariants on `viewer`'s row of `g` (ENGINE.md 125): every value finite
/// and bounded (head and tail); the head is `encode_features`' when the row has no tail (a row with
/// one is refused there: R1, `tests/v7_tails.rs`); the head's v6 projection is layout v6's encoding
/// (`mtg_eoe`'s path) but on a page v6 showed as a window; every option of the viewer's decision
/// points at its object's token (`check_pointers`); the row's capacity within the limit (counted:
/// zero-core's driver aborts such a game); the census (`FUZZ_CAP_CENSUS`).
fn check_row(g: &Game, viewer: u8, row: &engine::az::tail_encode::Row, what: &dyn Fn() -> String) {
    use engine::az::encode::{v6_projection, NUM_FEATURES_V6};
    let vals: Vec<f32> = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).copied().collect();
    check_bounded(&vals, &|| format!("{} (viewer P{viewer})", what()));
    if row.tail.is_empty() {
        let mut h = vec![0.0f32; engine::az::NUM_FEATURES];
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
    let n = engine::az::tail_encode::check_pointers(g, viewer, row).unwrap_or_else(|e| panic!("{}: {e} (viewer P{viewer})", what()));
    POINTERS.fetch_add(n as u64, Ordering::Relaxed);
    let cap = row.capacity() as u64;
    MAX_CAPACITY.fetch_max(cap, Ordering::Relaxed);
    if cap > engine::az::tail_encode::MAX_ROW_TOKENS as u64 {
        ROWS_OVER_LIMIT.fetch_add(1, Ordering::Relaxed);
    }
    census::record(g, viewer, row);
}

/// `viewer`'s whole row of `g` (`buf`: the row as one vector, `Row::flat`, the head first), checked
/// (`check_row`).
fn check_encoding(g: &Game, viewer: u8, buf: &mut Vec<f32>, what: &dyn Fn() -> String) {
    let row = engine::az::tail_encode::Row::of(g, viewer);
    check_row(g, viewer, &row, what);
    *buf = row.flat();
}

/// The rows' report: pointers checked, v6 projections, the limit.
fn rows_report() -> String {
    format!(
        "rows: {} options' pointers checked; v6 projection equal on {} rows, {} on pages v6 showed as a window; the most tokens in a row {} (limit {}), {} rows past it",
        POINTERS.load(Ordering::Relaxed),
        V6_SAME.load(Ordering::Relaxed),
        V6_PAGED.load(Ordering::Relaxed),
        MAX_CAPACITY.load(Ordering::Relaxed),
        engine::az::tail_encode::MAX_ROW_TOKENS,
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
    use engine::az::tail_encode::{seg, Row, SEG_HEAD};

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
        *ON.get_or_init(|| std::env::var("FUZZ_CAP_CENSUS").is_ok_and(|v| v == "1"))
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
        let path = std::env::var("FUZZ_CAP_CENSUS_OUT").unwrap_or_else(|_| format!("fuzz_cap_census_{pool}.json"));
        std::fs::write(&path, &s).unwrap_or_else(|e| panic!("FUZZ_CAP_CENSUS_OUT {path}: {e}"));
        Some(s)
    }
}

/// The spec of drafted match `seed` (`FUZZ_POOL=drafted`): a 4v4 or 2v2 event
/// (random, or team A as `consensus`) and one of its matches.
fn drafted_spec(seed: u64) -> engine::az::MtgSpec {
    use engine::az::draft::{DraftSpec, TeamDraftAz};
    use engine::az::draft_bots::{match_spec, Consensus};
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
    engine::az::MtgSpec::parse(&match_spec(setup, turns, engine::az::Format::Bo3)).unwrap()
}

/// The groups' cards as ids of `db`, and `db`'s supported lands.
/// `FUZZ_GROUPS=n`: the first `n` groups only, so a failure recorded before
/// a group was added deals its decks again (the farm's interactions seeds
/// at 075649c: `FUZZ_GROUPS=13`; ENGINE.md 51).
fn interaction_pool(db: &CardDb) -> (Vec<Vec<u16>>, Vec<u16>) {
    let n: usize = std::env::var("FUZZ_GROUPS").ok().map(|s| s.parse().expect("FUZZ_GROUPS: a number of groups")).unwrap_or(GROUPS.len());
    assert!((1..=GROUPS.len()).contains(&n), "FUZZ_GROUPS: 1 to {}", GROUPS.len());
    let groups = GROUPS[..n]
        .iter()
        .map(|(_, names)| names.iter().map(|n| db.expect_id(n).unwrap_or_else(|e| panic!("FUZZ_POOL group card: {e}"))).collect())
        .collect();
    let lands = db.supported().map(|n| db.id(n).unwrap()).filter(|&id| db.def(id).is(engine::db::types::LAND)).collect();
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

struct Played {
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    outcome: Outcome,
    turns: u32,
    /// Companions revealed, and put into a hand (ENGINE.md 35).
    companions: u32,
    companions_used: u32,
}

/// The decks and sideboards of game `seed` (ENGINE.md 35): one game in four
/// has the self-play generator's decks (two colours, singleton, a sideboard,
/// often a companion, a deck built for it); the others random decks with a
/// sideboard of seven random cards.
fn decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    let db = common::db();
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
    if names_mode() {
        return [(names_deck(&mut rng), Vec::new()), (names_deck(&mut rng), Vec::new())];
    }
    if eoe_mode() {
        return [(eoe_deck(&mut rng), Vec::new()), (eoe_deck(&mut rng), Vec::new())];
    }
    if sealed_mode() {
        return sealed_decks(seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
    if seed % 4 == 3 {
        let gen = engine::az::decks::DeckGen::default();
        return [
            engine::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
            engine::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
        ];
    }
    let (_, spells) = common::fuzz_pool();
    let sideboard = |side: &mut Rng| -> Vec<u16> { (0..7).map(|_| spells[side.below(spells.len() as u32) as usize]).collect() };
    let d0 = common::random_deck(db, &mut rng);
    let d1 = common::random_deck(db, &mut rng);
    [(d0, sideboard(&mut side)), (d1, sideboard(&mut side))]
}

fn new_game(seed: u64) -> Game {
    let [(d0, s0), (d1, s1)] = decks(seed);
    // `FUZZ_LOG=1` keeps the game log, so a panic message shows the game.
    let log = std::env::var("FUZZ_LOG").is_ok();
    let starting_life = if exponential_mode() { EXP_LIFE } else { GameSpec::default().starting_life };
    Game::with_sideboards(game_db(), GameSpec { log, starting_life, ..GameSpec::default() }, [&d0, &d1], [&s0, &s1], seed).unwrap()
}

/// Play one game with random actions; run the extra checks along the way.
fn play(seed: u64, checks: bool) -> Played {
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
    let mut sns_named: Vec<engine::ids::ObjRef> = Vec::new();
    // (`FUZZ_POOL=names`: the namers counted and the stack items seen, ENGINE.md 127.)
    let (mut names_named, mut names_seen): (Vec<engine::ids::ObjRef>, Vec<u32>) = (Vec::new(), Vec::new());
    let (mut turn, mut turn_steps) = (g.turn().number, 0u64);
    while !g.is_over() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        // (Exponential boards: attackers are declared one time in four, so
        // the boards grow through the 60 turns.)
        let a = if exp && g.kind() == Some(DecisionKind::DeclareAttackers) && rng.below(4) != 0 && legal.contains(&engine::act::PASS) {
            engine::act::PASS
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
            let row = engine::az::tail_encode::Row::of(&g, mover);
            check_row(&g, mover, &row, &|| format!("seed {seed} step {steps}"));
            if census::on() {
                let other = engine::az::tail_encode::Row::of(&g, 1 - mover);
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
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}
{}", g.log_lines().iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("
")));
        actions.push(a);
        steps += 1;
        if g.turn().number != turn {
            record_turn(seed, turn, turn_steps);
            (turn, turn_steps) = (g.turn().number, 0);
        }
        turn_steps += 1;
        if turn_steps == LONG_TURN && std::env::var("FUZZ_LOG").is_ok() {
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
        if names_mode() {
            count_names(&g, &mut names_named, &mut names_seen);
        }
        assert!(steps < 100_000, "seed {seed}: game does not end
{}", g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
"));
    }
    MAX_GAME_STEPS.fetch_max(steps as u64, Ordering::Relaxed);
    record_turn(seed, turn, turn_steps);
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
            let c = engine::CardIdx::new(i as u32);
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
        if viewer == mover {
            check_information_set(g, viewer, seed, rng);
        }
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for _ in 0..30 {
            if d.is_over() {
                break;
            }
            d.legal_actions(&mut legal);
            d.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
            d.check_invariants().unwrap_or_else(|e| {
                panic!(
                    "seed {seed} (determinized for P{viewer}): {e}\n{}",
                    d.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
                )
            });
        }
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

/// Every revealed fact is keyed: two worlds `determinize(viewer)` samples,
/// played with the same actions, stay indistinguishable to the viewer as
/// long as the search would keep them in one node: zero-core keys children
/// by (chance keys, next decider, next legal actions), so equal keys, an
/// equal next decider and equal legal actions must mean an equal view.
fn check_information_set(g: &Game, viewer: u8, seed: u64, rng: &mut Rng) {
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    for step in 0..25 {
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
        assert_eq!(a.view_of(viewer), b.view_of(viewer), "seed {seed} step {step}: same keys, different views
{}
----
{}", a.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"), b.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"));
        if a.is_over() {
            return;
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.apply(act, &mut ka).unwrap();
        b.apply(act, &mut kb).unwrap();
        if ka != kb {
            return; // different outcomes, told apart by the keys
        }
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut wins, mut draws, mut aborted, mut turns, mut decisions) = ([0u32; 2], 0u32, 0u32, 0u64, 0u64);
    let (mut companions, mut companions_used) = (0u32, 0u32);
    let mut skips = 0u64;
    let mut unplayable: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for seed in first..first + n {
        if skipped(seed) {
            skips += 1;
            continue;
        }
        if let Some(e) = sealed_unplayable(seed) {
            // (The error names the card: "... <name> is not supported: <blockers>".)
            let card = e.split(": ").find(|x| x.ends_with(" is not supported")).map_or(e.clone(), |x| x.trim_end_matches(" is not supported").to_string());
            *unplayable.entry(card).or_insert(0) += 1;
            continue;
        }
        let p = std::panic::catch_unwind(|| if hidden_decks_mode() { play_hidden(seed) } else { play(seed, true) }).unwrap_or_else(|e| {
            panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
        });
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
    if let Some(c) = census::report(&std::env::var("FUZZ_POOL").unwrap_or_else(|_| "cube".into()), n) {
        println!("FUZZ_CAP_CENSUS:\n{c}");
    }
    if n >= 200 && pool_mode() == PoolMode::Default {
        assert!(companions_used > 0, "no companion special action in {n} games");
    }
    if n >= 100 && exponential_mode() {
        assert!(MAX_POWER.load(Ordering::Relaxed) > u32::MAX as i64, "no board past u32::MAX power: {}", extremes_report());
    }
    println!("longest game: {} actions (cap 100,000)", MAX_GAME_STEPS.load(Ordering::Relaxed));
    println!("controlled decisions (CR 722.4, encoded in the controlled seat: ENGINE.md 100.5): {}", CONTROLLED.load(Ordering::Relaxed));
    println!("{}; seeds skipped (FUZZ_SKIP): {skips}", longest_turn_report());
    if hidden_decks_mode() {
        let (w, x, st, lt) =
            (HD_WORLDS.load(Ordering::Relaxed), HD_OTHER_DECK.load(Ordering::Relaxed), HD_STEPS.load(Ordering::Relaxed), HD_LONG_TURNS.load(Ordering::Relaxed));
        println!("FUZZ_HIDDEN_DECKS: {w} hidden-decklist worlds, {x} with another deck than the real one; {st} information-set steps in step; {lt} turns past {HD_LONG_TURN} decisions");
        if n >= 200 {
            assert!(w > 0 && x * 10 > w * 9, "FUZZ_HIDDEN_DECKS: {x} of {w} worlds with another deck");
        }
    }
    if let Some(k) = deck_choice_fuzz().filter(|_| sealed_mode() && hidden_decks_mode()) {
        let (c, w) = (DC_CHOICES.load(Ordering::Relaxed), DC_WORLDS.load(Ordering::Relaxed));
        println!("FUZZ_DECK_CHOICE={k}: {c} deck choices played, {w} worlds checked during them");
        if let Ok(path) = std::env::var("FUZZ_DECK_CHOICE_BY") {
            let (ch, ex) = (DC_EVAL_CHOSEN.load(Ordering::Relaxed), DC_EVAL_EXPLORE.load(Ordering::Relaxed));
            println!("FUZZ_DECK_CHOICE_BY={path}: {ch} seats chose by the evaluator, {ex} explored");
            if n >= 100 {
                assert!(ch > 0 && c == 0, "FUZZ_DECK_CHOICE_BY: {ch} seats chose, {c} decisions (the evaluator's choice is none)");
            }
        } else if n >= 100 {
            assert!(c > 0 && w > 0, "FUZZ_DECK_CHOICE: no deck choice was played");
        }
    }
    if let Some(t) = deck_tilt_fuzz() {
        let (w, r) = (DT_WORLDS.load(Ordering::Relaxed), DT_RATIO.load(Ordering::Relaxed));
        let mean = r as f64 / 1e6 / w.max(1) as f64;
        println!("FUZZ_DECK_TILT={t}: {w} deck-tilted worlds from the K cached decks; K x the dealt deck's probability {mean:.3} on average (uniform 1)");
        if w >= 100 {
            assert!(mean > 1.5, "FUZZ_DECK_TILT: the deck tilt does not concentrate the deal ({mean:.3})");
        }
    }
    if let Some(t) = belief_tilt() {
        let (w, d, u) = (BELIEF_WORLDS.load(Ordering::Relaxed), BELIEF_DEALT.load(Ordering::Relaxed), BELIEF_UNIFORM.load(Ordering::Relaxed));
        println!(
            "FUZZ_BELIEF={t}: {w} weighted worlds with unknown hand cards; the favoured identity dealt {:.3} copies per world, the uniform deal's {:.3}",
            d as f64 / 1000.0 / w.max(1) as f64,
            u as f64 / 1000.0 / w.max(1) as f64
        );
        if w >= 100 {
            assert!(d > 2 * u, "FUZZ_BELIEF: the weighted deal does not concentrate ({d} vs uniform {u})");
        }
    }
    println!(
        "graveyard lists (ENGINE.md 65.10): {} decisions checked, longest {} (cards offered from both graveyards: at most {}; the limit 64)",
        GY_LISTS.load(Ordering::Relaxed),
        GY_LIST_MAX.load(Ordering::Relaxed),
        GY_LIST_BOTH_MAX.load(Ordering::Relaxed)
    );
    if n >= 200 {
        assert!(GY_LISTS.load(Ordering::Relaxed) > 0, "no graveyard-list decision in {n} games");
    }
    if let Some(c) = chance_every() {
        let (w, s, f, x) = (
            CHANCE_WALKS.load(Ordering::Relaxed),
            CHANCE_STEPS.load(Ordering::Relaxed),
            CHANCE_FORCED.load(Ordering::Relaxed),
            CHANCE_SPLIT.load(Ordering::Relaxed),
        );
        println!("FUZZ_CHANCE={c}: {w} chance walks, {s} steps at one node, {f} replays whose outcome the force changed, {x} walks split by the keys");
        if n >= 100 {
            assert!(f > 0, "FUZZ_CHANCE: no replay took a forced outcome ({w} walks, {s} steps)");
        }
    }
    if sealed_mode() {
        let total: u64 = unplayable.values().sum();
        println!("FUZZ_POOL=sealed: {} games played; {total} games skipped, a pool holding a card this checkout doesn't support: {unplayable:?}", n - total - skips);
        // the mixed field (ENGINE.md 107.1): how many seats played the best guess
        if let Ok(v) = std::env::var("FUZZ_BEST_SHARE") {
            let p: f64 = v.parse().expect("FUZZ_BEST_SHARE: a probability");
            let best = (first..first + n).map(|s| (0..2).filter(|&q| engine::az::sealed::best_seat(s, q, p)).count()).sum::<usize>();
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
        if n >= 200 {
            assert!(m > 0 && r > 0 && f > 0 && c > 0 && t > 0 && u > 0, "lane B's cards weren't played: {m} {r} {f} {c} {t} {u}");
        }
    }
    if delver_mode() {
        let (m, t, c) =
            (DELVER_MURKTIDES.load(Ordering::Relaxed), DELVER_MURK_TRIGGERS.load(Ordering::Relaxed), DELVER_CHARMS.load(Ordering::Relaxed));
        println!("FUZZ_POOL=delver: Murktide Regent cast {m} times, its leave-the-graveyard triggers {t}, Prismari Charm cast {c} times");
        if n >= 200 {
            assert!(m > 0 && t > 0 && c > 0, "the legacy cards weren't played: {m} {t} {c}");
        }
    }
    if dd_a_mode() {
        let casts: Vec<u64> = DD_A_CASTS.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let pacts = DD_A_PACT_TRIGGERS.load(Ordering::Relaxed);
        println!(
            "FUZZ_POOL=dd_a: seen on the stack {}; Pact of Negation's triggers {pacts}",
            DD_A_CARDS.iter().zip(&casts).map(|(n, c)| format!("{n} {c} times")).collect::<Vec<_>>().join(", ")
        );
        if n >= 200 {
            assert!(casts.iter().all(|&c| c > 0) && pacts > 0, "lane A's cards weren't played: {casts:?} {pacts}");
        }
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
        if n >= 300 {
            assert!(ec > 0 && ey > 0 && wc > 0 && wy > 0 && lo > 0 && lg > 0 && le > 0, "lane C's cards weren't played");
        }
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
        let cast = |name: &str| casts[DOOMSDAY_SPELLS.iter().position(|&n| n == name).unwrap()];
        if n >= 200 {
            let main = ["Doomsday", "Pact of Negation", "Personal Tutor", "Flow State", "Misdirection", "Thassa's Oracle", "Murktide Regent", "Prismari Charm"];
            assert!(main.iter().all(|&m| cast(m) > 0), "the decks' cards weren't played: {casts:?}");
            assert!(abil[0] > 0 && abil[1] > 0 && abil[2] > 0 && r > 0 && t > 0, "the legacy cards' abilities weren't used: {abil:?} {r} {t}");
        }
        // (Edge of Autumn and Street Wraith are cycled far more often than
        // cast: their casts are asserted over 2,000 games.)
        if n >= 2000 {
            assert!(cast("Edge of Autumn") > 0 && cast("Street Wraith") > 0, "Edge of Autumn or Street Wraith never cast: {casts:?}");
        }
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
        if n >= 200 {
            assert!([0, 2, 3, 4, 7].iter().all(|&k| chosen[0][k] > 0), "Anointed Peacekeeper's kinds of names: {chosen:?}");
            assert!(chosen[1].iter().sum::<u64>() > 0 && taxed > 0, "Disruptor Flute's names, taxed spells: {chosen:?} {taxed}");
        }
    }
    if sns_mode() {
        let seen: Vec<u64> = SNS_SEEN.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let (free, unc, named) = (SNS_FREE.load(Ordering::Relaxed), SNS_UNCOUNTERABLE.load(Ordering::Relaxed), SNS_NAMED.load(Ordering::Relaxed));
        println!(
            "FUZZ_POOL={}: seen on the stack {}; spells cast without paying their mana costs {free}, made uncounterable {unc}; names chosen {named}",
            if pool_mode() == PoolMode::Sns { "sns" } else { "sneak_show" },
            SNS_CARDS.iter().zip(&seen).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")
        );
        if n >= 200 && pool_mode() == PoolMode::Sns {
            assert!(seen.iter().all(|&c| c > 0) && free > 0 && unc > 0 && named > 0, "the lane's cards weren't played: {seen:?} {free} {unc} {named}");
        }
        // (The deck's one Omniscience, ten mana, mostly enters by Show and Tell: its free casts
        // are what is required.)
        if n >= 200 && pool_mode() == PoolMode::SneakShow {
            assert!(seen[0] > 0 && seen[2] > 0 && free > 0 && unc > 0, "the deck's lane cards weren't played: {seen:?} {free} {unc}");
        }
    }
}

#[test]
fn same_seed_and_actions_replay_identically() {
    for seed in 1000..1030 {
        if sealed_unplayable(seed).is_some() {
            continue;
        }
        let a = play(seed, false);
        // Replay the recorded actions from the same seed.
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), a.fingerprint, "seed {seed}");
        assert_eq!(keys, a.keys, "seed {seed}: chance keys differ");
        assert_eq!(g.outcome(), Some(a.outcome));
        // And the whole random game is reproducible.
        let b = play(seed, false);
        assert_eq!(b.fingerprint, a.fingerprint);
    }
}

/// The spec of fuzzed match `seed`: random pools (bo3's default), the
/// evaluation pools, M8a's generated decks with 7-card sideboards, or short
/// games (drawn games: the turn cap).
fn match_spec(seed: u64) -> engine::az::MtgSpec {
    if pool_mode() == PoolMode::Drafted {
        return drafted_spec(seed);
    }
    if interactions_mode() {
        // Combo-dense lists (cube cards), best-of-three, open decklists;
        // one match in four short.
        let db = CardDb::cube();
        let mut rng = Rng::new(seed ^ 0x1A7E_DEC5);
        let p = [interaction_deck(db, &mut rng), interaction_deck(db, &mut rng)];
        let mut s = engine::az::MtgSpec::parse(r#"{"format": "bo3", "pools": false}"#).unwrap();
        s.decks = engine::az::DeckMode::ListsWithSideboards(p);
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
    engine::az::MtgSpec::parse(json).unwrap()
}

struct PlayedMatch {
    /// The match ended aborted (ENGINE.md 100.7).
    aborted: bool,
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    results: [u8; 3],
    games: u8,
    sideboard_decisions: u32,
    changed_decks: u32,
}

/// Counts of the hidden-pool checks (ENGINE.md 36.11).
#[derive(Default)]
struct HiddenStats {
    worlds: u64,
    first_picks_changed: u64,
    decks_changed: u64,
}

/// A world `determinize(viewer)` sampled (`d`) from match `g`: the viewer's
/// view and own cards unchanged, no card it knows resampled; the opponent's
/// pool its known part plus as many first picks; their deck a legal deck of
/// that pool with the public sizes, holding every card of theirs the viewer
/// knows, fulfilling a revealed companion, and in the game the world's
/// cards; the clouds hold.
fn check_hidden_pool(g: &engine::az::MtgAz, d: &engine::az::MtgAz, viewer: u8, seed: u64, stats: &mut HiddenStats) {
    use engine::az::sample;
    let o = 1 - viewer;
    let (v, ou) = (viewer as usize, o as usize);
    assert_eq!(g.m.pools[v], d.m.pools[v], "match {seed}: the viewer's pool was resampled");
    assert_eq!(g.m.decks[v], d.m.decks[v], "match {seed}: the viewer's deck was resampled");
    assert_eq!(g.g.player(viewer).sideboard, d.g.player(viewer).sideboard, "match {seed}: the viewer's sideboard was resampled");
    let ids = engine::az::identity::identities(g.g.db());
    let (rp, sp) = (&g.m.pools[ou], &d.m.pools[ou]);
    let known = |m: &engine::az::MatchState, pool: &engine::az::matches::MatchPool| -> Vec<(u16, u8)> {
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
        let c = engine::CardIdx::new(i as u32);
        if g.g.knows(viewer, c) {
            assert_eq!((x.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "match {seed}: a card P{viewer} knows was resampled");
        }
    }
    d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
    d.g.check_knowledge().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
}

/// Two worlds sampled for the decider, played with the same actions, give it
/// the same encoding while their keys, next decider and legal actions agree
/// (zero-core's child signature), across games and sideboarding.
fn check_match_information_set(g: &engine::az::MtgAz, viewer: usize, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut zero_search::rng::Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    let (mut xa, mut xb): (Vec<f32>, Vec<f32>);
    for step in 0..25 {
        if a.match_over() || b.match_over() {
            return;
        }
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            assert!(!(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)), "match {seed} step {step}: the viewer's options differ");
            return;
        }
        xa = engine::az::tail_encode::Row::of_az(&a, viewer).flat();
        xb = engine::az::tail_encode::Row::of_az(&b, viewer).flat();
        if let Some(i) = (0..xa.len()).find(|&i| xa[i] != xb[i]) {
            panic!(
                "match {seed} step {step}: same keys, different encodings of P{viewer} at {i} ({} / {}); decisions {:?} / {:?}; game {} sb {}
{}
----
{}",
                xa[i],
                xb[i],
                a.pending_decision(),
                b.pending_decision(),
                a.m.game_no,
                a.sideboarding().is_some(),
                stack_names(&a.g),
                stack_names(&b.g)
            );
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.step(act, &mut ka).unwrap();
        b.step(act, &mut kb).unwrap();
        if ka != kb {
            return;
        }
    }
}

fn stack_names(g: &Game) -> String {
    let db = g.db();
    let mut v: Vec<String> = g
        .stack()
        .iter()
        .map(|it| match it.kind {
            engine::game::StackKind::Spell { card } => format!("spell {} (own {})", db.def(g.obj(card).def).name, db.def(g.own_def(card)).name),
            engine::game::StackKind::Ability { source, .. } => format!("ability of {}", db.def(g.obj(source.idx).def).name),
        })
        .collect();
    for p in 0..2u8 {
        v.push(format!("P{p} battlefield: {:?}", g.objs().iter().filter(|o| o.zone == engine::Zone::Battlefield && o.controller == p).map(|o| db.def(o.def).name.clone()).collect::<Vec<_>>()));
    }
    v.join("
")
}

fn play_match(seed: u64, checks: bool) -> PlayedMatch {
    play_match_stats(seed, checks, &mut HiddenStats::default())
}

fn play_match_stats(seed: u64, checks: bool, stats: &mut HiddenStats) -> PlayedMatch {
    use engine::az::MtgAz;
    use zero_search::game::SearchGame;
    let sp = match_spec(seed);
    let mut g = MtgAz::new_game(&sp, 2, seed).unwrap_or_else(|e| panic!("match {seed}: {e}"));
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 5);
    let (mut legal, mut legal2, mut keys, mut all_keys, mut actions) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut x1: Vec<f32>;
    let mut x2: Vec<f32>;
    let (mut sideboard_decisions, mut changed_decks) = (0u32, 0u32);
    let mut decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
    let mut game_no = 1;
    while let Some(p) = g.to_move() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "match {seed}: no legal action");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "match {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        let sb = g.sideboarding().is_some();
        sideboard_decisions += sb as u32;
        let a = legal[rng.below(legal.len() as u32) as usize];
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
                x1 = engine::az::tail_encode::Row::of_az(&g, viewer).flat();
                x2 = az_row_checked(&d, viewer, &|| format!("match {seed}: a world determinized for P{viewer}"));
                if let Some(i) = (0..x1.len()).find(|&i| x1[i] != x2[i]) {
                    panic!("match {seed}: determinize changed the encoding of P{viewer} (mover P{p}) at {i}: {} -> {}", x1[i], x2[i]);
                }
                check_hidden_pool(&g, &d, viewer as u8, seed, stats);
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
                    x2 = engine::az::tail_encode::Row::of_az(&dw, viewer).flat();
                    if let Some(i) = (0..x1.len()).find(|&i| x1[i] != x2[i]) {
                        panic!("match {seed}: a weighted deal changed the encoding of P{viewer} at {i}");
                    }
                    check_hidden_pool(&g, &dw, viewer as u8, seed, &mut HiddenStats::default());
                }
                // The world plays on.
                let mut kd = Vec::new();
                for _ in 0..30 {
                    if d.to_move().is_none() {
                        break;
                    }
                    d.legal_actions(&mut legal2);
                    let a2 = legal2[rng.below(legal2.len() as u32) as usize];
                    d.step(a2, &mut kd).unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    if d.sideboarding().is_none() && !d.match_over() {
                        d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    }
                }
            }
        }
        g.step(a, &mut keys).unwrap_or_else(|e| panic!("match {seed}: {e}"));
        all_keys.append(&mut keys);
        actions.push(a);
        if g.sideboarding().is_none() && !g.match_over() {
            g.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} step {}: {e}", actions.len()));
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
                // them: ENGINE.md 42.)
                assert_eq!((pool.min_deck, pool.max_deck, deck.len()), (40, 40, 40), "match {seed}: {} cards", deck.len());
                for (k, &(_, n)) in pool.list.iter().enumerate() {
                    assert!(g.m.decks[q as usize][k] <= n.max(decks[q as usize][k]));
                }
                changed_decks += (g.m.decks[q as usize] != decks[q as usize]) as u32;
            }
            decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        }
        assert!(actions.len() < 300_000, "match {seed}: does not end");
    }
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
        changed_decks,
    }
}

#[test]
fn fuzz_random_matches() {
    if constructed_mode() {
        println!("FUZZ_POOL=delver / dd_a / dd_b / dd_c / doomsday / sneak_show / sns: games only (constructed best-of-three isn't built)");
        return;
    }
    if eoe_mode() {
        println!("FUZZ_POOL=eoe: games only (sealed matches come with milestone S2)");
        return;
    }
    if sealed_mode() {
        println!("FUZZ_POOL=sealed: games only (sealed is best-of-one: ENGINE.md 101)");
        return;
    }
    if hidden_decks_mode() {
        println!("FUZZ_HIDDEN_DECKS: games only (hidden decklists are best-of-one)");
        return;
    }
    let games: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n: u64 = std::env::var("FUZZ_MATCHES").ok().and_then(|s| s.parse().ok()).unwrap_or((games / 20).max(20));
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut games_played, mut draws, mut decisions, mut sb, mut changed, mut three) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut aborted = 0u64;
    let mut stats = HiddenStats::default();
    for seed in first..first + n {
        if skipped(seed) {
            continue;
        }
        let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play_match_stats(seed, true, &mut stats)))
            .unwrap_or_else(|e| panic!("match {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_MATCHES=1): {e:?}"));
        games_played += p.games as u64;
        draws += p.results.iter().filter(|&&r| r == 3).count() as u64;
        decisions += p.actions.len() as u64;
        sb += p.sideboard_decisions as u64;
        changed += p.changed_decks as u64;
        three += (p.games == 3) as u64;
        aborted += p.aborted as u64;
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
    // (Over a chunk of matches; a single match's rerun, `FUZZ_MATCHES=1`,
    // passes when the match completes: ENGINE.md 51.)
    assert!(n < 20 || sb > 0 && changed > 0 && three > 0, "no sideboarding, changed deck or match of three");
    // (Interaction lists have no first picks: open decklists.)
    assert!(
        n < 20 || interactions_mode() || stats.first_picks_changed > stats.worlds / 4 && stats.decks_changed > stats.worlds / 4,
        "the hidden pool is rarely resampled"
    );
}

#[test]
fn same_seed_and_actions_replay_matches_identically() {
    use engine::az::MtgAz;
    use zero_search::game::SearchGame;
    if constructed_mode() || eoe_mode() || sealed_mode() {
        return;
    }
    for seed in 2000..2012 {
        let a = play_match(seed, false);
        let mut g = MtgAz::new_game(&match_spec(seed), 2, seed).unwrap();
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.step(x, &mut keys).unwrap();
        }
        assert_eq!((g.g.fingerprint(), g.m.results), (a.fingerprint, a.results), "match {seed}");
        assert_eq!(keys, a.keys, "match {seed}: chance keys differ");
        assert_eq!(play_match(seed, false).fingerprint, a.fingerprint);
    }
}

// ---- FUZZ_HIDDEN_DECKS=1: hidden decklists (ENGINE.md 102) ----

/// `FUZZ_HIDDEN_DECKS=1` (module docs): the games are played through `MtgAz` with hidden
/// decklists.
fn hidden_decks_mode() -> bool {
    std::env::var("FUZZ_HIDDEN_DECKS").is_ok_and(|s| s == "1")
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
    std::env::var("FUZZ_DECK_CHOICE").ok().map(|v| v.parse().expect("FUZZ_DECK_CHOICE: the candidates per seat (2..=32)"))
}

/// The deck-choice spec of game `seed` (`FUZZ_DECK_CHOICE`): `sealed_spec`'s with the choice.
fn choice_spec(seed: u64, k: u8) -> &'static engine::az::MtgSpec {
    static SPECS: std::sync::OnceLock<[engine::az::MtgSpec; 2]> = std::sync::OnceLock::new();
    let specs = SPECS.get_or_init(|| {
        [0u64, 1].map(|s| {
            let mut v: serde_json::Value = serde_json::from_str(&format!(r#"{{"sealed": {}}}"#, sealed_spec(s).sealed.as_deref().unwrap().to_json())).unwrap();
            v["sealed"]["deck_choice"] = serde_json::json!({"k": k, "explore": 0.1});
            // (FUZZ_DECK_CHOICE_BY, ENGINE.md 112: the deck choice by an evaluator file)
            if let Ok(path) = std::env::var("FUZZ_DECK_CHOICE_BY") {
                v["sealed"]["deck_choice"]["by"] = serde_json::json!("evaluator");
                v["sealed"]["deck_choice"]["evaluator"] = serde_json::json!(path);
            }
            engine::az::MtgSpec::parse(&v.to_string()).unwrap()
        })
    });
    &specs[(seed % 2) as usize]
}

/// `FUZZ_DECK_CHOICE_BY` (module docs): each built seat's deck is its explore pick or the evaluator's best
/// candidate, recomputed from the spec; its prior chooses as it does. Nothing without the evaluator.
fn check_evaluator_choice(a: &engine::az::MtgAz, spec: &engine::az::MtgSpec, seed: u64) {
    let sl = spec.sealed.as_deref().unwrap();
    let Some(dc) = sl.deck_choice.as_ref() else { return };
    let Some(e) = dc.evaluator() else { return };
    let ss = spec.sealed_seed(seed);
    assert!(a.deck_choice().is_none(), "seed {seed}: the deck choice by an evaluator is no decision");
    for p in 0..2u8 {
        let (cands, _) = sl.candidates(ss, p).unwrap().expect("a built seat's candidates");
        let how = a.m.how[p as usize].expect("how the deck was made");
        let deck = a.m.deck(p);
        match engine::az::sealed::explore_pick(ss, p, dc.explore.0, dc.k) {
            Some(j) => {
                assert_eq!((how.mode, how.candidate, how.evaluator), (engine::az::sealed::Mode::Explore, Some(j), None), "seed {seed}: P{p} explores");
                assert_eq!(deck, cands[j as usize].deck, "seed {seed}: P{p}'s explore pick");
                DC_EVAL_EXPLORE.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                let scores: Vec<f64> = cands.iter().map(|c| e.scorer.score(&c.deck).unwrap()).collect();
                let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let j = scores.iter().position(|&x| x == best).unwrap();
                assert_eq!((how.mode, how.candidate, how.evaluator), (engine::az::sealed::Mode::Chosen, Some(j as u8), Some(e.scorer.sha256)), "seed {seed}: P{p} chose");
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
fn play_deck_choice(a: &mut engine::az::MtgAz, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let mut zr = zero_search::rng::Rng::new(seed ^ 0xDEC0_C401);
    let (mut b0, mut b1) = (Vec::new(), Vec::new());
    let mut keys = Vec::new();
    while a.deck_choice().is_some() {
        assert!(a.value_choice(), "seed {seed}: a deck choice is a value choice");
        let p = a.to_move().unwrap() as u8;
        let legal = az_legal(a);
        assert!(legal.len() >= 2 && legal.iter().enumerate().all(|(j, &x)| x == engine::act::NUMBER + j as u16), "seed {seed}: the choice's options {legal:?}");
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
                    for _ in 0..30 {
                        if w.to_move().is_none() {
                            break;
                        }
                        let l = az_legal(&w);
                        w.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
                    }
                }
                DC_WORLDS.fetch_add(1, Ordering::Relaxed);
            }
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
fn az_encode(a: &engine::az::MtgAz, viewer: u8, buf: &mut Vec<f32>) {
    *buf = engine::az::tail_encode::Row::of_az(a, viewer as usize).flat();
}

/// `viewer`'s whole row of match `a` as one vector, its values bounded; in a game (not between
/// the games of a match, not the deck choice) the game's row checked (`check_row`).
fn az_row_checked(a: &engine::az::MtgAz, viewer: usize, what: &dyn Fn() -> String) -> Vec<f32> {
    let row = engine::az::tail_encode::Row::of_az(a, viewer);
    let vals: Vec<f32> = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).copied().collect();
    check_bounded(&vals, &|| format!("{} (viewer P{viewer})", what()));
    if a.sideboarding().is_none() && a.deck_choice().is_none() && a.g.pending_decision().is_some() {
        let r = engine::az::tail_encode::Row::of(&a.g, viewer as u8);
        check_row(&a.g, viewer as u8, &r, what);
    }
    row.flat()
}

fn az_legal(a: &engine::az::MtgAz) -> Vec<u16> {
    use zero_search::game::SearchGame;
    let mut l = Vec::new();
    a.legal_actions(&mut l);
    l
}

/// Player `p`'s starting deck's cards (sorted card definitions; the game's objects are the two
/// decks first, P0's then P1's).
fn hd_deck(a: &engine::az::MtgAz, p: u8) -> Vec<u16> {
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if p == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| engine::az::sample::card_of(&a.g, engine::CardIdx::new(i as u32))).collect();
    v.sort_unstable();
    v
}

/// The cards `viewer` has seen of the other player's deck (deck cards it knows exactly, its
/// clouds' identities), sorted: every world's deck holds them.
fn hd_shown(a: &engine::az::MtgAz, viewer: u8) -> Vec<u16> {
    let o = 1 - viewer;
    let g = &a.g;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> =
        r.clone().map(|i| engine::CardIdx::new(i as u32)).filter(|&c| g.knows(viewer, c)).map(|c| engine::az::sample::card_of(g, c)).collect();
    for k in g.clouds().iter().filter(|k| k.viewer == viewer && k.owner == o) {
        v.extend(k.defs.iter().map(|&d| g.db().def(d).card_def(d)));
    }
    v.sort_unstable();
    // (A cloud's identity may be a card of theirs outside their deck, hidden from the viewer: a
    // token in a hidden zone, a companion put into the hand. Not a deck card.)
    for (i, ob) in g.objs().iter().enumerate() {
        let c = engine::CardIdx::new(i as u32);
        if ob.owner == o && !r.contains(&i) && matches!(ob.zone, engine::Zone::Hand | engine::Zone::Library | engine::Zone::FaceDown) && !g.knows(viewer, c) {
            if let Ok(at) = v.binary_search(&engine::az::sample::card_of(g, c)) {
                v.remove(at);
            }
        }
    }
    v
}

/// One game of `FUZZ_HIDDEN_DECKS` (module docs): the mode's decks, played through `MtgAz` with
/// hidden decklists (the table's `TablePrior` for both decks), with the checks of `play` and, at
/// each checkpoint, `check_determinize_hidden`.
fn play_hidden(seed: u64) -> Played {
    use zero_search::game::SearchGame;
    let [(d0, s0), (d1, s1)] = decks(seed);
    let log = std::env::var("FUZZ_LOG").is_ok();
    let gspec = GameSpec { log, hidden_decks: true, hidden_decklists: true, ..GameSpec::default() };
    let seat = |deck: Vec<u16>, sideboard: Vec<u16>| engine::az::Seat { deck, sideboard, first_picks: Vec::new(), elsewhere: Vec::new() };
    // (FUZZ_POOL=sealed: the sealed prior of the game's spec, ENGINE.md 102.6; else the table's stub.)
    let prior: std::sync::Arc<dyn engine::az::hidden_decks::DeckPrior> = match sealed_spec(seed).sealed.as_deref().filter(|_| sealed_mode()) {
        Some(sl) => std::sync::Arc::new(engine::az::hidden_decks::SealedPrior::of_spec(sl)),
        None => engine::az::hidden_decks::TablePrior::of(game_db()),
    };
    // (FUZZ_DECK_CHOICE, ENGINE.md 107.2: the spec's game with its deck choice, made by `MtgAz::new`)
    let choice = deck_choice_fuzz().filter(|_| sealed_mode());
    let mut a = match choice {
        Some(k) => engine::az::MtgAz::new(choice_spec(seed, k), seed).unwrap_or_else(|e| panic!("seed {seed}: {e}")),
        None => engine::az::MtgAz::from_seats(game_db(), gspec, engine::az::Format::Bo1, &[seat(d0, s0), seat(d1, s1)], seed, [Some(prior.clone()), Some(prior)])
            .unwrap_or_else(|e| panic!("seed {seed}: {e}")),
    };
    let _ = log;
    // (The K-sample deal, ENGINE.md 102.8: `FUZZ_DEAL_SAMPLES=k`, default the spec's 32; 0 = a
    // fresh sample of the prior at every deal.)
    a.m.deal.k = std::env::var("FUZZ_DEAL_SAMPLES").ok().map_or(32, |v| v.parse().expect("FUZZ_DEAL_SAMPLES: a number"));
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
    if let Some(k) = choice {
        check_evaluator_choice(&a, choice_spec(seed, k), seed);
        play_deck_choice(&mut a, seed, &mut rng);
    }
    let mut crng = Rng::new(seed ^ 0xC0_4D_1E7E);
    let (mut actions, mut keys) = (Vec::new(), Vec::new());
    let (mut names_named, mut names_seen): (Vec<engine::ids::ObjRef>, Vec<u32>) = (Vec::new(), Vec::new());
    let mut steps = 0usize;
    let (mut turn, mut turn_steps) = (a.g.turn().number, 0u64);
    while !a.g.is_over() {
        let legal = az_legal(&a);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        let act = if turn_steps >= HD_LONG_TURN && legal.contains(&engine::act::PASS) {
            if turn_steps == HD_LONG_TURN {
                HD_LONG_TURNS.fetch_add(1, Ordering::Relaxed);
            }
            engine::act::PASS
        } else if let Some(next) = names_next_page(&a.g, &mut rng) {
            next
        } else {
            legal[rng.below(legal.len() as u32) as usize]
        };
        count_kind(a.g.kind());
        let mover = a.to_move().unwrap() as u8;
        az_row_checked(&a, mover as usize, &|| format!("seed {seed} step {steps}"));
        if a.deck_choice().is_none() {
            let row = engine::az::tail_encode::Row::of(&a.g, mover);
            check_graveyard_list(&a.g, &row, &|| format!("seed {seed} step {steps}"));
        }
        if rng.below(checkpoint_every()) == 0 {
            check_clone(&a.g, act, seed);
            check_determinize_hidden(&a, seed, &mut rng);
        }
        if complete_now(&a.g, &mut crng) {
            check_options_complete(&a.g, seed, "the game");
        }
        a.step(act, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        a.g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}"));
        a.g.check_knowledge().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}"));
        if names_mode() {
            count_names(&a.g, &mut names_named, &mut names_seen);
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
    let g = &a.g;
    let companions = (0..2).filter(|&p| g.player(p).companion.is_some()).count() as u32;
    let companions_used = (0..2).filter(|&p| g.player(p).companion_used).count() as u32;
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number, companions, companions_used }
}

/// `MtgAz::determinize` with hidden decklists, for the decider and the other player (ENGINE.md
/// 102.3): the decider's actions (the other player: the decider still to move), the viewer's view
/// and whole encoding as in the real game, no card it knows dealt again, the opponent's sampled
/// deck of the public size holding every card the viewer has seen and equal to the match's deck of
/// theirs, the viewer's own cards untouched, the invariants and the clouds; every option of the
/// world's decision completes; for the decider, two worlds played with the same actions stay
/// indistinguishable while their keys, next decider and legal actions agree; then 30 random
/// actions in the world.
fn check_determinize_hidden(a: &engine::az::MtgAz, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let mover = a.to_move().unwrap() as u8;
    let (mut b0, mut b1) = (Vec::new(), Vec::new());
    for viewer in [mover, 1 - mover] {
        let o = 1 - viewer;
        let shown = hd_shown(a, viewer);
        let (real, mine) = (hd_deck(a, o), hd_deck(a, viewer));
        let mut d = a.clone();
        let s = rng.next_u64();
        // (FUZZ_DECK_TILT: the deck-tilted deal, ENGINE.md 106; the prediction from its own generator)
        let tilted = deck_tilt_fuzz().map(|t| {
            let mut pr = Rng::new(s ^ 0xDEC_7117);
            let mut seg = || {
                let mut p: Vec<f32> = (0..engine::az::identity::D).map(|_| ((pr.below(1000) + 1) as f32).powi(3)).collect();
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
            let t = engine::az::hidden_decks::DeckTilt { p: engine::az::belief::deck_segment(&b.pred), tilt: b.tilts[1], clip: b.clip };
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
            let c = engine::CardIdx::new(i as u32);
            if a.g.knows(viewer, c) {
                assert_eq!((ob.def, a.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "{}: a card the viewer knows was dealt again", what());
            }
        }
        d.g.check_invariants().unwrap_or_else(|e| panic!("{}: {e}", what()));
        d.g.check_knowledge().unwrap_or_else(|e| panic!("{}: {e}", what()));
        let world = hd_deck(&d, o);
        assert_eq!(world.len(), real.len(), "{}: the deck's size", what());
        let mut left = world.clone();
        for x in &shown {
            match left.binary_search(x) {
                Ok(i) => {
                    left.remove(i);
                }
                Err(_) => panic!("{}: the sampled deck lacks a shown {}", what(), a.g.db().def(*x).name),
            }
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
        for _ in 0..30 {
            if d.to_move().is_none() {
                break;
            }
            let l = az_legal(&d);
            d.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap_or_else(|e| panic!("{}: {e}", what()));
            d.g.check_invariants().unwrap_or_else(|e| panic!("{} (played on): {e}", what()));
        }
    }
}

/// zero-core's child signature on hidden-decklist worlds (`check_information_set`'s, through
/// `MtgAz`): equal keys, next decider and legal actions mean an equal view and encoding.
fn check_hidden_information_set(a: &engine::az::MtgAz, viewer: u8, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let s = rng.next_u64();
    let mut x = a.clone();
    x.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s));
    let mut y = a.clone();
    y.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
    let (mut bx, mut by) = (Vec::new(), Vec::new());
    for step in 0..25 {
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
            return;
        }
    }
}
