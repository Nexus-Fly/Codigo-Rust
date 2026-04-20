use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::AgentKind;

// ─────────────────────────────────────────────────────────────────────────────
// Structs
// ─────────────────────────────────────────────────────────────────────────────

/// A remote peer as listed in the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Human-readable label, e.g. "node2".
    pub id: String,
    /// Network address: "ip:port".
    pub address: String,
    /// Base58-encoded public key of this peer.
    pub public_key: String,
}

/// Full node configuration loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    // ── Identity ──────────────────────────────────────────────────────────
    /// Human-readable node label, e.g. "node1".
    pub node_id: String,
    /// Base58-encoded secret key for this node.
    pub secret_key: String,

    // ── Network ───────────────────────────────────────────────────────────
    /// Local address this node binds to, e.g. "127.0.0.1:8001".
    pub bind: String,
    /// Known peers.
    #[serde(default)]
    pub peers: Vec<PeerConfig>,

    // ── Agent properties ──────────────────────────────────────────────────
    /// Delivery agent type: Drone | Robot | Ebike.
    pub agent_kind: AgentKind,
    /// Operator or fleet owner name.
    pub vendor: String,
    /// Initial GPS latitude.
    pub x: f64,
    /// Initial GPS longitude.
    pub y: f64,
    /// Initial battery level 0–100.
    pub battery: u8,
    /// Max simultaneous orders this agent can carry.
    pub capacity: u8,
}

// ─────────────────────────────────────────────────────────────────────────────
// Loader
// ─────────────────────────────────────────────────────────────────────────────

/// Read and parse a TOML node config file.
///
/// ```no_run
/// use vertex_swarm_demo::config::load_config;
/// let cfg = load_config("config/node1.toml").unwrap();
/// println!("{}", cfg.bind);
/// ```
#[allow(dead_code)]
pub fn load_config(path: &str) -> Result<AppConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read config file: {path}"))?;
    toml::from_str(&raw)
        .with_context(|| format!("Failed to parse TOML config: {path}"))
}

