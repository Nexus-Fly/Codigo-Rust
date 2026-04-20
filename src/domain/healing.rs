#![allow(dead_code)]

use std::collections::HashMap;

use crate::types::{NodeId, Order, OrderStatus, Timestamp};

// ---------------------------------------------------------------------------
// HeartbeatTracker
// ---------------------------------------------------------------------------

/// Tracks the last heartbeat timestamp received from each agent.
///
/// Timestamps are passed in explicitly so the logic is fully deterministic
/// and testable without touching the real system clock.
#[derive(Debug, Default)]
pub struct HeartbeatTracker {
    /// `node_id` → last seen timestamp (seconds).
    last_seen: HashMap<NodeId, Timestamp>,
}

impl HeartbeatTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or refresh) a heartbeat for `node_id` at time `now`.
    pub fn record_heartbeat(&mut self, node_id: impl Into<NodeId>, now: Timestamp) {
        self.last_seen.insert(node_id.into(), now);
    }

    /// Return the node ids of every agent whose last heartbeat was received
    /// more than `timeout_s` seconds before `now`.
    ///
    /// Agents with no heartbeat on record are considered failed.
    pub fn detect_failed_agents(&self, now: Timestamp, timeout_s: u64) -> Vec<NodeId> {
        self.last_seen
            .iter()
            .filter(|&(_, last)| now.saturating_sub(*last) > timeout_s)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Return the `order_id`s from `orders` that are currently assigned to a
    /// failed agent and therefore need to be re-auctioned.
    ///
    /// An order is eligible if:
    /// - it is in `Assigned`, `Pickup`, `InTransit`, or `HandoffPending` status, and
    /// - its `assigned_to` agent is in the failed set.
    pub fn orders_to_reauction<'a>(
        &self,
        orders: impl Iterator<Item = &'a Order>,
        now: Timestamp,
        timeout_s: u64,
    ) -> Vec<String> {
        let failed: std::collections::HashSet<NodeId> =
            self.detect_failed_agents(now, timeout_s).into_iter().collect();

        orders
            .filter(|o| {
                matches!(
                    o.status,
                    OrderStatus::Assigned
                        | OrderStatus::Pickup
                        | OrderStatus::InTransit
                        | OrderStatus::HandoffPending
                ) && o
                    .assigned_to
                    .as_ref()
                    .map(|id| failed.contains(id))
                    .unwrap_or(false)
            })
            .map(|o| o.order_id.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order as fsm;

    const BASE: Timestamp = 1_700_000_000;
    const TIMEOUT: u64 = 30;
    const PICKUP: (f64, f64) = (40.71, -74.00);
    const DROPOFF: (f64, f64) = (40.75, -73.98);

    fn assigned_order(order_id: &str, agent: &str) -> Order {
        let (o, _) = fsm::create_order(order_id, agent.to_string(), PICKUP, DROPOFF, BASE);
        let o = fsm::mark_bidding(o, BASE).unwrap();
        let (o, _) = fsm::assign_order(o, agent.to_string(), BASE).unwrap();
        o
    }

    #[test]
    fn heartbeat_recording_updates_last_seen() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("node-1", BASE);
        // Well within timeout — not failed.
        let failed = tracker.detect_failed_agents(BASE + TIMEOUT / 2, TIMEOUT);
        assert!(failed.is_empty());
    }

    #[test]
    fn no_false_positive_before_timeout() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("node-1", BASE);
        // Exactly at the boundary (not strictly greater) — still healthy.
        let failed = tracker.detect_failed_agents(BASE + TIMEOUT, TIMEOUT);
        assert!(!failed.contains(&"node-1".to_string()));
    }

    #[test]
    fn failure_detected_after_timeout() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("node-1", BASE);
        // One second past the timeout.
        let failed = tracker.detect_failed_agents(BASE + TIMEOUT + 1, TIMEOUT);
        assert!(failed.contains(&"node-1".to_string()));
    }

    #[test]
    fn refreshed_heartbeat_clears_failure() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("node-1", BASE);
        // Would be failed at BASE+31, but we refresh at BASE+20.
        tracker.record_heartbeat("node-1", BASE + 20);
        let failed = tracker.detect_failed_agents(BASE + TIMEOUT + 1, TIMEOUT);
        assert!(!failed.contains(&"node-1".to_string()));
    }

    #[test]
    fn orders_to_reauction_returns_affected_orders() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("agent-a", BASE);
        tracker.record_heartbeat("agent-b", BASE);

        let order_a = assigned_order("ord-a", "agent-a");
        let order_b = assigned_order("ord-b", "agent-b");

        // agent-a goes silent; agent-b stays alive.
        let now = BASE + TIMEOUT + 1;
        tracker.record_heartbeat("agent-b", now - 1);

        let to_reauction =
            tracker.orders_to_reauction([&order_a, &order_b].into_iter(), now, TIMEOUT);

        assert_eq!(to_reauction, vec!["ord-a"]);
    }

    #[test]
    fn delivered_orders_are_not_reauctioned() {
        let mut tracker = HeartbeatTracker::new();
        tracker.record_heartbeat("agent-a", BASE);

        let mut order = assigned_order("ord-delivered", "agent-a");
        // Manually advance to Delivered for this test.
        let o = fsm::mark_pickup(order, BASE).unwrap();
        let o = fsm::mark_in_transit(o, BASE).unwrap();
        let (o, _) = fsm::mark_delivered(o, "agent-a".to_string(), BASE).unwrap();
        order = o;

        let now = BASE + TIMEOUT + 1;
        let to_reauction =
            tracker.orders_to_reauction(std::iter::once(&order), now, TIMEOUT);

        assert!(to_reauction.is_empty());
    }
}

