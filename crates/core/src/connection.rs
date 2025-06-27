// code inspired by https://github.com/vincev/freezeout
// Improved version of EncryptedConnection module from https://github.com/vincev/freezeout
use std::sync::LazyLock;

/// TLS and Noise protocol encrypted WebSocket connection types.
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use snow::TransportState;
use snow::params::NoiseParams;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{self as websocket, MaybeTlsStream, WebSocketStream};

use crate::message::SignedMessage;
use crate::net::traits::TableMessage;
use crate::net::{NetRx, NetTx};

static NOISE_PARAMETERS: LazyLock<NoiseParams,> =
    LazyLock::new(|| "Noise_NN_25519_ChaChaPoly_BLAKE2s".parse().unwrap(),);

const MAX_MESSAGE_SIZE: usize = 1 << 12; // 4096 bytes

pub type ClientConnection = SecureWebSocket<MaybeTlsStream<TcpStream,>,>;

/// A Noise-encrypted WebSocket connection that exchanges `SignedMessages`.
pub struct SecureWebSocket<S,> {
    stream: WebSocketStream<S,>,
    noise_transport: TransportState,
}

impl<S,> SecureWebSocket<S,>
where S: AsyncRead + AsyncWrite + Unpin
{
    /// Send a signed message securely.
    pub async fn send(&mut self, message: &SignedMessage,) -> Result<(),> {
        let mut buffer = BytesMut::zeroed(MAX_MESSAGE_SIZE,);
        let message_len = self.noise_transport.write_message(&message.serialize(), &mut buffer,)?;

        self.stream.send(WsMessage::Binary(buffer.freeze().slice(..message_len,),),).await?;

        Ok((),)
    }

    pub async fn send_table(&mut self, message: &TableMessage,) -> Result<(),> {
        Err(anyhow!("tried to send table message to another player"),)
    }
    /// Receive a signed message securely.
    pub async fn receive(&mut self,) -> Option<Result<SignedMessage,>,> {
        let mut buffer = BytesMut::zeroed(MAX_MESSAGE_SIZE,);

        while let Some(msg_result,) = self.stream.next().await {
            match msg_result {
                | Ok(WsMessage::Binary(payload,),) => {
                    return Some(
                        self.noise_transport
                            .read_message(&payload, &mut buffer)
                            .map_err(anyhow::Error::from)
                            .and_then(|len| SignedMessage::deserialize_and_verify(&buffer[..len])),
                    );
                },
                | Ok(_,) => continue, // Ignore non-binary messages
                | Err(e,) => return Some(Err(anyhow!("WebSocket error: {e}"),),),
            }
        }

        None
    }

    /// Gracefully close the connection.
    pub async fn close(&mut self,) {
        let _ = self.stream.close(None,).await;
    }

    /// Accept an inbound encrypted WebSocket connection.
    pub async fn accept_connection(stream: S,) -> Result<Self,> {
        let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE_SIZE,),);
        let mut stream = websocket::accept_async_with_config(stream, Some(config,),).await?;

        let mut responder = snow::Builder::new(NOISE_PARAMETERS.clone(),).build_responder()?;
        let mut buffer = BytesMut::zeroed(MAX_MESSAGE_SIZE,);

        match stream.next().await {
            | Some(Ok(WsMessage::Binary(payload,),),) => {
                responder
                    .read_message(&payload, &mut buffer,)
                    .map_err(|e| anyhow!("Noise responder error: {e}"),)?;
            },
            | Some(Ok(_,),) => bail!("Expected binary message during Noise handshake"),
            | Some(Err(e,),) => bail!("WebSocket error during handshake: {e}"),
            | None => bail!("Connection closed during Noise handshake"),
        }

        let message_len = responder.write_message(&[], &mut buffer,)?;
        stream.send(WsMessage::Binary(buffer.freeze().slice(..message_len,),),).await?;

        Ok(Self {
            stream,
            noise_transport: responder.into_transport_mode()?,
        },)
    }

    /// Establish an outbound encrypted WebSocket connection.
    pub async fn connect_to(url: &str,) -> Result<ClientConnection,> {
        let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE_SIZE,),);

        let (mut stream, _,) =
            websocket::connect_async_with_config(url, Some(config,), false,).await?;

        let mut initiator = snow::Builder::new(NOISE_PARAMETERS.clone(),).build_initiator()?;
        let mut buffer = BytesMut::zeroed(MAX_MESSAGE_SIZE,);

        let len = initiator.write_message(&[], &mut buffer,)?;
        stream.send(WsMessage::Binary(buffer.freeze().slice(..len,),),).await?;

        match stream.next().await {
            | Some(Ok(WsMessage::Binary(payload,),),) => {
                let mut response_buf = BytesMut::zeroed(MAX_MESSAGE_SIZE,);
                initiator
                    .read_message(&payload, &mut response_buf,)
                    .map_err(|e| anyhow!("Noise initiator error: {e}"),)?;
            },
            | Some(Ok(_,),) => bail!("Expected binary message during Noise handshake"),
            | Some(Err(e,),) => bail!("WebSocket error during handshake: {e}"),
            | None => bail!("Connection closed during Noise handshake"),
        }

        Ok(SecureWebSocket {
            stream,
            noise_transport: initiator.into_transport_mode()?,
        },)
    }
}

#[async_trait]
impl<S,> NetTx for SecureWebSocket<S,>
where S: AsyncRead + AsyncWrite + Unpin + Send + Sync
{
    async fn send(&mut self, msg: SignedMessage,) -> Result<(),> {
        // re-use the existing method
        Self::send(self, &msg,).await
    }
    async fn send_table(&mut self, msg: TableMessage,) -> Result<(),> {
        Err(anyhow!("tried to send table message to another player"),)
    }
}

#[async_trait]
impl<S,> NetRx for SecureWebSocket<S,>
where S: AsyncRead + AsyncWrite + Unpin + Send + Sync
{
    async fn next_msg(&mut self,) -> Option<SignedMessage,> {
        match Self::receive(self,).await {
            | Some(res,) => Some(res.unwrap(),),
            | None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::{ClientConnection, SecureWebSocket};
    use crate::crypto::SigningKey;
    use crate::message::{Message, SignedMessage};
    use crate::poker::Chips;

    #[tokio::test]
    async fn encrypted_websocket_connection() {
        let addr = "127.0.0.1:12345";
        let (tx, rx,) = tokio::sync::oneshot::channel();

        let listener = TcpListener::bind(addr,).await.unwrap();
        tokio::spawn(async move {
            let (stream, _,) = listener.accept().await.unwrap();
            let mut con = SecureWebSocket::accept_connection(stream,).await.unwrap();

            let msg = con.receive().await.unwrap().unwrap();
            assert!(matches!(
                msg.message(),
                Message::JoinTableRequest { nickname, .. } if nickname == "Bob"
            ));

            let msg = con.receive().await.unwrap().unwrap();
            assert!(matches!(msg.message(), Message::ShowAccount { .. }));

            tx.send((),).unwrap();
        },);

        let url = format!("ws://{addr}");
        let mut con: ClientConnection = ClientConnection::connect_to(&url,).await.unwrap();
        let keypair = SigningKey::default();
        let peer_id = keypair.verifying_key().peer_id();

        let msg = SignedMessage::new(
            &keypair,
            Message::JoinTableRequest {
                player_id: peer_id,
                nickname: "Bob".to_string(),
            },
        );
        con.send(&msg,).await.unwrap();

        let msg = SignedMessage::new(
            &keypair,
            Message::ShowAccount {
                chips: Chips::ZERO,
            },
        );
        con.send(&msg,).await.unwrap();

        rx.await.unwrap();
    }
}
