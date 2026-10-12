"""Pack the per-size RGBA dumps from `cargo test app_icon` into app icons; the format follows the output extension:

- .ico: Windows (PNG-compressed, 16-256)
- .icns: macOS (PNG-compressed, 16-1024)
- .png: Linux, the 256 px image

Usage: python scripts/make_ico.py <rgba dir> assets/app.ico [assets/app.icns assets/app.png]
"""
import struct
import sys
import zlib
from pathlib import Path


def png(size: int, rgba: bytes) -> bytes:
    rows = b"".join(b"\0" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))

    def chunk(tag: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def with_backdrop(size: int, rgba: bytes) -> bytes:
    """Put a dark disc under the icon so it stays readable on light backgrounds (Explorer, the setup wizard)."""
    out = bytearray(rgba)
    c = size / 2
    bg = (0x1c, 0x1c, 0x1e)
    for y in range(size):
        for x in range(size):
            # Disc coverage with 2x2 supersampling
            cov = sum(((x + dx) - c) ** 2 + ((y + dy) - c) ** 2 <= c * c for dx in (0.25, 0.75) for dy in (0.25, 0.75)) / 4
            if cov == 0:
                continue
            i = (y * size + x) * 4
            fa = out[i + 3] / 255
            a = fa + cov * (1 - fa)
            for k in range(3):
                out[i + k] = round((out[i + k] * fa + bg[k] * cov * (1 - fa)) / a)
            out[i + 3] = round(a * 255)
    return bytes(out)


def ico(images: dict[int, bytes]) -> bytes:
    sizes = [s for s in sorted(images) if s <= 256]
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for s in sizes:
        dim = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(images[s]), offset)
        offset += len(images[s])
    return header + entries + b"".join(images[s] for s in sizes)


# icns type codes for PNG entries
ICNS_TYPES = {16: b"icp4", 32: b"icp5", 64: b"icp6", 128: b"ic07", 256: b"ic08", 512: b"ic09", 1024: b"ic10"}


def icns(images: dict[int, bytes]) -> bytes:
    body = b"".join(
        ICNS_TYPES[s] + struct.pack(">I", 8 + len(images[s])) + images[s] for s in sorted(images) if s in ICNS_TYPES
    )
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def main() -> None:
    src = Path(sys.argv[1])
    sizes = sorted(int(p.stem) for p in src.glob("*.rgba"))
    images = {s: png(s, with_backdrop(s, (src / f"{s}.rgba").read_bytes())) for s in sizes}
    for dst in map(Path, sys.argv[2:]):
        if dst.suffix == ".ico":
            dst.write_bytes(ico(images))
        elif dst.suffix == ".icns":
            dst.write_bytes(icns(images))
        elif dst.suffix == ".png":
            dst.write_bytes(images[256])
        else:
            sys.exit(f"unsupported format: {dst}")
        print(f"{dst}: {sizes}")


if __name__ == "__main__":
    main()
