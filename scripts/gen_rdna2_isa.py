#!/usr/bin/env python3
"""Generate crates/d3d12_xs/src/rdna2_isa.rs from AMD MIT Machine-Readable ISA XML (amdgpu_isa_rdna2.xml)."""
import os
import sys
import xml.etree.ElementTree as ET

FAMILIES = [
    ("ENC_SOP1", "Sop1"),
    ("ENC_SOP2", "Sop2"),
    ("ENC_SOPC", "Sopc"),
    ("ENC_SOPK", "Sopk"),
    ("ENC_SOPP", "Sopp"),
    ("ENC_SMEM", "Smem"),
    ("ENC_VOP1", "Vop1"),
    ("ENC_VOP2", "Vop2"),
    ("ENC_VOPC", "Vopc"),
    ("ENC_VOP3", "Vop3"),
    ("ENC_VOP3_SDST_ENC", "Vop3"),
    ("ENC_VOP3P", "Vop3p"),
    ("ENC_DS", "Ds"),
    ("ENC_MUBUF", "Mubuf"),
    ("ENC_MTBUF", "Mtbuf"),
    ("ENC_MIMG", "Mimg"),
    ("ENC_EXP", "Exp"),
    ("ENC_VINTRP", "Vintrp"),
    ("ENC_FLAT", "Flat"),
    ("ENC_FLAT_GLOBAL", "FlatGlobal"),
    ("ENC_FLAT_SCRATCH", "FlatScratch"),
]

def main():
    xml_path = sys.argv[1] if len(sys.argv) > 1 else os.path.expandvars(r"%TEMP%\amd_isa\amdgpu_isa_rdna2.xml")
    out_path = os.path.join(os.path.dirname(__file__), "..", "crates", "d3d12_xs", "src", "rdna2_isa.rs")
    tree = ET.parse(xml_path)
    isa = tree.getroot().find("ISA")
    enc_map = dict(FAMILIES)
    tables = {fam: {} for _, fam in FAMILIES}

    for insn in isa.find("Instructions"):
        name = insn.findtext("InstructionName").strip().lower()
        for ie in insn.findall("./InstructionEncodings/InstructionEncoding"):
            enc_name = ie.findtext("EncodingName").strip()
            fam = enc_map.get(enc_name)
            if not fam:
                continue
            op_txt = ie.findtext("Opcode")
            if op_txt is None:
                continue
            op = int(op_txt.strip(), 0)
            tables[fam].setdefault(op, name)

    # Scarlett GFX10.1 console extensions
    tables["Vop2"].setdefault(0x1F, "v_mac_f32")
    tables["Vop2"].setdefault(0x20, "v_madmk_f32")
    tables["Vop2"].setdefault(0x21, "v_madak_f32")
    tables["Vop3"].setdefault(0x11F, "v_mac_f32")
    tables["Vop3"].setdefault(0x141, "v_mad_f32")

    lines = [
        "// Generated from AMD MIT Machine-Readable ISA XML (amdgpu_isa_rdna2.xml) via scripts/gen_rdna2_isa.py",
        "",
        "#[derive(Clone, Copy, PartialEq, Eq, Debug)]",
        "pub enum EncFamily {",
    ]
    seen_fams = []
    for _, fam in FAMILIES:
        if fam not in seen_fams:
            seen_fams.append(fam)
            lines.append(f"    {fam},")
    lines.extend([
        "}",
        "",
        "pub fn lookup_opcode(fam: EncFamily, op: u32) -> Option<&'static str> {",
        "    match fam {",
    ])
    for fam in seen_fams:
        entries = sorted(tables[fam].items())
        if not entries:
            lines.append(f"        EncFamily::{fam} => None,")
            continue
        lines.append(f"        EncFamily::{fam} => match op {{")
        for op, name in entries:
            lines.append(f'            0x{op:03X} => Some("{name}"),')
        lines.append("            _ => None,")
        lines.append("        },")
    lines.extend(["    }", "}"])

    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    total = sum(len(tables[fam]) for fam in seen_fams)
    print(f"Generated {out_path} ({total} opcode entries across {len(seen_fams)} families)")

if __name__ == "__main__":
    main()
