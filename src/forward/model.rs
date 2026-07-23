use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JumpHost {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub root_username: String,
    pub root_password: String,
    #[serde(default)]
    pub http_proxy: Option<HttpProxyConfig>,
}

impl JumpHost {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.name.trim().is_empty(), "跳板机名称不能为空");
        anyhow::ensure!(!self.host.trim().is_empty(), "SSH 服务地址不能为空");
        anyhow::ensure!(self.port > 0, "SSH 服务端口无效");
        anyhow::ensure!(!self.username.trim().is_empty(), "SSH 登录用户名不能为空");
        anyhow::ensure!(!self.password.is_empty(), "SSH 登录密码不能为空");
        anyhow::ensure!(!self.root_username.trim().is_empty(), "root 用户名不能为空");
        anyhow::ensure!(!self.root_password.is_empty(), "root 密码不能为空");
        if let Some(proxy) = &self.http_proxy {
            anyhow::ensure!(!proxy.host.trim().is_empty(), "HTTP 代理地址不能为空");
            anyhow::ensure!(proxy.port > 0, "HTTP 代理端口无效");
            anyhow::ensure!(
                proxy.host.trim() != self.password && proxy.host.trim() != self.root_password,
                "HTTP 代理地址误填成了密码，请填写代理服务器主机名或 IP，例如 127.0.0.1"
            );
            anyhow::ensure!(
                !proxy.host.contains("://"),
                "HTTP 代理地址只需填写主机名或 IP，不要包含 http:// 或 https://"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jump_host(proxy_host: &str) -> JumpHost {
        JumpHost {
            id: "host-1".into(),
            name: "test".into(),
            host: "ssh".into(),
            port: 22,
            username: "tester".into(),
            password: "tester123".into(),
            root_username: "root".into(),
            root_password: "root123".into(),
            http_proxy: Some(HttpProxyConfig {
                host: proxy_host.into(),
                port: 8888,
                username: "proxyuser".into(),
                password: "proxypass".into(),
            }),
        }
    }

    #[test]
    fn rejects_password_in_http_proxy_host_field() {
        let error = jump_host("tester123").validate().unwrap_err().to_string();
        assert!(error.contains("误填成了密码"));
    }

    #[test]
    fn accepts_http_proxy_hostname() {
        jump_host("127.0.0.1").validate().unwrap();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardConfig {
    pub id: String,
    pub name: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub jump_host_id: String,
    #[serde(default)]
    pub keep_alive: bool,
    #[serde(default = "default_keep_alive_interval_secs")]
    pub keep_alive_interval_secs: u32,
}

fn default_keep_alive_interval_secs() -> u32 {
    30
}

impl ForwardConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.name.trim().is_empty(), "配置名称不能为空");
        anyhow::ensure!(!self.remote_ip.trim().is_empty(), "远程 IP 不能为空");
        anyhow::ensure!(self.remote_port > 0, "远程端口无效");
        anyhow::ensure!(self.local_port > 0, "本地端口无效");
        anyhow::ensure!(!self.jump_host_id.is_empty(), "请选择跳板机");
        if self.keep_alive {
            anyhow::ensure!(
                (2..=3600).contains(&self.keep_alive_interval_secs),
                "心跳间隔必须在 2–3600 秒之间"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod forward_config_tests {
    use super::*;

    #[test]
    fn old_config_defaults_keep_alive_to_disabled() {
        let config: ForwardConfig = serde_json::from_str(
            r#"{
                "id":"forward-1",
                "name":"database",
                "local_port":5432,
                "remote_ip":"database.internal",
                "remote_port":5432,
                "jump_host_id":"host-1"
            }"#,
        )
        .unwrap();

        assert!(!config.keep_alive);
        assert_eq!(config.keep_alive_interval_secs, 30);
    }

    #[test]
    fn validates_enabled_keep_alive_interval() {
        let config = ForwardConfig {
            id: "forward-1".into(),
            name: "database".into(),
            local_port: 5432,
            remote_ip: "database.internal".into(),
            remote_port: 5432,
            jump_host_id: "host-1".into(),
            keep_alive: true,
            keep_alive_interval_secs: 1,
        };

        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("心跳间隔")
        );
    }
}
