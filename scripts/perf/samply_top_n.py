#!/usr/bin/env python3
"""Extract Top-N functions from a samply Gecko profile JSON.

Usage: samply_top_n.py <profile.json> [N]

On macOS, attempts to resolve raw hex addresses to symbol names using
'atos' when the profile is not symbolicated (symbolicated=false).
Resolves symbols for all libraries that atos can handle (user binaries
and system libraries that ship with debug info).
"""
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict


def _resolve_with_atos(lib_path, addresses, base=0x100000000):
    """Resolve a list of integer addresses to symbol names via atos.

    Returns a dict mapping address -> name string.
    Falls back silently if atos fails or is unavailable.
    """
    if not addresses:
        return {}
    addr_strs = [hex(base + a) for a in addresses]
    try:
        result = subprocess.run(
            ["atos", "-arch", "arm64", "-o", lib_path, "-l", hex(base)]
            + addr_strs,
            capture_output=True,
            text=True,
            timeout=60,
        )
        lines = result.stdout.strip().splitlines()
        resolved = {}
        for addr, line in zip(addresses, lines):
            # Strip location info like "(in yosh) (file.rs:42)"
            name = line.split(" (in ")[0].strip() if " (in " in line else line.strip()
            # If it still looks like a raw hex, skip
            if re.match(r"^0x[0-9a-f]+$", name):
                continue
            # Demangle Rust: strip trailing hash like ::h1234abcd
            name = re.sub(r"::h[0-9a-f]{16}$", "", name)
            resolved[addr] = name
        return resolved
    except Exception:
        return {}


def _build_addr_map(thread, data_libs, symbolicated):
    """Return a dict mapping frame_index -> resolved symbol name."""
    if symbolicated:
        return {}

    frame_funcs = thread["frameTable"]["func"]
    frame_addresses = thread["frameTable"].get("address", [])
    func_resources = thread["funcTable"].get("resource", [])
    resource_lib = thread["resourceTable"].get("lib", [])

    # Map lib_index -> (lib_name, lib_path)
    lib_info = {i: lib for i, lib in enumerate(data_libs)}

    # Group frames by lib_index for batch atos calls
    lib_frames: dict = defaultdict(list)  # lib_idx -> [(f_idx, raw_addr)]
    for f_idx, func_idx in enumerate(frame_funcs):
        if func_idx >= len(func_resources):
            continue
        res = func_resources[func_idx]
        if res is None or res < 0 or res >= len(resource_lib):
            continue
        lib_idx = resource_lib[res]
        if lib_idx is None or lib_idx < 0:
            continue
        raw_addr = frame_addresses[f_idx] if f_idx < len(frame_addresses) else None
        if raw_addr is not None and raw_addr >= 0:
            lib_frames[lib_idx].append((f_idx, raw_addr))

    addr_map: dict = {}
    for lib_idx, frame_list in lib_frames.items():
        lib = lib_info.get(lib_idx)
        if not lib:
            continue
        lib_path = lib.get("debugPath") or lib.get("path", "")
        if not lib_path:
            continue
        unique_addrs = list({addr for _, addr in frame_list})
        resolved = _resolve_with_atos(lib_path, unique_addrs)
        for f_idx, raw_addr in frame_list:
            if raw_addr in resolved:
                addr_map[f_idx] = resolved[raw_addr]

    return addr_map


def main():
    path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    data = json.load(open(path))

    symbolicated = data.get("meta", {}).get("symbolicated", True)
    data_libs = data.get("libs", [])

    self_counter: Counter = Counter()
    total_counter: Counter = Counter()

    for thread in data["threads"]:
        samples = thread.get("samples")
        if not samples or not samples.get("stack"):
            continue
        stack_frames = thread["stackTable"]["frame"]
        stack_prefix = thread["stackTable"]["prefix"]
        frame_funcs = thread["frameTable"]["func"]
        func_names = thread["funcTable"]["name"]
        strings = thread.get("stringTable") or thread["stringArray"]

        addr_map = _build_addr_map(thread, data_libs, symbolicated)

        def name_of(stack_idx):
            if stack_idx is None:
                return None
            f_idx = stack_frames[stack_idx]
            if f_idx in addr_map:
                return addr_map[f_idx]
            fn = frame_funcs[f_idx]
            return strings[func_names[fn]]

        for s_idx in samples["stack"]:
            if s_idx is None:
                continue
            top = name_of(s_idx)
            if top:
                self_counter[top] += 1
            seen = set()
            cur = s_idx
            while cur is not None:
                nm = name_of(cur)
                if nm and nm not in seen:
                    seen.add(nm)
                    total_counter[nm] += 1
                cur = stack_prefix[cur]

    total = sum(self_counter.values())
    print(f"# samply Top-{n} — `{path}`")
    print(f"\nTotal samples: {total}\n")

    print(f"## Self time Top-{n}\n")
    print("| Rank | Function | Self % | Count |")
    print("|------|----------|--------|-------|")
    for rank, (nm, cnt) in enumerate(self_counter.most_common(n), 1):
        pct = 100.0 * cnt / total if total else 0
        print(f"| {rank} | `{nm}` | {pct:.1f}% | {cnt} |")

    print(f"\n## Total time Top-{n}\n")
    print("| Rank | Function | Total % | Count |")
    print("|------|----------|---------|-------|")
    for rank, (nm, cnt) in enumerate(total_counter.most_common(n), 1):
        pct = 100.0 * cnt / total if total else 0
        print(f"| {rank} | `{nm}` | {pct:.1f}% | {cnt} |")


if __name__ == "__main__":
    main()
