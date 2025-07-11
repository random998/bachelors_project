use serde::{Serialize, Deserialize, Deserializer};
use blake3::Hash;
use crate::protocol::msg::*;
use crate::crypto::PeerId;

#[derive(Clone, Serialize, Deserialize)]
pub enum Phase { Waiting, Starting, Ready }

#[derive(Clone, Serialize, Deserialize)]
pub struct PlayerFlags {
    pub notified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ContractState {
    pub phase:   Phase,
    pub players: std::collections::BTreeMap<PeerId, PlayerFlags>,
}

impl Default for ContractState {
    fn default() -> Self { Self { phase: Phase::Waiting, players: Default::default() } }
}

/* ---------- single deterministic transition ------------------------------ */
pub fn step(prev: &ContractState, msg: &WireMsg) -> anyhow::Result<ContractState> {
    let mut st = prev.clone();
    match msg {
        WireMsg::PlayerJoinedConf { player_id, .. } => {
            st.players.entry(*player_id).or_insert(PlayerFlags { notified: false });
        }
        WireMsg::StartGameNotify { seat_order, .. } => {
            for pid in seat_order {
                st.players.entry(*pid).or_default().notified = true;
            }
            if st.players.values().all(|f| f.notified) {
                st.phase = Phase::Ready;
            } else {
                st.phase = Phase::Starting;
            }
        }
        _ => {} // ignore others for now
    }
    Ok(st)
}

/* helper for hashing */
pub fn hash_state(st: &ContractState) -> Hash {
    let bytes = bincode::serialize(st).unwrap();
    blake3::hash(&bytes)
}

