// code inspired by https://github.com/vincev/freezeout
//! poker server.
#![warn(clippy::all, rust_2018_idioms, missing_docs)]

extern crate ahash;
extern crate anyhow;
extern crate log;
extern crate poker_core;
extern crate rand;
extern crate thiserror;
extern crate tokio;
pub mod db;

pub mod server;
pub use server::{run, Config};

pub mod table;
pub mod tables_pool;
