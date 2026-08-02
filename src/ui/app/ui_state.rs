//! 应用导航与通用 UI 状态。

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum Page {
    JumpHosts,
    Ssh,
    Forward,
    Crypto,
    Codec,
    Format,
    Time,
    Update,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui) enum ForwardState {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum ForwardStatusFilter {
    All,
    Running,
    Stopped,
    Failed,
}
