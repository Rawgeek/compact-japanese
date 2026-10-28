#!/usr/bin/env python3
"""Keep the public summary honest against raw measurements and built data."""
from pathlib import Path
import hashlib
import json
import statistics

ROOT = Path(__file__).resolve().parents[1]
REPORT = json.loads((ROOT / 'benchmarks/report.json').read_text())
MANIFEST = json.loads((ROOT / 'data/manifest.json').read_text())


def raw(name: str) -> dict:
    return json.loads((ROOT / 'benchmarks/results' / name).read_text())


assert REPORT['schema_version'] == 1
dictionary = ROOT / 'data/dictionary.bin'
assert dictionary.stat().st_size == REPORT['integrity']['core_after_bytes'] == MANIFEST['dictionary']['size']
assert hashlib.sha256(dictionary.read_bytes()).hexdigest() == REPORT['integrity']['dictionary_sha256']
assert 128 + sum(MANIFEST['dictionary']['sections']) == MANIFEST['dictionary']['size']
assert MANIFEST['dictionary']['sys.dic']['words'] == REPORT['integrity']['vocabulary_candidates']
assert MANIFEST['dictionary']['unk.dic']['words'] == REPORT['integrity']['unknown_candidates']

for platform, filename in {
    'macos_arm64_t6': 'macos-arm64-native-t6.json',
    'linux_arm64_t1': 'linux-arm64-native-t1.json',
    'linux_arm64_t6': 'linux-arm64-native-t6.json',
}.items():
    measured = raw(filename)
    assert measured['calls_per_second'] == REPORT['throughput'][platform]['after_fields_per_second']
    assert measured['cpu_microseconds_per_call'] == REPORT['cpu_microseconds_per_field'][platform]['after']

for platform in ('macos-arm64', 'linux-arm64'):
    before = raw(f'{platform}-cutlet-t6.json')
    after = raw(f'{platform}-native-t6.json')
    assert before['output_sha256'] == after['output_sha256']

macos_startup = raw('macos-arm64-startup.json')
linux_startup = raw('linux-arm64-startup.json')
assert statistics.median(macos_startup['milliseconds']['cutlet']) == REPORT['startup_median_ms']['macos_arm64']['before']
assert statistics.median(macos_startup['milliseconds']['native']) == REPORT['startup_median_ms']['macos_arm64']['after']
assert statistics.median(linux_startup['milliseconds']['cutlet']) == REPORT['startup_median_ms']['linux_arm64']['before']
assert statistics.median(linux_startup['milliseconds']['native']) == REPORT['startup_median_ms']['linux_arm64']['after']

before = raw('linux-arm64-cutlet-t6.json')
after = raw('linux-arm64-native-t6.json')
memory = REPORT['linux_arm64_memory_mib_t6']
assert before['warm']['pss_mib'] == memory['before_warm_pss']
assert after['warm']['pss_mib'] == memory['after_warm_pss']
assert before['after_distinct']['pss_mib'] == memory['before_after_distinct_pss']
assert after['after_distinct']['pss_mib'] == memory['after_after_distinct_pss']
assert before['after_distinct']['private_mib'] - before['warm']['private_mib'] == memory['before_private_growth']
assert abs(after['after_distinct']['private_mib'] - after['warm']['private_mib'] - memory['after_private_growth']) < 1e-12
assert before['after_distinct']['dictionary_mappings'] == memory['before_dictionary_mappings']
assert after['after_distinct']['dictionary_mappings'] == memory['after_dictionary_mappings']

print(json.dumps({'report': 'verified', 'dictionary_bytes': dictionary.stat().st_size}))
