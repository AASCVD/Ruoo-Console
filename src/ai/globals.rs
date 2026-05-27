// === 全局状态变量 + 访问器 ===
// GLOBAL_PLUGIN_MGR, GLOBAL_CMD_REGISTRY, GLOBAL_PLUGIN_REGISTRY,
// AI_PERM_LEVEL, CLEAR_HISTORY_FLAG, TOOL_ABORT_FLAG

use std::sync::{Arc, OnceLock};
use crate::ai::types::AiPermLevel;

// ── 全局持久插件管理器 — 供 AI plugin_load/plugin_call 使用 ──
pub(crate) static GLOBAL_PLUGIN_MGR: std::sync::Mutex<Option<crate::script::PluginManager>> =
    std::sync::Mutex::new(None);
pub(crate) static GLOBAL_CMD_REGISTRY: std::sync::Mutex<Option<crate::plugin::CommandRegistry>> =
    std::sync::Mutex::new(None);

pub(crate) fn global_plugin_mgr() -> std::sync::MutexGuard<'static, Option<crate::script::PluginManager>> {
    GLOBAL_PLUGIN_MGR.lock().unwrap_or_else(|e| e.into_inner())
}
pub(crate) fn global_cmd_registry() -> std::sync::MutexGuard<'static, Option<crate::plugin::CommandRegistry>> {
    GLOBAL_CMD_REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// 获取全局插件命令注册表引用 (供 TUI help/tab-complete 读取)
pub fn with_global_cmd_registry<F, R>(f: F) -> Option<R>
where F: FnOnce(&crate::plugin::CommandRegistry) -> R
{
    let guard = GLOBAL_CMD_REGISTRY.lock().ok()?;
    guard.as_ref().map(f)
}

/// 获取全局插件管理器引用
pub fn with_global_plugin_mgr<F, R>(f: F) -> Option<R>
where F: FnOnce(&crate::script::PluginManager) -> R
{
    let guard = GLOBAL_PLUGIN_MGR.lock().ok()?;
    guard.as_ref().map(f)
}

/// 获取全局插件管理器可变引用 (供 commands.rs 调用)
pub fn with_global_plugin_mgr_mut<F, R>(f: F) -> Option<R>
where F: FnOnce(&mut crate::script::PluginManager) -> R
{
    let mut guard = GLOBAL_PLUGIN_MGR.lock().ok()?;
    guard.as_mut().map(f)
}

// ── 全局插件注册表引用 — 供 AI plugin_reg/unreg/enable/disable 使用 ──
static GLOBAL_PLUGIN_REGISTRY: OnceLock<std::sync::Mutex<Option<Arc<crate::plugin_registry::PluginRegistry>>>> = OnceLock::new();

pub(crate) fn global_plugin_registry() -> std::sync::MutexGuard<'static, Option<Arc<crate::plugin_registry::PluginRegistry>>> {
    GLOBAL_PLUGIN_REGISTRY.get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn set_global_plugin_registry(reg: Arc<crate::plugin_registry::PluginRegistry>) {
    let mut guard = global_plugin_registry();
    *guard = Some(reg);
}

pub fn with_app_plugin_registry<F, R>(f: F) -> Option<R>
where F: FnOnce(&crate::plugin_registry::PluginRegistry) -> R
{
    let guard = GLOBAL_PLUGIN_REGISTRY.get()?.lock().ok()?;
    guard.as_ref().map(|reg| f(reg))
}

// ── 全局 AI 权限等级 — 供 execute_tool() 读取 ──
static AI_PERM_LEVEL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(5);

pub(crate) fn set_ai_perm_level(level: AiPermLevel) {
    AI_PERM_LEVEL.store(level.as_i32(), std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn get_ai_perm_level() -> AiPermLevel {
    AiPermLevel::from_i32(AI_PERM_LEVEL.load(std::sync::atomic::Ordering::Relaxed))
}

// ── 清理请求标志 — execute_tool 设置 → send() 处理 ──
pub(crate) static CLEAR_HISTORY_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ── 工具强制中断标志 — abort_tool 设置 → 工具执行循环检测 → 丢弃未执行工具 ──
pub(crate) static TOOL_ABORT_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 公开接口: 触发工具强制中断 (供 process_cmd 的 abort 命令调用)
pub fn trigger_tool_abort() -> String {
    TOOL_ABORT_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
    "[+] 工具强制中断信号已发送\n  [*] 当前正在执行的工具将被线程超时(120s)自动回收\n  [*] 后续排队的工具调用已全部跳过\n  [!] 如果当前工具卡在系统调用，最多等待120秒".to_string()
}
