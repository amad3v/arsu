#!/usr/bin/env python3
"""Lists the entries that differ between two APKs (or any zip files).

Usage: apk-diff.py A.apk B.apk

Prints each entry whose content differs, or that only one of them has,
with its size in each. The signature's own entries (META-INF/*.SF, .RSA…)
differ between differently signed APKs; everything else must match for the
build to be reproducible. Exits with 1 if anything differs.
"""

import hashlib
import sys
import zipfile


def digests(path):
    with zipfile.ZipFile(path) as apk:
        return {
            info.filename: (hashlib.sha256(apk.read(info)).hexdigest(), info.file_size)
            for info in apk.infolist()
        }


def main(a_path, b_path):
    a, b = digests(a_path), digests(b_path)
    differing = [name for name in sorted(a.keys() | b.keys()) if a.get(name) != b.get(name)]
    for name in differing:
        a_size = a[name][1] if name in a else "-"
        b_size = b[name][1] if name in b else "-"
        print(f"{name}\t{a_size}\t{b_size}")
    return 1 if differing else 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2]))
