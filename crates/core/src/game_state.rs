//! Poker table state management – **lock-free** client version
//! Adapted from <https://github.com/vincev/freezeout>

use std::fmt;
use std::fmt::Formatter;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use libp2p::Multiaddr;
use log::{info, warn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use poker_eval::HandValue;
use crate::crypto::{PeerId, SecretKey, KeyPair};
use crate::message::{
    HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage,
};
use crate::net::traits::P2pTransport;
use crate::players_state::PlayerStateObjects;
use crate::poker::{Card, Chips, Deck, GameId, PlayerCards, TableId}; // per-player helper

// ────────────────────────────────────────────────────────────────────────────
//  Small helpers
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize,)]
pub struct Pot {
    participants: AHashSet<PeerId,>,
    total_chips:  Chips,
}

impl Pot {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPrivate {
    pub id:           PeerId,
    pub nickname:     String,
    pub chips:        Chips,
    pub bet:          Chips,
    pub payoff:       Option<HandPayoff,>,
    pub action:       PlayerAction,
    pub action_timer: Option<u64,>,
    pub hole_cards:        PlayerCards,
    pub public_cards:        PlayerCards,
    pub has_button:   bool,
    pub is_active:    bool,
}

impl PlayerPrivate {
    pub(crate) fn start_hand(&mut self) {
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
        self.id.to_string()
    }
}

impl PlayerPrivate {
    pub(crate) fn reset_for_new_hand(&mut self,) {
        self.is_active = self.chips > Chips::ZERO;
        self.has_button = false;
        self.bet = Chips::ZERO;
        self.action = PlayerAction::None;
        self.hole_cards= PlayerCards::None;
        self.public_cards = PlayerCards::None;
    }
}

impl PlayerPrivate {
    const fn new(id: PeerId, nickname: String, chips: Chips,) -> Self {
        Self {
            id,
            nickname,
            chips,
            bet: Chips::ZERO,
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

    pub fn place_bet(&mut self, action: PlayerAction, total_bet: Chips,) {
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

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
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

// ────────────────────────────────────────────────────────────────────────────
//  Main table state held by *one* peer
// ────────────────────────────────────────────────────────────────────────────

pub struct InternalTableState {
    // static info ---------------------------------------------------------
    table_id:    TableId,
    game_id: GameId,
    num_seats:   usize,
    key_pair: KeyPair,

    // networking ----------------------------------------------------------
    pub connection: P2pTransport,
    /// UI callback – echoes every locally-generated message without locks.
    pub cb:         Box<dyn FnMut(SignedMessage,) + Send,>,

    // game state ----------------------------------------------------------
    phase:           HandPhase,
    players:         PlayerStateObjects,
    deck:            Deck,
    board: Vec<Card,>,
    current_pot:     Pot,
    action_request:  Option<ActionRequest,>,
    small_blind:     Chips,
    big_blind:       Chips,
    active_player: Option<PlayerPrivate>,
    last_bet: Chips,
    hand_count: usize,
    min_raise: Chips,
    pots: Vec<Pot>,

    // misc ----------------------------------------------------------------
    rng:              StdRng,
    new_hand_start_timer: Option<Instant,>,
    new_hand_timeout: Duration,
    listen_addr: Option<Multiaddr>,
    listener_id: Option<String>,
}

impl InternalTableState {
    const START_GAME_SB: Chips = Chips::ZERO;
    const START_GAME_BB: Chips = Chips::ZERO;
    const ACTION_TIMEOUT_MILLIS: u64 = 1500;
    /// grab an immutable snapshot for the GUI
    #[must_use]
    pub fn snapshot(&self,) -> GameState {
        GameState {
            table_id:     self.table_id,
            seats:        self.num_seats,
            game_started: !matches!(self.phase, HandPhase::WaitingForPlayers),

            player_id: self.key_pair.clone().peer_id(),
            nickname:  String::new(), // Optional: store locally if needed

            players:    self.players.clone(),
            board:      self.board.clone(),
            pot:        self.current_pot.clone(),
            action_req: self.action_request.clone(),
            hand_phase: self.phase.clone(),
            listen_addr: self.listen_addr.clone(),
        }
    }

    #[must_use]
    pub fn secret_key(&self,) -> SecretKey {
        self.key_pair.secret()
    }

    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.key_pair.clone().peer_id()
    }

    #[must_use]
    pub const fn num_seats(&self,) -> usize {
        self.num_seats
    }
    #[must_use]
    pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone().iter().map(|p| p.clone(),).collect()
    }
    #[must_use]
    pub fn board(&self,) -> Vec<Card,> {
        self.board.clone()
    }
    pub fn pot(&self,) -> Pot {
        self.current_pot.clone()
    }

    #[must_use]
    pub const fn table_id(&self,) -> TableId {
        self.table_id
    }
}

impl InternalTableState {
    // — constructors —

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        table_id: TableId,
        num_seats: usize,
        key_pair: KeyPair,
        connection: P2pTransport,
        cb: impl FnMut(SignedMessage,) + Send + 'static,
    ) -> Self {
        Self {
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
            cb: Box::new(cb,),

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
            listener_id: None,
            listen_addr: None,
        }
    }

    // — public helpers (runtime ↔ UI) —

    /// Non-blocking pull used by the runtime loop.
    pub fn try_recv(
        &mut self,
    ) -> Result<SignedMessage, mpsc::error::TryRecvError,> {
        self.connection.rx.receiver.try_recv()
    }

    pub fn try_recv_event(
        &mut self,
    ) -> Result<Message, mpsc::error::TryRecvError,> {
        self.connection.rx.event_receiver.try_recv()
    }

    /// Sign, broadcast **and** echo a local command.
    pub fn sign_and_send(
        &mut self,
        msg: Message,
    ) -> Result<(), mpsc::error::TrySendError<SignedMessage,>,> {
        let signed = SignedMessage::new(&self.key_pair, msg,);
        self.send(signed,)
    }

    /// Low-level send used by `sign_and_send` **and** by the gossip handler.
    pub fn send(
        &mut self,
        msg: SignedMessage,
    ) -> Result<(), mpsc::error::TrySendError<SignedMessage,>,> {
        info!("{} sending {}", self.peer_id(), msg.message().label());
        // network → gossipsub
        self.connection.tx.sender.try_send(msg.clone(),)?;
        // local echo → UI
        (self.cb)(msg,);
        Ok((),)
    }

    /// Called every ~20 ms by the runtime.
    pub async fn tick(&mut self,) {
        if let Some(p,) =
            self.players.iter_mut().find(|p| p.action_timer.is_some(),)
        {
            if p.action_timer.unwrap() >= 1_500{
                p.fold();
            }
        }
    }

    pub fn try_join(&mut self, id: PeerId, nickname: &String, chips: &Chips,) {
        info!("entered try join function");
        if self.players().iter().any(|p| p.id == id,) {
            info!("player already joined");
            return;
        }

        // if we are not the player sending a join request, then send a join
        // request for ourselves to that player
        if self.peer_id() != id {
            info!("id of joined player is different from our own");
            if let Some(us,) =
                self.players().iter().find(|p| p.id == self.peer_id(),)
            {
                info!("sending join request from us to newly joined player...");
                let rq = Message::PlayerJoinTableRequest {
                    table_id:  self.table_id,
                    player_id: us.id,
                    nickname: nickname.clone(),
                    chips:     us.chips,
                };
                let res = self.sign_and_send(rq,);
                if let Err(e,) = res {
                    warn!("error sending message: {e}");
                }
            } else {
                info!("NOT sending join request from existing player to newly joined player...");
            }
        } else {
            info!("joined player id equals our own player id...");
        }

        self.players.add(PlayerPrivate::new(id, nickname.clone(), *chips,),);
        info!("added new player to players list...");

        // send confirmation that this player has joined our table.
        info!("preparing confirmation message...");
        let confirmation = Message::PlayerJoinedConfirmation {
            table_id:  self.table_id,
            player_id: id,
            chips:     *chips,
            nickname:  nickname.clone(),
        };

        info!("sending confirmation message...");
        let res = self.sign_and_send(confirmation,);
        if let Err(e,) = res {
            warn!("error: {e}");
        }


        if self.players().len() >= self.num_seats {
            info!("enough players joined the table, starting game");
            let _ = self.enter_start_game();
        } else {
            info!("not enough players joined the table, joined: {}, required: {}", self.players().len(), self.num_seats);
        }
    }


    pub fn handle_event(&mut self, m: Message) {
        match m {
            Message::NewListenAddr {listener_id, multiaddr} => {
                self.listener_id = Some(listener_id);
                self.listen_addr = Some(multiaddr);

            }
            _ => {}
        }
    }

    // — dispatcher called by runtime for every inbound network msg —

    pub fn handle_message(&mut self, sm: SignedMessage,) {
        info!(
            "internal table state of peer {} handling message with label {:?} sent from peer {}",
            self.peer_id(),
            sm.message().label(),
            sm.sender()
        );
        match sm.message() {
            Message::PlayerJoinTableRequest{
                player_id,
                nickname,
                chips,
                ..
            } => self.try_join(player_id.clone(), nickname, chips,),
            Message::PlayerLeaveRequest { player_id, .. } => {
                self.players().retain(|p| &p.id != player_id,);
            },
            Message::GameStateUpdate {
                board,
                pot,
                player_updates,
                ..
            } => {
                self.board = board.clone();
                self.current_pot.total_chips = pot.total_chips;
                self.update_players(player_updates,);
            },
            Message::PlayerJoinedConfirmation {
                table_id: _table_id,
                player_id,
                chips,
                nickname,
            } => self.try_join(player_id.clone(), nickname, chips,),
            other => warn!("unhandled msg in InternalTableState: {other}"),
        }
    }

    #[must_use]
    pub fn action_request(&self,) -> Option<ActionRequest,> {
        self.action_request.clone()
    }

    pub fn reset_action_request(&mut self,) {
        self.action_request = None;
    }

    fn update_players(&mut self, updates: &[PlayerUpdate]) {
        for update in updates {
            if let Some(pos) = self
                .players().iter()
                .position(|p| p.id == update.player_id)
            {
                let player = &mut self.players()[pos];
                player.chips = update.chips;
                player.bet = update.bet;
                player.action = update.action;
                player.action_timer = update.action_timer;
                player.has_button = update.is_dealer;
                player.is_active = update.is_active;

                // Do not override cards for the local player as they are updated
                // when we get a DealCards message.
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
        self.check_action(PlayerAction::Call,)
    }

    /// Check if a check action is in the request.
    #[must_use]
    pub fn can_check(&self,) -> bool {
        self.check_action(PlayerAction::Check,)
    }

    /// Check if a bet action is in the request.
    #[must_use]
    pub fn can_bet(&self,) -> bool {
        self.check_action(PlayerAction::Bet,)
    }

    /// Check if a raise action is in the request.
    #[must_use]
    pub fn can_raise(&self,) -> bool {
        self.check_action(PlayerAction::Raise,)
    }

    fn check_action(&self, action: PlayerAction,) -> bool {
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

    pub player_id: PeerId, // local player
    pub nickname:  String,

    pub players:    PlayerStateObjects,
    pub board:      Vec<Card,>,
    pub pot:        Pot,
    pub action_req: Option<ActionRequest,>,
    pub hand_phase: HandPhase,
    pub listen_addr: Option<Multiaddr>,
}

impl GameState {
    #[must_use]
    pub fn new(peer_id: PeerId, table_id: TableId, nickname: String, seats: usize) -> Self {
        Self {
            table_id,
            seats,
            game_started: false,
            player_id:    peer_id,
            nickname,
            players:      PlayerStateObjects::default(),
            board:        Vec::default(),
            pot:          Pot::default(),
            action_req:   None,
            hand_phase: HandPhase::StartingGame,
            listen_addr: None,
        }
    }

    pub fn default() -> Self {
        GameState {
            table_id: TableId::new_id(),
            seats: 0,
            game_started: false,
            player_id: PeerId::default(),
            players: PlayerStateObjects::default(),
            nickname: String::default(),
            board: Vec::default(),
            pot: Pot::default(),
            action_req: None,
            hand_phase: HandPhase::StartingGame,
            listen_addr: None,
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

    pub fn players(&self) -> Vec<PlayerPrivate> {
        self.players.players()
    }

    pub fn player_id(&self) -> String {
        self.player_id.to_string()
    }
}

impl InternalTableState {
    fn enter_start_game(&mut self,) {
        self.phase = HandPhase::StartingGame;

        // Shuffle seats before starting the game.
        self.players.shuffle_seats(&mut self.rng,);

        // Tell players to update their seats order.
        let seats = self.players.iter().map(|p| p.id.clone(),).collect();

        let _ = self.sign_and_send(Message::StartGameNotify {
            seat_order: seats,
            table_id:   self.table_id,
            game_id:    GameId::new_id(),
        },);

        self.enter_start_hand();
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
        if let Some(p) = &mut self.active_player {
            p.place_bet(PlayerAction::SmallBlind, self.small_blind);
        }

        // Pay small and big blind.
        if let Some(p) = &mut self.active_player {
            p.place_bet(PlayerAction::BigBlind, self.big_blind);
        }

        self.last_bet = self.big_blind;
        self.min_raise = self.big_blind;

        // Create a new deck.
        self.deck = Deck::shuffled(&mut self.rng,);

        // Clear board.
        self.board.clear();

        // Reset pots.
        self.pots = vec![Pot::default()];

        // Tell clients to prepare for a new hand.
        let _ = self.sign_and_send(Message::StartHand {table_id: self.table_id, game_id: self.game_id},);

        // Deal cards to each player.
        for player in self.players.iter_mut() {
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
                player.hole_cards= PlayerCards::None;
                player.public_cards= PlayerCards::None;
            }
        }

        // Tell clients to update all players state.
        let _ = self.broadcast_game_update();

        // Deal the cards to each player.
        for player in self.players() {
            if let PlayerCards::Cards(c1, c2,) = player.hole_cards {
                let msg = Message::DealCards{card1: c1, card2: c2, player_id: player.id, table_id: self.table_id, game_id: self.game_id};
                let _ = self.sign_and_send(msg);
            }
        }

        self.enter_preflop_betting();
    }

    fn enter_preflop_betting(&mut self,) {
        self.phase = HandPhase::PreflopBetting;
        let _ = self.action_update();
    }

    fn action_update(&mut self) {
        self.players.activate_next_player();
        let _ = self.broadcast_game_update();

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

        for player in self.players.iter_mut() {
            player.action = PlayerAction::None;
            if player.is_active {
                player.public_cards = player.hole_cards;
            }
        }

        self.enter_end_hand();
    }

    fn enter_end_hand(&mut self,) {
        self.new_hand_timeout =
            if matches!(self.phase, HandPhase::Showdown) {
                // If coming from a showdown give players more time to see the
                // winning hand and chips.
                Duration::from_millis(7_000,)
            } else {
                Duration::from_millis(3_000,)
            };

        self.phase = HandPhase::EndingHand;

        self.update_pots();
        let _ = self.broadcast_game_update();

        // Give time to the UI to look at the updated pot and board.
        let _ = self.send_throttle(Duration::from_millis(1_000,),);

        let winners = self.pay_bets();

        // Update players and broadcast update to all players.
        self.players.end_hand();
        let _ = self.sign_and_send(Message::EndHand{
            table_id: self.table_id,
            game_id: self.game_id,
            payoffs: winners,
            board:   self.board.clone(),
            cards:   self
                .players
                .iter()
                .map(|p| (p.id.clone(), p.public_cards,),)
                .collect(),
        },);

        // End game if only player has chips or move to next hand.
        if self.players.count_with_chips() < 2 {
            let _ = self.enter_end_game();
        } else {
            // All players that run out of chips must leave the table before the
            // start of a new hand.
            for player in self.players() {
                if player.chips == Chips::ZERO {
                    // Notify the client that this player has left the table.
                    let _ = self.sign_and_send(Message::PlayerLeaveRequest {player_id: player.id, table_id: self.table_id},);
                }
            }

            self.players.remove_with_no_chips();
            self.new_hand_start_timer = Some(Instant::now(),);
        }
    }

    fn enter_end_game(&mut self,) {
        // Give time to the UI to look at winning results before ending the
        // game.
        self.broadcast_throttle(Duration::from_millis(4500,),);

        self.phase = HandPhase::EndingGame;

        for player in self.players() {
            // Pay the winning player.
            let _= self.credit_player(player.id.clone(), player.chips,);

            // Notify the client that this player has left the table.
            let msg = Message::PlayerLeftTable {
                peer_id: player.id,
                game_id: self.game_id,
                table_id: self.table_id,
            };
            let _ = self.sign_and_send(msg.clone());
        }

        self.players.clear();

        // Reset hand count for next game.
        self.hand_count = 0;

        // Wait for players to join.
        self.phase = HandPhase::WaitingForPlayers;
    }

    fn credit_player(&mut self, player_id: PeerId, chips: Chips) {
        assert!(chips >= Chips::ZERO);
        let new_balance = self.players.get(&player_id).unwrap().chips + chips;
        let msg = Message::AccountBalanceUpdate {
            player_id,
            chips: new_balance,
        };
        let _ = self.sign_and_send(msg);
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
                if let Some(player,) = &mut self.active_player{
                    for pot in self.pots.clone() {
                        player.chips += pot.total_chips.clone();

                        if let Some(payoff,) = payoffs
                            .iter_mut()
                            .find(|po| po.player_id == player.id,)
                        {
                            payoff.chips += pot.total_chips;
                        } else {
                            payoffs.push(HandPayoff {
                                player_id: player.id,
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
                    let mut hands = self
                        .players
                        .iter_mut()
                        .filter(|p| {
                            p.is_active && pot.participants.contains(&p.id,)
                        },)
                        .filter_map(|p| {
                            match p.hole_cards {
                                PlayerCards::None | PlayerCards::Covered => {
                                    None
                                },
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
                        cards.sort_by_key(|c| c.rank(),);

                        // If a player has already a payoff add chips to that
                        // one.
                        if let Some(payoff,) = payoffs
                            .iter_mut()
                            .find(|po| po.player_id == player.id,)
                        {
                            payoff.chips += player_payoff;
                        } else {
                            payoffs.push(HandPayoff {
                                player_id: player.id.clone(),
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

        for player in self.players.iter() {
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

        for player in self.players.iter() {
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
                HandPhase::PreflopBetting => {self.enter_deal_flop();},
                HandPhase::TurnBetting => {self.enter_deal_river();},
                HandPhase::FlopBetting => {self.enter_deal_turn();},
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
        self.broadcast_throttle(Duration::from_millis(1000,),);

        for player in self.players.iter_mut() {
            player.bet = Chips::ZERO;
            player.action = PlayerAction::None;
        }

        self.min_raise = self.big_blind;

        self.players.start_round();

        let _ = self.broadcast_game_update();
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
                for player in self.players.iter_mut() {
                    let pot = self.pots.last_mut().unwrap();
                    if player.bet > Chips::ZERO {
                        player.bet -= min_bet;
                        pot.total_chips += min_bet;

                        if !pot.participants.contains(&player.id,) {
                            pot.participants.insert(player.id.clone(),);
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

    /// Broadcast a game state update to all connected players.
    async fn broadcast_game_update(&mut self,) {
        let players = self
            .players
            .iter()
            .map(|p| {
                let action_timer = p.action_timer.map(|t| {
                    Self::ACTION_TIMEOUT_MILLIS
                        .saturating_sub(t,)
                },);

                PlayerUpdate {
                    player_id: p.id.clone(),
                    chips: p.chips,
                    bet: p.bet,
                    action: p.action,
                    action_timer,
                    hole_cards: p.hole_cards,
                    is_dealer: p.has_button,
                    is_active: p.is_active,
                }
            },)
            .collect();

        let chips = self
            .pots
            .iter()
            .map(|p| p.total_chips,)
            .fold(Chips::ZERO, |acc, c| acc + c,);

        self.current_pot.total_chips = chips;

        let msg = Message::GameStateUpdate{
            table_id: self.table_id,
            game_id: self.game_id,
            player_updates: players,
            board: self.board.clone(),
            pot: self.current_pot.clone(),
        };
        for _ in self.players() {
            let _ = self.sign_and_send(msg.clone());
        }
    }

    /// Request action to the active player.
    async fn request_action(&mut self,) {
        if let Some(player,) = &mut self.active_player {
            let mut actions = vec![PlayerAction::Fold];

            if player.bet == self.last_bet{
                actions.push(PlayerAction::Check,);
            }

            if player.bet < self.last_bet {
                actions.push(PlayerAction::Call,);
            }

            if self.last_bet == Chips::ZERO && player.chips > Chips::ZERO {
                actions.push(PlayerAction::Bet,);
            }

            if player.chips + player.bet > self.last_bet
                && self.last_bet > Chips::ZERO
                && player.chips > Chips::ZERO
            {
                actions.push(PlayerAction::Raise,);
            }

            player.action_timer = Some(0);

            let msg = Message::ActionRequest {
                game_id: self.game_id,
                table_id: self.table_id,
                player_id: player.id.clone(),
                min_raise: self.min_raise + self.last_bet,
                big_blind: self.big_blind,
                actions,
            };

            let _ = self.sign_and_send(msg,);
        }
    }

    /// Broadcast a throttle message to all players at the table.
    fn broadcast_throttle(&mut self, dt: Duration,) {
        for _ in self.players() {
            let _ = self.send_throttle(dt,);
        }
    }

    async fn send_throttle(&mut self, duration: Duration) {
        let msg = Message::Throttle { duration };
        let _ = self.sign_and_send(msg);
    }

}
