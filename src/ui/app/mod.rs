use self::{
    forms::*,
    ui_state::*,
    workspace_state::{ForwardWorkspace, NavigationState, ServerWorkspace, SshWorkspace},
};
use super::{
    forwarding::page as forward_page,
    server::page as jump_host_page,
    ssh::{page as ssh_page, state::*, terminal::*},
    tools::{
        drawing as drawing_page, format as format_page, text as tool_page, time as time_page,
        update as update_page,
    },
};
use crate::{
    forward::{self, ForwardConfig, HttpProxyConfig, JumpHost},
    storage,
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
    scroll::ScrollableElement,
    table::TableState,
    text::TextView,
    *,
};
use std::time::{Duration, Instant};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

mod forms;
pub(super) mod message_center;
mod sidebar;
pub(super) mod ui_state;
mod workspace_state;

#[path = "controllers/forward_config.rs"]
mod forward_controller;
#[path = "controllers/forward_runtime.rs"]
mod forward_runtime;
#[path = "controllers/server.rs"]
mod jump_host_controller;
#[path = "controllers/sftp.rs"]
mod sftp_controller;
#[path = "controllers/ssh_admin.rs"]
mod ssh_admin_controller;
#[path = "controllers/ssh_session.rs"]
mod ssh_controller;

const DEFAULT_UI_FONT_SIZE: f32 = 16.;
const SSH_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const SSH_OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(30);
pub(super) const UI_FONT_SIZES: [u8; 15] =
    [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22];

struct UpdateAvailableNotification;

fn render_app_title_bar(content: AnyElement, window: &mut Window, cx: &mut App) -> AnyElement {
    if !cfg!(target_os = "windows") {
        return TitleBar::new().child(content).into_any_element();
    }

    let control = |id: &'static str, icon: IconName, area: WindowControlArea| {
        div()
            .id(id)
            .w(px(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .window_control_area(area)
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .child(Icon::new(icon).small())
    };
    let maximize_icon = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };

    h_flex()
        .h(px(34.))
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .bg(cx.theme().tokens.title_bar)
        .child(content)
        .child(
            h_flex()
                .h_full()
                .flex_shrink_0()
                .child(control(
                    "window-minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                ))
                .child(control(
                    "window-maximize",
                    maximize_icon,
                    WindowControlArea::Max,
                ))
                .child(
                    control(
                        "window-close",
                        IconName::WindowClose,
                        WindowControlArea::Close,
                    )
                    .hover(|style| style.bg(cx.theme().danger)),
                ),
        )
        .into_any_element()
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
        let columns = if let Some(separator) = custom_separator {
            line.split(separator).map(str::trim).collect::<Vec<_>>()
        } else {
            ['\t', ',', '，', '|']
                .into_iter()
                .find(|separator| line.contains(*separator))
                .map(|separator| line.split(separator).map(str::trim).collect::<Vec<_>>())
                .unwrap_or_else(|| line.split_whitespace().collect::<Vec<_>>())
        };
        anyhow::ensure!(
            columns.len() == 2,
            "第 {} 行必须且只能包含两列（服务器名称和 SSH 地址），当前识别到 {} 列",
            index + 1,
            columns.len()
        );
        let name = columns[0];
        let host = columns[1];
        anyhow::ensure!(!name.is_empty(), "第 {} 行服务器名称不能为空", index + 1);
        anyhow::ensure!(!host.is_empty(), "第 {} 行 SSH 地址不能为空", index + 1);
        entries.push((name.to_string(), host.to_string()));
    }
    anyhow::ensure!(!entries.is_empty(), "请至少输入一台服务器");
    anyhow::ensure!(entries.len() <= 500, "单次最多批量新增 500 台服务器");
    Ok(entries)
}

pub(super) struct AppView {
    pub(super) navigation: NavigationState,
    pub(super) servers: ServerWorkspace,
    pub(super) forwarding: ForwardWorkspace,
    pub(super) ssh: SshWorkspace,
    pub(super) crypto_tools: Entity<tool_page::ToolState>,
    pub(super) codec_tools: Entity<tool_page::ToolState>,
    pub(super) format_tools: Entity<format_page::FormatToolState>,
    pub(super) time_tools: time_page::TimeToolState,
    pub(super) drawing_tools: drawing_page::DrawingToolState,
    pub(super) message_center: Entity<message_center::MessageCenter>,
    pub(super) distribution: crate::Distribution,
    pub(super) updates: Entity<update_page::UpdateModel>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub(super) fn new(
        distribution: crate::Distribution,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Theme::global_mut(cx).font_size = px(DEFAULT_UI_FONT_SIZE);
        window.set_rem_size(px(DEFAULT_UI_FONT_SIZE));
        let forward_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("正则搜索名称、端口、远程目标或服务器")
        });
        let jump_host_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("正则搜索名称、地址或登录用户"));
        let forward_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("正则搜索服务器"));
        let ssh_host_picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("正则搜索服务器"));
        let jump_host_form = JumpHostForm::new(window, cx);
        let batch_entries = jump_host_form.batch_entries.clone();
        let batch_separator = jump_host_form.batch_separator.clone();
        let message_center = cx.new(|cx| message_center::MessageCenter::new(window, cx));
        let updates = cx.new(|cx| update_page::UpdateModel::new(distribution, window, cx));
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
            cx.subscribe(&batch_entries, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) && this.servers.batch_mode {
                    this.validate_jump_host_batch_entries(false, cx);
                }
            }),
            cx.subscribe(&batch_separator, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) && this.servers.batch_mode {
                    this.validate_jump_host_batch_entries(false, cx);
                }
            }),
            cx.subscribe(&message_center, |_, _, event, cx| {
                if matches!(event, message_center::MessageCenterEvent::HistoryChanged) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&updates, window, |this, _, event, window, cx| match event {
                update_page::UpdateEvent::Available(message) => {
                    this.push_update_notification(message.clone(), window, cx);
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
            navigation: NavigationState {
                page: Page::JumpHosts,
                sidebar_collapsed: false,
                ui_font_size: DEFAULT_UI_FONT_SIZE,
            },
            servers: ServerWorkspace {
                jump_hosts: config.jump_hosts,
                selected_jump_host_id,
                form: jump_host_form,
                form_error: None,
                batch_entries_error: None,
                editing_id: None,
                batch_mode: false,
                search: jump_host_search,
                table: jump_host_table,
                selected: HashSet::new(),
            },
            forwarding: ForwardWorkspace {
                configs: config.forwards,
                tunnels: HashMap::new(),
                form: ForwardForm::new(window, cx),
                form_keep_alive: false,
                editing_id: None,
                host_picker_search: forward_host_picker_search,
                table: forward_table,
                search: forward_search,
                status_filter: ForwardStatusFilter::All,
                states: HashMap::new(),
                startup_logs: HashMap::new(),
                selected: HashSet::new(),
            },
            ssh: SshWorkspace {
                terminal_history_lines,
                host_picker_search: ssh_host_picker_search,
                tabs: Vec::new(),
                active_tab_id: None,
                quick_commands: config.quick_commands,
                command_history,
            },
            crypto_tools: cx.new(|cx| tool_page::ToolState::new(window, cx)),
            codec_tools: cx.new(|cx| tool_page::ToolState::new(window, cx)),
            format_tools: cx.new(|cx| format_page::FormatToolState::new(window, cx)),
            time_tools: time_page::TimeToolState::new(window, cx),
            drawing_tools: drawing_page::DrawingToolState::new(),
            message_center,
            distribution,
            updates,
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
        message_center::push(&self.message_center, message, window, cx);
    }

    fn push_update_notification(
        &mut self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.message_center
            .update(cx, |center, cx| center.push(message.clone(), cx));
        let view = cx.entity();
        window.push_notification(
            Notification::new()
                .id::<UpdateAvailableNotification>()
                .title("发现新版本")
                .message(message)
                .action(move |_, _, _| {
                    let view = view.clone();
                    Button::new("open-application-update")
                        .primary()
                        .label("前往更新")
                        .on_click(move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.navigation.page = Page::Update;
                                cx.notify();
                            });
                            window.remove_notification::<UpdateAvailableNotification>(cx);
                        })
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
        message_center::show_hint(message, window, cx);
    }

    fn set_ui_font_size(&mut self, font_size: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.ui_font_size = font_size;
        Theme::global_mut(cx).font_size = px(font_size);
        window.set_rem_size(px(font_size));
        window.refresh();
        cx.notify();
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar_collapsed = self.navigation.sidebar_collapsed;
        let ui_font_size = self.navigation.ui_font_size;
        let message_center = self.message_center.clone();
        let message_count = message_center.read(cx).len();
        let font_size_view = cx.entity();
        let sidebar_content = if sidebar_collapsed {
            None
        } else {
            Some(sidebar::render(self, cx).into_any_element())
        };
        let page_content = match self.navigation.page {
            Page::JumpHosts => jump_host_page::render(self, window, cx),
            Page::Ssh => ssh_page::render(self, cx),
            Page::Forward => forward_page::render(self, window, cx),
            Page::Crypto => {
                tool_page::render(self.crypto_tools.clone(), message_center.clone(), true, cx)
            }
            Page::Codec => {
                tool_page::render(self.codec_tools.clone(), message_center.clone(), false, cx)
            }
            Page::Format => {
                format_page::render(self.format_tools.clone(), message_center.clone(), cx)
            }
            Page::Time => time_page::render(self, cx),
            Page::Drawing => drawing_page::render(&mut self.drawing_tools, window, cx),
            Page::Update => update_page::render(self.updates.clone(), self.distribution, cx),
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
            .child(render_app_title_bar(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px_3()
                    .gap_2()
                    .justify_between()
                    .child(
                        h_flex()
                            .flex_1()
                            .h_full()
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
                                        this.navigation.sidebar_collapsed =
                                            !this.navigation.sidebar_collapsed;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .window_control_area(WindowControlArea::Drag)
                                    .child("S Porter"),
                            ),
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
                                                            ui_font_size == f32::from(font_size),
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
                                        let message_center = message_center.clone();
                                        window.open_sheet(cx, move |sheet, _, cx| {
                                            message_center::render(
                                                sheet,
                                                message_center.clone(),
                                                cx,
                                            )
                                        });
                                    }),
                            ),
                    )
                    .into_any_element(),
                window,
                cx,
            ))
            .child(div().flex_1().min_h_0().child(main_layout))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TerminalPoint, TerminalSelection, is_terminal_copy_shortcut, is_terminal_paste_shortcut,
        parse_jump_host_batch_entries, remember_command, terminal_key_bytes,
        terminal_search_matches, terminal_selected_text,
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
            "生产-01, 10.0.0.11\n生产-02，10.0.0.12\n测试机|ssh.example.com\n预发机\t10.0.0.13\n开发机 10.0.0.14",
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
                ("开发机".into(), "10.0.0.14".into()),
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
        assert!(error.to_string().contains("当前识别到 1 列"));
        let error = parse_jump_host_batch_entries("服务器一\t10.0.0.1\t多余列", "").unwrap_err();
        assert!(error.to_string().contains("当前识别到 3 列"));
    }

    #[test]
    fn terminal_keys_encode_control_and_full_screen_navigation() {
        assert!(is_terminal_copy_shortcut(
            &Keystroke::parse("ctrl-insert").unwrap()
        ));
        assert!(is_terminal_paste_shortcut(
            &Keystroke::parse("shift-insert").unwrap()
        ));
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
