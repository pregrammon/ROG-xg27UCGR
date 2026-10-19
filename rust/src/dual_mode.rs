//! 通过显示器的 DDC/CI 厂商 VCP 通道，在 4K @ ~162 Hz 与 FHD @ 485 Hz
//! 之间切换 ROG 双模显示器（ROG Strix XG27UCGR "绝神二代"）。
//! 取值与逆向得到的 C# 版一致：
//!   - 写 VCP 0xFD = 0x128 (296) -> 1K 1920x1080（FrameRateBoost 开，VRR 关）
//!   - 写 VCP 0xFD = 0x001 (1)   -> 4K 3840x2160（VRR 开，FrameRateBoost 关）
//! 当前状态检测使用 Windows 分辨率，绝不读取 VCP
//! （这块屏的 VCP 读取经常失败并返回垃圾输出参数）。

use windows::core::PCWSTR;

pub const VCP_DUAL_MODE: u8 = 0xFD;
pub const MODE_1K_VALUE: u32 = 0x128;
pub const MODE_4K_VALUE: u32 = 0x001;

const WIDTH_4K: u32 = 3840;
const HEIGHT_4K: u32 = 2160;
const WIDTH_1K: u32 = 1920;
const HEIGHT_1K: u32 = 1080;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualMode {
    Unknown,
    Mode4K,
    Mode1K,
}

#[repr(C)]
struct DevModeW {
    dm_device_name: [u16; 32],
    dm_spec_version: u16,
    dm_driver_version: u16,
    dm_size: u16,
    dm_driver_extra: u16,
    dm_fields: u32,
    dm_position_x: i32,
    dm_position_y: i32,
    dm_display_orientation: u32,
    dm_display_fixed_output: u32,
    dm_color: i16,
    dm_duplex: i16,
    dm_y_resolution: i16,
    dm_tt_option: i16,
    dm_collate: i16,
    dm_form_name: [u16; 32],
    dm_log_pixels: u16,
    dm_bits_per_pel: u32,
    dm_pels_width: u32,
    dm_pels_height: u32,
    dm_display_flags: u32,
    dm_display_frequency: u32,
    dm_icm_method: u32,
    dm_icm_intent: u32,
    dm_media_type: u32,
    dm_dither_type: u32,
    dm_reserved1: u32,
    dm_reserved2: u32,
    dm_panning_width: u32,
    dm_panning_height: u32,
}

unsafe extern "system" {
    #[link_name = "EnumDisplaySettingsW"]
    fn enum_display_settings_w(
        lpsz_device_name: PCWSTR,
        i_mode_num: i32,
        lp_dev_mode: *mut DevModeW,
    ) -> i32;
}

const ENUM_CURRENT_SETTINGS: i32 = -1;

/// 通过读取当前的 Windows 分辨率，
/// 报告显示器当前处于哪种面板状态。
pub fn get_current_mode() -> DualMode {
    // SAFETY: 局部的、已清零的 DevModeW；该缓冲区在整个调用期间有效。
    let mut mode: DevModeW = unsafe { std::mem::zeroed() };
    mode.dm_size = std::mem::size_of::<DevModeW>() as u16;
    let ok = unsafe { enum_display_settings_w(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) };
    if ok == 0 {
        return DualMode::Unknown;
    }
    if mode.dm_pels_width == WIDTH_4K && mode.dm_pels_height == HEIGHT_4K {
        DualMode::Mode4K
    } else if mode.dm_pels_width == WIDTH_1K && mode.dm_pels_height == HEIGHT_1K {
        DualMode::Mode1K
    } else {
        DualMode::Unknown
    }
}

/// 把显示器切换到指定的双模状态。成功时返回空字符串，
/// 失败时返回人类可读的原因。该调用会阻塞
/// （显示器要重新握手链路）；请在 UI 线程之外运行。
pub fn switch_to(target: DualMode) -> Result<(), String> {
    let value = match target {
        DualMode::Mode1K => MODE_1K_VALUE,
        DualMode::Mode4K => MODE_4K_VALUE,
        DualMode::Unknown => return Err("invalid switch target".into()),
    };

    if !crate::ddc::set_vcp_primary(VCP_DUAL_MODE, value) {
        return Err("display refused the dual-mode command (DDC/CI disabled?)".into());
    }

    // 显示器会重新握手链路（短暂黑屏），随后系统会跟着读取新的 EDID；
    // 这里等几秒钟，以便后续的状态读取
    // 能看到已经稳定下来的模式。
    std::thread::sleep(std::time::Duration::from_secs(3));
    Ok(())
}
