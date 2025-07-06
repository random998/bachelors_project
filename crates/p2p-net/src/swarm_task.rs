//! crates/p2p-net/src/swarm_task.rs

use futures::StreamExt;
use libp2p::core::upgrade;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{
    gossipsub, identify, noise, tcp, yamux, Multiaddr, SwarmBuilder, Transport,
};
use log::info;
use poker_core::message::SignedMessage;
use poker_core::net::traits::{NetTx, P2pRx, P2pTransport, P2pTx};
use poker_core::poker::TableId;
use tokio::sync::mpsc;
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

pub fn new(
    table_id: &TableId,
    keypair: poker_core::crypto::KeyPair,
    seed_peer_addr: Option<Multiaddr,>,
) -> P2pTransport {
    // identity ------------------------------------------------------
    let peer_id = keypair.clone().to_peer_id();

    // transport -----------------------------------------------------
    let transport =
        tcp::tokio::Transport::new(tcp::Config::default().nodelay(true,),)
            .upgrade(upgrade::Version::V1,)
            .authenticate(
                noise::Config::new(&libp2p_identity::Keypair::from(
                    keypair.0.clone(),
                ),)
                .unwrap(),
            )
            .multiplex(yamux::Config::default(),)
            .boxed();

    // behaviour -----------------------------------------------------
    let topic = gossipsub::IdentTopic::new(format!("poker/{table_id}"),);
    let gossip = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(libp2p_identity::Keypair::from(
            keypair.0.clone(),
        ),),
        gossipsub::Config::default(),
    )
    .unwrap();

    let behaviour = Behaviour {
        gossipsub: gossip,
        identify:  identify::Behaviour::new(identify::Config::new(
            "zk-poker/0.1".into(),
            libp2p_identity::PublicKey::from(keypair.0.public(),),
        ),),
    };

    let mut swarm = SwarmBuilder::with_existing_identity(
        libp2p_identity::Keypair::from(keypair.0,),
    )
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

    let multiaddr: Multiaddr = "/ip4/0.0.0.0/tcp/0"
        .parse()
        .expect("failed parsing multiaddr",);

    // tell swarm to listen on all interfaces on a random, OS-assigned port.
    swarm
        .listen_on(multiaddr,)
        .expect("listen on /ip4/0.0.0.0/tcp/0 failed",);

    // Subscribe to the gossipsub topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic,).unwrap();

    // try to dial seed peer
    if let Some(addr,) = seed_peer_addr {
        swarm.dial(addr.clone(),).expect("dial failure",);
        info!("sucessfully dialed seed peer at {addr}");
    }

    // mpsc pipes ---------------------------------------------------
    let (to_swarm_tx, mut to_swarm_rx,) = mpsc::channel::<SignedMessage,>(64,);
    let (from_swarm_tx, from_swarm_rx,) = mpsc::channel::<SignedMessage,>(64,);

    if swarm.listeners().count() > 0 {
        info!(
            "{} listening on {}",
            peer_id,
            swarm.listeners().next().expect("no listeners for swarm")
        );
    }
    info!("{} subscribed to {}", peer_id, topic.hash());

    // background task ---------------------------------------------
    tokio::spawn(async move {
        loop {
            tokio::select! {
                /* outbound messages from game → swarm -------------- */
                Some(blob) = to_swarm_rx.recv() => {
                    // convert the typed message into bytes just before publish
                    info!("sending msg to gossipsub: {:?}", blob.message());
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
                event = swarm.select_next_some()
                => match event {
                    SwarmEvent::Behaviour(
                        BehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })
                    ) => {
                        if let Ok(msg) = bincode::deserialize::<SignedMessage>(&message.data) {
                            let _ = from_swarm_tx.send(msg).await;
                        }
                    }
                    _ => {
                        info!("received event from gossipsub: {event:?}");
                    }       // ignore everything else
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
