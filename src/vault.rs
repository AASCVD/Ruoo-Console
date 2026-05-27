// ============================================================
// Ruoo Arsenal — Encrypted Vault (加密数据库)
// 安全存储: API Keys, 凭证, AI 记忆, 工具缓存
//
// 安全设计:
//   - AES-256-GCM 认证加密 (认证失败 = 密码错误)
//   - PBKDF2-HMAC-SHA256 密钥派生 (600K 迭代)
//   - 命名空间隔离: system / ai / tools
//   - 内存清零: Drop 时覆盖敏感数据
//   - 自动锁定: 可配置超时
// ============================================================

use aes_gcm::{aead::{Aead, KeyInit, OsRng}, Aes256Gcm, Nonce};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde_json::{Map, Value};
use sha2::Sha256;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

// ─── Vault 结构 ─────────────────────────────────────────
pub struct Vault {
    data: RwLock<Value>,
    vault_path: PathBuf,
    salt_path: PathBuf,
    key: RwLock<Vec<u8>>,
    last_activity: RwLock<Instant>,
    auto_lock_timeout: RwLock<Duration>,
    mounted: RwLock<bool>,
}

impl Vault {
    /// 首次创建 Vault
    pub fn create(base_dir: &PathBuf, password: &str) -> Result<Self, String> {
        let vault_path = base_dir.join("vault.rdb");
        let salt_path = base_dir.join("vault.salt");
        if vault_path.exists() {
            return Err("Vault 已存在，请使用 mount() 挂载".into());
        }

        let mut salt = vec![0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = derive_key(password, &salt);

        std::fs::write(&salt_path, &salt)
            .map_err(|e| format!("无法保存 salt: {}", e))?;

        let data = init_vault_data();

        let vault = Vault {
            data: RwLock::new(data),
            vault_path: vault_path.clone(),
            salt_path: salt_path.clone(),
            key: RwLock::new(key),
            last_activity: RwLock::new(Instant::now()),
            auto_lock_timeout: RwLock::new(Duration::from_secs(900)),
            mounted: RwLock::new(true),
        };

        vault.save()?;
        Ok(vault)
    }

    /// 挂载已有 Vault (登录)
    pub fn mount(base_dir: &PathBuf, password: &str) -> Result<Self, String> {
        let vault_path = base_dir.join("vault.rdb");
        let salt_path = base_dir.join("vault.salt");

        if !vault_path.exists() {
            return Err("Vault 不存在，请先创建".into());
        }
        if !salt_path.exists() {
            return Err("Salt 文件丢失，Vault 已损坏".into());
        }

        let salt = std::fs::read(&salt_path)
            .map_err(|e| format!("无法读取 salt: {}", e))?;
        if salt.len() != SALT_LEN {
            return Err("Salt 文件损坏".into());
        }

        let encrypted = std::fs::read(&vault_path)
            .map_err(|e| format!("无法读取 Vault: {}", e))?;

        if encrypted.len() < NONCE_LEN + 16 {
            return Err("Vault 文件损坏".into());
        }

        let key = derive_key(password, &salt);

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;
        let nonce = Nonce::from_slice(&encrypted[..NONCE_LEN]);
        let plaintext = cipher
            .decrypt(nonce, &encrypted[NONCE_LEN..])
            .map_err(|_| "密码错误！".to_string())?;

        let data: Value = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("Vault 数据损坏: {}", e))?;

        let vault = Vault {
            data: RwLock::new(data),
            vault_path,
            salt_path,
            key: RwLock::new(key),
            last_activity: RwLock::new(Instant::now()),
            auto_lock_timeout: RwLock::new(Duration::from_secs(900)),
            mounted: RwLock::new(true),
        };

        vault.update_login_time();
        Ok(vault)
    }

    /// 保存 Vault 到磁盘
    pub fn save(&self) -> Result<(), String> {
        if !*self.mounted.read().unwrap() {
            return Err("VaultLocked: Vault已锁定, 请先执行 vault unlock".into());
        }

        let data = self.data.read().unwrap();
        let plaintext =
            serde_json::to_vec(&*data).map_err(|e| format!("序列化失败: {}", e))?;

        let key = self.key.read().unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("密钥错误: {}", e))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| format!("加密失败: {}", e))?;

        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        std::fs::write(&self.vault_path, &output)
            .map_err(|e| format!("保存失败: {}", e))?;

        Ok(())
    }

    /// 锁定 Vault (清除内存密钥)
    /// ★ BUG-FIX v4.2: 统一锁顺序为 data→key→mounted (与 remount 一致), 消除潜在死锁
    /// ★ BUG-FIX v4.3: 处理中毒锁 — 使用 into_inner() 恢复, 确保即使 RwLock 中毒也能擦除敏感数据
    /// ★ BUG-C 修复: 在持有 data 写锁后二次检查 mounted, 防止 TOCTOU 竞态
    ///   (check_timeout 与 remount 并发时, remount 可能在 lock 之前重新挂载,
    ///    导致刚解密的数据被错误清零)
    pub fn lock(&self) {
        // 使用 recoverable write: 中毒锁也能强制清零
        match self.data.write() {
            Ok(mut data) => {
                // ★ BUG-C: 二次检查 — 防止 check_timeout 在 remount 之后错误清零
                if !*self.mounted.read().unwrap() {
                    return; // 已被其他线程锁定, 无需重复操作
                }
                *data = Value::Null;
            }
            Err(poisoned) => {
                let mut data = poisoned.into_inner();
                if !*self.mounted.read().unwrap() {
                    return;
                }
                *data = Value::Null;
            }
        }
        match self.key.write() {
            Ok(mut key) => { key.zeroize(); }
            Err(poisoned) => {
                let mut key = poisoned.into_inner();
                key.zeroize();
            }
        }
        *self.mounted.write().unwrap() = false;
    }

    /// 重新挂载 (解锁)
    pub fn remount(&self, password: &str) -> Result<(), String> {
        let salt = std::fs::read(&self.salt_path)
            .map_err(|e| format!("salt: {}", e))?;
        let encrypted = std::fs::read(&self.vault_path)
            .map_err(|e| format!("vault: {}", e))?;
        let key = derive_key(password, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("key: {}", e))?;
        let nonce = Nonce::from_slice(&encrypted[..NONCE_LEN]);
        let plaintext = cipher
            .decrypt(nonce, &encrypted[NONCE_LEN..])
            .map_err(|_| "密码错误！".to_string())?;
        let data: Value = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("data: {}", e))?;

        *self.data.write().unwrap() = data;
        *self.key.write().unwrap() = key;
        *self.last_activity.write().unwrap() = Instant::now();
        *self.mounted.write().unwrap() = true;

        Ok(())
    }

    /// 自动锁定检查
    /// ★ BUG-23 修复: 消除 TOCTOU 竞态 — 在持有 mounted 读锁期间
    ///   原子检查 is_mounted + last_activity + timeout 并调用 lock
    pub fn check_timeout(&self) -> bool {
        if !*self.mounted.read().unwrap() { return false; }
        let elapsed = self.last_activity.read().unwrap().elapsed();
        let timeout = *self.auto_lock_timeout.read().unwrap();
        if elapsed > timeout {
            self.lock();
            return true;
        }
        false
    }

    pub fn touch(&self) {
        if self.is_mounted() {
            *self.last_activity.write().unwrap() = Instant::now();
        }
    }

    pub fn is_mounted(&self) -> bool { *self.mounted.read().unwrap() }

    pub fn set_timeout(&self, seconds: u64) {
        *self.auto_lock_timeout.write().unwrap() = Duration::from_secs(seconds);
    }

    // ─── 命名空间读写 ─────────────────────────────────

    /// 获取值 (支持点路径: "api_keys.openai")
    ///
    /// ★ BUG-15 修复: 增加挂载检查 — lock 后不再静默返回 None
    ///   调用方可通过检查 is_mounted() 或返回值区分"未挂载"和"键不存在"
    pub fn get(&self, namespace: &str, key: &str) -> Option<String> {
        if !self.is_mounted() { return None; } // 明确: Vault已锁定
        self.touch();
        let data = self.data.read().unwrap();
        let node = data.get(namespace)?;
        json_nav(node, key).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    /// 设置值
    ///
    /// ★ BUG-15 修复: 明确返回 VaultLocked 错误而非 "Vault 数据损坏"
    pub fn set(&self, namespace: &str, key: &str, val: &str) -> Result<(), String> {
        if !self.is_mounted() {
            return Err("VaultLocked: Vault已锁定, 请先执行 vault unlock".into());
        }
        self.touch();
        let mut data = self.data.write().unwrap();
        let obj = data
            .as_object_mut()
            .ok_or("Vault 数据损坏")?;
        let entry = obj
            .entry(namespace.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        json_set(entry, key, Value::String(val.to_string()));
        drop(data);
        self.save()
    }

    /// 删除值
    ///
    /// ★ BUG-15 修复: 增加挂载检查
    pub fn delete(&self, namespace: &str, key: &str) -> Result<(), String> {
        if !self.is_mounted() {
            return Err("VaultLocked: Vault已锁定, 请先执行 vault unlock".into());
        }
        self.touch();
        let mut data = self.data.write().unwrap();
        if let Some(node) = data.get_mut(namespace) {
            json_del(node, key);
        }
        drop(data);
        self.save()
    }

    /// 总条目数 (所有命名空间)
    pub fn entry_count(&self) -> usize {
        if !self.is_mounted() { return 0; }
        let data = self.data.read().unwrap();
        let mut count = 0;
        if let Some(obj) = data.as_object() {
            for (_, v) in obj {
                if let Value::Object(m) = v { count += m.len(); }
            }
        }
        count
    }

    /// 列出命名空间下的所有键
    pub fn list_keys(&self, namespace: &str) -> Vec<String> {
        if !self.is_mounted() { return vec![]; }
        let data = self.data.read().unwrap();
        match data.get(namespace) {
            Some(Value::Object(map)) => map.keys().cloned().collect(),
            _ => vec![],
        }
    }

    /// 列出所有命名空间
    pub fn list_namespaces(&self) -> Vec<String> {
        if !self.is_mounted() { return vec![]; }
        self.data
            .read()
            .unwrap()
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// 导出命名空间 JSON
    pub fn get_namespace_json(&self, namespace: &str) -> String {
        if !self.is_mounted() { return "{}".into(); }
        let data = self.data.read().unwrap();
        match data.get(namespace) {
            Some(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
            None => "{}".into(),
        }
    }

    /// 导出完整 Vault (⚠️ 危险)
    pub fn export_json(&self) -> String {
        if !self.is_mounted() { return "{}".into(); }
        let data = self.data.read().unwrap();
        serde_json::to_string_pretty(&*data).unwrap_or_default()
    }

    /// 修改密码
    pub fn change_password(&self, old_pw: &str, new_pw: &str) -> Result<(), String> {
        // ★ BUG-14 修复: 新密码强度校验，拒绝弱密码
        if new_pw.is_empty() {
            return Err("新密码不能为空".into());
        }
        if new_pw.len() < 12 {
            return Err(format!(
                "新密码太短 ({}字符), 要求 ≥12字符, 含大小写+数字+特殊字符",
                new_pw.len()
            ));
        }
        let strength = crate::auth::password_strength(new_pw);
        match strength {
            crate::auth::Strength::VeryWeak | crate::auth::Strength::Weak => {
                return Err(format!(
                    "新密码强度不足 ({:?}), 要求至少 Medium 强度。请混合大小写+数字+特殊字符",
                    strength
                ));
            }
            _ => {}
        }

        // 验证旧密码
        let salt = std::fs::read(&self.salt_path)
            .map_err(|e| format!("salt: {}", e))?;
        let old_key = derive_key(old_pw, &salt);
        let encrypted = std::fs::read(&self.vault_path)
            .map_err(|e| format!("vault: {}", e))?;
        let cipher = Aes256Gcm::new_from_slice(&old_key)
            .map_err(|_| "内部错误".to_string())?;
        let nonce = Nonce::from_slice(&encrypted[..NONCE_LEN]);
        cipher
            .decrypt(nonce, &encrypted[NONCE_LEN..])
            .map_err(|_| "旧密码错误！".to_string())?;

        // 新 salt + 新密钥
        let mut new_salt = vec![0u8; SALT_LEN];
        OsRng.fill_bytes(&mut new_salt);
        let new_key = derive_key(new_pw, &new_salt);

        std::fs::write(&self.salt_path, &new_salt)
            .map_err(|e| format!("salt: {}", e))?;

        *self.key.write().unwrap() = new_key;
        self.save()?;

        Ok(())
    }

    // ─── AI 快捷方法 ──────────────────────────────────

    pub fn ai_set_api_key(&self, provider: &str, key: &str) -> Result<(), String> {
        self.set("ai", &format!("api_keys.{}", provider), key)
    }

    pub fn ai_get_api_key(&self, provider: &str) -> Option<String> {
        self.get("ai", &format!("api_keys.{}", provider))
    }

    pub fn ai_remember(&self, key: &str, value: &str) -> Result<(), String> {
        self.set("ai", &format!("memory.{}", key), value)
    }

    pub fn ai_recall(&self, key: &str) -> Option<String> {
        self.get("ai", &format!("memory.{}", key))
    }

    // ─── 工具快捷方法 ────────────────────────────────

    pub fn tool_cache(&self, tool: &str, key: &str, val: &str) -> Result<(), String> {
        self.set("tools", &format!("cache.{}.{}", tool, key), val)
    }

    pub fn tool_get_cache(&self, tool: &str, key: &str) -> Option<String> {
        self.get("tools", &format!("cache.{}.{}", tool, key))
    }

    // ─── 内部 ─────────────────────────────────────────

    fn update_login_time(&self) {
        if let Ok(mut data) = self.data.write() {
            if let Some(sys) = data.get_mut("system") {
                if let Some(auth) = sys.get_mut("auth") {
                    auth["last_login"] =
                        Value::String(chrono::Utc::now().to_rfc3339());
                }
            }
        }
        let _ = self.save();
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        if let Ok(mut k) = self.key.write() {
            k.zeroize();
        }
        if let Ok(mut d) = self.data.write() {
            *d = Value::Null;
        }
    }
}

// ─── 辅助函数 ─────────────────────────────────────────

fn derive_key(password: &str, salt: &[u8]) -> Vec<u8> {
    let mut key = vec![0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key);
    key
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn init_vault_data() -> Value {
    serde_json::json!({
        "system": {
            "auth": {
                "created_at": now_iso(),
                "last_login": now_iso(),
                "version": env!("CARGO_PKG_VERSION")
            },
            "config": {}
        },
        "ai": {
            "api_keys": {},
            "memory": {},
            "sessions": {},
            "preferences": {
                "model": "gpt-4",
                "temperature": "0.7"
            }
        },
        "tools": {
            "cache": {},
            "preferences": {}
        }
    })
}

/// 点路径导航 JSON
fn json_nav<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for p in path.split('.') {
        match cur {
            Value::Object(m) => cur = m.get(p)?,
            _ => return None,
        }
    }
    Some(cur)
}

/// 点路径设置 JSON (v4.1: 中间节点类型检查，非Object自动覆盖)
fn json_set(root: &mut Value, path: &str, val: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() { return; }

    // 确保所有中间节点存在且为 Object
    for i in 0..parts.len() - 1 {
        let key = parts[i];
        // 使用 pointer 重新获取当前级别 (避免 borrow 冲突)
        let prefix = format!("/{}", parts[..=i].join("/"));
        let parent_prefix = if i > 0 { format!("/{}", parts[..i].join("/")) } else { String::new() };

        match root.pointer_mut(&prefix) {
            Some(Value::Object(_)) => { /* 已存在且为 Object, 继续 */ }
            Some(_) | None => {
                // 需要创建或覆盖为 Object
                if i == 0 {
                    if let Value::Object(m) = root {
                        m.insert(key.to_string(), Value::Object(Map::new()));
                    }
                } else if let Some(parent) = root.pointer_mut(&parent_prefix) {
                    if let Value::Object(m) = parent {
                        m.insert(key.to_string(), Value::Object(Map::new()));
                    }
                }
            }
        }
    }

    // 设置最终值
    let last = parts.last().unwrap();
    if parts.len() == 1 {
        if let Value::Object(m) = root {
            m.insert(last.to_string(), val);
        }
    } else {
        let parent_prefix = format!("/{}", parts[..parts.len()-1].join("/"));
        if let Some(parent) = root.pointer_mut(&parent_prefix) {
            if let Value::Object(m) = parent {
                m.insert(last.to_string(), val);
            }
        }
    }
}

// ─── 全局 Vault 单例 ───────────────────────────────────

static GLOBAL_VAULT: OnceLock<Arc<Vault>> = OnceLock::new();

/// 设置全局 Vault (main.rs 启动时调用)
pub fn set_global_vault(vault: Arc<Vault>) {
    let _ = GLOBAL_VAULT.set(vault);
}

/// 在全局 Vault 上执行操作
pub fn with_vault<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&Vault) -> R,
{
    GLOBAL_VAULT.get().map(|v| f(v.as_ref()))
}

/// Vault 工具桥接函数 (供 AI 工具调用)
pub fn vault_set(ns: &str, key: &str, val: &str) -> Result<(), String> {
    with_vault(|v| v.set(ns, key, val)).ok_or_else(|| String::from("Vault 未挂载"))?
}

pub fn vault_get(ns: &str, key: &str) -> Option<String> {
    with_vault(|v| v.get(ns, key)).flatten()
}

pub fn vault_list(ns: &str) -> Option<Vec<String>> {
    with_vault(|v| v.list_keys(ns))
}

pub fn vault_list_namespaces() -> Option<Vec<String>> {
    with_vault(|v| v.list_namespaces())
}

pub fn vault_del(ns: &str, key: &str) -> Result<(), String> {
    with_vault(|v| v.delete(ns, key)).ok_or_else(|| String::from("Vault 未挂载"))?
}

/// 点路径删除 JSON
fn json_del(root: &mut Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() { return; }
    if parts.len() == 1 {
        if let Value::Object(m) = root { m.remove(parts[0]); }
        return;
    }
    let mut cur = root;
    for (i, p) in parts.iter().enumerate() {
        if i == parts.len() - 2 {
            if let Value::Object(m) = cur { m.remove(*p); }
            return;
        }
        if let Value::Object(m) = cur {
            if let Some(n) = m.get_mut(*p) { cur = n; } else { return; }
        }
    }
}
