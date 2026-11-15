//! One test binary for the other engine test files (AZ layer, decks, drafts, interactions, views, ...) (docs/ENGINE.md 165): each file a module of it, so the engine is linked once
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
#[path = "constructed.rs"]
mod constructed;
#[path = "constructed_bo3.rs"]
mod constructed_bo3;
#[path = "dealt_integrated.rs"]
mod dealt_integrated;
#[path = "deck_choice.rs"]
mod deck_choice;
#[path = "deck_posterior.rs"]
mod deck_posterior;
#[path = "deck_scorer.rs"]
mod deck_scorer;
#[path = "draft_deferred.rs"]
mod draft_deferred;
#[path = "draft_matches.rs"]
mod draft_matches;
#[path = "eoe_net.rs"]
mod eoe_net;
#[path = "farm_fuzz_seeds.rs"]
mod farm_fuzz_seeds;
#[path = "game_log.rs"]
mod game_log;
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
