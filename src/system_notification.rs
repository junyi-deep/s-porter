use anyhow::Context as _;
use notify_rust::Notification;

pub fn send(body: &str) -> anyhow::Result<()> {
    Notification::new()
        .appname("S Porter")
        .summary("S Porter 番茄时钟")
        .body(body)
        .show()
        .context("发送系统通知失败")?;
    Ok(())
}
