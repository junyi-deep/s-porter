//! SSH 会话页面及终端领域状态。

use gpui::{App, KeyBinding, actions};

pub(super) mod page;
pub(super) mod state;
pub(super) mod terminal;

actions!(ssh_terminal, [SendTab, SendBackTab]);

pub(super) fn init(cx: &mut App) {
    // gpui-component 的 Root 会把 Tab/Shift+Tab 绑定为全局焦点导航。
    // SSH 终端使用更具体的键盘上下文接管这两个按键，交给远端 shell。
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, Some("SshTerminal")),
        KeyBinding::new("shift-tab", SendBackTab, Some("SshTerminal")),
    ]);
}
