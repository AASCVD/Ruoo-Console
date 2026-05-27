// ============================================================
// RUOO-ARSENAL v8.0 — 模块自治命令注册表 (内核清理完毕)
// 仅保留: 核心/插件/脚本/内核/启动/Vault/AI数据/文件操作/编译器
// ============================================================

/// 所有内置命令 — v8.0 模块自治聚合
pub fn all_commands() -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
    use std::sync::OnceLock;
    static ALL: OnceLock<&[(&str, &str, &str, &str)]> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<(&str, &str, &str, &str)> = Vec::with_capacity(256);
        v.extend_from_slice(CORE);
        v.extend_from_slice(PLUGIN);
        v.extend_from_slice(SCRIPT);
        v.extend_from_slice(KERNEL);
        v.extend_from_slice(BOOTSCRIPT);
        v.extend_from_slice(VAULT);
        v.extend_from_slice(AI_DATA);
        v.extend_from_slice(FILE_OPS);
        v.extend_from_slice(COMPILER);
        Box::leak(v.into_boxed_slice())
    })
}

// (name, syntax, desc, category)
type CmdDef = (&'static str, &'static str, &'static str, &'static str);

// ═══ 核心命令 (29) ═══
const CORE: &[CmdDef] = &[
    ("help / ?", "", "显示帮助 — help 1~9分页, help <分类>过滤, help all全览(F9)", "核心命令"),
    ("dpai", "", "切换AI助手", "核心命令"),
    ("&<消息>", "", "AI对话(普通)", "核心命令"),
    ("&%<消息>", "", "AI对话(实时)", "核心命令"),
    ("&+*", "", "任务流程表(添加/列表/清空/关闭)", "核心命令"),
    ("&+*list", "", "查看任务流程表", "核心命令"),
    ("&+*clear", "", "清空任务流程表", "核心命令"),
    ("&+*stop", "", "关闭任务流程表", "核心命令"),
    ("clear / cls", "", "清屏", "核心命令"),
    ("clear history", "", "AI对话历史清除", "核心命令"),
    ("clear cache", "", "清除工具缓存", "核心命令"),
    ("clear context", "", "清除AI上下文", "核心命令"),
    ("perm [0-5]", "", "设置AI权限", "核心命令"),
    ("status", "", "系统状态", "核心命令"),
    ("config", "", "查看/修改配置", "核心命令"),
    ("hide / show", "", "隐藏/恢复TUI窗口", "核心命令"),
    ("sysinfo", "", "系统硬件信息(OS/CPU/内存/磁盘)", "核心命令"),
    ("netstat", "", "进程列表(tasklist)", "核心命令"),
    ("lock / unlock", "", "锁定/解锁Vault", "核心命令"),
    ("exec <命令>", "", "执行系统命令(perm 5, 超时60s)", "核心命令"),
    ("abort / abort_tool", "", "强制中断卡住的AI工具调用", "核心命令"),
    ("edit <文件>", "", "编辑器打开文件(notepad/vim)", "核心命令"),
    ("save [文件]", "", "保存终端输出到文件", "核心命令"),
    ("ifconfig", "", "网络接口详情(IP/MAC/掩码/网关)", "核心命令"),
    ("df", "", "磁盘空间使用查询", "核心命令"),
    ("httpget <URL>", "", "HTTP GET请求(直接获取URL响应)", "核心命令"),
    ("httppost <URL> <body>", "", "HTTP POST请求(发送JSON/文本body)", "核心命令"),
    ("httpget-hdr / httpgeth <URL> <headers_JSON>", "", "HTTP GET+自定义Headers(Cookie/Referer/Auth等)", "核心命令"),
    ("httppost-hdr / httpposth <URL> <body> <headers_JSON>", "", "HTTP POST+自定义Headers+ContentType", "核心命令"),
];

// ═══ 插件扩展 (15) ═══
const PLUGIN: &[CmdDef] = &[
    ("plugin load <名> <路径>", "", "加载原生插件(.dll/.so)", "插件扩展"),
    ("plugin call <名> <命令>", "", "调用已加载插件函数", "插件扩展"),
    ("plugin unload <名>", "", "正常卸载插件(调用shutdown)", "插件扩展"),
    ("plugin list", "", "列出所有已加载插件", "插件扩展"),
    ("plugin hotload <名>", "", "手动热重载插件", "插件扩展"),
    ("plugin force-unload <名>", "", "强制卸载(跳过shutdown,直接drop库)", "插件扩展"),
    ("plugin crash-recover <名>", "", "崩溃恢复(自动诊断+强制卸载+详细报告)", "插件扩展"),
    ("plugin health <名>", "", "插件健康状态(HEALTHY/DEGRADED/FAULTED)", "插件扩展"),
    ("plugin health-all", "", "所有插件健康状态总览", "插件扩展"),
    ("plugin reg <别名> <路径> [auto_load] [load_order]", "", "注册插件到加密数据库(持久化)", "插件扩展"),
    ("plugin unreg <别名>", "", "从注册表注销插件", "插件扩展"),
    ("plugin enable <别名>", "", "启用注册表中的插件(自动加载)", "插件扩展"),
    ("plugin disable <别名>", "", "禁用注册表中的插件(保留注册信息)", "插件扩展"),
    ("plugin reg-list", "", "查看注册表所有条目", "插件扩展"),
    ("plugin reg-save", "", "手动保存注册表到磁盘", "插件扩展"),
];

// ═══ 脚本引擎 (8) ═══
const SCRIPT: &[CmdDef] = &[
    ("run <脚本.ruoo>", "", "执行.ruoo脚本", "脚本引擎"),
    ("run --force <脚本>", "", "跳过许可强制执行脚本", "脚本引擎"),
    ("script list", "", "列出所有脚本", "脚本引擎"),
    ("script new <名称>", "", "创建含示例的模板脚本", "脚本引擎"),
    ("script edit <名称>", "", "编辑器打开脚本", "脚本引擎"),
    ("script permit <名称>", "", "SHA-256白名单授权脚本", "脚本引擎"),
    ("script revoke <名称>", "", "撤销脚本许可", "脚本引擎"),
    ("script perms", "", "列出已许可脚本", "脚本引擎"),
];

// ═══ 内核驱动 (8) ═══
const KERNEL: &[CmdDef] = &[
    ("sysload <名称> <路径>", "", "加载内核驱动(.sys/.ko,需管理员)", "内核驱动"),
    ("sysunload <名称>", "", "卸载内核驱动", "内核驱动"),
    ("syslist", "", "列出已加载驱动", "内核驱动"),
    ("kinfo", "", "内核后端信息(平台/支持特性)", "内核驱动"),
    ("kcompile <源> <输出>", "", "编译驱动源码→.sys/.ko", "内核驱动"),
    ("ktemplate <名称> <平台>", "", "生成驱动代码模板(windows/linux)", "内核驱动"),
    ("kvalidate <路径>", "", "验证驱动文件格式(扩展名/签名)", "内核驱动"),
    ("kscaffold <名> <目录> <平台>", "", "生成完整驱动项目骨架", "内核驱动"),
];

// ═══ 启动脚本 (5) ═══
const BOOTSCRIPT: &[CmdDef] = &[
    ("bootscript", "", "查看启动脚本状态", "启动脚本"),
    ("bootscript edit", "", "加密编辑器修改启动脚本", "启动脚本"),
    ("bootscript reset", "", "恢复默认启动脚本", "启动脚本"),
    ("bootscript run", "", "手动执行启动脚本", "启动脚本"),
    ("bootscript clear", "", "删除启动脚本", "启动脚本"),
];

// ═══ Vault保险库 (10) ═══
const VAULT: &[CmdDef] = &[
    ("vault", "", "查看Vault状态(命名空间+条目数)", "保险库"),
    ("vault passwd", "", "修改Vault主密码", "保险库"),
    ("vault set <命名空间> <键> <值>", "", "加密存储键值对(ai/tools/system)", "保险库"),
    ("vault add <键> <值>", "", "存入加密键值对(默认命名空间)", "保险库"),
    ("vault get <键>", "", "读取解密后的值", "保险库"),
    ("vault del <键>", "", "删除指定条目", "保险库"),
    ("vault list [命名空间]", "", "列出命名空间下所有键", "保险库"),
    ("vault ns", "", "列出所有Vault命名空间", "保险库"),
    ("vault remember <分类> <键> <值>", "", "AI长期记忆存储", "保险库"),
    ("vault recall <键>", "", "AI长期记忆召回", "保险库"),
];

// ═══ AI数据存储 (3) ═══
const AI_DATA: &[CmdDef] = &[
    ("aisave <文件名> <内容>", "", "AI明文存储(保存到ai_storage/)", "AI数据"),
    ("airead <文件名>", "", "AI明文读取(从ai_storage/读取)", "AI数据"),
    ("ailist", "", "AI存储列表(所有已保存文件)", "AI数据"),
];

// ═══ 文件操作 (41) ═══
const FILE_OPS: &[CmdDef] = &[
    ("base64 <文件> encode|decode", "", "文件Base64编解码", "文件操作"),
    ("encdetect <文件>", "", "检测文本编码(BOM/UTF-8/UTF-16)", "文件操作"),
    ("jq <JSON> <查询>", "", "JSON点路径提取(如 windows.0.url)", "文件操作"),
    ("jkeys <JSON>", "", "列出JSON所有键及类型预览", "文件操作"),
    ("csv <文件> [行数] [分隔符]", "", "CSV解析预览(表头+前N行)", "文件操作"),
    ("rename <目录> <正则> <替换>", "", "批量正则重命名(--real执行)", "文件操作"),
    ("tempfile [前缀] [后缀] [内容]", "", "创建临时文件", "文件操作"),
    ("syminfo <路径>", "", "符号链接/junction信息", "文件操作"),
    ("slice <文件> <起始> <结束>", "", "提取字节范围到新文件", "文件操作"),
    ("sample <文件> <N> [种子]", "", "随机等距行采样N行", "文件操作"),
    ("empty <目录> [--flat]", "", "查找空文件和空目录", "文件操作"),
    ("concat <文件列表> [输出] [--sep]", "", "拼接多个文件", "文件操作"),
    ("dusort <目录> [深度] [前N]", "", "按大小排序目录占用", "文件操作"),
    ("copytext <文件> [最大KB]", "", "复制文件内容到剪贴板", "文件操作"),
    ("pastetext <文件> [--append]", "", "剪贴板文本写入文件", "文件操作"),
    ("copypath <文件>", "", "复制文件绝对路径到剪贴板", "文件操作"),
    ("hexedit <文件> [偏移] [HEX]", "", "二进制编辑器(查看/修改字节)", "文件操作"),
    ("copyb64 <文件> [最大MB]", "", "文件Base64编码到剪贴板", "文件操作"),
    ("pasteb64 <文件>", "", "剪贴板Base64解码到文件", "文件操作"),
    ("filecut <文件>", "", "剪切文件(记录到剪切缓冲区)", "文件操作"),
    ("filepaste <目标目录>", "", "粘贴剪切文件(移动缓冲区文件)", "文件操作"),
    ("cutstatus", "", "查看剪切缓冲区状态", "文件操作"),
    ("copymulti <文件1,文件2,…>", "", "批量复制文件路径到剪贴板", "文件操作"),
    ("hexdump <文件> [偏移] [字节]", "", "十六进制转储(hexdump格式)", "文件操作"),
    ("mime <文件>", "", "MIME类型检测(魔术字节识别)", "文件操作"),
    ("shred <文件> [遍数] [zero|random|dod]", "", "安全删除(多遍覆写,默认3遍zero)", "文件操作"),
    ("touch <文件>", "", "创建空文件或更新文件时间戳", "文件操作"),
    ("filetime <文件> [创建时间] [修改时间] [访问时间]", "", "设置文件时间戳(修改/访问/创建)", "文件操作"),
    ("adsread <文件> <流名>", "", "读取Windows NTFS备用数据流(ADS)", "文件操作"),
    ("adswrite <文件> <流名> <数据>", "", "写入Windows NTFS备用数据流(ADS)", "文件操作"),
    ("adslist <文件>", "", "列出文件所有ADS流", "文件操作"),
    ("dirdiff <目录1> <目录2>", "", "两目录差异对比(仅A/仅B/大小不同)", "文件操作"),
    ("hexfind <文件> <HEX模式> [最大结果]", "", "二进制HEX字节模式搜索(含上下文)", "文件操作"),
    ("health", "", "容错系统健康检查+故障统计", "文件操作"),
    ("regexsearch <文件> <正则> [大小写] [最大匹配] [上下文]", "", "流式正则搜索大文件(不加载到内存,支持捕获组)", "文件操作"),
    ("byterep <文件> <偏移> <HEX字节>", "", "按字节偏移精确替换(原子操作,自动备份回滚)", "文件操作"),
    ("multirep <文件> <JSON对> [大小写] [最大替换]", "", "流式多模式批量替换(一次遍历多个find→replace)", "文件操作"),
    ("searchexport <文件> <模式> <输出> [大小写] [上下文]", "", "搜索结果导出到文件(流式处理不爆内存)", "文件操作"),
    ("colex <文件> <列索引> [分隔符] [最大行]", "", "CSV/TSV流式列提取(comma/tab/pipe/semicolon)", "文件操作"),
    ("bindiff <文件1> <文件2> [最大差异]", "", "大文件二进制差异比较(64KB分块流式)", "文件操作"),
    ("chunkscan <文件> [块大小MB] [最大块数]", "", "分块扫描大文件(概览每块偏移/类型/可打印率)", "文件操作"),
];

// ═══ 编译器 (11) ═══
const COMPILER: &[CmdDef] = &[
    ("compilers", "", "列出可用编译器及版本(12种编译器自动探测)", "编译器"),
    ("compile <语言> <源文件> [选项]", "", "多语言编译: rust/c/cpp/go/java/cs(c#)/python/asm(nasm)/zig/auto", "编译器"),
    ("compile-rust / rustc <源> [输出] [--release]", "", "Rust编译(rustc/cargo build)", "编译器"),
    ("compile-c / cc / gcc / clang <源> [输出] [-O2]", "", "C编译(gcc/clang自动选择)", "编译器"),
    ("compile-cpp / cxx / cppc <源> [输出] [-O2]", "", "C++编译(g++/clang++自动选择,C++17)", "编译器"),
    ("compile-go / goc <源> [输出] [-O]", "", "Go编译(go build, -O=strip符号)", "编译器"),
    ("compile-java / javac <源> [输出]", "", "Java编译(javac→.class)", "编译器"),
    ("compile-cs / csc / dotnet-build <源> [输出] [-O]", "", "C#编译(csc/dotnet build自动选择)", "编译器"),
    ("compile-python / pyc <源>", "", "Python编译(py_compile→.pyc)", "编译器"),
    ("compile-asm / nasm <源> [输出]", "", "ASM汇编(NASM→.o,需链接器)", "编译器"),
    ("compile-zig / zigc <源> [输出] [-O]", "", "Zig编译(zig build-exe,支持交叉编译)", "编译器"),
];
