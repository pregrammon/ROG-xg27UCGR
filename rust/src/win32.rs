//! 全应用共用的 Win32 底层设施：窗口矩形 / 光标辅助函数、DWM
//! 属性、桌面层嵌入支持、单实例以及窗口
//! 激活。对 `windows` crate 中已有的调用直接使用它；
//! 显示器 DDC/CI 入口点位于 `ddc.rs`，因为它们是从
//! monitorconfigapi.dll / dxva2.dll 动态加载的。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};

use windows::Win32::Foundation::{
    COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM, BOOL, CloseHandle,
};
use windows::Win32::Graphics::Dwm::{
    DwmEnableBlurBehindWindow, DwmSetWindowAttribute, DWM_BLURBEHIND, DWM_BB_BLURREGION,
    DWM_BB_ENABLE, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn, DeleteDC, DeleteObject, GetDC,
    MapWindowPoints, MonitorFromWindow, ReleaseDC, SelectObject, SetWindowRgn, AC_SRC_ALPHA,
    AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP,
    HDC, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW,
    GetCursorPos, GetDesktopWindow, GetForegroundWindow, GetParent, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    SystemParametersInfoW, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, GWL_EXSTYLE, GWLP_HWNDPARENT, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_VISIBLE,
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
/// 即材质色调：AABBGGRR 格式的 #D9D9D9，alpha 为 0.90
/// （0xE6，即 10% 透明度）。中性浅白灰，不再偏红。
pub unsafe fn set_acrylic_blur(hwnd: HWND, enabled: bool) {
    let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
        return;
    };
    let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
        return;
    };
    type SetAttr = unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;
    let set_attr: SetAttr = std::mem::transmute_copy(&proc);

    // 色调 #D9D9D9（R=G=B=0xD9），alpha 0.90 -> 0xE6D9D9D9。
    // 中性浅白灰：接近白色、不偏红，同时保留一点高斯模糊。
    let mut policy = AccentPolicy {
        accent_state: if enabled {
            ACCENT_ENABLE_ACRYLICBLURBEHIND
        } else {
            ACCENT_DISABLED
        },
        accent_flags: 0,
        gradient_color: if enabled { 0x000000E6 } else { 0 },
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut AccentPolicy as *mut core::ffi::c_void,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    let _ = set_attr(hwnd, &mut data);
}

/// 请求 DWM 使用圆角窗口（Windows 11+）。acrylic 背景由 DWM
/// 合成，SetWindowRgn 无法裁剪它，因此必须同时请求系统圆角；
/// 旧版 Windows 会忽略这个属性。
pub unsafe fn prefer_round_corners(hwnd: HWND) {
    let preference: u32 = 2; // DWMWCP_ROUND
    DwmSetWindowAttribute(
        hwnd,
        DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
        &preference as *const u32 as *const core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
}

/// 使用 DWM blur-behind，并把模糊区域裁成圆角矩形。
/// `ACCENT_ENABLE_ACRYLICBLURBEHIND` 的合成层会忽略窗口区域，
/// 导致模糊背景在四角保持直角；blur-behind 允许显式传入区域。
pub unsafe fn set_blur_behind(
    hwnd: HWND,
    w_px: i32,
    h_px: i32,
    radius_px: i32,
    enabled: bool,
) {
    let mut flags = DWM_BB_ENABLE;
    let mut region = windows::Win32::Graphics::Gdi::HRGN(std::ptr::null_mut());
    if enabled && w_px > 0 && h_px > 0 && radius_px > 0 {
        region = CreateRoundRectRgn(
            0,
            0,
            w_px + 1,
            h_px + 1,
            radius_px * 2,
            radius_px * 2,
        );
        if !region.is_invalid() {
            flags |= DWM_BB_BLURREGION;
        }
    }
    let blur = DWM_BLURBEHIND {
        dwFlags: flags,
        fEnable: BOOL(if enabled { 1 } else { 0 }),
        hRgnBlur: region,
        fTransitionOnMaximized: BOOL(0),
    };
    let _ = DwmEnableBlurBehindWindow(hwnd, &blur);
    if !region.is_invalid() {
        let _ = DeleteObject(HGDIOBJ(region.0));
    }
}

/// 移除 DWM 的 1px 窗口边框，让挂件使用自己绘制的白色半透明边框。
pub unsafe fn clear_border_color(hwnd: HWND) {
    let none: u32 = 0xFFFFFFFE; // DWMWA_COLOR_NONE
    DwmSetWindowAttribute(
        hwnd,
        DWMWINDOWATTRIBUTE(34), // DWMWA_BORDER_COLOR
        &none as *const u32 as *const core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
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

/// 去掉 WS_EX_LAYERED。acrylic 由 DWM 合成，去掉分层窗口后
/// 窗口区域才能把 acrylic 背景裁成圆角。
pub unsafe fn remove_layered(hwnd: HWND) {
    const WS_EX_LAYERED: isize = 0x0008_0000;
    let style = get_ex_style(hwnd);
    if style & WS_EX_LAYERED != 0 {
        set_ex_style(hwnd, style & !WS_EX_LAYERED);
    }
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

/// 切换显示模式前解除桌面层挂接并隐藏窗口。
///
/// 小挂件是 `SHELLDLL_DefView` 的 owned window：显示器模式切换时
/// Explorer 可能重建桌面，owned window 会随宿主一起被销毁，进而让
/// eframe 认为根窗口关闭而退出。临时解除所有权即可避免。
pub unsafe fn detach_from_desktop(hwnd: HWND) {
    SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);
    let _ = ShowWindow(hwnd, SW_HIDE);
    SetWindowPos(
        hwnd,
        HWND_BOTTOM,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// 显示模式切换结束后重新挂回桌面层并显示。
pub unsafe fn reattach_to_desktop(hwnd: HWND) {
    if !attach_to_desktop_layer(hwnd) {
        move_to_bottom(hwnd);
    }
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
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

unsafe extern "system" {
    fn SetProcessDPIAware() -> i32;
}

/// 保持**系统级** DPI 感知（而不是 winit 默认的 per-monitor v2）。
///
/// 4K（150%）与 1K（100%）切换会改变显示器 DPI；per-monitor 感知会让
/// egui 在切换瞬间重排纹理，触发 epaint 的
/// “Partial texture update is outside the bounds” panic。系统级感知下
/// DPI 固定，Windows 只做位图缩放，切换稳定（代价是略微模糊）。
pub fn make_system_dpi_aware() {
    // winit 随后会尝试设置 per-monitor v2，但进程 awareness 已锁定，
    // 那些调用会失败并被忽略。
    unsafe {
        SetProcessDPIAware();
    }
}

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

// ---- 按标题查找窗口 ----

pub unsafe fn find_window_by_title(title: &str) -> Option<HWND> {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let hwnd = FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())).ok()?;
    (!hwnd.is_invalid()).then_some(hwnd)
}

/// 主窗口的深色标题栏 + 边框色。窗口每次重建都需要重新应用。
pub unsafe fn apply_dark_title(hwnd: HWND) {
    force_dark_mode(hwnd);
    set_border_color(hwnd);
}

// ---------------------------------------------------------------------------
// 桌面挂件的独立阴影窗口
// ---------------------------------------------------------------------------

const SHADOW_MARGIN: i32 = 18;
const SHADOW_OFFSET: i32 = 5;
const SHADOW_SIGMA: f32 = 9.0;
const SHADOW_ALPHA: f32 = 0.30;

unsafe extern "system" fn shadow_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 创建一个顶层透明分层窗口，放在挂件下方，用来绘制外阴影。
pub unsafe fn create_shadow_window(_widget: HWND) -> Option<HWND> {
    static CLASS: Once = Once::new();
    CLASS.call_once(|| {
        let module = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(shadow_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE(module.0),
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: Default::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("RogControlShadow"),
        };
        let _ = RegisterClassW(&class);
    });
    let Ok(shadow) = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
        w!("RogControlShadow"),
        w!(""),
        windows::Win32::UI::WindowsAndMessaging::WS_POPUP
            | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE,
        0,
        0,
        1,
        1,
        HWND::default(),
        windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
        HINSTANCE::default(),
        None,
    ) else {
        return None;
    };
    Some(shadow)
}

pub unsafe fn destroy_shadow(shadow: HWND) {
    let _ = DestroyWindow(shadow);
}

unsafe fn build_shadow_bitmap(w: i32, h: i32) -> Option<(HDC, HBITMAP, HGDIOBJ)> {
    let sw = w + SHADOW_MARGIN * 2;
    let sh = h + SHADOW_MARGIN * 2 + SHADOW_OFFSET;
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(screen);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sw,
            biHeight: -sh,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default(); 1],
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0).ok()?;
    let old = SelectObject(mem, HGDIOBJ(bitmap.0));
    let pixels = bits as *mut u32;
    let left = SHADOW_MARGIN as f32;
    let top = SHADOW_MARGIN as f32;
    let right = (SHADOW_MARGIN + w) as f32;
    let bottom = (SHADOW_MARGIN + h) as f32;
    for y in 0..sh {
        for x in 0..sw {
            let fx = x as f32;
            let fy = y as f32;
            let dx = (left - fx).max(fx - right).max(0.0);
            let dy = (top - fy).max(fy - bottom).max(0.0);
            let distance = (dx * dx + dy * dy).sqrt();
            let alpha = if distance <= 0.0 {
                0
            } else {
                let falloff = (-(distance * distance) / (2.0 * SHADOW_SIGMA * SHADOW_SIGMA)).exp();
                (SHADOW_ALPHA * falloff * 255.0).round() as u32
            };
            *pixels.add((y * sw + x) as usize) = alpha << 24;
        }
    }
    ReleaseDC(None, screen);
    Some((mem, bitmap, old))
}

/// 按挂件当前矩形重建/移动阴影。只有尺寸变化时才重新生成位图。
pub unsafe fn update_shadow(shadow: HWND, widget: HWND, last_size: &mut Option<(i32, i32)>) {
    let Some(rect) = get_window_rect(widget) else {
        return;
    };
    let w = rect.width();
    let h = rect.height();
    if *last_size != Some((w, h)) {
        if let Some((mem, bitmap, old)) = build_shadow_bitmap(w, h) {
            let size = SIZE {
                cx: w + SHADOW_MARGIN * 2,
                cy: h + SHADOW_MARGIN * 2 + SHADOW_OFFSET,
            };
            let source = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let screen = GetDC(None);
            let result = UpdateLayeredWindow(
                shadow,
                screen,
                None,
                Some(&size as *const SIZE),
                mem,
                Some(&source as *const POINT),
                COLORREF(0),
                Some(&blend as *const _),
                ULW_ALPHA,
            );
            crate::store::append_log(
                "lifecycle.log",
                &format!("shadow update {result:?} size={w}x{h}"),
            );
            ReleaseDC(None, screen);
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem);
            *last_size = Some((w, h));
        }
    }
    let _ = SetWindowPos(
        shadow,
        windows::Win32::UI::WindowsAndMessaging::HWND_TOP,
        rect.left - SHADOW_MARGIN,
        rect.top - SHADOW_MARGIN + SHADOW_OFFSET,
        w + SHADOW_MARGIN * 2,
        h + SHADOW_MARGIN * 2 + SHADOW_OFFSET,
        SWP_NOACTIVATE,
    );
}

// ---- 第二实例“显示主界面”事件 ----

pub const SHOW_EVENT_NAME: &str = "Local\\RogControl_ShowMain";

/// 创建/打开命名事件；第二个实例用它唤醒第一个实例显示主界面。
pub unsafe fn create_show_event(name: &str) -> Option<windows::Win32::Foundation::HANDLE> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    CreateEventW(None, BOOL(0), BOOL(0), PCWSTR(wide.as_ptr())).ok()
}

pub unsafe fn signal_show_event(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, BOOL(0), PCWSTR(wide.as_ptr())) else {
        return false;
    };
    let ok = SetEvent(event).is_ok();
    let _ = CloseHandle(event);
    ok
}

/// 后台等待“显示主界面”信号；收到后置位标志并唤醒 eframe。
pub fn spawn_show_listener(name: &str, flag: Arc<AtomicBool>, ctx: egui::Context) {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    std::thread::spawn(move || {
        let Ok(event) = (unsafe { CreateEventW(None, BOOL(0), BOOL(0), PCWSTR(wide.as_ptr())) })
        else {
            return;
        };
        loop {
            let result = unsafe { WaitForSingleObject(event, INFINITE) };
            if result.0 != 0 {
                break;
            }
            flag.store(true, Ordering::SeqCst);
            ctx.request_repaint();
        }
        let _ = unsafe { CloseHandle(event) };
    });
}
