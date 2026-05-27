// ============================================================
// RUOO-ARSENAL v4.2 — 会话持久化
// 保存/恢复: 标签页状态、命令历史
// ============================================================

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 单个标签页的序列化状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSession {
    pub id: usize,
    pub name: String,
    pub target: String,
    pub ai_mode: bool,
}

/// 完整会话状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub tabs: Vec<TabSession>,
    pub active_tab_idx: usize,
    pub next_tab_id: usize,
}

impl SessionState {
    pub fn save_path() -> PathBuf {
        crate::config::data_dir().join("session.json")
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::save_path();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&path, &json)
            .map_err(|e| format!("写入失败: {}", e))
    }

    pub fn load() -> Option<Self> {
        let path = Self::save_path();
        let json = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&json).ok()
    }
}

/// 从当前 TabManager 构建会话状态
pub fn build_session(tab_mgr: &crate::tabs::TabManager) -> SessionState {
    let tabs: Vec<TabSession> = tab_mgr.tabs.iter().map(|tab| {
        TabSession {
            id: tab.id,
            name: tab.name.clone(),
            target: tab.target.clone(),
            ai_mode: tab.ai_mode,
        }
    }).collect();

    SessionState {
        tabs,
        active_tab_idx: tab_mgr.active_idx,
        next_tab_id: tab_mgr.tabs.iter().map(|t| t.id).max().map(|m| m + 1).unwrap_or(1),
    }
}

/// 从会话状态恢复 TabManager
pub fn restore_tabs(session: &SessionState) -> crate::tabs::TabManager {
    let mut mgr = crate::tabs::TabManager::new();

    // 清除默认标签，用保存的标签替换
    if !session.tabs.is_empty() {
        mgr.tabs.clear();
    }

    for ts in &session.tabs {
        let mut tab = crate::tabs::Tab::new(ts.id, &ts.name);
        tab.target = ts.target.clone();
        tab.ai_mode = ts.ai_mode;
        mgr.tabs.push(tab);
    }

    if session.active_tab_idx < mgr.tabs.len() {
        mgr.active_idx = session.active_tab_idx;
    }

    mgr.set_next_id(session.next_tab_id);

    mgr
}
