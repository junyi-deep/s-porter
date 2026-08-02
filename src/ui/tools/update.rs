//! 应用更新页面与更新状态实体。

use crate::updater;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    progress::Progress,
    *,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Failed,
}

pub(in crate::ui) enum UpdateEvent {
    Available(String),
}

pub(in crate::ui) struct UpdateModel {
    distribution: crate::Distribution,
    pub(in crate::ui) info: Option<updater::UpdateInfo>,
    pub(in crate::ui) progress: updater::UpdateProgress,
    pub(in crate::ui) phase: UpdatePhase,
    pub(in crate::ui) status: String,
    last_notified_version: Option<String>,
}

impl EventEmitter<UpdateEvent> for UpdateModel {}

impl UpdateModel {
    pub(in crate::ui) fn new(
        distribution: crate::Distribution,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn_in(window, async move |weak, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            loop {
                if weak
                    .update_in(cx, |model, window, cx| model.check(true, window, cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(60 * 60))
                    .await;
            }
        })
        .detach();
        Self {
            distribution,
            info: None,
            progress: updater::UpdateProgress::default(),
            phase: UpdatePhase::Idle,
            status: "尚未检查更新".into(),
            last_notified_version: None,
        }
    }

    pub(in crate::ui) fn check(
        &mut self,
        automatic: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.phase, UpdatePhase::Checking | UpdatePhase::Downloading) {
            return;
        }
        let config = updater::configured_server(self.distribution);
        let distribution = self.distribution;
        self.phase = UpdatePhase::Checking;
        self.status = "正在连接更新服务器并检查版本…".into();
        self.info = None;
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { updater::check(&config, distribution) })
                .await;
            let _ = weak.update_in(cx, |model, _, cx| {
                match result {
                    Ok(info) => {
                        let available = info.update_available();
                        model.status = if available {
                            format!("发现新版本 {}", info.latest_version)
                        } else {
                            format!("当前已是最新版本 {}", info.current_version)
                        };
                        model.phase = if available {
                            UpdatePhase::Available
                        } else {
                            UpdatePhase::UpToDate
                        };
                        if automatic
                            && available
                            && model.last_notified_version.as_deref()
                                != Some(info.latest_version.as_str())
                        {
                            model.last_notified_version = Some(info.latest_version.clone());
                            cx.emit(UpdateEvent::Available(format!(
                                "发现新版本 {}，可以立即下载并安装",
                                info.latest_version
                            )));
                        }
                        model.info = Some(info);
                    }
                    Err(error) => {
                        model.phase = UpdatePhase::Failed;
                        model.status = format!("检查更新失败：{error:#}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn download_and_install(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.phase != UpdatePhase::Available {
            return;
        }
        let Some(info) = self.info.clone() else {
            return;
        };
        let config = updater::configured_server(self.distribution);
        let progress = updater::UpdateProgress::default();
        self.progress = progress.clone();
        self.phase = UpdatePhase::Downloading;
        self.status = format!("正在下载版本 {}…", info.latest_version);
        cx.notify();

        cx.spawn_in(window, async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_polling = weak
                    .update_in(cx, |model, _, cx| {
                        if model.phase == UpdatePhase::Downloading {
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();

        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let downloaded = updater::download(&config, &info, &progress)?;
                    updater::install_and_restart(&downloaded)?;
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            match result {
                Ok(()) => std::process::exit(0),
                Err(error) => {
                    let _ = weak.update_in(cx, |model, _, cx| {
                        model.phase = UpdatePhase::Failed;
                        model.status = format!("自动更新失败：{error:#}");
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}

fn format_size(size: u64) -> String {
    if size < 1_024 {
        format!("{size} B")
    } else if size < 1_048_576 {
        format!("{:.1} KB", size as f64 / 1_024.)
    } else {
        format!("{:.1} MB", size as f64 / 1_048_576.)
    }
}

pub(in crate::ui) fn render(
    model: Entity<UpdateModel>,
    distribution: crate::Distribution,
    cx: &mut App,
) -> AnyElement {
    let state = model.read(cx);
    let checking = state.phase == UpdatePhase::Checking;
    let downloading = state.phase == UpdatePhase::Downloading;
    let progress = state.progress.snapshot();
    let percentage = progress.percentage();
    let info = state.info.clone();
    let phase = state.phase;
    let status = state.status.clone();
    let check_model = model.clone();
    let install_model = model;
    let distribution = match distribution {
        crate::Distribution::Yellow => "Yellow",
        crate::Distribution::Green => "Green",
    };
    let status_color = match phase {
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
                                    check_model
                                        .update(cx, |model, cx| model.check(false, window, cx));
                                }),
                        ),
                )
                .child(div().text_sm().text_color(status_color).child(status))
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
                .when(phase == UpdatePhase::Available, |panel| {
                    panel.child(
                        h_flex().justify_end().child(
                            Button::new("download-install-update")
                                .primary()
                                .label("下载并重启更新")
                                .on_click(move |_, window, cx| {
                                    install_model.update(cx, |model, cx| {
                                        model.download_and_install(window, cx)
                                    });
                                }),
                        ),
                    )
                }),
        )
        .into_any_element()
}
