//! 端口转发运行时控制器。

use super::*;

impl AppView {
    pub(in crate::ui) fn toggle_selected(
        &mut self,
        id: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if selected {
            self.forwarding.selected.insert(id.to_string());
        } else {
            self.forwarding.selected.remove(id);
        }
        cx.notify();
    }

    pub(in crate::ui) fn toggle_jump_host_selected(
        &mut self,
        id: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if selected {
            self.servers.selected.insert(id.to_string());
        } else {
            self.servers.selected.remove(id);
        }
        cx.notify();
    }

    pub(in crate::ui) fn select_jump_host_ids(
        &mut self,
        ids: &[String],
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if selected {
            self.servers.selected.extend(ids.iter().cloned());
        } else {
            for id in ids {
                self.servers.selected.remove(id);
            }
        }
        cx.notify();
    }

    pub(in crate::ui) fn select_ids(
        &mut self,
        ids: &[String],
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if selected {
            self.forwarding.selected.extend(ids.iter().cloned());
        } else {
            for id in ids {
                self.forwarding.selected.remove(id);
            }
        }
        cx.notify();
    }

    pub(in crate::ui) fn start_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.forwarding.selected.iter().cloned().collect::<Vec<_>>();
        for id in ids {
            self.start_tunnel(&id, window, cx);
        }
    }

    pub(in crate::ui) fn stop_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.forwarding.selected.iter().cloned().collect::<Vec<_>>();
        for id in ids {
            self.stop_tunnel(&id, window, cx);
        }
    }

    pub(in crate::ui) fn delete_configs(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            self.push_message("请先选择要删除的配置", window, cx);
            return;
        }
        for id in &ids {
            if let Some(mut handle) = self.forwarding.tunnels.remove(id) {
                handle.stop();
            }
            self.forwarding.states.remove(id);
            self.forwarding.startup_logs.remove(id);
            self.forwarding.selected.remove(id);
        }
        self.forwarding
            .configs
            .retain(|item| !ids.contains(&item.id));
        match self.persist() {
            Ok(()) => self.push_message(format!("已删除 {} 个转发配置", ids.len()), window, cx),
            Err(error) => {
                self.push_message(format!("配置已删除，但保存失败：{error:#}"), window, cx)
            }
        }
        cx.notify();
    }

    pub(in crate::ui) fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_configs(
            self.forwarding.selected.iter().cloned().collect(),
            window,
            cx,
        );
    }

    pub(in crate::ui) fn start_tunnel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.forwarding.states.get(id),
            Some(ForwardState::Starting | ForwardState::Running)
        ) {
            self.push_message("该转发已在运行", window, cx);
            return;
        }
        let Some(item) = self
            .forwarding
            .configs
            .iter()
            .find(|item| item.id == id)
            .cloned()
        else {
            return;
        };
        let Some(jump_host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == item.jump_host_id)
            .cloned()
        else {
            self.push_message("关联的服务器配置不存在", window, cx);
            return;
        };
        let id = id.to_string();
        self.forwarding
            .states
            .insert(id.clone(), ForwardState::Starting);
        self.forwarding
            .startup_logs
            .entry(id.clone())
            .or_default()
            .push("开始启动：检查本地端口、SSH 认证和远程目标连通性".into());
        self.push_message(format!("{} 正在启动", item.name), window, cx);
        cx.notify();

        cx.spawn_in(window, async move |weak, cx| {
            let name = item.name.clone();
            let result = cx
                .background_executor()
                .spawn(async move { forward::TunnelHandle::start(item, jump_host) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                if !this.forwarding.configs.iter().any(|item| item.id == id) {
                    if let Ok(mut handle) = result {
                        handle.stop();
                    }
                    return;
                }
                match result {
                    Ok(handle) => {
                        this.forwarding.tunnels.insert(id.clone(), handle);
                        this.forwarding
                            .states
                            .insert(id.clone(), ForwardState::Running);
                        this.forwarding
                            .startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push("启动成功：本地监听已就绪".into());
                        this.push_message(format!("{} 启动成功", name), window, cx);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.forwarding
                            .states
                            .insert(id.clone(), ForwardState::Failed(message.clone()));
                        this.forwarding
                            .startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("启动失败：{message}"));
                        this.push_message(format!("{} 启动失败：{}", name, message), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn stop_tunnel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(mut handle) = self.forwarding.tunnels.remove(id) {
            handle.stop();
            self.forwarding
                .states
                .insert(id.to_string(), ForwardState::Stopped);
            self.forwarding
                .startup_logs
                .entry(id.to_string())
                .or_default()
                .push("转发已停止并清理本地监听".into());
            self.push_message("端口转发已停止", window, cx);
            cx.notify();
        } else if matches!(
            self.forwarding.states.get(id),
            Some(ForwardState::Failed(_))
        ) {
            self.forwarding
                .states
                .insert(id.to_string(), ForwardState::Stopped);
            cx.notify();
        } else {
            self.push_message("该转发当前未运行", window, cx);
        }
    }

    pub(in crate::ui) fn show_logs(&self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut logs = self
            .forwarding
            .startup_logs
            .get(id)
            .cloned()
            .unwrap_or_default()
            .join("\n");
        let runtime_logs = self
            .forwarding
            .tunnels
            .get(id)
            .map(|handle| handle.logs())
            .unwrap_or_default();
        if !runtime_logs.is_empty() {
            if !logs.is_empty() {
                logs.push('\n');
            }
            logs.push_str(&runtime_logs);
        }
        if let Some(ForwardState::Failed(error)) = self.forwarding.states.get(id)
            && !logs.contains(error)
        {
            if !logs.is_empty() {
                logs.push('\n');
            }
            logs.push_str(&format!("最近一次错误：{error}"));
        }
        if logs.is_empty() {
            logs = "该转发尚无运行日志。".into();
        }
        let log_view_id = format!("forward-log-{id}");
        window.open_dialog(cx, move |dialog, window, _| {
            crate::ui::dialog_layout::responsive_dialog(dialog, window)
                .title("转发日志")
                .w(px(680.))
                .min_h(px(280.))
                .child(
                    div().max_h(px(520.)).overflow_scrollbar().child(
                        TextView::markdown(log_view_id.clone(), format!("```text\n{logs}\n```"))
                            .selectable(true),
                    ),
                )
        });
    }
}
