use std::str::{from_utf8, FromStr};

use anyhow::anyhow;
use clap::Parser;
use tashi_vertex::{
    Context, Engine, KeyPublic, KeySecret, Message, Options, Peers, Socket, Transaction,
};

#[derive(Debug, Clone)]
struct PeerArg {
    pub address: String,
    pub public: KeyPublic,
}

impl FromStr for PeerArg {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (public, address) = s
            .split_once('@')
            .ok_or_else(|| anyhow!("Formato inválido. Usa <public_key>@<ip:puerto>"))?;

        let public = public.parse()?;
        let address = address.to_string();

        Ok(PeerArg { address, public })
    }
}

#[derive(Debug, Parser)]
#[command(name = "vertex-node")]
#[command(about = "Nodo de prueba para Tashi Vertex")]
struct Args {
    /// Dirección local donde este nodo escucha
    #[arg(short = 'B', long = "bind")]
    pub bind: String,

    /// Clave secreta Base58 del nodo
    #[arg(short = 'K', long = "key")]
    pub key: String,

    /// Peers remotos en formato <public_key>@<ip:puerto>
    #[arg(short = 'P', long = "peer")]
    pub peers: Vec<PeerArg>,

    /// Mensaje inicial que se enviará al arrancar
    #[arg(short = 'M', long = "message", default_value = "PING")]
    pub message: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let key = args.key.parse::<KeySecret>()?;

    // Crear conjunto de peers
    let mut peers = Peers::with_capacity(args.peers.len() + 1)?;
    for peer in &args.peers {
        peers.insert(&peer.address, &peer.public, Default::default())?;
    }

    // Agregarnos a nosotros mismos
    peers.insert(&args.bind, &key.public(), Default::default())?;
    println!(":: Red configurada con {} peers", args.peers.len() + 1);

    // Runtime/contexto de Vertex
    let context = Context::new()?;
    println!(":: Context inicializado");

    // Bind del socket local
    let socket = Socket::bind(&context, &args.bind).await?;
    println!(":: Socket escuchando en {}", args.bind);

    // Opciones del motor
    let mut options = Options::default();
    options.set_report_gossip_events(true);
    options.set_fallen_behind_kick_s(10);

    // false = iniciar nueva sesión, igual que el ejemplo oficial actual
    let engine = Engine::start(&context, socket, options, &key, peers, false)?;
    println!(":: Engine iniciado");

    // Enviar transacción inicial
    send_transaction_cstr(&engine, &args.message)?;
    println!(":: Transacción inicial enviada: {}", args.message);

    // Escuchar mensajes ordenados por consenso
    while let Some(message) = engine.recv_message().await? {
        match message {
            Message::Event(event) => {
                if event.transaction_count() > 0 {
                    println!("\n> EVENT");
                    println!("  - Creator: {}", event.creator());
                    println!("  - Created at: {}", event.created_at());
                    println!("  - Consensus at: {}", event.consensus_at());
                    println!("  - Transactions: {}", event.transaction_count());

                    for tx in event.transactions() {
                        match from_utf8(&tx) {
                            Ok(text) => println!("  - TX: {}", text.trim_end_matches('\0')),
                            Err(_) => println!("  - TX (binaria): {:?}", tx),
                        }
                    }
                }
            }
            Message::SyncPoint(_) => {
                println!("\n> SYNC POINT");
            }
        }
    }

    Ok(())
}

/// Envía una transacción string null-terminated, igual que el ejemplo oficial.
fn send_transaction_cstr(engine: &Engine, s: &str) -> tashi_vertex::Result<()> {
    let mut transaction = Transaction::allocate(s.len() + 1);
    transaction[..s.len()].copy_from_slice(s.as_bytes());
    transaction[s.len()] = 0;
    engine.send_transaction(transaction)
}