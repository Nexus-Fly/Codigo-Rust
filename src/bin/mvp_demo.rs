//! Local end-to-end MVP simulation — no network required.
//!
//! Run with:
//!   cargo run --bin mvp_demo

use anyhow::Result;
use vertex_swarm_demo::{
    config::AppConfig,
    domain::{
        auction,
        handoff as handoff_domain,
        healing::HeartbeatTracker,
        ledger::Ledger,
        order as order_fsm,
        safety::SafetyMonitor,
    },
    types::{AgentKind, AgentStatus, AuctionBid, NodeId, SafetyZone, Timestamp},
};

const T: Timestamp = 1_700_000_000;
const PICKUP: (f64, f64) = (40.7128, -74.0060);
const DROPOFF: (f64, f64) = (40.7580, -73.9855);
const ESCROW: u64 = 300;
const HANDOFF_FEE: u64 = 100;
const HEARTBEAT_TIMEOUT: u64 = 30;

fn main() -> Result<()> {
    banner("MVP DELIVERY SIMULATION");

    // -- agents --------------------------------------------------------------
    let configs = vec![
        make_config("drone-1", AgentKind::Drone, 40.71, -74.01, 90),
        make_config("robot-2", AgentKind::Robot, 40.75, -73.99, 60),
        make_config("ebike-3", AgentKind::Ebike, 40.73, -74.00, 80),
    ];
    let agents: Vec<LocalAgent> = configs.iter().map(LocalAgent::from_config).collect();
    println!("Agents online:");
    for (a, c) in agents.iter().zip(configs.iter()) {
        println!(
            "  [{:?}] {}  @ ({:.4}, {:.4})  bat={}%",
            c.agent_kind, a.id, a.x, a.y, a.battery
        );
    }
    println!();

    // -- Step 1: ORDER CREATED -----------------------------------------------
    section("ORDER CREATED");
    let (mut order, _) = order_fsm::create_order(
        "mvp-order-1",
        agents[0].id.clone(),
        PICKUP,
        DROPOFF,
        T,
    );
    println!("  id      : {}", order.order_id);
    println!("  origin  : {}", order.origin);
    println!("  pickup  : ({:.4}, {:.4})", order.pickup.0, order.pickup.1);
    println!("  dropoff : ({:.4}, {:.4})", order.dropoff.0, order.dropoff.1);
    println!("  status  : {:?}\n", order.status);
    order = order_fsm::mark_bidding(order, T)?;

    // -- Step 2: BIDS RECEIVED -----------------------------------------------
    section("BIDS RECEIVED");
    let bids: Vec<AuctionBid> = agents.iter().map(|a| a.bid(&order.order_id)).collect();
    for b in &bids {
        println!("  {}  eta={}s  battery={}%", b.bidder, b.eta_s, b.battery_pct);
    }
    println!();

    // -- Step 3: WINNER CHOSEN -----------------------------------------------
    section("WINNER CHOSEN");
    let winner_id: NodeId = auction::choose_winner_id(&bids)
        .ok_or_else(|| anyhow::anyhow!("no bids submitted"))?;
    println!("  winner : {}\n", winner_id);

    // -- Step 4: ORDER ASSIGNED + escrow -------------------------------------
    section("ORDER ASSIGNED");
    let mut ledger = Ledger::new();
    ledger.credit(agents[0].id.as_str(), ESCROW);
    ledger.reserve_escrow(order.order_id.as_str(), agents[0].id.as_str(), ESCROW)?;
    let (o, _) = order_fsm::assign_order(order, winner_id.clone(), T)?;
    order = o;
    println!("  assigned_to : {}", winner_id);
    println!("  escrow      : {} units reserved\n", ESCROW);

    order = order_fsm::mark_pickup(order, T)?;
    order = order_fsm::mark_in_transit(order, T)?;
    println!("  [in transit] holder: {}\n", winner_id);

    // -- Step 5: HANDOFF COMPLETED -------------------------------------------
    section("HANDOFF COMPLETED");
    let dest = agents
        .iter()
        .find(|a| a.id != winner_id)
        .expect("at least two agents");

    let (o, _) = handoff_domain::create_handoff_request(
        order,
        winner_id.as_str(),
        dest.id.clone(),
        &AgentStatus::Idle,
        T,
    )?;
    order = o;
    ledger.transfer_for_handoff(
        order.order_id.as_str(),
        winner_id.as_str(),
        dest.id.as_str(),
        HANDOFF_FEE,
    )?;
    let (o, _) =
        handoff_domain::complete_handoff(order, dest.id.clone(), &AgentStatus::Idle, T)?;
    order = o;
    let delivering_agent = dest.id.clone();
    println!("  from     : {}", winner_id);
    println!("  to       : {}", delivering_agent);
    println!("  fee paid : {} units\n", HANDOFF_FEE);

    // -- Step 6: ORDER DELIVERED ---------------------------------------------
    section("ORDER DELIVERED");
    let (order, _) = order_fsm::mark_delivered(order, delivering_agent.clone(), T)?;
    println!("  delivered_by : {}", delivering_agent);
    println!("  status       : {:?}\n", order.status);

    // -- Step 7: LEDGER UPDATED ----------------------------------------------
    section("LEDGER UPDATED");
    ledger.release_final_payment(order.order_id.as_str(), delivering_agent.as_str())?;
    let balance = ledger.balances.get(&delivering_agent).copied().unwrap_or(0);
    println!("  {} credited {} units (final payment)\n", delivering_agent, balance);

    // -- Step 8: FAILED AGENTS -----------------------------------------------
    section("FAILED AGENTS");
    let failed_id = agents[0].id.clone();
    let mut tracker = HeartbeatTracker::new();
    for a in &agents {
        tracker.record_heartbeat(a.id.as_str(), T);
    }
    let fail_time = T + HEARTBEAT_TIMEOUT + 1;
    for a in agents.iter().skip(1) {
        tracker.record_heartbeat(a.id.as_str(), fail_time - 1);
    }
    println!("  {} went silent (simulated failure)\n", failed_id);

    let (dummy, _) =
        order_fsm::create_order("reauction-1", failed_id.clone(), PICKUP, DROPOFF, T);
    let dummy = order_fsm::mark_bidding(dummy, T)?;
    let (dummy, _) = order_fsm::assign_order(dummy, failed_id.clone(), T)?;

    // -- Step 9: REAUCTION CANDIDATES ----------------------------------------
    section("REAUCTION CANDIDATES");
    let candidates =
        tracker.orders_to_reauction(std::iter::once(&dummy), fail_time, HEARTBEAT_TIMEOUT);
    if candidates.is_empty() {
        println!("  none\n");
    } else {
        for c in &candidates {
            println!("  order {} needs a new auction", c);
        }
        println!();
    }

    // -- Step 10: SAFETY CHECK -----------------------------------------------
    section("SAFETY CHECK");
    let mut safety = SafetyMonitor::new();
    safety.add_alert(SafetyZone {
        zone_id: "zone-alpha".into(),
        center: PICKUP,
        radius_m: 0.01,
        active: true,
        declared_at: T,
    });
    let paused = safety.is_paused_by_safety(PICKUP.0, PICKUP.1);
    println!("  zone     : zone-alpha  (active, centred on pickup point)");
    println!(
        "  at pickup: {}\n",
        if paused {
            "PAUSED — agent inside safety zone"
        } else {
            "CLEAR"
        }
    );

    banner("SIMULATION COMPLETE");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn banner(title: &str) {
    let line = "=".repeat(52);
    println!("\n{line}");
    println!("  {title}");
    println!("{line}\n");
}

fn section(title: &str) {
    let pad = 44usize.saturating_sub(title.len());
    println!("-- {title} {}", "-".repeat(pad));
}

// ---------------------------------------------------------------------------
// Minimal local agent
// ---------------------------------------------------------------------------

struct LocalAgent {
    id: NodeId,
    x: f64,
    y: f64,
    battery: u8,
}

impl LocalAgent {
    fn from_config(c: &AppConfig) -> Self {
        Self {
            id: c.node_id.clone(),
            x: c.x,
            y: c.y,
            battery: c.battery,
        }
    }

    fn eta_to(&self, target: (f64, f64)) -> u32 {
        let dx = self.x - target.0;
        let dy = self.y - target.1;
        ((dx * dx + dy * dy).sqrt() * 100.0)
            .round()
            .clamp(1.0, 599.0) as u32
    }

    fn bid(&self, order_id: &str) -> AuctionBid {
        AuctionBid {
            order_id: order_id.to_owned(),
            bidder: self.id.clone(),
            eta_s: self.eta_to(PICKUP),
            battery_pct: self.battery,
            submitted_at: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Config builder (no file I/O)
// ---------------------------------------------------------------------------

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
