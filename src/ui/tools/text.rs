use crate::toolkit;
use crate::ui::app::message_center::{self, MessageCenter};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    *,
};

pub(in crate::ui) struct ToolState {
    source: Entity<InputState>,
    result: Entity<InputState>,
    password: Entity<InputState>,
    crypto_tab: CryptoTab,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CryptoTab {
    Aes,
    Base64,
}

impl ToolState {
    pub(in crate::ui) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            password: cx.new(|cx| InputState::new(window, cx).placeholder("加解密密码")),
            crypto_tab: CryptoTab::Aes,
        }
    }

    fn set_result(&self, result: anyhow::Result<String>, window: &mut Window, cx: &mut App) {
        let text = result.unwrap_or_else(|error| format!("错误：{error:#}"));
        self.result
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    fn run_codec(&self, action: &'static str, window: &mut Window, cx: &mut App) {
        let source = self.source.read(cx).value().to_string();
        let result = match action {
            "b64e" => Ok(toolkit::base64_encode(&source)),
            "b64d" => toolkit::base64_decode(&source),
            "urle" => Ok(toolkit::url_encode(&source)),
            "urld" => toolkit::url_decode(&source),
            "md5" => Ok(toolkit::md5_digest(&source)),
            "sha256" => Ok(toolkit::sha256_digest(&source)),
            _ => Err(anyhow::anyhow!("未知操作")),
        };
        self.set_result(result, window, cx);
    }

    fn run_crypto(&self, decrypt: bool, window: &mut Window, cx: &mut App) {
        let source = self.source.read(cx).value().to_string();
        let password = self.password.read(cx).value().to_string();
        let result = if decrypt {
            toolkit::decrypt(&source, &password)
        } else {
            toolkit::encrypt(&source, &password)
        };
        self.set_result(result, window, cx);
    }
}

fn action_buttons(state: Entity<ToolState>, crypto: bool, crypto_tab: CryptoTab) -> Vec<Button> {
    if crypto && crypto_tab == CryptoTab::Aes {
        let encrypt_state = state.clone();
        let decrypt_state = state;
        return vec![
            Button::new("encrypt")
                .primary()
                .label("AES-256-GCM 加密")
                .on_click(move |_, window, cx| {
                    encrypt_state.update(cx, |state, cx| state.run_crypto(false, window, cx))
                }),
            Button::new("decrypt")
                .outline()
                .label("解密")
                .on_click(move |_, window, cx| {
                    decrypt_state.update(cx, |state, cx| state.run_crypto(true, window, cx))
                }),
        ];
    }

    if crypto {
        return [("b64e", "Base64 编码"), ("b64d", "Base64 解码")]
            .into_iter()
            .map(|(action, label)| {
                let state = state.clone();
                Button::new(format!("crypto-{action}"))
                    .outline()
                    .label(label)
                    .on_click(move |_, window, cx| {
                        state.update(cx, |state, cx| state.run_codec(action, window, cx))
                    })
            })
            .collect();
    }

    [
        ("b64e", "Base64 编码"),
        ("b64d", "Base64 解码"),
        ("urle", "URL 编码"),
        ("urld", "URL 解码"),
        ("md5", "MD5 摘要"),
        ("sha256", "SHA-256 摘要"),
    ]
    .into_iter()
    .map(|(action, label)| {
        let state = state.clone();
        Button::new(action)
            .outline()
            .label(label)
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.run_codec(action, window, cx))
            })
    })
    .collect()
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

pub(in crate::ui) fn render(
    state: Entity<ToolState>,
    messages: Entity<MessageCenter>,
    crypto: bool,
    cx: &mut App,
) -> AnyElement {
    let crypto_tab = state.read(cx).crypto_tab;
    let buttons = action_buttons(state.clone(), crypto, crypto_tab);
    let (title, subtitle, page_key) = if crypto {
        (
            "加解密工具",
            "使用 Argon2 派生密钥和 AES-256-GCM 认证加密",
            "crypto",
        )
    } else {
        ("编解码工具", "常用文本编码、解码与摘要计算", "codec")
    };
    let tool_state = state.read(cx);
    let source_state = tool_state.source.clone();
    let result_state = tool_state.result.clone();
    let password_state = tool_state.password.clone();
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
                .child(div().text_2xl().font_semibold().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .when(crypto, |page| {
            let aes_state = state.clone();
            let base64_state = state.clone();
            page.child(
                h_flex()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("crypto-tab-aes")
                            .ghost()
                            .when(crypto_tab == CryptoTab::Aes, |button| button.primary())
                            .label("AES 加解密")
                            .on_click(move |_, _, cx| {
                                aes_state.update(cx, |state, cx| {
                                    state.crypto_tab = CryptoTab::Aes;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("crypto-tab-base64")
                            .ghost()
                            .when(crypto_tab == CryptoTab::Base64, |button| button.primary())
                            .label("Base64")
                            .on_click(move |_, _, cx| {
                                base64_state.update(cx, |state, cx| {
                                    state.crypto_tab = CryptoTab::Base64;
                                    cx.notify();
                                });
                            }),
                    ),
            )
        })
        .when(crypto && crypto_tab == CryptoTab::Aes, |page| {
            page.child(
                v_flex()
                    .max_w(px(520.))
                    .gap_1p5()
                    .child(div().text_sm().font_medium().child("加解密密码"))
                    .child(Input::new(&password_state).mask_toggle()),
            )
        })
        .child(h_flex().flex_wrap().gap_2().children(buttons))
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
