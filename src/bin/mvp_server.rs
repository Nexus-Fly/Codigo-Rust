//! Long-running MVP delivery simulation server — no network required.
//!
//! Run with:
//!   cargo run --bin mvp_server
//!
//! Press Ctrl+C to stop gracefully.
//! Prints one or more log lines every second. New orders are generated
//! periodically, agents bid, winners are chosen, deliveries progress,
//! handoffs happen, battery drains, agents fail and recover.

use anyhow::Result;
use tokio::time::{interval, Duration};
use vertex_swarm_demo::{
    sim::runner::{WorldAgent, WorldSim},
    types::AgentKind,
    types::AgentStatus,
};

#[tokio::main]
async fn main() -> Result<()> {
    banner("MVP DELIVERY SERVER  (Ctrl+C to stop)");

    let agents = vec![
        world_agent("drone-1", AgentKind::Drone, 40.7100, -74.0100, 90),
        world_agent("robot-2", AgentKind::Robot, 40.7500, -73.9900, 70),
        world_agent("ebike-3", AgentKind::Ebike, 40.7300, -74.0000, 80),
        world_agent("drone-4", AgentKind::Drone, 40.7200, -74.0050, 60),
    ];

    println!("Agents online:");
    for a in &agents {
        println!(
            "  [{:?}] {}  @ ({:.4}, {:.4})  bat={}%",
            AgentKind::Drone, // kind not stored in WorldAgent, shown per-construction below
            a.id,
            a.location.0,
            a.location.1,
            a.battery_pct
        );
    }
    println!();

    // Deterministic seed — change to vary world behaviour.
    let mut sim = WorldSim::new(agents, 42);

    // Graceful Ctrl+C handling.
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let log = sim.tick();
                if !log.is_empty() {
                    println!("{log}");
                } else {
                    // Quiet tick — print a heartbeat every 10 ticks so the
                    // terminal doesn't appear frozen.
                    if sim.current_tick() % 10 == 0 {
                        println!("[t={}] ...", sim.current_tick());
                    }
                }
            }
            _ = &mut shutdown => {
                println!("\n[t={}] received Ctrl+C — shutting down", sim.current_tick());
                break;
            }
        }
    }

    banner("SERVER STOPPED");
    Ok(())
}

fn world_agent(id: &str, _kind: AgentKind, lat: f64, lon: f64, battery: u8) -> WorldAgent {
    WorldAgent {
        id: id.to_owned(),
        location: (lat, lon),
        battery_pct: battery,
        status: AgentStatus::Idle,
        offline_countdown: 0,
    }
}

fn banner(title: &str) {
    let line = "=".repeat(52);
    println!("\n{line}");
    println!("  {title}");
    println!("{line}\n");
}
