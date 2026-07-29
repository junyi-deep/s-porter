use super::app::{
    AppView, RemoteSortField, SshConnectionState, SshFilePanelView, SshTab, TerminalSelection,
    TransferDirection, TransferStatus, UI_FONT_SIZES,
};
use crate::forward::TransferStage;
use gpui::InteractiveElement as _;
use gpui::StatefulInteractiveElement as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    input::{Input, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    progress::Progress,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement,
    tooltip::Tooltip,
    *,
};
use std::{cell::Cell, path::PathBuf, rc::Rc};
use unicode_width::UnicodeWidthChar as _;

fn open_quick_command_dialog(
    view: Entity<AppView>,
    quick_command: Option<crate::storage::QuickCommand>,
    window: &mut Window,
    cx: &mut App,
) {
    let editing_id = quick_command.as_ref().map(|command| command.id.clone());
    let name_value = quick_command
        .as_ref()
        .map(|command| command.name.clone())
        .unwrap_or_default();
    let command_value = quick_command
        .as_ref()
        .map(|command| command.command.clone())
        .unwrap_or_default();
    let name = cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(name_value)
            .placeholder("例如：查看系统信息")
    });
    let command = cx.new(|cx| {
        InputState::new(window, cx)
            .multi_line(true)
            .rows(5)
            .default_value(command_value)
            .placeholder("输入具体命令")
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let save_view = view.clone();
        let delete_view = view.clone();
        let save_name = name.clone();
        let save_command = command.clone();
        let save_id = editing_id.clone();
        let delete_id = editing_id.clone();
        dialog
            .title(if editing_id.is_some() {
                "编辑快捷命令"
            } else {
                "新增快捷命令"
            })
            .w(px(520.))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().font_medium().child("命令名称"))
                            .child(Input::new(&name)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().font_medium().child("具体命令"))
                            .child(Input::new(&command).font_family("monospace")),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .when_some(delete_id, |footer, id| {
                        footer.child(
                            Button::new("delete-quick-command")
                                .danger()
                                .label("删除")
                                .on_click(move |_, window, cx| {
                                    if delete_view.update(cx, |this, cx| {
                                        this.delete_quick_command(&id, window, cx)
                                    }) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                DialogClose::new().child(
                                    Button::new("cancel-quick-command").outline().label("取消"),
                                ),
                            )
                            .child(
                                Button::new("save-quick-command")
                                    .primary()
                                    .label("保存")
                                    .on_click(move |_, window, cx| {
                                        let name = save_name.read(cx).value().to_string();
                                        let command = save_command.read(cx).value().to_string();
                                        if save_view.update(cx, |this, cx| {
                                            this.save_quick_command(
                                                save_id.as_deref(),
                                                &name,
                                                &command,
                                                window,
                                                cx,
                                            )
                                        }) {
                                            window.close_dialog(cx);
                                        }
                                    }),
                            ),
                    ),
            )
    });
}

fn open_connection_dialog(
    view: Entity<AppView>,
    hosts: Vec<crate::forward::JumpHost>,
    search: Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("选择服务器")
            .w(px(440.))
            .content({
                let view = view.clone();
                let hosts = hosts.clone();
                let search = search.clone();
                move |content, _, cx| {
                    let connect_view = view.clone();
                    content.child(super::jump_host_picker::render(
                        "ssh-host-picker",
                        &hosts,
                        &search,
                        None,
                        move |host_id, window, cx| {
                            connect_view.update(cx, |this, cx| {
                                this.open_ssh_connection(&host_id, window, cx)
                            });
                            window.close_dialog(cx);
                        },
                        cx,
                    ))
                }
            })
            .footer(DialogFooter::new().child(
                DialogClose::new().child(Button::new("cancel-ssh-connect").outline().label("取消")),
            ))
    });
}

fn render_tabs(
    view_state: &AppView,
    view: &Entity<AppView>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let active_id = view_state.active_ssh_tab_id.clone();
    let tabs = view_state.ssh_tabs.iter().map(|tab| {
        let activate_view = view.clone();
        let close_view = view.clone();
        let close_menu_view = view.clone();
        let close_others_view = view.clone();
        let close_all_view = view.clone();
        let activate_id = tab.id.clone();
        let close_id = tab.id.clone();
        let close_menu_id = tab.id.clone();
        let close_others_id = tab.id.clone();
        let is_active = active_id.as_deref() == Some(tab.id.as_str());
        let tab_text_color = if is_active {
            cx.theme().button_primary_foreground
        } else {
            cx.theme().secondary_foreground
        };
        h_flex()
            .gap_0()
            .rounded_md()
            .border_1()
            .border_color(if is_active {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .when(is_active, |tab| tab.bg(cx.theme().primary))
            .overflow_hidden()
            .context_menu(move |menu, _, _| {
                menu.item(PopupMenuItem::new("关闭").on_click({
                    let view = close_menu_view.clone();
                    let id = close_menu_id.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| this.close_ssh_tab(&id, cx));
                    }
                }))
                .item(PopupMenuItem::new("关闭其它").on_click({
                    let view = close_others_view.clone();
                    let id = close_others_id.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| this.close_other_ssh_tabs(&id, cx));
                    }
                }))
                .separator()
                .item(PopupMenuItem::new("关闭所有").on_click({
                    let view = close_all_view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| this.close_all_ssh_tabs(cx));
                    }
                }))
            })
            .child(
                Button::new(format!("ssh-tab-{}", tab.id))
                    .small()
                    .ghost()
                    .text_color(tab_text_color)
                    .label(tab.title.clone())
                    .on_click(move |_, _, cx| {
                        activate_view.update(cx, |this, cx| {
                            this.activate_ssh_tab(activate_id.clone(), cx)
                        });
                    }),
            )
            .child(
                Button::new(format!("ssh-tab-close-{}", tab.id))
                    .xsmall()
                    .ghost()
                    .text_color(tab_text_color)
                    .icon(IconName::Close)
                    .tooltip("关闭连接；右键页签可关闭其它或全部")
                    .on_click(move |_, _, cx| {
                        close_view.update(cx, |this, cx| this.close_ssh_tab(&close_id, cx));
                    }),
            )
    });
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .overflow_x_scrollbar()
        .children(tabs)
        .into_any_element()
}

fn terminal_rgb(color: [u8; 3]) -> Hsla {
    rgb((u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2])).into()
}

fn terminal_highlight(style: crate::forward::TerminalTextStyle) -> HighlightStyle {
    let default_foreground = Some(rgb(0x111827).into());
    let default_background = Some(rgb(0xffffff).into());
    let (color, background_color) = if style.inverse {
        (
            style.background.map(terminal_rgb).or(default_background),
            style.foreground.map(terminal_rgb).or(default_foreground),
        )
    } else {
        (
            style.foreground.map(terminal_rgb),
            style.background.map(terminal_rgb),
        )
    };
    HighlightStyle {
        color,
        background_color,
        font_weight: style.bold.then_some(FontWeight::BOLD),
        font_style: style.italic.then_some(FontStyle::Italic),
        underline: style.underline.then_some(UnderlineStyle {
            thickness: px(1.),
            color,
            wavy: false,
        }),
        fade_out: style.dim.then_some(0.45),
        ..HighlightStyle::default()
    }
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

fn terminal_selection_range(
    line_index: usize,
    text: &str,
    selection: Option<TerminalSelection>,
) -> Option<std::ops::Range<usize>> {
    let selection = selection?;
    let (start, end) = if selection.anchor <= selection.cursor {
        (selection.anchor, selection.cursor)
    } else {
        (selection.cursor, selection.anchor)
    };
    if line_index < start.line || line_index > end.line {
        return None;
    }
    let start_column = if line_index == start.line {
        start.column
    } else {
        0
    };
    let end_column = if line_index == end.line {
        end.column
    } else {
        terminal_display_width(text)
    };
    let range = terminal_byte_offset(text, start_column)..terminal_byte_offset(text, end_column);
    (!range.is_empty()).then_some(range)
}

const TERMINAL_CURSOR_GLYPH: char = '▏';

fn terminal_text_with_cursor(
    line: &crate::forward::TerminalLine,
) -> (String, Option<std::ops::Range<usize>>) {
    let Some(cursor_column) = line.cursor_column else {
        return (line.text.clone(), None);
    };
    let cursor_offset = terminal_byte_offset(&line.text, cursor_column);
    let mut text = line.text.clone();
    text.insert(cursor_offset, TERMINAL_CURSOR_GLYPH);
    let cursor_end = cursor_offset + TERMINAL_CURSOR_GLYPH.len_utf8();
    (text, Some(cursor_offset..cursor_end))
}

fn terminal_display_range(
    range: std::ops::Range<usize>,
    cursor_range: Option<&std::ops::Range<usize>>,
) -> std::ops::Range<usize> {
    let Some(cursor) = cursor_range else {
        return range;
    };
    let cursor_len = cursor.len();
    let start = range.start + usize::from(range.start >= cursor.start) * cursor_len;
    let end = range.end + usize::from(range.end > cursor.start) * cursor_len;
    start..end
}

fn terminal_column(
    position_x: Pixels,
    content_left: f32,
    font_size: f32,
    line: &crate::forward::TerminalLine,
    window: &mut Window,
) -> usize {
    let (display_text, _) = terminal_text_with_cursor(line);
    let text = SharedString::from(display_text);
    let shaped = window.text_system().shape_line(
        text.clone(),
        px(font_size),
        &[TextRun {
            len: text.len(),
            font: font("monospace"),
            color: transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let x = px((f32::from(position_x) - content_left - 8.).max(0.));
    let byte_offset = shaped.closest_index_for_x(x).min(text.len());
    let visual_column = terminal_display_width(&text[..byte_offset]);
    match line.cursor_column {
        Some(cursor_column) if visual_column > cursor_column => visual_column - 1,
        _ => visual_column,
    }
}

#[derive(Clone, Copy, Debug)]
struct TerminalScrollbarMetrics {
    viewport_height: f32,
    thumb_height: f32,
    thumb_top: f32,
    max_scroll: f32,
}

fn terminal_scrollbar_metrics(
    line_count: usize,
    line_height: f32,
    viewport_height: f32,
    offset_y: f32,
) -> Option<TerminalScrollbarMetrics> {
    let viewport_height = viewport_height.max(line_height);
    let content_height = line_count.max(1) as f32 * line_height;
    if content_height <= viewport_height {
        return None;
    }
    let max_scroll = content_height - viewport_height;
    let thumb_height =
        (viewport_height * viewport_height / content_height).clamp(48., viewport_height);
    let thumb_travel = (viewport_height - thumb_height).max(0.);
    let scroll_offset = (-offset_y).clamp(0., max_scroll);
    let thumb_top = if max_scroll <= f32::EPSILON {
        0.
    } else {
        scroll_offset / max_scroll * thumb_travel
    };
    Some(TerminalScrollbarMetrics {
        viewport_height,
        thumb_height,
        thumb_top,
        max_scroll,
    })
}

fn set_terminal_scroll_from_thumb(
    handle: &UniformListScrollHandle,
    metrics: TerminalScrollbarMetrics,
    thumb_top: f32,
    track_height: f32,
) {
    let scale = if metrics.viewport_height <= f32::EPSILON {
        1.
    } else {
        track_height / metrics.viewport_height
    };
    let thumb_height = (metrics.thumb_height * scale).clamp(0., track_height);
    let thumb_travel = (track_height - thumb_height).max(0.);
    let thumb_top = thumb_top.clamp(0., thumb_travel);
    let scroll_offset = if thumb_travel <= f32::EPSILON {
        0.
    } else {
        thumb_top / thumb_travel * metrics.max_scroll
    };
    let base_handle = handle.0.borrow().base_handle.clone();
    let current_offset = base_handle.offset();
    base_handle.set_offset(point(current_offset.x, px(-scroll_offset)));
}

fn render_terminal(
    tab: &SshTab,
    quick_commands: &[crate::storage::QuickCommand],
    global_font_size: f32,
    terminal_history_lines: usize,
    view: &Entity<AppView>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let is_running = tab
        .terminal
        .as_ref()
        .is_some_and(|terminal| terminal.is_running());
    let (status, status_color) = match &tab.state {
        SshConnectionState::Connecting => ("连接中", cx.theme().warning),
        SshConnectionState::Connected if is_running => ("已连接", cx.theme().success),
        SshConnectionState::Connected => ("已断开", cx.theme().danger),
        SshConnectionState::Failed(_) => ("连接失败", cx.theme().danger),
    };
    let clear_view = view.clone();
    let reconnect_view = view.clone();
    let files_view = view.clone();
    let search_view = view.clone();
    let search_previous_view = view.clone();
    let search_next_view = view.clone();
    let clear_id = tab.id.clone();
    let reconnect_id = tab.id.clone();
    let files_id = tab.id.clone();
    let search_id = tab.id.clone();
    let search_previous_id = tab.id.clone();
    let search_next_id = tab.id.clone();
    let terminal_input_view = view.clone();
    let terminal_input_id = tab.id.clone();
    let terminal_select_view = view.clone();
    let terminal_select_id = tab.id.clone();
    let terminal_finish_view = view.clone();
    let terminal_finish_id = tab.id.clone();
    let terminal_copy_view = view.clone();
    let terminal_copy_id = tab.id.clone();
    let terminal_focus = tab.terminal_focus.clone();
    let terminal_control: Option<crate::forward::SshTerminalControl> =
        tab.terminal.as_ref().map(|terminal| terminal.control());
    let terminal_size_state = tab.terminal_size.clone();
    let terminal_viewport_height = tab.terminal_viewport_height.clone();
    let terminal_content_left = tab.terminal_content_left.clone();
    let terminal_left_for_resize = terminal_content_left.clone();
    let terminal_left_for_lines = terminal_content_left.clone();
    let font_size_view = view.clone();
    let font_size_tab_id = tab.id.clone();
    let history_lines_view = view.clone();
    let custom_font_size = tab.terminal_font_size;
    let terminal_font_size = custom_font_size.unwrap_or(global_font_size);
    let terminal_selection = tab.terminal_selection;
    let terminal_scroll_for_selection = tab.terminal_scroll.clone();
    let terminal_search_query = tab.terminal_search.read(cx).value().to_string();
    let terminal_search_matches = std::sync::Arc::new(super::app::terminal_search_matches(
        &tab.terminal_lines,
        &terminal_search_query,
    ));
    let terminal_search_index = tab
        .terminal_search_index
        .filter(|index| *index < terminal_search_matches.len());
    let active_terminal_search_match = terminal_search_index
        .and_then(|index| terminal_search_matches.get(index))
        .cloned();
    let terminal_search_status = if terminal_search_matches.is_empty() {
        "0/0".to_string()
    } else {
        format!(
            "{}/{}",
            terminal_search_index.map(|index| index + 1).unwrap_or(0),
            terminal_search_matches.len()
        )
    };
    let add_quick_command_view = view.clone();
    let quick_command_buttons = quick_commands.iter().map(|quick_command| {
        let fill_view = view.clone();
        let edit_view = view.clone();
        let fill_tab_id = tab.id.clone();
        let command = quick_command.command.clone();
        let edit_command = quick_command.clone();
        h_flex()
            .flex_none()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(
                Button::new(format!("quick-command-{}", quick_command.id))
                    .small()
                    .ghost()
                    .label(quick_command.name.clone())
                    .tooltip(quick_command.command.clone())
                    .on_click(move |_, window, cx| {
                        fill_view.update(cx, |this, cx| {
                            this.run_ssh_quick_command(&fill_tab_id, &command, window, cx)
                        });
                    }),
            )
            .child(
                Button::new(format!("edit-quick-command-{}", quick_command.id))
                    .xsmall()
                    .ghost()
                    .icon(IconName::Settings2)
                    .tooltip("编辑快捷命令")
                    .on_click(move |_, window, cx| {
                        open_quick_command_dialog(
                            edit_view.clone(),
                            Some(edit_command.clone()),
                            window,
                            cx,
                        );
                    }),
            )
    });

    v_flex()
        .size_full()
        .min_w_0()
        .min_h_0()
        .child(
            h_flex()
                .h(px(38.))
                .px_3()
                .justify_between()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(tab.title.clone()))
                        .child(div().text_xs().text_color(status_color).child(status)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .when(tab.terminal_search_open, |actions| {
                            actions
                                .child(
                                    div().w(px(220.)).child(
                                        Input::new(&tab.terminal_search)
                                            .prefix(Icon::new(IconName::Search).small()),
                                    ),
                                )
                                .child(
                                    div()
                                        .min_w(px(42.))
                                        .text_center()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(terminal_search_status),
                                )
                                .child(
                                    Button::new(format!("ssh-search-previous-{}", tab.id))
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::ArrowUp)
                                        .tooltip("上一个匹配项（Shift+Enter）")
                                        .on_click(move |_, window, cx| {
                                            search_previous_view.update(cx, |this, cx| {
                                                this.navigate_ssh_terminal_search(
                                                    &search_previous_id,
                                                    -1,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }),
                                )
                                .child(
                                    Button::new(format!("ssh-search-next-{}", tab.id))
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::ArrowDown)
                                        .tooltip("下一个匹配项（Enter）")
                                        .on_click(move |_, window, cx| {
                                            search_next_view.update(cx, |this, cx| {
                                                this.navigate_ssh_terminal_search(
                                                    &search_next_id,
                                                    1,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }),
                                )
                        })
                        .child(
                            Button::new(format!("ssh-search-toggle-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .icon(if tab.terminal_search_open {
                                    IconName::Close
                                } else {
                                    IconName::Search
                                })
                                .tooltip(if tab.terminal_search_open {
                                    "关闭搜索"
                                } else {
                                    "搜索终端内容"
                                })
                                .on_click(move |_, window, cx| {
                                    search_view.update(cx, |this, cx| {
                                        this.toggle_ssh_terminal_search(&search_id, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new(format!("ssh-font-size-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .label(format!("字号 {terminal_font_size:.0}px"))
                                .dropdown_caret(true)
                                .tooltip(if custom_font_size.is_some() {
                                    "SSH 输出内容使用独立字号"
                                } else {
                                    "SSH 输出内容跟随全局字号"
                                })
                                .dropdown_menu(move |menu, _, _| {
                                    let follow_view = font_size_view.clone();
                                    let follow_id = font_size_tab_id.clone();
                                    UI_FONT_SIZES.into_iter().fold(
                                        menu.item(
                                            PopupMenuItem::new(format!(
                                                "跟随全局（{global_font_size:.0}px）"
                                            ))
                                            .checked(custom_font_size.is_none())
                                            .on_click(move |_, _, cx| {
                                                follow_view.update(cx, |this, cx| {
                                                    this.set_ssh_terminal_font_size(
                                                        &follow_id, None, cx,
                                                    )
                                                });
                                            }),
                                        )
                                        .separator(),
                                        |menu, font_size| {
                                            let view = font_size_view.clone();
                                            let id = font_size_tab_id.clone();
                                            menu.item(
                                                PopupMenuItem::new(format!("{font_size}px"))
                                                    .checked(
                                                        custom_font_size
                                                            == Some(f32::from(font_size)),
                                                    )
                                                    .on_click(move |_, _, cx| {
                                                        view.update(cx, |this, cx| {
                                                            this.set_ssh_terminal_font_size(
                                                                &id,
                                                                Some(f32::from(font_size)),
                                                                cx,
                                                            )
                                                        });
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .child(
                            Button::new(format!("ssh-history-lines-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .label(format!("保留 {terminal_history_lines} 行"))
                                .dropdown_caret(true)
                                .tooltip("设置 SSH 交互信息保留行数")
                                .dropdown_menu(move |menu, _, _| {
                                    [100, 500, 1_000, 2_000, 5_000, 10_000].into_iter().fold(
                                        menu,
                                        |menu, lines| {
                                            let view = history_lines_view.clone();
                                            menu.item(
                                                PopupMenuItem::new(format!("{lines} 行"))
                                                    .checked(terminal_history_lines == lines)
                                                    .on_click(move |_, _, cx| {
                                                        view.update(cx, |this, cx| {
                                                            this.set_terminal_history_lines(
                                                                lines, cx,
                                                            )
                                                        });
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .child(
                            Button::new(format!("ssh-reconnect-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .icon(IconName::Redo)
                                .tooltip("重新连接")
                                .disabled(matches!(tab.state, SshConnectionState::Connecting))
                                .on_click(move |_, window, cx| {
                                    reconnect_view.update(cx, |this, cx| {
                                        this.reconnect_ssh_tab(&reconnect_id, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new(format!("ssh-clear-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .icon(IconName::Delete)
                                .label("清屏")
                                .tooltip("清空当前 SSH 交互内容")
                                .on_click(move |_, window, cx| {
                                    clear_view.update(cx, |this, cx| {
                                        this.clear_ssh_terminal(&clear_id, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new(format!("ssh-files-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .icon(if tab.file_panel_open {
                                    IconName::PanelRightClose
                                } else {
                                    IconName::PanelRightOpen
                                })
                                .tooltip(if tab.file_panel_open {
                                    "收起远程文件"
                                } else {
                                    "展开远程文件"
                                })
                                .on_click(move |_, window, cx| {
                                    files_view.update(cx, |this, cx| {
                                        this.toggle_ssh_file_panel(&files_id, window, cx)
                                    });
                                }),
                        ),
                ),
        )
        .child(div().flex_1().min_h_0().p_2().bg(rgb(0xffffff)).child({
            let terminal_lines = tab.terminal_lines.clone();
            let line_count = terminal_lines.len();
            let line_height = terminal_font_size * 1.45;
            let viewport_height = tab
                .terminal_viewport_height
                .get()
                .max(f32::from(tab.terminal_size.get().1) * line_height);
            let terminal_offset_y =
                f32::from(tab.terminal_scroll.0.borrow().base_handle.offset().y);
            let scrollbar_metrics = terminal_scrollbar_metrics(
                line_count,
                line_height,
                viewport_height,
                terminal_offset_y,
            );
            let scrollbar_handle_down = tab.terminal_scroll.clone();
            let scrollbar_handle_move = tab.terminal_scroll.clone();
            let scrollbar_track_bounds = Rc::new(Cell::new((0_f32, viewport_height)));
            let scrollbar_track_bounds_for_paint = scrollbar_track_bounds.clone();
            let scrollbar_track_bounds_for_down = scrollbar_track_bounds.clone();
            let scrollbar_track_bounds_for_move = scrollbar_track_bounds.clone();
            let scrollbar_grab_offset = Rc::new(Cell::new(0_f32));
            let scrollbar_grab_offset_for_down = scrollbar_grab_offset.clone();
            let scrollbar_grab_offset_for_move = scrollbar_grab_offset.clone();
            let terminal_search_matches = terminal_search_matches.clone();
            let terminal_selection_color = cx.theme().selection.opacity(0.65);
            let scrollbar_track_color = cx.theme().scrollbar;
            let scrollbar_border_color = cx.theme().border;
            let scrollbar_thumb_color = cx.theme().tokens.scrollbar_thumb;
            let scrollbar_thumb_hover_color = cx.theme().tokens.scrollbar_thumb_hover;
            div()
                .id(format!("ssh-terminal-input-{}", tab.id))
                .relative()
                .size_full()
                .min_h_0()
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .overflow_hidden()
                .focusable()
                .track_focus(&tab.terminal_focus)
                .key_context("SshTerminal")
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    terminal_focus.focus(window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    let handled = terminal_input_view.update(cx, |this, cx| {
                        this.send_ssh_keystroke(&terminal_input_id, event, window, cx)
                    });
                    if handled {
                        cx.stop_propagation();
                    }
                })
                .on_prepaint(move |bounds, _, _| {
                    terminal_left_for_resize.set(f32::from(bounds.origin.x));
                    terminal_viewport_height.set(f32::from(bounds.size.height));
                    let Some(control) = terminal_control.as_ref() else {
                        return;
                    };
                    let width = (f32::from(bounds.size.width) - 16.).max(1.);
                    let height = (f32::from(bounds.size.height) - 16.).max(1.);
                    let cols = (width / (terminal_font_size * 0.62)).floor() as u16;
                    let rows = (height / (terminal_font_size * 1.45)).floor() as u16;
                    let size = (cols.max(20), rows.max(5));
                    if terminal_size_state.get() != size {
                        terminal_size_state.set(size);
                        control.resize(size.0, size.1);
                    }
                })
                .child(
                    uniform_list(
                        format!("ssh-terminal-lines-{}", tab.id),
                        line_count,
                        move |range, _, _| {
                            range
                                .map(|index| {
                                    let line = &terminal_lines[index];
                                    let (display_text, cursor_range) =
                                        terminal_text_with_cursor(line);
                                    let base_highlights = line.styles.iter().map(|span| {
                                        (
                                            terminal_display_range(
                                                span.range.clone(),
                                                cursor_range.as_ref(),
                                            ),
                                            terminal_highlight(span.style),
                                        )
                                    });
                                    let search_highlights = terminal_search_matches
                                        .iter()
                                        .filter(|matched| matched.line == index)
                                        .map(|matched| {
                                            let is_active = active_terminal_search_match
                                                .as_ref()
                                                .is_some_and(|active| active == matched);
                                            (
                                                terminal_display_range(
                                                    matched.range.clone(),
                                                    cursor_range.as_ref(),
                                                ),
                                                HighlightStyle {
                                                    background_color: Some(
                                                        rgb(if is_active {
                                                            0xf59e0b
                                                        } else {
                                                            0xfde68a
                                                        })
                                                        .opacity(if is_active { 0.9 } else { 0.65 })
                                                        .into(),
                                                    ),
                                                    color: is_active
                                                        .then_some(rgb(0x111827).into()),
                                                    ..HighlightStyle::default()
                                                },
                                            )
                                        });
                                    let selected = terminal_selection_range(
                                        index,
                                        &line.text,
                                        terminal_selection,
                                    )
                                    .map(|range| {
                                        (
                                            terminal_display_range(range, cursor_range.as_ref()),
                                            HighlightStyle {
                                                background_color: Some(terminal_selection_color),
                                                ..HighlightStyle::default()
                                            },
                                        )
                                    });
                                    let cursor_highlight = cursor_range.clone().map(|range| {
                                        (
                                            range,
                                            HighlightStyle {
                                                color: Some(rgb(0x111827).into()),
                                                ..HighlightStyle::default()
                                            },
                                        )
                                    });
                                    let highlights =
                                        combine_highlights(base_highlights, search_highlights);
                                    let highlights = combine_highlights(highlights, selected);
                                    let highlights =
                                        combine_highlights(highlights, cursor_highlight)
                                            .collect::<Vec<_>>();
                                    let begin_view = terminal_select_view.clone();
                                    let update_view = terminal_select_view.clone();
                                    let select_id = terminal_select_id.clone();
                                    let update_id = terminal_select_id.clone();
                                    let content_left = terminal_left_for_lines.clone();
                                    let update_content_left = terminal_left_for_lines.clone();
                                    let begin_scroll = terminal_scroll_for_selection.clone();
                                    let update_scroll = terminal_scroll_for_selection.clone();
                                    let begin_line = line.clone();
                                    let update_line = line.clone();
                                    div()
                                        .h(px(terminal_font_size * 1.45))
                                        .min_w_full()
                                        .px_2()
                                        .font_family("monospace")
                                        .text_size(px(terminal_font_size))
                                        .line_height(px(terminal_font_size * 1.45))
                                        .text_color(rgb(0x111827))
                                        .whitespace_nowrap()
                                        .cursor_text()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |event, window, cx| {
                                                let column = terminal_column(
                                                    event.position.x,
                                                    content_left.get()
                                                        + f32::from(
                                                            begin_scroll
                                                                .0
                                                                .borrow()
                                                                .base_handle
                                                                .offset()
                                                                .x,
                                                        ),
                                                    terminal_font_size,
                                                    &begin_line,
                                                    window,
                                                );
                                                begin_view.update(cx, |this, cx| {
                                                    this.begin_ssh_terminal_selection(
                                                        &select_id, index, column, cx,
                                                    )
                                                });
                                            },
                                        )
                                        .on_mouse_move(move |event, window, cx| {
                                            if !event.dragging() {
                                                return;
                                            }
                                            let column = terminal_column(
                                                event.position.x,
                                                update_content_left.get()
                                                    + f32::from(
                                                        update_scroll
                                                            .0
                                                            .borrow()
                                                            .base_handle
                                                            .offset()
                                                            .x,
                                                    ),
                                                terminal_font_size,
                                                &update_line,
                                                window,
                                            );
                                            update_view.update(cx, |this, cx| {
                                                this.update_ssh_terminal_selection(
                                                    &update_id, index, column, cx,
                                                )
                                            });
                                        })
                                        .child(
                                            StyledText::new(display_text)
                                                .with_highlights(highlights),
                                        )
                                })
                                .collect::<Vec<_>>()
                        },
                    )
                    .track_scroll(&tab.terminal_scroll)
                    .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                    .pr(px(16.))
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .w(px(16.))
                        .border_l_1()
                        .border_color(scrollbar_border_color)
                        .bg(scrollbar_track_color)
                        .on_prepaint(move |bounds, _, _| {
                            scrollbar_track_bounds_for_paint
                                .set((f32::from(bounds.origin.y), f32::from(bounds.size.height)));
                        })
                        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                            let Some(metrics) = scrollbar_metrics else {
                                return;
                            };
                            let (track_top, track_height) = scrollbar_track_bounds_for_down.get();
                            let scale = track_height / metrics.viewport_height;
                            let thumb_height = metrics.thumb_height * scale;
                            let thumb_top = metrics.thumb_top * scale;
                            let pointer = f32::from(event.position.y) - track_top;
                            let grab_offset =
                                if pointer >= thumb_top && pointer <= thumb_top + thumb_height {
                                    pointer - thumb_top
                                } else {
                                    thumb_height / 2.
                                };
                            scrollbar_grab_offset_for_down.set(grab_offset);
                            set_terminal_scroll_from_thumb(
                                &scrollbar_handle_down,
                                metrics,
                                pointer - grab_offset,
                                track_height,
                            );
                            window.refresh();
                            cx.stop_propagation();
                        })
                        .on_mouse_move(move |event, window, cx| {
                            if !event.dragging() {
                                return;
                            }
                            let Some(metrics) = scrollbar_metrics else {
                                return;
                            };
                            let (track_top, track_height) = scrollbar_track_bounds_for_move.get();
                            let pointer = f32::from(event.position.y) - track_top;
                            set_terminal_scroll_from_thumb(
                                &scrollbar_handle_move,
                                metrics,
                                pointer - scrollbar_grab_offset_for_move.get(),
                                track_height,
                            );
                            window.refresh();
                            cx.stop_propagation();
                        })
                        .when_some(scrollbar_metrics, |track, metrics| {
                            track.child(
                                div()
                                    .absolute()
                                    .right(px(4.))
                                    .top(px(metrics.thumb_top))
                                    .w(px(8.))
                                    .h(px(metrics.thumb_height))
                                    .rounded_full()
                                    .bg(scrollbar_thumb_color)
                                    .hover(move |style| style.bg(scrollbar_thumb_hover_color)),
                            )
                        }),
                )
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    terminal_finish_view.update(cx, |this, cx| {
                        this.finish_ssh_terminal_selection(&terminal_finish_id, cx)
                    });
                })
                .context_menu(move |menu, _, _| {
                    let copy_view = terminal_copy_view.clone();
                    let copy_id = terminal_copy_id.clone();
                    menu.item(
                        PopupMenuItem::new("复制选中内容").on_click(move |_, _, cx| {
                            copy_view.update(cx, |this, cx| {
                                this.copy_ssh_terminal_selection(&copy_id, cx);
                            });
                        }),
                    )
                })
        }))
        .child(
            h_flex()
                .h(px(42.))
                .flex_shrink_0()
                .px_2()
                .gap_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new(format!("add-quick-command-{}", tab.id))
                        .xsmall()
                        .outline()
                        .icon(IconName::Plus)
                        .label("快捷命令")
                        .on_click(move |_, window, cx| {
                            open_quick_command_dialog(
                                add_quick_command_view.clone(),
                                None,
                                window,
                                cx,
                            );
                        }),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .overflow_x_scrollbar()
                        .children(quick_command_buttons),
                ),
        )
        .into_any_element()
}

fn format_size(size: u64) -> String {
    if size < 1_024 {
        format!("{size} B")
    } else if size < 1_048_576 {
        format!("{:.1} KB", size as f64 / 1_024.)
    } else {
        let megabytes = size as f64 / 1_048_576.;
        if megabytes < 1_024. {
            format!("{megabytes:.1} MB")
        } else {
            format!("{:.1} GB", megabytes / 1_024.)
        }
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds} 秒")
    } else if seconds < 3_600 {
        format!("{} 分 {} 秒", seconds / 60, seconds % 60)
    } else {
        format!("{} 小时 {} 分", seconds / 3_600, seconds % 3_600 / 60)
    }
}

fn render_transfer_panel(
    tab: &SshTab,
    view: &Entity<AppView>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let transfers = tab.transfers.iter().map(|transfer| {
        let snapshot = transfer.progress.snapshot();
        let percentage = if snapshot.total_bytes == 0 {
            if transfer.status == TransferStatus::Completed {
                100.
            } else {
                0.
            }
        } else {
            snapshot.transferred_bytes as f32 * 100. / snapshot.total_bytes as f32
        };
        let (status, status_color) = match &transfer.status {
            TransferStatus::Running if snapshot.stage == TransferStage::Scanning => {
                ("扫描中", cx.theme().primary)
            }
            TransferStatus::Running => ("传输中", cx.theme().primary),
            TransferStatus::Cancelling => ("正在取消", cx.theme().warning),
            TransferStatus::Completed => ("已完成", cx.theme().success),
            TransferStatus::Cancelled => ("已取消", cx.theme().muted_foreground),
            TransferStatus::Failed(_) => ("失败", cx.theme().danger),
        };
        let cancel_view = view.clone();
        let cancel_tab_id = tab.id.clone();
        let cancel_transfer_id = transfer.id.clone();
        let speed = snapshot.bytes_per_second();
        let transfer_detail = if snapshot.stage == TransferStage::Scanning {
            "正在扫描文件…".to_string()
        } else if speed > 0. {
            let eta = snapshot
                .remaining_seconds()
                .map(format_duration)
                .unwrap_or_else(|| "-".into());
            format!("{}/s，预计剩余 {eta}", format_size(speed as u64))
        } else {
            "正在准备传输…".to_string()
        };
        let files = snapshot.files.iter().map(|file| {
            let file_percentage = if file.size == 0 {
                if file.completed { 100. } else { 0. }
            } else {
                file.transferred as f32 * 100. / file.size as f32
            };
            v_flex()
                .gap_1()
                .py_1()
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(file.path.clone()),
                        )
                        .child(format!(
                            "{} / {}",
                            format_size(file.transferred),
                            format_size(file.size)
                        )),
                )
                .child(
                    Progress::new(format!("transfer-file-{}-{}", transfer.id, file.path))
                        .xsmall()
                        .value(file_percentage),
                )
        });

        v_flex()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::new(match transfer.direction {
                            TransferDirection::Upload => IconName::ArrowUp,
                            TransferDirection::Download => IconName::ArrowDown,
                        })
                        .small(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(transfer.title.clone()),
                    )
                    .child(div().text_xs().text_color(status_color).child(status))
                    .when(
                        matches!(
                            transfer.status,
                            TransferStatus::Running | TransferStatus::Cancelling
                        ),
                        |row| {
                            row.child(
                                Button::new(format!("cancel-transfer-{}", transfer.id))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Close)
                                    .tooltip("取消传输")
                                    .disabled(transfer.status == TransferStatus::Cancelling)
                                    .on_click(move |_, _, cx| {
                                        cancel_view.update(cx, |this, cx| {
                                            this.cancel_ssh_transfer(
                                                &cancel_tab_id,
                                                &cancel_transfer_id,
                                                cx,
                                            )
                                        });
                                    }),
                            )
                        },
                    ),
            )
            .child(
                Progress::new(format!("transfer-total-{}", transfer.id))
                    .small()
                    .loading(
                        snapshot.files.is_empty()
                            && matches!(
                                transfer.status,
                                TransferStatus::Running | TransferStatus::Cancelling
                            ),
                    )
                    .value(percentage),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} / {}（{percentage:.0}%）",
                        format_size(snapshot.transferred_bytes),
                        format_size(snapshot.total_bytes)
                    )),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(transfer_detail)
                    .child(format!(
                        "开始 {}  结束 {}",
                        transfer.started_at,
                        transfer.finished_at.as_deref().unwrap_or("-")
                    )),
            )
            .when_some(
                match &transfer.status {
                    TransferStatus::Failed(error) => Some(error.clone()),
                    _ => None,
                },
                |card, error| {
                    card.child(div().text_xs().text_color(cx.theme().danger).child(error))
                },
            )
            .when(!snapshot.files.is_empty(), |card| {
                card.child(
                    v_flex()
                        .max_h(px(160.))
                        .overflow_y_scrollbar()
                        .children(files),
                )
            })
    });

    v_flex()
        .flex_1()
        .min_h_0()
        .gap_1()
        .p_2()
        .overflow_y_scrollbar()
        .when(tab.transfers.is_empty(), |panel| {
            panel.child(
                div()
                    .py_6()
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("暂无上传或下载任务"),
            )
        })
        .children(transfers)
        .into_any_element()
}

fn format_modified_time(timestamp: Option<u64>) -> String {
    timestamp
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp as i64, 0))
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "-".into())
}

fn remote_special_rank(name: &str) -> u8 {
    match name {
        "." => 0,
        ".." => 1,
        _ => 2,
    }
}

fn sorted_remote_entries(
    entries: &[crate::forward::RemoteEntry],
    field: RemoteSortField,
    ascending: bool,
) -> Vec<crate::forward::RemoteEntry> {
    let mut entries = entries.to_vec();
    entries.sort_by(|left, right| {
        let left_rank = remote_special_rank(&left.name);
        let right_rank = remote_special_rank(&right.name);
        if left_rank != right_rank {
            return left_rank.cmp(&right_rank);
        }
        if left_rank < 2 {
            return std::cmp::Ordering::Equal;
        }
        let ordering = match field {
            RemoteSortField::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            RemoteSortField::Modified => left
                .modified_at
                .unwrap_or_default()
                .cmp(&right.modified_at.unwrap_or_default())
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
        };
        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    entries
}

fn format_permissions(permissions: Option<u32>) -> String {
    permissions
        .map(|permissions| format!("{:04o}", permissions & 0o7777))
        .unwrap_or_else(|| "-".into())
}

fn drag_target(tab_id: &str, name: &str) -> PathBuf {
    std::env::temp_dir()
        .join("s-porter-downloads")
        .join(tab_id)
        .join(name)
}

fn render_file_panel(
    tab: &SshTab,
    view: &Entity<AppView>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let parent_view = view.clone();
    let drop_view = view.clone();
    let menu_view = view.clone();
    let new_file_view = view.clone();
    let new_directory_view = view.clone();
    let entry_new_file_view = view.clone();
    let entry_new_directory_view = view.clone();
    let entry_upload_view = view.clone();
    let file_view_settings = view.clone();
    let panel_view = view.clone();
    let name_sort_view = view.clone();
    let modified_sort_view = view.clone();
    let parent_id = tab.id.clone();
    let drop_id = tab.id.clone();
    let menu_id = tab.id.clone();
    let new_file_id = tab.id.clone();
    let new_directory_id = tab.id.clone();
    let entry_new_file_id = tab.id.clone();
    let entry_new_directory_id = tab.id.clone();
    let entry_upload_id = tab.id.clone();
    let file_view_settings_id = tab.id.clone();
    let panel_view_id = tab.id.clone();
    let name_sort_id = tab.id.clone();
    let modified_sort_id = tab.id.clone();
    let parent = crate::forward::parent_path(&tab.remote_path);
    let show_file_time = tab.show_file_time;
    let show_file_size = tab.show_file_size;
    let show_file_permissions = tab.show_file_permissions;
    let showing_transfers = tab.file_panel_view == SshFilePanelView::Transfers;
    let transfer_panel = showing_transfers.then(|| render_transfer_panel(tab, view, cx));
    let sorted_entries = sorted_remote_entries(
        &tab.remote_entries,
        tab.remote_sort_field,
        tab.remote_sort_ascending,
    );
    let sort_suffix = |field| {
        if tab.remote_sort_field == field {
            if tab.remote_sort_ascending {
                " ↑"
            } else {
                " ↓"
            }
        } else {
            ""
        }
    };
    let name_sort_label = format!("名称{}", sort_suffix(RemoteSortField::Name));
    let modified_sort_label = format!("修改时间{}", sort_suffix(RemoteSortField::Modified));
    let entries = sorted_entries.iter().map(|entry| {
        let is_parent = entry.name == "..";
        let is_special = entry.name == "." || is_parent;
        let open_view = view.clone();
        let download_view = view.clone();
        let menu_download_view = view.clone();
        let delete_view = view.clone();
        let drag_view = view.clone();
        let open_id = tab.id.clone();
        let download_id = tab.id.clone();
        let menu_download_id = tab.id.clone();
        let delete_id = tab.id.clone();
        let drag_id = tab.id.clone();
        let open_entry = entry.clone();
        let download_entry = entry.clone();
        let menu_entry = entry.clone();
        let delete_entry = entry.clone();
        let drag_entry = entry.clone();
        let copy_path = entry.path.clone();
        let new_file_view = entry_new_file_view.clone();
        let new_directory_view = entry_new_directory_view.clone();
        let upload_view = entry_upload_view.clone();
        let new_file_id = entry_new_file_id.clone();
        let new_directory_id = entry_new_directory_id.clone();
        let upload_id = entry_upload_id.clone();
        let target = drag_target(&tab.id, &entry.name);
        let paths = ExternalPaths(vec![target].into());
        h_flex()
            .id(format!("remote-entry-{}-{}", tab.id, entry.path))
            .h(px(34.))
            .px_2()
            .gap_2()
            .rounded_md()
            .hover(|row| row.bg(cx.theme().muted))
            .cursor_pointer()
            .child(
                Icon::new(if entry.is_dir {
                    IconName::Folder
                } else {
                    IconName::File
                })
                .small(),
            )
            .child(
                div()
                    .id(format!("remote-entry-name-{}-{}", tab.id, entry.path))
                    .flex_1()
                    .min_w_0()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_left()
                    .child(entry.name.clone())
                    .tooltip({
                        let name = entry.name.clone();
                        move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                    }),
            )
            .when(show_file_time, |row| {
                row.child(
                    div()
                        .w(px(146.))
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format_modified_time(entry.modified_at)),
                )
            })
            .when(show_file_size, |row| {
                row.child(
                    div()
                        .w(px(54.))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if entry.is_dir {
                            "-".into()
                        } else {
                            format_size(entry.size)
                        }),
                )
            })
            .when(show_file_permissions, |row| {
                row.child(
                    div()
                        .w(px(54.))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(cx.theme().muted_foreground)
                        .child(format_permissions(entry.permissions)),
                )
            })
            .when(!is_special, |row| {
                row.child(
                    Button::new(format!("download-{}-{}", tab.id, entry.path))
                        .xsmall()
                        .ghost()
                        .icon(IconName::ArrowDown)
                        .tooltip("下载")
                        .on_click(move |_, window, cx| {
                            download_view.update(cx, |this, cx| {
                                this.prompt_ssh_download(
                                    &download_id,
                                    download_entry.clone(),
                                    window,
                                    cx,
                                )
                            });
                        }),
                )
            })
            .when(entry.is_dir, |row| {
                row.on_click(move |_, window, cx| {
                    open_view.update(cx, |this, cx| {
                        this.load_ssh_directory(&open_id, &open_entry.path, window, cx)
                    });
                })
            })
            .when(!is_special, |row| {
                row.on_drag(paths, move |paths: &ExternalPaths, _, window, cx| {
                    drag_view.update(cx, |this, cx| {
                        this.prepare_ssh_drag(&drag_id, drag_entry.clone(), window, cx);
                    });
                    cx.new(|_| paths.clone())
                })
            })
            .map(|row| {
                row.context_menu(move |menu, _, _| {
                    let copy_path = copy_path.clone();
                    let menu = menu.item(PopupMenuItem::new("复制完整路径").on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()));
                        },
                    ));
                    let menu = if is_special {
                        menu
                    } else {
                        let delete_view = delete_view.clone();
                        let delete_id = delete_id.clone();
                        let delete_entry_for_action = delete_entry.clone();
                        menu.separator()
                            .item(PopupMenuItem::new("下载").on_click({
                                let entry = menu_entry.clone();
                                let view = menu_download_view.clone();
                                let id = menu_download_id.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.prompt_ssh_download(&id, entry.clone(), window, cx)
                                    });
                                }
                            }))
                            .separator()
                            .item(
                                PopupMenuItem::new(if delete_entry.is_dir {
                                    "删除文件夹"
                                } else {
                                    "删除文件"
                                })
                                .on_click(move |_, window, cx| {
                                    delete_view.update(cx, |this, cx| {
                                        this.confirm_delete_ssh_entry(
                                            &delete_id,
                                            delete_entry_for_action.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                }),
                            )
                    };
                    menu.separator()
                        .item(PopupMenuItem::new("新建文件夹").on_click({
                            let view = new_directory_view.clone();
                            let id = new_directory_id.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.prompt_create_ssh_entry(&id, true, window, cx)
                                });
                            }
                        }))
                        .item(PopupMenuItem::new("新建文件").on_click({
                            let view = new_file_view.clone();
                            let id = new_file_id.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.prompt_create_ssh_entry(&id, false, window, cx)
                                });
                            }
                        }))
                        .separator()
                        .item(PopupMenuItem::new("上传文件或文件夹").on_click({
                            let view = upload_view.clone();
                            let id = upload_id.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.prompt_ssh_upload(&id, window, cx));
                            }
                        }))
                })
                .into_any_element()
            })
    });

    v_flex()
        .size_full()
        .min_w(px(300.))
        .h_full()
        .border_l_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .h(px(38.))
                .px_2()
                .gap_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new(format!("remote-parent-{}", tab.id))
                        .xsmall()
                        .ghost()
                        .icon(IconName::ArrowLeft)
                        .tooltip("上一级")
                        .disabled(tab.remote_path == "/")
                        .on_click(move |_, window, cx| {
                            parent_view.update(cx, |this, cx| {
                                this.load_ssh_directory(&parent_id, &parent, window, cx)
                            });
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&tab.remote_path_input).font_family("monospace")),
                )
                .child(
                    Button::new(format!("remote-view-{}", tab.id))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Eye)
                        .label("查看")
                        .dropdown_caret(true)
                        .dropdown_menu(move |menu, _, _| {
                            let time_view = file_view_settings.clone();
                            let size_view = file_view_settings.clone();
                            let permissions_view = file_view_settings.clone();
                            let time_id = file_view_settings_id.clone();
                            let size_id = file_view_settings_id.clone();
                            let permissions_id = file_view_settings_id.clone();
                            menu.item(
                                PopupMenuItem::new("显示时间")
                                    .checked(show_file_time)
                                    .on_click(move |_, _, cx| {
                                        time_view.update(cx, |this, cx| {
                                            this.toggle_ssh_file_view(&time_id, "time", cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("显示大小")
                                    .checked(show_file_size)
                                    .on_click(move |_, _, cx| {
                                        size_view.update(cx, |this, cx| {
                                            this.toggle_ssh_file_view(&size_id, "size", cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("显示权限")
                                    .checked(show_file_permissions)
                                    .on_click(move |_, _, cx| {
                                        permissions_view.update(cx, |this, cx| {
                                            this.toggle_ssh_file_view(
                                                &permissions_id,
                                                "permissions",
                                                cx,
                                            )
                                        });
                                    }),
                            )
                        }),
                )
                .child(
                    Button::new(format!("remote-panel-view-{}", tab.id))
                        .xsmall()
                        .ghost()
                        .icon(if showing_transfers {
                            IconName::Folder
                        } else {
                            IconName::ArrowDown
                        })
                        .tooltip(if showing_transfers {
                            "切换到远程文件列表"
                        } else {
                            "切换到上传 / 下载列表"
                        })
                        .on_click(move |_, _, cx| {
                            panel_view.update(cx, |this, cx| {
                                this.toggle_ssh_file_panel_view(&panel_view_id, cx)
                            });
                        }),
                ),
        )
        .when(!showing_transfers, |panel| {
            panel.child(
                h_flex()
                    .h(px(28.))
                    .px_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(16.)).flex_none())
                    .child(
                        Button::new(format!("remote-sort-name-{}", tab.id))
                            .xsmall()
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .label(name_sort_label)
                            .on_click(move |_, _, cx| {
                                name_sort_view.update(cx, |this, cx| {
                                    this.sort_ssh_remote_entries(
                                        &name_sort_id,
                                        RemoteSortField::Name,
                                        cx,
                                    )
                                });
                            }),
                    )
                    .when(show_file_time, |row| {
                        row.child(
                            Button::new(format!("remote-sort-modified-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .w(px(146.))
                                .flex_none()
                                .label(modified_sort_label)
                                .on_click(move |_, _, cx| {
                                    modified_sort_view.update(cx, |this, cx| {
                                        this.sort_ssh_remote_entries(
                                            &modified_sort_id,
                                            RemoteSortField::Modified,
                                            cx,
                                        )
                                    });
                                }),
                        )
                    })
                    .when(show_file_size, |row| {
                        row.child(div().w(px(54.)).flex_none().text_right().child("大小"))
                    })
                    .when(show_file_permissions, |row| {
                        row.child(div().w(px(54.)).flex_none().text_right().child("权限"))
                    })
                    .child(div().w(px(24.)).flex_none()),
            )
        })
        .children(transfer_panel)
        .when(!showing_transfers, |panel| {
            panel.child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .gap_1()
                    .overflow_y_scrollbar()
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style.bg(cx.theme().primary.opacity(0.08))
                    })
                    .on_drop(move |paths: &ExternalPaths, window, cx| {
                        drop_view.update(cx, |this, cx| {
                            this.upload_ssh_paths(&drop_id, paths.paths().to_vec(), window, cx)
                        });
                    })
                    .when(tab.file_loading, |list| {
                        list.child(
                            div()
                                .py_6()
                                .text_center()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("正在读取目录…"),
                        )
                    })
                    .when(
                        !tab.file_loading
                            && tab.file_error.is_none()
                            && tab.remote_entries.is_empty(),
                        |list| {
                            list.child(
                                div()
                                    .py_6()
                                    .text_center()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("目录为空，可拖入文件上传"),
                            )
                        },
                    )
                    .children(entries)
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(40.))
                            .context_menu(move |menu, _, _| {
                                menu.item(PopupMenuItem::new("新建文件夹").on_click({
                                    let view = new_directory_view.clone();
                                    let id = new_directory_id.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.prompt_create_ssh_entry(&id, true, window, cx)
                                        });
                                    }
                                }))
                                .item(PopupMenuItem::new("新建文件").on_click({
                                    let view = new_file_view.clone();
                                    let id = new_file_id.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.prompt_create_ssh_entry(&id, false, window, cx)
                                        });
                                    }
                                }))
                                .separator()
                                .item(
                                    PopupMenuItem::new("上传文件或文件夹").on_click({
                                        let view = menu_view.clone();
                                        let id = menu_id.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.prompt_ssh_upload(&id, window, cx)
                                            });
                                        }
                                    }),
                                )
                            }),
                    ),
            )
        })
        .into_any_element()
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let active_tab = view_state
        .active_ssh_tab_id
        .as_deref()
        .and_then(|id| view_state.ssh_tabs.iter().find(|tab| tab.id == id));
    let connect_view = view.clone();
    let hosts = view_state.jump_hosts.clone();
    let host_search = view_state.ssh_host_picker_search.clone();
    let tabs = render_tabs(view_state, &view, cx);

    v_flex()
        .size_full()
        .child(
            h_flex()
                .h(px(46.))
                .flex_shrink_0()
                .px_3()
                .gap_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(tabs)
                .child(
                    Button::new("new-ssh-connection")
                        .small()
                        .primary()
                        .icon(IconName::Plus)
                        .label("新增连接")
                        .disabled(hosts.is_empty())
                        .tooltip(if hosts.is_empty() {
                            "请先新增服务器"
                        } else {
                            "选择服务器并新建连接"
                        })
                        .on_click(move |_, window, cx| {
                            connect_view.update(cx, |this, cx| {
                                this.clear_ssh_host_picker_search(window, cx)
                            });
                            open_connection_dialog(
                                connect_view.clone(),
                                hosts.clone(),
                                host_search.clone(),
                                window,
                                cx,
                            );
                        }),
                ),
        )
        .child(if let Some(tab) = active_tab {
            if tab.file_panel_open {
                let initial_file_width = 300.
                    + if tab.show_file_time { 150. } else { 0. }
                    + if tab.show_file_size { 58. } else { 0. }
                    + if tab.show_file_permissions { 58. } else { 0. };
                h_resizable(format!("ssh-content-panels-{}", tab.id))
                    .child(
                        resizable_panel().child(div().size_full().min_w_0().min_h_0().child(
                            render_terminal(
                                tab,
                                &view_state.quick_commands,
                                view_state.ui_font_size(),
                                view_state.terminal_history_lines(),
                                &view,
                                cx,
                            ),
                        )),
                    )
                    .child(
                        resizable_panel()
                            .size(px(initial_file_width))
                            .size_range(px(300.)..px(900.))
                            .flex_none()
                            .child(render_file_panel(tab, &view, cx)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(div().size_full().min_w_0().min_h_0().child(render_terminal(
                        tab,
                        &view_state.quick_commands,
                        view_state.ui_font_size(),
                        view_state.terminal_history_lines(),
                        &view,
                        cx,
                    )))
                    .into_any_element()
            }
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(if view_state.jump_hosts.is_empty() {
                    "请先新增服务器"
                } else {
                    "点击右上角“新增连接”"
                })
                .into_any_element()
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        RemoteSortField, set_terminal_scroll_from_thumb, sorted_remote_entries,
        terminal_display_range, terminal_scrollbar_metrics, terminal_text_with_cursor,
    };
    use crate::forward::{RemoteEntry, TerminalLine};
    use gpui::UniformListScrollHandle;

    fn entry(name: &str, modified_at: u64) -> RemoteEntry {
        RemoteEntry {
            name: name.into(),
            path: format!("/{name}"),
            is_dir: false,
            size: 0,
            modified_at: Some(modified_at),
            permissions: None,
        }
    }

    #[test]
    fn remote_sort_keeps_dot_entries_first_in_both_directions() {
        let entries = vec![
            entry("beta", 1),
            entry("..", 0),
            entry("alpha", 2),
            entry(".", 0),
        ];
        let descending = sorted_remote_entries(&entries, RemoteSortField::Name, false);
        assert_eq!(
            descending
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec![".", "..", "beta", "alpha"]
        );

        let by_time = sorted_remote_entries(&entries, RemoteSortField::Modified, true);
        assert_eq!(by_time[0].name, ".");
        assert_eq!(by_time[1].name, "..");
        assert_eq!(by_time[2].name, "beta");
        assert_eq!(by_time[3].name, "alpha");
    }

    #[test]
    fn terminal_scrollbar_uses_content_rows_and_current_offset() {
        assert!(terminal_scrollbar_metrics(10, 20., 200., 0.).is_none());

        let middle = terminal_scrollbar_metrics(100, 20., 200., -900.).unwrap();
        assert_eq!(middle.max_scroll, 1_800.);
        assert_eq!(middle.thumb_height, 48.);
        assert_eq!(middle.thumb_top, 76.);

        let bottom = terminal_scrollbar_metrics(100, 20., 200., -1_800.).unwrap();
        assert_eq!(bottom.thumb_top, 152.);
    }

    #[test]
    fn dragging_terminal_scrollbar_updates_uniform_list_offset() {
        let handle = UniformListScrollHandle::new();
        let metrics = terminal_scrollbar_metrics(100, 20., 200., 0.).unwrap();

        set_terminal_scroll_from_thumb(&handle, metrics, 76., 200.);

        assert_eq!(f32::from(handle.0.borrow().base_handle.offset().y), -900.);
    }

    #[test]
    fn visual_cursor_does_not_shift_terminal_selection_ranges() {
        let line = TerminalLine {
            text: "beta".into(),
            styles: Vec::new(),
            cursor_column: Some(2),
        };

        let (display, cursor) = terminal_text_with_cursor(&line);
        assert_eq!(display, "be▏ta");
        assert_eq!(cursor, Some(2..5));

        let selected = terminal_display_range(2..4, cursor.as_ref());
        assert_eq!(&display[selected], "ta");
    }
}
