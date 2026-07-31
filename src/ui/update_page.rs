use super::app::{AppView, UpdatePhase};
use crate::updater;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    progress::Progress,
    *,
};

fn format_size(size: u64) -> String {
    if size < 1_024 {
        format!("{size} B")
    } else if size < 1_048_576 {
        format!("{:.1} KB", size as f64 / 1_024.)
    } else {
        format!("{:.1} MB", size as f64 / 1_048_576.)
    }
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let check_view = view.clone();
    let install_view = view;
    let checking = view_state.update_phase == UpdatePhase::Checking;
    let downloading = view_state.update_phase == UpdatePhase::Downloading;
    let progress = view_state.update_progress.snapshot();
    let percentage = progress.percentage();
    let info = view_state.update_info.clone();
    let distribution = match view_state.distribution {
        crate::Distribution::Yellow => "Yellow",
        crate::Distribution::Green => "Green",
    };
    let status_color = match view_state.update_phase {
        UpdatePhase::Available | UpdatePhase::Downloading => cx.theme().primary,
        UpdatePhase::UpToDate => cx.theme().success,
        UpdatePhase::Failed => cx.theme().danger,
        _ => cx.theme().muted_foreground,
    };

    v_flex()
        .size_full()
        .p_6()
        .gap_5()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_2xl().font_semibold().child("应用更新"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("通过内置的内网更新源检查并安装新版本"),
                ),
        )
        .child(
            v_flex()
                .max_w(px(760.))
                .p_4()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .justify_between()
                        .child(div().font_semibold().child("版本状态"))
                        .child(
                            Button::new("check-update")
                                .outline()
                                .label(if checking {
                                    "检查中…"
                                } else {
                                    "检查更新"
                                })
                                .disabled(checking || downloading)
                                .on_click(move |_, window, cx| {
                                    check_view
                                        .update(cx, |this, cx| this.check_for_update(window, cx));
                                }),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(status_color)
                        .child(view_state.update_status.clone()),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("当前版本"),
                        )
                        .child(updater::current_version())
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("当前网络分区"),
                        )
                        .child(distribution)
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("服务器版本"),
                        )
                        .child(
                            info.as_ref()
                                .map(|value| value.latest_version.clone())
                                .unwrap_or_else(|| "-".into()),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("文件大小"),
                        )
                        .child(
                            info.as_ref()
                                .map(|value| format_size(value.size))
                                .unwrap_or_else(|| "-".into()),
                        ),
                )
                .when(downloading, |panel| {
                    panel
                        .child(
                            Progress::new("application-update-progress")
                                .small()
                                .value(percentage),
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} / {}",
                                    format_size(progress.transferred),
                                    format_size(progress.total)
                                ))
                                .child(format!("{percentage:.0}%")),
                        )
                })
                .when(view_state.update_phase == UpdatePhase::Available, |panel| {
                    panel.child(
                        h_flex().justify_end().child(
                            Button::new("download-install-update")
                                .primary()
                                .label("下载并重启更新")
                                .on_click(move |_, window, cx| {
                                    install_view.update(cx, |this, cx| {
                                        this.download_and_install_update(window, cx)
                                    });
                                }),
                        ),
                    )
                }),
        )
        .into_any_element()
}
