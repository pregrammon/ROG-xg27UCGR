"""Disassemble arbitrary DisplayWidgetCenter methods."""
import sys
import dnfile
from dncil.cil.body.reader import read_method_body_from_bytes

PATH = r"C:\Program Files (x86)\ASUS\DisplayWidgetCenter\DisplayWidgetCenter.exe"
TARGETS = [
    ("DisplayWidgetCenter.GameVisualPage", "FrameRateBoost_Checked"),
    ("DisplayWidgetCenter.GameVisualPage", "FrameRateBoost_Unchecked"),
    ("DisplayWidgetCenter.VCPAPI", "SetVCPFeatureInternalForEzOSD"),
    ("DisplayWidgetCenter.VCPAPI", "GetVCPFeatureInternal"),
    ("DisplayWidgetCenter.VCPAPI", "SetVCPCodeBasic"),
]

pe = dnfile.dnPE(PATH, fast_load=True)
pe.parse_data_directories()
md = pe.net.mdtables
data = open(PATH, "rb").read()


def rva_to_off(rva):
    for s in pe.sections:
        va = s.VirtualAddress
        if va <= rva < va + max(s.Misc_VirtualSize, s.SizeOfRawData):
            return s.PointerToRawData + (rva - va)
    return None


def resolve(tok):
    try:
        if tok.table == 0x06:
            return "MethodDef " + str(md.MethodDef.rows[tok.ridx - 1].Name)
        if tok.table == 0x0A:
            r = md.MemberRef.rows[tok.ridx - 1]
            cls = ""
            try:
                cls = str(r.Class.row.TypeNamespace) + "." + str(r.Class.row.TypeName)
            except Exception:
                pass
            return f"MemberRef {cls}::{r.Name}"
        if tok.table == 0x04:
            r = md.Field.rows[tok.ridx - 1]
            return "Field " + str(r.Name)
        if tok.table == 0x01:
            r = md.TypeRef.rows[tok.ridx - 1]
            return "TypeRef " + str(r.TypeNamespace) + "." + str(r.TypeName)
        if tok.table == 0x02:
            r = md.TypeDef.rows[tok.ridx - 1]
            return "TypeDef " + str(r.TypeNamespace) + "." + str(r.TypeName)
    except Exception as e:
        return f"tok(0x{tok.value:08x}) ?{e}"
    return f"tok(0x{tok.value:08x})"


for tns, mname in TARGETS:
    for td in md.TypeDef.rows:
        full = f"{td.TypeNamespace}.{td.TypeName}"
        if full != tns:
            continue
        for mrow in td.MethodList:
            m = mrow.row
            if m is None or str(m.Name) != mname:
                continue
            print(f"\n===== {full}::{mname} rva=0x{m.Rva:x}")
            body = read_method_body_from_bytes(data[rva_to_off(m.Rva):])
            for insn in body.instructions:
                op = insn.opcode.name
                operand = ""
                if insn.operand is not None:
                    operand = str(insn.operand)
                    if op.startswith("call") or op.startswith("newobj") or op in ("ldsfld", "ldfld", "stfld"):
                        operand = resolve(insn.operand)
                print(f"  IL_{insn.offset:04x}: {op:12s} {operand}")
