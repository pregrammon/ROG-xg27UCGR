//! 窗口状态持久化（%APPDATA%\RogControl\settings.json），与 C# 版保持一致：
//! 位置 + 尺寸，外加透明/模糊偏好。
//!
//! 同一个矩形还会额外按主屏分辨率存一份比例值。
//! 显示器在 4K 与 1K 之间切换时分辨率会改变
//! （因此同一个物理矩形对应的视觉位置和大小也会改变），
//! 所以重放的是这些比例值：控件保持相同的视觉位置，
//! 宽高随分辨率等比缩放（2160 -> 1080 正好减半）。

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Config {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub transparent: bool,
    pub blur: bool,
    /// 标题栏固定按钮（锁定位置）的状态。持久化保存，
    /// 使控件在重启或显示器模式切换之后仍然保持固定。
    #[serde(default)]
    pub pinned: bool,
    /// 位置/尺寸占主屏分辨率的比例（0.0..1.0）。
    /// 在此字段出现之前写入的配置文件中该值全为 0，
    /// 这时会回退到上面绝对的 x/y/w/h 坐标。
    #[serde(default)]
    pub fx: f32,
    #[serde(default)]
    pub fy: f32,
    #[serde(default)]
    pub fw: f32,
    #[serde(default)]
    pub fh: f32,
    /// 记录该矩形时的原生 DPI（egui 的每点像素数）除以当时的屏幕宽度。
    /// 这里已包含 Windows 缩放：显示器使用“推荐”缩放，
    /// 所以 4K 报 150%，而 1K 报 100%。
    /// 乘以当前屏幕宽度即可还原出控件应有的精确 ppp，
    /// 从而让它的*逻辑*尺寸保持恒定，
    /// 使内容在两种模式下看起来完全一致，而不会被挤压。
    /// 0.0 表示“无数据”（较旧的配置文件）。
    #[serde(default)]
    pub ppp_per_px: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            x: 0,
            y: 0,
            w: 340,
            h: 232,
            transparent: true,
            blur: false,
            pinned: false,
            fx: 0.0,
            fy: 0.0,
            fw: 0.0,
            fh: 0.0,
            ppp_per_px: 0.0,
        }
    }
}

fn settings_dir() -> std::path::PathBuf {
    // 与主程序共用数据目录，ROG_DATA_DIR 便携模式同样生效。
    crate::store::data_dir()
}

fn settings_path() -> std::path::PathBuf {
    settings_dir().join("settings.json")
}

pub fn load() -> Config {
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return Config::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(config: &Config) {
    let dir = settings_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string(config) {
        let _ = std::fs::write(dir.join("settings.json"), text);
    }
}

/// 把保存的尺寸限制在合理范围内并返回。
/// 这些边界以物理像素为单位，而且故意设得很宽松：
/// 控件尺寸会随屏幕分辨率缩放，
/// 因此低分辨率模式下合法地会产生比 4K 小得多的矩形。
pub fn clamp_size(w: i32, h: i32) -> (i32, i32) {
    (w.clamp(100, 8000), h.clamp(60, 8000))
}

/// 在 `ref_w` x `ref_h` 的屏幕上，窗口应当放置到的物理矩形。
/// 存有比例值时按比例计算（使控件落在相同的视觉位置并随分辨率缩放），
/// 否则使用绝对坐标值。
pub fn resolve_rect(cfg: &Config, ref_w: i32, ref_h: i32) -> (i32, i32, i32, i32) {
    let has_frac = cfg.fw > 0.0 && cfg.fh > 0.0 && ref_w > 0 && ref_h > 0;
    if !has_frac {
        let (w, h) = clamp_size(cfg.w, cfg.h);
        return (cfg.x, cfg.y, w, h);
    }
    let (w, h) = clamp_size(
        (cfg.fw * ref_w as f32).round() as i32,
        (cfg.fh * ref_h as f32).round() as i32,
    );
    let x = (cfg.fx * ref_w as f32).round() as i32;
    let y = (cfg.fy * ref_h as f32).round() as i32;
    (x, y, w, h)
}

/// 同时以绝对坐标和屏幕分辨率比例两种形式记录窗口矩形。
pub fn capture_rect(cfg: &mut Config, x: i32, y: i32, w: i32, h: i32, ref_w: i32, ref_h: i32) {
    cfg.x = x;
    cfg.y = y;
    cfg.w = w;
    cfg.h = h;
    if ref_w > 0 && ref_h > 0 {
        cfg.fx = x as f32 / ref_w as f32;
        cfg.fy = y as f32 / ref_h as f32;
        cfg.fw = w as f32 / ref_w as f32;
        cfg.fh = h as f32 / ref_h as f32;
    }
}
