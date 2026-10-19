"""Scan raw PE bytes for UTF-8 metadata names and UTF-16 user strings."""
import sys
import re

KEYS = [
    "FrameRateBoost", "SetToggleSettingsItem", "SetFrameRateBoost",
    "ToggleSettings", "VCP", "SetVCPFeature", "GetVCPFeature",
    "Brightness", "Contrast", "InputSource", "DisplayMode", "HDR",
    "DDC", "Ddc", "DDC/CI", "MonitorHandle", "PhysicalMonitor",
    "ColorTemperature", "Overdrive", "BlueLight", "GameVisual",
    "TraceFree", "ELMB", "AdaptiveSync", "VRR", "VariableRefresh",
    "0xFD", "0x10", "0xF0", "DisplayWidgetCenter",
]


def scan(path):
    data = open(path, "rb").read()
    print(f"== {path} ({len(data)} bytes)")
    # UTF-8 metadata names, printable ASCII runs
    for m in re.finditer(rb"[ -~]{4,}", data):
        s = m.group().decode("ascii", "ignore")
        for k in KEYS:
            if k in s:
                print(f"  UTF8  0x{m.start():08x} {s[:160]}")
                break
    # UTF-16LE strings
    for m in re.finditer(rb"(?:[ -~]\x00){4,}", data):
        s = m.group().decode("utf-16le", "ignore")
        for k in KEYS:
            if k in s:
                print(f"  UTF16 0x{m.start():08x} {s[:160]}")
                break


if __name__ == "__main__":
    for p in sys.argv[1:]:
        scan(p)
