// code taken from https://github.com/vincev/freezeout

//! poker server entry point
// "In computer programming, an entry point is the place in a program where the execution of a program begins, and where the program has access to command line arguments." - wikipedia

use anyhow::{Result, anyhow, bail};
use log::{error, info, warn};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    signal,
    sync::{broadcast, mpsc},
    time::{self, Duration},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig as TlsServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
    server::TlsStream,
};

use poker_core::{
    connection::{self, EncryptedConnection},
    crypto::{PeerId, SigningKey},
    message::{Message, SignedMessage},
    poker::Chips,
};

use crate::{
    db::Db,
    table::{Table, TableMessage},
    tables_pool::{TablesPool, TablesPoolsError},
};

#[derive(Debug)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    pub table_count: usize,
    pub seats_per_table: usize,
    pub data_path: Option<std::path::PathBuf>,
    pub key_path: Option<std::path::PathBuf>,
    pub cert_chain_path: Option<std::path::PathBuf>,
}

pub async fn start_server(config: ServerConfig) -> Result<()> {
    let bind_addr = format!("{}:{}", config.address, config.port);
    info!("Starting server on {bind_addr} with {} tables, {} seats/table", config.table_count, config.seats_per_table);

    let listener = TcpListener::bind(&bind_addr).await?;
    let signing_key = load_signing_key(&config.data_path)?;
    let database = Database::open(config.data_path)?;

    let tls_acceptor = match (&config.key_path, &config.cert_chain_path) {
        (Some(key), Some(chain)) => Some(load_tls(key, chain)?),
        _ => {
            warn!("TLS not enabled, using fallback encryption");
            None
        }
    };

    let shutdown_signal = signal::ctrl_c();
    let (shutdown_tx, _) = broadcast::channel(1);
    let (shutdown_done_tx, mut shutdown_done_rx) = mpsc::channel(1);

    let tables = TablesPool::new(
        config.table_count,
        config.seats_per_table,
        signing_key.clone(),
        database.clone(),
        &shutdown_tx,
        &shutdown_done_tx,
    );

    let mut server = PokerServer {
        listener,
        signing_key,
        database,
        tls_acceptor,
        tables,
        shutdown_tx,
        shutdown_done_tx,
    };

    tokio::select! {
        result = server.run() => {
            result.map_err(|e| anyhow!("TCP listener error: {e}"))?
        }
        _ = shutdown_signal => {
            info!("Shutdown signal received.");
        }
    }

    let PokerServer { shutdown_tx, shutdown_done_tx, .. } = server;
    drop(shutdown_tx);
    drop(shutdown_done_tx);
    let _ = shutdown_done_rx.recv().await;

    Ok(())
}
pub struct PokerServer {
listener: TcpListener,
signing_key: SigningKey,
database: Database,
tls_acceptor: Option<TlsAcceptor>,
tables: TablesPool,
shutdown_tx: broadcast::Sender<()>,
shutdown_done_tx: mpsc::Sender<()>,
}

impl PokerServer {
    async fn run(&mut self) -> Result<()> {
        loop {
            let (stream, addr) = self.accept_connection().await?;
            info!("New connection from {addr}");

            let handler = ConnectionHandler::new(
                self.tables.clone(),
                self.signing_key.clone(),
                self.database.clone(),
                self.shutdown_tx.subscribe(),
                self.shutdown_done_tx.clone(),
            );

            let tls_acceptor = self.tls_acceptor.clone();

            tokio::spawn(async move {
                let result = match tls_acceptor {
                    Some(acceptor) => acceptor.accept(stream).await.map_err(|e| anyhow!("TLS accept error: {e}"))
                        .and_then(|s| handler.handle_tls(s).await),
                    None => handler.handle_tcp(stream).await,
                };

                if let Err(e) = result {
                    error!("Connection {addr} failed: {e}");
                }
                info!("Connection {addr} closed");
            });
        }
    }

    async fn accept_connection(&self) -> Result<(TcpStream, SocketAddr)> {
        let mut retries = 0;
        loop {
            match self.listener.accept().await {
                Ok(conn) => return Ok(conn),
                Err(e) if retries < 5 => {
                    let wait = Duration::from_secs(1 << retries);
                    time::sleep(wait).await;
                    retries += 1;
                },
                Err(e) => return Err(e.into()),
            }
        }
    }
}
/// client connection handler.
impl ConnectionHandler {
    async fn handle_connection<S>(&self, conn: &mut EncryptedConnection<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (nickname, player_id) = self.receive_initial_join(conn).await?;
        let (table_msg_tx, mut table_msg_rx) = mpsc::channel::<TableMessage>(128);
        self.connection_loop(conn, player_id, nickname, table_msg_tx, &mut table_msg_rx).await
    }

    async fn receive_initial_join<S>(&self, conn: &mut EncryptedConnection<S>) -> Result<(String, PeerId)>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let message = tokio::select! {
            result = conn.recv() => match result {
                Some(Ok(msg)) => msg,
                Some(Err(err)) => return Err(err.into()),
                None => return Ok(()),
            },
            _ = self.shutdown_rx.recv() => return Ok(()),
        };

        match message.message() {
            Message::JoinServer { nickname } => {
                let player = self.database.upsert_player(message.sender(), nickname, Chips::new(1_000_000)).await?;
                let response = Message::ServerJoined {
                    nickname: player.nickname.clone(),
                    chips: player.chips.clone(),
                };
                conn.send(&SignedMessage::new(&self.signing_key, response)).await?;
                Ok((nickname.to_string(), message.sender()))
            }
            _ => bail!("Invalid initial message from {}: expected JoinServer", message.sender()),
        }
    }

    async fn connection_loop<S>(
        &self,
        conn: &mut EncryptedConnection<S>,
        player_id: PeerId,
        nickname: String,
        table_msg_tx: mpsc::Sender<TableMessage>,
        table_msg_rx: &mut mpsc::Receiver<TableMessage>,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        enum Incoming {
            FromClient(SignedMessage),
            FromTable(TableMessage),
            Shutdown,
        }

        loop {
            let next = tokio::select! {
                client_msg = conn.recv() => match client_msg {
                    Some(Ok(msg)) => Incoming::FromClient(msg),
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                },
                table_msg = table_msg_rx.recv() => match table_msg {
                    Some(msg) => Incoming::FromTable(msg),
                    None => return Ok(()),
                },
                _ = self.shutdown_rx.recv() => Incoming::Shutdown,
            };

            match next {
                Incoming::FromClient(msg) => self.handle_client_message(conn, &player_id, &nickname, msg, &table_msg_tx).await?,
                Incoming::FromTable(msg) => self.handle_table_message(conn, &player_id, msg).await?,
                Incoming::Shutdown => return Ok(()),
            }
        }
    }

    async fn handle_client_message<S>(
        &self,
        conn: &mut EncryptedConnection<S>,
        player_id: &PeerId,
        nickname: &str,
        msg: SignedMessage,
        table_msg_tx: &mpsc::Sender<TableMessage>,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        match msg.message() {
            Message::JoinTable => {
                let sufficient = self.database.deduct_chips(player_id, Chips::new(1_000_000)).await?;
                if !sufficient {
                    let notice = Message::NotEnoughChips;
                    conn.send(&SignedMessage::new(&self.signing_key, notice)).await?;
                    return Ok(());
                }

                match self.tables.join(player_id, nickname, Chips::new(1_000_000), table_msg_tx.clone()).await {
                    Ok(_) => {}
                    Err(TablesPoolError::AlreadyJoined) => {
                        let msg = Message::PlayerAlreadyJoined;
                        conn.send(&SignedMessage::new(&self.signing_key, msg)).await?;
                    }
                    Err(TablesPoolError::NoTablesLeft) => {
                        let msg = Message::NoTablesLeftNotication;
                        conn.send(&SignedMessage::new(&self.signing_key, msg)).await?;
                    }
                }
            }
            Message::LeaveTable => {
                self.tables.leave(player_id).await;
            }
            _ => {
                warn!("Unexpected message from {}", player_id);
            }
        }
        Ok(())
    }

    async fn handle_table_message<S>(
        &self,
        conn: &mut EncryptedConnection<S>,
        player_id: &PeerId,
        msg: TableMessage,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        match msg {
            TableMessage::Send(signed_msg) => {
                conn.send(&signed_msg).await?;
            }
            TableMessage::PlayerLeft => {
                let chips = self.database.get_or_refill_chips(player_id).await?;
                let msg = Message::ShowAccount { chips };
                conn.send(&SignedMessage::new(&self.signing_key, msg)).await?;
            }
            TableMessage::Throttle(duration) => {
                time::sleep(duration).await;
            }
            TableMessage::Close => {
                info!("Connection closing due to table signal");
                return Ok(());
            }
        }
        Ok(())
    }
}
