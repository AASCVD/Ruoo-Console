// ═══════════════════════════════════════════════════════════════
// plugin_stderr.rs — 插件 stderr 捕获模块
// Zero-Day | ruoo-arsenal v4.1
// ═══════════════════════════════════════════════════════════════
//
// 原理: 在 plugin_call 的独立线程中, 用 SetStdHandle 将 STD_ERROR_HANDLE
// 重定向到匿名管道, 执行插件后恢复, 管道中捕获的内容通过消息代理
// 推送到 TUI 输出面板, 避免 stderr 泄露到输入框区域。
//
// 线程安全: stderr 是进程级句柄, 但本模块只在 plugin_call 的隔离线程
// 中使用, 且执行前后保存/恢复, 与其他线程互不干扰。

#[cfg(windows)]
mod imp {
    // (no extra imports needed — all Windows APIs via extern "system")

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

    /// stderr 捕获守卫 — RAII: 构造时重定向, Drop时恢复
    pub struct StderrCapture {
        saved_handle: isize,
        read_end: isize,
        write_end: isize,
    }

    impl StderrCapture {
        /// 开始捕获 — 重定向 stderr 到管道
        pub fn begin() -> Option<Self> {
            unsafe {
                let saved = GetStdHandle(STD_ERROR_HANDLE);
                if saved == -1 || saved == 0 {
                    return None;
                }

                let mut read_end: isize = 0;
                let mut write_end: isize = 0;
                let ret = CreatePipe(&mut read_end, &mut write_end, std::ptr::null(), 0);
                if ret == 0 {
                    return None;
                }

                SetStdHandle(STD_ERROR_HANDLE, write_end);

                Some(Self {
                    saved_handle: saved,
                    read_end,
                    write_end,
                })
            }
        }

        /// 结束捕获, 恢复 stderr, 返回捕获内容
        pub fn end(mut self) -> String {
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

pub use imp::StderrCapture;
