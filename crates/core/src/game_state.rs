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
use crate::players_state::PlayerStateObjects;
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
    pub peer_id:                      PeerId,
    pub nickname:                     String,
    pub chips:                        Chips,
    pub bet:                          Chips,
    pub payoff:                       Option<HandPayoff,>,
    pub action:                       PlayerAction,
    pub action_timer:                 Option<u64,>,
    pub hole_cards:                   PlayerCards,
    pub public_cards:                 PlayerCards,
    pub has_button:                   bool,
    pub is_active:                    bool,
    has_sent_start_game_notification: bool,
}

impl PlayerPrivate {
    pub(crate) fn start_hand(&mut self,) {
        self.is_active = self.chips > Chips::ZERO;
        self.has_button = false;
        self.bet = Chips::ZERO;
        self.action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
        self.hole_cards = PlayerCards::None;
    }
}

impl PlayerPrivate {
    pub(crate) const fn finalize_hand(&mut self,) {
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
    pub(crate) fn reset_for_new_hand(&mut self,) {
        self.is_active = self.chips > Chips::ZERO;
        self.has_button = false;
        self.bet = Chips::ZERO;
        self.action = PlayerAction::None;
        self.hole_cards = PlayerCards::None;
        self.public_cards = PlayerCards::None;
    }
}

impl PlayerPrivate {
    const fn new(id: PeerId, nickname: String, chips: Chips,) -> Self {
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
    pub players:      PlayerStateObjects,
    pub board:        Vec<Card,>,
    pub pots:         Vec<Pot,>,
    pub small_blind:  Chips,
    pub big_blind:    Chips,
    num_seats:        usize,
    key_pair:         KeyPair,
    /// whether player associated with `key_pair` has joined a table.
    has_joined_table: bool,
    ui_has_joined_table: bool,

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
    pub fn apply(&mut self, msg: &WireMsg,) {
        match msg {
            WireMsg::JoinTableReq {
                player_id,
                nickname,
                chips,
                ..
            } => {
                self.players.add(PlayerPrivate::new(
                    *player_id,
                    nickname.clone(),
                    *chips,
                ),);
                self.has_joined_table = true;
            },
            WireMsg::PlayerJoinedConf {
                player_id,
                chips,
                seat_idx,
                ..
            } => {
                self.players.update_chips(*player_id, *chips,);
                self.players.set_seat(*player_id, *seat_idx,);
            },
            WireMsg::StartGameNotify {
                seat_order, sb, bb,
            ..
            } => {
                self.players.reseat(seat_order,);
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
                self.players.get_mut(&self.peer_id(),).unwrap().hole_cards =
                    PlayerCards::Cards(*card1, *card2,);
            },
            WireMsg::PlayerAction {
                peer_id, action, ..
            } => {
                match action {
                    PlayerAction::Bet { bet_amount, } => {
                        self.players.place_bet(*peer_id, *bet_amount, *action,);
                    },
                    PlayerAction::Fold => self.players.fold(*peer_id,),
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
            ui_has_joined_table: false,
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

            players:     self.players.clone(),
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
    pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone().iter().cloned().collect()
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
}

impl Projection {
    // — constructors —
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        nick: String,
        table_id: TableId,
        num_seats: usize,
        key_pair: KeyPair,
        connection: P2pTransport,
        cb: impl FnMut(SignedMessage,) + Send + 'static,
    ) -> Self {
        Self {
            ui_has_joined_table: false,
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
            pots: Vec::default(),
            game_id: GameId::new_id(),
            last_bet: Chips::ZERO,
            active_player: None,
            table_id,
            num_seats,
            key_pair,
            connection,
            callback: Box::new(cb,),

            phase: HandPhase::WaitingForPlayers,
            players: PlayerStateObjects::default(),
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

    /// Called every ~20 ms by the runtime.
    pub async fn tick(&mut self,) {
        if let Some(p,) =
            self.players.iter_mut().find(|p| p.action_timer.is_some(),)
        {
            if p.action_timer.unwrap() >= 1_500 {
                p.fold();
            }
        }
    }

    // — dispatcher called by runtime for every inbound network msg —
    pub async fn handle_network_msg(&mut self, sm: SignedMessage,) {
        info!(
            "internal table state of peer {} handling message with label {:?} sent from peer {}",
            self.peer_id(),
            sm.message().label(),
            sm.sender()
        );
        match sm.message() {
            NetworkMessage::ProtocolEntry(entry,) => {
                if entry.prev_hash != self.hash_head {
                    warn!("hash chain mismatch – discarding");
                    warn!("prev hash of sender: {}", entry.prev_hash);
                    warn!("prev hash of receiver (us) : {}", self.hash_head);
                    return;
                }
                let _ = self.commit_step(&entry.payload,);
            },
            NetworkMessage::NewListenAddr {
                listener_id: _listener_id,
                multiaddr,
            } => {
                self.listen_addr = Some(multiaddr.clone(),);
            },
        }
    }

    /// Run the pure state-machine transition **and immediately apply
    /// any side effects** (currently only `Effect::Send`).
    ///
    /// * `payload` – the incoming `WireMsg` that should be appended to the log.
    ///
    /// On success the canonical `self.contract` and `self.hash_head` are
    /// updated and every `Effect::Send` is signed & broadcast with the
    /// existing helpers.
    fn commit_step(&mut self, payload: &WireMsg) -> anyhow::Result<()> {
        use contract::{Effect, StepResult};

        // ---------- 1. pure state transition ----------
        let StepResult { next, effects } =
            contract::step(&self.contract, payload, &self.peer_context);

        // keep projection in sync
        self.apply(payload);

        // ---------- 2. append to our log ----------
        let next_hash = contract::hash_state(&next);
        let entry = LogEntry::with_key(
            self.hash_head.clone(),
            payload.clone(),
            next_hash.clone(),
            self.peer_id(),
        );

        // broadcast *and* loop back
        let signed = SignedMessage::new(&self.key_pair,
                                        NetworkMessage::ProtocolEntry(entry));
        self.send(signed)?;

        // commit canonical state
        self.contract = next;
        self.hash_head = next_hash;

        // ---------- 3. side‑effects ----------
        self.pending_effects.extend(
            effects.into_iter().filter_map(|e| match e {
                Effect::Send(m) => Some(m),
            })
        );

        Ok(())
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
                if table_id == self.table_id && peer_id == self.peer_id() {
                    let wiremsg = WireMsg::JoinTableReq {
                        table: table_id,
                        player_id: peer_id,
                        nickname,
                        chips,
                    };
                    self.has_joined_table = true;
                    let _ = self.sign_and_send(wiremsg,);
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
            if let Some(pos,) = self
                .players()
                .iter()
                .position(|p| p.peer_id == update.player_id,)
            {
                let player = &mut self.players()[pos];
                player.chips = update.chips;
                player.bet = update.bet;
                player.action = update.action;
                player.action_timer = update.action_timer;
                player.has_button = update.is_dealer;
                player.is_active = update.is_active;

                // Do not override cards for the local player as they are
                // updated when we get a DealCards message.
                if pos != 0 {
                    player.hole_cards = update.hole_cards;
                }

                // If local player has folded remove its cards.
                if pos == 0 && !player.is_active {
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
    pub ui_has_joined_table: bool,

    pub players:     PlayerStateObjects,
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
            ui_has_joined_table: false,
            key_pair: KeyPair::default(),
            prev_hash: GENESIS_HASH.clone(),
            table_id,
            seats,
            game_started: false,
            player_id: peer_id,
            nickname,
            players: PlayerStateObjects::default(),
            board: Vec::default(),
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
            ui_has_joined_table: false,
            key_pair:         KeyPair::default(),
            prev_hash:        GENESIS_HASH.clone(),
            has_joined_table: false,
            table_id:         TableId::new_id(),
            seats:            0,
            game_started:     false,
            player_id:        PeerId::default(),
            players:          PlayerStateObjects::default(),
            nickname:         String::default(),
            board:            Vec::default(),
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
        self.players.players()
    }

    #[must_use]
    pub fn player_id(&self,) -> String {
        self.player_id.to_string()
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
        let seats =
            self.players.iter().map(|p| p.peer_id,).collect::<Vec<_,>>();
        let _ = self.send_contract(WireMsg::StartGameNotify {
            seat_order: seats,
            table:      self.table_id,
            game_id:    self.game_id,
            sb:         Self::START_GAME_SB,
            bb:         Self::START_GAME_BB,
        },);
        if let Some(me,) = self.players.get_mut(&self.peer_id(),) {
            me.sent_start_game_notification();
        }

        // -----------------------------------------------------------
        // 2) wait until *everyone* has sent the notify
        // ---------------------------------------------------------
        let deadline = Instant::now()
            + Duration::from_secs(timeout_s * u64::from(max_retries,),);
        loop {
            if self
                .players
                .iter()
                .all(PlayerPrivate::has_sent_start_game_notification,)
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

        self.players.start_hand();

        // If there are fewer than 2 active players end the game.
        if self.players.count_active() < 2 {
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

        // Reset pots.
        self.pots = vec![Pot::default()];

        // Deal cards to each player.
        for player in &mut self.players {
            if player.is_active {
                player.public_cards = PlayerCards::Covered;

                // Sort cards for the UI.
                let (c1, c2,) = (self.deck.deal(), self.deck.deal(),);
                player.hole_cards = if c1.rank() < c2.rank() {
                    PlayerCards::Cards(c1, c2,)
                } else {
                    PlayerCards::Cards(c2, c1,)
                };
            } else {
                player.hole_cards = PlayerCards::None;
                player.public_cards = PlayerCards::None;
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
        let () = self.action_update();
    }

    fn action_update(&mut self,) {
        self.players.activate_next_player();

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

        for player in &mut self.players {
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
        self.players.end_hand();
        let _ = self.send_contract(WireMsg::EndHand {
            payoffs: winners,
            pot:     self.pot().total_chips,
        },);

        // End game if only player has chips or move to next hand.
        if self.players.count_with_chips() < 2 {
            let () = self.enter_end_game();
        } else {
            // All players that run out of chips must leave the table before the
            // start of a new hand.
            for player in self.players() {
                if player.chips == Chips::ZERO {
                    // Notify the client that this player has left the table.
                    let _ = self.send_contract(WireMsg::LeaveTable {
                        player_id: player.peer_id,
                    },);
                }
            }

            self.players.remove_with_no_chips();
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
            let _ = self.send_contract(msg.clone(),);
        }

        self.players.clear();

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

        match self.players.count_active() {
            1 => {
                // If one player left gets all the chips.
                if let Some(player,) = &mut self.active_player {
                    for pot in self.pots.clone() {
                        player.chips += pot.total_chips;

                        if let Some(payoff,) = payoffs
                            .iter_mut()
                            .find(|po| po.player_id == player.peer_id,)
                        {
                            payoff.chips += pot.total_chips;
                        } else {
                            payoffs.push(HandPayoff {
                                player_id: player.peer_id,
                                chips:     pot.total_chips,
                                cards:     Vec::default(),
                                rank:      String::default(),
                            },);
                        }
                    }
                }
            },
            n if n > 1 => {
                // With more than 1 active player we need to compare hands for
                // each pot
                for pot in self.pots.drain(..,) {
                    // Evaluate all active players hands.
                    let mut hands =
                        self.players
                            .iter_mut()
                            .filter(|p| {
                                p.is_active
                                    && pot.participants.contains(&p.peer_id,)
                            },)
                            .filter_map(|p| {
                                match p.hole_cards {
                                    PlayerCards::None
                                    | PlayerCards::Covered => None,
                                    PlayerCards::Cards(c1, c2,) => {
                                        Some((p, c1, c2,),)
                                    },
                                }
                            },)
                            .map(|(p, c1, c2,)| {
                                let mut cards = vec![c1, c2];
                                cards.extend_from_slice(&self.board,);
                                let (v, bh,) =
                                    HandValue::eval_with_best_hand(&cards,);
                                (p, v, bh,)
                            },)
                            .collect::<Vec<_,>>();

                    // This may happen when the last pot is empty.
                    if hands.is_empty() {
                        continue;
                    }

                    // Sort descending order, winners first.
                    hands.sort_by(|p1, p2| p2.1.cmp(&p1.1,),);

                    // Count hands with the same value.
                    let winners_count = hands
                        .iter()
                        .filter(|(_, v, _,)| v == &hands[0].1,)
                        .count();
                    let win_payoff = pot.total_chips / winners_count as u32;
                    let win_remainder = pot.total_chips % winners_count as u32;

                    for (idx, (player, v, bh,),) in
                        hands.iter_mut().take(winners_count,).enumerate()
                    {
                        // Give remainder to first player.
                        let player_payoff = if idx == 0 {
                            win_payoff + win_remainder
                        } else {
                            win_payoff
                        };

                        player.chips += player_payoff;

                        // Sort by rank for the UI.
                        let mut cards = bh.to_vec();
                        cards.sort_by_key(Card::rank,);

                        // If a player has already a payoff add chips to that
                        // one.
                        if let Some(payoff,) = payoffs
                            .iter_mut()
                            .find(|po| po.player_id == player.peer_id,)
                        {
                            payoff.chips += player_payoff;
                        } else {
                            payoffs.push(HandPayoff {
                                player_id: player.peer_id,
                                chips: player_payoff,
                                cards,
                                rank: v.rank().to_string(),
                            },);
                        }
                    }
                }
            },
            _ => {},
        }

        payoffs
    }

    /// Checks if all players in the hand have acted.
    fn is_round_complete(&self,) -> bool {
        if self.players.count_active() < 2 {
            return true;
        }

        for player in &self.players {
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
        if self.players.count_active_with_chips() < 2 {
            return true;
        }

        for player in &self.players {
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
        if self.players.count_active() < 2 {
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

        for player in &mut self.players {
            player.bet = Chips::ZERO;
            player.action = PlayerAction::None;
        }

        self.min_raise = self.big_blind;

        self.players.start_round();

        let _ = self.request_action();
    }

    fn update_pots(&mut self,) {
        // Updates pots if there is a bet.
        if self.last_bet > Chips::ZERO {
            // Move bets to pots.
            loop {
                // Find minimum bet in case a player went all in.
                let min_bet = self
                    .players
                    .iter()
                    .filter(|p| p.bet > Chips::ZERO,)
                    .map(|p| p.bet,)
                    .min()
                    .unwrap_or_default();

                if min_bet == Chips::ZERO {
                    break;
                }

                let mut went_all_in = false;
                for player in &mut self.players {
                    let pot = self.pots.last_mut().unwrap();
                    if player.bet > Chips::ZERO {
                        player.bet -= min_bet;
                        pot.total_chips += min_bet;

                        if !pot.participants.contains(&player.peer_id,) {
                            pot.participants.insert(player.peer_id,);
                        }

                        went_all_in = player.chips == Chips::ZERO;
                    }
                }

                if went_all_in {
                    self.pots.push(Pot::default(),);
                }
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
