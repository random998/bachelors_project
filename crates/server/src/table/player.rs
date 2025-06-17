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