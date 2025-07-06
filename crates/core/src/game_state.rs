//! Poker table state management – **lock-free** client version
//! Adapted from <https://github.com/vincev/freezeout>

use std::fmt;
use std::fmt::Formatter;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use log::{info, warn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::crypto::{PeerId, SigningKey};
use crate::message::{
    HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage,
};
use crate::net::traits::P2pTransport;
use crate::players_state::PlayerStateObjects;
use crate::poker::{Card, Chips, Deck, PlayerCards, TableId}; // per-player helper

// ────────────────────────────────────────────────────────────────────────────
//  Small helpers
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize,)]
pub(crate) struct Pot {
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
#[derive(Debug, Clone, Serialize, Deserialize,)]
pub struct PlayerPrivate {
    pub id:           PeerId,
    pub nickname:     String,
    pub chips:        Chips,
    pub bet:          Chips,
    pub payoff:       Option<HandPayoff,>,
    pub action:       PlayerAction,
    pub action_timer: Option<u64,>,
    pub cards:        PlayerCards,
    pub has_button:   bool,
    pub is_active:    bool,
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
        self.cards = PlayerCards::None;
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
            cards: PlayerCards::None,
            has_button: false,
            is_active: true,
        }
    }

    pub const fn fold(&mut self,) {
        self.is_active = false;
        self.action = PlayerAction::Fold;
    }

    #[must_use]
    pub fn has_chips(&self,) -> bool {
        self.chips > Chips::ZERO
    }
}

#[derive(Debug, Eq, PartialEq,)]
enum HandPhase {
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
    num_seats:   usize,
    signing_key: Arc<SigningKey,>,
    player_id:   PeerId,

    // networking ----------------------------------------------------------
    pub connection: P2pTransport,
    /// UI callback – echoes every locally-generated message without locks.
    pub cb:         Box<dyn FnMut(SignedMessage,) + Send,>,

    // game state ----------------------------------------------------------
    phase:                HandPhase,
    players:              Vec<PlayerPrivate,>,
    player_state_objects: PlayerStateObjects,
    deck:                 Deck,
    community_cards:      Vec<Card,>,
    current_pot:          Pot,
    action_request:       Option<ActionRequest,>,

    // misc ----------------------------------------------------------------
    rng:              StdRng,
    hand_start_timer: Option<Instant,>,
    hand_start_delay: Duration,
}

impl InternalTableState {
    /// grab an immutable snapshot for the GUI
    #[must_use]
    pub fn snapshot(&self,) -> GameState {
        GameState {
            table_id:     self.table_id,
            seats:        self.num_seats,
            game_started: !matches!(self.phase, HandPhase::WaitingForPlayers),

            player_id: self.signing_key.peer_id(),
            nickname:  String::new(), // Optional: store locally if needed

            players:    self.players.clone(),
            board:      self.community_cards.clone(),
            pot:        self.current_pot.clone(),
            action_req: self.action_request.clone(),
        }
    }

    #[must_use]
    pub fn signing_key(&self,) -> Arc<SigningKey,> {
        self.signing_key.clone()
    }

    #[must_use]
    pub const fn num_seats(&self,) -> usize {
        self.num_seats
    }
    #[must_use]
    pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone()
    }
    #[must_use]
    pub fn community_cards(&self,) -> Vec<Card,> {
        self.community_cards.clone()
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

impl InternalTableState {
    // — constructors —

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        peer_id: PeerId,
        table_id: TableId,
        num_seats: usize,
        signing_key: Arc<SigningKey,>,
        connection: P2pTransport,
        cb: impl FnMut(SignedMessage,) + Send + 'static,
    ) -> Self {
        Self {
            player_id: peer_id,
            table_id,
            num_seats,
            signing_key,
            connection,
            cb: Box::new(cb,),

            phase: HandPhase::WaitingForPlayers,
            players: Vec::new(),
            player_state_objects: PlayerStateObjects::default(),
            deck: Deck::default(),
            community_cards: Vec::new(),
            current_pot: Pot::default(),

            rng: StdRng::from_os_rng(),
            hand_start_timer: None,
            hand_start_delay: Duration::from_millis(1_000,),
            action_request: None,
        }
    }

    // — public helpers (runtime ↔ UI) —

    /// Non-blocking pull used by the runtime loop.
    pub fn try_recv(
        &mut self,
    ) -> Result<SignedMessage, mpsc::error::TryRecvError,> {
        self.connection.rx.receiver.try_recv()
    }

    /// Sign, broadcast **and** echo a local command.
    pub fn sign_and_send(
        &mut self,
        msg: Message,
    ) -> Result<(), mpsc::error::TrySendError<SignedMessage,>,> {
        let signed = SignedMessage::new(&self.signing_key, msg,);
        self.send(signed,)
    }

    /// Low-level send used by `sign_and_send` **and** by the gossip handler.
    pub fn send(
        &mut self,
        msg: SignedMessage,
    ) -> Result<(), mpsc::error::TrySendError<SignedMessage,>,> {
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
            if p.action_timer.unwrap() >= 1_500 {
                p.fold();
            }
        }
    }

    pub fn try_join(&mut self, id: &PeerId, nickname: &String, chips: &Chips,) {
        if self.players.iter().any(|p| p.id == *id,) {
            // player already joined.
            return;
        }

        // if we are not the player sending a join request, then send a join
        // request for ourselves to that player
        if self.player_id != *id {
            if let Some(us,) =
                self.players.iter().find(|p| p.id == self.player_id,)
            {
                let rq = Message::PlayerJoinTableRequest {
                    table_id:  self.table_id,
                    player_id: us.id,
                    nickname:  us.nickname.clone(),
                    chips:     us.chips,
                };
                let res = self.sign_and_send(rq,);
                if let Err(e,) = res {
                    warn!("error sending message: {e}");
                }
            }
        }

        // TODO: check if chips amount is allowed, nickname is allowed etc.

        self.players
            .push(PlayerPrivate::new(*id, nickname.clone(), *chips,),);

        // send confirmation that this player has joined our table.
        let confirmation = Message::PlayerJoinedConfirmation {
            table_id:  self.table_id,
            player_id: *id,
            chips:     *chips,
            nickname:  nickname.clone(),
        };
        let res = self.sign_and_send(confirmation,);

        if let Err(e,) = res {
            warn!("error: {e}");
        }

        // since this player has just joined, we need to send a join
        // confirmation message for each other player, such that they join the
        // new players lobby.
        for player in &mut self.players.clone() {
            let msg = Message::PlayerJoinedConfirmation {
                table_id:  self.table_id,
                player_id: player.id,
                chips:     player.chips,
                nickname:  player.nickname.clone(),
            };
            let res = self.sign_and_send(msg,);
            if let Err(e,) = res {
                warn!("error when trying to send message: {e}");
            }
        }
    }

    // — dispatcher called by runtime for every inbound network msg —

    pub fn handle_message(&mut self, sm: SignedMessage,) {
        info!(
            "internal table state of peer {} handling message with label {:?} sent from peer {}",
            self.player_id,
            sm.message().label(),
            sm.sender()
        );
        match sm.message() {
            Message::PlayerJoinTableRequest {
                player_id,
                nickname,
                chips,
                ..
            } => self.try_join(player_id, nickname, chips,),
            Message::PlayerLeaveRequest { player_id, .. } => {
                self.players.retain(|p| &p.id != player_id,);
            },
            Message::GameStateUpdate {
                community_cards,
                pot,
                player_updates,
                ..
            } => {
                self.community_cards = community_cards.clone();
                self.current_pot.total_chips = pot.total_chips;
                self.update_players(player_updates,);
            },
            Message::PlayerJoinedConfirmation {
                table_id: _table_id,
                player_id,
                chips,
                nickname,
            } => {
                // check if player has actually joined
                if !self.players.iter().any(|p| p.id == *player_id,) {
                    self.players.push(PlayerPrivate::new(
                        *player_id,
                        nickname.clone(),
                        *chips,
                    ),);
                    // send confirmation
                    let confirmation = Message::PlayerJoinedConfirmation {
                        table_id:  self.table_id,
                        player_id: *player_id,
                        chips:     *chips,
                        nickname:  nickname.clone(),
                    };
                    let res = self.sign_and_send(confirmation,);
                    if let Err(e,) = res {
                        info!(
                            "internal table state could not send message: {e}"
                        );
                    }
                }
            },
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

    // — internals —

    fn update_players(&mut self, updates: &[PlayerUpdate],) {
        for u in updates {
            if let Some(p,) =
                self.players.iter_mut().find(|p| p.id == u.player_id,)
            {
                p.chips = u.chips;
                p.bet = u.bet;
                p.action = u.action;
                p.action_timer = u.action_timer;
                p.has_button = u.is_dealer;
                p.is_active = u.is_active;
                if p.id != self.signing_key.peer_id() {
                    p.cards = u.hole_cards;
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

    pub players:    Vec<PlayerPrivate,>,
    pub board:      Vec<Card,>,
    pub pot:        Pot,
    pub action_req: Option<ActionRequest,>,
}

impl GameState {
    #[must_use]
    pub fn default() -> Self {
        Self {
            table_id:     TableId::new_id(),
            seats:        0,
            game_started: false,
            player_id:    PeerId::default(),
            nickname:     String::default(),
            players:      Vec::default(),
            board:        Vec::default(),
            pot:          Pot::default(),
            action_req:   None,
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
}
