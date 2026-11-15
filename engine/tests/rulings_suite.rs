//! One test binary for the official rulings tests (every `rulings_*.rs`) (docs/ENGINE.md 165): each file a module of it, so the engine is linked once
//! for all 24 instead of once per file. Run one file's tests with its module name as the filter:
//! `cargo test --release -p engine --test rulings_suite rulings_casts::` (was `--test rulings_casts`).
//! A file keeps its own `mod common;` / kit modules: `#[path]` loads it like a crate root, so its modules
//! resolve beside it as before.

#[path = "rulings_casts.rs"]
mod rulings_casts;
#[path = "rulings_combat.rs"]
mod rulings_combat;
#[path = "rulings_copies.rs"]
mod rulings_copies;
#[path = "rulings_copies2.rs"]
mod rulings_copies2;
#[path = "rulings_copies3.rs"]
mod rulings_copies3;
#[path = "rulings_facedown.rs"]
mod rulings_facedown;
#[path = "rulings_faces.rs"]
mod rulings_faces;
#[path = "rulings_faces2.rs"]
mod rulings_faces2;
#[path = "rulings_mana.rs"]
mod rulings_mana;
#[path = "rulings_mana2.rs"]
mod rulings_mana2;
#[path = "rulings_other.rs"]
mod rulings_other;
#[path = "rulings_protection.rs"]
mod rulings_protection;
#[path = "rulings_protection2.rs"]
mod rulings_protection2;
#[path = "rulings_replacement.rs"]
mod rulings_replacement;
#[path = "rulings_replacement2.rs"]
mod rulings_replacement2;
#[path = "rulings_stack.rs"]
mod rulings_stack;
#[path = "rulings_stack2.rs"]
mod rulings_stack2;
#[path = "rulings_stack3.rs"]
mod rulings_stack3;
#[path = "rulings_triggers.rs"]
mod rulings_triggers;
#[path = "rulings_triggers2.rs"]
mod rulings_triggers2;
#[path = "rulings_triggers3.rs"]
mod rulings_triggers3;
#[path = "rulings_walkers.rs"]
mod rulings_walkers;
#[path = "rulings_walkers2.rs"]
mod rulings_walkers2;
#[path = "rulings_zones2.rs"]
mod rulings_zones2;
