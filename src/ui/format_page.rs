use super::app::{AppView, ToolInputs};
use crate::toolkit;
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

pub(super) struct FormatToolState {
    tab: FormatTab,
    json: ToolInputs,
    xml: ToolInputs,
}

impl FormatToolState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
        Self {
            tab: FormatTab::Json,
            json: ToolInputs::new(window, cx),
            xml: ToolInputs::new(window, cx),
        }
    }
}

impl AppView {
    pub(super) fn run_format(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = match self.format_tools.tab {
            FormatTab::Json => &self.format_tools.json,
            FormatTab::Xml => &self.format_tools.xml,
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
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let view = cx.entity();
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
                            view.update(cx, |this, cx| {
                                this.push_message("已复制到剪贴板", window, cx)
                            });
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
    cx: &mut Context<AppView>,
) -> Button {
    let view = cx.entity();
    Button::new(id)
        .when(primary, |button| button.primary())
        .when(!primary, |button| button.outline())
        .label(label)
        .on_click(move |_, window, cx| view.update(cx, |this, cx| this.run_format(id, window, cx)))
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let tab = view_state.format_tools.tab;
    let inputs = match tab {
        FormatTab::Json => &view_state.format_tools.json,
        FormatTab::Xml => &view_state.format_tools.xml,
    };
    let source_state = inputs.source.clone();
    let result_state = inputs.result.clone();
    let page_key = match tab {
        FormatTab::Json => "format-json",
        FormatTab::Xml => "format-xml",
    };
    let actions = match tab {
        FormatTab::Json => vec![
            action_button("json-format", "JSON 格式化", true, cx),
            action_button("json-minify", "JSON 压缩", false, cx),
            action_button("json-escape", "转义为字符串", false, cx),
            action_button("json-unescape", "字符串反转义", false, cx),
        ],
        FormatTab::Xml => vec![
            action_button("xml-format", "XML 格式化", true, cx),
            action_button("xml-minify", "XML 压缩", false, cx),
        ],
    };
    let view = cx.entity();
    let json_view = view.clone();
    let xml_view = view;
    let source_header =
        editor_header(page_key, "输入内容", "source", source_state.clone(), cx).into_any_element();
    let result_header =
        editor_header(page_key, "处理结果", "result", result_state.clone(), cx).into_any_element();

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
                            json_view.update(cx, |this, cx| {
                                this.format_tools.tab = FormatTab::Json;
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
                            xml_view.update(cx, |this, cx| {
                                this.format_tools.tab = FormatTab::Xml;
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
