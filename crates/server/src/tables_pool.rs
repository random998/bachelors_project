// code inspired byhttps://github.com/vincev/freezeout
//! tables pool.
use anyhow::Result;
use log::error;
use std::{collections::VecDeque, sync::Arc};
use thiserror::Error;
use tokio::sync::{Mutex, broadcast, mpsc};

use freezeout_core::{
    crypto::{PeerId, SigningKey},
    poker::Chips,
};

use crate::{
    db::Db,
    table::{Table, TableJoinError, TableMessage},
};

/// error from tables pool join operations.
#[derive(Debug, Error)]
pub enum TablesError {
    #[error("no tables left")]
    NoTablesLeft,
    #[error("player already joined")]
    PlayerAlreadyJoined,
}

#[derive(Debug)]
pub struct TablesPool(Arc<Mutex<Shared>>);

#[derive(Debug)]
struct Shared {
    available_tables: VecDeque<Arc<Table>>,
    full_tables: VecDeque<Arc<Table>>,
}

impl TablesPool {
    pub fn new(
        num_tables: usize,
        num_seats: usize,
        signing_key: Arc<SigningKey>, // Arc: A thread-safe reference-counting pointer. ‘Arc’ stands for ‘Atomically Reference Counted’.
        database: Db,
        shutdown_broadcast_tx: &broadcast::Sender<()>,
        shutdown_complete_tx: &mpsc::Sender<()>,
    ) -> TablesPool {
        let available_tables = (0..num_tables).map(|_| {
            Arc::new(Table::new(
                num_seats,
                signing_key.clone(),
                database.clone(),
                shutdown_broadcast_tx.subscribe(),
                shutdown_complete_tx.clone(),
            ))
        }).collect();
        let shared_state = Shared {
            available_tables,
            full_tables: VecDeque::with_capacity(num_tables),
        };
        TablesPool(Arc::new(Mutex::new(shared_state)))
    }

    pub async fn join(
        &self,
        player_id: &PeerId,
        nickname: &str,
        join_chips: Chips,
        table_tx: mpsc::Sender<TableMessage>
    ) -> Result<Arc<Table>, TablesPoolError> {
        let mut pool = self.0.lock().await;

        // if there are no available tables, check if there is really no free seat at any of the tables.
        if pool.available.is_empty() {
            for _ in 0..pool.full_tables.len() {
                if let Some(table) = pool.available_tables.pop_front() {
                    if table.player_can_join().await {
                        pool.available_tables.push_back(table);
                    } else {
                        pool.full_tables.push_back(table);
                    }
                }
            }
        }

        if let Some(table) = pool.available_tables.front() {
            let result = tables.try_join(player_id, nickname, join_chips, table_tx.clone()).await;
            match result {
                Err(TableJoinError::PlayerAlreadyJoined) => {
                    return Err(TablesPoolError::PlayerAlreadyJoined);
                }
                Err(_) => {
                    return Err(TablesPoolError::NoTablesLeft);
                }
                _ => {}
            };

            // if no free seats are left at the table, move it to the full queue.
            if !table.player_can_join().await {
                let table = pool.available_tables.pop_front().unwrap();
                pool.full_tables.push_back(table.clone());
                Ok(table) //TODO: why not clone here?
            } else {
                Ok(table.clone())
            }
        } else {
            Err(TablesPoolError::NoTablesLeft)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freezeout_core::poker::TableId;

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