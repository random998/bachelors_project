// code copied from github.com/vincev/freezeout

#[cfg(feature = "eval")]
pub use eval::{HandRank, HandValue};
pub use poker_cards::{Card, Deck, Rank, Suit};
use serde::{Deserialize, Serialize};
use std::{fmt, ops};

// a unique table identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TableId(u32);

impl TableId {
    // unassigned tables receive id 0.
    pub const NO_TABLE: TableId = TableId(0);

    fn get_random_u32() -> Result<u32, getrandom::Error> {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf)?;
        Ok(u32::from_ne_bytes(buf))
    }
    // create new unique (with high probability) table id.
    pub fn new_id() -> TableId {
        TableId(Self::get_random_u32().unwrap())
    }
}

impl fmt::Display for TableId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// data structure for storing the amount of chips for a given table.
#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Chips(u32);

impl Chips {
    pub const ZERO: Chips = Chips(0);

    /// create chips struct with the given amount of chips.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// retrieve the integer amount of chips of a given table.
    pub fn amount(&self) -> u32 {
        self.0
    }
}

impl From<Chips> for u32 {
    fn from(val: Chips) -> Self { val.0}
}

impl ops::Add for Chips {
    type Output = Chips;

    fn add(self, rhs: Chips) -> Chips {
        Chips(self.0.saturating_add(rhs.0))
    }
}

impl ops::AddAssign for Chips {
    fn add_assign(&mut self, rhs: Chips) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl ops::Sub for Chips {
    type Output = Chips;

    fn sub(self, rhs: Chips) -> Chips {
        Chips(self.0.saturating_sub(rhs.0))
    }
}

impl ops::SubAssign for Chips {
    fn sub_assign(&mut self, rhs: Chips) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl ops::Mul<u32> for Chips {
    type Output = Chips;

    fn mul(self, rhs: u32) -> Chips {
        Chips(self.0.saturating_mul(rhs))
    }
}

impl ops::Div<u32> for Chips {
    type Output = Chips;

    fn div(self, rhs: u32) -> Chips {
        Chips(self.0.saturating_div(rhs))
    }
}

impl ops::Rem<u32> for Chips {
    type Output = Chips;
    fn rem(self, rhs: u32) -> Chips {
        Chips(self.0 % rhs)
    }
}

impl fmt::Display for Chips {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amount = self.0;
        if amount >= 10_000_000 {
            write!(f, "{:.1}M", amount as f64 / 1e6)
        } else if amount >= 1_000_000 {
            write!(f,
                   "{},{:03},{:03}K",
                   amount / 1_000_000,
                   (amount %  1_000_000) / 1000,
                   amount /  1000
            )
        } else if amount >= 1_000 {
                   write!(f, "{},{:03}", amount / 1000, amount % 1000)
        } else {
            write!(f, "{}", amount)
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub enum PlayerCards {
    #[default]
    None, // the player has no cards.
    Covered, // the player has cards but they are covered.
    Cards(Card, Card) // the player has two cards, which are not covered.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_formatting() {
        assert_eq!(format!("{}", Chips::ZERO), "0");
        assert_eq!(Chips(123).to_string(), "123");
        assert_eq!(Chips(1_000).to_string(), "1,000");
        assert_eq!(Chips(1_234).to_string(), "1,234");
        assert_eq!(Chips(12_345).to_string(), "12,345");
        assert_eq!(Chips(123_456).to_string(), "123,456");
        assert_eq!(Chips(1_000_000).to_string(), "1,000,000");
        assert_eq!(Chips(1_234_567).to_string(), "1,234,567");
        assert_eq!(Chips(10_000_000).to_string(), "10.0M");
        assert_eq!(Chips(123_456_789).to_string(), "123.5M");
    }
}
