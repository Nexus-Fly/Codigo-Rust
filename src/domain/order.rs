use crate::types::{NodeId, Round};

/// A work order submitted by an agent.
#[derive(Debug, Clone)]
pub struct Order {
    pub origin: NodeId,
    pub round: Round,
    pub payload: String,
}
