use super::{JumpHost, ssh::connect};
use anyhow::{Context, Result, bail};
use std::{
    io::{ErrorKind, Read, Write},
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[cfg(test)]
use unicode_width::UnicodeWidthChar as _;

const MAX_OUTPUT_LINES: usize = 1_000;
const MAX_OUTPUT_BYTES: usize = 512 * 1_024;
const TERMINAL_ROWS: u16 = 40;
const TERMINAL_COLS: u16 = 120;
const TERMINAL_SCROLLBACK: usize = MAX_OUTPUT_LINES - TERMINAL_ROWS as usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalTextStyle {
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalStyleSpan {
    pub range: Range<usize>,
    pub style: TerminalTextStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalLine {
    pub text: String,
    pub styles: Vec<TerminalStyleSpan>,
}

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

    #[cfg(test)]
    fn snapshot(&self) -> String {
        self.snapshot_impl(false)
    }

    #[cfg(test)]
    fn display_snapshot(&self) -> String {
        self.snapshot_impl(true)
    }

    #[cfg(test)]
    fn snapshot_impl(&self, show_cursor: bool) -> String {
        let current = self.parser.screen();
        let (rows, cols) = current.size();
        let mut screen = current.clone();
        screen.set_scrollback(usize::MAX);
        let mut scrollback = screen.scrollback();
        let mut lines = Vec::with_capacity(scrollback + usize::from(TERMINAL_ROWS));

        while scrollback > 0 {
            screen.set_scrollback(scrollback);
            let take = scrollback.min(usize::from(rows));
            lines.extend(screen.rows(0, cols).take(take));
            scrollback -= take;
        }
        let mut current_lines = current.rows(0, cols).collect::<Vec<_>>();
        if show_cursor && !current.hide_cursor() {
            let (cursor_row, cursor_col) = current.cursor_position();
            if let Some(line) = current_lines.get_mut(usize::from(cursor_row)) {
                insert_cursor(line, usize::from(cursor_col));
            }
        }
        lines.extend(current_lines);
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if lines.len() > MAX_OUTPUT_LINES {
            lines.drain(..lines.len() - MAX_OUTPUT_LINES);
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

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.revision = self.revision.wrapping_add(1);
    }
}

fn terminal_color(color: vt100::Color) -> Option<[u8; 3]> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Rgb(red, green, blue) => Some([red, green, blue]),
        vt100::Color::Idx(index) => Some(xterm_color(index)),
    }
}

fn xterm_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            let component = |part: u8| if part == 0 { 0 } else { 55 + part * 40 };
            [
                component(value / 36),
                component((value % 36) / 6),
                component(value % 6),
            ]
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            [value, value, value]
        }
    }
}

fn cell_style(cell: &vt100::Cell) -> TerminalTextStyle {
    TerminalTextStyle {
        foreground: terminal_color(cell.fgcolor()),
        background: terminal_color(cell.bgcolor()),
        bold: cell.bold(),
        dim: cell.dim(),
        italic: cell.italic(),
        underline: cell.underline(),
        inverse: cell.inverse(),
    }
}

fn styled_row(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    cursor_col: Option<u16>,
) -> TerminalLine {
    let last_column = (0..cols).rev().find(|column| {
        cursor_col == Some(*column)
            || screen.cell(row, *column).is_some_and(|cell| {
                cell.has_contents() || cell.bgcolor() != vt100::Color::Default || cell.underline()
            })
    });
    let Some(last_column) = last_column else {
        return TerminalLine {
            text: " ".into(),
            styles: Vec::new(),
        };
    };

    let mut line = TerminalLine::default();
    let mut current_style = None::<(usize, TerminalTextStyle)>;
    for column in 0..=last_column {
        if cursor_col == Some(column) {
            if let Some((start, style)) = current_style.take()
                && style != TerminalTextStyle::default()
            {
                line.styles.push(TerminalStyleSpan {
                    range: start..line.text.len(),
                    style,
                });
            }
            let start = line.text.len();
            line.text.push('▏');
            line.styles.push(TerminalStyleSpan {
                range: start..line.text.len(),
                style: TerminalTextStyle {
                    foreground: Some([17, 24, 39]),
                    ..TerminalTextStyle::default()
                },
            });
        }
        let Some(cell) = screen.cell(row, column) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let style = cell_style(cell);
        if current_style
            .as_ref()
            .is_some_and(|(_, current)| *current != style)
        {
            let (start, current) = current_style.take().expect("style exists");
            if current != TerminalTextStyle::default() {
                line.styles.push(TerminalStyleSpan {
                    range: start..line.text.len(),
                    style: current,
                });
            }
        }
        if current_style.is_none() {
            current_style = Some((line.text.len(), style));
        }
        if cell.has_contents() {
            line.text.push_str(cell.contents());
        } else {
            line.text.push(' ');
        }
    }
    if let Some((start, style)) = current_style
        && style != TerminalTextStyle::default()
    {
        line.styles.push(TerminalStyleSpan {
            range: start..line.text.len(),
            style,
        });
    }
    line
}

fn styled_screen_snapshot(current: &vt100::Screen, show_cursor: bool) -> Vec<TerminalLine> {
    let (rows, cols) = current.size();
    let mut screen = current.clone();
    screen.set_scrollback(usize::MAX);
    let mut scrollback = screen.scrollback();
    let mut lines = Vec::with_capacity(scrollback + usize::from(rows));

    while scrollback > 0 {
        screen.set_scrollback(scrollback);
        let take = scrollback.min(usize::from(rows));
        lines.extend(
            (0..u16::try_from(take).unwrap_or(rows))
                .map(|row| styled_row(&screen, row, cols, None)),
        );
        scrollback -= take;
    }
    let cursor = show_cursor
        .then(|| current.cursor_position())
        .filter(|_| !current.hide_cursor());
    lines.extend((0..rows).map(|row| {
        styled_row(
            current,
            row,
            cols,
            cursor
                .filter(|(cursor_row, _)| *cursor_row == row)
                .map(|(_, cursor_col)| cursor_col),
        )
    }));
    while lines.len() > 1
        && lines
            .last()
            .is_some_and(|line| line.styles.is_empty() && line.text.trim().is_empty())
    {
        lines.pop();
    }
    if lines.len() > MAX_OUTPUT_LINES {
        lines.drain(..lines.len() - MAX_OUTPUT_LINES);
    }
    let mut bytes = lines.iter().map(|line| line.text.len()).sum::<usize>();
    while lines.len() > 1 && bytes > MAX_OUTPUT_BYTES {
        bytes = bytes.saturating_sub(lines[0].text.len());
        lines.remove(0);
    }
    lines
}

#[cfg(test)]
fn insert_cursor(line: &mut String, column: usize) {
    let mut display_column = 0;
    let mut byte_index = line.len();
    for (index, character) in line.char_indices() {
        let width = character.width().unwrap_or(0);
        if display_column + width > column || display_column == column {
            byte_index = index;
            break;
        }
        display_column += width;
    }
    if display_column < column && byte_index == line.len() {
        line.extend(std::iter::repeat_n(' ', column - display_column));
        byte_index = line.len();
    }
    line.insert(byte_index, '▏');
}

enum TerminalInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

#[derive(Clone)]
pub struct SshTerminalControl {
    input: Sender<TerminalInput>,
    running: Arc<AtomicBool>,
}

impl SshTerminalControl {
    pub fn resize(&self, cols: u16, rows: u16) {
        if self.running.load(Ordering::Relaxed) {
            let _ = self.input.send(TerminalInput::Resize {
                cols: cols.max(20),
                rows: rows.max(5),
            });
        }
    }
}

pub struct SshTerminalHandle {
    input: Sender<TerminalInput>,
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

        let (input, receiver) = mpsc::channel::<TerminalInput>();
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
                let mut progressed = false;
                loop {
                    match receiver.try_recv() {
                        Ok(TerminalInput::Data(bytes)) => pending.extend(bytes),
                        Ok(TerminalInput::Resize { cols, rows }) => {
                            if channel
                                .request_pty_size(u32::from(cols), u32::from(rows), None, None)
                                .is_ok()
                                && let Ok(mut output) = thread_output.lock()
                            {
                                output.resize(rows, cols);
                            }
                            progressed = true;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            thread_stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
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
        self.input
            .send(TerminalInput::Data(bytes))
            .context("SSH 输入通道已关闭")
    }

    pub fn send_bytes(&self, bytes: Vec<u8>) -> Result<()> {
        if !self.is_running() {
            bail!("SSH 连接已断开");
        }
        self.input
            .send(TerminalInput::Data(bytes))
            .context("SSH 输入通道已关闭")
    }

    pub fn application_cursor(&self) -> bool {
        self.output
            .lock()
            .map(|output| output.parser.screen().application_cursor())
            .unwrap_or(false)
    }

    pub fn send_paste(&self, text: &str) -> Result<()> {
        let bracketed = self
            .output
            .lock()
            .map(|output| output.parser.screen().bracketed_paste())
            .unwrap_or(false);
        let text = text.replace('\x1b', "");
        let bytes = if bracketed {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        };
        self.send_bytes(bytes)
    }

    pub fn control(&self) -> SshTerminalControl {
        SshTerminalControl {
            input: self.input.clone(),
            running: self.running.clone(),
        }
    }

    pub fn output_if_changed(&self, revision: u64) -> Option<(u64, Vec<TerminalLine>)> {
        let (revision, screen) = {
            let output = self.output.lock().ok()?;
            if output.revision == revision {
                return None;
            }
            (output.revision, output.parser.screen().clone())
        };
        Some((revision, styled_screen_snapshot(&screen, true)))
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
    use super::{
        MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES, TerminalOutputBuffer, append_output,
        sanitize_terminal_output,
    };
    use crate::forward::{HttpProxyConfig, JumpHost, SshTerminalHandle};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn styled_snapshot_text(lines: &[super::TerminalLine]) -> String {
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn strips_ansi_sequences_and_carriage_returns() {
        assert_eq!(
            sanitize_terminal_output(b"\x1b[32mready\x1b[0m\r\n"),
            "ready"
        );
    }

    #[test]
    fn styled_snapshot_keeps_terminal_colors_and_attributes() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"\x1b[1;32;44mready\x1b[0m");

        let lines = super::styled_screen_snapshot(output.parser.screen(), false);
        assert_eq!(lines[0].text, "ready");
        assert_eq!(lines[0].styles.len(), 1);
        let style = lines[0].styles[0].style;
        assert_eq!(style.foreground, Some([13, 188, 121]));
        assert_eq!(style.background, Some([36, 114, 200]));
        assert!(style.bold);
    }

    #[test]
    fn styled_cursor_is_a_thin_glyph_without_a_block_background() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"ready");

        let lines = super::styled_screen_snapshot(output.parser.screen(), true);
        let cursor_span = lines[0]
            .styles
            .iter()
            .find(|span| lines[0].text[span.range.clone()].contains('▏'))
            .expect("cursor style");
        assert_eq!(cursor_span.style.background, None);
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
    fn display_snapshot_marks_the_terminal_cursor() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"vim");

        assert_eq!(output.display_snapshot(), "vim▏");
        assert_eq!(output.snapshot(), "vim");
    }

    #[test]
    fn terminal_resize_updates_screen_dimensions_and_keeps_output_bounded() {
        let mut output = TerminalOutputBuffer::default();
        output.resize(80, 160);
        output.append(
            (0..1_200)
                .map(|line| format!("line-{line}\r\n"))
                .collect::<String>()
                .as_bytes(),
        );

        assert_eq!(output.parser.screen().size(), (80, 160));
        assert!(output.snapshot().lines().count() <= MAX_OUTPUT_LINES);
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
                if styled_snapshot_text(&output).contains("__S_PORTER_STRESS_DONE__") {
                    break output;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "等待大量 SSH 输出完成超时"
            );
            std::thread::sleep(Duration::from_millis(20));
        };

        let snapshot_text = styled_snapshot_text(&snapshot);
        assert!(snapshot.len() <= 1_000);
        assert!(snapshot_text.len() <= MAX_OUTPUT_BYTES);
        assert!(snapshot_text.contains("__S_PORTER_STRESS_DONE__"));
        terminal.stop();
    }

    #[test]
    #[ignore = "requires docker-compose.test.yml services"]
    fn ctrl_c_interrupts_a_running_remote_command() {
        let host = JumpHost {
            id: "docker-ssh-control".into(),
            name: "Docker SSH control input".into(),
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
            .send_line("echo __S_PORTER_SLEEP_STARTED__; sleep 30")
            .unwrap();

        let started = Instant::now();
        let mut revision = 0;
        loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if styled_snapshot_text(&output).contains("__S_PORTER_SLEEP_STARTED__") {
                    break;
                }
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(20));
        }

        terminal.send_bytes(vec![0x03]).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        terminal
            .send_line("echo __S_PORTER_AFTER_CTRL_C__")
            .unwrap();
        let interrupted_at = Instant::now();
        loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if styled_snapshot_text(&output).contains("__S_PORTER_AFTER_CTRL_C__") {
                    break;
                }
            }
            assert!(
                interrupted_at.elapsed() < Duration::from_secs(5),
                "Ctrl+C 未及时中断远端命令"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        terminal.stop();
    }
}
