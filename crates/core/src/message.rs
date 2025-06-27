// based on https://github.com/vincev/freezeout

use std::fmt;
use std::sync::Arc;

/// Type definitions for p2p messages.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::crypto::{PeerId, Signature, SigningKey, VerifyingKey};
use crate::poker::{Card, Chips, PlayerCards, TableId};
/// Represents a message exchanged between peers in the P2P poker protocol.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
pub enum Message {
    /// client requests to join a server.
    JoinServerRequest {
        /// The player nickname.
        nickname: String,
        player_id: PeerId,
    },

    /// response by server that particular client joined the server.
    JoinedServerConfirmation {
        /// The player nickname.
        nickname: String,
        /// The chips amount for the player.
        chips: Chips,
        player_id: PeerId,
    },

    /// Request (by a player) to join a table with the given nickname.
    JoinTableRequest {
        player_id: PeerId,
        nickname: String,
    },

    /// Notification that a player has left their current table.
    PlayerLeftNotification {
        player_id: PeerId,
    },

    /// Sent when no available tables remain for the player to join.
    NoTablesLeftNotification,

    /// Indicates that the player does not have enough chips to join a game.
    NotEnoughChips,

    /// Indicates that the player has already joined a table.
    PlayerAlreadyJoined,

    /// Confirmation that a specific player has joined a specific table.
    PlayerJoined {
        // TODO: implement display trait for this struct
        table_id: TableId,
        player_id: PeerId,
        nickname: String,
        chips: Chips,
    },

    /// Request to show the player’s current chip balance/account summary.
    ShowAccount {
        chips: Chips,
    },

    /// Starts the game and notifies all players of seat order.
    StartGame(Vec<PeerId,>,),

    /// Begins a new poker hand.
    StartHand,

    /// Ends the current poker hand, providing final results.
    EndHand {
        /// Final payoffs for each player.
        payoffs: Vec<HandPayoff,>,

        /// Community cards on the board at showdown.
        board: Vec<Card,>,

        /// Map of each player's ID to their revealed hole cards.
        cards: Vec<(PeerId, PlayerCards,),>,
    },

    /// Deals two hole cards to the player.
    DealCards(Card, Card,),

    /// Notifies peers that this player has left the table.
    PlayerLeftTable,

    /// Updated game state including players, board, and pot.
    GameStateUpdate {
        players: Vec<PlayerUpdate,>,
        community_cards: Vec<Card,>,
        pot: Chips,
    },

    /// Requests a specific player to make a game action (e.g., Call, Raise).
    ActionRequest {
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

        /// Chips committed with the action, if applicable (used for Bet/Raise).
        amount: Chips,
    },
}

impl Message {
    // Returns a label of the message variant as a string.
    #[must_use]
    pub const fn label(&self,) -> &'static str {
        match self {
            | Self::JoinTableRequest {
                ..
            } => "JoinTableRequest",
            | Self::PlayerLeftNotification {
                ..
            } => "PlayerLeftNotification",
            | Self::NoTablesLeftNotification => "NoTablesLeftNotification",
            | Self::NotEnoughChips => "NotEnoughChips",
            | Self::PlayerAlreadyJoined => "PlayerAlreadyJoined",
            | Self::PlayerJoined {
                ..
            } => "PlayerJoined",
            | Self::ShowAccount {
                ..
            } => "ShowAccount",
            | Self::StartGame(_,) => "StartGame",
            | Self::StartHand => "StartHand",
            | Self::EndHand {
                ..
            } => "EndHand",
            | Self::DealCards(..,) => "DealCards",
            | Self::PlayerLeftTable => "PlayerLeftTable",
            | Self::GameStateUpdate {
                ..
            } => "GameStateUpdate",
            | Self::ActionRequest {
                ..
            } => "ActionRequest",
            | Self::ActionResponse {
                ..
            } => "ActionResponse",
            | Self::JoinServerRequest {
                ..
            } => "JoinServerRequest",
            | Self::JoinedServerConfirmation {
                ..
            } => "JoinServerConfirmation",
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
    pub player_id: PeerId,
    pub chips: Chips,
    pub bet: Chips,
    pub action: PlayerAction,
    pub action_timer: Option<u16,>,
    pub is_dealer: bool,
    pub is_active: bool,
    pub hole_cards: PlayerCards,
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
            | Self::SmallBlind => "SB",
            | Self::BigBlind => "BB",
            | Self::Call => "CALL",
            | Self::Check => "CHECK",
            | Self::Bet => "BET",
            | Self::Raise => "RAISE",
            | Self::Fold => "FOLD",
            | Self::None => "",
        }
    }
}

/// Hand payoff description.
#[derive(Clone, Debug, Serialize, Deserialize,)]
pub struct HandPayoff {
    /// The player receiving the payment.
    pub player_id: PeerId,
    /// The payment amount.
    pub chips: Chips,
    /// The winning cards.
    pub cards: Vec<Card,>,
    /// Cards rank description.
    pub rank: String,
}

/// A signed message.
#[derive(Debug, Clone, Deserialize, Serialize,)]
pub struct SignedMessage {
    /// Clonable payload for broadcasting to multiple connection tasks.
    payload: Arc<Payload,>,
}

/// Private signed message payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
struct Payload {
    msg: Message,
    sig: Signature,
    vk: VerifyingKey,
}

impl SignedMessage {
    /// Creates a new signed message.
    #[must_use]
    pub fn new(sk: &SigningKey, msg: Message,) -> Self {
        let sig = sk.sign(&msg,);
        Self {
            payload: Arc::new(Payload {
                msg,
                sig,
                vk: sk.verifying_key(),
            },),
        }
    }

    /// Deserializes this message and verifies its signature.
    pub fn deserialize_and_verify(buf: &[u8],) -> Result<Self,> {
        let sm = Self {
            payload: Arc::new(bincode::deserialize::<Payload,>(buf,)?,),
        };

        if !sm.payload.vk.verify(&sm.payload.msg, &sm.payload.sig,) {
            bail!("Invalid signature");
        }

        Ok(sm,)
    }

    /// Serializes this message.
    #[must_use]
    pub fn serialize(&self,) -> Vec<u8,> {
        bincode::serialize(self.payload.as_ref(),).expect("Failed to serialize signed message",)
    }

    /// Returns the identifier of the player who sent this message.
    #[must_use]
    pub fn sender(&self,) -> PeerId {
        self.payload.vk.peer_id()
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
        let sk = SigningKey::default();
        let vk = sk.verifying_key();
        let peer_id = vk.peer_id();
        let message = Message::JoinTableRequest {
            player_id: peer_id,
            nickname: "Alice".to_string(),
        };

        let signed_message = SignedMessage::new(&sk, message,);
        let bytes = signed_message.serialize();

        let deserialized_msg = SignedMessage::deserialize_and_verify(&bytes,).unwrap();
        assert!(
            matches!(deserialized_msg.message(), Message::JoinTableRequest{nickname, player_id } if nickname == "Alice" && peer_id == *player_id)
        );
    }
}
