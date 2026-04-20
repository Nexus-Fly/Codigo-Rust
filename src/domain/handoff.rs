#![allow(dead_code)]

use anyhow::{bail, Result};

use crate::{
    domain::order as order_fsm,
    types::{AgentStatus, NexusMessage, NodeId, Order, OrderStatus, Timestamp},
};

// ---------------------------------------------------------------------------
// Business-rule validation
// ---------------------------------------------------------------------------

/// Validate that a handoff request is permissible without mutating state.
///
/// Rules enforced:
/// - The order must be `InTransit` (not pending, not already handed off).
/// - `requester` must be the agent currently assigned to the order.
/// - `destination_status` must be `Idle` (available to accept work).
pub fn validate_handoff(
    order: &Order,
    requester: &str,
    destination_status: &AgentStatus,
) -> Result<()> {
    // State guard — also catches duplicate requests (HandoffPending is not InTransit).
    if order.status != OrderStatus::InTransit {
        bail!(
            "Order '{}': handoff requires InTransit status, found {:?}",
            order.order_id,
            order.status
        );
    }

    // Ownership check.
    let owner = order
        .assigned_to
        .as_deref()
        .unwrap_or(order.origin.as_str());
    if owner != requester {
        bail!(
            "Order '{}': requester '{}' does not own the order (owner: '{}')",
            order.order_id,
            requester,
            owner
        );
    }

    // Destination availability check.
    if *destination_status != AgentStatus::Idle {
        bail!(
            "Order '{}': destination agent is not available ({:?})",
            order.order_id,
            destination_status
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// State transitions
// ---------------------------------------------------------------------------

/// Validate and initiate a handoff: `InTransit` → `HandoffPending`.
///
/// Enforces all business rules via `validate_handoff`, then delegates the
/// FSM transition to `domain::order::request_handoff`.
///
/// Returns the updated `Order` and the `HandoffRequest` message to broadcast.
pub fn create_handoff_request(
    order: Order,
    requester: &str,
    destination: NodeId,
    destination_status: &AgentStatus,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    validate_handoff(&order, requester, destination_status)?;
    order_fsm::request_handoff(order, destination, now)
}

/// Complete an accepted handoff: `HandoffPending` → `HandedOff`.
///
/// The destination agent must still be `Idle` (it hasn't taken another job).
/// Delegates the FSM transition to `domain::order::complete_handoff`.
///
/// Returns the updated `Order` and the `HandoffComplete` message to broadcast.
pub fn complete_handoff(
    order: Order,
    new_holder: NodeId,
    destination_status: &AgentStatus,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    if order.status != OrderStatus::HandoffPending {
        bail!(
            "Order '{}': complete_handoff requires HandoffPending status, found {:?}",
            order.order_id,
            order.status
        );
    }
    if *destination_status != AgentStatus::Idle {
        bail!(
            "Order '{}': destination agent is no longer available ({:?})",
            order.order_id,
            destination_status
        );
    }
    order_fsm::complete_handoff(order, new_holder, now)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OrderStatus;

    const T: Timestamp = 1_700_000_000;
    const PICKUP: (f64, f64) = (40.7128, -74.0060);
    const DROPOFF: (f64, f64) = (40.7580, -73.9855);

    // Build an order already in InTransit, assigned to "carrier-1".
    fn in_transit_order() -> Order {
        use crate::domain::order as fsm;
        let (o, _) = fsm::create_order("ord-1", "carrier-1".to_string(), PICKUP, DROPOFF, T);
        let o = fsm::mark_bidding(o, T).unwrap();
        let (o, _) = fsm::assign_order(o, "carrier-1".into(), T).unwrap();
        let o = fsm::mark_pickup(o, T).unwrap();
        fsm::mark_in_transit(o, T).unwrap()
    }

    #[test]
    fn valid_handoff_request_advances_state() {
        let order = in_transit_order();
        let (updated, msg) = create_handoff_request(
            order,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap();
        assert_eq!(updated.status, OrderStatus::HandoffPending);
        assert!(matches!(msg, NexusMessage::HandoffRequest { .. }));
    }

    #[test]
    fn valid_complete_handoff_advances_state() {
        let order = in_transit_order();
        let (pending, _) = create_handoff_request(
            order,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap();
        let (done, msg) =
            complete_handoff(pending, "carrier-2".into(), &AgentStatus::Idle, T).unwrap();
        assert_eq!(done.status, OrderStatus::HandedOff);
        assert_eq!(done.assigned_to.as_deref(), Some("carrier-2"));
        assert!(matches!(msg, NexusMessage::HandoffComplete { .. }));
    }

    #[test]
    fn invalid_owner_is_rejected() {
        let order = in_transit_order();
        let err = create_handoff_request(
            order,
            "impostor",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not own"));
    }

    #[test]
    fn unavailable_destination_is_rejected() {
        let order = in_transit_order();
        let err = create_handoff_request(
            order,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Busy,
            T,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not available"));
    }

    #[test]
    fn duplicate_handoff_request_is_rejected() {
        let order = in_transit_order();
        // First request succeeds and moves to HandoffPending.
        let (pending, _) = create_handoff_request(
            order,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap();
        // Second request on the same order must fail (not InTransit anymore).
        let err = create_handoff_request(
            pending,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap_err();
        assert!(err.to_string().contains("InTransit"));
    }

    #[test]
    fn complete_handoff_fails_if_destination_became_busy() {
        let order = in_transit_order();
        let (pending, _) = create_handoff_request(
            order,
            "carrier-1",
            "carrier-2".into(),
            &AgentStatus::Idle,
            T,
        )
        .unwrap();
        let err =
            complete_handoff(pending, "carrier-2".into(), &AgentStatus::Busy, T).unwrap_err();
        assert!(err.to_string().contains("no longer available"));
    }
}

