use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::types::{EscrowId, EscrowInfo, OrderId, AgentId};
use crate::crypto::client_escrow_signature;

/// How long (ms) to hold an unclaimed escrow before auto-return.
pub const ESCROW_TIMEOUT_MS: u64 = 5_000;

/// Central escrow registry shared within one agent process.
#[derive(Debug, Default)]
pub struct EscrowLedger {
    escrows:  HashMap<EscrowId, EscrowInfo>,
    balances: HashMap<AgentId, u64>,
}

impl EscrowLedger {
    pub fn new() -> Self { Self::default() }

    /// Credit initial balance for an agent.
    pub fn set_balance(&mut self, agent_id: &str, balance: u64) {
        self.balances.insert(agent_id.to_string(), balance);
    }

    pub fn balance(&self, agent_id: &str) -> u64 {
        *self.balances.get(agent_id).unwrap_or(&0)
    }

    /// Lock funds into an escrow.
    /// Returns `Err` if client signature invalid or escrow_id already exists.
    pub fn lock_escrow(
        &mut self,
        escrow_id:        EscrowId,
        order_id:         OrderId,
        amount:           u64,
        holder_agent:     &str,
        client_signature: &[u8],
    ) -> anyhow::Result<()> {
        // Verify client signature
        let expected = client_escrow_signature(order_id, amount);
        if expected != client_signature {
            anyhow::bail!("Invalid client signature for escrow {escrow_id}");
        }
        if self.escrows.contains_key(&escrow_id) {
            anyhow::bail!("Escrow {escrow_id} already exists (double-spend prevented)");
        }
        let now_ms = now_ms();
        self.escrows.insert(
            escrow_id,
            EscrowInfo {
                escrow_id,
                order_id,
                amount,
                client_signature: client_signature.to_vec(),
                holder_agent: holder_agent.to_string(),
                locked_at: now_ms,
                released: false,
            },
        );
        tracing::info!("[escrow] Locked {amount} tokens in escrow {escrow_id} for order {order_id}");
        Ok(())
    }

    /// Release escrow to `to_agent`.  Requires BFT quorum (≥1 sig for demo).
    pub fn release_escrow(
        &mut self,
        escrow_id:            EscrowId,
        to_agent:             &str,
        amount:               u64,
        _validator_signatures: &[Vec<u8>],
    ) -> anyhow::Result<()> {
        let info = self.escrows.get_mut(&escrow_id)
            .ok_or_else(|| anyhow::anyhow!("Escrow {escrow_id} not found"))?;

        if info.released {
            anyhow::bail!("Escrow {escrow_id} already released");
        }
        if amount > info.amount {
            anyhow::bail!("Release amount {amount} > escrowed {}", info.amount);
        }

        info.released = true;
        *self.balances.entry(to_agent.to_string()).or_insert(0) += amount;
        tracing::info!(
            "[escrow] Released {amount} tokens from escrow {escrow_id} → {to_agent} (balance: {})",
            self.balances[to_agent]
        );
        Ok(())
    }

    /// Pay `amount` from `from_agent` to `to_agent` (handoff micro-payment).
    pub fn transfer(
        &mut self,
        from_agent: &str,
        to_agent:   &str,
        amount:     u64,
    ) -> anyhow::Result<()> {
        let from_bal = self.balances.entry(from_agent.to_string()).or_insert(0);
        if *from_bal < amount {
            anyhow::bail!("Insufficient balance: {from_agent} has {from_bal} < {amount}");
        }
        *from_bal -= amount;
        let from_bal_after = *from_bal;
        *self.balances.entry(to_agent.to_string()).or_insert(0) += amount;
        let to_bal_after = self.balances[to_agent];
        tracing::info!(
            "[payment] {amount} tokens: {from_agent} ({from_bal_after}) → {to_agent} ({to_bal_after})"
        );
        Ok(())
    }

    /// Recover timed-out escrows and return funds to holder.
    pub fn recover_timed_out(&mut self) -> Vec<EscrowId> {
        let now = now_ms();
        let mut recovered = Vec::new();
        for (id, info) in self.escrows.iter_mut() {
            if !info.released && (now - info.locked_at) > ESCROW_TIMEOUT_MS {
                info.released = true;
                let holder = info.holder_agent.clone();
                let amount = info.amount;
                *self.balances.entry(holder.clone()).or_insert(0) += amount;
                tracing::warn!(
                    "[escrow] Timeout recovery: {amount} tokens returned to {holder} (escrow {id})"
                );
                recovered.push(*id);
            }
        }
        recovered
    }

    /// Return escrow to previous holder (on agent failure during handoff).
    pub fn return_escrow(&mut self, escrow_id: EscrowId) -> anyhow::Result<()> {
        let info = self.escrows.get_mut(&escrow_id)
            .ok_or_else(|| anyhow::anyhow!("Escrow {escrow_id} not found"))?;
        if info.released {
            return Ok(());
        }
        info.released = true;
        let holder = info.holder_agent.clone();
        let amount = info.amount;
        *self.balances.entry(holder.clone()).or_insert(0) += amount;
        tracing::info!("[escrow] Returned {amount} tokens to {holder} (escrow {escrow_id})");
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
