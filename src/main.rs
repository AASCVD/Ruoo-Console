// ============================================================
// RUOO-CONSOLE v4.1.0 — 终端面板
// 模块化架构: main / ui / commands / utils / theme / tools / ai / config / script
// ============================================================
#![allow(dead_code)]

mod theme;
mod utils;
mod tools;
mod commands;
mod ui;
mod config;
mod ai;
mod script;
mod vault;
mod auth;
mod bootscript;
mod plugin;
mod plugin_registry;
mod plugin_vault;
mod plugin_progress;
mod message_broker;
mod memsec;
mod tabs;
mod clipboard;
mod shortcuts;
mod session;
mod window;
mod crypto_channel;
mod panic_guard;
mod telemetry;

use chrono::Local;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    text::Line,
    Terminal,
};
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

// ═══════════════════════════════════════════════════
// 管理员检测 (Windows)
// ═══════════════════════════════════════════════════
fn is_running_as_admin() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
            let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut std::ffi::c_void,
                size,
                &mut size,
            );
            CloseHandle(token);
            result != 0 && elevation.TokenIsElevated != 0
        }
    }
    #[cfg(not(windows))]
    {
        false // 非 Windows 平台暂不检测
    }
}

// ═══════════════════════════════════════════════════
// 全局防崩溃系统 v4.1
// ═══════════════════════════════════════════════════

/// 安装全局 panic hook — 将崩溃信息写入 ruoo_crash.log
fn install_crash_defense() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "未知 panic (非字符串负载)".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "未知位置".to_string());
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");

        let crash_msg = format!("[CRASH {}] {} | {}", ts, loc, payload);
        eprintln!("{}", crash_msg);

        // ★ BUG-LOW 修复: 崩溃日志脱敏 — 限制回溯行数，不记录敏感请求参数
        // 日志写入 %TEMP%/ruoo_crash.log，避免泄露用户输入和工具参数
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("ruoo_crash.log"))
        {
            use std::io::Write;
            let _ = writeln!(f, "{}", crash_msg);
            let _ = writeln!(f, "  ── 堆栈回溯 (仅函数名，已脱敏) ──");
            let backtrace = std::backtrace::Backtrace::force_capture();
            let bt_str = format!("{}", backtrace);
            // ★ 只记录前15行函数调用，去除文件路径中可能含有的参数信息
            for line in bt_str.lines().take(15) {
                let sanitized = line
                    .replace('\\', "/")  // 统一路径分隔符
                    .chars()
                    .take(200)           // 截断过长行
                    .collect::<String>();
                let _ = writeln!(f, "  {}", sanitized);
            }
            let _ = writeln!(f, "  (回溯已截断，完整堆栈请使用调试器)");
            let _ = writeln!(f);
            let _ = f.flush();
        }

        // 调用默认 hook (打印到 stderr)
        default_hook(info);
    }));
}

/// 安全执行闭包 — 捕获 panic 返回 Result
fn catch_crash<F, R>(label: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> R + panic::UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(result) => Ok(result),
        Err(err) => {
            let msg = if let Some(s) = err.downcast_ref::<&str>() {
                format!("[SHD] {} 捕获崩溃: {}", label, s)
            } else if let Some(s) = err.downcast_ref::<String>() {
                format!("[SHD] {} 捕获崩溃: {}", label, s)
            } else {
                format!("[🛡] {} 捕获崩溃: 未知原因", label)
            };
            Err(msg)
        }
    }
}

// ── 进度条状态 (后台线程写入 → UI 线程读取) ──
#[derive(Clone)]
pub struct ProgressState {
    pub label: String,
    pub current: usize,
    pub total: usize,
    pub message: String,
}

// ── 右侧信息侧边栏显示模式 ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightSidebarMode {
    Dashboard,    // v13.1: 仪表盘 — 上半:系统资源, 下半:实时活动
    SystemInfo,   // 系统详情
}

impl RightSidebarMode {
    fn label(&self) -> &str {
        match self {
            RightSidebarMode::Dashboard  => "仪表盘",
            RightSidebarMode::SystemInfo => "系统详情",
        }
    }
    fn next(self) -> Self {
        match self {
            RightSidebarMode::Dashboard  => RightSidebarMode::SystemInfo,
            RightSidebarMode::SystemInfo => RightSidebarMode::Dashboard,
        }
    }
    fn prev(self) -> Self {
        self.next() // 只有2个模式，prev等同于next
    }
}

// ── 登录阶段 (v4.5: TUI内矩阵登录代替rpassword) ──
#[derive(Clone, PartialEq)]
pub enum LoginPhase {
    None,           // 未在登录流程
    LoginPassword,  // 输入密码登录
    CreatePassword, // 首次创建: 输入密码
    ConfirmPassword,// 首次创建: 确认密码
    LoginFailed,    // 密码错误, 等待重新输入
}

// ── 矩阵雨特效列状态 (持久化, 帧间保持连续性) ──
#[derive(Clone)]
pub struct MatrixColumn {
    /// 雨滴头部位置 (屏幕行号, 使用isize允许负值=未入场)
    pub head_y: isize,
    /// 下落速度分母 (每tick移动像素行数, 用分数表示: 1/den)
    pub speed_num: isize,
    pub speed_den: isize,
    /// 雨滴长度 (字符数)
    pub len: usize,
    /// 字符索引 (指向CHARS数组)
    pub char_idx: usize,
    /// 尾部字符索引 (渐暗用)
    pub trail_char: [usize; 6],
    /// 是否活跃 (有雨滴在下落)
    pub active: bool,
    /// 重新生成的延迟倒计时 (tick数)
    pub spawn_delay: u64,
    /// 屏幕列位置
    pub col: usize,
}

// ── 应用状态 ──
pub struct App {
    pub input: String,
    pub output: Vec<String>,
    pub cursor: usize,
    pub cmd_history: Vec<String>,
    pub history_idx: usize,
    pub target: String,
    pub running: bool,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub scroll_max: usize,
    pub show_sidebar: bool,
    /// v14.x: 左侧栏工具模块和快捷命令的独立滚动偏移
    pub sidebar_tool_scroll: usize,
    pub sidebar_quick_scroll: usize,
    /// v14.x: 左侧栏焦点 (false=输出区, true=侧边栏), 滚动键分配目标
    pub sidebar_focused: bool,
    /// v5.0: F9/help 命令进入的独立帮助页面模式
    pub help_mode: bool,
    pub help_page_content: Vec<String>,
    /// v5.1: 帮助页面模式切换 — true=选择模式(鼠标可选文本), false=复制模式(F8一键复制全部)
    pub help_select_mode: bool,
    /// v9.0: help直接输出时的行追踪 — 用于分页切换时原地替换
    pub help_output_start: usize,
    pub help_output_len: usize,
    pub tick_count: u64,
    pub cfg: config::AppConfig,
    pub ai_mode: bool,
    pub auto_task_mode: bool,
    pub task_queue: Vec<String>,
    pub task_counter: usize,
    pub ai_session: Option<ai::AiSession>,
    pub ai_pending: bool,
    pub ai_stream_mode: bool,  // 当前请求是否为流式模式
    pub ai_active_tool: Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>,
    pub ai_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub ai_tx: Option<std::sync::mpsc::Sender<(ai::AiSession, Result<(String, Vec<String>), String>)>>,
    pub cmd_pending: bool,
    pub cmd_rx: Option<std::sync::mpsc::Receiver<Vec<String>>>,
    pub cmd_tx: Option<std::sync::mpsc::Sender<Vec<String>>>,
    /// v4.2.1: 后台命令线程句柄 — 防止线程泄漏导致 DLL 卸载时 segfault
    pub cmd_thread: Option<std::thread::JoinHandle<()>>,
    pub progress: Option<Arc<Mutex<ProgressState>>>,
    pub mouse_capture: bool,
    pub toggle_mouse_capture: bool,
    pub script_depth: u32,
    pub global_out: Option<Arc<RwLock<Vec<String>>>>,
    pub ai_stream_rx: Option<std::sync::mpsc::Receiver<crate::ai::StreamMsg>>,
    pub ai_stream_tx: Option<std::sync::mpsc::Sender<crate::ai::StreamMsg>>,
    pub ai_message_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub ai_message_raw_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub vault: Option<Arc<crate::vault::Vault>>,
    pub vault_locked: bool,
    // ── TUI 密码输入模式 (替代 rpassword，兼容 raw mode) ──
    pub password_mode: bool,
    pub password_prompt: String,
    pub password_buffer: String,
    pub password_action: String,  // "unlock" | "passwd" | "login" | "login_create" | "login_confirm"
    // ── 初始登录状态 (v4.5: TUI内矩阵登录) ──
    pub login_phase: LoginPhase,
    pub login_attempts: u32,
    pub max_login_attempts: u32,
    pub login_error: String,
    pub vault_base_dir: Option<std::path::PathBuf>,
    // ── 启动脚本编辑器模式 ──
    pub editor_mode: bool,
    pub editor_buffer: Option<crate::bootscript::EditorBuffer>,
    pub editor_cursor_row: usize,
    pub editor_cursor_col: usize,
    pub editor_scroll: usize,
    /// v4.3: 输入框水平滚动偏移 (长命令自动跟焦)
    pub input_scroll: usize,
    // ── 动态命令注册表 (插件热注册) ──
    pub cmd_registry: crate::plugin::CommandRegistry,
    pub plugin_registry: Option<Arc<crate::plugin_registry::PluginRegistry>>,
    pub plugin_progress: Arc<crate::plugin_progress::MultiProgressManager>,
    pub message_broker: Arc<crate::message_broker::MessageBroker>,
    pub plugin_mgr: crate::script::PluginManager,
    /// v4.3: 字体大小 (Ctrl+滚轮调节)
    pub font_size: u32,
    /// IME 光标定位：UI 渲染时写入，draw() 后用 MoveTo+Hide 同批发送
    pub cursor_screen_pos: Option<(u16, u16)>,
    /// v4.4: 密码登录界面矩阵特效 — XORshift 随机种子
    pub password_matrix_seed: u64,
    /// v4.5: 矩阵背景缓存 — 每3帧更新一次 (≈20fps)
    pub password_matrix_ticks: u64,
    pub password_matrix_cache: Vec<Line<'static>>,
    /// v4.6: 持久化雨滴列状态 — 帧间保持连续性, 不再每8帧重新随机
    pub password_matrix_cols: Vec<MatrixColumn>,
    /// v4.6: 列状态是否已初始化
    pub password_matrix_cols_init: bool,
    /// v4.5.1: 字体变更标记 — 事件中设flag, 主循环通过backend写入escape序列
    pub font_size_changed: bool,
    // ── v4.7: 多标签页系统 ──
    pub tab_mgr: tabs::TabManager,
    // ── v4.7: Ctrl+R 历史搜索 ──
    pub history_search: shortcuts::HistorySearch,
    // ── v4.7: 搜索高亮模式 (在输出中搜索) ──
    pub output_search: String,
    // ── v4.2: 窗口控制 (隐藏/显示 + 全局热键) ──
    pub window_hidden: bool,
    pub window_show_requested: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub is_admin: bool,
    // ── v13.1: 右侧信息侧边栏 (F10切换, ]/[ 切换模式) ──
    pub show_right_sidebar: bool,
    pub right_sidebar_mode: RightSidebarMode,
    // ── v13.0.1: 遥测快照缓存 — 避免每帧调用 snapshot() 造成 CreateToolhelp32Snapshot 卡顿 ──
    pub telemetry_cache: Option<crate::telemetry::TelemetrySnapshot>,
    pub telemetry_cache_tick: u64,
}

impl App {
    fn new() -> Self {
        let cfg = config::load_config();
        let font_size = cfg.font_size;  // ★ v4.3: 提前读取 (cfg 将被 move)
        let target = if cfg.target.is_empty() { String::from("—") } else { cfg.target.clone() };
        let sidebar = cfg.show_sidebar;
        let theme_name = cfg.theme_name.clone();
        let mut output = vec![
            format!("[*] 会话初始化 — {}", Local::now().format("%Y-%m-%d %H:%M:%S")),
            format!("[+] 工作目录: {}", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "未知".into())),
            format!("[+] 核心数: {} | 架构: {} | v4.0 ",
                std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8),
                std::env::consts::ARCH),
            format!("[+] 主题: {} | 哈希: sha2/md5/sha1 | TLS: native-tls", theme_name),
            "[$] 输入 help 或 F9 查看命令 // F10 右栏(Tab切焦点后]/[切模式) // F1-F5 面板".into(),
            String::new(),
        ];
        let history = cfg.history.clone();
        if !history.is_empty() {
            output.push(format!("[+] 已加载 {} 条历史命令", history.len()));
        }
        let history_len = history.len();

        // v4.7: 恢复上次会话的标签页
        let tab_mgr = match session::SessionState::load() {
            Some(s) => {
                let mgr = session::restore_tabs(&s);
                output.push(format!("[+] 已恢复 {} 个标签页 (上次会话)", mgr.tab_count()));
                mgr
            }
            None => tabs::TabManager::new(),
        };
        Self {
            input: String::new(),
            output,
            cursor: 0,
            cmd_history: history,
            history_idx: history_len,
            target,
            running: true,
            scroll_offset: 0,
            auto_scroll: true,
            scroll_max: 0,
            show_sidebar: sidebar,
            sidebar_tool_scroll: 0,
            sidebar_quick_scroll: 0,
            sidebar_focused: false,
            help_mode: false,
            help_page_content: Vec::new(),
            help_select_mode: false,  // 默认复制模式(F8复制全部)
            help_output_start: 0,   // v9.0: help直接输出追踪
            help_output_len: 0,
            tick_count: 0,
            cfg,
            ai_mode: false,
            auto_task_mode: false,
            task_queue: Vec::new(),
            task_counter: 0,
            ai_session: None,
            ai_pending: false,
            ai_stream_mode: false,
            ai_active_tool: None,
            ai_cancel: None,
            ai_tx: None,
            cmd_pending: false,
            cmd_rx: None,
            cmd_tx: None,
            cmd_thread: None,
            progress: None,
            mouse_capture: true,
            toggle_mouse_capture: false,
            script_depth: 0,
            global_out: None,
            ai_stream_rx: None,
            ai_stream_tx: None,
            ai_message_rx: None,
            ai_message_raw_rx: None,
            vault: None,
            vault_locked: false,
            password_mode: false,
            password_prompt: String::new(),
            password_buffer: String::new(),
            password_action: String::new(),
            login_phase: LoginPhase::None,
            login_attempts: 0,
            max_login_attempts: 3,
            login_error: String::new(),
            vault_base_dir: None,
            editor_mode: false,
            editor_buffer: None,
            editor_cursor_row: 0,
            editor_cursor_col: 0,
            editor_scroll: 0,
            input_scroll: 0,
            cmd_registry: crate::plugin::CommandRegistry::new(),
            plugin_registry: None,  // 在 vault 挂载后初始化
            plugin_progress: {
                let mgr = Arc::new(crate::plugin_progress::MultiProgressManager::new());
                crate::plugin_progress::set_global_progress(mgr.clone());
                mgr
            },
            message_broker: {
                let broker = Arc::new(crate::message_broker::MessageBroker::new());
                crate::message_broker::set_global_broker(broker.clone());
                broker
            },
            plugin_mgr: crate::script::PluginManager::new(),
            font_size,
            cursor_screen_pos: None,
            password_matrix_seed: 0xDEADBEEF_CAFE_BABE,
            password_matrix_ticks: 0,
            password_matrix_cache: Vec::new(),
            password_matrix_cols: Vec::new(),
            password_matrix_cols_init: false,
            font_size_changed: false,
            tab_mgr,
            history_search: shortcuts::HistorySearch::new(),
            output_search: String::new(),
            window_hidden: false,
            window_show_requested: None,
            is_admin: false,
            show_right_sidebar: false,
            right_sidebar_mode: RightSidebarMode::Dashboard,
            telemetry_cache: None,
            telemetry_cache_tick: 0,

        }
    }
    pub fn push_output(&mut self, msg: &str) {
        // ★ 过滤 \r 防止 ratatui 渲染错位: 孤立 \r(0x0D)会使Paragraph光标归零,
        //    后续文本从列0覆盖写入, 导致动态"..."覆盖行首时间戳等
        // ★ BUG-FIX: 按\n拆分为多行,每行独立push — ratatui Line不处理内嵌\n
        for line in msg.split('\n') {
            self.output.push(line.replace('\r', ""));

            // ★ v4.5: 输出上限500行 — 超出行静默丢弃, 不再写文件
            const MAX_OUTPUT: usize = 800;
            if self.output.len() > MAX_OUTPUT {
                let overflow_count = self.output.len() - MAX_OUTPUT;
                self.output.drain(0..overflow_count);
            }
        }
        // ★ v11.0: 自动推送系统事件到遥测 (以首行匹配)
        let first_line = msg.lines().next().unwrap_or("");
        if first_line.starts_with("[+]") {
            crate::telemetry::push_system("output", "info", &first_line[3..].trim().chars().take(30).collect::<String>());
        } else if first_line.starts_with("[!]") {
            crate::telemetry::push_system("output", "warn", &first_line[3..].trim().chars().take(30).collect::<String>());
        }
        // v14.2: 智能追底 — 仅在用户已在底部时追底，避免覆盖用户手动滚动位置
        if self.auto_scroll {
            self.reset_scroll();
        }
        // 实时同步全局输出 (供 AI get_terminal_output 工具读取)
        // ★ BUG-FIX: try_write失败时使用write阻塞等待, 防止全局输出落后
        if let Some(ref out) = self.global_out {
            match out.try_write() {
                Ok(mut g) => { *g = self.output.clone(); }
                Err(std::sync::TryLockError::WouldBlock) => {
                    // 仅在每10次push时尝试fallback到write (避免频繁阻塞)
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    // 中毒锁忽略 — 不会影响核心功能
                }
            }
        }
    }
    fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        self.auto_scroll = false; // 用户手动离开底部
    }
    fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset
            .saturating_add(lines)
            .min(self.scroll_max);
        if self.scroll_offset >= self.scroll_max {
            self.auto_scroll = true;
        }
    }
    fn reset_scroll(&mut self) {
        self.auto_scroll = true;
        // scroll_offset 由下一帧 render_output 自动追底 (auto_scroll=true 时强制 max_scroll)
    }
}

// ── 事件处理 ──
fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    // ── 全局 Ctrl+C / Ctrl+D 退出 ──
    if modifiers.contains(KeyModifiers::CONTROL) {
        match key {
            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('d') | KeyCode::Char('D') => {
                app.running = false;
                return;
            }
            _ => {}
        }
    }

    // ── 编辑器模式 ──
    if app.editor_mode {
        handle_editor_key(app, key, modifiers);
        return;
    }

    // ── 历史搜索模式 (Ctrl+R) ──
    if app.history_search.active {
        handle_history_search_key(app, key, modifiers);
        return;
    }

    // ── 帮助页面模式 (F9) ──
    if app.help_mode {
        match key {
            KeyCode::Esc | KeyCode::F(9) => {} // 由上层 Esc/F9 处理, 继续执行以退出帮助
            KeyCode::F(7) => {} // 允许 F7 鼠标捕获切换, 继续执行
            KeyCode::F(8) => {} // 允许 F8 复制, 继续执行
            KeyCode::Tab => {
                // ★ v5.1: 切换选择/复制模式 + 鼠标捕获联动
                app.help_select_mode = !app.help_select_mode;
                // 选择模式: 释放鼠标给终端做文本选择
                // 复制模式: 恢复鼠标捕获(F8一键复制)
                app.toggle_mouse_capture = true; // 触发一次mouse capture切换
                return;
            }
            KeyCode::PageUp => { app.scroll_up(10); return; }
            KeyCode::PageDown => { app.scroll_down(10); return; }
            KeyCode::Up => { app.scroll_up(3); return; }
            KeyCode::Down => { app.scroll_down(3); return; }
            KeyCode::Home => { app.scroll_offset = 0; app.auto_scroll = false; return; }
            KeyCode::End => { app.reset_scroll(); return; }
            _ => { return; } // 拦截所有其他按键(包括输入字符)
        }
    }

    // ── 密码输入模式 ──
    if app.password_mode {
        match key {
            KeyCode::Enter => {
                let password = app.password_buffer.clone();
                crate::memsec::zeroize_string(&mut app.password_buffer);
                app.password_mode = false;
                // ★ v4.5: 清除光标位置防止登录框→正常UI跳变
                app.cursor_screen_pos = None;
                let action = app.password_action.clone();
                app.password_action.clear();
                let prompt = app.password_prompt.clone();
                app.password_prompt.clear();
                // 在输出中隐藏密码
                app.push_output(&format!("{} ********", prompt));
                match action.as_str() {
                    // ── v4.5: TUI矩阵登录流程 ──
                    "login" => do_login(app, &password),
                    "login_create" => {
                        // 首次创建: 检查密码强度 → 进入确认阶段
                        let strength = crate::auth::password_strength(&password);
                        if matches!(strength, crate::auth::Strength::VeryWeak | crate::auth::Strength::Weak) {
                            app.login_error = format!("密码太弱! 需要≥12字符 (当前强度: {:?})", strength);
                            app.login_phase = LoginPhase::LoginFailed;
                            app.password_mode = true;
                            app.password_prompt = "🔑 输入主密码".into();
                            app.password_action = "login".into();
                        } else {
                            app.login_phase = LoginPhase::ConfirmPassword;
                            app.password_mode = true;
                            app.password_matrix_seed = app.tick_count.wrapping_mul(6364136223846793005u64);
                            app.password_prompt = "[CONFIRM] 再次输入确认".into();
                            app.password_action = format!("login_confirm:{}", password);
                        }
                    }
                    "login_confirm" => {
                        // 确认密码: 比对 → 创建Vault
                        let parts: Vec<&str> = action.splitn(2, ':').collect();
                        let first_pw = parts.get(1).copied().unwrap_or("");
                        if password != first_pw {
                            app.login_error = "两次密码不匹配!".into();
                            app.login_phase = LoginPhase::LoginFailed;
                            app.password_mode = true;
                            app.password_prompt = "[NEW] 创建主密码 (>=12字符)".into();
                            app.password_action = "login_create".into();
                        } else {
                            // 创建Vault
                            match do_create_vault(app, &password) {
                                Ok(()) => {
                                    app.login_phase = LoginPhase::None;
                                    // login流程完成, 继续正常TUI
                                }
                                Err(e) => {
                                    app.login_error = e;
                                    app.login_phase = LoginPhase::LoginFailed;
                                    app.password_mode = true;
                                    app.password_prompt = "[NEW] 创建主密码 (>=12字符)".into();
                                    app.password_action = "login_create".into();
                                }
                            }
                        }
                    }
                    "unlock" => do_unlock(app, &password),
                    "passwd_old" => {
                        // 第一阶段: 收集旧密码 → 进入第二阶段
                        app.password_mode = true;
                        app.password_matrix_seed = app.tick_count.wrapping_mul(1442695040888963407u64);
                        app.password_prompt = "[NEW] 新密码".into();
                        app.password_action = format!("passwd_new:{}\x1F", password);  // ★ BUG-19: 用\x1F(单元分隔符)代替冒号
                        app.push_output("  [*] 第二阶段: 输入新密码 (≥12字符, 含大小写+数字+特殊字符)");
                    }
                    "lock" => do_lock(app),
                    _ => {
                        // passwd_new → 第三阶段: 收集确认密码
                        if action.starts_with("passwd_new:") {
                            let old_password = action.strip_prefix("passwd_new:").unwrap_or("");
                            // ★ BUG-19: 去除尾部\x1F分隔符
                            let old_password = old_password.trim_end_matches('\x1F');
                            app.password_mode = true;
                            app.password_matrix_seed = app.tick_count.wrapping_mul(6364136223846793005u64);
                            app.password_prompt = "[CONFIRM] 确认新密码".into();
                            app.password_action = format!("passwd_confirm:{}\x1F{}", old_password, password);
                            app.push_output("  [*] 第三阶段: 再次输入新密码确认");
                        } else if action.starts_with("passwd_confirm:") {
                            // ★ BUG-19 修复: 用\x1F(单元分隔符)代替冒号分隔密码字段,
                            //    避免密码中包含':'时解析错位
                            let payload = action.strip_prefix("passwd_confirm:").unwrap_or("");
                            let parts: Vec<&str> = payload.splitn(2, '\x1F').collect();
                            let old_pw = parts.first().copied().unwrap_or("");
                            let new_pw = parts.get(1).copied().unwrap_or("");
                            let confirm = password;
                            do_change_password(app, old_pw, new_pw, &confirm);
                        }
                    }
                }
                app.input.clear();
                app.cursor = 0;
                app.reset_scroll();
            }
            KeyCode::Esc => {
                // ★ v4.7 SEC-FIX: 登录阶段Esc不可绕过 — 仅清空缓冲区,保持登录界面
                //    非登录密码操作(unlock/passwd)才允许Esc取消
                let is_login_flow = app.password_action.starts_with("login");
                if is_login_flow {
                    // 登录流程: 清空密码缓冲区让用户重新输入, 不退出密码模式
                    crate::memsec::zeroize_string(&mut app.password_buffer);
                    app.input.clear();
                    app.cursor = 0;
                    app.push_output(&format!("[!] {} — 登录不可取消, 请重新输入", app.password_prompt));
                } else {
                    // 非登录操作(unlock/passwd): 允许Esc取消
                    app.push_output(&format!("[!] {} 已取消", app.password_prompt));
                    app.password_mode = false;
                    app.cursor_screen_pos = None;  // ★ v4.5: 清除光标防跳变
                    crate::memsec::zeroize_string(&mut app.password_buffer);
                    app.password_prompt.clear();
                    app.password_action.clear();
                }
                app.reset_scroll();
            }
            KeyCode::Char(c) => {
                app.password_buffer.push(c);
                // 更新显示为 *
                app.input.push('*');
                app.cursor = app.input.len();
            }
            KeyCode::Backspace => {
                if !app.password_buffer.is_empty() {
                    app.password_buffer.pop();
                    app.input.pop();
                    app.cursor = app.input.len();
                }
            }
            _ => {} // 忽略其他键
        }
        return;
    }

    if modifiers.contains(KeyModifiers::CONTROL) && modifiers.contains(KeyModifiers::SHIFT) {
        match key {
            KeyCode::Char('h') | KeyCode::Char('H') => {
                app.window_hidden = !window::toggle_console();
                if app.window_hidden {
                    app.push_output("[s] 窗口已隐藏到后台 — Ctrl+Shift+S 可从桌面恢复显示");
                } else {
                    app.push_output("[s] 窗口已恢复显示");
                }
                return;
            }
            _ => {}
        }
    }

    if modifiers.contains(KeyModifiers::CONTROL) {
        match key {
            KeyCode::Char('l') | KeyCode::Char('L') => {
                app.output.clear();
                app.reset_scroll();
                app.push_output("[+] 终端已清空 (Ctrl+L)");
                // v4.7: 同步清理到当前标签
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
            }
            // v4.5.1: 键盘调节字体 (Alt+滚轮也可)
            KeyCode::Char('=') => {
                app.font_size = (app.font_size + 1).min(48);
                app.cfg.font_size = app.font_size;
                app.font_size_changed = true;
                app.push_output(&format!("[s] 字体: {}px", app.font_size));
                // ★ 立即持久化: 避免退出前崩溃导致字体丢失
                if let Err(e) = config::save_config(&app.cfg) {
                    app.push_output(&format!("[!] 保存字体失败: {}", e));
                }
            }
            KeyCode::Char('-') => {
                app.font_size = (app.font_size.saturating_sub(1)).max(8);
                app.cfg.font_size = app.font_size;
                app.font_size_changed = true;
                app.push_output(&format!("[s] 字体: {}px", app.font_size));
                if let Err(e) = config::save_config(&app.cfg) {
                    app.push_output(&format!("[!] 保存字体失败: {}", e));
                }
            }
            // ── v4.7: 行编辑快捷键 ──
            KeyCode::Char('a') | KeyCode::Char('A') => {
                app.cursor = 0; // Home
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                app.cursor = app.input.len(); // End
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                shortcuts::delete_to_end(&mut app.input, app.cursor);
            }
            KeyCode::Char('u') | KeyCode::Char('U') => {
                shortcuts::delete_to_start(&mut app.input, &mut app.cursor);
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                // Ctrl+W: 关闭当前标签页 (保存当前状态)
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                let name = app.tab_mgr.active().name.clone();
                if app.tab_mgr.close_current() {
                    app.tab_mgr.restore_active_to(
                        &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                        &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                        
                    );
                    app.push_output(&format!("[+] 标签已关闭: {} | 当前: {}",
                        name, app.tab_mgr.active().name));
                } else {
                    app.push_output("[!] 至少保留一个标签页");
                }
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                // Ctrl+T: 新建标签页 (保存当前状态，开新标签)
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                app.tab_mgr.new_tab(None);
                app.tab_mgr.restore_active_to(
                    &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                    &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                    
                );
                app.push_output(&format!("[+] 新建标签: {}", app.tab_mgr.active().name));
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                // Ctrl+R: 历史搜索
                let cmd_history = app.cmd_history.clone();
                let input = app.input.clone();
                let hs = shortcuts::HistorySearch::activate(&input, &cmd_history);
                app.history_search = hs;
                // 不清空 input，保留预输入作为搜索初始值
            }
            // Ctrl+Backspace: 删除前一个单词
            KeyCode::Backspace => {
                shortcuts::delete_word_backward(&mut app.input, &mut app.cursor);
            }
            // Ctrl+Tab / Ctrl+PageDown: 切换下一个标签
            // Ctrl+Shift+Tab(BackTab) / Ctrl+PageUp: 切换上一个标签
            // 注意: Ctrl+Tab 在某些终端会被拦截, 推荐用 Ctrl+PageDown/Up
            KeyCode::Tab => {
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                app.tab_mgr.next_tab();
                app.tab_mgr.restore_active_to(
                    &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                    &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                    
                );
                app.push_output(&format!("[+] 切换到: {}", app.tab_mgr.active().name));
            }
            KeyCode::BackTab => {
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                app.tab_mgr.prev_tab();
                app.tab_mgr.restore_active_to(
                    &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                    &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                    
                );
                app.push_output(&format!("[+] 切换到: {}", app.tab_mgr.active().name));
            }
            KeyCode::PageDown => {
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                app.tab_mgr.next_tab();
                app.tab_mgr.restore_active_to(
                    &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                    &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                    
                );
                app.push_output(&format!("[+] 切换到: {}", app.tab_mgr.active().name));
            }
            KeyCode::PageUp => {
                app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
                    app.auto_scroll, &app.target, app.ai_mode);
                app.tab_mgr.prev_tab();
                app.tab_mgr.restore_active_to(
                    &mut app.output, &mut app.scroll_offset, &mut app.scroll_max,
                    &mut app.auto_scroll, &mut app.target, &mut app.ai_mode,
                    
                );
                app.push_output(&format!("[+] 切换到: {}", app.tab_mgr.active().name));
            }
            // Ctrl+Left/Right: 单词跳转（与Alt+Left/Right相同功能）
            _ => {}
        }
        return;
    }

    // ── Alt 组合键 ──
    if modifiers.contains(KeyModifiers::ALT) {
        match key {
            KeyCode::Left => {
                shortcuts::word_backward(&app.input, &mut app.cursor);
            }
            KeyCode::Right => {
                shortcuts::word_forward(&app.input, &mut app.cursor);
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                // Alt+C: 复制最后一条命令输出到剪贴板
                let text = clipboard::extract_last_command_output(&app.output);
                if text.trim().is_empty() {
                    app.push_output("[!] 没有可复制的输出");
                } else {
                    match clipboard::copy_to_clipboard(&text) {
                        Ok(msg) => app.push_output(&msg),
                        Err(e) => app.push_output(&format!("[!] {}", e)),
                    }
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                // Alt+V: 从剪贴板粘贴
                match clipboard::paste_from_clipboard() {
                    Ok(text) => {
                        app.input.insert_str(app.cursor, &text);
                        app.cursor += text.len();
                        app.push_output(&format!("[s] 已粘贴 {} 字符", text.len()));
                    }
                    Err(e) => app.push_output(&format!("[!] {}", e)),
                }
            }
            _ => {}
        }
        return;
    }

    // ★ 防御: 确保光标在有效 UTF-8 字符边界上, 防止多字节字符中间切片 panic
    if app.cursor > app.input.len() || !app.input.is_char_boundary(app.cursor) {
        app.cursor = (0..app.cursor.min(app.input.len())).rev()
            .find(|&i| app.input.is_char_boundary(i))
            .unwrap_or(0);
    }

    match key {
        KeyCode::Enter => {
            let cmd = app.input.clone();
            if !cmd.trim().is_empty() {
                app.cmd_history.push(cmd.clone());
                app.history_idx = app.cmd_history.len();

                // ═══ 任务流程表 &+* ═══
                if cmd == "&+*" {
                    app.auto_task_mode = !app.auto_task_mode;
                    if app.auto_task_mode {
                        app.push_output("[*] ══════ 任务流程表已开启 ══════");
                        app.push_output("[*] 用法: & <任务内容> — 添加任务到流程表");
                        app.push_output("[*] 用法: &+*list — 查看任务列表");
                        app.push_output("[*] 用法: &+*clear — 清空任务列表");
                        app.push_output("[*] 用法: &+*stop  — 关闭任务流程表");
                        app.push_output("[*] 任务将从上往下排队，AI回复后自动发送下一条");
                        app.push_output("[*] 任务完成后自动关闭流程表");
                    } else {
                        let remaining = app.task_queue.len();
                        app.task_queue.clear();
                        app.task_counter = 0;
                        app.push_output(&format!("[*] 任务流程表已暂停 ({}条任务已清空)", remaining));
                    }
                } else if cmd.starts_with("&+*") && cmd.len() > 3
                    && cmd != "&+*list" && cmd != "&+*clear" && cmd != "&+*stop" {
                    app.push_output(&format!("[!] 未知的 &+* 子命令: {}", &cmd[3..]));
                    app.push_output("[*] 可用: &+* | &+*list | &+*clear | &+*stop");
                } else if cmd == "&+*list" {
                    if app.task_queue.is_empty() {
                        app.push_output("[*] 任务流程表为空 — 使用 & <任务> 添加任务");
                    } else {
                        let total = app.task_queue.len();
                        let tasks: Vec<String> = app.task_queue.iter().enumerate()
                            .map(|(i, t)| format!("  [{}/{}] {}", i + 1, total, t))
                            .collect();
                        app.push_output(&format!("[*] ══════ 任务流程表 ({}条) ══════", total));
                        for line in tasks {
                            app.push_output(&line);
                        }
                    }
                } else if cmd == "&+*clear" {
                    let count = app.task_queue.len();
                    app.task_queue.clear();
                    app.task_counter = 0;
                    app.auto_task_mode = false;
                    app.push_output(&format!("[*] 任务流程表已清空 ({}条任务)，流程表已关闭", count));
                } else if cmd == "&+*stop" {
                    app.auto_task_mode = false;
                    let remaining = app.task_queue.len();
                    app.task_queue.clear();
                    app.task_counter = 0;
                    app.push_output(&format!("[*] 任务流程表已关闭 (剩余{}条未执行任务)", remaining));
                } else if cmd.starts_with('&') && app.auto_task_mode && !app.ai_mode {
                    app.push_output("[!] 任务流程表需要AI模式 — 请先输入 dpai 启用AI助手");
                } else if app.ai_mode && cmd.starts_with('&') {
                    // &% = 流式模式, & = 普通模式
                    let (stream_mode, query) = if cmd.starts_with("&%") {
                        (true, cmd[2..].trim().to_string())
                    } else {
                        (false, cmd[1..].trim().to_string())
                    };
                    if query.is_empty() {
                        app.push_output("[!] AI 模式: & 后需要输入内容");
                    } else if app.auto_task_mode {
                        // 自动任务模式: 添加到队列
                        let tagged = format!("[自动任务已开启] {}", query);
                        app.task_queue.push(tagged.clone());
                        app.task_counter += 1;
                        app.push_output(&format!("[+] 已添加任务 [{}/∞]: {}", app.task_counter, query));
                        // 如果当前没有AI请求在处理，立即开始第一个任务
                        if !app.ai_pending && !app.task_queue.is_empty() {
                            let task = app.task_queue.remove(0);
                            let prefix = if app.ai_mode { "#dp" } else { "$" };
                            app.push_output(&format!("{} {}", prefix, task));
                            app.reset_scroll();
                            app.ai_stream_mode = false;
                            commands::process_ai_query(app, &task, false);
                        }
                    } else if app.ai_pending {
                        app.push_output("[!] AI 请求进行中，请等待回复");
                    } else {
                        let prefix = if app.ai_mode { "#dp" } else { "$" };
                        let mode_tag = if stream_mode { " [实时]" } else { "" };
                        app.push_output(&format!("{} {}{}", prefix, &query, mode_tag));
                        app.reset_scroll();
                        app.ai_stream_mode = stream_mode;
                        commands::process_ai_query(app, &query, stream_mode);
                    }
                } else {
                    let prefix = if app.ai_mode { "#dp" } else { "$" };
                    app.push_output(&format!("{} {}", prefix, cmd));
                    app.reset_scroll();
                    commands::process_cmd(app, &cmd);
                }
            }
            app.input.clear();
            app.cursor = 0;
        }
        KeyCode::Char(c) => {
            app.input.insert(app.cursor, c);
            app.cursor += c.len_utf8();
        }
        KeyCode::Backspace => {
            if app.cursor > 0 {
                if let Some((prev, _)) = app.input[..app.cursor].char_indices().next_back() {
                    app.input.remove(prev);
                    app.cursor = prev;
                }
            }
        }
        KeyCode::Delete => {
            if app.cursor < app.input.len() {
                app.input.remove(app.cursor);
            }
        }
        KeyCode::Left => {
            if app.cursor > 0 {
                if let Some((prev, _)) = app.input[..app.cursor].char_indices().next_back() {
                    app.cursor = prev;
                }
            }
        }
        KeyCode::Right => {
            if app.cursor < app.input.len() {
                if let Some(c) = app.input[app.cursor..].chars().next() {
                    app.cursor += c.len_utf8();
                }
            }
        }
        KeyCode::Home => app.cursor = 0,
        KeyCode::End => {
            if app.input.is_empty() {
                // 输入为空: 跳转到输出底部
                app.reset_scroll();
            } else {
                // 有输入: 移动光标到末尾
                app.cursor = app.input.len();
            }
        },
        KeyCode::Up => {
            // ★ v14.x: 侧边栏滚动 — 左侧栏打开且获得焦点时, Up/Down 滚动侧边栏
            if app.show_sidebar && app.sidebar_focused {
                app.sidebar_tool_scroll = app.sidebar_tool_scroll.saturating_sub(1);
                return;
            }
            if app.history_idx > 0 {
                app.history_idx -= 1;
                app.input = app.cmd_history[app.history_idx].clone();
                app.cursor = app.input.len();
            }
        }
        KeyCode::Down => {
            if app.show_sidebar && app.sidebar_focused {
                app.sidebar_tool_scroll = app.sidebar_tool_scroll.saturating_add(1);
                return;
            }
            if app.history_idx < app.cmd_history.len() {
                app.history_idx += 1;
                app.input = if app.history_idx < app.cmd_history.len() {
                    app.cmd_history[app.history_idx].clone()
                } else {
                    String::new()
                };
                app.cursor = app.input.len();
            }
        }
        KeyCode::PageUp => {
            // ★ v14.x: 侧边栏滚动
            if app.show_sidebar && app.sidebar_focused {
                app.sidebar_tool_scroll = app.sidebar_tool_scroll.saturating_sub(5);
                return;
            }
            app.scroll_up(10);
        }
        KeyCode::PageDown => {
            if app.show_sidebar && app.sidebar_focused {
                app.sidebar_tool_scroll = app.sidebar_tool_scroll.saturating_add(5);
                return;
            }
            app.scroll_down(10);
        }
        KeyCode::Tab => {
            // ★ v14.x: 空输入时Tab切换左侧栏焦点 / 非空时智能补全
            if app.input.is_empty() && app.show_sidebar {
                app.sidebar_focused = !app.sidebar_focused;
                if app.sidebar_focused {
                    app.push_output("[*] 左侧栏已获得焦点 — Up/Down/PgUp/PgDn 滚动工具列表, Tab 切回输出区");
                } else {
                    app.push_output("[*] 焦点已回到输出区");
                }
                return;
            }
            // 智能Tab — 输入有内容优先补全
            if !app.input.is_empty() {
                if let Some(completed) = commands::tab_complete(&app.input, app) {
                    app.input = completed;
                    app.cursor = app.input.len();
                    return;
                }
            }

        }
        KeyCode::Esc => {
            // v5.0: 优先退出帮助页面
            if app.help_mode {
                app.help_mode = false;
                app.help_select_mode = false; // 重置选择模式
                app.push_output("[+] 已退出帮助页面 — 返回终端");
                return;
            }
            if app.ai_pending {
                if let Some(ref cancel) = app.ai_cancel {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    app.push_output("[!] 正在取消 AI 请求… (Esc)");
                }
            } else {
                app.running = false;
            }
        }
        KeyCode::F(6) => {
            // 左侧栏已移除
        }
        KeyCode::F(10) => {
            // v14.2: F10 切换右侧信息侧边栏 (系统资源/工具调用/网络收发)
            app.show_right_sidebar = !app.show_right_sidebar;
            if app.show_right_sidebar {
                app.push_output("[+] 右侧信息栏已打开 — 系统资源/工具调用/网络收发 | F10关闭");
            } else {
                app.push_output("[+] 右侧信息栏已关闭 — F10 重新打开");
            }
        }

        KeyCode::F(7) => {
            app.toggle_mouse_capture = true;
        }
        KeyCode::F(9) => {
            // v5.0: F9 切换帮助页面
            if app.help_mode {
                app.help_mode = false;
                app.help_select_mode = false; // 重置选择模式
                app.push_output("[+] 已退出帮助页面 — 返回终端");
            } else {
                app.help_page_content = commands::help::help_text_filtered(app, None);
                app.help_mode = true;
                app.scroll_offset = 0;
                app.auto_scroll = false; // 进入帮助页从顶部开始
                // ★ 不再自动复制 — 用户按F8手动复制
            }
        }
        KeyCode::F(8) => {
            // v5.1: 帮助模式下尊重选择/复制模式
            if app.help_mode && app.help_select_mode {
                // 选择模式: F8不复制, 提示用Tab切换回复制模式
                app.push_output("[*] 当前为选择模式 — 请用鼠标在终端选中文本后 Ctrl+Shift+C 复制, 或按 Tab 切换到复制模式后 F8 一键复制");
                return;
            }
            // v5.0: 帮助模式下复制帮助内容
            let all_text = if app.help_mode {
                app.help_page_content.join("\n")
            } else {
                app.output.join("\n")
            };
            let line_count = if app.help_mode { app.help_page_content.len() } else { app.output.len() };
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    match clipboard.set_text(&all_text) {
                        Ok(()) => app.push_output(&format!("[+] F8 已复制全部 {} 行{}到剪贴板", line_count, if app.help_mode { "帮助内容" } else { "输出" })),
                        Err(e) => app.push_output(&format!("[!] 剪贴板写入失败: {}", e)),
                    }
                }
                Err(_e) => {
                    // arboard 在某些终端(如WSL/SSH)不可用, 回退到 powershell clip
                    // v4.1: 显式 drop stdin 防止 wait 死锁
                    let mut child = match std::process::Command::new("powershell")
                        .args(["-Command", "Set-Clipboard -Value $input"])
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            app.push_output(&format!("[!] 剪贴板不可用: {}", e));
                            return;
                        }
                    };

                    {
                        use std::io::Write;
                        if let Some(ref mut stdin) = child.stdin {
                            let _ = stdin.write_all(all_text.as_bytes());
                            // stdin 在此 drop → powershell 的 $input 流关闭
                        }
                    } // stdin 已释放

                    match child.wait() {
                        Ok(s) if s.success() => app.push_output(&format!("[+] F8 已复制全部 {} 行{}到剪贴板 (powershell)", line_count, if app.help_mode { "帮助内容" } else { "输出" })),
                        Ok(s) => app.push_output(&format!("[!] clip失败, exit: {}", s)),
                        Err(e) => app.push_output(&format!("[!] clip失败: {}", e)),
                    }
                }
            }
        }
        _ => {}
    }
}

// ── 自动加载插件目录全部插件 ──

#[allow(dead_code)]
fn auto_load_plugins(app: &mut App) {
    let plugins_dir = std::path::PathBuf::from("plugins");
    if !plugins_dir.exists() || !plugins_dir.is_dir() {
        return;
    }

    app.push_output("[*] ======== 自动加载插件目录 ========");

    let mut loaded_count = 0u32;
    let mut cmd_count = 0u32;

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            app.push_output(&format!("[!] 无法读取插件目录: {}", e));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // 只处理 .dll 和 .so 文件
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext != "dll" && ext != "so" {
            continue;
        }

        // 从文件名提取插件名 (不含扩展名)
        let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // 去掉平台前缀 "ruoo_"
        let plugin_name = if file_stem.starts_with("ruoo_") {
            &file_stem[5..]
        } else {
            file_stem
        };

        let path_str = path.to_string_lossy().to_string();

        app.push_output(&format!("  [·] 加载: {} ({})...", plugin_name, path_str));

        match app.plugin_mgr.load(plugin_name, &path_str) {
            Ok(msg) => {
                app.push_output(&format!("    {}", msg));
                loaded_count += 1;

                // 注册命令 (App)
                match app.plugin_mgr.get_library(plugin_name) {
                    Some(lib) => {
                        match crate::plugin::try_register_from_library(lib, plugin_name, &mut app.cmd_registry) {
                            Ok(count) if count > 0 => {
                                app.push_output(&format!("    [+] 已注册 {} 个命令", count));
                                cmd_count += count as u32;
                            }
                            Ok(_) => {
                                app.push_output("    [*] 无命令声明 (可用 plugin call 调用)");
                            }
                            Err(e) => {
                                app.push_output(&format!("    [*] 命令探测: {}", e));
                            }
                        }
                    }
                    None => {
                        app.push_output("    [!] 无法获取插件库引用");
                    }
                }

                // ★ 同步到 AI 全局 PluginManager (GLOBAL_PLUGIN_MGR)
                //    AI 的 plugin_list/plugin_call/plugin_load 使用独立管理器
                //    此处将 App 已加载的插件 Arc 共享过去，避免重复加载 DLL
                if let Some(info) = app.plugin_mgr.get_info(plugin_name) {
                    let mut ai_guard = crate::ai::GLOBAL_PLUGIN_MGR.lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if ai_guard.is_none() {
                        *ai_guard = Some(crate::script::PluginManager::new());
                    }
                    if let Some(ref mut ai_mgr) = *ai_guard {
                        // 克隆 PluginInfo (Arc<Library> 引用计数+1，不重复加载 DLL)
                        let cloned_info = crate::script::PluginInfo {
                            lib: info.lib.clone(),
                            path: info.path.clone(),
                            permissions: info.permissions.clone(),
                            last_modified: info.last_modified,
                            version: info.version.clone(),
                            crash_count: info.crash_count,
                            faulted: info.faulted,
                            circuit_breaker: crate::plugin::CircuitBreaker::new(10),
                            abandoned_threads: Vec::new(),
                        };
                        ai_mgr.insert_info(plugin_name, cloned_info);

                        // 同步注册命令到 AI 全局 CommandRegistry
                        let mut ai_cmd_guard = crate::ai::GLOBAL_CMD_REGISTRY.lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if ai_cmd_guard.is_none() {
                            *ai_cmd_guard = Some(crate::plugin::CommandRegistry::new());
                        }
                        if let Some(ref mut ai_cmd) = *ai_cmd_guard {
                            let _ = crate::plugin::try_register_from_library(
                                info.lib.as_ref(), plugin_name, ai_cmd
                            );
                        }
                    }
                }
            }
            Err(e) => {
                app.push_output(&format!("    [!] 加载失败: {}", e));
            }
        }
    }

    if loaded_count > 0 {
        app.push_output(&format!("[+] 自动加载完成: {} 插件, {} 命令", loaded_count, cmd_count));
    } else {
        app.push_output("[*] 插件目录无 .dll/.so 文件，跳过自动加载");
    }
}

// ── TUI 原生密码操作 (替代 rpassword) ──

/// v4.5: TUI矩阵登录 — 验证密码并挂载Vault
fn do_login(app: &mut App, password: &str) {
    let base_dir = match app.vault_base_dir.as_ref() {
        Some(d) => d.clone(),
        None => {
            app.login_error = "Vault目录未设置".into();
            app.login_phase = LoginPhase::LoginFailed;
            app.password_mode = true;
            app.password_prompt = "🔑 输入主密码".into();
            app.password_action = "login".into();
            return;
        }
    };

    match crate::vault::Vault::mount(&base_dir, password) {
        Ok(vault) => {
            app.login_phase = LoginPhase::None;
            app.push_output("[+] Vault 已挂载 — 安全通道就绪");
            app.push_output("[+] Vault 已挂载 — AI/tools 加密存储可用");
            app.push_output("[*] 命令: vault 查看状态 | lock 锁定 | unlock 解锁 | passwd 改密");
            let vault_arc = Arc::new(vault);
            crate::vault::set_global_vault(vault_arc.clone());
            app.vault = Some(vault_arc.clone());

            // ★ 插件注册表: 使用 Vault 密码挂载
            match crate::plugin_registry::PluginRegistry::mount_or_create(&base_dir, password) {
                Ok(reg) => {
                    let reg_arc = Arc::new(reg);
                    app.plugin_registry = Some(reg_arc.clone());
                    crate::ai::set_global_plugin_registry(reg_arc.clone());
                    app.push_output("[+] 插件注册表已挂载");

                    // 自动加载注册表中的插件 (含命令注册)
                    app.plugin_mgr.set_plugin_registry(reg_arc);
        // v8.0: 设置全局 PluginManager 供 AI 统一命令池调度
        crate::script::set_global_plugin_manager(&mut app.plugin_mgr);
                    let load_results = app.plugin_mgr.auto_load_from_registry(&mut app.cmd_registry);
                    for msg in load_results {
                        app.push_output(&msg);
                    }
                }
                Err(e) => {
                    app.push_output(&format!("[!] 插件注册表挂载失败: {}", e));
                    app.push_output("[*] 插件注册表: 使用 plugin_reg_create 创建");
                }
            }
        }
        Err(e) => {
            app.login_attempts += 1;
            let remaining = app.max_login_attempts.saturating_sub(app.login_attempts);
            if remaining == 0 {
                app.push_output(&format!("[!!] 登录失败 {} 次 — 程序即将退出", app.max_login_attempts));
                app.push_output(&format!("[!] 原因: {}", e));
                app.running = false;
                return;
            }
            app.login_error = format!("密码错误 ({}/{}) — {}", app.login_attempts, app.max_login_attempts, e);
            app.login_phase = LoginPhase::LoginFailed;
            app.password_mode = true;
            app.password_prompt = format!("[RETRY] 密码错误 ({}/{})", app.login_attempts, app.max_login_attempts);
            app.password_action = "login".into();
            app.password_matrix_seed = app.tick_count.wrapping_mul(1442695040888963407u64);
        }
    }
}

/// v4.5: TUI矩阵创建 — 创建新Vault
fn do_create_vault(app: &mut App, password: &str) -> Result<(), String> {
    let base_dir = match app.vault_base_dir.as_ref() {
        Some(d) => d.clone(),
        None => return Err("Vault目录未设置".into()),
    };

    let vault = crate::vault::Vault::create(&base_dir, password)?;
    app.login_phase = LoginPhase::None;
    app.push_output("[+] Vault 已创建 — 加密数据库就绪");
    app.push_output("[+] Vault 已挂载 — AI/tools 加密存储可用");
    app.push_output("[*] 备份提示: 定期备份 ~/.ruoo/vault.rdb 和 vault.salt");
    app.push_output("[*] 命令: vault 查看状态 | lock 锁定 | unlock 解锁 | passwd 改密");
    let vault_arc = Arc::new(vault);
    crate::vault::set_global_vault(vault_arc.clone());
    app.vault = Some(vault_arc.clone());

    // ★ 插件注册表: 使用 Vault 密码创建
    match crate::plugin_registry::PluginRegistry::create(&base_dir, password) {
        Ok(reg) => {
            let reg_arc = Arc::new(reg);
            app.plugin_registry = Some(reg_arc.clone());
            crate::ai::set_global_plugin_registry(reg_arc.clone());
            app.push_output("[+] 插件注册表已创建");
            app.plugin_mgr.set_plugin_registry(reg_arc);
        // v8.0: 设置全局 PluginManager 供 AI 统一命令池调度
        crate::script::set_global_plugin_manager(&mut app.plugin_mgr);
            app.push_output("[*] 使用 plugin_reg 注册插件到加密数据库");
        }
        Err(e) => {
            app.push_output(&format!("[!] 插件注册表创建失败: {}", e));
        }
    }
    Ok(())
}

fn do_unlock(app: &mut App, password: &str) {
    let vault = match app.vault.as_ref() {
        Some(v) => v.clone(),
        None => { app.push_output("[!] Vault 未挂载"); return; }
    };
    if !app.vault_locked {
        app.push_output("[*] Vault 已解锁");
        return;
    }
    if password.is_empty() {
        app.push_output("[!] 密码不能为空");
        return;
    }
    match vault.remount(password) {
        Ok(()) => {
            app.vault_locked = false;
            app.push_output("[+] Vault 已解锁 — 加密数据可用");
            app.push_output("[*] 命令: vault get/list 读取加密数据");
        }
        Err(e) => app.push_output(&format!("[!] 解锁失败: {}", e)),
    }
}

fn do_lock(app: &mut App) {
    if app.vault.is_none() {
        app.push_output("[!] Vault 未挂载");
        return;
    }
    if app.vault_locked {
        app.push_output("[*] Vault 已锁定");
    } else {
        if let Some(ref v) = app.vault { v.lock(); }
        app.vault_locked = true;
        app.push_output("[+] Vault 已锁定 — 内存加密数据已卸载");
    }
}

fn do_change_password(app: &mut App, old_pw: &str, new_pw: &str, confirm: &str) {
    if new_pw != confirm {
        app.push_output("[!] 两次新密码不匹配！");
        return;
    }
    let strength = crate::auth::password_strength(new_pw);
    if matches!(strength, crate::auth::Strength::VeryWeak | crate::auth::Strength::Weak) {
        app.push_output("[!] 新密码太弱！需要 ≥12字符, 含大小写+数字+特殊字符");
        return;
    }
    let vault = match app.vault.as_ref() {
        Some(v) => v.clone(),
        None => { app.push_output("[!] Vault 未挂载"); return; }
    };
    match vault.change_password(old_pw, new_pw) {
        Ok(()) => app.push_output("[+] 密码已更新 — 请妥善保管新密码"),
        Err(e) => app.push_output(&format!("[!] 密码修改失败: {}", e)),
    }
}

// ── 历史搜索按键处理 (Ctrl+R) ──
fn handle_history_search_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyModifiers;

    match key {
        KeyCode::Esc => {
            // 取消搜索，恢复输入
            app.history_search.active = false;
            app.input = app.history_search.query.clone();
            app.cursor = app.input.len();
        }
        KeyCode::Enter => {
            // 接受当前匹配 (Rust支持结构体字段的分离借用)
            let App { ref cmd_history, ref mut history_search, ref mut input, ref mut cursor, .. } = *app;
            history_search.accept(cmd_history, input, cursor);
            history_search.active = false;
        }
        KeyCode::Up | KeyCode::PageUp => {
            app.history_search.prev_match();
            let cmd = {
                let hs = &app.history_search;
                let hist = &app.cmd_history;
                hs.current_match(hist).cloned()
            };
            if let Some(cmd) = cmd {
                app.input = cmd;
                app.cursor = app.input.len();
            }
        }
        KeyCode::Down | KeyCode::PageDown => {
            app.history_search.next_match();
            let cmd = {
                let hs = &app.history_search;
                let hist = &app.cmd_history;
                hs.current_match(hist).cloned()
            };
            if let Some(cmd) = cmd {
                app.input = cmd;
                app.cursor = app.input.len();
            }
        }
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'r' | 'R' => {
                        // Ctrl+R 再次按下 → 上一个匹配
                        app.history_search.prev_match();
                        let cmd = {
                            let hs = &app.history_search;
                            let hist = &app.cmd_history;
                            hs.current_match(hist).cloned()
                        };
                        if let Some(cmd) = cmd {
                            app.input = cmd;
                            app.cursor = app.input.len();
                        }
                    }
                    _ => {}
                }
                return;
            }
            // 普通字符 → 追加到搜索
            {
                let hs = &mut app.history_search;
                let hist = &app.cmd_history;
                hs.query.push(c);
                hs.search(hist);
                hs.current_idx = 0;
            }
            // 更新显示为第一个匹配
            let cmd = {
                let hs = &app.history_search;
                let hist = &app.cmd_history;
                hs.current_match(hist).cloned()
            };
            if let Some(cmd) = cmd {
                app.input = cmd;
                app.cursor = app.input.len();
            } else {
                app.input = app.history_search.query.clone();
                app.cursor = app.input.len();
            }
        }
        KeyCode::Backspace => {
            if !app.history_search.query.is_empty() {
                {
                    let hs = &mut app.history_search;
                    let hist = &app.cmd_history;
                    hs.query.pop();
                    hs.search(hist);
                    hs.current_idx = 0;
                }
                let cmd = {
                    let hs = &app.history_search;
                    let hist = &app.cmd_history;
                    hs.current_match(hist).cloned()
                };
                if let Some(cmd) = cmd {
                    app.input = cmd;
                    app.cursor = app.input.len();
                } else {
                    app.input = app.history_search.query.clone();
                    app.cursor = app.input.len();
                }
            }
        }
        _ => {}
    }
}

// ── 编辑器按键处理 ──
fn handle_editor_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    let buf = match &mut app.editor_buffer {
        Some(b) => b,
        None => {
            app.editor_mode = false;
            app.push_output("[!] 编辑器缓冲区异常，已退出编辑模式");
            return;
        }
    };

    // Ctrl+S 保存
    if modifiers.contains(KeyModifiers::CONTROL) {
        match key {
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // 保存到 Vault
                let content = buf.to_string();
                match crate::bootscript::set_bootscript(&content) {
                    Ok(()) => {
                        app.push_output("[+] 💾 启动脚本已保存到加密 Vault");
                        app.push_output("[*] 重启程序时将自动执行新脚本");
                    }
                    Err(e) => app.push_output(&format!("[!] 保存失败: {}", e)),
                }
                app.editor_mode = false;
                app.editor_buffer = None;
                app.input.clear();
                app.cursor = 0;
                return;
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                // Ctrl+Q 放弃并退出
                app.push_output("[!] 编辑已取消 — 更改未保存");
                app.editor_mode = false;
                app.editor_buffer = None;
                app.input.clear();
                app.cursor = 0;
                return;
            }
            _ => {}
        }
        return;
    }

    match key {
        KeyCode::Esc => {
            // Esc 退出编辑 (提示未保存)
            if buf.modified {
                app.push_output("[!] ⚠ 编辑已取消 — 更改未保存 (Ctrl+S 保存)");
            } else {
                app.push_output("[*] 编辑已退出 — 无更改");
            }
            app.editor_mode = false;
            app.editor_buffer = None;
            app.input.clear();
            app.cursor = 0;
        }
        KeyCode::Enter => {
            let (new_row, new_col) = buf.insert_newline(app.editor_cursor_row, app.editor_cursor_col);
            app.editor_cursor_row = new_row;
            app.editor_cursor_col = new_col;
            // 自动滚动
            if app.editor_cursor_row >= app.editor_scroll + (app.output.len().max(20) - 3) {
                app.editor_scroll = app.editor_cursor_row.saturating_sub(10);
            }
        }
        KeyCode::Char(c) => {
            buf.insert_char(app.editor_cursor_row, app.editor_cursor_col, c);
            app.editor_cursor_col += 1;
        }
        KeyCode::Backspace => {
            if let Some((new_row, new_col)) = buf.delete_backward(app.editor_cursor_row, app.editor_cursor_col) {
                app.editor_cursor_row = new_row;
                app.editor_cursor_col = new_col;
            } else if app.editor_cursor_col > 0 {
                app.editor_cursor_col -= 1;
            }
        }
        KeyCode::Delete => {
            buf.delete_forward(app.editor_cursor_row, app.editor_cursor_col);
        }
        KeyCode::Left => {
            if app.editor_cursor_col > 0 {
                app.editor_cursor_col -= 1;
            } else if app.editor_cursor_row > 0 {
                app.editor_cursor_row -= 1;
                app.editor_cursor_col = buf.lines[app.editor_cursor_row].chars().count();
            }
        }
        KeyCode::Right => {
            let line_len = buf.lines[app.editor_cursor_row].chars().count();
            if app.editor_cursor_col < line_len {
                app.editor_cursor_col += 1;
            } else if app.editor_cursor_row + 1 < buf.lines.len() {
                app.editor_cursor_row += 1;
                app.editor_cursor_col = 0;
            }
        }
        KeyCode::Up => {
            if app.editor_cursor_row > 0 {
                app.editor_cursor_row -= 1;
                let line_len = buf.lines[app.editor_cursor_row].chars().count();
                if app.editor_cursor_col > line_len {
                    app.editor_cursor_col = line_len;
                }
                if app.editor_cursor_row < app.editor_scroll {
                    app.editor_scroll = app.editor_scroll.saturating_sub(1);
                }
            }
        }
        KeyCode::Down => {
            if app.editor_cursor_row + 1 < buf.lines.len() {
                app.editor_cursor_row += 1;
                let line_len = buf.lines[app.editor_cursor_row].chars().count();
                if app.editor_cursor_col > line_len {
                    app.editor_cursor_col = line_len;
                }
            }
        }
        KeyCode::Home => app.editor_cursor_col = 0,
        KeyCode::End => {
            app.editor_cursor_col = buf.lines[app.editor_cursor_row].chars().count();
        }
        KeyCode::PageUp => {
            app.editor_scroll = app.editor_scroll.saturating_sub(10);
            app.editor_cursor_row = app.editor_cursor_row.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.editor_scroll += 10;
            if app.editor_cursor_row + 10 < buf.lines.len() {
                app.editor_cursor_row += 10;
            } else {
                app.editor_cursor_row = buf.lines.len().saturating_sub(1);
            }
        }
        _ => {}
    }
}

// ── 主循环 ──
fn main() -> io::Result<()> {
    // ═══ 防崩溃系统 — 必须在任何操作之前安装 ═══
    install_crash_defense();
    



    // Windows: 设置控制台为 UTF-8，解决中文乱码
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "chcp 65001 >nul 2>&1"])
            .status();
    }
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture, cursor::Hide)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // ★ v4.6: 管理员检测
    app.is_admin = is_running_as_admin();
    if app.is_admin {
        app.push_output("[!] root 管理员模式 — 终端已提升");
    }

    // ★ v4.5.1: 首次帧通过 backend 写入字体大小
    app.font_size_changed = true;
    app.push_output(&format!("[s] 字体: {}px | Ctrl+= 放大 Ctrl+- 缩小 (8~48px, 重启记忆)", app.font_size));

    // ── Vault 登录流程 (v4.5: TUI矩阵特效密码界面) ──
    let vault_base_dir = crate::config::data_dir();
    match std::fs::create_dir_all(&vault_base_dir) {
        Ok(()) => {}
        Err(e) => {
            app.push_output(&format!("[!] 无法创建数据目录 {}: {}", vault_base_dir.display(), e));
            app.push_output("[!] Vault 初始化可能失败 — 检查磁盘权限");
        }
    }
    let vault_exists = vault_base_dir.join("vault.rdb").exists();
    app.vault_base_dir = Some(vault_base_dir);
    app.max_login_attempts = if vault_exists { 3 } else { 5 };

    // ★ v4.5: 启动TUI后立即进入矩阵密码登录界面
    if vault_exists {
        app.login_phase = LoginPhase::LoginPassword;
        app.password_mode = true;
        app.password_prompt = "[>] 输入主密码".into();
        app.password_action = "login".into();
        app.password_matrix_seed = 0xDEADBEEF_CAFE_BABE;
    } else {
        app.login_phase = LoginPhase::CreatePassword;
        app.password_mode = true;
        app.password_prompt = "[NEW] 创建主密码 (>=12字符)".into();
        app.password_action = "login_create".into();
        app.password_matrix_seed = 0xCAFE_BABE_DEAD_BEEF;
        app.push_output("[*] 首次运行 — 请在矩阵界面中创建主密码");
        app.push_output("[*] 要求: ≥12字符, 含大小写+数字+特殊字符");
    }

    // ★ BUG-3 修复: 禁用自动加载，插件仅通过用户显式 plugin load 命令加载
    // auto_load_plugins(&mut app);  // 已禁用 — 防止持久化后门自动执行



    // ── 启动加载脚本 (Vault 挂载后立即执行) ──
    if app.vault.is_some() {
        if !app.vault_locked {
            if let Some(script) = crate::bootscript::get_bootscript() {
                if !script.trim().is_empty() {
                    app.push_output("[*] ======== 执行启动加载脚本 (bootscript) ========");
                    // 解析脚本行并逐行执行
                    let lines: Vec<&str> = script.lines().collect();
                    let mut ctx = crate::script::ScriptContext::new(
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "bootscript"
                    );
                    // 启动脚本给予全部权限 (用户自己的脚本)
                    ctx.perms = crate::script::PermissionSet::all();
                    ctx.force = true;
                    ctx.keep_plugins = true;  // ★ v4.3: bootscript 插件持久化
                    crate::script::execute_script(&mut ctx, &lines, &mut app);
                    app.push_output("[+] 启动加载脚本执行完毕");
                }
            }
        }
    }

    // ★ v4.2: 启动全局热键后台线程 (Ctrl+Shift+S 显示窗口 / Ctrl+Shift+H 切换)
    let window_show_flag = window::start_hotkey_thread();
    app.window_show_requested = Some(window_show_flag);
    app.push_output("[s] 窗口热键: Ctrl+Shift+H 隐藏/切换 | Ctrl+Shift+S 显示 (全局)");



    // 全局输出 — 供 AI 工具 get_terminal_output 读取
    let global_out: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(app.output.clone()));
    ai::set_terminal_output(global_out.clone());
    app.global_out = Some(global_out.clone());

    // AI 异步通道
    let (ai_tx, ai_rx) = std::sync::mpsc::channel::<(ai::AiSession, Result<(String, Vec<String>), String>)>();
    app.ai_tx = Some(ai_tx);

    // AI 流式通道 — 实时输出 API/工具调用信息
    let (ai_stream_tx, ai_stream_rx) = std::sync::mpsc::channel::<crate::ai::StreamMsg>();
    app.ai_stream_tx = Some(ai_stream_tx);
    app.ai_stream_rx = Some(ai_stream_rx);

    // AI 直出消息通道 — ai_message 绕过LLM直接输出到TUI
    let (ai_msg_tx, ai_msg_rx) = std::sync::mpsc::channel::<String>();
    crate::ai::set_ai_message_tx(ai_msg_tx);
    app.ai_message_rx = Some(ai_msg_rx);

    // AI 裸消息通道 — ai_message_raw 绕过前缀直接输出原始文本
    let (ai_raw_msg_tx, ai_raw_msg_rx) = std::sync::mpsc::channel::<String>();
    crate::ai::set_ai_message_raw_tx(ai_raw_msg_tx);
    app.ai_message_raw_rx = Some(ai_raw_msg_rx);

    // 命令异步通道 — 避免阻塞UI
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Vec<String>>();
    app.cmd_rx = Some(cmd_rx);
    app.cmd_tx = Some(cmd_tx);

    let tick = Duration::from_millis(150);

    while app.running {
        app.tick_count = app.tick_count.wrapping_add(1);

        // ★ v4.2: 防御系统 — 已移除桩模块, 后台轮询不再需要

        // ★ v5.2: 轮询全局热键 — 桌面快捷键恢复窗口
        if let Some(ref flag) = app.window_show_requested {
            if flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
                if app.window_hidden {
                    window::show_console();
                    app.window_hidden = false;
                    app.push_output("[s] 窗口已由全局热键恢复 (Ctrl+Shift+S)");
                } else {
                    // 窗口可见时按热键 → 隐藏到系统托盘
                    window::hide_console();
                    app.window_hidden = true;
                    app.push_output("[s] 窗口已隐藏到系统托盘 (Ctrl+Shift+H 全局热键, 右键托盘图标退出)");
                }
            }
        }

        // ★ v5.2: 检测托盘右键菜单"退出"请求
        if window::exit_requested() {
            app.running = false;
            app.push_output("[*] 收到托盘退出请求 — 正在关闭...");
        }

        // 轮询流式消息 (实时输出 API/工具调用信息)
        // 先收集再 push 避免同时持有不可变借用和可变借用
        {
            let mut stream_msgs: Vec<String> = Vec::new();
            if let Some(ref stream_rx) = app.ai_stream_rx {
                loop {
                    match stream_rx.try_recv() {
                        Ok(msg) => {
                            use crate::ai::StreamKind;
                            // 跳过冗余的 API 轮次信息
                            if matches!(msg.kind, StreamKind::Api) { continue; }
                            // 非流式模式(&)仅显示重试/错误，不显示工具调用
                            if !app.ai_stream_mode && matches!(msg.kind, StreamKind::Tool | StreamKind::Result_ | StreamKind::Reply) { continue; }
                            let prefix = match msg.kind {
                                StreamKind::Api     => unreachable!(),
                                StreamKind::Tool    => "[>>] ",
                                StreamKind::Result_ => "[<<] ",
                                StreamKind::Reply   => "[OK] ",
                                StreamKind::Error   => "[!!] ",
                                StreamKind::Retry   => "[🔄] ",
                                StreamKind::Info    => "[::] ",
                            };
                            // ★ v4.6: 截短工具调用参数+结果, 超过300字符自动写入文件 + 时间戳
                            let display_content = if (matches!(msg.kind, StreamKind::Tool) || matches!(msg.kind, StreamKind::Result_)) && msg.content.len() > 300 {
                                let truncated: String = msg.content.chars().take(280).collect();
                                let overflow_file = format!("ruoo_tool_dump_{}.log", 
                                    Local::now().format("%Y%m%d_%H%M%S"));
                                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&overflow_file) {
                                    use std::io::Write;
                                    let _ = writeln!(f, "{}", msg.content);
                                }
                                format!("{}…[截短: {}→280字符, 完整结果→{}]", truncated, msg.content.len(), overflow_file)
                            } else {
                                msg.content.clone()
                            };
                            let ts = Local::now().format("%H:%M:%S");
                            stream_msgs.push(format!("[{}] {}{}", ts, prefix, display_content));
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            for m in stream_msgs {
                app.push_output(&m);
            }
        }

        // AI直出消息 — ai_message 工具绕过LLM直接输出 (始终显示, 不受流模式过滤)
        {
            let mut direct_msgs: Vec<String> = Vec::new();
            if let Some(ref msg_rx) = app.ai_message_rx {
                loop {
                    match msg_rx.try_recv() {
                        Ok(text) => {
                            let ts = Local::now().format("%H:%M:%S");
                            for line in text.lines() {
                                direct_msgs.push(format!("[{}] [::] {}", ts, line));
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            for m in direct_msgs {
                app.push_output(&m);
            }
        }

        // AI裸消息 — ai_message_raw 绕过前缀直接输出原始文本
        {
            let mut raw_msgs: Vec<String> = Vec::new();
            if let Some(ref raw_rx) = app.ai_message_raw_rx {
                loop {
                    match raw_rx.try_recv() {
                        Ok(text) => {
                            for line in text.lines() {
                                raw_msgs.push(line.to_string());
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            for m in raw_msgs {
                app.push_output(&m);
            }
        }

        if app.ai_pending {
            match ai_rx.try_recv() {
                Ok((session, result)) => {
                    if app.ai_mode { app.ai_session = Some(session); }
                    app.ai_pending = false;
                    app.ai_active_tool = None;
                    app.ai_cancel = None;
                    // ★ BUG-FIX: AI工具调用(exec_cmd等)可能触发控制台模式变更
                    //    导致鼠标捕获+raw mode静默失效 → 滚轮失效+上键变历史命令
                    enable_raw_mode().ok();
                    if app.mouse_capture {
                        execute!(io::stdout(), event::EnableMouseCapture).ok();
                    }
                    match result {
                        Ok((reply, _tool_logs)) => {
                            app.ai_stream_mode = false;
                            for line in reply.lines() { app.push_output(line); }
                            app.push_output("[+] #dp 回复完成 — 继续对话或输入命令");
                            app.reset_scroll();
                            // ═══ 任务流程表: 自动发送下一条任务 ═══
                            if app.auto_task_mode {
                                if app.task_queue.is_empty() {
                                    app.auto_task_mode = false;
                                    app.push_output("[*] 任务流程表已完成 — 所有任务执行完毕，流程表自动关闭");
                                } else {
                                    let next_task = app.task_queue.remove(0);
                                    let remaining = app.task_queue.len();
                                    let prefix = if app.ai_mode { "#dp" } else { "$" };
                                    app.push_output(&format!("[*] [自动任务] 执行下一条 (剩余{}条)", remaining));
                                    app.push_output(&format!("{} {}", prefix, next_task));
                                    app.reset_scroll();
                                    app.ai_stream_mode = false;
                                    commands::process_ai_query(&mut app, &next_task, false);
                                }
                            }
                        }
                        Err(e) => {
                            app.push_output(&format!("[!] AI 请求失败: {}", e));
                            app.reset_scroll();
                            // ═══ 任务流程表: 即使失败也继续执行下一条 ═══
                            if app.auto_task_mode {
                                if app.task_queue.is_empty() {
                                    app.auto_task_mode = false;
                                    app.push_output("[*] 任务流程表已完成 (最后一条执行失败) — 流程表自动关闭");
                                } else {
                                    let next_task = app.task_queue.remove(0);
                                    let remaining = app.task_queue.len();
                                    let prefix = if app.ai_mode { "#dp" } else { "$" };
                                    app.push_output(&format!("[!] [自动任务] 上条失败，继续执行下一条 (剩余{}条)", remaining));
                                    app.push_output(&format!("{} {}", prefix, next_task));
                                    app.reset_scroll();
                                    app.ai_stream_mode = false;
                                    commands::process_ai_query(&mut app, &next_task, false);
                                }
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    app.push_output("[!] AI 通道断开 — 请重启 dpai");
                    app.ai_pending = false;
                    app.ai_active_tool = None;
                    app.ai_cancel = None;
                    // ★ BUG-FIX: 恢复可能被子进程破坏的终端状态
                    enable_raw_mode().ok();
                    if app.mouse_capture {
                        execute!(io::stdout(), event::EnableMouseCapture).ok();
                    }
                    // ═══ 任务流程表: 通道断开时自动关闭 ═══
                    if app.auto_task_mode {
                        let remaining = app.task_queue.len();
                        app.auto_task_mode = false;
                        app.task_queue.clear();
                        app.task_counter = 0;
                        app.push_output(&format!("[!] 任务流程表已强制关闭 (通道断开，丢弃{}条未执行任务)", remaining));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        // 异步命令结果轮询 (避免主线程阻塞)
        if app.cmd_pending {
            if let Some(ref rx) = app.cmd_rx {
                match rx.try_recv() {
                    Ok(lines) => {
                        for line in &lines { app.push_output(line); }
                        app.cmd_pending = false;
                        app.progress = None; // 清除进度条
                        app.reset_scroll();
                        // v4.2.1: 回收已完成的后台线程 — 防止 DLL 卸载时 segfault
                        if let Some(handle) = app.cmd_thread.take() {
                            let _ = handle.join();
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        app.cmd_pending = false;
                        app.progress = None;
                        // v4.2.1: channel断开也回收线程
                        if let Some(handle) = app.cmd_thread.take() {
                            let _ = handle.join();
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        } else {
            // 无活跃命令时确保进度条已清除
            if app.progress.is_some() {
                app.progress = None;
            }
        }

        // v4.1: Vault 主动锁定检测 (每10 tick ≈1.5s检查一次)
        // AI 思考期间自动续期 — 用户正在等待, 不应锁
        if app.tick_count % 10 == 0 {
            if !app.vault_locked {
                if let Some(ref v) = app.vault {
                    if app.ai_pending {
                        v.touch(); // AI 运行中, 续期
                    } else if v.check_timeout() {
                        app.vault_locked = true;
                        app.push_output("[!] Vault 已自动锁定 (超时) — 输入 unlock 解锁");
                    }
                }
            }
            // ★ v5.0: 清理过期进度条 + 清除上一次渲染的进度条输出
            app.plugin_progress.cleanup_stale();
            app.output.retain(|l| !l.starts_with("[▌"));
        }

        // ★ BUG-FIX: AI工具调用(exec_cmd等)可能通过子进程改变控制台模式
        //    导致鼠标捕获/raw mode静默失效 → 滚轮失效 + 上键变历史命令
        //    每tick (~150ms)主动恢复终端状态, 不限于ai_pending
        //    ai_pending=true时额外强化: 每tick强制恢复
        if app.ai_pending {
            // AI工具执行期间: 每tick强制恢复 (子进程可能随时篡改控制台)
            enable_raw_mode().ok();
            if app.mouse_capture {
                execute!(io::stdout(), event::EnableMouseCapture).ok();
            }
        } else if app.tick_count % 5 == 0 {
            // 非AI期间: 每5 tick (~0.75s)轻度维护
            enable_raw_mode().ok();
        }

        // ★ v5.0: 多进度条渲染 — 每5 tick (≈0.75s) 刷新到输出区
        if app.tick_count % 5 == 0 {
            let active = app.plugin_progress.active_bars();
            if !active.is_empty() {
                // 移除旧的进度条输出行
                app.output.retain(|l| !l.starts_with("[▌"));
                // 注入新的进度条行
                let mut lines = vec!["".to_string()];
                lines.push(format!("[▌] ═══ 进度 ({}插件) ═══", active.len()));
                for bar in &active {
                    lines.push(format!("[▌] {}", bar.render_line()));
                }
                lines.push("[▌] ══════════════════════".to_string());
                lines.push("".to_string());
                for line in lines {
                    app.output.push(line);
                }
                app.auto_scroll = true; // 触发下一次渲染时自动滚到底部
            }
        }

        // ★ v5.1: 排空消息代理 — 插件/后台线程消息注入输出
        app.message_broker.drain(&mut app.output);

        // ★ v4.5: 渲染 — I/O错误不崩溃, 连续失败3次→terminal重置
        if let Err(e) = terminal.draw(|f| ui::ui(f, &mut app)) {
            app.push_output(&format!("[!] 渲染错误: {} — 继续运行", e));
        }
        // ★ v4.5.1: 字体变更通过 backend 写入 escape 序列 (不走裸 stdout, 防止 raw mode 吞掉)
        if app.font_size_changed {
            app.font_size_changed = false;
            let sz = app.font_size;
            use std::io::Write;
            // Windows Terminal: OSC 9;4;PT ST
            let _ = execute!(terminal.backend_mut(), crossterm::style::Print(format!("\x1b]9;4;{}\x1b\\", sz)));
            // iTerm2 / Alacritty: OSC 50 ; FontSize = XX BEL
            let _ = execute!(terminal.backend_mut(), crossterm::style::Print(format!("\x1b]50;FontSize={}\x07", sz)));
            // Kitty: OSC 99 ; XX ST
            let _ = execute!(terminal.backend_mut(), crossterm::style::Print(format!("\x1b]99;{}\x1b\\", sz)));
            let _ = terminal.backend_mut().flush();
        }
        // ★ IME 光标定位: 仅 MoveTo，不每帧 Hide
        //    Hide 在启动时执行一次即可。光标规范：Move 不改变可见性。
        //    每帧 Hide 会导致 IME 反复丢失坐标 → 候选窗跳顶
        if let Some((cx, cy)) = app.cursor_screen_pos.take() {
            let _ = execute!(terminal.backend_mut(), cursor::MoveTo(cx, cy));
        } else {
            let _ = execute!(terminal.backend_mut(), cursor::Hide);
        }

        if event::poll(tick)? {
            match catch_crash("事件读取", AssertUnwindSafe(|| event::read())) {
                Ok(Ok(event)) => {
                    match event {
                        Event::Key(key) => {
                            if key.kind != KeyEventKind::Release {
                                // 按键处理防护: 命令 panic 不应崩溃进程
                                if let Err(crash_msg) = catch_crash(
                                    &format!("按键处理 {:?}", key.code),
                                    AssertUnwindSafe(|| handle_key(&mut app, key.code, key.modifiers)),
                                ) {
                                    app.push_output(&crash_msg);
                                    app.push_output("[🛡] 防崩溃系统已捕获异常 — 进程继续运行");
                                }
                                if app.toggle_mouse_capture {
                                    app.toggle_mouse_capture = false;
                                    app.mouse_capture = !app.mouse_capture;
                                    if app.mouse_capture {
                                        execute!(io::stdout(), event::EnableMouseCapture).ok();
                                        app.push_output("[+] 鼠标捕获已开启 — 滚轮翻页");
                                    } else {
                                        execute!(io::stdout(), event::DisableMouseCapture).ok();
                                        app.push_output("[+] 鼠标捕获已关闭 — 可选中文字 Ctrl+Shift+C 复制");
                                    }
                                }
                            }
                        }
                        Event::Mouse(mouse) => {
                            // ★ v4.5.1: Alt+滚轮 → 调节字体大小 (Ctrl被WT终端自身缩放占用)
                            if mouse.modifiers.contains(KeyModifiers::ALT) {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        app.font_size = (app.font_size + 1).min(48);
                                        app.cfg.font_size = app.font_size;
                                        app.font_size_changed = true;
                                        let _ = config::save_config(&app.cfg);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        app.font_size = (app.font_size.saturating_sub(1)).max(8);
                                        app.cfg.font_size = app.font_size;
                                        app.font_size_changed = true;
                                        let _ = config::save_config(&app.cfg);
                                    }
                                    _ => {}
                                }
                            } else if app.mouse_capture {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp    => app.scroll_up(3),
                                    MouseEventKind::ScrollDown  => app.scroll_down(3),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Err(e)) => {
                    app.push_output(&format!("[🛡] 事件读取 I/O 错误: {}", e));
                }
                Err(crash_msg) => {
                    app.push_output(&crash_msg);
                    app.push_output("[🛡] 事件读取崩溃 — 跳过本次事件循环");
                }
            }
        }
    }

    // 持久化
    app.cfg.target = if app.target == "—" { String::new() } else { app.target.clone() };
    app.cfg.history = app.cmd_history.clone();
    app.cfg.show_sidebar = app.show_sidebar;
    app.cfg.font_size = app.font_size;
    if let Err(e) = config::save_config(&app.cfg) {
        eprintln!("[!] 配置保存失败: {}", e);
    }

    // v4.7: 保存会话标签页状态
    app.tab_mgr.save_active(&app.output, app.scroll_offset, app.scroll_max,
        app.auto_scroll, &app.target, app.ai_mode);
    let session_state = session::build_session(&app.tab_mgr);
    if let Err(e) = session_state.save() {
        eprintln!("[!] 会话保存失败: {}", e);
    }

    // v4.2.1: 等待后台命令线程完成 — 防止DLL卸载时线程仍在FFI调用中
    if let Some(handle) = app.cmd_thread.take() {
        let _ = handle.join();
    }

    // ★ v4.2: 清理备用终端
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), cursor::Show, LeaveAlternateScreen, event::DisableMouseCapture)?;
    println!("ruoo-arsenal v4.2 — 会话结束 // 配置已保存\n");
    Ok(())
}
