//! MTG Zero rules engine. Architecture: `docs/ENGINE.md`.
//!
//! Entry points: [`db::CardDb`] (cards lowered from the IR),
//! [`game::Game::new`], then [`game::Game::pending_decision`] /
//! [`game::Game::legal_actions`] / [`game::Game::apply`] until
//! [`game::Game::outcome`] is set.

mod activate;
pub mod az;
mod cast;
mod cost_order;
mod combat;
mod companion;
pub mod copies;
mod faces;
pub mod db;
pub mod decision;
mod flow;
pub mod game;
pub mod hidden;
pub mod ids;
pub mod lower;
mod loops;
pub mod mana;
pub mod num;
mod pay;
pub mod program;
mod resolve;
mod resolve_ext;
mod resolve_tail;
mod resolve_tail2;
mod resolve_tail3;
mod resolve_m7d;
mod resolve_eoe_a;
mod resolve_eoe_d;
mod resolve_walkers;
mod resolve_eoe_b;
pub mod rng;
mod sba;
mod stack_ext;
mod svec;
pub mod dungeon;
pub mod eoe_c;
pub mod eoe_x;
pub mod eoe_w3;
pub mod eoe_w4;
pub mod eoe_w1;
pub mod eoe_w6;
pub mod doomsday_a;
pub mod eoe_w5;
pub mod doomsday_b;
pub mod doomsday_c;
mod eoe_w2;
pub mod public_state;
pub mod testkit;
pub mod layers;
mod triggers;

pub use decision::{act, Choice, Decision, DecisionKind, NUM_ACTIONS};
pub use game::{Game, GameSpec, Outcome, Step};
pub use ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
pub use triggers::{EvCtx, PendingTrigger};
pub use layers::{Affected, Effect, Until};
