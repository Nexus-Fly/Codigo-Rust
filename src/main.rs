mod agent;
mod auction;
mod crypto;
mod handoff;
mod healing;
mod payments;
mod safety;
mod types;
mod vertex_engine;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use types::{AgentType, NexusAgent, Point, SwarmMessage};
use agent::AgentRuntime;
use vertex_engine::VertexEngine;

/// NexusFly – P2P delivery coordination with trustless micropayments.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Base58-encoded Ed25519 secret key for this node.
    #[arg(short = 'K', long)]
    secret_key: String,

    /// Local bind address (host:port).
    #[arg(short = 'B', long, default_value = "127.0.0.1:9000")]
    bind_addr: String,

    /// Peer addresses in `<public_key_b58>@<host:port>` format (repeatable).
    #[arg(short = 'P', long)]
    peer: Vec<String>,

    /// Agent identifier.
    #[arg(long, default_value = "agent-001")]
    agent_id: String,

    /// Agent type: drone | robot | ebike.
    #[arg(long, default_value = "drone")]
    agent_type: String,

    /// Initial token balance.
    #[arg(long, default_value_t = 100)]
    balance: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();

    let agent_type = match cli.agent_type.as_str() {
        "robot" => AgentType::GroundRobot,
        "ebike" => AgentType::Ebike,
        _       => AgentType::Drone,
    };

    let agent = NexusAgent::new(
        &cli.agent_id,
        agent_type,
        "NexusFly",
        0.0, 0.0,
        100.0, 5.0,
        cli.balance,
    );

    tracing::info!("[{}] Starting – balance: {} tokens", agent.id, agent.balance);

    // Parse peers: "<pub_b58>@<addr>"
    let peers: Vec<(String, String)> = cli.peer.iter().map(|p| {
        let mut parts = p.splitn(2, '@');
        let pub_key = parts.next().unwrap_or("").to_string();
        let addr    = parts.next().unwrap_or("").to_string();
        (addr, pub_key)
    }).collect();

    let engine = VertexEngine::start(&cli.secret_key, &cli.bind_addr, peers).await?;
    let mut runtime = AgentRuntime::new(agent);

    // Broadcast initial state
    let state_msg = runtime.broadcast_state();
    engine.send(&state_msg)?;

    loop {
        // Tick heartbeat
        runtime.tick();

        // Receive next consensus message
        match engine.recv().await? {
            None => {
                tracing::info!("Engine shut down");
                break;
            }
            Some(msg) => {
                runtime.handle(msg);
                // Send any queued outbound messages
                for out in runtime.drain_outbox() {
                    engine.send(&out)?;
                }
            }
        }
    }

    Ok(())
}

