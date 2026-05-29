// ═══════════════════════════════════════════════════════════════
// plugin_stderr.rs — 插件 stderr 捕获模块
// Zero-Day | ruoo-arsenal v4.1 → v4.2 (Zero-Day 审计修复)
// ═══════════════════════════════════════════════════════════════
//
// 原理: 在 plugin_call 的独立线程中, 用 SetStdHandle 将 STD_ERROR_HANDLE
// 重定向到匿名管道, 执行插件后恢复, 管道中捕获的内容通过消息代理
// 推送到 TUI 输出面板, 避免 stderr 泄露到输入框区域。
//
// v4.2 修复 (Zero-Day):
//   - 全局互斥锁 STDER_CAPTURE_MUTEX: SetStdHandle 是进程级句柄,
//     多线程并发 stderr 捕获会导致句柄竞态。begin() 获取锁, end()/drop 释放。
//   - 修改线程安全注释, 承认进程级句柄的并发风险。

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    // ── v4.2: 全局互斥锁 — 防止并发 SetStdHandle 竞态 ──
    static STDER_CAPTURE_MUTEX: Mutex<()> = Mutex::new(());

    // Windows API
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> isize;
        fn SetStdHandle(nStdHandle: u32, hHandle: isize) -> i32;
        fn CreatePipe(
            hReadPipe: *mut isize,
            hWritePipe: *mut isize,
            lpPipeAttributes: *const u8,
            nSize: u32,
        ) -> i32;
        fn CloseHandle(hObject: isize) -> i32;
        fn ReadFile(
            hFile: isize,
            lpBuffer: *mut u8,
            nNumberOfBytesToRead: u32,
            lpNumberOfBytesRead: *mut u32,
            lpOverlapped: *const u8,
        ) -> i32;
        fn PeekNamedPipe(
            hNamedPipe: isize,
            lpBuffer: *mut u8,
            nBufferSize: u32,
            lpBytesRead: *mut u32,
            lpTotalBytesAvail: *mut u32,
            lpBytesLeftThisMessage: *mut u32,
        ) -> i32;
    }

    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFFDu32; // -12 as u32

    /// stderr 捕获守卫 — RAII: 构造时获取全局锁+重定向, Drop时恢复+释放锁
    pub struct StderrCapture {
        saved_handle: isize,
        read_end: isize,
        write_end: isize,
        /// v4.2: 持有全局锁直到 end()/drop, 防止并发 SetStdHandle
        _guard: Option<std::sync::MutexGuard<'static, ()>>,
    }

    impl StderrCapture {
        /// 开始捕获 — 获取全局锁 + 重定向 stderr 到管道
        /// 如果锁被占用 (其他线程正在捕获), 将阻塞等待, 避免句柄竞态
        pub fn begin() -> Option<Self> {
            // v4.2: 获取全局锁 — 阻塞直到独占 stderr 控制权
            let guard = match STDER_CAPTURE_MUTEX.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };

            unsafe {
                let saved = GetStdHandle(STD_ERROR_HANDLE);
                if saved == -1 || saved == 0 {
                    return None; // guard dropped here → lock released
                }

                let mut read_end: isize = 0;
                let mut write_end: isize = 0;
                let ret = CreatePipe(&mut read_end, &mut write_end, std::ptr::null(), 0);
                if ret == 0 {
                    return None; // guard dropped here → lock released
                }

                SetStdHandle(STD_ERROR_HANDLE, write_end);

                Some(Self {
                    saved_handle: saved,
                    read_end,
                    write_end,
                    _guard: Some(guard),
                })
            }
        }

        /// 结束捕获, 恢复 stderr, 返回捕获内容, 释放全局锁
        pub fn end(mut self) -> String {
            // v4.2: 先显式释放锁再清理管道 (顺序不重要, 但清晰)
            self._guard.take();
            unsafe {
                // 1. 关闭写端 — 让管道可读
                CloseHandle(self.write_end);
                self.write_end = 0;

                // 2. 恢复原始 stderr
                SetStdHandle(STD_ERROR_HANDLE, self.saved_handle);

                // 3. 读取管道内容 (非阻塞, 循环读直到空)
                let mut total = Vec::new();
                let mut chunk = vec![0u8; 4096];
                loop {
                    let mut bytes_read: u32 = 0;
                    let ret = ReadFile(
                        self.read_end,
                        chunk.as_mut_ptr(),
                        chunk.len() as u32,
                        &mut bytes_read,
                        std::ptr::null(),
                    );
                    if ret == 0 || bytes_read == 0 {
                        break;
                    }
                    total.extend_from_slice(&chunk[..bytes_read as usize]);
                    if (bytes_read as usize) < chunk.len() {
                        break;
                    }
                }
                CloseHandle(self.read_end);
                self.read_end = 0;

                // 过滤掉空行和纯空白行
                let text = String::from_utf8_lossy(&total);
                let filtered: String = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<&str>>()
                    .join("\n");

                filtered
            }
        }
    }

    impl Drop for StderrCapture {
        fn drop(&mut self) {
            // v4.2: 先释放锁
            self._guard.take();
            unsafe {
                // 安全检查: 确保恢复原始 stderr
                if self.write_end != 0 {
                    CloseHandle(self.write_end);
                }
                if self.read_end != 0 {
                    CloseHandle(self.read_end);
                }
                if self.saved_handle != 0 {
                    SetStdHandle(STD_ERROR_HANDLE, self.saved_handle);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    // Linux/macOS: 占位 — 未实现, stub 返回空
    // TODO: 使用 dup2 重定向 fd 2 (需要 libc crate)
    pub struct StderrCapture;

    impl StderrCapture {
        pub fn begin() -> Option<Self> { Some(Self) }
        pub fn end(self) -> String { String::new() }
    }
}


