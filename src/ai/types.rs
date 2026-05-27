// === AI 核心类型定义 ===
// AiPermLevel + StreamMsg/StreamKind + 全局通道 + AiResponse/ToolCall

use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};

// ════════════════════════════════════════════════════════
// AI 权限系统 v4.1 — 防 AI/脚本/插件/sys 绕过
// ════════════════════════════════════════════════════════

/// AI 工具权限等级 — 数字越大越危险
/// 只能通过 TUI `perm <N>` 命令提升，AI/脚本/插件/sys 无法自升级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum AiPermLevel {
    /// 0: 只读信息查询 (system_info, pwd, ls, stat, help-like)
    Safe = 0,
    /// 1: 网络探测 (scan, dns, whois, ping, ssl, subdomain, traceroute, geoip)
    Net = 1,
    /// 2: Web 请求 (http_get/post, cors, headers, clickjack, cookies)
    Web = 2,
    /// 3: 文件读写 + 代码生成 (read/write/delete + payload/shell/webshell)
    Files = 3,
    /// 4: 系统级 (process_list, disk_usage, exec-like, net interface)
    System = 4,
    /// 5: 最高危险 (plugin_load, sysload, script_run, vault write, compile_rust)
    Dangerous = 5,
}

impl AiPermLevel {
    pub fn from_i32(n: i32) -> Self {
        match n {
            0 => Self::Safe,
            1 => Self::Net,
            2 => Self::Web,
            3 => Self::Files,
            4 => Self::System,
            5 => Self::Dangerous,
            _ if n < 0 => Self::Safe,
            _ => Self::Dangerous,
        }
    }

    pub fn as_i32(self) -> i32 { self as i32 }

    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "SAFE(0)",
            Self::Net => "NET(1)",
            Self::Web => "WEB(2)",
            Self::Files => "FILES(3)",
            Self::System => "SYSTEM(4)",
            Self::Dangerous => "DANGER(5)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Safe => "只读信息查询: system_info/pwd/ls/stat/help-like",
            Self::Net => "+网络探测: scan/dns/whois/ping/ssl/subdomain/traceroute/geoip",
            Self::Web => "+Web请求: http_get/post/cors/headers/clickjack/cookies/robots",
            Self::Files => "+文件读写+代码生成: read/write/delete + payload/shell/webshell",
            Self::System => "+系统级: process_list/disk_usage/net_iface + Vault读",
            Self::Dangerous => "+最高危险: plugin_load/sysload/script_run/vault写/compile_rust/exec_cmd",
        }
    }
}

impl std::fmt::Display for AiPermLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ── 流式消息 (实时输出) ──
#[derive(Debug, Clone)]
pub enum StreamKind {
    Api,      // API 调用信息 (蓝色)
    Tool,     // 工具调用 (青色)
    Result_,  // 工具返回 (暗绿)
    Reply,    // AI 最终回复 (白色)
    Error,    // 错误 (红色)
    Retry,    // 重连/重试进度 (黄色)
    Info,     // AI直出信息 (绿色) — 始终显示, 不受流模式过滤
}

#[derive(Debug, Clone)]
pub struct StreamMsg {
    pub kind: StreamKind,
    pub content: String,
}

// ── API 调用 ──
pub struct AiResponse {
    pub content: String,
    pub reasoning_content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// ── 全局终端输出 — 供 get_terminal_output 工具读取 ──
static TERMINAL_OUTPUT: OnceLock<Arc<RwLock<Vec<String>>>> = OnceLock::new();

pub fn set_terminal_output(out: Arc<RwLock<Vec<String>>>) {
    let _ = TERMINAL_OUTPUT.set(out);
}

pub(crate) fn get_terminal_output_text() -> String {
    match TERMINAL_OUTPUT.get() {
        Some(arc) => match arc.read() {
            Ok(lines) => lines.join("\n"),
            Err(_) => "(输出锁被占用)".to_string(),
        },
        None => "(输出不可用)".to_string(),
    }
}

// ── AI直出消息通道 — ai_message 工具绕过LLM直接输出到TUI ──
static AI_MESSAGE_TX: OnceLock<Mutex<mpsc::Sender<String>>> = OnceLock::new();

pub fn set_ai_message_tx(tx: mpsc::Sender<String>) {
    let _ = AI_MESSAGE_TX.set(Mutex::new(tx));
}

pub fn send_ai_message(text: &str) {
    if let Some(lock) = AI_MESSAGE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(text.to_string());
        }
    }
}

// ── AI裸消息通道 — ai_message_raw 工具绕过前缀，直接输出原始文本 ──
static AI_MESSAGE_RAW_TX: OnceLock<Mutex<mpsc::Sender<String>>> = OnceLock::new();

pub fn set_ai_message_raw_tx(tx: mpsc::Sender<String>) {
    let _ = AI_MESSAGE_RAW_TX.set(Mutex::new(tx));
}

pub fn send_ai_message_raw(text: &str) {
    if let Some(lock) = AI_MESSAGE_RAW_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(text.to_string());
        }
    }
}
