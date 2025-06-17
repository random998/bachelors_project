// code taken from https://github.com/vincev/freezeout

//! poker table implementation.
use anyhow::Result;
use log::{error, info};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time,
};

use freezeout_core::{
    crypto::{PeerId, SigningKey},
    message::SignedMessage,
    poker::{Chips, TableId},
};

use crate::db::Db;

mod player;
mod state;

pub use state::TableJoinError;

/// table state shared between players.
#[derive(Debug)]
pub struct Table {
    commands_tx: mpsc::Sender<TableCommand>, /// channel for sending commands.
    table_id: TableId,
}

/// message structure to send to players from table.
#[derive(Debug)]
pub enum TableMessage {
    /// sends signed message to client.
    Send(SignedMessage),
    /// instruct client to leave table.
    PlayerLeave,
    /// instruct client to introduce delay between messages.
    Throttle(Duration),
    /// Close a client connection.
    Close,
}

enum TableCommand {
    TryJoin {
        player_id: PeerId,
        nickname: String,
        join_chips: Chips,
        table_tx: mpsc::Sender<TableMessage>,
        response_tx: mpsc::Sender<Result<(), State:TableJoinError>>,
    },
    /// query if a player can join.
    CanPlayerJoin {response_tx: oneshot::Sender<bool>},
    /// leave this table.
    Leave(PeerId),
    /// handle a player's signed message.
    HandleMessage(SignedMessage)
}

impl Table {
    pub fn new(
          num_seats: usize,
          signing_key: Arc<SigningKey>,
          database: Db,
          shutdown_broadcast_rx: broadcast::Sender<()>,
          shutdown_complete_tx: mpsc::Sender<()>,
      ) -> Table {
          assert!(seats > 1, "at least two seats should be available");

          let (commands_tx, commands_rx) = mpsc::channel::<TableCommand>(128); //TODO: meaning of 128?
          let table_id = TableId::new_id();

          let mut task = TableTask {
              table_id,
              num_seats,
              signing_key,
              database,
              commands_rx,
              shutdown_broadcast_rx,
              _shutdown_complete_tx: shutdown_complete_tx,
          };

          tokio::spawn(async move {
              if let Err(err) = task.run().await {
                  error!("table {} error: {}", task.table_id, err);
              }
              info!("table task for table {} stopped", task.table_id);
          });

          Table {
              commands_tx,
              table_id,
          }
      }
      pub fn table_id(&self) -> TableId {
          self.table_id
      }

      pub async fn can_player_join(&self) -> bool {
          let (response_tx, response_rx) = oneshot::channel();
      }

}


}