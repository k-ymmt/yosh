# POSIX Shell Command Language Reference

Implementation reference for building a POSIX-compliant Unix shell.
Based on IEEE Std 1003.1-2024 (POSIX.1-2024), Section 2 — Shell Command Language.

Source: https://pubs.opengroup.org/onlinepubs/9799919799/utilities/V3_chap02.html

---

## Table of Contents

1. [Shell Processing Pipeline](#1-shell-processing-pipeline)
2. [Quoting](#2-quoting)
3. [Token Recognition](#3-token-recognition)
4. [Reserved Words](#4-reserved-words)
5. [Parameters and Variables](#5-parameters-and-variables)
6. [Word Expansions](#6-word-expansions)
7. [Redirection](#7-redirection)
8. [Exit Status and Errors](#8-exit-status-and-errors)
9. [Shell Commands](#9-shell-commands)
10. [Shell Grammar](#10-shell-grammar)
11. [Job Control](#11-job-control)
12. [Signals and Error Handling](#12-signals-and-error-handling)
13. [Shell Execution Environment](#13-shell-execution-environment)
14. [Pattern Matching](#14-pattern-matching)
15. [Special Built-In Utilities](#15-special-built-in-utilities)
16. [Implementation Notes](#16-implementation-notes)

---

## 1. Shell Processing Pipeline

Reference: Section 2.1

The shell processes input in the following order:

1. Read input from file (`sh`), `-c` option, or `system()`/`popen()`
2. Break input into tokens (words and operators)
3. Parse tokens into simple commands and compound commands
4. Perform word expansions (escape sequences, parameter substitution, etc.)
5. Perform redirections and remove redirection operators
6. Execute commands (functions, built-ins, or external programs)
7. Optionally collect exit status and wait for completion

For each word, the shell first processes backslash escape sequences within dollar-single-quotes, then performs the various expansions.

---

## 2. Quoting

Reference: Section 2.2

Quoting removes the special meaning of metacharacters and prevents reserved word recognition.

### Characters that always require quoting

```
& ; < > ( ) $ ` \ " ' <space> <tab> <newline>
```

### Characters that require quoting depending on context

```
* ? [ ] ^ - ! # ~ = % { , }
```

### 2.1 Escape Character (Backslash)

- An unquoted `\` preserves the literal value of the next character (except `<newline>`)
- `\<newline>` is a line continuation: both characters are removed from the token
- The backslash itself is retained in the token, along with the next character

### 2.2 Single-Quotes

- All characters within single-quotes retain their literal values
- A single-quote **cannot** appear within single-quotes

### 2.3 Double-Quotes

- Most characters are literal, but `$`, backtick, and `\` retain special meaning
- `$` introduces parameter expansion, command substitution, and arithmetic expansion
- Characters within `$(...)` and `${...}` are not affected by double-quotes
- Outside `$(...)` and `${...}`, `\` only acts as an escape before `$`, `` ` ``, `\`, `<newline>`, `"`

### 2.4 Dollar-Single-Quotes

`$'...'` form: preserves literal values while processing these escape sequences:

| Escape | Meaning |
|--------|---------|
| `\"` | Double-quote |
| `\'` | Single-quote |
| `\\` | Backslash |
| `\a` | Alert (BEL) |
| `\b` | Backspace |
| `\e` | ESC character |
| `\f` | Form feed |
| `\n` | Newline |
| `\r` | Carriage return |
| `\t` | Tab |
| `\v` | Vertical tab |
| `\cX` | Control character |
| `\xHH` | Hexadecimal byte value |
| `\ddd` | Octal byte value (1-3 digits) |

These escapes are processed immediately before expansion of the word containing the dollar-single-quote string.

---

## 3. Token Recognition

Reference: Section 2.3

The shell applies the following rules in order to break input into tokens (except during here-document processing):

1. End of input delimits the current token
2. If the previous character is part of an operator and the current character can extend that operator, it is part of the same token
3. If the current character cannot extend the operator, the operator is delimited
4. Unquoted `\`, `'`, `"`, `$'` affect the quoting state of subsequent characters
5. Unquoted `$` or backtick begins an expansion
6. An unquoted operator-start character delimits the current token and starts a new operator token
7. Unquoted whitespace delimits the current token and is discarded
8. If the previous character is part of a word, the current character is appended to the word
9. `#` starts a comment (until the next newline)
10. The current character starts a new word

**Key principle:** Characters from token start to end strictly constitute that token, including quote characters.

### 3.1 Alias Substitution

A token is subject to alias substitution when ALL of the following conditions are met:

- It contains no quote characters
- It is a valid alias name
- That alias exists
- It was not generated from a previous substitution of the same alias name
- It is parsed as the command name of a simple command, or follows an alias substitution that ended with whitespace

When an alias value is substituted, token recognition restarts from the beginning of the value. If the alias value ends with whitespace, the next token is also subject to alias substitution.

---

## 4. Reserved Words

Reference: Section 2.4

### Required reserved words

```
! { } case do done elif else esac fi for if in then until while
```

### Optional reserved words (behavior is unspecified if recognized)

```
[[ ]] function namespace select time
```

### Recognition conditions

Reserved words are recognized only when:

- Characters are not quoted
- The word is the first word of a command
- The word follows another reserved word (except `case`, `for`, `in`)
- For `case`: only `in` is valid as the 3rd word
- For `for`: only `in` and `do` are valid as the 3rd word

All words ending with `:` are reserved words when recognized, but their use produces unspecified results.

---

## 5. Parameters and Variables

Reference: Section 2.5

### 5.1 Positional Parameters

- Denoted by positive decimal integers
- Always interpreted as decimal (even with leading zeros)
- Multi-digit parameters must be enclosed in `{}`: `${10}`
- `$10` is `$1` concatenated with `0`
- `${00}` behavior is unspecified (`0` is a special parameter, not positional)
- Set/changed at shell invocation, function calls, or via `set`

### 5.2 Special Parameters

| Parameter | Expansion |
|-----------|-----------|
| `@` | Each positional parameter as a separate field. Within double-quotes with field splitting, each parameter is preserved as an individual field. Zero fields if no positional parameters |
| `*` | Positional parameters. Without field splitting, joined by the first character of `IFS` into a single field |
| `#` | Decimal count of positional parameters (shortest form) |
| `?` | Exit status of the most recent pipeline. Subshell creation preserves the calling shell's value |
| `-` | Current option flags concatenated as a string. `-i` is always included for interactive shells |
| `$` | Shell's process ID (same value even in subshells) |
| `!` | Process ID associated with the last asynchronous AND-OR list |
| `0` | Shell name or shell script name |

### 5.3 Shell Variables

Initialized from the environment. New values can be set via variable assignments.

| Variable | Purpose |
|----------|---------|
| `ENV` | Executed as a file (after parameter expansion) when an interactive shell starts |
| `HOME` | User's home directory. Used for tilde expansion |
| `IFS` | Characters used for field splitting. When unset, behaves as `<space><tab><newline>`. Set to `<space><tab><newline>` at shell startup |
| `LANG` | Default value for unset internationalization variables |
| `LC_ALL` | Overrides all `LC_*` variables and `LANG` |
| `LC_COLLATE` | Determines behavior of range expressions, equivalence classes, and multi-character collating elements in pattern matching |
| `LC_CTYPE` | Determines interpretation of byte sequences as characters and character classes in pattern matching |
| `LC_MESSAGES` | Determines the language for messages |
| `LINENO` | Line number of the currently executing script/function (starts at 1) |
| `NLSPATH` | Location of message catalogs for `LC_MESSAGES` |
| `PATH` | Command search path |
| `PPID` | Shell's parent process ID (same value even in subshells) |
| `PS1` | Interactive shell primary prompt (default: `"$ "`). `!` expands to history number |
| `PS2` | Continuation input prompt (default: `"> "`) |
| `PS4` | Execution trace prompt (default: `"+ "`) |
| `PWD` | Current working directory. Updated by `cd`, etc. |

---

## 6. Word Expansions

Reference: Section 2.6

### Expansion order

1. Tilde expansion, parameter expansion, command substitution, arithmetic expansion (left to right)
2. Field splitting
3. Pathname expansion (unless `set -f` is active)
4. Quote removal (always last)

### 6.1 Tilde Expansion

- From `~` at the start of a word to the first `/` or end of word
- In assignments, also applies at the start or after each unquoted `:`
- `~` alone: replaced with the value of `HOME`
- `~name`: replaced with the home directory of `name`'s login name (`getpwnam()` equivalent)
- The result is protected from field splitting and pathname expansion
- If followed by `/` and the path ends with `/`, the trailing `/` should be omitted

### 6.2 Parameter Expansion

**Basic form:** `${parameter}` — replaced with the parameter's value

**Conditional forms:**

| Form | Behavior |
|------|----------|
| `${parameter:-word}` | Use `word` if unset or null |
| `${parameter:=word}` | Assign `word` if unset or null (variables only, not positional/special) |
| `${parameter:?word}` | Error `word` to stderr and exit if unset or null (interactive need not exit) |
| `${parameter:+word}` | Use `word` if set and non-null, otherwise null |
| `${parameter-word}` | Use `word` if unset (null is not used) |
| `${parameter=word}` | Assign `word` if unset (null is not assigned) |
| `${parameter?word}` | Error if unset (null is OK) |
| `${parameter+word}` | Use `word` if set (even if null) |

**String operations:**

| Form | Behavior |
|------|----------|
| `${#parameter}` | Character count of parameter value |
| `${parameter%word}` | Remove shortest match of pattern from the end |
| `${parameter%%word}` | Remove longest match of pattern from the end |
| `${parameter#word}` | Remove shortest match of pattern from the start |
| `${parameter##word}` | Remove longest match of pattern from the start |

**Key rules:**

- With `:` — tests for unset or null
- Without `:` — tests only for unset
- When `set -u` is active, expanding an unset variable fails
- Using `#`, `*`, `@` with `%`/`#` pattern removal produces unspecified results

### 6.3 Command Substitution

**Modern form:** `$(commands)` — recommended

**Legacy form:** `` `commands` ``

- Executes commands in a subshell environment, replaces with stdout
- Trailing newlines are removed (other newlines are preserved)
- Behavior with null bytes is unspecified
- Results are not subject to tilde expansion, parameter expansion, command substitution, or arithmetic expansion

**Backtick rules:**

- Outside double-quotes, `\` is literal except before `$`, `` ` ``, `\`
- Nesting: `` \`commands\` `` (escape inner backticks with backslash)

**`$((` ambiguity:**

- `$((` prioritizes arithmetic expansion
- For command substitution starting a subshell: `$( (commands) )`

### 6.4 Arithmetic Expansion

Form: `$((expression))`

- The expression is treated as if in double-quotes (but `"` is not special)
- Parameter expansion, command substitution, and quote removal are performed on all tokens
- **Required:** Signed long integer arithmetic only
- **Required:** Decimal, octal, and hexadecimal constants (ISO C compliant)
- **Not required:** `sizeof()`, prefix/postfix `++`/`--` (implementation-dependent)
- **Not required:** Selection, iteration, jump statements
- Changes to variables remain in effect after expansion
- Invalid expressions produce an error to stderr and the expansion fails

### 6.5 Field Splitting

Applied to expansion results outside double-quotes.

**IFS behavior:**

- `IFS` set and non-empty: split on `IFS` characters
- `IFS` is empty string: no field splitting (but empty expansion result fields are removed)
- `IFS` unset: behaves as `<space><tab><newline>`

**IFS whitespace:** `<space>`, `<tab>`, `<newline>` that are contained in `IFS`

**Algorithm:**

1. IFS whitespace is skipped at the beginning and end
2. An IFS non-whitespace character creates one field delimiter (along with surrounding whitespace)
3. Consecutive whitespace is treated as a single delimiter
4. Only bytes from expansion are treated as delimiters (literal bytes are always ordinary characters)

### 6.6 Pathname Expansion

When `set -f` is not active, each field is expanded via pattern matching.

- If the pattern matches existing files/paths, the pattern is replaced with a sorted list of matches
- If no match, the pattern string remains as-is

### 6.7 Quote Removal

- `$'...'`, backslash, single-quotes, and double-quotes are removed unless they themselves are quoted
- After removal, the shell still remembers which characters were quoted (for `case` pattern matching, etc.)

---

## 7. Redirection

Reference: Section 2.7

**Basic form:** `[n]redir-op word`

- `n` is an optional file descriptor number (no preceding whitespace allowed)
- If `n` is quoted, the expression is not recognized as a redirection
- Required FD range: 0-9 (implementations may support wider ranges)
- Word expansion for redirection operators: tilde, parameter, command substitution, arithmetic, quote removal (`<<`/`<<-` only do quote removal)
- Pathname expansion is not performed in non-interactive shells
- Multiple redirections are evaluated left to right
- Failure to open a file causes the redirection to fail

### 7.1 Redirecting Input

Form: `[n]<word`

- `n` defaults to stdin (FD 0)

### 7.2 Redirecting Output

Form: `[n]>word` or `[n]>|word`

- `n` defaults to stdout (FD 1)
- With `noclobber` (`set -C`): `>` fails if a regular file already exists
- `>|` ignores `noclobber`
- File existence check and open are atomic (`O_CREAT|O_EXCL` equivalent)
- Without `noclobber`: existing file opened with `O_TRUNC`

### 7.3 Appending Redirected Output

Form: `[n]>>word`

- Opened with `O_APPEND` flag
- File is created if it does not exist

### 7.4 Here-Document

Form: `[n]<<word` or `[n]<<-word`

- Content from the next newline until a line consisting only of the delimiter is used as input
- `n` defaults to stdin (FD 0)

**Quoting and delimiter:**

- If `word` is quoted: quote-removed string becomes delimiter, here-document lines are NOT expanded
- If `word` is not quoted: the string as-is becomes delimiter, lines undergo parameter expansion, command substitution, arithmetic expansion

**Backslash behavior (during expansion):** Same as within double-quotes, but `"` is not special

**`<<-` operator:** Strips leading tab characters from all lines (including the delimiter line)

**Multiple here-documents:** If multiple `<<` operators appear on one line, the first operator is processed first

**Interactive:** `PS2` value is written to stderr before each line of input

### 7.5 Duplicating an Input File Descriptor

Form: `[n]<&word`

- If `word` is a digit: FD `n` (default 0) becomes a copy of FD `word`
- If `word` is `-`: close FD `n` (default 0) (closing an unopened FD is not an error)

### 7.6 Duplicating an Output File Descriptor

Form: `[n]>&word`

- If `word` is a digit: FD `n` (default 1) becomes a copy of FD `word`
- If `word` is `-`: close FD `n` (default 1)

### 7.7 Open File Descriptors for Reading and Writing

Form: `[n]<>word`

- Opened for both reading and writing
- File is created if it does not exist
- `n` defaults to stdin (FD 0)

---

## 8. Exit Status and Errors

Reference: Section 2.8

### 8.1 Consequences of Shell Errors

| Error Type | Non-interactive Shell | Interactive Shell |
|------------|----------------------|-------------------|
| Shell language syntax error | Exit | Do not exit |
| Special built-in error (not via `command`) | Exit | Do not exit |
| Other utility error | Do not exit | Do not exit |
| Special built-in redirection error | Exit | Do not exit |
| Compound command/function redirection error | Do not exit | Do not exit |
| Variable assignment error | Exit | Do not exit |
| Expansion error | Exit | Do not exit |

**Errors in subshells:** When a "shall exit" or "may exit" error occurs, the subshell exits with a non-zero exit status.

### 8.2 Exit Status for Commands

| Situation | Exit Status |
|-----------|-------------|
| Command not found | 127 |
| Command exists but not executable | 126 |
| Terminated by signal | Greater than 128 (to identify the signal) |
| Normal completion | `WEXITSTATUS` macro value (exit() argument mod 256) |

---

## 9. Shell Commands

Reference: Section 2.9

### 9.1 Simple Commands

An arbitrary sequence of variable assignments and redirections, optionally followed by words and redirections.

#### 9.1.1 Order of Processing

1. **Word classification:** Save variable assignments and redirections for steps 3 and 4
2. **Command name identification:** Expand the first word that is neither an assignment nor a redirection. If fields remain, the first is the command name. Subsequent words are also expanded (declaration utilities use assignment context)
3. **Redirection execution**
4. **Variable assignment expansion:** Tilde, parameter, command substitution, arithmetic, quote removal (no field splitting or pathname expansion)

When there is no command name, or for special built-ins, the order of steps 3 and 4 may be swapped.

#### 9.1.2 Variable Assignments

- **No command name:** Affects the current execution environment
- **Non-special built-in or non-function command:** Exported to the command's execution environment but does not affect the current environment (except for side effects of expansion)
- **Special built-in:** Affects the current environment before command execution and persists after completion
- **Non-standard user function:** Affects the current environment during function execution. After completion, behavior is unspecified

**Error:** Assignment to a readonly variable causes a variable assignment error.

#### 9.1.3 Commands with no Command Name

- Redirections are performed in a subshell environment
- Redirection failure causes immediate failure with non-zero exit status
- If command substitution is present, the exit status is that of the last command substitution
- Otherwise, exit status is 0

#### 9.1.4 Command Search and Execution

**Search order for command names without slashes:**

1. Special built-in utilities
2. Implementation-defined utility names (`alloc`, `declare`, `integer`, `typeset`, `local`, etc.)
3. Functions
4. Built-in utilities (intrinsic utilities)
5. `PATH` search

**On PATH search success:**

- If a built-in or function is associated with the found directory location, execute it
- Otherwise, execute as a non-built-in utility per 9.1.6

**On PATH search failure:** Exit status 127, error message

**Command names with slashes:** Execute directly per 9.1.6

#### 9.1.5 Standard File Descriptors

If FDs 0, 1, 2 are closed when executing a utility, the implementation may open them to unspecified files.

#### 9.1.6 Non-built-in Utility Execution

When executing via PATH search or a command name containing a slash: after all expansions, assignments, and redirections, launch the executable via the system mechanism.

### 9.2 Pipelines

Form: `[!] command1 [| command2 ...]`

- `|` connects commands, linking the left command's stdout to the right command's stdin
- Pipe connections are assigned before redirection operators (can be overridden by redirections)
- Non-background: waits for the last command to complete (implementation may wait for all)
- `!` prefix: logically negates the exit status

**Exit status table:**

| pipefail | `!` prefix | Exit Status |
|----------|------------|-------------|
| Disabled | No | Exit status of the last command |
| Disabled | Yes | 0 if last command is non-zero, otherwise 1 |
| Enabled | No | 0 if all commands are 0, otherwise the status of the last non-zero command |
| Enabled | Yes | 0 if any is non-zero, otherwise 1 |

**`!` and `(` rule:** When using `!` followed by a subshell command, one or more spaces are required between `!` and `(`. The behavior of `!(` is unspecified.

### 9.3 Lists

- **AND-OR lists:** One or more pipelines separated by `&&` and `||`
- **Lists:** One or more AND-OR lists separated by `;` and `&`
- `&&` and `||` have equal precedence and are left-associative
- AND-OR lists separated by `;` or `<newline>` are executed sequentially
- AND-OR lists separated by `&` are executed asynchronously

#### 9.3.1 Asynchronous AND-OR Lists

- AND-OR lists terminated with `&` are executed asynchronously in a subshell environment
- With job control enabled: becomes a background job with an assigned job number
- With job control disabled: subshell's stdin is redirected to `/dev/null` equivalent (can be overridden by explicit redirection)
- Interactive shells write job number and process ID to stderr: `"[%d] %d\n"`
- Exit status of an asynchronous list is 0
- `$!` expands to the process ID of the last asynchronous list

#### 9.3.2 Sequential AND-OR Lists

Form: `aolist1 [; aolist2 ...]`

- Expanded and executed in order
- Exit status is that of the last pipeline executed

#### 9.3.3 AND Lists

Form: `command1 [&& command2 ...]`

- Execute `command1`; if zero, execute `command2`, and so on
- Continues until a non-zero exit or no more commands
- Exit status is that of the last command executed

#### 9.3.4 OR Lists

Form: `command1 [|| command2 ...]`

- Execute `command1`; if non-zero, execute `command2`, and so on
- Continues until a zero exit or no more commands
- Exit status is that of the last command executed

### 9.4 Compound Commands

Each compound command begins with a reserved word or control operator and ends with a corresponding closing reserved word/operator. Redirections may follow on the same line, applying to all commands within.

#### 9.4.1 Grouping Commands

**Subshell:**

```sh
( compound-list )
```

- Executed in a subshell environment
- Variable assignments and environment changes do not persist after list completion
- For `((`, whitespace is required between the two `(` to avoid ambiguity with arithmetic evaluation

**Current environment:**

```sh
{ compound-list ; }
```

- Executed in the current process environment
- Variable assignments and environment changes persist
- `;` or `<newline>` required before `}`
- Exit status is that of `compound-list`

#### 9.4.2 The for Loop

```sh
for name [in [word ...]]
do
  compound-list
done
```

- With `in word ...`: assigns each word to `name` in turn and executes
- Without `in word ...`: equivalent to `"$@"`
- If expansion produces no items, `compound-list` is not executed
- Exit status: if at least one item, the status of the last executed `compound-list`; otherwise 0

#### 9.4.3 Case Conditional Construct

```sh
case word in
  [( ] pattern [| pattern ...]  ) compound-list ;; ]...
  [( ] pattern [| pattern ...]  ) compound-list [;; | ;&] ]
esac
```

- `word` is expanded with tilde, parameter, command substitution, arithmetic, and quote removal
- Patterns are compared from the top
- The first matching `compound-list` is executed
- `;;` terminator: no further clauses are executed
- `;&` terminator: the next clause's `compound-list` is also executed, continuing until `;;`
- No match: exit status 0
- Match: exit status of the last executed `compound-list`

#### 9.4.4 The if Conditional Construct

```sh
if compound-list
then compound-list
[elif compound-list
 then compound-list]...
[else compound-list]
fi
```

- Execute `if compound-list`; if zero, execute `then compound-list` and finish
- Otherwise, evaluate each `elif compound-list`; if zero, execute the corresponding `then compound-list`
- If none are zero, execute `else compound-list`
- Exit status: that of the executed `then`/`else` clause; 0 if nothing executed

#### 9.4.5 The while Loop

```sh
while compound-list-1
do compound-list-2
done
```

- Execute `compound-list-1`; repeat `compound-list-2` until non-zero
- Exit status: that of the last executed `compound-list-2`; 0 if never executed

#### 9.4.6 The until Loop

```sh
until compound-list-1
do compound-list-2
done
```

- Execute `compound-list-1`; repeat `compound-list-2` until zero (opposite of while)
- Exit status: that of the last executed `compound-list-2`; 0 if never executed

### 9.5 Function Definition Command

Form:

```sh
fname ( ) compound-command [io-redirect ...]
```

- `fname` must be a valid name and must NOT be the name of a special built-in
- At declaration time, no expansion of `compound-command` or `io-redirect` is performed
- At invocation: positional parameters are temporarily replaced with the function's arguments; `$#` is updated
- `$0` is not changed
- On function completion: positional parameters and `$#` are restored
- Early exit via the `return` built-in
- Exit status: 0 if declaration succeeds; on invocation, the status of the function's last command

---

## 10. Shell Grammar

Reference: Section 2.10

### 10.1 Lexical Conventions

Token classification rules (applied in order):

1. If it is an operator, use that operator's token identifier
2. If the string is digits only and the delimiter is `<` or `>`: `IO_NUMBER`
3. If the string starts with `{`, ends with `}`, is 3+ characters, and the delimiter is `<` or `>`: `IO_LOCATION` (optional)
4. Otherwise: `TOKEN`

`TOKEN` is further classified context-dependently into `WORD`, `NAME`, `ASSIGNMENT_WORD`, or a reserved word based on rules 1-9:

- **Rule 1:** TOKEN exactly matches a reserved word -> that reserved word token
- **Rule 2:** Filename for redirection (expand `word` to produce exactly 1 field)
- **Rule 3:** Here-document delimiter (quote removal only)
- **Rule 4:** Check for exact match with `esac` (`case` terminator)
- **Rule 5:** `NAME` within `for` (must be a valid name; otherwise `WORD`)
- **Rule 6a:** `case` only: check for exact match with `in`
- **Rule 6b:** `for` only: check for exact match with `in` or `do`
- **Rule 7a:** First word of a command: check for reserved words
- **Rule 7b:** Non-command-name words: if it contains `=` and the leading portion is a valid name -> `ASSIGNMENT_WORD`
- **Rule 8:** Function's `fname`: if not a reserved word and is a valid name -> `NAME`
- **Rule 9:** Function body: do not perform expansion or assignment

### 10.2 Grammar Rules (BNF)

```
%token WORD ASSIGNMENT_WORD NAME NEWLINE IO_NUMBER IO_LOCATION

/* Multi-character operators */
%token AND_IF OR_IF DSEMI SEMI_AND
/* '&&'   '||'  ';;'  ';&'  */

%token DLESS DGREAT LESSAND GREATAND LESSGREAT DLESSDASH
/* '<<'   '>>'   '<&'    '>&'    '<>'      '<<-'     */

%token CLOBBER
/* '>|' */

/* Reserved words */
%token If Then Else Elif Fi Do Done
%token Case Esac While Until For
%token Lbrace Rbrace Bang
%token In

%%
program          : linebreak complete_commands linebreak
                 | linebreak
                 ;
complete_commands: complete_commands newline_list complete_command
                 | complete_command
                 ;
complete_command : list separator_op
                 | list
                 ;
list             : list separator_op and_or
                 | and_or
                 ;
and_or           : pipeline
                 | and_or AND_IF linebreak pipeline
                 | and_or OR_IF  linebreak pipeline
                 ;
pipeline         : pipe_sequence
                 | Bang pipe_sequence
                 ;
pipe_sequence    : command
                 | pipe_sequence '|' linebreak command
                 ;
command          : simple_command
                 | compound_command
                 | compound_command redirect_list
                 | function_definition
                 ;
compound_command : brace_group
                 | subshell
                 | for_clause
                 | case_clause
                 | if_clause
                 | while_clause
                 | until_clause
                 ;
subshell         : '(' compound_list ')'
                 ;
compound_list    : linebreak term
                 | linebreak term separator
                 ;
term             : term separator and_or
                 | and_or
                 ;
for_clause       : For name do_group
                 | For name sequential_sep do_group
                 | For name linebreak in sequential_sep do_group
                 | For name linebreak in wordlist sequential_sep do_group
                 ;
name             : NAME   /* Rule 5 applies */
                 ;
in               : In     /* Rule 6 applies */
                 ;
wordlist         : wordlist WORD
                 | WORD
                 ;
case_clause      : Case WORD linebreak in linebreak case_list Esac
                 | Case WORD linebreak in linebreak case_list_ns Esac
                 | Case WORD linebreak in linebreak Esac
                 ;
case_list_ns     : case_list case_item_ns
                 | case_item_ns
                 ;
case_list        : case_list case_item
                 | case_item
                 ;
case_item_ns     : pattern_list ')' linebreak
                 | pattern_list ')' compound_list
                 ;
case_item        : pattern_list ')' linebreak DSEMI linebreak
                 | pattern_list ')' compound_list DSEMI linebreak
                 | pattern_list ')' linebreak SEMI_AND linebreak
                 | pattern_list ')' compound_list SEMI_AND linebreak
                 ;
pattern_list     : WORD                      /* Rule 4 applies */
                 | '(' WORD                  /* Rule 4 does not apply */
                 | pattern_list '|' WORD     /* Rule 4 does not apply */
                 ;
if_clause        : If compound_list Then compound_list else_part Fi
                 | If compound_list Then compound_list Fi
                 ;
else_part        : Elif compound_list Then compound_list
                 | Elif compound_list Then compound_list else_part
                 | Else compound_list
                 ;
while_clause     : While compound_list do_group
                 ;
until_clause     : Until compound_list do_group
                 ;
function_definition : fname '(' ')' linebreak function_body
                    ;
function_body    : compound_command                /* Rule 9 applies */
                 | compound_command redirect_list  /* Rule 9 applies */
                 ;
fname            : NAME   /* Rule 8 applies */
                 ;
brace_group      : Lbrace compound_list Rbrace
                 ;
do_group         : Do compound_list Done   /* Rule 6 applies */
                 ;
simple_command   : cmd_prefix cmd_word cmd_suffix
                 | cmd_prefix cmd_word
                 | cmd_prefix
                 | cmd_name cmd_suffix
                 | cmd_name
                 ;
cmd_name         : WORD   /* Rule 7a applies */
                 ;
cmd_word         : WORD   /* Rule 7b applies */
                 ;
cmd_prefix       : io_redirect
                 | cmd_prefix io_redirect
                 | ASSIGNMENT_WORD
                 | cmd_prefix ASSIGNMENT_WORD
                 ;
cmd_suffix       : io_redirect
                 | cmd_suffix io_redirect
                 | WORD
                 | cmd_suffix WORD
                 ;
redirect_list    : io_redirect
                 | redirect_list io_redirect
                 ;
io_redirect      : io_file
                 | IO_NUMBER io_file
                 | IO_LOCATION io_file      /* optional */
                 | io_here
                 | IO_NUMBER io_here
                 | IO_LOCATION io_here      /* optional */
                 ;
io_file          : '<'       filename
                 | LESSAND   filename
                 | '>'       filename
                 | GREATAND  filename
                 | DGREAT    filename
                 | LESSGREAT filename
                 | CLOBBER   filename
                 ;
filename         : WORD   /* Rule 2 applies */
                 ;
io_here          : DLESS     here_end
                 | DLESSDASH here_end
                 ;
here_end         : WORD   /* Rule 3 applies */
                 ;
newline_list     : NEWLINE
                 | newline_list NEWLINE
                 ;
linebreak        : newline_list
                 | /* empty */
                 ;
separator_op     : '&'
                 | ';'
                 ;
separator        : separator_op linebreak
                 | newline_list
                 ;
sequential_sep   : ';' linebreak
                 | newline_list
                 ;
```

---

## 11. Job Control

Reference: Section 2.11

- Enabled with `set -m` (default for interactive shells)
- When the shell has a controlling terminal, it sets its own process group ID as the foreground process group ID
- Jobs: asynchronous AND-OR lists form background jobs with assigned job numbers
- When a foreground job terminates or stops, the terminal's foreground process group is restored to the shell
- Each background job's process ID is recorded in the current shell execution environment
- `SIGTSTP`, `SIGTTIN`, `SIGTTOU` stopping a foreground job creates a suspended job

---

## 12. Signals and Error Handling

Reference: Section 2.12

- With job control disabled, commands in asynchronous AND-OR lists ignore `SIGINT` and `SIGQUIT` (`SIG_IGN`)
- Otherwise, commands have signal actions inherited from the shell (modifiable via `trap`)
- When a signal arrives while waiting for a foreground command, the trap is executed after command completion
- When a signal arrives during `wait`, `wait` returns immediately with exit status > 128, then the trap is executed
- When multiple signals are pending, the order of trap action execution is unspecified

---

## 13. Shell Execution Environment

Reference: Section 2.13

### Components of the shell execution environment

1. Open files inherited at shell invocation + open files controlled by `exec`
2. Working directory set by `cd`
3. File creation mask set by `umask`
4. File size limit set by `ulimit`
5. Current traps set by `trap`
6. Shell parameters from variable assignments or inherited from the environment at startup
7. Shell functions
8. Options set at startup or by `set`
9. Background jobs and their associated process IDs
10. Shell aliases

### Utility execution environment (non-special built-ins)

- Inherited from parent shell: open files (with redirection modifications), working directory, file creation mask
- For shell scripts: trapped signals are reset to default; ignored signals inherit the ignore action
- Variables with the `export` attribute are passed as environment variables

### Subshell environment

- Created as a duplicate of the shell environment
- Non-ignored traps are reset to default actions
- Interactive shell's subshells operate as non-interactive (except for `set -n` ignoring per `$-`'s `-i`)
- Subshell changes do not affect the parent shell's environment
- Command substitution, `()` groups, and asynchronous AND-OR lists execute in subshell environments
- Each command in a multi-command pipeline executes in a subshell (but may execute in the current environment as an extension)

---

## 14. Pattern Matching

Reference: Section 2.14

### 14.1 Patterns Matching a Single Character

- **Ordinary character:** matches itself
- **Quoted/escaped character:** matches as a literal character
- **`?` (unquoted):** matches any single character
- **`*` (unquoted):** used for multiple-character matching (see 14.2)
- **`[` (unquoted):** introduces a bracket expression (use `!` instead of `^` for negation)
- Backslash preserves the literal value of the next character outside bracket expressions

### 14.2 Patterns Matching Multiple Characters

- `*` matches any string including the null string
- Concatenation of single-character patterns is a valid pattern
- `*` combined with single-character patterns: `*` matches the longest possible string that allows the remaining pattern to match (greedy)

### 14.3 Patterns Used for Filename Expansion

- **Slash:** `*`, `?`, and bracket expressions never match `/`. Only literal `/` in the pattern
- **Leading dot:** `*`, `?`, and negated bracket expressions do not match a leading `.` (pattern must have an explicit `.` at the start or immediately after `/`)
- When wildcards are present: match against existing files/paths, sort by current locale collation order, and replace
- No match: use the pattern string as-is
- Directory components with `*`, `?`, `[`: read permission is required for that directory
- Non-final components without wildcards: search permission is required

---

## 15. Special Built-In Utilities

Reference: Section 2.15

**Key differences from regular built-ins:**

1. Special built-in errors may abort the shell (regular built-in errors do not)
2. Variable assignments preceding special built-ins affect the current execution environment (regular built-ins do not)

### break

```sh
break [n]
```

- Exit from the `n`-th enclosing `for`/`while`/`until` loop (default: 1)
- `n` must be a positive decimal integer
- If `n` exceeds the loop count: exit the outermost loop
- If no loop: behavior is unspecified
- **Lexical containment:** Only effective when the loop lexically contains `break` (same execution environment, same `compound-list`, not within a function definition)

### colon (:)

```sh
: [argument ...]
```

- Does nothing. Returns exit status 0
- Does not treat `--` specially

### continue

```sh
continue [n]
```

- Jump to the beginning of the `n`-th enclosing `for`/`while`/`until` loop (default: 1)
- Same lexical containment rules as `break`

### dot (.)

```sh
. file
```

- Tokenize, parse, and execute the contents of `file` in the current environment
- If `file` has no slashes: search via `PATH` (need not be executable)
- If no readable file found: non-interactive shell aborts, interactive shell shows error message
- Exit status: 0 if `file` is empty or all null; otherwise the status of the last command executed

### eval

```sh
eval [argument ...]
```

- Concatenate all arguments with spaces to form a command string
- Tokenize, parse, and execute in the current environment
- No arguments or only null: exit status 0
- Otherwise: exit status of the constructed command

### exec

```sh
exec [utility [argument ...]]
```

- **No operands:** Execute the redirections associated with `exec` in the current shell environment
  - FDs greater than 2 that are opened: whether they remain for the next utility invocation is unspecified
- **With `utility`:** Execute `utility` replacing the current shell (as a non-built-in)
  - On failure: non-interactive shell exits; interactive shell does not exit unless it's a subshell
  - Successful redirections remain in effect even after exec failure

### exit

```sh
exit [n]
```

- Exit from the current execution environment (subshell returns to parent)
- `n` 0-255: that value is the exit status
- `n` > 256 corresponding to signal termination: indicates termination by that signal (without performing signal-related actions)
- `n` omitted: uses `$?` (but from `trap` action end, uses the value before the trap)
- EXIT trap is executed before exit (but if `exit` is called within that trap, exit immediately)

### export

```sh
export name[=word] ...
export -p
```

- Set the `export` attribute on specified variables, including them in subsequent command environments
- `name=word` form also sets the value
- Declaration utility: words in `name=word` form are expanded in assignment context
- `-p`: output all exported variables in `export name=value` or `export name` format (suitable for re-input to the shell)

### readonly

```sh
readonly name[=word] ...
readonly -p
```

- Set the `readonly` attribute on specified variables
- readonly variables cannot have their values changed by `export`/`getopts`/`readonly`/`read`, nor deleted by `unset`
- Acts as a declaration utility
- `-p`: output all readonly variables in `readonly name=value` or `readonly name` format

### return

```sh
return [n]
```

- Stop execution of the current function or dot script
- Use outside a function or dot script has unspecified behavior
- Exit status: `n` (0-255). If omitted, `$?` (from `trap` action end, uses the value before the trap)

### set

```sh
set [-abCefhmnuvx] [-o option] [argument ...]
set [+abCefhmnuvx] [+o option] [argument ...]
set -- [argument ...]
set -o
set +o
```

**Options (`-` enables, `+` disables):**

| Option | Long name | Description |
|--------|-----------|-------------|
| `-a` | `allexport` | Set export attribute on all variable assignments |
| `-b` | `notify` | Asynchronously notify of background job completion (job control only) |
| `-C` | `noclobber` | Prevent `>` from overwriting files (`>|` overrides) |
| `-e` | `errexit` | Exit immediately on command failure (with exceptions) |
| `-f` | `noglob` | Disable pathname expansion |
| `-h` | — | Speed up PATH search (deprecated) |
| `-m` | `monitor` | Enable job control |
| `-n` | `noexec` | Read commands but do not execute (syntax checking) |
| `-u` | `nounset` | Error on expansion of unset variables |
| `-v` | `verbose` | Write shell input lines to stderr when read |
| `-x` | `xtrace` | Trace output before each command execution |
| — | `ignoreeof` | Prevent `Ctrl-D` from exiting interactive shell |
| — | `nolog` | Do not put function definitions in command history (deprecated) |
| — | `pipefail` | Derive pipeline exit status from all commands |
| — | `vi` | Use built-in vi editor |

**`-e` exceptions:**

- Individual command failures in multi-command pipelines are not subject
- Condition lists in `while`/`until`/`if`/`elif`, pipelines starting with `!`, and non-final commands in AND-OR lists are ignored

### shift

```sh
shift [n]
```

- Shift positional parameters by `n` (default: 1)
- `$1` becomes the previous `$(1+n)` value
- `n` must be 0 <= `n` <= `$#`
- `n=0`: no change
- `n > $#`: may be an error

### times

```sh
times
```

- Output cumulative user and system time for the shell and all child processes
- Format: `%dm%fs %dm%fs\n%dm%fs %dm%fs\n` (shell user/system, child user/system)

### trap

```sh
trap n [condition ...]
trap -p [condition ...]
trap [action condition ...]
```

- **`action` is `-`:** Reset each `condition` to default
- **`action` is null (`""`):** Ignore the specified `condition`
- **`action` is a string:** Execute `eval action` when `condition` occurs
- **Condition names:** `EXIT` (or `0`), signal names without `SIG` prefix: `HUP`, `INT`, `QUIT`, `TERM`, etc.
- **Trapping `SIGKILL`/`SIGSTOP`:** Unspecified result
- **EXIT condition:** Occurs when the shell exits normally (may also occur on abnormal signal exit)
- **EXIT trap environment:** Same as the environment immediately after the last command executed before the trap
- **Signals ignored at non-interactive shell startup:** Cannot be trapped or set (no error required)
- **On subshell entry:** Non-ignored traps are reset to default actions
- **No arguments:** List commands associated with conditions not in default state
- **`-p`:** List commands for all conditions (except SIGKILL/SIGSTOP)

### unset

```sh
unset [-fv] name ...
```

- `-v`: Treat as variable name; unset variable and remove from environment (readonly variables cannot be unset)
- `-f`: Treat as function name; unset function definition
- No `-v`/`-f`: Treat as variable (whether to unset a function if no such variable exists is unspecified)
- Unsetting a non-existent variable/function is not an error
- `VARIABLE=` does NOT unset `VARIABLE` (sets it to empty string)
- Special parameters (`$@`, `$*`, etc.) cannot be unset

---

## 16. Implementation Notes

Key considerations for building a POSIX-compliant shell in Rust:

### 16.1 Lexer/Parser Separation

Token recognition is context-dependent. The same string can be different token types depending on position (rules 1-9). The lexer needs feedback from the parser to correctly classify tokens.

### 16.2 Expansion Order is Critical

Tilde -> Parameter -> Command substitution -> Arithmetic -> Field splitting -> Pathname expansion -> Quote removal. Changing the order changes behavior.

### 16.3 Subshell vs Current Environment

`()` is a subshell, `{}` is the current environment. Each command in a pipeline is also a subshell (except as an implementation extension).

### 16.4 IFS Handling

Only bytes from expansion results are treated as delimiters. Literal bytes are always ordinary characters. The distinction between IFS whitespace and IFS non-whitespace characters is critical for correct field splitting.

### 16.5 Here-Document Quoting Rules

Whether `word` is quoted completely changes whether the content is expanded or not.

### 16.6 Special Built-In Distinction

Error behavior and variable assignment persistence differ between special and regular built-ins. This distinction must be tracked in the command execution path.

### 16.7 `$@` Behavior in Double-Quotes

Each positional parameter is preserved as an individual field. This is one of the most critical behaviors to implement correctly.

### 16.8 Pipeline `!` and `pipefail`

Exit status calculation involves a complex 4-pattern combination depending on whether `!` and `pipefail` are active.

### 16.9 `case` Fall-through with `;&`

`;&` causes execution to continue to the next clause's `compound-list` without pattern matching, until `;;` is reached.

### 16.10 Trap Reset on Subshell Entry

Caught (non-ignored) traps are reset to default actions when entering a subshell. Ignored signals remain ignored.

### 16.11 `set -e` (errexit) Exceptions

The `errexit` option has numerous exceptions that must be carefully tracked: condition lists, negated pipelines, AND-OR list non-final commands, etc.

### 16.12 Alias Expansion and Token Re-scanning

Alias values are re-scanned for tokens and further aliases. If an alias value ends with whitespace, the next token is also subject to alias expansion. Recursive expansion of the same alias must be prevented.

### 16.13 Arithmetic Expression Evaluation

Must support signed long integer arithmetic with C-style operators, decimal/octal/hexadecimal constants, and nested parameter expansion within expressions.

### 16.14 Signal Handling During Wait

Signals arriving during `wait` cause immediate return with status > 128, followed by trap execution. Signals during foreground command wait are deferred until command completion.
