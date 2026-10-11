mod config;
mod control;
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
use protocol::AgentCommand;

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
    /// Block until the relay emits its next event.
    Wait {
        /// Last event ID already processed. Zero starts at the oldest retained event.
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Maximum wait before returning a wait_timeout event.
        #[arg(long, default_value_t = 120)]
        timeout_seconds: u64,
    },
    /// Speak text on the active telephone call through the local control listener.
    Speak {
        text: String,
        /// Marks this chunk as the end of the current response.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        last: bool,
    },
    /// End the active telephone call through the local control listener.
    Hangup {
        #[arg(long)]
        reason: Option<String>,
    },
    /// Check the local control path.
    Ping,
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
        Command::Wait {
            after,
            timeout_seconds,
        } => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            control::wait(&config, after, timeout_seconds).await
        }
        Command::Speak { text, last } => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            control::command(&config, AgentCommand::Speak { text, last }).await
        }
        Command::Hangup { reason } => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            control::command(&config, AgentCommand::Hangup { reason }).await
        }
        Command::Ping => {
            let config = Config::load(&cli.config)?;
            config.validate()?;
            control::command(&config, AgentCommand::Ping).await
        }
    }
}
