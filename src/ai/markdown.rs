// === Markdown 剥离工具 ===
// 硬防线: 剥离 AI 回复中的 Markdown 格式

/// 硬防线: 剥离 Markdown 格式 — AI 回复经此处理后绝无 ** ` ``` ### 等
pub fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim();
        // 跳过纯分隔线 (--- *** === ~~~) — _ 已从检测中移除，因误伤变量名/路径
        if trimmed.chars().all(|c| c == '-' || c == '*' || c == '=' || c == '~')
            && trimmed.len() >= 3 {
            continue;
        }
        let mut l = line.to_string();
        // 代码块标记 → 移除
        if l.trim() == "```" || l.trim().starts_with("```") { continue; }
        // 内联代码 `code` → code
        l = strip_inline_markers(&l, '`');
        // **bold** → bold
        l = strip_inline_markers(&l, '*');
        // ~~strikethrough~~ → strikethrough
        l = strip_inline_markers(&l, '~');
        // ### heading → heading
        l = strip_heading(&l);
        // > quote → quote
        l = strip_quote(&l);
        // [text](url) → text
        l = strip_links(&l);
        // 移除行首 - 列表标记 (保留缩进)
        l = strip_list_marker(&l);
        // 清理多余空白
        if !l.trim().is_empty() {
            out.push_str(&l);
            out.push('\n');
        } else {
            out.push('\n');
        }
    }
    // 清理连续的3个以上空行
    while out.contains("\n\n\n\n") { out = out.replace("\n\n\n\n", "\n\n\n"); }
    out.trim().to_string()
}

fn strip_inline_markers(line: &str, marker: char) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i+1] == marker {
            result.push(chars[i+1]);
            i += 2;
            continue;
        }
        if chars[i] == marker {
            let mut count = 1;
            while i + count < chars.len() && chars[i+count] == marker {
                count += 1;
            }
            let saved_len = result.len();
            let mut found = false;
            let mut j = i + count;
            while j + count <= chars.len() {
                if chars[j..j+count].iter().all(|&c| c == marker) {
                    found = true;
                    i = j + count;
                    break;
                }
                if chars[j] == '\\' && j + 1 < chars.len() && chars[j+1] == marker {
                    result.push(chars[j+1]);
                    j += 2;
                    continue;
                }
                result.push(chars[j]);
                j += 1;
            }
            if !found {
                result.truncate(saved_len);
                for _ in 0..count { result.push(marker); }
                i += count;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn strip_heading(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("#### ") {
        format!("  {}", rest)
    } else if let Some(rest) = trimmed.strip_prefix("### ") {
        format!("  {}", rest)
    } else if let Some(rest) = trimmed.strip_prefix("## ") {
        format!(" {}", rest)
    } else if let Some(rest) = trimmed.strip_prefix("# ") {
        rest.to_string()
    } else {
        line.to_string()
    }
}

fn strip_quote(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("> ") {
        format!("  {}", rest)
    } else if let Some(rest) = trimmed.strip_prefix(">> ") {
        format!("    {}", rest)
    } else if let Some(rest) = trimmed.strip_prefix(">") {
        rest.to_string()
    } else {
        line.to_string()
    }
}

fn strip_links(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut text_end = None;
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == ']' && (j+1 >= chars.len() || chars[j+1] == '(') {
                    text_end = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(end) = text_end {
                let text: String = chars[i+1..end].iter().collect();
                let mut k = end + 2;
                while k < chars.len() && chars[k] != ')' {
                    k += 1;
                }
                result.push_str(&text);
                i = if k < chars.len() { k + 1 } else { end + 2 };
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn strip_list_marker(line: &str) -> String {
    let trimmed = line.trim_start();
    let ws = &line[..line.len() - trimmed.len()];
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        format!("{}  {}", ws, &trimmed[2..])
    } else if let Some(dot) = trimmed.find(". ") {
        if trimmed[..dot].chars().all(|c| c.is_ascii_digit()) {
            format!("{}  {}", ws, &trimmed[dot+2..])
        } else {
            line.to_string()
        }
    } else {
        line.to_string()
    }
}
