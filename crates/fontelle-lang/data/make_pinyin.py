#!/usr/bin/env python3
"""Builds data/pinyin.txt from mozillazg/pinyin-data (MIT; its readings are
Unicode's Unihan kMandarin, Unicode-3.0). Run from anywhere:

    make_pinyin.py <pinyin-data checkout or folder with kMandarin.txt,
                    kMandarin_overwrite.txt, kMandarin_8105.txt> > pinyin.txt

Each character's most common reading: kMandarin, corrected by
kMandarin_overwrite, then kMandarin_8105 (the 2013 Table of General
Standard Chinese Characters' most common reading) for the 8105 everyday
characters. Tones are dropped (what singing voicebanks are keyed by), and
ü is written v after l and n (lv, nve), u elsewhere (ju, xue), as the
Opencpop-style dictionaries DiffSinger banks use do. Output: one line per
reading, `reading<TAB>characters`, sorted.
"""
import sys, unicodedata, collections, os

def read(path):
    out = {}
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line or ":" not in line:
            continue
        cp, readings = line.split(":", 1)
        first = readings.replace(",", " ").split()
        if not first:
            continue
        out[int(cp.strip()[2:], 16)] = first[0]
    return out

def toneless(r):
    r = r.replace("ü", "v").replace("ǖ", "v").replace("ǘ", "v").replace("ǚ", "v").replace("ǜ", "v")
    r = "".join(c for c in unicodedata.normalize("NFD", r) if not unicodedata.combining(c))
    r = r.lower()
    if r.startswith(("jv", "qv", "xv", "yv")):
        r = r[0] + "u" + r[2:]
    return r

src = sys.argv[1]
table = read(os.path.join(src, "kMandarin.txt"))
for name in ("kMandarin_overwrite.txt", "kMandarin_8105.txt"):
    table.update(read(os.path.join(src, name)))
groups = collections.defaultdict(list)
for cp, r in sorted(table.items()):
    r = toneless(r)
    if not r.isascii() or not r.isalpha():
        continue
    groups[r].append(chr(cp))
for r in sorted(groups):
    print(f"{r}\t{''.join(groups[r])}")
