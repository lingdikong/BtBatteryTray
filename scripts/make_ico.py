"""把 cargo test app_icon 导出的各尺寸 RGBA 打包成 PNG 压缩的 .ico。

用法: python scripts/make_ico.py <rgba目录> assets/app.ico
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
    """在图标下面垫一个深色圆底，保证在浅色背景（资源管理器、安装向导）上也看得清。"""
    out = bytearray(rgba)
    c = size / 2
    bg = (0x1c, 0x1c, 0x1e)
    for y in range(size):
        for x in range(size):
            # 圆底 2x2 超采样覆盖率
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


def main() -> None:
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    sizes = sorted(int(p.stem) for p in src.glob("*.rgba"))
    images = [png(s, with_backdrop(s, (src / f"{s}.rgba").read_bytes())) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for s, data in zip(sizes, images):
        dim = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    dst.write_bytes(header + entries + b"".join(images))
    print(f"{dst}: {sizes}")


if __name__ == "__main__":
    main()
