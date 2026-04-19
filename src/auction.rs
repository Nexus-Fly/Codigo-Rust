use std::collections::HashMap;
use crate::types::{OrderId, AgentId, NexusAgent, Point, SwarmMessage};

/// All bids received for a single order.
#[derive(Debug, Default)]
pub struct AuctionState {
    pub order_id:       OrderId,
    pub pickup:         Point,
    pub delivery:       Point,
    pub weight:         f64,
    pub escrow_amount:  u64,
    pub bids:           HashMap<AgentId, f64>, // agent_id → score
    pub winner:         Option<AgentId>,
}

/// Calculate bid score for an agent given an order.
///
/// score = (1 / distance) × (battery / 100) × capacity_multiplier × (1 + 0.1 × reputation)
pub fn calculate_bid_score(agent: &NexusAgent, pickup: &Point, weight: f64) -> f64 {
    let distance = agent.position.distance_to(pickup).max(0.001); // avoid div/0
    let capacity_multiplier = if agent.capacity >= weight { 1.0 } else { 0.0 };
    let reputation = agent.reputation();

    (1.0 / distance) * (agent.battery / 100.0) * capacity_multiplier * (1.0 + 0.1 * reputation)
}

/// Auction book: tracks active auctions and determines winners via BFT order.
#[derive(Debug, Default)]
pub struct AuctionBook {
    pub auctions: HashMap<OrderId, AuctionState>,
}

impl AuctionBook {
    pub fn new() -> Self { Self::default() }

    /// Register a new order.
    pub fn open_auction(&mut self, msg: &SwarmMessage) {
        if let SwarmMessage::OrderCreated {
            order_id, pickup, delivery, weight, escrow_amount,
        } = msg {
            self.auctions.insert(*order_id, AuctionState {
                order_id:      *order_id,
                pickup:        *pickup,
                delivery:      *delivery,
                weight:        *weight,
                escrow_amount: *escrow_amount,
                bids:          HashMap::new(),
                winner:        None,
            });
        }
    }

    /// Record a bid.
    pub fn record_bid(&mut self, order_id: OrderId, agent_id: AgentId, score: f64) {
        if let Some(auction) = self.auctions.get_mut(&order_id) {
            auction.bids.insert(agent_id, score);
        }
    }

    /// Determine winner: first agent with highest score in BFT-ordered bids.
    /// Since Vertex provides total ordering, the first `AuctionWinner` message
    /// in consensus sequence is authoritative. Here we compute locally.
    pub fn determine_winner(&mut self, order_id: OrderId) -> Option<AgentId> {
        let auction = self.auctions.get_mut(&order_id)?;
        if auction.winner.is_some() {
            return auction.winner.clone();
        }
        let winner = auction
            .bids
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(id, _)| id.clone());
        auction.winner = winner.clone();
        winner
    }
}
