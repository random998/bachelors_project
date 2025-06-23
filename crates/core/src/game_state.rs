// code copied from https://github.com/vincev/freezeout
// Game state representation for each peer client in a peer-to-peer poker game.

use crate::crypto::PeerId;
use crate::message::{HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage};
use crate::poker::{Card, Chips, PlayerCards, TableId};

/// Represents the complete state of a single poker game during a game session.
#[derive(Debug,)]
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

    /// Indicates whether the player is still participating in the current hand.
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
            id_digits: peer_id.digits(),
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

    /// Minimum raise amount allowed in the current betting round (based on game
    /// rules).
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

/// Represents the local game state as seen by a specific peer.
///
/// This struct holds everything the local peer needs to know about the table,
/// including player info, the board, and betting context.
#[derive(Debug,)]
pub struct ClientGameState {
    /// ID of this local player.
    player_id: PeerId,
    /// Nickname associated with this peer.
    nickname: String,
    /// Identifier of the host key or session authority (may be removed in P2P
    /// mode).
    legacy_server_key: String, // TODO: remove when switching fully to p2p.
    /// Unique identifier for the poker table instance.
    table_id: TableId,
    /// Number of player seats at the table.
    num_seats: usize,
    /// Number taken player seats at the table.
    num_taken_seats: usize,
    /// Whether the game has started.
    game_started: bool,
    /// List of all players currently seated at the table.
    players: Vec<Player,>,
    /// Action request currently directed to this player, if any.
    action_request: Option<ActionRequest,>,
    /// Community cards on the board (flop, turn, river).
    community_cards: Vec<Card,>,
    /// Total amount of chips currently in the pot.
    pot: Chips,
}

impl ClientGameState {
    /// Initializes a new `GameState` instance for the local player
    #[must_use]
    pub fn new(player_id: PeerId, nickname: String,) -> Self {
        Self {
            player_id,
            nickname,
            table_id: TableId::NO_TABLE,
            legacy_server_key: String::default(),
            num_seats: 0, // maximum number of seats at the table
            game_started: false,
            players: Vec::default(),
            num_taken_seats: 0,
            action_request: None,
            community_cards: Vec::default(),
            pot: Chips::ZERO,
        }
    }

    /// Handle an incoming server (legacy, wanted: peer) message.
    pub fn handle_message(&mut self, msg: SignedMessage,) {
        match msg.message() {
            | Message::PlayerJoined {
                player_id,
                nickname,
                chips,
                table_id,
            } => {
                self.table_id = *table_id;
                self.num_taken_seats += 1usize;
                self.legacy_server_key = msg.sender().digits();

                // Add the joined player as the first player in the players list.
                self.players.push(Player::new(player_id.clone(), nickname.clone(), *chips,),);
            },
            | Message::PlayerLeftNotification {
                player_id,
            } => {
                self.players.retain(|p| &p.id != player_id,);
            },
            | Message::StartGame(seats,) => {
                // Reorder seats according to the new order.
                println!("handling incoming server message StartGame");
                println!("current seats list: {:?}", seats);
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
            | Message::StartHand => {
                // Prepare for a new hand.
                for player in &mut self.players {
                    player.hole_cards = PlayerCards::None;
                    player.last_action = PlayerAction::None;
                    player.hand_payoff = None;
                }
            },
            | Message::EndHand {
                payoffs,
                ..
            } => {
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
            | Message::DealCards(c1, c2,) => {
                // This client player should be in first position.
                assert!(!self.players.is_empty());
                assert_eq!(self.players[0].id.digits(), self.player_id.digits());

                self.players[0].hole_cards = PlayerCards::Cards(*c1, *c2,);
            },
            | Message::GameStateUpdate {
                players,
                community_cards,
                pot,
            } => {
                self.update_players(players,);
                self.community_cards = community_cards.clone();
                self.pot = *pot;
            },
            | Message::ActionRequest {
                player_id,
                min_raise,
                big_blind,
                actions,
            } => {
                // Check if the action has been requested for this player.
                if &self.player_id == player_id {
                    self.action_request = Some(ActionRequest {
                        available_actions: actions.clone(),
                        minimum_raise: *min_raise,
                        big_blind_amount: *big_blind,
                    },);
                }
            },
            | _ => {},
        }
    }
    fn update_players(&mut self, updates: &[PlayerUpdate],) {
        let updates = updates;
        for update in updates {
            let update = update.clone(); // Clone once

            if let Some(pos,) =
                self.players.iter_mut().position(|p| p.id.digits() == update.player_id.digits(),)
            {
                let player = &mut self.players[pos];
                player.chips = update.chips;
                player.current_bet = update.bet;
                player.last_action = update.action;
                player.last_action_timer = update.action_timer;
                player.is_dealer = update.is_dealer;
                player.participating_in_hand = update.is_active;

                // Do not override cards for the local player as they are updated
                // when we get a DealCards message.
                if pos != 0 {
                    player.hole_cards = update.hole_cards;
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
}
