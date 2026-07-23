use super::{ForwardConfig, JumpHost};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ssh2::{ErrorCode, Session};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_LOG_LINES: usize = 500;
const SSH_ERROR_EAGAIN: i32 = -37;

pub struct TunnelHandle {
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    logs: Arc<Mutex<VecDeque<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl TunnelHandle {
    pub fn start(config: ForwardConfig, jump_host: JumpHost) -> Result<Self> {
        config.validate()?;
        jump_host.validate()?;
        let listener = TcpListener::bind(("127.0.0.1", config.local_port))
            .with_context(|| format!("无法监听本地端口 {}", config.local_port))?;
        listener.set_nonblocking(true)?;
        test_connection(&config, &jump_host).context("启动前连通性检查失败")?;
        let stop = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        push_log(&logs, "SSH 登录和目标端口连通性检查成功".into());
        push_log(
            &logs,
            format!(
                "监听 127.0.0.1:{} → {}:{}（经 {}）",
                config.local_port, config.remote_ip, config.remote_port, jump_host.name
            ),
        );
        if config.keep_alive {
            push_log(
                &logs,
                format!(
                    "SSH 保活已启用，心跳间隔 {} 秒",
                    config.keep_alive_interval_secs
                ),
            );
        }

        let thread_stop = stop.clone();
        let thread_running = running.clone();
        let thread_logs = logs.clone();
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        push_log(&thread_logs, format!("收到本地连接：{peer}"));
                        let cfg = config.clone();
                        let host = jump_host.clone();
                        let logs = thread_logs.clone();
                        let stop = thread_stop.clone();
                        thread::spawn(move || {
                            if let Err(error) = forward_connection(stream, &cfg, &host, &stop) {
                                push_log(&logs, format!("连接失败：{error:#}"));
                            } else {
                                push_log(&logs, format!("连接已关闭：{peer}"));
                            }
                        });
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(60));
                    }
                    Err(error) => {
                        push_log(&thread_logs, format!("监听失败：{error}"));
                        break;
                    }
                }
            }
            thread_running.store(false, Ordering::Relaxed);
            push_log(&thread_logs, "转发已停止".into());
        });

        Ok(Self {
            stop,
            running,
            logs,
            worker: Some(worker),
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn logs(&self) -> String {
        self.logs
            .lock()
            .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_else(|_| "日志读取失败".into())
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn test_connection(config: &ForwardConfig, jump_host: &JumpHost) -> Result<()> {
    config.validate()?;
    jump_host.validate()?;
    let session = connect(jump_host)?;
    let mut channel = session
        .channel_direct_tcpip(&config.remote_ip, config.remote_port, None)
        .context("SSH 已登录，但无法连接目标服务")?;
    channel.close().ok();
    Ok(())
}

pub fn test_jump_host_connection(jump_host: &JumpHost) -> Result<()> {
    jump_host.validate()?;
    connect(jump_host)?;
    Ok(())
}

pub fn enable_forwarding(jump_host: &JumpHost) -> Result<String> {
    jump_host.validate()?;
    if jump_host.root_password.is_empty() {
        bail!("root 密码不能为空");
    }
    if !jump_host
        .root_username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("root 用户名包含非法字符");
    }

    // First authenticate as the normal service account, as required by the workflow.
    let session = connect(jump_host)?;
    let mut channel = session.channel_session().context("创建远端会话失败")?;
    channel
        .request_pty("xterm", None, Some((120, 40, 0, 0)))
        .context("申请远端终端失败")?;
    channel.shell().context("打开远端 shell 失败")?;

    writeln!(channel, "su - {}", jump_host.root_username)?;
    channel.flush()?;
    read_until(&mut channel, &["assword", "密码"], Duration::from_secs(10))?;
    writeln!(channel, "{}", jump_host.root_password)?;
    channel.flush()?;
    thread::sleep(Duration::from_millis(500));

    // Keep Match blocks intact: replace global forwarding options only, validate
    // the candidate file, and restore the backup if service restart fails.
    let command = r#"set -eu; C=/etc/ssh/sshd_config; B="$C.s-porter.bak.$(date +%Y%m%d%H%M%S)"; T=$(mktemp /etc/ssh/sshd_config.s-porter.XXXXXX); cp -p "$C" "$B"; awk 'BEGIN{m=0; added=0} /^[[:space:]]*Match[[:space:]]/{if(!added){print "AllowTcpForwarding yes"; print "DisableForwarding no"; print "PermitOpen any"; added=1} m=1} {k=tolower($1)} !m && (k=="allowtcpforwarding" || k=="disableforwarding" || k=="permitopen"){next} {print} END{if(!added){print "AllowTcpForwarding yes"; print "DisableForwarding no"; print "PermitOpen any"}}' "$C" > "$T"; chmod --reference="$C" "$T" 2>/dev/null || chmod 600 "$T"; chown --reference="$C" "$T" 2>/dev/null || true; if ! sshd -t -f "$T"; then rm -f "$T"; echo __S_PORTER_INVALID__; exit 1; fi; mv "$T" "$C"; restart_ssh(){ systemctl restart sshd || systemctl restart ssh || service sshd restart || service ssh restart; }; if ! restart_ssh; then cp -p "$B" "$C"; restart_ssh || true; echo __S_PORTER_ROLLED_BACK__; exit 1; fi; echo __S_PORTER_OK__"#;
    writeln!(channel, "{command}")?;
    channel.flush()?;
    let output = read_until(&mut channel, &["__S_PORTER_OK__"], Duration::from_secs(30))?;
    writeln!(channel, "exit")?;
    channel.close().ok();
    Ok(output)
}

pub(super) fn connect(jump_host: &JumpHost) -> Result<Session> {
    let address = format!("{}:{}", jump_host.host.trim(), jump_host.port);
    let tcp = if let Some(proxy) = &jump_host.http_proxy {
        connect_via_http_proxy(jump_host, proxy)
            .with_context(|| format!("无法通过 HTTP 代理连接 SSH 服务 {address}"))?
    } else {
        connect_address(&address).with_context(|| format!("无法连接 SSH 服务 {address}"))?
    };
    tcp.set_read_timeout(Some(Duration::from_secs(35)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))?;
    let mut session = Session::new().context("初始化 SSH 会话失败")?;
    session.set_tcp_stream(tcp);
    session.handshake().context("SSH 握手失败")?;
    session
        .userauth_password(&jump_host.username, &jump_host.password)
        .with_context(|| format!("SSH 用户 {} 认证失败", jump_host.username))?;
    if !session.authenticated() {
        bail!("SSH 认证未通过");
    }
    Ok(session)
}

fn connect_address(address: &str) -> Result<TcpStream> {
    let addresses = address
        .to_socket_addrs()
        .with_context(|| format!("无法解析地址 {address}"))?;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, Duration::from_secs(8)) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow::anyhow!("地址没有可用的 IP")))
}

fn connect_via_http_proxy(
    jump_host: &JumpHost,
    proxy: &super::HttpProxyConfig,
) -> Result<TcpStream> {
    let proxy_address = format!("{}:{}", proxy.host.trim(), proxy.port);
    let mut tcp = connect_address(&proxy_address)
        .with_context(|| format!("无法连接 HTTP 代理 {proxy_address}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(12)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(12)))?;

    let target = format!("{}:{}", jump_host.host.trim(), jump_host.port);
    let request = http_connect_request(&target, proxy);
    tcp.write_all(request.as_bytes())?;
    tcp.flush()?;

    let mut response = Vec::with_capacity(512);
    let mut byte = [0_u8; 1];
    while response.len() < 16 * 1024 && !response.ends_with(b"\r\n\r\n") {
        tcp.read_exact(&mut byte).context("HTTP 代理响应不完整")?;
        response.push(byte[0]);
    }
    let response = String::from_utf8_lossy(&response);
    let status = response.lines().next().unwrap_or_default();
    if status
        .split_whitespace()
        .nth(1)
        .is_none_or(|code| code != "200")
    {
        bail!("HTTP 代理拒绝 CONNECT 请求：{status}");
    }
    Ok(tcp)
}

fn http_connect_request(target: &str, proxy: &super::HttpProxyConfig) -> String {
    let mut request =
        format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: Keep-Alive\r\n");
    if !proxy.username.is_empty() {
        let credentials = STANDARD.encode(format!("{}:{}", proxy.username, proxy.password));
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("\r\n");
    request
}

fn forward_connection(
    mut local: TcpStream,
    config: &ForwardConfig,
    jump_host: &JumpHost,
    stop: &AtomicBool,
) -> Result<()> {
    let session = connect(jump_host)?;
    let mut remote = session
        .channel_direct_tcpip(&config.remote_ip, config.remote_port, None)
        .context("打开 SSH direct-tcpip 通道失败")?;
    if config.keep_alive {
        session.set_keepalive(true, config.keep_alive_interval_secs);
    }
    session.set_blocking(false);
    local.set_nonblocking(true)?;
    let mut local_buffer = [0_u8; 32 * 1024];
    let mut remote_buffer = [0_u8; 32 * 1024];
    let mut local_eof = false;
    let mut remote_eof = false;
    let mut next_keep_alive =
        Instant::now() + Duration::from_secs(config.keep_alive_interval_secs.into());

    while !(stop.load(Ordering::Relaxed) || local_eof && remote_eof) {
        let mut progressed = false;
        if !local_eof {
            match local.read(&mut local_buffer) {
                Ok(0) => {
                    local_eof = true;
                    remote.send_eof().ok();
                }
                Ok(n) => {
                    write_channel(&mut remote, &local_buffer[..n], stop)?;
                    progressed = true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(error).context("读取本地连接失败"),
            }
        }
        if !remote_eof {
            match remote.read(&mut remote_buffer) {
                Ok(0) => remote_eof = true,
                Ok(n) => {
                    write_socket(&mut local, &remote_buffer[..n], stop)?;
                    progressed = true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(error).context("读取 SSH 通道失败"),
            }
        }
        if config.keep_alive && Instant::now() >= next_keep_alive {
            match session.keepalive_send() {
                Ok(seconds_to_next) => {
                    next_keep_alive =
                        Instant::now() + Duration::from_secs(seconds_to_next.max(1).into());
                }
                Err(error) if error.code() == ErrorCode::Session(SSH_ERROR_EAGAIN) => {
                    next_keep_alive = Instant::now() + Duration::from_millis(50);
                }
                Err(error) => return Err(error).context("发送 SSH 保活心跳失败"),
            }
        }
        if !progressed {
            thread::sleep(Duration::from_millis(3));
        }
    }
    remote.close().ok();
    Ok(())
}

fn write_channel(channel: &mut ssh2::Channel, mut data: &[u8], stop: &AtomicBool) -> Result<()> {
    while !data.is_empty() && !stop.load(Ordering::Relaxed) {
        match channel.write(data) {
            Ok(0) => bail!("SSH 通道已关闭"),
            Ok(n) => data = &data[n..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(3))
            }
            Err(error) => return Err(error).context("写入 SSH 通道失败"),
        }
    }
    Ok(())
}

fn write_socket(socket: &mut TcpStream, mut data: &[u8], stop: &AtomicBool) -> Result<()> {
    while !data.is_empty() && !stop.load(Ordering::Relaxed) {
        match socket.write(data) {
            Ok(0) => bail!("本地连接已关闭"),
            Ok(n) => data = &data[n..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(3))
            }
            Err(error) => return Err(error).context("写入本地连接失败"),
        }
    }
    Ok(())
}

fn read_until(channel: &mut ssh2::Channel, needles: &[&str], timeout: Duration) -> Result<String> {
    let started = Instant::now();
    let mut output = String::new();
    let mut buffer = [0_u8; 2048];
    while started.elapsed() < timeout {
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                output.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if needles.iter().any(|needle| output.contains(needle)) {
                    return Ok(output);
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(30));
            }
            Err(error) if error.kind() == ErrorKind::TimedOut => break,
            Err(error) => return Err(error).context("读取远端输出失败"),
        }
    }
    bail!("等待远端响应超时。远端输出：{}", output.trim())
}

fn push_log(logs: &Arc<Mutex<VecDeque<String>>>, message: String) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() >= MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::HttpProxyConfig;

    #[test]
    fn http_proxy_request_has_connect_target_and_basic_auth() {
        let proxy = HttpProxyConfig {
            host: "127.0.0.1".into(),
            port: 3128,
            username: "alice".into(),
            password: "password".into(),
        };

        let request = http_connect_request("ssh.internal:22", &proxy);
        assert!(request.starts_with("CONNECT ssh.internal:22 HTTP/1.1\r\n"));
        assert!(request.contains("Proxy-Authorization: Basic YWxpY2U6cGFzc3dvcmQ=\r\n"));
    }
}
