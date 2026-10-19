//! Windows DDC/CI 底层：显示器枚举、能力字符串、VCP 读写、亮度控制。
//!
//! 访问链路与原版 DisplayWidgetCenter 一致：
//! `EnumDisplayMonitors` → `GetPhysicalMonitorsFromHMONITOR` →
//! `GetVCPFeatureAndVCPFeatureReply` / `SetVCPFeature` / `GetCapabilitiesStringLength`。
//! 所有 DDC 事务经过同一个全局锁，并保证相邻事务间隔 ≥ 32 ms，
//! 避免 ASUS 面板丢包（原版 VCPAPI.Set_Time 为 20 ms）。

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::core::{s, w, PCWSTR};

/// PHYSICAL_MONITOR 在 x64 上的完整大小：HANDLE(8) + WCHAR[128](256)。
const PHYSICAL_MONITOR_SIZE: usize = 264;
/// 原版每次 VCP 事务前等待 20 ms；这里用更保守的 32 ms。
const PACE: Duration = Duration::from_millis(32);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MonitorRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MonitorInfoExW {
    cb_size: u32,
    rc_monitor: MonitorRect,
    rc_work: MonitorRect,
    flags: u32,
    sz_device: [u16; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PhysicalMonitor {
    handle: isize,
    description: [u16; 128],
}

impl PhysicalMonitor {
    fn empty() -> PhysicalMonitor {
        PhysicalMonitor {
            handle: 0,
            description: [0; 128],
        }
    }

    fn description_string(&self) -> String {
        let end = self
            .description
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(self.description.len());
        String::from_utf16_lossy(&self.description[..end])
            .trim()
            .to_string()
    }
}

const MONITORINFOF_PRIMARY: u32 = 1;

// ---- 动态解析的入口点 ----
type GetNumFn = unsafe extern "system" fn(isize, *mut u32) -> i32;
type GetPhysFn = unsafe extern "system" fn(isize, u32, *mut PhysicalMonitor) -> i32;
type DestroyFn = unsafe extern "system" fn(u32, *mut PhysicalMonitor) -> i32;
type GetVcpFn = unsafe extern "system" fn(isize, u8, *mut u32, *mut u32, *mut u32) -> i32;
type SetVcpFn = unsafe extern "system" fn(isize, u8, u32) -> i32;
type GetCapsLenFn = unsafe extern "system" fn(isize, *mut u32) -> i32;
type GetCapsFn = unsafe extern "system" fn(isize, *mut u8, u32) -> i32;
type GetBrightFn = unsafe extern "system" fn(isize, *mut u32, *mut u32, *mut u32) -> i32;
type SetBrightFn = unsafe extern "system" fn(isize, u32) -> i32;
type EnumMonitorsFn = unsafe extern "system" fn(isize, isize, MonitorEnumProc, isize) -> i32;
type MonitorEnumProc = unsafe extern "system" fn(isize, isize, *mut MonitorRect, isize) -> i32;
type GetMonitorInfoFn = unsafe extern "system" fn(isize, *mut MonitorInfoExW) -> i32;

unsafe impl Send for DdcLib {}
unsafe impl Sync for DdcLib {}

pub struct DdcLib {
    get_num: GetNumFn,
    get_phys: GetPhysFn,
    destroy: DestroyFn,
    get_vcp: GetVcpFn,
    set_vcp: SetVcpFn,
    get_caps_len: GetCapsLenFn,
    get_caps: GetCapsFn,
    get_brightness: GetBrightFn,
    set_brightness: SetBrightFn,
    enum_monitors: EnumMonitorsFn,
    get_monitor_info: GetMonitorInfoFn,
    _module: HMODULE,
}

unsafe fn resolve<T>(module: HMODULE, name: windows::core::PCSTR) -> Option<T> {
    // SAFETY: 目标类型与导出签名一致；FARPROC 被转换为函数指针。
    let proc = unsafe { GetProcAddress(module, name) }?;
    let addr = proc as usize;
    Some(std::mem::transmute_copy::<usize, T>(&addr))
}

impl DdcLib {
    fn load() -> Option<DdcLib> {
        let system32 = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());

        // user32 已经加载；EnumDisplayMonitors / GetMonitorInfoW 从这里取。
        let user32 = unsafe { GetModuleHandleW(w!("user32.dll")) }.ok()?;
        let enum_monitors =
            unsafe { resolve::<EnumMonitorsFn>(user32, s!("EnumDisplayMonitors")) }?;
        let get_monitor_info =
            unsafe { resolve::<GetMonitorInfoFn>(user32, s!("GetMonitorInfoW")) }?;

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
            let resolved = unsafe {
                (
                    resolve::<GetNumFn>(module, s!("GetNumberOfPhysicalMonitorsFromHMONITOR")),
                    resolve::<GetPhysFn>(module, s!("GetPhysicalMonitorsFromHMONITOR")),
                    resolve::<DestroyFn>(module, s!("DestroyPhysicalMonitors")),
                    resolve::<GetVcpFn>(module, s!("GetVCPFeatureAndVCPFeatureReply")),
                    resolve::<SetVcpFn>(module, s!("SetVCPFeature")),
                    resolve::<GetCapsLenFn>(module, s!("GetCapabilitiesStringLength")),
                    resolve::<GetCapsFn>(module, s!("CapabilitiesRequestAndCapabilitiesReply")),
                    resolve::<GetBrightFn>(module, s!("GetMonitorBrightness")),
                    resolve::<SetBrightFn>(module, s!("SetMonitorBrightness")),
                )
            };
            let (
                Some(get_num),
                Some(get_phys),
                Some(destroy),
                Some(get_vcp),
                Some(set_vcp),
                Some(get_caps_len),
                Some(get_caps),
                Some(get_brightness),
                Some(set_brightness),
            ) = resolved
            else {
                continue;
            };
            return Some(DdcLib {
                get_num,
                get_phys,
                destroy,
                get_vcp,
                set_vcp,
                get_caps_len,
                get_caps,
                get_brightness,
                set_brightness,
                enum_monitors,
                get_monitor_info,
                _module: module,
            });
        }
        None
    }
}

static LIB: OnceLock<Option<DdcLib>> = OnceLock::new();

pub fn lib() -> Option<&'static DdcLib> {
    LIB.get_or_init(DdcLib::load).as_ref()
}

// ---------------------------------------------------------------------------
// 全局节流锁
// ---------------------------------------------------------------------------
static DDC_LOCK: Mutex<Option<Instant>> = Mutex::new(None);

pub struct DdcGuard {
    _guard: MutexGuard<'static, Option<Instant>>,
}

/// 取得全局 DDC 事务锁，并保证与上一次事务间隔 ≥ 32 ms。
/// 锁在整个 Win32 调用期间持有，因此所有模块的 DDC 访问天然串行。
pub fn ddc_guard() -> DdcGuard {
    let mut guard = DDC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(last) = *guard {
        let elapsed = last.elapsed();
        if elapsed < PACE {
            std::thread::sleep(PACE - elapsed);
        }
    }
    *guard = Some(Instant::now());
    DdcGuard { _guard: guard }
}

// ---------------------------------------------------------------------------
// 数据
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VcpValue {
    pub code: u8,
    pub current: u32,
    pub maximum: u32,
    pub vcp_type: u32,
}

/// 一台物理显示器。句柄必须成对释放（Drop 会调用 DestroyPhysicalMonitors）。
pub struct MonitorHandle {
    pub hmonitor: isize,
    pub physical: isize,
    pub description: String,
    pub device_name: String,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
    pub index: usize,
    closed: bool,
}

unsafe impl Send for MonitorHandle {}

impl MonitorHandle {
    pub fn resolution(&self) -> String {
        if self.width > 0 && self.height > 0 {
            format!("{} × {}", self.width, self.height)
        } else {
            "—".into()
        }
    }

    pub fn device_short(&self) -> String {
        self.device_name
            .rsplit('\\')
            .next()
            .unwrap_or(&self.device_name)
            .to_string()
    }

    /// 读取能力字符串（重试若干次，首次失败后间隔 60 ms）。
    pub fn query_capabilities(&self, retries: u32) -> Option<String> {
        let lib = lib()?;
        let mut length = 0u32;
        let mut ok = false;
        for attempt in 0..retries.max(1) {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(60));
            }
            let _guard = ddc_guard();
            ok = (unsafe { (lib.get_caps_len)(self.physical, &mut length) }) != 0;
            if ok && length > 0 {
                break;
            }
        }
        if !ok || length == 0 {
            return None;
        }
        let mut buffer = vec![0u8; length as usize + 1];
        let read_ok = {
            let _guard = ddc_guard();
            (unsafe { (lib.get_caps)(self.physical, buffer.as_mut_ptr(), length) }) != 0
        };
        if !read_ok {
            return None;
        }
        let end = buffer
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(buffer.len());
        Some(String::from_utf8_lossy(&buffer[..end]).trim().to_string())
    }

    pub fn get_vcp(&self, code: u8, retries: u32) -> Option<VcpValue> {
        let lib = lib()?;
        let mut vcp_type = 0u32;
        let mut current = 0u32;
        let mut maximum = 0u32;
        for attempt in 0..retries.max(1) {
            let ok = {
                let _guard = ddc_guard();
                (unsafe {
                    (lib.get_vcp)(
                        self.physical,
                        code,
                        &mut vcp_type,
                        &mut current,
                        &mut maximum,
                    )
                }) != 0
            };
            if ok {
                return Some(VcpValue {
                    code,
                    current,
                    maximum,
                    vcp_type,
                });
            }
            if attempt + 1 < retries.max(1) {
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        None
    }

    pub fn set_vcp(&self, code: u8, value: u32, retries: u32) -> bool {
        let Some(lib) = lib() else { return false };
        for attempt in 0..retries.max(1) {
            let ok = {
                let _guard = ddc_guard();
                (unsafe { (lib.set_vcp)(self.physical, code, value) }) != 0
            };
            if ok {
                return true;
            }
            if attempt + 1 < retries.max(1) {
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        false
    }

    /// GetMonitorBrightness：返回 (min, current, max)。
    pub fn get_brightness(&self) -> Option<(u32, u32, u32)> {
        let lib = lib()?;
        let mut min = 0u32;
        let mut current = 0u32;
        let mut max = 0u32;
        let ok = {
            let _guard = ddc_guard();
            (unsafe { (lib.get_brightness)(self.physical, &mut min, &mut current, &mut max) }) != 0
        };
        ok.then_some((min, current, max))
    }

    pub fn set_brightness(&self, value: u32) -> bool {
        let Some(lib) = lib() else { return false };
        let _guard = ddc_guard();
        (unsafe { (lib.set_brightness)(self.physical, value) }) != 0
    }

    pub fn close(&mut self) {
        if self.closed || self.physical == 0 {
            return;
        }
        self.closed = true;
        if let Some(lib) = lib() {
            let mut slot = PhysicalMonitor::empty();
            slot.handle = self.physical;
            let _guard = ddc_guard();
            unsafe { (lib.destroy)(1, &mut slot) };
        }
    }
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------
pub fn enumerate_monitors() -> Vec<MonitorHandle> {
    let Some(lib) = lib() else { return Vec::new() };
    let mut hmons: Vec<isize> = Vec::new();

    unsafe extern "system" fn callback(
        hmon: isize,
        _hdc: isize,
        _rect: *mut MonitorRect,
        data: isize,
    ) -> i32 {
        let list = unsafe { &mut *(data as *mut Vec<isize>) };
        list.push(hmon);
        1
    }

    let ok = unsafe { (lib.enum_monitors)(0, 0, callback, &mut hmons as *mut _ as isize) };
    if ok == 0 {
        return Vec::new();
    }

    let mut monitors = Vec::new();
    for hmon in hmons {
        let mut info = MonitorInfoExW {
            cb_size: std::mem::size_of::<MonitorInfoExW>() as u32,
            rc_monitor: MonitorRect::default(),
            rc_work: MonitorRect::default(),
            flags: 0,
            sz_device: [0; 32],
        };
        let has_info = unsafe { (lib.get_monitor_info)(hmon, &mut info) } != 0;
        let device = if has_info {
            let end = info
                .sz_device
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.sz_device.len());
            String::from_utf16_lossy(&info.sz_device[..end])
        } else {
            String::new()
        };
        let width = if has_info {
            (info.rc_monitor.right - info.rc_monitor.left).max(0)
        } else {
            0
        };
        let height = if has_info {
            (info.rc_monitor.bottom - info.rc_monitor.top).max(0)
        } else {
            0
        };
        let primary = has_info && (info.flags & MONITORINFOF_PRIMARY) != 0;

        let mut count = 0u32;
        if unsafe { (lib.get_num)(hmon, &mut count) } == 0 || count == 0 {
            continue;
        }
        let mut buffer = vec![PhysicalMonitor::empty(); count as usize];
        if unsafe { (lib.get_phys)(hmon, count, buffer.as_mut_ptr()) } == 0 {
            continue;
        }
        for entry in buffer.iter() {
            if entry.handle == 0 {
                continue;
            }
            monitors.push(MonitorHandle {
                hmonitor: hmon,
                physical: entry.handle,
                description: entry.description_string(),
                device_name: device.clone(),
                width,
                height,
                primary,
                index: monitors.len(),
                closed: false,
            });
        }
    }
    monitors
}

/// 主显示器的第一台物理显示器。
pub fn primary_handle() -> Option<MonitorHandle> {
    let mut monitors = enumerate_monitors();
    if monitors.is_empty() {
        return None;
    }
    let index = monitors.iter().position(|m| m.primary).unwrap_or(0);
    let handle = monitors.swap_remove(index);
    // 其余句柄随 Vec 析构释放。
    Some(handle)
}

/// 在主显示器上写一个 VCP（供双模切换使用）。
pub fn set_vcp_primary(code: u8, value: u32) -> bool {
    let Some(handle) = primary_handle() else {
        return false;
    };
    handle.set_vcp(code, value, 2)
}

// ---------------------------------------------------------------------------
// 挂件亮度控制器（沿用原有 API，内部改用 MonitorHandle）
// ---------------------------------------------------------------------------
pub struct DdcController {
    handle: Option<MonitorHandle>,
    min: u32,
    max: u32,
    pub supported: bool,
    pub reason: String,
}

impl DdcController {
    pub fn new() -> DdcController {
        let mut c = DdcController {
            handle: None,
            min: 0,
            max: 0,
            supported: false,
            reason: String::new(),
        };
        c.init();
        c
    }

    fn init(&mut self) {
        if lib().is_none() {
            self.reason =
                "DDC/CI support libraries missing (monitorconfigapi.dll / dxva2.dll)".into();
            return;
        }
        match self.open() {
            Ok(()) => {
                self.supported = true;
                self.reason.clear();
            }
            Err(reason) => {
                self.reason = reason;
                self.supported = false;
            }
        }
    }

    fn open(&mut self) -> Result<(), String> {
        self.handle = None;
        let handle = primary_handle().ok_or_else(|| "no physical monitor found".to_string())?;
        let (min, _current, max) = handle.get_brightness().ok_or_else(|| {
            "failed to read brightness (display may not support DDC/CI)".to_string()
        })?;
        self.min = min;
        self.max = max;
        self.handle = Some(handle);
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), String> {
        self.supported = false;
        let result = self.open();
        if result.is_ok() {
            self.supported = true;
        }
        result
    }

    pub fn get_brightness_percent(&mut self) -> Result<i32, String> {
        if !self.supported {
            return Err(if self.reason.is_empty() {
                "display does not support DDC/CI brightness".into()
            } else {
                self.reason.clone()
            });
        }
        let read = self
            .handle
            .as_ref()
            .and_then(|h| h.get_brightness())
            .map(|(_, current, _)| current);
        if let Some(current) = read {
            return Ok(range_to_percent(current, self.min, self.max));
        }
        self.reopen()?;
        let current = self
            .handle
            .as_ref()
            .and_then(|h| h.get_brightness())
            .map(|(_, current, _)| current)
            .ok_or_else(|| "cannot read display brightness".to_string())?;
        Ok(range_to_percent(current, self.min, self.max))
    }

    pub fn set_brightness_percent(&mut self, percent: i32) -> Result<(), String> {
        if !self.supported {
            return Err(if self.reason.is_empty() {
                "display does not support DDC/CI brightness".into()
            } else {
                self.reason.clone()
            });
        }
        let percent = percent.clamp(0, 100);
        let value = (self.min as f64
            + (self.max as f64 - self.min as f64) * percent as f64 / 100.0)
            .round() as u32;
        if self
            .handle
            .as_ref()
            .is_some_and(|h| h.set_brightness(value))
        {
            return Ok(());
        }
        self.reopen()?;
        if self
            .handle
            .as_ref()
            .is_some_and(|h| h.set_brightness(value))
        {
            return Ok(());
        }
        Err("failed to set brightness".into())
    }
}

fn range_to_percent(value: u32, min: u32, max: u32) -> i32 {
    if max <= min {
        return 100;
    }
    let percent = (value as f64 - min as f64) * 100.0 / (max as f64 - min as f64);
    (percent.round() as i32).clamp(0, 100)
}

#[allow(dead_code)]
fn _layout_check() -> [usize; 2] {
    // 结构布局回归检查：x64 上 PHYSICAL_MONITOR 必须正好 264 字节。
    [PHYSICAL_MONITOR_SIZE, std::mem::size_of::<PhysicalMonitor>()]
}
