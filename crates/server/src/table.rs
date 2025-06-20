// Refactored poker table implementation for improved clarity and structure.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use log::{error, info};
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::SignedMessage;
use poker_core::poker::{Chips, TableId};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time;

use crate::db::Database;

mod player;
mod state;

pub use state::TableJoinError;

#[derive(Debug,)]
pub struct Table {
    command_sender: mpsc::Sender<TableCommand,>,
    table_id: TableId,
}

#[derive(Debug,)]
pub enum TableMessage {
    Send(SignedMessage,),
    PlayerLeave,
    Throttle(Duration,),
    Close,
}

enum TableCommand {
    TryJoin {
        player_id: PeerId,
        nickname: String,
        join_chips: Chips,
        table_tx: mpsc::Sender<TableMessage,>,
        response_tx: oneshot::Sender<Result<(), TableJoinError,>,>,
    },
    CanPlayerJoin {
        response_tx: oneshot::Sender<bool,>,
    },
    Leave(PeerId,),
    HandleMessage(SignedMessage,),
}

impl Table {
    pub fn new(
        seats: usize, signing_key: Arc<SigningKey,>, database: Database,
        shutdown_rx: broadcast::Receiver<(),>, shutdown_complete_tx: mpsc::Sender<(),>,
    ) -> Self {
        assert!(seats > 1, "A table must have at least two seats");

        let (command_sender, command_receiver,) = mpsc::channel::<TableCommand,>(128,);
        let table_id = TableId::new_id();

        let mut task = TableTask {
            table_id,
            seats,
            signing_key,
            database,
            command_receiver,
            shutdown_rx,
            shutdown_complete_tx,
        };

        tokio::spawn(async move {
            if let Err(err,) = task.run().await {
                error!("Table {} encountered an error: {}", task.table_id, err);
            }
            info!("Table task {} has been stopped", task.table_id);
        },);

        Self {
            command_sender,
            table_id,
        }
    }

    pub fn id(&self,) -> TableId {
        self.table_id
    }

    pub async fn can_player_join(&self,) -> bool {
        let (response_tx, response_rx,) = oneshot::channel();
        let sent = self
            .command_sender
            .send(TableCommand::CanPlayerJoin {
                response_tx,
            },)
            .await
            .is_ok();
        sent && response_rx.await.unwrap_or(false,)
    }

    pub async fn try_join(
        &self, player_id: &PeerId, nickname: &str, chips: Chips,
        table_tx: mpsc::Sender<TableMessage,>,
    ) -> Result<(), TableJoinError,> {
        let (response_tx, response_rx,) = oneshot::channel();

        self.command_sender
            .send(TableCommand::TryJoin {
                player_id: player_id.clone(),
                nickname: nickname.to_string(),
                join_chips: chips,
                table_tx,
                response_tx,
            },)
            .await
            .map_err(|_| TableJoinError::Unknown,)?;

        response_rx.await.map_err(|_| TableJoinError::Unknown,)?
    }

    pub async fn leave(&self, player_id: &PeerId,) {
        let _ = self.command_sender.send(TableCommand::Leave(player_id.clone(),),).await;
    }

    pub async fn handle_message(&self, msg: SignedMessage,) {
        let _ = self.command_sender.send(TableCommand::HandleMessage(msg,),).await;
    }
}

struct TableTask {
    table_id: TableId,
    seats: usize,
    signing_key: Arc<SigningKey,>,
    database: Database,
    command_receiver: mpsc::Receiver<TableCommand,>,
    shutdown_rx: broadcast::Receiver<(),>,
    shutdown_complete_tx: mpsc::Sender<(),>,
}

impl TableTask {
    async fn run(&mut self,) -> Result<(),> {
        let mut state = state::InternalTableState::new(
            self.table_id,
            self.seats,
            self.signing_key.clone(),
            self.database.clone(),
        );
        let mut ticker = time::interval(Duration::from_millis(500,),);

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => break Ok(()),
                _ = ticker.tick() => state.tick().await,
                command = self.command_receiver.recv() => {
                    match command {
                        Some(TableCommand::TryJoin { player_id, nickname, join_chips, table_tx, response_tx }) => {
                            let result = state.try_join(&player_id, &nickname, join_chips, table_tx).await;
                            let _ = response_tx.send(result);
                        }
                        Some(TableCommand::CanPlayerJoin { response_tx }) => {
                            let can_join = state.can_join();
                            let _ = response_tx.send(can_join);
                        }
                        Some(TableCommand::Leave(pid)) => {
                            state.leave(&pid).await;
                        }
                        Some(TableCommand::HandleMessage(msg)) => {
                            state.message(msg).await;
                        }
                        None => break Ok(()),
                    }
                }
            }
        }
    }
}
