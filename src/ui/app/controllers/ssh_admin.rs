//! SSH 服务管理操作控制器。

use super::*;

impl AppView {
    pub(in crate::ui) fn run_ssh_operation(
        &mut self,
        item: ForwardConfig,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            self.push_message("已有 SSH 操作正在执行", window, cx);
            return;
        }
        self.busy = true;
        let operation = if enable {
            "开启允许转发"
        } else {
            "测试连接"
        };
        let id = item.id.clone();
        let Some(jump_host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == item.jump_host_id)
            .cloned()
        else {
            self.busy = false;
            self.push_message("关联的服务器配置不存在", window, cx);
            return;
        };
        self.forwarding
            .startup_logs
            .entry(id.clone())
            .or_default()
            .push(format!(
                "{operation}：开始连接 SSH 服务 {}:{}",
                jump_host.host, jump_host.port
            ));
        self.push_message(
            if enable {
                "正在配置远端 sshd"
            } else {
                "正在测试 SSH 与目标端口"
            },
            window,
            cx,
        );
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if enable {
                        forward::enable_forwarding(&jump_host)
                            .map(|_| "远端已允许 TCP 转发，sshd 已重载并验证生效".to_string())
                    } else {
                        forward::test_connection(&item, &jump_host)
                            .map(|_| "测试成功：SSH 登录及目标端口均可访问".to_string())
                    }
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(message) => {
                        this.forwarding
                            .startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("{operation}成功：{message}"));
                        this.push_message(message.clone(), window, cx);
                    }
                    Err(error) => {
                        let message = format!("操作失败：{error:#}");
                        this.forwarding
                            .startup_logs
                            .entry(id.clone())
                            .or_default()
                            .push(format!("{operation}失败：{error:#}"));
                        this.push_message(message.clone(), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui) fn run_form_ssh(
        &mut self,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.form_config(cx) {
            Ok(item) => self.run_ssh_operation(item, enable, window, cx),
            Err(error) => self.show_hint(error.to_string(), window, cx),
        }
    }
}
