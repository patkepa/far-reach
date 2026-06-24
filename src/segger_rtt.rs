use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, net::TcpStream, time::timeout};

use crate::{
    config::SeggerRttConfig,
    protocol::{Phase, WireEvent, write_json},
};

pub async fn run<S>(send: &mut S, config: &SeggerRttConfig) -> Result<bool>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    if config.host.trim().is_empty() {
        bail!("SEGGER RTT host cannot be empty");
    }
    if config.port == 0 {
        bail!("SEGGER RTT port cannot be 0");
    }

    let endpoint = format!("{}:{}", config.host, config.port);
    write_json(
        send,
        &WireEvent::Status {
            message: format!("connecting to SEGGER RTT at {endpoint}"),
        },
    )
    .await?;

    let connect_timeout = Duration::from_millis(config.connect_timeout_ms);
    let mut stream = timeout(
        connect_timeout,
        TcpStream::connect((config.host.as_str(), config.port)),
    )
    .await
    .with_context(|| format!("timed out connecting to SEGGER RTT at {endpoint}"))?
    .with_context(|| format!("failed to connect to SEGGER RTT at {endpoint}"))?;

    write_json(
        send,
        &WireEvent::Status {
            message: format!("connected to SEGGER RTT at {endpoint}"),
        },
    )
    .await?;

    let mut buf = [0_u8; 8192];
    loop {
        let n = stream
            .read(&mut buf)
            .await
            .with_context(|| format!("failed to read from SEGGER RTT at {endpoint}"))?;
        if n == 0 {
            write_json(
                send,
                &WireEvent::Status {
                    message: format!("SEGGER RTT connection at {endpoint} closed"),
                },
            )
            .await?;
            return Ok(true);
        }

        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        write_json(
            send,
            &WireEvent::Stdout {
                phase: Phase::Monitor,
                text,
            },
        )
        .await?;
    }
}
