# compact-japanese

Japanese to Modified Hepburn in Rust, with **zero Cargo dependencies** and a complete installed dictionary and library under **32 MB**.

It is designed for short names, addresses and labels. A single immutable dictionary is shared across threads. Scratch memory is bounded, and input strings are never cached.

The dictionary occupies **28,816,754 bytes**. Every one of the original **756,264 dictionary candidates** and **35 unknown-word candidates** is retained. Compression preserves all connection scores exactly.

## Quick start on Apple Silicon

M1, M2 and later Macs use `aarch64-apple-darwin`. No Rosetta is needed.

Install [Rust](https://rustup.rs/) and Python 3.10 or later, then:

```sh
git clone https://github.com/Rawgeek/compact-japanese.git
cd compact-japanese
rustup toolchain install 1.95.0 --profile minimal
python3 tools/build_data.py
cargo run --release --example romanize -- data/dictionary.bin '東京都渋谷区'
```

Output:

```text
Tokyo to Shibuya ku
```

The build downloads a checksum-pinned `unidic-lite` archive. It verifies every trie state, terminal and retained matrix score. Python is needed only for data preparation or the optional binding. Runtime conversion never downloads data or runs a compiler.

Prebuilt dictionaries and Apple Silicon binaries are available from [Releases](https://github.com/Rawgeek/compact-japanese/releases). Verify the accompanying SHA-256 sums before use. Keep the dictionary and its license directory together.

## Rust

Add the Git dependency, preferably pinned to a release commit:

```toml
[dependencies]
compact-japanese = { git = "https://github.com/Rawgeek/compact-japanese", tag = "v0.1.0" }
```

```rust
use compact_japanese::Romanizer;

let romanizer = Romanizer::open("data/dictionary.bin")?;
assert_eq!(romanizer.romanize("山田太郎")?, "Yamada Tarou");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Clone `Romanizer` to share its read-only mapping between threads. The Rust and C interfaces accept UTF-8 that the caller has normalized to NFKC. The Python binding applies NFKC itself.

## Python

The binding uses only the standard library and a plain C ABI. It requires no Python extension ABI or Python package dependencies.

```python
import sys
sys.path.insert(0, "python")
from compact_japanese import Romanizer

romanizer = Romanizer(
    "target/release/libcompact_japanese.dylib",  # .so on Linux
    "data/dictionary.bin",
)
print(romanizer.romanize("山田太郎"))  # Yamada Tarou
```

Calls of at most 256 UTF-8 bytes keep Python's GIL to avoid transition overhead. Longer calls release it. Native Rust callers execute concurrently without this Python constraint.

## What is retained and removed

| Component | Representation |
|---|---|
| Surface lookup | 4,798,656-byte LOUDS byte trie |
| Candidates | 6,050,392 bytes; packed context IDs, word cost and feature offset |
| Readings and rendering flags | 3,077,800 bytes; deduplicated precomputed romaji |
| Connection scores | 14,627,542 bytes; exact context deduplication and independently compressed tiles |
| Character classes and unknown categories | 262,236 bytes |
| Container header | 128 bytes |

Equal matrix rows and columns are interned. Each remaining 32×32 tile uses reversible second differences, zigzag integers and LZ4 block encoding. Only requested tiles are expanded. Each thread retains at most 256 KiB of numeric tiles and 64 KiB of individual numeric scores, plus bounded lattice scratch.

Unused grammatical metadata, accent fields, general output formats, model training, N-best parsing and configurable romanization modes are omitted. There is no MeCab shared-library dependency and no runtime decompression of the complete matrix.

## Compatibility and limits

The target behavior is [Cutlet 0.4.0](https://github.com/polm/cutlet), configured as `Cutlet("hepburn", use_foreign_spelling=False)`, with [unidic-lite 1.0.8](https://github.com/polm/unidic-lite). Candidate order, strict Viterbi tie handling and unknown-word grouping follow [MeCab 0.996](https://taku910.github.io/mecab/).

This deliberately preserves that configuration's capitalization, spacing, exceptions and sometimes surprising name readings. Unknown non-kana characters can produce `?`. Callers can preserve the original input when that occurs. Unsupported readings return an error.

Inputs are limited to 16 KiB and 262,144 lattice nodes. NUL bytes are rejected. These bounds suit fields, rather than documents. Error paths leave caller output buffers untouched. A malformed or missing dictionary returns an error.

macOS and GNU/Linux on 64-bit ARM and x86 are supported by the source. Apple Silicon and Linux ARM are tested locally; GitHub Actions also verifies Linux x86 and macOS ARM. Windows support is not implemented.

See [BENCHMARKS.md](BENCHMARKS.md) for measurements and reproduction commands. Size means uncompressed installed bytes, including the dictionary, library, binding and license notices.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets --locked --offline -- -D warnings
COMPACT_JAPANESE_DICTIONARY="$PWD/data/dictionary.bin" cargo test --all-targets --locked --offline
python3 tools/verify.py
```

For an optional reference comparison, install the original packages in a disposable environment, then run `python3 tools/verify.py --reference`. Reference packages are development tools only.

## Originals and licenses

This project depends on the work of the original authors:

* [Cutlet](https://github.com/polm/cutlet), by Paul McCann: kana mappings, romanization rules and exceptions. Its MIT notice is preserved in [LICENSE.cutlet](LICENSE.cutlet).
* [MeCab](https://taku910.github.io/mecab/) / [source](https://github.com/taku910/mecab), by Taku Kudo and NTT: dictionary format and compatible parsing behavior. The BSD notice is preserved in [LICENSE.mecab](LICENSE.mecab).
* [UniDic](https://clrd.ninjal.ac.jp/unidic/), by the UniDic Consortium: vocabulary and connection scores. Its BSD license accompanies every generated dictionary.
* [unidic-lite](https://github.com/polm/unidic-lite), by Paul McCann: the pinned packaging of UniDic 2.1.2, including its documented dictionary adjustments. Original notices accompany generated data.
* [Fugashi](https://github.com/polm/fugashi): the reference Python interface used by Cutlet. It is used only for development comparisons.
* [LZ4 block format](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md): the block encoding implemented here without an external library.

New code is MIT licensed; inherited BSD and MIT obligations remain in force. See [LICENSE](LICENSE), [LICENSE.mecab](LICENSE.mecab), [LICENSE.cutlet](LICENSE.cutlet) and the generated data's `licenses/` directory. This project is independent of the original projects.
