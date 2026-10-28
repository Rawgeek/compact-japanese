"""Build-only projection of pinned UniDic data into compact romanization token features."""

from __future__ import annotations

import csv
import functools
import mmap
import struct
import subprocess
import tempfile
from pathlib import Path
from typing import Callable

TOKEN = struct.Struct("<HHHhII")
MAGIC = 0xEF718F77
KINDS = {"補助記号": 1, "接頭辞": 2, "接尾辞": 3, "動詞": 4, "助動詞": 5, "形容詞": 6, "助詞": 7}


def feature(parts: list[str], compile_reading: Callable[[str], bytes | None]) -> bytes:
    pos1 = parts[0]
    pos2 = parts[1] if len(parts) > 1 else ""
    flags = KINDS.get(pos1, 0) | (8 if pos2 == "固有名詞" else 0) | (16 if pos2 in "接続助詞" else 0)
    pronunciation = parts[9] if len(parts) > 9 else ""
    reading = parts[17] if len(parts) > 17 else ""
    if pos1 == "助詞" and pronunciation in {"ワ", "エ"}:
        romaji = {"ワ": b"wa", "エ": b"e"}[pronunciation]
    elif not reading:
        flags |= 32
        romaji = b""
    else:
        compiled = compile_reading(reading)
        if compiled is None:
            flags |= 64
            romaji = b""
        else:
            romaji = compiled
    return bytes([flags]) + romaji + b"\0"


def compact_dictionary(source: Path, destination: Path, compile_reading: Callable[[str], bytes | None]) -> dict:
    """Preserve every token, trie unit and score; replace only feature offsets.

    MeCab's binary token layout is fixed by the pinned version. CSV parsing is
    necessary because UniDic accent fields can contain quoted commas.
    """
    compile_reading = functools.lru_cache(maxsize=None)(compile_reading)
    with source.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        if len(data) < 72:
            raise ValueError("Short MeCab dictionary header")
        header = list(struct.unpack_from("<10I", data))
        magic, version, _kind, words, _left, _right, trie_size, token_size, feature_size, _dummy = header
        token_start = 72 + trie_size
        feature_start = token_start + token_size
        if (
            magic ^ MAGIC != len(data)
            or version != 102
            or token_size != words * TOKEN.size
            or feature_start + feature_size != len(data)
            or bytes(data[40:72]).rstrip(b"\0") != b"utf8"
        ):
            raise ValueError("Unexpected pinned MeCab dictionary layout")
        tokens = bytearray(data[token_start:feature_start])
        pool = bytearray()
        offsets: dict[bytes, int] = {}
        for index in range(words):
            old_offset = struct.unpack_from("<I", tokens, index * TOKEN.size + 8)[0]
            if old_offset >= feature_size or old_offset and data[feature_start + old_offset - 1] != 0:
                raise ValueError("Invalid token feature offset")
            start = feature_start + old_offset
            end = data.find(b"\0", start, len(data))
            if end < 0:
                raise ValueError("Unterminated token feature")
            parts = next(csv.reader([data[start:end].decode("utf-8")], strict=True))
            if not parts or len(parts) > 26:
                raise ValueError("Unexpected UniDic feature fields")
            encoded = feature(parts, compile_reading)
            new_offset = offsets.get(encoded)
            if new_offset is None:
                new_offset = len(pool)
                offsets[encoded] = new_offset
                pool.extend(encoded)
            struct.pack_into("<I", tokens, index * TOKEN.size + 8, new_offset)
        header[8] = len(pool)
        total = feature_start + len(pool)
        header[0] = total ^ MAGIC
        with destination.open("wb") as output:
            output.write(struct.pack("<10I", *header))
            # Copy the charset and trie without rebuilding or reordering them.
            output.write(data[40:token_start])
            output.write(tokens)
            output.write(pool)
    return {"words": words, "unique_features": len(offsets), "size": total}


def unknown_range(data: bytes, key: bytes) -> tuple[int, int]:
    base = struct.unpack_from("<i", data, 72)[0]
    for byte in key:
        position = 72 + (base + byte + 1) * 8
        next_base, check = struct.unpack_from("<iI", data, position)
        if check != base or next_base <= 0:
            raise ValueError("Missing unknown-word category")
        base = next_base
    terminal, check = struct.unpack_from("<iI", data, 72 + base * 8)
    if check != base or terminal >= 0:
        raise ValueError("Invalid unknown-word category terminal")
    value = -terminal - 1
    return value >> 8, value & 255


def build_dictionary(
    source: Path, destination: Path, compile_reading: Callable[[str], bytes | None], compiler_directory: Path
) -> dict:
    """Repack all candidates and verify the exact trie and connection scores."""
    destination.mkdir()
    with tempfile.TemporaryDirectory(prefix="compact-japanese-data-") as temporary:
        scratch = Path(temporary)
        result = {
            name: compact_dictionary(source / name, scratch / name, compile_reading)
            for name in ("sys.dic", "unk.dic")
        }
        subprocess.run(
            [str(compiler_directory / "compile_index"), str(source / "sys.dic"), str(scratch / "index")],
            check=True, timeout=180,
        )
        subprocess.run(
            [str(compiler_directory / "compile_matrix"), str(source / "matrix.bin"), str(scratch / "matrix"), str(scratch / "maps")],
            check=True, timeout=180,
        )
        maps = (scratch / "maps").read_bytes()
        old_left, old_right, new_left, new_right = struct.unpack_from("<4H", maps)
        left = struct.unpack_from(f"<{old_left}H", maps, 8)
        right = struct.unpack_from(f"<{old_right}H", maps, 8 + old_left * 2)
        pool = bytearray()
        packed = bytearray()
        words = result["sys.dic"]["words"]
        for name in ("sys.dic", "unk.dic"):
            data = (scratch / name).read_bytes()
            header = struct.unpack_from("<10I", data)
            count, trie_size, token_size = header[3], header[6], header[7]
            features = 72 + trie_size + token_size
            pool_base = len(pool)
            pool.extend(data[features:])
            if len(pool) >= 1 << 22 or new_left >= 8192 or new_right >= 8192:
                raise ValueError("Dictionary exceeds the packed token layout")
            for index in range(count):
                lc, rc, _pos, cost, feature_offset, _unused = TOKEN.unpack_from(data, 72 + trie_size + index * TOKEN.size)
                value = left[lc] | right[rc] << 13 | (cost & 65535) << 26 | (feature_offset + pool_base) << 42
                packed.extend(struct.pack("<Q", value))
                # Verify every packed token against its original fields.
                if (value & 8191, value >> 13 & 8191, value >> 26 & 65535, value >> 42) != (
                    left[lc], right[rc], cost & 65535, feature_offset + pool_base,
                ):
                    raise ValueError("Packed candidate did not round trip")
        characters = (source / "char.bin").read_bytes()
        categories = struct.unpack_from("<I", characters)[0]
        if len(characters) != 4 + categories * 32 + 65535 * 4:
            raise ValueError("Unexpected character table size")
        table = bytearray(struct.pack("<II", categories, 65535))
        unknown = (source / "unk.dic").read_bytes()
        for index in range(categories):
            key = characters[4 + index * 32:4 + (index + 1) * 32].rstrip(b"\0")
            start, count = unknown_range(unknown, key)
            if not count or start + count > result["unk.dic"]["words"]:
                raise ValueError("Invalid unknown-word range")
            table.extend(struct.pack("<II", words + start, count))
        table.extend(characters[4 + categories * 32:])
        sections = [(scratch / "index").read_bytes(), packed, pool, (scratch / "matrix").read_bytes(), table]
        header = bytearray(128)
        header[:8] = b"FJPN2\0\0\0"
        total = 128 + sum(map(len, sections))
        struct.pack_into("<5I", header, 8, 2, words, len(packed) // 8, 5, total)
        position = 128
        for index, section in enumerate(sections):
            struct.pack_into("<II", header, 32 + index * 8, position, len(section))
            position += len(section)
        with (destination / "dictionary.bin").open("wb") as output:
            output.write(header)
            for section in sections:
                output.write(section)
        result.update({"size": total, "sections": list(map(len, sections)), "matrix_contexts": [new_left, new_right]})
        return result
