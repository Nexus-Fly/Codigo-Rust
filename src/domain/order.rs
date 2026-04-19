use anyhow::{bail, Result};

use crate::types::{NexusMessage, NodeId, Order, OrderStatus, Timestamp};

// ─────────────────────────────────────────────────────────────────────────────
// State machine helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Enforce that `order` is currently in `expected` status.
fn require_status(order: &Order, expected: OrderStatus) -> Result<()> {
    if order.status != expected {
        bail!(
            "Order '{}': expected status {:?}, found {:?}",
            order.order_id,
            expected,
            order.status
        );
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle functions
// Each function takes ownership of the Order, mutates it, and returns it
// together with the NexusMessage that should be broadcast over the swarm.
// ─────────────────────────────────────────────────────────────────────────────

/// Create a brand-new order in `Created` status.
///
/// Returns the `Order` value and the `NexusMessage::OrderCreated` to broadcast.
pub fn create_order(
    order_id: impl Into<String>,
    origin: NodeId,
    pickup: (f64, f64),
    dropoff: (f64, f64),
    now: Timestamp,
) -> (Order, NexusMessage) {
    let order = Order {
        order_id: order_id.into(),
        status: OrderStatus::Created,
        origin,
        assigned_to: None,
        pickup,
        dropoff,
        created_at: now,
        updated_at: now,
    };
    let msg = NexusMessage::OrderCreated(order.clone());
    (order, msg)
}

/// Transition: `Created` → `Bidding`.
///
/// Called when the auction phase begins for this order.
pub fn mark_bidding(mut order: Order, now: Timestamp) -> Result<Order> {
    require_status(&order, OrderStatus::Created)?;
    order.status = OrderStatus::Bidding;
    order.updated_at = now;
    Ok(order)
}

/// Transition: `Bidding` → `Assigned`.
///
/// Records the winning agent. Returns an `AuctionWinner` message to broadcast.
pub fn assign_order(
    mut order: Order,
    winner: NodeId,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    require_status(&order, OrderStatus::Bidding)?;
    order.assigned_to = Some(winner.clone());
    order.status = OrderStatus::Assigned;
    order.updated_at = now;
    let msg = NexusMessage::AuctionWinner {
        order_id: order.order_id.clone(),
        winner,
    };
    Ok((order, msg))
}

/// Transition: `Assigned` → `Pickup`.
///
/// The assigned agent has arrived at the pickup point.
pub fn mark_pickup(mut order: Order, now: Timestamp) -> Result<Order> {
    require_status(&order, OrderStatus::Assigned)?;
    order.status = OrderStatus::Pickup;
    order.updated_at = now;
    Ok(order)
}

/// Transition: `Pickup` → `InTransit`.
///
/// The agent has collected the package and is en route to the dropoff.
pub fn mark_in_transit(mut order: Order, now: Timestamp) -> Result<Order> {
    require_status(&order, OrderStatus::Pickup)?;
    order.status = OrderStatus::InTransit;
    order.updated_at = now;
    Ok(order)
}

/// Transition: `InTransit` → `HandoffPending`.
///
/// The current carrier cannot complete delivery and requests a hand-off.
/// Returns a `HandoffRequest` message to broadcast.
pub fn request_handoff(
    mut order: Order,
    to: NodeId,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    require_status(&order, OrderStatus::InTransit)?;
    let from = order
        .assigned_to
        .clone()
        .unwrap_or_else(|| order.origin.clone());
    order.status = OrderStatus::HandoffPending;
    order.updated_at = now;
    let msg = NexusMessage::HandoffRequest {
        order_id: order.order_id.clone(),
        from,
        to,
    };
    Ok((order, msg))
}

/// Transition: `HandoffPending` → `HandedOff`.
///
/// The receiving agent has accepted the package.
/// Returns a `HandoffComplete` message to broadcast.
pub fn complete_handoff(
    mut order: Order,
    new_holder: NodeId,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    require_status(&order, OrderStatus::HandoffPending)?;
    order.assigned_to = Some(new_holder.clone());
    order.status = OrderStatus::HandedOff;
    order.updated_at = now;
    let msg = NexusMessage::HandoffComplete {
        order_id: order.order_id.clone(),
        new_holder,
    };
    Ok((order, msg))
}

/// Transition: `HandedOff` | `InTransit` → `Delivered`.
///
/// Accepts both `InTransit` (direct delivery) and `HandedOff` (after handoff).
/// Returns an `OrderDelivered` message to broadcast.
pub fn mark_delivered(
    mut order: Order,
    delivered_by: NodeId,
    now: Timestamp,
) -> Result<(Order, NexusMessage)> {
    if order.status != OrderStatus::InTransit && order.status != OrderStatus::HandedOff {
        bail!(
            "Order '{}': cannot mark Delivered from {:?}",
            order.order_id,
            order.status
        );
    }
    order.status = OrderStatus::Delivered;
    order.updated_at = now;
    let msg = NexusMessage::OrderDelivered {
        order_id: order.order_id.clone(),
        delivered_by,
        at: now,
    };
    Ok((order, msg))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const T: Timestamp = 1_700_000_000;
    const PICKUP: (f64, f64) = (40.7128, -74.0060);
    const DROPOFF: (f64, f64) = (40.7580, -73.9855);

    fn new_order() -> Order {
        let (order, _) = create_order("order-1", "node-a".into(), PICKUP, DROPOFF, T);
        order
    }

    #[test]
    fn create_sets_created_status() {
        let order = new_order();
        assert_eq!(order.status, OrderStatus::Created);
        assert_eq!(order.order_id, "order-1");
        assert!(order.assigned_to.is_none());
    }

    #[test]
    fn create_emits_order_created_message() {
        let (_, msg) = create_order("order-1", "node-a".into(), PICKUP, DROPOFF, T);
        assert!(matches!(msg, NexusMessage::OrderCreated(_)));
    }

    #[test]
    fn happy_path_direct_delivery() {
        let order = new_order();

        let order = mark_bidding(order, T + 1).unwrap();
        assert_eq!(order.status, OrderStatus::Bidding);

        let (order, msg) = assign_order(order, "node-b".into(), T + 2).unwrap();
        assert_eq!(order.status, OrderStatus::Assigned);
        assert!(matches!(msg, NexusMessage::AuctionWinner { .. }));
        assert_eq!(order.assigned_to.as_deref(), Some("node-b"));

        let order = mark_pickup(order, T + 3).unwrap();
        assert_eq!(order.status, OrderStatus::Pickup);

        let order = mark_in_transit(order, T + 4).unwrap();
        assert_eq!(order.status, OrderStatus::InTransit);

        let (order, msg) = mark_delivered(order, "node-b".into(), T + 5).unwrap();
        assert_eq!(order.status, OrderStatus::Delivered);
        assert!(matches!(msg, NexusMessage::OrderDelivered { .. }));
    }

    #[test]
    fn happy_path_with_handoff() {
        let order = new_order();
        let order = mark_bidding(order, T + 1).unwrap();
        let (order, _) = assign_order(order, "node-b".into(), T + 2).unwrap();
        let order = mark_pickup(order, T + 3).unwrap();
        let order = mark_in_transit(order, T + 4).unwrap();

        let (order, msg) = request_handoff(order, "node-c".into(), T + 5).unwrap();
        assert_eq!(order.status, OrderStatus::HandoffPending);
        assert!(matches!(msg, NexusMessage::HandoffRequest { .. }));

        let (order, msg) = complete_handoff(order, "node-c".into(), T + 6).unwrap();
        assert_eq!(order.status, OrderStatus::HandedOff);
        assert_eq!(order.assigned_to.as_deref(), Some("node-c"));
        assert!(matches!(msg, NexusMessage::HandoffComplete { .. }));

        let (order, _) = mark_delivered(order, "node-c".into(), T + 7).unwrap();
        assert_eq!(order.status, OrderStatus::Delivered);
    }

    #[test]
    fn invalid_transition_returns_error() {
        let order = new_order(); // Created
        // Cannot skip to Assigned without going through Bidding
        assert!(assign_order(order, "node-b".into(), T).is_err());
    }

    #[test]
    fn cannot_deliver_from_created() {
        let order = new_order();
        assert!(mark_delivered(order, "node-b".into(), T).is_err());
    }

    #[test]
    fn cannot_handoff_before_in_transit() {
        let order = new_order();
        let order = mark_bidding(order, T).unwrap();
        let (order, _) = assign_order(order, "node-b".into(), T).unwrap();
        // Still Assigned, not InTransit
        assert!(request_handoff(order, "node-c".into(), T).is_err());
    }
}

