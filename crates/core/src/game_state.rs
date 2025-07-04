// code copied from https://github.com/vincev/freezeout
// Game state representation for each peer client in a peer-to-peer poker game.

use std::fmt;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use anyhow::Error;
use poker_cards::Deck;
use rand::prelude::StdRng;
use tracing::info;

use crate::crypto::PeerId;
use crate::message::{
    HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage,
};
use crate::message::PlayerAction::Check;
use crate::poker::{Card, Chips, GameId, PlayerCards, TableId};

/// Represents a single betting pot.
#[derive(Debug, Default,)]
struct Pot {
    participants: AHashSet<PeerId,>,
    total_chips:  Chips,
}

/// Represents the current phase of a hand being played.
#[derive(Debug, Eq, PartialEq,)]
enum Round {
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

/// Represents the complete state of a single poker game during a game session.
#[derive(Debug, Clone)]
pub struct Player {
    /// Unique ID assigned to this player (network peer ID).
    pub id: PeerId,

    /// Cached string representation of the player ID digits for rendering.
    pub id_digits: String,

    /// Player's visible name or alias.
    pub nickname: String,

    /// Total number of chips this player currently holds.
    pub chips: Chips,

    /// Amount the player has bet in the current round.
    pub current_bet: Chips,

    /// the chips the player wins or loses in the current hand.
    pub hand_payoff: Option<HandPayoff,>,

    /// Most recent action taken by the player (e.g., fold, call, raise).
    pub last_action: PlayerAction,

    /// Timer indicating how long ago the last action was made.
    pub last_action_timer: Option<u16,>,

    /// The player's currently held cards.
    pub hole_cards: PlayerCards,

    /// Indicates whether this player currently holds the dealer button.
    pub is_dealer: bool,

    /// Indicates whether the player is still participating in the current
    /// hand.
    pub participating_in_hand: bool,
}

impl Player {
    /// Creates a new `Player` instance with default values for a new
    /// participant.
    ///
    /// # Arguments
    /// * `peer_id` - Unique cryptographic identifier for the player.
    /// * `nickname` - Player's chosen display name.
    /// * `initial_chips` - Number of chips the player starts with.
    fn new(peer_id: PeerId, nickname: String, chips: Chips,) -> Self {
        Self {
            id: peer_id,
            id_digits: peer_id.to_string(),
            nickname,
            chips,
            current_bet: Chips::ZERO,
            hand_payoff: None,
            last_action: PlayerAction::None,
            last_action_timer: None,
            hole_cards: PlayerCards::None,
            is_dealer: false,
            participating_in_hand: true,
        }
    }
}

/// Represents a request for player action made by the consensus (majority) of
/// peers.
///
/// This replaces a traditional client-server model with decentralized peer
/// coordination. Each player is asked to choose one of the permitted actions
/// (e.g., Fold, Call, Raise).
#[derive(Debug,)]
pub struct ActionRequest {
    /// Set of valid actions the player may choose from at this point.
    pub available_actions: Vec<PlayerAction,>,

    /// Minimum raise amount allowed in the current betting round (based on
    /// game rules).
    pub minimum_raise: Chips,

    /// Big blind value for the current hand, used for validation and context.
    pub big_blind_amount: Chips,
}

impl ActionRequest {
    /// Returns `true` if the player is allowed to call.
    #[must_use]
    pub fn can_call(&self,) -> bool {
        self.is_action_allowed(PlayerAction::Call,)
    }

    /// Returns `true` if the player is allowed to check.
    #[must_use]
    pub fn can_check(&self,) -> bool {
        self.is_action_allowed(PlayerAction::Check,)
    }

    /// Returns `true` if the player is allowed to bet.
    #[must_use]
    pub fn can_bet(&self,) -> bool {
        self.is_action_allowed(PlayerAction::Bet,)
    }

    /// Returns `true` if the player is allowed to raise.
    #[must_use]
    pub fn can_raise(&self,) -> bool {
        self.is_action_allowed(PlayerAction::Raise,)
    }

    /// Checks whether a specific action is in the set of allowed actions.
    fn is_action_allowed(&self, action: PlayerAction,) -> bool {
        self.available_actions.contains(&action,)
    }
}

#[derive(Debug,)]
pub struct RoundData {
    /// players that were active at the start of this round.
    pub starting_player_active: Vec<PeerId>,
    pub needs_action: Vec<PeerId>,
    min_raise:         Chips,
    last_bet:          Chips,
    /// how much chips each player has put in so far.
    player_bets:              Vec<Chips,>,
    /// number of times anyone has put in chips.
    total_bet_count: u64,
    /// number of times anyone has increased the bet non-forced.
    total_raise_count: u64,
    /// idx of the next player to act.
    pub to_act_idx: usize,
    pub pot: Pot,

}

impl RoundData {
    pub fn new(num_players: usize, min_raise: Chips, active_players: Vec<PeerId>, to_act_idx: usize) -> Self {
        RoundData {
            needs_action: active_players.clone(),
            starting_player_active: active_players,
            min_raise,
            last_bet: Chips::ZERO,
            player_bets: vec![Chips::ZERO; num_players],
            total_bet_count: 0,
            total_raise_count: 0,
            to_act_idx,
            pot: Pot::default(),
        }
    }
}

/// Represents the local game state as seen by a specific peer.
///
/// This struct holds everything the local peer needs to know about the table,
/// including player info, the board, and betting context.
#[derive(Debug,)]
pub struct ClientGameState {
    /// ID of this local player.
    player_id:         PeerId,
    /// Nickname associated with this peer.
    nickname:          String,
    /// Identifier of the host key or session authority (may be removed in P2P
    /// mode).
    legacy_server_key: String, // TODO: remove when switching fully to p2p.
    /// Unique identifier for the poker table instance.
    table_id:          TableId,
    /// Number of player seats at the table.
    num_seats:         usize,
    /// Number taken player seats at the table.
    num_taken_seats:   usize,
    /// Whether the game has started.
    game_started:      bool,
    /// List of all players currently seated at the table.
    players:           Vec<Player,>,
    deck:              Deck,
    /// Action request currently directed to this player, if any.
    action_request:    Option<ActionRequest,>,
    /// Community cards on the board (flop, turn, river).
    community_cards:   Vec<Card,>,

    /// id of the current game/hand.
    game_id: GameId,

    hand_start_timer: Option<Instant,>,
    hand_start_delay: Duration,

    round_data: RoundData,
}

impl ClientGameState {
    const ACTION_TIMEOUT: Duration = Duration::from_secs(15,);
    const INITIAL_SMALL_BLIND: Chips = Chips::new(10_000,);
    const INITIAL_BIG_BLIND: Chips = Chips::new(20_000,);

    #[must_use]
    pub fn new(player_id: PeerId, nickname: String,) -> Self {
        let round_data= RoundData::new(3, Chips::ZERO, Vec::default(), 0);
        let hand_start_delay = Duration::from_millis(1000);
        let hand_start_timer = Some(Instant::now());
        Self {
            hand_start_delay,
            hand_start_timer,
            round_data,
            deck: Deck::default(),
            player_id,
            nickname,
            table_id: TableId::NO_TABLE,
            game_id: GameId::NO_GAME,
            legacy_server_key: String::default(),
            num_seats: 0, // maximum number of seats at the table
            game_started: false,
            players: Vec::default(),
            num_taken_seats: 0,
            action_request: None,
            community_cards: Vec::default(),
        }
    }

    /// Handle an incoming peer message.
    pub fn handle_message(&mut self, msg: SignedMessage,) {
        info!(
            "peer handling incoming message: {:?}",
            msg.message().to_string()
        );
        match msg.message() {
            Message::JoinTableRequest {
                player_id,
                nickname,
                chips,
                table_id,
            } => {
                self.table_id = *table_id;
                self.num_taken_seats += 1usize;
                self.legacy_server_key = msg.sender().to_string();

                // Add the joined player as the first player in the players
                // list.
                self.players.push(Player::new(
                    *player_id,
                    nickname.clone(),
                    *chips,
                ),);
            },
            Message::PlayerLeaveRequest {
                player_id,
                table_id,
            } => {
                if self.table_id != *table_id {
                    return;
                }
                self.players.retain(|p| &p.id != player_id,);
            },
            Message::StartGameNotify {
                seat_order: seats,
                table_id: _table_id,
                game_id: _game_id,
            } => {
                // TODO: handle table_id and game_id.

                // Reorder seats according to the new order.
                println!("handling incoming server message StartGame");
                println!("current seats list: {seats:?}");
                println!(
                    "player id list: {:?}",
                    self.players.iter().map(|p| p.id).collect::<Vec<_,>>()
                );

                for (idx, seat_id,) in seats.iter().enumerate() {
                    let pos = self
                        .players
                        .iter()
                        .position(|p| &p.id == seat_id,)
                        .expect("Player not found",);
                    self.players.swap(idx, pos,);
                }

                // Move local player in first position.
                let pos = self
                    .players
                    .iter()
                    .position(|p| p.id == self.player_id,)
                    .expect("Local player not found",);
                self.players.rotate_left(pos,);

                self.game_started = true;
            },
            Message::StartHand {
                table_id: _table_id,
                game_id: _game_id,
            } => {
                // TODO: handle table_id and game_id

                // Prepare for a new hand.
                for player in &mut self.players {
                    player.hole_cards = PlayerCards::None;
                    player.last_action = PlayerAction::None;
                    player.hand_payoff = None;
                }
            },
            Message::EndHand { payoffs, .. } => {
                self.action_request = None;
                self.pot = Chips::ZERO;

                // Update winnings for each winning player.
                for payoff in payoffs {
                    if let Some(p,) = self
                        .players
                        .iter_mut()
                        .find(|p| p.id.digits() == payoff.player_id.digits(),)
                    {
                        p.hand_payoff = Some(payoff.clone(),);
                    }
                }
            },
            Message::DealCards {
                table_id: _table_id,
                game_id: _game_id,
                player_id,
                card1,
                card2,
            } => {
                // TODO: handle game_id, table_id

                // check if cards are dealt to this player.
                if self.player_id != *player_id {
                    return;
                }

                // This client player should be in first position.
                assert!(!self.players.is_empty());
                assert_eq!(
                    self.players[0].id.digits(),
                    self.player_id.digits()
                );

                self.players[0].hole_cards =
                    PlayerCards::Cards(*card1, *card2,);
                info!("hole cards of player {}", self.players[0].hole_cards);
            },
            Message::GameStateUpdate {
                table_id: _table_id,
                game_id: _game_id,
                player_updates,
                community_cards,
                pot,
            } => {
                // TODO: handle table_id and game_id.
                self.update_players(player_updates,);
                self.community_cards = community_cards.clone();
                self.pot = *pot;
            },
            Message::ActionRequest {
                game_id: _game_id,
                table_id: _table_id,
                player_id,
                min_raise,
                big_blind,
                actions,
            } => {
                // TODO: handle game_id and table_id

                // Check if the action has been requested for this player.
                if self.player_id != *player_id {
                    return;
                }

                self.action_request = Some(ActionRequest {
                    available_actions: actions.clone(),
                    minimum_raise:     *min_raise,
                    big_blind_amount:  *big_blind,
                },);
            },
            _ => {
                info!(
                    "client game state is not handling the following msg: {}",
                    msg.message().to_string()
                );
            },
        }
    }
    fn update_players(&mut self, updates: &[PlayerUpdate],) {
        for player_update in updates {
            let player_update = player_update.clone(); // Clone once

            if let Some(pos,) =
                self.players.iter_mut().position(|p| {
                    p.id.digits() == player_update.player_id.digits()
                },)
            {
                let player = &mut self.players[pos];
                player.chips = player_update.chips;
                player.current_bet = player_update.bet;
                player.last_action = player_update.action;
                player.last_action_timer = player_update.action_timer;
                player.is_dealer = player_update.is_dealer;
                player.participating_in_hand = player_update.is_active;

                // Do not override cards for the local player as they are
                // updated when we get a DealCards message.
                if pos != 0 {
                    player.hole_cards = player_update.hole_cards;
                }

                // If local player has folded, remove its cards.
                if pos == 0 && !player.participating_in_hand {
                    player.hole_cards = PlayerCards::None;
                    self.action_request = None;
                }
            }
        }
    }

    /// Returns the server key.
    #[must_use]
    pub fn server_key(&self,) -> &str {
        &self.legacy_server_key
    }

    /// Returns a reference to the players.
    #[must_use]
    pub fn players(&self,) -> &[Player] {
        &self.players
    }

    /// The current pot.
    #[must_use]
    pub const fn pot(&self,) -> Chips {
        self.pot
    }

    /// The board cards.
    #[must_use]
    pub fn community_cards(&self,) -> &[Card] {
        &self.community_cards
    }

    /// The number of seats at this table.
    #[must_use]
    pub const fn num_seats(&self,) -> usize {
        self.num_seats
    }

    /// Checks if the game has started.
    #[must_use]
    pub const fn game_started(&self,) -> bool {
        self.game_started
    }

    /// Checks if the local player is active.
    #[must_use]
    pub fn is_active(&self,) -> bool {
        !self.players.is_empty() && self.players[0].participating_in_hand
    }
    /// Returns the requested player action if any.
    #[must_use]
    pub const fn action_request(&self,) -> Option<&ActionRequest,> {
        self.action_request.as_ref()
    }

    /// Reset the action request.
    pub fn reset_action_request(&mut self,) {
        self.action_request = None;
    }

    pub fn can_join(&self,) -> bool {
        if !matches!(self.phase, Round::WaitingForPlayers) {
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

        if !matches!(self.phase, Round::WaitingForPlayers) {
            return Err(TableJoinError::GameStarted,);
        }

        if self.players.iter().any(|player| &player.id == player_id,) {
            return Err(TableJoinError::AlreadyJoined,);
        }

        let new_player: crate::player::Player = crate::player::Player::new(
            *player_id,
            nickname.to_string(),
            starting_chips,
        );

        let confirmation_message = Message::PlayerLeaveRequest {
            table_id:  self.table_id,
            player_id: *player_id,
        };

        let signed_message =
            SignedMessage::new(&self.signing_key, confirmation_message,);

        let _ = self.connection.tx.send(signed_message,);

        // for each existing player, send a player joined message to the newly
        // joined player.
        for existing_player in self.players.iter() {
            let join_msg = Message::PlayerJoinedConfirmation {
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
    pub fn handle_message(&mut self, msg: SignedMessage,) {
        self.client_game_state.handle_message(msg,)
    }
}

impl fmt::Display for Round {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        match self {
            Round::WaitingForPlayers => write!(f, "WaitingForPlayers"),
            Round::StartingGame => write!(f, "StartingGame"),
            Round::StartingHand => write!(f, "StartingHand"),
            Round::PreflopBetting => write!(f, "PreflopBetting"),
            Round::Preflop => write!(f, "Preflop"),
            Round::FlopBetting => write!(f, "FlopBetting"),
            Round::Flop => write!(f, "Flop"),
            Round::TurnBetting => write!(f, "TurnBetting"),
            Round::Turn => write!(f, "Turn"),
            Round::RiverBetting => write!(f, "RiverBetting"),
            Round::River => write!(f, "River"),
            Round::Showdown => write!(f, "Showdown"),
            Round::EndingHand => write!(f, "EndingHand"),
            Round::EndingGame => write!(f, "EndingGame"),
        }
    }
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
