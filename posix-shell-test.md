# POSIX Shell Compliance — Missing E2E Test Coverage

Gap analysis of `e2e/posix_spec/` against `docs/posix-shell-reference.md`
(IEEE Std 1003.1-2024, Section 2 — Shell Command Language). Date: 2026-07-13.

Legend:

- **[GAP]** — no test found anywhere under `e2e/` (genuine hole).
- **[LEGACY]** — behavior is tested in a legacy suite outside `posix_spec/`
  (`e2e/arithmetic/`, `e2e/control_flow/`, `e2e/function/`, …) but has no
  `posix_spec` counterpart. Porting is optional; listed for completeness.
- Interactive-only behavior is excluded here; see "Out of scope" at the end.

## Top gaps (highest value first)

1. **`$'...'` dollar-single-quotes (§2.2.4)** — entire subsection untested
   (new in POSIX.1-2024).
2. **`set -o pipefail` (§2.9.2)** — the 4-way `!` × pipefail exit-status
   matrix is untested (new in POSIX.1-2024).
3. **`;&` case fall-through terminator (§2.9.4.3)** — untested
   (new in POSIX.1-2024).
4. **Signal-terminated commands → `$?` = 128+N (§2.8.2)** — no test asserts
   e.g. SIGTERM → 143, neither directly nor via `wait`.
5. **Asynchronous lists (§2.9.3.1)** — async list exit status is 0; stdin of
   an async command is `/dev/null` when job control is off.
6. **Assignments preceding a special builtin persist (§2.9.1.2)** — the
   special-vs-regular builtin assignment-persistence distinction is untested.
7. **`command` with a special builtin (§2.9.1.4)** — `command` suppresses the
   exit-on-error and assignment-persistence properties of special builtins.
8. **Signals during `wait` (§2.12)** — `wait` returns immediately with status
   >128, then the trap runs.
9. **`break`/`continue` lexical containment (§2.15)** — `break` inside a
   function called from a loop must not exit the caller's loop.
10. **`set -a` / `set -v` / `set -o` / `set +o` (§2.15 set)** — allexport,
    verbose, and the option-listing output forms are untested.

---

## 2.1 Shell Introduction (`2_01_shell_introduction/`)

- [GAP] Script file operand invocation: `yosh script.sh a b` sets `$0` to the
  script path and `$1 $2` to the operands (only `-c` and stdin are tested).
- [GAP] `-c` with a `command_name` operand: `yosh -c 'echo $0 $1' name a`
  sets `$0=name`, `$1=a`.

## 2.2 Quoting (`2_02_quoting/`)

- [GAP] `$'...'` escape sequences: `\n`, `\t`, `\\`, `\'`, `\"` produce the
  corresponding characters (§2.2.4, POSIX 2024).
- [GAP] `$'\xHH'` hexadecimal and `$'\ddd'` octal byte escapes.
- [GAP] `$'\cX'` control-character escape.
- [GAP] `$'` is not special inside double-quotes: `"$'a'"` stays literal.
- [GAP] Backtick retains special meaning inside double-quotes:
  `"` `` `echo hi` `` `"` substitutes.
- [GAP] Backslash-newline inside double-quotes is a line continuation.
- [LEGACY] Inside double-quotes `\` escapes only `` $ ` " \ <newline> ``
  (`e2e/quoting/`).
- [LEGACY] Adjacent quoted strings concatenate into one word; `""`/`''`
  produce empty arguments (`e2e/quoting/`).

## 2.3 Token Recognition (`2_03_token_recognition/`)

- [GAP] `#` starts a comment only at the start of a token: `echo a#b`
  prints `a#b`.
- [GAP] Alias substitution (§2.3.1): an alias whose value ends in a blank
  makes the *next* word subject to alias substitution.
- [GAP] A quoted word is not alias-substituted (`\ls` / `'ls'` bypass an
  alias named `ls`).
- [GAP] Alias is expanded only in command-name position, not in argument
  position.

## 2.4 Reserved Words (`2_04_reserved_words/`)

- [GAP] `!` is a reserved word only in command position: `echo !` prints `!`.
- [GAP] Reserved word recognized after `&&`/`||`/`;`/`(` (only `|` and
  assignment-prefix are tested).

## 2.5.1 Positional Parameters (`2_05_01_positional_params/`)

- [GAP] `$0` is unchanged inside a function body (§2.9.5 cross-ref).
- [GAP] `${01}` is interpreted as decimal 1 (same as `$1`).

## 2.5.2 Special Parameters (`2_05_02_special_params/`)

- [GAP] `"$@"` with zero positional parameters expands to zero fields
  (`set --; set -- "$@"; echo $#` → 0).
- [GAP] Subshell creation preserves `$?`: `false; (echo $?)` prints 1.
- [GAP] Unquoted `$*` undergoes field splitting.
- [GAP] `"$*"` with unset IFS joins with a space; with empty `IFS=''` joins
  with no separator.
- [LEGACY] `"$@"` vs `"$*"` difference (`e2e/variable_and_expansion/`).

## 2.5.3 Shell Variables (`2_05_03_shell_variables/`)

- [GAP] IFS is set to `<space><tab><newline>` at shell startup even when a
  different `IFS` is inherited from the environment (POSIX 2024 wording).

## 2.6.2 Parameter Expansion (`2_06_02_parameter_expansion/`)

- [GAP] `${var:?}` with the word omitted: shell writes a default diagnostic
  and exits.
- [GAP] `${var:?word}` when var is set and non-null expands to the value
  (no error path).
- [GAP] Conditional forms on positional parameters: `${1:-default}` etc.
- [GAP] Under `set -u`, `${unset-fallback}` / `${unset:-fallback}` do NOT
  error (nounset interacts with default forms).
- [GAP] Assignment forms on positional/special parameters are an error:
  `${1=x}`.
- [LEGACY] `${var=word}` assigns when unset; `${var+word}` set-but-empty
  matrix (`e2e/variable_and_expansion/`).

## 2.6.3 Command Substitution (`2_06_03_command_substitution/`)

- [GAP] Backtick nesting with escaped inner backticks: `` `echo \`echo hi\`` ``.
- [GAP] Backslash rules inside backticks: `\$`, `` \` ``, `\\` are special;
  other backslashes are literal.
- [GAP] Command substitution runs in a subshell environment: variable
  assignments inside `$(...)` do not leak to the parent.
- [GAP] `$( (echo x) )` — space disambiguates a subshell from arithmetic
  expansion `$((`.
- [LEGACY] Embedded (non-trailing) newlines preserved; empty substitution;
  deep nesting (`e2e/command_substitution/`).

## 2.6.4 Arithmetic Expansion (`2_06_04_arithmetic_expansion/`)

- [GAP] Invalid expression: error message to stderr, expansion fails, and a
  non-interactive shell exits (expansion-error consequence, §2.8.1).
- [GAP] `$`-prefixed parameter expansion inside `$(( $x + 1 ))` (only bare
  names are tested in posix_spec).
- [LEGACY] Hex/octal literals, assignment persistence, comparison / logical /
  bitwise / shift / ternary / comma / compound-assignment operators, unary
  minus, division and modulo by zero (`e2e/arithmetic/`).

## 2.6.5 Field Splitting (`2_06_05_field_splitting/`)

- [GAP] Leading IFS non-whitespace delimiter yields a leading empty field:
  `IFS=:; v=":a"` → 2 fields, first empty.
- [GAP] Trailing IFS non-whitespace delimiter does not create a trailing
  empty field: `v="a:"` → 1 field.
- [GAP] Field splitting applies to command-substitution results
  (`set -- $(echo "a b")` → 2 fields).

## 2.6.6 Pathname Expansion (`2_06_06_pathname_expansion/`)

- [GAP] Expansion results are sorted in collation order.
- [GAP] `*` / `?` / bracket expressions never match `/`: `a*c` does not
  match `a/c`.
- [GAP] An explicit leading `.` in the pattern matches dot-files
  (`.f*` matches `.foo`).
- [GAP] A negated bracket expression `[!a]*` still does not match a leading
  dot.
- [GAP] Wildcard in a non-final path component: `*/data.txt`.

## 2.7 Redirection (`2_07_redirection/`, `2_07_04_heredoc/`)

- [GAP] A quoted digit is a word, not an IO_NUMBER: `echo x "2">f` writes
  `x 2` to `f` via fd 1.
- [GAP] Redirection applied to a compound command:
  `while read l; do echo "$l"; done < file`, `{ ...; } > f`, `if ... fi > f`.
- [GAP] Redirection to a path in a nonexistent directory fails the command;
  the shell continues (regular command).
- [GAP] Two here-document operators on one line are gathered in order
  (`prog <<A 3<<B`).
- [GAP] Partially quoted here-doc delimiter counts as quoted: `cat <<E"OF"`
  suppresses expansion in the body.
- [GAP] Here-doc body: command substitution and arithmetic expansion with an
  unquoted delimiter (only parameter expansion is tested in posix_spec).
- [LEGACY] `set -C` does not restrict `>>`; command substitution in a
  redirect filename; empty heredoc; heredoc piped onward (`e2e/redirection/`).

## 2.8 Exit Status and Errors (`2_08_01_consequences_of_shell_errors/`)

- [GAP] Assignment to a readonly variable causes a non-interactive shell to
  exit (variable-assignment error row of the §2.8.1 table; existing tests
  only check the error message/value, not shell exit).
- [GAP] A "shall exit" error inside a subshell exits only the subshell with
  nonzero status; the parent continues.
- [GAP] Command terminated by a signal: `$?` is greater than 128
  (e.g. SIGTERM → 143).
- [GAP] A special-builtin error invoked via `command` does not exit the
  shell (§2.9.1.4 cross-ref).

## 2.9.1 Simple Commands (`2_09_01_simple_commands/`)

- [GAP] Variable assignments preceding a *special* builtin persist in the
  current environment (`v=1 :; echo $v` → 1).
- [GAP] Variable assignments preceding a *regular* builtin do not persist.
- [GAP] Command search order: a function shadows a PATH utility of the same
  name; a special builtin cannot be shadowed by a function.
- [GAP] First word expanding to nothing: `e=; $e echo hi` runs `echo hi`.
- [LEGACY] Prefix assignment on an external command does not persist;
  126/127 statuses; ENOEXEC `/bin/sh` fallback (`e2e/command_execution/`).

## 2.9.2 Pipelines (`2_09_02_pipelines/`)

- [GAP] `set -o pipefail`: the full 4-pattern exit-status matrix with and
  without `!` (§9.2 table). No pipefail test exists anywhere.
- [GAP] `!` applied to a *succeeding* pipeline yields exactly 1.
- [LEGACY] Each pipeline command runs in a subshell environment
  (`e2e/pipeline_and_list/`).

## 2.9.3 Lists (`2_09_03_lists/`)

- [GAP] Exit status of an asynchronous list is 0 (`false & echo $?` → 0).
- [GAP] With job control disabled, stdin of an async command is `/dev/null`
  (a background `read` hits EOF instead of consuming the script's stdin).
- [GAP] Newline after `&&` / `||` continues the list (linebreak rule).

## 2.9.4 Compound Commands (`2_09_04_compound_commands/`)

- [GAP] `;&` fall-through: execution continues into the next clause's
  compound-list without pattern matching, until `;;` (POSIX 2024).
- [GAP] `case` item with a leading `(` before the pattern: `( pat ) ...;;`.
- [GAP] `case` with no matching pattern → exit status 0; with a match →
  status of the last command executed in the clause.
- [GAP] The `case` word and the patterns both undergo expansions
  (variable in the word; variable in a pattern).
- [GAP] Brace-group / subshell exit status is that of the compound-list.
- [LEGACY] `until`; `if`/`elif`/`else` execution paths; `for` with an empty
  word list; never-true `while` (`e2e/control_flow/`).

## 2.9.5 Function Definition (`2_09_05_function_definition/`)

- [GAP] `$0` is unchanged during function invocation.
- [GAP] Defining a function with the name of a special builtin is an error.
- [GAP] The exit status of the function *definition* itself is 0.
- [LEGACY] Positional parameters and `$#` restored after the call;
  redirection attached to a function definition applies at invocation;
  a function overrides a regular builtin (`e2e/function/`).

## 2.10 Shell Grammar (`2_10_shell_grammar/`, `2_10_1_lexical/`)

Coverage is thorough. No required gaps identified beyond items already
listed under other sections (e.g. quoted IO_NUMBER under §2.7,
`&&`-newline under §2.9.3).

## 2.11 Signals and Error Handling (`2_11_signals_and_error_handling/`)

(Directory numbering follows POSIX.1-2017; reference doc §12.)

- [GAP] Commands in async lists ignore SIGINT/SIGQUIT when job control is
  disabled (`(kill -INT $$; echo alive) & wait`-style probe on the child).
- [GAP] Signal arriving during `wait`: `wait` returns immediately with
  status >128, then the trap action runs.
- [GAP] A trapped signal received while a foreground command runs executes
  the trap only after that command completes.
- [GAP] Signals ignored at non-interactive shell entry cannot be trapped.
- [GAP] `exit` without an operand inside a trap action uses the pre-trap
  `$?`.
- [GAP] The EXIT trap runs in the environment of the last executed command
  (`$?` of the last command is visible inside the EXIT trap).
- [GAP] `trap -p` and `trap -p CONDITION` output form (POSIX 2024).

## 2.12 Shell Execution Environment (`2_12_shell_exec_env/`)

(Reference doc §13.)

- [GAP] `umask` changed in a subshell does not affect the parent.
- [GAP] `cd` in a command substitution does not affect the parent's PWD.
- [LEGACY] cd-in-subshell isolation, shell-option isolation, alias/function
  isolation, ignored-signal inheritance (`e2e/subshell/`).

## 2.13 Pattern Matching (`2_13_pattern_matching/`)

(Reference doc §14.)

- [GAP] `*` greedy semantics: with `abcXdefXghi`, `${v#*X}` → `defXghi` and
  `${v##*X}` → `ghi` (shortest vs longest driven by `*` matching).
- [GAP] In `case` patterns (non-filename context) wildcards DO match `/`
  and a leading `.` (contrast with pathname expansion rules).
- [GAP] Concatenated single- and multi-character patterns: `a?c*d`.

## 2.14/2.15 times (`2_14_13_times/`)

Covered adequately.

## Special Built-ins (`4_special_builtin/`)

- [GAP] `set -a` (allexport): subsequent assignments are exported.
- [GAP] `set -v` (verbose): input lines echoed to stderr as read.
- [GAP] `set -o` with no args: option-listing output; `set +o`: output
  suitable for re-input.
- [GAP] `set -o pipefail` (see §2.9.2).
- [GAP] `set a b c` (operands with no `--`) sets positional parameters.
- [GAP] `break`/`continue` lexical containment: `break` inside a function
  called from a loop does not exit the caller's loop (POSIX 2024 wording).
- [GAP] `exit` invoked inside an EXIT trap action exits immediately with the
  given status (no re-run of the trap).
- [GAP] `dot` of an empty file → exit status 0.
- [GAP] `trap n [condition]` first-operand-numeric reset form
  (e.g. `trap 15` ≡ `trap - 15`). Low priority.
- [GAP] `set -b` / `set -h` — low priority (`-b` needs job control; `-h` is
  deprecated).

## Required (regular) Built-ins (`4_required_builtin/`)

- [GAP] `command SPECIAL_BUILTIN`: error does not exit the shell; preceding
  assignments do not persist (the two §2.15 properties are suppressed).
- [GAP] `read` splits input using the current `IFS` (non-default IFS).
- [GAP] `read` without `-r`: backslash-newline joins continuation lines;
  backslash escapes the next character.
- [GAP] `umask` symbolic mode operand: `umask u=rwx,g=rx,o=` sets the mask.
- [GAP] `wait` on a child terminated by a signal returns 128+N.
- [GAP] `command -v` on a reserved word reports the word itself.
- [GAP] `kill %jobspec` form (`kill %1`). Low priority.
- [LEGACY] CDPATH resolution details, `cd --`, `%string` job specs,
  `test`/`[` suite, `command -p/-v/-V` details (`e2e/builtin/`,
  `e2e/builtin_command/`).

## Out of scope for file-based E2E

- Job control semantics (reference doc §11): foreground/background process
  groups, `fg`/`bg` state transitions, `SIGTSTP` suspension — covered by the
  PTY suite (`tests/pty_interactive.rs`); e2e only asserts the error paths
  without monitor mode.
- Interactive-only behavior: PS1/PS2 display, `ignoreeof`, `ENV` sourcing
  visibility, history editing via `fc` with a live editor.
- `newgrp` (process-group/login side effects; not safely testable).
- Unspecified/implementation-defined behaviors (`${00}`, `!(`, unsetting
  special parameters, `%`/`#` pattern removal on `$@`/`$*`, null bytes in
  command substitution) — only worth testing as documented yosh choices,
  not as POSIX conformance.
