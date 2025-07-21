//! `tests/two_peer_demo.rs`
//! A fully in‑memory integration test: two peers, one table.

use poker_core::crypto::KeyPair;
use poker_core::game_state::Projection;
use poker_core::message::{SignedMessage, UiCmd};
use poker_core::net::traits::{P2pRx, P2pTransport, P2pTx};
use poker_core::poker::{Chips, TableId};

/// Construct an in‑process [`P2pTransport`] from two unbounded channels.
const fn mock_transport(
    outbound: tokio::sync::mpsc::Sender<SignedMessage,>,
    inbound: tokio::sync::mpsc::Receiver<SignedMessage,>,
) -> P2pTransport {
    P2pTransport {
        tx: P2pTx {
            network_msg_sender: outbound,
        },
        rx: P2pRx {
            network_msg_receiver: inbound,
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peers_join_and_start_game() -> anyhow::Result<(),> {
    // 1. deterministic key pairs
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    // 2. wire the two peers with in‑memory pipes
    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    // common table id so both peers end up in the same room
    let table = TableId::new_id();

    // 3. create the two projections
    let mut alice = Projection::new(
        "Alice".into(),
        table,
        6,
        kp_a.clone(),
        transport_a,
        |_| {}, // loopback callback not needed in test
        true,
    );
    let mut bob = Projection::new(
        "Bob".into(),
        table,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );

    // 4. Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: table,
            peer_id:  alice.peer_id(),
            nickname: "Alice".into(),
            chips:    Chips::new(1_000,),
        },)
        .await;

    // 5. pump once so her JoinTableReq reaches Bob
    alice.tick().await;
    bob.tick().await;

    // bob should have 0 players in his state.
    assert_eq!(bob.players().len(), 0);

    // 6. Bob processes Alice’s message and emits his own JoinTableReq
    if let Ok(msg,) = bob.try_recv() {
        bob.handle_network_msg(msg,).await;
    }
    bob.tick().await;

    // --- NEW: pump once more so Alice sees Bob’s message ------------------
    alice.tick().await;
    if let Ok(msg,) = alice.try_recv() {
        alice.handle_network_msg(msg,).await;
    }
    alice.tick().await;

    // 7. final assertions
    assert_eq!(alice.phase, bob.phase, "Both peers are in the same phase");
    assert_eq!(
        bob.players().len(),
        2,
        "{}",
        format!(
            "Bob should see both players, but sees the following players: {:?}",
            bob.players()
        )
    );
    assert_eq!(
        alice.players().len(),
        2,
        "{}",
        format!(
            "Alice should see both players, but sees the following players: {:?}",
            alice.players()
        )
    );

    Ok((),)
}
