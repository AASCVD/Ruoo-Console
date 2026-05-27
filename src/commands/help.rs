// ============================================================
// RUOO-ARSENAL 帮助系统 v10.0 — 插件按分类分组 + 动态分页
// 
// v10.0 变更:
//   - 插件命令不再全部挤在"外部插件"下，按 category 字段(来自manifest)分组
//   - 插件未指定 category 时使用 plugin_name 作为分类名
//   - 分页完全动态计算: 新插件/新分类自动纳入分页，无需手动修改页映射
//   - 内置命令保持定义顺序(非字母序)
// ============================================================

use crate::plugin::PluginCommandDef;

/// 内部: 格式化单个分类的插件命令输出到 out
fn format_plugin_category(
    cat_name: &str,
    cmds: &[&PluginCommandDef],
    out: &mut Vec<String>,
) {
    if cmds.is_empty() { return; }
    out.push(String::new());
    // 给插件分类标题统一加 [插件] 前缀，让用户容易区分内置命令和插件命令
    let display_name = if cat_name.starts_with("[插件]") {
        cat_name.to_string()
    } else {
        format!("[插件] {}", cat_name)
    };
    out.push(format!("[cat] ══ {} ({}) ══", display_name, cmds.len()));

    // 按base名分组
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
    let flush = |buf: &mut Vec<(String, String)>, out: &mut Vec<String>| {
        for chunk in buf.chunks(2) {
            let left = &chunk[0];
            let left_str = format!("[cmd] {}  {}", left.0, left.1);
            if chunk.len() == 2 {
                let right = &chunk[1];
                let right_str = format!("[cmd] {}  {}", right.0, right.1);
                out.push(format!("  {:<52}│ {}", left_str, right_str));
            } else {
                out.push(format!("  {}", left_str));
            }
        }
        buf.clear();
    };

    for (base, group) in &groups {
        if group.len() == 1 {
            let c = group[0];
            let cmd_str = if c.usage.is_empty() { c.cmd.clone() }
                else { format!("{} {}", c.cmd, c.usage) };
            let desc = if c.help.is_empty() { c.usage.clone() } else { c.help.clone() };
            leaf_buf.push((cmd_str, desc));
        } else {
            flush(&mut leaf_buf, out);
            let base_item = group.iter().find(|d| d.cmd == *base);
            if let Some(bi) = base_item {
                let cmd_str = if bi.usage.is_empty() { bi.cmd.clone() }
                    else { format!("{} {}", bi.cmd, bi.usage) };
                let desc = if bi.help.is_empty() { bi.usage.clone() } else { bi.help.clone() };
                out.push(format!("  [cmd] {}  {}", cmd_str, desc));
                for item in group {
                    if item.cmd != *base {
                        let sub = if item.usage.is_empty() { item.cmd.clone() }
                            else { format!("{} {}", item.cmd, item.usage) };
                        out.push(format!("    {}", sub));
                    }
                }
            } else {
                for item in group {
                    let cmd_str = if item.usage.is_empty() { item.cmd.clone() }
                        else { format!("{} {}", item.cmd, item.usage) };
                    let desc = if item.help.is_empty() { item.usage.clone() } else { item.help.clone() };
                    leaf_buf.push((cmd_str, desc));
                }
            }
        }
    }
    flush(&mut leaf_buf, out);
}

/// 按分类/关键词过滤的帮助文本（None=全部, Some("scan")=匹配分类或命令名）
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

    // 插件命令: 按 category 字段分组 (consuming iteration)
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
        let flush = |buf: &mut Vec<(String, String)>, out: &mut Vec<String>| {
            for chunk in buf.chunks(2) {
                let left = &chunk[0];
                let left_str = format!("[cmd] {}  {}", left.0, left.1);
                if chunk.len() == 2 {
                    let right = &chunk[1];
                    let right_str = format!("[cmd] {}  {}", right.0, right.1);
                    out.push(format!("  {:<52}│ {}", left_str, right_str));
                } else {
                    out.push(format!("  {}", left_str));
                }
            }
            buf.clear();
        };

        for (base, group) in &groups {
            if group.len() == 1 {
                let cmd_str = if group[0].1.is_empty() { group[0].0.to_string() } 
                    else { format!("{} {}", group[0].0, group[0].1) };
                leaf_buf.push((cmd_str, group[0].2.to_string()));
            } else {
                flush(&mut leaf_buf, &mut out);
                let base_item = group.iter().find(|i| i.0 == *base);
                if let Some(bi) = base_item {
                    let cmd_str = if bi.1.is_empty() { bi.0.to_string() } else { format!("{} {}", bi.0, bi.1) };
                    out.push(format!("  [cmd] {}  {}", cmd_str, bi.2));
                    for item in group {
                        if item.0 != *base {
                            let sub = if item.1.is_empty() { item.0.to_string() } else { format!("{} {}", item.0, item.1) };
                            out.push(format!("    {}", sub));
                        }
                    }
                } else {
                    for item in group {
                        let cmd_str = if item.1.is_empty() { item.0.to_string() }
                            else { format!("{} {}", item.0, item.1) };
                        leaf_buf.push((cmd_str, item.2.to_string()));
                    }
                }
            }
        }
        flush(&mut leaf_buf, &mut out);
    }

    // 输出插件命令分类
    if category.is_none() || !plugin_cats.is_empty() {
        for (cat_name, cmds) in &plugin_cats {
            format_plugin_category(cat_name, cmds, &mut out);
        }
    }

    out
}

/// 列出所有分类名 — 保持定义顺序 (仅内置)
pub fn list_categories() -> Vec<&'static str> {
    let mut cats: Vec<&str> = Vec::new();
    for (_, _, _, c) in crate::commands::mod_meta::all_commands() {
        if !cats.contains(c) {
            cats.push(c);
        }
    }
    cats
}

/// 获取当前所有分类信息 (内置+插件), 返回 (分类名, 命令数, is_plugin)
fn gather_all_categories(app: &crate::App) -> Vec<(String, usize, bool)> {
    let mut result: Vec<(String, usize, bool)> = Vec::new();

    // 内置分类 (保持定义顺序)
    for (_, _, _, cat) in crate::commands::mod_meta::all_commands() {
        if let Some((_, count, _)) = result.iter_mut().find(|(c, _, _)| c == cat) {
            *count += 1;
        } else {
            result.push((cat.to_string(), 1, false));
        }
    }

    // 插件分类 (consuming iteration)
    for def in app.cmd_registry.list_all().into_iter() {
        if def.is_builtin { continue; }
        let cat_name = if def.category.is_empty() {
            format!("[插件] {}", def.plugin_name)
        } else {
            format!("[插件] {}", def.category)
        };
        if let Some((_, count, _)) = result.iter_mut().find(|(c, _, _)| *c == cat_name) {
            *count += 1;
        } else {
            result.push((cat_name, 1, true));
        }
    }

    result
}

/// v10.0: 动态分页help — 根据实际内容自动计算页数
/// 
/// 策略:
///   1. 收集所有分类(内置+插件)及其命令数
///   2. 每个分类预估行数 = 1(头) + 命令数
///   3. 每页目标行数 ≈ 25 行
///   4. 贪心分配: 当前页行数超过目标则换页
///   5. 最少1页, 最多20页(防止异常)
pub fn help_page(page: usize, app: &crate::App) -> Vec<String> {
    let all_categories = gather_all_categories(app);

    if all_categories.is_empty() {
        return vec![
            String::new(),
            "══ RUOO-ARSENAL 帮助 — 无可用命令 ══".into(),
        ];
    }

    // 贪心分页
    const TARGET_LINES_PER_PAGE: usize = 25;
    const MIN_CATS_PER_PAGE: usize = 2;

    let mut pages: Vec<Vec<(String, usize, bool)>> = Vec::new();
    let mut current_page: Vec<(String, usize, bool)> = Vec::new();
    let mut current_lines: usize = 0;

    for cat in &all_categories {
        let est_lines = 1 + cat.1; // 分类头 + 命令行

        if !current_page.is_empty()
            && current_lines + est_lines > TARGET_LINES_PER_PAGE
            && current_page.len() >= MIN_CATS_PER_PAGE
        {
            pages.push(std::mem::take(&mut current_page));
            current_lines = 0;
        }
        current_lines += est_lines;
        current_page.push(cat.clone());
    }
    if !current_page.is_empty() {
        pages.push(current_page);
    }

    let total_pages = pages.len().max(1).min(20);

    if page < 1 || page > total_pages {
        return vec![format!(
            "[!] 无效页码: {} — 当前共 {} 页 (help 1 ~ help {})",
            page, total_pages, total_pages
        )];
    }

    let page_data = &pages[page - 1];

    // 生成页面标题
    let cat_names: Vec<&str> = page_data.iter().map(|(n, _, _)| n.as_str()).collect();
    let title_summary = if cat_names.len() <= 3 {
        cat_names.join(" + ")
    } else {
        format!("{} + {} …共{}个分类", cat_names[0], cat_names[1], cat_names.len())
    };
    let total_cmds: usize = page_data.iter().map(|(_, c, _)| c).sum();

    let mut out: Vec<String> = Vec::new();
    out.push(String::new());
    out.push(format!(
        "══ RUOO-ARSENAL 帮助 — 第{}/{}页: {} ({}命令) ══",
        page, total_pages, title_summary, total_cmds
    ));
    out.push(format!(
        "[*] 切换分页: help 1 ~ help {} | 分类过滤: help <分类名> | 全览: F9",
        total_pages
    ));
    out.push(String::new());

    // 输出每个分类
    let all_cmds = crate::commands::mod_meta::all_commands();

    for (cat_name, _count, is_plugin) in page_data {
        out.push(String::new());
        out.push(format!("[cat] ══ {} ══", cat_name));

        if *is_plugin {
            // 插件分类
            for def in app.cmd_registry.list_all().into_iter() {
                if def.is_builtin { continue; }
                let dc = if def.category.is_empty() { &def.plugin_name } else { &def.category };
                if format!("[插件] {}", dc) != *cat_name { continue; }
                let cmd_str = if def.usage.is_empty() {
                    def.cmd.clone()
                } else {
                    format!("{} {}", def.cmd, def.usage)
                };
                let desc = if def.help.is_empty() { &def.usage } else { &def.help };
                out.push(format!("  [cmd] {}  {}", cmd_str, desc));
            }
        } else {
            // 内置分类
            for (name, syntax, desc, cat) in all_cmds {
                if *cat == cat_name {
                    let cmd_str = if syntax.is_empty() {
                        name.to_string()
                    } else {
                        format!("{} {}", name, syntax)
                    };
                    out.push(format!("  [cmd] {}  {}", cmd_str, desc));
                }
            }
        }
    }

    out.push(String::new());
    out.push(format!(
        "── 第{}/{}页结束 | help {} 下一页 | help {} 上一页 | F9 帮助全览 ──",
        page, total_pages,
        if page < total_pages { page + 1 } else { 1 },
        if page > 1 { page - 1 } else { total_pages }
    ));

    out
}

/// 返回当前动态总页数
pub fn total_pages(app: &crate::App) -> usize {
    let all_categories = gather_all_categories(app);
    const TARGET_LINES_PER_PAGE: usize = 25;
    const MIN_CATS_PER_PAGE: usize = 2;

    let mut page_count: usize = 0;
    let mut current_lines: usize = 0;
    let mut cats_on_page: usize = 0;

    for cat in &all_categories {
        let est_lines = 1 + cat.1;
        if cats_on_page > 0
            && current_lines + est_lines > TARGET_LINES_PER_PAGE
            && cats_on_page >= MIN_CATS_PER_PAGE
        {
            page_count += 1;
            current_lines = 0;
            cats_on_page = 0;
        }
        current_lines += est_lines;
        cats_on_page += 1;
    }
    if cats_on_page > 0 { page_count += 1; }

    page_count.max(1).min(20)
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
