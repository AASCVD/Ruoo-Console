// ============================================================
// RUOO-ARSENAL v4.1 — 内存安全字符串 (SecStr)
//
// 特性:
//   - XOR 加密存储（每实例随机 32 字节密钥）
//   - Drop 时零化密钥 + 密文（防止内存取证）
//   - ★ v4.1: mlock/VirtualLock 防止内存被换出到磁盘
//   - ★ v4.1: ptr::write_volatile 防止编译器优化掉零化操作
//   - with() 安全访问模式 — 明文仅在闭包内短暂可见
//   - 用于保护 API Key、密码、令牌等敏感数据
// ============================================================

use rand::RngCore;
use std::fmt;
use std::ptr;

/// 安全字符串 — 内存中 XOR 加密存储
///
/// 敏感字符串应使用此类型而非 String。
/// 明文仅在 `with()` 闭包内短暂可见，返回后立即零化。
#[allow(dead_code)]
pub struct SecStr {
    ciphertext: Vec<u8>,
    key: [u8; 32],
    len: usize,
    /// ★ v4.1: 内存是否已锁定 (mlock/VirtualLock)
    locked: bool,
}

#[allow(dead_code)]
impl SecStr {
    /// 从明文字符串构造加密实例
    pub fn new(plaintext: &str) -> Self {
        let plain_bytes = plaintext.as_bytes();
        let len = plain_bytes.len();
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);

        let mut ciphertext: Vec<u8> = plain_bytes
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % 32])
            .collect();

        // ★ v4.1: 尝试锁定包含密文的内存页，防止交换到磁盘
        let locked = lock_memory(&mut ciphertext);

        Self { ciphertext, key, len, locked }
    }

    /// 构造空安全字符串
    pub fn empty() -> Self {
        Self { ciphertext: Vec::new(), key: [0u8; 32], len: 0, locked: false }
    }

    /// 临时解密访问 — 闭包内可见明文，返回后零化
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&str) -> R,
    {
        let mut plain: Vec<u8> = self
            .ciphertext
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ self.key[i % 32])
            .collect();

        // ★ v4.1: 锁定明文暂存区
        let _locked = lock_memory(&mut plain);

        let result = match std::str::from_utf8(&plain) {
            Ok(s) => f(s),
            Err(_) => {
                let lossy = String::from_utf8_lossy(&plain);
                f(&lossy)
            }
        };

        // ★ 零化 — 使用 write_volatile 防止编译器优化掉
        zeroize_slice_volatile(&mut plain);
        result
    }

    /// 直接解密克隆（不推荐，仅必要场景）
    pub fn decrypt_clone(&self) -> String {
        self.with(|s| s.to_string())
    }

    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn len(&self) -> usize { self.len }
}

impl Drop for SecStr {
    fn drop(&mut self) {
        // ★ v4.1: 使用 write_volatile 零化，防止编译器优化消除
        zeroize_slice_volatile(&mut self.key);
        zeroize_slice_volatile(&mut self.ciphertext);
        self.len = 0;
        // 注意: 解锁内存由操作系统在进程退出或 munmap 时自动处理
        // 主动 unlock 可能因其他引用而危险，此处故意不执行
    }
}

impl Clone for SecStr {
    fn clone(&self) -> Self {
        let mut ct = self.ciphertext.clone();
        let locked = lock_memory(&mut ct);
        Self { ciphertext: ct, key: self.key, len: self.len, locked }
    }
}

impl fmt::Debug for SecStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecStr")
            .field("len", &self.len)
            .field("encrypted", &true)
            .field("locked", &self.locked)
            .finish_non_exhaustive()
    }
}

// ── 内存锁定 (防止交换到磁盘) ──
// ★ BUG-6 修复: 防止敏感数据被写入 pagefile/swap

/// 尝试锁定内存区域，防止操作系统将其换出到磁盘
/// Windows: VirtualLock | Linux: mlock
fn lock_memory(slice: &mut [u8]) -> bool {
    if slice.is_empty() { return false; }
    let ptr = slice.as_mut_ptr() as *mut std::ffi::c_void;
    let len = slice.len();

    #[cfg(windows)]
    {
        // Windows: VirtualLock 锁定页面 (kernel32.dll)
        extern "system" {
            fn VirtualLock(lpAddress: *mut std::ffi::c_void, dwSize: usize) -> i32;
        }
        unsafe { VirtualLock(ptr, len) != 0 }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: mlock(2) 锁定物理内存页
        extern "C" {
            fn mlock(addr: *const std::ffi::c_void, len: usize) -> i32;
        }
        unsafe { mlock(ptr as *const _, len) == 0 }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        // macOS/BSD: mlock 同样可用
        extern "C" {
            fn mlock(addr: *const std::ffi::c_void, len: usize) -> i32;
        }
        unsafe { mlock(ptr as *const _, len) == 0 }
    }
}

// ── 零化辅助 (防编译器优化) ──

/// ★ v4.1: 使用 ptr::write_volatile 零化切片
/// 编译器无法优化掉 volatile 写入，确保零化一定执行
fn zeroize_slice_volatile(slice: &mut [u8]) {
    for b in slice.iter_mut() {
        unsafe { ptr::write_volatile(b, 0); }
    }
}

pub fn zeroize_string(s: &mut String) {
    unsafe {
        for b in s.as_bytes_mut() { ptr::write_volatile(b, 0); }
    }
    s.clear();
}

#[allow(dead_code)]
pub fn zeroize_vec(v: &mut Vec<u8>) {
    unsafe {
        for b in v.iter_mut() { ptr::write_volatile(b, 0); }
    }
    v.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let s = SecStr::new("P@ssw0rd!秘密");
        assert_eq!(s.len(), 15); // "P@ssw0rd!"=9 + 秘(3字节) + 密(3字节) = 15
        let r = s.with(|p| p.to_string());
        assert_eq!(r, "P@ssw0rd!秘密");
    }

    #[test]
    fn test_empty() {
        let s = SecStr::empty();
        assert!(s.is_empty());
        assert_eq!(s.with(|p| p.len()), 0);
    }

    #[test]
    fn test_clone() {
        let a = SecStr::new("secret");
        let b = a.clone();
        assert_eq!(a.with(|p| p.to_string()), b.with(|p| p.to_string()));
    }
}
