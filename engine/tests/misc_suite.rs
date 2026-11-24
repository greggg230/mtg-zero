//! One test binary for the other engine test files (AZ layer, decks, drafts, interactions, views, ...) (docs/ENGINE.md 167.9): each file a module of it, so the engine is linked once
//! for all 27 instead of once per file. Run one file's tests with its module name as the filter:
//! `cargo test --release -p engine --test misc_suite autopass_reuse::` (was `--test autopass_reuse`).
//! A file keeps its own `mod common;` / kit modules: `#[path]` loads it like a crate root, so its modules
//! resolve beside it as before.

#[path = "autopass_reuse.rs"]
mod autopass_reuse;
#[path = "bo3_episodes.rs"]
mod bo3_episodes;
#[path = "bots.rs"]
mod bots;
#[path = "build.rs"]
mod build;
#[path = "constructed.rs"]
mod constructed;
#[path = "constructed_bo3.rs"]
mod constructed_bo3;
#[path = "dealt_around.rs"]
mod dealt_around;
#[path = "dealt_integrated.rs"]
mod dealt_integrated;
#[path = "dealt_per_seat.rs"]
mod dealt_per_seat;
#[path = "deck_posterior.rs"]
mod deck_posterior;
#[path = "determinize_consistency.rs"]
mod determinize_consistency;
#[path = "draft_deferred.rs"]
mod draft_deferred;
#[path = "draft_matches.rs"]
mod draft_matches;
#[path = "eoe_net.rs"]
mod eoe_net;
#[path = "farm_fuzz_seeds.rs"]
mod farm_fuzz_seeds;
#[path = "forced_find.rs"]
mod forced_find;
#[path = "hidden_refs.rs"]
mod hidden_refs;
#[path = "interactions_copies.rs"]
mod interactions_copies;
#[path = "interactions_costs.rs"]
mod interactions_costs;
#[path = "interactions_faces.rs"]
mod interactions_faces;
#[path = "interactions_zones.rs"]
mod interactions_zones;
#[path = "known_face_key.rs"]
mod known_face_key;
#[path = "lab.rs"]
mod lab;
#[path = "match_worlds.rs"]
mod match_worlds;
#[path = "measure_cast_mana.rs"]
mod measure_cast_mana;
#[path = "merge_key.rs"]
mod merge_key;
#[path = "metagame.rs"]
mod metagame;
#[path = "pair_sideboarding.rs"]
mod pair_sideboarding;
#[path = "post_board.rs"]
mod post_board;
#[path = "sealed.rs"]
mod sealed;
#[path = "smoke.rs"]
mod smoke;
#[path = "v7_tails.rs"]
mod v7_tails;
#[path = "view.rs"]
mod view;
