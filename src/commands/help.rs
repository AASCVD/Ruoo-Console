// ============================================================
// RUOO-CONSOLE 帮助系统 v10.2 — 修复版
// 
// v10.2 修复 (Zero-Day):
//   - 修复 F9 帮助页被分页截断：移除 help_text_filtered 内部自动分页
//     分页仅由 help_page() 负责，help_text_filtered 始终返回完整内容
//   - 修复双列对齐：预计算全局 col_width，所有分类/批次使用统一列宽
//     不再每个 batch 独立计算，│ 分隔符位置全局一致
// ============================================================

use crate::plugin::PluginCommandDef;

/// 智能构建命令显示字符串，避免 "ipxx ipxx <参数>" 重复
fn smart_cmd_str(cmd: &str, usage: &str) -> String {
    if usage.is_empty() {
        cmd.to_string()
    } else {
        let usage_trimmed = usage.trim_start();
        if usage_trimmed.starts_with(cmd) {
            let after_cmd = &usage_trimmed[cmd.len()..];
            if after_cmd.is_empty() || after_cmd.starts_with(' ') {
                return usage_trimmed.to_string();
            }
        }
        format!("{} {}", cmd, usage)
    }
}

/// 计算单个命令的格式化左列宽度
fn cmd_display_width(name: &str, usage: &str, desc: &str) -> usize {
    format!("[cmd] {}  {}", smart_cmd_str(name, usage), desc).len()
}

/// 预计算全局列宽 — 遍历所有命令找最大宽度
fn compute_global_col_width(
    builtin_items: &[(&str, &str, &str)],
    plugin_cmds: &[&PluginCommandDef],
) -> usize {
    let mut max_w = 24usize;
    for (name, syntax, desc) in builtin_items {
        let w = cmd_display_width(name, syntax, desc);
        if w > max_w { max_w = w; }
    }
    for cmd in plugin_cmds {
        let desc = if cmd.help.is_empty() { &cmd.usage } else { &cmd.help };
        let w = cmd_display_width(&cmd.cmd, &cmd.usage, desc);
        if w > max_w { max_w = w; }
    }
    max_w.min(70) // 上限70防止过宽
}

/// 双列渲染: 将 leaf_buf 中条目按 col_width 一致的列宽格式化
fn flush_leaf_with_width(
    buf: &mut Vec<(String, String)>,
    out: &mut Vec<String>,
    col_width: usize,
) {
    if buf.is_empty() { return; }
    let left_items: Vec<String> = buf.iter()
        .map(|(name, desc)| format!("[cmd] {}  {}", name, desc))
        .collect();

    for (chunk_idx, chunk) in buf.chunks(2).enumerate() {
        let idx = chunk_idx * 2;
        let left_str = &left_items[idx];
        if chunk.len() == 2 {
            let right_str = &left_items[idx + 1];
            out.push(format!("  {:<col_width$} │ {}", left_str, right_str, col_width = col_width));
        } else {
            out.push(format!("  {}", left_str));
        }
    }
    buf.clear();
}

/// 内部: 格式化单个分类的插件命令输出到 out (使用统一 col_width)
fn format_plugin_category_with_width(
    cat_name: &str,
    cmds: &[&PluginCommandDef],
    out: &mut Vec<String>,
    col_width: usize,
) {
    if cmds.is_empty() { return; }
    out.push(String::new());
    let display_name = if cat_name.starts_with("[插件]") {
        cat_name.to_string()
    } else {
        format!("[插件] {}", cat_name)
    };
    out.push(format!("[cat] ══ {} ({}) ══", display_name, cmds.len()));

    let mut groups: Vec<(String, Vec<&PluginCommandDef>)> = Vec::new();
    for cmd in cmds.iter() {
        let base = cmd.cmd.split(|c: char| c == ' ' || c == '/')
            .next().unwrap_or(&cmd.cmd).to_string();
        if let Some((_, g)) = groups.iter_mut().find(|(b, _)| *b == base) {
            g.push(cmd);
        } else {
            groups.push((base, vec![*cmd]));
        }
    }

    let mut leaf_buf: Vec<(String, String)> = Vec::new();

    for (base, group) in &groups {
        if group.len() == 1 {
            let c = group[0];
            let cmd_str = smart_cmd_str(&c.cmd, &c.usage);
            let desc = if c.help.is_empty() { c.usage.clone() } else { c.help.clone() };
            leaf_buf.push((cmd_str, desc));
        } else {
            flush_leaf_with_width(&mut leaf_buf, out, col_width);
            let base_item = group.iter().find(|d| d.cmd == *base);
            if let Some(bi) = base_item {
                let cmd_str = smart_cmd_str(&bi.cmd, &bi.usage);
                let desc = if bi.help.is_empty() { bi.usage.clone() } else { bi.help.clone() };
                out.push(format!("  [cmd] {}  {}", cmd_str, desc));
                for item in group {
                    if item.cmd != *base {
                        let sub = smart_cmd_str(&item.cmd, &item.usage);
                        out.push(format!("    {}", sub));
                    }
                }
            } else {
                for item in group {
                    let cmd_str = smart_cmd_str(&item.cmd, &item.usage);
                    let desc = if item.help.is_empty() { item.usage.clone() } else { item.help.clone() };
                    leaf_buf.push((cmd_str, desc));
                }
            }
        }
    }
    flush_leaf_with_width(&mut leaf_buf, out, col_width);
}

/// 按分类/关键词过滤的帮助文本（None=全部, Some("scan")=匹配分类或命令名）
/// v10.2: 不再内部分页 — 始终返回完整帮助内容
pub fn help_text_filtered(app: &crate::App, category: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let all_cmds = crate::commands::mod_meta::all_commands();

    // 内置命令: 按category分组
    let mut cats: Vec<(&str, Vec<(&str, &str, &str)>)> = Vec::new();
    for (name, syntax, desc, cat) in all_cmds {
        if let Some(filter) = category {
            let fl = filter.to_lowercase();
            let matched = cat.to_lowercase().contains(&fl)
                || name.to_lowercase().contains(&fl)
                || desc.to_lowercase().contains(&fl);
            if !matched { continue; }
        }
        if let Some((_, items)) = cats.iter_mut().find(|(c, _)| *c == *cat) {
            items.push((name, syntax, desc));
        } else {
            cats.push((cat, vec![(name, syntax, desc)]));
        }
    }

    // 插件命令: 按 category 字段分组
    let mut plugin_cats: Vec<(String, Vec<&PluginCommandDef>)> = Vec::new();
    for def in app.cmd_registry.list_all().into_iter() {
        if def.is_builtin { continue; }
        if let Some(filter) = category {
            let fl = filter.to_lowercase();
            let cat_name = if def.category.is_empty() { &def.plugin_name } else { &def.category };
            let matched = cat_name.to_lowercase().contains(&fl)
                || def.cmd.to_lowercase().contains(&fl)
                || def.help.to_lowercase().contains(&fl);
            if !matched { continue; }
        }
        let cat_name = if def.category.is_empty() {
            format!("[插件] {}", def.plugin_name)
        } else {
            format!("[插件] {}", def.category)
        };
        if let Some((_, items)) = plugin_cats.iter_mut().find(|(c, _)| *c == cat_name) {
            items.push(def);
        } else {
            plugin_cats.push((cat_name, vec![def]));
        }
    }

    if cats.is_empty() && plugin_cats.is_empty() { return out; }

    // ── v10.2: 预计算全局列宽 ──
    let mut all_builtin: Vec<(&str, &str, &str)> = Vec::new();
    for (_cat, items) in &cats {
        for item in items {
            all_builtin.push((item.0, item.1, item.2));
        }
    }
    let mut all_plugin: Vec<&PluginCommandDef> = Vec::new();
    for (_cat_name, cmds) in &plugin_cats {
        for cmd in cmds {
            all_plugin.push(cmd);
        }
    }
    let global_col_width = compute_global_col_width(&all_builtin, &all_plugin);

    // 输出内置命令分类
    for (cat, items) in &cats {
        if category.is_none() {
            out.push(String::new());
            out.push(format!("[cat] ══ {} ({}) ══", cat, items.len()));
        }
        let mut groups: Vec<(String, Vec<&(&str, &str, &str)>)> = Vec::new();
        for item in items {
            let base = item.0.split(|c: char| c == ' ' || c == '/').next().unwrap_or(item.0).to_string();
            if let Some((_, g)) = groups.iter_mut().find(|(b, _)| *b == base) {
                g.push(item);
            } else {
                groups.push((base, vec![item]));
            }
        }

        let mut leaf_buf: Vec<(String, String)> = Vec::new();

        for (base, group) in &groups {
            if group.len() == 1 {
                let cmd_str = smart_cmd_str(group[0].0, group[0].1);
                leaf_buf.push((cmd_str, group[0].2.to_string()));
            } else {
                // flush 缓冲 (使用全局 col_width)
                flush_leaf_with_width(&mut leaf_buf, &mut out, global_col_width);

                let base_item = group.iter().find(|i| i.0 == *base);
                if let Some(bi) = base_item {
                    let cmd_str = smart_cmd_str(bi.0, bi.1);
                    out.push(format!("  [cmd] {}  {}", cmd_str, bi.2));
                    for item in group {
                        if item.0 != *base {
                            let sub = smart_cmd_str(item.0, item.1);
                            out.push(format!("    {}", sub));
                        }
                    }
                } else {
                    for item in group {
                        let cmd_str = smart_cmd_str(item.0, item.1);
                        leaf_buf.push((cmd_str, item.2.to_string()));
                    }
                }
            }
        }
        // flush 剩余的
        flush_leaf_with_width(&mut leaf_buf, &mut out, global_col_width);
    }

    // 输出插件命令分类
    for (cat_name, cmds) in &plugin_cats {
        format_plugin_category_with_width(cat_name, cmds, &mut out, global_col_width);
    }

    // v10.2: 不再内部分页 — 始终返回完整内容
    // 分页由 help_page() 负责
    out
}

// ════════════════════════════════════════════════════════
// 分类收集 & 分页 (终端 help <N> 用)
// ════════════════════════════════════════════════════════

fn gather_all_categories(app: &crate::App) -> Vec<(String, usize)> {
    let mut result: Vec<(String, usize)> = Vec::new();

    for (_, _, _, cat) in crate::commands::mod_meta::all_commands() {
        if let Some((_, count)) = result.iter_mut().find(|(c, _)| *c == *cat) {
            *count += 1;
        } else {
            result.push((cat.to_string(), 1));
        }
    }

    for def in app.cmd_registry.list_all().into_iter() {
        if def.is_builtin { continue; }
        let cat_name = if def.category.is_empty() {
            format!("[插件] {}", def.plugin_name)
        } else {
            format!("[插件] {}", def.category)
        };
        if let Some((_, count)) = result.iter_mut().find(|(c, _)| *c == cat_name) {
            *count += 1;
        } else {
            result.push((cat_name, 1));
        }
    }

    result
}

fn compute_total_pages(categories: &[(String, usize)]) -> usize {
    const TARGET_LINES: usize = 25;
    const MIN_CATS: usize = 2;

    let mut pages = 0;
    let mut lines = 0;
    let mut cats_on_page = 0;

    for (_cat, count) in categories {
        let est = 1 + count;
        if cats_on_page > 0 && lines + est > TARGET_LINES && cats_on_page >= MIN_CATS {
            pages += 1;
            lines = 0;
            cats_on_page = 0;
        }
        lines += est;
        cats_on_page += 1;
    }
    if cats_on_page > 0 { pages += 1; }
    pages.max(1).min(20)
}

fn paginate(full: &[String], _categories: &[(String, usize)], page: usize, total: usize) -> Vec<String> {
    if total <= 1 { return full.to_vec(); }

    let mut cat_boundaries: Vec<usize> = Vec::new();
    for (i, line) in full.iter().enumerate() {
        if line.starts_with("[cat]") {
            cat_boundaries.push(i);
        }
    }

    let start_cat = (page - 1) * 3;
    let end_cat = (start_cat + 3).min(cat_boundaries.len());

    if start_cat >= cat_boundaries.len() {
        return full.to_vec();
    }

    let start_line = cat_boundaries[start_cat];
    let end_line = if end_cat < cat_boundaries.len() {
        cat_boundaries[end_cat]
    } else {
        full.len()
    };

    let mut result: Vec<String> = full[start_line..end_line].to_vec();

    result.push(String::new());
    result.push(format!(
        "── 第{}/{}页结束 | help {} 下一页 | help {} 上一页 | F9 帮助全览 ──",
        page, total,
        if page < total { page + 1 } else { 1 },
        if page > 1 { page - 1 } else { total }
    ));

    result
}

/// 返回当前动态总页数
pub fn total_pages(app: &crate::App) -> usize {
    let all_categories = gather_all_categories(app);
    compute_total_pages(&all_categories)
}

/// 分类概览: (分类名, 命令数) — 保持定义顺序  
pub fn category_summary() -> Vec<(&'static str, usize)> {
    let mut result: Vec<(&str, usize)> = Vec::new();
    for (_, _, _, cat) in crate::commands::mod_meta::all_commands() {
        if let Some((_, count)) = result.iter_mut().find(|(c, _)| *c == *cat) {
            *count += 1;
        } else {
            result.push((cat, 1));
        }
    }
    result
}

/// 命令总数 (仅内置)
pub fn total_commands() -> usize {
    crate::commands::mod_meta::all_commands().len()
}

/// 列出所有分类名 (内置+插件)
pub fn list_categories() -> Vec<String> {
    let mut cats: Vec<String> = Vec::new();
    for (_, _, _, cat) in crate::commands::mod_meta::all_commands() {
        if !cats.contains(&cat.to_string()) {
            cats.push(cat.to_string());
        }
    }
    cats
}

/// 返回特定分页的帮助内容 (供 help <N> 命令使用)
pub fn help_page(page: usize, app: &crate::App) -> Vec<String> {
    let full = help_text_filtered(app, None);
    let all_categories = gather_all_categories(app);
    let total = compute_total_pages(&all_categories);
    let p = page.min(total).max(1);
    paginate(&full, &all_categories, p, total)
}

/// v10.1: 如果帮助页正在显示，刷新其内容
/// 应在 plugin load/unload/reload 后调用
pub fn help_refresh_if_open(app: &mut crate::App) {
    if app.help_mode {
        app.help_page_content = help_text_filtered(app, None);
    }
}
