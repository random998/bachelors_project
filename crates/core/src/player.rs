// code inspired by / taken from // code taken from https://github.com/vincev/freezeout
//! Table player types and player state management.

use std::fmt;
use std::time::Instant;

use crate::crypto::PeerId;
use crate::message::{PlayerAction};
use crate::poker::{Chips, PlayerCards};

/// Represents a single poker player at a table (with publicly known data).
#[derive(Clone,)]
pub struct PlayerPublic {
    pub(crate) id:            PeerId,
    pub(crate) nickname:      String,
    pub(crate) chips:         Chips,
    current_bet:   Chips,
    pub(crate) last_action:   PlayerAction,
    action_timer:  Option<Instant,>,
    pub(crate) public_cards:  PlayerCards,
    /// this player is active in the hand
    active:        bool,
    dealer:        bool,
}

impl fmt::Debug for PlayerPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        f.debug_struct("Player",)
            .field("id", &self.id,)
            .field("nickname", &self.nickname,)
            .field("chips", &self.chips,)
            .field("current_bet", &self.current_bet,)
            .field("last_action", &self.last_action,)
            .field("active", &self.active,)
            .field("dealer", &self.dealer,)
            .finish()
    }
}

impl PlayerPublic {
    pub fn new(id: PeerId, nickname: String, chips: &Chips,) -> Self {
        Self {
            id,
            nickname,
            chips: *chips,
            current_bet: Chips::ZERO,
            last_action: PlayerAction::None,
            action_timer: None,
            public_cards: PlayerCards::None,
            active: true,
            dealer: false,
        }
    }
    pub fn place_bet(&mut self, action: PlayerAction, total_bet: Chips,) {
        let required = total_bet - self.current_bet;
        let actual_bet = required.min(self.chips,);

        self.chips -= actual_bet;
        self.current_bet += actual_bet;
        self.last_action = action;
    }

    pub fn fold(&mut self,) {
        self.active = false;
        self.last_action = PlayerAction::Fold;
        self.public_cards = PlayerCards::None;
        self.action_timer = None;
    }

    pub fn reset_for_new_hand(&mut self,) {
        self.active = self.chips > Chips::ZERO;
        self.dealer = false;
        self.current_bet = Chips::ZERO;
        self.last_action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
    }

    pub fn reset_bet(&mut self,) {
        self.current_bet = Chips::ZERO;
    }

    pub fn finalize_hand(&mut self,) {
        self.last_action = PlayerAction::None;
        self.action_timer = None;
    }

    pub fn has_chips(&self,) -> bool {
        self.chips > Chips::ZERO
    }
}

impl fmt::Display for PlayerPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn test_player_bet() {
        // todo!();
    }

    #[test]
    fn test_player_fold() {
        // todo!();
    }
}
