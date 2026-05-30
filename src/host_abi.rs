// ============================================================
// RUOO-CONSOLE v4.1 — 宿主 ABI 模块 (Host ABI)
//
// HostAPI 函数指针表 — 通过 ruoo_plugin_init 传递给插件
// 插件无需依赖 EXE 导出表, 直接通过指针调用宿主服务
//
// #[no_mangle] C ABI 导出:
//   - ruoo_message_* → 在 message_broker.rs 中定义
//   - ruoo_progress_* → 在 plugin_progress.rs 中定义
//   - ruoo_vault_*    → 在此模块定义 (简单KV)
//   - ruoo_interplugin_* → 通过 message_broker 转发
//
// 模式检测:
//   - 主进程(TUI): 函数操作真实的全局状态
//   - 子进程(--plugin-exec): 函数静默无操作
//
// Zero-Day | 2025修复
// ============================================================

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use parking_lot::Mutex;

// ═══════════════════════════════════════════════════════
// 子进程模式标志
// ═══════════════════════════════════════════════════════

static SUBPROCESS_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_subprocess_mode() {
    SUBPROCESS_MODE.store(true, Ordering::Release);
}

pub fn is_subprocess() -> bool {
    SUBPROCESS_MODE.load(Ordering::Acquire)
}

/// 子进程模式: 将进度/消息写入stderr供父进程转发到TUI
/// 格式: [RPP:create|plugin|id|label|total] — forward_stderr_line_to_tui 解析
pub fn subprocess_stderr_write(tag: &str, payload: &str) {
    if is_subprocess() {
        eprintln!("[{}|{}]", tag, payload);
    }
}

// ═══════════════════════════════════════════════════════
// HostAPI 函数指针表
// ═══════════════════════════════════════════════════════

#[repr(C)]
pub struct HostAPI {
    pub version: u32,
    pub progress_create: unsafe extern "C" fn(
        plugin: *const c_char, id: *const c_char,
        label: *const c_char, total: u64,
    ) -> *mut c_char,
    pub progress_update: unsafe extern "C" fn(
        handle: *const c_char, current: u64, msg: *const c_char,
    ),
    pub progress_finish: unsafe extern "C" fn(handle: *const c_char),
    pub progress_set_total: unsafe extern "C" fn(
        handle: *const c_char, total: u64,
    ),
    pub message_push: unsafe extern "C" fn(
        plugin: *const c_char, text: *const c_char,
    ),
    pub message_tagged: unsafe extern "C" fn(
        tag: *const c_char, text: *const c_char,
    ),
    pub message_update: unsafe extern "C" fn(
        tag: *const c_char, text: *const c_char,
    ),
    pub message_remove: unsafe extern "C" fn(tag: *const c_char),
    pub vault_store: unsafe extern "C" fn(
        key: *const c_char, value: *const c_char,
    ) -> i32,
    pub vault_load: unsafe extern "C" fn(
        key: *const c_char,
    ) -> *mut c_char,
    pub vault_delete: unsafe extern "C" fn(key: *const c_char) -> i32,
    pub vault_list: unsafe extern "C" fn() -> *mut c_char,
    pub interplugin_send: unsafe extern "C" fn(
        from: *const c_char, to: *const c_char,
        msg_type: *const c_char, payload: *const c_char,
    ) -> i32,
    pub interplugin_broadcast: unsafe extern "C" fn(
        from: *const c_char, msg_type: *const c_char, payload: *const c_char,
    ) -> i32,
}

pub fn get_host_api() -> &'static HostAPI {
    &HOST_API_VTABLE
}

static HOST_API_VTABLE: HostAPI = HostAPI {
    version: 1,
    progress_create: host_progress_create,
    progress_update: host_progress_update,
    progress_finish: host_progress_finish,
    progress_set_total: host_progress_set_total,
    message_push: host_message_push,
    message_tagged: host_message_tagged,
    message_update: host_message_update,
    message_remove: host_message_remove,
    vault_store: host_vault_store,
    vault_load: host_vault_load,
    vault_delete: host_vault_delete,
    vault_list: host_vault_list,
    interplugin_send: host_interplugin_send,
    interplugin_broadcast: host_interplugin_broadcast,
};

// ═══════════════════════════════════════════════════════
// 辅助
// ═══════════════════════════════════════════════════════

unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    CStr::from_ptr(ptr).to_str().unwrap_or("")
}

unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() { return String::new(); }
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

fn str_to_cstring(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => CString::new("").unwrap().into_raw(),
    }
}

// ═══════════════════════════════════════════════════════
// 进度条 — 委托给 plugin_progress 全局管理器
// ═══════════════════════════════════════════════════════

unsafe extern "C" fn host_progress_create(
    plugin: *const c_char, id: *const c_char,
    label: *const c_char, total: u64,
) -> *mut c_char {
    if is_subprocess() {
        let pn = cstr_to_str(plugin);
        let bid = cstr_to_str(id);
        let bl = cstr_to_str(label);
        let handle = format!("{}::{}", pn, bid);
        subprocess_stderr_write("RPP:create", &format!("{}|{}|{}|{}", pn, bid, bl, total));
        return str_to_cstring(&handle);
    }
    let pn = cstr_to_str(plugin);
    let bid = cstr_to_str(id);
    let bl = cstr_to_str(label);
    let handle = format!("{}::{}", pn, bid);
    if let Some(ref mgr) = crate::plugin_progress::global_progress_manager() {
        mgr.create(pn, bid, bl, total as usize);
    }
    str_to_cstring(&handle)
}

unsafe extern "C" fn host_progress_update(
    handle: *const c_char, current: u64, msg: *const c_char,
) {
    if is_subprocess() {
        subprocess_stderr_write("RPP:update", &format!("{}|{}|{}", cstr_to_str(handle), current, cstr_to_str(msg)));
        return;
    }
    if let Some(ref mgr) = crate::plugin_progress::global_progress_manager() {
        let _ = mgr.update(cstr_to_str(handle), current as usize, cstr_to_str(msg));
    }
}

unsafe extern "C" fn host_progress_finish(handle: *const c_char) {
    if is_subprocess() {
        subprocess_stderr_write("RPP:finish", cstr_to_str(handle));
        return;
    }
    if let Some(ref mgr) = crate::plugin_progress::global_progress_manager() {
        let _ = mgr.finish(cstr_to_str(handle));
    }
}

unsafe extern "C" fn host_progress_set_total(handle: *const c_char, total: u64) {
    if is_subprocess() {
        subprocess_stderr_write("RPP:set_total", &format!("{}|{}", cstr_to_str(handle), total));
        return;
    }
    if let Some(ref mgr) = crate::plugin_progress::global_progress_manager() {
        let _ = mgr.set_total(cstr_to_str(handle), total as usize);
    }
}

// ═══════════════════════════════════════════════════════
// 消息 — 委托给 message_broker 全局代理
// ═══════════════════════════════════════════════════════

unsafe extern "C" fn host_message_push(plugin: *const c_char, text: *const c_char) {
    if is_subprocess() {
        subprocess_stderr_write("RMSG:push", &format!("{}|{}", cstr_to_str(plugin), cstr_to_str(text)));
        return;
    }
    let p = cstr_to_string(plugin);
    let t = cstr_to_string(text);
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit(&p, crate::message_broker::BrokerMsg::Append {
            plugin_name: p.clone(), text: t,
        });
    }
}

unsafe extern "C" fn host_message_tagged(tag: *const c_char, text: *const c_char) {
    if is_subprocess() {
        subprocess_stderr_write("RMSG:tagged", &format!("{}|{}", cstr_to_str(tag), cstr_to_str(text)));
        return;
    }
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit("system", crate::message_broker::BrokerMsg::TaggedAppend {
            tag: cstr_to_string(tag), text: cstr_to_string(text),
        });
    }
}

unsafe extern "C" fn host_message_update(tag: *const c_char, text: *const c_char) {
    if is_subprocess() {
        subprocess_stderr_write("RMSG:update", &format!("{}|{}", cstr_to_str(tag), cstr_to_str(text)));
        return;
    }
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit("system", crate::message_broker::BrokerMsg::Update {
            tag: cstr_to_string(tag), text: cstr_to_string(text),
        });
    }
}

unsafe extern "C" fn host_message_remove(tag: *const c_char) {
    if is_subprocess() {
        subprocess_stderr_write("RMSG:remove", cstr_to_str(tag));
        return;
    }
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit("system", crate::message_broker::BrokerMsg::Remove {
            tag: cstr_to_string(tag),
        });
    }
}

// ═══════════════════════════════════════════════════════
// 插件 KV 存储 (后备于主 Vault)
// ═══════════════════════════════════════════════════════

static PLUGIN_KV: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

unsafe extern "C" fn host_vault_store(key: *const c_char, value: *const c_char) -> i32 {
    if is_subprocess() {
        // ★ v4.1修复: 子进程模式转发到父进程, 不再直接返回-1
        subprocess_stderr_write("VAULT:store", &format!("{}|{}", cstr_to_str(key), cstr_to_str(value)));
        return 0;
    }
    let k = cstr_to_string(key);
    let v = cstr_to_string(value);
    // 持久化到主Vault (vault_set内部已处理未挂载情况)
    let _ = crate::vault::vault_set("plugins", &k, &v);
    // 同时写入内存KV (插件快速访问)
    PLUGIN_KV.lock().insert(k, v);
    0
}

unsafe extern "C" fn host_vault_load(key: *const c_char) -> *mut c_char {
    if is_subprocess() { return std::ptr::null_mut(); }
    let k = cstr_to_string(key);
    if let Some(val) = crate::vault::vault_get("plugins", &k) {
        if !val.is_empty() { return str_to_cstring(&val); }
    }
    if let Some(val) = PLUGIN_KV.lock().get(&k) {
        return str_to_cstring(val);
    }
    std::ptr::null_mut()
}

unsafe extern "C" fn host_vault_delete(key: *const c_char) -> i32 {
    if is_subprocess() {
        // ★ v4.1修复: 子进程模式转发到父进程
        subprocess_stderr_write("VAULT:delete", cstr_to_str(key));
        return 0;
    }
    let k = cstr_to_string(key);
    PLUGIN_KV.lock().remove(&k);
    // 从主Vault中清除 (空值=删除)
    let _ = crate::vault::vault_set("plugins", &k, "");
    0
}

unsafe extern "C" fn host_vault_list() -> *mut c_char {
    if is_subprocess() { return str_to_cstring("[]"); }
    let keys: Vec<String> = PLUGIN_KV.lock().keys().cloned().collect();
    let json = serde_json::to_string(&keys).unwrap_or_else(|_| "[]".into());
    str_to_cstring(&json)
}

// ═══════════════════════════════════════════════════════
// 插件间通信 — 委托给 message_broker
// ═══════════════════════════════════════════════════════

unsafe extern "C" fn host_interplugin_send(
    from: *const c_char, to: *const c_char,
    msg_type: *const c_char, payload: *const c_char,
) -> i32 {
    if is_subprocess() { return -1; }
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit(&cstr_to_string(from), crate::message_broker::BrokerMsg::InterPlugin {
            from_plugin: cstr_to_string(from),
            to_plugin: cstr_to_string(to),
            msg_type: cstr_to_string(msg_type),
            payload: cstr_to_string(payload),
        });
        0
    } else { -1 }
}

unsafe extern "C" fn host_interplugin_broadcast(
    from: *const c_char, msg_type: *const c_char, payload: *const c_char,
) -> i32 {
    if is_subprocess() { return -1; }
    if let Some(ref broker) = crate::message_broker::global_broker() {
        broker.send_with_limit(&cstr_to_string(from), crate::message_broker::BrokerMsg::EventBroadcast {
            event_type: cstr_to_string(msg_type),
            data: cstr_to_string(payload),
            source_plugin: cstr_to_string(from),
        });
        0
    } else { -1 }
}

// ═══════════════════════════════════════════════════════
// #[no_mangle] C ABI 导出 — 出现在 EXE 导出表中
// (通过 build.rs /EXPORT: 链接标志)
// 注: ruoo_message_* 在 message_broker.rs 中定义
//     ruoo_progress_* 在 plugin_progress.rs 中定义
//     此处仅定义 ruoo_vault_* 和 ruoo_interplugin_*
// ═══════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn ruoo_vault_store(key: *const c_char, value: *const c_char) -> i32 {
    host_vault_store(key, value)
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_vault_load(key: *const c_char) -> *mut c_char {
    host_vault_load(key)
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_vault_delete(key: *const c_char) -> i32 {
    host_vault_delete(key)
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_vault_list() -> *mut c_char {
    host_vault_list()
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_interplugin_send(
    from: *const c_char, to: *const c_char,
    msg_type: *const c_char, payload: *const c_char,
) -> i32 {
    host_interplugin_send(from, to, msg_type, payload)
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_interplugin_broadcast(
    from: *const c_char, msg_type: *const c_char, payload: *const c_char,
) -> i32 {
    host_interplugin_broadcast(from, msg_type, payload)
}
