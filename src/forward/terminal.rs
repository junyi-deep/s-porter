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
    time::{Duration, Instant},
};
#[cfg(test)]
use unicode_width::UnicodeWidthChar as _;

pub const DEFAULT_TERMINAL_HISTORY_LINES: usize = 1_000;
pub const MIN_TERMINAL_HISTORY_LINES: usize = 100;
pub const MAX_TERMINAL_HISTORY_LINES: usize = 10_000;
const MAX_OUTPUT_LINES: usize = DEFAULT_TERMINAL_HISTORY_LINES;
const MAX_OUTPUT_BYTES: usize = 512 * 1_024;
const TERMINAL_ROWS: u16 = 40;
const TERMINAL_COLS: u16 = 120;
const TERMINAL_SCROLLBACK: usize = MAX_TERMINAL_HISTORY_LINES - TERMINAL_ROWS as usize;
const TERMINAL_READ_BUFFER_SIZE: usize = 64 * 1024;
const TERMINAL_MAX_READS_PER_TICK: usize = 32;
const TERMINAL_KEEPALIVE_INTERVAL_SECS: u32 = 15;
const SSH_ERROR_EAGAIN: i32 = -37;

fn is_retryable_io_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

fn is_retryable_ssh_error(error: &ssh2::Error) -> bool {
    error.code() == ssh2::ErrorCode::Session(SSH_ERROR_EAGAIN)
}

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
    pub cursor_column: Option<usize>,
}

#[derive(Default)]
struct TerminalModeState {
    application_cursor: AtomicBool,
    bracketed_paste: AtomicBool,
}

struct TerminalOutputBuffer {
    parser: vt100::Parser,
    revision: u64,
    modes: Arc<TerminalModeState>,
    history_limit: usize,
    primary_screen_before_alternate: Option<vt100::Screen>,
    primary_visible_before_clear: Option<Vec<TerminalLine>>,
    pending_alternate_sequence: Vec<u8>,
}

impl Default for TerminalOutputBuffer {
    fn default() -> Self {
        Self::with_modes(Arc::new(TerminalModeState::default()))
    }
}

impl TerminalOutputBuffer {
    fn with_modes(modes: Arc<TerminalModeState>) -> Self {
        Self {
            parser: vt100::Parser::new(TERMINAL_ROWS, TERMINAL_COLS, TERMINAL_SCROLLBACK),
            revision: 0,
            modes,
            history_limit: DEFAULT_TERMINAL_HISTORY_LINES,
            primary_screen_before_alternate: None,
            primary_visible_before_clear: None,
            pending_alternate_sequence: Vec::new(),
        }
    }

    fn append(&mut self, value: &[u8]) {
        if value.is_empty() {
            return;
        }
        self.process_with_alternate_screen_tracking(value);
        self.refresh_modes();
        self.revision = self.revision.wrapping_add(1);
    }

    fn process_with_alternate_screen_tracking(&mut self, value: &[u8]) {
        #[derive(Clone, Copy)]
        enum SequenceKind {
            EnterAlternate,
            ExitAlternate,
            ClearPrimary,
        }
        const TRACKED_SEQUENCES: [(&[u8], SequenceKind); 9] = [
            (b"\x1b[?47h", SequenceKind::EnterAlternate),
            (b"\x1b[?1047h", SequenceKind::EnterAlternate),
            (b"\x1b[?1049h", SequenceKind::EnterAlternate),
            (b"\x1b[?47l", SequenceKind::ExitAlternate),
            (b"\x1b[?1047l", SequenceKind::ExitAlternate),
            (b"\x1b[?1049l", SequenceKind::ExitAlternate),
            (b"\x1b[H\x1b[2J", SequenceKind::ClearPrimary),
            (b"\x1b[1;1H\x1b[2J", SequenceKind::ClearPrimary),
            (b"\x1b[2J", SequenceKind::ClearPrimary),
        ];

        let mut bytes = std::mem::take(&mut self.pending_alternate_sequence);
        bytes.extend_from_slice(value);

        // 控制序列可能被 SSH 分成两次读取。暂存仍可能组成备用屏幕切换序列的尾部，
        // 避免错过恰好跨读取边界的 `ESC[?1049h/l`。
        let max_pending_len = TRACKED_SEQUENCES
            .iter()
            .map(|(sequence, _)| sequence.len().saturating_sub(1))
            .max()
            .unwrap_or(0);
        let pending_len = (1..=bytes.len().min(max_pending_len))
            .rev()
            .find(|length| {
                let suffix = &bytes[bytes.len() - length..];
                TRACKED_SEQUENCES
                    .iter()
                    .any(|(sequence, _)| sequence.len() > *length && sequence.starts_with(suffix))
            })
            .unwrap_or(0);
        let process_len = bytes.len() - pending_len;
        if pending_len > 0 {
            self.pending_alternate_sequence
                .extend_from_slice(&bytes[process_len..]);
        }

        let bytes = &bytes[..process_len];
        let mut offset = 0;
        while offset < bytes.len() {
            let next = TRACKED_SEQUENCES
                .iter()
                .filter_map(|(sequence, kind)| {
                    bytes[offset..]
                        .windows(sequence.len())
                        .position(|window| window == *sequence)
                        .map(|position| (offset + position, *sequence, *kind))
                })
                .min_by_key(|(position, _, _)| *position);
            let Some((position, sequence, kind)) = next else {
                self.parser.process(&bytes[offset..]);
                break;
            };

            self.parser.process(&bytes[offset..position]);
            match kind {
                SequenceKind::EnterAlternate if !self.parser.screen().alternate_screen() => {
                    self.primary_screen_before_alternate = Some(self.parser.screen().clone());
                }
                SequenceKind::ClearPrimary
                    if !self.parser.screen().alternate_screen()
                        && self.primary_visible_before_clear.is_none() =>
                {
                    let visible = styled_visible_snapshot(self.parser.screen(), false);
                    if visible.iter().any(|line| !line.text.trim().is_empty()) {
                        self.primary_visible_before_clear = Some(visible);
                    }
                }
                _ => {}
            }
            self.parser.process(sequence);
            if matches!(kind, SequenceKind::ExitAlternate)
                && !self.parser.screen().alternate_screen()
            {
                self.primary_screen_before_alternate = None;
            }
            offset = position + sequence.len();
        }
    }

    fn refresh_modes(&self) {
        let screen = self.parser.screen();
        self.modes
            .application_cursor
            .store(screen.application_cursor(), Ordering::Relaxed);
        self.modes
            .bracketed_paste
            .store(screen.bracketed_paste(), Ordering::Relaxed);
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
        let mut lines = self.lines_snapshot(show_cursor);
        for line in &mut lines {
            if show_cursor && let Some(column) = line.cursor_column {
                while line.text.ends_with(' ') {
                    line.text.pop();
                }
                insert_cursor(&mut line.text, column);
            }
        }
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(test)]
    fn lines_snapshot(&self, show_cursor: bool) -> Vec<TerminalLine> {
        terminal_lines_snapshot(
            self.parser.screen().clone(),
            self.primary_screen_before_alternate.clone(),
            self.primary_visible_before_clear.clone(),
            show_cursor,
            self.history_limit,
        )
    }

    fn clear(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, TERMINAL_SCROLLBACK);
        self.primary_screen_before_alternate = None;
        self.primary_visible_before_clear = None;
        self.pending_alternate_sequence.clear();
        self.refresh_modes();
        self.revision = self.revision.wrapping_add(1);
    }

    fn set_history_limit(&mut self, lines: usize) {
        let lines = lines.clamp(MIN_TERMINAL_HISTORY_LINES, MAX_TERMINAL_HISTORY_LINES);
        if self.history_limit != lines {
            self.history_limit = lines;
            self.revision = self.revision.wrapping_add(1);
        }
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
            text: String::new(),
            styles: Vec::new(),
            cursor_column: None,
        };
    };

    let mut line = TerminalLine {
        cursor_column: cursor_col.map(usize::from),
        ..TerminalLine::default()
    };
    let mut current_style = None::<(usize, TerminalTextStyle)>;
    for column in 0..=last_column {
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

fn styled_screen_snapshot(
    current: vt100::Screen,
    show_cursor: bool,
    history_limit: usize,
) -> Vec<TerminalLine> {
    let mut lines = styled_scrollback_snapshot(current.clone(), history_limit);
    lines.extend(styled_visible_snapshot(&current, show_cursor));
    trim_terminal_lines(&mut lines, history_limit);
    lines
}

fn styled_visible_snapshot(current: &vt100::Screen, show_cursor: bool) -> Vec<TerminalLine> {
    let (rows, cols) = current.size();
    let cursor = show_cursor
        .then(|| current.cursor_position())
        .filter(|_| !current.hide_cursor());
    let mut lines = (0..rows)
        .map(|row| {
            styled_row(
                current,
                row,
                cols,
                cursor
                    .filter(|(cursor_row, _)| *cursor_row == row)
                    .map(|(_, cursor_col)| cursor_col),
            )
        })
        .collect::<Vec<_>>();
    trim_trailing_blank_lines(&mut lines);
    lines
}

fn styled_scrollback_snapshot(
    mut current: vt100::Screen,
    history_limit: usize,
) -> Vec<TerminalLine> {
    let (rows, cols) = current.size();
    current.set_scrollback(usize::MAX);
    let mut scrollback = current.scrollback().min(history_limit);
    let mut lines = Vec::with_capacity(scrollback + usize::from(rows));
    while scrollback > 0 {
        current.set_scrollback(scrollback);
        let take = scrollback.min(usize::from(rows));
        lines.extend(
            (0..u16::try_from(take).unwrap_or(rows))
                .map(|row| styled_row(&current, row, cols, None)),
        );
        scrollback -= take;
    }
    lines
}

fn trim_trailing_blank_lines(lines: &mut Vec<TerminalLine>) {
    while lines.len() > 1
        && lines
            .last()
            .is_some_and(|line| line.styles.is_empty() && line.text.trim().is_empty())
    {
        lines.pop();
    }
}

fn terminal_lines_snapshot(
    current: vt100::Screen,
    primary_screen_before_alternate: Option<vt100::Screen>,
    primary_visible_before_clear: Option<Vec<TerminalLine>>,
    show_cursor: bool,
    history_limit: usize,
) -> Vec<TerminalLine> {
    if current.alternate_screen()
        && let Some(primary) = primary_screen_before_alternate
    {
        let mut lines = styled_screen_snapshot(primary, false, history_limit);
        if let Some(archived) = primary_visible_before_clear {
            lines.extend(archived);
        }
        lines.extend(styled_screen_snapshot(current, show_cursor, history_limit));
        trim_terminal_lines(&mut lines, history_limit);
        return lines;
    }
    if let Some(archived) = primary_visible_before_clear {
        let mut lines = styled_scrollback_snapshot(current.clone(), history_limit);
        lines.extend(archived);
        lines.extend(styled_visible_snapshot(&current, show_cursor));
        trim_terminal_lines(&mut lines, history_limit);
        return lines;
    }
    styled_screen_snapshot(current, show_cursor, history_limit)
}

fn trim_terminal_lines(lines: &mut Vec<TerminalLine>, history_limit: usize) {
    let history_limit = history_limit.clamp(MIN_TERMINAL_HISTORY_LINES, MAX_TERMINAL_HISTORY_LINES);
    if lines.len() > history_limit {
        lines.drain(..lines.len() - history_limit);
    }
    let max_output_bytes = history_limit.saturating_mul(MAX_OUTPUT_BYTES / MAX_OUTPUT_LINES);
    let mut bytes = lines.iter().map(|line| line.text.len()).sum::<usize>();
    let mut remove_count = 0;
    while remove_count + 1 < lines.len() && bytes > max_output_bytes {
        bytes = bytes.saturating_sub(lines[remove_count].text.len());
        remove_count += 1;
    }
    if remove_count > 0 {
        lines.drain(..remove_count);
    }
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
    modes: Arc<TerminalModeState>,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SshTerminalHandle {
    #[cfg(test)]
    pub fn start(jump_host: JumpHost) -> Result<Self> {
        Self::start_with_history_limit(jump_host, DEFAULT_TERMINAL_HISTORY_LINES)
    }

    pub fn start_with_history_limit(jump_host: JumpHost, history_limit: usize) -> Result<Self> {
        jump_host.validate()?;
        let session = connect(&jump_host)?;
        let mut channel = session.channel_session().context("创建 SSH 终端失败")?;
        channel
            .request_pty("xterm-256color", None, Some((120, 40, 0, 0)))
            .context("申请远端伪终端失败")?;
        channel.shell().context("打开远端 shell 失败")?;
        session.set_keepalive(true, TERMINAL_KEEPALIVE_INTERVAL_SECS);
        session.set_blocking(false);

        let (input, receiver) = mpsc::channel::<TerminalInput>();
        let modes = Arc::new(TerminalModeState::default());
        let mut output_buffer = TerminalOutputBuffer::with_modes(modes.clone());
        output_buffer.set_history_limit(history_limit);
        let output = Arc::new(Mutex::new(output_buffer));
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
            let mut pending_resize = None::<(u16, u16)>;
            let mut resize_retrying = false;
            let mut keepalive_pending = false;
            let mut buffer = vec![0_u8; TERMINAL_READ_BUFFER_SIZE];
            let mut next_keepalive =
                Instant::now() + Duration::from_secs(TERMINAL_KEEPALIVE_INTERVAL_SECS.into());
            'terminal: while !thread_stop.load(Ordering::Relaxed) {
                let mut progressed = false;
                if !resize_retrying {
                    loop {
                        match receiver.try_recv() {
                            Ok(TerminalInput::Data(bytes)) => pending.extend(bytes),
                            Ok(TerminalInput::Resize { cols, rows }) => {
                                pending_resize = Some((cols, rows));
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                thread_stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                }
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }

                // libssh2 的非阻塞操作返回 EAGAIN 后必须先重试同一个操作。
                // 否则 resize/keepalive 与读写交错，可能让会话状态机进入 BAD_USE。
                if keepalive_pending {
                    match session.keepalive_send() {
                        Ok(seconds_to_next) => {
                            keepalive_pending = false;
                            next_keepalive =
                                Instant::now() + Duration::from_secs(seconds_to_next.max(1).into());
                            progressed = true;
                        }
                        Err(error) if is_retryable_ssh_error(&error) => {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => {
                            append_output(
                                &thread_output,
                                format!("\r\nSSH 保活失败：{error}\r\n").as_bytes(),
                            );
                            break;
                        }
                    }
                }

                if let Some((cols, rows)) = pending_resize {
                    match channel.request_pty_size(u32::from(cols), u32::from(rows), None, None) {
                        Ok(()) => {
                            pending_resize = None;
                            resize_retrying = false;
                            if let Ok(mut output) = thread_output.lock() {
                                output.resize(rows, cols);
                            }
                            progressed = true;
                        }
                        Err(error) if is_retryable_ssh_error(&error) => {
                            resize_retrying = true;
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => {
                            pending_resize = None;
                            resize_retrying = false;
                            append_output(
                                &thread_output,
                                format!("\r\nSSH 终端尺寸调整失败：{error}\r\n").as_bytes(),
                            );
                        }
                    }
                }

                while !pending.is_empty() {
                    match channel.write(&pending) {
                        Ok(0) if channel.eof() => {
                            append_output(&thread_output, b"\r\nSSH channel closed.\r\n");
                            break 'terminal;
                        }
                        // ssh2 的非阻塞 Channel::write 在发送窗口暂时不可用时会返回
                        // Ok(0)，这不是 EOF，保留数据到下一轮继续发送。
                        Ok(0) => break,
                        Ok(written) => {
                            pending.drain(..written);
                            progressed = true;
                        }
                        Err(error) if is_retryable_io_error(&error) => break,
                        Err(error) => {
                            append_output(
                                &thread_output,
                                format!("\r\nWrite failed: {error}\r\n").as_bytes(),
                            );
                            break 'terminal;
                        }
                    }
                }

                for _ in 0..TERMINAL_MAX_READS_PER_TICK {
                    match channel.read(&mut buffer) {
                        Ok(0) if channel.eof() => break 'terminal,
                        Ok(0) => break,
                        Ok(read) => {
                            append_output(&thread_output, &buffer[..read]);
                            progressed = true;
                        }
                        Err(error) if is_retryable_io_error(&error) => break,
                        Err(error) => {
                            append_output(
                                &thread_output,
                                format!("\r\nSSH 读取失败：{error}\r\n").as_bytes(),
                            );
                            break 'terminal;
                        }
                    }
                }

                if Instant::now() >= next_keepalive {
                    keepalive_pending = true;
                }
                if !progressed {
                    thread::sleep(Duration::from_millis(5));
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
            modes,
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
        self.modes.application_cursor.load(Ordering::Relaxed)
    }

    pub fn send_paste(&self, text: &str) -> Result<()> {
        let bracketed = self.modes.bracketed_paste.load(Ordering::Relaxed);
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
        let (revision, screen, primary_screen, primary_visible, history_limit) = {
            // 终端线程可能正在解析大块输出。UI 此帧跳过快照比等待互斥锁更平滑，
            // 下一次刷新仍会取得最新 revision，不会丢失任何内容。
            let output = self.output.try_lock().ok()?;
            if output.revision == revision {
                return None;
            }
            (
                output.revision,
                output.parser.screen().clone(),
                output.primary_screen_before_alternate.clone(),
                output.primary_visible_before_clear.clone(),
                output.history_limit,
            )
        };
        Some((
            revision,
            terminal_lines_snapshot(screen, primary_screen, primary_visible, true, history_limit),
        ))
    }

    pub fn clear_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            output.clear();
        }
    }

    pub fn set_history_limit(&self, lines: usize) {
        if let Ok(mut output) = self.output.lock() {
            output.set_history_limit(lines);
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
        is_retryable_io_error, is_retryable_ssh_error, sanitize_terminal_output,
    };
    use crate::forward::{HttpProxyConfig, JumpHost, SshTerminalHandle};
    use std::sync::{Arc, Mutex, atomic::Ordering};
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
    fn retries_nonblocking_and_temporary_terminal_io_errors() {
        for kind in [
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::Interrupted,
        ] {
            assert!(is_retryable_io_error(&std::io::Error::from(kind)));
        }
        assert!(!is_retryable_io_error(&std::io::Error::from(
            std::io::ErrorKind::ConnectionReset,
        )));
        assert!(is_retryable_ssh_error(&ssh2::Error::new(
            ssh2::ErrorCode::Session(super::SSH_ERROR_EAGAIN),
            "would block",
        )));
    }

    #[test]
    fn styled_snapshot_keeps_terminal_colors_and_attributes() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"\x1b[1;32;44mready\x1b[0m");

        let lines = super::styled_screen_snapshot(
            output.parser.screen().clone(),
            false,
            super::DEFAULT_TERMINAL_HISTORY_LINES,
        );
        assert_eq!(lines[0].text, "ready");
        assert_eq!(lines[0].styles.len(), 1);
        let style = lines[0].styles[0].style;
        assert_eq!(style.foreground, Some([13, 188, 121]));
        assert_eq!(style.background, Some([36, 114, 200]));
        assert!(style.bold);
    }

    #[test]
    fn terminal_modes_are_cached_for_nonblocking_keyboard_input() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"\x1b[?1h\x1b[?2004h");

        assert!(output.modes.application_cursor.load(Ordering::Relaxed));
        assert!(output.modes.bracketed_paste.load(Ordering::Relaxed));
    }

    #[test]
    fn styled_snapshot_keeps_visual_cursor_out_of_copyable_text() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"ready");

        let lines = super::styled_screen_snapshot(
            output.parser.screen().clone(),
            true,
            super::DEFAULT_TERMINAL_HISTORY_LINES,
        );
        assert_eq!(lines[0].text, "ready ");
        assert_eq!(lines[0].cursor_column, Some(5));
        assert!(!lines[0].text.contains('▏'));
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
    fn full_screen_refresh_preserves_history_without_accumulating_old_frames() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"old heading\r\nold row");
        output.append(b"\x1b[2J\x1b[Htop heading\r\nfresh row");

        let snapshot = output.snapshot();
        assert!(snapshot.contains("old heading\nold row"));
        assert!(snapshot.contains("top heading\nfresh row"));

        output.append(b"\x1b[H\x1b[2Jtop heading 2\r\nfresh row 2");
        let refreshed = output.snapshot();
        assert!(refreshed.contains("old heading\nold row"));
        assert!(refreshed.contains("top heading 2\nfresh row 2"));
        assert!(!refreshed.contains("top heading\nfresh row"));
    }

    #[test]
    fn alternate_screen_keeps_primary_history_and_only_the_latest_live_frame() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"history-one\r\nhistory-two\r\n$ top\r\n");
        output.append(b"\x1b[?1049h\x1b[2J\x1b[Htop frame one");

        let first = output.snapshot();
        assert!(first.contains("history-one"));
        assert!(first.contains("history-two"));
        assert!(first.contains("$ top"));
        assert!(first.contains("top frame one"));

        output.append(b"\x1b[2J\x1b[Htop frame two");
        let refreshed = output.snapshot();
        assert!(refreshed.contains("history-one"));
        assert!(refreshed.contains("$ top"));
        assert!(refreshed.contains("top frame two"));
        assert!(!refreshed.contains("top frame one"));

        output.append(b"\x1b[?1049l");
        let exited = output.snapshot();
        assert!(exited.contains("history-one"));
        assert!(exited.contains("$ top"));
        assert!(!exited.contains("top frame two"));
    }

    #[test]
    fn alternate_screen_sequence_can_be_split_across_ssh_reads() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"history\r\n$ top\r\n\x1b[?10");
        output.append(b"49h\x1b[2J\x1b[Hlive top");

        let snapshot = output.snapshot();
        assert!(snapshot.contains("history"));
        assert!(snapshot.contains("$ top"));
        assert!(snapshot.contains("live top"));
    }

    #[test]
    fn clear_removes_primary_and_alternate_screen_contents() {
        let mut output = TerminalOutputBuffer::default();
        output.append(b"history\r\n\x1b[?1049hlive top");
        output.clear();

        assert_eq!(output.snapshot(), "");
        assert!(output.primary_screen_before_alternate.is_none());
        assert!(output.primary_visible_before_clear.is_none());
    }

    #[test]
    fn retained_line_limit_can_be_changed() {
        let mut output = TerminalOutputBuffer::default();
        output.set_history_limit(100);
        output.append(
            (0..150)
                .map(|line| format!("line-{line}\r\n"))
                .collect::<String>()
                .as_bytes(),
        );

        let snapshot = output.snapshot();
        assert!(snapshot.lines().count() <= 100);
        assert!(!snapshot.contains("line-0\n"));
        assert!(snapshot.ends_with("line-149"));
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

    #[test]
    #[ignore = "requires docker-compose.test.yml services"]
    fn top_stays_connected_and_accepts_input_after_continuous_refresh() {
        let host = JumpHost {
            id: "docker-ssh-top".into(),
            name: "Docker SSH top".into(),
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
            .send_line("echo __S_PORTER_HISTORY_BEFORE_TOP__")
            .unwrap();
        let history_started = Instant::now();
        let mut revision = 0;
        loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if styled_snapshot_text(&output).contains("__S_PORTER_HISTORY_BEFORE_TOP__") {
                    break;
                }
            }
            assert!(
                history_started.elapsed() < Duration::from_secs(5),
                "进入 top 前的历史标记未出现"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        terminal.send_line("top -d 0.1").unwrap();

        let started = Instant::now();
        let mut saw_top = false;
        while started.elapsed() < Duration::from_secs(45) {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                let output = styled_snapshot_text(&output);
                saw_top |= output.contains("load average");
                assert!(
                    output.contains("__S_PORTER_HISTORY_BEFORE_TOP__"),
                    "top 运行期间进入 top 前的历史交互信息消失"
                );
            }
            assert!(terminal.is_running(), "top 持续刷新期间 SSH 连接意外断开");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_top, "没有读取到 top 输出");

        terminal.send_bytes(vec![0x03]).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        terminal.send_line("echo __S_PORTER_AFTER_TOP__").unwrap();
        let interrupted_at = Instant::now();
        loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if styled_snapshot_text(&output).contains("__S_PORTER_AFTER_TOP__") {
                    break;
                }
            }
            assert!(
                interrupted_at.elapsed() < Duration::from_secs(5),
                "退出 top 后 SSH 终端无法继续接受命令"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        terminal.stop();
    }

    #[test]
    #[ignore = "requires docker-compose.test.yml services"]
    fn ui_resize_and_character_input_do_not_disconnect_terminal() {
        let host = JumpHost {
            id: "docker-ssh-ui-input".into(),
            name: "Docker SSH UI input".into(),
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
        let control = terminal.control();
        let command = b"echo __S_PORTER_UI_INPUT__\r";

        for (index, byte) in command.iter().copied().enumerate() {
            control.resize(100 + index as u16, 30 + index as u16 % 5);
            terminal.send_bytes(vec![byte]).unwrap();
            assert!(
                terminal.is_running(),
                "页面 resize 后逐字符输入导致 SSH 连接断开"
            );
            std::thread::sleep(Duration::from_millis(2));
        }

        let started = Instant::now();
        let mut revision = 0;
        loop {
            if let Some((next_revision, output)) = terminal.output_if_changed(revision) {
                revision = next_revision;
                if styled_snapshot_text(&output).contains("__S_PORTER_UI_INPUT__") {
                    break;
                }
            }
            assert!(terminal.is_running(), "等待命令输出时 SSH 连接断开");
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "页面逐字符输入的命令未执行"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        terminal.stop();
    }
}
