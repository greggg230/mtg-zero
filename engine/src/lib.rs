//! MTG Zero rules engine. Architecture: `docs/ENGINE.md`.
//!
//! Entry points: [`db::CardDb`] (cards lowered from the IR),
//! [`game::Game::new`], then [`game::Game::pending_decision`] /
//! [`game::Game::legal_actions`] / [`game::Game::apply`] until
//! [`game::Game::outcome`] is set.

mod activate;
mod cast;
mod combat;
pub mod db;
pub mod decision;
mod flow;
pub mod game;
pub mod hidden;
pub mod ids;
pub mod lower;
pub mod mana;
mod pay;
pub mod program;
mod resolve;
pub mod rng;
mod sba;
pub mod testkit;
mod triggers;

pub use decision::{act, Choice, Decision, DecisionKind, NUM_ACTIONS};
pub use game::{Game, GameSpec, Outcome, Step};
pub use ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
pub use triggers::PendingTrigger;
