"""Visual theme for the tkinter reconstruction (ROG-inspired dark UI)."""

from __future__ import annotations

import ctypes
import tkinter as tk
from tkinter import font as tkfont
from tkinter import ttk

COLORS = {
    "bg": "#0C0E12",
    "sidebar": "#101319",
    "panel": "#141821",
    "card": "#181C25",
    "card_hover": "#1E2330",
    "card_active": "#2A1119",
    "border": "#262C38",
    "border_soft": "#1F242E",
    "text": "#E9ECF1",
    "text_dim": "#A7AFBF",
    "muted": "#727C90",
    "accent": "#FF0050",
    "accent_hover": "#FF2D6B",
    "accent_soft": "#3A0A18",
    "cyan": "#39C6FF",
    "green": "#3DDC97",
    "amber": "#FFB020",
    "red": "#FF4D5E",
    "track": "#2A303C",
    "white": "#FFFFFF",
}


def ui_font(size: int = 10, weight: str = "normal") -> tuple:
    family = _font_family()
    return (family, size, weight)


_FAMILY = None


def _font_family() -> str:
    global _FAMILY
    if _FAMILY:
        return _FAMILY
    try:
        families = set(tkfont.families())
    except Exception:
        families = set()
    for candidate in ("Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI"):
        if candidate in families:
            _FAMILY = candidate
            break
    else:
        _FAMILY = "TkDefaultFont"
    return _FAMILY


def mono_font(size: int = 9) -> tuple:
    try:
        families = set(tkfont.families())
    except Exception:
        families = set()
    for candidate in ("Cascadia Mono", "Consolas", "Courier New"):
        if candidate in families:
            return (candidate, size)
    return ("TkFixedFont", size)


def enable_dpi_awareness() -> None:
    try:
        # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
    except Exception:
        try:
            ctypes.windll.shcore.SetProcessDpiAwareness(1)
        except Exception:
            pass


def dark_title_bar(root: tk.Misc, dark: bool = True) -> None:
    """Ask DWM to paint the native title bar dark (Windows 10/11)."""
    try:
        hwnd = ctypes.windll.user32.GetParent(root.winfo_id())
        value = ctypes.c_int(1 if dark else 0)
        for attr in (20, 19):
            if ctypes.windll.dwmapi.DwmSetWindowAttribute(
                hwnd, attr, ctypes.byref(value), ctypes.sizeof(value)
            ) == 0:
                break
    except Exception:
        pass


def apply_theme(root: tk.Tk) -> ttk.Style:
    style = ttk.Style(root)
    try:
        style.theme_use("clam")
    except tk.TclError:
        pass

    root.configure(bg=COLORS["bg"])
    root.option_add("*Font", ui_font(10))
    root.option_add("*Background", COLORS["bg"])
    root.option_add("*Foreground", COLORS["text"])
    root.option_add("*selectBackground", COLORS["accent"])
    root.option_add("*selectForeground", COLORS["white"])
    root.option_add("*TCombobox*Listbox.background", COLORS["panel"])
    root.option_add("*TCombobox*Listbox.foreground", COLORS["text"])
    root.option_add("*TCombobox*Listbox.selectBackground", COLORS["accent"])
    root.option_add("*TCombobox*Listbox.selectForeground", COLORS["white"])
    root.option_add("*TCombobox*Listbox.font", ui_font(10))

    style.configure(
        ".",
        background=COLORS["bg"],
        foreground=COLORS["text"],
        fieldbackground=COLORS["panel"],
        bordercolor=COLORS["border"],
        lightcolor=COLORS["border"],
        darkcolor=COLORS["border"],
        troughcolor=COLORS["track"],
        focuscolor=COLORS["accent"],
        font=ui_font(10),
    )
    style.configure(
        "TCombobox",
        background=COLORS["panel"],
        fieldbackground=COLORS["panel"],
        foreground=COLORS["text"],
        arrowcolor=COLORS["text_dim"],
        bordercolor=COLORS["border"],
        padding=(10, 6),
    )
    style.map(
        "TCombobox",
        fieldbackground=[("readonly", COLORS["panel"]), ("disabled", COLORS["card"])],
        foreground=[("disabled", COLORS["muted"])],
        bordercolor=[("focus", COLORS["accent"]), ("hover", COLORS["border"])],
    )
    style.configure(
        "Vertical.TScrollbar",
        background=COLORS["card"],
        troughcolor=COLORS["bg"],
        bordercolor=COLORS["bg"],
        arrowcolor=COLORS["muted"],
        width=9,
        arrowsize=9,
    )
    style.map("Vertical.TScrollbar", background=[("active", COLORS["border"])])
    style.configure("TSeparator", background=COLORS["border"])
    return style
