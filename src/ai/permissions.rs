// === 权限验证 & Shell命令注入防护 ===

use crate::ai::types::AiPermLevel;
use crate::ai::globals;

/// 检查当前权限是否足够 (execute_tool 内部使用)
pub(crate) fn check_tool_perm(required: AiPermLevel, tool_name: &str) -> Result<(), String> {
    let current = globals::get_ai_perm_level();
    if current >= required {
        Ok(())
    } else {
        Err(format!(
            "权限不足: {tool} 需要 {req}，当前 {cur}\n  \
             [{tool}] 被阻止 — 请用户在 TUI 执行: perm {req_n} (当前: perm {cur_n})",
            tool = tool_name, req = required.label(), cur = current.label(),
            req_n = required.as_i32(), cur_n = current.as_i32()
        ))
    }
}

/// Shell命令注入防护 — 拦截危险的 shell 元字符
pub fn validate_shell_command(cmd: &str) -> Result<(), String> {
    if cmd.contains('\n') || cmd.contains('\r') {
        return Err("命令包含换行符 (CR/LF注入)".into());
    }
    if cmd.contains('`') {
        return Err("命令包含反引号 ` (命令替换注入)".into());
    }
    if cmd.contains("$(") {
        return Err("命令包含 $(...) 命令替换语法".into());
    }
    if cmd.contains("${") {
        return Err("命令包含 ${...} 变量展开语法".into());
    }

    // 引号感知: 剥离引号内容后再检查
    let mut stripped = String::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_double {
            in_single = !in_single;
            stripped.push(' ');
        } else if c == '"' && !in_single {
            in_double = !in_double;
            stripped.push(' ');
        } else if in_single || in_double {
            stripped.push(' ');
        } else {
            stripped.push(c);
        }
        i += 1;
    }

    if stripped.contains(';') {
        return Err("命令包含 ; 分隔符 (命令链接注入)".into());
    }
    if stripped.contains('|') {
        return Err("命令包含 | 管道符 — 请使用脚本或插件替代管道操作".into());
    }
    if stripped.contains('&') {
        return Err("命令包含 & 后台/命令链符号".into());
    }
    if stripped.contains('>') || stripped.contains('<') {
        return Err("命令包含 >/< 重定向符号".into());
    }

    Ok(())
}
