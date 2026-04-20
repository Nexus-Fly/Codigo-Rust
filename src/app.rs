#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use tracing::{info, warn};

use crate::{
    config::AppConfig,
    domain::auction,
    domain::order as order_fsm,
    types::{AgentState, AgentStatus, AuctionBid, NexusMessage, NodeId, Order, Timestamp},
};

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Central application state for one MVP delivery node.
///
/// Holds all in-memory state: local agent info, known orders, peer states,
/// and pending auction bids. Designed to receive `NexusMessage` values
/// decoded from Tashi Vertex transactions.
///
/// `src/main.rs` is not yet migrated to use `App`; that is a later step.
pub struct App {
    pub config: AppConfig,

    // Local agent state broadcast to peers.
    pub local_agent: AgentState,

    // All orders seen on the network, keyed by order_id.
    pub orders: HashMap<String, Order>,

    // Latest known state for each peer node.
    pub peer_states: HashMap<NodeId, AgentState>,

    // Bids collected per order, keyed by order_id.
    pub bids: HashMap<String, Vec<AuctionBid>>,
}

impl App {
    /// Initialise from a loaded `AppConfig`.
    pub fn new(config: AppConfig) -> Result<Self> {
        let local_agent = AgentState {
            node_id: config.node_id.clone(),
            kind: config.agent_kind.clone(),
            status: AgentStatus::Idle,
            location: (config.x, config.y),
            battery_pct: config.battery,
            updated_at: now(),
        };

        info!(node_id = %config.node_id, bind = %config.bind, "App initialised");

        Ok(Self {
            config,
            local_agent,
            orders: HashMap::new(),
            peer_states: HashMap::new(),
            bids: HashMap::new(),
        })
    }

    /// Alias for [`App::new`] — initialise from a loaded [`AppConfig`].
    pub fn from_config(config: AppConfig) -> Result<Self> {
        Self::new(config)
    }

    /// Return an `AgentState` heartbeat message for this node without mutating state.
    pub fn heartbeat(&self) -> NexusMessage {
        NexusMessage::AgentState(self.local_agent.clone())
    }

    // -----------------------------------------------------------------------
    // Message handler (called per decoded NexusMessage from the swarm)
    // -----------------------------------------------------------------------

    /// Process one decoded `NexusMessage` received from a consensus event.
    pub fn handle_message(&mut self, msg: NexusMessage) -> Result<()> {
        match msg {
            NexusMessage::AgentState(state) => self.on_agent_state(state),
            NexusMessage::OrderCreated(order) => self.on_order_created(order),
            NexusMessage::AuctionBid(bid) => self.on_auction_bid(bid),
            NexusMessage::AuctionWinner { order_id, winner } => {
                self.on_auction_winner(order_id, winner)
            }
            NexusMessage::HandoffRequest { order_id, from, to } => {
                self.on_handoff_request(order_id, from, to)
            }
            NexusMessage::HandoffComplete { order_id, new_holder } => {
                self.on_handoff_complete(order_id, new_holder)
            }
            NexusMessage::AgentFailure { node_id, reason } => {
                self.on_agent_failure(node_id, reason)
            }
            NexusMessage::SafetyAlert(zone) => {
                info!(zone_id = %zone.zone_id, "Safety alert received");
                Ok(())
            }
            NexusMessage::SafetyClear { zone_id } => {
                info!(%zone_id, "Safety zone cleared");
                Ok(())
            }
            NexusMessage::OrderDelivered {
                order_id,
                delivered_by,
                at,
            } => self.on_order_delivered(order_id, delivered_by, at),
        }
    }

    // -----------------------------------------------------------------------
    // Outbound helpers
    // -----------------------------------------------------------------------

    /// Create a new order and return the message to broadcast.
    pub fn submit_order(
        &mut self,
        order_id: impl Into<String>,
        pickup: (f64, f64),
        dropoff: (f64, f64),
    ) -> NexusMessage {
        let origin = self.config.node_id.clone();
        let (order, msg) = order_fsm::create_order(order_id, origin, pickup, dropoff, now());
        info!(order_id = %order.order_id, "Order submitted");
        self.orders.insert(order.order_id.clone(), order);
        msg
    }

    /// Record a local bid and return the message to broadcast.
    pub fn submit_bid(&mut self, order_id: impl Into<String>, eta_s: u32) -> NexusMessage {
        let order_id = order_id.into();
        let bid = AuctionBid {
            order_id: order_id.clone(),
            bidder: self.config.node_id.clone(),
            eta_s,
            battery_pct: self.local_agent.battery_pct,
            submitted_at: now(),
        };
        info!(order_id = %bid.order_id, bidder = %bid.bidder, "Bid submitted");
        self.bids
            .entry(order_id)
            .or_default()
            .push(bid.clone());
        NexusMessage::AuctionBid(bid)
    }

    // -----------------------------------------------------------------------
    // Runtime loop (placeholder — wired to VertexNode in a later phase)
    // -----------------------------------------------------------------------

    /// Main event loop. Currently a no-op stub; will be wired to
    /// `VertexNode::recv_messages` once `main.rs` is migrated.
    pub async fn run(&mut self) -> Result<()> {
        info!("App::run called (stub — not yet wired to Vertex engine)");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal handlers
    // -----------------------------------------------------------------------

    fn on_agent_state(&mut self, state: AgentState) -> Result<()> {
        info!(node_id = %state.node_id, status = ?state.status, "Peer state updated");
        self.peer_states.insert(state.node_id.clone(), state);
        Ok(())
    }

    fn on_order_created(&mut self, order: Order) -> Result<()> {
        info!(order_id = %order.order_id, origin = %order.origin, "Order received");
        self.orders
            .entry(order.order_id.clone())
            .or_insert(order);
        Ok(())
    }

    fn on_auction_bid(&mut self, bid: AuctionBid) -> Result<()> {
        info!(order_id = %bid.order_id, bidder = %bid.bidder, "Bid received");
        self.bids
            .entry(bid.order_id.clone())
            .or_default()
            .push(bid);
        Ok(())
    }

    fn on_auction_winner(&mut self, order_id: String, winner: NodeId) -> Result<()> {
        info!(%order_id, %winner, "Auction winner received");
        let order = match self.orders.get_mut(&order_id) {
            Some(o) => o,
            None => {
                warn!(%order_id, "AuctionWinner for unknown order");
                return Ok(());
            }
        };
        // Advance state: Bidding -> Assigned.
        // In a distributed system messages may arrive out of order; warn and
        // continue rather than failing the whole handler.
        match order_fsm::assign_order(order.clone(), winner, now()) {
            Ok((updated, _)) => *order = updated,
            Err(e) => warn!(%order_id, error = %e, "Cannot apply AuctionWinner transition"),
        }
        Ok(())
    }

    fn on_handoff_request(&mut self, order_id: String, from: NodeId, to: NodeId) -> Result<()> {
        info!(%order_id, %from, %to, "Handoff request received");
        // State update is driven by the eventual HandoffComplete.
        Ok(())
    }

    fn on_handoff_complete(&mut self, order_id: String, new_holder: NodeId) -> Result<()> {
        info!(%order_id, %new_holder, "Handoff complete");
        let order = match self.orders.get_mut(&order_id) {
            Some(o) => o,
            None => {
                warn!(%order_id, "HandoffComplete for unknown order");
                return Ok(());
            }
        };
        match order_fsm::complete_handoff(order.clone(), new_holder, now()) {
            Ok((updated, _)) => *order = updated,
            Err(e) => warn!(%order_id, error = %e, "Cannot apply HandoffComplete transition"),
        }
        Ok(())
    }

    fn on_agent_failure(&mut self, node_id: NodeId, reason: String) -> Result<()> {
        warn!(%node_id, %reason, "Agent failure reported");
        if let Some(state) = self.peer_states.get_mut(&node_id) {
            state.status = AgentStatus::Offline;
        }
        Ok(())
    }

    fn on_order_delivered(&mut self, order_id: String, delivered_by: NodeId, _at: Timestamp) -> Result<()> {
        info!(%order_id, %delivered_by, "Order delivered");
        let order = match self.orders.get_mut(&order_id) {
            Some(o) => o,
            None => {
                warn!(%order_id, "OrderDelivered for unknown order");
                return Ok(());
            }
        };
        match order_fsm::mark_delivered(order.clone(), delivered_by, now()) {
            Ok((updated, _)) => *order = updated,
            Err(e) => warn!(%order_id, error = %e, "Cannot apply OrderDelivered transition"),
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Auction convenience
    // -----------------------------------------------------------------------

    /// Evaluate current bids for an order and return the winner's NodeId, if any.
    pub fn evaluate_auction(&self, order_id: &str) -> Option<NodeId> {
        let bids = self.bids.get(order_id)?;
        auction::choose_winner_id(bids)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

