use super::app::AppView;
use crate::{system_notification, toolkit};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};

const TIMEZONES: [(&str, &str); 5] = [
    ("Asia/Shanghai", "上海 UTC+8"),
    ("UTC", "UTC"),
    ("Asia/Tokyo", "东京 UTC+9"),
    ("Europe/London", "伦敦"),
    ("America/New_York", "纽约"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimeTab {
    Convert,
    Timers,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PomodoroPhase {
    Work,
    Break,
}

pub(super) struct TimeToolState {
    tab: TimeTab,
    timezone: String,
    custom_timezone_offset: i8,
    datetime: Entity<InputState>,
    milliseconds: Entity<InputState>,
    seconds: Entity<InputState>,
    days: Entity<InputState>,
    stopwatch_seconds: u64,
    stopwatch_running: bool,
    countdown_minutes: Entity<InputState>,
    countdown_seconds: Entity<InputState>,
    countdown_remaining: u64,
    countdown_running: bool,
    pomodoro_work_minutes: Entity<InputState>,
    pomodoro_break_minutes: Entity<InputState>,
    pomodoro_total_cycles_input: Entity<InputState>,
    pomodoro_total_cycles: u64,
    pomodoro_completed_cycles: u64,
    pomodoro_system_notifications: bool,
    pomodoro_remaining: u64,
    pomodoro_running: bool,
    pomodoro_phase: PomodoroPhase,
}

impl TimeToolState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
        let timezone = "Asia/Shanghai";
        let now = toolkit::format_now(timezone).unwrap_or_default();
        let (milliseconds, seconds, days) =
            toolkit::timestamp_values(&now, timezone).unwrap_or_default();
        let mut input = |value: String, placeholder: &'static str, cx: &mut Context<AppView>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            })
        };
        Self {
            tab: TimeTab::Convert,
            timezone: timezone.into(),
            custom_timezone_offset: 8,
            datetime: input(now, "YYYY-MM-DD HH:mm:ss", cx),
            milliseconds: input(milliseconds.to_string(), "毫秒时间戳", cx),
            seconds: input(seconds.to_string(), "秒时间戳", cx),
            days: input(days.to_string(), "天时间戳", cx),
            stopwatch_seconds: 0,
            stopwatch_running: false,
            countdown_minutes: input("5".into(), "分钟", cx),
            countdown_seconds: input("0".into(), "秒", cx),
            countdown_remaining: 300,
            countdown_running: false,
            pomodoro_work_minutes: input("25".into(), "专注分钟", cx),
            pomodoro_break_minutes: input("5".into(), "休息分钟", cx),
            pomodoro_total_cycles_input: input("4".into(), "总周期数", cx),
            pomodoro_total_cycles: 4,
            pomodoro_completed_cycles: 0,
            pomodoro_system_notifications: false,
            pomodoro_remaining: 25 * 60,
            pomodoro_running: false,
            pomodoro_phase: PomodoroPhase::Work,
        }
    }
}

fn set_input(
    input: &Entity<InputState>,
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<AppView>,
) {
    input.update(cx, |state, cx| state.set_value(value, window, cx));
}

fn configured_seconds(
    minutes: &Entity<InputState>,
    seconds: Option<&Entity<InputState>>,
    cx: &App,
) -> anyhow::Result<u64> {
    let minutes = minutes
        .read(cx)
        .value()
        .trim()
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("分钟必须是非负整数"))?;
    let seconds = seconds
        .map(|state| state.read(cx).value().trim().parse::<u64>())
        .transpose()
        .map_err(|_| anyhow::anyhow!("秒数必须是非负整数"))?
        .unwrap_or(0);
    minutes
        .checked_mul(60)
        .and_then(|value| value.checked_add(seconds))
        .ok_or_else(|| anyhow::anyhow!("计时时长过大"))
}

impl AppView {
    fn reset_current_time(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match toolkit::format_now(&self.time_tools.timezone) {
            Ok(value) => {
                let input = self.time_tools.datetime.clone();
                set_input(&input, value, window, cx);
                self.convert_datetime(window, cx);
            }
            Err(error) => self.push_message(error.to_string(), window, cx),
        }
    }

    fn switch_timezone(
        &mut self,
        timezone: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.time_tools.timezone = timezone.into();
        self.reset_current_time(window, cx);
        cx.notify();
    }

    fn convert_datetime(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.time_tools.datetime.read(cx).value().to_string();
        match toolkit::timestamp_values(&value, &self.time_tools.timezone) {
            Ok((milliseconds, seconds, days)) => {
                let states = [
                    (
                        self.time_tools.milliseconds.clone(),
                        milliseconds.to_string(),
                    ),
                    (self.time_tools.seconds.clone(), seconds.to_string()),
                    (self.time_tools.days.clone(), days.to_string()),
                ];
                for (state, value) in states {
                    set_input(&state, value, window, cx);
                }
            }
            Err(error) => self.push_message(format!("转换失败：{error:#}"), window, cx),
        }
    }

    fn convert_timestamp(
        &mut self,
        unit: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match unit {
            toolkit::MILLISECONDS => &self.time_tools.milliseconds,
            toolkit::SECONDS => &self.time_tools.seconds,
            _ => &self.time_tools.days,
        };
        match toolkit::from_timestamp(
            input.read(cx).value().as_ref(),
            unit,
            &self.time_tools.timezone,
        ) {
            Ok(value) => {
                let datetime = self.time_tools.datetime.clone();
                set_input(&datetime, value, window, cx);
                self.convert_datetime(window, cx);
            }
            Err(error) => self.push_message(format!("转换失败：{error:#}"), window, cx),
        }
    }

    fn adjust_custom_timezone(&mut self, delta: i8, cx: &mut Context<Self>) {
        self.time_tools.custom_timezone_offset = self
            .time_tools
            .custom_timezone_offset
            .saturating_add(delta)
            .clamp(-12, 12);
        cx.notify();
    }

    fn apply_custom_timezone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timezone = format!("UTC{:+}", self.time_tools.custom_timezone_offset);
        self.switch_timezone(timezone, window, cx);
    }

    fn toggle_stopwatch(&mut self, cx: &mut Context<Self>) {
        self.time_tools.stopwatch_running = !self.time_tools.stopwatch_running;
        cx.notify();
    }

    fn reset_stopwatch(&mut self, cx: &mut Context<Self>) {
        self.time_tools.stopwatch_running = false;
        self.time_tools.stopwatch_seconds = 0;
        cx.notify();
    }

    fn toggle_countdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.time_tools.countdown_running {
            self.time_tools.countdown_running = false;
        } else {
            if self.time_tools.countdown_remaining == 0 {
                match configured_seconds(
                    &self.time_tools.countdown_minutes,
                    Some(&self.time_tools.countdown_seconds),
                    cx,
                ) {
                    Ok(0) => {
                        self.push_message("倒计时时长必须大于 0", window, cx);
                        return;
                    }
                    Ok(value) => self.time_tools.countdown_remaining = value,
                    Err(error) => {
                        self.push_message(error.to_string(), window, cx);
                        return;
                    }
                }
            }
            self.time_tools.countdown_running = true;
        }
        cx.notify();
    }

    fn reset_countdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match configured_seconds(
            &self.time_tools.countdown_minutes,
            Some(&self.time_tools.countdown_seconds),
            cx,
        ) {
            Ok(value) => {
                self.time_tools.countdown_running = false;
                self.time_tools.countdown_remaining = value;
                cx.notify();
            }
            Err(error) => self.push_message(error.to_string(), window, cx),
        }
    }

    fn pomodoro_phase_seconds(&self, cx: &App) -> anyhow::Result<u64> {
        let input = match self.time_tools.pomodoro_phase {
            PomodoroPhase::Work => &self.time_tools.pomodoro_work_minutes,
            PomodoroPhase::Break => &self.time_tools.pomodoro_break_minutes,
        };
        configured_seconds(input, None, cx)
    }

    fn configured_pomodoro_cycles(&self, cx: &App) -> anyhow::Result<u64> {
        let cycles = self
            .time_tools
            .pomodoro_total_cycles_input
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("总周期数必须是正整数"))?;
        anyhow::ensure!(cycles > 0, "总周期数必须大于 0");
        Ok(cycles)
    }

    fn send_pomodoro_notification(
        &mut self,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_message(message.to_string(), window, cx);
        if self.time_tools.pomodoro_system_notifications
            && let Err(error) = system_notification::send(message)
        {
            self.push_message(format!("系统通知发送失败：{error:#}"), window, cx);
        }
    }

    fn toggle_pomodoro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.time_tools.pomodoro_running {
            self.time_tools.pomodoro_running = false;
        } else {
            match self.configured_pomodoro_cycles(cx) {
                Ok(cycles) => self.time_tools.pomodoro_total_cycles = cycles,
                Err(error) => {
                    self.push_message(error.to_string(), window, cx);
                    return;
                }
            }
            if self.time_tools.pomodoro_completed_cycles >= self.time_tools.pomodoro_total_cycles {
                self.time_tools.pomodoro_completed_cycles = 0;
                self.time_tools.pomodoro_phase = PomodoroPhase::Work;
                self.time_tools.pomodoro_remaining = 0;
            }
            if self.time_tools.pomodoro_remaining == 0 {
                match self.pomodoro_phase_seconds(cx) {
                    Ok(0) => {
                        self.push_message("番茄时长必须大于 0", window, cx);
                        return;
                    }
                    Ok(value) => self.time_tools.pomodoro_remaining = value,
                    Err(error) => {
                        self.push_message(error.to_string(), window, cx);
                        return;
                    }
                }
            }
            self.time_tools.pomodoro_running = true;
        }
        cx.notify();
    }

    fn reset_pomodoro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.time_tools.pomodoro_phase = PomodoroPhase::Work;
        self.time_tools.pomodoro_running = false;
        self.time_tools.pomodoro_completed_cycles = 0;
        match self.configured_pomodoro_cycles(cx) {
            Ok(cycles) => self.time_tools.pomodoro_total_cycles = cycles,
            Err(error) => {
                self.push_message(error.to_string(), window, cx);
                return;
            }
        }
        match self.pomodoro_phase_seconds(cx) {
            Ok(value) => self.time_tools.pomodoro_remaining = value,
            Err(error) => self.push_message(error.to_string(), window, cx),
        }
        cx.notify();
    }

    pub(super) fn tick_time_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.time_tools.stopwatch_running {
            self.time_tools.stopwatch_seconds = self.time_tools.stopwatch_seconds.saturating_add(1);
            changed = true;
        }
        if self.time_tools.countdown_running {
            self.time_tools.countdown_remaining =
                self.time_tools.countdown_remaining.saturating_sub(1);
            changed = true;
            if self.time_tools.countdown_remaining == 0 {
                self.time_tools.countdown_running = false;
                self.push_message("倒计时已结束", window, cx);
            }
        }
        if self.time_tools.pomodoro_running {
            self.time_tools.pomodoro_remaining =
                self.time_tools.pomodoro_remaining.saturating_sub(1);
            changed = true;
            if self.time_tools.pomodoro_remaining == 0 {
                if self.time_tools.pomodoro_phase == PomodoroPhase::Work {
                    self.time_tools.pomodoro_completed_cycles =
                        self.time_tools.pomodoro_completed_cycles.saturating_add(1);
                    if self.time_tools.pomodoro_completed_cycles
                        >= self.time_tools.pomodoro_total_cycles
                    {
                        self.time_tools.pomodoro_running = false;
                        let message = format!(
                            "番茄钟已完成：{}/{} 个周期",
                            self.time_tools.pomodoro_completed_cycles,
                            self.time_tools.pomodoro_total_cycles
                        );
                        self.send_pomodoro_notification(&message, window, cx);
                        cx.notify();
                        return;
                    }
                    self.time_tools.pomodoro_phase = PomodoroPhase::Break;
                } else {
                    self.time_tools.pomodoro_phase = PomodoroPhase::Work;
                }
                match self.pomodoro_phase_seconds(cx) {
                    Ok(value) if value > 0 => {
                        self.time_tools.pomodoro_remaining = value;
                        let message = match self.time_tools.pomodoro_phase {
                            PomodoroPhase::Work => "休息结束，开始新的专注时段",
                            PomodoroPhase::Break => "专注完成，开始休息",
                        };
                        self.send_pomodoro_notification(message, window, cx);
                    }
                    _ => self.time_tools.pomodoro_running = false,
                }
            }
        }
        if changed {
            cx.notify();
        }
    }
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn copy_button(
    id: impl Into<ElementId>,
    state: Entity<InputState>,
    view: Entity<AppView>,
) -> Button {
    Button::new(id)
        .xsmall()
        .ghost()
        .icon(IconName::Copy)
        .tooltip("复制到剪贴板")
        .on_click(move |_, window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                state.read(cx).value().to_string(),
            ));
            view.update(cx, |this, cx| {
                this.push_message("已复制到剪贴板", window, cx)
            });
        })
}

fn timestamp_row(
    label: &'static str,
    key: &'static str,
    state: Entity<InputState>,
    view: Entity<AppView>,
) -> impl IntoElement {
    let copy_view = view.clone();
    let convert_view = view;
    h_flex()
        .gap_3()
        .child(div().w(px(92.)).text_sm().font_medium().child(label))
        .child(div().flex_1().child(Input::new(&state)))
        .child(copy_button(format!("copy-{key}"), state, copy_view))
        .child(
            Button::new(format!("to-time-{key}"))
                .outline()
                .label("转为时间")
                .on_click(move |_, window, cx| {
                    convert_view.update(cx, |this, cx| this.convert_timestamp(key, window, cx));
                }),
        )
}

fn timer_card(
    title: &'static str,
    subtitle: impl Into<SharedString>,
    time: String,
    controls: impl IntoElement,
    settings: impl IntoElement,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let subtitle: SharedString = subtitle.into();
    v_flex()
        .flex_1()
        .min_w(px(250.))
        .gap_4()
        .p_5()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .gap_1()
                .child(div().font_semibold().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .child(div().text_3xl().font_family("monospace").child(time))
        .child(settings)
        .child(controls)
}

fn render_convert(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let reset_view = view.clone();
    let convert_view = view.clone();
    let timezone = view_state.time_tools.timezone.clone();
    let custom_offset = view_state.time_tools.custom_timezone_offset;
    let datetime = view_state.time_tools.datetime.clone();
    let timezone_buttons = TIMEZONES.into_iter().map(|(name, label)| {
        let view = view.clone();
        Button::new(format!("timezone-{name}"))
            .small()
            .when(name == timezone, |button| button.primary())
            .when(name != timezone, |button| button.outline())
            .label(label)
            .on_click(move |_, window, cx| {
                view.update(cx, |this, cx| this.switch_timezone(name, window, cx));
            })
    });

    v_flex()
        .gap_5()
        .child(
            v_flex()
                .gap_2()
                .child(div().text_sm().font_medium().child("时区"))
                .child(h_flex().flex_wrap().gap_2().children(timezone_buttons))
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().text_sm().font_medium().child("自定义 UTC 时区"))
                        .child(
                            Button::new("custom-timezone-minus")
                                .outline()
                                .icon(IconName::Minus)
                                .tooltip("减少一小时")
                                .disabled(custom_offset <= -12)
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.adjust_custom_timezone(-1, cx)
                                        });
                                    }
                                }),
                        )
                        .child(
                            div()
                                .w(px(72.))
                                .text_center()
                                .font_family("monospace")
                                .font_semibold()
                                .child(format!("UTC{custom_offset:+}")),
                        )
                        .child(
                            Button::new("custom-timezone-plus")
                                .outline()
                                .icon(IconName::Plus)
                                .tooltip("增加一小时")
                                .disabled(custom_offset >= 12)
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.adjust_custom_timezone(1, cx)
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("apply-custom-timezone")
                                .outline()
                                .label("应用")
                                .on_click({
                                    let view = view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.apply_custom_timezone(window, cx)
                                        });
                                    }
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("当前：{}", view_state.time_tools.timezone)),
                        ),
                ),
        )
        .child(
            v_flex()
                .gap_3()
                .p_5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(div().font_semibold().child("时间（精确到秒）"))
                .child(
                    h_flex()
                        .gap_3()
                        .child(div().flex_1().child(Input::new(&datetime)))
                        .child(copy_button("copy-datetime", datetime, view.clone()))
                        .child(Button::new("now").outline().label("当前时间").on_click(
                            move |_, window, cx| {
                                reset_view
                                    .update(cx, |this, cx| this.reset_current_time(window, cx));
                            },
                        ))
                        .child(
                            Button::new("to-timestamps")
                                .primary()
                                .label("转为时间戳")
                                .on_click(move |_, window, cx| {
                                    convert_view
                                        .update(cx, |this, cx| this.convert_datetime(window, cx));
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "格式：YYYY-MM-DD HH:mm:ss；天时间戳为自 Unix 纪元起经过的完整天数。",
                        ),
                ),
        )
        .child(
            v_flex()
                .gap_3()
                .p_5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(div().font_semibold().child("时间戳互转"))
                .child(timestamp_row(
                    "毫秒时间戳",
                    toolkit::MILLISECONDS,
                    view_state.time_tools.milliseconds.clone(),
                    view.clone(),
                ))
                .child(timestamp_row(
                    "秒时间戳",
                    toolkit::SECONDS,
                    view_state.time_tools.seconds.clone(),
                    view.clone(),
                ))
                .child(timestamp_row(
                    "天时间戳",
                    toolkit::DAYS,
                    view_state.time_tools.days.clone(),
                    view,
                )),
        )
        .into_any_element()
}

fn render_timers(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let stopwatch_view = view.clone();
    let stopwatch_reset_view = view.clone();
    let countdown_view = view.clone();
    let countdown_reset_view = view.clone();
    let pomodoro_view = view.clone();
    let pomodoro_reset_view = view.clone();
    let pomodoro_notify_view = view;
    let stopwatch_controls = h_flex()
        .gap_2()
        .child(
            Button::new("stopwatch-toggle")
                .primary()
                .icon(if view_state.time_tools.stopwatch_running {
                    IconName::Pause
                } else {
                    IconName::Play
                })
                .label(if view_state.time_tools.stopwatch_running {
                    "暂停"
                } else {
                    "开始"
                })
                .on_click(move |_, _, cx| {
                    stopwatch_view.update(cx, |this, cx| this.toggle_stopwatch(cx));
                }),
        )
        .child(
            Button::new("stopwatch-reset")
                .outline()
                .label("重置")
                .on_click(move |_, _, cx| {
                    stopwatch_reset_view.update(cx, |this, cx| this.reset_stopwatch(cx));
                }),
        );
    let countdown_settings = h_flex()
        .gap_2()
        .child(
            div()
                .w(px(100.))
                .child(Input::new(&view_state.time_tools.countdown_minutes)),
        )
        .child(div().text_sm().child("分"))
        .child(
            div()
                .w(px(100.))
                .child(Input::new(&view_state.time_tools.countdown_seconds)),
        )
        .child(div().text_sm().child("秒"));
    let countdown_controls = h_flex()
        .gap_2()
        .child(
            Button::new("countdown-toggle")
                .primary()
                .label(if view_state.time_tools.countdown_running {
                    "暂停"
                } else {
                    "开始"
                })
                .on_click(move |_, window, cx| {
                    countdown_view.update(cx, |this, cx| this.toggle_countdown(window, cx));
                }),
        )
        .child(
            Button::new("countdown-reset")
                .outline()
                .label("应用并重置")
                .on_click(move |_, window, cx| {
                    countdown_reset_view.update(cx, |this, cx| this.reset_countdown(window, cx));
                }),
        );
    let pomodoro_settings = v_flex()
        .gap_3()
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(82.))
                        .child(Input::new(&view_state.time_tools.pomodoro_work_minutes)),
                )
                .child(div().text_sm().child("专注分钟"))
                .child(
                    div()
                        .w(px(82.))
                        .child(Input::new(&view_state.time_tools.pomodoro_break_minutes)),
                )
                .child(div().text_sm().child("休息分钟")),
        )
        .child(
            h_flex()
                .gap_3()
                .child(div().w(px(82.)).child(Input::new(
                    &view_state.time_tools.pomodoro_total_cycles_input,
                )))
                .child(div().text_sm().child("总周期数"))
                .child(
                    Checkbox::new("pomodoro-system-notification")
                        .checked(view_state.time_tools.pomodoro_system_notifications)
                        .label("发送系统通知")
                        .on_click(move |checked, _, cx| {
                            pomodoro_notify_view.update(cx, |this, cx| {
                                this.time_tools.pomodoro_system_notifications = *checked;
                                cx.notify();
                            });
                        }),
                ),
        );
    let pomodoro_controls = h_flex()
        .gap_2()
        .child(
            Button::new("pomodoro-toggle")
                .primary()
                .label(if view_state.time_tools.pomodoro_running {
                    "暂停"
                } else {
                    "开始"
                })
                .on_click(move |_, window, cx| {
                    pomodoro_view.update(cx, |this, cx| this.toggle_pomodoro(window, cx));
                }),
        )
        .child(
            Button::new("pomodoro-reset")
                .outline()
                .label("重置周期")
                .on_click(move |_, window, cx| {
                    pomodoro_reset_view.update(cx, |this, cx| this.reset_pomodoro(window, cx));
                }),
        );

    h_flex()
        .items_stretch()
        .flex_wrap()
        .gap_4()
        .child(timer_card(
            "计时器",
            "累计经过时间",
            format_duration(view_state.time_tools.stopwatch_seconds),
            stopwatch_controls,
            div().h(px(34.)),
            cx,
        ))
        .child(timer_card(
            "倒计时",
            "按设定时长倒数并在结束时通知",
            format_duration(view_state.time_tools.countdown_remaining),
            countdown_controls,
            countdown_settings,
            cx,
        ))
        .child(timer_card(
            "番茄时钟",
            format!(
                "当前：{} · 已完成 {}/{} 个周期",
                match view_state.time_tools.pomodoro_phase {
                    PomodoroPhase::Work => "专注",
                    PomodoroPhase::Break => "休息",
                },
                view_state.time_tools.pomodoro_completed_cycles,
                view_state.time_tools.pomodoro_total_cycles,
            ),
            format_duration(view_state.time_tools.pomodoro_remaining),
            pomodoro_controls,
            pomodoro_settings,
            cx,
        ))
        .into_any_element()
}

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let view = cx.entity();
    let convert_view = view.clone();
    let timers_view = view;
    let tab = view_state.time_tools.tab;
    v_flex()
        .size_full()
        .p_6()
        .gap_5()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_2xl().font_semibold().child("时间工具"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("时间与多种时间戳互转，以及常用计时工具"),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("time-convert-tab")
                        .when(tab == TimeTab::Convert, |button| button.primary())
                        .when(tab != TimeTab::Convert, |button| button.ghost())
                        .label("时间转换")
                        .on_click(move |_, _, cx| {
                            convert_view.update(cx, |this, cx| {
                                this.time_tools.tab = TimeTab::Convert;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("time-timers-tab")
                        .when(tab == TimeTab::Timers, |button| button.primary())
                        .when(tab != TimeTab::Timers, |button| button.ghost())
                        .label("计时 / 倒计时 / 番茄钟")
                        .on_click(move |_, _, cx| {
                            timers_view.update(cx, |this, cx| {
                                this.time_tools.tab = TimeTab::Timers;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(match tab {
                    TimeTab::Convert => render_convert(view_state, cx),
                    TimeTab::Timers => render_timers(view_state, cx),
                }),
        )
        .into_any_element()
}
