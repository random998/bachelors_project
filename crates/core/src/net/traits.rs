use anyhow::{Error, Result, anyhow};
use async_trait::async_trait;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::message::{SignedMessage, UiCmd};

use crate::game_state::GameState;

pub trait Gui {
    fn send_ui_cmd(&mut self, cmd: UiCmd);
    fn get_latest_snapshot(&self) -> Option<GameState>;
    fn handle_signed_message(&mut self, msg: SignedMessage);
}

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
            tx: P2pTx {
                network_msg_sender: sender,
            },
            rx: P2pRx {
                network_msg_receiver: receiver,
            },
        }
    }
}

/// Outbound half
#[derive(Clone, Debug,)]
pub struct P2pTx {
    pub network_msg_sender: Sender<SignedMessage,>,
}

/// Inbound half
#[derive(Debug,)]
pub struct P2pRx {
    pub network_msg_receiver: Receiver<SignedMessage,>,
}

/// ------------- NetTx implementation -----------------
#[async_trait]
impl NetTx for P2pTx {
    async fn send(&mut self, msg: SignedMessage,) -> Result<(), Error,> {
        self.network_msg_sender
            .send(msg,)
            .await
            .map_err(|e| anyhow!("{:?}", e),)
    }
}

/// ------------- NetRx implementation -----------------
#[async_trait]
impl NetRx for P2pRx {
    async fn try_recv(&mut self,) -> Result<SignedMessage,> {
        match self.network_msg_receiver.recv().await {
            Some(msg,) => Ok(msg,),
            _err => Err(anyhow!("{:?}", self),),
        }
    }
}
