//! Poker table state management – **lock-free** client version
//! Adapted from <https://github.com/vincev/freezeout>

use std::fmt;
use std::fmt::Formatter;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use libp2p::Multiaddr;
use log::{error, info, warn};
use poker_eval::HandValue;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::connection_stats::ConnectionStats;
use crate::crypto::{KeyPair, PeerId, SecretKey};
use crate::message::{
    HandPayoff, NetworkMessage, PlayerAction, PlayerUpdate, SignedMessage,
    UIEvent,
};
use crate::net::traits::P2pTransport;
use crate::poker::{Card, Chips, Deck, GameId, PlayerCards, TableId};
use crate::protocol::msg::{DealCards, Hash, LogEntry, Transition};
pub use crate::protocol::state::{
    self as contract, ContractState, GENESIS_HASH, HandPhase, PeerContext,
    PlayerPrivate,
};

#[allow(missing_docs)]
#[derive(Debug, Default, Clone, Serialize, Deserialize,)]
pub struct Pot {
    participants: AHashSet<PeerId,>,
    total_chips:  Chips,
}

impl Pot {
    #[must_use]
    pub const fn total_chips(&self,) -> Chips {
        self.total_chips
    }
}

impl fmt::Display for Pot {
    fn fmt(&self, f: &mut Formatter<'_,>,) -> fmt::Result {
        write!(f, "{}", self.total_chips)
    }
}

/// A *derived*, mutable view of the canonical contract state,
/// enriched with peer–local convenience data.
///
/// Most of it is what you already have in `InternalTableState`.
pub struct Projection {
    // 1. Canonical data – always equals contract after replay
    pub table_id:                     TableId,
    pub game_id:                      GameId,
    pub board:                        Vec<Card,>,
    pub small_blind:                  Chips,
    pub big_blind:                    Chips,
    num_seats:                        usize,
    key_pair:                         KeyPair,
    /// whether player associated with `key_pair` has joined a table.
    has_joined_table:                 bool,
    is_seed_peer:                     bool,
    is_synced:                        bool,
    // 2. Peer-local “soft” state – NOT part of consensus
    pub rng:                          StdRng, // used only when *we* deal
    pub listen_addr:                  Option<Multiaddr,>,
    pub connection_info:              ConnectionStats, /* bytes/sec graphs,
                                                        * peer RTT … */
    // networking ----------------------------------------------------------
    pub connection:                   P2pTransport,
    // Outbound messages that came back from the pure state machine and still
    /// need to be broadcast.
    pending_effects:                  Vec<Transition,>,
    // game state ----------------------------------------------------------
    deck:                             Deck,
    current_pot:                      Pot,
    action_request:                   Option<ActionRequest,>,
    active_player_id:                 Option<PeerId,>,
    last_bet:                         Chips,
    hand_count:                       usize,
    min_raise:                        Chips,
    pub game_started:                 bool,
    hash_chain:                       Vec<LogEntry,>,
    has_sent_start_game_notification: bool,
    has_sent_start_game_batch:        bool,
    // misc ----------------------------------------------------------------
    new_hand_start_timer:             Option<Instant,>,
    new_hand_timeout:                 Duration,
    // crypto --------------------------------------------------------------
    contract:                         ContractState,
    hash_head:                        Hash,
    // peer context
    peer_context:                     PeerContext,
    start_game_message_buffer:        Vec<SignedMessage,>, /* Buffer for
                                                            * plain
                                                            * StartGameNotify
                                                            * messages */
    start_game_timeout:               Option<Instant,>, /* Timer for buffering phase */
    start_game_proposer:              Option<PeerId,>,  // Cached proposer ID
}

impl Projection {

    #[must_use]
    pub fn get_last_bet(&self) -> Chips {
        self.last_bet.clone()
    }

    pub fn get_game_id(&self) -> GameId {
        self.game_id.clone()
    }

    pub fn get_table_id(&self) -> TableId {
        self.table_id.clone()
    }
    #[must_use]
    pub const fn game_started(&self,) -> bool {
        self.game_started
    }

    #[must_use]
    pub fn hash_head(&self,) -> Hash {
        self.hash_head.clone()
    }

    #[must_use]
    pub const fn has_joined_table(&self,) -> bool {
        self.has_joined_table
    }

    #[must_use]
    pub fn hash_chain(&self,) -> Vec<LogEntry,> {
        self.hash_chain.clone()
    }

    // Derived players access
    #[must_use]
    pub fn get_players(&self,) -> Vec<PlayerPrivate,> {
        self.contract.get_players().values().cloned().collect()
    }
    pub fn get_players_mut(
        &mut self,
    ) -> Vec<&mut PlayerPrivate> {
        self.contract.get_players_mut()
    }

    pub fn get_player_mut(
        &mut self,
        id: &PeerId,
    ) -> Option<&mut PlayerPrivate> {
        self.contract.get_player_mut(id)
    }

    #[must_use]
    pub fn get_player(&mut self, id: &PeerId,) -> Option<&PlayerPrivate,> {
        self.contract.get_player(id)
    }

    // Example derived methods for counts and operations
    #[must_use]
    pub fn count_active(&self,) -> usize {
        self.get_players()
            .iter()
            .filter(|p| p.is_active,)
            .count()
    }

    #[must_use]
    pub fn count_with_chips(&self,) -> usize {
        self
            .get_players()
            .iter()
            .filter(|p| p.has_chips(),)
            .count()
    }

    #[must_use]
    pub fn count_active_with_chips(&self,) -> usize {
        self
            .get_players()
            .iter()
            .filter(|p| p.is_active && p.has_chips(),)
            .count()
    }
}

impl Projection {
    const START_GAME_SB: Chips = Chips::ZERO;
    const START_GAME_BB: Chips = Chips::ZERO;
    const ACTION_TIMEOUT_MILLIS: u64 = 1500;

    /// grab an immutable snapshot for the GUI
    #[must_use]
    pub fn snapshot(&mut self,) -> GameState {
        // Cycle the vector until the local peer is at index 0
        let mut players = self.get_players();
        if let Some(local_idx) = players.iter().position(|p| p.peer_id == self.peer_id()) {
            players.rotate_left(local_idx);
        };
        
        GameState {
            has_sent_start_game_notification: self
                .has_sent_start_game_notification,
            hash_chain:                       self.hash_chain.clone(),
            is_seed_peer:                     self.has_joined_table,
            key_pair:                         self.key_pair.clone(),
            hash_head:                        self.hash_head.clone(),
            has_joined_table:                 self.has_joined_table,
            table_id:                         self.table_id,
            seats:                            self.num_seats,
            game_started:                     !matches!(
                self.contract.get_phase(),
                HandPhase::WaitingForPlayers
            ),
            player_id:                        self.key_pair.clone().peer_id(),
            nickname:                         self.peer_context.nick.clone(),
            players, 
            board:                            self.board.clone(),
            pot:                              self.current_pot.clone(),
            action_req:                       self.action_request.clone(),
            hand_phase:                       self.contract.get_phase(),
            listen_addr:                      self.listen_addr.clone(),
        }
    }

    #[must_use]
    pub fn secret_key(&self,) -> SecretKey {
        self.key_pair.secret()
    }

    #[must_use]
    pub fn peer_id(&self,) -> PeerId {
        self.key_pair.clone().peer_id()
    }

    #[must_use]
    pub const fn num_seats(&self,) -> usize {
        self.num_seats
    }

    #[must_use]
    pub fn board(&self,) -> Vec<Card,> {
        self.board.clone()
    }

    #[must_use]
    pub fn pot(&self,) -> Pot {
        self.current_pot.clone()
    }

    #[must_use]
    pub const fn table_id(&self,) -> TableId {
        self.table_id
    }

    // — constructors —
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        nick: String,
        table_id: TableId,
        num_seats: usize,
        key_pair: KeyPair,
        connection: P2pTransport,
        is_seed_peer: bool,
    ) -> Self {
        let obj = Self {
            has_sent_start_game_batch: false,
            start_game_message_buffer: Vec::new(),
            start_game_timeout: None,
            start_game_proposer: None,
            has_sent_start_game_notification: false,
            is_synced: is_seed_peer,
            is_seed_peer,
            peer_context: PeerContext::new(
                key_pair.peer_id(),
                nick,
                Chips::default(),
            ),
            pending_effects: Vec::new(),
            connection_info: ConnectionStats::default(),
            has_joined_table: false,
            contract: ContractState::new(num_seats,),
            hash_head: GENESIS_HASH.clone(),
            game_started: false,
            hand_count: 0,
            min_raise: Chips::ZERO,
            game_id: GameId::new_id(),
            last_bet: Chips::ZERO,
            active_player_id: None,
            table_id,
            num_seats,
            key_pair,
            connection,
            deck: Deck::default(),
            board: Vec::new(),
            current_pot: Pot::default(),
            small_blind: Chips::default(),
            big_blind: Chips::default(),
            rng: StdRng::from_os_rng(),
            new_hand_start_timer: None,
            new_hand_timeout: Duration::from_millis(1_000,),
            action_request: None,
            listen_addr: None,
            hash_chain: Vec::new(),
        };
        info!("peer {} has nickname {}", obj.peer_id(), obj.peer_context.nick);
        obj
    }

    pub fn update(&mut self,) {
        if self.contract.get_phase() == HandPhase::StartingGame {
            self.enter_start_game(100, 50,);
        } else if self.contract.get_phase() == HandPhase::StartingHand {
            self.enter_start_hand();
        }
    }

    pub fn try_recv(
        &mut self,
    ) -> Result<SignedMessage, mpsc::error::TryRecvError,> {
        self.connection.rx.network_msg_receiver.try_recv()
    }

    /// Low-level send used by `sign_and_send` **and** by the gossip handler.
    pub fn send(&mut self, msg: SignedMessage,) -> anyhow::Result<(),> {
        // engine → network
        self.connection.tx.network_msg_sender.try_send(msg,)?;
        // also send the same msg to ourselves
        // TODO send callbacks to ourselves.
        Ok((),)
    }

    /// Called every ~20 ms by the runtime.
    pub fn tick(&mut self,) {
        // NEW ───────────────────────────────────────────────────────────
        // broadcast all WireMsg side‑effects that the *pure* contract
        // returned in previous steps. Each message is appended to the log
        // via `sign_and_send`, so ordering & hashing stay consistent.
        let mut pending = self.pending_effects.clone();
        while let Some(eff,) = pending.pop() {
            // ignore errors – the network layer already logs them
            let _ = self.sign_and_send(eff,);
        }
        self.pending_effects = pending;
        // ───────────────────────────────────────────────────────────────
        // existing fold‑timeout logic
        self.contract.fold_inactive_players();
        if self.phase() == HandPhase::StartingGame
            && !self.has_sent_start_game_batch
        {
            let expected_count = self.get_players().len();
            if self.start_game_message_buffer.len() >= expected_count {
                // All received: sort locally
                let mut sorted_buffer = self.start_game_message_buffer.clone();
                sorted_buffer.sort_by_key(|sm| sm.sender().to_string(),);
                if self.peer_id() == self.start_game_proposer.unwrap() {
                    // Proposer: Append batch to chain
                    let batch_msg = Transition::StartGameBatch(sorted_buffer,);
                    if self.commit_step(&batch_msg,).is_ok() {
                        info!(
                            "Proposer ({}) appended StartGameBatch",
                            self.peer_id()
                        );
                        self.has_sent_start_game_batch = true;
                    }
                } else {
                    self.start_game_message_buffer.clear();
                }
            }
        }
    }

    // ────────────────────────────────────────────────────────────────────────────
    // inside `impl Projection { … }` ‑ add to `handle_network_msg`
    // ────────────────────────────────────────────────────────────────────────────
    pub fn handle_network_msg(&mut self, sm: SignedMessage,) {
        match sm.message() {
            NetworkMessage::StartGameNotify {
                seat_order: _seat_order,
                sb: _sb,
                bb: _bb,
                table: _table,
                game_id: _game_id,
                sender,
            } => {
                if self.phase() != HandPhase::StartingGame {
                    return;
                }
                // Verify signature and not duplicate
                if !self
                    .start_game_message_buffer
                    .iter()
                    .any(|b| b.sender() == sm.sender(),)
                {
                    self.start_game_message_buffer.push(sm.clone(),);
                }
                self.contract.set_has_sent_start_game_notification(sender,);
            },
            NetworkMessage::ProtocolEntry(entry,) => {
                // 1. reject genuinely invalid branches
                if entry.prev_hash != self.hash_head {
                    warn!(
                        "{} (nick {}) received message ProtocolEntry: {} from {} with hash mismatch – discarding",
                        self.peer_context.id,
                        self.peer_context.nick.clone(),
                        entry.payload.label(),
                        sm.sender()
                    );
                    return;
                }
                // 2. reject any messages before we have synced our state with
                // the seed peer.
                if !self.is_synced {
                    return;
                }
                // 2. **first** commit the message we just received
                let _ = self.commit_step(&entry.payload,);
            },
            NetworkMessage::NewListenAddr { multiaddr, .. } => {
                self.listen_addr = Some(multiaddr.clone(),);
            },
            NetworkMessage::SyncReq {
                table,
                player_asking_for_sync: player_id,
                nickname,
                chips,
            } => {
                if self.table_id != *table {
                    return; // wrong table.
                }
                if self.get_player(player_id,).is_some() {
                    return; // Already joined.
                }
                if self.contract.get_players().len() >= self.num_seats {
                    return; // Table full
                }
                if self.game_started {
                    return; // Game started
                }
                if self.peer_id() == *player_id {
                    return; // do not handle own sync request.
                }
                if !self.is_seed_peer {
                    return; // only seed peer should answer sync requests.
                }
                info!(
                    "{} did not throw away sync request",
                    self.peer_context.nick.clone()
                );
                // Append JoinTableReq to chain
                let join_msg = Transition::JoinTableReq {
                    table:     *table,
                    player_id: *player_id,
                    nickname:  nickname.clone(),
                    chips:     *chips,
                };
                let _ = self.commit_step(&join_msg,);
                // Send full chain (now includes join) to new player
                let resp = NetworkMessage::SyncResp {
                    player_asking_for_sync: *player_id,
                    chain:                  self.hash_chain.clone(),
                };
                let _ = self.send_plain(resp,);
            },
            NetworkMessage::SyncResp {
                player_asking_for_sync: target,
                chain,
            } => {
                if *target != self.peer_id() {
                    return; // do not apply syncResponses not addressed to us.
                }
                if self.is_seed_peer {
                    return; // do not apply any syncResponses if we are the seed peer.
                }
                // Validate and replay chain from the very beginning.
                let mut current_state = ContractState::new(self.num_seats.clone());
                let mut current_hash = GENESIS_HASH.clone();
                let received_chain = chain.iter().cloned();
                // first check the validity of the chain before applying any
                // entries contained in it.
                for entry in received_chain.clone() {
                    if entry.prev_hash != current_hash {
                        warn!(
                            "{} (nick {}) received sync response from {} with invalid chain - prev_hash mismatch",
                            self.peer_context.id,
                            self.peer_context.nick.clone(),
                            entry.metadata.author
                        );
                        return;
                    }
                    // set our current hash one hash forward.
                    current_hash = entry.hash;
                }
                // apply logentries of chain to our state only if all hashes in
                // hash state chain were valid.
                for entry in received_chain.clone() {
                    let res =
                        contract::step(&mut current_state, &entry.payload,);
                    let next = res.next;
                    let next_hash = contract::hash_state(&next,);
                    if next_hash != entry.hash {
                        warn!(
                            "{} (nick {}) received sync response with invalid chain - next_hash mismatch",
                            self.peer_context.id,
                            self.peer_context.nick.clone()
                        );
                        return;
                    }
                    // Ignore effects during replay
                    current_state = next;
                    current_hash = next_hash;
                }
                self.contract = current_state;
                self.hash_head = current_hash;
                self.hash_chain = chain.clone();
                self.is_synced = true;
                self.has_joined_table = true;
                info!("Chain synced successfully; joined table");
            },
            NetworkMessage::Dummy => {} //ignore
        }
    }

    // ---------------------------------------------------------------------------
    // 2. Commit step: move “canonical state commit” above the side‑effect queue
    // (so that a subsequent self.sign_and_send() sees the new head)
    // ---------------------------------------------------------------------------
    fn commit_step(&mut self, payload: &Transition,) -> anyhow::Result<(),> {
        use contract::{Effect, StepResult};
        // 1. pure state transition
        let StepResult { next, effects, } =
            contract::step(&self.contract, payload,);
        // 3. compute next hash & build entry with deterministic ordering key
        let next_hash = contract::hash_state(&next,);
        let entry = LogEntry::with_key(
            self.hash_head.clone(),
            payload.clone(),
            next_hash.clone(),
            self.peer_id(),
        );
        // 4. broadcast (+ loop-back)
        let signed = SignedMessage::new(
            &self.key_pair,
            NetworkMessage::ProtocolEntry(entry.clone(),),
        );
        self.send(signed,)?;
        // 5. commit canonical state *before* enqueueing side-effects
        self.contract = next;
        self.hash_head = next_hash;
        self.hash_chain.push(entry,);
        // 6. queue side-effects for the next tick
        self.pending_effects
            .extend(effects.into_iter().filter_map(|e| {
                match e {
                    Effect::Send(m,) => Some(m,),
                }
            },),);

        Ok((),)
    }

    pub fn handle_ui_msg(&mut self, msg: UIEvent,) {
        match msg {
            UIEvent::PlayerJoinTableRequest {
                table_id,
                player_requesting_join: peer_id,
                nickname,
                chips,
            } => {
                if table_id == self.table_id
                    && peer_id == self.peer_id()
                    && !self.has_joined_table
                    && !self.game_started
                    && self.get_player(&peer_id,).is_none()
                    && self.num_seats > self.get_players().len()
                {
                    let wire_msg = Transition::JoinTableReq {
                        table: table_id,
                        player_id: peer_id,
                        nickname: nickname.clone(),
                        chips,
                    };
                    if !self.is_seed_peer
                        && self.hash_head == GENESIS_HASH.clone()
                    {
                        // New non-seed player: request sync first
                        let sync_msg = NetworkMessage::SyncReq {
                            table: table_id,
                            player_asking_for_sync: peer_id,
                            nickname,
                            chips,
                        };
                        let _ = self.send_plain(sync_msg,);
                    } else {
                        let _ = self.commit_step(&wire_msg,);
                        self.has_joined_table = true;
                    }
                }
            },
            _ => {
                error!("handling of ui msg {msg} not implemented yet");
            },
        }
    }

    #[must_use]
    pub fn action_request(&self,) -> Option<ActionRequest,> {
        self.action_request.clone()
    }

    pub fn reset_action_request(&mut self,) {
        self.action_request = None;
    }

    fn update_players(&mut self, updates: &[PlayerUpdate],) {
        for update in updates {
            let id = self.peer_id();
            if let Some(player,) = self
                .get_players_mut().iter_mut()
                .find(|p| p.peer_id == update.player_id,)
            {
                player.chips = update.chips;
                player.bet = update.bet;
                player.action = update.action;
                player.action_timer = update.action_timer;
                player.has_button = update.is_dealer;
                player.is_active = update.is_active;

                // Do not override cards for the local player as they are
                // updated when we get a DealCards message.
                if player.peer_id != id {
                    player.hole_cards = update.hole_cards;
                }

                // If local player has folded remove its cards.
                if player.peer_id == id && !player.is_active {
                    player.public_cards = PlayerCards::None;
                    player.hole_cards = PlayerCards::None;
                    self.action_request = None;
                }
            }
        }
    }
}

/// A player action request from the server.
#[derive(Debug, Clone, Serialize, Deserialize,)]
pub struct ActionRequest {
    /// The actions choices requested by server.
    pub actions:   Vec<PlayerAction,>,
    /// The action minimum raise
    pub min_raise: Chips,
    /// The hand big blind.
    pub big_blind: Chips,
}

impl ActionRequest {
    /// Check if a call action is in the request.
    #[must_use]
    pub fn can_call(&self,) -> bool {
        self.can_do_action(PlayerAction::Call,)
    }

    /// Check if a check action is in the request.
    #[must_use]
    pub fn can_check(&self,) -> bool {
        self.can_do_action(PlayerAction::Check,)
    }

    /// Check if a bet action is in the request.
    #[must_use]
    pub fn can_bet(&self, amount: Chips,) -> bool {
        self.can_do_action(PlayerAction::Bet {
            bet_amount: amount,
        },)
    }

    /// Check if a raise action is in the request.
    #[must_use]
    pub fn can_raise(&self, amount: Chips,) -> bool {
        self.can_do_action(PlayerAction::Raise {
            bet_amount: amount,
        },)
    }

    fn can_do_action(&self, action: PlayerAction,) -> bool {
        self.actions.iter().any(|a| a == &action,)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Errors (user-visible)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Error, Debug,)]
pub enum TableJoinError {
    #[error("game already started")]
    GameStarted,
    #[error("table is full")]
    TableFull,
    #[error("player already joined")]
    AlreadyJoined,
    #[error("unknown join error")]
    Unknown,
}

/// The **immutable** snapshot handed to the GUI every frame.
#[derive(Clone, Serialize, Deserialize,)]
pub struct GameState {
    pub has_sent_start_game_notification: bool,
    pub table_id:                         TableId,
    pub seats:                            usize,
    pub game_started:                     bool,
    pub player_id:                        PeerId, // local player
    pub nickname:                         String,
    /// player with `player_id` has joined table
    pub has_joined_table:                 bool,
    pub is_seed_peer:                     bool,
    pub players:                          Vec<PlayerPrivate,>,
    pub board:                            Vec<Card,>,
    pub pot:                              Pot,
    pub action_req:                       Option<ActionRequest,>,
    pub hand_phase:                       HandPhase,
    pub listen_addr:                      Option<Multiaddr,>,
    // -- crypto
    pub hash_head:                        Hash,
    pub hash_chain:                       Vec<LogEntry,>,
    pub key_pair:                         KeyPair,
}

impl PartialEq for GameState {
    fn eq(&self, other: &Self,) -> bool {
        let self_bytes = bincode::serialize(&self).unwrap();
        let self_hash = blake3::hash(self_bytes.as_slice());
        let other_bytes = bincode::serialize(&other).unwrap();
        let other_hash = blake3::hash(other_bytes.as_slice());
        self_hash == other_hash
    }
}

impl GameState {
    #[must_use]
    pub fn new(
        peer_id: PeerId,
        table_id: TableId,
        nickname: String,
        seats: usize,
    ) -> Self {
        Self {
            has_sent_start_game_notification: false,
            hash_chain: Vec::default(),
            is_seed_peer: false,
            key_pair: KeyPair::generate(),
            hash_head: GENESIS_HASH.clone(),
            table_id,
            seats,
            game_started: false,
            player_id: peer_id,
            nickname,
            players: Vec::new(),
            board: Vec::new(),
            pot: Pot::default(),
            action_req: None,
            hand_phase: HandPhase::WaitingForPlayers,
            listen_addr: None,
            has_joined_table: false,
        }
    }

    #[must_use]
    pub fn default() -> Self {
        Self {
            has_sent_start_game_notification: false,
            hash_chain:                       Vec::default(),
            is_seed_peer:                     false,
            key_pair:                         KeyPair::generate(),
            hash_head:                        GENESIS_HASH.clone(),
            has_joined_table:                 false,
            table_id:                         TableId::new_id(),
            seats:                            0,
            game_started:                     false,
            player_id:                        PeerId::default(),
            players:                          Vec::new(),
            nickname:                         String::default(),
            board:                            Vec::new(),
            pot:                              Pot::default(),
            action_req:                       None,
            hand_phase:                       HandPhase::WaitingForPlayers,
            listen_addr:                      None,
        }
    }

    #[must_use]
    pub fn action_req(&self,) -> Option<ActionRequest,> {
        self.action_req.clone()
    }

    #[must_use]
    pub const fn has_joined_table(&self,) -> bool {
        self.has_joined_table
    }

    #[must_use]
    pub fn phase(&self,) -> HandPhase {
        self.hand_phase.clone()
    }

    #[must_use]
    pub fn hash_head(&self,) -> Hash {
        self.hash_head.clone()
    }

    #[must_use]
    pub fn hash_chain(&self,) -> Vec<LogEntry,> {
        self.hash_chain.clone()
    }

    pub fn pot(&mut self,) -> Pot {
        self.pot.clone()
    }

    pub const fn pot_chips(&mut self,) -> Chips {
        self.pot.total_chips
    }

    #[must_use]
    pub fn get_players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone()
    }

    #[must_use]
    pub const fn player_id(&self,) -> PeerId {
        self.player_id
    }
}

impl Projection {
    // send a protocol entry (hash-chained)
    fn send_contract(&mut self, payload: Transition,) -> anyhow::Result<(),> {
        self.commit_step(&payload,)
    }

    // send messages that are not appended to the log entry list.
    fn send_plain(
        &mut self,
        msg: NetworkMessage,
    ) -> (SignedMessage, anyhow::Result<(),>,) {
        let sm = SignedMessage::new(&self.key_pair, msg,);
        (sm.clone(), self.send(sm,),)
    }

    // convenience wrappers ------------------------------------------
    pub fn sign_and_send(&mut self, payload: Transition,) -> anyhow::Result<(),> {
        self.send_contract(payload,)
    }

    #[must_use]
    pub fn phase(&self,) -> HandPhase {
        self.contract.get_phase()
    }

    fn enter_start_game(&mut self, timeout_s: u64, max_retries: u32,) {
        if self.game_started {
            return;
        }
        self.game_started = true;

        // Compute deterministic proposer: lowest peer_id in sorted player list
        let mut player_ids: Vec<PeerId,> =
            self.get_players().iter().map(|p| p.peer_id,).collect();
        player_ids.sort_by_key(std::string::ToString::to_string,); // Lexicographical sort
        self.start_game_proposer = Some(player_ids[0],); // Lowest is proposer

        // Send our own StartGameNotify as plain signed message
        let seats = player_ids.clone(); // Use sorted for consistency if needed
        let notify = NetworkMessage::StartGameNotify {
            seat_order: seats,
            table:      self.table_id,
            game_id:    self.game_id,
            sb:         Self::START_GAME_SB,
            bb:         Self::START_GAME_BB,
            sender:     self.peer_id(),
        };
        let (sm, result,) = self.send_plain(notify,);
        if matches!(result, Ok(()))
            && !self
                .start_game_message_buffer
                .iter()
                .any(|b| b.sender() == sm.sender(),)
        {
            self.start_game_message_buffer.push(sm,);
        }

        // Set our local flag (for UI/progress)
        self.contract
            .set_has_sent_start_game_notification(&self.peer_id(),);
        self.has_sent_start_game_notification = true;

        // Start buffering timeout
        self.start_game_timeout = Some(
            Instant::now()
                + Duration::from_secs(timeout_s * u64::from(max_retries,),),
        );
    }

    fn enter_start_hand(&mut self,) {
        info!("enter start hand...");
        if self.count_active() < 2 {
            info!("exiting starting hand, less than 2 active players");
            self.enter_end_game();
            return;
        }
        self.update_blinds();

        // Pay small and big blind (using active_player_id).
        if let Some(id,) = self.active_player_id {
            self.contract.place_bet(
                &id,
                self.small_blind,
                PlayerAction::SmallBlind,
            );
        }
        if let Some(id,) = self.active_player_id {
            self.contract.place_bet(
                &id,
                self.big_blind,
                PlayerAction::BigBlind,
            );
        }
        self.last_bet = self.big_blind;
        self.min_raise = self.big_blind;

        // Create a new deck.
        self.deck = Deck::shuffled(&mut self.rng,);

        // Clear board.
        self.board.clear();

        let active_ids = self
            .get_players()
            .iter()
            .filter(|p| p.is_active,)
            .map(|p| p.peer_id,)
            .collect::<Vec<_,>>();

        for player in self.get_players_mut() {
            if !active_ids.contains(&player.peer_id,) {
                player.hole_cards = PlayerCards::None;
                player.public_cards = PlayerCards::None;
            }
        }

        // Collect DealCards for all active players
        let mut deal_cards_list: Vec<DealCards,> = Vec::new();
        for &id in &active_ids {
            let c1 = self.deck.deal();
            let c2 = self.deck.deal();
            if let Some(player,) = self.get_player_mut(&id)
            {
                player.public_cards = PlayerCards::Covered;
                player.hole_cards = if c1.rank() < c2.rank() {
                    PlayerCards::Cards(c1, c2,)
                } else {
                    PlayerCards::Cards(c2, c1,)
                };
            }
            deal_cards_list.push(DealCards {
                card1:     c1,
                card2:     c2,
                player_id: id,
                table:     self.table_id,
                game_id:   self.game_id,
            },);
        }

        // Sort by player_id for deterministic order
        deal_cards_list.sort_by_key(|dc| dc.player_id,);

        // Append batch to chain
        let batch_msg = Transition::DealCardsBatch(deal_cards_list,);
        let _ = self.send_contract(batch_msg,);

        self.enter_preflop_betting();
    }

    fn enter_preflop_betting(&mut self,) {
        self.contract.set_phase(HandPhase::PreflopBetting,);
        self.action_update();
    }

    fn action_update(&mut self,) {
        self.contract.activate_next_player();
        if self.is_round_complete() {
            self.next_round();
        } else {
            let () = self.request_action();
        }
    }

    fn enter_deal_flop(&mut self,) {
        for _ in 1..=3 {
            self.board.push(self.deck.deal(),);
        }
        self.contract.set_phase(HandPhase::FlopBetting,);
        self.start_round();
    }

    fn enter_deal_turn(&mut self,) {
        self.board.push(self.deck.deal(),);
        self.contract.set_phase(HandPhase::TurnBetting,);
        self.start_round();
    }

    fn enter_deal_river(&mut self,) {
        self.board.push(self.deck.deal(),);
        self.contract.set_phase(HandPhase::RiverBetting,);
        self.start_round();
    }

    fn enter_showdown(&mut self,) {
        self.contract.set_phase(HandPhase::Showdown,);
        for player in self.get_players_mut() {
            player.action = PlayerAction::None;
            if player.is_active {
                player.public_cards = player.hole_cards;
            }
        }
        self.enter_end_hand();
    }

    fn enter_end_hand(&mut self,) {
        self.new_hand_timeout = if matches!(self.phase(), HandPhase::Showdown) {
            // If coming from a showdown give players more time to see the
            // winning hand and chips.
            Duration::from_millis(7_000,)
        } else {
            Duration::from_millis(3_000,)
        };
        self.contract.set_phase(HandPhase::EndingHand,);
        self.update_pots();
        // Give time to the UI to look at the updated pot and board.
        let () = self.send_throttle(100,);
        let winners = self.pay_bets();
        // Update players and broadcast update to all players.
        self.contract.end_hand();
        let _ = self.send_contract(Transition::EndHand {
            payoffs: winners,
            pot:     self.pot().total_chips,
        },);
        // End game if only player has chips or move to next hand.
        if self.count_with_chips() < 2 {
            self.enter_end_game();
        } else {
            // All players that run out of chips must leave the table before the
            // start of a new hand.
            let players = self.get_players();
            for player in players {
                // Clone to avoid borrow issues
                if player.chips == Chips::ZERO {
                    // Notify the client that this player has left the table.
                    let _ = self.send_contract(Transition::LeaveTable {
                        player_id: player.peer_id,
                    },);
                }
            }
            self.contract.remove_with_no_chips();
            self.new_hand_start_timer = Some(Instant::now(),);
        }
    }

    fn enter_end_game(&mut self,) {
        // Give time to the UI to look at winning results before ending the
        // game.
        self.broadcast_throttle(4_500,);
        self.contract.set_phase(HandPhase::EndingGame,);
        for player in self.get_players() {
            // Notify the client that this player has left the table.
            let msg = Transition::LeaveTable {
                player_id: player.peer_id,
            };
            let _ = self.send_contract(msg,);
        }
        self.contract.clear_players();
        // Reset hand count for next game.
        self.hand_count = 0;
        // Wait for players to join.
        self.contract.set_phase(HandPhase::EndingGame,);
    }

    fn update_blinds(&mut self,) {
        let multiplier = (1 << (self.hand_count / 4).min(4,)) as u32;
        if multiplier < 16 {
            self.small_blind = Self::START_GAME_SB * multiplier;
            self.big_blind = Self::START_GAME_BB * multiplier;
        } else {
            // Cap at 12 times initial blinds.
            self.small_blind = Self::START_GAME_SB * 12;
            self.big_blind = Self::START_GAME_BB * 12;
        }
        self.hand_count += 1;
    }

    fn pay_bets(&mut self,) -> Vec<HandPayoff,> {
        let mut payoffs = Vec::<HandPayoff,>::new();
        let active_count = self.count_active();
        if active_count == 1 {
            self.handle_single_player_payoff(&mut payoffs,);
        } else if active_count > 1 {
            self.handle_multi_player_payoff(&mut payoffs,);
        }
        payoffs
    }

    fn handle_single_player_payoff(&mut self, payoffs: &mut Vec<HandPayoff,>,) {
        let pot_chips = self.current_pot.total_chips.clone();
        if let Some(id,) = self.active_player_id {
            let player = self.get_player_mut(&id,).expect("err",);
            player.chips += pot_chips;
            if let Some(payoff,) = payoffs
                .iter_mut()
                .find(|po| po.player_id == player.peer_id,)
            {
                payoff.chips += self.current_pot.total_chips;
            } else {
                payoffs.push(HandPayoff {
                    player_id: player.peer_id,
                    chips:     self.current_pot.total_chips,
                    cards:     Vec::new(),
                    rank:      String::default(),
                },);
            }
        }
    }

    fn handle_multi_player_payoff(&mut self, payoffs: &mut Vec<HandPayoff,>,) {
        let board = self.board.clone();
        let player_data = self.collect_player_data_for_pot(&self.current_pot,);
        let mut hands = self.evaluate_hands(&board, &player_data,);
        hands.sort_by(|a, b| b.1.cmp(&a.1,),);
        let winners_count = self.calculate_winners_count(&hands,);
        let win_payoff = self.current_pot.total_chips / winners_count as u32;
        let win_remainder = self.current_pot.total_chips % winners_count as u32;
        for (idx, &(id, v, ref bh,),) in
            hands.iter().take(winners_count,).enumerate()
        {
            let player_payoff = if idx == 0 {
                win_payoff + win_remainder
            } else {
                win_payoff
            };
            if let Some(player,) = self.contract.get_player_mut(&id,) {
                player.chips += player_payoff;
            }
            let mut cards = bh.to_vec();
            cards.sort_by_key(Card::rank,);
            if let Some(payoff,) =
                payoffs.iter_mut().find(|po| po.player_id == id,)
            {
                payoff.chips += player_payoff;
            } else {
                payoffs.push(HandPayoff {
                    player_id: id,
                    chips: player_payoff,
                    cards,
                    rank: v.rank().to_string(),
                },);
            }
        }
    }

    fn collect_player_data_for_pot(
        &self,
        pot: &Pot,
    ) -> Vec<(PeerId, Option<(Card, Card,),>,),> {
        self.get_players()
            .iter()
            .filter(|p| p.is_active && pot.participants.contains(&p.peer_id,),)
            .map(|p| {
                (p.peer_id, match p.hole_cards {
                    PlayerCards::Cards(c1, c2,) => Some((c1, c2,),),
                    _ => None,
                },)
            },)
            .filter(|(_, cards,)| cards.is_some(),)
            .collect()
    }

    fn evaluate_hands(
        &self,
        board: &[Card],
        player_data: &[(PeerId, Option<(Card, Card,),>,)],
    ) -> Vec<(PeerId, HandValue, [Card; 5],),> {
        player_data
            .iter()
            .map(|(id, cards,)| {
                let (c1, c2,) = cards.unwrap();
                let mut hand_cards = vec![c1, c2];
                hand_cards.extend_from_slice(board,);
                let (v, bh,) = HandValue::eval_with_best_hand(&hand_cards,);
                (*id, v, bh,)
            },)
            .collect()
    }

    fn calculate_winners_count(
        &self,
        hands: &[(PeerId, HandValue, [Card; 5],)],
    ) -> usize {
        hands.iter().filter(|(_, v, _,)| *v == hands[0].1,).count()
    }

    /// Checks if all players in the hand have acted.
    fn is_round_complete(&self,) -> bool {
        if self.count_active() < 2 {
            return true;
        }
        for player in self.get_players() {
            // If a player didn't match the last bet and is not all-in then the
            // player has to act and the round is not complete.
            if player.is_active
                && player.bet < self.last_bet
                && player.chips > Chips::ZERO
            {
                return false;
            }
        }
        // Only one player has chips all others are all in.
        if self.count_active_with_chips() < 2 {
            return true;
        }
        for player in self.get_players() {
            if player.is_active {
                // If a player didn't act the round is not complete.
                match player.action {
                    PlayerAction::None
                    | PlayerAction::SmallBlind
                    | PlayerAction::BigBlind
                        if player.chips > Chips::ZERO =>
                    {
                        return false;
                    },
                    _ => {},
                }
            }
        }
        true
    }

    fn next_round(&mut self,) {
        if self.count_active() < 2 {
            self.enter_end_hand();
            return;
        }
        while self.is_round_complete() {
            match self.phase() {
                HandPhase::PreflopBetting => {
                    self.enter_deal_flop();
                },
                HandPhase::TurnBetting => {
                    self.enter_deal_river();
                },
                HandPhase::FlopBetting => {
                    self.enter_deal_turn();
                },
                HandPhase::RiverBetting => {
                    self.enter_showdown();
                    return;
                },
                _ => {},
            }
        }
    }

    fn start_round(&mut self,) {
        self.update_pots();
        // Give some time to watch last action and pots.
        self.broadcast_throttle(1_000,);
        for player in self.contract.get_players_mut() {
            player.bet = Chips::ZERO;
            player.action = PlayerAction::None;
        }
        self.min_raise = self.big_blind;
        let () = self.request_action();
    }

    fn update_pots(&mut self,) {
        // Updates pots if there is a bet.
        if self.last_bet > Chips::ZERO {
            // Move bets to pots.
            loop {
                // Find minimum bet in case a player went all in.
                let min_bet = self
                    .get_players()
                    .iter()
                    .filter(|p| p.bet > Chips::ZERO,)
                    .map(|p| p.bet,)
                    .min()
                    .unwrap_or_default();
                if min_bet == Chips::ZERO {
                    break;
                }
                let active_player_ids = self
                    .get_players()
                    .iter()
                    .filter(|p| p.bet > Chips::ZERO,)
                    .map(|p| p.peer_id,)
                    .collect::<Vec<_,>>();
                let mut pot = self.current_pot.clone();
                for id in active_player_ids {
                    if let Some(player,) = self
                        .contract
                        .get_players_mut()
                        .iter_mut().find(|p| p.peer_id == id,)
                    {
                        player.bet -= min_bet;
                        pot.total_chips += min_bet;
                        if !pot.participants.contains(&id,) {
                            pot.participants.insert(id,);
                        }
                    }
                }
                self.current_pot = pot;
            }
        }
    }

    /// Request action to the active player.
    fn request_action(&mut self,) {
        let last_bet = self.get_last_bet();
        let game_id = self.get_game_id();
        let table_id = self.get_table_id();

        if let Some(id,) = self.active_player_id {
            let player = self.get_player_mut(&id).expect("err");

            let mut actions = vec![PlayerAction::Fold];
            if player.bet == last_bet {
                actions.push(PlayerAction::Check,);
            }
            if player.bet < last_bet {
                actions.push(PlayerAction::Call,);
            }
            if last_bet == Chips::ZERO && player.chips > Chips::ZERO {
                actions.push(PlayerAction::Bet {
                    bet_amount: last_bet,
                },);
            }
            if player.chips + player.bet > last_bet
                && last_bet > Chips::ZERO
                && player.chips > Chips::ZERO
            {
                actions.push(PlayerAction::Raise {
                    bet_amount: last_bet,
                },);
            }
            player.action_timer = Some(0,);
            let msg = Transition::ActionRequest {
                game_id,
                table: table_id,
                player_id: player.peer_id,
                min_raise: self.min_raise + self.last_bet,
                big_blind: self.big_blind,
                allowed:   actions,
            };
            let _ = self.send_contract(msg,);
        }
    }

    /// Broadcast a throttle message to all players at the table.
    fn broadcast_throttle(&mut self, millis: u32,) {
        for _ in self.get_players() {
            let () = self.send_throttle(millis,);
        }
    }

    fn send_throttle(&mut self, millis: u32,) {
        let msg = Transition::Throttle { millis, };
        let _ = self.send_contract(msg,);
    }
}
