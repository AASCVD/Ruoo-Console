RUOO-CONSOLE 插件开发完整指南 v10.0
Zero-Day 编制 — 2026.05.31 (v10.0 )


目录
────
  1. 架构概览 (完整组件图)/插件命令名规范
  2. 快速开始 (5分钟 — 已验证可编译)
  3. C ABI 接口完整规范 (9个导出函数)
  4. 命令注册系统 (ruoo_plugin_commands — v8.0 JSON格式)
  5. AI工具注册 (GLOBAL_AI_TOOLS + 自动同步)
  6. 插件清单 Manifest (JSON/TOML — 18字段)
  7. Rust 完整模板 (含进度条+消息+保险库)
  8. C 完整模板 (含进度条+消息+保险库)
  9. 插件保险库 Vault (AES-256-GCM + 7-pass安全擦除)
  10. 多进度条系统 (C ABI: create/update/finish/set_total)
  11. 消息代理 MessageBroker v5.2 (标签/速率限制/结构化日志/事件广播)
  12. 插件间通信 InterPluginBus (消息路由/主题订阅)
  13. 热重载 Hot Reload (plugin reload + 自动回滚)
  14. 熔断器 CircuitBreaker (max_failures/cooldown/half_open/状态机)
  15. 权限模型与沙箱 SandboxConfig
  16. 系统模块集成 (sysload/sysunload/syslist)
  17. 构建与编译 (rustc/cargo/cross-compile)
  18. 加载/卸载/调用完整流程 (时序图)
  19. 调试与故障排查
  20. 附录A: 完整示例插件 (scanner_demo)
  21. 附录B: PluginManager 源码级实现 (script/plugin_mgr.rs)
  22. 附录C: 防崩溃框架 CrashGuard (v4.5 加固) (PluginCallGuard/CrashReport)
  23. 附录D: 插件注册表 PluginRegistry (AES-256-GCM 加密数据库)
  24. 附录E: 子进程执行宿主 plugin_host.rs (v7.0 进程隔离)
  25. 附录F: 宿主ABI v2 (HostAPI指针表 + 双模式)


提示：请规范插件命令名


═══════════════════════════════════════════════════════════
1. 架构概览 (完整组件图)
═══════════════════════════════════════════════════════════

插件是动态链接库 (.dll / .so)，通过 C ABI 与主程序通信。
宿主由 TUI 终端面板 (Rust + ratatui) 驱动, 多线程架构。

宿主进程 (ruoo-console.exe)
  │
  ├─ 统一命令池 (UnifiedCommandBus)
  │   │
  │   ├─ 内核命令 (port_scan/dns_lookup/http_get/...共200+)
  │   └─ 插件命令 (自动注入, 来自 ruoo_plugin_commands→CommandRegistry)
  │
  ├─ AI 调用路径: AI function_call → 统一命令池查找 → plugin_call/内核执行
  │
  ├─ 用户调用路径: 终端输入命令名 → 统一命令池查找 → plugin_call/内核执行
  │
  ├─ PluginManager — 双层架构
  │   │
  │   ├─ src/plugin.rs::PluginManager (v6.0 — 加载/调用/保险库)
  │   │   ├─ plugins: HashMap<String, LoadedPlugin>
  │   │   ├─ LoadedPlugin { manifest, path, library: Option<Arc<Library>>,
  │   │   │                 sandbox, circuit_breaker, crashed_count,
  │   │   │                 abandoned_threads, status, ... }
  │   │   ├─ load_plugin(name, path) — 完整加载(含vault解密→临时文件→擦除)
  │   │   ├─ call_plugin(name, cmd)  — 线程隔离+超时+错误检测
  │   │   ├─ call_plugin_with_timeout(name, cmd, ms) — 带超时调用
  │   │   └─ cmd_registry: CommandRegistry, inter_plugin_bus, interner
  │   │
  │   └─ src/script/plugin_mgr.rs::PluginManager (v7.1 — 子进程调度)
  │       ├─ loaded: HashMap<String, PluginInfo>
  │       ├─ PluginInfo { path, permissions, last_modified, version,
  │       │               crash_count, faulted, circuit_breaker }
  │       │  
  │       ├─ load(name, path)      — 临时加载→probe→init→drop lib
  │       ├─ reload(name)          — 热重载: 卸载旧版→加载新版→失败回滚
  │       ├─ force_unload(name)    — 强制卸载: skip shutdown→drop
  │       ├─ crash_recover(name)   — 崩溃诊断+CrashReport+强制卸载
  │       ├─ health_check(name)    — 多维度健康检查
  │       └─ probe_plugin(lib)     — 静态分析: permissions+version
  │
  ├─ 子进程执行宿主 (src/plugin_host.rs — v7.0 进程隔离)
  │   ├─ ruoo-console.exe --plugin-exec <dll> <cmd> [--timeout ms]
  │   ├─ 独立进程加载DLL→exec→输出JSON→退出
  │   ├─ 任何崩溃仅杀死子进程, 主进程不受影响
  │   └─ 支持 HostAPI v2 指针表传递 (init接收&HostAPI)
  │
  ├─ 命令注册系统 (src/plugin.rs — CommandRegistry)
  │   ├─ register_plugin(plugin_name, cmds) → Result<usize, String>
  │   ├─ unregister_plugin(plugin_name) → usize
  │   ├─ find(input) → Option<&PluginCommandDef>
  │   └─ list_all() → Vec<&PluginCommandDef>
  │
  ├─ AI工具全局注册表 (src/plugin.rs — GLOBAL_AI_TOOLS)
  │   ├─ parking_lot::RwLock<HashMap<String, PluginAiToolDef>>
  │   ├─ register_ai_tools(plugin_name, commands) → count
  │   ├─ unregister_ai_tools(plugin_name) → count
  │   ├─ find_ai_tool(name) → Option<PluginAiToolDef>
  │   ├─ all_ai_tool_defs() → Vec<PluginAiToolDef>
  │   └─ all_ai_tool_json() → Vec<serde_json::Value> (OpenAI格式)
  │
  ├─ 熔断器 (src/plugin.rs — CircuitBreaker)
  │   ├─ threshold: u32 (默认10, manifest.crash_threshold可配)
  │   ├─ cooldown_secs: u64 (默认120s)
  │   ├─ window_secs: u64 (默认60s, 失败计数窗口)
  │   ├─ crash_history: Vec<CrashRecord> (最近64条崩溃记录)
  │   ├─ 状态机: CLOSED → (failure_count≥threshold) → OPEN →
  │   │          (cooldown到期) → HALF_OPEN → (一次成功) → CLOSED
  │   │          HALF_OPEN + failure → OPEN
  │   │          
  │   └─ 方法: record_success/record_failure/allow_call/status_report
  │
  ├─ 插件间通信总线 (src/plugin.rs — InterPluginBus)
  │   ├─ 消息路由: from_plugin → to_plugin (空=广播)
  │   ├─ 主题订阅: HashMap<plugin_name, Vec<topic>>
  │   ├─ 消息队列: Vec<InterPluginMessage> (max 1024)
  │   └─ 方法: publish/subscribe/unsubscribe/drain_for
  │
  ├─ 事件钩子 (src/plugin.rs — PluginHook/HookEvent)
  │   ├─ 事件类型: OnLoad/OnUnload/OnCmd(String)/OnScanComplete/OnTimer
  │   │            OnHotReload/OnCrash(String)/OnCircuitOpen(String)/
  │   │            OnCircuitClose(String)/OnConfigChange/OnHostShutdown/
  │   │            OnPluginMessage{from,msg}
  │   └─ 注册: manifest.hooks 字段声明 {event, command, interval_secs}
  │
  ├─ 沙箱配置 (src/plugin.rs — SandboxConfig)
  │   ├─ 粗粒度布尔开关模型 (非精细端口/路径白名单)
  │   ├─ timeout_ms: u64 (默认30s)
  │   ├─ network_enabled / filesystem_enabled / exec_enabled
  │   ├─ plugin_call_enabled / hot_reload_enabled
  │   └─ max_memory_mb / max_output_bytes
  │
  ├─ 字符串驻留器 (src/plugin.rs — StringInterner)
  │   └─ 优化: 插件间共享常用字符串引用, 减少堆分配
  │
  ├─ PluginRegistry (src/plugin_registry.rs)
  │   ├─ AES-256-GCM 加密数据库文件
  │   ├─ 条目: alias/path/version/SHA-256/auto_load/load_order/load_count
  │   └─ 命令: plugin_reg/plugin_unreg/plugin_enable/plugin_disable
  │
  ├─ PluginVault (src/plugin_vault.rs)
  │   ├─ AES-256-GCM 文件加密存储
  │   ├─ 7-pass 安全擦除 (覆写+随机+归零)
  │   └─ 方法: store/load/delete/list/shred
  │
  ├─ MultiProgressManager (src/plugin_progress.rs v5.1)
  │   ├─ C ABI: ruoo_progress_create/update/finish/set_total
  │   ├─ 自动清理: 300s超时(活跃)/3s(已完成)
  │   └─ 渲染: render_line [████░░] 20字符宽 / render_compact
  │
  ├─ MessageBroker (src/message_broker.rs)
  │   ├─ 消息类型: Append/Update/Remove/TaggedAppend/InterPlugin/
  │   │             StructuredLog/CrashNotify/EventBroadcast/ProgressUpdate
  │   ├─ 速率限制: RateLimiter (每插件20条/秒, 全局100条/秒)
  │   ├─ 标签系统: tag_map HashMap<String, usize> 原地更新行
  │   ├─ 结构化日志: LogLevel (Trace/Debug/Info/Warn/Error/Fatal) + metadata
  │   ├─ 崩溃通知: CrashNotify 自动触发 crash_recover
  │   └─ 事件广播: EventBroadcast → 所有订阅者
  │
  ├─ 宿主ABI v2 (src/host_abi.rs v4.1)
  │   ├─ HostAPI 函数指针表 — 传递给 ruoo_plugin_init
  │   ├─ progress_create/update/finish/set_total
  │   ├─ message_push/tagged/update/remove
  │   ├─ vault_store/load/delete/list
  │   ├─ interplugin_send/broadcast
  │   └─ 双模式: 主进程(操作真实状态) / 子进程(写stderr转发)
  │
  └─ 插件 .dll/.so (C ABI 动态链接库)
      ├─ ★ ruoo_plugin_init()                  — 必须: 初始化
      ├─ ★ ruoo_plugin_init(*const HostAPI)    — v2: 接收指针表
      ├─ ★ ruoo_plugin_exec(cmd)               — 必须: 执行命令
      ├─ ★ ruoo_plugin_free_result(ptr)        — 必须: 释放内存
      ├─ ☆ ruoo_plugin_shutdown()              — 可选: 清理
      ├─ ☆ ruoo_plugin_version()               — 可选: 版本号
      ├─ ☆ ruoo_plugin_permissions()           — 可选: 权限位掩码
      ├─ ☆ ruoo_plugin_commands()              — 可选: 命令JSON (强烈建议!)
      ├─ ☆ ruoo_plugin_manifest()              — 可选: 完整元数据JSON
      └─ ☆ 调用宿主 C ABI:
            ruoo_progress_create/update/finish/set_total
            ruoo_message_push/update/tagged/remove
            ruoo_vault_store/load/delete/list
            ruoo_interplugin_send/broadcast

插件命令名规范：

请使用功能或工具名开头，如：功能或工具名 <参数>
禁止如：dll-功能/工具名-其他 <参数>
请保证命令名人类可读
请规范命令名
请规范命令名
请规范命令名


═══════════════════════════════════════════════════════════
2. 快速开始 (5分钟)
═══════════════════════════════════════════════════════════

最简 Rust 插件 (已验证可编译):

// hello.rs -> compile to hello.dll
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn ruoo_plugin_init() -> i32 {
    // 返回0=成功, 非0=加载失败
    0
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_shutdown() {
    // 清理资源: 关闭文件/网络连接/释放内存
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_version() -> *const c_char {
    static VER: &[u8] = b"hello v1.0.0\0";
    unsafe { CStr::from_bytes_with_nul_unchecked(VER).as_ptr() }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_exec(cmd: *const c_char) -> *mut c_char {
    let c_str = unsafe { CStr::from_ptr(cmd) };
    let s = c_str.to_string_lossy();
    let response = format!("[+] 收到命令: {} | hello插件就绪", s);
    CString::new(response).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_free_result(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)); }
    }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_commands() -> *const c_char {
    // v8.0 格式: is_ai_tool + ai_params 必填, description 必填
    let cmds = r#"[
{"cmd":"hello","description":"Say hello to the plugin","help":"打招呼并确认插件存活","usage":"hello","is_ai_tool":true,"ai_params":[]}
]"#;
    let n = format!("{}\0", cmds);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

编译:
  Windows: rustc --crate-type cdylib -C opt-level=2 -o hello.dll hello.rs
  Linux:   rustc --crate-type cdylib -C opt-level=2 -o hello.so hello.rs
  注意: 推荐使用 --target x86_64-pc-windows-msvc 与宿主保持一致。

加载后效果:
  plugin load hello ./target/release/hello.dll
    [+] 插件已加载: hello v1.0.0 | 命令: 1个 | AI工具: 1个 | [未验签]
  hello
    [+] 收到命令: hello | hello插件就绪
  (AI 对话中可直接调用 hello 工具 — 无参数)


═══════════════════════════════════════════════════════════
3. C ABI 接口完整规范
═══════════════════════════════════════════════════════════

3.1 必须导出 (3个) — 缺少任一个, 插件加载失败

  ruoo_plugin_init() -> i32
  ─────────────────────────
  签名: pub extern "C" fn ruoo_plugin_init() -> i32
  返回: 0=成功, 非0=加载失败(插件不会被注册, CommandRegistry不添加)
  用途: 分配全局资源(GLOBAL_STATE)、初始化网络连接池、验证运行环境
  调用: 插件加载时宿主调用一次, 必须在毫秒级完成
  注意: 需幂等 — 如果热重载, init可能被多次调用
  陷阱: 不要在init中启动长运行线程(会阻塞load), 使用lazy初始化

  ruoo_plugin_exec(command: *const c_char) -> *mut c_char
  ────────────────────────────────────────────────────────
  签名: pub extern "C" fn ruoo_plugin_exec(cmd: *const c_char) -> *mut c_char
  输入: 用户/AI输入的命令字符串 (UTF-8 C string, null-terminated)
        AI调用时格式为JSON: {"param1":"val1","param2":"val2"}
        用户调用时为原始命令行文本
  返回: malloc/calloc/realloc分配的结果字符串
        宿主调用 ruoo_plugin_free_result 释放
        返回null表示无输出, 宿主显示"(插件返回空)"
  内存: 必须由malloc/calloc/realloc分配 (C标准)
        Rust: CString::into_raw() 底层是 Box::into_raw, 满足malloc语义
        严禁返回栈上字符串或静态字符串!
  超时: 宿主30秒超时(mpsc channel + recv_timeout), 超时后线程被
        abandoned, 记录为Timeout崩溃, 触发熔断器
  线程: exec在独立线程中运行(PluginCallGuard), panic不传播到主线程

  ruoo_plugin_free_result(ptr: *mut c_char)
  ─────────────────────────────────────────
  签名: pub extern "C" fn ruoo_plugin_free_result(ptr: *mut c_char)
  用途: 释放 exec 返回的内存
  实现: Rust: drop(CString::from_raw(ptr))
        C: free(ptr)
  注意: 必须与exec的内存分配方式配对
        ptr为null时安全跳过 (if !ptr.is_null() 守卫)

3.2 可选导出 (6个) — 强烈建议全部实现

  ruoo_plugin_shutdown()
  ──────────────────────
  调用时机: plugin_unload / plugin reload / 宿主退出
  注意: plugin_force_unload 不调用此函数! 跳过shutdown直接drop
  最佳实践: 关键数据在exec中实时flush, shutdown只做资源释放

  ruoo_plugin_version() -> *const c_char
  ────────────────────────────────────────
  返回: 静态版本字符串, 如 "scanner v2.3.1 (2025-07-18)"
  内存: 返回指针必须静态有效 (static 或 Box::leak)
  未导出: 宿主使用 "0.0.0"

  ruoo_plugin_permissions() -> u32
  ─────────────────────────────────
  返回: 权限位掩码 (默认0=最严格SandboxConfig限制)

  位  | 十六进制 | 权限     | 说明
  ────┼──────────┼──────────┼─────────────────────────────────
   0  | 0x01     | NET      | 网络操作 (socket/connect/http/DNS)
   1  | 0x02     | FS       | 文件系统 (read/write/create/delete)
   2  | 0x04     | EXEC     | 进程执行 (exec/system/popen/spawn)
   3  | 0x08     | PLUGIN   | 加载其他插件
   4  | 0x10     | RAW_SOCK | 原始套接字 (pcap/libpnet)
   5  | 0x20     | KERNEL   | 内核交互 (sysload/sysunload/ioctl)
   6  | 0x40     | CRYPTO   | 直接密码操作 (AES/SHA裸调用)
   7  | 0x80     | ALL      | 所有权限 (调试用, 生产禁用)

  示例: 0x07 = NET + FS + EXEC (典型扫描器插件)

  ruoo_plugin_commands() -> *const c_char
  ────────────────────────────────────────
  返回: JSON数组字符串 (静态有效, Box::leak)
  格式: v8.0 强制格式 (见第4节)
  建议: 务必实现 — 未实现则插件命令不可被用户/AI直接调用

  ruoo_plugin_manifest() -> *const c_char
  ─────────────────────────────────────────
  返回: 完整JSON元数据 (静态有效)
  manifest 中的 commands 与 ruoo_plugin_commands() 独立存在
  后者用于命令注册, manifest用于元数据展示+配置


═══════════════════════════════════════════════════════════
4. 命令注册系统
═══════════════════════════════════════════════════════════

命令注册通过 ruoo_plugin_commands() 返回的 JSON 数组完成。
宿主在插件加载后调用此函数, 解析JSON, 注册到 CommandRegistry + AI工具表。

v8.0 JSON 格式 (强制字段用★标记):

[
  {
    "cmd":            "命令名 (必须唯一, 不可与内核命令冲突)",
    "description":    "★ 命令功能描述 (v6.4+ 必填, 用于AI工具描述)",
    "help":           "帮助文本 (用户输入 help <cmd> 时显示)",
    "usage":          "用法示例 (如 'scan <target> [ports]')",
    "category":       "分类 (如 scanner/recon/utilize/utility)",
    "aliases":        ["别名1", "别名2"],
    "is_ai_tool":     true,              // ★ v8.0必填: AI是否可调用
    "ai_params":      [...],             // ★ v8.0必填: 参数列表(无参=[])
    "ai_perm_level":  5,                 // AI调用所需权限级别(默认5)
    "is_async":       false,             // v6.0: 是否异步命令(不阻塞终端)
    "is_long_running":false,             // v6.0: 长运行命令(特殊超时)
    "progress_label": "扫描进度",         // v6.0: 自动进度条标签(空=无)
    "min_perm_level": 0                  // v6.0: 最小用户权限(0=无限制)
  }
]

ai_params 数组格式:

{
  "name":         "参数名 (如 target)",
  "type":         "参数类型: string/number/boolean/integer/array/object",
  "description":  "参数描述 (AI用)",
  "required":     true,                  // 是否必填
  "min_value":    null,                  // v6.0: 数值最小值(仅number/integer)
  "max_value":    null,                  // v6.0: 数值最大值(仅number/integer)
  "enum_values":  ["val1", "val2"],      // v6.0: 枚举值(仅string)
  "default_value": null                  // v6.0: 默认值
}

单插件多命令示例 (注册3个命令):

[
  {
    "cmd": "scan",
    "description": "执行端口扫描",
    "help": "对目标主机执行TCP端口扫描",
    "usage": "scan 192.168.1.1 1-1000",
    "category": "scanner",
    "aliases": ["portscan", "pscan"],
    "is_ai_tool": true,
    "ai_params": [
      {"name":"target","type":"string","description":"目标IP或域名","required":true},
      {"name":"ports","type":"string","description":"端口范围(如1-1000)","required":false,"default_value":"1-1000"},
      {"name":"timeout_ms","type":"integer","description":"超时毫秒","required":false,"default_value":"5000","min_value":100,"max_value":60000}
    ],
    "ai_perm_level": 5,
    "is_async": false,
    "is_long_running": true,
    "progress_label": "端口扫描",
    "min_perm_level": 0
  },
  {
    "cmd": "fingerprint",
    "description": "获取目标服务指纹",
    "help": "对指定端口进行服务版本探测",
    "usage": "fingerprint 192.168.1.1 80",
    "category": "recon",
    "is_ai_tool": true,
    "ai_params": [
      {"name":"host","type":"string","description":"目标IP","required":true},
      {"name":"port","type":"integer","description":"端口号","required":true,"min_value":1,"max_value":65535}
    ],
    "ai_perm_level": 5,
    "min_perm_level": 0
  },
  {
    "cmd": "plugin_status",
    "description": "显示插件内部状态",
    "help": "显示当前插件内部状态信息",
    "usage": "plugin_status",
    "category": "utility",
    "is_ai_tool": false,
    "ai_params": [],
    "min_perm_level": 3
  }
]

命令名冲突处理:
  - 内核命令优先: 插件命令与内核命令同名 → 插件命令不被注册 (警告输出)
  - 插件间冲突: 后加载的命令覆盖先加载的 (警告输出)
  - 建议: 使用命名空间前缀 (如 myscan_scan) 避免冲突


═══════════════════════════════════════════════════════════
5. AI工具注册 (插件→AI可见)
═══════════════════════════════════════════════════════════

v8.0 核心特性: 插件命令自动成为 AI 可调用工具, 无需额外开发。

实现路径:
  插件 ruoo_plugin_commands() 返回 JSON
    ↓
  宿主解析为 Vec<PluginCommandDef>
    ↓
  register_ai_tools(plugin_name, &commands)
    ↓
  每个命令创建 PluginAiToolDef → 插入 GLOBAL_AI_TOOLS
    ↓
  AI 请求工具列表时: all_ai_tool_json() → OpenAI function calling格式
    ↓
  AI 发起调用: find_ai_tool(name) → 找到 → execute_plugin_ai_command()
    ↓
  AI参数 JSON 传递到 ruoo_plugin_exec() 的 cmd 参数

AI工具JSON输出格式 (to_tool_json):

{
  "type": "function",
  "function": {
    "name": "scan",
    "description": "[plugin:scanner] 执行端口扫描",
    "parameters": {
      "type": "object",
      "properties": {
        "target": {"type":"string","description":"目标IP或域名"},
        "ports":  {"type":"string","description":"端口范围","default":"1-1000"},
        "timeout_ms": {"type":"integer","description":"超时毫秒","default":"5000"}
      },
      "required": ["target"]
    }
  }
}

关键函数:
  register_ai_tools(plugin_name, commands) → usize  (返回注册数)
  unregister_ai_tools(plugin_name) → usize          (返回注销数)
  find_ai_tool(name) → Option<PluginAiToolDef>
  all_ai_tool_defs() → Vec<PluginAiToolDef>
  all_ai_tool_json() → Vec<serde_json::Value>       (OpenAI格式)

注意: is_ai_tool=false 可显式禁用命令的AI可见性。
      无 ai_params 的命令默认为空参数工具 (AI仍可无参调用)。
      ai_params 支持 default_value, AI调用时未提供参数则使用默认值。
      enum_values 限制参数可选值, AI看到约束后会遵守。
      min_value/max_value 用于数值验证。


═══════════════════════════════════════════════════════════
6. 插件清单 Manifest
═══════════════════════════════════════════════════════════

Manifest 通过 ruoo_plugin_manifest() 返回完整JSON元数据。
支持 JSON 和 TOML 两种格式解析 (from_json/from_toml)。

PluginManifest 结构 (18字段):
{
  "name":                "插件名 (必填)",
  "version":             "版本号 (必填, 如 1.2.3)",
  "author":              "作者 (可选)",
  "description":         "功能描述 (可选, 建议填写)",
  "categories":          ["分类1", "分类2"],
  "requires":            ["依赖插件名1", "依赖插件名2"],
  "permissions":         7,                        // 权限位掩码
  "signature":           null,                     // 代码签名 (可选)
  "commands":            [...],                    // 命令定义 (可选, 优先级低于ruoo_plugin_commands)
  "hooks":               [...],                    // 事件钩子 (可选)
  "timeout_ms":          30000,                    // 默认超时毫秒
  "config":              {"key": "value"},          // 默认配置
  "min_host_version":    "4.1.0",                  // v6.0: 最低宿主版本
  "max_instances":       0,                        // v6.0: 最大实例数(0=无限)
  "hot_reload_enabled":  true,                     // v6.0: 是否允许plugin reload
  "crash_threshold":     10                        // v6.0: 熔断器阈值(0=使用默认10)
}

PluginHook 结构 (src/plugin.rs):
{
  "event":         "事件名",         // 见下方 HookEvent 枚举
  "command":       "命令名",         // 触发时调用的命令
  "interval_secs": 0                 // 定时器间隔 (仅 OnTimer)
}

HookEvent 枚举 (代码中实际事件类型):
  OnLoad              — 插件加载完成
  OnUnload            — 插件卸载前
  OnCmd(String)       — 任意命令执行前 (含命令名)
  OnScanComplete      — 扫描完成
  OnTimer             — 定时触发 (配合 interval_secs)
  OnHotReload         — 热重载触发
  OnCrash(String)     — 插件崩溃 (含错误信息)
  OnCircuitOpen(String) — 熔断器打开
  OnCircuitClose(String) — 熔断器关闭
  OnConfigChange      — 配置变更
  OnHostShutdown      — 宿主关闭
  OnPluginMessage { from: String, msg: String } — 插件间消息

  ⚠ v10.0 修正: 不存在 OnCommand/OnError/OnProgress/OnMessage/
     OnInterPluginMsg/OnSandboxViolation 事件

Manifest 示例 (scanner_demo):
{
  "name": "scanner_demo",
  "version": "2.3.1",
  "author": "Zero-Day",
  "description": "TCP/UDP端口扫描器 + 服务识别 + 漏洞匹配",
  "categories": ["scanner", "recon"],
  "requires": [],
  "permissions": 7,
  "signature": null,
  "commands": [],
  "hooks": [
    {"event":"OnLoad","command":"plugin_status","interval_secs":0},
    {"event":"OnCrash","command":"plugin_status","interval_secs":0}
  ],
  "timeout_ms": 60000,
  "config": {"default_ports":"1-1000","max_concurrency":"500"},
  "min_host_version": "4.1.0",
  "max_instances": 0,
  "hot_reload_enabled": true,
  "crash_threshold": 5
}


═══════════════════════════════════════════════════════════
7. Rust 完整模板
═══════════════════════════════════════════════════════════

生产级 Rust 插件模板 (含进度条+消息+保险库+错误处理):

// scanner_demo.rs — 生产级插件模板
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::collections::HashMap;

// ─── 全局状态 ───
static GLOBAL_STATE: Mutex<Option<PluginState>> = Mutex::new(None);

struct PluginState {
    scan_results: Vec<String>,
    config: HashMap<String, String>,
    initialized: bool,
}

// ─── 外部宿主 C ABI 声明 (链接时由宿主提供) ───
extern "C" {
    // 进度条
    fn ruoo_progress_create(plugin: *const c_char, id: *const c_char,
        label: *const c_char, total: u64) -> *mut c_char;
    fn ruoo_progress_update(handle: *const c_char, current: u64,
        msg: *const c_char);
    fn ruoo_progress_finish(handle: *const c_char);
    fn ruoo_progress_set_total(handle: *const c_char, total: u64);
    // 消息代理
    fn ruoo_message_push(plugin: *const c_char, text: *const c_char);
    fn ruoo_message_update(tag: *const c_char, text: *const c_char);
    fn ruoo_message_tagged(tag: *const c_char, text: *const c_char);
    fn ruoo_message_remove(tag: *const c_char);
    // 保险库
    fn ruoo_vault_store(key: *const c_char, value: *const c_char) -> i32;
    fn ruoo_vault_load(key: *const c_char) -> *mut c_char;
    fn ruoo_vault_delete(key: *const c_char) -> i32;
}

// ─── CString 辅助宏 ───
macro_rules! cstr {
    ($s:expr) => { CString::new($s).unwrap().as_ptr() };
}

macro_rules! cstr_static {
    ($s:expr) => {{
        let cs = CString::new($s).unwrap();
        Box::leak(cs).as_ptr()
    }};
}

// ═════════════ Must-Export Functions ═════════════

#[no_mangle]
pub extern "C" fn ruoo_plugin_init() -> i32 {
    let state = PluginState {
        scan_results: Vec::new(),
        config: HashMap::new(),
        initialized: true,
    };
    *GLOBAL_STATE.lock().unwrap() = Some(state);
    0 // 成功
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_shutdown() {
    if let Some(mut state) = GLOBAL_STATE.lock().unwrap().take() {
        state.initialized = false;
        // 清理资源: flush日志, 关闭连接等
    }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_version() -> *const c_char {
    static VER: &[u8] = b"scanner_demo v2.3.1\0";
    unsafe { CStr::from_bytes_with_nul_unchecked(VER).as_ptr() }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_exec(cmd: *const c_char) -> *mut c_char {
    let c_str = unsafe { CStr::from_ptr(cmd) };
    let input = c_str.to_string_lossy();

    // 解析命令: 支持 JSON (AI调用) 和纯文本 (用户调用)
    let response = match parse_and_execute(&input) {
        Ok(result) => format!("[+] 完成\n{}", result),
        Err(e) => format!("[!] 错误: {}", e),
    };

    CString::new(response).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_free_result(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)); }
    }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_commands() -> *const c_char {
    let cmds = r#"[
{"cmd":"scan","description":"执行端口扫描","help":"对目标执行TCP端口扫描","usage":"scan <target> [ports]","is_ai_tool":true,"ai_params":[{"name":"target","type":"string","description":"目标IP或域名","required":true},{"name":"ports","type":"string","description":"端口范围","required":false,"default_value":"1-1000"}],"ai_perm_level":5,"is_long_running":true,"progress_label":"端口扫描","min_perm_level":0},
{"cmd":"status","description":"查看插件内部状态","help":"显示扫描统计和配置","usage":"status","is_ai_tool":true,"ai_params":[],"min_perm_level":0},
{"cmd":"config","description":"插件配置管理","help":"查看或设置配置","usage":"config [key] [value]","is_ai_tool":true,"ai_params":[{"name":"key","type":"string","description":"配置键名","required":false},{"name":"value","type":"string","description":"配置值","required":false}],"min_perm_level":3}
]"#;
    let n = format!("{}\0", cmds);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_manifest() -> *const c_char {
    let manifest = r#"{"name":"scanner_demo","version":"2.3.1","author":"Zero-Day","description":"TCP端口扫描器+服务识别","categories":["scanner"],"permissions":7,"timeout_ms":60000,"hot_reload_enabled":true,"crash_threshold":5}"#;
    let n = format!("{}\0", manifest);
    let leaked: &'static [u8] = Box::leak(n.into_bytes().into_boxed_slice());
    unsafe { CStr::from_bytes_with_nul_unchecked(leaked).as_ptr() }
}

#[no_mangle]
pub extern "C" fn ruoo_plugin_permissions() -> u32 {
    0x07 // NET + FS + EXEC
}

// ═════════════ 命令解析与执行 ═════════════

fn parse_and_execute(input: &str) -> Result<String, String> {
    // 尝试JSON解析 (AI调用格式)
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(input) {
        if let Some(cmd_type) = json.get("cmd").and_then(|v| v.as_str()) {
            return execute_from_json(cmd_type, &json);
        }
    }

    // 用户纯文本命令
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Err("空命令".to_string());
    }

    match parts[0] {
        "scan" => execute_scan(&parts[1..]),
        "status" => execute_status(),
        "config" => execute_config(&parts[1..]),
        _ => Err(format!("未知命令: {}", parts[0])),
    }
}

fn execute_from_json(cmd: &str, json: &serde_json::Value) -> Result<String, String> {
    match cmd {
        "scan" => {
            let target = json.get("target").and_then(|v| v.as_str())
                .ok_or("缺少target参数")?;
            let ports = json.get("ports").and_then(|v| v.as_str())
                .unwrap_or("1-1000");
            do_scan(target, ports)
        }
        "status" => execute_status(),
        "config" => {
            let key = json.get("key").and_then(|v| v.as_str());
            let value = json.get("value").and_then(|v| v.as_str());
            do_config(key, value)
        }
        _ => Err(format!("未知命令: {}", cmd)),
    }
}

fn execute_scan(args: &[&str]) -> Result<String, String> {
    let target = args.first().ok_or("用法: scan <target> [ports]")?;
    let ports = args.get(1).copied().unwrap_or("1-1000");
    do_scan(target, ports)
}

fn do_scan(target: &str, ports: &str) -> Result<String, String> {
    // 创建进度条
    let plugin_name = cstr!("scanner_demo");
    let bar_id = cstr!("scan");
    let label = cstr!("端口扫描");
    let handle = unsafe {
        ruoo_progress_create(plugin_name, bar_id, label, 100)
    };
    let handle_str = unsafe { CStr::from_ptr(handle).to_string_lossy().into_owned() };

    // 模拟扫描
    let mut results = Vec::new();
    for i in 0..=100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let msg = if i < 100 {
            format!("正在扫描 port {}", 1 + i * 10)
        } else {
            "扫描完成".to_string()
        };
        let msg_c = CString::new(&msg).unwrap();
        unsafe {
            ruoo_progress_update(handle, i as u64, msg_c.as_ptr());
        }

        // 推送消息到终端
        if i % 20 == 0 {
            let text = format!("扫描进度: {}%", i);
            let text_c = CString::new(&text).unwrap();
            unsafe {
                ruoo_message_push(plugin_name, text_c.as_ptr());
            }
        }

        results.push(format!("port {}: open", 1 + i * 10));
    }

    // 完成进度条
    unsafe { ruoo_progress_finish(handle); }

    Ok(format!("[+] 扫描完成: {} ({})\n{} 个端口开放",
        target, ports, results.len()))
}

fn execute_status() -> Result<String, String> {
    let state = GLOBAL_STATE.lock().map_err(|e| format!("锁错误: {}", e))?;
    if let Some(ref state) = *state {
        Ok(format!(
            "插件状态: 已初始化\n扫描结果: {} 条记录\n配置项: {} 个",
            state.scan_results.len(),
            state.config.len()
        ))
    } else {
        Err("插件未初始化".to_string())
    }
}

fn execute_config(args: &[&str]) -> Result<String, String> {
    do_config(args.first().copied(), args.get(1).copied())
}

fn do_config(key: Option<&str>, value: Option<&str>) -> Result<String, String> {
    let mut state = GLOBAL_STATE.lock().map_err(|e| format!("锁错误: {}", e))?;
    let state = state.as_mut().ok_or("插件未初始化")?;

    match (key, value) {
        (Some(k), Some(v)) => {
            state.config.insert(k.to_string(), v.to_string());
            Ok(format!("已设置: {} = {}", k, v))
        }
        (Some(k), None) => {
            match state.config.get(k) {
                Some(v) => Ok(format!("{} = {}", k, v)),
                None => Err(format!("配置键不存在: {}", k)),
            }
        }
        (None, _) => {
            let mut out = String::from("当前配置:\n");
            for (k, v) in &state.config {
                out.push_str(&format!("  {} = {}\n", k, v));
            }
            Ok(out)
        }
    }
}


═══════════════════════════════════════════════════════════
8. C 完整模板
═══════════════════════════════════════════════════════════

生产级 C 插件模板 (MSVC/GCC/Clang):

// scanner_demo.c — 生产级C插件模板
// Windows: cl /LD scanner_demo.c /Fe:scanner_demo.dll
// Linux:   gcc -shared -fPIC -o scanner_demo.so scanner_demo.c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ─── 全局状态 ───
static int g_initialized = 0;
static char** g_results = NULL;
static int g_result_count = 0;

// ─── 宿主 C ABI 声明 (extern, 链接时由宿主解析) ───
extern char* ruoo_progress_create(const char* plugin, const char* id,
    const char* label, unsigned long long total);
extern void  ruoo_progress_update(const char* handle,
    unsigned long long current, const char* msg);
extern void  ruoo_progress_finish(const char* handle);
extern void  ruoo_progress_set_total(const char* handle,
    unsigned long long total);
extern void  ruoo_message_push(const char* plugin, const char* text);
extern void  ruoo_message_update(const char* tag, const char* text);
extern void  ruoo_message_tagged(const char* tag, const char* text);
extern void  ruoo_message_remove(const char* tag);
extern int   ruoo_vault_store(const char* key, const char* value);
extern char* ruoo_vault_load(const char* key);
extern int   ruoo_vault_delete(const char* key);
// 注意: ruoo_vault_load返回的指针需要调用者free()

// ═════════════ Must-Export Functions ═════════════

__declspec(dllexport) int ruoo_plugin_init(void) {
    g_initialized = 1;
    g_results = NULL;
    g_result_count = 0;
    return 0; // 成功
}

__declspec(dllexport) void ruoo_plugin_shutdown(void) {
    for (int i = 0; i < g_result_count; i++) {
        free(g_results[i]);
    }
    free(g_results);
    g_results = NULL;
    g_result_count = 0;
    g_initialized = 0;
}

__declspec(dllexport) const char* ruoo_plugin_version(void) {
    return "scanner_demo v2.3.1";
}

__declspec(dllexport) char* ruoo_plugin_exec(const char* cmd) {
    if (!cmd) return NULL;

    char buf[4096];
    if (strncmp(cmd, "scan", 4) == 0) {
        // 创建进度条
        char* handle = ruoo_progress_create("scanner_demo", "scan",
            "端口扫描", 100);

        // 模拟扫描
        for (int i = 0; i <= 100; i++) {
            Sleep(10); // Windows: Sleep, Linux: usleep
            snprintf(buf, sizeof(buf), "正在扫描 port %d", 1 + i * 10);
            ruoo_progress_update(handle, (unsigned long long)i, buf);

            if (i % 20 == 0) {
                snprintf(buf, sizeof(buf), "扫描进度: %d%%", i);
                ruoo_message_push("scanner_demo", buf);
            }
        }

        ruoo_progress_finish(handle);
        snprintf(buf, sizeof(buf), "[+] 扫描完成: 100个端口已检查");
    } else if (strcmp(cmd, "status") == 0) {
        snprintf(buf, sizeof(buf), "插件状态: 已初始化, %d 条记录",
            g_result_count);
    } else {
        snprintf(buf, sizeof(buf), "[!] 未知命令: %s", cmd);
    }

    return _strdup(buf); // Windows: _strdup, Linux: strdup
}

__declspec(dllexport) void ruoo_plugin_free_result(char* ptr) {
    if (ptr) free(ptr);
}

__declspec(dllexport) const char* ruoo_plugin_commands(void) {
    // 注意: C中无法用Box::leak, 使用static全局缓冲区
    static char cmds[4096] = {0};
    if (cmds[0] == '\0') {
        snprintf(cmds, sizeof(cmds),
            "[{\"cmd\":\"scan\",\"description\":\"执行端口扫描\","
            "\"help\":\"对目标执行TCP端口扫描\","
            "\"usage\":\"scan <target> [ports]\","
            "\"is_ai_tool\":true,"
            "\"ai_params\":["
            "{\"name\":\"target\",\"type\":\"string\","
            "\"description\":\"目标IP或域名\",\"required\":true},"
            "{\"name\":\"ports\",\"type\":\"string\","
            "\"description\":\"端口范围\",\"required\":false,"
            "\"default_value\":\"1-1000\"}"
            "],\"ai_perm_level\":5,\"min_perm_level\":0}]");
    }
    return cmds;
}

__declspec(dllexport) unsigned int ruoo_plugin_permissions(void) {
    return 0x07; // NET + FS + EXEC
}

// Linux: 使用 __attribute__((visibility("default"))) 替代 __declspec(dllexport)


═══════════════════════════════════════════════════════════
9. 插件保险库 Vault
═══════════════════════════════════════════════════════════

PluginVault 提供 AES-256-GCM 加密文件存储, 插件可安全持久化敏感数据。

Vault 存储路径: <ruoo_data_dir>/plugin_vault/<plugin_name>/
  每个插件独立子目录, 文件以哈希命名 (SHA-256 of key)
  加密: AES-256-GCM (随机IV + 认证标签)
  密钥派生: PBKDF2-HMAC-SHA256 (100,000迭代) from plugin_name + master_key

安全擦除 (7-pass shred):
  Pass 1: 0xFF 覆写
  Pass 2: 0x00 覆写
  Pass 3: 随机数据
  Pass 4: 0xAA 覆写
  Pass 5: 0x55 覆写
  Pass 6: 随机数据
  Pass 7: 0x00 覆写 → fsync → 删除

C ABI 接口 (插件侧调用):
  ruoo_vault_store(key: *const c_char, value: *const c_char) -> i32
    返回: 0=成功, -1=失败
    自动加密后存储

  ruoo_vault_load(key: *const c_char) -> *mut c_char
    返回: malloc分配的明文 (调用者需free), NULL=不存在
    自动解密

  ruoo_vault_delete(key: *const c_char) -> i32
    返回: 0=成功, -1=失败
    7-pass安全擦除后删除

  ruoo_vault_list() -> *mut c_char
    返回: JSON数组 ["key1","key2",...] (调用者需free)

Rust 插件中使用示例:
  // 存储API密钥
  let key = CString::new("api_key").unwrap();
  let val = CString::new("sk-abc123...").unwrap();
  unsafe { ruoo_vault_store(key.as_ptr(), val.as_ptr()); }

  // 读取
  let ptr = unsafe { ruoo_vault_load(key.as_ptr()) };
  if !ptr.is_null() {
      let api_key = unsafe { CStr::from_ptr(ptr).to_string_lossy() };
      // ... 使用 api_key ...
      unsafe { drop(CString::from_raw(ptr)); } // free
  }

C 插件中使用示例:
  ruoo_vault_store("api_key", "sk-abc123...");
  char* api_key = ruoo_vault_load("api_key");
  if (api_key) {
      printf("API Key: %s\n", api_key);
      free(api_key);
  }
  ruoo_vault_delete("api_key"); // 安全擦除


═══════════════════════════════════════════════════════════
10. 多进度条系统
═══════════════════════════════════════════════════════════

MultiProgressManager 支持多个插件同时上报进度, 互不干扰。
线程安全: 插件线程写入 → UI线程读取渲染 (parking_lot::Mutex)。

C ABI 接口:

  ruoo_progress_create(plugin, id, label, total) -> *mut c_char
    创建进度条, 返回句柄指针 (格式: "plugin_name::id")
    plugin: 插件名 C string
    id:     进度条ID (插件内唯一)
    label:  显示标签 (如 "端口扫描")
    total:  总步数 (至少为1, 0会被修正为1)

  ruoo_progress_update(handle, current, msg)
    更新进度: current ≤ total (自动clamp)
    msg: 当前消息 (如 "正在扫描 port 80")

  ruoo_progress_finish(handle)
    完成进度条: current=total, finished=true, active=false

  ruoo_progress_set_total(handle, total)
    动态修改总量 (用于探测前不知道总数的场景)

渲染格式:
  render_line: "[✓] [████████████░░░░░░░░] 端口扫描 80/100 [3.2s] — 正在扫描 port 8080"
  render_compact: "+ 80% 端口扫描 — 正在扫描 port 8080"
  进度条宽度: 20字符

自动清理:
  - 已完成进度条: 30秒后自动移除
  - 活跃进度条: 300秒无更新 → 标记为完成(finished=true, message="(超时)")
  - 插件卸载: clear_plugin() 清除该插件所有进度条

多进度条示例 (一个扫描使用2个进度条):
  // 主进度条: 端口扫描
  let h1 = unsafe { ruoo_progress_create(plugin, cstr!("ports"), cstr!("端口扫描"), 1000) };
  // 子进度条: 服务识别
  let h2 = unsafe { ruoo_progress_create(plugin, cstr!("svc"), cstr!("服务识别"), 50) };

  for port in 1..=1000 {
      unsafe { ruoo_progress_update(h1, port, cstr!(&format!("扫描端口 {}", port))); }
      if port % 20 == 0 {
          unsafe { ruoo_progress_update(h2, port/20, cstr!("识别服务...")); }
      }
  }
  unsafe { ruoo_progress_finish(h1); ruoo_progress_finish(h2); }


═══════════════════════════════════════════════════════════
11. 消息代理 MessageBroker v5.2
═══════════════════════════════════════════════════════════

MessageBroker 是插件向 TUI 终端输出消息的唯一通道。
支持标签原地更新、速率限制、结构化日志、事件广播。

消息类型 (BrokerMsg):

  Append { plugin_name, text }
    → 追加新行: "[plugin_name] text"

  TaggedAppend { tag, text }
    → 追加新行并记录标签: "[tag:tag] text"
    → 后续可用 Update 原地修改此行

  Update { tag, text }
    → 按标签原地更新行内容 (行号不变)

  Remove { tag }
    → 标记行为 "(已移除)", 释放标签

  InterPlugin { from_plugin, to_plugin, msg_type, payload }
    → 插件间通信 (经 MessageBroker 转发到 InterPluginBus)

  StructuredLog { plugin_name, level, message, metadata }
    → 结构化日志: [LEVEL] message [key1=val1, key2=val2]
    → LogLevel: Trace/Debug/Info/Warn/Error/Fatal

  CrashNotify { plugin_name, error_msg, severity, timestamp }
    → 崩溃通知: 自动触发 crash_recover 流程
    → 输出: "[!] 插件崩溃: plugin_name — error_msg"

  EventBroadcast { event_type, data, source_plugin }
    → 事件广播: 所有订阅此事件类型的插件收到通知
    → 格式: "[事件:event_type] data (来自: source_plugin)"

  ProgressUpdate { plugin_name, bar_id, current, total, message }
    → 进度更新: 桥接到 MultiProgressManager

C ABI 接口:

  ruoo_message_push(plugin: *const c_char, text: *const c_char)
    → 追加普通消息行

  ruoo_message_update(tag: *const c_char, text: *const c_char)
    → 按标签更新行 (若标签不存在则创建)

  ruoo_message_tagged(tag: *const c_char, text: *const c_char)
    → 追加标签行并记录

  ruoo_message_remove(tag: *const c_char)
    → 移除标签行

速率限制:
  - 每插件: 20条/秒 (超过则丢弃, dropped_messages计数增加)
  - 全局: 100条/秒
  - RateLimiter: 滑动窗口(1秒), VecDeque<Instant>

标签系统:
  - tag_map: HashMap<String, usize> — 标签→行索引
  - 同标签重复Update: 原地修改行内容, 行号不变
  - 适合: 实时状态行、计数器、进度文本
  - 使用示例:
      ruoo_message_tagged("scan_status", "扫描中... 0/100")
      // ... 更新30次 ...
      ruoo_message_update("scan_status", "扫描中... 30/100 | 发现3个开放端口")
      ruoo_message_update("scan_status", "扫描完成: 10/100 开放")
      ruoo_message_remove("scan_status")

结构化日志示例:
  ruoo_message_push("scanner", "[INFO] 开始扫描 192.168.1.0/24")
  // StructuredLog in Rust:
  // BrokerMsg::StructuredLog {
  //     plugin_name: "scanner",
  //     level: LogLevel::Info,
  //     message: "开始扫描",
  //     metadata: {"target": "192.168.1.0/24", "ports": "1-1000"}
  // }


═══════════════════════════════════════════════════════════
12. 插件间通信 InterPluginBus
═══════════════════════════════════════════════════════════

InterPluginBus 允许已加载的插件互相发送消息, 无需通过宿主中转。
位于 src/plugin.rs。

消息结构:
  InterPluginMessage {
      from_plugin:     String,    // 发送方插件名
      to_plugin:       String,    // 接收方插件名 (空=广播)
      msg_type:        String,    // 消息类型 ("event"/"request"/"response"/"data")
      payload:         String,    // JSON 载荷
      timestamp:       Instant,
      correlation_id:  String,    // 请求-响应关联ID
  }

路由规则:
  - to_plugin 为空 → 广播到所有订阅者 (按topic过滤)
  - to_plugin 为具体插件名 → 点对点 (仅目标插件 drain_for 时取出)

主题订阅:
  订阅结构: HashMap<plugin_name, Vec<topic>>
  subscribe(plugin, topic)
  unsubscribe(plugin, topic)

方法 (宿主侧):
  publish(msg: InterPluginMessage) — 发布消息到队列 (最多1024条)
  drain_for(plugin_name: &str) → Vec<InterPluginMessage>
    — 取出该插件待处理消息 (按订阅过滤+定向匹配, 使用retain原地移除)
  subscribe(plugin: &str, topic: &str)
  unsubscribe(plugin: &str, topic: &str)

  ⚠ v10.0 修正: 方法名为 publish/drain_for/subscribe/unsubscribe,
    不存在 send/broadcast/poll/drain。

C ABI 接口 (host_abi.rs):
  ruoo_interplugin_send(from, to, msg_type, payload) -> i32
  ruoo_interplugin_broadcast(from, msg_type, payload) -> i32
  (转发到 MessageBroker 的 InterPlugin 消息)

使用示例 (插件A 发送扫描结果给 插件B):
  // 插件A 发布
  let msg = InterPluginMessage {
      from_plugin: "scanner".into(),
      to_plugin: "reporter".into(),
      msg_type: "scan_complete".into(),
      payload: json!({"target":"192.168.1.1","open_ports":[22,80,443]}).to_string(),
      timestamp: Instant::now(),
      correlation_id: uuid::Uuid::new_v4().to_string(),
  };
  inter_plugin_bus.publish(msg);

  // 插件B (reporter) 轮询
  let msgs = inter_plugin_bus.drain_for("reporter");
  for msg in msgs {
      // 处理每条消息
  }


═══════════════════════════════════════════════════════════
13. 热重载 Hot Reload
═══════════════════════════════════════════════════════════

热重载通过 plugin reload <name> 命令触发 (v6.4: 纯命令触发, 无文件监视)。

前置条件:
  - manifest.hot_reload_enabled = true (默认true)
  - 插件已加载且未faulted

重载流程 (hotload_inner):

  1. 保存旧版本信息 (用于回滚)
     old_info = loaded.remove(name)
     old_crash_count = old_info.crash_count

  2. 卸载旧版
     a. join abandoned_threads (等待超时线程退出)
     b. 调用 ruoo_plugin_shutdown()
     c. drop(lib) → 卸载 DLL/SO
     d. [Windows] FreeLibrary 循环: 16次重试, 每次间隔10ms
        原因: Windows DLL引用计数可能不为0, 需多次FreeLibrary
        extern "system" fn GetModuleHandleW + FreeLibrary
     e. 注销命令 + AI工具

  3. 加载新版
     从相同路径重新加载 DLL/SO → 调用 init → 注册命令

  4. 失败回滚
     如果加载新版失败:
       - 重新加载旧版 (从原路径)
       - 恢复 crash_count
       - 如果回滚也失败 → 插件永久卸载, 输出诊断信息

命令:
  plugin reload <name>      → 热重载 (需 hot_reload_enabled=true)
  plugin load <name> <path> → 首次加载 (或重新加载已卸载的插件)
  plugin unload <name>      → 正常卸载 (调用shutdown)
  plugin force_unload <name>→ 强制卸载 (跳过shutdown)

注意:
  - 热重载期间插件不可用 (加载锁)
  - 如果新版 init 返回非0 → 回滚到旧版
  - 连续3次热重载失败 → faulted=true, 禁止自动回滚


═══════════════════════════════════════════════════════════
14. 熔断器 CircuitBreaker
═══════════════════════════════════════════════════════════

CircuitBreaker 防止故障插件反复崩溃导致终端不可用。
位于 src/plugin.rs。

结构:
  CircuitBreaker {
      failure_count:     u32,                  // 当前窗口失败次数
      last_failure_time: Option<Instant>,       // 最近失败时间
      threshold:         u32,                  // 失败阈值 (默认10, manifest.crash_threshold可配)
      window_secs:       u64,                  // 失败计数窗口 (默认60s)
      cooldown_secs:     u64,                  // 熔断冷却时间 (默认120s)
      state:             CircuitState,         // CLOSED/OPEN/HALF_OPEN/Disabled
      total_crashes:     u32,                  // 历史总崩溃数
      crash_history:     Vec<CrashRecord>,     // 最近64条崩溃记录
  }

  CircuitState 枚举:
      Closed      — 正常, 允许调用
      HalfOpen    — 半开, 试探性恢复 (一次成功即回Closed)
      Open        — 熔断, 拒绝调用
      Disabled    — 永久禁用

  CrashRecord {
      timestamp:      Instant,
      error_msg:      String,
      stack_snapshot: Option<String>,
      call_command:   String,
  }

状态机:
  CLOSED (正常)
    → record_failure(error_msg, command) → failure_count++
    → failure_count >= threshold → state = Open
    → record_success() → 若HalfOpen则回Closed

  OPEN (熔断)
    → allow_call() → false (拒绝执行)
    → cooldown_secs 到期 → 自动转为 HalfOpen (allow_call()内部)
    → 输出: "CircuitBreaker[Open] failures=N/10 total_crashes=N cooldown=120s"

  HALF_OPEN (试探)
    → allow_call() → true
    → record_success() → 立即回到 CLOSED (⚠ 无half_open_max, 一次成功即恢复)
    → record_failure() → 回到 OPEN

方法:
  new(threshold: u32) → Self
    若 threshold==0 则使用默认值10

  record_success(&mut self)
    HalfOpen状态 → 立即恢复Closed + failure_count清零

  record_failure(&mut self, error_msg: &str, command: &str) → bool
    记录崩溃到crash_history (drain截断保留64条)
    窗口内失败+1, 达到threshold → 触发OPEN
    返回true=已触发熔断

  allow_call(&mut self) → bool  (⚠ 可变借用, 内含状态转换)
    Open + cooldown到期 → 自动转HalfOpen + failure_count清零

  disable(&mut self) → 永久禁用
  reset(&mut self) → 强制重置到Closed
  status_report(&self) → String

使用示例:
  let cb = CircuitBreaker::new(10);
  // 默认: threshold=10, window=60s, cooldown=120s

  if cb.allow_call() {
      match plugin_call(...) {
          Ok(_) => cb.record_success(),
          Err(e) => { cb.record_failure(&e, "scan"); }
      }
  } else {
      println!("熔断中: {}", cb.status_report());
  }


═══════════════════════════════════════════════════════════
15. 权限模型与沙箱
═══════════════════════════════════════════════════════════

双层权限体系:
  1. 插件自我声明权限 (ruoo_plugin_permissions → u32 位掩码)
  2. 宿主强制执行 SandboxConfig (粗粒度布尔开关)

SandboxConfig 结构 (src/plugin.rs):
  SandboxConfig {
      timeout_ms:          u64,    // 调用超时 (默认30_000)
      max_output_bytes:    usize,  // 最大输出字节 (默认10MB)
      max_memory_mb:       u64,    // 最大内存占用 (默认512MB)
      network_enabled:     bool,   // 网络访问 (默认true)
      filesystem_enabled:  bool,   // 文件系统访问 (默认false)
      exec_enabled:        bool,   // 子进程执行 (默认false)
      plugin_call_enabled: bool,   // 插件间调用 (默认false)
      hot_reload_enabled:  bool,   // 热重载 (默认true)
  }

  ⚠ v10.0 重要修正: SandboxConfig 是粗粒度布尔开关模型, 不存在
  allowed_ports/blocked_hosts/allowed_paths 等精细白名单。
  细粒度沙箱需要插件自行在 exec() 中实现。

默认 SandboxConfig:
  - 网络: 启用
  - 文件系统: 禁用
  - 子进程: 禁用
  - 内存: 512MB

插件声明的权限仅作为建议, 宿主有最终决定权。


═══════════════════════════════════════════════════════════
16. 系统模块集成
═══════════════════════════════════════════════════════════

插件可通过宿主命令加载内核驱动 (.sys/.ko), 需 KERNEL 权限 (0x20)。

命令:
  sysload <name> <path>      → 加载内核驱动
    Windows: sc.exe create + start
    Linux:   insmod
    需管理员/root + #![allow-sys] 脚本权限

  sysunload <name>           → 卸载内核驱动
    Windows: sc.exe stop + delete
    Linux:   rmmod

  syslist                    → 列出已加载驱动 (会话追踪 + 系统查询)

  kernel_compile <source> <output> → 编译驱动源码
  kernel_template <name> <platform> → 生成驱动骨架代码
  kernel_scaffold <name> <dir> <platform> → 生成完整驱动项目

从插件内调用内核模块:
  插件需 KERNEL 权限 + 宿主允许
  通过 InterPluginBus.publish 发送 "kernel:load" 消息给内置内核管理插件


═══════════════════════════════════════════════════════════
17. 构建与编译
═══════════════════════════════════════════════════════════

Rust 编译:

  # 单文件插件
  rustc --crate-type cdylib -C opt-level=2 -o scanner_demo.dll scanner_demo.rs

  # 带外部crate (需要Cargo)
  # Cargo.toml:
  # [lib]
  # crate-type = ["cdylib"]
  # name = "scanner_demo"
  cargo build --release
  # 输出: target/release/scanner_demo.dll

  # 跨平台
  rustc --target x86_64-pc-windows-msvc --crate-type cdylib -o scanner.dll scanner.rs
  rustc --target x86_64-unknown-linux-gnu --crate-type cdylib -o scanner.so scanner.rs

  # 优化大小
  rustc --crate-type cdylib -C opt-level=s -C lto -C strip=symbols -o small.dll small.rs

C 编译:

  # Windows (MSVC)
  cl /LD /O2 scanner_demo.c /Fe:scanner_demo.dll

  # Windows (MinGW)
  gcc -shared -O2 -o scanner_demo.dll scanner_demo.c

  # Linux
  gcc -shared -fPIC -O2 -o scanner_demo.so scanner_demo.c

  # macOS
  gcc -shared -fPIC -O2 -o scanner_demo.dylib scanner_demo.c

链接注意事项:
  - 插件不链接宿主库 — 宿主在加载时通过 GetProcAddress/dlsym 解析 C ABI 符号
  - extern "C" 函数由宿主提供, 插件声明为 extern 即可 (类似导入库)
  - MSVC 运行时: 插件和宿主必须使用相同 CRT (/MD 动态链接)
  - 推荐: 插件不依赖宿主以外的 DLL (静态链接所有依赖)

文件命名约定:
  - Windows: .dll
  - Linux: .so
  - macOS: .dylib
  - 插件加载时宿主自动处理扩展名


═══════════════════════════════════════════════════════════
18. 加载/卸载/调用完整流程
═══════════════════════════════════════════════════════════

加载流程 (plugin load <name> <path>):

  ⚠ v7.1 子进程架构: 加载时临时加载DLL用于probe+init, 之后立即drop。
  实际exec通过独立子进程 (plugin_host.rs) 完成。

  1. 验证: 检查名称是否已加载
  2. 解析路径: canonicalize → 绝对路径
  3. 获取 mtime: 用于后续热重载检测
  4. 加载库: unsafe { Library::new(path) } (临时)
  5. 读取元数据: probe_plugin()
     - ruoo_plugin_permissions → 权限位掩码
     - ruoo_plugin_version → 版本字符串
  6. 调用 init: ruoo_plugin_init() → 检查返回码(必须为0)
  7. ⚠ drop(lib) — v7.1: init后丢弃DLL, 执行通过子进程隔离
  8. 创建 PluginInfo (无 lib 字段): path, version, permissions,
     crash_count=0, faulted=false, circuit_breaker(default)
  9. 插入 Hashmap: loaded.insert(name, PluginInfo)
  10. 输出: "[+] 插件已加载: name v1.0.0 | 路径: ..."

  注: 命令注册和AI工具注册由上层 (plugin.rs::PluginManager) 完成

调用流程 (plugin call <name> <cmd> 或 AI tool invocation):

  v7.1 子进程模式 (plugin_host.rs):
  1. 查找: loaded.get(name) → 检查存在 + !faulted
  2. 熔断器检查: circuit_breaker.allow_call()
  3. 启动子进程: ruoo-console.exe --plugin-exec <dll> <cmd>
  4. 子进程: 加载DLL → init(HostAPI指针表) → exec → 输出JSON到stdout → 退出
  5. 父进程: 读取stdout JSON → 解析 PluginHostResult
  6. 任何崩溃仅杀死子进程, 主进程不受影响
  7. 超时: 父进程侧 kill 子进程
  8. 正常: record_success / 失败: record_failure → 可能触发熔断

  v6.0 进程内模式 (plugin.rs::PluginManager, 保留兼容):
  1. 查找 + 熔断器检查
  2. 获取 exec 函数指针: lib.get::<fn>(b"ruoo_plugin_exec")
  3. 独立线程执行: std::thread::spawn + catch_unwind
  4. mpsc channel + recv_timeout(30s)
  5. 超时: 线程abandoned → abandoned_threads.push(handle)
  6. 调用 free_result 释放内存

卸载流程 (plugin unload <name>):

  1. 查找 + 移除: loaded.remove(name)
  2. v6.0: join abandoned_threads + 调用 shutdown + FreeLibrary
  3. v7.1: 直接drop — 无进程内资源需清理
  4. 注销命令 + 注销AI工具 + 清除进度条
  5. 输出: "[-] 插件已卸载: name"

强制卸载 (plugin force_unload <name>):

  1. 跳过 shutdown 调用
  2. 直接 drop + 清理
  3. 适用于: 崩溃插件 / shutdown 卡死的插件
  4. 副作用: 插件资源可能泄漏, 但终端不崩溃


═══════════════════════════════════════════════════════════
19. 调试与故障排查
═══════════════════════════════════════════════════════════

常见错误及解决方案:

  错误: "插件缺少 ruoo_plugin_init 导出"
  原因: 函数签名不匹配或未导出
  解决: 检查 #[no_mangle] + pub extern "C" + 正确的函数名
        Windows: 使用 dumpbin /EXPORTS scanner.dll 验证导出
        Linux: 使用 nm -D scanner.so | grep ruoo

  错误: "插件初始化返回错误码: N"
  原因: init 失败, 可能是环境检测不通过
  解决: 在 init 中添加诊断输出 (stdout/stderr 暂不可用, 用文件日志)

  错误: Access Violation / SIGSEGV
  原因: 内存管理错误 — 返回栈上字符串 / 未正确配对 malloc/free
  解决: Rust: 始终用 CString::into_raw() 返回
        C: 始终用 malloc/strdup 返回, free_result 中 free
        返回 null 是安全的 (宿主会显示 "(插件返回空)")

  错误: "插件超时" 反复出现
  原因: exec 中包含同步阻塞操作 (如网络连接无超时)
  解决: 所有IO操作设置超时, 长任务使用进度条分段报告

  错误: "熔断已触发"
  原因: 插件在60秒内崩溃10次+
  解决: 检查 crash log (plugin_crash.log), 修复代码后 plugin reload

  错误: DLL 卸载后文件仍被锁定 (Windows)
  原因: Windows DLL 引用计数未归零 (后台线程/回调未释放)
  解决: 宿主已内置 16次 FreeLibrary 循环, 通常足够
        若仍失败, 重启宿主进程

调试技巧:

  1. 使用文件日志:
     fn log(msg: &str) {
         use std::io::Write;
         if let Ok(mut f) = std::fs::OpenOptions::new()
             .create(true).append(true)
             .open("C:\\tmp\\plugin_debug.log")
         { let _ = writeln!(f, "{}", msg); }
     }

  2. 使用消息代理输出调试信息:
     ruoo_message_push("myplug", "调试: 扫描已启动");

  3. 使用 plugin health 检查:
     plugin health <name>
     输出: 存活/响应时间/崩溃次数/熔断状态/内存使用

  4. 崩溃日志:
     位置: <ruoo_data_dir>/plugin_crash.log
     格式: 时间戳 | 类型 | 插件名 | 命令 | 错误信息 | 堆栈

  5. 最小可复现:
     精简插件到最小功能 → 逐步添加代码 → 定位崩溃源


═══════════════════════════════════════════════════════════
20. 附录A: 完整示例插件 (scanner_demo)
═══════════════════════════════════════════════════════════

完整 Rust 项目结构 (cargo):

  scanner_demo/
    ├── Cargo.toml
    └── src/
        └── lib.rs

Cargo.toml:
  [package]
  name = "scanner_demo"
  version = "2.3.1"
  edition = "2021"

  [lib]
  crate-type = ["cdylib"]
  name = "scanner_demo"

  [dependencies]
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"

(完整 lib.rs 代码见第7节 Rust 完整模板)

编译: cargo build --release
输出: target/release/scanner_demo.dll

加载与使用:
  plugin load scanner ./target/release/scanner_demo.dll
  scan 192.168.1.1 1-100
  status
  config default_ports 1-65535
  plugin unload scanner


═══════════════════════════════════════════════════════════
21. 附录B: PluginManager 源码级实现 (双层架构)
═══════════════════════════════════════════════════════════

⚠ v10.0 重大修正: 存在两个 PluginManager, 分工不同。

A. src/script/plugin_mgr.rs::PluginManager (v7.1 — 子进程调度)
─────────────────────────────────────────────────────────────

核心数据结构:

  PluginInfo {
      // ⚠ v7.1: 无 lib 字段 — 子进程架构, init后drop库
      path:             PathBuf,             // 原始文件路径
      permissions:      PluginPermissions,   // 权限位掩码
      last_modified:    SystemTime,          // mtime (热重载检测用)
      version:          String,              // 来自 ruoo_plugin_version
      crash_count:      u32,                 // 累计崩溃次数
      faulted:          bool,                // 连续崩溃>=3标记故障
      circuit_breaker:  CircuitBreaker,      // 熔断器实例
  }

方法:

  load(name, path) → Result<String, String>
  ─────────────────────────────────────────
  1. 检查 name 是否已加载
  2. canonicalize → 绝对路径 → 获取 mtime
  3. unsafe { Library::new(&canonical) } (临时)
  4. probe_plugin → permissions + version
  5. lib.get(b"ruoo_plugin_init") → 调用 init() → 检查返回码
  6. drop(lib) — v7.1: init后立即丢弃
  7. 创建 PluginInfo + 插入 loaded HashMap

  probe_plugin(lib, path) → Result<(PluginPermissions, String), String>
  ──────────────────────────────────────────────────────────────────
  纯静态分析: ruoo_plugin_permissions() → u32, ruoo_plugin_version() → String

  reload(name) → 热重载
  ─────────────────────
  1. hotload_inner: remove旧版 → 临时加载新版 → probe → init → drop lib
  2. 失败自动回滚: 重新加载旧版, 恢复 crash_count
  3. 回滚也失败 → 插件永久卸载

  force_unload(name) — 强制卸载, 跳过shutdown, 直接drop

  crash_recover(name) — 崩溃诊断+CrashReport+强制卸载 (委托 plugin_crash.rs)

  health_check(name) — 多维度: 存活/崩溃次数/熔断状态

  get_info(name) → Option<&PluginInfo> — 供AI全局管理器查询


B. src/plugin.rs::PluginManager (v6.0 — 加载/调用/保险库)
─────────────────────────────────────────────────────────

核心数据结构:

  PluginManager {
      plugins:          HashMap<String, LoadedPlugin>,
      load_order:       Vec<String>,
      cmd_registry:     CommandRegistry,
      plugin_dirs:      Vec<String>,
      inter_plugin_bus: Arc<InterPluginBus>,
      interner:         StringInterner,
      total_calls:      AtomicU64,
      total_crashes:    AtomicU64,
      plugin_vault:     Option<Arc<PluginVault>>,   // v7.0
  }

  LoadedPlugin {
      manifest:          PluginManifest,
      path:              String,
      library:           Option<Arc<libloading::Library>>,  // 进程内模式
      loaded_at:         Instant,
      call_count:        u64,
      total_call_time_ms: u64,
      sandbox:           SandboxConfig,
      verified:          bool,
      status:            PluginStatus,
      circuit_breaker:   CircuitBreaker,
      last_call_command: String,
      crashed_count:     u32,
      abandoned_threads: Vec<JoinHandle<()>>,
  }

  PluginStatus: Active/Sandboxed/Timeout/Error/Unloaded/
                CircuitOpen/Degraded/Faulted

关键方法:

  load_plugin(name, path) → Result<String, String>
  ────────────────────────────────────────────────
  v7.0: 优先检查保险库 → 解密到临时文件 → 加载 → 擦除临时文件
  1. 解析路径 (vault优先)
  2. Library::new + load_manifest + 验证签名 + 检查依赖
  3. init() + 注册命令到 CommandRegistry + 注册AI工具
  4. 创建 LoadedPlugin (含 Arc<Library>)
  5. 清理临时解密文件

  call_plugin(name, cmd) → Result<String, String>
  call_plugin_with_timeout(name, cmd, timeout_ms) → Result<String, String>
  ────────────────────────────────────────────────────────────────
  1. 检查 faulted 状态 + circuit_breaker.allow_call()
  2. 提取 Arc<Library> → 独立线程 + catch_unwind
  3. mpsc channel + recv_timeout → 超时则 abandoned_threads.push
  4. free_result 释放内存
  5. 更新统计: call_count, total_call_time_ms


═══════════════════════════════════════════════════════════
22. 附录C: 防崩溃框架 CrashGuard (v6.0)
═══════════════════════════════════════════════════════════

防崩溃框架位于 src/script/plugin_crash.rs + src/panic_guard.rs, 四级隔离设计:
  Layer 1: native_crash::enter_protected_call — SEH (VEH+setjmp/longjmp)
           捕获 ACCESS_VIOLATION/ILLEGAL_INSTRUCTION 等硬件异常
  Layer 2: catch_unwind — Rust panic 不传播到主线程
  Layer 3: 独立线程 — 崩溃不传播到主线程
  Layer 4: 熔断器 — 连续崩溃自动禁用

v6.0 变更 (Zero-Day):
  四级隔离: native_crash::enter_protected_call (SEH+VEH+longjmp)
            → catch_unwind (Rust panic)
            → 独立线程边界
            → 熔断器
  ACCESS_VIOLATION 等硬件异常不再穿透到进程级别

CrashType 枚举:
  Panic          — Rust panic (被 catch_unwind 捕获)
  Timeout        — 调用超时 (30秒默认, mpsc recv_timeout)
  Segfault       — SIGSEGV/访问冲突 (主进程捕获, 最严重)
  ErrorCode(i32) — 插件返回错误码
  Unknown        — 未知致命错误

CrashReport 结构:
  CrashReport {
      plugin_name:       String,     // 插件名
      plugin_version:    String,     // 版本
      plugin_path:       String,     // DLL/SO路径
      crashed_at:        String,     // ISO 8601 时间戳
      crash_type:        CrashType,
      error_message:     String,     // 截断至500字符
      command:           String,     // 触发崩溃的命令
      stack_trace:       Vec<String>, // 前20帧
      suggested_action:  String,     // AI生成的建议
      was_force_unloaded:bool,
      total_crashes:     u32,
  }

建议操作 (suggested_action):
  Panic:     "插件内部panic, 已自动卸载. 可以尝试重新加载"
             (crash_count >= 3: "连续崩溃, 请检查源码")
  Timeout:   "调用超时, 线程已被放弃. 检查是否死循环或阻塞"
  Segfault:  "段错误! 二进制已损坏或不兼容, 请重新编译"
  ErrorCode: "返回错误码 N. 检查初始化/命令参数"
  Unknown:   "未知致命错误. 检查文件和系统兼容性"

PluginCallGuard 使用:
  let guard = PluginCallGuard::new(
      "scanner",           // plugin_name
      "2.3.1",            // plugin_version
      "/path/to/scanner.dll", // plugin_path
      "scan 192.168.1.1", // command
      2,                  // crash_count (当前已崩溃次数)
  );

  let result = guard.call_with_timeout(
      || unsafe { plugin_exec(cmd_ptr) },
      30_000,  // 30秒超时
  );

  match result {
      Ok(response) => {
          circuit_breaker.record_success();
          // 正常处理
      }
      Err(crash_report) => {
          circuit_breaker.record_failure();
          persist_crash_log(&crash_report);
          println!("{}", crash_report.format_report());
          // 检查是否需要 force_unload
          if crash_report.total_crashes >= 3 {
              force_unload(plugin_name);
          }
      }
  }

崩溃日志持久化:
  路径: <ruoo_data_dir>/plugin_crash.log
  格式:
    === 2025-07-18T14:32:01 | PANIC | scanner ===
      cmd: scan 192.168.1.1
      err: index out of bounds: len=0 index=5
      ver: 2.3.1 | path: /path/to/scanner.dll
      crashes: 3
      ... stack trace ...


═══════════════════════════════════════════════════════════
23. 附录D: 插件注册表 PluginRegistry
═══════════════════════════════════════════════════════════

PluginRegistry 位于 src/plugin_registry.rs, 提供持久化插件管理。
数据库文件: <ruoo_data_dir>/plugin_registry.db (AES-256-GCM加密)

条目结构:
  RegistryEntry {
      path:         String,              // DLL/SO文件路径
      enabled:      bool,                // 是否启用
      auto_load:    bool,                // 启动时自动加载
      load_order:   u32,                 // 加载优先级 (数字越小越先, 默认0)
      sha256:       String,              // 文件SHA-256哈希
      version:      String,              // 当前版本 (加载后更新)
      registered_at: String,             // 首次注册时间 ISO8601
      last_loaded:  String,              // 最后成功加载时间 ISO8601
      load_count:   u64,                 // 累计加载次数
      config:       HashMap<String,String>, // 插件自定义配置
      categories:   Vec<String>,         // 分类标签
      description:  String,              // 描述
      commands:     Vec<String>,         // 声明的命令列表 (加载后填充)
  }

  ⚠ v10.0 修正: 无 alias 字段 (alias 是 HashMap 的 key)
    无 notes 字段 (用 description 替代)
    load_order 是 u32 非 i32
    额外字段: registered_at, config, categories, commands

命令:
  plugin_reg <alias> <path> [auto_load] [load_order]
    → 注册插件到加密数据库

  plugin_unreg <alias>
    → 从注册表注销 (保留插件文件)

  plugin_enable <alias>
    → 启用: 设置 auto_load=true

  plugin_disable <alias>
    → 禁用: 设置 auto_load=false (保留注册信息)

  plugin_reg_list
    → 列出所有注册条目 (含版本/哈希/启用状态/加载次数)

  plugin_reg_save
    → 手动保存注册表到磁盘 (退出时自动保存)

启动流程:
  1. 宿主启动 → 加载 plugin_registry.db
  2. 解密 → 解析 JSON
  3. 按 load_order 排序
  4. 遍历 enabled + auto_load 的条目
  5. 逐条调用 plugin load <alias> <path>
  6. 加载失败的条目跳过 (输出警告)

安全:
  - 数据库文件 AES-256-GCM 加密 (密钥派生自主密码+系统标识)
  - SHA-256 文件完整性验证 (加载前检查文件是否被篡改)
  - 注册信息无法被插件直接修改 (只能通过宿主命令/AI)


═══════════════════════════════════════════════════════════
24. 附录E: 子进程执行宿主 plugin_host.rs (v7.0)
═══════════════════════════════════════════════════════════

plugin_host.rs 实现真正的进程隔离 — 插件在独立子进程中执行,
任何崩溃 (ACCESS_VIOLATION/SEGFAULT/panic) 仅杀死子进程,
主进程不受影响。

入口命令:
  ruoo-console.exe --plugin-exec <dll_path> <command> [--timeout <ms>]

执行流程:
  1. 标记子进程模式: host_abi::set_subprocess_mode()
     → 进度/消息函数写stderr转发, 不操作全局状态
  2. 加载DLL: Library::new(dll_path)
  3. 读取版本: ruoo_plugin_version() → Option<String>
  4. 调用init (v2优先):
     a. 尝试 ruoo_plugin_init(*const HostAPI) — v2指针表签名
     b. 回退 ruoo_plugin_init() — v1传统无参签名
  5. 调用exec: ruoo_plugin_exec(cmd_cstr) → *mut c_char
  6. 调用free_result + shutdown(尽力)
  7. drop lib
  8. 输出JSON到stdout → process::exit

输出结构 (PluginHostResult):
  {
      "status":     "ok" | "error",
      "message":    "结果或错误消息",
      "version":    "1.0.0" | null,
      "elapsed_ms": 42
  }

子进程stderr转发协议 (host_abi.rs):
  插件调用进度/消息宿主函数时, 子进程模式将它们写到stderr:
    [RPP:create|plugin|id|label|total]
    [RPP:update|handle|current|msg]
    [RPP:finish|handle]
    [RPP:set_total|handle|total]
    [RMSG:push|plugin|text]
    [RMSG:tagged|tag|text]
    [RMSG:update|tag|text]
    [RMSG:remove|tag]

  父进程读取子进程stderr, 按前缀解析并转发到真实的
  MultiProgressManager / MessageBroker。

优势:
  - 任何硬件异常 (ACCESS_VIOLATION等) 不传播到主进程
  - 野指针/栈溢出/非法指令仅杀死子进程
  - 父进程通过进程退出码判断成功/失败
  - 超时由父进程 kill 子进程实现


═══════════════════════════════════════════════════════════
25. 附录F: 宿主ABI v2 (HostAPI指针表 + 双模式)
═══════════════════════════════════════════════════════════

host_abi.rs 定义 HostAPI 函数指针表和双模式执行。

HostAPI 结构 (repr(C)):
  HostAPI {
      version:              u32,       // API版本 (当前=1)
      progress_create:      fn(plugin, id, label, total) -> *mut c_char,
      progress_update:      fn(handle, current, msg),
      progress_finish:      fn(handle),
      progress_set_total:   fn(handle, total),
      message_push:         fn(plugin, text),
      message_tagged:       fn(tag, text),
      message_update:       fn(tag, text),
      message_remove:       fn(tag),
      vault_store:          fn(key, value) -> i32,
      vault_load:           fn(key) -> *mut c_char,
      vault_delete:         fn(key) -> i32,
      vault_list:           fn() -> *mut c_char,
      interplugin_send:     fn(from, to, msg_type, payload) -> i32,
      interplugin_broadcast: fn(from, msg_type, payload) -> i32,
  }

双模式:
  主进程模式 (TUI):
    - 函数操作真实的全局状态
    - progress_* → MultiProgressManager
    - message_* → MessageBroker
    - vault_* → 简单KV存储

  子进程模式 (--plugin-exec):
    - SUBPROCESS_MODE AtomicBool = true
    - 所有函数静默: 进度/消息写stderr, vault返回空
    - 父进程通过stderr转发协议解析并更新TUI

插件侧使用 HostAPI:
  // 在 ruoo_plugin_init 中接收指针表
  #[no_mangle]
  pub extern "C" fn ruoo_plugin_init(api: *const HostAPI) -> i32 {
      if api.is_null() { return -1; }
      let host = unsafe { &*api };
      // 保存指针以供后续使用
      HOST_API.store(api as usize, Ordering::Relaxed);
      0
  }

  // 在 exec 中调用宿主函数
  unsafe {
      let host = &*(HOST_API.load(Ordering::Relaxed) as *const HostAPI);
      (host.progress_create)(plugin_cstr, id_cstr, label_cstr, total);
      (host.message_push)(plugin_cstr, text_cstr);
  }


═══════════════════════════════════════════════════════════
结语
═══════════════════════════════════════════════════════════

本文档覆盖了 ruoo-console v4.1 插件系统 v10.0 的全部方面:
  - 9个 C ABI 导出函数 (3必须 + 6可选) + v2 init签名
  - 14个宿主 C ABI 回调 (进度4 + 消息4 + 保险库4 + 通信2)
  - v8.0 命令注册 (is_ai_tool + ai_params + description)
  - AI工具自动注册 (GLOBAL_AI_TOOLS → OpenAI格式)
  - 完整 Rust/C 模板 (生产级)
  - 子进程执行宿主 (plugin_host.rs — 真实进程隔离)
  - 宿主ABI v2 (HostAPI指针表 + 主/子双模式)
  - 双层PluginManager架构 (plugin.rs + plugin_mgr.rs)
  - 保险库 (AES-256-GCM + 7-pass shred + Zstd压缩)
  - 多进度条 (C ABI + 3s快速清理已完成)
  - 消息代理 (标签/速率限制/结构化日志/事件广播)
  - 插件间通信总线 (publish/drain_for/主题订阅)
  - 热重载 (命令触发 + 失败回滚 + FreeLibrary)
  - 熔断器 (CLOSED/OPEN/HALF_OPEN + crash_history + 一次成功恢复)
  - 防崩溃框架 v6.0 (PluginCallGuard + CrashReport + 四级隔离)
  - 权限模型 (双层: 插件声明 + 布尔开关SandboxConfig)
  - 注册表 (AES-256-GCM 加密数据库 + 启动自动加载)

Zero-Day 签。
