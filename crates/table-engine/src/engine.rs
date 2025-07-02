//! Poker table state management.
//! Adapted and refactored from https://github.com/vincev/freezeout

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ahash::AHashSet;
use anyhow::Error;
use futures::FutureExt;
use futures::future::BoxFuture;
use log::{error, info};
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::{
    HandPayoff, Message, PlayerAction, PlayerUpdate, SignedMessage,
};
use poker_core::net::NetTx;
use poker_core::net::traits::{P2pTransport, P2pTx};
use poker_core::poker::{Card, Chips, Deck, PlayerCards, TableId};
use poker_eval::HandValue;
use rand::prelude::StdRng;
use rand::{SeedableRng, rng};
use thiserror::Error;

use super::player::Player;
use super::players_state::PlayersState;

pub trait EngineCallbacks: Send + Sync + 'static {
    fn send(&mut self, player: PeerId, msg: SignedMessage,);
    fn throttle(&mut self, player: PeerId, dt: Duration,); // NEW helper
    fn disconnect(&mut self, player: PeerId,);
    fn credit_chips(
        &mut self,
        player: PeerId,
        amount: Chips,
    ) -> Result<(), anyhow::Error,>;
}

/// Represents the current phase of a hand being played.
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
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        match self {
            HandPhase::WaitingForPlayers => write!(f, "WaitingForPlayers"),
            HandPhase::StartingGame => write!(f, "StartingGame"),
            HandPhase::StartingHand => write!(f, "StartingHand"),
            HandPhase::PreflopBetting => write!(f, "PreflopBetting"),
            HandPhase::Preflop => write!(f, "Preflop"),
            HandPhase::FlopBetting => write!(f, "FlopBetting"),
            HandPhase::Flop => write!(f, "Flop"),
            HandPhase::TurnBetting => write!(f, "TurnBetting"),
            HandPhase::Turn => write!(f, "Turn"),
            HandPhase::RiverBetting => write!(f, "RiverBetting"),
            HandPhase::River => write!(f, "River"),
            HandPhase::Showdown => write!(f, "Showdown"),
            HandPhase::EndingHand => write!(f, "EndingHand"),
            HandPhase::EndingGame => write!(f, "EndingGame"),
        }
    }
}

/// Represents a single betting pot.
#[derive(Debug, Default,)]
struct Pot {
    participants: AHashSet<PeerId,>,
    total_chips:  Chips,
}

/// Possible errors when a player attempts to join a table.
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

/// Core state of a poker table instance.
pub struct InternalTableState {
    pub connection:   P2pTransport,
    table_id:    TableId,
    num_seats:   usize,
    signing_key: Arc<SigningKey,>,

    phase:       HandPhase,
    hand_number: usize,

    small_blind: Chips,
    big_blind:   Chips,

    players:         PlayersState,
    deck:            Deck,
    community_cards: Vec<Card,>,

    last_bet:  Chips,
    min_raise: Chips,
    pots:      Vec<Pot,>,

    rng:              StdRng,
    hand_start_timer: Option<Instant,>,
    hand_start_delay: Duration,
}
impl InternalTableState {
    const ACTION_TIMEOUT: Duration = Duration::from_secs(15,);
    const INITIAL_SMALL_BLIND: Chips = Chips::new(10_000,);
    const INITIAL_BIG_BLIND: Chips = Chips::new(20_000,);

    pub fn new(
        transport: P2pTransport,
        table_id: TableId,
        max_seats: usize,
        signing_key: Arc<SigningKey,>,
    ) -> Self {
        let rng = StdRng::from_rng(&mut rng(),);
        Self::with_rng(transport, table_id, max_seats, signing_key, rng,)
    }

    fn with_rng(
        transport: P2pTransport,
        table_id: TableId,
        max_seats: usize,
        signing_key: Arc<SigningKey,>,
        mut rng: StdRng,
    ) -> Self {
        Self {
            connection: transport,
            table_id,
            num_seats: max_seats,
            signing_key,
            phase: HandPhase::WaitingForPlayers,
            hand_number: 0,
            small_blind: Self::INITIAL_SMALL_BLIND,
            big_blind: Self::INITIAL_BIG_BLIND,
            players: PlayersState::default(),
            deck: Deck::shuffled(&mut rng,),
            community_cards: vec![],
            last_bet: Chips::ZERO,
            min_raise: Chips::ZERO,
            pots: vec![Pot::default()],
            rng,
            hand_start_timer: None,
            hand_start_delay: Duration::from_millis(3000,),
        }
    }
    pub fn can_join(&self,) -> bool {
        if !matches!(self.phase, HandPhase::WaitingForPlayers) {
            false
        } else {
            self.players.iter().count() < self.num_seats
        }
    }
    pub async fn try_join(
        &mut self,
        player_id: &PeerId,
        nickname: &str,
        starting_chips: Chips,
    ) -> Result<(), TableJoinError,> {
        if self.players.count() >= self.num_seats {
            return Err(TableJoinError::TableFull,);
        }

        if !matches!(self.phase, HandPhase::WaitingForPlayers) {
            return Err(TableJoinError::GameStarted,);
        }

        if self.players.iter().any(|player| &player.id == player_id,) {
            return Err(TableJoinError::AlreadyJoined,);
        }

        let new_player: Player =
            Player::new(*player_id, nickname.to_string(), starting_chips,);

        let confirmation_message = Message::PlayerJoined {
            table_id:  self.table_id,
            nickname:  nickname.to_string(),
            chips:     starting_chips,
            player_id: *player_id,
        };

        let signed_message =
            SignedMessage::new(&self.signing_key, confirmation_message,);

        let _ = self.connection.tx.send(signed_message,);

        // for each existing player, send a player joined message to the newly
        // joined player.
        for existing_player in self.players.iter() {
            let join_msg = Message::PlayerJoined {
                player_id: existing_player.id,
                nickname:  existing_player.nickname.clone(),
                chips:     existing_player.chips,
                table_id:  self.table_id,
            };

            let signed = SignedMessage::new(&self.signing_key, join_msg,);
            let _ = self.connection.tx.send(signed,);
        }

        info!("Player {player_id} joined table {}", self.table_id);

        // tell all existing players that a new player joined.
        self.sign_and_send(Message::PlayerJoined {
            nickname:  new_player.clone().nickname.to_string(),
            player_id: new_player.clone().id,
            chips:     new_player.clone().chips,
            table_id:  self.table_id,
        },)
            .await;

        self.players.add(new_player.clone(),);

        // if all seats are occupied, start the game.
        if self.players.count() == self.num_seats {
            self.start_game().await;
        }

        Ok((),)
    }
    pub async fn leave(
        &mut self,
        player_id: &PeerId,
    ) -> Result<(), anyhow::Error,> {
        let active_is_leaving = self.players.is_active(player_id,);
        if let Some(leaver,) = self.players.remove(player_id,) {
            // add player bets to the pot.
            if let Some(pot,) = self.pots.last_mut() {
                pot.total_chips += leaver.current_bet;
            }

            if self.players.count_active() < 2 {
                self.enter_end_hand().await;
                return Ok((),);
            }

            if active_is_leaving {
                self.request_action().await;
            }

            let msg = Message::PlayerLeftNotification {
                player_id: *player_id,
            };

            return self.sign_and_send(msg,).await;
        }
        Ok((),)
    }

    pub async fn sign_and_send(&mut self, msg: Message,) -> Result<(), Error,> {
        let signed = SignedMessage::new(&self.signing_key, msg,);
        self.connection.tx.send(signed,).await
    }
    
    pub async fn send(&mut self, msg: SignedMessage,) -> Result<(), Error,> {
        self.connection.tx.send(msg,).await
    }
    
    
    

    /// handle incoming message from a player.
    pub async fn handle_message(&mut self, msg: SignedMessage,) {
        info!("server handling incoming message: {:?}", msg.message());
        println!("server handling incoming message: {:?}", msg.message());
        if let Message::ActionResponse { action, amount, } = msg.message() {
            if let Some(player,) = self.players.active_player() {
                // only process actionResponses incoming from the active player
                if player.id == msg.sender() {
                    player.last_action = *action;
                    player.action_timer = None;

                    match action {
                        PlayerAction::Fold => {
                            player.fold();
                        },
                        PlayerAction::Call => {
                            player.place_bet(*action, self.last_bet,)
                        },
                        PlayerAction::Check => {},
                        PlayerAction::Bet | PlayerAction::Raise => {
                            let amount: Chips = (*amount)
                                .min(player.current_bet + player.chips,);
                            self.min_raise =
                                (amount - self.last_bet).max(self.min_raise,);
                            self.last_bet = amount.max(self.last_bet,);
                        },
                        _ => {},
                    }
                    self.action_update().await;
                }
            }
        }
    }
    pub async fn tick(&mut self,) {
        let mut players = self.players.clone();
        if let Some(player,) = players
            .iter_mut()
            .find(|p| p.action_timer.is_some() && p.active,)
        {
            if player.action_timer.unwrap().elapsed() > Self::ACTION_TIMEOUT {
                player.fold();
                self.action_update().await;
            } else {
                self.broadcast_game_update().await;
            }
        }

        if let Some(timer,) = self.hand_start_timer {
            if timer.elapsed() > self.hand_start_delay {
                self.hand_start_timer = None;
                self.start_hand().await;
            }
        }
    }

    fn action_update(&mut self,) -> BoxFuture<'_, (),> {
        async move {
            self.players.advance_turn();
            self.broadcast_game_update().await;

            if self.is_round_complete() {
                self.next_round().await;
            } else {
                self.request_action().await;
            }
        }
        .boxed() // <─ converts the async block into a BoxFuture
    }
    async fn next_round(&mut self,) {
        if self.players.count_active() < 2 {
            self.enter_end_hand().await;
            return;
        }

        while self.is_round_complete() {
            match self.phase {
                HandPhase::PreflopBetting => self.enter_deal_flop().await,
                HandPhase::FlopBetting => self.enter_deal_turn().await,
                HandPhase::TurnBetting => self.enter_deal_river().await,
                HandPhase::RiverBetting => {
                    self.enter_showdown().await;
                    return;
                },
                _ => {},
            }
        }
    }

    fn is_round_complete(&self,) -> bool {
        if self.players.count_active() < 2 {
            return true;
        }

        // check if all players matched the last bet.
        for player in self.players.iter() {
            if player.active
                && player.current_bet < self.last_bet
                && player.chips > Chips::ZERO
            {
                return false;
            }
        }

        if self.players.count_active_with_chips() < 2 {
            // TODO: do not hardcode constants.
            return true;
        }

        // TODO: refactor for improved readability.
        // if one player did not act, round is not complete.
        for player in self.players.iter() {
            if player.active {
                match player.last_action {
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

    async fn start_game(&mut self,) {
        self.phase = HandPhase::StartingGame;
        self.players.shuffle(&mut self.rng,);
        let seat_order = self.players.iter().map(|p| p.id,).collect();
        let res =self.sign_and_send(Message::StartGame(seat_order,),).await;
        if let Err(e) = res{
            info!("error: {}", e)
        }
        self.start_hand().await;
    }

    /// method for starting one round as pat of a hand.
    async fn start_round(&mut self,) {
        self.update_pots();

        // Give some time to watch last action and pots.
        self.broadcast_throttle(Duration::from_millis(1000,),).await; // TODO: do not hardcode numbers.

        for player in self.players.iter_mut() {
            player.current_bet = Chips::ZERO;
            player.last_action = PlayerAction::None;
        }

        self.last_bet = Chips::ZERO;
        self.min_raise = self.big_blind;

        self.players.start_round();

        self.broadcast_game_update().await;
        self.request_action().await;
    }

    fn update_blinds(&mut self,) {
        let multiplier = (1 << (self.hand_number / 4).min(4,)) as u32;
        if multiplier < 16 {
            self.small_blind = Self::INITIAL_SMALL_BLIND * multiplier;
            self.big_blind = Self::INITIAL_BIG_BLIND * multiplier;
        } else {
            // Cap at 12 times initial blinds.
            self.small_blind = Self::INITIAL_SMALL_BLIND * 12;
            self.big_blind = Self::INITIAL_BIG_BLIND * 12;
        }

        self.hand_number += 1;
    }
    async fn start_hand(&mut self,) {
        self.phase = HandPhase::StartingHand;
        info!("entering {}", self.phase);
        self.players.start_hand();

        if self.players.count_active() < 2 {
            self.enter_end_game().await;
            return;
        }
        self.update_blinds();

        if let Some(player,) = self.players.active_player() {
            player.place_bet(PlayerAction::SmallBlind, self.small_blind,);
        };

        self.players.advance_turn();

        if let Some(player,) = self.players.active_player() {
            player.place_bet(PlayerAction::BigBlind, self.big_blind,);
        }

        self.last_bet = self.big_blind;
        self.min_raise = self.small_blind;

        self.deck = Deck::shuffled(&mut self.rng,);
        self.community_cards.clear();
        self.pots = vec![Pot::default()];
        let res = self.sign_and_send(Message::StartHand,).await;
        if let Err(e) = res {
            info!("error: {}", e);
        } 

        info!("dealing cards to players: {}", self.phase);
        info!("current players list: {}", self.players.clone());
        // deal cards
        for player in self.players.iter_mut() {
            if player.active {
                player.public_cards = PlayerCards::Covered;

                // Sort cards for the UI.
                let (c1, c2,) = (self.deck.deal(), self.deck.deal(),);
                player.private_cards = if c1.rank() < c2.rank() {
                    PlayerCards::Cards(c1, c2,)
                } else {
                    PlayerCards::Cards(c2, c1,)
                };
            } else {
                player.public_cards = PlayerCards::None;
                player.private_cards = PlayerCards::None;
            }
        }

        self.broadcast_game_update().await;

        // send the dealt cards to each player.
        for player in self.players.clone().iter_mut() {
            if let PlayerCards::Cards(c1, c2,) = player.private_cards {
                let msg = Message::DealCards(c1, c2,);
                self.sign_and_send(msg,).await;
            }
        }

        self.enter_preflop_betting().await;
    }

    async fn start_betting_round(&mut self,) {
        self.update_pots();
        self.players.reset_bets();
        self.last_bet = Chips::ZERO;
        self.min_raise = self.big_blind;

        self.broadcast_throttle(Duration::from_millis(1000,),).await; // TODO: remove hardcoded number.

        self.players.start_round();
        self.request_action().await;
    }

    async fn enter_preflop_betting(&mut self,) {
        self.phase = HandPhase::PreflopBetting;
        self.action_update().await;
    }

    async fn enter_deal_flop(&mut self,) {
        for _ in 1..=3 {
            self.community_cards.push(self.deck.deal(),);
        }
        self.phase = HandPhase::FlopBetting;
        self.action_update().await;
    }

    async fn enter_deal_turn(&mut self,) {
        self.community_cards.push(self.deck.deal(),);
        self.phase = HandPhase::TurnBetting;
        self.start_round().await;
    }

    async fn enter_deal_river(&mut self,) {
        self.community_cards.push(self.deck.deal(),);
        self.phase = HandPhase::RiverBetting;
        self.start_round().await;
    }

    /// Called when the hand transitions to the showdown or end state.
    async fn enter_showdown(&mut self,) {
        self.phase = HandPhase::Showdown;

        // Reveal all cards for active players.
        for player in self.players.iter_mut() {
            player.last_action = PlayerAction::None;
            if player.active {
                player.public_cards = player.private_cards;
            }
        }
        self.enter_end_hand().await;
    }
    /// Called after showdown to handle chips payout, UI notifications, and
    /// player cleanup.
    async fn enter_end_hand(&mut self,) {
        self.phase = HandPhase::EndingHand;

        // Show the board/results longer after showdown, shorter otherwise.
        let delay = if self.phase == HandPhase::Showdown {
            Duration::from_secs(7,)
        } else {
            Duration::from_secs(3,)
        };
        self.hand_start_timer = Some(Instant::now(),);
        self.hand_start_delay = delay;

        // Payout and collect payoffs.
        let payoffs = self.pay_bets();

        // Inform all clients.
        self.broadcast_game_update().await;
        self.broadcast_end_hand(&payoffs,).await;

        // Remove busted players.
        self.remove_broke_players().await;

        // End game if not enough players remain.
        if self.players.count_with_chips() < 2 {
            self.enter_end_game().await;
        }
    }

    /// Broadcast the end-of-hand result to all players.
    async fn broadcast_end_hand(&mut self, payoffs: &[HandPayoff],) {
        let msg = Message::EndHand {
            payoffs: payoffs.to_vec(),
            board:   self.community_cards.clone(),
            cards:   self
                .players
                .iter()
                .map(|p| (p.id, p.public_cards,),)
                .collect(),
        };
        self.sign_and_send(msg,).await;
    }

    /// Broadcast a throttle message to all players at the table.
    async fn broadcast_throttle(&mut self, dt: Duration,) {
        let table_msg = Message::Throttle { duration: dt, };
        self.sign_and_send(table_msg,).await;
    }

    /// Broadcast a game state update to all connected players.
    async fn broadcast_game_update(&mut self,) {
        let players = self
            .players
            .iter()
            .map(|p| {
                let action_timer = p.action_timer.map(|t| {
                    Self::ACTION_TIMEOUT
                        .saturating_sub(t.elapsed(),)
                        .as_secs_f32() as u16
                },);

                PlayerUpdate {
                    player_id: p.id,
                    chips: p.chips,
                    bet: p.current_bet,
                    action: p.last_action,
                    action_timer,
                    hole_cards: p.private_cards,
                    is_dealer: p.dealer,
                    is_active: p.active,
                }
            },)
            .collect();

        let pot = self
            .pots
            .iter()
            .map(|p| p.total_chips,)
            .fold(Chips::ZERO, |acc, c| acc + c,);

        let msg = Message::GameStateUpdate {
            players,
            community_cards: self.community_cards.clone(),
            pot,
        };
        self.sign_and_send(msg,).await;
    }
    /// Request action to the active player.
    async fn request_action(&mut self,) {
        if let Some(player,) = self.players.active_player() {
            let mut actions = vec![PlayerAction::Fold];

            if player.current_bet == self.last_bet {
                actions.push(PlayerAction::Check,);
            }

            if player.current_bet < self.last_bet {
                actions.push(PlayerAction::Call,);
            }

            if self.last_bet == Chips::ZERO && player.chips > Chips::ZERO {
                actions.push(PlayerAction::Bet,);
            }

            if player.chips + player.current_bet > self.last_bet
                && self.last_bet > Chips::ZERO
                && player.chips > Chips::ZERO
            {
                actions.push(PlayerAction::Raise,);
            }

            player.action_timer = Some(Instant::now(),);

            let message = Message::ActionRequest {
                player_id: player.id,
                min_raise: self.min_raise + self.last_bet,
                big_blind: self.big_blind,
                actions,
            };

            self.sign_and_send(message,).await;
        }
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
                    .filter(|p| p.current_bet > Chips::ZERO,)
                    .map(|p| p.current_bet,)
                    .min()
                    .unwrap_or_default();

                if min_bet == Chips::ZERO {
                    break;
                }

                let mut went_all_in = false;
                for player in self.players.iter_mut() {
                    let pot = self.pots.last_mut().unwrap();
                    if player.current_bet > Chips::ZERO {
                        player.current_bet -= min_bet;
                        pot.total_chips += min_bet;

                        if !pot.participants.contains(&player.id,) {
                            pot.participants.insert(player.id,);
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

    /// Remove and notify players who have lost all their chips.
    async fn remove_broke_players(&mut self,) {
        let broke: Vec<_,> = self
            .players
            .iter()
            .filter(|p| p.chips == Chips::ZERO,)
            .map(|p| p.id,)
            .collect();

        for player_id in broke {
            let msg = Message::PlayerLeftTable {
                peer_id: player_id,
            };
            self.sign_and_send(msg,).await;
            self.players.remove(&player_id,);
        }
    }

    /// sends disconnect message for the player corresponding to the given peer
    /// id.
    async fn disconnect(&mut self, peer_id: PeerId,) {
        let msg = Message::PlayerLeftTable { peer_id, };
        self.sign_and_send(msg,).await;
    }
    /// Handles the end of the game: pays out winners and resets the table.
    async fn enter_end_game(&mut self,) {
        // Give time to the UI to look at winning results before ending the
        // game.
        self.broadcast_throttle(Duration::from_millis(4500,),).await;

        self.phase = HandPhase::EndingGame;

        // Payout remaining chips to each player, notify and remove them.
        for player in self.players.clone().iter_mut() {
            if let Err(err,) = self.credit_chips(player.id, player.chips,).await
            {
                error!("Failed to pay player {}: {}", player.id, err);
                println!("error enter_end_game: {err}");
            }
            let _ = self.disconnect(player.id,);
        }
        self.players.clear();
        self.hand_number = 0;
        self.phase = HandPhase::WaitingForPlayers;
    }

    async fn credit_chips(
        &mut self,
        id: PeerId,
        chips: Chips,
    ) -> Result<(), anyhow::Error,> {
        let msg = Message::BalanceUpdate {
            player_id: id,
            chips,
        };
        self.sign_and_send(msg,).await
    }

    /// Distribute the pots among winners and return their payoff info for the
    /// UI.
    fn pay_bets(&mut self,) -> Vec<HandPayoff,> {
        let mut payoffs = Vec::new();
        match self.players.count_active() {
            1 => self.pay_single_winner(&mut payoffs,),
            n if n > 1 => self.pay_multiple_winners(&mut payoffs,),
            _ => {},
        }
        payoffs
    }

    fn pay_single_winner(&mut self, payoffs: &mut Vec<HandPayoff,>,) {
        if let Some(player,) = self.players.active_player() {
            for pot in self.pots.drain(..,) {
                player.chips += pot.total_chips;
                match payoffs.iter_mut().find(|p| p.player_id == player.id,) {
                    Some(payoff,) => payoff.chips += pot.total_chips,
                    None => {
                        payoffs.push(HandPayoff {
                            player_id: player.id,
                            chips:     pot.total_chips,
                            cards:     vec![],
                            rank:      String::new(),
                        },)
                    },
                }
            }
        }
    }
    fn pay_multiple_winners(&mut self, payoffs: &mut Vec<HandPayoff,>,) {
        let pots = self.pots.drain(..,);
        let community_cards = &self.community_cards;

        for pot in pots {
            // Gather all active players in this pot. //TODO: refactor
            let mut contenders = self
                .players
                .iter_mut()
                .filter(|p| p.active && pot.participants.contains(&p.id,),)
                .filter_map(|player| {
                    match player.private_cards {
                        PlayerCards::Cards(c1, c2,) => Some((player, c1, c2,),),
                        _ => None,
                    }
                },)
                .map(|(p, c1, c2,)| {
                    let mut cards = vec![c1, c2];
                    cards.extend_from_slice(community_cards,);
                    let (value, best_hand,) =
                        HandValue::eval_with_best_hand(&cards,);
                    (p, value, best_hand,)
                },)
                .collect::<Vec<_,>>();

            if contenders.is_empty() {
                continue;
            }

            // Sort descending by hand value.
            contenders.sort_by(|a, b| b.1.cmp(&a.1,),);
            let best_value = &contenders[0].1;
            let winner_count = contenders
                .iter()
                .filter(|(_, v, _,)| v == best_value,)
                .count();
            let chips_per_winner: Chips =
                pot.total_chips / (winner_count as u32);
            let extra_chip: Chips = pot.total_chips % winner_count as u32;

            for (i, (player, value, best_hand,),) in
                contenders.iter_mut().take(winner_count,).enumerate()
            {
                let payoff = chips_per_winner
                    + if i == 0 {
                        extra_chip
                    } else {
                        Chips::ZERO
                    };
                player.chips += payoff;

                let mut best_hand_cards = best_hand.to_vec();
                best_hand_cards.sort_by_key(|c| c.rank(),);

                match payoffs.iter_mut().find(|p| p.player_id == player.id,) {
                    Some(existing,) => existing.chips += payoff,
                    None => {
                        payoffs.push(HandPayoff {
                            player_id: player.id,
                            chips:     payoff,
                            cards:     best_hand_cards,
                            rank:      value.rank().to_string(),
                        },)
                    },
                }
            }
        }
    }
}
