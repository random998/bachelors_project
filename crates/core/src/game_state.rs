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
use tokio::time::sleep;

use crate::connection_stats::ConnectionStats;
use crate::crypto::{KeyPair, PeerId, SecretKey};
use crate::message::{
    HandPayoff, NetworkMessage, PlayerAction, PlayerUpdate, SignedMessage,
    UiCmd,
};
use crate::net::traits::P2pTransport;
use crate::poker::{Card, Chips, Deck, GameId, PlayerCards, TableId};
use crate::protocol::msg::{Hash, LogEntry, WireMsg};
use crate::protocol::state::{
    self as contract, ContractState, GENESIS_HASH, PeerContext,
};
// per-player helper

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

/// One player as stored in *our* local copy of the table state.
#[derive(Debug, Clone, Serialize, Deserialize,)]
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
    #[must_use] pub const fn new(id: PeerId, nickname: String, chips: Chips,) -> Self {
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

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize,)]
pub enum HandPhase {
    WaitingForPlayers,
    StartingGame,
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

/// A *derived*, mutable view of the canonical contract state,
/// enriched with peer–local convenience data.
///
/// Most of it is what you already have in `InternalTableState`.
pub struct Projection {
    // 1. Canonical data – always equals contract after replay
    pub table_id:     TableId,
    pub game_id:      GameId,
    pub phase:        HandPhase,
    pub board:        Vec<Card,>,
    pub small_blind:  Chips,
    pub big_blind:    Chips,
    num_seats:        usize,
    key_pair:         KeyPair,
    /// whether player associated with `key_pair` has joined a table.
    has_joined_table: bool,
    is_seed_peer:     bool,

    // 2. Peer-local “soft” state – NOT part of consensus
    pub rng:             StdRng, // used only when *we* deal
    pub listen_addr:     Option<Multiaddr,>,
    pub connection_info: ConnectionStats, // bytes/sec graphs, peer RTT …

    // networking ----------------------------------------------------------
    pub connection:  P2pTransport,
    /// UI callback – echoes every locally-generated message without locks.
    pub callback:    Box<dyn FnMut(SignedMessage,) + Send,>,
    // Outbound messages that came back from the pure state machine and still
    /// need to be broadcast.
    pending_effects: Vec<WireMsg,>,

    // game state ----------------------------------------------------------
    deck:           Deck,
    current_pot:    Pot,
    action_request: Option<ActionRequest,>,
    active_player:  Option<PlayerPrivate,>,
    last_bet:       Chips,
    hand_count:     usize,
    min_raise:      Chips,
    game_started:   bool,
    chain:          Vec<LogEntry,>,

    // misc ----------------------------------------------------------------
    new_hand_start_timer: Option<Instant,>,
    new_hand_timeout:     Duration,

    // crypto --------------------------------------------------------------
    contract:  ContractState,
    hash_head: Hash,

    // peer context
    peer_context: PeerContext,
}

impl Projection {
    #[must_use] pub fn hash_head(&self,) -> Hash {
        self.hash_head.clone()
    }

    #[must_use] pub const fn has_joined_table(&self,) -> bool {
        self.has_joined_table
    }
    #[must_use] pub fn chain(&self,) -> Vec<LogEntry,> {
        self.chain.clone()
    }

    // Derived players access
    #[must_use] pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.contract.players.values().cloned().collect()
    }

    pub fn players_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut PlayerPrivate,> + '_ {
        self.contract.players.values_mut()
    }

    #[must_use] pub fn get_player(&self, id: &PeerId,) -> Option<PlayerPrivate,> {
        self.contract.players.get(id,).cloned()
    }

    pub fn get_player_mut(
        &mut self,
        id: &PeerId,
    ) -> Option<&mut PlayerPrivate,> {
        self.contract.players.get_mut(id,)
    }

    // Example derived methods for counts and operations
    #[must_use] pub fn count_active(&self,) -> usize {
        self.contract
            .players
            .values()
            .filter(|p| p.is_active,)
            .count()
    }

    #[must_use] pub fn count_with_chips(&self,) -> usize {
        self.contract
            .players
            .values()
            .filter(|p| p.has_chips(),)
            .count()
    }

    #[must_use] pub fn count_active_with_chips(&self,) -> usize {
        self.contract
            .players
            .values()
            .filter(|p| p.is_active && p.has_chips(),)
            .count()
    }

    pub fn reseat(&mut self, seat_order: &[PeerId],) {
        for (idx, id,) in seat_order.iter().enumerate() {
            if let Some(player,) = self.get_player_mut(id,) {
                player.seat_idx = Some(idx as u8,);
            }
        }
    }

    pub fn fold(&mut self, id: &PeerId,) {
        if let Some(player,) = self.get_player_mut(id,) {
            player.fold();
        }
    }

    pub fn place_bet(
        &mut self,
        id: &PeerId,
        total_bet: Chips,
        action: PlayerAction,
    ) {
        if let Some(player,) = self.get_player_mut(id,) {
            player.place_bet(total_bet, action,);
        }
    }

    pub fn start_hand(&mut self,) {
        for player in self.players_mut() {
            player.start_hand();
        }
    }

    pub fn end_hand(&mut self,) {
        for player in self.players_mut() {
            player.finalize_hand();
        }
    }

    pub fn remove_with_no_chips(&mut self,) {
        self.contract.players.retain(|_, p| p.chips > Chips::ZERO,);
    }

    pub fn activate_next_player(&mut self,) {
        // Implement based on seat order or logic; stub
        todo!()
    }

    pub fn apply(&mut self, msg: &WireMsg,) {
        match msg {
            WireMsg::JoinTableReq {
                player_id,
                nickname,
                chips,
                ..
            } => {
                let player =
                    PlayerPrivate::new(*player_id, nickname.clone(), *chips,);
                self.contract.players.insert(*player_id, player,);
            },
            WireMsg::PlayerJoinedConf {
                player_id,
                chips,
                seat_idx,
                ..
            } => {
                if let Some(player,) = self.contract.players.get_mut(player_id,)
                {
                    player.chips = *chips;
                    player.seat_idx = Some(*seat_idx,);
                } else {
                    let nick = format!("peer‑{}", &player_id.to_string()[..6]);
                    let mut player =
                        PlayerPrivate::new(*player_id, nick, *chips,);
                    player.seat_idx = Some(*seat_idx,);
                    self.contract.players.insert(*player_id, player,);
                }
            },
            WireMsg::StartGameNotify {
                seat_order, sb, bb,
            ..
            } => {
                self.reseat(seat_order,);
                self.small_blind = *sb;
                self.big_blind = *bb;
                self.phase = HandPhase::StartingGame;
            },
            WireMsg::DealCards {
                player_id,
                card1,
                card2,
                ..
            } if *player_id == self.peer_id() => {
                if let Some(player,) = self.get_player_mut(&self.peer_id(),) {
                    player.hole_cards = PlayerCards::Cards(*card1, *card2,);
                }
            },
            WireMsg::PlayerAction {
                peer_id, action, ..
            } => {
                match action {
                    PlayerAction::Bet { bet_amount, } => {
                        self.place_bet(peer_id, *bet_amount, *action,);
                    },
                    PlayerAction::Fold => self.fold(peer_id,),
                    _ => {},
                }
            },
            _ => {},
        }
    }
}

impl Projection {
    const START_GAME_SB: Chips = Chips::ZERO;
    const START_GAME_BB: Chips = Chips::ZERO;
    const ACTION_TIMEOUT_MILLIS: u64 = 1500;

    /// grab an immutable snapshot for the GUI
    #[must_use]
    pub fn snapshot(&self,) -> GameState {
        GameState {
            is_seed_peer:     self.has_joined_table,
            key_pair:         self.key_pair.clone(),
            prev_hash:        self.hash_head.clone(),
            has_joined_table: self.has_joined_table,
            table_id:         self.table_id,
            seats:            self.num_seats,
            game_started:     !matches!(
                self.phase,
                HandPhase::WaitingForPlayers
            ),

            player_id: self.key_pair.clone().peer_id(),
            nickname:  String::new(), // Optional: store locally if needed

            players:     self.players(),
            board:       self.board.clone(),
            pot:         self.current_pot.clone(),
            action_req:  self.action_request.clone(),
            hand_phase:  self.phase.clone(),
            listen_addr: self.listen_addr.clone(),
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
        cb: impl FnMut(SignedMessage,) + Send + 'static,
        is_seed_peer: bool,
    ) -> Self {
        Self {
            is_seed_peer,
            peer_context: PeerContext::new(
                key_pair.peer_id(),
                nick,
                Chips::default(),
            ),
            pending_effects: Vec::new(),
            connection_info: ConnectionStats::default(),
            has_joined_table: false,
            contract: Default::default(),
            hash_head: GENESIS_HASH.clone(),
            game_started: false,
            hand_count: 0,
            min_raise: Chips::ZERO,
            game_id: GameId::new_id(),
            last_bet: Chips::ZERO,
            active_player: None,
            table_id,
            num_seats,
            key_pair,
            connection,
            callback: Box::new(cb,),

            phase: HandPhase::WaitingForPlayers,
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
            chain: Vec::new(),
        }
    }

    // — public helpers (runtime ↔ UI) —

    pub async fn update(&mut self,) {
        if self.game_started && self.phase == HandPhase::StartingGame {
            self.enter_start_game(1, 5,).await;
        }
    }

    pub fn try_recv(
        &mut self,
    ) -> Result<SignedMessage, mpsc::error::TryRecvError,> {
        self.connection.rx.network_msg_receiver.try_recv()
    }

    /// Low-level send used by `sign_and_send` **and** by the gossip handler.
    pub fn send(&mut self, msg: SignedMessage,) -> anyhow::Result<(),> {
        info!("{} sending {}", self.peer_id(), msg.message().label());
        // engine → network
        self.connection.tx.network_msg_sender.try_send(msg,)?;
        // also send the same msg to ourselves
        // TODO send callbacks to ourselves.
        Ok((),)
    }

    /// Called every ~20 ms by the runtime.
    pub async fn tick(&mut self,) {
        // NEW ───────────────────────────────────────────────────────────
        // broadcast all WireMsg side‑effects that the *pure* contract
        // returned in previous steps.  Each message is appended to the log
        // via `sign_and_send`, so ordering & hashing stay consistent.

        let mut pending = self.pending_effects.clone();
        while let Some(eff,) = pending.pop() {
            // ignore errors – the network layer already logs them
            let _ = self.sign_and_send(eff,);
        }
        self.pending_effects = pending;

        // ───────────────────────────────────────────────────────────────

        // existing fold‑timeout logic
        if let Some(p,) = self.players_mut().find(|p| p.action_timer.is_some(),)
        {
            if p.action_timer.unwrap_or(0,) >= 1_500 {
                p.fold();
            }
        }
    }

    // ────────────────────────────────────────────────────────────────────────────
    // inside `impl Projection { … }` ‑ add to `handle_network_msg`
    // ────────────────────────────────────────────────────────────────────────────

    // game_state.rs  ─ replace the body of `handle_network_msg`
    pub async fn handle_network_msg(&mut self, sm: SignedMessage,) {
        match sm.message() {
            NetworkMessage::ProtocolEntry(entry,) => {
                // 1. reject genuinely invalid branches
                if entry.prev_hash != self.hash_head {
                    warn!("hash mismatch – discarding");
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
                player_id,
                nickname,
                chips,
            } => {
                if self.table_id != *table {
                    return;
                }
                if self.get_player(player_id,).is_some() {
                    return; // Already joined
                }
                if self.contract.players.len() >= self.num_seats {
                    return; // Table full
                }
                if self.game_started {
                    return; // Game started
                }

                // Append JoinTableReq to chain
                let join_msg = WireMsg::JoinTableReq {
                    table:     *table,
                    player_id: *player_id,
                    nickname:  nickname.clone(),
                    chips:     *chips,
                };
                let _ = self.commit_step(&join_msg,);

                // Send full chain (now includes join) to new player
                let resp = NetworkMessage::SyncResp {
                    target: *player_id,
                    chain:  self.chain.clone(),
                };
                let _ = self.send_plain(resp,);
            },
            NetworkMessage::SyncResp { target, chain, } => {
                if *target != self.peer_id() {
                    return;
                }

                // Validate and replay chain
                let mut current_state = ContractState::default();
                let mut current_hash = GENESIS_HASH.clone();
                for entry in chain.iter().cloned() {
                    if entry.prev_hash != current_hash {
                        warn!("invalid chain - prev_hash mismatch");
                        return;
                    }

                    let res = contract::step(&current_state, &entry.payload,);
                    let next = res.next;
                    let next_hash = contract::hash_state(&next,);

                    if next_hash != entry.next_hash {
                        warn!("invalid chain - next_hash mismatch");
                        return;
                    }

                    // Update projection
                    self.apply(&entry.payload,);

                    // Ignore effects during replay

                    current_state = next;
                    current_hash = next_hash;
                }

                self.contract = current_state;
                self.hash_head = current_hash;
                self.chain = chain.clone();
                self.has_joined_table = true;
                info!("Chain synced successfully; joined table");
            },
        }
    }
    // ---------------------------------------------------------------------------
    // 2. Commit step: move “canonical state commit” above the side‑effect queue
    //    (so that a subsequent self.sign_and_send() sees the new head)
    // ---------------------------------------------------------------------------

    fn commit_step(&mut self, payload: &WireMsg,) -> anyhow::Result<(),> {
        use contract::{Effect, StepResult};

        // 1. pure state transition
        let StepResult { next, effects, } =
            contract::step(&self.contract, payload,);

        // 2. bring local *projection* in sync
        self.apply(payload,);

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
        self.chain.push(entry,);

        // 6. queue side-effects for the next tick
        self.pending_effects
            .extend(effects.into_iter().filter_map(|e| {
                match e {
                    Effect::Send(m,) => Some(m,),
                }
            },),);
        Ok((),)
    }
    pub async fn handle_ui_msg(&mut self, msg: UiCmd,) {
        match msg {
            UiCmd::PlayerJoinTableRequest {
                table_id,
                peer_id,
                nickname,
                chips,
            } => {
                info!("handling ui jointablereq command!");
                if table_id == self.table_id
                    && peer_id == self.peer_id()
                    && !self.has_joined_table
                    && self.get_player(&peer_id,).is_none()
                {
                    let wiremsg = WireMsg::JoinTableReq {
                        table: table_id,
                        player_id: peer_id,
                        nickname: nickname.clone(),
                        chips,
                    };
                    if !self.is_seed_peer && self.hash_head == *GENESIS_HASH {
                        // New non-seed player: request sync first
                        let sync_msg = NetworkMessage::SyncReq {
                            table: table_id,
                            player_id: peer_id,
                            nickname,
                            chips,
                        };
                        let _ = self.send_plain(sync_msg,);
                    } else {
                        // Seed peer or already-synced: direct commit
                        let _ = self.commit_step(&wiremsg,);
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
            if let Some(player,) = self.get_player_mut(&update.player_id,) {
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
//  Errors (user-visible)
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
#[derive(Debug, Clone, Serialize, Deserialize,)]
pub struct GameState {
    pub table_id:     TableId,
    pub seats:        usize,
    pub game_started: bool,

    pub player_id:        PeerId, // local player
    pub nickname:         String,
    /// player with `player_id` has joined table
    pub has_joined_table: bool,
    pub is_seed_peer:     bool,

    pub players:     Vec<PlayerPrivate,>,
    pub board:       Vec<Card,>,
    pub pot:         Pot,
    pub action_req:  Option<ActionRequest,>,
    pub hand_phase:  HandPhase,
    pub listen_addr: Option<Multiaddr,>,

    // -- crypto
    pub prev_hash: Hash,
    pub key_pair:  KeyPair,
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
            is_seed_peer: false,
            key_pair: KeyPair::generate(),
            prev_hash: GENESIS_HASH.clone(),
            table_id,
            seats,
            game_started: false,
            player_id: peer_id,
            nickname,
            players: Vec::new(),
            board: Vec::new(),
            pot: Pot::default(),
            action_req: None,
            hand_phase: HandPhase::StartingGame,
            listen_addr: None,
            has_joined_table: false,
        }
    }

    #[must_use]
    pub fn default() -> Self {
        Self {
            is_seed_peer:     false,
            key_pair:         KeyPair::generate(),
            prev_hash:        GENESIS_HASH.clone(),
            has_joined_table: false,
            table_id:         TableId::new_id(),
            seats:            0,
            game_started:     false,
            player_id:        PeerId::default(),
            players:          Vec::new(),
            nickname:         String::default(),
            board:            Vec::new(),
            pot:              Pot::default(),
            action_req:       None,
            hand_phase:       HandPhase::StartingGame,
            listen_addr:      None,
        }
    }

    #[must_use]
    pub fn action_req(&self,) -> Option<ActionRequest,> {
        self.action_req.clone()
    }

    pub fn pot(&mut self,) -> Pot {
        self.pot.clone()
    }

    pub const fn pot_chips(&mut self,) -> Chips {
        self.pot.total_chips
    }

    #[must_use]
    pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone()
    }

    #[must_use]
    pub const fn player_id(&self,) -> PeerId {
        self.player_id
    }
}

impl Projection {
    // send a protocol entry (hash-chained)
    fn send_contract(&mut self, payload: WireMsg,) -> anyhow::Result<(),> {
        self.commit_step(&payload,)
    }

    // old api still needed for GUI / UX messages ---------------------
    fn send_plain(&mut self, msg: NetworkMessage,) -> anyhow::Result<(),> {
        let sm = SignedMessage::new(&self.key_pair, msg,);
        self.send(sm,)
    }

    // convenience wrappers ------------------------------------------
    pub fn sign_and_send(&mut self, payload: WireMsg,) -> anyhow::Result<(),> {
        self.send_contract(payload,)
    }

    pub fn send_gui(&mut self, msg: NetworkMessage,) -> anyhow::Result<(),> {
        self.send_plain(msg,)
    }

    async fn enter_start_game(&mut self, timeout_s: u64, max_retries: u32,) {
        self.phase = HandPhase::StartingGame;
        self.game_started = true;

        // -----------------------------------------------------------
        // 1) broadcast our StartGameNotify and set *our* flag
        // ---------------------------------------------------------
        let seats = self
            .players()
            .iter()
            .map(|p| p.peer_id,)
            .collect::<Vec<_,>>();
        let _ = self.send_contract(WireMsg::StartGameNotify {
            seat_order: seats,
            table:      self.table_id,
            game_id:    self.game_id,
            sb:         Self::START_GAME_SB,
            bb:         Self::START_GAME_BB,
        },);
        if let Some(me,) = self.get_player_mut(&self.peer_id(),) {
            me.sent_start_game_notification();
        }

        // -----------------------------------------------------------
        // 2) wait until *everyone* has sent the notify
        // ---------------------------------------------------------
        let deadline = Instant::now()
            + Duration::from_secs(timeout_s * u64::from(max_retries,),);
        loop {
            if self
                .players()
                .iter()
                .all(|p| p.has_sent_start_game_notification,)
            {
                // ✅ ready – enter the hand
                self.enter_start_hand();
                break;
            }

            if Instant::now() >= deadline {
                warn!("start-game sync timed out – aborting");
                break;
            }

            sleep(Duration::from_secs(timeout_s,),).await;
        }
    }
    /// Start a new hand.
    fn enter_start_hand(&mut self,) {
        self.phase = HandPhase::StartingHand;

        self.start_hand();

        // If there are fewer than 2 active players end the game.
        if self.count_active() < 2 {
            self.enter_end_game();
            return;
        }

        self.update_blinds();

        // Pay small and big blind.
        if let Some(p,) = &mut self.active_player {
            p.place_bet(self.small_blind, PlayerAction::SmallBlind,);
        }

        // Pay small and big blind.
        if let Some(p,) = &mut self.active_player {
            p.place_bet(self.big_blind, PlayerAction::BigBlind,);
        }

        self.last_bet = self.big_blind;
        self.min_raise = self.big_blind;

        // Create a new deck.
        self.deck = Deck::shuffled(&mut self.rng,);

        // Clear board.
        self.board.clear();

        let active_ids = self
            .players()
            .iter()
            .filter(|p| p.is_active,)
            .map(|p| p.peer_id,)
            .collect::<Vec<_,>>();

        for player in self.players_mut() {
            if !active_ids.contains(&player.peer_id,) {
                player.hole_cards = PlayerCards::None;
                player.public_cards = PlayerCards::None;
            }
        }

        for &id in &active_ids {
            let c1 = self.deck.deal();
            let c2 = self.deck.deal();
            if let Some(player,) = self.get_player_mut(&id,) {
                player.public_cards = PlayerCards::Covered;
                player.hole_cards = if c1.rank() < c2.rank() {
                    PlayerCards::Cards(c1, c2,)
                } else {
                    PlayerCards::Cards(c2, c1,)
                };
            }
        }
        // Deal the cards to each player.
        for player in self.players() {
            if let PlayerCards::Cards(c1, c2,) = player.hole_cards {
                let msg = WireMsg::DealCards {
                    card1:     c1,
                    card2:     c2,
                    player_id: player.peer_id,
                    table:     self.table_id,
                    game_id:   self.game_id,
                };
                let _ = self.send_contract(msg,);
            }
        }

        self.enter_preflop_betting();
    }

    fn enter_preflop_betting(&mut self,) {
        self.phase = HandPhase::PreflopBetting;
        self.action_update();
    }

    fn action_update(&mut self,) {
        self.activate_next_player();

        if self.is_round_complete() {
            self.next_round();
        } else {
            let _ = self.request_action();
        }
    }

    fn enter_deal_flop(&mut self,) {
        for _ in 1..=3 {
            self.board.push(self.deck.deal(),);
        }

        self.phase = HandPhase::FlopBetting;
        self.start_round();
    }

    fn enter_deal_turn(&mut self,) {
        self.board.push(self.deck.deal(),);
        self.phase = HandPhase::TurnBetting;
        self.start_round();
    }

    fn enter_deal_river(&mut self,) {
        self.board.push(self.deck.deal(),);

        self.phase = HandPhase::RiverBetting;
        self.start_round();
    }

    fn enter_showdown(&mut self,) {
        self.phase = HandPhase::Showdown;

        for player in self.players_mut() {
            player.action = PlayerAction::None;
            if player.is_active {
                player.public_cards = player.hole_cards;
            }
        }

        self.enter_end_hand();
    }

    fn enter_end_hand(&mut self,) {
        self.new_hand_timeout = if matches!(self.phase, HandPhase::Showdown) {
            // If coming from a showdown give players more time to see the
            // winning hand and chips.
            Duration::from_millis(7_000,)
        } else {
            Duration::from_millis(3_000,)
        };

        self.phase = HandPhase::EndingHand;

        self.update_pots();

        // Give time to the UI to look at the updated pot and board.
        let _ = self.send_throttle(100,);

        let winners = self.pay_bets();

        // Update players and broadcast update to all players.
        self.end_hand();
        let _ = self.send_contract(WireMsg::EndHand {
            payoffs: winners,
            pot:     self.pot().total_chips,
        },);

        // End game if only player has chips or move to next hand.
        if self.count_with_chips() < 2 {
            self.enter_end_game();
        } else {
            // All players that run out of chips must leave the table before the
            // start of a new hand.
            for player in self.players() {
                // Clone to avoid borrow issues
                if player.chips == Chips::ZERO {
                    // Notify the client that this player has left the table.
                    let _ = self.send_contract(WireMsg::LeaveTable {
                        player_id: player.peer_id,
                    },);
                }
            }

            self.remove_with_no_chips();
            self.new_hand_start_timer = Some(Instant::now(),);
        }
    }

    fn enter_end_game(&mut self,) {
        // Give time to the UI to look at winning results before ending the
        // game.
        self.broadcast_throttle(4_500,);

        self.phase = HandPhase::EndingGame;

        for player in self.players() {
            // Notify the client that this player has left the table.
            let msg = WireMsg::LeaveTable {
                player_id: player.peer_id,
            };
            let _ = self.send_contract(msg,);
        }

        self.contract.players.clear();

        // Reset hand count for next game.
        self.hand_count = 0;

        // Wait for players to join.
        self.phase = HandPhase::WaitingForPlayers;
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
        if let Some(player,) = &mut self.active_player {
            player.chips += self.current_pot.total_chips;

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

            if let Some(player,) = self.get_player_mut(&id,) {
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
        self.players()
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

        for player in self.players() {
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

        for player in self.players() {
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
            match self.phase {
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

        for player in self.players_mut() {
            player.bet = Chips::ZERO;
            player.action = PlayerAction::None;
        }

        self.min_raise = self.big_blind;

        self.start_round(); // Assuming this is a method; adjust if needed

        let _ = self.request_action();
    }

    fn update_pots(&mut self,) {
        // Updates pots if there is a bet.
        if self.last_bet > Chips::ZERO {
            // Move bets to pots.
            loop {
                // Find minimum bet in case a player went all in.
                let min_bet = self
                    .players()
                    .iter()
                    .filter(|p| p.bet > Chips::ZERO,)
                    .map(|p| p.bet,)
                    .min()
                    .unwrap_or_default();

                if min_bet == Chips::ZERO {
                    break;
                }

                let mut went_all_in = false;
                let active_player_ids = self
                    .players()
                    .iter()
                    .filter(|p| p.bet > Chips::ZERO,)
                    .map(|p| p.peer_id,)
                    .collect::<Vec<_,>>();

                let mut pot = self.current_pot.clone();
                for id in active_player_ids {
                    if let Some(player,) = self.get_player_mut(&id,) {
                        player.bet -= min_bet;
                        pot.total_chips += min_bet;

                        if !pot.participants.contains(&id,) {
                            pot.participants.insert(id,);
                        }

                        went_all_in |= player.chips == Chips::ZERO;
                    }
                }
                self.current_pot = pot;
            }
        }
    }
    /// Request action to the active player.
    async fn request_action(&mut self,) {
        if let Some(player,) = &mut self.active_player {
            let mut actions = vec![PlayerAction::Fold];

            if player.bet == self.last_bet {
                actions.push(PlayerAction::Check,);
            }

            if player.bet < self.last_bet {
                actions.push(PlayerAction::Call,);
            }

            if self.last_bet == Chips::ZERO && player.chips > Chips::ZERO {
                actions.push(PlayerAction::Bet {
                    bet_amount: self.last_bet,
                },);
            }

            if player.chips + player.bet > self.last_bet
                && self.last_bet > Chips::ZERO
                && player.chips > Chips::ZERO
            {
                actions.push(PlayerAction::Raise {
                    bet_amount: self.last_bet,
                },);
            }

            player.action_timer = Some(0,);

            let msg = WireMsg::ActionRequest {
                game_id:   self.game_id,
                table:     self.table_id,
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
        for _ in self.players() {
            let _ = self.send_throttle(millis,);
        }
    }

    async fn send_throttle(&mut self, millis: u32,) {
        let msg = WireMsg::Throttle { millis, };
        let _ = self.send_contract(msg,);
    }
}
