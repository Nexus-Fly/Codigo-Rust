use clap::Parser;
use tashi_vertex::{KeyPublic, KeySecret};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    secret: Option<String>,

    #[arg(long)]
    public: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if let Some(secret) = args.secret {
        let secret: KeySecret = secret.parse()?;
        let public = secret.public();

        println!("Secret: {secret}");
        println!("Public: {public}");
    } else if let Some(public) = args.public {
        let public: KeyPublic = public.parse()?;
        println!("Public: {public}");
    } else {
        println!("Uso:");
        println!("  cargo run --bin keyparse -- --secret <KEY>");
        println!("  cargo run --bin keyparse -- --public <KEY>");
    }

    Ok(())
}