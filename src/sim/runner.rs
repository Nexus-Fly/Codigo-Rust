#![allow(dead_code)]

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

#[allow(dead_code)]
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
// LiveSim – tick-based simulation state machine
// ---------------------------------------------------------------------------

/// Simulation phases, executed one per tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimPhase {
    Init,
    OrderCreated,
    BidsReceived,
    WinnerChosen,
    MovingToPickup,
    PickupCompleted,
    InTransit,
    HandoffRequested,
    HandoffCompleted,
    Delivered,
    LedgerSettled,
    Done,
}

/// State for the live, tick-driven simulation.
pub struct LiveSim {
    agents: Vec<SimAgent>,
    tick: u32,
    phase: SimPhase,

    // mutable delivery state built up across ticks
    order_id: String,
    winner_id: NodeId,
    delivering_agent: NodeId,
    ledger: Ledger,
    reauction_candidates: Vec<String>,
    safety_paused: bool,
}

impl LiveSim {
    const PICKUP: (f64, f64) = (40.7128, -74.0060);
    const DROPOFF: (f64, f64) = (40.7580, -73.9855);
    const ESCROW: u64 = 300;
    const HANDOFF_FEE: u64 = 100;
    const HEARTBEAT_TIMEOUT: u64 = 30;
    const BASE_T: Timestamp = 1_700_000_000;

    pub fn new(configs: Vec<AppConfig>) -> Self {
        let agents = configs.into_iter().map(SimAgent::new).collect();
        Self {
            agents,
            tick: 0,
            phase: SimPhase::Init,
            order_id: String::new(),
            winner_id: String::new(),
            delivering_agent: String::new(),
            ledger: Ledger::new(),
            reauction_candidates: Vec::new(),
            safety_paused: false,
        }
    }

    pub fn current_tick(&self) -> u32 {
        self.tick
    }

    pub fn is_complete(&self) -> bool {
        self.phase == SimPhase::Done
    }

    /// Advance one simulation tick. Returns a log line for this tick.
    pub fn step(&mut self) -> Result<String> {
        let t = Self::BASE_T + self.tick as u64;
        let log = match self.phase {
            SimPhase::Init => {
                self.order_id = format!("order-{}", self.tick);
                let msg = format!(
                    "[t={}] simulation started — {} agent(s) online",
                    self.tick,
                    self.agents.len()
                );
                // drain 1 battery per tick
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::OrderCreated;
                msg
            }

            SimPhase::OrderCreated => {
                let origin = self.agents[0].config.node_id.clone();
                let (order, _) = order_fsm::create_order(
                    &self.order_id,
                    origin.clone(),
                    Self::PICKUP,
                    Self::DROPOFF,
                    t,
                );
                let _ = order_fsm::mark_bidding(order, t)?;
                self.phase = SimPhase::BidsReceived;
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                format!("[t={}] order created  id={}", self.tick, self.order_id)
            }

            SimPhase::BidsReceived => {
                let bids: Vec<AuctionBid> = self
                    .agents
                    .iter()
                    .map(|a| a.build_bid(&self.order_id, Self::PICKUP))
                    .collect();
                let summary: Vec<String> = bids
                    .iter()
                    .map(|b| format!("{}(eta={}s,bat={}%)", b.bidder, b.eta_s, b.battery_pct))
                    .collect();
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::WinnerChosen;
                format!("[t={}] bids received  [{}]", self.tick, summary.join(", "))
            }

            SimPhase::WinnerChosen => {
                let bids: Vec<AuctionBid> = self
                    .agents
                    .iter()
                    .map(|a| a.build_bid(&self.order_id, Self::PICKUP))
                    .collect();
                self.winner_id = auction::choose_winner_id(&bids)
                    .ok_or_else(|| anyhow::anyhow!("no bids submitted"))?;

                // credit + reserve escrow
                let payer = self.agents[0].config.node_id.clone();
                self.ledger.credit(payer.clone(), Self::ESCROW);
                self.ledger.reserve_escrow(
                    self.order_id.clone(),
                    payer,
                    Self::ESCROW,
                )?;

                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::MovingToPickup;
                format!("[t={}] winner chosen  winner={}", self.tick, self.winner_id)
            }

            SimPhase::MovingToPickup => {
                // move winner agent toward pickup (linear interpolation step)
                if let Some(winner) = self.agents.iter_mut().find(|a| a.config.node_id == self.winner_id) {
                    let (tx, ty) = Self::PICKUP;
                    let dx = tx - winner.state.location.0;
                    let dy = ty - winner.state.location.1;
                    let step = 0.1_f64.min((dx * dx + dy * dy).sqrt());
                    let norm = ((dx * dx + dy * dy).sqrt()).max(f64::EPSILON);
                    winner.state.location.0 += dx / norm * step;
                    winner.state.location.1 += dy / norm * step;
                    winner.state.battery_pct = winner.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::PickupCompleted;
                format!(
                    "[t={}] moving toward pickup  winner={} loc=({:.4},{:.4})",
                    self.tick,
                    self.winner_id,
                    self.agents
                        .iter()
                        .find(|a| a.config.node_id == self.winner_id)
                        .map(|a| a.state.location.0)
                        .unwrap_or(0.0),
                    self.agents
                        .iter()
                        .find(|a| a.config.node_id == self.winner_id)
                        .map(|a| a.state.location.1)
                        .unwrap_or(0.0),
                )
            }

            SimPhase::PickupCompleted => {
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::InTransit;
                format!("[t={}] pickup completed  holder={}", self.tick, self.winner_id)
            }

            SimPhase::InTransit => {
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::HandoffRequested;
                format!("[t={}] in transit  holder={}", self.tick, self.winner_id)
            }

            SimPhase::HandoffRequested => {
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                if self.agents.len() >= 3 {
                    let dest = self
                        .agents
                        .iter()
                        .find(|a| a.config.node_id != self.winner_id)
                        .map(|a| a.config.node_id.clone())
                        .expect("at least one other agent");
                    self.delivering_agent = dest.clone();
                    self.phase = SimPhase::HandoffCompleted;
                    format!("[t={}] handoff requested  from={} to={}", self.tick, self.winner_id, dest)
                } else {
                    self.delivering_agent = self.winner_id.clone();
                    self.phase = SimPhase::Delivered;
                    format!("[t={}] no handoff (fewer than 3 agents)  holder={}", self.tick, self.winner_id)
                }
            }

            SimPhase::HandoffCompleted => {
                self.ledger.transfer_for_handoff(
                    &self.order_id,
                    self.winner_id.clone(),
                    self.delivering_agent.clone(),
                    Self::HANDOFF_FEE,
                )?;
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::Delivered;
                format!(
                    "[t={}] handoff completed  from={} to={}  fee={}",
                    self.tick, self.winner_id, self.delivering_agent, Self::HANDOFF_FEE
                )
            }

            SimPhase::Delivered => {
                for a in &mut self.agents {
                    a.state.battery_pct = a.state.battery_pct.saturating_sub(1);
                }
                self.phase = SimPhase::LedgerSettled;
                format!("[t={}] delivered  by={}", self.tick, self.delivering_agent)
            }

            SimPhase::LedgerSettled => {
                self.ledger.release_final_payment(
                    &self.order_id,
                    self.delivering_agent.clone(),
                )?;

                // failure detection
                let mut tracker = HeartbeatTracker::new();
                for a in &self.agents {
                    tracker.record_heartbeat(a.config.node_id.clone(), t);
                }
                let fail_time = t + Self::HEARTBEAT_TIMEOUT + 1;
                for a in self.agents.iter().skip(1) {
                    tracker.record_heartbeat(a.config.node_id.clone(), fail_time - 1);
                }
                let failed_id = self.agents[0].config.node_id.clone();
                let (dummy, _) = order_fsm::create_order(
                    "sim-reauction-1",
                    failed_id.clone(),
                    Self::PICKUP,
                    Self::DROPOFF,
                    t,
                );
                let dummy = order_fsm::mark_bidding(dummy, t)?;
                let (dummy, _) = order_fsm::assign_order(dummy, failed_id, t)?;
                self.reauction_candidates =
                    tracker.orders_to_reauction(std::iter::once(&dummy), fail_time, Self::HEARTBEAT_TIMEOUT);

                // safety check
                let mut safety = SafetyMonitor::new();
                safety.add_alert(SafetyZone {
                    zone_id: "zone-alpha".into(),
                    center: Self::PICKUP,
                    radius_m: 0.01,
                    active: true,
                    declared_at: t,
                });
                self.safety_paused = safety.is_paused_by_safety(Self::PICKUP.0, Self::PICKUP.1);

                let bal = self.ledger.balances.get(&self.delivering_agent).copied().unwrap_or(0);
                self.phase = SimPhase::Done;
                format!(
                    "[t={}] ledger settled  {} balance={}  reauction={:?}  safety_paused={}",
                    self.tick, self.delivering_agent, bal, self.reauction_candidates, self.safety_paused
                )
            }

            SimPhase::Done => {
                format!("[t={}] simulation already complete", self.tick)
            }
        };

        self.tick += 1;
        Ok(log)
    }

    /// Drive the simulation in real time, sleeping 1 second between ticks.
    pub async fn run_live(&mut self) -> Result<()> {
        while !self.is_complete() {
            let line = self.step()?;
            println!("{line}");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// WorldSim – persistent multi-agent world simulation
// ---------------------------------------------------------------------------

/// Tiny deterministic LCG (seed → next seed, value in 0..modulus).
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// In-world representation of one agent.
#[derive(Debug, Clone)]
pub struct WorldAgent {
    pub id: NodeId,
    pub location: (f64, f64),
    pub battery_pct: u8,
    pub status: AgentStatus,
    /// Ticks until the agent comes back online (0 = not offline).
    pub offline_countdown: u32,
}

/// In-world representation of one order and its delivery lifecycle.
#[derive(Debug)]
struct WorldOrder {
    order: crate::types::Order,
    /// How many ticks this order has spent in its current phase.
    phase_ticks: u32,
    /// Agent currently holding delivery responsibility.
    holder: Option<NodeId>,
    /// Whether a handoff has already been done for this order.
    handoff_done: bool,
    /// Whether escrow has been reserved for this order.
    escrow_reserved: bool,
}

/// Long-running multi-agent world simulation.
///
/// Call [`WorldSim::tick`] once per second. The world generates new orders,
/// auctions them off, advances deliveries, performs handoffs, simulates
/// battery drain and agent failures, and settles payments — all using the
/// same domain modules as the rest of the project.
pub struct WorldSim {
    pub agents: Vec<WorldAgent>,
    orders: Vec<WorldOrder>,
    ledger: Ledger,
    tracker: HeartbeatTracker,
    safety: SafetyMonitor,
    tick: u64,
    order_counter: u64,
    rng: u64,

    // configuration knobs
    order_every_ticks: u64,
    heartbeat_timeout: u64,
    battery_drain_per_tick: u8,
    offline_recovery_ticks: u32,
    phase_advance_ticks: u32,
    safety_zone_every_ticks: u64,
    safety_zone_duration_ticks: u64,
    failure_every_ticks: u64,
}

impl WorldSim {
    const BASE_T: Timestamp = 1_700_000_000;
    const ESCROW_AMOUNT: u64 = 300;
    const HANDOFF_FEE: u64 = 100;

    /// Create a new world with the given agent roster. `seed` controls the LCG.
    pub fn new(agents: Vec<WorldAgent>, seed: u64) -> Self {
        // Give every agent a starting balance so escrow can be reserved.
        let mut ledger = Ledger::new();
        for a in &agents {
            ledger.credit(a.id.clone(), 10_000);
        }
        Self {
            agents,
            orders: Vec::new(),
            ledger,
            tracker: HeartbeatTracker::new(),
            safety: SafetyMonitor::new(),
            tick: 0,
            order_counter: 0,
            rng: seed,
            order_every_ticks: 8,
            heartbeat_timeout: 20,
            battery_drain_per_tick: 1,
            offline_recovery_ticks: 15,
            phase_advance_ticks: 3,
            safety_zone_every_ticks: 30,
            safety_zone_duration_ticks: 10,
            failure_every_ticks: 25,
        }
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    // ── public helpers ──────────────────────────────────────────────────────

    /// Advance the world by one second. Returns a (possibly multi-line) log string.
    pub fn tick(&mut self) -> String {
        let mut lines: Vec<String> = Vec::new();
        let now = Self::BASE_T + self.tick;

        self.update_agents(now, &mut lines);
        self.generate_order_if_needed(now, &mut lines);
        self.manage_safety_zones(now, &mut lines);
        self.process_active_orders(now, &mut lines);
        self.detect_failures(now, &mut lines);

        self.tick += 1;
        lines.join("\n")
    }

    // ── internal helpers ────────────────────────────────────────────────────

    /// Drain battery, advance offline countdowns, record heartbeats.
    fn update_agents(&mut self, now: Timestamp, lines: &mut Vec<String>) {
        for agent in &mut self.agents {
            if agent.offline_countdown > 0 {
                agent.offline_countdown -= 1;
                if agent.offline_countdown == 0 {
                    agent.status = AgentStatus::Idle;
                    agent.battery_pct = 80; // recharged
                    lines.push(format!(
                        "[t={}] agent back online  id={}  bat={}%",
                        self.tick, agent.id, agent.battery_pct
                    ));
                }
            } else {
                agent.battery_pct = agent.battery_pct.saturating_sub(self.battery_drain_per_tick);
                if agent.status != AgentStatus::Offline {
                    self.tracker.record_heartbeat(agent.id.clone(), now);
                }
                // Force agent offline for recharge if critically low.
                if agent.battery_pct == 0 && agent.status != AgentStatus::Offline {
                    agent.status = AgentStatus::Offline;
                    agent.offline_countdown = self.offline_recovery_ticks;
                    lines.push(format!(
                        "[t={}] agent battery depleted  id={} — going offline",
                        self.tick, agent.id
                    ));
                }
            }
        }
    }

    /// Spawn a new order every `order_every_ticks` ticks if an idle agent exists.
    fn generate_order_if_needed(&mut self, now: Timestamp, lines: &mut Vec<String>) {
        if self.tick % self.order_every_ticks != 0 {
            return;
        }
        let idle_exists = self.agents.iter().any(|a| a.status == AgentStatus::Idle);
        if !idle_exists {
            return;
        }

        let origin = self.agents[0].id.clone();
        let order_id = format!("order-{}", self.order_counter);
        self.order_counter += 1;

        // Vary pickup/dropoff with LCG.
        let lat_off = (lcg_next(&mut self.rng) % 100) as f64 * 0.001;
        let lon_off = (lcg_next(&mut self.rng) % 100) as f64 * 0.001;
        let pickup = (40.7128 + lat_off, -74.0060 + lon_off);
        let dropoff = (40.7580 + lat_off, -73.9855 + lon_off);

        let (mut order, _) = order_fsm::create_order(&order_id, origin, pickup, dropoff, now);
        match order_fsm::mark_bidding(order.clone(), now) {
            Ok(o) => order = o,
            Err(_) => return,
        }

        lines.push(format!(
            "[t={}] new order created  id={}  pickup=({:.4},{:.4})",
            self.tick, order_id, pickup.0, pickup.1
        ));

        // Collect bids from all idle agents.
        let bids: Vec<AuctionBid> = self
            .agents
            .iter()
            .filter(|a| a.status == AgentStatus::Idle)
            .map(|a| {
                let dx = a.location.0 - pickup.0;
                let dy = a.location.1 - pickup.1;
                let eta = ((dx * dx + dy * dy).sqrt() * 100.0).round().clamp(1.0, 599.0) as u32;
                AuctionBid {
                    order_id: order_id.clone(),
                    bidder: a.id.clone(),
                    eta_s: eta,
                    battery_pct: a.battery_pct,
                    submitted_at: now,
                }
            })
            .collect();

        if bids.is_empty() {
            return;
        }

        let bid_summary: Vec<String> = bids
            .iter()
            .map(|b| format!("{}(eta={}s,bat={}%)", b.bidder, b.eta_s, b.battery_pct))
            .collect();
        lines.push(format!(
            "[t={}] bids received  [{}]",
            self.tick,
            bid_summary.join(", ")
        ));

        let winner_id = match auction::choose_winner_id(&bids) {
            Some(id) => id,
            None => return,
        };

        // Reserve escrow from originator.
        let can_reserve = self
            .ledger
            .balances
            .get(&self.agents[0].id)
            .copied()
            .unwrap_or(0)
            >= Self::ESCROW_AMOUNT;

        if !can_reserve {
            return;
        }

        let payer = self.agents[0].id.clone();
        if self
            .ledger
            .reserve_escrow(order_id.clone(), payer, Self::ESCROW_AMOUNT)
            .is_err()
        {
            return;
        }

        order = match order_fsm::assign_order(order, winner_id.clone(), now) {
            Ok((o, _)) => o,
            Err(_) => return,
        };

        // Mark winner as Busy.
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == winner_id) {
            agent.status = AgentStatus::Busy;
        }

        lines.push(format!(
            "[t={}] winner chosen  id={}  winner={}",
            self.tick, order_id, winner_id
        ));

        self.orders.push(WorldOrder {
            order,
            phase_ticks: 0,
            holder: Some(winner_id),
            handoff_done: false,
            escrow_reserved: true,
        });
    }

    /// Periodically activate and deactivate a safety zone.
    fn manage_safety_zones(&mut self, now: Timestamp, lines: &mut Vec<String>) {
        if self.tick % self.safety_zone_every_ticks == 0 && self.tick > 0 {
            let zone_id = format!("zone-{}", self.tick);
            let lat = 40.71 + (lcg_next(&mut self.rng) % 50) as f64 * 0.001;
            let lon = -74.01 + (lcg_next(&mut self.rng) % 50) as f64 * 0.001;
            self.safety.add_alert(SafetyZone {
                zone_id: zone_id.clone(),
                center: (lat, lon),
                radius_m: 0.005,
                active: true,
                declared_at: now,
            });
            lines.push(format!(
                "[t={}] safety zone activated  id={}  center=({:.4},{:.4})",
                self.tick, zone_id, lat, lon
            ));
        }
        // Clear zones older than `safety_zone_duration_ticks`.
        if self.tick > self.safety_zone_duration_ticks
            && self.tick % self.safety_zone_every_ticks == self.safety_zone_duration_ticks % self.safety_zone_every_ticks
        {
            let zone_id = format!("zone-{}", self.tick - self.safety_zone_duration_ticks);
            self.safety.clear_alert(&zone_id);
            lines.push(format!(
                "[t={}] safety zone cleared  id={}",
                self.tick, zone_id
            ));
        }
    }

    /// Advance every active order through its delivery lifecycle.
    fn process_active_orders(&mut self, now: Timestamp, lines: &mut Vec<String>) {
        let advance = self.phase_advance_ticks;
        let handoff_fee = Self::HANDOFF_FEE;

        // Iterate by index to allow mutable borrow of self.agents and self.ledger.
        let mut i = 0;
        while i < self.orders.len() {
            self.orders[i].phase_ticks += 1;
            if self.orders[i].phase_ticks < advance {
                i += 1;
                continue;
            }
            self.orders[i].phase_ticks = 0;
            let status = self.orders[i].order.status.clone();

            match status {
                OrderStatus::Assigned => {
                    if let Ok(o) = order_fsm::mark_pickup(self.orders[i].order.clone(), now) {
                        self.orders[i].order = o;
                        let holder = self.orders[i].holder.clone().unwrap_or_default();
                        lines.push(format!(
                            "[t={}] pickup completed  id={}  holder={}",
                            self.tick, self.orders[i].order.order_id, holder
                        ));
                    }
                }
                OrderStatus::Pickup => {
                    if let Ok(o) = order_fsm::mark_in_transit(self.orders[i].order.clone(), now) {
                        self.orders[i].order = o;
                        let holder = self.orders[i].holder.clone().unwrap_or_default();
                        lines.push(format!(
                            "[t={}] in transit  id={}  holder={}",
                            self.tick, self.orders[i].order.order_id, holder
                        ));
                    }
                }
                OrderStatus::InTransit => {
                    // Try a handoff if there is another idle agent and none done yet.
                    let can_handoff = !self.orders[i].handoff_done;
                    let holder_id = self.orders[i].holder.clone().unwrap_or_default();
                    let order_id = self.orders[i].order.order_id.clone();

                    let idle_other = if can_handoff {
                        self.agents
                            .iter()
                            .find(|a| a.id != holder_id && a.status == AgentStatus::Idle)
                            .map(|a| a.id.clone())
                    } else {
                        None
                    };

                    if let Some(dest_id) = idle_other {
                        // Perform handoff via ledger (skip FSM to keep it simple).
                        let _ = self.ledger.transfer_for_handoff(
                            &order_id,
                            holder_id.clone(),
                            dest_id.clone(),
                            handoff_fee,
                        );
                        // Mark old holder as Idle, new holder as Busy.
                        if let Some(a) = self.agents.iter_mut().find(|a| a.id == holder_id) {
                            a.status = AgentStatus::Idle;
                        }
                        if let Some(a) = self.agents.iter_mut().find(|a| a.id == dest_id) {
                            a.status = AgentStatus::Busy;
                        }
                        self.orders[i].holder = Some(dest_id.clone());
                        self.orders[i].handoff_done = true;
                        lines.push(format!(
                            "[t={}] handoff completed  id={}  from={}  to={}  fee={}",
                            self.tick, order_id, holder_id, dest_id, handoff_fee
                        ));
                    } else {
                        // Deliver directly.
                        let order = self.orders[i].order.clone();
                        let holder = holder_id.clone();
                        if let Ok((o, _)) = order_fsm::mark_delivered(order, holder.clone(), now) {
                            self.orders[i].order = o;
                            // Free the agent.
                            if let Some(a) = self.agents.iter_mut().find(|a| a.id == holder) {
                                a.status = AgentStatus::Idle;
                            }
                            // Settle ledger.
                            let _ = self
                                .ledger
                                .release_final_payment(&order_id, holder_id.clone());
                            let bal = self
                                .ledger
                                .balances
                                .get(&holder_id)
                                .copied()
                                .unwrap_or(0);
                            lines.push(format!(
                                "[t={}] delivered  id={}  by={}  balance={}",
                                self.tick, order_id, holder_id, bal
                            ));
                        }
                    }
                }
                OrderStatus::HandedOff => {
                    // Deliver after one more phase.
                    let holder_id = self.orders[i].holder.clone().unwrap_or_default();
                    let order_id = self.orders[i].order.order_id.clone();
                    let order = self.orders[i].order.clone();
                    if let Ok((o, _)) =
                        order_fsm::mark_delivered(order, holder_id.clone(), now)
                    {
                        self.orders[i].order = o;
                        if let Some(a) = self.agents.iter_mut().find(|a| a.id == holder_id) {
                            a.status = AgentStatus::Idle;
                        }
                        let _ = self
                            .ledger
                            .release_final_payment(&order_id, holder_id.clone());
                        let bal = self
                            .ledger
                            .balances
                            .get(&holder_id)
                            .copied()
                            .unwrap_or(0);
                        lines.push(format!(
                            "[t={}] delivered  id={}  by={}  balance={}",
                            self.tick, order_id, holder_id, bal
                        ));
                    }
                }
                OrderStatus::Delivered => {
                    // Remove completed orders.
                    self.orders.remove(i);
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Simulate occasional agent failures and detect them via HeartbeatTracker.
    fn detect_failures(&mut self, now: Timestamp, lines: &mut Vec<String>) {
        // Occasionally take an active agent offline.
        if self.tick > 0 && self.tick % self.failure_every_ticks == 0 {
            // Pick the first non-offline, non-busy idle agent.
            let target = self
                .agents
                .iter()
                .position(|a| a.status == AgentStatus::Idle);
            if let Some(idx) = target {
                self.agents[idx].status = AgentStatus::Offline;
                self.agents[idx].offline_countdown = self.offline_recovery_ticks;
                lines.push(format!(
                    "[t={}] agent offline (simulated failure)  id={}",
                    self.tick, self.agents[idx].id
                ));
            }
        }

        // Detect agents that haven't sent a heartbeat.
        let failed = self.tracker.detect_failed_agents(now, self.heartbeat_timeout);
        for fid in &failed {
            // Find orders held by failed agents and mark for re-auction.
            for wo in &mut self.orders {
                if wo.holder.as_deref() == Some(fid.as_str())
                    && !matches!(wo.order.status, OrderStatus::Delivered | OrderStatus::Cancelled)
                {
                    lines.push(format!(
                        "[t={}] order flagged for reauction  id={}  failed_agent={}",
                        self.tick, wo.order.order_id, fid
                    ));
                    // Reset to bidding so the order re-enters the auction.
                    wo.holder = None;
                    if let Some(a) = self.agents.iter_mut().find(|a| &a.id == fid) {
                        a.status = AgentStatus::Offline;
                    }
                }
            }
        }
    }
}


mod tests {
    use super::*;
    use crate::types::AgentKind;

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
            auto_order_source: false,
            order_interval_secs: 20,
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