use std::fmt;
use std::fmt::Formatter;

use log::info;
use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::game_state::PlayerPrivate;
use crate::poker::Chips;
use crate::protocol::msg::{Hash, WireMsg};

pub static GENESIS_HASH: std::sync::LazyLock<Hash,> =
    std::sync::LazyLock::new(|| {
        let empty = ContractState::default();
        hash_state(&empty,)
    },);

#[derive(Clone,)]
pub struct PeerContext {
    pub id:    PeerId,
    pub nick:  String,
    pub chips: Chips,
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize,)]
pub enum HandPhase {
    WaitingForPlayers, /* waiting till the required amount of players
                        * joined the table. */
    StartingGame, // waiting for all peers to send StartingGame Notification.
    StartingHand,
    PreflopBetting,
    Preflop,
    FlopBetting,
    Flop,
    TurnBetting,
    Turn,
    RiverBetting,
    River,
    Showdown,
    EndingHand,
    EndingGame,
}

impl fmt::Display for HandPhase {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
        use HandPhase::{
            EndingGame, EndingHand, Flop, FlopBetting, Preflop, PreflopBetting,
            River, RiverBetting, Showdown, StartingGame, StartingHand, Turn,
            TurnBetting, WaitingForPlayers,
        };
        let s = match self {
            WaitingForPlayers => "WaitingForPlayers",
            StartingGame => "StartingGame",
            StartingHand => "StartingHand",
            PreflopBetting => "PreflopBetting",
            Preflop => "Preflop",
            FlopBetting => "FlopBetting",
            Flop => "Flop",
            TurnBetting => "TurnBetting",
            Turn => "Turn",
            RiverBetting => "RiverBetting",
            River => "River",
            Showdown => "Showdown",
            EndingHand => "EndingHand",
            EndingGame => "EndingGame",
        };
        write!(f, "{s}")
    }
}

impl PeerContext {
    #[must_use]
    pub const fn new(id: PeerId, nick: String, chips: Chips,) -> Self {
        Self { id, nick, chips, }
    }

    #[must_use]
    pub fn default() -> Self {
        Self {
            id:    PeerId::default(),
            nick:  String::default(),
            chips: Chips::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize,)]
pub enum Phase {
    Waiting,
    Starting,
    Ready,
}

/// Pure transition result
pub struct StepResult {
    pub next:    ContractState,
    pub effects: Vec<Effect,>,
}

/// Things that _should_ be sent after the state is committed
#[derive(Clone, Serialize, Deserialize,)]
pub enum Effect {
    Send(WireMsg,),
}

#[derive(Clone, Serialize, Deserialize,)]
pub struct ContractState {
    pub phase:     HandPhase,
    pub players:   std::collections::BTreeMap<PeerId, PlayerPrivate,>,
    pub num_seats: u64,
}

impl Default for ContractState {
    fn default() -> Self {
        Self::new(3,)
    }
}

impl ContractState {
    fn new(num_seats: u64,) -> Self {
        Self {
            phase: HandPhase::WaitingForPlayers,
            players: Default::default(),
            num_seats,
        }
    }
}

impl fmt::Debug for ContractState {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
        f.write_str(&format!(
            "phase: {:?}, players tree len: {:?}",
            self.phase,
            self.players.len()
        ),)
    }
}

// ---------- single deterministic transition ------------------------------
#[must_use]
pub fn step(prev: &ContractState, msg: &WireMsg,) -> StepResult {
    let mut st = prev.clone();
    let mut out = Vec::new();

    match msg {
        WireMsg::StartGameBatch(batch,) => {
            // Verify: Complete, sorted, valid
            let expected_senders: Vec<PeerId,> =
                st.players.keys().copied().collect();

            let mut batch_senders_sorted: Vec<PeerId,> = batch
                .iter()
                .map(super::super::message::SignedMessage::sender,)
                .collect();
            batch_senders_sorted.sort_by_key(std::string::ToString::to_string,);

            let mut expected_senders_sorted: Vec<PeerId,> =
                expected_senders.clone();
            expected_senders_sorted
                .sort_by_key(std::string::ToString::to_string,);

            if batch_senders_sorted != expected_senders_sorted
                || batch_senders_sorted.len() != expected_senders.len()
            {
                info!(
                    "invalid startGameBatch message, rejecting:\n\
                    batch_senders_len: {:#?},\n\
                    expected_senders_len: {:#?}",
                    batch_senders_sorted, expected_senders_sorted
                );
                return StepResult {
                    next:    prev.clone(),
                    effects: vec![],
                };
            }

            // Verify each signature and fields match
            for sm in batch {
                if !sm.verify() {
                    return StepResult {
                        next:    prev.clone(),
                        effects: vec![],
                    };
                }
                // Apply: Set flags
                if let Some(p,) = st.players.get_mut(&sm.sender(),) {
                    p.has_sent_start_game_notification = true;
                }
            }
            // All good: Advance phase
            if st
                .players
                .values()
                .all(|p| p.has_sent_start_game_notification,)
            {
                st.phase = HandPhase::StartingHand;
            }
            // add startGameBatch message to effects, since we want to send it
            // to our peers.else {
            let eff = Effect::Send(msg.clone(),);
            out.push(eff,)
        },
        WireMsg::JoinTableReq {
            player_id,
            table: _table,
            chips,
            nickname,
        } => {
            st.players.insert(
                *player_id,
                PlayerPrivate::new(*player_id, nickname.clone(), *chips,),
            );

            if st.players.len() >= st.num_seats as usize {
                st.phase = HandPhase::StartingGame;
            }
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
        next:    st,
        effects: out,
    }
}

// helper for hashing
#[must_use]
pub fn hash_state(st: &ContractState,) -> Hash {
    let bytes = bincode::serialize(st,);
    let hash = blake3::hash(bytes.unwrap().as_slice(),);
    Hash(hash,)
}
