//! SSH 会话与终端控制器。

use super::*;

impl AppView {
    pub(in crate::ui) fn open_ssh_connection(
        &mut self,
        jump_host_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == jump_host_id)
            .cloned()
        else {
            self.push_message("服务器配置不存在", window, cx);
            return;
        };
        let tab_id = uuid::Uuid::new_v4().to_string();
        let remote_path_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入远程路径"));
        let terminal_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索终端内容"));
        let path_tab_id = tab_id.clone();
        let path_subscription = cx.subscribe_in(
            &remote_path_input,
            window,
            move |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    let path = input.read(cx).value().to_string();
                    this.load_ssh_directory(&path_tab_id, path.trim(), window, cx);
                }
            },
        );
        self._subscriptions.push(path_subscription);
        let search_tab_id = tab_id.clone();
        let search_subscription = cx.subscribe_in(
            &terminal_search,
            window,
            move |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    if let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == search_tab_id)
                    {
                        tab.terminal_search_index = None;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.navigate_ssh_terminal_search(
                        &search_tab_id,
                        if *shift { -1 } else { 1 },
                        window,
                        cx,
                    );
                }
                _ => {}
            },
        );
        self._subscriptions.push(search_subscription);
        self.ssh.tabs.push(SshTab {
            id: tab_id.clone(),
            jump_host_id: host.id.clone(),
            title: host.name.clone(),
            state: SshConnectionState::Connecting,
            terminal: None,
            terminal_lines: Arc::new(vec![forward::TerminalLine {
                text: "正在建立 SSH 连接…".into(),
                styles: Vec::new(),
                cursor_column: None,
            }]),
            terminal_scroll: UniformListScrollHandle::new(),
            terminal_focus: cx.focus_handle().tab_stop(true),
            terminal_size: Rc::new(Cell::new((120, 40))),
            terminal_viewport_height: Rc::new(Cell::new(0.)),
            terminal_content_left: Rc::new(Cell::new(0.)),
            terminal_output_revision: 0,
            terminal_last_output_sync: Instant::now() - SSH_OUTPUT_FRAME_INTERVAL,
            terminal_selection: None,
            terminal_selecting: false,
            terminal_search,
            terminal_search_open: false,
            terminal_search_index: None,
            file_panel_open: false,
            remote_path: String::new(),
            remote_path_input,
            remote_entries: Vec::new(),
            file_loading: false,
            file_error: None,
            show_file_time: true,
            show_file_size: false,
            show_file_permissions: false,
            remote_sort_field: RemoteSortField::Name,
            remote_sort_ascending: true,
            terminal_font_size: None,
            transfers: Vec::new(),
            file_panel_view: SshFilePanelView::Files,
        });
        self.ssh.active_tab_id = Some(tab_id.clone());
        self.navigation.page = Page::Ssh;
        cx.notify();

        let terminal_history_lines = self.ssh.terminal_history_lines;
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::SshTerminalHandle::start_with_history_limit(
                        host,
                        terminal_history_lines,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                        tab.terminal_focus.focus(window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
                            text: format!("SSH 连接失败：{message}"),
                            styles: Vec::new(),
                            cursor_column: None,
                        }]);
                        this.push_message(format!("SSH 连接失败：{message}"), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn activate_ssh_tab(&mut self, id: String, cx: &mut Context<Self>) {
        self.ssh.active_tab_id = Some(id);
        self.sync_active_ssh_output(cx);
        cx.notify();
    }

    pub(super) fn sync_active_ssh_output(&mut self, cx: &mut Context<Self>) {
        if self.navigation.page != Page::Ssh {
            return;
        }
        let Some(active_id) = self.ssh.active_tab_id.as_deref() else {
            return;
        };
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == active_id) else {
            return;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return;
        };
        if tab.terminal_last_output_sync.elapsed() < SSH_OUTPUT_FRAME_INTERVAL {
            return;
        }
        let Some((revision, output)) = terminal.output_if_changed(tab.terminal_output_revision)
        else {
            return;
        };
        tab.terminal_last_output_sync = Instant::now();
        tab.terminal_output_revision = revision;
        tab.terminal_lines = if output.is_empty() {
            Arc::new(vec![forward::TerminalLine {
                text: "终端输出已清空".into(),
                styles: Vec::new(),
                cursor_column: None,
            }])
        } else {
            Arc::new(output)
        };
        tab.terminal_scroll.scroll_to_item(
            tab.terminal_lines.len().saturating_sub(1),
            ScrollStrategy::Bottom,
        );
        cx.notify();
    }

    pub(in crate::ui) fn close_ssh_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.ssh.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        Self::cancel_ssh_tab_transfers(&self.ssh.tabs[index]);
        self.ssh.tabs.remove(index);
        if self.ssh.active_tab_id.as_deref() == Some(id) {
            self.ssh.active_tab_id = self
                .ssh
                .tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|index| self.ssh.tabs.get(index))
                })
                .map(|tab| tab.id.clone());
        }
        cx.notify();
    }

    pub(in crate::ui) fn close_other_ssh_tabs(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.ssh.tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        for tab in self.ssh.tabs.iter().filter(|tab| tab.id != id) {
            Self::cancel_ssh_tab_transfers(tab);
        }
        self.ssh.tabs.retain(|tab| tab.id == id);
        self.ssh.active_tab_id = Some(id.to_string());
        cx.notify();
    }

    pub(in crate::ui) fn close_all_ssh_tabs(&mut self, cx: &mut Context<Self>) {
        for tab in &self.ssh.tabs {
            Self::cancel_ssh_tab_transfers(tab);
        }
        self.ssh.tabs.clear();
        self.ssh.active_tab_id = None;
        cx.notify();
    }

    pub(super) fn cancel_ssh_tab_transfers(tab: &SshTab) {
        for transfer in &tab.transfers {
            if matches!(
                transfer.status,
                TransferStatus::Running | TransferStatus::Cancelling
            ) {
                transfer.progress.cancel();
            }
        }
    }

    pub(in crate::ui) fn run_ssh_quick_command(
        &mut self,
        id: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.ssh.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if command.trim().is_empty() {
            return;
        }
        let result = self.ssh.tabs[tab_index]
            .terminal
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH 尚未连接"))
            .and_then(|terminal| terminal.send_line(command));
        match result {
            Ok(()) => {
                self.record_command_history(command.trim_end(), window, cx);
                self.ssh.tabs[tab_index].terminal_focus.focus(window, cx);
            }
            Err(error) => self.show_ssh_interaction_error(id, error.to_string(), cx),
        }
    }

    fn show_ssh_interaction_error(
        &mut self,
        id: &str,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let mut lines = tab.terminal_lines.as_ref().clone();
        lines.push(forward::TerminalLine {
            text: format!("SSH 交互错误：{}", message.into()),
            styles: Vec::new(),
            cursor_column: None,
        });
        if lines.len() > self.ssh.terminal_history_lines {
            lines.drain(..lines.len() - self.ssh.terminal_history_lines);
        }
        tab.terminal_lines = Arc::new(lines);
        tab.terminal_scroll.scroll_to_item(
            tab.terminal_lines.len().saturating_sub(1),
            ScrollStrategy::Bottom,
        );
        cx.notify();
    }

    pub(in crate::ui) fn send_ssh_keystroke(
        &mut self,
        id: &str,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let copy_shortcut = if cfg!(target_os = "macos") {
            event.keystroke.modifiers.platform
                && !event.keystroke.modifiers.control
                && event.keystroke.key.eq_ignore_ascii_case("c")
        } else {
            event.keystroke.modifiers.control
                && event.keystroke.modifiers.shift
                && event.keystroke.key.eq_ignore_ascii_case("c")
        };
        if copy_shortcut && self.copy_ssh_terminal_selection(id, cx) {
            return true;
        }
        let Some(tab) = self.ssh.tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return false;
        };
        let paste_shortcut = if cfg!(target_os = "macos") {
            event.keystroke.modifiers.platform
                && !event.keystroke.modifiers.control
                && event.keystroke.key.eq_ignore_ascii_case("v")
        } else {
            event.keystroke.modifiers.control
                && event.keystroke.modifiers.shift
                && event.keystroke.key.eq_ignore_ascii_case("v")
        };
        if paste_shortcut {
            let Some(text) = cx
                .read_from_clipboard()
                .and_then(|clipboard| clipboard.text())
            else {
                return true;
            };
            if let Err(error) = terminal.send_paste(&text) {
                self.show_ssh_interaction_error(id, format!("粘贴失败：{error:#}"), cx);
            }
            return true;
        }
        let Some(bytes) = terminal_key_bytes(
            &event.keystroke,
            terminal.application_cursor(),
            event.prefer_character_input,
        ) else {
            return false;
        };
        if let Err(error) = terminal.send_bytes(bytes) {
            self.show_ssh_interaction_error(id, format!("输入失败：{error:#}"), cx);
            return false;
        }
        true
    }

    pub(in crate::ui) fn send_ssh_terminal_tab(
        &mut self,
        id: &str,
        reverse: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return;
        };
        let bytes = if reverse {
            b"\x1b[Z".to_vec()
        } else {
            vec![b'\t']
        };
        if let Err(error) = terminal.send_bytes(bytes) {
            self.show_ssh_interaction_error(id, format!("输入失败：{error:#}"), cx);
        }
    }

    pub(in crate::ui) fn begin_ssh_terminal_selection(
        &mut self,
        id: &str,
        line: usize,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let point = TerminalPoint { line, column };
        tab.terminal_selection = Some(TerminalSelection {
            anchor: point,
            cursor: point,
        });
        tab.terminal_selecting = true;
        cx.notify();
    }

    pub(in crate::ui) fn update_ssh_terminal_selection(
        &mut self,
        id: &str,
        line: usize,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .ssh
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id && tab.terminal_selecting)
        else {
            return;
        };
        if let Some(selection) = &mut tab.terminal_selection {
            selection.cursor = TerminalPoint { line, column };
            cx.notify();
        }
    }

    pub(in crate::ui) fn finish_ssh_terminal_selection(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_selecting = false;
        cx.notify();
    }

    pub(in crate::ui) fn copy_ssh_terminal_selection(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.ssh.tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        let Some(selection) = tab.terminal_selection else {
            return false;
        };
        let Some(text) = terminal_selected_text(&tab.terminal_lines, selection) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub(in crate::ui) fn toggle_ssh_terminal_search(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_search_open = !tab.terminal_search_open;
        if tab.terminal_search_open {
            tab.terminal_search
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            tab.terminal_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(in crate::ui) fn navigate_ssh_terminal_search(
        &mut self,
        id: &str,
        direction: i32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let query = tab.terminal_search.read(cx).value().to_string();
        let matches = terminal_search_matches(&tab.terminal_lines, &query);
        if matches.is_empty() {
            tab.terminal_search_index = None;
            cx.notify();
            return;
        }
        let next = match (tab.terminal_search_index, direction.is_negative()) {
            (None, false) => 0,
            (None, true) => matches.len() - 1,
            (Some(current), false) => (current + 1) % matches.len(),
            (Some(current), true) => current.checked_sub(1).unwrap_or(matches.len() - 1),
        };
        tab.terminal_search_index = Some(next);
        tab.terminal_scroll
            .scroll_to_item(matches[next].line, ScrollStrategy::Center);
        cx.notify();
    }

    fn record_command_history(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        remember_command(&mut self.ssh.command_history, command);
        if let Err(error) = self.persist() {
            self.push_message(format!("历史命令保存失败：{error:#}"), window, cx);
        }
    }

    pub(in crate::ui) fn set_ssh_terminal_font_size(
        &mut self,
        id: &str,
        font_size: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.terminal_font_size = font_size;
        cx.notify();
    }

    pub(in crate::ui) fn terminal_history_lines(&self) -> usize {
        self.ssh.terminal_history_lines
    }

    pub(in crate::ui) fn set_terminal_history_lines(
        &mut self,
        lines: usize,
        cx: &mut Context<Self>,
    ) {
        let lines = lines.clamp(
            forward::MIN_TERMINAL_HISTORY_LINES,
            forward::MAX_TERMINAL_HISTORY_LINES,
        );
        if self.ssh.terminal_history_lines == lines {
            return;
        }
        let previous = self.ssh.terminal_history_lines;
        self.ssh.terminal_history_lines = lines;
        for tab in &self.ssh.tabs {
            if let Some(terminal) = &tab.terminal {
                terminal.set_history_limit(lines);
            }
        }
        if storage::save(&self.app_config()).is_err() {
            self.ssh.terminal_history_lines = previous;
            for tab in &self.ssh.tabs {
                if let Some(terminal) = &tab.terminal {
                    terminal.set_history_limit(previous);
                }
            }
        }
        cx.notify();
    }

    pub(in crate::ui) fn save_quick_command(
        &mut self,
        id: Option<&str>,
        name: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() || command.is_empty() {
            self.show_hint("快捷命令名称和具体命令均不能为空", window, cx);
            return false;
        }
        let previous = self.ssh.quick_commands.clone();
        if let Some(id) = id {
            let Some(existing) = self
                .ssh
                .quick_commands
                .iter_mut()
                .find(|item| item.id == id)
            else {
                self.push_message("快捷命令不存在", window, cx);
                return false;
            };
            existing.name = name.to_string();
            existing.command = command.to_string();
        } else {
            self.ssh.quick_commands.push(storage::QuickCommand {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.to_string(),
                command: command.to_string(),
            });
        }
        if let Err(error) = self.persist() {
            self.ssh.quick_commands = previous;
            self.push_message(format!("快捷命令保存失败：{error:#}"), window, cx);
            return false;
        }
        self.push_message("快捷命令已保存", window, cx);
        cx.notify();
        true
    }

    pub(in crate::ui) fn delete_quick_command(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = self.ssh.quick_commands.clone();
        self.ssh.quick_commands.retain(|command| command.id != id);
        if self.ssh.quick_commands.len() == previous.len() {
            return false;
        }
        if let Err(error) = self.persist() {
            self.ssh.quick_commands = previous;
            self.push_message(format!("快捷命令删除失败：{error:#}"), window, cx);
            return false;
        }
        self.push_message("快捷命令已删除", window, cx);
        cx.notify();
        true
    }

    pub(in crate::ui) fn clear_ssh_terminal(
        &mut self,
        id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let Some(terminal) = &tab.terminal {
            terminal.clear_output();
        }
        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
            text: "终端输出已清空".into(),
            styles: Vec::new(),
            cursor_column: None,
        }]);
        cx.notify();
    }

    pub(in crate::ui) fn reconnect_ssh_tab(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            self.push_message("服务器配置不存在，无法重连", window, cx);
            return;
        };
        tab.terminal = None;
        tab.state = SshConnectionState::Connecting;
        tab.terminal_size.set((0, 0));
        tab.terminal_output_revision = 0;
        tab.terminal_last_output_sync = Instant::now() - SSH_OUTPUT_FRAME_INTERVAL;
        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
            text: "正在重新建立 SSH 连接…".into(),
            styles: Vec::new(),
            cursor_column: None,
        }]);
        let title = tab.title.clone();
        let tab_id = id.to_string();
        self.push_message(format!("正在重连 {title}"), window, cx);
        let terminal_history_lines = self.ssh.terminal_history_lines;
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::SshTerminalHandle::start_with_history_limit(
                        host,
                        terminal_history_lines,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(terminal) => {
                        tab.terminal = Some(terminal);
                        tab.state = SshConnectionState::Connected;
                        tab.terminal_focus.focus(window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        tab.state = SshConnectionState::Failed(message.clone());
                        tab.terminal_lines = Arc::new(vec![forward::TerminalLine {
                            text: format!("SSH 重连失败：{message}"),
                            styles: Vec::new(),
                            cursor_column: None,
                        }]);
                        this.push_message(format!("SSH 重连失败：{message}"), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn toggle_ssh_file_panel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.file_panel_open = !tab.file_panel_open;
        let should_load = tab.file_panel_open && tab.remote_path.is_empty() && !tab.file_loading;
        cx.notify();
        if should_load {
            self.load_ssh_directory(id, "", window, cx);
        }
    }

    pub(in crate::ui) fn load_ssh_directory(
        &mut self,
        id: &str,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            self.push_message("服务器配置不存在，无法读取文件", window, cx);
            return;
        };
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let requested_path = path.to_string();
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::list_directory(&host, &requested_path) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                let mut resolved_path = None;
                let mut failure = None;
                {
                    let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                        return;
                    };
                    tab.file_loading = false;
                    match result {
                        Ok((path, entries)) => {
                            tab.remote_path = path.clone();
                            tab.remote_entries = entries;
                            tab.file_error = None;
                            resolved_path = Some((tab.remote_path_input.clone(), path));
                        }
                        Err(error) => {
                            let message = format!("{error:#}");
                            tab.file_error = Some(message.clone());
                            failure = Some(message);
                        }
                    }
                }
                if let Some((input, path)) = resolved_path {
                    input.update(cx, |input, cx| input.set_value(path, window, cx));
                }
                if let Some(message) = failure {
                    this.push_message(format!("远程路径跳转失败：{message}"), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn toggle_ssh_file_view(
        &mut self,
        id: &str,
        option: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        match option {
            "time" => tab.show_file_time = !tab.show_file_time,
            "size" => tab.show_file_size = !tab.show_file_size,
            "permissions" => tab.show_file_permissions = !tab.show_file_permissions,
            _ => return,
        }
        cx.notify();
    }

    pub(in crate::ui) fn sort_ssh_remote_entries(
        &mut self,
        id: &str,
        field: RemoteSortField,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.remote_sort_field == field {
            tab.remote_sort_ascending = !tab.remote_sort_ascending;
        } else {
            tab.remote_sort_field = field;
            tab.remote_sort_ascending = true;
        }
        cx.notify();
    }

    pub(in crate::ui) fn toggle_ssh_file_panel_view(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.ssh.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        tab.file_panel_view = match tab.file_panel_view {
            SshFilePanelView::Files => SshFilePanelView::Transfers,
            SshFilePanelView::Transfers => SshFilePanelView::Files,
        };
        cx.notify();
    }
}
