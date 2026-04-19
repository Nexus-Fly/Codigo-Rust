use anyhow::Result;
use tashi_vertex::{Context, Engine, KeySecret, Message, Options, Peers, Socket, Transaction};
use crate::types::SwarmMessage;

/// Thin wrapper around the Vertex consensus engine.
pub struct VertexEngine {
    engine: Engine,
}

impl VertexEngine {
    /// Start a Vertex node.
    ///
    /// * `secret_b58`  – Base58-encoded Ed25519 secret key for this node
    /// * `bind_addr`   – local UDP address, e.g. `"127.0.0.1:9000"`
    /// * `peers`       – list of `(addr, public_key_b58)` for other nodes
    pub async fn start(
        secret_b58: &str,
        bind_addr: &str,
        peers: Vec<(String, String)>,
    ) -> Result<Self> {
        let key: KeySecret = secret_b58.parse()?;

        let mut peer_set = Peers::new()?;
        // Add remote peers
        for (addr, pub_b58) in &peers {
            let pub_key = pub_b58.parse()?;
            peer_set.insert(addr, &pub_key, Default::default())?;
        }
        // Add ourselves
        peer_set.insert(bind_addr, &key.public(), Default::default())?;

        let context = Context::new()?;
        let socket  = Socket::bind(&context, bind_addr).await?;
        let options = Options::default();

        let engine = Engine::start(&context, socket, options, &key, peer_set, false)?;

        Ok(Self { engine })
    }

    /// Broadcast a `SwarmMessage` as a Vertex transaction.
    pub fn send(&self, msg: &SwarmMessage) -> Result<()> {
        let bytes = msg.to_bytes()?;
        let mut tx = Transaction::allocate(bytes.len());
        tx.copy_from_slice(&bytes);
        self.engine.send_transaction(tx)?;
        Ok(())
    }

    /// Receive the next consensus-ordered `SwarmMessage`.
    /// Returns `None` when the engine shuts down.
    pub async fn recv(&self) -> Result<Option<SwarmMessage>> {
        loop {
            match self.engine.recv_message().await? {
                None => return Ok(None),
                Some(Message::SyncPoint(_)) => continue,
                Some(Message::Event(event)) => {
                    // Take the first transaction from the event
                    for tx in event.transactions() {
                        match SwarmMessage::from_bytes(tx) {
                            Ok(msg) => return Ok(Some(msg)),
                            Err(e) => {
                                tracing::warn!("Failed to decode transaction: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}
