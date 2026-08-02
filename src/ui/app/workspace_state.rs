//! 各功能工作区在主窗口中的聚合状态。

use super::{
    SshTab,
    forms::{ForwardForm, JumpHostForm},
    forward_page, jump_host_page,
    ui_state::{ForwardState, ForwardStatusFilter, Page},
};
use crate::{
    forward::{self, ForwardConfig, JumpHost},
    storage,
};
use gpui::*;
use gpui_component::{input::InputState, table::TableState};
use std::collections::{HashMap, HashSet};

pub(in crate::ui) struct NavigationState {
    pub(in crate::ui) page: Page,
    pub(in crate::ui) sidebar_collapsed: bool,
    pub(in crate::ui) ui_font_size: f32,
}

pub(in crate::ui) struct ServerWorkspace {
    pub(in crate::ui) jump_hosts: Vec<JumpHost>,
    pub(in crate::ui) selected_jump_host_id: Option<String>,
    pub(in crate::ui) form: JumpHostForm,
    pub(in crate::ui) form_error: Option<String>,
    pub(in crate::ui) editing_id: Option<String>,
    pub(in crate::ui) batch_mode: bool,
    pub(in crate::ui) search: Entity<InputState>,
    pub(in crate::ui) table: Entity<TableState<jump_host_page::JumpHostTableDelegate>>,
    pub(in crate::ui) selected: HashSet<String>,
}

pub(in crate::ui) struct ForwardWorkspace {
    pub(in crate::ui) configs: Vec<ForwardConfig>,
    pub(in crate::ui) tunnels: HashMap<String, forward::TunnelHandle>,
    pub(in crate::ui) form: ForwardForm,
    pub(in crate::ui) form_keep_alive: bool,
    pub(in crate::ui) editing_id: Option<String>,
    pub(in crate::ui) host_picker_search: Entity<InputState>,
    pub(in crate::ui) table: Entity<TableState<forward_page::ForwardTableDelegate>>,
    pub(in crate::ui) search: Entity<InputState>,
    pub(in crate::ui) status_filter: ForwardStatusFilter,
    pub(in crate::ui) states: HashMap<String, ForwardState>,
    pub(in crate::ui) startup_logs: HashMap<String, Vec<String>>,
    pub(in crate::ui) selected: HashSet<String>,
}

pub(in crate::ui) struct SshWorkspace {
    pub(in crate::ui) terminal_history_lines: usize,
    pub(in crate::ui) host_picker_search: Entity<InputState>,
    pub(in crate::ui) tabs: Vec<SshTab>,
    pub(in crate::ui) active_tab_id: Option<String>,
    pub(in crate::ui) quick_commands: Vec<storage::QuickCommand>,
    pub(in crate::ui) command_history: Vec<String>,
}
