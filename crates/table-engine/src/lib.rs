//! Pure game-logic for a Texas-Hold'em table.  No networking, no storage.

pub mod engine;
pub mod player;
pub mod players_state;

pub use engine::{EngineCallbacks, InternalTableState};
