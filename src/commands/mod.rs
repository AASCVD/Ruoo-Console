// ============================================================
// RUOO-CONSOLE v4.0 — Command Processing & Help
// All tools use real implementations from tools.rs
// Async: heavy commands run on background threads (no UI freeze)
// Progress bar: scan + subdomain show real-time progress
// ============================================================

// ═══ 子模块声明 v8.0 ═══
pub mod help;
pub mod mod_meta;

use crate::App;
use crate::ProgressState;
use crate::tools;
use crate::window;
use std::sync::{Arc, Mutex};

// ── Async command dispatcher (no progress) ──
pub(crate) fn spawn_async_cmd(app: &mut App, label: String, f: impl FnOnce(&mut Vec<String>) + Send + 'static) {
    _spawn(app, label, None, move |out, _prog| f(out));
}

// ── Async command dispatcher (with progress bar) ──
pub(crate) fn spawn_async_cmd_progress(
    app: &mut App,
    label: String,
    progress: Arc<Mutex<ProgressState>>,
    f: impl FnOnce(&mut Vec<String>, &Mutex<ProgressState>) + Send + 'static,
) {
    _spawn(app, label, Some(progress), f);
}

pub(crate) fn _spawn(
    app: &mut App,
    label: String,
    progress: Option<Arc<Mutex<ProgressState>>>,
    f: impl FnOnce(&mut Vec<String>, &Mutex<ProgressState>) + Send + 'static,
) {
    if app.cmd_pending {
        app.push_output(&format!("[!] 命令执行中，请等待 '{}' 完成", label));
        return;
    }
    let tx = match &app.cmd_tx {
        Some(tx) => tx.clone(),
        None => {
            app.push_output("[!] Command channel not initialized");
            return;
        }
    };
    app.cmd_pending = true;
    app.progress = progress.clone();
    let lbl = label.clone();
    app.push_output(&format!("[*](async) {} — running in background...", label));
    std::thread::Builder::new()
        .name(format!("ruoo-cmd-{}", lbl.chars().take(20).collect::<String>()))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut output = Vec::new();
                let dummy = Mutex::new(ProgressState { label: String::new(), current: 0, total: 0, message: String::new() });
                let prog: &Mutex<ProgressState> = if let Some(ref p) = progress { p } else { &dummy };
                f(&mut output, prog);
                output.push(format!("[+] {} — done", lbl));
                output
            }));
            match result {
                Ok(mut output) => {
                    // ★ 排空路径穿越警告 — 防止 println! 破坏 TUI 界面
                    let warnings = crate::tools::files::drain_path_warnings();
                    if !warnings.is_empty() {
                        let done_msg = output.pop(); // 移出 "[+] ... — done"
                        for w in warnings { output.push(w); }
                        if let Some(msg) = done_msg { output.push(msg); }
                    }
                    let _ = tx.send(output);
                }
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        format!("[!] 工具线程 panic: {}", s)
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        format!("[!] 工具线程 panic: {}", s)
                    } else {
                        "[!] 工具线程 panic (未知原因)".to_string()
                    };
                    // ★ v9.4: 记录容错统计
                    crate::tools::fault_tolerant::record_fault(
                        &crate::tools::fault_tolerant::FaultError::ToolPanic {
                            context: lbl.clone(), message: msg.clone()
                        });
                    let _ = tx.send(vec![format!("[!] {} — 异常终止 (核心不受影响)", lbl), msg]);
                }
            }
        })
        .expect("spawn thread");
}
// ═══════════════════════════════════════════════════════
// RUOO-CONSOLE v7.1 — 统一命令注册表
// 唯一真相源: src/commands/mod_meta.rs → all_commands()
// help / tab_complete 均动态读取 mod_meta
// 添加新命令: 在 mod_meta.rs ALL 数组添加 tuple 即可
// ═══════════════════════════════════════════════════════

pub fn help_text_impl(app: &crate::App) -> Vec<String> {
    help::help_text_filtered(app, None)
}

// ── Tab completion (v7.1 动态读取 mod_meta) ──
pub fn tab_complete(input: &str, app: &crate::App) -> Option<String> {
    // 从 mod_meta 动态构建补全列表 (缓存到 lazy_static 避免重复分配)
    use std::sync::LazyLock;
    use std::collections::HashSet;
    static COMPLETIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
        let mut set: HashSet<&str> = HashSet::new();
        for (name, _syntax, _desc, _cat) in crate::commands::mod_meta::all_commands() {
            // 提取主命令名 (取 '/' 或 ' ' 之前的第一个词)
            let base = name.split(|c: char| c == '/' || c == ' ').next().unwrap_or(name).trim();
            if !base.is_empty() {
                set.insert(base);
            }
            // 对于 "cmd / alias" 格式，也添加纯命令名
            let pure = name.split(' ').next().unwrap_or(name).trim();
            if pure != base && !pure.is_empty() && !pure.contains('/') {
                set.insert(pure);
            }
        }
        let mut v: Vec<&str> = set.into_iter().collect();
        v.sort();
        v
    });
    let commands: &[&str] = &COMPLETIONS;

    // ── 动态插件命令 (本地 + AI全局) ──
    let mut plugin_cmds: Vec<String> = app.cmd_registry.command_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(ai_cmds) = crate::ai::with_global_cmd_registry(|reg| reg.command_names().iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
        for cmd in ai_cmds {
            if !plugin_cmds.contains(&cmd) { plugin_cmds.push(cmd); }
        }
    }

    let lower = input.to_lowercase();
    for cmd in commands {
        if cmd.starts_with(&lower) && cmd.len() > lower.len() {
            return Some(cmd.to_string());
        }
    }
    // ── 动态插件命令 ──
    for cmd in &plugin_cmds {
        if cmd.starts_with(&lower) && cmd.len() > lower.len() {
            return Some(cmd.clone());
        }
    }
    None
}

/// v9.0: 移除终端中上一次help输出的行, 实现分页切换时的原地替换
fn remove_help_output(app: &mut App) {
    if app.help_output_len > 0 {
        let start = app.help_output_start;
        let end = start + app.help_output_len;
        if end <= app.output.len() {
            app.output.drain(start..end);
        }
        app.help_output_start = 0;
        app.help_output_len = 0;
    }
}

pub fn process_cmd(app: &mut App, raw: &str) {
    use chrono::Local;

    let parts: Vec<&str> = raw.splitn(3, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg1 = parts.get(1).map(|s| s.to_string());
    let arg2 = parts.get(2).map(|s| s.to_string());
    // v4.4: rest = full text after command word (fixes config set, exec, encode, etc.)
    let rest = raw.trim_start().find(' ').map(|p| &raw.trim_start()[p+1..]);

    // ── ★ 动态命令路由: 先查已注册的插件命令 (本地 + AI全局) ──
    let plugin_def = app.cmd_registry.find(&cmd).cloned()
        .or_else(|| crate::ai::with_global_cmd_registry(|reg| reg.find(&cmd).cloned()).flatten());
    if let Some(ref def) = plugin_def {
        // ★ v4.6: 内置命令跳过 plugin_mgr.call, 直接走 match 路由
        if !def.is_builtin {
            let plugin_name = def.plugin_name.clone();

            // 拼接完整命令: "cmd arg1 arg2..."
            let plugin_cmd = {
                let mut full = cmd.clone();
                for p in parts.iter().skip(1) {
                    full.push(' ');
                    full.push_str(p);
                }
                full
            };

            app.push_output(&format!("[plugin:{}] {} → {}", plugin_name, def.cmd, plugin_cmd));

            // v9.0: 统一全局 PluginManager — TUI和AI共享同一实例
            let perms = crate::script::PermissionSet::all();
            // v7.0: 使用智能调用 — 首次线程模式, 崩溃后自动降级子进程
            let call_result = {
                let mut mgr = app.plugin_mgr.lock().unwrap();
                mgr.call_smart(&plugin_name, &plugin_cmd, &perms, 30_000)
            }; // MutexGuard在此释放
            match call_result {
                Ok(result) => {
                    plugin_push_multiline(app, &plugin_name, &result);
                }
                Err(local_err) => {
                    let msg = local_err.to_lowercase();
                    if msg.contains("unloaded") || msg.contains("not loaded") || msg.contains("not found") {
                        app.push_output(&format!("[!] 插件 '{}' 未加载 — 请先 plugin load {} <路径>", plugin_name, plugin_name));
                        app.push_output("[*] 可用: plugin load <名称> <路径> → 加载并自动注册命令");
                    } else {
                        app.push_output(&format!("[!] 插件调用失败: {}", local_err));
                    }
                }
            }
            return;
        }
    }

    match cmd.as_str() {
        // ── Core ──
        "help" | "?" => {
            // v10.0: help直接输出到终端，help 1~N切换分页(动态页数)，自动替换已输出的help内容
            let page = match arg1.as_deref() {
                Some("all") | Some("full") => 0,
                Some(s) if s.len() <= 2 && s.chars().all(|c| c.is_ascii_digit()) => {
                    let p = s.parse::<usize>().unwrap_or(1);
                    if p < 1 { 1 } else { p }  // v10.0: 动态分页, help_page内部校验上限
                }
                Some(cat) => {
                    // 分类过滤 — 直接输出, 不进入help_mode
                    let filtered = help::help_text_filtered(app, Some(cat));
                    if filtered.is_empty() {
                        let cats = help::list_categories();
                        let suggestions: Vec<_> = cats.iter()
                            .filter(|c| c.to_lowercase().contains(&cat.to_lowercase()))
                            .collect();
                        remove_help_output(app);
                        if suggestions.is_empty() {
                            app.push_output(&format!("[!] 未找到分类 '{}'", cat));
                            app.push_output("[*] 可用分类:");
                            for chunk in cats.chunks(5) {
                                app.push_output(&format!("  {}", chunk.join(" | ")));
                            }
                            app.push_output("[*] 用法: help <分类名>  如: help 信息侦察");
                            let tp = help::total_pages(app);
                            app.push_output(&format!("[*] 分页: help 1 ~ help {}", tp));
                        } else {
                            app.push_output(&format!("[*] 未找到 '{}', 你是否想找:", cat));
                            for s in &suggestions { app.push_output(&format!("  help {}", s)); }
                        }
                        return;
                    }
                    remove_help_output(app);
                    let start = app.output.len();
                    for line in &filtered { app.push_output(line); }
                    app.help_output_start = start;
                    app.help_output_len = filtered.len();
                    let tp = help::total_pages(app);
                    app.push_output(&format!("[*] help {} — 已显示 | help 1~{} 分页 | F9 全览", cat, tp));
                    return;
                }
                None => 1,
            };

            if page == 0 {
                // all/full → 进入独立帮助页面(保留原有F9全览行为)
                let content = help::help_text_filtered(app, None);
                app.help_page_content = content;
                app.help_mode = true;
                app.scroll_offset = 0;
                app.auto_scroll = false;
                app.push_output("[+] 进入帮助全览 — F9/Esc 退出 | PageUp/Down 翻页 | F8 复制全部");
                let help_text = app.help_page_content.join("\n");
                match crate::clipboard::copy_to_clipboard(&help_text) {
                    Ok(msg) => app.push_output(&msg),
                    Err(e) => app.push_output(&format!("[!] {}", e)),
                }
            } else {
                // 分页输出 — 删旧help，末尾追加新help
                let content = help::help_page(page, app);
                remove_help_output(app);
                let start = app.output.len();
                for line in &content { app.push_output(line); }
                app.help_output_start = start;
                app.help_output_len = content.len();
            }
        }
        "clear" | "cls" => {
            let sub = arg1.as_deref().unwrap_or("screen");
            match sub {
                _ => {
                    app.output.clear();
                    app.reset_scroll();
                    app.help_output_start = 0;
                    app.help_output_len = 0;
                    app.push_output("[+] 终端已清空");
                }
            }
        }
        "perm" => {
            match arg1.as_deref() {
                Some(n) => {
                    match n.parse::<i32>() {
                        Ok(level) if (0..=5).contains(&level) => {
                            use crate::ai::AiPermLevel;
                            let perm = AiPermLevel::from_i32(level);
                            if let Some(ref mut session) = app.ai_session {
                                session.perm_level = perm;
                            }
                            app.cfg.deepseek_model = app.cfg.deepseek_model.clone();
                            app.push_output(&format!("[+] 🔐 AI 权限已设置为: {} — {}", perm.label(), perm.description()));
                            if level < 5 {
                                app.push_output(&format!("[!] 已限制 {}，以下工具被阻止:", perm.label()));
                                app.push_output(&format!("     perm 5 (=DANGEROUS) 可恢复全部权限"));
                            }
                            if level == 0 {
                                app.push_output("[*] 仅允许只读操作 — 所有网络/文件/系统操作被禁止");
                            }
                            let _ = crate::config::save_config(&app.cfg);
                        }
                        Ok(n) => {
                            app.push_output(&format!("[!] 无效权限等级: {} — 有效范围: 0-5", n));
                            app.push_output("[*] perm 0=只读 1=网络 2=Web 3=文件 4=系统 5=全部");
                        }
                        Err(_) => {
                            app.push_output(&format!("[!] 无效参数: {} — 请输入数字 0-5", n));
                            app.push_output("[*] perm 0=只读 1=网络 2=Web 3=文件 4=系统 5=全部");
                        }
                    }
                }
                None => {
                    let current = match &app.ai_session {
                        Some(s) => s.perm_level,
                        None => crate::ai::AiPermLevel::Dangerous,
                    };
                    app.push_output("[*] ======== AI 权限系统 ========");
                    app.push_output(&format!("  当前等级: {}", current.label()));
                    app.push_output("");
                    app.push_output("  ┌──────┬─────────────────────────────────────────────────┐");
                    app.push_output("  │ 等级 │ 允许的工具                                       │");
                    app.push_output("  ├──────┼─────────────────────────────────────────────────┤");
                    for lvl in 0..=5i32 {
                        let p = crate::ai::AiPermLevel::from_i32(lvl);
                        let marker = if p == current { " ◀ 当前" } else { "" };
                        app.push_output(&format!("  │  {}   │ {}{}", lvl, p.description(), marker));
                    }
                    app.push_output("  └──────┴─────────────────────────────────────────────────┘");
                    app.push_output("");
                    app.push_output("[*] 用法: perm <0-5>  —  设置 AI 工具权限等级");
                    app.push_output("[!] ⚠ 权限降低后 AI 无法自我提升 — 仅用户可通过 TUI 命令修改");
                    app.push_output("[*] 安全: 此命令不暴露给 AI/脚本/插件/驱动 — 无法被绕过");
                }
            }
        }
        "status" => {
            app.push_output("[*] ========== 系统状态 ==========");
            app.push_output("  主机名     : ruoo-arsenal");
            app.push_output(&format!("  平台       : {}", std::env::consts::OS));
            app.push_output(&format!("  架构       : {}", std::env::consts::ARCH));
            app.push_output(&format!("  核心数     : {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8)));
            app.push_output(&format!("  工作目录   : {}", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "(未知)".into())));
            app.push_output(&format!("  输出行数   : {}", app.output.len()));
            app.push_output(&format!("  历史命令   : {}", app.cmd_history.len()));
            app.push_output(&format!("  时间       : {}", Local::now().format("%Y-%m-%d %H:%M:%S")));
            let ai_perm = match &app.ai_session {
                Some(s) => s.perm_level.label().to_string(),
                None => "— (AI未激活)".to_string(),
            };
            app.push_output(&format!("  AI权限     : {}", ai_perm));
            let task_status = if app.auto_task_mode {
                format!("开启 ({}条待执行)", app.task_queue.len())
            } else {
                "关闭".to_string()
            };
            app.push_output(&format!("  任务流程表 : {}", task_status));
            app.push_output("[+] 状态已刷新");
        }
        "config" => cmd_config(app, &arg1, rest),

    

        // ── Script ──
        "run"    => cmd_run(app, &arg1),
        "script" => cmd_script(app, &arg1, &arg2),

        // ── Plugin ──
        "plugin" => cmd_plugin(app, &arg1, &arg2),

        // ── SysLoad ──
        "sysload"   => cmd_sysload(app, &arg1, &arg2),
        "sysunload" => cmd_sysunload(app, &arg1),
        "syslist"   => cmd_syslist(app),
        "kinfo"     => cmd_kinfo(app),
        "kcompile"  => cmd_kcompile(app, &arg1, &arg2),
        "ktemplate" => cmd_ktemplate(app, &arg1, &arg2),
        "kvalidate" => cmd_kvalidate(app, &arg1),
        "kscaffold" => {
            let plat = rest.as_ref().map(|r| r.to_string());
            cmd_kscaffold(app, &arg1, &arg2, &plat);
        }

        // ── System ──
        "sysinfo" => cmd_sysinfo(app),
        "netstat" => cmd_netstat(app),
        "exec"    => cmd_exec(app, rest),
        "edit"    => cmd_edit(app, &arg1),
        "hexedit" => cmd_hexedit(app, &arg1, &arg2, rest),
        "save"    => cmd_save(app, &arg1),
        "dpai"    => cmd_dpai(app),
        "vault"   => cmd_vault(app, &arg1, &arg2),
        "rustc" | "compile-rust" => cmd_rustc(app, &arg1, &arg2, rest),
        "compile" => {
            // 通用编译器 — 根据第一个参数(语言)分发
            match arg1.as_deref() {
                Some("rust") | Some("rs") => cmd_rustc(app, &arg2, &None, rest),
                Some("c") => cmd_cc(app, &arg2, &None, rest),
                Some("cpp") | Some("c++") | Some("cxx") => cmd_compile_cpp(app, &arg2, &None, rest),
                Some("go") => cmd_compile_go(app, &arg2, &None, rest),
                Some("java") => cmd_compile_java(app, &arg2, &None, rest),
                Some("cs") | Some("c#") | Some("csharp") => cmd_compile_cs(app, &arg2, &None, rest),
                Some("python") | Some("py") => cmd_compile_python(app, &arg2, &None, rest),
                Some("asm") | Some("nasm") => cmd_compile_asm(app, &arg2, &None, rest),
                Some("zig") => cmd_compile_zig(app, &arg2, &None, rest),
                Some(other) => {
                    app.push_output(&format!("[!] 未知语言: {} — 支持: rust/c/cpp/go/java/cs/python/asm/zig", other));
                    app.push_output("[*] 查看: compilers — 列出所有可用编译器");
                }
                None => {
                    app.push_output("[*] compile — 通用编译器分发:");
                    app.push_output("  compile rust <源> [--release]     → Rust");
                    app.push_output("  compile c <源> [-O2]              → C");
                    app.push_output("  compile cpp <源> [-O2]            → C++");
                    app.push_output("  compile go <源> [-O]              → Go");
                    app.push_output("  compile java <源>                 → Java");
                    app.push_output("  compile cs <源> [-O]              → C#");
                    app.push_output("  compile python <源>               → Python");
                    app.push_output("  compile asm <源>                  → NASM");
                    app.push_output("  compile zig <源> [-O]             → Zig");
                    app.push_output("[*] 也可直接用: compile-rust / compile-c / … / compile-zig");
                    app.push_output("[*] 查看: compilers — 列出所有可用编译器");
                }
            }
        }
        "cc" | "gcc" | "clang" | "compile-c" => cmd_cc(app, &arg1, &arg2, rest),
        "compile-cpp" | "cxx" | "cppc" => cmd_compile_cpp(app, &arg1, &arg2, rest),
        "compile-go" | "goc" => cmd_compile_go(app, &arg1, &arg2, rest),
        "compile-java" | "javac" => cmd_compile_java(app, &arg1, &arg2, rest),
        "compile-cs" | "csc" | "dotnet-build" => cmd_compile_cs(app, &arg1, &arg2, rest),
        "compile-python" | "pyc" => cmd_compile_python(app, &arg1, &arg2, rest),
        "compile-asm" | "nasm" => cmd_compile_asm(app, &arg1, &arg2, rest),
        "compile-zig" | "zigc" => cmd_compile_zig(app, &arg1, &arg2, rest),
        "compilers" => cmd_compilers(app),
        "lock"    => cmd_lock(app),
        "unlock"  => cmd_unlock(app),
        "bootscript" => cmd_bootscript(app, &arg1),
        "hide" => {
            if app.window_hidden {
                app.push_output("[!] 窗口已在后台隐藏中");
            } else {
                window::hide_console();
                app.window_hidden = true;
                app.push_output("[s] 窗口已隐藏到后台 — 任务继续运行");
                app.push_output("[*] Ctrl+Shift+S 可从桌面恢复显示 | 输入 show 恢复");
            }
        }
        "show" => {
            if app.window_hidden {
                window::show_console();
                app.window_hidden = false;
                app.push_output("[s] 窗口已恢复显示");
            } else {
                app.push_output("[·] 窗口已是可见状态");
            }
        }

        "abort" | "abort_tool" => {
            let msg = crate::ai::trigger_tool_abort();
            app.push_output(&msg);
        }
        // ═══ v6.1 新增命令调度 (corrected signatures) ═══
        // ── 系统信息 ──
        "ifconfig" => { spawn_async_cmd(app, "网络接口信息".into(), |out| { out.extend(tools::network_interfaces()); }); }
        "df" => { spawn_async_cmd(app, "磁盘使用".into(), |out| { out.extend(tools::disk_usage()); }); }
        // ── 网络搜索 v1.0 ──
        // ── Web直接请求 ──
        "httpget" => {
            if let Some(ref url) = arg1 {
                let u = url.clone();
                spawn_async_cmd(app, format!("HTTP GET {}", u), move |out| {
                    match tools::http_get(&u) {
                        Ok((status, headers, body)) => {
                            out.push(format!("[+] HTTP {} — {} bytes", status, body.len()));
                            for line in headers.lines() { out.push(format!("  {}", line)); }
                            out.push(String::new());
                            for line in body.lines().take(200) { out.push(format!("  {}", line)); }
                        }
                        Err(e) => out.push(format!("[!] {}", e))
                    }
                });
            } else { app.push_output("[!] 用法: httpget <URL>"); }
        }
        "httppost" => {
            if let (Some(url), Some(body)) = (arg1.clone(), arg2.clone()) {
                let u = url; let b = body;
                spawn_async_cmd(app, format!("HTTP POST {}", u), move |out| {
                    match tools::http_post(&u, &b, "application/json") {
                        Ok((status, headers, resp_body)) => {
                            out.push(format!("[+] HTTP {} — {} bytes", status, resp_body.len()));
                            for line in headers.lines() { out.push(format!("  {}", line)); }
                            out.push(String::new());
                            for line in resp_body.lines().take(200) { out.push(format!("  {}", line)); }
                        }
                        Err(e) => out.push(format!("[!] {}", e))
                    }
                });
            } else { app.push_output("[!] 用法: httppost <URL> <body>"); }
        }
        "httpget-hdr" | "httpgeth" => { app.push_output("[*] httpget-hdr — 通过AI代理: &httpget-hdr <URL> <headers_JSON>"); }
        "httppost-hdr" | "httpposth" => { app.push_output("[*] httppost-hdr — 通过AI代理: &httppost-hdr <URL> <body> <headers_JSON>"); }
        // ── 服务探测 v6.1 ──
        // ── 高级扫描 v6.1 ──
        // ── APK/Java v7.2 ──
        // ── 二进制逆向 v6.1 ──
        "mime" => { if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("MIME检测 {}", file), move |out| { match tools::mime_detect(&file) { Ok(s) => out.push(format!("[+] {}", s)), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: mime <文件>"); } }
        "copyb64" => { if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("Base64剪贴板 {}", file), move |out| { match tools::base64_file(&file, "encode") { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: copyb64 <文件>"); } }
        "pasteb64" => { if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("Base64解码 {}", file), move |out| { match tools::base64_file(&file, "decode") { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: pasteb64 <文件>"); } }
        // ── Vault扩展 v6.1 ═─
        // ── AI存储 v6.1 ═─
        "aisave" => { app.push_output("[*] aisave — 通过AI代理: &aisave <文件名> <内容>"); }
        "airead" => { app.push_output("[*] airead — 通过AI代理: &airead <文件名>"); }
        "ailist" => { app.push_output("[*] ailist — 通过AI代理: &ailist"); }
        // ═══ v10.0 文件操作补全 (35条 — 此前仅在mod_meta注册, 未在match实现) ═══
        "base64" => {
            if let (Some(ref f), Some(ref op)) = (arg1.clone(), arg2.clone()) {
                let file = f.clone(); let op_str = op.clone();
                spawn_async_cmd(app, format!("Base64 {}", file), move |out| {
                    match tools::base64_file(&file, &op_str) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) }
                });
            } else { app.push_output("[!] 用法: base64 <文件> encode|decode"); }
        }
        "encdetect" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("编码检测 {}", file), move |out| { match tools::file_encoding_detect(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: encdetect <文件>"); }
        }
        "jq" => {
            if let (Some(ref f), Some(ref q)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let query = q.clone(); spawn_async_cmd(app, format!("JSON查询 {}", file), move |out| { match tools::json_query(&file, &query) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: jq <JSON文件> <查询>"); }
        }
        "jkeys" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("JSON键列表 {}", file), move |out| { match tools::json_keys(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: jkeys <JSON文件>"); }
        }
        "csv" => {
            if let Some(ref f) = arg1 {
                let file = f.clone();
                let max_rows = arg2.as_deref().and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);
                let delim = rest.map(|r| r.split_whitespace().last().unwrap_or(",")).unwrap_or(",").to_string();
                spawn_async_cmd(app, format!("CSV解析 {}", file), move |out| { match tools::csv_parse(&file, max_rows, &delim) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } });
            } else { app.push_output("[!] 用法: csv <文件> [行数] [分隔符]"); }
        }
        "rename" => {
            if let (Some(ref dir), Some(ref find)) = (arg1.clone(), arg2.clone()) {
                let d = dir.clone(); let f_pat = find.clone();
                let repl = rest.map(|r| r.split_whitespace().last().unwrap_or("")).unwrap_or("").to_string();
                let dry = !rest.map(|r| r.contains("--real")).unwrap_or(false);
                spawn_async_cmd(app, format!("批量重命名 {}", d), move |out| { match tools::batch_rename(&d, &f_pat, &repl, dry) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } });
            } else { app.push_output("[!] 用法: rename <目录> <正则> <替换> [--real]"); }
        }
        "tempfile" => {
            let prefix = arg1.as_deref().unwrap_or("ruoo_").to_string();
            let suffix = arg2.as_deref().unwrap_or(".tmp").to_string();
            let content = rest.map(|r| r.trim().to_string()).filter(|s| !s.is_empty());
            spawn_async_cmd(app, "创建临时文件".into(), move |out| { match tools::temp_file(&prefix, &suffix, content.as_deref()) { Ok(path) => out.push(path), Err(e) => out.push(format!("[!] {}", e)) } });
        }
        "syminfo" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("符号链接信息 {}", file), move |out| { match tools::symlink_info(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: syminfo <路径>"); }
        }
        "slice" => {
            if let (Some(ref f), Some(ref start)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let s_off: usize = start.parse().unwrap_or(0); let e_off = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(s_off + 1024); let out = String::new(); spawn_async_cmd(app, format!("文件切片 {}", file), move |out2| { match tools::file_slice(&file, s_off, e_off, &out) { Ok(s) => out2.push(s), Err(e) => out2.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: slice <文件> <起始> <结束> [输出]"); }
        }
        "sample" => {
            if let (Some(ref f), Some(ref n)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let cnt: usize = n.parse().unwrap_or(10); let seed = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(0u64); spawn_async_cmd(app, format!("行采样 {}", file), move |out| { match tools::lines_sample(&file, cnt, seed) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: sample <文件> <N> [种子]"); }
        }
        "empty" => {
            if let Some(ref d) = arg1 { let dir = d.clone(); let rec = !rest.map(|r| r.contains("--flat")).unwrap_or(false); spawn_async_cmd(app, format!("空文件查找 {}", dir), move |out| { match tools::empty_files(&dir, rec) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: empty <目录> [--flat]"); }
        }
        "concat" => {
            if let Some(ref files) = arg1 { let f = files.clone(); let out = arg2.as_deref().unwrap_or("concat_output.txt").to_string(); let sep = rest.map(|r| r.contains("--sep")).unwrap_or(false); spawn_async_cmd(app, format!("拼接文件"), move |out2| { match tools::file_concat(&f, &out, sep) { Ok(s) => out2.push(s), Err(e) => out2.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: concat <文件1,文件2,...> [输出] [--sep]"); }
        }
        "dusort" => {
            if let Some(ref d) = arg1 { let dir = d.clone(); let depth = arg2.as_deref().and_then(|s| s.parse().ok()).unwrap_or(1usize); let top_n = 20usize; spawn_async_cmd(app, format!("目录排序 {}", dir), move |out| { match tools::du_sorted(&dir, depth, top_n) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: dusort <目录> [深度] [前N]"); }
        }
        "copytext" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let max_kb = arg2.as_deref().and_then(|s| s.parse().ok()).unwrap_or(1024usize); spawn_async_cmd(app, format!("复制内容 {}", file), move |out| { match tools::file_copy_content(&file, max_kb) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: copytext <文件> [最大KB]"); }
        }
        "pastetext" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let append = arg2.as_deref().map(|s| s == "--append").unwrap_or(false); spawn_async_cmd(app, format!("粘贴到 {}", file), move |out| { match tools::file_paste_content(&file, append) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: pastetext <文件> [--append]"); }
        }
        "copypath" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("复制路径 {}", file), move |out| { match tools::file_copy_path(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: copypath <文件>"); }
        }
        "filecut" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("剪切 {}", file), move |out| { match tools::file_cut(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: filecut <文件>"); }
        }
        "filepaste" => {
            if let Some(ref d) = arg1 { let dir = d.clone(); spawn_async_cmd(app, format!("粘贴剪切"), move |out| { match tools::file_paste_cut(&dir) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: filepaste <目标目录>"); }
        }
        "cutstatus" => { app.push_output(&tools::file_cut_status()); }
        "copymulti" => {
            if let Some(ref paths) = arg1 { let p = paths.clone(); spawn_async_cmd(app, "批量复制路径".into(), move |out| { match tools::file_copy_multi(&p) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: copymulti <文件1,文件2,...>"); }
        }
        "hexdump" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let max_bytes = arg2.as_deref().and_then(|s| s.parse().ok()).unwrap_or(1024usize); let offset = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(0usize); spawn_async_cmd(app, format!("HEX转储 {}", file), move |out| { match tools::hex_dump(&file, max_bytes, offset) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: hexdump <文件> [字节数] [偏移]"); }
        }
        "shred" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let passes = arg2.as_deref().and_then(|s| s.parse().ok()).unwrap_or(3usize); let pat = rest.map(|r| r.trim()).unwrap_or("zero").to_string(); spawn_async_cmd(app, format!("安全删除 {}", file), move |out| { match tools::secure_delete(&file, passes, &pat) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: shred <文件> [遍数] [zero|random|dod]"); }
        }
        "touch" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("touch {}", file), move |out| { match tools::touch(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: touch <文件>"); }
        }
        "filetime" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let created = arg2.as_deref().map(|s| s.to_string()); let modified = rest.map(|r| r.split_whitespace().next().unwrap_or("").to_string()).filter(|s| !s.is_empty()); let accessed = rest.and_then(|r| r.split_whitespace().nth(1).map(|s| s.to_string())); spawn_async_cmd(app, format!("时间戳 {}", file), move |out| { match tools::set_file_times(&file, created.as_deref(), modified.as_deref(), accessed.as_deref()) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: filetime <文件> [创建时间] [修改时间] [访问时间]"); }
        }
        "adsread" => {
            if let (Some(ref f), Some(ref s)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let stream = s.clone(); spawn_async_cmd(app, format!("ADS读取 {}", file), move |out| { match tools::ads_read(&file, &stream) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: adsread <文件> <流名>"); }
        }
        "adswrite" => {
            if let (Some(ref f), Some(ref s)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let stream = s.clone(); let data = rest.unwrap_or("").to_string(); spawn_async_cmd(app, format!("ADS写入 {}", file), move |out| { match tools::ads_write(&file, &stream, &data) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: adswrite <文件> <流名> <数据>"); }
        }
        "adslist" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("ADS列表 {}", file), move |out| { match tools::ads_list(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: adslist <文件>"); }
        }
        "dirdiff" => {
            if let (Some(ref a), Some(ref b)) = (arg1.clone(), arg2.clone()) { let d1 = a.clone(); let d2 = b.clone(); spawn_async_cmd(app, format!("目录差异 {} vs {}", d1, d2), move |out| { match tools::dir_diff(&d1, &d2) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: dirdiff <目录1> <目录2>"); }
        }
        "hexfind" => {
            if let (Some(ref f), Some(ref pat)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let pattern = pat.clone(); let max_res = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(50usize); spawn_async_cmd(app, format!("二进制搜索 {}", file), move |out| { match tools::binary_search(&file, &pattern, max_res) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: hexfind <文件> <HEX模式> [最大结果]"); }
        }
        "health" => { let lines = tools::health_check(); for l in lines { app.push_output(&l); } }
        "regexsearch" => {
            if let (Some(ref f), Some(ref pat)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let pattern = pat.clone(); let ci = rest.map(|r| r.contains("-i")).unwrap_or(false); let max_m = rest.map(|r| r.split_whitespace().filter_map(|s| s.parse().ok()).next()).unwrap_or(Some(100usize)).unwrap_or(100); let ctx = rest.map(|r| r.split_whitespace().filter_map(|s| s.parse().ok()).nth(1)).unwrap_or(Some(0usize)).unwrap_or(0); spawn_async_cmd(app, format!("正则搜索 {}", file), move |out| { match tools::regex_search(&file, &pattern, ci, max_m, ctx) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: regexsearch <文件> <正则> [-i] [最大匹配] [上下文]"); }
        }
        "byterep" => {
            if let (Some(ref f), Some(ref off)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let offset: u64 = off.parse().unwrap_or(0); let hex = rest.unwrap_or("").to_string(); spawn_async_cmd(app, format!("字节替换 {}", file), move |out| { match tools::byte_range_replace(&file, offset, &hex) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: byterep <文件> <偏移> <HEX字节>"); }
        }
        "multirep" | "smi" => {
            if let (Some(ref f), Some(ref pairs)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let p = pairs.clone(); let ci = rest.map(|r| r.contains("-i")).unwrap_or(false); let max_r = rest.map(|r| r.split_whitespace().filter_map(|s| s.parse().ok()).next()).unwrap_or(Some(10000usize)).unwrap_or(10000); spawn_async_cmd(app, format!("多模式替换 {}", file), move |out| { match tools::stream_multi_replace(&file, &p, ci, max_r) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: multirep <文件> <JSON对> [-i] [最大替换]"); }
        }
        "searchexport" => {
            if let (Some(ref f), Some(ref pat)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let pattern = pat.clone(); let out = rest.map(|r| r.split_whitespace().next().unwrap_or("search_export.txt")).unwrap_or("search_export.txt").to_string(); let ci = rest.map(|r| r.contains("-i")).unwrap_or(false); let ctx = 0usize; spawn_async_cmd(app, format!("搜索导出 {}", file), move |out2| { match tools::search_export(&file, &pattern, &out, ci, ctx) { Ok(s) => out2.push(s), Err(e) => out2.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: searchexport <文件> <模式> <输出> [-i]"); }
        }
        "colex" => {
            if let (Some(ref f), Some(ref cols)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let columns = cols.clone(); let delim = rest.map(|r| r.split_whitespace().next().unwrap_or("comma")).unwrap_or("comma").to_string(); let max_r = 500usize; spawn_async_cmd(app, format!("列提取 {}", file), move |out| { match tools::column_extract(&file, &columns, &delim, max_r) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: colex <文件> <列索引> [分隔符] [最大行]"); }
        }
        "bindiff" | "binarydiff" => {
            if let (Some(ref a), Some(ref b)) = (arg1.clone(), arg2.clone()) { let f1 = a.clone(); let f2 = b.clone(); let max_d = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(50usize); spawn_async_cmd(app, format!("二进制差异"), move |out| { match tools::binary_diff(&f1, &f2, max_d) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: bindiff <文件1> <文件2> [最大差异]"); }
        }
        "chunkscan" => {
            if let Some(ref f) = arg1 { let file = f.clone(); let chunk_mb = arg2.as_deref().and_then(|s| s.parse().ok()).unwrap_or(64usize); let max_c = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(50usize); spawn_async_cmd(app, format!("分块扫描 {}", file), move |out| { match tools::file_chunk_scan(&file, chunk_mb, max_c) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: chunkscan <文件> [块大小MB] [最大块数]"); }
        }
        // ═══ v10.0 大文件流式操作 (此前仅在AI工具可用, 现TUI可直接调用) ═══
        "streamgrep" => {
            if let (Some(ref f), Some(ref pat)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let pattern = pat.clone(); let ci = rest.map(|r| r.contains("-i")).unwrap_or(false); let max_m = 100usize; let ctx = 0usize; spawn_async_cmd(app, format!("流式搜索 {}", file), move |out| { match tools::stream_grep(&file, &pattern, ci, max_m, ctx) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: streamgrep <文件> <模式> [-i]"); }
        }
        "streambin" => {
            if let (Some(ref f), Some(ref pat)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let pattern = pat.clone(); let max_r = 50usize; spawn_async_cmd(app, format!("流式二进制扫描 {}", file), move |out| { match tools::stream_binary_scan(&file, &pattern, max_r) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: streambin <文件> <HEX模式>"); }
        }
        "ranged" => {
            if let (Some(ref f), Some(ref off)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let offset: u64 = off.parse().unwrap_or(0); let length = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(4096usize); spawn_async_cmd(app, format!("范围读取 {}", file), move |out| { match tools::range_read(&file, offset, length) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: ranged <文件> <偏移> [长度]"); }
        }
        "sfr" => {
            if let (Some(ref f), Some(ref find), Some(ref rep)) = (arg1.clone(), arg2.clone(), rest.map(|r| r.split_whitespace().next().unwrap_or(""))) { let file = f.clone(); let f_str = find.clone(); let r_str = rep.to_string(); if f_str.is_empty() || r_str.is_empty() { app.push_output("[!] 用法: sfr <文件> <查找> <替换> [-i]"); } else { let ci = rest.map(|r| r.contains("-i")).unwrap_or(false); spawn_async_cmd(app, format!("流式替换 {}", file), move |out| { match tools::stream_find_replace(&file, &f_str, &r_str, ci, 10000) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } } else { app.push_output("[!] 用法: sfr <文件> <查找> <替换> [-i]"); }
        }
        "bldidx" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("构建索引 {}", file), move |out| { match tools::build_line_index(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: bldidx <文件>"); }
        }
        "seekline" => {
            if let (Some(ref f), Some(ref start)) = (arg1.clone(), arg2.clone()) { let file = f.clone(); let ls: u64 = start.parse().unwrap_or(1); let le = rest.and_then(|r| r.split_whitespace().next()?.parse().ok()); spawn_async_cmd(app, format!("索引定位 {}", file), move |out| { match tools::seek_line(&file, ls, le) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: seekline <文件> <起始行> [结束行]"); }
        }
        "qidx" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("索引查询 {}", file), move |out| { match tools::query_line_index(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: qidx <文件>"); }
        }
        "sfinfo" => {
            if let Some(ref f) = arg1 { let file = f.clone(); spawn_async_cmd(app, format!("文件统计 {}", file), move |out| { match tools::stream_file_info(&file) { Ok(s) => out.push(s), Err(e) => out.push(format!("[!] {}", e)) } }); } else { app.push_output("[!] 用法: sfinfo <文件>"); }
        }
        // ═══ v10.1 后台进程管理 (exec_bg/exec_list/exec_kill/exec_send) ═══
        "exec_bg" => {
            if let Some(cmd) = rest {
                if let Err(e) = crate::ai::validate_shell_command(cmd) {
                    app.push_output(&format!("[!] 命令被安全策略拦截: {}", e));
                    return;
                }
                let c = cmd.to_string();
                let shell = arg1.as_deref().unwrap_or("cmd").to_lowercase();
                cmd_exec_bg(app, &c, &shell);
            } else { app.push_output("[!] 用法: exec_bg <命令> [shell]\n[*] 例: exec_bg \"python -m http.server 8080\"\n[*] 查看: exec_list | 终止: exec_kill <PID> | stdin: exec_send <PID> <文本>"); }
        }
        "exec_list" => { cmd_exec_list(app); }
        "exec_kill" => {
            if let Some(pid_str) = arg1.as_deref() {
                match pid_str.parse::<u32>() {
                    Ok(pid) => cmd_exec_kill(app, pid),
                    Err(_) => app.push_output(&format!("[!] 无效PID: {}", pid_str)),
                }
            } else { app.push_output("[!] 用法: exec_kill <PID>"); }
        }
        "exec_send" => {
            if let (Some(pid_str), Some(input)) = (arg1.as_ref(), rest) {
                match pid_str.parse::<u32>() {
                    Ok(pid) => cmd_exec_send(app, pid, input),
                    Err(_) => app.push_output(&format!("[!] 无效PID: {}", pid_str)),
                }
            } else { app.push_output("[!] 用法: exec_send <PID> <输入文本>"); }
        }
        _ => {
            // v7.1: 未匹配命令 → 自动转发到 AI (若已激活)
            if app.ai_mode && app.ai_session.is_some() && !app.ai_pending {
                let ai_query = format!("&{}", raw.trim());
                app.push_output(&format!("[*] 自动转发到 AI: {}", ai_query));
                process_ai_query(app, raw.trim(), false);
            } else if app.ai_mode && app.ai_pending {
                app.push_output(&format!("[!] AI 正忙 — 请等待当前请求完成后再试: {}", cmd));
            } else {
                app.push_output(&format!("[!] 未知命令: {} — 输入 help 查看帮助", cmd));
                app.push_output("[*] 提示: 输入 dpai 激活 AI 助手后可自动执行任意命令");
                let suggestions = crate::utils::fuzzy_match(cmd.as_str());
                if !suggestions.is_empty() {
                    app.push_output(&format!("[*] 你是否想输入: {} ?", suggestions.join(", ")));
                }
            }
        }
    }
}

// ═══════════════════════════════════════════
// Core commands
// ═══════════════════════════════════════════

fn cmd_config(app: &mut App, arg1: &Option<String>, rest: Option<&str>) {
    if let Some(key) = arg1 {
        if key == "save" {
            app.cfg.target = if app.target == "—" { String::new() } else { app.target.clone() };
            app.cfg.history = app.cmd_history.clone();
            app.cfg.show_sidebar = app.show_sidebar;
            match crate::config::save_config(&app.cfg) {
                Ok(_) => app.push_output("[+] 配置已保存到 arsenal_config.json"),
                Err(e) => app.push_output(&format!("[!] 保存失败: {}", e)),
            }
        } else if key == "reload" {
            app.cfg = crate::config::load_config();
            app.push_output("[+] 配置已重新加载");
        } else if key == "set" {
            // rest = "set deepseek_api_key=sk-xxx" — strip "set " prefix
            let set_rest = rest.and_then(|r| {
                if r.len() > 4 && r[..4].eq_ignore_ascii_case("set ") {
                    Some(&r[4..])
                } else if r == "set" {
                    None
                } else {
                    Some(r)
                }
            });
            if let Some(kv) = set_rest {
                if let Some((k, v)) = kv.split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    match app.cfg.set(k, v) {
                        Ok(msg) => {
                            app.push_output(&format!("[+] {}", msg));
                            crate::config::save_config(&app.cfg).ok();
                            // ★ 用户设置 API Key 后展示安全警告（不破坏 TUI）
                            if k == "deepseek_api_key" && crate::config::has_plaintext_api_key(&app.cfg) {
                                app.push_output(crate::config::api_key_plaintext_warning());
                                app.push_output("[*] 建议: vault add api_keys deepseek <key> → 迁移到加密 Vault");
                            }
                        }
                        Err(e) => app.push_output(&format!("[!] {}", e)),
                    }
                } else { app.push_output("[!] 格式: config set <键>=<值>"); }
            } else { app.push_output("[!] 用法: config set <键>=<值>  例: config set theme.accent=#FF0000"); }
        } else { app.push_output(&format!("[!] 未知子命令: {}", key)); }
    } else {
        app.push_output("[*] ========== 配置面板 ==========");
        for line in app.cfg.display() { app.push_output(&line); }
        app.push_output("  修改: config set <键>=<值>  |  config save  |  config reload");
    }
}

fn cmd_dpai(app: &mut App) {
    // ★ v4.1.1: 先检查 ai_enabled 总开关
    if !app.cfg.ai_enabled {
        app.push_output("[!] AI 助手已被禁用 (ai_enabled=false)");
        app.push_output("[*] 请执行: config set ai_enabled=true");
        return;
    }
    // ★ 使用 resolve_api_key_with_source 获取 Key + 来源追踪
    let (api_key, key_source) = app.cfg.resolve_api_key_with_source(app.vault.as_deref());
    if api_key.is_empty() {
        app.push_output("[!] DeepSeek API Key 未配置");
        app.push_output("[*] 请执行: config set deepseek_api_key=sk-xxx");
        app.push_output("[*] 或者: vault set api_keys deepseek <key> (加密存储)");
        return;
    }
    if app.ai_pending { app.push_output("[!] AI 请求进行中，请等待"); return; }
    if app.cmd_pending { app.push_output("[!] 命令进行中，请等待"); return; }
    app.ai_mode = !app.ai_mode;
    if app.ai_mode {
        let session = crate::ai::AiSession::new(Some(&app.cfg.deepseek_system_prompt));
        let tool_count = session.tool_count();
        app.ai_session = Some(session);
        app.push_output("[+] ========== #dp DeepSeek AI 已激活 ==========");
        app.push_output("[+] &<消息> 普通AI对话 | &%<消息> 实时流式输出");
        app.push_output(&format!("[+] 模型: {} | 工具数: {}", app.cfg.deepseek_model, tool_count));
        // ★ 报告 Key 来源，让用户知道 Key 从哪里取到的
        match key_source {
            crate::config::KeySource::VaultApiKeys => {
                app.push_output("[*] Key 来源: Vault 加密存储 (api_keys/deepseek)");
            }
            crate::config::KeySource::VaultDefaultBackCompat => {
                app.push_output("[!] Key 来源: Vault 旧格式 (default/api_keys) — 建议迁移:");
                app.push_output("[*]   执行: vault set api_keys deepseek <key>  后删除旧条目");
            }
            crate::config::KeySource::ConfigPlaintext => {
                app.push_output(crate::config::api_key_plaintext_warning());
                app.push_output("[*] 建议: vault add api_keys deepseek <key> → 迁移到加密 Vault");
            }
            crate::config::KeySource::None => {} // unreachable due to is_empty check above
        }
    } else {
        app.ai_session = None;
        app.push_output("[+] DeepSeek AI 已关闭");
    }
}

// Crypto commands — real crypto
// ═══════════════════════════════════════════

// ═══════════════════════════════════════════
// System commands
// ═══════════════════════════════════════════

fn cmd_sysinfo(app: &mut App) {
    let progress = Arc::new(Mutex::new(ProgressState {
        label: "System Info".into(),
        current: 0, total: 2,
        message: "Collecting system data...".into(),
    }));
    spawn_async_cmd_progress(app, "System Info".to_string(), progress.clone(), move |out, prog| {
        out.push("[*] ========== 详细系统信息 ==========".into());
        { let mut p = prog.lock().unwrap(); p.current = 1; p.message = "正在收集系统数据...".into(); }
        for line in tools::system_info() { out.push(line); }
        out.push(format!("  时间       : {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")));
        out.push("[+] 系统信息获取完成".into());
        { let mut p = prog.lock().unwrap(); p.current = 2; p.message = "Done".into(); }
    });
}

fn cmd_netstat(app: &mut App) {
    let progress = Arc::new(Mutex::new(ProgressState {
        label: "Netstat".into(),
        current: 0, total: 2,
        message: "Running netstat...".into(),
    }));
    spawn_async_cmd_progress(app, "Netstat".to_string(), progress.clone(), move |out, prog| {
        out.push("[*] ======== 网络连接 ========".into());
        { let mut p = prog.lock().unwrap(); p.current = 1; p.message = "正在执行 netstat -ano...".into(); }
        #[cfg(windows)]
        {
            if let Ok(output) = std::process::Command::new("netstat")
                .args(["-ano"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                let show = if lines.len() > 50 { &lines[..50] } else { &lines[..] };
                for line in show {
                    if !line.trim().is_empty() { out.push(format!("  {}", line)); }
                }
                if lines.len() > 50 {
                    out.push(format!("  ... (共 {} 行，仅显示前50行)", lines.len()));
                }
            } else {
                out.push("  [!] netstat 命令不可用".into());
            }
        }
        #[cfg(not(windows))]
        {
            let (tool, args): (&str, &[&str]) = if std::process::Command::new("ss").arg("-tunap").output().is_ok() {
                ("ss", &["-tunap"] as &[&str])
            } else {
                ("netstat", &["-tunap"] as &[&str])
            };
            if let Ok(output) = std::process::Command::new(tool).args(args).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().take(50) {
                    if !line.trim().is_empty() { out.push(format!("  {}", line)); }
                }
            } else {
                out.push("  [!] ss/netstat 命令不可用 (尝试安装 iproute2)".into());
            }
        }
        out.push("[+] 完成".into());
        { let mut p = prog.lock().unwrap(); p.current = 2; p.message = "Done".into(); }
    });
}

fn cmd_exec(app: &mut App, rest: Option<&str>) {
    if let Some(cmd) = rest {
        // ★ BUG-2 修复: TUI命令注入防护
        if let Err(e) = crate::ai::validate_shell_command(cmd) {
            app.push_output(&format!("[!] 命令被安全策略拦截: {}", e));
            app.push_output("[*] 提示: 请使用 exec_safe 模式或脚本执行复杂命令");
            return;
        }

        let c = cmd.to_string();
        let progress = Arc::new(Mutex::new(ProgressState {
            label: format!("Exec {}", c),
            current: 0, total: 2,
            message: "Running command...".into(),
        }));
        spawn_async_cmd_progress(app, format!("Exec {}", c), progress.clone(), move |out, prog| {
            out.push(format!("[*] 执行命令: {}", c));
            { let mut p = prog.lock().unwrap(); p.current = 1; p.message = "等待输出...".into(); }
            #[cfg(windows)]
            let output = std::process::Command::new("cmd").args(["/C", &c]).output();
            #[cfg(not(windows))]
            let output = std::process::Command::new("sh").args(["-c", &c]).output();
            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    if !stdout.is_empty() { for l in stdout.lines() { out.push(l.into()); } }
                    if !stderr.is_empty() { for l in stderr.lines() { out.push(format!("[!] {}", l)); } }
                    out.push(format!("[+] 退出码: {}", o.status.code().unwrap_or(-1)));
                }
                Err(e) => out.push(format!("[!] 执行失败: {}", e)),
            }
            { let mut p = prog.lock().unwrap(); p.current = 2; p.message = "Done".into(); }
        });
    } else { app.push_output("[!] 用法: exec <命令>"); }
}

// ═══ v10.1 后台进程管理 ═══

fn cmd_exec_bg(app: &mut App, command: &str, shell: &str) {
    let ts_display = chrono::Local::now().format("%H:%M:%S").to_string();

    #[cfg(windows)]
    let (shell_cmd, shell_arg) = if shell == "powershell" || shell == "ps" {
        ("powershell", "-Command")
    } else {
        ("cmd", "/C")
    };
    #[cfg(not(windows))]
    let (shell_cmd, shell_arg) = match shell {
        "bash" => ("bash", "-c"),
        _ => ("sh", "-c"),
    };

    let mut cmd = std::process::Command::new(shell_cmd);
    cmd.args([shell_arg, command]);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            std::thread::sleep(std::time::Duration::from_millis(100));
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    let mut out = String::new();
                    let mut err = String::new();
                    use std::io::Read;
                    if let Some(mut s) = child.stdout.take() { let _ = s.read_to_string(&mut out); }
                    if let Some(mut s) = child.stderr.take() { let _ = s.read_to_string(&mut err); }
                    app.push_output(&format!("[!] 进程立即退出 (退出码:{}) PID:{}", code, pid));
                    if !out.is_empty() { for l in out.lines() { app.push_output(&format!("  {}", l)); } }
                    if !err.is_empty() { for l in err.lines() { app.push_output(&format!("  [E] {}", l)); } }
                }
                Ok(None) => {
                    let stdout_pipe = child.stdout.take().unwrap();
                    let stderr_pipe = child.stderr.take().unwrap();
                    let stdin_pipe = child.stdin.take().unwrap();

                    let pid_label = pid;
                    // stdout 实时推送线程
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader};
                        let reader = BufReader::new(stdout_pipe);
                        for line in reader.lines() {
                            match line {
                                Ok(text) => {
                                    crate::ai::types::send_ai_message_raw(
                                        &format!("[bg:{}] {}", pid_label, text)
                                    );
                                }
                                Err(_) => break,
                            }
                        }
                        crate::ai::types::send_ai_message_raw(
                            &format!("[bg:{}] ── 进程输出流结束 ──", pid_label)
                        );
                        crate::ai::globals::bg_cleanup(pid_label);
                    });
                    // stderr 实时推送线程
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader};
                        let reader = BufReader::new(stderr_pipe);
                        for line in reader.lines() {
                            match line {
                                Ok(text) => {
                                    crate::ai::types::send_ai_message_raw(
                                        &format!("[bg:{} ERR] {}", pid_label, text)
                                    );
                                }
                                Err(_) => break,
                            }
                        }
                    });

                    crate::ai::globals::bg_cleanup_dead();
                    {
                        let map = crate::ai::globals::bg_proc_map();
                        let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
                        if guard.contains_key(&pid) { guard.remove(&pid); }
                        guard.insert(pid, crate::ai::globals::BgProcState {
                            child,
                            command: command.to_string(),
                            started: ts_display.clone(),
                        });
                        let mut sguard = crate::ai::globals::bg_stdin_map()
                            .lock().unwrap_or_else(|e| e.into_inner());
                        sguard.remove(&pid);
                        sguard.insert(pid, stdin_pipe);
                    }

                    app.push_output(&format!("[+] PID:{} | 状态:运行中 | 启动:{}", pid, ts_display));
                    app.push_output(&format!("    命令: {}", command));
                    app.push_output(&format!("[*] exec_list 查看 | exec_kill {} 终止 | exec_send {} <文本> 发送stdin", pid, pid));
                }
                Err(e) => app.push_output(&format!("[!] wait错误: {}", e)),
            }
        }
        Err(e) => app.push_output(&format!("[!] 启动失败: {}", e)),
    }
}

fn cmd_exec_list(app: &mut App) {
    crate::ai::globals::bg_cleanup_dead();
    let map = crate::ai::globals::bg_proc_map();
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };

    if guard.is_empty() {
        app.push_output("[*] 没有运行中的后台进程");
        return;
    }

    app.push_output(&format!("═══ 后台进程 ({}个) ═══", guard.len()));
    let pids: Vec<u32> = guard.keys().copied().collect();
    for pid in pids {
        if let Some(state) = guard.get_mut(&pid) {
            let alive = match state.child.try_wait() {
                Ok(None) => "运行中",
                Ok(Some(s)) => {
                    let code = s.code().unwrap_or(-1);
                    // 进程已退出 — 标记清理
                    drop(guard);
                    crate::ai::globals::bg_cleanup(pid);
                    app.push_output(&format!("  PID:{} — 已退出(退出码:{}) — 已自动清理", pid, code));
                    return;
                }
                Err(_) => "未知",
            };
            app.push_output(&format!("  PID:{} | {} | 启动:{} | {}", pid, alive, state.started, state.command));
        }
    }
    app.push_output("[*] exec_kill <PID> 终止 | exec_send <PID> <文本> 发送stdin");
}

fn cmd_exec_kill(app: &mut App, pid: u32) {
    let map = crate::ai::globals::bg_proc_map();
    let guard = match map.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if !guard.contains_key(&pid) {
        app.push_output(&format!("[!] PID:{} 不在后台进程列表中", pid));
        return;
    }
    drop(guard);
    crate::ai::globals::bg_cleanup(pid);
    app.push_output(&format!("[+] PID:{} 已终止并清理", pid));
}

fn cmd_exec_send(app: &mut App, pid: u32, input: &str) {
    let stdin_map = crate::ai::globals::bg_stdin_map();
    let mut guard = match stdin_map.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    match guard.get_mut(&pid) {
        Some(stdin) => {
            use std::io::Write;
            match writeln!(stdin, "{}", input) {
                Ok(_) => app.push_output(&format!("[+] 已发送到 PID:{}", pid)),
                Err(e) => app.push_output(&format!("[!] 发送失败: {}", e)),
            }
        }
        None => app.push_output(&format!("[!] PID:{} 不存在或无stdin管道 (进程可能在exec_bg之前启动)", pid)),
    }
}

fn cmd_edit(app: &mut App, arg: &Option<String>) {
    if let Some(path) = arg {
        let p = path.clone();
        spawn_async_cmd(app, format!("编辑 {}", p), move |out| {
            match crate::tools::edit_file(&p) {
                Ok(msg) => out.push(msg),
                Err(e) => out.push(format!("[!] 编辑失败: {}", e)),
            }
        });
    } else {
        app.push_output("[!] 用法: edit <文件路径>");
        app.push_output("[*] 打开系统默认编辑器 (notepad/vim/nano) 编辑文件");
    }
}

fn cmd_hexedit(app: &mut App, arg1: &Option<String>, arg2: &Option<String>, rest: Option<&str>) {
    if let Some(path) = arg1 {
        let p = path.clone();
        // 解析offset: 支持十进制或十六进制(0x前缀)
        let offset: Option<usize> = arg2.as_deref().and_then(|s| {
            if s == "--gui" {
                None
            } else if s.starts_with("0x") || s.starts_with("0X") {
                usize::from_str_radix(&s[2..], 16).ok()
            } else {
                s.parse::<usize>().ok()
            }
        });

        // hex_bytes: 来自 rest (去掉offset后的部分)
        let hex_bytes: Option<String> = if offset.is_some() {
            rest.map(|r| {
                // rest = "0x1A4 FF AE 01" — 需要去掉第一个token(offset已解析)
                let tokens: Vec<&str> = r.splitn(2, ' ').collect();
                if tokens.len() > 1 { tokens[1].to_string() } else { String::new() }
            }).filter(|s| !s.is_empty())
        } else {
            arg2.as_deref().filter(|s| *s == "--gui").map(|_| String::new())
        };

        let _is_gui = arg2.as_deref() == Some("--gui");

        spawn_async_cmd(app, format!("HexEdit {}", p), move |out| {
            match crate::tools::hex_edit(&p, offset, hex_bytes.as_deref()) {
                Ok(msg) => out.push(msg),
                Err(e) => out.push(format!("[!] HexEdit失败: {}", e)),
            }
        });
    } else {
        app.push_output("[!] 用法: hexedit <文件> [偏移] [十六进制字节]");
        app.push_output("[*] 查看:   hexedit payload.bin");
        app.push_output("[*] 修改:   hexedit payload.bin 420 FF AE 01");
        app.push_output("[*] 修改:   hexedit payload.bin 0x1A4 FF AE 01");
        app.push_output("[*] GUI模式: hexedit payload.bin --gui");
    }
}

fn cmd_save(app: &mut App, arg: &Option<String>) {
    if let Some(filename) = arg {
        let content = app.output.join("\n");
        match std::fs::write(filename, &content) {
            Ok(_) => app.push_output(&format!("[+] 会话已保存到: {} ({} 行)", filename, app.output.len())),
            Err(e) => app.push_output(&format!("[!] 保存失败: {}", e)),
        }
    } else { app.push_output("[!] 用法: save <文件名>"); }
}

// ═══════════════════════════════════════════
// Rust 编译 — rustc / cargo build
// ═══════════════════════════════════════════

fn cmd_rustc(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => {
            app.push_output("[!] 用法: rustc <source.rs> [-o <output>] [-r | --release] [-- <extra_args>]");
            return;
        }
        None => {
            app.push_output("[!] 用法: rustc <source.rs> [-o <output>] [-r | --release] [-- <extra_args>]");
            app.push_output("[*] 也可编译 Cargo 项目: rustc <目录> [-r]");
            app.push_output("[*] 例: rustc mytool.rs -r | rustc ./myproject --release");
            return;
        }
    };

    // ── 解析所有剩余参数 (arg2 = output, 可能含多个 token) ──
    let mut release = false;
    let mut explicit_output: Option<String> = None;
    let mut extra_args: Vec<String> = Vec::new();

    let token_stream = output.as_deref().unwrap_or("");

    let mut tokens = token_stream.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        if tok == "-r" || tok == "--release" {
            release = true;
        } else if tok == "-o" {
            if let Some(&name) = tokens.peek() {
                if !name.starts_with('-') {
                    explicit_output = Some(name.to_string());
                    tokens.next();
                }
            }
        } else if tok == "--" {
            for t in tokens.by_ref() {
                extra_args.push(t.to_string());
            }
            break;
        } else if tok.starts_with('-') {
            extra_args.push(tok.to_string());
        }
        // 忽略非选项 token
    }

    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| if release { "release".into() } else { "debug".into() });
    spawn_async_cmd(app, format!("编译 {}", src), move |out| {
        out.push(format!("[*] ======== Rust 编译: {} ========", src));
        out.push(format!("[*] 模式: {} | 输出: {}", if release { "release" } else { "debug" }, out_label));

        match crate::tools::files::compile_rust(&src, explicit_output.as_deref(), release, &extra_args) {
            Ok(msg) => {
                for line in msg.lines() { out.push(format!("  {}", line)); }
                out.push("[+] 编译完成 — 二进制已生成".into());
            }
            Err(e) => {
                out.push(format!("[!] {}", e));
            }
        }
    });
}

// ═══════════════════════════════════════════
// C/C++ 编译 — gcc / clang / msvc
// ═══════════════════════════════════════════

fn cmd_cc(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => {
            app.push_output("[!] 用法: cc <source.c> [-o <output>] [-O2 | --release] [-- <extra_args>]");
            app.push_output("[*] 支持: .c / .cpp / .cxx / .cc  自动检测 gcc/clang/msvc");
            app.push_output("[*] 例: cc payload.c -O2 -o payload.exe");
            return;
        }
        None => {
            app.push_output("[!] 用法: cc <source> [-o <output>] [-O2 | --release] [-- <extra>]");
            app.push_output("[*] C 编译命令 — 自动检测 GCC/Clang/MSVC");
            app.push_output("[*] 支持 .c .cpp .cxx .cc | 例: cc shell.c -O2 -o shell");
            return;
        }
    };

    // ── 解析所有剩余参数 (arg2 = output, 可能含多个 token) ──
    let mut optimize = false;
    let mut explicit_output: Option<String> = None;
    let mut extra_args: Vec<String> = Vec::new();

    // 仅解析 arg2 (= output)，rest 是 arg1 的副本这里不用
    let token_stream = output.as_deref().unwrap_or("");

    let mut tokens = token_stream.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        if tok == "-O2" || tok == "-O3" || tok == "--release" || tok == "-r" {
            optimize = true;
        } else if tok == "-o" {
            // 下一个 token 是输出文件名
            if let Some(&name) = tokens.peek() {
                if !name.starts_with('-') {
                    explicit_output = Some(name.to_string());
                    tokens.next();
                }
            }
        } else if tok == "--" {
            // -- 之后全部是额外参数
            for t in tokens.by_ref() {
                extra_args.push(t.to_string());
            }
            break;
        } else if tok.starts_with('-') {
            extra_args.push(tok.to_string());
        }
        // 忽略非选项 token (如重复的源文件名)
    }

    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto".into());
    spawn_async_cmd(app, format!("C编译 {}", src), move |out| {
        out.push(format!("[*] ======== C/C++ 编译: {} ========", src));
        out.push(format!("[*] 优化: {} | 输出: {}", if optimize { "O2" } else { "无" }, out_label));

        match crate::tools::files::compile_c(&src, explicit_output.as_deref(), optimize, &extra_args) {
            Ok(msg) => {
                for line in msg.lines() { out.push(format!("  {}", line)); }
                out.push("[+] C 编译完成 — 二进制已生成".into());
            }
            Err(e) => {
                out.push(format!("[!] {}", e));
                out.push("[*] 提示: 确保已安装 GCC/MinGW 或 Clang 或 MSVC 编译器".into());
            }
        }
    });
}

// ═══════════════════════════════════════════
// 多语言编译器命令处理器
// ═══════════════════════════════════════════

fn cmd_compile_cpp(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    // 复用 cmd_cc 的参数解析模式
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-cpp <source.cpp> [-o <output>] [-O2]"); return; }
        None => { app.push_output("[!] 用法: compile-cpp <source.cpp> [-o <output>] [-O2]"); return; }
    };
    let (optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto".into());
    spawn_async_cmd(app, format!("C++编译 {}", src), move |out| {
        out.push(format!("[*] ======== C++ 编译: {} ========", src));
        out.push(format!("[*] 优化: {} | 输出: {}", if optimize { "O2" } else { "无" }, out_label));
        match crate::tools::compiler::compile_cpp(&src, explicit_output.as_deref(), optimize, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] C++ 编译完成".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 g++/clang++ 编译器".into()); }
        }
    });
}

fn cmd_compile_go(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-go <source.go> [-o <output>] [-O]"); return; }
        None => { app.push_output("[!] 用法: compile-go <source.go> [-o <output>] [-O]"); return; }
    };
    let (optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto".into());
    spawn_async_cmd(app, format!("Go编译 {}", src), move |out| {
        out.push(format!("[*] ======== Go 编译: {} ========", src));
        out.push(format!("[*] 优化: {} | 输出: {}", if optimize { "strip" } else { "无" }, out_label));
        match crate::tools::compiler::compile_go(&src, explicit_output.as_deref(), optimize, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] Go 编译完成".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 Go 编译器 (go build)".into()); }
        }
    });
}

fn cmd_compile_java(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-java <source.java> [-d <output_dir>]"); return; }
        None => { app.push_output("[!] 用法: compile-java <source.java> [-d <output_dir>]"); return; }
    };
    let (optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| ".".into());
    spawn_async_cmd(app, format!("Java编译 {}", src), move |out| {
        out.push(format!("[*] ======== Java 编译: {} ========", src));
        out.push(format!("[*] 输出目录: {}", out_label));
        match crate::tools::compiler::compile_java(&src, Some(&out_label), optimize, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] Java 编译完成 (.class)".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 JDK (javac)".into()); }
        }
    });
}

fn cmd_compile_cs(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-cs <source.cs> [-o <output>] [-O]"); return; }
        None => { app.push_output("[!] 用法: compile-cs <source.cs> [-o <output>] [-O]"); return; }
    };
    let (optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto".into());
    spawn_async_cmd(app, format!("C#编译 {}", src), move |out| {
        out.push(format!("[*] ======== C# 编译: {} ========", src));
        out.push(format!("[*] 优化: {} | 输出: {}", if optimize { "Release" } else { "Debug" }, out_label));
        match crate::tools::compiler::compile_csharp(&src, explicit_output.as_deref(), optimize, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] C# 编译完成".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 .NET SDK (dotnet build) 或 csc".into()); }
        }
    });
}

fn cmd_compile_python(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-python <source.py>"); return; }
        None => { app.push_output("[!] 用法: compile-python <source.py>"); return; }
    };
    let (optimize, explicit_output, _extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    spawn_async_cmd(app, format!("Python编译 {}", src), move |out| {
        out.push(format!("[*] ======== Python 编译: {} ========", src));
        match crate::tools::compiler::compile_python(&src, explicit_output.as_deref(), optimize, &[]) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] Python 编译完成 (.pyc)".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 Python (py_compile)".into()); }
        }
    });
}

fn cmd_compile_asm(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-asm <source.asm> [-o <output.o>]"); return; }
        None => { app.push_output("[!] 用法: compile-asm <source.asm> [-o <output.o>]"); return; }
    };
    let (_optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto.o".into());
    spawn_async_cmd(app, format!("ASM编译 {}", src), move |out| {
        out.push(format!("[*] ======== NASM 汇编: {} ========", src));
        out.push(format!("[*] 输出: {}", out_label));
        match crate::tools::compiler::compile_asm(&src, explicit_output.as_deref(), false, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] NASM 汇编完成 (.o) — 需链接器生成可执行文件".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 NASM 汇编器".into()); }
        }
    });
}

fn cmd_compile_zig(app: &mut App, source: &Option<String>, output: &Option<String>, _rest: Option<&str>) {
    let source = match source {
        Some(s) if !s.starts_with('-') => s.clone(),
        Some(_) => { app.push_output("[!] 用法: compile-zig <source.zig> [-o <output>] [-O]"); return; }
        None => { app.push_output("[!] 用法: compile-zig <source.zig> [-o <output>] [-O]"); return; }
    };
    let (optimize, explicit_output, extra_args) = parse_compile_flags(output.as_deref().unwrap_or(""));
    let src = source.clone();
    let out_label = explicit_output.clone().unwrap_or_else(|| "auto".into());
    spawn_async_cmd(app, format!("Zig编译 {}", src), move |out| {
        out.push(format!("[*] ======== Zig 编译: {} ========", src));
        out.push(format!("[*] 优化: {} | 输出: {}", if optimize { "ReleaseFast" } else { "Debug" }, out_label));
        match crate::tools::compiler::compile_zig(&src, explicit_output.as_deref(), optimize, &extra_args) {
            Ok(msg) => { for line in msg.lines() { out.push(format!("  {}", line)); } out.push("[+] Zig 编译完成".into()); }
            Err(e) => { out.push(format!("[!] {}", e)); out.push("[*] 提示: 需安装 Zig 编译器 (zig build-exe)".into()); }
        }
    });
}

fn cmd_compilers(app: &mut App) {
    let output = crate::tools::compiler::list_compilers();
    for line in output.lines() { app.push_output(line); }
}

/// 统一解析编译标志: (-o output, -O/-O2/--release, -- extra_args)
fn parse_compile_flags(flags: &str) -> (bool, Option<String>, Vec<String>) {
    let mut optimize = false;
    let mut output: Option<String> = None;
    let mut extra_args: Vec<String> = Vec::new();
    let mut tokens = flags.split_whitespace().peekable();

    while let Some(tok) = tokens.next() {
        if tok == "-O" || tok == "-O2" || tok == "-O3" || tok == "--release" || tok == "-r" {
            optimize = true;
        } else if tok == "-o" || tok == "-d" {
            if let Some(&name) = tokens.peek() {
                if !name.starts_with('-') { output = Some(name.to_string()); tokens.next(); }
            }
        } else if tok == "--" {
            for t in tokens.by_ref() { extra_args.push(t.to_string()); }
            break;
        } else if tok.starts_with('-') {
            extra_args.push(tok.to_string());
        }
    }
    (optimize, output, extra_args)
}

// ═══════════════════════════════════════════
// AI Assistant — process_ai_query
// ═══════════════════════════════════════════

pub fn process_ai_query(app: &mut App, query: &str, stream_mode: bool) {
    // ★ v9.4: AI熔断器 — 连续3次失败后自动熔断30秒
    use crate::tools::fault_tolerant::CircuitBreaker;
    static AI_CIRCUIT_BREAKER: std::sync::OnceLock<CircuitBreaker> = std::sync::OnceLock::new();
    let cb = AI_CIRCUIT_BREAKER.get_or_init(|| CircuitBreaker::new(3, 30));

    if cb.is_open() {
        if cb.should_half_open() {
            cb.half_open();
            app.push_output("[*] AI熔断器半开 — 尝试恢复连接...");
        } else {
            app.push_output("[!] AI服务熔断中 — 连续失败过多, 请稍后再试 (或等30秒自动恢复)");
            return;
        }
    }

    let (api_key, key_source) = app.cfg.resolve_api_key_with_source(app.vault.as_deref());
    let api_url = app.cfg.deepseek_api_url.clone();
    let model = app.cfg.deepseek_model.clone();

    // ★ 防御性检查: API Key 为空时阻止请求 (兜底, cmd_dpai已有检查但此处保底)
    if api_key.is_empty() {
        app.push_output("[!] DeepSeek API Key 未配置 — 无法发送AI请求");
        app.push_output("[*] 请执行: config set deepseek_api_key=sk-xxx");
        app.push_output("[*] 或者: vault set api_keys deepseek <key> (加密存储)");
        app.ai_pending = false;
        return;
    }

    // ★ v4.1.1: 熔断恢复后报告 Key 来源 (帮助诊断连接问题)
    if key_source == crate::config::KeySource::VaultDefaultBackCompat {
        app.push_output("[*] Key 来自 Vault 旧格式 (default/api_keys) — 建议迁移到 api_keys/deepseek");
    }

    if app.ai_session.is_none() {
        app.ai_session = Some(crate::ai::AiSession::new(Some(&app.cfg.deepseek_system_prompt)));
    }

    let tx = match &app.ai_tx {
        Some(tx) => tx.clone(),
        None => {
            app.push_output("[!] AI channel not initialized — please restart");
            app.ai_pending = false;
            return;
        }
    };

    let mut session = match app.ai_session.take() {
        Some(s) => s,
        None => {
            app.push_output("[!] AI session error — please restart dpai");
            app.ai_pending = false;
            return;
        }
    };

    let tool_status: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    app.ai_active_tool = Some(tool_status.clone());

    let cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool> =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.ai_cancel = Some(cancel_flag.clone());

    app.push_output(if stream_mode { "[*] AI 实时模式 — API/工具调用将实时输出..." } else { "[*] AI thinking..." });
    app.ai_pending = true;

    // 始终传递 stream_tx — 非流式模式下也需要显示重试/错误消息
    let stream_tx: Option<std::sync::mpsc::Sender<crate::ai::StreamMsg>> = app.ai_stream_tx.clone();

    let query_owned = query.to_string();

    std::thread::Builder::new()
        .name(format!("ruoo-ai-{}", &query_owned.chars().take(24).collect::<String>()))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.send(&api_key, &api_url, &model, &query_owned, Some(&tool_status), Some(&cancel_flag), stream_tx.as_ref())
            }));
            match result {
                Ok(r) => { let _ = tx.send((session, r)); }
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        format!("AI 线程 panic: {}", s)
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        format!("AI 线程 panic: {}", s)
                    } else {
                        "AI 线程 panic: 未知原因".to_string()
                    };
                    let _ = tx.send((session, Err(msg)));
                }
            }
        })
        .expect("spawn AI thread");
}

// ═══════════════════════════════════════════
// Script engine bridge
// ═══════════════════════════════════════════

const MAX_SCRIPT_DEPTH: u32 = 16;

fn cmd_run(app: &mut App, filename: &Option<String>) {
    let mut force = false;
    let fname = match filename {
        Some(f) if f.starts_with("--force") || f.starts_with("-f") => {
            force = true;
            f.split_once(' ').map(|(_, name)| name.trim().to_string())
                .unwrap_or_default()
        }
        Some(f) => f.clone(),
        None => {
            app.push_output("[!] 用法: run <脚本.ruoo> | run --force <脚本.ruoo>");
            app.push_output("[*] 提示: 脚本文件在 scripts/ 目录下，首次运行需 script permit <名称>");
            return;
        }
    };

    if fname.is_empty() {
        app.push_output("[!] 用法: run --force <脚本.ruoo>");
        return;
    }

    if app.script_depth >= MAX_SCRIPT_DEPTH {
        app.push_output(&format!("[!] Script recursion depth exceeded (max {}) — blocked", MAX_SCRIPT_DEPTH));
        return;
    }
    app.script_depth += 1;

    let content = match crate::script::load_script(&fname) {
        Ok(c) => c,
        Err(e) => {
            app.push_output(&format!("[!] {}", e));
            app.script_depth = app.script_depth.saturating_sub(1);
            return;
        }
    };

    let script_path = if std::path::Path::new(&fname).is_absolute() {
        std::path::PathBuf::from(&fname)
    } else if fname.ends_with(".ruoo") {
        crate::script::scripts_dir().join(&fname)
    } else {
        crate::script::scripts_dir().join(format!("{}.ruoo", fname))
    };

    if !script_path.exists() {
        app.push_output(&format!("[!] 脚本未找到: {}", script_path.display()));
        app.script_depth = app.script_depth.saturating_sub(1);
        return;
    }

    let script_dir = script_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    let script_name = script_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&fname)
        .to_string();

    let mut ctx = crate::script::ScriptContext::new(script_dir.clone(), &script_name);
    ctx.force = force;
    ctx.vars.insert("TARGET".into(), app.target.clone());
    ctx.vars.insert("target".into(), app.target.clone());

    if !force {
        if let Err(e) = ctx.check_permitted(&script_path) {
            app.push_output(&format!("[!] {}", e));
            app.push_output("[*] 使用 run --force <脚本> 跳过检查 (风险自负)");
            app.script_depth = app.script_depth.saturating_sub(1);
            return;
        }
        app.push_output(&format!("[+] 脚本已许可: {} (SHA-256 验证通过)", script_name));
    } else {
        app.push_output("[!] ⚠️ 强制模式 — 跳过脚本许可检查");
    }

    app.push_output(&format!("[*] ======== 执行脚本: {} ========", script_path.display()));

    let lines: Vec<&str> = content.lines().collect();
    let start_time = std::time::Instant::now();

    crate::script::execute_script(&mut ctx, &lines, app);

    let elapsed = start_time.elapsed();
    let verdict = if ctx.errors > 0 { "⚠ 有错误" } else { "✅ 成功" };
    app.push_output(&format!(
        "[+] 脚本结束 — {} 指令 / {} 错误 / {:.1}s / {}| 深度{}/{}",
        ctx.executed, ctx.errors, elapsed.as_secs_f64(), verdict,
        app.script_depth, MAX_SCRIPT_DEPTH
    ));

    app.script_depth = app.script_depth.saturating_sub(1);
}

fn cmd_script(app: &mut App, sub: &Option<String>, arg2: &Option<String>) {
    let sub = sub.as_deref().unwrap_or("list");
    match sub {
        "edit" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => { app.push_output("[!] 用法: script edit <名称>"); return; }
            };
            let path = if name.ends_with(".ruoo") {
                crate::script::scripts_dir().join(&name)
            } else {
                crate::script::scripts_dir().join(format!("{}.ruoo", name))
            };
            if !path.exists() {
                app.push_output(&format!("[!] 脚本未找到: {}", path.display()));
                app.push_output("[*] 使用: script new <名称>");
                return;
            }
            #[cfg(windows)]
            { let _ = std::process::Command::new("notepad").arg(&path).spawn(); }
            #[cfg(not(windows))]
            {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                let _ = std::process::Command::new(&editor).arg(&path).spawn();
            }
            app.push_output(&format!("[*] 已在编辑器中打开: {}", path.display()));
        }
        _ => {
            app.push_output(&format!("[!] 未知子命令: {} — script list/new/edit/permit/revoke/perms", sub));
        }
    }
}

// ═══════════════════════════════════════════
// SysLoad commands — 内核驱动加载
// ═══════════════════════════════════════════

fn cmd_sysload(app: &mut App, name: &Option<String>, path: &Option<String>) {
    let name = match name {
        Some(n) => n.clone(),
        None => {
            app.push_output("[!] 用法: sysload <名称> <路径>  例: sysload mydrv ./mydrv.sys");
            app.push_output("[!] ⚠ 需要管理员/root 权限");
            return;
        }
    };
    let path = match path {
        Some(p) => p.clone(),
        None => {
            app.push_output("[!] 用法: sysload <名称> <路径>  例: sysload mydrv ./mydrv.sys");
            return;
        }
    };

    let n = name.clone();
    let p = path.clone();
    spawn_async_cmd(app, format!("SysLoad {}", n), move |out| {
        out.push(format!("[⚙] 加载内核驱动: {} ({})...", n, p));
        match crate::tools::kernel::kernel_driver_load(&n, &p) {
            Ok(msg) => out.push(msg),
            Err(e) => out.push(format!("[!] 加载失败: {}", e)),
        }
    });
}

fn cmd_sysunload(app: &mut App, name: &Option<String>) {
    let name = match name {
        Some(n) => n.clone(),
        None => {
            app.push_output("[!] 用法: sysunload <名称>");
            return;
        }
    };

    let n = name.clone();
    spawn_async_cmd(app, format!("SysUnload {}", n), move |out| {
        out.push(format!("[⚙] 卸载内核驱动: {}...", n));
        match crate::tools::kernel::kernel_driver_unload(&n) {
            Ok(msg) => out.push(msg),
            Err(e) => out.push(format!("[!] 卸载失败: {}", e)),
        }
    });
}

fn cmd_syslist(app: &mut App) {
    spawn_async_cmd(app, "SysList".to_string(), move |out| {
        match crate::tools::kernel::kernel_driver_list() {
            Ok(msg) => {
                for line in msg.lines() {
                    out.push(line.to_string());
                }
            }
            Err(e) => out.push(format!("[!] {}", e)),
        }
    });
}

fn cmd_kinfo(app: &mut App) {
    app.push_output(&crate::tools::kernel::kernel_backend_info());
}

fn cmd_kcompile(app: &mut App, source: &Option<String>, output: &Option<String>) {
    let source = match source {
        Some(s) => s.clone(),
        None => { app.push_output("[!] 用法: kcompile <源文件> <输出>  例: kcompile mydrv.c mydrv.sys"); return; }
    };
    let output = match output {
        Some(o) => o.clone(),
        None => { app.push_output("[!] 用法: kcompile <源文件> <输出>"); return; }
    };
    let s = source.clone();
    let o = output.clone();
    spawn_async_cmd(app, format!("KCompile {}", s), move |out| {
        out.push(format!("[⚙] 编译驱动: {} → {}...", s, o));
        match crate::tools::kernel::kernel_compile(&s, &o) {
            Ok(msg) => out.push(msg),
            Err(e) => out.push(format!("[!] 编译失败: {}", e)),
        }
    });
}

fn cmd_ktemplate(app: &mut App, name: &Option<String>, platform: &Option<String>) {
    let name = match name {
        Some(n) => n.clone(),
        None => { app.push_output("[!] 用法: ktemplate <名称> <平台>  例: ktemplate mydrv windows"); return; }
    };
    let platform = platform.as_deref().unwrap_or(if cfg!(windows) { "windows" } else { "linux" });
    match crate::tools::kernel::kernel_template(&name, platform) {
        Ok(code) => app.push_output(&format!("[+] 驱动模板 ({} / {}):\n{}", name, platform, code)),
        Err(e) => app.push_output(&format!("[!] {}", e)),
    }
}

fn cmd_kvalidate(app: &mut App, path: &Option<String>) {
    let path = match path {
        Some(p) => p.clone(),
        None => { app.push_output("[!] 用法: kvalidate <路径>  例: kvalidate ./mydrv.sys"); return; }
    };
    match crate::tools::kernel::kernel_validate(&path) {
        Ok(msg) => app.push_output(&msg),
        Err(e) => app.push_output(&format!("[!] {}", e)),
    }
}

fn cmd_kscaffold(app: &mut App, name: &Option<String>, output_dir: &Option<String>, platform: &Option<String>) {
    let name = match name {
        Some(n) => n.clone(),
        None => { app.push_output("[!] 用法: kscaffold <名称> <输出目录> <平台>  例: kscaffold mydrv ./drivers/mydrv_project windows"); return; }
    };
    let output_dir = match output_dir {
        Some(o) => o.clone(),
        None => { app.push_output("[!] 用法: kscaffold <名称> <输出目录> <平台>"); return; }
    };
    let platform = platform.as_deref().unwrap_or(if cfg!(windows) { "windows" } else { "linux" });
    let n = name.clone();
    let o = output_dir.clone();
    let p = platform.to_string();
    spawn_async_cmd(app, format!("KScaffold {}", n), move |out| {
        out.push(format!("[⚙] 生成驱动项目: {} → {} ({}）...", n, o, p));
        match crate::tools::kernel::kernel_scaffold(&n, &o, &p) {
            Ok(msg) => out.push(msg),
            Err(e) => out.push(format!("[!] 骨架生成失败: {}", e)),
        }
    });
}



// ★ v4.3: 多行插件结果分行推送
fn plugin_push_multiline(app: &mut App, plugin_name: &str, result: &str) {
    let mut lines = result.lines();
    if let Some(first) = lines.next() {
        app.push_output(&format!("[plugin:{}] {}", plugin_name, first));
        for line in lines {
            app.push_output(&format!("           {}", line));
        }
    }
}

// ═══════════════════════════════════════════
// Plugin commands — 插件管理 (终端级别)
// ═══════════════════════════════════════════

fn cmd_plugin(app: &mut App, sub: &Option<String>, arg2: &Option<String>) {
    let sub = sub.as_deref().unwrap_or("list");
    match sub {
        // ── v7.1: 软重启 — 崩溃后一键恢复 ──
        "restart" | "reload" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => {
                    app.push_output("[!] 用法: plugin restart <名称>");
                    return;
                }
            };
            let report = {
                let mut mgr = app.plugin_mgr.lock().unwrap();
                mgr.soft_restart(&name, &mut app.cmd_registry)
            }; // mgr dropped here
            app.push_output(&report);
        }
        // ── v7.1: 批量恢复所有故障插件 ──
        "recover-all" => {
            let report = {
                let mut mgr = app.plugin_mgr.lock().unwrap();
                mgr.auto_recover_all(&mut app.cmd_registry)
            };
            app.push_output(&report);
        }
        // ── 防崩溃框架 ──
        "crash-recover" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => {
                    app.push_output("[!] 用法: plugin crash-recover <名称>");
                    return;
                }
            };
            let (report, _) = app.plugin_mgr.lock().unwrap().crash_recover(&name);
            app.push_output(&report);
        }
        "force-unload" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => {
                    app.push_output("[!] 用法: plugin force-unload <名称>");
                    return;
                }
            };
            let result = app.plugin_mgr.lock().unwrap().force_unload(&name);
            match result {
                Ok(msg) => app.push_output(&msg),
                Err(e) => app.push_output(&format!("[!] {}", e)),
            }
        }
        "health" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => {
                    app.push_output("[!] 用法: plugin health <名称> (或 plugin health-all 查看全部)");
                    return;
                }
            };
            let health = app.plugin_mgr.lock().unwrap().plugin_health_report(&name);
            app.push_output(&health);
        }
        "health-all" => {
            let summary = app.plugin_mgr.lock().unwrap().plugin_health_summary();
            app.push_output(&summary);
        }
        // ── 列表 ──
        "list" => {
            let details = app.plugin_mgr.lock().unwrap().list_detailed();
            if details.is_empty() {
                app.push_output("[*] 无已加载插件");
            } else {
                app.push_output("[*] ====== 已加载插件 ======");
                for d in &details {
                    app.push_output(d);
                }
            }
        }
        // ── 加载/卸载 ──
        "load" => {
            let args = arg2.as_deref().unwrap_or("");
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
                app.push_output("[!] 用法: plugin load <别名> <路径>");
                return;
            }
            let alias = parts[0];
            let path = parts[1];
            app.push_output(&format!("[·] 加载插件: {} ({})...", alias, path));
            let (load_result, reg_result) = {
                let mut mgr = app.plugin_mgr.lock().unwrap();
                let result = mgr.load(alias, path);
                let reg = match result {
                    Ok(_) => crate::plugin::try_register_from_file(path, alias, &mut app.cmd_registry),
                    Err(_) => Ok(0),
                };
                (result, reg)
            }; // mgr dropped here
            match load_result {
                Ok(msg) => {
                    app.push_output(&format!("  {}", msg));
                    match reg_result {
                        Ok(n) if n > 0 => app.push_output(&format!("  [+] 已注册 {} 个命令", n)),
                        Ok(_) => app.push_output("  [*] 无命令声明"),
                        Err(e) => app.push_output(&format!("  [!] 命令注册: {}", e)),
                    }
                }
                Err(e) => app.push_output(&format!("  [!] 加载失败: {}", e)),
            }
        }
        "unload" => {
            let name = match arg2 {
                Some(n) => n.clone(),
                None => {
                    app.push_output("[!] 用法: plugin unload <名称>");
                    return;
                }
            };
            let (unload_result, removed) = {
                let mut mgr = app.plugin_mgr.lock().unwrap();
                let result = mgr.unload(&name);
                let removed = if result.is_ok() { app.cmd_registry.unregister_plugin(&name) } else { 0 };
                (result, removed)
            }; // mgr dropped here
            match unload_result {
                Ok(()) => {
                    app.push_output(&format!("[+] 插件已卸载: {} (注销 {} 个命令)", name, removed));
                }
                Err(e) => app.push_output(&format!("[!] {}", e)),
            }
        }
        // ── 插件注册表 ──
        _ => {
            app.push_output(&format!(
                "[!] 未知子命令: {}\n\
                 [*] 可用: load/unload/list/restart/recover-all/crash-recover/force-unload/health/health-all",
                sub
            ));
        }
    }
}

// ═══════════════════════════════════════════
// Vault commands — 加密保险库
// ═══════════════════════════════════════════

fn cmd_vault(app: &mut App, sub: &Option<String>, args: &Option<String>) {
    let sub = sub.as_deref().unwrap_or("status");
    let vault = match app.vault.as_ref() {
        Some(v) => v.clone(),
        None => {
            app.push_output("[!] Vault 未挂载 — 请重启程序并输入主密码");
            return;
        }
    };

    match sub {
        "status" | "" => {
            app.push_output("[*] ======== Vault 状态 ========");
            let locked = app.vault_locked;
            app.push_output(&format!("  状态      : {}", if locked { "🔒 已锁定" } else { "🔓 已解锁" }));
            app.push_output(&format!("  条目数    : {}", vault.entry_count()));
            app.push_output("  加密算法  : AES-256-GCM");
            app.push_output("  KDF       : PBKDF2-SHA256 (600k iter)");
            let nss = vault.list_namespaces();
            app.push_output(&format!("  命名空间  : {}", nss.join(", ")));
            app.push_output("[+] vault add/set/get/del/list 管理数据 | vault passwd 修改密码");
        }
        "list" => {
            if app.vault_locked { app.push_output("[!] Vault 已锁定，请先 unlock"); return; }
            app.push_output("[*] ======== Vault 键列表 ========");
            let ns = args.as_deref().unwrap_or("");
            if ns.is_empty() {
                let nss = vault.list_namespaces();
                for ns_name in &nss {
                    let keys = vault.list_keys(ns_name);
                    app.push_output(&format!("  [{}/] {} 个条目", ns_name, keys.len()));
                    for k in &keys { app.push_output(&format!("    .{}", k)); }
                }
                if nss.is_empty() { app.push_output("  (空)"); }
            } else {
                let keys = vault.list_keys(ns);
                if keys.is_empty() {
                    app.push_output(&format!("  [{}] (空)", ns));
                } else {
                    for k in &keys { app.push_output(&format!("  {}", k)); }
                    app.push_output(&format!("[+] 共 {} 个条目 (ns: {})", keys.len(), ns));
                }
            }
        }
        "add" | "set" => {
            if app.vault_locked { app.push_output("[!] Vault 已锁定，请先 unlock"); return; }
            if let Some(ref arg_str) = args {
                let parts: Vec<&str> = arg_str.splitn(3, ' ').collect();
                match parts.len() {
                    3 => {
                        let ns = parts[0].trim();
                        let key = parts[1].trim();
                        let value = parts[2].trim();
                        match vault.set(ns, key, value) {
                            Ok(()) => app.push_output(&format!("[+] 已存入: [{}/{}] = ***", ns, key)),
                            Err(e) => app.push_output(&format!("[!] 存储失败: {}", e)),
                        }
                    }
                    2 => {
                        let key = parts[0].trim();
                        let value = parts[1].trim();
                        match vault.set("default", key, value) {
                            Ok(()) => app.push_output(&format!("[+] 已存入: [default/{}] = ***", key)),
                            Err(e) => app.push_output(&format!("[!] 存储失败: {}", e)),
                        }
                    }
                    _ => {
                        app.push_output("[!] 用法: vault add/set [<命名空间>] <键> <值>");
                        app.push_output("[*] 例: vault set api_keys deepseek sk-xxx (三参数)");
                        app.push_output("[*] 例: vault set mykey myvalue (两参数,存入default)");
                    }
                }
            } else {
                app.push_output("[!] 用法: vault add/set [<命名空间>] <键> <值>");
            }
        }
        "get" => {
            if app.vault_locked { app.push_output("[!] Vault 已锁定，请先 unlock"); return; }
            if let Some(ref arg_str) = args {
                let parts: Vec<&str> = arg_str.splitn(2, ' ').collect();
                match parts.len() {
                    2 => {
                        let ns = parts[0].trim();
                        let key = parts[1].trim();
                        match vault.get(ns, key) {
                            Some(value) => app.push_output(&format!("  [{}/{}] = {}", ns, key, value)),
                            None => app.push_output(&format!("[!] 键不存在: [{}/{}]", ns, key)),
                        }
                    }
                    1 => {
                        let key = parts[0].trim();
                        match vault.get("default", key) {
                            Some(value) => app.push_output(&format!("  [default/{}] = {}", key, value)),
                            None => app.push_output(&format!("[!] 键不存在: [default/{}]", key)),
                        }
                    }
                    _ => {
                        app.push_output("[!] 用法: vault get [<命名空间>] <键>");
                    }
                }
            } else {
                app.push_output("[!] 用法: vault get [<命名空间>] <键>");
            }
        }
        "del" => {
            if app.vault_locked { app.push_output("[!] Vault 已锁定，请先 unlock"); return; }
            if let Some(ref arg_str) = args {
                let parts: Vec<&str> = arg_str.splitn(2, ' ').collect();
                match parts.len() {
                    2 => {
                        let ns = parts[0].trim();
                        let key = parts[1].trim();
                        match vault.delete(ns, key) {
                            Ok(()) => app.push_output(&format!("[+] 已删除: [{}/{}]", ns, key)),
                            Err(e) => app.push_output(&format!("[!] 删除失败: {}", e)),
                        }
                    }
                    1 => {
                        let key = parts[0].trim();
                        match vault.delete("default", key) {
                            Ok(()) => app.push_output(&format!("[+] 已删除: [default/{}]", key)),
                            Err(e) => app.push_output(&format!("[!] 删除失败: {}", e)),
                        }
                    }
                    _ => {
                        app.push_output("[!] 用法: vault del [<命名空间>] <键>");
                    }
                }
            } else {
                app.push_output("[!] 用法: vault del [<命名空间>] <键>");
            }
        }
        "passwd" => {
            if app.vault_locked { app.push_output("[!] Vault 已锁定，请先 unlock"); return; }
            app.password_mode = true;
            app.password_matrix_seed = app.tick_count.wrapping_mul(1442695040888963407u64);
            app.password_prompt = "🔑 当前密码".into();
            app.password_action = "passwd_old".into();
            app.password_buffer.clear();
            app.input.clear();
            app.cursor = 0;
            app.push_output("[*] ======== 修改 Vault 主密码 ========");
            app.push_output("  [*] 第一阶段: 输入当前密码");
            app.push_output("[*] 输入密码后按 Enter，Esc 取消");
        }
        _ => {
            app.push_output(&format!("[!] 未知子命令: {} — vault status/list/add/set/get/del/passwd", sub));
        }
    }
}

fn cmd_lock(app: &mut App) {
    if app.vault.is_none() {
        app.push_output("[!] Vault 未挂载");
        return;
    }
    if app.vault_locked {
        app.push_output("[*] Vault 已锁定");
    } else {
        if let Some(ref v) = app.vault {
            v.lock();
        }
        app.vault_locked = true;
        app.push_output("[+] 🔒 Vault 已锁定 — 内存加密数据已卸载");
    }
}

fn cmd_unlock(app: &mut App) {
    if app.vault.is_none() {
        app.push_output("[!] Vault 未挂载");
        return;
    }
    if !app.vault_locked {
        app.push_output("[*] Vault 已解锁");
        return;
    }
    app.password_mode = true;
    app.password_matrix_seed = app.tick_count.wrapping_mul(6364136223846793005u64);
    app.password_prompt = "🔑 输入主密码解锁".into();
    app.password_action = "unlock".into();
    app.password_buffer.clear();
    app.input.clear();
    app.cursor = 0;
    app.push_output("🔑 输入主密码解锁:");
    app.push_output("[*] 输入密码后按 Enter，Esc 取消");
}

// ═══════════════════════════════════════════
// 启动脚本管理 (BootScript)
// ═══════════════════════════════════════════

fn cmd_bootscript(app: &mut App, sub: &Option<String>) {
    let sub = sub.as_deref().unwrap_or("status");

    if app.vault.is_none() {
        app.push_output("[!] Vault 未挂载 — 启动脚本功能不可用");
        return;
    }
    if app.vault_locked {
        app.push_output("[!] Vault 已锁定 — 请先 unlock 再操作启动脚本");
        return;
    }

    match sub {
        "status" | "" => {
            app.push_output("[*] ======== 启动加载脚本 (BootScript) ========");
            app.push_output("  存储位置  : Vault → bootscript.script (AES-256-GCM 加密)");
            app.push_output(&format!("  存在性    : {}", if crate::bootscript::bootscript_exists() { "✅ 已配置" } else { "⚠ 未配置" }));
            match crate::bootscript::get_bootscript() {
                Some(script) if !script.trim().is_empty() => {
                    let lines: Vec<&str> = script.lines().collect();
                    app.push_output(&format!("  状态      : ✅ 已配置 ({} 行)", lines.len()));
                    app.push_output(&format!("  大小      : {} 字节", script.len()));
                    app.push_output("  ─── 内容预览 (前10行) ───");
                    for (i, line) in lines.iter().take(10).enumerate() {
                        let display = if line.len() > 70 {
                            let end = line.floor_char_boundary(67);
                            format!("{}...", &line[..end])
                        } else {
                            line.to_string()
                        };
                        app.push_output(&format!("  {:>3} │ {}", i + 1, display));
                    }
                    if lines.len() > 10 {
                        app.push_output(&format!("  ... (共 {} 行)", lines.len()));
                    }
                    app.push_output("[*] bootscript edit → 修改 | bootscript run → 手动执行 | bootscript reset → 恢复默认");
                }
                _ => {
                    app.push_output("  状态      : ⚠ 未配置 (启动时不执行)");
                    app.push_output("[*] bootscript edit → 创建并编辑 | bootscript reset → 恢复默认模板");
                }
            }
        }

        "edit" => {
            let content = crate::bootscript::get_bootscript()
                .unwrap_or_else(|| crate::bootscript::default_bootscript());

            app.editor_buffer = Some(crate::bootscript::EditorBuffer::new(&content));
            app.editor_cursor_row = 0;
            app.editor_cursor_col = 0;
            app.editor_scroll = 0;
            app.editor_mode = true;
            app.push_output("[*] 📝 已打开启动脚本编辑器");
            app.push_output("[*] Ctrl+S → 加密保存到 Vault | Esc → 取消 | Ctrl+Q → 强制退出");
            app.push_output("[*] 脚本内容在编辑器关闭后从内存中清零");
        }

        "run" => {
            match crate::bootscript::get_bootscript() {
                Some(script) if !script.trim().is_empty() => {
                    app.push_output("[*] ======== 手动执行启动脚本 ========");
                    let lines: Vec<&str> = script.lines().collect();
                    let mut ctx = crate::script::ScriptContext::new(
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "bootscript"
                    );
                    ctx.perms = crate::script::PermissionSet::all();
                    ctx.force = true;
                    crate::script::execute_script(&mut ctx, &lines, app);
                    app.push_output(&format!(
                        "[+] 启动脚本执行完毕 — {} 指令 / {} 错误",
                        ctx.executed, ctx.errors
                    ));
                }
                _ => {
                    app.push_output("[!] 无启动脚本 — 使用 bootscript edit 创建或 bootscript reset 恢复默认");
                }
            }
        }

        "clear" => {
            match crate::bootscript::delete_bootscript() {
                Ok(()) => {
                    app.push_output("[+] 🗑 启动脚本已清除 — 程序启动时不再自动执行");
                    app.push_output("[*] bootscript edit → 重新创建 | bootscript reset → 恢复默认");
                }
                Err(e) => app.push_output(&format!("[!] 清除失败: {}", e)),
            }
        }

        _ => {
            app.push_output(&format!("[!] 未知子命令: {} — bootscript status/edit/reset/run/clear", sub));
        }
    }
}
