use codigo_rust::{
    agent::AgentRuntime,
    auction::{AuctionBook, calculate_bid_score},
    handoff::HandoffManager,
    healing::FailureDetector,
    safety::SafetyMesh,
    types::{AgentStatus, AgentType, NexusAgent, Point, SwarmMessage},
};
use std::collections::HashMap;

fn make_agent(id: &str, atype: AgentType, x: f64, y: f64, battery: f64, balance: u64) -> AgentRuntime {
    AgentRuntime::new(NexusAgent::new(id, atype, "TestVendor", x, y, battery, 10.0, balance))
}

fn broadcast(agents: &mut Vec<AgentRuntime>, msg: SwarmMessage) -> Vec<SwarmMessage> {
    let mut out = Vec::new();
    for a in agents.iter_mut() {
        a.handle(msg.clone());
        out.extend(a.drain_outbox());
    }
    out
}

// â”€â”€ Protocol 1: P2P Discovery â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_agent_state_broadcast() {
    let agent = NexusAgent::new("a1", AgentType::Drone, "v", 1.0, 2.0, 80.0, 5.0, 100);
    let rt = AgentRuntime::new(agent);
    let msg = rt.broadcast_state();
    assert!(matches!(msg, SwarmMessage::AgentState { .. }));
}

#[test]
fn test_agent_state_received() {
    let mut rt = make_agent("a1", AgentType::Drone, 0.0, 0.0, 100.0, 100);
    let msg = SwarmMessage::AgentState {
        agent_id: "a2".to_string(),
        agent_type: AgentType::GroundRobot,
        vendor: "v".to_string(),
        x: 1.0, y: 1.0,
        battery: 90.0, capacity: 5.0,
        status: AgentStatus::Idle,
    };
    rt.handle(msg); // Should not panic
}

// â”€â”€ Protocol 2: Auction â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_bid_score_increases_with_closer_agent() {
    let mut agent_close = NexusAgent::new("c", AgentType::Drone, "v", 1.0, 1.0, 100.0, 10.0, 0);
    let mut agent_far   = NexusAgent::new("f", AgentType::Drone, "v", 100.0, 100.0, 100.0, 10.0, 0);
    let pickup = Point::new(1.5, 1.5);
    let score_close = calculate_bid_score(&agent_close, &pickup, 1.0);
    let score_far   = calculate_bid_score(&agent_far,   &pickup, 1.0);
    assert!(score_close > score_far, "Closer agent should score higher");
}

#[test]
fn test_bid_score_zero_when_insufficient_capacity() {
    let agent = NexusAgent::new("a", AgentType::Drone, "v", 1.0, 1.0, 100.0, 1.0, 0);
    let pickup = Point::new(2.0, 2.0);
    let score = calculate_bid_score(&agent, &pickup, 5.0); // weight > capacity
    assert_eq!(score, 0.0);
}

#[test]
fn test_auction_book_records_bids() {
    let mut book = AuctionBook::new();
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 1, pickup: Point::new(0.0,0.0), delivery: Point::new(1.0,1.0), weight: 1.0, escrow_amount: 100,
    };
    book.open_auction(&order_msg);
    book.record_bid(1, "a1".to_string(), 0.9);
    book.record_bid(1, "a2".to_string(), 0.5);
    let winner = book.determine_winner(1);
    assert_eq!(winner, Some("a1".to_string()));
}

#[test]
fn test_auction_winner_highest_score() {
    let mut book = AuctionBook::new();
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 2, pickup: Point::new(0.0,0.0), delivery: Point::new(1.0,1.0), weight: 1.0, escrow_amount: 100,
    };
    book.open_auction(&order_msg);
    book.record_bid(2, "low".to_string(),  0.1);
    book.record_bid(2, "high".to_string(), 0.99);
    book.record_bid(2, "mid".to_string(),  0.5);
    assert_eq!(book.determine_winner(2), Some("high".to_string()));
}

#[test]
fn test_auction_no_bids_returns_none() {
    let mut book = AuctionBook::new();
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 99, pickup: Point::new(0.0,0.0), delivery: Point::new(1.0,1.0), weight: 1.0, escrow_amount: 0,
    };
    book.open_auction(&order_msg);
    assert_eq!(book.determine_winner(99), None);
}

#[test]
fn test_agent_bids_on_order() {
    let mut agents = vec![make_agent("drone-001", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 1, pickup: Point::new(1.0, 1.0), delivery: Point::new(5.0, 5.0), weight: 2.0, escrow_amount: 100,
    };
    let out = broadcast(&mut agents, order_msg);
    assert!(out.iter().any(|m| matches!(m, SwarmMessage::AuctionBid { .. })));
}

#[test]
fn test_agent_status_becomes_busy_when_wins() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 5, pickup: Point::new(1.0,1.0), delivery: Point::new(2.0,2.0), weight: 1.0, escrow_amount: 50,
    };
    broadcast(&mut agents, order_msg);
    broadcast(&mut agents, SwarmMessage::AuctionWinner { order_id: 5, winner_id: "a1".to_string() });
    assert_eq!(agents[0].state.status, AgentStatus::Busy);
}

// â”€â”€ Protocol 3: Multi-Hop Handoff â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_handoff_manager_initiates() {
    let mut mgr = HandoffManager::new();
    let id = mgr.initiate(1, "a".to_string(), "b".to_string(), Point::new(1.0, 1.0));
    assert_eq!(id, 0);
    assert!(mgr.handoffs.contains_key(&0));
}

#[test]
fn test_handoff_complete_emitted() {
    let mut agents = vec![
        make_agent("drone-001", AgentType::Drone, 0.0, 0.0, 100.0, 100),
        make_agent("robot-002", AgentType::GroundRobot, 5.0, 5.0, 90.0, 50),
    ];
    // Setup winner first
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 1, pickup: Point::new(1.0,1.0), delivery: Point::new(5.0,5.0), weight: 1.0, escrow_amount: 100,
    };
    broadcast(&mut agents, order_msg);
    broadcast(&mut agents, SwarmMessage::AuctionWinner { order_id: 1, winner_id: "drone-001".to_string() });

    let handoff = SwarmMessage::HandoffRequest {
        order_id: 1,
        from_agent: "drone-001".to_string(),
        to_agent: "robot-002".to_string(),
        point: Point::new(3.0, 3.0),
    };
    let out = broadcast(&mut agents, handoff);
    assert!(out.iter().any(|m| matches!(m, SwarmMessage::HandoffComplete { .. })));
}

#[test]
fn test_multihop_drone_to_robot_to_ebike() {
    let mut agents = vec![
        make_agent("drone", AgentType::Drone, 0.0, 0.0, 100.0, 100),
        make_agent("robot", AgentType::GroundRobot, 5.0, 5.0, 90.0, 50),
        make_agent("ebike", AgentType::Ebike, 10.0, 10.0, 80.0, 30),
    ];
    // Handoff 1: drone â†’ robot
    let h1 = SwarmMessage::HandoffRequest {
        order_id: 10, from_agent: "drone".to_string(), to_agent: "robot".to_string(), point: Point::new(5.0, 5.0),
    };
    let out1 = broadcast(&mut agents, h1);
    assert!(out1.iter().any(|m| matches!(m, SwarmMessage::HandoffComplete { .. })));

    // Handoff 2: robot â†’ ebike
    let h2 = SwarmMessage::HandoffRequest {
        order_id: 10, from_agent: "robot".to_string(), to_agent: "ebike".to_string(), point: Point::new(8.0, 8.0),
    };
    let out2 = broadcast(&mut agents, h2);
    assert!(out2.iter().any(|m| matches!(m, SwarmMessage::HandoffComplete { .. })));
}

// â”€â”€ Protocol 4: Self-Healing â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_failure_detection_triggers_re_auction() {
    let mut detector = FailureDetector::new();
    let mut holders: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    holders.insert(1, "failed-agent".to_string());
    let (orders, _escrows) = detector.handle_failure("failed-agent", &holders);
    assert!(orders.contains(&1));
}

#[test]
fn test_agent_recovery_clears_failed_state() {
    let mut detector = FailureDetector::new();
    let holders = std::collections::HashMap::new();
    detector.handle_failure("a1", &holders);
    assert!(detector.is_failed("a1"));
    detector.handle_recovery("a1");
    assert!(!detector.is_failed("a1"));
}

#[test]
fn test_failure_message_handled() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    let fail_msg = SwarmMessage::AgentFailure { agent_id: "a2".to_string(), reason: "battery".to_string() };
    broadcast(&mut agents, fail_msg); // Should not panic
}

#[test]
fn test_recovery_message_handled() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    broadcast(&mut agents, SwarmMessage::AgentFailure { agent_id: "a1".to_string(), reason: "crash".to_string() });
    broadcast(&mut agents, SwarmMessage::AgentRecovery { agent_id: "a1".to_string(), battery: 85.0 });
    assert_eq!(agents[0].state.status, AgentStatus::Idle);
    assert_eq!(agents[0].state.battery, 85.0);
}

// â”€â”€ Protocol 5: Safety Mesh â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_safety_alert_freezes_agent_inside_radius() {
    let mut mesh = SafetyMesh::new();
    mesh.activate(1, Point::new(0.0, 0.0), 10.0);
    assert!(mesh.is_frozen(&Point::new(5.0, 0.0)));
    assert!(!mesh.is_frozen(&Point::new(20.0, 20.0)));
}

#[test]
fn test_safety_clear_unfreezes() {
    let mut mesh = SafetyMesh::new();
    mesh.activate(1, Point::new(0.0, 0.0), 10.0);
    mesh.clear(1);
    assert!(!mesh.is_frozen(&Point::new(5.0, 0.0)));
}

#[test]
fn test_safety_alert_prevents_bidding() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 1.0, 1.0, 100.0, 100)];
    // Activate safety zone covering agent
    broadcast(&mut agents, SwarmMessage::SafetyAlert { alert_id: 1, x: 0.0, y: 0.0, radius: 50.0 });
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 1, pickup: Point::new(2.0,2.0), delivery: Point::new(5.0,5.0), weight: 1.0, escrow_amount: 100,
    };
    let bids = broadcast(&mut agents, order_msg);
    // Agent inside zone should NOT bid
    assert!(!bids.iter().any(|m| matches!(m, SwarmMessage::AuctionBid { .. })));
}

#[test]
fn test_safety_clear_allows_bidding_again() {
    let mut agents = vec![make_agent("a1", AgentType::Drone, 1.0, 1.0, 100.0, 100)];
    broadcast(&mut agents, SwarmMessage::SafetyAlert { alert_id: 1, x: 0.0, y: 0.0, radius: 50.0 });
    broadcast(&mut agents, SwarmMessage::SafetyClear { alert_id: 1 });
    let order_msg = SwarmMessage::OrderCreated {
        order_id: 2, pickup: Point::new(2.0,2.0), delivery: Point::new(5.0,5.0), weight: 1.0, escrow_amount: 100,
    };
    let bids = broadcast(&mut agents, order_msg);
    assert!(bids.iter().any(|m| matches!(m, SwarmMessage::AuctionBid { .. })));
}

#[test]
fn test_multiple_safety_zones() {
    let mut mesh = SafetyMesh::new();
    mesh.activate(1, Point::new(0.0, 0.0), 5.0);
    mesh.activate(2, Point::new(20.0, 20.0), 5.0);
    assert!(mesh.is_frozen(&Point::new(3.0, 0.0)));
    assert!(mesh.is_frozen(&Point::new(22.0, 20.0)));
    assert!(!mesh.is_frozen(&Point::new(10.0, 10.0)));
}

// â”€â”€ Distance / geometry â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_point_distance() {
    let a = Point::new(0.0, 0.0);
    let b = Point::new(3.0, 4.0);
    assert!((a.distance_to(&b) - 5.0).abs() < 1e-9);
}

// â”€â”€ Reputation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_reputation_zero_assignments() {
    let agent = NexusAgent::new("a", AgentType::Drone, "v", 0.0, 0.0, 100.0, 5.0, 0);
    assert_eq!(agent.reputation(), 1.0);
}

#[test]
fn test_reputation_increases_with_deliveries() {
    let mut agent = NexusAgent::new("a", AgentType::Drone, "v", 0.0, 0.0, 100.0, 5.0, 0);
    agent.total_assignments = 10;
    agent.successful_deliveries = 8;
    assert!((agent.reputation() - 0.8).abs() < 1e-9);
}

#[test]
fn test_bid_score_higher_with_better_reputation() {
    let pickup = Point::new(1.0, 1.0);
    let mut a_new = NexusAgent::new("new", AgentType::Drone, "v", 0.0, 0.0, 100.0, 10.0, 0);
    let mut a_exp = NexusAgent::new("exp", AgentType::Drone, "v", 0.0, 0.0, 100.0, 10.0, 0);
    a_exp.total_assignments = 10;
    a_exp.successful_deliveries = 10;
    let score_new = calculate_bid_score(&a_new, &pickup, 1.0);
    let score_exp = calculate_bid_score(&a_exp, &pickup, 1.0);
    assert!(score_exp >= score_new);
}

// â”€â”€ Message serialisation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_message_round_trip_serialisation() {
    let msg = SwarmMessage::AuctionBid { order_id: 42, agent_id: "x".to_string(), score: 0.75 };
    let bytes = msg.to_bytes().unwrap();
    let decoded = SwarmMessage::from_bytes(&bytes).unwrap();
    assert!(matches!(decoded, SwarmMessage::AuctionBid { order_id: 42, .. }));
}

#[test]
fn test_all_message_types_serialise() {
    let msgs: Vec<SwarmMessage> = vec![
        SwarmMessage::SafetyAlert { alert_id: 1, x: 0.0, y: 0.0, radius: 10.0 },
        SwarmMessage::SafetyClear { alert_id: 1 },
        SwarmMessage::AgentFailure { agent_id: "a".to_string(), reason: "r".to_string() },
        SwarmMessage::AgentRecovery { agent_id: "a".to_string(), battery: 80.0 },
        SwarmMessage::HandoffRequest { order_id: 1, from_agent: "a".to_string(), to_agent: "b".to_string(), point: Point::new(1.0, 1.0) },
        SwarmMessage::HandoffComplete { order_id: 1, from_agent: "a".to_string(), to_agent: "b".to_string() },
        SwarmMessage::OrderDelivered { order_id: 1, agent_id: "a".to_string() },
    ];
    for msg in msgs {
        let bytes = msg.to_bytes().unwrap();
        assert!(!bytes.is_empty());
    }
}

// â”€â”€ Additional edge cases â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn test_agent_ignores_handoff_not_for_it() {
    let mut agents = vec![make_agent("x", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    let msg = SwarmMessage::HandoffRequest {
        order_id: 1, from_agent: "a".to_string(), to_agent: "b".to_string(), point: Point::new(0.0, 0.0),
    };
    let out = broadcast(&mut agents, msg);
    // "x" is neither from nor to agent â€“ should produce no output
    assert!(out.is_empty());
}

#[test]
fn test_multiple_orders_independent() {
    let mut agents = vec![make_agent("drone", AgentType::Drone, 0.0, 0.0, 100.0, 100)];
    for order_id in [1u64, 2, 3] {
        let out = broadcast(&mut agents, SwarmMessage::OrderCreated {
            order_id, pickup: Point::new(1.0,1.0), delivery: Point::new(2.0,2.0), weight: 1.0, escrow_amount: 50,
        });
        // Agent bids on each independently (while Idle â€“ but it becomes Busy after winning)
    }
}

#[test]
fn test_failure_escrow_recovery() {
    let mut detector = FailureDetector::new();
    detector.track_escrow("agent-fail", 99);
    let holders = { let mut m = std::collections::HashMap::new(); m.insert(1u64, "agent-fail".to_string()); m };
    let (_, escrows) = detector.handle_failure("agent-fail", &holders);
    assert!(escrows.contains(&99));
}
