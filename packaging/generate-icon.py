#!/usr/bin/env python3
"""Genere l'icone de l'application a partir du sprite pixel art du terminal.

L'icone n'est pas dessinee a la main: elle est rasterisee depuis la meme grille
8x8 que `src/ui/pixel.rs`, avec la palette de `src/theme.rs`. Regenerer apres
toute modification du sprite:

    python3 packaging/generate-icon.py

Aucune dependance: le PNG est ecrit a la main (RGBA, un seul bloc IDAT).
"""

import struct
import sys
import zlib
from pathlib import Path

# Doit rester identique a `pixel::TERMINAL` dans src/ui/pixel.rs.
SPRITE = [
    "########",
    "#......#",
    "#.##...#",
    "#...#..#",
    "#.##...#",
    "#......#",
    "#.oooo.#",
    "########",
]

# Palette de src/theme.rs.
BACKGROUND = (0x16, 0x16, 0x1C, 255)  # bg_deep
PRIMARY = (0x8B, 0x5C, 0xF6, 255)  # accent
SECONDARY = (0xA7, 0x8B, 0xFA, 255)  # accent_soft

SIZE = 256
MARGIN_CELLS = 1  # marge, exprimee en cellules du sprite


def render() -> list[list[tuple[int, int, int, int]]]:
    """Rasterise le sprite en une image carree, sans interpolation."""
    grid = len(SPRITE) + 2 * MARGIN_CELLS
    cell = SIZE // grid
    offset = (SIZE - cell * grid) // 2  # centre le reste de la division
    pixels = [[BACKGROUND for _ in range(SIZE)] for _ in range(SIZE)]

    for row, line in enumerate(SPRITE):
        for column, symbol in enumerate(line):
            color = {"#": PRIMARY, "o": SECONDARY}.get(symbol)
            if color is None:
                continue
            x0 = offset + (column + MARGIN_CELLS) * cell
            y0 = offset + (row + MARGIN_CELLS) * cell
            for y in range(y0, y0 + cell):
                for x in range(x0, x0 + cell):
                    pixels[y][x] = color
    return pixels


def write_png(path: Path, pixels: list[list[tuple[int, int, int, int]]]) -> None:
    height = len(pixels)
    width = len(pixels[0])
    raw = bytearray()
    for row in pixels:
        raw.append(0)  # filtre "None" pour la ligne
        for pixel in row:
            raw.extend(pixel)

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def main() -> int:
    target = Path(__file__).resolve().parent / "sshpass.png"
    write_png(target, render())
    print(f"{target} ecrit ({target.stat().st_size} octets, {SIZE}x{SIZE})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
