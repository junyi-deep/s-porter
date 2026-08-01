mod forward;
mod storage;
mod system_notification;
mod toolkit;
mod ui;
mod updater;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distribution {
    Yellow,
    Green,
}

pub fn run(distribution: Distribution) {
    ui::run(distribution);
}
