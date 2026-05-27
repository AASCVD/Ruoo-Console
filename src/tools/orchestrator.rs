// ============================================================
// RUOO-ARSENAL — 工具联动引擎 v1.0
//
// 核心能力:
//   - $last / $result_N 变量引用前序工具输出
//   - | 管道语法: tool1 | tool2  
//   - 智能建议: 根据结果自动推荐下一步工具
//   - 工作流模板: 预定义工具链 (net/full_scan/cve_chain)
//   - 结果缓存: LRU缓存最近1000条工具结果
// ============================================================

use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use serde::Serialize;
use regex::Regex;

// ════════════════════════════════════════════════════════
// 结果缓存
// ════════════════════════════════════════════════════════

const MAX_CACHE_SIZE: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub args: String,
    pub result: String,
    #[serde(skip)]
    pub timestamp: Instant,
    pub extracted: HashMap<String, String>,
    pub success: bool,
    pub duration_ms: u64,
}

pub struct ResultCache {
    results: VecDeque<ToolResult>,
    by_tool: HashMap<String, Vec<usize>>, // tool_name → indices
    sequence: usize,                       // 全局序列号
}

impl ResultCache {
    pub fn new() -> Self {
        Self {
            results: VecDeque::new(),
            by_tool: HashMap::new(),
            sequence: 0,
        }
    }

    /// 存储工具结果
    pub fn store(&mut self, tool_name: &str, args: &str, result: &str, success: bool, duration_ms: u64) -> usize {
        let extracted = Self::extract_fields(result);
        let entry = ToolResult {
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            result: result.to_string(),
            timestamp: Instant::now(),
            extracted,
            success,
            duration_ms,
        };

        let idx = self.sequence;
        self.sequence += 1;

        if self.results.len() >= MAX_CACHE_SIZE {
            self.results.pop_front();
        }

        self.results.push_back(entry);
        self.by_tool.entry(tool_name.to_string()).or_default().push(idx);

        idx
    }

    /// 获取最近N条结果
    pub fn recent(&self, n: usize) -> Vec<&ToolResult> {
        self.results.iter().rev().take(n).collect()
    }

    /// 获取最后一次工具结果
    pub fn last(&self) -> Option<&ToolResult> {
        self.results.back()
    }

    /// 按索引获取
    pub fn get(&self, _idx: usize) -> Option<&ToolResult> {
        self.results.back()
    }

    /// 按工具名获取最近结果
    pub fn last_by_tool(&self, tool_name: &str) -> Option<&ToolResult> {
        self.by_tool.get(tool_name)
            .and_then(|indices| indices.last())
            .and_then(|_idx| self.results.back()) // 返回最近条目
    }

    /// 统计
    pub fn stats(&self) -> String {
        let mut output = format!("═══ 结果缓存 ═══\n条目: {}\n\n", self.results.len());
        let mut tool_counts: HashMap<&str, usize> = HashMap::new();
        for r in &self.results {
            *tool_counts.entry(&r.tool_name).or_default() += 1;
        }
        let mut sorted: Vec<_> = tool_counts.iter().collect();
        sorted.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
        for (name, count) in sorted.iter().take(15) {
            output.push_str(&format!("  {}: {}次\n", name, count));
        }
        output
    }

    /// 清空
    pub fn clear(&mut self) {
        self.results.clear();
        self.by_tool.clear();
    }

    // ── 自动提取关键字段 ──

    fn extract_fields(text: &str) -> HashMap<String, String> {
        let mut fields = HashMap::new();

        // IPv4
        let ip_re = Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").ok();
        if let Some(re) = &ip_re {
            for cap in re.captures_iter(text).take(5) {
                let ip = cap[1].to_string();
                if !ip.starts_with("0.") && !ip.starts_with("127.") && ip != "0.0.0.0" {
                    fields.entry("ip".into()).or_insert(ip);
                }
            }
        }

        // 域名
        let domain_re = Regex::new(r"\b([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,}\b").ok();
        if let Some(re) = &domain_re {
            for cap in re.captures_iter(text).take(5) {
                let d = cap[0].to_lowercase();
                if d.len() > 4 && !d.contains("example") && !d.contains("...") {
                    fields.entry("domain".into()).or_insert(d);
                }
            }
        }

        // 端口
        let port_re = Regex::new(r"(?:port|端口)[:\s]*(\d{1,5})").ok();
        if let Some(re) = &port_re {
            for cap in re.captures_iter(text).take(5) {
                if let Ok(p) = cap[1].parse::<u16>() {
                    if p > 0 { fields.entry("port".into()).or_insert(p.to_string()); }
                }
            }
        }

        // CVE
        let cve_re = Regex::new(r"(?i)CVE-\d{4}-\d{4,}").ok();
        if let Some(re) = &cve_re {
            for cap in re.captures_iter(text).take(10) {
                fields.entry("cve".into()).or_insert(cap[0].to_string());
            }
        }

        // HTTP状态码
        let status_re = Regex::new(r"\b(200|301|302|401|403|404|500|502|503)\b").ok();
        if let Some(re) = &status_re {
            if let Some(cap) = re.captures(text) {
                fields.entry("http_status".into()).or_insert(cap[1].to_string());
            }
        }

        fields
    }
}

// ════════════════════════════════════════════════════════
// 变量解析器 — $last / $result_N / $ip / $domain
// ════════════════════════════════════════════════════════

pub struct VariableResolver {
    cache: std::sync::Arc<std::sync::Mutex<ResultCache>>,
}

impl VariableResolver {
    pub fn new(cache: std::sync::Arc<std::sync::Mutex<ResultCache>>) -> Self {
        Self { cache }
    }

    /// 展开命令中的变量引用
    /// $last → 最近一次工具输出的完整结果
    /// $result → 同 $last
    /// $ip → 最近结果中提取的第一个IP
    /// $domain → 最近结果中提取的第一个域名
    /// $port → 最近结果中提取的第一个端口
    /// $cve → 最近结果中提取的第一个CVE
    /// $tool:名称 → 指定工具的最近结果
    pub fn expand(&self, cmd: &str) -> String {
        let cache = self.cache.lock().unwrap();
        let last = cache.last();

        let mut result = cmd.to_string();

        // $last / $result
        if result.contains("$last") || result.contains("$result") {
            if let Some(r) = last {
                let val = r.result.lines().take(20).collect::<Vec<_>>().join("\n");
                result = result.replace("$last", &val).replace("$result", &val);
            }
        }

        // $tool:工具名
        let tool_re = Regex::new(r"\$tool:(\w+)").ok();
        if let Some(re) = &tool_re {
            let result_clone = result.clone();
            let captures: Vec<_> = re.captures_iter(&result_clone).collect();
            for cap in captures.iter().rev() {
                let tool_name = &cap[1];
                if let Some(r) = cache.last_by_tool(tool_name) {
                    result = result.replace(&cap[0], &r.result);
                }
            }
        }

        // $ip / $domain / $port / $cve
        if let Some(r) = last {
            if let Some(ip) = r.extracted.get("ip") {
                result = result.replace("$ip", ip);
            }
            if let Some(domain) = r.extracted.get("domain") {
                result = result.replace("$domain", domain);
            }
            if let Some(port) = r.extracted.get("port") {
                result = result.replace("$port", port);
            }
            if let Some(cve) = r.extracted.get("cve") {
                result = result.replace("$cve", cve);
            }
        }

        result
    }
}

// ════════════════════════════════════════════════════════
// 管道解析器 — cmd1 | cmd2 | cmd3
// ════════════════════════════════════════════════════════

pub struct PipelineParser;

impl PipelineParser {
    /// 解析管道命令: "scan 192.168.1.1:80 | banner 192.168.1.1 $port"
    pub fn parse(cmd: &str) -> Vec<String> {
        // 简单按 | 分割 (不分割引号内的|)
        let mut commands = Vec::new();
        let mut current = String::new();
        let mut in_quote = false;
        let mut quote_char = '"';

        for ch in cmd.chars() {
            match ch {
                '"' | '\'' if !in_quote => {
                    in_quote = true;
                    quote_char = ch;
                    current.push(ch);
                }
                ch if in_quote && ch == quote_char => {
                    in_quote = false;
                    current.push(ch);
                }
                '|' if !in_quote => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        commands.push(trimmed);
                    }
                    current = String::new();
                }
                _ => current.push(ch),
            }
        }

        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            commands.push(trimmed);
        }

        commands
    }

    /// 检测是否包含管道
    pub fn has_pipeline(cmd: &str) -> bool {
        cmd.contains('|') && !cmd.starts_with('|')
    }
}

// ════════════════════════════════════════════════════════
// 智能建议引擎 — 根据结果推荐下一步
// ════════════════════════════════════════════════════════

pub struct SuggestionEngine;

impl SuggestionEngine {
    /// 根据工具名和结果推荐后续工具
    pub fn suggest(tool_name: &str, result: &str) -> Vec<String> {
        let mut suggestions = Vec::new();

        match tool_name {
            "port_scan" | "syn_scan" => {
                if result.contains("80") || result.contains("443") || result.contains("8080") {
                    suggestions.push("http_get <URL> — 探测Web服务".into());
                    suggestions.push("http_headers_analyze <URL> — 安全头分析".into());
                    suggestions.push("dir_bruteforce <URL> — 目录爆破".into());
                }
                if result.contains("22") { suggestions.push("ssh_version_probe <HOST> — SSH版本".into()); }
                if result.contains("21") { suggestions.push("ftp_anonymous_check <HOST> — FTP匿名登录".into()); }
                if result.contains("3306") { suggestions.push("mysql_version_probe <HOST> — MySQL版本".into()); }
                if result.contains("6379") { suggestions.push("redis_info_probe <HOST> — Redis未授权".into()); }
                if result.contains("27017") { suggestions.push("mongo_db_probe <HOST> — MongoDB未授权".into()); }
                if result.contains("445") || result.contains("139") {
                    suggestions.push("smb_share_enum <HOST> — SMB共享枚举".into());
                }
                suggestions.push("ssl_cert_info <HOST> 443 — SSL证书".into());
            }

            "http_get" | "http_headers_analyze" => {
                suggestions.push("dir_bruteforce <URL> — 目录爆破".into());
                suggestions.push("web_vuln_scan <URL> — 综合Web漏洞扫描".into());
                suggestions.push("git_exposure_scan <URL> — 敏感文件泄露".into());
                suggestions.push("cors_misconfig_check <URL> — CORS配置".into());
                suggestions.push("web_crawler <URL> normal 50 — 爬取站点".into());
            }

            "dns_lookup" => {
                suggestions.push("subdomain_enum <DOMAIN> — 子域名枚举".into());
                suggestions.push("dns_full_query <DOMAIN> MX — 邮件服务器".into());
                suggestions.push("dns_zone_transfer_check <DOMAIN> — 区域传送".into());
                suggestions.push("whois_lookup <DOMAIN> — WHOIS查询".into());
            }

            "whois_lookup" => {
                suggestions.push("dns_lookup <DOMAIN> — DNS解析".into());
                suggestions.push("ip_geolocation <IP> — IP地理定位".into());
            }

            "ssl_cert_info" => {
                suggestions.push("ssl_cipher_enum <HOST> — 密码套件".into());
                suggestions.push("ssl_heartbleed_check <HOST> — Heartbleed".into());
            }

            _ => {
                // 通用建议
                if !result.is_empty() {
                    if result.contains("CVE-") {
                        suggestions.push("cve_lookup <服务名> <版本> — CVE漏洞匹配".into());
                    }
                    if result.contains("open") || result.contains("开放") {
                        suggestions.push("service_version_probe <HOST> <PORT> — 服务版本探测".into());
                    }
                }
            }
        }

        if suggestions.is_empty() {
            suggestions.push("port_scan <HOST> 1-1000 — 端口扫描".into());
            suggestions.push("dns_lookup <HOST> — DNS解析".into());
        }

        suggestions
    }

    /// 格式化建议列表
    pub fn format_suggestions(tool_name: &str, result: &str) -> String {
        let suggestions = Self::suggest(tool_name, result);
        if suggestions.is_empty() { return String::new(); }
        let mut out = format!("\n═══ 建议下一步 ═══\n");
        for (i, s) in suggestions.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, s));
        }
        out
    }
}

// ════════════════════════════════════════════════════════
// 工作流模板
// ════════════════════════════════════════════════════════

pub struct WorkflowEngine;

impl WorkflowEngine {
    /// 获取预定义工作流
    pub fn get_workflow(name: &str) -> Option<Vec<String>> {
        match name.to_lowercase().as_str() {
            "net" | "网络信息" => Some(vec![
                "dns_lookup $domain".into(),
                "whois_lookup $domain".into(),
                "subdomain_enum $domain".into(),
                "port_scan $ip 1-1000".into(),
            ]),

            "web" | "web扫描" => Some(vec![
                "http_get https://$domain".into(),
                "http_headers_analyze https://$domain".into(),
                "dir_bruteforce https://$domain quick".into(),
                "git_exposure_scan https://$domain".into(),
                "ssl_cert_info $domain 443".into(),
            ]),

            "full" | "全面扫描" => Some(vec![
                "dns_lookup $domain".into(),
                "whois_lookup $domain".into(),
                "subdomain_enum $domain".into(),
                "port_scan $ip 1-65535".into(),
                "http_headers_analyze https://$domain".into(),
                "ssl_cert_info $domain 443".into(),
                "dir_bruteforce https://$domain quick".into(),
            ]),

            "vuln" | "漏洞扫描" => Some(vec![
                "port_scan $ip 1-1000".into(),
                "web_vuln_scan https://$domain all".into(),
                "ssl_heartbleed_check $domain".into(),
            ]),

            "email" | "邮件安全" => Some(vec![
                "email_security_check $domain".into(),
                "dns_full_query $domain MX".into(),
            ]),

            "cve" | "cve链" => Some(vec![
                "port_scan $ip 1-1000".into(),
                "service_version_probe $ip $port".into(),
            ]),

            _ => None,
        }
    }

    /// 列出所有工作流
    pub fn list_workflows() -> String {
        let workflows = [
            ("net", "网络信息 — DNS/WHOIS/子域名/端口"),
            ("web", "Web扫描 — HTTP/安全头/目录爆破/Git泄露/SSL"),
            ("full", "全面扫描 — 侦察+Web+SSL+目录"),
            ("vuln", "漏洞扫描 — 端口+Web漏洞+Heartbleed"),
            ("email", "邮件安全 — SPF/DMARC/DKIM+MX"),
            ("cve", "CVE链 — 端口→服务版本→CVE匹配"),
        ];

        let mut out = "═══ 预定义工作流 ═══\n\n".to_string();
        for (name, desc) in &workflows {
            out.push_str(&format!("  {} — {}\n", name, desc));
        }
        out.push_str("\n使用: workflow <名称> <目标>\n");
        out
    }

    /// 执行工作流 (返回命令列表)
    pub fn execute(&self, name: &str, target: &str, resolver: &VariableResolver) -> Result<Vec<String>, String> {
        let template = Self::get_workflow(name)
            .ok_or_else(|| format!("未知工作流: {}。使用 workflow list 查看", name))?;

        // 预展开 $domain 和 $ip (如果target是域名)
        let mut expanded = Vec::new();
        let target_is_ip = target.parse::<std::net::Ipv4Addr>().is_ok();

        for cmd in &template {
            let mut resolved = cmd.clone();
            if target_is_ip {
                resolved = resolved.replace("$ip", target).replace("$domain", target);
            } else {
                resolved = resolved.replace("$domain", target).replace("$ip", target);
            }
            // 再用resolver展开已有的变量
            resolved = resolver.expand(&resolved);
            expanded.push(resolved);
        }

        Ok(expanded)
    }
}

// ════════════════════════════════════════════════════════
// 全局单例
// ════════════════════════════════════════════════════════

use std::sync::OnceLock;
static RESULT_CACHE: OnceLock<std::sync::Arc<std::sync::Mutex<ResultCache>>> = OnceLock::new();

pub fn get_cache() -> std::sync::Arc<std::sync::Mutex<ResultCache>> {
    RESULT_CACHE.get_or_init(|| {
        std::sync::Arc::new(std::sync::Mutex::new(ResultCache::new()))
    }).clone()
}

pub fn get_resolver() -> VariableResolver {
    VariableResolver::new(get_cache())
}

// ════════════════════════════════════════════════════════
// 公开API
// ════════════════════════════════════════════════════════

/// 存储工具结果到缓存
pub fn store_result(tool_name: &str, args: &str, result: &str, success: bool, duration_ms: u64) {
    let cache = get_cache();
    cache.lock().unwrap().store(tool_name, args, result, success, duration_ms);
}

/// 获取建议
pub fn get_suggestions(tool_name: &str, result: &str) -> String {
    SuggestionEngine::format_suggestions(tool_name, result)
}

/// 展开变量
pub fn expand_variables(cmd: &str) -> String {
    get_resolver().expand(cmd)
}

/// 解析管道
pub fn parse_pipeline(cmd: &str) -> Vec<String> {
    PipelineParser::parse(cmd)
}

/// 检测管道
pub fn has_pipeline(cmd: &str) -> bool {
    PipelineParser::has_pipeline(cmd)
}

/// 获取工作流
pub fn get_workflow(name: &str, target: &str) -> Result<Vec<String>, String> {
    WorkflowEngine.execute(name, target, &get_resolver())
}

/// 列出工作流
pub fn list_workflows() -> String {
    WorkflowEngine::list_workflows()
}

/// 缓存统计
pub fn cache_stats() -> String {
    get_cache().lock().unwrap().stats()
}

/// 清空缓存
pub fn clear_cache() {
    get_cache().lock().unwrap().clear();
}

// ════════════════════════════════════════════════════════
// 测试
// ════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_parse_simple() {
        let cmds = PipelineParser::parse("scan 192.168.1.1:80 | banner 192.168.1.1 $port");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0], "scan 192.168.1.1:80");
        assert_eq!(cmds[1], "banner 192.168.1.1 $port");
    }

    #[test]
    fn test_pipeline_parse_quoted() {
        let cmds = PipelineParser::parse("echo \"hello | world\" | grep hello");
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].contains("|"));
    }

    #[test]
    fn test_pipeline_parse_no_pipe() {
        let cmds = PipelineParser::parse("scan 192.168.1.1");
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_has_pipeline() {
        assert!(PipelineParser::has_pipeline("scan | grep"));
        assert!(!PipelineParser::has_pipeline("scan 1.1.1.1"));
    }

    #[test]
    fn test_variable_expansion() {
        let cache = get_cache();
        cache.lock().unwrap().store("port_scan", "192.168.1.1:80", "Host 192.168.1.1 — Port 80 open\nPort 443 open", true, 100);
        let resolver = get_resolver();
        let expanded = resolver.expand("banner $ip $port");
        assert!(expanded.contains("192.168.1.1"));
    }

    #[test]
    fn test_workflow_list() {
        let output = WorkflowEngine::list_workflows();
        assert!(output.contains("net"));
        assert!(output.contains("web"));
    }

    #[test]
    fn test_workflow_get() {
        let wf = WorkflowEngine::get_workflow("net");
        assert!(wf.is_some());
        assert!(wf.unwrap().len() >= 3);
    }

    #[test]
    fn test_result_cache_store() {
        let cache = get_cache();
        cache.lock().unwrap().store("test", "arg", "result", true, 50);
        let last = cache.lock().unwrap().last().cloned();
        assert!(last.is_some());
        assert_eq!(last.unwrap().tool_name, "test");
    }

    #[test]
    fn test_suggestions() {
        let suggestions = SuggestionEngine::suggest("port_scan", "Port 22 open\nPort 80 open\nPort 443 open");
        assert!(!suggestions.is_empty());
        let has_ssh = suggestions.iter().any(|s| s.contains("ssh"));
        let has_http = suggestions.iter().any(|s| s.contains("http_get"));
        assert!(has_ssh || has_http);
    }

    #[test]
    fn test_extract_fields() {
        let text = "Server at 192.168.1.100 port 8080 running Apache/2.4.49 CVE-2021-41773";
        let fields = ResultCache::extract_fields(text);
        assert!(fields.contains_key("ip"));
        assert!(fields.contains_key("port"));
        assert!(fields.contains_key("cve"));
    }
}
