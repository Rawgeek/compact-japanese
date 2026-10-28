# compact-japanese 0.1.0 technical report

This report evaluates `compact-japanese` 0.1.0 against the implementation it replaces: **Cutlet 0.4.0**, configured with `Cutlet("hepburn", use_foreign_spelling=False)`, using **Fugashi 1.3.3** and **unidic-lite 1.0.8**. The scope is short Japanese names, addresses and labels. This is an engineering comparison of one fixed romanization configuration, not a claim that every Japanese romanization method produces the same text.

The project is public at [github.com/Rawgeek/compact-japanese](https://github.com/Rawgeek/compact-japanese). Version 0.1.0 pins Rust crate revision `299af4ec1f4a382135654d46946e74d51026236f`.

## Results at a glance

| Criterion | Original stack | compact-japanese | Result |
|---|---:|---:|---:|
| Vocabulary candidates | 756,264 | 756,264 | Preserved |
| Unknown-word candidates | 35 | 35 | Preserved |
| Four core dictionary files | 259,493,567 B | 28,816,754 B | 88.90% smaller |
| Complete installed Apple Silicon runtime | Not comparable | 29,329,463 B | 2,670,537 B below 32,000,000 B |
| Cargo dependencies | Not applicable | 0 | No crates |
| Python runtime package dependencies | 5 | 0 | Standard library only |
| macOS ARM, 6 Python threads | 66,654 fields/s | 176,276 fields/s | 2.64× faster |
| Linux ARM, 6 Python threads | 71,719 fields/s | 199,698 fields/s | 2.78× faster |
| Linux ARM CPU cost | 14.17 µs/field | 5.08 µs/field | 64.2% lower |
| Linux ARM warm PSS | 190.29 MiB | 41.54 MiB | 78.2% lower |
| Linux ARM PSS after distinct inputs | 251.91 MiB | 41.59 MiB | 83.5% lower |
| Linux ARM private growth after warmup | 61.625 MiB | 0.027 MiB | Bounded |
| Dictionary mappings with 6 tokenizers | 12 | 1 | Shared immutable model |
| macOS startup median | 24.01 ms | 8.88 ms | 63.0% lower |
| Linux startup median | 15.50 ms | 18.92 ms | 3.42 ms slower |
| Matching comparison inputs | Baseline | 24,096 / 24,096 | No output differences |

The installed size covers the dictionary, native library, Python binding and license notices. It is uncompressed. The measured process itself is larger than 32 MB because Python, the memory mapping and runtime scratch share the address space. The budget describes installed files.

## Environment and workload

| Item | macOS run | Linux run |
|---|---|---|
| CPU | Apple M1 Max, 64-bit ARM | 64-bit ARM, 2-CPU Docker limit |
| Memory limit | Host default | 2 GiB |
| Python | 3.13 | 3.11.17 |
| Platform | macOS 27.0.1, `aarch64-apple-darwin` | Linux 7.0.14, glibc 2.41, `aarch64-unknown-linux-gnu` |
| Rust | 1.95.0 | 1.95.0 |
| Reference packages | Cutlet 0.4.0, Fugashi 1.3.3, unidic-lite 1.0.8 | Same |

Each engine ran in a fresh process. The stress workload warms threads with 4,096 systematically sampled proper nouns, then converts 50,000 distinct fields per thread. Output SHA-256 hashes must match before timing is compared. Five separate fresh processes measured startup, alternating engines. Latency used 100 warmup calls and 2,000 measured calls per field. The machine was otherwise idle, and the filesystem cache was warm.

These are synthetic field workloads. They measure lookup, tokenization, conversion, binding overhead and retained memory. They do not predict every production input distribution or Heroku dyno topology.

## Correctness and retained behavior

| Check | Result |
|---|---|
| Full source vocabulary | 756,264 candidates retained |
| Full unknown vocabulary | 35 candidates retained |
| Trie states and terminals | Every source state and terminal verified by the builder |
| Connection scores | Every retained signed score verified, including duplicate-context remapping |
| 4,096 public-word plus generated cases | 24,096 exact matches to the reference |
| Generated mixed-script suite | 20,000 exact matches on macOS ARM and Linux ARM |
| Shipping-field migration suite | 444 exact matches; development-only fixtures are not published |
| Six-thread stress output | All 300,000 fields produced matching per-thread digests |
| Public CI | Passed on macOS ARM and Linux x86 |

Compatibility includes Cutlet's configured Modified Hepburn output, candidate order, strict Viterbi tie handling, unknown-word grouping, capitalization, spacing and known exceptions. Unknown non-kana characters can produce `?`; applications can reject that result. Unsupported readings return an error. These limits are part of the target behavior.

The public builder checks dictionary structure. Reference comparisons check behavior. Neither establishes correctness for every possible Japanese string or every Unicode input. Callers should retain the original field when `?` is unsuitable.

## Size and representation

| Data | Original | compact-japanese | Purpose |
|---|---:|---:|---|
| `sys.dic` | 187,680,870 B | 13,926,848 B | LOUDS trie, packed candidates and deduplicated readings |
| `unk.dic` | 5,475 B | Included above | 35 unknown candidates and category ranges |
| `matrix.bin` | 71,544,726 B | 14,627,542 B | Deduplicated, independently compressed connection contexts |
| `char.bin` | 262,496 B | 262,236 B | Character types and unknown-category lookup |
| Container header | None | 128 B | Version, counts, offsets and section lengths |
| **Core total** | **259,493,567 B** | **28,816,754 B** | Exact source scores and candidates |
| Compressed data release | 47,356,746 B source archive | 16,521,903 B | Installable artifact |

The final dictionary is one memory-mappable file:

| Section | Bytes |
|---|---:|
| LOUDS byte trie | 4,798,656 |
| Packed vocabulary and unknown candidates | 6,050,392 |
| Deduplicated reading and rendering records | 3,077,800 |
| Compressed connection matrix | 14,627,542 |
| Character classes and unknown ranges | 262,236 |
| Header | 128 |
| **Total** | **28,816,754** |

The 456,000-byte macOS ARM dynamic library, 2,106-byte Python binding and license notices bring the complete installed public runtime to 29,329,463 bytes. The release dictionary SHA-256 is `ebb5bbe30325025108154435d56a0a81d4c710c73160823c3cc4c781345c4c4e`.

## Compression design

The original double-array index stores sparse transitions and per-token metadata. The replacement uses a bit-packed LOUDS trie with a terminal bitmap, rank samples and 32-bit terminal values. It preserves byte-key ordering, every terminal candidate and candidate order.

Token records are 8 bytes instead of 16. Left and right context IDs are remapped through verified deduplication maps, word costs retain their signed 16-bit values, and feature offsets address a deduplicated pool. Only the reading, rendering flags and information needed for token category, preposition spacing, proper-name capitalization and unknown handling remain.

Equal matrix rows and columns are interned first. Each 32×32 tile is transformed with reversible second differences, zigzag encoded as variable-length integers and compressed with an LZ4 block codec implemented in the crate. Only requested tiles are expanded. Equal contexts are measured for exact duplicates during build; retained scores remain identical.

There is no training data, full feature schema, accent metadata, N-best output, model conversion, configurable romanization mode or MeCab runtime in the installed artifact.

## Runtime and memory model

One read-only mapping contains the complete dictionary. `Romanizer` clones share it across threads. The operating system can demand-page sections rather than loading all 28.8 MB into private heap memory.

Parsing state is thread-local and bounded:

| Resource | Bound |
|---|---:|
| Expanded matrix tiles | 256 KiB per active thread |
| Individual numeric score cache | 64 KiB per active thread |
| Lattice nodes | 262,144 |
| Input | 16 KiB |
| Retained token, input or output history | 0 after a call |

The C ABI reports required capacity without partially writing a short output buffer. Invalid UTF-8, NUL bytes, oversized fields, malformed dictionaries, invalid contexts and decompression failures return errors. An ABI version prevents accidental mismatch. Closing must not race active calls. The immutable artifact must remain in place while mapped.

The Python binding uses `ctypes`; it does not depend on a specific Python extension ABI. Inputs of at most 256 UTF-8 bytes keep the GIL to avoid transition overhead. Longer calls release it. Python's 6-thread throughput therefore stays near its one-thread throughput; use Rust workers when CPU parallelism matters.

## Throughput

| Platform | Threads | Original fields/s | compact-japanese fields/s | Change |
|---|---:|---:|---:|---:|
| macOS ARM | 6 | 66,654 | 176,276 | 2.64× |
| Linux ARM | 1 | 72,844 | 198,280 | 2.72× |
| Linux ARM | 6 | 71,719 | 199,698 | 2.78× |

| Platform | Threads | Original CPU µs/field | compact-japanese CPU µs/field | Change |
|---|---:|---:|---:|---:|
| macOS ARM | 6 | 12.03 | 4.79 | 60.2% lower |
| Linux ARM | 1 | 13.73 | 5.04 | 63.3% lower |
| Linux ARM | 6 | 14.17 | 5.08 | 64.2% lower |

## Latency

Linux ARM warm latency in microseconds:

| Field | Original p50 | New p50 | Original p95 | New p95 | Original p99 | New p99 |
|---|---:|---:|---:|---:|---:|---:|
| Name | 8.75 | 3.21 | 12.04 | 7.92 | 19.08 | 17.88 |
| Address | 28.13 | 5.46 | 33.13 | 18.33 | 90.25 | 86.38 |
| Katakana label | 15.46 | 12.38 | 20.83 | 14.17 | 42.58 | 33.96 |
| ASCII label | 6.54 | 2.00 | 8.21 | 2.42 | 21.79 | 4.92 |
| Repeated address, 24 copies | 727.94 | 95.58 | 2330.50 | 318.50 | 7538.13 | 1608.83 |

macOS ARM warm latency in microseconds:

| Field | Original p50 | New p50 | Original p95 | New p95 | Original p99 | New p99 |
|---|---:|---:|---:|---:|---:|---:|
| Name | 8.96 | 3.33 | 17.96 | 3.46 | 93.46 | 3.67 |
| Address | 27.08 | 5.42 | 28.58 | 5.67 | 154.42 | 12.83 |
| Katakana label | 15.33 | 12.21 | 16.92 | 15.67 | 75.71 | 79.00 |
| ASCII label | 6.25 | 2.33 | 6.79 | 2.83 | 10.54 | 3.04 |
| Repeated address, 24 copies | 639.73 | 87.17 | 4617.75 | 160.96 | 16293.08 | 390.54 |

The macOS katakana p99 is a measured regression. Tail measurements are sensitive to scheduling, allocator behavior and filesystem cache; the raw samples and percentiles are published.

## Startup

| Platform | Sample | Original ms | compact-japanese ms |
|---|---:|---:|---:|
| macOS ARM | Median of 5 fresh processes | 24.01 | 8.88 |
| Linux ARM | Median of 5 fresh processes | 15.50 | 18.92 |

Startup includes import, dictionary mapping, model initialization and one conversion. The native implementation is slower on this Linux image. Faster steady-state conversion and lower retained memory remain the measured benefits there.

## Linux memory details

| Threads | Stage | Metric | Original | compact-japanese |
|---:|---|---:|---:|---:|
| 1 | Warm | PSS | 181.49 MiB | 39.66 MiB |
| 1 | After distinct fields | PSS | 190.69 MiB | 39.71 MiB |
| 1 | After distinct fields | Private | 185.81 MiB | 34.81 MiB |
| 1 | After distinct fields | Mappings | 2 | 1 |
| 6 | Warm | PSS | 190.29 MiB | 41.54 MiB |
| 6 | After distinct fields | PSS | 251.91 MiB | 41.59 MiB |
| 6 | After distinct fields | Private | 80.10 MiB | 36.69 MiB |
| 6 | After distinct fields | Mappings | 12 | 1 |
| 6 | After distinct fields | Peak RSS | 1,035.31 MiB | 54.78 MiB |

PSS apportions shared pages, while summed RSS counts every mapped copy and can overstate the original stack's physical memory. PSS is the primary cross-engine comparison. The new implementation starts with more private scratch than the original before warmup, then remains essentially flat: 0.027 MiB private growth across 300,000 distinct calls with six live workers.

The macOS measurement reports only RSS. It fell from 581.52 MiB to 48.13 MiB after the six-thread distinct-input run. macOS mapping semantics and absent PSS data limit direct physical-memory conclusions.

## Packaging and build characteristics

| Artifact | Size |
|---|---:|
| Public dictionary archive | 16,521,903 B |
| Public macOS ARM runtime archive | 16,718,110 B |
| Public Linux ARM runtime archive | 16,723,348 B |
| Final lossless dictionary | 28,816,754 B |
| macOS ARM dynamic library | 456,000 B |

The Rust source is about 2,076 lines, has no Cargo dependencies and builds with Rust 1.95.0. Dictionary construction downloads the checksum-pinned 47,356,746-byte UniDic source archive, verifies its SHA-256, and writes a fresh artifact atomically. Consumers can instead download the 16.5 MB dictionary release.

The builder verifies source trie positions, terminal ranges, character classes, matrix source scores and exact context maps before publishing. Runtime startup validates the header, file sizes and small code files without reading and hashing the full dictionary. This avoids faulting every page merely to start a process.

## Reproduction

```sh
git clone https://github.com/Rawgeek/compact-japanese.git
cd compact-japanese
rustup toolchain install 1.95.0 --profile minimal
python3 tools/build_data.py

cargo fmt --check
cargo clippy --all-targets --locked --offline -- -D warnings
COMPACT_JAPANESE_DICTIONARY="$PWD/data/dictionary.bin" cargo test --all-targets --locked --offline
python3 tools/verify.py

python3.13 -m venv .reference
.reference/bin/pip install cutlet==0.4.0 fugashi==1.3.3 unidic-lite==1.0.8 jaconv==0.3.4 mojimoji==0.0.13
.reference/bin/python tools/verify.py --reference
.reference/bin/python tools/benchmark.py --engine cutlet --threads 6 --distinct 50000
.reference/bin/python tools/benchmark.py --engine native --threads 6 --distinct 50000
.reference/bin/python tools/latency.py --engine cutlet
.reference/bin/python tools/latency.py --engine native
```

Compare output hashes before timing. On Linux, inspect PSS and private memory in `/proc/self/smaps_rollup`; do not use summed RSS as a proxy for unique physical pages. Raw JSON measurements and digests are in [benchmarks/results](benchmarks/results). A compact machine-readable summary is in [benchmarks/report.json](benchmarks/report.json).

## Limitations and conclusion

The implementation targets the measured short-field use case. It rejects inputs over 16 KiB. Its fixed dictionary has no user vocabulary, runtime updates or alternative romanization mode. Output intentionally follows one Cutlet configuration, including its established mistakes and unknown-character behavior. Windows is not implemented. Linux ARM, macOS ARM and Linux x86 verification are the current evidence; broader application input distributions, long-running worker memory and production dyno behavior remain separate measurements.

Within that scope, the complete dictionary shrinks by 88.90% without dropping a candidate or changing a score. The public installed runtime is below 32 MB. The measured core is faster, uses substantially less process memory after warmup and retains one shared mapping. The Linux startup regression, Python GIL plateau, small tail-latency regressions and 16 KiB bound are published rather than averaged away.

## Originals and licenses

* [Cutlet](https://github.com/polm/cutlet), MIT, by Paul McCann.
* [MeCab](https://taku910.github.io/mecab/) and [source](https://github.com/taku910/mecab), BSD, by Taku Kudo and NTT.
* [UniDic](https://clrd.ninjal.ac.jp/unidic/), BSD, by the UniDic Consortium.
* [unidic-lite](https://github.com/polm/unidic-lite), by Paul McCann, packaging UniDic 2.1.2.
* [Fugashi](https://github.com/polm/fugashi), MIT, by Paul McCann; used only for reference comparisons.
* [LZ4 block format](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md), implemented here without an external library.

New code is MIT licensed. Inherited MIT and BSD notices remain in [LICENSE.cutlet](LICENSE.cutlet), [LICENSE.mecab](LICENSE.mecab) and the generated dictionary's license directory.
