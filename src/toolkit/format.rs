use anyhow::{Context, Result, bail};
use quick_xml::{Reader, Writer, events::Event};
use serde_json::Value;
use std::io::Cursor;

pub fn json_format(input: &str) -> Result<String> {
    let value: Value = serde_json::from_str(input).context("JSON 格式无效")?;
    serde_json::to_string_pretty(&value).context("JSON 格式化失败")
}

pub fn json_minify(input: &str) -> Result<String> {
    let value: Value = serde_json::from_str(input).context("JSON 格式无效")?;
    serde_json::to_string(&value).context("JSON 压缩失败")
}

pub fn json_escape_string(input: &str) -> Result<String> {
    serde_json::to_string(input).context("字符串转义失败")
}

pub fn json_unescape_string(input: &str) -> Result<String> {
    let input = input.trim();
    if input.starts_with('"') && input.ends_with('"') {
        return serde_json::from_str(input).context("字符串转义格式无效");
    }

    serde_json::from_str(&format!("\"{input}\"")).context("字符串转义格式无效")
}

fn transform_xml(input: &str, pretty: bool) -> Result<String> {
    let mut reader = Reader::from_str(input.trim());
    reader.trim_text(false);
    let output = Cursor::new(Vec::new());
    let mut writer = if pretty {
        Writer::new_with_indent(output, b' ', 2)
    } else {
        Writer::new(output)
    };
    let mut depth = 0_usize;
    let mut root_count = 0_usize;

    loop {
        let event = reader
            .read_event()
            .with_context(|| format!("XML 格式无效，位置 {}", reader.buffer_position()))?;
        match &event {
            Event::Start(_) => {
                if depth == 0 {
                    root_count += 1;
                }
                depth += 1;
            }
            Event::Empty(_) if depth == 0 => root_count += 1,
            Event::End(_) => {
                depth = depth.saturating_sub(1);
            }
            Event::Text(text) => {
                let bytes: &[u8] = text.as_ref();
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                if depth == 0 && !bytes.is_empty() {
                    bail!("XML 根元素外不能包含文本");
                }
            }
            Event::Eof => break,
            _ => {}
        }
        writer.write_event(event).context("XML 输出失败")?;
    }

    if root_count != 1 || depth != 0 {
        bail!("XML 必须包含且只能包含一个根元素");
    }

    let bytes = writer.into_inner().into_inner();
    let output = String::from_utf8(bytes).context("XML 不是 UTF-8 文本")?;
    Ok(output.trim_start_matches('\n').to_string())
}

pub fn xml_format(input: &str) -> Result<String> {
    transform_xml(input, true)
}

pub fn xml_minify(input: &str) -> Result<String> {
    transform_xml(input, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_operations() {
        let source = r#"{"name":"S Porter","ports":[22,8080]}"#;
        assert_eq!(
            json_format(source).unwrap(),
            "{\n  \"name\": \"S Porter\",\n  \"ports\": [\n    22,\n    8080\n  ]\n}"
        );
        assert_eq!(json_minify(&json_format(source).unwrap()).unwrap(), source);

        let escaped = json_escape_string(source).unwrap();
        assert_eq!(escaped, r#""{\"name\":\"S Porter\",\"ports\":[22,8080]}""#);
        assert_eq!(json_unescape_string(&escaped).unwrap(), source);
        assert_eq!(
            json_unescape_string(r#"{\"name\":\"S Porter\"}"#).unwrap(),
            r#"{"name":"S Porter"}"#
        );
    }

    #[test]
    fn invalid_json_is_rejected() {
        assert!(json_format("{name: 1}").is_err());
        assert!(json_unescape_string(r#"{\"name\":\q}"#).is_err());
    }

    #[test]
    fn xml_operations() {
        let source = r#"<?xml version="1.0"?><root id="1"><item>文本</item><empty/></root>"#;
        assert_eq!(
            xml_format(source).unwrap(),
            "<?xml version=\"1.0\"?>\n<root id=\"1\">\n  <item>文本</item>\n  <empty/>\n</root>"
        );
        assert_eq!(xml_minify(&xml_format(source).unwrap()).unwrap(), source);
    }

    #[test]
    fn xml_preserves_mixed_content_and_rejects_invalid_documents() {
        let mixed = "<p>Hello <b>world</b> !</p>";
        assert_eq!(xml_minify(mixed).unwrap(), mixed);
        assert!(xml_format("<root><item></root>").is_err());
        assert!(xml_format("<one/><two/>").is_err());
    }
}
