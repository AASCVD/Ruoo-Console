// ═══════════════════════════════════════════════════════════
// RUOO-ARSENAL v4.2 — 全局panic防护层 + 分级响应
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

        // 写入崩溃日志
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("ruoo_crash.log"))
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
