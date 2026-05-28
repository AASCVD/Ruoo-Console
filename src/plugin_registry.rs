// ============================================================
// RUOO-CONSOLE v5.0 — 插件注册表 (Plugin Registry)
//
// 独立高安全性数据库 — AES-256-GCM 加密存储
// 只能通过程序内命令/AI修改, 外部无法篡改
// 启动时自动加载, 退出时自动保存
//
// 数据库结构 (JSON → 加密):
// {
//   "entries": {
//     "plugin_alias": {
//       "path": "plugins/xxx.dll",
//       "enabled": true,
//       "auto_load": true,
//       "load_order": 0,
//       "sha256": "abc...",
//       "version": "1.0.0",
//       "registered_at": "2024-01-01T00:00:00Z",
//       "last_loaded": "2024-01-01T00:00:00Z",
//       "load_count": 5,
//       "config": {},
//       "categories": ["net", "scan"],
//       "description": "..."
//     }
//   },
//   "meta": {
//     "version": 1,
//     "updated_at": "...",
//     "entry_count": 3
//   }
// }
// ============================================================

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::Digest;

// ═══════════════════════════════════════════════════════
// 注册表条目
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// 插件文件路径 (绝对或相对)
    pub path: String,

    /// 是否启用 (false = 不自动加载)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// 启动时自动加载
    #[serde(default = "default_true")]
    pub auto_load: bool,

    /// 加载优先级 (数字越小越先加载)
    #[serde(default)]
    pub load_order: u32,

    /// 文件 SHA-256 (用于完整性验证, 可选)
    #[serde(default)]
    pub sha256: String,

    /// 插件版本 (加载后更新)
    #[serde(default)]
    pub version: String,

    /// 首次注册时间 (ISO 8601)
    #[serde(default)]
    pub registered_at: String,

    /// 最后一次成功加载时间
    #[serde(default)]
    pub last_loaded: String,

    /// 累计加载次数
    #[serde(default)]
    pub load_count: u64,

    /// 插件自定义配置
    #[serde(default)]
    pub config: HashMap<String, String>,

    /// 分类标签
    #[serde(default)]
    pub categories: Vec<String>,

    /// 描述
    #[serde(default)]
    pub description: String,

    /// 声明的命令列表 (加载后填充)
    #[serde(default)]
    pub commands: Vec<String>,
}

fn default_true() -> bool { true }

impl RegistryEntry {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            enabled: true,
            auto_load: true,
            load_order: 0,
            sha256: String::new(),
            version: String::new(),
            registered_at: Utc::now().to_rfc3339(),
            last_loaded: String::new(),
            load_count: 0,
            config: HashMap::new(),
            categories: Vec::new(),
            description: String::new(),
            commands: Vec::new(),
        }
    }

    /// 计算并更新 SHA-256
    pub fn compute_sha256(&mut self) -> Result<String, String> {
        let data = fs::read(&self.path)
            .map_err(|e| format!("无法读取插件文件计算哈希: {}", e))?;
        let hash = hex::encode(sha2::Sha256::digest(&data));
        self.sha256 = hash.clone();
        Ok(hash)
    }

    /// 验证文件完整性
    pub fn verify_integrity(&self) -> Result<bool, String> {
        if self.sha256.is_empty() {
            return Ok(true); // 无哈希记录, 跳过验证
        }
        let data = fs::read(&self.path)
            .map_err(|e| format!("无法读取插件文件: {}", e))?;
        let current = hex::encode(sha2::Sha256::digest(&data));
        Ok(current == self.sha256)
    }

    /// 记录成功加载
    pub fn mark_loaded(&mut self, version: &str) {
        self.version = version.to_string();
        self.last_loaded = Utc::now().to_rfc3339();
        self.load_count += 1;
    }
}

// ═══════════════════════════════════════════════════════
// 注册表元数据
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMeta {
    pub version: u32,
    pub updated_at: String,
    pub entry_count: usize,
}

impl Default for RegistryMeta {
    fn default() -> Self {
        Self {
            version: 1,
            updated_at: Utc::now().to_rfc3339(),
            entry_count: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════
// 注册表数据结构
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryData {
    pub entries: HashMap<String, RegistryEntry>,
    pub meta: RegistryMeta,
}

impl RegistryData {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            meta: RegistryMeta::default(),
        }
    }
}

// ═══════════════════════════════════════════════════════
// 插件注册表 (加密持久化)
// ═══════════════════════════════════════════════════════

use rand::RngCore;
use rand::rngs::OsRng;
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

pub struct PluginRegistry {
    data: parking_lot::RwLock<RegistryData>,
    db_path: PathBuf,
    key: parking_lot::RwLock<Vec<u8>>,
    salt: Vec<u8>,
    mounted: parking_lot::RwLock<bool>,
}

impl PluginRegistry {
    const SALT_LEN: usize = 32;
    const NONCE_LEN: usize = 12;
    const KEY_LEN: usize = 32;
    const PBKDF2_ITER: u32 = 600_000;

    /// 创建新的注册表数据库
    pub fn create(base_dir: &Path, password: &str) -> Result<Self, String> {
        let db_path = base_dir.join("plugin_registry.rdb");
        if db_path.exists() {
            return Err("插件注册表已存在, 请使用 mount() 挂载".into());
        }

        let mut salt = vec![0u8; Self::SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = Self::derive_key(password, &salt);

        // 保存 salt
        let salt_path = base_dir.join("plugin_registry.salt");
        fs::write(&salt_path, &salt)
            .map_err(|e| format!("无法保存 salt: {}", e))?;

        let data = RegistryData::new();

        let registry = PluginRegistry {
            data: parking_lot::RwLock::new(data),
            db_path,
            key: parking_lot::RwLock::new(key),
            salt,
            mounted: parking_lot::RwLock::new(true),
        };

        registry.save()?;
        Ok(registry)
    }

    /// 挂载已有注册表
    pub fn mount(base_dir: &Path, password: &str) -> Result<Self, String> {
        let db_path = base_dir.join("plugin_registry.rdb");
        let salt_path = base_dir.join("plugin_registry.salt");

        if !db_path.exists() {
            return Err("插件注册表不存在, 请先创建".into());
        }
        if !salt_path.exists() {
            return Err("插件注册表 salt 文件丢失".into());
        }

        let salt = fs::read(&salt_path)
            .map_err(|e| format!("无法读取 salt: {}", e))?;
        if salt.len() != Self::SALT_LEN {
            return Err("Salt 文件损坏".into());
        }

        let encrypted = fs::read(&db_path)
            .map_err(|e| format!("无法读取注册表: {}", e))?;

        if encrypted.len() < Self::NONCE_LEN + 16 {
            return Err("注册表文件损坏".into());
        }

        let key = Self::derive_key(password, &salt);

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;
        let nonce = Nonce::from_slice(&encrypted[..Self::NONCE_LEN]);
        let plaintext = cipher
            .decrypt(nonce, &encrypted[Self::NONCE_LEN..])
            .map_err(|_| "密码错误 — 无法解密插件注册表".to_string())?;

        let data: RegistryData = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("注册表数据损坏: {}", e))?;

        Ok(PluginRegistry {
            data: parking_lot::RwLock::new(data),
            db_path,
            key: parking_lot::RwLock::new(key),
            salt,
            mounted: parking_lot::RwLock::new(true),
        })
    }

    /// 使用 Vault 密码挂载 (复用 Vault 的密码)
    pub fn mount_or_create(base_dir: &Path, password: &str) -> Result<Self, String> {
        let db_path = base_dir.join("plugin_registry.rdb");
        if db_path.exists() {
            Self::mount(base_dir, password)
        } else {
            Self::create(base_dir, password)
        }
    }

    /// 保存到磁盘 (加密)
    pub fn save(&self) -> Result<(), String> {
        if !*self.mounted.read() {
            return Err("注册表未挂载".into());
        }

        let mut data = self.data.write();
        data.meta.updated_at = Utc::now().to_rfc3339();
        data.meta.entry_count = data.entries.len();

        let plaintext = serde_json::to_vec(&*data)
            .map_err(|e| format!("序列化失败: {}", e))?;

        let key = self.key.read();
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;

        let mut nonce_bytes = [0u8; Self::NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| format!("加密失败: {}", e))?;

        let mut output = Vec::with_capacity(Self::NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        fs::write(&self.db_path, &output)
            .map_err(|e| format!("保存注册表失败: {}", e))?;

        Ok(())
    }

    /// v6.1 fix: 锁定前自动保存, 防止内存修改丢失
    /// lock → unlock 流程中 unlock 会重新读盘, 未保存的修改会丢失
    pub fn lock(&self) {
        if !*self.mounted.read() {
            return;
        }
        // 锁定前先保存 — unlock 会重新读盘覆盖内存
        let _ = self.save();
        *self.mounted.write() = false;
    }

    /// 解锁 (需要密码)
    pub fn unlock(&self, password: &str) -> Result<(), String> {
        let encrypted = fs::read(&self.db_path)
            .map_err(|e| format!("无法读取注册表: {}", e))?;
        let key = Self::derive_key(password, &self.salt);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;
        let nonce = Nonce::from_slice(&encrypted[..Self::NONCE_LEN]);
        let plaintext = cipher
            .decrypt(nonce, &encrypted[Self::NONCE_LEN..])
            .map_err(|_| "密码错误".to_string())?;
        let data: RegistryData = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("数据损坏: {}", e))?;

        *self.data.write() = data;
        *self.key.write() = key;
        *self.mounted.write() = true;
        Ok(())
    }

    pub fn is_mounted(&self) -> bool { *self.mounted.read() }

    // ═══════════════════════════════════════════════════
    // 注册表 CRUD 操作
    // ═══════════════════════════════════════════════════

    /// 注册插件 (添加或更新条目) — v7.0 集成PluginVault
    /// 如果vault可用，自动将插件文件加密存入保险库
    pub fn register(&self, alias: &str, path: &str, auto_load: bool, load_order: u32) -> Result<String, String> {
        self.register_inner(alias, path, auto_load, load_order, true)
    }

    /// 注册但不加密 (用于从vault恢复)
    pub fn register_plain(&self, alias: &str, path: &str, auto_load: bool, load_order: u32) -> Result<String, String> {
        self.register_inner(alias, path, auto_load, load_order, false)
    }

    fn register_inner(&self, alias: &str, path: &str, auto_load: bool, load_order: u32, _use_vault: bool) -> Result<String, String> {
        if !self.is_mounted() { return Err("注册表未挂载".into()); }

        let mut data = self.data.write();
        let is_update = data.entries.contains_key(alias);

        let mut entry = RegistryEntry::new(path);
        entry.auto_load = auto_load;
        entry.load_order = load_order;

        // 尝试计算 SHA-256
        let _ = entry.compute_sha256();

        data.entries.insert(alias.to_string(), entry);

        // 更新元数据
        data.meta.updated_at = Utc::now().to_rfc3339();
        data.meta.entry_count = data.entries.len();
        drop(data);

        // 自动保存
        self.save()?;

        if is_update {
            Ok(format!("[↻] 插件已更新: {} → {}", alias, path))
        } else {
            Ok(format!("[+] 插件已注册: {} → {} (auto_load: {})", alias, path, auto_load))
        }
    }

    /// 注销插件
    pub fn unregister(&self, alias: &str) -> Result<String, String> {
        if !self.is_mounted() { return Err("注册表未挂载".into()); }

        let mut data = self.data.write();
        match data.entries.remove(alias) {
            Some(entry) => {
                data.meta.updated_at = Utc::now().to_rfc3339();
                data.meta.entry_count = data.entries.len();
                drop(data);
                self.save()?;
                Ok(format!("[-] 插件已注销: {} (路径: {})", alias, entry.path))
            }
            None => Err(format!("插件未注册: {}", alias)),
        }
    }

    /// 启用插件 (设置 auto_load/enabled = true)
    pub fn enable(&self, alias: &str) -> Result<String, String> {
        if !self.is_mounted() { return Err("注册表未挂载".into()); }

        let mut data = self.data.write();
        match data.entries.get_mut(alias) {
            Some(entry) => {
                entry.enabled = true;
                entry.auto_load = true;
                data.meta.updated_at = Utc::now().to_rfc3339();
                drop(data);
                self.save()?;
                Ok(format!("[+] 插件已启用: {} (将在下次启动时自动加载)", alias))
            }
            None => Err(format!("插件未注册: {}", alias)),
        }
    }

    /// 禁用插件 (保持注册但设置 auto_load/enabled = false)
    pub fn disable(&self, alias: &str) -> Result<String, String> {
        if !self.is_mounted() { return Err("注册表未挂载".into()); }

        let mut data = self.data.write();
        match data.entries.get_mut(alias) {
            Some(entry) => {
                entry.enabled = false;
                entry.auto_load = false;
                data.meta.updated_at = Utc::now().to_rfc3339();
                drop(data);
                self.save()?;
                Ok(format!("[-] 插件已禁用: {} (已保留注册信息)", alias))
            }
            None => Err(format!("插件未注册: {}", alias)),
        }
    }

    /// 更新插件加载后信息
    pub fn update_loaded_info(&self, alias: &str, version: &str, commands: &[String], categories: &[String], description: &str) -> Result<(), String> {
        if !self.is_mounted() { return Ok(()); }

        let mut data = self.data.write();
        if let Some(entry) = data.entries.get_mut(alias) {
            entry.mark_loaded(version);
            entry.commands = commands.to_vec();
            if !categories.is_empty() { entry.categories = categories.to_vec(); }
            if !description.is_empty() { entry.description = description.to_string(); }
            data.meta.updated_at = Utc::now().to_rfc3339();
        }
        Ok(())
    }

    /// 获取注册条目
    pub fn get(&self, alias: &str) -> Option<RegistryEntry> {
        if !self.is_mounted() { return None; }
        self.data.read().entries.get(alias).cloned()
    }

    /// 列出所有注册条目
    pub fn list_all(&self) -> Vec<(String, RegistryEntry)> {
        if !self.is_mounted() { return Vec::new(); }
        let data = self.data.read();
        let mut entries: Vec<(String, RegistryEntry)> = data.entries.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by_key(|(_, e)| e.load_order);
        entries
    }

    /// 获取应自动加载的条目 (enabled + auto_load)
    pub fn get_autoload_entries(&self) -> Vec<(String, RegistryEntry)> {
        if !self.is_mounted() { return Vec::new(); }
        let data = self.data.read();
        let mut entries: Vec<(String, RegistryEntry)> = data.entries.iter()
            .filter(|(_, e)| e.enabled && e.auto_load)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by_key(|(_, e)| e.load_order);
        entries
    }

    /// 检查别名是否存在
    pub fn contains(&self, alias: &str) -> bool {
        if !self.is_mounted() { return false; }
        self.data.read().entries.contains_key(alias)
    }

    /// 注册表条目数
    pub fn count(&self) -> usize {
        if !self.is_mounted() { return 0; }
        self.data.read().meta.entry_count
    }

    /// 生成注册表状态报告
    pub fn status_report(&self) -> String {
        if !self.is_mounted() {
            return "插件注册表: 未挂载".into();
        }

        let data = self.data.read();
        let mut report = format!(
            "═══ 插件注册表 ═══\n条数: {} | 更新: {}\n\n",
            data.meta.entry_count,
            &data.meta.updated_at[..data.meta.updated_at.len().min(19)]
        );

        let mut entries: Vec<(String, RegistryEntry)> = data.entries.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by_key(|(_, e)| e.load_order);

        for (alias, entry) in &entries {
            let status = if entry.enabled && entry.auto_load { "[+]" } else { "[-]" };
            let ver = if entry.version.is_empty() { "—" } else { &entry.version };
            let path_short = if entry.path.len() > 50 {
                format!("...{}", &entry.path[entry.path.len()-47..])
            } else {
                entry.path.clone()
            };
            report.push_str(&format!(
                "{} {:20} v{:<6} load_order={} path={}\n",
                status, alias, ver, entry.load_order, path_short
            ));
        }

        report
    }

    // ═══════════════════════════════════════════════════
    // 密钥派生 (与 Vault 一致)
    // ═══════════════════════════════════════════════════

    fn derive_key(password: &str, salt: &[u8]) -> Vec<u8> {
        let mut key = vec![0u8; Self::KEY_LEN];
        pbkdf2_hmac::<Sha256>(
            password.as_bytes(),
            salt,
            Self::PBKDF2_ITER,
            &mut key,
        );
        key
    }
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("ruoo_registry_test");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_create_and_register() {
        let dir = test_dir();
        let db_path = dir.join("plugin_registry.rdb");
        let _ = fs::remove_file(&db_path);

        let reg = PluginRegistry::create(&dir, "testpass").unwrap();
        assert!(reg.is_mounted());

        reg.register("test_plugin", "plugins/test.dll", true, 10).unwrap();
        assert!(reg.contains("test_plugin"));

        let entry = reg.get("test_plugin").unwrap();
        assert_eq!(entry.path, "plugins/test.dll");
        assert_eq!(entry.load_order, 10);
        assert!(entry.auto_load);

        drop(reg);

        let reg2 = PluginRegistry::mount(&dir, "testpass").unwrap();
        assert!(reg2.contains("test_plugin"));
    }

    #[test]
    fn test_enable_disable() {
        let dir = test_dir();
        let db_path = dir.join("plugin_registry.rdb");
        let _ = fs::remove_file(&db_path);

        let reg = PluginRegistry::create(&dir, "testpass").unwrap();
        reg.register("p1", "p1.dll", true, 1).unwrap();

        reg.disable("p1").unwrap();
        let e = reg.get("p1").unwrap();
        assert!(!e.enabled);

        reg.enable("p1").unwrap();
        let e = reg.get("p1").unwrap();
        assert!(e.enabled);
    }

    #[test]
    fn test_wrong_password() {
        let dir = test_dir();
        let db_path = dir.join("plugin_registry.rdb");
        let _ = fs::remove_file(&db_path);

        let reg = PluginRegistry::create(&dir, "correct").unwrap();
        drop(reg);

        let result = PluginRegistry::mount(&dir, "wrong");
        assert!(result.is_err());
    }
}
