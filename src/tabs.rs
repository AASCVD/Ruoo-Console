// ============================================================
// RUOO-CONSOLE v4.2 — 多标签页系统
// 每个标签独立输出缓冲/滚动/目标/面板状态
// ============================================================

/// 单个标签页状态
#[derive(Clone)]
pub struct Tab {
    pub id: usize,
    pub name: String,
    pub output: Vec<String>,
    pub scroll_offset: usize,
    pub scroll_max: usize,
    pub auto_scroll: bool,
    pub target: String,
    pub ai_mode: bool,
}

impl Tab {
    pub fn new(id: usize, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            output: Vec::new(),
            scroll_offset: 0,
            scroll_max: 0,
            auto_scroll: true,
            target: String::from("—"),
            ai_mode: false,
        }
    }

    pub fn push_output(&mut self, msg: &str) {
        self.output.push(msg.to_string());
        const MAX_OUTPUT: usize = 500;
        if self.output.len() > MAX_OUTPUT {
            let overflow_count = self.output.len() - MAX_OUTPUT;
            let _overflow: Vec<String> = self.output.drain(0..overflow_count).collect();
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let filename = format!("ruoo_tab{}_overflow_{}.log", self.id, ts);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&filename) {
                use std::io::Write;
                for line in &_overflow {
                    let _ = writeln!(f, "{}", line);
                }
            }
            self.output.insert(0, format!("[s] Tab{} 输出达{}行上限, 旧行归档→{}", self.id, MAX_OUTPUT, filename));
        }
        if self.auto_scroll {
            self.reset_scroll();
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset
            .saturating_add(lines)
            .min(self.scroll_max);
        if self.scroll_offset >= self.scroll_max {
            self.auto_scroll = true;
        }
    }

    pub fn reset_scroll(&mut self) {
        self.auto_scroll = true;
    }

    pub fn clear(&mut self) {
        self.output.clear();
        self.scroll_offset = 0;
        self.scroll_max = 0;
        self.auto_scroll = true;
    }
}

/// 标签页管理器
pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_idx: usize,
    next_id: usize,
}

impl TabManager {
    pub fn new() -> Self {
        let tab = Tab::new(0, "终端");
        Self {
            tabs: vec![tab],
            active_idx: 0,
            next_id: 1,
        }
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active_idx]
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_idx]
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// 新建标签页
    pub fn new_tab(&mut self, name: Option<&str>) -> usize {
        let default_name = format!("Tab{}", self.next_id);
        let name = name.unwrap_or(&default_name);
        let tab = Tab::new(self.next_id, name);
        self.next_id += 1;
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_idx = idx;
        idx
    }

    /// 关闭当前标签页。返回是否还有标签存活
    pub fn close_current(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false; // 至少保留一个标签
        }
        self.tabs.remove(self.active_idx);
        if self.active_idx >= self.tabs.len() {
            self.active_idx = self.tabs.len() - 1;
        }
        true
    }

    /// 切换到下一个标签
    pub fn next_tab(&mut self) {
        self.active_idx = (self.active_idx + 1) % self.tabs.len();
    }

    /// 切换到上一个标签
    pub fn prev_tab(&mut self) {
        if self.active_idx == 0 {
            self.active_idx = self.tabs.len() - 1;
        } else {
            self.active_idx -= 1;
        }
    }

    /// 设置下一个标签ID (会话恢复用)
    pub fn set_next_id(&mut self, id: usize) {
        self.next_id = id;
    }

    /// 切换到指定索引标签
    pub fn switch_to(&mut self, idx: usize) -> bool {
        if idx < self.tabs.len() {
            self.active_idx = idx;
            true
        } else {
            false
        }
    }

    /// 同步 App 状态到当前活跃标签 (保存当前状态)
    pub fn save_active(&mut self, output: &[String], scroll_offset: usize, scroll_max: usize,
                       auto_scroll: bool, target: &str, ai_mode: bool) {
        let tab = &mut self.tabs[self.active_idx];
        tab.output = output.to_vec();
        tab.scroll_offset = scroll_offset;
        tab.scroll_max = scroll_max;
        tab.auto_scroll = auto_scroll;
        tab.target = target.to_string();
        tab.ai_mode = ai_mode;
    }

    /// 同步当前标签状态到 App (恢复标签状态) — 直接写入App字段
    pub fn restore_active_to(&self, output: &mut Vec<String>, scroll_offset: &mut usize,
                              scroll_max: &mut usize, auto_scroll: &mut bool,
                              target: &mut String, ai_mode: &mut bool) {
        let tab = &self.tabs[self.active_idx];
        *output = tab.output.clone();
        *scroll_offset = tab.scroll_offset;
        *scroll_max = tab.scroll_max;
        *auto_scroll = tab.auto_scroll;
        *target = tab.target.clone();
        *ai_mode = tab.ai_mode;
    }
}

/// 渲染标签栏 (返回 Line 列表供 UI 使用)
pub fn render_tab_bar(tabs: &TabManager) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };
    use crate::theme;

    let mut spans: Vec<Span> = Vec::new();

    for (i, tab) in tabs.tabs.iter().enumerate() {
        let is_active = i == tabs.active_idx;

        let style = if is_active {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(theme::MUTED)
        };

        let prefix = if is_active { "▐ " } else { "  " };
        let suffix = if is_active { " ▌" } else { "  " };

        spans.push(Span::styled(
            format!("{}{}{}", prefix, tab.name, suffix),
            style,
        ));

        // 分隔符
        if i + 1 < tabs.tabs.len() {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::MUTED)));
        }
    }

    // 右侧信息：标签计数
    let count_text = format!(" [{}个标签] ", tabs.tab_count());
    // 填充空白到终端宽度
    let bar_width: usize = spans.iter().map(|s| s.content.len()).sum();
    let fill = if bar_width < 60 {
        " ".repeat(60usize.saturating_sub(bar_width))
    } else {
        String::from("  ")
    };

    spans.push(Span::styled(
        format!("{}{}", fill, count_text),
        Style::default().fg(theme::MUTED),
    ));

    vec![Line::from(spans)]
}
