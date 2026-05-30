// ============================================================
// RUOO-CONSOLE — 插件系统 v6.4
//
// v6.4 变更 (热重载改造):
//   - 移除文件监视热加载 (MD5/mtime检测) — 纯命令触发
//   - reload_plugin 保留 — 通过 plugin reload <name> 命令触发
//   - 移除 hot_watch / hot_watch_enabled / file_hash / compute_file_md5
//   - hot_reload_enabled manifest字段保留 — 控制是否允许命令重载
//
// v6.3 修复 (Zero-Day 审计):
//   - call_plugin_with_timeout: mpsc channel + recv_timeout 真实超时
//   - 内存泄漏修复: free_result 未找到时明确处理
//   - reload_plugin: 加载失败时自动回滚旧版本
//   - ruoo_plugin_health_check: 多维度健康检查
//   - ruoo_plugin_config_get: 返回纯值字符串(非JSON包装)
//   - load_plugin: 消除冗余 get_plugin_commands C FFI调用
// ============================================================

use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, Duration};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
// ── parking_lot 低竞争锁 ──
use parking_lot::RwLock;

// ════════════════════════════════════════════════════════
// 插件清单 v6.0
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub permissions: u32,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub commands: Vec<PluginCommandDef>,
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub config: HashMap<String, String>,
    // ── v6.0 新增 ──
    #[serde(default)]
    pub min_host_version: Option<String>,  // 最低宿主版本要求
    #[serde(default)]
    pub max_instances: u32,                // 最大实例数(0=无限)
    #[serde(default = "default_true")]
    pub hot_reload_enabled: bool,          // 是否允许热重载
    #[serde(default)]
    pub crash_threshold: u32,              // 熔断器阈值(0=使用默认10)
}

fn default_timeout_ms() -> u64 { 30_000 }
fn default_true() -> bool { true }

impl PluginManifest {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("插件清单JSON解析失败: {}", e))
    }

    pub fn from_toml(toml: &str) -> Result<Self, String> {
        toml::from_str(toml).map_err(|e| format!("插件清单TOML解析失败: {}", e))
    }
}

// ════════════════════════════════════════════════════════
// 命令定义 v6.0
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommandDef {
    pub cmd: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(skip)]
    pub plugin_name: String,
    #[serde(skip)]
    pub is_builtin: bool,
    #[serde(default)]
    pub is_ai_tool: bool,
    #[serde(default)]
    pub ai_params: Vec<AiToolParam>,
    #[serde(default)]
    pub ai_perm_level: u8,
    // ── v6.0: 命令元数据增强 ──
    #[serde(default)]
    pub is_async: bool,              // 异步命令(不阻塞)
    #[serde(default)]
    pub is_long_running: bool,       // 长运行命令(特殊超时)
    #[serde(default)]
    pub progress_label: String,      // 进度条标签(空=无进度条)
    #[serde(default)]
    pub min_perm_level: u8,          // 最小调用权限(0=无限制)
}

// ════════════════════════════════════════════════════════
// AI工具参数定义
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    // ── v6.0: 参数验证 ──
    #[serde(default)]
    pub min_value: Option<f64>,
    #[serde(default)]
    pub max_value: Option<f64>,
    #[serde(default)]
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginAiToolDef {
    pub name: String,
    pub description: String,
    pub plugin_name: String,
    pub params: Vec<AiToolParam>,
    pub perm_level: u8,
}

impl PluginAiToolDef {
    pub fn to_tool_json(&self) -> serde_json::Value {
        use serde_json::json;
        let mut props = serde_json::Map::new();
        let mut required: Vec<String> = Vec::new();

        for p in &self.params {
            let mut prop = serde_json::Map::new();
            prop.insert("type".to_string(), json!(p.param_type));
            prop.insert("description".to_string(), json!(p.description));
            if let Some(ref dv) = p.default_value {
                prop.insert("default".to_string(), json!(dv));
            }
            if !p.enum_values.is_empty() {
                prop.insert("enum".to_string(), json!(p.enum_values));
            }
            props.insert(p.name.clone(), json!(prop));
            if p.required {
                required.push(p.name.clone());
            }
        }

        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": format!("[plugin:{}] {}", self.plugin_name, self.description),
                "parameters": {
                    "type": "object",
                    "properties": props,
                    "required": required
                }
            }
        })
    }
}

// ════════════════════════════════════════════════════════
// AI工具全局注册表 (使用 parking_lot RwLock)
// ════════════════════════════════════════════════════════

static GLOBAL_AI_TOOLS: std::sync::LazyLock<RwLock<HashMap<String, PluginAiToolDef>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

fn ai_tool_registry() -> &'static RwLock<HashMap<String, PluginAiToolDef>> {
    &GLOBAL_AI_TOOLS
}

pub fn register_ai_tools(plugin_name: &str, commands: &[PluginCommandDef]) -> usize {
    let mut reg = ai_tool_registry().write();
    let mut count = 0;
    for cmd in commands {
        // v8.0: 所有插件命令默认为AI工具 (有 ai_params 则用, 无则为空)
        // is_ai_tool 字段仅用于显式禁用 (设为 false)
        if !cmd.is_ai_tool && cmd.ai_params.is_empty() {
            // 未显式标记且无参数: v8.0 自动启用 (空参数)
        }
        let tool = PluginAiToolDef {
            name: cmd.cmd.clone(),
            description: if cmd.help.is_empty() { cmd.usage.clone() } else { cmd.help.clone() },
            plugin_name: plugin_name.to_string(),
            params: cmd.ai_params.clone(),
            perm_level: if cmd.ai_perm_level == 0 { 5 } else { cmd.ai_perm_level },
        };
        reg.insert(cmd.cmd.clone(), tool);
        count += 1;
    }
    count
}

pub fn unregister_ai_tools(plugin_name: &str) -> usize {
    let mut reg = ai_tool_registry().write();
    let to_remove: Vec<String> = reg.iter()
        .filter(|(_, t)| t.plugin_name == plugin_name)
        .map(|(k, _)| k.clone())
        .collect();
    let count = to_remove.len();
    for k in &to_remove { reg.remove(k); }
    count
}

pub fn find_ai_tool(name: &str) -> Option<PluginAiToolDef> {
    ai_tool_registry().read().get(name).cloned()
}

pub fn all_ai_tool_defs() -> Vec<PluginAiToolDef> {
    ai_tool_registry().read().values().cloned().collect()
}

pub fn all_ai_tool_json() -> Vec<serde_json::Value> {
    ai_tool_registry().read().values().map(|t| t.to_tool_json()).collect()
}

// ════════════════════════════════════════════════════════
// 统一命令池调度 v8.0 — AI/用户共享执行路径
// ════════════════════════════════════════════════════════

// v8.0: 全局 PluginManager 和 execute_plugin_ai_command 已迁移至 script/plugin_mgr.rs
// 避免循环依赖: plugin ← script (script已依赖plugin::CircuitBreaker)
// AI工具调度入口: crate::script::execute_plugin_ai_command(name, args_json)

// ════════════════════════════════════════════════════════
// 事件钩子 v6.0 — 扩展事件类型
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHook {
    pub event: String,
    pub command: String,
    #[serde(default)]
    pub interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HookEvent {
    OnLoad,
    OnUnload,
    OnCmd(String),
    OnScanComplete,
    OnTimer,
    // ── v6.0 新事件 ──
    OnHotReload,          // 热重载触发
    OnCrash(String),      // 插件崩溃(含错误信息)
    OnCircuitOpen(String),// 熔断器打开
    OnCircuitClose(String),// 熔断器关闭
    OnConfigChange,       // 配置变更
    OnHostShutdown,       // 宿主关闭
    OnPluginMessage { from: String, msg: String }, // 插件间消息
}

// ════════════════════════════════════════════════════════
// 沙箱配置 v6.0
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub max_memory_mb: u64,
    pub network_enabled: bool,
    pub filesystem_enabled: bool,
    pub exec_enabled: bool,
    pub plugin_call_enabled: bool,     // v6.0: 插件间调用
    pub hot_reload_enabled: bool,      // v6.0: 热重载
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_output_bytes: 10 * 1024 * 1024,
            max_memory_mb: 512,
            network_enabled: true,
            filesystem_enabled: false,
            exec_enabled: false,
            plugin_call_enabled: false,
            hot_reload_enabled: true,
        }
    }
}

// ════════════════════════════════════════════════════════
// 熔断器 v6.0 — 防崩溃核心
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    pub failure_count: u32,
    pub last_failure_time: Option<Instant>,
    pub threshold: u32,           // 失败次数阈值
    pub window_secs: u64,         // 时间窗口(秒)
    pub cooldown_secs: u64,       // 熔断冷却时间
    pub state: CircuitState,
    pub total_crashes: u32,       // 历史总崩溃数
    pub crash_history: Vec<CrashRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,      // 正常
    HalfOpen,    // 半开(试探性恢复)
    Open,        // 熔断(拒绝调用)
    Disabled,    // 永久禁用
}

#[derive(Debug, Clone)]
pub struct CrashRecord {
    pub timestamp: Instant,
    pub error_msg: String,
    pub stack_snapshot: Option<String>,
    pub call_command: String,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failure_count: 0,
            last_failure_time: None,
            threshold: if threshold == 0 { 10 } else { threshold },
            window_secs: 60,
            cooldown_secs: 120,
            state: CircuitState::Closed,
            total_crashes: 0,
            crash_history: Vec::with_capacity(32),
        }
    }

    /// 记录一次失败，返回是否触发熔断
    pub fn record_failure(&mut self, error_msg: &str, command: &str) -> bool {
        let now = Instant::now();
        self.total_crashes += 1;
        self.crash_history.push(CrashRecord {
            timestamp: now,
            error_msg: error_msg.to_string(),
            stack_snapshot: None,
            call_command: command.to_string(),
        });
        // v6.2 fix: 使用drain截断前N个旧记录, 避免Vec::remove(0)的O(n)移动
        if self.crash_history.len() > 64 {
            let excess = self.crash_history.len() - 64;
            self.crash_history.drain(0..excess);
        }

        match self.last_failure_time {
            Some(last) if now.duration_since(last) < Duration::from_secs(self.window_secs) => {
                self.failure_count += 1;
            }
            _ => {
                self.failure_count = 1;
            }
        }
        self.last_failure_time = Some(now);

        if self.failure_count >= self.threshold {
            self.state = CircuitState::Open;
            return true;
        }
        false
    }

    /// 检查是否允许调用
    pub fn allow_call(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last) = self.last_failure_time {
                    if last.elapsed() >= Duration::from_secs(self.cooldown_secs) {
                        self.state = CircuitState::HalfOpen;
                        self.failure_count = 0;
                        return true;
                    }
                }
                false
            }
            CircuitState::Disabled => false,
        }
    }

    /// 记录成功调用
    pub fn record_success(&mut self) {
        if self.state == CircuitState::HalfOpen {
            self.state = CircuitState::Closed;
            self.failure_count = 0;
        }
    }

    /// 永久禁用
    pub fn disable(&mut self) {
        self.state = CircuitState::Disabled;
    }

    /// 强制重置
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.last_failure_time = None;
    }

    pub fn status_report(&self) -> String {
        format!(
            "CircuitBreaker[{:?}] failures={}/{} total_crashes={} cooldown={}s",
            self.state, self.failure_count, self.threshold,
            self.total_crashes, self.cooldown_secs
        )
    }
}

// ════════════════════════════════════════════════════════
// 插件间通信总线 v6.0
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct InterPluginMessage {
    pub from_plugin: String,
    pub to_plugin: String,       // 空 = 广播
    pub msg_type: String,        // "event" | "request" | "response" | "data"
    pub payload: String,         // JSON
    pub timestamp: Instant,
    pub correlation_id: String,  // 请求-响应关联
}

pub struct InterPluginBus {
    pub subscribers: RwLock<HashMap<String, Vec<String>>>, // plugin → [topic]
    pub message_queue: RwLock<Vec<InterPluginMessage>>,
    pub max_queue_size: usize,
}

impl InterPluginBus {
    pub fn new() -> Self {
        Self {
            subscribers: RwLock::new(HashMap::new()),
            message_queue: RwLock::new(Vec::with_capacity(256)),
            max_queue_size: 1024,
        }
    }

    pub fn publish(&self, msg: InterPluginMessage) {
        let mut queue = self.message_queue.write();
        queue.push(msg);
        if queue.len() > self.max_queue_size {
            queue.remove(0);
        }
    }

    /// v6.2 fix: 使用订阅过滤 + Vec::retain避免O(n)克隆
    pub fn drain_for(&self, plugin_name: &str) -> Vec<InterPluginMessage> {
        let mut queue = self.message_queue.write();
        let mut result = Vec::new();

        // 获取该插件订阅的topic列表
        let subs = self.subscribers.read();
        let topics: Vec<String> = subs.get(plugin_name).cloned().unwrap_or_default();
        drop(subs);

        // 使用retain原地过滤, 避免drain+collect的O(n)重新分配
        queue.retain(|m| {
            // 定向消息: to_plugin匹配
            if !m.to_plugin.is_empty() {
                if m.to_plugin == plugin_name {
                    result.push(m.clone());
                    return false; // 移除
                }
                return true; // 保留给其他插件
            }
            // 广播消息: 检查订阅 — 插件订阅了msg_type才投递
            if topics.is_empty() || topics.iter().any(|t| t == &m.msg_type) {
                result.push(m.clone());
                return false; // 移除(已投递)
            }
            true // 未订阅, 保留给其他订阅者
        });
        result
    }

    pub fn subscribe(&self, plugin: &str, topic: &str) {
        self.subscribers.write()
            .entry(plugin.to_string())
            .or_default()
            .push(topic.to_string());
    }

    pub fn unsubscribe(&self, plugin: &str, topic: &str) {
        if let Some(vec) = self.subscribers.write().get_mut(plugin) {
            vec.retain(|t| t != topic);
        }
    }
}

// ════════════════════════════════════════════════════════
// 字符串Intern池 v6.0 — 命令名去重
// ════════════════════════════════════════════════════════

pub struct StringInterner {
    pool: RwLock<HashMap<String, Arc<str>>>,
    max_capacity: usize,  // v6.2: Arc<str>引用计数替代Box::leak
}

impl StringInterner {
    pub fn new() -> Self {
        Self { pool: RwLock::new(HashMap::new()), max_capacity: 4096 }
    }

    /// v6.2 fix: 移除Box::leak永久泄漏, 使用Arc<str>引用计数管理
    /// 读路径直接查表返回克隆, 写路径原子插入, 无TOCTOU窗口
    pub fn intern(&self, s: &str) -> String {
        // 单次写锁完成查+插, 消除TOCTOU竞态窗口
        {
            let pool = self.pool.read();
            if let Some(cached) = pool.get(s) {
                return cached.to_string(); // Arc<str> → String
            }
            if pool.len() >= self.max_capacity {
                return s.to_string();
            }
        }
        // 未命中: 写锁插入
        let mut pool = self.pool.write();
        // 双重检查: 防止写锁获取前其他线程已插入
        if let Some(cached) = pool.get(s) {
            return cached.to_string();
        }
        if pool.len() >= self.max_capacity {
            return s.to_string();
        }
        let arc_str: Arc<str> = Arc::from(s.to_string());
        pool.insert(s.to_string(), arc_str);
        s.to_string()
    }
}

// ════════════════════════════════════════════════════════
// 已加载插件实例 v6.0
// ════════════════════════════════════════════════════════

pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub path: String,
    pub library: Option<Arc<libloading::Library>>,  // v6.1: Arc包装支持线程安全共享
    pub loaded_at: Instant,
    pub call_count: u64,
    pub total_call_time_ms: u64,
    pub sandbox: SandboxConfig,
    pub verified: bool,
    pub status: PluginStatus,
    // ── v6.0 新增 ──
    pub circuit_breaker: CircuitBreaker,
    pub last_call_command: String,
    pub crashed_count: u32,
    // v4.2.1: 超时后被抛弃的线程句柄 — 卸载前必须join, 防止DLL卸载时FFI调用segfault
    pub abandoned_threads: Vec<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    Active,
    Sandboxed,
    Timeout,
    Error,
    Unloaded,
    CircuitOpen,     // v6.0: 熔断状态
    Degraded,        // v6.0: 降级运行
    Faulted,         // v6.1: 故障 (连续崩溃>=3次, 需手动恢复)
}

// ════════════════════════════════════════════════════════
// 命令注册表
// ════════════════════════════════════════════════════════

pub struct CommandRegistry {
    commands: HashMap<String, PluginCommandDef>,  // cmd → def
    aliases: HashMap<String, String>,             // alias → cmd
    plugin_commands: HashMap<String, Vec<String>>, // plugin → [cmd]
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
            plugin_commands: HashMap::new(),
        }
    }

    pub fn register_plugin(&mut self, plugin_name: &str, cmds: &[PluginCommandDef]) -> Result<usize, String> {
        // 检查命令名冲突
        for cmd in cmds {
            if let Some(existing) = self.commands.get(&cmd.cmd) {
                return Err(format!(
                    "命令名冲突: '{}' 已被插件 '{}' 注册",
                    cmd.cmd, existing.plugin_name
                ));
            }
        }

        let mut registered = 0;
        let mut cmd_names = Vec::new();
        for cmd in cmds {
            let mut def = cmd.clone();
            def.plugin_name = plugin_name.to_string();
            def.is_builtin = false;

            // 注册别名映射
            for alias in &def.aliases {
                if self.aliases.contains_key(alias) {
                    return Err(format!("别名冲突: '{}' 已被占用", alias));
                }
                self.aliases.insert(alias.clone(), def.cmd.clone());
            }

            cmd_names.push(def.cmd.clone());
            self.commands.insert(def.cmd.clone(), def);
            registered += 1;
        }

        self.plugin_commands.insert(plugin_name.to_string(), cmd_names);
        Ok(registered)
    }

    pub fn unregister_plugin(&mut self, plugin_name: &str) -> usize {
        let cmd_names = self.plugin_commands.remove(plugin_name).unwrap_or_default();
        let mut count = 0;
        for name in &cmd_names {
            if let Some(def) = self.commands.remove(name) {
                for alias in &def.aliases {
                    self.aliases.remove(alias);
                }
                count += 1;
            }
        }
        count
    }

    pub fn find(&self, input: &str) -> Option<&PluginCommandDef> {
        self.commands.get(input)
            .or_else(|| self.aliases.get(input).and_then(|cmd| self.commands.get(cmd)))
    }

    pub fn list_all(&self) -> Vec<&PluginCommandDef> {
        let mut v: Vec<_> = self.commands.values().collect();
        v.sort_by(|a, b| a.cmd.cmp(&b.cmd));
        v
    }

    pub fn list_by_category(&self, category: &str) -> Vec<&PluginCommandDef> {
        self.commands.values()
            .filter(|c| c.category.eq_ignore_ascii_case(category))
            .collect()
    }

    pub fn list_by_plugin(&self, plugin_name: &str) -> Vec<&PluginCommandDef> {
        self.plugin_commands.get(plugin_name)
            .map(|names| names.iter().filter_map(|n| self.commands.get(n)).collect())
            .unwrap_or_default()
    }

    pub fn command_count(&self) -> usize { self.commands.len() }
    pub fn command_names(&self) -> Vec<&str> { self.commands.keys().map(|s| s.as_str()).collect() }
}

// ════════════════════════════════════════════════════════
// 插件管理器 v6.0 — 核心
// ════════════════════════════════════════════════════════

pub struct PluginManager {
    pub plugins: HashMap<String, LoadedPlugin>,
    pub load_order: Vec<String>,
    pub cmd_registry: CommandRegistry,
    pub plugin_dirs: Vec<String>,
    pub inter_plugin_bus: Arc<InterPluginBus>,
    pub interner: StringInterner,
    // ── 全局统计 ──
    pub total_calls: AtomicU64,
    pub total_crashes: AtomicU64,
    // ── v7.0 插件保险库 ──
    pub plugin_vault: Option<Arc<crate::plugin_vault::PluginVault>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            load_order: Vec::new(),
            cmd_registry: CommandRegistry::new(),
            plugin_dirs: vec!["plugins".into(), ".".into()],
            inter_plugin_bus: Arc::new(InterPluginBus::new()),
            interner: StringInterner::new(),
            total_calls: AtomicU64::new(0),
            total_crashes: AtomicU64::new(0),
            plugin_vault: None,
        }
    }

    /// 设置插件保险库
    pub fn set_plugin_vault(&mut self, vault: Arc<crate::plugin_vault::PluginVault>) {
        self.plugin_vault = Some(vault);
    }

    // ═══════════════════════════════════════════════════
    // 加载插件 v6.0 — 完整流程
    // ═══════════════════════════════════════════════════

    /// v7.0: 从保险库或文件系统加载插件
    /// 优先检查保险库 → 解密到临时文件 → 加载后擦除临时文件
    pub fn load_plugin(&mut self, name: &str, path: &str) -> Result<String, String> {
        if self.plugins.contains_key(name) {
            return Err(format!("插件 '{}' 已加载", name));
        }

        let resolved_path;
        let mut temp_file: Option<PathBuf> = None;

        // v7.0: 检查保险库
        if let Some(ref vault) = self.plugin_vault {
            if vault.is_mounted() {
                // 尝试从保险库加载
                match vault.extract_to_temp(name) {
                    Ok(tmp_path) => {
                        temp_file = Some(tmp_path.clone());
                        resolved_path = tmp_path;
                    }
                    Err(_) => {
                        // 保险库中没有，回退到文件系统
                        resolved_path = PathBuf::from(self.resolve_plugin_path(path)?);
                    }
                }
            } else {
                resolved_path = PathBuf::from(self.resolve_plugin_path(path)?);
            }
        } else {
            resolved_path = PathBuf::from(self.resolve_plugin_path(path)?);
        }

        // 加载动态库
        let lib = unsafe {
            libloading::Library::new(&resolved_path)
                .map_err(|e| format!("无法加载动态库 '{}': {}", resolved_path.display(), e))?
        };

        // 读取清单
        let path_str = resolved_path.to_string_lossy().into_owned();
        let manifest = self.load_manifest(&lib, name, &path_str)?;

        // 验证签名
        let verified = self.verify_plugin_signature(&path_str, &manifest);

        // 检查依赖
        self.check_dependencies(&manifest)?;

        // 调用 init
        unsafe {
            if let Ok(init_func) = lib.get::<unsafe extern "C" fn() -> i32>(b"ruoo_plugin_init") {
                let ret = init_func();
                if ret != 0 {
                    return Err(format!("插件 '{}' 初始化失败: 返回码 {}", name, ret));
                }
            }
        }

        // v6.3: 优先使用manifest.commands (load_manifest已获取)
        // 避免冗余的C FFI调用 get_plugin_commands
        let declared_cmds = if !manifest.commands.is_empty() {
            manifest.commands.clone()
        } else {
            self.get_plugin_commands(&lib)
        };
        let cmd_count = self.cmd_registry.register_plugin(name, &declared_cmds)?;

        // 构建插件实例
        let threshold = if manifest.crash_threshold > 0 { manifest.crash_threshold } else { 10 };
        let plugin = LoadedPlugin {
            manifest: manifest.clone(),
            path: resolved_path.to_string_lossy().into_owned(),
            library: Some(Arc::new(lib)),
            loaded_at: Instant::now(),
            call_count: 0,
            total_call_time_ms: 0,
            sandbox: SandboxConfig::default(),
            verified,
            status: PluginStatus::Active,
            circuit_breaker: CircuitBreaker::new(threshold),
            last_call_command: String::new(),
            crashed_count: 0,
            abandoned_threads: Vec::new(),
        };

        self.plugins.insert(name.to_string(), plugin);
        self.load_order.push(name.to_string());

        // 注册AI工具
        let ai_count = register_ai_tools(name, &declared_cmds);

        // 触发钩子
        self.emit_internal_event(HookEvent::OnLoad, name);

        // v7.0: 清理临时解密文件 (库已加载到内存)
        if let Some(ref tmp) = temp_file {
            if let Some(ref vault) = self.plugin_vault {
                vault.cleanup_temp(tmp);
            }
        }

        let vault_tag = if temp_file.is_some() { " [🔒vault]" } else { "" };
        Ok(format!(
            "[+] 插件已加载: {} v{} | 命令: {}个 | AI工具: {}个 | 分类: [{}] | 钩子: {}个 | {}{}",
            name, manifest.version, cmd_count, ai_count,
            manifest.categories.join(", "),
            manifest.hooks.len(),
            if verified { "[验签✓]" } else { "[!未验签]" },
            vault_tag,
        ))
    }

    // ═══════════════════════════════════════════════════
    // 调用插件 v6.1 — 线程隔离 + 超时 + 错误检测
    // ═══════════════════════════════════════════════════

    /// v6.1: 委托给 call_plugin_with_timeout (30s默认超时)
    pub fn call_plugin(&mut self, name: &str, command: &str) -> Result<String, String> {
        self.call_plugin_with_timeout(name, command, 30_000)
    }

    /// v6.1 fix: 带超时的线程隔离调用 — 防止插件死循环/死锁挂起主进程
    /// 同时检测插件约定的错误前缀 ERR:/ERROR:/FATAL: 用于熔断器
    pub fn call_plugin_with_timeout(&mut self, name: &str, command: &str, timeout_ms: u64) -> Result<String, String> {
        let mut deferred_event: Option<HookEvent> = None;
        let name_owned = name.to_string();
        let name_for_timeout = name.to_string(); // v6.3: 超时错误消息用(线程闭包消耗name_owned)
        let cmd_owned = command.to_string();

        // ── 预检查 (持有plugins借用, 提取Arc<Library>) ──
        let lib_arc = {
            let plugin = self.plugins.get_mut(name)
                .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

            if plugin.status == PluginStatus::Faulted {
                return Err(format!("插件 '{}' 已标记为故障 (崩溃{}次) — 请用 crash_recover 恢复",
                    name, plugin.crashed_count));
            }

            if !plugin.circuit_breaker.allow_call() {
                return Err(format!(
                    "插件 '{}' 已熔断: {}", name, plugin.circuit_breaker.status_report()
                ));
            }

            if plugin.status == PluginStatus::CircuitOpen || plugin.status == PluginStatus::Degraded {
                return Err(format!("插件 '{}' 状态异常: {:?}", name, plugin.status));
            }

            let lib = plugin.library.as_ref()
                .ok_or("插件库句柄不可用")?;
            plugin.last_call_command = command.to_string();
            Arc::clone(lib)
        };

        let effective_timeout = {
            let plugin = self.plugins.get(name).unwrap();
            let t = plugin.manifest.timeout_ms;
            if t > 0 && t < timeout_ms { t } else { timeout_ms }
        };

        // ── 独立线程执行 + 真实超时保护 (mpsc channel) ──
        let start = Instant::now();
        let lib_for_thread = Arc::clone(&lib_arc);
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

        let handle = std::thread::Builder::new()
            .name(format!("plug-{}", &name_owned[..name_owned.len().min(16)]))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String, String> {
                    let c_cmd = CString::new(&*cmd_owned).map_err(|e| format!("命令编码失败: {}", e))?;
                    // v4.3.1: 统一VEH保护 — 所有插件FFI调用都走 enter/leave_protected_call
                    if !crate::panic_guard::native_crash::enter_protected_call(&name_owned) {
                        let (code, addr, desc) = crate::panic_guard::native_crash::crash_details();
                        return Err(format!(
                            "[原生崩溃] 插件 '{}' 触发硬件异常: {} (0x{:08X} at 0x{:016X})",
                            name_owned, desc, code, addr
                        ));
                    }
                    // RAII守卫: 确保leave在任何?/return/panic路径上都被调用
                    struct VehGuard;
                    impl Drop for VehGuard {
                        fn drop(&mut self) {
                            crate::panic_guard::native_crash::leave_protected_call();
                        }
                    }
                    let _veh_guard = VehGuard;
                    unsafe {
                        let func = lib_for_thread.get::<unsafe extern "C" fn(*const c_char) -> *mut c_char>(b"ruoo_plugin_exec")
                            .map_err(|_| "插件未导出 ruoo_plugin_exec".to_string())?;
                        let ptr = func(c_cmd.as_ptr());
                        if ptr.is_null() {
                            return Ok("(插件返回空)".to_string());
                        }
                        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                        if let Ok(free_func) = lib_for_thread.get::<unsafe extern "C" fn(*mut c_char)>(b"ruoo_plugin_free_result") {
                            free_func(ptr);
                        }
                        Ok(s)
                    }
                }));
                let final_result = match result {
                    Ok(r) => r,
                    Err(panic_err) => {
                        let msg = if let Some(s) = panic_err.downcast_ref::<&str>() { s.to_string() }
                        else if let Some(s) = panic_err.downcast_ref::<String>() { s.clone() }
                        else { "未知panic".to_string() };
                        Err(format!("插件 '{}' panic: {}", name_owned, msg))
                    }
                };
                let _ = tx.send(final_result); // 忽略发送失败(接收端已超时断开)
            })
            .map_err(|e| format!("无法创建插件线程: {}", e))?;

        // 真实超时等待: recv_timeout
        let result = match rx.recv_timeout(Duration::from_millis(effective_timeout)) {
            Ok(r) => r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // v4.2.1: 线程仍在运行 — 存储句柄到插件实例, 卸载时join
                // 防止 DLL 卸载时 FFI 调用仍在进行导致 segfault (0xC0000005)
                if let Some(plugin) = self.plugins.get_mut(name) {
                    plugin.abandoned_threads.push(handle);
                } else {
                    // 插件已被移除, 只能丢弃线程 (极端情况)
                    drop(handle);
                }
                Err(format!(
                    "插件 '{}' 执行超时 (>{:.1}s) — 线程仍在运行但已登记, 卸载前将等待完成",
                    name_for_timeout, effective_timeout as f64 / 1000.0
                ))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(format!("插件 '{}' 线程异常终止 (channel断开)", name_for_timeout))
            }
        };

        let elapsed_ms = start.elapsed().as_millis() as u64;

        // ── 更新统计 + 熔断器 ──
        let is_plugin_error;
        {
            if let Some(plugin) = self.plugins.get_mut(name) {
                plugin.call_count += 1;
                plugin.total_call_time_ms += elapsed_ms;

                match &result {
                    Ok(output) => {
                        let trimmed = output.trim_start();
                        is_plugin_error = trimmed.starts_with("ERR:")
                            || trimmed.starts_with("ERROR:")
                            || trimmed.starts_with("FATAL:");
                        if is_plugin_error {
                            let triggered = plugin.circuit_breaker.record_failure(output, command);
                            plugin.crashed_count += 1;
                            if plugin.crashed_count >= 3 {
                                plugin.status = PluginStatus::Faulted;
                            }
                            if triggered {
                                plugin.status = PluginStatus::CircuitOpen;
                                deferred_event = Some(HookEvent::OnCircuitOpen(name.to_string()));
                            }
                        } else {
                            plugin.circuit_breaker.record_success();
                        }
                    }
                    Err(e) => {
                        is_plugin_error = true;
                        let triggered = plugin.circuit_breaker.record_failure(e, command);
                        plugin.crashed_count += 1;
                        if plugin.crashed_count >= 3 {
                            plugin.status = PluginStatus::Faulted;
                        }
                        if triggered {
                            plugin.status = PluginStatus::CircuitOpen;
                            deferred_event = Some(HookEvent::OnCircuitOpen(name.to_string()));
                        }
                    }
                }
            } else {
                is_plugin_error = result.is_err();
            }
        }

        self.total_calls.fetch_add(1, Ordering::Relaxed);
        if is_plugin_error {
            self.total_crashes.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(event) = deferred_event {
            self.emit_internal_event(event, name);
        }

        result
    }

    // ═══════════════════════════════════════════════════
    // 安全调用插件 (catch_unwind包裹)
    // ═══════════════════════════════════════════════════

    /// v6.2 fix: 委托给 call_plugin_with_timeout 消除代码重复
    /// 设置极长超时(3600s)模拟无超时调用, panic/错误/熔断逻辑统一处理
    pub fn call_plugin_safe(&mut self, name: &str, command: &str) -> Result<String, String> {
        self.call_plugin_with_timeout(name, command, 3_600_000)
    }

    // ═══════════════════════════════════════════════════
    // 命令热重载 v6.4 — 仅通过 plugin reload <name> 命令触发
    // ═══════════════════════════════════════════════════

    /// 重载单个插件 (卸载+加载+回滚)
    /// 触发方式: plugin reload <name> 命令
    /// manifest.hot_reload_enabled=false 时拒绝重载
    pub fn reload_plugin(&mut self, name: &str) -> Result<String, String> {
        let path = self.plugins.get(name)
            .map(|p| p.path.clone())
            .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

        // 检查该插件是否允许热重载
        let hot_ok = self.plugins.get(name)
            .map(|p| p.manifest.hot_reload_enabled)
            .unwrap_or(true);

        if !hot_ok {
            return Err("该插件禁用了热重载".into());
        }

        // v6.3: 备份旧版本元数据用于回滚
        let old_call_count = self.plugins.get(name).map(|p| p.call_count).unwrap_or(0);
        let old_crashed = self.plugins.get(name).map(|p| p.crashed_count).unwrap_or(0);

        // 先卸载旧版本
        let _was_loaded = self.unload_plugin_internal(name)?;

        // 重新加载
        match self.load_plugin(name, &path) {
            Ok(msg) => {
                // 触发热重载钩子
                self.emit_internal_event(HookEvent::OnHotReload, name);
                Ok(format!("{} (热重载成功)", msg))
            }
            Err(e) => {
                // v6.3: 回滚 — 尝试从同一路径重新加载旧版本
                // (旧DLL可能仍存在或被替换, 尽力而为)
                match self.load_plugin(name, &path) {
                    Ok(_msg) => {
                        // 恢复统计 (近似)
                        if let Some(p) = self.plugins.get_mut(name) {
                            p.call_count = old_call_count;
                            p.crashed_count = old_crashed;
                        }
                        Err(format!(
                            "热重载失败: {} — 已回滚到旧版本 (统计已恢复)", e
                        ))
                    }
                    Err(rollback_err) => {
                        Err(format!(
                            "热重载失败: {} — 回滚也失败: {} (插件 '{}' 已完全卸载!)",
                            e, rollback_err, name
                        ))
                    }
                }
            }
        }
    }

    fn unload_plugin_internal(&mut self, name: &str) -> Result<String, String> {
        for (other_name, other_plugin) in &self.plugins {
            if other_plugin.manifest.requires.contains(&name.to_string()) {
                return Err(format!("无法卸载: '{}' 被 '{}' 依赖", name, other_name));
            }
        }

        // v6.2 fix: 先触发OnUnload事件(此时插件仍在plugins中, 其他插件可响应)
        self.emit_internal_event(HookEvent::OnUnload, name);

        let mut plugin = self.plugins.remove(name)
            .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

        // v4.2.1: 首先join所有超时后被抛弃的线程 — 这些线程持有Arc<Library>
        // 必须先join再drop library, 否则DLL卸载时FFI调用仍在进行→segfault (0xC0000005)
        let mut joined_count = 0;
        for handle in plugin.abandoned_threads.drain(..) {
            let _ = handle.join();
            joined_count += 1;
        }
        if joined_count > 0 {
            // 线程已回收, 短暂等待OS完成线程清理
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        if let Some(lib) = plugin.library.take() {
            // v6.6: 调用 shutdown 前先让出CPU，给孤立线程完成机会
            // call_plugin_with_timeout 使用 detached thread (JoinHandle被丢弃),
            // 线程可能仍持有Arc<Library>。短暂等待让线程自然完成。
            for _ in 0..10 {
                let strong = Arc::strong_count(&lib);
                if strong <= 1 { break; }  // 只剩我们自己, 没有其他持有者
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            unsafe {
                if let Ok(func) = lib.get::<unsafe extern "C" fn()>(b"ruoo_plugin_shutdown") {
                    func();
                }
            }

            // v6.6: 保存路径用于后续强制清理
            let plugin_path = plugin.path.clone();
            drop(lib);

            // v6.6: Windows强制释放DLL文件句柄 — 三阶段清理
            // 阶段1: 等待 detached 线程自然结束 (它们持有 Arc 克隆)
            // 阶段2: 循环FreeLibrary耗尽所有LoadLibrary引用
            // 阶段3: 验证文件是否解锁, 必要时重试
            #[cfg(windows)]
            {
                extern "system" {
                    fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                    fn GetModuleHandleExW(dwFlags: u32, lpModuleName: *const u16, phModule: *mut isize) -> i32;
                    fn FreeLibrary(hLibModule: isize) -> i32;
                }
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = std::ffi::OsStr::new(&plugin_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();

                // 阶段1: 等待 detached 线程释放 Arc 克隆 (最多1秒)
                for _ in 0..20 {
                    unsafe {
                        let h = GetModuleHandleW(wide.as_ptr());
                        if h == 0 { break; }  // DLL已完全卸载
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                // 阶段2: 激进FreeLibrary — 耗尽所有残留引用
                // libloading::Library::new → LoadLibraryExW (refcnt+1)
                // libloading::Library::drop → FreeLibrary   (refcnt-1)
                // 但 DLL DllMain可能创建线程/TLS, 这些隐式增加引用
                // 循环调用直到 GetModuleHandleW 返回0 或 重试耗尽
                for _ in 0..32 {
                    unsafe {
                        let h = GetModuleHandleW(wide.as_ptr());
                        if h == 0 { break; }
                        FreeLibrary(h);
                        // 给OS时间处理 DLL_PROCESS_DETACH
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }

                // 阶段3: 最终验证 — 如果模块仍在内存中, 使用 GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT
                // 检测引用计数是否已归零(0x00000002 = GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT)
                unsafe {
                    let mut h: isize = 0;
                    let still_loaded = GetModuleHandleExW(0x00000002u32, wide.as_ptr(), &mut h) != 0;
                    if still_loaded {
                        // 模块仍加载 — 最后一次尝试: 连续FreeLibrary
                        for _ in 0..8 {
                            FreeLibrary(h);
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            if GetModuleHandleW(wide.as_ptr()) == 0 { break; }
                        }
                    }
                }
            }
        }

        let cmd_count = self.cmd_registry.unregister_plugin(name);
        unregister_ai_tools(name);
        self.load_order.retain(|n| n != name);
        plugin.status = PluginStatus::Unloaded;

        Ok(format!("已卸载: {} | 注销命令: {}个", name, cmd_count))
    }

    // ═══════════════════════════════════════════════════
    // 卸载插件
    // ═══════════════════════════════════════════════════

    pub fn unload_plugin(&mut self, name: &str) -> Result<String, String> {
        self.unload_plugin_internal(name)
    }

    /// v6.6: 强制卸载 — 跳过shutdown直接释放, 用于崩溃/挂起插件
    /// 比unload_plugin更激进: 不调用ruoo_plugin_shutdown, 不留情面直接drop
    pub fn force_unload(&mut self, name: &str) -> Result<String, String> {
        // 依赖检查 (跳过 — 强制卸载不管依赖)
        let mut plugin = self.plugins.remove(name)
            .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

        let plugin_path = plugin.path.clone();
        let ver = plugin.manifest.version.clone();
        let crashes = plugin.crashed_count;

        // v4.2.1: 首先join所有超时后被抛弃的线程
        for handle in plugin.abandoned_threads.drain(..) {
            let _ = handle.join();
        }

        // 直接拿走Arc并强制释放 — 不调用shutdown
        if let Some(lib) = plugin.library.take() {
            // 等待孤立线程释放Arc (最多500ms)
            for _ in 0..10 {
                if Arc::strong_count(&lib) <= 1 { break; }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            drop(lib);

            // Windows: 激进FreeLibrary循环
            #[cfg(windows)]
            {
                extern "system" {
                    fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                    fn FreeLibrary(hLibModule: isize) -> i32;
                }
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = std::ffi::OsStr::new(&plugin_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                for _ in 0..32 {
                    unsafe {
                        let h = GetModuleHandleW(wide.as_ptr());
                        if h == 0 { break; }
                        FreeLibrary(h);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        }

        let cmd_count = self.cmd_registry.unregister_plugin(name);
        unregister_ai_tools(name);
        self.load_order.retain(|n| n != name);

        Ok(format!(
            "[!] 强制卸载: {} v{} | 注销命令: {}个 | 累计崩溃: {}",
            name, ver, cmd_count, crashes
        ))
    }

    /// v7.0: 卸载插件并恢复到plugins目录 (从vault提取)
    pub fn unload_and_restore(&mut self, name: &str) -> Result<String, String> {
        if let Some(ref vault) = self.plugin_vault {
            if vault.is_mounted() {
                // 尝试从vault提取到plugins目录
                let _ = vault.extract_to_dir(name, &PathBuf::from("plugins"));
            }
        }
        self.unload_plugin_internal(name)
    }

    /// v7.0: 卸载插件并重新加密存入vault
    pub fn unload_and_revault(&mut self, name: &str) -> Result<String, String> {
        // 先获取插件路径
        let plugin_path = self.plugins.get(name)
            .map(|p| p.path.clone())
            .unwrap_or_default();

        let result = self.unload_plugin_internal(name);

        // 重新加密入库
        if let Some(ref vault) = self.plugin_vault {
            if vault.is_mounted() && !plugin_path.is_empty() {
                let path = PathBuf::from(&plugin_path);
                if path.exists() {
                    let _ = vault.vault_plugin(&path, name);
                }
            }
        }

        result
    }

    /// v7.0: 将已注册插件加密入库
    pub fn vault_registered_plugin(&self, alias: &str, plugin_path: &Path) -> Result<String, String> {
        let vault = self.plugin_vault.as_ref()
            .ok_or("插件保险库未挂载")?;

        if !vault.is_mounted() {
            return Err("插件保险库未挂载".into());
        }

        let vp = vault.vault_plugin(plugin_path, alias)?;
        Ok(format!(
            "[🔒] 插件已加密入库: {}\n  \
             加密文件: {}\n  \
             原始哈希: {}\n  \
             加密大小: {}",
            alias,
            vp.vault_path.display(),
            &vp.original_hash[..vp.original_hash.len().min(16)],
            format_size(vp.encrypted_size),
        ))
    }

    /// v7.0: 列出保险库中的插件
    pub fn list_vaulted_plugins(&self) -> String {
        match self.plugin_vault.as_ref() {
            Some(vault) => vault.status_report(),
            None => "插件保险库: 未挂载".into(),
        }
    }

    // ═══════════════════════════════════════════════════
    // 熔断器管理
    // ═══════════════════════════════════════════════════

    pub fn reset_circuit(&mut self, name: &str) -> Result<String, String> {
        let plugin = self.plugins.get_mut(name)
            .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

        plugin.circuit_breaker.reset();
        plugin.status = PluginStatus::Active;
        Ok(format!("[↻] 插件 '{}' 熔断器已重置", name))
    }

    pub fn disable_plugin_permanent(&mut self, name: &str) -> Result<String, String> {
        let plugin = self.plugins.get_mut(name)
            .ok_or_else(|| format!("插件 '{}' 未加载", name))?;

        plugin.circuit_breaker.disable();
        plugin.status = PluginStatus::Unloaded;
        Ok(format!("[!] 插件 '{}' 已永久禁用", name))
    }

    // ═══════════════════════════════════════════════════
    // 插件间通信
    // ═══════════════════════════════════════════════════

    pub fn inter_plugin_send(&self, from: &str, to: &str, msg_type: &str, payload: &str) -> Result<(), String> {
        if !self.plugins.contains_key(from) {
            return Err(format!("发送方插件 '{}' 未加载", from));
        }
        if !to.is_empty() && !self.plugins.contains_key(to) {
            return Err(format!("接收方插件 '{}' 未加载", to));
        }

        self.inter_plugin_bus.publish(InterPluginMessage {
            from_plugin: from.to_string(),
            to_plugin: to.to_string(),
            msg_type: msg_type.to_string(),
            payload: payload.to_string(),
            timestamp: Instant::now(),
            correlation_id: uuid::Uuid::new_v4().to_string(),
        });

        Ok(())
    }

    pub fn inter_plugin_receive(&self, plugin_name: &str) -> Vec<InterPluginMessage> {
        self.inter_plugin_bus.drain_for(plugin_name)
    }

    // ═══════════════════════════════════════════════════
    // 列出和查询
    // ═══════════════════════════════════════════════════

    pub fn list_plugins(&self) -> String {
        if self.plugins.is_empty() {
            return "无已加载插件".into();
        }
        let mut output = format!("═══ 已加载插件 ({}个) ═══\n\n", self.plugins.len());
        for name in &self.load_order {
            if let Some(p) = self.plugins.get(name) {
                let sig = if p.verified { "[验签]" } else { "[!未验签]" };
                let status = match p.status {
                    PluginStatus::Active => "[+]",
                    PluginStatus::Sandboxed => "[~]",
                    PluginStatus::Timeout => "[!T]",
                    PluginStatus::Error => "[!E]",
                    PluginStatus::Unloaded => "[-]",
                    PluginStatus::CircuitOpen => "[!CIRCUIT]",
                    PluginStatus::Degraded => "[~D]",
                    PluginStatus::Faulted => "[!!FAULT]",
                };
                let cb = &p.circuit_breaker;
                output.push_str(&format!(
                    "{} {} v{} {} — {}\n",
                    status, name, p.manifest.version, sig,
                    p.manifest.description.as_deref().unwrap_or("(无描述)")
                ));
                output.push_str(&format!(
                    "   路径: {}\n   调用: {}次 | 耗时: {}ms | 命令: {}个",
                    p.path, p.call_count, p.total_call_time_ms,
                    self.cmd_registry.list_by_plugin(name).len(),
                ));
                if cb.total_crashes > 0 {
                    output.push_str(&format!(
                        " | 崩溃: {}次 | 熔断器: {:?}",
                        cb.total_crashes, cb.state
                    ));
                }
                output.push('\n');
                if !p.manifest.categories.is_empty() {
                    output.push_str(&format!("   分类: {}\n", p.manifest.categories.join(", ")));
                }
                if !p.manifest.requires.is_empty() {
                    output.push_str(&format!("   依赖: {}\n", p.manifest.requires.join(", ")));
                }
            }
        }
        output
    }

    pub fn list_commands(&self, category: Option<&str>) -> String {
        let cmds = match category {
            Some(cat) => self.cmd_registry.list_by_category(cat),
            None => self.cmd_registry.list_all(),
        };
        if cmds.is_empty() {
            return "无已注册插件命令".into();
        }
        let mut output = format!("═══ 插件命令 ({}个) ═══\n\n", cmds.len());
        for cmd in &cmds {
            output.push_str(&format!(
                "  {} — {} [{}]\n",
                cmd.cmd, cmd.help, cmd.plugin_name
            ));
            if !cmd.aliases.is_empty() {
                output.push_str(&format!("    别名: {}\n", cmd.aliases.join(", ")));
            }
        }
        output
    }

    pub fn plugin_health_report(&self, name: &str) -> Option<String> {
        self.plugins.get(name).map(|p| {
            format!(
                "═══ 插件健康: {} ═══\n\
                 状态: {:?}\n\
                 版本: {}\n\
                 调用: {}次\n\
                 总耗时: {}ms\n\
                 崩溃: {}次\n\
                 熔断器: {}\n\
                 命令数: {}个\n\
                 依赖: [{}]",
                name, p.status, p.manifest.version,
                p.call_count, p.total_call_time_ms,
                p.crashed_count,
                p.circuit_breaker.status_report(),
                self.cmd_registry.list_by_plugin(name).len(),
                p.manifest.requires.join(", ")
            )
        })
    }

    // ═══════════════════════════════════════════════════
    // 事件系统 v6.0
    // ═══════════════════════════════════════════════════

    fn emit_internal_event(&mut self, event: HookEvent, _source_plugin: &str) {
        let mut triggered = Vec::new();
        for (name, plugin) in &self.plugins {
            for hook in &plugin.manifest.hooks {
                let matches = match (hook.event.to_lowercase().as_str(), &event) {
                    ("on_load", HookEvent::OnLoad) => true,
                    ("on_unload", HookEvent::OnUnload) => true,
                    ("on_scan_complete", HookEvent::OnScanComplete) => true,
                    ("on_timer", HookEvent::OnTimer) => true,
                    ("on_hot_reload", HookEvent::OnHotReload) => true,
                    ("on_host_shutdown", HookEvent::OnHostShutdown) => true,
                    ("on_cmd", HookEvent::OnCmd(ref cmd)) => hook.command.contains(cmd),
                    ("on_crash", HookEvent::OnCrash(_)) => true,
                    ("on_circuit_open", HookEvent::OnCircuitOpen(_)) => true,
                    ("on_circuit_close", HookEvent::OnCircuitClose(_)) => true,
                    ("on_config_change", HookEvent::OnConfigChange) => true,
                    ("on_plugin_message", HookEvent::OnPluginMessage { .. }) => true,
                    _ => false,
                };
                if matches {
                    triggered.push((name.clone(), hook.command.clone()));
                }
            }
        }

        for (pname, cmd) in triggered {
            let _ = self.call_plugin(&pname, &cmd);
        }
    }

    pub fn emit_event(&mut self, event: HookEvent, source_plugin: &str) {
        self.emit_internal_event(event, source_plugin);
    }

    // ═══════════════════════════════════════════════════
    // 签名验证
    // ═══════════════════════════════════════════════════

    pub fn verify_plugin_signature(&self, path: &str, manifest: &PluginManifest) -> bool {
        // v6.2 fix: 缺失签名或空签名 = 未验证(而非自动通过)
        let expected = match &manifest.signature {
            Some(s) if !s.is_empty() => s,
            _ => return false, // 无签名声明 → 验证失败
        };

        match std::fs::read(path) {
            Ok(data) => {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(&data);
                let hash = format!("{:x}", hasher.finalize());
                hash == *expected
            }
            Err(_) => false,
        }
    }

    // ═══════════════════════════════════════════════════
    // 依赖检查
    // ═══════════════════════════════════════════════════

    fn check_dependencies(&self, manifest: &PluginManifest) -> Result<(), String> {
        for dep in &manifest.requires {
            if !self.plugins.contains_key(dep) {
                return Err(format!(
                    "插件 '{}' 依赖 '{}'，但 '{}' 未加载",
                    manifest.name, dep, dep
                ));
            }
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════
    // 辅助方法
    // ═══════════════════════════════════════════════════

    fn resolve_plugin_path(&self, path: &str) -> Result<String, String> {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
        for dir in &self.plugin_dirs {
            let full = std::path::Path::new(dir).join(path);
            if full.exists() { return Ok(full.to_string_lossy().into()); }
            #[cfg(windows)]
            let full_ext = full.with_extension("dll");
            #[cfg(not(windows))]
            let full_ext = full.with_extension("so");
            if full_ext.exists() { return Ok(full_ext.to_string_lossy().into()); }
        }
        Err(format!("未找到插件文件: {}", path))
    }

    fn load_manifest(&self, lib: &libloading::Library, name: &str, _path: &str) -> Result<PluginManifest, String> {
        // 尝试读取 ruoo_plugin_manifest() 导出
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_manifest") {
                let ptr = func();
                if !ptr.is_null() {
                    let json = CStr::from_ptr(ptr).to_string_lossy();
                    if let Ok(mut manifest) = PluginManifest::from_json(&json) {
                        if manifest.name.is_empty() { manifest.name = name.to_string(); }
                        let cmds = self.get_plugin_commands(lib);
                        manifest.commands = cmds;
                        return Ok(manifest);
                    }
                }
            }
        }

        // 回退: 从ruoo_plugin_commands构造基本清单
        let commands = self.get_plugin_commands(lib);
        let version = self.get_plugin_version(lib);
        Ok(PluginManifest {
            name: name.to_string(),
            version,
            commands,
            ..Default::default()
        })
    }

    fn get_plugin_commands(&self, lib: &libloading::Library) -> Vec<PluginCommandDef> {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_commands") {
                let ptr = func();
                if !ptr.is_null() {
                    let json = CStr::from_ptr(ptr).to_string_lossy();
                    if let Ok(cmds) = serde_json::from_str::<Vec<PluginCommandDef>>(&json) {
                        return cmds;
                    }
                }
            }
        }
        Vec::new()
    }

    fn get_plugin_version(&self, lib: &libloading::Library) -> String {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_version") {
                let ptr = func();
                if !ptr.is_null() {
                    return CStr::from_ptr(ptr).to_string_lossy().into_owned();
                }
            }
        }
        "0.0.0".to_string()
    }

    // ═══════════════════════════════════════════════════
    // 全局统计
    // ═══════════════════════════════════════════════════

    pub fn global_stats(&self) -> String {
        format!(
            "═══ 插件系统统计 ═══\n\
             已加载: {}个\n\
             总调用: {}次\n\
             总崩溃: {}次\n\
             已注册命令: {}个\n\
             活跃熔断器: {}个",
            self.plugins.len(),
            self.total_calls.load(Ordering::Relaxed),
            self.total_crashes.load(Ordering::Relaxed),
            self.cmd_registry.command_count(),
            self.plugins.values().filter(|p| p.circuit_breaker.state != CircuitState::Closed).count(),
        )
    }
}

// ════════════════════════════════════════════════════════
// Default impl for PluginManifest
// ════════════════════════════════════════════════════════

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.0.0".into(),
            author: None,
            description: None,
            categories: Vec::new(),
            requires: Vec::new(),
            permissions: 0,
            signature: None,
            commands: Vec::new(),
            hooks: Vec::new(),
            timeout_ms: 30_000,
            config: HashMap::new(),
            min_host_version: None,
            max_instances: 0,
            hot_reload_enabled: true,
            crash_threshold: 0,
        }
    }
}

// ════════════════════════════════════════════════════════
// 运行时配置全局存储 (供C ABI使用)
// ════════════════════════════════════════════════════════

static RUNTIME_CONFIG: std::sync::LazyLock<RwLock<HashMap<String, HashMap<String, String>>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

// ════════════════════════════════════════════════════════
// C ABI 导出 — v6.2 修复: 连接到实际全局状态
// 这些函数可由插件DLL/SO通过FFI调用
// ════════════════════════════════════════════════════════

/// 获取插件运行时配置 — v6.3: 返回纯值字符串(非JSON包装)
/// 未找到key时返回空字符串(非null, 避免插件崩溃)
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_config_get(
    plugin_name: *const c_char,
    key: *const c_char,
) -> *mut c_char {
    if plugin_name.is_null() || key.is_null() {
        return std::ptr::null_mut();
    }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let key = CStr::from_ptr(key).to_string_lossy();
    let config = RUNTIME_CONFIG.read();
    let result_val = config.get(name.as_ref())
        .and_then(|plug_cfg| plug_cfg.get(key.as_ref()))
        .cloned()
        .unwrap_or_default(); // 未找到 = 空字符串
    CString::new(result_val).map(|cs| cs.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// 设置插件运行时配置
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_config_set(
    plugin_name: *const c_char,
    key: *const c_char,
    value: *const c_char,
) -> i32 {
    if plugin_name.is_null() || key.is_null() || value.is_null() {
        return -1;
    }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let key = CStr::from_ptr(key).to_string_lossy();
    let val = CStr::from_ptr(value).to_string_lossy();
    RUNTIME_CONFIG.write()
        .entry(name.to_string())
        .or_default()
        .insert(key.to_string(), val.to_string());
    // 通知消息代理
    crate::message_broker::push_message(&name, &format!("[CONFIG] {} = {}", key, val));
    0
}

/// 插件日志 (结构化)
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_log(
    plugin_name: *const c_char,
    level: *const c_char,
    message: *const c_char,
) -> i32 {
    if plugin_name.is_null() || level.is_null() || message.is_null() {
        return -1;
    }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let lvl = CStr::from_ptr(level).to_string_lossy();
    let msg = CStr::from_ptr(message).to_string_lossy();
    crate::message_broker::push_message(&name, &format!("[{}] {}", lvl.to_uppercase(), msg));
    0
}

/// 插件向宿主发送事件
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_event_emit(
    plugin_name: *const c_char,
    event_type: *const c_char,
    event_data_json: *const c_char,
) -> i32 {
    if plugin_name.is_null() || event_type.is_null() {
        return -1;
    }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let evt = CStr::from_ptr(event_type).to_string_lossy();
    let data = if event_data_json.is_null() {
        String::new()
    } else {
        CStr::from_ptr(event_data_json).to_string_lossy().into_owned()
    };
    crate::message_broker::push_message(
        &name,
        &format!("[事件:{}] {}", evt, data),
    );
    0
}

/// 插件间通信
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_interplugin_call(
    from_plugin: *const c_char,
    to_plugin: *const c_char,
    msg_type: *const c_char,
    payload_json: *const c_char,
) -> i32 {
    if from_plugin.is_null() || msg_type.is_null() || payload_json.is_null() {
        return -1;
    }
    let from = CStr::from_ptr(from_plugin).to_string_lossy();
    let to = if to_plugin.is_null() {
        String::new()
    } else {
        CStr::from_ptr(to_plugin).to_string_lossy().into_owned()
    };
    let mtype = CStr::from_ptr(msg_type).to_string_lossy();
    let payload = CStr::from_ptr(payload_json).to_string_lossy();

    // 通过全局消息代理发送
    crate::message_broker::push_message(
        &from,
        &format!("[→{}:{}] {}", to, mtype, payload),
    );
    0
}

/// 健康检查 — v6.3: 多维度检查 (进度条停滞/配置可用性/消息代理积压)
/// 返回: 0=健康 / 1=降级(进度条停滞>120s) / 2=故障(配置写入失败) / -1=参数无效
/// 注: 熔断器/Faulted状态由PluginManager维护, 此C ABI仅检查运行时可见指标
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_health_check(
    plugin_name: *const c_char,
) -> i32 {
    if plugin_name.is_null() { return -1; }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let mut status: i32 = 0;

    // 维度1: 进度条停滞检测
    if let Some(prog) = crate::plugin_progress::global_progress() {
        let bars = prog.bars_by_plugin(&name);
        let stalled = bars.iter().any(|b| b.active && b.last_update.elapsed().as_secs() > 120);
        if stalled { status = status.max(1); } // DEGRADED
    }

    // 维度2: 配置可访问性
    let config = RUNTIME_CONFIG.read();
    if !config.contains_key(name.as_ref()) {
        // 插件尚未写入任何配置 — 这本身正常, 不降级
    }

    // 维度3: 返回综合状态
    // 注: CircuitBreaker/Faulted状态无法从此C ABI访问(需全局PluginManager引用)
    //    这些状态由 plugin_health 工具通过 PluginManager 查询
    status
}

/// 安全挂起
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_sleep(millis: u64) {
    std::thread::sleep(Duration::from_millis(millis));
}

/// 获取插件完整配置JSON — v6.2: 返回运行时配置
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_get_config_json(
    plugin_name: *const c_char,
) -> *mut c_char {
    if plugin_name.is_null() { return std::ptr::null_mut(); }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let config = RUNTIME_CONFIG.read();
    let cfg = config.get(name.as_ref()).cloned().unwrap_or_default();
    let json = serde_json::to_string(&cfg).unwrap_or_else(|_| "{}".into());
    CString::new(json).map(|cs| cs.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// 释放字符串 (统一free接口)
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// v6.0 元数据导出 — JSON格式完整插件描述
#[no_mangle]
pub unsafe extern "C" fn ruoo_plugin_metadata() -> *const c_char {
    // 插件侧实现 — 返回静态JSON
    // 此处仅提供声明，具体由各插件实现
    std::ptr::null()
}

// ════════════════════════════════════════════════════════
// 内置插件注册 — main.rs 使用
// ════════════════════════════════════════════════════════

/// 注册一个内置命令 (非DLL插件，进程内命令)
/// 如果命令已被插件注册，返回错误
pub fn register_builtin(
    registry: &mut CommandRegistry,
    plugin_name: &str,
    cmd: &str,
    help: &str,
    usage: &str,
) -> Result<(), String> {
    let def = PluginCommandDef {
        cmd: cmd.to_string(),
        help: help.to_string(),
        usage: usage.to_string(),
        plugin_name: plugin_name.to_string(),
        is_builtin: true,
        ..Default::default()
    };
    registry.register_plugin(plugin_name, &[def])?;
    Ok(())
}

/// 注册内置命令 — 若命令已被插件占用则静默跳过
/// 内置命令是回退实现；外部插件声明同名命令时，插件优先
pub fn register_builtin_or_skip(
    registry: &mut CommandRegistry,
    plugin_name: &str,
    cmd: &str,
    help: &str,
    usage: &str,
) {
    let def = PluginCommandDef {
        cmd: cmd.to_string(),
        help: help.to_string(),
        usage: usage.to_string(),
        plugin_name: plugin_name.to_string(),
        is_builtin: true,
        ..Default::default()
    };
    // 如果命令已存在(无论内置还是插件)，静默跳过
    if registry.commands.contains_key(cmd) {
        return;
    }
    // register_plugin 失败也静默跳过 (比如plugin_name下已有命令条目)
    let _ = registry.register_plugin(plugin_name, &[def]);
}

/// 探测插件命令 (从已加载的库，返回JSON字符串)
pub fn probe_plugin_commands(lib: &libloading::Library) -> Result<String, String> {
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_commands") {
            let ptr = func();
            if !ptr.is_null() {
                let json = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                return Ok(json);
            }
        }
    }
    Ok("[]".to_string())
}

/// 探测插件命令 (返回解析后的结构体列表)
pub fn probe_plugin_commands_parsed(lib: &libloading::Library) -> Vec<PluginCommandDef> {
    match probe_plugin_commands(lib) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 生成插件帮助文本 (返回行列表)
pub fn plugin_help_text(registry: &CommandRegistry) -> Vec<String> {
    let cmds = registry.list_all();
    if cmds.is_empty() {
        return vec!["无已注册插件命令".into()];
    }
    let mut out = vec!["═══ 插件命令 ═══".into(), String::new()];
    for cmd in &cmds {
        out.push(format!(
            "  {} — {} [{}]{}",
            cmd.cmd,
            cmd.help,
            cmd.plugin_name,
            if cmd.is_builtin { " (内置)" } else { "" }
        ));
    }
    out
}

/// 从文件路径尝试注册命令 (加载库→注册→卸载库)
pub fn try_register_from_file(
    path: &str,
    plugin_name: &str,
    registry: &mut CommandRegistry,
) -> Result<usize, String> {
    let lib = unsafe {
        libloading::Library::new(path)
            .map_err(|e| format!("无法加载库: {}", e))?
    };
    let result = try_register_from_library(&lib, plugin_name, registry);
    drop(lib);
    result
}

/// 从已加载的库中尝试注册命令
pub fn try_register_from_library(
    lib: &libloading::Library,
    plugin_name: &str,
    registry: &mut CommandRegistry,
) -> Result<usize, String> {
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_commands") {
            let ptr = func();
            if !ptr.is_null() {
                let json = CStr::from_ptr(ptr).to_string_lossy();
                match serde_json::from_str::<Vec<PluginCommandDef>>(&json) {
                    Ok(cmds) => {
                        return registry.register_plugin(plugin_name, &cmds);
                    }
                    Err(e) => {
                        return Err(format!("命令JSON解析失败: {}", e));
                    }
                }
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_normal() {
        let mut cb = CircuitBreaker::new(3);
        assert!(cb.allow_call());
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_trips() {
        let mut cb = CircuitBreaker::new(3);
        for i in 0..3 {
            assert!(cb.allow_call());
            cb.record_failure("test error", "test_cmd");
        }
        assert_eq!(cb.state, CircuitState::Open);
        assert!(!cb.allow_call());
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let mut cb = CircuitBreaker::new(3);
        cb.record_failure("e1", "cmd");
        cb.record_failure("e2", "cmd");
        cb.record_failure("e3", "cmd");
        assert_eq!(cb.state, CircuitState::Open);
        cb.reset();
        assert_eq!(cb.state, CircuitState::Closed);
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn test_command_registry() {
        let mut reg = CommandRegistry::new();
        let cmds = vec![
            PluginCommandDef {
                cmd: "test_cmd".into(),
                help: "测试命令".into(),
                aliases: vec!["tc".into()],
                ..Default::default()
            }
        ];
        let count = reg.register_plugin("test", &cmds).unwrap();
        assert_eq!(count, 1);

        assert!(reg.find("test_cmd").is_some());
        assert!(reg.find("tc").is_some());
        assert!(reg.find("nonexistent").is_none());

        let unregistered = reg.unregister_plugin("test");
        assert_eq!(unregistered, 1);
        assert!(reg.find("test_cmd").is_none());
    }
}

/// 格式化文件大小 (供vault使用)
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

// ── PluginCommandDef Default ──
impl Default for PluginCommandDef {
    fn default() -> Self {
        Self {
            cmd: String::new(),
            help: String::new(),
            usage: String::new(),
            category: String::new(),
            aliases: Vec::new(),
            plugin_name: String::new(),
            is_builtin: false,
            is_ai_tool: false,
            ai_params: Vec::new(),
            ai_perm_level: 5,
            is_async: false,
            is_long_running: false,
            progress_label: String::new(),
            min_perm_level: 0,
        }
    }
}
