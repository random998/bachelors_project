// crates/p2p-net/src/lib.rs
pub mod swarm_task;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use poker_core::message::SignedMessage;
use poker_core::net::{NetTx, NetRx};

use tokio::sync::mpsc::{Sender, Receiver};
use poker_core::net::traits::TableMessage;
use crate::swarm_task::SignedMsgBlob;

/// Outbound half
pub struct P2pTx {
    sender: Sender<SignedMsgBlob>,
}

/// Inbound half
pub struct P2pRx {
    receiver: Receiver<SignedMessage>,
}

/// ------------- NetTx implementation -----------------
#[async_trait]
impl NetTx for P2pTx {
    async fn send(&mut self, msg: SignedMessage) -> Result<()> {
        let blob = msg.serialize();
        self.sender.send(blob).await?;
        Ok(())
    }

    async fn send_table(&mut self, msg: TableMessage) -> Result<()> {
        todo!()
    }
}

/// ------------- NetRx implementation -----------------
#[async_trait]
impl NetRx for P2pRx {
    async fn next_msg(&mut self) -> Result<SignedMessage> {
        match self.receiver.recv().await {
            Some(msg) => Ok(msg),
            None      => Err(anyhow!("P2pRx closed")),
        }
    }
}

/// Helper returned to caller so they can plug both halves into Player.
pub struct P2pTransport {
    pub tx: P2pTx,
    pub rx: P2pRx,
}
