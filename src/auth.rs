// ============================================================
// Ruoo Arsenal — 登录认证系统
// 首次运行: 创建主密码
// 后续运行: 输入密码 → 挂载 Vault
// ============================================================

use crate::vault::Vault;
use std::path::PathBuf;

/// 密码强度
#[derive(Debug, PartialEq)]
pub enum Strength {
    VeryWeak,
    Weak,
    Medium,
    Strong,
    VeryStrong,
}

/// 检测全数字序列 (如 12345678, 987654321)
fn is_sequential_digits(s: &str) -> bool {
    let digits: Vec<i8> = s.chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as i8))
        .collect();
    if digits.len() < 4 { return false; }
    let ascending = digits.windows(2).all(|w| w[1] == w[0] + 1);
    let descending = digits.windows(2).all(|w| w[1] == w[0] - 1);
    ascending || descending
}

/// 检测密码强度
pub fn password_strength(pw: &str) -> Strength {
    let len = pw.len();
    let has_upper = pw.chars().any(|c| c.is_uppercase());
    let has_lower = pw.chars().any(|c| c.is_lowercase());
    let has_digit = pw.chars().any(|c| c.is_ascii_digit());
    let has_special = pw.chars().any(|c| !c.is_alphanumeric());
    let unique: usize = {
        let mut chars: Vec<char> = pw.chars().collect();
        chars.sort();
        chars.dedup();
        chars.len()
    };

    if len < 8 { return Strength::VeryWeak; }

    let mut score = 0u32;
    score += (len.min(32) * 3) as u32;
    if has_upper { score += 10; }
    if has_lower { score += 10; }
    if has_digit { score += 10; }
    if has_special { score += 15; }
    score += (unique.min(20) * 2) as u32;

    // ★ BUG-7 修复: 扩展常见弱密码库 — 基于 rockyou 高频密码
    let lower = pw.to_lowercase();
    let common = [
        // top-10 高频
        "password", "12345678", "123456789", "1234567890",
        "qwerty123", "qwerty1", "admin123", "letmein", "monkey",
        "dragon", "master", "hello", "iloveyou",
        // 常见弱模式
        "abc123", "password1", "password123", "admin", "administrator",
        "welcome", "sunshine", "princess", "football", "baseball",
        "qwerty", "qwertyuiop", "1q2w3e4r", "zaq12wsx",
        "login", "starwars", "trustno1", "batman", "superman",
        // 序列/重复
        "11111111", "00000000", "asdfghjk",
        // 中国常见
        "woaini", "5201314", "1314520",
        // 键盘模式
        "1qaz2wsx", "qazwsxedc", "qwertyuiop",
        "zxcvbnm", "mnbvcxz",
    ];
    // 精确匹配和包含检测
    if common.iter().any(|c| lower == *c || lower.contains(c)) {
        score = score.saturating_sub(50);
    }

    // 额外检测: 全数字简单序列
    let all_digits: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
    if all_digits.len() == lower.len() && lower.len() >= 6 {
        // 检查是否递增/递减序列
        if is_sequential_digits(&lower) {
            score = score.saturating_sub(30);
        }
    }

    match score {
        0..=25  => Strength::VeryWeak,
        26..=40 => Strength::Weak,
        41..=60 => Strength::Medium,
        61..=80 => Strength::Strong,
        _       => Strength::VeryStrong,
    }
}

/// 登录流程 (纯文本模式，供 TUI 调用)
/// 返回 (Vault, is_first_time, messages)
#[allow(dead_code)]
pub fn login_flow(base_dir: &PathBuf) -> Result<(Vault, bool, Vec<String>), String> {
    let vault_path = base_dir.join("vault.rdb");
    let mut messages = Vec::new();

    if !vault_path.exists() {
        // ═══ 首次创建 ═══
        messages.push("🔐 首次运行 — 创建主密码".into());
        messages.push("[*] 密码要求: ≥12字符, 含大小写+数字+特殊字符".into());
        messages.push("[!] 警告: 密码丢失后数据无法恢复!".into());

        let password = read_password("请设置主密码: ")?;
        let strength = password_strength(&password);

        messages.push(format!("[*] 密码强度: {:?}", strength));

        if matches!(strength, Strength::VeryWeak | Strength::Weak) {
            return Err("密码太弱！需要 ≥12字符, 含大小写+数字+特殊字符".into());
        }

        let confirm = read_password("请再次输入确认: ")?;
        if password != confirm {
            return Err("两次密码不匹配！".into());
        }

        let vault = Vault::create(base_dir, &password)?;
        messages.push("[+] Vault 创建成功 — 已加密保存".into());
        messages.push("[*] 备份提示: 定期备份 ~/.ruoo/vault.rdb 和 vault.salt".into());

        Ok((vault, true, messages))
    } else {
        // ═══ 登录 ═══
        let password = read_password("🔑 输入主密码: ")?;
        let vault = Vault::mount(base_dir, &password)?;
        messages.push("[+] Vault 已挂载 — 安全通道就绪".into());

        Ok((vault, false, messages))
    }
}

/// 重新解锁流程 (非TUI模式回退，TUI请用 main::do_unlock)
#[allow(dead_code)]
pub fn unlock_flow(vault: &Vault) -> Result<Vec<String>, String> {
    let password = read_password("🔑 输入主密码解锁: ")?;
    vault.remount(&password)?;
    Ok(vec!["[+] Vault 已解锁".into()])
}

/// 修改密码流程 (非TUI模式回退，TUI请用 main::do_change_password)
#[allow(dead_code)]
pub fn change_password_flow(vault: &Vault) -> Result<Vec<String>, String> {
    let old = read_password("当前密码: ")?;
    let new = read_password("新密码: ")?;
    let confirm = read_password("确认新密码: ")?;

    if new != confirm {
        return Err("两次新密码不匹配！".into());
    }

    let strength = password_strength(&new);
    if matches!(strength, Strength::VeryWeak | Strength::Weak) {
        return Err("新密码太弱！".into());
    }

    vault.change_password(&old, &new)?;
    Ok(vec!["[+] 密码已更新".into()])
}

/// 跨平台密码读取
fn read_password(prompt: &str) -> Result<String, String> {
    match rpassword::prompt_password(prompt) {
        Ok(p) => {
            // 清除 rpassword 提示行残留 (防止 TUI 重绘后残留)
            eprint!("\r\x1b[2K");
            if p.is_empty() {
                Err("密码不能为空".into())
            } else {
                Ok(p)
            }
        }
        Err(e) => {
            // rpassword 在某些终端不可用，回退到标准输入
            eprint!("\r\x1b[2K"); // 清除残留
            eprintln!("[!] 隐藏输入不可用 ({})，使用标准输入:", e);
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| format!("读取失败: {}", e))?;
            let p = input.trim().to_string();
            if p.is_empty() {
                Err("密码不能为空".into())
            } else {
                Ok(p)
            }
        }
    }
}
