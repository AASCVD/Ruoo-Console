# RUOO-CONSOLE v4.1

终端原生面板。TUI 控制台，内置 AI 助手、插件系统、加密保险库，200+ 工具集 — 全部运行在你的终端里。

## 这是什么

RUOO-CONSOLE 是一个用 Rust 编写的单二进制 TUI 终端面板。它将命令驱动的工具集与 AI 助手 Zero-Day 融为一体，AI 可以自主调用工具、加载插件、执行脚本、管理内核驱动。所有操作都在 ratatui 驱动的终端界面中完成，带有矩阵雨特效、侧边栏、进度条和实时命令输出。

AI 助手 Zero-Day 常驻控制台内部。它读取你的系统状态、运行工具、编写代码、迭代直到任务完成。用 `&` 前缀与它对话 — 就像身边有个随叫随到的高级运维。

## 快速开始

### 环境要求

Rust 工具链 (stable)。通过 rustup 安装：

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 构建与运行

```
git clone <仓库地址> ruoo-console
cd ruoo-console
cargo build --release
target/release/ruoo-console.exe
```

或者直接双击 `run.bat`（Windows）— 自动完成构建。

`quick.bat` 在已有 exe 时跳过构建，直接启动。

### 首次启动

首次运行需要创建主密码。此密码用于解锁 AES-256-GCM 加密的 Vault 保险库，API Key、凭证、AI 记忆均存储其中。密码强度强制要求 — 12 位以上，包含大小写字母、数字和特殊字符。

## 核心能力

### AI 助手 Zero-Day

用 `&` 前缀与 Zero-Day 对话：

```
& 帮我编写一个计算器
& 分析这一段数据
& 帮我写一个局域网ip扫描器
```

AI 可以调用所有工具 — 链式调用、读取文件、编译代码、加载插件、执行脚本 — 全自主完成。权限等级 (0-5) 限制其能力边界。

流式模式（工具调用实时输出）用 `&%` 前缀，实时逐字输出。

### 插件系统

通过 C ABI 加载原生 DLL/SO 插件。插件注册的命令自动注入统一命令池 — 用户和 AI 无差别调用。

```
plugin load scanner scanner.dll
plugin call scanner port_scan --target 192.168.1.1 --ports 1-1000
plugin reg scanner scanner.dll true 0    # 持久化 + 自动加载
```

完整插件 SDK：进度条、Vault 访问、消息代理、插件间通信总线、熔断器、崩溃恢复。详见 [PLUGIN_DEV_GUIDE.md](PLUGIN_DEV_GUIDE.md)（75KB 深度指南）。

### 加密保险库 Vault

AES-256-GCM 加密数据库，命名空间隔离：`api_keys`、`ai`、`tools`、`system`。

```
vault set api_keys openai sk-abc123
vault get openai
vault list api_keys
vault remember target 192.168.1.0/24 "内网网段，域控在 .10"
```

可配置超时自动锁定。PBKDF2-HMAC-SHA256 密钥派生，60 万次迭代。已删除条目 7 遍安全擦除。

### 脚本引擎

`.ruoo` 脚本，带权限沙箱。脚本可加载插件、执行命令、访问 Vault。

```
script new scan_lan.ruoo
script edit scan_lan.ruoo
run scan_lan.ruoo
```

SHA-256 白名单机制：`script permit scan_lan.ruoo`。

### 内核驱动

加载和管理 Windows `.sys` / Linux `.ko` 内核驱动。需要管理员/root 权限。

```
sysload mydriver target/release/mydriver.sys
syslist
sysunload mydriver
```

模板生成：`ktemplate mydriver windows` / `kscaffold mydriver ./mydriver windows`。

### 文件操作工具集

200+ 文件操作 — 读、写、搜索、哈希、编码、差异、合并、切片、粉碎、重命名等等。大文件全程流式处理，不加载到内存。

亮点：

```
stream_grep   access.log "ERROR"                 # 秒搜 10GB 日志
regex_search  dump.csv "\d{16}"                  # 查找信用卡号模式
hexfind       firmware.bin "FF AE 01"            # 二进制模式搜索
shred         secrets.txt 7 dod                  # DoD 5220.22-M 标准擦除
hexedit       binary.exe 0x1A4 "90 90"           # 二进制字节补丁
dirdiff       dir1 dir2                          # 目录差异对比
```

输入 `help all` 或按 F9 查看完整命令参考。

### 编译器集成

直接在终端中编译 Rust、C、C++、Go、Java、C#、Python、ASM、Zig。

```
compile rust mytool.rs --release
compile c exploit.c -o exploit.exe
```

## 命令分类

| 分类 | 数量 | 示例 |
|---|---|---|
| 核心命令 | 29 | help, exec, config, status, clear, dpai |
| 文件操作 | 41 | read_file, write_file, grep, hexdump, shred, stream_grep |
| 插件扩展 | 15 | plugin load/call/unload/reg/health |
| 脚本引擎 | 8 | run, script new/edit/permit |
| 内核驱动 | 8 | sysload, sysunload, ktemplate, kvalidate |
| Vault保险库 | 10 | vault set/get/list/remember/recall |
| AI数据 | 3 | aisave, airead, ailist |
| 启动脚本 | 5 | bootscript edit/run/reset/clear |
| 网络工具 | 8 | httpget, httppost, port_scan, dns_lookup, whois |
| 编译器 | 7 | compile, clist, build (rust/c/cpp/go/java/cs/python/asm/zig) |

使用 `help` 或 F9 进入交互式帮助浏览器，共 9 页分屏显示。

## 键盘快捷键

| 按键 | 功能 |
|---|---|
| F1 | 切换侧边栏 |
| F2 | 切换 AI 模式 |
| F3 | 右侧栏模式切换 (仪表盘 / 系统详情) |
| F4 | 窗口显隐切换 |
| F9 | 帮助浏览器 |
| Ctrl+C | 退出 |
| Tab | 命令自动补全 |
| 上下箭头 | 命令历史导航 |
| PgUp/PgDn | 输出滚动 |
| Home/End | 跳转到顶部/底部 |

## 项目结构

```
ruoo-console/
├── src/
│   ├── main.rs              # 入口，TUI 事件循环，应用状态
│   ├── ui.rs                # 终端渲染 (ratatui)
│   ├── config.rs            # 配置系统 (JSON 持久化)
│   ├── auth.rs              # 登录认证，密码强度检测
│   ├── vault.rs             # AES-256-GCM 加密数据库
│   ├── plugin.rs            # 插件系统核心，命令注册表，AI 工具注册
│   ├── plugin_registry.rs   # 加密插件注册表 (持久化存储)
│   ├── plugin_vault.rs      # 插件可访问的 Vault (AES-256-GCM + 安全擦除)
│   ├── plugin_progress.rs   # 多进度条管理器 (C ABI)
│   ├── message_broker.rs    # 结构化消息，速率限制，插件间通信
│   ├── memsec.rs            # 内存安全 SecStr (XOR 加密 + mlock + volatile 零化)
│   ├── panic_guard.rs       # 全局崩溃防护，脱敏崩溃日志
│   ├── tabs.rs              # 标签页管理
│   ├── clipboard.rs         # 剪贴板集成 (复制/粘贴/剪切)
│   ├── shortcuts.rs         # 键盘快捷键处理
│   ├── session.rs           # 会话状态
│   ├── window.rs            # 窗口显隐管理
│   ├── crypto_channel.rs    # 加密通信通道
│   ├── telemetry.rs         # 工具执行遥测统计
│   ├── theme.rs             # 配色主题
│   ├── utils.rs             # 工具函数
│   ├── bootscript.rs        # 启动脚本执行
│   ├── commands/
│   │   ├── mod.rs           # 命令调度器，异步执行
│   │   ├── mod_meta.rs      # 统一命令注册表 (全部 200+ 命令)
│   │   └── help.rs          # 交互式帮助浏览器
│   ├── tools/
│   │   ├── mod.rs           # 工具模块入口
│   │   ├── files.rs         # 文件操作 (120KB，200+ 函数)
│   │   ├── compiler.rs      # 多语言编译器集成
│   │   ├── crypto.rs        # 加密运算
│   │   ├── net.rs           # 网络工具 (扫描/DNS/HTTP/whois)
│   │   ├── web.rs           # Web 工具
│   │   ├── gen.rs           # 生成器 (UUID/密码/哈希)
│   │   ├── kernel.rs        # 内核驱动操作
│   │   ├── fault_tolerant.rs # 容错与错误恢复
│   │   └── orchestrator.rs  # 工具编排
│   ├── ai/
│   │   ├── mod.rs           # AI 模块入口
│   │   ├── deepseek.rs      # DeepSeek API 客户端
│   │   ├── executor.rs      # 工具执行引擎 (82KB，200+ 工具)
│   │   ├── prompt.rs        # 系统提示词构建，工具定义
│   │   ├── session.rs       # AI 会话管理
│   │   ├── types.rs         # AI 类型与通道定义
│   │   ├── globals.rs       # 全局状态 (插件管理器，中断标志等)
│   │   ├── cache.rs         # 工具结果缓存 (1小时 TTL)
│   │   ├── permissions.rs   # 权限系统与 Shell 注入防护
│   │   ├── markdown.rs      # Markdown 剥离，适配 TUI 渲染
│   │   ├── macros.rs        # 工具定义宏 (t0..t4)
│   │   └── misc.rs          # 解析工具
│   └── script/
│       ├── mod.rs           # 脚本引擎核心
│       ├── interpreter.rs   # .ruoo 脚本解释器
│       ├── permissions.rs   # 脚本权限沙箱
│       ├── plugin_mgr.rs    # 持久化 PluginManager
│       ├── plugin_crash.rs  # 崩溃恢复与诊断
│       ├── plugin_stderr.rs # 插件 stderr 捕获
│       └── sysdrv.rs        # 系统驱动管理
├── scripts/                 # .ruoo 脚本目录
├── tools/                   # 外部工具
├── ai_storage/              # AI 明文存储
├── vault_data/              # 加密 Vault 文件
├── backup/                  # 备份目录
├── result/                  # 结果输出
├── temporary files/         # 临时文件工作区
├── junk files/              # 废弃文件
├── downloads/               # 下载目录
├── PLUGIN_DEV_GUIDE.md      # 完整插件开发指南 (75KB)
├── system_prompt.txt         # AI 系统提示词模板
├── build.rs                 # 构建脚本 (版本信息，Windows 资源)
├── Cargo.toml               # 项目清单
├── run.bat                  # 构建 + 启动 (Windows)
└── quick.bat                # 快速启动 (跳过构建)
```

## 配置项

配置存储在可执行文件同目录下的 `arsenal_config.json`：

| 配置项 | 默认值 | 说明 |
|---|---|---|
| theme_name | cyber | TUI 配色主题 |
| font_size | 14 | 终端字体大小参考 |
| timeout_ms | 30000 | 默认工具超时 |
| connect_timeout_ms | 3000 | TCP 连接超时 |
| exec_timeout_ms | 120000 | 系统命令超时 |
| port_range | 1-10000 | 默认端口扫描范围 |
| max_history | 500 | 命令历史容量 |
| max_output_lines | 10000 | 输出缓冲区上限 |
| deepseek_api_url | api.deepseek.com/v1 | AI API 地址 |
| deepseek_model | deepseek-v4-pro | AI 模型 |
| ai_enabled | false | AI 开关 |

使用 `config` 查看当前配置，`config set <键> <值>` 修改。

## 安全机制

内存安全：敏感字符串使用 XOR 加密的 `SecStr` 存储，配合 mlock/VirtualLock 阻止换出到磁盘。Drop 时 volatile 零化，编译器无法优化消除。

Vault 保险库：AES-256-GCM 认证加密。PBKDF2-HMAC-SHA256 密钥派生，60 万次迭代。可配置超时自动锁定。删除条目 7 遍覆写（随机 + 归零）。

插件沙箱：每插件可配置独立沙箱 — 网络白名单/黑名单、文件系统路径限制、内存上限、子进程数量限制、执行超时。

崩溃防护：全局 panic hook 捕获所有崩溃，脱敏处理堆栈回溯，写入 `%TEMP%/ruoo_crash.log`。单个工具 panic 被隔离捕获，不会拖垮整个控制台。

API Key 保护：首次使用自动迁移到 Vault 加密存储。明文配置回退时显示安全警告。

## 平台支持

主力平台：Windows (x86_64)。部分功能（内核驱动、ADS 备用数据流）为 Windows 专属。

代码库可在 Linux 和 macOS 上编译。内核驱动支持在 Windows 使用 `sc.exe`，在 Linux 使用 `insmod`/`rmmod`。

## 关于杀毒软件报毒

没办法，可能运行的时候戳到杀毒软件的痒痒肉了，都是开源的，这边建议您加一个白名单呢qwq。

## 许可

本项目采用 [MIT 许可证](LICENSE)。

## 致谢

用 Rust、ratatui、crossterm 和无数深夜编码构建。

AI 助手：Zero-Day，正运行在你刚才读到的这个控制台里。
