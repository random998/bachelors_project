use std::{fmt, ops};

pub use poker_cards::{Card, Deck, Rank, Suit};
pub use poker_eval::eval::{HandRank, HandValue};
use serde::{Deserialize, Serialize};

// a unique table identifier.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
/// table id
pub struct TableId(pub u32,);
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
pub struct GameId(pub u32,);

impl GameId {
    // unassigned tables receive id 0.
    /// no table const
    pub const NO_GAME: Self = Self(0,);

    fn get_random_u32() -> Result<u32, getrandom::Error,> {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf,)?;
        Ok(u32::from_ne_bytes(buf,),)
    }
    /// create new unique (with high probability) game id.
    #[must_use]
    /// # Panics
    pub fn new_id() -> Self {
        Self(Self::get_random_u32().unwrap(),)
    }
}

impl TableId {
    // unassigned tables receive id 0.
    /// no table const
    pub const NO_TABLE: Self = Self(0,);

    fn get_random_u32() -> Result<u32, getrandom::Error,> {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf,)?;
        Ok(u32::from_ne_bytes(buf,),)
    }
    /// create new unique (with high probability) table id.
    #[must_use]
    /// # Panics
    pub fn new_id() -> Self {
        Self(Self::get_random_u32().expect("err",),)
    }
}

impl fmt::Display for TableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// data structure for storing the amount of chips for a given table.
#[derive(
    Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Copy, Clone,
)]
pub struct Chips(u32,);

impl Chips {
    /// const for zero chips.
    pub const ZERO: Self = Self(0,);

    /// create chips struct with the given amount of chips.
    #[must_use]
    pub const fn new(value: u32,) -> Self {
        Self(value,)
    }

    /// retrieve the integer amount of chips of a given table.
    #[must_use]
    pub const fn amount(&self,) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn default() -> Self {
        Self::new(1000,)
    }
}

impl Default for Chips {
    fn default() -> Self {
        Self::new(1000,)
    }
}

impl From<Chips,> for u32 {
    fn from(val: Chips,) -> Self {
        val.0
    }
}

impl ops::Add for Chips {
    type Output = Self;

    fn add(self, rhs: Self,) -> Self {
        Self(self.0.saturating_add(rhs.0,),)
    }
}

impl ops::AddAssign for Chips {
    fn add_assign(&mut self, rhs: Self,) {
        self.0 = self.0.saturating_add(rhs.0,);
    }
}

impl ops::Sub for Chips {
    type Output = Self;

    fn sub(self, rhs: Self,) -> Self {
        Self(self.0.saturating_sub(rhs.0,),)
    }
}

impl ops::SubAssign for Chips {
    fn sub_assign(&mut self, rhs: Self,) {
        self.0 = self.0.saturating_sub(rhs.0,);
    }
}

impl ops::Mul<u32,> for Chips {
    type Output = Self;

    fn mul(self, rhs: u32,) -> Self {
        Self(self.0.saturating_mul(rhs,),)
    }
}

impl ops::Div<u32,> for Chips {
    type Output = Self;

    fn div(self, rhs: u32,) -> Self {
        Self(self.0.saturating_div(rhs,),)
    }
}

impl ops::Rem<u32,> for Chips {
    type Output = Self;
    fn rem(self, rhs: u32,) -> Self {
        Self(self.0 % rhs,)
    }
}

impl fmt::Display for Chips {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        let amount = self.0;
        if amount >= 10_000_000 {
            write!(f, "{:.1}M", f64::from(amount) / 1e6)
        } else if amount >= 1_000_000 {
            write!(
                f,
                "{},{:03},{:03}",
                amount / 1_000_000,
                (amount % 1_000_000) / 1000,
                amount % 1_000
            )
        } else if amount >= 1_000 {
            write!(f, "{},{:03}", amount / 1000, amount % 1000)
        } else {
            write!(f, "{amount}")
        }
    }
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq,
)]
/// enum for listening the different states the player's cards can be in.
pub enum PlayerCards {
    #[default]
    /// the player has no cards.
    None,
    /// the player has cards, but they are covered (only visible to the player
    /// himself).
    Covered,
    /// the player has two cards, which are not covered (visible to all players
    /// at the table)
    Cards(Card, Card,),
}

impl fmt::Display for PlayerCards {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        let string = match self {
            Self::None => "None".to_string(),
            Self::Cards(card1, card2,) => format!("{card1}, {card2}"),
            Self::Covered => "Covered".to_string(),
        };
        write!(f, "{string}")
    }
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
