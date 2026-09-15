#!/usr/bin/env python3
"""Aggregate caller chains for a leaf function in a samply Gecko profile.

Usage: samply_callers.py <profile.json> <leaf-substring> [depth] [N]

For every sample whose leaf frame name contains <leaf-substring>, walk
up to <depth> caller frames (nearest first) and count the distinct
chains. Rust v0 mangled names are trimmed to their path segments so the
output stays readable without a demangler.
"""
import re
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import json
from samply_top_n import _build_addr_map  # noqa: E402


def tidy(name):
    if name is None:
        return "?"
    m = re.match(r"_R[A-Za-z0-9]*?(\d+[A-Za-z_][A-Za-z0-9_]*)", name)
    if name.startswith("_R"):
        # Pull out len-prefixed identifiers: 4yosh6expand... -> yosh::expand::...
        parts = re.findall(r"(\d+)([A-Za-z_][A-Za-z0-9_]*)", name)
        segs = []
        for ln, rest in parts:
            ln = int(ln)
            ident = rest[:ln]
            if ident and ident not in ("Cs", "Nt", "Nv", "B", "E") and len(ident) == ln:
                segs.append(ident)
        segs = [s for s in segs if not re.fullmatch(r"[A-Za-z0-9]{12,}", s) or "_" in s]
        return "::".join(segs[:8]) if segs else name[:60]
    return name


def main():
    path, needle = sys.argv[1], sys.argv[2]
    depth = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 15
    data = json.load(open(path))
    symbolicated = data.get("meta", {}).get("symbolicated", True)
    chains = Counter()
    total = 0
    for thread in data["threads"]:
        samples = thread.get("samples")
        if not samples or not samples.get("stack"):
            continue
        sf, sp = thread["stackTable"]["frame"], thread["stackTable"]["prefix"]
        ff, fnames = thread["frameTable"]["func"], thread["funcTable"]["name"]
        strings = thread.get("stringTable") or thread["stringArray"]
        addr_map = _build_addr_map(thread, data.get("libs", []), symbolicated)

        def name_of(si):
            fi = sf[si]
            return addr_map[fi] if fi in addr_map else strings[fnames[ff[fi]]]

        for si in samples["stack"]:
            if si is None:
                continue
            leaf = name_of(si)
            if not leaf or needle not in leaf:
                continue
            total += 1
            chain, cur, k = [], sp[si], 0
            while cur is not None and k < depth:
                nm = tidy(name_of(cur))
                if not chain or chain[-1] != nm:
                    chain.append(nm)
                    k += 1
                cur = sp[cur]
            chains[" <- ".join(chain)] += 1
    print(f"# {total} samples with leaf matching {needle!r}")
    for chain, c in chains.most_common(n):
        print(f"{c:5d}  {chain}")


if __name__ == "__main__":
    main()
