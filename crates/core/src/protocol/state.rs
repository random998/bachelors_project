use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::protocol::msg::{Hash, WireMsg};

pub static GENESIS_HASH: std::sync::LazyLock<Hash,> =
    std::sync::LazyLock::new(|| {
        let empty = ContractState::default();
        hash_state(&empty,)
    },);

#[derive(Clone, Debug, Serialize, Deserialize,)]
pub enum Phase {
    Waiting,
    Starting,
    Ready,
}

/// Pure transition result
pub struct StepResult {
    pub next:   ContractState,
    pub effects: Vec<Effect>,   // <-- new
}

/// Things that _should_ be sent after the state is committed
#[derive(Clone, Serialize, Deserialize)]
pub enum Effect {
    Send(WireMsg),
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
        WireMsg::JoinTableReq { player_id, .. } => {
            st.players.insert(*player_id, PlayerFlags{ notified:false });

            // ↯ If _I_ am not yet in the table, queue my own Join request
            if !st.players.contains_key(&self.p) {
                st.effects.push(Effect::Send(WireMsg::JoinTableReq {
                    table: *table,
                    player_id: my_peer_id,
                    nickname: my_nick.clone(),
                    chips: my_chips,
                }));
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
