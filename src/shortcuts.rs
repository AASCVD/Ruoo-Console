// ============================================================
// RUOO-ARSENAL v4.2 — 键盘快捷键系统
// Ctrl+R 历史搜索 | Ctrl+K/U/W 行编辑 | Alt+←→ 词跳转
// ============================================================

// shortcuts.rs — 仅使用 String 和 Vec

/// 历史搜索状态
#[derive(Clone)]
pub struct HistorySearch {
    pub active: bool,
    pub query: String,
    pub matches: Vec<usize>,    // 匹配项在 cmd_history 中的索引 (从新到旧)
    pub current_idx: usize,     // 当前选中匹配项在 matches 中的索引
}

impl HistorySearch {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            matches: Vec::new(),
            current_idx: 0,
        }
    }

    pub fn activate(input: &str, cmd_history: &[String]) -> Self {
        let mut hs = Self::new();
        hs.active = true;
        hs.query = input.to_string();
        hs.search(cmd_history);
        hs
    }

    /// 搜索匹配项
    pub fn search(&mut self, cmd_history: &[String]) {
        self.matches.clear();
        self.current_idx = 0;

        let query_lower = self.query.to_lowercase();
        // 从新到旧遍历
        for (i, cmd) in cmd_history.iter().enumerate().rev() {
            if query_lower.is_empty() || cmd.to_lowercase().contains(&query_lower) {
                self.matches.push(i);
            }
        }
    }

    /// 获取当前匹配的命令文本
    pub fn current_match<'a>(&self, cmd_history: &'a [String]) -> Option<&'a String> {
        self.matches.get(self.current_idx)
            .map(|&idx| &cmd_history[idx])
    }

    /// 下一个匹配 (更旧的)
    pub fn next_match(&mut self) {
        if self.current_idx + 1 < self.matches.len() {
            self.current_idx += 1;
        } else {
            self.current_idx = 0; // 循环
        }
    }

    /// 上一个匹配 (更新的)
    pub fn prev_match(&mut self) {
        if self.current_idx > 0 {
            self.current_idx -= 1;
        } else if !self.matches.is_empty() {
            self.current_idx = self.matches.len() - 1;
        }
    }

    /// 接受当前匹配并退出搜索
    pub fn accept(&self, cmd_history: &[String], input: &mut String, cursor: &mut usize) {
        if let Some(cmd) = self.current_match(cmd_history) {
            *input = cmd.clone();
            *cursor = input.len();
        }
    }
}

/// 删除光标左边的单词
pub fn delete_word_backward(input: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let chars: Vec<char> = input.chars().collect();
    let mut pos = *cursor;

    // 跳过空白
    while pos > 0 && chars[pos - 1].is_whitespace() {
        pos -= 1;
    }
    // 跳过单词字符
    while pos > 0 && !chars[pos - 1].is_whitespace() {
        pos -= 1;
    }

    let remove_start = input
        .char_indices()
        .nth(pos)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let remove_end = input
        .char_indices()
        .nth(*cursor)
        .map(|(i, _)| i)
        .unwrap_or(input.len());

    input.replace_range(remove_start..remove_end, "");
    *cursor = pos;
}

/// 删除光标到行尾
pub fn delete_to_end(input: &mut String, cursor: usize) {
    if cursor < input.len() {
        let truncate_pos = input
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        input.truncate(truncate_pos);
    }
}

/// 删除光标到行首
pub fn delete_to_start(input: &mut String, cursor: &mut usize) {
    if *cursor > 0 {
        let remove_end = input
            .char_indices()
            .nth(*cursor)
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        input.replace_range(0..remove_end, "");
        *cursor = 0;
    }
}

/// 光标跳一个单词 (向前)
pub fn word_forward(input: &str, cursor: &mut usize) {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut pos = *cursor;

    // 跳过当前单词剩余字符
    while pos < len && !chars[pos].is_whitespace() {
        pos += 1;
    }
    // 跳过空白
    while pos < len && chars[pos].is_whitespace() {
        pos += 1;
    }

    *cursor = pos;
}

/// 光标跳一个单词 (向后)
pub fn word_backward(input: &str, cursor: &mut usize) {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = *cursor;

    if pos == 0 {
        return;
    }

    // 跳过空白
    while pos > 0 && chars[pos - 1].is_whitespace() {
        pos -= 1;
    }
    // 跳过单词字符
    while pos > 0 && !chars[pos - 1].is_whitespace() {
        pos -= 1;
    }

    *cursor = pos;
}
