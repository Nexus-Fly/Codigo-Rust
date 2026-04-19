use crate::types::NodeId;

/// Represents a task hand-off between agents.
#[derive(Debug, Clone)]
pub struct Handoff {
    pub from: NodeId,
    pub to: NodeId,
}
