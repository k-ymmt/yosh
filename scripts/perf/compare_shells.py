#!/usr/bin/env python3
"""compare_shells.py — wall-clock comparison of yosh vs reference shells.

Runs every workload under benches/data/workloads/ N times per shell and
prints a Markdown table of median wall time (ms) plus the ratio against
the first reference shell.  Output of every shell is checked for
equality on the first run so a "fast but wrong" result is flagged.

Usage:
  scripts/perf/compare_shells.py [--runs N] [--shell NAME=PATH ...]
                                 [--filter SUBSTR] [--json OUT]
                                 [--env K=V ...] [--no-plugins]
Defaults: --runs 7, shells yosh=target/release/yosh dash=/bin/dash bash=/bin/bash
--no-plugins points HOME at an empty directory so an installed yosh
plugin does not skew the interpreter comparison (measured 2026-09-16:
a prompt plugin added ~32 ms to every non-interactive invocation).
"""
import argparse, json, os, resource, statistics, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WORKLOADS = os.path.join(ROOT, "benches", "data", "workloads")


def run_once(shell, script, env):
    r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
    t0 = time.perf_counter()
    p = subprocess.run([shell, script], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    wall = time.perf_counter() - t0
    r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)
    return wall, cpu, p.returncode, p.stdout, p.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=7)
    ap.add_argument("--shell", action="append", default=[])
    ap.add_argument("--filter", default="")
    ap.add_argument("--json")
    ap.add_argument("--env", action="append", default=[], metavar="K=V")
    ap.add_argument("--no-plugins", action="store_true")
    ap.add_argument("--dir", default=WORKLOADS, help="workload directory")
    a = ap.parse_args()
    env = dict(os.environ)
    for kv in a.env:
        k, v = kv.split("=", 1)
        env[k] = v
    if a.no_plugins:
        empty = tempfile.mkdtemp(prefix="yosh-noplug-")
        env["HOME"] = empty
        env["XDG_CONFIG_HOME"] = empty
    shells = [s.split("=", 1) for s in a.shell] or [
        ("yosh", os.path.join(ROOT, "target", "release", "yosh")),
        ("dash", "/bin/dash"),
        ("bash", "/bin/bash"),
    ]
    names = sorted(f for f in os.listdir(a.dir) if f.endswith(".sh") and a.filter in f)
    results = {}
    rows = []
    for wl in names:
        script = os.path.join(a.dir, wl)
        outs = {}
        row = {"workload": wl}
        for sname, spath in shells:
            walls, cpus = [], []
            for i in range(a.runs):
                wall, cpu, rc, out, err = run_once(spath, script, env)
                if i == 0:
                    outs[sname] = (rc, out)
                    if rc != 0:
                        sys.stderr.write(f"[{wl}] {sname} exit {rc}: {err.decode(errors='replace')[:200]}\n")
                walls.append(wall)
                cpus.append(cpu)
            row[sname] = {"wall_ms": statistics.median(walls) * 1000,
                          "min_ms": min(walls) * 1000,
                          "cpu_ms": statistics.median(cpus) * 1000}
        ref = outs[shells[0][0]]
        for sname, o in outs.items():
            if o != ref:
                sys.stderr.write(f"[{wl}] OUTPUT MISMATCH {shells[0][0]} vs {sname}\n")
        rows.append(row)
        results[wl] = row
    hdr = "| workload | " + " | ".join(f"{n} ms (min)" for n, _ in shells) + \
          " | " + " | ".join(f"{shells[0][0]}/{n}" for n, _ in shells[1:]) + " |"
    print(hdr)
    print("|" + "---|" * (hdr.count("|") - 1))
    for row in rows:
        cells = [f"{row[n]['wall_ms']:.1f} ({row[n]['min_ms']:.1f})" for n, _ in shells]
        base = row[shells[0][0]]["wall_ms"]
        ratios = [f"{base / row[n]['wall_ms']:.2f}x" for n, _ in shells[1:]]
        print(f"| {row['workload']} | " + " | ".join(cells) + " | " + " | ".join(ratios) + " |")
    if a.json:
        with open(a.json, "w") as f:
            json.dump(results, f, indent=1)


if __name__ == "__main__":
    main()
