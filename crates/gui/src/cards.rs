// code inspired by https://github.com/vincev/freezeout
/// Cards images loading and painting.
use ahash::AHashMap;
use eframe::egui;
use std::sync::LazyLock;

use crate::{Card, Deck, Rank, Suit};

/// The clubs.
// Clubs
const BYTES_2_OF_CLUBS: &[u8] = include_bytes!("assets/2_of_clubs.png");
const BYTES_3_OF_CLUBS: &[u8] = include_bytes!("assets/3_of_clubs.png");
const BYTES_4_OF_CLUBS: &[u8] = include_bytes!("assets/4_of_clubs.png");
const BYTES_5_OF_CLUBS: &[u8] = include_bytes!("assets/5_of_clubs.png");
const BYTES_6_OF_CLUBS: &[u8] = include_bytes!("assets/6_of_clubs.png");
const BYTES_7_OF_CLUBS: &[u8] = include_bytes!("assets/7_of_clubs.png");
const BYTES_8_OF_CLUBS: &[u8] = include_bytes!("assets/8_of_clubs.png");
const BYTES_9_OF_CLUBS: &[u8] = include_bytes!("assets/9_of_clubs.png");
const BYTES_10_OF_CLUBS: &[u8] = include_bytes!("assets/10_of_clubs.png");
const BYTES_JACK_OF_CLUBS: &[u8] = include_bytes!("assets/jack_of_clubs.png");
const BYTES_QUEEN_OF_CLUBS: &[u8] = include_bytes!("assets/queen_of_clubs.png");
const BYTES_KING_OF_CLUBS: &[u8] = include_bytes!("assets/king_of_clubs.png");
const BYTES_ACE_OF_CLUBS: &[u8] = include_bytes!("assets/ace_of_clubs.png");

// Diamonds
const BYTES_2_OF_DIAMONDS: &[u8] = include_bytes!("assets/2_of_diamonds.png");
const BYTES_3_OF_DIAMONDS: &[u8] = include_bytes!("assets/3_of_diamonds.png");
const BYTES_4_OF_DIAMONDS: &[u8] = include_bytes!("assets/4_of_diamonds.png");
const BYTES_5_OF_DIAMONDS: &[u8] = include_bytes!("assets/5_of_diamonds.png");
const BYTES_6_OF_DIAMONDS: &[u8] = include_bytes!("assets/6_of_diamonds.png");
const BYTES_7_OF_DIAMONDS: &[u8] = include_bytes!("assets/7_of_diamonds.png");
const BYTES_8_OF_DIAMONDS: &[u8] = include_bytes!("assets/8_of_diamonds.png");
const BYTES_9_OF_DIAMONDS: &[u8] = include_bytes!("assets/9_of_diamonds.png");
const BYTES_10_OF_DIAMONDS: &[u8] = include_bytes!("assets/10_of_diamonds.png");
const BYTES_JACK_OF_DIAMONDS: &[u8] = include_bytes!("assets/jack_of_diamonds.png");
const BYTES_QUEEN_OF_DIAMONDS: &[u8] = include_bytes!("assets/queen_of_diamonds.png");
const BYTES_KING_OF_DIAMONDS: &[u8] = include_bytes!("assets/king_of_diamonds.png");
const BYTES_ACE_OF_DIAMONDS: &[u8] = include_bytes!("assets/ace_of_diamonds.png");

// Hearts
const BYTES_2_OF_HEARTS: &[u8] = include_bytes!("assets/2_of_hearts.png");
const BYTES_3_OF_HEARTS: &[u8] = include_bytes!("assets/3_of_hearts.png");
const BYTES_4_OF_HEARTS: &[u8] = include_bytes!("assets/4_of_hearts.png");
const BYTES_5_OF_HEARTS: &[u8] = include_bytes!("assets/5_of_hearts.png");
const BYTES_6_OF_HEARTS: &[u8] = include_bytes!("assets/6_of_hearts.png");
const BYTES_7_OF_HEARTS: &[u8] = include_bytes!("assets/7_of_hearts.png");
const BYTES_8_OF_HEARTS: &[u8] = include_bytes!("assets/8_of_hearts.png");
const BYTES_9_OF_HEARTS: &[u8] = include_bytes!("assets/9_of_hearts.png");
const BYTES_10_OF_HEARTS: &[u8] = include_bytes!("assets/10_of_hearts.png");
const BYTES_JACK_OF_HEARTS: &[u8] = include_bytes!("assets/jack_of_hearts.png");
const BYTES_QUEEN_OF_HEARTS: &[u8] = include_bytes!("assets/queen_of_hearts.png");
const BYTES_KING_OF_HEARTS: &[u8] = include_bytes!("assets/king_of_hearts.png");
const BYTES_ACE_OF_HEARTS: &[u8] = include_bytes!("assets/ace_of_hearts.png");

// Spades
const BYTES_2_OF_SPADES: &[u8] = include_bytes!("assets/2_of_spades.png");
const BYTES_3_OF_SPADES: &[u8] = include_bytes!("assets/3_of_spades.png");
const BYTES_4_OF_SPADES: &[u8] = include_bytes!("assets/4_of_spades.png");
const BYTES_5_OF_SPADES: &[u8] = include_bytes!("assets/5_of_spades.png");
const BYTES_6_OF_SPADES: &[u8] = include_bytes!("assets/6_of_spades.png");
const BYTES_7_OF_SPADES: &[u8] = include_bytes!("assets/7_of_spades.png");
const BYTES_8_OF_SPADES: &[u8] = include_bytes!("assets/8_of_spades.png");
const BYTES_9_OF_SPADES: &[u8] = include_bytes!("assets/9_of_spades.png");
const BYTES_10_OF_SPADES: &[u8] = include_bytes!("assets/10_of_spades.png");
const BYTES_JACK_OF_SPADES: &[u8] = include_bytes!("assets/jack_of_spades.png");
const BYTES_QUEEN_OF_SPADES: &[u8] = include_bytes!("assets/queen_of_spades.png");
const BYTES_KING_OF_SPADES: &[u8] = include_bytes!("assets/king_of_spades.png");
const BYTES_ACE_OF_SPADES: &[u8] = include_bytes!("assets/ace_of_spades.png");
/// A collection of cards textures used for drawing.
pub struct Textures {
    cards: AHashMap<Card, egui::TextureHandle>,
    back: egui::TextureHandle,
}

impl Textures {
    /// Loads the cards textures.
    pub fn new(ctx: &egui::Context) -> Self {
        let cards = CARD_IMAGES
            .iter()
            .map(|(card, image_data)| {
                (
                    *card,
                    ctx.load_texture(
                        card.to_string(),
                        image_from_memory(image_data),
                        Default::default(),
                    ),
                )
            })
            .collect();

        let back = ctx.load_texture("back", image_from_memory(BYTES_BB), Default::default());

        Self { cards, back }
    }

    /// Gets a texture for a card.
    pub fn card(&self, card: Card) -> egui::TextureHandle {
        self.cards.get(&card).unwrap().clone()
    }

    /// Gets a texture for a hole card.
    pub fn back(&self) -> egui::TextureHandle {
        self.back.clone()
    }
}

fn image_from_memory(image_data: &[u8]) -> egui::ColorImage {
    let image = image::load_from_memory(image_data).unwrap();
    let size = [image.width() as _, image.height() as _];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice())
}