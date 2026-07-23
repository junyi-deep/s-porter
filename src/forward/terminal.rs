use super::{JumpHost, ssh::connect};
use anyhow::{Context, Result, bail};
use std::{
    io::{ErrorKind, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_OUTPUT_LINES: usize = 1_000;

#[derive(Default)]
struct TerminalSanitizer {
    state: EscapeState,
}

#[derive(Default)]
enum EscapeState {
    #[default]
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

impl TerminalSanitizer {
    fn push(&mut self, bytes: &[u8]) -> String {
        let mut clean = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            match self.state {
                EscapeState::Text => match byte {
                    b'\x1b' => self.state = EscapeState::Escape,
                    b'\r' => {}
                    _ => clean.push(byte),
                },
                EscapeState::Escape => {
                    self.state = match byte {
                        b'[' => EscapeState::Csi,
                        b']' => EscapeState::Osc,
                        _ => EscapeState::Text,
                    };
                }
                EscapeState::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = EscapeState::Text;
                    }
                }
                EscapeState::Osc => match byte {
                    b'\x07' => self.state = EscapeState::Text,
                    b'\x1b' => self.state = EscapeState::OscEscape,
                    _ => {}
                },
                EscapeState::OscEscape => {
                    self.state = if byte == b'\\' {
                        EscapeState::Text
                    } else if byte == b'\x1b' {
                        EscapeState::OscEscape
                    } else {
                        EscapeState::Osc
                    };
                }
            }
        }
        String::from_utf8_lossy(&clean).into_owned()
    }
}

pub struct SshTerminalHandle {
    input: Sender<Vec<u8>>,
    output: Arc<Mutex<String>>,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SshTerminalHandle {
    pub fn start(jump_host: JumpHost) -> Result<Self> {
        jump_host.validate()?;
        let session = connect(&jump_host)?;
        let mut channel = session.channel_session().context("创建 SSH 终端失败")?;
        channel
            .request_pty("xterm-256color", None, Some((120, 40, 0, 0)))
            .context("申请远端伪终端失败")?;
        channel.shell().context("打开远端 shell 失败")?;
        session.set_blocking(false);

        let (input, receiver) = mpsc::channel::<Vec<u8>>();
        let output = Arc::new(Mutex::new(String::new()));
        let running = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_output = output.clone();
        let thread_running = running.clone();
        let thread_stop = stop.clone();

        let worker = thread::spawn(move || {
            let mut sanitizer = TerminalSanitizer::default();
            append_output(
                &thread_output,
                format!(
                    "已连接到 {}@{}:{}\n",
                    jump_host.username, jump_host.host, jump_host.port
                ),
            );
            let mut pending = Vec::<u8>::new();
            let mut buffer = [0_u8; 16 * 1024];
            while !thread_stop.load(Ordering::Relaxed) {
                loop {
                    match receiver.try_recv() {
                        Ok(bytes) => pending.extend(bytes),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            thread_stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }

                let mut progressed = false;
                while !pending.is_empty() {
                    match channel.write(&pending) {
                        Ok(0) => {
                            append_output(&thread_output, "\nSSH 通道已关闭。\n".into());
                            thread_stop.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(written) => {
                            pending.drain(..written);
                            progressed = true;
                        }
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(error) => {
                            append_output(&thread_output, format!("\n写入失败：{error}\n"));
                            thread_stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                if progressed {
                    channel.flush().ok();
                }

                match channel.read(&mut buffer) {
                    Ok(0) if channel.eof() => break,
                    Ok(0) => {}
                    Ok(read) => {
                        append_output(&thread_output, sanitizer.push(&buffer[..read]));
                        progressed = true;
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                    Err(error) => {
                        append_output(&thread_output, format!("\n读取失败：{error}\n"));
                        break;
                    }
                }
                if !progressed {
                    thread::sleep(Duration::from_millis(15));
                }
            }
            channel.send_eof().ok();
            channel.close().ok();
            thread_running.store(false, Ordering::Relaxed);
            append_output(&thread_output, "\n连接已断开。\n".into());
        });

        Ok(Self {
            input,
            output,
            running,
            stop,
            worker: Some(worker),
        })
    }

    pub fn send_line(&self, command: &str) -> Result<()> {
        if !self.is_running() {
            bail!("SSH 连接已断开");
        }
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\n');
        self.input.send(bytes).context("SSH 输入通道已关闭")
    }

    pub fn output(&self) -> String {
        self.output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_else(|_| "终端输出读取失败".into())
    }

    pub fn clear_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            output.clear();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for SshTerminalHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn append_output(output: &Arc<Mutex<String>>, value: String) {
    if let Ok(mut output) = output.lock() {
        output.push_str(&value);
        let line_count = output.lines().count();
        if line_count > MAX_OUTPUT_LINES {
            let lines_to_remove = line_count - MAX_OUTPUT_LINES;
            let boundary = output
                .match_indices('\n')
                .nth(lines_to_remove.saturating_sub(1))
                .map(|(index, _)| index + 1)
                .unwrap_or(0);
            output.drain(..boundary);
        }
    }
}

#[cfg(test)]
fn sanitize_terminal_output(bytes: &[u8]) -> String {
    TerminalSanitizer::default().push(bytes)
}

#[cfg(test)]
mod tests {
    use super::{append_output, sanitize_terminal_output};
    use std::sync::{Arc, Mutex};

    #[test]
    fn strips_ansi_sequences_and_carriage_returns() {
        assert_eq!(
            sanitize_terminal_output(b"\x1b[32mready\x1b[0m\r\n"),
            "ready\n"
        );
    }

    #[test]
    fn strips_osc_window_title_sequences() {
        assert_eq!(
            sanitize_terminal_output(b"\x1b]0;tester@server: ~\x07tester@server:~$ "),
            "tester@server:~$ "
        );
        assert_eq!(
            sanitize_terminal_output(b"\x1b]2;title\x1b\\ready"),
            "ready"
        );
    }

    #[test]
    fn strips_osc_sequences_split_across_reads() {
        let mut sanitizer = super::TerminalSanitizer::default();
        assert_eq!(sanitizer.push(b"\x1b]0;tester"), "");
        assert_eq!(sanitizer.push(b"@server\x07ready\n"), "ready\n");
    }

    #[test]
    fn keeps_only_the_latest_thousand_lines() {
        let output = Arc::new(Mutex::new(String::new()));
        append_output(
            &output,
            (0..1_005)
                .map(|line| format!("line-{line}\n"))
                .collect::<String>(),
        );
        let output = output.lock().unwrap();
        assert_eq!(output.lines().count(), 1_000);
        assert!(output.starts_with("line-5\n"));
        assert!(output.ends_with("line-1004\n"));
    }
}
