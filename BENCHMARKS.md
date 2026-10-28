# Measurements

The initial dictionary is **28,816,754 bytes**. On Apple Silicon, the dictionary, dynamic library, Python binding and license notices occupy **29,329,463 bytes** uncompressed.

| Criterion | Original | compact-japanese |
|---|---|---|
| Dictionary candidates | 756,264 | 756,264 |
| Unknown-word candidates | 35 | 35 |
| Connection scores | Original signed 16-bit scores | Identical scores, losslessly encoded |
| Core dictionary files | 259,493,567 bytes | 28,816,754 bytes |
| Cargo dependencies | Not applicable | 0 |
| Native tokenizer dependency | MeCab via Fugashi | Rust standard library and system mapping API |
| Python runtime dependencies | Cutlet, Fugashi, unidic-lite, jaconv, mojimoji | Standard library |
| Address or token history | Fugashi token cache grows with distinct inputs | No retained input or output strings |
| Numeric scratch | Per-tokenizer lattice | Bounded tiles, scores and lattice |

The core file comparison includes `sys.dic`, `matrix.bin`, `unk.dic` and `char.bin`. It excludes Python and native library overhead on both sides.

The builder checks every trie edge and terminal against the original index. It verifies every encoded matrix tile and exact duplicate-context remapping. This checks structural preservation; generated reference comparisons separately check output behavior.

## Reproduction

Use a disposable Python environment for the original packages:

```sh
python3 -m venv .reference
.reference/bin/pip install cutlet==0.4.0 fugashi==1.3.3 unidic-lite==1.0.8 jaconv==0.3.4 mojimoji==0.0.13
.reference/bin/python tools/verify.py --reference
.reference/bin/python tools/benchmark.py --engine cutlet --threads 6 --distinct 50000
.reference/bin/python tools/benchmark.py --engine native --threads 6 --distinct 50000
.reference/bin/python tools/latency.py --engine cutlet
.reference/bin/python tools/latency.py --engine native
```

Run each command separately on an idle machine. The stress test warms each thread with 4,096 public dictionary names, then converts 50,000 distinct synthetic fields per thread. Compare `output_sha256` before comparing timing.

Linux reports proportional set size (PSS) and private memory from `/proc/self/smaps_rollup`. RSS can count the same dictionary pages repeatedly when each original tokenizer maps the dictionary. PSS is the better comparison for physical memory pressure. The second memory snapshot is taken while all worker threads remain alive.

The vocabulary is a systematic sample of proper nouns. The synthetic workload tests retained memory and lookup overhead; it does not represent a measured production distribution. Timings depend on input, allocator, CPU and filesystem cache. Startup measurements use fresh processes with a warm operating-system file cache.

The Python binding keeps the GIL for short fields. Rust callers can run concurrently. Treat Python thread throughput and native parallelism as separate measurements.
