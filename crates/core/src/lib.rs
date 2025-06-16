// copied from https://github.com/vincev/freezeout
// SPDX-License-Identifier: Apache-2.0

//! zkpoker core types shared by peers.
#![warn(clippy::all, rust_2018_idioms, missing_docs)]

#[cfg(feature = "connection")]
pub mod connection;
pub mod crypto;
pub mod game_state;
pub mod message;
pub mod poker;

