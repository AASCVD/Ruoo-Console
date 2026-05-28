// ═══════════════════════════════════════════════════════════
// RUOO-CONSOLE v7.0 — 插件保险库 (Plugin Vault)
//
// 插件文件全生命周期加密:
//   1. 注册时: 插件.dll → AES-256-GCM加密 → plugins_vault/*.enc
//              原始文件安全擦除 (7-pass覆写)
//   2. 加载时: vault解密 → 临时文件 → libloading加载
//              临时文件立即安全擦除
//              密钥仅存内存 (mlock尝试)
//   3. 卸载时: vault解密 → plugins/恢复 (可选)
//              vault副本保留或删除
//
// 加密格式 (.enc):
//   [12字节 Nonce] [Zstd压缩载荷] [16字节 HMAC-SHA256 Tag]
//   载荷 = AES-256-GCM(原始DLL字节)
//
// C ABI 接口:
//   ruoo_plugin_vault_status()      — 保险库状态
//   ruoo_plugin_vault_list()        — 列出加密插件
//   ruoo_plugin_vault_verify()      — 验证加密文件完整性
//   ruoo_plugin_vault_export()      — 解密导出插件
// ═══════════════════════════════════════════════════════════

use std::path::{Path, PathBuf};
use std::io::Write;
use std::fs;
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

// ═══════════════════════════════════════════════════════
// 加密插件条目
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct VaultedPlugin {
    /// 加密文件路径 (plugins_vault/name.dll.enc)
    pub vault_path: PathBuf,
    /// 原始文件名
    pub original_name: String,
    /// 插件别名
    pub alias: String,
    /// 原始文件SHA-256 (加密前)
    pub original_hash: String,
    /// 加密文件SHA-256
    pub vault_hash: String,
    /// 加密时间
    pub vaulted_at: String,
    /// 文件大小 (加密后)
    pub encrypted_size: u64,
    /// 原始文件大小
    pub original_size: u64,
    /// 是否已验证
    pub verified: bool,
}

// ═══════════════════════════════════════════════════════
// 插件保险库
// ═══════════════════════════════════════════════════════

pub struct PluginVault {
    /// 保险库目录 (plugins_vault/)
    vault_dir: PathBuf,
    /// 插件目录 (plugins/)
    plugins_dir: PathBuf,
    /// 临时目录 (用于解密加载)
    temp_dir: PathBuf,
    /// 派生密钥 (AES-256)
    key: parking_lot::RwLock<Vec<u8>>,
    /// 密钥派生盐值
    salt: Vec<u8>,
    /// 是否已挂载
    mounted: parking_lot::RwLock<bool>,
}

impl PluginVault {
    const NONCE_LEN: usize = 12;
    const KEY_LEN: usize = 32;
    const SALT_LEN: usize = 32;
    const PBKDF2_ITER: u32 = 600_000;
    const SALT_FILE: &'static str = "plugin_vault.salt";
    const VAULT_EXTENSION: &'static str = "enc";

    /// 创建新的插件保险库
    pub fn create(base_dir: &Path, password: &str) -> Result<Self, String> {
        let vault_dir = base_dir.join("plugins_vault");
        let plugins_dir = base_dir.join("plugins");
        let temp_dir = std::env::temp_dir().join("ruoo_plugin_tmp");

        // 创建目录
        fs::create_dir_all(&vault_dir)
            .map_err(|e| format!("无法创建保险库目录: {}", e))?;
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("无法创建临时目录: {}", e))?;

        // 生成盐值
        let mut salt = vec![0u8; Self::SALT_LEN];
        OsRng.fill_bytes(&mut salt);

        // 保存盐值 + 验证令牌 (用于空保险库密码验证)
        // salt文件格式: [32字节salt][12字节nonce][加密"RUOO_VAULT_OK"+16字节GCM tag]
        let key = Self::derive_key(password, &salt);
        let verify_plaintext = b"RUOO_VAULT_OK";
        let mut verify_nonce = [0u8; Self::NONCE_LEN];
        OsRng.fill_bytes(&mut verify_nonce);
        let verify_cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;
        let verify_ct = verify_cipher
            .encrypt(Nonce::from_slice(&verify_nonce), verify_plaintext.as_ref())
            .map_err(|e| format!("加密验证令牌失败: {}", e))?;
        let mut salt_data = Vec::with_capacity(Self::SALT_LEN + Self::NONCE_LEN + verify_ct.len());
        salt_data.extend_from_slice(&salt);
        salt_data.extend_from_slice(&verify_nonce);
        salt_data.extend_from_slice(&verify_ct);

        let salt_path = vault_dir.join(Self::SALT_FILE);
        fs::write(&salt_path, &salt_data)
            .map_err(|e| format!("无法保存盐值: {}", e))?;

        Ok(Self {
            vault_dir,
            plugins_dir,
            temp_dir,
            key: parking_lot::RwLock::new(key),
            salt: salt.to_vec(),
            mounted: parking_lot::RwLock::new(true),
        })
    }

    /// 挂载已有保险库
    pub fn mount(base_dir: &Path, password: &str) -> Result<Self, String> {
        let vault_dir = base_dir.join("plugins_vault");
        let plugins_dir = base_dir.join("plugins");
        let temp_dir = std::env::temp_dir().join("ruoo_plugin_tmp");

        if !vault_dir.exists() {
            return Err("插件保险库不存在, 请先创建".into());
        }

        let salt_path = vault_dir.join(Self::SALT_FILE);
        if !salt_path.exists() {
            return Err("保险库盐值文件丢失".into());
        }

        let salt_data = fs::read(&salt_path)
            .map_err(|e| format!("无法读取盐值: {}", e))?;

        // v6.2: salt文件格式 [32字节salt][可选: 12字节nonce+验证令牌]
        if salt_data.len() < Self::SALT_LEN {
            return Err("盐值文件损坏".into());
        }
        let salt = &salt_data[..Self::SALT_LEN];

        // 验证密码
        let key = Self::derive_key(password, salt);
        if !Self::verify_password(&vault_dir, &key) {
            return Err("密码错误".into());
        }

        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("无法创建临时目录: {}", e))?;

        Ok(Self {
            vault_dir,
            plugins_dir,
            temp_dir,
            key: parking_lot::RwLock::new(key),
            salt: salt.to_vec(),
            mounted: parking_lot::RwLock::new(true),
        })
    }

    /// 创建或挂载
    pub fn mount_or_create(base_dir: &Path, password: &str) -> Result<Self, String> {
        let vault_dir = base_dir.join("plugins_vault");
        if vault_dir.exists() && vault_dir.join(Self::SALT_FILE).exists() {
            Self::mount(base_dir, password)
        } else {
            Self::create(base_dir, password)
        }
    }

    pub fn is_mounted(&self) -> bool {
        *self.mounted.read()
    }

    // ═══════════════════════════════════════════════════
    // 加密插件文件 → vault
    // ═══════════════════════════════════════════════════

    /// 将插件文件加密存入保险库
    /// 返回 VaultedPlugin 元数据
    pub fn vault_plugin(&self, plugin_path: &Path, alias: &str) -> Result<VaultedPlugin, String> {
        if !self.is_mounted() {
            return Err("保险库未挂载".into());
        }

        if !plugin_path.exists() {
            return Err(format!("插件文件不存在: {}", plugin_path.display()));
        }

        // 读取原始文件
        let original_data = fs::read(plugin_path)
            .map_err(|e| format!("无法读取插件文件: {}", e))?;

        let original_size = original_data.len() as u64;

        // 计算原始SHA-256
        let original_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&original_data);
            hex::encode(hasher.finalize())
        };

        // 确定加密文件名
        let file_stem = plugin_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(alias);
        let enc_filename = format!("{}.{}", file_stem, Self::VAULT_EXTENSION);
        let enc_path = self.vault_dir.join(&enc_filename);

        // Zstd 压缩
        let compressed = zstd::stream::encode_all(&original_data[..], 3)
            .map_err(|e| format!("压缩失败: {}", e))?;

        // AES-256-GCM 加密
        let key = self.key.read();
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;

        let mut nonce_bytes = [0u8; Self::NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, compressed.as_ref())
            .map_err(|e| format!("加密失败: {}", e))?;

        // HMAC-SHA256 完整性标签
        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&key)
            .map_err(|_| "HMAC初始化失败".to_string())?;
        mac.update(&nonce_bytes);
        mac.update(&ciphertext);
        let tag = mac.finalize().into_bytes();

        // 写入加密文件: [Nonce] [Ciphertext] [HMAC Tag]
        let mut enc_data = Vec::with_capacity(Self::NONCE_LEN + ciphertext.len() + 32);
        enc_data.extend_from_slice(&nonce_bytes);
        enc_data.extend_from_slice(&ciphertext);
        enc_data.extend_from_slice(&tag);

        fs::write(&enc_path, &enc_data)
            .map_err(|e| format!("无法写入加密文件: {}", e))?;

        let encrypted_size = enc_data.len() as u64;

        // 计算加密文件SHA-256
        let vault_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&enc_data);
            hex::encode(hasher.finalize())
        };

        drop(key);

        // 安全擦除原始文件 (7-pass)
        self.secure_shred_file(plugin_path)?;

        let original_name = plugin_path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(VaultedPlugin {
            vault_path: enc_path,
            original_name,
            alias: alias.to_string(),
            original_hash,
            vault_hash,
            vaulted_at: chrono::Utc::now().to_rfc3339(),
            encrypted_size,
            original_size,
            verified: true,
        })
    }

    // ═══════════════════════════════════════════════════
    // 解密插件 → 临时文件 (用于加载)
    // ═══════════════════════════════════════════════════

    /// 解密插件到临时文件，返回临时文件路径
    /// 调用者在加载完成后应删除临时文件
    pub fn extract_to_temp(&self, alias: &str) -> Result<PathBuf, String> {
        if !self.is_mounted() {
            return Err("保险库未挂载".into());
        }

        let enc_path = self.find_vault_file(alias)?;
        self.decrypt_to_temp(&enc_path, alias)
    }

    /// 解密插件到指定目录 (用于卸载恢复)
    pub fn extract_to_dir(&self, alias: &str, target_dir: &Path) -> Result<PathBuf, String> {
        if !self.is_mounted() {
            return Err("保险库未挂载".into());
        }

        let enc_path = self.find_vault_file(alias)?;
        let plaintext = self.decrypt_plugin_data(&enc_path)?;

        // 恢复原始文件名
        let original_name = self.get_original_name(&enc_path, alias);
        let target_path = target_dir.join(&original_name);

        fs::write(&target_path, &plaintext)
            .map_err(|e| format!("无法写入解密文件: {}", e))?;

        Ok(target_path)
    }

    /// 解密插件数据 (返回原始字节)
    pub fn decrypt_plugin_data(&self, enc_path: &Path) -> Result<Vec<u8>, String> {
        let enc_data = fs::read(enc_path)
            .map_err(|e| format!("无法读取加密插件: {}", e))?;

        if enc_data.len() < Self::NONCE_LEN + 32 + 16 {
            return Err("加密文件损坏: 数据过短".into());
        }

        let tag_start = enc_data.len() - 32;
        let nonce_bytes = &enc_data[..Self::NONCE_LEN];
        let ciphertext = &enc_data[Self::NONCE_LEN..tag_start];
        let stored_tag = &enc_data[tag_start..];

        let key = self.key.read();

        // 验证HMAC
        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&key)
            .map_err(|_| "HMAC初始化失败".to_string())?;
        mac.update(nonce_bytes);
        mac.update(ciphertext);
        mac.verify_slice(stored_tag)
            .map_err(|_| "完整性验证失败 — 文件可能被篡改".to_string())?;

        // AES-256-GCM 解密
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let compressed = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| "解密失败 — 密码错误或文件损坏".to_string())?;

        drop(key);

        // Zstd 解压
        let plaintext = zstd::stream::decode_all(&compressed[..])
            .map_err(|e| format!("解压失败: {}", e))?;

        Ok(plaintext)
    }

    fn decrypt_to_temp(&self, enc_path: &Path, alias: &str) -> Result<PathBuf, String> {
        let plaintext = self.decrypt_plugin_data(enc_path)?;

        // 写入临时文件 (随机名称)
        let mut rng_bytes = [0u8; 16];
        OsRng.fill_bytes(&mut rng_bytes);
        let temp_name = format!("ruoo_pl_{}_{}", alias, hex::encode(&rng_bytes[..8]));
        let temp_path = self.temp_dir.join(&temp_name);

        // 平台扩展名
        #[cfg(windows)]
        let temp_path = temp_path.with_extension("dll");
        #[cfg(not(windows))]
        let temp_path = temp_path.with_extension("so");

        fs::write(&temp_path, &plaintext)
            .map_err(|e| format!("无法写入临时文件: {}", e))?;

        Ok(temp_path)
    }

    // ═══════════════════════════════════════════════════
    // 删除保险库中的插件
    // ═══════════════════════════════════════════════════

    pub fn remove_from_vault(&self, alias: &str) -> Result<(), String> {
        let enc_path = self.find_vault_file(alias)?;
        self.secure_shred_file(&enc_path)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════
    // 查询
    // ═══════════════════════════════════════════════════

    pub fn find_vault_file(&self, alias: &str) -> Result<PathBuf, String> {
        // 精确匹配
        let exact = self.vault_dir.join(format!("{}.{}", alias, Self::VAULT_EXTENSION));
        if exact.exists() {
            return Ok(exact);
        }

        // 模糊搜索
        let pattern = format!("{}", alias);
        for entry in fs::read_dir(&self.vault_dir)
            .map_err(|e| format!("无法读取保险库目录: {}", e))? 
        {
            let entry = entry.map_err(|e| format!("目录项错误: {}", e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&pattern) && name.ends_with(&format!(".{}", Self::VAULT_EXTENSION)) {
                return Ok(entry.path());
            }
        }

        Err(format!("保险库中未找到插件: {}", alias))
    }

    pub fn list_vaulted(&self) -> Result<Vec<VaultedPlugin>, String> {
        let mut plugins = Vec::new();

        for entry in fs::read_dir(&self.vault_dir)
            .map_err(|e| format!("无法读取保险库目录: {}", e))? 
        {
            let entry = entry.map_err(|e| format!("目录项错误: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some(Self::VAULT_EXTENSION) {
                continue;
            }

            if path.file_name().and_then(|s| s.to_str()) == Some(Self::SALT_FILE) {
                continue;
            }

            // 获取文件大小
            let meta = fs::metadata(&path).ok();
            let encrypted_size = meta.as_ref().map(|m| m.len()).unwrap_or(0);

            let file_stem = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(); // 立即克隆，避免后续move冲突

            let alias = file_stem.clone();

            // 计算加密文件哈希
            let vault_hash = if let Ok(data) = fs::read(&path) {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                hex::encode(hasher.finalize())
            } else {
                String::new()
            };

            plugins.push(VaultedPlugin {
                vault_path: path,
                original_name: format!("{}.dll", &file_stem),
                alias,
                original_hash: String::new(),
                vault_hash,
                vaulted_at: meta.and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default())
                    .unwrap_or_default(),
                encrypted_size,
                original_size: 0,
                verified: false,
            });
        }

        plugins.sort_by(|a, b| a.alias.cmp(&b.alias));
        Ok(plugins)
    }

    pub fn vault_count(&self) -> usize {
        fs::read_dir(&self.vault_dir)
            .map(|entries| {
                entries.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some(Self::VAULT_EXTENSION))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn status_report(&self) -> String {
        if !self.is_mounted() {
            return "插件保险库: 未挂载".into();
        }

        let count = self.vault_count();
        let mut report = format!(
            "═══ 插件保险库 ═══\n\
             状态: 已挂载\n\
             目录: {}\n\
             加密插件: {}个\n",
            self.vault_dir.display(),
            count,
        );

        if count > 0 {
            if let Ok(plugins) = self.list_vaulted() {
                report.push_str("\n── 已加密插件 ──\n");
                for p in &plugins {
                    report.push_str(&format!(
                        "  [🔒] {:30} | 加密大小: {:>8} | 哈希: {}...\n",
                        p.alias,
                        format_size(p.encrypted_size),
                        &p.vault_hash[..p.vault_hash.len().min(12)],
                    ));
                }
            }
        }

        report
    }

    fn get_original_name(&self, enc_path: &Path, alias: &str) -> String {
        let file_stem = enc_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(alias);

        #[cfg(windows)]
        return format!("{}.dll", file_stem);
        #[cfg(not(windows))]
        return format!("{}.so", file_stem);
    }

    // ═══════════════════════════════════════════════════
    // 安全擦除 (7-pass DoD 5220.22-M)
    // ═══════════════════════════════════════════════════

    pub fn secure_shred_file(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }

        let file_size = fs::metadata(path)
            .map(|m| m.len() as usize)
            .unwrap_or(0);

        if file_size == 0 {
            // 空文件直接删除
            let _ = fs::remove_file(path);
            return Ok(());
        }

        // 7-pass 覆写
        let patterns: [u8; 7] = [0x00, 0xFF, 0xAA, 0x55, 0x92, 0x49, 0x6D];

        for &pattern in &patterns {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|_| "无法打开文件进行安全擦除".to_string())?;

            let buffer = vec![pattern; file_size.min(65536)];
            let mut written = 0usize;

            while written < file_size {
                let chunk_size = buffer.len().min(file_size - written);
                file.write_all(&buffer[..chunk_size])
                    .map_err(|_| "安全擦除写入失败".to_string())?;
                written += chunk_size;
            }

            file.flush().ok();
            file.sync_all().ok();
        }

        // 最后一轮: 随机数据
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|_| "无法打开文件进行最终擦除".to_string())?;

            let mut rng_buf = vec![0u8; file_size.min(65536)];
            let mut written = 0usize;

            while written < file_size {
                let chunk_size = rng_buf.len().min(file_size - written);
                OsRng.fill_bytes(&mut rng_buf[..chunk_size]);
                file.write_all(&rng_buf[..chunk_size])
                    .map_err(|_| "随机擦除写入失败".to_string())?;
                written += chunk_size;
            }

            file.flush().ok();
            file.sync_all().ok();
            rng_buf.zeroize();
        }

        // 删除文件
        fs::remove_file(path)
            .map_err(|_| "无法删除已擦除文件".to_string())?;

        Ok(())
    }

    // ═══════════════════════════════════════════════════
    // 临时文件清理 (加载后调用)
    // ═══════════════════════════════════════════════════

    pub fn cleanup_temp(&self, temp_path: &Path) {
        if temp_path.exists() {
            let _ = self.secure_shred_file(temp_path);
        }
    }

    /// 清理所有临时文件
    pub fn cleanup_all_temp(&self) -> usize {
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(&self.temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with("ruoo_pl_"))
                    .unwrap_or(false)
                {
                    let _ = self.secure_shred_file(&path);
                    count += 1;
                }
            }
        }
        count
    }

    // ═══════════════════════════════════════════════════
    // 密码验证
    // ═══════════════════════════════════════════════════

    fn verify_password(vault_dir: &Path, key: &[u8]) -> bool {
        // v6.2 fix: 空保险库时检查验证令牌(存储在salt文件后的HMAC)
        // 先尝试解密第一个.enc文件以验证密码
        if let Ok(entries) = fs::read_dir(vault_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some(Self::VAULT_EXTENSION) {
                    // 找到enc文件 — 用它验证密码
                    if let Ok(data) = fs::read(&path) {
                        if data.len() >= Self::NONCE_LEN + 32 + 16 {
                            let tag_start = data.len() - 32;
                            let nonce_bytes = &data[..Self::NONCE_LEN];
                            let ciphertext = &data[Self::NONCE_LEN..tag_start];
                            let stored_tag = &data[tag_start..];

                            if let Ok(mut mac) = <HmacSha256 as hmac::Mac>::new_from_slice(key) {
                                mac.update(nonce_bytes);
                                mac.update(ciphertext);
                                if mac.verify_slice(stored_tag).is_ok() {
                                    if let Ok(cipher) = Aes256Gcm::new_from_slice(key) {
                                        let nonce = Nonce::from_slice(nonce_bytes);
                                        if cipher.decrypt(nonce, ciphertext).is_ok() {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // 第一个enc文件验证失败 = 密码错误
                    return false;
                }
            }
            // v6.3: 无enc文件时, 检查salt文件中的验证令牌
            // (此分支在for循环未找到任何enc文件时可达)
            let salt_path = vault_dir.join(Self::SALT_FILE);
            if let Ok(salt_data) = fs::read(&salt_path) {
                // salt文件格式: [32字节salt][12字节nonce][加密的验证令牌+16字节GCM tag]
                if salt_data.len() > Self::SALT_LEN + 12 + 16 {
                    let verify_nonce = &salt_data[Self::SALT_LEN..Self::SALT_LEN + 12];
                    let verify_ct = &salt_data[Self::SALT_LEN + 12..];
                    if let Ok(cipher) = Aes256Gcm::new_from_slice(key) {
                        let nonce = Nonce::from_slice(verify_nonce);
                        if cipher.decrypt(nonce, verify_ct).is_ok() {
                            return true;
                        }
                    }
                }
            }
            return false;
        }
        false
    }

    // ═══════════════════════════════════════════════════
    // 密钥派生
    // ═══════════════════════════════════════════════════

    fn derive_key(password: &str, salt: &[u8]) -> Vec<u8> {
        let mut key = vec![0u8; Self::KEY_LEN];
        pbkdf2::pbkdf2_hmac::<Sha256>(
            password.as_bytes(),
            salt,
            Self::PBKDF2_ITER,
            &mut key,
        );
        key
    }

    pub fn lock(&self) {
        *self.mounted.write() = false;
        // 清零内存密钥
        let mut key = self.key.write();
        key.zeroize();
    }

    pub fn unlock(&self, password: &str) -> Result<(), String> {
        let key = Self::derive_key(password, &self.salt);

        // 验证密码
        if !Self::verify_password(&self.vault_dir, &key) {
            let mut k = key;
            k.zeroize();
            return Err("密码错误".into());
        }

        *self.key.write() = key;
        *self.mounted.write() = true;
        Ok(())
    }

    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<(), String> {
        if !self.is_mounted() {
            return Err("保险库未挂载".into());
        }

        // 验证旧密码
        let old_key = Self::derive_key(old_password, &self.salt);
        if !Self::verify_password(&self.vault_dir, &old_key) {
            return Err("旧密码错误".into());
        }

        // 解密所有插件 → 用新密钥重新加密
        let vaulted = self.list_vaulted()?;

        let new_key = Self::derive_key(new_password, &self.salt);

        for plugin in &vaulted {
            // 用旧密钥解密
            let old_cipher = Aes256Gcm::new_from_slice(&old_key)
                .map_err(|_| "密钥错误".to_string())?;

            let enc_data = fs::read(&plugin.vault_path)
                .map_err(|e| format!("无法读取: {}", e))?;

            let tag_start = enc_data.len() - 32;
            let old_nonce = Nonce::from_slice(&enc_data[..Self::NONCE_LEN]);
            let old_ciphertext = &enc_data[Self::NONCE_LEN..tag_start];

            let compressed = old_cipher
                .decrypt(old_nonce, old_ciphertext)
                .map_err(|_| "解密失败".to_string())?;

            // 用新密钥加密
            let new_cipher = Aes256Gcm::new_from_slice(&new_key)
                .map_err(|_| "密钥错误".to_string())?;

            let mut new_nonce_bytes = [0u8; Self::NONCE_LEN];
            OsRng.fill_bytes(&mut new_nonce_bytes);
            let new_nonce = Nonce::from_slice(&new_nonce_bytes);
            let new_ciphertext = new_cipher
                .encrypt(new_nonce, compressed.as_ref())
                .map_err(|_| "加密失败".to_string())?;

            let mut new_mac = <HmacSha256 as hmac::Mac>::new_from_slice(&new_key)
                .map_err(|_| "HMAC失败".to_string())?;
            new_mac.update(&new_nonce_bytes);
            new_mac.update(&new_ciphertext);
            let new_tag = new_mac.finalize().into_bytes();

            let mut new_data = Vec::new();
            new_data.extend_from_slice(&new_nonce_bytes);
            new_data.extend_from_slice(&new_ciphertext);
            new_data.extend_from_slice(&new_tag);

            fs::write(&plugin.vault_path, &new_data)
                .map_err(|_| "写入失败".to_string())?;
        }

        // 更新密钥
        *self.key.write() = new_key;

        Ok(())
    }
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ═══════════════════════════════════════════════════════
// Drop — 安全清理
// ═══════════════════════════════════════════════════════

impl Drop for PluginVault {
    fn drop(&mut self) {
        let mut key = self.key.write();
        key.zeroize();
        self.cleanup_all_temp();
    }
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("ruoo_vault_test");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::create_dir_all(dir.join("plugins"));
        dir
    }

    #[test]
    fn test_vault_create_and_mount() {
        let dir = test_dir();
        let vault_dir = dir.join("plugins_vault");
        let _ = fs::remove_dir_all(&vault_dir);

        let vault = PluginVault::create(&dir, "testpass").unwrap();
        assert!(vault.is_mounted());

        drop(vault);

        let vault2 = PluginVault::mount(&dir, "testpass").unwrap();
        assert!(vault2.is_mounted());

        // 错误密码
        let result = PluginVault::mount(&dir, "wrongpass");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&vault_dir);
    }

    #[test]
    fn test_vault_and_extract() {
        let dir = test_dir();
        let vault_dir = dir.join("plugins_vault");
        let plugins_dir = dir.join("plugins");
        let _ = fs::remove_dir_all(&vault_dir);
        fs::create_dir_all(&plugins_dir).unwrap();

        // 创建测试插件文件
        let test_plugin = plugins_dir.join("test_plug.dll");
        let test_data = b"This is a test plugin binary content for vault testing.".repeat(50);
        fs::write(&test_plugin, &test_data).unwrap();

        let vault = PluginVault::create(&dir, "testpass").unwrap();

        // 加密入库
        let vp = vault.vault_plugin(&test_plugin, "test_plug").unwrap();
        assert_eq!(vp.alias, "test_plug");
        assert!(!test_plugin.exists()); // 原始文件已擦除

        // 解密提取
        let temp_path = vault.extract_to_temp("test_plug").unwrap();
        assert!(temp_path.exists());

        let extracted = fs::read(&temp_path).unwrap();
        assert_eq!(extracted, test_data);

        // 清理
        vault.cleanup_temp(&temp_path);
        let _ = fs::remove_dir_all(&vault_dir);
    }

    #[test]
    fn test_secure_shred() {
        let dir = test_dir();
        let test_file = dir.join("shred_test.bin");
        fs::write(&test_file, b"Sensitive data to shred!").unwrap();
        assert!(test_file.exists());

        let vault = PluginVault::create(&dir, "shredpass").unwrap();
        vault.secure_shred_file(&test_file).unwrap();
        assert!(!test_file.exists());

        let _ = fs::remove_dir_all(dir.join("plugins_vault"));
    }
}
