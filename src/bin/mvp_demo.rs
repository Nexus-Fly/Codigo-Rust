//! Local end-to-end MVP live simulation — no network required.
//!
//! Run with:
//!   cargo run --bin mvp_demo
//!
//! Prints one log line per second until the scenario completes.

use anyhow::Result;
use vertex_swarm_demo::{
    config::AppConfig,
    sim::runner::LiveSim,
    types::AgentKind,
};

#[tokio::main]
async fn main() -> Result<()> {
    banner("MVP DELIVERY LIVE SIMULATION");

    let configs = vec![
        make_config("drone-1", AgentKind::Drone, 40.71, -74.01, 90),
        make_config("robot-2", AgentKind::Robot, 40.75, -73.99, 60),
        make_config("ebike-3", AgentKind::Ebike, 40.73, -74.00, 80),
    ];

    println!("Agents online:");
    for c in &configs {
        println!(
            "  [{:?}] {}  @ ({:.4}, {:.4})  bat={}%",
            c.agent_kind, c.node_id, c.x, c.y, c.battery
        );
    }
    println!();

    let mut sim = LiveSim::new(configs);
    sim.run_live().await?;

    banner("SIMULATION COMPLETE");
    Ok(())
}

fn banner(title: &str) {
    let line = "=".repeat(52);
    println!("\n{line}");
    println!("  {title}");
    println!("{line}\n");
}

fn make_config(id: &str, kind: AgentKind, x: f64, y: f64, battery: u8) -> AppConfig {
    use vertex_swarm_demo::config::PeerConfig;
    AppConfig {
        node_id: id.to_owned(),
        secret_key: format!("demo-key-{id}"),
        bind: "127.0.0.1:0".to_owned(),
        peers: Vec::<PeerConfig>::new(),
        agent_kind: kind,
        vendor: "demo".to_owned(),
        x,
        y,
        battery,
        capacity: 1,
    }
}