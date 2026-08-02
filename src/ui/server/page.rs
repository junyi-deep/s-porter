use crate::ui::app::AppView;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::{Dialog, DialogClose, DialogFooter, DialogHeader, DialogTitle},
    input::{Input, InputState},
    table::{Column, DataTable, TableDelegate, TableState},
    text::TextView,
    *,
};

type FormInput = (&'static str, Entity<InputState>, bool, bool);

#[derive(Clone)]
struct JumpHostTableRow {
    host: crate::forward::JumpHost,
    forward_count: usize,
    connection_count: usize,
    selected: bool,
}

pub(in crate::ui) struct JumpHostTableDelegate {
    view: WeakEntity<AppView>,
    columns: Vec<Column>,
    rows: Vec<JumpHostTableRow>,
    visible_ids: Vec<String>,
    all_selected: bool,
    empty_message: String,
}

impl JumpHostTableDelegate {
    pub(in crate::ui) fn new(view: Entity<AppView>) -> Self {
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
                    .width(px(180.))
                    .min_width(px(100.))
                    .max_width(px(360.)),
                Column::new("address", "SSH 服务")
                    .p_0()
                    .width(px(220.))
                    .min_width(px(130.))
                    .max_width(px(420.)),
                Column::new("username", "登录用户")
                    .p_0()
                    .width(px(140.))
                    .min_width(px(90.))
                    .max_width(px(280.)),
                Column::new("relations", "关联")
                    .p_0()
                    .width(px(150.))
                    .min_width(px(100.))
                    .max_width(px(260.)),
                Column::new("actions", "操作")
                    .p_0()
                    .width(px(160.))
                    .min_width(px(140.))
                    .max_width(px(240.))
                    .selectable(false),
            ],
            rows: Vec::new(),
            visible_ids: Vec::new(),
            all_selected: false,
            empty_message: "暂无服务器，点击右上角新增".into(),
        }
    }

    fn update_rows(
        &mut self,
        rows: Vec<JumpHostTableRow>,
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

impl TableDelegate for JumpHostTableDelegate {
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
        let header = div()
            .size_full()
            .flex()
            .items_center()
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
                    Checkbox::new("select-all-jump-hosts")
                        .checked(self.all_selected)
                        .tooltip("全选当前列表")
                        .on_click(move |selected, _, cx| {
                            let _ = select_view.update(cx, |this, cx| {
                                this.select_jump_host_ids(&visible_ids, *selected, cx)
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
        let host = row.host;
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
                let id = host.id.clone();
                cell.child(
                    Checkbox::new(format!("select-jump-host-{}", host.id))
                        .checked(row.selected)
                        .on_click(move |selected, _, cx| {
                            let _ = select_view.update(cx, |this, cx| {
                                this.toggle_jump_host_selected(&id, *selected, cx)
                            });
                        }),
                )
                .into_any_element()
            }
            1 => cell
                .child(
                    TextView::markdown(format!("host-name-{}", host.id), host.name)
                        .selectable(true),
                )
                .into_any_element(),
            2 => cell
                .child(
                    TextView::markdown(
                        format!("host-address-{}", host.id),
                        format!("{}:{}", host.host, host.port),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            3 => cell.child(host.username).into_any_element(),
            4 => cell
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{} 转发 / {} 连接",
                    row.forward_count, row.connection_count
                ))
                .into_any_element(),
            _ => {
                let connect_view = self.view.clone();
                let edit_view = self.view.clone();
                let copy_view = self.view.clone();
                let delete_view = self.view.clone();
                let connect_id = host.id.clone();
                let edit_id = host.id.clone();
                let copy_id = host.id.clone();
                let delete_id = host.id.clone();
                cell.gap_1()
                    .child(
                        Button::new(format!("connect-host-{}", host.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::SquareTerminal)
                            .tooltip("连接服务器")
                            .on_click(move |_, window, cx| {
                                let _ = connect_view.update(cx, |this, cx| {
                                    this.open_ssh_connection(&connect_id, window, cx)
                                });
                            }),
                    )
                    .child(
                        Button::new(format!("edit-host-{}", host.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Settings2)
                            .tooltip("编辑服务器")
                            .on_click(move |_, window, cx| {
                                if let Some(view) = edit_view.upgrade() {
                                    open_edit_dialog(view, edit_id.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("copy-host-{}", host.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Copy)
                            .tooltip("复制服务器")
                            .on_click(move |_, window, cx| {
                                if let Some(view) = copy_view.upgrade() {
                                    open_copy_dialog(view, copy_id.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("delete-host-{}", host.id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::default().path("icons/trash-2.svg"))
                            .tooltip("删除服务器")
                            .on_click(move |_, window, cx| {
                                let id = delete_id.clone();
                                let _ = delete_view.update(cx, |this, cx| {
                                    this.request_delete_jump_host(id, window, cx)
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
        ("服务器名称", view.servers.form.name.clone(), false, true),
        (
            "SSH 地址 / 域名",
            view.servers.form.host.clone(),
            false,
            true,
        ),
        ("SSH 端口", view.servers.form.port.clone(), false, true),
        (
            "登录用户名",
            view.servers.form.username.clone(),
            false,
            true,
        ),
        ("登录密码", view.servers.form.password.clone(), true, true),
        (
            "root 用户名",
            view.servers.form.root_username.clone(),
            false,
            true,
        ),
        (
            "root 密码",
            view.servers.form.root_password.clone(),
            true,
            true,
        ),
        (
            "HTTP 代理地址（可选）",
            view.servers.form.proxy_host.clone(),
            false,
            false,
        ),
        (
            "HTTP 代理端口",
            view.servers.form.proxy_port.clone(),
            false,
            false,
        ),
        (
            "代理用户名（可选）",
            view.servers.form.proxy_username.clone(),
            false,
            false,
        ),
        (
            "代理密码（可选）",
            view.servers.form.proxy_password.clone(),
            true,
            false,
        ),
    ]
}

fn configure_dialog(
    dialog: Dialog,
    view: Entity<AppView>,
    inputs: Vec<FormInput>,
    is_batch: bool,
) -> Dialog {
    let keyboard_save_view = view.clone();
    let button_save_view = view.clone();
    let test_view = view.clone();
    dialog
        .width(px(820.))
        .on_ok(move |_, window, cx| {
            keyboard_save_view.update(cx, |this, cx| {
                this.servers.batch_mode = is_batch;
                this.save_jump_host(window, cx)
            })
        })
        .p_0()
        .content(move |content, _, cx| {
            let view_state = view.read(cx);
            let is_editing = view_state.servers.editing_id.is_some();
            let batch_entries = view_state.servers.form.batch_entries.clone();
            let batch_separator = view_state.servers.form.batch_separator.clone();
            let form_error = view_state.servers.form_error.clone();
            let test_view = test_view.clone();
            let save_view = button_save_view.clone();
            content
                .w_full()
                .child(
                    DialogHeader::new()
                        .p_5()
                        .child(DialogTitle::new().child(if is_editing {
                            "编辑服务器"
                        } else if is_batch {
                            "批量新增服务器"
                        } else {
                            "新增服务器"
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(if is_batch {
                                    "每行输入服务器名称和 SSH 地址，默认支持逗号、Tab、空格或自定义分隔符；其它配置由本批次共用。"
                                } else {
                                    "登录用户和 root 用户的用户名、密码均为必填项。"
                                }),
                        ),
                )
                .when_some(form_error, |content, error| {
                    content.child(
                        div()
                            .mx_5()
                            .mb_3()
                            .p_3()
                            .rounded_md()
                            .bg(cx.theme().danger.opacity(0.1))
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(error),
                    )
                })
                .when(is_batch, |content| {
                    content.child(
                        v_flex()
                            .mx_5()
                            .mb_4()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(div().text_sm().font_medium().child("服务器列表"))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_bold()
                                            .text_color(cx.theme().danger)
                                            .child("*"),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(150.))
                                    .child(Input::new(&batch_entries).h_full()),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .text_sm()
                                    .font_medium()
                                    .child("自定义分隔符（可选）"),
                            )
                            .child(
                                Input::new(&batch_separator),
                            ),
                    )
                })
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_x_5()
                        .gap_y_3()
                        .px_5()
                        .pb_5()
                        .children(
                            inputs
                                .iter()
                                .filter(|(label, _, _, _)| {
                                    !is_batch
                                        || !matches!(*label, "服务器名称" | "SSH 地址 / 域名")
                                })
                                .map(|(label, state, password, required)| {
                                    v_flex()
                                        .gap_1()
                                        .when(*label == "服务器名称", |field| {
                                            field.col_span_full()
                                        })
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .child(
                                                    div().text_sm().font_medium().child(*label),
                                                )
                                                .when(*required, |element| {
                                                    element.child(
                                                        div()
                                                            .text_sm()
                                                            .font_bold()
                                                            .text_color(cx.theme().danger)
                                                            .child("*"),
                                                    )
                                                }),
                                        )
                                        .child(Input::new(state).when(*password, |input| {
                                            input.mask_toggle()
                                        }))
                                }),
                        ),
                )
                .child(
                    DialogFooter::new()
                        .p_4()
                        .bg(cx.theme().muted)
                        .justify_between()
                        .when(!is_batch, |footer| {
                            footer.child(
                                Button::new("test-jump-host")
                                    .outline()
                                    .label("测试连通性")
                                    .on_click(move |_, window, cx| {
                                        test_view.update(cx, |this, cx| {
                                            this.test_jump_host_form(window, cx)
                                        });
                                    }),
                            )
                        })
                        .when(is_batch, |footer| footer.child(div()))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    DialogClose::new().child(
                                        Button::new("cancel-jump-host").outline().label("取消"),
                                    ),
                                )
                                .child(
                                    Button::new("save-jump-host")
                                        .primary()
                                        .label("保存")
                                        .on_click(move |_, window, cx| {
                                            if save_view.update(cx, |this, cx| {
                                                this.servers.batch_mode = is_batch;
                                                this.save_jump_host(window, cx)
                                            }) {
                                                window.close_dialog(cx);
                                            }
                                        }),
                                ),
                        ),
                )
        })
}

fn add_dialog(view_state: &AppView, cx: &mut Context<AppView>) -> impl IntoElement {
    let view = cx.entity();
    let reset_view = view.clone();
    let batch_view = view.clone();
    let inputs = form_inputs(view_state);
    let single = configure_dialog(
        Dialog::new(cx).trigger(
            Button::new("add-jump-host")
                .primary()
                .icon(IconName::Plus)
                .label("新增服务器")
                .on_click(move |_, window, cx| {
                    reset_view.update(cx, |this, cx| this.prepare_new_jump_host(window, cx));
                }),
        ),
        view.clone(),
        inputs.clone(),
        false,
    );
    let batch = configure_dialog(
        Dialog::new(cx).trigger(
            Button::new("batch-add-jump-host")
                .outline()
                .icon(IconName::Plus)
                .label("批量新增服务器")
                .on_click(move |_, window, cx| {
                    batch_view.update(cx, |this, cx| this.prepare_batch_jump_hosts(window, cx));
                }),
        ),
        view,
        inputs,
        true,
    );
    h_flex().gap_2().child(single).child(batch)
}

fn open_edit_dialog(view: Entity<AppView>, id: String, window: &mut Window, cx: &mut App) {
    if !view.update(cx, |this, cx| this.prepare_edit_jump_host(&id, window, cx)) {
        return;
    }
    let inputs = form_inputs(view.read(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        configure_dialog(dialog, view.clone(), inputs.clone(), false)
    });
}

fn open_copy_dialog(view: Entity<AppView>, id: String, window: &mut Window, cx: &mut App) {
    if !view.update(cx, |this, cx| this.prepare_copy_jump_host(&id, window, cx)) {
        return;
    }
    let inputs = form_inputs(view.read(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        configure_dialog(dialog, view.clone(), inputs.clone(), false)
    });
}

pub(in crate::ui) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let dialog = add_dialog(view_state, cx).into_any_element();
    let search =
        crate::ui::search::RegexSearch::new(view_state.servers.search.read(cx).value().as_ref());
    let search_error = search.error().map(ToOwned::to_owned);
    let filtered = view_state
        .servers
        .jump_hosts
        .iter()
        .filter(|host| {
            search.matches_any([
                host.name.as_str(),
                host.host.as_str(),
                host.username.as_str(),
                host.root_username.as_str(),
            ])
        })
        .collect::<Vec<_>>();
    let visible_ids = filtered
        .iter()
        .map(|host| host.id.clone())
        .collect::<Vec<_>>();
    let all_selected = !visible_ids.is_empty()
        && visible_ids
            .iter()
            .all(|id| view_state.servers.selected.contains(id));
    let filtered_len = filtered.len();
    let has_selection = !view_state.servers.selected.is_empty();
    let delete_selected_view = view.clone();
    let rows = filtered
        .into_iter()
        .map(|host| JumpHostTableRow {
            host: host.clone(),
            forward_count: view_state
                .forwarding
                .configs
                .iter()
                .filter(|item| item.jump_host_id == host.id)
                .count(),
            connection_count: view_state
                .ssh
                .tabs
                .iter()
                .filter(|tab| tab.jump_host_id == host.id)
                .count(),
            selected: view_state.servers.selected.contains(&host.id),
        })
        .collect();
    let empty_message = if view_state.servers.jump_hosts.is_empty() {
        "暂无服务器，点击右上角新增"
    } else {
        "没有符合搜索条件的服务器"
    };
    view_state.servers.table.update(cx, |table, cx| {
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
                        .child(div().text_2xl().font_semibold().child("服务器"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("集中管理 SSH 服务器、登录用户及 root 凭据"),
                        ),
                )
                .child(dialog),
        )
        .child(
            h_flex()
                .justify_between()
                .gap_3()
                .child(
                    v_flex()
                        .w(px(380.))
                        .gap_1()
                        .child(
                            Input::new(&view_state.servers.search)
                                .prefix(Icon::new(IconName::Search).small()),
                        )
                        .when_some(search_error, |field, error| {
                            field.child(div().text_xs().text_color(cx.theme().danger).child(error))
                        }),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "显示 {filtered_len} 项 · 已选择 {} 项",
                                    view_state.servers.selected.len()
                                )),
                        )
                        .child(
                            Button::new("batch-delete-jump-hosts")
                                .small()
                                .danger()
                                .label("批量删除")
                                .disabled(!has_selection)
                                .on_click(move |_, window, cx| {
                                    delete_selected_view.update(cx, |this, cx| {
                                        this.request_delete_selected_jump_hosts(window, cx)
                                    });
                                }),
                        ),
                ),
        )
        .child(
            div().flex_1().min_h_0().overflow_hidden().child(
                DataTable::new(&view_state.servers.table)
                    .bordered(true)
                    .scrollbar_visible(true, true),
            ),
        )
        .into_any_element()
}
