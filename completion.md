# Command Completion Definition Files — Design

Status: **Draft (design approved, not yet implemented)**
Date: 2026-07-14

User-definable tab completion for commands: subcommands, flags, and
argument candidates, declared in per-command TOML files.

## Goals

- Let users freely define completions for any command without writing
  Rust code or building a wasm plugin.
- Declarative first: static structure (subcommands, flags) is declared
  in TOML; dynamic candidates (e.g. `git branch` output) are produced
  by running a shell command declared as a string.
- Zero startup cost: definitions are loaded lazily, per command, on
  first Tab.
- Never break the line editor: any failure (missing file, parse error,
  slow/failing `exec`) degrades gracefully to the existing path
  completion.

## Non-goals (v1)

- Candidate descriptions in the selector UI (fish-style
  `candidate — description`). The schema deliberately leaves room to
  add a `description` field later, but v1 neither parses nor displays
  it.
- Project-local completion directories (`./.yosh/completions/`).
- A `complete` builtin / shell-function-based completion API.
- Completion for words that need full shell expansion (globs,
  parameter expansion) inside the completed word.

## File location and discovery

```
~/.config/yosh/completions/<command>.toml
```

- The file name (minus `.toml`) is the command name it completes.
  Only the final path component of the typed command is matched:
  `/usr/bin/git` and `git` both resolve to `git.toml`.
- Loaded lazily: the first Tab press whose command word is `git` reads
  and parses `git.toml`. The parsed spec is cached in the interactive
  session (keyed by command name). A missing file is also cached as a
  negative entry so repeated Tabs don't re-stat the directory.
- A parse error prints one warning per session to stderr —
  `yosh: completion: git.toml: <error>` — and falls back to path
  completion for that command.
- The directory location follows the existing convention used by
  `plugins.lock` (`~/.config/yosh/`).

## Schema

### Candidate sources

Everywhere the schema needs "how to produce candidates" it accepts a
**candidate source** — a table with exactly one of these keys:

| Key      | Type     | Meaning                                                        |
| -------- | -------- | -------------------------------------------------------------- |
| `type`   | string   | Built-in generator: `"file"`, `"directory"`, `"command"`, `"none"` |
| `values` | [string] | Static candidate list                                          |
| `exec`   | string   | Shell command; stdout is split on newlines, one candidate per line |

- `type = "file"` / `"directory"` reuse the existing path-completion
  logic (`src/interactive/completion.rs`), with `directory` filtering
  to directories only.
- `type = "command"` reuses the existing command-name completer
  (PATH executables + builtins + aliases) — useful for wrappers like
  `sudo`, `time`, `xargs`.
- `type = "none"` produces no candidates **and** suppresses the
  default path-completion fallback (for free-form values such as a new
  branch name or a commit message).
- Specifying zero keys or more than one key is a parse error.

### Top-level structure

The top level of the file describes the command itself. `args`,
`flags`, and `subcommands` may each appear at any level of the
subcommand tree; all are optional.

```toml
# ~/.config/yosh/completions/git.toml

# Positional arguments of the bare command (used when the command has
# no subcommands, or for words before the first subcommand).
[[args]]
type = "file"

# Flags. `names` lists all spellings of one flag.
[[flags]]
names = ["-C"]
value = { type = "directory" }   # presence of `value` = flag takes a value

[[flags]]
names = ["--no-pager"]           # no `value` = boolean flag

# Subcommands. Nest arbitrarily deep via [[subcommands.subcommands]].
[[subcommands]]
name = "checkout"

[[subcommands.flags]]
names = ["-b"]
value = { type = "none" }        # new branch name: no candidates

[[subcommands.args]]
exec = "git branch --format='%(refname:short)'"

[[subcommands]]
name = "remote"

[[subcommands.subcommands]]
name = "add"

[[subcommands.subcommands]]
name = "remove"

[[subcommands.subcommands.args]]
exec = "git remote"
```

Field reference:

- `args` — array of candidate sources, one per positional argument,
  in order. The **last** entry repeats: a command whose `args` has one
  entry completes every positional argument from that source. An empty
  or absent `args` means positional arguments fall back to path
  completion (unless suppressed with `type = "none"`).
- `flags` — array of tables:
  - `names` (required, non-empty) — all spellings, e.g.
    `["-b"]` or `["-m", "--message"]`. Short (`-x`) and long (`--xyz`)
    forms are both just strings; the engine does not synthesize one
    from the other.
  - `value` (optional) — a candidate source. If present the flag takes
    a value, completed from this source. If absent the flag is boolean.
- `subcommands` — array of tables:
  - `name` (required) — the literal subcommand word.
  - `args` / `flags` / `subcommands` — same shapes, recursively.

Duplicate `name` within one `subcommands` array, or duplicate flag
spelling within one level, is a parse error.

## Matching semantics

On Tab, the engine:

1. Extracts the current word and the words before it on the current
   simple command, reusing the existing word-extraction logic
   (`extract_completion_word` and the command-position scanner). Only
   the current pipeline segment is considered — words after `|`, `;`,
   `&&` etc. start a fresh command.
2. Resolves the first word to a spec file (basename, lazy load). No
   spec → existing behavior (path completion / command completion).
3. Walks the preceding words left to right against the spec tree:
   - A word matching a `subcommands[].name` at the current level
     descends into that subcommand. Matching resumes at the new level.
   - A word matching a boolean flag is consumed.
   - A word matching a value-taking flag consumes the **next** word as
     its value (`-C /path`). A word of the form `--flag=value` is
     self-contained.
   - Any other word is counted as a positional argument at the current
     level.
4. Generates candidates for the word under the cursor:
   - If the previous word is a value-taking flag → that flag's `value`
     source.
   - Else if the current word starts with `-` → all flag spellings at
     the current level (prefix-filtered). For `--flag=` with a cursor
     after the `=`, the flag's `value` source.
   - Else → the current level's `subcommands` names **plus** the
     candidate source for the current positional index in `args`.
5. Prefix-filters candidates by the current word and hands them to the
   existing completion flow (longest-common-prefix insertion, selector
   UI on multiple matches).

Fallback rule: if the resolved source produces zero candidates and the
source was not `type = "none"`, fall back to path completion — a wrong
or stale spec should never make Tab dead.

## `exec` execution

- Runs as a child process: `sh -c '<exec string>'` with the shell's
  current working directory and exported environment. Running in a
  child (not the in-process executor) guarantees completion can never
  mutate shell state (variables, cwd, traps) and can be killed on
  timeout.
- stdout is split on `\n`; empty lines are dropped; no further parsing
  or word-splitting is applied. stderr is discarded.
- **Timeout: 500 ms.** On timeout the child is killed and the result
  is treated as zero candidates (triggering the fallback rule above).
  A non-zero exit status is likewise treated as zero candidates.
- Results are not cached — each Tab reruns the command, so candidates
  (branches, remotes, containers…) are always fresh.
- Security stance: definition files are user-owned configuration, the
  same trust level as `.profile`. No sandboxing is applied, but the
  500 ms timeout bounds the interactive cost.

## Error handling summary

| Failure                          | Behavior                                          |
| -------------------------------- | ------------------------------------------------- |
| No spec file for command         | Existing path/command completion                  |
| TOML parse / schema error        | Warn once per session on stderr; path completion  |
| `exec` non-zero / timeout        | Zero candidates → fallback rule                   |
| `exec` output empty              | Zero candidates → fallback rule                   |
| Source is `type = "none"`        | No candidates, no fallback                        |

## Implementation sketch

New module `src/interactive/spec_completion.rs`:

- `struct CompletionSpec` — deserialized via `serde` + `toml`
  (both already in the dependency tree via the plugin config).
- `struct SpecStore` — lazy loader + per-session cache
  (`HashMap<String, Option<CompletionSpec>>`), lives next to
  `CommandCompleter` in the interactive loop's state.
- `fn resolve(spec, words, current_word) -> ResolvedSource` — pure
  function implementing the matching semantics; unit-testable without
  a terminal.
- Integration point: `LineEditor::handle_tab_complete`
  (`src/interactive/line_editor.rs`) — before the existing
  path-completion call, consult `SpecStore`; on a resolved source,
  produce candidates and feed the existing selector/common-prefix
  machinery.

## Testing

- **Unit tests** for schema parsing (valid, invalid, duplicate names,
  multi-key sources) and for `resolve` over a fixture spec — table
  cases for subcommand descent, flag value position, `--flag=` forms,
  positional indexing, repeat-last-arg.
- **Unit tests** for `exec` execution with a tempdir spec: newline
  splitting, timeout kill, non-zero exit.
- **PTY E2E** (`tests/pty_interactive.rs`): one end-to-end case —
  spec file in a temp `$HOME`, type `cmd <Tab>`, assert the candidate
  is inserted. Keep timeouts generous per existing PTY-test guidance.

## Future extensions (explicitly out of v1)

- `description` on subcommands/flags/values, displayed in the
  selector UI.
- Project-local `./.yosh/completions/` search path.
- A `complete` builtin for script-driven registration.
- Caching/TTL for expensive `exec` sources.
