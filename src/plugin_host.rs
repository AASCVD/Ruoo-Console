// ============================================================
// RUOO-CONSOLE v7.0 — 插件子进程执行宿主 (Plugin Host)
//
// 架构:
//   ruoo-console.exe --plugin-exec <dll_path> <command> [--timeout <ms>]
//   独立进程加载 DLL → 调用 ruoo_plugin_exec → 输出 JSON → 退出
//   任何崩溃 (ACCESS_VIOLATION/SEGFAULT/panic) 仅杀死此子进程
//
// Zero-Day | v7.0: 从 VEH+setjmp 升级到真实进程隔离
// ============================================================

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::process;
use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════
// 子进程输出结构
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHostResult {
    /// "ok" | "error" | "timeout" (timeout由父进程侧判断)
    pub status: String,
    /// 成功时为插件返回值, 失败时为错误消息
    pub message: String,
    /// 插件版本 (来自 ruoo_plugin_version)
    pub version: Option<String>,
    /// 执行耗时 (毫秒)
    pub elapsed_ms: u64,
}

impl PluginHostResult {
    pub fn ok(msg: String, version: Option<String>, elapsed_ms: u64) -> Self {
        Self { status: "ok".into(), message: msg, version, elapsed_ms }
    }

    pub fn error(msg: String) -> Self {
        Self { status: "error".into(), message: msg, version: None, elapsed_ms: 0 }
    }
}

// ════════════════════════════════════════════════════════
// 子进程入口 — 完全不初始化 TUI, 直接执行插件
// ════════════════════════════════════════════════════════

pub fn run_plugin_exec_mode(args: &[String]) -> ! {
    // 参数解析: --plugin-exec <dll> <cmd> [--timeout <ms>]
    if args.len() < 4 {
        let result = PluginHostResult::error(
            "USAGE: ruoo-console.exe --plugin-exec <dll_path> <command> [--timeout <ms>]".into()
        );
        print_result_and_exit(&result, 1);
    }

    let dll_path = &args[2];
    let command = &args[3];

    // 解析可选超时
    let _timeout_ms: u64 = if args.len() >= 6 && args[4] == "--timeout" {
        args[5].parse().unwrap_or(30_000)
    } else {
        30_000
    };

    let start = std::time::Instant::now();

    // ── 步骤 1: 加载 DLL ──
    let lib = match unsafe { libloading::Library::new(dll_path) } {
        Ok(l) => l,
        Err(e) => {
            let result = PluginHostResult::error(format!("DLL加载失败: {}", e));
            print_result_and_exit(&result, 2);
        }
    };

    // ── 步骤 2: 读取版本 ──
    let version: Option<String> = unsafe {
        match lib.get::<unsafe extern "C" fn() -> *const c_char>(b"ruoo_plugin_version") {
            Ok(func) => {
                let ptr = func();
                if ptr.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
                }
            }
            Err(_) => None,
        }
    };

    // ── 步骤 3: 调用 ruoo_plugin_init ──
    let init_code: i32 = unsafe {
        match lib.get::<unsafe extern "C" fn() -> i32>(b"ruoo_plugin_init") {
            Ok(func) => func(),
            Err(e) => {
                let result = PluginHostResult::error(
                    format!("缺少 ruoo_plugin_init 导出: {}", e)
                );
                print_result_and_exit(&result, 3);
            }
        }
    };

    if init_code != 0 {
        let result = PluginHostResult::error(
            format!("ruoo_plugin_init 返回错误码: {}", init_code)
        );
        print_result_and_exit(&result, 4);
    }

    // ── 步骤 4: 调用 ruoo_plugin_exec ──
    let exec_result: Result<String, String> = unsafe {
        let exec: libloading::Symbol<unsafe extern "C" fn(*const c_char) -> *mut c_char> = match lib
            .get(b"ruoo_plugin_exec")
        {
            Ok(f) => f,
            Err(e) => {
                let result = PluginHostResult::error(
                    format!("缺少 ruoo_plugin_exec 导出: {}", e)
                );
                print_result_and_exit(&result, 5);
            }
        };

        let cmd_cstr = match CString::new(command.as_str()) {
            Ok(c) => c,
            Err(e) => {
                let result = PluginHostResult::error(
                    format!("命令包含空字节: {}", e)
                );
                print_result_and_exit(&result, 6);
            }
        };

        let ptr = exec(cmd_cstr.as_ptr());

        if ptr.is_null() {
            Err("ruoo_plugin_exec 返回 NULL 指针".into())
        } else {
            let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();

            // 释放结果内存
            if let Ok(free_fn) =
                lib.get::<unsafe extern "C" fn(*mut c_char)>(b"ruoo_plugin_free_result")
            {
                free_fn(ptr);
            }

            Ok(s)
        }
    };

    // ── 步骤 5: 调用 ruoo_plugin_shutdown (尽力) ──
    unsafe {
        if let Ok(shutdown) = lib.get::<unsafe extern "C" fn()>(b"ruoo_plugin_shutdown") {
            shutdown();
        }
    }

    drop(lib);

    let elapsed = start.elapsed().as_millis() as u64;

    match exec_result {
        Ok(msg) => {
            let result = PluginHostResult::ok(msg, version, elapsed);
            print_result_and_exit(&result, 0);
        }
        Err(msg) => {
            let result = PluginHostResult::error(msg);
            print_result_and_exit(&result, 7);
        }
    }
}

// ════════════════════════════════════════════════════════
// 输出 JSON 到 stdout 并退出
// ════════════════════════════════════════════════════════

fn print_result_and_exit(result: &PluginHostResult, exit_code: i32) -> ! {
    let json = serde_json::to_string(result).unwrap_or_else(|_| {
        r#"{"status":"error","message":"JSON序列化失败","version":null,"elapsed_ms":0}"#.to_string()
    });
    println!("{}", json);

    // 确保 stdout 被刷新 (父进程读取用)
    use std::io::Write;
    let _ = std::io::stdout().flush();

    process::exit(exit_code);
}

// ════════════════════════════════════════════════════════
// 测试
// ════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_serialization() {
        let r = PluginHostResult::ok("hello".into(), Some("1.0.0".into()), 42);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"message\":\"hello\""));
    }

    #[test]
    fn test_error_result() {
        let r = PluginHostResult::error("test error".into());
        assert_eq!(r.status, "error");
    }
}
