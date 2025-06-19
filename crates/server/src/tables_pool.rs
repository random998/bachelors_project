// code inspired by https://github.com/vincev/freezeout
//! tables pool.

use anyhow::Result;
use log::error;
use std::{collections::VecDeque, sync::Arc};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, Mutex};

use poker_core::{
    crypto::{PeerId, SigningKey},
    poker::Chips,
};

use crate::{
    db::Database,
    table::{Table, TableJoinError, TableMessage},
};

#[derive(Debug, Error)]
pub enum TablesPoolError {
    #[error("No tables available for joining.")]
    NoTablesLeft,

    #[error("Player has already joined a table.")]
    PlayerAlreadyJoined,
}

#[derive(Debug)]
pub struct TablesPool(Arc<Mutex<SharedTables>>);

#[derive(Debug)]
struct SharedTables {
    available: VecDeque<Arc<Table>>,
    full: VecDeque<Arc<Table>>,
}

impl TablesPool {
    pub fn new(
        table_count: usize,
        seats_per_table: usize,
        signing_key: Arc<SigningKey>,
        database: Database,
        shutdown_tx: &broadcast::Sender<()>,
        shutdown_complete_tx: &mpsc::Sender<()>,
    ) -> Self {
        let available = (0..table_count)
            .map(|_| {
                Arc::new(Table::new(
                    seats_per_table,
                    signing_key.clone(),
                    database.clone(),
                    shutdown_tx.subscribe(),
                    shutdown_complete_tx.clone(),
                ))
            })
            .collect();

        Self(Arc::new(Mutex::new(SharedTables {
            available,
            full: VecDeque::with_capacity(table_count),
        })))
    }

    pub async fn join(
        &self,
        player_id: &PeerId,
        nickname: &str,
        chips: Chips,
        msg_tx: mpsc::Sender<TableMessage>,
    ) -> Result<Arc<Table>, TablesPoolError> {
        let mut pool = self.0.lock().await;

        if pool.available.is_empty() {
            Self::rehydrate_available_tables(&mut pool).await;
        }

        if let Some(table) = pool.available.front() {
            let join_result = table
                .try_join(player_id, nickname, chips, msg_tx.clone())
                .await;

            match join_result {
                Err(TableJoinError::AlreadyJoined) => Err(TablesPoolError::PlayerAlreadyJoined),
                Err(_) => Err(TablesPoolError::NoTablesLeft),
                Ok(_) => {
                    if !table.can_player_join().await {
                        let full_table = pool.available.pop_front().unwrap();
                        pool.full.push_back(full_table.clone());
                        Ok(full_table)
                    } else {
                        Ok(table.clone())
                    }
                }
            }
        } else {
            Err(TablesPoolError::NoTablesLeft)
        }
    }

    async fn rehydrate_available_tables(pool: &mut SharedTables) {
        for _ in 0..pool.full.len() {
            if let Some(table) = pool.full.pop_front() {
                if table.can_player_join().await {
                    pool.available.push_back(table);
                } else {
                    pool.full.push_back(table);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::poker::TableId;

    struct TestPool {
        pool: TablesPool,
        _shutdown_broadcast_tx: broadcast::Sender<()>,
        _shutdown_complete_rx: mpsc::Receiver<()>,
    }

    impl TestPool {
        fn new(n: usize) -> Self {
            let sk = SigningKey::default();
            let db = Database::open_in_memory().unwrap();
            let (shutdown_complete_tx, shutdown_complete_rx) = mpsc::channel(1);
            let (shutdown_broadcast_tx, _) = broadcast::channel(1);
            let pool = TablesPool::new(
                n,
                2,
                Arc::new(sk),
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

        async fn join(&self, test_player: &TestPlayer) -> Option<Arc<Table>> {
            self.pool
                .join(
                    &test_player.id,
                    "nn",
                    Chips::new(1_000_000),
                    test_player.tx.clone(),
                )
                .await
                .ok()
        }

        async fn avail_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.available.iter().map(|t| t.id()).collect()
        }

        async fn count_avail(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.available.len()
        }

        async fn full_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.full.iter().map(|t| t.id()).collect()
        }

        async fn count_full(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.full.len()
        }
    }

    struct TestPlayer {
        tx: mpsc::Sender<TableMessage>,
        _rx: mpsc::Receiver<TableMessage>,
        id: PeerId,
    }

    impl TestPlayer {
        fn new() -> Self {
            let sk = SigningKey::default();
            let peer_id = sk.verifying_key().peer_id();
            let (tx, rx) = mpsc::channel(64);
            Self {
                tx,
                _rx: rx,
                id: peer_id,
            }
        }
    }

    #[tokio::test]
    async fn test_table_pool() {
        let tp = TestPool::new(2);
        let tids = tp.avail_ids().await;

        // Player 1 join table 1 that should be in first position.
        let player1 = TestPlayer::new();
        let table1 = tp.join(&player1).await.unwrap();
        assert_eq!(table1.id(), tids[0]);

        // Player 2 join table 1.
        let player2 = TestPlayer::new();
        let table1 = tp.join(&player2).await.unwrap();
        assert_eq!(table1.id(), tids[0]);

        // As the table is full it should move to the full queue.
        let table_ids = tp.full_ids().await;
        assert_eq!(table1.id(), tids[0]);

        // Player 1 join table 2, table 2 should be at front of the queue.
        let table_ids = tp.avail_ids().await;
        let table2 = tp.join(&player1).await.unwrap();
        assert_eq!(table2.id(), table_ids[0]);

        // Player 2 join table 2.
        let table2 = tp.join(&player2).await.unwrap();
        assert_eq!(table2.id(), table_ids[0]);

        // Player 3 tries to join but there are no tables.
        let player3 = TestPlayer::new();
        assert!(tp.join(&player3).await.is_none());

        // Players 2 leaves table 1 that becomes ready because with one player left
        // the game ends (2 seats per table), table 1 should move to the available
        // queue when a play tries to join.
        table1.leave(&player2.id).await;

        // Player 1 joins table 2, note that the join operation moves the tables between
        // queue.
        let table2 = tp.join(&player1).await.unwrap();
        let table_ids = tp.avail_ids().await;
        assert_eq!(table2.id(), tids[0]);

        // Player 2 join table 2.
        let table2 = tp.join(&player2).await.unwrap();
        assert_eq!(table2.id(), tids[0]);
    }

    #[tokio::test]
    async fn test_big_pool() {
        const N: usize = 1_000;
        let tp = TestPool::new(N);

        // We should be able to join all tables.
        let mut players = Vec::with_capacity(N * 2);
        for _ in 0..N * 2 {
            let p = TestPlayer::new();
            let t = tp.join(&p).await.unwrap();
            players.push((p, t));
        }

        assert_eq!(tp.count_avail().await, 0);
        assert_eq!(tp.count_full().await, N);

        // Leave all the tables.
        for (p, t) in players {
            t.leave(&p.id).await;
        }

        // One player joins.
        let p = TestPlayer::new();
        tp.join(&p).await.unwrap();

        assert_eq!(tp.count_avail().await, N);
        assert_eq!(tp.count_full().await, 0);

        // Another player joins first table full.
        let p = TestPlayer::new();
        tp.join(&p).await.unwrap();
        assert_eq!(tp.count_avail().await, N - 1);
        assert_eq!(tp.count_full().await, 1);
    }
}
