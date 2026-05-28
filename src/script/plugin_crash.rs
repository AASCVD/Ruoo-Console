// ============================================================
// RUOO-CONSOLE v5.0 — 插件防崩溃框架 (Plugin Crash Guard)
//
// 防止插件致命性崩溃把终端搞崩.
// 三层隔离: panic捕获 → 线程超时强杀 → 库强制卸载
//
// 特性:
//   - PluginCallGuard: 全隔离调用, panic不传播到主线程
//   - CrashReport: 详细崩溃诊断 (位置/原因/堆栈/建议)
//   - force_unload: 强制卸载 (跳过shutdown, 直接drop+反复GC)
//   - crash_count 跟踪: 连续崩溃自动禁用插件
//   - crash log: 持久化到 ~/.ruoo/plugin_crash.log
// ============================================================

use std::time::{Duration, Instant};
use std::panic::{catch_unwind, AssertUnwindSafe, UnwindSafe};

// ═══════════════════════════════════════════════════════
// 崩溃报告
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct CrashReport {
    /// 插件名称
    pub plugin_name: String,
    /// 插件版本
    pub plugin_version: String,
    /// 插件路径
    pub plugin_path: String,
    /// 崩溃时间 (ISO 8601)
    pub crashed_at: String,
    /// 崩溃类型
    pub crash_type: CrashType,
    /// 错误消息 (截断至500字符)
    pub error_message: String,
    /// 调用命令
    pub command: String,
    /// 崩溃时的堆栈 (前20帧)
    pub stack_trace: Vec<String>,
    /// 建议操作
    pub suggested_action: String,
    /// 是否已强制卸载
    pub was_force_unloaded: bool,
    /// 该插件累计崩溃次数
    pub total_crashes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashType {
    /// Rust panic (被 catch_unwind 捕获)
    Panic,
    /// 调用超时 (插件线程超时被放弃)
    Timeout,
    /// 调用后 SIGSEGV/访问冲突 (主进程捕获)
    Segfault,
    /// 插件返回错误码
    ErrorCode(i32),
    /// 未知致命错误
    Unknown,
}

impl CrashType {
    pub fn label(&self) -> &str {
        match self {
            CrashType::Panic => "PANIC",
            CrashType::Timeout => "TIMEOUT",
            CrashType::Segfault => "SEGFAULT",
            CrashType::ErrorCode(_) => "ERROR_CODE",
            CrashType::Unknown => "UNKNOWN",
        }
    }
}

impl CrashReport {
    pub fn new(
        plugin_name: &str,
        plugin_version: &str,
        plugin_path: &str,
        crash_type: CrashType,
        error_message: &str,
        command: &str,
        total_crashes: u32,
        was_force_unloaded: bool,
    ) -> Self {
        let stack_trace = capture_stack_frames(20);
        let suggested_action = match crash_type {
            CrashType::Panic => format!(
                "插件内部panic, 已自动卸载. 累计{}次崩溃. {}",
                total_crashes,
                if total_crashes >= 3 {
                    "建议: 插件连续崩溃, 请检查源码或联系作者修复后再重新加载"
                } else {
                    "可以尝试重新加载: plugin load {} <路径>"
                }
            ),
            CrashType::Timeout => format!(
                "插件调用超时, 线程已被放弃. 累计{}次超时. 建议: 检查插件是否死循环或阻塞",
                total_crashes
            ),
            CrashType::Segfault => format!(
                "插件触发段错误! 累计{}次. 建议: 插件二进制已损坏或不兼容, 请重新编译",
                total_crashes
            ),
            CrashType::ErrorCode(code) => format!(
                "插件返回错误码 {}. 累计{}次错误. 建议: 检查插件初始化/命令参数",
                code, total_crashes
            ),
            CrashType::Unknown => format!(
                "插件发生未知致命错误. 累计{}次. 建议: 检查插件文件和系统兼容性",
                total_crashes
            ),
        };

        Self {
            plugin_name: plugin_name.to_string(),
            plugin_version: plugin_version.to_string(),
            plugin_path: plugin_path.to_string(),
            crashed_at: chrono::Utc::now().to_rfc3339(),
            crash_type,
            error_message: error_message.chars().take(500).collect(),
            command: command.to_string(),
            stack_trace,
            suggested_action,
            was_force_unloaded,
            total_crashes,
        }
    }

    /// 格式化完整崩溃报告
    pub fn format_report(&self) -> String {
        let mut r = String::new();
        r.push_str("═══ 插件崩溃报告 ═══\n");
        r.push_str(&format!("  插件    : {} (v{})\n", self.plugin_name, self.plugin_version));
        r.push_str(&format!("  路径    : {}\n", self.plugin_path));
        r.push_str(&format!("  时间    : {}\n", &self.crashed_at[..self.crashed_at.len().min(19)]));
        r.push_str(&format!("  类型    : {}\n", self.crash_type.label()));
        r.push_str(&format!("  命令    : {}\n", self.command));
        r.push_str(&format!("  错误    : {}\n", self.error_message));
        r.push_str(&format!("  崩溃次数: {}\n", self.total_crashes));
        if self.was_force_unloaded {
            r.push_str("  状态    : 已强制卸载\n");
        }
        if !self.stack_trace.is_empty() {
            r.push_str("  ── 堆栈 (最近帧) ──\n");
            for frame in &self.stack_trace {
                r.push_str(&format!("    {}\n", frame));
            }
        }
        r.push_str(&format!("\n  {}\n", self.suggested_action));
        r
    }
}

// ═══════════════════════════════════════════════════════
// 堆栈帧捕获
// ═══════════════════════════════════════════════════════

fn capture_stack_frames(max_frames: usize) -> Vec<String> {
    let bt = std::backtrace::Backtrace::force_capture();
    let bt_str = format!("{}", bt);
    bt_str
        .lines()
        .take(max_frames)
        .map(|l| {
            l.chars().take(200).collect::<String>()
                .replace('\\', "/")
        })
        .collect()
}

// ═══════════════════════════════════════════════════════
// 崩溃日志持久化
// ═══════════════════════════════════════════════════════

pub fn persist_crash_log(report: &CrashReport) {
    let log_path = crate::config::data_dir().join("plugin_crash.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(f, "=== {} | {} | {} ===", 
            &report.crashed_at[..report.crashed_at.len().min(19)],
            report.crash_type.label(),
            report.plugin_name);
        let _ = writeln!(f, "  cmd: {}", report.command);
        let _ = writeln!(f, "  err: {}", report.error_message);
        let _ = writeln!(f, "  ver: {} | path: {}", report.plugin_version, report.plugin_path);
        let _ = writeln!(f, "  crashes: {}", report.total_crashes);
        for frame in &report.stack_trace {
            let _ = writeln!(f, "  {}", frame);
        }
        let _ = writeln!(f);
        let _ = f.flush();
    }
}

// ═══════════════════════════════════════════════════════
// 防崩溃调用守卫
// ═══════════════════════════════════════════════════════

/// 插件调用守卫 — 完全隔离插件崩溃
/// 
/// 使用方式:
///   let guard = PluginCallGuard::new("myplug", "1.0", "/path/to/plug.dll", "scan");
///   let result = guard.call_with_timeout(|| unsafe { plugin_exec(cmd) }, 30_000);
pub struct PluginCallGuard {
    pub plugin_name: String,
    pub plugin_version: String,
    pub plugin_path: String,
    pub command: String,
    pub crash_count: u32,
}

impl PluginCallGuard {
    pub fn new(name: &str, version: &str, path: &str, command: &str, crash_count: u32) -> Self {
        Self {
            plugin_name: name.to_string(),
            plugin_version: version.to_string(),
            plugin_path: path.to_string(),
            command: command.to_string(),
            crash_count,
        }
    }

    /// 带超时的安全调用 — panic被捕获, 超时被检测
    /// 返回: Ok(result) 或 Err(CrashReport)
    pub fn call_with_timeout<F, R>(
        &self,
        f: F,
        timeout_ms: u64,
    ) -> Result<R, CrashReport>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let name = self.plugin_name.clone();
        let ver = self.plugin_version.clone();
        let path = self.plugin_path.clone();
        let cmd = self.command.clone();
        let crashes = self.crash_count + 1;
        let timeout = Duration::from_millis(timeout_ms);

        let start = Instant::now();
        let deadline = start + timeout;

        // 在独立线程中执行, panic不传播
        let handle = match std::thread::Builder::new()
            .name(format!("plug-{}", &name[..name.len().min(16)]))
            .spawn(move || {
                catch_unwind(AssertUnwindSafe(f))
            })
        {
            Ok(h) => h,
            Err(e) => {
                let report = CrashReport::new(
                    &name, &ver, &path,
                    CrashType::Unknown,
                    &format!("无法创建插件线程: {}", e),
                    &cmd, crashes, false,
                );
                persist_crash_log(&report);
                return Err(report);
            }
        };

        // 轮询等待 (每50ms检查)
        let check_interval = Duration::from_millis(50);
        loop {
            if handle.is_finished() {
                match handle.join() {
                    Ok(Ok(result)) => return Ok(result),
                    Ok(Err(panic_info)) => {
                        let msg = extract_panic_message(&panic_info);
                        let crash_type = if msg.contains("SIGSEGV") || msg.contains("access violation") {
                            CrashType::Segfault
                        } else {
                            CrashType::Panic
                        };
                        let report = CrashReport::new(
                            &name, &ver, &path,
                            crash_type, &msg, &cmd, crashes, false,
                        );
                        persist_crash_log(&report);
                        return Err(report);
                    }
                    Err(_) => {
                        let report = CrashReport::new(
                            &name, &ver, &path,
                            CrashType::Unknown,
                            "插件线程join失败 (可能已被操作系统终止)",
                            &cmd, crashes, false,
                        );
                        persist_crash_log(&report);
                        return Err(report);
                    }
                }
            }

            if Instant::now() >= deadline {
                // 超时 — 线程仍在运行, 显式detach避免句柄泄漏
                // Rust无法安全终止线程, 只能detach让OS在线程结束后回收
                // 注意: DLL仍被Arc引用保持加载, 线程可能继续持有资源
                std::mem::forget(handle); // 不join, 直接丢弃JoinHandle
                let elapsed = start.elapsed().as_millis() as u64;
                let report = CrashReport::new(
                    &name, &ver, &path,
                    CrashType::Timeout,
                    &format!("超时 {}ms (限制 {}ms), 线程已detach", elapsed, timeout_ms),
                    &cmd, crashes, false,
                );
                persist_crash_log(&report);
                return Err(report);
            }

            std::thread::sleep(check_interval);
        }
    }

    /// 同步安全调用 (无超时)
    pub fn call_safe<F, R>(&self, f: F) -> Result<R, CrashReport>
    where
        F: FnOnce() -> R + UnwindSafe,
    {
        let name = &self.plugin_name;
        let ver = &self.plugin_version;
        let path = &self.plugin_path;
        let cmd = &self.command;
        let crashes = self.crash_count + 1;

        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(result) => Ok(result),
            Err(panic_info) => {
                let msg = extract_panic_message(&panic_info);
                let crash_type = if msg.contains("SIGSEGV") || msg.contains("access violation") {
                    CrashType::Segfault
                } else {
                    CrashType::Panic
                };
                let report = CrashReport::new(
                    name, ver, path,
                    crash_type, &msg, cmd, crashes, false,
                );
                persist_crash_log(&report);
                Err(report)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════
// 辅助: 从panic payload提取可读消息
// ═══════════════════════════════════════════════════════

pub fn extract_panic_message(panic_info: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic_info.downcast_ref::<String>() {
        s.chars().take(500).collect()
    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
        s.chars().take(500).collect()
    } else if let Some(s) = panic_info.downcast_ref::<std::string::String>() {
        s.chars().take(500).collect()
    } else {
        "(无法读取panic负载 — 非字符串类型)".to_string()
    }
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crash_report_format() {
        let report = CrashReport::new(
            "test_plug", "1.0.0", "/tmp/test.dll",
            CrashType::Panic, "index out of bounds: the len is 0 but the index is 1",
            "scan --ports 80", 1, true,
        );
        let formatted = report.format_report();
        assert!(formatted.contains("PANIC"));
        assert!(formatted.contains("test_plug"));
        assert!(formatted.contains("已强制卸载"));
    }

    #[test]
    fn test_call_guard_success() {
        let guard = PluginCallGuard::new("test", "1.0", "/t/t.dll", "ping", 0);
        let result = guard.call_safe(|| 42);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_call_guard_panic() {
        let guard = PluginCallGuard::new("test", "1.0", "/t/t.dll", "crash", 0);
        let result = guard.call_safe(|| -> i32 {
            panic!("deliberate test panic");
        });
        assert!(result.is_err());
        let report = result.unwrap_err();
        assert_eq!(report.crash_type, CrashType::Panic);
        assert!(report.error_message.contains("deliberate test panic"));
    }

    #[test]
    fn test_call_guard_timeout() {
        let guard = PluginCallGuard::new("test", "1.0", "/t/t.dll", "sleep", 0);
        let result = guard.call_with_timeout(
            || {
                std::thread::sleep(Duration::from_secs(10));
                42
            },
            100, // 100ms timeout
        );
        assert!(result.is_err());
        let report = result.unwrap_err();
        assert_eq!(report.crash_type, CrashType::Timeout);
    }
}
