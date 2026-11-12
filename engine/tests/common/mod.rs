//! Shared fixtures: the cube plus custom test cards written as card IR, so
//! they go through the same lowering as real cards. They live in the engine library
//! (`engine::fuzz::cards`, with the fuzz loops: docs/ENGINE.md 114.1); this module re-exports them.

#![allow(dead_code)]
#![allow(unused_imports)]

pub use engine::fuzz::cards::{custom_cards, db, fuzz_pool, random_deck};
