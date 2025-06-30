use std::path::PathBuf;
// crates/peer-node/src/main.rs
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use log::trace;
use p2p_net::P2pTransport;
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::SignedMessage;
use poker_core::net::{NetRx, NetTx};
use poker_core::poker::{Chips, TableId};
use table_engine::{EngineCallbacks, InternalTableState};

/// CLI --------------------------------------------------------------
#[derive(Parser, Debug,)]
struct Options {
    #[arg(long)]
    table: String,

    /// My nickname
    #[arg(long)]
    nick: String,

    /// Number of seats (only used by first peer that creates the table)
    #[arg(long, default_value_t = 6)]
    seats: usize,

    /// Path to my permanent signing key (is created if missing)
    #[arg(long, default_value = "peer.key")]
    key: PathBuf,
}

impl Options {
    pub fn table_id(&self,) -> TableId {
        TableId(self.table.parse().unwrap(),)
    }
}

#[tokio::main]
async fn main() -> Result<(),> {
    let opt = Options::parse();
    let signing_key = load_or_generate_key(&opt.key,)?;
    let peer_id = signing_key.verifying_key().peer_id();
    println!("peer-id = {peer_id}");

    // init logger
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info",),
    )
    .init();

    // ---------- P2P transport --------------------------------------
    let transport = p2p_net::swarm_task::new(&opt.table_id(), &signing_key,);

    // ---------- Engine --------------------------------------------
    let callbacks = Callbacks {
        tx: transport.tx.clone(),
    };
    let mut engine = InternalTableState::new(
        callbacks,
        opt.table_id(),
        opt.seats,
        std::sync::Arc::new(signing_key.clone(),),
    );

    // join myself (100 000 starting chips just for dev)
    engine
        .try_join(&peer_id, &opt.nick, Chips::new(100_000,),)
        .await?;

    // ---------- async run loop -------------------------------------
    tokio::select! {
        r = run(engine, transport) => { r?; }
    }
    Ok((),)
}

// persistent key ----------------------------------------------------
fn load_or_generate_key(path: &std::path::Path,) -> Result<SigningKey,> {
    use std::fs;
    use std::io::Write;
    if path.exists() {
        trace!("loading key from path: {} ...", path.display());
        let bytes = fs::read(path,)?;
        Ok(bincode::deserialize(&bytes,)?,)
    } else {
        trace!("loading default key...");
        let key = SigningKey::default();
        fs::File::create(path,)?
            .write_all(bincode::serialize(&key,)?.as_slice(),)?;
        Ok(key,)
    }
}

// run loop ----------------------------------------------------------
async fn run(
    mut engine: InternalTableState<Callbacks,>,
    mut transport: P2pTransport,
) -> Result<(),> {
    loop {
        // 1) inbound network → engine
        let msg = transport.rx.try_recv().await;
        while let Ok(ref message,) = msg {
            engine.handle_message(message.clone(),).await;
        }
        // 2) timers
        engine.tick().await;

        // 3) quick nap
        tokio::time::sleep(Duration::from_millis(20,),).await;
    }
}

#[derive(Clone,)]
struct Callbacks {
    tx: p2p_net::P2pTx,
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
