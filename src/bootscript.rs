// ============================================================
// RUOO-CONSOLE — 启动加载脚本 (Bootscript) v1.0
//
// 功能:
//   - 程序启动后自动执行 (Vault 挂载后)
//   - 脚本内容 AES-256-GCM 加密存储在 Vault 中
//   - TUI 编辑器: 全屏编辑 + 内存加密保护
//   - 命令: bootscript status/edit/reset/run/clear
//   - 安全: 禁止 AI/脚本/插件访问此命令
//
// 设计:
//   - 存储: Vault namespace "bootscript", key "script"
//   - 内存保护: EditorBuffer 实现 Zeroize on Drop
//   - 编辑器: 全屏接管，行号显示，Ctrl+S保存 Esc取消
// ============================================================

use zeroize::Zeroize;

// ─── Vault 存储命名空间 ───────────────────────────────
const BOOTSCRIPT_NS: &str = "bootscript";
const BOOTSCRIPT_KEY: &str = "script";

/// 默认启动脚本模板
pub fn default_bootscript() -> String {
    r#"# RUOO-CONSOLE 启动加载脚本
# 此脚本在程序启动并挂载 Vault 后自动执行
# 可用于: 显示欢迎信息、加载插件、设置变量、执行其他脚本等
#
# 命令: bootscript edit  → 修改此脚本
#       bootscript reset → 恢复默认
#       bootscript clear → 清除脚本(禁用自动执行)
#       bootscript run   → 手动执行
#       bootscript status → 查看状态

echo "=== RUOO-CONSOLE v4.2 ==="
echo "启动加载脚本已执行"
echo "输入 help 查看可用命令"

# ── 示例: 如果你想在启动时加载插件 ──
# #![allow-plugin]
# load myplug ./plugins/myplug.dll
# call myplug "init"

# ── 示例: 设置默认目标 ──
# set TARGET=192.168.1.0/24

# ── 示例: 显示 CPU/内存信息 ──
# sysinfo

echo "bootscript 执行完毕"
"#.to_string()
}

// ═══════════════════════════════════════════════════════
// Editor Buffer — 内存加密保护
// ═══════════════════════════════════════════════════════

/// 编辑器缓冲区 — Drop 时自动清零
pub struct EditorBuffer {
    pub lines: Vec<String>,
    pub file_path_hint: String,  // 显示用，非文件路径
    pub modified: bool,
}

impl EditorBuffer {
    pub fn new(content: &str) -> Self {
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(|s| s.to_string()).collect()
        };
        Self {
            lines,
            file_path_hint: "🔒 bootscript (Vault 加密存储)".into(),
            modified: false,
        }
    }

    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }

    /// 在光标位置插入字符
    pub fn insert_char(&mut self, row: usize, col: usize, c: char) {
        if row >= self.lines.len() { return; }
        let line = &mut self.lines[row];
        let byte_pos = line.char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        line.insert(byte_pos, c);
        self.modified = true;
    }

    /// 在光标位置删除字符 (Backspace)
    pub fn delete_backward(&mut self, row: usize, col: usize) -> Option<(usize, usize)> {
        if col == 0 {
            // 合并到上一行
            if row > 0 {
                let current = self.lines.remove(row);
                let prev_len = self.lines[row - 1].len();
                self.lines[row - 1].push_str(&current);
                self.modified = true;
                Some((row - 1, prev_len))
            } else {
                None
            }
        } else {
            let line = &mut self.lines[row];
            let byte_pos = line.char_indices()
                .nth(col - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            line.remove(byte_pos);
            self.modified = true;
            None
        }
    }

    /// 在光标位置删除字符 (Delete)
    pub fn delete_forward(&mut self, row: usize, col: usize) {
        if row >= self.lines.len() { return; }
        let line = &mut self.lines[row];
        if col < line.chars().count() {
            let byte_pos = line.char_indices()
                .nth(col)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            line.remove(byte_pos);
            self.modified = true;
        } else if row + 1 < self.lines.len() {
            // 合并下一行
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
            self.modified = true;
        }
    }

    /// 在光标位置插入换行
    pub fn insert_newline(&mut self, row: usize, col: usize) -> (usize, usize) {
        if row >= self.lines.len() { return (row, col); }
        let line = &mut self.lines[row];
        let byte_pos = line.char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let rest = line[byte_pos..].to_string();
        line.truncate(byte_pos);
        self.lines.insert(row + 1, rest);
        self.modified = true;
        (row + 1, 0)
    }
}

impl Drop for EditorBuffer {
    fn drop(&mut self) {
        // ★ 内存清零: 防止明文残留在进程内存中
        for line in &mut self.lines {
            line.zeroize();
        }
        self.lines.clear();
        self.file_path_hint.zeroize();
    }
}

// ═══════════════════════════════════════════════════════
// Vault 操作接口
// ═══════════════════════════════════════════════════════

/// 从 Vault 获取启动脚本内容
pub fn get_bootscript() -> Option<String> {
    crate::vault::vault_get(BOOTSCRIPT_NS, BOOTSCRIPT_KEY)
}

/// 将启动脚本存入 Vault (AES-256-GCM 加密)
pub fn set_bootscript(content: &str) -> Result<(), String> {
    crate::vault::vault_set(BOOTSCRIPT_NS, BOOTSCRIPT_KEY, content)
}

/// 删除启动脚本
pub fn delete_bootscript() -> Result<(), String> {
    crate::vault::vault_set(BOOTSCRIPT_NS, BOOTSCRIPT_KEY, "")
}

/// 重置为默认脚本
pub fn reset_bootscript() -> Result<(), String> {
    set_bootscript(&default_bootscript())
}

/// 检查启动脚本是否存在
pub fn bootscript_exists() -> bool {
    get_bootscript().map(|s| !s.is_empty()).unwrap_or(false)
}
