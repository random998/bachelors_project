//! Hash-chained packet format (ready for ZK integration)

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::crypto::PeerId;
use crate::message::{HandPayoff, PlayerAction};
use crate::poker::*;
use crate::zk::{Commitment, Proof, RangeProof, ShuffleProof}; // empty stub today

// ---------- Public envelope ------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize,)]
pub struct LogEntry {
    pub prev_hash: Hash,
    pub payload:   WireMsg,
    pub next_hash: Hash,
    /// zkVM proof that `step(prev, payload) -> next`
    pub proof:     Proof,
}

#[derive(Clone, Debug,)]
pub struct Hash(pub blake3::Hash,);

impl Serialize for Hash {
    fn serialize<S,>(&self, serializer: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        serializer.serialize_bytes(self.0.as_bytes(),)
    }
}

impl PartialEq<Hash,> for Hash {
    fn eq(&self, other: &Hash,) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}

impl<'de,> Deserialize<'de,> for Hash {
    fn deserialize<D,>(deserializer: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes = <&[u8]>::deserialize(deserializer,)?;
        let hash = blake3::Hash::from_slice(bytes,)
            .expect("error deserializing hash",);
        Ok(Hash(hash,),)
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
    PlayerJoinedConf {
        table:     TableId,
        player_id: PeerId,
        seat_idx:  u8,
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
        player_id:   PeerId,
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
