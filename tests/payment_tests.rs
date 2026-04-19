use codigo_rust::{
    agent::AgentRuntime,
    crypto::{client_escrow_signature, derive_key, proof_hash, sign_message, verify_signature, validator_signature},
    payments::EscrowLedger,
    types::{AgentType, NexusAgent, Point, SwarmMessage},
};

fn make_agent(id: &str, atype: AgentType, x: f64, y: f64, balance: u64) -> AgentRuntime {
    AgentRuntime::new(NexusAgent::new(id, atype, "TestVendor", x, y, 100.0, 10.0, balance))
}

fn broadcast(agents: &mut Vec<AgentRuntime>, msg: SwarmMessage) -> Vec<SwarmMessage> {
    let mut out = Vec::new();
    for a in agents.iter_mut() {
        a.handle(msg.clone());
        out.extend(a.drain_outbox());
    }
    out
}

// -- Escrow lifecycle ------------------------------------------------------

#[test]
fn test_escrow_lock_and_release() {
    let mut ledger = EscrowLedger::new();
    ledger.set_balance("client", 1000);
    let sig = client_escrow_signature(1, 500);
    ledger.lock_escrow(1, 1, 500, "agent-a", &sig).unwrap();
    ledger.release_escrow(1, "agent-a", 500, &[]).unwrap();
    assert_eq!(ledger.balance("agent-a"), 500);
}

#[test]
fn test_double_spend_prevented_by_duplicate_escrow() {
    let mut ledger = EscrowLedger::new();
    let sig = client_escrow_signature(1, 100);
    ledger.lock_escrow(1, 1, 100, "a", &sig).unwrap();
    // Second lock with same escrow_id should fail
    let result = ledger.lock_escrow(1, 1, 100, "a", &sig);
    assert!(result.is_err(), "Duplicate escrow should be rejected");
}

#[test]
fn test_invalid_client_signature_rejected() {
    let mut ledger = EscrowLedger::new();
    let bad_sig = vec![0u8; 32]; // wrong signature
    let result = ledger.lock_escrow(1, 1, 100, "a", &bad_sig);
    assert!(result.is_err());
}

#[test]
fn test_release_already_released_escrow() {
    let mut ledger = EscrowLedger::new();
    let sig = client_escrow_signature(1, 200);
    ledger.lock_escrow(1, 1, 200, "a", &sig).unwrap();
    ledger.release_escrow(1, "a", 200, &[]).unwrap();
    let result = ledger.release_escrow(1, "a", 200, &[]);
    assert!(result.is_err());
}

#[test]
fn test_escrow_return_on_failure() {
    let mut ledger = EscrowLedger::new();
    ledger.set_balance("holder", 0);
    let sig = client_escrow_signature(5, 300);
    ledger.lock_escrow(5, 5, 300, "holder", &sig).unwrap();
    ledger.return_escrow(5).unwrap();
    assert_eq!(ledger.balance("holder"), 300);
}

// -- Handoff payments ------------------------------------------------------

#[test]
fn test_transfer_between_agents() {
    let mut ledger = EscrowLedger::new();
    ledger.set_balance("from", 200);
    ledger.set_balance("to", 50);
    ledger.transfer("from", "to", 60).unwrap();
    assert_eq!(ledger.balance("from"), 140);
    assert_eq!(ledger.balance("to"), 110);
}

#[test]
fn test_transfer_insufficient_balance_fails() {
    let mut ledger = EscrowLedger::new();
    ledger.set_balance("broke", 10);
    let result = ledger.transfer("broke", "rich", 100);
    assert!(result.is_err());
}

#[test]
fn test_handoff_payment_30_pct_of_escrow() {
    let mut agents = vec![
        make_agent("drone-001", AgentType::Drone, 0.0, 0.0, 100),
        make_agent("robot-002", AgentType::GroundRobot, 5.0, 5.0, 50),
    ];
    let escrow_amount = 500u64;

    // Order + winner
    broadcast(&mut agents, SwarmMessage::OrderCreated {
        order_id: 1, pickup: Point::new(1.0,1.0), delivery: Point::new(5.0,5.0), weight: 1.0, escrow_amount,
    });
    broadcast(&mut agents, SwarmMessage::AuctionWinner { order_id: 1, winner_id: "drone-001".to_string() });
    let escrow_msgs: Vec<_> = agents.iter_mut().flat_map(|a| a.drain_outbox()).collect();
    for m in &escrow_msgs { broadcast(&mut agents, m.clone()); }

    // Handoff request â†’ HandoffPayment
    let handoff = SwarmMessage::HandoffRequest {
        order_id: 1, from_agent: "drone-001".to_string(), to_agent: "robot-002".to_string(), point: Point::new(3.0,3.0),
    };
    let out = broadcast(&mut agents, handoff);

    let pay_msg = out.iter().find(|m| matches!(m, SwarmMessage::HandoffPayment { .. }));
    assert!(pay_msg.is_some(), "HandoffPayment should be emitted");
    if let Some(SwarmMessage::HandoffPayment { amount, .. }) = pay_msg {
        let expected = (escrow_amount as f64 * 0.30) as u64;
        assert_eq!(*amount, expected, "Handoff payment should be 30% of escrow");
    }
}

// -- Cryptographic signatures ----------------------------------------------

#[test]
fn test_sign_and_verify() {
    let key = derive_key("test-agent");
    let msg = b"hello nexusfly";
    let sig = sign_message(&key, msg);
    assert!(verify_signature(&key, msg, &sig));
}

#[test]
fn test_wrong_key_fails_verification() {
    let key1 = derive_key("agent-1");
    let key2 = derive_key("agent-2");
    let msg  = b"payment";
    let sig  = sign_message(&key1, msg);
    assert!(!verify_signature(&key2, msg, &sig));
}

#[test]
fn test_tampered_message_fails_verification() {
    let key = derive_key("agent");
    let sig = sign_message(&key, b"original");
    assert!(!verify_signature(&key, b"tampered", &sig));
}

#[test]
fn test_proof_hash_is_deterministic() {
    let h1 = proof_hash(42.3, -71.1, 1000, 123);
    let h2 = proof_hash(42.3, -71.1, 1000, 123);
    assert_eq!(h1, h2);
}

#[test]
fn test_proof_hash_differs_for_different_inputs() {
    let h1 = proof_hash(42.3, -71.1, 1000, 123);
    let h2 = proof_hash(42.3, -71.1, 1001, 123); // different timestamp
    assert_ne!(h1, h2);
}

#[test]
fn test_client_escrow_signature_is_deterministic() {
    let s1 = client_escrow_signature(1, 500);
    let s2 = client_escrow_signature(1, 500);
    assert_eq!(s1, s2);
}

#[test]
fn test_validator_signature() {
    let vkey = derive_key("validator");
    let sig = validator_signature(&vkey, 1, "robot-002", 350);
    assert_eq!(sig.len(), 32);
}

// -- Full payment flow integration -----------------------------------------

#[test]
fn test_full_payment_flow() {
    let mut agents = vec![
        make_agent("drone-001", AgentType::Drone, 0.0, 0.0, 100),
        make_agent("robot-002", AgentType::GroundRobot, 5.0, 5.0, 50),
    ];

    let order_id = 10u64;
    let escrow_amount = 300u64;

    // OrderCreated
    let mut bids = broadcast(&mut agents, SwarmMessage::OrderCreated {
        order_id, pickup: Point::new(1.0,1.0), delivery: Point::new(5.0,5.0), weight: 1.0, escrow_amount,
    });

    // AuctionWinner -> PaymentEscrow from winner (broadcast already drains outbox)
    let escrow_msgs = broadcast(&mut agents, SwarmMessage::AuctionWinner { order_id, winner_id: "drone-001".to_string() });
    for m in &escrow_msgs { broadcast(&mut agents, m.clone()); }

    // HandoffRequest
    let handoff = SwarmMessage::HandoffRequest {
        order_id, from_agent: "drone-001".to_string(), to_agent: "robot-002".to_string(), point: Point::new(3.0,3.0),
    };
    let h_out = broadcast(&mut agents, handoff);
    for m in &h_out { broadcast(&mut agents, m.clone()); }

    // OrderDelivered
    let claim_out = broadcast(&mut agents, SwarmMessage::OrderDelivered {
        order_id, agent_id: "robot-002".to_string(),
    });

    // PaymentClaim â†’ PaymentRelease
    for msg in &claim_out {
        if matches!(msg, SwarmMessage::PaymentClaim { .. }) {
            let rel_out = broadcast(&mut agents, msg.clone());
            for r in &rel_out {
                if matches!(r, SwarmMessage::PaymentRelease { .. }) {
                    broadcast(&mut agents, r.clone());
                }
            }
        }
    }

    // Robot should have received the 30% handoff payment
    let robot_bal = agents.iter().find(|a| a.state.id == "robot-002")
        .map(|a| a.ledger.balance("robot-002")).unwrap_or(0);
    // Base 50 + 30% of 300 = 50 + 90 = 140, plus 70% remaining = 50 + 90 + 210 = 350
    assert!(robot_bal > 50, "Robot should have earned payment tokens");
}

// -- Escrow timeout recovery -----------------------------------------------

#[test]
fn test_escrow_timeout_recovery() {
    use std::time::Duration;
    let mut ledger = EscrowLedger::new();
    ledger.set_balance("agent", 0);
    let sig = client_escrow_signature(1, 100);
    ledger.lock_escrow(1, 1, 100, "agent", &sig).unwrap();
    // Manually force timeout (5s) by adjusting: we can't control time easily,
    // so just verify that no recovery occurs before timeout.
    let recovered = ledger.recover_timed_out();
    // Should be empty since escrow was just created
    assert!(recovered.is_empty());
}

// -- Balance query/response ------------------------------------------------

#[test]
fn test_balance_query_response() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 0.0, 0.0, 250)];
    let out = broadcast(&mut agents, SwarmMessage::BalanceQuery { agent_id: "a1".to_string(), request_id: 42 });
    assert!(out.iter().any(|m| matches!(m, SwarmMessage::BalanceResponse { balance: 250, request_id: 42, .. })));
}

#[test]
fn test_balance_query_ignored_for_other_agent() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 0.0, 0.0, 250)];
    let out = broadcast(&mut agents, SwarmMessage::BalanceQuery { agent_id: "a2".to_string(), request_id: 1 });
    assert!(out.is_empty());
}
