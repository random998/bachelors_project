use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

use poker_core::net::traits::P2pTx;
use poker_core::net::{NetRx, NetTx};
use poker_core::{
    crypto::{KeyPair, PeerId, SigningKey},
    message::SignedMessage,
    poker::{Chips, TableId},
};
use poker_table_engine::{EngineCallbacks, InternalTableState};

pub struct UiHandle {
    pub tx: mpsc::Sender<SignedMessage>,     // UI → engine
    pub rx: mpsc::Receiver<SignedMessage>,   // engine → UI
    _rt:  Arc<Runtime>,                      // keep runtime alive
}

/// Call once from the GUI thread.
pub fn start(
    table     : TableId,
    seats     : usize,
    keypair   : KeyPair,
    nickname  : String,
    seed_addr : Option<libp2p::Multiaddr>,
) -> UiHandle {
    // Independent runtime (multi-thread ⇒ doesn’t block GUI).
    let rt = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("net-rt")
            .build()
            .unwrap(),
    );
    let handle = rt.handle().clone();

    // Channels UI ↔ engine.
    let (tx_ui,  mut rx_ui ) = mpsc::channel::<SignedMessage>(128);
    let (_tx_net, rx_net)     = mpsc::channel::<SignedMessage>(128);

    // Spawn everything inside that runtime.
    handle.spawn(async move {
        // --- libp2p transport ------------------------------------
        let transport = crate::swarm_task::new(&table, keypair.clone(), seed_addr);

        // --- poker engine ----------------------------------------
        #[derive(Clone)]
        struct Cb { tx_gossip: P2pTx, tx_ui: mpsc::Sender<SignedMessage> }
        impl EngineCallbacks for Cb {
            fn send(&mut self, _dst: PeerId, m: SignedMessage) {
                let _ = self.tx_gossip.send(m.clone());
                let _ = self.tx_ui.try_send(m);
            }
            fn throttle(&mut self, player_id: PeerId, duration: Duration) {}
            fn disconnect(&mut self, peer_id: PeerId) {}
            fn credit_chips(&mut self, peer_id: PeerId, amount: Chips) -> anyhow::Result<()> { Ok(()) }
        }

        let signing = SigningKey::new(&keypair);
        let mut eng = InternalTableState::new(transport, table, seats, Arc::new(signing.clone()));
        eng.try_join(&signing.peer_id(), &nickname, Chips::new(100_000)).await.unwrap();

        // --- main loop -------------------------------------------
        loop {
            // 1) UI → engine
            while let Ok(m) = rx_ui.try_recv() { eng.handle_message(m).await; }
            // 2) gossip → engine
            while let Ok(m) = eng.connection.rx.receiver.try_recv() { eng.handle_message(m).await; }
            // 3) timeouts
            eng.tick().await;

            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });

    UiHandle { tx: tx_ui, rx: rx_net, _rt: rt }
}

