//! Config-driven MVP demo — loads a TOML node config and runs a local
//! end-to-end delivery scenario through the real App architecture.
//!
//! Run with:
//!   cargo run --bin mvp_config_demo -- --config config/node1.toml

use anyhow::Result;
use clap::Parser;
use vertex_swarm_demo::{
    app::App,
    config::{load_config, AppConfig},
    sim::runner::run_mvp_flow,
    types::AgentKind,
};

#[derive(Parser, Debug)]
#[command(name = "mvp_config_demo")]
#[command(about = "Config-driven local MVP delivery simulation")]
struct Args {
    /// Path to a TOML node config file (e.g. config/node1.toml)
    #[arg(short, long, default_value = "config/node1.toml")]
    config: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // ── CONFIG LOADED ────────────────────────────────────────────────────────
    section("CONFIG LOADED");
    let config = load_config(&args.config)?;
    println!("  file        : {}", args.config);
    println!("  node_id     : {}", config.node_id);
    println!("  bind        : {}", config.bind);
    println!("  agent_kind  : {:?}", config.agent_kind);
    println!("  location    : ({}, {})", config.x, config.y);
    println!("  battery     : {}%", config.battery);
    println!("  peers       : {}\n", config.peers.len());
    for p in &config.peers {
        println!("    - {} @ {}", p.id, p.address);
    }
    println!();

    // ── APP INITIALIZED ──────────────────────────────────────────────────────
    section("APP INITIALIZED");
    let app = App::from_config(config.clone())?;
    println!("  node_id  : {}", app.config.node_id);
    println!("  status   : {:?}", app.local_agent.status);
    println!("  location : ({:.4}, {:.4})\n", app.local_agent.location.0, app.local_agent.location.1);

    // ── LOCAL AGENT READY ────────────────────────────────────────────────────
    section("LOCAL AGENT READY");
    println!("  {} is ready to receive orders\n", app.config.node_id);

    // ── DEMO SCENARIO STARTED ────────────────────────────────────────────────
    section("DEMO SCENARIO STARTED");

    // Build a peer fleet: the loaded config is agent 0; add two synthetic
    // peers so run_mvp_flow has enough agents for the full flow (handoff etc.).
    let peer_a = synthetic_peer("sim-peer-a", AgentKind::Robot, 40.75, -73.99, 60);
    let peer_b = synthetic_peer("sim-peer-b", AgentKind::Ebike, 40.73, -74.00, 80);
    let configs: Vec<AppConfig> = vec![config, peer_a, peer_b];

    println!("  Running full MVP flow with {} agents...\n", configs.len());
    let result = run_mvp_flow(configs)?;

    // ── DEMO SCENARIO COMPLETED ──────────────────────────────────────────────
    section("DEMO SCENARIO COMPLETED");
    println!("  order id            : {}", result.order_id);
    println!("  auction winner      : {}", result.winner_id);
    println!("  final order status  : {:?}", result.final_order_status);
    println!("  delivering agent balance : {} units", result.delivering_agent_balance);
    println!(
        "  reauction candidates: {}",
        if result.reauction_candidates.is_empty() {
            "none".to_owned()
        } else {
            result.reauction_candidates.join(", ")
        }
    );
    println!(
        "  safety zone paused  : {}\n",
        if result.safety_paused { "yes" } else { "no" }
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn section(title: &str) {
    let pad = 44usize.saturating_sub(title.len());
    println!("-- {title} {}", "-".repeat(pad));
}

/// Minimal synthetic peer config (no secret key needed for local sim).
fn synthetic_peer(id: &str, kind: AgentKind, x: f64, y: f64, battery: u8) -> AppConfig {
    use vertex_swarm_demo::config::PeerConfig;
    AppConfig {
        node_id: id.to_owned(),
        secret_key: format!("sim-key-{id}"),
        bind: "127.0.0.1:0".to_owned(),
        peers: Vec::<PeerConfig>::new(),
        agent_kind: kind,
        vendor: "sim".to_owned(),
        x,
        y,
        battery,
        capacity: 1,
    }
}
