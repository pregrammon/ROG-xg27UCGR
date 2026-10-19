#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ASUS DisplayWidget Center —— Python/tkinter 简化复刻版（仅针对 ROG XG27UCGR）

逆向依据（静态分析 DisplayWidgetCenter.exe 1.5.1.3, SHA-256 d9d6d043...790f）：

  * DisplayWidgetCenter.VCPAPI.SetVCPFeatureInternal(monitor, code, value)
      -> DisplayWidgetCenter.DDCMethods.SetVCPFeature  (dxva2/monitorconfigapi)
  * DisplayWidgetCenter.VCPAPI.SetFrameRateBoost(monitor, enable, newVcp)
        IL: ldc.i4 253 (0xFD)  + ldc.i4 2048 (0x800) / ldc.i4 256 (0x100)
      -> SetToggleSettingsItem 对 VCP 0xFD 做 read-modify-write：
         value = cur | mask（开） / cur & ~mask（关）
  * DisplayWidgetCenter.VCPAPI.GetVCPVersion 读取 VCP 0xEF 低字节作为版本
  * 亮度走标准 VCP 0x10（README 与 DDCMethods 语义一致）

本脚本沿用仓库 Platform/RogScreenDualMode.cs 中已在 XG27UCGR 实机验证的写法：
  - 1K（像素合并, 1920x1080@485Hz）：VCP 0xFD = 0x128
  - 4K（3840x2160@162Hz）        ：VCP 0xFD = 0x001
  - 亮度：VCP 0x10，标准 Get/SetMonitorBrightness

依赖：仅 Python 标准库（tkinter + ctypes），Windows 10/11 x64。

用法：
    python display_widget_center.py            # 启动 GUI
    python display_widget_center.py --probe    # 只读探针（不写显示器）
    python display_widget_center.py --selftest # 建窗 1 秒后自动关闭，验证环境
"""

from __future__ import annotations

import ctypes
import os
import queue
import sys
import threading
import time
import tkinter as tk
from ctypes import wintypes

# --------------------------------------------------------------------------
# 常量
# --------------------------------------------------------------------------

VCP_BRIGHTNESS = 0x10
VCP_DUAL_MODE = 0xFD

MODE_1K_VALUE = 0x128   # FrameRateBoost 开（bit 8）+ 辅助位，VRR 关
MODE_4K_VALUE = 0x001   # VRR 开，FrameRateBoost 关

WIDTH_4K, HEIGHT_4K = 3840, 2160
WIDTH_1K, HEIGHT_1K = 1920, 1080

MONITOR_DEFAULTTOPRIMARY = 0x00000001
PHYSICAL_MONITOR_SIZE = 264  # x64: HANDLE(8) + WCHAR[128](256)

# --------------------------------------------------------------------------
# Win32 / DDC-CI
# --------------------------------------------------------------------------

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.GetDesktopWindow.restype = wintypes.HWND
user32.MonitorFromWindow.argtypes = [wintypes.HWND, wintypes.DWORD]
user32.MonitorFromWindow.restype = wintypes.HANDLE
user32.GetSystemMetrics.argtypes = [ctypes.c_int]
user32.GetSystemMetrics.restype = ctypes.c_int


def _set_dpi_aware() -> None:
    """让 GetSystemMetrics 返回真实像素，而不是 DPI 虚拟化后的值。"""
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_AWARE
        return
    except Exception:
        pass
    try:
        user32.SetProcessDPIAware()
    except Exception:
        pass


def _load_ddc_library():
    """monitorconfigapi.dll (Win8+) 优先，dxva2.dll 兜底。"""
    system_dir = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32")
    for name in ("monitorconfigapi.dll", "dxva2.dll"):
        path = os.path.join(system_dir, name)
        if os.path.exists(path):
            return name, ctypes.WinDLL(path, use_last_error=True)
    raise OSError("系统中找不到 monitorconfigapi.dll / dxva2.dll")


class MonitorError(RuntimeError):
    """DDC/CI 调用失败。"""


class DdcMonitor:
    """主显示器的物理句柄封装（与 C# 版 RogScreenNativeMethods 等价）。"""

    def __init__(self) -> None:
        self.dll_name, self.lib = _load_ddc_library()
        self._bind()
        self.handle = wintypes.HANDLE()
        self.description = ""
        self.minimum = 0
        self.maximum = 100
        self._open()

    # ---- 原型声明 ----
    def _bind(self) -> None:
        lib = self.lib
        lib.GetNumberOfPhysicalMonitorsFromHMONITOR.argtypes = [
            wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)
        ]
        lib.GetNumberOfPhysicalMonitorsFromHMONITOR.restype = wintypes.BOOL
        lib.GetPhysicalMonitorsFromHMONITOR.argtypes = [
            wintypes.HANDLE, wintypes.DWORD, ctypes.c_void_p
        ]
        lib.GetPhysicalMonitorsFromHMONITOR.restype = wintypes.BOOL
        lib.DestroyPhysicalMonitors.argtypes = [wintypes.DWORD, ctypes.c_void_p]
        lib.DestroyPhysicalMonitors.restype = wintypes.BOOL
        lib.GetMonitorBrightness.argtypes = [
            wintypes.HANDLE,
            ctypes.POINTER(wintypes.DWORD),
            ctypes.POINTER(wintypes.DWORD),
            ctypes.POINTER(wintypes.DWORD),
        ]
        lib.GetMonitorBrightness.restype = wintypes.BOOL
        lib.SetMonitorBrightness.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        lib.SetMonitorBrightness.restype = wintypes.BOOL
        lib.GetVCPFeatureAndVCPFeatureReply.argtypes = [
            wintypes.HANDLE, ctypes.c_ubyte, ctypes.c_void_p,
            ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(wintypes.DWORD),
        ]
        lib.GetVCPFeatureAndVCPFeatureReply.restype = wintypes.BOOL
        lib.SetVCPFeature.argtypes = [wintypes.HANDLE, ctypes.c_ubyte, wintypes.DWORD]
        lib.SetVCPFeature.restype = wintypes.BOOL

    # ---- 句柄 ----
    def _open(self) -> None:
        hmon = user32.MonitorFromWindow(user32.GetDesktopWindow(), MONITOR_DEFAULTTOPRIMARY)
        if not hmon:
            raise MonitorError("无法获取主显示器句柄")

        count = wintypes.DWORD(0)
        if not self.lib.GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, ctypes.byref(count)) or count.value == 0:
            raise MonitorError("未找到物理显示器（DDC/CI 是否开启？）")

        buf = ctypes.create_string_buffer(PHYSICAL_MONITOR_SIZE * count.value)
        if not self.lib.GetPhysicalMonitorsFromHMONITOR(hmon, count.value, buf):
            raise MonitorError("GetPhysicalMonitorsFromHMONITOR 失败")

        handles = [
            ctypes.cast(ctypes.byref(buf, i * PHYSICAL_MONITOR_SIZE),
                        ctypes.POINTER(wintypes.HANDLE)).contents.value
            for i in range(count.value)
        ]
        self.handle = wintypes.HANDLE(handles[0])
        self.description = ctypes.wstring_at(
            ctypes.byref(buf, 8), 128).split("\x00", 1)[0]

        for extra in handles[1:]:
            self._destroy_one(extra)

        self._read_range()

    def _destroy_one(self, handle) -> None:
        if not handle:
            return
        slot = ctypes.create_string_buffer(PHYSICAL_MONITOR_SIZE)
        ctypes.cast(slot, ctypes.POINTER(wintypes.HANDLE))[0] = handle
        try:
            self.lib.DestroyPhysicalMonitors(1, slot)
        except Exception:
            pass

    def close(self) -> None:
        h = getattr(self, "handle", None)
        if h is not None and h.value:
            self._destroy_one(h.value)
            self.handle = wintypes.HANDLE()

    def reopen(self) -> None:
        """双模切换会触发 EDID 重协商，旧句柄可能失效，需要重新打开。"""
        self.close()
        self._open()

    # ---- 亮度 ----
    def _read_range(self) -> None:
        mn, cur, mx = (wintypes.DWORD() for _ in range(3))
        if not self.lib.GetMonitorBrightness(
                self.handle, ctypes.byref(mn), ctypes.byref(cur), ctypes.byref(mx)):
            raise MonitorError("读取显示器亮度范围失败（显示器可能不支持 DDC/CI）")
        self.minimum, self.maximum = mn.value, mx.value
        self._current = cur.value

    def brightness_percent(self) -> int:
        mn, cur, mx = (wintypes.DWORD() for _ in range(3))
        if self.lib.GetMonitorBrightness(
                self.handle, ctypes.byref(mn), ctypes.byref(cur), ctypes.byref(mx)):
            self.minimum, self.maximum = mn.value, mx.value
            return self._to_percent(cur.value)
        # 句柄可能因模式切换而失效，重开一次再试
        self.reopen()
        if self.lib.GetMonitorBrightness(
                self.handle, ctypes.byref(mn), ctypes.byref(cur), ctypes.byref(mx)):
            return self._to_percent(cur.value)
        raise MonitorError("无法读取显示器亮度")

    def set_brightness_percent(self, percent: int) -> None:
        percent = max(0, min(100, int(percent)))
        value = round(self.minimum + (self.maximum - self.minimum) * percent / 100.0)
        if self.lib.SetMonitorBrightness(self.handle, value):
            return
        self.reopen()
        if not self.lib.SetMonitorBrightness(self.handle, value):
            raise MonitorError("调节亮度失败（DDC/CI 被拒绝）")

    def _to_percent(self, value: int) -> int:
        if self.maximum <= self.minimum:
            return 100
        return max(0, min(100, round((value - self.minimum) * 100.0 /
                                     (self.maximum - self.minimum))))

    # ---- 厂商 VCP ----
    def set_vcp(self, code: int, value: int) -> None:
        if not self.lib.SetVCPFeature(self.handle, code, value):
            raise MonitorError(f"写入 VCP 0x{code:02X} = 0x{value:X} 失败")
        time.sleep(3.0)  # 面板重新握手（会短暂黑屏）

    def read_vcp(self, code: int) -> tuple[bool, int, int]:
        cur, mx = wintypes.DWORD(), wintypes.DWORD()
        ok = bool(self.lib.GetVCPFeatureAndVCPFeatureReply(
            self.handle, code, None, ctypes.byref(cur), ctypes.byref(mx)))
        return ok, cur.value, mx.value


# --------------------------------------------------------------------------
# 画面状态
# --------------------------------------------------------------------------


def current_resolution() -> tuple[int, int]:
    return user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)


def current_mode() -> str:
    """返回 '4k' / '1k' / 'other'。"""
    w, h = current_resolution()
    if (w, h) == (WIDTH_4K, HEIGHT_4K):
        return "4k"
    if (w, h) == (WIDTH_1K, HEIGHT_1K):
        return "1k"
    return "other"


def mode_text(mode: str) -> str:
    if mode == "4k":
        return "4K 3840×2160 @162Hz"
    if mode == "1k":
        return "1K 1920×1080 @485Hz"
    w, h = current_resolution()
    return f"其他分辨率 {w}×{h}"


# --------------------------------------------------------------------------
# GUI
# --------------------------------------------------------------------------


class App:
    BG = "#1b1b1f"
    FG = "#e8e8ea"
    ACCENT = "#e23b3b"
    MUTED = "#9a9aa2"

    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        self.ddc: DdcMonitor | None = None
        self._pending_apply: str | None = None
        self._busy = False
        self._ui_queue: "queue.Queue" = queue.Queue()

        # 后台线程 -> 主线程的唯一通道（tkinter 不是线程安全的）
        self.root.after(50, self._pump)

        root.title("ROG 屏幕控制 · Python")
        root.configure(bg=self.BG)
        root.resizable(False, False)
        root.attributes("-topmost", True)

        self._build_ui()
        self._init_backend()

    # ---- UI ----
    def _build_ui(self) -> None:
        pad = {"padx": 16, "pady": 6}

        header = tk.Label(self.root, text="ROG XG27UCGR", bg=self.BG, fg=self.ACCENT,
                          font=("Segoe UI", 14, "bold"))
        header.pack(anchor="w", padx=16, pady=(12, 0))

        self.monitor_label = tk.Label(self.root, text="正在连接显示器…", bg=self.BG,
                                      fg=self.MUTED, font=("Segoe UI", 8))
        self.monitor_label.pack(anchor="w", padx=16)

        # 亮度
        row = tk.Frame(self.root, bg=self.BG)
        row.pack(fill="x", **pad)
        tk.Label(row, text="亮度", bg=self.BG, fg=self.FG,
                 font=("Segoe UI", 10)).pack(side="left")

        self.brightness_var = tk.IntVar(value=50)
        self.scale = tk.Scale(
            row, from_=0, to=100, orient="horizontal", variable=self.brightness_var,
            command=self._on_scale, showvalue=False, length=240, width=16,
            sliderlength=20, bg=self.BG, fg=self.FG, troughcolor="#3a3a42",
            highlightthickness=0, sliderrelief="raised", borderwidth=1,
            activebackground=self.ACCENT,
        )
        self.scale.pack(side="left", expand=True, fill="x", padx=(10, 8))

        self.brightness_label = tk.Label(row, text="--%", width=4, anchor="e",
                                         bg=self.BG, fg=self.FG, font=("Consolas", 10))
        self.brightness_label.pack(side="right")

        # 双模
        mode_row = tk.Frame(self.root, bg=self.BG)
        mode_row.pack(fill="x", padx=16, pady=(8, 4))

        self.mode_label = tk.Label(mode_row, text="当前模式：检测中…", bg=self.BG,
                                   fg=self.FG, font=("Segoe UI", 10))
        self.mode_label.pack(anchor="w")

        self.mode_button = tk.Button(
            mode_row, text="切换到 1K 485Hz", command=self._on_switch_mode,
            bg="#2a2a31", fg=self.FG, activebackground=self.ACCENT,
            activeforeground="white", relief="flat", font=("Segoe UI", 10, "bold"),
            padx=12, pady=6, cursor="hand2",
        )
        self.mode_button.pack(fill="x", pady=(8, 0))

        # 底部
        bottom = tk.Frame(self.root, bg=self.BG)
        bottom.pack(fill="x", padx=16, pady=(8, 12))

        self.status_label = tk.Label(bottom, text="就绪", bg=self.BG, fg=self.MUTED,
                                     font=("Segoe UI", 8), anchor="w", wraplength=300)
        self.status_label.pack(side="left", fill="x", expand=True)

        self.refresh_button = tk.Button(bottom, text="刷新", command=self.refresh,
                                        bg=self.BG, fg=self.MUTED, relief="flat",
                                        font=("Segoe UI", 8), cursor="hand2")
        self.refresh_button.pack(side="right")

    # ---- 线程调度 ----
    def _post(self, fn) -> None:
        """从后台线程提交一段回调，由主线程的 _pump 执行。"""
        self._ui_queue.put(fn)

    def _pump(self) -> None:
        try:
            while True:
                self._ui_queue.get_nowait()()
        except queue.Empty:
            pass
        self.root.after(50, self._pump)

    # ---- 后端 ----
    def _init_backend(self) -> None:
        def work() -> None:
            try:
                ddc = DdcMonitor()
            except Exception as exc:  # noqa: BLE001
                self._post(lambda e=exc: self._backend_failed(str(e)))
                return
            self._post(lambda d=ddc: self._backend_ready(d))

        threading.Thread(target=work, daemon=True).start()

    def _backend_ready(self, ddc: DdcMonitor) -> None:
        self.ddc = ddc
        self.monitor_label.config(text=f"DDC/CI: {ddc.description or '主显示器'} ({ddc.dll_name})")
        try:
            pct = ddc.brightness_percent()
        except Exception as exc:  # noqa: BLE001
            self._set_status(f"亮度不可用：{exc}", error=True)
            self.scale.config(state="disabled")
        else:
            self.brightness_var.set(pct)
            self.brightness_label.config(text=f"{pct}%")
        self._update_mode()

    def _backend_failed(self, message: str) -> None:
        self._set_status(f"显示器初始化失败：{message}", error=True)
        self.scale.config(state="disabled")
        self.mode_button.config(state="disabled")
        self.mode_label.config(text="当前模式：未知")

    # ---- 亮度 ----
    def _on_scale(self, value: str) -> None:
        pct = int(float(value))
        self.brightness_label.config(text=f"{pct}%")
        if self._pending_apply is not None:
            self.root.after_cancel(self._pending_apply)
        self._pending_apply = self.root.after(150, self._apply_brightness)

    def _apply_brightness(self) -> None:
        self._pending_apply = None
        if self.ddc is None:
            return
        pct = self.brightness_var.get()

        def work() -> None:
            try:
                self.ddc.set_brightness_percent(pct)
            except Exception as exc:  # noqa: BLE001
                self._post(lambda e=exc: self._set_status(f"亮度写入失败：{e}", error=True))

        threading.Thread(target=work, daemon=True).start()

    # ---- 双模 ----
    def _update_mode(self) -> None:
        mode = current_mode()
        self.mode_label.config(text=f"当前模式：{mode_text(mode)}")
        if mode == "4k":
            self.mode_button.config(text="切换到 1K 485Hz")
        elif mode == "1k":
            self.mode_button.config(text="切换到 4K 162Hz")
        else:
            self.mode_button.config(text="切换到 1K 485Hz")

    def _on_switch_mode(self) -> None:
        if self.ddc is None or self._busy:
            return
        mode = current_mode()
        if mode == "1k":
            target, value, want = "4k", MODE_4K_VALUE, (WIDTH_4K, HEIGHT_4K)
        else:
            target, value, want = "1k", MODE_1K_VALUE, (WIDTH_1K, HEIGHT_1K)

        self._busy = True
        self.mode_button.config(state="disabled")
        self._set_status("正在切换，面板会短暂黑屏…")

        def work() -> None:
            error = None
            try:
                self.ddc.set_vcp(VCP_DUAL_MODE, value)
                deadline = time.time() + 12
                while time.time() < deadline:
                    if current_resolution() == want:
                        break
                    time.sleep(0.5)
            except Exception as exc:  # noqa: BLE001
                error = exc

            def done() -> None:
                self._busy = False
                self.mode_button.config(state="normal")
                if error is not None:
                    self._set_status(f"切换失败：{error}", error=True)
                else:
                    self._update_mode()
                    self._set_status("切换完成")
                # 新链路可能换了句柄，刷新亮度
                self.refresh()

            self._post(done)

        threading.Thread(target=work, daemon=True).start()

    # ---- 状态 ----
    def _set_status(self, text: str, error: bool = False) -> None:
        self.status_label.config(text=text, fg=self.ACCENT if error else self.MUTED)

    def refresh(self) -> None:
        if self.ddc is None:
            return
        try:
            pct = self.ddc.brightness_percent()
        except Exception as exc:  # noqa: BLE001
            self._set_status(f"刷新失败：{exc}", error=True)
        else:
            self.brightness_var.set(pct)
            self.brightness_label.config(text=f"{pct}%")
        self._update_mode()


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def probe() -> int:
    print(f"Python {sys.version.split()[0]}  |  {sys.platform}")
    try:
        ddc = DdcMonitor()
    except Exception as exc:  # noqa: BLE001
        print(f"[FAIL] {exc}")
        return 1

    try:
        print(f"[OK] DLL            : {ddc.dll_name}")
        print(f"[OK] 物理显示器     : {ddc.description!r}")
        print(f"[OK] 亮度范围       : {ddc.minimum}..{ddc.maximum}")
        try:
            print(f"[OK] 当前亮度       : {ddc.brightness_percent()}%")
        except Exception as exc:  # noqa: BLE001
            print(f"[WARN] 读取亮度失败 : {exc}")

        w, h = current_resolution()
        print(f"[OK] 当前分辨率     : {w}x{h} ({current_mode()})")

        ok, cur, mx = ddc.read_vcp(VCP_DUAL_MODE)
        print(f"[{'OK' if ok else 'WARN'}] VCP 0xFD 读取 : ok={ok} cur=0x{cur:X} max=0x{mx:X}"
              "（该显示器读 VCP 经常失败，属正常）")
        ok, cur, mx = ddc.read_vcp(VCP_BRIGHTNESS)
        print(f"[{'OK' if ok else 'WARN'}] VCP 0x10 读取 : ok={ok} cur={cur} max={mx}")
    finally:
        ddc.close()
    return 0


def switch_cli(target: str) -> int:
    """命令行切换双模，便于不启动 GUI 时验证：--switch 1k / --switch 4k"""
    target = target.lower()
    if target not in ("1k", "4k"):
        print("用法：--switch 1k|4k")
        return 2

    value = MODE_1K_VALUE if target == "1k" else MODE_4K_VALUE
    want = (WIDTH_1K, HEIGHT_1K) if target == "1k" else (WIDTH_4K, HEIGHT_4K)
    print(f"目标 {target}：写入 VCP 0xFD = 0x{value:X}（面板会短暂黑屏）…")

    try:
        ddc = DdcMonitor()
    except Exception as exc:  # noqa: BLE001
        print(f"[FAIL] {exc}")
        return 1

    try:
        ddc.set_vcp(VCP_DUAL_MODE, value)
    finally:
        ddc.close()

    deadline = time.time() + 12
    while time.time() < deadline and current_resolution() != want:
        time.sleep(0.5)

    w, h = current_resolution()
    print(f"当前分辨率：{w}x{h} ({current_mode()})")
    return 0 if (w, h) == want else 1


def selftest() -> int:
    root = tk.Tk()
    App(root)
    root.after(1200, root.destroy)
    root.mainloop()
    print("[OK] tkinter 窗口创建/销毁正常")
    return 0


def main() -> int:
    # 管道/重定向时避免 GBK 乱码
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

    _set_dpi_aware()
    if "--probe" in sys.argv:
        return probe()
    if "--switch" in sys.argv:
        idx = sys.argv.index("--switch")
        return switch_cli(sys.argv[idx + 1] if idx + 1 < len(sys.argv) else "")
    if "--selftest" in sys.argv:
        return selftest()

    root = tk.Tk()
    App(root)
    root.mainloop()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
