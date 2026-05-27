// ============================================================
// RUOO-ARSENAL v4.0 — DeepSeek AI 助手模块
// 真实工具调用 — 所有 execute_tool 调用 tools.rs 真实实现
// ============================================================

// ── 子模块 ──
#[macro_use] pub mod macros;      // t0..t4 工具定义宏
pub mod types;        // AiPermLevel, StreamMsg, StreamKind, AiResponse, ToolCall, 全局通道
pub mod misc;         // parse_ports, is_code_tool
pub mod markdown;     // strip_markdown 系列
pub mod globals;      // 全局静态变量 (GLOBAL_PLUGIN_MGR, AI_PERM_LEVEL, TOOL_ABORT_FLAG 等)
pub mod cache;        // 工具结果双层缓存
pub mod permissions;  // 权限检查 + Shell注入防护
pub mod deepseek;     // DeepSeek API 调用 (call_deepseek, sanitize_history)
pub mod prompt;       // build_system_prompt + build_tools + build_memory_context
pub mod session;      // AiSession 会话管理
pub mod executor;     // execute_tool 工具调度

// ── 重导出 — 保持向后兼容 ──
pub use types::{AiPermLevel, StreamMsg, StreamKind};
pub use types::{set_terminal_output, send_ai_message, send_ai_message_raw, set_ai_message_tx, set_ai_message_raw_tx};
pub use session::AiSession;
pub use globals::{trigger_tool_abort, with_global_cmd_registry, with_global_plugin_mgr_mut};
pub use globals::{set_global_plugin_registry, with_app_plugin_registry};
pub use cache::clear_all_cache;
pub use permissions::validate_shell_command;
pub(crate) use globals::{GLOBAL_PLUGIN_MGR, GLOBAL_CMD_REGISTRY};
