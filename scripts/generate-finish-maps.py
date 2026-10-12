#!/usr/bin/env python3
"""Author TCAD's seamless grayscale finish-height images (original WTFPL assets).

This is an asset generation utility, not a modeling or acceptance exercise.
The renderer samples the resulting height pixels; it does not synthesize grain
in the shader. Periodic multiscale fields make irregular, finite machining
streaks along U without a repeated sine-wave band pattern.
"""

import math
from pathlib import Path
import random
import struct
import zlib


SIZE = 1024
DESTINATION = Path(__file__).resolve().parents[1] / "assets/appearance"


def field(columns, rows, seed):
    generator = random.Random(seed)
    grid = [[generator.uniform(-1.0, 1.0) for _ in range(columns)] for _ in range(rows)]
    samples = []
    for count in (columns, rows):
        axis = []
        for pixel in range(SIZE):
            position = pixel * count / SIZE
            lower = math.floor(position)
            weight = position - lower
            weight = weight * weight * (3.0 - 2.0 * weight)
            axis.append((lower % count, (lower + 1) % count, weight))
        samples.append(axis)
    return grid, samples[0], samples[1]


def sample(layer, x, y):
    grid, horizontal, vertical = layer
    x0, x1, u = horizontal[x]
    y0, y1, v = vertical[y]
    a = grid[y0][x0] * (1.0 - u) + grid[y0][x1] * u
    b = grid[y1][x0] * (1.0 - u) + grid[y1][x1] * u
    return a * (1.0 - v) + b * v


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))


def write(name, layers):
    scanlines = bytearray()
    for y in range(SIZE):
        scanlines.append(0)  # PNG filter None, unsigned grayscale, big endian.
        for x in range(SIZE):
            height = 0.5 + sum(weight * sample(layer, x, y) for weight, layer in layers)
            scanlines.extend(struct.pack(">H", round(max(0.0, min(1.0, height)) * 65535)))
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 16, 0, 0, 0, 0)
    encoded = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
    encoded += chunk(b"IDAT", zlib.compress(scanlines, level=9)) + chunk(b"IEND", b"")
    DESTINATION.mkdir(parents=True, exist_ok=True)
    path = DESTINATION / f"{name}-height-v1.png"
    path.write_bytes(encoded)
    print(f"Authored {path} ({len(encoded)} bytes, 16-bit grayscale)")


def main():
    # Grain orientation is encoded in the image. Fine mottling interrupts the
    # streaks; longer, lower-amplitude marks keep close views from looking tiled.
    layers = [
        (0.19, field(32, 256, 71021)),
        (0.09, field(8, 128, 93041)),
        (0.04, field(4, 64, 17209)),
        (0.025, field(512, 512, 43313)),
    ]
    write("brushed", layers)
    # These are height images, not color images. Their grayscale gradients
    # supply the physical grain direction and relief used by the renderer.
    profiles = {
        "blasted": [(0.19, 128, 128, 31243), (0.09, 512, 512, 17771), (0.04, 32, 32, 73979)],
        "machined": [(0.22, 4, 512, 26237), (0.08, 16, 128, 54539), (0.025, 256, 256, 71087)],
        "orange-peel": [(0.23, 48, 48, 32309), (0.08, 128, 128, 89261), (0.025, 256, 256, 54377)],
        "molded": [(0.18, 64, 64, 76103), (0.10, 256, 256, 40927), (0.035, 512, 512, 58043)],
        "wood": [(0.19, 4, 64, 93281), (0.12, 16, 256, 58363), (0.06, 2, 32, 25931), (0.02, 256, 256, 73363)],
    }
    for name, definition in profiles.items():
        write(name, [(weight, field(columns, rows, seed)) for weight, columns, rows, seed in definition])


if __name__ == "__main__":
    main()
