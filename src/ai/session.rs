// === AI 会话管理 ===
// AiSession struct + impl

use serde_json::{json, Value};
use crate::ai::types::{AiPermLevel, StreamMsg, StreamKind};
use crate::ai::deepseek::call_deepseek;
use crate::ai::globals::{self, TOOL_ABORT_FLAG, CLEAR_HISTORY_FLAG};
use crate::ai::misc::is_code_tool;
use crate::ai::markdown::strip_markdown;
use crate::ai::prompt::{build_system_prompt, build_tools};
use crate::ai::cache;

pub const MAX_HISTORY_MESSAGES: usize = 500;

// ── AI 会话 ──
pub struct AiSession {
    pub history: Vec<Value>,
    pub system_prompt: String,
    tools: Vec<Value>,
    /// AI 工具权限等级 — 只能通过 TUI `perm <N>` 命令修改
    pub perm_level: AiPermLevel,
}

impl AiSession {
    pub fn new(custom_prompt: Option<&str>) -> Self {
        let sp_raw = custom_prompt
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                p.replace("{platform}", std::env::consts::OS)
                 .replace("{arch}", std::env::consts::ARCH)
                 .replace("{cores}", &std::thread::available_parallelism()
                     .map(|n| n.get().to_string()).unwrap_or_else(|_| "8".into()))
                 .replace("{cwd}", &std::env::current_dir()
                     .map(|p| p.display().to_string()).unwrap_or_else(|_| "(未知)".into()))
            })
            .unwrap_or_else(build_system_prompt);
        let sp = sp_raw;
        let mut tools = build_tools();
        let plugin_tools = crate::plugin::all_ai_tool_json();
        tools.extend(plugin_tools);

        Self {
            history: vec![json!({"role":"system","content":sp.clone()})],
            system_prompt: sp,
            tools,
            perm_level: AiPermLevel::Dangerous,
        }
    }

    pub fn new_with_level(custom_prompt: Option<&str>, level: AiPermLevel) -> Self {
        let mut s = Self::new(custom_prompt);
        s.perm_level = level;
        s
    }

    pub fn reset(&mut self) {
        self.history = vec![json!({"role":"system","content":self.system_prompt.clone()})];
    }

    pub fn clear_cache(&self) {
        cache::clear_all_cache();
    }

    pub fn check_perm(&self, required: AiPermLevel, tool_name: &str) -> Result<(), String> {
        if self.perm_level >= required {
            Ok(())
        } else {
            Err(format!(
                "权限不足: {tool} 需要 {req}，当前 {cur}\n  \
                 [{tool}] 被阻止 — 请用户在 TUI 执行: perm {req_n} (当前: perm {cur_n})\n  \
                 权限说明: perm 0=只读 1=网络 2=Web 3=文件 4=系统 5=全部",
                tool = tool_name, req = required.label(), cur = self.perm_level.label(),
                req_n = required.as_i32(), cur_n = self.perm_level.as_i32()
            ))
        }
    }

    pub fn tool_count(&self) -> usize { self.tools.len() }

    pub fn send(&mut self, api_key: &str, api_url: &str, model: &str, msg: &str,
        tool_status: Option<&std::sync::Arc<std::sync::Mutex<Option<String>>>>,
        cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
        stream_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>)
        -> Result<(String, Vec<String>), String>
    {
        // ★ 防御: API Key 为空时直接拒绝 (兜底检查)
        if api_key.is_empty() {
            return Err("DeepSeek API Key 未配置 — 请执行 config set deepseek_api_key=sk-xxx 或 vault set api_keys deepseek <key>".into());
        }
        globals::set_ai_perm_level(self.perm_level);
        // ★ 重置中断标志 — 每次新对话开始时清除之前的 abort
        TOOL_ABORT_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);

        self.history.push(json!({"role":"user","content":msg}));

        let mut tool_logs = Vec::new();
        let mut final_content = String::new();
        const MAX_ROUNDS: u32 = 3000;

        for round in 0..MAX_ROUNDS {
            if let Some(c) = cancel {
                if c.load(std::sync::atomic::Ordering::Relaxed) {
                    final_content = "[!] AI 请求已被用户取消 (Esc)".into();
                    break;
                }
            }

            if let Some(tx) = stream_tx {
                let _ = tx.send(StreamMsg { kind: StreamKind::Api, content: format!("API 请求 #{} → {}", round + 1, model) });
            }

            let reply = call_deepseek(api_key, api_url, model, &self.history, &self.tools, stream_tx, cancel)?;

            match reply.tool_calls {
                Some(tool_calls) => {
                    if let Some(tx) = stream_tx {
                        let _ = tx.send(StreamMsg { kind: StreamKind::Api, content: format!("AI 决定调用 {} 个工具", tool_calls.len()) });
                    }

                    let atc: Vec<Value> = tool_calls.iter().map(|tc| {
                        let safe_args = tc.arguments.clone();
                        json!({
                            "id": tc.id, "type": "function",
                            "function": {"name": tc.name, "arguments": safe_args}
                        })
                    }).collect();

                    let mut am = json!({"role":"assistant","tool_calls":atc});
                    if !reply.content.is_empty() { am["content"] = json!(reply.content); }
                    if !reply.reasoning_content.is_empty() { am["reasoning_content"] = json!(reply.reasoning_content); }
                    self.history.push(am);

                    for tc in &tool_calls {
                        if TOOL_ABORT_FLAG.load(std::sync::atomic::Ordering::Relaxed) {
                            if let Some(tx) = stream_tx {
                                let _ = tx.send(StreamMsg { kind: StreamKind::Error, content: format!("中断: 跳过 {}", tc.name) });
                            }
                            tool_logs.push(format!("[!] 中断: 跳过 {} ({})", tc.name, tc.arguments));
                            continue;
                        }
                        if let Some(ts) = tool_status {
                            *ts.lock().unwrap() = Some(format!("{}…", tc.name));
                        }
                        if let Some(tx) = stream_tx {
                            let _ = tx.send(StreamMsg { kind: StreamKind::Tool, content: format!("调用 {} ({})", tc.name, tc.arguments) });
                        }

                        let tool_name = tc.name.clone();
                        let tool_args = tc.arguments.clone();
                        let (tx_result, rx_result) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                crate::ai::executor::execute_tool(&tool_name, &tool_args)
                            }));
                            let result = match res {
                                Ok(s) => s,
                                Err(e) => {
                                    if let Some(s) = e.downcast_ref::<&str>() {
                                        format!("[🛡] 工具 '{}' panic: {} — 终端进程已保护", tool_name, s)
                                    } else if let Some(s) = e.downcast_ref::<String>() {
                                        format!("[🛡] 工具 '{}' panic: {} — 终端进程已保护", tool_name, s)
                                    } else {
                                        format!("[🛡] 工具 '{}' panic — 终端进程已保护", tool_name)
                                    }
                                }
                            };
                            let _ = tx_result.send(result);
                        });
                        let result_full = loop {
                            match rx_result.recv_timeout(std::time::Duration::from_millis(100)) {
                                Ok(res) => break res,
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    // 用户手动中断
                                    if TOOL_ABORT_FLAG.load(std::sync::atomic::Ordering::Relaxed) {
                                        break format!("[!] 工具 {} 已被用户强制中断 (线程在后台回收中)", tc.name);
                                    }
                                    // Esc 取消
                                    if let Some(c) = cancel {
                                        if c.load(std::sync::atomic::Ordering::Relaxed) {
                                            break format!("[!] 工具 {} 已被用户取消 (Esc)", tc.name);
                                        }
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                    break format!("[!] 工具 {} 执行线程异常终止", tc.name);
                                }
                            }
                        };

                        if let Some(ts) = tool_status {
                            if result_full.starts_with("[!] 工具") && result_full.contains("强制中断") {
                                *ts.lock().unwrap() = Some(format!("{} ⊘", tc.name));
                            } else {
                                *ts.lock().unwrap() = Some(format!("{} ✓", tc.name));
                            }
                        }
                        let brief: String = result_full.chars().take(120).collect();
                        let suffix = if result_full.len() > 120 { "..." } else { "" };
                        if let Some(tx) = stream_tx {
                            let _ = tx.send(StreamMsg { kind: StreamKind::Result_, content: format!("← {}{}", brief, suffix) });
                        }
                        tool_logs.push(format!("[·] {} ({}) → {}{}", tc.name, tc.arguments, brief, suffix));
                        let truncated = if result_full.len() > 8192 {
                            let head: String = result_full.chars().take(8000).collect();
                            format!("{}…[截断: {}→8KB, 完整结果在终端输出中]", head, result_full.len())
                        } else {
                            result_full
                        };
                        let safe_result = if is_code_tool(&tc.name) {
                            truncated
                        } else {
                            truncated
                        };
                        self.history.push(json!({"role":"tool","tool_call_id":tc.id,"content":safe_result}));
                    }
                    TOOL_ABORT_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);
                    if let Some(ts) = tool_status {
                        *ts.lock().unwrap() = None;
                    }
                }
                None => {
                    final_content = if reply.content.is_empty() { "[!] AI 返回为空".into() } else { reply.content };
                    let mut fam = json!({"role":"assistant","content":final_content});
                    if !reply.reasoning_content.is_empty() { fam["reasoning_content"] = json!(reply.reasoning_content); }
                    self.history.push(fam);
                    if let Some(tx) = stream_tx {
                        let _ = tx.send(StreamMsg { kind: StreamKind::Reply, content: "AI 回复就绪".into() });
                    }
                    break;
                }
            }
        }

        if CLEAR_HISTORY_FLAG.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.history = vec![json!({"role":"system","content":self.system_prompt.clone()})];
            if final_content.is_empty() {
                final_content = "[+] 对话历史已清除 — 系统提示词已重置".into();
            } else {
                final_content = format!("{}\n\n[+] 对话历史已清除 — 系统提示词已重置", final_content);
            }
        }

        let max_history = MAX_HISTORY_MESSAGES + 2;
        if self.history.len() > max_history {
            let sys = self.history[0].clone();
            let tail = self.history.split_off(self.history.len().saturating_sub(MAX_HISTORY_MESSAGES));
            self.history = vec![sys];
            self.history.extend(tail);

            let mut in_block = false;
            let mut cut_at: Option<usize> = None;
            for i in 1..self.history.len() {
                let role = self.history[i].get("role").and_then(|r| r.as_str()).unwrap_or("");
                let has_tc = self.history[i].get("tool_calls").is_some();
                match (role, has_tc) {
                    ("assistant", true) => in_block = true,
                    ("tool", _) => {
                        if !in_block { cut_at = Some(i); break; }
                    }
                    _ => in_block = false,
                }
            }
            if let Some(start) = cut_at {
                self.history.truncate(start);
            }

            if let Some(last) = self.history.last() {
                if last.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    && last.get("tool_calls").is_some()
                {
                    self.history.pop();
                }
            }
        }

        final_content = strip_markdown(&final_content);

        Ok((final_content, tool_logs))
    }
}
