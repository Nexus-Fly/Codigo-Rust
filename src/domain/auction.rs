use crate::types::{AuctionBid, NodeId};

// ETA ceiling: bids slower than this score 0 on distance.
const MAX_ETA_S: f64 = 600.0;

const W_DISTANCE: f64 = 1.0;
const W_BATTERY: f64 = 1.0;
const W_REPUTATION: f64 = 0.5;

/// Calculate a deterministic numeric score for a bid.
///
/// - `bid`        - the bid to score.
/// - `reputation` - optional normalised value in [0.0, 1.0].
///                  Pass `None` to use the neutral default (0.5).
pub fn calculate_bid_score(bid: &AuctionBid, reputation: Option<f64>) -> f64 {
    let distance_score = 1.0 - (bid.eta_s as f64 / MAX_ETA_S).clamp(0.0, 1.0);
    let battery_score = bid.battery_pct as f64 / 100.0;
    let rep_score = reputation.unwrap_or(0.5).clamp(0.0, 1.0);

    W_DISTANCE * distance_score + W_BATTERY * battery_score + W_REPUTATION * rep_score
}

/// Select the winning bid from a slice.
///
/// Rules (deterministic, order-independent):
/// 1. Highest score wins.
/// 2. Score tie -> lexicographically smaller `bidder` id wins.
///
/// Returns `None` if `bids` is empty.
pub fn choose_winner(bids: &[AuctionBid]) -> Option<AuctionBid> {
    bids.iter()
        .max_by(|a, b| {
            let sa = calculate_bid_score(a, None);
            let sb = calculate_bid_score(b, None);
            sa.total_cmp(&sb)
                .then_with(|| b.bidder.cmp(&a.bidder))
        })
        .cloned()
}

/// Convenience: return only the winner's `NodeId`.
pub fn choose_winner_id(bids: &[AuctionBid]) -> Option<NodeId> {
    choose_winner(bids).map(|b| b.bidder)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_700_000_000;

    fn bid(bidder: &str, eta_s: u32, battery_pct: u8) -> AuctionBid {
        AuctionBid {
            order_id: "order-1".into(),
            bidder: bidder.into(),
            eta_s,
            battery_pct,
            submitted_at: T,
        }
    }

    #[test]
    fn empty_bids_returns_none() {
        assert!(choose_winner(&[]).is_none());
    }

    #[test]
    fn higher_score_wins() {
        let bids = [bid("node-a", 60, 90), bid("node-b", 300, 30)];
        assert_eq!(choose_winner(&bids).unwrap().bidder, "node-a");
    }

    #[test]
    fn low_battery_lowers_score() {
        let high = calculate_bid_score(&bid("x", 60, 90), None);
        let low = calculate_bid_score(&bid("x", 60, 10), None);
        assert!(high > low);
    }

    #[test]
    fn high_eta_lowers_score() {
        let fast = calculate_bid_score(&bid("x", 30, 80), None);
        let slow = calculate_bid_score(&bid("x", 500, 80), None);
        assert!(fast > slow);
    }

    #[test]
    fn tie_break_lexicographically_smaller_wins() {
        let bids = [bid("node-b", 60, 80), bid("node-a", 60, 80)];
        assert_eq!(choose_winner(&bids).unwrap().bidder, "node-a");
    }

    #[test]
    fn single_bid_always_wins() {
        let bids = [bid("only-node", 120, 50)];
        assert_eq!(choose_winner(&bids).unwrap().bidder, "only-node");
    }

    #[test]
    fn reputation_bonus_can_break_near_tie() {
        let a = bid("node-a", 100, 80);
        let b = bid("node-b", 90, 80);
        let score_a = calculate_bid_score(&a, Some(1.0));
        let score_b = calculate_bid_score(&b, Some(0.0));
        assert!(score_a > score_b);
    }

    #[test]
    fn score_is_non_negative() {
        let s = calculate_bid_score(&bid("x", 99_999, 0), Some(0.0));
        assert!(s >= 0.0);
    }
}