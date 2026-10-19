#!/usr/bin/env python3
"""Entry point for the ASUS DisplayWidget Center Python reconstruction."""

from __future__ import annotations

import argparse
import ctypes
import sys
from ctypes import wintypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from asusdwc.app import App  # noqa: E402

_MUTEX = None


def _acquire_single_instance():
    """Match the original app's global mutex (one DWC instance per session)."""
    global _MUTEX
    try:
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.CreateMutexW.argtypes = [ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR]
        kernel32.CreateMutexW.restype = wintypes.HANDLE
        handle = kernel32.CreateMutexW(None, False, "Local\\AsusDisplayWidgetCenterPy")
        if not handle:
            return True
        if ctypes.get_last_error() == 183:  # ERROR_ALREADY_EXISTS
            ctypes.windll.user32.MessageBoxW(
                0,
                "ASUS DisplayWidget Center (Python) 已经在运行。\n"
                "ASUS DisplayWidget Center (Python) is already running.",
                "DisplayWidget Center",
                0x40,
            )
            return False
        _MUTEX = handle
        return True
    except Exception:
        return True


def main() -> int:
    parser = argparse.ArgumentParser(description="ASUS DisplayWidget Center - Python reconstruction")
    parser.add_argument("--page", default=None, help="start page: overview/display/eyecare/gameplus/osd/profiles/settings")
    parser.add_argument("--lang", default=None, choices=["zh", "en"], help="UI language")
    parser.add_argument("--scroll-bottom", action="store_true", help="open with the page scrolled to the bottom (debug)")
    args = parser.parse_args()
    if not _acquire_single_instance():
        return 0
    app = App(initial_page=args.page, language=args.lang, scroll_bottom=args.scroll_bottom)
    app.mainloop()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
