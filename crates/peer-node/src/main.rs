use std::path::PathBuf;
// crates/peer-node/src/main.rs
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use log::{info, trace};
use poker_core::crypto::{KeyPair, PeerId, SigningKey, VerifyingKey};
use poker_core::message::SignedMessage;
use poker_core::net::traits::P2pTransport;
use poker_core::net::{NetRx, NetTx};
use poker_core::poker::{Chips, TableId};
use poker_table_engine::{EngineCallbacks, InternalTableState};
use libp2p::Multiaddr;

/// CLI --------------------------------------------------------------
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

#[tokio::main]
async fn main() -> Result<(),> {
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
        opt.table_id().to_string()
    );
    let transport = p2p_net::swarm_task::new(&opt.table_id(), keypair, opt.seed_addr());

    let mut engine = InternalTableState::new(
        transport,
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
        r = run(engine) => { r?; }
    }
    Ok((),)
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

// run loop ----------------------------------------------------------
async fn run(
    mut engine: InternalTableState,
) -> Result<(),> {
    loop {
        // 1) inbound network → engine
        let msg = engine.connection.rx.try_recv().await;
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
