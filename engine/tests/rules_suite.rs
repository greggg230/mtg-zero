//! One test binary for the comprehensive-rules tests (every `rules_*.rs`) (docs/ENGINE.md 167.9): each file a module of it, so the engine is linked once
//! for all 36 instead of once per file. Run one file's tests with its module name as the filter:
//! `cargo test --release -p engine --test rules_suite rules_activated::` (was `--test rules_activated`).
//! A file keeps its own `mod common;` / kit modules: `#[path]` loads it like a crate root, so its modules
//! resolve beside it as before.

#[path = "rules_activated.rs"]
mod rules_activated;
#[path = "rules_castpay.rs"]
mod rules_castpay;
#[path = "rules_chance.rs"]
mod rules_chance;
#[path = "rules_colorless_pips.rs"]
mod rules_colorless_pips;
#[path = "rules_combat.rs"]
mod rules_combat;
#[path = "rules_copies.rs"]
mod rules_copies;
#[path = "rules_costs.rs"]
mod rules_costs;
#[path = "rules_f7_ranges.rs"]
mod rules_f7_ranges;
#[path = "rules_faces.rs"]
mod rules_faces;
#[path = "rules_faithful_a.rs"]
mod rules_faithful_a;
#[path = "rules_faithful_c.rs"]
mod rules_faithful_c;
#[path = "rules_faithful_c2.rs"]
mod rules_faithful_c2;
#[path = "rules_faithful_e.rs"]
mod rules_faithful_e;
#[path = "rules_farm.rs"]
mod rules_farm;
#[path = "rules_farm2.rs"]
mod rules_farm2;
#[path = "rules_float.rs"]
mod rules_float;
#[path = "rules_initiative.rs"]
mod rules_initiative;
#[path = "rules_lands.rs"]
mod rules_lands;
#[path = "rules_layers.rs"]
mod rules_layers;
#[path = "rules_library.rs"]
mod rules_library;
#[path = "rules_lki_mana_value.rs"]
mod rules_lki_mana_value;
#[path = "rules_loops.rs"]
mod rules_loops;
#[path = "rules_m8a.rs"]
mod rules_m8a;
#[path = "rules_m8b.rs"]
mod rules_m8b;
#[path = "rules_m8b_hidden.rs"]
mod rules_m8b_hidden;
#[path = "rules_numbers.rs"]
mod rules_numbers;
#[path = "rules_pay_fixes.rs"]
mod rules_pay_fixes;
#[path = "rules_payment.rs"]
mod rules_payment;
#[path = "rules_payment_b.rs"]
mod rules_payment_b;
#[path = "rules_priority.rs"]
mod rules_priority;
#[path = "rules_sba.rs"]
mod rules_sba;
#[path = "rules_setup.rs"]
mod rules_setup;
#[path = "rules_shortcuts.rs"]
mod rules_shortcuts;
#[path = "rules_stack.rs"]
mod rules_stack;
#[path = "rules_triggers.rs"]
mod rules_triggers;
#[path = "rules_walkers.rs"]
mod rules_walkers;
#[path = "rules_ward.rs"]
mod rules_ward;
