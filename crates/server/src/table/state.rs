// code inspired by https://github.com/vincev/freezeout
//! table state types
use ahash::AHashSet;
use log::{error, info};
use rand::{SeedableRng, rngs::StdRng};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::mpsc;

use freezeout_core::{
    crypto::{PeerId, SigningKey},
    message::{HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage},
    poker::{Card, Chips, Deck, HandValue, PlayerCards, TableId},
};

use crate::db::Db;

use super::{
    TableMessage,
    player::{Player, PlayersState},
};

#[derive(Debug)]
enum HandState {
    /// table is waiting for players to join before starting the game.
    WaitingForPlayers,
    StartingGame,
    StartingHand,
    PreflopBetting,
    FlopBetting,
    TurnBetting,
    Showdown,
    EndingHand,
    EndingGame,
}

struct Pot {
    players: AHashSet<PeerId>,
    chips: Chips,
}

#[derive(Error, Debug)]
pub enum TableJoinError {
    #[error("game has started")]
    GameStarted,
    #[error("table is full")]
    TableFull,
    #[error("player already joined")]
    AlreadyJoined,
    #[error("unknown error")]
    Unknown,
}

/// internal table state
pub struct InternalTableState {
    table_id: TableId,
    seats: usize,
    signing_key: Arc<SigningKey>,
    database: Db,
    hand_state: HandState,
    small_blind_amount: Chips,
    big_blind_amount: Chips,
    hand_count: usize,
    players_state: PlayersState,
    deck: Deck,
    last_bet: Chips,
    min_raise: Chips,
    pots: Vec<Pot>,
    board: Vec<Card>,
    rng: StdRng,
    new_hand_timer: Option<Instant>,
    new_hand_timeout: Duration,
}

impl InternalTableState {
    const ACTION_TIMEOUT: Duration = Duration::from_secs(15); //TODO: change this from const to variable(as option, maybe?)
    const START_GAME_SMALL_BLIND: Chips = Chips::new(10_000); //TODO: change this from const to variable (as option, maybe?)
    const START_GAME_BIG_BLIND: Chips = Chips::new(10_000); //TODO: change this from const to variable (as option, maybe?)

    pub fn new(table_id: TableId, seats: usize, signing_key: Arc<SigningKey>, database: Db) -> InternalTableState {
        Self::with_rng(table_id, seats, signing_key, database, StdRng::from_os_rng())
    }

    /// create state with user initialized randomness.
    fn with_rng(
        table_id: TableId,
        seats: usize,
        signing_key: Arc<SigningKey>,
        database: Db,
        mut rng: StdRng,
    ) -> InternalTableState {
        InternalTableState {
            table_id,
            seats,
            signing_key,
            database,
            hand_state: HandState::WaitingForPlayers,
            small_blind_amount: Self::START_GAME_SMALL_BLIND,
            big_blind_amount: Self::START_GAME_BIG_BLIND,
            hand_count: 0,
            players_state: PlayersState::default(),
            deck: Deck::shuffled(&mut rng),
            last_bet: Chips::ZERO,
            min_raise: Chips::ZERO,
            pots: vec![Pot::default()],
            board: Vec::default(),
            rng,
            new_hand_timer: None,
            new_hand_timeout: Duration::dedfault(),
        }
    }

    pub fn can_player_join(&self) -> bool {
        if !matches(self.hand_state, HandState::WaitingForPlayers) {
            return false;
        } else {
           self.players.count() < self.seats
        }
    }

    pub async fn try_join(
        &mut self,
        player_id: &PeerId,
        nickname: &str,
        join_chips: Chips,
        table_tx: mpsc::Sender<TableMessage>,
    ) -> Result<(), TableJoinError> {
        if self.players.count() >= self.seats {
            return Err(TableJoinError::AlreadyJoined);
        }
        if !matches!(self.hand_state, HandState::WaitingForPlayers) {
            return Err(TableJoinError::GameStarted);
        }
        if self.players.iter().any(|p| &p.player_id == player_id) {
            return Err(TableJoinError::AlreadyJoined);
        }

        let joining_player = Player::new(
            player_id.clone(),
            nickname.to_string(),
            join_chips,
            table_tx
        );

        let msg_join_confirmation = Message::TableJoined {
            table_id: self.table_id,
            chips: join_player.chips,
            seats: self.seats,
        };

        let signed_msg = SignedMessage::new(&self.signing_key, msg_join_confirmation);
        let _ = joining_player.table_tx.send(TableMessage::Send(signed_msg)).await;

        // send joined message for each player at the table to the new player TODO:???
        for player in self.players.iter() {
            let msg = Message::PlayerJoined {
                player_id: player.player_id.clone(),
                nickname: player.nickname.clone(),
                chips: player.chips,
            };
            let signed_msg = SignedMessage::new(&self.signing_key, msg);
            let _ = player.table_tx.send(TableMessage::Send(signed_msg)).await;
        }
        // tell all players at the table that a player has joined. Note that because the player has not been added to the table yet, he won't get the broadcast.
        let msg = Message::PlayerJoined {
            player_id: player_id.clone(),
            nickname: nickname.to_string(),
            chips: joining_player.chips,
        };
        self.broadcast_message(msg).await;
        // add new player to the table.
        self.players.join(join_player);
        info!("Player {player_id} joined table {}", self.table_id);

        // if all seats are full, start game.
        if self.players.count() >= self.seats {
            self.enter_start_game().await()
        }
        Ok(())
    }

    /// Player leaves the table.
    pub async fn leave(&mut self, player_id: &PeerId) //TODO: why no return type?
    {
        let active_player_is_leaving = self.players.is_active(player_id);
        if let Some(player) = self.players.leave(player_id) {
            // add player's bets into the pot.
            if let Some(pot) = self.pots.last_mut() { //TODO: meaning of last_mut()?
                pot.chips += player.bet;
            }

            // tell other players that this player has left the table.
            let msg = Message::PlayerLeft(player_id.clone());
            self.broadcast_message(msg).await;

            // Notify the handler that this player has left the table. //TODO: handler??
            player.send_player_left().await;


        }
    }
}