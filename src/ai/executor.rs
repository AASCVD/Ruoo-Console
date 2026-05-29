// === 工具执行器 ===
// execute_tool — 所有工具的中央调度 (巨型 match)

use serde_json::Value;
use crate::ai::types::{AiPermLevel, get_terminal_output_text};
use crate::ai::permissions::check_tool_perm;
use crate::ai::globals::{CLEAR_HISTORY_FLAG, TOOL_ABORT_FLAG, global_plugin_mgr, global_cmd_registry};
use crate::ai::{with_global_cmd_registry, clear_all_cache, validate_shell_command, send_ai_message, send_ai_message_raw};
pub(crate) fn execute_tool(name: &str, args_json: &str) -> String {
    let args: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析失败: {}", e),
    };
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let n = |k: &str, d: i64| args.get(k).and_then(|v| v.as_i64()).unwrap_or(d);
    let b = |k: &str, d: bool| args.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    let u = |k: &str, d: u32| args.get(k).and_then(|v| v.as_u64()).unwrap_or(d as u64) as u32;

    // ★ BUG-9 修复: 必填参数校验 — 缺失时返回明确错误而非静默执行
    let req = |k: &str| -> Result<String, String> {
        match args.get(k) {
            Some(v) if v.is_string() && !v.as_str().unwrap_or("").is_empty() => {
                Ok(v.as_str().unwrap().to_string())
            }
            Some(_) => Err(format!("参数 '{}' 为空或类型错误", k)),
            None => Err(format!("缺少必填参数: '{}'", k)),
        }
    };

    crate::telemetry::push_tool_start(name);
    let tool_start = std::time::Instant::now();

    // ★ v5.1: 先检查插件AI工具注册表
    if let Some(tool_def) = crate::plugin::find_ai_tool(name) {
        // 权限检查
        let required = AiPermLevel::from_i32(tool_def.perm_level as i32);
        if let Err(e) = check_tool_perm(required, name) { return e; }

        let result = match global_plugin_mgr() {
            Some(mgr_arc) => {
                let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                // 构建命令字符串: "tool_name param1=val1 param2=val2 ..."
                let mut cmd_parts = vec![name.to_string()];
                for param in &tool_def.params {
                    let val = match param.param_type.as_str() {
                        "integer" => n(&param.name, 0).to_string(),
                        "boolean" => b(&param.name, false).to_string(),
                        _ => s(&param.name),
                    };
                    cmd_parts.push(format!("{}={}", param.name, val));
                }
                let command = cmd_parts.join(" ");

                let perms = crate::script::PermissionSet::all();
                match mgr.call(&tool_def.plugin_name, &command, &perms, 30_000) {
                    Ok(output) => format!("[ai:{}] {}", name, output),
                    Err(e) => format!("[!] 插件AI工具 '{}' 执行失败: {}", name, e),
                }
            }
            None => format!("[!] 插件管理器未初始化 — AI工具 '{}' 不可用", name),
        };

        crate::telemetry::push_tool_start(name);
        return result;
    }

    // ★ v5.2: 降级检查通用命令注册表 (插件命令 — 不用 is_ai_tool)
    if let Some(cmd_def) = with_global_cmd_registry(|reg| reg.find(name).cloned()).flatten() {
        if !cmd_def.plugin_name.is_empty() && !cmd_def.is_builtin {
            if let Some(mgr_arc) = global_plugin_mgr() {
                let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                // 构建位置参数: "cmd arg1 arg2 ..."
                let mut cmd_parts = vec![name.to_string()];
                if let Some(obj) = args.as_object() {
                    for (_, v) in obj {
                        match v {
                            Value::String(s) => cmd_parts.push(s.clone()),
                            Value::Number(n) => cmd_parts.push(n.to_string()),
                            Value::Bool(b) => cmd_parts.push(b.to_string()),
                            _ => {}
                        }
                    }
                }
                let command = cmd_parts.join(" ");
                let perms = crate::script::PermissionSet::all();
                match mgr.call(&cmd_def.plugin_name, &command, &perms, 30_000) {
                    Ok(output) => {
                        crate::telemetry::push_tool_start(name);
                        return format!("[plugin:{}] {}", cmd_def.plugin_name, output);
                    }
                    Err(_e) => {
                        // 插件调用失败, 继续走硬编码兜底
                    }
                }
            }
        }
    }

    let result = match name {
        
        // ═══ 系统 — 真实查询 ═══
        "system_info" => {
            crate::tools::system_info().join("\n")
        }

        "process_list" => {
            if let Err(e) = check_tool_perm(AiPermLevel::System, "process_list") { return e; }
            let filter = s("filter");
            let f = if filter.is_empty() { None } else { Some(filter.as_str()) };
            crate::tools::process_list(f).join("\n")
        }

        "network_interface_info" => {
            crate::tools::network_interfaces().join("\n")
        }

        "disk_usage" => {
            crate::tools::disk_usage().join("\n")
        }

        // ═══ 工具类 — 真实计算 ═══
        "subnet_calc" => {
            let cidr = s("cidr");
            match crate::tools::subnet_calc(&cidr) {
                Ok(info) => format!("子网 {} — 网络:{} 广播:{} 掩码:{} 可用:{}~{} (共{}个)", 
                    cidr, info.network, info.broadcast, info.mask, info.first_host, info.last_host, info.usable_hosts),
                Err(e) => format!("子网计算失败: {}", e),
            }
        }

        "generate_uuid" => {
            let uuid = format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
                rand::random::<u32>(),
                rand::random::<u32>() & 0xFFFF,
                rand::random::<u32>() & 0xFFF,
                (rand::random::<u32>() & 0x3FFF) | 0x8000,
                rand::random::<u64>() & 0xFFFFFFFFFFFF
            );
            format!("UUIDv4: {}", uuid)
        }

        "checksum_file" => {
            let filename = s("filename");
            let algo = s("algorithm");
            let resolved = crate::tools::files::resolve_path(&filename);
            match crate::tools::file_hash(&resolved, &algo) {
                Ok(hash) => format!("文件哈希 ({}): {}", algo.to_uppercase(), hash),
                Err(e) => format!("文件哈希失败: {}", e),
            }
        }

        "mime_detect" => {
            let filename = s("filename");
            let resolved = crate::tools::files::resolve_path(&filename);
            match crate::tools::mime_detect(&resolved) {
                Ok(mime) => format!("MIME类型: {}", mime),
                Err(e) => format!("MIME检测失败: {}", e),
            }
        }

        "extract_pattern" => {
            let text = s("text");
            let ptype = s("type");
            crate::tools::extract_pattern(&text, &ptype)
        }

        "validate_json" => {
            let json_str = s("json_string");
            match serde_json::from_str::<Value>(&json_str) {
                Ok(_) => "JSON有效".into(),
                Err(e) => format!("JSON无效: {}", e),
            }
        }

        "format_json" => {
            let json_str = s("json_string");
            match serde_json::from_str::<Value>(&json_str) {
                Ok(v) => match serde_json::to_string_pretty(&v) {
                    Ok(formatted) => formatted,
                    Err(e) => format!("JSON格式化失败: {}", e),
                },
                Err(e) => format!("JSON无效: {}", e),
            }
        }

        "regex_tester" => {
            let pattern = s("pattern");
            let text = s("text");
            crate::tools::regex_test(&pattern, &text)
        }

        "unit_converter" => {
            let value = s("value");
            let from = s("from_unit");
            let to = s("to_unit");
            match value.parse::<f64>() {
                Ok(v) => crate::tools::unit_convert(v, &from, &to),
                Err(_) => format!("无效数值: {}", value),
            }
        }

        "color_convert" => {
            let color = s("color");
            crate::tools::color_convert(&color)
        }

        "markdown_to_html" => {
            let md = s("markdown_text");
            crate::tools::markdown_to_html(&md)
        }

        // ═══ 文件操作 ═══
        "pwd" => crate::tools::pwd(),
        "cd" => {
            let path = s("path");
            match crate::tools::cd(&path) {
                Ok(p) => format!("已切换: {}", p),
                Err(e) => format!("cd失败: {}", e),
            }
        }
        "ls" => {
            let path = s("path");
            let p = if path.is_empty() { None } else { Some(path.as_str()) };
            match crate::tools::ls(p) {
                Ok(entries) => entries.join("\n"),
                Err(e) => format!("ls失败: {}", e),
            }
        }
        "read_file" => {
            let path = s("path");
            match crate::tools::read_file(&path) {
                Ok(content) => content,
                Err(e) => format!("读取失败: {}", e),
            }
        }
        "read_file_lines" => {
            let path = s("path");
            let start = n("start", 1) as usize;
            let end = args.get("end").and_then(|v| v.as_i64()).map(|v| v as usize);
            match crate::tools::read_file_lines(&path, start, end) {
                Ok(content) => content,
                Err(e) => format!("读取失败: {}", e),
            }
        }
        "write_file" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "write_file") { return e; }
            let path = match req("path") { Ok(p) => p, Err(e) => { return e; } };
            let content = s("content");
            // ★ BUG-9: content为空时仍执行会清空文件，增加明确警告
            if content.is_empty() {
                return "⚠ write_file 拒绝执行: content 参数为空，将清空文件内容。如需清空请显式传入非空字符串后覆盖。".into();
            }
            match crate::tools::write_file(&path, &content) {
                Ok(msg) => msg,
                Err(e) => format!("写入失败: {}", e),
            }
        }
        "append_file" => {
            let path = s("path");
            let content = s("content");
            match crate::tools::append_file(&path, &content) {
                Ok(msg) => msg,
                Err(e) => format!("追加失败: {}", e),
            }
        }
        "create_file" => {
            let path = s("path");
            let content = s("content");
            let c = if content.is_empty() { None } else { Some(content.as_str()) };
            match crate::tools::create_file(&path, c) {
                Ok(msg) => msg,
                Err(e) => format!("创建失败: {}", e),
            }
        }
        "delete_file" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "delete_file") { return e; }
            let path = s("path");
            match crate::tools::delete_file(&path) {
                Ok(msg) => msg,
                Err(e) => format!("删除失败: {}", e),
            }
        }
        "copy_file" => {
            let src = s("source");
            let dst = s("target");
            match crate::tools::copy_file(&src, &dst) {
                Ok(msg) => msg,
                Err(e) => format!("复制失败: {}", e),
            }
        }
        "move_file" => {
            let src = s("source");
            let dst = s("target");
            match crate::tools::move_file(&src, &dst) {
                Ok(msg) => msg,
                Err(e) => format!("移动失败: {}", e),
            }
        }
        "mkdir" => {
            let path = s("path");
            match crate::tools::mkdir(&path) {
                Ok(msg) => msg,
                Err(e) => format!("mkdir失败: {}", e),
            }
        }
        "rmdir" => {
            let path = s("path");
            match crate::tools::rmdir(&path) {
                Ok(msg) => msg,
                Err(e) => format!("rmdir失败: {}", e),
            }
        }
        "stat" => {
            let path = s("path");
            match crate::tools::stat(&path) {
                Ok(info) => info,
                Err(e) => format!("stat失败: {}", e),
            }
        }
        "hex_dump" => {
            let path = s("path");
            let max = n("max_bytes", 1024) as usize;
            let off = n("offset", 0) as usize;
            match crate::tools::hex_dump(&path, max, off) {
                Ok(dump) => dump,
                Err(e) => format!("hexdump失败: {}", e),
            }
        }
        "find_files" => {
            let pattern = s("pattern");
            let path = s("path");
            let p = if path.is_empty() { None } else { Some(path.as_str()) };
            match crate::tools::find_files(&pattern, p) {
                Ok(files) => files.join("\n"),
                Err(e) => format!("find失败: {}", e),
            }
        }
        "grep" => {
            let pattern = s("pattern");
            let path = s("path");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::tools::grep(&pattern, &path, ci, 200) {
                Ok(lines) => lines.join("\n"),
                Err(e) => format!("grep失败: {}", e),
            }
        }
        "du" => {
            let path = s("path");
            let p = if path.is_empty() { None } else { Some(path.as_str()) };
            match crate::tools::du(p) {
                Ok(info) => info,
                Err(e) => format!("du失败: {}", e),
            }
        }
        "find_replace_in_file" => {
            let path = s("path");
            let find = s("find");
            let replace = s("replace");
            match crate::tools::find_replace(&path, &find, &replace, false, false) {
                Ok(msg) => msg,
                Err(e) => format!("替换失败: {}", e),
            }
        }
        "deduplicate_lines" => {
            let path = s("path");
            let ci = args.get("ignore_case").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::tools::dedup(&path, ci) {
                Ok(msg) => msg,
                Err(e) => format!("去重失败: {}", e),
            }
        }
        "sort_lines" => {
            let path = s("path");
            let reverse = args.get("reverse").and_then(|v| v.as_bool()).unwrap_or(false);
            let numeric = args.get("numeric").and_then(|v| v.as_bool()).unwrap_or(false);
            let unique = args.get("unique").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::tools::sort_lines(&path, reverse, numeric, unique) {
                Ok(msg) => msg,
                Err(e) => format!("排序失败: {}", e),
            }
        }
        "word_frequency" => {
            let path = s("path");
            let top_n = n("top_n", 20) as usize;
            match crate::tools::word_count(&path, top_n.min(100), true, 2) {
                Ok(result) => result,
                Err(e) => format!("词频统计失败: {}", e),
            }
        }
        "text_stats" => {
            let path = s("path");
            match crate::tools::text_stats(&path) {
                Ok(info) => info,
                Err(e) => format!("统计失败: {}", e),
            }
        }
        "split_file" => {
            let path = s("path");
            let mb = n("chunk_size_mb", 10) as usize;
            match crate::tools::split_file(&path, mb.max(1)) {
                Ok(msg) => msg,
                Err(e) => format!("分割失败: {}", e),
            }
        }
        "edit_file" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "edit_file") { return e; }
            let path = s("path");
            match crate::tools::edit_file(&path) {
                Ok(msg) => msg,
                Err(e) => format!("编辑器错误: {}", e),
            }
        }
        "hex_edit" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "hex_edit") { return e; }
            let path = s("path");
            let offset = args.get("offset").and_then(|v| v.as_f64()).map(|f| f as usize);
            let hex_bytes = args.get("hex_bytes").and_then(|v| v.as_str());
            match crate::tools::hex_edit(&path, offset, hex_bytes) {
                Ok(msg) => msg,
                Err(e) => format!("hexedit错误: {}", e),
            }
        }
        "compile_rust" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "compile_rust") { return e; }
            let source = s("source");
            let output = args.get("output").and_then(|v| v.as_str());
            let release = args.get("release").and_then(|v| v.as_bool()).unwrap_or(false);
            let extras: Vec<String> = args.get("extra_args")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect())
                .unwrap_or_default();
            match crate::tools::files::compile_rust(&source, output, release, &extras) {
                Ok(msg) => msg,
                Err(e) => format!("编译失败: {}", e),
            }
        }

        // ═══ 多语言编译器 v1.0 ═══
        "compiler_list" => {
            crate::tools::compiler::list_compilers()
        }

        "workflow_list" => {
            crate::tools::orchestrator::list_workflows()
        }
        "workflow_run" => {
            let name = s("name");
            let target = s("target");
            match crate::tools::orchestrator::get_workflow(&name, &target) {
                Ok(cmds) => {
                    let mut out = format!("═══ 工作流: {} → {} ═══\n\n", name, target);
                    for (i, cmd) in cmds.iter().enumerate() {
                        out.push_str(&format!("  {}. {}\n", i + 1, cmd));
                    }
                    out.push_str("\n[*] 请按顺序执行以上命令 (支持管道: cmd1 | cmd2)\n");
                    out
                }
                Err(e) => format!("工作流错误: {}", e),
            }
        }
        "tool_suggest" => {
            let cache = crate::tools::orchestrator::get_cache();
            let guard = cache.lock().unwrap();
            if let Some(last) = guard.last() {
                crate::tools::orchestrator::get_suggestions(&last.tool_name, &last.result)
            } else {
                "缓存为空 — 请先执行一些工具\n建议: port_scan / dns_lookup / whois_lookup".to_string()
            }
        }
        "cache_stats" => {
            crate::tools::orchestrator::cache_stats()
        }

    

        // ═══ 高级文件操作 (v5.1 新增) ═══
        "tail_file" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "tail_file") { return e; }
            let path = s("path");
            let lines = n("lines", 10) as usize;
            match crate::tools::files::tail_file(&path, lines) {
                Ok(s) => s,
                Err(e) => format!("[!] tail_file失败: {}", e),
            }
        }
        "head_file" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "head_file") { return e; }
            let path = s("path");
            let lines = n("lines", 10) as usize;
            match crate::tools::files::head_file(&path, lines) {
                Ok(s) => s,
                Err(e) => format!("[!] head_file失败: {}", e),
            }
        }
        "diff_files" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "diff_files") { return e; }
            let file1 = s("file1");
            let file2 = s("file2");
            match crate::tools::files::diff_files(&file1, &file2) {
                Ok(s) => s,
                Err(e) => format!("[!] diff_files失败: {}", e),
            }
        }
        "insert_lines" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "insert_lines") { return e; }
            let path = s("path");
            let line = n("line", 1) as usize;
            let content = s("content");
            match crate::tools::files::insert_lines(&path, line, &content) {
                Ok(s) => s,
                Err(e) => format!("[!] insert_lines失败: {}", e),
            }
        }
        "delete_lines_range" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "delete_lines_range") { return e; }
            let path = s("path");
            let start = n("start", 1) as usize;
            let end = n("end", 1) as usize;
            match crate::tools::files::delete_lines_range(&path, start, end) {
                Ok(s) => s,
                Err(e) => format!("[!] delete_lines失败: {}", e),
            }
        }
        "truncate_file_lines" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "truncate_file_lines") { return e; }
            let path = s("path");
            let lines = n("lines", 10) as usize;
            match crate::tools::files::truncate_file_lines(&path, lines) {
                Ok(s) => s,
                Err(e) => format!("[!] truncate_file失败: {}", e),
            }
        }
        "merge_files" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "merge_files") { return e; }
            let sources_str = s("sources");
            let target = s("target");
            let sources: Vec<String> = sources_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if sources.is_empty() { return "[!] merge_files: 需要至少一个源文件".into(); }
            match crate::tools::files::merge_files(&sources, &target) {
                Ok(s) => s,
                Err(e) => format!("[!] merge_files失败: {}", e),
            }
        }
        "grep_recursive" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "grep_recursive") { return e; }
            let pattern = s("pattern");
            let dir = s("dir");
            let case_insensitive = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::tools::files::grep_recursive(&pattern, &dir, case_insensitive) {
                Ok(lines) => lines.join("\n"),
                Err(e) => format!("[!] grep_recursive失败: {}", e),
            }
        }
        "batch_replace" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "batch_replace") { return e; }
            let dir = s("dir");
            let find = s("find");
            let replace = s("replace");
            let file_pattern = s("pattern");
            let case_insensitive = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::tools::files::batch_replace(&dir, &find, &replace, &file_pattern, case_insensitive) {
                Ok(s) => s,
                Err(e) => format!("[!] batch_replace失败: {}", e),
            }
        }
        "file_permissions" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "file_permissions") { return e; }
            let path = s("path");
            match crate::tools::files::file_permissions(&path) {
                Ok(s) => s,
                Err(e) => format!("[!] file_permissions失败: {}", e),
            }
        }
        "binary_read" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "binary_read") { return e; }
            let path = s("path");
            let offset = n("offset", 0) as usize;
            let length = n("length", 256) as usize;
            match crate::tools::files::binary_read(&path, offset, length) {
                Ok(s) => s,
                Err(e) => format!("[!] binary_read失败: {}", e),
            }
        }
        "count_lines" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "count_lines") { return e; }
            let path = s("path");
            match crate::tools::files::count_lines(&path) {
                Ok(s) => s,
                Err(e) => format!("[!] count_lines失败: {}", e),
            }
        }

        // ═══ 大文件流式操作 (v4.8) ═══
        "stream_grep" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "stream_grep") { return e; }
            let path = s("path");
            let pattern = s("pattern");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            let max_matches = n("max_matches", 100) as usize;
            let ctx = n("context_lines", 0) as usize;
            match crate::tools::files::stream_grep(&path, &pattern, ci, max_matches, ctx) {
                Ok(s) => s,
                Err(e) => format!("[!] stream_grep失败: {}", e),
            }
        }
        "stream_binary_scan" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "stream_binary_scan") { return e; }
            let path = s("path");
            let hex_pattern = s("hex_pattern");
            let max_results = n("max_results", 50) as usize;
            match crate::tools::files::stream_binary_scan(&path, &hex_pattern, max_results) {
                Ok(s) => s,
                Err(e) => format!("[!] stream_binary_scan失败: {}", e),
            }
        }
        "range_read" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "range_read") { return e; }
            let path = s("path");
            let offset = n("offset", 0) as u64;
            let length = n("length", 4096) as usize;
            match crate::tools::files::range_read(&path, offset, length) {
                Ok(s) => s,
                Err(e) => format!("[!] range_read失败: {}", e),
            }
        }
        "stream_find_replace" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "stream_find_replace") { return e; }
            let path = s("path");
            let find = s("find");
            let replace = s("replace");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            // 默认最多替换10000处，传0表示无限制(危险)
            let max_r = args.get("max_replacements").and_then(|v| v.as_i64()).unwrap_or(10000) as usize;
            match crate::tools::files::stream_find_replace(&path, &find, &replace, ci, max_r) {
                Ok(s) => s,
                Err(e) => format!("[!] stream_find_replace失败: {}", e),
            }
        }
        "build_line_index" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "build_line_index") { return e; }
            let path = s("path");
            match crate::tools::files::build_line_index(&path) {
                Ok(s) => s,
                Err(e) => format!("[!] build_line_index失败: {}", e),
            }
        }
        "seek_line" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "seek_line") { return e; }
            let path = s("path");
            let line_start = n("line_start", 1) as u64;
            let line_end = args.get("line_end").and_then(|v| v.as_i64()).map(|v| v as u64);
            match crate::tools::files::seek_line(&path, line_start, line_end) {
                Ok(s) => s,
                Err(e) => format!("[!] seek_line失败: {}", e),
            }
        }
        "query_line_index" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "query_line_index") { return e; }
            let path = s("path");
            match crate::tools::files::query_line_index(&path) {
                Ok(s) => s,
                Err(e) => format!("[!] query_line_index失败: {}", e),
            }
        }
        "stream_file_info" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "stream_file_info") { return e; }
            let path = s("path");
            match crate::tools::files::stream_file_info(&path) {
                Ok(s) => s,
                Err(e) => format!("[!] stream_file_info失败: {}", e),
            }
        }

        // ═══ 大文件精准操作 (v4.9) ═══
        "regex_search" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "regex_search") { return e; }
            let path = s("path");
            let regex_pattern = s("regex_pattern");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            let mm = n("max_matches", 100) as usize;
            let ctx = args.get("context_lines").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
            match crate::tools::files::regex_search(&path, &regex_pattern, ci, mm, ctx) {
                Ok(s) => s,
                Err(e) => format!("[!] regex_search失败: {}", e),
            }
        }
        "byte_range_replace" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "byte_range_replace") { return e; }
            let path = s("path");
            let offset = n("offset", 0) as u64;
            let hex_bytes = s("hex_bytes");
            match crate::tools::files::byte_range_replace(&path, offset, &hex_bytes) {
                Ok(s) => s,
                Err(e) => format!("[!] byte_range_replace失败: {}", e),
            }
        }
        "stream_multi_replace" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "stream_multi_replace") { return e; }
            let path = s("path");
            let pairs_json = s("pairs_json");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            let max_r = n("max_replacements", 10000) as usize;
            match crate::tools::files::stream_multi_replace(&path, &pairs_json, ci, max_r) {
                Ok(s) => s,
                Err(e) => format!("[!] stream_multi_replace失败: {}", e),
            }
        }
        "search_export" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "search_export") { return e; }
            let path = s("path");
            let pattern = s("pattern");
            let output_path = s("output_path");
            let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
            let ctx = args.get("context_lines").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
            match crate::tools::files::search_export(&path, &pattern, &output_path, ci, ctx) {
                Ok(s) => s,
                Err(e) => format!("[!] search_export失败: {}", e),
            }
        }
        "column_extract" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "column_extract") { return e; }
            let path = s("path");
            let columns = s("columns");
            let delimiter = s("delimiter");
            let max_rows = n("max_rows", 500) as usize;
            match crate::tools::files::column_extract(&path, &columns, &delimiter, max_rows) {
                Ok(s) => s,
                Err(e) => format!("[!] column_extract失败: {}", e),
            }
        }
        "binary_diff" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "binary_diff") { return e; }
            let file1 = s("file1");
            let file2 = s("file2");
            let max_diffs = n("max_diffs", 50) as usize;
            match crate::tools::files::binary_diff(&file1, &file2, max_diffs) {
                Ok(s) => s,
                Err(e) => format!("[!] binary_diff失败: {}", e),
            }
        }
        "file_chunk_scan" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "file_chunk_scan") { return e; }
            let path = s("path");
            let chunk_size_mb = n("chunk_size_mb", 64) as usize;
            let max_chunks = n("max_chunks", 50) as usize;
            match crate::tools::files::file_chunk_scan(&path, chunk_size_mb, max_chunks) {
                Ok(s) => s,
                Err(e) => format!("[!] file_chunk_scan失败: {}", e),
            }
        }

        // ═══ AI 专属明文存储 (v4.5) ═══
        "ai_save" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "ai_save") { return e; }
            let filename = s("filename");
            let content = s("content");
            if filename.is_empty() { return "[!] ai_save: 需要文件名".into(); }
            // 安全检查: 禁止路径遍历
            if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
                return "[!] ai_save: 文件名不能包含路径分隔符".into();
            }
            let storage_dir = std::path::PathBuf::from(crate::tools::files::resolve_path("ai_storage"));
            let _ = std::fs::create_dir_all(&storage_dir);
            let file_path = storage_dir.join(&filename);
            match std::fs::write(&file_path, &content) {
                Ok(()) => format!("[+] ai_save: {} ({:.1}KB) 已保存到 ai_storage/", filename, content.len() as f64 / 1024.0),
                Err(e) => format!("[!] ai_save 失败: {}", e),
            }
        }
        "ai_read" => {
            let filename = s("filename");
            if filename.is_empty() { return "[!] ai_read: 需要文件名".into(); }
            if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
                return "[!] ai_read: 文件名不能包含路径分隔符".into();
            }
            let storage_dir = std::path::PathBuf::from(crate::tools::files::resolve_path("ai_storage"));
            let file_path = storage_dir.join(&filename);
            match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    let lines = content.lines().count();
                    format!("[ai_storage] {} ({}行, {:.1}KB):\n{}", filename, lines, content.len() as f64 / 1024.0, content)
                }
                Err(e) => format!("[!] ai_read 失败: {} — 文件 '{}' 不存在或无法读取", e, filename),
            }
        }
        "ai_list" => {
            let storage_dir = std::path::PathBuf::from(crate::tools::files::resolve_path("ai_storage"));
            if !storage_dir.exists() {
                return "[ai_storage] 目录为空 — 使用 ai_save 存储信息".into();
            }
            let mut entries: Vec<String> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&storage_dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                        entries.push(format!("  {}  ({:.1}KB)", name, size as f64 / 1024.0));
                    }
                }
            }
            if entries.is_empty() {
                "[ai_storage] 目录为空 — 使用 ai_save 存储重要信息".into()
            } else {
                entries.sort();
                format!("[ai_storage] 共 {} 个文件:\n{}", entries.len(), entries.join("\n"))
            }
        }

        // ═══ Vault 加密数据库 ═══
        "vault_set" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "vault_set") { return e; }
            let ns = s("namespace");
            let key = s("key");
            let val = s("value");
            match crate::vault::vault_set(&ns, &key, &val) {
                Ok(()) => format!("[+] Vault: {}.{} = (已加密保存)", ns, key),
                Err(e) => format!("Vault写入失败: {}", e),
            }
        }
        "vault_get" => {
            let ns = s("namespace");
            let key = s("key");
            match crate::vault::vault_get(&ns, &key) {
                Some(v) => {
                    if key.contains("api_key") || key.contains("password") || key.contains("secret") {
                        format!("[{}] {}.{} = ***已存储***", ns, ns, key)
                    } else {
                        format!("[{}] {}.{} = {}", ns, ns, key, v)
                    }
                }
                None => format!("[{}] {}.{} 未设置", ns, ns, key),
            }
        }
        "vault_list" => {
            let ns = s("namespace");
            match crate::vault::vault_list(&ns) {
                Some(keys) => format!("Vault {} ({} 键): {}", ns, keys.len(), keys.join(", ")),
                None => format!("Vault {} 命名空间不存在或为空", ns),
            }
        }
        "vault_list_ns" => {
            match crate::vault::vault_list_namespaces() {
                Some(ns) => format!("Vault 命名空间: {}", ns.join(", ")),
                None => "Vault 未挂载".into(),
            }
        }
        "vault_remember" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "vault_remember") { return e; }
            let key = s("key");
            let val = s("value");
            let cat = s("category");
            let full_key = format!("memory.{}.{}", cat, key);
            match crate::vault::vault_set("ai", &full_key, &val) {
                Ok(()) => format!("[+] 记忆已存储: {}/{}", cat, key),
                Err(e) => format!("记忆存储失败: {}", e),
            }
        }
        "vault_recall" => {
            let key = s("key");
            // 尝试所有分类
            let cats = ["target", "preference", "finding", "note"];
            for cat in &cats {
                let full_key = format!("memory.{}.{}", cat, key);
                if let Some(v) = crate::vault::vault_get("ai", &full_key) {
                    return format!("[回忆:{}] {} = {}", cat, key, v);
                }
            }
            format!("[回忆] {} 未找到", key)
        }

        // ═══ 清理 & 上下文管理 ═══
        "clear_history" => {
            CLEAR_HISTORY_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
            "[+] 对话历史清除请求已发出 — 将在当前回复完成后重置\n  [*] 下次对话将从空白上下文开始".into()
        }
        "clear_cache" => {
            let msg = clear_all_cache();
            format!("{}\n[*] 下次查询将不使用缓存，重新获取最新数据", msg)
        }
        "abort_tool" => {
            TOOL_ABORT_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
            ("[+] 工具强制中断信号已发送\n  [*] 当前正在执行的工具将被线程超时(120s)自动回收\n  [*] 后续排队的工具调用已全部跳过\n  [!] 如果当前工具卡在系统调用，最多等待120秒\n  [!] 如需立即恢复，可在终端输入 abort_tool 再次确认").to_string()
        }
        "clear_context" => {
            CLEAR_HISTORY_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
            let cache_msg = clear_all_cache();
            format!(
                "[+] 上下文已完全清除:\n  [*] 对话历史 → 已重置\n  {}\n  [*] 下次对话将使用全新上下文",
                cache_msg
            )
        }

        // ═══ 终端交互 ═══
        "get_terminal_output" => {
            let text = get_terminal_output_text();
            let line_count = text.lines().count();
            format!("终端输出 ({} 行):\n{}", line_count, text)
        }



        // ═══ 插件/脚本/驱动 (v4.1: AI 可调用, 需要最高权限) ═══

        "plugin_load" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_load") { return e; }
            let name = s("name");
            let path = s("path");

            if name.is_empty() || path.is_empty() {
                return "[!] 用法: plugin_load <name> <path> — 例: plugin_load myplug ./plugins/scanner.dll".into();
            }

            if path.contains("..") {
                return format!("[!] 安全拦截: 路径包含非法字符 '..' — 路径遍历攻击被阻止");
            }

            // ★ v9.0: 使用统一全局 PluginManager 单例
            let mgr_arc = global_plugin_mgr()
                .unwrap_or_else(|| panic!("PluginManager 未初始化"));
            let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());

            // 如果已加载，先卸载旧版本
            let _ = mgr.unload(&name);

            match mgr.load(&name, &path) {
                Ok(msg) => {
                    // ── ★ 探测 ruoo_plugin_commands 并注册到全局 CommandRegistry ──
                    let mut cmd_guard = global_cmd_registry();
                    if cmd_guard.is_none() { *cmd_guard = Some(crate::plugin::CommandRegistry::new()); }
                    let cmd_reg = cmd_guard.as_mut().unwrap();
                    // 先注销旧命令
                    cmd_reg.unregister_plugin(&name);

                    let reg_result = crate::plugin::try_register_from_file(&path, &name, cmd_reg);

                    let perms = crate::script::PermissionSet::all();
                    let ping_result = mgr.call(&name, "ping", &perms, 5000)
                        .unwrap_or_else(|e| format!("(ping失败但插件已加载: {})", e));

                    let mut out = format!(
                        "[+] 插件已加载: {}\n  {} | ping → {}\n  [!] ⚠ 原生代码可完全突破沙箱 — 仅信任审计过的插件",
                        name, msg, ping_result
                    );

                    match reg_result {
                        Ok(count) if count > 0 => {
                            let cmd_list: Vec<String> = cmd_reg.list_by_plugin(&name)
                                .iter()
                                .map(|c| format!("  {} — {}", c.cmd, c.usage))
                                .collect();
                            out.push_str(&format!(
                                "\n[plugin] ✅ 已注册 {} 个命令:\n{}\n[*] 直接在终端输入命令名即可使用",
                                count, cmd_list.join("\n")
                            ));
                        }
                        Ok(_) => { out.push_str("\n[plugin] 无命令声明 (可用 plugin call 调用)"); }
                        Err(e) => { out.push_str(&format!("\n[plugin] 命令探测: {}", e)); }
                    }
                    out
                }
                Err(e) => format!("[!] 插件加载失败: {}", e),
            }
        }

        "plugin_call" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_call") { return e; }
            let name = s("name");
            let command = s("command");
            if name.is_empty() || command.is_empty() {
                return "[!] 用法: plugin_call <name> <command> — 例: plugin_call myplug \"scan 192.168.1.1\"".into();
            }
            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    let perms = crate::script::PermissionSet::all();
                    match mgr.call(&name, &command, &perms, 30_000) {
                        Ok(result) => format!("[plugin:{}] {}", name, result),
                        Err(e) => format!("[!] 调用失败: {}", e),
                    }
                }
                None => "[!] 无已加载插件 — 请先 plugin_load".into(),
            }
        }

        "plugin_unload" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_unload") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: plugin_unload <name>".into();
            }
            // v6.1 fix: 先卸载, 成功后注销命令 (卸载失败则保留命令, 防止僵尸状态)
            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    match mgr.unload(&name) {
                        Ok(()) => {
                            let mut cmd_guard = global_cmd_registry();
                            if let Some(ref mut reg) = *cmd_guard {
                                reg.unregister_plugin(&name);
                            }
                            drop(cmd_guard);
                            crate::plugin::unregister_ai_tools(&name);
                            format!("[+] 插件已卸载: {} — 命令+AI工具已注销", name)
                        }
                        Err(e) => format!("[!] 卸载失败: {} — 命令和AI工具已保留", e),
                    }
                },
                None => "[!] 无已加载插件".into(),
            }
        }

        "plugin_list" => {

            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    let details = mgr.list_detailed();
                    if details.is_empty() {
                        "[plugin] 无已加载插件".into()
                    } else {
                        let mut out = String::from("[plugin] ====== 已加载插件 ======\n");
                        for d in &details { out.push_str(&format!("{}\n", d)); }
                        // 也显示已注册命令
                        let cmd_guard = global_cmd_registry();
                        if let Some(ref reg) = *cmd_guard {
                            let help = crate::plugin::plugin_help_text(reg);
                            for h in &help { out.push_str(&format!("{}\n", h)); }
                        }
                        // ★ 显示健康状态
                        out.push_str("\n── 健康状态 ──\n");
                        let health = mgr.plugin_health_summary();
                        out.push_str(&health);
                        out
                    }
                }
                None => "[plugin] 无已加载插件".into(),
            }
        }

        // ── 插件防崩溃 v5.0 ──
        "plugin_force_unload" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_force_unload") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: plugin_force_unload <name> — 强制卸载崩溃/挂起的插件".into();
            }
            // v6.1 fix: 先检查插件是否存在, 再卸载+注销命令

            let result = match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    match mgr.force_unload(&name) {
                        Ok(msg) => {
                            // 只有成功卸载后才注销命令和AI工具
                            let mut cmd_guard = global_cmd_registry();
                            if let Some(ref mut reg) = *cmd_guard {
                                reg.unregister_plugin(&name);
                            }
                            drop(cmd_guard);
                            crate::plugin::unregister_ai_tools(&name);
                            format!("[+] {}", msg)
                        }
                        Err(e) => format!("[!] 强制卸载失败: {}", e),
                    }
                },
                None => format!("[!] 插件 '{}' 未加载 — 无需卸载", name),
            };
            result
        }

        "plugin_reload" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_reload") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: plugin_reload <name> — 热重载指定插件 (卸载旧版→加载新版, 失败自动回滚)".into();
            }

            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    match mgr.hotload(&name) {
                        Ok(msg) => format!("[↻] 热重载成功: {}", msg),
                        Err(e) => format!("[!] 热重载失败: {}", e),
                    }
                },
                None => format!("[!] 插件 '{}' 未加载 — 无法热重载", name),
            }
        }

        "plugin_crash_recover" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_crash_recover") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: plugin_crash_recover <name> — 自动诊断并恢复崩溃插件".into();
            }

            let result = match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    let (report, success) = mgr.crash_recover(&name);
                    if success {
                        let mut cmd_guard = global_cmd_registry();
                        if let Some(ref mut reg) = *cmd_guard {
                            reg.unregister_plugin(&name);
                        }
                        drop(cmd_guard);
                        crate::plugin::unregister_ai_tools(&name);
                    }
                    report
                }
                None => format!("[*] 插件 '{}' 未加载, 无需恢复", name),
            };
            result
        }

        // ── v7.1: 插件软重启 ──
        "plugin_restart" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_restart") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: plugin_restart <name> — 软重启崩溃插件 (卸载→重载→注册命令→重置熔断器)".into();
            }
            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    let mut cmd_guard = global_cmd_registry();
                    if cmd_guard.is_none() { *cmd_guard = Some(crate::plugin::CommandRegistry::new()); }
                    let cmd_reg = cmd_guard.as_mut().unwrap();
                    let report = mgr.soft_restart(&name, cmd_reg);
                    drop(cmd_guard);
                    report
                }
                None => format!("[!] PluginManager 未初始化 — 插件 '{}' 无法软重启", name),
            }
        }

        "plugin_recover_all" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_recover_all") { return e; }
            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mut mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    let mut cmd_guard = global_cmd_registry();
                    if cmd_guard.is_none() { *cmd_guard = Some(crate::plugin::CommandRegistry::new()); }
                    let cmd_reg = cmd_guard.as_mut().unwrap();
                    let report = mgr.auto_recover_all(cmd_reg);
                    drop(cmd_guard);
                    report
                }
                None => "[!] PluginManager 未初始化".into(),
            }
        }

        "plugin_health" => {
            let name = s("name");

            match global_plugin_mgr() {
                Some(mgr_arc) => {
                    let mgr = mgr_arc.lock().unwrap_or_else(|e| e.into_inner());
                    if name.is_empty() {
                        mgr.plugin_health_summary()
                    } else {
                        mgr.plugin_health_report(&name)
                    }
                }
                None => "[plugin] 无已加载插件".into(),
            }
        }

        // ── 插件注册表 v5.0 ──
        "plugin_reg" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_reg") { return e; }
            let alias = s("alias");
            let path = s("path");
            let auto_load = b("auto_load", true);
            let load_order = u("load_order", 0u32) as u32;

            if alias.is_empty() || path.is_empty() {
                return "[!] 用法: plugin_reg <alias> <path> [auto_load=true] [load_order=0]".into();
            }
            if path.contains("..") {
                return format!("[!] 安全拦截: 路径包含非法字符 '..'");
            }

            // 通过 app 的 plugin_mgr (需要访问 app 的注册表)
            match crate::ai::with_app_plugin_registry(|reg| {
                reg.register(&alias, &path, auto_load, load_order)
            }) {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => format!("[!] 注册失败: {}", e),
                None => "[!] 插件注册表未挂载 — 请先登录Vault".into(),
            }
        }

        "plugin_unreg" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_unreg") { return e; }
            let alias = s("alias");
            if alias.is_empty() {
                return "[!] 用法: plugin_unreg <alias>".into();
            }
            match crate::ai::with_app_plugin_registry(|reg| reg.unregister(&alias)) {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => format!("[!] 注销失败: {}", e),
                None => "[!] 插件注册表未挂载".into(),
            }
        }

        "plugin_enable" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_enable") { return e; }
            let alias = s("alias");
            if alias.is_empty() {
                return "[!] 用法: plugin_enable <alias>".into();
            }
            match crate::ai::with_app_plugin_registry(|reg| reg.enable(&alias)) {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => format!("[!] 启用失败: {}", e),
                None => "[!] 插件注册表未挂载".into(),
            }
        }

        "plugin_disable" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "plugin_disable") { return e; }
            let alias = s("alias");
            if alias.is_empty() {
                return "[!] 用法: plugin_disable <alias>".into();
            }
            match crate::ai::with_app_plugin_registry(|reg| reg.disable(&alias)) {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => format!("[!] 禁用失败: {}", e),
                None => "[!] 插件注册表未挂载".into(),
            }
        }

        "plugin_reg_list" => {
            match crate::ai::with_app_plugin_registry(|reg| reg.status_report()) {
                Some(report) => report,
                None => "插件注册表: 未挂载".into(),
            }
        }

        "plugin_reg_save" => {
            match crate::ai::with_app_plugin_registry(|reg| {
                reg.save().map(|_| "[+] 插件注册表已保存".to_string())
            }) {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => format!("[!] 保存失败: {}", e),
                None => "[!] 插件注册表未挂载".into(),
            }
        }

        "script_run" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "script_run") { return e; }
            let script_path = s("script_path");
            if script_path.is_empty() {
                return "[!] 用法: script_run <脚本路径> — 例: script_run scripts/myscan.ruoo".into();
            }

            // 路径安全: 拦截 .. 和 \
            if script_path.contains("..") || script_path.contains('\\') {
                return format!("[!] 安全拦截: 路径包含非法字符 — 路径遍历攻击被阻止");
            }

            // 读取脚本文件
            let content = match crate::script::load_script(&script_path) {
                Ok(c) => c,
                Err(e) => return format!("[!] 读取脚本失败: {}", e),
            };

            let lines: Vec<&str> = content.lines().collect();
            if lines.is_empty() {
                return "[!] 脚本为空".into();
            }

            // 创建执行上下文 (全权限 — AI 工具模式)
            let script_dir = std::path::Path::new(&script_path)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            let script_name = std::path::Path::new(&script_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&script_path)
                .to_string();

            let mut ctx = crate::script::ScriptContext::new(script_dir, &script_name);
            ctx.force = true; // AI 调用跳过 permit 检查
            ctx.perms = crate::script::PermissionSet::all(); // AI 拥有全权限

            // 全局超时: 60s
            ctx.global_timeout_secs = 60;
            ctx.start_time = Some(std::time::Instant::now());

            let mut output: Vec<String> = Vec::new();
            let mut i = 0usize;

            // 首先解析所有 #![...] 指令
            for line in &lines {
                let trimmed = line.trim();
                if let Some(dir) = trimmed.strip_prefix("#![") {
                    if let Some(inner) = dir.strip_suffix(']') {
                        let inner = inner.trim();
                        if inner == "watch-plugins" {
                            ctx.plugins.set_watch(true);
                            output.push("[s] 插件热加载已启用".into());
                        } else if inner == "watch-sys" {
                            ctx.sysdrv.set_watch(true);
                            output.push("[s] 驱动热加载监控已启用".into());
                        } else {
                            ctx.perms.apply_directive(inner);
                            output.push(format!("[s] 权限: {}", inner));
                        }
                    }
                }
            }

            // 执行脚本指令 — 只执行 AI 工具支持的子集
            output.push(format!("[*] ======== AI 执行脚本: {} ========", script_path));
            output.push(format!("[*] {} 行指令 | 超时: {}s | 权限: 全权限(AI模式)", lines.len(), ctx.global_timeout_secs));

            while i < lines.len() && ctx.running {
                // 全局超时
                if let Some(start) = ctx.start_time {
                    if start.elapsed().as_secs() > ctx.global_timeout_secs {
                        output.push(format!("[!] ⏱ 脚本超时 ({}s) — 强制终止", ctx.global_timeout_secs));
                        ctx.running = false;
                        ctx.errors += 1;
                        break;
                    }
                }

                let line = lines[i];
                let inst = crate::script::parse_line(line);

                match &inst {
                    crate::script::ScriptInstruction::Noop
                    | crate::script::ScriptInstruction::Directive(_) => {
                        i += 1; continue;
                    }

                    crate::script::ScriptInstruction::Echo(text) => {
                        let expanded = ctx.expand(text);
                        output.push(expanded);
                        ctx.executed += 1;
                    }
                    crate::script::ScriptInstruction::Print(var) => {
                        let val = ctx.vars.get(var).cloned().unwrap_or_default();
                        output.push(format!("  {} = {}", var, val));
                        ctx.executed += 1;
                    }
                    crate::script::ScriptInstruction::Set(key, value) => {
                        let expanded = ctx.expand(value);
                        ctx.vars.insert(key.clone(), expanded);
                        ctx.executed += 1;
                    }
                    crate::script::ScriptInstruction::Sleep(ms) => {
                        let ms = (*ms).max(1).min(60_000); // AI模式最多60秒
                        output.push(format!("[z] 暂停 {}ms...", ms));
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                        ctx.executed += 1;
                    }

                    crate::script::ScriptInstruction::OnError(mode) => {
                        ctx.on_error = mode.clone();
                        output.push(format!("[s] onerror = {}", mode));
                        ctx.executed += 1;
                    }

                    crate::script::ScriptInstruction::LoadPlugin(name, path) => {
                        match ctx.perms.check_plugin() {
                            Ok(()) => {
                                let expanded = ctx.expand(path);
                                if expanded.contains("..") {
                                    output.push(format!("[!] 插件路径包含非法字符: .. — 已拦截"));
                                    ctx.errors += 1;
                                } else {
                                    match ctx.plugins.load(name, &expanded) {
                                        Ok(msg) => {
                                            output.push(msg);
                                            output.push("[!] ⚠ 原生代码可完全突破沙箱".into());
                                            ctx.executed += 1;
                                        }
                                        Err(e) => {
                                            output.push(format!("[!] 插件加载失败: {}", e));
                                            ctx.errors += 1;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                output.push(format!("[!] {}", e));
                                ctx.errors += 1;
                            }
                        }
                    }

                    crate::script::ScriptInstruction::CallPlugin(plugin, command, timeout_opt) => {
                        match ctx.perms.check_plugin() {
                            Ok(()) => {
                                let expanded = ctx.expand(command);
                                let timeout = timeout_opt
                                    .unwrap_or(ctx.timeout_secs * 1000)
                                    .max(1000).min(300_000);
                                output.push(format!("[plugin] {} → {} (超时{}ms)...", plugin, expanded, timeout));
                                match ctx.plugins.call(plugin, &expanded, &ctx.perms, timeout) {
                                    Ok(result) => {
                                        ctx.vars.insert("PLUGIN_RESULT".into(), result.clone());
                                        output.push(format!("[plugin:{}] {}", plugin, result));
                                        ctx.executed += 1;
                                    }
                                    Err(e) => {
                                        output.push(format!("[!] 插件调用失败: {}", e));
                                        ctx.errors += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                output.push(format!("[!] {}", e));
                                ctx.errors += 1;
                            }
                        }
                    }

                    crate::script::ScriptInstruction::UnloadPlugin(plugin) => {
                        match ctx.plugins.unload(plugin) {
                            Ok(()) => {
                                output.push(format!("[s] 插件已卸载: {}", plugin));
                                ctx.executed += 1;
                            }
                            Err(e) => {
                                output.push(format!("[!] 卸载失败: {}", e));
                                ctx.errors += 1;
                            }
                        }
                    }

                    crate::script::ScriptInstruction::ListPlugins => {
                        let details = ctx.plugins.list_detailed();
                        if details.is_empty() {
                            output.push("[s] 无已加载插件".into());
                        } else {
                            output.push("[s] ====== 已加载插件 ======".into());
                            for d in &details { output.push(d.clone()); }
                        }
                        ctx.executed += 1;
                    }

                    crate::script::ScriptInstruction::SysLoad(name, path) => {
                        match ctx.perms.check_sys() {
                            Ok(()) => {
                                let expanded_path = ctx.expand(path);
                                if expanded_path.contains("..") {
                                    output.push("[!] 驱动路径包含非法字符: .. — 已拦截".into());
                                    ctx.errors += 1;
                                } else {
                                    output.push(format!("[⚙] 加载内核驱动: {} ({})...", name, expanded_path));
                                    let mut mgr = crate::script::SysDriverManager::new();
                                    match mgr.load(name, &expanded_path) {
                                        Ok(msg) => {
                                            output.push(msg);
                                            output.push(format!("[!] ⚡ 内核驱动已加载: {} — ring 0 权限", name));
                                            ctx.executed += 1;
                                        }
                                        Err(e) => {
                                            output.push(format!("[!] 驱动加载失败: {}", e));
                                            ctx.errors += 1;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                output.push(format!("[!] {}", e));
                                ctx.errors += 1;
                            }
                        }
                    }

                    crate::script::ScriptInstruction::SysUnload(name) => {
                        match ctx.perms.check_sys() {
                            Ok(()) => {
                                match ctx.sysdrv.unload(name) {
                                    Ok(()) => {
                                        output.push(format!("[⚙] 驱动已卸载: {}", name));
                                        ctx.executed += 1;
                                    }
                                    Err(e) => {
                                        output.push(format!("[!] 驱动卸载失败: {}", e));
                                        ctx.errors += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                output.push(format!("[!] {}", e));
                                ctx.errors += 1;
                            }
                        }
                    }

                    crate::script::ScriptInstruction::SysList => {
                        let details = ctx.sysdrv.list_detailed();
                        if details.is_empty() {
                            output.push("[⚙] 无已加载内核驱动".into());
                        } else {
                            output.push("[⚙] ====== 已加载内核驱动 ======".into());
                            for d in &details { output.push(d.clone()); }
                        }
                        ctx.executed += 1;
                    }

                    crate::script::ScriptInstruction::SysStatus(name) => {
                        match ctx.sysdrv.status(name) {
                            Ok(s) => { output.push(format!("[⚙] {}", s)); ctx.executed += 1; }
                            Err(e) => { output.push(format!("[!] {}", e)); ctx.errors += 1; }
                        }
                    }

                    // 不支持的指令 — 跳过并警告
                    crate::script::ScriptInstruction::Cmd(_)
                    | crate::script::ScriptInstruction::Run(_)
                    | crate::script::ScriptInstruction::Retry(_, _)
                    | crate::script::ScriptInstruction::Assert(_, _)
                    | crate::script::ScriptInstruction::Try
                    | crate::script::ScriptInstruction::Catch(_)
                    | crate::script::ScriptInstruction::EndBlock
                    | crate::script::ScriptInstruction::IfExists(_, _)
                    | crate::script::ScriptInstruction::EndIf
                    | crate::script::ScriptInstruction::Unset(_)
                    | crate::script::ScriptInstruction::Timeout(_)
                    | crate::script::ScriptInstruction::HotloadPlugin(_)
                    | crate::script::ScriptInstruction::SysHotload(_) => {
                        // AI 模式下不支持的指令 — 静默跳过
                        // (Cmd/Run 需要 App 上下文; try/catch/if 需要嵌套状态机)
                    }
                }
                i += 1;
            }

            // 清理插件
            let plugin_names: Vec<String> = ctx.plugins.list();
            for name in plugin_names {
                if let Err(e) = ctx.plugins.unload(&name) {
                    output.push(format!("[!] 插件卸载失败: {} ({})", name, e));
                }
            }

            // 报告驱动残留
            let sys_names = ctx.sysdrv.list();
            if !sys_names.is_empty() {
                output.push(format!(
                    "[!] ⚠ {} 个内核驱动仍在加载: {} — 需手动 sysunload",
                    sys_names.len(), sys_names.join(", ")
                ));
            }

            output.push(format!(
                "[+] 脚本结束 — {} 指令 / {} 错误 / 权限: {}",
                ctx.executed, ctx.errors,
                if ctx.perms == crate::script::PermissionSet::all() { "全权限(AI)" } else { "受限" }
            ));

            output.join("\n")
        }

        "sysload" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "sysload") { return e; }
            let name = s("name");
            let path = s("path");
            if name.is_empty() || path.is_empty() {
                return "[!] 用法: sysload <名称> <路径> — 例: sysload mydrv ./drivers/mydrv.sys  ⚡ 需管理员/root权限".into();
            }
            if path.contains("..") {
                return format!("[!] 安全拦截: 路径包含非法字符 '..' — 路径遍历攻击被阻止");
            }
            match crate::tools::kernel::kernel_driver_load(&name, &path) {
                Ok(msg) => msg,
                Err(e) => format!("[!] 驱动加载失败: {}", e),
            }
        }

        "sysunload" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "sysunload") { return e; }
            let name = s("name");
            if name.is_empty() {
                return "[!] 用法: sysunload <名称> — 例: sysunload mydrv  ⚡ 需管理员/root权限".into();
            }
            match crate::tools::kernel::kernel_driver_unload(&name) {
                Ok(msg) => msg,
                Err(e) => format!("[!] 驱动卸载失败: {}", e),
            }
        }

        "syslist" => {
            if let Err(e) = check_tool_perm(AiPermLevel::System, "syslist") { return e; }
            match crate::tools::kernel::kernel_driver_list() {
                Ok(msg) => msg,
                Err(e) => e,
            }
        }

        "kernel_compile" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "kernel_compile") { return e; }
            let source = s("source");
            let output = s("output");
            if source.is_empty() || output.is_empty() {
                return "[!] 用法: kernel_compile <源文件> <输出路径> — 例: kernel_compile mydrv.c mydrv.sys".into();
            }
            match crate::tools::kernel::kernel_compile(&source, &output) {
                Ok(msg) => msg,
                Err(e) => format!("[!] 编译失败: {}", e),
            }
        }

        "kernel_template" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "kernel_template") { return e; }
            let name = s("name");
            let platform = s("platform");
            if name.is_empty() {
                return "[!] 用法: kernel_template <名称> <平台> — 例: kernel_template mydrv windows".into();
            }
            match crate::tools::kernel::kernel_template(&name, &platform) {
                Ok(code) => format!("[+] 驱动模板代码 ({} / {}):\n\n{}", name, platform, code),
                Err(e) => e,
            }
        }

        "kernel_validate" => {
            if let Err(e) = check_tool_perm(AiPermLevel::System, "kernel_validate") { return e; }
            let path = s("path");
            if path.is_empty() {
                return "[!] 用法: kernel_validate <路径> — 例: kernel_validate ./drivers/mydrv.sys".into();
            }
            match crate::tools::kernel::kernel_validate(&path) {
                Ok(msg) => msg,
                Err(e) => format!("[!] 验证失败: {}", e),
            }
        }

        "kernel_backend_info" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Safe, "kernel_backend_info") { return e; }
            crate::tools::kernel::kernel_backend_info()
        }

        "kernel_scaffold" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Files, "kernel_scaffold") { return e; }
            let name = s("name");
            let output_dir = s("output_dir");
            let platform = s("platform");
            if name.is_empty() || output_dir.is_empty() || platform.is_empty() {
                return "[!] 用法: kernel_scaffold <名称> <输出目录> <平台> — 例: kernel_scaffold mydrv ./drivers/mydrv_project windows".into();
            }
            match crate::tools::kernel::kernel_scaffold(&name, &output_dir, &platform) {
                Ok(msg) => msg,
                Err(e) => format!("[!] 骨架生成失败: {}", e),
            }
        }

        "exec_cmd" => {
            if let Err(e) = check_tool_perm(AiPermLevel::Dangerous, "exec_cmd") { return e; }
            let command = s("command");
            if command.is_empty() {
                return "[!] 用法: exec_cmd <命令> [shell] — 例: exec_cmd \"cargo check\" cmd".into();
            }

            // ★ BUG-1 修复: Shell命令注入防护 — 黑名单拦截危险元字符
            if let Err(e) = validate_shell_command(&command) {
                return format!("[!] 命令被安全策略拦截: {}\n[*] 提示: 请将命令拆分或使用参数数组形式", e);
            }

            let shell = s("shell").to_lowercase();
            #[cfg(windows)]
            let (shell_cmd, shell_arg) = {
                if shell == "powershell" || shell == "ps" {
                    ("powershell", "-Command")
                } else {
                    ("cmd", "/C")
                }
            };

            #[cfg(not(windows))]
            let (shell_cmd, shell_arg) = {
                match shell.as_str() {
                    "bash" => ("bash", "-c"),
                    "sh" => ("sh", "-c"),
                    _ => ("sh", "-c"),
                }
            };

            // ★ 超时保护: 使用 spawn + wait_timeout 替代无限阻塞的 output()
            let exec_timeout = crate::config::load_config().exec_timeout_ms;
            let timeout_dur = std::time::Duration::from_millis(exec_timeout);

            match std::process::Command::new(shell_cmd)
                .args([shell_arg, &command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(mut child) => {
                    match crate::tools::fault_tolerant::wait_timeout(&mut child, timeout_dur) {
                        Ok(Some(status)) => {
                            let stdout = child.stdout.take()
                                .and_then(|mut s| { use std::io::Read; let mut buf = Vec::new(); s.read_to_end(&mut buf).ok()?; Some(buf) })
                                .unwrap_or_default();
                            let stderr = child.stderr.take()
                                .and_then(|mut s| { use std::io::Read; let mut buf = Vec::new(); s.read_to_end(&mut buf).ok()?; Some(buf) })
                                .unwrap_or_default();
                            let stdout = String::from_utf8_lossy(&stdout);
                            let stderr = String::from_utf8_lossy(&stderr);
                            let code = status.code().unwrap_or(-1);
                            let mut result = format!("退出码: {}\n", code);
                            if !stdout.is_empty() {
                                if stdout.len() > 4000 {
                                    let trunc = stdout.floor_char_boundary(4000);
                                    result.push_str(&format!("STDOUT ({}字节, 截断):\n{}...\n", stdout.len(), &stdout[..trunc]));
                                } else {
                                    result.push_str(&format!("STDOUT:\n{}", stdout));
                                }
                            } else {
                                result.push_str("STDOUT: (空)\n");
                            }
                            if !stderr.is_empty() {
                                let stderr_short = if stderr.len() > 1000 { &stderr[..stderr.floor_char_boundary(1000)] } else { stderr.as_ref() };
                                result.push_str(&format!("STDERR:\n{}", stderr_short));
                            }
                            result
                        }
                        Ok(None) => {
                            // 超时 — 强制杀进程
                            let _ = child.kill();
                            let _ = child.wait();
                            format!("[!] 命令超时 (>{:.0}s), 已强制终止\n[*] 配置: config set exec_timeout_ms <毫秒> (当前={}ms)",
                                timeout_dur.as_secs_f64(), exec_timeout)
                        }
                        Err(e) => format!("[!] 命令等待异常: {}", e),
                    }
                }
                Err(e) => format!("[!] 命令启动失败: {}", e),
            }
        }



        // ═══ AI 辅助 ═══
        "ai_message" => {
            let message = s("message");
            let text = message.replace("\\n", "\n");
            send_ai_message(&text);
            format!("[+] 信息已直出到终端 ({} 字符)", text.len())
        }
        "ai_message_raw" => {
            let message = s("message");
            let text = message.replace("\\n", "\n");
            send_ai_message_raw(&text);
            format!("[+] 裸消息已直出到终端 ({} 字符)", text.len())
        }

        // ═══ 文件操作扩展 v5.9 ═══
        "base64_file" => {
            let path = s("file_path");  // AI可能传file_path
            let path = if path.is_empty() { s("path") } else { path };
            let op = s("operation");
            crate::tools::files::base64_file(&path, &op).unwrap_or_else(|e| e)
        }
        "file_encoding_detect" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::file_encoding_detect(&path).unwrap_or_else(|e| e)
        }
        "json_query" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            let query = s("query");
            crate::tools::files::json_query(&path, &query).unwrap_or_else(|e| e)
        }
        "json_keys" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::json_keys(&path).unwrap_or_else(|e| e)
        }
        "csv_parse" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            let max_rows = args.get("max_rows").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let delim = s("delimiter");
            crate::tools::files::csv_parse(&path, max_rows, &delim).unwrap_or_else(|e| e)
        }
        "batch_rename" => {
            let dir = s("dir");
            let find = s("find");
            let replace = s("replace");
            let dry = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(true);
            crate::tools::files::batch_rename(&dir, &find, &replace, dry).unwrap_or_else(|e| e)
        }
        "temp_file" => {
            let prefix = s("prefix");
            let suffix = s("suffix");
            let content = args.get("content").and_then(|v| v.as_str());
            crate::tools::files::temp_file(&prefix, &suffix, content).unwrap_or_else(|e| e)
        }
        "symlink_info" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::symlink_info(&path).unwrap_or_else(|e| e)
        }
        "file_slice" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            let start = args.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let end = args.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let output = s("output");
            crate::tools::files::file_slice(&path, start, end, &output).unwrap_or_else(|e| e)
        }
        "lines_sample" => {
            let path = s("file_path");
            let path = if path.is_empty() { s("path") } else { path };
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let seed = args.get("seed").and_then(|v| v.as_u64()).unwrap_or(0);
            crate::tools::files::lines_sample(&path, count, seed).unwrap_or_else(|e| e)
        }
        "empty_files" => {
            let dir = s("dir");
            let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true);
            crate::tools::files::empty_files(&dir, recursive).unwrap_or_else(|e| e)
        }
        "file_concat" => {
            let sources = s("sources");
            let output = s("output");
            let sep = args.get("add_separator").and_then(|v| v.as_bool()).unwrap_or(false);
            crate::tools::files::file_concat(&sources, &output, sep).unwrap_or_else(|e| e)
        }
        "du_sorted" => {
            let dir = s("dir");
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            crate::tools::files::du_sorted(&dir, depth, top_n).unwrap_or_else(|e| e)
        }
        // ═══ 文件剪贴板 v5.9 ═══
        "file_copy_content" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            let max_kb = args.get("max_size_kb").and_then(|v| v.as_u64()).unwrap_or(1024) as usize;
            crate::tools::files::file_copy_content(&path, max_kb).unwrap_or_else(|e| e)
        }
        "file_paste_content" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            let append = args.get("append").and_then(|v| v.as_bool()).unwrap_or(false);
            crate::tools::files::file_paste_content(&path, append).unwrap_or_else(|e| e)
        }
        "file_copy_path" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::file_copy_path(&path).unwrap_or_else(|e| e)
        }
        "file_copy_base64" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            let max_mb = args.get("max_size_mb").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            crate::tools::files::file_copy_base64(&path, max_mb).unwrap_or_else(|e| e)
        }
        "file_paste_base64" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::file_paste_base64(&path).unwrap_or_else(|e| e)
        }
        "file_cut" => {
            let path = s("file_path"); let path = if path.is_empty() { s("path") } else { path };
            crate::tools::files::file_cut(&path).unwrap_or_else(|e| e)
        }
        "file_paste_cut" => {
            let dest = s("dest_dir");
            crate::tools::files::file_paste_cut(&dest).unwrap_or_else(|e| e)
        }
        "file_cut_status" => {
            crate::tools::files::file_cut_status()
        }
        "file_copy_multi" => {
            let paths = s("paths");
            crate::tools::files::file_copy_multi(&paths).unwrap_or_else(|e| e)
        }

        _ => format!("未知工具: {}", name),

    };

        // ═══ 隐身扫描 v5.5 — 原始套接字 ═══
    // ★ 遥测: 记录工具执行结果
    let duration_ms = tool_start.elapsed().as_millis() as u64;
    let success = !result.starts_with("[!]") && !result.starts_with("参数解析失败") && !result.starts_with("未知工具");
    let short_result = if result.len() > 80 {
        let end = result.floor_char_boundary(77);
        format!("{}...", &result[..end])
    } else { result.clone() };
    crate::telemetry::push_tool_result(name, success, duration_ms, &short_result);

    // ★ 路径穿越警告统一收取 — 防止 println! 破坏 TUI 界面
    let warnings = crate::tools::files::drain_path_warnings();
    if warnings.is_empty() {
        result
    } else {
        format!("{}\n{}", warnings.join("\n"), result)
    }
}
