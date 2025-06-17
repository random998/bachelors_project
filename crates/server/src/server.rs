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

use zkpoker_core::{
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
pub struct Config {
    pub listening_address: String, // server listening address.
    pub listening_port: u16, // server listening port.
    pub num_tables: usize, // number of tables on this server.
    pub num_seats_per_table: usize,
    pub data_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>, // TLS private key PEM path, on the meaning of 'PEM": https://serverfault.com/questions/9708/what-is-a-pem-file-and-how-does-it-differ-from-other-openssl-generated-key-file
    pub chain_path: Option<PathBuf>, // TLS certificate chain PEM path.
}

// server entry point , "In computer programming, an entry point is the place in a program where the execution of a program begins, and where the program has access to command line arguments." - wikipedia
pub async fn run(config: Config) -> Result<()> {
    let address = format!("{}:{}", config.listening_address, config.listening_port);
    info!(
        "Listening on {} with {} tables and {} seats per table",
        address, config.num_tables, config.num_seats_per_table
    );

    let listener = TcpListener::bind(address).await.map_err(|e| anyhow!("{}", e))?;
    let signing_key = load_signing_key(&config.data_path)?;
    let db = open_database(&config.data_path)?;
    let tls = match (config.key_path, config.chain_path) {
        (Some(key_path), Some(chain_path)) => Some(load_tls(&key_path, &chain_path)?),
        _ => {
            warn!("TLS not enabled, using NOISE encryption");
            None
        }
    };

    let shutdown_signal = signal::ctrl_c();
    let (shutdown_broadcast_tx, _) = broadcast::channel(1);
    let (shutdown_complete_tx, mut shutdown_complete_rx) = mpsc::channel(1);

    let tables = TablesPool::new(
        config.num_tables,
        config.num_seats_per_table,
        signing_key.clone(),
        db.clone(),
        &shutdown_broadcast_tx,
        &shutdown_complete_tx,
    );

    let mut server = Server {
        tables,
        signing_key,
        db,
        listener,
        tls,
        shutdown_broadcast_tx,
        shutdown_complete_tx,
    };

    tokio::select! { // select! Waits on multiple concurrent branches, returning when the first branch completes, cancelling the remaining branches.
        result = server.run() => {
            result.map_err(|e| anyhow!("tcp listener accept error: {}", e))?;
        }
        _ = shutdown_signal => {
            info!("shutdown signal received, shutting down.");
        }
    }
    // wait for all open connections to shut down.
    let Server {
        shutdown_broadcast_tx,
        shutdown_complete_tx,
        .. // 'Ignore the rest of the fields.'
    } = server;

    // Notify all connections to start the shutdown, then wait for all connections to terminate & drop their shutdown channel.
    drop(shutdown_broadcast_tx);
    drop(shutdown_complete_tx);
    let _ = shutdown_complete_rx.recv().await;
    Ok(())
}

/// server which handles client connections and state.
struct Server {
    tables: TablesPool, // tables associated/offered by this server.
    signing_key: SigningKey, // server signing key shared by all connections.
    db: Db, // database for storing the player data.
    listener: TcpListener, // server listener object.
    tls: Option<TlsAcceptor>,
    shutdown_broadcast_tx: broadcast::Sender<()>, // channel for sending shutdown notifications.
    shutdown_complete_tx: mpsc::Sender<()>, // shutdown sender (?) cloned by each connection.
}

impl Server {
    /// runs the server.
    async fn run(&mut self) -> Result<()> {
        loop {
            let (stream, addr) = self.accept_with_retry().await?;
            info!("Accepted connection from {}", addr);

            let mut handler = Handler {
                tables: self.tables.clone(),
                signing_key: self.signing_key.clone(),
                db: self.db.clone(),
                table: None,
                shutdown_broadcast_tx: self.shutdown_broadcast_tx.subscribe(), //TODO: what is the rationale behind the 'tx' naming. What does it mean to subscribe to a channel in this given context?
                _shutdown_complete_tx: self.shutdown_complete_tx.clone(),
            };
            let tls_acceptor = self.tls.clone();
            // spawn a task to handle accepting of connection messages.
            tokio::spawn(async move {
                let res = if let Some(acceptor) = tls_acceptor {
                    match acceptor.accept(stream).await {
                        Ok(stream) => handler.run_tls(stream).await,
                        Err(e) => Err(anyhow!("tls accept error: {}", e)),
                    }
                } else {
                    handler.run_tcp(stream).await
                };

                if let Err(err) = res {
                    error!("Connection to {addr} {err}")
                }
                info!("connection to {addr} closed.");
            });
        }
    }

    /// accepts a connection with retries.
    async fn accept_with_retry(&self) -> Result<(TcpStream, SocketAddr)> {
        let mut retry = 0;
        loop {
            match self.listener.accept().await {
                Ok((socket, address)) => {
                    return Ok((socket, address));
                }
                Err(e) => {
                    if retry >= 5 {
                        return Err(e.into());
                    }
                }
            }
            let num_seconds = 1 << retry; //TODO: what does this mean exactly?
            time::sleep(Duration::from_secs(num_seconds)).await;
            retry += 1;
        }
    }

    /// client connection handler.




}
