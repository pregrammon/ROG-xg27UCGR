"""Global hotkeys through RegisterHotKey + a dedicated message-loop thread.

The original app installs a low level keyboard hook; a hidden message-only
window and RegisterHotKey is the safe, documented equivalent for this clone.
"""

from __future__ import annotations

import ctypes
import threading
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

MOD_ALT = 0x0001
MOD_CONTROL = 0x0002
MOD_SHIFT = 0x0004
MOD_WIN = 0x0008
MOD_NOREPEAT = 0x4000
WM_HOTKEY = 0x0312
WM_QUIT = 0x0012
VK_UP = 0x26
VK_DOWN = 0x28
VK_LEFT = 0x25
VK_RIGHT = 0x27
VK_B = 0x42
VK_M = 0x4D
VK_V = 0x56

user32.RegisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int, wintypes.UINT, wintypes.UINT]
user32.RegisterHotKey.restype = wintypes.BOOL
user32.UnregisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int]
user32.UnregisterHotKey.restype = wintypes.BOOL
user32.GetMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), wintypes.HWND, wintypes.UINT, wintypes.UINT]
user32.GetMessageW.restype = ctypes.c_int
user32.PostThreadMessageW.argtypes = [wintypes.DWORD, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.PostThreadMessageW.restype = wintypes.BOOL

DEFAULT_BINDINGS = [
    {"action": "brightness_up", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_UP,
     "zh": "亮度 +5", "en": "Brightness +5", "keys": "Ctrl + Alt + ↑"},
    {"action": "brightness_down", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_DOWN,
     "zh": "亮度 -5", "en": "Brightness -5", "keys": "Ctrl + Alt + ↓"},
    {"action": "contrast_up", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_RIGHT,
     "zh": "对比度 +5", "en": "Contrast +5", "keys": "Ctrl + Alt + →"},
    {"action": "contrast_down", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_LEFT,
     "zh": "对比度 -5", "en": "Contrast -5", "keys": "Ctrl + Alt + ←"},
    {"action": "toggle_bluelight", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_B,
     "zh": "切换滤蓝光", "en": "Toggle blue light filter", "keys": "Ctrl + Alt + B"},
    {"action": "cycle_mode", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_M,
     "zh": "切换显示模式", "en": "Cycle display mode", "keys": "Ctrl + Alt + M"},
    {"action": "toggle_mute", "mods": MOD_CONTROL | MOD_ALT, "vk": VK_V,
     "zh": "静音切换", "en": "Toggle mute", "keys": "Ctrl + Alt + V"},
]


class HotkeyManager:
    def __init__(self, callback, bindings=None):
        self.callback = callback
        self.bindings = bindings or DEFAULT_BINDINGS
        self._thread: threading.Thread | None = None
        self._thread_id: int | None = None
        self._ready = threading.Event()
        self.status: dict[str, bool] = {}
        self._running = False

    def start(self) -> bool:
        if self._thread and self._thread.is_alive():
            return True
        self._ready.clear()
        self._thread = threading.Thread(target=self._run, name="dwc-hotkeys", daemon=True)
        self._thread.start()
        self._ready.wait(timeout=2.0)
        return bool(self.status)

    def stop(self) -> None:
        if self._thread_id:
            user32.PostThreadMessageW(self._thread_id, WM_QUIT, 0, 0)
        if self._thread:
            self._thread.join(timeout=1.5)
        self._thread = None
        self._thread_id = None

    def _run(self) -> None:
        self._thread_id = int(kernel32.GetCurrentThreadId())
        registered: list[int] = []
        self.status = {}
        for index, binding in enumerate(self.bindings, start=1):
            ok = bool(
                user32.RegisterHotKey(
                    None, index, binding["mods"] | MOD_NOREPEAT, binding["vk"]
                )
            )
            self.status[binding["action"]] = ok
            if ok:
                registered.append(index)
        self._ready.set()
        self._running = True
        msg = wintypes.MSG()
        try:
            while self._running:
                result = user32.GetMessageW(ctypes.byref(msg), None, 0, 0)
                if result == 0 or result == -1:
                    break
                if msg.message == WM_HOTKEY:
                    index = int(msg.wParam)
                    if 1 <= index <= len(self.bindings):
                        action = self.bindings[index - 1]["action"]
                        try:
                            self.callback(action)
                        except Exception:
                            pass
        finally:
            for index in registered:
                user32.UnregisterHotKey(None, index)
            self._running = False
