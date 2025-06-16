mod deck;
pub use deck::{Card, Deck, Rank, Suit};

#[cfg(feature = "egui")]
pub mod egui;
