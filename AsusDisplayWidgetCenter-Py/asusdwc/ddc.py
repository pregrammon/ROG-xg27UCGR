"""Low level DDC/CI access for Windows monitors (pure ctypes, no third-party deps).

The ASUS DisplayWidgetCenter application talks to monitors through the Windows
Monitor Configuration API (dxva2.dll) + the low level VESA DDC commands exposed
by dxva2/gdi32.  This module reproduces exactly that access path:

    EnumDisplayMonitors -> GetPhysicalMonitorsFromHMONITOR -> hPhysicalMonitor
    GetVCPFeatureAndVCPFeatureReply / SetVCPFeature
    GetCapabilitiesStringLength / CapabilitiesRequestAndCapabilitiesReply

Every DDC transaction is serialised with a global lock and a small inter-command
delay because many monitors (especially ASUS gaming panels) drop back-to-back
I2C requests.
"""

from __future__ import annotations

import ctypes
import threading
import time
from ctypes import wintypes
from dataclasses import dataclass, field

user32 = ctypes.WinDLL("user32", use_last_error=True)
dxva2 = ctypes.WinDLL("dxva2", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", wintypes.LONG),
        ("top", wintypes.LONG),
        ("right", wintypes.LONG),
        ("bottom", wintypes.LONG),
    ]


class MONITORINFOEXW(ctypes.Structure):
    _fields_ = [
        ("cbSize", wintypes.DWORD),
        ("rcMonitor", RECT),
        ("rcWork", RECT),
        ("dwFlags", wintypes.DWORD),
        ("szDevice", wintypes.WCHAR * 32),
    ]


class PHYSICAL_MONITOR(ctypes.Structure):
    _fields_ = [
        ("hPhysicalMonitor", wintypes.HANDLE),
        ("szPhysicalMonitorDescription", wintypes.WCHAR * 128),
    ]


MONITORENUMPROC = ctypes.WINFUNCTYPE(
    wintypes.BOOL,
    wintypes.HMONITOR,
    wintypes.HDC,
    ctypes.POINTER(RECT),
    wintypes.LPARAM,
)

user32.EnumDisplayMonitors.argtypes = [
    wintypes.HDC,
    ctypes.POINTER(RECT),
    MONITORENUMPROC,
    wintypes.LPARAM,
]
user32.EnumDisplayMonitors.restype = wintypes.BOOL
user32.GetMonitorInfoW.argtypes = [wintypes.HMONITOR, ctypes.POINTER(MONITORINFOEXW)]
user32.GetMonitorInfoW.restype = wintypes.BOOL

dxva2.GetNumberOfPhysicalMonitorsFromHMONITOR.argtypes = [
    wintypes.HMONITOR,
    ctypes.POINTER(wintypes.DWORD),
]
dxva2.GetNumberOfPhysicalMonitorsFromHMONITOR.restype = wintypes.BOOL
dxva2.GetPhysicalMonitorsFromHMONITOR.argtypes = [
    wintypes.HMONITOR,
    wintypes.DWORD,
    ctypes.POINTER(PHYSICAL_MONITOR),
]
dxva2.GetPhysicalMonitorsFromHMONITOR.restype = wintypes.BOOL
dxva2.DestroyPhysicalMonitors.argtypes = [
    wintypes.DWORD,
    ctypes.POINTER(PHYSICAL_MONITOR),
]
dxva2.DestroyPhysicalMonitors.restype = wintypes.BOOL
dxva2.GetVCPFeatureAndVCPFeatureReply.argtypes = [
    wintypes.HANDLE,
    wintypes.BYTE,
    ctypes.POINTER(wintypes.DWORD),
    ctypes.POINTER(wintypes.DWORD),
    ctypes.POINTER(wintypes.DWORD),
]
dxva2.GetVCPFeatureAndVCPFeatureReply.restype = wintypes.BOOL
dxva2.SetVCPFeature.argtypes = [wintypes.HANDLE, wintypes.BYTE, wintypes.DWORD]
dxva2.SetVCPFeature.restype = wintypes.BOOL
dxva2.GetCapabilitiesStringLength.argtypes = [
    wintypes.HANDLE,
    ctypes.POINTER(wintypes.DWORD),
]
dxva2.GetCapabilitiesStringLength.restype = wintypes.BOOL
dxva2.CapabilitiesRequestAndCapabilitiesReply.argtypes = [
    wintypes.HANDLE,
    ctypes.c_char_p,
    wintypes.DWORD,
]
dxva2.CapabilitiesRequestAndCapabilitiesReply.restype = wintypes.BOOL


@dataclass
class VcpValue:
    code: int
    current: int
    maximum: int
    vcp_type: int = 0


@dataclass
class MonitorHandle:
    """A physical monitor that can be addressed through DDC/CI."""

    hmonitor: int
    hphysical: int
    description: str
    device_name: str
    rect: tuple[int, int, int, int]
    primary: bool
    index: int
    capabilities: str | None = None
    error: str | None = None
    _closed: bool = field(default=False, repr=False)

    # ------------------------------------------------------------------ caps
    def query_capabilities(self, retries: int = 5) -> str | None:
        length = wintypes.DWORD(0)
        ok = False
        for attempt in range(retries):
            if attempt:
                time.sleep(0.06)
            ok = bool(dxva2.GetCapabilitiesStringLength(self.hphysical, ctypes.byref(length)))
            if ok and length.value:
                break
        if not ok or not length.value:
            self.error = "capabilities unavailable"
            return None
        buf = ctypes.create_string_buffer(int(length.value) + 1)
        if not dxva2.CapabilitiesRequestAndCapabilitiesReply(
            self.hphysical, buf, int(length.value)
        ):
            self.error = "capabilities read failed"
            return None
        text = buf.value.decode("ascii", "replace").strip("\x00")
        self.capabilities = text
        return text

    # ------------------------------------------------------------------- vcp
    def get_vcp(self, code: int, retries: int = 2) -> VcpValue | None:
        code = int(code) & 0xFF
        current = wintypes.DWORD(0)
        maximum = wintypes.DWORD(0)
        vcp_type = wintypes.DWORD(0)
        for attempt in range(retries):
            with _DDC_LOCK:
                _pace()
                ok = bool(
                    dxva2.GetVCPFeatureAndVCPFeatureReply(
                        self.hphysical,
                        code,
                        ctypes.byref(vcp_type),
                        ctypes.byref(current),
                        ctypes.byref(maximum),
                    )
                )
            if ok:
                return VcpValue(code, int(current.value), int(maximum.value), int(vcp_type.value))
            if attempt + 1 < retries:
                time.sleep(0.03)
        return None

    def set_vcp(self, code: int, value: int, retries: int = 2) -> bool:
        code = int(code) & 0xFF
        value = int(value) & 0xFFFFFFFF
        for attempt in range(retries):
            with _DDC_LOCK:
                _pace()
                ok = bool(dxva2.SetVCPFeature(self.hphysical, code, value))
            if ok:
                return True
            if attempt + 1 < retries:
                time.sleep(0.04)
        return False

    # --------------------------------------------------------------- utility
    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            arr = (PHYSICAL_MONITOR * 1)()
            arr[0].hPhysicalMonitor = self.hphysical
            arr[0].szPhysicalMonitorDescription = self.description
            with _DDC_LOCK:
                dxva2.DestroyPhysicalMonitors(1, arr)
        except Exception:
            pass

    @property
    def width(self) -> int:
        return max(0, self.rect[2] - self.rect[0])

    @property
    def height(self) -> int:
        return max(0, self.rect[3] - self.rect[1])


_DDC_LOCK = threading.RLock()
_LAST_CALL = 0.0
# The original app sleeps 20 ms before every VCP transaction (VCPAPI.Set_Time).
# We keep a slightly larger gap so slider drags / profile bursts stay reliable.
_PACE_SECONDS = 0.032


def _pace() -> None:
    global _LAST_CALL
    now = time.monotonic()
    delta = now - _LAST_CALL
    if delta < _PACE_SECONDS:
        time.sleep(_PACE_SECONDS - delta)
    _LAST_CALL = time.monotonic()


def enumerate_monitors() -> list[MonitorHandle]:
    """Enumerate every physical monitor attached to the current desktop."""
    hmons: list[int] = []

    def _callback(hmon, hdc, lprect, data):
        hmons.append(int(hmon))
        return True

    callback = MONITORENUMPROC(_callback)
    if not user32.EnumDisplayMonitors(None, None, callback, 0):
        raise OSError(ctypes.get_last_error(), "EnumDisplayMonitors failed")

    monitors: list[MonitorHandle] = []
    for index, hmon in enumerate(hmons):
        info = MONITORINFOEXW()
        info.cbSize = ctypes.sizeof(MONITORINFOEXW)
        device = ""
        rect = (0, 0, 0, 0)
        primary = False
        if user32.GetMonitorInfoW(hmon, ctypes.byref(info)):
            device = info.szDevice
            rect = (
                info.rcMonitor.left,
                info.rcMonitor.top,
                info.rcMonitor.right,
                info.rcMonitor.bottom,
            )
            primary = bool(info.dwFlags & 1)

        count = wintypes.DWORD(0)
        if not dxva2.GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, ctypes.byref(count)):
            continue
        if count.value == 0:
            continue
        array = (PHYSICAL_MONITOR * count.value)()
        if not dxva2.GetPhysicalMonitorsFromHMONITOR(hmon, count.value, array):
            continue
        for sub in range(count.value):
            entry = array[sub]
            monitors.append(
                MonitorHandle(
                    hmonitor=hmon,
                    hphysical=int(entry.hPhysicalMonitor),
                    description=entry.szPhysicalMonitorDescription,
                    device_name=device,
                    rect=rect,
                    primary=primary,
                    index=len(monitors),
                )
            )
    return monitors
