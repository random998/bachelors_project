use super::*;
use libp2p::{gossipsub, identity, noise, tcp, yamux, core::upgrade, swarm::{NetworkBehaviour, Swarm, SwarmEvent}, PeerId, Multiaddr, Transport};
use tokio::sync::mpsc;
use poker_core::crypto::SigningKey;

pub type SignedMsgBlob = Vec<u8>;

/// Build and spawn; returns (tx, rx) half for the game layer
pub fn new(table_id: &str, signing_key: &SigningKey) -> P2pTransport {
    // ---------- 1. derive libp2p identity from your SigningKey ----------
    let kp = identity_from_signing_key(signing_key);           // helper below
    let peer_id = kp.public().to_peer_id();

    // ---------- 2. transport + noise + yamux ----------
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&kp).unwrap())
        .multiplex(yamux::Config::default())
        .boxed();

    // ---------- 3. gossipsub behaviour ----------
    let topic      = gossipsub::IdentTopic::new(format!("poker/{table_id}"));
    let gossipsub  = gossipsub::Gossipsub::new(
        gossipsub::MessageAuthenticity::Signed(kp.clone()),
        gossipsub::GossipsubConfig::default(),
    ).unwrap();

    #[derive(NetworkBehaviour)]
    struct Behaviour {
        gossipsub: gossipsub::Gossipsub,
        identify : identify::Identify,
    }

    let behaviour = Behaviour {
        gossipsub,
        identify: identify::Identify::new(
            identify::Config::new("zk-poker/0.1".into(), kp.public())
        ),
    };

    let mut swarm = Swarm::with_tokio_executor(transport, behaviour, peer_id);

    // Listen on all interfaces port 0 (let OS pick)
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
    swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();

    // ---------- 4. mpsc pipes ----------
    let (to_swarm_tx , mut to_swarm_rx) = mpsc::channel::<SignedMsgBlob>(64);
    let (from_swarm_tx, from_swarm_rx) = mpsc::channel::<SignedMessage>(64);

    // ---------- 5. spawn background loop ----------
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // 5.a outbound messages from game logic -> swarm publish
                Some(blob) = to_swarm_rx.recv() => {
                    if let Err(e) = swarm.behaviour_mut()
                        .gossipsub.publish(topic.clone(), blob) {
                        log::warn!("publish error: {e}");
                    }
                }

                // 5.b swarm events -> forward signed messages up
                event = swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(
                        BehaviourEvent::gossipsub(gossipsub::Event::Message { message, .. })
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

/// helper: derive libp2p keypair from your SigningKey bytes
fn identity_from_signing_key(key: &SigningKey) -> libp2p::identity::Keypair {
    use libp2p::identity::{ed25519, Keypair};
    let bytes = key.to_bytes();                  // 32-byte secret
    let secret = ed25519::SecretKey::try_from_bytes(bytes).unwrap();
    let kp = ed25519::Keypair::from(secret);
    Keypair::Ed25519(kp)
}
