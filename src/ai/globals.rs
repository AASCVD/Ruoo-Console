// === 全局状态变量 + 访问器 (v9.0 统一) ===
// GLOBAL_CMD_REGISTRY, GLOBAL_PLUGIN_REGISTRY,
// AI_PERM_LEVEL, CLEAR_HISTORY_FLAG, TOOL_ABORT_FLAG
//
// v9.0: GLOBAL_PLUGIN_MGR 已迁移到 script::plugin_mgr 统一单例
//       所有插件操作通过 script::get_global_plugin_manager() / global_plugin_manager()

use std::sync::{Arc, OnceLock};
use crate::ai::types::AiPermLevel;

// ── 全局命令注册表 — 供 AI plugin_load/help 使用 ──
pub(crate) static GLOBAL_CMD_REGISTRY: std::sync::Mutex<Option<crate::plugin::CommandRegistry>> =
    std::sync::Mutex::new(None);

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

// ── v9.0: 插件管理器统一访问器 (指向 script::plugin_mgr 全局单例) ──

/// 获取全局 PluginManager 的 Arc 引用
pub fn global_plugin_mgr() -> Option<Arc<std::sync::Mutex<crate::script::PluginManager>>> {
    crate::script::get_global_plugin_manager()
}

/// 获取全局 PluginManager (必须已初始化)
pub fn require_plugin_mgr() -> Arc<std::sync::Mutex<crate::script::PluginManager>> {
    crate::script::global_plugin_manager()
}

/// 获取全局插件管理器引用 (只读)
pub fn with_global_plugin_mgr<F, R>(f: F) -> Option<R>
where F: FnOnce(&crate::script::PluginManager) -> R
{
    let mgr = crate::script::get_global_plugin_manager()?;
    let guard = mgr.lock().ok()?;
    Some(f(&*guard))
}

/// 获取全局插件管理器可变引用 (供 commands.rs 调用)
pub fn with_global_plugin_mgr_mut<F, R>(f: F) -> Option<R>
where F: FnOnce(&mut crate::script::PluginManager) -> R
{
    let mgr = crate::script::get_global_plugin_manager()?;
    let mut guard = mgr.lock().ok()?;
    Some(f(&mut *guard))
}

/// 向后兼容: 旧代码中返回 MutexGuard<Option<PluginManager>> 的地方
/// 已废弃 — 新代码应使用 global_plugin_mgr() + lock()
#[deprecated(note = "使用 global_plugin_mgr() + lock() 替代")]
pub(crate) fn _global_plugin_mgr_legacy() -> std::sync::MutexGuard<'static, Option<crate::script::PluginManager>> {
    panic!("已废弃: 请使用 global_plugin_mgr() + lock()")
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
