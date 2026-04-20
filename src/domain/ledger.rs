#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::types::NodeId;

// ---------------------------------------------------------------------------
// Settlement states for one order
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum EscrowState {
    /// Funds are reserved but not yet disbursed.
    Reserved,
    /// A mid-delivery handoff transfer has been applied.
    Transferred,
    /// Final payment has been released to the delivering agent.
    Released,
    /// The order was cancelled and funds refunded to the originator.
    Refunded,
}

#[derive(Debug, Clone)]
struct EscrowEntry {
    originator: NodeId,
    current_holder: NodeId,
    amount: u64,
    state: EscrowState,
}

// ---------------------------------------------------------------------------
// Ledger
// ---------------------------------------------------------------------------

/// Minimal in-memory settlement ledger.
///
/// Tracks escrow reservations, mid-delivery handoff transfers, final
/// payments, and refunds per order. No blockchain or signature logic.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Spendable balances per agent.
    pub balances: HashMap<NodeId, u64>,
    /// Escrow entries per order_id.
    escrows: HashMap<String, EscrowEntry>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Credit an agent's balance (e.g. during test setup or top-up).
    pub fn credit(&mut self, agent: impl Into<NodeId>, amount: u64) {
        *self.balances.entry(agent.into()).or_default() += amount;
    }

    /// Reserve `amount` from `originator`'s balance into escrow for `order_id`.
    ///
    /// Fails if the order already has an escrow entry or the originator has
    /// insufficient funds.
    pub fn reserve_escrow(
        &mut self,
        order_id: impl Into<String>,
        originator: impl Into<NodeId>,
        amount: u64,
    ) -> Result<()> {
        let order_id = order_id.into();
        let originator = originator.into();

        if self.escrows.contains_key(&order_id) {
            bail!("Order '{}': escrow already exists", order_id);
        }

        let balance = self.balances.entry(originator.clone()).or_default();
        if *balance < amount {
            bail!(
                "Order '{}': insufficient funds (have {}, need {})",
                order_id,
                balance,
                amount
            );
        }
        *balance -= amount;

        self.escrows.insert(
            order_id,
            EscrowEntry {
                originator,
                current_holder: String::new(),
                amount,
                state: EscrowState::Reserved,
            },
        );
        Ok(())
    }

    /// Transfer a partial fee to the outgoing agent on handoff.
    ///
    /// `fee` is paid to `from_agent`; the remainder stays in escrow for the
    /// incoming agent. Advances escrow state to `Transferred`.
    ///
    /// Fails if the escrow is not in `Reserved` or `Transferred` state, or if
    /// `fee` exceeds the remaining escrow amount.
    pub fn transfer_for_handoff(
        &mut self,
        order_id: &str,
        from_agent: impl Into<NodeId>,
        to_agent: impl Into<NodeId>,
        fee: u64,
    ) -> Result<()> {
        let from_agent = from_agent.into();
        let to_agent = to_agent.into();

        let entry = self
            .escrows
            .get_mut(order_id)
            .ok_or_else(|| anyhow::anyhow!("Order '{}': no escrow found", order_id))?;

        if entry.state != EscrowState::Reserved && entry.state != EscrowState::Transferred {
            bail!(
                "Order '{}': cannot transfer from state {:?}",
                order_id,
                entry.state
            );
        }
        if fee > entry.amount {
            bail!(
                "Order '{}': handoff fee {} exceeds escrow {}",
                order_id,
                fee,
                entry.amount
            );
        }

        entry.amount -= fee;
        entry.current_holder = to_agent;
        entry.state = EscrowState::Transferred;
        *self.balances.entry(from_agent).or_default() += fee;
        Ok(())
    }

    /// Release remaining escrow to `delivering_agent` as final payment.
    ///
    /// Fails if the escrow is already `Released` or `Refunded`.
    pub fn release_final_payment(
        &mut self,
        order_id: &str,
        delivering_agent: impl Into<NodeId>,
    ) -> Result<()> {
        let delivering_agent = delivering_agent.into();
        let entry = self
            .escrows
            .get_mut(order_id)
            .ok_or_else(|| anyhow::anyhow!("Order '{}': no escrow found", order_id))?;

        match entry.state {
            EscrowState::Released => bail!("Order '{}': already released", order_id),
            EscrowState::Refunded => bail!("Order '{}': already refunded", order_id),
            _ => {}
        }

        let amount = entry.amount;
        entry.amount = 0;
        entry.state = EscrowState::Released;
        *self.balances.entry(delivering_agent).or_default() += amount;
        Ok(())
    }

    /// Refund the full escrow to the originator (e.g. cancelled order).
    ///
    /// Fails if already `Released` or `Refunded`.
    pub fn refund_previous_agent(&mut self, order_id: &str) -> Result<()> {
        let entry = self
            .escrows
            .get_mut(order_id)
            .ok_or_else(|| anyhow::anyhow!("Order '{}': no escrow found", order_id))?;

        match entry.state {
            EscrowState::Released => bail!("Order '{}': already released, cannot refund", order_id),
            EscrowState::Refunded => bail!("Order '{}': already refunded", order_id),
            _ => {}
        }

        let amount = entry.amount;
        let originator = entry.originator.clone();
        entry.amount = 0;
        entry.state = EscrowState::Refunded;
        *self.balances.entry(originator).or_default() += amount;
        Ok(())
    }

    /// Current escrow amount remaining for an order (0 if not found).
    pub fn escrow_balance(&self, order_id: &str) -> u64 {
        self.escrows.get(order_id).map(|e| e.amount).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn funded_ledger() -> Ledger {
        let mut l = Ledger::new();
        l.credit("customer", 1000);
        l
    }

    #[test]
    fn reserve_deducts_balance_and_creates_escrow() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 200).unwrap();
        assert_eq!(l.balances["customer"], 800);
        assert_eq!(l.escrow_balance("ord-1"), 200);
    }

    #[test]
    fn reserve_fails_on_insufficient_funds() {
        let mut l = funded_ledger();
        assert!(l.reserve_escrow("ord-1", "customer", 9999).is_err());
    }

    #[test]
    fn reserve_fails_on_duplicate_escrow() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 100).unwrap();
        assert!(l.reserve_escrow("ord-1", "customer", 100).is_err());
    }

    #[test]
    fn handoff_transfer_pays_outgoing_agent() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 300).unwrap();
        l.transfer_for_handoff("ord-1", "agent-a", "agent-b", 100).unwrap();
        assert_eq!(l.balances.get("agent-a").copied().unwrap_or(0), 100);
        assert_eq!(l.escrow_balance("ord-1"), 200);
    }

    #[test]
    fn handoff_transfer_fails_if_fee_exceeds_escrow() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 50).unwrap();
        assert!(l.transfer_for_handoff("ord-1", "agent-a", "agent-b", 100).is_err());
    }

    #[test]
    fn final_release_pays_delivering_agent() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 300).unwrap();
        l.release_final_payment("ord-1", "agent-a").unwrap();
        assert_eq!(l.balances["agent-a"], 300);
        assert_eq!(l.escrow_balance("ord-1"), 0);
    }

    #[test]
    fn double_release_is_rejected() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 100).unwrap();
        l.release_final_payment("ord-1", "agent-a").unwrap();
        assert!(l.release_final_payment("ord-1", "agent-a").is_err());
    }

    #[test]
    fn refund_returns_escrow_to_originator() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 400).unwrap();
        assert_eq!(l.balances["customer"], 600);
        l.refund_previous_agent("ord-1").unwrap();
        assert_eq!(l.balances["customer"], 1000);
        assert_eq!(l.escrow_balance("ord-1"), 0);
    }

    #[test]
    fn refund_after_release_is_rejected() {
        let mut l = funded_ledger();
        l.reserve_escrow("ord-1", "customer", 100).unwrap();
        l.release_final_payment("ord-1", "agent-a").unwrap();
        assert!(l.refund_previous_agent("ord-1").is_err());
    }
}
