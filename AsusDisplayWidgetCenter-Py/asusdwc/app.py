"""ASUS DisplayWidget Center - Python/tkinter reconstruction.

The window layout, control set and VCP mappings are reconstructed from the
decompiled .NET application (see README.md / REVERSE_ENGINEERING.md).
"""

from __future__ import annotations

import queue
import time
import tkinter as tk
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from tkinter import filedialog, messagebox, ttk

from . import controller, store, vcp
from .controller import MonitorManager, MonitorState
from .hotkeys import DEFAULT_BINDINGS, HotkeyManager
from .theme import COLORS, apply_theme, dark_title_bar, mono_font, ui_font
from .widgets import (
    Card,
    ChoiceRow,
    ModeCard,
    NavItem,
    PillButton,
    ScrollFrame,
    SectionHeader,
    Segmented,
    SliderRow,
    StatCard,
    ToggleRow,
    draw_icon,
)

APP_TITLE = "ASUS DisplayWidget Center"

PAGE_DEFS = [
    ("overview", "概览", "Overview", "显示器的实时状态与常用调节", "Live status and everyday controls"),
    ("display", "显示与色彩", "Display & Color", "GameVisual 模式、画面与色彩参数", "GameVisual modes, picture and color"),
    ("eyecare", "护眼", "Eye Care", "滤蓝光、色彩增强与休息提醒", "Blue light, color augmentation and breaks"),
    ("gameplus", "游戏辅助", "GamePlus", "准星、计时器、FPS 与对齐", "Crosshair, timer, FPS and alignment"),
    ("osd", "OSD 遥控", "OSD Remote", "直接操作显示器内置 OSD 菜单", "Drive the monitor's built-in OSD"),
    ("profiles", "配置文件", "Profiles", "保存、应用和交换显示配置", "Save, apply and exchange display presets"),
    ("settings", "设置", "Settings", "语言、热键、启动与 DDC 选项", "Language, hotkeys, startup and DDC"),
]


class BasePage:
    key = "overview"
    title_zh = ""
    title_en = ""
    sub_zh = ""
    sub_en = ""

    def __init__(self, app: "App", parent: tk.Misc):
        self.app = app
        self.state = app.state
        self.frame = tk.Frame(parent, bg=COLORS["bg"])
        self.scroll = ScrollFrame(self.frame)
        self.scroll.pack(fill="both", expand=True)
        self.body = tk.Frame(self.scroll.inner, bg=COLORS["bg"])
        self.body.pack(fill="both", expand=True, padx=(26, 12), pady=(18, 26))
        self.build()

    def t(self, zh: str, en: str) -> str:
        return self.app.t(zh, en)

    def build(self) -> None:  # pragma: no cover - overridden
        raise NotImplementedError

    def refresh(self) -> None:  # pragma: no cover - overridden
        pass

    def show(self) -> None:
        self.frame.pack(fill="both", expand=True)
        self.refresh()
        self.scroll.scroll_top()


class OverviewPage(BasePage):
    key = "overview"
    title_zh = "概览"
    title_en = "Overview"
    sub_zh = "显示器的实时状态与常用调节"
    sub_en = "Live status and everyday controls"

    def build(self) -> None:
        self.stats = tk.Frame(self.body, bg=COLORS["bg"])
        self.stats.pack(fill="x")
        self.stat_mode = StatCard(self.stats, self.t("当前模式", "Current mode"), "—")
        self.stat_input = StatCard(self.stats, self.t("输入源", "Input"), "—")
        self.stat_temp = StatCard(self.stats, self.t("色温", "Color temp"), "—")
        self.stat_fw = StatCard(self.stats, self.t("固件", "Firmware"), "—")
        for idx, card in enumerate(
            (self.stat_mode, self.stat_input, self.stat_temp, self.stat_fw)
        ):
            self.stats.grid_columnconfigure(idx, weight=1, uniform="stats")
            card.grid(row=0, column=idx, sticky="nsew", padx=(0 if idx == 0 else 8, 0))

        row = tk.Frame(self.body, bg=COLORS["bg"])
        row.pack(fill="both", expand=True, pady=(16, 0))

        left = Card(row, title=self.t("常用调节", "Quick adjustments"), subtitle=self.t("实时写入显示器", "Written to the monitor live"))
        left.pack(side="left", fill="both", expand=True)
        self.sl_brightness = SliderRow(
            left.body, self.t("亮度", "Brightness"), 0, 100,
            command=lambda v: self.app.set_vcp(vcp.VCP_BRIGHTNESS, v, "亮度", "Brightness"),
            suffix="%",
        )
        self.sl_brightness.pack(fill="x", pady=(0, 12))
        self.sl_contrast = SliderRow(
            left.body, self.t("对比度", "Contrast"), 0, 100,
            command=lambda v: self.app.set_vcp(vcp.VCP_CONTRAST, v, "对比度", "Contrast"),
            suffix="%",
        )
        self.sl_contrast.pack(fill="x", pady=(0, 12))
        self.sl_volume = SliderRow(
            left.body, self.t("音量", "Volume"), 0, 100,
            command=lambda v: self.app.set_vcp(vcp.VCP_VOLUME, v, "音量", "Volume"),
            suffix="%",
        )
        self.sl_volume.pack(fill="x")

        right = tk.Frame(row, bg=COLORS["bg"], width=306)
        right.pack(side="right", fill="y", padx=(16, 0))
        right.pack_propagate(False)
        scenes = Card(right, title=self.t("快速场景", "Quick scenes"), subtitle=self.t("一键组合", "One click"))
        scenes.pack(fill="x")
        self.scene_buttons = {}
        for key, zh, en, desc_zh, desc_en in [
            ("movie", "观影", "Movie", "影院模式 + 暖色", "Cinema + warm"),
            ("game", "游戏", "Gaming", "FPS 模式 + 响应", "FPS mode + overdrive"),
            ("reading", "阅读", "Reading", "滤蓝光 + 阅读", "Blue light + reading"),
            ("night", "夜间", "Night", "低蓝光 + 夜间", "Low blue light + night"),
        ]:
            btn = tk.Frame(
                scenes.body, bg=COLORS["panel"], highlightbackground=COLORS["border"],
                highlightthickness=1, cursor="hand2",
            )
            btn.pack(fill="x", pady=(0, 6))
            inner = tk.Frame(btn, bg=COLORS["panel"])
            inner.pack(fill="x", padx=12, pady=9)
            title = tk.Label(inner, text=self.t(zh, en), bg=COLORS["panel"], fg=COLORS["text"], font=ui_font(10, "bold"))
            title.pack(side="left")
            sub = tk.Label(inner, text=self.t(desc_zh, desc_en), bg=COLORS["panel"], fg=COLORS["muted"], font=ui_font(8))
            sub.pack(side="right")
            for widget in (btn, inner, title, sub):
                widget.bind("<Button-1>", lambda e, k=key: self._apply_scene(k))
                widget.bind("<Enter>", lambda e, b=btn, i=inner, t=title, s=sub: self._scene_hover(b, i, t, s, True))
                widget.bind("<Leave>", lambda e, b=btn, i=inner, t=title, s=sub: self._scene_hover(b, i, t, s, False))
            self.scene_buttons[key] = btn

        info = Card(self.body, title=self.t("显示器信息", "Monitor information"))
        info.pack(fill="x", pady=(16, 0))
        grid = tk.Frame(info.body, bg=COLORS["card"])
        grid.pack(fill="x")
        self.info_labels = {}
        fields = [
            ("model", "型号", "Model"),
            ("device", "设备", "Device"),
            ("resolution", "分辨率", "Resolution"),
            ("primary", "主显示器", "Primary"),
            ("vcpver", "VCP 版本", "VCP version"),
            ("firmware", "固件", "Firmware"),
            ("commands", "支持命令", "Commands"),
            ("status", "DDC/CI", "DDC/CI"),
        ]
        for idx, (key, zh, en) in enumerate(fields):
            col = idx % 2
            row_idx = idx // 2
            cell = tk.Frame(grid, bg=COLORS["card"])
            cell.grid(row=row_idx, column=col, sticky="w", padx=(0, 60), pady=6)
            tk.Label(cell, text=self.t(zh, en), bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8)).pack(anchor="w")
            value = tk.Label(cell, text="—", bg=COLORS["card"], fg=COLORS["text"], font=ui_font(10))
            value.pack(anchor="w", pady=(1, 0))
            self.info_labels[key] = value
        for col in range(2):
            grid.grid_columnconfigure(col, weight=1)

        inputs = Card(self.body, title=self.t("输入源", "Input source"), subtitle=self.t("MCCS 0x60", "MCCS 0x60"))
        inputs.pack(fill="x", pady=(16, 0))
        self.input_seg = Segmented(inputs.body, command=self._set_input, columns=4)
        self.input_seg.pack(fill="x")
        self.input_hint = tk.Label(
            inputs.body, text=self.t("切换输入源会让画面短暂黑屏，这是正常现象。", "Switching input blanks the screen briefly; this is normal."),
            bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8),
        )
        self.input_hint.pack(anchor="w", pady=(8, 0))

    def _scene_hover(self, frame, inner, title, sub, entering):
        bg = COLORS["card_hover"] if entering else COLORS["panel"]
        for widget in (frame, inner, title, sub):
            widget.configure(bg=bg)

    def _apply_scene(self, key):
        state = self.state
        pairs: list[tuple[int, int]] = []
        mode_code = vcp.display_mode_code(state.line)

        def mode_value(name):
            for item in vcp.mode_set(state.line):
                if item.key == name:
                    return item.value
            return None

        if key == "movie":
            value = mode_value("cinema") or mode_value("theater")
            if value is not None:
                pairs.append((mode_code, vcp.encode_display_mode(state.line, value)))
            pairs.append((vcp.VCP_BLUE_LIGHT, 1))
        elif key == "game":
            value = mode_value("fps") or mode_value("racing") or mode_value("game")
            if value is not None:
                pairs.append((mode_code, vcp.encode_display_mode(state.line, value)))
            pairs.append((vcp.VCP_BLUE_LIGHT, 0))
            if state.supported(vcp.VCP_SHADOW_BOOST):
                pairs.append((vcp.VCP_SHADOW_BOOST, 2))
        elif key == "reading":
            value = mode_value("reading")
            if value is not None:
                pairs.append((mode_code, vcp.encode_display_mode(state.line, value)))
            pairs.append((vcp.VCP_BLUE_LIGHT, 3))
        else:
            value = mode_value("night") or mode_value("darkroom")
            if value is not None:
                pairs.append((mode_code, vcp.encode_display_mode(state.line, value)))
            pairs.append((vcp.VCP_BLUE_LIGHT, 4))
        self.app.write_many(pairs, self.t("应用场景", "Apply scene"), self.t("场景已应用", "Scene applied"))

    def _set_input(self, value):
        self.app.set_vcp(vcp.VCP_INPUT, value, "输入源", "Input source")

    def refresh(self) -> None:
        state = self.state
        mode = state.current_mode()
        self.stat_mode.set_value(self.t(mode.zh, mode.en) if mode else "—",
                                 self.t("GameVisual", "GameVisual"))
        self.stat_input.set_value(state.input_name(), "MCCS 0x60")
        self.stat_temp.set_value(state.color_temp_name(self.app.lang), "MCCS 0x14")
        self.stat_fw.set_value(state.firmware(), "MCCS 0xFF")

        self.sl_brightness.set_range(0, state.maximum(vcp.VCP_BRIGHTNESS) or 100)
        self.sl_brightness.set_value(state.value(vcp.VCP_BRIGHTNESS) or 0)
        self.sl_brightness.set_enabled(state.supported(vcp.VCP_BRIGHTNESS))
        self.sl_contrast.set_range(0, state.maximum(vcp.VCP_CONTRAST) or 100)
        self.sl_contrast.set_value(state.value(vcp.VCP_CONTRAST) or 0)
        self.sl_contrast.set_enabled(state.supported(vcp.VCP_CONTRAST))
        self.sl_volume.set_range(0, state.maximum(vcp.VCP_VOLUME) or 100)
        self.sl_volume.set_value(state.value(vcp.VCP_VOLUME) or 0)
        self.sl_volume.set_enabled(state.supported(vcp.VCP_VOLUME))

        options = []
        values = state.caps.values(vcp.VCP_INPUT)
        for value in values or [15, 17, 18, 26]:
            options.append((vcp.INPUT_SOURCES.get(value, f"0x{value:02X}"), value))
        self.input_seg.set_options(options, state.value(vcp.VCP_INPUT))
        self.input_seg.set_enabled(state.supported(vcp.VCP_INPUT))

        self.info_labels["model"].configure(text=state.model or "—")
        self.info_labels["device"].configure(text=state.device_short or "—")
        self.info_labels["resolution"].configure(text=state.resolution)
        self.info_labels["primary"].configure(text=self.t("是", "Yes") if state.handle.primary else self.t("否", "No"))
        self.info_labels["vcpver"].configure(text=state.vcp_version())
        self.info_labels["firmware"].configure(text=state.firmware())
        self.info_labels["commands"].configure(text=f"{state.caps.vcp_count} VCP")
        self.info_labels["status"].configure(
            text=self.t("正常", "OK") if state.ddc_ok else self.t("无响应", "No response"),
            fg=COLORS["green"] if state.ddc_ok else COLORS["red"],
        )


class DisplayPage(BasePage):
    key = "display"
    title_zh = "显示与色彩"
    title_en = "Display & Color"
    sub_zh = "GameVisual 模式、画面与色彩参数"
    sub_en = "GameVisual modes, picture and color"

    def build(self) -> None:
        header = SectionHeader(
            self.body,
            self.t("GameVisual 模式", "GameVisual modes"),
            self.t("写入 MCCS 0xDC / 0xE3", "Written through MCCS 0xDC / 0xE3"),
            right_text=self.t("点击卡片切换", "Click a card to switch"),
        )
        header.pack(fill="x")
        self.mode_grid = tk.Frame(self.body, bg=COLORS["bg"])
        self.mode_grid.pack(fill="x", pady=(12, 0))
        self.mode_cards: dict[int, ModeCard] = {}
        modes = vcp.mode_set(self.state.line)
        for idx, mode in enumerate(modes):
            card = ModeCard(
                self.mode_grid,
                self.t(mode.zh, mode.en),
                mode.en.upper(),
                mode.value,
                self._set_mode,
            )
            card.grid(row=idx // 4, column=idx % 4, sticky="nsew", padx=(0 if idx % 4 == 0 else 6, 0), pady=(0, 6))
            self.mode_cards[mode.value] = card
        for col in range(4):
            self.mode_grid.grid_columnconfigure(col, weight=1, uniform="modes")

        two = tk.Frame(self.body, bg=COLORS["bg"])
        two.pack(fill="both", expand=True, pady=(16, 0))
        left = Card(two, title=self.t("画面", "Picture"))
        left.pack(side="left", fill="both", expand=True)
        right = tk.Frame(two, bg=COLORS["bg"], width=360)
        right.pack(side="right", fill="both", padx=(16, 0))
        right.pack_propagate(False)

        self.sl_brightness = self._slider(left.body, "亮度", "Brightness", vcp.VCP_BRIGHTNESS)
        self.sl_contrast = self._slider(left.body, "对比度", "Contrast", vcp.VCP_CONTRAST)
        self.sl_sharpness = self._slider(left.body, "锐度", "Sharpness", vcp.VCP_SHARPNESS)
        self.sl_saturation = self._slider(left.body, "饱和度", "Saturation", vcp.VCP_SATURATION)
        self.sl_hue = self._slider(left.body, "色调", "Hue", vcp.VCP_HUE)
        self.gamma = ChoiceRow(
            left.body, self.t("伽马", "Gamma"),
            self._gamma_options(),
            command=lambda v: self.app.set_vcp(vcp.VCP_GAMMA, v, "伽马", "Gamma"),
            hint=self.t("ASUS 16 位伽马值（0x5000 – 0xA000）", "ASUS 16-bit gamma values (0x5000 - 0xA000)"),
        )
        self.gamma.pack(fill="x", pady=(10, 0))

        color = Card(right, title=self.t("色彩", "Color"))
        color.pack(fill="x")
        self.color_temp = ChoiceRow(
            color.body, self.t("色温", "Color temperature"),
            self._temp_options(),
            command=lambda v: self.app.set_vcp(vcp.VCP_COLOR_TEMP, v, "色温", "Color temperature", delay_refresh=450),
        )
        self.color_temp.pack(fill="x", pady=(0, 10))
        self.sl_red = self._slider(color.body, "红色增益", "Red gain", vcp.VCP_RED_GAIN)
        self.sl_green = self._slider(color.body, "绿色增益", "Green gain", vcp.VCP_GREEN_GAIN)
        self.sl_blue = self._slider(color.body, "蓝色增益", "Blue gain", vcp.VCP_BLUE_GAIN)
        self.color_hint = tk.Label(
            color.body,
            text=self.t("仅在色温为“用户”时可调", "Editable when color temperature is User"),
            bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8), wraplength=300, justify="left",
        )
        self.color_hint.pack(anchor="w", pady=(6, 0))

        enhance = Card(self.body, title=self.t("画面增强", "Enhancement"))
        enhance.pack(fill="x", pady=(16, 0))
        row = tk.Frame(enhance.body, bg=COLORS["card"])
        row.pack(fill="x")
        self.sl_overdrive = SliderRow(
            row, self.t("响应速度 (OverDrive)", "OverDrive"), 0, 20,
            command=lambda v: self.app.set_vcp(vcp.VCP_OVERDRIVE, v, "响应速度", "OverDrive"),
        )
        self.sl_overdrive.pack(side="left", fill="x", expand=True, padx=(0, 24))
        right_box = tk.Frame(row, bg=COLORS["card"])
        right_box.pack(side="right", fill="x", expand=True)
        self.shadow = Segmented(
            right_box,
            [(self.t("关", "Off"), 0), (self.t("低", "Low"), 1), (self.t("中", "Mid"), 2),
             (self.t("高", "High"), 3), (self.t("动态", "Dynamic"), 4)],
            command=lambda v: self.app.set_vcp(vcp.VCP_SHADOW_BOOST, v, "暗部增强", "Shadow Boost"),
            small=True,
        )
        self.shadow.pack(anchor="w")
        tk.Label(
            right_box, text=self.t("暗部增强 Shadow Boost", "Shadow Boost"), bg=COLORS["card"],
            fg=COLORS["muted"], font=ui_font(8),
        ).pack(anchor="w", pady=(5, 0))

        toggles = tk.Frame(enhance.body, bg=COLORS["card"])
        toggles.pack(fill="x", pady=(14, 0))
        self.tg_ascr = ToggleRow(
            toggles, self.t("ASCR 动态对比度", "ASCR dynamic contrast"),
            command=self._set_ascr,
            hint=self.t("新 VCP：0xFD bit 1；旧 VCP：0xE1", "New VCP: 0xFD bit 1; legacy: 0xE1"),
        )
        self.tg_ascr.pack(side="left", fill="x", expand=True, padx=(0, 24))
        self.tg_power = ToggleRow(
            toggles, self.t("省电模式", "Power saving"),
            command=self._set_power,
        )
        self.tg_power.pack(side="right", fill="x", expand=True)

        audio = Card(self.body, title=self.t("输入与音频", "Input & audio"))
        audio.pack(fill="x", pady=(16, 0))
        audio_row = tk.Frame(audio.body, bg=COLORS["card"])
        audio_row.pack(fill="x")
        self.input_choice = ChoiceRow(
            audio_row, self.t("输入源", "Input source"),
            command=lambda v: self.app.set_vcp(vcp.VCP_INPUT, v, "输入源", "Input source"),
        )
        self.input_choice.pack(side="left", fill="x", expand=True, padx=(0, 30))
        self.volume = SliderRow(
            audio_row, self.t("音量", "Volume"), 0, 100,
            command=lambda v: self.app.set_vcp(vcp.VCP_VOLUME, v, "音量", "Volume"), suffix="%",
        )
        self.volume.pack(side="right", fill="x", expand=True)
        self.tg_mute = ToggleRow(
            audio.body, self.t("静音", "Mute"),
            command=lambda v: self.app.toggle_bit(vcp.VCP_TOGGLES1, vcp.MUTE_BIT, v, "静音"),
            hint=self.t("MCCS 0xFC 的 bit 3", "Bit 3 of MCCS 0xFC"),
        )
        self.tg_mute.pack(fill="x", pady=(10, 0))

    def _slider(self, parent, zh, en, code):
        widget = SliderRow(
            parent, self.t(zh, en), 0, self.state.maximum(code) or 100,
            command=lambda v, c=code, z=zh, e=en: self.app.set_vcp(c, v, z, e),
            suffix="%",
        )
        widget.pack(fill="x", pady=(0, 10))
        return widget

    def _temp_options(self):
        advertised = self.state.caps.values(vcp.VCP_COLOR_TEMP)
        items = vcp.color_temps(self.state.line, self.state.model)
        if advertised:
            items = [item for item in items if item.value in advertised]
        return [(vcp.color_temp_label(item, self.app.lang), item.value) for item in items]

    def _gamma_options(self):
        advertised = self.state.caps.values(vcp.VCP_GAMMA)
        known = {value for value, _ in vcp.GAMMA_STEPS}
        if advertised and any(value in known for value in advertised):
            return [(label, value) for value, label in vcp.GAMMA_STEPS if value in advertised]
        return [(label, value) for value, label in vcp.GAMMA_STEPS]

    def _ascr_code(self):
        return vcp.VCP_TOGGLES2 if self.state.is_new_vcp() else vcp.VCP_POWER_SAVING

    def _set_ascr(self, enabled):
        if self.state.is_new_vcp():
            self.app.toggle_bit(vcp.VCP_TOGGLES2, vcp.TOGGLE_ASCR, enabled, "ASCR")
        else:
            self.app.set_vcp(vcp.VCP_POWER_SAVING, 1 if enabled else 0, "ASCR", "ASCR")

    def _set_power(self, enabled):
        code = vcp.VCP_POWER_SAVING if self.state.is_new_vcp() else vcp.VCP_POWER_SAVING_LEGACY
        self.app.set_vcp(code, 1 if enabled else 0, "省电模式", "Power saving")

    def _set_mode(self, value):
        mode = next((m for m in vcp.mode_set(self.state.line) if m.value == value), None)
        if mode:
            self.app.set_mode(mode)

    def refresh(self) -> None:
        state = self.state
        current = state.current_mode()
        for value, card in self.mode_cards.items():
            card.set_active(bool(current and current.value == value))
            card.set_enabled(self._mode_supported(value))

        for widget, code in (
            (self.sl_brightness, vcp.VCP_BRIGHTNESS),
            (self.sl_contrast, vcp.VCP_CONTRAST),
            (self.sl_sharpness, vcp.VCP_SHARPNESS),
            (self.sl_saturation, vcp.VCP_SATURATION),
            (self.sl_hue, vcp.VCP_HUE),
        ):
            widget.set_range(0, state.maximum(code) or 100)
            widget.set_value(state.value(code) or 0)
            widget.set_enabled(state.supported(code))

        gamma_value = state.value(vcp.VCP_GAMMA) or 0
        idx = vcp.gamma_index(gamma_value)
        self.gamma.set_options(self._gamma_options(), vcp.GAMMA_STEPS[idx][0] if idx is not None else None)
        self.gamma.set_enabled(state.supported(vcp.VCP_GAMMA))

        self.color_temp.set_options(self._temp_options(), state.value(vcp.VCP_COLOR_TEMP))
        self.color_temp.set_enabled(state.supported(vcp.VCP_COLOR_TEMP))
        user_temp = state.value(vcp.VCP_COLOR_TEMP) == 11
        for widget, code in (
            (self.sl_red, vcp.VCP_RED_GAIN),
            (self.sl_green, vcp.VCP_GREEN_GAIN),
            (self.sl_blue, vcp.VCP_BLUE_GAIN),
        ):
            widget.set_range(0, state.maximum(code) or 100)
            widget.set_value(state.value(code) or 0)
            widget.set_enabled(state.supported(code) and user_temp)

        self.sl_overdrive.set_range(0, state.maximum(vcp.VCP_OVERDRIVE) or 20)
        self.sl_overdrive.set_value(state.value(vcp.VCP_OVERDRIVE) or 0)
        self.sl_overdrive.set_enabled(state.supported(vcp.VCP_OVERDRIVE))
        self.shadow.set_value(state.value(vcp.VCP_SHADOW_BOOST) or 0)
        self.shadow.set_enabled(state.supported(vcp.VCP_SHADOW_BOOST))
        if state.is_new_vcp():
            ascr_on = state.is_on(vcp.VCP_TOGGLES2, vcp.TOGGLE_ASCR)
            ascr_ok = state.supported(vcp.VCP_TOGGLES2)
            power_code = vcp.VCP_POWER_SAVING
        else:
            ascr_on = bool(state.value(vcp.VCP_POWER_SAVING))
            ascr_ok = state.supported(vcp.VCP_POWER_SAVING)
            power_code = vcp.VCP_POWER_SAVING_LEGACY
        self.tg_ascr.set_state(ascr_on)
        self.tg_ascr.set_enabled(ascr_ok)
        self.tg_power.set_state(bool(state.value(power_code)))
        self.tg_power.set_enabled(state.supported(power_code))

        values = state.caps.values(vcp.VCP_INPUT)
        self.input_choice.set_options(
            [(vcp.INPUT_SOURCES.get(value, f"0x{value:02X}"), value) for value in values]
            or [(vcp.INPUT_SOURCES.get(value, f"0x{value:02X}"), value) for value in (15, 17, 18, 26)],
            state.value(vcp.VCP_INPUT),
        )
        self.input_choice.set_enabled(state.supported(vcp.VCP_INPUT))
        self.volume.set_range(0, state.maximum(vcp.VCP_VOLUME) or 100)
        self.volume.set_value(state.value(vcp.VCP_VOLUME) or 0)
        self.volume.set_enabled(state.supported(vcp.VCP_VOLUME))
        self.tg_mute.set_state(state.is_on(vcp.VCP_TOGGLES1, vcp.MUTE_BIT))
        self.tg_mute.set_enabled(state.supported(vcp.VCP_TOGGLES1))

    def _mode_supported(self, value) -> bool:
        values = self.state.caps.values(vcp.display_mode_code(self.state.line))
        if not values:
            return True
        return value in values or (value << 8) in values or (value << 8 | 0xFF) in values


class EyeCarePage(BasePage):
    key = "eyecare"
    title_zh = "护眼"
    title_en = "Eye Care"
    sub_zh = "滤蓝光、色彩增强与休息提醒"
    sub_en = "Blue light, color augmentation and breaks"

    def build(self) -> None:
        hero = Card(self.body, title=self.t("滤蓝光", "Blue light filter"), subtitle="MCCS 0xE6")
        hero.pack(fill="x")
        top = tk.Frame(hero.body, bg=COLORS["card"])
        top.pack(fill="x")
        self.blue_value = tk.Label(
            top, text="0", bg=COLORS["card"], fg=COLORS["accent"], font=ui_font(40, "bold")
        )
        self.blue_value.pack(side="left")
        self.blue_max = tk.Label(
            top, text="/ 4", bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(12)
        )
        self.blue_max.pack(side="left", padx=(6, 0), pady=(18, 0))
        self.blue_desc = tk.Label(
            top,
            text=self.t("级别越高，蓝光越少，画面越暖。", "Higher levels remove more blue light and warm the picture."),
            bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(9),
        )
        self.blue_desc.pack(side="right", pady=(20, 0))

        self.blue_slider = SliderRow(
            hero.body, self.t("级别", "Level"), 0, 4,
            command=lambda v: self.app.set_vcp(vcp.VCP_BLUE_LIGHT, v, "滤蓝光", "Blue light"),
        )
        self.blue_slider.pack(fill="x", pady=(6, 10))
        self.blue_presets = Segmented(
            hero.body,
            [(self.t("关闭", "Off"), 0), (self.t("低", "Low"), 1), (self.t("中", "Mid"), 2),
             (self.t("高", "High"), 3), (self.t("最高", "Max"), 4)],
            command=lambda v: self.app.set_vcp(vcp.VCP_BLUE_LIGHT, v, "滤蓝光", "Blue light"),
            columns=5,
        )
        self.blue_presets.pack(fill="x")

        color = Card(
            self.body, title=self.t("色彩增强", "Color augmentation"),
            subtitle=self.t("六轴饱和度 · MCCS 0x59 – 0x5E", "Six-axis saturation · MCCS 0x59 - 0x5E"),
        )
        color.pack(fill="x", pady=(16, 0))
        self.sat_sliders = {}
        for code, zh, en in [
            (vcp.VCP_RED_SAT, "红色", "Red"),
            (vcp.VCP_YELLOW_SAT, "黄色", "Yellow"),
            (vcp.VCP_GREEN_SAT, "绿色", "Green"),
            (vcp.VCP_BLUE_SAT, "蓝色", "Blue"),
        ]:
            slider = SliderRow(
                color.body, self.t(zh, en), 0, 100,
                command=lambda v, c=code, z=zh, e=en: self.app.set_vcp(c, v, z, e),
                suffix="%",
            )
            slider.pack(fill="x", pady=(0, 8))
            self.sat_sliders[code] = slider

        rest = Card(self.body, title=self.t("休息提醒", "Rest reminder"), subtitle="MCCS 0xED")
        rest.pack(fill="x", pady=(16, 0))
        self.rest_choice = ChoiceRow(
            rest.body,
            self.t("提醒间隔", "Reminder interval"),
            [(self.t("关闭", "Off"), 0)]
            + [(f"{minutes} {self.t('分钟', 'min')}", idx) for idx, minutes in enumerate(range(5, 65, 5), start=1)],
            command=lambda v: self.app.set_vcp(vcp.VCP_REST_REMINDER, v, "休息提醒", "Rest reminder"),
            hint=self.t("由显示器固件计时并弹出提醒，不依赖电脑运行。", "Timed by the monitor firmware, independent of the PC."),
        )
        self.rest_choice.pack(fill="x")

        tip = Card(self.body, title=self.t("使用建议", "Tips"))
        tip.pack(fill="x", pady=(16, 0))
        tk.Label(
            tip.body,
            text=self.t(
                "滤蓝光会改变白点，色彩工作请关闭。若显示器处于 HDR 模式或 sRGB 模式，"
                "部分厂商选项可能被固件锁定，属正常现象。",
                "Blue light filtering shifts the white point - turn it off for color work. "
                "Some vendor options are locked by firmware in HDR or sRGB mode.",
            ),
            bg=COLORS["card"], fg=COLORS["text_dim"], font=ui_font(9), justify="left", wraplength=760,
        ).pack(anchor="w")

    def refresh(self) -> None:
        state = self.state
        maximum = state.maximum(vcp.VCP_BLUE_LIGHT) or 4
        maximum = max(1, min(10, maximum))
        self.blue_slider.set_range(0, maximum)
        self.blue_slider.set_value(state.value(vcp.VCP_BLUE_LIGHT) or 0)
        self.blue_slider.set_enabled(state.supported(vcp.VCP_BLUE_LIGHT))
        self.blue_value.configure(text=str(state.value(vcp.VCP_BLUE_LIGHT) or 0))
        self.blue_max.configure(text=f"/ {maximum}")
        self.blue_presets.set_enabled(state.supported(vcp.VCP_BLUE_LIGHT))
        self.blue_presets.set_value(state.value(vcp.VCP_BLUE_LIGHT) or 0)

        for code, slider in self.sat_sliders.items():
            slider.set_range(0, state.maximum(code) or 100)
            slider.set_value(state.value(code) or 0)
            slider.set_enabled(state.supported(code))

        supported = state.supported(vcp.VCP_REST_REMINDER) and (state.maximum(vcp.VCP_REST_REMINDER) or 0) > 0
        self.rest_choice.set_enabled(supported)
        if supported:
            self.rest_choice.set_value(state.value(vcp.VCP_REST_REMINDER) or 0)


class GamePlusPage(BasePage):
    key = "gameplus"
    title_zh = "游戏辅助"
    title_en = "GamePlus"
    sub_zh = "准星、计时器、FPS 与对齐"
    sub_en = "Crosshair, timer, FPS and alignment"

    def build(self) -> None:
        cross = Card(self.body, title=self.t("准星", "Crosshair"), subtitle="MCCS 0xE3")
        cross.pack(fill="x")
        grid = tk.Frame(cross.body, bg=COLORS["card"])
        grid.pack(fill="x")
        self.cross_cards = {}
        if self.state.line == vcp.ProductLine.PROART:
            # On ProArt panels VCP 0xE3 selects the display mode, not a crosshair.
            styles = {}
            tk.Label(
                cross.body,
                text=self.t("ProArt 机型没有游戏准星功能（0xE3 用于专业显示模式）。",
                            "ProArt panels have no crosshair (0xE3 selects pro display modes)."),
                bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(9), justify="left",
            ).pack(anchor="w", pady=8)
        else:
            styles = vcp.crosshair_styles(self.state.caps.values(vcp.VCP_CROSSHAIR))
        for idx, (value, name) in enumerate(styles.items()):
            cell = tk.Frame(
                grid, bg=COLORS["panel"], highlightbackground=COLORS["border"],
                highlightthickness=1, cursor="hand2",
            )
            cell.grid(row=idx // 5, column=idx % 5, sticky="nsew", padx=(0 if idx % 5 == 0 else 6, 0), pady=6)
            preview = tk.Canvas(cell, width=54, height=34, bg=COLORS["panel"], highlightthickness=0)
            preview.pack(pady=(7, 0))
            label = tk.Label(cell, text=self.t(name, self._en_crosshair(value)), bg=COLORS["panel"], fg=COLORS["text_dim"], font=ui_font(8))
            label.pack(pady=(1, 7))
            self._draw_crosshair(preview, value)
            for widget in (cell, preview, label):
                widget.bind("<Button-1>", lambda e, v=value: self.app.set_vcp(vcp.VCP_CROSSHAIR, v, "准星", "Crosshair"))
                widget.bind("<Enter>", lambda e, c=cell, p=preview, l=label: self._hover(c, p, l, True))
                widget.bind("<Leave>", lambda e, c=cell, p=preview, l=label: self._hover(c, p, l, False))
            self.cross_cards[value] = (cell, preview, label)
        for col in range(5):
            grid.grid_columnconfigure(col, weight=1, uniform="cross")

        row = tk.Frame(self.body, bg=COLORS["bg"])
        row.pack(fill="x", pady=(16, 0))
        timer = Card(row, title=self.t("计时器", "Timer"), subtitle="MCCS 0xE4")
        timer.pack(side="left", fill="both", expand=True)
        self.timer_seg = Segmented(
            timer.body,
            [(label, value) for value, label in vcp.TIMER_VALUES.items()],
            command=lambda v: self.app.set_vcp(vcp.VCP_GAMEPLUS_TIMER, v, "计时器", "Timer"),
            columns=3,
        )
        self.timer_seg.pack(fill="x")

        fps = Card(row, title=self.t("FPS 计数器", "FPS counter"), subtitle="MCCS 0xEA")
        fps.pack(side="right", fill="both", expand=True, padx=(16, 0))
        self.fps_seg = Segmented(
            fps.body,
            [(label, value) for value, label in vcp.FPS_VALUES.items()],
            command=lambda v: self.app.set_vcp(vcp.VCP_FPS_COUNTER, v, "FPS 计数器", "FPS counter"),
            columns=3,
        )
        self.fps_seg.pack(fill="x")

        align = Card(self.body, title=self.t("显示对齐", "Display alignment"), subtitle="MCCS 0xE7 / 0xE8")
        align.pack(fill="x", pady=(16, 0))
        self.tg_align = ToggleRow(
            align.body, self.t("对齐辅助线", "Alignment grid"),
            command=lambda v: self.app.set_vcp(vcp.VCP_ALIGNMENT, 1 if v else 0, "显示对齐", "Display alignment"),
        )
        self.tg_align.pack(fill="x")
        self.pos_seg = Segmented(
            align.body,
            [(self.t(zh, en), value) for value, zh, en in [
                (1, "左上", "Top left"), (2, "右上", "Top right"),
                (3, "左下", "Bottom left"), (4, "右下", "Bottom right"),
            ]],
            command=lambda v: self.app.set_vcp(vcp.VCP_GAMEPLUS_POSITION, v, "游戏辅助位置", "GamePlus position"),
            columns=4,
        )
        self.pos_seg.pack(fill="x", pady=(10, 0))

    def _draw_crosshair(self, canvas, value):
        canvas.delete("all")
        if value == 0:
            canvas.create_line(20, 17, 34, 17, fill=COLORS["muted"], width=1)
            return
        colors = {1: "#FF4D5E", 2: "#3DDC97", 3: "#FF4D5E", 4: "#3DDC97", 5: "#FF4D5E",
                  6: "#3DDC97", 7: "#39C6FF", 8: "#3DDC97", 9: "#39C6FF", 10: "#3DDC97",
                  11: "#39C6FF", 12: "#3DDC97"}
        color = colors.get(value, COLORS["accent"])
        cx, cy = 27, 17
        style = value
        if style in (1, 2, 7, 8):
            canvas.create_oval(cx - 2, cy - 2, cx + 2, cy + 2, fill=color, outline=color)
        elif style in (3, 4):
            canvas.create_oval(cx - 7, cy - 7, cx + 7, cy + 7, outline=color, width=2)
        elif style in (5, 6):
            canvas.create_line(cx - 9, cy, cx - 3, cy, fill=color, width=2)
            canvas.create_line(cx + 3, cy, cx + 9, cy, fill=color, width=2)
            canvas.create_line(cx, cy - 9, cx, cy - 3, fill=color, width=2)
            canvas.create_line(cx, cy + 3, cx, cy + 9, fill=color, width=2)
        elif style in (9, 10):
            canvas.create_oval(cx - 8, cy - 8, cx + 8, cy + 8, outline=color, width=2)
            canvas.create_oval(cx - 2, cy - 2, cx + 2, cy + 2, fill=color, outline=color)
        elif style in (11, 12):
            canvas.create_oval(cx - 8, cy - 8, cx + 8, cy + 8, outline=color, width=1)
            canvas.create_line(cx - 9, cy, cx + 9, cy, fill=color, width=2)
            canvas.create_line(cx, cy - 9, cx, cy + 9, fill=color, width=2)

    def _hover(self, cell, preview, label, entering):
        bg = COLORS["card_hover"] if entering else COLORS["panel"]
        for widget in (cell, preview, label):
            widget.configure(bg=bg)

    @staticmethod
    def _en_crosshair(value):
        return {
            0: "Off", 1: "Red point", 2: "Green point", 3: "Red ring", 4: "Green ring",
            5: "Red cross", 6: "Green cross", 7: "Blue point", 8: "Green dot",
            9: "Blue target", 10: "Green target", 11: "Blue cross", 12: "Green cross",
        }.get(value, str(value))

    def refresh(self) -> None:
        state = self.state
        current = state.value(vcp.VCP_CROSSHAIR)
        for value, (cell, preview, label) in self.cross_cards.items():
            active = value == current
            cell.configure(
                bg=COLORS["card_active"] if active else COLORS["panel"],
                highlightbackground=COLORS["accent"] if active else COLORS["border"],
            )
            preview.configure(bg=COLORS["card_active"] if active else COLORS["panel"])
            label.configure(bg=COLORS["card_active"] if active else COLORS["panel"],
                            fg=COLORS["white"] if active else COLORS["text_dim"])
            self._draw_crosshair(preview, value)
        self.timer_seg.set_value(state.value(vcp.VCP_GAMEPLUS_TIMER) or 0)
        self.timer_seg.set_enabled(state.supported(vcp.VCP_GAMEPLUS_TIMER))
        self.fps_seg.set_value(state.value(vcp.VCP_FPS_COUNTER) or 0)
        self.fps_seg.set_enabled(state.supported(vcp.VCP_FPS_COUNTER))
        self.tg_align.set_state(bool(state.value(vcp.VCP_ALIGNMENT)))
        self.tg_align.set_enabled(state.supported(vcp.VCP_ALIGNMENT))
        self.pos_seg.set_value(state.value(vcp.VCP_GAMEPLUS_POSITION) or 1)
        self.pos_seg.set_enabled(state.supported(vcp.VCP_GAMEPLUS_POSITION))


class OSDPage(BasePage):
    key = "osd"
    title_zh = "OSD 遥控"
    title_en = "OSD Remote"
    sub_zh = "直接操作显示器内置 OSD 菜单"
    sub_en = "Drive the monitor's built-in OSD"

    OPERATIONS = [
        (0, "关闭", "Close", "Close"),
        (1, "菜单", "Menu", "Menu"),
        (2, "上", "Up", "Up"),
        (3, "下", "Down", "Down"),
        (4, "右", "Right", "Right"),
        (5, "左", "Left", "Left"),
        (6, "确认", "Enter", "Enter"),
        (7, "返回", "Back", "Back"),
        (8, "输入选择", "Input select", "Input"),
        (9, "QuickFit", "QuickFit", "QuickFit"),
    ]

    def build(self) -> None:
        card = Card(self.body, title=self.t("遥控面板", "Remote panel"), subtitle="MCCS 0xEB")
        card.pack(fill="x")
        grid = tk.Frame(card.body, bg=COLORS["card"])
        grid.pack(pady=6)
        positions = {
            (0, 1): ("▲", 2), (1, 0): ("◀", 5), (1, 1): ("OK", 6),
            (1, 2): ("▶", 4), (2, 1): ("▼", 3),
        }
        for (row, col), (text, op) in positions.items():
            btn = tk.Label(
                grid, text=text, bg=COLORS["panel"], fg=COLORS["text"], width=6, height=3,
                font=ui_font(13 if text in "▲▼◀▶" else 11, "bold"), cursor="hand2",
            )
            btn.grid(row=row, column=col, padx=4, pady=4)
            btn.bind("<Button-1>", lambda e, o=op: self._send(o))
            btn.bind("<Enter>", lambda e, b=btn: b.configure(bg=COLORS["card_hover"]))
            btn.bind("<Leave>", lambda e, b=btn: b.configure(bg=COLORS["panel"]))

        extra = tk.Frame(card.body, bg=COLORS["card"])
        extra.pack(fill="x", pady=(12, 0))
        for op, zh, en, _ in self.OPERATIONS:
            if op in (2, 3, 4, 5, 6):
                continue
            button = PillButton(extra, self.t(zh, en), command=lambda o=op: self._send(o))
            button.pack(side="left", padx=(0, 8), pady=3)
            setattr(self, f"btn_{op}", button)

        lang = Card(self.body, title=self.t("OSD 语言", "OSD language"), subtitle="MCCS 0xCC")
        lang.pack(fill="x", pady=(16, 0))
        self.lang_choice = ChoiceRow(
            lang.body, self.t("显示器菜单语言", "Monitor menu language"),
            command=lambda v: self.app.set_vcp(vcp.VCP_OSD_LANGUAGE, v, "OSD 语言", "OSD language", delay_refresh=300),
        )
        self.lang_choice.pack(fill="x")

        note = Card(self.body, title=self.t("说明", "Notes"))
        note.pack(fill="x", pady=(16, 0))
        tk.Label(
            note.body,
            text=self.t(
                "这些按钮通过 DDC/CI 模拟显示器机身上的摇杆/按键，适合在键鼠场景下快速调整。"
                "不同固件支持的操作数量不同：当前显示器会按 MCCS 0xEB 的最大值自动禁用不支持的项目。",
                "These buttons drive the monitor's joystick/buttons over DDC/CI. "
                "Available operations depend on firmware; unsupported ones are disabled automatically.",
            ),
            bg=COLORS["card"], fg=COLORS["text_dim"], font=ui_font(9), justify="left", wraplength=760,
        ).pack(anchor="w")

    def _send(self, operation):
        self.app.set_vcp(vcp.VCP_EZOSD, operation, f"OSD {operation}", f"OSD {operation}", refresh=False)

    def refresh(self) -> None:
        state = self.state
        maximum = state.maximum(vcp.VCP_EZOSD)
        supported = state.supported(vcp.VCP_EZOSD)
        for op, zh, en, _ in self.OPERATIONS:
            button = getattr(self, f"btn_{op}", None)
            if not button:
                continue
            enabled = supported and (maximum == 0 or op <= maximum)
            button.set_enabled(enabled)

        values = state.caps.values(vcp.VCP_OSD_LANGUAGE) or list(vcp.OSD_LANGUAGES)
        options = [(vcp.OSD_LANGUAGES.get(value, f"0x{value:02X}"), value) for value in values]
        self.lang_choice.set_options(options, state.value(vcp.VCP_OSD_LANGUAGE))
        self.lang_choice.set_enabled(state.supported(vcp.VCP_OSD_LANGUAGE))


class ProfilesPage(BasePage):
    key = "profiles"
    title_zh = "配置文件"
    title_en = "Profiles"
    sub_zh = "保存、应用和交换显示配置"
    sub_en = "Save, apply and exchange display presets"

    def build(self) -> None:
        self.profiles = store.load_profiles()
        self.selected = -1

        bar = tk.Frame(self.body, bg=COLORS["bg"])
        bar.pack(fill="x")
        PillButton(bar, self.t("保存当前设置", "Save current"), command=self._save_current, accent=True).pack(side="left")
        PillButton(bar, self.t("导入", "Import"), command=self._import).pack(side="left", padx=(8, 0))
        PillButton(bar, self.t("导出所选", "Export selected"), command=self._export).pack(side="left", padx=(8, 0))
        tk.Label(
            bar, text=str(store.profiles_path()), bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(8)
        ).pack(side="right")

        row = tk.Frame(self.body, bg=COLORS["bg"])
        row.pack(fill="both", expand=True, pady=(16, 0))
        left = Card(row, title=self.t("已保存配置", "Saved profiles"))
        left.pack(side="left", fill="both", expand=True)
        self.listbox = tk.Listbox(
            left.body, bg=COLORS["panel"], fg=COLORS["text"], selectbackground=COLORS["accent"],
            selectforeground=COLORS["white"], font=ui_font(10), bd=0, highlightthickness=0,
            activestyle="none", height=12,
        )
        self.listbox.pack(fill="both", expand=True)
        self.listbox.bind("<<ListboxSelect>>", self._on_select)

        right = tk.Frame(row, bg=COLORS["bg"], width=330)
        right.pack(side="right", fill="y", padx=(16, 0))
        right.pack_propagate(False)
        detail = Card(right, title=self.t("配置详情", "Profile details"))
        detail.pack(fill="x")
        self.detail_name = tk.Label(detail.body, text="—", bg=COLORS["card"], fg=COLORS["text"], font=ui_font(13, "bold"), wraplength=280, justify="left")
        self.detail_name.pack(anchor="w")
        self.detail_meta = tk.Label(detail.body, text="", bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(9), justify="left", wraplength=280)
        self.detail_meta.pack(anchor="w", pady=(6, 12))
        self.btn_apply = PillButton(detail.body, self.t("应用配置", "Apply"), command=self._apply, accent=True)
        self.btn_apply.pack(fill="x", pady=(0, 6))
        self.btn_update = PillButton(detail.body, self.t("用当前状态覆盖", "Overwrite with current"), command=self._update)
        self.btn_update.pack(fill="x", pady=(0, 6))
        self.btn_delete = PillButton(detail.body, self.t("删除", "Delete"), command=self._delete)
        self.btn_delete.pack(fill="x")

        self._reload_list()

    def _reload_list(self):
        self.listbox.delete(0, "end")
        for profile in self.profiles:
            self.listbox.insert("end", profile.get("name", "?"))
        if self.profiles:
            self.selected = min(max(self.selected, 0), len(self.profiles) - 1)
            self.listbox.selection_set(self.selected)
            self._show_details(self.selected)
        else:
            self.selected = -1
            self._show_details(-1)

    def _on_select(self, _event=None):
        selection = self.listbox.curselection()
        if selection:
            self.selected = int(selection[0])
            self._show_details(self.selected)

    def _show_details(self, index):
        if 0 <= index < len(self.profiles):
            profile = self.profiles[index]
            self.detail_name.configure(text=profile.get("name", "?"))
            self.detail_meta.configure(
                text=f"{self.t('型号', 'Model')}: {profile.get('model', '—')}\n"
                     f"{self.t('创建时间', 'Created')}: {profile.get('created', '—')}\n"
                     f"{self.t('参数数量', 'Values')}: {len(profile.get('vcp', {}))}"
            )
            for btn in (self.btn_apply, self.btn_update, self.btn_delete):
                btn.set_enabled(True)
        else:
            self.detail_name.configure(text=self.t("未选择", "Nothing selected"))
            self.detail_meta.configure(text=self.t("保存当前设置或导入一个 JSON 配置。", "Save the current state or import a JSON profile."))
            for btn in (self.btn_apply, self.btn_update, self.btn_delete):
                btn.set_enabled(False)

    def _save_current(self):
        default = f"{self.state.model or 'Display'} {time.strftime('%m-%d %H:%M')}"
        name = self.app.ask_text(self.t("保存配置", "Save profile"), self.t("配置名称", "Profile name"), default)
        if not name:
            return
        profile = controller.capture_profile(self.state, name.strip())
        self.profiles.append(profile)
        store.save_profiles(self.profiles)
        self.selected = len(self.profiles) - 1
        self._reload_list()
        self.app.set_status(self.t(f"已保存配置：{name}", f"Saved profile: {name}"))

    def _update(self):
        if 0 <= self.selected < len(self.profiles):
            old = self.profiles[self.selected]
            profile = controller.capture_profile(self.state, old.get("name", "?"))
            profile["created"] = old.get("created", profile["created"])
            self.profiles[self.selected] = profile
            store.save_profiles(self.profiles)
            self._show_details(self.selected)
            self.app.set_status(self.t("配置已更新", "Profile updated"))

    def _delete(self):
        if not (0 <= self.selected < len(self.profiles)):
            return
        name = self.profiles[self.selected].get("name", "?")
        if not self.app.confirm(self.t("删除配置", "Delete profile"),
                                self.t(f"确定删除“{name}”吗？", f"Delete \"{name}\"?")):
            return
        del self.profiles[self.selected]
        store.save_profiles(self.profiles)
        self.selected = min(self.selected, len(self.profiles) - 1)
        self._reload_list()

    def _apply(self):
        if not (0 <= self.selected < len(self.profiles)):
            return
        profile = self.profiles[self.selected]
        state = self.state
        gen = self.app.generation
        self.app.set_status(self.t("正在应用配置…", "Applying profile..."))

        def job():
            return controller.apply_profile(state, profile)

        def done(result):
            if gen != self.app.generation:
                return
            applied, failed = result
            if failed:
                self.app.set_status(self.t(f"应用完成：{applied} 项成功，{failed} 项失败", f"Applied {applied}, {failed} failed"), "warn")
            else:
                self.app.set_status(self.t(f"配置已应用（{applied} 项）", f"Profile applied ({applied} values)"))
            self.app.refresh_current_page()
            self.app.update_header()

        self.app.run(job, done)

    def _export(self):
        if not (0 <= self.selected < len(self.profiles)):
            return
        profile = self.profiles[self.selected]
        path = filedialog.asksaveasfilename(
            title=self.t("导出配置", "Export profile"),
            defaultextension=".json",
            initialfile=f"{profile.get('name', 'profile')}.json",
            filetypes=[("JSON", "*.json")],
        )
        if path:
            Path(path).write_text(controller.profile_json(profile), encoding="utf-8")
            self.app.set_status(self.t("配置已导出", "Profile exported"))

    def _import(self):
        path = filedialog.askopenfilename(
            title=self.t("导入配置", "Import profile"),
            filetypes=[("JSON", "*.json")],
        )
        if not path:
            return
        try:
            import json

            profile = json.loads(Path(path).read_text(encoding="utf-8"))
            if not isinstance(profile, dict) or "vcp" not in profile:
                raise ValueError("invalid profile")
            profile.setdefault("name", Path(path).stem)
            self.profiles.append(profile)
            store.save_profiles(self.profiles)
            self.selected = len(self.profiles) - 1
            self._reload_list()
            self.app.set_status(self.t("配置已导入", "Profile imported"))
        except Exception as exc:
            messagebox.showerror(self.t("导入失败", "Import failed"), str(exc))

    def refresh(self) -> None:
        pass


class SettingsPage(BasePage):
    key = "settings"
    title_zh = "设置"
    title_en = "Settings"
    sub_zh = "语言、热键、启动与 DDC 选项"
    sub_en = "Language, hotkeys, startup and DDC"

    def build(self) -> None:
        app = self.app
        appearance = Card(self.body, title=self.t("外观", "Appearance"))
        appearance.pack(fill="x")
        row = tk.Frame(appearance.body, bg=COLORS["card"])
        row.pack(fill="x", pady=(0, 10))
        tk.Label(row, text=self.t("界面语言", "Language"), bg=COLORS["card"], fg=COLORS["text"], font=ui_font(10)).pack(side="left")
        languages = Segmented(
            row,
            [("简体中文", "zh"), ("English", "en")],
            value=app.lang,
            command=app.change_language,
            small=True,
        )
        languages.pack(side="right")
        self.tg_topmost = ToggleRow(
            appearance.body, self.t("窗口置顶", "Always on top"), value=app.settings["topmost"],
            command=self._topmost,
        )
        self.tg_topmost.pack(fill="x")

        startup = Card(self.body, title=self.t("启动", "Startup"))
        startup.pack(fill="x", pady=(16, 0))
        self.tg_autostart = ToggleRow(
            startup.body, self.t("开机自动启动", "Start with Windows"),
            value=store.is_autostart_enabled(),
            command=self._autostart,
            hint=self.t("写入 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", "Writes HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        )
        self.tg_autostart.pack(fill="x")

        hot = Card(self.body, title=self.t("全局快捷键", "Global hotkeys"), subtitle="RegisterHotKey")
        hot.pack(fill="x", pady=(16, 0))
        self.tg_hotkeys = ToggleRow(
            hot.body, self.t("启用全局快捷键", "Enable global hotkeys"),
            value=app.settings["hotkeys"], command=app.toggle_hotkeys,
        )
        self.tg_hotkeys.pack(fill="x", pady=(0, 8))
        for binding in DEFAULT_BINDINGS:
            line = tk.Frame(hot.body, bg=COLORS["card"])
            line.pack(fill="x", pady=1)
            tk.Label(line, text=app.t(binding["zh"], binding["en"]), bg=COLORS["card"], fg=COLORS["text_dim"], font=ui_font(9)).pack(side="left")
            tk.Label(line, text=binding["keys"], bg=COLORS["card"], fg=COLORS["muted"], font=mono_font(9)).pack(side="right")
        self.hotkey_status = tk.Label(hot.body, text="", bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8))
        self.hotkey_status.pack(anchor="w", pady=(6, 0))

        ddc = Card(self.body, title=self.t("显示器与 DDC/CI", "Monitor & DDC/CI"))
        ddc.pack(fill="x", pady=(16, 0))
        info = tk.Label(
            ddc.body,
            text=self.t(
                f"设备：{self.state.device_short}    型号：{self.state.model}    "
                f"命令数：{self.state.caps.vcp_count}    VCP 版本：{self.state.vcp_version()}",
                f"Device: {self.state.device_short}    Model: {self.state.model}    "
                f"Commands: {self.state.caps.vcp_count}    VCP: {self.state.vcp_version()}",
            ),
            bg=COLORS["card"], fg=COLORS["text_dim"], font=ui_font(9), justify="left", wraplength=760,
        )
        info.pack(anchor="w")
        buttons = tk.Frame(ddc.body, bg=COLORS["card"])
        buttons.pack(fill="x", pady=(10, 0))
        PillButton(buttons, self.t("重新扫描显示器", "Rescan monitors"), command=app.rescan, accent=True).pack(side="left")
        PillButton(buttons, self.t("刷新全部参数", "Refresh all values"), command=app.refresh_all).pack(side="left", padx=(8, 0))
        PillButton(buttons, self.t("查看 VCP 命令表", "View VCP table"), command=app.show_vcp_table).pack(side="left", padx=(8, 0))

        reset = Card(self.body, title=self.t("恢复出厂", "Factory reset"), subtitle="MCCS 0x04 / 0x08")
        reset.pack(fill="x", pady=(16, 0))
        tk.Label(
            reset.body,
            text=self.t("这些命令会覆盖显示器内部的所有画面设置，请谨慎使用。", "These commands overwrite every picture setting stored in the monitor."),
            bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8),
        ).pack(anchor="w")
        btn_row = tk.Frame(reset.body, bg=COLORS["card"])
        btn_row.pack(fill="x", pady=(8, 0))
        PillButton(btn_row, self.t("恢复全部设置", "Reset all"), command=lambda: self._reset(0x04)).pack(side="left")
        PillButton(btn_row, self.t("仅恢复颜色", "Reset color"), command=lambda: self._reset(0x08)).pack(side="left", padx=(8, 0))

        about = Card(self.body, title=self.t("关于", "About"))
        about.pack(fill="x", pady=(16, 0))
        tk.Label(
            about.body,
            text=self.t(
                f"ASUS DisplayWidget Center · Python 复刻版 {store.APP_VERSION}\n"
                "基于 DisplayWidgetCenter 1.5.1.3（.NET/WPF）的逆向分析实现，"
                "使用纯 Python ctypes 调用 dxva2!GetVCPFeatureAndVCPFeatureReply / SetVCPFeature。",
                f"ASUS DisplayWidget Center - Python reconstruction {store.APP_VERSION}\n"
                "Reimplemented from DisplayWidgetCenter 1.5.1.3 (.NET/WPF) using pure Python "
                "ctypes calls to dxva2!GetVCPFeatureAndVCPFeatureReply / SetVCPFeature.",
            ),
            bg=COLORS["card"], fg=COLORS["text_dim"], font=ui_font(9), justify="left", wraplength=760,
        ).pack(anchor="w")
        links = tk.Frame(about.body, bg=COLORS["card"])
        links.pack(fill="x", pady=(8, 0))
        PillButton(links, self.t("打开逆向报告", "Open RE report"), command=lambda: app.open_path(app.report_path)).pack(side="left")
        PillButton(links, self.t("打开数据目录", "Open data folder"), command=lambda: app.open_path(store.data_dir())).pack(side="left", padx=(8, 0))

    def _topmost(self, value):
        self.app.settings["topmost"] = bool(value)
        self.app.attributes("-topmost", bool(value))
        store.save_settings(self.app.settings)

    def _autostart(self, value):
        ok = store.set_autostart(bool(value))
        if not ok:
            self.tg_autostart.set_state(not value)
            self.app.set_status(self.t("无法修改启动项", "Could not update startup entry"), "error")
            return
        self.app.settings["autostart"] = bool(value)
        store.save_settings(self.app.settings)

    def _reset(self, code):
        label = "恢复全部设置" if code == 0x04 else "恢复颜色设置"
        if not self.app.confirm(self.t("确认恢复", "Confirm reset"), self.t("确定要恢复出厂设置吗？", "Reset the monitor to factory defaults?")):
            return
        self.app.set_vcp(code, 1, label, "Factory reset", delay_refresh=800)

    def refresh(self) -> None:
        if self.app.hotkey_manager:
            status = self.app.hotkey_manager.status
            active = sum(1 for value in status.values() if value)
            self.hotkey_status.configure(
                text=self.t(f"已注册 {active}/{len(status)} 个热键", f"{active}/{len(status)} hotkeys registered")
            )
        else:
            self.hotkey_status.configure(text=self.t("快捷键未启用", "Hotkeys disabled"))


# ---------------------------------------------------------------------------
# Main application
# ---------------------------------------------------------------------------
class App(tk.Tk):
    def __init__(self, initial_page: str | None = None, language: str | None = None,
                 scroll_bottom: bool = False):
        # Deliberately keep the process DPI-unaware: the ASUS panel is commonly
        # driven at 125-200% scaling and Tk's mixed point/pixel geometry is far
        # more predictable when Windows scales the whole window bitmap.
        super().__init__()
        self.scroll_bottom = scroll_bottom
        self.settings = store.load_settings()
        self.lang = language or self.settings.get("language") or "zh"
        self.title(APP_TITLE)
        self.configure(bg=COLORS["bg"])
        self.minsize(1120, 720)
        self.geometry(self.settings.get("geometry") or "1260x800")
        apply_theme(self)
        dark_title_bar(self)
        self.protocol("WM_DELETE_WINDOW", self._on_close)

        try:
            icon = Path(r"C:\Program Files (x86)\ASUS\DisplayWidgetCenter\asus_display_widget_center_app_icon_256.ico")
            if icon.exists():
                self.iconbitmap(str(icon))
        except Exception:
            pass

        self.manager = MonitorManager()
        self.states: list[MonitorState] = []
        self.state: MonitorState | None = None
        self.executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="dwc-ddc")
        self.ui_queue: queue.Queue = queue.Queue()
        self.pages: dict[str, BasePage] = {}
        self.current_page = initial_page or self.settings.get("last_page") or "overview"
        self.generation = 0
        self.hotkey_manager: HotkeyManager | None = None
        self._suppress_monitor_event = False
        self.loading_frame: tk.Frame | None = None
        self.report_path = Path(__file__).resolve().parent.parent / "REVERSE_ENGINEERING.md"

        self._build_shell()
        self.after(60, self._poll_queue)
        self.after(120, self.start_scan)
        self.after(600, self._maybe_start_hotkeys)
        self.after(150, lambda: dark_title_bar(self))
        if self.settings.get("topmost"):
            self.attributes("-topmost", True)

    # ------------------------------------------------------------------ i18n
    def t(self, zh: str, en: str) -> str:
        return zh if self.lang == "zh" else en

    # ----------------------------------------------------------------- shell
    def _build_shell(self):
        self.grid_rowconfigure(1, weight=1)
        self.grid_columnconfigure(1, weight=1)

        self.sidebar = tk.Frame(self, bg=COLORS["sidebar"], width=238)
        self.sidebar.grid(row=0, column=0, rowspan=3, sticky="nsw")
        self.sidebar.grid_propagate(False)

        brand = tk.Frame(self.sidebar, bg=COLORS["sidebar"])
        brand.pack(fill="x", padx=20, pady=(22, 14))
        logo = tk.Label(brand, text="ASUS", bg=COLORS["sidebar"], fg=COLORS["accent"], font=ui_font(21, "bold"))
        logo.pack(anchor="w")
        tk.Label(brand, text="DisplayWidget Center", bg=COLORS["sidebar"], fg=COLORS["text"], font=ui_font(10)).pack(anchor="w")
        tk.Label(brand, text="PYTHON EDITION", bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(7, "bold")).pack(anchor="w", pady=(3, 0))

        monitor_box = tk.Frame(self.sidebar, bg=COLORS["sidebar"])
        monitor_box.pack(fill="x", padx=18, pady=(6, 10))
        tk.Label(monitor_box, text=self.t("显示器", "MONITOR"), bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(7, "bold")).pack(anchor="w")
        self.monitor_combo = ttk.Combobox(monitor_box, state="readonly", font=ui_font(9))
        self.monitor_combo.pack(fill="x", pady=(5, 0))
        self.monitor_combo.bind("<<ComboboxSelected>>", self._on_monitor_selected)
        status_row = tk.Frame(monitor_box, bg=COLORS["sidebar"])
        status_row.pack(fill="x", pady=(6, 0))
        self.sidebar_dot = tk.Canvas(status_row, width=10, height=10, bg=COLORS["sidebar"], highlightthickness=0)
        self.sidebar_dot.pack(side="left")
        self.sidebar_status = tk.Label(status_row, text=self.t("正在扫描…", "Scanning..."), bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(8))
        self.sidebar_status.pack(side="left", padx=(5, 0))

        self.nav_frame = tk.Frame(self.sidebar, bg=COLORS["sidebar"])
        self.nav_frame.pack(fill="x", pady=(6, 0))
        self.nav_items: dict[str, NavItem] = {}
        for key, zh, en, _, _ in PAGE_DEFS:
            item = NavItem(self.nav_frame, key, self.t(zh, en), key, self.navigate)
            item.pack(fill="x", pady=1)
            self.nav_items[key] = item

        bottom = tk.Frame(self.sidebar, bg=COLORS["sidebar"])
        bottom.pack(side="bottom", fill="x", padx=18, pady=16)
        tk.Frame(bottom, bg=COLORS["border"], height=1).pack(fill="x", pady=(0, 10))
        refresh = tk.Frame(bottom, bg=COLORS["sidebar"], cursor="hand2")
        refresh.pack(fill="x")
        icon = tk.Canvas(refresh, width=18, height=18, bg=COLORS["sidebar"], highlightthickness=0)
        icon.pack(side="left")
        draw_icon(icon, "refresh", COLORS["muted"], 18)
        label = tk.Label(refresh, text=self.t("重新扫描", "Rescan"), bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(8))
        label.pack(side="left", padx=(6, 0))
        for widget in (refresh, icon, label):
            widget.bind("<Button-1>", lambda e: self.rescan())
        tk.Label(bottom, text=f"v{store.APP_VERSION}  ·  py{'.'.join(map(str, __import__('sys').version_info[:2]))}", bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(7)).pack(anchor="w", pady=(8, 0))

        self.header = tk.Frame(self, bg=COLORS["bg"], height=82)
        self.header.grid(row=0, column=1, sticky="new")
        self.header.grid_propagate(False)
        self.header.grid_columnconfigure(0, weight=1)
        title_box = tk.Frame(self.header, bg=COLORS["bg"])
        title_box.grid(row=0, column=0, sticky="w", padx=(28, 0), pady=(16, 0))
        self.page_title = tk.Label(title_box, text="", bg=COLORS["bg"], fg=COLORS["text"], font=ui_font(18, "bold"))
        self.page_title.pack(anchor="w")
        self.page_sub = tk.Label(title_box, text="", bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(9))
        self.page_sub.pack(anchor="w", pady=(2, 0))

        right = tk.Frame(self.header, bg=COLORS["bg"])
        right.grid(row=0, column=1, sticky="e", padx=(0, 26), pady=(20, 0))
        self.badge = tk.Frame(right, bg=COLORS["panel"], highlightbackground=COLORS["border"], highlightthickness=1)
        self.badge.pack(side="left", padx=(0, 10))
        badge_inner = tk.Frame(self.badge, bg=COLORS["panel"])
        badge_inner.pack(padx=12, pady=6)
        self.badge_dot = tk.Canvas(badge_inner, width=8, height=8, bg=COLORS["panel"], highlightthickness=0)
        self.badge_dot.pack(side="left")
        self.badge_label = tk.Label(badge_inner, text="—", bg=COLORS["panel"], fg=COLORS["text_dim"], font=ui_font(8))
        self.badge_label.pack(side="left", padx=(6, 0))
        refresh_btn = tk.Label(right, text="↻", bg=COLORS["panel"], fg=COLORS["text_dim"], font=ui_font(12), padx=9, pady=3, cursor="hand2",
                               highlightbackground=COLORS["border"], highlightthickness=1)
        refresh_btn.pack(side="left")
        refresh_btn.bind("<Button-1>", lambda e: self.refresh_all())
        refresh_btn.bind("<Enter>", lambda e: refresh_btn.configure(bg=COLORS["card_hover"]))
        refresh_btn.bind("<Leave>", lambda e: refresh_btn.configure(bg=COLORS["panel"]))

        self.page_host = tk.Frame(self, bg=COLORS["bg"])
        self.page_host.grid(row=1, column=1, sticky="nsew")

        self.status = tk.Frame(self, bg=COLORS["sidebar"], height=28)
        self.status.grid(row=2, column=1, sticky="sew")
        self.status.grid_propagate(False)
        self.status_dot = tk.Canvas(self.status, width=8, height=8, bg=COLORS["sidebar"], highlightthickness=0)
        self.status_dot.pack(side="left", padx=(28, 6))
        self.status_label = tk.Label(self.status, text=self.t("就绪", "Ready"), bg=COLORS["sidebar"], fg=COLORS["muted"], font=ui_font(8))
        self.status_label.pack(side="left")
        self.status_right = tk.Label(self.status, text="", bg=COLORS["sidebar"], fg=COLORS["muted"], font=mono_font(8))
        self.status_right.pack(side="right", padx=(0, 26))
        self._set_status_dot(COLORS["muted"])

    def _set_status_dot(self, color):
        self.status_dot.delete("all")
        self.status_dot.create_oval(1, 1, 8, 8, fill=color, outline=color)

    def set_status(self, text, kind="ok"):
        colors = {"ok": COLORS["green"], "error": COLORS["red"], "warn": COLORS["amber"], "busy": COLORS["cyan"]}
        try:
            if self.status_label.winfo_exists():
                self.status_label.configure(text=text)
                self._set_status_dot(colors.get(kind, COLORS["muted"]))
        except tk.TclError:
            pass

    def set_status_right(self, text):
        try:
            self.status_right.configure(text=text)
        except tk.TclError:
            pass

    # --------------------------------------------------------------- monitor
    def start_scan(self, force_caps: bool = False):
        self.generation += 1
        gen = self.generation
        self.show_loading()
        self.set_status(self.t("正在枚举显示器…", "Enumerating monitors..."), "busy")

        def job():
            states = self.manager.scan()
            for state in states:
                state.probe(force_caps=force_caps)
            return states

        def done(states):
            if gen == self.generation:
                self._on_scanned(states)

        self.run(job, done, guard=False)

    def _on_scanned(self, states):
        self.states = states
        if not states:
            self.state = None
            self._suppress_monitor_event = False
            self.show_empty(self.t("未发现显示器", "No monitors found"))
            self.set_status(self.t("未发现显示器", "No monitors found"), "error")
            self.sidebar_status.configure(text=self.t("未连接", "Not connected"))
            return
        names = []
        for state in states:
            suffix = "" if state.ddc_ok else self.t(" · 不支持 DDC", " · no DDC")
            names.append(f"{state.short_name}{suffix}")
        self._suppress_monitor_event = True
        self.monitor_combo.configure(values=names)
        saved = self.settings.get("monitor_key", "")
        index = next((i for i, s in enumerate(states) if s.key == saved), -1)
        if index < 0:
            index = next((i for i, s in enumerate(states) if s.ddc_ok), 0)
        self.monitor_combo.current(index)
        self._suppress_monitor_event = False
        self._select_state(index)
        self.set_status(self.t("显示器已就绪", "Monitor ready"))

    def _select_state(self, index):
        if not (0 <= index < len(self.states)):
            return
        self.generation += 1
        self.state = self.states[index]
        self.settings["monitor_key"] = self.state.key
        store.save_settings(self.settings)
        ok = self.state.ddc_ok
        self.sidebar_dot.delete("all")
        self.sidebar_dot.create_oval(1, 1, 9, 9, fill=COLORS["green"] if ok else COLORS["red"], outline="")
        self.sidebar_status.configure(
            text=self.t("DDC/CI 已连接", "DDC/CI connected") if ok else self.t("DDC/CI 无响应", "DDC/CI unavailable"),
            fg=COLORS["green"] if ok else COLORS["red"],
        )
        self.update_header()
        self.rebuild_pages()
        if not ok:
            self.show_empty(self.t("此显示器不支持 DDC/CI", "This monitor does not expose DDC/CI"))
            self.set_status(self.t("所选显示器没有 DDC/CI 响应", "Selected monitor has no DDC/CI response"), "warn")
            return
        self.navigate(self.current_page, force=True)
        self._load_remaining()

    def _load_remaining(self):
        state = self.state
        if not state or state.extra_loaded or not state.caps.vcp:
            return
        generation = self.generation

        def job():
            state.probe_remaining()
            return True

        def done(_):
            if generation == self.generation and state is self.state:
                self.refresh_current_page()
                self.update_header()

        self.run(job, done)

    def _on_monitor_selected(self, _event=None):
        if self._suppress_monitor_event:
            return
        index = self.monitor_combo.current()
        if index >= 0:
            self._select_state(index)

    def update_header(self):
        state = self.state
        if not state:
            return
        text = f"{state.short_name}  ·  {state.resolution}"
        self.badge_label.configure(text=text)
        self.badge_dot.delete("all")
        self.badge_dot.create_oval(1, 1, 8, 8, fill=COLORS["green"] if state.ddc_ok else COLORS["red"], outline="")
        if hasattr(self, "page_title"):
            self.page_title.configure(text=self.t(self._page_meta()[1], self._page_meta()[2]))
            self.page_sub.configure(text=self.t(self._page_meta()[3], self._page_meta()[4]))
        self.set_status_right(
            f"{state.model or state.short_name}  ·  VCP {state.caps.vcp_count}  ·  {state.resolution}"
        )

    def _page_meta(self):
        for key, zh, en, sub_zh, sub_en in PAGE_DEFS:
            if key == self.current_page:
                return key, zh, en, sub_zh, sub_en
        return "overview", "概览", "Overview", "", ""

    # ------------------------------------------------------------------ pages
    def rebuild_pages(self):
        for page in self.pages.values():
            page.frame.destroy()
        self.pages = {}

    def navigate(self, key, force=False):
        if not self.state or not self.state.ddc_ok:
            return
        if key not in [item[0] for item in PAGE_DEFS]:
            key = "overview"
        self.current_page = key
        self.settings["last_page"] = key
        store.save_settings(self.settings)
        for nav_key, item in self.nav_items.items():
            item.set_active(nav_key == key)
        meta = self._page_meta()
        self.page_title.configure(text=self.t(meta[1], meta[2]))
        self.page_sub.configure(text=self.t(meta[3], meta[4]))

        if self.loading_frame and self.loading_frame.winfo_exists():
            self.loading_frame.destroy()
            self.loading_frame = None

        for other_key, page in self.pages.items():
            if other_key != key:
                page.frame.pack_forget()
        page = self.pages.get(key)
        if page is None:
            page = self._create_page(key)
            self.pages[key] = page
        page.show()
        if self.scroll_bottom and page.scroll is not None:
            self.after(250, lambda: page.scroll.canvas.yview_moveto(1.0))

    def _create_page(self, key):
        cls = {
            "overview": OverviewPage,
            "display": DisplayPage,
            "eyecare": EyeCarePage,
            "gameplus": GamePlusPage,
            "osd": OSDPage,
            "profiles": ProfilesPage,
            "settings": SettingsPage,
        }[key]
        state = self.state
        try:
            return cls(self, self.page_host)
        except Exception as exc:
            import traceback

            traceback.print_exc()
            frame = tk.Frame(self.page_host, bg=COLORS["bg"])
            tk.Label(frame, text=f"页面加载失败：{exc}", bg=COLORS["bg"], fg=COLORS["red"], font=ui_font(11)).pack(pady=40)
            page = BasePage.__new__(BasePage)
            page.app, page.state, page.frame, page.scroll, page.body = self, state, frame, None, None
            page.refresh = lambda: None
            page.show = lambda: frame.pack(fill="both", expand=True)
            return page

    def refresh_current_page(self):
        page = self.pages.get(self.current_page)
        if page:
            try:
                page.refresh()
            except Exception:
                pass
        self.update_header()

    def show_loading(self):
        self.rebuild_pages()
        for child in self.page_host.winfo_children():
            child.destroy()
        self.loading_frame = tk.Frame(self.page_host, bg=COLORS["bg"])
        self.loading_frame.pack(fill="both", expand=True)
        box = tk.Frame(self.loading_frame, bg=COLORS["bg"])
        box.place(relx=0.5, rely=0.42, anchor="center")
        spinner = tk.Canvas(box, width=46, height=46, bg=COLORS["bg"], highlightthickness=0)
        spinner.pack()
        spinner.create_arc(6, 6, 40, 40, start=30, extent=280, width=4, outline=COLORS["accent"], style="arc")
        tk.Label(box, text=self.t("正在读取显示器信息…", "Reading monitor information..."), bg=COLORS["bg"], fg=COLORS["text"], font=ui_font(11)).pack(pady=(14, 4))
        tk.Label(box, text=self.t("DDC/CI 通信约需 1–2 秒", "DDC/CI usually takes 1-2 seconds"), bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(8)).pack()
        self._spin(spinner, 30)

    def _spin(self, canvas, angle):
        if not canvas.winfo_exists():
            return
        canvas.delete("all")
        canvas.create_arc(6, 6, 40, 40, start=angle, extent=280, width=4, outline=COLORS["accent"], style="arc")
        self.after(40, lambda: self._spin(canvas, (angle + 18) % 360))

    def show_empty(self, message):
        self.rebuild_pages()
        for child in self.page_host.winfo_children():
            child.destroy()
        frame = tk.Frame(self.page_host, bg=COLORS["bg"])
        frame.pack(fill="both", expand=True)
        box = tk.Frame(frame, bg=COLORS["bg"])
        box.place(relx=0.5, rely=0.42, anchor="center")
        tk.Label(box, text="⚠", bg=COLORS["bg"], fg=COLORS["amber"], font=ui_font(28)).pack()
        tk.Label(box, text=message, bg=COLORS["bg"], fg=COLORS["text"], font=ui_font(12, "bold")).pack(pady=(8, 4))
        tk.Label(box, text=self.t("请检查显示器连接，或点击下方按钮重试。", "Check the monitor connection and try again."), bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(9)).pack()
        PillButton(box, self.t("重新扫描", "Rescan"), command=self.rescan, accent=True).pack(pady=14)

    # ------------------------------------------------------------------- ddc
    def run(self, fn, on_done=None, on_error=None, guard=True):
        generation = self.generation

        def job():
            try:
                result = fn()
            except Exception as exc:  # noqa: BLE001
                self.ui_queue.put(("error", on_error, exc, generation if guard else None))
                return
            self.ui_queue.put(("ok", on_done, result, generation if guard else None))

        self.executor.submit(job)

    def _poll_queue(self):
        try:
            while True:
                kind, callback, payload, generation = self.ui_queue.get_nowait()
                if kind == "hotkey":
                    self._handle_hotkey(callback)
                    continue
                if generation is not None and generation != self.generation:
                    continue
                if kind == "error":
                    if callback:
                        callback(payload)
                    else:
                        self.set_status(str(payload), "error")
                elif callback:
                    callback(payload)
        except queue.Empty:
            pass
        except tk.TclError:
            return
        try:
            self.after(40, self._poll_queue)
        except tk.TclError:
            pass

    def set_vcp(self, code, value, label_zh, label_en, delay_refresh=0, refresh=True):
        state = self.state
        if not state or value is None:
            return
        if not state.supported(code):
            self.set_status(self.t(f"{label_zh} 不受支持", f"{label_en} is not supported"), "warn")
            return

        def job():
            return state.write(code, value)

        def done(ok):
            if ok:
                text = f"{self.t(label_zh, label_en)} → {value}"
                self.set_status(text)
                if delay_refresh:
                    self.after(delay_refresh, self.refresh_current_page)
                elif refresh:
                    self.refresh_current_page()
            else:
                self.set_status(self.t(f"{label_zh} 设置失败", f"Failed to set {label_en}"), "error")

        self.run(job, done)

    def toggle_bit(self, code, bit, enabled, label):
        state = self.state
        if not state:
            return

        def job():
            return state.write_toggle_bit(code, bit, enabled)

        def done(ok):
            if ok:
                self.set_status(f"{label} → {'ON' if enabled else 'OFF'}")
                self.refresh_current_page()
            else:
                self.set_status(self.t(f"{label} 设置失败", f"Failed to set {label}"), "error")

        self.run(job, done)

    def set_mode(self, mode):
        state = self.state
        if not state:
            return

        def job():
            return state.set_mode(mode)

        def done(ok):
            if ok:
                self.set_status(f"{self.t('显示模式', 'Display mode')} → {self.t(mode.zh, mode.en)}")
                self.after(350, self.refresh_current_page)
                self.update_header()
            else:
                self.set_status(self.t("切换显示模式失败", "Failed to switch display mode"), "error")

        self.run(job, done)

    def write_many(self, pairs, label_zh, label_en):
        state = self.state
        if not state:
            return

        def job():
            count = 0
            for code, value in pairs:
                if state.supported(code) and state.write(code, value):
                    count += 1
                time.sleep(0.03)
            return count

        def done(count):
            self.set_status(f"{self.t(label_zh, label_en)} ({count})")
            self.after(300, self.refresh_current_page)

        self.run(job, done)

    def refresh_all(self):
        state = self.state
        if not state:
            self.start_scan()
            return
        self.set_status(self.t("正在刷新…", "Refreshing..."), "busy")

        def job():
            state.refresh()
            return True

        def done(_):
            self.refresh_current_page()
            self.update_header()
            self.set_status(self.t("参数已刷新", "Values refreshed"))

        self.run(job, done)

    def rescan(self):
        self.start_scan(force_caps=True)

    # --------------------------------------------------------------- hotkeys
    def toggle_hotkeys(self, enabled):
        self.settings["hotkeys"] = bool(enabled)
        store.save_settings(self.settings)
        if enabled:
            self._start_hotkeys()
        else:
            if self.hotkey_manager:
                self.hotkey_manager.stop()
                self.hotkey_manager = None
            self.set_status(self.t("全局快捷键已禁用", "Global hotkeys disabled"))

    def _start_hotkeys(self):
        if self.hotkey_manager:
            return
        manager = HotkeyManager(lambda action: self.ui_queue.put(("hotkey", action, None, None)))
        manager.start()
        self.hotkey_manager = manager
        active = sum(1 for value in manager.status.values() if value)
        self.set_status(self.t(f"全局快捷键已启用（{active} 个）", f"Global hotkeys enabled ({active})"))

    def _maybe_start_hotkeys(self):
        if self.settings.get("hotkeys") and not self.hotkey_manager:
            self._start_hotkeys()

    def _handle_hotkey(self, action):
        state = self.state
        if not state:
            return
        if action in ("brightness_up", "brightness_down"):
            code = vcp.VCP_BRIGHTNESS
            delta = 5 if action.endswith("up") else -5
        elif action in ("contrast_up", "contrast_down"):
            code = vcp.VCP_CONTRAST
            delta = 5 if action.endswith("up") else -5
        else:
            code = None
            delta = 0
        if code is not None:
            current = state.value(code)
            if current is None:
                return
            maximum = state.maximum(code) or 100
            new_value = min(maximum, max(0, current + delta))
            self.set_vcp(code, new_value, "亮度" if code == vcp.VCP_BRIGHTNESS else "对比度",
                         "Brightness" if code == vcp.VCP_BRIGHTNESS else "Contrast")
            return
        if action == "toggle_bluelight":
            current = state.value(vcp.VCP_BLUE_LIGHT) or 0
            self.set_vcp(vcp.VCP_BLUE_LIGHT, 0 if current else 3, "滤蓝光", "Blue light")
        elif action == "toggle_mute":
            self.toggle_bit(vcp.VCP_TOGGLES1, vcp.MUTE_BIT,
                            not state.is_on(vcp.VCP_TOGGLES1, vcp.MUTE_BIT), "静音")
        elif action == "cycle_mode":
            modes = [m for m in vcp.mode_set(state.line) if self._mode_available(m.value)]
            if not modes:
                return
            current = state.current_mode()
            try:
                index = modes.index(current) if current in modes else -1
            except ValueError:
                index = -1
            self.set_mode(modes[(index + 1) % len(modes)])

    def _mode_available(self, value):
        state = self.state
        values = state.caps.values(vcp.display_mode_code(state.line))
        if not values:
            return True
        return value in values or (value << 8) in values or (value << 8 | 0xFF) in values

    # ------------------------------------------------------------------ misc
    def change_language(self, lang):
        if lang == self.lang:
            return
        self.lang = lang
        self.settings["language"] = lang
        store.save_settings(self.settings)
        current = self.current_page
        for child in self.winfo_children():
            child.destroy()
        self.pages = {}
        self._build_shell()
        self._populate_monitor_combo()
        self.update_header()
        self.navigate(current, force=True)

    def _populate_monitor_combo(self):
        names = []
        for state in self.states:
            suffix = "" if state.ddc_ok else self.t(" · 不支持 DDC", " · no DDC")
            names.append(f"{state.short_name}{suffix}")
        self._suppress_monitor_event = True
        self.monitor_combo.configure(values=names)
        for index, state in enumerate(self.states):
            if state is self.state:
                self.monitor_combo.current(index)
                break
        self._suppress_monitor_event = False

    def ask_text(self, title, prompt, initial=""):
        top = tk.Toplevel(self)
        top.title(title)
        top.configure(bg=COLORS["panel"])
        top.transient(self)
        top.resizable(False, False)
        top.grab_set()
        result = {"value": None}
        frame = tk.Frame(top, bg=COLORS["panel"])
        frame.pack(fill="both", expand=True, padx=22, pady=18)
        tk.Label(frame, text=prompt, bg=COLORS["panel"], fg=COLORS["text"], font=ui_font(10)).pack(anchor="w")
        entry = tk.Entry(
            frame, bg=COLORS["card"], fg=COLORS["text"], insertbackground=COLORS["text"],
            relief="flat", font=ui_font(11), highlightthickness=1,
            highlightbackground=COLORS["border"], highlightcolor=COLORS["accent"],
        )
        entry.pack(fill="x", pady=(10, 14), ipady=6)
        entry.insert(0, initial)
        entry.select_range(0, "end")
        entry.focus_set()
        buttons = tk.Frame(frame, bg=COLORS["panel"])
        buttons.pack(fill="x")

        def ok():
            result["value"] = entry.get().strip()
            top.destroy()

        tk.Button(buttons, text=self.t("取消", "Cancel"), command=top.destroy, bg=COLORS["card"], fg=COLORS["text_dim"],
                  relief="flat", padx=14, pady=5, font=ui_font(9)).pack(side="right")
        tk.Button(buttons, text=self.t("确定", "OK"), command=ok, bg=COLORS["accent"], fg=COLORS["white"],
                  relief="flat", padx=16, pady=5, font=ui_font(9, "bold"), activebackground=COLORS["accent_hover"]).pack(side="right", padx=(0, 8))
        entry.bind("<Return>", lambda e: ok())
        entry.bind("<Escape>", lambda e: top.destroy())
        self._center_window(top, 420, 170)
        self.wait_window(top)
        return result["value"] or None

    def confirm(self, title, message):
        top = tk.Toplevel(self)
        top.title(title)
        top.configure(bg=COLORS["panel"])
        top.transient(self)
        top.resizable(False, False)
        top.grab_set()
        result = {"ok": False}
        frame = tk.Frame(top, bg=COLORS["panel"])
        frame.pack(fill="both", expand=True, padx=24, pady=20)
        tk.Label(frame, text=message, bg=COLORS["panel"], fg=COLORS["text"], font=ui_font(10),
                 wraplength=360, justify="left").pack(anchor="w")
        buttons = tk.Frame(frame, bg=COLORS["panel"])
        buttons.pack(fill="x", pady=(18, 0))

        def ok():
            result["ok"] = True
            top.destroy()

        tk.Button(buttons, text=self.t("取消", "Cancel"), command=top.destroy, bg=COLORS["card"], fg=COLORS["text_dim"],
                  relief="flat", padx=14, pady=5, font=ui_font(9)).pack(side="right")
        tk.Button(buttons, text=self.t("确定", "Confirm"), command=ok, bg=COLORS["accent"], fg=COLORS["white"],
                  relief="flat", padx=16, pady=5, font=ui_font(9, "bold"), activebackground=COLORS["accent_hover"]).pack(side="right", padx=(0, 8))
        self._center_window(top, 440, 190)
        self.wait_window(top)
        return result["ok"]

    def _center_window(self, window, width, height):
        self.update_idletasks()
        x = self.winfo_rootx() + (self.winfo_width() - width) // 2
        y = self.winfo_rooty() + (self.winfo_height() - height) // 2
        window.geometry(f"{width}x{height}+{x}+{y}")

    def show_vcp_table(self):
        state = self.state
        if not state:
            return
        top = tk.Toplevel(self)
        top.title(self.t("VCP 命令表", "VCP command table"))
        top.configure(bg=COLORS["bg"])
        top.geometry("760x620")
        top.transient(self)
        title = tk.Label(top, text=f"{state.short_name} · {state.caps.vcp_count} VCP", bg=COLORS["bg"], fg=COLORS["text"], font=ui_font(14, "bold"))
        title.pack(anchor="w", padx=20, pady=(16, 8))
        text = tk.Text(top, bg=COLORS["panel"], fg=COLORS["text"], insertbackground=COLORS["text"],
                       font=mono_font(9), relief="flat", padx=14, pady=12, wrap="none")
        text.pack(fill="both", expand=True, padx=20, pady=(0, 20))
        header = f"{'CODE':<6} {'NAME':<22} {'CURRENT':>8} {'MAX':>8} {'TYPE':>5}\n" + "-" * 66 + "\n"
        text.insert("1.0", header)
        for code in sorted(state.caps.vcp):
            entry = state.values.get(code)
            name = (vcp.VCP_NAMES_EN if self.lang == "en" else vcp.VCP_NAMES).get(code, "")
            current = str(entry.current) if entry else "-"
            maximum = str(entry.maximum) if entry else "-"
            vtype = str(entry.vcp_type) if entry else "-"
            text.insert("end", f"0x{code:02X}   {name:<22} {current:>8} {maximum:>8} {vtype:>5}\n")
        text.configure(state="disabled")
        tk.Label(top, text=self.t("数据来自 MCCS 能力字符串与实时读取", "From the MCCS capabilities string and live reads"),
                 bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(8)).pack(anchor="w", padx=20, pady=(0, 14))

    def open_path(self, path):
        try:
            import os

            os.startfile(str(path))
        except Exception:
            messagebox.showinfo(APP_TITLE, str(path))

    # ----------------------------------------------------------------- close
    def _on_close(self):
        try:
            self.settings["geometry"] = self.geometry()
            store.save_settings(self.settings)
        except Exception:
            pass
        if self.hotkey_manager:
            self.hotkey_manager.stop()
        self.executor.shutdown(wait=False)
        try:
            self.manager.close()
        except Exception:
            pass
        self.destroy()
