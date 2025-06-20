// code inspired by https://github.com/vincev/freezeout
//! poker server.
#![warn(clippy::all, rust_2018_idioms, missing_docs)]

pub mod db;

pub mod server;
// pub use server::{run, Config};

pub mod table;
pub mod tables_pool;
