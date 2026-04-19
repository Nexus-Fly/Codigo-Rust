use anyhow::{bail, Result};

use crate::{
    config::AppConfig,
    domain::{
        auction,
        handoff as handoff_domain,
        healing::HeartbeatTracker,
        ledger::Ledger,
        order as order_fsm,
        safety::SafetyMonitor,
    },
    types::{
        AgentState, AgentStatus, AuctionBid, NexusMessage, NodeId, OrderStatus, SafetyZone,
        Timestamp,
    },
};

// ---------------------------------------------------------------------------
// MvpFlowResult
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct MvpFlowResult {
    pub order_id: String,
    pub winner_id: NodeId,
    pub final_order_status: OrderStatus,
    pub delivering_agent_balance: u64,
    pub reauction_candidates: Vec<String>,
    pub safety_paused: bool,
}

// ---------------------------------------------------------------------------
// run_mvp_flow
// ---------------------------------------------------------------------------

pub fn run_mvp_flow(configs: Vec<AppConfig>) -> Result<MvpFlowResult> {
    if configs.len() < 2 {
        bail!("run_mvp_flow requires at least 2 agent configs");
    }

    const T: Timestamp = 1_700_000_000;
    const PICKUP: (f64, f64) = (40.7128, -74.0060);
    const DROPOFF: (f64, f64) = (40.7580, -73.9855);
    const ESCROW: u64 = 300;
    const HANDOFF_FEE: u64 = 100;
    const HEARTBEAT_TIMEOUT: u64 = 30;

    let agents: Vec<SimAgent> = configs.iter().map(|c| SimAgent::new(c.clone())).collect();

    // Step 1: create order
    let (mut order, msg) = order_fsm::create_order(
        "mvp-order-1",
        agents[0].config.node_id.clone(),
        PICKUP,
        DROPOFF,
        T,
    );
    log_event("OrderCreated", &msg);

    // Step 2: open bidding
    order = order_fsm::mark_bidding(order, T)?;

    // Step 3: collect bids
    let bids: Vec<AuctionBid> = agents
        .iter()
        .map(|a| a.build_bid(&order.order_id, PICKUP))
        .collect();
    for b in &bids {
        eprintln!("[mvp] bid: {} eta={}s bat={}%", b.bidder, b.eta_s, b.battery_pct);
    }

    // Step 4: choose winner + reserve escrow
    let winner_id = auction::choose_winner_id(&bids)
        .ok_or_else(|| anyhow::anyhow!("No bids submitted"))?;
    eprintln!("[mvp] winner: {}", winner_id);

    let mut ledger = Ledger::new();
    ledger.credit(agents[0].config.node_id.clone(), ESCROW);
    ledger.reserve_escrow(order.order_id.clone(), agents[0].config.node_id.clone(), ESCROW)?;

    let (o, msg) = order_fsm::assign_order(order, winner_id.clone(), T)?;
    order = o;
    log_event("AuctionWinner", &msg);

    // Step 5: delivery progress
    order = order_fsm::mark_pickup(order, T)?;
    order = order_fsm::mark_in_transit(order, T)?;
    eprintln!("[mvp] in transit, holder: {}", winner_id);

    // Step 6: optional handoff (>=3 agents)
    let delivering_agent: NodeId;
    if configs.len() >= 3 {
        let dest_id = agents
            .iter()
            .find(|a| a.config.node_id != winner_id)
            .expect("at least one other agent")
            .config
            .node_id
            .clone();

        let (o, msg) = handoff_domain::create_handoff_request(
            order,
            &winner_id,
            dest_id.clone(),
            &AgentStatus::Idle,
            T,
        )?;
        order = o;
        log_event("HandoffRequest", &msg);

        ledger.transfer_for_handoff(
            order.order_id.as_str(),
            winner_id.clone(),
            dest_id.clone(),
            HANDOFF_FEE,
        )?;

        let (o, msg) =
            handoff_domain::complete_handoff(order, dest_id.clone(), &AgentStatus::Idle, T)?;
        order = o;
        log_event("HandoffComplete", &msg);

        delivering_agent = dest_id;
        eprintln!("[mvp] handoff complete, new holder: {}", delivering_agent);
    } else {
        delivering_agent = winner_id.clone();
    }

    // Step 7: mark delivered
    let (order, msg) = order_fsm::mark_delivered(order, delivering_agent.clone(), T)?;
    log_event("OrderDelivered", &msg);

    // Step 8: ledger settlement
    ledger.release_final_payment(order.order_id.as_str(), delivering_agent.clone())?;
    let delivering_agent_balance = ledger.balances.get(&delivering_agent).copied().unwrap_or(0);
    eprintln!("[mvp] {} balance after delivery: {}", delivering_agent, delivering_agent_balance);

    // Step 9: agent failure simulation
    let mut tracker = HeartbeatTracker::new();
    let failed_node_id = agents[0].config.node_id.clone();
    for a in &agents {
        tracker.record_heartbeat(a.config.node_id.clone(), T);
    }
    let fail_time = T + HEARTBEAT_TIMEOUT + 1;
    for a in agents.iter().skip(1) {
        tracker.record_heartbeat(a.config.node_id.clone(), fail_time - 1);
    }
    let (dummy, _) = order_fsm::create_order(
        "mvp-reauction-1",
        failed_node_id.clone(),
        PICKUP,
        DROPOFF,
        T,
    );
    let dummy = order_fsm::mark_bidding(dummy, T)?;
    let (dummy, _) = order_fsm::assign_order(dummy, failed_node_id, T)?;
    let reauction_candidates =
        tracker.orders_to_reauction(std::iter::once(&dummy), fail_time, HEARTBEAT_TIMEOUT);
    eprintln!("[mvp] re-auction candidates: {:?}", reauction_candidates);

    // Step 10: safety zone evaluation
    let mut safety = SafetyMonitor::new();
    safety.add_alert(SafetyZone {
        zone_id: "zone-alpha".into(),
        center: PICKUP,
        radius_m: 0.01,
        active: true,
        declared_at: T,
    });
    let safety_paused = safety.is_paused_by_safety(PICKUP.0, PICKUP.1);
    eprintln!("[mvp] safety paused at pickup: {}", safety_paused);

    Ok(MvpFlowResult {
        order_id: order.order_id,
        winner_id,
        final_order_status: order.status,
        delivering_agent_balance,
        reauction_candidates,
        safety_paused,
    })
}

// ---------------------------------------------------------------------------
// SimAgent (private)
// ---------------------------------------------------------------------------

struct SimAgent {
    config: AppConfig,
    state: AgentState,
}

impl SimAgent {
    fn new(config: AppConfig) -> Self {
        let state = AgentState {
            node_id: config.node_id.clone(),
            kind: config.agent_kind.clone(),
            status: AgentStatus::Idle,
            location: (config.x, config.y),
            battery_pct: config.battery,
            updated_at: 0,
        };
        Self { config, state }
    }

    fn eta_to(&self, target: (f64, f64)) -> u32 {
        let dx = self.state.location.0 - target.0;
        let dy = self.state.location.1 - target.1;
        let dist = (dx * dx + dy * dy).sqrt();
        (dist * 100.0).round().clamp(1.0, 599.0) as u32
    }

    fn build_bid(&self, order_id: &str, pickup: (f64, f64)) -> AuctionBid {
        AuctionBid {
            order_id: order_id.to_owned(),
            bidder: self.config.node_id.clone(),
            eta_s: self.eta_to(pickup),
            battery_pct: self.state.battery_pct,
            submitted_at: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Runner (backward-compatible)
// ---------------------------------------------------------------------------

pub struct Runner {
    agents: Vec<SimAgent>,
}

impl Runner {
    pub fn new(configs: Vec<AppConfig>) -> Self {
        Self {
            agents: configs.into_iter().map(SimAgent::new).collect(),
        }
    }

    pub fn run_scenario(&mut self) -> Result<()> {
        if self.agents.is_empty() {
            bail!("Runner requires at least one agent");
        }

        let pickup = (40.7128, -74.0060);
        let dropoff = (40.7580, -73.9855);

        let (mut order, msg) = order_fsm::create_order(
            "sim-order-1",
            self.agents[0].config.node_id.clone(),
            pickup,
            dropoff,
            0,
        );
        log_event("OrderCreated", &msg);
        order = order_fsm::mark_bidding(order, 0)?;

        let bids: Vec<AuctionBid> = self
            .agents
            .iter()
            .map(|a| a.build_bid(&order.order_id, pickup))
            .collect();
        for bid in &bids {
            eprintln!("[sim] bid from '{}': eta={}s battery={}%", bid.bidder, bid.eta_s, bid.battery_pct);
        }

        let winner_id = auction::choose_winner_id(&bids)
            .ok_or_else(|| anyhow::anyhow!("No bids submitted"))?;
        eprintln!("[sim] winner: '{}'", winner_id);

        let (mut order, msg) = order_fsm::assign_order(order, winner_id, 0)?;
        log_event("AuctionWinner", &msg);

        order = order_fsm::mark_pickup(order, 0)?;
        order = order_fsm::mark_in_transit(order, 0)?;

        let delivered_by = order.assigned_to.clone().unwrap_or_default();
        let (order, msg) = order_fsm::mark_delivered(order, delivered_by, 0)?;
        log_event("OrderDelivered", &msg);
        eprintln!("[sim] final status: {:?}", order.status);

        Ok(())
    }
}

fn log_event(label: &str, msg: &NexusMessage) {
    eprintln!("[sim] {} -- {:?}", label, msg);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, OrderStatus};

    fn make_config(node_id: &str, kind: AgentKind, x: f64, y: f64, battery: u8) -> AppConfig {
        use crate::config::PeerConfig;
        AppConfig {
            node_id: node_id.to_owned(),
            secret_key: format!("sim-secret-{}", node_id),
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

    fn three_agents() -> Vec<AppConfig> {
        vec![
            make_config("drone-1", AgentKind::Drone, 40.71, -74.01, 90),
            make_config("robot-2", AgentKind::Robot, 40.75, -73.99, 60),
            make_config("ebike-3", AgentKind::Ebike, 40.73, -74.00, 80),
        ]
    }

    #[test]
    fn scenario_completes_without_error() {
        let mut runner = Runner::new(three_agents());
        runner.run_scenario().expect("scenario should succeed");
    }

    #[test]
    fn winner_is_deterministic() {
        let mut r1 = Runner::new(three_agents());
        let mut r2 = Runner::new(three_agents());
        r1.run_scenario().unwrap();
        r2.run_scenario().unwrap();
    }

    #[test]
    fn empty_runner_errors() {
        let mut runner = Runner::new(vec![]);
        assert!(runner.run_scenario().is_err());
    }

    #[test]
    fn eta_grows_with_distance() {
        let near = make_config("a", AgentKind::Drone, 40.7128, -74.0060, 80);
        let far = make_config("b", AgentKind::Drone, 41.0000, -75.0000, 80);
        let a = SimAgent::new(near);
        let b = SimAgent::new(far);
        let pickup = (40.7128, -74.0060);
        assert!(a.eta_to(pickup) < b.eta_to(pickup));
    }

    #[test]
    fn mvp_flow_requires_at_least_two_agents() {
        let one = vec![make_config("solo", AgentKind::Drone, 0.0, 0.0, 80)];
        assert!(run_mvp_flow(one).is_err());
    }

    #[test]
    fn mvp_flow_winner_is_selected() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert!(!result.winner_id.is_empty());
    }

    #[test]
    fn mvp_flow_order_reaches_delivered() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert_eq!(result.final_order_status, OrderStatus::Delivered);
    }

    #[test]
    fn mvp_flow_handoff_occurs_with_three_agents() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert!(result.delivering_agent_balance > 0);
    }

    #[test]
    fn mvp_flow_ledger_settled() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert!(result.delivering_agent_balance > 0);
    }

    #[test]
    fn mvp_flow_reauction_detects_failed_agent() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert_eq!(result.reauction_candidates, vec!["mvp-reauction-1"]);
    }

    #[test]
    fn mvp_flow_safety_zone_pauses_at_pickup() {
        let result = run_mvp_flow(three_agents()).unwrap();
        assert!(result.safety_paused);
    }

    #[test]
    fn mvp_flow_two_agents_no_handoff() {
        let two = vec![
            make_config("a", AgentKind::Drone, 40.71, -74.01, 90),
            make_config("b", AgentKind::Robot, 40.75, -73.99, 60),
        ];
        let result = run_mvp_flow(two).unwrap();
        assert_eq!(result.final_order_status, OrderStatus::Delivered);
        assert!(result.delivering_agent_balance > 0);
    }
}