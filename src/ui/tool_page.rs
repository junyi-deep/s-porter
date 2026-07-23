use super::app::AppView;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    *,
};

fn action_buttons(crypto: bool, cx: &mut Context<AppView>) -> Vec<Button> {
    let view = cx.entity();
    if crypto {
        let encrypt_view = view.clone();
        let decrypt_view = view;
        return vec![
            Button::new("encrypt")
                .primary()
                .label("AES-256-GCM 加密")
                .on_click(move |_, window, cx| {
                    encrypt_view.update(cx, |this, cx| this.run_crypto(false, window, cx))
                }),
            Button::new("decrypt")
                .outline()
                .label("解密")
                .on_click(move |_, window, cx| {
                    decrypt_view.update(cx, |this, cx| this.run_crypto(true, window, cx))
                }),
        ];
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
        let view = view.clone();
        Button::new(action)
            .outline()
            .label(label)
            .on_click(move |_, window, cx| {
                view.update(cx, |this, cx| this.run_codec(action, window, cx))
            })
    })
    .collect()
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

pub(super) fn render(view_state: &AppView, crypto: bool, cx: &mut Context<AppView>) -> AnyElement {
    let buttons = action_buttons(crypto, cx);
    let (title, subtitle, page_key) = if crypto {
        (
            "加解密工具",
            "使用 Argon2 派生密钥和 AES-256-GCM 认证加密",
            "crypto",
        )
    } else {
        ("编解码工具", "常用文本编码、解码与摘要计算", "codec")
    };
    let tool_inputs = if crypto {
        &view_state.crypto_tools
    } else {
        &view_state.codec_tools
    };
    let source_state = tool_inputs.source.clone();
    let result_state = tool_inputs.result.clone();
    let password_state = tool_inputs.password.clone();
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
                .child(div().text_2xl().font_semibold().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .when(crypto, |page| {
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
