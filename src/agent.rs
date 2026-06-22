use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use iroh::{
    Endpoint, RelayMode,
    endpoint::{Connection, presets},
};
use tempfile::TempDir;
use tokio::{fs, io::AsyncReadExt};
use tracing::{error, info, warn};

use crate::{
    config::{self, AgentConfig},
    identity,
    protocol::{ALPN, Phase, WireEvent, WireRequest, read_json, write_json},
    runner::{ResolvedTarget, run_command},
};

pub async fn run(config_path: PathBuf, identity_path: PathBuf) -> Result<()> {
    let config = Arc::new(config::load(&config_path)?);
    let secret_key = identity::load_or_generate(&identity_path)?;

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .map_err(|err| anyhow!("failed to bind Iroh endpoint: {err:#}"))?;

    endpoint.online().await;
    print_agent_addressing(&endpoint);

    while let Some(incoming) = endpoint.accept().await {
        let accepting = match incoming.accept() {
            Ok(accepting) => accepting,
            Err(err) => {
                warn!("incoming connection failed: {err:#}");
                continue;
            }
        };

        let config = Arc::clone(&config);
        tokio::spawn(async move {
            match accepting.await {
                Ok(conn) => {
                    if let Err(err) = handle_connection(conn, config).await {
                        error!("connection failed: {err:#}");
                    }
                }
                Err(err) => warn!("connection handshake failed: {err:#}"),
            }
        });
    }

    Ok(())
}

fn print_agent_addressing(endpoint: &Endpoint) {
    let addr = endpoint.addr();
    println!("agent endpoint id: {}", endpoint.id());

    if let Some(relay_url) = addr.relay_urls().next() {
        println!("agent relay url: {relay_url}");
    }

    let addrs = addr.ip_addrs().map(ToString::to_string).collect::<Vec<_>>();
    if !addrs.is_empty() {
        println!("agent direct addrs: {}", addrs.join(" "));
    }

    println!();
    println!("client example:");
    if let Some(relay_url) = addr.relay_urls().next() {
        println!(
            "  far-reach flash --peer {} --relay-url {} --target <target> --firmware <firmware>",
            endpoint.id(),
            relay_url
        );
    } else {
        println!(
            "  far-reach flash --peer {} --target <target> --firmware <firmware>",
            endpoint.id()
        );
    }
    println!();
}

async fn handle_connection(conn: Connection, config: Arc<AgentConfig>) -> Result<()> {
    let peer = conn.remote_id();
    if !config.authorized_peers.is_empty() && !config.authorized_peers.contains(&peer) {
        conn.close(1_u32.into(), b"unauthorized");
        anyhow::bail!("rejected unauthorized peer {peer}");
    }

    info!("accepted connection from {peer}");
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|err| anyhow!("failed to accept stream: {err:#}"))?;

    write_json(
        &mut send,
        &WireEvent::Accepted {
            peer: peer.to_string(),
        },
    )
    .await?;

    let request = read_json::<_, WireRequest>(&mut recv)
        .await?
        .context("client closed before sending a request")?;

    let result = match request {
        WireRequest::Flash {
            device,
            firmware_name,
            firmware_len,
            monitor_after,
        } => {
            let target = ResolvedTarget::resolve(&config.targets, device)?;
            let firmware = receive_firmware(
                &mut recv,
                config.work_dir.as_deref(),
                &firmware_name,
                firmware_len,
            )
            .await?;
            write_json(
                &mut send,
                &WireEvent::FirmwareSaved {
                    bytes: firmware_len,
                },
            )
            .await?;

            let argv = target.render_flash(firmware.path(), &firmware_name)?;
            let flash_ok = run_command(&mut send, Phase::Flash, argv).await?;
            if flash_ok && monitor_after {
                let argv = target.render_monitor()?;
                run_command(&mut send, Phase::Monitor, argv).await?
            } else {
                flash_ok
            }
        }
        WireRequest::Monitor { device } => {
            let target = ResolvedTarget::resolve(&config.targets, device)?;
            let argv = target.render_monitor()?;
            run_command(&mut send, Phase::Monitor, argv).await?
        }
    };

    write_json(&mut send, &WireEvent::Done { success: result }).await?;
    send.finish()
        .map_err(|err| anyhow!("failed to finish stream: {err:#}"))?;
    Ok(())
}

struct StagedFirmware {
    _dir: TempDir,
    path: PathBuf,
}

impl StagedFirmware {
    fn path(&self) -> &Path {
        &self.path
    }
}

async fn receive_firmware<R>(
    recv: &mut R,
    work_dir: Option<&Path>,
    firmware_name: &str,
    firmware_len: u64,
) -> Result<StagedFirmware>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let dir = match work_dir {
        Some(path) => {
            fs::create_dir_all(path)
                .await
                .with_context(|| format!("failed to create work dir {}", path.display()))?;
            tempfile::Builder::new()
                .prefix("upload-")
                .tempdir_in(path)
                .with_context(|| format!("failed to create temp dir in {}", path.display()))?
        }
        None => tempfile::Builder::new()
            .prefix("far-reach-")
            .tempdir()
            .context("failed to create temp dir")?,
    };

    let safe_name = Path::new(firmware_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("firmware.bin");
    let path = dir.path().join(safe_name);

    let mut file = fs::File::create(&path)
        .await
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut limited = recv.take(firmware_len);
    let copied = tokio::io::copy(&mut limited, &mut file)
        .await
        .with_context(|| format!("failed to receive firmware into {}", path.display()))?;

    if copied != firmware_len {
        anyhow::bail!("firmware upload ended after {copied} bytes, expected {firmware_len}");
    }

    Ok(StagedFirmware { _dir: dir, path })
}
