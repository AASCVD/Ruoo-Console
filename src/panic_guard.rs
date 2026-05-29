// ═══════════════════════════════════════════════════════════
// RUOO-CONSOLE v4.3 — 全局panic防护层 + 原生崩溃捕获
//
// v4.3 新增 (Zero-Day):
//   - native_crash 模块: Windows VEH 原生崩溃捕获 + setjmp/longjmp 恢复
//   - 防止插件 DLL 的 SEGV/访问违规/非法指令等硬崩溃导致终端进程终止
//
// v4.2 新增:
//   - 崩溃分级响应 (warn→sandbox→disable)
//   - 崩溃转储收集 (调用栈捕获)
//   - CrashGuard: 插件级崩溃计数+自动熔断联动
//   - safe_call: 带熔断器的安全函数调用
//   - 崩溃统计面板数据
// ═══════════════════════════════════════════════════════════

use std::panic::{self, AssertUnwindSafe};
use std::thread;
use std::sync::atomic::{AtomicU64, AtomicU32, AtomicBool, Ordering};
use std::time::{Instant, Duration};

// ═══════════════════════════════════════════════════════
// 崩溃分级
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashSeverity {
    /// 轻微 — 仅记录，不限制
    Minor,
    /// 中等 — 触发降级运行
    Moderate,
    /// 严重 — 触发熔断
    Severe,
    /// 致命 — 永久禁用
    Fatal,
}

impl CrashSeverity {
    pub fn from_error_msg(msg: &str) -> Self {
        let lower = msg.to_lowercase();
        if lower.contains("fatal") || lower.contains("segfault") || lower.contains("access violation") {
            CrashSeverity::Fatal
        } else if lower.contains("panic") || lower.contains("unwinding") || lower.contains("assertion") {
            CrashSeverity::Severe
        } else if lower.contains("timeout") || lower.contains("resource") || lower.contains("memory") {
            CrashSeverity::Moderate
        } else {
            CrashSeverity::Minor
        }
    }
}

// ═══════════════════════════════════════════════════════
// 崩溃记录
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CrashRecord {
    pub timestamp: Instant,
    pub thread_name: String,
    pub error_msg: String,
    pub severity: CrashSeverity,
    pub plugin_name: Option<String>,
    pub command: Option<String>,
    pub stack_snapshot: Option<String>,
}

// ═══════════════════════════════════════════════════════
// 崩溃守卫 — 全局单例
// ═══════════════════════════════════════════════════════

pub struct CrashGuard {
    pub total_crashes: AtomicU64,
    pub recent_crashes: AtomicU32,       // 60s内崩溃数
    pub last_crash_time: parking_lot::Mutex<Option<Instant>>,
    pub crash_history: parking_lot::Mutex<Vec<CrashRecord>>,
    pub max_history: usize,
    pub crash_window_secs: u64,
    pub severe_threshold: u32,           // 60s内>=此数触发全局熔断
    pub global_circuit_open: AtomicBool,
}

impl CrashGuard {
    pub fn new() -> Self {
        Self {
            total_crashes: AtomicU64::new(0),
            recent_crashes: AtomicU32::new(0),
            last_crash_time: parking_lot::Mutex::new(None),
            crash_history: parking_lot::Mutex::new(Vec::with_capacity(128)),
            max_history: 128,
            crash_window_secs: 60,
            severe_threshold: 20,
            global_circuit_open: AtomicBool::new(false),
        }
    }

    /// 记录一次崩溃
    pub fn record_crash(
        &self,
        thread_name: &str,
        error_msg: &str,
        plugin_name: Option<&str>,
        command: Option<&str>,
    ) -> CrashSeverity {
        let severity = CrashSeverity::from_error_msg(error_msg);
        let now = Instant::now();

        self.total_crashes.fetch_add(1, Ordering::Relaxed);
        self.recent_crashes.fetch_add(1, Ordering::Relaxed);

        // 清理过期记录 (60秒窗口)
        {
            let mut last = self.last_crash_time.lock();
            if let Some(prev) = *last {
                if now.duration_since(prev) > Duration::from_secs(self.crash_window_secs) {
                    self.recent_crashes.store(1, Ordering::Relaxed);
                }
            }
            *last = Some(now);
        }

        // 存储崩溃记录
        {
            let mut history = self.crash_history.lock();
            history.push(CrashRecord {
                timestamp: now,
                thread_name: thread_name.to_string(),
                error_msg: error_msg.to_string(),
                severity,
                plugin_name: plugin_name.map(|s| s.to_string()),
                command: command.map(|s| s.to_string()),
                stack_snapshot: None,
            });
            if history.len() > self.max_history {
                history.remove(0);
            }
        }

        // 检查是否需要全局熔断
        if self.recent_crashes.load(Ordering::Relaxed) >= self.severe_threshold {
            self.global_circuit_open.store(true, Ordering::Relaxed);
        }

        // 写入崩溃日志 → 工作目录/log/
        {
            let log_dir = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("log");
            let _ = std::fs::create_dir_all(&log_dir);
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("ruoo_crash.log"))
                .map(|mut f| {
                    use std::io::Write;
                    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let plugin_str = plugin_name.unwrap_or("(host)");
                    let cmd_str = command.unwrap_or("(none)");
                    let _ = writeln!(
                        f,
                        "[{}] [{:?}] thread={} plugin={} cmd={} | {}",
                        ts, severity, thread_name, plugin_str, cmd_str, error_msg
                    );
                });
        }

        severity
    }

    /// 获取崩溃统计
    pub fn stats(&self) -> String {
        format!(
            "═══ 崩溃统计 ═══\n\
             总崩溃: {}次\n\
             60s内: {}次\n\
             全局熔断: {}\n\
             历史记录: {}条\n\
             严重阈值: {}次/{}s",
            self.total_crashes.load(Ordering::Relaxed),
            self.recent_crashes.load(Ordering::Relaxed),
            if self.global_circuit_open.load(Ordering::Relaxed) { "开启 [!!!]" } else { "关闭" },
            self.crash_history.lock().len(),
            self.severe_threshold, self.crash_window_secs,
        )
    }

    /// 获取最近N条崩溃记录
    pub fn recent_records(&self, n: usize) -> Vec<CrashRecord> {
        let history = self.crash_history.lock();
        let start = if history.len() > n { history.len() - n } else { 0 };
        history[start..].to_vec()
    }

    /// 重置全局熔断
    pub fn reset_global_circuit(&self) {
        self.global_circuit_open.store(false, Ordering::Relaxed);
        self.recent_crashes.store(0, Ordering::Relaxed);
    }

    /// 检查是否允许插件调用
    pub fn allow_plugin_call(&self) -> bool {
        !self.global_circuit_open.load(Ordering::Relaxed)
    }
}

// ═══════════════════════════════════════════════════════
// 全局单例
// ═══════════════════════════════════════════════════════

static GLOBAL_CRASH_GUARD: std::sync::LazyLock<CrashGuard> =
    std::sync::LazyLock::new(CrashGuard::new);

pub fn crash_guard() -> &'static CrashGuard {
    &GLOBAL_CRASH_GUARD
}

// ═══════════════════════════════════════════════════════
// 安全的线程生成 (升级版)
// ═══════════════════════════════════════════════════════

pub fn spawn_safe<F>(name: &str, f: F) -> thread::JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    let label = name.to_string();
    let plugin_hint = if name.starts_with("plugin_") {
        Some(name.strip_prefix("plugin_").unwrap_or(name).to_string())
    } else {
        None
    };

    thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(f));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    format!("线程 '{}' panic: {}", label, s)
                } else if let Some(s) = e.downcast_ref::<String>() {
                    format!("线程 '{}' panic: {}", label, s)
                } else {
                    format!("线程 '{}' panic — 未知原因", label)
                };

                // 记录到崩溃守卫
                crash_guard().record_crash(
                    &label,
                    &msg,
                    plugin_hint.as_deref(),
                    None,
                );

                // 通过ai_message通知
                crate::ai::send_ai_message_raw(&msg);
            }
        })
        .expect("spawn_safe: 线程创建失败")
}

// ═══════════════════════════════════════════════════════
// 安全的插件调用 — 带熔断器联动
// ═══════════════════════════════════════════════════════

pub fn spawn_plugin_call_safe<F>(
    plugin_name: &str,
    command: &str,
    f: F,
) -> thread::JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    let pname = plugin_name.to_string();
    let cmd = command.to_string();
    let thread_label = format!("plugin_{}", plugin_name);

    spawn_safe(&thread_label.clone(), move || {
        // 检查全局熔断
        if !crash_guard().allow_plugin_call() {
            crate::ai::send_ai_message_raw(
                &format!("[!] 全局熔断器开启 — 插件 '{}' 调用被拒绝", pname)
            );
            return;
        }

        let result = panic::catch_unwind(AssertUnwindSafe(f));
        if let Err(e) = result {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "未知panic".to_string()
            };

            crash_guard().record_crash(
                &thread_label,
                &msg,
                Some(&pname),
                Some(&cmd),
            );
        }
    })
}

// ═══════════════════════════════════════════════════════
// 安全执行闭包 (同步)
// ═══════════════════════════════════════════════════════

pub fn catch_panic<F, R>(label: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> R + panic::UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(r) => Ok(r),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                format!("{} 捕获崩溃: {}", label, s)
            } else if let Some(s) = e.downcast_ref::<String>() {
                format!("{} 捕获崩溃: {}", label, s)
            } else {
                format!("{} 捕获崩溃: 未知原因", label)
            };

            crash_guard().record_crash(label, &msg, None, None);
            Err(msg)
        }
    }
}

// ═══════════════════════════════════════════════════════
// 安全执行带插件上下文
// ═══════════════════════════════════════════════════════

pub fn catch_plugin_panic<F, R>(
    plugin_name: &str,
    command: &str,
    f: F,
) -> Result<R, String>
where
    F: FnOnce() -> R + panic::UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(r) => Ok(r),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "未知panic".to_string()
            };

            let severity = crash_guard().record_crash(
                &format!("plugin_{}", plugin_name),
                &msg,
                Some(plugin_name),
                Some(command),
            );

            let full_msg = match severity {
                CrashSeverity::Fatal => format!("[致命] 插件 '{}' 崩溃 (已记录): {}", plugin_name, msg),
                CrashSeverity::Severe => format!("[严重] 插件 '{}' 崩溃: {}", plugin_name, msg),
                _ => format!("插件 '{}' 执行异常: {}", plugin_name, msg),
            };

            Err(full_msg)
        }
    }
}

// ═══════════════════════════════════════════════════════════
// v4.3: Windows 原生崩溃捕获 (SEGV/访问违规/非法指令)
//
// Rust 的 catch_unwind 只能捕获 panic，无法捕获原生代码的:
//   - EXCEPTION_ACCESS_VIOLATION (0xC0000005) — 空指针/野指针
//   - EXCEPTION_ILLEGAL_INSTRUCTION (0xC000001D) — 坏指令  
//   - EXCEPTION_STACK_OVERFLOW (0xC00000FD) — 栈溢出
//   - EXCEPTION_IN_PAGE_ERROR (0xC0000006) — 页面错误
//
// 通过 AddVectoredExceptionHandler + setjmp/longjmp 实现:
//   1. VEH 回调在异常发生时被 OS 调用
//   2. 检查当前线程是否处于被保护的插件调用中
//   3. 如果是 → 记录崩溃信息 → longjmp 跳回安全点
//   4. 如果不是 → 记录并让 OS 继续处理 (进程仍会终止)
// ═══════════════════════════════════════════════════════════

#[cfg(windows)]
pub mod native_crash {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::cell::UnsafeCell;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::collections::HashMap;
    use std::thread::ThreadId;

    // ── FFI: msvcrt 的 _setjmp / longjmp ──
    extern "C" {
        /// 保存当前执行上下文到 buf。返回 0 表示正常保存，
        /// 非 0 表示从 longjmp 恢复。
        fn _setjmp(buf: *mut u8) -> i32;
        /// 跳转到 _setjmp 保存的上下文。永不返回。
        fn longjmp(buf: *mut u8, val: i32) -> !;
    }

    /// _JUMP_BUF 在 Windows x86_64 上的大小
    const JMP_BUF_SIZE: usize = 256;

    /// 每线程的插件保护状态
    struct PluginProtectionState {
        active: AtomicBool,
        plugin_name: Mutex<String>,
        crash_caught: AtomicBool,
        crash_code: AtomicU32,
        crash_addr: AtomicUsize,
        crash_msg: Mutex<String>,
        jmp_buf: UnsafeCell<[u8; JMP_BUF_SIZE]>,
    }

    unsafe impl Sync for PluginProtectionState {}

    impl PluginProtectionState {
        fn new() -> Self {
            Self {
                active: AtomicBool::new(false),
                plugin_name: Mutex::new(String::new()),
                crash_caught: AtomicBool::new(false),
                crash_code: AtomicU32::new(0),
                crash_addr: AtomicUsize::new(0),
                crash_msg: Mutex::new(String::new()),
                jmp_buf: UnsafeCell::new([0u8; JMP_BUF_SIZE]),
            }
        }
    }

    std::thread_local! {
        static STATE: PluginProtectionState = PluginProtectionState::new();
    }

    static mut VEH_HANDLE: *mut std::ffi::c_void = std::ptr::null_mut();

    // ═══════════════════════════════════════════════════════════
    // v4.4: 插件线程注册表 + 崩溃信号通道
    //
    // 解决问题: 插件自建 native_thread 崩溃时,
    //   VEH无法识别崩溃线程属于哪个插件, 无法触发插件级熔断。
    //
    // PluginThreadRegistry: ThreadId → PluginName 映射
    //   插件加载时, 所有插件派生的线程都应调用 register_plugin_thread()
    //   VEH handler通过当前ThreadId反查插件名
    //
    // CrashSignal: 跨线程崩溃通知队列
    //   VEH运行在崩溃线程上下文中, 无法直接访问PluginManager (在主线程Arc<Mutex<>>内)
    //   通过此队列将崩溃事件异步传递给主线程, 由主线程触发熔断/强制卸载
    // ═══════════════════════════════════════════════════════════

    /// 插件线程注册表: ThreadId → PluginName
    static PLUGIN_THREAD_REGISTRY: std::sync::LazyLock<parking_lot::RwLock<HashMap<ThreadId, String>>> =
        std::sync::LazyLock::new(|| parking_lot::RwLock::new(HashMap::new()));

    /// 崩溃通知 — VEH捕获后入队, 主线程轮询处理
    #[derive(Debug, Clone)]
    pub struct CrashNotification {
        pub plugin_name: String,
        pub thread_name: String,
        pub crash_desc: String,
        pub exception_code: u32,
        pub exception_addr: usize,
        pub is_recoverable: bool,
        pub timestamp: std::time::Instant,
    }

    /// 崩溃信号队列 — VEH写入, 主线程drain
    static CRASH_SIGNALS: std::sync::LazyLock<parking_lot::Mutex<Vec<CrashNotification>>> =
        std::sync::LazyLock::new(|| parking_lot::Mutex::new(Vec::with_capacity(64)));

    /// 本会话中每个插件的native线程崩溃计数 (用于熔断)
    static PLUGIN_NATIVE_CRASH_COUNT: std::sync::LazyLock<parking_lot::Mutex<HashMap<String, u32>>> =
        std::sync::LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

    /// native线程崩溃熔断阈值 (超过此数自动触发force_unload信号)
    const NATIVE_CRASH_CIRCUIT_BREAKER: u32 = 3;

    /// 注册当前线程为某插件的native线程
    pub fn register_plugin_thread(plugin_name: &str) {
        let tid = std::thread::current().id();
        PLUGIN_THREAD_REGISTRY.write().insert(tid, plugin_name.to_string());
    }

    /// 注销当前线程
    pub fn unregister_plugin_thread() {
        let tid = std::thread::current().id();
        PLUGIN_THREAD_REGISTRY.write().remove(&tid);
    }

    /// 主线程轮询: 排出所有待处理的崩溃通知
    pub fn drain_crash_signals() -> Vec<CrashNotification> {
        let mut signals = CRASH_SIGNALS.lock();
        std::mem::take(&mut *signals)
    }

    /// 查询某插件的native线程崩溃计数
    pub fn plugin_native_crash_count(plugin_name: &str) -> u32 {
        PLUGIN_NATIVE_CRASH_COUNT.lock()
            .get(plugin_name)
            .copied()
            .unwrap_or(0)
    }

    /// 重置某插件的native线程崩溃计数 (force_unload后调用)
    pub fn reset_plugin_native_crash_count(plugin_name: &str) {
        PLUGIN_NATIVE_CRASH_COUNT.lock().remove(plugin_name);
    }

    /// 检查某插件是否超过native崩溃熔断阈值
    pub fn is_plugin_native_circuit_triggered(plugin_name: &str) -> bool {
        plugin_native_crash_count(plugin_name) >= NATIVE_CRASH_CIRCUIT_BREAKER
    }

    // ── 原生 Windows 类型和常量 (避免 windows-sys feature 依赖) ──

    #[repr(C)]
    struct EXCEPTION_RECORD {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut EXCEPTION_RECORD,
        exception_address: *mut std::ffi::c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    #[repr(C)]
    struct EXCEPTION_POINTERS {
        exception_record: *mut EXCEPTION_RECORD,
        context_record: *mut std::ffi::c_void,
    }

    const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC0000005;
    const EXCEPTION_ILLEGAL_INSTRUCTION: u32 = 0xC000001D;
    const EXCEPTION_STACK_OVERFLOW: u32 = 0xC00000FD;
    const EXCEPTION_IN_PAGE_ERROR: u32 = 0xC0000006;
    const EXCEPTION_DATATYPE_MISALIGNMENT: u32 = 0x80000002;
    const EXCEPTION_PRIV_INSTRUCTION: u32 = 0xC0000096;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    extern "system" {
        fn AddVectoredExceptionHandler(first: u32, handler: Option<unsafe extern "system" fn(*mut EXCEPTION_POINTERS) -> i32>) -> *mut std::ffi::c_void;
        /// v4.5: 干净终止当前线程, 防止不可恢复异常传播到进程级别
        fn ExitThread(dwExitCode: u32) -> !;
    }

    // ── VEH 回调 v4.3.1 — OS 在异常时调用 ──
    // v4.3.1 修复 (Zero-Day):
    //   - STACK_OVERFLOW: 绝不尝试 longjmp 恢复 — OS在备用栈上调用handler,
    //     恢复到的原始栈已损坏, longjmp必导致二次崩溃
    //   - ACCESS_VIOLATION 目标地址检查: 0xFFFFFFFFFFFFFFFF / NULL 等明显
    //     野指针 → 不恢复, 避免恢复后状态不一致
    //   - ILLEGAL_INSTRUCTION: 不恢复 — 代码段已损坏无法信任
    //   - 只有可恢复的 ACCESS_VIOLATION 才走 longjmp
    unsafe extern "system" fn veh_handler(
        exception_info: *mut EXCEPTION_POINTERS,
    ) -> i32 {
        let record = &*(*exception_info).exception_record;
        let code = record.exception_code;

        // 只处理致命硬件异常
        let is_fatal = code == EXCEPTION_ACCESS_VIOLATION
            || code == EXCEPTION_ILLEGAL_INSTRUCTION
            || code == EXCEPTION_STACK_OVERFLOW
            || code == EXCEPTION_IN_PAGE_ERROR
            || code == EXCEPTION_DATATYPE_MISALIGNMENT
            || code == EXCEPTION_PRIV_INSTRUCTION;

        if !is_fatal {
            return EXCEPTION_CONTINUE_SEARCH;
        }

        let addr = record.exception_address as usize;
        let code_u32 = code as u32;

        // 提取 ACCESS_VIOLATION 的目标地址 (parameter[1])
        let violation_target = if code_u32 == 0xC0000005 && record.number_parameters >= 2 {
            record.exception_information[1] as usize
        } else {
            0
        };

        let crash_desc = match code_u32 {
            0xC0000005 => {
                let access_type = if record.number_parameters >= 1 {
                    match record.exception_information[0] {
                        0 => "READ",
                        1 => "WRITE",
                        8 => "EXECUTE(DEP)",
                        _ => "UNKNOWN",
                    }
                } else {
                    "UNKNOWN"
                };
                format!("ACCESS_VIOLATION({}) at 0x{:016X} -> 0x{:016X}",
                    access_type, addr, violation_target)
            }
            0xC000001D => format!("ILLEGAL_INSTRUCTION at 0x{:016X}", addr),
            0xC00000FD => format!("STACK_OVERFLOW at 0x{:016X}", addr),
            0xC0000006 => format!("IN_PAGE_ERROR at 0x{:016X}", addr),
            _ => format!("NATIVE_CRASH 0x{:08X} at 0x{:016X}", code_u32, addr),
        };

        // ── v4.3.1: 判断是否可恢复 ──
        // STACK_OVERFLOW: 不可恢复, OS在备用栈运行handler, longjmp回原始栈=必死
        let unrecoverable = code_u32 == EXCEPTION_STACK_OVERFLOW
            || code_u32 == EXCEPTION_ILLEGAL_INSTRUCTION  // 代码段损坏
            || code_u32 == EXCEPTION_IN_PAGE_ERROR;        // 页面错误

        // ACCESS_VIOLATION: 检查目标地址是否明显无效
        let is_wild_pointer = code_u32 == 0xC0000005 && (
            violation_target == 0xFFFFFFFFFFFFFFFF  // INVALID_HANDLE_VALUE解引用
            || violation_target == 0                // 空指针解引用
            || violation_target < 0x10000           // 低位地址(大概率未初始化)
        );

        let can_recover = !unrecoverable && !is_wild_pointer;

        // ── v4.4: 识别崩溃线程所属插件 ──
        // 优先级:
        //   1. 检查 PluginThreadRegistry (插件自建native线程)
        //   2. 回退到 thread_local STATE (enter_protected_call包裹的调用)
        let owning_plugin: Option<String> = {
            let tid = std::thread::current().id();
            PLUGIN_THREAD_REGISTRY.read().get(&tid).cloned()
        };

        // v4.4 回退方案: 从thread_local STATE读取 (作为临时String然后借用)
        let fallback_plugin: String;
        let plugin_label: &str = if let Some(ref p) = owning_plugin {
            p.as_str()
        } else {
            fallback_plugin = STATE.with(|state| {
                state.plugin_name.lock()
                    .map(|p| if p.is_empty() { "(host)".to_string() } else { p.clone() })
                    .unwrap_or_else(|_| "(host)".to_string())
            });
            &fallback_plugin
        };

        // 记录到崩溃守卫
        super::crash_guard().record_crash(
            "native_thread",
            &format!("{} [{}]", crash_desc,
                if can_recover { "可恢复" } else { "不可恢复" }),
            Some(&plugin_label),
            None,
        );

        // ── v4.4: 插件native线程崩溃 → 入队崩溃信号 + 递增熔断计数 ──
        if let Some(ref pname) = owning_plugin {
            // 递增该插件的native崩溃计数
            let count = {
                let mut counts = PLUGIN_NATIVE_CRASH_COUNT.lock();
                let c = counts.entry(pname.clone()).or_insert(0);
                *c += 1;
                *c
            };

            // 入队崩溃通知 — 主线程稍后drain并触发熔断
            {
                let mut signals = CRASH_SIGNALS.lock();
                if signals.len() < 64 {
                    signals.push(CrashNotification {
                        plugin_name: pname.clone(),
                        thread_name: format!("native_thread({:?})", std::thread::current().id()),
                        crash_desc: crash_desc.clone(),
                        exception_code: code_u32,
                        exception_addr: addr,
                        is_recoverable: can_recover,
                        timestamp: std::time::Instant::now(),
                    });
                }
            }

            // 如果超过熔断阈值, 在崩溃信号中追加紧急标记
            if count >= NATIVE_CRASH_CIRCUIT_BREAKER {
                let mut signals = CRASH_SIGNALS.lock();
                if signals.len() < 64 {
                    signals.push(CrashNotification {
                        plugin_name: pname.clone(),
                        thread_name: "VEH_CIRCUIT_BREAKER".to_string(),
                        crash_desc: format!(
                            "插件native线程累计崩溃{}次, 超过熔断阈值{}, 建议立即force_unload",
                            count, NATIVE_CRASH_CIRCUIT_BREAKER
                        ),
                        exception_code: 0,
                        exception_addr: 0,
                        is_recoverable: false,
                        timestamp: std::time::Instant::now(),
                    });
                }
            }
        }

        // ── v4.6: protected 线程 → longjmp 恢复 (无论是否"可恢复") ──
        // v4.6 关键修复 (Zero-Day):
        //   对 enter_protected_call 内的 Rust std::thread 线程, 绝不使用 ExitThread.
        //   原因: Rust std 会检测到线程非正常终止并 panic 整个进程
        //   (lifecycle.rs:243 "threads should not terminate unexpectedly")
        //   策略: 所有异常(含野指针等"不可恢复"异常)都 longjmp 回 setjmp,
        //   让 Rust 线程正常 unwind → catch_unwind 捕获 → mpsc 返回错误 → join 正常
        //   DLL 内部状态可能已损坏, 但调用者立即返回错误, 不继续使用 DLL 资源
        //   仅 STACK_OVERFLOW 例外 (OS在备用栈运行handler, 无法longjmp回损坏栈)
        STATE.with(|state| {
            if state.active.load(Ordering::Relaxed) {
                if code_u32 == EXCEPTION_STACK_OVERFLOW {
                    // 栈溢出: 无法 longjmp, 在下方 ExitThread 块中终止
                    return;
                }
                // 所有其他异常 (含不可恢复野指针等): longjmp 恢复
                state.crash_caught.store(true, Ordering::SeqCst);
                state.crash_code.store(code_u32, Ordering::SeqCst);
                state.crash_addr.store(addr, Ordering::SeqCst);
                let recover_msg = format!("{} [VEH longjmp, was:{}]",
                    crash_desc,
                    if can_recover { "可恢复" } else { "不可恢复" });
                if let Ok(mut msg) = state.crash_msg.lock() {
                    *msg = recover_msg;
                }
                let jmp_buf = state.jmp_buf.get();
                longjmp(jmp_buf as *mut u8, 1);
            }
        });

        // ── v4.6: ExitThread 仅用于非 Rust 托管线程 ──
        // 执行到此处的线程:
        //   (a) STACK_OVERFLOW 在 protected 内 (上方 longjmp 块中 return 跳过)
        //   (b) state.active==false 竞态 (极罕见)
        //   (c) PLUGIN_THREAD_REGISTRY 注册的原生线程 (不在 protected 内, 非 Rust 托管)
        //   (d) 非插件线程
        //
        //   (a)(b)(c): 插件相关 → ExitThread 干净终止 (非Rust线程无生命周期检查)
        //   (d):       非插件 → EXCEPTION_CONTINUE_SEARCH 正常传播
        //
        let is_plugin_thread = owning_plugin.is_some();
        let is_in_protected = STATE.with(|s| s.active.load(Ordering::Relaxed));

        if is_plugin_thread || is_in_protected {
            // 插件线程 (native / protected竞态 / 栈溢出) → ExitThread
            super::crash_guard().record_crash(
                "native_thread",
                &format!(
                    "线程终止: {} [ExitThread(0x{:08X})]",
                    crash_desc, code_u32
                ),
                Some(plugin_label),
                None,
            );
            unsafe { ExitThread(code_u32); }
        }

        // 非插件线程 (宿主线程) — 异常继续传播 (正常行为)
        EXCEPTION_CONTINUE_SEARCH
    }

    // ── 公共 API ──

    /// 安装 VEH 处理器。在 main() 启动时调用一次。
    pub fn install() {
        unsafe {
            let handle = AddVectoredExceptionHandler(1, Some(veh_handler));
            VEH_HANDLE = handle;
        }
        let log_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("log");
        let _ = std::fs::create_dir_all(&log_dir);
        let _ = std::fs::write(
            log_dir.join("ruoo_veh_installed.txt"),
            "VEH active: AddVectoredExceptionHandler priority=1\n",
        );
    }

    /// 进入受保护的插件调用。
    /// 返回 true: 正常，可执行插件代码
    /// 返回 false: 从崩溃中恢复，应立即返回错误。
    ///            调用者不应再使用插件DLL中的任何资源。
    /// v4.4: 自动注册当前线程到 PluginThreadRegistry
    pub fn enter_protected_call(plugin_name: &str) -> bool {
        // v4.4: 自动注册到插件线程注册表
        register_plugin_thread(plugin_name);

        STATE.with(|state| {
            if let Ok(mut pn) = state.plugin_name.lock() {
                *pn = plugin_name.to_string();
            }
            state.crash_caught.store(false, Ordering::SeqCst);
            state.crash_code.store(0, Ordering::SeqCst);
            state.crash_addr.store(0, Ordering::SeqCst);
            state.active.store(true, Ordering::SeqCst);

            let jmp_buf = state.jmp_buf.get();
            let ret = unsafe { _setjmp(jmp_buf as *mut u8) };

            if ret != 0 {
                // 从 longjmp 恢复 — 插件崩溃已被捕获
                // 标记保护状态已结束
                state.active.store(false, Ordering::SeqCst);
                // v4.4: 注销线程 (崩溃恢复后线程不应继续使用插件资源)
                unregister_plugin_thread();
                // 注意: 此时插件 DLL 内部状态可能已损坏
                // 调用者应尽快返回错误, 不要在后续代码中使用
                // 任何来自该插件的 FFI 资源
                false
            } else {
                true // 正常进入
            }
        })
    }

    /// 正常退出受保护调用 (无崩溃)
    /// v4.4: 自动从插件线程注册表注销
    pub fn leave_protected_call() {
        STATE.with(|state| {
            state.active.store(false, Ordering::SeqCst);
        });
        // v4.4: 注销线程
        unregister_plugin_thread();
    }

    /// 强制清理当前线程的保护状态 (用于abort/强制卸载前)
    pub fn force_clear_protection() {
        STATE.with(|state| {
            state.active.store(false, Ordering::SeqCst);
            state.crash_caught.store(false, Ordering::SeqCst);
        })
    }

    /// 上次受保护调用是否捕获了崩溃
    pub fn was_crash_caught() -> bool {
        STATE.with(|state| state.crash_caught.load(Ordering::SeqCst))
    }

    /// 崩溃详情: (异常代码, 地址, 描述)
    pub fn crash_details() -> (u32, usize, String) {
        STATE.with(|state| {
            let code = state.crash_code.load(Ordering::SeqCst);
            let addr = state.crash_addr.load(Ordering::SeqCst);
            let msg = state.crash_msg.lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            (code, addr, msg)
        })
    }
}

#[cfg(not(windows))]
pub mod native_crash {
    /// 非 Windows 平台桩实现
    pub fn install() {}
    pub fn enter_protected_call(_name: &str) -> bool { true }
    pub fn leave_protected_call() {}
    pub fn force_clear_protection() {}
    pub fn was_crash_caught() -> bool { false }
    pub fn crash_details() -> (u32, usize, String) { (0, 0, String::new()) }
    // v4.4 桩
    pub fn register_plugin_thread(_name: &str) {}
    pub fn unregister_plugin_thread() {}
    pub fn drain_crash_signals() -> Vec<CrashNotification> { Vec::new() }
    pub fn plugin_native_crash_count(_name: &str) -> u32 { 0 }
    pub fn reset_plugin_native_crash_count(_name: &str) {}
    pub fn is_plugin_native_circuit_triggered(_name: &str) -> bool { false }
    #[derive(Debug, Clone)]
    pub struct CrashNotification {
        pub plugin_name: String,
        pub thread_name: String,
        pub crash_desc: String,
        pub exception_code: u32,
        pub exception_addr: usize,
        pub is_recoverable: bool,
        pub timestamp: std::time::Instant,
    }
}

// ═══════════════════════════════════════════════════════
// v4.6: 全局 panic hook — 防御 lifecycle.rs:243 进程级崩溃
// ═══════════════════════════════════════════════════════

/// 安装全局 panic hook 作为最后防线:
/// 如果 stdin::thread::lifecycle.rs:243 的 "threads should not terminate unexpectedly"
/// 仍然触发 (如 STACK_OVERFLOW → ExitThread → Rust检测), 在此截获并记录,
/// 防止进程 abort.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{}", info)
        };

        // lifecycle.rs:243 — 插件线程异常终止导致的生命周期检查失败
        // 不传播此 panic, 仅记录
        if msg.contains("threads should not terminate unexpectedly")
            || msg.contains("lifecycle.rs")
        {
            let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".into());
            eprintln!(
                "[RUOO] 防御性截获 lifecycle panic ({}): {}\n  \
                 这表示插件原生线程异常终止被Rust std检测到, 但进程已被保护.",
                location, msg
            );
            // 写入日志
            if let Ok(log_dir) = std::env::current_dir() {
                let _ = std::fs::create_dir_all(log_dir.join("log"));
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_dir.join("log").join("ruoo_lifecycle_panic.log"))
                {
                    use std::io::Write;
                    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let _ = writeln!(f, "[{}] {} | {}", ts, location, msg);
                }
            }
            // 不调用 default_hook — 进程继续运行
            return;
        }

        // 其他 panic: 走默认处理
        default_hook(info);
    }));
}

// ═══════════════════════════════════════════════════════
// v4.4: 顶层封装 — 从主循环轮询native崩溃信号
// ═══════════════════════════════════════════════════════

/// 主线程轮询: 排出所有待处理的插件native线程崩溃通知
/// 应在主循环每帧调用, 发现崩溃信号后触发插件熔断/卸载
pub fn poll_native_crash_signals() -> Vec<native_crash::CrashNotification> {
    native_crash::drain_crash_signals()
}

/// 检查插件是否触发native崩溃熔断
pub fn is_plugin_native_crash_circuit(plugin_name: &str) -> bool {
    native_crash::is_plugin_native_circuit_triggered(plugin_name)
}

/// 重置插件native崩溃计数 (force_unload后)
pub fn reset_plugin_native_crashes(plugin_name: &str) {
    native_crash::reset_plugin_native_crash_count(plugin_name)
}

/// v4.4: 注册当前线程为插件native线程
/// 插件在spawn线程时应调用此函数, 确保VEH能识别崩溃归属
pub fn register_plugin_thread(plugin_name: &str) {
    native_crash::register_plugin_thread(plugin_name);
}

/// v4.4: 注销当前线程
pub fn unregister_plugin_thread() {
    native_crash::unregister_plugin_thread();
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catch_panic_normal() {
        let result = catch_panic("test", || 42);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_catch_panic_captures() {
        let result = catch_panic("test_panic", || {
            panic!("故意触发");
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("故意触发"));
    }

    #[test]
    fn test_crash_guard_record() {
        let guard = CrashGuard::new();
        guard.record_crash("test_thread", "测试崩溃", Some("test_plugin"), Some("test_cmd"));
        assert_eq!(guard.total_crashes.load(Ordering::Relaxed), 1);
        assert_eq!(guard.recent_records(1).len(), 1);
    }

    #[test]
    fn test_severity_classification() {
        assert_eq!(CrashSeverity::from_error_msg("FATAL ERROR"), CrashSeverity::Fatal);
        assert_eq!(CrashSeverity::from_error_msg("panic occurred"), CrashSeverity::Severe);
        assert_eq!(CrashSeverity::from_error_msg("timeout"), CrashSeverity::Moderate);
        assert_eq!(CrashSeverity::from_error_msg("minor issue"), CrashSeverity::Minor);
    }
}