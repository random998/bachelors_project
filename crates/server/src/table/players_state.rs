use std::fmt;

use poker_core::crypto::PeerId;
use rand::prelude::SliceRandom;
use rand::Rng;

use crate::table::player::Player;

#[derive(Clone, Debug, Default,)]
pub struct PlayersState {
    players: Vec<Player,>,
    active_player_idx: Option<usize,>,
}

impl PlayersState {
    pub fn add(&mut self, player: Player,) {
        self.players.push(player,);
    }

    pub fn clear(&mut self,) {
        self.players.clear();
        self.active_player_idx = None;
    }

    pub fn remove(&mut self, id: &PeerId,) -> Option<Player,> {
        let mut old_active_player_idx = 0;
        if self.active_player_idx.is_some() {
            old_active_player_idx = self.active_player_idx.unwrap();
        }
        if let Some(pos,) = self.players.iter().position(|p| &p.id == id,) {
            let removed = self.players.remove(pos,);

            // if the removed player was active, then active_player_idx should be none.
            assert!(
                self.active_player_idx.is_none()
                    || (old_active_player_idx == self.active_player_idx.unwrap()
                        && self.active_player_idx.is_some())
            );

            match self.active_player_idx {
                | Some(idx,) if idx == pos => {
                    self.active_player_idx = self.players.iter().position(|p| p.active,);
                },
                | Some(idx,) if pos < idx => {
                    self.active_player_idx = Some(idx - 1,);
                },
                | _ => {},
            }

            Some(removed,)
        } else {
            None
        }
    }

    pub fn get(&self, id: &PeerId,) -> Option<&Player,> {
        self.players.iter().find(|p| &p.id == id,)
    }

    pub fn shuffle<R: Rng,>(&mut self, rng: &mut R,) {
        self.players.shuffle(rng,);
    }

    /// returns the total number of players.
    pub fn count(&self,) -> usize {
        self.players.len()
    }

    pub fn count_active(&self,) -> usize {
        self.players.iter().filter(|p| p.active,).count()
    }

    pub fn count_with_chips(&self,) -> usize {
        self.players.iter().filter(|p| p.has_chips(),).count()
    }

    pub fn count_active_with_chips(&self,) -> usize {
        self.players.iter().filter(|p| p.active && p.has_chips(),).count()
    }

    pub fn active_player(&mut self,) -> Option<&mut Player,> {
        self.active_player_idx.and_then(move |i| self.players.get_mut(i,),).filter(|p| p.active,)
    }

    pub fn is_active(&self, id: &PeerId,) -> bool {
        self.active_player_idx
            .and_then(|i| self.players.get(i,),)
            .map(|p| &p.id == id,)
            .unwrap_or(false,)
    }

    pub fn iter(&self,) -> impl Iterator<Item = &Player,> {
        self.players.iter()
    }

    pub fn iter_mut(&mut self,) -> impl Iterator<Item = &mut Player,> {
        self.players.iter_mut()
    }

    pub fn advance_turn(&mut self,) {
        if self.count_active_with_chips() <= 1 || self.active_player_idx.is_none() {
            return;
        }

        let current = self.active_player_idx.unwrap();
        for (i, player,) in
            self.players.iter().enumerate().cycle().skip(current + 1,).take(self.players.len(),)
        {
            if player.active && player.has_chips() {
                self.active_player_idx = Some(i,);
                break;
            }
        }
    }

    pub fn start_hand(&mut self,) {
        for player in &mut self.players {
            player.reset_for_new_hand();
        }

        if self.count_active() > 1 {
            self.players.iter_mut().rev().find(|p| p.active,).map(|p| p.dealer = true,);
            self.active_player_idx = self.players.iter().position(|p| p.active,);
        } else {
            self.active_player_idx = None;
        }
    }

    pub fn start_round(&mut self,) {
        self.active_player_idx = None;
        if self.count_active_with_chips() > 1 {
            self.active_player_idx = self.players.iter().position(|p| p.active && p.has_chips(),);
        }
    }

    pub fn end_hand(&mut self,) {
        self.active_player_idx = None;
        for p in &mut self.players {
            p.finalize_hand();
        }
    }
    pub fn remove_bankrupt_players(&mut self,) {
        self.players.retain(|p| p.has_chips(),);
    }

    pub fn reset_bets(&mut self,) {
        for player in &mut self.players {
            player.reset_for_new_hand();
        }
    }
}

impl fmt::Display for PlayersState {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{:?}", self.players.clone())
    }
}

mod tests {
    use async_trait::async_trait;
    use poker_core::crypto::SigningKey;
    use poker_core::message::SignedMessage;
    use poker_core::net::traits::{ChannelNetTx, TableMessage};
    use poker_core::net::NetTx;
    use poker_core::poker::Chips;
    use tokio::sync::mpsc;

    use crate::table::player::Player;
    use crate::table::players_state::PlayersState;

    struct MockNet;
    #[async_trait]
    impl NetTx for MockNet {
        async fn send(&mut self, _m: SignedMessage,) -> anyhow::Result<(),> {
            Ok((),)
        }

        async fn send_table(&mut self, _msg: TableMessage,) -> anyhow::Result<(),> {
            Ok((),)
        }
    }

    fn new_player(chips: Chips,) -> Player {
        let peer_id = SigningKey::default().verifying_key().peer_id();
        let (table_tx, _table_rx,) = mpsc::channel(4,);
        let table_tx: ChannelNetTx = ChannelNetTx {
            tx: table_tx,
        };

        let net_box: Box<dyn NetTx + Send,> = Box::new(MockNet,); // NEW
        Player::new(peer_id, "Alice".to_string(), chips, table_tx, net_box,)
    }
    fn new_players_state(n: usize,) -> PlayersState {
        let mut players = PlayersState::default();
        (0..n).for_each(|_| players.add(new_player(Chips::new(100_000,),),),);
        players
    }

    #[test]
    fn player_before_active_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS,);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.advance_turn();
        assert_eq!(players.active_player_idx.unwrap(), 1);

        // Player before active leaves, the active player moved to position 0.
        let player_id = players.players[0].id;
        assert!(players.remove(&player_id).is_some());
        assert_eq!(players.active_player_idx.unwrap(), 0);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn player_after_active_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS,);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.advance_turn();
        assert_eq!(players.active_player_idx.unwrap(), 1);

        // Player after active leaves, the active player should be the same.
        let player_id = players.players[2].id;
        assert!(players.remove(&player_id).is_some());
        assert_eq!(players.active_player_idx.unwrap(), 1);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn active_player_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS,);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.advance_turn();
        assert_eq!(players.active_player_idx.unwrap(), 1);

        // Active leaves the next player should become active.
        let active_id = players.players[1].id;
        let next_id = players.players[0].id;
        assert!(players.remove(&active_id).is_some());
        assert_eq!(players.active_player_idx.unwrap(), 0);
        assert_eq!(players.active_player().unwrap().id, next_id);
        assert_eq!(players.count_active(), SEATS - 1);
    }

    #[test]
    fn active_player_before_inactive_player_leaves() {
        const SEATS: usize = 4;
        let mut players = new_players_state(SEATS,);

        assert_eq!(players.count_active(), SEATS);
        assert!(players.active_player().is_none());

        // Make player at index 1 active.
        players.start_hand();
        players.advance_turn();
        assert_eq!(players.active_player_idx.unwrap(), 1);

        // Deactivate player at index 0
        players.iter_mut().nth(0,).unwrap().fold();
        assert_eq!(players.count_active(), SEATS - 1);

        // check if player at index 0 and player at index 1 have different ids.
        assert_ne!(players.players[0].id, players.players[1].id);

        // Active player (player at idx 1) leaves but the player at index 0 has folded
        // so the next player at index 2 should become active, which has been
        // moved to idx 1.
        let active_id = players.players[1].id;
        assert_eq!(players.active_player_idx.unwrap(), 1);
        let next_id = players.players[2].id;
        assert!(players.remove(&active_id).is_some());
        assert_eq!(players.active_player_idx.unwrap(), 1);
        assert_eq!(players.active_player().unwrap().id, next_id);
        assert_eq!(players.count_active(), SEATS - 2);
    }
}
