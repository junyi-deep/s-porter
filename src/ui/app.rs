use super::{forward_page, sidebar, time_page, tool_page};
use crate::{
    forward::{self, ForwardConfig, HttpProxyConfig},
    storage, toolkit,
};
use gpui::*;
use gpui_component::{
    input::{InputEvent, InputState},
    text::TextView,
    *,
};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Forward,
    Crypto,
    Codec,
    Time,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum ForwardState {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ForwardStatusFilter {
    All,
    Running,
    Stopped,
    Failed,
}

pub(super) struct ForwardForm {
    pub(super) name: Entity<InputState>,
    pub(super) local_port: Entity<InputState>,
    pub(super) remote_ip: Entity<InputState>,
    pub(super) remote_port: Entity<InputState>,
    pub(super) ssh_ip: Entity<InputState>,
    pub(super) ssh_port: Entity<InputState>,
    pub(super) ssh_user: Entity<InputState>,
    pub(super) ssh_password: Entity<InputState>,
    pub(super) root_user: Entity<InputState>,
    pub(super) root_password: Entity<InputState>,
    pub(super) proxy_host: Entity<InputState>,
    pub(super) proxy_port: Entity<InputState>,
    pub(super) proxy_username: Entity<InputState>,
    pub(super) proxy_password: Entity<InputState>,
}

impl ForwardForm {
    fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
        let mut input =
            |value: &'static str, placeholder: &'static str, cx: &mut Context<AppView>| {
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
            ssh_ip: input("", "SSH 堡垒机 IP 或域名", cx),
            ssh_port: input("22", "SSH 端口", cx),
            ssh_user: input("paas", "SSH 登录用户名", cx),
            ssh_password: input("", "SSH 登录密码", cx),
            root_user: input("root", "提权用户名", cx),
            root_password: input("", "root 密码", cx),
            proxy_host: input("", "可选，例如 127.0.0.1", cx),
            proxy_port: input("", "代理端口", cx),
            proxy_username: input("", "可选", cx),
            proxy_password: input("", "可选", cx),
        }
    }
}

pub(super) struct ToolInputs {
    pub(super) source: Entity<InputState>,
    pub(super) result: Entity<InputState>,
    pub(super) password: Entity<InputState>,
}

impl ToolInputs {
    fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
        Self {
            source: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("在此输入待处理内容")
            }),
            result: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("处理结果")
            }),
            password: cx.new(|cx| InputState::new(window, cx).placeholder("加解密密码")),
        }
    }
}

pub(super) struct AppView {
    pub(super) page: Page,
    pub(super) forwards: Vec<ForwardConfig>,
    pub(super) tunnels: HashMap<String, forward::TunnelHandle>,
    pub(super) form: ForwardForm,
    pub(super) crypto_tools: ToolInputs,
    pub(super) codec_tools: ToolInputs,
    pub(super) time_tools: time_page::TimeToolState,
    pub(super) forward_search: Entity<InputState>,
    pub(super) forward_status_filter: ForwardStatusFilter,
    pub(super) forward_states: HashMap<String, ForwardState>,
    pub(super) startup_logs: HashMap<String, Vec<String>>,
    pub(super) selected: HashSet<String>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let forward_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("搜索名称、端口、远程目标或 SSH 服务")
        });
        let subscriptions = vec![cx.subscribe(&forward_search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        let view = Self {
            page: Page::Forward,
            forwards: storage::load().unwrap_or_default(),
            tunnels: HashMap::new(),
            form: ForwardForm::new(window, cx),
            crypto_tools: ToolInputs::new(window, cx),
            codec_tools: ToolInputs::new(window, cx),
            time_tools: time_page::TimeToolState::new(window, cx),
            forward_search,
            forward_status_filter: ForwardStatusFilter::All,
            forward_states: HashMap::new(),
            startup_logs: HashMap::new(),
            selected: HashSet::new(),
            busy: false,
            _subscriptions: subscriptions,
        };
        cx.spawn_in(window, async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if weak
                    .update_in(cx, |this, window, cx| this.tick_time_tools(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }

    fn form_config(&self, cx: &App) -> anyhow::Result<ForwardConfig> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let name = value(&self.form.name);
        let remote_ip = value(&self.form.remote_ip);
        let ssh_ip = value(&self.form.ssh_ip);
        let local_port = value(&self.form.local_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("本地端口必须是 1–65535 的数字"))?;
        let remote_port = value(&self.form.remote_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("远程端口必须是 1–65535 的数字"))?;
        let ssh_port = value(&self.form.ssh_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("SSH 端口必须是 1–65535 的数字"))?;
        let proxy_host = value(&self.form.proxy_host);
        let http_proxy = if proxy_host.trim().is_empty() {
            None
        } else {
            Some(HttpProxyConfig {
                host: proxy_host,
                port: value(&self.form.proxy_port)
                    .parse::<u16>()
                    .map_err(|_| anyhow::anyhow!("HTTP 代理端口必须是 1–65535 的数字"))?,
                username: value(&self.form.proxy_username),
                password: value(&self.form.proxy_password),
            })
        };
        let config = ForwardConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            local_port,
            remote_ip,
            remote_port,
            ssh_ip,
            ssh_port,
            ssh_user: value(&self.form.ssh_user),
            ssh_password: value(&self.form.ssh_password),
            root_user: value(&self.form.root_user),
            root_password: value(&self.form.root_password),
            http_proxy,
        };
        config.validate()?;
        Ok(config)
    }

    pub(super) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.form_config(cx) {
            Ok(item) => {
                self.forwards.push(item);
                if let Err(error) = storage::save(&self.forwards) {
                    self.forwards.pop();
                    window.push_notification(format!("保存失败：{error:#}"), cx);
                    return false;
                }
                cx.notify();
                true
            }
            Err(error) => {
                window.push_notification(error.to_string(), cx);
                false
            }
        }
    }

    fn set_form_value(
        input: &Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    pub(super) fn prepare_new_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = [
            (&self.form.name, ""),
            (&self.form.local_port, "8080"),
            (&self.form.remote_ip, ""),
            (&self.form.remote_port, ""),
            (&self.form.ssh_ip, ""),
            (&self.form.ssh_port, "22"),
            (&self.form.ssh_user, "paas"),
            (&self.form.ssh_password, ""),
            (&self.form.root_user, "root"),
            (&self.form.root_password, ""),
            (&self.form.proxy_host, ""),
            (&self.form.proxy_port, ""),
            (&self.form.proxy_username, ""),
            (&self.form.proxy_password, ""),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
    }

    pub(super) fn prepare_clone_form(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(item) = self.forwards.iter().find(|item| item.id == id).cloned() else {
            return false;
        };
        let proxy = item.http_proxy.clone().unwrap_or_default();
        let values = [
            (&self.form.name, format!("{}_copy", item.name)),
            (&self.form.local_port, item.local_port.to_string()),
            (&self.form.remote_ip, item.remote_ip),
            (&self.form.remote_port, item.remote_port.to_string()),
            (&self.form.ssh_ip, item.ssh_ip),
            (&self.form.ssh_port, item.ssh_port.to_string()),
            (&self.form.ssh_user, item.ssh_user),
            (&self.form.ssh_password, item.ssh_password),
            (&self.form.root_user, item.root_user),
            (&self.form.root_password, item.root_password),
            (&self.form.proxy_host, proxy.host),
            (
                &self.form.proxy_port,
                if proxy.port == 0 {
                    String::new()
                } else {
                    proxy.port.to_string()
                },
            ),
            (&self.form.proxy_username, proxy.username),
            (&self.form.proxy_password, proxy.password),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        true
    }

    pub(super) fn toggle_selected(&mut self, id: &str, selected: bool, cx: &mut Context<Self>) {
        if selected {
            self.selected.insert(id.to_string());
        } else {
            self.selected.remove(id);
        }
        cx.notify();
    }

    pub(super) fn select_ids(&mut self, ids: &[String], selected: bool, cx: &mut Context<Self>) {
        if selected {
            self.selected.extend(ids.iter().cloned());
        } else {
            for id in ids {
                self.selected.remove(id);
            }
        }
        cx.notify();
    }

    pub(super) fn start_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected.iter().cloned().collect::<Vec<_>>();
        for id in ids {
            self.start_tunnel(&id, window, cx);
        }
    }

    pub(super) fn stop_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected.iter().cloned().collect::<Vec<_>>();
        for id in ids {
            self.stop_tunnel(&id, window, cx);
        }
    }

    pub(super) fn delete_configs(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            window.push_notification("请先选择要删除的配置", cx);
            return;
        }
        for id in &ids {
            if let Some(mut handle) = self.tunnels.remove(id) {
                handle.stop();
            }
            self.forward_states.remove(id);
            self.startup_logs.remove(id);
            self.selected.remove(id);
        }
        self.forwards.retain(|item| !ids.contains(&item.id));
        match storage::save(&self.forwards) {
            Ok(()) => window.push_notification(format!("已删除 {} 个转发配置", ids.len()), cx),
            Err(error) => {
                window.push_notification(format!("配置已删除，但保存失败：{error:#}"), cx)
            }
        }
        cx.notify();
    }

    pub(super) fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_configs(self.selected.iter().cloned().collect(), window, cx);
    }

    pub(super) fn start_tunnel(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.forward_states.get(id),
            Some(ForwardState::Starting | ForwardState::Running)
        ) {
            window.push_notification("该转发已在运行", cx);
            return;
        }
        let Some(item) = self.forwards.iter().find(|item| item.id == id).cloned() else {
            return;
        };
        let id = id.to_string();
        self.forward_states
            .insert(id.clone(), ForwardState::Starting);
        self.startup_logs
            .entry(id.clone())
            .or_default()
            .push("开始启动：检查本地端口、SSH 认证和远程目标连通性".into());
        window.push_notification(format!("{} 正在启动", item.name), cx);
        cx.notify();

        cx.spawn_in(window, async move |weak, cx| {
            let name = item.name.clone();
            let result = cx
                .background_executor()
                .spawn(async move { forward::TunnelHandle::start(item) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                if !this.forwards.iter().any(|item| item.id == id) {
                    if let Ok(mut handle) = result {
                        handle.stop();
                    }
                    return;
                }
                match result {
                    Ok(handle) => {
                        this.tunnels.insert(id.clone(), handle);
                        this.forward_states
                            .insert(id.clone(), ForwardState::Running);
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push("启动成功：本地监听已就绪".into());
                        window.push_notification(format!("{} 启动成功", name), cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.forward_states
                            .insert(id.clone(), ForwardState::Failed(message.clone()));
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("启动失败：{message}"));
                        window.push_notification(format!("{} 启动失败：{}", name, message), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn stop_tunnel(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut handle) = self.tunnels.remove(id) {
            handle.stop();
            self.forward_states
                .insert(id.to_string(), ForwardState::Stopped);
            self.startup_logs
                .entry(id.to_string())
                .or_default()
                .push("转发已停止并清理本地监听".into());
            window.push_notification("端口转发已停止", cx);
            cx.notify();
        } else if matches!(self.forward_states.get(id), Some(ForwardState::Failed(_))) {
            self.forward_states
                .insert(id.to_string(), ForwardState::Stopped);
            cx.notify();
        } else {
            window.push_notification("该转发当前未运行", cx);
        }
    }

    pub(super) fn show_logs(&self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut logs = self
            .startup_logs
            .get(id)
            .cloned()
            .unwrap_or_default()
            .join("\n");
        let runtime_logs = self
            .tunnels
            .get(id)
            .map(|handle| handle.logs())
            .unwrap_or_default();
        if !runtime_logs.is_empty() {
            if !logs.is_empty() {
                logs.push('\n');
            }
            logs.push_str(&runtime_logs);
        }
        if let Some(ForwardState::Failed(error)) = self.forward_states.get(id)
            && !logs.contains(error)
        {
            if !logs.is_empty() {
                logs.push('\n');
            }
            logs.push_str(&format!("最近一次错误：{error}"));
        }
        if logs.is_empty() {
            logs = "该转发尚无运行日志。".into();
        }
        let log_view_id = format!("forward-log-{id}");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("转发日志").w(px(680.)).min_h(px(280.)).child(
                div().max_h(px(520.)).overflow_hidden().child(
                    TextView::markdown(log_view_id.clone(), format!("```text\n{logs}\n```"))
                        .selectable(true),
                ),
            )
        });
    }

    pub(super) fn run_ssh_operation(
        &mut self,
        item: ForwardConfig,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            window.push_notification("已有 SSH 操作正在执行", cx);
            return;
        }
        self.busy = true;
        let operation = if enable {
            "开启允许转发"
        } else {
            "测试连接"
        };
        let id = item.id.clone();
        self.startup_logs
            .entry(id.clone())
            .or_default()
            .push(format!(
                "{operation}：开始连接 SSH 服务 {}:{}",
                item.ssh_ip, item.ssh_port
            ));
        window.push_notification(
            if enable {
                "正在配置远端 sshd"
            } else {
                "正在测试 SSH 与目标端口"
            },
            cx,
        );
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if enable {
                        forward::enable_forwarding(&item)
                            .map(|_| "远端已允许 TCP 转发，sshd 已重启".to_string())
                    } else {
                        forward::test_connection(&item)
                            .map(|_| "测试成功：SSH 登录及目标端口均可访问".to_string())
                    }
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(message) => {
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("{operation}成功：{message}"));
                        window.push_notification(message.clone(), cx);
                    }
                    Err(error) => {
                        let message = format!("操作失败：{error:#}");
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("{operation}失败：{error:#}"));
                        window.push_notification(message.clone(), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn run_form_ssh(
        &mut self,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.form_config(cx) {
            Ok(item) => self.run_ssh_operation(item, enable, window, cx),
            Err(error) => window.push_notification(error.to_string(), cx),
        }
    }

    fn set_tool_result(
        result: anyhow::Result<String>,
        output: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = result.unwrap_or_else(|error| format!("错误：{error:#}"));
        output.update(cx, |state, cx| state.set_value(text, window, cx));
    }

    pub(super) fn run_codec(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self.codec_tools.source.read(cx).value().to_string();
        let result = match action {
            "b64e" => Ok(toolkit::base64_encode(&source)),
            "b64d" => toolkit::base64_decode(&source),
            "urle" => Ok(toolkit::url_encode(&source)),
            "urld" => toolkit::url_decode(&source),
            "md5" => Ok(toolkit::md5_digest(&source)),
            "sha256" => Ok(toolkit::sha256_digest(&source)),
            _ => Err(anyhow::anyhow!("未知操作")),
        };
        Self::set_tool_result(result, self.codec_tools.result.clone(), window, cx);
    }

    pub(super) fn run_crypto(
        &mut self,
        decrypt: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self.crypto_tools.source.read(cx).value().to_string();
        let password = self.crypto_tools.password.read(cx).value().to_string();
        let result = if decrypt {
            toolkit::decrypt(&source, &password)
        } else {
            toolkit::encrypt(&source, &password)
        };
        Self::set_tool_result(result, self.crypto_tools.result.clone(), window, cx);
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new().child(h_flex().w_full().px_3().child("S Porter")))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar::render(self, cx))
                    .child(div().flex_1().h_full().min_w_0().child(match self.page {
                        Page::Forward => forward_page::render(self, cx),
                        Page::Crypto => tool_page::render(self, true, cx),
                        Page::Codec => tool_page::render(self, false, cx),
                        Page::Time => time_page::render(self, cx),
                    })),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
