// code copied from https://github.com/vincev/freezeout

//! Freezeout Poker egui app implementation.

use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use clap::Parser;
use eframe::Storage;
use eframe::egui::{Context, Theme};
use poker_cards::egui::Textures;
use poker_core::crypto::{KeyPair, PeerId, SigningKey, VerifyingKey};
use poker_core::message::{Message, SignedMessage};
use serde::{Deserialize, Serialize};
use libp2p::Multiaddr;
use log::{info, trace};
use poker_core::net::{NetRx, NetTx};
use poker_core::net::traits::P2pTransport;
use poker_core::poker::{Chips, TableId};
use poker_table_engine::engine::InternalTableState;
use poker_table_engine::EngineCallbacks;
use crate::{ConnectView, Connection, ConnectionEvent};

/// App configuration parameters.
#[derive(Debug,)]
pub struct Config {
    /// seed peer id 
    pub seed_peer_multiaddr: Multiaddr,
}

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

    pub fn seed_addr(&self) -> Option<Multiaddr> {
        if self.seed_addr == "" {
            info!("did not specify seed address, not parsing it.");
            None
        } else {
            Some(self.seed_addr.clone().to_string().as_str().parse().expect("failed to parse seed-addr"))
        }
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
    /// The application configuration.
    pub config:   Config,
    /// The app textures.
    pub textures: Textures,
    /// The application message signing key.
    sk:           SigningKey,
    /// This client player id.
    player_id:    PeerId,
    /// This client nickname
    nickname:     String,
}

impl App {
    const STORAGE_KEY: &'static str = "appdata";

    fn new(config: Config, textures: Textures,) -> Self {
        let sk = SigningKey::default();
        Self {
            config,
            textures,
            player_id: sk.verifying_key().to_peer_id(),
            sk,
            nickname: String::default(),
        }
    }
    
    async fn init(config: Config) -> Result<(),>{
        let opt = Options::parse();
        let keypair = Self::load_or_generate_keypair(&opt.key_pair,).expect("err",);
        let signing_key: SigningKey = SigningKey::new(&keypair,);
        let pub_key: VerifyingKey = signing_key.verifying_key();
        println!("peer-id = {}", pub_key.to_peer_id());

        // init logger
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("info",),
        )
            .init();

        // ---------- P2P transport --------------------------------------
        info!(
        "creating p2p transport with table id: {}",
        opt.table_id().to_string()
    );
        let transport = p2p_net::swarm_task::new(&opt.table_id(), keypair, opt.seed_addr());

        let mut engine = InternalTableState::new(
            transport.tx.clone(),
            opt.table_id(),
            opt.seats,
            std::sync::Arc::new(signing_key.clone(),),
        );

        // join myself (100 000 starting chips just for dev)
        engine
            .try_join(&signing_key.peer_id(), &opt.nick, Chips::new(100_000,),)
            .await?;

        // dial known peer

        // ---------- async run loop -------------------------------------
        tokio::select! {
        r = run(engine, transport) => { r?; }
    }
        Ok((),)
    }

    /// This client player id.
    #[must_use]
    pub const fn player_id(&self,) -> &PeerId {
        &self.player_id
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

    // persistent key ----------------------------------------------------
    fn load_or_generate_keypair(path: &std::path::Path,) -> Result<KeyPair,> {
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
    pub fn new(config: Config, cc: &eframe::CreationContext<'_,>,) -> Self {
        cc.egui_ctx.set_theme(Theme::Dark,);

        info!("Creating new app with config: {config:?}");
        let app = App::new(config, Textures::new(&cc.egui_ctx,),);
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
// run loop ----------------------------------------------------------
async fn run(
    mut engine: InternalTableState,
    mut transport: P2pTransport,
) -> Result<(),> {
    loop {
        // 1) inbound network → engine
        let msg = transport.rx.try_recv().await;
        while let Ok(ref message,) = msg {
            info!("received message: {}", message.message());
            println!("received message: {}", message.message());
            engine.handle_message(message.clone(),).await;
        }
        // 2) timers
        engine.tick().await;

        // 3) quick nap
        tokio::time::sleep(Duration::from_millis(20,),).await;
    }
}

struct Callbacks {
    tx: poker_core::net::traits::P2pTx,
    rx: poker_core::net::traits::P2pRx,
}
impl EngineCallbacks for Callbacks {
    fn send(&mut self, _dest: PeerId, msg: SignedMessage,) {
        // broadcast – in gossipsub every peer gets everything anyway
        let _ = self.tx.send(msg,);
    }
    fn throttle(&mut self, _dest: PeerId, _dt: Duration,) {}
    fn disconnect(&mut self, _dest: PeerId,) {}
    fn credit_chips(
        &mut self,
        _dest: PeerId,
        _chips: Chips,
    ) -> Result<(), anyhow::Error,> {
        Ok((),)
    }
}
