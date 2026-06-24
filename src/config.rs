use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use iroh::EndpointId;
use serde::Deserialize;

use crate::protocol::Platform;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub authorized_peers: Vec<EndpointId>,
    pub work_dir: Option<PathBuf>,
    #[serde(default)]
    pub targets: BTreeMap<String, TargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    #[serde(default)]
    pub platform: Option<Platform>,
    #[serde(default)]
    pub flash: Option<Vec<String>>,
    #[serde(default)]
    pub monitor: Option<MonitorConfig>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub chip: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MonitorConfig {
    Command(Vec<String>),
    SeggerRtt(SeggerRttConfig),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeggerRttConfig {
    #[serde(rename = "type")]
    _kind: SeggerRttBackend,
    #[serde(default = "default_segger_rtt_host")]
    pub host: String,
    #[serde(default = "default_segger_rtt_port")]
    pub port: u16,
    #[serde(default = "default_segger_rtt_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum SeggerRttBackend {
    #[serde(alias = "rtt-segger")]
    SeggerRtt,
}

fn default_segger_rtt_host() -> String {
    "127.0.0.1".to_string()
}

fn default_segger_rtt_port() -> u16 {
    19021
}

fn default_segger_rtt_connect_timeout_ms() -> u64 {
    5_000
}

pub fn load(path: &Path) -> Result<ServerConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_monitor_config() {
        let target = toml::from_str::<TargetConfig>(
            r#"
monitor = ["probe-rs", "attach", "--chip", "{chip}"]
"#,
        )
        .unwrap();

        match target.monitor.unwrap() {
            MonitorConfig::Command(argv) => {
                assert_eq!(argv, vec!["probe-rs", "attach", "--chip", "{chip}"]);
            }
            MonitorConfig::SeggerRtt(_) => panic!("expected command monitor"),
        }
    }

    #[test]
    fn parses_segger_rtt_monitor_config_with_defaults() {
        let target = toml::from_str::<TargetConfig>(
            r#"
monitor = { type = "segger-rtt" }
"#,
        )
        .unwrap();

        match target.monitor.unwrap() {
            MonitorConfig::SeggerRtt(config) => {
                assert_eq!(config.host, "127.0.0.1");
                assert_eq!(config.port, 19021);
                assert_eq!(config.connect_timeout_ms, 5_000);
            }
            MonitorConfig::Command(_) => panic!("expected SEGGER RTT monitor"),
        }
    }
}
