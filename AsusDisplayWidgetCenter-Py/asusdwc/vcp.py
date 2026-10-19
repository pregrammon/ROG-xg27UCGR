"""VCP/DDC command catalogue reconstructed from ASUS DisplayWidgetCenter.

The constants and value tables below were recovered by decompiling
``DisplayWidgetCenter.exe`` (VCPAPI.cs / SplendidGroup.cs / VCPLanguage.cs /
Utils.cs / ImportExportManager.cs) and then verified against a real
ASUS XG27UCGR panel over DDC/CI.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# VCP codes used by the application (decimal -> hex)
# ---------------------------------------------------------------------------
VCP_RESTORE_FACTORY = 0x04
VCP_RESTORE_COLOR = 0x08
VCP_CALI_CONTRO = 0x0F
VCP_BRIGHTNESS = 0x10
VCP_CONTRAST = 0x12
VCP_COLOR_TEMP = 0x14
VCP_RED_GAIN = 0x16
VCP_GREEN_GAIN = 0x18
VCP_BLUE_GAIN = 0x1A
VCP_COLOR_WEAKNESS = 0x49
VCP_RED_SAT = 0x59
VCP_YELLOW_SAT = 0x5A
VCP_GREEN_SAT = 0x5B
VCP_CYAN_SAT = 0x5C
VCP_BLUE_SAT = 0x5D
VCP_MAGENTA_SAT = 0x5E
VCP_INPUT = 0x60
VCP_VOLUME = 0x62
VCP_BACKLIGHT_WHITE = 0x6B
VCP_RED_OFFSET = 0x6C
VCP_GREEN_OFFSET = 0x6E
VCP_BLUE_OFFSET = 0x70
VCP_GAMMA = 0x72
VCP_SHARPNESS = 0x87
VCP_SATURATION = 0x8A
VCP_HUE = 0x90
VCP_ORIENTATION = 0xAA
VCP_AUDIO_MUTE = 0x8D
VCP_OSD_LANGUAGE = 0xCC
VCP_POWER_MODE = 0xD6
VCP_DISPLAY_MODE = 0xDC
VCP_OVERDRIVE = 0xE0
VCP_POWER_SAVING = 0xE1
VCP_POWER_SAVING_LEGACY = 0xEE
VCP_HDR = 0xE2
VCP_CROSSHAIR = 0xE3
VCP_GAMEPLUS_TIMER = 0xE4
VCP_SHADOW_BOOST = 0xE5
VCP_BLUE_LIGHT = 0xE6
VCP_ALIGNMENT = 0xE7
VCP_GAMEPLUS_POSITION = 0xE8
VCP_FPS_COUNTER = 0xEA
VCP_EZOSD = 0xEB
VCP_RESET = 0xEC
VCP_REST_REMINDER = 0xED
VCP_OLED_REMINDER = 0xF8
VCP_SHORTCUTS = 0xFB
VCP_TOGGLES1 = 0xFC
VCP_TOGGLES2 = 0xFD
VCP_VCP_VERSION = 0xEF
VCP_FIRMWARE = 0xFF

# toggle bits inside VCP 0xFD (from VCPAPI.ToggleSettings2)
TOGGLE_VRR = 1 << 0
TOGGLE_ASCR = 1 << 1
TOGGLE_SNIPER = 1 << 2
TOGGLE_OLED_SAVER = 1 << 3
TOGGLE_PIXEL_CLEAN = 1 << 4
TOGGLE_OLED_LOGO = 1 << 5
TOGGLE_OLED_UNIFORM = 1 << 6
TOGGLE_OLED_CLEAR = 1 << 7
TOGGLE_FRAME_BOOST = 1 << 8
TOGGLE_AI_VISUAL = 1 << 9
TOGGLE_AMBIENT_BRIGHT = 1 << 11
TOGGLE_BOUNDARY = 1 << 12
TOGGLE_OUTER_DIM = 1 << 13
TOGGLE_GLOBAL_DIM = 1 << 14

# mute lives in bit 3 of VCP 0xFC (CapabilityConfigManager.GetAudioMuteConfig)
MUTE_BIT = 1 << 3

VCP_NAMES = {
    VCP_RESTORE_FACTORY: "恢复出厂设置",
    VCP_RESTORE_COLOR: "恢复颜色出厂设置",
    VCP_CALI_CONTRO: "校准控制",
    VCP_BRIGHTNESS: "亮度",
    VCP_CONTRAST: "对比度",
    VCP_COLOR_TEMP: "色温",
    VCP_RED_GAIN: "红色增益",
    VCP_GREEN_GAIN: "绿色增益",
    VCP_BLUE_GAIN: "蓝色增益",
    VCP_COLOR_WEAKNESS: "色弱模式",
    VCP_RED_SAT: "红色饱和度",
    VCP_YELLOW_SAT: "黄色饱和度",
    VCP_GREEN_SAT: "绿色饱和度",
    VCP_CYAN_SAT: "青色饱和度",
    VCP_BLUE_SAT: "蓝色饱和度",
    VCP_MAGENTA_SAT: "洋红饱和度",
    VCP_INPUT: "输入源",
    VCP_VOLUME: "音量",
    VCP_BACKLIGHT_WHITE: "背光亮度",
    VCP_RED_OFFSET: "红色偏移",
    VCP_GREEN_OFFSET: "绿色偏移",
    VCP_BLUE_OFFSET: "蓝色偏移",
    VCP_GAMMA: "伽马",
    VCP_SHARPNESS: "锐度",
    VCP_SATURATION: "饱和度",
    VCP_HUE: "色调",
    VCP_ORIENTATION: "方向",
    VCP_AUDIO_MUTE: "静音",
    VCP_OSD_LANGUAGE: "OSD 语言",
    VCP_POWER_MODE: "电源模式",
    VCP_DISPLAY_MODE: "显示模式",
    VCP_OVERDRIVE: "响应速度",
    VCP_POWER_SAVING: "省电模式",
    VCP_HDR: "HDR",
    VCP_CROSSHAIR: "准星",
    VCP_GAMEPLUS_TIMER: "计时器",
    VCP_SHADOW_BOOST: "暗部增强",
    VCP_BLUE_LIGHT: "滤蓝光",
    VCP_ALIGNMENT: "显示对齐",
    VCP_GAMEPLUS_POSITION: "游戏辅助位置",
    VCP_FPS_COUNTER: "FPS 计数器",
    VCP_EZOSD: "OSD 遥控",
    VCP_RESET: "重置",
    VCP_REST_REMINDER: "休息提醒",
    VCP_TOGGLES1: "开关组 1",
    VCP_TOGGLES2: "开关组 2",
    VCP_VCP_VERSION: "VCP 版本",
    VCP_FIRMWARE: "固件版本",
}

VCP_NAMES_EN = {
    VCP_RESTORE_FACTORY: "Restore factory defaults",
    VCP_RESTORE_COLOR: "Restore color defaults",
    VCP_CALI_CONTRO: "Calibration control",
    VCP_BRIGHTNESS: "Brightness",
    VCP_CONTRAST: "Contrast",
    VCP_COLOR_TEMP: "Color temperature",
    VCP_RED_GAIN: "Red gain",
    VCP_GREEN_GAIN: "Green gain",
    VCP_BLUE_GAIN: "Blue gain",
    VCP_COLOR_WEAKNESS: "Color weakness",
    VCP_RED_SAT: "Red saturation",
    VCP_YELLOW_SAT: "Yellow saturation",
    VCP_GREEN_SAT: "Green saturation",
    VCP_CYAN_SAT: "Cyan saturation",
    VCP_BLUE_SAT: "Blue saturation",
    VCP_MAGENTA_SAT: "Magenta saturation",
    VCP_INPUT: "Input source",
    VCP_VOLUME: "Volume",
    VCP_BACKLIGHT_WHITE: "Backlight level",
    VCP_RED_OFFSET: "Red offset",
    VCP_GREEN_OFFSET: "Green offset",
    VCP_BLUE_OFFSET: "Blue offset",
    VCP_GAMMA: "Gamma",
    VCP_SHARPNESS: "Sharpness",
    VCP_SATURATION: "Saturation",
    VCP_HUE: "Hue",
    VCP_ORIENTATION: "Orientation",
    VCP_AUDIO_MUTE: "Audio mute",
    VCP_OSD_LANGUAGE: "OSD language",
    VCP_POWER_MODE: "Power mode",
    VCP_DISPLAY_MODE: "Display mode",
    VCP_OVERDRIVE: "Overdrive",
    VCP_POWER_SAVING: "Power saving",
    VCP_HDR: "HDR",
    VCP_CROSSHAIR: "Crosshair",
    VCP_GAMEPLUS_TIMER: "Timer",
    VCP_SHADOW_BOOST: "Shadow Boost",
    VCP_BLUE_LIGHT: "Blue light filter",
    VCP_ALIGNMENT: "Display alignment",
    VCP_GAMEPLUS_POSITION: "GamePlus position",
    VCP_FPS_COUNTER: "FPS counter",
    VCP_EZOSD: "OSD remote",
    VCP_RESET: "Reset",
    VCP_REST_REMINDER: "Rest reminder",
    VCP_TOGGLES1: "Toggle settings 1",
    VCP_TOGGLES2: "Toggle settings 2",
    VCP_VCP_VERSION: "VCP version",
    VCP_FIRMWARE: "Firmware",
}


# MCCS 0x60 input source values (VCPAPI.GetSourceTypeFromTable)
INPUT_SOURCES = {
    1: "Analog (RGB) 1",
    2: "Analog (RGB) 2",
    3: "DVI 1",
    4: "DVI 2",
    5: "Composite 1",
    6: "Composite 2",
    7: "S-Video 1",
    8: "S-Video 2",
    9: "Tuner 1",
    10: "Tuner 2",
    11: "Tuner 3",
    12: "Component 1",
    13: "Component 2",
    14: "Component 3",
    15: "DisplayPort 1",
    16: "DisplayPort 2",
    17: "HDMI 1",
    18: "HDMI 2",
    19: "HDMI 3",
    20: "HDMI 4",
    21: "Thunderbolt 1",
    22: "Thunderbolt 2",
    23: "Thunderbolt 3",
    24: "Thunderbolt 4",
    26: "USB Type-C 1",
    27: "USB Type-C 2",
    28: "USB Type-C 3",
    29: "USB Type-C 4",
}

# VCPLanguage
OSD_LANGUAGES = {
    1: "繁體中文",
    2: "English",
    3: "Français",
    4: "Deutsch",
    5: "Italiano",
    6: "日本語",
    7: "한국어",
    8: "Português",
    9: "Русский",
    10: "Español",
    12: "Türkçe",
    13: "简体中文",
    17: "Hrvatski",
    18: "Čeština",
    20: "Nederlands",
    26: "Magyar",
    30: "Polski",
    31: "Română",
    35: "ไทย",
    36: "Українська",
    37: "Tiếng Việt",
    38: "فارسی",
    39: "Bahasa Indonesia",
}

POWER_MODES = {1: "开机", 4: "待机", 5: "关机"}

CROSSHAIR_STYLES = {
    0: "关闭",
    1: "红点",
    2: "绿点",
    3: "红色圆环",
    4: "绿色圆环",
    5: "红色十字",
    6: "绿色十字",
    7: "蓝点",
    8: "绿色准星",
    9: "蓝色目标圆环",
    10: "绿色目标圆环",
    11: "蓝色目标十字",
    12: "绿色目标十字",
}

# Newer panels advertise E3(00 07 08 09 0A 0B 0C ...) and only accept the
# 7..12 "new crosshair" style ids.  CheckCrosshairTypeFromDictionary in the
# original app tests for sub-keys 07/0C.
CROSSHAIR_STYLES_NEW = {
    0: "关闭",
    7: "蓝点",
    8: "绿点",
    9: "蓝色目标圆环",
    10: "绿色目标圆环",
    11: "蓝色目标十字",
    12: "绿色目标十字",
}


def crosshair_styles(values: list[int] | None = None) -> dict[int, str]:
    values = values or []
    if 7 in values or 12 in values:
        styles = dict(CROSSHAIR_STYLES_NEW)
    else:
        styles = {key: value for key, value in CROSSHAIR_STYLES.items() if key <= 6}
    if values:
        styles = {key: name for key, name in styles.items() if key == 0 or key in values}
        styles.setdefault(0, "关闭")
    return styles


TIMER_VALUES = {0: "关闭", 1: "30 分钟", 2: "40 分钟", 3: "50 分钟", 4: "60 分钟", 5: "90 分钟"}
FPS_VALUES = {0: "关闭", 1: "数字", 2: "柱状图"}
SHADOW_BOOST = {0: "关闭", 1: "级别 1", 2: "级别 2", 3: "级别 3", 4: "动态调整"}
POSITION_VALUES = {1: "左上", 2: "右上", 3: "左下", 4: "右下"}


@dataclass(frozen=True)
class ColorTemp:
    value: int
    label: str
    label_en: str = ""


GAMING_COLOR_TEMPS = [
    ColorTemp(3, "4000K", "4000K"),
    ColorTemp(4, "5000K", "5000K"),
    ColorTemp(5, "6500K", "6500K"),
    ColorTemp(6, "7500K", "7500K"),
    ColorTemp(7, "8200K", "8200K"),
    ColorTemp(8, "9300K", "9300K"),
    ColorTemp(9, "10000K", "10000K"),
    ColorTemp(11, "用户", "User"),
]
VG_COLOR_TEMPS = [
    ColorTemp(8, "冷色", "Cool"),
    ColorTemp(6, "正常", "Normal"),
    ColorTemp(5, "暖色", "Warm"),
    ColorTemp(11, "用户", "User"),
]
PROART_COLOR_TEMPS = [
    ColorTemp(4, "5000K", "5000K"),
    ColorTemp(14, "5500K", "5500K"),
    ColorTemp(5, "6500K", "6500K"),
    ColorTemp(6, "7500K", "7500K"),
    ColorTemp(8, "9300K", "9300K"),
    ColorTemp(15, "P3-Theater", "P3-Theater"),
    ColorTemp(17, "M Model", "M Model"),
]


def color_temp_label(item: ColorTemp, lang: str = "zh") -> str:
    if lang == "en" and item.label_en:
        return item.label_en
    return item.label


@dataclass(frozen=True)
class DisplayMode:
    value: int
    key: str
    zh: str
    en: str


GAMING_MODES = [
    DisplayMode(1, "cinema", "影院", "Cinema"),
    DisplayMode(2, "scenery", "风景", "Scenery"),
    DisplayMode(3, "srgb", "sRGB", "sRGB"),
    DisplayMode(4, "user", "用户", "User"),
    DisplayMode(5, "racing", "竞速", "Racing"),
    DisplayMode(6, "rts", "RTS/RPG", "RTS/RPG"),
    DisplayMode(7, "fps", "FPS", "FPS"),
    DisplayMode(8, "moba", "MOBA", "MOBA"),
]
MAINSTREAM_MODES = [
    DisplayMode(1, "theater", "影院", "Theater"),
    DisplayMode(2, "scenery", "风景", "Scenery"),
    DisplayMode(3, "srgb", "sRGB", "sRGB"),
    DisplayMode(4, "standard", "标准", "Standard"),
    DisplayMode(5, "game", "游戏", "Game"),
    DisplayMode(6, "night", "夜间", "Night View"),
    DisplayMode(7, "reading", "阅读", "Reading"),
    DisplayMode(8, "darkroom", "暗房", "Darkroom"),
]
PROART_MODES = [
    DisplayMode(1, "native", "原生", "Native"),
    DisplayMode(5, "srgb", "sRGB", "sRGB"),
    DisplayMode(8, "adobe", "Adobe RGB", "Adobe RGB"),
    DisplayMode(9, "rec2020", "Rec.2020", "Rec.2020"),
    DisplayMode(10, "dcip3", "DCI-P3", "DCI-P3"),
    DisplayMode(11, "dicom", "DICOM", "DICOM"),
    DisplayMode(14, "rec709", "Rec.709", "Rec.709"),
    DisplayMode(22, "user1", "用户 1", "User 1"),
    DisplayMode(23, "user2", "用户 2", "User 2"),
    DisplayMode(30, "user3", "用户 3", "User 3"),
    DisplayMode(31, "displayp3", "Display P3", "Display P3"),
    DisplayMode(32, "mp3", "M Model P3", "M Model P3"),
]

GAMMA_STEPS = [
    (0x5000, "1.8"),
    (0x6400, "2.0"),
    (0x7800, "2.2"),
    (0x8C00, "2.4"),
    (0xA000, "2.6"),
]


# ---------------------------------------------------------------------------
# Product line detection - faithful port of Utils.GetProductLine
# ---------------------------------------------------------------------------
class ProductLine:
    NONE = "none"
    GAMING = "gaming"
    MAINSTREAM = "mainstream"
    PORTABLE = "portable"
    PROART = "proart"
    LIGHTBAR = "lightbar"
    GLASSES = "glasses"


def clean_model_name(name: str) -> str:
    name = (name or "").strip()
    if name.upper().startswith("ASUS"):
        name = name[4:].strip()
    return name.replace(" ", "")


def product_line(model: str) -> str:
    model = clean_model_name(model or "")
    low = model.lower()
    if "_d" in low or "gcd01" in low:
        return ProductLine.GAMING
    if "r1" in low:
        return ProductLine.GLASSES
    if "benq" in low or "generic" in low or len(model) < 5:
        return ProductLine.NONE
    if "alb" in low:
        return ProductLine.LIGHTBAR
    stripped = re.sub(r"^\d+\.\s*", "", model)
    prefix = stripped[:2].lower()
    if prefix in ("mb", "mq"):
        return ProductLine.PORTABLE
    if prefix in ("vx", "vp", "vy", "vz", "vu", "va", "vt", "be"):
        return ProductLine.MAINSTREAM
    if prefix in ("pa", "pq"):
        return ProductLine.PROART
    if prefix in ("vg", "pg", "xg") or "_D" in model:
        return ProductLine.GAMING
    return ProductLine.NONE


def mode_set(line: str) -> list[DisplayMode]:
    if line == ProductLine.PROART:
        return PROART_MODES
    if line in (ProductLine.MAINSTREAM, ProductLine.PORTABLE):
        return MAINSTREAM_MODES
    return GAMING_MODES


def color_temps(line: str, model: str) -> list[ColorTemp]:
    model_u = (model or "").upper()
    if line == ProductLine.PROART:
        return PROART_COLOR_TEMPS
    if line == ProductLine.GAMING:
        return VG_COLOR_TEMPS if "VG" in model_u else GAMING_COLOR_TEMPS
    return VG_COLOR_TEMPS


def display_mode_code(line: str) -> int:
    return VCP_CROSSHAIR if line == ProductLine.PROART else VCP_DISPLAY_MODE


def encode_display_mode(line: str, value: int) -> int:
    """Port of VCPAPI.SetDisplayMode/SetDisplayModeByProduct byte packing."""
    if line == ProductLine.PROART:
        specials = {
            3840, 3841, 3842, 4096, 4097, 4098, 4607, 4863, 4867, 4868,
            5120, 5121, 5122,
        }
        if value not in specials and value < 256:
            return (value << 8) | 0xFF
    return value


# ---------------------------------------------------------------------------
# Capability string parsing - port of DisplayManager.ParseString
# ---------------------------------------------------------------------------
@dataclass
class CapabilityInfo:
    raw: str = ""
    model: str = ""
    mccs_version: str = ""
    vcp: dict[int, list[int]] = field(default_factory=dict)

    def supports(self, code: int) -> bool:
        return code in self.vcp

    def values(self, code: int) -> list[int]:
        return self.vcp.get(code, [])

    @property
    def vcp_count(self) -> int:
        return len(self.vcp)


def _extract_block(text: str, keyword: str) -> str | None:
    idx = text.lower().find(keyword.lower())
    if idx < 0:
        return None
    start = idx + len(keyword)
    if start < len(text) and text[start] == "(":
        depth = 0
        for pos in range(start, len(text)):
            ch = text[pos]
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    return text[start + 1: pos]
    return None


def parse_capabilities(text: str | None) -> CapabilityInfo:
    info = CapabilityInfo()
    if not text:
        return info
    info.raw = text

    model = _extract_block(text, "model")
    if model:
        info.model = clean_model_name(model)

    mccs = _extract_block(text, "mccs_ver")
    if mccs:
        info.mccs_version = mccs.strip()

    block = _extract_block(text, "vcp")
    if block:
        for match in re.finditer(r"([0-9a-fA-F]{2})(?:\(([^)]*)\))?", block):
            code = int(match.group(1), 16)
            values: list[int] = []
            if match.group(2):
                for token in re.split(r"\s+", match.group(2).strip()):
                    if not token:
                        continue
                    try:
                        values.append(int(token, 16))
                    except ValueError:
                        pass
            info.vcp[code] = values
    return info


def firmware_string(current: int, maximum: int) -> str:
    """VCPAPI.GetFWVersion: fwVersion = max, panelID = current & 0xFF."""
    if maximum <= 0:
        return "—"
    return f"{maximum >> 8}.{maximum & 0xFF:02X} (panel 0x{current & 0xFF:02X})"


def vcp_version_string(maximum: int) -> str:
    """VCPAPI.GetVCPVersion: version = max >> 8, revision = max & 0xFF."""
    if maximum <= 0:
        return "—"
    return f"{maximum >> 8}.{maximum & 0xFF}"


def gamma_index(raw: int) -> int | None:
    for idx, (value, _) in enumerate(GAMMA_STEPS):
        if value == raw:
            return idx
    high = (raw >> 8) & 0xFF
    table = {80: 0, 100: 1, 120: 2, 140: 3, 160: 4}
    return table.get(high)
