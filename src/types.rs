use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Primitive aliases
// ─────────────────────────────────────────────────────────────────────────────

/// Node identifier – Base58 public key string.
pub type NodeId = String;

/// Raw transaction bytes produced / consumed by Tashi Vertex.
pub type RawTransaction = Vec<u8>;

/// Logical consensus round counter.
pub type Round = u64;

/// Unix timestamp in seconds.
pub type Timestamp = u64;

// ─────────────────────────────────────────────────────────────────────────────
// Enums
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentKind {
    Drone,
    Robot,
    Ebike,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Busy,
    Paused,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Created,
    Bidding,
    Assigned,
    Pickup,
    InTransit,
    HandoffPending,
    HandedOff,
    Delivered,
    Cancelled,
}

// ─────────────────────────────────────────────────────────────────────────────
// Structs
// ─────────────────────────────────────────────────────────────────────────────

/// Live state of a delivery agent broadcast over the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Public-key identifier of this agent's node.
    pub node_id: NodeId,
    pub kind: AgentKind,
    pub status: AgentStatus,
    /// Current GPS location as (latitude, longitude).
    pub location: (f64, f64),
    /// Battery level 0–100.
    pub battery_pct: u8,
    pub updated_at: Timestamp,
}

/// A delivery order propagated through the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub status: OrderStatus,
    /// Node that created the order.
    pub origin: NodeId,
    /// Node currently responsible for delivery (None until assigned).
    pub assigned_to: Option<NodeId>,
    /// Pickup point as (latitude, longitude).
    pub pickup: (f64, f64),
    /// Drop-off point as (latitude, longitude).
    pub dropoff: (f64, f64),
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A bid submitted by an agent during the auction phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuctionBid {
    pub order_id: String,
    pub bidder: NodeId,
    /// Estimated travel time to pickup in seconds.
    pub eta_s: u32,
    /// Battery remaining at the time of bidding.
    pub battery_pct: u8,
    pub submitted_at: Timestamp,
}

/// A geographic exclusion or restricted zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyZone {
    pub zone_id: String,
    /// Centre of the zone (latitude, longitude).
    pub center: (f64, f64),
    /// Radius in metres.
    pub radius_m: f32,
    pub active: bool,
    pub declared_at: Timestamp,
}

/// An immutable record appended to the local ledger after each confirmed round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub round: Round,
    pub order_id: String,
    pub actor: NodeId,
    pub event: String,
    pub recorded_at: Timestamp,
}

impl LedgerEntry {
    pub fn new(round: Round, order_id: &str, actor: &str, event: &str, ts: Timestamp) -> Self {
        Self {
            round,
            order_id: order_id.to_owned(),
            actor: actor.to_owned(),
            event: event.to_owned(),
            recorded_at: ts,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Network message envelope
// ─────────────────────────────────────────────────────────────────────────────

/// Every Tashi Vertex transaction carries exactly one `NexusMessage`.
/// Serialised to JSON bytes before being passed to `Transaction::allocate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NexusMessage {
    AgentState(AgentState),
    OrderCreated(Order),
    AuctionBid(AuctionBid),
    /// Winner announcement: (order_id, winner_node_id)
    AuctionWinner { order_id: String, winner: NodeId },
    /// Agent requesting a hand-off of an order to another agent.
    HandoffRequest { order_id: String, from: NodeId, to: NodeId },
    /// Hand-off acknowledged and complete.
    HandoffComplete { order_id: String, new_holder: NodeId },
    /// Agent reporting its own failure or unexpected shutdown.
    AgentFailure { node_id: NodeId, reason: String },
    SafetyAlert(SafetyZone),
    /// Zone is no longer active.
    SafetyClear { zone_id: String },
    OrderDelivered { order_id: String, delivered_by: NodeId, at: Timestamp },
}

