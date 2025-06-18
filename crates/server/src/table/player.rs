// code inspired by / taken from // code taken from https://github.com/vincev/freezeout
//! Table player types and player state management.

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

/// Represents a single poker player at a table.
#[derive(Debug)]
pub struct Player {
    pub id: PeerId,
    pub tx: mpsc::Sender<TableMessage>,
    pub nickname: String,
    pub chips: Chips,
    pub current_bet: Chips,
    pub last_action: PlayerAction,
    pub action_timer: Option<Instant>,
    pub public_cards: PlayerCards,
    pub private_cards: PlayerCards,
    pub active: bool,
    pub dealer: bool,
}

impl Player {
    pub fn new(id: PeerId, nickname: String, chips: Chips, tx: mpsc::Sender<TableMessage>) -> Self {
        Self {
            id,
            tx,
            nickname,
            chips,
            current_bet: Chips::ZERO,
            last_action: PlayerAction::None,
            action_timer: None,
            public_cards: PlayerCards::None,
            private_cards: PlayerCards::None,
            active: true,
            dealer: false,
        }
    }

    pub async fn send(&self, msg: SignedMessage) {
        let _ = self.tx.send(TableMessage::Send(msg)).await;
    }

    pub async fn notify_left(&self) {
        let _ = self.tx.send(TableMessage::PlayerLeave).await;
    }

    pub async fn throttle(&self, duration: Duration) {
        let _ = self.tx.send(TableMessage::Throttle(duration)).await;
    }

    pub fn place_bet(&mut self, action: PlayerAction, total_bet: Chips) {
        let required = total_bet - self.current_bet;
        let actual_bet = required.min(self.chips);

        self.chips -= actual_bet;
        self.current_bet += actual_bet;
        self.last_action = action;
    }

    pub fn fold(&mut self) {
        self.active = false;
        self.last_action = PlayerAction::Fold;
        self.private_cards = PlayerCards::None;
        self.public_cards = PlayerCards::None;
        self.action_timer = None;
    }

    pub fn reset_for_new_hand(&mut self) {
        self.active = self.chips > Chips::ZERO;
        self.dealer = false;
        self.current_bet = Chips::ZERO;
        self.last_action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
        self.private_cards = PlayerCards::None;
    }

    pub fn finalize_hand(&mut self) {
        self.last_action = PlayerAction::None;
        self.action_timer = None;
    }

    pub fn has_chips(&self) -> bool {
        self.chips > Chips::ZERO
    }
}

#[derive(Debug, Default)]
pub struct PlayersState {
    players: Vec<Player>,
    active_index: Option<usize>,
}

impl PlayersState {
    pub fn add(&mut self, player: Player) {
        self.players.push(player);
    }

    pub fn clear(&mut self) {
        self.players.clear();
        self.active_index = None;
    }

    pub fn remove(&mut self, id: &PeerId) -> Option<Player> {
        if let Some(pos) = self.players.iter().position(|p| &p.id == id) {
            let removed = self.players.remove(pos);

            match self.active_index {
                Some(idx) if idx == pos => {
                    self.active_index = self.players.iter().position(|p| p.active);
                }
                Some(idx) if pos < idx => {
                    self.active_index = Some(idx - 1);
                }
                _ => {}
            }

            Some(removed)
        } else {
            None
        }
    }

    pub fn shuffle<R: Rng>(&mut self, rng: &mut R) {
        self.players.shuffle(rng);
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn count_active(&self) -> usize {
        self.players.iter().filter(|p| p.active).count()
    }

    pub fn count_with_chips(&self) -> usize {
        self.players.iter().filter(|p| p.has_chips()).count()
    }

    pub fn count_active_with_chips(&self) -> usize {
        self.players.iter().filter(|p| p.active && p.has_chips()).count()
    }

    pub fn active_player(&mut self) -> Option<&mut Player> {
        self.active_index.and_then(|i| self.players.get_mut(i)).filter(|p| p.active)
    }

    pub fn is_active(&self, id: &PeerId) -> bool {
        self.active_index
            .and_then(|i| self.players.get(i))
            .map(|p| &p.id == id)
            .unwrap_or(false)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Player> {
        self.players.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Player> {
        self.players.iter_mut()
    }

    pub fn advance_turn(&mut self) {
        if self.count_active_with_chips() <= 1 || self.active_index.is_none() {
            return;
        }

        let current = self.active_index.unwrap();
        for (i, player) in self.players.iter().enumerate().cycle().skip(current + 1).take(self.players.len()) {
            if player.active && player.has_chips() {
                self.active_index = Some(i);
                break;
            }
        }
    }

    pub fn start_hand(&mut self) {
        for p in &mut self.players {
            p.reset_for_new_hand();
        }

        if self.count_active() > 1 {
            self.players.iter_mut().rev().find(|p| p.active).map(|p| p.dealer = true);
            self.active_index = self.players.iter().position(|p| p.active);
        } else {
            self.active_index = None;
        }
    }

    pub fn start_round(&mut self) {
        self.active_index = None;
        if self.count_active_with_chips() > 1 {
            self.active_index = self.players.iter().position(|p| p.active && p.has_chips());
        }
    }

    pub fn end_hand(&mut self) {
        self.active_index = None;
        for p in &mut self.players {
            p.finalize_hand();
        }
    }

    pub fn remove_bankrupt_players(&mut self) {
        self.players.retain(|p| p.has_chips());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::crypto::SigningKey;

    fn new_player(chips: Chips) -> Player {
        let peer_id = SigningKey::default().verifying_key().peer_id();
        let (table_tx, _table_rx) = mpsc::channel(10);
        Player::new(
            peer_id.clone(),
            "Alice".to_string(),
            chips,
            table_tx.clone(),
        )
    }

    #[test]
    fn test_player_bet() {
        let init_chips = Chips::new(100_000);
        let mut player = new_player(init_chips);

        // Simple bet.
        let bet_size = Chips::new(60_000);
        player.bet(PlayerAction::Bet, bet_size);
        assert_eq!(player.bet, bet_size);
        assert_eq!(player.chips, init_chips - bet_size);
        assert!(matches!(player.action, PlayerAction::Bet));

        // The bet amount is the total bet check chips paid are the new bet minus the
        // previous bet.
        let bet_size = bet_size + Chips::new(20_000);
        player.bet(PlayerAction::Bet, bet_size);
        assert_eq!(player.bet, bet_size);
        assert_eq!(player.chips, init_chips - bet_size);

        // Start new hand reset bet chips and action.
        player.start_hand();
        assert!(matches!(player.action, PlayerAction::None));
        assert!(player.is_active);
        assert_eq!(player.bet, Chips::ZERO);
        assert_eq!(player.chips, init_chips - bet_size);

        // Bet more than remaining chips goes all in.
        let remaining_chips = player.chips;
        player.bet(PlayerAction::Bet, Chips::new(1_000_000));
        assert_eq!(player.bet, remaining_chips);
        assert_eq!(player.chips, Chips::ZERO);
    }

    #[test]
    fn test_player_fold() {
        let init_chips = Chips::new(100_000);
        let mut player = new_player(init_chips);

        player.bet(PlayerAction::Bet, Chips::new(20_000));
        player.action_timer = Some(Instant::now());

        player.fold();
        assert!(matches!(player.action, PlayerAction::Fold));
        assert!(!player.is_active);
        assert!(player.action_timer.is_none());
    }

    fn new_players_state(n: usize) -> PlayersState {
        let mut players = PlayersState::default();
        (0..n).for_each(|_| players.join(new_player(Chips::new(100_000))));
        players
    }

    #[test]
    fn player_before_active_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.activate_next_player();
        assert_eq!(players.active_player.unwrap(), 1);

        // Player before active leaves, the active player moved to position 0.
        let player_id = players.player(0).player_id.clone();
        assert!(players.leave(&player_id).is_some());
        assert_eq!(players.active_player.unwrap(), 0);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn player_after_active_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.activate_next_player();
        assert_eq!(players.active_player.unwrap(), 1);

        // Player after active leaves, the active player should be the same.
        let player_id = players.player(2).player_id.clone();
        assert!(players.leave(&player_id).is_some());
        assert_eq!(players.active_player.unwrap(), 1);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn active_player_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.activate_next_player();
        assert_eq!(players.active_player.unwrap(), 1);

        // Active leaves the next player should become active.
        let active_id = players.player(1).player_id.clone();
        let next_id = players.player(2).player_id.clone();
        assert!(players.leave(&active_id).is_some());
        assert_eq!(players.active_player.unwrap(), 1);
        assert_eq!(players.active_player().unwrap().player_id, next_id);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn active_player_before_inactive_player_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.activate_next_player();
        assert_eq!(players.active_player.unwrap(), 1);

        // Deactivate player at index 2
        players.iter_mut().nth(2).unwrap().fold();
        assert_eq!(players.count_active(), SEATS - 1);

        // Active leaves but the player after that has folded so the next player at
        // index 3, that will move to index 2, should become active.
        let active_id = players.player(1).player_id.clone();
        let next_id = players.player(3).player_id.clone();
        assert!(players.leave(&active_id).is_some());
        assert_eq!(players.active_player.unwrap(), 2);
        assert_eq!(players.active_player().unwrap().player_id, next_id);
        assert_eq!(players.count_active(), SEATS - 2);
    }
}