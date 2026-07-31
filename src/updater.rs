use crate::{
    Distribution,
    forward::{self, HttpProxyConfig, JumpHost},
};
use anyhow::{Context, Result};
use semver::Version;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const REMOTE_DIRECTORY: &str = "/home/paas/s-porter";
#[cfg(target_os = "windows")]
pub const REMOTE_FILE_NAME: &str = "s-porter.exe";
#[cfg(target_os = "macos")]
pub const REMOTE_FILE_NAME: &str = "s-porter";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const REMOTE_FILE_NAME: &str = "s-porter";
const DOWNLOAD_BUFFER_SIZE: usize = 256 * 1024;

// 内网更新服务器配置。替换生产参数时只需修改这一处。
const UPDATE_SERVER_HOST: &str = "127.0.0.1";
const UPDATE_SERVER_PORT: u16 = 22;
const UPDATE_SERVER_USERNAME: &str = "tester";
const UPDATE_SERVER_PASSWORD: &str = "tester123";
const YELLOW_PROXY_HOST: &str = "127.0.0.1";
const YELLOW_PROXY_PORT: u16 = 8888;
const YELLOW_PROXY_USERNAME: &str = "proxyuser";
const YELLOW_PROXY_PASSWORD: &str = "proxypass";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateServerConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub http_proxy: Option<HttpProxyConfig>,
}

pub fn configured_server(distribution: Distribution) -> UpdateServerConfig {
    UpdateServerConfig {
        host: UPDATE_SERVER_HOST.into(),
        port: UPDATE_SERVER_PORT,
        username: UPDATE_SERVER_USERNAME.into(),
        password: UPDATE_SERVER_PASSWORD.into(),
        http_proxy: (distribution == Distribution::Yellow).then(|| HttpProxyConfig {
            host: YELLOW_PROXY_HOST.into(),
            port: YELLOW_PROXY_PORT,
            username: YELLOW_PROXY_USERNAME.into(),
            password: YELLOW_PROXY_PASSWORD.into(),
        }),
    }
}

fn distribution_directory(distribution: Distribution) -> &'static str {
    match distribution {
        Distribution::Yellow => "yellow",
        Distribution::Green => "green",
    }
}

impl UpdateServerConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.host.trim().is_empty(), "更新服务器 IP 不能为空");
        anyhow::ensure!(self.port > 0, "更新服务器 SSH 端口无效");
        anyhow::ensure!(!self.username.trim().is_empty(), "更新服务器用户名不能为空");
        anyhow::ensure!(!self.password.is_empty(), "更新服务器密码不能为空");
        if let Some(proxy) = &self.http_proxy {
            anyhow::ensure!(!proxy.host.trim().is_empty(), "HTTP 代理地址不能为空");
            anyhow::ensure!(proxy.port > 0, "HTTP 代理端口无效");
            anyhow::ensure!(
                !proxy.host.contains("://"),
                "HTTP 代理地址只需填写主机名或 IP"
            );
        }
        Ok(())
    }

    fn jump_host(&self) -> JumpHost {
        JumpHost {
            id: "update-server".into(),
            name: "更新服务器".into(),
            host: self.host.trim().into(),
            port: self.port,
            username: self.username.trim().into(),
            password: self.password.clone(),
            root_username: "root".into(),
            root_password: "unused".into(),
            http_proxy: self.http_proxy.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub remote_path: String,
    pub size: u64,
}

impl UpdateInfo {
    pub fn update_available(&self) -> bool {
        match (
            Version::parse(&self.latest_version),
            Version::parse(&self.current_version),
        ) {
            (Ok(latest), Ok(current)) => latest > current,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UpdateProgressSnapshot {
    pub transferred: u64,
    pub total: u64,
}

impl UpdateProgressSnapshot {
    pub fn percentage(&self) -> f32 {
        if self.total == 0 {
            0.
        } else {
            (self.transferred as f32 * 100. / self.total as f32).clamp(0., 100.)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UpdateProgress(Arc<Mutex<UpdateProgressSnapshot>>);

impl UpdateProgress {
    pub fn snapshot(&self) -> UpdateProgressSnapshot {
        self.0.lock().map(|value| value.clone()).unwrap_or_default()
    }

    fn set_total(&self, total: u64) {
        if let Ok(mut value) = self.0.lock() {
            value.total = total;
            value.transferred = 0;
        }
    }

    fn add(&self, amount: u64) {
        if let Ok(mut value) = self.0.lock() {
            value.transferred = value.transferred.saturating_add(amount).min(value.total);
        }
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn check(config: &UpdateServerConfig, distribution: Distribution) -> Result<UpdateInfo> {
    config.validate()?;
    let session = forward::connect(&config.jump_host()).context("连接更新服务器失败")?;
    let sftp = session.sftp().context("初始化更新服务器 SFTP 失败")?;
    let entries = sftp
        .readdir(Path::new(REMOTE_DIRECTORY))
        .with_context(|| format!("读取更新目录 {REMOTE_DIRECTORY} 失败"))?;

    let mut releases = entries
        .into_iter()
        .filter_map(|(path, stat)| {
            if !stat.is_dir() {
                return None;
            }
            let version = version_from_directory(&path)?;
            let executable = path
                .join(distribution_directory(distribution))
                .join(REMOTE_FILE_NAME);
            let executable_stat = sftp.stat(&executable).ok()?;
            executable_stat.is_file().then_some((
                version,
                executable,
                executable_stat.size.unwrap_or(0),
            ))
        })
        .collect::<Vec<_>>();
    releases.sort_by(|left, right| left.0.cmp(&right.0));
    let (latest, path, size) = releases.pop().ok_or_else(|| {
        anyhow::anyhow!(
            "更新目录中没有找到 {REMOTE_DIRECTORY}/<版本号>/{}/{REMOTE_FILE_NAME}",
            distribution_directory(distribution)
        )
    })?;

    Ok(UpdateInfo {
        current_version: current_version().into(),
        latest_version: latest.to_string(),
        remote_path: path.to_string_lossy().into_owned(),
        size,
    })
}

pub fn download(
    config: &UpdateServerConfig,
    info: &UpdateInfo,
    progress: &UpdateProgress,
) -> Result<PathBuf> {
    config.validate()?;
    let session = forward::connect(&config.jump_host()).context("连接更新服务器失败")?;
    let sftp = session.sftp().context("初始化更新服务器 SFTP 失败")?;
    let mut remote = sftp
        .open(Path::new(&info.remote_path))
        .with_context(|| format!("打开远程更新文件 {} 失败", info.remote_path))?;
    let total = remote
        .stat()
        .ok()
        .and_then(|stat| stat.size)
        .unwrap_or(info.size);
    anyhow::ensure!(total > 0, "远程更新文件为空");
    progress.set_total(total);

    let local_path = download_path(&info.latest_version);
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o700);
    let mut local = options
        .open(&local_path)
        .with_context(|| format!("创建临时更新文件 {} 失败", local_path.display()))?;
    let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
    loop {
        let count = remote.read(&mut buffer).context("下载更新文件失败")?;
        if count == 0 {
            break;
        }
        local
            .write_all(&buffer[..count])
            .context("写入更新文件失败")?;
        progress.add(count as u64);
    }
    local.sync_all().context("同步更新文件失败")?;
    anyhow::ensure!(
        progress.snapshot().transferred == total,
        "更新文件下载不完整"
    );
    #[cfg(unix)]
    fs::set_permissions(&local_path, fs::Permissions::from_mode(0o700))?;
    Ok(local_path)
}

pub fn install_and_restart(downloaded: &Path) -> Result<()> {
    anyhow::ensure!(downloaded.is_file(), "下载的更新文件不存在");
    self_replace::self_replace(downloaded).context("替换应用程序失败")?;
    let _ = fs::remove_file(downloaded);
    let executable = std::env::current_exe().context("获取应用程序路径失败")?;
    Command::new(executable)
        .arg("--updated")
        .spawn()
        .context("重新启动应用程序失败")?;
    Ok(())
}

fn download_path(version: &str) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    std::env::temp_dir().join(format!("s-porter-update-{version}{suffix}"))
}

fn version_from_directory(path: &Path) -> Option<Version> {
    Version::parse(path.file_name()?.to_str()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_selects_expected_proxy() {
        assert!(configured_server(Distribution::Yellow).http_proxy.is_some());
        assert!(configured_server(Distribution::Green).http_proxy.is_none());
    }

    #[test]
    fn distribution_selects_expected_remote_directory() {
        assert_eq!(distribution_directory(Distribution::Yellow), "yellow");
        assert_eq!(distribution_directory(Distribution::Green), "green");
    }

    #[test]
    fn parses_version_directories() {
        assert_eq!(
            version_from_directory(Path::new("/home/paas/s-porter/1.2.3")),
            Some(Version::new(1, 2, 3))
        );
        assert_eq!(
            version_from_directory(Path::new("/home/paas/s-porter/2.0.0-beta.1")),
            Some(Version::parse("2.0.0-beta.1").unwrap())
        );
        assert_eq!(
            version_from_directory(Path::new("2.0.0")),
            Some(Version::new(2, 0, 0))
        );
        assert!(version_from_directory(Path::new("/home/paas/s-porter/latest")).is_none());
    }

    #[test]
    fn compares_available_version() {
        let info = UpdateInfo {
            current_version: "1.0.0".into(),
            latest_version: "1.1.0".into(),
            remote_path: format!("/home/paas/s-porter/1.1.0/green/{REMOTE_FILE_NAME}"),
            size: 1,
        };
        assert!(info.update_available());
    }
}
