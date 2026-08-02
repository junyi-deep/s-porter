//! 紧凑的服务器搜索选择器。

use crate::forward::JumpHost;
use crate::ui::search::RegexSearch;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};

pub(in crate::ui) fn render<F>(
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
    let search = RegexSearch::new(search_input.read(cx).value().as_ref());
    let search_error = search.error().map(ToOwned::to_owned);
    let filtered = hosts
        .iter()
        .filter(|host| {
            search.matches_any([
                host.name.as_str(),
                host.host.as_str(),
                host.username.as_str(),
            ]) || search.matches(&host.port.to_string())
        })
        .cloned()
        .collect::<Vec<_>>();
    let is_empty = filtered.is_empty();

    v_flex()
        .gap_2()
        .child(
            v_flex()
                .gap_1()
                .child(
                    Input::new(search_input)
                        .prefix(Icon::new(IconName::Search).small())
                        .cleanable(true),
                )
                .when_some(search_error, |field, error| {
                    field.child(div().text_xs().text_color(cx.theme().danger).child(error))
                }),
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
                                "暂无服务器"
                            } else {
                                "没有符合搜索条件的服务器"
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
