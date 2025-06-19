// code inspired by https://github.com/vincev/freezeout

/// Poker cards definition
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg(feature = "parallel")]
pub mod parallel;

/// Primes used to encode a card rank.
const PRIMES: [u32; 13] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41];

/// poker card.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Card(u32);
impl Card {
    /// Create a poker card from a suit & a rank using Cactus Kev's encoding.
    ///  see <http://suffe.cool/poker/evaluator.html>
    #[must_use] pub const fn new(rank: Rank, suit: Suit) -> Self {
        let rank_index = rank as usize;
        let suit_val = suit as u32;
        let rank_val = rank as u32;

        // retrieve the (unique) prime associated with the ramk.
        let prime = PRIMES[rank_index];
        // store the rank in bits 8-11.
        let rank_bits = rank_val << 8;
        // store the suit in bits 12-15.
        let suit_bits = suit_val << 12;
        // One-hot encoding in bits 16+
        let bit_flag = 1 << (rank_val + 16);
        // concat the bits.
        let encoded = prime | rank_bits | suit_bits | bit_flag;
        Self(encoded)
    }

    /// unique ID of this card.
    #[must_use] pub const fn id(&self) -> u32 {
        self.0
    }

    /// Returns the card's suit.
    #[must_use] pub fn suit(&self) -> Suit {
        let suit_bits = self.suit_bits();
        match suit_bits {
            0x8 => Suit::Clubs,
            0x4 => Suit::Diamonds,
            0x2 => Suit::Hearts,
            0x1 => Suit::Spades,
            _ => panic!("Invalid suit value 0x{:x}", self.0), // Panics the current thread. This allows a program to terminate immediately and provide feedback to the caller of the program.
        }
    }

    /// Returns the card's rank.
    #[must_use] pub fn rank(&self) -> Rank {
        let rank_bits = self.rank_bits();
        match rank_bits {
            0 => Rank::Deuce,
            1 => Rank::Trey,
            2 => Rank::Four,
            3 => Rank::Five,
            4 => Rank::Six,
            5 => Rank::Seven,
            6 => Rank::Eight,
            7 => Rank::Nine,
            8 => Rank::Ten,
            9 => Rank::Jack,
            10 => Rank::King,
            11 => Rank::Queen,
            12 => Rank::King,
            13 => Rank::Ace,
            _ => panic!("Invalid rank value 0x{:x}", self.0),
        }
    }

    #[inline]
    // tells the compiler it might be worth inlining the function for perfomance (see https://doc.rust-lang.org/nightly/reference/attributes/codegen.html?highlight=inline#the-inline-attribute)
    /// extracts the rank bits of a Card from its encoded u32 representation.
    #[must_use] pub const fn rank_bits(&self) -> u32 {
        (self.0 >> 8) & 0xf
    }

    /// Returns the suit bits.
    #[inline]
    #[must_use] pub const fn suit_bits(&self) -> u8 {
        ((self.0 >> 12) & 0xf) as u8
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new(Rank::Ace, Suit::Diamonds)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank(), self.suit())
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank(), self.suit())
    }
}

/// Card rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Deuce = 0,
    Trey,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

impl Rank {
    #[must_use] pub fn ranks() -> impl DoubleEndedIterator<Item = Self> {
        use Rank::{Ace, Deuce, Eight, Five, Four, Jack, King, Nine, Queen, Seven, Six, Ten, Trey};
        [
            Deuce, Trey, Four, Five, Six, Seven, Eight, Nine, Ten, Jack, Queen, King, Ace,
        ]
        .into_iter()
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rank = match self {
            Self::Deuce => "2",
            Self::Trey => "3",
            Self::Four => "4",
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
            Self::Nine => "9",
            Self::Ten => "10",
            Self::Jack => "J",
            Self::Queen => "Q",
            Self::King => "K",
            Self::Ace => "A",
        };
        write!(f, "{rank}")
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Suit {
    Clubs = 8,
    Diamonds = 4,
    Hearts = 2,
    Spades = 1,
}

impl fmt::Display for Suit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suit = match self {
            Self::Clubs => 'C',
            Self::Diamonds => 'D',
            Self::Hearts => 'H',
            Self::Spades => 'S',
        };
        write!(f, "{suit}")
    }
}

impl Suit {
    #[must_use] pub fn suits() -> impl DoubleEndedIterator<Item = Self> {
        [Self::Clubs, Self::Diamonds, Self::Hearts, Self::Spades].into_iter()
    }
}

#[derive(Debug)]
pub struct Deck {
    cards: Vec<Card>,
}

impl Deck {
    /// number of cards in a deck.
    pub const SIZE: usize = 52;

    /// create a new shuffled deck.
    pub fn shuffled<R: Rng>(rng: &mut R) -> Self {
        let mut deck = Self::default();
        deck.cards.shuffle(rng);
        deck
    }

    /// deals a card from the deck.
    pub fn deal(&mut self) -> Card {
        self.cards.pop().expect("Could not deal card from deck")
    }

    /// checks whether the deck is empty.
    #[must_use] pub const fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// returns the number of cards in the deck.
    #[must_use] pub const fn count(&self) -> usize {
        self.cards.len()
    }

    /// removes the specified card from the deck.
    pub fn remove(&mut self, card: Card) {
        self.cards.retain(|c| c != &card);
    }

    /// Calls the given closure n times with a sample of k cards.
    ///
    /// Panics if k is not in the [1 .. `Self::count()`] range.
    pub fn sample<F>(&self, n: usize, k: usize, mut f: F)
    where
        F: FnMut(&[Card]),
    {
        assert!(k > 0 && k < self.cards.len());
        let mut h = vec![Card::new(Rank::Ace, Suit::Hearts); k];
        let mut rng = SmallRng::from_os_rng(); //TODO: webasm os possible???

        for _ in 0..n {
            for (pos, c) in self.cards.choose_multiple(&mut rng, k).enumerate() {
                h[pos] = *c;
            }
            f(&h);
        }
    }

    /// Calls the `f` enclosure for each k-cards hand.
    ///
    /// Panics if k is not in range 2 <= k <= 7.
    pub fn for_each<F>(&self, k: usize, mut f: F)
    where
        F: FnMut(&[Card]),
    {
        assert!((2..=7).contains(&k), "2 <= k <= 7");

        if k > self.cards.len() {
            return;
        }

        let n = self.cards.len();
        let mut h = [Card::new(Rank::Ace, Suit::Hearts); 7];

        for c1 in 0..n {
            h[0] = self.cards[c1];

            for c2 in (c1 + 1)..n {
                h[1] = self.cards[c2];

                if k == 2 {
                    f(&h[0..k]);
                    continue;
                }

                for c3 in (c2 + 1)..n {
                    h[2] = self.cards[c3];

                    if k == 3 {
                        f(&h[0..k]);
                        continue;
                    }

                    for c4 in (c3 + 1)..n {
                        h[3] = self.cards[c4];

                        if k == 4 {
                            f(&h[0..k]);
                            continue;
                        }

                        for c5 in (c4 + 1)..n {
                            h[4] = self.cards[c5];

                            if k == 5 {
                                f(&h[0..k]);
                                continue;
                            }

                            for c6 in (c5 + 1)..n {
                                h[5] = self.cards[c6];

                                if k == 6 {
                                    f(&h[0..k]);
                                    continue;
                                }

                                for c7 in (c6 + 1)..n {
                                    h[6] = self.cards[c7];
                                    f(&h[0..k]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Default for Deck {
    fn default() -> Self {
        let cards = Suit::suits()
            .flat_map(|s| Rank::ranks().map(move |r| Card::new(r, s)))
            .collect::<Vec<_>>();
        Self { cards }
    }
}

impl IntoIterator for Deck {
    type Item = Card;
    type IntoIter = std::vec::IntoIter<Card>;

    fn into_iter(self) -> Self::IntoIter {
        self.cards.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashSet;

    #[test]
    fn card_encoding() {
        let mut cards = HashSet::default();
        let mut deck = Deck::shuffled(&mut rand::rng());

        while !deck.is_empty() {
            let card = deck.deal();
            assert_eq!(card.id() & 0xFF, PRIMES[card.rank() as usize]);
            assert_eq!((card.id() >> 8) & 0xF, card.rank() as u32);
            assert_eq!((card.id() >> 12) & 0xF, card.suit() as u32);
            assert_eq!(card.id() >> 16, 1 << (card.rank() as usize));
            cards.insert(card.id());
        }

        // Check uniquness.
        assert_eq!(cards.len(), Deck::SIZE);

        // From the Cactus Kev's website.
        let kd = Card::new(Rank::King, Suit::Diamonds);
        assert_eq!(kd.id(), 0x08004b25);

        let fs = Card::new(Rank::Five, Suit::Spades);
        assert_eq!(fs.id(), 0x00081307);

        let jc = Card::new(Rank::Jack, Suit::Clubs);
        assert_eq!(jc.id(), 0x0200891d);
    }

    #[test]
    fn card_to_string() {
        let c = Card::new(Rank::King, Suit::Diamonds);
        assert_eq!(c.to_string(), "KD");

        let c = Card::new(Rank::Five, Suit::Spades);
        assert_eq!(c.to_string(), "5S");

        let c = Card::new(Rank::Jack, Suit::Clubs);
        assert_eq!(c.to_string(), "JC");

        let c = Card::new(Rank::Ten, Suit::Hearts);
        assert_eq!(c.to_string(), "TH");

        let c = Card::new(Rank::Ace, Suit::Hearts);
        assert_eq!(c.to_string(), "AH");
    }

    #[test]
    fn deck_for_each() {
        let deck = Deck::default();
        assert_eq!(deck.count(), Deck::SIZE);

        let mut hands = HashSet::default();
        deck.for_each(5, |cards| {
            assert_eq!(cards.len(), 5);
            hands.insert(cards.to_owned());
        });
        assert_eq!(hands.len(), 2_598_960);

        hands.clear();
        deck.for_each(2, |cards| {
            assert_eq!(cards.len(), 2);
            hands.insert(cards.to_owned());
        });
        assert_eq!(hands.len(), 1_326);

        hands.clear();
        deck.for_each(3, |cards| {
            assert_eq!(cards.len(), 3);
            hands.insert(cards.to_owned());
        });
        assert_eq!(hands.len(), 22_100);
    }

    #[test]
    fn deck_for_each_7cards() {
        let deck = Deck::default();

        let mut count = 0;
        deck.for_each(7, |cards| {
            assert_eq!(cards.len(), 7);
            count += 1;
        });
        assert_eq!(count, 133_784_560);
    }

    #[test]
    fn deck_for_each_remove() {
        let mut deck = Deck::default();
        deck.remove(Card::new(Rank::Ace, Suit::Diamonds));
        deck.remove(Card::new(Rank::King, Suit::Diamonds));

        let mut count = 0;
        deck.for_each(7, |cards| {
            assert_eq!(cards.len(), 7);
            count += 1;
        });
        assert_eq!(count, 99_884_400);
    }

    #[test]
    fn sample() {
        let mut counter = 0;
        Deck::default().sample(10, 1, |hand| {
            assert_eq!(hand.len(), 1);
            counter += 1;
        });
        assert_eq!(counter, 10);

        let mut counter = 0;
        Deck::default().sample(10, 7, |hand| {
            assert_eq!(hand.len(), 7);
            counter += 1;
        });
        assert_eq!(counter, 10);
    }
}
