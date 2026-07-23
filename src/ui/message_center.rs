use super::app::AppMessage;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    input::{Input, InputState},
    scroll::ScrollableElement,
    sheet::Sheet,
    text::TextView,
    *,
};
use std::collections::VecDeque;

pub(super) fn render(
    sheet: Sheet,
    message_search: Entity<InputState>,
    history: VecDeque<AppMessage>,
    cx: &mut App,
) -> Sheet {
    let search = message_search.read(cx).value().trim().to_lowercase();
    let messages = history
        .iter()
        .rev()
        .filter(|message| search.is_empty() || message.text.to_lowercase().contains(&search))
        .cloned()
        .collect::<Vec<_>>();
    let is_empty = messages.is_empty();

    sheet
        .title(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::Bell))
                .child(format!("消息中心（最近 {} 条）", history.len())),
        )
        .size(px(460.))
        .child(
            v_flex()
                .size_full()
                .gap_3()
                .child(Input::new(&message_search))
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_2()
                        .overflow_y_scrollbar()
                        .when(is_empty, |list| {
                            list.child(
                                div()
                                    .py_8()
                                    .text_center()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if history.is_empty() {
                                        "暂无消息"
                                    } else {
                                        "没有符合搜索条件的消息"
                                    }),
                            )
                        })
                        .children(messages.into_iter().map(|message| {
                            v_flex()
                                .gap_1()
                                .p_3()
                                .rounded_lg()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(message.created_at),
                                )
                                .child(
                                    TextView::markdown(
                                        format!("message-history-{}", message.id),
                                        message.text,
                                    )
                                    .selectable(true),
                                )
                        })),
                ),
        )
}
