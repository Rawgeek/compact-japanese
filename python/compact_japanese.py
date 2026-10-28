"""Minimal stdlib Python binding. Build/download the library and dictionary first."""
from __future__ import annotations

import ctypes
import os
from pathlib import Path
import unicodedata


class Romanizer:
    """One shared, read-only dictionary; bounded native scratch per calling thread."""
    def __init__(self, library: str | Path, dictionary: str | Path):
        lib = ctypes.CDLL(str(Path(library).resolve()))
        lib.cj_abi_version.restype = ctypes.c_uint32
        if lib.cj_abi_version() != 2:
            raise RuntimeError('native ABI mismatch')
        lib.cj_open.argtypes = [ctypes.c_char_p]
        lib.cj_open.restype = ctypes.c_void_p
        lib.cj_close.argtypes = [ctypes.c_void_p]
        lib.cj_close.restype = None
        lib.cj_romanize.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        lib.cj_romanize.restype = ctypes.c_int32
        self._context = lib.cj_open(os.fsencode(Path(dictionary).resolve()))
        if not self._context:
            raise ValueError('invalid or unavailable dictionary')
        self._library = lib
        self._short = ctypes.PYFUNCTYPE(ctypes.c_int32, *lib.cj_romanize.argtypes)(('cj_romanize', lib))

    def __del__(self):
        if getattr(self, '_context', None):
            self._library.cj_close(self._context)
            self._context = None

    def romanize(self, text: str) -> str:
        encoded = unicodedata.normalize('NFKC', text).encode('utf-8')
        if len(encoded) > 16384 or b'\0' in encoded:
            raise ValueError('input contains NUL or exceeds 16 KiB')
        output = ctypes.create_string_buffer(len(encoded) * 4 + 64)
        required = ctypes.c_size_t()
        call = self._short if len(encoded) <= 256 else self._library.cj_romanize
        status = call(self._context, encoded, len(encoded), output, len(output), ctypes.byref(required))
        if status or required.value > len(output):
            raise ValueError(f'romanization failed: {status}')
        return output.raw[:required.value].decode('utf-8')
