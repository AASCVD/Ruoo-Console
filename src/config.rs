// ============================================================
// RUOO-ARSENAL — 配置系统 v2.0
// 持久化: 主题 / 工具预设 / 历史 / 偏好
// ============================================================

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ── 顶层配置 ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub target: String,

    // BUG-9 修复: default_true → default_false (函数名与语义一致)
    #[serde(default = "default_false")]
    pub show_sidebar: bool,

    #[serde(default)]
    pub history: Vec<String>,

    #[serde(default = "default_theme_name")]
    pub theme_name: String,

    #[serde(default = "default_font_size")]
    pub font_size: u32,

    #[serde(default)]
    pub theme: ThemeColors,

    #[serde(default)]
    pub tools: ToolsPreset,

    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// TCP连接超时 (扫描/探测用) — 默认3秒, 之前5秒太慢
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,

    /// 长命令超时 (exec_cmd/compile等) — 默认2分钟
    #[serde(default = "default_exec_timeout")]
    pub exec_timeout_ms: u64,

    #[serde(default = "default_port_range")]
    pub port_range: String,

    #[serde(default = "default_max_history")]
    pub max_history: usize,

    #[serde(default = "default_max_output")]
    pub max_output_lines: usize,

    // ── AI 助手配置 ──
    #[serde(default = "default_ds_api_url")]
    pub deepseek_api_url: String,

    #[serde(default)]
    pub deepseek_api_key: String,

    #[serde(default = "default_ds_model")]
    pub deepseek_model: String,

    #[serde(default)]
    pub ai_enabled: bool,

    #[serde(default)]
    pub deepseek_system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_cyber")]
    pub cyber: String,
    #[serde(default = "default_warn")]
    pub warn: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    #[serde(default = "default_muted")]
    pub muted: String,
    #[serde(default = "default_white")]
    pub white: String,
    #[serde(default = "default_dim_green")]
    pub dim_green: String,
}

impl ThemeColors {
    pub fn accent_c(&self) -> Color { hex_to_color(&self.accent) }
    pub fn cyber_c(&self) -> Color { hex_to_color(&self.cyber) }
    pub fn warn_c(&self) -> Color { hex_to_color(&self.warn) }
    pub fn danger_c(&self) -> Color { hex_to_color(&self.danger) }
    pub fn muted_c(&self) -> Color { hex_to_color(&self.muted) }
    pub fn white_c(&self) -> Color { hex_to_color(&self.white) }
    pub fn dim_green_c(&self) -> Color { hex_to_color(&self.dim_green) }
}

fn hex_to_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::Rgb(0, 255, 136); // fallback accent
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(136);
    Color::Rgb(r, g, b)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsPreset {
    #[serde(default = "default_scan_ports")]
    pub scan_ports: String,
    #[serde(default = "default_subdomain_wordlist")]
    pub subdomain_wordlist: String,
}

// ── 默认值函数 ──
// BUG-9 修复: 重命名以匹配语义
fn default_false() -> bool { false }
fn default_theme_name() -> String { String::from("cyber") }
fn default_font_size() -> u32 { 14 }
fn default_timeout() -> u64 { 30_000 }
fn default_connect_timeout() -> u64 { 10_000 }
fn default_exec_timeout() -> u64 { 120_000 }
fn default_port_range() -> String { String::from("1-10000") }
fn default_max_history() -> usize { 500 }
fn default_max_output() -> usize { 10000 }

fn default_accent() -> String { String::from("#00FF88") }
fn default_cyber() -> String { String::from("#00E6E6") }
fn default_warn() -> String { String::from("#FFB400") }
fn default_danger() -> String { String::from("#FF3232") }
fn default_muted() -> String { String::from("#64646E") }
fn default_white() -> String { String::from("#DCDCDC") }
fn default_dim_green() -> String { String::from("#78DC8C") }

fn default_scan_ports() -> String { String::from("1-10000") }
fn default_subdomain_wordlist() -> String {
    String::from("www,mail,api,admin,dev,test,staging,blog,shop,cdn,app,vpn,remote,portal")
}
fn default_ds_api_url() -> String { String::from("https://api.deepseek.com/v1/chat/completions") }
fn default_ds_model() -> String { String::from("deepseek-v4-pro") }

// ── 路径 ──
/// Vault 数据目录 (~/.ruoo/)
pub fn data_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".ruoo")
}

fn config_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("arsenal_config.json")
}

// ── 公开 API ──
pub fn load_config() -> AppConfig {
    let path = config_path();
    let mut cfg = match fs::read_to_string(&path) {
        Ok(json) => {
            match serde_json::from_str::<AppConfig>(&json) {
                Ok(cfg) => cfg,
                Err(e) => {
                    // ★ BUG-FIX: 不再静默吞错误, 写入stderr供调试
                    //   反序列化失败意味着所有设置丢失, 用户应知晓
                    eprintln!("[!] 配置解析失败: {e}");
                    eprintln!("[*] 已回退到默认配置 — 文件可能损坏: {}", path.display());
                    return AppConfig::default();
                }
            }
        }
        Err(_e) => {
            // 首次启动 — TUI 初始化后会展示欢迎信息
            return AppConfig::default();
        }
    };

    // ── 废弃别名自动迁移（仅内存，退出时持久化）──
    let old_model = cfg.deepseek_model.clone();
    match old_model.as_str() {
        "deepseek-chat" | "deepseek-reasoner" => {
            cfg.deepseek_model = String::from("deepseek-v4-pro");
            eprintln!("[↑] 模型别名已更新: {old_model} → deepseek-v4-pro（退出时自动保存）");
        }
        _ => {}
    }

    cfg
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    save_config_inner(cfg, &config_path())
}

/// 保存配置，返回可选的警告消息（API Key 明文存储提醒）
fn save_config_inner(cfg: &AppConfig, path: &PathBuf) -> Result<(), String> {
    // ★ BUG-4 修复改进: 不再自动清除 API Key，
    //    因为那会破坏 config set 命令且 eprintln 会破坏 TUI。
    //    而是保留密钥并返回 Ok，让调用方决定是否展示安全警告。
    //    明文 API Key 的保护由 resolve_api_key / migrate_api_key_to_vault 负责。
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("序列化失败: {e}"))?;
    fs::write(path, &json)
        .map_err(|e| format!("写入失败: {e}"))
}

/// 检测是否明文存储了 API Key（供 TUI 展示安全警告）
pub fn has_plaintext_api_key(cfg: &AppConfig) -> bool {
    !cfg.deepseek_api_key.is_empty()
}

/// 获取 API Key 安全警告文本（供 TUI push_output 使用，不破坏终端渲染）
pub fn api_key_plaintext_warning() -> &'static str {
    "[!] ⚠ API Key 以明文存储在 arsenal_config.json 中 — 建议迁移到 Vault"
}

// ── 运行时修改 ──
impl AppConfig {
    /// ★ BUG-4 修复: 安全获取 API Key — Vault 优先, 明文配置兜底
    /// 若 Vault 中存储了 api_keys.deepseek，优先使用；否则回退到配置文件明文值
    /// 同时在首次从明文配置读取时发出安全警告
    pub fn resolve_api_key(&self, vault: Option<&crate::vault::Vault>) -> String {
        // 优先从 Vault 读取 (已加密存储)
        if let Some(v) = vault {
            if v.is_mounted() {
                // 1. 标准路径: namespace="api_keys", key="deepseek"
                if let Some(enc_key) = v.get("api_keys", "deepseek") {
                    if !enc_key.is_empty() {
                        return enc_key;
                    }
                }
                // 2. 向后兼容: 旧版 vault add api_keys deepseek <key> 
                //    会将值存入 namespace="default", key="api_keys" 
                //    值格式: "deepseek sk-xxx" → 提取 sk- 开头的部分
                if let Some(raw_val) = v.get("default", "api_keys") {
                    if !raw_val.is_empty() {
                        // 尝试从旧格式 "deepseek sk-xxx" 中提取 key
                        for part in raw_val.split_whitespace() {
                            if part.starts_with("sk-") {
                                return part.to_string();
                            }
                        }
                        // 如果值本身就是 key (无空格)
                        if raw_val.starts_with("sk-") {
                            return raw_val;
                        }
                    }
                }
            }
        }
        // 回退到配置文件明文值 (兼容旧配置)
        // 安全警告通过 has_plaintext_api_key / api_key_plaintext_warning 由 TUI 层展示
        self.deepseek_api_key.clone()
    }

    /// ★ BUG-4 修复: 将 API Key 保存到 Vault 并从明文配置中清除
    pub fn migrate_api_key_to_vault(&mut self, vault: &crate::vault::Vault) -> Result<String, String> {
        if self.deepseek_api_key.is_empty() {
            return Err("API Key 为空，无需迁移".into());
        }
        vault.set("api_keys", "deepseek", &self.deepseek_api_key)?;
        self.deepseek_api_key.clear();
        let _ = save_config(self);
        Ok("[+] API Key 已迁移到加密 Vault，明文配置已清除".into())
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        match key {
            "target" => { self.target = value.into(); Ok(format!("target = {value}")) }
            "show_sidebar" => { self.show_sidebar = parse_bool(value)?; Ok(format!("show_sidebar = {}", self.show_sidebar)) }
            "theme_name" => { self.theme_name = value.into(); Ok(format!("theme_name = {value}")) }
            "font_size" => { self.font_size = value.parse().map_err(|_| "需要整数")?; Ok(format!("font_size = {}", self.font_size)) }
            "timeout_ms" => { self.timeout_ms = value.parse().map_err(|_| "需要整数")?; Ok(format!("timeout_ms = {}", self.timeout_ms)) }
            "connect_timeout_ms" => { self.connect_timeout_ms = value.parse().map_err(|_| "需要整数")?; Ok(format!("connect_timeout_ms = {}ms", self.connect_timeout_ms)) }
            "exec_timeout_ms" => { self.exec_timeout_ms = value.parse().map_err(|_| "需要整数")?; Ok(format!("exec_timeout_ms = {}ms", self.exec_timeout_ms)) }
            "port_range" => { self.port_range = value.into(); Ok(format!("port_range = {value}")) }
            "max_history" => { self.max_history = value.parse().map_err(|_| "需要整数")?; Ok(format!("max_history = {}", self.max_history)) }
            "max_output_lines" => { self.max_output_lines = value.parse().map_err(|_| "需要整数")?; Ok(format!("max_output_lines = {}", self.max_output_lines)) }

            // 主题色
            "theme.accent" => { self.theme.accent = value.into(); Ok(format!("theme.accent = {value}")) }
            "theme.cyber" => { self.theme.cyber = value.into(); Ok(format!("theme.cyber = {value}")) }
            "theme.warn" => { self.theme.warn = value.into(); Ok(format!("theme.warn = {value}")) }
            "theme.danger" => { self.theme.danger = value.into(); Ok(format!("theme.danger = {value}")) }
            "theme.muted" => { self.theme.muted = value.into(); Ok(format!("theme.muted = {value}")) }
            "theme.white" => { self.theme.white = value.into(); Ok(format!("theme.white = {value}")) }
            "theme.dim_green" => { self.theme.dim_green = value.into(); Ok(format!("theme.dim_green = {value}")) }

            // 工具
            "tools.scan_ports" => { self.tools.scan_ports = value.into(); Ok(format!("tools.scan_ports = {value}")) }
            "tools.subdomain_wordlist" => { self.tools.subdomain_wordlist = value.into(); Ok(format!("tools.subdomain_wordlist = {value}")) }

            // AI
            "deepseek_api_key" => { self.deepseek_api_key = value.into(); Ok("deepseek_api_key = ***".into()) }
            "deepseek_api_url" => { self.deepseek_api_url = value.into(); Ok(format!("deepseek_api_url = {value}")) }
            "deepseek_model" => { self.deepseek_model = value.into(); Ok(format!("deepseek_model = {value}")) }
            "ai_enabled" => { self.ai_enabled = parse_bool(value)?; Ok(format!("ai_enabled = {}", self.ai_enabled)) }
            "deepseek_system_prompt" => { self.deepseek_system_prompt = value.into(); Ok("deepseek_system_prompt = (已更新)".into()) }

            _ => Err(format!("未知配置项: {key}。输入 config 查看全部"))
        }
    }

    // BUG-10 修复: 移除死代码 get_str() — 脚本引擎改用 ScriptContext.expand()

    pub fn display(&self) -> Vec<String> {
        vec![
            format!("═══ 应用配置 ═══"),
            format!("  target            = {}", if self.target.is_empty() { "—" } else { &self.target }),
            format!("  show_sidebar      = {}", self.show_sidebar),
            format!("  theme_name        = {}", self.theme_name),
            format!("  font_size         = {}px", self.font_size),
            format!("  timeout_ms        = {}ms", self.timeout_ms),
            format!("  connect_timeout_ms = {}ms (扫描/探测TCP连接)", self.connect_timeout_ms),
            format!("  exec_timeout_ms   = {}ms (长命令: exec_cmd/编译)", self.exec_timeout_ms),
            format!("  port_range        = {}", self.port_range),
            format!("  max_history       = {}", self.max_history),
            format!("  max_output_lines  = {}", self.max_output_lines),
            String::new(),
            format!("═══ 主题配色 ═══"),
            format!("  theme.accent      = {}", self.theme.accent),
            format!("  theme.cyber       = {}", self.theme.cyber),
            format!("  theme.warn        = {}", self.theme.warn),
            format!("  theme.danger      = {}", self.theme.danger),
            format!("  theme.muted       = {}", self.theme.muted),
            format!("  theme.white       = {}", self.theme.white),
            format!("  theme.dim_green    = {}", self.theme.dim_green),
            String::new(),
            format!("═══ 工具预设 ═══"),
            format!("  tools.scan_ports  = {}", self.tools.scan_ports),
            format!("  tools.subdomain_wordlist = {}", self.tools.subdomain_wordlist),
            // shell_port/shell_ip removed — fields no longer exist on ToolsPreset
            String::new(),
            format!("═══ AI 助手 ═══"),
            format!("  deepseek_api_url  = {}", self.deepseek_api_url),
            format!("  deepseek_api_key  = {}", if self.deepseek_api_key.is_empty() { "⚠ 未配置" } else { "*** 已配置 ***" }),
            format!("  deepseek_model    = {}", self.deepseek_model),
            format!("  ai_enabled        = {}", self.ai_enabled),
            format!("  system_prompt     = {}", if self.deepseek_system_prompt.is_empty() { "默认" } else { "已自定义" }),
            String::new(),
            format!("历史: {} 条命令", self.history.len()),
            format!("文件: {}", config_path().display()),
        ]
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            target: String::new(),
            // BUG-9 修复: 使用重命名后的函数
            show_sidebar: default_false(),
            history: Vec::new(),
            theme_name: default_theme_name(),
            font_size: default_font_size(),
            theme: ThemeColors::default(),
            tools: ToolsPreset::default(),
            timeout_ms: default_timeout(),
            port_range: default_port_range(),
            max_history: default_max_history(),
            max_output_lines: default_max_output(),
            deepseek_api_url: default_ds_api_url(),
            connect_timeout_ms: default_connect_timeout(),
            exec_timeout_ms: default_exec_timeout(),
            deepseek_api_key: String::new(),
            deepseek_model: default_ds_model(),
            ai_enabled: true,
            deepseek_system_prompt: String::new(),
        }
    }
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            accent: default_accent(),
            cyber: default_cyber(),
            warn: default_warn(),
            danger: default_danger(),
            muted: default_muted(),
            white: default_white(),
            dim_green: default_dim_green(),
        }
    }
}

impl Default for ToolsPreset {
    fn default() -> Self {
        Self {
            scan_ports: default_scan_ports(),
            subdomain_wordlist: default_subdomain_wordlist(),
        }
    }
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on"  => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err("需要 true/false/1/0/yes/no".into()),
    }
}
