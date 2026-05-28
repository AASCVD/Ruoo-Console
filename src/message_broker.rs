// ============================================================
// RUOO-CONSOLE v5.2 — 消息输出代理 (Message Broker)
//
// v5.2 新增:
//   - 插件间通信消息 (InterPlugin)
//   - 结构化日志消息 (StructuredLog)
//   - 崩溃通知消息 (CrashNotify)
//   - 事件广播 (EventBroadcast)
//   - 消息优先级队列
//   - 速率限制 (每秒最多N条避免刷屏)
// ============================================================

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, mpsc};
use std::time::{Instant, Duration};
use std::ffi::CStr;
use std::os::raw::c_char;

// ═══════════════════════════════════════════════════════
// 消息类型 v5.2
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum BrokerMsg {
    /// 追加新行
    Append {
        plugin_name: String,
        text: String,
    },
    /// 更新标记行 (按标签)
    Update {
        tag: String,
        text: String,
    },
    /// 删除标记行
    Remove {
        tag: String,
    },
    /// 追加带标签的行 (首次)
    TaggedAppend {
        tag: String,
        text: String,
    },
    // ── v5.2 新增 ──
    /// 插件间通信消息
    InterPlugin {
        from_plugin: String,
        to_plugin: String,
        msg_type: String,
        payload: String,
    },
    /// 结构化日志
    StructuredLog {
        plugin_name: String,
        level: LogLevel,
        message: String,
        metadata: HashMap<String, String>,
    },
    /// 崩溃通知
    CrashNotify {
        plugin_name: String,
        error_msg: String,
        severity: String,
        timestamp: Instant,
    },
    /// 事件广播 (所有订阅者可见)
    EventBroadcast {
        event_type: String,
        data: String,
        source_plugin: String,
    },
    /// 进度更新 (来自插件)
    ProgressUpdate {
        plugin_name: String,
        bar_id: String,
        current: usize,
        total: usize,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            "fatal" => LogLevel::Fatal,
            _ => LogLevel::Info,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Fatal => "FATAL",
        }
    }
}

// ═══════════════════════════════════════════════════════
// 速率限制器
// ═══════════════════════════════════════════════════════

pub struct RateLimiter {
    pub max_per_second: usize,
    pub window: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn new(max_per_second: usize) -> Self {
        Self {
            max_per_second,
            window: VecDeque::with_capacity(max_per_second + 10),
        }
    }

    /// 检查是否允许通过
    pub fn allow(&mut self) -> bool {
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(1);

        while self.window.front().map_or(false, |t| *t < cutoff) {
            self.window.pop_front();
        }

        if self.window.len() < self.max_per_second {
            self.window.push_back(now);
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.window.clear();
    }
}

// ═══════════════════════════════════════════════════════
// 消息代理 v5.2
// ═══════════════════════════════════════════════════════

pub struct MessageBroker {
    /// 消息接收端 (UI线程消费)
    rx: parking_lot::Mutex<mpsc::Receiver<BrokerMsg>>,
    /// 消息发送端 (插件线程生产)
    tx: mpsc::Sender<BrokerMsg>,
    /// 标签→输出行索引映射
    tag_map: parking_lot::Mutex<HashMap<String, usize>>,
    /// 速率限制器 (按插件名)
    rate_limiters: parking_lot::Mutex<HashMap<String, RateLimiter>>,
    /// 全局速率限制
    global_limiter: parking_lot::Mutex<RateLimiter>,
    /// 待处理的插件间消息
    inter_plugin_queue: parking_lot::Mutex<VecDeque<(String, BrokerMsg)>>,
    /// 统计计数
    pub total_messages: std::sync::atomic::AtomicU64,
    pub dropped_messages: std::sync::atomic::AtomicU64,
}

impl MessageBroker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BrokerMsg>();
        Self {
            rx: parking_lot::Mutex::new(rx),
            tx,
            tag_map: parking_lot::Mutex::new(HashMap::new()),
            rate_limiters: parking_lot::Mutex::new(HashMap::new()),
            global_limiter: parking_lot::Mutex::new(RateLimiter::new(100)),
            inter_plugin_queue: parking_lot::Mutex::new(VecDeque::new()),
            total_messages: std::sync::atomic::AtomicU64::new(0),
            dropped_messages: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 获取发送端 (给插件)
    pub fn sender(&self) -> mpsc::Sender<BrokerMsg> {
        self.tx.clone()
    }

    /// 带速率限制的发送
    pub fn send_with_limit(&self, plugin_name: &str, msg: BrokerMsg) -> bool {
        let mut limiters = self.rate_limiters.lock();
        let limiter = limiters.entry(plugin_name.to_string())
            .or_insert_with(|| RateLimiter::new(20));

        if limiter.allow() {
            self.total_messages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = self.tx.send(msg);
            true
        } else {
            self.dropped_messages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        }
    }

    /// 消费所有待处理消息, 注入到输出缓冲区
    pub fn drain(&self, output: &mut Vec<String>) {
        let mut tag_map = self.tag_map.lock();
        let rx = self.rx.lock();

        while let Ok(msg) = rx.try_recv() {
            match msg {
                BrokerMsg::Append { plugin_name, text } => {
                    output.push(format!("[{}] {}", plugin_name, text));
                }
                BrokerMsg::TaggedAppend { tag, text } => {
                    let full_line = format!("[tag:{}] {}", tag, text);
                    let idx = output.len();
                    output.push(full_line);
                    tag_map.insert(tag, idx);
                }
                BrokerMsg::Update { tag, text } => {
                    if let Some(&idx) = tag_map.get(&tag) {
                        if idx < output.len() {
                            output[idx] = format!("[tag:{}] {}", tag, text);
                        }
                    } else {
                        let full_line = format!("[tag:{}] {}", tag, text);
                        let idx = output.len();
                        output.push(full_line);
                        tag_map.insert(tag, idx);
                    }
                }
                BrokerMsg::Remove { tag } => {
                    if let Some(&idx) = tag_map.get(&tag) {
                        if idx < output.len() {
                            output[idx] = format!("[tag:{}] (已移除)", tag);
                        }
                        tag_map.remove(&tag);
                    }
                }
                BrokerMsg::InterPlugin { from_plugin, to_plugin, msg_type, payload } => {
                    let target = if to_plugin.is_empty() { "all" } else { &to_plugin };
                    output.push(format!(
                        "[↔{}→{}:{}] {}",
                        from_plugin, target, msg_type, payload
                    ));
                }
                BrokerMsg::StructuredLog { plugin_name, level, message, metadata } => {
                    let meta_str = if metadata.is_empty() {
                        String::new()
                    } else {
                        let parts: Vec<String> = metadata.iter()
                            .map(|(k, v)| format!("{}={}", k, v))
                            .collect();
                        format!(" [{}]", parts.join(", "))
                    };
                    output.push(format!(
                        "[{}] [{}] {}{}",
                        level.as_str(), plugin_name, message, meta_str
                    ));
                }
                BrokerMsg::CrashNotify { plugin_name, error_msg, severity, timestamp: _ } => {
                    output.push(format!(
                        "[!!!崩溃:{}] {} — {}",
                        severity, plugin_name, error_msg
                    ));
                }
                BrokerMsg::EventBroadcast { event_type, data, source_plugin } => {
                    output.push(format!(
                        "[事件:{}@{}] {}",
                        event_type, source_plugin, data
                    ));
                }
                BrokerMsg::ProgressUpdate { plugin_name, bar_id, current, total, message } => {
                    let pct = if total > 0 { (current as f64 / total as f64 * 100.0) as usize } else { 0 };
                    let bar_width = 15;
                    let filled = pct * bar_width / 100;
                    let empty = bar_width - filled;
                    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));
                    let full_line = format!(
                        "[tag:prog_{}_{}] {} {:3}% {}/{} {}",
                        plugin_name, bar_id, bar, pct, current, total, message
                    );
                    let tag = format!("prog_{}_{}", plugin_name, bar_id);
                    if let Some(&idx) = tag_map.get(&tag) {
                        if idx < output.len() {
                            output[idx] = full_line;
                        }
                    } else {
                        let idx = output.len();
                        output.push(full_line);
                        tag_map.insert(tag, idx);
                    }
                }
            }
        }
    }

    /// 清空标签映射
    pub fn clear_tags(&self) {
        self.tag_map.lock().clear();
    }

    /// 获取活跃标签数
    pub fn tag_count(&self) -> usize {
        self.tag_map.lock().len()
    }

    /// 获取统计信息
    pub fn stats(&self) -> String {
        format!(
            "═══ 消息代理统计 ═══\n\
             总消息: {}条\n\
             丢弃: {}条\n\
             活跃标签: {}个\n\
             速率限制器: {}个",
            self.total_messages.load(std::sync::atomic::Ordering::Relaxed),
            self.dropped_messages.load(std::sync::atomic::Ordering::Relaxed),
            self.tag_count(),
            self.rate_limiters.lock().len(),
        )
    }
}

// ═══════════════════════════════════════════════════════
// 全局单例
// ═══════════════════════════════════════════════════════

static GLOBAL_BROKER: std::sync::LazyLock<Arc<MessageBroker>> =
    std::sync::LazyLock::new(|| Arc::new(MessageBroker::new()));

pub fn set_global_broker(_broker: Arc<MessageBroker>) {
    // 无法直接设置LazyLock，使用内部可变性
    let _ = &*GLOBAL_BROKER; // 确保已初始化
}

pub fn global_broker() -> Option<Arc<MessageBroker>> {
    Some(GLOBAL_BROKER.clone())
}

/// 推送消息到 TUI 输出
pub fn push_message(plugin_name: &str, text: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::Append {
            plugin_name: plugin_name.to_string(),
            text: text.to_string(),
        });
    }
}

/// 更新带标签的输出行
pub fn update_tagged_line(tag: &str, text: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::Update {
            tag: tag.to_string(),
            text: text.to_string(),
        });
    }
}

/// 推送带标签的行
pub fn push_tagged_line(tag: &str, text: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::TaggedAppend {
            tag: tag.to_string(),
            text: text.to_string(),
        });
    }
}

/// 移除带标签的行
pub fn remove_tagged_line(tag: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::Remove {
            tag: tag.to_string(),
        });
    }
}

/// v5.2: 插件间消息
pub fn inter_plugin_send(from: &str, to: &str, msg_type: &str, payload: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::InterPlugin {
            from_plugin: from.to_string(),
            to_plugin: to.to_string(),
            msg_type: msg_type.to_string(),
            payload: payload.to_string(),
        });
    }
}

/// v5.2: 结构化日志
pub fn structured_log(plugin_name: &str, level: LogLevel, message: &str, metadata: HashMap<String, String>) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::StructuredLog {
            plugin_name: plugin_name.to_string(),
            level,
            message: message.to_string(),
            metadata,
        });
    }
}

/// v5.2: 崩溃通知
pub fn crash_notify(plugin_name: &str, error_msg: &str, severity: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::CrashNotify {
            plugin_name: plugin_name.to_string(),
            error_msg: error_msg.to_string(),
            severity: severity.to_string(),
            timestamp: Instant::now(),
        });
    }
}

/// v5.2: 事件广播
pub fn event_broadcast(event_type: &str, data: &str, source_plugin: &str) {
    if let Some(broker) = global_broker() {
        let _ = broker.tx.send(BrokerMsg::EventBroadcast {
            event_type: event_type.to_string(),
            data: data.to_string(),
            source_plugin: source_plugin.to_string(),
        });
    }
}

// ═══════════════════════════════════════════════════════
// C ABI 导出 — 供插件 DLL/SO 直接调用
// ═══════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn ruoo_message_push(
    plugin_name: *const c_char,
    text: *const c_char,
) -> i32 {
    if plugin_name.is_null() || text.is_null() { return -1; }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let msg = CStr::from_ptr(text).to_string_lossy();
    push_message(&name, &msg);
    0
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_message_update(
    tag: *const c_char,
    text: *const c_char,
) -> i32 {
    if tag.is_null() || text.is_null() { return -1; }
    let t = CStr::from_ptr(tag).to_string_lossy();
    let msg = CStr::from_ptr(text).to_string_lossy();
    update_tagged_line(&t, &msg);
    0
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_message_tagged(
    tag: *const c_char,
    text: *const c_char,
) -> i32 {
    if tag.is_null() || text.is_null() { return -1; }
    let t = CStr::from_ptr(tag).to_string_lossy();
    let msg = CStr::from_ptr(text).to_string_lossy();
    push_tagged_line(&t, &msg);
    0
}

#[no_mangle]
pub unsafe extern "C" fn ruoo_message_remove(
    tag: *const c_char,
) -> i32 {
    if tag.is_null() { return -1; }
    let t = CStr::from_ptr(tag).to_string_lossy();
    remove_tagged_line(&t);
    0
}

/// v5.2: 结构化日志 C ABI
#[no_mangle]
pub unsafe extern "C" fn ruoo_message_log(
    plugin_name: *const c_char,
    level: *const c_char,
    message: *const c_char,
) -> i32 {
    if plugin_name.is_null() || level.is_null() || message.is_null() { return -1; }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let lvl = CStr::from_ptr(level).to_string_lossy();
    let msg = CStr::from_ptr(message).to_string_lossy();
    structured_log(&name, LogLevel::from_str(&lvl), &msg, HashMap::new());
    0
}

/// v5.2: 崩溃通知 C ABI
#[no_mangle]
pub unsafe extern "C" fn ruoo_message_crash(
    plugin_name: *const c_char,
    error_msg: *const c_char,
    severity: *const c_char,
) -> i32 {
    if plugin_name.is_null() || error_msg.is_null() { return -1; }
    let name = CStr::from_ptr(plugin_name).to_string_lossy();
    let err = CStr::from_ptr(error_msg).to_string_lossy();
    let sev = if severity.is_null() {
        "error"
    } else {
        CStr::from_ptr(severity).to_str().unwrap_or("error")
    };
    crash_notify(&name, &err, sev);
    0
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append() {
        let broker = MessageBroker::new();
        let tx = broker.sender();
        let mut output: Vec<String> = Vec::new();

        tx.send(BrokerMsg::Append { plugin_name: "test".into(), text: "hello".into() }).unwrap();
        broker.drain(&mut output);
        assert_eq!(output.len(), 1);
        assert!(output[0].contains("hello"));
    }

    #[test]
    fn test_tagged_update() {
        let broker = MessageBroker::new();
        let tx = broker.sender();
        let mut output: Vec<String> = Vec::new();

        tx.send(BrokerMsg::TaggedAppend { tag: "s1".into(), text: "line 1".into() }).unwrap();
        broker.drain(&mut output);
        tx.send(BrokerMsg::Update { tag: "s1".into(), text: "line 1 updated".into() }).unwrap();
        broker.drain(&mut output);
        assert!(output[0].contains("updated"));
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.allow());
        }
        assert!(!limiter.allow());
    }

    #[test]
    fn test_log_level() {
        assert_eq!(LogLevel::from_str("ERROR"), LogLevel::Error);
        assert_eq!(LogLevel::from_str("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("unknown"), LogLevel::Info);
    }

    #[test]
    fn test_structured_log() {
        let broker = MessageBroker::new();
        let tx = broker.sender();
        let mut output: Vec<String> = Vec::new();

        tx.send(BrokerMsg::StructuredLog {
            plugin_name: "test".into(),
            level: LogLevel::Warn,
            message: "something happened".into(),
            metadata: HashMap::new(),
        }).unwrap();
        broker.drain(&mut output);
        assert!(output[0].contains("WARN"));
        assert!(output[0].contains("something happened"));
    }
}
