//! Poker table state management.
//! Adapted and refactored from https://github.com/vincev/freezeout

use ahash::AHashSet;
use log::{error, info};
use rand::{rngs::StdRng, SeedableRng};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::mpsc;

use poker_core::{
    crypto::{PeerId, SigningKey},
    message::{HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage},
    poker::{Card, Chips, Deck, HandValue, PlayerCards, TableId},
};

use crate::db::Db;
use super::{player::{Player, PlayersState}, TableMessage};

/// Represents the current phase of a hand being played.
#[derive(Debug)]
enum HandPhase {
    WaitingForPlayers,
    StartingGame,
    StartingHand,
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
    EndingHand,
    EndingGame,
}

/// Represents a single betting pot.
#[derive(Debug, Default)]
struct Pot {
    participants: AHashSet<PeerId>,
    total_chips: Chips,
}

/// Possible errors when a player attempts to join a table.
#[derive(Error, Debug)]
pub enum TableJoinError {
    #[error("game already started")]
    GameStarted,
    #[error("table is full")]
    TableFull,
    #[error("player already joined")]
    AlreadyJoined,
    #[error("unknown join error")]
    Unknown,
}


/// Core state of a poker table instance.
#[derive(Debug)]
pub struct TableState {
    table_id: TableId,
    max_seats: usize,
    signing_key: Arc<SigningKey>,
    database: Db,

    phase: HandPhase,
    hand_number: usize,

    small_blind: Chips,
    big_blind: Chips,

    players: PlayersState,
    deck: Deck,
    board: Vec<Card>,

    last_bet: Chips,
    min_raise: Chips,
    pots: Vec<Pot>,

    rng: StdRng,
    hand_start_timer: Option<Instant>,
    hand_start_delay: Duration,
}
impl InternalTableState {
    const ACTION_TIMEOUT: Duration = Duration::from_secs(15);
    const INITIAL_SMALL_BLIND: Chips = Chips::new(10_000);
    const INITIAL_BIG_BLIND: Chips = Chips::new(20_000);

    pub fn new(table_id: TableId, max_seats: usize, signing_key: Arc<SigningKey>, database: Db) -> Self {
        let rng = StdRng::from_entropy();
        Self::with_rng(table_id, max_seats, signing_key, database, rng)
    }

    fn with_rng(table_id: TableId, max_seats: usize, signing_key: Arc<SigningKey>, database: Db, mut rng: StdRng) -> Self {
        Self {
            table_id,
            max_seats,
            signing_key,
            database,
            phase: HandPhase::WaitingForPlayers,
            hand_number: 0,
            small_blind: Self::INITIAL_SMALL_BLIND,
            big_blind: Self::INITIAL_BIG_BLIND,
            players: PlayersState::default(),
            deck: Deck::shuffled(&mut rng),
            board: vec![],
            last_bet: Chips::ZERO,
            min_raise: Chips::ZERO,
            pots: vec![Pot::default()],
            rng,
            hand_start_timer: None,
            hand_start_delay: Duration::from_millis(3000),
        }
    }

    fn with_rng(table_id: TableId, max_seats: usize, signing_key: Arc<SigningKey>, database: Db, mut rng: StdRng) -> Self {
        Self {
            table_id,
            max_seats,
            signing_key,
            database,
            phase: HandPhase::WaitingForPlayers,
            hand_number: 0,
            small_blind: Self::INITIAL_SMALL_BLIND,
            big_blind: Self::INITIAL_BIG_BLIND,
            players: PlayersState::default(),
            deck: Deck::shuffled(&mut rng),
            board: vec![],
            last_bet: Chips::ZERO,
            min_raise: Chips::ZERO,
            pots: vec![Pot::default()],
            rng,
            hand_start_timer: None,
            hand_start_delay: Duration::from_millis(3000),
        }
    }
    pub fn can_join(&self) -> bool {
        matches!(self.phase, HandPhase::WaitingForPlayers) && self.players.count() < self.max_seats
    }

    pub async fn try_join(
        &mut self,
        player_id: PeerId,
        nickname: String,
        starting_chips: Chips,
        channel: mpsc::Sender<TableMessage>,
    ) -> Result<(), TableJoinError> {
        if !matches!(self.phase, HandPhase::WaitingForPlayers) {
            return Err(TableJoinError::GameStarted);
        }
        if self.players.count() >= self.max_seats {
            return Err(TableJoinError::TableFull);
        }
        if self.players.iter().any(|p| p.id == player_id) {
            return Err(TableJoinError::AlreadyJoined);
        }

        let new_player = Player::new(player_id.clone(), nickname.clone(), starting_chips, channel);

        let confirmation = Message::PlayerJoinedTableConfirmation {
            table_id: self.table_id,
            chips: starting_chips,
            seats: self.max_seats,
        };
        let signed = SignedMessage::new(&self.signing_key, confirmation);
        let _ = new_player.send(signed).await;

        for existing in self.players.iter() {
            let join_msg = Message::PlayerJoined {
                player_id: existing.id.clone(),
                nickname: existing.nickname.clone(),
                chips: existing.total_chips,
            };
            let signed = SignedMessage::new(&self.signing_key, join_msg);
            let _ = new_player.send(signed).await;
        }

        self.broadcast(Message::PlayerJoined {
            player_id: new_player.id.clone(),
            nickname,
            chips: new_player.total_chips,
        }).await;

        self.players.join(new_player);
        info!("Player {player_id} joined table {}", self.table_id);

        if self.players.count() == self.max_seats {
            self.start_game().await;
        }

        Ok(())
    }
    pub async fn leave(&mut self, player_id: &PeerId) {
        if let Some(leaver) = self.players.remove(player_id) {
            let msg = Message::PlayerLeft(player_id.clone());
            self.broadcast(msg).await;
            leaver.notify_left().await;
        }
    }

    async fn broadcast(&self, msg: Message) {
        let signed = SignedMessage::new(&self.signing_key, msg);
        for player in self.players.iter() {
            let _ = player.send(signed.clone()).await;
        }
    }
    /// handle incoming message from a player.
    pub async fn message(&mut self, msg: SignedMessage) {
        if let Message::ActionResponse {action, amount} = msg.message() {
            if let Some(player) = self.players.active_player() { // only process actionResponses incoming from the active player
                if player.player_id == msg.sender() {
                    player.action = *action;
                    player.action_timer = None;

                    match action {
                        PlayerAction::Fold => {
                            player.fold();
                        }
                        PlayerAction::Call => {
                            player.bet(*action, self.last_bet)
                        }
                        PlayerAction::Check => {}
                        PlayerAction::Bet | PlayerAction::Raise => {
                            let amount = *amount.min(&(player.bet + player.chips));
                            self.min_raise = (amount - self.last_bet).max(self.min_raise);
                            self.last_bet = amount.max(self.last_bet);
                        }
                        _ => {}
                    }
                    self.action_update().await;
                }
            }
        }
    }
    pub async fn tick(&mut self) {
        if let Some(player) = self.players.iter_mut().find(|p| p.action_timer.is_some()) {
            if player.action_timer.unwrap().elapsed() > Self::ACTION_TIMEOUT {
                player.fold();
                self.action_update().await;
            } else {
                self.broadcast_game_update().await;
            }
        }

        if let Some(timer) = self.hand_start_timer {
            if timer.elapsed() > self.hand_start_delay {
                self.hand_start_timer = None;
                self.start_hand().await;
            }
        }
    }
    pub async fn action_update(&mut self) {
        self.players.activate_next_player();
        self.broadcast_game_update().await;

        if self.is_round_complete() {
            self.next_round().await;
        } else {
            self.request_action().await;
        }
    }


    async fn start_game(&mut self) {
        self.phase = HandPhase::StartingGame;
        self.players.shuffle_seats(&mut self.rng);
        let order = self.players.iter().map(|p| p.id.clone()).collect();
        self.broadcast(Message::StartGame(order)).await;

        self.start_hand().await;
    }
    async fn start_hand(&mut self) {
        self.phase = HandPhase::StartingHand;
        self.deck = Deck::shuffled(&mut self.rng);
        self.board.clear();
        self.pots.clear();
        self.pots.push(Pot::default());
        self.last_bet = Chips::ZERO;
        self.min_raise = Chips::ZERO;

        for player in self.players.iter_mut() {
            if player.can_play_hand() {
                player.assign_hole_cards(self.deck.deal(), self.deck.deal());
            } else {
                player.clear_cards();
            }
        }

        self.broadcast(Message::StartHand).await;

        for player in self.players.iter() {
            if let PlayerCards::Cards(c1, c2) = player.hole_cards {
                let signed = SignedMessage::new(&self.signing_key, Message::DealCards(c1, c2));
                let _ = player.send(signed).await;
            }
        }

        self.phase = HandPhase::Preflop;
        self.start_betting_round().await;
    }

    async fn start_betting_round(&mut self) {
        self.pots.push(Pot::default());
        self.players.reset_bets();
        self.last_bet = Chips::ZERO;
        self.min_raise = self.big_blind;

        self.players.advance_to_first_actor();
        self.broadcast(Message::NextToAct(self.players.current_actor_id().cloned())).await;
    }

    async fn enter_preflop_betting(&mut self) {
        self.hand_state = HandState::PreflopBetting;
        self.action_update().await;
    }

    async fn enter_deal_flop(&mut self, msg: Message) {
        for _ in 1..=3 {
            self.board_push(self.deck.deal());
        }
        self.hand_state = HandState::FlopBetting;
        self.start_round().await;
    }

    async fn enter_deal_turn(&mut self) {
        self.board_push(self.deck.deal());
        self.hand_state = HandState::TurnBetting;
        self.start_round().await;
    }

    async fn enter_deal_river(&mut self) {
        self.board_push(self.deck.deal());
        self.hand_state = HandState::RiverBetting;
        self.start_round().await;
    }

    /// Called when the hand transitions to the showdown or end state.
    async fn enter_showdown(&mut self) {
        self.phase = HandPhase::Showdown;

        // Reveal all cards for active players.
        for player in self.players.iter_mut() {
            player.action = PlayerAction::None;
            if player.is_active() {
                player.public_cards = player.hole_cards;
            }
        }
        self.enter_end_hand().await;
    }
    /// Called after showdown to handle chips payout, UI notifications, and player cleanup.
    async fn enter_end_hand(&mut self) {
        self.phase = HandPhase::EndingHand;

        // Show the board/results longer after showdown, shorter otherwise.
        let delay = if self.phase == HandPhase::Showdown {
            Duration::from_secs(7)
        } else {
            Duration::from_secs(3)
        };
        self.hand_start_timer = Some(Instant::now());
        self.hand_start_delay = delay;

        // Payout and collect payoffs.
        let payoffs = self.pay_bets();

        // Inform all clients.
        self.broadcast_game_update().await;
        self.broadcast_end_hand(&payoffs).await;

        // Remove busted players.
        self.remove_broke_players().await;

        // End game if not enough players remain.
        if self.players.count_with_chips() < 2 {
            self.enter_end_game().await;
        }
    }

    /// Broadcast the end-of-hand result to all players.
    async fn broadcast_end_hand(&self, payoffs: &[HandPayoff]) {
        let msg = Message::EndHand {
            payoffs: payoffs.to_vec(),
            board: self.board.clone(),
            cards: self.players.iter().map(|p| (p.player_id.clone(), p.public_cards)).collect(),
        };
        self.broadcast(msg).await;
    }

    /// Remove and notify players who have lost all their chips.
    async fn remove_broke_players(&mut self) {
        let broke: Vec<_> = self.players
            .iter()
            .filter(|p| p.chips == Chips::ZERO)
            .map(|p| p.player_id.clone())
            .collect();

        for pid in broke {
            if let Some(player) = self.players.get(&pid) {
                let _ = player.table_tx.send(TableMessage::PlayerLeft).await;
                self.broadcast(Message::PlayerLeft(pid.clone())).await;
            }
            self.players.remove(&pid);
        }
    }
    /// Handles the end of the game: pays out winners and resets the table.
    async fn enter_end_game(&mut self) {
        self.phase = HandPhase::EndingGame;
        self.broadcast_game_update().await;

        // Payout remaining chips to each player, notify and remove them.
        for player in self.players.iter() {
            if let Err(err) = self.database.pay_to_player(player.player_id.clone(), player.chips).await {
                error!("Failed to pay player {}: {}", player.player_id, err);
            }
            let _ = player.table_tx.send(TableMessage::PlayerLeft).await;
        }
        self.players.clear();
        self.hand_number = 0;
        self.phase = HandPhase::WaitingForPlayers;
    }

    /// Distribute the pots among winners and return their payoff info for the UI.
    fn pay_bets(&mut self) -> Vec<HandPayoff> {
        let mut payoffs = Vec::new();
        match self.players.count_active() {
            1 => self.pay_single_winner(&mut payoffs),
            n if n > 1 => self.pay_multiple_winners(&mut payoffs),
            _ => {}
        }
        payoffs
    }

    fn pay_single_winner(&mut self, payoffs: &mut Vec<HandPayoff>) {
        if let Some(player) = self.players.active_player() {
            for pot in self.pots.drain(..) {
                player.chips += pot.total_chips;
                match payoffs.iter_mut().find(|p| p.player_id == player.player_id) {
                    Some(payoff) => payoff.chips += pot.total_chips,
                    None => payoffs.push(HandPayoff {
                        player_id: player.player_id.clone(),
                        chips: pot.total_chips,
                        cards: vec![],
                        rank: String::new(),
                    }),
                }
            }
        }
    }
    fn pay_multiple_winners(&mut self, payoffs: &mut Vec<HandPayoff>) {
        for pot in self.pots.drain(..) {
            // Gather all active players in this pot.
            let mut contenders = self
                .players
                .iter_mut()
                .filter(|p| p.is_active() && pot.participants.contains(&p.player_id))
                .filter_map(|player| match player.hole_cards {
                    PlayerCards::Cards(c1, c2) => Some((player, c1, c2)),
                    _ => None,
                })
                .map(|(p, c1, c2)| {
                    let mut cards = vec![c1, c2];
                    cards.extend_from_slice(&self.board);
                    let (value, best_hand) = HandValue::eval_with_best_hand(&cards);
                    (p, value, best_hand)
                })
                .collect::<Vec<_>>();

            if contenders.is_empty() {
                continue;
            }

            // Sort descending by hand value.
            contenders.sort_by(|a, b| b.1.cmp(&a.1));
            let best_value = &contenders[0].1;
            let winner_count = contenders.iter().filter(|(_, v, _)| v == best_value).count();
            let chips_per_winner = pot.total_chips / winner_count as u32;
            let extra_chip = pot.total_chips % winner_count as u32;

            for (i, (player, value, best_hand)) in contenders.iter_mut().take(winner_count).enumerate() {
                let payoff = chips_per_winner + if i == 0 { extra_chip } else { 0 };
                player.chips += payoff;

                let mut best_hand_cards = best_hand.to_vec();
                best_hand_cards.sort_by_key(|c| c.rank());

                match payoffs.iter_mut().find(|p| p.player_id == player.player_id) {
                    Some(existing) => existing.chips += payoff,
                    None => payoffs.push(HandPayoff {
                        player_id: player.player_id.clone(),
                        chips: payoff,
                        cards: best_hand_cards,
                        rank: value.rank().to_string(),
                    }),
                }
            }
        }
    }
}

