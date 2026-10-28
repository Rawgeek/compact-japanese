#!/usr/bin/env python3
"""Fresh-process startup and warm per-field latency, measured in microseconds."""
import argparse
import json
from pathlib import Path
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--engine', choices=['native', 'cutlet'], required=True)
args = parser.parse_args()
started = time.perf_counter_ns()
if args.engine == 'native':
    sys.path.insert(0, str(ROOT/'python'))
    from compact_japanese import Romanizer
    library = ROOT/'target/release'/('libcompact_japanese.dylib' if sys.platform=='darwin' else 'libcompact_japanese.so')
    model = Romanizer(library, ROOT/'data/dictionary.bin')
    convert = model.romanize
else:
    import cutlet
    model = cutlet.Cutlet('hepburn', use_foreign_spelling=False)
    convert = model.romaji
assert convert('山田太郎') == 'Yamada Tarou'
startup_ms = (time.perf_counter_ns()-started)/1e6
result = {'engine':args.engine, 'import_init_first_call_ms':startup_ms, 'latency_us':{}}
for name, value in {
    'name':'山田太郎', 'address':'東京都渋谷区神南1-2-3',
    'kana':'エンゼルタワー3F', 'ascii':'Example Building 123',
    'long':'東京都渋谷区神南1-2-3 ' * 24,
}.items():
    for _ in range(100): convert(value)
    durations=[]
    for _ in range(2000):
        start=time.perf_counter_ns()
        convert(value)
        durations.append((time.perf_counter_ns()-start)/1000)
    durations.sort()
    result['latency_us'][name] = {'p50':statistics.median(durations), 'p95':durations[1899], 'p99':durations[1979]}
print(json.dumps(result,indent=2))
