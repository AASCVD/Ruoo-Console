// ============================================================
// RUOO-CONSOLE v5.0 — 多进度条管理器 (Plugin Progress Bar)
//
// 插件公共进度条接口:
//   - 支持多个插件同时上报进度, 互不干扰
//   - 线程安全: 插件线程写入 → UI 线程读取渲染
//   - C ABI 导出: DLL/SO 插件可直接调用
//   - 自动清理: 超时未更新的进度条自动移除 (300s)
//
// C ABI 接口 (插件侧调用):
//   ruoo_progress_create(plugin, id, label, total) -> handle
//   ruoo_progress_update(handle, current, msg)
//   ruoo_progress_finish(handle)
//   ruoo_progress_set_total(handle, total)
//
// 返回格式: handle = "plugin_name::id"
// ============================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ═══════════════════════════════════════════════════════
// 单个进度条
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// 唯一标识 (plugin_name::bar_id)
    pub handle: String,
    /// 所属插件名
    pub plugin_name: String,
    /// 进度条ID (插件自定义)
    pub bar_id: String,
    /// 显示标签
    pub label: String,
    /// 当前进度
    pub current: usize,
    /// 总进度
    pub total: usize,
    /// 当前消息
    pub message: String,
    /// 创建时间
    pub created_at: Instant,
    /// 最后更新时间
    pub last_update: Instant,
    /// 是否活跃
    pub active: bool,
    /// 是否完成
    pub finished: bool,
}

impl ProgressBar {
    pub fn new(plugin_name: &str, bar_id: &str, label: &str, total: usize) -> Self {
        let now = Instant::now();
        Self {
            handle: format!("{}::{}", plugin_name, bar_id),
            plugin_name: plugin_name.to_string(),
            bar_id: bar_id.to_string(),
            label: label.to_string(),
            current: 0,
            total: total.max(1),
            message: String::new(),
            created_at: now,
            last_update: now,
            active: true,
            finished: false,
        }
    }

    /// 进度百分比
    pub fn percent(&self) -> f64 {
        if self.total == 0 { return 0.0; }
        (self.current as f64 / self.total as f64 * 100.0).min(100.0)
    }

    /// 渲染为单行文本 (40字符宽进度条)
    pub fn render_line(&self) -> String {
        let pct = self.percent();
        let bar_width = 20usize;
        let filled = (pct / 100.0 * bar_width as f64) as usize;
        let empty = bar_width - filled;

        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

        let status = if self.finished {
            "[✓]".to_string()
        } else {
            format!("{:3.0}%", pct)
        };

        let msg = if self.message.is_empty() {
            String::new()
        } else {
            format!(" — {}", self.message)
        };

        format!(
            "{} {} {} {}/{} [{:.1}s]{}",
            status, bar, self.label,
            self.current, self.total,
            self.last_update.duration_since(self.created_at).as_secs_f64(),
            msg
        )
    }

    /// 渲染为紧凑行
    pub fn render_compact(&self) -> String {
        let pct = self.percent();
        let status = if self.finished { "+" } else { " " };
        format!(
            "{} {:.0}% {}{}",
            status, pct, self.label,
            if self.message.is_empty() { String::new() } else { format!(" — {}", self.message) }
        )
    }
}

// ═══════════════════════════════════════════════════════
// 多进度条管理器
// ═══════════════════════════════════════════════════════

pub struct MultiProgressManager {
    bars: parking_lot::Mutex<HashMap<String, ProgressBar>>,
    /// 超时自动清理 (秒)
    stale_timeout_secs: u64,
}

impl MultiProgressManager {
    pub fn new() -> Self {
        Self {
            bars: parking_lot::Mutex::new(HashMap::new()),
            stale_timeout_secs: 300,
        }
    }

    /// 创建进度条, 返回句柄
    pub fn create(&self, plugin_name: &str, bar_id: &str, label: &str, total: usize) -> String {
        let bar = ProgressBar::new(plugin_name, bar_id, label, total);
        let handle = bar.handle.clone();
        let mut bars = self.bars.lock();
        bars.insert(handle.clone(), bar);
        handle
    }

    /// 更新进度
    pub fn update(&self, handle: &str, current: usize, message: &str) -> Result<(), String> {
        let mut bars = self.bars.lock();
        match bars.get_mut(handle) {
            Some(bar) => {
                bar.current = current.min(bar.total);
                bar.message = message.to_string();
                bar.last_update = Instant::now();
                Ok(())
            }
            None => Err(format!("进度条不存在: {}", handle)),
        }
    }

    /// 设置总量
    pub fn set_total(&self, handle: &str, total: usize) -> Result<(), String> {
        let mut bars = self.bars.lock();
        match bars.get_mut(handle) {
            Some(bar) => {
                bar.total = total.max(1);
                bar.last_update = Instant::now();
                Ok(())
            }
            None => Err(format!("进度条不存在: {}", handle)),
        }
    }

    /// 完成进度条
    pub fn finish(&self, handle: &str) -> Result<(), String> {
        let mut bars = self.bars.lock();
        match bars.get_mut(handle) {
            Some(bar) => {
                bar.current = bar.total;
                bar.finished = true;
                bar.active = false;
                bar.last_update = Instant::now();
                Ok(())
            }
            None => Err(format!("进度条不存在: {}", handle)),
        }
    }

    /// 移除进度条
    pub fn remove(&self, handle: &str) {
        let mut bars = self.bars.lock();
        bars.remove(handle);
    }

    /// 清理过期进度条 (超过stale_timeout未更新)
    pub fn cleanup_stale(&self) -> usize {
        let mut bars = self.bars.lock();
        let timeout = Duration::from_secs(self.stale_timeout_secs);
        let now = Instant::now();
        let stale: Vec<String> = bars.iter()
            .filter(|(_, b)| !b.active && now.duration_since(b.last_update) > Duration::from_secs(30))
            .map(|(k, _)| k.clone())
            .collect();
        let count = stale.len();
        for h in &stale { bars.remove(h); }
        // 也清理超时的活跃进度条
        let stale_active: Vec<String> = bars.iter()
            .filter(|(_, b)| b.active && now.duration_since(b.last_update) > timeout)
            .map(|(k, _)| k.clone())
            .collect();
        for h in &stale_active {
            if let Some(bar) = bars.get_mut(h) {
                bar.active = false;
                bar.finished = true;
                bar.message = "(超时)".to_string();
            }
        }
        count + stale_active.len()
    }

    /// 获取所有活跃进度条 (按创建时间排序)
    pub fn active_bars(&self) -> Vec<ProgressBar> {
        let bars = self.bars.lock();
        let mut v: Vec<ProgressBar> = bars.values()
            .filter(|b| b.active || b.finished)
            .cloned()
            .collect();
        v.sort_by_key(|b| b.created_at);
        v
    }

    /// 获取活跃进度条数量
    pub fn active_count(&self) -> usize {
        let bars = self.bars.lock();
        bars.values().filter(|b| b.active).count()
    }

    /// 获取所有进度条 (含已完成)
    pub fn all_bars(&self) -> Vec<ProgressBar> {
        let bars = self.bars.lock();
        let mut v: Vec<ProgressBar> = bars.values().cloned().collect();
        v.sort_by_key(|b| b.created_at);
        v
    }

    /// 按插件名获取进度条
    pub fn bars_by_plugin(&self, plugin_name: &str) -> Vec<ProgressBar> {
        let bars = self.bars.lock();
        let mut v: Vec<ProgressBar> = bars.values()
            .filter(|b| b.plugin_name == plugin_name)
            .cloned()
            .collect();
        v.sort_by_key(|b| b.created_at);
        v
    }

    /// 清除指定插件的所有进度条
    pub fn clear_plugin(&self, plugin_name: &str) -> usize {
        let mut bars = self.bars.lock();
        let to_remove: Vec<String> = bars.iter()
            .filter(|(_, b)| b.plugin_name == plugin_name)
            .map(|(k, _)| k.clone())
            .collect();
        let count = to_remove.len();
        for h in &to_remove { bars.remove(h); }
        count
    }

    /// 清空所有进度条
    pub fn clear_all(&self) -> usize {
        let mut bars = self.bars.lock();
        let count = bars.len();
        bars.clear();
        count
    }

    /// 渲染所有活跃进度条
    pub fn render_active(&self) -> Vec<String> {
        let bars = self.active_bars();
        if bars.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        lines.push(format!("── 进度 ({}条) ──", bars.len()));
        for bar in &bars {
            lines.push(bar.render_line());
        }
        lines
    }
}

// ═══════════════════════════════════════════════════════
// 全局单例 (供 C ABI 和内部使用)
// ═══════════════════════════════════════════════════════

static GLOBAL_PROGRESS: std::sync::OnceLock<Arc<MultiProgressManager>> = std::sync::OnceLock::new();

pub fn set_global_progress(mgr: Arc<MultiProgressManager>) {
    let _ = GLOBAL_PROGRESS.set(mgr);
}

pub fn global_progress() -> Option<Arc<MultiProgressManager>> {
    GLOBAL_PROGRESS.get().cloned()
}

// ═══════════════════════════════════════════════════════
// C ABI 导出 — 供插件 DLL/SO 直接调用
//
// 插件使用方式:
//   // 获取函数指针 (通过 dlsym/GetProcAddress)
//   typedef char* (*ruoo_progress_create_t)(const char*, const char*, const char*, size_t);
//   typedef int (*ruoo_progress_update_t)(const char*, size_t, const char*);
//   typedef int (*ruoo_progress_finish_t)(const char*);
//
//   // 创建进度条
//   char* handle = ruoo_progress_create("myplug", "scan", "扫描端口", 100);
//   // 更新
//   ruoo_progress_update(handle, 50, "已完成 50/100");
//   // 完成
//   ruoo_progress_finish(handle);
//   // 释放 handle 字符串
//   ruoo_progress_free_handle(handle);
// ═══════════════════════════════════════════════════════

/// 创建进度条
/// 参数: plugin_name, bar_id, label, total
/// 返回: handle (C字符串, 调用方需用 ruoo_progress_free_handle 释放)
#[no_mangle]
pub unsafe extern "C" fn ruoo_progress_create(
    plugin_name: *const c_char,
    bar_id: *const c_char,
    label: *const c_char,
    total: usize,
) -> *mut c_char {
    if plugin_name.is_null() || bar_id.is_null() || label.is_null() {
        return std::ptr::null_mut();
    }

    let plugin = match CStr::from_ptr(plugin_name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let id = match CStr::from_ptr(bar_id).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let lbl = CStr::from_ptr(label).to_string_lossy();

    let mgr = match global_progress() {
        Some(m) => m,
        None => return std::ptr::null_mut(),
    };

    let handle = mgr.create(plugin, id, &lbl, total);
    CString::new(handle).map(|cs| cs.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// 更新进度
/// 参数: handle, current, message
/// 返回: 0=成功, -1=失败
#[no_mangle]
pub unsafe extern "C" fn ruoo_progress_update(
    handle: *const c_char,
    current: usize,
    message: *const c_char,
) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let h = match CStr::from_ptr(handle).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let msg = if message.is_null() {
        String::new()
    } else {
        CStr::from_ptr(message).to_string_lossy().to_string()
    };

    let mgr = match global_progress() {
        Some(m) => m,
        None => return -1,
    };

    match mgr.update(h, current, &msg) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 设置总量
#[no_mangle]
pub unsafe extern "C" fn ruoo_progress_set_total(
    handle: *const c_char,
    total: usize,
) -> i32 {
    if handle.is_null() { return -1; }

    let h = match CStr::from_ptr(handle).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let mgr = match global_progress() {
        Some(m) => m,
        None => return -1,
    };

    match mgr.set_total(h, total) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 完成进度条
#[no_mangle]
pub unsafe extern "C" fn ruoo_progress_finish(
    handle: *const c_char,
) -> i32 {
    if handle.is_null() { return -1; }

    let h = match CStr::from_ptr(handle).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let mgr = match global_progress() {
        Some(m) => m,
        None => return -1,
    };

    match mgr.finish(h) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 释放 ruoo_progress_create 返回的 handle 字符串
#[no_mangle]
pub unsafe extern "C" fn ruoo_progress_free_handle(handle: *mut c_char) {
    if !handle.is_null() {
        let _ = CString::from_raw(handle);
        // CString drop 释放内存
    }
}

// ═══════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_update_finish() {
        let mgr = MultiProgressManager::new();
        let h = mgr.create("testplug", "scan", "扫描", 100);
        assert!(h.contains("testplug::scan"));

        mgr.update(&h, 50, "一半").unwrap();
        let bars = mgr.active_bars();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].current, 50);
        assert_eq!(bars[0].percent(), 50.0);

        mgr.finish(&h).unwrap();
        let bars = mgr.active_bars();
        assert_eq!(bars[0].finished, true);
    }

    #[test]
    fn test_multi_plugin() {
        let mgr = MultiProgressManager::new();
        let h1 = mgr.create("p1", "a", "任务A", 10);
        let h2 = mgr.create("p2", "b", "任务B", 20);

        mgr.update(&h1, 5, "").unwrap();
        mgr.update(&h2, 10, "").unwrap();

        let all = mgr.all_bars();
        assert_eq!(all.len(), 2);

        let p1_bars = mgr.bars_by_plugin("p1");
        assert_eq!(p1_bars.len(), 1);
        assert_eq!(p1_bars[0].current, 5);

        mgr.clear_plugin("p1");
        assert_eq!(mgr.all_bars().len(), 1);
    }
}
