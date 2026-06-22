use std::io;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"far-reach/bench/0";
const MAX_FRAME_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceOverrides {
    pub target: String,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub chip: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WireRequest {
    Flash {
        device: DeviceOverrides,
        firmware_name: String,
        firmware_len: u64,
        monitor_after: bool,
    },
    Monitor {
        device: DeviceOverrides,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WireEvent {
    Accepted {
        peer: String,
    },
    FirmwareSaved {
        bytes: u64,
    },
    CommandStarted {
        phase: Phase,
        argv: Vec<String>,
    },
    Stdout {
        phase: Phase,
        text: String,
    },
    Stderr {
        phase: Phase,
        text: String,
    },
    CommandExited {
        phase: Phase,
        code: Option<i32>,
        success: bool,
    },
    Done {
        success: bool,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Flash,
    Monitor,
}

pub async fn write_json<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).context("failed to encode frame")?;
    let len = u32::try_from(bytes.len()).context("frame too large")?;
    if len > MAX_FRAME_BYTES {
        bail!("frame is larger than {MAX_FRAME_BYTES} bytes");
    }

    writer
        .write_u32(len)
        .await
        .context("failed to write frame length")?;
    writer
        .write_all(&bytes)
        .await
        .context("failed to write frame body")?;
    writer.flush().await.context("failed to flush frame")?;
    Ok(())
}

pub async fn read_json<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let len = match reader.read_u32().await {
        Ok(len) => len,
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err).context("failed to read frame length"),
    };

    if len > MAX_FRAME_BYTES {
        bail!("received frame larger than {MAX_FRAME_BYTES} bytes");
    }

    let mut bytes = vec![0_u8; len as usize];
    reader
        .read_exact(&mut bytes)
        .await
        .context("failed to read frame body")?;
    let value = serde_json::from_slice(&bytes).context("failed to decode frame")?;
    Ok(Some(value))
}
