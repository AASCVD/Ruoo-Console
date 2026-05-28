// ============================================================
// RUOO-CONSOLE — 遥测事件总线 (右侧特效侧边栏数据源)
// 线程安全环形缓冲区，收集网络/工具/AI/系统实时事件
// ============================================================

use std::sync::{Mutex, LazyLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ═══ 事件类型 ═══

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetDirection {
    Upload,    // 本机发出
    Download,  // 本机接收
    Both,      // 双向
}

#[derive(Debug, Clone)]
pub struct NetworkEvent {
    pub timestamp: Instant,
    pub direction: NetDirection,
    pub host: String,
    pub port: u16,
    pub protocol: String,   // TCP/UDP/HTTP/DNS/TLS
    pub bytes: Option<u64>,
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum ToolStatus {
    Started,
    Success,
    Failed,
    Timeout,
}

#[derive(Debug, Clone)]
pub struct ToolEvent {
    pub timestamp: Instant,
    pub tool_name: String,
    pub status: ToolStatus,
    pub duration_ms: u64,
    pub detail: String,  // 简短结果描述
}

#[derive(Debug, Clone)]
pub enum AiPhase {
    Thinking,
    ToolCalling(String), // 工具名
    Streaming,
    Done,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct AiEvent {
    pub timestamp: Instant,
    pub phase: AiPhase,
    pub tokens: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SystemEvent {
    pub timestamp: Instant,
    pub category: String,  // file/dir/process/plugin/driver
    pub action: String,
    pub detail: String,
}

// ═══ 全局环形缓冲区 ═══

const NET_RING_SIZE: usize = 120;
const TOOL_RING_SIZE: usize = 60;
const AI_RING_SIZE: usize = 40;
const SYS_RING_SIZE: usize = 60;

struct TelemetryBuffers {
    network: Vec<NetworkEvent>,
    tools: Vec<ToolEvent>,
    ai: Vec<AiEvent>,
    system: Vec<SystemEvent>,
    // 网络吞吐统计 (滑动窗口)
    net_bytes_up: u64,
    net_bytes_down: u64,
    net_connections: u64,
    net_window_start: Instant,
    // 进程统计
    startup: Instant,
    total_tool_calls: u64,
    total_tool_errors: u64,
    total_ai_requests: u64,
}

/// 进程启动时间 — 遥测模块首次初始化时记录
pub static STARTUP: LazyLock<Instant> = LazyLock::new(Instant::now);

static TELEMETRY: LazyLock<Mutex<TelemetryBuffers>> = LazyLock::new(|| {
    Mutex::new(TelemetryBuffers {
        network: Vec::with_capacity(NET_RING_SIZE),
        tools: Vec::with_capacity(TOOL_RING_SIZE),
        ai: Vec::with_capacity(AI_RING_SIZE),
        system: Vec::with_capacity(SYS_RING_SIZE),
        net_bytes_up: 0,
        net_bytes_down: 0,
        net_connections: 0,
        net_window_start: Instant::now(),
        startup: *STARTUP,
        total_tool_calls: 0,
        total_tool_errors: 0,
        total_ai_requests: 0,
    })
});

// ═══ 推送接口 (工具层调用) ═══

pub fn push_network(host: &str, port: u16, protocol: &str, direction: NetDirection, bytes: Option<u64>, latency_ms: Option<f64>) {
    // ★ BUG-23 修复: 中毒锁自动恢复，避免静默丢失事件
    let mut buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
        if buf.network.len() >= NET_RING_SIZE {
            buf.network.remove(0);
        }
        buf.network.push(NetworkEvent {
            timestamp: Instant::now(),
            direction,
            host: host.to_string(),
            port,
            protocol: protocol.to_string(),
            bytes,
            latency_ms,
        });
        // 统计
        if let Some(b) = bytes {
            match direction {
                NetDirection::Upload => buf.net_bytes_up += b,
                NetDirection::Download => buf.net_bytes_down += b,
                NetDirection::Both => { buf.net_bytes_up += b / 2; buf.net_bytes_down += b / 2; }
            }
        }
        buf.net_connections += 1;
        // 每60秒重置吞吐窗口
        if buf.net_window_start.elapsed().as_secs() > 60 {
            buf.net_bytes_up = 0;
            buf.net_bytes_down = 0;
            buf.net_connections = 0;
            buf.net_window_start = Instant::now();
        }
}

pub fn push_tool_start(name: &str) {
    let mut buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
        if buf.tools.len() >= TOOL_RING_SIZE {
            buf.tools.remove(0);
        }
        buf.tools.push(ToolEvent {
            timestamp: Instant::now(),
            tool_name: name.to_string(),
            status: ToolStatus::Started,
            duration_ms: 0,
            detail: String::new(),
        });
}

pub fn push_tool_result(name: &str, success: bool, duration_ms: u64, detail: &str) {
    let mut buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
        buf.total_tool_calls += 1;
        if !success { buf.total_tool_errors += 1; }
        // 找到对应的 Started 事件并更新，或新建
        if let Some(ev) = buf.tools.iter_mut().rev().find(|e| e.tool_name == name && matches!(e.status, ToolStatus::Started)) {
            ev.status = if success { ToolStatus::Success } else { ToolStatus::Failed };
            ev.duration_ms = duration_ms;
            ev.detail = detail.to_string();
        } else {
            if buf.tools.len() >= TOOL_RING_SIZE {
                buf.tools.remove(0);
            }
            buf.tools.push(ToolEvent {
                timestamp: Instant::now(),
                tool_name: name.to_string(),
                status: if success { ToolStatus::Success } else { ToolStatus::Failed },
                duration_ms,
                detail: detail.to_string(),
            });
        }
}

pub fn push_ai_phase(phase: AiPhase) {
    let mut buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
        if buf.ai.len() >= AI_RING_SIZE {
            buf.ai.remove(0);
        }
        if matches!(phase, AiPhase::Thinking) {
            buf.total_ai_requests += 1;
        }
        buf.ai.push(AiEvent {
            timestamp: Instant::now(),
            phase,
            tokens: None,
        });
}

pub fn push_system(category: &str, action: &str, detail: &str) {
    let mut buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
        if buf.system.len() >= SYS_RING_SIZE {
            buf.system.remove(0);
        }
        buf.system.push(SystemEvent {
            timestamp: Instant::now(),
            category: category.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
        });
}

// ═══ 进程状态 ═══

#[derive(Debug, Clone)]
pub struct ProcessStats {
    pub uptime_secs: u64,
    pub pid: u32,
    pub thread_count: usize,
    pub memory_rss_mb: f64,      // 物理内存 (MB) — WorkingSet/VM RSS
    pub memory_virtual_mb: f64,  // 虚拟内存 (MB) — 进程虚拟地址空间
    pub memory_commit_mb: f64,   // 已提交内存 (MB) — Windows PagefileUsage / Linux VmHWM
    pub system_ram_total_mb: f64,// 系统物理内存总量 (MB)
    pub cpu_percent: f64,        // 进程CPU占用百分比 (0-100)
    pub total_tool_calls: u64,
    pub total_tool_errors: u64,
    pub total_ai_requests: u64,
    pub total_net_connections: u64,
    pub total_sys_events: u64,
    // v13.1: 扩展系统信息
    pub hostname: String,
    pub cpu_cores: usize,
    pub disk_total_gb: f64,
    pub disk_free_gb: f64,
    pub network_ips: Vec<String>,
    pub work_dir: String,
    // v14.0: 系统级全网收发统计
    pub sys_net_rx_bytes: u64,   // 系统所有网卡累计接收字节
    pub sys_net_tx_bytes: u64,   // 系统所有网卡累计发送字节
    pub sys_net_rx_bps: f64,     // 系统接收速率 (bytes/sec, 两次采样差值)
    pub sys_net_tx_bps: f64,     // 系统发送速率 (bytes/sec)
}

/// 查询当前进程线程数
fn query_thread_count() -> usize {
    #[cfg(windows)]
    {
        // Windows: 通过 toolhelp32 快照计数
        unsafe {
            use windows_sys::Win32::System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            };
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            let pid = std::process::id();
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE { return 0; }
            let mut te: THREADENTRY32 = std::mem::zeroed();
            te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut count = 0usize;
            if Thread32First(snapshot, &mut te) != 0 {
                loop {
                    if te.th32OwnerProcessID == pid { count += 1; }
                    if Thread32Next(snapshot, &mut te) == 0 { break; }
                }
            }
            windows_sys::Win32::Foundation::CloseHandle(snapshot);
            count
        }
    }
    #[cfg(not(windows))]
    {
        // Linux: count /proc/self/task entries
        if let Ok(entries) = std::fs::read_dir("/proc/self/task") {
            entries.count()
        } else {
            0
        }
    }
}

/// 查询系统总物理内存 (MB)
fn query_system_ram_mb() -> f64 {
    #[cfg(windows)]
    {
        // 使用 kernel32 GlobalMemoryStatusEx (通过 windows-sys 链接)
        extern "system" {
            fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
        }
        #[repr(C)]
        #[allow(non_snake_case)]
        struct MEMORYSTATUSEX {
            dwLength: u32,
            dwMemoryLoad: u32,
            ullTotalPhys: u64,
            ullAvailPhys: u64,
            ullTotalPageFile: u64,
            ullAvailPageFile: u64,
            ullTotalVirtual: u64,
            ullAvailVirtual: u64,
            ullAvailExtendedVirtual: u64,
        }
        unsafe {
            let mut mem_status: MEMORYSTATUSEX = std::mem::zeroed();
            mem_status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut mem_status) != 0 {
                return mem_status.ullTotalPhys as f64 / 1_048_576.0;
            }
        }
        0.0
    }
    #[cfg(not(windows))]
    {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb) = line.split_whitespace().nth(1).and_then(|s| s.parse::<f64>().ok()) {
                        return kb / 1024.0;
                    }
                }
            }
        }
        0.0
    }
}

/// 查询进程CPU使用率 (基于前后两次采样的差值)
/// BUG-FIX: 使用 Instant 追踪真实墙钟时间, 取代错误的 Instant::now().elapsed() 近零噪声
static LAST_CPU_TIME: AtomicU64 = AtomicU64::new(0);
static LAST_CPU_INSTANT: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

fn query_cpu_percent() -> f64 {
    #[cfg(windows)]
    {
        unsafe {
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, GetProcessTimes,
            };
            use windows_sys::Win32::Foundation::FILETIME;
            let mut _creation: FILETIME = std::mem::zeroed();
            let mut _exit: FILETIME = std::mem::zeroed();
            let mut kernel: FILETIME = std::mem::zeroed();
            let mut user: FILETIME = std::mem::zeroed();
            let h = GetCurrentProcess();
            if GetProcessTimes(h, &mut _creation, &mut _exit, &mut kernel, &mut user) == 0 {
                return 0.0;
            }
            let kernel_ticks = (kernel.dwLowDateTime as u64) | ((kernel.dwHighDateTime as u64) << 32);
            let user_ticks = (user.dwLowDateTime as u64) | ((user.dwHighDateTime as u64) << 32);
            let total_ticks = kernel_ticks + user_ticks;

            // BUG-FIX: 用 Instant::now() 作为单调时钟戳, 不再用 now.elapsed() (~0ns 噪声)
            let now = Instant::now();
            let mut last_instant_guard = LAST_CPU_INSTANT.lock().unwrap();
            let last_ticks = LAST_CPU_TIME.swap(total_ticks, Ordering::Relaxed);

            if let Some(last_instant) = *last_instant_guard {
                let elapsed = now.duration_since(last_instant);
                let elapsed_ns = elapsed.as_nanos() as u64;
                if elapsed_ns > 0 && last_ticks > 0 {
                    let delta_ticks = total_ticks.saturating_sub(last_ticks);
                    let cpu_ns = delta_ticks * 100; // 100-nanosecond intervals → nanoseconds
                    let pct = (cpu_ns as f64 / elapsed_ns as f64) * 100.0;
                    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                    *last_instant_guard = Some(now);
                    return pct.min(100.0 * cores) / cores;
                }
            }
            *last_instant_guard = Some(now);
        }
        0.0
    }
    #[cfg(not(windows))]
    {
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
            let parts: Vec<&str> = stat.split_whitespace().collect();
            if parts.len() >= 15 {
                let utime = parts[13].parse::<u64>().unwrap_or(0);
                let stime = parts[14].parse::<u64>().unwrap_or(0);
                let total = utime + stime;
                let now = Instant::now();
                let mut last_instant_guard = LAST_CPU_INSTANT.lock().unwrap();
                let last_ticks = LAST_CPU_TIME.swap(total, Ordering::Relaxed);
                if let Some(last_instant) = *last_instant_guard {
                    let elapsed = now.duration_since(last_instant);
                    let elapsed_ms = elapsed.as_millis() as u64;
                    if elapsed_ms > 0 && last_ticks > 0 {
                        let delta = total.saturating_sub(last_ticks);
                        let clk_tck = 100u64;
                        let cpu_ms = delta * 1000 / clk_tck;
                        *last_instant_guard = Some(now);
                        return (cpu_ms as f64 / elapsed_ms as f64) * 100.0;
                    }
                }
                *last_instant_guard = Some(now);
            }
        }
        0.0
    }
}

/// 查询当前进程内存 — 平台特定实现
fn query_process_memory() -> (f64, f64, f64) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        unsafe {
            let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut pmc,
                pmc.cb,
            ) != 0 {
                let rss = pmc.WorkingSetSize as f64 / 1_048_576.0;
                let virt = pmc.QuotaPagedPoolUsage as f64 / 1_048_576.0; // rough proxy
                let commit = pmc.PagefileUsage as f64 / 1_048_576.0;
                return (rss, virt, commit);
            }
        }
        (0.0, 0.0, 0.0)
    }
    #[cfg(not(windows))]
    {
        // Linux: read /proc/self/status
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            let mut rss = 0f64;
            let mut virt = 0f64;
            let mut hwm = 0f64;
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb) = line.split_whitespace().nth(1).and_then(|s| s.parse::<f64>().ok()) {
                        rss = kb / 1024.0;
                    }
                }
                if line.starts_with("VmSize:") {
                    if let Some(kb) = line.split_whitespace().nth(1).and_then(|s| s.parse::<f64>().ok()) {
                        virt = kb / 1024.0;
                    }
                }
                if line.starts_with("VmHWM:") {
                    if let Some(kb) = line.split_whitespace().nth(1).and_then(|s| s.parse::<f64>().ok()) {
                        hwm = kb / 1024.0;
                    }
                }
            }
            return (rss, virt, hwm);
        }
        (0.0, 0.0, 0.0)
    }
}

// ═══ v13.1: 扩展系统查询 ═══

fn query_hostname() -> String {
    // 跨平台: 优先环境变量, 回退到 hostname 命令
    if let Ok(h) = std::env::var("COMPUTERNAME") {
        return h;
    }
    if let Ok(h) = std::env::var("HOSTNAME") {
        return h;
    }
    // 回退: 执行 hostname 命令
    if let Ok(out) = std::process::Command::new("hostname").output() {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    "unknown".to_string()
}

fn query_disk_info() -> (f64, f64) {
    #[cfg(windows)]
    {
        // 查询当前工作目录所在盘符
        if let Ok(cwd) = std::env::current_dir() {
            if let Some(root) = cwd.to_str().and_then(|s| s.chars().next()) {
                let drive = format!("{}:\\", root);
                unsafe {
                    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExA;
                    let mut total: u64 = 0;
                    let mut free: u64 = 0;
                    let drive_cstr = std::ffi::CString::new(drive.clone()).unwrap_or_default();
                    if GetDiskFreeSpaceExA(drive_cstr.as_ptr() as *const u8, std::ptr::null_mut(), &mut total, &mut free) != 0 {
                        return (total as f64 / 1_073_741_824.0, free as f64 / 1_073_741_824.0);
                    }
                }
            }
        }
        (0.0, 0.0)
    }
    #[cfg(not(windows))]
    {
        // Linux: df 命令获取当前目录磁盘信息
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(out) = std::process::Command::new("df")
                .arg("-BG")
                .arg(cwd.to_string_lossy().as_ref())
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout);
                    // 解析 df -BG 输出: Filesystem 1G-blocks Used Available Use% Mounted
                    if let Some(line) = s.lines().nth(1) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            let total = parts[1].trim_end_matches('G').parse::<f64>().unwrap_or(0.0);
                            let free = parts[3].trim_end_matches('G').parse::<f64>().unwrap_or(0.0);
                            return (total, free);
                        }
                    }
                }
            }
        }
        (0.0, 0.0)
    }
}

// ═══ v14.0: 系统级全网收发统计 ═══

static LAST_SYS_NET_RX: AtomicU64 = AtomicU64::new(0);
static LAST_SYS_NET_TX: AtomicU64 = AtomicU64::new(0);
// BUG-FIX: 改用 Instant 追踪真实墙钟时间
static LAST_SYS_NET_INSTANT: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

/// 查询系统所有网卡累计收发字节 (不包含 loopback)
fn query_system_net_io() -> (u64, u64) {
    #[cfg(windows)]
    {
        // 使用 iphlpapi GetIfTable2 获取所有接口统计
        // 回退方案: 直接调用 netstat -e 解析输出
        if let Ok(out) = std::process::Command::new("netstat")
            .args(["-e"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            // 输出格式:
            // Interface Statistics
            //                            Received            Sent
            // Bytes                      123456789       987654321
            // ...
            let mut rx: u64 = 0;
            let mut tx: u64 = 0;
            for line in s.lines() {
                if line.contains("Bytes") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        rx = parts[1].parse().unwrap_or(0);
                        tx = parts[2].parse().unwrap_or(0);
                    }
                }
            }
            return (rx, tx);
        }
        (0, 0)
    }
    #[cfg(not(windows))]
    {
        // Linux: parse /proc/net/dev
        if let Ok(dev) = std::fs::read_to_string("/proc/net/dev") {
            let mut rx: u64 = 0;
            let mut tx: u64 = 0;
            for line in dev.lines().skip(2) {
                // eth0: 123456 0 0 0 0 0 0 0  654321 0 0 0 0 0 0 0
                if let Some(colon) = line.find(':') {
                    let rest = line[colon+1..].trim();
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() >= 9 {
                        let iface = line[..colon].trim();
                        if iface != "lo" {
                            rx += parts[0].parse::<u64>().unwrap_or(0);
                            tx += parts[8].parse::<u64>().unwrap_or(0);
                        }
                    }
                }
            }
            return (rx, tx);
        }
        (0, 0)
    }
}

/// 计算系统网速 (bytes/sec, 基于前后两次采样)
/// BUG-FIX: 使用 Instant 追踪真实墙钟时间, 而非 now.elapsed() 近零噪声
fn query_system_net_speed() -> (f64, f64) {
    let (rx, tx) = query_system_net_io();
    let now = Instant::now();

    let mut last_instant_guard = LAST_SYS_NET_INSTANT.lock().unwrap();
    let last_rx = LAST_SYS_NET_RX.swap(rx, Ordering::Relaxed);
    let last_tx = LAST_SYS_NET_TX.swap(tx, Ordering::Relaxed);

    if let Some(last_instant) = *last_instant_guard {
        let elapsed = now.duration_since(last_instant);
        let elapsed_s = elapsed.as_secs_f64();
        *last_instant_guard = Some(now);
        if elapsed_s < 0.1 { return (0.0, 0.0); }
        let rx_speed = rx.saturating_sub(last_rx) as f64 / elapsed_s;
        let tx_speed = tx.saturating_sub(last_tx) as f64 / elapsed_s;
        (rx_speed, tx_speed)
    } else {
        *last_instant_guard = Some(now);
        (0.0, 0.0)
    }
}

fn query_network_ips() -> Vec<String> {
    let mut ips = Vec::new();
    #[cfg(windows)]
    {
        // 使用 ipconfig 风格获取 (通过 socket 绑定方式)
        use std::net::UdpSocket;
        // 尝试连接一个外部地址来获取默认接口IP
        if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
            if let Ok(()) = s.connect("8.8.8.8:53") {
                if let Ok(addr) = s.local_addr() {
                    ips.push(addr.ip().to_string());
                }
            }
        }
        // 也尝试获取本机所有地址
        if let Ok(hostname) = query_hostname().parse::<std::net::IpAddr>().or_else(|_| {
            "localhost".parse()
        }) {
            let _ = hostname;
        }
    }
    #[cfg(not(windows))]
    {
        // Linux: parse /proc/net/fib_trie or use getifaddrs
        if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if let Ok(()) = s.connect("8.8.8.8:53") {
                if let Ok(addr) = s.local_addr() {
                    ips.push(addr.ip().to_string());
                }
            }
        }
    }
    if ips.is_empty() {
        ips.push("127.0.0.1".to_string());
    }
    ips
}

// ═══ 读取接口 (UI 渲染层调用) ═══

pub struct TelemetrySnapshot {
    pub network: Vec<NetworkEvent>,
    pub tools: Vec<ToolEvent>,
    pub ai: Vec<AiEvent>,
    pub system: Vec<SystemEvent>,
    pub net_bytes_up: u64,
    pub net_bytes_down: u64,
    pub net_connections: u64,
    pub net_speed_up_bps: f64,    // 上行速率 bytes/sec (滑动窗口)
    pub net_speed_down_bps: f64,  // 下行速率 bytes/sec
    pub net_window_secs: u64,     // 滑动窗口秒数
    pub proc: ProcessStats,
}

pub fn snapshot() -> TelemetrySnapshot {
    let (rss, virt, commit) = query_process_memory();
    let hostname = query_hostname();
    let (disk_total, disk_free) = query_disk_info();
    let network_ips = query_network_ips();
    let cpu_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let sys_ram = query_system_ram_mb();
    let cpu_pct = query_cpu_percent();
    let (sys_rx_bps, sys_tx_bps) = query_system_net_speed();
    // 获取累计值 (用于显示总流量)
    let (sys_rx, sys_tx) = (
        LAST_SYS_NET_RX.load(Ordering::Relaxed),
        LAST_SYS_NET_TX.load(Ordering::Relaxed),
    );
    let work_dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "?".to_string());

    // ★ BUG-23 修复: 中毒锁自动恢复
    let buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let window_secs = buf.net_window_start.elapsed().as_secs().max(1);
    let speed_up = buf.net_bytes_up as f64 / window_secs as f64;
    let speed_down = buf.net_bytes_down as f64 / window_secs as f64;
    TelemetrySnapshot {
        network: buf.network.clone(),
        tools: buf.tools.clone(),
        ai: buf.ai.clone(),
        system: buf.system.clone(),
        net_bytes_up: buf.net_bytes_up,
        net_bytes_down: buf.net_bytes_down,
        net_connections: buf.net_connections,
        net_speed_up_bps: speed_up,
        net_speed_down_bps: speed_down,
        net_window_secs: window_secs,
        proc: ProcessStats {
            uptime_secs: buf.startup.elapsed().as_secs(),
            pid: std::process::id(),
            thread_count: query_thread_count(),
            memory_rss_mb: rss,
            memory_virtual_mb: virt,
            memory_commit_mb: commit,
            system_ram_total_mb: sys_ram,
            cpu_percent: cpu_pct,
            total_tool_calls: buf.total_tool_calls,
            total_tool_errors: buf.total_tool_errors,
            total_ai_requests: buf.total_ai_requests,
            total_net_connections: buf.net_connections,
            total_sys_events: buf.system.len() as u64,
            hostname,
            cpu_cores,
            disk_total_gb: disk_total,
            disk_free_gb: disk_free,
            network_ips,
            work_dir,
            sys_net_rx_bytes: sys_rx,
            sys_net_tx_bytes: sys_tx,
            sys_net_rx_bps: sys_rx_bps,
            sys_net_tx_bps: sys_tx_bps,
        },
    }
}

/// 获取当前正在执行的工具名 (最近一个 Started 且无 Success/Failed 匹配的)
pub fn active_tool_name() -> Option<String> {
    let buf = match TELEMETRY.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    buf.tools.iter().rev()
        .filter(|e| matches!(e.status, ToolStatus::Started))
        .map(|e| e.tool_name.clone())
        .next()
}
