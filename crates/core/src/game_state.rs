// code copied from https://github.com/vincev/freezeout
// Game state representation for each peer client in a peer-to-peer poker game.

use crate::{
    crypto::PeerID,
    message::{HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage},
    poker::{Card, Chips, PlayerCards, TableId},
};

/// Represents the complete state of a single poker game during a game session.
#[derive(Debug)]
pub struct Player {
    /// Unique ID assigned to this player (network peer ID).
    pub peer_id: PeerId,

    /// Cached string representation of the player ID digits for rendering.
    pub peer_id_digits: String,

    /// Player's visible name or alias.
    pub nickname: String,

    /// Total number of chips this player currently holds.
    pub total_chips: Chips,

    /// Amount the player has bet in the current round.
    pub current_bet: Chips,

    /// Outcome of the hand for this player (e.g., win/loss amount).
    pub hand_result: Option<HandPayoff>,

    /// Most recent action taken by the player (e.g., fold, call, raise).
    pub last_action: PlayerAction,

    /// Timer indicating how long ago the last action was made.
    pub last_action_timer: Option<u16>,

    /// The player's currently held cards.
    pub hole_cards: PlayerCards,

    /// Indicates whether this player currently holds the dealer button.
    pub is_dealer: bool,

    /// Indicates whether the player is still participating in the current hand.
    pub in_hand: bool,
}

impl Player {
    /// Creates a new `Player` instance with default values for a new participant.
    ///
    /// # Arguments
    /// * `peer_id` - Unique cryptographic identifier for the player.
    /// * `nickname` - Player's chosen display name.
    /// * `initial_chips` - Number of chips the player starts with.
    fn new(peer_id: PeerID, nickname: String, chips: Chips) -> Player {
        Player {
            peer_id,
            peer_id_digits: peer_id.digits(),
            nickname,
            total_chips: chips,
            current_bet: Chips::ZERO,
            hand_result: None,
            last_action: PlayerAction::None,
            last_action_timer: None,
            hole_cards: PlayerCards::None,
            is_dealer: false,
            in_hand: true,
        }
    }
}


/// Represents a request for player action made by the consensus (majority) of peers.
///
/// This replaces a traditional client-server model with decentralized peer coordination.
/// Each player is asked to choose one of the permitted actions (e.g., Fold, Call, Raise).
#[derive(Debug)]
#[derive(Debug)]
pub struct ActionRequest {
    /// Set of valid actions the player may choose from at this point.
    pub available_actions: Vec<PlayerAction>,

    /// Minimum raise amount allowed in the current betting round (based on game rules).
    pub minimum_raise: Chips,

    /// Big blind value for the current hand, used for validation and context.
    pub big_blind_amount: Chips,
}

impl ActionRequest {

    /// Returns `true` if the player is allowed to call.
    pub fn can_call(&self) -> bool {
        self.action(PlayerAction::Call)
    }

    /// Returns `true` if the player is allowed to check.
    pub fn can_check(&self) -> bool {
        self.action(PlayerAction::Check)
    }

    /// Returns `true` if the player is allowed to bet.
    pub fn can_bet(&self) -> bool {
        self.action(PlayerAction::Bet)
    }

    /// Returns `true` if the player is allowed to raise.
    pub fn can_raise(&self) -> bool {
        self.action(PlayerAction::Raise)
    }


    /// Checks whether a specific action is in the set of allowed actions.
    fn is_action_allowed(&self, action: PlayerAction) -> bool {
        self.available_actions.contains(&action)
    }
}

/// Represents the local game state as seen by a specific peer.
///
/// This struct holds everything the local peer needs to know about the table,
/// including player info, the board, and betting context.
#[derive(Debug)]
pub struct GameState {
    /// ID of this local peer.
    peer_id: PeerID,
    /// Nickname associated with this peer.
    nickname: String,
    /// Identifier of the host key or session authorithy (may be removed in P2P mode).
    legacy_server_key: String, //TODO: remove when switching fully to p2p.
    /// Unique identifier for the poker table instance.
    table_id: TableId,
    /// Number of player seats at the table.
    max_seats: usize,
    /// Whether the game has started.
    has_game_started: bool,
    /// List of all players currently seated at the table.
    players: Vec<Player>,
    /// Action request currently directed to this player, if any.
    current_action_requests: Option<ActionRequest>,
    /// Community cards on the board (flop, turn, river).
    community_cards: Vec<Card>,
    /// Total amount of chips currently in the pot.
    total_pot: Chips,
}

impl GameState {
    /// Initializes a new GameState instance for the local player
    pub fn new(player_id: PeerID, nickname: String) -> Self {
        Self {
            peer_id: player_id,
            nickname,
            table_id: TableId::default(),
            legacy_server_key: String::default(),
            max_seats: 0,
            has_game_started: false,
            players: Vec::default(),
            current_action_requests: None,
            community_cards: Vec::default(),
            total_pot: Chips::ZERO,
        }
    }

    /// Handle an incoming server (legacy, wanted: peer) message.
    pub fn handle_message(&mut self, msg: SignedMessage) {
        match msg.message() {

        }
    }
}

