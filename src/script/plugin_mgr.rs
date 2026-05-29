use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use super::permissions::{PermissionSet, PluginPermissions};

// ─── 插件信息 v7.1: 子进程架构 — 无进程内DLL ───────
pub(crate) struct PluginInfo {
    /// 原始文件路径 (用于子进程启动)
    pub(crate) path: PathBuf,
    /// 插件声明的权限
    pub(crate) permissions: PluginPermissions,
    /// 最后修改时间 (用于 hotload 检测)
    pub(crate) last_modified: std::time::SystemTime,
    /// 插件版本 (来自 ruoo_plugin_version)
    pub(crate) version: String,
    /// 累计崩溃次数 (防崩溃框架跟踪)
    pub(crate) crash_count: u32,
    /// 是否已被标记为故障 (连续崩溃>=3)
    pub(crate) faulted: bool,
    /// v6.2: 熔断器 — 防止反复崩溃挂起终端
    pub(crate) circuit_breaker: crate::plugin::CircuitBreaker,
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

        // v7.1: 子进程架构 — init后丢弃lib, 执行通过子进程隔离
        drop(lib);

        self.loaded.insert(name.to_string(), PluginInfo {
            path: canonical,
            permissions: perms.clone(),
            last_modified: mtime,
            version,
            crash_count: 0,
            faulted: false,
            circuit_breaker: crate::plugin::CircuitBreaker::new(10),
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

    /// 内部热加载实现 v7.1: 子进程架构 — 临时加载DLL探头后丢弃
    fn hotload_inner(&mut self, name: &str, path: &PathBuf) -> Result<String, String> {
        // 备份旧版本用于回滚
        let old_info = self.loaded.remove(name);
        let old_crash_count = old_info.as_ref().map(|o| o.crash_count).unwrap_or(0);
        // v7.1: 旧 info 直接 drop — 无进程内资源需清理

        // 加载新版 (临时, 用于探头+init)
        let mtime = path.metadata()
            .map_err(|e| format!("无法读取元数据: {}", e))?
            .modified()
            .map_err(|e| format!("无法获取修改时间: {}", e))?;

        let lib = match unsafe { libloading::Library::new(path) } {
            Ok(l) => l,
            Err(e) => {
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
                drop(lib);
                return Err(format!("热加载失败: 探测插件失败 ({}) — 旧版本已卸载, 请手动恢复", e));
            }
        };

        let init_result = unsafe {
            lib.get::<unsafe extern "C" fn() -> i32>(b"ruoo_plugin_init")
        };
        let init = match init_result {
            Ok(func) => func,
            Err(_) => {
                drop(lib);
                return Err("热加载: 缺少 ruoo_plugin_init — 旧版本已卸载".to_string());
            }
        };
        unsafe {
            let code = init();
            if code != 0 {
                drop(lib);
                return Err(format!("热加载: init 返回 {} — 旧版本已卸载", code));
            }
        }

        // v7.1: init 后丢弃 DLL — 执行通过子进程隔离
        drop(lib);

        self.loaded.insert(name.to_string(), PluginInfo {
            path: path.clone(),
            permissions: perms.clone(),
            last_modified: mtime,
            version,
            crash_count: old_crash_count,
            faulted: false,
            circuit_breaker: crate::plugin::CircuitBreaker::new(10),
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
        // v7.1: 所有插件执行统一走子进程隔离 — ACCESS_VIOLATION/STACK_OVERFLOW 不再影响主进程
        self.call_subprocess(name, command, script_perms, timeout_ms)
    }

    /// 无超时调用 (兼容旧接口, v7.1: 走子进程)
    #[allow(dead_code)]
    pub fn call_default(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
    ) -> Result<String, String> {
        self.call_subprocess(name, command, script_perms, 30_000) // 默认30秒超时
    }

    /// 卸载插件 v7.1: 子进程架构 — 无进程内DLL, 直接移除
    pub fn unload(&mut self, name: &str) -> Result<(), String> {
        if self.loaded.remove(name).is_some() {
            Ok(())
        } else {
            Err(format!("插件未加载: {}", name))
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

    /// v7.1: 探测插件命令 — 临时加载DLL读取ruoo_plugin_commands后立即释放
    pub fn probe_plugin_commands(&self, name: &str) -> Result<String, String> {
        let info = self.loaded.get(name)
            .ok_or_else(|| format!("插件未加载: {}", name))?;
        let lib = unsafe {
            libloading::Library::new(&info.path)
                .map_err(|e| format!("无法加载库以探测命令: {}", e))?
        };
        let result = crate::plugin::probe_plugin_commands(&lib);
        drop(lib);
        result
    }


    /// 设置插件注册表引用
    pub fn set_plugin_registry(&mut self, reg: Arc<crate::plugin_registry::PluginRegistry>) {
        self.plugin_registry = Some(reg);
    }


    // ═══════════════════════════════════════════════════════
    // 防崩溃框架 v5.0 — crash guard / force unload / crash recover
    // ═══════════════════════════════════════════════════════

    /// 强制卸载插件 v7.1: 子进程架构 — 无需清理进程内资源
    pub fn force_unload(&mut self, name: &str) -> Result<String, String> {
        let info = match self.loaded.remove(name) {
            Some(i) => i,
            None => return Err(format!("插件未加载: {}", name)),
        };

        let path = info.path.display().to_string();
        let ver = info.version.clone();
        let crashes = info.crash_count;

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

    /// v7.1: 软重启 — 崩溃后一键卸载+重载+恢复可用
    /// 步骤: 记录路径 → 卸载 → 重载 → 重新注册命令 → 重置熔断器
    /// 适用于子进程崩溃后无需重启终端即可恢复插件
    pub fn soft_restart(
        &mut self,
        name: &str,
        cmd_registry: &mut crate::plugin::CommandRegistry,
    ) -> String {
        let mut report = String::new();
        report.push_str(&format!("═══ 插件软重启: {} ═══\n", name));

        // 1. 获取当前状态
        let (dll_path, was_faulted, crashes) = match self.loaded.get(name) {
            Some(info) => (
                info.path.to_string_lossy().to_string(),
                info.faulted,
                info.crash_count,
            ),
            None => {
                // 插件未加载 — 尝试从注册表恢复
                let reg_path = self.plugin_registry.as_ref()
                    .and_then(|reg| reg.get(name))
                    .map(|entry| entry.path.clone());
                if let Some(reg_path) = reg_path {
                    report.push_str("  [*] 插件未加载, 从注册表恢复...\n");
                    match self.load(name, &reg_path) {
                        Ok(msg) => {
                            report.push_str(&format!("  {}\n", msg));
                            match crate::plugin::try_register_from_file(&reg_path, name, cmd_registry) {
                                Ok(n) => report.push_str(&format!("  [+] 已注册 {} 个命令\n", n)),
                                Err(e) => report.push_str(&format!("  [!] 命令注册: {}\n", e)),
                            }
                            report.push_str("  [+] 软重启完成\n");
                            return report;
                        }
                        Err(e) => {
                            report.push_str(&format!("  [!] 加载失败: {}\n", e));
                            return report;
                        }
                    }
                }
                report.push_str("  [!] 插件未加载且注册表无记录\n");
                return report;
            }
        };

        report.push_str(&format!("  路径: {}\n", dll_path));
        report.push_str(&format!("  崩溃次数: {} | 故障标记: {}\n", crashes, was_faulted));

        // 2. 卸载 (保留路径)
        match self.force_unload(name) {
            Ok(msg) => report.push_str(&format!("  [+] 卸载: {}\n", msg)),
            Err(e) => report.push_str(&format!("  [!] 卸载: {}\n", e)),
        }

        // 3. 注销旧命令
        let removed = cmd_registry.unregister_plugin(name);
        if removed > 0 {
            report.push_str(&format!("  [·] 注销 {} 个旧命令\n", removed));
        }

        // 4. 重新加载
        match self.load(name, &dll_path) {
            Ok(msg) => {
                report.push_str(&format!("  [+] 重载: {}\n", msg));
                // 5. 重新注册命令
                match crate::plugin::try_register_from_file(&dll_path, name, cmd_registry) {
                    Ok(n) if n > 0 => report.push_str(&format!("  [+] 重新注册 {} 个命令\n", n)),
                    Ok(_) => report.push_str("  [*] 无命令声明\n"),
                    Err(e) => report.push_str(&format!("  [!] 命令注册: {}\n", e)),
                }
                // 6. 重置熔断器
                if let Some(info) = self.loaded.get_mut(name) {
                    info.circuit_breaker.reset();
                    info.faulted = false;
                    info.crash_count = 0;
                }
                report.push_str("  [+] 熔断器已重置, 故障标记已清除\n");
                report.push_str("  [+] 软重启完成 — 插件已恢复可用\n");
            }
            Err(e) => {
                report.push_str(&format!("  [!] 重载失败: {}\n", e));
                report.push_str("  [!] 软重启失败 — 请检查插件文件\n");
            }
        }

        report
    }

    /// v7.1: 自动恢复所有故障插件
    pub fn auto_recover_all(&mut self, cmd_registry: &mut crate::plugin::CommandRegistry) -> String {
        let faulted_names: Vec<String> = self.loaded
            .iter()
            .filter(|(_, info)| info.faulted || info.crash_count > 0)
            .map(|(n, _)| n.clone())
            .collect();

        if faulted_names.is_empty() {
            return "[+] 所有插件运行正常, 无需恢复".to_string();
        }

        let mut report = String::new();
        report.push_str(&format!(
            "═══ 批量软重启: {} 个故障插件 ═══\n",
            faulted_names.len()
        ));

        for name in &faulted_names {
            report.push_str(&self.soft_restart(name, cmd_registry));
            report.push('\n');
        }

        report.push_str(&format!(
            "[+] 批量恢复完成 — {} 个插件已处理",
            faulted_names.len()
        ));
        report
    }

    /// 安全调用插件 v7.1: 全部走子进程, 崩溃不传播到主进程
    pub fn call_guarded(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
        timeout_ms: u64,
    ) -> Result<String, String> {
        self.call_subprocess(name, command, script_perms, timeout_ms)
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
                    // ★ 命令注册 (v7.1: 临时加载DLL探测)
                    match crate::plugin::try_register_from_file(&entry.path, alias, cmd_registry) {
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
                    // v9.0: 全局 PluginManager 统一单例 — 无需同步

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



    // ═══════════════════════════════════════════════════════════
    // v7.0: 子进程隔离调用 — 100% 防崩溃
    //
    // 架构: spawn ruoo-console.exe --plugin-exec <dll> <cmd>
    //       子进程独立地址空间, 崩溃不影响主进程
    //       通过 stdout JSON 返回结果
    //
    // 性能: 首次spawn ~50-100ms (冷启动), 后续利用OS磁盘缓存更快
    //       对稳定性要求高的插件自动切换到此路径
    // ═══════════════════════════════════════════════════════════

    /// 通过子进程安全调用插件 — 适用于已知会崩溃的插件
    /// 返回 Ok(结果) 或 Err(错误消息, 含详细崩溃报告)
    pub fn call_subprocess(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
        timeout_ms: u64,
    ) -> Result<String, String> {
        use std::process::{Command, Stdio, ExitStatus};
        use std::io::Read;
        use std::time::Instant;
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        // 1. 权限预检查
        script_perms.check_plugin()?;

        // 2. 获取 DLL 路径 + 权限覆盖检查 + 熔断器检查
        let (dll_path, ver) = {
            // 先做不可变检查
            let info = self.loaded.get(name)
                .ok_or_else(|| format!("插件未加载: {}", name))?;

            script_perms.covers(&info.permissions)?;

            if info.faulted {
                return Err(format!(
                    "[!] 插件 '{}' 已被标记为故障 (累计{}次崩溃) — 请使用 crash_recover 恢复",
                    name, info.crash_count
                ));
            }

            (info.path.clone(), info.version.clone())
        };

        // 熔断器检查 (需要 &mut)
        {
            let info = self.loaded.get_mut(name)
                .ok_or_else(|| format!("插件未加载: {}", name))?;
            if !info.circuit_breaker.allow_call() {
                return Err(format!(
                    "插件 '{}' 已熔断: {}",
                    name, info.circuit_breaker.status_report()
                ));
            }
        }

        // 3. 获取当前可执行文件路径
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("无法获取可执行文件路径: {}", e))?;

        // 4. 启动子进程
        let start = Instant::now();
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new(&exe_path);
            c.creation_flags(0x08000000u32); // CREATE_NO_WINDOW
            c
        };
        #[cfg(not(windows))]
        let mut cmd = Command::new(&exe_path);

        let mut child = cmd
            .args(&[
                "--plugin-exec",
                &dll_path.to_string_lossy(),
                command,
                "--timeout",
                &timeout_ms.to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Windows: CREATE_NO_WINDOW — 不弹出控制台窗口
            // (通过 cfg 条件编译在 spawn 之前设置)
        .spawn()
            .map_err(|e| format!("无法启动插件子进程: {}", e))?;

        // 5. 带超时的等待
        let deadline = start + std::time::Duration::from_millis(timeout_ms + 10_000); // 额外10s容错
        let exit_status: ExitStatus;
        let mut stdout_str = String::new();
        let mut stderr_str = String::new();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = status;
                    // 读取 stdout/stderr
                    {
                        let mut out: Box<dyn Read> = Box::new(child.stdout.take().unwrap());
                        let _ = out.read_to_string(&mut stdout_str);
                    }
                    {
                        let mut err: Box<dyn Read> = Box::new(child.stderr.take().unwrap());
                        let _ = err.read_to_string(&mut stderr_str);
                    }
                    break;
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        // 超时 — 杀死子进程
                        let _ = child.kill();
                        let _ = child.wait();
                        // 读取剩余输出
                        if let Some(ref mut out) = child.stdout {
                            let _ = out.read_to_string(&mut stdout_str);
                        }
                        if let Some(ref mut err) = child.stderr {
                            let _ = err.read_to_string(&mut stderr_str);
                        }

                        let elapsed = start.elapsed().as_millis();
                        let err_msg = format!(
                            "═══ 插件子进程超时 ═══\n\
                             插件: {} v{}\n\
                             命令: {}\n\
                             超时: {}ms (限制 {}ms)\n\
                             状态: 子进程已终止, 主进程不受影响\n\
                             stderr: {}",
                            name, ver, command, elapsed, timeout_ms,
                            if stderr_str.is_empty() { "(无)" } else { &stderr_str }
                        );

                        // 更新熔断器
                        if let Some(info) = self.loaded.get_mut(name) {
                            info.crash_count += 1;
                            info.circuit_breaker.record_failure(&err_msg, command);
                            if info.crash_count >= 3 { info.faulted = true; }
                        }

                        return Err(err_msg);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(format!("子进程通信失败: {}", e));
                }
            }
        }

        let elapsed = start.elapsed().as_millis();

        // 6. 解析子进程结果
        if !exit_status.success() {
            // 子进程崩溃或返回非零
            let exit_code = exit_status.code().unwrap_or(-1);

            // 尝试解析 stdout (即使崩溃, 也可能输出 JSON)
            let host_result = serde_json::from_str::<crate::plugin_host::PluginHostResult>(
                stdout_str.trim()
            );

            let error_msg = match host_result {
                Ok(hr) => {
                    format!(
                        "═══ 插件子进程异常退出 ═══\n\
                         插件: {} v{}\n\
                         命令: {}\n\
                         状态: exit_code={}, status={}\n\
                         {}: {}\n\
                         耗时: {}ms\n\
                         主进程不受影响",
                        name, ver, command, exit_code, hr.status,
                        if hr.status == "error" { "错误" } else { "消息" },
                        hr.message, elapsed
                    )
                }
                Err(_) => {
                    let code_u32 = exit_code as u32;
                    let hint = if code_u32 == 0xC0000005u32 {
                        "ACCESS_VIOLATION (内存访问违规)"
                    } else if code_u32 == 0xC00000FDu32 {
                        "STACK_OVERFLOW (栈溢出)"
                    } else if code_u32 == 0xC0000017u32 {
                        "STATUS_NO_MEMORY (内存不足)"
                    } else {
                        "未知系统异常"
                    };

                    format!(
                        "═══ 插件子进程崩溃 ═══\n\
                         插件: {} v{}\n\
                         命令: {}\n\
                         exit_code: 0x{:08X} — {}\n\
                         地址空间隔离: 主进程未受影响\n\
                         stderr: {}\n\
                         耗时: {}ms",
                        name, ver, command, exit_code, hint,
                        if stderr_str.is_empty() { "(无)" } else { &stderr_str },
                        elapsed
                    )
                }
            };

            // 更新熔断器和崩溃计数
            if let Some(info) = self.loaded.get_mut(name) {
                info.crash_count += 1;
                info.circuit_breaker.record_failure(&error_msg, command);
                if info.crash_count >= 3 { info.faulted = true; }
            }

            // 持久化崩溃日志
            {
                let log_path = crate::config::data_dir().join("plugin_crash.log");
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true).append(true).open(&log_path)
                {
                    use std::io::Write;
                    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let _ = writeln!(f, "[{}] [SUBPROCESS CRASH] plugin={} cmd={} | exit=0x{:08X}",
                        ts, name, command, exit_code);
                    let _ = writeln!(f, "  stderr: {}", stderr_str);
                    let _ = f.flush();
                }
            }

            return Err(error_msg);
        }

        // 7. 成功 — 解析 JSON
        let host_result: crate::plugin_host::PluginHostResult = match serde_json::from_str(stdout_str.trim()) {
            Ok(r) => r,
            Err(e) => {
                return Err(format!(
                    "插件子进程返回了无效JSON: {}\n原始输出: {}",
                    e,
                    &stdout_str[..stdout_str.len().min(200)]
                ));
            }
        };

        match host_result.status.as_str() {
            "ok" => {
                // 成功 — 更新熔断器 (不放 success, 用子进程调用意味着插件不稳定)
                if let Some(info) = self.loaded.get_mut(name) {
                    info.circuit_breaker.record_success();
                }
                Ok(host_result.message)
            }
            "error" => {
                // 业务错误 — 不触发熔断
                Err(format!("[子进程] 插件返回错误: {}", host_result.message))
            }
            other => {
                Err(format!("[子进程] 未知状态码: {} — {}", other, host_result.message))
            }
        }
    }

    /// 智能调用 — v7.0: 默认子进程隔离, 100% 防崩溃
    /// 线程模式(call)仅作兼容保留, 不再自动使用
    pub fn call_smart(
        &mut self,
        name: &str,
        command: &str,
        script_perms: &PermissionSet,
        timeout_ms: u64,
    ) -> Result<String, String> {
        // v7.0: 直接走子进程 — 线程模式无法防御 ACCESS_VIOLATION
        self.call_subprocess(name, command, script_perms, timeout_ms)
    }
}

// ═══════════════════════════════════════════════════════════
// v9.0: 统一全局 PluginManager — Arc<Mutex<>> 单例
//
// 所有子系统共享同一实例:
//   - TUI commands (main.rs)
//   - AI tools (ai/executor.rs)
//   - Script engine (script/mod.rs)
//
// 不再需要 sync/merge/copy 操作。
// ═══════════════════════════════════════════════════════════

use std::sync::{Mutex, OnceLock};

/// 全局插件管理器单例 — 整个进程只有一个实例
static GLOBAL_PLUGIN_MANAGER: OnceLock<Arc<Mutex<PluginManager>>> = OnceLock::new();

/// 初始化全局 PluginManager (main.rs 启动时调用一次)
/// 返回 Arc 供 App 存储引用
/// 如果已初始化则返回现有实例 (幂等操作)
pub fn init_global_plugin_manager() -> Arc<Mutex<PluginManager>> {
    GLOBAL_PLUGIN_MANAGER.get_or_init(|| {
        Arc::new(Mutex::new(PluginManager::new()))
    }).clone()
}

/// 获取全局 PluginManager 的 Arc 引用
pub fn get_global_plugin_manager() -> Option<Arc<Mutex<PluginManager>>> {
    GLOBAL_PLUGIN_MANAGER.get().cloned()
}

/// 获取全局 PluginManager 的 Arc 引用 (必须已初始化)
pub fn global_plugin_manager() -> Arc<Mutex<PluginManager>> {
    GLOBAL_PLUGIN_MANAGER.get()
        .expect("PluginManager 未初始化 — 请先调用 init_global_plugin_manager()")
        .clone()
}

/// v8.0 兼容: 设置全局 PluginManager (已废弃, 改用 init_global_plugin_manager)
#[deprecated(note = "使用 init_global_plugin_manager() 替代")]
pub fn set_global_plugin_manager(_mgr: &mut PluginManager) {
    // 不再需要 — 全局单例由 init_global_plugin_manager 管理
}

/// AI工具命令调度入口 (v9.0: 使用全局单例)
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

    let mgr = match get_global_plugin_manager() {
        Some(m) => m,
        None => return "[!] PluginManager 未初始化".to_string(),
    };

    let mut guard = mgr.lock().unwrap_or_else(|e| e.into_inner());
    let perms = PermissionSet::all();
    // v7.0: 使用智能调用 — 崩溃后自动降级到子进程
    match guard.call_smart(&tool.plugin_name, &command_str, &perms, 30_000) {
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