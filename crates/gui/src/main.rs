#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use eframe::egui;
use eframe::egui::ViewportBuilder;
use libp2p::Multiaddr;
use log::{info, trace};
use poker_core::crypto::{KeyPair, SigningKey, VerifyingKey};
use poker_core::poker::{Chips, TableId};
use poker_gui::gui;
use poker_table_engine::InternalTableState;

#[derive(Parser, Debug,)]
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

impl Options {
    pub fn table_id(&self,) -> TableId {
        TableId(self.table.parse().unwrap(),)
    }

    pub fn seed_addr(&self,) -> Option<Multiaddr,> {
        if self.seed_addr.is_empty() {
            info!("did not specify seed address, not parsing it.");
            None
        } else {
            Some(
                self.seed_addr
                    .clone()
                    
                    .as_str()
                    .parse()
                    .expect("failed to parse seed-addr",),
            )
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    eframe::WebLogger::init(log::LevelFilter::Debug,).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window",)
            .document()
            .expect("No document",);
        let canvas = document
            .get_element_by_id("canvas",)
            .expect("Failed to find Canvas Element",)
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Canvas was not a HtmlCanvasElement",);
        let server_url = document
            .get_element_by_id("server-url",)
            .expect("Failed to find server-address element",)
            .inner_html();

        let config = poker_gui::gui::Config { server_url, };
        eframe::WebRunner::new()
            .start(
                canvas,
                Default::default(),
                Box::new(|cc| {
                    Ok(Box::new(poker_gui::gui::AppFrame::new(config, cc,),),)
                },),
            )
            .await
            .expect("failed to start eframe",)
    },)
}

#[tokio::main]
#[cfg(not(target_arch = "wasm32"))]
async fn main() -> eframe::Result<(),> {
    let engine = Arc::new(tokio::sync::Mutex::new(
        start_engine().await.expect("failed to start engine",),
    ),);

    // network / timer loop
    {
        let engine_net = engine.clone();
        tokio::spawn(async move {
            if let Err(e,) = run_engine(engine_net,).await {
                eprintln!("engine task failed: {e:?}");
            }
        },);
    }

    // launch native UI on this thread
    start_ui(engine,)
}
fn start_ui(
    internal_table_state: Arc<tokio::sync::Mutex<InternalTableState,>,>,
) -> eframe::Result<(),> {
    let init_size = [1024.0, 640.0,];
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder {
            resizable: Some(true,),
            inner_size: Some(egui::vec2(800.0, 500.0,),),
            ..Default::default()
        }
        .with_inner_size(init_size,)
        .with_min_inner_size(init_size,)
        .with_max_inner_size(init_size,)
        .with_title("Cards",),
        ..Default::default()
    };

    let app_name = "zk-poker".to_string();

    info!("starting eframe");
    eframe::run_native(
        &app_name,
        native_options,
        Box::new(|cc| {
            Ok(Box::new(gui::AppFrame::new(cc, internal_table_state,),),)
        },),
    )
}

async fn run_engine(
    engine: Arc<tokio::sync::Mutex<InternalTableState,>,>,
) -> anyhow::Result<(),> {
    loop {
        {
            let mut eng = engine.try_lock()?;
            // 1) inbound network → engine
            while let Ok(message,) = eng.connection.rx.receiver.try_recv() {
                eng.handle_message(message,).await;
            }
            // 2) timers
            eng.tick().await;
        }
        // 3) quick nap
        tokio::time::sleep(Duration::from_millis(20,),).await;
    }
}

async fn start_engine() -> Result<InternalTableState, anyhow::Error,> {
    let opt = Options::parse();
    let keypair = load_or_generate_keypair(&opt.key_pair,).expect("err",);
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
        opt.table_id()
    );
    let transport =
        p2p_net::swarm_task::new(&opt.table_id(), keypair, opt.seed_addr(),);

    let mut engine = InternalTableState::new(
        transport,
        opt.table_id(),
        opt.seats,
        Arc::new(signing_key.clone(),),
    );

    // join myself (100 000 starting chips just for dev)
    engine
        .try_join(&signing_key.peer_id(), &opt.nick, Chips::new(100_000,),)
        .await
        .expect("TODO: panic message",);

    // dial known peer

    Ok(engine,)
}

// persistent key ----------------------------------------------------
fn load_or_generate_keypair(
    path: &std::path::Path,
) -> anyhow::Result<KeyPair,> {
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
