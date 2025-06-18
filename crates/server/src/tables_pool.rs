// code inspired byhttps://github.com/vincev/freezeout
//! tables pool.

use anyhow::Result;
use log::{info, warn, error};
use std::{collections::VecDeque, sync::Arc};
use thiserror::Error;
use tokio::sync::{Mutex, broadcast, mpsc};

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
            .map(|_| Arc::new(Table::new(
                seats_per_table,
                signing_key.clone(),
                database.clone(),
                shutdown_tx.subscribe(),
                shutdown_complete_tx.clone(),
            )))
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
                Err(TableJoinError::PlayerAlreadyJoined) => return Err(TablesPoolError::PlayerAlreadyJoined),
                Err(_) => return Err(TablesPoolError::NoTablesLeft),
                Ok(_) => {
                    if !table.player_can_join().await {
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
                if table.player_can_join().await {
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
            let db = Db::open_in_memory().unwrap();
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

        async fn join(&self, p: &TestPlayer) -> Option<Arc<Table>> {
            self.pool
                .join(&p.peer_id, "nn", Chips::new(1_000_000), p.tx.clone())
                .await
                .ok()
        }

        async fn avail_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.avail.iter().map(|t| t.table_id()).collect()
        }

        async fn count_avail(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.avail.len()
        }

        async fn full_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.full.iter().map(|t| t.table_id()).collect()
        }

        async fn count_full(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.full.len()
        }
    }

    struct TestPlayer {
        tx: mpsc::Sender<TableMessage>,
        _rx: mpsc::Receiver<TableMessage>,
        peer_id: PeerId,
    }

    impl TestPlayer {
        fn new() -> Self {
            let sk = SigningKey::default();
            let peer_id = sk.verifying_key().peer_id();
            let (tx, rx) = mpsc::channel(64);
            Self {
                tx,
                _rx: rx,
                peer_id,
            }
        }
    }

    #[tokio::test]
    async fn test_table_pool() {
        let tp = TestPool::new(2);
        let tids = tp.avail_ids().await;

        // Player 1 join table 1 that should be in first position.
        let p1 = TestPlayer::new();
        let t1 = tp.join(&p1).await.unwrap();
        assert_eq!(t1.table_id(), tids[0]);

        // Player 2 join table 1.
        let p2 = TestPlayer::new();
        let t1 = tp.join(&p2).await.unwrap();
        assert_eq!(t1.table_id(), tids[0]);

        // As the table is full it should move to the full queue.
        let tids = tp.full_ids().await;
        assert_eq!(t1.table_id(), tids[0]);

        // Player 1 join table 2, table 2 should be at front of the queue.
        let tids = tp.avail_ids().await;
        let t2 = tp.join(&p1).await.unwrap();
        assert_eq!(t2.table_id(), tids[0]);

        // Player 2 join table 2.
        let t2 = tp.join(&p2).await.unwrap();
        assert_eq!(t2.table_id(), tids[0]);

        // Player 3 tries to join but there are no tables.
        let p3 = TestPlayer::new();
        assert!(tp.join(&p3).await.is_none());

        // Players 2 leaves table 1 that becomes ready because with one player left
        // the game ends (2 seats per table), table 1 should move to the available
        // queue when a play tries to join.
        t1.leave(&p2.peer_id).await;

        // Player 1 join table 2, not the join operation move the tables between
        // queue.
        let t2 = tp.join(&p1).await.unwrap();
        let tids = tp.avail_ids().await;
        assert_eq!(t2.table_id(), tids[0]);

        // Player 2 join table 2.
        let t2 = tp.join(&p2).await.unwrap();
        assert_eq!(t2.table_id(), tids[0]);
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
            t.leave(&p.peer_id).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::poker::TableId;

    struct TestTablesPool {
        pool: TablesPool,
        _shutdown_tx: broadcast::Sender<()>,
        _shutdown_rx: mpsc::Receiver<()>,
    }

    impl TestTablesPool {
        fn new(table_count: usize) -> Self {
            let signing_key = SigningKey::default();
            let db = Database::open_in_memory().unwrap();
            let (shutdown_tx, _) = broadcast::channel(1);
            let (shutdown_complete_tx, shutdown_rx) = mpsc::channel(1);

            let pool = TablesPool::new(
                table_count,
                2,
                Arc::new(signing_key),
                db,
                &shutdown_tx,
                &shutdown_complete_tx,
            );

            Self {
                pool,
                _shutdown_tx: shutdown_tx,
                _shutdown_rx: shutdown_rx,
            }
        }

        async fn join(&self, player: &TestPlayer) -> Option<Arc<Table>> {
            self.pool
                .join(&player.peer_id, "test", Chips::new(1_000_000), player.tx.clone())
                .await
                .ok()
        }

        async fn available_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.available.iter().map(|t| t.table_id()).collect()
        }

        async fn full_ids(&self) -> Vec<TableId> {
            let pool = self.pool.0.lock().await;
            pool.full.iter().map(|t| t.table_id()).collect()
        }

        async fn count_available(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.available.len()
        }

        async fn count_full(&self) -> usize {
            let pool = self.pool.0.lock().await;
            pool.full.len()
        }
    }

    struct TestPlayer {
        tx: mpsc::Sender<TableMessage>,
        _rx: mpsc::Receiver<TableMessage>,
        peer_id: PeerId,
    }

    impl TestPlayer {
        fn new() -> Self {
            let signing_key = SigningKey::default();
            let peer_id = signing_key.verifying_key().peer_id();
            let (tx, rx) = mpsc::channel(64);
            Self { tx, _rx: rx, peer_id }
        }
    }

    #[tokio::test]
    async fn test_basic_joining() {
        let test_pool = TestTablesPool::new(2);
        let initial_ids = test_pool.available_ids().await;

        let p1 = TestPlayer::new();
        let t1 = test_pool.join(&p1).await.unwrap();
        assert_eq!(t1.table_id(), initial_ids[0]);

        let p2 = TestPlayer::new();
        let t2 = test_pool.join(&p2).await.unwrap();
        assert_eq!(t2.table_id(), initial_ids[0]);

        let full_ids = test_pool.full_ids().await;
        assert_eq!(full_ids[0], t1.table_id());

        let t2_ids = test_pool.available_ids().await;
        let t3 = test_pool.join(&p1).await.unwrap();
        assert_eq!(t3.table_id(), t2_ids[0]);
    }

    #[tokio::test]
    async fn test_large_pool_behavior() {
        const TABLE_COUNT: usize = 1000;
        let test_pool = TestTablesPool::new(TABLE_COUNT);

        let mut players = Vec::new();
        for _ in 0..(TABLE_COUNT * 2) {
            let player = TestPlayer::new();
            let table = test_pool.join(&player).await.unwrap();
            players.push((player, table));
        }

        assert_eq!(test_pool.count_available().await, 0);
        assert_eq!(test_pool.count_full().await, TABLE_COUNT);

        for (player, table) in players {
            table.leave(&player.peer_id).await;
        }

        let one_player = TestPlayer::new();
        test_pool.join(&one_player).await.unwrap();

        assert_eq!(test_pool.count_available().await, TABLE_COUNT);
        assert_eq!(test_pool.count_full().await, 0);
    }
}