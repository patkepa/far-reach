mod agent;
mod client;
mod config;
mod identity;
mod protocol;
mod runner;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use iroh::{EndpointId, RelayUrl};
use protocol::DeviceOverrides;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the Raspberry Pi side bench agent.
    Agent(AgentArgs),
    /// Upload firmware and run the target flash command.
    Flash(FlashArgs),
    /// Run the target monitor command and stream logs.
    Monitor(MonitorArgs),
    /// Flash firmware, then run the target monitor command.
    FlashMonitor(FlashArgs),
    /// Generate a stable Iroh identity key.
    Keygen(KeygenArgs),
}

#[derive(Debug, Args)]
struct AgentArgs {
    /// Agent config file.
    #[arg(long, default_value = "far-reach.toml")]
    config: PathBuf,
    /// Iroh secret key file for this agent.
    #[arg(long, default_value = ".far-reach/agent.key")]
    identity: PathBuf,
}

#[derive(Debug, Args)]
struct RemoteArgs {
    /// Iroh secret key file for this client.
    #[arg(long, default_value = ".far-reach/client.key")]
    identity: PathBuf,
    /// Remote agent endpoint ID.
    #[arg(long)]
    peer: EndpointId,
    /// Remote agent relay URL, printed by `far-reach agent`.
    #[arg(long)]
    relay_url: Option<RelayUrl>,
    /// Optional direct UDP addresses printed by `far-reach agent`.
    #[arg(long, value_parser, num_args = 0.., value_delimiter = ' ')]
    addr: Vec<SocketAddr>,
}

#[derive(Debug, Args)]
struct TargetArgs {
    /// Target name from the agent config.
    #[arg(long)]
    target: String,
    /// Override target serial path, for example /dev/ttyUSB0.
    #[arg(long)]
    serial: Option<String>,
    /// Override target UART baud rate.
    #[arg(long)]
    baud: Option<u32>,
    /// Override chip name passed to command templates.
    #[arg(long)]
    chip: Option<String>,
}

#[derive(Debug, Args)]
struct FlashArgs {
    #[command(flatten)]
    remote: RemoteArgs,
    #[command(flatten)]
    target: TargetArgs,
    /// Optional local build command to run before uploading the firmware.
    #[arg(long)]
    build_command: Option<String>,
    /// Already-built local firmware image to upload to the remote agent.
    #[arg(long)]
    firmware: PathBuf,
}

#[derive(Debug, Args)]
struct MonitorArgs {
    #[command(flatten)]
    remote: RemoteArgs,
    #[command(flatten)]
    target: TargetArgs,
}

#[derive(Debug, Args)]
struct KeygenArgs {
    /// Destination key file.
    #[arg(long, default_value = ".far-reach/client.key")]
    identity: PathBuf,
    /// Replace an existing key.
    #[arg(long)]
    force: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Command::Agent(args) => agent::run(args.config, args.identity).await,
        Command::Flash(args) => {
            client::flash(
                args.remote.into(),
                args.target.into(),
                args.firmware,
                args.build_command,
                false,
            )
            .await
        }
        Command::FlashMonitor(args) => {
            client::flash(
                args.remote.into(),
                args.target.into(),
                args.firmware,
                args.build_command,
                true,
            )
            .await
        }
        Command::Monitor(args) => client::monitor(args.remote.into(), args.target.into()).await,
        Command::Keygen(args) => identity::generate(&args.identity, args.force),
    }
}

impl From<RemoteArgs> for client::Remote {
    fn from(value: RemoteArgs) -> Self {
        Self {
            identity: value.identity,
            peer: value.peer,
            relay_url: value.relay_url,
            addrs: value.addr,
        }
    }
}

impl From<TargetArgs> for DeviceOverrides {
    fn from(value: TargetArgs) -> Self {
        Self {
            target: value.target,
            serial: value.serial,
            baud: value.baud,
            chip: value.chip,
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
}
