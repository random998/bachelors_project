#![warn(clippy::all, rust_2018_idioms)]
pub mod account_view;
pub use account_view::AccountView;

pub mod connect_view;
pub use connect_view::ConnectView;
pub mod game_view;
pub use game_view::GameView;

pub mod gui;
pub use gui::{App, AppData, AppFrame, View};
