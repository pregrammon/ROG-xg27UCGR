"""Unit tests for the pure-python parts (no hardware access)."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from asusdwc import controller, vcp  # noqa: E402

CAPS = (
    "(prot(monitor)type(lcd)model(XG27UCGR)cmds(01 02 03 07 0C E3 F3)"
    "vcp(02 03 04 05 08 0B 0C 10 12 14(03 04 05 06 07 08 09 0B) 16 18 1A "
    "60(0F 12 1A) 62 72(5000 6400 7800 8C00 A000) 87 8A CC(01 02 03) "
    "DC(0000 0100 0200 01 02 03 04 05 06 07 08 09) E0(00 01 02) "
    "E3(00 07 08 09) E5 E6 EB(00 01 02 03 04 05 06 07) "
    "FC(081F) FD(0103) FF)mswhql(1)mccs_ver(2.0))"
)


class CapabilityTests(unittest.TestCase):
    def test_parse(self):
        caps = vcp.parse_capabilities(CAPS)
        self.assertEqual(caps.model, "XG27UCGR")
        self.assertEqual(caps.mccs_version, "2.0")
        self.assertIn(0x10, caps.vcp)
        self.assertEqual(caps.values(0x14), [3, 4, 5, 6, 7, 8, 9, 11])
        self.assertEqual(caps.values(0x60), [0x0F, 0x12, 0x1A])
        self.assertEqual(caps.values(0x72), [0x5000, 0x6400, 0x7800, 0x8C00, 0xA000])
        self.assertGreater(caps.vcp_count, 15)

    def test_empty(self):
        caps = vcp.parse_capabilities(None)
        self.assertEqual(caps.vcp, {})
        self.assertFalse(caps.supports(0x10))


class ProductLineTests(unittest.TestCase):
    def test_gaming(self):
        self.assertEqual(vcp.product_line("XG27UCGR"), vcp.ProductLine.GAMING)
        self.assertEqual(vcp.product_line("ASUS PG279Q"), vcp.ProductLine.GAMING)
        self.assertEqual(vcp.product_line("VG27A"), vcp.ProductLine.GAMING)

    def test_others(self):
        self.assertEqual(vcp.product_line("PA329CV"), vcp.ProductLine.PROART)
        self.assertEqual(vcp.product_line("VX239H"), vcp.ProductLine.MAINSTREAM)
        self.assertEqual(vcp.product_line("MB16AC"), vcp.ProductLine.PORTABLE)
        self.assertEqual(vcp.product_line("Generic PnP Monitor"), vcp.ProductLine.NONE)
        self.assertEqual(vcp.product_line("ALB01"), vcp.ProductLine.LIGHTBAR)


class MappingTests(unittest.TestCase):
    def test_encode_proart(self):
        self.assertEqual(vcp.encode_display_mode(vcp.ProductLine.PROART, 5), 0x05FF)
        self.assertEqual(vcp.encode_display_mode(vcp.ProductLine.PROART, 3840), 3840)
        self.assertEqual(vcp.encode_display_mode(vcp.ProductLine.GAMING, 5), 5)

    def test_gamma(self):
        self.assertEqual(vcp.gamma_index(0x7800), 2)
        self.assertEqual(vcp.gamma_index(0x5000), 0)
        self.assertIsNone(vcp.gamma_index(0x1234))

    def test_modes(self):
        modes = vcp.mode_set(vcp.ProductLine.GAMING)
        self.assertEqual(len(modes), 8)
        self.assertEqual(modes[0].value, 1)
        self.assertEqual(vcp.display_mode_code(vcp.ProductLine.PROART), vcp.VCP_CROSSHAIR)
        self.assertEqual(vcp.display_mode_code(vcp.ProductLine.GAMING), vcp.VCP_DISPLAY_MODE)


class FakeState:
    def __init__(self):
        self.model = "XG27UCGR"
        self.line = vcp.ProductLine.GAMING
        self.values = {
            0x10: type("V", (), {"current": 70, "maximum": 100})(),
            0x12: type("V", (), {"current": 50, "maximum": 100})(),
            0xDC: type("V", (), {"current": 4, "maximum": 521})(),
        }
        self.writes = []

    def supported(self, code):
        return code in self.values

    def value(self, code):
        entry = self.values.get(code)
        return entry.current if entry else None

    def maximum(self, code):
        entry = self.values.get(code)
        return entry.maximum if entry else 0

    def write(self, code, value):
        self.writes.append((code, value))
        if code not in self.values:
            return False
        self.values[code] = type("V", (), {"current": value, "maximum": self.values[code].maximum})()
        return True

    def refresh(self):
        pass


class ProfileTests(unittest.TestCase):
    def test_capture_and_apply(self):
        state = FakeState()
        profile = controller.capture_profile(state, "test")
        self.assertEqual(profile["name"], "test")
        self.assertIn(str(vcp.VCP_BRIGHTNESS), profile["vcp"])
        self.assertEqual(profile["vcp"][str(vcp.VCP_BRIGHTNESS)], 70)

        state.values[vcp.VCP_BRIGHTNESS] = type("V", (), {"current": 10, "maximum": 100})()
        applied, failed = controller.apply_profile(
            state,
            {"name": "p", "vcp": {str(vcp.VCP_BRIGHTNESS): 55, str(vcp.VCP_CONTRAST): 60}},
        )
        self.assertEqual((applied, failed), (2, 0))
        self.assertEqual(state.values[vcp.VCP_BRIGHTNESS].current, 55)
        self.assertIn((vcp.VCP_BRIGHTNESS, 55), state.writes)

    def test_all_captured_codes_supported(self):
        state = FakeState()
        for code in controller.PROFILE_CODES:
            state.values[code] = type("V", (), {"current": 1, "maximum": 100})()
        profile = controller.capture_profile(state, "wide")
        self.assertEqual(len(profile["vcp"]), len(set(controller.PROFILE_CODES)))
        json.dumps(profile)


class InputTests(unittest.TestCase):
    def test_names(self):
        self.assertEqual(vcp.INPUT_SOURCES[15], "DisplayPort 1")
        self.assertEqual(vcp.INPUT_SOURCES[26], "USB Type-C 1")
        self.assertIn(13, vcp.OSD_LANGUAGES)


if __name__ == "__main__":
    unittest.main(verbosity=2)
