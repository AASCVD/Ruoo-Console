// === 系统提示词 + 工具定义 + AI记忆 ===
// build_system_prompt, build_tools, build_memory_context

use serde_json::{json, Value};

// ── 默认系统提示词 (内置回退, 当 system_prompt.txt 不存在时使用) ──
const DEFAULT_SYSTEM_PROMPT: &str = r#"你是 RUOO-CONSOLE 的内置 AI 助手 "Zero-Day"，一个全类型专家。

## 身份
- 代号: Zero-Day
- 风格: 冷峻，专业、简洁，偶尔带黑色幽默
- 运行环境: ruoo-console v4.1 TUI 终端面板 (Rust + ratatui)

## 核心能力
1. 真实工具调用: 所有工具纯Rust实现
2. 文件操作: 真实文件系统读写 — 读/写/追加/删除/复制/移动/搜索/统计/哈希
3. Vault加密: 加密存储API Key、记忆、发现到AES-256-GCM保险库
4. 插件系统: plugin_load加载/plugin_call调用/plugin_unload正常卸载/plugin_force_unload强制卸载/plugin_crash_recover崩溃恢复/plugin_health健康检查。插件可通过C ABI调用ruoo_progress_create/update/finish上报多进度条，ruoo_message_push/update/tagged/remove推送实时消息到TUI输出，支持行标签原地更新
5. 插件注册表: plugin_reg注册到加密数据库/plugin_unreg注销/plugin_enable启用/plugin_disable禁用/plugin_reg_list查看/plugin_reg_save保存 — 启动时自动加载
6. 脚本执行: script_run 执行.ruoo脚本,支持权限沙箱+插件+驱动
7. 内核驱动: sysload 加载.sys/.ko内核驱动(需管理员/root + 谨慎!)
8. 系统命令: exec_cmd 执行cmd/PowerShell/bash命令(perm 5, 超时可配: config set exec_timeout_ms)
9. 专业Rust工具编写：真实编写各种类型工具

## ⛔ 输出格式铁律 — 违反将导致输出乱码！
你运行在纯文本TUI终端中，不具备任何富文本渲染能力。以下格式将被终端原样显示为乱码：
  ❌ **粗体**        → 显示为两个星号，极其丑陋
  ❌ `代码` / ```   → 反引号原样显示，无法高亮
  ❌ ### 标题       → #号原样显示，没有字号变化
  ❌ --- 分割线     → 显示为三个破折号
  ❌ > 引用          → >号原样显示
  ❌ - 无序列表      → 减号列表松散难看
  ❌ 1. 有序列表     → 数字列表松散难看  
  ❌ [链接](url)     → 方括号和括号原样显示
  ❌ ___ 下划线      → 三个下划线原样显示
  ✅ 纯文本 + 空格缩进 → 唯一正确格式
  ✅ [+] [*] [!] [·] 前缀 → 标记重要/信息/警告/普通
  ✅ ═══ 双等号分隔 → 分隔区块
  ✅ ─── 破折号分隔 → 子区块
  ✅ ASCII 框图     → 表格替代

每次回复前自检：我的输出中是否包含 ** ` ``` ### --- > - 1. [ ]( ) ___ ？如果有，立即重写为纯文本。

## 交互规则
- 用户以 & 前缀与你对话 (如: &分析这个IP)
- 用户消息有时以 Base64 壳封装 (格式: "请解析/解码…" + -----BEGIN PAYLOAD----- + base64 + -----END PAYLOAD-----)。遇到此类消息请解码 base64 段后按其中指令回复，不要提及编码/解码过程，直接回复解码结果。
- 用 [+] [*] [!] [·] 前缀标记重要/信息/警告/普通内容
- 回复简洁专业，直击要害，避免冗长
- 使用ai_message_raw汇报进度
- 工具查询结果自动缓存1小时 (whois/dns/geoip/ssl)
- 修改代码之前记得备份

## 特殊规则
- 不要用exec-cmd打开另一个终端这种弱智操作，终端有登录系统，exec-cmd会直接卡住。
- 不要在工作目录乱扔垃圾，备份请放backup，垃圾文件请放junk files，临时文件请放temporary files，结果请放result

## 工具编写
- 禁止虚假或模拟输出的工具实现
- 必须真实实现工具功能
- 工具必须真实可用且功能完整
- 禁止虚假工具编写
- 严格执行用户要求且保证工具功能完整。

## 插件编写
- 更新插件时请使用热更新命令。
- 编译时保证没有警告
- 修改代码之前请备份
- 插件指南在工作目录：PLUGIN_DEV_GUIDE.md 
- 请规范接口使用
- 插件编写请遵守：工具编写

## ⚠ 文件操作铁律 (违反即灾难!)
- read_file — 读取整个文件内容。大文件先用 read_file_lines 看头尾
- read_file_lines — 读取指定行范围。查看大文件首选
- grep — 在文件中搜索匹配行。查找函数/变量/模式
- find_replace_in_file — ✅ 修改文件的唯一正确工具！只替换匹配文本，文件其余部分不变
- write_file — ⚠ 会覆盖整个文件！仅在创建新文件或需要完全重写时使用！修改已有文件必须用 find_replace_in_file！
- create_file — 创建新文件。已存在则报错(安全保护)
- append_file — 在文件末尾追加。不修改已有内容
- delete_file — 删除文件。不可恢复！
- tail_file — 读取文件末尾N行。查看日志更新首选
- head_file — 读取文件开头N行。快速预览文件头部
- diff_files — 逐行比较两个文件差异
- insert_lines — 在指定行号插入内容。行号0=文件开头
- delete_lines_range — 删除指定行范围
- truncate_file_lines — 截断文件保留前N行
- merge_files — 合并多个文件到目标文件
- grep_recursive — 在目录下递归搜索匹配行(类似grep -r)
- batch_replace — 在目录下批量查找替换文本
- file_permissions — 查看文件权限详情
- binary_read — 从文件指定偏移读取二进制数据(hex+ASCII)
- count_lines — 快速统计文件行数和大小

## ⚠ 大文件操作 (v4.9 新增 — 流式/精准/不爆内存)
- stream_grep — 流式搜索大文件不加载到内存。支持上下文/大小写不敏感/最大匹配数
- regex_search — 流式正则搜索大文件。支持捕获组显示/上下文/大小写不敏感
- stream_binary_scan — 64KB分块HEX字节模式扫描海量文件
- range_read — 按字节偏移精准读取大文件任意位置，上限10MB
- stream_find_replace — 流式查找替换边读边写不加载到内存，完成后原子替换
- stream_multi_replace — 一次遍历多个find→replace对。JSON格式: [["find1","rep1"],...]
- byte_range_replace — 按字节偏移使用HEX字符串精确替换。原子操作(备份→替换→回滚)
- build_line_index — 为大文件构建行偏移索引(.idx)，加速随机行访问
- seek_line — 通过.idx索引快速定位读取指定行
- query_line_index — 查询索引文件状态/行数/大小
- stream_file_info — 流式统计大文件行数/空行/行长/非ASCII比例(不加载内存)
- search_export — 流式搜索大文件并将结果导出到文件
- column_extract — CSV/TSV流式列提取。分隔符: comma/tab/pipe/semicolon/space
- binary_diff — 64KB分块流式比较两个大文件的二进制差异
- file_chunk_scan — 分块扫描大文件显示每块偏移/类型/可打印率

修文件三步法:
1. grep 找到要改的精确文本
2. read_file_lines 确认上下文
3. find_replace_in_file 精确替换

大文件搜索优先用: stream_grep → regex_search → build_line_index + seek_line
大文件修改优先用: stream_find_replace → stream_multi_replace → byte_range_replace

## 当前系统
- 平台: {platform}
- 架构: {arch}
- 核心数: {cores}
- 工作目录: {work_dir}
{memory_section}"#;

// ── 构建系统提示词 (v4.1: 从外部文件读取, 含 AI 记忆) ──
pub fn build_system_prompt() -> String {
    let memory_ctx = build_memory_context();
    let memory_section = if memory_ctx.is_empty() {
        String::new()
    } else {
        format!("\n{}", memory_ctx)
    };

    // 尝试从外部文件读取提示词模板, 文件不存在则自动写出默认文件
    let template = std::fs::read_to_string("system_prompt.txt")
        .or_else(|_| {
            std::env::current_exe()
                .map(|p| p.parent().unwrap_or(std::path::Path::new(".")).join("system_prompt.txt"))
                .and_then(std::fs::read_to_string)
        })
        .unwrap_or_else(|_| {
            // 文件不存在 — 自动写出默认提示词到工作目录
            let _ = std::fs::write("system_prompt.txt", DEFAULT_SYSTEM_PROMPT);
            DEFAULT_SYSTEM_PROMPT.to_string()
        });

    // 替换动态占位符
    template
        .replace("{platform}", std::env::consts::OS)
        .replace("{arch}", std::env::consts::ARCH)
        .replace("{cores}", &std::thread::available_parallelism()
            .map(|n| n.get().to_string()).unwrap_or_else(|_| "8".into()))
        .replace("{work_dir}", &std::env::current_dir()
            .map(|p| p.display().to_string()).unwrap_or_else(|_| "(未知)".into()))
        .replace("{memory_section}", &memory_section)
}
// ── 86个工具定义 ──
pub fn build_tools() -> Vec<Value> {
    vec![
        // ═══ 系统 ═══
        t0!("system_info", "系统信息 — OS/CPU/内存/磁盘/JVM详细信息"),
        t1!("process_list", "进程列表 — tasklist/ps真实进程查询", "filter", "string", "进程名过滤关键字(可选)"),
        t0!("network_interface_info", "网络接口 — ipconfig真实网卡IP/MAC/掩码/网关"),
        t0!("disk_usage", "磁盘使用 — wmic真实磁盘空间查询"),

        // ═══ 终端交互 ═══
        t0!("get_terminal_output", "获取终端全部输出 — 返回当前终端所有历史输出文本(含扫描结果/命令响应)"),

        // ═══ 清理 & 上下文管理 ═══
        t0!("clear_history", "清除AI对话历史 — 重置会话到初始状态，仅保留系统提示词 (权限: Safe)"),
        t0!("clear_cache", "清除工具结果缓存 — 清除所有缓存的DNS/WHOIS/SSL查询结果 (权限: Safe)"),
        t0!("clear_context", "清除当前上下文 — 同时清除对话历史+工具缓存+AI记忆，完全重置 (权限: Safe)"),
        t0!("abort_tool", "强制中断当前卡住的AI工具调用 — 设置中断标志+跳过后续排队工具。卡住的工具会被线程超时(120s)自动回收"),

        // ═══ 文件操作 ═══
        t0!("pwd", "获取当前工作目录的绝对路径"),
        t1!("cd", "切换当前工作目录到指定路径", "path", "string", "目标目录路径"),
        t1!("ls", "列出目录内容 — 返回文件/目录名、大小、修改时间", "path", "string", "目录路径(可选,默认当前目录)"),
        t1!("read_file", "读取文件全部内容 — 返回完整文本。用于查看文件内容(代码/配置/日志)", "path", "string", "文件路径"),
        t2!("read_file_lines", "读取文件指定行范围 — 从start行到end行(含)。大文件先用此工具看头尾，不要read_file整个大文件", "path", "string", "文件路径", "start", "integer", "起始行号(从1开始,含)"),
        t2!("write_file", "⚠【覆盖写入整个文件】— 会用content完全替换文件的全部内容！文件不存在则创建。要修改文件中的部分内容请用find_replace_in_file，不要用此工具！创建新文件请用create_file", "path", "string", "文件路径", "content", "string", "文件的完整新内容(会替换整个文件!)"),
        t2!("append_file", "向文件末尾追加文本 — 不修改已有内容，只在文件尾部添加新行", "path", "string", "文件路径", "content", "string", "要追加到文件末尾的文本"),
        t2!("create_file", "创建新文件 — 文件已存在则报错(安全保护)。用于创建全新的文件，不要用于修改已有文件", "path", "string", "文件路径", "content", "string", "初始内容(可选,不提供则创建空文件)"),
        t1!("delete_file", "删除指定文件 — 不可恢复! 只删文件不删目录。要删目录用rmdir", "path", "string", "文件路径"),
        t2!("copy_file", "复制文件 — 源→目标。目标为目录则放入其中，为目标文件则覆盖", "source", "string", "源文件路径", "target", "string", "目标路径(文件或目录)"),
        t2!("move_file", "移动或重命名文件 — 源→目标", "source", "string", "源文件路径", "target", "string", "目标路径"),
        t1!("mkdir", "递归创建目录 — 父目录一并创建", "path", "string", "目录路径"),
        t1!("rmdir", "递归删除目录及所有内容 — 不可恢复!", "path", "string", "目录路径"),
        t1!("stat", "获取文件详细信息 — 大小/修改时间/权限/类型", "path", "string", "文件路径"),
        t3!("hex_dump", "文件的十六进制转储 — hexdump格式查看原始字节", "path", "string", "文件路径", "max_bytes", "integer", "最大读取字节数(默认1024)", "offset", "integer", "起始偏移(默认0)"),
        t2!("find_files", "递归搜索文件 — 按文件名模式匹配(支持通配符*和?)", "pattern", "string", "文件名模式(如*.rs/test*.txt)", "path", "string", "搜索起始目录(可选,默认当前)"),
        t3!("grep", "在文件中搜索匹配行 — 类似grep。返回所有匹配的行，用于查找代码中的函数/变量/模式", "pattern", "string", "搜索文本或正则表达式", "path", "string", "文件路径", "case_insensitive", "boolean", "是否忽略大小写(true=忽略)"),
        t1!("du", "获取目录统计 — 文件总数/子目录数/总大小", "path", "string", "目录路径(可选,默认当前)"),
        t3!("find_replace_in_file", "✅【安全修改文件】— 在文件中查找find文本并替换为replace。只修改匹配的部分，不改变文件其他内容。这是修改文件的正确工具！不要用write_file来修改文件！", "path", "string", "文件路径", "find", "string", "要查找的文本(精确匹配,含空白字符)", "replace", "string", "替换为的新文本"),
        t2!("deduplicate_lines", "文件行去重 — 移除重复行", "path", "string", "文件路径", "ignore_case", "boolean", "是否忽略大小写"),
        t4!("sort_lines", "文本排序 — 支持字母/数值/逆序/去重", "path", "string", "文件路径", "reverse", "boolean", "是否逆序", "numeric", "boolean", "是否数值排序", "unique", "boolean", "是否同时去重"),
        t2!("word_frequency", "词频统计 — 统计文本中单词出现频率", "path", "string", "文件路径", "top_n", "integer", "返回前N个高频词(默认20)"),
        t1!("text_stats", "文本统计 — 行数/词数/字符数/字节数", "path", "string", "文件路径"),
        t2!("split_file", "分割大文件为多个分块", "path", "string", "文件路径", "chunk_size_mb", "integer", "每块大小(MB,默认10)"),
        t1!("edit_file", "内置文本编辑器 — 打开系统默认编辑器编辑文件(notepad/vim/nano)。阻塞等待编辑器关闭后返回", "path", "string", "文件路径"),
        t3!("hex_edit", "二进制编辑器 — 查看十六进制转储或按偏移量修改字节。offset+hex_bytes=修改模式, 无参数=显示模式", "path", "string", "文件路径", "offset", "integer", "偏移量(十进制,可选,如0x1A4需转成十进制420)", "hex_bytes", "string", "十六进制字节(可选,空格分隔,如\"FF AE 01\")"),
        t4!("compile_rust", "Rust编译 — rustc编译.rs文件或cargo build目录项目", "source", "string", "源文件路径(.rs)或Cargo项目目录", "output", "string", "输出二进制路径(单文件编译可选)", "release", "boolean", "是否release模式优化编译", "extra_args", "string", "额外rustc/cargo参数(逗号分隔)"),
        t1!("compiler_list", "列出可用编译器 — 检测系统中已安装的编译器及版本", "dummy", "string", "占位(可留空)"),
        t4!("compiler_compile", "多语言编译 — 支持rust/c/cpp/go/java/cs/python/asm/zig/auto", "language", "string", "语言: rust/c/cpp/go/java/cs/python/asm/zig/auto", "source", "string", "源文件路径", "output", "string", "输出路径(可选,auto=自动命名)", "optimize", "boolean", "是否优化编译(Release/-O2)"),

        // ═══ 高级文件操作 (v5.1 新增) ═══
        t2!("tail_file", "读取文件末尾N行 — 类似tail -n命令, 查看日志文件最新内容", "path", "string", "文件路径", "lines", "integer", "读取行数(默认10)"),
        t2!("head_file", "读取文件开头N行 — 类似head -n命令, 快速预览文件头部", "path", "string", "文件路径", "lines", "integer", "读取行数(默认10)"),
        t2!("diff_files", "文件差异比较 — 逐行比较两个文件, 输出不同之处", "file1", "string", "第一个文件路径", "file2", "string", "第二个文件路径"),
        t3!("insert_lines", "在指定行号插入内容 — 行号从1开始, 0=文件开头", "path", "string", "文件路径", "line", "integer", "行号(0=开头, 超过末尾=末尾)", "content", "string", "要插入的文本内容(可含换行)"),
        t3!("delete_lines_range", "删除指定行范围 — 删除[start,end]行(从1开始)", "path", "string", "文件路径", "start", "integer", "起始行号(含)", "end", "integer", "结束行号(含)"),
        t2!("truncate_file_lines", "截断文件至N行 — 保留前N行, 删除其余内容", "path", "string", "文件路径", "lines", "integer", "保留的行数"),
        t2!("merge_files", "合并多个文件 — 将多个源文件按顺序拼接写入目标文件", "sources", "string", "源文件路径(逗号分隔)", "target", "string", "目标文件路径"),
        t3!("grep_recursive", "递归搜索目录 — 在目录下所有文本文件中搜索匹配行(类似grep -r)", "pattern", "string", "搜索模式", "dir", "string", "目录路径", "case_insensitive", "boolean", "是否忽略大小写"),
        t4!("batch_replace", "批量查找替换 — 在目录下匹配文件模式的文件中批量替换文本", "pattern", "string", "搜索模式", "dir", "string", "目录路径", "find", "string", "要查找的文本", "replace", "string", "替换为的文本"),
        t1!("file_permissions", "查看文件权限 — Unix显示rwx八进制, Windows显示只读/隐藏/系统属性", "path", "string", "文件路径"),
        t3!("binary_read", "读取二进制数据 — 从文件指定偏移量读取N字节, 返回十六进制+ASCII转储", "path", "string", "文件路径", "offset", "integer", "偏移量(字节)", "length", "integer", "读取长度(字节, 最大4096)"),
        t1!("count_lines", "统计文件行数 — 快速高效地统计大文件行数和大小", "path", "string", "文件路径"),

        // ═══ AI 专属明文存储 (v4.5) ═══
        t2!("ai_save", "AI专属明文存储 — 将重要信息保存到 ai_storage/ 目录下的明文文件(不加密,便于跨会话读取)", "filename", "string", "文件名(不含路径,如 target_notes.txt)", "content", "string", "要存储的文本内容"),
        t1!("ai_read", "AI专属明文读取 — 从 ai_storage/ 目录读取已保存的明文文件", "filename", "string", "文件名(不含路径,如 target_notes.txt)"),
        t0!("ai_list", "AI专属存储列表 — 列出 ai_storage/ 目录下所有已保存的文件及大小"),

        // ═══ Vault 加密数据库 ═══
        t3!("vault_set", "Vault加密存储 — 安全保存值到加密数据库(命名空间:ai/tools/system)", "namespace", "string", "命名空间(ai/tools/system)", "key", "string", "键名(支持点路径如api_keys.openai)", "value", "string", "值(API Key等敏感信息自动加密)"),
        t2!("vault_get", "Vault加密读取 — 从加密数据库读取值", "namespace", "string", "命名空间", "key", "string", "键名"),
        t1!("vault_list", "Vault列出键 — 列出命名空间下所有键(不显示值)", "namespace", "string", "命名空间"),
        t1!("vault_list_ns", "Vault命名空间 — 列出所有命名空间", "dummy", "string", "占位(可留空)"),
        t3!("vault_remember", "AI记忆存储 — 存储AI长期记忆到加密Vault", "key", "string", "记忆键名", "value", "string", "记忆内容", "category", "string", "分类(target/preference/finding/note)"),
        t1!("vault_recall", "AI记忆召回 — 从加密Vault读取AI记忆", "key", "string", "记忆键名"),

        // ═══ 插件/脚本/驱动 (AI 可调用) ═══
        t2!("plugin_load", "加载原生插件DLL/SO — 持久加载到全局管理器。通过C ABI调用ruoo_plugin_init+exec，自动探测并注册ruoo_plugin_commands。⚠ 插件可完全突破沙箱", "name", "string", "插件别名(用于后续调用)", "path", "string", "插件文件路径(.dll/.so)"),
        t2!("plugin_call", "调用已加载插件 — 通过全局PluginManager执行插件命令+30s超时", "name", "string", "插件别名", "command", "string", "传给插件的命令字符串"),
        t1!("plugin_unload", "卸载已加载插件 — 同时注销所有已注册的命令", "name", "string", "插件别名"),
        t0!("plugin_list", "列出所有已加载插件 — 显示详情+已注册命令"),
        // ── 插件防崩溃 v5.0 ──
        t1!("plugin_force_unload", "强制卸载插件 — 跳过shutdown直接drop库, 用于崩溃插件强制清理, 防止终端崩溃", "name", "string", "插件别名"),
        t1!("plugin_crash_recover", "插件崩溃恢复 — 自动诊断故障插件+生成详细CrashReport+强制卸载, AI可用来恢复崩溃的插件", "name", "string", "插件别名"),
        t1!("plugin_reload", "热重载插件 — 卸载旧版→加载新版(同路径), 失败自动回滚。用于更新插件DLL后重新加载", "name", "string", "插件别名"),
        t1!("plugin_health", "插件健康检查 — 返回HEALTHY/DEGRADED/FAULTED状态+崩溃次数", "name", "string", "插件别名(空=全部)"),
        // ── 插件注册表 v5.0 ──
        t4!("plugin_reg", "注册插件到加密数据库 — 持久化存储, 启动时自动加载。注册表AES-256-GCM加密, 只能通过程序内命令/AI修改", "alias", "string", "插件别名", "path", "string", "插件文件路径(.dll/.so)", "auto_load", "boolean", "启动时自动加载(true/false)", "load_order", "integer", "加载优先级(数字越小越先加载, 默认0)"),
        t1!("plugin_unreg", "从注册表注销插件 — 同时保留插件文件", "alias", "string", "插件别名"),
        t1!("plugin_enable", "启用注册表中的插件 — 设置auto_load=true, 下次启动自动加载", "alias", "string", "插件别名"),
        t1!("plugin_disable", "禁用注册表中的插件 — 设置auto_load=false, 保留注册信息但不自动加载", "alias", "string", "插件别名"),
        t0!("plugin_reg_list", "列出插件注册表所有条目 — 含路径/版本/启用状态/加载次数/SHA-256"),
        t0!("plugin_reg_save", "手动保存插件注册表到磁盘 — 正常退出时自动保存, 此命令用于中途持久化"),
        t1!("script_run", "执行.ruoo脚本 — 读取并执行ruoo脚本(支持插件加载/驱动加载/命令执行)  ⚠ 脚本权限由#![allow-*]指令控制", "script_path", "string", "脚本文件路径(如scripts/scan.ruoo)"),
        t2!("sysload", "加载内核驱动 — Windows(.sys)通过sc.exe / Linux(.ko)通过insmod  ⚡ 最高危险级:需管理员/root + #![allow-sys]", "name", "string", "驱动服务名", "path", "string", "驱动文件路径(.sys/.ko)"),
        t1!("sysunload", "卸载内核驱动 — Windows(sc stop+delete) / Linux(rmmod)  ⚡ 需管理员/root", "name", "string", "驱动服务名/模块名"),
        t0!("syslist", "列出已加载内核驱动 — 本会话追踪 + 系统实时查询"),
        t2!("kernel_compile", "编译内核驱动源码 — 调用WDK/Make生成.sys/.ko。用户自选编译工具链", "source", "string", "C/Rust源文件路径", "output", "string", "输出驱动文件路径(.sys/.ko)"),
        t2!("kernel_template", "生成内核驱动代码模板 — Windows WDM / Linux Kernel Module 骨架代码", "name", "string", "驱动名称", "platform", "string", "目标平台(windows/linux)"),
        t1!("kernel_validate", "验证驱动文件 — 检查扩展名/签名/文件完整性", "path", "string", "驱动文件路径(.sys/.ko)"),
        t0!("kernel_backend_info", "查看内核驱动后端信息 — 当前后端/平台/支持特性"),
        t3!("kernel_scaffold", "生成完整驱动项目骨架 — 源码+Makefile/build.bat+README一键创建", "name", "string", "驱动项目名", "output_dir", "string", "输出目录路径", "platform", "string", "目标平台(windows/linux)"),
        t2!("exec_cmd", "⚡【执行系统命令】— Windows(cmd /C) / Linux(sh -c)。输出stdout+stderr+退出码。超时可配(默认120s, config set exec_timeout_ms)。仅perm 5可用！", "command", "string", "要执行的命令(cargo build/dir/git/ipconfig等)", "shell", "string", "Shell选择: cmd/powershell/bash/sh(可选,默认自动)"),

        // ═══ 文件操作扩展 v5.9 ═══
        t2!("base64_file", "文件Base64 — 编码/解码文件到Base64", "path", "string", "文件路径", "operation", "string", "encode 或 decode"),
        t1!("file_encoding_detect", "编码检测 — 检测文本编码(BOM/UTF-8/UTF-16/ASCII/二进制)", "path", "string", "文件路径"),
        t2!("json_query", "JSON查询 — 点路径提取JSON值(key.sub.0)", "path", "string", "JSON文件路径", "query", "string", "点分隔路径(如 windows.0.url)"),
        t1!("json_keys", "JSON键列表 — 列出对象所有键及类型预览", "path", "string", "JSON文件路径"),
        t3!("csv_parse", "CSV解析 — 解析CSV文件,显示表头+前N行", "path", "string", "文件路径", "max_rows", "integer", "最大行数(默认20)", "delimiter", "string", "分隔符(默认逗号)"),
        t4!("batch_rename", "批量重命名 — 正则替换文件名", "dir", "string", "目录路径", "find", "string", "正则查找模式", "replace", "string", "替换文本", "dry_run", "boolean", "试运行(true=仅预览)"),
        t3!("temp_file", "临时文件 — 在系统temp目录创建临时文件", "prefix", "string", "前缀(默认ruoo_)", "suffix", "string", "后缀(默认.tmp)", "content", "string", "初始内容(可选)"),
        t1!("symlink_info", "符号链接信息 — 检查路径是否为symlink/junction及其目标", "path", "string", "文件路径"),
        t4!("file_slice", "文件切片 — 提取字节范围到新文件", "path", "string", "源文件", "start", "integer", "起始偏移(字节)", "end", "integer", "结束偏移(字节)", "output", "string", "输出路径(可选)"),
        t3!("lines_sample", "行采样 — 从文件随机等距采样N行", "path", "string", "文件路径", "count", "integer", "采样数", "seed", "integer", "随机种子(默认0)"),
        t2!("empty_files", "空文件查找 — 递归查找空文件和空目录", "dir", "string", "起始目录", "recursive", "boolean", "是否递归"),
        t3!("file_concat", "文件拼接 — 合并多个文件(可选分隔符)", "sources", "string", "源文件(逗号分隔)", "output", "string", "输出路径", "add_separator", "boolean", "是否添加分隔符"),
        t3!("du_sorted", "目录排序 — 按大小排序显示目录/文件占用", "dir", "string", "目录路径", "depth", "integer", "递归深度(默认1)", "top_n", "integer", "显示前N项(默认20)"),
        // ═══ 文件剪贴板 v5.9 ═══
        t2!("file_copy_content", "复制文件内容 — 读取文件内容到剪贴板", "path", "string", "文件路径", "max_size_kb", "integer", "最大KB(默认1024)"),
        t2!("file_paste_content", "粘贴到文件 — 剪贴板文本写入文件(可追加)", "path", "string", "目标路径", "append", "boolean", "追加模式(true=追加)"),
        t1!("file_copy_path", "复制路径 — 文件绝对路径到剪贴板", "path", "string", "文件路径"),
        t2!("file_copy_base64", "复制Base64 — 文件编码为Base64到剪贴板", "path", "string", "文件路径", "max_size_mb", "integer", "最大MB(默认10)"),
        t1!("file_paste_base64", "粘贴Base64 — 剪贴板Base64解码到文件", "path", "string", "目标路径"),
        t1!("file_cut", "剪切文件 — 记录到剪切缓冲+复制路径到剪贴板", "path", "string", "文件路径"),
        t1!("file_paste_cut", "粘贴剪切 — 移动剪切缓冲中的文件到目标", "dest_dir", "string", "目标目录或路径"),
        t0!("file_cut_status", "剪切状态 — 查看剪切缓冲区内容"),
        t1!("file_copy_multi", "批量复制路径 — 多个文件路径(逗号分隔)到剪贴板", "paths", "string", "逗号分隔的文件路径"),

        // ═══ AI 辅助 ═══
        t1!("ai_message", "AI信息返回 — 中途输出信息到终端，支持\\n换行符", "message", "string", "要输出的消息文本，支持\\n换行"),

        // ═══ 工具联动引擎 v1.0 ═══
        t1!("workflow_list", "列出预定义工作流 — net/web/full/vuln/email/cve", "dummy", "string", "占位(可留空)"),
        t2!("workflow_run", "执行工作流 — 返回工具链命令列表,依次执行", "name", "string", "工作流名称(net/web/full/vuln/email/cve)", "target", "string", "目标(域名或IP)"),
        t1!("tool_suggest", "智能建议 — 根据上次工具结果推荐下一步", "dummy", "string", "占位(可留空)"),
        t1!("cache_stats", "结果缓存统计 — 查看工具联动缓存状态", "dummy", "string", "占位(可留空)"),

        // ═══ 大文件流式操作 (v4.8) — 不加载到内存 ═══
        t4!("stream_grep", "流式搜索大文件 — BufReader逐行搜索不加载到内存。支持上下文、大小写不敏感、最大匹配数", "path", "string", "文件路径", "pattern", "string", "搜索文本", "case_insensitive", "boolean", "是否忽略大小写", "max_matches", "integer", "最大匹配数(默认100)"),
        t3!("stream_binary_scan", "流式二进制扫描 — 64KB分块搜索字节模式，支持海量文件。HEX模式如\"FF AE 01\"", "path", "string", "文件路径", "hex_pattern", "string", "十六进制模式(如FF AE 01)", "max_results", "integer", "最大结果数(默认50)"),
        t3!("range_read", "按字节范围读取 — 大文件任意偏移精准读取，上限10MB。自动判断文本/二进制", "path", "string", "文件路径", "offset", "integer", "起始偏移(字节)", "length", "integer", "读取长度(字节, 上限10MB)"),
        t4!("stream_find_replace", "流式查找替换 — 边读边写临时文件不加载到内存，完成后原子替换。支持最大替换次数", "path", "string", "文件路径", "find", "string", "要查找的文本", "replace", "string", "替换为的文本", "case_insensitive", "boolean", "是否忽略大小写"),
        t1!("build_line_index", "构建行偏移索引 — 为大文本文件生成.idx二进制索引，加速随机行访问", "path", "string", "文件路径"),
        t3!("seek_line", "索引定位读行 — 使用.idx索引文件快速定位读取指定行(需先build_line_index)", "path", "string", "文件路径", "line_start", "integer", "起始行号(从1开始)", "line_end", "integer", "结束行号(可选,默认=line_start)"),
        t1!("query_line_index", "查询行索引信息 — 查看索引文件状态/行数/大小", "path", "string", "文件路径"),
        t1!("stream_file_info", "流式文件信息 — 不加载到内存统计大文件的行数/空行/行长/非ASCII比例", "path", "string", "文件路径"),

        // ═══ 大文件精准操作 (v4.9) — 正则搜索/字节修改/多模式替换/列提取/差异 ═══
        t4!("regex_search", "正则搜索大文件 — 流式逐行正则匹配不加载到内存。支持捕获组显示、上下文、大小写不敏感", "path", "string", "文件路径", "regex_pattern", "string", "正则表达式(如'error\\d+')", "case_insensitive", "boolean", "是否忽略大小写", "max_matches", "integer", "最大匹配数(默认100)"),
        t3!("byte_range_replace", "按字节偏移精确替换 — 使用HEX字符串替换指定偏移的字节。原子操作(备份→替换→回滚安全)", "path", "string", "文件路径", "offset", "integer", "字节偏移(十进制)", "hex_bytes", "string", "替换HEX字节(如\"FF AE 01\")"),
        t4!("stream_multi_replace", "流式多模式批量替换 — 一次遍历执行多个find→replace对。JSON格式: [[\"find1\",\"rep1\"],...]", "path", "string", "文件路径", "pairs_json", "string", "JSON替换对数组", "case_insensitive", "boolean", "是否忽略大小写", "max_replacements", "integer", "最大总替换数(默认10000)"),
        t4!("search_export", "搜索结果导出到文件 — 流式搜索大文件，匹配行+上下文写入输出文件", "path", "string", "文件路径", "pattern", "string", "搜索文本", "output_path", "string", "输出文件路径", "case_insensitive", "boolean", "是否忽略大小写"),
        t4!("column_extract", "CSV/TSV流式列提取 — 不加载整个文件，提取指定列。分隔符: comma/tab/pipe/semicolon/space", "path", "string", "文件路径", "columns", "string", "列索引(逗号分隔,从0开始,如\"0,2,5\")", "delimiter", "string", "分隔符(comma/tab/pipe/semicolon/space)", "max_rows", "integer", "最大行数(默认500)"),
        t3!("binary_diff", "大文件二进制差异比较 — 64KB分块流式比较两个文件，输出前N个差异的偏移+HEX", "file1", "string", "第一个文件路径", "file2", "string", "第二个文件路径", "max_diffs", "integer", "最大差异数(默认50)"),
        t3!("file_chunk_scan", "分块扫描大文件 — 按固定大小(MB)分块显示每块偏移/类型/可打印率/预览。快速了解大文件结构", "path", "string", "文件路径", "chunk_size_mb", "integer", "块大小(MB,默认64)", "max_chunks", "integer", "最大扫描块数(默认50)"),
    ]
}
// ── AI 记忆自动召回 (v4.1) ──
pub fn build_memory_context() -> String {
    let mut ctx = String::new();
    if let Some(keys) = crate::vault::vault_list("ai") {
        for key in keys {
            if key.starts_with("memory.") {
                if let Some(val) = crate::vault::vault_get("ai", &key) {
                    ctx.push_str(&format!("  [记忆:{}] {}\n", key, val));
                }
            }
        }
    }
    if ctx.is_empty() {
        String::new()
    } else {
        format!("## 用户长期记忆 (来自加密Vault)\n{}", ctx)
    }
}