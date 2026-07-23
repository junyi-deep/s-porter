use super::app::{AppView, ForwardState, ForwardStatusFilter};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::{Dialog, DialogAction, DialogClose, DialogFooter, DialogHeader, DialogTitle},
    input::{Input, InputState},
    table::{Column, DataTable, TableDelegate, TableState},
    text::TextView,
    *,
};

type FormInput = (&'static str, Entity<InputState>, bool, bool);

#[derive(Clone)]
struct ForwardTableRow {
    item: crate::forward::ForwardConfig,
    host_label: String,
    state: ForwardState,
    selected: bool,
    can_edit: bool,
}

pub(super) struct ForwardTableDelegate {
    view: WeakEntity<AppView>,
    columns: Vec<Column>,
    rows: Vec<ForwardTableRow>,
    visible_ids: Vec<String>,
    all_selected: bool,
    empty_message: String,
}

impl ForwardTableDelegate {
    pub(super) fn new(view: Entity<AppView>) -> Self {
        Self {
            view: view.downgrade(),
            columns: vec![
                Column::new("selected", "")
                    .p_0()
                    .width(px(44.))
                    .min_width(px(40.))
                    .max_width(px(64.))
                    .selectable(false),
                Column::new("name", "名称")
                    .p_0()
                    .width(px(140.))
                    .min_width(px(80.))
                    .max_width(px(320.)),
                Column::new("local", "本地端口")
                    .p_0()
                    .width(px(125.))
                    .min_width(px(95.))
                    .max_width(px(240.)),
                Column::new("remote", "远程目标")
                    .p_0()
                    .width(px(170.))
                    .min_width(px(110.))
                    .max_width(px(360.)),
                Column::new("jump_host", "跳板机")
                    .p_0()
                    .width(px(190.))
                    .min_width(px(120.))
                    .max_width(px(400.)),
                Column::new("status", "状态")
                    .p_0()
                    .width(px(85.))
                    .min_width(px(70.))
                    .max_width(px(160.)),
                Column::new("actions", "操作")
                    .p_0()
                    .width(px(230.))
                    .min_width(px(210.))
                    .max_width(px(360.))
                    .selectable(false),
            ],
            rows: Vec::new(),
            visible_ids: Vec::new(),
            all_selected: false,
            empty_message: "暂无配置，点击右上角“新增配置”开始".into(),
        }
    }

    fn update_rows(
        &mut self,
        rows: Vec<ForwardTableRow>,
        visible_ids: Vec<String>,
        all_selected: bool,
        empty_message: String,
    ) {
        self.rows = rows;
        self.visible_ids = visible_ids;
        self.all_selected = all_selected;
        self.empty_message = empty_message;
    }
}

impl TableDelegate for ForwardTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let header = h_flex()
            .size_full()
            .justify_center()
            .px_3()
            .border_r_1()
            .border_color(cx.theme().table_row_border)
            .text_xs()
            .font_semibold();
        if col_ix == 0 {
            let select_view = self.view.clone();
            let visible_ids = self.visible_ids.clone();
            header
                .child(
                    Checkbox::new("select-all")
                        .checked(self.all_selected)
                        .tooltip("全选")
                        .on_click(move |selected, _, cx| {
                            let _ = select_view.update(cx, |this, cx| {
                                this.select_ids(&visible_ids, *selected, cx)
                            });
                        }),
                )
                .into_any_element()
        } else {
            header
                .child(self.columns[col_ix].name.clone())
                .into_any_element()
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = self.rows[row_ix].clone();
        let item = row.item;
        let cell = h_flex()
            .size_full()
            .min_w_0()
            .justify_start()
            .px_3()
            .border_r_1()
            .border_color(cx.theme().table_row_border)
            .text_sm();
        match col_ix {
            0 => {
                let select_view = self.view.clone();
                let id = item.id.clone();
                cell.child(
                    Checkbox::new(format!("select-{}", item.id))
                        .checked(row.selected)
                        .on_click(move |selected, _, cx| {
                            let _ = select_view
                                .update(cx, |this, cx| this.toggle_selected(&id, *selected, cx));
                        }),
                )
                .into_any_element()
            }
            1 => cell
                .child(TextView::markdown(format!("name-{}", item.id), item.name).selectable(true))
                .into_any_element(),
            2 => cell
                .child(
                    TextView::markdown(
                        format!("local-{}", item.id),
                        format!("127.0.0.1:{}", item.local_port),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            3 => cell
                .child(
                    TextView::markdown(
                        format!("remote-{}", item.id),
                        format!("{}:{}", item.remote_ip, item.remote_port),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            4 => cell
                .child(
                    TextView::markdown(format!("ssh-{}", item.id), row.host_label).selectable(true),
                )
                .into_any_element(),
            5 => {
                let (label, color) = match row.state {
                    ForwardState::Stopped => ("已停止", cx.theme().muted_foreground),
                    ForwardState::Starting => ("启动中", cx.theme().warning),
                    ForwardState::Running => ("运行中", cx.theme().success),
                    ForwardState::Failed(_) => ("失败", cx.theme().danger),
                };
                cell.text_color(color).child(label).into_any_element()
            }
            _ => {
                let start_view = self.view.clone();
                let stop_view = self.view.clone();
                let enable_view = self.view.clone();
                let log_view = self.view.clone();
                let edit_view = self.view.clone();
                let clone_view = self.view.clone();
                let delete_view = self.view.clone();
                let start_id = item.id.clone();
                let stop_id = item.id.clone();
                let log_id = item.id.clone();
                let edit_id = item.id.clone();
                let clone_id = item.id.clone();
                let delete_id = item.id.clone();
                let enable_item = item.clone();
                let is_busy = matches!(row.state, ForwardState::Starting | ForwardState::Running);
                let can_stop = matches!(row.state, ForwardState::Running | ForwardState::Failed(_));
                cell.gap_1()
                    .child(
                        Button::new(format!("start-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Play)
                            .tooltip("启动转发")
                            .disabled(is_busy)
                            .on_click(move |_, window, cx| {
                                let _ = start_view.update(cx, |this, cx| {
                                    this.start_tunnel(&start_id, window, cx)
                                });
                            }),
                    )
                    .child(
                        Button::new(format!("stop-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Pause)
                            .tooltip("停止转发")
                            .disabled(!can_stop)
                            .on_click(move |_, window, cx| {
                                let _ = stop_view
                                    .update(cx, |this, cx| this.stop_tunnel(&stop_id, window, cx));
                            }),
                    )
                    .child(
                        Button::new(format!("enable-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Settings2)
                            .tooltip("开启远端 SSH 端口转发")
                            .on_click(move |_, window, cx| {
                                let item = enable_item.clone();
                                let _ = enable_view.update(cx, |this, cx| {
                                    this.run_ssh_operation(item, true, window, cx)
                                });
                            }),
                    )
                    .child(
                        Button::new(format!("logs-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::File)
                            .tooltip("查看转发日志")
                            .on_click(move |_, window, cx| {
                                let _ = log_view
                                    .update(cx, |this, cx| this.show_logs(&log_id, window, cx));
                            }),
                    )
                    .child(
                        Button::new(format!("edit-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Settings)
                            .tooltip(if row.can_edit {
                                "编辑配置"
                            } else {
                                "请先停止转发，再编辑配置"
                            })
                            .disabled(!row.can_edit)
                            .on_click(move |_, window, cx| {
                                if let Some(view) = edit_view.upgrade() {
                                    open_edit_dialog(view, edit_id.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("clone-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Copy)
                            .tooltip("克隆配置")
                            .on_click(move |_, window, cx| {
                                if let Some(view) = clone_view.upgrade() {
                                    open_clone_dialog(view, clone_id.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("delete-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::default().path("icons/trash-2.svg"))
                            .tooltip("删除配置并停止转发")
                            .on_click(move |_, window, cx| {
                                let id = delete_id.clone();
                                let _ = delete_view.update(cx, |this, cx| {
                                    this.delete_configs(vec![id], window, cx)
                                });
                            }),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(self.empty_message.clone())
    }
}

fn form_inputs(view: &AppView) -> Vec<FormInput> {
    vec![
        ("配置名称", view.form.name.clone(), false, true),
        ("本地端口", view.form.local_port.clone(), false, true),
        ("远程 IP / 域名", view.form.remote_ip.clone(), false, true),
        ("远程端口", view.form.remote_port.clone(), false, true),
    ]
}

fn configure_form_dialog(dialog: Dialog, view: Entity<AppView>, inputs: Vec<FormInput>) -> Dialog {
    let save_view = view.clone();
    let test_view = view.clone();
    let enable_view = view.clone();
    dialog
        .width(px(720.))
        .on_ok(move |_, window, cx| save_view.update(cx, |this, cx| this.save_form(window, cx)))
        .p_0()
        .content(move |content, _, cx| {
            let enable_view = enable_view.clone();
            let test_view = test_view.clone();
            let keep_alive_view = view.clone();
            let hosts = view.read(cx).jump_hosts.clone();
            let selected = view.read(cx).selected_jump_host_id.clone();
            let host_search = view.read(cx).forward_host_picker_search.clone();
            let keep_alive = view.read(cx).form_keep_alive;
            let picker_view = view.clone();
            content
                .w_full()
                .child(
                    DialogHeader::new()
                        .p_5()
                        .child(DialogTitle::new().child("本地端口转发配置"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("转发目标通过已保存的跳板机建立 SSH 隧道。"),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .px_5()
                        .pb_4()
                        .child(
                            h_flex()
                                .gap_1()
                                .child(div().text_sm().font_medium().child("选择跳板机"))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_bold()
                                        .text_color(cx.theme().danger)
                                        .child("*"),
                                ),
                        )
                        .child(super::jump_host_picker::render(
                            "forward-host-picker",
                            &hosts,
                            &host_search,
                            selected.as_deref(),
                            move |id, _, cx| {
                                picker_view
                                    .update(cx, |this, cx| this.select_forward_jump_host(id, cx));
                            },
                            cx,
                        )),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_x_5()
                        .gap_y_3()
                        .px_5()
                        .pb_5()
                        .children(inputs.iter().map(|(label, state, password, required)| {
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(div().text_sm().font_medium().child(*label))
                                        .when(*required, |label| {
                                            label.child(
                                                div()
                                                    .text_sm()
                                                    .font_bold()
                                                    .text_color(cx.theme().danger)
                                                    .child("*"),
                                            )
                                        }),
                                )
                                .child(
                                    Input::new(state).when(*password, |input| input.mask_toggle()),
                                )
                        })),
                )
                .child(
                    h_flex()
                        .gap_5()
                        .px_5()
                        .pb_5()
                        .child(
                            Checkbox::new("forward-keep-alive")
                                .checked(keep_alive)
                                .label("启用 SSH 保活")
                                .on_click(move |enabled, _, cx| {
                                    keep_alive_view.update(cx, |this, cx| {
                                        this.set_form_keep_alive(*enabled, cx)
                                    });
                                }),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_medium()
                                        .text_color(if keep_alive {
                                            cx.theme().foreground
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .child("心跳间隔（秒）"),
                                )
                                .child(
                                    div().w(px(180.)).child(
                                        Input::new(&view.read(cx).form.keep_alive_interval)
                                            .disabled(!keep_alive),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("建议 30 秒，可设置 2–3600 秒"),
                        ),
                )
                .child(
                    DialogFooter::new()
                        .p_4()
                        .bg(cx.theme().muted)
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("form-enable")
                                        .outline()
                                        .label("开启允许转发")
                                        .on_click(move |_, window, cx| {
                                            enable_view.update(cx, |this, cx| {
                                                this.run_form_ssh(true, window, cx)
                                            });
                                        }),
                                )
                                .child(
                                    Button::new("form-test")
                                        .outline()
                                        .label("测试连接")
                                        .on_click(move |_, window, cx| {
                                            test_view.update(cx, |this, cx| {
                                                this.run_form_ssh(false, window, cx)
                                            });
                                        }),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    DialogClose::new()
                                        .child(Button::new("cancel-add").outline().label("取消")),
                                )
                                .child(
                                    DialogAction::new()
                                        .child(Button::new("save-add").primary().label("保存")),
                                ),
                        ),
                )
        })
}

fn add_dialog(view_state: &AppView, cx: &mut Context<AppView>) -> impl IntoElement {
    let view = cx.entity();
    let reset_view = view.clone();
    configure_form_dialog(
        Dialog::new(cx).trigger(
            Button::new("add-forward")
                .primary()
                .icon(IconName::Plus)
                .label("新增配置")
                .disabled(view_state.jump_hosts.is_empty())
                .tooltip(if view_state.jump_hosts.is_empty() {
                    "请先新增跳板机"
                } else {
                    "新增本地转发配置"
                })
                .on_click(move |_, window, cx| {
                    reset_view.update(cx, |this, cx| this.prepare_new_form(window, cx));
                }),
        ),
        view,
        form_inputs(view_state),
    )
}

fn open_clone_dialog(view: Entity<AppView>, id: String, window: &mut Window, cx: &mut App) {
    if !view.update(cx, |this, cx| this.prepare_clone_form(&id, window, cx)) {
        return;
    }
    let inputs = form_inputs(view.read(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        configure_form_dialog(dialog, view.clone(), inputs.clone())
    });
}

fn open_edit_dialog(view: Entity<AppView>, id: String, window: &mut Window, cx: &mut App) {
    if !view.update(cx, |this, cx| {
        this.prepare_edit_forward_form(&id, window, cx)
    }) {
        return;
    }
    let inputs = form_inputs(view.read(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        configure_form_dialog(dialog, view.clone(), inputs.clone())
    });
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let dialog = add_dialog(view_state, cx).into_any_element();
    let view = cx.entity();
    let search = view_state
        .forward_search
        .read(cx)
        .value()
        .trim()
        .to_lowercase();
    let jump_host_label = |item: &crate::forward::ForwardConfig| {
        view_state
            .jump_hosts
            .iter()
            .find(|host| host.id == item.jump_host_id)
            .map(|host| {
                format!(
                    "{} · {}@{}:{}",
                    host.name, host.username, host.host, host.port
                )
            })
            .unwrap_or_else(|| "跳板机已不存在".into())
    };
    let state_of = |item: &crate::forward::ForwardConfig| {
        if view_state
            .tunnels
            .get(&item.id)
            .is_some_and(|handle| !handle.is_running())
        {
            ForwardState::Failed("本地监听线程已退出，请查看日志".into())
        } else {
            view_state
                .forward_states
                .get(&item.id)
                .cloned()
                .unwrap_or(ForwardState::Stopped)
        }
    };
    let filtered = view_state
        .forwards
        .iter()
        .filter(|item| {
            let matches_search = search.is_empty()
                || item.name.to_lowercase().contains(&search)
                || item.local_port.to_string().contains(&search)
                || format!("{}:{}", item.remote_ip, item.remote_port)
                    .to_lowercase()
                    .contains(&search)
                || jump_host_label(item).to_lowercase().contains(&search);
            let state = state_of(item);
            let matches_status = match view_state.forward_status_filter {
                ForwardStatusFilter::All => true,
                ForwardStatusFilter::Running => {
                    matches!(state, ForwardState::Starting | ForwardState::Running)
                }
                ForwardStatusFilter::Stopped => matches!(state, ForwardState::Stopped),
                ForwardStatusFilter::Failed => matches!(state, ForwardState::Failed(_)),
            };
            matches_search && matches_status
        })
        .collect::<Vec<_>>();
    let visible_ids = filtered
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let all_selected = !visible_ids.is_empty()
        && visible_ids
            .iter()
            .all(|id| view_state.selected.contains(id));
    let filtered_len = filtered.len();
    let start_selected_view = view.clone();
    let stop_selected_view = view.clone();
    let delete_selected_view = view.clone();
    let has_selection = !view_state.selected.is_empty();
    let rows = filtered
        .into_iter()
        .map(|item| ForwardTableRow {
            item: item.clone(),
            host_label: jump_host_label(item),
            state: state_of(item),
            selected: view_state.selected.contains(&item.id),
            can_edit: !view_state.tunnels.contains_key(&item.id),
        })
        .collect();
    let empty_message = if view_state.forwards.is_empty() {
        "暂无配置，点击右上角“新增配置”开始"
    } else {
        "没有符合搜索或状态条件的配置"
    };
    view_state.forward_table.update(cx, |table, cx| {
        table
            .delegate_mut()
            .update_rows(rows, visible_ids, all_selected, empty_message.into());
        cx.notify();
    });

    v_flex()
        .size_full()
        .p_6()
        .gap_4()
        .child(
            h_flex()
                .justify_between()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_2xl().font_semibold().child("端口转发"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("通过 SSH 建立安全的本地端口转发"),
                        ),
                )
                .child(dialog),
        )
        .child(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().w(px(330.)).child(
                                Input::new(&view_state.forward_search)
                                    .prefix(Icon::new(IconName::Search).small()),
                            ),
                        )
                        .children(
                            [
                                ("filter-all", "全部", ForwardStatusFilter::All),
                                ("filter-running", "运行中", ForwardStatusFilter::Running),
                                ("filter-stopped", "已停止", ForwardStatusFilter::Stopped),
                                ("filter-failed", "失败", ForwardStatusFilter::Failed),
                            ]
                            .into_iter()
                            .map(|(id, label, filter)| {
                                let filter_view = view.clone();
                                Button::new(id)
                                    .small()
                                    .when(view_state.forward_status_filter == filter, |button| {
                                        button.primary()
                                    })
                                    .when(view_state.forward_status_filter != filter, |button| {
                                        button.outline()
                                    })
                                    .label(label)
                                    .on_click(move |_, _, cx| {
                                        filter_view.update(cx, |this, cx| {
                                            this.forward_status_filter = filter;
                                            cx.notify();
                                        });
                                    })
                            }),
                        ),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "显示 {} 项 · 已选择 {} 项",
                                    filtered_len,
                                    view_state.selected.len()
                                )),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("batch-start")
                                        .small()
                                        .outline()
                                        .label("批量启动")
                                        .disabled(!has_selection)
                                        .on_click(move |_, window, cx| {
                                            start_selected_view.update(cx, |this, cx| {
                                                this.start_selected(window, cx)
                                            });
                                        }),
                                )
                                .child(
                                    Button::new("batch-stop")
                                        .small()
                                        .outline()
                                        .label("批量停止")
                                        .disabled(!has_selection)
                                        .on_click(move |_, window, cx| {
                                            stop_selected_view.update(cx, |this, cx| {
                                                this.stop_selected(window, cx)
                                            });
                                        }),
                                )
                                .child(
                                    Button::new("batch-delete")
                                        .small()
                                        .danger()
                                        .label("批量删除")
                                        .disabled(!has_selection)
                                        .on_click(move |_, window, cx| {
                                            delete_selected_view.update(cx, |this, cx| {
                                                this.delete_selected(window, cx)
                                            });
                                        }),
                                ),
                        ),
                ),
        )
        .child(
            div().flex_1().min_h_0().overflow_hidden().child(
                DataTable::new(&view_state.forward_table)
                    .bordered(true)
                    .scrollbar_visible(true, true),
            ),
        )
        .into_any_element()
}
