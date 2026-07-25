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
const MAX_OUTPUT_BYTES: usize = 512 * 1_024;
const TERMINAL_ROWS: u16 = 40;
const TERMINAL_COLS: u16 = 120;
const TERMINAL_SCROLLBACK: usize = MAX_OUTPUT_LINES - TERMINAL_ROWS as usize;

struct TerminalOutputBuffer {
    parser: vt100::Parser,
    revision: u64,
}

impl Default for TerminalOutputBuffer {
    fn default() -> Self {
        Self {
            parser: vt100::Parser::new(TERMINAL_ROWS, TERMINAL_COLS, TERMINAL_SCROLLBACK),
            revision: 0,
        }
    }
}

impl TerminalOutputBuffer {
    fn append(&mut self, value: &[u8]) {
        if value.is_empty() {
            return;
        }
        self.parser.process(value);
        self.revision = self.revision.wrapping_add(1);
    }

    fn snapshot(&self) -> String {
        let current = self.parser.screen();
        let (_, cols) = current.size();
        let mut screen = current.clone();
        screen.set_scrollback(usize::MAX);
        let mut scrollback = screen.scrollback();
        let mut lines = Vec::with_capacity(scrollback + usize::from(TERMINAL_ROWS));

        while scrollback > 0 {
            screen.set_scrollback(scrollback);
            let take = scrollback.min(usize::from(TERMINAL_ROWS));
            lines.extend(screen.rows(0, cols).take(take));
            scrollback -= take;
        }
        lines.extend(current.rows(0, cols));
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }

        let mut contents = lines.join("\n");
        if contents.len() > MAX_OUTPUT_BYTES {
            let mut boundary = contents.len() - MAX_OUTPUT_BYTES;
            while !contents.is_char_boundary(boundary) {
                boundary += 1;
            }
            contents.drain(..boundary);
        }
        contents
    }

    fn clear(&mut self) {
        self.parser = vt100::Parser::new(TERMINAL_ROWS, TERMINAL_COLS, TERMINAL_SCROLLBACK);
        self.revision = self.revision.wrapping_add(1);
    }
}

pub struct SshTerminalHandle {
    input: Sender<Vec<u8>>,
    output: Arc<Mutex<TerminalOutputBuffer>>,
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
        let output = Arc::new(Mutex::new(TerminalOutputBuffer::default()));
        let running = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_output = output.clone();
        let thread_running = running.clone();
        let thread_stop = stop.clone();

        let worker = thread::spawn(move || {
            append_output(
                &thread_output,
                format!(
                    "已连接到 {}@{}:{}\r\n",
                    jump_host.username, jump_host.host, jump_host.port
                )
                .as_bytes(),
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
                            append_output(&thread_output, b"\r\nSSH channel closed.\r\n");
                            thread_stop.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(written) => {
                            pending.drain(..written);
                            progressed = true;
                        }
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(error) => {
                            append_output(
                                &thread_output,
                                format!("\r\nWrite failed: {error}\r\n").as_bytes(),
                            );
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
                        append_output(&thread_output, &buffer[..read]);
                        progressed = true;
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                    Err(error) => {
                        append_output(
                            &thread_output,
                            format!("\r\nRead failed: {error}\r\n").as_bytes(),
                        );
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
            append_output(&thread_output, b"\r\nConnection closed.\r\n");
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

    pub fn output_if_changed(&self, revision: u64) -> Option<(u64, String)> {
        let output = self.output.lock().ok()?;
        (output.revision != revision).then(|| (output.revision, output.snapshot()))
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

fn append_output(output: &Arc<Mutex<TerminalOutputBuffer>>, value: &[u8]) {
    if let Ok(mut output) = output.lock() {
        output.append(value);
    }
}

#[cfg(test)]
fn sanitize_terminal_output(bytes: &[u8]) -> String {
    let mut output = TerminalOutputBuffer::default();
    output.append(bytes);
    output.snapshot()
}

#[cfg(test)]
mod tests {
    use super::{MAX_OUTPUT_BYTES, TerminalOutputBuffer, append_output, sanitize_terminal_output};
    use crate::forward::{HttpProxyConfig, JumpHost, SshTerminalHandle};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn strips_ansi_sequences_and_carriage_returns() {
        assert_eq!(
            sanitize_terminal_output(b"\x1b[32mready\x1b[0m\r\n"),
            "ready"
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
        let mut output = TerminalOutputBuffer::default();
        output.append(b"\x1b]0;tester");
        assert_eq!(output.snapshot(), "");
        output.append(b"@server\x07ready\r\n");
        assert_eq!(output.snapshot(), "ready");
    }

    #[test]
    fn keeps_only_the_latest_thousand_lines() {
        let output = Arc::new(Mutex::new(TerminalOutputBuffer::default()));
        let value = (0..1_005)
            .map(|line| format!("line-{line}\r\n"))
            .collect::<String>();
        append_output(&output, value.as_bytes());
        let output = output.lock().unwrap();
        let snapshot = output.snapshot();
        assert!((999..=1_000).contains(&snapshot.lines().count()));
        assert!(!snapshot.contains("line-0\n"));
        assert!(snapshot.ends_with("line-1004"));
    }

    #[test]
    fn large_continuous_output_stays_bounded_and_keeps_the_tail() {
        let mut output = TerminalOutputBuffer::default();
        for batch in 0..200 {
            let chunk = (0..1_000)
                .map(|line| format!("batch-{batch:03}-line-{line:04} service output\n"))
                .collect::<String>();
            output.append(chunk.replace('\n', "\r\n").as_bytes());
        }

        let snapshot = output.snapshot();
        assert!((999..=1_000).contains(&snapshot.lines().count()));
        assert!(snapshot.len() <= MAX_OUTPUT_BYTES);
        assert!(snapshot.contains("batch-199-line-0999 service output"));
        assert!(!snapshot.contains("batch-000-line-0000 service output"));
    }

    #[test]
    fn terminal_control_sequences_replace_the_visible_screen() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"old heading\r\nold row");
        output.append(b"\x1b[2J\x1b[Htop heading\r\nfresh row");

        let snapshot = output.snapshot();
        assert_eq!(snapshot, "top heading\nfresh row");
        assert!(!snapshot.contains("old"));
    }

    #[test]
    fn carriage_return_overwrites_the_current_line() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"progress 10%\rprogress 90%");

        assert_eq!(output.snapshot(), "progress 90%");
    }

    #[test]
    fn huge_line_without_newlines_is_bounded_on_utf8_boundary() {
        let mut output = TerminalOutputBuffer::default();
        output.append("日志".repeat(MAX_OUTPUT_BYTES).as_bytes());

        let snapshot = output.snapshot();
        assert!(snapshot.len() <= MAX_OUTPUT_BYTES);
        assert!(std::str::from_utf8(snapshot.as_bytes()).is_ok());
    }

    #[test]
    #[ignore = "requires docker-compose.test.yml services"]
    fn streams_large_output_from_local_docker_ssh_without_unbounded_growth() {
        let host = JumpHost {
            id: "docker-ssh-output".into(),
            name: "Docker SSH output stress".into(),
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
        let mut terminal = SshTerminalHandle::start(host).unwrap();
        terminal
            .send_line("seq 1 200000; echo __S_PORTER_STRESS_DONE__")
            .unwrap();

        let started = Instant::now();
        let mut revision = 0;
        let snapshot = loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if output.contains("__S_PORTER_STRESS_DONE__") {
                    break output;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "等待大量 SSH 输出完成超时"
            );
            std::thread::sleep(Duration::from_millis(20));
        };

        assert!(snapshot.lines().count() <= 1_000);
        assert!(snapshot.len() <= MAX_OUTPUT_BYTES);
        assert!(snapshot.contains("__S_PORTER_STRESS_DONE__"));
        terminal.stop();
    }
}
