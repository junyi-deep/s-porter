//! 服务器配置控制器。

use super::*;

impl AppView {
    pub(in crate::ui) fn prepare_new_jump_host(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.servers.editing_id = None;
        self.servers.batch_mode = false;
        self.servers.form_error = None;
        self.servers.batch_entries_error = None;
        let values = [
            (&self.servers.form.name, ""),
            (&self.servers.form.host, ""),
            (&self.servers.form.batch_entries, ""),
            (&self.servers.form.batch_separator, ""),
            (&self.servers.form.port, "22"),
            (&self.servers.form.username, "paas"),
            (&self.servers.form.password, ""),
            (&self.servers.form.root_username, "root"),
            (&self.servers.form.root_password, ""),
            (&self.servers.form.proxy_host, ""),
            (&self.servers.form.proxy_port, ""),
            (&self.servers.form.proxy_username, ""),
            (&self.servers.form.proxy_password, ""),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
    }

    pub(in crate::ui) fn prepare_batch_jump_hosts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_new_jump_host(window, cx);
        self.servers.batch_mode = true;
        cx.notify();
    }

    pub(in crate::ui) fn prepare_edit_jump_host(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == id)
            .cloned()
        else {
            return false;
        };
        let proxy = host.http_proxy.unwrap_or_default();
        self.servers.editing_id = Some(host.id);
        self.servers.batch_mode = false;
        self.servers.form_error = None;
        self.servers.batch_entries_error = None;
        let values = [
            (&self.servers.form.name, host.name),
            (&self.servers.form.host, host.host),
            (&self.servers.form.port, host.port.to_string()),
            (&self.servers.form.username, host.username),
            (&self.servers.form.password, host.password),
            (&self.servers.form.root_username, host.root_username),
            (&self.servers.form.root_password, host.root_password),
            (&self.servers.form.proxy_host, proxy.host),
            (
                &self.servers.form.proxy_port,
                if proxy.port > 0 {
                    proxy.port.to_string()
                } else {
                    String::new()
                },
            ),
            (&self.servers.form.proxy_username, proxy.username),
            (&self.servers.form.proxy_password, proxy.password),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        true
    }

    pub(in crate::ui) fn prepare_copy_jump_host(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == id)
            .cloned()
        else {
            return false;
        };
        let base_name = format!("{}_copy", host.name);
        let mut copy_name = base_name.clone();
        let mut suffix = 2;
        while self
            .servers
            .jump_hosts
            .iter()
            .any(|host| host.name == copy_name)
        {
            copy_name = format!("{base_name}_{suffix}");
            suffix += 1;
        }
        let proxy = host.http_proxy.unwrap_or_default();
        self.servers.editing_id = None;
        self.servers.batch_mode = false;
        self.servers.form_error = None;
        self.servers.batch_entries_error = None;
        let values = [
            (&self.servers.form.name, copy_name),
            (&self.servers.form.host, host.host),
            (&self.servers.form.port, host.port.to_string()),
            (&self.servers.form.username, host.username),
            (&self.servers.form.password, host.password),
            (&self.servers.form.root_username, host.root_username),
            (&self.servers.form.root_password, host.root_password),
            (&self.servers.form.proxy_host, proxy.host),
            (
                &self.servers.form.proxy_port,
                if proxy.port > 0 {
                    proxy.port.to_string()
                } else {
                    String::new()
                },
            ),
            (&self.servers.form.proxy_username, proxy.username),
            (&self.servers.form.proxy_password, proxy.password),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        true
    }

    fn jump_host_form_value_with_identity(
        &self,
        id: String,
        name: String,
        host: String,
        cx: &App,
    ) -> anyhow::Result<JumpHost> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let proxy_host = value(&self.servers.form.proxy_host);
        let http_proxy = if proxy_host.trim().is_empty() {
            None
        } else {
            Some(HttpProxyConfig {
                host: proxy_host,
                port: value(&self.servers.form.proxy_port)
                    .parse()
                    .map_err(|_| anyhow::anyhow!("HTTP 代理端口必须是 1–65535 的数字"))?,
                username: value(&self.servers.form.proxy_username),
                password: value(&self.servers.form.proxy_password),
            })
        };
        let host = JumpHost {
            id,
            name,
            host,
            port: value(&self.servers.form.port)
                .parse()
                .map_err(|_| anyhow::anyhow!("SSH 端口必须是 1–65535 的数字"))?,
            username: value(&self.servers.form.username),
            password: value(&self.servers.form.password),
            root_username: value(&self.servers.form.root_username),
            root_password: value(&self.servers.form.root_password),
            http_proxy,
        };
        host.validate()?;
        Ok(host)
    }

    fn jump_host_form_value(&self, cx: &App) -> anyhow::Result<JumpHost> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        self.jump_host_form_value_with_identity(
            self.servers
                .editing_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            value(&self.servers.form.name),
            value(&self.servers.form.host),
            cx,
        )
    }

    fn jump_host_batch_values(&self, cx: &App) -> anyhow::Result<Vec<JumpHost>> {
        let source = self.servers.form.batch_entries.read(cx).value().to_string();
        let separator = self
            .servers
            .form
            .batch_separator
            .read(cx)
            .value()
            .to_string();
        let entries = parse_jump_host_batch_entries(&source, &separator)?;
        entries
            .into_iter()
            .enumerate()
            .map(|(index, (name, host))| {
                self.jump_host_form_value_with_identity(
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    host,
                    cx,
                )
                .map_err(|error| anyhow::anyhow!("第 {} 行：{error}", index + 1))
            })
            .collect()
    }

    pub(in crate::ui) fn validate_jump_host_batch_entries(
        &mut self,
        require_non_empty: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let source = self.servers.form.batch_entries.read(cx).value().to_string();
        if !require_non_empty && source.trim().is_empty() {
            self.servers.batch_entries_error = None;
            cx.notify();
            return true;
        }
        let separator = self
            .servers
            .form
            .batch_separator
            .read(cx)
            .value()
            .to_string();
        self.servers.batch_entries_error = parse_jump_host_batch_entries(&source, &separator)
            .err()
            .map(|error| error.to_string());
        cx.notify();
        self.servers.batch_entries_error.is_none()
    }

    pub(in crate::ui) fn save_jump_host(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.servers.batch_mode && !self.validate_jump_host_batch_entries(true, cx) {
            self.servers.form_error = None;
            return false;
        }
        let hosts = match if self.servers.batch_mode {
            self.jump_host_batch_values(cx)
        } else {
            self.jump_host_form_value(cx).map(|host| vec![host])
        } {
            Ok(hosts) => hosts,
            Err(error) => {
                let message = error.to_string();
                self.servers.form_error = Some(message.clone());
                return false;
            }
        };
        let mut next = self.servers.jump_hosts.clone();
        if self.servers.batch_mode {
            next.extend(hosts.iter().cloned());
        } else {
            let host = &hosts[0];
            if let Some(existing) = next.iter_mut().find(|item| item.id == host.id) {
                *existing = host.clone();
            } else {
                next.push(host.clone());
            }
        }
        let config = storage::AppConfig {
            jump_hosts: next.clone(),
            forwards: self.forwarding.configs.clone(),
            quick_commands: self.ssh.quick_commands.clone(),
            command_history: self.ssh.command_history.clone(),
            terminal_history_lines: self.ssh.terminal_history_lines,
        };
        if let Err(error) = storage::save(&config) {
            let message = format!("保存失败：{error:#}");
            self.servers.form_error = Some(message.clone());
            self.push_message(message, window, cx);
            return false;
        }
        self.servers.form_error = None;
        self.servers.batch_entries_error = None;
        self.servers.jump_hosts = next;
        if let Some(host) = hosts.first() {
            self.servers
                .selected_jump_host_id
                .get_or_insert(host.id.clone());
        }
        if self.servers.batch_mode {
            self.push_message(format!("已批量新增 {} 台服务器", hosts.len()), window, cx);
        } else {
            self.push_message("服务器配置已保存", window, cx);
        }
        cx.notify();
        true
    }

    pub(in crate::ui) fn test_jump_host_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            self.push_message("已有 SSH 操作正在执行", window, cx);
            return;
        }
        let host = match self.jump_host_form_value(cx) {
            Ok(host) => host,
            Err(error) => {
                let message = error.to_string();
                self.servers.form_error = Some(message.clone());
                return;
            }
        };
        let endpoint = format!("{}@{}:{}", host.username, host.host, host.port);
        self.busy = true;
        self.push_message(format!("正在测试 SSH 连接：{endpoint}"), window, cx);
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::test_jump_host_connection(&host) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.push_message(format!("SSH 连接测试成功：{endpoint}"), window, cx)
                    }
                    Err(error) => {
                        this.push_message(format!("SSH 连接测试失败：{error:#}"), window, cx)
                    }
                }
            });
        })
        .detach();
    }

    pub(in crate::ui) fn request_delete_jump_host(
        &self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_delete_jump_hosts(vec![id], window, cx);
    }

    pub(in crate::ui) fn request_delete_selected_jump_hosts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self
            .servers
            .jump_hosts
            .iter()
            .filter(|host| self.servers.selected.contains(&host.id))
            .map(|host| host.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.show_hint("请先选择要删除的服务器", window, cx);
            return;
        }
        self.request_delete_jump_hosts(ids, window, cx);
    }

    fn request_delete_jump_hosts(
        &self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids_set = ids.iter().map(String::as_str).collect::<HashSet<_>>();
        let hosts = self
            .servers
            .jump_hosts
            .iter()
            .filter(|host| ids_set.contains(host.id.as_str()))
            .collect::<Vec<_>>();
        if hosts.is_empty() {
            return;
        }
        let host_lines = hosts
            .iter()
            .map(|host| format!("- 服务器：{}（{}:{}）", host.name, host.host, host.port));
        let forward_names = self
            .forwarding
            .configs
            .iter()
            .filter(|item| ids_set.contains(item.jump_host_id.as_str()))
            .map(|item| format!("- 关联本地转发：{}", item.name));
        let connection_names = self
            .ssh
            .tabs
            .iter()
            .filter(|tab| ids_set.contains(tab.jump_host_id.as_str()))
            .map(|tab| format!("- 关联 SSH 连接页签：{}", tab.title));
        let details = host_lines
            .chain(forward_names)
            .chain(connection_names)
            .collect::<Vec<_>>()
            .join("\n");
        let title = if hosts.len() == 1 {
            format!("确认删除服务器“{}”？", hosts[0].name)
        } else {
            format!("确认批量删除 {} 台服务器？", hosts.len())
        };
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, window, _| {
            let delete_ids = ids.clone();
            let delete_view = view.clone();
            crate::ui::dialog_layout::responsive_dialog(dialog, window)
                .title(title.clone())
                .w(px(620.))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-delete-host").outline().label("取消")),
                        )
                        .child(DialogAction::new().child(
                            Button::new("confirm-delete-host").danger().label(
                                if delete_ids.len() > 1 {
                                    "确认批量删除"
                                } else {
                                    "确认删除"
                                },
                            ),
                        )),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child("删除后将同时停止并清理以下关联项：")
                        .child(
                            div().max_h(px(480.)).overflow_scrollbar().child(
                                TextView::markdown(
                                    format!("delete-jump-hosts-{}", delete_ids.join("-")),
                                    details.clone(),
                                )
                                .selectable(true),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    delete_view.update(cx, |this, cx| {
                        this.delete_jump_hosts(&delete_ids, window, cx)
                    })
                })
        });
    }

    fn delete_jump_hosts(
        &mut self,
        ids: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let ids_set = ids.iter().map(String::as_str).collect::<HashSet<_>>();
        let forward_ids = self
            .forwarding
            .configs
            .iter()
            .filter(|item| ids_set.contains(item.jump_host_id.as_str()))
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let connection_count = self
            .ssh
            .tabs
            .iter()
            .filter(|tab| ids_set.contains(tab.jump_host_id.as_str()))
            .count();
        let active_tab_survives = self.ssh.active_tab_id.as_deref().is_some_and(|active_id| {
            self.ssh
                .tabs
                .iter()
                .any(|tab| tab.id == active_id && !ids_set.contains(tab.jump_host_id.as_str()))
        });
        let next = storage::AppConfig {
            jump_hosts: self
                .servers
                .jump_hosts
                .iter()
                .filter(|host| !ids_set.contains(host.id.as_str()))
                .cloned()
                .collect(),
            forwards: self
                .forwarding
                .configs
                .iter()
                .filter(|item| !ids_set.contains(item.jump_host_id.as_str()))
                .cloned()
                .collect(),
            quick_commands: self.ssh.quick_commands.clone(),
            command_history: self.ssh.command_history.clone(),
            terminal_history_lines: self.ssh.terminal_history_lines,
        };
        if let Err(error) = storage::save(&next) {
            self.push_message(format!("删除失败：{error:#}"), window, cx);
            return false;
        }
        for forward_id in &forward_ids {
            if let Some(mut handle) = self.forwarding.tunnels.remove(forward_id) {
                handle.stop();
            }
            self.forwarding.states.remove(forward_id);
            self.forwarding.startup_logs.remove(forward_id);
            self.forwarding.selected.remove(forward_id);
        }
        for tab in self
            .ssh
            .tabs
            .iter()
            .filter(|tab| ids_set.contains(tab.jump_host_id.as_str()))
        {
            Self::cancel_ssh_tab_transfers(tab);
        }
        self.ssh
            .tabs
            .retain(|tab| !ids_set.contains(tab.jump_host_id.as_str()));
        self.servers.jump_hosts = next.jump_hosts;
        self.forwarding.configs = next.forwards;
        if !active_tab_survives {
            self.ssh.active_tab_id = self.ssh.tabs.last().map(|tab| tab.id.clone());
        }
        self.servers
            .selected
            .retain(|id| !ids_set.contains(id.as_str()));
        if self
            .servers
            .selected_jump_host_id
            .as_deref()
            .is_some_and(|id| ids_set.contains(id))
        {
            self.servers.selected_jump_host_id =
                self.servers.jump_hosts.first().map(|host| host.id.clone());
        }
        self.push_message(
            format!(
                "已删除 {} 台服务器，同时清理 {} 个本地转发和 {} 个 SSH 连接",
                ids_set.len(),
                forward_ids.len(),
                connection_count
            ),
            window,
            cx,
        );
        cx.notify();
        true
    }
}
