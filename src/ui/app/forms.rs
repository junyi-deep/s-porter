//! 服务器与端口转发表单状态。

use gpui::*;
use gpui_component::input::{InputState, TabSize};

pub(in crate::ui) struct ForwardForm {
    pub(in crate::ui) name: Entity<InputState>,
    pub(in crate::ui) local_port: Entity<InputState>,
    pub(in crate::ui) remote_ip: Entity<InputState>,
    pub(in crate::ui) remote_port: Entity<InputState>,
    pub(in crate::ui) keep_alive_interval: Entity<InputState>,
}

impl ForwardForm {
    pub(in crate::ui) fn new(window: &mut Window, cx: &mut App) -> Self {
        let mut input = |value: &'static str, placeholder: &'static str, cx: &mut App| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            })
        };
        Self {
            name: input("", "例如：测试环境数据库", cx),
            local_port: input("8080", "本地监听端口", cx),
            remote_ip: input("", "目标服务 IP 或域名", cx),
            remote_port: input("", "目标服务端口", cx),
            keep_alive_interval: input("30", "2–3600 秒", cx),
        }
    }
}

pub(in crate::ui) struct JumpHostForm {
    pub(in crate::ui) name: Entity<InputState>,
    pub(in crate::ui) host: Entity<InputState>,
    pub(in crate::ui) batch_entries: Entity<InputState>,
    pub(in crate::ui) batch_separator: Entity<InputState>,
    pub(in crate::ui) port: Entity<InputState>,
    pub(in crate::ui) username: Entity<InputState>,
    pub(in crate::ui) password: Entity<InputState>,
    pub(in crate::ui) root_username: Entity<InputState>,
    pub(in crate::ui) root_password: Entity<InputState>,
    pub(in crate::ui) proxy_host: Entity<InputState>,
    pub(in crate::ui) proxy_port: Entity<InputState>,
    pub(in crate::ui) proxy_username: Entity<InputState>,
    pub(in crate::ui) proxy_password: Entity<InputState>,
}

impl JumpHostForm {
    pub(in crate::ui) fn new(window: &mut Window, cx: &mut App) -> Self {
        let batch_entries = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .tab_size(TabSize {
                    tab_size: 4,
                    hard_tabs: true,
                })
                .placeholder(
                    "每行一台：服务器名称, SSH地址\n例如：\n生产-01, 10.0.0.11\n生产-02, 10.0.0.12",
                )
        });
        let batch_separator = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("可选，例如：;（留空则自动识别逗号、Tab 或空格）")
        });
        let mut input = |value: &'static str, placeholder: &'static str, cx: &mut App| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            })
        };
        Self {
            name: input("", "例如：生产环境服务器", cx),
            host: input("", "SSH 服务器 IP 或域名", cx),
            batch_entries,
            batch_separator,
            port: input("22", "SSH 端口", cx),
            username: input("paas", "SSH 登录用户名", cx),
            password: input("", "SSH 登录密码", cx),
            root_username: input("root", "root 用户名", cx),
            root_password: input("", "root 密码", cx),
            proxy_host: input("", "例如 127.0.0.1，请勿填写用户名或密码", cx),
            proxy_port: input("", "代理端口", cx),
            proxy_username: input("", "可选", cx),
            proxy_password: input("", "可选", cx),
        }
    }
}
