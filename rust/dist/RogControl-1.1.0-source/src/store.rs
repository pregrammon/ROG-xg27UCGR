//! 设置、配置文件、能力字符串缓存与开机自启。
//!
//! 数据目录与原 C# 版一致：`%APPDATA%\RogControl`。
//! 配置文件格式与 Python 复刻版兼容（`profiles.json` 的 vcp 键为十进制字符串）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const APP_DIR: &str = "RogControl";

pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn data_dir() -> PathBuf {
    // 便携模式：允许用环境变量把全部数据放到指定目录。
    if let Ok(dir) = std::env::var("ROG_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        format!("{home}\\AppData\\Roaming")
    });
    PathBuf::from(base).join(APP_DIR)
}

fn ensure_dir() -> PathBuf {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn profiles_path() -> PathBuf {
    data_dir().join("profiles.json")
}

pub fn caps_cache_path() -> PathBuf {
    data_dir().join("capabilities.json")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("ui.json")
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(path, text);
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// ---------------------------------------------------------------------------
// 桌面小挂件控件
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct WidgetConfig {
    pub brightness: bool,
    pub contrast: bool,
    pub overdrive: bool,
    pub sharpness: bool,
    pub color_temp: bool,
    pub eye_care: bool,
    pub frame_rate_boost: bool,
    pub scenes: bool,
    /// 要显示的 GameVisual 模式键；None = 全部显示。
    pub scene_modes: Option<Vec<String>>,
}

impl Default for WidgetConfig {
    fn default() -> WidgetConfig {
        WidgetConfig {
            brightness: true,
            contrast: false,
            overdrive: false,
            sharpness: false,
            color_temp: false,
            eye_care: false,
            frame_rate_boost: true,
            scenes: false,
            scene_modes: None,
        }
    }
}

impl WidgetConfig {
    pub fn slider_count(&self) -> usize {
        [
            self.brightness,
            self.contrast,
            self.overdrive,
            self.sharpness,
            self.color_temp,
            self.eye_care,
        ]
        .iter()
        .filter(|on| **on)
        .count()
    }

    pub fn scene_visible(&self, key: &str) -> bool {
        match &self.scene_modes {
            None => true,
            Some(list) => list.iter().any(|item| item == key),
        }
    }

    /// 按用户勾选顺序返回要显示的模式键；None 表示按默认顺序显示全部。
    pub fn ordered_scene_keys(&self, all_keys: &[&'static str]) -> Vec<&'static str> {
        match &self.scene_modes {
            None => all_keys.to_vec(),
            Some(list) => list
                .iter()
                .filter_map(|key| {
                    all_keys
                        .iter()
                        .find(|item| **item == key.as_str())
                        .copied()
                })
                .collect(),
        }
    }

    /// 勾选/取消某个模式；`all_keys` 是当前机型支持的全部模式键（按显示顺序）。
    pub fn set_scene_visible(&mut self, key: &str, on: bool, all_keys: &[&str]) {
        let mut current: Vec<String> = match &self.scene_modes {
            None => all_keys.iter().map(|item| item.to_string()).collect(),
            Some(list) => list.clone(),
        };
        current.retain(|item| item != key);
        if on {
            // 新勾选的放到末尾：挂件里先勾选的排在最前面。
            current.push(key.to_string());
        }
        let ordered: Vec<String> = current
            .into_iter()
            .filter(|item| all_keys.contains(&item.as_str()))
            .collect();
        self.scene_modes = Some(ordered);
    }
}

// ---------------------------------------------------------------------------
// 应用设置
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AppSettings {
    pub language: String,
    pub topmost: bool,
    pub hotkeys: bool,
    pub monitor_key: String,
    pub last_page: String,
    /// 主窗口外框位置（egui 点），None 表示首次打开时居中。
    pub main_pos: Option<[f32; 2]>,
    /// 主窗口内容尺寸（egui 点）。
    pub main_size: Option<[f32; 2]>,
    pub main_maximized: bool,
    /// 桌面小挂件要显示哪些调节项。
    pub widget: WidgetConfig,
}

impl Default for AppSettings {
    fn default() -> AppSettings {
        AppSettings {
            language: "zh".into(),
            topmost: false,
            hotkeys: true,
            monitor_key: String::new(),
            last_page: "overview".into(),
            main_pos: None,
            main_size: Some([1200.0, 760.0]),
            main_maximized: false,
            widget: WidgetConfig::default(),
        }
    }
}

pub fn load_settings() -> AppSettings {
    let mut settings = read_json::<AppSettings>(&settings_path()).unwrap_or_default();
    if settings.language != "zh" && settings.language != "en" {
        settings.language = "zh".into();
    }
    if settings.main_size.is_none() {
        settings.main_size = Some([1200.0, 760.0]);
    }
    settings
}

pub fn save_settings(settings: &AppSettings) {
    ensure_dir();
    write_json(&settings_path(), settings);
}

// ---------------------------------------------------------------------------
// 配置文件
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub model: String,
    pub line: String,
    pub created: String,
    /// 键为十进制 VCP 码字符串，与 Python 版导出的 JSON 完全兼容。
    pub vcp: BTreeMap<String, u32>,
}

impl Profile {
    pub fn value_count(&self) -> usize {
        self.vcp.len()
    }
}

pub fn load_profiles() -> Vec<Profile> {
    read_json::<Vec<Profile>>(&profiles_path()).unwrap_or_default()
}

pub fn save_profiles(profiles: &[Profile]) {
    ensure_dir();
    write_json(&profiles_path(), &profiles.to_vec());
}

pub fn write_profile_file(path: &PathBuf, profile: &Profile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn read_profile_file(path: &PathBuf) -> Result<Profile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let profile: Profile = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if profile.vcp.is_empty() {
        return Err("invalid profile: missing vcp values".into());
    }
    Ok(profile)
}

// ---------------------------------------------------------------------------
// 能力字符串缓存（7 天）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CapsEntry {
    #[serde(default)]
    pub caps: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub time: f64,
}

const CAPS_TTL: f64 = 7.0 * 24.0 * 3600.0;

fn load_caps_cache() -> BTreeMap<String, CapsEntry> {
    read_json::<BTreeMap<String, CapsEntry>>(&caps_cache_path()).unwrap_or_default()
}

/// 读取仍在有效期内的能力字符串。
pub fn cached_caps(key: &str) -> Option<String> {
    let cache = load_caps_cache();
    let entry = cache.get(key)?;
    if now_secs() - entry.time > CAPS_TTL {
        return None;
    }
    if !entry.caps.to_ascii_lowercase().contains("vcp(") {
        return None;
    }
    Some(entry.caps.clone())
}

pub fn save_caps_entry(key: &str, caps: &str, model: &str) {
    ensure_dir();
    let mut cache = load_caps_cache();
    cache.insert(
        key.to_string(),
        CapsEntry {
            caps: caps.to_string(),
            model: model.to_string(),
            time: now_secs(),
        },
    );
    write_json(&caps_cache_path(), &cache);
}

// ---------------------------------------------------------------------------
// 开机自启（HKCU\...\Run）
// ---------------------------------------------------------------------------
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "RogControl";

pub fn is_auto_start_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(RUN_KEY) else {
        return false;
    };
    key.get_value::<String, _>(RUN_VALUE).is_ok()
}

/// 启用/禁用开机自启，返回注册表当前的真实状态。
pub fn set_auto_start_enabled(enable: bool) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if enable {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "rogcontrol.exe".into());
        if let Ok(key) = hkcu.create_subkey(RUN_KEY) {
            let _ = key.0.set_value(RUN_VALUE, &format!("\"{exe}\""));
        }
    } else if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
        let _ = key.delete_value(RUN_VALUE);
    }

    is_auto_start_enabled() == enable
}

/// 用资源管理器打开一个路径（文件或目录）。
pub fn open_path(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer")
        .arg(path)
        .spawn();
}

/// 追加一行诊断日志到数据目录下的指定文件。
pub fn append_log(name: &str, line: &str) {
    use std::io::Write;
    ensure_dir();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir().join(name))
    {
        let _ = writeln!(file, "{line}");
    }
}

/// panic 时把消息与回溯写到 `panic.log`（GUI 子系统没有控制台）。
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        append_log("panic.log", &format!("{info}\n{backtrace}"));
    }));
}

/// 本地时间字符串 `%Y-%m-%d %H:%M:%S`（不引入 chrono，直接调 Win32）。
pub fn timestamp() -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}
