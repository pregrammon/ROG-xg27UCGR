"""End-to-end smoke test against the real monitor and the tkinter app.

Run manually:  python tests/test_ui_smoke.py
It briefly changes brightness / mode and restores the original values.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from asusdwc.app import App  # noqa: E402
from asusdwc import vcp  # noqa: E402


def pump(app: App, seconds: float):
    end = time.time() + seconds
    while time.time() < end:
        app.update()
        time.sleep(0.02)


def wait_for(app: App, predicate, timeout=15.0):
    end = time.time() + timeout
    while time.time() < end:
        app.update()
        time.sleep(0.03)
        if predicate():
            return True
    return False


def main() -> int:
    app = App(initial_page="overview", language="zh")
    try:
        assert wait_for(app, lambda: app.state is not None and app.state.ddc_ok), "monitor not ready"
        app.update()
        print("model:", app.state.model, "vcp:", app.state.caps.vcp_count)
        brightness = app.state.value(vcp.VCP_BRIGHTNESS)
        print("brightness:", brightness)

        target = brightness + 3 if brightness < 97 else brightness - 3
        app.set_vcp(vcp.VCP_BRIGHTNESS, target, "亮度", "Brightness")
        assert wait_for(app, lambda: app.state.value(vcp.VCP_BRIGHTNESS) == target, 6), "brightness write failed"
        print("brightness write ok:", target)

        app.set_vcp(vcp.VCP_BRIGHTNESS, brightness, "亮度", "Brightness")
        assert wait_for(app, lambda: app.state.value(vcp.VCP_BRIGHTNESS) == brightness, 6), "brightness restore failed"
        print("brightness restore ok")

        mode = app.state.current_mode()
        options = [m for m in vcp.mode_set(app.state.line) if m.value != (mode.value if mode else -1)]
        app.set_mode(options[0])
        assert wait_for(app, lambda: app.state.current_mode_value() == options[0].value, 8), "mode write failed"
        print("mode write ok:", options[0].en)
        if mode:
            app.set_mode(mode)
            assert wait_for(app, lambda: app.state.current_mode_value() == mode.value, 8), "mode restore failed"
            print("mode restore ok:", mode.en)

        # Language rebuild
        app.change_language("en")
        pump(app, 0.8)
        assert app.page_title.cget("text") == "Overview", app.page_title.cget("text")
        app.change_language("zh")
        pump(app, 0.8)
        assert app.page_title.cget("text") == "概览", app.page_title.cget("text")
        print("language rebuild ok")

        # Visit every page
        for page in ("display", "eyecare", "gameplus", "osd", "profiles", "settings"):
            app.navigate(page)
            pump(app, 0.35)
            print("page ok:", page)
        print("SMOKE TEST PASSED")
        return 0
    finally:
        app._on_close()


if __name__ == "__main__":
    raise SystemExit(main())
