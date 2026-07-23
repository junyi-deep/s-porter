use crate::forward::{ForwardConfig, HttpProxyConfig, JumpHost};
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    pub command: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub jump_hosts: Vec<JumpHost>,
    #[serde(default)]
    pub forwards: Vec<ForwardConfig>,
    #[serde(default)]
    pub quick_commands: Vec<QuickCommand>,
    #[serde(default)]
    pub command_history: Vec<String>,
}

#[derive(Deserialize)]
struct LegacyConfigFile {
    #[serde(default)]
    forwards: Vec<LegacyForwardConfig>,
}

#[derive(Clone, Deserialize)]
struct LegacyForwardConfig {
    id: String,
    name: String,
    local_port: u16,
    remote_ip: String,
    remote_port: u16,
    ssh_ip: String,
    ssh_port: u16,
    ssh_user: String,
    ssh_password: String,
    root_user: String,
    root_password: String,
    #[serde(default)]
    http_proxy: Option<HttpProxyConfig>,
}

fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "s-porter", "S Porter").context("无法确定应用配置目录")?;
    Ok(dirs.config_dir().join("forwards.json"))
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("读取配置失败：{}", path.display()))?;
    if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
        return Ok(config);
    }
    let legacy = serde_json::from_str::<LegacyConfigFile>(&content).context("配置文件格式错误")?;
    Ok(migrate_legacy(legacy))
}

pub fn save(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("创建配置目录失败")?;
    }
    let content = serde_json::to_string_pretty(config)?;
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

fn migrate_legacy(legacy: LegacyConfigFile) -> AppConfig {
    let mut config = AppConfig::default();
    for item in legacy.forwards {
        let jump_host_id = config
            .jump_hosts
            .iter()
            .find(|host| {
                host.host == item.ssh_ip
                    && host.port == item.ssh_port
                    && host.username == item.ssh_user
                    && host.password == item.ssh_password
                    && host.root_username == item.root_user
                    && host.root_password == item.root_password
                    && host.http_proxy == item.http_proxy
            })
            .map(|host| host.id.clone())
            .unwrap_or_else(|| {
                let id = uuid::Uuid::new_v4().to_string();
                config.jump_hosts.push(JumpHost {
                    id: id.clone(),
                    name: format!("{}@{}:{}", item.ssh_user, item.ssh_ip, item.ssh_port),
                    host: item.ssh_ip.clone(),
                    port: item.ssh_port,
                    username: item.ssh_user.clone(),
                    password: item.ssh_password.clone(),
                    root_username: item.root_user.clone(),
                    root_password: item.root_password.clone(),
                    http_proxy: item.http_proxy.clone(),
                });
                id
            });
        config.forwards.push(ForwardConfig {
            id: item.id,
            name: item.name,
            local_port: item.local_port,
            remote_ip: item.remote_ip,
            remote_port: item.remote_port,
            jump_host_id,
            keep_alive: false,
            keep_alive_interval_secs: 30,
        });
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_and_deduplicates_legacy_jump_hosts() {
        let legacy = LegacyConfigFile {
            forwards: vec![legacy_forward("one", 8080), legacy_forward("two", 8081)],
        };
        let migrated = migrate_legacy(legacy);
        assert_eq!(migrated.jump_hosts.len(), 1);
        assert_eq!(migrated.forwards.len(), 2);
        assert_eq!(
            migrated.forwards[0].jump_host_id,
            migrated.forwards[1].jump_host_id
        );
    }

    #[test]
    fn old_config_defaults_command_collections() {
        let config: AppConfig = serde_json::from_str(r#"{"jump_hosts":[],"forwards":[]}"#).unwrap();
        assert!(config.quick_commands.is_empty());
        assert!(config.command_history.is_empty());
    }

    fn legacy_forward(name: &str, local_port: u16) -> LegacyForwardConfig {
        LegacyForwardConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            local_port,
            remote_ip: "10.0.0.10".into(),
            remote_port: 5432,
            ssh_ip: "jump.example.com".into(),
            ssh_port: 22,
            ssh_user: "service".into(),
            ssh_password: "login-password".into(),
            root_user: "root".into(),
            root_password: "root-password".into(),
            http_proxy: None,
        }
    }
}
