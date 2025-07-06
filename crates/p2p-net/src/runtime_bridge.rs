//! Glue that runs the networking / engine stack on its own Tokio runtime
//! and exposes three channels so the GUI can …
//!   • push commands   ➜ `cmd_tx`  (`SignedMessage` → runtime)
//!   • observe echoes  ⇦ `msg_rx`  (`SignedMessage` ← runtime)
//!   • pull snapshots  ⇦ `state_rx`(`GameState`     ← runtime)

use std::sync::Arc;
use std::time::Duration;

use log::info;
use poker_core::crypto::{KeyPair, PeerId, SigningKey};
use poker_core::game_state::{GameState, InternalTableState};
use poker_core::message::{Message, SignedMessage};
use poker_core::net::traits::P2pTransport;
use poker_core::net::NetTx;
use poker_core::poker::{Chips, TableId};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

/// Handle returned to the GUI
pub struct UiHandle {
    /// GUI ➜ runtime  –  signed commands produced by user input
    pub cmd_tx:   mpsc::Sender<SignedMessage,>,
    /// runtime ➜ GUI  –  every locally generated or echoed message
    pub msg_rx:   mpsc::Receiver<SignedMessage,>,
    /// runtime ➜ GUI  –  immutable snapshot of the whole table state
    pub state_rx: mpsc::Receiver<GameState,>,
    pub _rt:      Arc<Runtime,>, // keep Tokio alive
}

/// Spawn the background runtime and give the GUI its three channel ends.
#[must_use]
pub fn start(
    peer_id: PeerId,
    table: TableId,
    seats: usize,
    kp: KeyPair,
    nick: String,
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
    let (cmd_tx, mut cmd_rx,) = mpsc::channel::<SignedMessage,>(64,);
    let (msg_tx, msg_rx,) = mpsc::channel::<SignedMessage,>(64,);
    let (state_tx, state_rx,) = mpsc::channel::<GameState,>(32,);

    // ─────────────────────── background task ───────────────────────────
    let rt_handle = rt.handle().clone();
    rt_handle.spawn(async move {
        // 1) transport (libp2p)
        let transport: P2pTransport =
            crate::swarm_task::new(&table, kp.clone(), seed,);

        // 2) engine
        let signing = SigningKey::new(&kp,);
        let signing_a = Arc::new(signing.clone(),);

        // 3) “loopback” closure → every *local* message is sent both
        // to the network layer and back to the GUI
        let mut tx_net = transport.tx.clone();
        let tx_ui = msg_tx.clone();
        let loopback = move |m: SignedMessage| {
            let _ = tx_net.send(m.clone(),); // to peers
            let _ = tx_ui.try_send(m,); // to GUI
        };

        let mut eng = InternalTableState::new(
            peer_id, table, seats, signing_a, transport, loopback,
        );

        // join ourselves with an initial chip-stack
        let _ = eng.sign_and_send(Message::PlayerJoinTableRequest {
            table_id:  table,
            player_id: peer_id,
            nickname:  nick.clone(),
            chips:     Chips::new(100_000,),
        },);

        // 4) main loop
        loop {
            // a) commands from GUI
            while let Ok(cmd,) = cmd_rx.try_recv() {
                eng.handle_message(cmd,);
            }

            // b) gossip → engine
            while let Ok(msg,) = eng.try_recv() {
                eng.handle_message(msg,);
            }

            // c) timers
            eng.tick().await;

            // d) ship an immutable snapshot to the GUI (non-blocking)
            let _ = state_tx.try_send(eng.snapshot(),);

            tokio::time::sleep(Duration::from_millis(16,),).await;
        }
    },);

    UiHandle {
        cmd_tx,
        msg_rx,
        state_rx,
        _rt: rt,
    }
}
