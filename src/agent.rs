use std::collections::HashMap;
use crate::types::{
    AgentId, AgentStatus, EscrowId, HandoffId, NexusAgent,
    OrderId, Point, SwarmMessage,
};
use crate::auction::{AuctionBook, calculate_bid_score};
use crate::crypto::{client_escrow_signature, derive_key, proof_hash, sign_message, validator_signature, verify_signature};
use crate::handoff::HandoffManager;
use crate::healing::FailureDetector;
use crate::payments::EscrowLedger;
use crate::safety::SafetyMesh;

/// Full NexusFly agent runtime.
pub struct AgentRuntime {
    pub state:         NexusAgent,
    pub auction_book:  AuctionBook,
    pub handoffs:      HandoffManager,
    pub detector:      FailureDetector,
    pub ledger:        EscrowLedger,
    pub safety:        SafetyMesh,
    pub order_holders: HashMap<OrderId, AgentId>, // order_id → current holder
    pub escrow_amounts: HashMap<OrderId, u64>,    // order_id → escrow total
    pub escrow_ids:    HashMap<OrderId, EscrowId>,
    next_escrow_id:    EscrowId,
    next_handoff_id:   HandoffId,
    /// Messages queued to be sent via Vertex
    pub outbox:        Vec<SwarmMessage>,
}

impl AgentRuntime {
    pub fn new(agent: NexusAgent) -> Self {
        let mut ledger = EscrowLedger::new();
        ledger.set_balance(&agent.id, agent.balance);
        Self {
            state:          agent,
            auction_book:   AuctionBook::new(),
            handoffs:       HandoffManager::new(),
            detector:       FailureDetector::new(),
            ledger,
            safety:         SafetyMesh::new(),
            order_holders:  HashMap::new(),
            escrow_amounts: HashMap::new(),
            escrow_ids:     HashMap::new(),
            next_escrow_id: 1,
            next_handoff_id: 1,
            outbox:         Vec::new(),
        }
    }

    // ── Message dispatcher ────────────────────────────────────────────────

    pub fn handle(&mut self, msg: SwarmMessage) {
        match msg {
            SwarmMessage::OrderCreated { order_id, pickup, delivery, weight, escrow_amount } =>
                self.on_order_created(order_id, pickup, delivery, weight, escrow_amount),

            SwarmMessage::AuctionBid { order_id, agent_id, score } =>
                self.on_auction_bid(order_id, agent_id, score),

            SwarmMessage::AuctionWinner { order_id, winner_id } =>
                self.on_auction_winner(order_id, winner_id),

            SwarmMessage::HandoffRequest { order_id, from_agent, to_agent, point } =>
                self.on_handoff_request(order_id, from_agent, to_agent, point),

            SwarmMessage::HandoffComplete { order_id, from_agent, to_agent } =>
                self.on_handoff_complete(order_id, from_agent, to_agent),

            SwarmMessage::OrderDelivered { order_id, agent_id } =>
                self.on_order_delivered(order_id, agent_id),

            SwarmMessage::AgentFailure { agent_id, reason } =>
                self.on_agent_failure(agent_id, reason),

            SwarmMessage::AgentRecovery { agent_id, battery } =>
                self.on_agent_recovery(agent_id, battery),

            SwarmMessage::SafetyAlert { alert_id, x, y, radius } =>
                self.safety.activate(alert_id, Point::new(x, y), radius),

            SwarmMessage::SafetyClear { alert_id } =>
                self.safety.clear(alert_id),

            SwarmMessage::PaymentEscrow { order_id, from_agent, amount, client_signature, escrow_id } =>
                self.on_payment_escrow(order_id, from_agent, amount, client_signature, escrow_id),

            SwarmMessage::PaymentClaim { order_id, agent_id, proof_hash: ph, timestamp } =>
                self.on_payment_claim(order_id, agent_id, ph, timestamp),

            SwarmMessage::PaymentRelease { escrow_id, to_agent, amount, validator_signatures } =>
                self.on_payment_release(escrow_id, to_agent, amount, validator_signatures),

            SwarmMessage::HandoffPayment { handoff_id, from_agent, to_agent, amount, signature } =>
                self.on_handoff_payment(handoff_id, from_agent, to_agent, amount, signature),

            SwarmMessage::BalanceQuery { agent_id, request_id } =>
                self.on_balance_query(agent_id, request_id),

            SwarmMessage::BalanceResponse { .. } | SwarmMessage::AgentState { .. } => {}
        }
    }

    // ── Protocol handlers ─────────────────────────────────────────────────

    fn on_order_created(&mut self, order_id: OrderId, pickup: Point, delivery: Point, weight: f64, escrow_amount: u64) {
        if self.safety.is_frozen(&self.state.position) {
            tracing::warn!("[{}] Frozen by safety mesh – skipping auction for order {order_id}", self.state.id);
            return;
        }
        self.auction_book.open_auction(&SwarmMessage::OrderCreated {
            order_id, pickup, delivery, weight, escrow_amount,
        });
        self.escrow_amounts.insert(order_id, escrow_amount);
        if self.state.status == AgentStatus::Idle && self.state.capacity >= weight {
            let score = calculate_bid_score(&self.state, &pickup, weight);
            if score > 0.0 {
                tracing::info!("[{}] Bidding on order {order_id} with score {score:.4}", self.state.id);
                self.outbox.push(SwarmMessage::AuctionBid {
                    order_id,
                    agent_id: self.state.id.clone(),
                    score,
                });
            }
        }
    }

    fn on_auction_bid(&mut self, order_id: OrderId, agent_id: AgentId, score: f64) {
        self.auction_book.record_bid(order_id, agent_id, score);
    }

    fn on_auction_winner(&mut self, order_id: OrderId, winner_id: AgentId) {
        self.order_holders.insert(order_id, winner_id.clone());
        if winner_id == self.state.id {
            tracing::info!("[{}] Won auction for order {order_id}!", self.state.id);
            self.state.status = AgentStatus::Busy;
            self.state.total_assignments += 1;
            // Lock escrow
            let amount = *self.escrow_amounts.get(&order_id).unwrap_or(&0);
            let escrow_id = self.next_escrow_id;
            self.next_escrow_id += 1;
            let sig = client_escrow_signature(order_id, amount);
            self.escrow_ids.insert(order_id, escrow_id);
            self.detector.track_escrow(&self.state.id, escrow_id);
            let _ = self.ledger.lock_escrow(escrow_id, order_id, amount, &self.state.id, &sig);
            self.outbox.push(SwarmMessage::PaymentEscrow {
                order_id,
                from_agent: self.state.id.clone(),
                amount,
                client_signature: sig,
                escrow_id,
            });
        }
    }

    fn on_handoff_request(&mut self, order_id: OrderId, from_agent: AgentId, to_agent: AgentId, point: Point) {
        // The receiving agent processes the request
        if to_agent == self.state.id {
            tracing::info!("[{}] HandoffRequest from {from_agent} for order {order_id}", self.state.id);
            let handoff_id = self.next_handoff_id;
            self.next_handoff_id += 1;
            self.handoffs.initiate(order_id, from_agent.clone(), to_agent.clone(), point);

            // Emit HandoffComplete (robot accepts)
            self.order_holders.insert(order_id, self.state.id.clone());
            self.outbox.push(SwarmMessage::HandoffComplete {
                order_id,
                from_agent: from_agent.clone(),
                to_agent: to_agent.clone(),
            });

            // Sign HandoffPayment: from_agent pays 30% to to_agent
            let escrow_total = *self.escrow_amounts.get(&order_id).unwrap_or(&0);
            let pay_amount = (escrow_total as f64 * 0.30) as u64;
            let key = derive_key(&from_agent);
            let msg_bytes: Vec<u8> = [
                handoff_id.to_le_bytes().as_ref(),
                pay_amount.to_le_bytes().as_ref(),
            ].concat();
            let signature = sign_message(&key, &msg_bytes);
            self.outbox.push(SwarmMessage::HandoffPayment {
                handoff_id,
                from_agent: from_agent.clone(),
                to_agent: self.state.id.clone(),
                amount: pay_amount,
                signature,
            });
        }
    }

    fn on_handoff_complete(&mut self, order_id: OrderId, _from_agent: AgentId, to_agent: AgentId) {
        tracing::info!("[{}] HandoffComplete for order {order_id} → {to_agent}", self.state.id);
        self.order_holders.insert(order_id, to_agent);
    }

    fn on_order_delivered(&mut self, order_id: OrderId, agent_id: AgentId) {
        if agent_id == self.state.id {
            tracing::info!("[{}] Delivered order {order_id}", self.state.id);
            self.state.successful_deliveries += 1;
            self.state.status = AgentStatus::Idle;

            // Build proof hash
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let ph = proof_hash(self.state.position.x, self.state.position.y, ts, order_id);
            tracing::info!("[{}] Proof hash: 0x{}", self.state.id, hex_prefix(&ph));

            self.outbox.push(SwarmMessage::PaymentClaim {
                order_id,
                agent_id: self.state.id.clone(),
                proof_hash: ph,
                timestamp: ts,
            });
        }
    }

    fn on_agent_failure(&mut self, agent_id: AgentId, reason: String) {
        tracing::warn!("[{}] Agent {agent_id} failed: {reason}", self.state.id);
        let (orders, escrows) = self.detector.handle_failure(&agent_id, &self.order_holders);
        // Return escrows
        for eid in escrows {
            let _ = self.ledger.return_escrow(eid);
        }
        // Re-auction affected orders
        for order_id in orders {
            if let Some(amount) = self.escrow_amounts.get(&order_id) {
                let amount = *amount;
                self.outbox.push(SwarmMessage::OrderCreated {
                    order_id,
                    pickup: Point::new(0.0, 0.0), // simplified: coordinates unknown
                    delivery: Point::new(0.0, 0.0),
                    weight: 1.0,
                    escrow_amount: amount,
                });
            }
        }
    }

    fn on_agent_recovery(&mut self, agent_id: AgentId, battery: f64) {
        self.detector.handle_recovery(&agent_id);
        if agent_id == self.state.id {
            self.state.battery = battery;
            self.state.status = AgentStatus::Idle;
        }
    }

    fn on_payment_escrow(&mut self, order_id: OrderId, from_agent: AgentId, amount: u64, client_signature: Vec<u8>, escrow_id: EscrowId) {
        tracing::info!("[{}] PaymentEscrow received: {amount} tokens locked for order {order_id}", self.state.id);
        let _ = self.ledger.lock_escrow(escrow_id, order_id, amount, &from_agent, &client_signature);
        self.escrow_ids.insert(order_id, escrow_id);
    }

    fn on_payment_claim(&mut self, order_id: OrderId, agent_id: AgentId, ph: Vec<u8>, _timestamp: u64) {
        // Any node can act as validator in this demo
        tracing::info!("[validator] Verifying proof for order {order_id} from {agent_id}: 0x{}... accepted", hex_prefix(&ph));
        // Emit PaymentRelease with quorum signature (simplified: 1 validator)
        if let Some(&escrow_id) = self.escrow_ids.get(&order_id) {
            let remaining = *self.escrow_amounts.get(&order_id).unwrap_or(&0);
            let pay_remaining = (remaining as f64 * 0.70) as u64; // 70% remaining after handoff
            let vkey = derive_key("validator-1");
            let vsig = validator_signature(&vkey, escrow_id, &agent_id, pay_remaining);
            self.outbox.push(SwarmMessage::PaymentRelease {
                escrow_id,
                to_agent: agent_id.clone(),
                amount: pay_remaining,
                validator_signatures: vec![vsig],
            });
        }
    }

    fn on_payment_release(&mut self, escrow_id: EscrowId, to_agent: AgentId, amount: u64, validator_signatures: Vec<Vec<u8>>) {
        if to_agent == self.state.id {
            let _ = self.ledger.release_escrow(escrow_id, &to_agent, amount, &validator_signatures);
            self.state.balance = self.ledger.balance(&self.state.id);
            tracing::info!("[{}] PaymentRelease: +{amount} tokens, balance: {}", self.state.id, self.state.balance);
        }
    }

    fn on_handoff_payment(&mut self, handoff_id: HandoffId, from_agent: AgentId, to_agent: AgentId, amount: u64, signature: Vec<u8>) {
        if to_agent == self.state.id {
            // Verify signature
            let key = derive_key(&from_agent);
            let msg_bytes: Vec<u8> = [
                handoff_id.to_le_bytes().as_ref(),
                amount.to_le_bytes().as_ref(),
            ].concat();
            if verify_signature(&key, &msg_bytes, &signature) {
                let _ = self.ledger.transfer(&from_agent, &to_agent, amount);
                self.state.balance = self.ledger.balance(&self.state.id);
                tracing::info!(
                    "[{}] HandoffPayment received +{amount} tokens, balance: {}",
                    self.state.id, self.state.balance
                );
            } else {
                tracing::warn!("[{}] Invalid HandoffPayment signature from {from_agent}", self.state.id);
            }
        }
    }

    fn on_balance_query(&mut self, agent_id: AgentId, request_id: u64) {
        if agent_id == self.state.id {
            let balance = self.ledger.balance(&self.state.id);
            self.outbox.push(SwarmMessage::BalanceResponse {
                agent_id: self.state.id.clone(),
                balance,
                request_id,
            });
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    /// Broadcast current agent state.
    pub fn broadcast_state(&self) -> SwarmMessage {
        SwarmMessage::AgentState {
            agent_id:   self.state.id.clone(),
            agent_type: self.state.agent_type.clone(),
            vendor:     self.state.vendor.clone(),
            x:          self.state.position.x,
            y:          self.state.position.y,
            battery:    self.state.battery,
            capacity:   self.state.capacity,
            status:     self.state.status.clone(),
        }
    }

    /// Drain queued outbound messages.
    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        std::mem::take(&mut self.outbox)
    }

    /// Tick: recover timed-out escrows.
    pub fn tick(&mut self) {
        let _recovered = self.ledger.recover_timed_out();
    }
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes.iter().take(4).map(|b| format!("{b:02x}")).collect()
}
