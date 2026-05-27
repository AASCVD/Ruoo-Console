// ============================================================
// RUOO-ARSENAL — 脚本解释器 v4.0
//
// 新增:
//   - for/while 循环
//   - function 定义与调用
//   - 数组支持 (push/pop/get/set/len)
//   - 数学表达式求值 (+ - * / %)
//   - import 脚本导入
//   - assert 断言 (带消息)
//   - echo 调试输出
//   - sleep 延迟
//   - regex 正则捕获
//   - json_get / json_set
// ============================================================

use std::collections::HashMap;
use regex::Regex;

// ════════════════════════════════════════════════════════
// 脚本指令枚举 v4.0
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptInstruction {
    Noop,
    Comment(String),
    Directive(String),              // #![allow-net] 等
    IfExists(String, bool),         // ifexists <path> [not]
    EndIf,
    Try,
    Catch(Option<String>),
    EndBlock,
    Command(String),                // 普通命令

    // ── v4.0 新增 ──
    For(String, String, String),    // for <var> in <start>..<end>
    EndFor,
    While(String),                  // while <condition>
    EndWhile,
    Function(String),               // function <name>
    EndFunction,
    Call(String, Vec<String>),      // call <func> [args...]
    Return(Option<String>),         // return [value]
    Import(String),                 // import <path>
    Echo(String),                   // echo <text>  (调试输出)
    Assert(String, Option<String>), // assert <condition> [message]
    Sleep(u64),                     // sleep <ms>
    Let(String, String),            // let <var> = <expr>
    ArrayPush(String, String),      // array push <name> <value>
    ArrayGet(String, usize, String),// array get <name>[<idx>] → <var>
    ArraySet(String, usize, String),// array set <name>[<idx>] = <value>
    ArrayLen(String, String),       // array len <name> → <var>
    RegexMatch(String, String, String), // regex <pattern> <text> → <var>
    JsonGet(String, String, String),// json_get <json> <path> → <var>
    JsonSet(String, String, String, String), // json_set <json> <path> <value> → <var>
}

// ════════════════════════════════════════════════════════
// 脚本运行时 v4.0
// ════════════════════════════════════════════════════════

pub struct ScriptRuntime {
    pub vars: HashMap<String, String>,
    pub arrays: HashMap<String, Vec<String>>,
    pub functions: HashMap<String, Vec<String>>,  // func_name → lines
    pub stack: Vec<HashMap<String, String>>,      // 调用栈 (局部变量)
    pub output: Vec<String>,
    pub errors: Vec<String>,
    pub running: bool,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            functions: HashMap::new(),
            stack: Vec::new(),
            output: Vec::new(),
            errors: Vec::new(),
            running: true,
        }
    }

    // ── 变量展开 ─────────────────────────────────

    pub fn expand(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                if chars[i + 1] == '{' {
                    if let Some(end) = chars[i + 2..].iter().position(|&c| c == '}') {
                        let var_name: String = chars[i + 2..i + 2 + end].iter().collect();
                        let val = self.resolve_var(&var_name);
                        result.push_str(&val);
                        i += 2 + end + 1;
                        continue;
                    }
                } else if chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' {
                    let mut j = i + 1;
                    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                        j += 1;
                    }
                    let var_name: String = chars[i + 1..j].iter().collect();
                    let val = self.resolve_var(&var_name);
                    result.push_str(&val);
                    i = j;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        result
    }

    fn resolve_var(&self, name: &str) -> String {
        // 1. 局部变量 (调用栈顶)
        if let Some(frame) = self.stack.last() {
            if let Some(v) = frame.get(name) { return v.clone(); }
        }
        // 2. 全局变量
        if let Some(v) = self.vars.get(name) { return v.clone(); }
        // 3. 未定义
        String::new()
    }

    // ── 数学表达式求值 ───────────────────────────

    pub fn eval_expr(&self, expr: &str) -> Result<i64, String> {
        let expanded = self.expand(expr);
        let trimmed = expanded.trim();
        if trimmed.is_empty() { return Ok(0); }

        // 尝试直接解析整数
        if let Ok(n) = trimmed.parse::<i64>() { return Ok(n); }

        // 简单四则运算: a + b, a - b, a * b, a / b, a % b
        let ops = ['+', '-', '*', '/', '%'];
        for &op in &ops {
            if let Some(pos) = trimmed.find(op) {
                // 避免匹配负号开头
                if op == '-' && pos == 0 { continue; }
                // 确保操作符两边有内容 (或负号后接数字的边界情况)
                if pos == 0 || pos >= trimmed.len() - 1 { continue; }

                let left = self.eval_expr(&trimmed[..pos])?;
                let right = self.eval_expr(&trimmed[pos+1..])?;
                return match op {
                    '+' => Ok(left + right),
                    '-' => Ok(left - right),
                    '*' => Ok(left * right),
                    '/' => {
                        if right == 0 { Err("除零错误".into()) }
                        else { Ok(left / right) }
                    }
                    '%' => {
                        if right == 0 { Err("取模零错误".into()) }
                        else { Ok(left % right) }
                    }
                    _ => Err(format!("未知运算符: {}", op)),
                };
            }
        }

        // 变量引用
        if let Ok(n) = self.resolve_var(trimmed).parse::<i64>() {
            return Ok(n);
        }

        Err(format!("无法求值表达式: {}", trimmed))
    }

    // ── 比较运算 ───────────────────────────────

    pub fn eval_condition(&self, cond: &str) -> Result<bool, String> {
        let expanded = self.expand(cond);
        let trimmed = expanded.trim();

        if trimmed.is_empty() { return Ok(false); }
        if trimmed == "true" { return Ok(true); }
        if trimmed == "false" { return Ok(false); }

        // == 比较
        if let Some(pos) = trimmed.find("==") {
            let left = self.expand(trimmed[..pos].trim());
            let right = self.expand(trimmed[pos+2..].trim());
            return Ok(left == right);
        }
        // != 比较
        if let Some(pos) = trimmed.find("!=") {
            let left = self.expand(trimmed[..pos].trim());
            let right = self.expand(trimmed[pos+2..].trim());
            return Ok(left != right);
        }
        // >= 比较 (数字)
        if let Some(pos) = trimmed.find(">=") {
            let left = self.eval_expr(trimmed[..pos].trim())?;
            let right = self.eval_expr(trimmed[pos+2..].trim())?;
            return Ok(left >= right);
        }
        // <= 比较 (数字)
        if let Some(pos) = trimmed.find("<=") {
            let left = self.eval_expr(trimmed[..pos].trim())?;
            let right = self.eval_expr(trimmed[pos+2..].trim())?;
            return Ok(left <= right);
        }
        // > 比较 (数字)
        if let Some(pos) = trimmed.find('>') {
            let left = self.eval_expr(trimmed[..pos].trim())?;
            let right = self.eval_expr(trimmed[pos+1..].trim())?;
            return Ok(left > right);
        }
        // < 比较 (数字)
        if let Some(pos) = trimmed.find('<') {
            let left = self.eval_expr(trimmed[..pos].trim())?;
            let right = self.eval_expr(trimmed[pos+1..].trim())?;
            return Ok(left < right);
        }

        // 单变量/数字: 非零/非空 = true
        if let Ok(n) = self.eval_expr(trimmed) {
            return Ok(n != 0);
        }
        Ok(!trimmed.is_empty())
    }

    // ── 正则匹配 ───────────────────────────────

    pub fn regex_capture(&self, pattern: &str, text: &str) -> Result<String, String> {
        let re = Regex::new(pattern).map_err(|e| format!("正则错误: {}", e))?;
        let expanded_text = self.expand(text);
        if let Some(cap) = re.captures(&expanded_text) {
            if let Some(m) = cap.get(1) {
                return Ok(m.as_str().to_string());
            }
            if let Some(m) = cap.get(0) {
                return Ok(m.as_str().to_string());
            }
        }
        Ok(String::new())
    }

    // ── JSON 操作 ───────────────────────────────

    pub fn json_get(&self, json_str: &str, path: &str) -> Result<String, String> {
        let expanded = self.expand(json_str);
        let v: serde_json::Value = serde_json::from_str(&expanded)
            .map_err(|e| format!("JSON解析错误: {}", e))?;

        let parts: Vec<&str> = path.split('.').collect();
        let mut current = &v;
        for part in &parts {
            current = match current {
                serde_json::Value::Object(map) => {
                    map.get(*part).unwrap_or(&serde_json::Value::Null)
                }
                serde_json::Value::Array(arr) => {
                    if let Ok(idx) = part.parse::<usize>() {
                        arr.get(idx).unwrap_or(&serde_json::Value::Null)
                    } else { &serde_json::Value::Null }
                }
                _ => &serde_json::Value::Null,
            };
        }
        Ok(match current {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        })
    }

    pub fn json_set(&self, json_str: &str, path: &str, value: &str) -> Result<String, String> {
        let expanded = self.expand(json_str);
        let mut v: serde_json::Value = serde_json::from_str(&expanded)
            .map_err(|e| format!("JSON解析错误: {}", e))?;
        let expanded_val = self.expand(value);

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() { return Err("空路径".into()); }

        // 简单实现: 只支持一级路径
        if parts.len() == 1 {
            if let serde_json::Value::Object(ref mut map) = v {
                map.insert(parts[0].to_string(), serde_json::Value::String(expanded_val));
            }
        }

        serde_json::to_string(&v).map_err(|e| format!("JSON序列化错误: {}", e))
    }
}

// ════════════════════════════════════════════════════════
// 行解析器 v4.0
// ════════════════════════════════════════════════════════

pub fn parse_line(line: &str) -> ScriptInstruction {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "\n" || trimmed == "\r\n" {
        return ScriptInstruction::Noop;
    }

    let lower = trimmed.to_lowercase();

    // 注释
    if trimmed.starts_with("//") || trimmed.starts_with('#') && !trimmed.starts_with("#!") {
        return ScriptInstruction::Comment(trimmed.to_string());
    }

    // 指令
    if trimmed.starts_with("#![") && trimmed.ends_with(']') {
        let dir = &trimmed[3..trimmed.len()-1].trim().to_lowercase();
        return ScriptInstruction::Directive(dir.to_string());
    }

    // ifexists
    if lower.starts_with("ifexists ") {
        let rest = &trimmed[9..].trim();
        let (path, negated) = if rest.starts_with("not ") {
            (&rest[4..].trim(), false)
        } else { (rest, true) };
        return ScriptInstruction::IfExists(path.to_string(), negated);
    }

    if lower == "endif" { return ScriptInstruction::EndIf; }
    if lower == "try" { return ScriptInstruction::Try; }
    if lower.starts_with("catch") {
        let var = if lower.len() > 5 { Some(trimmed[6..].trim().to_string()) } else { None };
        return ScriptInstruction::Catch(var);
    }
    if lower == "end" || lower == "endblock" { return ScriptInstruction::EndBlock; }

    // ── v4.0 新增指令 ──

    // for <var> in <start>..<end>
    if lower.starts_with("for ") {
        let rest = &trimmed[4..].trim();
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        if parts.len() >= 3 && parts[1] == "in" {
            let range: Vec<&str> = parts[2].split("..").collect();
            if range.len() == 2 {
                return ScriptInstruction::For(
                    parts[0].to_string(),
                    range[0].trim().to_string(),
                    range[1].trim().to_string(),
                );
            }
        }
    }
    if lower == "endfor" { return ScriptInstruction::EndFor; }

    // while <condition>
    if lower.starts_with("while ") {
        let cond = &trimmed[6..].trim();
        return ScriptInstruction::While(cond.to_string());
    }
    if lower == "endwhile" { return ScriptInstruction::EndWhile; }

    // function <name>
    if lower.starts_with("function ") {
        let name = &trimmed[9..].trim();
        return ScriptInstruction::Function(name.to_string());
    }
    if lower == "endfunction" { return ScriptInstruction::EndFunction; }

    // return [value]
    if lower == "return" {
        return ScriptInstruction::Return(None);
    }
    if lower.starts_with("return ") {
        let val = trimmed[7..].trim().to_string();
        return ScriptInstruction::Return(Some(val));
    }

    // import <path>
    if lower.starts_with("import ") {
        let path = trimmed[7..].trim();
        return ScriptInstruction::Import(path.to_string());
    }

    // echo <text>
    if lower.starts_with("echo ") {
        let text = trimmed[5..].trim().to_string();
        return ScriptInstruction::Echo(text);
    }

    // assert <condition> [message]
    if lower.starts_with("assert ") {
        let rest = trimmed[7..].trim();
        if let Some(pos) = rest.find(" // ") {
            return ScriptInstruction::Assert(
                rest[..pos].trim().to_string(),
                Some(rest[pos+4..].trim().to_string()),
            );
        }
        if let Some(pos) = rest.find(" # ") {
            return ScriptInstruction::Assert(
                rest[..pos].trim().to_string(),
                Some(rest[pos+3..].trim().to_string()),
            );
        }
        return ScriptInstruction::Assert(rest.to_string(), None);
    }

    // sleep <ms>
    if lower.starts_with("sleep ") {
        if let Ok(ms) = trimmed[6..].trim().parse::<u64>() {
            return ScriptInstruction::Sleep(ms);
        }
    }

    // let <var> = <expr>
    if lower.starts_with("let ") {
        let rest = &trimmed[4..].trim();
        if let Some(pos) = rest.find('=') {
            let var = rest[..pos].trim().to_string();
            let expr = rest[pos+1..].trim().to_string();
            return ScriptInstruction::Let(var, expr);
        }
    }

    // array push <name> <value>
    if lower.starts_with("array push ") {
        let rest = &trimmed[11..].trim();
        if let Some(pos) = rest.find(' ') {
            let name = rest[..pos].trim().to_string();
            let value = rest[pos+1..].trim().to_string();
            return ScriptInstruction::ArrayPush(name, value);
        }
    }

    // array get <name>[<idx>] → <var>  or  array get <name> <idx> → <var>
    if lower.starts_with("array get ") {
        let rest = &trimmed[10..].trim();
        // 格式: name idx → var
        let parts: Vec<&str> = rest.split("->").collect();
        if parts.len() == 2 {
            let left = parts[0].trim();
            let target_var = parts[1].trim().to_string();
            let left_parts: Vec<&str> = left.splitn(2, ' ').collect();
            if left_parts.len() == 2 {
                if let Ok(idx) = left_parts[1].parse::<usize>() {
                    return ScriptInstruction::ArrayGet(left_parts[0].to_string(), idx, target_var);
                }
            }
        }
    }

    // array set <name> <idx> = <value>
    if lower.starts_with("array set ") {
        let rest = &trimmed[10..].trim();
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let right_parts: Vec<&str> = parts[1].splitn(2, '=').collect();
            if right_parts.len() == 2 {
                if let Ok(idx) = right_parts[0].trim().parse::<usize>() {
                    return ScriptInstruction::ArraySet(
                        parts[0].to_string(), idx, right_parts[1].trim().to_string(),
                    );
                }
            }
        }
    }

    // array len <name> → <var>
    if lower.starts_with("array len ") {
        let rest = &trimmed[10..].trim();
        if let Some(pos) = rest.find("->") {
            let name = rest[..pos].trim().to_string();
            let var = rest[pos+2..].trim().to_string();
            return ScriptInstruction::ArrayLen(name, var);
        }
    }

    // regex <pattern> <text> → <var>
    if lower.starts_with("regex ") {
        let rest = &trimmed[6..].trim();
        if let Some(pos) = rest.find("->") {
            let left = rest[..pos].trim();
            let var = rest[pos+2..].trim().to_string();
            let parts: Vec<&str> = left.splitn(2, ' ').collect();
            if parts.len() == 2 {
                return ScriptInstruction::RegexMatch(parts[0].to_string(), parts[1].to_string(), var);
            }
        }
    }

    // json_get <json> <path> → <var>
    if lower.starts_with("json_get ") {
        let rest = &trimmed[9..].trim();
        if let Some(pos) = rest.find("->") {
            let left = rest[..pos].trim();
            let var = rest[pos+2..].trim().to_string();
            let parts: Vec<&str> = left.splitn(2, ' ').collect();
            if parts.len() == 2 {
                return ScriptInstruction::JsonGet(parts[0].to_string(), parts[1].to_string(), var);
            }
        }
    }

    // 普通命令
    ScriptInstruction::Command(trimmed.to_string())
}

// ════════════════════════════════════════════════════════
// 脚本执行器 v4.0
// ════════════════════════════════════════════════════════

pub struct ScriptExecutor {
    pub rt: ScriptRuntime,
}

impl ScriptExecutor {
    pub fn new() -> Self {
        Self { rt: ScriptRuntime::new() }
    }

    /// 执行解析后的指令列表
    pub fn execute(
        &mut self,
        instructions: &[ScriptInstruction],
        exec_cmd_fn: &mut dyn FnMut(&str) -> String,
    ) {
        let mut ip: isize = 0; // 指令指针
        let _skip_until: Option<(&str, u32)> = None; // ("endif"/"endfor"/etc, depth)
        let mut func_def_depth = 0u32;
        let mut current_func: Option<String> = None;
        let mut loop_stack: Vec<(usize, usize, usize)> = Vec::new(); // (start_ip, end_ip, current_iter)

        while ip >= 0 && (ip as usize) < instructions.len() && self.rt.running {
            let inst = &instructions[ip as usize];

            // 在函数定义内收集行
            if func_def_depth > 0 {
                match inst {
                    ScriptInstruction::Function(_) => func_def_depth += 1,
                    ScriptInstruction::EndFunction => {
                        func_def_depth -= 1;
                        if func_def_depth == 0 {
                            current_func = None;
                        }
                    }
                    _ => {
                        if let Some(ref fname) = current_func {
                            if let ScriptInstruction::Command(cmd) = inst {
                                self.rt.functions.entry(fname.clone())
                                    .or_default()
                                    .push(cmd.clone());
                            }
                        }
                    }
                }
                ip += 1;
                continue;
            }

            match inst {
                ScriptInstruction::Noop | ScriptInstruction::Comment(_) => {
                    ip += 1;
                }

                ScriptInstruction::Directive(_) => {
                    ip += 1; // 指令由外层处理
                }

                ScriptInstruction::IfExists(_, _) => {
                    ip += 1; // 由外层处理
                }
                ScriptInstruction::EndIf => {
                    ip += 1;
                }

                ScriptInstruction::Try => { ip += 1; }
                ScriptInstruction::Catch(_) => { ip += 1; }
                ScriptInstruction::EndBlock => { ip += 1; }

                // ── While 循环 ──
                ScriptInstruction::While(cond) => {
                    let while_start_ip = (ip + 1) as usize;
                    if !self.rt.eval_condition(cond).unwrap_or(false) {
                        // 跳过循环体
                        let mut depth = 1u32;
                        ip += 1;
                        while (ip as usize) < instructions.len() && depth > 0 {
                            match &instructions[ip as usize] {
                                ScriptInstruction::While(_) => depth += 1,
                                ScriptInstruction::EndWhile => depth -= 1,
                                _ => {}
                            }
                            ip += 1;
                        }
                        continue;
                    }
                    loop_stack.push((while_start_ip, 1, 0));
                    ip += 1;
                }

                ScriptInstruction::EndWhile => {
                    if let Some((start_ip, _depth, _)) = loop_stack.last() {
                        let while_inst = &instructions[*start_ip - 1];
                        if let ScriptInstruction::While(ref cond) = while_inst {
                            if self.rt.eval_condition(cond).unwrap_or(false) {
                                ip = *start_ip as isize;
                            } else {
                                loop_stack.pop();
                                ip += 1;
                            }
                        } else {
                            loop_stack.pop();
                            ip += 1;
                        }
                    } else {
                        ip += 1;
                    }
                }

                // ── Call 函数调用 ──
                ScriptInstruction::Call(func_name, args) => {
                    if let Some(body) = self.rt.functions.get(func_name).cloned() {
                        // 压入局部作用域
                        let mut frame = HashMap::new();
                        for (i, arg) in args.iter().enumerate() {
                            let expanded = self.rt.expand(arg);
                            frame.insert(format!("_arg{}", i), expanded);
                        }
                        self.rt.stack.push(frame);
                        // 转为指令执行
                        let func_insts: Vec<ScriptInstruction> = body.iter()
                            .map(|line| parse_line(line))
                            .collect();
                        self.execute(&func_insts, exec_cmd_fn);
                        self.rt.stack.pop();
                    } else {
                        self.rt.errors.push(format!("未定义函数: {}", func_name));
                    }
                    ip += 1;
                }

                // ── For 循环 ──
                ScriptInstruction::For(var, start_expr, end_expr) => {
                    let start = self.rt.eval_expr(start_expr).unwrap_or(0);
                    let end = self.rt.eval_expr(end_expr).unwrap_or(0);
                    let for_start_ip = (ip + 1) as usize;

                    if start > end {
                        // 跳过循环体
                        let mut depth = 1u32;
                        ip += 1;
                        while (ip as usize) < instructions.len() && depth > 0 {
                            match &instructions[ip as usize] {
                                ScriptInstruction::For(_, _, _) => depth += 1,
                                ScriptInstruction::EndFor => depth -= 1,
                                _ => {}
                            }
                            ip += 1;
                        }
                        continue;
                    }

                    self.rt.vars.insert(var.clone(), start.to_string());
                    loop_stack.push((for_start_ip, 1, start as usize));

                    ip += 1;
                }

                ScriptInstruction::EndFor => {
                    if let Some((start_ip, _depth, ref mut iter)) = loop_stack.last_mut() {
                        // 获取循环变量
                        let for_inst = &instructions[*start_ip - 1];
                        if let ScriptInstruction::For(ref var, _, ref end_expr) = for_inst {
                            *iter += 1;
                            let end = self.rt.eval_expr(end_expr).unwrap_or(0) as usize;
                            if *iter <= end {
                                self.rt.vars.insert(var.clone(), (*iter).to_string());
                                ip = *start_ip as isize;
                            } else {
                                loop_stack.pop();
                                ip += 1;
                            }
                        } else {
                            loop_stack.pop();
                            ip += 1;
                        }
                    } else {
                        ip += 1;
                    }
                }

                // ── Function 定义 ──
                ScriptInstruction::Function(name) => {
                    func_def_depth = 1;
                    current_func = Some(name.clone());
                    self.rt.functions.entry(name.clone()).or_default();
                    ip += 1;
                }
                ScriptInstruction::EndFunction => {
                    current_func = None;
                    func_def_depth = 0;
                    ip += 1;
                }

                // ── Return ──
                ScriptInstruction::Return(val) => {
                    if let Some(ref v) = val {
                        let expanded = self.rt.expand(v);
                        self.rt.vars.insert("_return".into(), expanded);
                    }
                    // 弹出调用栈
                    if let Some(_frame) = self.rt.stack.pop() {
                        // 恢复变量 (调用栈帧弹出)
                    }
                    ip += 1;
                }

                // ── Import ──
                ScriptInstruction::Import(path) => {
                    let expanded = self.rt.expand(path);
                    self.rt.output.push(format!("[import] {}", expanded));
                    ip += 1;
                }

                // ── Echo ──
                ScriptInstruction::Echo(text) => {
                    let expanded = self.rt.expand(text);
                    self.rt.output.push(expanded);
                    ip += 1;
                }

                // ── Assert ──
                ScriptInstruction::Assert(cond, msg) => {
                    match self.rt.eval_condition(cond) {
                        Ok(true) => {},
                        Ok(false) => {
                            let err = msg.as_deref().unwrap_or("断言失败");
                            let expanded = self.rt.expand(err);
                            self.rt.errors.push(format!("ASSERT: {}", expanded));
                            self.rt.output.push(format!("[!] ASSERT FAILED: {}", expanded));
                        }
                        Err(e) => {
                            self.rt.errors.push(format!("ASSERT评估错误: {}", e));
                        }
                    }
                    ip += 1;
                }

                // ── Sleep ──
                ScriptInstruction::Sleep(ms) => {
                    std::thread::sleep(std::time::Duration::from_millis(*ms));
                    ip += 1;
                }

                // ── Let (赋值+表达式求值) ──
                ScriptInstruction::Let(var, expr) => {
                    match self.rt.eval_expr(expr) {
                        Ok(val) => { self.rt.vars.insert(var.clone(), val.to_string()); }
                        Err(_e) => {
                            // 如果不是数学表达式，当作字符串赋值
                            let expanded = self.rt.expand(expr);
                            self.rt.vars.insert(var.clone(), expanded);
                        }
                    }
                    ip += 1;
                }

                // ── Array 操作 ──
                ScriptInstruction::ArrayPush(name, value) => {
                    let expanded = self.rt.expand(value);
                    self.rt.arrays.entry(name.clone()).or_default().push(expanded);
                    ip += 1;
                }

                ScriptInstruction::ArrayGet(name, idx, target) => {
                    let val = self.rt.arrays.get(name)
                        .and_then(|arr| arr.get(*idx))
                        .cloned()
                        .unwrap_or_default();
                    self.rt.vars.insert(target.clone(), val);
                    ip += 1;
                }

                ScriptInstruction::ArraySet(name, idx, value) => {
                    let expanded = self.rt.expand(value);
                    if let Some(arr) = self.rt.arrays.get_mut(name) {
                        if *idx < arr.len() {
                            arr[*idx] = expanded;
                        } else {
                            arr.resize(*idx + 1, String::new());
                            arr[*idx] = expanded;
                        }
                    }
                    ip += 1;
                }

                ScriptInstruction::ArrayLen(name, target) => {
                    let len = self.rt.arrays.get(name).map_or(0, |a| a.len());
                    self.rt.vars.insert(target.clone(), len.to_string());
                    ip += 1;
                }

                // ── Regex ──
                ScriptInstruction::RegexMatch(pattern, text, target) => {
                    match self.rt.regex_capture(pattern, text) {
                        Ok(captured) => { self.rt.vars.insert(target.clone(), captured); }
                        Err(e) => { self.rt.errors.push(e); }
                    }
                    ip += 1;
                }

                // ── JSON ──
                ScriptInstruction::JsonGet(json_str, path, target) => {
                    match self.rt.json_get(json_str, path) {
                        Ok(val) => { self.rt.vars.insert(target.clone(), val); }
                        Err(e) => { self.rt.errors.push(e); }
                    }
                    ip += 1;
                }

                ScriptInstruction::JsonSet(json_str, path, value, target) => {
                    match self.rt.json_set(json_str, path, value) {
                        Ok(val) => { self.rt.vars.insert(target.clone(), val); }
                        Err(e) => { self.rt.errors.push(e); }
                    }
                    ip += 1;
                }

                // ── 普通命令 ──
                ScriptInstruction::Command(cmd) => {
                    let expanded = self.rt.expand(cmd);
                    let result = exec_cmd_fn(&expanded);
                    if !result.is_empty() {
                        self.rt.output.push(result);
                    }
                    ip += 1;
                }
            }
        }
    }

    /// 获取输出
    pub fn get_output(&self) -> Vec<String> {
        self.rt.output.clone()
    }

    /// 获取错误
    pub fn get_errors(&self) -> Vec<String> {
        self.rt.errors.clone()
    }
}

// ════════════════════════════════════════════════════════
// 测试
// ════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line_directives() {
        assert_eq!(parse_line("#![allow-net]"), ScriptInstruction::Directive("allow-net".into()));
        assert_eq!(parse_line("#![allow-all]"), ScriptInstruction::Directive("allow-all".into()));
    }

    #[test]
    fn test_parse_line_for() {
        let inst = parse_line("for i in 0..5");
        assert_eq!(inst, ScriptInstruction::For("i".into(), "0".into(), "5".into()));
    }

    #[test]
    fn test_parse_line_let() {
        let inst = parse_line("let x = 1 + 2");
        assert_eq!(inst, ScriptInstruction::Let("x".into(), "1 + 2".into()));
    }

    #[test]
    fn test_parse_line_echo() {
        let inst = parse_line("echo hello world");
        assert_eq!(inst, ScriptInstruction::Echo("hello world".into()));
    }

    #[test]
    fn test_parse_line_array_push() {
        let inst = parse_line("array push urls https://example.com");
        assert_eq!(inst, ScriptInstruction::ArrayPush("urls".into(), "https://example.com".into()));
    }

    #[test]
    fn test_eval_expr_simple() {
        let rt = ScriptRuntime::new();
        assert_eq!(rt.eval_expr("1+2").unwrap(), 3);
        assert_eq!(rt.eval_expr("10-3").unwrap(), 7);
        assert_eq!(rt.eval_expr("4*5").unwrap(), 20);
        assert_eq!(rt.eval_expr("20/4").unwrap(), 5);
        assert_eq!(rt.eval_expr("10%3").unwrap(), 1);
    }

    #[test]
    fn test_eval_condition() {
        let rt = ScriptRuntime::new();
        assert_eq!(rt.eval_condition("5 > 3").unwrap(), true);
        assert_eq!(rt.eval_condition("2 > 5").unwrap(), false);
        assert_eq!(rt.eval_condition("abc == abc").unwrap(), true);
        assert_eq!(rt.eval_condition("abc != xyz").unwrap(), true);
        assert_eq!(rt.eval_condition("true").unwrap(), true);
        assert_eq!(rt.eval_condition("false").unwrap(), false);
    }

    #[test]
    fn test_variable_expansion() {
        let mut rt = ScriptRuntime::new();
        rt.vars.insert("name".into(), "Zero-Day".into());
        rt.vars.insert("port".into(), "8080".into());
        assert_eq!(rt.expand("Hello ${name}"), "Hello Zero-Day");
        assert_eq!(rt.expand("Port: $port"), "Port: 8080");
    }

    #[test]
    fn test_regex_capture() {
        let rt = ScriptRuntime::new();
        let result = rt.regex_capture(r"(\d+\.\d+\.\d+\.\d+)", "IP: 192.168.1.1").unwrap();
        assert_eq!(result, "192.168.1.1");
    }

    #[test]
    fn test_json_get() {
        let rt = ScriptRuntime::new();
        let json = r#"{"name":"test","version":"1.0"}"#;
        assert_eq!(rt.json_get(json, "name").unwrap(), "test");
        assert_eq!(rt.json_get(json, "version").unwrap(), "1.0");
    }

    #[test]
    fn test_executor_simple() {
        let lines = vec![
            ScriptInstruction::Let("x".into(), "10".into()),
            ScriptInstruction::Let("y".into(), "20".into()),
            ScriptInstruction::Echo("done".into()),
        ];
        let mut ex = ScriptExecutor::new();
        ex.execute(&lines, &mut |cmd| format!("CMD: {}", cmd));
        assert!(ex.rt.vars.get("x").unwrap() == "10");
        assert!(ex.rt.vars.get("y").unwrap() == "20");
    }

    #[test]
    fn test_executor_for_loop() {
        let lines = vec![
            ScriptInstruction::For("i".into(), "0".into(), "3".into()),
            ScriptInstruction::Echo("loop".into()),
            ScriptInstruction::EndFor,
        ];
        let mut ex = ScriptExecutor::new();
        ex.execute(&lines, &mut |cmd| format!("CMD: {}", cmd));
        // 应该输出4次 (i=0,1,2,3)
        assert_eq!(ex.rt.output.len(), 4);
        assert_eq!(ex.rt.vars.get("i").unwrap(), "3");
    }
}
