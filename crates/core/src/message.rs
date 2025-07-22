use std::fmt;
use std::fmt::{Display, Formatter, Write};
use std::sync::Arc;

/// Type definitions for p2p messages.
use anyhow::{Result, bail};
use libp2p;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::crypto::{KeyPair, PeerId, PublicKey, Signature};
use crate::game_state::GameState;
use crate::poker::{Card, Chips, PlayerCards, TableId};
use crate::protocol::msg::LogEntry;

/// Represents a message exchanged between peers in the P2P poker protocol.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
pub enum NetworkMessage {
    /// protocol entry for zk log.
    ProtocolEntry(LogEntry,),
    /// the network has given this peer a new listen address.
    NewListenAddr {
        listener_id: String,
        multiaddr:   Multiaddr,
    },
    SyncReq {
        table:     TableId,
        player_id: PeerId,
        nickname:  String,
        chips:     Chips,
    },
    SyncResp {
        target: PeerId,
        chain:  Vec<LogEntry,>,
    },
}

/// Represents a message send from the p2p poker instance to the ui.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
pub enum UiEvent {
    Snapshot(GameState,), // fresh projection every frame / on tick
    ActionRequest {
        allowed:   Vec<PlayerAction,>,
        min_raise: Chips,
    },
    Error(String,), // protocol or network issues
    PeerUpdate {
        peer_id: PeerId,
        online:  bool,
    },
    Chat {
        from: PeerId,
        text: String,
    },
}

/// Sent from egui/iced/etc. into the Tokio task driving Projection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize,)]
pub enum UiCmd {
    Connect {
        nickname: String,
        buy_in:   Chips,
    },
    SeatRequest {
        seat: u8,
    },
    LeaveTable,
    Action {
        kind:   PlayerAction,
        amount: Chips,
    }, // Bet/Call/Check/Fold
    ToggleReady, // “I’m ready to deal”
    Chat {
        text: String,
    },
    PlayerJoinTableRequest {
        table_id: TableId,
        peer_id:  PeerId,
        nickname: String,
        chips:    Chips,
    },
}
impl Display for UiCmd {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
        let str = match self {
            Self::Connect { .. } => "connect",
            Self::SeatRequest { .. } => "seat",
            Self::LeaveTable => "leave_table",
            Self::Action { .. } => "action",
            Self::ToggleReady => "toggle_ready",
            Self::Chat { .. } => "chat",
            Self::PlayerJoinTableRequest { .. } => "player_join_table_request",
        };
        write!(f, "{str}")
    }
}

impl NetworkMessage {
    // Returns a label of the message variant as a string.
    #[must_use]
    pub fn label(&self,) -> String {
        match self {
            Self::ProtocolEntry(logentry,) => {
                format!("ProtocolEntry: {}", logentry.payload.label())
            },
            Self::NewListenAddr { .. } => "NewListenAddr".to_string(),
            Self::SyncReq { .. } => "SyncReq".to_string(),
            Self::SyncResp { .. } => "SyncResp".to_string(),
        }
    }
}

impl Display for NetworkMessage {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
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
    Bet { bet_amount: Chips, },
    /// Player raises.
    Raise { bet_amount: Chips, },
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
            Self::Bet { .. } => "BET",
            Self::Raise { .. } => "RAISE",
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
    msg:        NetworkMessage,
    sig:        Signature,
    public_key: PublicKey,
}

impl SignedMessage {
    /// Creates a new signed message.
    #[must_use]
    pub fn new(key_pair: &KeyPair, msg: NetworkMessage,) -> Self {
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
    pub fn message(&self,) -> &NetworkMessage {
        &self.payload.msg
    }
}

impl Display for SignedMessage {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
        write!(f, "{}", self.message().label())
    }
}
