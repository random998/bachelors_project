// code inspired by / taken from // code taken from https://github.com/vincev/freezeout
//! Table player types and player state management.

use std::fmt;
use std::time::{Duration, Instant};

use poker_core::crypto::PeerId;
use poker_core::message::{PlayerAction, SignedMessage};
use poker_core::net::traits::{ChannelNetTx, TableMessage};
use poker_core::net::NetTx;
use poker_core::poker::{Chips, PlayerCards};
use tokio::sync::Mutex;
use crate::table::Sender;

use crate::table::Arc;

/// Represents a single poker player at a table.
#[derive(Clone,)]
pub struct Player {
    pub id: PeerId,
    pub tx: ChannelNetTx,
    pub net_tx: Arc<Mutex<dyn NetTx + Send,>,>,
    pub nickname: String,
    pub chips: Chips,
    pub current_bet: Chips,
    pub last_action: PlayerAction,
    pub action_timer: Option<Instant,>,
    pub public_cards: PlayerCards,
    pub private_cards: PlayerCards,
    /// this player is active in the hand
    pub active: bool,
    pub dealer: bool,
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        f.debug_struct("Player",)
            .field("id", &self.id,)
            .field("nickname", &self.nickname,)
            .field("chips", &self.chips,)
            .field("current_bet", &self.current_bet,)
            .field("last_action", &self.last_action,)
            .field("active", &self.active,)
            .field("dealer", &self.dealer,)
            .finish()
    }
}

impl Player {
    pub fn new(
        id: PeerId, nickname: String, chips: Chips, tx: ChannelNetTx,
        net_tx: Box<dyn NetTx + Send,>,
    ) -> Self {
        Self {
            id,
            tx,
            net_tx: Arc::new(Mutex::new(net_tx,),),
            nickname,
            chips,
            current_bet: Chips::ZERO,
            last_action: PlayerAction::None,
            action_timer: None,
            public_cards: PlayerCards::None,
            private_cards: PlayerCards::None,
            active: true,
            dealer: false,
        }
    }

    pub async fn send_table_msg(&mut self, msg: TableMessage,) {
        let _ = self.tx.send_table(msg,).await;
    }
    pub async fn send(&mut self, msg: SignedMessage,) {
        let _ = self.tx.send(msg,).await;
    }

    pub async fn notify_left(&mut self,) {
        let _ = self.send_table_msg(TableMessage::PlayerLeave,).await;
    }

    pub async fn send_throttle(&mut self, duration: Duration,) {
        let _ = self.send_table_msg(TableMessage::Throttle(duration,),).await;
    }

    pub fn place_bet(&mut self, action: PlayerAction, total_bet: Chips,) {
        let required = total_bet - self.current_bet;
        let actual_bet = required.min(self.chips,);

        self.chips -= actual_bet;
        self.current_bet += actual_bet;
        self.last_action = action;
    }

    pub fn fold(&mut self,) {
        self.active = false;
        self.last_action = PlayerAction::Fold;
        self.private_cards = PlayerCards::None;
        self.public_cards = PlayerCards::None;
        self.action_timer = None;
    }

    pub fn reset_for_new_hand(&mut self,) {
        self.active = self.chips > Chips::ZERO;
        self.dealer = false;
        self.current_bet = Chips::ZERO;
        self.last_action = PlayerAction::None;
        self.public_cards = PlayerCards::None;
        self.private_cards = PlayerCards::None;
    }

    pub fn reset_bet(&mut self,) {
        self.current_bet = Chips::ZERO;
    }

    pub fn finalize_hand(&mut self,) {
        self.last_action = PlayerAction::None;
        self.action_timer = None;
    }

    pub fn has_chips(&self,) -> bool {
        self.chips > Chips::ZERO
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use tokio::sync::mpsc;
    use poker_core::crypto::SigningKey;

    use super::*;

    struct MockNet;
    #[async_trait::async_trait]
    impl NetTx for MockNet {
        async fn send(&mut self, _m: SignedMessage,) -> anyhow::Result<(),> {
            Ok((),)
        }

        async fn send_table(&mut self, msg: TableMessage,) -> anyhow::Result<(), > {
            Ok(())
        }
    }

    fn new_player(chips: Chips,) -> Player {
        let peer_id = SigningKey::default().verifying_key().peer_id();
        let (table_tx, _table_rx,): (Sender<TableMessage,>, mpsc::Receiver<TableMessage,>,) =
            mpsc::channel(10,);
        let table_tx = ChannelNetTx {
            tx: table_tx.clone(),
        };
        let net = Box::new(MockNet,);
        Player::new(peer_id, "Alice".to_string(), chips, table_tx, net,)
    }

    #[test]
    fn test_player_bet() {
        let init_chips = Chips::new(100_000,);
        let mut player = new_player(init_chips,);

        // Simple bet.
        let bet_size = Chips::new(60_000,);
        player.place_bet(PlayerAction::Bet, bet_size,);
        assert_eq!(player.current_bet, bet_size.clone());
        assert_eq!(player.chips, init_chips - bet_size);
        assert!(matches!(player.last_action, PlayerAction::Bet));

        // The bet amount is the total bet check chips paid are the new bet minus the
        // previous bet.
        let bet_size = bet_size + Chips::new(20_000,);
        player.place_bet(PlayerAction::Bet, bet_size,);
        assert_eq!(player.current_bet, bet_size.clone());
        assert_eq!(player.chips, init_chips - bet_size);

        // Start new hand reset bet chips and action.
        player.reset_for_new_hand();
        assert!(matches!(player.last_action, PlayerAction::None));
        assert!(player.active);
        assert_eq!(player.current_bet, Chips::ZERO);
        assert_eq!(player.chips, init_chips - bet_size);

        // Bet more than remaining chips goes all in.
        let remaining_chips = player.chips;
        player.place_bet(PlayerAction::Bet, Chips::new(1_000_000,),);
        assert_eq!(player.current_bet, remaining_chips);
        assert_eq!(player.chips, Chips::ZERO);
    }

    #[test]
    fn test_player_fold() {
        let init_chips = Chips::new(100_000,);
        let mut player = new_player(init_chips,);

        player.place_bet(PlayerAction::Bet, Chips::new(20_000,),);
        player.action_timer = Some(Instant::now(),);

        player.fold();
        assert!(matches!(player.last_action, PlayerAction::Fold));
        assert!(!player.active);
        assert!(player.action_timer.is_none());
    }
}
