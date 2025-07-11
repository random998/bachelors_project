use blake3::Hash;
use serde::{Serialize, Deserialize};
use crate::poker::*;        // Chips, Card, etc.
use crate::zk::*;           // Proof, Commitment, ShuffleProof, …

/// Every logged packet is hash-chained & ZK-verified.
#[derive(Serialize, Deserialize)]
pub struct LogEntry {
    pub prev_hash: Hash,       // h_{i-1}
    pub payload:   WireMsg,
    pub next_hash: Hash,       // h_i = H(state_after_step)
    pub proof:     Proof,      // zkVM proof: step(prev,msg) = next
}

/// User-visible envelope (signed by the sender).
#[derive(Serialize, Deserialize)]
pub enum WireMsg {
    // ───── Lobby / seating ────────────────────────────────────────────
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
    LeaveTable { player_id: PeerId },

    // ───── Game start & shuffle ───────────────────────────────────────
    StartGameNotify {
        table:     TableId,
        game_id:   GameId,
        seat_order: Vec<PeerId>,        // dealer = seat[0]
        sb: Chips,
        bb: Chips,
    },
    ShuffleCommit {
        deck_commit: Commitment,        // commitment to permuted deck
        shuffle_proof: ShuffleProof,    // ZK perm-proof (σ,rand) hidden
    },

    // ───── Betting & action phase ─────────────────────────────────────
    ActionRequest {
        table:     TableId,
        game_id:   GameId,
        player_id: PeerId,
        min_raise: Chips,
        big_blind: Chips,
        allowed:   Vec<PlayerAction>,   // Fold / Call / Raise / …
    },
    PlayerAction {
        table:     TableId,
        game_id:   GameId,
        player_id: PeerId,
        action:    PlayerAction,        // Fold, Call, Bet(Δ), Raise(Δ)
        bet_commit: Commitment,         // Pedersen commit to bet amount
        range_proof: RangeProof,        // bet ∈ [0, stack]
    },
    TimeoutFold { player_id: PeerId },   // produced by *any* peer

    // ───── Showdown & results ─────────────────────────────────────────
    RevealCards {
        player_id: PeerId,
        c1: Card, c2: Card,
        open_rand: [u8;32],             // opens commit for bet validation
    },
    EndHand {
        pot: Chips,
        payoffs: Vec<(PeerId, Chips)>,
    },
    EndGame {},

    // ───── Aux / infra ────────────────────────────────────────────────
    Throttle { millis: u32 },            // UI pacing
    Ping,
}

