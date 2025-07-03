//! Poker table state management.
//! Adapted and refactored from https://github.com/vincev/freezeout

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use anyhow::Error;
use log::{error, info};
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::{
    Message, PlayerAction, PlayerUpdate, SignedMessage,
};
use poker_core::net::NetTx;
use poker_core::net::traits::{P2pTransport};
use poker_core::poker::{Card, Chips, Deck, TableId};
use rand::prelude::StdRng;
use rand::{SeedableRng, rng};
use thiserror::Error;
use poker_core::game_state::ClientGameState;
use super::player::Player;
use super::players_state::PlayersState;

pub trait EngineCallbacks: Send + Sync + 'static {
    fn send(&mut self, player: PeerId, msg: SignedMessage,);
    fn throttle(&mut self, player: PeerId, dt: Duration,); // NEW helper
    fn disconnect(&mut self, player: PeerId,);
    fn credit_chips(
        &mut self,
        player: PeerId,
        amount: Chips,
    ) -> Result<(), anyhow::Error,>;
}

/// Represents the current phase of a hand being played.
#[derive(Debug, Eq, PartialEq,)]
enum HandPhase {
    WaitingForPlayers,
    StartingGame,
    StartingHand,
    PreflopBetting,
    Preflop,
    FlopBetting,
    Flop,
    TurnBetting,
    Turn,
    RiverBetting,
    River,
    Showdown,
    EndingHand,
    EndingGame,
}

impl fmt::Display for HandPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        match self {
            HandPhase::WaitingForPlayers => write!(f, "WaitingForPlayers"),
            HandPhase::StartingGame => write!(f, "StartingGame"),
            HandPhase::StartingHand => write!(f, "StartingHand"),
            HandPhase::PreflopBetting => write!(f, "PreflopBetting"),
            HandPhase::Preflop => write!(f, "Preflop"),
            HandPhase::FlopBetting => write!(f, "FlopBetting"),
            HandPhase::Flop => write!(f, "Flop"),
            HandPhase::TurnBetting => write!(f, "TurnBetting"),
            HandPhase::Turn => write!(f, "Turn"),
            HandPhase::RiverBetting => write!(f, "RiverBetting"),
            HandPhase::River => write!(f, "River"),
            HandPhase::Showdown => write!(f, "Showdown"),
            HandPhase::EndingHand => write!(f, "EndingHand"),
            HandPhase::EndingGame => write!(f, "EndingGame"),
        }
    }
}

/// Represents a single betting pot.
#[derive(Debug, Default,)]
struct Pot {
    participants: AHashSet<PeerId,>,
    total_chips:  Chips,
}

/// Possible errors when a player attempts to join a table.
#[derive(Error, Debug,)]
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
pub struct InternalTableState {
    /// id of the local peer (the one which is simulating this internalTable state).
    player_id: PeerId,

    pub connection: P2pTransport,
    table_id:       TableId,
    num_seats:      usize,
    signing_key:    Arc<SigningKey,>,

    phase:       HandPhase,
    hand_number: usize,

    small_blind: Chips,
    big_blind:   Chips,

    players:         PlayersState,
    deck:            Deck,
    community_cards: Vec<Card,>,

    last_bet:  Chips,
    min_raise: Chips,
    pots:      Vec<Pot,>,

    rng:              StdRng,
    hand_start_timer: Option<Instant,>,
    hand_start_delay: Duration,
    
    client_game_state: ClientGameState
}
impl InternalTableState {
    const ACTION_TIMEOUT: Duration = Duration::from_secs(15,);
    const INITIAL_SMALL_BLIND: Chips = Chips::new(10_000,);
    const INITIAL_BIG_BLIND: Chips = Chips::new(20_000,);

    pub fn new(
        client_game_state: ClientGameState,
        peer_id: PeerId,
        transport: P2pTransport,
        table_id: TableId,
        max_seats: usize,
        signing_key: Arc<SigningKey,>,
    ) -> Self {
        let rng = StdRng::from_rng(&mut rng(),);
        Self::with_rng(client_game_state, peer_id, transport, table_id, max_seats, signing_key, rng)
    }

    fn with_rng(
        client_game_state: ClientGameState,
        peer_id: PeerId,
        transport: P2pTransport,
        table_id: TableId,
        max_seats: usize,
        signing_key: Arc<SigningKey,>,
        mut rng: StdRng,
    ) -> Self {
        Self {
            client_game_state,
            player_id: peer_id,
            connection: transport,
            table_id,
            num_seats: max_seats,
            signing_key,
            phase: HandPhase::WaitingForPlayers,
            hand_number: 0,
            small_blind: Self::INITIAL_SMALL_BLIND,
            big_blind: Self::INITIAL_BIG_BLIND,
            players: PlayersState::default(),
            deck: Deck::shuffled(&mut rng,),
            community_cards: vec![],
            last_bet: Chips::ZERO,
            min_raise: Chips::ZERO,
            pots: vec![Pot::default()],
            rng,
            hand_start_timer: None,
            hand_start_delay: Duration::from_millis(3000,),
        }
    }
    pub fn can_join(&self,) -> bool {
        if !matches!(self.phase, HandPhase::WaitingForPlayers) {
            false
        } else {
            self.players.iter().count() < self.num_seats
        }
    }
    pub async fn try_join(
        &mut self,
        player_id: &PeerId,
        nickname: &str,
        starting_chips: &Chips,
    ) -> Result<(), TableJoinError,> {
        if self.players.count() >= self.num_seats {
            return Err(TableJoinError::TableFull,);
        }

        if !matches!(self.phase, HandPhase::WaitingForPlayers) {
            return Err(TableJoinError::GameStarted,);
        }

        if self.players.iter().any(|player| &player.id == player_id,) {
            return Err(TableJoinError::AlreadyJoined,);
        }

        let new_player: Player =
            Player::new(*player_id, nickname.to_string(), starting_chips,);

        let confirmation_message = Message::PlayerLeaveRequest{
            table_id:  self.table_id,
            player_id: *player_id,
        };

        let signed_message =
            SignedMessage::new(&self.signing_key, confirmation_message,);

        let _ = self.connection.tx.send(signed_message,);

        // for each existing player, send a player joined message to the newly
        // joined player.
        for existing_player in self.players.iter() {
            let join_msg = Message::PlayerJoinedConfirmation{
                player_id: existing_player.id,
                chips:     existing_player.chips,
                table_id:  self.table_id,
            };

            let signed = SignedMessage::new(&self.signing_key, join_msg,);
            let _ = self.connection.tx.send(signed,);
        }

        info!("Player {player_id} joined table {}", self.table_id);

        // tell all existing players that a new player joined.
        self.sign_and_send(Message::PlayerJoinedConfirmation {
            player_id: new_player.clone().id,
            chips:     new_player.clone().chips,
            table_id:  self.table_id,
        },)
            .await;

        self.players.add(new_player.clone(),);

        // if all seats are occupied, start the game.
        if self.players.count() == self.num_seats {
            self.start_game().await;
        }

        Ok((),)
    }
    pub async fn leave(
        &mut self,
        player_id: &PeerId,
    ) -> Result<(), anyhow::Error,> {
        let active_is_leaving = self.players.is_active(player_id,);
        if let Some(leaver,) = self.players.remove(player_id,) {
            // add player bets to the pot.
            if let Some(pot,) = self.pots.last_mut() {
                pot.total_chips += leaver.current_bet;
            }

            if self.players.count_active() < 2 {
                self.client_game_state.enter_end_hand().await;
                return Ok((),);
            }

            if active_is_leaving {
                self.request_action().await;
            }

            let msg = Message::PlayerLeaveRequest {
                player_id: *player_id,
            };

            return self.sign_and_send(msg,).await;
        }
        Ok((),)
    }

    pub async fn sign_and_send(&mut self, msg: Message,) -> Result<(), Error,> {
        let signed = SignedMessage::new(&self.signing_key, msg,);
        self.connection.tx.send(signed,).await
    }

    pub async fn send(&mut self, msg: SignedMessage,) -> Result<(), Error,> {
        self.connection.tx.send(msg,).await
    }

    /// Handle an incoming server message.
    pub fn handle_message(&mut self, msg: SignedMessage) {
        self.client_game_state.handle_message(msg)
    }
}
