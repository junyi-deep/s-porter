use super::app::{AppView, SshConnectionState, SshTab, UI_FONT_SIZES};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    input::{Input, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    resizable::{resizable_panel, v_resizable},
    scroll::ScrollableElement,
    text::{TextView, TextViewStyle},
    *,
};
use std::path::PathBuf;

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
            .title("选择跳板机")
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
        let activate_id = tab.id.clone();
        let close_id = tab.id.clone();
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
                    .tooltip("关闭连接")
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

fn render_terminal(
    tab: &SshTab,
    quick_commands: &[crate::storage::QuickCommand],
    global_font_size: f32,
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
    let output = tab
        .terminal
        .as_ref()
        .map(|terminal| terminal.output())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| match &tab.state {
            SshConnectionState::Connecting => "正在建立 SSH 连接…".into(),
            SshConnectionState::Failed(error) => format!("SSH 连接失败：{error}"),
            SshConnectionState::Connected => "已连接，等待远端输出…".into(),
        });
    let clear_view = view.clone();
    let reconnect_view = view.clone();
    let files_view = view.clone();
    let clear_id = tab.id.clone();
    let reconnect_id = tab.id.clone();
    let files_id = tab.id.clone();
    let font_size_view = view.clone();
    let font_size_tab_id = tab.id.clone();
    let custom_font_size = tab.terminal_font_size;
    let terminal_font_size = custom_font_size.unwrap_or(global_font_size);
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
                            this.fill_ssh_command(&fill_tab_id, &command, window, cx)
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
                        .child(
                            Button::new(format!("ssh-font-size-{}", tab.id))
                                .xsmall()
                                .ghost()
                                .label(format!("输出字号 {terminal_font_size:.0}px"))
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
                                .tooltip("清空输入和终端内容")
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
        .child(
            div().flex_1().min_h_0().child(
                v_resizable(format!("ssh-terminal-panels-{}", tab.id))
                    .child(
                        resizable_panel().child(
                            v_flex()
                                .size_full()
                                .min_h_0()
                                .p_3()
                                .bg(gpui::rgb(0xffffff))
                                .text_color(gpui::rgb(0x111827))
                                .overflow_y_scrollbar()
                                .child(
                                    TextView::markdown(
                                        format!("ssh-output-{}", tab.id),
                                        format!("```text\n{output}\n```"),
                                    )
                                    .style(
                                        TextViewStyle::default().code_block(
                                            StyleRefinement::default()
                                                .text_size(px(terminal_font_size)),
                                        ),
                                    )
                                    .selectable(true),
                                )
                                .into_any_element(),
                        ),
                    )
                    .child(
                        resizable_panel()
                            .size(px(150.))
                            .size_range(px(110.)..px(420.))
                            .child(
                                v_flex()
                                    .size_full()
                                    .min_h_0()
                                    .gap_2()
                                    .p_2()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .bg(gpui::rgb(0xffffff))
                                    .text_color(gpui::rgb(0x111827))
                                    .child(
                                        div().flex_1().min_h_0().child(
                                            Input::new(&tab.command).font_family("monospace"),
                                        ),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_shrink_0()
                                            .gap_1()
                                            .child(
                                                Button::new(format!(
                                                    "add-quick-command-{}",
                                                    tab.id
                                                ))
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
                                    ),
                            ),
                    ),
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
        format!("{:.1} MB", size as f64 / 1_048_576.)
    }
}

fn format_modified_time(timestamp: Option<u64>) -> String {
    timestamp
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp as i64, 0))
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".into())
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
    let upload_view = view.clone();
    let drop_view = view.clone();
    let menu_view = view.clone();
    let new_file_view = view.clone();
    let new_directory_view = view.clone();
    let file_view_settings = view.clone();
    let parent_id = tab.id.clone();
    let upload_id = tab.id.clone();
    let drop_id = tab.id.clone();
    let menu_id = tab.id.clone();
    let new_file_id = tab.id.clone();
    let new_directory_id = tab.id.clone();
    let file_view_settings_id = tab.id.clone();
    let parent = crate::forward::parent_path(&tab.remote_path);
    let show_file_time = tab.show_file_time;
    let show_file_size = tab.show_file_size;
    let show_file_permissions = tab.show_file_permissions;
    let panel_width = 300.
        + if show_file_time { 92. } else { 0. }
        + if show_file_size { 58. } else { 0. }
        + if show_file_permissions { 58. } else { 0. };
    let entries = tab.remote_entries.iter().map(|entry| {
        let open_view = view.clone();
        let download_view = view.clone();
        let menu_download_view = view.clone();
        let drag_view = view.clone();
        let open_id = tab.id.clone();
        let download_id = tab.id.clone();
        let menu_download_id = tab.id.clone();
        let drag_id = tab.id.clone();
        let open_entry = entry.clone();
        let download_entry = entry.clone();
        let menu_entry = entry.clone();
        let drag_entry = entry.clone();
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
            .child(div().flex_1().min_w_0().text_sm().child(entry.name.clone()))
            .when(show_file_time, |row| {
                row.child(
                    div()
                        .w(px(88.))
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
            .child(
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
            .when(entry.is_dir, |row| {
                row.on_click(move |_, window, cx| {
                    open_view.update(cx, |this, cx| {
                        this.load_ssh_directory(&open_id, &open_entry.path, window, cx)
                    });
                })
            })
            .on_drag(paths, move |paths: &ExternalPaths, _, window, cx| {
                drag_view.update(cx, |this, cx| {
                    this.prepare_ssh_drag(&drag_id, drag_entry.clone(), window, cx);
                });
                cx.new(|_| paths.clone())
            })
            .context_menu(move |menu, _, _| {
                menu.item(PopupMenuItem::new("下载").on_click({
                    let entry = menu_entry.clone();
                    let view = menu_download_view.clone();
                    let id = menu_download_id.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.prompt_ssh_download(&id, entry.clone(), window, cx)
                        });
                    }
                }))
            })
    });

    v_flex()
        .w(px(panel_width))
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
                    Button::new(format!("remote-upload-{}", tab.id))
                        .xsmall()
                        .outline()
                        .label("上传")
                        .on_click(move |_, window, cx| {
                            upload_view.update(cx, |this, cx| {
                                this.prompt_ssh_upload(&upload_id, window, cx)
                            });
                        }),
                ),
        )
        .child(
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
                    !tab.file_loading && tab.file_error.is_none() && tab.remote_entries.is_empty(),
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
                    .item(PopupMenuItem::new("上传文件或文件夹").on_click({
                        let view = menu_view.clone();
                        let id = menu_id.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.prompt_ssh_upload(&id, window, cx));
                        }
                    }))
                }),
        )
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
                            "请先新增跳板机"
                        } else {
                            "选择跳板机并新建连接"
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
                h_flex()
                    .flex_1()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .child(render_terminal(
                                tab,
                                &view_state.quick_commands,
                                view_state.ui_font_size(),
                                &view,
                                cx,
                            )),
                    )
                    .child(render_file_panel(tab, &view, cx))
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
                    "请先新增跳板机"
                } else {
                    "点击右上角“新增连接”"
                })
                .into_any_element()
        })
        .into_any_element()
}
