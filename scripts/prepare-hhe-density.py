#!/usr/bin/env python3
"""Compile locally held, SHA-pinned EOS sources. No download or redistribution.

The output is an optional local runtime resource, not a committed dataset.
"""
import argparse
import hashlib
import importlib.util
from pathlib import Path
import struct

spec = importlib.util.spec_from_file_location(
    "probe", Path(__file__).with_name("probe-envelope-prescription.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def compile_table(directory):
    source = probe.DensityReference(directory)
    out = bytearray(b"NSHHE001" + struct.pack("<II", 61, 261))
    for i in range(61):
        for j in range(261):
            h, he = (source.grids[name][i, j] for name in ("H", "He"))
            flags = int(h[6] > 0) | (int(he[6] > 0) << 1)
            out.extend(struct.pack("<dddB", h[2], he[2], source.correction[i, j][0], flags))
    return bytes(out)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    data = compile_table(args.directory)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    print(f"{len(data)} bytes; SHA-256 {hashlib.sha256(data).hexdigest()}")
    # Stable non-cryptographic runtime corruption check. Source authenticity is
    # independently checked with SHA-256 before conversion, above.
    checksum = 0xcbf29ce484222325
    for byte in data:
        checksum = ((checksum ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    print(f"Runtime FNV-1a checksum: 0x{checksum:016x}")
