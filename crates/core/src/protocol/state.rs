use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::game_state::PlayerPrivate;
use crate::poker::Chips;
use crate::protocol::msg::{Hash, WireMsg};

pub static GENESIS_HASH: std::sync::LazyLock<Hash> =
    std::sync::LazyLock::new(|| {
        let empty = ContractState::default();
        hash_state(&empty)
    });

#[derive(Clone)]
pub struct PeerContext {
    pub id: PeerId,
    pub nick: String,
    pub chips: Chips,
}

impl PeerContext {
    #[must_use]
    pub const fn new(id: PeerId, nick: String, chips: Chips) -> Self {
        Self { id, nick, chips }
    }

    pub fn default() -> Self {
        Self {
            id: PeerId::default(),
            nick: String::default(),
            chips: Chips::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Phase {
    Waiting,
    Starting,
    Ready,
}

/// Pure transition result
pub struct StepResult {
    pub next: ContractState,
    pub effects: Vec<Effect>,
}

/// Things that _should_ be sent after the state is committed
#[derive(Clone, Serialize, Deserialize)]
pub enum Effect {
    Send(WireMsg),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ContractState {
    pub phase: Phase,
    pub players: std::collections::BTreeMap<PeerId, PlayerPrivate>,
}

impl Default for ContractState {
    fn default() -> Self {
        Self {
            phase: Phase::Waiting,
            players: Default::default(),
        }
    }
}

impl std::fmt::Debug for ContractState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "phase: {:?}, players tree len: {:?}",
            self.phase,
            self.players.len()
        ))
    }
}

// ---------- single deterministic transition ------------------------------
#[must_use]
pub fn step(prev: &ContractState, msg: &WireMsg) -> StepResult {
    let mut st = prev.clone();
    let out = Vec::new();

    match msg {
        WireMsg::StartGameNotify {
            seat_order: _seat_order,
            ..
        } => {
            if st
                .players
                .values()
                .all(|p| p.has_sent_start_game_notification)
            {
                st.phase = Phase::Ready;
            } else {
                st.phase = Phase::Starting;
            }
        },
        WireMsg::JoinTableReq {
            player_id,
            table: _table,
            chips,
            nickname,
        } => {
            st.players.insert(
                *player_id,
                PlayerPrivate::new(*player_id, nickname.clone(), *chips),
            );
        },
        WireMsg::StartGameNotify {
            seat_order: _seat_order,
            ..
        } => {
            todo!()
        },
        WireMsg::DealCards { .. } => {
            todo!()
        },
        WireMsg::ActionRequest { .. } => {
            todo!()
        },
        WireMsg::Ping => {
            todo!()
        },
        _ => {
            todo!()
        },
    }
    StepResult {
        next: st,
        effects: out,
    }
}

// helper for hashing
#[must_use]
pub fn hash_state(st: &ContractState) -> Hash {
    let bytes = bincode::serialize(st).unwrap();
    let hash = blake3::hash(&bytes);
    Hash(hash)
}