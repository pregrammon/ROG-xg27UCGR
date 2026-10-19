//! VCP/DDC 命令目录、能力字符串解析、机型识别与各机型取值表。
//!
//! 逐项移植自 Python 复刻版 `asusdwc/vcp.py`，而后者又来自对
//! DisplayWidgetCenter 1.5.1.3 的反编译（VCPAPI.cs / SplendidGroup.cs /
//! VCPLanguage.cs / Utils.cs / ImportExportManager.cs）与真机验证。

use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// VCP 码
// ---------------------------------------------------------------------------
pub const VCP_RESTORE_FACTORY: u8 = 0x04;
pub const VCP_RESTORE_COLOR: u8 = 0x08;
pub const VCP_CALI_CONTRO: u8 = 0x0F;
pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_COLOR_TEMP: u8 = 0x14;
pub const VCP_RED_GAIN: u8 = 0x16;
pub const VCP_GREEN_GAIN: u8 = 0x18;
pub const VCP_BLUE_GAIN: u8 = 0x1A;
pub const VCP_COLOR_WEAKNESS: u8 = 0x49;
pub const VCP_RED_SAT: u8 = 0x59;
pub const VCP_YELLOW_SAT: u8 = 0x5A;
pub const VCP_GREEN_SAT: u8 = 0x5B;
pub const VCP_CYAN_SAT: u8 = 0x5C;
pub const VCP_BLUE_SAT: u8 = 0x5D;
pub const VCP_MAGENTA_SAT: u8 = 0x5E;
pub const VCP_INPUT: u8 = 0x60;
pub const VCP_VOLUME: u8 = 0x62;
pub const VCP_BACKLIGHT_WHITE: u8 = 0x6B;
pub const VCP_RED_OFFSET: u8 = 0x6C;
pub const VCP_GREEN_OFFSET: u8 = 0x6E;
pub const VCP_BLUE_OFFSET: u8 = 0x70;
pub const VCP_GAMMA: u8 = 0x72;
pub const VCP_SHARPNESS: u8 = 0x87;
pub const VCP_SATURATION: u8 = 0x8A;
pub const VCP_HUE: u8 = 0x90;
pub const VCP_ORIENTATION: u8 = 0xAA;
pub const VCP_AUDIO_MUTE: u8 = 0x8D;
pub const VCP_OSD_LANGUAGE: u8 = 0xCC;
pub const VCP_POWER_MODE: u8 = 0xD6;
pub const VCP_DISPLAY_MODE: u8 = 0xDC;
pub const VCP_OVERDRIVE: u8 = 0xE0;
pub const VCP_POWER_SAVING: u8 = 0xE1;
pub const VCP_POWER_SAVING_LEGACY: u8 = 0xEE;
pub const VCP_HDR: u8 = 0xE2;
pub const VCP_CROSSHAIR: u8 = 0xE3;
pub const VCP_GAMEPLUS_TIMER: u8 = 0xE4;
pub const VCP_SHADOW_BOOST: u8 = 0xE5;
pub const VCP_BLUE_LIGHT: u8 = 0xE6;
pub const VCP_ALIGNMENT: u8 = 0xE7;
pub const VCP_GAMEPLUS_POSITION: u8 = 0xE8;
pub const VCP_FPS_COUNTER: u8 = 0xEA;
pub const VCP_EZOSD: u8 = 0xEB;
pub const VCP_RESET: u8 = 0xEC;
pub const VCP_REST_REMINDER: u8 = 0xED;
pub const VCP_OLED_REMINDER: u8 = 0xF8;
pub const VCP_SHORTCUTS: u8 = 0xFB;
pub const VCP_TOGGLES1: u8 = 0xFC;
pub const VCP_TOGGLES2: u8 = 0xFD;
pub const VCP_VCP_VERSION: u8 = 0xEF;
pub const VCP_FIRMWARE: u8 = 0xFF;

// 0xFD 内部位域（VCPAPI.ToggleSettings2）
pub const TOGGLE_VRR: u32 = 1 << 0;
pub const TOGGLE_ASCR: u32 = 1 << 1;
pub const TOGGLE_SNIPER: u32 = 1 << 2;
pub const TOGGLE_OLED_SAVER: u32 = 1 << 3;
pub const TOGGLE_PIXEL_CLEAN: u32 = 1 << 4;
pub const TOGGLE_OLED_LOGO: u32 = 1 << 5;
pub const TOGGLE_OLED_UNIFORM: u32 = 1 << 6;
pub const TOGGLE_OLED_CLEAR: u32 = 1 << 7;
pub const TOGGLE_FRAME_BOOST: u32 = 1 << 8;
pub const TOGGLE_AI_VISUAL: u32 = 1 << 9;
pub const TOGGLE_AMBIENT_BRIGHT: u32 = 1 << 11;
pub const TOGGLE_BOUNDARY: u32 = 1 << 12;
pub const TOGGLE_OUTER_DIM: u32 = 1 << 13;
pub const TOGGLE_GLOBAL_DIM: u32 = 1 << 14;

/// 静音位于 VCP 0xFC 的 bit 3（CapabilityConfigManager.GetAudioMuteConfig）。
pub const MUTE_BIT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// 语言
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Zh,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Lang {
        if code.eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::Zh
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    /// 二选一本地化。
    pub fn t(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }
}

// ---------------------------------------------------------------------------
// 名称表
// ---------------------------------------------------------------------------
pub fn vcp_name(code: u8, lang: Lang) -> &'static str {
    match code {
        VCP_RESTORE_FACTORY => lang.t("恢复出厂设置", "Restore factory defaults"),
        VCP_RESTORE_COLOR => lang.t("恢复颜色出厂设置", "Restore color defaults"),
        VCP_CALI_CONTRO => lang.t("校准控制", "Calibration control"),
        VCP_BRIGHTNESS => lang.t("亮度", "Brightness"),
        VCP_CONTRAST => lang.t("对比度", "Contrast"),
        VCP_COLOR_TEMP => lang.t("色温", "Color temperature"),
        VCP_RED_GAIN => lang.t("红色增益", "Red gain"),
        VCP_GREEN_GAIN => lang.t("绿色增益", "Green gain"),
        VCP_BLUE_GAIN => lang.t("蓝色增益", "Blue gain"),
        VCP_COLOR_WEAKNESS => lang.t("色弱模式", "Color weakness"),
        VCP_RED_SAT => lang.t("红色饱和度", "Red saturation"),
        VCP_YELLOW_SAT => lang.t("黄色饱和度", "Yellow saturation"),
        VCP_GREEN_SAT => lang.t("绿色饱和度", "Green saturation"),
        VCP_CYAN_SAT => lang.t("青色饱和度", "Cyan saturation"),
        VCP_BLUE_SAT => lang.t("蓝色饱和度", "Blue saturation"),
        VCP_MAGENTA_SAT => lang.t("洋红饱和度", "Magenta saturation"),
        VCP_INPUT => lang.t("输入源", "Input source"),
        VCP_VOLUME => lang.t("音量", "Volume"),
        VCP_BACKLIGHT_WHITE => lang.t("背光亮度", "Backlight level"),
        VCP_RED_OFFSET => lang.t("红色偏移", "Red offset"),
        VCP_GREEN_OFFSET => lang.t("绿色偏移", "Green offset"),
        VCP_BLUE_OFFSET => lang.t("蓝色偏移", "Blue offset"),
        VCP_GAMMA => lang.t("伽马", "Gamma"),
        VCP_SHARPNESS => lang.t("锐度", "Sharpness"),
        VCP_SATURATION => lang.t("饱和度", "Saturation"),
        VCP_HUE => lang.t("色调", "Hue"),
        VCP_ORIENTATION => lang.t("方向", "Orientation"),
        VCP_AUDIO_MUTE => lang.t("静音", "Audio mute"),
        VCP_OSD_LANGUAGE => lang.t("OSD 语言", "OSD language"),
        VCP_POWER_MODE => lang.t("电源模式", "Power mode"),
        VCP_DISPLAY_MODE => lang.t("显示模式", "Display mode"),
        VCP_OVERDRIVE => lang.t("响应速度", "Overdrive"),
        VCP_POWER_SAVING => lang.t("省电模式", "Power saving"),
        VCP_HDR => lang.t("HDR", "HDR"),
        VCP_CROSSHAIR => lang.t("准星", "Crosshair"),
        VCP_GAMEPLUS_TIMER => lang.t("计时器", "Timer"),
        VCP_SHADOW_BOOST => lang.t("暗部增强", "Shadow Boost"),
        VCP_BLUE_LIGHT => lang.t("滤蓝光", "Blue light filter"),
        VCP_ALIGNMENT => lang.t("显示对齐", "Display alignment"),
        VCP_GAMEPLUS_POSITION => lang.t("游戏辅助位置", "GamePlus position"),
        VCP_FPS_COUNTER => lang.t("FPS 计数器", "FPS counter"),
        VCP_EZOSD => lang.t("OSD 遥控", "OSD remote"),
        VCP_RESET => lang.t("重置", "Reset"),
        VCP_REST_REMINDER => lang.t("休息提醒", "Rest reminder"),
        VCP_TOGGLES1 => lang.t("开关组 1", "Toggle settings 1"),
        VCP_TOGGLES2 => lang.t("开关组 2", "Toggle settings 2"),
        VCP_VCP_VERSION => lang.t("VCP 版本", "VCP version"),
        VCP_FIRMWARE => lang.t("固件版本", "Firmware"),
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// 输入源（MCCS 0x60）
// ---------------------------------------------------------------------------
pub const INPUT_SOURCES: &[(u32, &str)] = &[
    (1, "Analog (RGB) 1"),
    (2, "Analog (RGB) 2"),
    (3, "DVI 1"),
    (4, "DVI 2"),
    (5, "Composite 1"),
    (6, "Composite 2"),
    (7, "S-Video 1"),
    (8, "S-Video 2"),
    (9, "Tuner 1"),
    (10, "Tuner 2"),
    (11, "Tuner 3"),
    (12, "Component 1"),
    (13, "Component 2"),
    (14, "Component 3"),
    (15, "DisplayPort 1"),
    (16, "DisplayPort 2"),
    (17, "HDMI 1"),
    (18, "HDMI 2"),
    (19, "HDMI 3"),
    (20, "HDMI 4"),
    (21, "Thunderbolt 1"),
    (22, "Thunderbolt 2"),
    (23, "Thunderbolt 3"),
    (24, "Thunderbolt 4"),
    (26, "USB Type-C 1"),
    (27, "USB Type-C 2"),
    (28, "USB Type-C 3"),
    (29, "USB Type-C 4"),
];

pub fn input_name(value: u32) -> String {
    INPUT_SOURCES
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| format!("0x{value:02X}"))
}

// ---------------------------------------------------------------------------
// OSD 语言（VCPLanguage）
// ---------------------------------------------------------------------------
pub const OSD_LANGUAGES: &[(u32, &str)] = &[
    (1, "繁體中文"),
    (2, "English"),
    (3, "Français"),
    (4, "Deutsch"),
    (5, "Italiano"),
    (6, "日本語"),
    (7, "한국어"),
    (8, "Português"),
    (9, "Русский"),
    (10, "Español"),
    (12, "Türkçe"),
    (13, "简体中文"),
    (17, "Hrvatski"),
    (18, "Čeština"),
    (20, "Nederlands"),
    (26, "Magyar"),
    (30, "Polski"),
    (31, "Română"),
    (35, "ไทย"),
    (36, "Українська"),
    (37, "Tiếng Việt"),
    (38, "فارسی"),
    (39, "Bahasa Indonesia"),
];

pub fn osd_language_name(value: u32) -> String {
    OSD_LANGUAGES
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| format!("0x{value:02X}"))
}

// ---------------------------------------------------------------------------
// 其它枚举
// ---------------------------------------------------------------------------
pub fn power_mode_name(value: u32, lang: Lang) -> &'static str {
    match value {
        1 => lang.t("开机", "On"),
        4 => lang.t("待机", "Standby"),
        5 => lang.t("关机", "Off"),
        _ => "",
    }
}

fn crosshair_zh(value: u32) -> &'static str {
    match value {
        0 => "关闭",
        1 => "红点",
        2 => "绿点",
        3 => "红色圆环",
        4 => "绿色圆环",
        5 => "红色十字",
        6 => "绿色十字",
        7 => "蓝点",
        8 => "绿色准星",
        9 => "蓝色目标圆环",
        10 => "绿色目标圆环",
        11 => "蓝色目标十字",
        12 => "绿色目标十字",
        _ => "",
    }
}

fn crosshair_en(value: u32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Red point",
        2 => "Green point",
        3 => "Red ring",
        4 => "Green ring",
        5 => "Red cross",
        6 => "Green cross",
        7 => "Blue point",
        8 => "Green dot",
        9 => "Blue target",
        10 => "Green target",
        11 => "Blue cross",
        12 => "Green cross",
        _ => "",
    }
}

pub fn crosshair_name(value: u32, lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => crosshair_zh(value),
        Lang::En => crosshair_en(value),
    }
}

/// 新版面板只接受 7..12 号准星；能力字符串含 07 或 0C 时判定为新式
/// （移植 `CheckCrosshairTypeFromDictionary`）。
pub fn crosshair_styles(values: &[u32]) -> Vec<u32> {
    let new_style = values.iter().any(|v| *v == 7 || *v == 12);
    let all: &[u32] = if new_style {
        &[0, 7, 8, 9, 10, 11, 12]
    } else {
        &[0, 1, 2, 3, 4, 5, 6]
    };
    if values.is_empty() {
        return all.to_vec();
    }
    let mut out: Vec<u32> = all
        .iter()
        .copied()
        .filter(|v| *v == 0 || values.contains(v))
        .collect();
    if !out.contains(&0) {
        out.insert(0, 0);
    }
    out
}

pub fn timer_label(value: u32, lang: Lang) -> &'static str {
    match value {
        0 => lang.t("关闭", "Off"),
        1 => lang.t("30 分钟", "30 min"),
        2 => lang.t("40 分钟", "40 min"),
        3 => lang.t("50 分钟", "50 min"),
        4 => lang.t("60 分钟", "60 min"),
        5 => lang.t("90 分钟", "90 min"),
        _ => "",
    }
}

pub const FPS_VALUES: &[(u32, &str, &str)] = &[
    (0, "关闭", "Off"),
    (1, "数字", "Numeric"),
    (2, "柱状图", "Bar"),
];

pub const SHADOW_BOOST_VALUES: &[(u32, &str, &str)] = &[
    (0, "关", "Off"),
    (1, "低", "Low"),
    (2, "中", "Mid"),
    (3, "高", "High"),
    (4, "动态", "Dynamic"),
];

pub const POSITION_VALUES: &[(u32, &str, &str)] = &[
    (1, "左上", "Top left"),
    (2, "右上", "Top right"),
    (3, "左下", "Bottom left"),
    (4, "右下", "Bottom right"),
];

// ---------------------------------------------------------------------------
// 色温与 GameVisual 模式表
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, Debug)]
pub struct ColorTemp {
    pub value: u32,
    pub zh: &'static str,
    pub en: &'static str,
}

impl ColorTemp {
    pub fn label(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::Zh => self.zh,
            Lang::En => self.en,
        }
    }
}

pub const GAMING_COLOR_TEMPS: &[ColorTemp] = &[
    ColorTemp { value: 3, zh: "4000K", en: "4000K" },
    ColorTemp { value: 4, zh: "5000K", en: "5000K" },
    ColorTemp { value: 5, zh: "6500K", en: "6500K" },
    ColorTemp { value: 6, zh: "7500K", en: "7500K" },
    ColorTemp { value: 7, zh: "8200K", en: "8200K" },
    ColorTemp { value: 8, zh: "9300K", en: "9300K" },
    ColorTemp { value: 9, zh: "10000K", en: "10000K" },
    ColorTemp { value: 11, zh: "用户", en: "User" },
];

pub const VG_COLOR_TEMPS: &[ColorTemp] = &[
    ColorTemp { value: 8, zh: "冷色", en: "Cool" },
    ColorTemp { value: 6, zh: "正常", en: "Normal" },
    ColorTemp { value: 5, zh: "暖色", en: "Warm" },
    ColorTemp { value: 11, zh: "用户", en: "User" },
];

pub const PROART_COLOR_TEMPS: &[ColorTemp] = &[
    ColorTemp { value: 4, zh: "5000K", en: "5000K" },
    ColorTemp { value: 14, zh: "5500K", en: "5500K" },
    ColorTemp { value: 5, zh: "6500K", en: "6500K" },
    ColorTemp { value: 6, zh: "7500K", en: "7500K" },
    ColorTemp { value: 8, zh: "9300K", en: "9300K" },
    ColorTemp { value: 15, zh: "P3-Theater", en: "P3-Theater" },
    ColorTemp { value: 17, zh: "M Model", en: "M Model" },
];

#[derive(Clone, Copy, Debug)]
pub struct DisplayMode {
    pub value: u32,
    pub key: &'static str,
    pub zh: &'static str,
    pub en: &'static str,
}

impl DisplayMode {
    pub fn label(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::Zh => self.zh,
            Lang::En => self.en,
        }
    }
}

pub const GAMING_MODES: &[DisplayMode] = &[
    DisplayMode { value: 1, key: "cinema", zh: "影院", en: "Cinema" },
    DisplayMode { value: 2, key: "scenery", zh: "风景", en: "Scenery" },
    DisplayMode { value: 3, key: "srgb", zh: "sRGB", en: "sRGB" },
    DisplayMode { value: 4, key: "user", zh: "用户", en: "User" },
    DisplayMode { value: 5, key: "racing", zh: "竞速", en: "Racing" },
    DisplayMode { value: 6, key: "rts", zh: "RTS/RPG", en: "RTS/RPG" },
    DisplayMode { value: 7, key: "fps", zh: "FPS", en: "FPS" },
    DisplayMode { value: 8, key: "moba", zh: "MOBA", en: "MOBA" },
];

pub const MAINSTREAM_MODES: &[DisplayMode] = &[
    DisplayMode { value: 1, key: "theater", zh: "影院", en: "Theater" },
    DisplayMode { value: 2, key: "scenery", zh: "风景", en: "Scenery" },
    DisplayMode { value: 3, key: "srgb", zh: "sRGB", en: "sRGB" },
    DisplayMode { value: 4, key: "standard", zh: "标准", en: "Standard" },
    DisplayMode { value: 5, key: "game", zh: "游戏", en: "Game" },
    DisplayMode { value: 6, key: "night", zh: "夜间", en: "Night View" },
    DisplayMode { value: 7, key: "reading", zh: "阅读", en: "Reading" },
    DisplayMode { value: 8, key: "darkroom", zh: "暗房", en: "Darkroom" },
];

pub const PROART_MODES: &[DisplayMode] = &[
    DisplayMode { value: 1, key: "native", zh: "原生", en: "Native" },
    DisplayMode { value: 5, key: "srgb", zh: "sRGB", en: "sRGB" },
    DisplayMode { value: 8, key: "adobe", zh: "Adobe RGB", en: "Adobe RGB" },
    DisplayMode { value: 9, key: "rec2020", zh: "Rec.2020", en: "Rec.2020" },
    DisplayMode { value: 10, key: "dcip3", zh: "DCI-P3", en: "DCI-P3" },
    DisplayMode { value: 11, key: "dicom", zh: "DICOM", en: "DICOM" },
    DisplayMode { value: 14, key: "rec709", zh: "Rec.709", en: "Rec.709" },
    DisplayMode { value: 22, key: "user1", zh: "用户 1", en: "User 1" },
    DisplayMode { value: 23, key: "user2", zh: "用户 2", en: "User 2" },
    DisplayMode { value: 30, key: "user3", zh: "用户 3", en: "User 3" },
    DisplayMode { value: 31, key: "displayp3", zh: "Display P3", en: "Display P3" },
    DisplayMode { value: 32, key: "mp3", zh: "M Model P3", en: "M Model P3" },
];

pub const GAMMA_STEPS: &[(u32, &str)] = &[
    (0x5000, "1.8"),
    (0x6400, "2.0"),
    (0x7800, "2.2"),
    (0x8C00, "2.4"),
    (0xA000, "2.6"),
];

// ---------------------------------------------------------------------------
// 机型系列识别（移植 Utils.GetProductLine）
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProductLine {
    #[default]
    None,
    Gaming,
    Mainstream,
    Portable,
    ProArt,
    Lightbar,
    Glasses,
}

impl ProductLine {
    pub fn as_str(self) -> &'static str {
        match self {
            ProductLine::None => "none",
            ProductLine::Gaming => "gaming",
            ProductLine::Mainstream => "mainstream",
            ProductLine::Portable => "portable",
            ProductLine::ProArt => "proart",
            ProductLine::Lightbar => "lightbar",
            ProductLine::Glasses => "glasses",
        }
    }

    pub fn from_str(s: &str) -> ProductLine {
        match s {
            "gaming" => ProductLine::Gaming,
            "mainstream" => ProductLine::Mainstream,
            "portable" => ProductLine::Portable,
            "proart" => ProductLine::ProArt,
            "lightbar" => ProductLine::Lightbar,
            "glasses" => ProductLine::Glasses,
            _ => ProductLine::None,
        }
    }
}

pub fn clean_model_name(name: &str) -> String {
    let trimmed = name.trim();
    let without_asus = if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("ASUS") {
        trimmed[4..].trim()
    } else {
        trimmed
    };
    without_asus.replace(' ', "")
}

/// 去掉型号前导的 "12. " 这类序号（正则 `^\d+\.\s*`）。
fn strip_number_prefix(model: &str) -> &str {
    let bytes = model.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        return &model[i..];
    }
    model
}

pub fn product_line(model: &str) -> ProductLine {
    let model = clean_model_name(model);
    let low = model.to_ascii_lowercase();
    if low.contains("_d") || low.contains("gcd01") {
        return ProductLine::Gaming;
    }
    if low.contains("r1") {
        return ProductLine::Glasses;
    }
    if low.contains("benq") || low.contains("generic") || model.chars().count() < 5 {
        return ProductLine::None;
    }
    if low.contains("alb") {
        return ProductLine::Lightbar;
    }
    let stripped = strip_number_prefix(&model);
    let prefix: String = stripped.chars().take(2).collect::<String>().to_ascii_lowercase();
    // "_D" 的原始判断区分大小写，但上面已经不区分大小写命中过，这里保留原始语义。
    if prefix == "mb" || prefix == "mq" {
        return ProductLine::Portable;
    }
    if matches!(
        prefix.as_str(),
        "vx" | "vp" | "vy" | "vz" | "vu" | "va" | "vt" | "be"
    ) {
        return ProductLine::Mainstream;
    }
    if prefix == "pa" || prefix == "pq" {
        return ProductLine::ProArt;
    }
    if matches!(prefix.as_str(), "vg" | "pg" | "xg") || model.contains("_D") {
        return ProductLine::Gaming;
    }
    ProductLine::None
}

pub fn mode_set(line: ProductLine) -> &'static [DisplayMode] {
    match line {
        ProductLine::ProArt => PROART_MODES,
        ProductLine::Mainstream | ProductLine::Portable => MAINSTREAM_MODES,
        _ => GAMING_MODES,
    }
}

pub fn color_temps(line: ProductLine, model: &str) -> &'static [ColorTemp] {
    let model_upper = model.to_ascii_uppercase();
    match line {
        ProductLine::ProArt => PROART_COLOR_TEMPS,
        ProductLine::Gaming => {
            if model_upper.contains("VG") {
                VG_COLOR_TEMPS
            } else {
                GAMING_COLOR_TEMPS
            }
        }
        _ => VG_COLOR_TEMPS,
    }
}

pub fn display_mode_code(line: ProductLine) -> u8 {
    if line == ProductLine::ProArt {
        VCP_CROSSHAIR
    } else {
        VCP_DISPLAY_MODE
    }
}

/// 移植 `VCPAPI.SetDisplayMode` 的 ProArt 字节打包。
pub fn encode_display_mode(line: ProductLine, value: u32) -> u32 {
    if line == ProductLine::ProArt {
        const SPECIALS: &[u32] = &[
            3840, 3841, 3842, 4096, 4097, 4098, 4607, 4863, 4867, 4868, 5120, 5121, 5122,
        ];
        if !SPECIALS.contains(&value) && value < 256 {
            return (value << 8) | 0xFF;
        }
    }
    value
}

// ---------------------------------------------------------------------------
// 能力字符串解析（移植 DisplayManager.ParseString）
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapabilityInfo {
    pub raw: String,
    pub model: String,
    pub mccs_version: String,
    pub vcp: BTreeMap<u8, Vec<u32>>,
}

impl CapabilityInfo {
    pub fn supports(&self, code: u8) -> bool {
        self.vcp.contains_key(&code)
    }

    pub fn values(&self, code: u8) -> &[u32] {
        self.vcp.get(&code).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn vcp_count(&self) -> usize {
        self.vcp.len()
    }
}

fn extract_block<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find(&keyword.to_ascii_lowercase())?;
    let bytes = text.as_bytes();
    let mut open = idx + keyword.len();
    while open < bytes.len() && (bytes[open] as char).is_whitespace() {
        open += 1;
    }
    if open >= bytes.len() || bytes[open] != b'(' {
        return None;
    }
    let start = open + 1;
    let mut depth = 0i32;
    for (offset, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(&text[start..start + offset]);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// 解析 `vcp(10 12(0B 0C) 60(0F 11) ...)` 块。
fn parse_vcp_block(block: &str) -> BTreeMap<u8, Vec<u32>> {
    let mut out = BTreeMap::new();
    let bytes = block.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        let (Some(hi), Some(lo)) = (hi, lo) else {
            i += 1;
            continue;
        };
        let code = ((hi << 4) | lo) as u8;
        i += 2;
        // 跳过空白
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        let mut values: Vec<u32> = Vec::new();
        if i < bytes.len() && bytes[i] == b'(' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b')' {
                i += 1;
            }
            let inner = &block[start..i.min(block.len())];
            for token in inner.split_whitespace() {
                if let Ok(v) = u32::from_str_radix(token, 16) {
                    values.push(v);
                }
            }
            if i < bytes.len() && bytes[i] == b')' {
                i += 1;
            }
        }
        // 同一个码重复出现时保留第一次（与字典覆盖语义一致）。
        out.entry(code).or_insert(values);
        // 跳过码之间的分隔符
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
    }
    out
}

pub fn parse_capabilities(text: &str) -> CapabilityInfo {
    let mut info = CapabilityInfo {
        raw: text.to_string(),
        ..Default::default()
    };
    if text.trim().is_empty() {
        return info;
    }
    if let Some(model) = extract_block(text, "model") {
        info.model = clean_model_name(model);
    }
    if let Some(mccs) = extract_block(text, "mccs_ver") {
        info.mccs_version = mccs.trim().to_string();
    }
    if let Some(block) = extract_block(text, "vcp") {
        info.vcp = parse_vcp_block(block);
    }
    info
}

// ---------------------------------------------------------------------------
// 版本字符串与伽马
// ---------------------------------------------------------------------------
/// VCPAPI.GetFWVersion：固件号取 max，Panel ID 取 current 低字节。
pub fn firmware_string(current: u32, maximum: u32) -> String {
    if maximum == 0 {
        return "—".into();
    }
    format!(
        "{}.{:02X} (panel 0x{:02X})",
        maximum >> 8,
        maximum & 0xFF,
        current & 0xFF
    )
}

/// VCPAPI.GetVCPVersion：版本取 max 高字节、修订取低字节。
pub fn vcp_version_string(maximum: u32) -> String {
    if maximum == 0 {
        return "—".into();
    }
    format!("{}.{}", maximum >> 8, maximum & 0xFF)
}

/// 当前伽马值在 5 档表中的下标；失败时用高字节兜底。
pub fn gamma_index(raw: u32) -> Option<usize> {
    if let Some(idx) = GAMMA_STEPS.iter().position(|(value, _)| *value == raw) {
        return Some(idx);
    }
    let high = (raw >> 8) & 0xFF;
    match high {
        80 => Some(0),
        100 => Some(1),
        120 => Some(2),
        140 => Some(3),
        160 => Some(4),
        _ => None,
    }
}

/// 供 UI 使用的“能力字符串声明了哪些模式”判断。
pub fn mode_supported(caps: &CapabilityInfo, line: ProductLine, value: u32) -> bool {
    let code = display_mode_code(line);
    let values = caps.values(code);
    if values.is_empty() {
        return true;
    }
    values.contains(&value)
        || values.contains(&(value << 8))
        || values.contains(&((value << 8) | 0xFF))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: &str = "mccs_ver(2.2)model(XG27UCGR)vcp(02 04 05 10 12 14(03 04 05 06 07 08 09 0B) 16 18 1A 60(0F 11 12 1A) 62 72 87 8A 90 E0 E1 E3(00 07 08 09 0A 0B 0C) E4 E5 E6 E7 E8 EA EB ED EF FC FD)";

    #[test]
    fn parses_capabilities() {
        let caps = parse_capabilities(CAPS);
        assert_eq!(caps.model, "XG27UCGR");
        assert_eq!(caps.mccs_version, "2.2");
        assert_eq!(caps.values(VCP_COLOR_TEMP), &[3, 4, 5, 6, 7, 8, 9, 11]);
        assert_eq!(caps.values(VCP_INPUT), &[15, 17, 18, 26]);
        assert!(caps.supports(VCP_TOGGLES2));
    }

    #[test]
    fn detects_gaming_line() {
        assert_eq!(product_line("ASUS XG27UCGR"), ProductLine::Gaming);
        assert_eq!(product_line("PA248QV"), ProductLine::ProArt);
        assert_eq!(product_line("VX239H"), ProductLine::Mainstream);
        assert_eq!(product_line("MB16AC"), ProductLine::Portable);
        assert_eq!(product_line("1. XG27UCGR"), ProductLine::Gaming);
    }

    #[test]
    fn packs_proart_mode() {
        assert_eq!(encode_display_mode(ProductLine::ProArt, 5), 0x05FF);
        assert_eq!(encode_display_mode(ProductLine::ProArt, 3840), 3840);
        assert_eq!(encode_display_mode(ProductLine::Gaming, 5), 5);
    }

    #[test]
    fn gamma_roundtrip() {
        assert_eq!(gamma_index(0x7800), Some(2));
        assert_eq!(gamma_index(0x7801), Some(2));
        assert_eq!(gamma_index(0), None);
    }

    #[test]
    fn crosshair_styles_follow_caps() {
        let styles = crosshair_styles(&[0, 7, 8, 12]);
        assert_eq!(styles, vec![0, 7, 8, 12]);
        let legacy = crosshair_styles(&[]);
        assert_eq!(legacy, vec![0, 1, 2, 3, 4, 5, 6]);
    }
}
