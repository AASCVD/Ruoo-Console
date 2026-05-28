// ============================================================
// RUOO-CONSOLE v4.2 — 剪贴板集成
// Ctrl+Shift+C: 复制最后一条命令输出
// Ctrl+Shift+V: 从剪贴板粘贴到输入
// ============================================================

use arboard::Clipboard;

/// 复制文本到剪贴板。优先 arboard，失败回退 powershell clip / wsl clip.exe
///
/// ★ BUG-13 修复: WSL 中无 powershell 时, 回退尝试 wsl.exe clip.exe 或给出明确指引
pub fn copy_to_clipboard(text: &str) -> Result<String, String> {
    match Clipboard::new() {
        Ok(mut clipboard) => {
            clipboard
                .set_text(text)
                .map_err(|e| format!("arboard写入失败: {}", e))?;
            Ok(format!("[+] 已复制 {} 字符到剪贴板", text.len()))
        }
        Err(arboard_err) => {
            // arboard 不可用 (WSL/SSH/wayland无合成器)
            // 先尝试 powershell (Windows)
            let ps_result = std::process::Command::new("powershell")
                .args(["-Command", "Set-Clipboard -Value $input"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .and_then(|mut child| {
                    {
                        use std::io::Write;
                        if let Some(ref mut stdin) = child.stdin {
                            let _ = stdin.write_all(text.as_bytes());
                        }
                    }
                    child.wait()
                });

            match ps_result {
                Ok(s) if s.success() => {
                    return Ok(format!("[+] 已复制 {} 字符到剪贴板 (powershell)", text.len()));
                }
                _ => {
                    // powershell 不可用 — 可能是 WSL, 尝试 clip.exe
                    let clip_result = std::process::Command::new("clip.exe")
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .and_then(|mut child| {
                            use std::io::Write;
                            if let Some(ref mut stdin) = child.stdin {
                                let _ = stdin.write_all(text.as_bytes());
                            }
                            child.wait()
                        });

                    match clip_result {
                        Ok(s) if s.success() => {
                            return Ok(format!("[+] 已复制 {} 字符到剪贴板 (clip.exe/WSL)", text.len()));
                        }
                        _ => {
                            // 全部回退失败 — 给出平台指引
                            let hint = if cfg!(windows) {
                                "arboard初始化失败. 尝试以管理员运行或检查显示服务器."
                            } else if cfg!(target_os = "linux") {
                                "无法访问剪贴板. Linux: 安装 xclip 或 wl-clipboard. \
                                 WSL: 确保 clip.exe 可用 (sudo apt install wsl-clipboard 或使用 Windows 版)."
                            } else {
                                "剪贴板不可用 — 此平台不支持."
                            };
                            Err(format!("剪贴板不可用: arboard={}, powershell=失败, clip.exe=失败. {}",
                                arboard_err, hint))
                        }
                    }
                }
            }
        }
    }
}

/// 从剪贴板读取文本
///
/// ★ BUG-13 修复: 增加 clip.exe 回退 (WSL) 和更清晰的错误指引
pub fn paste_from_clipboard() -> Result<String, String> {
    match Clipboard::new() {
        Ok(mut clipboard) => {
            clipboard
                .get_text()
                .map_err(|e| format!("剪贴板读取失败: {}", e))
        }
        Err(_) => {
            // 回退1: powershell Get-Clipboard
            let ps_output = std::process::Command::new("powershell")
                .args(["-Command", "Get-Clipboard"])
                .output();

            if let Ok(output) = &ps_output {
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout)
                        .trim_end_matches(|c| c == '\r' || c == '\n')
                        .to_string();
                    return Ok(text);
                }
            }

            // 回退2: clip.exe / paste (WSL 互操作)
            // WSL 中 clip.exe 只能写不能读，用 powershell.exe (Windows侧)
            let ps_exe_output = std::process::Command::new("powershell.exe")
                .args(["-Command", "Get-Clipboard"])
                .output();

            match ps_exe_output {
                Ok(output) if output.status.success() => {
                    let text = String::from_utf8_lossy(&output.stdout)
                        .trim_end_matches(|c| c == '\r' || c == '\n')
                        .to_string();
                    Ok(text)
                }
                _ => {
                    let hint = if cfg!(target_os = "linux") {
                        "WSL: 尝试 powershell.exe Get-Clipboard. \
                         纯Linux: 安装 xclip 或 wl-clipboard."
                    } else {
                        "剪贴板不可用."
                    };
                    Err(format!("剪贴板读取失败: arboard + powershell + powershell.exe 均失败. {}", hint))
                }
            }
        }
    }
}

/// 提取最后一条命令的输出 (不含命令回显行)
pub fn extract_last_command_output(output: &[String]) -> String {
    // 从后往前找到最近的 "$ " 或 "#dp " 前缀行 (命令回显)
    // 该行之后到末尾的所有行就是最后一条命令的输出
    let mut cmd_idx: Option<usize> = None;

    for (i, line) in output.iter().enumerate().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("$ ") || trimmed.starts_with("#dp ") {
            cmd_idx = Some(i);
            break;
        }
    }

    match cmd_idx {
        Some(idx) => {
            let result_lines: Vec<&str> = output[idx + 1..]
                .iter()
                .map(|s| s.as_str())
                .collect();
            result_lines.join("\n")
        }
        None => {
            // 没有命令回显，返回最近20行
            let start = output.len().saturating_sub(20);
            output[start..].join("\n")
        }
    }
}
