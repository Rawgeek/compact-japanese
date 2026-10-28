#!/usr/bin/env python3
"""Build and verify the complete lossless dictionary using only stdlib Python."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

from dictionary import build_dictionary

ROOT = Path(__file__).resolve().parents[1]


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def reading_compiler(library: Path):
    native = ctypes.CDLL(str(library))
    convert = native.cj_compile_reading
    convert.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    convert.restype = ctypes.c_int32

    def reading(text: str) -> bytes | None:
        encoded = text.encode()
        output = ctypes.create_string_buffer(len(encoded) * 4 + 32)
        required = ctypes.c_size_t()
        status = convert(encoded, len(encoded), output, len(output), ctypes.byref(required))
        if status == 3:
            return None
        if status or required.value > len(output):
            raise ValueError(f'reading compiler failed: {status}')
        return output.raw[:required.value]
    return reading


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'data')
    parser.add_argument('--archive', type=Path, help='Use an already downloaded pinned source archive')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        parser.error('output directory already exists; choose a fresh directory')
    pin = json.loads((ROOT / 'tools/inputs.json').read_text())['dictionary']
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    archive = args.archive or cache / 'unidic-lite-1.0.8.tar.gz'
    expected = pin['hash'].removeprefix('sha256:')
    if not archive.is_file():
        with urllib.request.urlopen(pin['url'], timeout=60) as response, tempfile.NamedTemporaryFile(dir=cache, delete=False) as pending:
            path = Path(pending.name)
            try:
                remaining = pin['size']
                while chunk := response.read(min(1024 * 1024, remaining + 1)):
                    remaining -= len(chunk)
                    if remaining < 0:
                        raise ValueError('archive exceeds pinned length')
                    pending.write(chunk)
                pending.flush()
                if remaining or digest(path) != expected:
                    raise ValueError('archive checksum mismatch')
                path.replace(archive)
            finally:
                path.unlink(missing_ok=True)
    if archive.stat().st_size != pin['size'] or digest(archive) != expected:
        raise ValueError('source archive checksum or length mismatch')
    subprocess.run(['cargo', 'build', '--release', '--locked', '--offline'], cwd=ROOT, check=True)
    binaries = ROOT / 'target/release'
    library = binaries / ('libcompact_japanese.dylib' if sys.platform == 'darwin' else 'libcompact_japanese.so')
    with tempfile.TemporaryDirectory(prefix='compact-japanese-build-') as temporary:
        source = Path(temporary) / 'source'
        source.mkdir()
        licenses = Path(temporary) / 'licenses'
        licenses.mkdir()
        wanted = {'sys.dic', 'unk.dic', 'matrix.bin', 'char.bin'}
        with tarfile.open(archive, 'r:gz') as bundle:
            for member in bundle:
                name = Path(member.name).name
                is_data = name in wanted and '/unidic_lite/dicdir/' in member.name
                is_license = name in {'BSD', 'GPL', 'LGPL', 'COPYING', 'LICENSE', 'LICENSE.unidic'}
                if not member.isfile() or not (is_data or is_license):
                    continue
                stream = bundle.extractfile(member)
                if stream is None:
                    raise ValueError('missing archive member')
                destination = source / name if is_data else licenses / ('unidic-' + name)
                with stream, destination.open('wb') as target:
                    shutil.copyfileobj(stream, target)
        if {p.name for p in source.iterdir()} != wanted:
            raise ValueError('dictionary archive is incomplete')
        result = build_dictionary(source, output, reading_compiler(library), binaries)
        shutil.copytree(licenses, output / 'licenses')
    for name in ('LICENSE', 'LICENSE.cutlet', 'LICENSE.mecab'):
        shutil.copyfile(ROOT / name, output / 'licenses' / name)
    manifest = {
        'format': 2, 'source': pin, 'dictionary': result,
        'files': {p.relative_to(output).as_posix(): {'size':p.stat().st_size, 'sha256':digest(p)} for p in sorted(output.rglob('*')) if p.is_file()},
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'dictionary_bytes':(output/'dictionary.bin').stat().st_size, 'sha256':digest(output/'dictionary.bin')}, indent=2))


if __name__ == '__main__':
    main()
