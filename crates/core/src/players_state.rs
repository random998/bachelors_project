use std::fmt;

use rand::Rng;
use rand::prelude::SliceRandom;

use crate::crypto::PeerId;
use crate::player::PlayerPublic;

#[derive(Clone, Debug, Default,)]
pub struct PlayerStateObjects {
    players:           Vec<PlayerPublic,>,
    active_player_idx: Option<usize,>,
}

impl PlayerStateObjects {
    pub fn add(&mut self, player: PlayerPublic,) {
        self.players.push(player,);
    }

    pub fn clear(&mut self,) {
        self.players.clear();
        self.active_player_idx = None;
    }

    pub fn remove(&mut self, id: &PeerId,) -> Option<PlayerPublic,> {
        let mut old_active_player_idx = 0;
        if self.active_player_idx.is_some() {
            old_active_player_idx = self.active_player_idx.unwrap();
        }
        if let Some(pos,) = self.players.iter().position(|p| &p.id == id,) {
            let removed = self.players.remove(pos,);

            // if the removed player was active, then active_player_idx should
            // be none.
            assert!(
                self.active_player_idx.is_none()
                    || (old_active_player_idx
                        == self.active_player_idx.unwrap()
                        && self.active_player_idx.is_some())
            );

            match self.active_player_idx {
                Some(idx,) if idx == pos => {
                    self.active_player_idx =
                        self.players.iter().position(|p| p.active,);
                },
                Some(idx,) if pos < idx => {
                    self.active_player_idx = Some(idx - 1,);
                },
                _ => {},
            }

            Some(removed,)
        } else {
            None
        }
    }

    pub fn get(&self, id: &PeerId,) -> Option<&PlayerPublic,> {
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
        self.players
            .iter()
            .filter(|p| p.active && p.has_chips(),)
            .count()
    }

    pub fn active_player(&mut self,) -> Option<&mut PlayerPublic,> {
        self.active_player_idx
            .and_then(move |i| self.players.get_mut(i,),)
            .filter(|p| p.active,)
    }

    pub fn is_active(&self, id: &PeerId,) -> bool {
        self.active_player_idx
            .and_then(|i| self.players.get(i,),)
            .map(|p| &p.id == id,)
            .unwrap_or(false,)
    }

    pub fn iter(&self,) -> impl Iterator<Item = &PlayerPublic,> {
        self.players.iter()
    }

    pub fn iter_mut(&mut self,) -> impl Iterator<Item = &mut PlayerPublic,> {
        self.players.iter_mut()
    }

    pub fn advance_turn(&mut self,) {
        if self.count_active_with_chips() <= 1
            || self.active_player_idx.is_none()
        {
            return;
        }

        let current = self.active_player_idx.unwrap();
        for (i, player,) in self
            .players
            .iter()
            .enumerate()
            .cycle()
            .skip(current + 1,)
            .take(self.players.len(),)
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
            self.players
                .iter_mut()
                .rev()
                .find(|p| p.active,)
                .map(|p| p.dealer = true,);
            self.active_player_idx =
                self.players.iter().position(|p| p.active,);
        } else {
            self.active_player_idx = None;
        }
    }

    pub fn start_round(&mut self,) {
        self.active_player_idx = None;
        if self.count_active_with_chips() > 1 {
            self.active_player_idx =
                self.players.iter().position(|p| p.active && p.has_chips(),);
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

impl fmt::Display for PlayerStateObjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{:?}", self.players.clone())
    }
}
