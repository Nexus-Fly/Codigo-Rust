#![allow(dead_code)]

use anyhow::Result;
use tashi_vertex::{Context, Engine, KeySecret, Message, Options, Peers, Socket, Transaction};

use crate::codec::{decode_message, encode_message};
use crate::config::AppConfig;
use crate::types::NexusMessage;

// ─────────────────────────────────────────────────────────────────────────────
// VertexNode
// ─────────────────────────────────────────────────────────────────────────────

/// Thin wrapper around the Tashi Vertex [`Engine`].
///
/// Encodes/decodes [`NexusMessage`] values automatically so callers never
/// touch raw `Transaction` bytes directly.
///
/// `main.rs` continues to own its own integration for now; this wrapper will
/// be wired in during a later phase.
pub struct VertexNode {
    engine: Engine,
}

impl VertexNode {
    /// Build a [`VertexNode`] directly from a loaded [`AppConfig`].
    ///
    /// Parses the secret key and peer list from the config file fields.
    pub async fn from_config(config: &AppConfig) -> Result<Self> {
        let key: KeySecret = config.secret_key.parse()?;
        let peers: Vec<(&str, &str)> = config
            .peers
            .iter()
            .map(|p| (p.address.as_str(), p.public_key.as_str()))
            .collect();
        Self::start(&config.bind, key, peers).await
    }

    /// Start a Vertex node: create context, bind socket, launch engine.
    ///
    /// `bind`  — local address to listen on, e.g. `"127.0.0.1:8001"`.
    /// `key`   — secret key for this node.
    /// `peers` — iterator of `(address, public_key_str)` pairs for known peers.
    ///           The local node is added automatically.
    pub async fn start<'a, I>(bind: &str, key: KeySecret, peers: I) -> Result<Self>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        // Build peer set, mirroring the pattern in main.rs exactly.
        let peer_list: Vec<(&str, &str)> = peers.into_iter().collect();
        let mut peer_set = Peers::with_capacity(peer_list.len() + 1)?;

        for (addr, pub_str) in &peer_list {
            let public = pub_str.parse()?;
            peer_set.insert(addr, &public, Default::default())?;
        }

        // Add ourselves.
        peer_set.insert(bind, &key.public(), Default::default())?;

        let context = Context::new()?;
        let socket = Socket::bind(&context, bind).await?;

        let mut options = Options::default();
        options.set_report_gossip_events(true);
        options.set_fallen_behind_kick_s(10);

        // false = start a new session (same flag as the working demo).
        let engine = Engine::start(&context, socket, options, &key, peer_set, false)?;

        Ok(Self { engine })
    }

    /// Encode a [`NexusMessage`] as JSON and submit it as a Vertex transaction.
    pub fn send_message(&self, msg: &NexusMessage) -> Result<()> {
        let payload = encode_message(msg)?;
        let mut tx = Transaction::allocate(payload.len());
        tx.copy_from_slice(&payload);
        self.engine.send_transaction(tx)?;
        Ok(())
    }

    /// Wait for the next consensus-ordered event and return all [`NexusMessage`]
    /// values found in its transactions.
    ///
    /// Returns `None` when the engine stream has closed.
    /// Returns an empty `Vec` for `SyncPoint` messages (no payload).
    pub async fn recv_messages(&self) -> Result<Option<Vec<NexusMessage>>> {
        match self.engine.recv_message().await? {
            None => Ok(None),
            Some(Message::SyncPoint(_)) => Ok(Some(Vec::new())),
            Some(Message::Event(event)) => {
                let mut out = Vec::new();
                for tx in event.transactions() {
                    match decode_message(&tx) {
                        Ok(msg) => out.push(msg),
                        // Non-NexusMessage transactions (e.g. plain PING from main.rs)
                        // are silently skipped so the wrapper stays backward-compatible.
                        Err(_) => {}
                    }
                }
                Ok(Some(out))
            }
        }
    }
}

