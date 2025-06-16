// based on https://github.com/vincev/freezeout

/// Type definitions for p2p messages.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    crypto::{PeerId, Signature, SigningKey, VerifyingKey},
    poker::{Card, Chips, PlayerCards, TableId},
};
/// Represents a message exchanged between peers in the P2P poker protocol.
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// Request to join a table with the given nickname.
    JoinTable {
        player_id: PeerId,
        nickname: String,
    },

    /// Confirmation that the player has joined a table.
    TableJoined {
        player_id: PeerId,
        chips: Chips,
        table_id: TableId,
    },

    /// Notification that a player has left their current table.
    LeaveTable {
        player_id: PeerId,
    },

    /// Sent when no available tables remain for the player to join.
    NoTablesLeft,

    /// Indicates that the player does not have enough chips to join a game.
    NotEnoughChips,

    /// Indicates that the player has already joined a table.
    PlayerAlreadyJoined,

    /// Confirmation that a specific player has joined a specific table.
    PlayerJoined {
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
    StartGame(Vec<PeerId>),

    /// Begins a new poker hand.
    StartHand,

    /// Ends the current poker hand, providing final results.
    EndHand {
        /// Final payoffs for each player.
        payoffs: Vec<Payoff>,

        /// Community cards on the board at showdown.
        board: Vec<Card>,

        /// Map of each player's ID to their revealed hole cards.
        cards: Vec<(PeerId, PlayerCards)>,
    },

    /// Deals two hole cards to the player.
    DealCards(Card, Card),

    /// Notifies peers that a player has left the table.
    PlayerLeftTable(PeerId),

    /// Updated game state including players, board, and pot.
    GameStateUpdate {
        players: Vec<PlayerUpdate>,
        board: Vec<Card>,
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
        actions: Vec<PlayerAction>,
    },

    /// Response from a player specifying their action and any bet amount.
    ActionResponse {
        /// Action taken (Fold, Call, Raise, etc.).
        action: PlayerAction,

        /// Chips committed with the action, if applicable (used for Bet/Raise).
        amount: Chips,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerUpdate {
    pub player_id: PeerId,
    pub chips: Chips,
    pub bet: Chips,
    pub action: PlayerAction,
    pub action_timer: Option<u16>,
    pub is_dealer: bool,
    pub is_active: bool,
}

/// A Player action.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
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
    pub fn label(&self) -> &'static str {
        match self {
            PlayerAction::SmallBlind => "SB",
            PlayerAction::BigBlind => "BB",
            PlayerAction::Call => "CALL",
            PlayerAction::Check => "CHECK",
            PlayerAction::Bet => "BET",
            PlayerAction::Raise => "RAISE",
            PlayerAction::Fold => "FOLD",
            PlayerAction::None => "",
        }
    }
}

/// Hand payoff description.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandPayoff {
    /// The player receiving the payment.
    pub player_id: PeerId,
    /// The payment amount.
    pub chips: Chips,
    /// The winning cards.
    pub cards: Vec<Card>,
    /// Cards rank description.
    pub rank: String,
}

/// A signed message.
#[derive(Debug, Clone)]
pub struct SignedMessage {
    /// Clonable payload for broadcasting to multiple connection tasks.
    payload: Arc<Payload>,
}

/// Private signed message payload.
#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    msg: Message,
    sig: Signature,
    vk: VerifyingKey,
}

impl SignedMessage {
    /// Creates a new signed message.
    pub fn new(sk: &SigningKey, msg: Message) -> Self {
        let sig = sk.sign(&msg);
        SignedMessage {
            payload: Arc::new(Payload {
                msg,
                sig,
                vk: sk.verifying_key(),
            }),
        }
    }

    /// Deserializes this message and verifies its signature.
    pub fn deserialize_and_verify(buf: &[u8]) -> Result<Self> {
        let sm = SignedMessage {
            payload: Arc::new(bincode::deserialize::<Payload>(buf)?),
        };

        if !sm.payload.vk.verify(&sm.payload.msg, &sm.payload.sig) {
            bail!("Invalid signature");
        }

        Ok(sm)
    }

    /// Serializes this message.
    pub fn serialize(&self) -> [u8] {
        bincode::serialize(self.payload.as_ref()).expect("Failed to serialize signed message")
    }

    /// Returns the identifier of the player who sent this message.
    pub fn sender(&self) -> PeerId {
        self.payload.vk.peer_id()
    }

    /// Extracts the signed message (payload).
    pub fn message(&self) -> &Message {
        &self.payload.msg
    }
}

#[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn signed_message() {
            let sk = SigningKey::default();
            let message = Message::JoinServer {
                nickname: "Alice".to_string(),
            };

            let smsg = SignedMessage::new(&sk, message);
            let bytes = smsg.serialize();

            let deser_msg = SignedMessage::deserialize_and_verify(&bytes).unwrap();
            assert!(
                matches!(deser_msg.message(), Message::JoinServer{ nickname } if nickname == "Alice")
            );
        }
    }
