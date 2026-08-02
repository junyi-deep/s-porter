use crate::toolkit;
use crate::ui::app::message_center::{self, MessageCenter};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    *,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatTab {
    Json,
    Xml,
}

pub(in crate::ui) struct FormatToolState {
    tab: FormatTab,
    json: FormatInputs,
    xml: FormatInputs,
}

struct FormatInputs {
    source: Entity<InputState>,
    result: Entity<InputState>,
}

impl FormatInputs {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            source: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("在此输入待处理内容")
            }),
            result: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("处理结果")
            }),
        }
    }
}

impl FormatToolState {
    pub(in crate::ui) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            tab: FormatTab::Json,
            json: FormatInputs::new(window, cx),
            xml: FormatInputs::new(window, cx),
        }
    }

    fn run(&self, action: &'static str, window: &mut Window, cx: &mut App) {
        let inputs = match self.tab {
            FormatTab::Json => &self.json,
            FormatTab::Xml => &self.xml,
        };
        let source = inputs.source.read(cx).value().to_string();
        let result = match action {
            "json-format" => toolkit::json_format(&source),
            "json-minify" => toolkit::json_minify(&source),
            "json-escape" => toolkit::json_escape_string(&source),
            "json-unescape" => toolkit::json_unescape_string(&source),
            "xml-format" => toolkit::xml_format(&source),
            "xml-minify" => toolkit::xml_minify(&source),
            _ => Err(anyhow::anyhow!("未知操作")),
        };
        let text = result.unwrap_or_else(|error| format!("错误：{error:#}"));
        inputs
            .result
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }
}

fn editor_header(
    page_key: &'static str,
    label: &'static str,
    id: &'static str,
    state: Entity<InputState>,
    messages: Entity<MessageCenter>,
    cx: &mut App,
) -> impl IntoElement {
    let copy_state = state.clone();
    let clear_state = state;
    h_flex()
        .h(px(44.))
        .flex_shrink_0()
        .justify_between()
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.35))
        .child(div().text_sm().font_semibold().child(label))
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new(format!("{page_key}-{id}-copy"))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Copy)
                        .tooltip("复制到剪贴板")
                        .on_click(move |_, window, cx| {
                            let text = copy_state.read(cx).value().to_string();
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            message_center::push(&messages, "已复制到剪贴板", window, cx);
                        }),
                )
                .child(
                    Button::new(format!("{page_key}-{id}-clear"))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Delete)
                        .tooltip("清空")
                        .on_click(move |_, window, cx| {
                            clear_state.update(cx, |state, cx| state.set_value("", window, cx));
                        }),
                ),
        )
}

fn action_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    state: Entity<FormatToolState>,
) -> Button {
    Button::new(id)
        .when(primary, |button| button.primary())
        .when(!primary, |button| button.outline())
        .label(label)
        .on_click(move |_, window, cx| state.update(cx, |state, cx| state.run(id, window, cx)))
}

pub(in crate::ui) fn render(
    state: Entity<FormatToolState>,
    messages: Entity<MessageCenter>,
    cx: &mut App,
) -> AnyElement {
    let state_ref = state.read(cx);
    let tab = state_ref.tab;
    let inputs = match tab {
        FormatTab::Json => &state_ref.json,
        FormatTab::Xml => &state_ref.xml,
    };
    let source_state = inputs.source.clone();
    let result_state = inputs.result.clone();
    let page_key = match tab {
        FormatTab::Json => "format-json",
        FormatTab::Xml => "format-xml",
    };
    let actions = match tab {
        FormatTab::Json => vec![
            action_button("json-format", "JSON 格式化", true, state.clone()),
            action_button("json-minify", "JSON 压缩", false, state.clone()),
            action_button("json-escape", "转义为字符串", false, state.clone()),
            action_button("json-unescape", "字符串反转义", false, state.clone()),
        ],
        FormatTab::Xml => vec![
            action_button("xml-format", "XML 格式化", true, state.clone()),
            action_button("xml-minify", "XML 压缩", false, state.clone()),
        ],
    };
    let json_state = state.clone();
    let xml_state = state;
    let source_header = editor_header(
        page_key,
        "输入内容",
        "source",
        source_state.clone(),
        messages.clone(),
        cx,
    )
    .into_any_element();
    let result_header = editor_header(
        page_key,
        "处理结果",
        "result",
        result_state.clone(),
        messages,
        cx,
    )
    .into_any_element();

    v_flex()
        .size_full()
        .p_6()
        .gap_5()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_2xl().font_semibold().child("格式化工具"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("格式化、压缩 JSON 与 XML，并处理 JSON 字符串转义"),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("format-json-tab")
                        .when(tab == FormatTab::Json, |button| button.primary())
                        .when(tab != FormatTab::Json, |button| button.ghost())
                        .label("JSON")
                        .on_click(move |_, _, cx| {
                            json_state.update(cx, |state, cx| {
                                state.tab = FormatTab::Json;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("format-xml-tab")
                        .when(tab == FormatTab::Xml, |button| button.primary())
                        .when(tab != FormatTab::Xml, |button| button.ghost())
                        .label("XML")
                        .on_click(move |_, _, cx| {
                            xml_state.update(cx, |state, cx| {
                                state.tab = FormatTab::Xml;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(h_flex().flex_wrap().gap_2().children(actions))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h(px(280.))
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(source_header)
                        .child(
                            div().flex_1().min_h_0().p_1().child(
                                Input::new(&source_state)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .size_full(),
                            ),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h(px(280.))
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(result_header)
                        .child(
                            div().flex_1().min_h_0().p_1().child(
                                Input::new(&result_state)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .size_full(),
                            ),
                        ),
                ),
        )
        .into_any_element()
}
