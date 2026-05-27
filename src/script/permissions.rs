
// ─── 权限标志 ─────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionSet {
    pub net: bool,     // 网络访问 (scan/dns/whois/http)
    pub fs: bool,      // 文件系统 (read/write/delete)
    pub exec: bool,    // 命令执行 (exec/cmd/powershell)
    pub plugin: bool,  // 加载外部插件 (.dll/.so)
    pub sys: bool,     // 内核驱动 (.sys/.ko) — 最高危险级
}

impl PermissionSet {
    pub fn none() -> Self {
        Self { net: false, fs: false, exec: false, plugin: false, sys: false }
    }

    pub fn all() -> Self {
        Self { net: true, fs: true, exec: true, plugin: true, sys: true }
    }

    pub fn apply_directive(&mut self, directive: &str) -> bool {
        match directive {
            "allow-net"    => { self.net = true; true }
            "allow-fs"     => { self.fs = true; true }
            "allow-exec"   => { self.exec = true; true }
            "allow-plugin" => { self.plugin = true; true }
            "allow-sys"    => { self.sys = true; true }
            "allow-all"    => { *self = Self::all(); true }
            "deny-all"     => { *self = Self::none(); true }
            "watch-plugins" => true,
            "watch-sys"    => true,
            _ => false,
        }
    }

    pub fn check_net(&self) -> Result<(), String> {
        if self.net { Ok(()) } else { Err("权限不足: 需要 #![allow-net]".into()) }
    }

    pub fn check_fs(&self) -> Result<(), String> {
        if self.fs { Ok(()) } else { Err("权限不足: 需要 #![allow-fs]".into()) }
    }

    pub fn check_exec(&self) -> Result<(), String> {
        if self.exec { Ok(()) } else { Err("权限不足: 需要 #![allow-exec]".into()) }
    }

    pub fn check_plugin(&self) -> Result<(), String> {
        if self.plugin { Ok(()) } else { Err("权限不足: 需要 #![allow-plugin]".into()) }
    }

    pub fn check_sys(&self) -> Result<(), String> {
        if self.sys { Ok(()) } else { Err("权限不足: 需要 #![allow-sys] (内核驱动操作)".into()) }
    }

    /// 检查脚本权限是否覆盖插件要求的权限
    pub fn covers(&self, required: &PluginPermissions) -> Result<(), String> {
        let mut missing = Vec::new();
        if required.net && !self.net { missing.push("net"); }
        if required.fs && !self.fs { missing.push("fs"); }
        if required.exec && !self.exec { missing.push("exec"); }
        if !missing.is_empty() {
            return Err(format!(
                "插件要求权限但脚本未授予: {} — 添加 #![allow-{}]",
                missing.join(", "),
                missing.join("] #![allow-")
            ));
        }
        Ok(())
    }
}

// ─── 插件权限声明 ─────────────────────────────────────
/// 插件通过 ruoo_plugin_permissions() 导出此位掩码
/// 0x01=NET, 0x02=FS, 0x04=EXEC, 0x08=PLUGIN
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPermissions {
    pub net: bool,
    pub fs: bool,
    pub exec: bool,
    pub plugin: bool,
}

impl PluginPermissions {
    pub fn from_bits(bits: u32) -> Self {
        Self {
            net:    (bits & 0x01) != 0,
            fs:     (bits & 0x02) != 0,
            exec:   (bits & 0x04) != 0,
            plugin: (bits & 0x08) != 0,
        }
    }

    #[allow(dead_code)]
    pub fn to_bits(&self) -> u32 {
        let mut b = 0u32;
        if self.net { b |= 0x01; }
        if self.fs  { b |= 0x02; }
        if self.exec { b |= 0x04; }
        if self.plugin { b |= 0x08; }
        b
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.net { parts.push("net"); }
        if self.fs { parts.push("fs"); }
        if self.exec { parts.push("exec"); }
        if self.plugin { parts.push("plugin"); }
        if parts.is_empty() { "none".into() } else { parts.join("+") }    }
}