use anyhow::{Error, Result, anyhow};
use async_trait::async_trait;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::message::SignedMessage;

#[async_trait]
pub trait NetTx: Send + Sync {
    async fn send(&mut self, msg: SignedMessage,) -> Result<(), Error,>;
}

#[async_trait]
pub trait NetRx: Send + Sync {
    /// Returns `None` when the stream is closed.
    async fn try_recv(&mut self,) -> Result<SignedMessage,>;
}

#[async_trait]
impl<T,> NetTx for Box<T,>
where T: NetTx + ?Sized + Send /* forward to any NetTx */
{
    async fn send(&mut self, msg: SignedMessage,) -> Result<(),> {
        (**self).send(msg,).await
    }
}

/// Helper returned to caller so they can plug both halves into Player.
#[derive(Debug,)]
pub struct P2pTransport {
    pub tx: P2pTx,
    pub rx: P2pRx,
}

impl P2pTransport {
    pub async fn new(
        sender: Sender<SignedMessage,>,
        receiver: Receiver<SignedMessage,>,
    ) -> Self {
        Self {
            tx: P2pTx { sender, },
            rx: P2pRx { receiver, },
        }
    }
}

/// Outbound half
#[derive(Clone, Debug,)]
pub struct P2pTx {
    pub sender: Sender<SignedMessage,>,
}

/// Inbound half
#[derive(Debug,)]
pub struct P2pRx {
    pub receiver: Receiver<SignedMessage,>,
}

/// ------------- NetTx implementation -----------------
#[async_trait]
impl NetTx for P2pTx {
    async fn send(&mut self, msg: SignedMessage,) -> Result<(), Error,> {
        self.sender
            .send(msg,)
            .await
            .map_err(|e| anyhow!("{:?}", e),)
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
