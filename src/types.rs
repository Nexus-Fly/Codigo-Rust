use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Primitive type aliases ────────────────────────────────────────────────
pub type OrderId  = u64;
pub type AgentId  = String;
pub type AlertId  = u64;
pub type EscrowId = u64;
pub type HandoffId = u64;
pub type RequestId = u64;

// ─── Agent classification ──────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentType {
    Drone,
    GroundRobot,
    Ebike,
    Unknown,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::Drone       => write!(f, "drone"),
            AgentType::GroundRobot => write!(f, "robot"),
            AgentType::Ebike       => write!(f, "ebike"),
            AgentType::Unknown     => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Busy,
    Failed,
    Recovering,
}

// ─── Spatial point ─────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self { Self { x, y } }
    pub fn distance_to(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

// ─── Escrow information ────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowInfo {
    pub escrow_id:        EscrowId,
    pub order_id:         OrderId,
    pub amount:           u64,
    pub client_signature: Vec<u8>,
    pub holder_agent:     AgentId,
    pub locked_at:        u64, // Unix timestamp millis
    pub released:         bool,
}

// ─── Handoff payment record ────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffPaymentRecord {
    pub handoff_id:  HandoffId,
    pub from_agent:  AgentId,
    pub to_agent:    AgentId,
    pub amount:      u64,
    pub signature:   Vec<u8>,
}

// ─── SwarmMessage – all P2P messages ──────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwarmMessage {
    // ── SwarmLogix original ──────────────────────────────────────
    AgentState {
        agent_id:   AgentId,
        agent_type: AgentType,
        vendor:     String,
        x:          f64,
        y:          f64,
        battery:    f64,
        capacity:   f64,
        status:     AgentStatus,
    },
    OrderCreated {
        order_id:      OrderId,
        pickup:        Point,
        delivery:      Point,
        weight:        f64,
        escrow_amount: u64,
    },
    AuctionBid {
        order_id: OrderId,
        agent_id: AgentId,
        score:    f64,
    },
    AuctionWinner {
        order_id:  OrderId,
        winner_id: AgentId,
    },
    HandoffRequest {
        order_id:   OrderId,
        from_agent: AgentId,
        to_agent:   AgentId,
        point:      Point,
    },
    HandoffComplete {
        order_id:   OrderId,
        from_agent: AgentId,
        to_agent:   AgentId,
    },
    OrderDelivered {
        order_id: OrderId,
        agent_id: AgentId,
    },
    AgentFailure {
        agent_id: AgentId,
        reason:   String,
    },
    AgentRecovery {
        agent_id: AgentId,
        battery:  f64,
    },
    SafetyAlert {
        alert_id: AlertId,
        x:        f64,
        y:        f64,
        radius:   f64,
    },
    SafetyClear {
        alert_id: AlertId,
    },

    // ── Micropayments ────────────────────────────────────────────
    PaymentEscrow {
        order_id:         OrderId,
        from_agent:       AgentId,
        amount:           u64,
        client_signature: Vec<u8>,
        escrow_id:        EscrowId,
    },
    PaymentClaim {
        order_id:   OrderId,
        agent_id:   AgentId,
        proof_hash: Vec<u8>,
        timestamp:  u64,
    },
    PaymentRelease {
        escrow_id:            EscrowId,
        to_agent:             AgentId,
        amount:               u64,
        validator_signatures: Vec<Vec<u8>>,
    },
    HandoffPayment {
        handoff_id: HandoffId,
        from_agent: AgentId,
        to_agent:   AgentId,
        amount:     u64,
        signature:  Vec<u8>,
    },
    BalanceQuery {
        agent_id:   AgentId,
        request_id: RequestId,
    },
    BalanceResponse {
        agent_id:   AgentId,
        balance:    u64,
        request_id: RequestId,
    },
}

impl SwarmMessage {
    /// Serialize to bytes for Vertex transactions.
    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Deserialize from Vertex transaction bytes.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

// ─── Agent-level state ─────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusAgent {
    pub id:               AgentId,
    pub agent_type:       AgentType,
    pub vendor:           String,
    pub position:         Point,
    pub battery:          f64,
    pub capacity:         f64,
    pub status:           AgentStatus,
    // Payment state
    pub balance:          u64,
    pub pending_escrows:  HashMap<EscrowId, EscrowInfo>,
    pub handoff_payments: Vec<HandoffPaymentRecord>,
    // Reputation
    pub successful_deliveries: u32,
    pub total_assignments:     u32,
}

impl NexusAgent {
    pub fn new(
        id: impl Into<String>,
        agent_type: AgentType,
        vendor: impl Into<String>,
        x: f64,
        y: f64,
        battery: f64,
        capacity: f64,
        initial_balance: u64,
    ) -> Self {
        Self {
            id: id.into(),
            agent_type,
            vendor: vendor.into(),
            position: Point::new(x, y),
            battery,
            capacity,
            status: AgentStatus::Idle,
            balance: initial_balance,
            pending_escrows: HashMap::new(),
            handoff_payments: Vec::new(),
            successful_deliveries: 0,
            total_assignments: 0,
        }
    }

    pub fn reputation(&self) -> f64 {
        if self.total_assignments == 0 { 1.0 }
        else { self.successful_deliveries as f64 / self.total_assignments as f64 }
    }
}
