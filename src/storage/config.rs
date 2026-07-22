use crate::forward::ForwardConfig;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Default, Serialize, Deserialize)]
struct ConfigFile {
    forwards: Vec<ForwardConfig>,
}

fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "s-porter", "S Porter").context("无法确定应用配置目录")?;
    Ok(dirs.config_dir().join("forwards.json"))
}

pub fn load() -> Result<Vec<ForwardConfig>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("读取配置失败：{}", path.display()))?;
    Ok(serde_json::from_str::<ConfigFile>(&content)
        .context("配置文件格式错误")?
        .forwards)
}

pub fn save(forwards: &[ForwardConfig]) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("创建配置目录失败")?;
    }
    let content = serde_json::to_string_pretty(&ConfigFile {
        forwards: forwards.to_vec(),
    })?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .with_context(|| format!("打开配置文件失败：{}", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("保存配置失败：{}", path.display()))?;
    file.sync_all().context("同步配置文件失败")?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
