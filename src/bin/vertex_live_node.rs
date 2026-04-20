//! Long-lived distributed delivery node over Tashi Vertex consensus.
//!
//! Each instance:
//!   - loads its identity and peers from a TOML config file
//!   - joins the Tashi Vertex BFT cluster
//!   - emits local intents (AgentState, OrderCreated, AuctionBid …) as
//!     Vertex transactions
//!   - advances `App` state **only** when messages come back from the
//!     consensus-ordered event stream
//!
//! # Usage
//! ```
//! cargo run --bin vertex_live_node -- --config config/node1.toml
//! ```

use std::collections::{HashSet, VecDeque};

use anyhow::Result;
use clap::Parser;
use tashi_vertex::{Context, Engine, KeySecret, Message, Options, Peers, Socket, Transaction};
use tokio::time::{interval, Duration, MissedTickBehavior};
use vertex_swarm_demo::{
    app::App,
    codec::{decode_message, encode_message},
    config::load_config,
    domain::{auction, order as order_fsm},
    types::{AgentStatus, AuctionBid, NexusMessage, OrderStatus, Timestamp},
};

// ─────────────────────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "vertex-live-node")]
#[command(about = "Distributed delivery node running over Tashi Vertex consensus")]
struct Args {
    /// Path to TOML node configuration file.
    #[arg(short, long, default_value = "config/node1.toml")]
    config: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("[node] loading config from {}", args.config);
    let config = load_config(&args.config)?;
    println!("[node] config loaded  node_id={}  bind={}  peers={}", config.node_id, config.bind, config.peers.len());

    // ── App state machine ──────────────────────────────────────────────────
    let mut app = App::from_config(config.clone())?;
    let node_id = config.node_id.clone();
    println!("[node] app state initialised  node_id={node_id}");

    // ── Vertex engine ──────────────────────────────────────────────────────
    // Replicate the setup from consensus/vertex.rs inline; the consensus
    // module is not exposed through lib.rs (it's Linux-only), so each binary
    // that needs Vertex bootstraps it directly, exactly like main.rs does.
    let key: KeySecret = config.secret_key.parse()?;
    let mut peer_set = Peers::with_capacity(config.peers.len() + 1)?;
    for peer in &config.peers {
        let public = peer.public_key.parse()?;
        peer_set.insert(&peer.address, &public, Default::default())?;
    }
    // Add ourselves to the peer set.
    peer_set.insert(&config.bind, &key.public(), Default::default())?;

    let context = Context::new()?;
    let socket = Socket::bind(&context, &config.bind).await?;
    let mut options = Options::default();
    options.set_report_gossip_events(true);
    options.set_fallen_behind_kick_s(10);
    // false = join as a new session participant.
    let engine = Engine::start(&context, socket, options, &key, peer_set, false)?;
    println!("[node] vertex engine started — node is live");

    // ── Runtime state (local to this binary, NOT App) ──────────────────────
    // Per-node offset within the 20-tick order-creation window so that nodes
    // don't all emit OrderCreated on the same tick. Uses a simple byte-sum
    // hash that gives distinct values for "node1"/"node2"/"node3" (11/12/13).
    let order_emit_tick: u64 =
        node_id.bytes().fold(0u64, |a, b| a.wrapping_add(b as u64)) % 20;

    let mut tick: u64 = 0;
    let mut order_seq: u64 = 0;
    // NexusMessages queued for transmission; drained one per tick.
    let mut pending_sends: VecDeque<NexusMessage> = VecDeque::new();
    // Orders for which this node has already emitted AuctionWinner.
    let mut resolved_auctions: HashSet<String> = HashSet::new();
    // Orders for which this node is currently the winner and is delivering.
    let mut delivering: HashSet<String> = HashSet::new();

    // ── Timers / shutdown ──────────────────────────────────────────────────
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Announce our presence immediately.
    send_nexus(&engine, &app.heartbeat())?;

    println!("[node] node started — entering event loop");

    // ── Main loop ──────────────────────────────────────────────────────────
    loop {
        tokio::select! {
            // ── 1-second tick ─────────────────────────────────────────────
            _ = ticker.tick() => {
                tick += 1;

                // Drain the oldest pending intent (one per tick to avoid flooding).
                if let Some(msg) = pending_sends.pop_front() {
                    println!("[t={tick}] local intent emitted  {}", label(&msg));
                    let _ = send_nexus(&engine, &msg);
                }

                // Always broadcast a heartbeat so peers know we're alive.
                let _ = send_nexus(&engine, &app.heartbeat());

                // Periodically create a new order if this node is idle.
                // Stagger offset computed once before the loop.
                if tick % 20 == order_emit_tick {
                    let busy = is_busy(&app, &node_id) || !pending_sends.is_empty();
                    if !busy {
                        order_seq += 1;
                        let order_id = format!("{}-order-{}", node_id, order_seq);
                        let pickup  = (40.7128 + order_seq as f64 * 0.001, -74.0060);
                        let dropoff = (40.7580 - order_seq as f64 * 0.001, -73.9855);
                        let (_, order_msg) = order_fsm::create_order(
                            &order_id,
                            node_id.clone(),
                            pickup,
                            dropoff,
                            now_ts(),
                        );
                        println!("[t={tick}] local intent emitted  OrderCreated  id={order_id}");
                        // Send through consensus — do NOT update App state locally.
                        let _ = send_nexus(&engine, &order_msg);
                    }
                }
            }

            // ── Consensus event received ───────────────────────────────────
            result = engine.recv_message() => {
                match result? {
                    None => {
                        println!("[node] engine stream closed — exiting");
                        break;
                    }
                    Some(Message::SyncPoint(_)) => {
                        // Heartbeat / quorum signal; no payload to process.
                    }
                    Some(Message::Event(event)) => {
                        for tx in event.transactions() {
                            // Transactions from main.rs (plain ping bytes) may not
                            // decode as NexusMessage; skip them silently.
                            let msg = match decode_message(&tx) {
                                Ok(m) => m,
                                Err(_) => continue,
                            };

                            println!("[t={tick}] consensus event received  {}", label(&msg));

                            // Decide what follow-up intents to queue BEFORE
                            // mutating App state (so we see the pre-update view).
                            let intents = decide_follow_ups(
                                &app,
                                &msg,
                                &node_id,
                                &mut resolved_auctions,
                                &mut delivering,
                            );

                            // Advance App state — single source of truth.
                            if let Err(e) = app.handle_message(msg) {
                                println!("[node] warn: handle_message: {e}");
                            }

                            // Queue follow-up intents for transmission.
                            for intent in intents {
                                println!("[t={tick}] queueing follow-up intent  {}", label(&intent));
                                pending_sends.push_back(intent);
                            }
                        }
                    }
                }
            }

            // ── Ctrl+C ────────────────────────────────────────────────────
            _ = &mut shutdown => {
                println!("[node] Ctrl+C received — shutting down");
                break;
            }
        }
    }

    println!("[node] stopped  node_id={node_id}  ticks={tick}");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Intent generator
// ─────────────────────────────────────────────────────────────────────────────

/// Inspect the incoming consensus message (and current App state) to decide
/// what follow-up intents this node should emit.
///
/// Called **before** `app.handle_message` so the pre-update state is visible.
/// Any returned messages are queued in `pending_sends`; they are transmitted
/// one per tick — they are never applied directly to local App state.
fn decide_follow_ups(
    app: &App,
    msg: &NexusMessage,
    node_id: &str,
    resolved_auctions: &mut HashSet<String>,
    delivering: &mut HashSet<String>,
) -> Vec<NexusMessage> {
    let mut out = Vec::new();

    match msg {
        // ── New order → submit a bid ───────────────────────────────────────
        NexusMessage::OrderCreated(order) => {
            // Any node that is idle submits a bid.
            if app.local_agent.status == AgentStatus::Idle && !is_busy(app, node_id) {
                let (dx, dy) = (
                    app.local_agent.location.0 - order.pickup.0,
                    app.local_agent.location.1 - order.pickup.1,
                );
                let eta = ((dx * dx + dy * dy).sqrt() * 111_000.0 / 15.0)
                    .round()
                    .clamp(1.0, 3_600.0) as u32;
                out.push(NexusMessage::AuctionBid(AuctionBid {
                    order_id: order.order_id.clone(),
                    bidder:   node_id.to_owned(),
                    eta_s:    eta,
                    battery_pct: app.local_agent.battery_pct,
                    submitted_at: now_ts(),
                }));
            }
        }

        // ── Bid received → originator resolves the auction ─────────────────
        NexusMessage::AuctionBid(bid) => {
            let already_resolved = resolved_auctions.contains(&bid.order_id);
            if already_resolved {
                return out;
            }

            if let Some(order) = app.orders.get(&bid.order_id) {
                let we_created = order.origin == node_id;
                let still_open = matches!(order.status, OrderStatus::Created | OrderStatus::Bidding);

                if we_created && still_open {
                    // Combine bids we already have with this new one.
                    let mut all_bids = app
                        .bids
                        .get(&bid.order_id)
                        .cloned()
                        .unwrap_or_default();
                    all_bids.push(bid.clone());

                    if let Some(winner) = auction::choose_winner_id(&all_bids) {
                        println!(
                            "[auction] order {} → winner={winner}",
                            bid.order_id
                        );
                        resolved_auctions.insert(bid.order_id.clone());
                        out.push(NexusMessage::AuctionWinner {
                            order_id: bid.order_id.clone(),
                            winner,
                        });
                    }
                }
            }
        }

        // ── We won → simulate delivery ─────────────────────────────────────
        NexusMessage::AuctionWinner { order_id, winner } => {
            if winner == node_id && !delivering.contains(order_id) {
                delivering.insert(order_id.clone());
                println!("[node] order assigned  id={order_id}  winner={winner}");
                // Queue the delivery confirmation. In a production system this
                // would happen after physical pickup + transit; for the MVP
                // simulation it fires after a fixed queue delay (pending_sends
                // drains one message per tick so preceding items add latency).
                out.push(NexusMessage::OrderDelivered {
                    order_id:     order_id.clone(),
                    delivered_by: node_id.to_owned(),
                    at:           now_ts(),
                });
            }
        }

        // ── Order delivered → log + remove from delivery tracker ───────────
        NexusMessage::OrderDelivered { order_id, delivered_by, .. } => {
            delivering.remove(order_id);
            println!(
                "[node] order delivered  id={order_id}  by={delivered_by}"
            );
        }

        // ── Handoff complete → log ─────────────────────────────────────────
        NexusMessage::HandoffComplete { order_id, new_holder } => {
            println!(
                "[node] handoff completed  id={order_id}  new_holder={new_holder}"
            );
        }

        _ => {}
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Send a `NexusMessage` through the Vertex engine as a transaction.
fn send_nexus(engine: &Engine, msg: &NexusMessage) -> Result<()> {
    let payload = encode_message(msg)?;
    let mut tx = Transaction::allocate(payload.len());
    tx.copy_from_slice(&payload);
    engine.send_transaction(tx)?;
    Ok(())
}

/// Return `true` if this node has at least one active (non-terminal) order.
fn is_busy(app: &App, node_id: &str) -> bool {
    app.orders.values().any(|o| {
        o.assigned_to.as_deref() == Some(node_id)
            && !matches!(o.status, OrderStatus::Delivered | OrderStatus::Cancelled)
    })
}

/// Short human-readable label for a `NexusMessage` variant.
fn label(msg: &NexusMessage) -> &'static str {
    match msg {
        NexusMessage::AgentState(_)          => "AgentState",
        NexusMessage::OrderCreated(_)        => "OrderCreated",
        NexusMessage::AuctionBid(_)          => "AuctionBid",
        NexusMessage::AuctionWinner { .. }   => "AuctionWinner",
        NexusMessage::HandoffRequest { .. }  => "HandoffRequest",
        NexusMessage::HandoffComplete { .. } => "HandoffComplete",
        NexusMessage::AgentFailure { .. }    => "AgentFailure",
        NexusMessage::SafetyAlert(_)         => "SafetyAlert",
        NexusMessage::SafetyClear { .. }     => "SafetyClear",
        NexusMessage::OrderDelivered { .. }  => "OrderDelivered",
    }
}

/// Current Unix timestamp in seconds.
fn now_ts() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
