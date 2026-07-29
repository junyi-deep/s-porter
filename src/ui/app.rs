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
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
    table::TableState,
    text::TextView,
    *,
};
use std::time::{Duration, Instant};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};
use unicode_width::UnicodeWidthChar as _;

const DEFAULT_UI_FONT_SIZE: f32 = 14.;
const SSH_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const SSH_OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(30);
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
    pub(super) batch_entries: Entity<InputState>,
    pub(super) batch_separator: Entity<InputState>,
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
        let batch_entries = cx.new(|cx| {
            InputState::new(window, cx).multi_line(true).placeholder(
                "每行一台：服务器名称, SSH地址\n例如：\n生产-01, 10.0.0.11\n生产-02, 10.0.0.12",
            )
        });
        let batch_separator = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("可选，例如：;（留空则自动识别逗号、Tab 或空格）")
        });
        let mut input =
            |value: &'static str, placeholder: &'static str, cx: &mut Context<AppView>| {
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

#[derive(Clone, PartialEq, Eq)]
pub(super) enum SshConnectionState {
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum TransferStatus {
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SshFilePanelView {
    Files,
    Transfers,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RemoteSortField {
    Name,
    Modified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TerminalSearchMatch {
    pub(super) line: usize,
    pub(super) range: std::ops::Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TerminalPoint {
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalSelection {
    pub(super) anchor: TerminalPoint,
    pub(super) cursor: TerminalPoint,
}

pub(super) struct SshTransfer {
    pub(super) id: String,
    pub(super) direction: TransferDirection,
    pub(super) title: String,
    pub(super) progress: forward::TransferProgress,
    pub(super) status: TransferStatus,
    pub(super) started_at: String,
    pub(super) finished_at: Option<String>,
}

pub(super) struct SshTab {
    pub(super) id: String,
    pub(super) jump_host_id: String,
    pub(super) title: String,
    pub(super) state: SshConnectionState,
    pub(super) terminal: Option<forward::SshTerminalHandle>,
    pub(super) terminal_lines: Arc<Vec<forward::TerminalLine>>,
    pub(super) terminal_scroll: UniformListScrollHandle,
    pub(super) terminal_focus: FocusHandle,
    pub(super) terminal_size: Rc<Cell<(u16, u16)>>,
    pub(super) terminal_viewport_height: Rc<Cell<f32>>,
    pub(super) terminal_content_left: Rc<Cell<f32>>,
    pub(super) terminal_output_revision: u64,
    pub(super) terminal_last_output_sync: Instant,
    pub(super) terminal_selection: Option<TerminalSelection>,
    pub(super) terminal_selecting: bool,
    pub(super) terminal_search: Entity<InputState>,
    pub(super) terminal_search_open: bool,
    pub(super) terminal_search_index: Option<usize>,
    pub(super) file_panel_open: bool,
    pub(super) remote_path: String,
    pub(super) remote_path_input: Entity<InputState>,
    pub(super) remote_entries: Vec<forward::RemoteEntry>,
    pub(super) file_loading: bool,
    pub(super) file_error: Option<String>,
    pub(super) show_file_time: bool,
    pub(super) show_file_size: bool,
    pub(super) show_file_permissions: bool,
    pub(super) remote_sort_field: RemoteSortField,
    pub(super) remote_sort_ascending: bool,
    pub(super) terminal_font_size: Option<f32>,
    pub(super) transfers: Vec<SshTransfer>,
    pub(super) file_panel_view: SshFilePanelView,
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

fn remember_command(history: &mut Vec<String>, command: &str) {
    history.retain(|existing| existing != command);
    history.insert(0, command.to_string());
    history.truncate(500);
}

fn parse_jump_host_batch_entries(
    value: &str,
    custom_separator: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let custom_separator = match custom_separator {
        "\\t" => Some("\t"),
        value if value.trim().is_empty() => None,
        value => Some(value),
    };
    let mut entries = Vec::new();
    for (index, source) in value.lines().enumerate() {
        let line = source.trim();
        if line.is_empty() {
            continue;
        }
        let pair = if let Some(separator) = custom_separator {
            line.split_once(separator)
        } else {
            [',', '，', '|', '\t']
                .into_iter()
                .find_map(|separator| line.split_once(separator))
                .or_else(|| {
                    line.char_indices()
                        .rev()
                        .find(|(_, character)| character.is_whitespace())
                        .map(|(index, _)| line.split_at(index))
                })
        };
        let Some((name, host)) = pair else {
            anyhow::bail!("第 {} 行格式错误，请使用名称与 SSH 地址分隔符", index + 1);
        };
        let name = name.trim();
        let host = host.trim();
        anyhow::ensure!(!name.is_empty(), "第 {} 行服务器名称不能为空", index + 1);
        anyhow::ensure!(!host.is_empty(), "第 {} 行 SSH 地址不能为空", index + 1);
        entries.push((name.to_string(), host.to_string()));
    }
    anyhow::ensure!(!entries.is_empty(), "请至少输入一台服务器");
    anyhow::ensure!(entries.len() <= 500, "单次最多批量新增 500 台服务器");
    Ok(entries)
}

fn terminal_byte_offset(text: &str, column: usize) -> usize {
    let mut display_column = 0;
    for (offset, character) in text.char_indices() {
        if display_column >= column {
            return offset;
        }
        display_column += character.width().unwrap_or(1);
    }
    text.len()
}

fn terminal_display_width(text: &str) -> usize {
    text.chars()
        .map(|character| character.width().unwrap_or(1))
        .sum()
}

fn terminal_selected_text(
    lines: &[forward::TerminalLine],
    selection: TerminalSelection,
) -> Option<String> {
    let (start, end) = if selection.anchor <= selection.cursor {
        (selection.anchor, selection.cursor)
    } else {
        (selection.cursor, selection.anchor)
    };
    if start == end || start.line >= lines.len() {
        return None;
    }
    let end_line = end.line.min(lines.len().saturating_sub(1));
    let mut selected = String::new();
    for (line_index, line) in lines.iter().enumerate().take(end_line + 1).skip(start.line) {
        let text = &line.text;
        let start_column = if line_index == start.line {
            start.column
        } else {
            0
        };
        let end_column = if line_index == end_line {
            end.column
        } else {
            terminal_display_width(text)
        };
        let start_offset = terminal_byte_offset(text, start_column);
        let end_offset = terminal_byte_offset(text, end_column);
        if start_offset < end_offset {
            selected.push_str(&text[start_offset..end_offset]);
        }
        if line_index < end_line {
            selected.push('\n');
        }
    }
    (!selected.is_empty()).then_some(selected)
}

pub(super) fn terminal_search_matches(
    lines: &[forward::TerminalLine],
    query: &str,
) -> Vec<TerminalSearchMatch> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .flat_map(|(line, terminal_line)| {
            terminal_line
                .text
                .to_lowercase()
                .match_indices(&query)
                .map(move |(start, value)| TerminalSearchMatch {
                    line,
                    range: start..start + value.len(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn terminal_modifier_code(modifiers: Modifiers) -> u8 {
    1 + u8::from(modifiers.shift) + u8::from(modifiers.alt) * 2 + u8::from(modifiers.control) * 4
}

// Keyboard encoding follows the same xterm mappings used by Zed's terminal,
// whose implementation is derived from Alacritty's terminal input mapping.
fn terminal_key_bytes(
    keystroke: &Keystroke,
    application_cursor: bool,
    prefer_character_input: bool,
) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    // Tab 是 shell 补全键，必须先于 GPUI 的字符输入偏好处理。
    // 否则某些平台会把 Tab 标记为 prefer_character_input，但不提供 key_char，
    // 最终事件继续传播并触发界面焦点跳转。
    if keystroke.key == "tab" {
        return match modifiers {
            Modifiers {
                shift: true,
                control: false,
                alt: false,
                ..
            } => Some(b"\x1b[Z".to_vec()),
            Modifiers {
                shift: false,
                control: false,
                alt: false,
                ..
            } => Some(vec![b'\t']),
            _ => None,
        };
    }
    if prefer_character_input {
        return keystroke
            .key_char
            .as_deref()
            .map(|value| value.as_bytes().to_vec());
    }

    if modifiers.control && !modifiers.alt {
        let key = keystroke.key.as_str();
        let control = match key.to_ascii_lowercase().as_str() {
            "space" | "@" => Some(0),
            "[" => Some(27),
            "\\" => Some(28),
            "]" => Some(29),
            "^" => Some(30),
            "_" => Some(31),
            "?" => Some(127),
            value if value.len() == 1 => {
                let byte = value.as_bytes()[0];
                byte.is_ascii_lowercase().then_some(byte - b'a' + 1)
            }
            _ => None,
        };
        if let Some(control) = control {
            return Some(vec![control]);
        }
    }

    let no_modifiers = !modifiers.control && !modifiers.alt && !modifiers.shift;
    let cursor_prefix = if application_cursor { "\x1bO" } else { "\x1b[" };
    let value = match keystroke.key.as_str() {
        "escape" if no_modifiers => Some("\x1b".to_string()),
        "enter" if no_modifiers => Some("\r".to_string()),
        "enter" if modifiers.shift && !modifiers.control && !modifiers.alt => {
            Some("\n".to_string())
        }
        "enter" if modifiers.alt && !modifiers.control => Some("\x1b\r".to_string()),
        "backspace" if no_modifiers => Some("\x7f".to_string()),
        "backspace" if modifiers.control && !modifiers.alt => Some("\x08".to_string()),
        "backspace" if modifiers.alt && !modifiers.control => Some("\x1b\x7f".to_string()),
        "up" if no_modifiers => Some(format!("{cursor_prefix}A")),
        "down" if no_modifiers => Some(format!("{cursor_prefix}B")),
        "right" if no_modifiers => Some(format!("{cursor_prefix}C")),
        "left" if no_modifiers => Some(format!("{cursor_prefix}D")),
        "home" if no_modifiers => Some(format!("{cursor_prefix}H")),
        "end" if no_modifiers => Some(format!("{cursor_prefix}F")),
        "insert" if no_modifiers => Some("\x1b[2~".to_string()),
        "delete" if no_modifiers => Some("\x1b[3~".to_string()),
        "pageup" if no_modifiers => Some("\x1b[5~".to_string()),
        "pagedown" if no_modifiers => Some("\x1b[6~".to_string()),
        "f1" if no_modifiers => Some("\x1bOP".to_string()),
        "f2" if no_modifiers => Some("\x1bOQ".to_string()),
        "f3" if no_modifiers => Some("\x1bOR".to_string()),
        "f4" if no_modifiers => Some("\x1bOS".to_string()),
        "f5" if no_modifiers => Some("\x1b[15~".to_string()),
        "f6" if no_modifiers => Some("\x1b[17~".to_string()),
        "f7" if no_modifiers => Some("\x1b[18~".to_string()),
        "f8" if no_modifiers => Some("\x1b[19~".to_string()),
        "f9" if no_modifiers => Some("\x1b[20~".to_string()),
        "f10" if no_modifiers => Some("\x1b[21~".to_string()),
        "f11" if no_modifiers => Some("\x1b[23~".to_string()),
        "f12" if no_modifiers => Some("\x1b[24~".to_string()),
        "up" | "down" | "right" | "left" | "home" | "end"
            if modifiers.control || modifiers.alt || modifiers.shift =>
        {
            let suffix = match keystroke.key.as_str() {
                "up" => 'A',
                "down" => 'B',
                "right" => 'C',
                "left" => 'D',
                "home" => 'H',
                _ => 'F',
            };
            Some(format!(
                "\x1b[1;{}{suffix}",
                terminal_modifier_code(modifiers)
            ))
        }
        _ => None,
    };
    if let Some(value) = value {
        return Some(value.into_bytes());
    }

    let character = keystroke
        .key_char
        .as_deref()
        .or_else(|| (keystroke.key.chars().count() == 1).then_some(keystroke.key.as_str()))?;
    if modifiers.control {
        return None;
    }
    let mut bytes = Vec::with_capacity(character.len() + usize::from(modifiers.alt));
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(character.as_bytes());
    Some(bytes)
}

pub(super) struct AppView {
    pub(super) page: Page,
    pub(super) sidebar_collapsed: bool,
    ui_font_size: f32,
    terminal_history_lines: usize,
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
    pub(super) jump_host_batch_mode: bool,
    pub(super) jump_host_search: Entity<InputState>,
    pub(super) ssh_host_picker_search: Entity<InputState>,
    pub(super) jump_host_table: Entity<TableState<jump_host_page::JumpHostTableDelegate>>,
    pub(super) forward_table: Entity<TableState<forward_page::ForwardTableDelegate>>,
    pub(super) ssh_tabs: Vec<SshTab>,
    pub(super) active_ssh_tab_id: Option<String>,
    pub(super) quick_commands: Vec<storage::QuickCommand>,
    pub(super) command_history: Vec<String>,
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
            .new(|cx| InputState::new(window, cx).placeholder("搜索名称、端口、远程目标或服务器"));
        let jump_host_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称、地址或登录用户"));
        let forward_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索服务器"));
        let ssh_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索服务器"));
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
        let terminal_history_lines = config.terminal_history_lines.clamp(
            forward::MIN_TERMINAL_HISTORY_LINES,
            forward::MAX_TERMINAL_HISTORY_LINES,
        );
        let selected_jump_host_id = config.jump_hosts.first().map(|host| host.id.clone());
        let command_history = config
            .command_history
            .into_iter()
            .take(500)
            .collect::<Vec<_>>();
        let view = Self {
            page: Page::JumpHosts,
            sidebar_collapsed: false,
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            terminal_history_lines,
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
            jump_host_batch_mode: false,
            jump_host_search,
            ssh_host_picker_search,
            jump_host_table,
            forward_table,
            ssh_tabs: Vec::new(),
            active_ssh_tab_id: None,
            quick_commands: config.quick_commands,
            command_history,
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
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(SSH_OUTPUT_POLL_INTERVAL)
                    .await;
                if weak
                    .update_in(cx, |this, _, cx| this.sync_active_ssh_output(cx))
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

    pub(super) fn show_hint(
        &mut self,
        message: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = message.into();
        let id = uuid::Uuid::new_v4().to_string();
        window.push_notification(
            Notification::new().content(move |_, _, _| {
                TextView::markdown(format!("hint-{id}"), text.clone())
                    .selectable(true)
                    .into_any_element()
            }),
            cx,
        );
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
                .ok_or_else(|| anyhow::anyhow!("请先新增并选择服务器"))?,
            keep_alive: self.form_keep_alive,
            keep_alive_interval_secs,
        };
        config.validate()?;
        anyhow::ensure!(
            self.jump_hosts
                .iter()
                .any(|host| host.id == config.jump_host_id),
            "选择的服务器不存在"
        );
        Ok(config)
    }

    fn app_config(&self) -> storage::AppConfig {
        storage::AppConfig {
            jump_hosts: self.jump_hosts.clone(),
            forwards: self.forwards.clone(),
            quick_commands: self.quick_commands.clone(),
            command_history: self.command_history.clone(),
            terminal_history_lines: self.terminal_history_lines,
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
                self.show_hint(error.to_string(), window, cx);
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
        self.jump_host_batch_mode = false;
        self.jump_host_form_error = None;
        let values = [
            (&self.jump_host_form.name, ""),
            (&self.jump_host_form.host, ""),
            (&self.jump_host_form.batch_entries, ""),
            (&self.jump_host_form.batch_separator, ""),
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

    pub(super) fn prepare_batch_jump_hosts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_new_jump_host(window, cx);
        self.jump_host_batch_mode = true;
        cx.notify();
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
        self.jump_host_batch_mode = false;
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

    pub(super) fn prepare_copy_jump_host(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(host) = self.jump_hosts.iter().find(|host| host.id == id).cloned() else {
            return false;
        };
        let base_name = format!("{}_copy", host.name);
        let mut copy_name = base_name.clone();
        let mut suffix = 2;
        while self.jump_hosts.iter().any(|host| host.name == copy_name) {
            copy_name = format!("{base_name}_{suffix}");
            suffix += 1;
        }
        let proxy = host.http_proxy.unwrap_or_default();
        self.editing_jump_host_id = None;
        self.jump_host_batch_mode = false;
        self.jump_host_form_error = None;
        let values = [
            (&self.jump_host_form.name, copy_name),
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

    fn jump_host_form_value_with_identity(
        &self,
        id: String,
        name: String,
        host: String,
        cx: &App,
    ) -> anyhow::Result<JumpHost> {
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
            id,
            name,
            host,
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

    fn jump_host_form_value(&self, cx: &App) -> anyhow::Result<JumpHost> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        self.jump_host_form_value_with_identity(
            self.editing_jump_host_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            value(&self.jump_host_form.name),
            value(&self.jump_host_form.host),
            cx,
        )
    }

    fn jump_host_batch_values(&self, cx: &App) -> anyhow::Result<Vec<JumpHost>> {
        let source = self
            .jump_host_form
            .batch_entries
            .read(cx)
            .value()
            .to_string();
        let separator = self
            .jump_host_form
            .batch_separator
            .read(cx)
            .value()
            .to_string();
        let entries = parse_jump_host_batch_entries(&source, &separator)?;
        entries
            .into_iter()
            .enumerate()
            .map(|(index, (name, host))| {
                self.jump_host_form_value_with_identity(
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    host,
                    cx,
                )
                .map_err(|error| anyhow::anyhow!("第 {} 行：{error}", index + 1))
            })
            .collect()
    }

    pub(super) fn save_jump_host(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let hosts = match if self.jump_host_batch_mode {
            self.jump_host_batch_values(cx)
        } else {
            self.jump_host_form_value(cx).map(|host| vec![host])
        } {
            Ok(hosts) => hosts,
            Err(error) => {
                let message = error.to_string();
                self.jump_host_form_error = Some(message.clone());
                return false;
            }
        };
        let mut next = self.jump_hosts.clone();
        if self.jump_host_batch_mode {
            next.extend(hosts.iter().cloned());
        } else {
            let host = &hosts[0];
            if let Some(existing) = next.iter_mut().find(|item| item.id == host.id) {
                *existing = host.clone();
            } else {
                next.push(host.clone());
            }
        }
        let config = storage::AppConfig {
            jump_hosts: next.clone(),
            forwards: self.forwards.clone(),
            quick_commands: self.quick_commands.clone(),
            command_history: self.command_history.clone(),
            terminal_history_lines: self.terminal_history_lines,
        };
        if let Err(error) = storage::save(&config) {
            let message = format!("保存失败：{error:#}");
            self.jump_host_form_error = Some(message.clone());
            self.push_message(message, window, cx);
            return false;
        }
        self.jump_host_form_error = None;
        self.jump_hosts = next;
        if let Some(host) = hosts.first() {
            self.selected_jump_host_id.get_or_insert(host.id.clone());
        }
        if self.jump_host_batch_mode {
            self.push_message(format!("已批量新增 {} 台服务器", hosts.len()), window, cx);
        } else {
            self.push_message("服务器配置已保存", window, cx);
        }
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
        let title = format!("确认删除服务器“{}”？", host.name);
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
            terminal_history_lines: self.terminal_history_lines,
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
                "服务器已删除，同时清理 {} 个本地转发和相关 SSH 连接",
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
            self.push_message("服务器配置不存在", window, cx);
            return;
        };
        let tab_id = uuid::Uuid::new_v4().to_string();
        let remote_path_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入远程路径"));
        let terminal_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索终端内容"));
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
        let search_tab_id = tab_id.clone();
        let search_subscription = cx.subscribe_in(
            &terminal_search,
            window,
            move |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    if let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == search_tab_id)
                    {
                        tab.terminal_search_index = None;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.navigate_ssh_terminal_search(
                        &search_tab_id,
                        if *shift { -1 } else { 1 },
                        window,
                        cx,
                    );
                }
                _ => {}
            },
        );
        self._subscriptions.push(search_subscription);
        self.ssh_tabs.push(SshTab {
            id: tab_id.clone(),
            jump_host_id: host.id.clone(),
            title: host.name.clone(),
            state: SshConnectionState::Connecting,
            terminal: None,
            terminal_lines: Arc::new(vec![forward::TerminalLine {
                text: "正在建立 SSH 连接…".into(),
                styles: Vec::new(),
                cursor_column: None,
            }]),
            terminal_scroll: UniformListScrollHandle::new(),
            terminal_focus: cx.focus_handle().tab_stop(true),
            terminal_size: Rc::new(Cell::new((120, 40))),
            terminal_viewport_height: Rc::new(Cell::new(0.)),
            terminal_content_left: Rc::new(Cell::new(0.)),
            terminal_output_revision: 0,
            terminal_last_output_sync: Instant::now() - SSH_OUTPUT_FRAME_INTERVAL,
            terminal_selection: None,
            terminal_selecting: false,
            terminal_search,
            terminal_search_open: false,
            terminal_search_index: None,
            file_panel_open: false,
            remote_path: String::new(),
            remote_path_input,
            remote_entries: Vec::new(),
            file_loading: false,
            file_error: None,
            show_file_time: true,
            show_file_size: false,
            show_file_permissions: false,
            remote_sort_field: RemoteSortField::Name,
            remote_sort_ascending: true,
            terminal_font_size: None,
            transfers: Vec::new(),
            file_panel_view: SshFilePanelView::Files,
        });
        self.active_ssh_tab_id = Some(tab_id.clone());
        self.page = Page::Ssh;
        cx.notify();

        let terminal_history_lines = self.terminal_history_lines;
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::SshTerminalHandle::start_with_history_limit(
                        host,
                        terminal_history_lines,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                        tab.terminal_focus.focus(window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
                            text: format!("SSH 连接失败：{message}"),
                            styles: Vec::new(),
                            cursor_column: None,
                        }]);
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
        self.sync_active_ssh_output(cx);
        cx.notify();
    }

    fn sync_active_ssh_output(&mut self, cx: &mut Context<Self>) {
        if self.page != Page::Ssh {
            return;
        }
        let Some(active_id) = self.active_ssh_tab_id.as_deref() else {
            return;
        };
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == active_id) else {
            return;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return;
        };
        if tab.terminal_last_output_sync.elapsed() < SSH_OUTPUT_FRAME_INTERVAL {
            return;
        }
        let Some((revision, output)) = terminal.output_if_changed(tab.terminal_output_revision)
        else {
            return;
        };
        tab.terminal_last_output_sync = Instant::now();
        tab.terminal_output_revision = revision;
        tab.terminal_lines = if output.is_empty() {
            Arc::new(vec![forward::TerminalLine {
                text: "终端输出已清空".into(),
                styles: Vec::new(),
                cursor_column: None,
            }])
        } else {
            Arc::new(output)
        };
        tab.terminal_scroll.scroll_to_item(
            tab.terminal_lines.len().saturating_sub(1),
            ScrollStrategy::Bottom,
        );
        cx.notify();
    }

    pub(super) fn close_ssh_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.ssh_tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        Self::cancel_ssh_tab_transfers(&self.ssh_tabs[index]);
        self.ssh_tabs.remove(index);
        if self.active_ssh_tab_id.as_deref() == Some(id) {
            self.active_ssh_tab_id = self
                .ssh_tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|index| self.ssh_tabs.get(index))
                })
                .map(|tab| tab.id.clone());
        }
        cx.notify();
    }

    pub(super) fn close_other_ssh_tabs(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.ssh_tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        for tab in self.ssh_tabs.iter().filter(|tab| tab.id != id) {
            Self::cancel_ssh_tab_transfers(tab);
        }
        self.ssh_tabs.retain(|tab| tab.id == id);
        self.active_ssh_tab_id = Some(id.to_string());
        cx.notify();
    }

    pub(super) fn close_all_ssh_tabs(&mut self, cx: &mut Context<Self>) {
        for tab in &self.ssh_tabs {
            Self::cancel_ssh_tab_transfers(tab);
        }
        self.ssh_tabs.clear();
        self.active_ssh_tab_id = None;
        cx.notify();
    }

    fn cancel_ssh_tab_transfers(tab: &SshTab) {
        for transfer in &tab.transfers {
            if matches!(
                transfer.status,
                TransferStatus::Running | TransferStatus::Cancelling
            ) {
                transfer.progress.cancel();
            }
        }
    }

    pub(super) fn run_ssh_quick_command(
        &mut self,
        id: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.ssh_tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if command.trim().is_empty() {
            return;
        }
        let result = self.ssh_tabs[tab_index]
            .terminal
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH 尚未连接"))
            .and_then(|terminal| terminal.send_line(command));
        match result {
            Ok(()) => {
                self.record_command_history(command.trim_end(), window, cx);
                self.ssh_tabs[tab_index].terminal_focus.focus(window, cx);
            }
            Err(error) => self.show_ssh_interaction_error(id, error.to_string(), cx),
        }
    }

    fn show_ssh_interaction_error(
        &mut self,
        id: &str,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let mut lines = tab.terminal_lines.as_ref().clone();
        lines.push(forward::TerminalLine {
            text: format!("SSH 交互错误：{}", message.into()),
            styles: Vec::new(),
            cursor_column: None,
        });
        if lines.len() > self.terminal_history_lines {
            lines.drain(..lines.len() - self.terminal_history_lines);
        }
        tab.terminal_lines = Arc::new(lines);
        tab.terminal_scroll.scroll_to_item(
            tab.terminal_lines.len().saturating_sub(1),
            ScrollStrategy::Bottom,
        );
        cx.notify();
    }

    pub(super) fn send_ssh_keystroke(
        &mut self,
        id: &str,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let copy_shortcut = if cfg!(target_os = "macos") {
            event.keystroke.modifiers.platform
                && !event.keystroke.modifiers.control
                && event.keystroke.key.eq_ignore_ascii_case("c")
        } else {
            event.keystroke.modifiers.control
                && event.keystroke.modifiers.shift
                && event.keystroke.key.eq_ignore_ascii_case("c")
        };
        if copy_shortcut && self.copy_ssh_terminal_selection(id, cx) {
            return true;
        }
        let Some(tab) = self.ssh_tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return false;
        };
        let paste_shortcut = if cfg!(target_os = "macos") {
            event.keystroke.modifiers.platform
                && !event.keystroke.modifiers.control
                && event.keystroke.key.eq_ignore_ascii_case("v")
        } else {
            event.keystroke.modifiers.control
                && event.keystroke.modifiers.shift
                && event.keystroke.key.eq_ignore_ascii_case("v")
        };
        if paste_shortcut {
            let Some(text) = cx
                .read_from_clipboard()
                .and_then(|clipboard| clipboard.text())
            else {
                return true;
            };
            if let Err(error) = terminal.send_paste(&text) {
                self.show_ssh_interaction_error(id, format!("粘贴失败：{error:#}"), cx);
            }
            return true;
        }
        let Some(bytes) = terminal_key_bytes(
            &event.keystroke,
            terminal.application_cursor(),
            event.prefer_character_input,
        ) else {
            return false;
        };
        if let Err(error) = terminal.send_bytes(bytes) {
            self.show_ssh_interaction_error(id, format!("输入失败：{error:#}"), cx);
            return false;
        }
        true
    }

    pub(super) fn begin_ssh_terminal_selection(
        &mut self,
        id: &str,
        line: usize,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let point = TerminalPoint { line, column };
        tab.terminal_selection = Some(TerminalSelection {
            anchor: point,
            cursor: point,
        });
        tab.terminal_selecting = true;
        cx.notify();
    }

    pub(super) fn update_ssh_terminal_selection(
        &mut self,
        id: &str,
        line: usize,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .ssh_tabs
            .iter_mut()
            .find(|tab| tab.id == id && tab.terminal_selecting)
        else {
            return;
        };
        if let Some(selection) = &mut tab.terminal_selection {
            selection.cursor = TerminalPoint { line, column };
            cx.notify();
        }
    }

    pub(super) fn finish_ssh_terminal_selection(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_selecting = false;
        cx.notify();
    }

    pub(super) fn copy_ssh_terminal_selection(&self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.ssh_tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        let Some(selection) = tab.terminal_selection else {
            return false;
        };
        let Some(text) = terminal_selected_text(&tab.terminal_lines, selection) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub(super) fn toggle_ssh_terminal_search(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_search_open = !tab.terminal_search_open;
        if tab.terminal_search_open {
            tab.terminal_search
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            tab.terminal_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn navigate_ssh_terminal_search(
        &mut self,
        id: &str,
        direction: i32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let query = tab.terminal_search.read(cx).value().to_string();
        let matches = terminal_search_matches(&tab.terminal_lines, &query);
        if matches.is_empty() {
            tab.terminal_search_index = None;
            cx.notify();
            return;
        }
        let next = match (tab.terminal_search_index, direction.is_negative()) {
            (None, false) => 0,
            (None, true) => matches.len() - 1,
            (Some(current), false) => (current + 1) % matches.len(),
            (Some(current), true) => current.checked_sub(1).unwrap_or(matches.len() - 1),
        };
        tab.terminal_search_index = Some(next);
        tab.terminal_scroll
            .scroll_to_item(matches[next].line, ScrollStrategy::Center);
        cx.notify();
    }

    fn record_command_history(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        remember_command(&mut self.command_history, command);
        if let Err(error) = self.persist() {
            self.push_message(format!("历史命令保存失败：{error:#}"), window, cx);
        }
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

    pub(super) fn terminal_history_lines(&self) -> usize {
        self.terminal_history_lines
    }

    pub(super) fn set_terminal_history_lines(&mut self, lines: usize, cx: &mut Context<Self>) {
        let lines = lines.clamp(
            forward::MIN_TERMINAL_HISTORY_LINES,
            forward::MAX_TERMINAL_HISTORY_LINES,
        );
        if self.terminal_history_lines == lines {
            return;
        }
        let previous = self.terminal_history_lines;
        self.terminal_history_lines = lines;
        for tab in &self.ssh_tabs {
            if let Some(terminal) = &tab.terminal {
                terminal.set_history_limit(lines);
            }
        }
        if storage::save(&self.app_config()).is_err() {
            self.terminal_history_lines = previous;
            for tab in &self.ssh_tabs {
                if let Some(terminal) = &tab.terminal {
                    terminal.set_history_limit(previous);
                }
            }
        }
        cx.notify();
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
            self.show_hint("快捷命令名称和具体命令均不能为空", window, cx);
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let Some(terminal) = &tab.terminal {
            terminal.clear_output();
        }
        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
            text: "终端输出已清空".into(),
            styles: Vec::new(),
            cursor_column: None,
        }]);
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
            self.push_message("服务器配置不存在，无法重连", window, cx);
            return;
        };
        tab.terminal = None;
        tab.state = SshConnectionState::Connecting;
        tab.terminal_size.set((0, 0));
        tab.terminal_output_revision = 0;
        tab.terminal_last_output_sync = Instant::now() - SSH_OUTPUT_FRAME_INTERVAL;
        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
            text: "正在重新建立 SSH 连接…".into(),
            styles: Vec::new(),
            cursor_column: None,
        }]);
        let title = tab.title.clone();
        let tab_id = id.to_string();
        self.push_message(format!("正在重连 {title}"), window, cx);
        let terminal_history_lines = self.terminal_history_lines;
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::SshTerminalHandle::start_with_history_limit(
                        host,
                        terminal_history_lines,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                        tab.terminal_focus.focus(window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
                            text: format!("SSH 重连失败：{message}"),
                            styles: Vec::new(),
                            cursor_column: None,
                        }]);
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
            self.push_message("服务器配置不存在，无法读取文件", window, cx);
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

    pub(super) fn sort_ssh_remote_entries(
        &mut self,
        id: &str,
        field: RemoteSortField,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.remote_sort_field == field {
            tab.remote_sort_ascending = !tab.remote_sort_ascending;
        } else {
            tab.remote_sort_field = field;
            tab.remote_sort_ascending = true;
        }
        cx.notify();
    }

    pub(super) fn toggle_ssh_file_panel_view(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.ssh_tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.file_panel_view = match tab.file_panel_view {
            SshFilePanelView::Files => SshFilePanelView::Transfers,
            SshFilePanelView::Transfers => SshFilePanelView::Files,
        };
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
                            this.show_hint(error, window, cx);
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
            self.push_message("服务器配置不存在，无法新建文件", window, cx);
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

    pub(super) fn confirm_delete_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.name == ".." {
            return;
        }
        let view = cx.entity();
        let tab_id = id.to_string();
        let kind = if entry.is_dir { "文件夹" } else { "文件" };
        let warning = if entry.is_dir {
            format!(
                "确定删除远程文件夹“{}”吗？文件夹及其全部内容都会被删除，此操作无法撤销。",
                entry.name
            )
        } else {
            format!("确定删除远程文件“{}”吗？此操作无法撤销。", entry.name)
        };
        window.open_dialog(cx, move |dialog, _, _| {
            let delete_view = view.clone();
            let delete_id = tab_id.clone();
            let delete_entry = entry.clone();
            dialog
                .title(format!("删除{kind}"))
                .w(px(460.))
                .child(div().text_sm().child(warning.clone()))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-delete-remote").outline().label("取消")),
                        )
                        .child(
                            Button::new("confirm-delete-remote")
                                .danger()
                                .label("确认删除")
                                .on_click(move |_, window, cx| {
                                    delete_view.update(cx, |this, cx| {
                                        this.delete_ssh_entry(
                                            &delete_id,
                                            delete_entry.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn delete_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
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
            self.push_message("跳板机配置不存在，无法删除远程文件", window, cx);
            return;
        };
        let remote_dir = tab.remote_path.clone();
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let entry_name = entry.name.clone();
        let entry_path = entry.path.clone();
        let is_dir = entry.is_dir;
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::delete_entry(&host, &entry_path, is_dir) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.push_message(format!("已删除远程项目：{entry_name}"), window, cx);
                    this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                }
                Err(error) => {
                    if let Some(tab) = this.ssh_tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.file_loading = false;
                    }
                    this.push_message(format!("删除远程项目失败：{error:#}"), window, cx);
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
        let Some(tab_index) = self.ssh_tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &self.ssh_tabs[tab_index];
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
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let progress = forward::TransferProgress::default();
        let mut names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .take(3)
            .collect::<Vec<_>>()
            .join("、");
        if paths.len() > 3 {
            names = format!("{names} 等 {} 项", paths.len());
        }
        self.ssh_tabs[tab_index].transfers.insert(
            0,
            SshTransfer {
                id: transfer_id.clone(),
                direction: TransferDirection::Upload,
                title: names,
                progress: progress.clone(),
                status: TransferStatus::Running,
                started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                finished_at: None,
            },
        );
        self.ssh_tabs[tab_index].file_panel_view = SshFilePanelView::Transfers;
        self.push_message(
            format!("正在上传 {} 个项目到 {}", paths.len(), remote_dir),
            window,
            cx,
        );
        cx.spawn_in(window, async move |weak, cx| {
            let remote_dir_for_upload = remote_dir.clone();
            let task_progress = progress.clone();
            let worker = std::thread::Builder::new()
                .name("s-porter-sftp-upload".into())
                .spawn(move || {
                    forward::upload(&host, &remote_dir_for_upload, &paths, &task_progress)
                });
            let result = match worker {
                Ok(worker) => {
                    while !worker.is_finished() {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = weak.update_in(cx, |_, _, cx| cx.notify());
                    }
                    worker
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("上传线程意外终止")))
                }
                Err(error) => Err(anyhow::Error::new(error).context("无法启动上传线程")),
            };
            progress.finish();
            let _ = weak.update_in(cx, |this, window, cx| {
                let cancelled = progress.is_cancelled();
                if let Some(transfer) = this
                    .ssh_tabs
                    .iter_mut()
                    .find(|tab| tab.id == tab_id)
                    .and_then(|tab| {
                        tab.transfers
                            .iter_mut()
                            .find(|transfer| transfer.id == transfer_id)
                    })
                {
                    transfer.finished_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                    transfer.status = match &result {
                        Ok(_) => TransferStatus::Completed,
                        Err(_) if cancelled => TransferStatus::Cancelled,
                        Err(error) => TransferStatus::Failed(format!("{error:#}")),
                    };
                }
                match result {
                    Ok(count) => {
                        this.push_message(format!("上传完成：{count} 个文件"), window, cx);
                        this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                    }
                    Err(_) if cancelled => this.push_message("上传已取消", window, cx),
                    Err(error) => {
                        this.push_message(format!("上传失败：{error:#}"), window, cx);
                    }
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
        let Some(tab_index) = self.ssh_tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &self.ssh_tabs[tab_index];
        let Some(host) = self
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            return;
        };
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let progress = forward::TransferProgress::default();
        self.ssh_tabs[tab_index].transfers.insert(
            0,
            SshTransfer {
                id: transfer_id.clone(),
                direction: TransferDirection::Download,
                title: entry.name.clone(),
                progress: progress.clone(),
                status: TransferStatus::Running,
                started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                finished_at: None,
            },
        );
        self.ssh_tabs[tab_index].file_panel_view = SshFilePanelView::Transfers;
        self.push_message(format!("正在下载 {}", entry.name), window, cx);
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let task_progress = progress.clone();
            let worker = std::thread::Builder::new()
                .name("s-porter-sftp-download".into())
                .spawn(move || {
                    forward::download(&host, &entry.path, entry.is_dir, &target, &task_progress)
                        .map(|count| (count, target))
                });
            let result = match worker {
                Ok(worker) => {
                    while !worker.is_finished() {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = weak.update_in(cx, |_, _, cx| cx.notify());
                    }
                    worker
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("下载线程意外终止")))
                }
                Err(error) => Err(anyhow::Error::new(error).context("无法启动下载线程")),
            };
            progress.finish();
            let _ = weak.update_in(cx, |this, window, cx| {
                let cancelled = progress.is_cancelled();
                if let Some(transfer) = this
                    .ssh_tabs
                    .iter_mut()
                    .find(|tab| tab.id == tab_id)
                    .and_then(|tab| {
                        tab.transfers
                            .iter_mut()
                            .find(|transfer| transfer.id == transfer_id)
                    })
                {
                    transfer.finished_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                    transfer.status = match &result {
                        Ok(_) => TransferStatus::Completed,
                        Err(_) if cancelled => TransferStatus::Cancelled,
                        Err(error) => TransferStatus::Failed(format!("{error:#}")),
                    };
                }
                match result {
                    Ok((count, target)) => this.push_message(
                        format!("下载完成：{count} 个文件，保存到 {}", target.display()),
                        window,
                        cx,
                    ),
                    Err(_) if cancelled => this.push_message("下载已取消", window, cx),
                    Err(error) => {
                        this.push_message(format!("下载失败：{error:#}"), window, cx);
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn cancel_ssh_transfer(
        &mut self,
        tab_id: &str,
        transfer_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(transfer) = self
            .ssh_tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| {
                tab.transfers
                    .iter_mut()
                    .find(|transfer| transfer.id == transfer_id)
            })
        else {
            return;
        };
        if transfer.status == TransferStatus::Running {
            transfer.progress.cancel();
            transfer.status = TransferStatus::Cancelling;
            cx.notify();
        }
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
            self.push_message("关联的服务器配置不存在", window, cx);
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
            self.push_message("关联的服务器配置不存在", window, cx);
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
                            .map(|_| "远端已允许 TCP 转发，sshd 已重载并验证生效".to_string())
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
            Err(error) => self.show_hint(error.to_string(), window, cx),
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
        let sidebar_collapsed = self.sidebar_collapsed;
        let ui_font_size = self.ui_font_size;
        let message_count = self.messages.len();
        let message_search = self.message_search.clone();
        let messages = self.messages.clone();
        let font_size_view = cx.entity();
        let sidebar_content = if sidebar_collapsed {
            None
        } else {
            Some(sidebar::render(self, cx).into_any_element())
        };
        let page_content = match self.page {
            Page::JumpHosts => jump_host_page::render(self, cx),
            Page::Ssh => ssh_page::render(self, cx),
            Page::Forward => forward_page::render(self, cx),
            Page::Crypto => tool_page::render(self, true, cx),
            Page::Codec => tool_page::render(self, false, cx),
            Page::Format => format_page::render(self, cx),
            Page::Time => time_page::render(self, cx),
        };
        let main_content = div().size_full().min_w_0().child(page_content);
        let main_layout = if let Some(sidebar_content) = sidebar_content {
            h_resizable("main-layout")
                .child(
                    resizable_panel()
                        .size(px(196.))
                        .size_range(px(168.)..px(320.))
                        .flex_none()
                        .child(sidebar_content),
                )
                .child(resizable_panel().child(main_content))
                .into_any_element()
        } else {
            main_content.into_any_element()
        };
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
            .child(div().flex_1().min_h_0().child(main_layout))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TerminalPoint, TerminalSelection, parse_jump_host_batch_entries, remember_command,
        terminal_key_bytes, terminal_search_matches, terminal_selected_text,
    };
    use crate::forward::TerminalLine;
    use gpui::Keystroke;

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

    #[test]
    fn parses_batch_jump_hosts_with_supported_separators() {
        let entries = parse_jump_host_batch_entries(
            "生产-01, 10.0.0.11\n生产-02，10.0.0.12\n测试机|ssh.example.com\n预发机\t10.0.0.13\n有空格的 名称 10.0.0.14",
            "",
        )
        .unwrap();
        assert_eq!(
            entries,
            vec![
                ("生产-01".into(), "10.0.0.11".into()),
                ("生产-02".into(), "10.0.0.12".into()),
                ("测试机".into(), "ssh.example.com".into()),
                ("预发机".into(), "10.0.0.13".into()),
                ("有空格的 名称".into(), "10.0.0.14".into()),
            ]
        );
        assert_eq!(
            parse_jump_host_batch_entries("节点一::10.0.0.21", "::").unwrap(),
            vec![("节点一".into(), "10.0.0.21".into())]
        );
    }

    #[test]
    fn rejects_invalid_batch_jump_host_line() {
        let error = parse_jump_host_batch_entries("缺少分隔符", "").unwrap_err();
        assert!(error.to_string().contains("第 1 行格式错误"));
    }

    #[test]
    fn terminal_keys_encode_control_and_full_screen_navigation() {
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("tab").unwrap(), false, true),
            Some(vec![b'\t'])
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("shift-tab").unwrap(), false, true),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("ctrl-c").unwrap(), false, false),
            Some(vec![0x03])
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("escape").unwrap(), false, false),
            Some(vec![0x1b])
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("up").unwrap(), false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("up").unwrap(), true, false),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&Keystroke::parse("ctrl-d").unwrap(), false, false),
            Some(vec![0x04])
        );
    }

    #[test]
    fn terminal_selection_copies_across_lines_without_the_visual_cursor() {
        let lines = vec![
            TerminalLine {
                text: "alpha".into(),
                styles: Vec::new(),
                cursor_column: None,
            },
            TerminalLine {
                text: "beta".into(),
                styles: Vec::new(),
                cursor_column: Some(2),
            },
        ];
        let selection = TerminalSelection {
            anchor: TerminalPoint { line: 0, column: 2 },
            cursor: TerminalPoint { line: 1, column: 4 },
        };

        assert_eq!(
            terminal_selected_text(&lines, selection).as_deref(),
            Some("pha\nbeta")
        );
    }

    #[test]
    fn terminal_selection_preserves_a_real_cursor_like_glyph() {
        let lines = vec![TerminalLine {
            text: "a▏b".into(),
            styles: Vec::new(),
            cursor_column: None,
        }];
        let selection = TerminalSelection {
            anchor: TerminalPoint { line: 0, column: 0 },
            cursor: TerminalPoint { line: 0, column: 3 },
        };

        assert_eq!(
            terminal_selected_text(&lines, selection).as_deref(),
            Some("a▏b")
        );
    }

    #[test]
    fn terminal_search_finds_all_case_insensitive_matches() {
        let lines = vec![
            TerminalLine {
                text: "Ready then READY".into(),
                styles: Vec::new(),
                cursor_column: None,
            },
            TerminalLine {
                text: "not here".into(),
                styles: Vec::new(),
                cursor_column: None,
            },
        ];

        let matches = terminal_search_matches(&lines, "ready");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[0].range, 0..5);
        assert_eq!(matches[1].range, 11..16);
    }
}
