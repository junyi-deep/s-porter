use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

pub fn base64_encode(input: &str) -> String {
    STANDARD.encode(input.as_bytes())
}

pub fn base64_decode(input: &str) -> Result<String> {
    let bytes = STANDARD.decode(input.trim()).context("Base64 格式无效")?;
    String::from_utf8(bytes).context("解码结果不是 UTF-8 文本")
}

pub fn url_encode(input: &str) -> String {
    urlencoding::encode(input).into_owned()
}

pub fn url_decode(input: &str) -> Result<String> {
    Ok(urlencoding::decode(input)
        .context("URL 编码格式无效")?
        .into_owned())
}

pub fn md5_digest(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

pub fn sha256_digest(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codecs_round_trip() {
        assert_eq!(
            base64_decode(&base64_encode("端口 8080")).unwrap(),
            "端口 8080"
        );
        assert_eq!(url_decode(&url_encode("a b/中文")).unwrap(), "a b/中文");
        assert_eq!(md5_digest("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            sha256_digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
