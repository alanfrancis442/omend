#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Label and convert headless GUI test frames to PNG using only Python stdlib.

Generate the frames first (no display server or hardware connection is used):
  OMEND_GUI_PREVIEW_DIR="$PWD/dist/gui-preview" \
    cargo test -p omen-gui --release --locked --offline
  python3 scripts/render-preview.py

The test uses explicit synthetic temperatures, RPM, utilization and settings.
This conversion adds a visible synthetic-data banner outside the app viewport.
It does not capture a desktop or claim hardware, usability or accessibility
validation. The locked software renderer omits item rotations: Dashboard gauge
ring ticks are incomplete, so use Settings/About for documentation previews.
Keep the image caption when copying a preview into documentation.
"""
import argparse
from pathlib import Path
import struct
import zlib

LABEL = "SYNTHETIC DEMO - NO LIVE HARDWARE DATA"
# A small bitmap alphabet drawn for the preview label; no external font assets.
FONT = {
    "A": "01110 10001 10001 11111 10001 10001 10001",
    "C": "01111 10000 10000 10000 10000 10000 01111",
    "D": "11110 10001 10001 10001 10001 10001 11110",
    "E": "11111 10000 10000 11110 10000 10000 11111",
    "H": "10001 10001 10001 11111 10001 10001 10001",
    "I": "11111 00100 00100 00100 00100 00100 11111",
    "L": "10000 10000 10000 10000 10000 10000 11111",
    "M": "10001 11011 10101 10101 10001 10001 10001",
    "N": "10001 11001 10101 10011 10001 10001 10001",
    "O": "01110 10001 10001 10001 10001 10001 01110",
    "R": "11110 10001 10001 11110 10100 10010 10001",
    "S": "01111 10000 10000 01110 00001 00001 11110",
    "T": "11111 00100 00100 00100 00100 00100 00100",
    "V": "10001 10001 10001 10001 10001 01010 00100",
    "W": "10001 10001 10001 10101 10101 10101 01010",
    "Y": "10001 10001 01010 00100 00100 00100 00100",
    "-": "00000 00000 00000 11111 00000 00000 00000",
    " ": "00000 00000 00000 00000 00000 00000 00000",
}


def chunk(kind, data):
    return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data))


def convert(source, destination):
    header = source.read_bytes().split(b"\n", 4)
    if len(header) != 5 or header[0] != b"P6" or header[1] != b"# SYNTHETIC DEMO - NO LIVE HARDWARE DATA" or header[3] != b"255":
        raise ValueError(f"not a labeled synthetic test frame: {source.name}")
    width, height = map(int, header[2].split())
    if (width, height) not in ((480, 800), (1280, 800)) or len(header[4]) != width * height * 3:
        raise ValueError(f"unexpected frame dimensions/data: {source.name}")
    banner_height = 32
    banner = bytearray(bytes((38, 25, 54)) * width * banner_height)
    for index, character in enumerate(LABEL):
        for y, row in enumerate(FONT[character].split()):
            for x, bit in enumerate(row):
                if bit == "1":
                    for dy in range(2):
                        for dx in range(2):
                            offset = ((9 + y * 2 + dy) * width + 12 + index * 12 + x * 2 + dx) * 3
                            banner[offset:offset + 3] = b"\xff\xff\xff"
    banner[-width * 3:] = bytes((255, 55, 95)) * width
    image = bytes(banner) + header[4]
    rows = b"".join(b"\0" + image[row * width * 3:(row + 1) * width * 3] for row in range(height + banner_height))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!IIBBBBB", width, height + banner_height, 8, 2, 0, 0, 0))
    png += chunk(b"tEXt", b"Description\0Synthetic headless GUI demo; no live hardware data. Added banner is outside the app viewport.")
    png += chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    destination.write_bytes(png)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    repository = Path(__file__).resolve().parent.parent
    parser.add_argument("--directory", type=Path, default=repository / "dist/gui-preview")
    args = parser.parse_args()
    directory = args.directory.resolve()
    if not directory.is_relative_to((repository / "dist").resolve()):
        parser.error("generated previews must remain under this repository's dist directory")
    frames = sorted(directory.glob("synthetic-*.ppm"))
    expected = {f"synthetic-{page}-{layout}.ppm"
                for page in ("dashboard", "settings", "about")
                for layout in ("compact", "desktop")}
    if {frame.name for frame in frames} != expected:
        parser.error("expected six Dashboard/Settings/About frames; run the documented headless GUI test first")
    try:
        for source in frames:
            if source.is_symlink():
                raise ValueError("preview frames must not be symlinks")
            destination = source.with_suffix(".png")
            if destination.is_symlink():
                raise ValueError("preview PNG destination must not be a symlink")
            convert(source, destination)
            print(destination.relative_to(repository))
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
