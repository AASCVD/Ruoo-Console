// ============================================================
// RUOO-ARSENAL v4.0 — 文件操作模块
// read/write/append/create/delete/copy/move
// list/search/grep/stat/hash/hexdump/mime
// mkdir/rmdir/cd/pwd/du
// split/dedup/sort/compress/wordcount
// ============================================================

use std::fs;
use std::io::{Write, BufReader, BufRead, Read, Seek};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

// ── 路径穿越警告缓冲区 — TUI下println!会破坏界面，改为缓冲后由上层统一收取 ──
static PATH_WARNINGS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> = std::sync::OnceLock::new();

fn path_warnings_lock() -> &'static std::sync::Mutex<Vec<String>> {
    PATH_WARNINGS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn warn_path_traversal(msg: String) {
    // ★ BUG-22 修复: 删除 eprintln! — 在 raw terminal 模式下
    //    eprintln! 会绕过 TUI 渲染管线，直接把文本喷到终端
    //    光标位置 (通常是输入框)，破坏 UI 布局。
    //    警告通过 PATH_WARNINGS 缓冲区 → execute_tool/_spawn 排空
    //    后正确进入输出区，不再需要 stderr 输出。
    if let Ok(mut buf) = path_warnings_lock().lock() {
        buf.push(msg);
    }
}

/// 排空并返回所有积压的路径穿越警告
pub fn drain_path_warnings() -> Vec<String> {
    match PATH_WARNINGS.get() {
        Some(lock) => match lock.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// ★ BUG-22: 排空并拼接 — 用于嵌入工具返回值
/// 调用后清空缓冲区，返回格式化警告字符串 (含前缀)
pub fn drain_path_warnings_string() -> String {
    let warnings = drain_path_warnings();
    if warnings.is_empty() {
        String::new()
    } else {
        warnings.join("\n")
    }
}

// ── 当前工作目录原子变量 ──
static CWD: std::sync::OnceLock<std::sync::RwLock<PathBuf>> = std::sync::OnceLock::new();

fn cwd() -> &'static std::sync::RwLock<PathBuf> {
    CWD.get_or_init(|| {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        std::sync::RwLock::new(dir)
    })
}

/// 沙箱根 — 进程启动时的工作目录，所有文件操作不得越界
fn sandbox_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| d.canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }).clone()
}

/// ★ BUG-21: 手动规范化路径组件 — 消解 .. 和 . 但不依赖 canonicalize (文件可能不存在)
fn normalize_path_components(raw: &Path) -> PathBuf {
    let mut components: Vec<&std::ffi::OsStr> = Vec::new();
    for comp in raw.components() {
        match comp {
            std::path::Component::ParentDir => {
                // 消解 .. — 若栈非空且栈顶不是 .. 则弹出
                if !components.is_empty() && components.last() != Some(&std::ffi::OsStr::new("..")) {
                    components.pop();
                } else if components.is_empty() {
                    // 根路径下的 .. 忽略 (无法越界)
                }
            }
            std::path::Component::CurDir => {
                // 跳过 .
            }
            other => {
                components.push(other.as_os_str());
            }
        }
    }
    let mut result = PathBuf::new();
    for c in components {
        result.push(c);
    }
    result
}

fn resolve(path: &str) -> PathBuf {
    let p = Path::new(path);
    let raw = if p.is_absolute() { p.to_path_buf() } else { cwd().read().unwrap().join(p) };

    // ★ BUG-2 修复: 路径穿越防护 v2 — 严格校验，canonicalize 失败直接拒绝
    let root = sandbox_root();

    match raw.canonicalize() {
        Ok(canon) => {
            if !canon.starts_with(&root) {
                warn_path_traversal(format!("[!] 路径穿越被拦截: {} → 拒绝访问 (沙箱: {})", path, root.display()));
                return root.join("__BLOCKED__");
            }
            canon
        }
        Err(_) => {
            // ★ BUG-21 修复: canonicalize 失败时 (如文件尚不存在),
            //   手动消解 raw 中的 .. 和 . 组件后再检查祖先
            let cleaned = normalize_path_components(&raw);
            let mut found_safe_ancestor = false;
            let mut ancestor = cleaned.clone();
            while let Some(parent) = ancestor.parent() {
                if parent.as_os_str().is_empty() { break; }
                ancestor = parent.to_path_buf();
                if ancestor.exists() {
                    if let Ok(canon) = ancestor.canonicalize() {
                        if canon.starts_with(&root) { found_safe_ancestor = true; }
                    }
                    break;
                }
            }
            if !found_safe_ancestor {
                warn_path_traversal(format!("[!] 路径穿越被拦截 (无安全祖先): {} → 拒绝访问", path));
                return root.join("__BLOCKED__");
            }
            cleaned
        }
    }
}

pub fn resolve_path(path: &str) -> String {
    resolve(path).display().to_string()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1}{}", size, UNITS[unit_idx])
}

fn format_ts(secs: u64) -> String {
    let t = secs;
    let s = t % 60; let m = (t / 60) % 60; let h = (t / 3600) % 24;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

// ═══ 目录操作 ═══

pub fn pwd() -> String { cwd().read().unwrap().display().to_string() }

pub fn cd(path: &str) -> Result<String, String> {
    let new = resolve(path);
    if !new.is_dir() { return Err(format!("不是目录: {}", new.display())); }
    let canonical = new.canonicalize().map_err(|e| format!("cd 失败: {}", e))?;
    *cwd().write().unwrap() = canonical.clone();
    Ok(format!("{}", canonical.display()))
}

pub fn ls(path: Option<&str>) -> Result<Vec<String>, String> {
    let dir = match path { Some(p) => resolve(p), None => cwd().read().unwrap().clone() };
    let mut entries: Vec<String> = Vec::new();
    let read = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in read {
        if let Ok(e) = entry {
            let name = e.file_name().to_string_lossy().to_string();
            let ft = e.file_type().map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("");
            let size = if ft.is_empty() { e.metadata().map(|m| m.len()).ok() } else { None };
            match size {
                Some(s) => entries.push(format!("{:<40} {:>10} {}", format!("{}{}", name, ft), format_bytes(s),
                    e.metadata().map(|m| {
                        let t = m.modified().map(|mt| {
                            let secs = mt.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                            format_ts(secs)
                        }).unwrap_or_default(); t
                    }).unwrap_or_default())),
                None => entries.push(format!("{}{}", name, ft)),
            }
        }
    }
    entries.sort_by(|a, b| {
        let a_dir = a.contains('/'); let b_dir = b.contains('/');
        b_dir.cmp(&a_dir).then(a.to_lowercase().cmp(&b.to_lowercase()))
    });
    Ok(entries)
}

pub fn mkdir(path: &str) -> Result<String, String> {
    let p = resolve(path);
    fs::create_dir_all(&p).map_err(|e| format!("mkdir 失败: {}", e))?;
    Ok(format!("已创建: {}", p.display()))
}

pub fn rmdir(path: &str) -> Result<String, String> {
    let p = resolve(path);
    if !p.exists() { return Err(format!("不存在: {}", p.display())); }
    if p.is_dir() { fs::remove_dir_all(&p).map_err(|e| format!("删除目录失败: {}", e))?; }
    else { fs::remove_file(&p).map_err(|e| format!("删除文件失败: {}", e))?; }
    Ok(format!("已删除: {}", p.display()))
}

pub fn du(path: Option<&str>) -> Result<String, String> {
    let dir = match path { Some(p) => resolve(p), None => cwd().read().unwrap().clone() };
    let (files, dirs, bytes) = walk_size(&dir);
    Ok(format!("{} — {} 文件, {} 目录, {}", dir.display(), files, dirs, format_bytes(bytes)))
}

fn walk_size(path: &Path) -> (usize, usize, u64) {
    let mut files = 0usize; let mut dirs = 0usize; let mut bytes = 0u64;
    fn walk(p: &Path, files: &mut usize, dirs: &mut usize, bytes: &mut u64) {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                if let Ok(ft) = e.file_type() {
                    if ft.is_dir() { *dirs += 1; walk(&e.path(), files, dirs, bytes); }
                    else { *files += 1; *bytes += e.metadata().map(|m| m.len()).unwrap_or(0); }
                }
            }
        }
    }
    walk(path, &mut files, &mut dirs, &mut bytes);
    (files, dirs, bytes)
}

// ═══ 文件读写 ═══

pub fn read_file(path: &str) -> Result<String, String> {
    let p = resolve(path);
    fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))
}

pub fn read_file_lines(path: &str, start: usize, end: Option<usize>) -> Result<String, String> {
    let p = resolve(path);
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let end = end.unwrap_or(lines.len()).min(lines.len());
    if start > end || start > lines.len() {
        return Err(format!("行号无效: {}-{} (共{}行)", start, end, lines.len()));
    }
    Ok(lines[start.saturating_sub(1)..end].join("\n"))
}

pub fn write_file(path: &str, content: &str) -> Result<String, String> {
    let p = resolve(path);
    fs::write(&p, content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已写入: {} ({} 字节)", p.display(), content.len()))
}

pub fn append_file(path: &str, content: &str) -> Result<String, String> {
    let p = resolve(path);
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&p)
        .map_err(|e| format!("打开失败: {}", e))?;
    file.write_all(content.as_bytes()).map_err(|e| format!("追加失败: {}", e))?;
    Ok(format!("已追加: {} (+{} 字节)", p.display(), content.len()))
}

pub fn create_file(path: &str, content: Option<&str>) -> Result<String, String> {
    let p = resolve(path);
    if p.exists() { return Err(format!("文件已存在: {}", p.display())); }
    if let Some(parent) = p.parent() { fs::create_dir_all(parent).ok(); }
    match content {
        Some(c) => { fs::write(&p, c).map_err(|e| format!("创建失败: {}", e))?; }
        None => { fs::File::create(&p).map_err(|e| format!("创建失败: {}", e))?; }
    }
    Ok(format!("已创建: {}", p.display()))
}

pub fn delete_file(path: &str) -> Result<String, String> {
    let p = resolve(path);
    fs::remove_file(&p).map_err(|e| format!("删除失败: {}", e))?;
    Ok(format!("已删除: {}", p.display()))
}

pub fn copy_file(src: &str, dst: &str) -> Result<String, String> {
    let sp = resolve(src); let dp = resolve(dst);
    let dp = if dp.is_dir() { dp.join(sp.file_name().unwrap_or_default()) } else { dp };
    fs::copy(&sp, &dp).map_err(|e| format!("复制失败: {}", e))?;
    Ok(format!("已复制: {} → {}", sp.display(), dp.display()))
}

pub fn move_file(src: &str, dst: &str) -> Result<String, String> {
    let sp = resolve(src); let dp = resolve(dst);
    fs::rename(&sp, &dp).map_err(|e| format!("移动失败: {}", e))?;
    Ok(format!("已移动: {} → {}", sp.display(), dp.display()))
}

pub fn stat(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let metadata = p.metadata().map_err(|e| format!("stat失败: {}", e))?;
    let mtime = metadata.modified().map(|t| {
        let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        format_ts(secs)
    }).unwrap_or_default();
    Ok(format!("文件: {}\n大小: {} ({})\n修改时间: {}\n只读: {}",
        p.display(), metadata.len(), format_bytes(metadata.len()), mtime, metadata.permissions().readonly()))
}

pub fn hash_file(path: &str, algo: &str) -> Result<String, String> {
    let p = resolve(path);
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    use sha2::Digest;
    let hash = match algo.to_lowercase().as_str() {
        "md5" => format!("{:x}", md5::Md5::digest(&data)),
        "sha-1" | "sha1" => format!("{:x}", sha1::Sha1::digest(&data)),
        "sha-256" | "sha256" => format!("{:x}", sha2::Sha256::digest(&data)),
        "sha-512" | "sha512" => format!("{:x}", sha2::Sha512::digest(&data)),
        _ => return Err(format!("不支持的算法: {}", algo)),
    };
    Ok(format!("{} {}  {}", hash, algo, p.display()))
}

pub fn hex_dump(path: &str, max_bytes: usize, offset: usize) -> Result<String, String> {
    let p = resolve(path);
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let len = max_bytes.min(4096).max(1);
    let off = offset.min(data.len().saturating_sub(1));
    let end = (off + len).min(data.len());
    let slice = &data[off..end];
    let mut out = Vec::new();
    out.push(format!("文件: {}  偏移: 0x{:X} ({})  长度: {}字节", p.display(), off, off, slice.len()));
    out.push(format!("{:>8}  {:48}  {}", "OFFSET", "HEX", "ASCII"));
    out.push("-".repeat(80));
    for (i, chunk) in slice.chunks(16).enumerate() {
        let addr = off + i * 16;
        let hex: String = chunk.iter().map(|b| format!("{:02X} ", b)).collect::<String>();
        let ascii: String = chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
        out.push(format!("{:08X}  {:<48}  {}", addr, hex, ascii));
    }
    Ok(out.join("\n"))
}

pub fn find_files(pattern: &str, path: Option<&str>) -> Result<Vec<String>, String> {
    let dir = match path { Some(p) => resolve(p), None => cwd().read().unwrap().clone() };
    let mut results = Vec::new();
    fn walk(dir: &Path, pattern: &str, results: &mut Vec<String>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, pattern, results); continue; }
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                if simple_match(&name, pattern) { results.push(p.display().to_string()); }
            }
        }
    }
    walk(&dir, pattern, &mut results);
    Ok(results)
}

fn simple_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "*.*" { return true; }
    let re_pat = format!("^{}$", pattern.replace('*', ".*").replace('?', "."));
    regex::Regex::new(&re_pat).map(|r| r.is_match(name)).unwrap_or(false)
}

pub fn grep(pattern: &str, path: &str, case_insensitive: bool, max: usize) -> Result<Vec<String>, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut results = Vec::new();
    let pl = if case_insensitive { pattern.to_lowercase() } else { pattern.to_string() };
    for (i, line) in content.lines().enumerate() {
        let cl = if case_insensitive { line.to_lowercase() } else { line.to_string() };
        if cl.contains(&pl) {
            results.push(format!("{}:{}: {}", i + 1, line, line.trim()));
            if results.len() >= max.max(1) { break; }
        }
    }
    Ok(results)
}

pub fn find_replace(path: &str, find: &str, replace: &str, regex_mode: bool, case_insensitive: bool) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let (new_content, count) = if regex_mode {
        let re = regex::Regex::new(find).map_err(|e| format!("正则错误: {}", e))?;
        let c = re.find_iter(&content).count();
        (re.replace_all(&content, replace).to_string(), c)
    } else if case_insensitive {
        let lower = content.to_lowercase(); let fl = find.to_lowercase();
        let mut result = String::with_capacity(content.len()); let mut cnt = 0usize; let mut pos = 0usize;
        while pos < content.len() {
            if lower[pos..].starts_with(&fl) { result.push_str(replace); pos += find.len(); cnt += 1; }
            else { let c = content[pos..].chars().next().unwrap(); result.push(c); pos += c.len_utf8(); }
        }
        (result, cnt)
    } else {
        (content.replace(find, replace), content.matches(find).count())
    };
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已替换: {} 处修改 → {}", count, p.display()))
}

pub fn dedup(path: &str, ignore_case: bool) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    let mut removed = 0usize;
    for line in &lines {
        let key = if ignore_case { line.to_lowercase() } else { line.to_string() };
        if seen.insert(key) { unique.push(*line); } else { removed += 1; }
    }
    let new_content = unique.join("\n");
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("去重完成: 移除 {} 行 → {}", removed, p.display()))
}

pub fn sort_lines(path: &str, reverse: bool, numeric: bool, unique: bool) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    if numeric {
        lines.sort_by(|a, b| {
            let na: f64 = a.trim().parse().unwrap_or(f64::MAX);
            let nb: f64 = b.trim().parse().unwrap_or(f64::MAX);
            if reverse { nb.partial_cmp(&na).unwrap_or(std::cmp::Ordering::Equal) }
            else { na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal) }
        });
    } else {
        if reverse { lines.sort_by(|a, b| b.cmp(a)); } else { lines.sort(); }
    }
    if unique {
        let mut seen = std::collections::HashSet::new();
        lines.retain(|l| seen.insert(l.clone()));
    }
    let new_content = lines.join("\n");
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已排序: {} — {}行 (逆序:{}, 数值:{}, 去重:{})", p.display(), lines.len(), reverse, numeric, unique))
}

pub fn word_count(path: &str, top_n: usize, ignore_case: bool, min_len: usize) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut freq = std::collections::HashMap::new();
    for word in content.split(|c: char| !c.is_alphanumeric()) {
        let w = if ignore_case { word.to_lowercase() } else { word.to_string() };
        if w.len() >= min_len && !w.is_empty() { *freq.entry(w).or_insert(0usize) += 1; }
    }
    let mut pairs: Vec<(String, usize)> = freq.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = format!("词频统计 {} (top {}):\n", p.display(), top_n);
    for (word, count) in pairs.iter().take(top_n) { out.push_str(&format!("  {:>6}  {}\n", count, word)); }
    Ok(out)
}

pub fn text_stats(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let chars = content.chars().count(); let bytes = content.len(); let lines = content.lines().count();
    let words = content.split_whitespace().count();
    Ok(format!("{} — {}行, {}词, {}字符, {}字节", p.display(), lines, words, chars, bytes))
}

pub fn split_file(path: &str, chunk_size_mb: usize) -> Result<String, String> {
    let p = resolve(path);
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let chunk_size = (chunk_size_mb.max(1) * 1024 * 1024).min(data.len());
    let _total = data.len(); let mut chunks = 0usize;
    for (i, chunk) in data.chunks(chunk_size).enumerate() {
        let cpath = format!("{}.{:04}", p.display(), i);
        fs::write(&cpath, chunk).map_err(|e| format!("写入块{}失败: {}", i, e))?;
        chunks += 1;
    }
    Ok(format!("已分割: {} → {} 块 (每块{}MB)", p.display(), chunks, chunk_size_mb))
}

pub fn zip_files(sources: &[String], output: &str) -> Result<String, String> {
    let op = resolve(output);
    let file = fs::File::create(&op).map_err(|e| format!("创建失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for src in sources {
        let sp = resolve(src);
        let name = sp.file_name().unwrap_or_default().to_string_lossy();
        let data = fs::read(&sp).map_err(|e| format!("读取{}失败: {}", src, e))?;
        zip.start_file(name.as_ref(), options).map_err(|e| format!("zip添加失败: {}", e))?;
        zip.write_all(&data).map_err(|e| format!("zip写入失败: {}", e))?;
    }
    zip.finish().map_err(|e| format!("zip完成失败: {}", e))?;
    Ok(format!("已压缩: {} 个文件 -> {}", sources.len(), op.display()))
}

pub fn unzip(archive: &str, out_dir: &str) -> Result<String, String> {
    let ap = resolve(archive);
    let od = resolve(out_dir);
    fs::create_dir_all(&od).ok();
    let file = fs::File::open(&ap).map_err(|e| format!("打开失败: {}", e))?;
    let mut za = zip::ZipArchive::new(file).map_err(|e| format!("zip解析失败: {}", e))?;
    let count = za.len();
    for i in 0..count {
        let mut entry = za.by_index(i).map_err(|e| format!("读取条目{}失败: {}", i, e))?;
        let outpath = od.join(entry.name());
        if entry.is_dir() { fs::create_dir_all(&outpath).ok(); continue; }
        if let Some(parent) = outpath.parent() { fs::create_dir_all(parent).ok(); }
        let mut outfile = fs::File::create(&outpath).map_err(|e| format!("创建{}失败: {}", outpath.display(), e))?;
        std::io::copy(&mut entry, &mut outfile).map_err(|e| format!("解压失败: {}", e))?;
    }
    Ok(format!("已解压: {} ({} 条目) -> {}", ap.display(), count, od.display()))
}

pub fn compile_rust(source: &str, output: Option<&str>, release: bool, extra_args: &[String]) -> Result<String, String> {
    let sp = resolve(source);
    let is_dir = sp.is_dir();
    let mut cmd = std::process::Command::new("cargo");
    if is_dir { cmd.arg("build").current_dir(&sp); }
    else {
        cmd.arg("rustc").arg(sp.display().to_string());
        if let Some(o) = output { cmd.arg("-o").arg(o); }
    }
    if release { cmd.arg("--release"); }
    for a in extra_args { if !a.is_empty() { cmd.arg(a); } }
    let output = cmd.output().map_err(|e| format!("执行失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() { Ok(format!("编译成功\n{}", stdout)) }
    else { Err(format!("编译失败\n{}", stderr)) }
}

pub fn compile_c(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    let sp = resolve(source);
    let mut cmd = std::process::Command::new("gcc");
    cmd.arg(sp.display().to_string());
    if let Some(o) = output { cmd.arg("-o").arg(o); }
    if optimize { cmd.arg("-O2"); }
    for a in extra_args { if !a.is_empty() { cmd.arg(a); } }
    let out = cmd.output().map_err(|e| format!("执行失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() { Ok(format!("编译成功\n{}", stdout)) }
    else { Err(format!("编译失败\n{}", stderr)) }
}

pub fn edit_file(path: &str) -> Result<String, String> {
    let p = resolve(path);
    #[cfg(windows)] { std::process::Command::new("notepad").arg(p.display().to_string()).spawn().map_err(|e| format!("启动失败: {}", e))?; }
    #[cfg(not(windows))] { std::process::Command::new("vim").arg(p.display().to_string()).spawn().map_err(|e| format!("启动失败: {}", e))?; }
    Ok(format!("编辑器已启动: {}", p.display()))
}

pub fn hex_edit(path: &str, offset: Option<usize>, hex_bytes: Option<&str>) -> Result<String, String> {
    let p = resolve(path);
    if let (Some(off), Some(hb)) = (offset, hex_bytes) {
        let mut data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
        let bytes: Vec<u8> = hb.split_whitespace().filter_map(|s| u8::from_str_radix(s, 16).ok()).collect();
        if bytes.is_empty() { return Err("无效的十六进制字节".into()); }
        if off + bytes.len() > data.len() { data.resize(off + bytes.len(), 0); }
        data[off..off + bytes.len()].copy_from_slice(&bytes);
        fs::write(&p, &data).map_err(|e| format!("写入失败: {}", e))?;
        return Ok(format!("已修改 {} : 偏移0x{:X} 写入 {} 字节", p.display(), off, bytes.len()));
    }
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut out = format!("[*] 文件: {} ({} 字节)\n", p.display(), data.len());
    for (i, row) in data.chunks(16).enumerate() {
        let hex1: String = row[..8.min(row.len())].iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
        let hex2: String = if row.len() > 8 { row[8..].iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ") } else { String::new() };
        let ascii: String = row.iter().map(|&b| if b >= 0x20 && b < 0x7F { b as char } else { '.' }).collect();
        out.push_str(&format!("  0x{:08X}  {:<23}  {:<23}  |{:16}|\n", i * 16, hex1, hex2, ascii));
    }
    out.push_str("\n[*] 修改: hexedit <文件> <偏移> <十六进制字节>\n");
    Ok(out)
}

// ═══════════════════════════════════════
// 高级文件操作 (v5.1 新增)
// ═══════════════════════════════════════

pub fn tail_file(path: &str, lines: usize) -> Result<String, String> {
    let p = resolve(path);
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let n = lines.min(all_lines.len()).max(1);
    Ok(all_lines[all_lines.len() - n..].join("\n"))
}

pub fn head_file(path: &str, lines: usize) -> Result<String, String> {
    let p = resolve(path);
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::new(file);
    let result: Vec<String> = reader.lines().filter_map(|l| l.ok()).take(lines.max(1)).collect();
    Ok(result.join("\n"))
}

pub fn diff_files(file1: &str, file2: &str) -> Result<String, String> {
    let p1 = resolve(file1);
    let p2 = resolve(file2);
    let c1 = fs::read_to_string(&p1).map_err(|e| format!("读取文件1失败: {}", e))?;
    let c2 = fs::read_to_string(&p2).map_err(|e| format!("读取文件2失败: {}", e))?;
    let l1: Vec<&str> = c1.lines().collect();
    let l2: Vec<&str> = c2.lines().collect();
    let mut out = Vec::new();
    let max = l1.len().max(l2.len());
    let mut diff_count = 0usize;
    for i in 0..max {
        let a = l1.get(i).unwrap_or(&"(EOF)");
        let b = l2.get(i).unwrap_or(&"(EOF)");
        if a != b {
            diff_count += 1;
            out.push(format!("行{}: < {}", i + 1, a));
            out.push(format!("行{}: > {}", i + 1, b));
            if diff_count >= 200 { out.push("... (差异超过200行, 截断)".into()); break; }
        }
    }
    Ok(format!("=== diff: {} vs {} ===\n共{}行差异\n{}",
        p1.file_name().unwrap_or_default().to_string_lossy(),
        p2.file_name().unwrap_or_default().to_string_lossy(),
        diff_count, out.join("\n")))
}

pub fn insert_lines(path: &str, line: usize, content: &str) -> Result<String, String> {
    let p = resolve(path);
    let existing = if p.exists() { fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))? } else { String::new() };
    let mut lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();
    let pos = if line == 0 { 0 } else { line.min(lines.len() + 1).saturating_sub(1) };
    for (j, l) in content.lines().enumerate() { lines.insert(pos + j, l.to_string()); }
    let new_content = lines.join("\n");
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已插入 {} 行到 {} 第{}行", content.lines().count(), p.display(), pos + 1))
}

pub fn delete_lines_range(path: &str, start_line: usize, end_line: usize) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    if start_line > lines.len() || start_line > end_line {
        return Err(format!("行号无效: {}-{} (共{}行)", start_line, end_line, lines.len()));
    }
    let end = end_line.min(lines.len());
    let removed: Vec<String> = lines[start_line.saturating_sub(1)..end].to_vec();
    lines.drain(start_line.saturating_sub(1)..end);
    let new_content = lines.join("\n");
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    let preview = if removed.len() > 10 { removed[..10].join("\n") + "\n... (截断)" } else { removed.join("\n") };
    Ok(format!("已删除 {} 第{}-{}行 ({}行)\n删除内容:\n{}", p.display(), start_line, end, removed.len(), preview))
}

pub fn truncate_file_lines(path: &str, keep_lines: usize) -> Result<String, String> {
    let p = resolve(path);
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if keep_lines >= total { return Ok(format!("文件 {} 仅{}行, 无需截断", p.display(), total)); }
    let new_content = lines[..keep_lines].join("\n");
    fs::write(&p, &new_content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已截断 {} : {}行 -> {}行 (删除{}行)", p.display(), total, keep_lines, total - keep_lines))
}

pub fn merge_files(sources: &[String], target: &str) -> Result<String, String> {
    let tp = resolve(target);
    let mut merged = String::new();
    let mut count = 0usize;
    let mut total_bytes = 0usize;
    for src in sources {
        let sp = resolve(src);
        let content = fs::read_to_string(&sp).map_err(|e| format!("读取 {} 失败: {}", src, e))?;
        merged.push_str(&content);
        merged.push('\n');
        count += 1;
        total_bytes += content.len();
    }
    fs::write(&tp, &merged).map_err(|e| format!("写入目标失败: {}", e))?;
    Ok(format!("已合并 {} 个文件 -> {} ({} 字节)", count, tp.display(), total_bytes))
}

pub fn grep_recursive(pattern: &str, dir: &str, case_insensitive: bool) -> Result<Vec<String>, String> {
    let d = resolve(dir);
    if !d.is_dir() { return Err(format!("不是目录: {}", d.display())); }
    let mut results: Vec<String> = Vec::new();
    let mut searched = 0usize;
    let mut matched = 0usize;
    let pl = if case_insensitive { pattern.to_lowercase() } else { pattern.to_string() };
    fn walk(dir: &Path, pl: &str, case_insensitive: bool, results: &mut Vec<String>, searched: &mut usize, matched: &mut usize) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() { walk(&path, pl, case_insensitive, results, searched, matched); continue; }
                if let Ok(m) = path.metadata() { if m.len() > 1024 * 1024 { continue; } }
                *searched += 1;
                if let Ok(content) = fs::read_to_string(&path) {
                    for (i, line) in content.lines().enumerate() {
                        let check = if case_insensitive { line.to_lowercase() } else { line.to_string() };
                        if check.contains(pl) {
                            *matched += 1;
                            results.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                            if results.len() >= 500 { results.push("... (结果超过500条, 截断)".into()); return; }
                        }
                    }
                }
            }
        }
    }
    walk(&d, &pl, case_insensitive, &mut results, &mut searched, &mut matched);
    Ok(vec![format!("=== grep_recursive: \"{}\" in {} === 搜索{}文件, {}匹配", pattern, d.display(), searched, matched), results.join("\n")])
}

pub fn batch_replace(dir: &str, find: &str, replace: &str, file_pattern: &str, case_insensitive: bool) -> Result<String, String> {
    let d = resolve(dir);
    if !d.is_dir() { return Err(format!("不是目录: {}", d.display())); }
    let mut changed_files = 0usize;
    let mut total_replacements = 0usize;
    let mut details: Vec<String> = Vec::new();
    fn walk(dir: &Path, find: &str, replace: &str, file_pattern: &str, case_insensitive: bool,
            changed_files: &mut usize, total_replacements: &mut usize, details: &mut Vec<String>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() { walk(&path, find, replace, file_pattern, case_insensitive, changed_files, total_replacements, details); continue; }
                let fname = path.file_name().unwrap_or_default().to_string_lossy();
                if !simple_match(&fname, file_pattern) { continue; }
                if let Ok(m) = path.metadata() { if m.len() > 1024 * 1024 { continue; } }
                if let Ok(content) = fs::read_to_string(&path) {
                    let (new_content, count) = if case_insensitive {
                        let lower = content.to_lowercase(); let fl = find.to_lowercase();
                        let mut result = String::with_capacity(content.len());
                        let mut cnt = 0usize; let mut pos = 0usize;
                        while pos < content.len() {
                            if lower[pos..].starts_with(&fl) { result.push_str(replace); pos += find.len(); cnt += 1; }
                            else { let c = content[pos..].chars().next().unwrap(); result.push(c); pos += c.len_utf8(); }
                        }
                        (result, cnt)
                    } else { (content.replace(find, replace), content.matches(find).count()) };
                    if count > 0 {
                        if let Err(e) = fs::write(&path, &new_content) { details.push(format!("[!] 写入 {} 失败: {}", path.display(), e)); }
                        else { *changed_files += 1; *total_replacements += count; details.push(format!("{} : {}处替换", path.display(), count)); }
                        if details.len() >= 100 { details.push("... (超过100个文件, 截断)".into()); return; }
                    }
                }
            }
        }
    }
    walk(&d, find, replace, file_pattern, case_insensitive, &mut changed_files, &mut total_replacements, &mut details);
    Ok(format!("=== batch_replace ===\n查找: \"{}\" -> 替换: \"{}\"\n目录: {}\n模式: {}\n结果: {}个文件, {}处替换\n{}",
        find, replace, d.display(), file_pattern, changed_files, total_replacements, details.join("\n")))
}

pub fn file_permissions(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let metadata = p.metadata().map_err(|e| format!("stat失败: {}", e))?;
    let mut attrs = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        let or = mode & 0o400 != 0; let ow = mode & 0o200 != 0; let oe = mode & 0o100 != 0;
        let gr = mode & 0o040 != 0; let gw = mode & 0o020 != 0; let ge = mode & 0o010 != 0;
        let otr = mode & 0o004 != 0; let otw = mode & 0o002 != 0; let ote = mode & 0o001 != 0;
        let su = mode & 0o4000 != 0; let sg = mode & 0o2000 != 0; let st = mode & 0o1000 != 0;
        let ox = if oe { if su { 's' } else { 'x' } } else { if su { 'S' } else { '-' } };
        let gx = if ge { if sg { 's' } else { 'x' } } else { if sg { 'S' } else { '-' } };
        let tx = if ote { if st { 't' } else { 'x' } } else { if st { 'T' } else { '-' } };
        attrs.push(format!("八进制: {:o}", mode));
        attrs.push(format!("权限: {}{}{}{}{}{}{}{}{}",
            if or { 'r' } else { '-' }, if ow { 'w' } else { '-' }, ox,
            if gr { 'r' } else { '-' }, if gw { 'w' } else { '-' }, gx,
            if otr { 'r' } else { '-' }, if otw { 'w' } else { '-' }, tx));
        if su || sg || st {
            let mut flags = String::new();
            if su { flags.push_str("SUID "); } if sg { flags.push_str("SGID "); } if st { flags.push_str("Sticky"); }
            attrs.push(format!("特殊标志: {}", flags.trim()));
        }
    }
    #[cfg(windows)]
    {
        attrs.push(format!("只读: {}", if metadata.permissions().readonly() { "是" } else { "否" }));
        use std::os::windows::fs::MetadataExt;
        let raw = metadata.file_attributes();
        if raw & 0x2 != 0 { attrs.push("隐藏文件".into()); }
        if raw & 0x4 != 0 { attrs.push("系统文件".into()); }
        if raw & 0x20 != 0 { attrs.push("存档".into()); }
    }
    let mtime = metadata.modified().map(|t| {
        let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        format_ts(secs)
    }).unwrap_or_default();
    Ok(format!("文件: {}\n大小: {}\n修改时间: {}\n{}", p.display(), format_bytes(metadata.len()), mtime, attrs.join("\n")))
}

pub fn binary_read(path: &str, offset: usize, length: usize) -> Result<String, String> {
    let p = resolve(path);
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let len = length.min(4096).max(1);
    let off = offset.min(data.len().saturating_sub(1));
    let end = (off + len).min(data.len());
    let slice = &data[off..end];
    let mut out = Vec::new();
    out.push(format!("文件: {}  偏移: 0x{:X} ({})  长度: {}字节", p.display(), off, off, slice.len()));
    out.push(format!("{:>8}  {:48}  {}", "OFFSET", "HEX", "ASCII"));
    out.push("-".repeat(80));
    for (i, chunk) in slice.chunks(16).enumerate() {
        let addr = off + i * 16;
        let hex: String = chunk.iter().map(|b| format!("{:02X} ", b)).collect::<String>();
        let ascii: String = chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
        out.push(format!("{:08X}  {:<48}  {}", addr, hex, ascii));
    }
    Ok(out.join("\n"))
}

pub fn count_lines(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::new(file);
    let count = reader.lines().count();
    let size = p.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(format!("{} — {}行, {}", p.display(), count, format_bytes(size)))
}

// ═══════════════════════════════════════════════════════════════
// v5.9 新增 — 文件操作扩展工具集
// ═══════════════════════════════════════════════════════════════

/// base64_file — 文件Base64编解码
pub fn base64_file(path: &str, operation: &str) -> Result<String, String> {
    let p = Path::new(path);
    match operation {
        "encode" => {
            let data = fs::read(p).map_err(|e| format!("读取失败: {}", e))?;
            Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data))
        }
        "decode" => {
            let text = fs::read_to_string(p).map_err(|e| format!("读取失败: {}", e))?;
            let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.trim())
                .map_err(|e| format!("Base64解码失败: {}", e))?;
            let out_path = format!("{}.decoded", path);
            fs::write(&out_path, &bytes).map_err(|e| format!("写入失败: {}", e))?;
            Ok(format!("已解码 {} bytes -> {}", bytes.len(), out_path))
        }
        _ => Err("operation 必须是 encode 或 decode".into()),
    }
}

/// file_encoding_detect — 检测文本文件编码
pub fn file_encoding_detect(path: &str) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if data.is_empty() { return Ok("空文件".into()); }

    let mut out = Vec::new();

    // BOM detection
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        out.push("UTF-8 (BOM)".to_string());
    } else if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
        out.push("UTF-16 LE (BOM)".to_string());
    } else if data.len() >= 2 && data[0] == 0xFE && data[1] == 0xFF {
        out.push("UTF-16 BE (BOM)".to_string());
    } else if data.len() >= 4 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0xFE && data[3] == 0xFF {
        out.push("UTF-32 BE (BOM)".to_string());
    } else if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xFE && data[2] == 0x00 && data[3] == 0x00 {
        out.push("UTF-32 LE (BOM)".to_string());
    }

    // UTF-8 validation (without BOM)
    if out.is_empty() {
        let is_valid_utf8 = std::str::from_utf8(&data).is_ok();
        if is_valid_utf8 {
            // Check if pure ASCII
            let ascii_count = data.iter().take(4096).filter(|&&b| b < 0x80).count();
            let sample = std::cmp::min(4096, data.len());
            if ascii_count == sample {
                out.push("ASCII / UTF-8 (纯英文)".to_string());
            } else {
                out.push("UTF-8 (无BOM)".to_string());
            }
        } else {
            // Try GBK detection (simplified: check if common CJK range)
            let non_utf8_pos = data.windows(3).position(|w| {
                // Invalid UTF-8 sequences
                w[0] >= 0x80 && (w[1] < 0x80 || w[1] > 0xBF)
            });
            if non_utf8_pos.is_some() {
                // Check for UTF-16LE without BOM patterns
                let even_zeroes = data.chunks(2).take(512).filter(|c| c.len() == 2 && c[1] == 0x00).count();
                if even_zeroes > data.len() / 8 {
                    out.push("UTF-16 LE (无BOM, 推测)".to_string());
                } else {
                    out.push("Latin-1 / GBK / 二进制 (非UTF-8)".to_string());
                }
            } else {
                out.push("未知编码".to_string());
            }
        }
    }

    // Additional stats
    let total = data.len();
    let printable = data.iter().filter(|&&b| b >= 0x20 && b < 0x7F || b == b'\n' || b == b'\r' || b == b'\t').count();
    let binary_ratio = 1.0 - (printable as f64 / total as f64);

    let stat_line = format!("{}B / {} 可打印 ({:.1}% 二进制)", total, printable, binary_ratio * 100.0);
    out.push(stat_line);

    Ok(out.join("\n"))
}

/// json_query — 从JSON中提取值 (简单路径: key.subkey.0)
pub fn json_query(path: &str, query: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    let parts: Vec<&str> = query.split('.').collect();
    let mut current = &value;
    for part in &parts {
        current = if let Ok(idx) = part.parse::<usize>() {
            current.get(idx).ok_or(format!("索引 {} 不存在", idx))?
        } else {
            current.get(*part).ok_or(format!("键 '{}' 不存在", part))?
        };
    }

    Ok(serde_json::to_string_pretty(current).unwrap_or_else(|_| current.to_string()))
}

/// json_keys — 列出JSON对象的所有顶层键
pub fn json_keys(path: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    match &value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().map(|k| {
                let v = &map[k];
                format!("{} ({}): {}", k, json_type_name(v), json_preview(v, 60))
            }).collect();
            keys.sort();
            Ok(format!("{} 个键:\n{}", keys.len(), keys.join("\n")))
        }
        serde_json::Value::Array(arr) => {
            Ok(format!("数组: {} 个元素", arr.len()))
        }
        _ => Ok(format!("标量值: {}", json_preview(&value, 200))),
    }
}

fn json_type_name(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(_) => "bool".to_string(),
        serde_json::Value::Number(_) => "number".to_string(),
        serde_json::Value::String(_) => "string".to_string(),
        serde_json::Value::Array(a) => format!("array[{}]", a.len()),
        serde_json::Value::Object(o) => format!("object[{}]", o.len()),
    }
}

fn json_preview(v: &serde_json::Value, max: usize) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s
    }
}

/// csv_parse — 解析CSV文件，返回表头+前N行
pub fn csv_parse(path: &str, max_rows: usize, delimiter: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    let delim = if delimiter.is_empty() { ',' } else { delimiter.chars().next().unwrap_or(',') };

    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    let header_cols: Vec<&str> = header.split(delim).collect();
    let col_count = header_cols.len();

    let mut out = Vec::new();
    out.push(format!("列数: {} | 表头: {}", col_count, header_cols.join(" | ")));
    out.push(format!("{:-<80}", ""));

    let mut row_count = 0;
    let mut shown = 0;
    for line in lines {
        row_count += 1;
        if shown < max_rows {
            let cols: Vec<&str> = line.split(delim).collect();
            out.push(format!("[{}] {}", row_count, cols.iter().take(8).map(|c| *c).collect::<Vec<_>>().join(" | ")));
            shown += 1;
        }
    }

    out.push(format!("{:-<80}", ""));
    out.push(format!("总行数: {} (显示前{})", row_count, max_rows));
    Ok(out.join("\n"))
}

/// batch_rename — 批量重命名文件 (按正则模式)
pub fn batch_rename(dir: &str, find: &str, replace: &str, dry_run: bool) -> Result<String, String> {
    let re = regex::Regex::new(find).map_err(|e| format!("正则无效: {}", e))?;
    let mut out = Vec::new();
    let mut count = 0u32;

    let entries = fs::read_dir(dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("条目错误: {}", e))?;
        let old_name = entry.file_name().to_string_lossy().to_string();
        let new_name = re.replace(&old_name, replace).to_string();
        if new_name != old_name {
            let old_path = entry.path();
            let new_path = old_path.parent().unwrap_or(Path::new(".")).join(&new_name);
            if !dry_run {
                fs::rename(&old_path, &new_path).map_err(|e| format!("重命名失败: {}", e))?;
            }
            out.push(format!("  {} -> {}", old_name, new_name));
            count += 1;
        }
    }

    if dry_run {
        out.push(format!("\n[DRY RUN] 将重命名 {} 个文件。去掉 dry_run 参数以执行。", count));
    } else {
        out.push(format!("\n已重命名 {} 个文件。", count));
    }
    Ok(out.join("\n"))
}

/// temp_file — 创建临时文件
pub fn temp_file(prefix: &str, suffix: &str, content: Option<&str>) -> Result<String, String> {
    let prefix = if prefix.is_empty() { "ruoo_" } else { prefix };
    let suffix = if suffix.is_empty() { ".tmp" } else { suffix };

    let mut path;
    for _ in 0..100 {
        let rand_suffix: String = (0..8).map(|_| {
            let b = rand::random::<u8>() % 36;
            if b < 10 { (b'0' + b) as char } else { (b'a' + b - 10) as char }
        }).collect();
        path = std::env::temp_dir().join(format!("{}{}{}", prefix, rand_suffix, suffix));
        if !path.exists() {
            if let Some(ct) = content {
                fs::write(&path, ct).map_err(|e| format!("写入失败: {}", e))?;
            } else {
                fs::File::create(&path).map_err(|e| format!("创建失败: {}", e))?;
            }
            return Ok(path.display().to_string());
        }
    }
    Err("无法创建临时文件 (100次尝试均冲突)".into())
}

/// symlink_info — 获取符号链接/ junctions 信息 (Windows)
pub fn symlink_info(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p).map_err(|e| format!("读取元数据失败: {}", e))?;
    let mut out = Vec::new();
    out.push(format!("路径: {}", p.display()));

    let file_type = meta.file_type();
    if file_type.is_symlink() {
        out.push("类型: 符号链接 (symlink)".to_string());
        if let Ok(target) = fs::read_link(p) {
            out.push(format!("目标: {}", target.display()));
        }
    } else if file_type.is_dir() {
        // Check for junction on Windows
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            let attrs = meta.file_attributes();
            if attrs & 0x400 != 0 { // FILE_ATTRIBUTE_REPARSE_POINT
                out.push("类型: 重解析点 (Junction/Mount Point)".to_string());
                if let Ok(target) = fs::read_link(p) {
                    out.push(format!("目标: {}", target.display()));
                }
            } else {
                out.push("类型: 普通目录".to_string());
            }
        }
        #[cfg(not(windows))]
        {
            out.push("类型: 普通目录".to_string());
        }
    } else if file_type.is_file() {
        out.push("类型: 普通文件".to_string());
    }

    out.push(format!("大小: {}", meta.len()));
    Ok(out.join("\n"))
}

/// file_slice — 提取文件的字节范围
pub fn file_slice(path: &str, start: usize, end: usize, output: &str) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("读取失败: {}", e))?;
    if start >= data.len() { return Err(format!("起始 {} 超出文件范围 ({} bytes)", start, data.len())); }
    let actual_end = std::cmp::min(end, data.len());
    let slice = &data[start..actual_end];

    let out_path = if output.is_empty() {
        format!("{}.slice_{}_{}", path, start, actual_end)
    } else {
        output.to_string()
    };
    fs::write(&out_path, slice).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已提取 {}..{} ({} bytes) -> {}", start, actual_end, slice.len(), out_path))
}

/// lines_sample — 从文件随机采样N行
pub fn lines_sample(path: &str, count: usize, seed: u64) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() { return Ok("空文件".into()); }
    if count >= lines.len() { return Ok(format!("文件仅{}行 (请求{})\n{}", lines.len(), count, text)); }

    // Simple deterministic sampling using seed
    let step = lines.len() / count;
    let mut out = Vec::new();
    let mut idx = (seed as usize) % step;
    for _ in 0..count {
        if idx < lines.len() {
            out.push(format!("[{}] {}", idx + 1, lines[idx]));
        }
        idx += step;
    }

    out.push(format!("\n采样 {} / {} 行 (种子={})", count, lines.len(), seed));
    Ok(out.join("\n"))
}

/// empty_files — 查找空文件和空目录
pub fn empty_files(dir: &str, recursive: bool) -> Result<String, String> {
    let mut empty: Vec<String> = Vec::new();
    let mut empty_dirs: Vec<String> = Vec::new();
    self::walk_empty(dir, recursive, &mut empty, &mut empty_dirs);

    let mut out = Vec::new();
    if !empty.is_empty() {
        out.push(format!("空文件 ({} 个):", empty.len()));
        empty.sort();
        for f in &empty { out.push(format!("  {}", f)); }
    }
    if !empty_dirs.is_empty() {
        out.push(format!("\n空目录 ({} 个):", empty_dirs.len()));
        empty_dirs.sort();
        for d in &empty_dirs { out.push(format!("  {}", d)); }
    }
    if empty.is_empty() && empty_dirs.is_empty() {
        out.push("未发现空文件或空目录".to_string());
    }
    Ok(out.join("\n"))
}

fn walk_empty(dir: &str, recursive: bool, empty: &mut Vec<String>, empty_dirs: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut item_count = 0u32;
    for entry in entries.flatten() {
        item_count += 1;
        let path = entry.path();
        if path.is_dir() && recursive {
            walk_empty(&path.display().to_string(), true, empty, empty_dirs);
        }
        if path.is_file() {
            if let Ok(meta) = path.metadata() {
                if meta.len() == 0 {
                    empty.push(path.display().to_string());
                }
            }
        }
    }
    if item_count == 0 && !dir.ends_with('.') {
        empty_dirs.push(dir.to_string());
    }
}

/// file_concat — 拼接多个文件
pub fn file_concat(sources: &str, output: &str, add_separator: bool) -> Result<String, String> {
    let files: Vec<&str> = sources.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if files.is_empty() { return Err("未提供源文件".into()); }

    let mut total = 0u64;
    let sep: &[u8] = if add_separator { b"\n--- FILE SEPARATOR ---\n" } else { b"" };

    let mut out = Vec::new();
    for f in &files {
        let data = fs::read(f).map_err(|e| format!("读取 {}: {}", f, e))?;
        if !out.is_empty() && !sep.is_empty() {
            out.extend_from_slice(sep);
        }
        out.extend_from_slice(&data);
        total += data.len() as u64;
    }

    let out_path = if output.is_empty() { "concat_output.txt" } else { output };
    fs::write(out_path, &out).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已合并 {} 个文件 ({} bytes) -> {}", files.len(), total, out_path))
}

/// du_sorted — 按大小排序的目录使用统计
pub fn du_sorted(dir: &str, depth: usize, top_n: usize) -> Result<String, String> {
    let mut items: Vec<(String, u64, bool)> = Vec::new(); // (path, size, is_dir)
    self::collect_sizes(dir, depth, 0, &mut items);

    items.sort_by(|a, b| b.1.cmp(&a.1));
    items.truncate(top_n);

    let mut out = Vec::new();
    for (path, size, is_dir) in &items {
        let icon = if *is_dir { "[DIR]" } else { "[FILE]" };
        out.push(format!("{} {:>10}  {}", icon, format_bytes(*size as u64), path));
    }

    let total: u64 = items.iter().map(|(_, s, _)| *s).sum();
    out.push(format!("--- 前{}项总计: {} ---", items.len(), format_bytes(total)));
    Ok(out.join("\n"))
}

fn collect_sizes(dir: &str, max_depth: usize, current_depth: usize, items: &mut Vec<(String, u64, bool)>) {
    if current_depth > max_depth { return; }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let display = path.display().to_string();
        if path.is_dir() {
            let size = dir_size_fast(&path);
            items.push((display.clone(), size, true));
            collect_sizes(&display, max_depth, current_depth + 1, items);
        } else if let Ok(meta) = path.metadata() {
            items.push((display, meta.len(), false));
        }
    }
}

fn dir_size_fast(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size_fast(&path);
            } else if let Ok(meta) = path.metadata() {
                total += meta.len();
            }
            if total > 10_000_000_000 { break; } // cap at ~10GB per dir
        }
    }
    total
}

// format_bytes 已在文件顶部定义

// ═══════════════════════════════════════════════════════════════
// v5.9 — 文件剪贴板操作 (copy/paste/cut)
// ═══════════════════════════════════════════════════════════════

/// file_copy_content — 复制文件内容到系统剪贴板
pub fn file_copy_content(path: &str, max_size_kb: usize) -> Result<String, String> {
    let p = resolve(path);
    let meta = p.metadata().map_err(|e| format!("读取元数据: {}", e))?;
    if meta.len() > (max_size_kb * 1024) as u64 {
        return Err(format!("文件 {}KB 超过限制 {}KB", meta.len()/1024, max_size_kb));
    }
    let content = fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
    crate::clipboard::copy_to_clipboard(&content)
        .map(|_| format!("已复制 {} 字符到剪贴板 (来自: {})", content.len(), p.display()))
        .map_err(|e| format!("剪贴板失败: {}", e))
}

/// file_paste_content — 从剪贴板粘贴内容到文件
pub fn file_paste_content(path: &str, append: bool) -> Result<String, String> {
    let text = crate::clipboard::paste_from_clipboard()
        .map_err(|e| format!("读取剪贴板失败: {}", e))?;
    if text.is_empty() { return Err("剪贴板为空".into()); }

    let p = resolve(path);
    if append && p.exists() {
        let mut f = fs::OpenOptions::new().append(true).open(&p)
            .map_err(|e| format!("打开失败: {}", e))?;
        use std::io::Write;
        f.write_all(text.as_bytes()).map_err(|e| format!("写入失败: {}", e))?;
    } else {
        fs::write(&p, &text).map_err(|e| format!("写入失败: {}", e))?;
    }

    Ok(format!("已粘贴 {} 字符 -> {}", text.len(), p.display()))
}

/// file_copy_path — 复制文件路径到剪贴板
pub fn file_copy_path(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let canonical = p.canonicalize().unwrap_or(p.clone());
    crate::clipboard::copy_to_clipboard(&canonical.display().to_string())
        .map(|_| format!("路径已复制: {}", canonical.display()))
        .map_err(|e| format!("剪贴板失败: {}", e))
}

/// file_copy_base64 — 文件Base64编码后复制到剪贴板
pub fn file_copy_base64(path: &str, max_size_mb: usize) -> Result<String, String> {
    let p = resolve(path);
    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let max_bytes = max_size_mb * 1024 * 1024;
    if data.len() > max_bytes {
        return Err(format!("文件 {}MB 超过限制 {}MB", data.len()/1024/1024, max_size_mb));
    }
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
    crate::clipboard::copy_to_clipboard(&b64)
        .map(|_| format!("已复制 Base64 ({} chars, 来自 {} bytes) -> {}", b64.len(), data.len(), p.display()))
        .map_err(|e| format!("剪贴板失败: {}", e))
}

/// file_paste_base64 — 从剪贴板读取Base64并解码到文件
pub fn file_paste_base64(path: &str) -> Result<String, String> {
    let text = crate::clipboard::paste_from_clipboard()
        .map_err(|e| format!("读取剪贴板失败: {}", e))?;
    if text.is_empty() { return Err("剪贴板为空".into()); }

    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.trim())
        .map_err(|e| format!("Base64解码失败: {}", e))?;

    let p = resolve(path);
    fs::write(&p, &bytes).map_err(|e| format!("写入失败: {}", e))?;

    Ok(format!("已解码 {} bytes -> {}", bytes.len(), p.display()))
}

// ── 剪切/粘贴 (cut buffer) ──

/// file_cut — 剪切文件 (记录到剪切缓冲区, 复制路径到剪贴板)
pub fn file_cut(path: &str) -> Result<String, String> {
    let p = resolve(path);
    if !p.exists() { return Err(format!("文件不存在: {}", p.display())); }
    let canonical = p.canonicalize().unwrap_or(p.clone());

    // Store cut buffer
    let cut_buf_path = std::env::temp_dir().join("ruoo_cut_buffer.txt");
    fs::write(&cut_buf_path, canonical.display().to_string())
        .map_err(|e| format!("写入cut buffer失败: {}", e))?;

    crate::clipboard::copy_to_clipboard(&canonical.display().to_string()).ok();

    let meta = p.metadata().map_err(|e| format!("元数据: {}", e))?;
    Ok(format!("已剪切: {} ({}) — 使用 pastecut <目标> 粘贴", canonical.display(), format_bytes(meta.len())))
}

/// file_paste_cut — 粘贴之前剪切的文件 (移动到目标)
pub fn file_paste_cut(dest_dir: &str) -> Result<String, String> {
    let cut_buf_path = std::env::temp_dir().join("ruoo_cut_buffer.txt");
    if !cut_buf_path.exists() { return Err("剪切缓冲区为空 (先使用 cut <文件> 剪切)".into()); }

    let source = fs::read_to_string(&cut_buf_path)
        .map_err(|_| "读取剪切缓冲区失败".to_string())?;
    let source = source.trim();
    if source.is_empty() { return Err("剪切缓冲区为空".into()); }

    let src_path = Path::new(source);
    if !src_path.exists() { return Err(format!("源文件已不存在: {}", source)); }

    let dest = resolve(dest_dir);
    let dest_path = if dest.is_dir() {
        dest.join(src_path.file_name().unwrap_or_default())
    } else {
        dest
    };

    fs::rename(src_path, &dest_path).map_err(|e| format!("移动失败: {}", e))?;
    let _ = fs::remove_file(&cut_buf_path); // clear buffer

    Ok(format!("已移动: {} -> {}", source, dest_path.display()))
}

/// file_cut_status — 查看剪切缓冲区状态
pub fn file_cut_status() -> String {
    let cut_buf_path = std::env::temp_dir().join("ruoo_cut_buffer.txt");
    if !cut_buf_path.exists() {
        return "剪切缓冲区: 空".to_string();
    }
    match fs::read_to_string(&cut_buf_path) {
        Ok(s) if !s.trim().is_empty() => {
            let src = s.trim();
            let exists = Path::new(src).exists();
            format!("剪切缓冲区: {} ({})", src, if exists { "存在" } else { "已删除!" })
        }
        _ => "剪切缓冲区: 空".to_string(),
    }
}

/// file_copy_multi — 批量复制文件路径到剪贴板
pub fn file_copy_multi(paths: &str) -> Result<String, String> {
    let files: Vec<String> = paths.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| {
        let p = resolve(s);
        p.canonicalize().unwrap_or(p).display().to_string()
    }).collect();

    if files.is_empty() { return Err("未提供文件".into()); }

    let text = files.join("\n");
    crate::clipboard::copy_to_clipboard(&text)
        .map(|_| format!("已复制 {} 个文件路径到剪贴板:\n{}", files.len(), text))
        .map_err(|e| format!("剪贴板失败: {}", e))
}

// ═══════════════════════════════════════════
// 扩展文件操作 v9.4 — 安全/取证专用
// ═══════════════════════════════════════════

/// secure_delete — 安全删除文件 (多遍覆写)
/// patterns: "zero" / "random" / "dod" (DoD 5220.22-M: 3遍) / "gutmann" (35遍)
pub fn secure_delete(path: &str, passes: usize, pattern: &str) -> Result<String, String> {
    let p = resolve(path);
    if !p.exists() { return Err("文件不存在".into()); }
    if !p.is_file() { return Err("只能是文件".into()); }

    let size = p.metadata().map(|m| m.len()).unwrap_or(0);
    let mut result = format!("[*] 安全删除: {} ({:.1}KB)\n", p.display(), size as f64 / 1024.0);

    for pass in 0..passes {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .map_err(|e| format!("打开失败 (第{}遍): {}", pass + 1, e))?;

        match pattern {
            "zero" => {
                let zeros = vec![0u8; 4096];
                let full = (size + 4095) / 4096;
                for _ in 0..full {
                    f.write_all(&zeros).ok();
                }
            }
            "random" => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                let mut state = seed.wrapping_add(pass as u64);
                let mut buf = vec![0u8; 4096];
                // 简单 xorshift 随机数生成
                for _ in 0..((size + 4095) / 4096) {
                    for b in buf.iter_mut() {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        *b = (state & 0xFF) as u8;
                    }
                    f.write_all(&buf).ok();
                }
            }
            "dod" | "dod3" => {
                // DoD 5220.22-M: 0xFF → 0x00 → random
                let vals: [u8; 3] = [0xFF, 0x00, 0xAA];
                let v = vals[pass % 3];
                let buf = vec![v; 4096];
                for _ in 0..((size + 4095) / 4096) {
                    f.write_all(&buf).ok();
                }
            }
            _ => {
                // 默认: 0x00
                let buf = vec![0u8; 4096];
                for _ in 0..((size + 4095) / 4096) {
                    f.write_all(&buf).ok();
                }
            }
        }
        f.flush().ok();
        drop(f);
        result.push_str(&format!("  [·] 第{}/{}遍完成\n", pass + 1, passes));
    }

    // 截断到0并删除
    {
        let f = std::fs::OpenOptions::new().write(true).truncate(true).open(&p).ok();
        drop(f);
    }
    fs::remove_file(&p).map_err(|e| format!("删除失败: {}", e))?;

    result.push_str("[+] 文件已安全删除\n");
    Ok(result)
}

/// touch — 创建空文件或更新时间戳
pub fn touch(path: &str) -> Result<String, String> {
    let p = resolve(path);
    if p.exists() {
        // 更新修改时间
        let now = filetime::FileTime::now();
        filetime::set_file_mtime(&p, now)
            .map_err(|e| format!("更新时间失败: {}", e))?;
        Ok(format!("[+] 已更新时间戳: {}", p.display()))
    } else {
        fs::File::create(&p).map_err(|e| format!("创建失败: {}", e))?;
        Ok(format!("[+] 已创建: {}", p.display()))
    }
}

/// set_file_times — 设置文件时间戳
pub fn set_file_times(path: &str, created: Option<&str>, modified: Option<&str>, accessed: Option<&str>) -> Result<String, String> {
    let p = resolve(path);
    if !p.exists() { return Err("文件不存在".into()); }

    let mut result = format!("[*] 修改时间戳: {}\n", p.display());

    // 解析时间字符串 (YYYY-MM-DD HH:MM:SS 或 Unix时间戳)
    let parse_time = |s: &str| -> Option<filetime::FileTime> {
        if let Ok(ts) = s.parse::<i64>() {
            // Unix时间戳 (秒)
            return Some(filetime::FileTime::from_unix_time(ts, 0));
        }
        // 尝试解析日期时间
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|dt| {
                let secs = dt.and_utc().timestamp();
                filetime::FileTime::from_unix_time(secs, 0)
            })
    };

    if let Some(ts_str) = created {
        if let Some(_ft) = parse_time(ts_str) {
            result.push_str(&format!("  [·] 创建时间 (需Windows API): {}\n", ts_str));
            // filetime crate不支持创建时间修改 (仅支持atime/mtime)
            // Windows: 可用 SetFileTime + CREATE_TIME 或 powershell
        }
    }
    if let Some(ts_str) = modified {
        if let Some(ft) = parse_time(ts_str) {
            filetime::set_file_mtime(&p, ft).ok();
            result.push_str(&format!("  [·] 修改时间: {}\n", ts_str));
        }
    }
    if let Some(ts_str) = accessed {
        if let Some(ft) = parse_time(ts_str) {
            filetime::set_file_atime(&p, ft).ok();
            result.push_str(&format!("  [·] 访问时间: {}\n", ts_str));
        }
    }

    result.push_str("[+] 时间戳已更新\n");
    Ok(result)
}

/// ads_read — 读取 Windows NTFS 备用数据流
pub fn ads_read(path: &str, stream_name: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    { return Err("ADS仅在Windows NTFS上可用".into()); }
    #[cfg(windows)]
    {
        let ads_path = format!("{}:{}", path, stream_name);
        let p = std::path::Path::new(&ads_path);
        if !p.exists() { return Err(format!("ADS流不存在: {}", stream_name)); }
        let data = fs::read(&ads_path).map_err(|e| format!("读取ADS失败: {}", e))?;
        let text = String::from_utf8_lossy(&data);
        Ok(format!("═══ ADS: {}:{} ({}) ═══\n{}", path, stream_name, data.len(), text))
    }
}

/// ads_write — 写入 Windows NTFS 备用数据流
pub fn ads_write(path: &str, stream_name: &str, data: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    { return Err("ADS仅在Windows NTFS上可用".into()); }
    #[cfg(windows)]
    {
        let ads_path = format!("{}:{}", path, stream_name);
        fs::write(&ads_path, data).map_err(|e| format!("写入ADS失败: {}", e))?;
        Ok(format!("[+] 已写入 ADS: {}:{} ({}B)", path, stream_name, data.len()))
    }
}

/// ads_list — 列出文件的所有ADS流 (Windows)
pub fn ads_list(path: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    { return Err("ADS仅在Windows NTFS上可用".into()); }
    #[cfg(windows)]
    {
        let p = resolve(path);
        if !p.exists() { return Err("文件不存在".into()); }
        let output = std::process::Command::new("cmd")
            .args(["/C", &format!("dir /r \"{}\"", p.display())])
            .output()
            .map_err(|e| format!("dir /r 失败: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(format!("═══ ADS列表: {} ═══\n{}", p.display(), stdout))
    }
}

/// dir_diff — 比较两个目录的内容差异
pub fn dir_diff(dir1: &str, dir2: &str) -> Result<String, String> {
    let d1 = resolve(dir1);
    let d2 = resolve(dir2);
    if !d1.is_dir() { return Err(format!("不是目录: {}", d1.display())); }
    if !d2.is_dir() { return Err(format!("不是目录: {}", d2.display())); }

    let mut result = format!("═══ 目录差异 ═══\n");
    result.push_str(&format!("  A: {}\n", d1.display()));
    result.push_str(&format!("  B: {}\n\n", d2.display()));

    let mut files1: Vec<String> = Vec::new();
    let mut files2: Vec<String> = Vec::new();

    // 递归列举目录1
    fn collect_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(base).unwrap_or(&path);
                if path.is_dir() {
                    collect_files(&path, base, out);
                } else {
                    out.push(rel.display().to_string());
                }
            }
        }
    }

    collect_files(&d1, &d1, &mut files1);
    collect_files(&d2, &d2, &mut files2);
    files1.sort();
    files2.sort();

    let mut only_in_1 = Vec::new();
    let mut only_in_2 = Vec::new();
    let mut different = Vec::new();
    let mut same = 0usize;

    let mut i = 0;
    let mut j = 0;
    while i < files1.len() && j < files2.len() {
        match files1[i].cmp(&files2[j]) {
            std::cmp::Ordering::Less => {
                only_in_1.push(files1[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                only_in_2.push(files2[j].clone());
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let p1 = d1.join(&files1[i]);
                let p2 = d2.join(&files2[j]);
                if let (Ok(m1), Ok(m2)) = (fs::metadata(&p1), fs::metadata(&p2)) {
                    if m1.len() != m2.len() {
                        different.push(format!("{} ({}B vs {}B)", files1[i], m1.len(), m2.len()));
                    } else {
                        same += 1;
                    }
                }
                i += 1;
                j += 1;
            }
        }
    }
    only_in_1.extend(files1[i..].iter().cloned());
    only_in_2.extend(files2[j..].iter().cloned());

    result.push_str(&format!("  仅A: {} 个\n", only_in_1.len()));
    for f in only_in_1.iter().take(20) { result.push_str(&format!("    - {}\n", f)); }
    if only_in_1.len() > 20 { result.push_str(&format!("    ... (+{} 个)\n", only_in_1.len() - 20)); }

    result.push_str(&format!("\n  仅B: {} 个\n", only_in_2.len()));
    for f in only_in_2.iter().take(20) { result.push_str(&format!("    + {}\n", f)); }
    if only_in_2.len() > 20 { result.push_str(&format!("    ... (+{} 个)\n", only_in_2.len() - 20)); }

    result.push_str(&format!("\n  大小不同: {} 个\n", different.len()));
    for d in different.iter().take(10) { result.push_str(&format!("    ≠ {}\n", d)); }
    if different.len() > 10 { result.push_str(&format!("    ... (+{} 个)\n", different.len() - 10)); }

    result.push_str(&format!("\n  相同: {} 个\n", same));
    result.push_str(&format!("  总计: A={}, B={}\n", files1.len(), files2.len()));

    Ok(result)
}

/// binary_search — 在文件中搜索十六进制字节模式
pub fn binary_search(path: &str, hex_pattern: &str, max_results: usize) -> Result<String, String> {
    let p = resolve(path);
    if !p.is_file() { return Err("不是文件".into()); }

    // 解析HEX模式 "FF AE 01" 或 "FFAE01"
    let clean = hex_pattern.replace(' ', "");
    if clean.len() % 2 != 0 { return Err("HEX模式长度必须是偶数".into()); }
    let pattern: Vec<u8> = (0..clean.len()).step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i+2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("HEX解析失败: {}", e))?;

    let data = fs::read(&p).map_err(|e| format!("读取失败: {}", e))?;
    let mut results = Vec::new();
    let mut offset = 0;

    while offset <= data.len().saturating_sub(pattern.len()) {
        if data[offset..offset + pattern.len()] == pattern[..] {
            // 显示上下文 (前后16字节)
            let ctx_start = offset.saturating_sub(16);
            let ctx_end = (offset + pattern.len() + 16).min(data.len());
            let ctx = &data[ctx_start..ctx_end];
            let hex_ctx: String = ctx.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            let ascii_ctx: String = ctx.iter().map(|&b| if b >= 32 && b < 127 { b as char } else { '.' }).collect();
            results.push(format!("  偏移 0x{:08X}: {} | {}", offset, hex_ctx, ascii_ctx));
            if results.len() >= max_results { break; }
        }
        offset += 1;
    }

    let mut result = format!("═══ 二进制搜索: {} ═══\n", p.display());
    result.push_str(&format!("  模式: {} ({}B)\n", hex_pattern, pattern.len()));
    result.push_str(&format!("  匹配: {} 个\n\n", results.len()));
    for r in &results { result.push_str(r); result.push('\n'); }

    Ok(result)
}

// ════════════════════════════════════════════════════════
// v4.8 大文件操作 — 流式处理/精准定位/增量修改
// ════════════════════════════════════════════════════════

/// stream_grep — 流式搜索大文件，不加载到内存
/// 使用 BufReader 逐行读取，支持上下文行数、最大匹配数、大小写不敏感
pub fn stream_grep(
    path: &str,
    pattern: &str,
    case_insensitive: bool,
    max_matches: usize,
    context_lines: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, file); // 256KB缓冲区

    let pl = if case_insensitive { pattern.to_lowercase() } else { pattern.to_string() };
    let mut results: Vec<String> = Vec::new();
    let mut ring: VecDeque<(usize, String)> = VecDeque::with_capacity(context_lines + 1);
    let mut pending_context = 0usize;
    let mut found = 0usize;

    for (i, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("读取行 {} 失败: {}", i + 1, e))?;
        let cl = if case_insensitive { line.to_lowercase() } else { line.clone() };

        if cl.contains(&pl) {
            found += 1;
            // 输出前置上下文
            for (li, ctx_line) in &ring {
                results.push(format!("{:>8}-: {}", li + 1, ctx_line));
            }
            // 输出匹配行
            results.push(format!("{:>8}>: {}", i + 1, line));
            pending_context = context_lines;

            if found >= max_matches {
                break;
            }
        } else if pending_context > 0 {
            results.push(format!("{:>8} : {}", i + 1, line));
            pending_context -= 1;
        } else {
            // 维护环形缓冲区
            if ring.len() >= context_lines {
                ring.pop_front();
            }
            ring.push_back((i, line));
        }
    }

    if results.is_empty() {
        return Ok(format!("未找到匹配: \"{}\" (搜索范围: {}行)", pattern, found));
    }

    let header = format!(
        "═══ 流式搜索: {} ═══\n模式: \"{}\" | 匹配: {}行 | 上下文: ±{}行\n",
        p.display(), pattern, found, context_lines
    );
    Ok(header + &results.join("\n"))
}

/// stream_binary_scan — 大文件二进制模式搜索 (流式分块)
/// 不加载整个文件，64KB分块扫描，支持海量文件
pub fn stream_binary_scan(
    path: &str,
    hex_pattern: &str,
    max_results: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    let clean = hex_pattern.replace(' ', "");
    if clean.len() % 2 != 0 {
        return Err("HEX模式长度必须是偶数".into());
    }
    let pattern: Vec<u8> = (0..clean.len()).step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("HEX解析失败: {}", e))?;

    if pattern.is_empty() || pattern.len() > 1024 {
        return Err("模式长度需在1-1024字节之间".into());
    }

    let mut file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let chunk_size: usize = 65536; // 64KB块
    let overlap = pattern.len().saturating_sub(1); // 跨块匹配所需的交叠

    let mut results: Vec<String> = Vec::new();
    let mut global_offset: u64 = 0;
    let mut buf = vec![0u8; chunk_size + overlap];
    let mut first_chunk = true;

    loop {
        let read_size = if first_chunk {
            file.read(&mut buf[..chunk_size]).map_err(|e| format!("读取失败: {}", e))?
        } else {
            // 保留交叠部分
            buf.copy_within(chunk_size..chunk_size + overlap, 0);
            file.read(&mut buf[overlap..overlap + chunk_size]).map_err(|e| format!("读取失败: {}", e))?
        };

        let effective_len = if first_chunk {
            read_size
        } else {
            overlap + read_size
        };

        if effective_len < pattern.len() && results.is_empty() {
            break;
        }

        // 在当前块中搜索
        let search_end = effective_len.saturating_sub(pattern.len());
        let mut local_off = 0usize;
        while local_off <= search_end {
            if buf[local_off..local_off + pattern.len()] == pattern[..] {
                let abs_off = global_offset + local_off as u64;
                // 读取上下文 (前后32字节)
                let ctx_start = abs_off.saturating_sub(32);
                let ctx_len = (pattern.len() as u64 + 64).min(file_size.saturating_sub(ctx_start));
                results.push(format!(
                    "  偏移 0x{:08X} ({:>10}) 上下文+{}B",
                    abs_off, abs_off, ctx_len
                ));
                if results.len() >= max_results {
                    break;
                }
                local_off += pattern.len(); // 跳过已匹配部分
            } else {
                local_off += 1;
            }
        }

        if results.len() >= max_results || read_size == 0 {
            break;
        }

        global_offset += chunk_size as u64;
        first_chunk = false;
    }

    let mut result = format!("═══ 流式二进制扫描: {} ═══\n", p.display());
    result.push_str(&format!(
        "  文件: {} | 模式: {} ({}B) | 匹配: {}个\n\n",
        format_bytes(file_size), hex_pattern, pattern.len(), results.len()
    ));
    for r in &results {
        result.push_str(r);
        result.push('\n');
    }

    Ok(result)
}

/// range_read — 按字节范围读取文件片段
/// 支持大文件任意偏移量读取，上限10MB
pub fn range_read(path: &str, offset: u64, length: usize) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    if offset >= file_size {
        return Err(format!("偏移 {} 超出文件大小 {} bytes", offset, file_size));
    }

    let max_len = 10 * 1024 * 1024; // 10MB上限
    let actual_len = length.min(max_len).min((file_size - offset) as usize);

    let mut file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    file.seek(std::io::SeekFrom::Start(offset))
        .map_err(|e| format!("seek失败: {}", e))?;

    let mut buf = vec![0u8; actual_len];
    file.read_exact(&mut buf)
        .map_err(|e| format!("读取失败: {}", e))?;

    // 判断是否为文本内容
    let printable_ratio = buf.iter().filter(|&&b| b >= 32 && b <= 126 || b == b'\n' || b == b'\r' || b == b'\t').count() as f64 / actual_len as f64;

    if printable_ratio > 0.85 && actual_len <= 512 * 1024 {
        // 文本模式
        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<&str> = text.lines().collect();
        let header = format!(
            "═══ 范围读取: {} ═══\n偏移: {} | 长度: {} ({}) | 大小: {}\n行数: {} | 文本(可打印率 {:.0}%)\n",
            p.display(), offset, actual_len, format_bytes(actual_len as u64), format_bytes(file_size),
            lines.len(), printable_ratio * 100.0
        );
        Ok(header + &text)
    } else {
        // 二进制模式 — hexdump
        let mut out = Vec::new();
        out.push(format!(
            "═══ 范围读取(二进制): {} ═══\n偏移: 0x{:X} | 长度: {} | 大小: {}\n",
            p.display(), offset, actual_len, format_bytes(file_size)
        ));
        out.push(format!("{:>8}  {:48}  {}", "OFFSET", "HEX", "ASCII"));
        out.push("-".repeat(80));

        for (i, chunk) in buf.chunks(16).enumerate() {
            let addr = offset as usize + i * 16;
            let hex: String = chunk.iter().map(|b| format!("{:02X} ", b)).collect::<String>();
            let ascii: String = chunk
                .iter()
                .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                .collect();
            out.push(format!("{:08X}  {:<48}  {}", addr, hex, ascii));
        }
        Ok(out.join("\n"))
    }
}

/// stream_find_replace — 流式查找替换大文件
/// 边读边写临时文件，完成后原子替换
pub fn stream_find_replace(
    path: &str,
    find: &str,
    replace: &str,
    case_insensitive: bool,
    max_replacements: usize,
) -> Result<String, String> {
    let p = resolve(path);
    if find.is_empty() {
        return Err("查找字符串不能为空".into());
    }

    let tmp_path = p.with_extension("tmp.stream_replace");
    let bak_path = p.with_extension("bak.stream_replace");

    let in_file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, in_file);
    let mut out_file = fs::File::create(&tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;

    let find_lower = if case_insensitive { find.to_lowercase() } else { find.to_string() };
    let mut total_replacements = 0usize;
    let mut total_lines = 0usize;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行失败: {}", e))?;
        total_lines += 1;

        if total_replacements < max_replacements {
            let check = if case_insensitive { line.to_lowercase() } else { line.clone() };
            if check.contains(&find_lower) {
                let new_line = if case_insensitive {
                    // 大小写不敏感的替换
                    let mut result = String::with_capacity(line.len());
                    let mut i = 0;
                    while i < line.len() {
                        if i + find.len() <= line.len() {
                            let slice = &line[i..i + find.len()];
                            if slice.to_lowercase() == find_lower {
                                result.push_str(replace);
                                i += find.len();
                                total_replacements += 1;
                                continue;
                            }
                        }
                        result.push(line[i..].chars().next().unwrap());
                        i += line[i..].chars().next().unwrap().len_utf8();
                    }
                    result
                } else {
                    // 精确替换
                    line.replace(find, replace)
                };
                let actual_count = if case_insensitive {
                    total_replacements // 上方已累计
                } else {
                    let cnt = line.matches(find).count();
                    total_replacements += cnt;
                    cnt
                };
                writeln!(out_file, "{}", new_line).map_err(|e| format!("写入失败: {}", e))?;

                if total_replacements >= max_replacements && actual_count == 0 {
                    // 已达到上限，剩余行原样写入
                }
            } else {
                writeln!(out_file, "{}", line).map_err(|e| format!("写入失败: {}", e))?;
            }
        } else {
            writeln!(out_file, "{}", line).map_err(|e| format!("写入失败: {}", e))?;
        }
    }
    out_file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    drop(out_file);

    // 原子替换: 备份原文件 → 移动临时文件
    fs::rename(&p, &bak_path).map_err(|e| format!("备份失败: {}", e))?;
    fs::rename(&tmp_path, &p).map_err(|e| {
        // 回滚: 恢复备份
        let _ = fs::rename(&bak_path, &p);
        format!("替换失败(已回滚): {}", e)
    })?;
    // 清理备份
    let _ = fs::remove_file(&bak_path);

    Ok(format!(
        "═══ 流式替换完成 ═══\n文件: {}\n查找: \"{}\" → \"{}\"\n替换: {}处 | 扫描: {}行",
        p.display(), find, replace, total_replacements, total_lines
    ))
}

/// build_line_index — 为大文本文件构建行偏移索引
/// 生成 .idx 二进制索引文件，每行偏移量u64编码
pub fn build_line_index(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    let idx_path = p.with_extension("idx");
    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(512 * 1024, file);
    let mut idx_file = fs::File::create(&idx_path).map_err(|e| format!("创建索引失败: {}", e))?;

    let mut offset: u64 = 0;
    let mut line_count: u64 = 0;
    let mut buf = [0u8; 8];

    // 写入文件头: 魔数 + 版本 + 行数占位
    idx_file.write_all(b"RUOO_IDX\x01").map_err(|e| format!("写索引头失败: {}", e))?;
    idx_file.write_all(&0u64.to_le_bytes()).map_err(|e| format!("写索引头失败: {}", e))?; // 行数占位，最后回填

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行失败: {}", e))?;
        // 写入当前行偏移 (u64 LE)
        buf.copy_from_slice(&offset.to_le_bytes());
        idx_file.write_all(&buf).map_err(|e| format!("写索引失败: {}", e))?;
        line_count += 1;
        offset += line.len() as u64 + 1; // +1 for newline

        if line_count % 1_000_000 == 0 {
            // 每百万行刷新一次，避免内存爆炸
            idx_file.flush().ok();
        }
    }

    // 回填行数
    idx_file.seek(std::io::SeekFrom::Start(9)).map_err(|e| format!("seek失败: {}", e))?;
    idx_file.write_all(&line_count.to_le_bytes()).map_err(|e| format!("写索引失败: {}", e))?;
    idx_file.flush().map_err(|e| format!("刷新失败: {}", e))?;

    let idx_size = idx_file.metadata().map(|m| m.len()).unwrap_or(0);

    Ok(format!(
        "═══ 行索引构建完成 ═══\n文件: {} ({})\n索引: {} ({})\n行数: {} | 每行偏移: 8B\n索引文件: {}",
        p.display(), format_bytes(file_size),
        idx_path.display(), format_bytes(idx_size),
        line_count, idx_path.display()
    ))
}

/// seek_line — 使用索引文件快速定位读取指定行
/// 需要先 build_line_index 生成 .idx 文件
pub fn seek_line(path: &str, line_start: u64, line_end: Option<u64>) -> Result<String, String> {
    let p = resolve(path);
    let idx_path = p.with_extension("idx");

    if !idx_path.exists() {
        return Err(format!(
            "索引文件不存在: {}\n请先运行 build_line_index 构建索引",
            idx_path.display()
        ));
    }

    let mut idx_file = fs::File::open(&idx_path).map_err(|e| format!("打开索引失败: {}", e))?;

    // 读取文件头
    let mut header = [0u8; 17];
    idx_file.read_exact(&mut header).map_err(|e| format!("读索引头失败: {}", e))?;

    if &header[..8] != b"RUOO_IDX" {
        return Err("无效的索引文件魔数".into());
    }

    let total_lines = u64::from_le_bytes(header[9..17].try_into().unwrap());

    if line_start < 1 || line_start > total_lines {
        return Err(format!("行号 {} 超出范围 (1-{})", line_start, total_lines));
    }

    let end = line_end.unwrap_or(line_start).min(total_lines);
    if end < line_start {
        return Err("结束行号不能小于起始行号".into());
    }

    // 定位到起始行偏移位置
    let offset_pos = 17 + (line_start - 1) as u64 * 8;
    idx_file.seek(std::io::SeekFrom::Start(offset_pos)).map_err(|e| format!("seek索引失败: {}", e))?;

    let mut offset_buf = [0u8; 8];
    idx_file.read_exact(&mut offset_buf).map_err(|e| format!("读偏移失败: {}", e))?;
    let start_offset = u64::from_le_bytes(offset_buf);

    // 如果有多行请求，读取结束行的下一个偏移来确定长度
    let read_len = if end > line_start {
        let end_pos = 17 + end as u64 * 8;
        if end_pos < 17 + total_lines * 8 {
            idx_file.seek(std::io::SeekFrom::Start(end_pos)).ok();
            let mut end_buf = [0u8; 8];
            if idx_file.read_exact(&mut end_buf).is_ok() {
                let end_offset = u64::from_le_bytes(end_buf);
                (end_offset - start_offset) as usize
            } else {
                1024 * 1024 // 默认1MB
            }
        } else {
            // 到文件末尾
            10 * 1024 * 1024 // 最多10MB
        }
    } else {
        1024 * 1024 // 单行默认最多1MB
    };

    // 读取文件内容
    let mut file = fs::File::open(&p).map_err(|e| format!("打开文件失败: {}", e))?;
    file.seek(std::io::SeekFrom::Start(start_offset)).map_err(|e| format!("seek文件失败: {}", e))?;

    let actual_len = read_len.min(10 * 1024 * 1024); // 10MB上限
    let mut buf = vec![0u8; actual_len];
    let n = file.read(&mut buf).map_err(|e| format!("读取失败: {}", e))?;
    buf.truncate(n);

    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();

    Ok(format!(
        "═══ 索引定位: {} ═══\n行 {}-{} / {} | 偏移: {} | 读取: {}\n\n{}",
        p.display(), line_start, end, total_lines, start_offset, format_bytes(n as u64),
        lines.join("\n")
    ))
}

/// query_line_index — 查询索引信息 (不读取文件内容)
pub fn query_line_index(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let idx_path = p.with_extension("idx");

    if !idx_path.exists() {
        return Ok(format!("索引文件不存在: {}\n(使用 build_line_index 创建)", idx_path.display()));
    }

    let mut idx_file = fs::File::open(&idx_path).map_err(|e| format!("打开索引失败: {}", e))?;
    let mut header = [0u8; 17];
    idx_file.read_exact(&mut header).map_err(|e| format!("读索引头失败: {}", e))?;

    if &header[..8] != b"RUOO_IDX" {
        return Err("无效的索引文件魔数".into());
    }

    let total_lines = u64::from_le_bytes(header[9..17].try_into().unwrap());
    let idx_meta = idx_file.metadata().map(|m| m.len()).unwrap_or(0);
    let file_meta = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);

    Ok(format!(
        "═══ 行索引信息 ═══\n文件: {} ({})\n索引: {} ({})\n总行数: {}\n每行偏移: 8B\n",
        p.display(), format_bytes(file_meta),
        idx_path.display(), format_bytes(idx_meta),
        total_lines
    ))
}

/// stream_file_info — 大文件流式统计 (不加载到内存)
/// 统计行数、大小、空行数、最大行长等
pub fn stream_file_info(path: &str) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, file);

    let mut total_lines: u64 = 0;
    let mut empty_lines: u64 = 0;
    let mut max_line_len: usize = 0;
    let mut min_line_len: usize = usize::MAX;
    let mut total_chars: u64 = 0;
    let mut non_ascii_lines: u64 = 0;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行失败: {}", e))?;
        total_lines += 1;
        let len = line.len();

        if len == 0 {
            empty_lines += 1;
        }
        if len > max_line_len {
            max_line_len = len;
        }
        if len < min_line_len {
            min_line_len = len;
        }
        total_chars += len as u64;

        if line.as_bytes().iter().any(|&b| b > 127) {
            non_ascii_lines += 1;
        }
    }

    if min_line_len == usize::MAX {
        min_line_len = 0;
    }

    let avg_line_len = if total_lines > 0 {
        total_chars / total_lines
    } else {
        0
    };

    Ok(format!(
        "═══ 流式文件统计: {} ═══\n\
         大小: {} | 行数: {}\n\
         空行: {} ({:.1}%) | 非ASCII行: {} ({:.1}%)\n\
         行长: 最小{} / 最大{} / 平均{}\n\
         总字符: {}",
        p.display(), format_bytes(file_size), total_lines,
        empty_lines, if total_lines > 0 { empty_lines as f64 / total_lines as f64 * 100.0 } else { 0.0 },
        non_ascii_lines, if total_lines > 0 { non_ascii_lines as f64 / total_lines as f64 * 100.0 } else { 0.0 },
        min_line_len, max_line_len, avg_line_len,
        total_chars
    ))
}



// ════════════════════════════════════════════════════════
// v4.9 大文件精准操作 — 正则搜索/字节修改/多模式替换/列提取/差异对比
// ════════════════════════════════════════════════════════

/// regex_search — 流式正则搜索大文件
/// 使用 regex crate，逐行流式匹配，不加载整个文件到内存
/// 支持捕获组显示、上下文行、匹配数限制
pub fn regex_search(
    path: &str,
    regex_pattern: &str,
    case_insensitive: bool,
    max_matches: usize,
    context_lines: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    // 构建正则
    let mut builder = regex::RegexBuilder::new(regex_pattern);
    builder.case_insensitive(case_insensitive);
    builder.multi_line(false); // 逐行匹配
    builder.dot_matches_new_line(false);
    let re = builder.build().map_err(|e| format!("正则编译失败: {}", e))?;

    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, file);

    let mut results: Vec<String> = Vec::new();
    let mut ring: VecDeque<(usize, String)> = VecDeque::with_capacity(context_lines + 1);
    let mut pending_context = 0usize;
    let mut found = 0usize;
    let mut total_lines: u64 = 0;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行 {} 失败: {}", total_lines + 1, e))?;
        total_lines += 1;

        if re.is_match(&line) {
            found += 1;
            // 输出前置上下文
            for (li, ctx_line) in &ring {
                results.push(format!("{:>8}-: {}", li + 1, ctx_line));
            }
            ring.clear();
            // 尝试捕获组
            if let Some(caps) = re.captures(&line) {
                let mut cap_info = String::new();
                for (gi, cap) in caps.iter().enumerate() {
                    if gi == 0 { continue; } // 跳过完整匹配
                    if let Some(m) = cap {
                        cap_info.push_str(&format!("  ${}={} ", gi, m.as_str()));
                    }
                }
                if cap_info.is_empty() {
                    results.push(format!("{:>8}>: {}", total_lines, line));
                } else {
                    results.push(format!("{:>8}>: {}  [捕获:{}]", total_lines, line, cap_info));
                }
            } else {
                results.push(format!("{:>8}>: {}", total_lines, line));
            }
            pending_context = context_lines;

            if found >= max_matches {
                break;
            }
        } else if pending_context > 0 {
            results.push(format!("{:>8} : {}", total_lines, line));
            pending_context -= 1;
        } else {
            if ring.len() >= context_lines {
                ring.pop_front();
            }
            ring.push_back((total_lines as usize - 1, line));
        }
    }

    if results.is_empty() {
        return Ok(format!(
            "═══ 正则搜索: {} ═══\n文件: {} | 模式: \"{}\"\n结果: 未找到匹配 (扫描{}行)",
            p.display(), format_bytes(file_size), regex_pattern, total_lines
        ));
    }

    let header = format!(
        "═══ 正则搜索: {} ═══\n文件: {} | 模式: \"{}\"\n匹配: {}行 | 扫描: {}行 | 上下文: ±{}行\n",
        p.display(), format_bytes(file_size), regex_pattern, found, total_lines, context_lines
    );
    Ok(header + &results.join("\n"))
}

/// byte_range_replace — 按字节偏移精确替换
/// 支持超大文件的部分字节级修改，使用hex字符串指定新数据
/// 原子操作: 写临时文件 → 备份原文件 → 原子替换
pub fn byte_range_replace(
    path: &str,
    offset: u64,
    hex_bytes: &str,
) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    if offset >= file_size {
        return Err(format!("偏移 {} 超出文件大小 {} bytes", offset, file_size));
    }

    // 解析hex字符串
    let clean = hex_bytes.replace(' ', "");
    if clean.len() % 2 != 0 {
        return Err("HEX字符串长度必须是偶数".into());
    }
    let new_data: Vec<u8> = (0..clean.len()).step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("HEX解析失败: {}", e))?;

    if new_data.is_empty() {
        return Err("替换数据不能为空".into());
    }

    let replace_len = new_data.len() as u64;
    let tmp_path = p.with_extension("tmp.byterep");
    let bak_path = p.with_extension("bak.byterep");

    // 复制原文件到临时文件，然后修改指定区域
    fs::copy(&p, &tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;

    let mut tmp_file = fs::OpenOptions::new()
        .write(true)
        .open(&tmp_path)
        .map_err(|e| format!("打开临时文件失败: {}", e))?;

    tmp_file.seek(std::io::SeekFrom::Start(offset))
        .map_err(|e| format!("seek失败: {}", e))?;
    tmp_file.write_all(&new_data)
        .map_err(|e| format!("写入失败: {}", e))?;
    tmp_file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    drop(tmp_file);

    // 原子替换
    fs::rename(&p, &bak_path).map_err(|e| format!("备份失败: {}", e))?;
    fs::rename(&tmp_path, &p).map_err(|e| {
        let _ = fs::rename(&bak_path, &p);
        format!("替换失败(已回滚): {}", e)
    })?;
    let _ = fs::remove_file(&bak_path);

    // 读取修改后的上下文
    let ctx_start = offset.saturating_sub(32);
    let ctx_len = (replace_len + 64).min(file_size.saturating_sub(ctx_start));
    let mut ctx_buf = vec![0u8; ctx_len as usize];
    if let Ok(mut f) = fs::File::open(&p) {
        f.seek(std::io::SeekFrom::Start(ctx_start)).ok();
        let _ = f.read(&mut ctx_buf);
    }

    let hex_ctx: String = ctx_buf.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .chunks(48)
        .map(|c| c.join(" "))
        .collect::<Vec<_>>()
        .join("\n  ");

    Ok(format!(
        "═══ 字节替换完成 ═══\n文件: {} ({})\n偏移: 0x{:X} ({}) | 替换: {}B → {}B\n修改后上下文(偏移0x{:X}):\n  {}",
        p.display(), format_bytes(file_size),
        offset, offset,
        replace_len, new_data.len(),
        ctx_start, hex_ctx
    ))
}

/// stream_multi_replace — 流式多模式批量替换
/// 在一次遍历中执行多个find→replace，支持大小写不敏感
/// pairs格式: JSON数组 [["find1","replace1"],["find2","replace2"],...]
pub fn stream_multi_replace(
    path: &str,
    pairs_json: &str,
    case_insensitive: bool,
    max_replacements: usize,
) -> Result<String, String> {
    let p = resolve(path);

    // 解析替换对
    let pairs: Vec<(String, String)> = serde_json::from_str(pairs_json)
        .map_err(|e| format!("JSON解析失败: {} — 期望格式 [[\"find\",\"replace\"],...]", e))?;

    if pairs.is_empty() {
        return Err("替换对列表为空".into());
    }

    for (i, (find, _)) in pairs.iter().enumerate() {
        if find.is_empty() {
            return Err(format!("pair[{}]: 查找字符串不能为空", i));
        }
    }

    let tmp_path = p.with_extension("tmp.multirep");
    let bak_path = p.with_extension("bak.multirep");

    let in_file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, in_file);
    let mut out_file = fs::File::create(&tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;

    // 预处理: 构建小写版本的查找字符串
    let pairs_lower: Vec<(String, &str)> = if case_insensitive {
        pairs.iter().map(|(f, r)| (f.to_lowercase(), r.as_str())).collect()
    } else {
        pairs.iter().map(|(f, r)| (f.clone(), r.as_str())).collect()
    };

    let mut total_replacements = 0usize;
    let mut total_lines = 0usize;
    let mut per_pair_counts = vec![0usize; pairs.len()];

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行失败: {}", e))?;
        total_lines += 1;

        if total_replacements >= max_replacements {
            writeln!(out_file, "{}", line).map_err(|e| format!("写入失败: {}", e))?;
            continue;
        }

        let check = if case_insensitive { line.to_lowercase() } else { line.clone() };
        let mut modified = line.clone();
        let mut line_replacements = 0usize;

        // 对每个替换对执行 (按长度降序避免短串覆盖长串)
        let mut sorted_indices: Vec<usize> = (0..pairs.len()).collect();
        sorted_indices.sort_by_key(|&i| -(pairs[i].0.len() as i64));

        for &pi in &sorted_indices {
            if total_replacements + line_replacements >= max_replacements {
                break;
            }

            let (find_lower, replace) = &pairs_lower[pi];
            let find_orig = &pairs[pi].0;

            if check.contains(find_lower) {
                // 统计替换次数
                let cnt = if case_insensitive {
                    check.matches(find_lower.as_str()).count()
                } else {
                    modified.matches(find_orig.as_str()).count()
                };
                let before = modified.clone();
                if case_insensitive {
                    // 大小写不敏感替换
                    let mut result = String::with_capacity(modified.len());
                    let mut i = 0;
                    while i < modified.len() {
                        if i + find_orig.len() <= modified.len() {
                            let slice = &modified[i..i + find_orig.len()];
                            if slice.to_lowercase() == *find_lower {
                                result.push_str(replace);
                                i += find_orig.len();
                                line_replacements += 1;
                                continue;
                            }
                        }
                        if let Some(c) = modified[i..].chars().next() {
                            result.push(c);
                            i += c.len_utf8();
                        } else {
                            break;
                        }
                    }
                    modified = result;
                } else {
                    modified = modified.replace(find_orig, replace);
                };
                // 更新check用于后续匹配
                if before != modified && case_insensitive {
                    // 更新小写版本
                }
                per_pair_counts[pi] += cnt;
            }
        }

        total_replacements += line_replacements;
        writeln!(out_file, "{}", modified).map_err(|e| format!("写入失败: {}", e))?;
    }
    out_file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    drop(out_file);

    // 原子替换
    fs::rename(&p, &bak_path).map_err(|e| format!("备份失败: {}", e))?;
    fs::rename(&tmp_path, &p).map_err(|e| {
        let _ = fs::rename(&bak_path, &p);
        format!("替换失败(已回滚): {}", e)
    })?;
    let _ = fs::remove_file(&bak_path);

    let mut summary = format!(
        "═══ 多模式流式替换完成 ═══\n文件: {}\n模式数: {} | 总替换: {}处 | 扫描: {}行\n",
        p.display(), pairs.len(), total_replacements, total_lines
    );
    for (i, (find, replace)) in pairs.iter().enumerate() {
        summary.push_str(&format!("  [{}] \"{}\" → \"{}\": {}处\n", i + 1, find, replace, per_pair_counts[i]));
    }
    Ok(summary)
}

/// search_export — 搜索结果导出到文件
/// 流式搜索大文件，将匹配行(含上下文)写入输出文件
pub fn search_export(
    path: &str,
    pattern: &str,
    output_path: &str,
    case_insensitive: bool,
    context_lines: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let out_p = resolve(output_path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, file);
    let mut out_file = fs::File::create(&out_p).map_err(|e| format!("创建输出文件失败: {}", e))?;

    let pl = if case_insensitive { pattern.to_lowercase() } else { pattern.to_string() };
    let mut ring: VecDeque<(usize, String)> = VecDeque::with_capacity(context_lines + 1);
    let mut pending_context = 0usize;
    let mut found = 0usize;
    let mut total_lines: u64 = 0;

    // 写入文件头
    writeln!(out_file, "# RUOO-ARSENAL 搜索结果导出")
        .map_err(|e| format!("写入失败: {}", e))?;
    writeln!(out_file, "# 源文件: {}", p.display())
        .map_err(|e| format!("写入失败: {}", e))?;
    writeln!(out_file, "# 模式: \"{}\" | 大小写敏感: {} | 上下文: ±{}行",
        pattern, !case_insensitive, context_lines)
        .map_err(|e| format!("写入失败: {}", e))?;
    writeln!(out_file, "# {}", "-".repeat(60))
        .map_err(|e| format!("写入失败: {}", e))?;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("读取行 {} 失败: {}", total_lines + 1, e))?;
        total_lines += 1;
        let cl = if case_insensitive { line.to_lowercase() } else { line.clone() };

        if cl.contains(&pl) {
            found += 1;
            // 写入前置上下文
            for (li, ctx_line) in &ring {
                writeln!(out_file, "  {:>8}-: {}", li + 1, ctx_line)
                    .map_err(|e| format!("写入失败: {}", e))?;
            }
            ring.clear();
            // 写入匹配行
            writeln!(out_file, ">>{:>8}>: {}", total_lines, line)
                .map_err(|e| format!("写入失败: {}", e))?;
            pending_context = context_lines;
        } else if pending_context > 0 {
            writeln!(out_file, "  {:>8} : {}", total_lines, line)
                .map_err(|e| format!("写入失败: {}", e))?;
            pending_context -= 1;
        } else {
            if ring.len() >= context_lines {
                ring.pop_front();
            }
            ring.push_back((total_lines as usize - 1, line));
        }
    }

    out_file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    let out_meta = fs::metadata(&out_p).map(|m| m.len()).unwrap_or(0);

    Ok(format!(
        "═══ 搜索导出完成 ═══\n源文件: {} ({})\n模式: \"{}\" | 匹配: {}行\n导出到: {} ({})\n扫描: {}行",
        p.display(), format_bytes(file_size),
        pattern, found,
        out_p.display(), format_bytes(out_meta),
        total_lines
    ))
}

/// column_extract — 流式CSV/TSV列提取
/// 从超大分隔符文件中提取指定列，流式处理不加载整个文件
/// 列索引从0开始，逗号分隔。delimiter: "comma"/"tab"/"pipe"/"semicolon"/自定义字符
pub fn column_extract(
    path: &str,
    columns: &str,
    delimiter: &str,
    max_rows: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    // 解析列索引
    let col_indices: Vec<usize> = columns.split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("列索引解析失败: {} (期望如 \"0,2,5\")", e))?;

    if col_indices.is_empty() {
        return Err("至少需要一个列索引".into());
    }

    let delim = match delimiter.to_lowercase().as_str() {
        "comma" | "," => b',',
        "tab" | "\\t" => b'\t',
        "pipe" | "|" => b'|',
        "semicolon" | ";" => b';',
        "space" | " " => b' ',
        s if s.len() == 1 => s.as_bytes()[0],
        _ => return Err(format!("不支持的分隔符: {} (使用 comma/tab/pipe/semicolon/space 或单字符)", delimiter)),
    };

    let file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let reader = BufReader::with_capacity(256 * 1024, file);

    let mut results: Vec<Vec<String>> = Vec::new();
    let mut total_lines = 0usize;
    let mut max_col = *col_indices.iter().max().unwrap_or(&0);

    for line_res in reader.lines() {
        if total_lines >= max_rows && max_rows > 0 {
            break;
        }
        let line = line_res.map_err(|e| format!("读取行 {} 失败: {}", total_lines + 1, e))?;
        total_lines += 1;

        let fields: Vec<&str> = line.split(delim as char).collect();
        if fields.len() > max_col {
            max_col = max_col.max(fields.len() - 1);
        }

        let row: Vec<String> = col_indices.iter()
            .map(|&ci| {
                if ci < fields.len() {
                    fields[ci].to_string()
                } else {
                    String::new()
                }
            })
            .collect();
        results.push(row);
    }

    let max_display = 500;
    let _display_rows = results.len().min(max_display);

    let mut out = format!(
        "═══ 列提取: {} ═══\n文件: {} | 列: {} | 分隔符: \"{}\"\n行数: {} | 最大列索引: {}\n\n",
        p.display(), format_bytes(file_size), columns, delim as char, total_lines, max_col
    );

    // 列头
    out.push_str(&format!("ROW  | {}\n", col_indices.iter()
        .map(|ci| format!("COL{:<8}", ci))
        .collect::<Vec<_>>()
        .join(" | ")));
    out.push_str(&format!("{:-<4}-+-{}\n", "", "-".repeat(col_indices.len() * 13)));

    for (i, row) in results.iter().take(max_display).enumerate() {
        out.push_str(&format!("{:>4} | {}\n",
            i + 1,
            row.iter().map(|v| format!("{:<12}", if v.len() > 12 { format!("{}...", &v[..9]) } else { v.clone() }))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }

    if results.len() > max_display {
        out.push_str(&format!("\n... 省略 {} 行 (总数 {} 行, 显示上限 {})",
            results.len() - max_display, results.len(), max_display));
    }

    Ok(out)
}

/// binary_diff — 大文件二进制差异比较
/// 流式分块比较两个文件，逐块输出差异位置和内容
/// 返回前N个差异的偏移量和hex内容
pub fn binary_diff(
    file1: &str,
    file2: &str,
    max_diffs: usize,
) -> Result<String, String> {
    let p1 = resolve(file1);
    let p2 = resolve(file2);

    let m1 = fs::metadata(&p1).map_err(|e| format!("文件1 stat失败: {}", e))?;
    let m2 = fs::metadata(&p2).map_err(|e| format!("文件2 stat失败: {}", e))?;

    let size1 = m1.len();
    let size2 = m2.len();

    let mut f1 = fs::File::open(&p1).map_err(|e| format!("打开文件1失败: {}", e))?;
    let mut f2 = fs::File::open(&p2).map_err(|e| format!("打开文件2失败: {}", e))?;

    let chunk_size: usize = 65536; // 64KB
    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];
    let mut offset: u64 = 0;
    let mut diffs: Vec<String> = Vec::new();

    loop {
        let n1 = f1.read(&mut buf1).map_err(|e| format!("读文件1失败: {}", e))?;
        let n2 = f2.read(&mut buf2).map_err(|e| format!("读文件2失败: {}", e))?;

        if n1 == 0 && n2 == 0 {
            break;
        }

        let min_n = n1.min(n2);
        if n1 != n2 {
            diffs.push(format!(
                "  偏移 0x{:08X}: 大小差异 (文件1={}B vs 文件2={}B 剩余)",
                offset, n1, n2
            ));
        }

        // 逐字节比较
        let mut diff_start: Option<u64> = None;
        let mut diff_bytes1: Vec<u8> = Vec::new();
        let mut diff_bytes2: Vec<u8> = Vec::new();

        for i in 0..min_n {
            if buf1[i] != buf2[i] {
                if diff_start.is_none() {
                    diff_start = Some(offset + i as u64);
                }
                diff_bytes1.push(buf1[i]);
                diff_bytes2.push(buf2[i]);
            } else if diff_start.is_some() {
                // 差异块结束
                let ds = diff_start.unwrap();
                let hex1: String = diff_bytes1.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
                let hex2: String = diff_bytes2.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
                diffs.push(format!(
                    "  偏移 0x{:08X} ({:>8}B):\n    F1: {}\n    F2: {}",
                    ds, diff_bytes1.len(), hex1, hex2
                ));
                diff_start = None;
                diff_bytes1.clear();
                diff_bytes2.clear();

                if diffs.len() >= max_diffs {
                    break;
                }
            }
        }

        // 处理块末尾的未闭合差异
        if let Some(ds) = diff_start {
            let hex1: String = diff_bytes1.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            let hex2: String = diff_bytes2.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            diffs.push(format!(
                "  偏移 0x{:08X} ({:>8}B) [块边界截断]:\n    F1: {}\n    F2: {}",
                ds, diff_bytes1.len(), hex1, hex2
            ));
        }

        if diffs.len() >= max_diffs || (n1 == 0 && n2 == 0) {
            break;
        }

        offset += min_n as u64;
    }

    let size_info = if size1 == size2 {
        format!("大小相同: {}", format_bytes(size1))
    } else {
        format!("大小不同: {} vs {}", format_bytes(size1), format_bytes(size2))
    };

    if diffs.is_empty() {
        Ok(format!(
            "═══ 二进制差异: 完全一致 ═══\nF1: {} ({})\nF2: {} ({})\n{}",
            p1.display(), format_bytes(size1),
            p2.display(), format_bytes(size2),
            size_info
        ))
    } else {
        Ok(format!(
            "═══ 二进制差异 ═══\nF1: {} ({})\nF2: {} ({})\n{} | 差异块: {}个\n\n{}",
            p1.display(), format_bytes(size1),
            p2.display(), format_bytes(size2),
            size_info, diffs.len(),
            diffs.join("\n")
        ))
    }
}

/// file_chunk_scan — 分块扫描大文件
/// 将文件分成固定大小的块，每块显示偏移范围、可打印字符比例、
/// 文本/二进制判断、首尾行预览。用于快速了解大文件结构
pub fn file_chunk_scan(
    path: &str,
    chunk_size_mb: usize,
    max_chunks: usize,
) -> Result<String, String> {
    let p = resolve(path);
    let meta = fs::metadata(&p).map_err(|e| format!("stat失败: {}", e))?;
    let file_size = meta.len();

    let chunk_size = (chunk_size_mb.max(1).min(1024)) * 1024 * 1024; // 1MB-1GB
    let total_chunks = ((file_size as f64) / (chunk_size as f64)).ceil() as usize;
    let chunks_to_scan = total_chunks.min(max_chunks.max(1));

    let mut file = fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
    let mut buf = vec![0u8; chunk_size.min(10 * 1024 * 1024)]; // 最多10MB缓冲区
    let sample_size = 64 * 1024; // 每块采样64KB

    let mut results: Vec<String> = Vec::new();

    for ci in 0..chunks_to_scan {
        let chunk_start = (ci as u64) * (chunk_size as u64);
        let effective_size = if ci == total_chunks - 1 {
            file_size - chunk_start
        } else {
            chunk_size as u64
        };

        let read_size = effective_size.min(sample_size as u64) as usize;
        file.seek(std::io::SeekFrom::Start(chunk_start))
            .map_err(|e| format!("seek失败: {}", e))?;

        buf.resize(read_size, 0);
        let n = file.read(&mut buf).map_err(|e| format!("读取失败: {}", e))?;
        buf.truncate(n);

        if n == 0 { break; }

        let printable = buf.iter().filter(|&&b| b >= 0x20 && b <= 0x7E || b == b'\n' || b == b'\r' || b == b'\t').count();
        let ratio = printable as f64 / n as f64 * 100.0;
        let content_type = if ratio > 80.0 { "TEXT" } else if ratio > 30.0 { "MIXED" } else { "BINARY" };

        // 提取首尾64字节作为预览
        let head_preview: String = buf.iter().take(64)
            .map(|&b| if b >= 0x20 && b <= 0x7E { b as char } else { '.' })
            .collect();
        let tail_start = if n > 64 { n - 64 } else { 0 };
        let tail_preview: String = buf[tail_start..].iter()
            .map(|&b| if b >= 0x20 && b <= 0x7E { b as char } else { '.' })
            .collect();

        results.push(format!(
            "块{:>4}  偏移 0x{:08X}-0x{:08X}  {:>8}  {:>6}  {:5.1}%可打印  首:{}  尾:{}",
            ci + 1, chunk_start, chunk_start + effective_size - 1,
            format_bytes(effective_size), content_type, ratio,
            head_preview, tail_preview
        ));
    }

    Ok(format!(
        "═══ 分块扫描: {} ═══\n文件大小: {} | 块大小: {}MB | 总块数: {} | 扫描: {}/{}块\n采样: 每块{}KB\n\n{}",
        p.display(), format_bytes(file_size), chunk_size_mb,
        total_chunks, chunks_to_scan, total_chunks,
        sample_size / 1024,
        results.join("\n")
    ))
}
