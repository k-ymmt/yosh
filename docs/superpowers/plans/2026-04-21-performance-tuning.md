# Performance Tuning Measurement Report — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce `performance.md` at the repository root — a consolidated measurement report identifying yosh's largest CPU hotspots and allocation sites across startup / script / interactive workloads.

**Architecture:** Three tools (samply for flame graphs, dhat-rs for allocation tracking, Criterion for micro-benchmarks) applied to three workloads (W1 startup, W2 script-heavy, W3 interactive-smoke). All binaries are built under a new `[profile.profiling]` (release + debug symbols). No production code is modified; only new build artifacts (profiling binary, benches, workload scripts) plus the report itself.

**Tech Stack:** Rust 2024, Cargo, Criterion 0.5, dhat 0.3, samply (external CLI), expectrl 0.8 (existing dev-dep for W3).

**Spec:** `docs/superpowers/specs/2026-04-21-performance-tuning-design.md`

---

## File Structure

**New files:**
- `benches/data/script_heavy.sh` — W2 workload (loops, functions, expansion, redirection)
- `benches/data/startup_loop.sh` — W1 N=1000 loop wrapper
- `src/bin/yosh-dhat.rs` — feature-gated dhat-instrumented binary running W2 in-process
- `benches/startup_bench.rs` — Criterion: external-process `yosh -c 'echo hi'` timing
- `benches/exec_bench.rs` — Criterion: in-process loop / function-call / expansion micro-benches
- `benches/interactive_smoke.rs` — `harness=false` bench entry driving W3 via expectrl (runnable under samply)
- `performance.md` — the report (repo root)

**Modified files:**
- `Cargo.toml` — `[profile.profiling]`, `dhat` dev-dep, `dhat-heap` feature, new `[[bench]]` entries

**Responsibilities:** `benches/data/*.sh` are passive workload definitions. `src/bin/yosh-dhat.rs` and `benches/*.rs` are measurement programs. `performance.md` is the authored report. All measurement outputs (`.perf.json`, `dhat-heap.json`, `target/criterion/`) are transient — transcribed into the report, not committed.

---

## Task 1: Add `[profile.profiling]`, dhat dev-dep, and `dhat-heap` feature

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Open `Cargo.toml`**

Current state (relevant excerpts):

```toml
[dependencies]
...
yosh-plugin-api = { version = "0.1.2", path = "crates/yosh-plugin-api" }
unicode-width = "0.2"
owo-colors = "4"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"
crossterm = "0.29"
expectrl = "0.8"
```

- [ ] **Step 2: Add `dhat` dev-dependency and `dhat-heap` feature**

After the `[dev-dependencies]` block, append:

```toml
[features]
# Enables dhat-rs heap profiling in the `yosh-dhat` binary.
# Build with: cargo build --features dhat-heap --bin yosh-dhat --profile profiling
dhat-heap = ["dep:dhat"]
```

And in `[dev-dependencies]`, add `dhat` as an optional-via-feature entry. Because dhat needs to be referenced by the `yosh-dhat` binary in `src/bin/`, put it in regular `[dependencies]` as an optional dep:

```toml
[dependencies]
...
owo-colors = "4"
dhat = { version = "0.3", optional = true }
```

Rationale: `src/bin/` binaries cannot use `dev-dependencies`. `dhat` is compiled only when the `dhat-heap` feature is enabled, so production builds are unaffected.

- [ ] **Step 3: Add `[profile.profiling]`**

At the end of `Cargo.toml`, append:

```toml
[profile.profiling]
inherits = "release"
debug = true
strip = false
```

- [ ] **Step 4: Register the upcoming benches**

In the `[[bench]]` section list, append three new entries after the existing three:

```toml
[[bench]]
name = "startup_bench"
harness = false

[[bench]]
name = "exec_bench"
harness = false

[[bench]]
name = "interactive_smoke"
harness = false
```

- [ ] **Step 5: Verify the base build still compiles**

Run: `cargo build --profile profiling`
Expected: compiles cleanly; produces `target/profiling/yosh`.

- [ ] **Step 6: Verify the feature flag compiles**

Run: `cargo build --profile profiling --features dhat-heap --lib`
Expected: compiles cleanly (binary not yet present, so `--lib` scopes the check to the library).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(bench): add profiling profile, dhat dep, and bench entries

- [profile.profiling] for release + debug symbols
- dhat 0.3 as optional dep gated behind dhat-heap feature
- register startup_bench / exec_bench / interactive_smoke

Design: docs/superpowers/specs/2026-04-21-performance-tuning-design.md"
```

---

## Task 2: Author the W2 workload script (`benches/data/script_heavy.sh`)

**Files:**
- Create: `benches/data/script_heavy.sh`

- [ ] **Step 1: Create the file**

```sh
#!/bin/sh
# script_heavy.sh — W2 workload for performance measurement.
# Exercises Lexer, Parser, Expander, and Executor hot paths.

# ── Section A: for-loop with arithmetic ─────────────────────────────────
SUM=0
for i in $(seq 1 1000); do
    SUM=$((SUM + i))
done
echo "sum=$SUM"

# ── Section B: function defined once, called 1000 times ─────────────────
greet() {
    local name=$1
    echo "hello, $name"
}

i=0
while [ "$i" -lt 1000 ]; do
    greet "world" > /dev/null
    i=$((i + 1))
done

# ── Section C: parameter expansion variety ──────────────────────────────
VAR="hello world"
UNSET=""
for _ in $(seq 1 200); do
    : "${UNSET:-fallback}"
    : "${VAR#hello }"
    : "${VAR%world}"
    : "${#VAR}"
    : "$(echo "$VAR")"
done

# ── Section D: redirection ──────────────────────────────────────────────
TMP=$(mktemp)
echo "line one" > "$TMP"
echo "line two" >> "$TMP"
echo "line three to stderr" 1>&2 2>/dev/null

cat <<HEREDOC > "$TMP"
heredoc body
more heredoc body
HEREDOC

rm -f "$TMP"
```

- [ ] **Step 2: Set permissions (644, not executable — per CLAUDE.md E2E convention)**

Run: `chmod 644 benches/data/script_heavy.sh`
Expected: no output.

Note: this file is passed as an argument to `yosh`, so it does not need execute bit.

- [ ] **Step 3: Run it under yosh to verify it executes cleanly**

Run: `./target/profiling/yosh benches/data/script_heavy.sh`
Expected: prints `sum=500500` and exits 0.

Run: `./target/profiling/yosh benches/data/script_heavy.sh; echo "exit=$?"`
Expected: last line is `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add benches/data/script_heavy.sh
git commit -m "test(bench): add W2 script_heavy workload for perf measurement

Covers for-loop, function-call, parameter expansion, and redirection
hot paths used by samply / dhat / Criterion in the perf report."
```

---

## Task 3: Author the W1 startup loop wrapper (`benches/data/startup_loop.sh`)

**Files:**
- Create: `benches/data/startup_loop.sh`

- [ ] **Step 1: Create the file**

```sh
#!/bin/sh
# startup_loop.sh — W1 loop wrapper for samply.
# Invokes yosh N times so that samply has enough samples to resolve
# short-lived startup costs.
#
# Usage: startup_loop.sh <yosh-binary> [N]

set -eu

YOSH=${1:?"missing yosh binary path"}
N=${2:-1000}

i=0
while [ "$i" -lt "$N" ]; do
    "$YOSH" -c 'echo hi' > /dev/null
    i=$((i + 1))
done
```

- [ ] **Step 2: Set permissions (755 — this one IS executed directly by samply, not by yosh)**

Run: `chmod 755 benches/data/startup_loop.sh`
Expected: no output.

- [ ] **Step 3: Run a short smoke test**

Run: `./benches/data/startup_loop.sh ./target/profiling/yosh 10 && echo ok`
Expected: prints `ok` (10 invocations succeed).

- [ ] **Step 4: Commit**

```bash
git add benches/data/startup_loop.sh
git commit -m "test(bench): add W1 startup loop wrapper for samply

Invokes yosh N=1000 times so samply has enough samples to profile
short-lived startup cost."
```

---

## Task 4: Implement `src/bin/yosh-dhat.rs` (dhat-instrumented W2 runner)

**Files:**
- Create: `src/bin/yosh-dhat.rs`

- [ ] **Step 1: Create the file**

```rust
//! yosh-dhat — dhat-instrumented binary that runs a yosh script in-process
//! with a custom global allocator for heap profiling.
//!
//! Build and run:
//!   cargo build --profile profiling --features dhat-heap --bin yosh-dhat
//!   ./target/profiling/yosh-dhat benches/data/script_heavy.sh
//!
//! Output: `dhat-heap.json` in CWD — open with https://nnethercote.github.io/dh_view/dh_view.html

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use std::process;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <script-path>", args[0]);
        process::exit(2);
    }
    let script_path = &args[1];

    let input = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yosh-dhat: {}: {}", script_path, e);
            process::exit(127);
        }
    };

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    let program = match yosh::parser::Parser::new(&input).parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("yosh-dhat: parse error: {}", e);
            process::exit(2);
        }
    };

    let status = executor.exec_program(&program);
    process::exit(status);
}
```

- [ ] **Step 2: Verify the library exposes the symbols used above**

Run these four greps; each must return at least one match:

```bash
grep -n "pub fn init_signal_handling" src/signal.rs
grep -n "pub fn new" src/exec/mod.rs | head -1
grep -n "pub fn load_plugins" src/exec/mod.rs
grep -n "pub fn ensure_default_path" src/env/default_path.rs
grep -n "pub fn exec_program" src/exec/mod.rs
grep -n "pub fn parse_program" src/parser/mod.rs
```

Expected: all six produce a matching line. If any is missing, STOP and surface the gap — the binary will not compile.

- [ ] **Step 3: Build with the feature enabled**

Run: `cargo build --profile profiling --features dhat-heap --bin yosh-dhat`
Expected: compiles; produces `target/profiling/yosh-dhat`.

- [ ] **Step 4: Build without the feature (ensure production path unaffected)**

Run: `cargo build --profile profiling --bin yosh-dhat`
Expected: compiles (dhat code is cfg-gated out; binary runs without profiling).

- [ ] **Step 5: Run the W2 script under dhat**

Run: `./target/profiling/yosh-dhat benches/data/script_heavy.sh`

Wait, this binary requires the `dhat-heap` feature to actually profile. The prior command in Step 4 built without the feature. Re-run with the feature active by using `cargo run`:

Run: `cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- benches/data/script_heavy.sh`
Expected: stdout contains `sum=500500`; exits 0; `dhat-heap.json` appears in CWD.

- [ ] **Step 6: Verify the dhat output is valid JSON**

Run: `python3 -c "import json; json.load(open('dhat-heap.json')); print('ok')"`
Expected: prints `ok`.

- [ ] **Step 7: Move the output out of the repo root**

Run: `mkdir -p target/perf && mv dhat-heap.json target/perf/dhat-heap-w2.json`
Expected: no output; `target/perf/dhat-heap-w2.json` exists.

(Note: `target/` is gitignored; artifacts are transient. The report will transcribe the Top-N findings.)

- [ ] **Step 8: Commit**

```bash
git add src/bin/yosh-dhat.rs
git commit -m "feat(bench): add yosh-dhat dhat-instrumented binary

Feature-gated (dhat-heap) binary that runs a yosh script in-process
under a dhat global allocator, emitting dhat-heap.json for analysis."
```

---

## Task 5: Implement `benches/startup_bench.rs` (Criterion W1)

**Files:**
- Create: `benches/startup_bench.rs`

- [ ] **Step 1: Create the file**

```rust
//! startup_bench — measures the wall-clock cost of a one-shot yosh invocation.
//!
//! Because startup cost involves the full OS process lifecycle (fork/exec,
//! libc init, dynamic linker, our own init), we invoke yosh as an external
//! subprocess per iteration. This is slow but accurate.

use std::process::{Command, Stdio};

use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn yosh_binary() -> String {
    // Tests and benches that need the compiled binary can use the
    // CARGO_BIN_EXE_<name> env var that Cargo sets for bench targets.
    // When that is unavailable (e.g., running the binary under samply
    // later), fall back to the profiling profile path.
    option_env!("CARGO_BIN_EXE_yosh")
        .map(String::from)
        .unwrap_or_else(|| "./target/profiling/yosh".to_string())
}

fn bench_startup_echo(c: &mut Criterion) {
    let yosh = yosh_binary();
    c.bench_function("startup_echo_hi", |b| {
        b.iter(|| {
            let status = Command::new(black_box(&yosh))
                .args(["-c", "echo hi"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("failed to spawn yosh");
            assert!(status.success(), "yosh -c 'echo hi' failed");
        });
    });
}

criterion_group!(benches, bench_startup_echo);
criterion_main!(benches);
```

- [ ] **Step 2: Build (compile-only check)**

Run: `cargo build --profile profiling --bench startup_bench`
Expected: compiles cleanly.

- [ ] **Step 3: Run a short sample**

Run: `cargo bench --bench startup_bench -- --sample-size 10 --warm-up-time 1 --measurement-time 2`
Expected: completes within ~30 s; prints a median time per iteration; no panics.

(Rationale: the default Criterion sample size of 100 with subprocess spawn is slow. The short run is just a smoke check — full measurement happens in Task 10.)

- [ ] **Step 4: Commit**

```bash
git add benches/startup_bench.rs
git commit -m "test(bench): add startup_bench Criterion micro-bench

Measures wall-clock of 'yosh -c echo hi' per iteration via
subprocess invocation. Used as W1 baseline in the perf report."
```

---

## Task 6: Implement `benches/exec_bench.rs` (Criterion W2 sub-benches)

**Files:**
- Create: `benches/exec_bench.rs`

- [ ] **Step 1: Create the file**

```rust
//! exec_bench — in-process micro-benchmarks for W2 pipeline components.
//! Unlike startup_bench (subprocess), these run the shell pipeline through
//! the library API so that parse + expand + exec costs are isolated.

use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn run_script(src: &str) -> i32 {
    let mut executor = yosh::exec::Executor::new("exec_bench", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    let program = yosh::parser::Parser::new(src)
        .parse_program()
        .expect("parse failed");
    executor.exec_program(&program)
}

const LOOP_SCRIPT: &str = r#"
sum=0
for i in $(seq 1 200); do
    sum=$((sum + i))
done
"#;

const FUNCTION_SCRIPT: &str = r#"
f() { : "$1"; }
i=0
while [ "$i" -lt 200 ]; do
    f arg
    i=$((i + 1))
done
"#;

const EXPANSION_SCRIPT: &str = r#"
VAR="hello world"
UNSET=""
for _ in $(seq 1 200); do
    : "${UNSET:-fallback}"
    : "${VAR#hello }"
    : "${VAR%world}"
    : "${#VAR}"
done
"#;

fn bench_exec(c: &mut Criterion) {
    c.bench_function("exec_for_loop_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(LOOP_SCRIPT));
            assert_eq!(status, 0);
        });
    });

    c.bench_function("exec_function_call_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(FUNCTION_SCRIPT));
            assert_eq!(status, 0);
        });
    });

    c.bench_function("exec_param_expansion_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(EXPANSION_SCRIPT));
            assert_eq!(status, 0);
        });
    });
}

criterion_group!(benches, bench_exec);
criterion_main!(benches);
```

- [ ] **Step 2: Compile-only check**

Run: `cargo build --profile profiling --bench exec_bench`
Expected: compiles cleanly.

- [ ] **Step 3: Run a short sample**

Run: `cargo bench --bench exec_bench -- --sample-size 10 --warm-up-time 1 --measurement-time 2`
Expected: prints median times for `exec_for_loop_200`, `exec_function_call_200`, `exec_param_expansion_200`; all assertions pass.

- [ ] **Step 4: Commit**

```bash
git add benches/exec_bench.rs
git commit -m "test(bench): add exec_bench Criterion sub-benches

In-process micro-benches for the for-loop, function-call, and
parameter-expansion paths of W2, isolating each from subprocess
startup overhead."
```

---

## Task 7: Implement `benches/interactive_smoke.rs` (W3 PTY harness)

**Files:**
- Create: `benches/interactive_smoke.rs`

- [ ] **Step 1: Create the file**

```rust
//! interactive_smoke — runnable binary that drives yosh through a short
//! interactive scenario via expectrl. Not a Criterion bench; declared as
//! `harness = false` so that `cargo bench --bench interactive_smoke`
//! produces a plain binary that samply can profile directly.
//!
//! Scenario:
//!   1. spawn yosh on a PTY
//!   2. wait for the prompt
//!   3. send "echo hello\n", expect "hello" back
//!   4. send Tab (one completion attempt)
//!   5. send Up arrow (history recall)
//!   6. send "exit\n", expect EOF

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use expectrl::{Eof, Expect, Regex, Session};
// `Expect` brings the `expect` / `send` / `send_line` methods into scope —
// same import pattern used by tests/pty_interactive.rs.

const PROMPT_TIMEOUT: Duration = Duration::from_secs(10);

fn main() {
    let yosh_bin: PathBuf = option_env!("CARGO_BIN_EXE_yosh")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./target/profiling/yosh"));

    let tmpdir = tempfile::tempdir().expect("tempdir");

    let mut cmd = Command::new(&yosh_bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("spawn yosh");
    session.set_expect_timeout(Some(PROMPT_TIMEOUT));

    // 1. Prompt
    session.expect("$ ").expect("initial prompt");

    // 2. echo hello
    session.send_line("echo hello").expect("send echo");
    session.expect(Regex("hello")).expect("echo output");
    session.expect("$ ").expect("prompt after echo");

    // 3. Tab completion (send Tab, give yosh ~200ms, then clear the line)
    session.send("\t").expect("send tab");
    std::thread::sleep(Duration::from_millis(200));
    // Ctrl-U clears the line regardless of what tab inserted.
    session.send("\x15").expect("send ctrl-u");

    // 4. History recall: Up arrow recalls "echo hello", then Ctrl-U clears.
    session.send("\x1b[A").expect("send up arrow");
    std::thread::sleep(Duration::from_millis(200));
    session.send("\x15").expect("send ctrl-u");

    // 5. Exit
    session.send_line("exit").expect("send exit");
    session.expect(Eof).expect("eof after exit");
}
```

- [ ] **Step 2: Verify `tempfile` is accessible from a bench**

Run: `grep -n tempfile Cargo.toml`
Expected: `tempfile = "3"` appears under `[dev-dependencies]`. Benches share dev-dependencies with tests, so this compiles.

- [ ] **Step 3: Compile-only check**

Run: `cargo build --profile profiling --bench interactive_smoke`
Expected: compiles cleanly.

- [ ] **Step 4: Run the bench**

Run: `cargo bench --bench interactive_smoke`
Expected: binary exits 0 within ~5 seconds. No panic messages.

If PTY timing is unstable (panic on expect), re-run up to 3 times. If it fails all 3, proceed to Step 6 (skip marker) — W3 is explicitly marked as optional in the spec.

- [ ] **Step 5: On success, commit**

```bash
git add benches/interactive_smoke.rs
git commit -m "test(bench): add interactive_smoke W3 PTY harness

harness=false bench binary driving yosh through a short
prompt/echo/tab/history/exit scenario via expectrl. Runnable
under samply for W3 measurement."
```

- [ ] **Step 6: On 3-time failure, record skip and commit**

Create `benches/interactive_smoke_skip.md` with content:

```markdown
# W3 interactive-smoke skipped

Three consecutive runs of `cargo bench --bench interactive_smoke` failed
with PTY timing issues. W3 is marked as best-effort in the design spec
(docs/superpowers/specs/2026-04-21-performance-tuning-design.md §3).

W3 findings in performance.md will reference this skip marker.
```

Then:

```bash
git add benches/interactive_smoke.rs benches/interactive_smoke_skip.md
git commit -m "test(bench): add W3 harness, mark as skipped per spec §3

Three PTY runs diverged; recording skip marker for the perf report."
```

---

## Task 8: Run samply × 3 workloads and capture Top-N

**Files (artifacts, transient — `target/perf/` is gitignored):**
- Create: `target/perf/samply_w1.json`, `target/perf/samply_w2.json`, `target/perf/samply_w3.json` (if W3 not skipped)
- Create: `target/perf/samply_top.md` — text summary

- [ ] **Step 1: Verify samply is installed**

Run: `samply --version`
Expected: prints a version number. If not installed, run `cargo install samply` and re-run.

- [ ] **Step 2: Ensure profiling binaries are fresh**

Run: `cargo build --profile profiling --bin yosh --bench interactive_smoke`
Expected: compiles; `target/profiling/yosh` exists.

- [ ] **Step 3: Profile W1 (startup loop)**

Run:
```bash
mkdir -p target/perf
samply record --save-only --output target/perf/samply_w1.json -- \
    ./benches/data/startup_loop.sh ./target/profiling/yosh 1000
```

Expected: exits 0; `target/perf/samply_w1.json` exists and is > 10 KB.

- [ ] **Step 4: Profile W2 (script_heavy)**

Run:
```bash
samply record --save-only --output target/perf/samply_w2.json -- \
    ./target/profiling/yosh benches/data/script_heavy.sh
```

Expected: exits 0 (stdout contains `sum=500500`); `target/perf/samply_w2.json` exists.

- [ ] **Step 5: Profile W3 (interactive_smoke)**

If W3 was skipped in Task 7 Step 6, skip this step.

Locate the bench binary:
```bash
ls target/profiling/deps/interactive_smoke-*
```
Pick the most recent one without a `.d` extension.

Run:
```bash
samply record --save-only --output target/perf/samply_w3.json -- \
    ./target/profiling/deps/interactive_smoke-<hash>
```

Expected: exits 0; `target/perf/samply_w3.json` exists.

- [ ] **Step 6: Create `scripts/perf/samply_top_n.py` for Top-N extraction**

Samply's JSON is in Gecko profile format. Write a Python parser that extracts the Top-N self-time and total-time functions:

```python
#!/usr/bin/env python3
"""Extract Top-N functions from a samply Gecko profile JSON.

Usage: samply_top_n.py <profile.json> [N]
"""
import json
import sys
from collections import Counter


def main():
    path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    data = json.load(open(path))

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
        strings = thread["stringTable"]

        def name_of(stack_idx):
            if stack_idx is None:
                return None
            fn = frame_funcs[stack_frames[stack_idx]]
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
```

Place the file at `scripts/perf/samply_top_n.py`, then:

```bash
mkdir -p scripts/perf
chmod 755 scripts/perf/samply_top_n.py
```

- [ ] **Step 7: Run the extractor against each profile**

```bash
{
    echo "# samply Top-N summary"
    echo
    echo "Measurement date: $(date -u '+%Y-%m-%d')"
    echo "Commit: $(git rev-parse --short HEAD)"
    echo "Host: $(uname -srm)"
    echo
    echo "## W1 startup_loop (N=1000)"
    python3 scripts/perf/samply_top_n.py target/perf/samply_w1.json 10 \
        | tail -n +2
    echo
    echo "## W2 script_heavy"
    python3 scripts/perf/samply_top_n.py target/perf/samply_w2.json 10 \
        | tail -n +2
    if [ -f target/perf/samply_w3.json ]; then
        echo
        echo "## W3 interactive_smoke"
        python3 scripts/perf/samply_top_n.py target/perf/samply_w3.json 10 \
            | tail -n +2
    else
        echo
        echo "## W3 interactive_smoke"
        echo
        echo "_Skipped — see \`benches/interactive_smoke_skip.md\`._"
    fi
} > target/perf/samply_top.md
```

Expected: `target/perf/samply_top.md` exists with self-time and total-time tables for W1, W2, and either W3 or a skip marker.

- [ ] **Step 8: Commit the extractor script**

The JSON profiles themselves are transient, but the extractor is reusable and belongs in the repo.

```bash
git add scripts/perf/samply_top_n.py
git commit -m "chore(perf): add samply Top-N extractor script

Parses Gecko profile JSON emitted by 'samply record --save-only'
and prints self-time / total-time Top-N tables in Markdown."
```

---

## Task 9: Run dhat × W2 and capture Top-N allocation sites

**Files (artifacts, transient):**
- Create: `target/perf/dhat-heap-w2.json` (already produced in Task 4 Step 7; re-run if stale)
- Create: `target/perf/dhat_top.md`

- [ ] **Step 1: Re-run W2 under dhat to ensure a fresh profile**

Run:
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2.json
```

Expected: stdout contains `sum=500500`; `target/perf/dhat-heap-w2.json` is > 1 KB.

- [ ] **Step 2: Create `scripts/perf/dhat_top_n.py` for Top-N extraction**

```python
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
```

Place at `scripts/perf/dhat_top_n.py`, then:

```bash
chmod 755 scripts/perf/dhat_top_n.py
```

- [ ] **Step 3: Run the extractor and capture Top-N**

```bash
{
    echo "# dhat Top-N allocation sites (W2)"
    echo
    echo "Measurement date: $(date -u '+%Y-%m-%d')"
    echo "Commit: $(git rev-parse --short HEAD)"
    echo
    python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10 \
        | tail -n +2
    echo
    echo "## Notes"
    echo
    echo "- TODO.md lists 'LINENO update allocates a String per command'"
    echo "  (src/exec/simple.rs, src/exec/compound.rs). Cross-check whether"
    echo "  this appears in the Top-N; confirmed or surprising absence is"
    echo "  recorded in performance.md §4."
} > target/perf/dhat_top.md
```

Expected: `target/perf/dhat_top.md` exists with both tables populated from real data.

- [ ] **Step 4: Commit the extractor script**

```bash
git add scripts/perf/dhat_top_n.py
git commit -m "chore(perf): add dhat Top-N extractor script

Parses dhat-heap.json (dhat-rs output) and prints Top-N sites by
total bytes and by call count in Markdown."
```

---

## Task 10: Run Criterion full suite and capture summaries

**Files (artifacts, transient):**
- Create: `target/perf/criterion_summary.md`

- [ ] **Step 1: Run the full bench suite**

Run: `cargo bench`
Expected: all six benches (lexer_bench, parser_bench, expand_bench, startup_bench, exec_bench, interactive_smoke) complete. `target/criterion/<name>/report/index.html` exists for each.

Note: this may take 10-30 minutes total due to startup_bench's subprocess iterations. Run in the background if needed.

- [ ] **Step 2: Extract the summary lines**

For each bench function, the last line of Criterion's per-function stdout output has the form:

```
<name>                  time:   [<min> <median> <max>]
```

Capture these into `target/perf/criterion_summary.md`:

```markdown
# Criterion summary

Measurement date: <YYYY-MM-DD>
Commit: <git rev-parse HEAD>
Profile: profiling (release + debug symbols)

## Existing benches (baseline)
| Bench | min | median | max |
|-------|-----|--------|-----|
| lex_small | ... | ... | ... |
| lex_large | ... | ... | ... |
| parse_small | ... | ... | ... |
| parse_large | ... | ... | ... |
| expand_param_default | ... | ... | ... |
| expand_field_split | ... | ... | ... |
| expand_literal_words | ... | ... | ... |

## New benches
| Bench | min | median | max |
|-------|-----|--------|-----|
| startup_echo_hi | ... | ... | ... |
| exec_for_loop_200 | ... | ... | ... |
| exec_function_call_200 | ... | ... | ... |
| exec_param_expansion_200 | ... | ... | ... |
```

Tip: the raw numbers live in `target/criterion/<bench>/<function>/new/estimates.json` as JSON if the stdout is lost.

Expected: `target/perf/criterion_summary.md` has populated rows for every bench.

- [ ] **Step 3: No commit**

Same rationale as prior tasks.

---

## Task 11: Author `performance.md` — sections 1 through 3

**Files:**
- Create: `performance.md` (repo root)

- [ ] **Step 1: Gather environment metadata**

Run these and capture their output into a scratch note:

```bash
sw_vers -productVersion    # macOS version (or `uname -a` on Linux)
uname -m                   # CPU arch
sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown CPU"
rustc --version
git rev-parse --short HEAD
date -u '+%Y-%m-%d'
```

Expected: six lines of output. Record them — they go into §1 Executive Summary.

- [ ] **Step 2: Draft `performance.md` with §1–§3**

Create `performance.md` with exactly this skeleton (fill in the real values from Steps 1 and from `target/perf/*_top.md`, `target/perf/criterion_summary.md`):

```markdown
# yosh Performance Report

**Measurement date:** <YYYY-MM-DD>
**Commit:** <short SHA>
**Environment:** <OS version> / <CPU arch> / <CPU brand> / rustc <version>
**Build profile:** `profiling` (`release` + `debug = true`, `strip = false`)

## 1. Executive Summary

_[Populated in Task 12 Step 3 after findings are written.]_

## 2. Methodology

### 2.1 Workloads

- **W1 — Startup:** `yosh -c 'echo hi'` invoked N=1000 times via
  `benches/data/startup_loop.sh`; supplemented by single-shot for samply.
- **W2 — Script-heavy:** `benches/data/script_heavy.sh` (1000-iter for-loop,
  1000-call function, parameter-expansion variety, redirection, heredoc).
- **W3 — Interactive-smoke:** `benches/interactive_smoke.rs` — expectrl
  scenario: prompt → `echo hello` → Tab → Up arrow → `exit`.
  <!-- If skipped, replace with: "W3 was skipped; see
       benches/interactive_smoke_skip.md for the reason." -->

### 2.2 Tools

- **samply** v<X.Y.Z> — whole-process flame graphs. `samply record --save-only`.
- **dhat-rs** v0.3 — heap allocation tracking via `src/bin/yosh-dhat.rs`
  (feature-gated behind `dhat-heap`).
- **Criterion** v0.5 — in-process micro-benchmarks.

### 2.3 Build profile

```toml
[profile.profiling]
inherits = "release"
debug = true
strip = false
```

All samply / dhat / Criterion runs use `--profile profiling` artifacts.
The `release` profile omits debug symbols and may differ slightly — this
is flagged in each finding where relevant.

## 3. Results

### 3.1 W1: Startup

**Wall-clock (Criterion `startup_echo_hi`):**
| Metric | Value |
|--------|-------|
| Min    | <ms>  |
| Median | <ms>  |
| Max    | <ms>  |

**samply Top-10 self time (W1):**

<copy the W1 Self-time table from target/perf/samply_top.md>

**samply Top-5 total time (W1):**

<copy the W1 Total-time table from target/perf/samply_top.md>

### 3.2 W2: Script-heavy

**Criterion micro-benches:**
| Bench | Median |
|-------|--------|
| exec_for_loop_200 | <µs/ms> |
| exec_function_call_200 | <µs/ms> |
| exec_param_expansion_200 | <µs/ms> |
| lex_large (baseline) | <µs> |
| parse_large (baseline) | <µs> |
| expand_* (baseline) | <µs> |

**samply Top-10 self time (W2):**

<copy from target/perf/samply_top.md>

**dhat Top-10 by bytes (W2):**

<copy from target/perf/dhat_top.md>

**dhat Top-10 by call count (W2):**

<copy from target/perf/dhat_top.md>

### 3.3 W3: Interactive-smoke

<If not skipped:>
**samply Top-10 self time (W3):**

<copy from target/perf/samply_top.md>

<If skipped:>
W3 was skipped; the `benches/interactive_smoke` harness exhibited PTY
timing instability across three consecutive runs. See
`benches/interactive_smoke_skip.md` for details.
```

- [ ] **Step 3: Sanity-check placeholders**

Run: `grep -n '<' performance.md | grep -v '<!-- '`
Expected: zero matches remain by end of Task 12. At this point some may still be present; that's fine — Task 12 continues.

Also: `grep -n 'TODO\|TBD\|XXX' performance.md`
Expected: zero matches.

- [ ] **Step 4: Commit progress**

```bash
git add performance.md
git commit -m "docs(perf): add performance.md sections 1-3 (methodology + results)

Environment, workload definitions, tool list, and measurement results
(samply Top-N, dhat Top-N, Criterion summary) for W1/W2/W3."
```

---

## Task 12: Author `performance.md` — sections 4 through 6, complete §1

**Files:**
- Modify: `performance.md`

- [ ] **Step 1: Identify 3-7 hotspots from §3 results**

Read `performance.md` §3 and cross-reference with `TODO.md` performance-adjacent entries. Select 3-7 hotspots that meet at least one of:
- Top-5 self-time CPU function across any workload
- Top-5 dhat bytes-allocated site
- Top-5 dhat call-count site that is not a trivial leaf (e.g., `String::from`)

For each hotspot, prepare a finding entry (templated in Step 2).

- [ ] **Step 2: Append §4 Findings**

For each hotspot, append a subsection of this form:

```markdown
### 4.<N>. <Short descriptive title>

**Location:** `<path/to/file.rs>:<line-range>`
**Measurement:**
- CPU: <% self / % total> (W<1|2|3> samply)
- Allocations: <bytes> across <count> calls (W2 dhat)

**Suspected cause:**
<1-3 sentences tying the measurement to what the code is actually doing.
Reference concrete lines or branches. Do not speculate beyond the data.>

**Fix candidates:**
1. **<Option 1 name>** — <1-2 sentences, trade-off one-liner>
2. **<Option 2 name>** — <1-2 sentences, trade-off one-liner>
   (optional 3rd)

**TODO.md cross-check:**
- <either: "confirmed existing entry at TODO.md line <N>: '<quote>'"
-  or:    "not in TODO.md — add a new entry">
```

- [ ] **Step 3: Append §5 Recommendations**

Sort the §4 findings by Impact × Effort. Write:

```markdown
## 5. Recommendations

### 5.1 Priority matrix

| Finding | Impact | Effort | Priority |
|---------|--------|--------|----------|
| 4.1 <title> | High/Med/Low | High/Med/Low | P0/P1/P2 |
...

Impact classification:
- **High:** > 10% of total CPU on a W1/W2 hotpath, or > 10% of allocated bytes in W2.
- **Medium:** 3-10%.
- **Low:** < 3% or tail allocations.

Effort classification:
- **Low:** < 1 day, contained to a single file, no API change.
- **Medium:** 1-3 days, touches 2-5 files.
- **High:** > 3 days or requires a design choice.

### 5.2 Next-project queue

Based on the matrix, the recommended order for follow-up projects is:

1. **<finding title>** (P0, <short justification>)
2. **<finding title>** (P0 or P1, ...)
...

This queue is the only continuation action from this report. No fixes
are implemented here.
```

- [ ] **Step 4: Append §6 Reproducibility**

```markdown
## 6. Reproducibility

### 6.1 Build artifacts

```bash
cargo build --profile profiling --bin yosh --bin yosh-dhat \
    --features dhat-heap \
    --bench startup_bench --bench exec_bench --bench interactive_smoke
```

### 6.2 samply runs

```bash
# W1
samply record --save-only --output target/perf/samply_w1.json -- \
    ./benches/data/startup_loop.sh ./target/profiling/yosh 1000

# W2
samply record --save-only --output target/perf/samply_w2.json -- \
    ./target/profiling/yosh benches/data/script_heavy.sh

# W3 (if not skipped)
samply record --save-only --output target/perf/samply_w3.json -- \
    ./target/profiling/deps/interactive_smoke-<hash>

# Extract Top-N tables in Markdown:
python3 scripts/perf/samply_top_n.py target/perf/samply_w1.json 10
python3 scripts/perf/samply_top_n.py target/perf/samply_w2.json 10
python3 scripts/perf/samply_top_n.py target/perf/samply_w3.json 10
```

For interactive exploration (optional): `samply load target/perf/samply_w1.json`.

### 6.3 dhat run

```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2.json

# Extract Top-N tables:
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10
```

### 6.4 Criterion runs

```bash
cargo bench
# -> target/criterion/<bench>/<function>/report/index.html
```
```

- [ ] **Step 5: Fill in §1 Executive Summary**

Replace the §1 placeholder with:

```markdown
## 1. Executive Summary

**Measured:** <YYYY-MM-DD>, commit `<short SHA>`, <OS> / <CPU>.

**Top 5 hotspots:**
1. <finding 4.1 title> — <1-line summary, e.g. "`VarStore::set` accounts for 14% of W2 CPU via LINENO updates">
2. <finding 4.2 title> — ...
3. ...

**Recommended next-project order:**
1. <top P0 finding>
2. <next P0 or P1>
3. <...>

See §4 for details and §5 for the full Impact × Effort matrix.
```

- [ ] **Step 6: Final placeholder scan**

Run: `grep -n '<[A-Za-z]' performance.md | grep -v '<!--' | grep -v '```'`
Expected: zero matches — every `<...>` placeholder must be filled.

Run: `grep -n 'TODO\|TBD\|XXX\|FIXME' performance.md`
Expected: zero matches.

Run: `grep -n 'copy from target' performance.md`
Expected: zero matches — all copy markers replaced with real content.

- [ ] **Step 7: Commit**

```bash
git add performance.md
git commit -m "docs(perf): complete performance.md — findings + recommendations

- §4: 3-7 concrete hotspots with cause, fix candidates, TODO.md cross-check
- §5: Impact × Effort matrix and next-project queue
- §6: reproducibility commands
- §1: executive summary populated

This report is the sole deliverable of the performance tuning project
(docs/superpowers/specs/2026-04-21-performance-tuning-design.md)."
```

---

## Self-review checklist

After completing all 12 tasks, verify:

1. **Spec §3 workloads all measured:** W1, W2, W3 each have §3.x in the report (or W3 has a skip marker).
2. **Spec §4 tools all used:** samply, dhat, Criterion results each appear in §3.
3. **Spec §5 report structure:** `performance.md` has sections 1–6 matching the spec's named sections.
4. **Spec §8 deliverables:** every file in the spec's deliverables checklist exists (`Cargo.toml` edits, `src/bin/yosh-dhat.rs`, `benches/data/script_heavy.sh`, `benches/data/startup_loop.sh`, `benches/startup_bench.rs`, `benches/exec_bench.rs`, `benches/interactive_smoke.rs`, `performance.md`).
5. **Out-of-scope boundary held:** no production source file under `src/` (other than `src/bin/yosh-dhat.rs`) has been modified for performance fixes. `git log --oneline <starting-commit>..HEAD -- src/` should only contain changes to `src/bin/yosh-dhat.rs`.

If any item fails, add a follow-up task and address it before declaring the report complete.
