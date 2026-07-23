use crate::forward::JumpHost;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};

pub(super) fn render<F>(
    id_prefix: &'static str,
    hosts: &[JumpHost],
    search_input: &Entity<InputState>,
    selected_id: Option<&str>,
    on_select: F,
    cx: &mut App,
) -> AnyElement
where
    F: Fn(String, &mut Window, &mut App) + Clone + 'static,
{
    let search = search_input.read(cx).value().trim().to_lowercase();
    let filtered = hosts
        .iter()
        .filter(|host| {
            search.is_empty()
                || host.name.to_lowercase().contains(&search)
                || host.host.to_lowercase().contains(&search)
                || host.username.to_lowercase().contains(&search)
                || host.port.to_string().contains(&search)
        })
        .cloned()
        .collect::<Vec<_>>();
    let is_empty = filtered.is_empty();

    v_flex()
        .gap_2()
        .child(
            Input::new(search_input)
                .prefix(Icon::new(IconName::Search).small())
                .cleanable(true),
        )
        .child(
            v_flex()
                .max_h(px(300.))
                .min_h(px(120.))
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .overflow_y_scrollbar()
                .when(is_empty, |list| {
                    list.child(
                        div()
                            .py_8()
                            .text_sm()
                            .text_center()
                            .text_color(cx.theme().muted_foreground)
                            .child(if hosts.is_empty() {
                                "暂无跳板机"
                            } else {
                                "没有符合搜索条件的跳板机"
                            }),
                    )
                })
                .children(filtered.into_iter().map(|host| {
                    let id = host.id.clone();
                    let selected = selected_id == Some(host.id.as_str());
                    let select = on_select.clone();
                    h_flex()
                        .id(format!("{id_prefix}-{}", host.id))
                        .w_full()
                        .min_h(px(48.))
                        .px_3()
                        .py_2()
                        .gap_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .cursor_pointer()
                        .when(selected, |row| row.bg(cx.theme().primary.opacity(0.12)))
                        .hover(|row| row.bg(cx.theme().muted))
                        .child(Icon::new(IconName::SquareTerminal).small().text_color(
                            if selected {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            },
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_medium()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(host.name),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(format!(
                                            "{}@{}:{}",
                                            host.username, host.host, host.port
                                        )),
                                ),
                        )
                        .when(selected, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .small()
                                    .text_color(cx.theme().primary),
                            )
                        })
                        .on_click(move |_, window, cx| select(id.clone(), window, cx))
                })),
        )
        .into_any_element()
}
