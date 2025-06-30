// crates/p2p-net/src/lib.rs
pub mod swarm_task;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use poker_core::message::SignedMessage;
use poker_core::net::traits::TableMessage;
use poker_core::net::{NetRx, NetTx};
use tokio::sync::mpsc::{Receiver, Sender};


/// Outbound half
#[derive(Clone)]
pub struct P2pTx {
    sender: Sender<SignedMessage,>,
}

/// Inbound half
pub struct P2pRx {
    receiver: Receiver<SignedMessage,>,
}

/// ------------- NetTx implementation -----------------
#[async_trait]
impl NetTx for P2pTx {
    async fn send(&mut self, msg: SignedMessage,) -> Result<(),> {
        self.sender.send(msg,).await?;
        Ok((),)
    }

    async fn send_table(&mut self, msg: TableMessage,) -> Result<(),> {
        todo!()
    }
}

/// ------------- NetRx implementation -----------------
#[async_trait]
impl NetRx for P2pRx {
    async fn try_recv(&mut self,) -> Result<SignedMessage,> {
        match self.receiver.recv().await {
            Some(msg,) => Ok(msg,),
            None => Err(anyhow!("P2pRx closed"),),
        }
    }
}

/// Helper returned to caller so they can plug both halves into Player.
pub struct P2pTransport {
    pub tx: P2pTx,
    pub rx: P2pRx,
}


impl P2pTransport {
    
    pub async fn new(sender: Sender<SignedMessage>, receiver: Receiver<SignedMessage>) -> P2pTransport {
        P2pTransport {
            tx: P2pTx {
                sender
            },
            rx: P2pRx {
                receiver
            }
        }
    }
}
