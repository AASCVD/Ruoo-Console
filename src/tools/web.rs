// ── HTTP / Web 工具 ──
use std::time::Duration;
use crate::telemetry;

pub fn http_get(url: &str) -> Result<(u16, String, String), String> {
    let resp = ureq::get(url).set("User-Agent","ruoo-arsenal/4.0").timeout(Duration::from_secs(15)).call()
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    let headers: Vec<String> = resp.headers_names().iter().map(|n| format!("{}: {}", n, resp.header(n).unwrap_or(""))).collect();
    let body = resp.into_string().unwrap_or_default();
    let host = url.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("unknown");
    let port = if url.starts_with("https://") { 443u16 } else { 80u16 };
    telemetry::push_network(host, port, "HTTP", telemetry::NetDirection::Both, Some(256 + body.len() as u64), None);
    Ok((status, headers.join("\n"), body))
}

pub fn http_post(url: &str, body: &str, content_type: &str) -> Result<(u16, String, String), String> {
    let resp = ureq::post(url).set("User-Agent","ruoo-arsenal/4.0").set("Content-Type",content_type)
        .timeout(Duration::from_secs(15)).send_string(body).map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    let headers: Vec<String> = resp.headers_names().iter().map(|n| format!("{}: {}", n, resp.header(n).unwrap_or(""))).collect();
    let resp_body = resp.into_string().unwrap_or_default();
    let host = url.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("unknown");
    let port = if url.starts_with("https://") { 443u16 } else { 80u16 };
    telemetry::push_network(host, port, "HTTP", telemetry::NetDirection::Both, Some(256 + body.len() as u64 + resp_body.len() as u64), None);
    Ok((status, headers.join("\n"), resp_body))
}

// ═══ v4.5 升级: http_get_hdr / http_post_hdr — 支持自定义 Headers ═══
// headers_json: JSON对象字符串，如 {"Referer":"https://bilibili.com","Cookie":"SESSDATA=xxx"}
pub fn http_get_hdr(url: &str, headers_json: &str) -> Result<(u16, String, String), String> {
    let mut req = ureq::get(url).set("User-Agent", "ruoo-arsenal/4.0").timeout(Duration::from_secs(15));
    // 解析并设置自定义 headers
    if !headers_json.is_empty() {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(headers_json) {
            if let Some(map) = obj.as_object() {
                for (k, v) in map {
                    if let Some(val) = v.as_str() {
                        req = req.set(k, val);
                    }
                }
            }
        }
    }
    let resp = req.call().map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    let headers: Vec<String> = resp.headers_names().iter().map(|n| format!("{}: {}", n, resp.header(n).unwrap_or(""))).collect();
    let resp_body = resp.into_string().unwrap_or_default();
    let host = url.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("unknown");
    let port = if url.starts_with("https://") { 443u16 } else { 80u16 };
    telemetry::push_network(host, port, "HTTP", telemetry::NetDirection::Both, Some(256 + resp_body.len() as u64), None);
    Ok((status, headers.join("\n"), resp_body))
}

pub fn http_post_hdr(url: &str, body: &str, content_type: &str, headers_json: &str) -> Result<(u16, String, String), String> {
    let mut req = ureq::post(url).set("User-Agent", "ruoo-arsenal/4.0").set("Content-Type", content_type).timeout(Duration::from_secs(15));
    // 解析并设置自定义 headers
    if !headers_json.is_empty() {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(headers_json) {
            if let Some(map) = obj.as_object() {
                for (k, v) in map {
                    if let Some(val) = v.as_str() {
                        req = req.set(k, val);
                    }
                }
            }
        }
    }
    let resp = req.send_string(body).map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    let hdrs: Vec<String> = resp.headers_names().iter().map(|n| format!("{}: {}", n, resp.header(n).unwrap_or(""))).collect();
    let resp_body = resp.into_string().unwrap_or_default();
    let host = url.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("unknown");
    let port = if url.starts_with("https://") { 443u16 } else { 80u16 };
    telemetry::push_network(host, port, "HTTP", telemetry::NetDirection::Both, Some(256 + body.len() as u64 + resp_body.len() as u64), None);
    Ok((status, hdrs.join("\n"), resp_body))
}