use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Type definitions for p2p messages.
use anyhow::{Result, bail};
use libp2p;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::crypto::{KeyPair, PeerId, PublicKey, Signature};
use crate::game_state::Pot;
use crate::poker::{Card, Chips, GameId, PlayerCards, TableId};
/// Represents a message exchanged between peers in the P2P poker protocol.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
pub enum Message {
    /// the network has given this peer a new listen address.
    NewListenAddr {
        listener_id: String,
        multiaddr:   Multiaddr,
    },

    /// response by server that particular client joined the server.
    JoinedTableConfirmation {
        table_id:  TableId,
        /// id of the player which joined
        player_id: PeerId,
        /// The player nickname.
        nickname:  String,
        /// The chips amount for the player.
        chips:     Chips,
    },

    /// Request (by a player) to join a table with the given nickname.
    PlayerJoinTableRequest {
        table_id:  TableId,
        /// id of the player which requests to join.
        player_id: PeerId,
        nickname:  String,
        chips:     Chips,
    },

    /// requests / commands the player with the id to leave the table.
    PlayerLeaveRequest {
        player_id: PeerId,
        table_id:  TableId,
    },

    /// Sent when no available tables remain for the player to join.
    NoTablesLeftNotification { player_id: PeerId, },

    /// Indicates that the player does not have enough chips to join a game.
    NotEnoughChips {
        player_id: PeerId,
        table_id:  TableId,
    },

    /// Indicates that the player has already joined a table.
    PlayerAlreadyJoined {
        table_id:  TableId,
        player_id: PeerId,
    },

    /// Confirmation that a specific player has joined a specific table.
    PlayerJoinedConfirmation {
        /// id of the table the player joined.
        table_id:  TableId,
        /// id of the player that joined the table.
        player_id: PeerId,
        /// chip balance the player joined the table.
        chips:     Chips,
        nickname:  String,
    },

    /// Request to show the player’s current chip balance/account summary.
    ShowAccount { player_id: PeerId, chips: Chips, },

    /// balance update of the account.
    AccountBalanceUpdate { player_id: PeerId, chips: Chips, },

    /// Starts the game and notifies all players of seat order.
    StartGameNotify {
        seat_order: Vec<PeerId,>,
        table_id:   TableId,
        /// the id the started game should have.
        game_id:    GameId,
    },

    /// Begins a new poker hand.
    StartHand { table_id: TableId, game_id: GameId, },

    /// Ends the current poker hand, providing final results.
    EndHand {
        /// Final payoffs for each player.
        payoffs: Vec<HandPayoff,>,

        /// Community cards on the board at showdown.
        board: Vec<Card,>,

        /// Map of each player's ID to their revealed hole cards.
        cards: Vec<(PeerId, PlayerCards,),>,

        table_id: TableId,
        game_id:  GameId,
    },

    /// Deals two hole cards to the player.
    DealCards {
        card1:     Card,
        card2:     Card,
        player_id: PeerId,
        table_id:  TableId,
        game_id:   GameId,
    },

    /// Notifies peers that this player has left the table.
    PlayerLeftTable {
        peer_id:  PeerId,
        table_id: TableId,
        game_id:  GameId,
    },

    /// Updated game state including players, board, and pot.
    GameStateUpdate {
        player_updates: Vec<PlayerUpdate,>,
        board:          Vec<Card,>,
        pot:            Pot,
        table_id:       TableId,
        game_id:        GameId,
    },

    /// Requests a specific player to make a game action (e.g., Call, Raise).
    ActionRequest {
        game_id: GameId,

        table_id: TableId,

        /// The player being prompted to act.
        player_id: PeerId,

        /// Minimum amount required to raise.
        min_raise: Chips,

        /// Current big blind value.
        big_blind: Chips,

        /// Legal actions the player may choose from.
        actions: Vec<PlayerAction,>,
    },

    /// Response from a player specifying their action and any bet amount.
    ActionResponse {
        /// Action taken (Fold, Call, Raise, etc.).
        action: PlayerAction,

        /// Chips committed with the action, if applicable (used for
        /// Bet/Raise).
        amount: Chips,
    },

    /// tells peers to show ui for longer
    Throttle { duration: Duration, },
}

impl Message {
    // Returns a label of the message variant as a string.
    #[must_use]
    pub const fn label(&self,) -> &'static str {
        match self {
            Self::AccountBalanceUpdate { .. } => "BalanceUpdateMsg",
            Self::Throttle { .. } => "ThrottleMsg",
            Self::PlayerJoinTableRequest { .. } => "PlayerJoinTableRequest",
            Self::PlayerLeaveRequest { .. } => "PlayerLeftNotification",
            Self::NoTablesLeftNotification { .. } => "NoTablesLeftNotification",
            Self::NotEnoughChips { .. } => "NotEnoughChips",
            Self::PlayerAlreadyJoined { .. } => "PlayerAlreadyJoined",
            Self::PlayerJoinedConfirmation { .. } => "PlayerJoinedConfirmation",
            Self::ShowAccount { .. } => "ShowAccount",
            Self::StartGameNotify { .. } => "StartGame",
            Self::StartHand { .. } => "StartHand",
            Self::EndHand { .. } => "EndHand",
            Self::DealCards { .. } => "DealCards",
            Self::PlayerLeftTable { .. } => "PlayerLeftTable",
            Self::GameStateUpdate { .. } => "GameStateUpdate",
            Self::ActionRequest { .. } => "ActionRequest",
            Self::ActionResponse { .. } => "ActionResponse",
            Self::JoinedTableConfirmation { .. } => "JoinedTableConfirmation",
            Self::NewListenAddr { .. } => "NewListenAddr",
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        self.label().fmt(f,)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize,)]
pub struct PlayerUpdate {
    pub player_id:    PeerId,
    pub chips:        Chips,
    pub bet:          Chips,
    pub action:       PlayerAction,
    pub action_timer: Option<u64,>, /* use u64 instead of Instant to make
                                     * serializable. */
    pub is_dealer:    bool,
    pub is_active:    bool,
    pub hole_cards:   PlayerCards,
}

/// A Player action.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq,)]
pub enum PlayerAction {
    /// No action.
    None,
    /// Player pays small blind.
    SmallBlind,
    /// Player pays big blind.
    BigBlind,
    /// Player calls.
    Call,
    /// Player checks.
    Check,
    /// Player bets.
    Bet,
    /// Player raises.
    Raise,
    /// Player folds.
    Fold,
}

impl PlayerAction {
    /// The action label.
    #[must_use]
    pub const fn label(&self,) -> &'static str {
        match self {
            Self::SmallBlind => "SB",
            Self::BigBlind => "BB",
            Self::Call => "CALL",
            Self::Check => "CHECK",
            Self::Bet => "BET",
            Self::Raise => "RAISE",
            Self::Fold => "FOLD",
            Self::None => "",
        }
    }
}

/// Hand payoff description.
#[derive(Clone, Debug, Serialize, Deserialize,)]
pub struct HandPayoff {
    /// The player receiving the payment.
    pub player_id: PeerId,
    /// The payment amount.
    pub chips:     Chips,
    /// The winning cards.
    pub cards:     Vec<Card,>,
    /// Cards rank description.
    pub rank:      String,
}

/// A signed message.
#[derive(Debug, Clone, Deserialize, Serialize,)]
pub struct SignedMessage {
    /// Clonable payload for broadcasting to multiple connection tasks.
    payload: Arc<Payload,>,
}

/// Private signed message payload.
#[derive(Debug, Clone, Serialize, Deserialize,)]
struct Payload {
    msg:        Message,
    sig:        Signature,
    public_key: PublicKey,
}

impl SignedMessage {
    /// Creates a new signed message.
    #[must_use]
    pub fn new(key_pair: &KeyPair, msg: Message,) -> Self {
        let sig = key_pair.secret().sign(&msg,);
        Self {
            payload: Arc::new(Payload {
                msg,
                sig,
                public_key: key_pair.public(),
            },),
        }
    }

    /// Deserializes this message and verifies its signature.
    pub fn deserialize_and_verify(buf: Vec<u8,>,) -> Result<Self,> {
        let payload = bincode::deserialize::<Payload,>(&buf,)?;
        let sm = Self {
            payload: Arc::new(payload,),
        };

        if !sm
            .payload
            .public_key
            .verify(&sm.payload.msg, &sm.payload.sig,)
        {
            bail!("Invalid signature");
        }

        Ok(sm,)
    }

    /// Serializes this message.
    #[must_use]
    pub fn serialize(&self,) -> Vec<u8,> {
        let payload = self.payload.clone();
        bincode::serialize(payload.as_ref(),)
            .expect("Failed to serialize signed message",)
    }

    /// Returns the identifier of the player who sent this message.
    #[must_use]
    pub fn sender(&self,) -> PeerId {
        self.payload.public_key.to_peer_id()
    }

    /// Extracts the signed message (payload).
    #[must_use]
    pub fn message(&self,) -> &Message {
        &self.payload.msg
    }
}

impl fmt::Display for SignedMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{}", self.message().label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_message() {
        let key_pair = KeyPair::default();
        let public_key = key_pair.public();
        let secret_key = key_pair.secret();
        let peer_id = public_key.to_peer_id();
        let chips = Chips::default();
        let message = Message::PlayerJoinTableRequest {
            player_id: peer_id,
            nickname: "Alice".to_string(),
            table_id: TableId(0,),
            chips,
        };

        let signed_message = SignedMessage::new(&key_pair, message,);
        let bytes = signed_message.serialize();

        let deserialized_msg =
            SignedMessage::deserialize_and_verify(bytes,).unwrap();
        assert!(
            matches!(deserialized_msg.message(), Message::PlayerJoinTableRequest{nickname, player_id, chips, table_id} if nickname == "Alice" && peer_id == *player_id && *chips == Chips::default() && *table_id == TableId(0))
        );
    }
}
