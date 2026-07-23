#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod forward;
mod storage;
mod system_notification;
mod toolkit;
mod ui;

fn main() {
    ui::run();
}
