//! One test binary for the card tests (`cards.rs` and every `cards_*.rs`) (docs/ENGINE.md 167.9): each file a module of it, so the engine is linked once
//! for all 72 instead of once per file. Run one file's tests with its module name as the filter:
//! `cargo test --release -p engine --test cards_suite cards::` (was `--test cards`).
//! A file keeps its own `mod common;` / kit modules: `#[path]` loads it like a crate root, so its modules
//! resolve beside it as before.

#[path = "cards.rs"]
mod cards;
#[path = "cards_delver_sb.rs"]
mod cards_delver_sb;
#[path = "cards_doomsday_a.rs"]
mod cards_doomsday_a;
#[path = "cards_doomsday_b.rs"]
mod cards_doomsday_b;
#[path = "cards_doomsday_c.rs"]
mod cards_doomsday_c;
#[path = "cards_eoe_a.rs"]
mod cards_eoe_a;
#[path = "cards_eoe_a_auras.rs"]
mod cards_eoe_a_auras;
#[path = "cards_eoe_a_cards.rs"]
mod cards_eoe_a_cards;
#[path = "cards_eoe_a_more.rs"]
mod cards_eoe_a_more;
#[path = "cards_eoe_a_ships.rs"]
mod cards_eoe_a_ships;
#[path = "cards_eoe_b.rs"]
mod cards_eoe_b;
#[path = "cards_eoe_b_base.rs"]
mod cards_eoe_b_base;
#[path = "cards_eoe_b_base2.rs"]
mod cards_eoe_b_base2;
#[path = "cards_eoe_c.rs"]
mod cards_eoe_c;
#[path = "cards_eoe_cross.rs"]
mod cards_eoe_cross;
#[path = "cards_eoe_cross_cards.rs"]
mod cards_eoe_cross_cards;
#[path = "cards_eoe_cross_more.rs"]
mod cards_eoe_cross_more;
#[path = "cards_eoe_d.rs"]
mod cards_eoe_d;
#[path = "cards_eoe_irfix.rs"]
mod cards_eoe_irfix;
#[path = "cards_eoe_irfix_lands.rs"]
mod cards_eoe_irfix_lands;
#[path = "cards_eoe_striations.rs"]
mod cards_eoe_striations;
#[path = "cards_eoe_w1.rs"]
mod cards_eoe_w1;
#[path = "cards_eoe_w2.rs"]
mod cards_eoe_w2;
#[path = "cards_eoe_w2_lands.rs"]
mod cards_eoe_w2_lands;
#[path = "cards_eoe_w3.rs"]
mod cards_eoe_w3;
#[path = "cards_eoe_w4.rs"]
mod cards_eoe_w4;
#[path = "cards_eoe_w5.rs"]
mod cards_eoe_w5;
#[path = "cards_eoe_w6.rs"]
mod cards_eoe_w6;
#[path = "cards_eoe_w6_counters.rs"]
mod cards_eoe_w6_counters;
#[path = "cards_eoe_w6_granted.rs"]
mod cards_eoe_w6_granted;
#[path = "cards_eoe_w6_mana.rs"]
mod cards_eoe_w6_mana;
#[path = "cards_eoe_w6_top.rs"]
mod cards_eoe_w6_top;
#[path = "cards_hogaak_c.rs"]
mod cards_hogaak_c;
#[path = "cards_legacy.rs"]
mod cards_legacy;
#[path = "cards_m4b1.rs"]
mod cards_m4b1;
#[path = "cards_m4b2.rs"]
mod cards_m4b2;
#[path = "cards_m7a.rs"]
mod cards_m7a;
#[path = "cards_m7a_b12.rs"]
mod cards_m7a_b12;
#[path = "cards_m7a_b13.rs"]
mod cards_m7a_b13;
#[path = "cards_m7a_b14.rs"]
mod cards_m7a_b14;
#[path = "cards_m7a_costs.rs"]
mod cards_m7a_costs;
#[path = "cards_m7a_misc.rs"]
mod cards_m7a_misc;
#[path = "cards_m7a_more.rs"]
mod cards_m7a_more;
#[path = "cards_m7a_threats.rs"]
mod cards_m7a_threats;
#[path = "cards_m7a_threats2.rs"]
mod cards_m7a_threats2;
#[path = "cards_m7a_x.rs"]
mod cards_m7a_x;
#[path = "cards_m7a_zones.rs"]
mod cards_m7a_zones;
#[path = "cards_m7a_zones2.rs"]
mod cards_m7a_zones2;
#[path = "cards_m7b_copies.rs"]
mod cards_m7b_copies;
#[path = "cards_m7b_copies2.rs"]
mod cards_m7b_copies2;
#[path = "cards_m7b_faces.rs"]
mod cards_m7b_faces;
#[path = "cards_m7b_faces2.rs"]
mod cards_m7b_faces2;
#[path = "cards_m7b_faces3.rs"]
mod cards_m7b_faces3;
#[path = "cards_m7b_faces4.rs"]
mod cards_m7b_faces4;
#[path = "cards_m7b_stack.rs"]
mod cards_m7b_stack;
#[path = "cards_m7b_stack2.rs"]
mod cards_m7b_stack2;
#[path = "cards_m7b_stack3.rs"]
mod cards_m7b_stack3;
#[path = "cards_m7b_tests_creatures.rs"]
mod cards_m7b_tests_creatures;
#[path = "cards_m7b_tests_creatures2.rs"]
mod cards_m7b_tests_creatures2;
#[path = "cards_m7b_tests_edges.rs"]
mod cards_m7b_tests_edges;
#[path = "cards_m7b_tests_lands.rs"]
mod cards_m7b_tests_lands;
#[path = "cards_m7b_tests_mana.rs"]
mod cards_m7b_tests_mana;
#[path = "cards_m7b_tests_spells.rs"]
mod cards_m7b_tests_spells;
#[path = "cards_m7b_walkers.rs"]
mod cards_m7b_walkers;
#[path = "cards_m7b_walkers2.rs"]
mod cards_m7b_walkers2;
#[path = "cards_m7b_walkers3.rs"]
mod cards_m7b_walkers3;
#[path = "cards_m7b_walkers_audit.rs"]
mod cards_m7b_walkers_audit;
#[path = "cards_m7c_tail.rs"]
mod cards_m7c_tail;
#[path = "cards_m7c_tail2.rs"]
mod cards_m7c_tail2;
#[path = "cards_m7c_tail3.rs"]
mod cards_m7c_tail3;
#[path = "cards_m7d.rs"]
mod cards_m7d;
#[path = "cards_sneak_show.rs"]
mod cards_sneak_show;
