"""Settings, profile and autostart persistence."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

APP_NAME = "AsusDWC-Py"
APP_VERSION = "1.0.0"


def data_dir() -> Path:
    base = os.environ.get("APPDATA") or str(Path.home())
    path = Path(base) / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def settings_path() -> Path:
    return data_dir() / "settings.json"


def profiles_path() -> Path:
    return data_dir() / "profiles.json"


DEFAULT_SETTINGS = {
    "language": "",          # "" -> auto (system UI language)
    "topmost": False,
    "autostart": False,
    "hotkeys": True,
    "monitor_key": "",
    "last_page": "overview",
    "geometry": "",
}


def default_language() -> str:
    try:
        import ctypes

        lang_id = ctypes.windll.kernel32.GetUserDefaultUILanguage()
        if (lang_id & 0x3FF) == 0x04:  # Chinese
            return "zh"
    except Exception:
        pass
    return "en"


def load_settings() -> dict:
    data = dict(DEFAULT_SETTINGS)
    try:
        raw = settings_path().read_text(encoding="utf-8")
        loaded = json.loads(raw)
        if isinstance(loaded, dict):
            data.update({k: v for k, v in loaded.items() if k in DEFAULT_SETTINGS})
    except Exception:
        pass
    if not data.get("language"):
        data["language"] = default_language()
    return data


def save_settings(data: dict) -> None:
    try:
        settings_path().write_text(
            json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
        )
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Profiles
# ---------------------------------------------------------------------------
def load_profiles() -> list[dict]:
    try:
        raw = profiles_path().read_text(encoding="utf-8")
        data = json.loads(raw)
        if isinstance(data, list):
            return [item for item in data if isinstance(item, dict)]
    except Exception:
        pass
    return []


def save_profiles(profiles: list[dict]) -> None:
    try:
        profiles_path().write_text(
            json.dumps(profiles, ensure_ascii=False, indent=2), encoding="utf-8"
        )
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Autostart (HKCU\...\Run)
# ---------------------------------------------------------------------------
RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
RUN_VALUE = "ASUSDisplayWidgetCenterPy"


def _autostart_command() -> str:
    script = Path(__file__).resolve().parent.parent / "main.py"
    exe = Path(sys.executable)
    pythonw = exe.with_name("pythonw.exe")
    if pythonw.exists():
        exe = pythonw
    return f'"{exe}" "{script}"'


def is_autostart_enabled() -> bool:
    try:
        import winreg

        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            value, _ = winreg.QueryValueEx(key, RUN_VALUE)
            return bool(value)
    except Exception:
        return False


def set_autostart(enabled: bool) -> bool:
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE
        ) as key:
            if enabled:
                winreg.SetValueEx(key, RUN_VALUE, 0, winreg.REG_SZ, _autostart_command())
            else:
                try:
                    winreg.DeleteValue(key, RUN_VALUE)
                except FileNotFoundError:
                    pass
        return True
    except Exception:
        return False
