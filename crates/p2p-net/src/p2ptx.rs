pub struct P2pTx {
    swarm: Swarm<Gossipsub>,
    topic: Topic,
}

#[async_trait::async_trait]
impl NetTx for P2pTx {
    async fn send(&mut self, msg: SignedMessage) -> anyhow::Result<()> {
        let data = bincode::serialize(&msg)?;
        self.swarm.behaviour_mut().publish(self.topic.clone(), data)?;
        Ok(())
    }
}

