#!/usr/bin/env python3
"""Compare Japanese core throughput and retained memory in fresh processes.

Run native and cutlet separately. The cutlet engine needs the old, optional
reference packages; none is a runtime dependency. Inputs are public fixtures
and synthetic addresses. Linux reports PSS/private memory as well as RSS, so
multiple mappings of the same dictionary are not mistaken for private RAM.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import platform
import resource
import statistics
import subprocess
import sys
import threading
import time
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))


def memory() -> dict:
    result = {
        "peak_rss_mib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        / (1024**2 if platform.system() == "Darwin" else 1024)
    }
    rollup = Path("/proc/self/smaps_rollup")
    if rollup.exists():
        values = {
            line.split(":", 1)[0]: int(line.split()[1])
            for line in rollup.read_text().splitlines()
            if ":" in line and line.split()[1].isdigit()
        }
        result.update(
            {
                "rss_mib": values["Rss"] / 1024,
                "pss_mib": values["Pss"] / 1024,
                "private_mib": (values["Private_Clean"] + values["Private_Dirty"]) / 1024,
            }
        )
        maps = Path("/proc/self/maps").read_text().splitlines()
        result["dictionary_mappings"] = sum(line.endswith(("/sys.dic", "/matrix.bin", "/dictionary.bin")) for line in maps)
    elif platform.system() == "Darwin":
        result["rss_mib"] = (
            int(subprocess.check_output(["/bin/ps", "-o", "rss=", "-p", str(os.getpid())], text=True)) / 1024
        )
    return result


def suffix(index: int) -> str:
    letters = []
    for _ in range(8):
        index, digit = divmod(index, 26)
        letters.append(chr(97 + digit))
    return "".join(letters)


def run(engine: str, threads: int, distinct: int, corpus: Path | None) -> dict:
    setup_started = time.perf_counter()
    if engine == "native":
        sys.path.insert(0, str(ROOT / "python"))
        from compact_japanese import Romanizer
        library = ROOT / "target/release" / ("libcompact_japanese.dylib" if sys.platform == "darwin" else "libcompact_japanese.so")
        shared = Romanizer(library, ROOT / "data/dictionary.bin")
        converter_factory = lambda: shared.romanize
    else:
        import cutlet
        converter_factory = lambda: cutlet.Cutlet("hepburn", use_foreign_spelling=False).romaji
    setup_seconds = time.perf_counter() - setup_started
    if corpus is not None:
        data = json.loads(corpus.read_text())
        words = data.get("vocabulary", [row["input"] for row in data.get("cases", [])])
    else:
        words = json.loads((ROOT / "benchmarks/vocabulary.json").read_text())["vocabulary"]
    words = [unicodedata.normalize("NFKC", word) for word in words if word]
    ready = threading.Barrier(threads + 1)
    proceed = threading.Barrier(threads + 1)

    def worker(number: int) -> str:
        convert = converter_factory()
        digest = hashlib.sha256()
        for word in words:
            try:
                result = convert(word)
            except Exception:
                result = word
            digest.update(result.encode())
        ready.wait(timeout=120)
        proceed.wait(timeout=120)
        for index in range(distinct):
            text = "東京都 zqx" + suffix(index + number * distinct)
            digest.update(convert(text).encode())
        ready.wait(timeout=120)
        proceed.wait(timeout=120)
        return digest.hexdigest()

    before = memory()
    with concurrent.futures.ThreadPoolExecutor(max_workers=threads) as pool:
        jobs = [pool.submit(worker, number) for number in range(threads)]
        ready.wait(timeout=120)
        warm = memory()
        started = time.perf_counter()
        cpu_started = time.process_time()
        proceed.wait(timeout=120)
        ready.wait(timeout=120)
        duration = time.perf_counter() - started
        cpu_seconds = time.process_time() - cpu_started
        retained = memory()
        proceed.wait(timeout=120)
        hashes = [job.result() for job in jobs]
    return {
        "engine": engine,
        "platform": platform.platform(),
        "threads": threads,
        "vocabulary_inputs": len(words),
        "distinct_per_thread": distinct,
        "before": before,
        "warm": warm,
        "after_distinct": retained,
        "setup_seconds": setup_seconds,
        "seconds": duration,
        "cpu_seconds": cpu_seconds,
        "cpu_microseconds_per_call": cpu_seconds * 1e6 / (threads * distinct),
        "calls_per_second": threads * distinct / duration,
        "output_sha256": hashes,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=("native", "cutlet"), default="native")
    parser.add_argument("--threads", type=int, default=6)
    parser.add_argument("--distinct", type=int, default=50000)
    parser.add_argument("--corpus", type=Path)
    args = parser.parse_args()
    if not 1 <= args.threads <= 64 or not 1 <= args.distinct <= 1000000:
        parser.error("threads must be 1..64 and distinct must be 1..1000000")
    print(json.dumps(run(args.engine, args.threads, args.distinct, args.corpus), indent=2))


if __name__ == "__main__":
    main()
