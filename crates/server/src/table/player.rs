// code inspired by / taken from // code taken from https://github.com/vincev/freezeout
//! table player types.
use rand::prelude::*;
use std::{
    cmp::Ordering,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use poker_core::{
    crypto::PeerId,
    message::{PlayerAction, SignedMessage},
    poker::{Chips, PlayerCards},
};
use super::TableMessage;
/// Represents the state of a player at a poker table.
#[derive(Debug)]
pub struct Player {
    /// Unique identifier for the player.
    pub id: PeerId,

    /// Channel for sending messages to the player's client.
    pub message_channel: mpsc::Sender<TableMessage>,

    /// Display name chosen by the player.
    pub nickname: String,

    /// Total chips currently owned by the player.
    pub total_chips: Chips,

    /// Amount the player has currently bet.
    pub current_bet: Chips,

    /// Most recent action taken by the player.
    pub recent_action: PlayerAction,

    /// When the player's action timer started (if any).
    pub action_timer: Option<Instant>,

    /// Cards revealed to all players.
    pub public_cards: PlayerCards,

    /// Player's private (hole) cards.
    pub private_cards: PlayerCards,

    /// Whether the player is active in the current hand.
    pub is_active: bool,

    /// Whether the player has the dealer button.
    pub is_dealer: bool,
}

impl Player {
    pub fn new(
        id: PeerId,
        nickname: String,
        total_chips: Chips,
        message_channel: mpsc::Sender<TableMessage>,
    ) -> Self {
        Self {
            id,
            nickname,
            total_chips,
            message_channel,
            current_bet: Chips::ZERO,
            recent_action: PlayerAction::None,
            action_timer: None,
            public_cards: PlayerCards::None,
            private_cards: PlayerCards::None,
            is_active: true,
            is_dealer: false,
        }
    }

    pub async fn send(&self, message: SignedMessage) {
        let _ = self.message_channel.send(TableMessage::Send(message)).await;
    }

    pub async fn notify_left(&self) {
        let _ = self.message_channel.send(TableMessage::PlayerLeave).await;
    }

    pub async fn apply_throttle(&self, delay: Duration) {
        let _ = self.message_channel.send(TableMessage::Throttle(delay)).await;
    }

    /// Updates the player's chip count and bet after placing a bet.
    pub fn place_bet(&mut self, action: PlayerAction, chips_to_bet: Chips) {
        let amount_needed = chips_to_bet - self.current_bet;

        if self.total_chips < amount_needed {
            // Player goes all-in
            self.current_bet += self.total_chips;
            self.total_chips = Chips::ZERO;
        } else {
            self.total_chips -= amount_needed;
            self.current_bet += amount_needed;
        }

        self.recent_action = action;
    }

    /// Marks the player as folded.
    pub fn fold(&mut self) {
        self.is_active = false;
        self.recent_action = PlayerAction::Fold;
        self.private_cards = PlayerCards::None;
        self.public_cards = PlayerCards::None;
        self.action_timer = None;
    }

    /// Prepares the player's state for a new hand.
    pub fn begin_new_hand(&mut self) {
        self.is_active = self.total_chips > Chips::ZERO;
        self.is_dealer = false;
        self.current_bet = Chips::ZERO;
        self.recent_action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
        self.private_cards = PlayerCards::None;
    }

    /// Resets any temporary hand-related state after the hand ends.
    pub fn finalize_hand(&mut self) {
        self.recent_action = PlayerAction::None;
        self.action_timer = None;
    }
}

/// represents the state of the players at the table.
#[derive(Debug, Default)]
pub struct PlayersState {
    players: Vec<Player>,
    active_player: Option<usize>,
}

impl PlayersState {
    pub fn join(&mut self, player: Player) {
        self.players.push(player);
    }

    pub fn clear(&mut self) {
        self.players.clear();
        self.active_player = None;
    }

    //TODO: reformat!
    pub fn remove(&mut self, player_id: &PeerId) -> Option<Player> {
        if let Some(pos) = self.players.iter().position(|p| &p.player_id == player_id) {
            let player = self.players.remove(pos);

            let active_count = self.count_active();
            if active_count == 0 {
                self.active_player = None;
            } else if active_count == 1 {
                self.active_player = self.players.iter().position(|p| p.is_active);
            } else if let Some(active_player) = self.active_player.as_mut() {
                // if we removed the active player, we need to select a new active player (one has to exist, since active_count > 1
                match pos.cmp(active_player) {
                    Ordering::Less => {
                        *active_player -= 1;
                    }
                    Ordering::Equal => {
                        if pos == self.players.len() {
                            *active_player = None;
                        }
                        loop {
                            if self.players[*active_player].is_active {
                                break;
                            }
                            *active_player = (*active_player + 1) % self.players.len();
                        }
                    }
                    _ => {}
                }
            }
            Some(player)
        } else {
            None
        }
    }

    pub fn shuffle_seats<R: Rng>(&mut self, rng: &mut R) {
        self.players.shuffle(rng);
    }

    pub fn count_players(&self) -> usize {
       self.players.len()
    }

    pub fn count_active(&self) -> usize {
        self.players.iter().filter(|p| p.is_active).count()
    }

    pub fn count_with_chips(&self) -> usize {
        self.players.iter().filter(|p| p.chis > Chips::ZERO).count()
    }

    pub fn count_active_with_chips(&self) -> usize {
        self.players.iter().filter(|p| p.is_active && p.chips > Chips::Zero).count(); //TODO: maybe introduce has_chips function?
    }

    pub fn get_active_player(&self) -> Option<&mut Player> {
        self.active_player.and_then(|idx| self.players.get_mut(idx)).filter(|p| p.is_active) //TODO: what is going on here?
    }

    pub fn is_active(&self, player_id: &PeerId) -> bool {
        self.active_player.and_then(|idx| self.players.get(idx)).map(|p| &p.player_id == player_id).unwrap_or(false)
    }


}