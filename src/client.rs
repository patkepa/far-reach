use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, TransportAddr, endpoint::presets,
};
use tokio::{fs, process::Command};

use crate::{
    identity,
    protocol::{ALPN, DeviceOverrides, Phase, WireEvent, WireRequest, read_json, write_json},
};

#[derive(Debug)]
pub struct Remote {
    pub identity: PathBuf,
    pub peer: EndpointId,
    pub relay_url: Option<RelayUrl>,
    pub addrs: Vec<SocketAddr>,
}

pub async fn flash(
    remote: Remote,
    device: DeviceOverrides,
    firmware: PathBuf,
    build_command: Option<String>,
    monitor_after: bool,
) -> Result<()> {
    if let Some(build_command) = build_command {
        run_local_build(&build_command).await?;
    }

    let firmware_name = firmware
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("firmware.bin")
        .to_string();
    let firmware_len = fs::metadata(&firmware)
        .await
        .with_context(|| format!("failed to stat firmware {}", firmware.display()))?
        .len();

    let request = WireRequest::Flash {
        device,
        firmware_name,
        firmware_len,
        monitor_after,
    };

    run(remote, request, Some(firmware)).await
}

async fn run_local_build(build_command: &str) -> Result<()> {
    println!("[build] {build_command}");

    #[cfg(windows)]
    let status = Command::new("cmd")
        .args(["/C", build_command])
        .status()
        .await
        .context("failed to start local build command")?;

    #[cfg(not(windows))]
    let status = Command::new("sh")
        .args(["-c", build_command])
        .status()
        .await
        .context("failed to start local build command")?;

    if !status.success() {
        anyhow::bail!("local build command failed with {status}");
    }

    Ok(())
}

pub async fn monitor(remote: Remote, device: DeviceOverrides) -> Result<()> {
    run(remote, WireRequest::Monitor { device }, None).await
}

async fn run(remote: Remote, request: WireRequest, firmware: Option<PathBuf>) -> Result<()> {
    let endpoint = bind_endpoint(&remote.identity).await?;
    let remote_addr = endpoint_addr(&remote);
    let conn = endpoint
        .connect(remote_addr, ALPN)
        .await
        .map_err(|err| anyhow!("failed to connect to server: {err:#}"))?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|err| anyhow!("failed to open stream: {err:#}"))?;

    write_json(&mut send, &request).await?;

    if let Some(firmware) = firmware {
        let mut file = fs::File::open(&firmware)
            .await
            .with_context(|| format!("failed to open firmware {}", firmware.display()))?;
        tokio::io::copy(&mut file, &mut send)
            .await
            .with_context(|| format!("failed to upload firmware {}", firmware.display()))?;
    }

    send.finish()
        .map_err(|err| anyhow!("failed to finish request stream: {err:#}"))?;

    let mut success = None;
    while let Some(event) = read_json::<_, WireEvent>(&mut recv).await? {
        match event {
            WireEvent::Accepted { peer } => println!("accepted by server as peer {peer}"),
            WireEvent::FirmwareSaved { bytes } => println!("uploaded {bytes} bytes"),
            WireEvent::Status { message } => println!("[status] {message}"),
            WireEvent::TargetResolved { target, platform } => {
                let platform = platform
                    .map(|platform| platform.to_string())
                    .unwrap_or_else(|| "unspecified".to_string());
                println!("[target] {target} ({platform})");
            }
            WireEvent::CommandStarted { phase, argv } => {
                println!("[{}] {}", phase_name(phase), shellish(&argv));
            }
            WireEvent::Stdout { text, .. } => print!("{text}"),
            WireEvent::Stderr { text, .. } => eprint!("{text}"),
            WireEvent::CommandExited {
                phase,
                code,
                success: command_success,
            } => {
                let code = code.map_or("signal".to_string(), |code| code.to_string());
                println!(
                    "[{}] exited with {code} ({})",
                    phase_name(phase),
                    if command_success { "ok" } else { "failed" }
                );
            }
            WireEvent::Done {
                success: request_success,
            } => success = Some(request_success),
            WireEvent::Error { message } => {
                eprintln!("server error: {message}");
                success = Some(false);
            }
        }
    }

    endpoint.close().await;

    match success {
        Some(true) => Ok(()),
        Some(false) => anyhow::bail!("remote command failed"),
        None => anyhow::bail!("server closed without a completion event"),
    }
}

async fn bind_endpoint(identity_path: &std::path::Path) -> Result<Endpoint> {
    let secret_key = identity::load_or_generate(identity_path)?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .map_err(|err| anyhow!("failed to bind Iroh endpoint: {err:#}"))?;
    endpoint.online().await;
    Ok(endpoint)
}

fn endpoint_addr(remote: &Remote) -> EndpointAddr {
    let transports = remote
        .addrs
        .iter()
        .copied()
        .map(TransportAddr::Ip)
        .chain(remote.relay_url.clone().map(TransportAddr::Relay));

    EndpointAddr::from_parts(remote.peer, transports)
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Flash => "flash",
        Phase::Monitor => "monitor",
    }
}

fn shellish(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=+".contains(ch))
            {
                arg.clone()
            } else {
                format!("{arg:?}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
