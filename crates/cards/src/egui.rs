// code taken from https://github.com/vincev/freezeout
// TODO: duplicate code? cards.rs

//! Cards images loading and painting.
use std::sync::LazyLock;

use ahash::AHashMap;
use eframe::egui;
use eframe::egui::Context;

use crate::deck::{Card, Deck, Rank, Suit};

// clubs.
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
const BYTES_JACK_OF_DIAMONDS: &[u8] =
    include_bytes!("assets/jack_of_diamonds.png");
const BYTES_QUEEN_OF_DIAMONDS: &[u8] =
    include_bytes!("assets/queen_of_diamonds.png");
const BYTES_KING_OF_DIAMONDS: &[u8] =
    include_bytes!("assets/king_of_diamonds.png");
const BYTES_ACE_OF_DIAMONDS: &[u8] =
    include_bytes!("assets/ace_of_diamonds.png");

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
const BYTES_QUEEN_OF_HEARTS: &[u8] =
    include_bytes!("assets/queen_of_hearts.png");
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
const BYTES_QUEEN_OF_SPADES: &[u8] =
    include_bytes!("assets/queen_of_spades.png");
const BYTES_KING_OF_SPADES: &[u8] = include_bytes!("assets/king_of_spades.png");
const BYTES_ACE_OF_SPADES: &[u8] = include_bytes!("assets/ace_of_spades.png");
/// The cards back.
const BYTES_CARD_BACK: &[u8] = include_bytes!("assets/card_back.png");

static CARD_IMAGES: LazyLock<AHashMap<Card, &'static [u8],>,> =
    LazyLock::new(|| {
        let mut cards = AHashMap::with_capacity(Deck::SIZE,);

        cards.insert(Card::new(Rank::Deuce, Suit::Clubs,), BYTES_2_OF_CLUBS,);
        cards.insert(Card::new(Rank::Trey, Suit::Clubs,), BYTES_3_OF_CLUBS,);
        cards.insert(Card::new(Rank::Four, Suit::Clubs,), BYTES_4_OF_CLUBS,);
        cards.insert(Card::new(Rank::Five, Suit::Clubs,), BYTES_5_OF_CLUBS,);
        cards.insert(Card::new(Rank::Six, Suit::Clubs,), BYTES_6_OF_CLUBS,);
        cards.insert(Card::new(Rank::Seven, Suit::Clubs,), BYTES_7_OF_CLUBS,);
        cards.insert(Card::new(Rank::Eight, Suit::Clubs,), BYTES_8_OF_CLUBS,);
        cards.insert(Card::new(Rank::Nine, Suit::Clubs,), BYTES_9_OF_CLUBS,);
        cards.insert(Card::new(Rank::Ten, Suit::Clubs,), BYTES_10_OF_CLUBS,);
        cards.insert(Card::new(Rank::Jack, Suit::Clubs,), BYTES_JACK_OF_CLUBS,);
        cards.insert(Card::new(Rank::Queen, Suit::Clubs), BYTES_QUEEN_OF_CLUBS);
        cards.insert(Card::new(Rank::King, Suit::Clubs,), BYTES_KING_OF_CLUBS,);
        cards.insert(Card::new(Rank::Ace, Suit::Clubs,), BYTES_ACE_OF_CLUBS,);

        cards.insert(
            Card::new(Rank::Deuce, Suit::Diamonds,),
            BYTES_2_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Trey, Suit::Diamonds,),
            BYTES_3_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Four, Suit::Diamonds,),
            BYTES_4_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Five, Suit::Diamonds,),
            BYTES_5_OF_DIAMONDS,
        );
        cards.insert(Card::new(Rank::Six, Suit::Diamonds), BYTES_6_OF_DIAMONDS);
        cards.insert(
            Card::new(Rank::Seven, Suit::Diamonds,),
            BYTES_7_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Eight, Suit::Diamonds,),
            BYTES_8_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Nine, Suit::Diamonds,),
            BYTES_9_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Ten, Suit::Diamonds,),
            BYTES_10_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Jack, Suit::Diamonds,),
            BYTES_JACK_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Queen, Suit::Diamonds,),
            BYTES_QUEEN_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::King, Suit::Diamonds,),
            BYTES_KING_OF_DIAMONDS,
        );
        cards.insert(
            Card::new(Rank::Ace, Suit::Diamonds,),
            BYTES_ACE_OF_DIAMONDS,
        );

        cards.insert(Card::new(Rank::Deuce, Suit::Hearts,), BYTES_2_OF_HEARTS,);
        cards.insert(Card::new(Rank::Trey, Suit::Hearts,), BYTES_3_OF_HEARTS,);
        cards.insert(Card::new(Rank::Four, Suit::Hearts,), BYTES_4_OF_HEARTS,);
        cards.insert(Card::new(Rank::Five, Suit::Hearts,), BYTES_5_OF_HEARTS,);
        cards.insert(Card::new(Rank::Six, Suit::Hearts,), BYTES_6_OF_HEARTS,);
        cards.insert(Card::new(Rank::Seven, Suit::Hearts,), BYTES_7_OF_HEARTS,);
        cards.insert(Card::new(Rank::Eight, Suit::Hearts,), BYTES_8_OF_HEARTS,);
        cards.insert(Card::new(Rank::Nine, Suit::Hearts,), BYTES_9_OF_HEARTS,);
        cards.insert(Card::new(Rank::Ten, Suit::Hearts,), BYTES_10_OF_HEARTS,);
        cards.insert(Card::new(Rank::Jack, Suit::Hearts), BYTES_JACK_OF_HEARTS);
        cards.insert(
            Card::new(Rank::Queen, Suit::Hearts,),
            BYTES_QUEEN_OF_HEARTS,
        );
        cards.insert(Card::new(Rank::King, Suit::Hearts), BYTES_KING_OF_HEARTS);
        cards.insert(Card::new(Rank::Ace, Suit::Hearts,), BYTES_ACE_OF_HEARTS,);

        cards.insert(Card::new(Rank::Deuce, Suit::Spades,), BYTES_2_OF_SPADES,);
        cards.insert(Card::new(Rank::Trey, Suit::Spades,), BYTES_3_OF_SPADES,);
        cards.insert(Card::new(Rank::Four, Suit::Spades,), BYTES_4_OF_SPADES,);
        cards.insert(Card::new(Rank::Five, Suit::Spades,), BYTES_5_OF_SPADES,);
        cards.insert(Card::new(Rank::Six, Suit::Spades,), BYTES_6_OF_SPADES,);
        cards.insert(Card::new(Rank::Seven, Suit::Spades,), BYTES_7_OF_SPADES,);
        cards.insert(Card::new(Rank::Eight, Suit::Spades,), BYTES_8_OF_SPADES,);
        cards.insert(Card::new(Rank::Nine, Suit::Spades,), BYTES_9_OF_SPADES,);
        cards.insert(Card::new(Rank::Ten, Suit::Spades,), BYTES_10_OF_SPADES,);
        cards.insert(Card::new(Rank::Jack, Suit::Spades), BYTES_JACK_OF_SPADES);
        cards.insert(
            Card::new(Rank::Queen, Suit::Spades,),
            BYTES_QUEEN_OF_SPADES,
        );
        cards.insert(Card::new(Rank::King, Suit::Spades), BYTES_KING_OF_SPADES);
        cards.insert(Card::new(Rank::Ace, Suit::Spades,), BYTES_ACE_OF_SPADES,);
        cards
    },);

/// A collection of cards textures used for drawing.
pub struct Textures {
    cards: AHashMap<Card, egui::TextureHandle,>,
    back:  egui::TextureHandle,
}

impl Textures {
    /// Loads the cards textures.
    pub fn new(ctx: &Context,) -> Self {
        let cards = CARD_IMAGES
            .iter()
            .map(|(card, image_data,)| {
                (
                    *card,
                    ctx.load_texture(
                        card.to_string(),
                        image_from_memory(image_data,),
                        Default::default(),
                    ),
                )
            },)
            .collect();

        let back = ctx.load_texture(
            "back",
            image_from_memory(BYTES_CARD_BACK,),
            Default::default(),
        );

        Self { cards, back, }
    }

    /// Gets a texture for a card.
    pub fn card(&self, card: Card,) -> egui::TextureHandle {
        self.cards.get(&card,).unwrap().clone()
    }

    /// Gets a texture for a hole card.
    pub fn back(&self,) -> egui::TextureHandle {
        self.back.clone()
    }
}

fn image_from_memory(image_data: &[u8],) -> egui::ColorImage {
    let image = image::load_from_memory(image_data,).unwrap();
    let size = [image.width() as _, image.height() as _,];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice(),)
}
