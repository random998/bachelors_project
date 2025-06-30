//! crates/p2p-net/src/swarm_task.rs

use futures::StreamExt;
use libp2p::core::upgrade;
use libp2p::identity::Keypair;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{gossipsub, identify, noise, tcp, yamux, SwarmBuilder, Transport};
use poker_core::crypto::SigningKey;
use poker_core::poker::TableId;
use tokio::sync::mpsc;

use super::{NetTx, P2pRx, P2pTransport, P2pTx, Result, SignedMessage};
// ------------------------------------------------------------------
// 1  Types & Behaviour ---------------------------------------------

pub type SignedMsgBlob = Vec<u8,>;

#[derive(NetworkBehaviour,)]
// derives an enum called `BehaviourEvent` automatically
struct Behaviour {
    gossipsub: gossipsub::Behaviour,
    identify:  identify::Behaviour,
}

// ------------------------------------------------------------------
// 2  Constructor ----------------------------------------------------

pub fn new(table_id: &TableId, signing_key: &SigningKey,) -> P2pTransport {
    // identity ------------------------------------------------------
    let kp = identity_from_signing_key(signing_key,);
    let peer_id = kp.public().to_peer_id();

    // transport -----------------------------------------------------
    let transport =
        tcp::tokio::Transport::new(tcp::Config::default().nodelay(true,),)
            .upgrade(upgrade::Version::V1,)
            .authenticate(noise::Config::new(&kp,).unwrap(),)
            .multiplex(yamux::Config::default(),)
            .boxed();

    // behaviour -----------------------------------------------------
    let topic = gossipsub::IdentTopic::new(format!("poker/{table_id}"),);
    let gossip = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(kp.clone(),),
        gossipsub::Config::default(),
    )
    .unwrap();

    let behaviour = Behaviour {
        gossipsub: gossip,
        identify:  identify::Behaviour::new(identify::Config::new(
            "zk-poker/0.1".into(),
            kp.public(),
        ),),
    };

    let mut swarm = SwarmBuilder::with_existing_identity(kp,)
        .with_tokio() // runtime
        .with_tcp(
            // transport
            tcp::Config::default().nodelay(true,),
            (noise::Config::new, noise::Config::new,), // security (fn ptrs)
            yamux::Config::default,                    // multiplex (fn ptr)
        )
        .unwrap()
        .with_behaviour(|_k| behaviour,)
        .unwrap()
        .build(); // finish

    // Listen on all local interfaces – port chosen by OS
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap(),)
        .unwrap();
    // Subscribe to the gossipsub topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic,).unwrap();

    // mpsc pipes ---------------------------------------------------
    let (to_swarm_tx, mut to_swarm_rx,) = mpsc::channel::<SignedMessage,>(64,);
    let (from_swarm_tx, from_swarm_rx,) = mpsc::channel::<SignedMessage,>(64,);

    // background task ---------------------------------------------
    tokio::spawn(async move {
        loop {
            tokio::select! {
                /* outbound messages from game → swarm -------------- */
                Some(blob) = to_swarm_rx.recv() => {
                    // convert the typed message into bytes just before publish
                    let bytes = match bincode::serialize(&blob) {
                       Ok(b)=> b,
                        Err(e) => { log::error!("serialize: {e}"); continue; }
                    };
                    if let Err(e) =
                        swarm.behaviour_mut().gossipsub.publish(topic.clone(), bytes)
                    {
                        log::warn!("publish error: {e}");
                    }
                }

                /* inbound swarm events → game ---------------------- */
                event = swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(
                        BehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })
                    ) => {
                        if let Ok(msg) = bincode::deserialize::<SignedMessage>(&message.data) {
                            let _ = from_swarm_tx.send(msg).await;
                        }
                    }
                    _ => {}       // ignore everything else
                }
            }
        }
    },);

    // return the transport handle ---------------------------------
    P2pTransport {
        tx: P2pTx {
            sender: to_swarm_tx,
        },
        rx: P2pRx {
            receiver: from_swarm_rx,
        },
    }
}

// ------------------------------------------------------------------
// 3  Helper ---------------------------------------------------------
fn identity_from_signing_key(key: &SigningKey,) -> Keypair {
    let mut bytes = bincode::serialize(key,).unwrap();
    let bytes_arr = bytes.as_mut_slice();
    Keypair::ed25519_from_bytes(bytes_arr).unwrap()
}
