// build.rs — RUOO Arsenal 编译构建脚本
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    // 随机种子: 每次构建不同的二进制hash
    println!("cargo:rustc-env=RUOO_BUILD_SEED={}", ts);
    println!("cargo:rustc-env=RUOO_BUILD_TIME={}", ts / 1_000_000_000);
    println!("cargo:rerun-if-changed=build.rs");

    // Windows 版本资源 — 使用真实项目信息，不伪装
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set("ProductName", "RUOO Console");
        res.set("FileDescription", "RUOO Console TUI Terminal Panel");
        res.set("CompanyName", "RUOO");
        res.set("LegalCopyright", "RUOO. All rights reserved.");
        res.set("OriginalFilename", "ruoo-console.exe");
        res.set("InternalName", "ruoo-console");
        res.set("FileVersion", "4.1.0.0");
        res.set("ProductVersion", "4.1.0");
        res.set_language(0x0804); // zh-CN
        // Manifest: asInvoker 无需管理员权限
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
</assembly>
"#);
        if let Err(e) = res.compile() {
            eprintln!("[!] winres资源编译失败: {} (非致命)", e);
        }
    }
}
