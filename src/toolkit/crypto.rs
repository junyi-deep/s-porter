use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng},
};
use anyhow::{Context, Result, bail};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD};
use rand_core::RngCore;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

pub fn encrypt(plaintext: &str, password: &str) -> Result<String> {
    if password.is_empty() {
        bail!("加密密码不能为空");
    }
    let mut salt = [0_u8; SALT_LEN];
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let mut key = [0_u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|error| anyhow::anyhow!("密码派生失败：{error}"))?;
    let cipher = Aes256Gcm::new_from_slice(&key).expect("AES-256 key length");
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow::anyhow!("加密失败"))?;

    let mut payload = Vec::with_capacity(SALT_LEN + NONCE_LEN + encrypted.len());
    payload.extend_from_slice(&salt);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&encrypted);
    Ok(format!("SP1:{}", STANDARD.encode(payload)))
}

pub fn decrypt(ciphertext: &str, password: &str) -> Result<String> {
    let encoded = ciphertext
        .trim()
        .strip_prefix("SP1:")
        .context("不是 S Porter 加密文本")?;
    let payload = STANDARD.decode(encoded).context("密文格式无效")?;
    if payload.len() <= SALT_LEN + NONCE_LEN {
        bail!("密文长度无效");
    }
    let (salt, rest) = payload.split_at(SALT_LEN);
    let (nonce, encrypted) = rest.split_at(NONCE_LEN);
    let mut key = [0_u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|error| anyhow::anyhow!("密码派生失败：{error}"))?;
    let cipher = Aes256Gcm::new_from_slice(&key).expect("AES-256 key length");
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), encrypted)
        .map_err(|_| anyhow::anyhow!("解密失败，请检查密码或密文"))?;
    String::from_utf8(plaintext).context("解密结果不是 UTF-8 文本")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let encrypted = encrypt("你好, s-porter", "secret").unwrap();
        assert_eq!(decrypt(&encrypted, "secret").unwrap(), "你好, s-porter");
    }
}
