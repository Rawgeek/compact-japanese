#!/usr/bin/env python3
"""Verify the public Python ABI, size budget, concurrency and optional reference."""
from pathlib import Path
import concurrent.futures
import json
import random
import sys
import unicodedata

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'python'))
from compact_japanese import Romanizer

library = ROOT / 'target/release' / ('libcompact_japanese.dylib' if sys.platform == 'darwin' else 'libcompact_japanese.so')
model = Romanizer(library, ROOT / 'data/dictionary.bin')
assert model.romanize('東京都渋谷区') == 'Tokyo to Shibuya ku'
assert model.romanize('山田太郎') == 'Yamada Tarou'
assert model.romanize('Ｃｏｏ Ｄｏｏ’ｓ') == "Coo Doo's"
assert model.romanize('') == ''
for text in ['a\0b', 'a' * 16385]:
    try:
        model.romanize(text)
    except ValueError:
        pass
    else:
        raise AssertionError('invalid input accepted')
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
    assert list(pool.map(model.romanize, ['東京', '大阪'] * 100)) == ['Tokyo', 'Osaka'] * 100
installed = library.stat().st_size + sum(p.stat().st_size for p in (ROOT/'data').rglob('*') if p.is_file())
installed += (ROOT/'python/compact_japanese.py').stat().st_size
assert installed < 32_000_000, installed
result = {'installed_bytes': installed, 'python_abi_and_concurrency': 'passed'}
if '--reference' in sys.argv:
    import cutlet
    reference = cutlet.Cutlet('hepburn', use_foreign_spelling=False)
    rng = random.Random(328113)
    parts = ['東京都', '大阪市', '北海道', '渋谷', '神南', '東京', 'エンゼル', 'ビル', 'っ', 'か', 'ん', 'ー', 'あ', 'を', 'A', ' ', '\t', '\n', '123', '-', '〒', '😀', '𠮷', "'", 'Ω', 'é', '々', '\u200b', '、', '「', '」']
    def safe(call, text):
        try: return call(text)
        except Exception: return '<conversion-error>'
    for _ in range(20000):
        text = unicodedata.normalize('NFKC', ''.join(rng.choices(parts, k=rng.randint(1, 15))))
        assert safe(model.romanize, text) == safe(reference.romaji, text), repr(text)
    result['generated_reference_cases'] = 20000
print(json.dumps(result, indent=2))
