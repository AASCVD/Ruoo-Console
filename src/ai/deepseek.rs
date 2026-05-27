// === DeepSeek API 调用 ===
// sanitize_history, truncate_tool_result, call_deepseek

use serde_json::{json, Value};
use crate::ai::types::{AiResponse, ToolCall, StreamMsg, StreamKind};

/// 发送前清理: 移除失去 tool_calls 父亲的孤儿 tool/assistant 消息对
fn sanitize_history(history: &[Value]) -> Vec<Value> {
    if history.is_empty() { return history.to_vec(); }
    let mut clean = Vec::with_capacity(history.len());
    clean.push(history[0].clone()); // system
    let mut pending_tool_calls: usize = 0;
    let mut orphan_count: usize = 0;

    for i in 1..history.len() {
        let role = history[i].get("role").and_then(|r| r.as_str()).unwrap_or("");
        let has_tool_calls = history[i].get("tool_calls").is_some();

        match (role, has_tool_calls) {
            ("assistant", true) => {
                let tc_count = history[i].get("tool_calls")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                pending_tool_calls += tc_count;
                let mut am = history[i].clone();
                if let Some(tcs) = am.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    for tc in tcs {
                        if let Some(func) = tc.get_mut("function") {
                            if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                func["arguments"] = json!(args);
                            }
                        }
                    }
                }
                clean.push(am);
            }
            ("tool", _) => {
                if pending_tool_calls > 0 {
                    pending_tool_calls -= 1;
                    clean.push(truncate_tool_result(history[i].clone()));
                } else {
                    orphan_count += 1;
                    let mut m = history[i].clone();
                    m["role"] = json!("tool");
                    if let Some(c) = m.get("content").and_then(|c| c.as_str()) {
                        m["content"] = json!(format!("[orphan#{}] {}", orphan_count, c));
                    }
                    clean.push(m);
                    pending_tool_calls = 0;
                }
            }
            ("user" | "system", _) => {
                pending_tool_calls = 0;
                orphan_count = 0;
                clean.push(history[i].clone());
            }
            _ => {
                clean.push(history[i].clone());
            }
        }
    }

    while let Some(last) = clean.last() {
        if last.get("role").and_then(|r| r.as_str()) == Some("assistant")
            && last.get("tool_calls").is_some()
        {
            clean.pop();
        } else {
            break;
        }
    }
    clean
}

fn truncate_tool_result(mut msg: Value) -> Value {
    const LIMIT: usize = 8192;
    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
        if content.len() > LIMIT {
            let head: String = content.chars().take(LIMIT - 60).collect();
            msg["content"] = json!(format!("{}…[截断: {}→8KB, 用 read_file_lines 查细节]", head, content.len()));
        }
    }
    msg
}

pub fn call_deepseek(
    api_key: &str, api_url: &str, model: &str,
    history: &[Value], tools: &[Value],
    stream_tx: Option<&std::sync::mpsc::Sender<StreamMsg>>,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<AiResponse, String> {
    let clean_history = sanitize_history(history);
    let safe_history: Vec<Value> = clean_history.into_iter().map(|mut m| {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        match role {
            "tool" => {
                if let Some(c) = m.get("content").and_then(|c| c.as_str()) {
                    if c.len() > 8192 {
                        let head: String = c.chars().take(8000).collect();
                        m["content"] = json!(format!("{}…[截断: {}→8KB]", head, c.len()));
                    }
                }
            }
            "assistant" => {
                if let Some(tcs) = m.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    for tc in tcs {
                        if let Some(func) = tc.get_mut("function") {
                            if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                func["arguments"] = json!(args);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        m
    }).collect();
    let body = json!({
        "model": model,
        "messages": safe_history,
        "max_tokens": 16384,
        "stream": false,
        "tools": tools,
        "tool_choice": "auto",
    });
    let body_str = body.to_string();

    const MAX_RETRIES: u32 = 3; // 有超时保护后减少重试，避免长时间卡住
    let mut last_err = String::new();
    let mut was_retrying = false;

    for attempt in 0..MAX_RETRIES {
        // ★ 检查取消标志 — 用户按 Esc 立即中断重试
        if let Some(c) = cancel {
            if c.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("AI 请求已被用户取消 (Esc)".into());
            }
        }
        if attempt > 0 {
            let backoff_secs = (attempt as u64).min(5).max(1);
            if !was_retrying {
                was_retrying = true;
                if let Some(tx) = stream_tx {
                    let _ = tx.send(StreamMsg { kind: StreamKind::Retry,
                        content: format!("网络波动，自动重连 DeepSeek… (第1次)") });
                }
            }
            if let Some(tx) = stream_tx {
                let _ = tx.send(StreamMsg { kind: StreamKind::Retry,
                    content: format!("等待 {}s 后第 {} 次重试… (上次: {})",
                        backoff_secs, attempt, last_err) });
            }
            std::thread::sleep(std::time::Duration::from_secs(backoff_secs));
        }

        let client = ureq::AgentBuilder::new()
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .timeout_connect(std::time::Duration::from_secs(30))
            .timeout_read(std::time::Duration::from_secs(900))
            .timeout_write(std::time::Duration::from_secs(60))
            .build();

        let response = match client
            .post(api_url)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("Connection", "close")
            .send_string(&body_str)
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, resp)) => {
                let b = resp.into_string().unwrap_or_default();
                last_err = if b.is_empty() { format!("HTTP {} (空body)", code) }
                           else { format!("HTTP {}: {}", code, b.chars().take(80).collect::<String>()) };
                continue;
            }
            Err(e) => {
                last_err = format!("{}", e);
                if last_err.contains("ConnectFailure") || last_err.contains("connection") {
                    last_err = "连接失败 (网络未就绪)".into();
                } else if last_err.contains("Timeout") || last_err.contains("timeout") {
                    last_err = "请求超时".into();
                }
                continue;
            }
        };

        let status = response.status();
        let cl_hdr = response.header("Content-Length").map(|s| s.to_string());
        let ct_hdr = response.header("Content-Type").map(|s| s.to_string());
        let te_hdr = response.header("Transfer-Encoding").map(|s| s.to_string());
        let resp_text = response.into_string().unwrap_or_default();

        if resp_text.is_empty() {
            if attempt == 0 {
                let _ = std::fs::write("debug_deepseek_request.json", &body_str);
            }
            last_err = format!("HTTP {} 空body (TE={}, CL={}, CT={}) [dump→debug_deepseek_request.json]",
                status,
                te_hdr.as_deref().unwrap_or("?"),
                cl_hdr.as_deref().unwrap_or("?"),
                ct_hdr.as_deref().unwrap_or("?"));
            continue;
        }

        if status != 200 {
            let preview: String = resp_text.chars().take(80).collect();
            if status >= 400 && status < 500 && status != 429 {
                return Err(format!("HTTP {} (客户端错误): {}", status, preview));
            }
            last_err = format!("HTTP {}: {}", status, preview);
            continue;
        }

        let resp_body: Value = match serde_json::from_str(&resp_text) {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("JSON解析: {}", e);
                continue;
            }
        };

        if let Some(err) = resp_body.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("未知");
            return Err(format!("API 错误: {}", msg));
        }

        if was_retrying {
            if let Some(tx) = stream_tx {
                let _ = tx.send(StreamMsg { kind: StreamKind::Retry,
                    content: format!("已恢复连接! (共 {} 次重试)", attempt) });
            }
        }

        let msg = &resp_body["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let reasoning = msg["reasoning_content"].as_str().unwrap_or("").to_string();

        let tool_calls = msg["tool_calls"].as_array().and_then(|arr| {
            let calls: Vec<_> = arr.iter().filter_map(|tc| Some(ToolCall {
                id: tc["id"].as_str()?.to_string(),
                name: tc["function"]["name"].as_str()?.to_string(),
                arguments: tc["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
            })).collect();
            if calls.is_empty() { None } else { Some(calls) }
        });

        return Ok(AiResponse { content, reasoning_content: reasoning, tool_calls });
    }

    Err(format!("网络不通 ({}次重试均失败): {}", MAX_RETRIES, last_err))
}
