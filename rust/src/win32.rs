//! 全应用共用的 Win32 底层设施：窗口矩形 / 光标辅助函数、DWM
//! 属性、桌面层嵌入支持、单实例以及窗口
//! 激活。对 `windows` crate 中已有的调用直接使用它；
//! 显示器 DDC/CI 入口点位于 `ddc.rs`，因为它们是从
//! monitorconfigapi.dll / dxva2.dll 动态加载的。

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, MonitorFromWindow, SetWindowRgn, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetCursorPos, GetDesktopWindow, GetForegroundWindow,
    GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, GWL_EXSTYLE, GWLP_HWNDPARENT, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_RESTORE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::{s, w, PCWSTR};

pub const HWND_BOTTOM: HWND = HWND(1 as _);
pub const HWND_NOTOPMOST: HWND = HWND(-2 as _);

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 无边框窗口的 DWM 边框颜色：表示 #28292A 的 COLORREF（0x00BBGGRR）
/// （B=0x2A，G=0x29，R=0x28）。
pub const BORDER_COLOR_DARK: u32 = 0x002A2928;

pub unsafe fn get_window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = windows::Win32::Foundation::RECT::default();
    if GetWindowRect(hwnd, &mut r).is_ok() {
        Some(Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        })
    } else {
        None
    }
}

pub unsafe fn get_cursor_pos() -> Option<Point> {
    let mut p = windows::Win32::Foundation::POINT::default();
    if GetCursorPos(&mut p).is_ok() {
        Some(Point { x: p.x, y: p.y })
    } else {
        None
    }
}

pub unsafe fn move_window(hwnd: HWND, x: i32, y: i32) {
    SetWindowPos(
        hwnd,
        HWND::default(),
        x,
        y,
        0,
        0,
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
    );
}

pub unsafe fn resize_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    SetWindowPos(
        hwnd,
        HWND::default(),
        x,
        y,
        w,
        h,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

/// DWMWA_SYSTEMBACKDROP_TYPE：0=自动，1=无，2=主窗口（Mica），
/// 3=瞬态窗口（acrylic 高斯模糊），4=选项卡式（Mica Alt）。
/// 模糊模式使用 3（acrylic），与 C# 构建的 acrylic 材质一致。
pub unsafe fn set_backdrop_type(hwnd: HWND, kind: u32) {
    let v = kind as i32;
    DwmSetWindowAttribute(
        hwnd,
        DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
        &v as *const i32 as *const core::ffi::c_void,
        std::mem::size_of::<i32>() as u32,
    );
}

/// 强制窗口进入深色模式（DWMWA_USE_IMMERSIVE_DARK_MODE）。若不这样做，
/// 每当桌面层窗口失去激活状态，DWM acrylic 材质就会切换到浅色的
/// “非激活”色调（看起来是白色）。
/// Windows 10 build 19041+ 使用属性 20；19 是旧值。
pub unsafe fn force_dark_mode(hwnd: HWND) {
    let v: i32 = 1;
    let attr = if DwmSetWindowAttribute(
        hwnd,
        DWMWINDOWATTRIBUTE(20),
        &v as *const i32 as *const core::ffi::c_void,
        std::mem::size_of::<i32>() as u32,
    )
    .is_err()
    {
        DWMWINDOWATTRIBUTE(19)
    } else {
        return;
    };
    let _ = DwmSetWindowAttribute(
        hwnd,
        attr,
        &v as *const i32 as *const core::ffi::c_void,
        std::mem::size_of::<i32>() as u32,
    );
}

// ---- 固定 acrylic 模糊（SetWindowCompositionAttribute） ----
//
// 与 DWMWA_SYSTEMBACKDROP_TYPE 的 acrylic 不同，这条未公开的合成
// 路径渲染的是一种恒定的 acrylic 模糊，窗口失去激活状态时
// 不会切换到浅色纯色回退。正是这种非激活状态下的
// 回退，导致焦点离开桌面层挂件时
// DWM 背景变白。

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32, // AABBGGRR
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut core::ffi::c_void,
    size: usize,
}

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

/// 启用/禁用整个窗口背后的固定 acrylic 模糊。渐变颜色
/// 即材质色调（与 C# 构建的
/// DesktopAcrylic TintColor/TintOpacity 作用相同）：AABBGGRR 格式的 #202226，alpha 为 0.29
/// （0x4A），这同时中和了默认
/// 近乎透明的 acrylic 色调所带的紫色偏色。
pub unsafe fn set_acrylic_blur(hwnd: HWND, enabled: bool) {
    let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
        return;
    };
    let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
        return;
    };
    type SetAttr = unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;
    let set_attr: SetAttr = std::mem::transmute_copy(&proc);

    // 色调 #202226（R=0x20 G=0x22 B=0x26），alpha 0.29 -> 0x4A262220。
    let mut policy = AccentPolicy {
        accent_state: if enabled {
            ACCENT_ENABLE_ACRYLICBLURBEHIND
        } else {
            ACCENT_DISABLED
        },
        accent_flags: 0,
        gradient_color: if enabled { 0x4A262220 } else { 0 },
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut AccentPolicy as *mut core::ffi::c_void,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    let _ = set_attr(hwnd, &mut data);
}

/// 绘制 1px 的 DWM 边框 #28292A，使无边框边界与
/// 卡片轮廓融合，而不会渲染成黑色。
pub unsafe fn set_border_color(hwnd: HWND) {
    let v = BORDER_COLOR_DARK;
    DwmSetWindowAttribute(
        hwnd,
        DWMWINDOWATTRIBUTE(34), // DWMWA_BORDER_COLOR
        &v as *const u32 as *const core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
}

/// 将窗口裁剪为圆角矩形（所有尺寸均为物理像素）。
/// 在圆角区域之外，窗口完全透明且对命中测试
/// 透明，因此材质和鼠标形状与圆角
/// 卡片完全一致（不存在可见/可点击的外部边距）。这也是隐藏
/// 非客户区标题栏的原因，从而让挂件保持无边框。
/// 每当窗口尺寸改变或 DPI 变化时都必须重新应用。
pub unsafe fn set_round_region(hwnd: HWND, w_px: i32, h_px: i32, radius_px: i32) {
    if w_px <= 0 || h_px <= 0 || radius_px <= 0 {
        return;
    }
    let rgn = CreateRoundRectRgn(
        0,
        0,
        w_px + 1,
        h_px + 1,
        radius_px * 2,
        radius_px * 2,
    );
    if rgn.is_invalid() {
        return;
    }
    // SetWindowRgn 会接管该区域的所有权（不要删除它）。
    let _ = SetWindowRgn(hwnd, rgn, BOOL(1));
}

pub unsafe fn get_ex_style(hwnd: HWND) -> isize {
    GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
}

pub unsafe fn set_ex_style(hwnd: HWND, style: isize) {
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
    SetWindowPos(
        hwnd,
        HWND::default(),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
}

pub const fn ex_style_no_activate(style: isize) -> isize {
    style | (WS_EX_NOACTIVATE.0 as isize)
}

pub const fn ex_style_without_no_activate(style: isize) -> isize {
    style & !(WS_EX_NOACTIVATE.0 as isize)
}

/// 将窗口从任务栏和 Alt+Tab 中移除，使托盘图标成为
/// 唯一入口。这相当于 C# 构建中
/// `AppWindow.IsShownInSwitchers = false` 的 Win32 等价做法：WS_EX_TOOLWINDOW 把窗口标记为
/// 工具窗口（shell 从不列出这类窗口），并清除 WS_EX_APPWINDOW
/// 以防 winit 设置过它。
pub unsafe fn hide_from_taskbar(hwnd: HWND) {
    let style = get_ex_style(hwnd);
    let updated = (style | (WS_EX_TOOLWINDOW.0 as isize)) & !(WS_EX_APPWINDOW.0 as isize);
    if updated != style {
        set_ex_style(hwnd, updated);
    }
}

// ---- 桌面层（SHELLDLL_DefView） ----

/// 查找 Explorer 桌面图标宿主（SHELLDLL_DefView），且不会强制
/// Explorer 创建它。若桌面图标已隐藏 / Explorer 仍在
/// 启动中则返回 None。
pub unsafe fn find_desktop_icon_view() -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut Option<HWND>);
        let Ok(child) = FindWindowExW(hwnd, HWND::default(), w!("SHELLDLL_DefView"), PCWSTR::null())
        else {
            return BOOL(1);
        };
        if !child.is_invalid() {
            *state = Some(child);
            BOOL(0) // 停止
        } else {
            BOOL(1)
        }
    }
    EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize)).ok()?;
    found
}

/// 将窗口挂接到桌面图标层（浮在壁纸之上，
/// 位于所有应用程序窗口之下）。成功时返回 true。
pub unsafe fn attach_to_desktop_layer(hwnd: HWND) -> bool {
    let Some(def_view) = find_desktop_icon_view() else {
        return false;
    };
    if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) != def_view.0 as isize {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, def_view.0 as isize);
    }
    SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    SetWindowPos(
        hwnd,
        HWND_BOTTOM,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    true
}

/// 将窗口推到普通 Z 序的底部（在不存在桌面图标宿主时的
/// 回退方案）。
pub unsafe fn move_to_bottom(hwnd: HWND) {
    SetWindowPos(
        hwnd,
        HWND_BOTTOM,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

// ---- 单实例 ----

/// 创建一个命名互斥体；当本进程创建了它（即唯一
/// 实例）时返回 true。
pub unsafe fn acquire_single_instance(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = CreateMutexW(None, BOOL(1), PCWSTR(wide.as_ptr()));
    let Ok(handle) = handle else {
        return false;
    };
    !handle.is_invalid() && windows::Win32::Foundation::GetLastError().0 == 0
}

/// 将已在运行的 RogControl 窗口带到前台（由第二个
/// 实例在退出前调用）。
pub unsafe fn activate_existing_instance(title: &str) {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) {
        if !hwnd.is_invalid() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
            return;
        }
    }
    // 回退到前台窗口（也许该实例正是活动实例）。
    let fg = GetForegroundWindow();
    if !fg.is_invalid() {
        SetForegroundWindow(fg);
    }
}

// ---- 杂项 ----

pub unsafe fn primary_monitor_from_desktop() -> Option<HMONITOR> {
    let desktop = GetDesktopWindow();
    let hmon = MonitorFromWindow(desktop, MONITOR_DEFAULTTOPRIMARY);
    (!hmon.is_invalid()).then_some(hmon)
}

/// 查找本进程自己的可见顶层窗口（即 eframe 窗口）。
pub unsafe fn find_own_window() -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut Option<HWND>);
        let mut p = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut p));
        if p == std::process::id() && IsWindowVisible(hwnd).as_bool() {
            *state = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize)).ok()?;
    found
}

/// 物理像素为单位的主屏分辨率。它是挂件
/// 布局的基准：ROG 的 4K/1K 切换会改变该值（3840x2160 -> 1920x1080），因此
/// 所存储的比例能让挂件的宽度和高度随之精确缩放。
pub unsafe fn get_screen_size() -> Option<(i32, i32)> {
    let w = GetSystemMetrics(SM_CXSCREEN);
    let h = GetSystemMetrics(SM_CYSCREEN);
    (w > 0 && h > 0).then_some((w, h))
}

/// 物理屏幕像素为单位的主工作区。
pub unsafe fn get_work_area() -> Option<Rect> {
    let mut r = windows::Win32::Foundation::RECT::default();
    let ok = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut r as *mut _ as *mut core::ffi::c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    if ok.is_ok() {
        Some(Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        })
    } else {
        None
    }
}
