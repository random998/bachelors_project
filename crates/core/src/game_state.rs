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
use crate::poker::{Card, Chips, Deck, GameId, PlayerCards, TableId}; // per-player helper

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
    players:              PlayerStateObjects,
    deck:                 Deck,
    community_cards:      Vec<Card,>,
    current_pot:          Pot,
    action_request:       Option<ActionRequest,>,
    small_blind:          Chips,
    big_blind:            Chips,

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
        self.players.clone().iter().map(|p| p.clone()).collect()
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
            players: PlayerStateObjects::default(),
            deck: Deck::default(),
            community_cards: Vec::new(),
            current_pot: Pot::default(),
            small_blind: Chips::default(),
            big_blind: Chips::default(),

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
                self.players().iter().find(|p| p.id == self.player_id,)
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
            .add(PlayerPrivate::new(*id, nickname.clone(), *chips,),);

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
                nickname:  player.nickname,
            };
            let res = self.sign_and_send(msg,);
            if let Err(e,) = res {
                warn!("error when trying to send message: {e}");
            }
        }

        if self.players().len() >= self.num_seats {
            self.enter_start_game();
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
                self.players().retain(|p| &p.id != player_id,);
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
                    self.players.add(PlayerPrivate::new(
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

    pub players:    PlayerStateObjects,
    pub board:      Vec<Card,>,
    pub pot:        Pot,
    pub action_req: Option<ActionRequest,>,
}

impl GameState {
    #[must_use]
    pub fn default() -> Self {
        Self {
            table_id: TableId::new_id(),
            seats: 0,
            game_started: false,
            player_id: PeerId::default(),
            nickname: String::default(),
            players: PlayerStateObjects::default(),
            board: Vec::default(),
            pot: Pot::default(),
            action_req: None,
        }
    }

    #[must_use]
    pub fn action_req(&self, ) -> Option<ActionRequest, > {
        self.action_req.clone()
    }

    pub fn pot(&mut self, ) -> Pot {
        self.pot.clone()
    }

    pub const fn pot_chips(&mut self, ) -> Chips {
        self.pot.total_chips
    }
}

impl InternalTableState {

    async fn enter_start_game(&mut self) {
        self.phase = HandPhase::StartingGame;

        // Shuffle seats before starting the game.
        self.players.shuffle_seats(&mut self.rng);

        // Tell players to update their seats order.
        let seats = self.players.iter().map(|p| p.id.clone()).collect();
        self.broadcast_message(Message::StartGameNotify {seat_order: seats, table_id: self.table_id, game_id: GameId::new_id()}).await;

        self.enter_start_hand().await;
    }

/// Start a new hand.
async fn enter_start_hand(&mut self) {
    self.phase = HandPhase::StartingHand;

    self.players.start_hand(&mut self.rng);

    // If there are fewer than 2 active players end the game.
    if self.players.count_active() < 2 {
        self.enter_end_game().await;
        return;
    }

    self.update_blinds();

    // Pay small and big blind.
    if let Some(player) = self.players.active_player() {
        player.bet(PlayerAction::SmallBlind, self.small_blind);
    };

    self.players.activate_next_player();

    if let Some(player) = self.players.active_player() {
        player.bet(PlayerAction::BigBlind, self.big_blind);
    };

    self.last_bet = self.big_blind;
    self.min_raise = self.big_blind;

    // Create a new deck.
    self.deck = Deck::shuffled(&mut self.rng);

    // Clear board.
    self.board.clear();

    // Reset pots.
    self.pots = vec![Pot::default()];

    // Tell clients to prepare for a new hand.
    self.broadcast_message(Message::StartHand).await;

    // Deal cards to each player.
    for player in self.players.iter_mut() {
        if player.is_active {
            player.public_cards = PlayerCards::Covered;

            // Sort cards for the UI.
            let (c1, c2) = (self.deck.deal(), self.deck.deal());
            player.hole_cards = if c1.rank() < c2.rank() {
                PlayerCards::Cards(c1, c2)
            } else {
                PlayerCards::Cards(c2, c1)
            };
        } else {
            player.public_cards = PlayerCards::None;
            player.hole_cards = PlayerCards::None;
        }
    }

    // Tell clients to update all players state.
    self.broadcast_game_update().await;

    // Deal the cards to each player.
    for player in self.players.iter() {
        if let PlayerCards::Cards(c1, c2) = player.hole_cards {
            let msg = Message::DealCards(c1, c2);
            let smsg = SignedMessage::new(&self.sk, msg);
            player.send_message(smsg).await;
        }
    }

    self.enter_preflop_betting().await;
}

async fn enter_preflop_betting(&mut self) {
    self.hand_state = HandState::PreflopBetting;
    self.action_update().await;
}

async fn enter_deal_flop(&mut self) {
    for _ in 1..=3 {
        self.board.push(self.deck.deal());
    }

    self.hand_state = HandState::FlopBetting;
    self.start_round().await;
}

async fn enter_deal_turn(&mut self) {
    self.board.push(self.deck.deal());

    self.hand_state = HandState::TurnBetting;
    self.start_round().await;
}

async fn enter_deal_river(&mut self) {
    self.board.push(self.deck.deal());

    self.hand_state = HandState::RiverBetting;
    self.start_round().await;
}

async fn enter_showdown(&mut self) {
    self.hand_state = HandState::Showdown;

    for player in self.players.iter_mut() {
        player.action = PlayerAction::None;
        if player.is_active {
            player.public_cards = player.hole_cards;
        }
    }

    self.enter_end_hand().await;
}

async fn enter_end_hand(&mut self) {
    self.new_hand_timeout = if matches!(self.hand_state, HandState::Showdown) {
        // If coming from a showdown give players more time to see the winning
        // hand and chips.
        Duration::from_millis(7_000)
    } else {
        Duration::from_millis(3_000)
    };

    self.hand_state = HandState::EndHand;

    self.update_pots();
    self.broadcast_game_update().await;

    // Give time to the UI to look at the updated pot and board.
    self.broadcast_throttle(Duration::from_millis(1_000)).await;

    let winners = self.pay_bets();

    // Update players and broadcast update to all players.
    self.players.end_hand();
    self.broadcast_message(Message::EndHand {
        payoffs: winners,
        board: self.board.clone(),
        cards: self
            .players
            .iter()
            .map(|p| (p.player_id.clone(), p.public_cards))
            .collect(),
    })
        .await;

    // End game if only player has chips or move to next hand.
    if self.players.count_with_chips() < 2 {
        self.enter_end_game().await;
    } else {
        // All players that run out of chips must leave the table before the
        // start of a new hand.
        for player in self.players.iter() {
            if player.chips == Chips::ZERO {
                // Notify the client that this player has left the table.
                let _ = player.table_tx.send(TableMessage::PlayerLeft).await;

                let msg = Message::PlayerLeft(player.player_id.clone());
                self.broadcast_message(msg).await;
            }
        }

        self.players.remove_with_no_chips();
        self.new_hand_timer = Some(Instant::now());
    }
}

async fn enter_end_game(&mut self) {
    // Give time to the UI to look at winning results before ending the game.
    self.broadcast_throttle(Duration::from_millis(4500)).await;

    self.hand_state = HandState::EndGame;

    for player in self.players.iter() {
        // Pay the winning player.
        let res = self
            .db
            .pay_to_player(player.player_id.clone(), player.chips)
            .await;
        if let Err(e) = res {
            error!("Db players update failed {e}");
        }

        // Notify the client that this player has left the table.
        let _ = player.table_tx.send(TableMessage::PlayerLeft).await;
    }

    self.players.clear();

    // Reset hand count for next game.
    self.hand_count = 0;

    // Wait for players to join.
    self.hand_state = HandState::WaitForPlayers;
}

fn update_blinds(&mut self) {
    let multiplier = (1 << (self.hand_count / 4).min(4)) as u32;
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

fn pay_bets(&mut self) -> Vec<HandPayoff> {
    let mut payoffs = Vec::<HandPayoff>::new();

    match self.players.count_active() {
        1 => {
            // If one player left gets all the chips.
            if let Some(player) = self.players.active_player() {
                for pot in self.pots.drain(..) {
                    player.chips += pot.chips;

                    if let Some(payoff) = payoffs
                        .iter_mut()
                        .find(|po| po.player_id == player.player_id)
                    {
                        payoff.chips += pot.chips;
                    } else {
                        payoffs.push(HandPayoff {
                            player_id: player.player_id.clone(),
                            chips: pot.chips,
                            cards: Vec::default(),
                            rank: String::default(),
                        });
                    }
                }
            }
        }
        n if n > 1 => {
            // With more than 1 active player we need to compare hands for each pot
            for pot in self.pots.drain(..) {
                // Evaluate all active players hands.
                let mut hands = self
                    .players
                    .iter_mut()
                    .filter(|p| p.is_active && pot.players.contains(&p.player_id))
                    .filter_map(|p| match p.hole_cards {
                        PlayerCards::None | PlayerCards::Covered => None,
                        PlayerCards::Cards(c1, c2) => Some((p, c1, c2)),
                    })
                    .map(|(p, c1, c2)| {
                        let mut cards = vec![c1, c2];
                        cards.extend_from_slice(&self.board);
                        let (v, bh) = HandValue::eval_with_best_hand(&cards);
                        (p, v, bh)
                    })
                    .collect::<Vec<_>>();

                // This may happen when the last pot is empty.
                if hands.is_empty() {
                    continue;
                }

                // Sort descending order, winners first.
                hands.sort_by(|p1, p2| p2.1.cmp(&p1.1));

                // Count hands with the same value.
                let winners_count = hands.iter().filter(|(_, v, _)| v == &hands[0].1).count();
                let win_payoff = pot.chips / winners_count as u32;
                let win_remainder = pot.chips % winners_count as u32;

                for (idx, (player, v, bh)) in hands.iter_mut().take(winners_count).enumerate() {
                    // Give remaineder to first player.
                    let player_payoff = if idx == 0 {
                        win_payoff + win_remainder
                    } else {
                        win_payoff
                    };

                    player.chips += player_payoff;

                    // Sort by rank for the UI.
                    let mut cards = bh.to_vec();
                    cards.sort_by_key(|c| c.rank());

                    // If a player has already a payoff add chips to that one.
                    if let Some(payoff) = payoffs
                        .iter_mut()
                        .find(|po| po.player_id == player.player_id)
                    {
                        payoff.chips += player_payoff;
                    } else {
                        payoffs.push(HandPayoff {
                            player_id: player.player_id.clone(),
                            chips: player_payoff,
                            cards,
                            rank: v.rank().to_string(),
                        });
                    }
                }
            }
        }
        _ => {}
    }

    payoffs
}

/// Checks if all players in the hand have acted.
fn is_round_complete(&self) -> bool {
    if self.players.count_active() < 2 {
        return true;
    }

    for player in self.players.iter() {
        // If a player didn't match the last bet and is not all-in then the
        // player has to act and the round is not complete.
        if player.is_active && player.bet < self.last_bet && player.chips > Chips::ZERO {
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
                PlayerAction::None | PlayerAction::SmallBlind | PlayerAction::BigBlind
                if player.chips > Chips::ZERO =>
                    {
                        return false;
                    }
                _ => {}
            }
        }
    }

    true
}

async fn next_round(&mut self) {
    if self.players.count_active() < 2 {
        self.enter_end_hand().await;
        return;
    }

    while self.is_round_complete() {
        match self.hand_state {
            HandState::PreflopBetting => self.enter_deal_flop().await,
            HandState::FlopBetting => self.enter_deal_turn().await,
            HandState::TurnBetting => self.enter_deal_river().await,
            HandState::RiverBetting => {
                self.enter_showdown().await;
                return;
            }
            _ => {}
        }
    }
}

async fn start_round(&mut self) {
    self.update_pots();

    // Give some time to watch last action and pots.
    self.broadcast_throttle(Duration::from_millis(1000)).await;

    for player in self.players.iter_mut() {
        player.bet = Chips::ZERO;
        player.action = PlayerAction::None;
    }

    self.last_bet = Chips::ZERO;
    self.min_raise = self.big_blind;

    self.players.start_round();

    self.broadcast_game_update().await;
    self.request_action().await;
}

fn update_pots(&mut self) {
    // Updates pots if there is a bet.
    if self.last_bet > Chips::ZERO {
        // Move bets to pots.
        loop {
            // Find minimum bet in case a player went all in.
            let min_bet = self
                .players
                .iter()
                .filter(|p| p.bet > Chips::ZERO)
                .map(|p| p.bet)
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
                    pot.chips += min_bet;

                    if !pot.players.contains(&player.player_id) {
                        pot.players.insert(player.player_id.clone());
                    }

                    went_all_in = player.chips == Chips::ZERO;
                }
            }

            if went_all_in {
                self.pots.push(Pot::default());
            }
        }
    }
}

/// Broadcast a game state update to all connected players.
async fn broadcast_game_update(&self) {
    let players = self
        .players
        .iter()
        .map(|p| {
            let action_timer = p.action_timer.map(|t| {
                Self::ACTION_TIMEOUT
                    .saturating_sub(t.elapsed())
                    .as_secs_f32() as u16
            });

            PlayerUpdate {
                player_id: p.player_id.clone(),
                chips: p.chips,
                bet: p.bet,
                action: p.action,
                action_timer,
                cards: p.public_cards,
                has_button: p.has_button,
                is_active: p.is_active,
            }
        })
        .collect();

    let pot = self
        .pots
        .iter()
        .map(|p| p.chips)
        .fold(Chips::ZERO, |acc, c| acc + c);

    let msg = Message::GameUpdate {
        players,
        board: self.board.clone(),
        pot,
    };
    let smsg = SignedMessage::new(&self.sk, msg);
    for player in self.players.iter() {
        player.send_message(smsg.clone()).await;
    }
}

/// Request action to the active player.
async fn request_action(&mut self) {
    if let Some(player) = self.players.active_player() {
        let mut actions = vec![PlayerAction::Fold];

        if player.bet == self.last_bet {
            actions.push(PlayerAction::Check);
        }

        if player.bet < self.last_bet {
            actions.push(PlayerAction::Call);
        }

        if self.last_bet == Chips::ZERO && player.chips > Chips::ZERO {
            actions.push(PlayerAction::Bet);
        }

        if player.chips + player.bet > self.last_bet
            && self.last_bet > Chips::ZERO
            && player.chips > Chips::ZERO
        {
            actions.push(PlayerAction::Raise);
        }

        player.action_timer = Some(Instant::now());

        let msg = Message::ActionRequest {
            player_id: player.player_id.clone(),
            min_raise: self.min_raise + self.last_bet,
            big_blind: self.big_blind,
            actions,
        };

        self.broadcast_message(msg).await;
    }
}

/// Broadcast a message to all players at the table.
async fn broadcast_message(&self, msg: Message) {
    let smsg = SignedMessage::new(&self.sk, msg);
    for player in self.players.iter() {
        player.send_message(smsg.clone()).await;
    }
}

/// Broadcast a throttle message to all players at the table.
async fn broadcast_throttle(&self, dt: Duration) {
    for player in self.players.iter() {
        player.send_throttle(dt).await;
    }
}
}
