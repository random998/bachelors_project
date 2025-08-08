//! Hash-chained packet format (ready for ZK integration)
use std::fmt::{Display, Formatter};
use rand_core::RngCore;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::crypto::PeerId;
use crate::message::{HandPayoff, PlayerAction, SignedMessage};
use crate::poker::{Card, Chips, GameId, TableId};
use crate::zk::{Commitment, Proof, RangeProof, ShuffleProof};
// empty stub today
// ---------- Public envelope ------------------------------------------------
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LogEntry {
    pub prev_hash: Hash,
    pub payload: Transition,
    pub hash: Hash,
    /// zkVM proof that `step(prev, payload) -> next`
    pub proof: Proof,
    pub metadata: EntryMetaData,
}
impl PartialEq for LogEntry {
    fn eq(&self, other: &Self) -> bool {
        self.prev_hash == other.prev_hash
            && self.hash == other.hash
            && self.payload == other.payload
            && self.proof == other.proof
    }
}
impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "prev_hash: {}\n\
            payload: {}\n\
            next_hash: {}\n",
            self.prev_hash,
            self.payload.label(),
            self.hash
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryMetaData {
    pub ts_micros: i64,
    pub author: PeerId,
}
impl PartialEq for EntryMetaData {
    fn eq(&self, other: &Self) -> bool {
        self.ts_micros == other.ts_micros && self.author == other.author
    }
}
impl LogEntry {
    #[must_use]
    pub fn with_key(
        prev_hash: Hash,
        payload: Transition,
        next_hash: Hash,
        author: PeerId,
    ) -> Self {
        Self {
            prev_hash,
            payload,
            hash: next_hash,
            metadata: EntryMetaData {
                ts_micros: chrono::Utc::now().timestamp_micros(),
                author,
            },
            proof: Proof::default(),
        }
    }
}
#[derive(Clone, Debug, Eq, std::hash::Hash)]
pub struct Hash(pub blake3::Hash);
impl Hash {
    #[must_use]
    pub fn generate_random() -> Self {
        let size = 16; // Example: 16 bytes
        let mut byte_array = vec![0u8; size]; // Initialize a vector of zeros
        rand::rng().fill_bytes(&mut byte_array[..]); // Fill with random bytes
        let hash = blake3::hash(&byte_array);
        Self(hash)
    }
}
impl Display for Hash {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let mut bytes = self.0.as_bytes().clone();
        serializer.serialize_bytes(bytes.as_mut_slice())
    }
}
impl PartialEq<Self> for Hash {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}
impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let bytes = <&[u8]>::deserialize(deserializer)?;
        let hash = blake3::Hash::from_slice(bytes)
            .expect("error deserializing hash");
        Ok(Self(hash))
    }
}
// ---------- All message kinds ----------------------------------------------
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DealCards {
    pub(crate) card1: Card,
    pub(crate) card2: Card,
    pub(crate) player_id: PeerId, // Receiver
    pub(crate) table: TableId,
    pub(crate) game_id: GameId,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub enum Transition {
    // ── lobby ─────────────────────────────────────────────
    /// Dealer sends each player their two private cards (**plaintext – will be
    /// replaced by commitments later**).
    JoinTableReq {
        table: TableId,
        player_id: PeerId,
        nickname: String,
        chips: Chips,
    },
    LeaveTable {
        player_id: PeerId,
    },
    StartGameBatch(Vec<SignedMessage>), /* Sorted list of plain
                                         * StartGameNotify signed
                                         * messages */
    DealCardsBatch(Vec<DealCards>), // Sorted by receiver peer_id
    ShuffleCommit {
        deck_commit: Commitment,
        shuffle_proof: ShuffleProof,
    },
    // ── betting ───────────────────────────────────────────
    ActionRequest {
        table: TableId,
        game_id: GameId,
        player_id: PeerId,
        min_raise: Chips,
        big_blind: Chips,
        allowed: Vec<PlayerAction>,
    },
    PlayerAction {
        table: TableId,
        game_id: GameId,
        peer_id: PeerId,
        action: PlayerAction,
        bet_commit: Commitment,
        range_proof: RangeProof,
    },
    TimeoutFold {
        player_id: PeerId,
    },
    // ── showdown ──────────────────────────────────────────
    RevealCards {
        player_id: PeerId,
        c1: Card,
        c2: Card,
        open_rand: [u8; 32],
    },
    EndHand {
        pot: Chips,
        payoffs: Vec<HandPayoff>,
    },
    EndGame {},
    // ── misc ──────────────────────────────────────────────
    Throttle {
        millis: u32,
    },
    Ping,
}
impl Transition {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::StartGameBatch(..) => "StartGameBatch".to_string(),
            Self::JoinTableReq { .. } => "JoinTableReq".to_string(),
            Self::LeaveTable { .. } => "LeaveTable".to_string(),
            Self::ShuffleCommit { .. } => "ShuffleCommit".to_string(),
            Self::ActionRequest { .. } => "ActionRequest".to_string(),
            Self::PlayerAction { .. } => "PlayerAction".to_string(),
            Self::TimeoutFold { .. } => "TimeoutFold".to_string(),
            Self::RevealCards { .. } => "RevealCards".to_string(),
            Self::EndHand { .. } => "EndHand".to_string(),
            Self::EndGame { .. } => "EndGame".to_string(),
            Self::Throttle { .. } => "Throttle".to_string(),
            Self::Ping => "Ping".to_string(),
            Self::DealCardsBatch(..) => "DealCards".to_string(),
        }
    }
}
