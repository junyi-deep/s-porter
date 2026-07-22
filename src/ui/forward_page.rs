use super::app::{AppView, ForwardState, ForwardStatusFilter};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::{Dialog, DialogAction, DialogClose, DialogFooter, DialogHeader, DialogTitle},
    input::{Input, InputState},
    text::TextView,
    *,
};

type FormInput = (&'static str, Entity<InputState>, bool, bool);

fn form_inputs(view: &AppView) -> Vec<FormInput> {
    vec![
        ("配置名称", view.form.name.clone(), false, true),
        ("本地端口", view.form.local_port.clone(), false, true),
        ("远程 IP / 域名", view.form.remote_ip.clone(), false, true),
        ("远程端口", view.form.remote_port.clone(), false, true),
        ("SSH 服务 IP / 域名", view.form.ssh_ip.clone(), false, true),
        ("SSH 服务端口", view.form.ssh_port.clone(), false, true),
        ("SSH 登录用户名", view.form.ssh_user.clone(), false, true),
        ("SSH 登录密码", view.form.ssh_password.clone(), true, true),
        ("root 用户名", view.form.root_user.clone(), false, true),
        ("root 密码", view.form.root_password.clone(), true, true),
        (
            "HTTP 代理地址（可选）",
            view.form.proxy_host.clone(),
            false,
            false,
        ),
        (
            "HTTP 代理端口（填写代理地址时必填）",
            view.form.proxy_port.clone(),
            false,
            false,
        ),
        (
            "代理用户名（可选）",
            view.form.proxy_username.clone(),
            false,
            false,
        ),
        (
            "代理密码（可选）",
            view.form.proxy_password.clone(),
            true,
            false,
        ),
    ]
}

fn configure_form_dialog(dialog: Dialog, view: Entity<AppView>, inputs: Vec<FormInput>) -> Dialog {
    let test_view = view.clone();
    let enable_view = view.clone();
    dialog
        .width(px(920.))
        .on_ok(move |_, window, cx| view.update(cx, |this, cx| this.save_form(window, cx)))
        .p_0()
        .content(move |content, _, cx| {
            let enable_view = enable_view.clone();
            let test_view = test_view.clone();
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
                                .child("标有红色 * 的字段为必填项；HTTP 代理留空时直接连接 SSH。"),
                        ),
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

fn selectable_cell(id: String, text: String, width: Pixels) -> impl IntoElement {
    div()
        .w(width)
        .overflow_hidden()
        .child(TextView::markdown(id, text).selectable(true))
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
                || format!("{}@{}:{}", item.ssh_user, item.ssh_ip, item.ssh_port)
                    .to_lowercase()
                    .contains(&search);
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
    let filtered_empty = filtered.is_empty();
    let select_all_view = view.clone();
    let start_selected_view = view.clone();
    let stop_selected_view = view.clone();
    let delete_selected_view = view.clone();
    let has_selection = !view_state.selected.is_empty();

    let header = h_flex()
        .h(px(44.))
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.35))
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(
            div().w(px(36.)).child(
                Checkbox::new("select-all")
                    .checked(all_selected)
                    .tooltip("全选")
                    .on_click(move |selected, _, cx| {
                        select_all_view
                            .update(cx, |this, cx| this.select_ids(&visible_ids, *selected, cx));
                    }),
            ),
        )
        .child(div().w(px(140.)).child("名称"))
        .child(div().w(px(105.)).child("本地端口"))
        .child(div().w(px(170.)).child("远程目标"))
        .child(div().w(px(150.)).child("SSH 服务"))
        .child(div().w(px(75.)).child("状态"))
        .child(div().flex_1().child("操作"));

    let rows = filtered.into_iter().map(|item| {
        let state = state_of(item);
        let (state_label, state_color) = match &state {
            ForwardState::Stopped => ("已停止", cx.theme().muted_foreground),
            ForwardState::Starting => ("启动中", cx.theme().warning),
            ForwardState::Running => ("运行中", cx.theme().success),
            ForwardState::Failed(_) => ("失败", cx.theme().danger),
        };
        let selected = view_state.selected.contains(&item.id);
        let select_view = view.clone();
        let start_view = view.clone();
        let stop_view = view.clone();
        let enable_view = view.clone();
        let log_view = view.clone();
        let clone_view = view.clone();
        let delete_view = view.clone();
        let select_id = item.id.clone();
        let start_id = item.id.clone();
        let stop_id = item.id.clone();
        let log_id = item.id.clone();
        let clone_id = item.id.clone();
        let delete_id = item.id.clone();
        let enable_item = item.clone();
        let is_busy = matches!(state, ForwardState::Starting | ForwardState::Running);
        let can_stop = matches!(state, ForwardState::Running | ForwardState::Failed(_));

        h_flex()
            .min_h(px(58.))
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(
                div().w(px(36.)).child(
                    Checkbox::new(format!("select-{}", item.id))
                        .checked(selected)
                        .on_click(move |selected, _, cx| {
                            select_view.update(cx, |this, cx| {
                                this.toggle_selected(&select_id, *selected, cx)
                            });
                        }),
                ),
            )
            .child(selectable_cell(
                format!("name-{}", item.id),
                item.name.clone(),
                px(140.),
            ))
            .child(selectable_cell(
                format!("local-{}", item.id),
                format!("127.0.0.1:{}", item.local_port),
                px(105.),
            ))
            .child(selectable_cell(
                format!("remote-{}", item.id),
                format!("{}:{}", item.remote_ip, item.remote_port),
                px(170.),
            ))
            .child(selectable_cell(
                format!("ssh-{}", item.id),
                format!("{}@{}:{}", item.ssh_user, item.ssh_ip, item.ssh_port),
                px(150.),
            ))
            .child(div().w(px(75.)).text_color(state_color).child(state_label))
            .child(
                h_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        Button::new(format!("start-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Play)
                            .tooltip("启动转发")
                            .disabled(is_busy)
                            .on_click(move |_, window, cx| {
                                start_view.update(cx, |this, cx| {
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
                                stop_view
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
                                enable_view.update(cx, |this, cx| {
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
                                log_view.update(cx, |this, cx| this.show_logs(&log_id, window, cx));
                            }),
                    )
                    .child(
                        Button::new(format!("clone-{}", item.id))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Copy)
                            .tooltip("克隆配置")
                            .on_click(move |_, window, cx| {
                                open_clone_dialog(clone_view.clone(), clone_id.clone(), window, cx);
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
                                delete_view.update(cx, |this, cx| {
                                    this.delete_configs(vec![id], window, cx)
                                });
                            }),
                    ),
            )
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
            v_flex()
                .flex_1()
                .min_h_0()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .overflow_hidden()
                .child(header)
                .when(filtered_empty, |table| {
                    table.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(cx.theme().muted_foreground)
                            .child(if view_state.forwards.is_empty() {
                                "暂无配置，点击右上角“新增配置”开始"
                            } else {
                                "没有符合搜索或状态条件的配置"
                            }),
                    )
                })
                .children(rows),
        )
        .into_any_element()
}
