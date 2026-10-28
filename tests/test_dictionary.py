"""The projection must preserve segmentation data and reject bad offsets."""

import csv
import io
import struct
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from dictionary import MAGIC, TOKEN, compact_dictionary


def source_dictionary(path: Path, *, broken_offset: bool = False) -> tuple[bytes, bytes]:
    records = []
    for unused in ("unused", "quoted,accent"):
        fields = ["*"] * 26
        fields[0:2] = ["名詞", "固有名詞"]
        fields[9] = "ヤマダ"
        fields[17] = "ヤマダ"
        fields[23] = unused
        line = io.StringIO()
        csv.writer(line, lineterminator="").writerow(fields)
        records.append(line.getvalue().encode() + b"\0")
    trie = b"unchanged trie"
    features = b"".join(records)
    tokens = TOKEN.pack(1, 2, 3, -10, len(features) if broken_offset else 0, 0)
    tokens += TOKEN.pack(4, 5, 6, 20, len(records[0]), 0)
    size = 72 + len(trie) + len(tokens) + len(features)
    header = struct.pack("<10I", size ^ MAGIC, 102, 0, 2, 7, 8, len(trie), len(tokens), len(features), 0)
    path.write_bytes(header + b"utf8".ljust(32, b"\0") + trie + tokens + features)
    return trie, tokens


class JapaneseDictionaryTests(unittest.TestCase):
    def test_unused_metadata_is_removed_without_changing_candidates_or_scores(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, output = Path(temporary) / "source.dic", Path(temporary) / "compact.dic"
            trie, old_tokens = source_dictionary(source)
            result = compact_dictionary(source, output, lambda reading: b"yamada" if reading == "ヤマダ" else None)
            data = output.read_bytes()
        header = struct.unpack_from("<10I", data)
        self.assertEqual(header[3:8], (2, 7, 8, len(trie), len(old_tokens)))
        self.assertEqual(data[72 : 72 + len(trie)], trie)
        start = 72 + len(trie)
        self.assertEqual(TOKEN.unpack_from(data, start), (1, 2, 3, -10, 0, 0))
        self.assertEqual(TOKEN.unpack_from(data, start + TOKEN.size), (4, 5, 6, 20, 0, 0))
        self.assertEqual(data[start + len(old_tokens) :], b"\x08yamada\0")
        self.assertEqual(header[0] ^ MAGIC, len(data))
        self.assertEqual(result["unique_features"], 1)

    def test_an_invalid_feature_offset_cannot_produce_an_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, output = Path(temporary) / "source.dic", Path(temporary) / "compact.dic"
            source_dictionary(source, broken_offset=True)
            with self.assertRaisesRegex(ValueError, "feature offset"):
                compact_dictionary(source, output, lambda _reading: b"unused")
            self.assertFalse(output.exists())
