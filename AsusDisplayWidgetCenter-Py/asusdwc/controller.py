"""High level monitor model: probing, capabilities, read/write, profiles."""

from __future__ import annotations

import json
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

from . import store, vcp
from .ddc import MonitorHandle, VcpValue

# Controls read during a probe / captured in a profile.
# Controls read during the fast first probe (overview + most-used display
# controls).  A second background pass fills the rest so the window can appear
# quickly even on slow DDC links.
FAST_CODES = [
    vcp.VCP_BRIGHTNESS,
    vcp.VCP_CONTRAST,
    vcp.VCP_COLOR_TEMP,
    vcp.VCP_INPUT,
    vcp.VCP_DISPLAY_MODE,
    vcp.VCP_VOLUME,
    vcp.VCP_BLUE_LIGHT,
    vcp.VCP_TOGGLES1,
    vcp.VCP_TOGGLES2,
    vcp.VCP_VCP_VERSION,
    vcp.VCP_FIRMWARE,
    vcp.VCP_SHADOW_BOOST,
    vcp.VCP_OVERDRIVE,
    vcp.VCP_GAMMA,
]

READ_CODES = FAST_CODES + [
    vcp.VCP_RED_GAIN,
    vcp.VCP_GREEN_GAIN,
    vcp.VCP_BLUE_GAIN,
    vcp.VCP_SATURATION,
    vcp.VCP_HUE,
    vcp.VCP_SHARPNESS,
    vcp.VCP_CROSSHAIR,
    vcp.VCP_POWER_MODE,
    vcp.VCP_GAMEPLUS_TIMER,
    vcp.VCP_FPS_COUNTER,
    vcp.VCP_ALIGNMENT,
    vcp.VCP_GAMEPLUS_POSITION,
    vcp.VCP_REST_REMINDER,
    vcp.VCP_OSD_LANGUAGE,
]

PROFILE_CODES = [
    vcp.VCP_DISPLAY_MODE,
    vcp.VCP_BRIGHTNESS,
    vcp.VCP_CONTRAST,
    vcp.VCP_COLOR_TEMP,
    vcp.VCP_RED_GAIN,
    vcp.VCP_GREEN_GAIN,
    vcp.VCP_BLUE_GAIN,
    vcp.VCP_RED_SAT,
    vcp.VCP_YELLOW_SAT,
    vcp.VCP_GREEN_SAT,
    vcp.VCP_CYAN_SAT,
    vcp.VCP_BLUE_SAT,
    vcp.VCP_MAGENTA_SAT,
    vcp.VCP_VOLUME,
    vcp.VCP_SHARPNESS,
    vcp.VCP_SATURATION,
    vcp.VCP_HUE,
    vcp.VCP_GAMMA,
    vcp.VCP_OVERDRIVE,
    vcp.VCP_SHADOW_BOOST,
    vcp.VCP_BLUE_LIGHT,
    vcp.VCP_INPUT,
    vcp.VCP_TOGGLES1,
    vcp.VCP_TOGGLES2,
    vcp.VCP_CROSSHAIR,
    vcp.VCP_GAMEPLUS_TIMER,
    vcp.VCP_FPS_COUNTER,
    vcp.VCP_ALIGNMENT,
]


_CAPS_TTL_SECONDS = 7 * 24 * 3600


def _caps_cache_path() -> Path:
    return store.data_dir() / "capabilities.json"


def _caps_cache_key(handle: MonitorHandle) -> str:
    return f"{handle.device_name}|{handle.description}"


def _load_caps_cache() -> dict:
    try:
        data = json.loads(_caps_cache_path().read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def _save_caps_cache(key: str, caps: str, model: str) -> None:
    data = _load_caps_cache()
    data[key] = {"caps": caps, "model": model, "time": time.time()}
    try:
        _caps_cache_path().write_text(
            json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8"
        )
    except Exception:
        pass


def _cached_caps(handle: MonitorHandle) -> str | None:
    entry = _load_caps_cache().get(_caps_cache_key(handle))
    if not isinstance(entry, dict):
        return None
    if time.time() - float(entry.get("time", 0)) > _CAPS_TTL_SECONDS:
        return None
    caps = entry.get("caps")
    return caps if isinstance(caps, str) and "vcp(" in caps.lower() else None


@dataclass
class MonitorState:
    handle: MonitorHandle
    caps: vcp.CapabilityInfo = field(default_factory=vcp.CapabilityInfo)
    model: str = ""
    line: str = vcp.ProductLine.NONE
    values: dict[int, VcpValue] = field(default_factory=dict)
    ddc_ok: bool = False
    probed: bool = False
    extra_loaded: bool = False
    error: str = ""

    # ------------------------------------------------------------------ ids
    @property
    def key(self) -> str:
        return f"{self.handle.device_name}|{self.model}|{self.handle.index}"

    @property
    def display_name(self) -> str:
        name = self.model or self.handle.description or f"显示器 {self.handle.index + 1}"
        if self.handle.device_name:
            short = self.handle.device_name.split("\\")[-1]
            return f"{name}  ({short})"
        return name

    @property
    def short_name(self) -> str:
        return self.model or self.handle.description or f"Display {self.handle.index + 1}"

    @property
    def device_short(self) -> str:
        return (self.handle.device_name or "").split("\\")[-1]

    @property
    def resolution(self) -> str:
        if self.handle.width and self.handle.height:
            return f"{self.handle.width} × {self.handle.height}"
        return "—"

    # -------------------------------------------------------------- reading
    def value(self, code: int) -> int | None:
        entry = self.values.get(code)
        return entry.current if entry else None

    def maximum(self, code: int) -> int:
        entry = self.values.get(code)
        return entry.maximum if entry else 0

    def raw_supported(self, code: int) -> bool:
        return code in self.caps.vcp

    def supported(self, code: int) -> bool:
        if code in self.caps.vcp:
            return True
        entry = self.values.get(code)
        return bool(entry and entry.maximum > 0)

    def read(self, code: int) -> VcpValue | None:
        result = self.handle.get_vcp(code)
        if result is not None:
            self.values[code] = result
        return result

    def probe(self, force_caps: bool = False) -> "MonitorState":
        caps_text = None if force_caps else _cached_caps(self.handle)
        if not caps_text:
            queried = self.handle.query_capabilities(retries=3)
            if queried:
                _save_caps_cache(
                    _caps_cache_key(self.handle),
                    queried,
                    vcp.parse_capabilities(queried).model,
                )
            caps_text = queried or _cached_caps(self.handle)
        if caps_text:
            self.caps = vcp.parse_capabilities(caps_text)
        if not self.model:
            self.model = self.caps.model or self.handle.description.strip()
        self.line = vcp.product_line(self.model) if self.model else vcp.ProductLine.NONE

        # Without a capabilities string we only probe a few well-known controls:
        # virtual displays / KVM ghosts would otherwise cost several seconds in
        # failing I2C transactions.
        if self.caps.vcp:
            codes = FAST_CODES
        else:
            codes = [
                vcp.VCP_BRIGHTNESS,
                vcp.VCP_CONTRAST,
                vcp.VCP_INPUT,
                vcp.VCP_VOLUME,
                vcp.VCP_COLOR_TEMP,
                vcp.VCP_DISPLAY_MODE,
            ]
        successes = 0
        for code in codes:
            if self.read(code) is not None:
                successes += 1

        self.ddc_ok = successes > 0
        self.probed = True
        if not self.ddc_ok:
            self.error = "DDC/CI 无响应（显示器可能不支持或已休眠）"
        return self

    def probe_remaining(self) -> None:
        """Second pass: read every advertised control that is still missing."""
        if not self.caps.vcp:
            return
        for code in READ_CODES:
            if code in self.values:
                continue
            if self.supported(code):
                self.read(code)
        for code in (
            vcp.VCP_RED_SAT,
            vcp.VCP_YELLOW_SAT,
            vcp.VCP_GREEN_SAT,
            vcp.VCP_CYAN_SAT,
            vcp.VCP_BLUE_SAT,
            vcp.VCP_MAGENTA_SAT,
            vcp.VCP_COLOR_WEAKNESS,
        ):
            if code not in self.values and self.supported(code):
                self.read(code)
        self.extra_loaded = True

    def refresh(self) -> None:
        for code in READ_CODES:
            if self.supported(code):
                self.read(code)
        for code in (vcp.VCP_RED_SAT, vcp.VCP_YELLOW_SAT, vcp.VCP_GREEN_SAT,
                     vcp.VCP_BLUE_SAT, vcp.VCP_CYAN_SAT, vcp.VCP_MAGENTA_SAT):
            if self.supported(code):
                self.read(code)

    # -------------------------------------------------------------- writing
    def write(self, code: int, value: int) -> bool:
        if value is None:
            return False
        ok = self.handle.set_vcp(code, value)
        if ok:
            old = self.values.get(code)
            self.values[code] = VcpValue(
                code, value, old.maximum if old else 0, old.vcp_type if old else 0
            )
        return ok

    def write_toggle_bit(self, code: int, bit: int, enabled: bool) -> bool:
        current = self.value(code) or 0
        new_value = (current | bit) if enabled else (current & ~bit)
        return self.write(code, new_value)

    def is_on(self, code: int, bit: int) -> bool:
        return bool((self.value(code) or 0) & bit)

    # ---------------------------------------------------------------- modes
    def current_mode_value(self) -> int | None:
        code = vcp.display_mode_code(self.line)
        raw = self.value(code)
        if raw is None:
            return None
        if self.line == vcp.ProductLine.PROART:
            if raw >= 256:
                return raw
            return raw
        return raw & 0xFF

    def current_mode(self) -> vcp.DisplayMode | None:
        current = self.current_mode_value()
        for mode in vcp.mode_set(self.line):
            if mode.value == current:
                return mode
        return None

    def set_mode(self, mode: vcp.DisplayMode) -> bool:
        code = vcp.display_mode_code(self.line)
        value = vcp.encode_display_mode(self.line, mode.value)
        return self.write(code, value)

    # --------------------------------------------------------------- labels
    def input_name(self) -> str:
        value = self.value(vcp.VCP_INPUT)
        return vcp.INPUT_SOURCES.get(value or -1, f"0x{value:02X}" if value is not None else "—")

    def color_temp_name(self, lang: str = "zh") -> str:
        value = self.value(vcp.VCP_COLOR_TEMP)
        for item in vcp.color_temps(self.line, self.model):
            if item.value == value:
                return vcp.color_temp_label(item, lang)
        return f"0x{value:02X}" if value is not None else "—"

    def firmware(self) -> str:
        entry = self.values.get(vcp.VCP_FIRMWARE)
        if not entry:
            return "—"
        return vcp.firmware_string(entry.current, entry.maximum)

    def vcp_version(self) -> str:
        entry = self.values.get(vcp.VCP_VCP_VERSION)
        if not entry:
            return "—"
        return vcp.vcp_version_string(entry.maximum)

    def is_new_vcp(self) -> bool:
        """VCPAPI.GetVCPVersion: version 2 uses the 0xE1/0xFD/0xE2 layout."""
        entry = self.values.get(vcp.VCP_VCP_VERSION)
        if entry is None:
            return True
        return (entry.maximum >> 8) == 2


class MonitorManager:
    """Owns physical monitor handles and serialises all DDC work."""

    def __init__(self) -> None:
        self.states: list[MonitorState] = []
        self._lock = threading.RLock()
        self._handles: list[MonitorHandle] = []

    def scan(self) -> list[MonitorState]:
        from .ddc import enumerate_monitors

        with self._lock:
            self.close()
            self._handles = enumerate_monitors()
            self.states = [MonitorState(handle=h) for h in self._handles]
            return self.states

    def probe_all(self, progress=None) -> list[MonitorState]:
        for index, state in enumerate(self.states):
            if progress:
                progress(index, state)
            state.probe()
        return self.states

    def close(self) -> None:
        for handle in self._handles:
            handle.close()
        self._handles = []
        self.states = []

    def get(self, index: int) -> MonitorState | None:
        if 0 <= index < len(self.states):
            return self.states[index]
        return None


# ---------------------------------------------------------------------------
# Profile persistence
# ---------------------------------------------------------------------------
def capture_profile(state: MonitorState, name: str) -> dict:
    data: dict[str, int] = {}
    for code in PROFILE_CODES:
        if not state.supported(code):
            continue
        entry = state.values.get(code)
        if entry:
            data[str(code)] = entry.current
    return {
        "name": name,
        "model": state.model,
        "line": state.line,
        "created": time.strftime("%Y-%m-%d %H:%M:%S"),
        "vcp": data,
    }


def apply_profile(state: MonitorState, profile: dict) -> tuple[int, int]:
    """Apply a profile; returns (applied, failed)."""
    items = profile.get("vcp", {})
    applied = failed = 0
    ordered = sorted(items.items(), key=lambda kv: 0 if int(kv[0]) == vcp.VCP_DISPLAY_MODE else 1)
    for key, value in ordered:
        code = int(key)
        if state.write(code, int(value)):
            applied += 1
        else:
            failed += 1
        time.sleep(0.02)
    state.refresh()
    return applied, failed


def profile_json(profile: dict) -> str:
    return json.dumps(profile, ensure_ascii=False, indent=2)
