//! 通过主显示器的物理显示器进行 DDC/CI 亮度控制。
//! 物理显示器 API（GetPhysicalMonitorsFromHMONITOR /
//! GetMonitorBrightness / SetMonitorBrightness / GetVCPFeatureAndVCPFeatureReply
//! / DestroyPhysicalMonitors）在 Windows 8+ 上位于 monitorconfigapi.dll，
//! 在更早的系统上位于 dxva2.dll；两者均在运行时探测。
//!
//! 原生 PHYSICAL_MONITOR 结构在 x64 上为 264 字节
//! （HANDLE 8 字节 + WCHAR[128] 256 字节）；我们只访问第一个
//! 字段（句柄），从不封送整个结构。

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{s, PCWSTR};

use crate::win32;

type GetNumFn = unsafe extern "system" fn(usize, *mut u32) -> i32;
type GetPhysFn = unsafe extern "system" fn(usize, u32, *mut c_void) -> i32;
type GetBrightFn = unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32, *mut u32) -> i32;
type SetBrightFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;
type GetVcpFn = unsafe extern "system" fn(*mut c_void, u8, *mut u32, *mut u32) -> i32;
type SetVcpFn = unsafe extern "system" fn(*mut c_void, u8, u32) -> i32;
type DestroyFn = unsafe extern "system" fn(u32, *mut c_void) -> i32;

/// 原生 PHYSICAL_MONITOR 布局（x64）：HANDLE(8) + WCHAR[128](256) = 264。
pub const PHYSICAL_MONITOR_SIZE: usize = 264;

#[repr(C)]
struct PhysicalMonitorSlot {
    handle: usize,
    _name: [u16; 128],
}

unsafe impl Send for DdcLib {}
unsafe impl Sync for DdcLib {}

/// 已加载的 DDC/CI 库（monitorconfigapi.dll 或 dxva2.dll），其入口
/// 点只解析一次。
pub struct DdcLib {
    get_num: GetNumFn,
    get_phys: GetPhysFn,
    get_brightness: GetBrightFn,
    set_brightness: SetBrightFn,
    get_vcp: GetVcpFn,
    set_vcp: SetVcpFn,
    destroy: DestroyFn,
    _module: HMODULE,
}

impl DdcLib {
    fn load() -> Option<DdcLib> {
        let system32 = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        for dll in ["monitorconfigapi.dll", "dxva2.dll"] {
            let path = format!("{system32}\\System32\\{dll}");
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: 局部宽字符缓冲区在调用期间有效。
            let Ok(module) = (unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }) else {
                continue;
            };
            if module.is_invalid() {
                continue;
            }
            // 每个符号都必须存在；否则丢弃该模块并尝试下一个。
            let (Some(get_num), Some(get_phys), Some(get_brightness), Some(set_brightness),
                Some(get_vcp), Some(set_vcp), Some(destroy)) = (
                unsafe { resolve::<GetNumFn>(module, s!("GetNumberOfPhysicalMonitorsFromHMONITOR")) },
                unsafe { resolve::<GetPhysFn>(module, s!("GetPhysicalMonitorsFromHMONITOR")) },
                unsafe { resolve::<GetBrightFn>(module, s!("GetMonitorBrightness")) },
                unsafe { resolve::<SetBrightFn>(module, s!("SetMonitorBrightness")) },
                unsafe { resolve::<GetVcpFn>(module, s!("GetVCPFeatureAndVCPFeatureReply")) },
                unsafe { resolve::<SetVcpFn>(module, s!("SetVCPFeature")) },
                unsafe { resolve::<DestroyFn>(module, s!("DestroyPhysicalMonitors")) },
            ) else {
                continue;
            };
            return Some(DdcLib {
                get_num,
                get_phys,
                get_brightness,
                set_brightness,
                get_vcp,
                set_vcp,
                destroy,
                _module: module,
            });
        }
        None
    }
}

unsafe fn resolve<T>(module: HMODULE, name: windows::core::PCSTR) -> Option<T> {
    // SAFETY: 调用方保证目标类型与导出的签名匹配；
    // FARPROC（指针大小）被转换为匹配的 fn 指针类型。
    let proc = unsafe { GetProcAddress(module, name) }?;
    let addr = proc as usize;
    Some(std::mem::transmute_copy::<usize, T>(&addr))
}

static LIB: OnceLock<Option<DdcLib>> = OnceLock::new();

fn lib() -> Option<&'static DdcLib> {
    LIB.get_or_init(DdcLib::load).as_ref()
}

/// 通过 DDC/CI 读写主显示器上第一个物理显示器的亮度。
/// 与 C# 的 MonitorBrightnessController 一致，包括其过期句柄
/// 恢复逻辑（双模面板切换会重新枚举显示器，并使已缓存的
/// 句柄失效）。
pub struct DdcController {
    phys: usize,
    min: u32,
    max: u32,
    pub supported: bool,
    pub reason: String,
}

impl DdcController {
    pub fn new() -> DdcController {
        let mut c = DdcController {
            phys: 0,
            min: 0,
            max: 0,
            supported: false,
            reason: String::new(),
        };
        c.init();
        c
    }

    fn init(&mut self) {
        let Some(lib) = lib() else {
            self.reason = "DDC/CI support libraries missing (monitorconfigapi.dll / dxva2.dll)".into();
            return;
        };
        if !self.acquire_handle(lib) {
            return;
        }
        let mut min = 0u32;
        let mut cur = 0u32;
        let mut max = 0u32;
        // SAFETY: phys 是有效的物理显示器句柄。
        if unsafe { (lib.get_brightness)(self.phys as *mut c_void, &mut min, &mut cur, &mut max) } == 0 {
            self.release_handle();
            self.reason = "failed to read brightness (display may not support DDC/CI)".into();
            return;
        }
        self.min = min;
        self.max = max;
        self.supported = true;
        self.reason.clear();
    }

    fn acquire_handle(&mut self, lib: &DdcLib) -> bool {
        let Some(hmon) = (unsafe { win32::primary_monitor_from_desktop() }) else {
            self.reason = "could not get the primary monitor handle".into();
            return false;
        };
        let mut count = 0u32;
        // SAFETY: hmon 是来自 MonitorFromWindow 的有效 HMONITOR 值。
        let num_rc = unsafe { (lib.get_num)(hmon.0 as usize, &mut count) };
        if num_rc == 0 || count == 0 {
            self.reason = format!("no physical monitor found: num_rc={num_rc} count={count}");
            return false;
        }
        let bytes = PHYSICAL_MONITOR_SIZE
            .checked_mul(count as usize)
            .unwrap_or(0);
        let mut buffer = vec![0u8; bytes];
        // SAFETY: buffer 有容纳 count 个 PHYSICAL_MONITOR 条目的空间。
        let phys_rc = unsafe { (lib.get_phys)(hmon.0 as usize, count, buffer.as_mut_ptr() as *mut c_void) };
        if phys_rc == 0 {
            self.reason = format!("GetPhysicalMonitors failed: phys_rc={phys_rc}");
            return false;
        }
        let first = unsafe { std::ptr::read::<usize>(buffer.as_ptr() as *const usize) };
        // 释放除保留的第一个句柄之外的所有句柄。
        for i in 1..count as usize {
            let handle = unsafe { std::ptr::read::<usize>(buffer.as_ptr().add(i * PHYSICAL_MONITOR_SIZE) as *const usize) };
            if handle != 0 {
                let slot = PhysicalMonitorSlot { handle, _name: [0; 128] };
                // SAFETY: slot 是有效的类 PHYSICAL_MONITOR 缓冲区。
                unsafe { (lib.destroy)(1, &slot as *const PhysicalMonitorSlot as *mut c_void) };
            }
        }
        if first == 0 {
            let dump: Vec<String> = buffer.iter().take(24).map(|b| format!("{b:02x}")).collect();
            self.reason = format!(
                "invalid physical monitor handle: count={count} phys_rc={phys_rc} first={first:#x} hmon={:#x} buf=[{}]",
                hmon.0 as usize,
                dump.join(" ")
            );
            return false;
        }
        self.phys = first;
        true
    }

    fn release_handle(&mut self) {
        if self.phys != 0 {
            if let Some(lib) = lib() {
                let slot = PhysicalMonitorSlot { handle: self.phys, _name: [0; 128] };
                // SAFETY: slot 是有效的类 PHYSICAL_MONITOR 缓冲区。
                unsafe { (lib.destroy)(1, &slot as *const PhysicalMonitorSlot as *mut c_void) };
            }
            self.phys = 0;
        }
    }

    /// 重新打开物理显示器（双模面板切换会使缓存的句柄失效）。
    /// 失败时返回错误字符串。
    fn reopen(&mut self) -> Result<(), String> {
        self.release_handle();
        let Some(lib) = lib() else {
            self.supported = false;
            self.reason = "DDC/CI support libraries missing".into();
            return Err(self.reason.clone());
        };
        if !self.acquire_handle(lib) {
            self.supported = false;
            return Err(self.reason.clone());
        }
        let mut min = 0u32;
        let mut cur = 0u32;
        let mut max = 0u32;
        // SAFETY: 刚获取的句柄。
        if unsafe { (lib.get_brightness)(self.phys as *mut c_void, &mut min, &mut cur, &mut max) } == 0 {
            self.release_handle();
            self.supported = false;
            self.reason = "display disconnected, cannot read brightness".into();
            return Err(self.reason.clone());
        }
        self.min = min;
        self.max = max;
        self.supported = true;
        Ok(())
    }

    fn try_read(&self) -> Option<u32> {
        let lib = lib()?;
        let mut min = 0u32;
        let mut cur = 0u32;
        let mut max = 0u32;
        // SAFETY: phys 有效。
        if unsafe { (lib.get_brightness)(self.phys as *mut c_void, &mut min, &mut cur, &mut max) } != 0 {
            Some(cur)
        } else {
            None
        }
    }

    fn try_write(&self, value: u32) -> bool {
        let Some(lib) = lib() else { return false };
        // SAFETY: phys 有效。
        (unsafe { (lib.set_brightness)(self.phys as *mut c_void, value) }) != 0
    }

    /// 当前亮度百分比，范围 [0, 100]；不支持或无法读取时
    /// 返回带原因的 Err。
    pub fn get_brightness_percent(&mut self) -> Result<i32, String> {
        if !self.supported {
            return Err(if self.reason.is_empty() {
                "display does not support DDC/CI brightness".into()
            } else {
                self.reason.clone()
            });
        }
        if let Some(cur) = self.try_read() {
            return Ok(range_to_percent(cur, self.min, self.max));
        }
        // 句柄过期（双模切换）：重新打开并重试一次。
        self.reopen()?;
        if let Some(cur) = self.try_read() {
            return Ok(range_to_percent(cur, self.min, self.max));
        }
        Err("cannot read display brightness".into())
    }

    /// 以 [0, 100] 范围内的百分比设置亮度。
    pub fn set_brightness_percent(&mut self, percent: i32) -> Result<(), String> {
        if !self.supported {
            return Err(if self.reason.is_empty() {
                "display does not support DDC/CI brightness".into()
            } else {
                self.reason.clone()
            });
        }
        let percent = percent.clamp(0, 100);
        let value = (self.min as f64 + (self.max as f64 - self.min as f64) * percent as f64 / 100.0).round() as u32;
        if self.try_write(value) {
            return Ok(());
        }
        self.reopen()?;
        if self.try_write(value) {
            return Ok(());
        }
        Err("failed to set brightness".into())
    }
}

impl Drop for DdcController {
    fn drop(&mut self) {
        self.release_handle();
    }
}

fn range_to_percent(value: u32, min: u32, max: u32) -> i32 {
    if max <= min {
        return 100;
    }
    let percent = (value as f64 - min as f64) * 100.0 / (max as f64 - min as f64);
    (percent.round() as i32).clamp(0, 100)
}

// ---- 与双模切换共享的 VCP 访问 ----

/// 在主显示器的第一个物理显示器上设置 VCP 功能值
/// （用于双模切换）。无法访问显示器时返回
/// false。
pub fn set_vcp_feature(code: u8, value: u32) -> bool {
    let Some(lib) = lib() else { return false };
    let Some(hmon) = (unsafe { win32::primary_monitor_from_desktop() }) else {
        return false;
    };
    let mut count = 0u32;
    // SAFETY: hmon 有效。
    if unsafe { (lib.get_num)(hmon.0 as usize, &mut count) } == 0 || count == 0 {
        return false;
    }
    let mut buffer = vec![0u8; PHYSICAL_MONITOR_SIZE * count as usize];
    // SAFETY: buffer 的大小可容纳 count 个条目。
    if unsafe { (lib.get_phys)(hmon.0 as usize, count, buffer.as_mut_ptr() as *mut c_void) } == 0 {
        return false;
    }
    let first = unsafe { std::ptr::read::<usize>(buffer.as_ptr() as *const usize) };
    // 释放多余的句柄。
    for i in 1..count as usize {
        let handle = unsafe { std::ptr::read::<usize>(buffer.as_ptr().add(i * PHYSICAL_MONITOR_SIZE) as *const usize) };
        if handle != 0 {
            let slot = PhysicalMonitorSlot { handle, _name: [0; 128] };
            // SAFETY: 有效的 slot。
            unsafe { (lib.destroy)(1, &slot as *const PhysicalMonitorSlot as *mut c_void) };
        }
    }
    if first == 0 {
        return false;
    }
    // SAFETY: first 是有效的物理显示器句柄。
    let ok = unsafe { (lib.set_vcp)(first as *mut c_void, code, value) } != 0;
    let slot = PhysicalMonitorSlot { handle: first, _name: [0; 128] };
    // SAFETY: 有效的 slot。
    unsafe { (lib.destroy)(1, &slot as *const PhysicalMonitorSlot as *mut c_void) };
    ok
}
