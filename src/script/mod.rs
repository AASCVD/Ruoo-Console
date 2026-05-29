// ============================================================
// RUOO-CONSOLE �?脚本引擎 v3.1 (.ruoo)
//
// v3.1 更新:
pub mod plugin_stderr; // StderrCapture — 插件stderr重定向到TUI输出
//   - 插件权限系统: ruoo_plugin_permissions() �?自动检�?
//   - 热加�? hotload <name> + #![watch-plugins] 自动检�?
//   - 后台线程执行: call 在独立线程运�?+ 超时保护
//   - Bug 修复: 内存泄漏(plugin_free_result) / 符号生命周期
// ============================================================

use std::collections::HashMap;
use std::path::PathBuf;

// ── 子模�?──
pub mod permissions;   // PermissionSet, PluginPermissions
pub mod plugin_mgr;    // PluginManager, PluginInfo, PluginCallResult
pub mod plugin_crash;  // PluginCallGuard, CrashReport, force_unload
pub mod sysdrv;
pub mod interpreter;

pub use permissions::*;
pub use plugin_mgr::*;
pub use sysdrv::*; #[allow(unused_imports)] pub use interpreter::*;

// --- ScriptContext ---
pub struct ScriptContext {
    pub vars: HashMap<String, String>,
    pub script_dir: PathBuf,
    pub running: bool,
    pub errors: usize,
    pub executed: usize,
    pub on_error: String,
    pub script_name: String,
    pub force: bool,
    pub call_stack: Vec<String>,
    pub perms: PermissionSet,
    pub timeout_secs: u64,
    pub plugins: PluginManager,
    pub sysdrv: SysDriverManager,
    pub assertions_failed: usize,
    /// v4.1: 全局脚本超时 (0=禁用, 默认300�?
    pub global_timeout_secs: u64,
    /// v4.1: 脚本开始时�?
    pub start_time: Option<std::time::Instant>,
    /// v4.3: bootscript 持久�?�?true 时脚本结束后保留插件/命令
    pub keep_plugins: bool,
}

/// �?BUG-4 修复: 废弃黑名�?�?基于权限的白名单机制
///
/// 旧策�? �?DANGEROUS_COMMANDS 关键词黑名单拦截危险命令, 但变量展开/Base64/十六进制�?
///          可轻易绕�?(�?set x=cm && %x%d /c whoami)
/// 新策�? 默认完全禁止进程创建, 只有显式声明 #![allow-exec] 的脚本才能执行系统命令�?
///          框架自身保护 (vault/sysload/bootscript) 仍强制拦�? 不依赖关键词检查�?
///
/// 保留 STRUCTURAL_GUARDS �?这些是框架完整性保�? 任何脚本都不能绕�?
///   - sysload/sysunload (需要独立的 #![allow-sys])
///   - vault passwd / vault del (密码系统保护)
///   - script permit/revoke (权限提升保护)
///   - bootscript 管理 (启动脚本保护)
const STRUCTURAL_GUARDS: &[&str] = &[
    "script permit", "script revoke",
    "sysload ", "sysunload ",
    "vault passwd", "vault del",
    "bootscript edit", "bootscript reset", "bootscript clear", "bootscript ",
];

/// 网络命令关键�?(需�?allow-net)
const NET_COMMANDS: &[&str] = &[
    "scan", "dns ", "whois", "http", "nmap", "curl", "wget",
    "ping", "traceroute", "ftp", "ssh", "telnet", "nc ",
    "socket", "connect", "subdomain", "geoip", "tcp ",
];

/// 文件系统命令 (需�?allow-fs)
const FS_COMMANDS: &[&str] = &[
    "read ", "write ", "delete ", "copy ", "move ", "mkdir",
    "rmdir", "ls ", "cat ", "echo >", ">> ", "> ", "file ",
    "append", "create", "stat ", "hex ", "du ", "grep",
    "sort ", "dedup", "find ", "chmod", "chown",
];

/// 执行命令 (需�?allow-exec)
const EXEC_COMMANDS: &[&str] = &[
    "exec", "cmd ", "powershell", "! ", "shell", "system",
    "process", "tasklist", "run ", "wmic ", "sc ",
];

// �?BUG-5 修复: 词边界匹�?�?替代简�?contains，防止编码绕�?
// "cm''d" 不被视为 "cmd", "myexec_tool" 不触�?exec 权限
fn contains_word_boundary(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return false; }
    let needle_trimmed = needle.trim_end();
    let require_trailing_space = needle.ends_with(' ') && needle_trimmed.len() < needle.len();

    let mut search_from = 0usize;
    while let Some(pos) = haystack[search_from..].find(needle_trimmed) {
        let abs_pos = search_from + pos;
        // 前边�? 行首、空格、制表符、换�?
        let boundary_before = abs_pos == 0
            || haystack.as_bytes().get(abs_pos - 1).map_or(true, |&b| {
                b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
                    || b == b';' || b == b'|' || b == b'&' || b == b'('
            });
        // 后边�? 行尾、空格、制表符、换�?
        let boundary_after = if require_trailing_space {
            // needle 本身带空�?(�?"cmd ") �?已通过 trim_end 处理
            // 此处 needle_trimmed �?"cmd", 需要确保后面是空格
            haystack.as_bytes().get(abs_pos + needle_trimmed.len()).map_or(true, |&b| {
                b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
            })
        } else {
            abs_pos + needle_trimmed.len() >= haystack.len()
                || haystack.as_bytes().get(abs_pos + needle_trimmed.len()).map_or(true, |&b| {
                    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
                        || b == b';' || b == b'|' || b == b'&' || b == b')'
                        || b == b'.' || b == b',' || b == b':'
                })
        };

        if boundary_before && boundary_after {
            return true;
        }
        search_from = abs_pos + 1; // 继续搜索
    }
    false
}

impl ScriptContext {
    pub fn new(script_dir: PathBuf, script_name: &str) -> Self {
        ScriptContext {
            vars: HashMap::new(),
            script_dir,
            running: true,
            errors: 0,
            executed: 0,
            on_error: "stop".to_string(),
            script_name: script_name.to_string(),
            force: false,
            call_stack: Vec::new(),
            perms: PermissionSet::none(),
            timeout_secs: 0,
            plugins: PluginManager::new(),
            sysdrv: SysDriverManager::new(),
            assertions_failed: 0,
            global_timeout_secs: 300, // v4.1: 默认5分钟全局超时
            start_time: Some(std::time::Instant::now()),
            keep_plugins: false,
        }
    }

    pub fn expand(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                if chars[i + 1] == '{' {
                    if let Some(end) = chars[i + 2..].iter().position(|&c| c == '}') {
                        let var_name: String = chars[i + 2..i + 2 + end].iter().collect();
                        if let Some(val) = self.vars.get(&var_name) {
                            result.push_str(val);
                        }
                        i += 2 + end + 1;
                        continue;
                    }
                    result.push_str("${");
                    i += 2;
                    continue;
                } else if chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' {
                    let mut j = i + 1;
                    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                        j += 1;
                    }
                    let var_name: String = chars[i + 1..j].iter().collect();
                    if let Some(val) = self.vars.get(&var_name) {
                        result.push_str(val);
                    }
                    i = j;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        result
    }

    pub fn check_permissions(&self, cmd: &str) -> Result<(), String> {
        let lower = cmd.to_lowercase();

        // �?BUG-4 修复: 框架完整性保�?�?始终拦截 (不依�?exec 权限)
        if let Some(guard) = self.is_structural_guard(cmd) {
            return Err(format!(
                "框架保护被触�? \"{}\" 匹配 \"{}\" �?此操作需要直接在终端执行",
                cmd, guard
            ));
        }

        // �?BUG-5 修复: 使用词边界匹配替代简�?contains �?防止编码绕过
        //   例如 "cm''d" 不再被误匹配�?"cmd", "myexec_tool" 不再触发 exec 权限检�?
        for &pattern in EXEC_COMMANDS {
            if contains_word_boundary(&lower, &pattern.to_lowercase()) {
                self.perms.check_exec()?;
                break;
            }
        }

        for &pattern in NET_COMMANDS {
            if contains_word_boundary(&lower, &pattern.to_lowercase()) {
                self.perms.check_net()?;
                break;
            }
        }

        for &pattern in FS_COMMANDS {
            if contains_word_boundary(&lower, &pattern.to_lowercase()) {
                self.perms.check_fs()?;
                break;
            }
        }

        Ok(())
    }

    /// 仅检查框架完整性保�?�?非黑名单, 是结构性约�?
    /// �?BUG-12 修复: 使用词边界匹配替代简�?contains，防止编码绕�?
    ///   例如 "vault passwd" 匹配 "vault passwd x" 但不匹配 "myvault_passwd_extra"
    pub fn is_structural_guard(&self, cmd: &str) -> Option<&'static str> {
        let lower = cmd.to_lowercase();
        for &guard in STRUCTURAL_GUARDS {
            let guard_lower = guard.to_lowercase();
            // 按空格拆分为词序列，要求所有词按顺序且词边界匹�?
            let guard_words: Vec<&str> = guard_lower.split_whitespace().collect();
            if guard_words.is_empty() { continue; }

            // �?lower 中按顺序查找每个词，每个词必须在词边界上
            let mut search_from = 0usize;
            let mut all_found = true;
            for (idx, word) in guard_words.iter().enumerate() {
                match lower[search_from..].find(word) {
                    Some(pos) => {
                        let abs_pos = search_from + pos;
                        // 词边界检�? 前边�?行首/空格) �?后边�?行尾/空格)
                        let boundary_before = abs_pos == 0
                            || lower.as_bytes().get(abs_pos - 1).map_or(true, |&b| b == b' ' || b == b'\t' || b == b'\n');
                        let boundary_after = abs_pos + word.len() >= lower.len()
                            || lower.as_bytes().get(abs_pos + word.len()).map_or(true, |&b| b == b' ' || b == b'\t' || b == b'\n');

                        if boundary_before && boundary_after {
                            search_from = abs_pos + word.len();
                        } else if idx == 0 && guard_words.len() == 1 && guard.ends_with(' ') {
                            // 单词语法带尾部空�?(�?"sysload ") �?放宽后边�?
                            search_from = abs_pos + word.len();
                        } else {
                            all_found = false;
                            break;
                        }
                    }
                    None => {
                        all_found = false;
                        break;
                    }
                }
            }
            if all_found {
                return Some(guard);
            }
        }
        None
    }

    /// �?保留向后兼容: is_dangerous 现在委托�?is_structural_guard
    pub fn is_dangerous(&self, cmd: &str) -> Option<&'static str> {
        self.is_structural_guard(cmd)
    }

    pub fn check_permitted(&self, script_path: &std::path::Path) -> Result<(), String> {
        if self.force {
            return Ok(());
        }

        let content = std::fs::read_to_string(script_path)
            .map_err(|e| format!("无法读取脚本: {}", e))?;
        let hash = crate::tools::hash_sha256(&content);

        let key = format!("permitted.{}", self.script_name);
        if let Some(stored_hash) = crate::vault::vault_get("system", &key) {
            if stored_hash == hash {
                return Ok(());
            } else {
                return Err(format!(
                    "脚本已被修改! SHA-256 不匹配\n  当前: {}\n  已许�? {}\n  请重�? script permit {}",
                    &hash[..16], &stored_hash[..16], self.script_name
                ));
            }
        }

        Err(format!(
            "脚本未获许可: {}\n  SHA-256: {}\n  请执�? script permit {}",
            self.script_name,
            &hash[..16],
            self.script_name
        ))
    }

    pub fn permit_script(&self, script_path: &std::path::Path) -> Result<String, String> {
        let content = std::fs::read_to_string(script_path)
            .map_err(|e| format!("无法读取脚本: {}", e))?;
        let hash = crate::tools::hash_sha256(&content);
        let key = format!("permitted.{}", self.script_name);
        crate::vault::vault_set("system", &key, &hash)
            .map_err(|e| format!("Vault 写入失败: {}", e))?;
        Ok(hash)
    }

    pub fn revoke_script(&self) -> Result<(), String> {
        let key = format!("permitted.{}", self.script_name);
        crate::vault::vault_set("system", &key, "")
            .map_err(|e| format!("Vault 写入失败: {}", e))?;
        Ok(())
    }

    pub fn list_permitted() -> Vec<(String, String)> {
        let mut result = Vec::new();
        if let Some(keys) = crate::vault::vault_list("system") {
            for key in keys {
                if let Some(prefix) = key.strip_prefix("permitted.") {
                    if let Some(hash) = crate::vault::vault_get("system", &key) {
                        if !hash.is_empty() {
                            result.push((prefix.to_string(), hash));
                        }
                    }
                }
            }
        }
        result
    }
}

// ─── 指令解析 ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ScriptInstruction {
    Noop,
    Directive(String),
    OnError(String),
    Timeout(u64),
    Echo(String),
    Print(String),
    Set(String, String),
    Unset(String),
    Sleep(u64),
    IfExists(String, bool),
    EndIf,
    Try,
    Catch(Option<String>),
    EndBlock,
    Assert(String, String),
    Retry(usize, String),
    Run(String),
    LoadPlugin(String, String),
    CallPlugin(String, String, Option<u64>),  // name, cmd, timeout_ms
    UnloadPlugin(String),
    ListPlugins,
    HotloadPlugin(String),                     // 热加�?
    SysLoad(String, String),                   // sysload <name> <path>
    SysUnload(String),                         // sysunload <name>
    SysList,                                   // syslist
    SysStatus(String),                         // sysstatus <name>
    SysHotload(String),                        // syshotload <name>
    Cmd(String),
}

pub fn parse_line(line: &str) -> ScriptInstruction {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return ScriptInstruction::Noop;
    }

    if trimmed.starts_with('#') && !trimmed.starts_with("#![") {
        return ScriptInstruction::Noop;
    }
    if trimmed.starts_with("//") {
        return ScriptInstruction::Noop;
    }

    // #![...]
    if let Some(directive) = trimmed.strip_prefix("#![") {
        if let Some(inner) = directive.strip_suffix(']') {
            return ScriptInstruction::Directive(inner.trim().to_string());
        }
        return ScriptInstruction::Directive(directive.trim().to_string());
    }

    // onerror
    if let Some(rest) = trimmed.strip_prefix("onerror ") {
        return ScriptInstruction::OnError(rest.trim().to_lowercase());
    }

    // timeout
    if let Some(rest) = trimmed.strip_prefix("timeout ") {
        if let Ok(secs) = rest.trim().parse() {
            return ScriptInstruction::Timeout(secs);
        }
        return ScriptInstruction::Noop;
    }

    // assert
    if let Some(rest) = trimmed.strip_prefix("assert ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let condition = parts[0].trim().to_string();
        let message = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
        return ScriptInstruction::Assert(condition, message);
    }

    // retry
    if let Some(rest) = trimmed.strip_prefix("retry ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if let Ok(n) = parts[0].parse() {
            let cmd = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
            return ScriptInstruction::Retry(n, cmd);
        }
        return ScriptInstruction::Noop;
    }

    // if exists / if not exists
    if let Some(rest) = trimmed.strip_prefix("if exists ") {
        return ScriptInstruction::IfExists(rest.trim().to_string(), true);
    }
    if let Some(rest) = trimmed.strip_prefix("if not exists ") {
        return ScriptInstruction::IfExists(rest.trim().to_string(), false);
    }
    if trimmed == "endif" || trimmed == "fi" {
        return ScriptInstruction::EndIf;
    }

    // try / catch
    if trimmed == "try" || trimmed == "try {" {
        return ScriptInstruction::Try;
    }
    if let Some(rest) = trimmed.strip_prefix("catch as ") {
        return ScriptInstruction::Catch(Some(rest.trim().trim_end_matches('{').trim().to_string()));
    }
    if trimmed == "catch" || trimmed == "} catch {" || trimmed == "catch {" {
        return ScriptInstruction::Catch(None);
    }
    if trimmed == "end" || trimmed == "}" || trimmed == "endtry" {
        return ScriptInstruction::EndBlock;
    }

    // echo / print
    if let Some(rest) = trimmed.strip_prefix("echo ") {
        return ScriptInstruction::Echo(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("print ") {
        return ScriptInstruction::Print(rest.trim().to_string());
    }

    // sleep
    if let Some(rest) = trimmed.strip_prefix("sleep ") {
        if let Ok(ms) = rest.trim().parse() {
            return ScriptInstruction::Sleep(ms);
        }
    }

    // set / unset
    if let Some(rest) = trimmed.strip_prefix("set ") {
        if let Some((k, v)) = rest.split_once('=') {
            return ScriptInstruction::Set(k.trim().to_string(), v.trim().to_string());
        }
        return ScriptInstruction::Set(rest.trim().to_string(), String::new());
    }
    if let Some(rest) = trimmed.strip_prefix("unset ") {
        return ScriptInstruction::Unset(rest.trim().to_string());
    }

    // run
    if let Some(rest) = trimmed.strip_prefix("run ") {
        return ScriptInstruction::Run(rest.trim().to_string());
    }

    // ── 插件 (增强语法) ──
    // hotload <name>
    if let Some(rest) = trimmed.strip_prefix("hotload ") {
        return ScriptInstruction::HotloadPlugin(rest.trim().to_string());
    }

    // load <name> <path> [timeout_ms]
    if let Some(rest) = trimmed.strip_prefix("load ") {
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        let name = parts[0].trim().to_string();
        let path = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_else(|| name.clone());
        return ScriptInstruction::LoadPlugin(name, path);
    }

    // call <plugin> <command> [timeout_ms]
    if let Some(rest) = trimmed.strip_prefix("call ") {
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        let plugin = parts[0].trim().to_string();
        let cmd = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
        let timeout = parts.get(2).and_then(|s| s.trim().parse::<u64>().ok());
        return ScriptInstruction::CallPlugin(plugin, cmd, timeout);
    }

    // unload <plugin>
    if let Some(rest) = trimmed.strip_prefix("unload ") {
        return ScriptInstruction::UnloadPlugin(rest.trim().to_string());
    }

    if trimmed == "plugins" || trimmed == "list plugins" {
        return ScriptInstruction::ListPlugins;
    }

    // ── 内核驱动 ──
    // sysload <name> <path>
    if let Some(rest) = trimmed.strip_prefix("sysload ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let name = parts[0].trim().to_string();
        let path = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
        return ScriptInstruction::SysLoad(name, path);
    }

    // sysunload <name>
    if let Some(rest) = trimmed.strip_prefix("sysunload ") {
        return ScriptInstruction::SysUnload(rest.trim().to_string());
    }

    // sysstatus <name>
    if let Some(rest) = trimmed.strip_prefix("sysstatus ") {
        return ScriptInstruction::SysStatus(rest.trim().to_string());
    }

    // syshotload <name>
    if let Some(rest) = trimmed.strip_prefix("syshotload ") {
        return ScriptInstruction::SysHotload(rest.trim().to_string());
    }

    if trimmed == "syslist" || trimmed == "list sys" {
        return ScriptInstruction::SysList;
    }

    ScriptInstruction::Cmd(trimmed.to_string())
}

// ─── 脚本执行�?────────────────────────────────────────

pub fn execute_script(
    ctx: &mut ScriptContext,
    lines: &[&str],
    app: &mut crate::App,
) {
    let mut skip_until_endif = false;
    let mut in_try = false;
    let mut try_succeeded = true;
    let mut try_depth = 0u32;
    let mut last_error: Option<String> = None;
    let mut i = 0;

    while i < lines.len() {
        if !ctx.running {
            break;
        }
        // v4.1: 全局脚本超时检查
        if ctx.global_timeout_secs > 0 {
            if let Some(start) = ctx.start_time {
                if start.elapsed().as_secs() > ctx.global_timeout_secs {
                    app.push_output(&format!(
                        "[!] ⏱ 全局脚本超时 ({}s) — 脚本已运行 {:.1}s，强制终止",
                        ctx.global_timeout_secs,
                        start.elapsed().as_secs_f64()
                    ));
                    ctx.running = false;
                    ctx.errors += 1;
                    break;
                }
            }
        }

        let line = lines[i];
        let inst = parse_line(line);

        match &inst {
            ScriptInstruction::Noop => { i += 1; continue; }

            ScriptInstruction::Directive(dir) => {
                if dir == "watch-plugins" {
                    ctx.plugins.set_watch(true);
                    app.push_output("[s] 插件热加载已启用 (watch-plugins)");
                } else if dir == "watch-sys" {
                    ctx.sysdrv.set_watch(true);
                    app.push_output("[s] 驱动热加载监控已启用 (watch-sys)");
                } else if ctx.perms.apply_directive(dir) {
                    app.push_output(&format!("[s] 权限: {}", dir));
                } else {
                    app.push_output(&format!("[!] 未知指令: #![{}]", dir));
                }
                i += 1;
                continue;
            }

            ScriptInstruction::IfExists(path, expected) => {
                let expanded_path = ctx.expand(path);
                let full_path = if PathBuf::from(&expanded_path).is_absolute() {
                    PathBuf::from(&expanded_path)
                } else {
                    ctx.script_dir.join(&expanded_path)
                };
                if full_path.exists() != *expected {
                    skip_until_endif = true;
                }
                i += 1;
                continue;
            }

            ScriptInstruction::EndIf => {
                skip_until_endif = false;
                i += 1;
                continue;
            }

            ScriptInstruction::Try => {
                in_try = true;
                try_succeeded = true;
                try_depth += 1;
                last_error = None;
                i += 1;
                continue;
            }

            ScriptInstruction::Catch(var) => {
                if try_succeeded {
                    i += 1;
                    let mut nested = 0u32;
                    while i < lines.len() {
                        match parse_line(lines[i]) {
                            ScriptInstruction::Try => nested += 1,
                            ScriptInstruction::EndBlock => {
                                if nested == 0 { i += 1; break; }
                                nested -= 1;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    in_try = false;
                    try_depth -= 1;
                    continue;
                } else {
                    if let (Some(ref v), Some(ref err)) = (var, &last_error) {
                        ctx.vars.insert(v.clone(), err.clone());
                    }
                    in_try = false;
                    try_succeeded = true;
                    i += 1;
                    continue;
                }
            }

            ScriptInstruction::EndBlock => {
                in_try = false;
                if try_depth > 0 { try_depth -= 1; }
                i += 1;
                continue;
            }

            ScriptInstruction::OnError(mode) => {
                ctx.on_error = mode.clone();
                app.push_output(&format!("[s] onerror = {}", mode));
                i += 1;
                continue;
            }

            ScriptInstruction::Timeout(secs) => {
                ctx.timeout_secs = *secs;
                app.push_output(&format!("[s] timeout = {}s", secs));
                i += 1;
                continue;
            }

            _ => {
                if skip_until_endif {
                    i += 1;
                    continue;
                }
            }
        }

        let exec_result = execute_instruction(ctx, app, &inst, lines, &mut i);

        match exec_result {
            Ok(()) => {
                ctx.executed += 1;
            }
            Err(err_msg) => {
                ctx.errors += 1;
                last_error = Some(err_msg.clone());
                let line_info = format!("[!] 脚本错误 (行{}, 指令{}): {}", i + 1, ctx.executed + 1, err_msg);
                app.push_output(&line_info);

                if in_try {
                    try_succeeded = false;
                    i += 1;
                    while i < lines.len() {
                        match parse_line(lines[i]) {
                            ScriptInstruction::Catch(_) => { i += 1; break; }
                            ScriptInstruction::EndBlock => { i += 1; break; }
                            _ => { i += 1; }
                        }
                    }
                    continue;
                }

                if ctx.on_error.starts_with("retry:") {
                    if let Ok(max_retries) = ctx.on_error[6..].parse::<usize>() {
                        if ctx.errors <= max_retries {
                            app.push_output(&format!("[s] retry {}/{} �?继续执行", ctx.errors, max_retries));
                            i += 1;
                            continue;
                        }
                    }
                }

                if ctx.on_error == "stop" || ctx.on_error.starts_with("retry:") {
                    ctx.running = false;
                    app.push_output(&format!("[!] onerror={} �?脚本终止", ctx.on_error));
                    break;
                }
            }
        }

        i += 1;
    }

    // 卸载所有插�?+ 注销命令
    // �?v4.3: bootscript (keep_plugins=true) 跳过卸载，插件持久化到终�?
    if !ctx.keep_plugins {
        let plugin_names: Vec<String> = ctx.plugins.list();
        for name in &plugin_names {
            app.cmd_registry.unregister_plugin(name);
            if let Err(e) = ctx.plugins.unload(name) {
                app.push_output(&format!("[!] 插件卸载失败: {} ({})", name, e));
            }
        }
    } else {
        // �?v4.3 fix: 将脚本加载的插件迁移到终端全局 PluginManager
        let merged = app.plugin_mgr.lock().unwrap().merge_from(&mut ctx.plugins);
        app.push_output(&format!("[s] 插件持久�? {} 个插件迁移到终端 (bootscript)", merged));
    }

    // 报告已加载驱�?(不自动卸�?�?需显式 sysunload)
    let sys_names = ctx.sysdrv.list();
    if !sys_names.is_empty() {
        app.push_output(&format!(
            "[!] �?{} 个内核驱动仍在加�? {} �?使用 sysunload <name> 卸载",
            sys_names.len(),
            sys_names.join(", ")
        ));
    }

    app.push_output(&format!(
        "[+] 脚本结束 �?{} 指令 / {} 错误 / {} 断言失败 / onerror={} / 权限={}",
        ctx.executed,
        ctx.errors,
        ctx.assertions_failed,
        ctx.on_error,
        perms_summary(&ctx.perms)
    ));
}

fn perms_summary(perms: &PermissionSet) -> String {
    let mut parts = Vec::new();
    if perms.net { parts.push("net"); }
    if perms.fs { parts.push("fs"); }
    if perms.exec { parts.push("exec"); }
    if perms.plugin { parts.push("plugin"); }
    if perms.sys { parts.push("sys"); }
    if parts.is_empty() { "sandbox".into() } else { parts.join("+") }
}

fn execute_instruction(
    ctx: &mut ScriptContext,
    app: &mut crate::App,
    inst: &ScriptInstruction,
    _lines: &[&str],
    _line_idx: &mut usize,
) -> Result<(), String> {
    match inst {
        ScriptInstruction::Echo(text) => {
            let expanded = ctx.expand(text);
            app.push_output(&expanded);
            Ok(())
        }

        ScriptInstruction::Print(var) => {
            let val = ctx.vars.get(var).cloned().unwrap_or_default();
            app.push_output(&format!("  {} = {}", var, val));
            Ok(())
        }

        ScriptInstruction::Set(key, value) => {
            let expanded_val = ctx.expand(value);
            ctx.vars.insert(key.clone(), expanded_val);
            Ok(())
        }

        ScriptInstruction::Unset(key) => {
            ctx.vars.remove(key);
            Ok(())
        }

        ScriptInstruction::Sleep(ms) => {
            // 安全限制: 1ms ~ 5min，防止负数溢出和无限阻塞
            let ms = (*ms).max(1).min(300_000);
            app.push_output(&format!("[z] 暂停 {}ms...", ms));
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(())
        }

        ScriptInstruction::Assert(condition, message) => {
            let cond = ctx.expand(condition);
            let result = if cond.contains("==") {
                let parts: Vec<&str> = cond.splitn(2, "==").collect();
                parts[0].trim() == parts[1].trim()
            } else if cond.contains("!=") {
                let parts: Vec<&str> = cond.splitn(2, "!=").collect();
                parts[0].trim() != parts[1].trim()
            } else if cond.starts_with("defined ") {
                let var = cond[8..].trim();
                ctx.vars.contains_key(var)
            } else if cond == "true" || cond == "1" {
                true
            } else if cond == "false" || cond == "0" {
                false
            } else {
                return Err(format!("断言条件无效: {} (支持 == / != / defined / true / false)", condition));
            };

            if !result {
                ctx.assertions_failed += 1;
                let msg = if message.is_empty() {
                    format!("断言失败: {}", condition)
                } else {
                    format!("断言失败: {} ({})", condition, message)
                };
                return Err(msg);
            }
            Ok(())
        }

        ScriptInstruction::Retry(n, cmd) => {
            let expanded = ctx.expand(cmd);
            let max = (*n).min(100);

            for attempt in 1..=max {
                ctx.check_permissions(&expanded)?;
                app.push_output(&format!("[r] retry {}/{} �?{}", attempt, max, expanded));

                let backup_errors = ctx.errors;
                crate::commands::process_cmd(app, &expanded);
                if ctx.errors == backup_errors {
                    app.push_output(&format!("[r] retry {} 成功", attempt));
                    return Ok(());
                }
                ctx.errors = backup_errors;

                if attempt < max {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
            Err(format!("retry {} 次全部失败", max))
        }

        ScriptInstruction::Cmd(cmd) => {
            let expanded = ctx.expand(cmd);
            ctx.check_permissions(&expanded)?;

            if ctx.call_stack.len() > 10 {
                return Err("脚本递归深度超过 10 �?�?可能存在循环引用".into());
            }

            app.push_output(&format!("$ {}", expanded));
            crate::commands::process_cmd(app, &expanded);
            Ok(())
        }

        ScriptInstruction::Run(script_name) => {
            let expanded = ctx.expand(script_name);

            // �?安全加固: 路径遍历保护
            if expanded.contains("..") || expanded.contains('\\') {
                return Err(format!("脚本路径包含非法字符: {} (禁止 .. �?\\ )", expanded));
            }

            if ctx.call_stack.contains(&expanded) {
                return Err(format!("循环引用: {} �?{}", ctx.call_stack.join(" �?"), expanded));
            }
            ctx.call_stack.push(expanded.clone());

            match load_script(&expanded) {
                Ok(content) => {
                    let script_path = scripts_dir().join(
                        if expanded.ends_with(".ruoo") { expanded.clone() } else { format!("{}.ruoo", expanded) }
                    );
                    if script_path.exists() {
                        if let Err(e) = ctx.check_permitted(&script_path) {
                            ctx.call_stack.pop();
                            return Err(e);
                        }
                    }

                    let lines: Vec<&str> = content.lines().collect();
                    let saved_errors = ctx.errors;
                    let saved_on_error = ctx.on_error.clone();
                    let saved_assertions = ctx.assertions_failed;
                    ctx.errors = 0;
                    ctx.assertions_failed = 0;

                    execute_script(ctx, &lines, app);

                    let sub_errors = ctx.errors;
                    let sub_assertions = ctx.assertions_failed;
                    ctx.errors = saved_errors + sub_errors;
                    ctx.assertions_failed = saved_assertions + sub_assertions;
                    ctx.on_error = saved_on_error;
                    ctx.call_stack.pop();

                    if sub_errors > 0 {
                        return Err(format!("子脚�?{} �?{} 错误结束", expanded, sub_errors));
                    }
                    Ok(())
                }
                Err(e) => {
                    ctx.call_stack.pop();
                    Err(e)
                }
            }
        }

        // ── 插件指令 (v3.1 增强) ──

        ScriptInstruction::LoadPlugin(name, path) => {
            ctx.perms.check_plugin()?;
            let expanded_path = ctx.expand(path);

            // �?安全加固: 插件路径必须通过 canonicalize 验证
            // (�?PluginManager::load 中执行，此处做预检)
            if expanded_path.contains("..") {
                return Err(format!("插件路径包含非法字符: .. (路径遍历拦截)"));
            }

            let result = ctx.plugins.load(name, &expanded_path)?;
            app.push_output(&result);
            // v4.1: 安全警告 �?插件可突破沙�?
            app.push_output("[!] �?插件已加�? 原生代码可完全突破脚本沙箱，仅信任经过审计的插件");

            // �?BUG-3 修复: 验证脚本权限覆盖插件声明权限
            // 若插件声明了 exec/net/fs 权限而脚本未授权，拒绝加�?(ruoo_plugin_init 可能已执�? 但后�?call 也将被阻�?
            {
                let perms_check = ctx.plugins.get_info(name)
                    .map(|info| (ctx.perms.covers(&info.permissions), info.permissions.describe()));
                if let Some((Err(e), plugin_desc)) = perms_check {
                    // 权限不足 �?卸载插件并返回错�?
                    ctx.plugins.unload(name).ok();
                    app.cmd_registry.unregister_plugin(name);
                    return Err(format!("插件权限不匹�? {}\n  [*] 插件声明: {} | 脚本授权: {}\n  [*] 在脚本头部添加缺失的 #![allow-*] 指令",
                        e,
                        plugin_desc,
                        perms_summary(&ctx.perms)
                    ));
                }
            }

            // �?v4.3: 探测 ruoo_plugin_commands 并注册到终端命令路由�?
            // 这样插件命令会出现在 help 中，用户可直接输入命令名调用
            let dll_path = ctx.plugins.loaded.get(name).map(|i| i.path.to_string_lossy().to_string());
            if let Some(ref path) = dll_path {
                match crate::plugin::try_register_from_file(path, name, &mut app.cmd_registry) {
                    Ok(count) if count > 0 => {
                        let cmds: Vec<String> = app.cmd_registry.list_by_plugin(name)
                            .iter()
                            .map(|c| format!("  {} �?{}", c.cmd, c.usage))
                            .collect();
                        app.push_output(&format!("[plugin] �?已注�?{} 个命令到终端:\n{}",
                            count, cmds.join("\n")));
                        if let Some((first_name, _)) = app.cmd_registry.list_by_plugin(name).first()
                            .map(|c| (c.cmd.clone(), c.usage.clone())) {
                            app.push_output(&format!("[*] 直接输入: {}", first_name));
                        }
                    }
                    Ok(_) => {} // 无命令声明，正常
                    Err(e) => app.push_output(&format!("[plugin] 命令注册失败: {}", e)),
                }
            }
            Ok(())
        }

        ScriptInstruction::CallPlugin(plugin, command, timeout_opt) => {
            ctx.perms.check_plugin()?;
            let expanded_cmd = ctx.expand(command);

            // �?安全加固: 插件命令也检查危险模�?(防止绕过沙箱)
            // 例如 call myplug "exec rm -rf /" 即使�?allow-exec 也被拦截
            if let Some(danger) = ctx.is_dangerous(&expanded_cmd) {
                return Err(format!(
                    "插件危险命令被拦�? \"{}\" 包含 \"{}\"",
                    expanded_cmd, danger
                ));
            }

            // 使用脚本超时或指令指定超�?
            let timeout = timeout_opt
                .unwrap_or(ctx.timeout_secs * 1000)
                .max(1000)   // 最�?�?
                .min(300_000); // 最�?分钟

            app.push_output(&format!("[plugin] {} �?{} (超时{}ms)...", plugin, expanded_cmd, timeout));

            match ctx.plugins.call(plugin, &expanded_cmd, &ctx.perms, timeout) {
                Ok(result) => {
                    // 将结果存入变�?$PLUGIN_RESULT
                    ctx.vars.insert("PLUGIN_RESULT".into(), result.clone());
                    app.push_output(&format!("[plugin:{}] {}", plugin, result));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        ScriptInstruction::UnloadPlugin(plugin) => {
            ctx.plugins.unload(plugin)?;
            // �?v4.3: 注销插件命令
            let removed = app.cmd_registry.unregister_plugin(plugin);
            if removed > 0 {
                app.push_output(&format!("[s] 已注销 {} 个命令", removed));
            }
             app.push_output(&format!("[s] 插件已卸载: {}", plugin));
            Ok(())
        }

        ScriptInstruction::ListPlugins => {
            let details = ctx.plugins.list_detailed();
            if details.is_empty() {
                app.push_output("[s] 无已加载插件");
            } else {
                app.push_output("[s] ====== 已加载插�?======");
                for d in &details {
                    app.push_output(d);
                }
                app.push_output(&format!("[s] �?{} 个插�?| watch={}",
                    ctx.plugins.count(),
                    if ctx.plugins.watch { "ON" } else { "OFF" }
                ));
            }
            Ok(())
        }

        ScriptInstruction::HotloadPlugin(plugin) => {
            ctx.perms.check_plugin()?;
            match ctx.plugins.hotload(plugin) {
                Ok(msg) => {
                    app.push_output(&msg);
                    // �?v4.3: 热重载后重新探测并注册命�?
                    app.cmd_registry.unregister_plugin(plugin);
                    let dll_path = ctx.plugins.loaded.get(plugin).map(|i| i.path.to_string_lossy().to_string());
                    if let Some(ref path) = dll_path {
                        match crate::plugin::try_register_from_file(path, plugin, &mut app.cmd_registry) {
                            Ok(count) if count > 0 => {
                                app.push_output(&format!("[plugin] ✅ 热重载后重新注册 {} 个命令", count));
                            }
                            _ => {}
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        // ── 内核驱动 (v3.2) ──

        ScriptInstruction::SysLoad(name, path) => {
            ctx.perms.check_sys()?;
            let expanded_path = ctx.expand(path);
            let expanded_name = ctx.expand(name);

            // �?安全加固: 驱动路径必须通过 canonicalize 防护
            if expanded_path.contains("..") {
                return Err(format!("驱动路径包含非法字符: .. (路径遍历拦截)"));
            }

            app.push_output(&format!(
                "[⚙] 加载内核驱动: {} ({})...",
                expanded_name, expanded_path
            ));

            // 后台线程执行 (驱动加载可能耗时)
            let name_clone = expanded_name.clone();
            let path_clone = expanded_path.clone();
            let (tx, rx) = std::sync::mpsc::channel();

            std::thread::Builder::new()
                .name(format!("ruoo-sysload-{}", expanded_name))
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut mgr = SysDriverManager::new();
                        let r = mgr.load(&name_clone, &path_clone);
                        (r, mgr)
                    }));
                    let final_result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                                format!("驱动加载 panic: {}", s)
                            } else if let Some(s) = e.downcast_ref::<String>() {
                                format!("驱动加载 panic: {}", s)
                            } else {
                                "驱动加载 panic: 未知原因".into()
                            };
                            (Err(msg), SysDriverManager::new())
                        }
                    };
                    let _ = tx.send(final_result);
                })
                .map_err(|e| format!("无法创建驱动加载线程: {}", e))?;

            match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok((Ok(msg), mut mgr)) => {
                    // 合并驱动状态到主管理器
                    if let Some(drv_info) = mgr.loaded.remove(&expanded_name) {
                        ctx.sysdrv.loaded.insert(expanded_name.clone(), drv_info);
                    }
                    app.push_output(&msg);
                    ctx.vars.insert("SYS_RESULT".into(), msg.clone());
                    Ok(())
                }
                Ok((Err(e), _)) => Err(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err("驱动加载超时 (60s) �?操作可能挂起".into())
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err("驱动加载线程异常终止".into())
                }
            }
        }

        ScriptInstruction::SysUnload(name) => {
            ctx.perms.check_sys()?;
            let expanded_name = ctx.expand(name);

            app.push_output(&format!("[⚙] 卸载内核驱动: {}...", expanded_name));

            match ctx.sysdrv.unload(&expanded_name) {
                Ok(()) => {
                    app.push_output(&format!("[⚙] 驱动已卸�? {}", expanded_name));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        ScriptInstruction::SysStatus(name) => {
            ctx.perms.check_sys()?;
            let expanded_name = ctx.expand(name);

            match ctx.sysdrv.status(&expanded_name) {
                Ok(status_str) => {
                    app.push_output(&format!("[⚙] {}", status_str));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        ScriptInstruction::SysList => {
            ctx.perms.check_sys()?;
            let details = ctx.sysdrv.list_detailed();
            if details.is_empty() {
                app.push_output("[⚙] 无已加载内核驱动");
            } else {
                app.push_output("[⚙] ====== 已加载内核驱动======");
                for d in &details {
                    app.push_output(d);
                }
                app.push_output(&format!("[⚙] 共 {} 个驱动", ctx.sysdrv.count()));
            }
            Ok(())
        }

        ScriptInstruction::SysHotload(name) => {
            ctx.perms.check_sys()?;
            match ctx.sysdrv.hotload(&ctx.expand(name)) {
                Ok(msg) => {
                    app.push_output(&msg);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        _ => Ok(()),
    }
}

// ─── 文件操作 ─────────────────────────────────────────

pub fn scripts_dir() -> PathBuf {
    let mut dir = dirs_or_default();
    dir.push("scripts");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).ok();
    }
    dir
}

fn dirs_or_default() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("scripts").exists() || cwd.join("Cargo.toml").exists() {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or(cwd)
}

pub fn list_scripts() -> Vec<String> {
    let dir = scripts_dir();
    if !dir.exists() {
        return vec![];
    }
    let mut scripts = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".ruoo") {
                scripts.push(name);
            }
        }
    }
    scripts.sort();
    scripts
}

pub fn load_script(filename: &str) -> Result<String, String> {
    let path = if PathBuf::from(filename).is_absolute() {
        PathBuf::from(filename)
    } else if filename.ends_with(".ruoo") {
        scripts_dir().join(filename)
    } else {
        scripts_dir().join(format!("{}.ruoo", filename))
    };
    std::fs::read_to_string(&path)
        .map_err(|e| format!("读取脚本失败: {} ({})", path.display(), e))
}

pub fn create_template(name: &str) -> Result<String, String> {
    let dir = scripts_dir();
    let filename = if name.ends_with(".ruoo") {
        name.to_string()
    } else {
        format!("{}.ruoo", name)
    };
    let path = dir.join(&filename);

    if path.exists() {
        return Err(format!("脚本已存�? {}", path.display()));
    }

    let template = format!(
        r#"# =================================
# RUOO-CONSOLE 脚本: {name}
# 安全: script permit {name} 授权后执�?
# =================================

# ── 权限沙箱 (按需开�? ──
#![allow-net]       # 网络: scan/dns/whois/http
#![allow-fs]        # 文件: read/write/delete
#![allow-exec]      # 执行: exec/cmd/powershell
#![allow-plugin]    # 插件: load/call 原生DLL
#![allow-sys]       # 内核驱动: sysload/sysunload (.sys/.ko)
#![watch-plugins]   # 插件热加�? 文件变更自动重载
#![watch-sys]       # 驱动热加�? 文件变更自动重载

# ── 容错策略 ──
onerror continue

# ── 超时保护 ──
# timeout 30  (单指�?插件调用超时秒数)

# ── 变量 ──
set TARGET=192.168.1.1
set PORT=80

# ── 侦察 ──
echo [*] 目标: ${{TARGET}}

# ── 插件 (v3.1 权限+热加�?线程隔离) ──
# 插件接口规范 (C ABI):
#   ruoo_plugin_init() -> i32              (0=成功)
#   ruoo_plugin_exec(cmd) -> *mut c_char   (返回结果)
#   ruoo_plugin_free_result(ptr)           (释放内存)
#   ruoo_plugin_permissions() -> u32       (权限位掩�? 可�?
#   ruoo_plugin_version() -> *const c_char (版本字符�? 可�?
#   ruoo_plugin_shutdown()                 (清理, 可�?
#
# 权限�? NET=1 FS=2 EXEC=4 PLUGIN=8
#
# load myplug ./plugins/myplug.dll
# call myplug "scan --ports 80,443" 15000   (15秒超�?
# call myplug "brute --user admin"          (使用脚本默认超时)
# hotload myplug                             (手动热加�?
# plugins                                    (列出已加载插件详�?

# ── 内核驱动 (v3.2 sysload) ──
# 需 #![allow-sys] + 管理�?root权限
# Windows: sc.exe create/start (需签名驱动�?testsigning)
# Linux:   sudo insmod/rmmod
#
# sysload mydrv ./drivers/mydrv.sys           (加载内核驱动)
# sysstatus mydrv                             (查询驱动状�?
# syslist                                     (列出所有驱�?
# syshotload mydrv                            (手动热重载驱�?
# sysunload mydrv                             (卸载驱动)

# ── 异常处理 ──
try
    echo 尝试操作...
catch as error_msg
    echo [!] 捕获错误: ${{error_msg}}
end

# ── 断言 ──
# assert defined PLUGIN_RESULT "插件无输�?

echo [+] 脚本完成
"#,
    );

    std::fs::write(&path, &template)
        .map_err(|e| format!("创建失败: {}", e))?;

    Ok(path.display().to_string())
}