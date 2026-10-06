#!/usr/bin/env python3
"""Compare the boot probe's SHA256 with hashlib, including padding boundaries."""
import ctypes
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile

header = Path(__file__).resolve().parents[1] / "sha256.h"
with tempfile.TemporaryDirectory() as temporary:
    folder = Path(temporary)
    source = folder / "check.c"
    source.write_text('#include "' + str(header) + '"\n'
                      'void check_sha(const uint8_t *d,size_t n,uint8_t *h)'
                      '{capsule_sha256(d,n,h);}\n')
    library = folder / "check.so"
    subprocess.run(["gcc", "-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror",
                    str(source), "-o", str(library)], check=True)
    function = ctypes.CDLL(str(library)).check_sha
    function.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    samples = [bytes(i % 251 for i in range(n))
               for n in (0, 1, 3, 55, 56, 63, 64, 65, 119, 120, 128, 1024)]
    samples.extend(Path(path).read_bytes() for path in sys.argv[1:])
    for sample in samples:
        actual = ctypes.create_string_buffer(32)
        function(sample, len(sample), actual)
        assert actual.raw == hashlib.sha256(sample).digest(), len(sample)
    print(f"SHA256 checks passed: {len(samples)} comparisons")
