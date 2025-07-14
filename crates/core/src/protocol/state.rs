use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::protocol::msg::{Hash, WireMsg};
use once_cell::sync::Lazy;

pub static GENESIS_HASH: Lazy<Hash> = Lazy::new(|| {
    let empty = ContractState::default();
    hash_state(&empty)
});


#[derive(Clone, Debug, Serialize, Deserialize,)]
pub enum Phase {
    Waiting,
    Starting,
    Ready,
}

#[derive(Clone, Serialize, Deserialize, Default,)]
pub struct PlayerFlags {
    pub notified: bool,
}

pub struct BTreeMap(pub std::collections::BTreeMap<PeerId, PlayerFlags,>,);
#[derive(Clone, Serialize, Deserialize,)]
pub struct ContractState {
    pub phase:   Phase,
    pub players: std::collections::BTreeMap<PeerId, PlayerFlags,>,
}

impl Default for ContractState {
    fn default() -> Self {
        Self {
            phase:   Phase::Waiting,
            players: Default::default(),
        }
    }
}

impl std::fmt::Debug for ContractState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_,>,) -> std::fmt::Result {
        f.write_str(&format!(
            "phase: {:?}, players tree len: {:?}",
            self.phase,
            self.players.len()
        ),)
    }
}

// ---------- single deterministic transition ------------------------------
pub fn step(
    prev: &ContractState,
    msg: &WireMsg,
) -> anyhow::Result<ContractState,> {
    let mut st = prev.clone();
    match msg {
        WireMsg::PlayerJoinedConf { player_id, .. } => {
            st.players
                .entry(*player_id,)
                .or_insert(PlayerFlags { notified: false, },);
        },
        WireMsg::StartGameNotify { seat_order, .. } => {
            for pid in seat_order {
                st.players.entry(*pid,).or_default().notified = true;
            }
            if st.players.values().all(|f| f.notified,) {
                st.phase = Phase::Ready;
            } else {
                st.phase = Phase::Starting;
            }
        },
        _ => {}, // ignore others for now
    }
    Ok(st,)
}

// helper for hashing
#[must_use]
pub fn hash_state(st: &ContractState,) -> Hash {
    let bytes = bincode::serialize(st,).unwrap();
    let hash = blake3::hash(&bytes,);
    Hash(hash,)
}
