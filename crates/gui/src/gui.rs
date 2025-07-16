//  Top-level GUI glue (egui + our App model)

use std::clone::Clone;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use eframe::Storage;
use eframe::egui::{Context, Theme};
use log::trace;
use p2p_net::runtime_bridge::UiHandle; // ← the two channels & runtime
use poker_cards::egui::Textures;
use poker_core::crypto::{KeyPair, PeerId};
use poker_core::game_state::GameState;
use poker_core::message::{UiCmd, UiEvent};
use poker_core::poker::TableId;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};

use crate::ConnectView;

// ─────────────────────────── CLI options (only on native) ───────────────

#[derive(Parser, Debug,)]
#[cfg(not(target_arch = "wasm32"))]
struct Options {
    #[arg(long, default_value = "1")]
    table: String,

    /// My nickname
    #[arg(long, default_value = "default_nick")]
    nick: String,

    /// swarm discovery peer id
    #[arg(long, default_value = "")]
    seed_addr: String,

    /// Number of seats (only used by first peer that creates the table)
    #[arg(long, default_value_t = 6)]
    seats: usize,

    /// Path to my permanent signing key (is created if missing)
    #[arg(long, default_value = "peer.key")]
    key_pair: PathBuf,
}

// ─────────────────────────── Persisted data (pass-phrase …) ─────────────

#[derive(Debug, serde::Serialize, serde::Deserialize,)]
pub struct AppData {
    pub passphrase: String,
    pub nickname:   String,
}

// ─────────────────────────── App (shared by all views) ───────────────────

pub struct App {
    // static
    pub textures: Textures,
    player_id:    PeerId,
    nickname:     String,
    key_pair:     KeyPair,

    // runtime ↔ GUI channels
    cmd_tx: mpsc::Sender<UiCmd,>, // GUI ➜ runtime
    msg_rx: mpsc::Receiver<UiEvent,>, // runtime ➜ GUI
    _rt:    Arc<Runtime,>,        // keep Tokio alive!

    // latest immutable snapshot
    pub game_state: GameState,
}

impl App {
    const STORAGE_KEY: &'static str = "appdata";

    pub fn update(&mut self,) {
        if let Ok(res,) = self.msg_rx.try_recv() {
            if let UiEvent::Snapshot(gs,) = res {
                self.game_state = gs;
            }
        }
        // TODO: handle remaining UIEvent messages.
    }

    #[must_use]
    pub fn new(
        ui: UiHandle, // returned by `start`
        state: GameState,
        textures: Textures,
        key_pair: KeyPair,
    ) -> Self {
        Self {
            key_pair,
            textures,
            cmd_tx: ui.cmd_tx.clone(),
            msg_rx: ui.msg_rx, // we *move* the receiver
            _rt: ui._rt,       // keep the runtime alive
            player_id: state.player_id,
            nickname: String::new(),
            game_state: state,
        }
    }

    // ----------- message plumbing --------------------------------

    /// non-blocking pull from the runtime ➜ GUI channel
    pub fn try_recv_event(&mut self,) -> Option<UiEvent,> {
        match self.msg_rx.try_recv() {
            Ok(m,) => Some(m,),
            Err(TryRecvError::Empty,) => None,
            Err(_,) => {
                log::warn!("runtime → GUI channel closed");
                None
            },
        }
    }

    /// UI -> runtime
    pub fn send_cmd_to_engine(
        &self,
        msg: UiCmd,
    ) -> Result<(), TrySendError<UiCmd,>,> {
        self.cmd_tx.try_send(msg,)
    }

    // ------------- helpers exposed to views ----------------------

    #[inline]
    #[must_use]
    pub const fn player_id(&self,) -> &PeerId {
        &self.player_id
    }
    #[inline]
    #[must_use]
    pub fn nickname(&self,) -> &str {
        &self.nickname
    }

    #[inline]
    #[must_use]
    pub fn key_pair(&self,) -> KeyPair {
        self.key_pair.clone()
    }

    /// latest immutable snapshot (cheap `Clone`)
    #[inline]
    #[must_use]
    pub fn snapshot(&self,) -> GameState {
        self.game_state.clone()
    }

    // ------------- (de)serialize tiny settings -------------------

    pub fn load_from_storage(&mut self, storage: Option<&dyn Storage,>,) {
        if let Some(data,) = storage
            .and_then(|s| eframe::get_value::<AppData,>(s, Self::STORAGE_KEY,),)
        {
            self.nickname = data.nickname;
        }
    }

    // Get a value from the app storage.
    #[must_use]
    pub fn get_storage(
        &self,
        storage: Option<&dyn Storage,>,
    ) -> Option<AppData,> {
        storage
            .and_then(|s| eframe::get_value::<AppData,>(s, Self::STORAGE_KEY,),)
    }

    pub fn save_to_storage(&self, storage: Option<&mut dyn Storage,>,) {
        if let Some(s,) = storage {
            let data = AppData {
                passphrase: String::new(),
                nickname:   self.nickname.clone(),
            };
            eframe::set_value::<AppData,>(s, Self::STORAGE_KEY, &data,);
            s.flush();
        }
    }
}

// ─────────────────────────── Trait implemented by every view ─────────────

pub trait View {
    fn update(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    );
    fn next(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,>;
}

// ─────────────────────────── Top-level egui frame ------------------------

pub struct AppFrame {
    app:   App,
    panel: Box<dyn View,>,
}

impl AppFrame {
    #[must_use]
    pub fn new(
        cc: &eframe::CreationContext<'_,>,
        ui: UiHandle,
        nickname: String,
        peer_id: PeerId,
        seats: usize,
        table_id: TableId,
        key_pair: KeyPair,
    ) -> Self {
        cc.egui_ctx.set_theme(Theme::Dark,);
        let textures = Textures::new(&cc.egui_ctx,);
        let app = App::new(
            ui,
            GameState::new(peer_id, table_id, nickname, seats,),
            textures,
            key_pair,
        );
        let panel = Box::new(ConnectView::new(&app,),);
        Self { app, panel, }
    }
}

impl eframe::App for AppFrame {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame,) {
        self.panel.update(ctx, frame, &mut self.app,);

        if let Some(next,) = self.panel.next(ctx, frame, &mut self.app,) {
            self.panel = next; // switch view
            self.panel.update(ctx, frame, &mut self.app,);
        }

        self.app.update();
    }
}

// ─────────────────────────── helper (native only) ------------------------

#[cfg(not(target_arch = "wasm32"))]
fn load_or_generate_keypair(
    path: &std::path::Path,
) -> anyhow::Result<KeyPair,> {
    use std::fs;
    use std::io::Write;
    trace!("loading default key …");
    let key = KeyPair::generate();
    fs::File::create(path,)?
        .write_all(bincode::serialize(&key,)?.as_slice(),)?;
    Ok(key,)
}
