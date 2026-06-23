mod client;
mod config;
mod identity;
mod protocol;
mod runner;
mod serve;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use iroh::{EndpointId, RelayUrl};
use protocol::{DeviceOverrides, Platform};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve remote flashing requests from the bench host.
    Serve(ServeArgs),
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
struct ServeArgs {
    /// Server config file.
    #[arg(long, default_value = "farreach.toml")]
    config: PathBuf,
    /// Iroh secret key file for this server.
    #[arg(long, default_value = ".farreach/server.key")]
    identity: PathBuf,
}

#[derive(Debug, Args)]
struct RemoteArgs {
    /// Iroh secret key file for this client.
    #[arg(long, default_value = ".farreach/client.key")]
    identity: PathBuf,
    /// Remote server endpoint ID.
    #[arg(long)]
    peer: EndpointId,
    /// Remote server relay URL, printed by `fr serve`.
    #[arg(long)]
    relay_url: Option<RelayUrl>,
    /// Optional direct UDP addresses printed by `fr serve`.
    #[arg(long, value_parser, num_args = 0.., value_delimiter = ' ')]
    addr: Vec<SocketAddr>,
}

#[derive(Debug, Args)]
struct TargetArgs {
    /// Target name from the server config. If omitted, the server selects by --platform or by its only configured target.
    #[arg(long)]
    target: Option<String>,
    /// Platform hint used by the server to pick the connected target automatically.
    #[arg(long, value_enum)]
    platform: Option<Platform>,
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
    /// Already-built local firmware image to upload to the remote server.
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
    #[arg(long, default_value = ".farreach/client.key")]
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
        Command::Serve(args) => serve::run(args.config, args.identity).await,
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
            platform: value.platform,
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
