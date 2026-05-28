// ============================================================
// RUOO-CONSOLE v9.3 — 容错系统 (Fault Tolerance)
// PoisonResistantMutex | GuardThread | ToolGuard | Recover
// ============================================================

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use std::panic::{catch_unwind, AssertUnwindSafe, UnwindSafe};

// ═══════════════════════════════════════════
// 1. PoisonFreeMutex — 永不中毒的Mutex
// ═══════════════════════════════════════════

/// 包装 std::sync::Mutex，自动从中毒状态恢复
pub struct PoisonFreeMutex<T> {
    inner: Mutex<T>,
}

impl<T> PoisonFreeMutex<T> {
    pub fn new(val: T) -> Self {
        Self { inner: Mutex::new(val) }
    }

    /// 获取锁 — 如果中毒则自动恢复
    pub fn lock(&self) -> MutexGuard<'_, T> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // ★ BUG-22: 用 ai_message_raw 替代 eprintln! — raw terminal 下 eprintln! 破坏TUI布局
                crate::ai::send_ai_message_raw("[FT] Mutex poisoned — auto-recovering");
                poisoned.into_inner()
            }
        }
    }

    /// 尝试获取锁 (非阻塞)
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.inner.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// 安全操作 — 在锁内执行闭包, 自动处理中毒+panic
    pub fn with_lock<F, R>(&self, f: F) -> Result<R, FaultError>
    where
        F: FnOnce(&mut T) -> R + UnwindSafe,
    {
        let mut guard = self.lock();
        let result = catch_unwind(AssertUnwindSafe(|| f(&mut *guard)));
        drop(guard); // 尽早释放锁
        match result {
            Ok(r) => Ok(r),
            Err(panic_info) => {
                let msg = extract_panic_msg(&panic_info);
                Err(FaultError::ToolPanic { context: "PoisonFreeMutex::with_lock".into(), message: msg })
            }
        }
    }
}

// ═══════════════════════════════════════════
// 2. guard_tool — 工具调用安全包装
// ═══════════════════════════════════════════

/// 容错错误类型
#[derive(Debug, Clone)]
pub enum FaultError {
    ToolPanic { context: String, message: String },
    Timeout { context: String, elapsed_ms: u64 },
    PoisonRecovered { context: String },
    InternalError(String),
}

impl std::fmt::Display for FaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaultError::ToolPanic { context, message } => {
                write!(f, "[FT] {}: PANIC — {}", context, message)
            }
            FaultError::Timeout { context, elapsed_ms } => {
                write!(f, "[FT] {}: TIMEOUT after {}ms", context, elapsed_ms)
            }
            FaultError::PoisonRecovered { context } => {
                write!(f, "[FT] {}: Mutex poison recovered", context)
            }
            FaultError::InternalError(msg) => {
                write!(f, "[FT] Internal: {}", msg)
            }
        }
    }
}

/// 安全调用工具函数 — 捕获panic, 返回Result
pub fn guard_tool<F, R>(context: &str, f: F) -> Result<R, FaultError>
where
    F: FnOnce() -> R + UnwindSafe,
{
    match catch_unwind(f) {
        Ok(result) => Ok(result),
        Err(panic_info) => {
            let msg = extract_panic_msg(&panic_info);
            Err(FaultError::ToolPanic { context: context.to_string(), message: msg })
        }
    }
}

/// 安全调用工具函数 (带超时) — 通过单独线程执行
pub fn guard_tool_timeout<F, R>(context: &str, timeout_ms: u64, f: F) -> Result<R, FaultError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let start = Instant::now();
    let ctx = context.to_string();

    let handle = std::thread::Builder::new()
        .name(format!("ft-{}", &ctx[..ctx.len().min(20)]))
        .spawn(move || catch_unwind(AssertUnwindSafe(f)))
        .map_err(|e| FaultError::InternalError(format!("spawn: {}", e)))?;

    // 轮询等待 (每100ms检查一次)
    let timeout = Duration::from_millis(timeout_ms);
    let check_interval = Duration::from_millis(100);
    let deadline = start + timeout;

    loop {
        if handle.is_finished() {
            return match handle.join() {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(panic_info)) => {
                    let msg = extract_panic_msg(&panic_info);
                    Err(FaultError::ToolPanic { context: ctx, message: msg })
                }
                Err(_) => Err(FaultError::InternalError("thread join failed".into())),
            };
        }

        if Instant::now() >= deadline {
            // 超时 — 线程仍在运行, 但返回错误
            // 注意: 线程无法被强制终止, 会继续在后台运行直至结束
            return Err(FaultError::Timeout {
                context: ctx,
                elapsed_ms: start.elapsed().as_millis() as u64,
            });
        }

        std::thread::sleep(check_interval);
    }
}

// ═══════════════════════════════════════════
// 3. spawn_guarded — 安全线程生成
// ═══════════════════════════════════════════

/// 生成受保护的后台线程 — 自动捕获panic, 通过channel返回结果
pub fn spawn_guarded<F, R>(
    name: &str,
    f: F,
) -> Result<std::thread::JoinHandle<Result<R, FaultError>>, std::io::Error>
where
    F: FnOnce() -> R + Send + UnwindSafe + 'static,
    R: Send + 'static,
{
    let ctx = name.to_string();
    std::thread::Builder::new()
        .name(format!("guard-{}", &ctx[..ctx.len().min(30)]))
        .spawn(move || {
            match catch_unwind(f) {
                Ok(r) => Ok(r),
                Err(panic_info) => {
                    let msg = extract_panic_msg(&panic_info);
                    Err(FaultError::ToolPanic { context: ctx, message: msg })
                }
            }
        })
}

/// 生成守护线程 — 忽略返回结果, panic自动静默
pub fn spawn_daemon<F>(name: &str, f: F) -> std::thread::JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    let ctx = name.to_string();
    std::thread::Builder::new()
        .name(format!("daemon-{}", &ctx[..ctx.len().min(30)]))
        .spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(f));
        })
        .expect("spawn daemon thread")
}

// ═══════════════════════════════════════════
// 4. 恢复辅助
// ═══════════════════════════════════════════

/// 从panic信息提取可读消息
pub fn extract_panic_msg(panic_info: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic_info.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "(unknown panic payload)".to_string()
    }
}

/// 全局容错恢复器 — 在程序启动时调用 (增强现有 crash_defense)
/// 注意: main.rs已有install_crash_defense()优先处理, 此函数作为补充容错层
pub fn install_global_recovery() {
    // 不替换现有hook — main.rs已安装crash_defense
    // 此函数用于其他需要容错恢复的上下文
}

/// 恢复被panic污染的Mutex — 生产环境请使用PoisonFreeMutex
#[allow(dead_code)]
pub fn recover_mutex<T>(_mutex: &Mutex<T>) -> Option<&T> {
    // PoisonFreeMutex 已自动处理恢复，此函数为兼容性占位
    None
}

// ═══════════════════════════════════════════
// 5. FaultToleranceLayer — TUI命令保护
// ═══════════════════════════════════════════

/// 命令执行保护层 — 包装整个命令执行
pub fn protect_command_execution<F>(label: &str, f: F) -> Vec<String>
where
    F: FnOnce() -> Vec<String> + UnwindSafe,
{
    match catch_unwind(f) {
        Ok(output) => output,
        Err(panic_info) => {
            let msg = extract_panic_msg(&panic_info);
            vec![
                format!("[!] 命令 '{}' 执行异常", label),
                format!("[!] 错误: {}", msg),
                "[*] 核心程序不受影响 — 可继续执行其他命令".to_string(),
            ]
        }
    }
}

// ═══════════════════════════════════════════
// 5.5. wait_timeout — 子进程超时等待 (v4.3)
// ═══════════════════════════════════════════

/// 等待子进程完成，支持超时。超时返回 Ok(None)，正常完成返回 Ok(Some(status))
pub fn wait_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<Option<std::process::ExitStatus>, FaultError> {
    let start = Instant::now();
    let check_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    return Ok(None); // 超时
                }
                std::thread::sleep(check_interval);
            }
            Err(e) => return Err(FaultError::InternalError(format!("try_wait: {}", e))),
        }
    }
}

// ═══════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════

// ═══════════════════════════════════════════
// 6. CircuitBreaker — 熔断器 (v9.4)
// ═══════════════════════════════════════════

pub struct CircuitBreaker {
    failure_count: std::sync::atomic::AtomicU32,
    threshold: u32,
    reset_after_secs: u64,
    last_fail: std::sync::Mutex<Option<std::time::Instant>>,
    state: std::sync::atomic::AtomicU8, // 0=CLOSED, 1=OPEN, 2=HALF_OPEN
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_after_secs: u64) -> Self {
        Self {
            failure_count: std::sync::atomic::AtomicU32::new(0),
            threshold,
            reset_after_secs,
            last_fail: std::sync::Mutex::new(None),
            state: std::sync::atomic::AtomicU8::new(0),
        }
    }

    /// 尝试执行 — 如果熔断器开启, 直接拒绝
    pub fn call<F, R>(&self, ctx: &str, f: F) -> Result<R, FaultError>
    where
        F: FnOnce() -> R + UnwindSafe,
    {
        let st = self.state.load(std::sync::atomic::Ordering::Relaxed);
        if st == 1 { // OPEN
            return Err(FaultError::InternalError(format!("[CB] {} 熔断器开启 — 拒绝调用", ctx)));
        }

        match guard_tool(ctx, f) {
            Ok(r) => {
                self.on_success();
                Ok(r)
            }
            Err(e) => {
                self.on_failure();
                Err(e)
            }
        }
    }

    fn on_success(&self) {
        self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
        self.state.store(0, std::sync::atomic::Ordering::Relaxed); // CLOSED
    }

    fn on_failure(&self) {
        let fc = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if let Ok(mut lf) = self.last_fail.lock() {
            *lf = Some(std::time::Instant::now());
        }
        if fc >= self.threshold {
            self.state.store(1, std::sync::atomic::Ordering::Relaxed); // OPEN
        }
    }

    /// 检查是否可以尝试半开
    pub fn should_half_open(&self) -> bool {
        if self.state.load(std::sync::atomic::Ordering::Relaxed) != 1 { return false; }
        if let Ok(lf) = self.last_fail.lock() {
            if let Some(t) = *lf {
                return t.elapsed().as_secs() >= self.reset_after_secs;
            }
        }
        true
    }

    pub fn half_open(&self) {
        self.state.store(2, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_open(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::Relaxed) == 1
    }

    pub fn failures(&self) -> u32 {
        self.failure_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ═══════════════════════════════════════════
// 7. RetryPolicy — 重试策略 (v9.4)
// ═══════════════════════════════════════════

pub struct RetryPolicy {
    max_retries: u32,
    base_delay_ms: u64,
    backoff_multiplier: f64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, base_delay_ms: u64) -> Self {
        Self { max_retries, base_delay_ms, backoff_multiplier: 2.0 }
    }

    pub fn with_backoff(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// 执行带重试的操作
    pub fn execute<F, R>(&self, ctx: &str, f: F) -> Result<R, FaultError>
    where
        F: Fn() -> R + UnwindSafe,
    {
        let mut last_err: Option<FaultError> = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let delay = (self.base_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32 - 1)) as u64;
                std::thread::sleep(Duration::from_millis(delay));
            }
            match guard_tool(&format!("{}/retry{}", ctx, attempt), AssertUnwindSafe(|| f())) {
                Ok(r) => return Ok(r),
                Err(e) => {
                    last_err = Some(e);
                    if attempt < self.max_retries {
                        // ★ BUG-22: 用 ai_message_raw 替代 eprintln!
                        crate::ai::send_ai_message_raw(
                            &format!("[FT] retry {}/{} for {}", attempt + 1, self.max_retries, ctx)
                        );
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| FaultError::InternalError("retry exhausted".into())))
    }
}

// ═══════════════════════════════════════════
// 8. FaultReporter — 全局故障统计 (v9.4)
// ═══════════════════════════════════════════

static FAULT_COUNTERS: std::sync::OnceLock<std::sync::Mutex<FaultStats>> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct FaultStats {
    pub total_panics: u64,
    pub total_timeouts: u64,
    pub total_poison_recoveries: u64,
    pub total_internal_errors: u64,
    pub last_error: Option<String>,
    pub last_error_time: Option<chrono::DateTime<chrono::Local>>,
}

fn fault_stats() -> &'static std::sync::Mutex<FaultStats> {
    FAULT_COUNTERS.get_or_init(|| std::sync::Mutex::new(FaultStats::default()))
}

pub fn record_fault(err: &FaultError) {
    if let Ok(mut stats) = fault_stats().lock() {
        match err {
            FaultError::ToolPanic { .. } => stats.total_panics += 1,
            FaultError::Timeout { .. } => stats.total_timeouts += 1,
            FaultError::PoisonRecovered { .. } => stats.total_poison_recoveries += 1,
            FaultError::InternalError(_) => stats.total_internal_errors += 1,
        }
        stats.last_error = Some(err.to_string());
        stats.last_error_time = Some(chrono::Local::now());
    }
}

pub fn get_fault_stats() -> FaultStats {
    fault_stats().lock().map(|s| s.clone()).unwrap_or_default()
}

pub fn reset_fault_stats() {
    if let Ok(mut stats) = fault_stats().lock() {
        *stats = FaultStats::default();
    }
}

/// 获取故障统计报告
pub fn fault_report() -> String {
    let stats = get_fault_stats();
    format!(
        "═══ 容错统计 ═══\n\
         Panics:        {}\n\
         Timeouts:      {}\n\
         PoisonRecover: {}\n\
         IntErrors:     {}\n\
         最后错误: {}\n\
         最后时间: {}\n",
        stats.total_panics, stats.total_timeouts, stats.total_poison_recoveries,
        stats.total_internal_errors,
        stats.last_error.as_deref().unwrap_or("无"),
        stats.last_error_time.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default()
    )
}

// ═══════════════════════════════════════════
// 9. 便捷集成宏 (v9.4)
// ═══════════════════════════════════════════

/// 容错调用宏 — 自动记录故障 + 返回Result
#[macro_export]
macro_rules! ft_call {
    ($ctx:expr, $f:expr) => {{
        let result = $crate::tools::fault_tolerant::guard_tool($ctx, || $f);
        if let Err(ref e) = result {
            $crate::tools::fault_tolerant::record_fault(e);
        }
        result
    }};
}

/// 容错调用 (带超时)
#[macro_export]
macro_rules! ft_call_timeout {
    ($ctx:expr, $timeout_ms:expr, $f:expr) => {{
        let result = $crate::tools::fault_tolerant::guard_tool_timeout($ctx, $timeout_ms, || $f);
        if let Err(ref e) = result {
            $crate::tools::fault_tolerant::record_fault(e);
        }
        result
    }};
}

/// 容错调用 (仅记录, 返回Option)
#[macro_export]
macro_rules! ft_try {
    ($ctx:expr, $f:expr) => {{
        match $crate::tools::fault_tolerant::guard_tool($ctx, || $f) {
            Ok(v) => Some(v),
            Err(e) => {
                $crate::tools::fault_tolerant::record_fault(&e);
                None
            }
        }
    }};
}

// ═══════════════════════════════════════════
// 10. 全局健康检查 (v9.4)
// ═══════════════════════════════════════════

/// 执行自检 — 验证核心子系统正常
pub fn health_check() -> Vec<String> {
    let mut results = Vec::new();
    results.push("═══ 健康检查 ═══".to_string());

    // 检查PoisonFreeMutex
    let m = PoisonFreeMutex::new(42);
    match m.with_lock(|v| { *v = 99; *v }) {
        Ok(99) => results.push("  [✓] PoisonFreeMutex".to_string()),
        _ => results.push("  [!] PoisonFreeMutex 异常".to_string()),
    }

    // 检查guard_tool
    match guard_tool("health", || 42) {
        Ok(42) => results.push("  [✓] guard_tool".to_string()),
        _ => results.push("  [!] guard_tool 异常".to_string()),
    }

    // 检查spawn_guarded
    if let Ok(h) = spawn_guarded("health", || 42) {
        match h.join() {
            Ok(Ok(42)) => results.push("  [✓] spawn_guarded".to_string()),
            _ => results.push("  [!] spawn_guarded 异常".to_string()),
        }
    } else {
        results.push("  [!] spawn_guarded spawn失败".to_string());
    }

    // 检查CircuitBreaker
    let cb = CircuitBreaker::new(3, 1);
    if cb.call("health", || 42).is_ok() {
        results.push("  [✓] CircuitBreaker".to_string());
    } else {
        results.push("  [!] CircuitBreaker 异常".to_string());
    }

    // 检查RetryPolicy
    let rp = RetryPolicy::new(2, 10);
    if rp.execute("health", || 42).is_ok() {
        results.push("  [✓] RetryPolicy".to_string());
    } else {
        results.push("  [!] RetryPolicy 异常".to_string());
    }

    let stats = get_fault_stats();
    results.push(format!("  故障总计: P={} T={} R={} I={}",
        stats.total_panics, stats.total_timeouts,
        stats.total_poison_recoveries, stats.total_internal_errors));

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_poison_free_mutex_basic() {
        let m = PoisonFreeMutex::new(42);
        assert_eq!(*m.lock(), 42);
    }

    #[test]
    fn test_poison_free_mutex_recovery() {
        let m = Arc::new(PoisonFreeMutex::new(0));
        let m2 = m.clone();

        // 模拟panic后恢复
        let handle = std::thread::spawn(move || {
            let result = m2.with_lock(|val| {
                *val = 99;
                panic!("simulated panic inside lock");
            });
            assert!(result.is_err());
        });
        let _ = handle.join();

        // 主线程仍能获取锁
        assert_eq!(*m.lock(), 99);
    }

    #[test]
    fn test_guard_tool_ok() {
        let result = guard_tool("test", || 42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_guard_tool_panic() {
        let result = guard_tool("test", || -> i32 { panic!("boo") });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, FaultError::ToolPanic { .. }));
    }

    #[test]
    fn test_guard_tool_timeout() {
        let result = guard_tool_timeout("test", 200, || {
            std::thread::sleep(Duration::from_millis(500));
            42
        });
        assert!(matches!(result, Err(FaultError::Timeout { .. })));
    }
}
