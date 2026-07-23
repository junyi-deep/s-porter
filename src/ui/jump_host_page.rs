use super::app::AppView;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
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
}

pub(super) struct JumpHostTableDelegate {
    view: WeakEntity<AppView>,
    columns: Vec<Column>,
    rows: Vec<JumpHostTableRow>,
    empty_message: String,
}

impl JumpHostTableDelegate {
    pub(super) fn new(view: Entity<AppView>) -> Self {
        Self {
            view: view.downgrade(),
            columns: vec![
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
                    .width(px(240.))
                    .min_width(px(220.))
                    .max_width(px(380.))
                    .selectable(false),
            ],
            rows: Vec::new(),
            empty_message: "暂无跳板机，点击右上角新增".into(),
        }
    }

    fn update_rows(&mut self, rows: Vec<JumpHostTableRow>, empty_message: String) {
        self.rows = rows;
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
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px_3()
            .border_r_1()
            .border_color(cx.theme().table_row_border)
            .text_xs()
            .font_semibold()
            .child(self.columns[col_ix].name.clone())
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
            0 => cell
                .child(
                    TextView::markdown(format!("host-name-{}", host.id), host.name)
                        .selectable(true),
                )
                .into_any_element(),
            1 => cell
                .child(
                    TextView::markdown(
                        format!("host-address-{}", host.id),
                        format!("{}:{}", host.host, host.port),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            2 => cell.child(host.username).into_any_element(),
            3 => cell
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{} 转发 / {} 连接",
                    row.forward_count, row.connection_count
                ))
                .into_any_element(),
            _ => {
                let connect_view = self.view.clone();
                let edit_view = self.view.clone();
                let delete_view = self.view.clone();
                let connect_id = host.id.clone();
                let edit_id = host.id.clone();
                let delete_id = host.id.clone();
                cell.gap_1()
                    .child(
                        Button::new(format!("connect-host-{}", host.id))
                            .small()
                            .primary()
                            .icon(IconName::SquareTerminal)
                            .label("连接")
                            .on_click(move |_, window, cx| {
                                let _ = connect_view.update(cx, |this, cx| {
                                    this.open_ssh_connection(&connect_id, window, cx)
                                });
                            }),
                    )
                    .child(
                        Button::new(format!("edit-host-{}", host.id))
                            .small()
                            .outline()
                            .icon(IconName::Settings2)
                            .label("编辑")
                            .on_click(move |_, window, cx| {
                                if let Some(view) = edit_view.upgrade() {
                                    open_edit_dialog(view, edit_id.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("delete-host-{}", host.id))
                            .small()
                            .danger()
                            .icon(Icon::default().path("icons/trash-2.svg"))
                            .label("删除")
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
        ("跳板机名称", view.jump_host_form.name.clone(), false, true),
        (
            "SSH 地址 / 域名",
            view.jump_host_form.host.clone(),
            false,
            true,
        ),
        ("SSH 端口", view.jump_host_form.port.clone(), false, true),
        (
            "登录用户名",
            view.jump_host_form.username.clone(),
            false,
            true,
        ),
        ("登录密码", view.jump_host_form.password.clone(), true, true),
        (
            "root 用户名",
            view.jump_host_form.root_username.clone(),
            false,
            true,
        ),
        (
            "root 密码",
            view.jump_host_form.root_password.clone(),
            true,
            true,
        ),
        (
            "HTTP 代理地址（可选）",
            view.jump_host_form.proxy_host.clone(),
            false,
            false,
        ),
        (
            "HTTP 代理端口",
            view.jump_host_form.proxy_port.clone(),
            false,
            false,
        ),
        (
            "代理用户名（可选）",
            view.jump_host_form.proxy_username.clone(),
            false,
            false,
        ),
        (
            "代理密码（可选）",
            view.jump_host_form.proxy_password.clone(),
            true,
            false,
        ),
    ]
}

fn configure_dialog(dialog: Dialog, view: Entity<AppView>, inputs: Vec<FormInput>) -> Dialog {
    let keyboard_save_view = view.clone();
    let button_save_view = view.clone();
    let test_view = view.clone();
    dialog
        .width(px(820.))
        .on_ok(move |_, window, cx| {
            keyboard_save_view.update(cx, |this, cx| this.save_jump_host(window, cx))
        })
        .p_0()
        .content(move |content, _, cx| {
            let view_state = view.read(cx);
            let is_editing = view_state.editing_jump_host_id.is_some();
            let form_error = view_state.jump_host_form_error.clone();
            let test_view = test_view.clone();
            let save_view = button_save_view.clone();
            content
                .w_full()
                .child(
                    DialogHeader::new()
                        .p_5()
                        .child(DialogTitle::new().child(if is_editing {
                            "编辑跳板机"
                        } else {
                            "新增跳板机"
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("登录用户和 root 用户的用户名、密码均为必填项。"),
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
                                .when(*label == "跳板机名称", |field| field.col_span_full())
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(div().text_sm().font_medium().child(*label))
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
                                .child(
                                    Input::new(state).when(*password, |input| input.mask_toggle()),
                                )
                        })),
                )
                .child(
                    DialogFooter::new()
                        .p_4()
                        .bg(cx.theme().muted)
                        .justify_between()
                        .child(
                            Button::new("test-jump-host")
                                .outline()
                                .label("测试连通性")
                                .on_click(move |_, window, cx| {
                                    test_view.update(cx, |this, cx| {
                                        this.test_jump_host_form(window, cx)
                                    });
                                }),
                        )
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
    configure_dialog(
        Dialog::new(cx).trigger(
            Button::new("add-jump-host")
                .primary()
                .icon(IconName::Plus)
                .label("新增跳板机")
                .on_click(move |_, window, cx| {
                    reset_view.update(cx, |this, cx| this.prepare_new_jump_host(window, cx));
                }),
        ),
        view,
        form_inputs(view_state),
    )
}

fn open_edit_dialog(view: Entity<AppView>, id: String, window: &mut Window, cx: &mut App) {
    if !view.update(cx, |this, cx| this.prepare_edit_jump_host(&id, window, cx)) {
        return;
    }
    let inputs = form_inputs(view.read(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        configure_dialog(dialog, view.clone(), inputs.clone())
    });
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let dialog = add_dialog(view_state, cx).into_any_element();
    let search = view_state
        .jump_host_search
        .read(cx)
        .value()
        .trim()
        .to_lowercase();
    let filtered = view_state
        .jump_hosts
        .iter()
        .filter(|host| {
            search.is_empty()
                || host.name.to_lowercase().contains(&search)
                || host.host.to_lowercase().contains(&search)
                || host.username.to_lowercase().contains(&search)
                || host.root_username.to_lowercase().contains(&search)
        })
        .collect::<Vec<_>>();
    let rows = filtered
        .into_iter()
        .map(|host| JumpHostTableRow {
            host: host.clone(),
            forward_count: view_state
                .forwards
                .iter()
                .filter(|item| item.jump_host_id == host.id)
                .count(),
            connection_count: view_state
                .ssh_tabs
                .iter()
                .filter(|tab| tab.jump_host_id == host.id)
                .count(),
        })
        .collect();
    let empty_message = if view_state.jump_hosts.is_empty() {
        "暂无跳板机，点击右上角新增"
    } else {
        "没有符合搜索条件的跳板机"
    };
    view_state.jump_host_table.update(cx, |table, cx| {
        table.delegate_mut().update_rows(rows, empty_message.into());
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
                        .child(div().text_2xl().font_semibold().child("跳板机"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("集中管理 SSH 服务器、登录用户及 root 凭据"),
                        ),
                )
                .child(dialog),
        )
        .child(div().w(px(380.)).child(
            Input::new(&view_state.jump_host_search).prefix(Icon::new(IconName::Search).small()),
        ))
        .child(
            div().flex_1().min_h_0().overflow_hidden().child(
                DataTable::new(&view_state.jump_host_table)
                    .bordered(true)
                    .scrollbar_visible(true, true),
            ),
        )
        .into_any_element()
}
