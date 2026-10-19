"""List managed types/methods of DisplayWidgetCenter.exe matching DDC/VCP keywords."""
import sys
import dnfile

PATH = r"C:\Program Files (x86)\ASUS\DisplayWidgetCenter\DisplayWidgetCenter.exe"
KEYS = ["ToggleSettings", "FrameRateBoost", "VCP", "DDC", "Brightness",
        "SetVCP", "GetVCP", "ColorTemperature", "Contrast", "InputSource"]

pe = dnfile.dnPE(PATH, fast_load=True)
pe.parse_data_directories()
md = pe.net.mdtables

entries = []
for td in md.TypeDef.rows:
    ns = str(td.TypeNamespace)
    nm = str(td.TypeName)
    full = f"{ns}.{nm}" if ns else nm
    methods = []
    for m in td.MethodList:
        if m.row is not None:
            methods.append(str(m.row.Name))
    entries.append((full, methods))

print("types:", len(entries))
seen = set()
for kw in KEYS:
    for full, methods in entries:
        hits = [m for m in methods if kw.lower() in m.lower()]
        if hits:
            key = (full, tuple(hits))
            if key in seen:
                continue
            seen.add(key)
            print(f"\n[{kw}] {full}")
            for h in hits:
                print(f"    {h}")
