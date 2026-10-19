#!/usr/bin/env python3
"""Command line diagnostics for the reconstruction (no GUI required).

Usage:
    python cli.py probe         # enumerate monitors, capabilities and key values
    python cli.py dump          # dump every VCP advertised by the monitor
    python cli.py test-write    # safe write test, restores every value
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

from asusdwc import vcp  # noqa: E402
from asusdwc.controller import MonitorManager  # noqa: E402
from asusdwc.ddc import enumerate_monitors  # noqa: E402


def probe() -> int:
    states = []
    for handle in enumerate_monitors():
        from asusdwc.controller import MonitorState

        state = MonitorState(handle=handle).probe()
        states.append(state)
        print("=" * 78)
        print(f"{state.display_name}")
        print(f"  device      : {handle.device_name}   primary={handle.primary}")
        print(f"  resolution  : {state.resolution}")
        print(f"  model       : {state.model!r}   line={state.line}")
        print(f"  caps model  : {state.caps.model!r}  mccs={state.caps.mccs_version}")
        print(f"  VCP count   : {state.caps.vcp_count}")
        print(f"  VCP version : {state.vcp_version()}")
        print(f"  firmware    : {state.firmware()}")
        print(f"  DDC/CI      : {state.ddc_ok}")
        mode = state.current_mode()
        print(f"  mode        : {mode.en if mode else '?'} (raw={state.current_mode_value()})")
        print(f"  input       : {state.input_name()} (raw={state.value(vcp.VCP_INPUT)})")
        print(f"  color temp  : {state.color_temp_name()} (raw={state.value(vcp.VCP_COLOR_TEMP)})")
        print(f"  brightness  : {state.value(vcp.VCP_BRIGHTNESS)} / {state.maximum(vcp.VCP_BRIGHTNESS)}")
        print(f"  contrast    : {state.value(vcp.VCP_CONTRAST)} / {state.maximum(vcp.VCP_CONTRAST)}")
        print(f"  volume      : {state.value(vcp.VCP_VOLUME)} / {state.maximum(vcp.VCP_VOLUME)}")
        print(f"  blue light  : {state.value(vcp.VCP_BLUE_LIGHT)} / {state.maximum(vcp.VCP_BLUE_LIGHT)}")
        print(f"  gamma raw   : 0x{(state.value(vcp.VCP_GAMMA) or 0):04X} (index {vcp.gamma_index(state.value(vcp.VCP_GAMMA) or 0)})")
        print(f"  overdrive   : {state.value(vcp.VCP_OVERDRIVE)} / {state.maximum(vcp.VCP_OVERDRIVE)}")
        print(f"  toggles FC  : 0x{(state.value(vcp.VCP_TOGGLES1) or 0):04X}")
        print(f"  toggles FD  : 0x{(state.value(vcp.VCP_TOGGLES2) or 0):04X}")
    return 0 if states else 1


def dump() -> int:
    manager = MonitorManager()
    states = manager.scan()
    for state in states:
        state.probe()
        if not state.ddc_ok:
            continue
        print("=" * 78)
        print(f"{state.display_name} - {state.model} ({state.caps.vcp_count} commands)")
        for code in sorted(state.caps.vcp):
            entry = state.values.get(code)
            if entry:
                print(f"  0x{code:02X}  cur={entry.current:6d} (0x{entry.current:04X})  max={entry.maximum:6d}  type={entry.vcp_type}")
            else:
                print(f"  0x{code:02X}  <read failed>")
    manager.close()
    return 0


def test_write() -> int:
    manager = MonitorManager()
    states = manager.scan()
    if not states:
        print("no monitors")
        return 1
    state = next((s for s in states if s.ddc_ok), None)
    if state is None:
        state = states[0]
        state.probe()
    print(f"testing {state.display_name} ({state.model})")

    def roundtrip(code, label, new_value=None):
        original = state.value(code)
        maximum = state.maximum(code)
        if original is None:
            print(f"  {label:<20} not supported")
            return
        if new_value is None:
            new_value = original + 1 if original < maximum else original - 1
        ok = state.write(code, new_value)
        time.sleep(0.4)
        read = state.value(code)
        print(f"  {label:<20} {original} -> {new_value}: write={ok} read={read}")
        ok2 = state.write(code, original)
        time.sleep(0.3)
        print(f"  {'':<20} restore {original}: {ok2} read={state.value(code)}")

    roundtrip(vcp.VCP_BRIGHTNESS, "brightness")
    roundtrip(vcp.VCP_CONTRAST, "contrast")
    roundtrip(vcp.VCP_VOLUME, "volume")
    roundtrip(vcp.VCP_BLUE_LIGHT, "blue light", 1)
    roundtrip(vcp.VCP_OVERDRIVE, "overdrive")

    gamma = state.value(vcp.VCP_GAMMA)
    if gamma is not None:
        idx = vcp.gamma_index(gamma) or 0
        target = vcp.GAMMA_STEPS[min(len(vcp.GAMMA_STEPS) - 1, idx + 1)][0]
        print(f"  gamma raw {gamma:#06x} -> {target:#06x}")
        print("   write", state.write(vcp.VCP_GAMMA, target))
        time.sleep(0.4)
        print("   read ", state.value(vcp.VCP_GAMMA))
        state.write(vcp.VCP_GAMMA, gamma)
        time.sleep(0.3)
        print("   restore read", state.value(vcp.VCP_GAMMA))

    mode = state.current_mode_value()
    if mode is not None:
        target = 7 if mode != 7 else 4
        print(f"  display mode raw {mode} -> {target}")
        ok = state.write(vcp.display_mode_code(state.line), vcp.encode_display_mode(state.line, target))
        time.sleep(0.8)
        print(f"   write={ok} read={state.current_mode_value()}")
        state.write(vcp.display_mode_code(state.line), vcp.encode_display_mode(state.line, mode))
        time.sleep(0.8)
        print(f"   restore read={state.current_mode_value()}")

    ctemp = state.value(vcp.VCP_COLOR_TEMP)
    if ctemp is not None:
        target = 6 if ctemp != 6 else 5
        print(f"  color temp {ctemp} -> {target}")
        print("   write", state.write(vcp.VCP_COLOR_TEMP, target))
        time.sleep(0.5)
        print("   read ", state.value(vcp.VCP_COLOR_TEMP))
        state.write(vcp.VCP_COLOR_TEMP, ctemp)
        time.sleep(0.5)
        print("   restore read", state.value(vcp.VCP_COLOR_TEMP))

    manager.close()
    return 0


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else "probe"
    raise SystemExit({"probe": probe, "dump": dump, "test-write": test_write}.get(command, probe)())
