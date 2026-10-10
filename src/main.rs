mod config;
mod credentials;
mod output;
mod protocol;
mod relay;
mod simulate;
mod telnyx;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;

#[derive(Debug, Parser)]
#[command(
    name = "bellhound",
    version,
    about = "A telephone TTY for interactive agents"
)]
struct Cli {
    #[arg(long, global = true, default_value = "bellhound.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write a documented starter configuration.
    Init {
        /// Replace an existing configuration file.
        #[arg(long)]
        force: bool,
    },
    /// Parse and validate the configuration without starting a listener.
    Check,
    /// Verify that a Telnyx API key can be resolved without printing it.
    Credentials,
    /// Run a deterministic local telephone conversation from a JSONL script.
    Simulate {
        #[arg(long)]
        script: PathBuf,
    },
    /// Serve the Telnyx Conversation Relay WebSocket bridge.
    Relay,
    /// Place an outbound call and attach it to an already-running relay.
    Dial {
        /// The first text Telnyx speaks after the call is answered.
        #[arg(long)]
        opening: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bellhound=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Init { force } => Config::write_example(&cli.config, force),
        Command::Check => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            println!("configuration valid: {}", cli.config.display());
            Ok(())
        }
        Command::Credentials => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            let resolved = credentials::resolve(&config.credentials)?;
            println!("credential available via {}", resolved.source());
            Ok(())
        }
        Command::Simulate { script } => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            simulate::run(config, script).await
        }
        Command::Relay => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            relay::run(config).await
        }
        Command::Dial { opening } => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            telnyx::dial(&config, opening).await
        }
    }
}
