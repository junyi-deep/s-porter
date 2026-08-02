//! SSH 页签、终端与文件传输视图状态。

use crate::forward;
use gpui::*;
use gpui_component::input::InputState;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Instant};

#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui) enum SshConnectionState {
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui) enum TransferStatus {
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum SshFilePanelView {
    Files,
    Transfers,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum RemoteSortField {
    Name,
    Modified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TerminalSearchMatch {
    pub(in crate::ui) line: usize,
    pub(in crate::ui) range: std::ops::Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::ui) struct TerminalPoint {
    pub(in crate::ui) line: usize,
    pub(in crate::ui) column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TerminalSelection {
    pub(in crate::ui) anchor: TerminalPoint,
    pub(in crate::ui) cursor: TerminalPoint,
}

pub(in crate::ui) struct SshTransfer {
    pub(in crate::ui) id: String,
    pub(in crate::ui) direction: TransferDirection,
    pub(in crate::ui) title: String,
    pub(in crate::ui) progress: forward::TransferProgress,
    pub(in crate::ui) status: TransferStatus,
    pub(in crate::ui) started_at: String,
    pub(in crate::ui) finished_at: Option<String>,
}

pub(in crate::ui) struct SshTab {
    pub(in crate::ui) id: String,
    pub(in crate::ui) jump_host_id: String,
    pub(in crate::ui) title: String,
    pub(in crate::ui) state: SshConnectionState,
    pub(in crate::ui) terminal: Option<forward::SshTerminalHandle>,
    pub(in crate::ui) terminal_lines: Arc<Vec<forward::TerminalLine>>,
    pub(in crate::ui) terminal_scroll: UniformListScrollHandle,
    pub(in crate::ui) terminal_focus: FocusHandle,
    pub(in crate::ui) terminal_size: Rc<Cell<(u16, u16)>>,
    pub(in crate::ui) terminal_viewport_height: Rc<Cell<f32>>,
    pub(in crate::ui) terminal_content_left: Rc<Cell<f32>>,
    pub(in crate::ui) terminal_output_revision: u64,
    pub(in crate::ui) terminal_last_output_sync: Instant,
    pub(in crate::ui) terminal_selection: Option<TerminalSelection>,
    pub(in crate::ui) terminal_selecting: bool,
    pub(in crate::ui) terminal_search: Entity<InputState>,
    pub(in crate::ui) terminal_search_open: bool,
    pub(in crate::ui) terminal_search_index: Option<usize>,
    pub(in crate::ui) file_panel_open: bool,
    pub(in crate::ui) remote_path: String,
    pub(in crate::ui) remote_path_input: Entity<InputState>,
    pub(in crate::ui) remote_entries: Vec<forward::RemoteEntry>,
    pub(in crate::ui) file_loading: bool,
    pub(in crate::ui) file_error: Option<String>,
    pub(in crate::ui) show_file_time: bool,
    pub(in crate::ui) show_file_size: bool,
    pub(in crate::ui) show_file_permissions: bool,
    pub(in crate::ui) remote_sort_field: RemoteSortField,
    pub(in crate::ui) remote_sort_ascending: bool,
    pub(in crate::ui) terminal_font_size: Option<f32>,
    pub(in crate::ui) transfers: Vec<SshTransfer>,
    pub(in crate::ui) file_panel_view: SshFilePanelView,
}
