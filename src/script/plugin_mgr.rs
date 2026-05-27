use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use super::permissions::{PermissionSet, PluginPermissions};

// ─── 插件信息 v6.2: 集成熔断器 ───────────────────────
pub(crate) struct PluginInfo {
    /// 共享库 (Arc 支持后台线程执行)
    pub(crate) lib: Arc<libloading::Library>,
    /// 原始文件路径 (用于热加载检测)
    pub(crate) path: PathBuf,
    /// 插件声明的权限
    pub(crate) permissions: PluginPermissions,
    /// 最后修改时间 (用于 watch-plugins)
    pub(crate) last_modified: std::time::SystemTime,
    /// 插件版本 (来自 ruoo_plugin_version)
    pub(crate) version: String,
    /// 累计崩溃次数 (防崩溃框架跟踪)
    pub(crate) crash_count: u32,
    /// 是否已被标记为故障 (连续崩溃>=3)
    pub(crate) faulted: bool,
    /// v6.2: 熔断器 — 防止反复崩溃挂起终端
    pub(crate) circuit_breaker: crate::plugin::CircuitBreaker,
    /// v4.2.1: 超时后被抛弃的线程句柄 — 卸载前必须join, 防止DLL卸载时FFI调用segfault
    pub(crate) abandoned_threads: Vec<std::thread::JoinHandle<()>>,
}

// ─── 插件管理器 v2 — 权限 + 热加载 + 后台线程 ─────────
pub struct PluginManager {
    pub(crate) loaded: HashMap<String, PluginInfo>,
    /// 是否启用热加载监控
    /// 插件注册表 (持久化加密数据库)
    pub(crate) plugin_registry: Option<Arc<crate::plugin_registry::PluginRegistry>>,
    pub(crate) watch: bool,
}

/// 插件调用结果
#[allow(dead_code)]
pub enum PluginCallResult {
    /// 成功返回
    Ok(String),
    /// 超时 (毫秒)
    Timeout(u64),
    /// 错误
    Err(String),
}

impl PluginManager {
    pub fn new() -> Self {
        Self { loaded: HashMap::new(), watch: false, plugin_registry: None }
    }

    pub fn set_watch(&mut self, enabled: bool) {
        self.watch = enabled;
    }

    // ── 读取插件元数据 (不执行 init) ──
    fn probe_plugin(lib: &libloading::Library, _path: &PathBuf) -> Result<(PluginPermissions, String), String> {
        unsafe {
            // 读取权限声明 (可选)
            let perms = match lib.get::<unsafe extern "C" fn() -> u32>(b"ruoo_plugin_permissions") {
                Ok(func) => PluginPermissions::from_bits(func()),
                Err(_) => PluginPermissions::from_bits(0), // 默认无权限
            };

            // 读取版本 (可选)
            let version = match lib.get::<unsafe extern "C" fn() -> *const std::ffi::c_char>(b"ruoo_plugin_version") {
                Ok(func) => {
                    let ptr = func();
                    if ptr.is_null() {
                        "0.0.0".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                    }
                }
                Err(_) => "0.0.0".to_string(),
            };

            Ok((perms, version))
        }
    }

    /// 加载插件
    /// - 读取权限声明
    /// - 调用 ruoo_plugin_init()
    /// - 记录文件 mtime (用于热加载)
    pub fn load(&mut self, name: &str, path: &str) -> Result<String, String> {
        if self.loaded.contains_key(name) {
            return Err(format!("插件已加载: {}", name));
        }

        let file_path = PathBuf::from(path);
        if !file_path.exists() {
            return Err(format!("插件文件不存在: {}", path));
        }

        // 规范化路径
        let canonical = file_path.canonicalize()
            .map_err(|e| format!("无法解析路径: {} ({})", path, e))?;

        // 获取修改时间
        let mtime = canonical.metadata()
            .map_err(|e| format!("无法读取文件元数据: {}", e))?
            .modified()
            .map_err(|e| format!("无法获取修改时间: {}", e))?;

        // 安全加载库
        let lib = unsafe {
            libloading::Library::new(&canonical)
                .map_err(|e| format!("加载插件失败: {} ({})", canonical.display(), e))?
        };

        // 读取元数据
        let (perms, version) = Self::probe_plugin(&lib, &canonical)?;

        // 调用初始化
        unsafe {
            let init: libloading::Symbol<unsafe extern "C" fn() -> i32> = lib
                .get(b"ruoo_plugin_init")
                .map_err(|_| format!(
                    "插件缺少 ruoo_plugin_init 导出: {}\n  \
                     插件接口规范:\n  \
                     - ruoo_plugin_init() -> i32\n  \
                     - ruoo_plugin_exec(cmd) -> *mut c_char\n  \
                     - ruoo_plugin_free_result(ptr)\n  \
                     - ruoo_plugin_permissions() -> u32 (可选)\n  \
                     - ruoo_plugin_shutdown() (可选)",
                    canonical.display()
                ))?;
            let code = init();
            if code != 0 {
                return Err(format!("插件初始化返回错误码: {} (版本: {})", code, version));
            }
        }

        self.loaded.insert(name.to_string(), PluginInfo {
            lib: Arc::new(lib),
            path: canonical,
            permissions: perms.clone(),
            last_modified: mtime,
            version,
            crash_count: 0,
            faulted: false,
            circuit_breaker: crate::plugin::CircuitBreaker::new(10),
            abandoned_threads: Vec::new(),
        });

        Ok(format!(
            "[+] 插件已加载: {} v{} | 权限: {} | 路径: {}",
            name,
            self.loaded[name].version,
            perms.describe(),
            path
        ))
    }

    /// 获取已加载插件信息 (供 AI 全局管理器共享)
    pub(crate) fn get_info(&self, name: &str) -> Option<&PluginInfo> {
        self.loaded.get(name)
    }

    /// 插入已加载插件 (Arc 共享，供 AI 全局管理器复用)
    pub(crate) fn insert_info(&mut self, name: &str, info: PluginInfo) {
        self.loaded.insert(name.to_string(), info);
    }

    /// 热加载: 检测文件是否被修改，自动重载
    fn check_hotload(&mut self, name: &str) -> Result<Option<String>, String> {
        if !self.watch {
            return Ok(None);
        }

        let info = match self.loaded.get(name) {
            Some(i) => i,
            None => return Ok(None),
        };

        // 检查文件修改时间
        let current_mtime = match info.path.metadata()
            .and_then(|m| m.modified())
        {
            Ok(t) => t,
            Err(_) => return Ok(None), // 文件可能被删除
        };

        if current_mtime <= info.last_modified {
            return Ok(None); // 未修改
        }

        // 文件已修改 → 热重载
        let path = info.path.clone();
        // info 引用在此自然释放 (NLL)

        match self.hotload_inner(name, &path) {
            Ok(msg) => Ok(Some(msg)),
            Err(e) => {
                // 热加载失败 → 卸载旧插件
                self.loaded.remove(name);
                Err(format!("热加载失败，插件已卸载: {}", e))
            }
        }
    }

    /// 内部热加载实现
    fn hotload_inner(&mut self, name: &str, path: &PathBuf) -> Result<String, String> {
        // v6.4: 备份旧版本用于回滚
        let old_info = self.loaded.remove(name);
        let old_crash_count = old_info.as_ref().map(|o| o.crash_count).unwrap_or(0);

        // 1. 卸载旧版 (调用 shutdown + join abandoned threads)
        if let Some(mut old) = old_info {
            let old_path = old.path.display().to_string();

            // v4.2.1: join abandoned threads before dropping library
            for handle in old.abandoned_threads.drain(..) {
                let _ = handle.join();
            }

            unsafe {
                if let Ok(shutdown) = old.lib.get::<unsafe extern "C" fn()>(b"ruoo_plugin_shutdown") {
                    shutdown();
                }
            }
            drop(old); // 显式卸载

            // v6.6: FreeLibrary 循环确保旧 DLL 完全释放
            #[cfg(windows)]
            {
                extern "system" {
                    fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                    fn FreeLibrary(hLibModule: isize) -> i32;
                }
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = std::ffi::OsStr::new(&old_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                for _ in 0..16 {
                    unsafe {
                        let h = GetModuleHandleW(wide.as_ptr());
                        if h == 0 { break; }
                        FreeLibrary(h);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        }

        // 2. 加载新版
        let mtime = path.metadata()
            .map_err(|e| format!("无法读取元数据: {}", e))?
            .modified()
            .map_err(|e| format!("无法获取修改时间: {}", e))?;

        let lib = match unsafe { libloading::Library::new(path) } {
            Ok(l) => l,
            Err(e) => {
                // v6.4: 加载失败 — 旧版本已卸载, 无法自动回滚
                return Err(format!(
                    "热加载失败: 无法加载新版DLL '{}' ({}) — 旧版本已卸载, 请手动 plugin load 恢复",
                    path.display(), e
                ));
            }
        };

        let probe_result = Self::probe_plugin(&lib, path);
        let (perms, version) = match probe_result {
            Ok(v) => v,
            Err(e) => {
                Self::force_drop_library(lib, path);
                return Err(format!("热加载失败: 探测插件失败 ({}) — 旧版本已卸载, 请手动恢复", e));
            }
        };

        let init_result = unsafe {
            lib.get::<unsafe extern "C" fn() -> i32>(b"ruoo_plugin_init")
        };
        let init = match init_result {
            Ok(func) => func,
            Err(_) => {
                Self::force_drop_library(lib, path);
                return Err("热加载: 缺少 ruoo_plugin_init — 旧版本已卸载".to_string());
            }
        };
        unsafe {
            let code = init();
            if code != 0 {
                Self::force_drop_library(lib, path);
                return Err(format!("热加载: init 返回 {} — 旧版本已卸载", code));
            }
        }

        self.loaded.insert(name.to_string(), PluginInfo {
            lib: Arc::new(lib),
            path: path.clone(),
            permissions: perms.clone(),
            last_modified: mtime,
            version,
            crash_count: old_crash_count, // v6.4: 保留旧崩溃计数
            faulted: false,
            circuit_breaker: crate::plugin::CircuitBreaker::new(10),
            abandoned_threads: Vec::new(),
        });

        Ok(format!(
            "[↻] 热加载完成: {} v{} | 权限: {}",
            name,
            self.loaded[name].version,
            perms.describe()
        ))
    }

    /// 手动热加载指定插件
    pub fn hotload(&mut self, name: &str) -> Result<String, String> {
        let path = match self.loaded.get(name) {
            Some(info) => info.path.clone(),
            None => return Err(format!("插件未加载: {}", name)),
        };
        self.hotload_inner(name, &path)
    }

    /// v6.2: 调用插件 — 后台线程执行 + 超时保护 + 熔断器
    ///
    /// 流程:
    ///   1. 权限预检查 (脚本权限 vs 插件声明权限)
    ///   2. 热加载检测 (watch-plugins 模式)
    ///   3. 熔断器检查 (防止反复崩溃)
    ///   4. spawn 独立线程执行 ruoo_plugin_exec
    ///   5. 主线程 join 等待 (带超时)
    ///   6. 调用 ruoo_plugin_free_result 释放内存
    pub fn call(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
        timeout_ms: u64,
    ) -> Result<String, String> {
        // 1. 权限预检查
        script_perms.check_plugin()?;

        // 2. 热加载检测
        let hot_msg = self.check_hotload(name)?;

        // 3. v6.2: 一次性检查 faulted + 熔断器 + 权限 + 提取参数
        {
            let info = self.loaded.get_mut(name)
                .ok_or_else(|| format!("插件未加载: {}", name))?;
            script_perms.covers(&info.permissions)?;
            if info.faulted {
                return Err(format!(
                    "[!] 插件 '{}' 已被标记为故障 (累计{}次崩溃) — 请使用 crash_recover 恢复",
                    name, info.crash_count
                ));
            }
            if !info.circuit_breaker.allow_call() {
                return Err(format!(
                    "插件 '{}' 已熔断: {}", name, info.circuit_breaker.status_report()
                ));
            }
        }

        // 4. 获取 info (热加载可能已替换, 使用不可变引用)
        let info = self.loaded.get(name)
            .ok_or_else(|| format!("插件热加载后丢失: {}", name))?;

        let (result, abandoned_handle) = self.call_inner(info, command, timeout_ms, hot_msg);

        // v4.2.1: 存储超时后被抛弃的线程句柄 — 卸载前join防止segfault
        if let Some(handle) = abandoned_handle {
            if let Some(info) = self.loaded.get_mut(name) {
                info.abandoned_threads.push(handle);
            }
        }

        // 5. 更新熔断器
        if let Some(info) = self.loaded.get_mut(name) {
            if result.is_err() {
                let _ = info.circuit_breaker.record_failure(&format!("{:?}", result.as_ref().err()), command);
            } else {
                info.circuit_breaker.record_success();
            }
        }

        result
    }

    /// 实际调用逻辑 (v4.2.1: 返回abandoned_handle供调用者存储, 防止DLL卸载时segfault)
    fn call_inner(
        &self,
        info: &PluginInfo,
        command: &str,
        timeout_ms: u64,
        prefix_msg: Option<String>,
    ) -> (Result<String, String>, Option<std::thread::JoinHandle<()>>) {
        let lib = info.lib.clone();
        let cmd_owned = command.to_string();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        // spawn 后台线程
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

        let handle = match std::thread::Builder::new()
            .name(format!("ruoo-plugin-{}", command.chars().take(16).collect::<String>()))
            .spawn(move || {
                // catch_unwind 防止插件 panic 导致进程崩溃
                // v4.2: stderr 捕获 — 重定向插件stderr到管道, 避免破坏TUI布局
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let stderr_cap = super::plugin_stderr::StderrCapture::begin();
                    let exec_result = Self::call_plugin_unsafe(&lib, &cmd_owned);
                    let captured = stderr_cap.map(|c| c.end()).unwrap_or_default();
                    match exec_result {
                        Ok(output) => {
                            if captured.is_empty() {
                                Ok(output)
                            } else {
                                Ok(format!("{}\n[plugin stderr]\n{}", output, captured))
                            }
                        }
                        Err(e) => {
                            if captured.is_empty() {
                                Err(e)
                            } else {
                                Err(format!("{}\n[plugin stderr]\n{}", e, captured))
                            }
                        }
                    }
                }));
                let final_result = match result {
                    Ok(r) => r,
                    Err(panic_info) => {
                        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                            format!("插件 panic: {}", s)
                        } else if let Some(s) = panic_info.downcast_ref::<String>() {
                            format!("插件 panic: {}", s)
                        } else {
                            "插件 panic: 未知原因 (可能内存损坏/空指针)".into()
                        };
                        Err(msg)
                    }
                };
                let _ = tx.send(final_result);
        }) {
            Ok(h) => h,
            Err(e) => return (Err(format!("无法创建插件线程: {}", e)), None),
        };

        // 等待结果 (超时保护)
        match rx.recv_timeout(timeout) {
            Ok(result) => {
                // 线程正常完成, join回收
                let _ = handle.join();
                let r = match result {
                    Ok(output) => {
                        let mut full = String::new();
                        if let Some(msg) = prefix_msg {
                            full.push_str(&msg);
                            full.push('\n');
                        }
                        full.push_str(&output);
                        Ok(full)
                    }
                    Err(e) => Err(e),
                };
                (r, None)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // v4.2.1: 超时 — 返回abandoned句柄, 调用者存储后由unload时join
                (Err(format!(
                    "插件调用超时 ({}ms) — 线程仍在运行, 已登记, 卸载前将等待完成",
                    timeout_ms
                )), Some(handle))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = handle.join();
                (Err("插件线程异常终止 (panic/崩溃)".into()), None)
            }
        }
    }

    /// 不安全调用 — 在独立线程中执行
    fn call_plugin_unsafe(lib: &libloading::Library, command: &str) -> Result<String, String> {
        let cmd = std::ffi::CString::new(command)
            .map_err(|e| format!("命令含空字符: {}", e))?;

        unsafe {
            // 获取 exec 符号
            let exec: libloading::Symbol<
                unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char,
            > = lib
                .get(b"ruoo_plugin_exec")
                .map_err(|_| "插件缺少 ruoo_plugin_exec 导出".to_string())?;

            // 调用
            let result_ptr = exec(cmd.as_ptr());

            if result_ptr.is_null() {
                return Err("插件返回空指针 (可能是内部错误)".into());
            }

            // 复制结果
            let result = std::ffi::CStr::from_ptr(result_ptr)
                .to_string_lossy()
                .into_owned();

            // ★ Bug 修复: 调用 ruoo_plugin_free_result 释放插件分配的内存
            if let Ok(free_func) = lib.get::<unsafe extern "C" fn(*mut std::ffi::c_char)>(
                b"ruoo_plugin_free_result",
            ) {
                free_func(result_ptr);
            }
            // 如果插件没有导出 free_result，内存泄漏 (但不会崩溃)

            Ok(result)
        }
        // cmd (CString) 在此释放 → as_ptr() 指针已过期但已用完
    }

    /// 无超时调用 (兼容旧接口)
    #[allow(dead_code)]
    pub fn call_default(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
    ) -> Result<String, String> {
        self.call(name, command, script_perms, 30_000) // 默认30秒超时
    }

    /// 卸载插件 v6.6: 增加 FreeLibrary 循环彻底释放 DLL 文件句柄
    pub fn unload(&mut self, name: &str) -> Result<(), String> {
        if let Some(mut info) = self.loaded.remove(name) {
            let path = info.path.display().to_string();

            // v4.2.1: 首先join所有超时后被抛弃的线程
            for handle in info.abandoned_threads.drain(..) {
                let _ = handle.join();
            }

            // 调用清理函数
            unsafe {
                if let Ok(shutdown) = info.lib.get::<unsafe extern "C" fn()>(b"ruoo_plugin_shutdown") {
                    shutdown();
                }
            }
            drop(info);

            // v6.6: Windows FreeLibrary 循环
            #[cfg(windows)]
            {
                extern "system" {
                    fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                    fn FreeLibrary(hLibModule: isize) -> i32;
                }
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = std::ffi::OsStr::new(&path)
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
            Ok(())
        } else {
            Err(format!("插件未加载: {}", name))
        }
    }

    /// v6.6: 强制释放库句柄 — 带 FreeLibrary 循环
    fn force_drop_library(lib: libloading::Library, path: &std::path::Path) {
        drop(lib);
        #[cfg(windows)]
        {
            extern "system" {
                fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                fn FreeLibrary(hLibModule: isize) -> i32;
            }
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = std::ffi::OsStr::new(path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            for _ in 0..16 {
                unsafe {
                    let h = GetModuleHandleW(wide.as_ptr());
                    if h == 0 { break; }
                    FreeLibrary(h);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
    }

    /// 列出已加载插件及详情
    pub fn list(&self) -> Vec<String> {
        self.loaded.keys().cloned().collect()
    }

    /// 列出插件详情
    pub fn list_detailed(&self) -> Vec<String> {
        self.loaded.iter().map(|(name, info)| {
            format!(
                "  {} v{} | 权限: {} | 路径: {} | {}",
                name,
                info.version,
                info.permissions.describe(),
                info.path.display(),
                if self.watch { "👁 watch" } else { "" }
            )
        }).collect()
    }

    /// 判断插件是否已加载
    #[allow(dead_code)]
    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains_key(name)
    }

    /// 获取插件数量
    pub fn count(&self) -> usize {
        self.loaded.len()
    }

    /// ★ v4.3 fix: 将另一个 PluginManager 的插件迁移到当前管理器
    /// 用于 bootscript keep_plugins=true 时将脚本插件迁移到终端全局
    pub fn merge_from(&mut self, other: &mut PluginManager) -> usize {
        let mut count = 0;
        let names: Vec<String> = other.loaded.keys().cloned().collect();
        for name in names {
            if !self.loaded.contains_key(&name) {
                if let Some(info) = other.loaded.remove(&name) {
                    self.loaded.insert(name, info);
                    count += 1;
                }
            } else {
                // 已存在 → 仅从源移除 (避免 drop 时重复 unload)
                other.loaded.remove(&name);
            }
        }
        count
    }

    /// ★ v4.3新增: 探测已加载插件的 ruoo_plugin_commands 导出
    /// 返回 JSON 命令列表字符串 (空数组表示无命令声明)
    /// 使用已加载的 Library 句柄，避免重复打开 DLL
    pub fn probe_plugin_commands(&self, name: &str) -> Result<String, String> {
        let info = self.loaded.get(name)
            .ok_or_else(|| format!("插件未加载: {}", name))?;
        crate::plugin::probe_plugin_commands(&info.lib)
    }


    /// 设置插件注册表引用
    pub fn set_plugin_registry(&mut self, reg: Arc<crate::plugin_registry::PluginRegistry>) {
        self.plugin_registry = Some(reg);
    }


    // ═══════════════════════════════════════════════════════
    // 防崩溃框架 v5.0 — crash guard / force unload / crash recover
    // ═══════════════════════════════════════════════════════

    /// 强制卸载插件 (跳过 shutdown, 直接 drop 库句柄)
    /// 用于插件崩溃/挂起后强制清理, 防止终端崩溃
    /// v6.6: 增强清理 — 等待孤立线程 + 激进FreeLibrary循环 彻底释放DLL文件句柄
    pub fn force_unload(&mut self, name: &str) -> Result<String, String> {
        let mut info = match self.loaded.remove(name) {
            Some(i) => i,
            None => return Err(format!("插件未加载: {}", name)),
        };

        let path = info.path.display().to_string();
        let ver = info.version.clone();
        let crashes = info.crash_count;

        // v4.2.1: 首先join所有超时后被抛弃的线程
        for handle in info.abandoned_threads.drain(..) {
            let _ = handle.join();
        }

        // v6.6: 等待孤立线程释放 Arc 克隆 (call_guarded 可能使用 detached 线程)
        // Arc 被 move 进线程闭包, 线程结束后引用计数减1
        for _ in 0..10 {
            if Arc::strong_count(&info.lib) <= 1 { break; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // 尝试调用 shutdown (可能成功也可能崩溃, 我们已做好最坏准备)
        unsafe {
            if let Ok(shutdown) = info.lib.get::<unsafe extern "C" fn()>(b"ruoo_plugin_shutdown") {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shutdown()));
            }
        }

        // 强制释放 Arc → 当所有引用释放后库被卸载
        drop(info);

        // v6.6: Windows 激进 FreeLibrary 循环 — 彻底耗尽残留引用
        #[cfg(windows)]
        {
            extern "system" {
                fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                fn FreeLibrary(hLibModule: isize) -> i32;
            }
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = std::ffi::OsStr::new(&path)
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

        // 让出CPU确保OS处理 DLL_PROCESS_DETACH
        for _ in 0..3 {
            std::thread::yield_now();
        }

        Ok(format!(
            "[!] 插件已强制卸载: {} v{} (路径: {}, 累计崩溃: {})",
            name, ver, path, crashes
        ))
    }

    /// 崩溃恢复 — AI友好接口
    /// 自动检测故障插件, 生成详细报告, 并强制卸载
    /// 返回 (恢复报告, 是否成功清理)
    pub fn crash_recover(&mut self, name: &str) -> (String, bool) {
        let info = match self.loaded.get(name) {
            Some(i) => i,
            None => return (format!("[*] 插件 '{}' 未加载, 无需恢复", name), true),
        };

        let crashes = info.crash_count;
        let faulted = info.faulted;
        let path = info.path.display().to_string();
        let ver = info.version.clone();

        if !faulted && crashes == 0 {
            return (format!("[+] 插件 '{}' v{} 运行正常, 无需恢复", name, ver), true);
        }

        let mut report = String::new();
        report.push_str(&format!("═══ 插件崩溃恢复: {} ═══\n", name));
        report.push_str(&format!("  版本: v{} | 路径: {}\n", ver, path));
        report.push_str(&format!("  累计崩溃: {} | 故障标记: {}\n", crashes, faulted));
        report.push_str(&format!("  状态: {}\n", if faulted { "[!] 故障 — 连续崩溃≥3次" } else { "[*] 轻微 — 崩溃<3次" }));

        if faulted {
            report.push_str("  建议: 插件已标记为故障, 需要修复后重新加载\n");
        } else {
            report.push_str("  建议: 可尝试重新加载: plugin load <名> <路径>\n");
        }

        // 强制卸载
        match self.force_unload(name) {
            Ok(msg) => {
                report.push_str(&format!("  {}\n", msg));
                report.push_str("  [+] 崩溃恢复完成 — 插件已安全卸载\n");
                (report, true)
            }
            Err(e) => {
                report.push_str(&format!("  [!] 强制卸载失败: {}\n", e));
                (report, false)
            }
        }
    }

    /// 安全调用插件 (带防崩溃守卫)
    /// panic → 自动捕获并生成CrashReport → 自动强制卸载故障插件
    /// 超时 → 自动生成CrashReport → 线程被放弃 (库仍加载但不可用)
    pub fn call_guarded(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
        timeout_ms: u64,
    ) -> Result<String, String> {
        use super::plugin_crash::PluginCallGuard;

        // 权限预检查
        script_perms.check_plugin()?;

        // 热加载检测
        if self.watch {
            match self.check_hotload(name) {
                Ok(Some(_)) => { /* 热加载成功, 继续 */ }
                Ok(None) => {}
                Err(_) => { /* 热加载失败, 继续使用旧版 */ }
            }
        }

        // v6.2: 一次性检查 faulted + 熔断器 + 权限 + 提取调用参数
        let (ver, path, crashes, lib) = {
            let info = self.loaded.get_mut(name)
                .ok_or_else(|| format!("插件未加载: {}", name))?;

            // 权限覆盖检查
            script_perms.covers(&info.permissions)?;

            // 故障状态检查
            if info.faulted {
                return Err(format!(
                    "[!] 插件 '{}' 已被标记为故障 (累计{}次崩溃) — 请使用 crash_recover 恢复",
                    name, info.crash_count
                ));
            }

            // v6.2: 熔断器检查 (可变引用, 状态变更会写回)
            if !info.circuit_breaker.allow_call() {
                return Err(format!(
                    "插件 '{}' 已熔断: {}",
                    name, info.circuit_breaker.status_report()
                ));
            }

            (info.version.clone(), info.path.display().to_string(), info.crash_count, info.lib.clone())
        };

        let guard = PluginCallGuard::new(name, &ver, &path, command, crashes);

        let cmd_owned = command.to_string();
        let name_owned = name.to_string();

        // 在防护下执行
        let result = guard.call_with_timeout(
            move || -> Result<String, String> {
                unsafe {
                    let exec: libloading::Symbol<unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char> =
                        lib.get(b"ruoo_plugin_exec")
                            .map_err(|e| format!("插件缺少 ruoo_plugin_exec: {}", e))?;
                    let c_cmd = std::ffi::CString::new(cmd_owned.clone())
                        .map_err(|e| format!("命令含空字节: {}", e))?;
                    let ptr = exec(c_cmd.as_ptr());
                    if ptr.is_null() {
                        return Err("插件返回空指针".to_string());
                    }
                    let result = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
                    // 释放内存
                    if let Ok(free_fn) = lib.get::<unsafe extern "C" fn(*mut std::ffi::c_char)>(b"ruoo_plugin_free_result") {
                        free_fn(ptr);
                    }
                    Ok(result)
                }
            },
            timeout_ms,
        );

        match result {
            Ok(Ok(inner)) => {
                // 成功 — 记录到熔断器, 不增加 crash_count
                if let Some(info) = self.loaded.get_mut(&name_owned) {
                    info.circuit_breaker.record_success();
                }
                Ok(inner)
            }
            Ok(Err(e)) => {
                // 插件返回业务错误 — 不视为崩溃, 不触发熔断器
                if let Some(info) = self.loaded.get_mut(&name_owned) {
                    info.circuit_breaker.record_success();
                }
                Err(e)
            }
            Err(report) => {
                // 崩溃 — 增加计数, 记录熔断器, 输出详细报告, 强制卸载
                if let Some(info) = self.loaded.get_mut(&name_owned) {
                    info.crash_count += 1;
                    let triggered = info.circuit_breaker.record_failure(
                        &report.error_message, command
                    );
                    if info.crash_count >= 3 {
                        info.faulted = true;
                    }
                    if triggered {
                        // 熔断器触发: 额外标记
                    }
                }

                let formatted = report.format_report();

                // 强制卸载崩溃插件
                let _ = self.force_unload(&name_owned);

                Err(format!("{}\n[!] 插件已强制卸载, 核心程序不受影响", formatted))
            }
        }
    }

    /// v6.2: 获取插件健康状态报告 (含熔断器状态)
    pub fn plugin_health_report(&self, name: &str) -> String {
        match self.loaded.get(name) {
            Some(info) => {
                let status = if info.faulted {
                    "[!] FAULTED"
                } else if info.crash_count > 0 {
                    "[*] DEGRADED"
                } else {
                    "[+] HEALTHY"
                };
                format!(
                    "{} v{} | {} | 崩溃:{} | 熔断:{} | 路径:{}",
                    name, info.version, status,
                    info.crash_count,
                    info.circuit_breaker.status_report(),
                    info.path.display()
                )
            }
            None => format!("[-] {} — 未加载", name),
        }
    }

    /// v6.2: 重置插件熔断器
    pub fn reset_circuit(&mut self, name: &str) -> Result<String, String> {
        let info = self.loaded.get_mut(name)
            .ok_or_else(|| format!("插件未加载: {}", name))?;
        info.circuit_breaker.reset();
        info.faulted = false;
        info.crash_count = 0;
        Ok(format!("[↻] 插件 '{}' 熔断器已重置, 故障标记已清除", name))
    }

    /// 列出所有插件的健康状态
    pub fn plugin_health_summary(&self) -> String {
        if self.loaded.is_empty() {
            return "[*] 无已加载插件".to_string();
        }
        let mut r = String::from("═══ 插件健康状态 ═══\n");
        let mut names: Vec<&String> = self.loaded.keys().collect();
        names.sort();
        for name in names {
            r.push_str(&format!("  {}\n", self.plugin_health_report(name)));
        }
        r
    }
    /// 获取插件注册表引用
    pub fn get_plugin_registry(&self) -> Option<&Arc<crate::plugin_registry::PluginRegistry>> {
        self.plugin_registry.as_ref()
    }

    /// 从注册表自动加载所有启用的插件
    pub fn auto_load_from_registry(&mut self, cmd_registry: &mut crate::plugin::CommandRegistry) -> Vec<String> {
        let mut results = Vec::new();
        let entries = match self.plugin_registry.as_ref() {
            Some(reg) => reg.get_autoload_entries(),
            None => {
                results.push("[*] 插件注册表未挂载 — 跳过自动加载".to_string());
                return results;
            }
        };

        if entries.is_empty() {
            return results;
        }

        results.push(format!("═══ 自动加载插件 (注册表) ═══ 共 {} 条", entries.len()));

        for (alias, entry) in &entries {
            match self.load(alias, &entry.path) {
                Ok(msg) => {
                    results.push(format!("  [+] {} — {}", alias, msg));
                    // ★ 命令注册 (v4.6修复: 原auto_load_from_registry缺失此步骤)
                    if let Some(lib) = self.get_library(alias) {
                        match crate::plugin::try_register_from_library(lib, alias, cmd_registry) {
                            Ok(count) if count > 0 => {
                                results.push(format!("    [+] 已注册 {} 个命令", count));
                            }
                            Ok(_) => {
                                results.push("    [*] 无命令声明 (可用 plugin call 调用)".into());
                            }
                            Err(e) => {
                                results.push(format!("    [!] ═══ 命令注册失败 ═══"));
                                results.push(format!("    [!] 原因: {}", e));
                                results.push(format!("    [!] 插件已加载但无法使用 — 可能命令名与内建命令冲突"));
                            }
                        }
                    }
                    // ★ v4.7修复: 同步到 AI 全局 PluginManager (GLOBAL_PLUGIN_MGR)
                    //   AI 的 plugin_list/plugin_health 查询独立管理器
                    //   此处将 App 已加载的插件 Arc 共享过去，避免重复加载 DLL
                    if let Some(info) = self.get_info(alias) {
                        let mut ai_guard = crate::ai::GLOBAL_PLUGIN_MGR.lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if ai_guard.is_none() {
                            *ai_guard = Some(crate::script::PluginManager::new());
                        }
                        if let Some(ref mut ai_mgr) = *ai_guard {
                            let cloned_info = crate::script::PluginInfo {
                                lib: info.lib.clone(),
                                path: info.path.clone(),
                                permissions: info.permissions.clone(),
                                last_modified: info.last_modified,
                                version: info.version.clone(),
                                crash_count: info.crash_count,
                                faulted: info.faulted,
                                circuit_breaker: crate::plugin::CircuitBreaker::new(10),
                                abandoned_threads: Vec::new(), // 新实例, 不继承源abandoned线程
                            };
                            ai_mgr.insert_info(alias, cloned_info);

                            // 同步注册命令到 AI 全局 CommandRegistry
                            let mut ai_cmd_guard = crate::ai::GLOBAL_CMD_REGISTRY.lock()
                                .unwrap_or_else(|e| e.into_inner());
                            if ai_cmd_guard.is_none() {
                                *ai_cmd_guard = Some(crate::plugin::CommandRegistry::new());
                            }
                            if let Some(ref mut ai_cmd) = *ai_cmd_guard {
                                let _ = crate::plugin::try_register_from_library(
                                    info.lib.as_ref(), alias, ai_cmd
                                );
                            }
                        }
                    }

                    if let Some(reg) = self.plugin_registry.as_ref() {
                        let info = self.loaded.get(alias);
                        let ver = info.map(|i| i.version.as_str()).unwrap_or("");
                        let _ = reg.update_loaded_info(alias, ver, &[], &[], "");
                    }
                }
                Err(e) => {
                    results.push(format!("  [!] {} — 加载失败: {}", alias, e));
                }
            }
        }

        results
    }

    /// 保存注册表到磁盘
    pub fn save_registry(&self) -> Result<(), String> {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.save(),
            None => Ok(()),
        }
    }

    /// 通过注册表注册插件
    pub fn register_in_registry(&self, alias: &str, path: &str, auto_load: bool, load_order: u32) -> Result<String, String> {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.register(alias, path, auto_load, load_order),
            None => Err("插件注册表未挂载".into()),
        }
    }

    /// 通过注册表注销插件
    pub fn unregister_from_registry(&self, alias: &str) -> Result<String, String> {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.unregister(alias),
            None => Err("插件注册表未挂载".into()),
        }
    }

    /// 启用注册表中的插件
    pub fn enable_in_registry(&self, alias: &str) -> Result<String, String> {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.enable(alias),
            None => Err("插件注册表未挂载".into()),
        }
    }

    /// 禁用注册表中的插件
    pub fn disable_in_registry(&self, alias: &str) -> Result<String, String> {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.disable(alias),
            None => Err("插件注册表未挂载".into()),
        }
    }

    /// 注册表状态报告
    pub fn registry_status(&self) -> String {
        match self.plugin_registry.as_ref() {
            Some(reg) => reg.status_report(),
            None => "插件注册表: 未挂载".into(),
        }
    }

    /// 获取已加载插件的 Library 引用
    /// 供 try_register_from_library 使用，避免重复加载 DLL
    pub fn get_library(&self, name: &str) -> Option<&libloading::Library> {
        self.loaded.get(name).map(|info| info.lib.as_ref())
    }
}

// ═══════════════════════════════════════════════════════════
// v8.0: 全局 PluginManager + 统一命令池调度
// 供 AI execute_tool 路由插件命令 (避免 plugin→script 循环依赖)
// ═══════════════════════════════════════════════════════════

use std::sync::atomic::AtomicPtr;
static GLOBAL_PLUGIN_MANAGER: AtomicPtr<PluginManager> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_global_plugin_manager(mgr: &mut PluginManager) {
    GLOBAL_PLUGIN_MANAGER.store(mgr as *mut PluginManager, std::sync::atomic::Ordering::Release);
}

pub fn execute_plugin_ai_command(name: &str, args_json: &str) -> String {
    let tool = match crate::plugin::find_ai_tool(name) {
        Some(t) => t,
        None => return format!("[!] 插件命令 '{}' 未注册为AI工具", name),
    };

    let args_map: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("[!] 参数JSON解析失败: {}", e),
    };

    let mut cmd_parts: Vec<String> = vec![name.to_string()];
    for param in &tool.params {
        if let Some(val) = args_map.get(&param.name) {
            let s = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            cmd_parts.push(s);
        } else if param.required {
            return format!("[!] 缺少必需参数: {}", param.name);
        }
    }
    let command_str = cmd_parts.join(" ");

    let mgr_ptr = GLOBAL_PLUGIN_MANAGER.load(std::sync::atomic::Ordering::Acquire);
    if mgr_ptr.is_null() {
        return "[!] PluginManager 未初始化".to_string();
    }
    let mgr: &mut PluginManager = unsafe { &mut *mgr_ptr };

    let perms = PermissionSet::all();
    match mgr.call(&tool.plugin_name, &command_str, &perms, 30_000) {
        Ok(result) => result,
        Err(e) => format!("[!] 插件执行失败: {}", e),
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        let names: Vec<String> = self.loaded.keys().cloned().collect();
        for name in names {
            self.unload(&name).ok();
        }
    }
}