// code inspired by https://github.com/vincev/freezeout
//! tables pool.

use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::Result;
use log::error;
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::poker::Chips;
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::{broadcast, Mutex};

use crate::db::Database;
use crate::table::{Table, TableJoinError, TableMessage};

#[derive(Debug, Error,)]
pub enum TablesPoolError {
    #[error("No tables available for joining.")]
    NoTablesLeft,

    #[error("Player has already joined a table.")]
    PlayerAlreadyJoined,
}

#[derive(Clone, Debug,)]
pub struct TablesPool(Arc<Mutex<SharedTables,>,>,);

#[derive(Debug,)]
struct SharedTables {
    available: VecDeque<Arc<Table,>,>,
    full: VecDeque<Arc<Table,>,>,
}

impl TablesPool {
    pub fn new(
        table_count: usize, seats_per_table: usize, signing_key: Arc<SigningKey,>,
        database: Database, shutdown_tx: &broadcast::Sender<(),>,
        shutdown_complete_tx: &Sender<(),>,
    ) -> Self {
        let available = (0..table_count)
            .map(|_| {
                Arc::new(Table::new(
                    seats_per_table,
                    signing_key.clone(),
                    database.clone(),
                    shutdown_tx.subscribe(),
                    shutdown_complete_tx.clone(),
                ),)
            },)
            .collect();

        Self(Arc::new(Mutex::new(SharedTables {
            available,
            full: VecDeque::with_capacity(table_count,),
        },),),)
    }

    pub async fn join(
        &self, player_id: &PeerId, nickname: &str, chips: Chips, msg_tx: Sender<TableMessage,>,
    ) -> Result<Arc<Table,>, TablesPoolError,> {
        let mut pool = self.0.lock().await;

        if pool.available.is_empty() {
            Self::rehydrate_available_tables(&mut pool,).await;
        }

        if let Some(table,) = pool.available.front() {
            let join_result = table.try_join(player_id, nickname, chips, msg_tx.clone(),).await;

            match join_result {
                | Err(TableJoinError::AlreadyJoined,) => Err(TablesPoolError::PlayerAlreadyJoined,),
                | Err(_,) => Err(TablesPoolError::NoTablesLeft,),
                | Ok(_,) => {
                    if !table.can_player_join().await {
                        let full_table = pool.available.pop_front().unwrap();
                        pool.full.push_back(full_table.clone(),);
                        Ok(full_table,)
                    } else {
                        Ok(table.clone(),)
                    }
                },
            }
        } else {
            Err(TablesPoolError::NoTablesLeft,)
        }
    }

    pub async fn leave(
        &self, table: Arc<Table,>, player_id: &PeerId,
    ) -> Result<Arc<Table,>, TablesPoolError,> {
        // Ask the table to remove the player.
        let res = table.leave(player_id,).await;
        // Try to move any now-joinable tables from full to available.
        let mut pool = self.0.lock().await;
        Self::rehydrate_available_tables(&mut pool,).await;
        match res {
            | Err(_,) => Err(TablesPoolError::NoTablesLeft,),
            | Ok(_,) => Ok(table,),
        }
    }

    async fn rehydrate_available_tables(pool: &mut SharedTables,) {
        for _ in 0..pool.full.len() {
            if let Some(table,) = pool.full.pop_front() {
                if table.can_player_join().await {
                    pool.available.push_back(table,);
                } else {
                    pool.full.push_back(table,);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use poker_core::poker::TableId;
    use tokio::sync::mpsc;

    use super::*;

    struct TestPool {
        pool: TablesPool,
        _shutdown_broadcast_tx: broadcast::Sender<(),>,
        _shutdown_complete_rx: mpsc::Receiver<(),>,
    }

    impl TestPool {
        fn new(n: usize,) -> Self {
            let sk = SigningKey::default();
            let db = Database::open_in_memory().unwrap();
            let (shutdown_complete_tx, shutdown_complete_rx,) = mpsc::channel(1,);
            let (shutdown_broadcast_tx, _,) = broadcast::channel(1,);
            let pool = TablesPool::new(
                n,
                2,
                Arc::new(sk,),
                db,
                &shutdown_broadcast_tx,
                &shutdown_complete_tx,
            );

            Self {
                pool,
                _shutdown_broadcast_tx: shutdown_broadcast_tx,
                _shutdown_complete_rx: shutdown_complete_rx,
            }
        }

        async fn join(&self, player: &TestPlayer,) -> Option<Arc<Table,>,> {
            self.pool.join(&player.id, "nn", Chips::new(1_000_000,), player.tx.clone(),).await.ok()
        }

        async fn leave(&self, table: Arc<Table,>, player: &TestPlayer,) -> Option<Arc<Table,>,> {
            self.pool.leave(table, &player.id,).await.ok()
        }

        async fn avail_ids(&self,) -> Vec<TableId,> {
            let pool = self.pool.0.lock().await;
            pool.available.iter().map(|t| t.id(),).collect()
        }

        async fn count_avail(&self,) -> usize {
            let pool = self.pool.0.lock().await;
            pool.available.len()
        }

        async fn full_ids(&self,) -> Vec<TableId,> {
            let pool = self.pool.0.lock().await;
            pool.full.iter().map(|t| t.id(),).collect()
        }

        async fn count_full(&self,) -> usize {
            let pool = self.pool.0.lock().await;
            pool.full.len()
        }
    }

    struct TestPlayer {
        tx: Sender<TableMessage,>,
        _rx: mpsc::Receiver<TableMessage,>,
        id: PeerId,
    }

    impl TestPlayer {
        fn new() -> Self {
            let sk = SigningKey::default();
            let peer_id = sk.verifying_key().peer_id();
            let (tx, rx,) = mpsc::channel(64,);
            Self {
                tx,
                _rx: rx,
                id: peer_id,
            }
        }
    }

    #[tokio::test]
    async fn test_table_pool() {
        let test_pool = TestPool::new(2,);
        let table_ids = test_pool.avail_ids().await;

        // Player 1 join table 1 that should be in first position.
        let player1 = TestPlayer::new();
        let table1 = test_pool.join(&player1,).await.unwrap();
        assert_eq!(table1.id(), table_ids[0]);

        // Player 2 join table 1.
        let player2 = TestPlayer::new();
        let table1 = test_pool.join(&player2,).await.unwrap();
        assert_eq!(table1.id(), table_ids[0]);

        // As the table is full it should move to the full queue.
        let table_ids = test_pool.full_ids().await;
        assert_eq!(table1.id(), table_ids[0]);

        // Player 1 join table 2, table 2 should be at front of the queue.
        let table_ids = test_pool.avail_ids().await;
        let table2 = test_pool.join(&player1,).await.unwrap();
        assert_eq!(table2.id(), table_ids[0]);

        // Player 2 join table 2.
        let table2 = test_pool.join(&player2,).await.unwrap();
        assert_eq!(table2.id(), table_ids[0]);

        // Player 3 tries to join but there are no tables.
        let player3 = TestPlayer::new();
        assert!(test_pool.join(&player3).await.is_none());

        // Players 2 leaves table 1 that becomes ready because with one player left
        // the game ends (2 seats per table), table 1 should move to the available
        // queue when a player tries to join.
        let table1 = test_pool.leave(table1, &player2,).await;

        println!(
            "available ids after player 2 leaves: {:?}",
            test_pool
                .avail_ids()
                .await
                .to_vec()
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<String,>>()
        );

        // check if table1 is in the available queue.
        assert!(test_pool.avail_ids().await.contains(&table1.clone().unwrap().id()));
        assert!(!test_pool.full_ids().await.contains(&table1.unwrap().id()));

        // Player 2 joins table 1, note that the join operation moves the tables between
        // queue.
        let table_ids2 = test_pool.avail_ids().await;
        println!("table ids: {:?}", table_ids2);
        let table1 = test_pool.join(&player2,).await.unwrap();
        assert_eq!(table1.id(), table_ids2[0]);

        // Player 3 join table 1.
        let table1 = test_pool.join(&player3,).await.unwrap();
        assert_eq!(table1.id(), table_ids2[0]);
    }

    #[tokio::test]
    async fn test_big_pool() {
        const N: usize = 1_000;
        let tp = TestPool::new(N,);

        // We should be able to join all tables.
        let mut players = Vec::with_capacity(N * 2,);
        for _ in 0..N * 2 {
            let p = TestPlayer::new();
            let t = tp.join(&p,).await.unwrap();
            players.push((p, t,),);
        }

        assert_eq!(tp.count_avail().await, 0);
        assert_eq!(tp.count_full().await, N);

        // Leave all the tables.
        for (p, t,) in players {
            _ = t.leave(&p.id,).await;
        }

        // One player joins.
        let p = TestPlayer::new();
        tp.join(&p,).await.expect("error");

        assert_eq!(tp.count_avail().await, N);
        assert_eq!(tp.count_full().await, 0);

        // Another player joins first table full.
        let p = TestPlayer::new();
        tp.join(&p,).await.unwrap();
        assert_eq!(tp.count_avail().await, N - 1);
        assert_eq!(tp.count_full().await, 1);
    }
}
