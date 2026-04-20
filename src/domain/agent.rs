#![allow(dead_code)]

use crate::types::NodeId;

/// Represents a participant node in the swarm.
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: NodeId,
}

impl Agent {
    pub fn new(id: impl Into<NodeId>) -> Self {
        Self { id: id.into() }
    }
}
