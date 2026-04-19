use anyhow::{Context, Result};

use crate::types::NexusMessage;

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Serialise a [`NexusMessage`] to JSON bytes.
///
/// The resulting `Vec<u8>` is valid UTF-8 and can be passed directly to
/// `Transaction::allocate` in Tashi Vertex.
pub fn encode_message(msg: &NexusMessage) -> Result<Vec<u8>> {
    serde_json::to_vec(msg).context("Failed to encode NexusMessage as JSON")
}

/// Deserialise a [`NexusMessage`] from raw bytes.
///
/// Accepts both plain JSON bytes and null-terminated payloads produced by the
/// existing `send_transaction_cstr` helper in `main.rs`.
pub fn decode_message(bytes: &[u8]) -> Result<NexusMessage> {
    let trimmed = bytes.strip_suffix(b"\0").unwrap_or(bytes);
    serde_json::from_slice(trimmed).context("Failed to decode NexusMessage from JSON")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AgentKind, AgentState, AgentStatus, AuctionBid, NexusMessage, Order, OrderStatus,
        SafetyZone,
    };

    fn sample_agent_state() -> NexusMessage {
        NexusMessage::AgentState(AgentState {
            node_id: "node-abc".into(),
            kind: AgentKind::Drone,
            status: AgentStatus::Idle,
            location: (40.7128, -74.0060),
            battery_pct: 87,
            updated_at: 1_700_000_000,
        })
    }

    fn sample_order() -> NexusMessage {
        NexusMessage::OrderCreated(Order {
            order_id: "order-1".into(),
            status: OrderStatus::Created,
            origin: "node-abc".into(),
            assigned_to: None,
            pickup: (40.7128, -74.0060),
            dropoff: (40.7580, -73.9855),
            created_at: 1_700_000_001,
            updated_at: 1_700_000_001,
        })
    }

    fn sample_bid() -> NexusMessage {
        NexusMessage::AuctionBid(AuctionBid {
            order_id: "order-1".into(),
            bidder: "node-xyz".into(),
            eta_s: 45,
            battery_pct: 72,
            submitted_at: 1_700_000_010,
        })
    }

    fn sample_safety_alert() -> NexusMessage {
        NexusMessage::SafetyAlert(SafetyZone {
            zone_id: "zone-99".into(),
            center: (40.730610, -73.935242),
            radius_m: 200.0,
            active: true,
            declared_at: 1_700_000_020,
        })
    }

    /// Helper: encode then decode, asserting the JSON survives a roundtrip.
    fn roundtrip(msg: &NexusMessage) {
        let encoded = encode_message(msg).expect("encode failed");
        assert!(!encoded.is_empty(), "encoded bytes must not be empty");

        // Must be valid UTF-8
        let _json = std::str::from_utf8(&encoded).expect("encoded bytes are not valid UTF-8");

        let decoded = decode_message(&encoded).expect("decode failed");

        // Re-encode the decoded value and compare JSON strings for equality
        let re_encoded = encode_message(&decoded).expect("re-encode failed");
        assert_eq!(encoded, re_encoded, "roundtrip JSON mismatch");
    }

    #[test]
    fn roundtrip_agent_state() {
        roundtrip(&sample_agent_state());
    }

    #[test]
    fn roundtrip_order_created() {
        roundtrip(&sample_order());
    }

    #[test]
    fn roundtrip_auction_bid() {
        roundtrip(&sample_bid());
    }

    #[test]
    fn roundtrip_safety_alert() {
        roundtrip(&sample_safety_alert());
    }

    #[test]
    fn roundtrip_struct_variants() {
        let msgs: &[NexusMessage] = &[
            NexusMessage::AuctionWinner {
                order_id: "order-1".into(),
                winner: "node-xyz".into(),
            },
            NexusMessage::HandoffRequest {
                order_id: "order-1".into(),
                from: "node-abc".into(),
                to: "node-xyz".into(),
            },
            NexusMessage::HandoffComplete {
                order_id: "order-1".into(),
                new_holder: "node-xyz".into(),
            },
            NexusMessage::AgentFailure {
                node_id: "node-abc".into(),
                reason: "battery depleted".into(),
            },
            NexusMessage::SafetyClear {
                zone_id: "zone-99".into(),
            },
            NexusMessage::OrderDelivered {
                order_id: "order-1".into(),
                delivered_by: "node-xyz".into(),
                at: 1_700_000_999,
            },
        ];

        for msg in msgs {
            roundtrip(msg);
        }
    }

    #[test]
    fn decode_null_terminated_payload() {
        let msg = sample_agent_state();
        let mut encoded = encode_message(&msg).expect("encode failed");
        encoded.push(0); // simulate null-terminated Vertex transaction

        decode_message(&encoded).expect("should decode null-terminated payload");
    }

    #[test]
    fn decode_invalid_bytes_returns_error() {
        let result = decode_message(b"not valid json");
        assert!(result.is_err(), "expected error for invalid JSON");
    }
}

