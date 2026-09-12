"""Small deterministic color atlases for the starter-set GLBs.

These are intentionally color-only maps.  They add broad dye/fabric variation
and a quiet second scale without baking directional light or replacing the
renderer lighting model.  The module uses only the Python standard library so
asset generation remains portable and reproducible.
"""
from __future__ import annotations

import math
import struct
import zlib
from functools import lru_cache
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ASSET_DIR = ROOT / "assets"
SIZE = 128


def _chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)


def _png(pixels: list[tuple[int, int, int]], width: int = SIZE, height: int = SIZE) -> bytes:
    rows = b"".join(b"\0" + bytes(channel for pixel in pixels[row * width:(row + 1) * width] for channel in pixel)
                   for row in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", header) + _chunk(b"IDAT", zlib.compress(rows, 9)) + _chunk(b"IEND", b"")


def _noise(x: int, y: int, seed: int) -> float:
    value = (x * 374761393 + y * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    value ^= value >> 13
    value = (value * 1274126177) & 0xFFFFFFFF
    return ((value ^ (value >> 16)) & 0xFFFF) / 65535.0


def _atlas(kind: str) -> bytes:
    seed = sum(ord(char) for char in kind) * 97
    pixels: list[tuple[int, int, int]] = []
    for y in range(SIZE):
        for x in range(SIZE):
            # Smooth periodic value noise: no block boundaries or tile seams.
            def noise(scale):
                u, v = x / SIZE * scale, y / SIZE * scale
                ix, iy = math.floor(u), math.floor(v)
                a, b = u-ix, v-iy
                a, b = a*a*(3-2*a), b*b*(3-2*b)
                row = lambda yy: _noise(ix%scale, yy%scale, seed)*(1-a)+_noise((ix+1)%scale, yy%scale, seed)*a
                return row(iy)*(1-b)+row(iy+1)*b
            broad = .65*noise(7)+.35*noise(19)
            fine = _noise(x, y, seed + 31)
            if kind == "denim":
                weave = 0.5 + 0.5 * math.sin((x + y) * 0.58) * math.sin((x - y) * 0.12)
                value = 0.90 + broad * 0.065 + fine * 0.02 + weave * 0.015
            elif kind == "fleece":
                value = 0.94 + broad * 0.045 + fine * 0.008
            elif kind == "rib":
                value = 0.88 + broad * 0.12 + (0.035 if (x // 3) % 2 else -0.01)
            elif kind == "hair":
                flow = 0.5 + 0.5 * math.sin((x * 0.12) + (y * 0.025))
                value = 0.83 + broad * 0.15 + flow * 0.045 + fine * 0.02
            elif kind == "skin":
                value = 0.992 + broad * 0.006
            elif kind == "leather":
                value = 0.90 + broad * 0.10 + fine * 0.02
            elif kind == "rubber":
                value = 0.95 + broad * 0.045 + fine * 0.01
            elif kind == "metal":
                value = 0.88 + broad * 0.08 + fine * 0.025
            elif kind == "satin":
                value = 0.87 + broad * 0.10 + 0.06 * math.sin(x * 0.17 + y * 0.03)
            else:
                value = 0.92 + broad * 0.08 + fine * 0.015
            value = max(0.0, min(1.0, value))
            channel = int(round(value * 255))
            pixels.append((channel, channel, channel))
    return _png(pixels)


@lru_cache(maxsize=1)
def ensure() -> dict[str, str]:
    ASSET_DIR.mkdir(parents=True, exist_ok=True)
    result = {}
    for kind in ("fleece", "rib", "denim", "hair", "skin", "leather", "rubber", "metal", "satin", "cotton"):
        name = f"starter-{kind}.png"
        path = ASSET_DIR / name
        # Always rewrite the deterministic output so changing a pattern in
        # this source cannot leave a stale checked-in atlas behind.
        path.write_bytes(_atlas(kind))
        result[kind] = name
    return result
