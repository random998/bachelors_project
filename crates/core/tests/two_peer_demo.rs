//! `tests/two_peer_demo.rs`
//! A fully in-memory integration test: two peers, one table.

use std::time::Duration;

use anyhow::Result;
use log::info;
use poker_core::crypto::KeyPair;
use poker_core::game_state::{HandPhase, Projection};
use poker_core::message::{SignedMessage, UiCmd};
use poker_core::net::traits::{P2pRx, P2pTransport, P2pTx};
use poker_core::poker::{Chips, TableId};
use tokio::time::sleep;

// Construct an in-memory ['P2pTransport'] from two unbounded channels.
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

//  Helper to pump messages between peers until no more are pending.
async fn pump_messages(alice: &mut Projection, bob: &mut Projection,) {
    loop {
        let mut progressed = false;
        alice.tick().await;
        if let Ok(msg,) = alice.try_recv() {
            info!("alice received message");
            alice.handle_network_msg(msg,).await;
            progressed = true;
        }
        bob.tick().await;
        if let Ok(msg,) = bob.try_recv() {
            info!("bob received message");
            bob.handle_network_msg(msg,).await;
            progressed = true;
        }

        if !progressed {
            info!("no one received message");
            break;
        }

        // small delay to simulate network latency.
        sleep(Duration::from_millis(10,),).await;
    }
}

/// Test successful join of two peers: seed (Alice) joins directly, non-seed
/// (Bob) sends SyncReq -> Alice processes, appends JoinTableReq, sends SyncResp
/// and ProtocolEntry
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_peers_join_success() -> Result<(),> {
    env_logger::init();
    // 1. Deterministic key pairs.
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    // 2. wire the two peers within in-memory pipes.
    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    // common table id
    let table_id = TableId::new_id();

    // 3. Create the two Projections.
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {}, // loopback callback not needed in test.
        true,   // Alice is seed.
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {}, // loopback callback not needed in test.
        false,  // Bob is not seed.
    );

    // 4. Alice joins her own instance directly via log entry.
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1_000,),
        },)
        .await;

    // pump to propagate Alice's ProtocolEntry (JoinTableReq)
    pump_messages(&mut alice, &mut bob,).await;

    // Assertions after Alice joins.
    assert_eq!(alice.players().len(), 1, "Alice should see herself");
    assert_eq!(
        bob.players().len(),
        1,
        "Bob should see Alice after processing her join"
    );
    assert_eq!(
        alice.hash_head(),
        bob.hash_head(),
        "Hashes match after Alice's join"
    );
    assert!(alice.has_joined_table());
    assert!(!bob.has_joined_table());
    assert_eq!(alice.phase, HandPhase::WaitingForPlayers);
    assert_eq!(bob.phase, HandPhase::WaitingForPlayers);

    // 5. Bob joins via SyncReq
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1_000,),
    },)
        .await;

    // pump to propagate: Bob sends SyncReq -> Alice processes, appends
    // JoinTableReq, sends SyncResp and ProtocolEntry.
    pump_messages(&mut alice, &mut bob,).await;

    // final assertions.
    assert_eq!(alice.players().len(), 2, "ALice sees both players");
    assert_eq!(bob.players().len(), 2, "Bob sees both players");
    assert_eq!(bob.hash_head(), bob.hash_head(), "Hashes match");
    assert!(bob.has_joined_table());
    assert_eq!(alice.chain().len(), 2, "Chain: Alice join + Bob join");
    assert_eq!(bob.chain().len(), 2, "bob replayed chain");
    assert_eq!(alice.phase, bob.phase, "phases match");

    Ok((),)
}
