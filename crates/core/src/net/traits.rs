use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::message::SignedMessage;

#[async_trait]
pub trait NetTx: Send + Sync {
    async fn send(&mut self, msg: SignedMessage,) -> anyhow::Result<(),>;
    async fn send_table(&mut self, msg: TableMessage,) -> anyhow::Result<(),>;
}

#[async_trait]
pub trait NetRx: Send + Sync {
    /// Returns `None` when the stream is closed.
    async fn try_recv(&mut self,) -> anyhow::Result<SignedMessage,>;
}

#[async_trait]
impl<T,> NetTx for Box<T,>
where T: NetTx + ?Sized + Send /* forward to any NetTx */
{
    async fn send(&mut self, msg: SignedMessage,) -> anyhow::Result<(),> {
        (**self).send(msg,).await
    }

    async fn send_table(&mut self, msg: TableMessage,) -> anyhow::Result<(),> {
        (**self).send_table(msg,).await
    }
}

#[derive(Debug,)]
pub enum TableMessage {
    Send(SignedMessage,),
    PlayerLeave,
    Throttle(Duration,),
    Close,
}

/// channel based network transmitter
#[derive(Clone,)]
pub struct ChannelNetTx {
    pub tx: Sender<TableMessage,>,
}

impl ChannelNetTx {
    #[must_use]
    pub const fn new(tx: Sender<TableMessage,>,) -> Self {
        Self { tx, }
    }
}

#[async_trait::async_trait]
impl NetTx for ChannelNetTx {
    async fn send(&mut self, msg: SignedMessage,) -> anyhow::Result<(),> {
        // forward every SignedMessage through the existing channel
        self.tx
            .send(TableMessage::Send(msg,),)
            .await
            .map_err(|e| anyhow::anyhow!("channel closed: {e}"),)
    }

    async fn send_table(&mut self, msg: TableMessage,) -> anyhow::Result<(),> {
        self.tx
            .send(msg,)
            .await
            .map_err(|e| anyhow::anyhow!("channel closed: {e}"),)
    }
}
