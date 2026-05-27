// ── 密码学 (hash, bcrypt, aes, jwt, passwords) ──
// v4.1: 纯 Rust 实现 — 移除外部命令回退，消除 XOR/openssl 依赖
use sha2::Digest;

mod hex_mod {
    pub fn encode(data: impl AsRef<[u8]>) -> String { data.as_ref().iter().map(|b| format!("{:02x}", b)).collect() }
}

pub fn hash_md5(text: &str) -> String { use md5::{Md5,Digest}; hex_mod::encode(Md5::digest(text.as_bytes())) }
pub fn hash_sha1(text: &str) -> String { use sha1::{Sha1,Digest}; hex_mod::encode(Sha1::digest(text.as_bytes())) }
pub fn hash_sha256(text: &str) -> String { hex_mod::encode(sha2::Sha256::digest(text.as_bytes())) }
pub fn hash_sha512(text: &str) -> String { hex_mod::encode(sha2::Sha512::digest(text.as_bytes())) }

pub fn file_hash(filename: &str, algorithm: &str) -> Result<String, String> {
    let data = std::fs::read(filename).map_err(|e| format!("读取文件失败: {}", e))?;
    match algorithm.to_lowercase().as_str() {
        "md5" => Ok(hash_md5_bytes(&data)), "sha1"|"sha-1" => Ok(hash_sha1_bytes(&data)),
        "sha256"|"sha-256" => Ok(hash_sha256_bytes(&data)), "sha512"|"sha-512" => Ok(hash_sha512_bytes(&data)),
        _ => Err(format!("不支持的算法: {}", algorithm)),
    }
}
fn hash_md5_bytes(data: &[u8]) -> String { use md5::{Md5,Digest}; hex_mod::encode(Md5::digest(data)) }
fn hash_sha1_bytes(data: &[u8]) -> String { use sha1::{Sha1,Digest}; hex_mod::encode(Sha1::digest(data)) }
fn hash_sha256_bytes(data: &[u8]) -> String { hex_mod::encode(sha2::Sha256::digest(data)) }
fn hash_sha512_bytes(data: &[u8]) -> String { hex_mod::encode(sha2::Sha512::digest(data)) }

// ─── bcrypt — 纯 Rust 实现（bcrypt crate）─────────────────
pub fn bcrypt_hash(password: &str, rounds: u32) -> Result<String, String> {
    let cost = rounds.clamp(4, 31);
    bcrypt::hash(password, cost).map_err(|e| format!("bcrypt 哈希失败: {}", e))
}

#[allow(dead_code)]
pub fn bcrypt_verify(password: &str, hash: &str) -> Result<bool, String> {
    bcrypt::verify(password, hash).map_err(|e| format!("bcrypt 验证失败: {}", e))
}

// ─── AES-256-GCM — 纯 Rust（aes-gcm crate）─────────────────
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

const AES_NONCE_LEN: usize = 12;
const AES_KEY_LEN: usize = 32;

fn derive_aes_key(password: &str, salt: &[u8]) -> Vec<u8> {
    let mut key = vec![0u8; AES_KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut key);
    key
}

pub fn aes_encrypt(text: &str, key: &str) -> Result<String, String> {
    // 纯 Rust AES-256-GCM: nonce(12B) || ciphertext+tag → Base64
    let mut salt = vec![0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let derived = derive_aes_key(key, &salt);

    let cipher = Aes256Gcm::new_from_slice(&derived)
        .map_err(|e| format!("AES 密钥错误: {}", e))?;

    let mut nonce_bytes = [0u8; AES_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, text.as_bytes())
        .map_err(|e| format!("AES 加密失败: {}", e))?;

    // 格式: salt(16B) || nonce(12B) || ciphertext → Base64
    let mut combined = Vec::with_capacity(16 + 12 + ciphertext.len());
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(crate::utils::base64_encode_bytes(&combined))
}

pub fn aes_decrypt(data_b64: &str, key: &str) -> Result<String, String> {
    let combined = crate::utils::base64_decode_bytes(data_b64)
        .map_err(|e| format!("Base64 解码失败: {}", e))?;

    if combined.len() < 16 + 12 + 16 {
        return Err("密文格式错误（数据太短）".into());
    }

    let salt = &combined[..16];
    let nonce_bytes = &combined[16..28];
    let ciphertext = &combined[28..];

    let derived = derive_aes_key(key, salt);
    let cipher = Aes256Gcm::new_from_slice(&derived)
        .map_err(|e| format!("AES 密钥错误: {}", e))?;

    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "AES 解密失败: 密码错误或数据损坏".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 解码失败: {}", e))
}

// ─── v10.0: HMAC (多算法) — 使用已有hmac+sha2 crate ──────
use hmac::{Hmac, Mac};
type HmacSha256 = Hmac<sha2::Sha256>;
type HmacSha512 = Hmac<sha2::Sha512>;

pub fn hmac_sha256(key: &str, message: &str) -> String {
    let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA256: 无效密钥");
    mac.update(message.as_bytes());
    hex_mod::encode(mac.finalize().into_bytes())
}

pub fn hmac_sha512(key: &str, message: &str) -> String {
    let mut mac = <HmacSha512 as hmac::Mac>::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA512: 无效密钥");
    mac.update(message.as_bytes());
    hex_mod::encode(mac.finalize().into_bytes())
}

// ─── v10.0: 常量时间比较 ─────────────────────────────────
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn constant_time_str_eq(a: &str, b: &str) -> bool {
    constant_time_eq(a.as_bytes(), b.as_bytes())
}

// ─── v10.0: Base32 编解码 ─────────────────────────────────
pub fn base32_encode(data: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for &byte in data {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(alphabet[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(alphabet[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    while result.len() % 8 != 0 {
        result.push('=');
    }
    result
}

pub fn base32_decode(encoded: &str) -> Result<Vec<u8>, String> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let clean = encoded.trim_end_matches('=').to_uppercase();
    let mut result = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for c in clean.chars() {
        let idx = alphabet.iter().position(|&a| a == c as u8)
            .ok_or_else(|| format!("无效Base32字符: {}", c))?;
        buffer = (buffer << 5) | idx as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            result.push((buffer >> bits) as u8);
        }
    }
    Ok(result)
}

// ─── v10.0: CRC32 / CRC64 ──────────────────────────────
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub fn crc32_hex(data: &[u8]) -> String {
    format!("{:08X}", crc32(data))
}

// ─── v10.0: PBKDF2 基准测试 ──────────────────────────────
pub fn pbkdf2_benchmark(iterations: u32) -> String {
    use std::time::Instant;
    let password = b"benchmark_password_123!";
    let salt = b"benchmark_salt_456";
    let mut key = vec![0u8; 32];

    let start = Instant::now();
    pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut key);
    let elapsed = start.elapsed();

    format!(
        "PBKDF2-SHA256: {} iterations in {:.3}ms ({:.0} iter/s)",
        iterations,
        elapsed.as_secs_f64() * 1000.0,
        iterations as f64 / elapsed.as_secs_f64()
    )
}

// ─── v10.0: 安全随机 (密码学级) ──────────────────────────
pub fn secure_random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

pub fn secure_random_hex(len: usize) -> String {
    hex_mod::encode(secure_random_bytes(len))
}

// ─── v10.0: 密钥派生函数比较 ─────────────────────────────
pub fn kdf_compare(password: &str, salt: &[u8]) -> Vec<String> {
    use std::time::Instant;
    let mut results = Vec::new();
    results.push("═══ KDF 性能比较 ═══".into());

    // PBKDF2-SHA256 100K
    let start = Instant::now();
    let mut key1 = vec![0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut key1);
    results.push(format!("  PBKDF2-SHA256 (100K iter):    {:.2}ms", start.elapsed().as_secs_f64() * 1000.0));

    // PBKDF2-SHA512 100K
    let start = Instant::now();
    let mut key2 = vec![0u8; 32];
    pbkdf2_hmac::<sha2::Sha512>(password.as_bytes(), salt, 100_000, &mut key2);
    results.push(format!("  PBKDF2-SHA512 (100K iter):    {:.2}ms", start.elapsed().as_secs_f64() * 1000.0));

    // bcrypt cost=10
    let start = Instant::now();
    let _ = bcrypt::hash(password, 10);
    results.push(format!("  bcrypt (cost=10):             {:.2}ms", start.elapsed().as_secs_f64() * 1000.0));

    results
}

// ─── JWT / Password / HashID ────────────────────────────────

pub fn jwt_decode(token: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 { return Err("无效 JWT: 需要 3 个部分 (header.payload.signature)".into()); }
    let header = crate::utils::base64_decode_str(parts[0]).map_err(|e| format!("Header 解码失败: {}", e))?;
    let payload = crate::utils::base64_decode_str(parts[1]).map_err(|e| format!("Payload 解码失败: {}", e))?;
    Ok((header, payload))
}

pub fn random_password(length: usize) -> String {
    use rand::Rng;
    let upper: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ".chars().collect();
    let lower: Vec<char> = "abcdefghjkmnpqrstuvwxyz".chars().collect();
    let digits: Vec<char> = "23456789".chars().collect();
    let special: Vec<char> = "!@#$%^&*_-+=".chars().collect();
    let all: Vec<char> = upper.iter().chain(lower.iter()).chain(digits.iter()).chain(special.iter()).copied().collect();
    let mut rng = rand::thread_rng();
    let mut pass: Vec<char> = vec![upper[rng.gen_range(0..upper.len())], lower[rng.gen_range(0..lower.len())], digits[rng.gen_range(0..digits.len())], special[rng.gen_range(0..special.len())]];
    for _ in 4..length { pass.push(all[rng.gen_range(0..all.len())]); }
    for i in (1..pass.len()).rev() { let j=rng.gen_range(0..=i); pass.swap(i,j); }
    pass.into_iter().collect()
}

pub fn hash_identifier(hash: &str) -> Vec<String> {
    let h = hash.trim(); let mut candidates = Vec::new();
    let is_hex = h.chars().all(|c| c.is_ascii_hexdigit());
    match h.len() {
        32 if is_hex => candidates.push("MD5 / MD4 / NTLM / LM / MySQL323".into()),
        40 if is_hex => candidates.push("SHA-1 / MySQL5.x / Tiger-160 / RIPEMD-160".into()),
        56 if is_hex => candidates.push("SHA-224 / SHA3-224".into()),
        64 if is_hex => candidates.push("SHA-256 / SHA3-256 / Blake2b-256 / GOST".into()),
        96 if is_hex => candidates.push("SHA-384 / SHA3-384".into()),
        128 if is_hex => candidates.push("SHA-512 / SHA3-512 / Whirlpool / Blake2b-512".into()),
        60|61|62|64 if h.starts_with("$2a$")||h.starts_with("$2b$")||h.starts_with("$2y$") => { candidates.push("bcrypt (Blowfish)".into()); }
        34 if h.starts_with("$1$") => candidates.push("MD5 Crypt (Unix)".into()),
        20 if h.starts_with("$5$") => candidates.push("SHA-256 Crypt (Unix)".into()),
        20..=200 if h.starts_with("$6$") => candidates.push("SHA-512 Crypt (Unix)".into()),
        _ => {}
    }
    if h.contains("$argon2") { candidates.push("Argon2".into()); }
    if h.contains("$scrypt$") { candidates.push("scrypt".into()); }
    if h.len()>=13 && h.starts_with("$P$") { candidates.push("phpass (WordPress/Drupal)".into()); }
    if candidates.is_empty() { candidates.push(format!("未知类型 (长度: {})", h.len())); }
    candidates
}
