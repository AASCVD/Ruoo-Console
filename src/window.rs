// ============================================================
// RUOO-ARSENAL v5.2 — 窗口控制模块
// 隐藏到系统托盘 (Shell_NotifyIcon) + 全局热键恢复
// 从任务栏彻底消失 → 仅托盘图标可见, 右键菜单 Show/Exit
// Windows: ShowWindow + RegisterHotKey + Shell_NotifyIcon + 消息窗口
// Linux/macOS: stub (终端模拟器自行管理)
// ============================================================

#[cfg(windows)]
pub mod win32 {
    #![allow(non_snake_case)]
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    // ── Windows API 类型 ──
    type HWND = isize;
    type BOOL = i32;
    type UINT = u32;
    type WPARAM = usize;
    type LPARAM = isize;
    type LRESULT = isize;
    type HINSTANCE = isize;
    type LPCWSTR = *const u16;
    type DWORD = u32;
    type ATOM = u16;
    type HMENU = isize;
    #[allow(non_camel_case_types)]
    type LONG_PTR = isize;
    type HICON = isize;
    #[allow(non_camel_case_types)]
    type ULONG_PTR = usize;

    // ── ShowWindow ──
    const SW_HIDE: i32 = 0;
    const SW_SHOW: i32 = 5;
    const SW_RESTORE: i32 = 9;

    // ── 热键 ──
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_NOREPEAT: u32 = 0x4000;

    // ── 窗口消息 ──
    const WM_HOTKEY: u32 = 0x0312;
    const WM_DESTROY: u32 = 0x0002;
    const WM_COMMAND: u32 = 0x0111;
    const WM_USER: u32 = 0x0400;
    const WM_APP: u32 = 0x8000;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;

    // ── 自定义消息 (托盘管理) ──
    const WM_TRAY_ADD: u32 = WM_APP + 100;
    const WM_TRAY_REMOVE: u32 = WM_APP + 101;
    const WM_TRAYICON: u32 = WM_APP + 102;

    // ── 窗口 ──
    const HWND_MESSAGE: isize = -3;
    const CW_USEDEFAULT: i32 = 0x80000000u32 as i32;

    // ── 窗口扩展样式 ──
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: DWORD = 0x00000080;
    const WS_EX_APPWINDOW: DWORD = 0x00040000;

    // ── Shell_NotifyIcon ──
    const NIM_ADD: DWORD = 0x00000000;
    const NIM_DELETE: DWORD = 0x00000002;
    const NIM_MODIFY: DWORD = 0x00000001;
    const NIF_MESSAGE: UINT = 0x00000001;
    const NIF_ICON: UINT = 0x00000002;
    const NIF_TIP: UINT = 0x00000004;

    // ── 系统图标 ──
    const IDI_SHIELD: LPCWSTR = 32518 as LPCWSTR;  // UAC 盾牌图标
    const IDI_APPLICATION: LPCWSTR = 32512 as LPCWSTR;

    // ── 弹出菜单 ──
    const TPM_RIGHTBUTTON: UINT = 0x0002;
    const TPM_BOTTOMALIGN: UINT = 0x0020;
    const TPM_LEFTALIGN: UINT = 0x0000;
    const MF_STRING: UINT = 0x00000000;
    const MF_SEPARATOR: UINT = 0x00000800;

    // ── 菜单命令ID ──
    const IDM_RESTORE: usize = 2001;
    const IDM_EXIT: usize = 2002;

    // ── 全局热键ID ──
    const HOTKEY_ID_SHOW: i32 = 1;
    const HOTKEY_ID_TOGGLE: i32 = 2;

    // ── 全局状态: 托盘图标是否已添加 ──
    static TRAY_ADDED: AtomicBool = AtomicBool::new(false);
    // ── 全局状态: 热键窗口句柄 (主线程 post 消息用, AtomicIsize 保证线程安全) ──
    static HOTKEY_HWND: AtomicIsize = AtomicIsize::new(0);
    // ── 退出请求标志 ──
    static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    struct MSG {
        hwnd: HWND,
        message: u32,
        wParam: WPARAM,
        lParam: LPARAM,
        time: u32,
        pt: POINT,
    }

    #[repr(C)]
    struct POINT { x: i32, y: i32 }

    #[repr(C)]
    struct WNDCLASSEXW {
        cbSize: u32,
        style: u32,
        lpfnWndProc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
        cbClsExtra: i32,
        cbWndExtra: i32,
        hInstance: HINSTANCE,
        hIcon: HICON,
        hCursor: isize,
        hbrBackground: isize,
        lpszMenuName: LPCWSTR,
        lpszClassName: LPCWSTR,
        hIconSm: HICON,
    }

    /// 系统托盘通知结构 (V1兼容, XP~Win11)
    #[repr(C)]
    struct NOTIFYICONDATAW {
        cbSize: DWORD,
        hWnd: HWND,
        uID: UINT,
        uFlags: UINT,
        uCallbackMessage: UINT,
        hIcon: HICON,
        szTip: [u16; 128],
    }

    extern "system" {
        // ── 控制台窗口 ──
        fn GetConsoleWindow() -> HWND;
        fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
        fn IsWindowVisible(hWnd: HWND) -> BOOL;

        // ── 热键 ──
        fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: u32, vk: u32) -> BOOL;
        fn UnregisterHotKey(hWnd: HWND, id: i32) -> BOOL;

        // ── 消息 ──
        fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: u32, wMsgFilterMax: u32) -> BOOL;
        fn PostQuitMessage(nExitCode: i32);
        fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
        fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;

        // ── 窗口创建/销毁 ──
        fn CreateWindowExW(
            dwExStyle: DWORD, lpClassName: LPCWSTR, lpWindowName: LPCWSTR,
            dwStyle: DWORD, X: i32, Y: i32, nWidth: i32, nHeight: i32,
            hWndParent: HWND, hMenu: isize, hInstance: HINSTANCE, lpParam: *mut std::ffi::c_void,
        ) -> HWND;
        fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        fn GetModuleHandleW(lpModuleName: LPCWSTR) -> HINSTANCE;
        fn DestroyWindow(hWnd: HWND) -> BOOL;

        // ── 窗口扩展样式 ──
        fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> LONG_PTR;
        fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: LONG_PTR) -> LONG_PTR;

        // ── 系统托盘 ──
        fn Shell_NotifyIconW(dwMessage: DWORD, lpData: *const NOTIFYICONDATAW) -> BOOL;

        // ── 图标 ──
        fn LoadIconW(hInstance: HINSTANCE, lpIconName: LPCWSTR) -> HICON;

        // ── 右键菜单 ──
        fn CreatePopupMenu() -> HMENU;
        fn AppendMenuW(hMenu: HMENU, uFlags: UINT, uIDNewItem: ULONG_PTR, lpNewItem: LPCWSTR) -> BOOL;
        fn TrackPopupMenu(hMenu: HMENU, uFlags: UINT, x: i32, y: i32, nReserved: i32, hWnd: HWND, prcRect: *const std::ffi::c_void) -> BOOL;
        fn DestroyMenu(hMenu: HMENU) -> BOOL;
        fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
        fn SetForegroundWindow(hWnd: HWND) -> BOOL;
    }

    // ── 宽字符串工具 ──
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // ══════════════════════════════════════════════════
    // 系统托盘图标 — 添加/删除
    // ══════════════════════════════════════════════════

    /// 添加系统托盘图标 (由热键线程调用)
    unsafe fn tray_add(hwnd: HWND) {
        if TRAY_ADDED.load(Ordering::Relaxed) {
            return; // 已添加
        }

        // 尝试加载 UAC 盾牌图标, 失败则用默认应用图标
        let hicon = LoadIconW(0, IDI_SHIELD);
        let hicon = if hicon == 0 { LoadIconW(0, IDI_APPLICATION) } else { hicon };
        if hicon == 0 { return; }

        let tip = to_wide("RUOO Arsenal");
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as DWORD,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: hicon,
            szTip: [0u16; 128],
        };
        // 复制提示文本 (最多127字符+null)
        for (i, &ch) in tip.iter().enumerate().take(127) {
            nid.szTip[i] = ch;
        }

        Shell_NotifyIconW(NIM_ADD, &nid);
        TRAY_ADDED.store(true, Ordering::Relaxed);
    }

    /// 删除系统托盘图标 (由热键线程调用)
    unsafe fn tray_remove(hwnd: HWND) {
        if !TRAY_ADDED.load(Ordering::Relaxed) {
            return;
        }

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as DWORD,
            hWnd: hwnd,
            uID: 1,
            uFlags: 0,
            uCallbackMessage: 0,
            hIcon: 0,
            szTip: [0u16; 128],
        };
        Shell_NotifyIconW(NIM_DELETE, &nid);
        TRAY_ADDED.store(false, Ordering::Relaxed);
    }

    /// 显示托盘右键菜单
    unsafe fn tray_show_menu(hwnd: HWND) {
        let menu = CreatePopupMenu();
        if menu == 0 { return; }

        // "显示 RUOO" 菜单项
        let restore_text = to_wide("显示 RUOO");
        AppendMenuW(menu, MF_STRING, IDM_RESTORE as ULONG_PTR, restore_text.as_ptr());

        // 分隔线
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());

        // "退出" 菜单项
        let exit_text = to_wide("退出");
        AppendMenuW(menu, MF_STRING, IDM_EXIT as ULONG_PTR, exit_text.as_ptr());

        // 获取光标位置
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);

        // ★ 必须先设前台窗口, 否则菜单点击后不会消失
        SetForegroundWindow(hwnd);

        // 显示弹出菜单
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            pt.x, pt.y,
            0, hwnd, std::ptr::null(),
        );

        // ★ 发送一个空消息确保菜单关闭
        PostMessageW(hwnd, 0, 0, 0);

        DestroyMenu(menu);
    }

    // ══════════════════════════════════════════════════
    // 任务栏按钮 — 隐藏/恢复
    // ══════════════════════════════════════════════════

    /// 从任务栏移除窗口 (添加 WS_EX_TOOLWINDOW)
    unsafe fn taskbar_hide(hwnd: HWND) {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TOOLWINDOW as LONG_PTR);
    }

    /// 恢复任务栏按钮 (移除 WS_EX_TOOLWINDOW)
    unsafe fn taskbar_show(hwnd: HWND) {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_TOOLWINDOW as LONG_PTR));
    }

    // ══════════════════════════════════════════════════
    // 窗口过程 (热键 + 托盘 + 自定义消息)
    // ══════════════════════════════════════════════════

    unsafe extern "system" fn hotkey_wndproc(
        hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
    ) -> LRESULT {
        // ── 全局热键 ──
        if msg == WM_HOTKEY {
            let id = wparam as i32;
            if id == HOTKEY_ID_SHOW || id == HOTKEY_ID_TOGGLE {
                show_console_raw();
                return 0;
            }
        }

        // ── 托盘图标回调 ──
        if msg == WM_TRAYICON {
            let event = lparam as u32;
            if event == WM_LBUTTONUP {
                // 左键单击托盘图标 → 恢复窗口
                show_console_raw();
                return 0;
            }
            if event == WM_RBUTTONUP {
                // 右键单击托盘图标 → 弹出菜单
                tray_show_menu(hwnd);
                return 0;
            }
        }

        // ── 菜单命令 ──
        if msg == WM_COMMAND {
            let cmd_id = wparam as usize;
            if cmd_id == IDM_RESTORE {
                show_console_raw();
                return 0;
            }
            if cmd_id == IDM_EXIT {
                // 先恢复窗口(否则进程可能残留), 然后请求退出
                show_console_raw();
                EXIT_REQUESTED.store(true, Ordering::SeqCst);
                return 0;
            }
        }

        // ── 自定义消息: 添加托盘图标 ──
        if msg == WM_TRAY_ADD {
            tray_add(hwnd);
            return 0;
        }

        // ── 自定义消息: 删除托盘图标 ──
        if msg == WM_TRAY_REMOVE {
            tray_remove(hwnd);
            return 0;
        }

        // ── 窗口销毁 ──
        if msg == WM_DESTROY {
            tray_remove(hwnd);
            PostQuitMessage(0);
            return 0;
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    // ══════════════════════════════════════════════════
    // 公开接口: 隐藏/显示/切换
    // ══════════════════════════════════════════════════

    /// 隐藏到系统托盘 — 窗口消失, 任务栏图标也消失, 仅托盘图标保留
    pub fn hide_console() {
        unsafe {
            let hwnd = GetConsoleWindow();
            if hwnd == 0 { return; }

            // 1. 先从任务栏移除
            taskbar_hide(hwnd);
            // 2. 隐藏窗口
            ShowWindow(hwnd, SW_HIDE);
            // 3. 通知热键线程添加托盘图标
            let hotkey_hwnd = HOTKEY_HWND.load(Ordering::Relaxed);
            if hotkey_hwnd != 0 {
                PostMessageW(hotkey_hwnd, WM_TRAY_ADD, 0, 0);
            }
        }
    }

    /// 从托盘恢复 — 窗口+任务栏图标重新出现, 托盘图标消失
    unsafe fn show_console_raw() {
        let hwnd = GetConsoleWindow();
        if hwnd == 0 { return; }

        // 1. 通知热键线程删除托盘图标
        let hotkey_hwnd = HOTKEY_HWND.load(Ordering::Relaxed);
        if hotkey_hwnd != 0 {
            PostMessageW(hotkey_hwnd, WM_TRAY_REMOVE, 0, 0);
        }
        // 2. 恢复任务栏按钮
        taskbar_show(hwnd);
        // 3. 显示并置顶
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }

    /// 显示控制台窗口 (公开接口)
    pub fn show_console() {
        unsafe { show_console_raw(); }
    }

    /// 切换控制台可见性 (含托盘管理)
    pub fn toggle_console() -> bool {
        unsafe {
            let hwnd = GetConsoleWindow();
            if hwnd == 0 { return false; }

            if IsWindowVisible(hwnd) != 0 {
                // 当前可见 → 隐藏到托盘
                hide_console();
                false
            } else {
                // 当前隐藏 → 恢复
                show_console_raw();
                true
            }
        }
    }

    /// 查询窗口是否可见
    pub fn is_visible() -> bool {
        unsafe {
            let hwnd = GetConsoleWindow();
            hwnd != 0 && IsWindowVisible(hwnd) != 0
        }
    }

    /// 查询是否已请求退出 (通过托盘右键菜单)
    pub fn exit_requested() -> bool {
        EXIT_REQUESTED.load(Ordering::Relaxed)
    }

    // ══════════════════════════════════════════════════
    // 后台线程: 全局热键 + 托盘消息泵
    // ══════════════════════════════════════════════════

    /// 启动全局热键+托盘后台线程
    /// 注册 Ctrl+Shift+S (显示) 和 Ctrl+Shift+H (切换)
    /// 返回一个 AtomicBool，当热键触发时设为 true
    pub fn start_hotkey_thread() -> Arc<AtomicBool> {
        let show_requested = Arc::new(AtomicBool::new(false));
        let flag = show_requested.clone();

        thread::Builder::new()
            .name("ruoo-hotkey".into())
            .spawn(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let class_name = to_wide("RUOO_HotkeyWindow");
                    let hinst = GetModuleHandleW(std::ptr::null());

                    let wc = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        style: 0,
                        lpfnWndProc: hotkey_wndproc,
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: hinst,
                        hIcon: 0,
                        hCursor: 0,
                        hbrBackground: 0,
                        lpszMenuName: std::ptr::null(),
                        lpszClassName: class_name.as_ptr(),
                        hIconSm: 0,
                    };

                    if RegisterClassExW(&wc) == 0 {
                        return;
                    }

                    let hwnd = CreateWindowExW(
                        0, class_name.as_ptr(), std::ptr::null(),
                        0, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
                        HWND_MESSAGE, 0, hinst, std::ptr::null_mut(),
                    );

                    if hwnd == 0 {
                        return;
                    }

                    // ★ 存储 HWND 供主线程 post 消息
                    HOTKEY_HWND.store(hwnd, Ordering::Relaxed);

                    // 注册两个全局热键
                    let vk_s = 'S' as u32;
                    RegisterHotKey(hwnd, HOTKEY_ID_SHOW, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, vk_s);

                    let vk_h = 'H' as u32;
                    RegisterHotKey(hwnd, HOTKEY_ID_TOGGLE, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, vk_h);

                    // 消息循环
                    let mut msg = MSG {
                        hwnd: 0,
                        message: 0,
                        wParam: 0,
                        lParam: 0,
                        time: 0,
                        pt: POINT { x: 0, y: 0 },
                    };
                    while GetMessageW(&mut msg, 0, 0, 0) != 0 {
                        if msg.message == WM_HOTKEY {
                            flag.store(true, Ordering::SeqCst);
                        }
                    }

                    // 清理
                    tray_remove(hwnd);
                    UnregisterHotKey(hwnd, HOTKEY_ID_SHOW);
                    UnregisterHotKey(hwnd, HOTKEY_ID_TOGGLE);
                    HOTKEY_HWND.store(0, Ordering::Relaxed);
                    DestroyWindow(hwnd);
                }
                })); // catch_unwind
            })
            .ok();

        show_requested
    }
}

// ═══════════════════════════════════════════════════════════
// 跨平台兼容层 (非Windows平台提供空实现)
// ═══════════════════════════════════════════════════════════

#[cfg(not(windows))]
pub mod win32 {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub fn hide_console() {
        // Linux/macOS: 终端由窗口管理器控制，无法直接隐藏
    }

    pub fn show_console() {}

    pub fn toggle_console() -> bool { false }

    pub fn is_visible() -> bool { true }

    pub fn exit_requested() -> bool { false }

    pub fn start_hotkey_thread() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
}

// ── 重新导出 (简化调用) ──
#[cfg(windows)]
pub use win32::*;
#[cfg(not(windows))]
pub use win32::*;
