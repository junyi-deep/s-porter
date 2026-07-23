use super::{JumpHost, ssh::connect};
use anyhow::{Context, Result, bail};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_at: Option<u64>,
    pub permissions: Option<u32>,
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

pub fn upload(jump_host: &JumpHost, remote_dir: &str, local_paths: &[PathBuf]) -> Result<usize> {
    anyhow::ensure!(!local_paths.is_empty(), "没有选择要上传的文件");
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let mut count = 0;
    for local_path in local_paths {
        let name = local_path
            .file_name()
            .context("无法获取本地文件名")?
            .to_string_lossy();
        upload_path(
            &sftp,
            local_path,
            &remote_join(remote_dir, &name),
            &mut count,
        )?;
    }
    Ok(count)
}

pub fn download(
    jump_host: &JumpHost,
    remote_path: &str,
    is_dir: bool,
    local_path: &Path,
) -> Result<usize> {
    let session = connect(jump_host)?;
    let sftp = session.sftp().context("初始化 SFTP 失败")?;
    let mut count = 0;
    download_path(&sftp, remote_path, is_dir, local_path, &mut count)?;
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

fn upload_path(
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_path: &str,
    count: &mut usize,
) -> Result<()> {
    if local_path.is_dir() {
        match sftp.mkdir(Path::new(remote_path), 0o755) {
            Ok(()) => {}
            Err(_) if sftp.stat(Path::new(remote_path)).is_ok() => {}
            Err(error) => {
                return Err(error).with_context(|| format!("创建远程目录失败：{remote_path}"));
            }
        }
        for child in fs::read_dir(local_path)
            .with_context(|| format!("读取本地目录失败：{}", local_path.display()))?
        {
            let child = child?;
            let name = child.file_name().to_string_lossy().to_string();
            upload_path(sftp, &child.path(), &remote_join(remote_path, &name), count)?;
        }
        return Ok(());
    }
    if !local_path.is_file() {
        bail!("不支持上传该类型：{}", local_path.display());
    }
    let mut local = fs::File::open(local_path)
        .with_context(|| format!("打开本地文件失败：{}", local_path.display()))?;
    let mut remote = sftp
        .create(Path::new(remote_path))
        .with_context(|| format!("创建远程文件失败：{remote_path}"))?;
    io::copy(&mut local, &mut remote)
        .with_context(|| format!("上传文件失败：{}", local_path.display()))?;
    *count += 1;
    Ok(())
}

fn download_path(
    sftp: &ssh2::Sftp,
    remote_path: &str,
    is_dir: bool,
    local_path: &Path,
    count: &mut usize,
) -> Result<()> {
    if is_dir {
        fs::create_dir_all(local_path)
            .with_context(|| format!("创建本地目录失败：{}", local_path.display()))?;
        for (child, stat) in sftp
            .readdir(Path::new(remote_path))
            .with_context(|| format!("读取远程目录失败：{remote_path}"))?
        {
            let Some(name) = child.file_name() else {
                continue;
            };
            let name = name.to_string_lossy();
            download_path(
                sftp,
                &remote_join(remote_path, &name),
                stat.is_dir(),
                &local_path.join(name.as_ref()),
                count,
            )?;
        }
        return Ok(());
    }
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut remote = sftp
        .open(Path::new(remote_path))
        .with_context(|| format!("打开远程文件失败：{remote_path}"))?;
    let mut local = fs::File::create(local_path)
        .with_context(|| format!("创建本地文件失败：{}", local_path.display()))?;
    io::copy(&mut remote, &mut local).with_context(|| format!("下载文件失败：{remote_path}"))?;
    *count += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
