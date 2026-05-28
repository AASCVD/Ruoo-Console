// ============================================================
// RUOO-CONSOLE — 多语言编译器 v1.0
//
// 支持: Rust / C / C++ / Go / C# / Java / Python / ASM / Zig
// 自动检测已安装的编译器, 优雅降级
// ============================================================

use std::process::Command;
use std::path::Path;

// ════════════════════════════════════════════════════════
// 编译器探测
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CompilerInfo {
    pub name: String,
    pub language: String,
    pub exe: String,
    pub version: String,
    pub available: bool,
}

/// 检测所有可用编译器
pub fn detect_compilers() -> Vec<CompilerInfo> {
    let checks = vec![
        ("rustc", "Rust", "rustc", &["--version"] as &[&str]),
        ("gcc", "C", "gcc", &["--version"]),
        ("g++", "C++", "g++", &["--version"]),
        ("clang", "C/C++", "clang", &["--version"]),
        ("go", "Go", "go", &["version"]),
        ("javac", "Java", "javac", &["-version"]),
        ("csc", "C#", "csc", &["-version"]),
        ("dotnet", "C# (.NET)", "dotnet", &["--version"]),
        ("python", "Python", "python", &["--version"]),
        ("nasm", "ASM(x86)", "nasm", &["-version"]),
        ("zig", "Zig", "zig", &["version"]),
        ("nim", "Nim", "nim", &["--version"]),
    ];

    checks.into_iter().map(|(exe, lang, display_exe, args)| {
        let version = Command::new(exe)
            .args(args)
            .output()
            .map(|o| {
                let out = String::from_utf8_lossy(if o.stdout.is_empty() { &o.stderr } else { &o.stdout });
                out.lines().next().unwrap_or("未知版本").to_string()
            })
            .unwrap_or_else(|_| "未安装".to_string());
        let available = version != "未安装";
        CompilerInfo {
            name: display_exe.to_string(),
            language: lang.to_string(),
            exe: exe.to_string(),
            version,
            available,
        }
    }).collect()
}

/// 列出所有编译器
pub fn list_compilers() -> String {
    let compilers = detect_compilers();
    let mut out = format!("═══ 编译器检测 ({}个) ═══\n\n", compilers.len());
    for c in &compilers {
        let icon = if c.available { "[+]" } else { "[-]" };
        out.push_str(&format!("{} {:12} {:6} — {}\n", icon, c.name, c.language, c.version));
    }
    out.push_str("\n[*] 使用: compile <语言> <源文件> [选项]\n");
    out
}

// ════════════════════════════════════════════════════════
// 统一编译入口
// ════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub language: String,
    pub source: String,
    pub output: Option<String>,
    pub optimize: bool,
    pub extra_args: Vec<String>,
    pub cross_compile_target: Option<String>,
}

pub fn compile(opts: &CompileOptions) -> Result<String, String> {
    match opts.language.to_lowercase().as_str() {
        "rust" | "rs" => compile_rust_impl(opts),
        "c" => compile_c_impl(opts),
        "cpp" | "c++" | "cxx" => compile_cpp_impl(opts),
        "go" | "golang" => compile_go_impl(opts),
        "java" => compile_java_impl(opts),
        "cs" | "c#" | "csharp" | "dotnet" => compile_csharp_impl(opts),
        "py" | "python" => compile_python_impl(opts),
        "asm" | "nasm" | "assembly" => compile_asm_impl(opts),
        "zig" => compile_zig_impl(opts),
        "auto" => compile_auto(opts),
        _ => Err(format!("不支持的语言: {}。支持: rust/c/cpp/go/java/cs/python/asm/zig/auto", opts.language)),
    }
}

// ════════════════════════════════════════════════════════
// 自动检测语言 (根据文件扩展名)
// ════════════════════════════════════════════════════════

fn compile_auto(opts: &CompileOptions) -> Result<String, String> {
    let ext = Path::new(&opts.source)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let lang = match ext.as_str() {
        "rs" => "rust",
        "c" => "c",
        "cpp" | "cc" | "cxx" | "c++" => "cpp",
        "go" => "go",
        "java" => "java",
        "cs" => "cs",
        "py" => "python",
        "asm" | "s" => "asm",
        "zig" => "zig",
        _ => return Err(format!("无法从扩展名 .{} 推断语言。请显式指定语言", ext)),
    };

    let mut auto_opts = opts.clone();
    auto_opts.language = lang.to_string();
    compile(&auto_opts)
}

// ════════════════════════════════════════════════════════
// Rust — rustc / cargo
// ════════════════════════════════════════════════════════

fn compile_rust_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = Path::new(&opts.source);
    if !sp.exists() { return Err(format!("文件不存在: {}", opts.source)); }

    let is_dir = sp.is_dir();
    let mut cmd = Command::new("cargo");
    if is_dir {
        cmd.arg("build").current_dir(sp);
    } else {
        cmd.arg("rustc").arg(sp.display().to_string());
        if let Some(ref o) = opts.output { cmd.arg("-o").arg(o); }
    }
    if opts.optimize { cmd.arg("--release"); }
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// C — gcc / clang
// ════════════════════════════════════════════════════════

fn compile_c_impl(opts: &CompileOptions) -> Result<String, String> {
    let compiler = pick_compiler(&["gcc", "clang"])?;
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new(&compiler);
    cmd.arg(sp.display().to_string());
    if let Some(ref o) = opts.output { cmd.arg("-o").arg(o); }
    if opts.optimize { cmd.arg("-O2"); }
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// C++ — g++ / clang++
// ════════════════════════════════════════════════════════

fn compile_cpp_impl(opts: &CompileOptions) -> Result<String, String> {
    let compiler = pick_compiler(&["g++", "clang++"])?;
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new(&compiler);
    cmd.arg(sp.display().to_string()).arg("-std=c++17");
    if let Some(ref o) = opts.output { cmd.arg("-o").arg(o); }
    if opts.optimize { cmd.arg("-O2"); }
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// Go — go build
// ════════════════════════════════════════════════════════

fn compile_go_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new("go");
    cmd.arg("build");
    if let Some(ref o) = opts.output { cmd.arg("-o").arg(o); }
    if opts.optimize { cmd.arg("-ldflags").arg("-s -w"); }
    cmd.arg(sp.display().to_string());
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// Java — javac
// ════════════════════════════════════════════════════════

fn compile_java_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new("javac");
    if let Some(ref o) = opts.output { cmd.arg("-d").arg(o); }
    if opts.optimize { cmd.arg("-g:none"); }
    cmd.arg(sp.display().to_string());
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    let result = run_cmd(&mut cmd)?;
    Ok(format!("编译成功 (Java .class)\n{}", result))
}

// ════════════════════════════════════════════════════════
// C# — csc / dotnet build
// ════════════════════════════════════════════════════════

fn compile_csharp_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;

    // 优先使用 dotnet build (项目)
    if sp.is_dir() || sp.extension().map_or(false, |e| e == "csproj" || e == "sln") {
        let mut cmd = Command::new("dotnet");
        cmd.arg("build");
        if opts.optimize { cmd.arg("-c").arg("Release"); }
        if sp.is_dir() { cmd.current_dir(&sp); }
        else { cmd.current_dir(sp.parent().unwrap()); }
        for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
        return run_cmd(&mut cmd);
    }

    // csc (单文件)
    let compiler = pick_compiler(&["csc", "mcs"])?;
    let mut cmd = Command::new(&compiler);
    if let Some(ref o) = opts.output { cmd.arg(format!("/out:{}", o)); }
    else { cmd.arg(format!("/out:{}.exe", sp.file_stem().unwrap().to_string_lossy())); }
    if opts.optimize { cmd.arg("/optimize+"); }
    cmd.arg(sp.display().to_string());
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// Python — py_compile → .pyc
// ════════════════════════════════════════════════════════

fn compile_python_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new("python");
    cmd.arg("-m").arg("py_compile").arg(sp.display().to_string());
    let result = run_cmd(&mut cmd)?;
    Ok(format!("Python 编译成功 (.pyc)\n{}", result))
}

// ════════════════════════════════════════════════════════
// Assembly — NASM
// ════════════════════════════════════════════════════════

fn compile_asm_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;
    let fmt = if cfg!(windows) { "win64" } else { "elf64" };
    let mut cmd = Command::new("nasm");
    cmd.arg("-f").arg(fmt);
    if let Some(ref o) = opts.output {
        cmd.arg("-o").arg(o);
    } else {
        let out_name = sp.with_extension("o");
        cmd.arg("-o").arg(out_name.display().to_string());
    }
    cmd.arg(sp.display().to_string());
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }

    let result = run_cmd(&mut cmd)?;
    Ok(format!("NASM 汇编成功 (.o)\n提示: 需要链接器 (ld/gcc) 生成可执行文件\n{}", result))
}

// ════════════════════════════════════════════════════════
// Zig — zig build-exe
// ════════════════════════════════════════════════════════

fn compile_zig_impl(opts: &CompileOptions) -> Result<String, String> {
    let sp = resolve_path(&opts.source)?;
    let mut cmd = Command::new("zig");
    cmd.arg("build-exe");
    if let Some(ref o) = opts.output { cmd.arg("-femit-bin").arg(o); }
    if opts.optimize { cmd.arg("-O").arg("ReleaseFast"); }
    if let Some(ref target) = opts.cross_compile_target { cmd.arg("-target").arg(target); }
    cmd.arg(sp.display().to_string());
    for a in &opts.extra_args { if !a.is_empty() { cmd.arg(a); } }
    run_cmd(&mut cmd)
}

// ════════════════════════════════════════════════════════
// 辅助函数
// ════════════════════════════════════════════════════════

fn resolve_path(source: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(source);
    if p.exists() { return Ok(p.to_path_buf()); }
    // 尝试在工作目录查找
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前目录: {}", e))?;
    let full = cwd.join(p);
    if full.exists() { return Ok(full); }
    Err(format!("文件不存在: {}", source))
}

fn pick_compiler(candidates: &[&str]) -> Result<String, String> {
    for &c in candidates {
        if Command::new(c).arg("--version").output().is_ok() {
            return Ok(c.to_string());
        }
    }
    Err(format!("未找到编译器。已尝试: {}\n请安装: gcc/clang 等", candidates.join(", ")))
}

fn run_cmd(cmd: &mut Command) -> Result<String, String> {
    let output = cmd.output().map_err(|e| format!("执行失败: {} — 编译器是否已安装?", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        let mut result = String::new();
        if !stdout.trim().is_empty() { result.push_str(&stdout); }
        if !stderr.trim().is_empty() { result.push_str(&format!("\n[stderr]\n{}", stderr)); }
        if result.trim().is_empty() { result = "编译成功 (无输出)".to_string(); }
        Ok(format!("编译成功\n{}", result))
    } else {
        let mut err = String::new();
        if !stderr.trim().is_empty() { err.push_str(&stderr); }
        if !stdout.trim().is_empty() { err.push_str(&format!("\n[stdout]\n{}", stdout)); }
        Err(format!("编译失败\n{}", err))
    }
}

// ════════════════════════════════════════════════════════
// 便捷API
// ════════════════════════════════════════════════════════

pub fn compile_language(
    language: &str,
    source: &str,
    output: Option<&str>,
    optimize: bool,
    extra_args: &[String],
) -> Result<String, String> {
    let opts = CompileOptions {
        language: language.to_string(),
        source: source.to_string(),
        output: output.map(|s| s.to_string()),
        optimize,
        extra_args: extra_args.to_vec(),
        cross_compile_target: None,
    };
    compile(&opts)
}

pub fn compile_rust(source: &str, output: Option<&str>, release: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("rust", source, output, release, extra_args)
}

pub fn compile_c(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("c", source, output, optimize, extra_args)
}

pub fn compile_cpp(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("cpp", source, output, optimize, extra_args)
}

pub fn compile_go(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("go", source, output, optimize, extra_args)
}

pub fn compile_java(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("java", source, output, optimize, extra_args)
}

pub fn compile_csharp(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("cs", source, output, optimize, extra_args)
}

pub fn compile_python(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("python", source, output, optimize, extra_args)
}

pub fn compile_asm(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("asm", source, output, optimize, extra_args)
}

pub fn compile_zig(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("zig", source, output, optimize, extra_args)
}

pub fn compile_auto_detect(source: &str, output: Option<&str>, optimize: bool, extra_args: &[String]) -> Result<String, String> {
    compile_language("auto", source, output, optimize, extra_args)
}

// ════════════════════════════════════════════════════════
// 测试
// ════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_compilers() {
        let compilers = detect_compilers();
        assert!(compilers.len() >= 10);
        // 至少 rustc 应该存在 (ruoo-arsenal 使用 Rust 构建)
        let rustc = compilers.iter().find(|c| c.exe == "rustc");
        assert!(rustc.is_some());
        assert!(rustc.unwrap().available);
    }

    #[test]
    fn test_list_compilers() {
        let output = list_compilers();
        assert!(output.contains("rustc"));
        assert!(output.contains("[+]") || output.contains("[-]"));
    }

    #[test]
    fn test_compile_auto_detect() {
        // 使用不存在的文件测试错误处理
        let result = compile_language("auto", "nonexistent.rs", None, false, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_language() {
        let result = compile_language("brainfuck", "test.bf", None, false, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不支持的语言"));
    }

    #[test]
    fn test_rust_compile_nonexistent() {
        let result = compile_rust("nonexistent.rs", None, false, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_c_compile_nonexistent() {
        let result = compile_c("nonexistent.c", None, false, &[]);
        assert!(result.is_err());
    }
}
