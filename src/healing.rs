use std::collections::{HashMap, HashSet};
use crate::types::{AgentId, OrderId, EscrowId};

/// Tracks known-failed agents and orders that need re-auctioning.
#[derive(Debug, Default)]
pub struct FailureDetector {
    failed_agents:         HashSet<AgentId>,
    pending_re_auction:    Vec<OrderId>,
    /// Escrows held by each agent (for recovery on failure)
    agent_escrows:         HashMap<AgentId, Vec<EscrowId>>,
}

impl FailureDetector {
    pub fn new() -> Self { Self::default() }

    /// Register an escrow held by an agent.
    pub fn track_escrow(&mut self, agent_id: &str, escrow_id: EscrowId) {
        self.agent_escrows
            .entry(agent_id.to_string())
            .or_default()
            .push(escrow_id);
    }

    /// Process an agent failure.  Returns escrow IDs that must be returned.
    pub fn handle_failure(
        &mut self,
        agent_id: &str,
        orders_by_agent: &HashMap<OrderId, AgentId>,
    ) -> (Vec<OrderId>, Vec<EscrowId>) {
        tracing::warn!("[healing] Agent {agent_id} reported as failed – triggering re-auction");
        self.failed_agents.insert(agent_id.to_string());

        // Collect orders that were assigned to this agent
        let affected_orders: Vec<OrderId> = orders_by_agent
            .iter()
            .filter(|(_, a)| a.as_str() == agent_id)
            .map(|(o, _)| *o)
            .collect();
        self.pending_re_auction.extend(affected_orders.iter().copied());

        // Collect escrows to return
        let escrows = self
            .agent_escrows
            .remove(agent_id)
            .unwrap_or_default();

        (affected_orders, escrows)
    }

    pub fn is_failed(&self, agent_id: &str) -> bool {
        self.failed_agents.contains(agent_id)
    }

    pub fn drain_pending_re_auction(&mut self) -> Vec<OrderId> {
        std::mem::take(&mut self.pending_re_auction)
    }

    /// Remove agent from failed set on recovery.
    pub fn handle_recovery(&mut self, agent_id: &str) {
        self.failed_agents.remove(agent_id);
        tracing::info!("[healing] Agent {agent_id} recovered");
    }
}
