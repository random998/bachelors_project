//! Glue that runs the networking / engine stack on its own Tokio runtime
//! and exposes three channels so the GUI can …
//!   • push commands   ➜ `cmd_tx`  (`SignedMessage` → runtime)
//!   • observe echoes  ⇦ `msg_rx`  (`SignedMessage` ← runtime)
//!   • pull snapshots  ⇦ `state_rx`(`GameState`     ← runtime)

use std::sync::Arc;
use std::time::Duration;

use poker_core::crypto::KeyPair;
use poker_core::game_state::Projection;
use poker_core::message::{SignedMessage, UiCmd, UiEvent};
use poker_core::net::traits::P2pTransport;
use poker_core::net::NetTx;
use poker_core::poker::TableId;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

/// Handle returned to the GUI
pub struct UiHandle {
    /// GUI ➜ runtime  –  signed commands produced by user input
    pub cmd_tx: mpsc::Sender<UiCmd,>,
    /// runtime ➜ GUI  –  every locally generated or echoed message
    pub msg_rx: mpsc::Receiver<UiEvent,>,
    pub _rt:    Arc<Runtime,>, // keep Tokio alive
}

/// Spawn the background runtime and give the GUI its three channel ends.
#[must_use]
pub fn start(
    table: TableId,
    seats: usize,
    kp: KeyPair,
    _nick: String,
    seed: Option<libp2p::Multiaddr,>,
) -> UiHandle {
    // ────────────────────────── Tokio runtime ───────────────────────────
    let rt = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("net-rt",)
            .build()
            .expect("cannot build Tokio runtime",),
    );

    // ─────────────── channels GUI ↔ runtime (unbounded) ────────────────
    let (cmd_tx, mut cmd_rx,) = mpsc::channel::<UiCmd,>(64,); // UI -> Runtime
    let (msg_tx, msg_rx,) = mpsc::channel::<UiEvent,>(64,); // Runtime -> UI

    // ─────────────────────── background task ───────────────────────────
    let rt_handle = rt.handle().clone();
    rt_handle.spawn(async move {
        // 1) transport (libp2p)
        let transport: P2pTransport =
            crate::swarm_task::new(&table, kp.clone(), seed,);

        // 2) engine

        // 3) “loopback” closure → every *local* message is sent both
        // to the network layer and back to the GUI
        let mut tx_net = transport.tx.clone();
        let loopback = move |m: SignedMessage| {
            let _ = tx_net.send(m,); // to peers
        };

        let mut eng = Projection::new(table, seats, kp, transport, loopback,);

        // 4) main loop
        loop {
            // a) GUI -> engine
            while let Ok(cmd,) = cmd_rx.try_recv() {
                eng.handle_ui_msg(cmd,).await;
            }

            // b) network → engine
            while let Ok(msg,) = eng.try_recv() {
                eng.handle_peer_msg(msg,).await;
            }

            // c) timers
            eng.tick().await;

            // d) engine -> Ui, send a snapshot of the engine.
            let sp = eng.snapshot();
            let msg = UiEvent::Snapshot(sp,);
            let _ = msg_tx.try_send(msg,);

            // e) update the internal table state periodically (handle state
            // transitions).
            let _ = eng.update();

            tokio::time::sleep(Duration::from_millis(16,),).await;
        }
    },);

    UiHandle {
        cmd_tx,
        msg_rx,
        _rt: rt,
    }
}
