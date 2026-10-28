# Measurements

The complete methodology, limitations, full latency distribution above and machine-readable evidence are in [REPORT.md](REPORT.md).

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

## Measured results

Measured on 28 October 2026 with an Apple M1 Max. macOS used ARM Python 3.13. Linux used ARM Python 3.11 in Docker, limited to two CPUs and 2 GiB. The Rust compiler was 1.95.0 with the checked-in release profile. Reference packages match the versions below.

| Criterion | Original | compact-japanese | Change |
|---|---:|---:|---:|
| macOS, 6 Python threads, fields/second | 66,654 | 176,276 | 2.64× |
| Linux, 1 Python thread, fields/second | 72,844 | 198,280 | 2.72× |
| Linux, 6 Python threads, fields/second | 71,719 | 199,698 | 2.78× |
| Linux, 6 threads, CPU µs/field | 14.17 | 5.08 | 64% lower |
| Linux, 6 threads, warm PSS | 190.29 MiB | 41.54 MiB | 78% lower |
| Linux, 6 threads, PSS after distinct fields | 251.91 MiB | 41.59 MiB | 83% lower |
| Linux, 6 threads, private growth during distinct fields | 61.625 MiB | 0.027 MiB | Bounded after warmup |
| Linux, live dictionary mappings | 12 | 1 | Shared model |
| macOS, median import + initialization + first call | 24.01 ms | 8.88 ms | 63% lower |
| Linux, median import + initialization + first call | 15.50 ms | 18.92 ms | 3.42 ms slower |

The startup medians use five fresh processes per engine, alternating engines. These are warm-filesystem-cache measurements. Native startup is slower on the tested Linux image. This is a measured tradeoff, despite faster steady-state conversion.

Linux warm per-field latency, microseconds:

| Field | Original p50 / p95 / p99 | compact-japanese p50 / p95 / p99 |
|---|---:|---:|
| Name | 8.75 / 12.04 / 19.08 | 3.21 / 7.92 / 17.88 |
| Address | 28.13 / 33.13 / 90.25 | 5.46 / 18.33 / 86.38 |
| Katakana label | 15.46 / 20.83 / 42.58 | 12.38 / 14.17 / 33.96 |
| ASCII label | 6.54 / 8.21 / 21.79 | 2.00 / 2.42 / 4.92 |
| Repeated address, 24 copies | 727.94 / 2330.50 / 7538.13 | 95.58 / 318.50 / 1608.83 |

These timings include each Python binding's conversion call. Applications may bypass the dictionary entirely for ASCII fields. Tail latency has visible scheduling noise; the raw macOS measurements also include a worse native p99 for the katakana case.

All stress output digests matched. The public generated comparison passed 20,000 mixed-script and unknown-character inputs on macOS ARM and Linux ARM. The builder's structural checks cover the complete dictionary. These comparisons establish the tested behavior, rather than a general claim about linguistic quality.

Raw measurements, output hashes, startup samples and latency percentiles are in [benchmarks/results](benchmarks/results). PSS includes the Python process: the total process uses more than 32 MB. The size budget applies to the installed runtime; bounded native scratch is additional process memory.

## Reproduction

Use a disposable Python environment for the original packages:

```sh
python3.13 -m venv .reference
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
