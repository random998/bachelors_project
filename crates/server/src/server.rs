// code taken from https://github.com/vincev/freezeout
// test.

//! poker server entry point
// "In computer programming, an entry point is the place in a program where the
// execution of a program begins, and where the program has access to command
// line arguments." - wikipedia

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use log::{error, info, warn};
use poker_core::connection::SecureWebSocket;
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::{Message, SignedMessage};
use poker_core::poker::Chips;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tokio::sync::mpsc::Sender;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{self, Duration};
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig as TlsServerConfig;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::db::Database;
use crate::table::{Table, TableMessage};
use crate::tables_pool::{TablesPool, TablesPoolError};

#[derive(Debug,)]
pub struct ServerConfig {
    /// ip address of the server
    pub address: String,
    /// port of the server
    pub port: u16,
    /// number of tables associated with the server
    pub table_count: usize,
    /// number of seats per table
    pub seats_per_table: usize,
    pub data_path: Option<PathBuf,>,
    pub key_path: Option<PathBuf,>,
    pub cert_chain_path: Option<PathBuf,>,
}

pub async fn start_server(config: ServerConfig,) -> Result<(),> {
    let bind_addr = format!("{}:{}", config.address, config.port);
    info!(
        "Starting server on {bind_addr} with {} tables, {} seats/table",
        config.table_count, config.seats_per_table
    );

    let listener = TcpListener::bind(&bind_addr,).await?;
    let signing_key = ConnectionHandler::load_signing_key(&config.data_path,)?;
    let database = ConnectionHandler::open_database(&config.data_path,)?;

    let tls_acceptor = match (&config.key_path, &config.cert_chain_path,) {
        | (Some(key,), Some(chain,),) => Some(ConnectionHandler::load_tls(key, chain,)?,),
        | _ => {
            warn!("TLS not enabled, using fallback encryption");
            None
        },
    };

    let shutdown_signal = signal::ctrl_c();
    let (shutdown_tx, _,) = broadcast::channel(1,);
    let (shutdown_done_tx, mut shutdown_done_rx,) = mpsc::channel(1,);

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

    let PokerServer {
        shutdown_tx,
        shutdown_done_tx,
        ..
    } = server;
    drop(shutdown_tx,);
    drop(shutdown_done_tx,);
    let _ = shutdown_done_rx.recv().await;

    Ok((),)
}
pub struct PokerServer {
    listener: TcpListener,
    signing_key: Arc<SigningKey,>,
    database: Database,
    tls_acceptor: Option<TlsAcceptor,>,
    tables: TablesPool,
    shutdown_tx: broadcast::Sender<(),>,
    shutdown_done_tx: Sender<(),>,
}

impl PokerServer {
    async fn run(&mut self,) -> Result<(),> {
        loop {
            let (stream, addr,) = self.accept_connection().await?;
            info!("New connection from {addr}");

            let mut handler = ConnectionHandler {
                tables: self.tables.clone(),
                signing_key: self.signing_key.clone(),
                database: self.database.clone(),
                table: None,
                shutdown_broadcast_rx: self.shutdown_tx.subscribe(),
                shutdown_complete_tx: self.shutdown_done_tx.clone(),
            };

            let tls_acceptor = self.tls_acceptor.clone();

            // Spawn a task to handle connection messages.
            tokio::spawn(async move {
                let res = if let Some(acceptor,) = tls_acceptor {
                    match acceptor.accept(stream,).await {
                        | Ok(stream,) => handler.run_tls(stream,).await,
                        | Err(e,) => Err(e.into(),),
                    }
                } else {
                    handler.run_tcp(stream,).await
                };

                if let Err(err,) = res {
                    error!("Connection to {addr} {err}");
                }

                info!("Connection to {addr} closed");
            },);
        }
    }

    async fn accept_connection(&self,) -> Result<(TcpStream, SocketAddr,),> {
        let mut retries = 0;
        loop {
            match self.listener.accept().await {
                | Ok(conn,) => return Ok(conn,),
                | Err(e,) if retries < 5 => {
                    let wait = Duration::from_secs(1 << retries,);
                    time::sleep(wait,).await;
                    retries += 1;
                },
                | Err(e,) => return Err(e.into(),),
            }
        }
    }
}
/// client connection handler.
struct ConnectionHandler {
    /// The tables on this server.
    tables: TablesPool,
    /// The server signing key shared by all connections.
    signing_key: Arc<SigningKey,>,
    /// The players Database.
    database: Database,
    /// This client table.
    table: Option<Arc<Table,>,>,
    /// Channel for listening shutdown notification.
    shutdown_broadcast_rx: broadcast::Receiver<(),>,
    /// Sender that drops when this connection is done.
    shutdown_complete_tx: Sender<(),>,
}

impl ConnectionHandler {
    const JOIN_TABLE_INITIAL_CHIP_BALANCE: Chips = Chips::new(1_000_000,);

    /// Handle TLS stream.
    async fn run_tls(&mut self, stream: TlsStream<TcpStream,>,) -> Result<(),> {
        let mut conn = SecureWebSocket::accept_connection(stream,).await?;
        let res = self.handle_connection(&mut conn,).await;
        conn.close().await;
        res
    }

    /// Handle unsecured stream.
    async fn run_tcp(&mut self, stream: TcpStream,) -> Result<(),> {
        let mut conn = SecureWebSocket::accept_connection(stream,).await?;
        let res = self.handle_connection(&mut conn,).await;
        conn.close().await;
        res
    }
    async fn handle_connection<S,>(&mut self, conn: &mut SecureWebSocket<S,>,) -> Result<(),>
    where S: AsyncRead + AsyncWrite + Unpin {
        self.connection_loop(conn,).await
    }

    async fn receive_initial_join<S,>(
        &mut self, conn: &mut SecureWebSocket<S,>, table_tx: Sender<TableMessage,>,
    ) -> Result<(String, PeerId,),>
    where S: AsyncRead + AsyncWrite + Unpin {
        let message = tokio::select! {
            result = conn.receive() => match result {
                Some(Ok(msg)) => msg,
                Some(Err(err)) => return Err(err),
                None => return Err(anyhow!("Connection closed")),
            },
            _ = self.shutdown_broadcast_rx.recv() => return Err(anyhow!("Connection closed")),
        };
        let (nickname, player_id,) = match message.message() {
            | Message::JoinServerRequest {
                nickname,
                player_id,
            } => {
                assert_eq!(
                    player_id.clone(),
                    message.sender(),
                    "id of message sender does not match stated id by message sender"
                );
                let player = self
                    .database
                    .join_server(message.sender(), nickname, Self::JOIN_TABLE_INITIAL_CHIP_BALANCE,)
                    .await?;

                // Notify client with the player account.
                let signed_message = SignedMessage::new(
                    &self.signing_key,
                    Message::JoinedServerConfirmation {
                        player_id: player_id.clone(),
                        nickname: player.nickname,
                        chips: player.chips,
                    },
                );

                conn.send(&signed_message,).await?;

                (nickname.to_string(), message.sender(),)
            },
            | _ => bail!("Invalid message from {} expecting a join server.", message.sender()),
        };
        Ok((nickname, player_id,),)
    }

    /// Handle connection messages.
    async fn connection_loop<S,>(&mut self, conn: &mut SecureWebSocket<S,>,) -> Result<(),>
    where S: AsyncRead + AsyncWrite + Unpin {
        // Wait for a JoinServer message from the client to join this server and get
        // the client nickname and player id.
        let msg = tokio::select! {
            res = conn.receive() => match res {
                Some(Ok(msg)) =>  msg,
                Some(Err(err)) => return Err(err),
                None => return Ok(()),
            },
            _ = self.shutdown_broadcast_rx.recv() => {
                return Ok(());
            }
        };

        let (nickname, player_id,) = match msg.message() {
            | Message::JoinServerRequest {
                nickname,
                player_id,
            } => {
                let player = self
                    .database
                    .join_server(msg.sender(), nickname, Self::JOIN_TABLE_INITIAL_CHIP_BALANCE,)
                    .await?;

                // Notify client with the player account.
                let smsg = SignedMessage::new(
                    &self.signing_key,
                    Message::JoinedServerConfirmation {
                        player_id: player_id.clone(),
                        nickname: player.nickname,
                        chips: player.chips,
                    },
                );

                conn.send(&smsg,).await?;

                (nickname.to_string(), msg.sender(),)
            },
            | _ => bail!("Invalid message from {} expecting a join server.", msg.sender()),
        };

        // Create channel to get messages from a table.
        let (table_tx, mut table_rx,) = mpsc::channel(128,);

        let res = loop {
            enum Branch {
                Conn(SignedMessage,),
                Table(TableMessage,),
            }

            let branch = tokio::select! {
                // We have received a message from the client.
                res = conn.receive() => match res {
                    Some(Ok(msg)) =>  Branch::Conn(msg),
                    Some(Err(err)) => break Err(err),
                    None => break Ok(()),
                },
                // We have received a message from the table.
                res = table_rx.recv() => match res {
                    Some(msg) => Branch::Table(msg),
                    None => break Ok(()),
                },
                // Server is shutting down exit this handler.
                _ = self.shutdown_broadcast_rx.recv() => break Ok(()),
            };

            match branch {
                | Branch::Conn(msg,) => match msg.message() {
                    | Message::JoinTableRequest {
                        player_id,
                        nickname,
                    } => {
                        // For now refill player chips if needed.
                        self.get_or_refill_chips(&player_id,).await?;

                        // Pay chips to joins a table.
                        let has_chips = self
                            .database
                            .deduct_chips(player_id.clone(), Self::JOIN_TABLE_INITIAL_CHIP_BALANCE,)
                            .await?;
                        if has_chips {
                            let res = self
                                .tables
                                .join(
                                    &player_id,
                                    &nickname,
                                    Self::JOIN_TABLE_INITIAL_CHIP_BALANCE,
                                    table_tx.clone(),
                                )
                                .await;
                            match res {
                                | Ok(table,) => self.table = Some(table,),
                                | Err(e,) => {
                                    // Refund chips and notify client.
                                    self.database
                                        .credit_chips(
                                            player_id.clone(),
                                            Self::JOIN_TABLE_INITIAL_CHIP_BALANCE,
                                        )
                                        .await?;

                                    let msg = match e {
                                        | TablesPoolError::NoTablesLeft => {
                                            Message::NoTablesLeftNotification
                                        },
                                        | TablesPoolError::PlayerAlreadyJoined => {
                                            Message::PlayerAlreadyJoined
                                        },
                                    };

                                    conn.send(&SignedMessage::new(&self.signing_key, msg,),)
                                        .await?;
                                },
                            };
                        } else {
                            // If this player doesn't have enough chips to join a
                            // table notify the client.
                            conn.send(&SignedMessage::new(
                                &self.signing_key,
                                Message::NotEnoughChips,
                            ),)
                                .await?;
                        }
                    },
                    | Message::PlayerLeftTable => {
                        if let Some(table,) = &self.table {
                            table.leave(&player_id,).await?
                        }
                    },
                    | _ => {
                        if let Some(table,) = &self.table {
                            table.handle_message(msg,).await;
                        }
                    },
                },
                | Branch::Table(msg,) => match msg {
                    | TableMessage::Send(msg,) => {
                        if let err @ Err(_,) = conn.send(&msg,).await {
                            break err;
                        }
                    },
                    | TableMessage::PlayerLeave => {
                        // If a player leaves the table reset the table and send
                        // updated player account information to the client.
                        self.table = None;

                        // Tell the client to show the account dialog.
                        let chips = self.get_or_refill_chips(&player_id,).await?;
                        let msg = Message::ShowAccount {
                            chips,
                        };
                        conn.send(&SignedMessage::new(&self.signing_key, msg,),).await?;
                    },
                    | TableMessage::Throttle(dt,) => {
                        time::sleep(dt,).await;
                    },
                    | TableMessage::Close => {
                        info!("Connection closed by table message");
                        break Ok((),);
                    },
                },
            }
        };

        if let Some(table,) = &self.table {
            table.leave(&player_id,).await?
        }

        res
    }

    async fn handle_client_message<S,>(
        &self, conn: &mut SecureWebSocket<S,>, player_id: &PeerId, msg: SignedMessage,
        table_msg_tx: &Sender<TableMessage,>,
    ) -> Result<(),>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        match msg.message() {
            | Message::JoinTableRequest {
                nickname,
                player_id,
            } => {
                let sufficient =
                    self.database.deduct_chips(*player_id, Chips::new(1_000_000,),).await?;
                if !sufficient {
                    let notice = Message::NotEnoughChips;
                    conn.send(&SignedMessage::new(&self.signing_key, notice,),).await?;
                    return Ok((),);
                }

                match self
                    .tables
                    .join(player_id, nickname, Chips::new(1_000_000,), table_msg_tx.clone(),)
                    .await
                {
                    | Ok(_,) => {},
                    | Err(TablesPoolError::PlayerAlreadyJoined,) => {
                        let msg = Message::PlayerAlreadyJoined;
                        conn.send(&SignedMessage::new(&self.signing_key, msg,),).await?;
                    },
                    | Err(TablesPoolError::NoTablesLeft,) => {
                        let msg = Message::NoTablesLeftNotification;
                        conn.send(&SignedMessage::new(&self.signing_key, msg,),).await?;
                    },
                }
            },
            | Message::PlayerLeftTable => {
                self.table.clone().unwrap().leave(player_id,).await?;
            },
            | _ => {
                warn!("Unexpected message from {}: {msg}", player_id);
            },
        }
        Ok((),)
    }

    async fn handle_table_message<S,>(
        &mut self, conn: &mut SecureWebSocket<S,>, player_id: &PeerId, msg: TableMessage,
    ) -> Result<(),>
    where S: AsyncRead + AsyncWrite + Unpin {
        match msg {
            | TableMessage::Send(signed_msg,) => {
                conn.send(&signed_msg,).await?;
            },
            | TableMessage::PlayerLeave => {
                let chips = self.get_or_refill_chips(player_id,).await?;
                let msg = Message::ShowAccount {
                    chips,
                };
                conn.send(&SignedMessage::new(&self.signing_key, msg,),).await?;
            },
            | TableMessage::Throttle(duration,) => {
                time::sleep(duration,).await;
            },
            | TableMessage::Close => {
                info!("Connection closing due to table signal");
                return Ok((),);
            },
        }
        Ok((),)
    }

    async fn get_or_refill_chips(&mut self, player_id: &PeerId,) -> Result<Chips,> {
        let mut player = self.database.get_player_by_id(*player_id,).await?;

        // For now refill player to be able to join a table.
        if player.chips < Self::JOIN_TABLE_INITIAL_CHIP_BALANCE {
            let refill = Self::JOIN_TABLE_INITIAL_CHIP_BALANCE - player.chips;
            self.database.credit_chips(*player_id, refill,).await?;
            player.chips = Self::JOIN_TABLE_INITIAL_CHIP_BALANCE;
        }

        Ok(player.chips,)
    }
    fn load_signing_key(path: &Option<PathBuf,>,) -> Result<Arc<SigningKey,>,> {
        fn load_or_create(path: &Path,) -> Result<Arc<SigningKey,>,> {
            let keypair_path = path.join("server.phrase",);
            let keypair = if keypair_path.exists() {
                info!("Loading keypair {}", keypair_path.display());
                let passphrase = std::fs::read_to_string(keypair_path,)?;
                SigningKey::from_phrase(&passphrase,)?
            } else {
                let keypair = SigningKey::default();
                std::fs::create_dir_all(path,)?;
                std::fs::write(&keypair_path, keypair.phrase().as_bytes(),)?;
                info!("Writing keypair {}", keypair_path.display());
                keypair
            };

            Ok(Arc::new(keypair,),)
        }

        // Load keypair from user path or try to create one if it doesn't exist.
        if let Some(path,) = path {
            load_or_create(path,)
        } else {
            let Some(proj_dirs,) = directories::ProjectDirs::from("", "", "zk_poker",) else {
                bail!("Cannot find project dirs");
            };

            load_or_create(proj_dirs.config_dir(),)
        }
    }

    fn open_database(path: &Option<PathBuf,>,) -> Result<Database,> {
        Self::load_database(path,)
    }

    fn load_or_create(path: &Path,) -> Result<Database,> {
        let database_path = path.join("game.Database",);
        if database_path.exists() {
            info!("Loading database {}", database_path.display());
            Database::open(database_path,)
        } else {
            std::fs::create_dir_all(path,)?;
            info!("Writing database {}", database_path.display());
            Database::open(database_path,)
        }
    }
    fn load_database(path: &Option<PathBuf,>,) -> Result<Database,> {
        // Load database from user path or try to create one if it doesn't exist.
        if let Some(path,) = path {
            Self::load_or_create(path,)
        } else {
            let Some(proj_dirs,) = directories::ProjectDirs::from("", "", "zk_poker",) else {
                bail!("Cannot find project dirs");
            };
            Self::load_or_create(proj_dirs.config_dir(),)
        }
    }

    fn load_tls(key_path: &PathBuf, chain_path: &PathBuf,) -> Result<TlsAcceptor,> {
        let key = PrivateKeyDer::from_pem_file(key_path,)?;
        let chain = CertificateDer::pem_file_iter(chain_path,)?.collect::<Result<Vec<_,>, _,>>()?;

        info!("Loaded TLS chain from {}", chain_path.display());
        info!("Loaded TLS key   from {}", key_path.display());

        let config =
            TlsServerConfig::builder().with_no_client_auth().with_single_cert(chain, key,)?;

        Ok(TlsAcceptor::from(Arc::new(config,),),)
    }
}
