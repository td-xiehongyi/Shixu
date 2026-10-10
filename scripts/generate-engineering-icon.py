#!/usr/bin/env python3
"""Deterministic engineering ICO: green tray motif, opaque BGRA DIBs.
No external packages, downloaded artwork or build-time placeholder.
"""
import argparse
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "src-tauri/icons/icon.ico"

def generate():
    frames = []
    for size in (16, 32, 48):
        # ICO DIB rows are bottom-up; height includes the monochrome AND mask.
        pixels = bytearray()
        for y in reversed(range(size)):
            for x in range(size):
                # Small ivory calendar strokes within the existing green motif.
                stroke = size // 8 <= x < size * 7 // 8 and (
                    size // 4 <= y < size // 4 + max(1, size // 16)
                    or size // 2 <= y < size // 2 + max(1, size // 16)
                )
                r, g, b = (243, 240, 229) if stroke else (47, 102, 85)
                pixels.extend((b, g, r, 255))
        mask = bytes(((size + 31) // 32) * 4 * size)
        header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels), 0, 0, 0, 0)
        frames.append(header + pixels + mask)
    output = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
    offset = 6 + 16 * len(frames)
    for size, frame in zip((16, 32, 48), frames):
        output.extend(struct.pack("<BBBBHHII", size, size, 0, 0, 1, 32, len(frame), offset))
        offset += len(frame)
    return bytes(output) + b"".join(frames)

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = generate()
    if args.check:
        if not ICON.is_file() or ICON.read_bytes() != data:
            raise SystemExit("BLOCKED: committed engineering ICO differs from deterministic generator")
    else:
        ICON.parent.mkdir(parents=True, exist_ok=True)
        ICON.write_bytes(data)
