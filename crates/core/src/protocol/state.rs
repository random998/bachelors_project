use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Formatter;

use log::info;
use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::message::{HandPayoff, PlayerAction};
use crate::poker::PlayerCards::Cards;
use crate::poker::{Chips, PlayerCards};
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
}
impl Default for PeerContext {
    fn default() -> Self {
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
    phase:     HandPhase,
    players:   BTreeMap<PeerId, PlayerPrivate,>,
    num_seats: usize,
}

impl ContractState {
    #[must_use]
    pub fn get_phase(&self,) -> HandPhase {
        self.phase.clone()
    }

    pub const fn set_phase(&mut self, phase: HandPhase,) {
        self.phase = phase;
    }

    pub fn clear_players(&mut self,) {
        self.players.clear();
    }


    #[must_use]
    pub const fn get_num_seats(&self,) -> usize {
        self.num_seats
    }
}
impl ContractState {
    pub fn get_players_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut PlayerPrivate,> {
        self.players.values_mut()
    }

    pub fn get_players(
        &self,
    ) -> Vec<PlayerPrivate> {
        self.players.values().cloned().collect()
    }

    pub fn get_player_mut(
        &mut self,
        id: PeerId,
    ) -> Option<&mut PlayerPrivate,> {
        self.get_players_mut().find(|p| p.peer_id == id,)
    }
}

impl Default for ContractState {
    fn default() -> Self {
        Self::new(3,)
    }
}

impl ContractState {
    pub(crate) fn new(num_seats: usize,) -> Self {
        Self {
            phase: HandPhase::WaitingForPlayers,
            players: BTreeMap::default(),
            num_seats,
        }
    }

    pub fn fold(&mut self, id: &PeerId,) {
        if let Some(player,) = self.players.get_mut(id,) {
            player.fold();
        }
    }

    pub fn activate_next_player(&mut self,) {
        if self.players.is_empty() {
            return;
        }

        // Get sorted list of player IDs (deterministic order)
        let mut player_ids: Vec<PeerId,> =
            self.players.keys().copied().collect();
        player_ids.sort_by_key(std::string::ToString::to_string,);

        let len = player_ids.len();

        // Find current active player's index
        let current_idx = self
            .players
            .values()
            .find(|p| p.is_active && p.chips > Chips::ZERO,)
            .map_or(0, |p| {
                player_ids
                    .iter()
                    .position(|id| *id == p.peer_id,)
                    .unwrap_or(0,)
            },);

        // Cycle from next index
        for offset in 1..=len {
            let next_idx = (current_idx + offset) % len;
            let next_id = player_ids[next_idx];
            if let Some(player,) = self.players.get_mut(&next_id,) {
                if player.is_active && player.chips > Chips::ZERO {
                    // Set as active (assuming active_player_id or similar;
                    // adjust if needed).
                    // If no active_player_id field, track separately or mark in
                    // PlayerPrivate
                    break;
                }
            }
        }
    }

    pub fn fold_inactive_players(&mut self,) {
        if let Some(p,) = self
            .players
            .values_mut()
            .find(|p| p.action_timer.is_some(),)
        {
            if p.action_timer.unwrap_or(0,) >= 1_500 {
                p.fold();
            }
        }
    }

    pub fn set_has_sent_start_game_notification(&mut self, id: &PeerId,) {
        // Update local player's flag if applicable
        if let Some(p,) = self.players.get_mut(id,) {
            p.has_sent_start_game_notification = true;
        }
    }

    pub fn place_bet(
        &mut self,
        id: &PeerId,
        total_bet: Chips,
        action: PlayerAction,
    ) {
        if let Some(player,) = self.players.get_mut(id,) {
            player.place_bet(total_bet, action,);
        }
    }

    pub fn start_hand(&mut self,) {
        for player in self.players.values_mut() {
            player.start_hand();
        }
    }

    pub fn end_hand(&mut self,) {
        for player in self.players.values_mut() {
            player.finalize_hand();
        }
    }

    pub fn remove_with_no_chips(&mut self,) {
        self.players.retain(|_, p| p.chips > Chips::ZERO,);
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
                    batch_senders_len: {batch_senders_sorted:#?},\n\
                    expected_senders_len: {expected_senders_sorted:#?}"
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
            out.push(eff,);
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

            if st.players.len() >= st.num_seats {
                st.phase = HandPhase::StartingGame;
            }
        },
        WireMsg::DealCardsBatch(batch,) => {
            // Verify: Complete, sorted, valid for active players
            let expected_receivers: Vec<PeerId,> = st
                .players
                .values()
                .filter(|p| p.is_active && p.chips > Chips::ZERO,)
                .map(|p| p.peer_id,)
                .collect();
            let mut batch_receivers_sorted: Vec<PeerId,> =
                batch.iter().map(|dc| dc.player_id,).collect();
            batch_receivers_sorted
                .sort_by_key(std::string::ToString::to_string,);
            let mut expected_receivers_sorted = expected_receivers.clone();
            expected_receivers_sorted
                .sort_by_key(std::string::ToString::to_string,);
            if batch_receivers_sorted != expected_receivers_sorted
                || batch.len() != expected_receivers.len()
            {
                info!("Invalid DealCardsBatch; rejecting");
                return StepResult {
                    next:    prev.clone(),
                    effects: vec![],
                };
            }
            // Apply: Update hole_cards for each
            for dc in batch {
                if let Some(player,) = st.players.get_mut(&dc.player_id,) {
                    player.hole_cards = Cards(dc.card1, dc.card2,);
                }
            }
            // Add to effects if needed (e.g., broadcast)
            let eff = Effect::Send(msg.clone(),);
            out.push(eff,);
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

/// # Panics
/// panics if the bytes are not as expected
pub fn hash_state(st: &ContractState,) -> Hash {
    let bytes = bincode::serialize(st,).expect("bincode serialization failed",);
    let hash = blake3::hash(bytes.as_slice(),);
    Hash(hash,)
}

/// One player as stored in *our* local copy of the table state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq,)]
pub struct PlayerPrivate {
    pub peer_id:                          PeerId,
    pub nickname:                         String,
    pub chips:                            Chips,
    pub bet:                              Chips,
    pub payoff:                           Option<HandPayoff,>,
    pub action:                           PlayerAction,
    pub action_timer:                     Option<u64,>,
    pub hole_cards:                       PlayerCards,
    pub public_cards:                     PlayerCards,
    pub has_button:                       bool,
    pub is_active:                        bool,
    pub has_sent_start_game_notification: bool,
    pub seat_idx:                         Option<u8,>,
}

impl PlayerPrivate {
    pub fn start_hand(&mut self,) {
        self.is_active = self.chips > Chips::ZERO;
        self.has_button = false;
        self.bet = Chips::ZERO;
        self.action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
        self.hole_cards = PlayerCards::None;
    }
}

impl PlayerPrivate {
    pub const fn finalize_hand(&mut self,) {
        self.action = PlayerAction::None;
        self.action_timer = None;
    }

    #[must_use]
    pub fn id_digits(&self,) -> String {
        self.peer_id.to_string()
    }

    pub const fn sent_start_game_notification(&mut self,) {
        self.has_sent_start_game_notification = true;
    }

    #[must_use]
    pub const fn has_sent_start_game_notification(&self,) -> bool {
        self.has_sent_start_game_notification
    }
}

impl PlayerPrivate {
    pub fn reset_for_new_hand(&mut self,) {
        self.is_active = self.chips > Chips::ZERO;
        self.has_button = false;
        self.bet = Chips::ZERO;
        self.action = PlayerAction::None;
        self.hole_cards = PlayerCards::None;
        self.public_cards = PlayerCards::None;
    }
}

impl PlayerPrivate {
    #[must_use]
    pub const fn new(id: PeerId, nickname: String, chips: Chips,) -> Self {
        Self {
            has_sent_start_game_notification: false,
            peer_id: id,
            nickname,
            chips,
            bet: Chips::default(),
            payoff: None,
            action: PlayerAction::None,
            action_timer: None,
            hole_cards: PlayerCards::None,
            public_cards: PlayerCards::None,
            has_button: false,
            is_active: true,
            seat_idx: None,
        }
    }

    pub const fn fold(&mut self,) {
        self.is_active = false;
        self.action = PlayerAction::Fold;
    }

    pub fn place_bet(&mut self, total_bet: Chips, action: PlayerAction,) {
        let required = total_bet - self.bet;
        let actual_bet = required.min(self.chips,);

        self.chips -= actual_bet;
        self.bet += actual_bet;
        self.action = action;
    }

    #[must_use]
    pub fn has_chips(&self,) -> bool {
        self.chips > Chips::ZERO
    }
}
