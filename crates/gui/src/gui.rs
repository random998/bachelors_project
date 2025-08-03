//  Top-level GUI glue (egui + our App model)
use std::clone::Clone;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use eframe::egui::{Context, Theme};
use p2p_net::runtime_bridge::UiHandle; // ← the two channels & runtime
use poker_cards::egui::Textures;
use poker_core::crypto::{KeyPair, PeerId};
use poker_core::game_state::GameState;
use poker_core::message::{EngineEvent, SignedMessage, UIEvent};
use poker_core::poker::TableId;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};

use crate::ConnectView;
// ─────────────────────────── CLI options (only on native) ───────────────
#[allow(missing_docs)]
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
#[allow(missing_docs)]
pub struct AppData {
    pub passphrase: String,
    pub nickname:   String,
}

// ─────────────────────────── App (shared by all views) ───────────────────

#[allow(missing_docs)]
pub trait Gui {
    fn send_ui_cmd(&mut self, cmd: UIEvent,);
    fn get_latest_snapshot(&self,) -> Option<GameState,>;
    fn handle_signed_message(&mut self, msg: SignedMessage,);
}

#[allow(missing_docs)]
pub struct App {
    // static
    pub textures: Textures,
    player_id:    PeerId,
    nickname:     String,
    key_pair:     KeyPair,

    // runtime ↔ GUI channels
    cmd_tx: mpsc::Sender<UIEvent,>, // GUI ➜ runtime
    msg_rx: mpsc::Receiver<EngineEvent,>, // runtime ➜ GUI
    _rt:    Arc<Runtime,>,          // keep Tokio alive!

    // latest immutable snapshot
    pub game_state: GameState,
}

impl App {
    #[allow(missing_docs)]
    pub fn update(&mut self,) {
        if let Ok(EngineEvent::Snapshot(gs),) = self.msg_rx.try_recv() {
                self.game_state = *gs;
        }
        // TODO: handle remaining UIEvent messages.
    }

    #[must_use]
    #[allow(missing_docs)]
    /// # Panics
    pub fn new(
        ui: UiHandle, // returned by `start`
        state: GameState,
        textures: Textures,
        key_pair: KeyPair,
    ) -> Self {
        let rt = ui._rt.unwrap();
        Self {
            key_pair,
            textures,
            cmd_tx: ui.cmd_tx.clone(),
            msg_rx: ui.msg_rx, // we *move* the receiver
            _rt: rt,
            player_id: state.player_id,
            nickname: String::new(),
            game_state: state,
        }
    }

    // ----------- message plumbing --------------------------------

    /// non-blocking pull from the runtime ➜ GUI channel
    pub fn try_recv_event(&mut self,) -> Option<EngineEvent,> {
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
    /// # Errors
    pub fn send_cmd_to_engine(
        &self,
        msg: UIEvent,
    ) -> Result<(), TrySendError<UIEvent,>,> {
        self.cmd_tx.try_send(msg,)
    }

    // ------------- helpers exposed to views ----------------------

    #[inline]
    #[allow(missing_docs)]
    #[must_use]
    pub const fn player_id(&self,) -> &PeerId {
        &self.player_id
    }
    #[inline]
    #[must_use]
    #[allow(missing_docs)]
    pub fn nickname(&self,) -> &str {
        &self.nickname
    }

    #[inline]
    #[must_use]
    #[allow(missing_docs)]
    pub fn key_pair(&self,) -> KeyPair {
        self.key_pair.clone()
    }

    /// latest immutable snapshot (cheap `Clone`)
    #[inline]
    #[must_use]
    pub fn snapshot(&self,) -> GameState {
        self.game_state.clone()
    }

}

// ─────────────────────────── Trait implemented by every view ─────────────

#[allow(missing_docs)]
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

#[allow(missing_docs)]
pub struct AppFrame {
    app:   App,
    panel: Box<dyn View,>,
}

impl AppFrame {
    #[must_use]
    #[allow(missing_docs)]
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