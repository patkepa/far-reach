use std::{fs, path::Path, str::FromStr};

use anyhow::{Context, Result, bail};
use iroh::SecretKey;

pub fn load_or_generate(path: &Path) -> Result<SecretKey> {
    if path.exists() {
        return load(path);
    }

    let key = SecretKey::generate();
    write(path, &key)?;
    Ok(key)
}

pub fn generate(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to replace it",
            path.display()
        );
    }

    let key = SecretKey::generate();
    write(path, &key)?;
    println!("identity: {}", path.display());
    println!("endpoint id: {}", key.public());
    Ok(())
}

fn load(path: &Path) -> Result<SecretKey> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read identity {}", path.display()))?;
    SecretKey::from_str(raw.trim())
        .with_context(|| format!("failed to parse identity {}", path.display()))
}

fn write(path: &Path, key: &SecretKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(path, format!("{}\n", hex::encode(key.to_bytes())))
        .with_context(|| format!("failed to write identity {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to lock down {}", path.display()))?;
    }

    Ok(())
}
