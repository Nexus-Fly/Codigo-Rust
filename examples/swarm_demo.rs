//! swarm_demo – simulates a 3-node NexusFly network in-process,
//! showing: order creation -> auction -> handoff + payment -> delivery + escrow release.

use codigo_rust::{
    agent::AgentRuntime,
    types::{AgentType, NexusAgent, OrderId, Point, SwarmMessage},
};

fn make_agent(id: &str, atype: AgentType, x: f64, y: f64, battery: f64, balance: u64) -> AgentRuntime {
    AgentRuntime::new(NexusAgent::new(id, atype, "NexusFly", x, y, battery, 10.0, balance))
}

fn broadcast(agents: &mut [AgentRuntime], msg: SwarmMessage) -> Vec<SwarmMessage> {
    let mut out = Vec::new();
    for a in agents.iter_mut() {
        a.handle(msg.clone());
        out.extend(a.drain_outbox());
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let mut agents = vec![
        make_agent("drone-001", AgentType::Drone,       0.0,  0.0,  100.0, 100),
        make_agent("robot-002", AgentType::GroundRobot, 5.0,  5.0,   90.0,  50),
        make_agent("ebike-003", AgentType::Ebike,      20.0, 20.0,   80.0,  30),
    ];

    for a in &agents {
        println!("[{}] Balance: {} tokens", a.state.id, a.ledger.balance(&a.state.id));
    }

    let order_id: OrderId = 123;
    let escrow_amount: u64 = 500;
    println!("\n[client] Order #{order_id} created with escrow: {escrow_amount} tokens");

    let order_msg = SwarmMessage::OrderCreated {
        order_id,
        pickup:        Point::new(1.0, 1.0),
        delivery:      Point::new(10.0, 10.0),
        weight:        2.0,
        escrow_amount,
    };
    let mut bids = broadcast(&mut agents, order_msg);
    bids.retain(|m| matches!(m, SwarmMessage::AuctionBid { .. }));

    let mut best_score = -1.0_f64;
    let mut winner_id  = String::new();
    for bid in &bids {
        if let SwarmMessage::AuctionBid { agent_id, score, .. } = bid {
            println!("[{agent_id}] Bid score: {score:.4}");
            broadcast(&mut agents, bid.clone());
            if *score > best_score {
                best_score = *score;
                winner_id  = agent_id.clone();
            }
        }
    }

    println!("\n[{winner_id}] Won auction with score {best_score:.4}");
    let winner_msg = SwarmMessage::AuctionWinner { order_id, winner_id: winner_id.clone() };
    let escrow_msgs = broadcast(&mut agents, winner_msg);

    for msg in &escrow_msgs {
        if let SwarmMessage::PaymentEscrow { from_agent, amount, .. } = msg {
            println!("[{from_agent}] Locked escrow: {amount} tokens for order #{order_id}");
            broadcast(&mut agents, msg.clone());
        }
    }

    println!("\n[drone-001] Handoff to robot-002 at (42.3, -71.1)");
    let handoff_req = SwarmMessage::HandoffRequest {
        order_id,
        from_agent: "drone-001".to_string(),
        to_agent:   "robot-002".to_string(),
        point:      Point::new(42.3, -71.1),
    };
    let handoff_out = broadcast(&mut agents, handoff_req);

    for msg in handoff_out {
        match &msg {
            SwarmMessage::HandoffPayment { from_agent, to_agent, amount, .. } => {
                println!("[{from_agent}] Signed HandoffPayment: {amount} tokens -> {to_agent}");
                broadcast(&mut agents, msg.clone());
                let robot_bal = agents.iter().find(|a| a.state.id == "robot-002")
                    .map(|a| a.ledger.balance("robot-002")).unwrap_or(0);
                println!("[robot-002] Received HandoffPayment, balance: {robot_bal} tokens");
            }
            SwarmMessage::HandoffComplete { to_agent, .. } => {
                println!("[{to_agent}] Handoff complete for order #{order_id}");
                broadcast(&mut agents, msg.clone());
            }
            _ => {}
        }
    }

    println!("\n[robot-002] Delivery confirmed");
    let delivered = SwarmMessage::OrderDelivered {
        order_id,
        agent_id: "robot-002".to_string(),
    };
    let claim_out = broadcast(&mut agents, delivered);

    for msg in claim_out {
        if let SwarmMessage::PaymentClaim { ref agent_id, ref proof_hash, .. } = msg {
            println!("[{agent_id}] PaymentClaim submitted, proof: 0x{}", &hex(proof_hash)[..8]);
            let release_out = broadcast(&mut agents, msg.clone());
            for rmsg in release_out {
                if let SwarmMessage::PaymentRelease { ref to_agent, amount, .. } = rmsg {
                    println!("[validator] Verifying proof... accepted");
                    broadcast(&mut agents, rmsg.clone());
                    let bal = agents.iter().find(|a| &a.state.id == to_agent)
                        .map(|a| a.ledger.balance(to_agent)).unwrap_or(0);
                    println!("[{to_agent}] PaymentRelease: +{amount} tokens received, balance: {bal}");
                }
            }
        }
    }

    println!("\n--- Final Balances ---");
    for a in &agents {
        println!("[{}] Balance: {} tokens", a.state.id, a.ledger.balance(&a.state.id));
    }
}
