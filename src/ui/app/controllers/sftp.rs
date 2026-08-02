//! SSH 文件操作与传输控制器。

use super::*;

impl AppView {
    pub(in crate::ui) fn prompt_create_ssh_entry(
        &mut self,
        id: &str,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if is_dir {
                "输入文件夹名称"
            } else {
                "输入文件名称"
            })
        });
        let view = cx.entity();
        let tab_id = id.to_string();
        let kind = if is_dir { "文件夹" } else { "文件" };
        window.open_dialog(cx, move |dialog, window, _| {
            let create_view = view.clone();
            let create_name = name.clone();
            let create_tab_id = tab_id.clone();
            crate::ui::dialog_layout::responsive_dialog(dialog, window)
                .title(format!("新建{kind}"))
                .w(px(420.))
                .child(Input::new(&name))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-create-remote").outline().label("取消")),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("confirm-create-remote").primary().label("创建"),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let entry_name = create_name.read(cx).value().to_string();
                    let entry_name = entry_name.trim();
                    let validation_error = if entry_name.is_empty() {
                        Some("名称不能为空")
                    } else if entry_name == "." || entry_name == ".." {
                        Some("名称不能是“.”或“..”")
                    } else if entry_name.contains('/') || entry_name.contains('\\') {
                        Some("名称不能包含路径分隔符")
                    } else {
                        None
                    };
                    if let Some(error) = validation_error {
                        create_view.update(cx, |this, cx| {
                            this.show_hint(error, window, cx);
                        });
                        return false;
                    }
                    create_view.update(cx, |this, cx| {
                        this.create_ssh_entry(&create_tab_id, entry_name, is_dir, window, cx)
                    });
                    true
                })
        });
    }

    fn create_ssh_entry(
        &mut self,
        id: &str,
        name: &str,
        is_dir: bool,
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
            self.push_message("服务器配置不存在，无法新建文件", window, cx);
            return;
        };
        let remote_dir = if tab.remote_path.is_empty() {
            ".".to_string()
        } else {
            tab.remote_path.clone()
        };
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let entry_name = name.to_string();
        let kind = if is_dir { "文件夹" } else { "文件" };
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let remote_dir_for_create = remote_dir.clone();
            let entry_name_for_create = entry_name.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    forward::create_entry(
                        &host,
                        &remote_dir_for_create,
                        &entry_name_for_create,
                        is_dir,
                    )
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.push_message(format!("已创建{kind}：{entry_name}"), window, cx);
                    this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                }
                Err(error) => {
                    if let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.file_loading = false;
                        tab.file_error = Some(format!("{error:#}"));
                    }
                    this.push_message(format!("新建{kind}失败：{error:#}"), window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(in crate::ui) fn confirm_delete_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.name == ".." {
            return;
        }
        let view = cx.entity();
        let tab_id = id.to_string();
        let kind = if entry.is_dir { "文件夹" } else { "文件" };
        let warning = if entry.is_dir {
            format!(
                "确定删除远程文件夹“{}”吗？文件夹及其全部内容都会被删除，此操作无法撤销。",
                entry.name
            )
        } else {
            format!("确定删除远程文件“{}”吗？此操作无法撤销。", entry.name)
        };
        window.open_dialog(cx, move |dialog, window, _| {
            let delete_view = view.clone();
            let delete_id = tab_id.clone();
            let delete_entry = entry.clone();
            crate::ui::dialog_layout::responsive_dialog(dialog, window)
                .title(format!("删除{kind}"))
                .w(px(460.))
                .child(div().text_sm().child(warning.clone()))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel-delete-remote").outline().label("取消")),
                        )
                        .child(
                            Button::new("confirm-delete-remote")
                                .danger()
                                .label("确认删除")
                                .on_click(move |_, window, cx| {
                                    delete_view.update(cx, |this, cx| {
                                        this.delete_ssh_entry(
                                            &delete_id,
                                            delete_entry.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn delete_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
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
            self.push_message("跳板机配置不存在，无法删除远程文件", window, cx);
            return;
        };
        let remote_dir = tab.remote_path.clone();
        tab.file_loading = true;
        tab.file_error = None;
        let tab_id = id.to_string();
        let entry_name = entry.name.clone();
        let entry_path = entry.path.clone();
        let is_dir = entry.is_dir;
        cx.notify();
        cx.spawn_in(window, async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { forward::delete_entry(&host, &entry_path, is_dir) })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.push_message(format!("已删除远程项目：{entry_name}"), window, cx);
                    this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                }
                Err(error) => {
                    if let Some(tab) = this.ssh.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.file_loading = false;
                    }
                    this.push_message(format!("删除远程项目失败：{error:#}"), window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(in crate::ui) fn prompt_ssh_upload(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("选择要上传的文件或文件夹".into()),
        });
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let Ok(Ok(Some(paths))) = selected.await else {
                return;
            };
            let _ = weak.update_in(cx, |this, window, cx| {
                this.upload_ssh_paths(&tab_id, paths, window, cx);
            });
        })
        .detach();
    }

    pub(in crate::ui) fn upload_ssh_paths(
        &mut self,
        id: &str,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.ssh.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &self.ssh.tabs[tab_index];
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            return;
        };
        let tab_id = id.to_string();
        let remote_dir = if tab.remote_path.is_empty() {
            ".".to_string()
        } else {
            tab.remote_path.clone()
        };
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let progress = forward::TransferProgress::default();
        let mut names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .take(3)
            .collect::<Vec<_>>()
            .join("、");
        if paths.len() > 3 {
            names = format!("{names} 等 {} 项", paths.len());
        }
        self.ssh.tabs[tab_index].transfers.insert(
            0,
            SshTransfer {
                id: transfer_id.clone(),
                direction: TransferDirection::Upload,
                title: names,
                progress: progress.clone(),
                status: TransferStatus::Running,
                started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                finished_at: None,
            },
        );
        self.ssh.tabs[tab_index].file_panel_view = SshFilePanelView::Transfers;
        self.push_message(
            format!("正在上传 {} 个项目到 {}", paths.len(), remote_dir),
            window,
            cx,
        );
        cx.spawn_in(window, async move |weak, cx| {
            let remote_dir_for_upload = remote_dir.clone();
            let task_progress = progress.clone();
            let worker = std::thread::Builder::new()
                .name("s-porter-sftp-upload".into())
                .spawn(move || {
                    forward::upload(&host, &remote_dir_for_upload, &paths, &task_progress)
                });
            let result = match worker {
                Ok(worker) => {
                    while !worker.is_finished() {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = weak.update_in(cx, |_, _, cx| cx.notify());
                    }
                    worker
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("上传线程意外终止")))
                }
                Err(error) => Err(anyhow::Error::new(error).context("无法启动上传线程")),
            };
            progress.finish();
            let _ = weak.update_in(cx, |this, window, cx| {
                let cancelled = progress.is_cancelled();
                if let Some(transfer) = this
                    .ssh
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == tab_id)
                    .and_then(|tab| {
                        tab.transfers
                            .iter_mut()
                            .find(|transfer| transfer.id == transfer_id)
                    })
                {
                    transfer.finished_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                    transfer.status = match &result {
                        Ok(_) => TransferStatus::Completed,
                        Err(_) if cancelled => TransferStatus::Cancelled,
                        Err(error) => TransferStatus::Failed(format!("{error:#}")),
                    };
                }
                match result {
                    Ok(count) => {
                        this.push_message(format!("上传完成：{count} 个文件"), window, cx);
                        this.load_ssh_directory(&tab_id, &remote_dir, window, cx);
                    }
                    Err(_) if cancelled => this.push_message("上传已取消", window, cx),
                    Err(error) => {
                        this.push_message(format!("上传失败：{error:#}"), window, cx);
                    }
                }
            });
        })
        .detach();
    }

    pub(in crate::ui) fn prompt_ssh_download(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择下载位置".into()),
        });
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let Ok(Ok(Some(mut paths))) = selected.await else {
                return;
            };
            let Some(directory) = paths.pop() else {
                return;
            };
            let target = directory.join(&entry.name);
            let _ = weak.update_in(cx, |this, window, cx| {
                this.download_ssh_entry(&tab_id, entry.clone(), target.clone(), window, cx);
            });
        })
        .detach();
    }

    pub(in crate::ui) fn prepare_ssh_drag(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PathBuf {
        let target = std::env::temp_dir()
            .join("s-porter-downloads")
            .join(id)
            .join(&entry.name);
        if entry.is_dir {
            let _ = std::fs::create_dir_all(&target);
        } else if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::File::create(&target);
        }
        self.download_ssh_entry(id, entry, target.clone(), window, cx);
        target
    }

    fn download_ssh_entry(
        &mut self,
        id: &str,
        entry: forward::RemoteEntry,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.ssh.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &self.ssh.tabs[tab_index];
        let Some(host) = self
            .servers
            .jump_hosts
            .iter()
            .find(|host| host.id == tab.jump_host_id)
            .cloned()
        else {
            return;
        };
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let progress = forward::TransferProgress::default();
        self.ssh.tabs[tab_index].transfers.insert(
            0,
            SshTransfer {
                id: transfer_id.clone(),
                direction: TransferDirection::Download,
                title: entry.name.clone(),
                progress: progress.clone(),
                status: TransferStatus::Running,
                started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                finished_at: None,
            },
        );
        self.ssh.tabs[tab_index].file_panel_view = SshFilePanelView::Transfers;
        self.push_message(format!("正在下载 {}", entry.name), window, cx);
        let tab_id = id.to_string();
        cx.spawn_in(window, async move |weak, cx| {
            let task_progress = progress.clone();
            let worker = std::thread::Builder::new()
                .name("s-porter-sftp-download".into())
                .spawn(move || {
                    forward::download(&host, &entry.path, entry.is_dir, &target, &task_progress)
                        .map(|count| (count, target))
                });
            let result = match worker {
                Ok(worker) => {
                    while !worker.is_finished() {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = weak.update_in(cx, |_, _, cx| cx.notify());
                    }
                    worker
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("下载线程意外终止")))
                }
                Err(error) => Err(anyhow::Error::new(error).context("无法启动下载线程")),
            };
            progress.finish();
            let _ = weak.update_in(cx, |this, window, cx| {
                let cancelled = progress.is_cancelled();
                if let Some(transfer) = this
                    .ssh
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == tab_id)
                    .and_then(|tab| {
                        tab.transfers
                            .iter_mut()
                            .find(|transfer| transfer.id == transfer_id)
                    })
                {
                    transfer.finished_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                    transfer.status = match &result {
                        Ok(_) => TransferStatus::Completed,
                        Err(_) if cancelled => TransferStatus::Cancelled,
                        Err(error) => TransferStatus::Failed(format!("{error:#}")),
                    };
                }
                match result {
                    Ok((count, target)) => this.push_message(
                        format!("下载完成：{count} 个文件，保存到 {}", target.display()),
                        window,
                        cx,
                    ),
                    Err(_) if cancelled => this.push_message("下载已取消", window, cx),
                    Err(error) => {
                        this.push_message(format!("下载失败：{error:#}"), window, cx);
                    }
                }
            });
        })
        .detach();
    }

    pub(in crate::ui) fn cancel_ssh_transfer(
        &mut self,
        tab_id: &str,
        transfer_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(transfer) = self
            .ssh
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| {
                tab.transfers
                    .iter_mut()
                    .find(|transfer| transfer.id == transfer_id)
            })
        else {
            return;
        };
        if transfer.status == TransferStatus::Running {
            transfer.progress.cancel();
            transfer.status = TransferStatus::Cancelling;
            cx.notify();
        }
    }
}
