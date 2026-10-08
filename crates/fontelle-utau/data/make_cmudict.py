#!/usr/bin/env python3
"""Builds data/cmudict.tsv.gz from CMUdict (BSD-2-Clause, notice in
CMUDICT-LICENSE): `cmudict.dict` of cmusphinx/cmudict, commit
74790861f652b15e4ac49015a90074ad62a27690.

    make_cmudict.py <cmudict.dict> | gzip -9n > cmudict.tsv.gz

Each word's first pronunciation, lowercase, stress marks removed (UTAU
banks do not record stress) except one: an unstressed AH is written `ax`,
the schwa, which English banks record apart from the stressed vowel
(X-SAMPA `@` against `V`). Comments after `#` are dropped. Output: one
`word<TAB>phones` line per word, in byte order.
"""
import re, sys

seen = {}
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.split("#", 1)[0].strip()
    if not line:
        continue
    word, *phones = line.split()
    if re.search(r"\(\d+\)$", word) or word in seen:
        continue
    seen[word] = " ".join(
        "ax" if p == "AH0" else re.sub(r"\d", "", p).lower() for p in phones
    )
out = sys.stdout.buffer
for word in sorted(seen, key=lambda w: w.encode()):
    out.write(f"{word}\t{seen[word]}\n".encode())
