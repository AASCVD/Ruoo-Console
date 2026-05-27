use std::collections::HashMap;
use std::path::PathBuf;


/// 驱动类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriverKind {
    /// Windows 内核驱动 (.sys) — sc.exe 管理
    WindowsSys,
    /// Linux 内核模块 (.ko) — insmod/rmmod 管理
    LinuxKo,
    /// 未知
    Unknown,
}

impl DriverKind {
    pub fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("sys") => Self::WindowsSys,
            Some("ko") => Self::LinuxKo,
            _ => Self::Unknown,
        }
    }
}

/// 驱动状态
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum DriverStatus {
    Loaded,
    Stopped,
    Pending,
    Error(String),
}

/// 驱动信息
#[allow(dead_code)]
pub(crate) struct DriverInfo {
    name: String,
    path: PathBuf,
    kind: DriverKind,
    status: DriverStatus,
    load_time: Option<std::time::Instant>,
    /// 文件最后修改时间 (用于 watch-sys 热加载检测)
    last_modified: Option<std::time::SystemTime>,
}

/// v4.1: 驱动数字签名验证 (Windows)
#[cfg(windows)]
fn verify_driver_signature(path: &std::path::Path) -> Result<(), String> {
    // 尝试 PowerShell Get-AuthenticodeSignature
    let ps_cmd = format!(
        "Get-AuthenticodeSignature -FilePath '{}' | Select-Object -ExpandProperty Status",
        path.display()
    );
    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.contains("Valid") || stdout.contains("有效") {
                Ok(())
            } else if stdout.contains("NotSigned") || stdout.contains("未签名") {
                Err("驱动未数字签名 — 需要启用 testsigning 模式".into())
            } else {
                // PowerShell 不可用或返回其他结果，不阻塞
                Ok(())
            }
        }
        Err(_) => Ok(()), // PowerShell 不可用
    }
}
#[cfg(not(windows))]
fn verify_driver_signature(_path: &std::path::Path) -> Result<(), String> {
    Ok(()) // Linux .ko 模块不检查签名 (可扩展 modinfo 检查)
}

/// SysDriverManager — 内核驱动加载/卸载
///
/// Windows:
///   - 加载: sc.exe create <name> type=kernel binPath=<path> → sc start <name>
///   - 卸载: sc.exe stop <name> → sc.exe delete <name>
///   - 状态: sc.exe query <name>
///   - 需 Administrator 权限 + 驱动签名 (或 testsigning)
///
/// Linux:
///   - 加载: sudo insmod <path> (或 modprobe <name>)
///   - 卸载: sudo rmmod <name>
///   - 状态: lsmod | grep <name>
///   - 需 root 权限
pub struct SysDriverManager {
    pub(crate) loaded: HashMap<String, DriverInfo>,
    /// 热加载监控
    pub(crate) watch: bool,
}

impl SysDriverManager {
    pub fn new() -> Self {
        Self { loaded: HashMap::new(), watch: false }
    }

    pub fn set_watch(&mut self, enabled: bool) {
        self.watch = enabled;
    }

    /// 加载内核驱动
    ///
    /// Windows (.sys): sc create + sc start
    /// Linux (.ko):    sudo insmod
    pub fn load(&mut self, name: &str, path: &str) -> Result<String, String> {
        if self.loaded.contains_key(name) {
            return Err(format!("驱动已加载: {} (先 sysunload 再加载)", name));
        }

        let file_path = PathBuf::from(path);
        if !file_path.exists() {
            return Err(format!("驱动文件不存在: {}", path));
        }

        let canonical = file_path.canonicalize()
            .map_err(|e| format!("无法解析路径: {} ({})", path, e))?;

        let kind = DriverKind::from_path(&canonical);

        // v4.1: Windows .sys 签名检查
        #[cfg(windows)]
        if kind == DriverKind::WindowsSys {
            match verify_driver_signature(&canonical) {
                Ok(()) => {} // 签名有效
                Err(msg) => {
                    // 仅警告，不阻止 (测试签名模式不总是可用)
                    eprintln!("[!] 驱动签名警告: {} — {}", canonical.display(), msg);
                }
            }
        }

        match kind {
            DriverKind::WindowsSys => self.load_windows_sys(name, &canonical),
            DriverKind::LinuxKo => self.load_linux_ko(name, &canonical),
            DriverKind::Unknown => {
                // 尝试根据操作系统推断
                #[cfg(windows)]
                { self.load_windows_sys(name, &canonical) }
                #[cfg(not(windows))]
                { self.load_linux_ko(name, &canonical) }
            }
        }?;

        // 获取文件 mtime (用于热加载检测)
        let mtime = canonical.metadata()
            .and_then(|m| m.modified())
            .ok();

        self.loaded.insert(name.to_string(), DriverInfo {
            name: name.to_string(),
            path: canonical,
            kind,
            status: DriverStatus::Loaded,
            load_time: Some(std::time::Instant::now()),
            last_modified: mtime,
        });

        Ok(format!(
            "[⚙] 内核驱动已加载: {} | 类型: {:?} | 路径: {}",
            name, kind, path
        ))
    }

    /// Windows: sc.exe create + start
    fn load_windows_sys(&self, name: &str, path: &PathBuf) -> Result<(), String> {
        let path_str = path.display().to_string();

        // 1. 创建服务
        let create_output = std::process::Command::new("sc.exe")
            .args(["create", name, "type=", "kernel", "binPath=", &path_str, "start=", "demand"])
            .output()
            .map_err(|e| format!("无法执行 sc.exe create: {}", e))?;

        let create_stdout = String::from_utf8_lossy(&create_output.stdout);
        let create_stderr = String::from_utf8_lossy(&create_output.stderr);

        if !create_output.status.success() {
            // [SC] CreateService SUCCESS 也返回 success
            let combined = format!("{}{}", create_stdout, create_stderr);
            if !combined.contains("SUCCESS") && !combined.contains("已存在") && !combined.contains("already exists") {
                return Err(format!("sc create 失败: {}", combined.trim()));
            }
        }

        // 2. 启动驱动
        let start_output = std::process::Command::new("sc.exe")
            .args(["start", name])
            .output()
            .map_err(|e| format!("无法执行 sc.exe start: {}", e))?;

        let start_stdout = String::from_utf8_lossy(&start_output.stdout);
        let start_stderr = String::from_utf8_lossy(&start_output.stderr);
        let combined = format!("{}{}", start_stdout, start_stderr);

        if !start_output.status.success()
            && !combined.contains("SUCCESS")
            && !combined.contains("正在运行")
            && !combined.contains("already running")
        {
            // 尝试删除已创建的服务
            let _ = std::process::Command::new("sc.exe")
                .args(["delete", name])
                .output();
            return Err(format!("sc start 失败: {}", combined.trim()));
        }

        Ok(())
    }

    /// Linux: sudo insmod
    fn load_linux_ko(&self, name: &str, path: &PathBuf) -> Result<(), String> {
        let path_str = path.display().to_string();

        let output = std::process::Command::new("sudo")
            .args(["insmod", &path_str])
            .output()
            .map_err(|e| format!("无法执行 sudo insmod: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // 尝试 modprobe
            let output2 = std::process::Command::new("sudo")
                .args(["modprobe", name])
                .output()
                .map_err(|_| format!("insmod 失败: {}", stderr.trim()))?;

            if !output2.status.success() {
                let stderr2 = String::from_utf8_lossy(&output2.stderr);
                return Err(format!(
                    "insmod/modprobe 均失败:\n  insmod: {}\n  modprobe: {}",
                    stderr.trim(), stderr2.trim()
                ));
            }
        }

        Ok(())
    }

    /// 卸载内核驱动
    ///
    /// Windows: sc stop + sc delete
    /// Linux:    sudo rmmod
    pub fn unload(&mut self, name: &str) -> Result<(), String> {
        let info = self.loaded.remove(name)
            .ok_or_else(|| format!("驱动未加载: {}", name))?;

        match info.kind {
            DriverKind::WindowsSys => self.unload_windows_sys(name),
            DriverKind::LinuxKo => self.unload_linux_ko(name),
            DriverKind::Unknown => {
                #[cfg(windows)]
                { self.unload_windows_sys(name) }
                #[cfg(not(windows))]
                { self.unload_linux_ko(name) }
            }
        }?;

        Ok(())
    }

    fn unload_windows_sys(&self, name: &str) -> Result<(), String> {
        // 1. 停止驱动
        let stop_output = std::process::Command::new("sc.exe")
            .args(["stop", name])
            .output()
            .map_err(|e| format!("无法执行 sc.exe stop: {}", e))?;

        let stop_combined = format!(
            "{}{}",
            String::from_utf8_lossy(&stop_output.stdout),
            String::from_utf8_lossy(&stop_output.stderr)
        );

        // 停止可能成功也可能已经停止
        if !stop_output.status.success()
            && !stop_combined.contains("STOPPED")
            && !stop_combined.contains("已停止")
            && !stop_combined.contains("not running")
            && !stop_combined.contains("未运行")
        {
            // 非致命 — 继续删除
        }

        // 2. 删除服务
        let delete_output = std::process::Command::new("sc.exe")
            .args(["delete", name])
            .output()
            .map_err(|e| format!("无法执行 sc.exe delete: {}", e))?;

        let delete_combined = format!(
            "{}{}",
            String::from_utf8_lossy(&delete_output.stdout),
            String::from_utf8_lossy(&delete_output.stderr)
        );

        if !delete_output.status.success()
            && !delete_combined.contains("SUCCESS")
            && !delete_combined.contains("成功")
        {
            return Err(format!("sc delete 失败: {}", delete_combined.trim()));
        }

        Ok(())
    }

    fn unload_linux_ko(&self, name: &str) -> Result<(), String> {
        let output = std::process::Command::new("sudo")
            .args(["rmmod", name])
            .output()
            .map_err(|e| format!("无法执行 sudo rmmod: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("rmmod 失败: {}", stderr.trim()));
        }

        Ok(())
    }

    /// 查询驱动状态
    pub fn status(&self, name: &str) -> Result<String, String> {
        let info = self.loaded.get(name)
            .ok_or_else(|| format!("驱动未加载: {}", name))?;

        let os_status = match info.kind {
            DriverKind::WindowsSys => self.query_windows_sys(name),
            DriverKind::LinuxKo => self.query_linux_ko(name),
            DriverKind::Unknown => {
                #[cfg(windows)]
                { self.query_windows_sys(name) }
                #[cfg(not(windows))]
                { self.query_linux_ko(name) }
            }
        };

        let uptime = info.load_time
            .map(|t| format!("{:.1}s", t.elapsed().as_secs_f64()))
            .unwrap_or_else(|| "N/A".into());

        Ok(format!(
            "  驱动: {} | 类型: {:?} | 路径: {} | 运行时长: {} | OS状态: {}",
            name, info.kind, info.path.display(), uptime, os_status
        ))
    }

    fn query_windows_sys(&self, name: &str) -> String {
        match std::process::Command::new("sc.exe")
            .args(["query", name])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("RUNNING") || stdout.contains("正在运行") {
                    "RUNNING".into()
                } else if stdout.contains("STOPPED") || stdout.contains("已停止") {
                    "STOPPED".into()
                } else {
                    "UNKNOWN".into()
                }
            }
            Err(_) => "QUERY_FAILED".into(),
        }
    }

    fn query_linux_ko(&self, name: &str) -> String {
        match std::process::Command::new("lsmod")
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.lines().any(|l| l.starts_with(name)) {
                    "LOADED".into()
                } else {
                    "NOT_FOUND".into()
                }
            }
            Err(_) => "QUERY_FAILED".into(),
        }
    }

    /// 热加载检测
    #[allow(dead_code)]
    fn check_hotload(&mut self, name: &str) -> Result<Option<String>, String> {
        if !self.watch { return Ok(None); }

        let (path, need_reload) = {
            let info = match self.loaded.get(name) {
                Some(i) => i,
                None => return Ok(None),
            };

            let current_mtime = match info.path.metadata().and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => return Ok(None),
            };

            let need = match info.last_modified {
                Some(last) => current_mtime > last,
                None => false,
            };

            (info.path.clone(), need)
        };

        if need_reload {
            // 文件已修改 → 热重载
            match self.hotload_inner(name, &path) {
                Ok(msg) => Ok(Some(msg)),
                Err(e) => {
                    self.loaded.remove(name);
                    Err(format!("驱动热加载失败，已卸载: {}", e))
                }
            }
        } else {
            Ok(None)
        }
    }

    /// 内部热加载实现 (驱动)
    fn hotload_inner(&mut self, name: &str, path: &PathBuf) -> Result<String, String> {
        let path_str = path.display().to_string();
        // 卸载 → 重载
        self.unload_inner(name)?;
        self.load(name, &path_str)?;
        Ok(format!("[↻] 驱动热加载完成: {}", name))
    }

    /// 内部卸载 (不改变 loaded map - 供 hotload 使用)
    fn unload_inner(&mut self, name: &str) -> Result<(), String> {
        let info = self.loaded.remove(name)
            .ok_or_else(|| format!("驱动未加载: {}", name))?;

        match info.kind {
            DriverKind::WindowsSys => self.unload_windows_sys(name),
            DriverKind::LinuxKo => self.unload_linux_ko(name),
            DriverKind::Unknown => {
                #[cfg(windows)]
                { self.unload_windows_sys(name) }
                #[cfg(not(windows))]
                { self.unload_linux_ko(name) }
            }
        }
    }

    /// 手动热加载驱动
    pub fn hotload(&mut self, name: &str) -> Result<String, String> {
        let path = match self.loaded.get(name) {
            Some(info) => info.path.clone(),
            None => return Err(format!("驱动未加载: {}", name)),
        };
        self.hotload_inner(name, &path)
    }

    /// 列出已加载驱动
    pub fn list(&self) -> Vec<String> {
        self.loaded.keys().cloned().collect()
    }

    pub fn list_detailed(&self) -> Vec<String> {
        self.loaded.iter().map(|(name, info)| {
            format!(
                "  {} | 类型: {:?} | 路径: {} | 状态: {:?}",
                name, info.kind, info.path.display(), info.status
            )
        }).collect()
    }

    pub fn count(&self) -> usize {
        self.loaded.len()
    }
}

impl Drop for SysDriverManager {
    fn drop(&mut self) {
        // 驱动不自动卸载 — 需要显式 sysunload
        // 自动卸载可能导致系统不稳定
        if !self.loaded.is_empty() {
            let names: Vec<String> = self.loaded.keys().cloned().collect();
            eprintln!(
                "[!] SysDriverManager: {} 驱动仍在加载中 ({}) — 需手动 sysunload",
                names.len(),
                names.join(", ")
            );
        }
    }
}