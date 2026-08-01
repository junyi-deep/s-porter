use super::{JumpHost, ssh::connect};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

// A larger buffer lets libssh2 keep enough SFTP requests in flight to fill
// high-bandwidth, high-latency links. Downloads use 4x read-ahead internally,
// capped at 8 MiB by libssh2, while uploads can queue the whole supplied slice.
const UPLOAD_TRANSFER_BUFFER_SIZE: usize = 4 * 1024 * 1024;
const DOWNLOAD_TRANSFER_BUFFER_SIZE: usize = 2 * 1024 * 1024;
const PROGRESS_UPDATE_BYTES: u64 = 1024 * 1024;
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransferStage {
    #[default]
    Scanning,
    Transferring,
}

#[derive(Clone, Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_at: Option<u64>,
    pub permissions: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct TransferFileProgress {
    pub path: String,
    pub size: u64,
    pub transferred: u64,
    pub completed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TransferSnapshot {
    pub files: Vec<TransferFileProgress>,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub stage: TransferStage,
    transfer_started_at: Option<Instant>,
    transfer_finished_at: Option<Instant>,
}

impl TransferSnapshot {
    pub fn bytes_per_second(&self) -> f64 {
        let Some(started_at) = self.transfer_started_at else {
            return 0.;
        };
        let elapsed = self
            .transfer_finished_at
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(started_at)
            .as_secs_f64();
        if elapsed <= f64::EPSILON {
            0.
        } else {
            self.transferred_bytes as f64 / elapsed
        }
    }

    pub fn remaining_seconds(&self) -> Option<u64> {
        let speed = self.bytes_per_second();
        if speed <= 0. || self.total_bytes <= self.transferred_bytes {
            return None;
        }
        Some(((self.total_bytes - self.transferred_bytes) as f64 / speed).ceil() as u64)
    }
}

#[derive(Clone, Default)]
pub struct TransferProgress {
    snapshot: Arc<Mutex<TransferSnapshot>>,
    cancelled: Arc<AtomicBool>,
}

impl TransferProgress {
    pub fn snapshot(&self) -> TransferSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn check_cancelled(&self) -> Result<()> {
        anyhow::ensure!(!self.is_cancelled(), "传输已取消");
        Ok(())
    }

    fn prepare(&self, files: &[TransferManifestEntry]) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.files = files
                .iter()
                .filter(|entry| !entry.is_dir)
                .map(|entry| TransferFileProgress {
                    path: entry.display_path.clone(),
                    size: entry.size,
                    transferred: 0,
                    completed: false,
                })
                .collect();
            snapshot.total_bytes = snapshot.files.iter().map(|file| file.size).sum();
            snapshot.transferred_bytes = 0;
            snapshot.stage = TransferStage::Transferring;
            snapshot.transfer_started_at = Some(Instant::now());
            snapshot.transfer_finished_at = None;
        }
    }

    pub fn finish(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock()
            && snapshot.transfer_started_at.is_some()
            && snapshot.transfer_finished_at.is_none()
        {
            snapshot.transfer_finished_at = Some(Instant::now());
        }
    }

    fn advance(&self, file_index: usize, bytes: u64) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.transferred_bytes = snapshot.transferred_bytes.saturating_add(bytes);
            if let Some(file) = snapshot.files.get_mut(file_index) {
                file.transferred = file.transferred.saturating_add(bytes).min(file.size);
            }
        }
    }

    fn complete_file(&self, file_index: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock()
            && let Some(file) = snapshot.files.get_mut(file_index)
        {
            file.completed = true;
            file.transferred = file.size;
        }
    }
}

#[derive(Clone, Debug)]
struct TransferManifestEntry {
    local_path: PathBuf,
    remote_path: String,
    display_path: String,
    size: u64,
    is_dir: bool,
}

pub fn list_directory(
    jump_host: &JumpHost,
    requested_path: &str,
) -> Result<(String, Vec<RemoteEntry>)> {
    jump_host.validate()?;
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let requested = if requested_path.trim().is_empty() {
        "."
    } else {
        requested_path
    };
    let path = sftp
        .realpath(Path::new(requested))
        .with_context(|| format!("无法解析远程路径 {requested}"))?;
    let path_text = path.to_string_lossy().to_string();
    let mut entries = sftp
        .readdir(&path)
        .with_context(|| format!("无法读取远程目录 {path_text}"))?
        .into_iter()
        .filter_map(|(path, stat)| {
            let name = path.file_name()?.to_string_lossy().to_string();
            Some(RemoteEntry {
                path: remote_join(&path_text, &name),
                name,
                is_dir: stat.is_dir(),
                size: stat.size.unwrap_or(0),
                modified_at: stat.mtime,
                permissions: stat.perm,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if path_text != "/" {
        entries.insert(
            0,
            RemoteEntry {
                name: "..".into(),
                path: parent_path(&path_text),
                is_dir: true,
                size: 0,
                modified_at: None,
                permissions: None,
            },
        );
    }
    entries.insert(
        0,
        RemoteEntry {
            name: ".".into(),
            path: path_text.clone(),
            is_dir: true,
            size: 0,
            modified_at: None,
            permissions: None,
        },
    );
    Ok((path_text, entries))
}

pub fn create_entry(
    jump_host: &JumpHost,
    remote_dir: &str,
    name: &str,
    is_dir: bool,
) -> Result<()> {
    let name = validate_entry_name(name)?;
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let path = remote_join(remote_dir, name);
    if is_dir {
        sftp.mkdir(Path::new(&path), 0o755)
            .with_context(|| format!("创建远程文件夹失败：{path}"))?;
    } else {
        sftp.open_mode(
            Path::new(&path),
            ssh2::OpenFlags::WRITE | ssh2::OpenFlags::EXCLUSIVE,
            0o644,
            ssh2::OpenType::File,
        )
        .with_context(|| format!("创建远程文件失败：{path}"))?;
    }
    Ok(())
}

pub fn delete_entry(jump_host: &JumpHost, remote_path: &str, is_dir: bool) -> Result<()> {
    jump_host.validate()?;
    let remote_path = remote_path.trim();
    anyhow::ensure!(!remote_path.is_empty(), "远程路径不能为空");
    anyhow::ensure!(remote_path != "/", "不能删除远程根目录");
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    if is_dir {
        delete_remote_directory(&sftp, Path::new(remote_path))
    } else {
        sftp.unlink(Path::new(remote_path))
            .with_context(|| format!("删除远程文件失败：{remote_path}"))
    }
}

fn delete_remote_directory(sftp: &ssh2::Sftp, path: &Path) -> Result<()> {
    let path_text = path.to_string_lossy();
    for (child, stat) in sftp
        .readdir(path)
        .with_context(|| format!("读取待删除目录失败：{path_text}"))?
    {
        let name = child
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == "." || name == ".." {
            continue;
        }
        if stat.is_dir() {
            delete_remote_directory(sftp, &child)?;
        } else {
            sftp.unlink(&child)
                .with_context(|| format!("删除远程文件失败：{}", child.display()))?;
        }
    }
    sftp.rmdir(path)
        .with_context(|| format!("删除远程文件夹失败：{path_text}"))
}

fn validate_entry_name(name: &str) -> Result<&str> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "名称不能为空");
    anyhow::ensure!(name != "." && name != "..", "名称不能是“.”或“..”");
    anyhow::ensure!(
        !name.contains('/') && !name.contains('\\'),
        "名称不能包含路径分隔符"
    );
    Ok(name)
}

pub fn upload(
    jump_host: &JumpHost,
    remote_dir: &str,
    local_paths: &[PathBuf],
    progress: &TransferProgress,
) -> Result<usize> {
    anyhow::ensure!(!local_paths.is_empty(), "没有选择要上传的文件");
    progress.check_cancelled()?;
    let mut manifest = Vec::new();
    for local_path in local_paths {
        let name = local_path
            .file_name()
            .context("无法获取本地文件名")?
            .to_string_lossy();
        collect_local_manifest(
            local_path,
            &remote_join(remote_dir, &name),
            &mut manifest,
            progress,
        )?;
    }
    progress.prepare(&manifest);
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let mut count = 0;
    let mut file_index = 0;
    for entry in &manifest {
        progress.check_cancelled()?;
        if entry.is_dir {
            match sftp.mkdir(Path::new(&entry.remote_path), 0o755) {
                Ok(()) => {}
                Err(_) if sftp.stat(Path::new(&entry.remote_path)).is_ok() => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("创建远程目录失败：{}", entry.remote_path));
                }
            }
            continue;
        }
        let mut local = fs::File::open(&entry.local_path)
            .with_context(|| format!("打开本地文件失败：{}", entry.local_path.display()))?;
        let mut remote = sftp
            .create(Path::new(&entry.remote_path))
            .with_context(|| format!("创建远程文件失败：{}", entry.remote_path))?;
        if let Err(error) = copy_with_progress(
            &mut local,
            &mut remote,
            progress,
            file_index,
            UPLOAD_TRANSFER_BUFFER_SIZE,
        ) {
            drop(remote);
            let _ = sftp.unlink(Path::new(&entry.remote_path));
            return Err(error)
                .with_context(|| format!("上传文件失败：{}", entry.local_path.display()));
        }
        progress.complete_file(file_index);
        file_index += 1;
        count += 1;
    }
    Ok(count)
}

pub fn download(
    jump_host: &JumpHost,
    remote_path: &str,
    is_dir: bool,
    local_path: &Path,
    progress: &TransferProgress,
) -> Result<usize> {
    progress.check_cancelled()?;
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let mut manifest = Vec::new();
    collect_remote_manifest(
        &sftp,
        remote_path,
        is_dir,
        None,
        local_path,
        &mut manifest,
        progress,
    )?;
    progress.prepare(&manifest);
    let mut count = 0;
    let mut file_index = 0;
    for entry in &manifest {
        progress.check_cancelled()?;
        if entry.is_dir {
            fs::create_dir_all(&entry.local_path)
                .with_context(|| format!("创建本地目录失败：{}", entry.local_path.display()))?;
            continue;
        }
        if let Some(parent) = entry.local_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut remote = sftp
            .open(Path::new(&entry.remote_path))
            .with_context(|| format!("打开远程文件失败：{}", entry.remote_path))?;
        let mut local = fs::File::create(&entry.local_path)
            .with_context(|| format!("创建本地文件失败：{}", entry.local_path.display()))?;
        if let Err(error) = copy_with_progress(
            &mut remote,
            &mut local,
            progress,
            file_index,
            DOWNLOAD_TRANSFER_BUFFER_SIZE,
        ) {
            drop(local);
            let _ = fs::remove_file(&entry.local_path);
            return Err(error).with_context(|| format!("下载文件失败：{}", entry.remote_path));
        }
        progress.complete_file(file_index);
        file_index += 1;
        count += 1;
    }
    Ok(count)
}

pub fn parent_path(path: &str) -> String {
    if path == "/" {
        return "/".into();
    }
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(parent, _)| {
            if parent.is_empty() {
                "/".into()
            } else {
                parent.into()
            }
        })
        .unwrap_or_else(|| ".".into())
}

fn remote_join(directory: &str, name: &str) -> String {
    if directory == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", directory.trim_end_matches('/'))
    }
}

fn collect_local_manifest(
    local_path: &Path,
    remote_path: &str,
    manifest: &mut Vec<TransferManifestEntry>,
    progress: &TransferProgress,
) -> Result<()> {
    progress.check_cancelled()?;
    let metadata = fs::symlink_metadata(local_path)
        .with_context(|| format!("读取本地文件信息失败：{}", local_path.display()))?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "不支持上传符号链接：{}",
        local_path.display()
    );
    if metadata.is_dir() {
        manifest.push(TransferManifestEntry {
            local_path: local_path.to_path_buf(),
            remote_path: remote_path.to_string(),
            display_path: local_path.display().to_string(),
            size: 0,
            is_dir: true,
        });
        for child in fs::read_dir(local_path)
            .with_context(|| format!("读取本地目录失败：{}", local_path.display()))?
        {
            let child = child?;
            let name = child.file_name().to_string_lossy().to_string();
            collect_local_manifest(
                &child.path(),
                &remote_join(remote_path, &name),
                manifest,
                progress,
            )?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        bail!("不支持上传该类型：{}", local_path.display());
    }
    manifest.push(TransferManifestEntry {
        local_path: local_path.to_path_buf(),
        remote_path: remote_path.to_string(),
        display_path: local_path.display().to_string(),
        size: metadata.len(),
        is_dir: false,
    });
    Ok(())
}

fn collect_remote_manifest(
    sftp: &ssh2::Sftp,
    remote_path: &str,
    is_dir: bool,
    known_size: Option<u64>,
    local_path: &Path,
    manifest: &mut Vec<TransferManifestEntry>,
    progress: &TransferProgress,
) -> Result<()> {
    progress.check_cancelled()?;
    if is_dir {
        manifest.push(TransferManifestEntry {
            local_path: local_path.to_path_buf(),
            remote_path: remote_path.to_string(),
            display_path: remote_path.to_string(),
            size: 0,
            is_dir: true,
        });
        for (child, stat) in sftp
            .readdir(Path::new(remote_path))
            .with_context(|| format!("读取远程目录失败：{remote_path}"))?
        {
            let Some(name) = child.file_name() else {
                continue;
            };
            let name = name.to_string_lossy();
            collect_remote_manifest(
                sftp,
                &remote_join(remote_path, &name),
                stat.is_dir(),
                stat.size,
                &local_path.join(name.as_ref()),
                manifest,
                progress,
            )?;
        }
        return Ok(());
    }
    let size = match known_size {
        Some(size) => size,
        None => sftp
            .stat(Path::new(remote_path))
            .with_context(|| format!("读取远程文件信息失败：{remote_path}"))?
            .size
            .unwrap_or(0),
    };
    manifest.push(TransferManifestEntry {
        local_path: local_path.to_path_buf(),
        remote_path: remote_path.to_string(),
        display_path: remote_path.to_string(),
        size,
        is_dir: false,
    });
    Ok(())
}

fn copy_with_progress(
    reader: &mut impl Read,
    writer: &mut impl Write,
    progress: &TransferProgress,
    file_index: usize,
    buffer_size: usize,
) -> Result<()> {
    debug_assert!(buffer_size > 0);
    let mut buffer = vec![0_u8; buffer_size.max(1)];
    let mut pending_progress = 0_u64;
    let mut last_progress_update = Instant::now();
    loop {
        if progress.is_cancelled() {
            if pending_progress > 0 {
                progress.advance(file_index, pending_progress);
            }
            progress.check_cancelled()?;
        }
        if pending_progress > 0
            && (pending_progress >= PROGRESS_UPDATE_BYTES
                || last_progress_update.elapsed() >= PROGRESS_UPDATE_INTERVAL)
        {
            progress.advance(file_index, pending_progress);
            pending_progress = 0;
            last_progress_update = Instant::now();
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        pending_progress = pending_progress.saturating_add(read as u64);
    }
    if pending_progress > 0 {
        progress.advance(file_index, pending_progress);
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::HttpProxyConfig;
    use ssh2::MethodType;
    use std::io::Cursor;

    const INTEGRATION_TRANSFER_SIZE: usize = 8 * 1024 * 1024;

    struct CancellingWriter {
        bytes: Vec<u8>,
        progress: TransferProgress,
    }

    impl Write for CancellingWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            self.progress.cancel();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn joins_remote_paths_without_platform_separators() {
        assert_eq!(remote_join("/", "etc"), "/etc");
        assert_eq!(
            remote_join("/home/tester/", "file.txt"),
            "/home/tester/file.txt"
        );
    }

    #[test]
    fn finds_remote_parent() {
        assert_eq!(parent_path("/home/tester"), "/home");
        assert_eq!(parent_path("/"), "/");
    }

    #[test]
    fn rejects_invalid_remote_entry_names() {
        assert!(validate_entry_name("").is_err());
        assert!(validate_entry_name("..").is_err());
        assert!(validate_entry_name("a/b").is_err());
        assert_eq!(validate_entry_name(" notes.txt ").unwrap(), "notes.txt");
    }

    #[test]
    fn transfer_progress_tracks_each_file_and_total_bytes() {
        let progress = TransferProgress::default();
        progress.prepare(&[
            TransferManifestEntry {
                local_path: "a".into(),
                remote_path: "/a".into(),
                display_path: "a".into(),
                size: 3,
                is_dir: false,
            },
            TransferManifestEntry {
                local_path: "b".into(),
                remote_path: "/b".into(),
                display_path: "b".into(),
                size: 2,
                is_dir: false,
            },
        ]);
        let mut output = Vec::new();
        copy_with_progress(&mut Cursor::new(b"abc"), &mut output, &progress, 0, 32).unwrap();
        progress.complete_file(0);

        let snapshot = progress.snapshot();
        assert_eq!(snapshot.total_bytes, 5);
        assert_eq!(snapshot.transferred_bytes, 3);
        assert_eq!(snapshot.files[0].transferred, 3);
        assert!(snapshot.files[0].completed);
        assert_eq!(snapshot.files[1].transferred, 0);
    }

    #[test]
    fn cancelled_transfer_stops_before_copying_more_data() {
        let progress = TransferProgress::default();
        progress.prepare(&[TransferManifestEntry {
            local_path: "large".into(),
            remote_path: "/large".into(),
            display_path: "large".into(),
            size: (UPLOAD_TRANSFER_BUFFER_SIZE * 2) as u64,
            is_dir: false,
        }]);
        let mut output = CancellingWriter {
            bytes: Vec::new(),
            progress: progress.clone(),
        };
        let error = copy_with_progress(
            &mut Cursor::new(vec![1; UPLOAD_TRANSFER_BUFFER_SIZE * 2]),
            &mut output,
            &progress,
            0,
            UPLOAD_TRANSFER_BUFFER_SIZE,
        )
        .unwrap_err();

        assert!(error.to_string().contains("已取消"));
        assert_eq!(output.bytes.len(), UPLOAD_TRANSFER_BUFFER_SIZE);
        assert_eq!(
            progress.snapshot().transferred_bytes,
            UPLOAD_TRANSFER_BUFFER_SIZE as u64
        );
    }

    #[test]
    #[ignore = "requires docker-compose.test.yml services"]
    fn uploads_lists_hidden_files_and_downloads_with_progress() {
        let id = format!("s-porter-transfer-{}", uuid::Uuid::new_v4());
        let source = std::env::temp_dir().join(&id);
        let target = std::env::temp_dir().join(format!("{id}-download"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join(".hidden"), b"hidden-content").unwrap();
        fs::write(source.join("visible.txt"), b"visible-content").unwrap();
        fs::write(
            source.join("large.bin"),
            vec![7_u8; INTEGRATION_TRANSFER_SIZE],
        )
        .unwrap();
        let host = JumpHost {
            id: "docker-sftp".into(),
            name: "Docker SFTP".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            password: "tester123".into(),
            root_username: "root".into(),
            root_password: "root123".into(),
            http_proxy: Some(HttpProxyConfig {
                host: "127.0.0.1".into(),
                port: 8888,
                username: "proxyuser".into(),
                password: "proxypass".into(),
            }),
        };

        let session = connect(&host).unwrap();
        assert_eq!(
            session.methods(MethodType::CryptCs),
            Some("aes128-gcm@openssh.com")
        );
        assert_eq!(
            session.methods(MethodType::CryptSc),
            Some("aes128-gcm@openssh.com")
        );
        drop(session);

        let upload_progress = TransferProgress::default();
        assert_eq!(
            upload(
                &host,
                "/tmp",
                std::slice::from_ref(&source),
                &upload_progress
            )
            .unwrap(),
            3
        );
        let remote_dir = format!("/tmp/{id}");
        let (_, entries) = list_directory(&host, &remote_dir).unwrap();
        assert_eq!(entries.first().map(|entry| entry.name.as_str()), Some("."));
        assert_eq!(entries.get(1).map(|entry| entry.name.as_str()), Some(".."));
        assert!(entries.iter().any(|entry| entry.name == ".hidden"));

        let download_progress = TransferProgress::default();
        assert_eq!(
            download(&host, &remote_dir, true, &target, &download_progress).unwrap(),
            3
        );
        assert_eq!(fs::read(target.join(".hidden")).unwrap(), b"hidden-content");
        assert_eq!(
            download_progress.snapshot().transferred_bytes,
            (b"hidden-content".len() + b"visible-content".len() + INTEGRATION_TRANSFER_SIZE) as u64
        );
        delete_entry(&host, &remote_dir, true).unwrap();
        let (_, tmp_entries) = list_directory(&host, "/tmp").unwrap();
        assert!(!tmp_entries.iter().any(|entry| entry.name == id));

        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(target).unwrap();
    }
}
