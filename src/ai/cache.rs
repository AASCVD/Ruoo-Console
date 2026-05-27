// === 工具结果缓存 ===
// 双层缓存: Vault 持久化 + 内存 LRU 回退

use std::collections::HashMap;

pub(crate) fn cached_tool(tool_name: &str, cache_key: &str, fetch: impl FnOnce() -> String) -> String {
    let ttl_key = format!("cache.{}.{}", tool_name, cache_key);
    let ttl_val_key = format!("cache.{}.{}.ts", tool_name, cache_key);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 第1层: Vault 持久化缓存 (跨会话)
    if let Some(cached) = crate::vault::vault_get("tools", &ttl_key) {
        if let Some(ts_str) = crate::vault::vault_get("tools", &ttl_val_key) {
            if let Ok(ts) = ts_str.parse::<u64>() {
                if now.saturating_sub(ts) < 3600 {
                    return format!("[缓存] {}", cached);
                }
            }
        }
    }

    // 第2层: 内存缓存回退 (Vault 锁定/未挂载时仍有效)
    {
        let mem = get_memory_cache().lock().unwrap();
        if let Some((ts, val)) = mem.get(&ttl_key) {
            if now.saturating_sub(*ts) < 3600 {
                return format!("[缓存] {}", val);
            }
        }
        drop(mem);
    }

    // 执行实际查询
    let result = fetch();

    // 写入双层缓存
    let _ = crate::vault::vault_set("tools", &ttl_key, &result);
    let _ = crate::vault::vault_set("tools", &ttl_val_key, &now.to_string());
    {
        let mut mem = get_memory_cache().lock().unwrap();
        if mem.len() > 100 {
            mem.clear();
        }
        mem.insert(ttl_key, (now, result.clone()));
    }

    result
}

// 内存缓存 (线程安全)
static MEMORY_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, (u64, String)>>> = std::sync::OnceLock::new();

fn get_memory_cache() -> &'static std::sync::Mutex<HashMap<String, (u64, String)>> {
    MEMORY_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 清除所有工具缓存 (Vault + 内存)
pub fn clear_all_cache() -> String {
    let mut count = 0usize;
    if let Ok(mut mem) = get_memory_cache().lock() {
        count += mem.len();
        mem.clear();
    }
    if let Some(keys) = crate::vault::vault_list("tools") {
        for key in &keys {
            if key.starts_with("cache.") {
                let _ = crate::vault::vault_del("tools", key);
                count += 1;
            }
        }
    }
    format!("[+] 已清除 {} 条缓存 (内存 + Vault)", count)
}
