use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use iroh::EndpointId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub authorized_peers: Vec<EndpointId>,
    pub work_dir: Option<PathBuf>,
    #[serde(default)]
    pub targets: BTreeMap<String, TargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    #[serde(default)]
    pub flash: Option<Vec<String>>,
    #[serde(default)]
    pub monitor: Option<Vec<String>>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub chip: Option<String>,
}

pub fn load(path: &Path) -> Result<AgentConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config {}", path.display()))
}
