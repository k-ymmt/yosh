#!/usr/bin/env python3
"""Extract Top-N allocation sites from a dhat-heap.json file.

Usage: dhat_top_n.py <path> [N]
"""
import json
import sys


def main():
    path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    data = json.load(open(path))
    pps = data["pps"]
    ftbl = data["ftbl"]

    def first_user_frame(fs):
        for idx in fs:
            name = ftbl[idx]
            # Prefer frames from this project; otherwise the leaf frame.
            if "yosh" in name or "/src/" in name or "benches/" in name:
                return name
        return ftbl[fs[0]] if fs else "(unknown)"

    def fmt_bytes(b):
        if b >= 1024 * 1024:
            return f"{b / 1024 / 1024:.2f} MB"
        if b >= 1024:
            return f"{b / 1024:.1f} KB"
        return f"{b} B"

    total_bytes = sum(p["tb"] for p in pps)
    total_blocks = sum(p["tbk"] for p in pps)

    print(f"# dhat Top-{n} — `{path}`")
    print(f"\nTotal bytes: {total_bytes:,}")
    print(f"Total blocks (calls): {total_blocks:,}\n")

    print(f"## Top {n} by bytes\n")
    print("| Rank | Site | Bytes | Calls |")
    print("|------|------|-------|-------|")
    for rank, p in enumerate(sorted(pps, key=lambda x: -x["tb"])[:n], 1):
        print(
            f"| {rank} | `{first_user_frame(p['fs'])}` "
            f"| {fmt_bytes(p['tb'])} | {p['tbk']:,} |"
        )

    print(f"\n## Top {n} by call count\n")
    print("| Rank | Site | Calls | Bytes |")
    print("|------|------|-------|-------|")
    for rank, p in enumerate(sorted(pps, key=lambda x: -x["tbk"])[:n], 1):
        print(
            f"| {rank} | `{first_user_frame(p['fs'])}` "
            f"| {p['tbk']:,} | {fmt_bytes(p['tb'])} |"
        )


if __name__ == "__main__":
    main()
