//! 端口转发配置表单控制器。

use super::*;

impl AppView {
    pub(super) fn form_config(&self, cx: &App) -> anyhow::Result<ForwardConfig> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let name = value(&self.forwarding.form.name);
        let remote_ip = value(&self.forwarding.form.remote_ip);
        let local_port = value(&self.forwarding.form.local_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("本地端口必须是 1–65535 的数字"))?;
        let remote_port = value(&self.forwarding.form.remote_port)
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("远程端口必须是 1–65535 的数字"))?;
        let keep_alive_interval_secs = if self.forwarding.form_keep_alive {
            value(&self.forwarding.form.keep_alive_interval)
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("心跳间隔必须是 2–3600 的数字"))?
        } else {
            30
        };
        let config = ForwardConfig {
            id: self
                .forwarding
                .editing_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name,
            local_port,
            remote_ip,
            remote_port,
            jump_host_id: self
                .servers
                .selected_jump_host_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("请先新增并选择服务器"))?,
            keep_alive: self.forwarding.form_keep_alive,
            keep_alive_interval_secs,
        };
        config.validate()?;
        anyhow::ensure!(
            self.servers
                .jump_hosts
                .iter()
                .any(|host| host.id == config.jump_host_id),
            "选择的服务器不存在"
        );
        Ok(config)
    }

    pub(super) fn app_config(&self) -> storage::AppConfig {
        storage::AppConfig {
            jump_hosts: self.servers.jump_hosts.clone(),
            forwards: self.forwarding.configs.clone(),
            quick_commands: self.ssh.quick_commands.clone(),
            command_history: self.ssh.command_history.clone(),
            terminal_history_lines: self.ssh.terminal_history_lines,
        }
    }

    pub(super) fn persist(&self) -> anyhow::Result<()> {
        storage::save(&self.app_config())
    }

    pub(in crate::ui) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.form_config(cx) {
            Ok(item) => {
                let existing = self.forwarding.editing_id.as_ref().and_then(|id| {
                    self.forwarding
                        .configs
                        .iter()
                        .position(|config| &config.id == id)
                });
                if existing.is_some_and(|index| {
                    self.forwarding
                        .tunnels
                        .contains_key(&self.forwarding.configs[index].id)
                }) {
                    self.push_message("请先停止转发，再编辑配置", window, cx);
                    return false;
                }
                let previous = existing.map(|index| {
                    std::mem::replace(&mut self.forwarding.configs[index], item.clone())
                });
                if existing.is_none() {
                    self.forwarding.configs.push(item);
                }
                if let Err(error) = self.persist() {
                    if let Some(index) = existing {
                        self.forwarding.configs[index] = previous.expect("编辑配置必须存在旧值");
                    } else {
                        self.forwarding.configs.pop();
                    }
                    self.push_message(format!("保存失败：{error:#}"), window, cx);
                    return false;
                }
                self.forwarding.editing_id = None;
                cx.notify();
                true
            }
            Err(error) => {
                self.show_hint(error.to_string(), window, cx);
                false
            }
        }
    }

    pub(super) fn set_form_value(
        input: &Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    pub(in crate::ui) fn prepare_new_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forwarding.editing_id = None;
        self.forwarding.form_keep_alive = false;
        Self::set_form_value(&self.forwarding.host_picker_search, "", window, cx);
        let values = [
            (&self.forwarding.form.name, ""),
            (&self.forwarding.form.local_port, "8080"),
            (&self.forwarding.form.remote_ip, ""),
            (&self.forwarding.form.remote_port, ""),
            (&self.forwarding.form.keep_alive_interval, "30"),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.servers.selected_jump_host_id =
            self.servers.jump_hosts.first().map(|host| host.id.clone());
    }

    pub(in crate::ui) fn prepare_clone_form(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(item) = self
            .forwarding
            .configs
            .iter()
            .find(|item| item.id == id)
            .cloned()
        else {
            return false;
        };
        self.forwarding.editing_id = None;
        self.forwarding.form_keep_alive = item.keep_alive;
        Self::set_form_value(&self.forwarding.host_picker_search, "", window, cx);
        let values = [
            (&self.forwarding.form.name, format!("{}_copy", item.name)),
            (
                &self.forwarding.form.local_port,
                item.local_port.to_string(),
            ),
            (&self.forwarding.form.remote_ip, item.remote_ip),
            (
                &self.forwarding.form.remote_port,
                item.remote_port.to_string(),
            ),
            (
                &self.forwarding.form.keep_alive_interval,
                item.keep_alive_interval_secs.to_string(),
            ),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.servers.selected_jump_host_id = Some(item.jump_host_id);
        true
    }

    pub(in crate::ui) fn prepare_edit_forward_form(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(item) = self
            .forwarding
            .configs
            .iter()
            .find(|item| item.id == id)
            .cloned()
        else {
            return false;
        };
        if self.forwarding.tunnels.contains_key(id) {
            self.push_message("请先停止转发，再编辑配置", window, cx);
            return false;
        }
        self.forwarding.editing_id = Some(item.id);
        self.forwarding.form_keep_alive = item.keep_alive;
        Self::set_form_value(&self.forwarding.host_picker_search, "", window, cx);
        let values = [
            (&self.forwarding.form.name, item.name),
            (
                &self.forwarding.form.local_port,
                item.local_port.to_string(),
            ),
            (&self.forwarding.form.remote_ip, item.remote_ip),
            (
                &self.forwarding.form.remote_port,
                item.remote_port.to_string(),
            ),
            (
                &self.forwarding.form.keep_alive_interval,
                item.keep_alive_interval_secs.to_string(),
            ),
        ];
        for (input, value) in values {
            Self::set_form_value(input, value, window, cx);
        }
        self.servers.selected_jump_host_id = Some(item.jump_host_id);
        true
    }

    pub(in crate::ui) fn set_form_keep_alive(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.forwarding.form_keep_alive = enabled;
        cx.notify();
    }

    pub(in crate::ui) fn select_forward_jump_host(&mut self, id: String, cx: &mut Context<Self>) {
        self.servers.selected_jump_host_id = Some(id);
        cx.notify();
    }

    pub(in crate::ui) fn clear_ssh_host_picker_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::set_form_value(&self.ssh.host_picker_search, "", window, cx);
    }
}
