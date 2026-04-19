use std::collections::HashMap;
use crate::types::{OrderId, AgentId, HandoffId, Point};

#[derive(Debug, Clone, PartialEq)]
pub enum HandoffPhase {
    Requested,
    PaymentSent,
    Complete,
    Failed,
}

#[derive(Debug)]
pub struct HandoffState {
    pub handoff_id:  HandoffId,
    pub order_id:    OrderId,
    pub from_agent:  AgentId,
    pub to_agent:    AgentId,
    pub point:       Point,
    pub phase:       HandoffPhase,
    pub payment_pct: f64, // fraction of escrow paid on handoff (0.30)
}

/// Manages in-flight handoffs.
#[derive(Debug, Default)]
pub struct HandoffManager {
    pub handoffs: HashMap<HandoffId, HandoffState>,
    next_id:      HandoffId,
}

impl HandoffManager {
    pub fn new() -> Self { Self::default() }

    /// Record a new handoff request.
    pub fn initiate(
        &mut self,
        order_id:   OrderId,
        from_agent: AgentId,
        to_agent:   AgentId,
        point:      Point,
    ) -> HandoffId {
        let id = self.next_id;
        self.next_id += 1;
        self.handoffs.insert(id, HandoffState {
            handoff_id: id,
            order_id,
            from_agent,
            to_agent,
            point,
            phase: HandoffPhase::Requested,
            payment_pct: 0.30,
        });
        id
    }

    /// Mark payment sent.
    pub fn mark_payment_sent(&mut self, handoff_id: HandoffId) {
        if let Some(h) = self.handoffs.get_mut(&handoff_id) {
            h.phase = HandoffPhase::PaymentSent;
        }
    }

    /// Mark handoff complete.
    pub fn mark_complete(&mut self, handoff_id: HandoffId) {
        if let Some(h) = self.handoffs.get_mut(&handoff_id) {
            h.phase = HandoffPhase::Complete;
        }
    }

    /// Mark handoff failed (voids payment).
    pub fn mark_failed(&mut self, handoff_id: HandoffId) {
        if let Some(h) = self.handoffs.get_mut(&handoff_id) {
            h.phase = HandoffPhase::Failed;
        }
    }

    /// Find active handoffs that involve a given agent (either end).
    pub fn handoffs_for_agent(&self, agent_id: &str) -> Vec<HandoffId> {
        self.handoffs
            .iter()
            .filter(|(_, h)| h.from_agent == agent_id || h.to_agent == agent_id)
            .filter(|(_, h)| h.phase != HandoffPhase::Complete && h.phase != HandoffPhase::Failed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Calculate handoff payment amount (30% of escrow).
    pub fn handoff_payment_amount(&self, handoff_id: HandoffId, total_escrow: u64) -> u64 {
        if let Some(h) = self.handoffs.get(&handoff_id) {
            (total_escrow as f64 * h.payment_pct) as u64
        } else {
            0
        }
    }
}
