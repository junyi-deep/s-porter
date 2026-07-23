use super::{
    format_page, forward_page, jump_host_page, message_center, sidebar, ssh_page, time_page,
    tool_page,
};
use crate::{
    forward::{self, ForwardConfig, HttpProxyConfig, JumpHost},
    storage, toolkit,
};
use chrono::Local;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogFooter},
    input::{CompletionProvider, Input, InputEvent, InputState, Rope, RopeExt},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
    table::TableState,
    text::TextView,
    *,
};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    TextEdit,
};
use std::time::Duration;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
};

const DEFAULT_UI_FONT_SIZE: f32 = 14.;
pub(super) const UI_FONT_SIZES: [u8; 15] =
    [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    JumpHosts,
    Ssh,
    Forward,
    Crypto,
    Codec,
    Format,
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
    pub(super) keep_alive_interval: Entity<InputState>,
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
            keep_alive_interval: input("30", "2–3600 秒", cx),
        }
    }
}

pub(super) struct JumpHostForm {
    pub(super) name: Entity<InputState>,
    pub(super) host: Entity<InputState>,
    pub(super) port: Entity<InputState>,
    pub(super) username: Entity<InputState>,
    pub(super) password: Entity<InputState>,
    pub(super) root_username: Entity<InputState>,
    pub(super) root_password: Entity<InputState>,
    pub(super) proxy_host: Entity<InputState>,
    pub(super) proxy_port: Entity<InputState>,
    pub(super) proxy_username: Entity<InputState>,
    pub(super) proxy_password: Entity<InputState>,
}

impl JumpHostForm {
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
            name: input("", "例如：生产环境跳板机", cx),
            host: input("", "SSH 服务器 IP 或域名", cx),
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

#[derive(Clone, PartialEq, Eq)]
pub(super) enum SshConnectionState {
    Connecting,
    Connected,
    Failed(String),
}

pub(super) struct SshTab {
    pub(super) id: String,
    pub(super) jump_host_id: String,
    pub(super) title: String,
    pub(super) command: Entity<InputState>,
    pub(super) state: SshConnectionState,
    pub(super) terminal: Option<forward::SshTerminalHandle>,
    pub(super) file_panel_open: bool,
    pub(super) remote_path: String,
    pub(super) remote_path_input: Entity<InputState>,
    pub(super) remote_entries: Vec<forward::RemoteEntry>,
    pub(super) file_loading: bool,
    pub(super) file_error: Option<String>,
    pub(super) show_file_time: bool,
    pub(super) show_file_size: bool,
    pub(super) show_file_permissions: bool,
    pub(super) terminal_font_size: Option<f32>,
}

pub(super) struct ToolInputs {
    pub(super) source: Entity<InputState>,
    pub(super) result: Entity<InputState>,
    pub(super) password: Entity<InputState>,
}

impl ToolInputs {
    pub(super) fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
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

#[derive(Clone)]
pub(super) struct AppMessage {
    pub(super) id: String,
    pub(super) created_at: String,
    pub(super) text: String,
}

#[derive(Clone)]
struct CommandHistoryProvider {
    history: Rc<RefCell<Vec<String>>>,
}

fn remember_command(history: &mut Vec<String>, command: &str) {
    history.retain(|existing| existing != command);
    history.insert(0, command.to_string());
    history.truncate(500);
}

impl CompletionProvider for CommandHistoryProvider {
    fn completions(
        &self,
        text: &Rope,
        _: usize,
        _: CompletionContext,
        _: &mut Window,
        _: &mut Context<InputState>,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let query = text.to_string();
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        }
        let range = lsp_types::Range::new(
            text.offset_to_position(0),
            text.offset_to_position(text.len()),
        );
        let items = self
            .history
            .borrow()
            .iter()
            .filter(|command| command.to_lowercase().contains(&query))
            .take(12)
            .map(|command| CompletionItem {
                label: command.replace('\n', " ↵ "),
                kind: Some(CompletionItemKind::TEXT),
                detail: Some("历史命令".into()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: command.clone(),
                })),
                ..Default::default()
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut Context<InputState>) -> bool {
        !new_text.is_empty()
    }
}

pub(super) struct AppView {
    pub(super) page: Page,
    pub(super) sidebar_collapsed: bool,
    ui_font_size: f32,
    pub(super) jump_hosts: Vec<JumpHost>,
    pub(super) forwards: Vec<ForwardConfig>,
    pub(super) tunnels: HashMap<String, forward::TunnelHandle>,
    pub(super) form: ForwardForm,
    pub(super) form_keep_alive: bool,
    pub(super) editing_forward_id: Option<String>,
    pub(super) selected_jump_host_id: Option<String>,
    pub(super) forward_host_picker_search: Entity<InputState>,
    pub(super) jump_host_form: JumpHostForm,
    pub(super) jump_host_form_error: Option<String>,
    pub(super) editing_jump_host_id: Option<String>,
    pub(super) jump_host_search: Entity<InputState>,
    pub(super) ssh_host_picker_search: Entity<InputState>,
    pub(super) jump_host_table: Entity<TableState<jump_host_page::JumpHostTableDelegate>>,
    pub(super) forward_table: Entity<TableState<forward_page::ForwardTableDelegate>>,
    pub(super) ssh_tabs: Vec<SshTab>,
    pub(super) active_ssh_tab_id: Option<String>,
    pub(super) quick_commands: Vec<storage::QuickCommand>,
    pub(super) command_history: Vec<String>,
    command_history_store: Rc<RefCell<Vec<String>>>,
    pub(super) crypto_tools: ToolInputs,
    pub(super) codec_tools: ToolInputs,
    pub(super) format_tools: format_page::FormatToolState,
    pub(super) time_tools: time_page::TimeToolState,
    pub(super) forward_search: Entity<InputState>,
    pub(super) message_search: Entity<InputState>,
    pub(super) messages: VecDeque<AppMessage>,
    pub(super) forward_status_filter: ForwardStatusFilter,
    pub(super) forward_states: HashMap<String, ForwardState>,
    pub(super) startup_logs: HashMap<String, Vec<String>>,
    pub(super) selected: HashSet<String>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Theme::global_mut(cx).font_size = px(DEFAULT_UI_FONT_SIZE);
        window.set_rem_size(px(DEFAULT_UI_FONT_SIZE));
        let forward_search = cx
            .new(|cx| InputState::new(window, cx).placeholder("搜索名称、端口、远程目标或跳板机"));
        let jump_host_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称、地址或登录用户"));
        let forward_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索跳板机"));
        let ssh_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索跳板机"));
        let message_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索最近 100 条消息"));
        let app_view = cx.entity();
        let jump_host_table = cx.new(|cx| {
            TableState::new(
                jump_host_page::JumpHostTableDelegate::new(app_view.clone()),
                window,
                cx,
            )
            .sortable(false)
            .col_movable(false)
            .col_selectable(false)
            .row_selectable(false)
        });
        let forward_table = cx.new(|cx| {
            TableState::new(
                forward_page::ForwardTableDelegate::new(app_view.clone()),
                window,
                cx,
            )
            .sortable(false)
            .col_movable(false)
            .col_selectable(false)
            .row_selectable(false)
        });
        let subscriptions = vec![
            cx.subscribe(&forward_search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&jump_host_search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&forward_host_picker_search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&ssh_host_picker_search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&message_search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let config = storage::load().unwrap_or_default();
        let selected_jump_host_id = config.jump_hosts.first().map(|host| host.id.clone());
        let command_history = config
            .command_history
            .into_iter()
            .take(500)
            .collect::<Vec<_>>();
        let command_history_store = Rc::new(RefCell::new(command_history.clone()));
        let view = Self {
            page: Page::JumpHosts,
            sidebar_collapsed: false,
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            jump_hosts: config.jump_hosts,
            forwards: config.forwards,
            tunnels: HashMap::new(),
            form: ForwardForm::new(window, cx),
            form_keep_alive: false,
            editing_forward_id: None,
            selected_jump_host_id,
            forward_host_picker_search,
            jump_host_form: JumpHostForm::new(window, cx),
            jump_host_form_error: None,
            editing_jump_host_id: None,
            jump_host_search,
            ssh_host_picker_search,
            jump_host_table,
            forward_table,
            ssh_tabs: Vec::new(),
            active_ssh_tab_id: None,
            quick_commands: config.quick_commands,
            command_history,
            command_history_store,
            crypto_tools: ToolInputs::new(window, cx),
            codec_tools: ToolInputs::new(window, cx),
            format_tools: format_page::FormatToolState::new(window, cx),
            time_tools: time_page::TimeToolState::new(window, cx),
            forward_search,
            message_search,
            messages: VecDeque::new(),
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
                    .update_in(cx, |this, window, cx| {
                        this.tick_time_tools(window, cx);
                        if this.page == Page::Ssh {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }

    pub(super) fn push_message(
        &mut self,
        message: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = message.into();
        let id = uuid::Uuid::new_v4().to_string();
        if self.messages.len() >= 100 {
            self.messages.pop_front();
        }
        self.messages.push_back(AppMessage {
            id: id.clone(),
            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            text: text.clone(),
        });
        window.push_notification(
            Notification::new().content(move |_, _, _| {
                TextView::markdown(format!("notification-{id}"), text.clone())
                    .selectable(true)
                    .into_any_element()
            }),
            cx,
        );
        cx.notify();
    }

    fn set_ui_font_size(&mut self, font_size: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.ui_font_size = font_size;
        Theme::global_mut(cx).font_size = px(font_size);
        window.set_rem_size(px(font_size));
        window.refresh();
        cx.notify();
    }

    fn form_config(&self, cx: &App) -> anyhow::Result<ForwardConfig> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let name = value(&self.form.name);
        let remote_ip = value(&self.form.remote_ip);
        let local_port = value(&self.form.local_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("本地端口必须是 1–65535 的数字"))?;
        let remote_port = value(&self.form.remote_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("远程端口必须是 1–65535 的数字"))?;
        let keep_alive_interval_secs = if self.form_keep_alive {
            value(&self.form.keep_alive_interval)
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("心跳间隔必须是 2–3600 的数字"))?
        } else {
            30
        };
        let config = ForwardConfig {
            id: self
                .editing_forward_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name,
            local_port,
            remote_ip,
            remote_port,
            jump_host_id: self
                .selected_jump_host_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("请先新增并选择跳板机"))?,
            keep_alive: self.form_keep_alive,
            keep_alive_interval_secs,
        };
        config.validate()?;
        anyhow::ensure!(
            self.jump_hosts
                .iter()
                .any(|host| host.id == config.jump_host_id),
            "选择的跳板机不存在"
        );
        Ok(config)
    }

    fn app_config(&self) -> storage::AppConfig {
        storage::AppConfig {
            jump_hosts: self.jump_hosts.clone(),
            forwards: self.forwards.clone(),
            quick_commands: self.quick_commands.clone(),
            command_history: self.command_history.clone(),
        }
    }

    fn persist(&self) -> anyhow::Result<()> {
        storage::save(&self.app_config())
    }

    pub(super) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.form_config(cx) {
            Ok(item) => {
                let existing = self
                    .editing_forward_id
                    .as_ref()
                    .and_then(|id| self.forwards.iter().position(|config| &config.id == id));
                if existing.is_some_and(|index| self.tunnels.contains_key(&self.forwards[index].id))
                {
                    self.push_message("请先停止转发，再编辑配置", window, cx);
                    return false;
                }
                let previous = existing
                    .map(|index| std::mem::replace(&mut self.forwards[index], item.clone()));
                if existing.is_none() {
                    self.forwards.push(item);
                }
                if let Err(error) = self.persist() {
                    if let Some(index) = existing {
                        self.forwards[index] = previous.expect("编辑配置必须存在旧值");
                    } else {
                        self.forwards.pop();
                    }
                    self.push_message(format!("保存失败：{error:#}"), window, cx);
                    return false;
                }
                self.editing_forward_id = None;
                cx.notify();
                true
            }
            Err(error) => {
                self.push_message(error.to_string(), window, cx);
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
        self.editing_forward_id = None;
        self.form_keep_alive = false;
        Self::set_form_value(&self.forward_host_picker_search, "", window, cx);
        let values = [
            (&self.form.name, ""),
            (&self.form.local_port, "8080"),
            (&self.form.remote_ip, ""),
            (&self.form.remote_port, ""),
            (&self.form.keep_alive_interval, "30"),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.selected_jump_host_id = self.jump_hosts.first().map(|host| host.id.clone());
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
        self.editing_forward_id = None;
        self.form_keep_alive = item.keep_alive;
        Self::set_form_value(&self.forward_host_picker_search, "", window, cx);
        let values = [
            (&self.form.name, format!("{}_copy", item.name)),
            (&self.form.local_port, item.local_port.to_string()),
            (&self.form.remote_ip, item.remote_ip),
            (&self.form.remote_port, item.remote_port.to_string()),
            (
                &self.form.keep_alive_interval,
                item.keep_alive_interval_secs.to_string(),
            ),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.selected_jump_host_id = Some(item.jump_host_id);
        true
    }

    pub(super) fn prepare_edit_forward_form(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(item) = self.forwards.iter().find(|item| item.id == id).cloned() else {
            return false;
        };
        if self.tunnels.contains_key(id) {
            self.push_message("请先停止转发，再编辑配置", window, cx);
            return false;
        }
        self.editing_forward_id = Some(item.id);
        self.form_keep_alive = item.keep_alive;
        Self::set_form_value(&self.forward_host_picker_search, "", window, cx);
        let values = [
            (&self.form.name, item.name),
            (&self.form.local_port, item.local_port.to_string()),
            (&self.form.remote_ip, item.remote_ip),
            (&self.form.remote_port, item.remote_port.to_string()),
            (
                &self.form.keep_alive_interval,
                item.keep_alive_interval_secs.to_string(),
            ),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.selected_jump_host_id = Some(item.jump_host_id);
        true
    }

    pub(super) fn set_form_keep_alive(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.form_keep_alive = enabled;
        cx.notify();
    }

    pub(super) fn select_forward_jump_host(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_jump_host_id = Some(id);
        cx.notify();
    }

    pub(super) fn clear_ssh_host_picker_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::set_form_value(&self.ssh_host_picker_search, "", window, cx);
    }

    pub(super) fn prepare_new_jump_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_jump_host_id = None;
        self.jump_host_form_error = None;
        let values = [
            (&self.jump_host_form.name, ""),
            (&self.jump_host_form.host, ""),
            (&self.jump_host_form.port, "22"),
            (&self.jump_host_form.username, "paas"),
            (&self.jump_host_form.password, ""),
            (&self.jump_host_form.root_username, "root"),
            (&self.jump_host_form.root_password, ""),
            (&self.jump_host_form.proxy_host, ""),
            (&self.jump_host_form.proxy_port, ""),
            (&self.jump_host_form.proxy_username, ""),
            (&self.jump_host_form.proxy_password, ""),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
    }

    pub(super) fn prepare_edit_jump_host(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(host) = self.jump_hosts.iter().find(|host| host.id == id).cloned() else {
            return false;
        };
        let proxy = host.http_proxy.unwrap_or_default();
        self.editing_jump_host_id = Some(host.id);
        self.jump_host_form_error = None;
        let values = [
            (&self.jump_host_form.name, host.name),
            (&self.jump_host_form.host, host.host),
            (&self.jump_host_form.port, host.port.to_string()),
            (&self.jump_host_form.username, host.username),
            (&self.jump_host_form.password, host.password),
            (&self.jump_host_form.root_username, host.root_username),
            (&self.jump_host_form.root_password, host.root_password),
            (&self.jump_host_form.proxy_host, proxy.host),
            (
                &self.jump_host_form.proxy_port,
                if proxy.port > 0 {
                    proxy.port.to_string()
                } else {
                    String::new()
                },
            ),
            (&self.jump_host_form.proxy_username, proxy.username),
            (&self.jump_host_form.proxy_password, proxy.password),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        true
    }

    fn jump_host_form_value(&self, cx: &App) -> anyhow::Result<JumpHost> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let proxy_host = value(&self.jump_host_form.proxy_host);
        let http_proxy = if proxy_host.trim().is_empty() {
            None
        } else {
            Some(HttpProxyConfig {
                host: proxy_host,
                port: value(&self.jump_host_form.proxy_port)
                    .parse()
                    .map_err(|_| anyhow::anyhow!("HTTP 代理端口必须是 1–65535 的数字"))?,
                username: value(&self.jump_host_form.proxy_username),
                password: value(&self.jump_host_form.proxy_password),
            })
        };
        let host = JumpHost {
            id: self
                .editing_jump_host_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: value(&self.jump_host_form.name),
            host: value(&self.jump_host_form.host),
            port: value(&self.jump_host_form.port)
                .parse()
                .map_err(|_| anyhow::anyhow!("SSH 端口必须是 1–65535 的数字"))?,
            username: value(&self.jump_host_form.username),
            password: value(&self.jump_host_form.password),
            root_username: value(&self.jump_host_form.root_username),
            root_password: value(&self.jump_host_form.root_password),
            http_proxy,
        };
        host.validate()?;
        Ok(host)
    }

    pub(super) fn save_jump_host(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let host = match self.jump_host_form_value(cx) {
            Ok(host) => host,
            Err(error) => {
                let message = error.to_string();
                self.jump_host_form_error = Some(message.clone());
                self.push_message(message, window, cx);
                return false;
            }
        };
        let mut next = self.jump_hosts.clone();
        if let Some(existing) = next.iter_mut().find(|item| item.id == host.id) {
            *existing = host.clone();
        } else {
            next.push(host.clone());
        }
        let config = storage::AppConfig {
            jump_hosts: next.clone(),
            forwards: self.forwards.clone(),
            quick_commands: self.quick_commands.clone(),
            command_history: self.command_history.clone(),
        };
        if let Err(error) = storage::save(&config) {
            let message = format!("保存失败：{error:#}");
            self.jump_host_form_error = Some(message.clone());
            self.push_message(message, window, cx);
            return false;
        }
        self.jump_host_form_error = None;
        self.jump_hosts = next;
        self.selected_jump_host_id.get_or_insert(host.id);
        self.push_message("跳板机配置已保存", window, cx);
        cx.notify();
        true
    }

    pub(super) fn test_jump_host_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            self.push_message("已有 SSH 操作正在执行", window, cx);
            return;
        }
        let host = match self.jump_host_form_value(cx) {
            Ok(host) => host,
            Err(error) => {
                let message = error.to_string();
                self.jump_host_form_error = Some(message.clone());
                self.push_message(message, window, cx);
                return;
            }
        };
        let endpoint = format!("{}@{}:{}", host.username, host.host, host.port);
        self.busy = true;
        self.push_message(format!("正在测试 SSH 连接：{endpoint}"), window, cx);
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::test_jump_host_connection(&host) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.push_message(format!("SSH 连接测试成功：{endpoint}"), window, cx)
                    }
                    Err(error) => {
                        this.push_message(format!("SSH 连接测试失败：{error:#}"), window, cx)
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn request_delete_jump_host(
        &self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.jump_hosts.iter().find(|host| host.id == id) else {
            return;
        };
        let forward_names = self
            .forwards
            .iter()
            .filter(|item| item.jump_host_id == id)
            .map(|item| format!("- 本地转发：{}", item.name));
        let connection_names = self
            .ssh_tabs
            .iter()
            .filter(|tab| tab.jump_host_id == id)
            .map(|tab| format!("- SSH 连接页签：{}", tab.title));
        let associated = forward_names.chain(connection_names).collect::<Vec<_>>();
        let details = if associated.is_empty() {
            "没有关联的 SSH 连接或本地转发配置。".to_string()
        } else {
            format!(
                "删除后将同时停止并删除以下关联项：\n\n{}",
                associated.join("\n")
            )
        };
        let title = format!("确认删除跳板机“{}”？", host.name);
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let delete_id = id.clone();
            let delete_view = view.clone();
            dialog
                .title(title.clone())
                .w(px(560.))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-delete-host").outline().label("取消")),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("confirm-delete-host")
                                    .danger()
                                    .label("确认删除"),
                            ),
                        ),
                )
                .child(
                    TextView::markdown(format!("delete-jump-host-{delete_id}"), details.clone())
                        .selectable(true),
                )
                .on_ok(move |_, window, cx| {
                    delete_view.update(cx, |this, cx| this.delete_jump_host(&delete_id, window, cx))
                })
        });
    }

    fn delete_jump_host(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let forward_ids = self
            .forwards
            .iter()
            .filter(|item| item.jump_host_id == id)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let next = storage::AppConfig {
            jump_hosts: self
                .jump_hosts
                .iter()
                .filter(|host| host.id != id)
                .cloned()
                .collect(),
            forwards: self
                .forwards
                .iter()
                .filter(|item| item.jump_host_id != id)
                .cloned()
                .collect(),
            quick_commands: self.quick_commands.clone(),
            command_history: self.command_history.clone(),
        };
        if let Err(error) = storage::save(&next) {
            self.push_message(format!("删除失败：{error:#}"), window, cx);
            return false;
        }
        for forward_id in &forward_ids {
            if let Some(mut handle) = self.tunnels.remove(forward_id) {
                handle.stop();
            }
            self.forward_states.remove(forward_id);
            self.startup_logs.remove(forward_id);
            self.selected.remove(forward_id);
        }
        self.ssh_tabs.retain(|tab| tab.jump_host_id != id);
        self.jump_hosts = next.jump_hosts;
        self.forwards = next.forwards;
        self.active_ssh_tab_id = self.ssh_tabs.last().map(|tab| tab.id.clone());
        if self.selected_jump_host_id.as_deref() == Some(id) {
            self.selected_jump_host_id = self.jump_hosts.first().map(|host| host.id.clone());
        }
        self.push_message(
            format!(
                "跳板机已删除，同时清理 {} 个本地转发和相关 SSH 连接",
                forward_ids.len()
            ),
            window,
            cx,
        );
        cx.notify();
        true
    }

    pub(super) fn open_ssh_connection(
        &mut self,
        jump_host_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == jump_host_id)
            .cloned()
        else {
            self.push_message("跳板机配置不存在", window, cx);
            return;
        };
        let tab_id = uuid::Uuid::new_v4().to_string();
        let command = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .submit_on_enter(true)
                .soft_wrap(true)
                .placeholder("输入命令，Enter 执行，Shift+Enter 换行")
        });
        let history_provider = CommandHistoryProvider {
            history: self.command_history_store.clone(),
        };
        command.update(cx, |input, _| {
            input.lsp.completion_provider = Some(Rc::new(history_provider));
        });
        let remote_path_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入远程路径"));
        let command_tab_id = tab_id.clone();
        let command_subscription =
            cx.subscribe_in(&command, window, move |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    this.send_ssh_command(&command_tab_id, window, cx);
                }
            });
        self._subscriptions.push(command_subscription);
        let path_tab_id = tab_id.clone();
        let path_subscription = cx.subscribe_in(
            &remote_path_input,
            window,
            move |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    let path = input.read(cx).value().to_string();
                    this.load_ssh_directory(&path_tab_id, path.trim(), window, cx);
                }
            },
        );
        self._subscriptions.push(path_subscription);
        self.ssh_tabs.push(SshTab {
            id: tab_id.clone(),
            jump_host_id: host.id.clone(),
            title: host.name.clone(),
            command,
            state: SshConnectionState::Connecting,
            terminal: None,
            file_panel_open: false,
            remote_path: String::new(),
            remote_path_input,
            remote_entries: Vec::new(),
            file_loading: false,
            file_error: None,
            show_file_time: false,
            show_file_size: true,
            show_file_permissions: false,
            terminal_font_size: None,
        });
        self.active_ssh_tab_id = Some(tab_id.clone());
        self.page = Page::Ssh;
        cx.notify();

        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::SshTerminalHandle::start(host) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        this.push_message(format!("SSH 连接失败：{message}"), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn activate_ssh_tab(&mut self, id: String, cx: &mut Context<Self>) {
        self.active_ssh_tab_id = Some(id);
        cx.notify();
    }

    pub(super) fn close_ssh_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.ssh_tabs.retain(|tab| tab.id != id);
        if self.active_ssh_tab_id.as_deref() == Some(id) {
            self.active_ssh_tab_id = self.ssh_tabs.last().map(|tab| tab.id.clone());
        }
        cx.notify();
    }

    pub(super) fn send_ssh_command(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.ssh_tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let command = self.ssh_tabs[tab_index]
            .command
            .read(cx)
            .value()
            .to_string();
        if command.trim().is_empty() {
            return;
        }
        let result = self.ssh_tabs[tab_index]
            .terminal
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH 尚未连接"))
            .and_then(|terminal| terminal.send_line(&command));
        match result {
            Ok(()) => {
                self.ssh_tabs[tab_index]
                    .command
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.record_command_history(command.trim_end(), window, cx);
            }
            Err(error) => self.push_message(error.to_string(), window, cx),
        }
    }

    fn record_command_history(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        remember_command(&mut self.command_history, command);
        *self.command_history_store.borrow_mut() = self.command_history.clone();
        if let Err(error) = self.persist() {
            self.push_message(format!("历史命令保存失败：{error:#}"), window, cx);
        }
    }

    pub(super) fn fill_ssh_command(
        &mut self,
        id: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let input = tab.command.clone();
        let line = command.matches('\n').count() as u32;
        let column = command
            .rsplit_once('\n')
            .map(|(_, line)| line.chars().count())
            .unwrap_or_else(|| command.chars().count()) as u32;
        input.update(cx, |input, cx| {
            input.set_value(command, window, cx);
            input.set_cursor_position(
                gpui_component::input::Position::new(line, column),
                window,
                cx,
            );
        });
        cx.notify();
    }

    pub(super) fn set_ssh_terminal_font_size(
        &mut self,
        id: &str,
        font_size: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_font_size = font_size;
        cx.notify();
    }

    pub(super) fn ui_font_size(&self) -> f32 {
        self.ui_font_size
    }

    pub(super) fn save_quick_command(
        &mut self,
        id: Option<&str>,
        name: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() || command.is_empty() {
            self.push_message("快捷命令名称和具体命令均不能为空", window, cx);
            return false;
        }
        let previous = self.quick_commands.clone();
        if let Some(id) = id {
            let Some(existing) = self.quick_commands.iter_mut().find(|item| item.id == id) else {
                self.push_message("快捷命令不存在", window, cx);
                return false;
            };
            existing.name = name.to_string();
            existing.command = command.to_string();
        } else {
            self.quick_commands.push(storage::QuickCommand {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.to_string(),
                command: command.to_string(),
            });
        }
        if let Err(error) = self.persist() {
            self.quick_commands = previous;
            self.push_message(format!("快捷命令保存失败：{error:#}"), window, cx);
            return false;
        }
        self.push_message("快捷命令已保存", window, cx);
        cx.notify();
        true
    }

    pub(super) fn delete_quick_command(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = self.quick_commands.clone();
        self.quick_commands.retain(|command| command.id != id);
        if self.quick_commands.len() == previous.len() {
            return false;
        }
        if let Err(error) = self.persist() {
            self.quick_commands = previous;
            self.push_message(format!("快捷命令删除失败：{error:#}"), window, cx);
            return false;
        }
        self.push_message("快捷命令已删除", window, cx);
        cx.notify();
        true
    }

    pub(super) fn clear_ssh_terminal(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.command
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(terminal) = &tab.terminal {
            terminal.clear_output();
        }
        cx.notify();
    }

    pub(super) fn reconnect_ssh_tab(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            self.push_message("跳板机配置不存在，无法重连", window, cx);
            return;
        };
        tab.terminal = None;
        tab.state = SshConnectionState::Connecting;
        let title = tab.title.clone();
        let tab_id = id.to_string();
        self.push_message(format!("正在重连 {title}"), window, cx);
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::SshTerminalHandle::start(host) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        this.push_message(format!("SSH 重连失败：{message}"), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn toggle_ssh_file_panel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.file_panel_open = !tab.file_panel_open;
        let should_load = tab.file_panel_open && tab.remote_path.is_empty() && !tab.file_loading;
        cx.notify();
        if should_load {
            self.load_ssh_directory(id, "", window, cx);
        }
    }

    pub(super) fn load_ssh_directory(
        &mut self,
        id: &str,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            self.push_message("跳板机配置不存在，无法读取文件", window, cx);
            return;
        };
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let requested_path = path.to_string();
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::list_directory(&host, &requested_path) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let mut resolved_path = None;
                let mut failure = None;
                {
                    let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                        return;
                    };
                    tab.file_loading = false;
                    match result {
                        Ok((path, entries)) => {
                            tab.remote_path = path.clone();
                            tab.remote_entries = entries;
                            tab.file_error = None;
                            resolved_path = Some((tab.remote_path_input.clone(), path));
                        }
                        Err(error) => {
                            let message = format!("{error:#}");
                            tab.file_error = Some(message.clone());
                            failure = Some(message);
                        }
                    }
                }
                if let Some((input, path)) = resolved_path {
                    input.update(cx, |input, cx| input.set_value(path, window, cx));
                }
                if let Some(message) = failure {
                    this.push_message(format!("远程路径跳转失败：{message}"), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn toggle_ssh_file_view(&mut self, id: &str, option: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        match option {
            "time" => tab.show_file_time = !tab.show_file_time,
            "size" => tab.show_file_size = !tab.show_file_size,
            "permissions" => tab.show_file_permissions = !tab.show_file_permissions,
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn prompt_create_ssh_entry(
        &mut self,
        id: &str,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if is_dir {
                "输入文件夹名称"
            } else {
                "输入文件名称"
            })
        });
        let view = cx.entity();
        let tab_id = id.to_string();
        let kind = if is_dir { "文件夹" } else { "文件" };
        window.open_dialog(cx, move |dialog, _, _| {
            let create_view = view.clone();
            let create_name = name.clone();
            let create_tab_id = tab_id.clone();
            dialog
                .title(format!("新建{kind}"))
                .w(px(420.))
                .child(Input::new(&name))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-create-remote").outline().label("取消")),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("confirm-create-remote").primary().label("创建"),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let entry_name = create_name.read(cx).value().to_string();
                    let entry_name = entry_name.trim();
                    let validation_error = if entry_name.is_empty() {
                        Some("名称不能为空")
                    } else if entry_name == "." || entry_name == ".." {
                        Some("名称不能是“.”或“..”")
                    } else if entry_name.contains('/') || entry_name.contains('\\') {
                        Some("名称不能包含路径分隔符")
                    } else {
                        None
                    };
                    if let Some(error) = validation_error {
                        create_view.update(cx, |this, cx| {
                            this.push_message(error, window, cx);
                        });
                        return false;
                    }
                    create_view.update(cx, |this, cx| {
                        this.create_ssh_entry(&create_tab_id, entry_name, is_dir, window, cx)
                    });
                    true
                })
        });
    }

    fn create_ssh_entry(
        &mut self,
        id: &str,
        name: &str,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            self.push_message("跳板机配置不存在，无法新建文件", window, cx);
            return;
        };
        let remote_dir = if tab.remote_path.is_empty() {
            ".".to_string()
        } else {
            tab.remote_path.clone()
        };
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let entry_name = name.to_string();
        let kind = if is_dir { "文件夹" } else { "文件" };
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let remote_dir_for_create = remote_dir.clone();
            let entry_name_for_create = entry_name.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::create_entry(
                        &host,
                        &remote_dir_for_create,
                        &entry_name_for_create,
                        is_dir,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.push_message(format!("已创建{kind}：{entry_name}"), window, cx);
                    this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                }
                Err(error) => {
                    if let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.file_loading = false;
                        tab.file_error = Some(format!("{error:#}"));
                    }
                    this.push_message(format!("新建{kind}失败：{error:#}"), window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn prompt_ssh_upload(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("选择要上传的文件或文件夹".into()),
        });
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let Ok(Ok(Some(paths))) = selected.await else {
                return;
            };
            let _ = weak.update_in(cx, |this, window, cx| {
                this.upload_ssh_paths(&tab_id, paths, window, cx);
            });
        })
        .detach();
    }

    pub(super) fn upload_ssh_paths(
        &mut self,
        id: &str,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            return;
        };
        let tab_id = id.to_string();
        let remote_dir = if tab.remote_path.is_empty() {
            ".".to_string()
        } else {
            tab.remote_path.clone()
        };
        self.push_message(
            format!("正在上传 {} 个项目到 {}", paths.len(), remote_dir),
            window,
            cx,
        );
        cx.spawn_in(window, async move |weak, cx| {
            let remote_dir_for_upload = remote_dir.clone();
            let result = cx
                .background_executor()
                .spawn(async move { forward::upload(&host, &remote_dir_for_upload, &paths) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok(count) => {
                    this.push_message(format!("上传完成：{count} 个文件"), window, cx);
                    this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                }
                Err(error) => {
                    this.push_message(format!("上传失败：{error:#}"), window, cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn prompt_ssh_download(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择下载位置".into()),
        });
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let Ok(Ok(Some(mut paths))) = selected.await else {
                return;
            };
            let Some(directory) = paths.pop() else {
                return;
            };
            let target = directory.join(&entry.name);
            let _ = weak.update_in(cx, |this, window, cx| {
                this.download_ssh_entry(&tab_id, entry.clone(), target.clone(), window, cx);
            });
        })
        .detach();
    }

    pub(super) fn prepare_ssh_drag(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PathBuf {
        let target = std::env::temp_dir()
            .join("s-porter-downloads")
            .join(id)
            .join(&entry.name);
        if entry.is_dir {
            let _ = std::fs::create_dir_all(&target);
        } else if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::File::create(&target);
        }
        self.download_ssh_entry(id, entry, target.clone(), window, cx);
        target
    }

    fn download_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            return;
        };
        self.push_message(format!("正在下载 {}", entry.name), window, cx);
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::download(&host, &entry.path, entry.is_dir, &target)
                        .map(|count| (count, target))
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok((count, target)) => this.push_message(
                    format!("下载完成：{count} 个文件，保存到 {}", target.display()),
                    window,
                    cx,
                ),
                Err(error) => {
                    this.push_message(format!("下载失败：{error:#}"), window, cx);
                }
            });
        })
        .detach();
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
            self.push_message("请先选择要删除的配置", window, cx);
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
        match self.persist() {
            Ok(()) => self.push_message(format!("已删除 {} 个转发配置", ids.len()), window, cx),
            Err(error) => {
                self.push_message(format!("配置已删除，但保存失败：{error:#}"), window, cx)
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
            self.push_message("该转发已在运行", window, cx);
            return;
        }
        let Some(item) = self.forwards.iter().find(|item| item.id == id).cloned() else {
            return;
        };
        let Some(jump_host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == item.jump_host_id)
            .cloned()
        else {
            self.push_message("关联的跳板机配置不存在", window, cx);
            return;
        };
        let id = id.to_string();
        self.forward_states
            .insert(id.clone(), ForwardState::Starting);
        self.startup_logs
            .entry(id.clone())
            .or_default()
            .push("开始启动：检查本地端口、SSH 认证和远程目标连通性".into());
        self.push_message(format!("{} 正在启动", item.name), window, cx);
        cx.notify();

        cx.spawn_in(window, async move |weak, cx| {
            let name = item.name.clone();
            let result = cx
                .background_executor()
                .spawn(async move { forward::TunnelHandle::start(item, jump_host) })
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
                        this.push_message(format!("{} 启动成功", name), window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.forward_states
                            .insert(id.clone(), ForwardState::Failed(message.clone()));
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("启动失败：{message}"));
                        this.push_message(format!("{} 启动失败：{}", name, message), window, cx);
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
            self.push_message("端口转发已停止", window, cx);
            cx.notify();
        } else if matches!(self.forward_states.get(id), Some(ForwardState::Failed(_))) {
            self.forward_states
                .insert(id.to_string(), ForwardState::Stopped);
            cx.notify();
        } else {
            self.push_message("该转发当前未运行", window, cx);
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
            self.push_message("已有 SSH 操作正在执行", window, cx);
            return;
        }
        self.busy = true;
        let operation = if enable {
            "开启允许转发"
        } else {
            "测试连接"
        };
        let id = item.id.clone();
        let Some(jump_host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == item.jump_host_id)
            .cloned()
        else {
            self.busy = false;
            self.push_message("关联的跳板机配置不存在", window, cx);
            return;
        };
        self.startup_logs
            .entry(id.clone())
            .or_default()
            .push(format!(
                "{operation}：开始连接 SSH 服务 {}:{}",
                jump_host.host, jump_host.port
            ));
        self.push_message(
            if enable {
                "正在配置远端 sshd"
            } else {
                "正在测试 SSH 与目标端口"
            },
            window,
            cx,
        );
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if enable {
                        forward::enable_forwarding(&jump_host)
                            .map(|_| "远端已允许 TCP 转发，sshd 已重启".to_string())
                    } else {
                        forward::test_connection(&item, &jump_host)
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
                        this.push_message(message.clone(), window, cx);
                    }
                    Err(error) => {
                        let message = format!("操作失败：{error:#}");
                        this.startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("{operation}失败：{error:#}"));
                        this.push_message(message.clone(), window, cx);
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
            Err(error) => self.push_message(error.to_string(), window, cx),
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
        window.set_rem_size(px(self.ui_font_size));
        let sidebar_collapsed = self.sidebar_collapsed;
        let ui_font_size = self.ui_font_size;
        let message_count = self.messages.len();
        let message_search = self.message_search.clone();
        let messages = self.messages.clone();
        let font_size_view = cx.entity();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .gap_2()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("toggle-sidebar")
                                        .xsmall()
                                        .ghost()
                                        .icon(if sidebar_collapsed {
                                            IconName::PanelLeftOpen
                                        } else {
                                            IconName::PanelLeftClose
                                        })
                                        .tooltip(if sidebar_collapsed {
                                            "展开侧边栏"
                                        } else {
                                            "收起侧边栏"
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.sidebar_collapsed = !this.sidebar_collapsed;
                                            cx.notify();
                                        })),
                                )
                                .child("S Porter"),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("font-size")
                                        .xsmall()
                                        .ghost()
                                        .label(format!("字号 {ui_font_size:.0}px"))
                                        .dropdown_caret(true)
                                        .tooltip("调整字号")
                                        .dropdown_menu_with_anchor(
                                            Anchor::BottomRight,
                                            move |menu, window, _| {
                                                UI_FONT_SIZES.into_iter().fold(
                                                    menu,
                                                    |menu, font_size| {
                                                        let view = font_size_view.clone();
                                                        menu.item(
                                                            PopupMenuItem::new(format!(
                                                                "{font_size}px"
                                                            ))
                                                            .checked(
                                                                ui_font_size
                                                                    == f32::from(font_size),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &view,
                                                                move |this, _, window, cx| {
                                                                    this.set_ui_font_size(
                                                                        f32::from(font_size),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                    },
                                                )
                                            },
                                        ),
                                )
                                .child(
                                    Button::new("message-center")
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::Bell)
                                        .label(message_count.to_string())
                                        .tooltip("查看最近 100 条消息")
                                        .on_click(move |_, window, cx| {
                                            let message_search = message_search.clone();
                                            let messages = messages.clone();
                                            window.open_sheet(cx, move |sheet, _, cx| {
                                                message_center::render(
                                                    sheet,
                                                    message_search.clone(),
                                                    messages.clone(),
                                                    cx,
                                                )
                                            });
                                        }),
                                ),
                        ),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-layout")
                        .child(
                            resizable_panel()
                                .visible(!sidebar_collapsed)
                                .size(px(196.))
                                .size_range(px(168.)..px(320.))
                                .flex_none()
                                .child(sidebar::render(self, cx)),
                        )
                        .child(resizable_panel().child(div().size_full().min_w_0().child(
                            match self.page {
                                Page::JumpHosts => jump_host_page::render(self, cx),
                                Page::Ssh => ssh_page::render(self, cx),
                                Page::Forward => forward_page::render(self, cx),
                                Page::Crypto => tool_page::render(self, true, cx),
                                Page::Codec => tool_page::render(self, false, cx),
                                Page::Format => format_page::render(self, cx),
                                Page::Time => time_page::render(self, cx),
                            },
                        ))),
                ),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::remember_command;

    #[test]
    fn command_history_is_recent_deduplicated_and_bounded() {
        let mut history = (0..500)
            .map(|index| format!("command-{index}"))
            .collect::<Vec<_>>();
        remember_command(&mut history, "command-12");
        assert_eq!(history.len(), 500);
        assert_eq!(history[0], "command-12");
        assert_eq!(
            history
                .iter()
                .filter(|command| command.as_str() == "command-12")
                .count(),
            1
        );

        remember_command(&mut history, "new-command");
        assert_eq!(history.len(), 500);
        assert_eq!(history[0], "new-command");
    }
}
