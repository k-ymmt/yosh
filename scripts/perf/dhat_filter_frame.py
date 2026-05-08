#!/usr/bin/env python3
"""Sum dhat allocation sites whose call stack contains a substring.

Used to verify the §4.2 scratch-linker-cache fix: pass
`LinkerInstance::insert` to count blocks/bytes attributed to linker
namespace insertion across all sites (top-N alone undercounts because
the work is spread across 10+ sites of ~17 KB each).

Usage: dhat_filter_frame.py <dhat-heap.json> <substring>
"""
import json
import sys


def main():
    path = sys.argv[1]
    needle = sys.argv[2]
    data = json.load(open(path))
    pps = data["pps"]
    ftbl = data["ftbl"]

    matched_bytes = 0
    matched_blocks = 0
    matched_sites = 0
    for p in pps:
        if any(needle in ftbl[idx] for idx in p["fs"]):
            matched_bytes += p["tb"]
            matched_blocks += p["tbk"]
            matched_sites += 1

    total_bytes = sum(p["tb"] for p in pps)
    total_blocks = sum(p["tbk"] for p in pps)

    print(f"# dhat filter — `{path}`  needle=`{needle}`")
    print()
    print(f"| Metric | Value |")
    print(f"|--------|-------|")
    print(f"| Matched sites | {matched_sites:,} |")
    print(f"| Matched bytes | {matched_bytes:,} |")
    print(f"| Matched blocks | {matched_blocks:,} |")
    print(f"| Total bytes (run) | {total_bytes:,} |")
    print(f"| Total blocks (run) | {total_blocks:,} |")
    if total_bytes:
        print(f"| Matched share (bytes) | {matched_bytes/total_bytes*100:.2f}% |")
    if total_blocks:
        print(f"| Matched share (blocks) | {matched_blocks/total_blocks*100:.2f}% |")


if __name__ == "__main__":
    main()
