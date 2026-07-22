# S Porter

基于 `gpui-component` 的桌面 SSH 本地端口转发与文本工具。

## 功能

- SSH 本地转发：`127.0.0.1:本地端口 → 远程 IP:远程端口`
- 可选 HTTP CONNECT 代理（支持 Basic 代理认证）
- 启动、停止、连接测试、运行日志、克隆和删除
- 多选批量启动、停止和删除
- 使用普通 SSH 账号登录后，通过 `su` 切换 root，安全修改并校验 `sshd_config`
- AES-256-GCM 加解密（Argon2 密钥派生）
- Base64、URL 编解码以及 MD5、SHA-256 摘要

## 运行

```bash
cargo run
```

## 桌面安装包

GitHub Actions 会在拉取请求中验证 macOS 与 Windows 的 release 构建，也可以从 Actions 页面手动运行。推送 `v*` 标签（例如 `v0.1.0`）后，流水线会创建 GitHub Release，并附带：

- `s-porter-macos.zip`：包含 ad-hoc 签名的 `S Porter.app`
- `s-porter-windows.zip`：包含 `s-porter.exe`

当前 macOS 包未使用 Apple Developer ID 公证，首次运行时可能需要在系统安全设置中确认。

## 项目结构

```text
src/
├── main.rs                 # 程序入口
├── forward/               # 端口转发领域
│   ├── model.rs           # 转发配置模型与校验
│   └── ssh.rs             # SSH 连接、sshd 配置和隧道生命周期
├── storage/
│   └── config.rs          # 配置文件持久化
├── toolkit/
│   ├── crypto.rs          # Argon2 + AES-256-GCM
│   └── codec.rs           # Base64、URL、MD5、SHA-256
└── ui/
    ├── app.rs             # UI 状态、事件处理和根视图
    ├── sidebar.rs         # 侧边栏
    ├── forward_page.rs    # 端口转发页面和新增弹窗
    └── tool_page.rs       # 加解密与编解码页面
```

SSH 连接默认直接使用用户名和密码认证，不读取或依赖 `~/.ssh/known_hosts`。这方便连接尚未信任的新服务器，但不会验证服务器主机密钥，无法防御 SSH 中间人攻击；请仅连接可信网络中的正确 IP 地址。

配置保存在操作系统的应用配置目录中。在 Unix 系统上文件权限固定为 `0600`。保存的 SSH 密码供后台转发自动登录使用，请确保本机账户和磁盘受到妥善保护。

“开启允许转发”会在远端执行以下受控流程：

1. 备份 `/etc/ssh/sshd_config`；
2. 仅修改全局 `AllowTcpForwarding`、`DisableForwarding` 和 `PermitOpen`，保留 `Match` 块；
3. 使用 `sshd -t` 校验候选配置；
4. 重启 `sshd`/`ssh` 服务；
5. 若重启失败，恢复备份并再次尝试启动服务。
