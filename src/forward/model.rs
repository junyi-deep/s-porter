use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HttpProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardConfig {
    pub id: String,
    pub name: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub ssh_ip: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_password: String,
    pub root_user: String,
    pub root_password: String,
    #[serde(default)]
    pub http_proxy: Option<HttpProxyConfig>,
}

impl ForwardConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.name.trim().is_empty(), "配置名称不能为空");
        anyhow::ensure!(!self.remote_ip.trim().is_empty(), "远程 IP 不能为空");
        anyhow::ensure!(self.remote_port > 0, "远程端口无效");
        anyhow::ensure!(self.local_port > 0, "本地端口无效");
        anyhow::ensure!(!self.ssh_ip.trim().is_empty(), "SSH 服务 IP 不能为空");
        anyhow::ensure!(!self.ssh_user.trim().is_empty(), "SSH 用户名不能为空");
        anyhow::ensure!(!self.ssh_password.is_empty(), "SSH 登录密码不能为空");
        anyhow::ensure!(!self.root_user.trim().is_empty(), "root 用户名不能为空");
        anyhow::ensure!(!self.root_password.is_empty(), "root 密码不能为空");
        if let Some(proxy) = &self.http_proxy {
            anyhow::ensure!(!proxy.host.trim().is_empty(), "HTTP 代理地址不能为空");
            anyhow::ensure!(proxy.port > 0, "HTTP 代理端口无效");
        }
        Ok(())
    }
}
