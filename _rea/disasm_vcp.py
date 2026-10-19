"""Disassemble selected DisplayWidgetCenter.VCPAPI methods with dnfile + dncil."""
import sys
import dnfile
from dncil.cil.body.reader import read_method_body_from_bytes

PATH = r"C:\Program Files (x86)\ASUS\DisplayWidgetCenter\DisplayWidgetCenter.exe"
TARGET_TYPE = "DisplayWidgetCenter.VCPAPI"
WANT = ["SetFrameRateBoost", "GetFrameRateBoost", "SetToggleSettingsItem",
        "GetToggleSettingsItem", "SetVCPFeatureInternal", "SetVCPCodeBasic",
        "GetVCPVersion"]

pe = dnfile.dnPE(PATH, fast_load=True)
pe.parse_data_directories()
md = pe.net.mdtables
data = open(PATH, "rb").read()

# rva -> file offset
def rva_to_off(rva):
    for s in pe.sections:
        va = s.VirtualAddress
        if va <= rva < va + max(s.Misc_VirtualSize, s.SizeOfRawData):
            return s.PointerToRawData + (rva - va)
    return None

def token_name(tok):
    try:
        if tok.table == 0x0A:  # MemberRef
            r = md.MemberRef.rows[tok.ridx - 1]
            return f"MemberRef {r.Class.row.TypeName if hasattr(r.Class,'row') else '?'}::{r.Name}"
        if tok.table == 0x06:  # MethodDef
            r = md.MethodDef.rows[tok.ridx - 1]
            return f"MethodDef {r.Name}"
        if tok.table == 0x01:  # TypeRef
            r = md.TypeRef.rows[tok.ridx - 1]
            return f"TypeRef {r.TypeNamespace}.{r.TypeName}"
        if tok.table == 0x02:  # TypeDef
            r = md.TypeDef.rows[tok.ridx - 1]
            return f"TypeDef {r.TypeNamespace}.{r.TypeName}"
    except Exception as e:
        return f"tok({tok.value}) err={e}"
    return f"tok(0x{tok.value:08x})"

for td in md.TypeDef.rows:
    ns = str(td.TypeNamespace)
    nm = str(td.TypeName)
    if f"{ns}.{nm}" != TARGET_TYPE:
        continue
    for mrow in td.MethodList:
        m = mrow.row
        if m is None:
            continue
        name = str(m.Name)
        if name not in WANT:
            continue
        rva = m.Rva
        off = rva_to_off(rva)
        print(f"\n===== {TARGET_TYPE}::{name} rva=0x{rva:x} off=0x{off:x}")
        if not off:
            print("  no offset")
            continue
        body = read_method_body_from_bytes(data[off:])
        print(f"  header={body.header_size} code={body.code_size}")
        for insn in body.instructions:
            op = insn.opcode.name
            operand = ""
            try:
                if insn.operand is not None:
                    operand = str(insn.operand)
            except Exception:
                pass
            print(f"  IL_{insn.offset:04x}: {op:12s} {operand}")
