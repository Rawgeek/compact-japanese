#!/usr/bin/env python3
"""Create reproducible source-free runtime archives with license notices."""
import gzip
import hashlib
from pathlib import Path
import platform
import tarfile

ROOT = Path(__file__).resolve().parents[1]
output = ROOT / 'dist'
output.mkdir(exist_ok=True)

def archive(name: str, files: list[Path]) -> Path:
    destination = output / name
    with destination.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed, tarfile.open(fileobj=compressed, mode='w') as bundle:
        for path in sorted(files):
            info = bundle.gettarinfo(str(path), arcname=path.relative_to(ROOT).as_posix())
            info.uid = info.gid = info.mtime = 0
            info.uname = info.gname = ''
            info.mode = 0o644
            with path.open('rb') as source:
                bundle.addfile(info, source)
    return destination

data = [p for p in (ROOT/'data').rglob('*') if p.is_file()]
created = [archive('compact-japanese-data-v2.tar.gz', data)]
system = {'Darwin':'macos', 'Linux':'linux'}[platform.system()]
machine = {'arm64':'arm64', 'aarch64':'arm64', 'x86_64':'x86_64'}[platform.machine()]
extension = 'dylib' if system == 'macos' else 'so'
files = data + [ROOT/f'target/release/libcompact_japanese.{extension}', ROOT/'python/compact_japanese.py', ROOT/'README.md', ROOT/'LICENSE', ROOT/'LICENSE.cutlet', ROOT/'LICENSE.mecab', ROOT/'include/compact_japanese.h']
created.append(archive(f'compact-japanese-v0.1.0-{system}-{machine}.tar.gz', files))
lines = [f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in created]
(output/'SHA256SUMS').write_text(''.join(lines))
print(''.join(lines), end='')
