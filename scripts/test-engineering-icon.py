#!/usr/bin/env python3
"""Independently validate ICO directory, DIB sizes, rows, alpha and masks."""
import hashlib
import struct
from pathlib import Path

root = Path(__file__).resolve().parent.parent
blob = (root / 'src-tauri/icons/icon.ico').read_bytes()
assert struct.unpack_from('<HHH', blob) == (0, 1, 3)
expected_offset = 54
for index, size in enumerate((16, 32, 48)):
    width, height, colors, reserved, planes, bits, length, offset = struct.unpack_from('<BBBBHHII', blob, 6 + index * 16)
    assert (width, height, colors, reserved, planes, bits) == (size, size, 0, 0, 1, 32)
    mask_size = ((size + 31) // 32) * 4 * size
    assert offset == expected_offset
    assert length == 40 + size * size * 4 + mask_size
    header = struct.unpack_from('<IiiHHIIiiII', blob, offset)
    assert header == (40, size, size * 2, 1, 32, 0, size * size * 4, 0, 0, 0, 0)
    rgba = blob[offset + 40:offset + 40 + size * size * 4]
    assert set(rgba[3::4]) == {255}
    assert set(tuple(rgba[i:i+4]) for i in range(0, len(rgba), 4)) == {(85, 102, 47, 255), (229, 240, 243, 255)}
    assert blob[offset + length - mask_size:offset + length] == bytes(mask_size)
    expected_offset += length
assert expected_offset == len(blob)
print('valid three-frame BGRA/DIB engineering ICO sha256=' + hashlib.sha256(blob).hexdigest())
