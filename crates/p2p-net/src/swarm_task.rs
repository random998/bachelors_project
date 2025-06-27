//! crates/p2p-net/src/swarm_task.rs
use super::*;
use anyhow::Result;
use futures::StreamExt;
use libp2p::{
    core::upgrade,
    gossipsub,
    identity, noise, tcp, yamux,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    PeerId, Transport,
};
use poker_core::crypto::SigningKey;
use tokio::sync::mpsc;

pub type SignedMsgBlob = Vec<u8>;

/// Build and spawn; returns (tx, rx) halves for the game layer
pub fn new(table_id: &str, signing_key: &SigningKey) -> P2pTransport {
    /* 1 — libp2p identity */
    let kp       = identity_from_signing_key(signing_key);
    let peer_id  = kp.public().to_peer_id();

    /* 2 — TCP + Noise + Yamux transport */
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&kp).unwrap())
        .multiplex(yamux::Config::default())
        .boxed();

    /* 3 — gossipsub behaviour */
    let topic  = gossipsub::IdentTopic::new(format!("poker/{table_id}"));
    let gossip = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(kp.clone()),
        gossipsub::Config::default(),
    )
        .unwrap();

    #[derive(NetworkBehaviour)]
    struct Behaviour {
        gossipsub: gossipsub::Behaviour,
        identify : identify::Behaviour,
    }

    let behaviour = Behaviour {
        gossipsub: gossip,
        identify : identify::Behaviour::new(
            identify::Config::new("zk-poker/0.1".into(), kp.public())
        ),
    };

    let mut swarm = Swarm::with_tokio_executor(transport, behaviour, peer_id);

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .expect("listen");
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&topic)
        .unwrap();

    /* 4 — mpsc pipes */
    let (to_swarm_tx , mut to_swarm_rx) = mpsc::channel::<SignedMsgBlob>(64);
    let (from_swarm_tx, from_swarm_rx) = mpsc::channel::<SignedMessage>(64);

    /* 5 — background task */
    tokio::spawn(async move {
        loop {
            tokio::select! {
                /* 5.a outbound */
                Some(blob) = to_swarm_rx.recv() => {
                    let _ = swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(topic.clone(), blob)
                        .map_err(|e| log::warn!("publish error: {e}"));
                }

                /* 5.b inbound */
                event = swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(
                        BehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })
                    ) => {
                        if let Ok(msg) = bincode::deserialize::<SignedMessage>(&message.data) {
                            let _ = from_swarm_tx.send(msg).await;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    P2pTransport {
        tx: P2pTx { sender: to_swarm_tx },
        rx: P2pRx { receiver: from_swarm_rx },
    }
}

/* ---- helper ---------------------------------------------------------- */
fn identity_from_signing_key(key: &SigningKey) -> identity::Keypair {
    use libp2p::identity::ed25519;
    let secret = ed25519::SecretKey::try_from_bytes(key.to_bytes()).unwrap();
    identity::Keypair::Ed25519(secret.into())
}
