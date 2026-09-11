#!/usr/bin/env python3
"""Regenerate the editable starter hair/accessory GLBs (no third-party modules).

These are initial stylized assets, each with three independent LOD meshes.
All positions are in the shared head joint's local coordinates. Keep recipes
in presets/*.json; running this tool never rewrites the curated starters.
"""
import json
import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT.parents[1] / "studio/tools"))
import generate_person_asset as person


def ellipsoid(vertices, indices, center, size, detail):
    radial, rows = {3: (16, 10), 2: (10, 6), 1: (6, 4)}[detail]
    offset = len(vertices)
    for row in range(rows + 1):
        phi = math.pi * row / rows
        for col in range(radial + 1):
            theta = math.tau * col / radial
            p = (math.sin(phi) * math.cos(theta), math.cos(phi), math.sin(phi) * math.sin(theta))
            vertices.append(tuple(center[i] + p[i] * size[i] / 2 for i in range(3)))
            if row < rows and col < radial:
                a = offset + row * (radial + 1) + col
                b = a + radial + 1
                indices.extend([a, a + 1, b, a + 1, b + 1, b])


def geometry(style, detail):
    vertices, indices = [], []
    def ball(center, size): ellipsoid(vertices, indices, center, size, detail)
    if style in ("round-glasses", "square-glasses"):
        for side in (-1, 1):
            for i in range(32 if detail == 3 else 20 if detail == 2 else 12):
                angle = math.tau * i / (32 if detail == 3 else 20 if detail == 2 else 12)
                x, y = math.cos(angle), math.sin(angle)
                if style == "square-glasses":
                    x, y = math.copysign(abs(x)**0.5, x), math.copysign(abs(y)**0.5, y)
                ball((side * .20 + x * .152, .072 + y * .112, -.435), (.040, .040, .042))
            ball((side * .43, .10, -.18), (.032, .032, .51))
        ball((0, .10, -.446), (.10, .030, .035))
    elif style == "hearing-aids":
        for side in (-1, 1):
            ball((side * .515, -.015, .02), (.065, .20, .11))
            ball((side * .53, .09, -.02), (.045, .045, .115))
            ball((side * .525, .035, -.08), (.032, .12, .032))
    else:
        if style == "buzz":
            # A close-fitting upper cap, leaving the entire face exposed.
            ball((0, .365, .015), (.90, .26, .75))
        else:
            ball((0, .35, .03), (1.01, .36, .82))
        if style == "floppy":
            for i in range(6):
                ball((-.32 + i * .115, .28 - i * .028, -.36), (.23, .32, .18))
        elif style in ("curls", "coils"):
            count = 12 if style == "curls" else 18
            for ring in range(3):
                for i in range(count):
                    angle = math.tau * (i + ring * .5) / count
                    radius = .40 - ring * .12
                    size = .26 if style == "curls" else .20
                    ball((math.cos(angle)*radius, .34 + ring*.09, math.sin(angle)*radius), (size, size, size))
        elif style == "braids":
            for side in (-1, 1):
                for i in range(8):
                    ball((side*(.45 + .015*math.sin(i*2)), .30-i*.105, .12), (.18, .18, .18))
            for i in range(7):
                ball((-.35+i*.115, .36, -.26), (.13, .22, .36))
        elif style == "bob":
            for side in (-1, 1):
                ball((side*.45, .02, .02), (.20, .85, .70))
            ball((0, -.03, .38), (.91, .80, .20))
            for i in range(6):
                ball((-.33+i*.13, .24, -.35), (.17, .34, .16))
        elif style == "buns":
            for side in (-1, 1):
                ball((side*.42, .53, .11), (.38, .38, .38))
    return vertices, indices


def write_asset(slug, kind, name, slot, coverage, color):
    directory = ROOT / "source/morphs" / kind / slug
    directory.mkdir(parents=True, exist_ok=True)
    binary, views, accessors, meshes, nodes = bytearray(), [], [], [], []
    counts = {}
    for level, detail in [("near", 3), ("mid", 2), ("far", 1)]:
        vertices, indices = geometry(slug, detail)
        counts[level] = len(indices)//3
        for values, fmt, target, component, accessor_type in [
            (vertices, "<3f", 34962, 5126, "VEC3"),
            ([(i,) for i in indices], "<I", 34963, 5125, "SCALAR"),
        ]:
            offset = len(binary)
            binary.extend(b"".join(struct.pack(fmt, *value) for value in values))
            views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(binary)-offset, "target": target})
            accessors.append({"bufferView": len(views)-1, "componentType": component, "count": len(values), "type": accessor_type})
        meshes.append({"primitives": [{"attributes": {"POSITION": len(accessors)-2}, "indices": len(accessors)-1, "material": 0}]})
        nodes.append({"name": level, "mesh": len(meshes)-1})
    document = {"asset": {"version": "2.0", "generator": "Cubacadabra starter parts"},
        "scene": 0, "scenes": [{"nodes": [0, 1, 2]}], "nodes": nodes, "meshes": meshes,
        "materials": [{"name": name, "pbrMetallicRoughness": {"baseColorFactor": color, "roughnessFactor": .75}}],
        "accessors": accessors, "bufferViews": views, "buffers": [{"byteLength": len(binary)}]}
    data = json.dumps(document, separators=(",", ":")).encode()
    data += b" " * (-len(data) % 4)
    glb = struct.pack("<III", 0x46546C67, 2, 28+len(data)+len(binary))
    glb += struct.pack("<II", len(data), 0x4E4F534A) + data
    glb += struct.pack("<II", len(binary), 0x004E4942) + binary
    (directory / f"{slug}.glb").write_bytes(glb)
    capabilities = ["mesh.rigid.v1"]
    if kind == "hair": capabilities.append("hair.authored.v1")
    if slug == "hearing-aids": capabilities.append("accessory.ear-device.v1")
    manifest = {"schemaVersion": 1, "asset": {
        "id": f"cuba:{kind}/{slug}.v1", "kind": kind, "displayName": name,
        "rigProfile": "cuba:rig/biped15.v1", "fitProfiles": ["cuba:fit/person-standard.v1"],
        "supportedBases": ["cuba:base/person.v1", "cuba:base/person-02.v1"],
        "occupiedSlots": [slot], "coverage": [coverage], "conflicts": [], "materials": [slug],
        "lod": counts, "requiredCapabilities": capabilities, "source": {"geometry": f"{slug}.glb"},
        "provenance": {"source": "Cubacadabra starter-set/generate_parts.py", "license": "Cubacadabra official"}},
        "geometry": {"file": f"{slug}.glb", "lodNodes": {level: level for level in counts}},
        "attachment": {"mode": "rigid", "joint": "head", "translation": [0, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]}}
    (directory / f"{slug}.morph.json").write_text(json.dumps(manifest, indent=2)+"\n")


def main():
    for slug, name, color in [
        ("floppy", "Floppy Hair", [.22, .10, .045, 1]),
        ("curls", "Loose Curls", [.32, .16, .055, 1]),
        ("coils", "Coils", [.045, .027, .021, 1]),
        ("braids", "Twin Braids", [.065, .035, .025, 1]),
        ("buzz", "Buzz Cut", [.09, .052, .03, 1]),
        ("bob", "Bob", [.11, .06, .04, 1]),
        ("buns", "Double Buns", [.055, .025, .016, 1]),
    ]: write_asset(slug, "hair", name, "hair", "scalp", color)
    write_asset("round-glasses", "facewear", "Round Glasses", "facewear", "eyes", [.12, .20, .26, 1])
    write_asset("square-glasses", "facewear", "Square Glasses", "facewear", "eyes", [.30, .12, .12, 1])
    write_asset("hearing-aids", "accessory", "Hearing Aids", "ear-device", "ears", [.20, .46, .65, 1])
    for variant_name, path in [
        ("person-01", "base/person/person_skinned.glb"),
        ("person-02", "base/person-02/person_02.glb"),
    ]:
        person.make_glb(ROOT / "source/morphs" / path, person.PERSON_VARIANTS[variant_name] | {"avatar_tint": True})


if __name__ == "__main__": main()
