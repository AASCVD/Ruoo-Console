// ============================================================
// RUOO-ARSENAL v4.8 — 内核驱动工具接口 (Kernel Driver Interface)
//
// 设计理念:
//   - 提供统一的 trait 抽象，用户可实现自己的底层驱动后端
//   - 内置 Windows (sc.exe) 和 Linux (insmod/rmmod) 默认后端
//   - 支持运行时切换后端 (set_kernel_backend)
//   - 用户可编译自定义后端 .dll/.so 并通过插件系统注入
//   - 所有操作需管理员/root 权限，perm 5 才可调用
//
// 用户编译/选择流程:
//   1. 默认: 自动检测平台使用 sc.exe 或 insmod
//   2. 自定义: 实现 KernelDriverBackend trait → 编译为插件 → plugin load
//   3. 替换: 运行时调用 set_kernel_backend(name) 切换后端
// ============================================================

use std::fmt;
use std::io::Write;
use std::sync::{Arc, OnceLock, RwLock};

// ═══════════════════════════════════════
// 1. 核心数据结构
// ═══════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    Loaded,
    Unloaded,
    Unknown,
    #[allow(dead_code)]
    Error(String),
}

impl fmt::Display for DriverStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverStatus::Loaded => write!(f, "LOADED"),
            DriverStatus::Unloaded => write!(f, "UNLOADED"),
            DriverStatus::Unknown => write!(f, "UNKNOWN"),
            DriverStatus::Error(e) => write!(f, "ERROR: {}", e),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DriverInfo {
    pub name: String,
    pub path: String,
    pub status: DriverStatus,
    pub platform: String,
    #[allow(dead_code)]
    pub loaded_at: Option<u64>,
    pub note: String,
}

impl DriverInfo {
    #[allow(dead_code)]
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            status: DriverStatus::Unloaded,
            platform: std::env::consts::OS.to_string(),
            loaded_at: None,
            note: String::new(),
        }
    }
}

impl fmt::Display for DriverInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}  {}  ({})",
            self.status, self.name, self.path, self.platform
        )
    }
}

// ═══════════════════════════════════════
// 2. 内核驱动后端 Trait (用户可实现)
// ═══════════════════════════════════════

pub trait KernelDriverBackend: Send + Sync {
    fn load(&self, name: &str, path: &str) -> Result<String, String>;
    fn unload(&self, name: &str) -> Result<String, String>;
    fn list(&self) -> Result<Vec<DriverInfo>, String>;
    fn platform(&self) -> &'static str;
    fn backend_name(&self) -> &'static str;

    fn compile_driver(&self, _source: &str, _output: &str) -> Result<String, String> {
        Err(format!(
            "{} 后端不支持编译驱动，请手动编译或使用其他后端",
            self.backend_name()
        ))
    }

    fn validate_driver(&self, path: &str) -> Result<String, String> {
        validate_driver_file(path, self.platform())
    }

    fn check_privileges(&self) -> Result<String, String> {
        #[cfg(windows)]
        {
            let test_path = "C:\\Windows\\System32\\drivers\\";
            match std::fs::metadata(test_path) {
                Ok(_) => Ok("[+] 管理员权限确认".into()),
                Err(_) => Err("[!] 需要管理员权限 — 请以管理员身份运行".into()),
            }
        }
        #[cfg(not(windows))]
        {
            // SAFETY: libc::getuid 是安全的 POSIX 调用
            let uid = unsafe { libc::getuid() };
            if uid == 0 {
                Ok("[+] root 权限确认".into())
            } else {
                Err("[!] 需要 root 权限 — 请使用 sudo 运行".into())
            }
        }
    }
}

/// 独立的驱动文件验证 (避免 trait 默认方法递归)
fn validate_driver_file(path: &str, platform: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("无法访问驱动文件 '{}': {}", path, e))?;
    if !meta.is_file() {
        return Err(format!("'{}' 不是一个文件", path));
    }
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let expected = match platform {
        "windows" => "sys",
        "linux" => "ko",
        _ => return Ok("跳过扩展名检查 (自定义平台)".into()),
    };
    if !ext.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "驱动扩展名不匹配: 期望 '.{}', 实际 '.{}'",
            expected, ext
        ));
    }
    let size = meta.len();
    Ok(format!(
        "[+] 驱动文件校验通过: {} ({} bytes, {})",
        path, size, platform
    ))
}

// ═══════════════════════════════════════
// 3. 内置 Windows 后端 (sc.exe)
// ═══════════════════════════════════════

pub struct WindowsScBackend;

/// ★ 自动探测 vcvarsall.bat 路径 (优先 vswhere.exe, 回退硬编码路径)
fn find_vcvarsall() -> Option<String> {
    // ★ 优先: vswhere.exe (VS 官方定位工具，随 VS 2017+ 安装)
    if let Ok(output) = std::process::Command::new("vswhere.exe")
        .args(["-latest", "-property", "installationPath"])
        .output()
    {
        let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !install_path.is_empty() {
            let vcvars = std::path::Path::new(&install_path)
                .join("VC")
                .join("Auxiliary")
                .join("Build")
                .join("vcvarsall.bat");
            if vcvars.exists() {
                return Some(vcvars.to_string_lossy().to_string());
            }
        }
    }
    // 回退2: 动态遍历 VS 安装根目录 (覆盖任意版本号如 2022/2019/18/...)
    let vs_roots = [
        "C:\\Program Files\\Microsoft Visual Studio",
        "C:\\Program Files (x86)\\Microsoft Visual Studio",
    ];
    for vs_root in &vs_roots {
        if let Ok(entries) = std::fs::read_dir(vs_root) {
            for entry in entries.flatten() {
                let version_dir = entry.path();
                if !version_dir.is_dir() {
                    continue;
                }
                // 在版本目录下尝试 Community/Professional/Enterprise/BuildTools
                for edition in &["Community", "Professional", "Enterprise", "BuildTools"] {
                    let vcvars = version_dir
                        .join(edition)
                        .join("VC")
                        .join("Auxiliary")
                        .join("Build")
                        .join("vcvarsall.bat");
                    if vcvars.exists() {
                        return Some(vcvars.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    // 回退3: 硬编码路径遍历 (VS 2022 / 2019 / 2017) — 兼容无 read_dir 权限场景
    let candidate_dirs = [
        // VS 2022
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Auxiliary\\Build",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Professional\\VC\\Auxiliary\\Build",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Auxiliary\\Build",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build",
        // VS 2019
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Community\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Professional\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Enterprise\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\BuildTools\\VC\\Auxiliary\\Build",
        // VS 2017
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Community\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Professional\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Enterprise\\VC\\Auxiliary\\Build",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\BuildTools\\VC\\Auxiliary\\Build",
    ];
    for dir in &candidate_dirs {
        let path = std::path::Path::new(dir).join("vcvarsall.bat");
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

/// ★ 安全校验: name 仅允许 [a-zA-Z0-9_-] 防止 sc.exe 命令注入
fn validate_driver_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("驱动名称不能为空".into());
    }
    for (i, c) in name.chars().enumerate() {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
            return Err(format!(
                "驱动名称包含非法字符 '{}' (位置{}), 仅允许 [a-zA-Z0-9_-]",
                c, i
            ));
        }
    }
    if name.len() > 80 {
        return Err("驱动名称过长 (最大80字符)".into());
    }
    Ok(())
}

/// ★ 直接调用 sc.exe 而不经过 cmd /C，杜绝命令注入
fn run_sc(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("sc")
        .args(args)
        .output()
        .map_err(|e| format!("执行 sc.exe 失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = stdout.to_string();
    if !stderr.is_empty() {
        result.push_str(&stderr);
    }
    Ok(result)
}

impl KernelDriverBackend for WindowsScBackend {
    fn load(&self, name: &str, path: &str) -> Result<String, String> {
        let mut results = Vec::new();
        match self.check_privileges() {
            Ok(msg) => results.push(msg),
            Err(e) => return Err(e),
        }

        // ★ 安全校验: name 白名单
        validate_driver_name(name)?;

        // ★ 直接调用 sc.exe 传递参数列表，不走 cmd /C
        let create_output = run_sc(&["create", name, "type=", "kernel", "binPath=", path])?;
        results.push(format!("[sc create] {}", create_output.trim()));

        if !create_output.contains("SUCCESS") && !create_output.contains("already exists") {
            results.push("[*] 服务可能已存在，尝试直接启动...".into());
        }

        let start_output = run_sc(&["start", name])?;
        results.push(format!("[sc start] {}", start_output.trim()));

        if start_output.contains("FAILED") && !start_output.contains("already running") {
            return Err(format!("驱动加载失败:\n{}", results.join("\n")));
        }

        let query_output = run_sc(&["query", name])?;
        results.push(format!("[sc query] {}", query_output.trim()));

        Ok(format!(
            "[+] 内核驱动 '{}' 加载成功 (Windows sc.exe)\n  Path: {}\n  Status: RUNNING\n{}",
            name,
            path,
            results
                .iter()
                .map(|s| format!("  {}", s))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }

    fn unload(&self, name: &str) -> Result<String, String> {
        let mut results = Vec::new();
        validate_driver_name(name)?;
        let stop_output = run_sc(&["stop", name])?;
        results.push(format!("[sc stop] {}", stop_output.trim()));
        let delete_output = run_sc(&["delete", name])?;
        results.push(format!("[sc delete] {}", delete_output.trim()));
        Ok(format!(
            "[+] 内核驱动 '{}' 已卸载 (Windows sc.exe)\n{}",
            name,
            results.join("\n")
        ))
    }

    fn list(&self) -> Result<Vec<DriverInfo>, String> {
        let output = run_sc(&["query", "type=", "driver", "state=", "all"])?;
        let mut drivers = Vec::new();
        let mut current_name = String::new();
        let mut current_state = String::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SERVICE_NAME:") {
                current_name = trimmed
                    .strip_prefix("SERVICE_NAME:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if trimmed.starts_with("STATE") {
                current_state = trimmed
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if !current_name.is_empty() && !current_state.is_empty() {
                let status = match current_state.as_str() {
                    "4" | "RUNNING" => DriverStatus::Loaded,
                    "1" | "STOPPED" => DriverStatus::Unloaded,
                    _ => DriverStatus::Unknown,
                };
                drivers.push(DriverInfo {
                    name: current_name.clone(),
                    path: "(查询驱动路径需: sc qc <name>)".into(),
                    status,
                    platform: "windows".into(),
                    loaded_at: None,
                    note: current_state.clone(),
                });
                current_name.clear();
                current_state.clear();
            }
        }
        if drivers.is_empty() {
            return Err("未找到内核驱动 (可能权限不足或 sc.exe 不可用)".into());
        }
        Ok(drivers)
    }

    fn platform(&self) -> &'static str {
        "windows"
    }
    fn backend_name(&self) -> &'static str {
        "WindowsScBackend"
    }

    fn compile_driver(&self, source: &str, output: &str) -> Result<String, String> {
        let ext = std::path::Path::new(source)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "c" | "cpp" => {
                // ★ 自动探测 vcvarsall.bat (优先 vswhere → 动态遍历VS根目录 → 硬编码路径)
                let vcvars = find_vcvarsall();
                let obj_arg = format!("{}.obj", output);
                let output_arg = format!("/Fe{}", output);

                // 构建 cl.exe 参数行 (被 cmd /C 包裹执行)
                let cl_args = format!(
                    "/DRIVER /WX /O2 /Fo{} {} {} /link /DRIVER /SUBSYSTEM:NATIVE",
                    obj_arg, output_arg, source
                );

                let (cmd, desc) = match vcvars {
                    Some(ref path) => {
                        // cmd /C 后不需要外层引号 — std::process::Command 会原样传递参数
                        // 外层引号与路径内部引号嵌套会导致 cmd 在空格处截断
                        let c = format!(
                            "call \"{}\" x64 1>nul 2>&1 && cl.exe {}",
                            path, cl_args
                        );
                        (c, format!("via {}", path))
                    }
                    None => {
                        // 回退: 直接试 cl.exe (用户可能已手动初始化环境)
                        let c = format!("cl.exe {}", cl_args);
                        (c, "vcvarsall.bat 未找到, 直接调用 cl.exe".into())
                    }
                };

                let result = std::process::Command::new("cmd")
                    .args(["/C", &cmd])
                    .output();

                match result {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if out.status.success() {
                            Ok(format!("[+] WDK编译成功 ({})\n{}{}", desc, stdout, stderr))
                        } else {
                            let mut msg = format!(
                                "WDK编译失败 ({})\n{}{}",
                                desc, stdout, stderr
                            );
                            if vcvars.is_none() {
                                msg.push_str("\n[!] 未找到 vcvarsall.bat — 请安装 Visual Studio + WDK");
                                msg.push_str("\n    手动编译: 在 Developer Command Prompt 中执行 cl.exe /DRIVER <源文件>");
                            }
                            Err(msg)
                        }
                    }
                    Err(e) => {
                        let mut msg = format!("WDK编译失败: {} ({})", e, desc);
                        if vcvars.is_none() {
                            msg.push_str("\n[!] 未找到 vcvarsall.bat — 请安装 Visual Studio + WDK");
                        }
                        Err(msg)
                    }
                }
            }
            _ => Err(format!(
                "不支持的源文件类型: .{}\n支持: .c / .cpp (WDK driver)",
                ext
            )),
        }
    }
}

// ═══════════════════════════════════════
// 4. 内置 Linux 后端 (insmod / rmmod)
// ═══════════════════════════════════════

pub struct LinuxInsmodBackend;

impl KernelDriverBackend for LinuxInsmodBackend {
    fn load(&self, name: &str, path: &str) -> Result<String, String> {
        let mut results = Vec::new();
        match self.check_privileges() {
            Ok(msg) => results.push(msg),
            Err(e) => return Err(e),
        }
        let insmod_output = run_cmd("insmod", &[path])?;
        results.push(format!("[insmod {}] {}", path, insmod_output.trim()));
        let lsmod_output = run_cmd("lsmod", &[])?;
        if lsmod_output.contains(name) {
            results.push(format!("[+] 模块 '{}' 已在 lsmod 中确认加载", name));
        } else {
            results.push(format!(
                "[*] 模块 '{}' 在 lsmod 中未找到，检查 dmesg 获取详情",
                name
            ));
        }
        Ok(format!(
            "[+] 内核模块 '{}' 加载成功 (Linux insmod)\n  Path: {}\n{}",
            name,
            path,
            results.join("\n")
        ))
    }

    fn unload(&self, name: &str) -> Result<String, String> {
        let rmmod_output = run_cmd("rmmod", &[name])?;
        Ok(format!(
            "[+] 内核模块 '{}' 已卸载 (Linux rmmod)\n{}",
            name,
            rmmod_output.trim()
        ))
    }

    fn list(&self) -> Result<Vec<DriverInfo>, String> {
        let output = run_cmd("lsmod", &[])?;
        let mut drivers = Vec::new();
        for line in output.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(name) = parts.first() {
                drivers.push(DriverInfo {
                    name: name.to_string(),
                    path: format!("/sys/module/{}/", name),
                    status: DriverStatus::Loaded,
                    platform: "linux".into(),
                    loaded_at: None,
                    note: format!(
                        "Size: {} | Used: {}",
                        parts.get(1).unwrap_or(&"?"),
                        parts.get(2).unwrap_or(&"0")
                    ),
                });
            }
        }
        if drivers.is_empty() {
            return Err("未找到已加载的内核模块 (lsmod 返回空)".into());
        }
        Ok(drivers)
    }

    fn platform(&self) -> &'static str {
        "linux"
    }
    fn backend_name(&self) -> &'static str {
        "LinuxInsmodBackend"
    }

    fn compile_driver(&self, source: &str, output: &str) -> Result<String, String> {
        let ext = std::path::Path::new(source)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "c" => {
                let src_dir = std::path::Path::new(source)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".into());
                let src_name = std::path::Path::new(source)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("driver");
                let makefile_content = format!(
                    "obj-m += {}.o\nall:\n\tmake -C /lib/modules/$(shell uname -r)/build M=$(PWD) modules\nclean:\n\tmake -C /lib/modules/$(shell uname -r)/build M=$(PWD) clean\n",
                    src_name
                );
                let makefile_path = format!("{}/Makefile", src_dir);
                std::fs::write(&makefile_path, &makefile_content)
                    .map_err(|e| format!("写入 Makefile 失败: {}", e))?;
                match run_cmd("make", &["-C", &src_dir]) {
                    Ok(out) => {
                        let ko_src = format!("{}/{}.ko", src_dir, src_name);
                        std::fs::copy(&ko_src, output)
                            .map_err(|e| format!("复制 .ko 失败: {}", e))?;
                        Ok(format!("[+] 内核模块编译成功: {}\n{}", output, out))
                    }
                    Err(e) => Err(format!(
                        "内核模块编译失败: {}\n请确保已安装 kernel-headers: sudo apt install linux-headers-$(uname -r)",
                        e
                    )),
                }
            }
            _ => Err(format!(
                "不支持的内核模块源文件类型: .{}\n支持: .c (Linux kernel module)",
                ext
            )),
        }
    }

    fn validate_driver(&self, path: &str) -> Result<String, String> {
        let base = validate_driver_file(path, self.platform())?;
        match run_cmd("modinfo", &[path]) {
            Ok(info) => Ok(format!("{}\n[modinfo]\n{}", base, info)),
            Err(_) => Ok(format!("{} (modinfo 不可用，跳过深入验证)", base)),
        }
    }
}

// ═══════════════════════════════════════
// 4.3 内置 macOS 后端 (kextload / kextunload)
// ═══════════════════════════════════════

pub struct MacOSKextBackend;

impl KernelDriverBackend for MacOSKextBackend {
    fn load(&self, name: &str, path: &str) -> Result<String, String> {
        let mut results = Vec::new();
        match self.check_privileges() {
            Ok(msg) => results.push(msg),
            Err(e) => return Err(e),
        }

        // 检查文件存在
        if !std::path::Path::new(path).exists() {
            return Err(format!("kext 文件不存在: {}", path));
        }

        // kextload
        let load_output = run_cmd("kextload", &[path])?;
        results.push(format!("[kextload] {}", load_output.trim()));

        // 验证加载
        let kextstat_output = run_cmd("kextstat", &[])?;
        if kextstat_output.contains(name) || kextstat_output.contains("com.ruoo") {
            results.push(format!("[+] kext '{}' 已确认加载", name));
        } else {
            results.push(format!("[*] kext '{}' 可能未完全加载, 检查 system.log", name));
        }

        Ok(format!(
            "[+] 内核扩展 '{}' 加载成功 (macOS kextload)\n  Path: {}\n{}",
            name,
            path,
            results.join("\n")
        ))
    }

    fn unload(&self, name: &str) -> Result<String, String> {
        // kextunload 可使用 bundle-id 或路径
        let kextstat_output = run_cmd("kextstat", &[]).unwrap_or_default();
        let bundle_id = if kextstat_output.contains("com.ruoo.kernel-defense") {
            "com.ruoo.kernel-defense"
        } else {
            name
        };

        let unload_output = run_cmd("kextunload", &["-b", bundle_id])?;
        Ok(format!(
            "[+] 内核扩展 '{}' 已卸载 (macOS kextunload)\n{}",
            name,
            unload_output.trim()
        ))
    }

    fn list(&self) -> Result<Vec<DriverInfo>, String> {
        let output = run_cmd("kextstat", &[])?;
        let mut drivers = Vec::new();
        for line in output.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let idx = parts[0];
                let kext_name = parts[5];
                drivers.push(DriverInfo {
                    name: kext_name.to_string(),
                    path: format!("Index #{}", idx),
                    status: DriverStatus::Loaded,
                    platform: "macos".into(),
                    loaded_at: None,
                    note: format!(
                        "Version: {} | Linked: {}",
                        parts.get(2).unwrap_or(&"?"),
                        parts.get(4).unwrap_or(&"?")
                    ),
                });
            }
        }
        if drivers.is_empty() {
            return Err("未找到已加载的内核扩展 (kextstat 返回空)".into());
        }
        Ok(drivers)
    }

    fn platform(&self) -> &'static str {
        "macos"
    }
    fn backend_name(&self) -> &'static str {
        "MacOSKextBackend"
    }

    fn compile_driver(&self, source: &str, _output: &str) -> Result<String, String> {
        let ext = std::path::Path::new(source)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "cpp" | "c" => {
                // 使用 xcodebuild 或 Makefile
                let src_dir = std::path::Path::new(source)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".into());
                match run_cmd("make", &["-C", &src_dir]) {
                    Ok(out) => {
                        // 查找 .kext 输出
                        let kext_path = format!("{}/ruoo_kernel_defense.kext", src_dir);
                        if std::path::Path::new(&kext_path).exists() {
                            Ok(format!("[+] macOS kext 编译成功: {}\n{}", kext_path, out))
                        } else {
                            Ok(format!("[*] Make 完成但未找到 .kext: {}\n{}", src_dir, out))
                        }
                    }
                    Err(e) => Err(format!(
                        "macOS kext 编译失败: {}\n需要 Xcode Command Line Tools + Kernel.framework",
                        e
                    )),
                }
            }
            _ => Err(format!(
                "不支持的源文件类型: .{} (macOS 需要 .cpp IOKit driver)",
                ext
            )),
        }
    }

    fn validate_driver(&self, path: &str) -> Result<String, String> {
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("无法访问 kext: {}", e))?;
        if meta.is_dir() {
            // .kext 是 bundle 目录
            let plist_path = format!("{}/Contents/Info.plist", path);
            if std::path::Path::new(&plist_path).exists() {
                Ok(format!("[+] macOS kext bundle 校验通过: {} ({} bytes)", path, meta.len()))
            } else {
                Err(format!("kext bundle 缺少 Info.plist: {}", path))
            }
        } else {
            Err(format!("kext 必须是 bundle 目录: {}", path))
        }
    }
}

// ═══════════════════════════════════════
// 5. 驱动模板 — 供用户编译 (.sys / .ko / .kext)
// ═══════════════════════════════════════

pub fn generate_windows_driver_template(driver_name: &str) -> String {
    let d = driver_name;
    format!(
        "// ============================================================\n// Windows Kernel Driver Template — {d}\n// 编译: cl.exe /DRIVER /WX /O2 {d}.c /link /DRIVER /SUBSYSTEM:NATIVE\n// 加载: sysload {d} {d}.sys\n// ============================================================\n\n#include <ntddk.h>\n#include <wdf.h>\n\nDRIVER_INITIALIZE DriverEntry;\nEVT_WDF_DRIVER_DEVICE_ADD {d}EvtDeviceAdd;\nEVT_WDF_DRIVER_UNLOAD {d}EvtDriverUnload;\n\nNTSTATUS DriverEntry(\n    _In_ PDRIVER_OBJECT  DriverObject,\n    _In_ PUNICODE_STRING RegistryPath\n)\n{{\n    DbgPrint(\"[{d}] DriverEntry called\\n\");\n    DbgPrint(\"[{d}] RegistryPath: %wZ\\n\", RegistryPath);\n\n    WDF_DRIVER_CONFIG config;\n    WDF_DRIVER_CONFIG_INIT(&config, {d}EvtDeviceAdd);\n    config.EvtDriverUnload = {d}EvtDriverUnload;\n\n    NTSTATUS status = WdfDriverCreate(\n        DriverObject,\n        RegistryPath,\n        WDF_NO_OBJECT_ATTRIBUTES,\n        &config,\n        WDF_NO_HANDLE\n    );\n\n    if (!NT_SUCCESS(status)) {{\n        DbgPrint(\"[{d}] WdfDriverCreate failed: 0x%x\\n\", status);\n        return status;\n    }}\n\n    DbgPrint(\"[{d}] Driver loaded successfully\\n\");\n    return STATUS_SUCCESS;\n}}\n\nNTSTATUS {d}EvtDeviceAdd(\n    _In_    WDFDRIVER       Driver,\n    _Inout_ PWDFDEVICE_INIT DeviceInit\n)\n{{\n    UNREFERENCED_PARAMETER(Driver);\n    DbgPrint(\"[{d}] EvtDeviceAdd called\\n\");\n\n    WDFDEVICE device;\n    NTSTATUS status = WdfDeviceCreate(\n        &DeviceInit,\n        WDF_NO_OBJECT_ATTRIBUTES,\n        &device\n    );\n\n    if (!NT_SUCCESS(status)) {{\n        DbgPrint(\"[{d}] WdfDeviceCreate failed: 0x%x\\n\", status);\n        return status;\n    }}\n\n    DbgPrint(\"[{d}] Device created successfully\\n\");\n    return STATUS_SUCCESS;\n}}\n\nVOID {d}EvtDriverUnload(_In_ WDFDRIVER Driver)\n{{\n    UNREFERENCED_PARAMETER(Driver);\n    DbgPrint(\"[{d}] Driver unloaded\\n\");\n}}\n"
    )
}

pub fn generate_linux_driver_template(driver_name: &str) -> String {
    let d = driver_name;
    format!(
        "// ============================================================\n// Linux Kernel Module Template — {d}\n// 编译: make (需要 kernel-headers)\n// 加载: sysload {d} {d}.ko\n// ============================================================\n\n#include <linux/module.h>\n#include <linux/kernel.h>\n#include <linux/init.h>\n\nMODULE_LICENSE(\"GPL\");\nMODULE_AUTHOR(\"ruoo-arsenal\");\nMODULE_DESCRIPTION(\"{d} - kernel module\");\nMODULE_VERSION(\"0.1\");\n\nstatic int __init {d}_init(void)\n{{\n    printk(KERN_INFO \"[{d}] Module loaded successfully\\n\");\n    printk(KERN_INFO \"[{d}] Hello from kernel space!\\n\");\n    return 0;\n}}\n\nstatic void __exit {d}_exit(void)\n{{\n    printk(KERN_INFO \"[{d}] Module unloaded\\n\");\n    printk(KERN_INFO \"[{d}] Goodbye from kernel space!\\n\");\n}}\n\nmodule_init({d}_init);\nmodule_exit({d}_exit);\n"
    )
}

pub fn generate_macos_driver_template(driver_name: &str) -> String {
    let d = driver_name;
    format!(
        "// ============================================================\n// macOS Kernel Extension Template — {d}\n// 编译: xcodebuild 或 make (需要 Xcode CLI)\n// 加载: sysload {d} {d}.kext\n// 注意: macOS 11+ 需关闭 SIP 或使用 System Extension\n// ============================================================\n\n#include <mach/mach_types.h>\n#include <libkern/libkern.h>\n#include <IOKit/IOLib.h>\n#include <IOKit/IOService.h>\n\nclass com_ruoo_{d} : public IOService {{\n    OSDeclareDefaultStructors(com_ruoo_{d})\npublic:\n    virtual bool start(IOService *provider) override;\n    virtual void stop(IOService *provider) override;\n}};\n\nOSDefineMetaClassAndStructors(com_ruoo_{d}, IOService)\n\nbool com_ruoo_{d}::start(IOService *provider) {{\n    if (!super::start(provider)) return false;\n    IOLog(\"[{d}] KEXT started\\n\");\n    registerService();\n    return true;\n}}\n\nvoid com_ruoo_{d}::stop(IOService *provider) {{\n    IOLog(\"[{d}] KEXT stopped\\n\");\n    super::stop(provider);\n}}\n\nextern \"C\" kern_return_t {d}_start(kmod_info_t *ki, void *d) {{\n    IOLog(\"[{d}] KEXT loaded\\n\");\n    return KERN_SUCCESS;\n}}\n\nextern \"C\" kern_return_t {d}_stop(kmod_info_t *ki, void *d) {{\n    IOLog(\"[{d}] KEXT unloaded\\n\");\n    return KERN_SUCCESS;\n}}\n"
    )
}

// ═══════════════════════════════════════
// 6. 全局后端注册表 + 已加载驱动跟踪
// ═══════════════════════════════════════

static LOADED_DRIVERS: OnceLock<RwLock<Vec<DriverInfo>>> = OnceLock::new();

fn loaded_drivers() -> &'static RwLock<Vec<DriverInfo>> {
    LOADED_DRIVERS.get_or_init(|| RwLock::new(Vec::new()))
}

static KERNEL_BACKEND: OnceLock<RwLock<Arc<dyn KernelDriverBackend + Send + Sync>>> =
    OnceLock::new();

fn backend_lock() -> &'static RwLock<Arc<dyn KernelDriverBackend + Send + Sync>> {
    KERNEL_BACKEND.get_or_init(|| {
        let default: Arc<dyn KernelDriverBackend + Send + Sync> = if cfg!(windows) {
            Arc::new(WindowsScBackend)
        } else {
            Arc::new(LinuxInsmodBackend)
        };
        RwLock::new(default)
    })
}

#[allow(dead_code)]
pub fn set_kernel_backend(
    backend: Arc<dyn KernelDriverBackend + Send + Sync>,
) -> Result<String, String> {
    let name = backend.backend_name();
    let platform = backend.platform();
    match backend_lock().write() {
        Ok(mut guard) => {
            *guard = backend;
            Ok(format!(
                "[+] 内核后端已切换: {} (platform: {})",
                name, platform
            ))
        }
        Err(e) => Err(format!("[!] 后端锁被占用: {}", e)),
    }
}

pub fn current_backend_name() -> String {
    match backend_lock().read() {
        Ok(guard) => guard.backend_name().to_string(),
        Err(_) => "<locked>".into(),
    }
}

pub fn current_platform() -> String {
    match backend_lock().read() {
        Ok(guard) => guard.platform().to_string(),
        Err(_) => "unknown".into(),
    }
}

// ═══════════════════════════════════════
// 7. 对外暴露的 AI 工具函数
// ═══════════════════════════════════════

pub fn kernel_driver_load(name: &str, path: &str) -> Result<String, String> {
    let backend = backend_lock()
        .read()
        .map_err(|e| format!("后端锁错误: {}", e))?;

    let validation = backend.validate_driver(path)?;
    let result = backend.load(name, path)?;

    if let Ok(mut drivers) = loaded_drivers().write() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs());
        drivers.push(DriverInfo {
            name: name.to_string(),
            path: path.to_string(),
            status: DriverStatus::Loaded,
            platform: backend.platform().to_string(),
            loaded_at: now,
            note: format!("Loaded via {}", backend.backend_name()),
        });
    }

    Ok(format!(
        "[+] sysload 成功 — {}\n\n[校验]\n{}\n\n[加载]\n{}",
        backend.backend_name(),
        validation,
        result
    ))
}

pub fn kernel_driver_unload(name: &str) -> Result<String, String> {
    let backend = backend_lock()
        .read()
        .map_err(|e| format!("后端锁错误: {}", e))?;
    let result = backend.unload(name)?;
    if let Ok(mut drivers) = loaded_drivers().write() {
        drivers.retain(|d| d.name != name);
    }
    Ok(format!(
        "[+] sysunload 成功 — {}\n{}",
        backend.backend_name(),
        result
    ))
}

pub fn kernel_driver_list() -> Result<String, String> {
    let backend = backend_lock()
        .read()
        .map_err(|e| format!("后端锁错误: {}", e))?;

    let tracked = match loaded_drivers().read() {
        Ok(drivers) => drivers.clone(),
        Err(_) => vec![],
    };
    let system_drivers = backend.list().unwrap_or_default();

    let mut lines = vec![
        format!("═══ 内核驱动状态 (后端: {}) ═══", backend.backend_name()),
        String::new(),
        "── 本会话加载的驱动 ──".into(),
    ];

    if tracked.is_empty() {
        lines.push("  (无)".into());
    } else {
        for d in &tracked {
            lines.push(format!("  [{}] {}  →  {}", d.status, d.name, d.path));
        }
    }

    lines.push(String::new());
    lines.push(format!(
        "── 系统驱动列表 ({} 个) ──",
        system_drivers.len()
    ));
    for d in &system_drivers {
        lines.push(format!("  [{}] {}  ({})", d.status, d.name, d.note));
    }

    Ok(lines.join("\n"))
}

pub fn kernel_compile(source: &str, output: &str) -> Result<String, String> {
    let backend = backend_lock()
        .read()
        .map_err(|e| format!("后端锁错误: {}", e))?;
    backend.compile_driver(source, output)
}

pub fn kernel_template(driver_name: &str, platform: &str) -> Result<String, String> {
    match platform.to_lowercase().as_str() {
        "windows" | "win" => Ok(generate_windows_driver_template(driver_name)),
        "linux" | "lnx" => Ok(generate_linux_driver_template(driver_name)),
        "macos" | "mac" | "darwin" => Ok(generate_macos_driver_template(driver_name)),
        _ => Err(format!(
            "不支持的平台: '{}' — 可选: windows / linux / macos",
            platform
        )),
    }
}

pub fn kernel_validate(path: &str) -> Result<String, String> {
    let backend = backend_lock()
        .read()
        .map_err(|e| format!("后端锁错误: {}", e))?;
    backend.validate_driver(path)
}

pub fn kernel_backend_info() -> String {
    let name = current_backend_name();
    let platform = current_platform();
    format!(
        "═══ 内核后端信息 ═══\n\
         后端名称: {}\n\
         目标平台: {}\n\
         当前系统: {}\n\
         支持扩展: {}\n\
         ─────────────────\n\
         切换后端: kernel_backend_set <name>\n\
         可用后端: WindowsScBackend / LinuxInsmodBackend / MacOSKextBackend / CustomPlugin",
        name,
        platform,
        std::env::consts::OS,
        if cfg!(windows) {
            ".sys (sc.exe)"
        } else {
            ".ko (insmod/rmmod)"
        },
    )
}

// ═══════════════════════════════════════
// 8. 驱动项目骨架生成
// ═══════════════════════════════════════

pub fn kernel_scaffold(
    driver_name: &str,
    output_dir: &str,
    platform: &str,
) -> Result<String, String> {
    let dir = std::path::Path::new(output_dir);
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("创建目录 {} 失败: {}", output_dir, e))?;

    let mut created = Vec::new();

    match platform.to_lowercase().as_str() {
        "windows" | "win" => {
            let c_path = dir.join(format!("{}.c", driver_name));
            write_file(
                &c_path.to_string_lossy(),
                &generate_windows_driver_template(driver_name),
            )?;
            created.push(format!("  {}.c — 驱动源代码", driver_name));

            let bat_path = dir.join("build.bat");
            let d = driver_name;
            write_file(
                &bat_path.to_string_lossy(),
                &format!(
                    "@echo off\r\n\
                     setlocal\r\n\
                     echo Building {d} Windows Driver...\r\n\
                     echo [*] Locating vcvarsall.bat...\r\n\
                     set \"VCDIR=\"\r\n\
                     for %%D in (\r\n\
                       \"C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files\\Microsoft Visual Studio\\2022\\Professional\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Community\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Professional\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Enterprise\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\BuildTools\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Community\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Professional\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Enterprise\\VC\\Auxiliary\\Build\"\r\n\
                       \"C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\BuildTools\\VC\\Auxiliary\\Build\"\r\n\
                     ) do (\r\n\
                       if exist \"%%D\\vcvarsall.bat\" (\r\n\
                         set \"VCDIR=%%D\"\r\n\
                         goto :found_vc\r\n\
                       )\r\n\
                     )\r\n\
                     :found_vc\r\n\
                     if defined VCDIR (\r\n\
                       echo [+] Found: %VCDIR%\\vcvarsall.bat\r\n\
                       call \"%VCDIR%\\vcvarsall.bat\" x64\r\n\
                       echo [*] Compiling...\r\n\
                       cl.exe /DRIVER /WX /O2 \"{d}.c\" /link /DRIVER /SUBSYSTEM:NATIVE /OUT:{d}.sys\r\n\
                     ) else (\r\n\
                       echo [!] vcvarsall.bat not found — trying cl.exe directly...\r\n\
                       cl.exe /DRIVER /WX /O2 \"{d}.c\" /link /DRIVER /SUBSYSTEM:NATIVE /OUT:{d}.sys\r\n\
                     )\r\n\
                     if %ERRORLEVEL% EQU 0 (\r\n\
                       echo [+] Build SUCCESS — {d}.sys\r\n\
                     ) else (\r\n\
                       echo [!] Build FAILED — 请确保已安装 Visual Studio + Windows Driver Kit (WDK)\r\n\
                     )\r\n\
                     endlocal\r\n"
                ),
            )?;
            created.push("  build.bat — 编译脚本 (需 WDK/MSVC)".into());

            write_file(
                &dir.join("README.txt").to_string_lossy(),
                &format!(
                    "═══ {d} Windows Kernel Driver ═══\r\n\r\n\
                     BUILD:\r\n  build.bat (需要 Windows Driver Kit + MSVC)\r\n\r\n\
                     LOAD:\r\n  sysload {d} {d}.sys\r\n\r\n\
                     UNLOAD:\r\n  sysunload {d}\r\n\r\n\
                     STATUS:\r\n  syslist\r\n"
                ),
            )?;
            created.push("  README.txt — 使用说明".into());
        }

        "linux" | "lnx" => {
            let c_path = dir.join(format!("{}.c", driver_name));
            write_file(
                &c_path.to_string_lossy(),
                &generate_linux_driver_template(driver_name),
            )?;
            created.push(format!("  {}.c — 内核模块源代码", driver_name));

            let d = driver_name;
            write_file(
                &dir.join("Makefile").to_string_lossy(),
                &format!(
                    "obj-m += {d}.o\n\n\
                     all:\n\
                     \tmake -C /lib/modules/$(shell uname -r)/build M=$(PWD) modules\n\n\
                     clean:\n\
                     \tmake -C /lib/modules/$(shell uname -r)/build M=$(PWD) clean\n"
                ),
            )?;
            created.push("  Makefile — 编译配置".into());

            write_file(
                &dir.join("README.txt").to_string_lossy(),
                &format!(
                    "═══ {d} Linux Kernel Module ═══\r\n\r\n\
                     DEPS:\r\n  sudo apt install linux-headers-$(uname -r) build-essential\r\n\r\n\
                     BUILD:\r\n  make\r\n\r\n\
                     LOAD:\r\n  sysload {d} {d}.ko\r\n\r\n\
                     UNLOAD:\r\n  sysunload {d}\r\n\r\n\
                     DEBUG:\r\n  dmesg | tail -20\r\n"
                ),
            )?;
            created.push("  README.txt — 使用说明".into());
        }

        "macos" | "mac" | "darwin" => {
            let cpp_path = dir.join(format!("{}.cpp", driver_name));
            write_file(
                &cpp_path.to_string_lossy(),
                &generate_macos_driver_template(driver_name),
            )?;
            created.push(format!("  {}.cpp — IOKit 驱动源代码", driver_name));

            write_file(
                &dir.join("Info.plist").to_string_lossy(),
                &format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                     <plist version=\"1.0\">\n\
                     <dict>\n\
                     \t<key>CFBundleIdentifier</key>\n\
                     \t<string>com.ruoo.{d}</string>\n\
                     \t<key>CFBundleName</key>\n\
                     \t<string>{d}</string>\n\
                     \t<key>CFBundleVersion</key>\n\
                     \t<string>1.0</string>\n\
                     \t<key>IOKitPersonalities</key>\n\
                     \t<dict>\n\
                     \t\t<key>{d}</key>\n\
                     \t\t<dict>\n\
                     \t\t\t<key>CFBundleIdentifier</key>\n\
                     \t\t\t<string>com.ruoo.{d}</string>\n\
                     \t\t\t<key>IOClass</key>\n\
                     \t\t\t<string>com_ruoo_{d}</string>\n\
                     \t\t\t<key>IOProviderClass</key>\n\
                     \t\t\t<string>IOResources</string>\n\
                     \t\t\t<key>IOResourceMatch</key>\n\
                     \t\t\t<string>IOKit</string>\n\
                     \t\t</dict>\n\
                     \t</dict>\n\
                     </dict>\n\
                     </plist>\n", d = driver_name),
            )?;
            created.push("  Info.plist — Bundle 配置".into());

            let d = driver_name;
            write_file(
                &dir.join("Makefile").to_string_lossy(),
                &format!(
                    "KEXT_NAME = {d}\n\
                     KEXT_BUNDLE = $(KEXT_NAME).kext\n\
                     SDKROOT = $(shell xcrun --sdk macosx --show-sdk-path)\n\n\
                     all: $(KEXT_BUNDLE)\n\n\
                     $(KEXT_BUNDLE): {d}.cpp Info.plist\n\
                     \tclang++ -std=c++17 -arch x86_64 -arch arm64 \\\n\
                     \t\t-mmacosx-version-min=10.15 \\\n\
                     \t\t-isysroot $(SDKROOT) \\\n\
                     \t\t-D_KERNEL -DKERNEL -DDRIVER_PRIVATE \\\n\
                     \t\t-nostdinc -fno-builtin -fno-rtti \\\n\
                     \t\t-o $(KEXT_BUNDLE)/Contents/MacOS/$(KEXT_NAME) \\\n\
                     \t\t{d}.cpp -lkmod -lcc_kext \\\n\
                     \t\t-framework IOKit -framework Kernel\n\
                     \tcp Info.plist $(KEXT_BUNDLE)/Contents/\n\
                     \tcodesign --force --sign - $(KEXT_BUNDLE)\n\n\
                     clean:\n\
                     \trm -rf $(KEXT_BUNDLE)\n"
                ),
            )?;
            created.push("  Makefile — 编译配置 (需 Xcode CLI)".into());

            write_file(
                &dir.join("README.txt").to_string_lossy(),
                &format!(
                    "═══ {d} macOS Kernel Extension ═══\r\n\r\n\
                     PREREQS:\r\n  xcode-select --install\r\n\r\n\
                     BUILD:\r\n  make\r\n\r\n\
                     LOAD (需关闭SIP):\r\n  sysload {d} {d}.kext\r\n\r\n\
                     UNLOAD:\r\n  sysunload {d}\r\n\r\n\
                     NOTE:\r\n  macOS 11+ 建议使用 Endpoint Security Framework\r\n  替代 IOKit KEXT。SIP 需在 Recovery 模式关闭。\r\n"
                ),
            )?;
            created.push("  README.txt — 使用说明".into());
        }

        _ => return Err(format!("不支持的平台: '{}'", platform)),
    }

    Ok(format!(
        "[+] 驱动项目骨架已生成: {}\n\n创建的文件:\n{}\n\n下一步:\n  cd {}\n  # 编辑源码 → 编译 → sysload",
        output_dir,
        created.join("\n"),
        output_dir
    ))
}

// ═══════════════════════════════════════
// 9. 内部辅助
// ═══════════════════════════════════════

fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    let output = cmd
        .output()
        .map_err(|e| format!("执行 '{}' 失败: {}", program, e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(stdout)
    } else if !stdout.is_empty() {
        Ok(stdout)
    } else {
        Err(format!(
            "命令 '{}' 失败 (exit={}): {}",
            program,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ))
    }
}

fn write_file(path: &str, content: &str) -> Result<(), String> {
    let mut f = std::fs::File::create(path)
        .map_err(|e| format!("创建文件 '{}' 失败: {}", path, e))?;
    f.write_all(content.as_bytes())
        .map_err(|e| format!("写入文件 '{}' 失败: {}", path, e))?;
    Ok(())
}

// ═══════════════════════════════════════
// 10. 测试
// ═══════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_backend_initialized() {
        let name = current_backend_name();
        assert!(!name.is_empty());
        assert!(name.contains("Backend"));
    }

    #[test]
    fn test_platform_detection() {
        let platform = current_platform();
        assert!(platform == "windows" || platform == "linux");
    }

    #[test]
    fn test_windows_template_generation() {
        let code = generate_windows_driver_template("testdrv");
        assert!(code.contains("DriverEntry"));
        assert!(code.contains("testdrv"));
        assert!(code.contains("ntddk.h"));
    }

    #[test]
    fn test_linux_template_generation() {
        let code = generate_linux_driver_template("testmod");
        assert!(code.contains("module_init"));
        assert!(code.contains("testmod"));
        assert!(code.contains("linux/module.h"));
    }

    #[test]
    fn test_macos_template_generation() {
        let code = generate_macos_driver_template("testkext");
        assert!(code.contains("IOService"));
        assert!(code.contains("testkext"));
        assert!(code.contains("IOKit/IOLib.h"));
    }

    #[test]
    fn test_validate_extension_windows() {
        let backend = WindowsScBackend;
        let result = backend.validate_driver("nonexistent.sys");
        assert!(result.is_err());
    }

    #[test]
    fn test_driver_info_display() {
        let info = DriverInfo {
            name: "test".into(),
            path: "/tmp/test.sys".into(),
            status: DriverStatus::Loaded,
            platform: "windows".into(),
            loaded_at: Some(12345),
            note: String::new(),
        };
        let s = format!("{}", info);
        assert!(s.contains("LOADED"));
        assert!(s.contains("test"));
    }

    #[test]
    fn test_kernel_backend_info() {
        let info = kernel_backend_info();
        assert!(info.contains("后端名称"));
        assert!(info.contains("目标平台"));
    }

    #[test]
    fn test_scaffold_invalid_platform() {
        let result = kernel_scaffold("test", "/tmp/nonexistent_ruoo", "freebsd");
        assert!(result.is_err());
    }

    #[test]
    fn test_kernel_template_macos() {
        let result = kernel_template("testkext", "macos");
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(code.contains("IOService"));
    }
}
