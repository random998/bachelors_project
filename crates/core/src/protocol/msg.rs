//! Hash-chained packet format (ready for ZK integration)

use std::fmt::Write;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::crypto::PeerId;
use crate::message::{HandPayoff, PlayerAction};
use crate::poker::{Card, Chips, GameId, TableId};
use crate::zk::{Commitment, Proof, RangeProof, ShuffleProof}; // empty stub today

// ---------- Public envelope ------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize,)]
pub struct LogEntry {
    pub prev_hash: Hash,
    pub payload:   WireMsg,
    pub next_hash: Hash,
    /// zkVM proof that `step(prev, payload) -> next`
    pub proof:     Proof,
    pub metadata:  EntryMetaData,
}

#[derive(Clone, Debug, Serialize, Deserialize,)]
pub struct EntryMetaData {
    pub ts_micros: u128,
    pub author:    PeerId,
}

impl LogEntry {
    #[must_use]
    pub fn with_key(
        prev_hash: Hash,
        payload: WireMsg,
        next_hash: Hash,
        author: PeerId,
    ) -> Self {
        Self {
            prev_hash,
            payload,
            next_hash,
            metadata: EntryMetaData {
                ts_micros: chrono::Utc::now().timestamp_micros() as u128,
                author,
            },
            proof: Proof::default(),
        }
    }
}

#[derive(Clone, Debug,)]
pub struct Hash(pub blake3::Hash,);

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter,) -> std::fmt::Result {
        self.0.fmt(f,)
    }
}
impl Serialize for Hash {
    fn serialize<S,>(&self, serializer: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        serializer.serialize_bytes(self.0.as_bytes(),)
    }
}

impl PartialEq<Self,> for Hash {
    fn eq(&self, other: &Self,) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}

impl<'de,> Deserialize<'de,> for Hash {
    fn deserialize<D,>(deserializer: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes = <&[u8]>::deserialize(deserializer,)?;
        let hash = blake3::Hash::from_slice(bytes,)
            .expect("error deserializing hash",);
        Ok(Self(hash,),)
    }
}

// ---------- All message kinds ----------------------------------------------
#[derive(Clone, Debug, Serialize, Deserialize,)]
pub enum WireMsg {
    // ── lobby ─────────────────────────────────────────────
    /// Dealer sends each player their two private cards (**plaintext – will be
    /// replaced by commitments later**).
    DealCards {
        table:     TableId,
        game_id:   GameId,
        player_id: PeerId, // receiver
        card1:     Card,
        card2:     Card,
    },

    JoinTableReq {
        table:     TableId,
        player_id: PeerId,
        nickname:  String,
        chips:     Chips,
    },
    LeaveTable {
        player_id: PeerId,
    },

    // ── game bootstrap & shuffle ─────────────────────────
    StartGameNotify {
        table:      TableId,
        game_id:    GameId,
        seat_order: Vec<PeerId,>,
        sb:         Chips,
        bb:         Chips,
    },
    ShuffleCommit {
        deck_commit:   Commitment,
        shuffle_proof: ShuffleProof,
    },

    // ── betting ───────────────────────────────────────────
    ActionRequest {
        table:     TableId,
        game_id:   GameId,
        player_id: PeerId,
        min_raise: Chips,
        big_blind: Chips,
        allowed:   Vec<PlayerAction,>,
    },
    PlayerAction {
        table:       TableId,
        game_id:     GameId,
        peer_id:     PeerId,
        action:      PlayerAction,
        bet_commit:  Commitment,
        range_proof: RangeProof,
    },
    TimeoutFold {
        player_id: PeerId,
    },

    // ── showdown ──────────────────────────────────────────
    RevealCards {
        player_id: PeerId,
        c1:        Card,
        c2:        Card,
        open_rand: [u8; 32],
    },
    EndHand {
        pot:     Chips,
        payoffs: Vec<HandPayoff,>,
    },
    EndGame {},

    // ── misc ──────────────────────────────────────────────
    Throttle {
        millis: u32,
    },
    Ping,
}

impl WireMsg {
    #[must_use]
    pub fn label(&self,) -> String {
        match self {
            Self::JoinTableReq { .. } => "JoinTableReq".to_string(),
            Self::LeaveTable { .. } => "LeaveTable".to_string(),
            Self::StartGameNotify { .. } => "StartGameNotify".to_string(),
            Self::ShuffleCommit { .. } => "ShuffleCommit".to_string(),
            Self::ActionRequest { .. } => "ActionRequest".to_string(),
            Self::PlayerAction { .. } => "PlayerAction".to_string(),
            Self::TimeoutFold { .. } => "TimeoutFold".to_string(),
            Self::RevealCards { .. } => "RevealCards".to_string(),
            Self::EndHand { .. } => "EndHand".to_string(),
            Self::EndGame { .. } => "EndGame".to_string(),
            Self::Throttle { .. } => "Throttle".to_string(),
            Self::Ping => "Ping".to_string(),
            Self::DealCards { .. } => "DealCards".to_string(),
        }
    }
}
