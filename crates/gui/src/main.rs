//! crates/gui/src/main.rs
//! Stand-alone native launcher for the GUI.
//! (Web target handled separately via the `wasm32` block.)

#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use clap::Parser;
use eframe::egui::ViewportBuilder;
use libp2p::Multiaddr;
use p2p_net::runtime_bridge;
use poker_core::crypto::KeyPair;
use poker_core::poker::TableId;
use poker_gui::gui;
// <-- background runtime

// ───────────────────────── CLI flags ────────────────────────────

#[derive(Parser, Debug,)]
struct Options {
    #[arg(long, default_value = "1")]
    table:     String,
    #[arg(long, default_value = "default_nick")]
    nick:      String,
    #[arg(long, default_value = "")]
    seed_addr: String,
    #[arg(long, default_value_t = 3)]
    seats:     usize,
    #[arg(long, default_value = "peer.key")]
    key_pair:  PathBuf,
}

impl Options {
    fn table_id(&self,) -> TableId {
        TableId(self.table.parse().unwrap(),)
    }
    fn seed_addr(&self,) -> Option<Multiaddr,> {
        if self.seed_addr.is_empty() {
            None
        } else {
            Some(self.seed_addr.parse().expect("invalid multi-addr",),)
        }
    }
}

// ───────────────────────── main (native) ───────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub fn main() -> eframe::Result<(),> {
    env_logger::init();
    let opt = Options::parse();

    // load / generate permanent key-pair
    //    let kp = load_or_generate_keypair(&opt.key_pair,)
    // .expect("cannot load/generate keypair",);
    let kp = KeyPair::default();
    let peer_id = kp.public().to_peer_id();

    // spawn background runtime (net + engine)
    let ui = runtime_bridge::start(
        opt.table_id(),
        opt.seats,
        kp.clone(),
        opt.nick.clone(),
        opt.seed_addr(),
    );

    // launch egui
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1024.0, 640.0,],)
            .with_min_inner_size([1024.0, 640.0,],)
            .with_max_inner_size([1024.0, 640.0,],)
            .with_title("zk-poker",),
        ..Default::default()
    };

    eframe::run_native(
        "zk-poker",
        native_options,
        Box::new(|cc| {
            Ok(Box::new(gui::AppFrame::new(
                cc,
                ui,
                opt.nick.clone(), 
                peer_id,
                opt.seats,
                opt.table_id(),
                kp.clone()
            ),),)
        },),
    )
}