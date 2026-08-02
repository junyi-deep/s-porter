//! 应用级消息中心实体与视图。

use chrono::Local;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    notification::Notification,
    scroll::ScrollableElement,
    sheet::Sheet,
    text::TextView,
    *,
};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone)]
pub(in crate::ui) struct AppMessage {
    pub(in crate::ui) id: String,
    pub(in crate::ui) created_at: String,
    pub(in crate::ui) text: String,
}

pub(in crate::ui) struct MessageCenter {
    search: Entity<InputState>,
    history: VecDeque<AppMessage>,
    filtered_messages: Arc<Vec<AppMessage>>,
    list_state: ListState,
    _search_subscription: Subscription,
}

pub(in crate::ui) enum MessageCenterEvent {
    HistoryChanged,
}

impl EventEmitter<MessageCenterEvent> for MessageCenter {}

impl MessageCenter {
    pub(in crate::ui) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索最近 100 条消息"));
        let search_subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_filter(cx);
                cx.notify();
            }
        });
        Self {
            search,
            history: VecDeque::new(),
            filtered_messages: Arc::new(Vec::new()),
            list_state: ListState::new(0, ListAlignment::Top, px(160.)),
            _search_subscription: search_subscription,
        }
    }

    pub(in crate::ui) fn len(&self) -> usize {
        self.history.len()
    }

    pub(in crate::ui) fn push(&mut self, text: String, cx: &mut Context<Self>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        if self.history.len() >= 100 {
            self.history.pop_front();
        }
        self.history.push_back(AppMessage {
            id: id.clone(),
            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            text,
        });
        self.refresh_filter(cx);
        cx.emit(MessageCenterEvent::HistoryChanged);
        cx.notify();
        id
    }

    fn refresh_filter(&mut self, cx: &App) {
        let search = self.search.read(cx).value().trim().to_lowercase();
        let filtered_messages = self
            .history
            .iter()
            .rev()
            .filter(|message| search.is_empty() || message.text.to_lowercase().contains(&search))
            .cloned()
            .collect::<Vec<_>>();
        self.list_state
            .reset_with_uniform_height(filtered_messages.len(), px(76.));
        self.filtered_messages = Arc::new(filtered_messages);
    }
}

impl Render for MessageCenter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let messages = self.filtered_messages.clone();
        let is_empty = messages.is_empty();
        let list_state = self.list_state.clone();
        let list_state_for_scrollbar = list_state.clone();
        let history_is_empty = self.history.is_empty();

        v_flex()
            .size_full()
            .gap_3()
            .child(Input::new(&self.search))
            .child(if is_empty {
                div()
                    .flex_1()
                    .py_8()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(if history_is_empty {
                        "暂无消息"
                    } else {
                        "没有符合搜索条件的消息"
                    })
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        list(list_state.clone(), move |index, _, cx| {
                            let message = &messages[index];
                            div()
                                .pb_2()
                                .child(
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
                                                .child(message.created_at.clone()),
                                        )
                                        .child(
                                            TextView::markdown(
                                                format!("message-history-{}", message.id),
                                                message.text.clone(),
                                            )
                                            .selectable(true),
                                        ),
                                )
                                .into_any_element()
                        })
                        .with_sizing_behavior(ListSizingBehavior::Auto)
                        .size_full(),
                    )
                    .vertical_scrollbar(&list_state_for_scrollbar)
                    .into_any_element()
            })
    }
}

pub(in crate::ui) fn push(
    center: &Entity<MessageCenter>,
    message: impl Into<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let text = message.into();
    let notification_text = text.clone();
    let id = center.update(cx, |center, cx| center.push(text, cx));
    let show_close_all = window.notifications(cx).len() >= 1;
    let close_all_button_id = format!("close-all-notifications-{id}");
    window.push_notification(
        Notification::new().content(move |_, _, _| {
            v_flex()
                .gap_2()
                .child(
                    TextView::markdown(format!("notification-{id}"), notification_text.clone())
                        .selectable(true),
                )
                .when(show_close_all, |content| {
                    content.child(
                        h_flex().justify_end().child(
                            Button::new(close_all_button_id.clone())
                                .xsmall()
                                .ghost()
                                .label("关闭全部")
                                .on_click(|_, window, cx| {
                                    cx.stop_propagation();
                                    window.clear_notifications(cx);
                                }),
                        ),
                    )
                })
                .into_any_element()
        }),
        cx,
    );
}

pub(in crate::ui) fn show_hint(message: impl Into<String>, window: &mut Window, cx: &mut App) {
    let text = message.into();
    let id = uuid::Uuid::new_v4().to_string();
    let show_close_all = window.notifications(cx).len() >= 1;
    let close_all_button_id = format!("close-all-hints-{id}");
    window.push_notification(
        Notification::new().content(move |_, _, _| {
            v_flex()
                .gap_2()
                .child(TextView::markdown(format!("hint-{id}"), text.clone()).selectable(true))
                .when(show_close_all, |content| {
                    content.child(
                        h_flex().justify_end().child(
                            Button::new(close_all_button_id.clone())
                                .xsmall()
                                .ghost()
                                .label("关闭全部")
                                .on_click(|_, window, cx| {
                                    cx.stop_propagation();
                                    window.clear_notifications(cx);
                                }),
                        ),
                    )
                })
                .into_any_element()
        }),
        cx,
    );
}

pub(in crate::ui) fn render(sheet: Sheet, center: Entity<MessageCenter>, cx: &mut App) -> Sheet {
    let message_count = center.read(cx).len();

    sheet
        .title(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::Bell))
                .child(format!("消息中心（最近 {message_count} 条）")),
        )
        .size(px(460.))
        .child(center)
}
