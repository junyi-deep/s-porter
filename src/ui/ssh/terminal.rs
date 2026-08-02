//! SSH 终端键盘编码、文本选择与搜索算法。

use super::state::{TerminalSearchMatch, TerminalSelection};
use crate::forward;
use gpui::{Keystroke, Modifiers};
use unicode_width::UnicodeWidthChar as _;

fn terminal_byte_offset(text: &str, column: usize) -> usize {
    let mut display_column = 0;
    for (offset, character) in text.char_indices() {
        if display_column >= column {
            return offset;
        }
        display_column += character.width().unwrap_or(1);
    }
    text.len()
}

fn terminal_display_width(text: &str) -> usize {
    text.chars()
        .map(|character| character.width().unwrap_or(1))
        .sum()
}

pub(in crate::ui) fn terminal_selected_text(
    lines: &[forward::TerminalLine],
    selection: TerminalSelection,
) -> Option<String> {
    let (start, end) = if selection.anchor <= selection.cursor {
        (selection.anchor, selection.cursor)
    } else {
        (selection.cursor, selection.anchor)
    };
    if start == end || start.line >= lines.len() {
        return None;
    }
    let end_line = end.line.min(lines.len().saturating_sub(1));
    let mut selected = String::new();
    for (line_index, line) in lines.iter().enumerate().take(end_line + 1).skip(start.line) {
        let text = &line.text;
        let start_column = if line_index == start.line {
            start.column
        } else {
            0
        };
        let end_column = if line_index == end_line {
            end.column
        } else {
            terminal_display_width(text)
        };
        let start_offset = terminal_byte_offset(text, start_column);
        let end_offset = terminal_byte_offset(text, end_column);
        if start_offset < end_offset {
            selected.push_str(&text[start_offset..end_offset]);
        }
        if line_index < end_line {
            selected.push('\n');
        }
    }
    (!selected.is_empty()).then_some(selected)
}

pub(in crate::ui) fn terminal_search_matches(
    lines: &[forward::TerminalLine],
    query: &str,
) -> Vec<TerminalSearchMatch> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .flat_map(|(line, terminal_line)| {
            terminal_line
                .text
                .to_lowercase()
                .match_indices(&query)
                .map(move |(start, value)| TerminalSearchMatch {
                    line,
                    range: start..start + value.len(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn terminal_modifier_code(modifiers: Modifiers) -> u8 {
    1 + u8::from(modifiers.shift) + u8::from(modifiers.alt) * 2 + u8::from(modifiers.control) * 4
}

// Keyboard encoding follows the same xterm mappings used by Zed's terminal,
// whose implementation is derived from Alacritty's terminal input mapping.
pub(in crate::ui) fn terminal_key_bytes(
    keystroke: &Keystroke,
    application_cursor: bool,
    prefer_character_input: bool,
) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    // Tab 是 shell 补全键，必须先于 GPUI 的字符输入偏好处理。
    // 否则某些平台会把 Tab 标记为 prefer_character_input，但不提供 key_char，
    // 最终事件继续传播并触发界面焦点跳转。
    if keystroke.key == "tab" {
        return match modifiers {
            Modifiers {
                shift: true,
                control: false,
                alt: false,
                ..
            } => Some(b"\x1b[Z".to_vec()),
            Modifiers {
                shift: false,
                control: false,
                alt: false,
                ..
            } => Some(vec![b'\t']),
            _ => None,
        };
    }
    if prefer_character_input {
        return keystroke
            .key_char
            .as_deref()
            .map(|value| value.as_bytes().to_vec());
    }

    if modifiers.control && !modifiers.alt {
        let key = keystroke.key.as_str();
        let control = match key.to_ascii_lowercase().as_str() {
            "space" | "@" => Some(0),
            "[" => Some(27),
            "\\" => Some(28),
            "]" => Some(29),
            "^" => Some(30),
            "_" => Some(31),
            "?" => Some(127),
            value if value.len() == 1 => {
                let byte = value.as_bytes()[0];
                byte.is_ascii_lowercase().then_some(byte - b'a' + 1)
            }
            _ => None,
        };
        if let Some(control) = control {
            return Some(vec![control]);
        }
    }

    let no_modifiers = !modifiers.control && !modifiers.alt && !modifiers.shift;
    let cursor_prefix = if application_cursor { "\x1bO" } else { "\x1b[" };
    let value = match keystroke.key.as_str() {
        "escape" if no_modifiers => Some("\x1b".to_string()),
        "enter" if no_modifiers => Some("\r".to_string()),
        "enter" if modifiers.shift && !modifiers.control && !modifiers.alt => {
            Some("\n".to_string())
        }
        "enter" if modifiers.alt && !modifiers.control => Some("\x1b\r".to_string()),
        "backspace" if no_modifiers => Some("\x7f".to_string()),
        "backspace" if modifiers.control && !modifiers.alt => Some("\x08".to_string()),
        "backspace" if modifiers.alt && !modifiers.control => Some("\x1b\x7f".to_string()),
        "up" if no_modifiers => Some(format!("{cursor_prefix}A")),
        "down" if no_modifiers => Some(format!("{cursor_prefix}B")),
        "right" if no_modifiers => Some(format!("{cursor_prefix}C")),
        "left" if no_modifiers => Some(format!("{cursor_prefix}D")),
        "home" if no_modifiers => Some(format!("{cursor_prefix}H")),
        "end" if no_modifiers => Some(format!("{cursor_prefix}F")),
        "insert" if no_modifiers => Some("\x1b[2~".to_string()),
        "delete" if no_modifiers => Some("\x1b[3~".to_string()),
        "pageup" if no_modifiers => Some("\x1b[5~".to_string()),
        "pagedown" if no_modifiers => Some("\x1b[6~".to_string()),
        "f1" if no_modifiers => Some("\x1bOP".to_string()),
        "f2" if no_modifiers => Some("\x1bOQ".to_string()),
        "f3" if no_modifiers => Some("\x1bOR".to_string()),
        "f4" if no_modifiers => Some("\x1bOS".to_string()),
        "f5" if no_modifiers => Some("\x1b[15~".to_string()),
        "f6" if no_modifiers => Some("\x1b[17~".to_string()),
        "f7" if no_modifiers => Some("\x1b[18~".to_string()),
        "f8" if no_modifiers => Some("\x1b[19~".to_string()),
        "f9" if no_modifiers => Some("\x1b[20~".to_string()),
        "f10" if no_modifiers => Some("\x1b[21~".to_string()),
        "f11" if no_modifiers => Some("\x1b[23~".to_string()),
        "f12" if no_modifiers => Some("\x1b[24~".to_string()),
        "up" | "down" | "right" | "left" | "home" | "end"
            if modifiers.control || modifiers.alt || modifiers.shift =>
        {
            let suffix = match keystroke.key.as_str() {
                "up" => 'A',
                "down" => 'B',
                "right" => 'C',
                "left" => 'D',
                "home" => 'H',
                _ => 'F',
            };
            Some(format!(
                "\x1b[1;{}{suffix}",
                terminal_modifier_code(modifiers)
            ))
        }
        _ => None,
    };
    if let Some(value) = value {
        return Some(value.into_bytes());
    }

    let character = keystroke
        .key_char
        .as_deref()
        .or_else(|| (keystroke.key.chars().count() == 1).then_some(keystroke.key.as_str()))?;
    if modifiers.control {
        return None;
    }
    let mut bytes = Vec::with_capacity(character.len() + usize::from(modifiers.alt));
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(character.as_bytes());
    Some(bytes)
}
