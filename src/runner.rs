use std::{collections::BTreeMap, path::Path, process::Stdio};

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, process::Command};

use crate::{
    config::TargetConfig,
    protocol::{DeviceOverrides, Phase, WireEvent, write_json},
};

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    name: String,
    flash: Option<Vec<String>>,
    monitor: Option<Vec<String>>,
    serial: Option<String>,
    baud: Option<u32>,
    chip: Option<String>,
}

impl ResolvedTarget {
    pub fn resolve(
        targets: &BTreeMap<String, TargetConfig>,
        device: DeviceOverrides,
    ) -> Result<Self> {
        let config = targets
            .get(&device.target)
            .with_context(|| format!("unknown target '{}'", device.target))?;

        Ok(Self {
            name: device.target,
            flash: config.flash.clone(),
            monitor: config.monitor.clone(),
            serial: device.serial.or_else(|| config.serial.clone()),
            baud: device.baud.or(config.baud),
            chip: device.chip.or_else(|| config.chip.clone()),
        })
    }

    pub fn render_flash(&self, firmware: &Path, firmware_name: &str) -> Result<Vec<String>> {
        let template = self
            .flash
            .as_ref()
            .context("target does not define a flash command")?;
        self.render(template, Some(firmware), firmware_name)
    }

    pub fn render_monitor(&self) -> Result<Vec<String>> {
        let template = self
            .monitor
            .as_ref()
            .context("target does not define a monitor command")?;
        self.render(template, None, "")
    }

    fn render(
        &self,
        template: &[String],
        firmware: Option<&Path>,
        firmware_name: &str,
    ) -> Result<Vec<String>> {
        if template.is_empty() {
            bail!("command template cannot be empty");
        }

        template
            .iter()
            .map(|arg| self.render_arg(arg, firmware, firmware_name))
            .collect()
    }

    fn render_arg(
        &self,
        arg: &str,
        firmware: Option<&Path>,
        firmware_name: &str,
    ) -> Result<String> {
        let mut out = arg.replace("{target}", &self.name);

        if out.contains("{firmware}") {
            let firmware = firmware.context("{firmware} used without a firmware upload")?;
            out = out.replace("{firmware}", &firmware.display().to_string());
        }

        if out.contains("{firmware_name}") {
            out = out.replace("{firmware_name}", firmware_name);
        }

        if out.contains("{serial}") {
            let serial = self
                .serial
                .as_deref()
                .context("{serial} used but no serial is set")?;
            out = out.replace("{serial}", serial);
        }

        if out.contains("{baud}") {
            let baud = self.baud.context("{baud} used but no baud rate is set")?;
            out = out.replace("{baud}", &baud.to_string());
        }

        if out.contains("{chip}") {
            let chip = self
                .chip
                .as_deref()
                .context("{chip} used but no chip is set")?;
            out = out.replace("{chip}", chip);
        }

        if out.contains('{') || out.contains('}') {
            bail!("unknown placeholder in command argument '{arg}'");
        }

        Ok(out)
    }
}

pub async fn run_command<S>(send: &mut S, phase: Phase, argv: Vec<String>) -> Result<bool>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    if argv.is_empty() {
        bail!("command cannot be empty");
    }

    write_json(
        send,
        &WireEvent::CommandStarted {
            phase,
            argv: argv.clone(),
        },
    )
    .await?;

    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start '{}'", argv[0]))?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut out_buf = [0_u8; 8192];
    let mut err_buf = [0_u8; 8192];
    let mut exit = None;

    while stdout.is_some() || stderr.is_some() || exit.is_none() {
        tokio::select! {
            result = read_optional(&mut stdout, &mut out_buf), if stdout.is_some() => {
                let n = result.context("failed to read command stdout")?;
                if n == 0 {
                    stdout = None;
                } else {
                    let text = String::from_utf8_lossy(&out_buf[..n]).to_string();
                    write_json(send, &WireEvent::Stdout { phase, text }).await?;
                }
            }
            result = read_optional(&mut stderr, &mut err_buf), if stderr.is_some() => {
                let n = result.context("failed to read command stderr")?;
                if n == 0 {
                    stderr = None;
                } else {
                    let text = String::from_utf8_lossy(&err_buf[..n]).to_string();
                    write_json(send, &WireEvent::Stderr { phase, text }).await?;
                }
            }
            status = child.wait(), if exit.is_none() => {
                exit = Some(status.context("failed to wait for command")?);
            }
        }
    }

    let status = exit.expect("loop exits only after process status exists");
    let success = status.success();
    write_json(
        send,
        &WireEvent::CommandExited {
            phase,
            code: status.code(),
            success,
        },
    )
    .await?;

    Ok(success)
}

async fn read_optional<R>(reader: &mut Option<R>, buf: &mut [u8]) -> std::io::Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    match reader {
        Some(reader) => reader.read(buf).await,
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_command_placeholders() {
        let target = ResolvedTarget {
            name: "esp32c3".to_string(),
            flash: None,
            monitor: None,
            serial: Some("/dev/ttyUSB0".to_string()),
            baud: Some(115200),
            chip: Some("esp32c3".to_string()),
        };

        let rendered = target
            .render(
                &[
                    "espflash".to_string(),
                    "flash".to_string(),
                    "--port".to_string(),
                    "{serial}".to_string(),
                    "--chip".to_string(),
                    "{chip}".to_string(),
                    "{firmware}".to_string(),
                ],
                Some(Path::new("/tmp/fw.bin")),
                "fw.bin",
            )
            .unwrap();

        assert_eq!(
            rendered,
            vec![
                "espflash",
                "flash",
                "--port",
                "/dev/ttyUSB0",
                "--chip",
                "esp32c3",
                "/tmp/fw.bin"
            ]
        );
    }
}
