// ============================================================
// RUOO-CONSOLE — UI 渲染 (v4.1.0 — 软件光标闪烁)
// ============================================================

use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use unicode_width::UnicodeWidthStr;

use crate::theme;
use crate::App;
// Panel removed — unused import
use crate::MatrixColumn;
// telemetry types removed — right sidebar removed in v12.0

/// 计算字符串的终端显示列宽（CJK=2列, ASCII=1列）
fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 解析 [cmd] 后缀: "cmd_name  description" → Vec<Span>
/// 无描述时整个作为命令名着色
fn parse_cmd_spans(rest: &str) -> Vec<Span<'static>> {
    if let Some(split_pos) = rest.find("  ") {
        let cmd_part = rest[..split_pos].trim_end();
        let mut desc_part = rest[split_pos..].trim_start();
        if desc_part.starts_with("[desc]") {
            desc_part = desc_part[6..].trim_start();
        }
        vec![
            Span::styled(cmd_part.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("  ".to_string(), Style::default()),
            Span::styled(desc_part.to_string(), Style::default().fg(theme::WHITE)),
        ]
    } else {
        vec![
            Span::styled(rest.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
        ]
    }
}

// ── 输出区渲染 ──
pub fn render_output(frame: &mut Frame, app: &mut App, rect: Rect) {
    // 使用 Paragraph::scroll 原生滚动 + line_count 测真实视觉行数
    // scroll_offset 以视觉行为单位, offset=0=顶部, offset=max_scroll=底部

    let styled_lines: Vec<Line> = app.output.iter().flat_map(|line| {
        let style = {
            // 提取缩进后的内容用于匹配（AI 输出普遍有 2 空格缩进）
            let is_indented = line.starts_with("  ") && line.len() > 2;
            let content = if is_indented { &line[2..] } else { line.as_str() };

            // ── v4.6: [cmd]前缀 → 命令名着色 + 描述着色 ──
            // ★ 先 trim 所有前导空白，处理 2/4/6 空格缩进（父/子树）
            let content_trimmed = content.trim_start();
            // ── v8.0: [cat]前缀 → 分类头着色 (CYBER+BOLD) ──
            if content_trimmed.starts_with("[cat]") {
                let rest = &content_trimmed[5..];
                let leading = &line[..line.len() - content.len()];
                return vec![Line::from(vec![
                    Span::styled(leading.to_string(), Style::default()),
                    Span::styled(rest.to_string(), Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
                ])];
            }

            if content_trimmed.starts_with("[cmd]") {
                // ── v4.7: 双列 [cmd] │ [cmd] 格式检测 ──
                if let Some(sep_pos) = content_trimmed.find("│ [cmd]") {
                    let leading = &line[..line.len() - content.len()];
                    let left_raw = content_trimmed[..sep_pos].trim_end();
                    let right_raw = content_trimmed[sep_pos + 3..].trim_start(); // skip │ (3-byte UTF-8)

                    // 解析左侧 [cmd]
                    let left_spans = parse_cmd_spans(&left_raw[5..]); // strip "[cmd]"
                    // 解析右侧 [cmd]
                    let right_spans = parse_cmd_spans(&right_raw[5..]);

                    let mut all_spans = vec![Span::styled(leading.to_string(), Style::default())];
                    all_spans.extend(left_spans);
                    all_spans.push(Span::styled(" │ ".to_string(), Style::default().fg(theme::MUTED)));
                    all_spans.extend(right_spans);
                    return vec![Line::from(all_spans)];
                }

                let leading = &line[..line.len() - content.len()]; // 原始缩进
                let rest = &content_trimmed[5..];
                // 按≥2空格分割命令名和描述
                if let Some(split_pos) = rest.find("  ") {
                    let cmd_part = rest[..split_pos].trim_end();
                    let mut desc_part = rest[split_pos..].trim_start();
                    // ★ 剥离 [desc] 前缀
                    if desc_part.starts_with("[desc]") {
                        desc_part = desc_part[6..].trim_start();
                    }
                    return vec![Line::from(vec![
                        Span::styled(leading.to_string(), Style::default()),
                        Span::styled(cmd_part.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                        Span::styled("  ", Style::default()),
                        Span::styled(desc_part.to_string(), Style::default().fg(theme::WHITE)),
                    ])];
                } else {
                    // 无描述 → 整个作为命令名
                    return vec![Line::from(vec![
                        Span::styled(leading.to_string(), Style::default()),
                        Span::styled(rest.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                    ])];
                }
            }

            // ── 先处理带缩进的框线/前缀（"  ╔", "  [+]", "  ║  [*]" 等）──
            if is_indented {
                if content.starts_with("[+]") {
                    Style::default().fg(theme::ACCENT)
                } else if content.starts_with("[*]") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("[!]") {
                    Style::default().fg(theme::DANGER)
                } else if content.starts_with("[·]") {
                    Style::default().fg(theme::MUTED)
                } else if content.starts_with("[$]") {
                    Style::default().fg(theme::WARN)
                } else if content.starts_with("[s]") {
                    Style::default().fg(theme::MUTED)
                } else if content.starts_with("[SHD]") {
                    Style::default().fg(theme::WARN)
                } else if content.starts_with("[>>]") {
                    Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)
                } else if content.starts_with("[OK]") {
                    Style::default().fg(theme::ACCENT)
                } else if content.starts_with("[!!]") {
                    Style::default().fg(theme::DANGER)
                } else if content.starts_with("[cat]") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("[API]") {
                    Style::default().fg(theme::DP_BLUE).add_modifier(Modifier::BOLD)
                } else if content.starts_with("[>>]") {
                    Style::default().fg(theme::CYBER)
                } else if content.starts_with("[<<]") {
                    Style::default().fg(theme::DIM_GREEN)
                // 单线框
                } else if content.starts_with("┌─") || content.starts_with("└─") || content.starts_with("├─") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("│  [+]") {
                    Style::default().fg(theme::ACCENT)
                } else if content.starts_with("│  [!]") {
                    Style::default().fg(theme::DANGER)
                } else if content.starts_with("│  [*]") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("│  [·]") {
                    Style::default().fg(theme::MUTED)
                } else if content.starts_with("│") {
                    Style::default().fg(theme::DIM_GREEN)
                // 双线框
                } else if content.starts_with("╔") || content.starts_with("╚") || content.starts_with("╠") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("║  [+]") {
                    Style::default().fg(theme::ACCENT)
                } else if content.starts_with("║  [!]") {
                    Style::default().fg(theme::DANGER)
                } else if content.starts_with("║  [*]") {
                    Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
                } else if content.starts_with("║  [·]") {
                    Style::default().fg(theme::MUTED)
                } else if content.starts_with("║") {
                    Style::default().fg(theme::DIM_GREEN)
                // 缩进分隔线 / 缩进续行
                } else if content.starts_with("──") || content.starts_with("─") {
                    Style::default().fg(theme::MUTED)
                } else {
                    Style::default().fg(theme::DIM_GREEN)
                }
            }
            // ── 无缩进（工具输出、命令结果等）──
            else if line.starts_with("[+]") {
                Style::default().fg(theme::ACCENT)
            } else if line.starts_with("[*]") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("[!]") {
                Style::default().fg(theme::DANGER)
            } else if line.starts_with("[$]") {
                Style::default().fg(theme::WARN)
            } else if line.starts_with("[cat]") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("[API]") {
                Style::default().fg(theme::DP_BLUE).add_modifier(Modifier::BOLD)
            } else if line.starts_with("[>>]") {
                Style::default().fg(theme::CYBER)
            } else if line.starts_with("[<<]") {
                Style::default().fg(theme::DIM_GREEN)
            } else if line.starts_with("[OK]") {
                Style::default().fg(theme::ACCENT)
            } else if line.starts_with("[!!]") {
                Style::default().fg(theme::DANGER)
            } else if line.starts_with("[🔄]") {
                Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)
            } else if line.starts_with("[s]") {
                Style::default().fg(theme::MUTED)
            } else if line.starts_with("[🛡]") {
                Style::default().fg(theme::WARN)
            } else if line.starts_with("[·]") {
                Style::default().fg(theme::MUTED)
            } else if line.starts_with("┌─") || line.starts_with("└─") || line.starts_with("├─") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("│  [+]") {
                Style::default().fg(theme::ACCENT)
            } else if line.starts_with("│  [!]") {
                Style::default().fg(theme::DANGER)
            } else if line.starts_with("│  [*]") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("│  [·]") {
                Style::default().fg(theme::MUTED)
            } else if line.starts_with("│") {
                Style::default().fg(theme::DIM_GREEN)
            } else if line.starts_with("╔") || line.starts_with("╚") || line.starts_with("╠") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("║  [+]") {
                Style::default().fg(theme::ACCENT)
            } else if line.starts_with("║  [!]") {
                Style::default().fg(theme::DANGER)
            } else if line.starts_with("║  [*]") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if line.starts_with("║  [·]") {
                Style::default().fg(theme::MUTED)
            } else if line.starts_with("║") {
                Style::default().fg(theme::DIM_GREEN)
            } else if line.contains("══") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::WHITE)
            }
        };

        // v4.9: 长行由Paragraph自动换行 — 不截断, Paragraph内部wrap, 输入框位置不受影响
        return vec![Line::styled(line.to_string(), style)];
    }).collect();

    // v4.9: Paragraph内部自动换行, line_count计算真实视觉行数用于滚动
    // Borders::TOP 只有顶部边框线, 无左右边框, 内容宽度 = rect.width
    let para_width = rect.width;
    let paragraph = Paragraph::new(styled_lines)
        .wrap(Wrap { trim: true })
        .block(Block::new().borders(Borders::TOP)
            .border_style(Style::default().fg(theme::MUTED))
            .title(Span::styled(" 终端输出 ", Style::default().fg(theme::CYBER))));

    let total_visual = paragraph.line_count(para_width);
    let visible_rows = rect.height.saturating_sub(1) as usize;
    let max_scroll = total_visual.saturating_sub(visible_rows);

    // ★ v13.1: 自动滚屏核心
    //    auto_scroll=true  → 强制追底, 每帧刷新 scroll_offset=max_scroll
    //    auto_scroll=false → 保留用户阅读位置, 仅钳位防止越界
    // ★ BUG-FIX: 防止 scroll_max 超过 u16::MAX 导致 ratatui scroll() panic
    app.scroll_max = max_scroll.min(u16::MAX as usize);
    if app.auto_scroll {
        app.scroll_offset = app.scroll_max;
    } else if app.scroll_offset > app.scroll_max {
        app.scroll_offset = app.scroll_max;
    }
    let scroll_y = app.scroll_offset.min(u16::MAX as usize) as u16;
    frame.render_widget(
        paragraph.scroll((scroll_y, 0)),
        rect,
    );
}

// ── 主 UI 布局 ──
pub fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // ── 编辑器模式全屏 ──
    if app.editor_mode {
        render_editor(frame, app, area);
        return;
    }

    // ── 密码模式全屏登录界面 (v4.4 矩阵特效) ──
    if app.password_mode {
        render_password_screen(frame, app, area);
        return;
    }

    // ── v5.0: 帮助页面模式 (无输入框, 全屏帮助) ──
    if app.help_mode {
        render_help_page(frame, app, area);
        return;
    }

    // v4.7: 多标签页始终显示
    let tab_count = app.tab_mgr.tab_count();

    let has_tabs = tab_count > 1;
    let constraints: Vec<Constraint> = if has_tabs {
        vec![
            Constraint::Length(1),  // 标题栏
            Constraint::Length(1),  // Tab 栏
            Constraint::Min(0),     // 主体
            Constraint::Length(1),  // 输入区
            Constraint::Length(1),  // 状态栏
            Constraint::Length(1),  // 快捷键
        ]
    } else {
        vec![
            Constraint::Length(1),  // 标题栏
            Constraint::Min(0),     // 主体
            Constraint::Length(1),  // 输入区
            Constraint::Length(1),  // 状态栏
            Constraint::Length(1),  // 快捷键
        ]
    };

    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0;

    // ── 标题栏 ──
    let now = Local::now();
    let panel_tag = String::new();
    let ai_tag = if app.ai_mode {
        if app.is_admin {
            Span::styled(" | root#dp", Style::default()
                .fg(theme::DANGER).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" | #dp", Style::default().fg(theme::DP_BLUE).add_modifier(Modifier::BOLD))
        }
    } else if app.is_admin {
        Span::styled(" | root", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("", Style::default())
    };
    let title = Line::from(vec![
        Span::styled("RUOO-CONSOLE", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(theme::MUTED)),
        Span::styled(" |", Style::default().fg(theme::MUTED)),
        Span::styled(panel_tag, Style::default().fg(theme::ACCENT)),
        Span::styled("|", Style::default().fg(theme::MUTED)),
        Span::styled(now.format("%H:%M:%S").to_string(), Style::default().fg(Color::Gray)),
        ai_tag,
    ]);
    frame.render_widget(Paragraph::new(title), main[idx]);
    idx += 1;

    // ── Tab 栏 (v4.7: 多标签页系统) ──
    if has_tabs {
        let tab_lines = crate::tabs::render_tab_bar(&app.tab_mgr);

        let spans: Vec<Span> = tab_lines.into_iter()
            .flat_map(|l| l.spans)
            .collect();

        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::default().bg(Color::Rgb(10, 12, 18))),
            main[idx],
        );
        idx += 1;
    }

    // ── 主体 ──
    if app.show_right_sidebar {
        let h_body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(32)])
            .split(main[idx]);
        render_output(frame, app, h_body[0]);
        render_right_sidebar(frame, app, h_body[1]);
    } else {
        render_output(frame, app, main[idx]);
    }
    idx += 1;

    // ── 输入区 ──
    render_input(frame, app, main[idx]);
    idx += 1;

    // ── 状态栏 / 进度条 ──
    if let Some(ref progress_arc) = app.progress {
        // 读取进度状态 (非阻塞)
        if let Ok(p) = progress_arc.try_lock() {
            let pct = if p.total > 0 {
                (p.current as f64 / p.total as f64 * 100.0).min(100.0) as u32
            } else { 0 };
            let bar_width = (main[idx].width as usize).saturating_sub(18);
            let filled = (bar_width as f64 * pct as f64 / 100.0) as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
            let status = Line::from(vec![
                Span::styled(
                    format!("{} [{}/{}] {}% {} {}", p.label, p.current, p.total, pct, bar, p.message),
                    Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                ),
            ]);
            frame.render_widget(Paragraph::new(status), main[idx]);
        } else {
            // 锁被占用, 显示静态等待
            let status = Line::from(vec![
                Span::styled("[...] Processing...", Style::default().fg(theme::WARN)),
            ]);
            frame.render_widget(Paragraph::new(status), main[idx]);
        }
    } else if app.password_mode {
        // 密码模式下状态栏提示
        let status = Line::from(vec![
            Span::styled("[LOCK] 密码输入中", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD)),
            Span::styled(" | 密码不会回显 | ", Style::default().fg(theme::MUTED)),
            Span::styled(if app.password_action.starts_with("login") { "Esc 清空" } else { "Esc 取消" }, Style::default().fg(theme::WARN)),
            Span::styled(" | ", Style::default().fg(theme::MUTED)),
            Span::styled("Enter 确认", Style::default().fg(theme::ACCENT)),
        ]);
        frame.render_widget(Paragraph::new(status), main[idx]);
    } else {
        let out_total = app.output.len();
        let at_bottom = app.auto_scroll;
        let scroll_info = if at_bottom {
            format!(" 行:{} v{} ↓底", out_total, env!("CARGO_PKG_VERSION"))
        } else {
            format!(" 行:{} v{} ↑浏览", out_total, env!("CARGO_PKG_VERSION"))
        };
        let ai_status = if app.ai_pending {
            // 心跳动画 + 工具指示
            let spinner = ["-", "\\", "|", "/"];
            let frame = spinner[(app.tick_count as usize / 2) % spinner.len()];
            let tool_info = match &app.ai_active_tool {
                Some(arc) => {
                    match arc.lock() {
                        Ok(guard) => {
                            match guard.as_ref() {
                                Some(tool) => format!(" {}", tool),
                                None => String::new(),
                            }
                        }
                        Err(_) => String::new(),
                    }
                }
                None => String::new(),
            };
            Span::styled(
                format!("|{} AI思考中…{}", frame, tool_info),
                Style::default().fg(theme::DP_BLUE).add_modifier(Modifier::BOLD),
            )
        } else if app.ai_mode {
            if app.is_admin {
                Span::styled("|root#dp活跃", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("|#dp活跃", Style::default().fg(theme::DP_BLUE))
            }
        } else {
            Span::styled("", Style::default())
        };
        let status = Line::from(vec![
            Span::styled("ARSENAL", Style::default().fg(theme::MUTED)),
            Span::styled("|Zero-Day", Style::default().fg(theme::DANGER)),
            Span::styled(format!("|行:{}", app.output.len()), Style::default().fg(theme::MUTED)),
            Span::styled(scroll_info, Style::default().fg(theme::WARN)),
            ai_status,
        ]);
        frame.render_widget(Paragraph::new(status), main[idx]);
    }
    idx += 1;

    // ── 快捷键提示 (v5.0 更新: 添加 F9帮助) ──
    let shortcuts = if app.help_mode {
        let (mode_label, mode_style) = if app.help_select_mode {
            ("选择模式", Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD))
        } else {
            ("复制模式", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
        };
        Line::from(vec![
            Span::styled("F9/Esc退出帮助", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD)),
            Span::styled(" PageUp/Down翻页", Style::default().fg(theme::MUTED)),
            Span::styled(" Home/End首尾", Style::default().fg(theme::MUTED)),
            Span::styled(" Tab切换", Style::default().fg(theme::CYBER)),
            Span::styled(mode_label, mode_style),
            Span::styled(" | F8复制全部", Style::default().fg(theme::ACCENT)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Ctrl+T新标签", Style::default().fg(theme::MUTED)),
            Span::styled(" Ctrl+W关标签", Style::default().fg(theme::MUTED)),
            Span::styled(" Ctrl+Tab切换", Style::default().fg(theme::MUTED)),
            Span::styled(" Ctrl+R搜索", Style::default().fg(theme::WARN)),
            Span::styled(" Alt+C/V剪贴", Style::default().fg(theme::MUTED)),
            Span::styled(" F7鼠标", Style::default().fg(theme::MUTED)),
            Span::styled(" F8复制全部", Style::default().fg(theme::ACCENT)),
            Span::styled(" F9帮助", Style::default().fg(theme::CYBER)),
            Span::styled(" F10右栏", Style::default().fg(theme::CYBER)),
            Span::styled(" Esc退出", Style::default().fg(theme::DANGER)),
        ])
    };
    frame.render_widget(Paragraph::new(shortcuts), main[idx]);
}

// ── v5.0: 帮助页面渲染 (F9 / help命令) ──
fn render_help_page(frame: &mut Frame, app: &mut App, area: Rect) {
    let help_lines = &app.help_page_content;

    // 布局: 标题 + 主体 + 状态栏 + 快捷键 (无输入框!)
    let constraints = vec![
        Constraint::Length(1),  // 标题栏
        Constraint::Min(0),     // 主体 (帮助内容)
        Constraint::Length(1),  // 状态栏
        Constraint::Length(1),  // 快捷键
    ];
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // ── 标题栏 ──
    let title = Line::from(vec![
        Span::styled("RUOO-CONSOLE", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(theme::MUTED)),
        Span::styled(" | ", Style::default().fg(theme::MUTED)),
        Span::styled("帮助页面", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" | ", Style::default().fg(theme::MUTED)),
        Span::styled(format!("{} 行命令参考", help_lines.len()), Style::default().fg(theme::WARN)),
        Span::styled(" | ", Style::default().fg(theme::MUTED)),
        Span::styled("F9/Esc 退出", Style::default().fg(theme::DANGER)),
    ]);
    frame.render_widget(Paragraph::new(title), main[0]);

    // ── 主体: 帮助内容 (与render_output相同方式渲染) ──
    let styled_lines: Vec<Line> = help_lines.iter().flat_map(|line| {
        let style = {
            let is_indented = line.starts_with("  ") && line.len() > 2;
            let content = if is_indented { &line[2..] } else { line.as_str() };
            let content_trimmed = content.trim_start();
            // ── v8.0: [cat]前缀 → 分类头着色 (CYBER+BOLD) ──
            if content_trimmed.starts_with("[cat]") {
                let rest = &content_trimmed[5..];
                let leading = &line[..line.len() - content.len()];
                return vec![Line::from(vec![
                    Span::styled(leading.to_string(), Style::default()),
                    Span::styled(rest.to_string(), Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
                ])];
            }

            if content_trimmed.starts_with("[cmd]") {
                if let Some(sep_pos) = content_trimmed.find("│ [cmd]") {
                    let leading = &line[..line.len() - content.len()];
                    let left_raw = content_trimmed[..sep_pos].trim_end();
                    let right_raw = content_trimmed[sep_pos + 3..].trim_start();
                    let left_spans = parse_cmd_spans(&left_raw[5..]);
                    let right_spans = parse_cmd_spans(&right_raw[5..]);
                    let mut all_spans = vec![Span::styled(leading.to_string(), Style::default())];
                    all_spans.extend(left_spans);
                    all_spans.push(Span::styled(" │ ".to_string(), Style::default().fg(theme::MUTED)));
                    all_spans.extend(right_spans);
                    return vec![Line::from(all_spans)];
                }
                let leading = &line[..line.len() - content.len()];
                let rest = &content_trimmed[5..];
                if let Some(split_pos) = rest.find("  ") {
                    let cmd_part = rest[..split_pos].trim_end();
                    let mut desc_part = rest[split_pos..].trim_start();
                    if desc_part.starts_with("[desc]") {
                        desc_part = desc_part[6..].trim_start();
                    }
                    return vec![Line::from(vec![
                        Span::styled(leading.to_string(), Style::default()),
                        Span::styled(cmd_part.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                        Span::styled("  ", Style::default()),
                        Span::styled(desc_part.to_string(), Style::default().fg(theme::WHITE)),
                    ])];
                } else {
                    return vec![Line::from(vec![
                        Span::styled(leading.to_string(), Style::default()),
                        Span::styled(rest.to_string(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                    ])];
                }
            }
            if content.contains("══") && content.contains("(") && content.contains(")") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if content.starts_with("[+]") {
                Style::default().fg(theme::ACCENT)
            } else if content.starts_with("[*]") {
                Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)
            } else if content.starts_with("[!]") {
                Style::default().fg(theme::DANGER)
            } else if content.starts_with("[·]") {
                Style::default().fg(theme::MUTED)
            } else if content.starts_with("──") || content.starts_with("─") {
                Style::default().fg(theme::MUTED)
            } else if is_indented {
                Style::default().fg(theme::DIM_GREEN)
            } else {
                Style::default().fg(theme::WHITE)
            }
        };
        vec![Line::styled(line.to_string(), style)]
    }).collect();

    let para_width = main[1].width;
    let paragraph = Paragraph::new(styled_lines)
        .wrap(Wrap { trim: true })
        .block(Block::new().borders(Borders::TOP)
            .border_style(Style::default().fg(theme::CYBER))
            .title(Span::styled(" 命令参考 ", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));

    let total_visual = paragraph.line_count(para_width);
    let visible_rows = main[1].height.saturating_sub(1) as usize;
    // ★ BUG-FIX: max_scroll = total_visual - visible_rows (与render_output一致)
    //    +5安全边距补偿line_count可能的少算 (CJK/样式Span使实际渲染行数略多)
    let max_scroll = total_visual.saturating_sub(visible_rows).saturating_add(5);

    // ★ BUG-FIX: 防止 scroll_max 超过 u16::MAX 导致 ratatui scroll() panic
    app.scroll_max = max_scroll.min(u16::MAX as usize);
    if app.auto_scroll {
        app.scroll_offset = app.scroll_max;
    } else if app.scroll_offset > app.scroll_max {
        app.scroll_offset = app.scroll_max;
    }

    let scroll_y = app.scroll_offset.min(u16::MAX as usize) as u16;
    frame.render_widget(paragraph.scroll((scroll_y, 0)), main[1]);

    // ── 状态栏 ──
    let (mode_text, mode_color) = if app.help_select_mode {
        ("选择", theme::WARN)
    } else {
        ("复制", theme::ACCENT)
    };
    // 当前可视行: 视口顶部对应的视觉行号 (1-based)
    let scroll_pos = if total_visual > 0 {
        (app.scroll_offset + 1).min(total_visual)
    } else {
        0
    };
    let at_bottom = app.auto_scroll;
    let scroll_tag = if at_bottom {
        format!("视行:{}/{} ↓底", scroll_pos, total_visual)
    } else {
        format!("视行:{}/{} ↑浏览", scroll_pos, total_visual)
    };
    let status = Line::from(vec![
        Span::styled("帮助页面", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" | 逻辑{}行", help_lines.len()), Style::default().fg(theme::MUTED)),
        Span::styled(format!(" | {}", scroll_tag), Style::default().fg(theme::WARN)),
        Span::styled(format!(" | Tab:{}模式", mode_text), Style::default().fg(mode_color)),
        Span::styled(" | F9/Esc退出", Style::default().fg(theme::DANGER)),
        Span::styled(" | F8复制全部", Style::default().fg(theme::ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(status), main[2]);

    // ── 快捷键已在父函数ui()中渲染到main[3] ──
    // 此处main[3]是快捷键栏, 由ui()中统一渲染
    // 因为help_mode是在ui()中提前return的, 我们在这里直接渲染快捷键
    let (mode_label, mode_style) = if app.help_select_mode {
        (" 选择 ", Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD))
    } else {
        (" 复制 ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
    };
    let shortcuts = Line::from(vec![
        Span::styled("F9/Esc 退出帮助", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD)),
        Span::styled(" PageUp/Down 翻页", Style::default().fg(theme::MUTED)),
        Span::styled(" Home/End 首尾", Style::default().fg(theme::MUTED)),
        Span::styled(" Tab切换", Style::default().fg(theme::CYBER)),
        Span::styled(mode_label, mode_style),
        Span::styled(" F8 复制全部", Style::default().fg(theme::ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(shortcuts), main[3]);
}

// ── 密码登录屏幕 (v4.5 增强数字矩阵方格特效 + 动态启动信息) ──
fn render_password_screen(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::widgets::Clear;

    let w = area.width as usize;
    let h = area.height as usize;

    // ── v4.6: 矩阵雨特效重写 — 持久列状态, 连续下落动画 ──
    let tick = app.tick_count;
    let cell_w: usize = 1;  // 单字符宽, 更密集
    let grid_cols = w / cell_w;
    let h_isize = h as isize;

    // 字符集 — 纯单宽字符 (修复双宽字符导致代码雨错位)
    const CHARS: [&str; 72] = [
        "0","1","2","3","4","5","6","7","8","9",
        "A","B","C","D","E","F","G","H","I","J",
        "K","L","M","N","O","P","Q","R","S","T",
        "U","V","W","X","Y","Z",
        "α","β","γ","δ","λ","μ","π","Σ","Ω","φ","θ","ξ","ε",
        "a","b","c","d","e","f","g",
        "+","-","*","/","=","<",">","~","|",
        "!","@","#","$","%","&","?",
    ];

    // 颜色梯度 (头→尾 逐渐变暗)
    let c_head  = Style::default().fg(Color::Rgb(200, 255, 200)).bg(theme::LOGIN_BG);
    let c_t1    = Style::default().fg(Color::Rgb(100, 255, 100)).bg(theme::LOGIN_BG);
    let c_t2    = Style::default().fg(Color::Rgb(0, 220, 40)).bg(theme::LOGIN_BG);
    let c_t3    = Style::default().fg(Color::Rgb(0, 160, 20)).bg(theme::LOGIN_BG);
    let c_t4    = Style::default().fg(Color::Rgb(0, 100, 10)).bg(theme::LOGIN_BG);
    let c_t5    = Style::default().fg(Color::Rgb(0, 50, 5)).bg(theme::LOGIN_BG);
    let c_t6    = Style::default().fg(Color::Rgb(0, 25, 3)).bg(theme::LOGIN_BG);
    let c_empty = Style::default().bg(theme::LOGIN_BG);

    // 首次初始化或尺寸变化时重建列状态
    if !app.password_matrix_cols_init
        || app.password_matrix_cols.len() != grid_cols
        || app.password_matrix_cache.len() != h
    {
        app.password_matrix_cols_init = true;
        app.password_matrix_cols.clear();
        app.password_matrix_cols.reserve(grid_cols);

        let seed = app.password_matrix_seed;
        for gcol in 0..grid_cols {
            // 每列独立的伪随机初始状态
            let s = seed.wrapping_add(gcol as u64).wrapping_mul(0x9E3779B97F4A7C15);
            let cs = {
                let mut x = s;
                x ^= x << 13; x ^= x >> 7; x ^= x << 17; x
            };
            let active = (cs & 0xFF) < 160; // ~63%列有雨滴
            let head_y = if active {
                (cs >> 8) as isize % h_isize
            } else {
                -(cs as isize % 60 + 5) // 负值=未入场, 等spawn_delay后从顶部开始
            };
            let speed_den = 2 + (cs >> 16) as isize % 3; // 分母2-4 (原6-9, ~3x加速)
            let speed_num = 1;
            let len = 5 + (cs >> 20) as usize % 13; // 长度5-17 (原4-13)
            let char_idx = (cs >> 24) as usize % CHARS.len();
            let mut trail_char: [usize; 6] = [0; 6];
            for d in 0..6 {
                trail_char[d] = (char_idx.wrapping_add(d * 3 + 7)) % CHARS.len();
            }
            let spawn_delay = if active { 0 } else { (cs >> 32) as u64 % 15 + 2 };

            app.password_matrix_cols.push(MatrixColumn {
                head_y, speed_num, speed_den, len, char_idx, trail_char,
                active, spawn_delay, col: gcol,
            });
        }
    }

    // ── 每帧推进雨滴 ──
    for col in &mut app.password_matrix_cols {
        if col.active {
            // 头部下移 (分数速度: 每tick累加, 超过分母则移动)
            // 使用tick的低位作为累加器
            let phase = (tick.wrapping_mul(col.speed_num as u64) / col.speed_den as u64) as isize;
            let prev_phase = (tick.wrapping_sub(1).wrapping_mul(col.speed_num as u64) / col.speed_den as u64) as isize;
            if phase != prev_phase {
                col.head_y += 1;
            }
            // 雨滴完全离开屏幕 → 停用并设置重新生成延迟
            if col.head_y - col.len as isize > h_isize + 5 {
                col.active = false;
                col.spawn_delay = (col.col as u64 % 12) + 2; // 短延迟, 快速重生成 (原30+5)
            }
        } else {
            // 等待重新生成
            if col.spawn_delay > 0 {
                col.spawn_delay -= 1;
            } else {
                // 重新初始化雨滴
                let s = app.password_matrix_seed
                    .wrapping_add(col.col as u64)
                    .wrapping_add(tick)
                    .wrapping_mul(0x9E3779B97F4A7C15);
                let cs = {
                    let mut x = s;
                    x ^= x << 13; x ^= x >> 7; x ^= x << 17; x
                };
                col.head_y = -((cs % 15) as isize + 1); // 从屏幕上方随机位置开始
                col.len = 5 + (cs >> 4) as usize % 13; // 长度5-17
                col.char_idx = (cs >> 8) as usize % CHARS.len();
                for d in 0..6 {
                    col.trail_char[d] = (col.char_idx.wrapping_add(d * 3 + 7)) % CHARS.len();
                }
                col.speed_den = 2 + (cs >> 16) as isize % 3;
                col.active = true;
            }
        }
    }

    // ── 逐行构建渲染 ──
    let mut cached: Vec<Line<'static>> = Vec::with_capacity(h);
    for row in 0..h_isize {
        // 先填满空格
        let mut row_chars: Vec<(&str, Style)> = vec![(" ", c_empty); grid_cols];

        for col in &app.password_matrix_cols {
            if !col.active {
                continue;
            }
            let dist = col.head_y - row;
            if dist < 0 {
                // 行在雨滴下方 → 空白
                continue;
            }
            let d = dist as usize;
            if d == 0 {
                // 头部
                row_chars[col.col] = (CHARS[col.char_idx], c_head);
            } else if d <= col.len {
                // 尾部渐暗 (适配更长雨滴 5-17)
                let (ch, style) = if d <= 1 {
                    (CHARS[col.trail_char[0]], c_t1)
                } else if d <= 2 {
                    (CHARS[col.trail_char[1]], c_t2)
                } else if d <= 4 {
                    (CHARS[col.trail_char[2]], c_t3)
                } else if d <= 7 {
                    (CHARS[col.trail_char[3]], c_t4)
                } else if d <= 11 {
                    (CHARS[col.trail_char[4]], c_t5)
                } else {
                    (CHARS[col.trail_char[5]], c_t6)
                };
                row_chars[col.col] = (ch, style);
            }
        }

        let spans: Vec<Span<'static>> = row_chars
            .iter()
            .map(|(ch, style)| Span::styled(ch.to_string(), *style))
            .collect();
        cached.push(Line::from(spans));
    }
    app.password_matrix_cache = cached;

    // ★ 渲染缓存矩阵 — 直接复用已构建的 Line<'static>
    frame.render_widget(
        Paragraph::new(app.password_matrix_cache.clone()),
        area,
    );

    // ── 居中登录框 ──
    let box_w = (w.min(54) as u16).max(34);
    let box_h = 7u16;
    let box_x = area.x + (area.width.saturating_sub(box_w)) / 2;
    // ★ v4.5: 防止登录框靠底, 最小距离顶部2行
    let box_y = (area.y + (area.height.saturating_sub(box_h)) / 2).max(area.y + 2);
    let box_rect = Rect::new(box_x, box_y, box_w, box_h);

    // 登录框背景光晕 (四角装饰)
    {
        // 上边框装饰
        let top_deco = Rect::new(box_x, box_y.saturating_sub(1), box_w, 1);
        if box_y > 0 {
            let deco_line = Line::from(vec![
                Span::styled("╔".to_string(), Style::default().fg(theme::MATRIX_DIM)),
                Span::styled("═".repeat(box_w.saturating_sub(2) as usize), Style::default().fg(theme::MATRIX_DARK)),
                Span::styled("╗".to_string(), Style::default().fg(theme::MATRIX_DIM)),
            ]);
            frame.render_widget(Clear, top_deco);
            frame.render_widget(Paragraph::new(deco_line), top_deco);
        }
        // 下边框装饰
        let bot_deco = Rect::new(box_x, box_y + box_h, box_w, 1);
        let deco_line = Line::from(vec![
            Span::styled("╚".to_string(), Style::default().fg(theme::MATRIX_DIM)),
            Span::styled("═".repeat(box_w.saturating_sub(2) as usize), Style::default().fg(theme::MATRIX_DARK)),
            Span::styled("╝".to_string(), Style::default().fg(theme::MATRIX_DIM)),
        ]);
        frame.render_widget(Clear, bot_deco);
        frame.render_widget(Paragraph::new(deco_line), bot_deco);
    }

    // 清除登录框区域
    frame.render_widget(Clear, box_rect);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // 标题
            Constraint::Length(1),  // 分隔
            Constraint::Length(1),  // 空行
            Constraint::Length(1),  // 提示
            Constraint::Length(1),  // 密码输入行
            Constraint::Length(1),  // 提示2
            Constraint::Length(1),  // 底部线
        ])
        .split(box_rect);

    // 标题行 (v4.5: 根据登录阶段动态显示)
    let phase_title = match app.login_phase {
        crate::LoginPhase::CreatePassword | crate::LoginPhase::ConfirmPassword => "创建 Vault",
        crate::LoginPhase::LoginFailed => "⚠ 验证失败",
        _ => "身份验证",
    };
    let title_color = if app.login_phase == crate::LoginPhase::LoginFailed {
        theme::DANGER
    } else {
        theme::ACCENT
    };
    let title = Line::from(vec![
        Span::styled(" RUOO-CONSOLE ", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
        Span::styled("v4.5", Style::default().fg(theme::MUTED)),
        Span::styled(" │ ", Style::default().fg(theme::MATRIX_DIM)),
        Span::styled(phase_title, Style::default().fg(title_color)),
    ]);
    frame.render_widget(Paragraph::new(title).alignment(ratatui::layout::Alignment::Center), inner[0]);

    // 分隔线
    let sep_w = box_w.saturating_sub(4) as usize;
    let sep = "═".repeat(sep_w);
    frame.render_widget(
        Paragraph::new(Span::styled(sep.clone(), Style::default().fg(theme::MATRIX_DIM))),
        inner[1],
    );

    // 错误信息行 (★ BUG-FIX: 显示login_error, 之前错误被静默隐藏)
    if !app.login_error.is_empty() {
        let err_line = Line::from(vec![
            Span::styled(format!("  ⚠ {}", app.login_error), Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(Paragraph::new(err_line), inner[2]);
    }

    // 提示行
    let prompt_display = if app.password_prompt.is_empty() {
        "🔑 输入主密码"
    } else {
        &app.password_prompt
    };
    let prompt_line = Line::from(vec![
        Span::styled(format!("  {}", prompt_display), Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)),
    ]);
    frame.render_widget(Paragraph::new(prompt_line), inner[3]);

    // 密码输入行 (带光标) — 修复回车错位: 光标始终跟随输入
    let masked = "*".repeat(app.input.len());
    let cursor_idx = app.cursor.min(masked.len());
    let before = &masked[..cursor_idx];
    let after = &masked[cursor_idx..];

    let input_line = if after.is_empty() {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(before.to_string(), Style::default().fg(theme::MATRIX_GREEN)),
            Span::styled(" ", Style::default().fg(Color::Black).bg(theme::MATRIX_GREEN)),
        ])
    } else {
        let ch = after.chars().next().unwrap();
        let remaining = &after[ch.len_utf8()..];
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(before.to_string(), Style::default().fg(theme::MATRIX_GREEN)),
            Span::styled(ch.to_string(), Style::default().fg(Color::Black).bg(theme::MATRIX_GREEN)),
            Span::styled(remaining.to_string(), Style::default().fg(theme::MATRIX_GREEN)),
        ])
    };
    frame.render_widget(Paragraph::new(input_line), inner[4]);

    // 提示2
    let hint = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Enter 确认", Style::default().fg(theme::ACCENT)),
        Span::styled("  │  ", Style::default().fg(theme::MUTED)),
        Span::styled("Esc 取消", Style::default().fg(theme::DANGER)),
    ]);
    frame.render_widget(Paragraph::new(hint), inner[5]);

    // 底部线
    frame.render_widget(
        Paragraph::new(Span::styled(sep, Style::default().fg(theme::MATRIX_DIM))),
        inner[6],
    );

    // 光标坐标 — 映射到密码输入行
    app.cursor_screen_pos = Some((
        box_rect.x + 2 + cursor_idx as u16,
        box_rect.y + 4, // inner[4] = 密码输入行
    ));
}

// ── 侧栏渲染 (v5.0 — 全面扩充, 152+工具对齐) ──
// ── 右侧信息侧边栏渲染 (v13.0: F10切换, +/-/= 切换模式) ──
fn render_right_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::RightSidebarMode;
    // v13.1: 缓存遥测快照 — 每15帧(≈250ms@60fps)才刷新
    let refresh_interval: u64 = 15;
    let need_refresh = app.telemetry_cache.is_none()
        || app.tick_count.wrapping_sub(app.telemetry_cache_tick) >= refresh_interval;
    if need_refresh {
        app.telemetry_cache = Some(crate::telemetry::snapshot());
        app.telemetry_cache_tick = app.tick_count;
    }
    let tele = app.telemetry_cache.as_ref().unwrap();
    let p = &tele.proc;
    let border_style = Style::default().fg(theme::CYBER);

    // ── 辅助格式化 ──
    let uptime_str = if p.uptime_secs < 60 { format!("{}s", p.uptime_secs) }
                     else if p.uptime_secs < 3600 { format!("{}m", p.uptime_secs/60) }
                     else { format!("{}h{}m", p.uptime_secs/3600, (p.uptime_secs%3600)/60) };
    let rss_str = if p.memory_rss_mb >= 1024.0 { format!("{:.1}G", p.memory_rss_mb/1024.0) }
                  else { format!("{:.0}M", p.memory_rss_mb) };
    let virt_str = if p.memory_virtual_mb >= 1024.0 { format!("{:.1}G", p.memory_virtual_mb/1024.0) }
                   else { format!("{:.0}M", p.memory_virtual_mb) };
    let disk_str = if p.disk_total_gb > 0.0 {
        format!("{:.0}G/{:.0}G", p.disk_free_gb, p.disk_total_gb)
    } else { "N/A".to_string() };
    let success_rate = if p.total_tool_calls > 0 {
        ((p.total_tool_calls - p.total_tool_errors) as f64 / p.total_tool_calls as f64 * 100.0) as u32
    } else { 100 };
    // v13.1: 管理员指示
    let admin_str = if app.is_admin { "ROOT" } else { "user" };
    let admin_color = if app.is_admin { theme::DANGER } else { theme::MUTED };

    match app.right_sidebar_mode {
        RightSidebarMode::Dashboard => {
            // ── v14.0 仪表盘: 自适应布局 + 真内存/CPU/磁盘条 + 网络速率 ──
            // 辅助: ASCII 进度条生成 (█ = 满, ▓ = 3/4, ▒ = 1/2, ░ = 1/4)
            let bar = |pct: u32, w: usize, color: Color| -> Vec<Span> {
                let w = w.max(1);
                let filled = (pct as usize * w / 100).min(w);
                let rem = (pct as usize * w * 4 / 100) - filled * 4; // 0-3
                let mut spans = Vec::new();
                for _ in 0..filled { spans.push(Span::styled("\u{2588}", Style::default().fg(color))); }
                if filled < w {
                    let ch = match rem { 3 => "\u{2593}", 2 => "\u{2592}", 1 => "\u{2591}", _ => " " };
                    spans.push(Span::styled(ch, Style::default().fg(theme::MUTED)));
                    for _ in (filled+1)..w { spans.push(Span::styled(" ", Style::default())); }
                }
                spans
            };

            // ── 系统内存% (对系统总RAM) ──
            let sys_mem_pct = if p.system_ram_total_mb > 0.0 {
                ((p.memory_rss_mb / p.system_ram_total_mb * 100.0).min(100.0)) as u32
            } else { 0 };
            let commit_str = if p.memory_commit_mb >= 1024.0 {
                format!("{:.1}G", p.memory_commit_mb/1024.0)
            } else { format!("{:.0}M", p.memory_commit_mb) };
            let sys_ram_str = if p.system_ram_total_mb >= 1024.0 {
                format!("{:.1}G", p.system_ram_total_mb/1024.0)
            } else { format!("{:.0}M", p.system_ram_total_mb) };
            let disk_pct = if p.disk_total_gb > 0.0 {
                ((1.0 - p.disk_free_gb / p.disk_total_gb) * 100.0) as u32
            } else { 0 };
            let disk_free_str = if p.disk_free_gb >= 10.0 {
                format!("{:.0}G", p.disk_free_gb)
            } else { format!("{:.1}G", p.disk_free_gb) };
            let disk_total_str = if p.disk_total_gb >= 10.0 {
                format!("{:.0}G", p.disk_total_gb)
            } else { format!("{:.1}G", p.disk_total_gb) };

            // 计算工具列表可显示数量 & AI活动数
            let tool_events = &tele.tools;
            let net_events = &tele.network;
            let has_tools = !tool_events.is_empty();
            let has_net = !net_events.is_empty();
            let has_ai = !tele.ai.is_empty();

            // ── 自适应高度 ──
            let res_fixed = 14u16;
            let act_min_lines = 6u16;
            let avail_h = area.height;
            let res_h = if avail_h > res_fixed + act_min_lines {
                (avail_h as f64 * 0.42) as u16
            } else {
                res_fixed.min(avail_h.saturating_sub(act_min_lines))
            }.max(10);

            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(res_h), Constraint::Min(act_min_lines)])
                .split(area);

            // ═══ 上半: 系统资源 ═══
            let bar_w = (parts[0].width.saturating_sub(8) as usize).max(4);
            let mut res_lines: Vec<Line> = Vec::new();

            res_lines.push(Line::from(vec![
                Span::styled(format!("{} ", &p.hostname), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(admin_str, Style::default().fg(admin_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}c", p.cpu_cores), Style::default().fg(theme::MUTED)),
            ]));
            res_lines.push(Line::from(vec![
                Span::styled("Up ", Style::default().fg(theme::MUTED)),
                Span::styled(uptime_str, Style::default().fg(theme::ACCENT)),
                Span::styled(format!(" PID:{}", p.pid), Style::default().fg(theme::MUTED)),
            ]));

            res_lines.push(Line::from(Span::styled("", Style::default())));
            res_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{5185}\u{5b58} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
            res_lines.push(Line::from(vec![
                Span::styled("RSS ", Style::default().fg(theme::MUTED)),
                Span::styled(rss_str, Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)),
                Span::styled(format!("/{}", sys_ram_str), Style::default().fg(theme::MUTED)),
            ]));
            let mem_color = if sys_mem_pct > 80 { theme::DANGER } else if sys_mem_pct > 50 { theme::WARN } else { theme::DIM_GREEN };
            res_lines.push(Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled(format!("{:>3}%", sys_mem_pct), Style::default().fg(mem_color)),
                Span::styled(" ", Style::default()),
            ].into_iter().chain(bar(sys_mem_pct, bar_w, mem_color)).collect::<Vec<_>>()));
            res_lines.push(Line::from(vec![
                Span::styled("Cmt ", Style::default().fg(theme::MUTED)),
                Span::styled(&commit_str, Style::default().fg(theme::MUTED)),
                Span::styled(format!(" Virt:{}", virt_str), Style::default().fg(theme::MUTED)),
            ]));

            res_lines.push(Line::from(Span::styled("\u{2500}\u{2500} CPU \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
            let cpu_str = format!("{:.1}%", p.cpu_percent);
            let cpu_color = if p.cpu_percent > 75.0 { theme::DANGER } else if p.cpu_percent > 30.0 { theme::WARN } else { theme::DIM_GREEN };
            let cpu_pct = p.cpu_percent.min(100.0) as u32;
            res_lines.push(Line::from(vec![
                Span::styled("CPU ", Style::default().fg(theme::MUTED)),
                Span::styled(&cpu_str, Style::default().fg(cpu_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" Thr:{}", p.thread_count), Style::default().fg(theme::MUTED)),
            ]));
            res_lines.push(Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled(format!("{:>3}%", cpu_pct), Style::default().fg(cpu_color)),
                Span::styled(" ", Style::default()),
            ].into_iter().chain(bar(cpu_pct, bar_w, cpu_color)).collect::<Vec<_>>()));

            res_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{78c1}\u{76d8} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
            let dsk_color = if disk_pct > 90 { theme::DANGER } else if disk_pct > 70 { theme::WARN } else { theme::DIM_GREEN };
            res_lines.push(Line::from(vec![
                Span::styled("Disk ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}/{}", disk_free_str, disk_total_str), Style::default().fg(dsk_color)),
                Span::styled(format!(" {}%\u{7528}", disk_pct), Style::default().fg(if disk_pct > 90 { theme::DANGER } else { theme::MUTED })),
            ]));

            frame.render_widget(
                Paragraph::new(res_lines)
                    .block(Block::new().borders(Borders::ALL).border_style(border_style)
                        .title(Span::styled(" \u{7cfb}\u{7edf}\u{8d44}\u{6e90} ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)))),
                parts[0],
            );

            // ═══ 下半: 活动面板 ═══
            // v14.1: 始终平分 — 避免内容出现/消失时布局突变导致面板跳动
            let act_split = [Constraint::Percentage(50), Constraint::Percentage(50)];
            let act_parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints(act_split)
                .split(parts[1]);

            // ── 工具调用 ──
            {
                let mut tool_lines: Vec<Line> = Vec::new();
                tool_lines.push(Line::from(vec![
                    Span::styled("\u{03a3} ", Style::default().fg(theme::MUTED)),
                    Span::styled(format!("{}/{}", p.total_tool_calls, p.total_tool_errors), Style::default().fg(theme::WARN)),
                    Span::styled(format!(" {}%", success_rate),
                        Style::default().fg(if p.total_tool_errors > 0 { theme::WARN } else { theme::DIM_GREEN })),
                    Span::styled(format!(" AI:{}", p.total_ai_requests), Style::default().fg(theme::DP_BLUE)),
                ]));

                let avail_tool_rows = act_parts[0].height.saturating_sub(2) as usize;
                if has_tools && avail_tool_rows > 1 {
                    let max_display = avail_tool_rows.min(10);
                    let recent_tools: Vec<&crate::telemetry::ToolEvent> = tool_events.iter().rev().take(max_display).collect();
                    for ev in recent_tools.iter().rev() {
                        let elapsed = ev.timestamp.elapsed().as_secs();
                        let time_str = if elapsed < 60 { format!("{}s", elapsed) }
                                       else if elapsed < 3600 { format!("{}m", elapsed/60) }
                                       else { format!("{}h", elapsed/3600) };
                        let (status_ch, status_color) = match ev.status {
                            crate::telemetry::ToolStatus::Started => ("\u{00b7}", theme::MUTED),
                            crate::telemetry::ToolStatus::Success => ("\u{2713}", theme::ACCENT),
                            crate::telemetry::ToolStatus::Failed  => ("\u{2717}", theme::DANGER),
                            crate::telemetry::ToolStatus::Timeout => ("\u{23f1}", theme::WARN),
                        };
                        let max_name_w = 14usize;
                        let short_name: String = if ev.tool_name.len() > max_name_w {
                            format!("{}\u{2026}", &ev.tool_name[..max_name_w-1])
                        } else { ev.tool_name.clone() };
                        tool_lines.push(Line::from(vec![
                            Span::styled(format!("{:>3}", time_str), Style::default().fg(theme::MUTED)),
                            Span::styled(status_ch, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                            Span::styled(format!(" {:<16}", short_name), Style::default().fg(theme::WHITE)),
                            Span::styled(if ev.duration_ms >= 1000 {
                                format!("{:.1}s", ev.duration_ms as f64/1000.0)
                            } else { format!("{}ms", ev.duration_ms) },
                                Style::default().fg(theme::MUTED)),
                        ]));
                    }
                } else if !has_tools {
                    tool_lines.push(Line::from(Span::styled(" \u{00b7} \u{65e0}\u{6d3b}\u{52a8}", Style::default().fg(theme::MUTED))));
                }
                if has_ai {
                    if let Some(ev) = tele.ai.last() {
                        let phase_label = match &ev.phase {
                            crate::telemetry::AiPhase::Thinking => "\u{601d}\u{8003}",
                            crate::telemetry::AiPhase::ToolCalling(t) => if t.len() > 12 { &t[..11] } else { t },
                            crate::telemetry::AiPhase::Streaming => "\u{8f93}\u{51fa}",
                            crate::telemetry::AiPhase::Done => "\u{5b8c}\u{6210}",
                            crate::telemetry::AiPhase::Error(_) => "\u{9519}\u{8bef}",
                        };
                        let phase_color = match &ev.phase {
                            crate::telemetry::AiPhase::Thinking => theme::DP_BLUE,
                            crate::telemetry::AiPhase::ToolCalling(_) => theme::WARN,
                            crate::telemetry::AiPhase::Streaming => theme::ACCENT,
                            crate::telemetry::AiPhase::Done => theme::DIM_GREEN,
                            crate::telemetry::AiPhase::Error(_) => theme::DANGER,
                        };
                        tool_lines.push(Line::from(vec![
                            Span::styled("AI ", Style::default().fg(theme::MUTED)),
                            Span::styled(phase_label, Style::default().fg(phase_color).add_modifier(Modifier::BOLD)),
                        ]));
                    }
                }

                // ★ BUG-FIX: 钳位内容到可视高度 (减去上下边框2行), 防止溢出挤压下方网络收发区
                let max_visible = (act_parts[0].height.saturating_sub(2) as usize).max(1);
                if tool_lines.len() > max_visible {
                    tool_lines.truncate(max_visible);
                }

                frame.render_widget(
                    Paragraph::new(tool_lines)
                        .block(Block::new().borders(Borders::ALL).border_style(border_style)
                            .title(Span::styled(" \u{5de5}\u{5177}\u{8c03}\u{7528} ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)))),
                    act_parts[0],
                );
            }

            // ── 网络收发 ──
            {
                let mut net_lines: Vec<Line> = Vec::new();
                // ── 程序流量已移除, 仅显示本机全部 ──

                // ── 本机全部网络 (所有网卡) ──
                let sys_up_str = if p.sys_net_tx_bps > 1_000_000.0 {
                    format!("{:.1}MB/s", p.sys_net_tx_bps / 1_000_000.0)
                } else if p.sys_net_tx_bps > 1_000.0 {
                    format!("{:.0}KB/s", p.sys_net_tx_bps / 1_000.0)
                } else if p.sys_net_tx_bps > 0.0 {
                    format!("{:.0}B/s", p.sys_net_tx_bps)
                } else {
                    "0 B/s".to_string()
                };
                let sys_down_str = if p.sys_net_rx_bps > 1_000_000.0 {
                    format!("{:.1}MB/s", p.sys_net_rx_bps / 1_000_000.0)
                } else if p.sys_net_rx_bps > 1_000.0 {
                    format!("{:.0}KB/s", p.sys_net_rx_bps / 1_000.0)
                } else if p.sys_net_rx_bps > 0.0 {
                    format!("{:.0}B/s", p.sys_net_rx_bps)
                } else {
                    "0 B/s".to_string()
                };
                let sys_tx_total = if p.sys_net_tx_bytes > 1_000_000_000 { format!("{:.1}G", p.sys_net_tx_bytes as f64/1_000_000_000.0) }
                    else if p.sys_net_tx_bytes > 1_000_000 { format!("{:.1}M", p.sys_net_tx_bytes as f64/1_000_000.0) }
                    else if p.sys_net_tx_bytes > 1_000 { format!("{:.0}K", p.sys_net_tx_bytes as f64/1_000.0) }
                    else { format!("{}B", p.sys_net_tx_bytes) };
                let sys_rx_total = if p.sys_net_rx_bytes > 1_000_000_000 { format!("{:.1}G", p.sys_net_rx_bytes as f64/1_000_000_000.0) }
                    else if p.sys_net_rx_bytes > 1_000_000 { format!("{:.1}M", p.sys_net_rx_bytes as f64/1_000_000.0) }
                    else if p.sys_net_rx_bytes > 1_000 { format!("{:.0}K", p.sys_net_rx_bytes as f64/1_000.0) }
                    else { format!("{}B", p.sys_net_rx_bytes) };

                net_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{672c}\u{673a}\u{5168}\u{90e8} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
                net_lines.push(Line::from(vec![
                    Span::styled("\u{25b2} ", Style::default().fg(theme::DANGER)),
                    Span::styled(&sys_up_str, Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" \u{03a3}{}", sys_tx_total), Style::default().fg(theme::MUTED)),
                ]));
                net_lines.push(Line::from(vec![
                    Span::styled("\u{25bc} ", Style::default().fg(theme::ACCENT)),
                    Span::styled(&sys_down_str, Style::default().fg(theme::DIM_GREEN).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" \u{03a3}{}", sys_rx_total), Style::default().fg(theme::MUTED)),
                ]));

                net_lines.push(Line::from(vec![
                    Span::styled("\u{2197} ", Style::default().fg(theme::MUTED)),
                    Span::styled(format!("{}\u{6b21}", tele.net_connections), Style::default().fg(theme::ACCENT)),
                    Span::styled(format!(" /{}s", tele.net_window_secs), Style::default().fg(theme::MUTED)),
                ]));

                let avail_net_rows = act_parts[1].height.saturating_sub(4) as usize;
                if has_net && avail_net_rows > 0 {
                    let max_display = avail_net_rows.min(5);
                    let recent_net: Vec<&crate::telemetry::NetworkEvent> = net_events.iter().rev().take(max_display).collect();
                    net_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{6700}\u{8fd1} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
                    for ev in recent_net.iter().rev() {
                        let dir_ch = match ev.direction {
                            crate::telemetry::NetDirection::Upload => "\u{25b2}",
                            crate::telemetry::NetDirection::Download => "\u{25bc}",
                            crate::telemetry::NetDirection::Both => "\u{21c5}",
                        };
                        let max_host_w = 16usize;
                        let short_host: String = if ev.host.len() > max_host_w {
                            format!("{}\u{2026}", &ev.host[..max_host_w-1])
                        } else { ev.host.clone() };
                        let bytes_str = if let Some(b) = ev.bytes {
                            if b > 1_000_000 { format!("{:.1}M", b as f64/1_000_000.0) }
                            else if b > 1_000 { format!("{:.0}K", b as f64/1_000.0) }
                            else { format!("{}B", b) }
                        } else { "".to_string() };
                        net_lines.push(Line::from(vec![
                            Span::styled(dir_ch, Style::default().fg(theme::MUTED)),
                            Span::styled(format!("{}:{}", short_host, ev.port), Style::default().fg(theme::ACCENT)),
                            Span::styled(format!(" {}", bytes_str), Style::default().fg(theme::MUTED)),
                        ]));
                    }
                } else if !has_net {
                    net_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{63a5}\u{53e3} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
                    if p.network_ips.is_empty() {
                        net_lines.push(Line::from(Span::styled(" (\u{65e0})", Style::default().fg(theme::MUTED))));
                    } else {
                        for ip in p.network_ips.iter().take(2) {
                            net_lines.push(Line::from(Span::styled(ip, Style::default().fg(theme::ACCENT))));
                        }
                    }
                }

                if act_parts[1].height > 12 {
                    net_lines.push(Line::from(Span::styled("\u{2500}\u{2500} \u{4f1a}\u{8bdd} \u{2500}\u{2500}", Style::default().fg(theme::CYBER))));
                    net_lines.push(Line::from(vec![
                        Span::styled("Tgt ", Style::default().fg(theme::MUTED)),
                        Span::styled(if app.target.is_empty() { "(\u{672a}\u{8bbe})" } else { &app.target },
                            Style::default().fg(if app.target.is_empty() { theme::MUTED } else { theme::WARN })),
                    ]));
                    net_lines.push(Line::from(vec![
                        Span::styled("Out ", Style::default().fg(theme::MUTED)),
                        Span::styled(format!("{}\u{884c}", app.output.len()), Style::default().fg(theme::ACCENT)),
                    ]));
                }

                // ★ v14.1: 钳位内容到可视高度 (减去上下边框2行), 防止溢出挤出屏幕
                let max_visible = (act_parts[1].height.saturating_sub(2) as usize).max(1);
                if net_lines.len() > max_visible {
                    // 固定头: 本机全部段(3行) + 连接数(1行) = 4行
                    let fixed_hdr = 4usize;
                    if max_visible > fixed_hdr + 2 {
                        // 空间充足: 截断到 max_visible (含部分事件行)
                        net_lines.truncate(max_visible);
                    } else if max_visible >= fixed_hdr {
                        // 刚好够固定头: 保留固定头部, 丢弃额外内容
                        net_lines.truncate(fixed_hdr);
                    } else {
                        // 极小空间: 尽可能保留更多的头部行
                        net_lines.truncate(max_visible);
                    }
                }

                frame.render_widget(
                    Paragraph::new(net_lines)
                        .block(Block::new().borders(Borders::ALL).border_style(border_style)
                            .title(Span::styled(" \u{7f51}\u{7edc}\u{6536}\u{53d1} ", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)))),
                    act_parts[1],
                );
        }
            }
        RightSidebarMode::SystemInfo => {
            // ── 系统详情全屏 ──
            let mut sys_lines: Vec<Line> = Vec::new();

            // ═══ 主机信息 ═══
            sys_lines.push(Line::from(Span::styled("═══ 主机 ═══", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));
            sys_lines.push(Line::from(vec![
                Span::styled("名称 ", Style::default().fg(theme::MUTED)),
                Span::styled(&p.hostname, Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" [{}]", admin_str), Style::default().fg(admin_color)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("OS   ", Style::default().fg(theme::MUTED)),
                Span::styled(std::env::consts::OS, Style::default().fg(theme::ACCENT)),
                Span::styled(format!(" {} {}", std::env::consts::ARCH, std::env::consts::FAMILY), Style::default().fg(theme::MUTED)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("核心 ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}c", p.cpu_cores), Style::default().fg(theme::ACCENT)),
            ]));
            // 工作目录
            let short_dir = if p.work_dir.len() > 22 { format!("..{}", &p.work_dir[p.work_dir.len()-20..]) } else { p.work_dir.clone() };
            sys_lines.push(Line::from(vec![
                Span::styled("目录 ", Style::default().fg(theme::MUTED)),
                Span::styled(short_dir, Style::default().fg(theme::MUTED)),
            ]));

            sys_lines.push(Line::from(Span::styled("", Style::default())));

            // ═══ 进程 ═══
            sys_lines.push(Line::from(Span::styled("═══ 进程 ═══", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));
            sys_lines.push(Line::from(vec![
                Span::styled("PID  ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}", p.pid), Style::default().fg(theme::ACCENT)),
                Span::styled(format!(" Up:{}", uptime_str), Style::default().fg(theme::MUTED)),
            ]));
            let mem_pct = if p.system_ram_total_mb > 0.0 { (p.memory_rss_mb / p.system_ram_total_mb * 100.0).min(100.0) as u32 } else { 0 };
            sys_lines.push(Line::from(vec![
                Span::styled("RSS  ", Style::default().fg(theme::MUTED)),
                Span::styled(rss_str, Style::default().fg(theme::WARN)),
                Span::styled(format!(" {}%", mem_pct), Style::default().fg(if mem_pct > 80 { theme::DANGER } else { theme::MUTED })),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("Virt ", Style::default().fg(theme::MUTED)),
                Span::styled(virt_str, Style::default().fg(theme::MUTED)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("线程 ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}/{}c", p.thread_count, p.cpu_cores), Style::default().fg(theme::ACCENT)),
            ]));

            sys_lines.push(Line::from(Span::styled("", Style::default())));

            // ═══ 存储与网络 ═══
            sys_lines.push(Line::from(Span::styled("═══ 存储与网络 ═══", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));
            let disk_pct = if p.disk_total_gb > 0.0 { ((1.0 - p.disk_free_gb / p.disk_total_gb) * 100.0) as u32 } else { 0 };
            sys_lines.push(Line::from(vec![
                Span::styled("磁盘 ", Style::default().fg(theme::MUTED)),
                Span::styled(disk_str, Style::default().fg(theme::DIM_GREEN)),
                Span::styled(format!(" 已用{}%", disk_pct), Style::default().fg(if disk_pct > 90 { theme::DANGER } else { theme::MUTED })),
            ]));
            // 网络接口IP
            if p.network_ips.is_empty() {
                sys_lines.push(Line::from(vec![
                    Span::styled("网络 ", Style::default().fg(theme::MUTED)),
                    Span::styled("(无)", Style::default().fg(theme::MUTED)),
                ]));
            } else {
                for (i, ip) in p.network_ips.iter().enumerate() {
                    if i == 0 {
                        sys_lines.push(Line::from(vec![
                            Span::styled("网络 ", Style::default().fg(theme::MUTED)),
                            Span::styled(ip, Style::default().fg(theme::ACCENT)),
                        ]));
                    } else {
                        sys_lines.push(Line::from(vec![
                            Span::styled("     ", Style::default().fg(theme::MUTED)),
                            Span::styled(ip, Style::default().fg(theme::ACCENT)),
                        ]));
                    }
                }
            }

            sys_lines.push(Line::from(Span::styled("", Style::default())));

            // ═══ 全网收发 ═══
            sys_lines.push(Line::from(Span::styled("═══ 全网收发 ═══", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));
            let sys_up_str = if p.sys_net_tx_bps > 1_000_000.0 {
                format!("{:.1}MB/s", p.sys_net_tx_bps / 1_000_000.0)
            } else if p.sys_net_tx_bps > 1_000.0 {
                format!("{:.0}KB/s", p.sys_net_tx_bps / 1_000.0)
            } else if p.sys_net_tx_bps > 0.0 {
                format!("{:.0}B/s", p.sys_net_tx_bps)
            } else { "0 B/s".to_string() };
            let sys_down_str = if p.sys_net_rx_bps > 1_000_000.0 {
                format!("{:.1}MB/s", p.sys_net_rx_bps / 1_000_000.0)
            } else if p.sys_net_rx_bps > 1_000.0 {
                format!("{:.0}KB/s", p.sys_net_rx_bps / 1_000.0)
            } else if p.sys_net_rx_bps > 0.0 {
                format!("{:.0}B/s", p.sys_net_rx_bps)
            } else { "0 B/s".to_string() };
            let sys_tx_total = if p.sys_net_tx_bytes > 1_000_000_000 { format!("{:.1}G", p.sys_net_tx_bytes as f64/1_000_000_000.0) }
                else if p.sys_net_tx_bytes > 1_000_000 { format!("{:.1}M", p.sys_net_tx_bytes as f64/1_000_000.0) }
                else if p.sys_net_tx_bytes > 1_000 { format!("{:.0}K", p.sys_net_tx_bytes as f64/1_000.0) }
                else { format!("{}B", p.sys_net_tx_bytes) };
            let sys_rx_total = if p.sys_net_rx_bytes > 1_000_000_000 { format!("{:.1}G", p.sys_net_rx_bytes as f64/1_000_000_000.0) }
                else if p.sys_net_rx_bytes > 1_000_000 { format!("{:.1}M", p.sys_net_rx_bytes as f64/1_000_000.0) }
                else if p.sys_net_rx_bytes > 1_000 { format!("{:.0}K", p.sys_net_rx_bytes as f64/1_000.0) }
                else { format!("{}B", p.sys_net_rx_bytes) };
            sys_lines.push(Line::from(vec![
                Span::styled("▲发送 ", Style::default().fg(theme::MUTED)),
                Span::styled(&sys_up_str, Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" Σ{}", sys_tx_total), Style::default().fg(theme::MUTED)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("▼接收 ", Style::default().fg(theme::MUTED)),
                Span::styled(&sys_down_str, Style::default().fg(theme::DIM_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" Σ{}", sys_rx_total), Style::default().fg(theme::MUTED)),
            ]));

            sys_lines.push(Line::from(Span::styled("", Style::default())));

            // ═══ 会话统计 ═══
            sys_lines.push(Line::from(Span::styled("═══ 会话统计 ═══", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD))));

            sys_lines.push(Line::from(vec![
                Span::styled("输出 ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}行", app.output.len()), Style::default().fg(theme::ACCENT)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("工具 ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("调用{} 错误{} {}%", p.total_tool_calls, p.total_tool_errors, success_rate),
                    Style::default().fg(if p.total_tool_errors > 0 { theme::WARN } else { theme::DIM_GREEN })),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("AI   ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}次", p.total_ai_requests), Style::default().fg(theme::DP_BLUE)),
                Span::styled(format!(" 网络{}次", p.total_net_connections), Style::default().fg(theme::CYBER)),
            ]));
            sys_lines.push(Line::from(vec![
                Span::styled("系统 ", Style::default().fg(theme::MUTED)),
                Span::styled(format!("{}事件", p.total_sys_events), Style::default().fg(theme::MUTED)),
            ]));
            // AI模式
            if app.ai_mode || app.ai_pending {
                sys_lines.push(Line::from(Span::styled("", Style::default())));
                sys_lines.push(Line::from(vec![
                    Span::styled("AI   ", Style::default().fg(theme::MUTED)),
                    Span::styled(if app.ai_pending { "思考中..." } else { "#dp活跃" },
                        Style::default().fg(theme::DP_BLUE).add_modifier(Modifier::BOLD)),
                ]));
            }

            // ★ BUG-FIX: 钳位到可视高度, 防止溢出
            let max_visible = (area.height.saturating_sub(2) as usize).max(1);
            if sys_lines.len() > max_visible {
                sys_lines.truncate(max_visible);
            }

            frame.render_widget(
                Paragraph::new(sys_lines)
                    .block(Block::new().borders(Borders::ALL).border_style(border_style)
                        .title(Span::styled(" 系统详情 ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)))),
                area,
            );
        }
    }
}

fn render_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let buf = match &app.editor_buffer {
        Some(b) => b,
        None => return,
    };

    let editor_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // 标题栏
            Constraint::Length(1),  // 状态栏
            Constraint::Min(0),     // 编辑区
            Constraint::Length(1),  // 快捷键
        ])
        .split(area);

    // ── 标题 ──
    let modified_marker = if buf.modified { " [MOD]" } else { "" };
    let title = Line::from(vec![
        Span::styled("[EDIT] 启动脚本编辑器 ", Style::default().fg(theme::CYBER).add_modifier(Modifier::BOLD)),
        Span::styled(&buf.file_path_hint, Style::default().fg(theme::MUTED)),
        Span::styled(modified_marker, Style::default().fg(theme::WARN)),
        Span::styled(format!(" | {} 行", buf.lines.len()), Style::default().fg(theme::ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(title), editor_area[0]);

    // ── 状态栏 (光标位置) ──
    let line_count = buf.lines.len();
    let cur_line = app.editor_cursor_row + 1;
    let cur_col = app.editor_cursor_col + 1;
    let status = Line::from(vec![
        Span::styled(format!("行 {}/{}  列 {}", cur_line, line_count, cur_col), 
            Style::default().fg(theme::DIM_GREEN)),
        Span::styled("  │  Ctrl+S 保存  │  Esc 取消  │  Ctrl+Q 强制退出", 
            Style::default().fg(theme::MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(status).style(Style::default().bg(Color::Rgb(20, 24, 35))),
        editor_area[1],
    );

    // ── 编辑区 ──
    let visible_rows = editor_area[2].height.saturating_sub(1) as usize;
    let scroll = app.editor_scroll;

    let styled_lines: Vec<Line> = buf.lines.iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
        .map(|(i, line)| {
            let line_num = i + 1;
            let is_current = i == app.editor_cursor_row;

            if is_current {
                // 当前行 — 显示光标
                let line_style = Style::default().bg(Color::Rgb(30, 40, 55));
                let cursor_col = app.editor_cursor_col;

                if line.is_empty() {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>4} │ ", line_num),
                            Style::default().fg(theme::MUTED).bg(Color::Rgb(30, 40, 55)),
                        ),
                        Span::styled(
                            " ",
                            Style::default().fg(Color::Black).bg(theme::ACCENT),
                        ),
                    ])
                } else if cursor_col >= line.chars().count() {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>4} │ ", line_num),
                            Style::default().fg(theme::MUTED).bg(Color::Rgb(30, 40, 55)),
                        ),
                        Span::styled(line.clone(), line_style),
                        Span::styled(" ", Style::default().fg(Color::Black).bg(theme::ACCENT)),
                    ])
                } else {
                    let before: String = line.chars().take(cursor_col).collect();
                    let at: String = line.chars().skip(cursor_col).take(1).collect();
                    let after: String = line.chars().skip(cursor_col + 1).collect();
                    Line::from(vec![
                        Span::styled(
                            format!("{:>4} │ ", line_num),
                            Style::default().fg(theme::MUTED).bg(Color::Rgb(30, 40, 55)),
                        ),
                        Span::styled(before, line_style),
                        Span::styled(at, Style::default().fg(Color::Black).bg(theme::ACCENT)),
                        Span::styled(after, line_style),
                    ])
                }
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("{:>4} │ ", line_num),
                        Style::default().fg(theme::MUTED),
                    ),
                    Span::styled(line.clone(), Style::default().fg(theme::WHITE)),
                ])
            }
        })
        .collect();

    frame.render_widget(
        Paragraph::new(styled_lines)
            .block(Block::new().borders(Borders::ALL)
                .border_style(Style::default().fg(theme::CYBER))
                .title(Span::styled(" 编辑区 ", Style::default().fg(theme::ACCENT)))),
        editor_area[2],
    );

    // ── 快捷键 ──
    let shortcuts = Line::from(vec![
        Span::styled("Ctrl+S 保存到Vault", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" | Esc 取消", Style::default().fg(theme::DANGER)),
        Span::styled(" | Ctrl+Q 放弃退出", Style::default().fg(theme::WARN)),
        Span::styled(" | ↑↓←→ 移动", Style::default().fg(theme::MUTED)),
        Span::styled(" | PgUp/PgDn 翻页", Style::default().fg(theme::MUTED)),
        Span::styled(" | Home/End 行首尾", Style::default().fg(theme::MUTED)),
    ]);
    frame.render_widget(Paragraph::new(shortcuts), editor_area[3]);

    // ★ 编辑器 IME 光标定位 (使用显示列宽, CJK=2列)
    let edit_rect = editor_area[2];
    let visible_row = app.editor_cursor_row.saturating_sub(app.editor_scroll);
    let line_num_width = 6u16; // "123 │ " 的宽度
    let cursor_display_col = if app.editor_cursor_row < buf.lines.len() {
        let cur_line = &buf.lines[app.editor_cursor_row];
        let before_cursor: String = cur_line.chars().take(app.editor_cursor_col).collect();
        display_width(&before_cursor) as u16
    } else {
        0u16
    };
    app.cursor_screen_pos = Some((
        edit_rect.x + line_num_width + cursor_display_col,
        edit_rect.y + visible_row as u16,
    ));
}

// ── 输入区渲染 (密码模式 vs 正常模式) ──
fn render_input(frame: &mut Frame, app: &mut App, rect: Rect) {
    if app.password_mode {
        // ── 密码模式：专用密码输入提示 ──
        let masked = "*".repeat(app.input.len());  // input 里存的就是 * 号
        let before = &masked[..app.cursor.min(masked.len())];
        let after = &masked[app.cursor.min(masked.len())..];

        let (cursor_span, remaining_after): (Span, &str) = if after.is_empty() {
            (Span::styled(" ", Style::default().fg(Color::Black).bg(theme::DANGER)), "")
        } else {
            let ch = after.chars().next().unwrap();
            (Span::styled(ch.to_string(), Style::default().fg(Color::Black).bg(theme::DANGER)), &after[ch.len_utf8()..])
        };

        let prompt_text = app.password_prompt.clone();
        let input_line = Line::from(vec![
            Span::styled(prompt_text, Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD)),
            Span::styled(before.to_string(), Style::default().fg(theme::WARN)),
            cursor_span,
            Span::styled(remaining_after.to_string(), Style::default().fg(theme::WARN)),
            Span::styled(if app.password_action.starts_with("login") { "  [Esc 清空 | Enter 确认]" } else { "  [Esc 取消 | Enter 确认]" }, Style::default().fg(theme::MUTED)),
        ]);

        frame.render_widget(
            Paragraph::new(input_line)
                .block(Block::new().borders(Borders::NONE)
                    .style(Style::default().bg(Color::Rgb(25, 10, 10)))),
            rect,
        );
        // ★ 记下光标屏幕坐标，draw() 后统一 MoveTo+Hide (IME用)
        // 密码模式下每个字符显示为一个 * (1列宽), 所以用字符数而非显示宽度
        let prompt_len = app.password_prompt.chars().count();
        let cursor_cols = app.input[..app.cursor].chars().count();
        app.cursor_screen_pos = Some((rect.x + (prompt_len + cursor_cols) as u16, rect.y));
    } else if app.history_search.active {
        // ── v4.7: 历史搜索模式 ──
        let prompt = "(history-search) > ";
        let prompt_color = theme::WARN;
        let safe_cursor = if app.input.is_char_boundary(app.cursor) {
            app.cursor
        } else {
            (0..app.cursor).rev().find(|&i| app.input.is_char_boundary(i)).unwrap_or(0)
        };
        let before = &app.input[..safe_cursor];
        let after = &app.input[safe_cursor..];

        let (cursor_span, remaining_after): (Span, &str) = if after.is_empty() {
            (Span::styled(" ", Style::default().fg(Color::Black).bg(theme::WARN)), "")
        } else {
            let ch = after.chars().next().unwrap();
            (Span::styled(ch.to_string(), Style::default().fg(Color::Black).bg(theme::WARN)), &after[ch.len_utf8()..])
        };

        let cursor_char = after.chars().next().map(|c| c.to_string()).unwrap_or_default();
        let full_text = format!("{}{}{}", before, cursor_char, remaining_after);
        let prompt_len = prompt.chars().count();

        let avail_w = rect.width.saturating_sub(2) as usize;
        let cursor_pos = prompt_len + display_width(&app.input[..safe_cursor]);
        let total_w = prompt_len + display_width(&full_text);

        if cursor_pos < app.input_scroll {
            app.input_scroll = cursor_pos.saturating_sub(2);
        } else if cursor_pos >= app.input_scroll + avail_w {
            app.input_scroll = cursor_pos.saturating_sub(avail_w.saturating_sub(2));
        }
        if total_w <= avail_w || app.input.is_empty() {
            app.input_scroll = 0;
        } else {
            if app.input_scroll > total_w.saturating_sub(avail_w.saturating_sub(1)) {
                app.input_scroll = total_w.saturating_sub(avail_w.saturating_sub(1));
            }
        }
        let scroll_x = app.input_scroll;

        // 搜索匹配信息
        let _match_info = if app.history_search.matches.is_empty() {
            Span::styled(" 无匹配", Style::default().fg(theme::DANGER))
        } else {
            Span::styled(
                format!(" {}/{}", app.history_search.current_idx + 1, app.history_search.matches.len()),
                Style::default().fg(theme::ACCENT),
            )
        };

        let input_line = Line::from(vec![
            Span::styled(prompt.to_string(), Style::default().fg(prompt_color).add_modifier(Modifier::BOLD)),
            Span::styled(before.to_string(), Style::default().fg(theme::WHITE)),
            cursor_span,
            Span::styled(remaining_after.to_string(), Style::default().fg(theme::WHITE)),
            _match_info,
            Span::styled("  [Ctrl+R下一个 | Enter确认 | Esc取消]", Style::default().fg(theme::MUTED)),
        ]);

        frame.render_widget(
            Paragraph::new(input_line)
                .block(Block::new().borders(Borders::NONE)
                    .style(Style::default().bg(Color::Rgb(25, 20, 10))))
                .scroll((0, scroll_x as u16)),
            rect,
        );

        let visible_cursor_x = (prompt_len + display_width(&app.input[..safe_cursor])).saturating_sub(scroll_x);
        app.cursor_screen_pos = Some((rect.x + visible_cursor_x as u16, rect.y));
    } else {
        // ── 正常命令模式 ──
        let (prompt, prompt_color) = if app.ai_mode {
            if app.is_admin {
                ("root#dp > ", theme::DANGER)
            } else {
                ("#dp > ", theme::DP_BLUE)
            }
        } else if app.is_admin {
            ("root@ruoo:~# ", theme::DANGER)
        } else {
            ("root@ruoo:~# ", theme::ACCENT)
        };
        // ★ 防御: 确保光标在有效 UTF-8 字符边界上, 防止多字节字符中间切片 panic
        let safe_cursor = if app.input.is_char_boundary(app.cursor) {
            app.cursor
        } else {
            // 光标落在多字节字符中间 → 回退到前一个合法边界
            (0..app.cursor).rev().find(|&i| app.input.is_char_boundary(i)).unwrap_or(0)
        };
        let before = &app.input[..safe_cursor];
        let after = &app.input[safe_cursor..];

        let cursor_bg = if app.is_admin { theme::DANGER } else if app.ai_mode { theme::DP_BLUE } else { theme::ACCENT };
        let cursor_fg = Color::Black;

        let (cursor_span, remaining_after): (Span, &str) = if after.is_empty() {
            (
                Span::styled(" ", Style::default().fg(cursor_fg).bg(cursor_bg)),
                "",
            )
        } else {
            let ch = after.chars().next().unwrap();
            let remaining = &after[ch.len_utf8()..];
            (
                Span::styled(ch.to_string(), Style::default().fg(cursor_fg).bg(cursor_bg)),
                remaining,
            )
        };

        let cursor_char = after.chars().next().map(|c| c.to_string()).unwrap_or_default();
        let full_text = format!("{}{}{}", before, cursor_char, remaining_after);
        let prompt_len = prompt.chars().count();

        // ★ 水平滚动: 光标超出可见区时自动跟焦
        let avail_w = rect.width.saturating_sub(2) as usize;
        let cursor_pos = prompt_len + display_width(&app.input[..safe_cursor]);
        let total_w = prompt_len + display_width(&full_text);

        // 光标离开可视区 → 重新计算 scroll
        if cursor_pos < app.input_scroll {
            // 光标在左边不可见 — 向左滚
            app.input_scroll = cursor_pos.saturating_sub(2);
        } else if cursor_pos >= app.input_scroll + avail_w {
            // 光标在右边不可见 — 向右滚让光标可见
            app.input_scroll = cursor_pos.saturating_sub(avail_w.saturating_sub(2));
        }
        // 边界钳位 + 短文本自动归零
        if total_w <= avail_w || app.input.is_empty() {
            app.input_scroll = 0;
        } else {
            if app.input_scroll > total_w.saturating_sub(avail_w.saturating_sub(1)) {
                app.input_scroll = total_w.saturating_sub(avail_w.saturating_sub(1));
            }
            if app.input_scroll > total_w { app.input_scroll = total_w.saturating_sub(1).max(0); }
        }
        let scroll_x = app.input_scroll;

        let input_line = Line::from(vec![
            Span::styled(prompt.to_string(), Style::default().fg(prompt_color).add_modifier(Modifier::BOLD)),
            Span::styled(before.to_string(), Style::default().fg(theme::WHITE)),
            cursor_span,
            Span::styled(remaining_after.to_string(), Style::default().fg(theme::WHITE)),
        ]);

        frame.render_widget(
            Paragraph::new(input_line)
                .block(Block::new().borders(Borders::NONE)
                    .style(Style::default().bg(Color::Rgb(15, 17, 24))))
                .scroll((0, scroll_x as u16)),
            rect,
        );

        // ★ 记下光标屏幕坐标，draw() 后统一 MoveTo+Hide (IME用)
        let visible_cursor_x = (prompt_len + display_width(&app.input[..safe_cursor])).saturating_sub(scroll_x);
        app.cursor_screen_pos = Some((rect.x + visible_cursor_x as u16, rect.y));
    }
}