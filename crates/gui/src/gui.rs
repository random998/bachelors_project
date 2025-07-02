// code copied from https://github.com/vincev/freezeout

//! Freezeout Poker egui app implementation.

use std::path::PathBuf;
use std::sync::Arc;
use clap::Parser;
use eframe::Storage;
use eframe::egui::{Context, Theme};
use poker_cards::egui::Textures;
use poker_core::crypto::{KeyPair, PeerId, SigningKey};
use poker_core::message::{Message, SignedMessage};
use serde::{Deserialize, Serialize};
use libp2p::Multiaddr;
use log::{info, trace};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::TryLockError;
use poker_core::poker::{TableId};
use poker_table_engine::engine::InternalTableState;
use crate::{ConnectView};


#[derive(Parser, Debug,)]
struct Options {
    #[arg(long, default_value = "1")]
    table: String,

    /// My nickname
    #[arg(long, default_value = "default_nick")]
    nick: String,

    /// swarm discovery peer id
    #[arg(long, default_value = "")]
    seed_addr : String,

    /// Number of seats (only used by first peer that creates the table)
    #[arg(long, default_value_t = 6)]
    seats: usize,

    /// Path to my permanent signing key (is created if missing)
    #[arg(long, default_value = "peer.key")]
    key_pair: PathBuf,
}

impl Options {
    pub fn table_id(&self,) -> TableId {
        TableId(self.table.parse().unwrap(),)
    }
}

/// Data persisted across sessions.
#[derive(Debug, Serialize, Deserialize,)]
pub struct AppData {
    /// The last saved passphrase.
    pub passphrase: String,
    /// The last saved nickname.
    pub nickname:   String,
}

/// The application state shared by all views.
pub struct App {
    /// The app textures.
    pub textures: Textures,
    /// The application message signing key.
    sk:           SigningKey,
    /// This client player id.
    player_id:    PeerId,
    /// This client nickname
    nickname:     String,
    pub(crate) table_state: Arc<tokio::sync::Mutex<InternalTableState>>,
}

impl App {
    const STORAGE_KEY: &'static str = "appdata";
    fn new(internal_table_state: Arc<tokio::sync::Mutex<InternalTableState>>, textures: Textures) -> Self {
        let sk = SigningKey::default();
        Self {
            textures,
            player_id: sk.verifying_key().to_peer_id(),
            sk,
            nickname: String::default(),
            table_state: internal_table_state,
        }
    }

    /// This client player id.
    #[must_use]
    pub const fn player_id(&self,) -> &PeerId {
        &self.player_id
    }

    /// Try to pull one signed message from the engine without blocking.
    /// Returns `None` if (a) no message is available, or (b) the mutex
    /// is momentarily busy, or (c) it was poisoned.
    pub fn try_recv(&mut self) -> Option<SignedMessage> {
        match self.table_state.try_lock() {
            Ok(mut engine) => match engine.connection.rx.receiver.try_recv() {
                Ok(msg)                       => Some(msg),
                Err(TryRecvError::Empty)      => None,
                Err(TryRecvError::Disconnected) => {
                    log::warn!("engine RX channel closed");
                    None
                }
            },
            Err(_) => {
                // UI thread: just skip this frame.
                None
            }
        }
    }
    // Signs a message and *attempts* to send it immediately.
    /// If the lock is busy we fall back to a small local queue so the
    /// button-click never panics or blocks.
    pub fn sign_and_send(
        &mut self,
        msg: Message,
    ) -> Result<(), TrySendError<SignedMessage>> {
        let signed = SignedMessage::new(&self.sk, msg);

        match self.table_state.try_lock() {
            Ok(engine) => engine.connection.tx.sender.try_send(signed),
            Err(_TryLockError) => {
                // Could push to a VecDeque and flush it next frame, or
                // just return Err so caller can retry.
                Err(TrySendError::Closed(signed))
            }
        }
    }
    /// This client nickname.
    #[must_use]
    pub fn nickname(&self,) -> &str {
        &self.nickname
    }

    /// Get a value from the app storage.
    #[must_use]
    pub fn get_storage(
        &self,
        storage: Option<&dyn Storage,>,
    ) -> Option<AppData,> {
        storage
            .and_then(|s| eframe::get_value::<AppData,>(s, Self::STORAGE_KEY,),)
    }

    /// Set a value in the app storage.
    pub fn set_storage(
        &self,
        storage: Option<&mut (dyn Storage + 'static),>,
        data: &AppData,
    ) {
        if let Some(s,) = storage {
            eframe::set_value::<AppData,>(s, Self::STORAGE_KEY, data,);
            s.flush();
        }
    }
}

/// Traits for UI views.
pub trait View {
    /// Process a view update.
    fn update(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    );

    /// Returns the next view if any.
    fn next(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,>;
}

/// The UI main frame.
pub struct AppFrame {
    app:   App,
    panel: Box<dyn View,>,
}

impl AppFrame {
    /// Creates a new App instance.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_,>, table_state: Arc<tokio::sync::Mutex<InternalTableState>>) -> Self {
        cc.egui_ctx.set_theme(Theme::Dark,);

        let app = App::new(table_state, Textures::new(&cc.egui_ctx,),);
        let panel = Box::new(ConnectView::new(cc.storage, &app,),);

        Self { app, panel, }
    }
}

impl eframe::App for AppFrame {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame,) {
        self.panel.update(ctx, frame, &mut self.app,);

        if let Some(panel,) = self.panel.next(ctx, frame, &mut self.app,) {
            self.panel = panel;
            self.panel.update(ctx, frame, &mut self.app,);
        }
    }
}

// persistent key ----------------------------------------------------
fn load_or_generate_keypair(path: &std::path::Path,) -> anyhow::Result<KeyPair, > {
    use std::fs;
    use std::io::Write;

    // if path.exists() {
    //        trace!("loading key from path: {} ...", path.display());
    // let bytes = fs::read(path,)?;
    // Ok(bincode::deserialize::<KeyPair>(&bytes,)?,)
    trace!("loading default key...");
    let key = KeyPair::default();
    fs::File::create(path,)?
        .write_all(bincode::serialize(&key,)?.as_slice(),)?;
    Ok(key,)
    //     }
}
