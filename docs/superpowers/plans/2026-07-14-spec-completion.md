# コマンド補完定義ファイル 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `~/.config/yosh/completions/<cmd>.toml` に置いた TOML 定義から、コマンドごとのサブコマンド・フラグ・引数のタブ補完を生成する(設計: リポジトリルートの `completion.md`)。

**Architecture:** 新モジュール `src/interactive/spec_completion.rs` にスキーマ(serde + toml)、遅延ロードストア、純関数のマッチングエンジン、`exec` 子プロセス実行を実装する。`LineEditor::handle_tab_complete` のコマンド名補完とパス補完の間に「スペック補完」の分岐を1つ追加し、スペックが候補を出せないときは既存のパス補完へフォールバックする。

**Tech Stack:** Rust。`toml = "0.8"` と `serde`(derive)は既に依存に含まれる。テストは in-module ユニットテスト + `tests/interactive.rs`(MockTerminal)+ `tests/pty_interactive.rs`(expectrl)。

## Global Constraints

- 警告メッセージは `yosh: ` プレフィックスで stderr に出す(例: `yosh: completion: git.toml: <error>`)。
- `exec` のタイムアウトは **500 ms**。タイムアウト・非ゼロ終了・空出力は「候補ゼロ」。
- フォールバック規則: 解決されたソースが候補ゼロで、かつ `type = "none"` でなければ、既存のパス補完へフォールバック(`try_complete` が `None` を返す)。`type = "none"` は空の候補リストを**返して**フォールバックを抑止する。
- コミットメッセージ末尾に必ず以下を付ける:
  ```
  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE
  ```
- `cargo build` は 1〜3 分かかることがある。テスト実行はタイムアウトを長め(300000 ms 以上)に設定すること。
- `cargo test --workspace` / `cargo build --workspace` は使わない(wasm クレートのホストビルドが失敗する)。

## ファイル構成

| ファイル | 役割 |
| --- | --- |
| `src/interactive/spec_completion.rs`(新規) | スキーマ型、パース+バリデーション、`SpecStore`、`resolve`、`run_exec`、`try_complete`、`command_words`。ユニットテスト同居 |
| `src/interactive/mod.rs`(変更) | モジュール登録、`InteractiveShell` に `spec_store` フィールド追加、呼び出し配線 |
| `src/interactive/line_editor.rs`(変更) | `read_line_with_completion` 系 3 関数に `specs: &mut SpecStore` パラメータ追加、`handle_tab_complete` に分岐追加 |
| `tests/interactive.rs`(変更) | 既存 9 箇所の呼び出し更新 + スペック補完の統合テスト追加 |
| `tests/pty_interactive.rs`(変更) | E2E テスト 1 件追加 |

---

### Task 1: スキーマ型と TOML パース

**Files:**
- Create: `src/interactive/spec_completion.rs`
- Modify: `src/interactive/mod.rs:3`(モジュール宣言の追加)

**Interfaces:**
- Produces(後続タスクが依存):
  - `pub enum CandidateSource { Builtin(BuiltinType), Values(Vec<String>), Exec(String) }`
  - `pub enum BuiltinType { File, Directory, Command, None }`
  - `pub struct FlagSpec { pub names: Vec<String>, pub value: Option<CandidateSource> }`
  - `pub struct CompletionSpec { pub args: Vec<CandidateSource>, pub flags: Vec<FlagSpec>, pub subcommands: Vec<Subcommand> }`
  - `pub struct Subcommand { pub name: String, pub args: ..., pub flags: ..., pub subcommands: Vec<Subcommand> }`
  - `impl CompletionSpec { pub fn parse(text: &str) -> Result<CompletionSpec, String>; pub fn level(&self) -> Level<'_> }`
  - `impl Subcommand { pub fn level(&self) -> Level<'_> }`
  - `pub struct Level<'a> { pub args: &'a [CandidateSource], pub flags: &'a [FlagSpec], pub subcommands: &'a [Subcommand] }`

- [ ] **Step 1: 失敗するテストを書く**

`src/interactive/spec_completion.rs` を新規作成し、まずファイル冒頭のドキュメントコメントとテストモジュールだけを書く:

```rust
//! Spec-based tab completion: per-command TOML definition files.
//!
//! Users drop `<command>.toml` files into `~/.config/yosh/completions/`
//! to define subcommand / flag / argument completion for any command.
//! See `completion.md` at the repository root for the full design.

#[cfg(test)]
mod tests {
    use super::*;

    // ── CompletionSpec::parse ────────────────────────────────────────

    const GIT_SPEC: &str = r#"
[[args]]
type = "file"

[[flags]]
names = ["-C"]
value = { type = "directory" }

[[flags]]
names = ["--no-pager"]

[[subcommands]]
name = "checkout"

[[subcommands.flags]]
names = ["-b"]
value = { type = "none" }

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
"#;

    #[test]
    fn parse_full_example() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        assert_eq!(spec.args, vec![CandidateSource::Builtin(BuiltinType::File)]);
        assert_eq!(spec.flags.len(), 2);
        assert_eq!(spec.flags[0].names, vec!["-C"]);
        assert_eq!(
            spec.flags[0].value,
            Some(CandidateSource::Builtin(BuiltinType::Directory))
        );
        assert_eq!(spec.flags[1].names, vec!["--no-pager"]);
        assert_eq!(spec.flags[1].value, None);
        assert_eq!(spec.subcommands.len(), 2);
        let checkout = &spec.subcommands[0];
        assert_eq!(checkout.name, "checkout");
        assert_eq!(
            checkout.args,
            vec![CandidateSource::Exec(
                "git branch --format='%(refname:short)'".to_string()
            )]
        );
        let remote = &spec.subcommands[1];
        assert_eq!(remote.subcommands.len(), 2);
        assert_eq!(remote.subcommands[1].name, "remove");
        assert_eq!(
            remote.subcommands[1].args,
            vec![CandidateSource::Exec("git remote".to_string())]
        );
    }

    #[test]
    fn parse_empty_file_is_valid() {
        let spec = CompletionSpec::parse("").unwrap();
        assert!(spec.args.is_empty());
        assert!(spec.flags.is_empty());
        assert!(spec.subcommands.is_empty());
    }

    #[test]
    fn parse_values_source() {
        let spec = CompletionSpec::parse("[[args]]\nvalues = [\"json\", \"yaml\"]\n").unwrap();
        assert_eq!(
            spec.args,
            vec![CandidateSource::Values(vec![
                "json".to_string(),
                "yaml".to_string()
            ])]
        );
    }

    #[test]
    fn parse_rejects_source_with_no_keys() {
        let err = CompletionSpec::parse("[[args]]\n").unwrap_err();
        assert!(err.contains("exactly one"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_source_with_two_keys() {
        let err =
            CompletionSpec::parse("[[args]]\ntype = \"file\"\nvalues = [\"a\"]\n").unwrap_err();
        assert!(err.contains("exactly one"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_unknown_builtin_type() {
        let err = CompletionSpec::parse("[[args]]\ntype = \"folder\"\n").unwrap_err();
        assert!(err.contains("folder"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_duplicate_subcommand_names() {
        let text = "[[subcommands]]\nname = \"a\"\n[[subcommands]]\nname = \"a\"\n";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(err.contains("duplicate subcommand"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_duplicate_flag_spelling() {
        let text = "[[flags]]\nnames = [\"-v\"]\n[[flags]]\nnames = [\"-v\", \"--verbose\"]\n";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(err.contains("duplicate flag"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_empty_flag_names() {
        let err = CompletionSpec::parse("[[flags]]\nnames = []\n").unwrap_err();
        assert!(err.contains("names"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_unknown_field() {
        // Typos in the file must surface as a parse error (and thus a
        // one-time warning), not silently do nothing.
        let err = CompletionSpec::parse("[[flags]]\nname = [\"-v\"]\n").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_rejects_duplicates_in_nested_level() {
        let text = "\
[[subcommands]]
name = \"remote\"
[[subcommands.subcommands]]
name = \"add\"
[[subcommands.subcommands]]
name = \"add\"
";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(err.contains("duplicate subcommand"), "unexpected error: {err}");
    }
}
```

`src/interactive/mod.rs` のモジュール宣言ブロック(アルファベット順)に追加:

```rust
pub mod spec_completion;
```

(`pub mod selector;` の直後、`pub mod terminal;` の前に入れる。)

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -20`
Expected: コンパイルエラー(`CompletionSpec` 未定義)

- [ ] **Step 3: 最小実装を書く**

`src/interactive/spec_completion.rs` のテストモジュールの上に以下を追加:

```rust
use serde::Deserialize;

// ── Schema ──────────────────────────────────────────────────────────

/// How to produce candidates. Deserialized from a TOML table that must
/// contain exactly one of `type` / `values` / `exec`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawSource")]
pub enum CandidateSource {
    /// A built-in generator (`type = "file"` etc.).
    Builtin(BuiltinType),
    /// A static candidate list (`values = [...]`).
    Values(Vec<String>),
    /// A shell command whose stdout lines become candidates (`exec = "..."`).
    Exec(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinType {
    File,
    Directory,
    Command,
    /// No candidates, and no fallback to path completion.
    None,
}

/// Raw deserialization target for [`CandidateSource`]; validated in the
/// `TryFrom` conversion so the exactly-one-key rule is a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    #[serde(rename = "type")]
    builtin: Option<String>,
    values: Option<Vec<String>>,
    exec: Option<String>,
}

impl TryFrom<RawSource> for CandidateSource {
    type Error = String;

    fn try_from(raw: RawSource) -> Result<Self, String> {
        let keys =
            raw.builtin.is_some() as u8 + raw.values.is_some() as u8 + raw.exec.is_some() as u8;
        if keys != 1 {
            return Err(
                "candidate source must have exactly one of `type`, `values`, `exec`".to_string(),
            );
        }
        if let Some(t) = raw.builtin {
            let builtin = match t.as_str() {
                "file" => BuiltinType::File,
                "directory" => BuiltinType::Directory,
                "command" => BuiltinType::Command,
                "none" => BuiltinType::None,
                other => return Err(format!("unknown builtin type `{other}`")),
            };
            Ok(CandidateSource::Builtin(builtin))
        } else if let Some(values) = raw.values {
            Ok(CandidateSource::Values(values))
        } else {
            Ok(CandidateSource::Exec(raw.exec.expect("exec key present")))
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagSpec {
    /// All spellings of this flag, e.g. `["-m", "--message"]`.
    pub names: Vec<String>,
    /// Present iff the flag takes a value, completed from this source.
    #[serde(default)]
    pub value: Option<CandidateSource>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionSpec {
    #[serde(default)]
    pub args: Vec<CandidateSource>,
    #[serde(default)]
    pub flags: Vec<FlagSpec>,
    #[serde(default)]
    pub subcommands: Vec<Subcommand>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subcommand {
    pub name: String,
    #[serde(default)]
    pub args: Vec<CandidateSource>,
    #[serde(default)]
    pub flags: Vec<FlagSpec>,
    #[serde(default)]
    pub subcommands: Vec<Subcommand>,
}

/// A borrowed view of one level of the spec tree, shared between the
/// top level (`CompletionSpec`) and nested `Subcommand`s.
#[derive(Clone, Copy)]
pub struct Level<'a> {
    pub args: &'a [CandidateSource],
    pub flags: &'a [FlagSpec],
    pub subcommands: &'a [Subcommand],
}

impl CompletionSpec {
    pub fn level(&self) -> Level<'_> {
        Level {
            args: &self.args,
            flags: &self.flags,
            subcommands: &self.subcommands,
        }
    }

    /// Parse and validate a completion spec from TOML text.
    pub fn parse(text: &str) -> Result<CompletionSpec, String> {
        let spec: CompletionSpec = toml::from_str(text).map_err(|e| e.to_string())?;
        validate_level(spec.level())?;
        Ok(spec)
    }
}

impl Subcommand {
    pub fn level(&self) -> Level<'_> {
        Level {
            args: &self.args,
            flags: &self.flags,
            subcommands: &self.subcommands,
        }
    }
}

/// Reject duplicate subcommand names and duplicate flag spellings within
/// one level, and flags with an empty `names` list. Recurses into
/// subcommands.
fn validate_level(level: Level<'_>) -> Result<(), String> {
    let mut sub_names = std::collections::HashSet::new();
    for sub in level.subcommands {
        if !sub_names.insert(sub.name.as_str()) {
            return Err(format!("duplicate subcommand `{}`", sub.name));
        }
        validate_level(sub.level())?;
    }
    let mut spellings = std::collections::HashSet::new();
    for flag in level.flags {
        if flag.names.is_empty() {
            return Err("flag with empty `names`".to_string());
        }
        for name in &flag.names {
            if !spellings.insert(name.as_str()) {
                return Err(format!("duplicate flag `{name}`"));
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: 12 tests passed

- [ ] **Step 5: fmt してコミット**

```bash
cargo fmt
git add src/interactive/spec_completion.rs src/interactive/mod.rs
git commit -m "feat(completion): add TOML schema for spec-based completion

Task 1 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 2: SpecStore(遅延ロード + キャッシュ)

**Files:**
- Modify: `src/interactive/spec_completion.rs`

**Interfaces:**
- Consumes: `CompletionSpec::parse`(Task 1)
- Produces:
  - `pub struct SpecStore`
  - `impl SpecStore { pub fn new(dir: PathBuf) -> Self; pub fn from_home(home: &str) -> Self; pub fn get(&mut self, command: &str) -> Option<&CompletionSpec> }`
  - `get` はコマンド名の最終パス要素で `<dir>/<name>.toml` を初回のみ読む。存在しない・パース失敗は `None` としてネガティブキャッシュ。パース失敗時のみ `yosh: completion: <name>.toml: <error>` を stderr に 1 回出す(ロードは 1 回しか走らないので自然に warn-once になる)

- [ ] **Step 1: 失敗するテストを書く**

`spec_completion.rs` のテストモジュールに追加:

```rust
    // ── SpecStore ────────────────────────────────────────────────────

    use std::path::PathBuf;

    fn store_with(specs: &[(&str, &str)]) -> (tempfile::TempDir, SpecStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        for (name, text) in specs {
            std::fs::write(tmp.path().join(format!("{name}.toml")), text).unwrap();
        }
        let store = SpecStore::new(tmp.path().to_path_buf());
        (tmp, store)
    }

    #[test]
    fn store_loads_spec_by_command_name() {
        let (_tmp, mut store) = store_with(&[("mytool", "[[args]]\nvalues = [\"a\"]\n")]);
        let spec = store.get("mytool").unwrap();
        assert_eq!(spec.args.len(), 1);
    }

    #[test]
    fn store_resolves_final_path_component() {
        let (_tmp, mut store) = store_with(&[("git", "[[subcommands]]\nname = \"log\"\n")]);
        assert!(store.get("/usr/bin/git").is_some());
    }

    #[test]
    fn store_missing_file_returns_none() {
        let (_tmp, mut store) = store_with(&[]);
        assert!(store.get("nosuch").is_none());
    }

    #[test]
    fn store_caches_first_load() {
        let (tmp, mut store) = store_with(&[("mytool", "[[args]]\nvalues = [\"a\"]\n")]);
        assert!(store.get("mytool").is_some());
        // Overwrite with garbage; the cached parse must still be served.
        std::fs::write(tmp.path().join("mytool.toml"), "not [ valid").unwrap();
        assert!(store.get("mytool").is_some());
    }

    #[test]
    fn store_parse_error_returns_none() {
        let (_tmp, mut store) = store_with(&[("bad", "not [ valid toml")]);
        assert!(store.get("bad").is_none());
        // Negative-cached: repeat lookups stay None.
        assert!(store.get("bad").is_none());
    }

    #[test]
    fn store_empty_command_returns_none() {
        let (_tmp, mut store) = store_with(&[]);
        assert!(store.get("").is_none());
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: コンパイルエラー(`SpecStore` 未定義)

- [ ] **Step 3: 実装を書く**

`spec_completion.rs` の `validate_level` の下に追加:

```rust
// ── SpecStore: lazy per-command loading ─────────────────────────────

/// Lazy loader and per-session cache for completion specs.
///
/// Specs live at `<dir>/<command>.toml`. Each command name is looked up
/// on disk at most once per session: both missing files and parse
/// failures are cached as `None`, which also makes the parse-error
/// warning naturally print only once.
pub struct SpecStore {
    dir: std::path::PathBuf,
    cache: std::collections::HashMap<String, Option<CompletionSpec>>,
}

impl SpecStore {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Store rooted at the standard location under `home`
    /// (`~/.config/yosh/completions`).
    pub fn from_home(home: &str) -> Self {
        Self::new(std::path::PathBuf::from(home).join(".config/yosh/completions"))
    }

    /// Look up the spec for `command`. Only the final path component is
    /// used: `/usr/bin/git` and `git` both resolve to `git.toml`.
    pub fn get(&mut self, command: &str) -> Option<&CompletionSpec> {
        let name = command.rsplit('/').next().unwrap_or(command);
        if name.is_empty() || name == "." || name == ".." {
            return None;
        }
        if !self.cache.contains_key(name) {
            let loaded = self.load(name);
            self.cache.insert(name.to_string(), loaded);
        }
        self.cache.get(name).and_then(|entry| entry.as_ref())
    }

    fn load(&self, name: &str) -> Option<CompletionSpec> {
        let path = self.dir.join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&path).ok()?;
        match CompletionSpec::parse(&text) {
            Ok(spec) => Some(spec),
            Err(err) => {
                eprintln!("yosh: completion: {name}.toml: {err}");
                None
            }
        }
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: 18 tests passed

- [ ] **Step 5: fmt してコミット**

```bash
cargo fmt
git add src/interactive/spec_completion.rs
git commit -m "feat(completion): add SpecStore with lazy per-command loading

Task 2 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 3: マッチングエンジン(`resolve` + `command_words`)

**Files:**
- Modify: `src/interactive/spec_completion.rs`

**Interfaces:**
- Consumes: Task 1 の型(`CompletionSpec`, `Level`, `FlagSpec`, `CandidateSource`)
- Produces:
  - `pub enum Resolution<'a> { FlagValue(&'a CandidateSource), FlagNames(Vec<String>), Positional { subcommands: Vec<String>, source: Option<&'a CandidateSource> } }`
  - `pub fn resolve<'a>(spec: &'a CompletionSpec, prior_words: &[String], current_word: &str) -> (Resolution<'a>, String)` — 戻り値の `String` は `keep_prefix`(`--flag=` 形式のとき `=` までをそのまま残す部分。それ以外は空)
  - `pub fn command_words(buf: &str, word_start: usize) -> Option<(String, Vec<String>)>` — 現在のパイプラインセグメントを (コマンド名, カーソルより前の引数ワード列) に分解。カーソルワード自体がコマンド名のときは `None`

- [ ] **Step 1: 失敗するテストを書く**

テストモジュールに追加(`GIT_SPEC` は Task 1 のものを再利用):

```rust
    // ── command_words ────────────────────────────────────────────────

    #[test]
    fn words_simple() {
        // "git checkout ma|" — word_start = 13
        let (cmd, args) = command_words("git checkout ma", 13).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["checkout"]);
    }

    #[test]
    fn words_cursor_on_command_is_none() {
        assert!(command_words("gi", 0).is_none());
        assert!(command_words("", 0).is_none());
    }

    #[test]
    fn words_pipeline_segment_only() {
        // "cat f | git checkout ma|" — word_start = 21
        let (cmd, args) = command_words("cat f | git checkout ma", 21).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["checkout"]);
    }

    #[test]
    fn words_after_semicolon() {
        // "echo a; git lo|" — word_start = 12
        let (cmd, args) = command_words("echo a; git lo", 12).unwrap();
        assert_eq!(cmd, "git");
        assert!(args.is_empty());
    }

    #[test]
    fn words_strips_quotes() {
        // "git commit -m 'a b' |" — word_start = 20
        let (cmd, args) = command_words("git commit -m 'a b' ", 20).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["commit", "-m", "a b"]);
    }

    #[test]
    fn words_skips_bang_prefix() {
        // "! git lo|" — word_start = 6
        let (cmd, args) = command_words("! git lo", 6).unwrap();
        assert_eq!(cmd, "git");
        assert!(args.is_empty());
    }

    // ── resolve ──────────────────────────────────────────────────────

    fn as_strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn resolve_top_level_positional() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, keep) = resolve(&spec, &[], "");
        match res {
            Resolution::Positional { subcommands, source } => {
                assert_eq!(subcommands, vec!["checkout", "remote"]);
                assert_eq!(source, Some(&CandidateSource::Builtin(BuiltinType::File)));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "");
    }

    #[test]
    fn resolve_descends_into_subcommand() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["checkout"]), "ma");
        match res {
            Resolution::Positional { subcommands, source } => {
                assert!(subcommands.is_empty());
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_nested_subcommand() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["remote", "remove"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec("git remote".to_string()))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_value_flag_completes_next_word() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["-C"]), "");
        match res {
            Resolution::FlagValue(source) => {
                assert_eq!(source, &CandidateSource::Builtin(BuiltinType::Directory));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_value_flag_consumes_its_value() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        // "-C /tmp checkout <cursor>" — /tmp is the value of -C, then descend.
        let (res, _) = resolve(&spec, &as_strings(&["-C", "/tmp", "checkout"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_boolean_flag_is_consumed() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["--no-pager"]), "");
        assert!(matches!(res, Resolution::Positional { .. }));
    }

    #[test]
    fn resolve_unknown_dash_word_is_consumed_as_boolean() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["-q", "checkout"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_dash_word_completes_flag_names() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, keep) = resolve(&spec, &[], "--n");
        match res {
            Resolution::FlagNames(names) => {
                assert_eq!(names, vec!["-C", "--no-pager"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "");
    }

    #[test]
    fn resolve_flag_eq_value_form() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\", \"yaml\"] }
";
        let spec = CompletionSpec::parse(text).unwrap();
        let (res, keep) = resolve(&spec, &[], "--format=j");
        match res {
            Resolution::FlagValue(source) => {
                assert_eq!(
                    source,
                    &CandidateSource::Values(vec!["json".to_string(), "yaml".to_string()])
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "--format=");
    }

    #[test]
    fn resolve_flag_eq_value_in_prior_word_is_self_contained() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\"] }
[[args]]
values = [\"target\"]
";
        let spec = CompletionSpec::parse(text).unwrap();
        // "--format=json <cursor>" must NOT treat the cursor word as the
        // flag's value.
        let (res, _) = resolve(&spec, &as_strings(&["--format=json"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Values(vec!["target".to_string()]))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_positional_index_and_repeat_last() {
        let text = "\
[[args]]
values = [\"first\"]
[[args]]
values = [\"second\"]
";
        let spec = CompletionSpec::parse(text).unwrap();
        let expect_values = |res: Resolution<'_>, want: &str| match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Values(vec![want.to_string()]))
                );
            }
            other => panic!("unexpected: {other:?}"),
        };
        let (res, _) = resolve(&spec, &[], "");
        expect_values(res, "first");
        let (res, _) = resolve(&spec, &as_strings(&["x"]), "");
        expect_values(res, "second");
        // Last entry repeats for further positionals.
        let (res, _) = resolve(&spec, &as_strings(&["x", "y"]), "");
        expect_values(res, "second");
    }

    #[test]
    fn resolve_no_args_declared_has_no_source() {
        let spec = CompletionSpec::parse("[[subcommands]]\nname = \"sub\"\n").unwrap();
        let (res, _) = resolve(&spec, &[], "");
        match res {
            Resolution::Positional { subcommands, source } => {
                assert_eq!(subcommands, vec!["sub"]);
                assert_eq!(source, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
```

`Resolution` にテストで `{other:?}` を使うので `#[derive(Debug)]` を忘れないこと。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: コンパイルエラー(`resolve` / `command_words` / `Resolution` 未定義)

- [ ] **Step 3: 実装を書く**

`SpecStore` の下に追加:

```rust
// ── Matching engine ─────────────────────────────────────────────────

/// What the word under the cursor should be completed as.
#[derive(Debug)]
pub enum Resolution<'a> {
    /// Complete a flag's value from this source.
    FlagValue(&'a CandidateSource),
    /// The current word starts with `-`: complete flag spellings.
    FlagNames(Vec<String>),
    /// Positional: subcommand names at the current level, plus the
    /// candidate source for the current positional index (if any).
    Positional {
        subcommands: Vec<String>,
        source: Option<&'a CandidateSource>,
    },
}

/// Walk `prior_words` (the words between the command name and the
/// cursor) down the spec tree and decide what `current_word` completes.
///
/// Returns the resolution plus `keep_prefix`: the leading part of the
/// current word to re-insert verbatim (non-empty only for `--flag=value`
/// forms, where it is `--flag=`).
pub fn resolve<'a>(
    spec: &'a CompletionSpec,
    prior_words: &[String],
    current_word: &str,
) -> (Resolution<'a>, String) {
    let mut level = spec.level();
    let mut positional_index = 0usize;
    let mut pending_value_flag: Option<&FlagSpec> = None;

    for word in prior_words {
        if pending_value_flag.take().is_some() {
            continue; // consumed as the pending flag's value
        }
        if let Some(sub) = level.subcommands.iter().find(|s| s.name == *word) {
            level = sub.level();
            positional_index = 0;
            continue;
        }
        if word.starts_with('-') {
            // `--flag=value` is self-contained; a bare value-taking flag
            // makes the NEXT word its value. Unknown flags are consumed
            // as booleans.
            let name = word.split('=').next().unwrap_or(word);
            if let Some(flag) = find_flag(level, name)
                && flag.value.is_some()
                && !word.contains('=')
            {
                pending_value_flag = Some(flag);
            }
            continue;
        }
        positional_index += 1;
    }

    if let Some(flag) = pending_value_flag {
        let source = flag.value.as_ref().expect("pending flag takes a value");
        return (Resolution::FlagValue(source), String::new());
    }

    if current_word.starts_with('-') {
        if let Some(eq) = current_word.find('=') {
            let name = &current_word[..eq];
            if let Some(flag) = find_flag(level, name)
                && let Some(source) = flag.value.as_ref()
            {
                return (
                    Resolution::FlagValue(source),
                    current_word[..=eq].to_string(),
                );
            }
        }
        let names: Vec<String> = level
            .flags
            .iter()
            .flat_map(|f| f.names.iter().cloned())
            .collect();
        return (Resolution::FlagNames(names), String::new());
    }

    let subcommands: Vec<String> = level.subcommands.iter().map(|s| s.name.clone()).collect();
    let source = if level.args.is_empty() {
        None
    } else {
        Some(&level.args[positional_index.min(level.args.len() - 1)])
    };
    (Resolution::Positional { subcommands, source }, String::new())
}

fn find_flag<'a>(level: Level<'a>, name: &str) -> Option<&'a FlagSpec> {
    level
        .flags
        .iter()
        .find(|f| f.names.iter().any(|n| n == name))
}

/// Split the current pipeline segment of `buf[..word_start]` into the
/// command word and the argument words before the cursor. Quote
/// characters wrapping a word are stripped; a leading `!` word is
/// skipped. Returns `None` when the cursor word is itself the command.
pub fn command_words(buf: &str, word_start: usize) -> Option<(String, Vec<String>)> {
    let bytes = buf.as_bytes();
    let end = word_start.min(buf.len());
    let mut seg_start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    for (i, &ch) in bytes.iter().enumerate().take(end) {
        match ch {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'|' | b';' | b'&' | b'(' if !in_single && !in_double => seg_start = i + 1,
            _ => {}
        }
    }
    let mut words: Vec<String> = buf[seg_start..end]
        .split_whitespace()
        .map(|w| w.trim_matches(|c| c == '\'' || c == '"').to_string())
        .collect();
    if words.first().is_some_and(|w| w == "!") {
        words.remove(0);
    }
    if words.is_empty() {
        return None;
    }
    let cmd = words.remove(0);
    Some((cmd, words))
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: 36 tests passed

- [ ] **Step 5: fmt してコミット**

```bash
cargo fmt
git add src/interactive/spec_completion.rs
git commit -m "feat(completion): add spec matching engine (resolve/command_words)

Task 3 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 4: `exec` ランナー(500ms タイムアウト付き子プロセス)

**Files:**
- Modify: `src/interactive/spec_completion.rs`

**Interfaces:**
- Produces:
  - `pub const EXEC_TIMEOUT: Duration`(500ms)
  - `pub fn run_exec(cmd: &str, timeout: Duration) -> Vec<String>` — `sh -c <cmd>` を実行し stdout を行分割(空行除去)。タイムアウト時は kill して空 Vec、非ゼロ終了も空 Vec

- [ ] **Step 1: 失敗するテストを書く**

テストモジュールに追加:

```rust
    // ── run_exec ─────────────────────────────────────────────────────

    use std::time::Duration;

    #[test]
    fn exec_splits_stdout_lines_and_drops_empties() {
        let lines = run_exec("printf 'alpha\\n\\nbeta\\n'", Duration::from_secs(5));
        assert_eq!(lines, vec!["alpha", "beta"]);
    }

    #[test]
    fn exec_nonzero_exit_yields_no_candidates() {
        let lines = run_exec("echo out; exit 1", Duration::from_secs(5));
        assert!(lines.is_empty());
    }

    #[test]
    fn exec_timeout_kills_child_and_yields_no_candidates() {
        let start = std::time::Instant::now();
        let lines = run_exec("sleep 5", Duration::from_millis(100));
        assert!(lines.is_empty());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout did not fire: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn exec_inherits_cwd() {
        // The child runs in the shell's cwd, so `pwd` output is non-empty
        // and matches the current dir.
        let cwd = std::env::current_dir().unwrap();
        let lines = run_exec("pwd", Duration::from_secs(5));
        assert_eq!(lines, vec![cwd.to_string_lossy().to_string()]);
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: コンパイルエラー(`run_exec` 未定義)

- [ ] **Step 3: 実装を書く**

`command_words` の下に追加:

```rust
// ── exec runner ─────────────────────────────────────────────────────

/// Budget for `exec` candidate commands. Chosen to keep Tab latency
/// bounded; see completion.md.
pub const EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Run `sh -c <cmd>` and return its stdout lines (empty lines dropped).
/// A timeout kills the child; timeouts and non-zero exits both yield an
/// empty Vec. Runs as a child process so completion can never mutate
/// shell state.
pub fn run_exec(cmd: &str, timeout: std::time::Duration) -> Vec<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Vec::new(),
    };

    // Drain stdout on a helper thread so a pipe-buffer-filling child can
    // never deadlock the timeout loop below.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };

    let out = reader.join().unwrap_or_default();
    match status {
        Some(status) if status.success() => out
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: 40 tests passed

- [ ] **Step 5: fmt してコミット**

```bash
cargo fmt
git add src/interactive/spec_completion.rs
git commit -m "feat(completion): add exec candidate runner with 500ms timeout

Task 4 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 5: 候補生成(`try_complete`)

**Files:**
- Modify: `src/interactive/spec_completion.rs`

**Interfaces:**
- Consumes: Task 1〜4 の全 API、`super::completion::{complete, longest_common_prefix, CompletionContext}`、`super::command_completion::CommandCompletionContext`
- Produces:
  - `pub struct SpecCompletion { pub candidates: Vec<String>, pub common_prefix: String, pub keep_prefix: String }`
  - `pub fn try_complete(buf: &str, pos: usize, word_start: usize, word: &str, store: &mut SpecStore, ctx: &CompletionContext, cmd_ctx: &mut CommandCompletionContext<'_>) -> Option<SpecCompletion>`
  - 戻り値の意味: `None` = スペックなし/指針なし → 呼び出し側は既存のパス補完へ。`Some`(空 candidates 含む)= この結果を最終とする(空 = `type = "none"` の抑止)

- [ ] **Step 1: 失敗するテストを書く**

テストモジュールに追加:

```rust
    // ── try_complete ─────────────────────────────────────────────────

    use crate::env::aliases::AliasStore;
    use crate::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
    use crate::interactive::completion::CompletionContext;

    /// Run try_complete over `line` (cursor at end) against a single spec
    /// named `cmd`, using a scratch cwd.
    fn spec_complete(
        cmd: &str,
        spec_text: &str,
        line: &str,
        cwd: &str,
    ) -> Option<SpecCompletion> {
        let (_tmp, mut store) = store_with(&[(cmd, spec_text)]);
        let ctx = CompletionContext {
            cwd: cwd.to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let aliases = AliasStore::default();
        let mut completer = CommandCompleter::new();
        let mut cmd_ctx = CommandCompletionContext {
            completer: &mut completer,
            path: "",
            builtins: &["echo", "exit", "cd"],
            aliases: &aliases,
        };
        let pos = line.len();
        let (word_start, word) = crate::interactive::completion::extract_completion_word(line, pos);
        let word = word.to_string();
        try_complete(line, pos, word_start, &word, &mut store, &ctx, &mut cmd_ctx)
    }

    #[test]
    fn complete_values_filters_by_prefix() {
        let result = spec_complete(
            "mytool",
            "[[args]]\nvalues = [\"alpha\", \"omega\", \"alto\"]\n",
            "mytool al",
            "/",
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["alpha", "alto"]);
        assert_eq!(result.common_prefix, "al");
        assert_eq!(result.keep_prefix, "");
    }

    #[test]
    fn complete_no_spec_returns_none() {
        let result = spec_complete("mytool", "[[args]]\nvalues = [\"a\"]\n", "other x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_none_source_suppresses_fallback() {
        let result = spec_complete("mytool", "[[args]]\ntype = \"none\"\n", "mytool x", "/")
            .unwrap();
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn complete_zero_matches_falls_back() {
        // Candidates exist but none match the word → fallback (None).
        let result = spec_complete(
            "mytool",
            "[[args]]\nvalues = [\"alpha\"]\n",
            "mytool zz",
            "/",
        );
        assert!(result.is_none());
    }

    #[test]
    fn complete_subcommand_names_merge_with_values() {
        let text = "\
[[args]]
values = [\"deep\"]
[[subcommands]]
name = \"deploy\"
";
        let result = spec_complete("mytool", text, "mytool de", "/").unwrap();
        assert_eq!(result.candidates, vec!["deep", "deploy"]);
    }

    #[test]
    fn complete_flag_names() {
        let text = "\
[[flags]]
names = [\"--verbose\", \"-v\"]
[[flags]]
names = [\"--version\"]
";
        let result = spec_complete("mytool", text, "mytool --ver", "/").unwrap();
        assert_eq!(result.candidates, vec!["--verbose", "--version"]);
    }

    #[test]
    fn complete_flag_eq_value_keeps_prefix() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\", \"yaml\"] }
";
        let result = spec_complete("mytool", text, "mytool --format=j", "/").unwrap();
        assert_eq!(result.candidates, vec!["json"]);
        assert_eq!(result.keep_prefix, "--format=");
    }

    #[test]
    fn complete_pure_file_source_falls_back_to_path_completion() {
        // type = "file" with no subcommands defers entirely to the
        // caller's existing path completion.
        let result = spec_complete("mytool", "[[args]]\ntype = \"file\"\n", "mytool x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_directory_source_filters_to_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::File::create(tmp.path().join("subfile.txt")).unwrap();
        let result = spec_complete(
            "mytool",
            "[[args]]\ntype = \"directory\"\n",
            "mytool su",
            tmp.path().to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["subdir/"]);
    }

    #[test]
    fn complete_command_source_uses_command_completer() {
        let result = spec_complete("mytool", "[[args]]\ntype = \"command\"\n", "mytool e", "/")
            .unwrap();
        // builtins list in the harness: echo, exit (both match "e"), cd.
        assert_eq!(result.candidates, vec!["echo", "exit"]);
    }

    #[test]
    fn complete_exec_source_runs_command() {
        let result = spec_complete(
            "mytool",
            "[[args]]\nexec = \"printf 'alpha\\\\nomega\\\\n'\"\n",
            "mytool al",
            "/",
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["alpha"]);
    }

    #[test]
    fn complete_exec_empty_output_falls_back() {
        let result = spec_complete("mytool", "[[args]]\nexec = \"true\"\n", "mytool x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_cursor_on_command_word_returns_none() {
        let result = spec_complete("mytool", "[[args]]\nvalues = [\"a\"]\n", "mytoo", "/");
        assert!(result.is_none());
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: コンパイルエラー(`try_complete` / `SpecCompletion` 未定義)

- [ ] **Step 3: 実装を書く**

`run_exec` の下に追加:

```rust
// ── Candidate generation ────────────────────────────────────────────

use super::command_completion::CommandCompletionContext;
use super::completion::{self, CompletionContext};

/// The outcome of a spec-based completion attempt.
pub struct SpecCompletion {
    pub candidates: Vec<String>,
    pub common_prefix: String,
    /// Re-inserted verbatim before the selected candidate: a `--flag=`
    /// prefix or the directory part of a path.
    pub keep_prefix: String,
}

/// Try spec-based completion for the word at `word_start..pos`.
///
/// Returns `None` when there is no spec or the spec gives no guidance
/// (including the fallback rule: a source that produced zero candidates)
/// — the caller should then run its existing path completion. Returns
/// `Some` with empty candidates only for `type = "none"` suppression.
#[allow(clippy::too_many_arguments)]
pub fn try_complete(
    buf: &str,
    pos: usize,
    word_start: usize,
    word: &str,
    store: &mut SpecStore,
    ctx: &CompletionContext,
    cmd_ctx: &mut CommandCompletionContext<'_>,
) -> Option<SpecCompletion> {
    let (cmd, prior_words) = command_words(buf, word_start)?;
    let spec = store.get(&cmd)?;
    let (resolution, keep_prefix) = resolve(spec, &prior_words, word);
    let filter = &word[keep_prefix.len()..];

    match resolution {
        Resolution::FlagNames(names) => {
            let mut candidates: Vec<String> =
                names.into_iter().filter(|n| n.starts_with(word)).collect();
            candidates.sort();
            finish(candidates, String::new())
        }
        Resolution::FlagValue(source) => {
            complete_source(source, filter, keep_prefix, &[], buf, pos, ctx, cmd_ctx)
        }
        Resolution::Positional {
            subcommands,
            source,
        } => {
            let sub_matches: Vec<String> = subcommands
                .into_iter()
                .filter(|s| s.starts_with(filter))
                .collect();
            match source {
                Some(source) => complete_source(
                    source,
                    filter,
                    keep_prefix,
                    &sub_matches,
                    buf,
                    pos,
                    ctx,
                    cmd_ctx,
                ),
                None => finish(sub_matches, keep_prefix),
            }
        }
    }
}

/// Generate candidates for one source, merge in already-filtered
/// subcommand names, and apply the fallback rule.
#[allow(clippy::too_many_arguments)]
fn complete_source(
    source: &CandidateSource,
    filter: &str,
    keep_prefix: String,
    sub_matches: &[String],
    buf: &str,
    pos: usize,
    ctx: &CompletionContext,
    cmd_ctx: &mut CommandCompletionContext<'_>,
) -> Option<SpecCompletion> {
    let (mut candidates, keep_prefix) = match source {
        CandidateSource::Builtin(BuiltinType::None) => {
            // Suppression: an empty result the caller treats as final.
            return Some(SpecCompletion {
                candidates: Vec::new(),
                common_prefix: String::new(),
                keep_prefix,
            });
        }
        CandidateSource::Builtin(BuiltinType::File) => {
            if sub_matches.is_empty() {
                // Pure file completion: defer to the caller's existing
                // path completion (identical behavior, fewer moving parts).
                return None;
            }
            let result = completion::complete(buf, pos, ctx);
            (result.candidates, result.dir_prefix)
        }
        CandidateSource::Builtin(BuiltinType::Directory) => {
            let result = completion::complete(buf, pos, ctx);
            let dirs: Vec<String> = result
                .candidates
                .into_iter()
                .filter(|c| c.ends_with('/'))
                .collect();
            (dirs, result.dir_prefix)
        }
        CandidateSource::Builtin(BuiltinType::Command) => {
            let cands =
                cmd_ctx
                    .completer
                    .complete(filter, cmd_ctx.path, cmd_ctx.builtins, cmd_ctx.aliases);
            (cands, keep_prefix)
        }
        CandidateSource::Values(values) => {
            let cands: Vec<String> = values
                .iter()
                .filter(|v| v.starts_with(filter))
                .cloned()
                .collect();
            (cands, keep_prefix)
        }
        CandidateSource::Exec(cmd) => {
            let cands: Vec<String> = run_exec(cmd, EXEC_TIMEOUT)
                .into_iter()
                .filter(|c| c.starts_with(filter))
                .collect();
            (cands, keep_prefix)
        }
    };

    candidates.extend(sub_matches.iter().cloned());
    candidates.sort();
    candidates.dedup();
    finish(candidates, keep_prefix)
}

/// Apply the fallback rule (empty → None) and compute the common prefix.
fn finish(candidates: Vec<String>, keep_prefix: String) -> Option<SpecCompletion> {
    if candidates.is_empty() {
        return None;
    }
    let common_prefix = completion::longest_common_prefix(&candidates);
    Some(SpecCompletion {
        candidates,
        common_prefix,
        keep_prefix,
    })
}
```

注意: `Resolution::Positional { source: None }` で `sub_matches` も空の場合、`finish` が `None` を返す(= パス補完へフォールバック)。これは設計どおり(`args` 未宣言の位置引数はパス補完)。

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test --lib interactive::spec_completion 2>&1 | tail -5`
Expected: 53 tests passed

- [ ] **Step 5: fmt してコミット**

```bash
cargo fmt
git add src/interactive/spec_completion.rs
git commit -m "feat(completion): add try_complete candidate generation

Task 5 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 6: line_editor / 対話ループへの統合

**Files:**
- Modify: `src/interactive/line_editor.rs:1167-1368`(`read_line_with_completion`、`read_line_loop_with_completion`、`handle_tab_complete`)
- Modify: `src/interactive/mod.rs`(`InteractiveShell` フィールド + 配線)
- Modify: `tests/interactive.rs`(既存 9 箇所の呼び出し + 新規統合テスト)

**Interfaces:**
- Consumes: `spec_completion::{SpecStore, try_complete}`(Task 2, 5)
- Produces: `read_line_with_completion` の新シグネチャ — `cmd_ctx` の直後に `specs: &mut SpecStore` を追加(内部 2 関数も同様)

- [ ] **Step 1: 失敗する統合テストを書く**

`tests/interactive.rs` の Tab completion テスト群の末尾に追加。既存テストと同じヘルパ(`chars`, `key`, `MockTerminal`)を使う:

```rust
#[test]
fn test_tab_spec_completion_subcommand_values() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    let spec_dir = tmp.path().join("completions");
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(
        spec_dir.join("mytool.toml"),
        "[[subcommands]]\nname = \"deploy\"\n\n[[subcommands.args]]\nvalues = [\"prod\", \"stage\"]\n",
    )
    .unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // "mytool deploy pr" + Tab → unique candidate "prod" is inserted.
    let mut events = chars("mytool deploy pr");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut spec_store = SpecStore::new(spec_dir);
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
        .read_line_with_completion(
            "$ ",
            &[],
            &mut history,
            &mut term,
            &ctx,
            &mut cmd_ctx,
            &mut spec_store,
            &mut scanner,
            &checker_env,
            "",
        )
        .unwrap();
    assert_eq!(result, Some("mytool deploy prod ".to_string()));
}

#[test]
fn test_tab_spec_none_source_suppresses_path_completion() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    // A file that plain path completion WOULD match.
    fs::File::create(tmp.path().join("unique_file.txt")).unwrap();
    let spec_dir = tmp.path().join("completions");
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(spec_dir.join("mytool.toml"), "[[args]]\ntype = \"none\"\n").unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    let mut events = chars("mytool uni");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut spec_store = SpecStore::new(spec_dir);
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
        .read_line_with_completion(
            "$ ",
            &[],
            &mut history,
            &mut term,
            &ctx,
            &mut cmd_ctx,
            &mut spec_store,
            &mut scanner,
            &checker_env,
            "",
        )
        .unwrap();
    // Tab must NOT expand to unique_file.txt.
    assert_eq!(result, Some("mytool uni".to_string()));
}

#[test]
fn test_tab_no_spec_falls_back_to_path_completion() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("unique_file.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    let mut events = chars("ls uni");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    // Store pointing at a dir with no specs — behavior must be identical
    // to the pre-feature path completion.
    let mut spec_store = SpecStore::new(tmp.path().join("no_specs"));
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
        .read_line_with_completion(
            "$ ",
            &[],
            &mut history,
            &mut term,
            &ctx,
            &mut cmd_ctx,
            &mut spec_store,
            &mut scanner,
            &checker_env,
            "",
        )
        .unwrap();
    assert_eq!(result, Some("ls unique_file.txt ".to_string()));
}
```

- [ ] **Step 2: 既存 9 箇所の呼び出しを新シグネチャに更新**

`tests/interactive.rs` の既存の `read_line_with_completion` 呼び出し(1086, 1135, 1183, 1231, 1287, 1347, 1397, 1446, 1494 行付近の 9 箇所)それぞれについて、呼び出しの前に:

```rust
    let mut spec_store =
        yosh::interactive::spec_completion::SpecStore::new(std::path::PathBuf::from("/nonexistent"));
```

を追加し、引数リストの `&mut cmd_ctx,` の直後に `&mut spec_store,` を挿入する。既存テストの意味(スペックなし → 従来どおりのパス/コマンド補完)は変わらない。

- [ ] **Step 3: テストが失敗(コンパイルエラー)することを確認**

Run: `cargo test --test interactive 2>&1 | tail -5`
Expected: コンパイルエラー(`read_line_with_completion` の引数個数不一致)

- [ ] **Step 4: line_editor.rs を変更**

`src/interactive/line_editor.rs` 冒頭の use 群(`use super::command_completion::CommandCompletionContext;` の近く)に追加:

```rust
use super::spec_completion::{self, SpecStore};
```

3 関数のシグネチャに `specs: &mut SpecStore` を追加する。`read_line_with_completion`(1167 行付近):

```rust
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
```

内部の `read_line_loop_with_completion` 呼び出しと定義にも同じ位置(`cmd_ctx` の次)で `specs` を渡す。ループ内の Tab 分岐:

```rust
                        KeyAction::TabComplete => {
                            term.reset_style()?;
                            self.handle_tab_complete(term, prompt, upper_lines, ctx, cmd_ctx, specs)?;
                        }
```

`handle_tab_complete` のシグネチャと補完分岐(1288-1319 行付近)を変更:

```rust
    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
    ) -> io::Result<()> {
        let (word_start, word) = {
            let buf = self.buffer();
            let (ws, w) = extract_completion_word(&buf, self.pos);
            (ws, w.to_owned())
        };
        let is_cmd_pos = {
            let buf = self.buffer();
            is_command_position(&buf, word_start)
        };

        let (candidates, common_prefix, dir_prefix) = if is_cmd_pos && !word.contains('/') {
            // Command name completion
            let (cands, common) = cmd_ctx.completer.complete_common_prefix(
                &word,
                cmd_ctx.path,
                cmd_ctx.builtins,
                cmd_ctx.aliases,
            );
            (cands, common, String::new())
        } else if let Some(result) = spec_completion::try_complete(
            &self.buffer(),
            self.pos,
            word_start,
            &word,
            specs,
            ctx,
            cmd_ctx,
        ) {
            // Spec-based completion (user-defined per-command TOML)
            (result.candidates, result.common_prefix, result.keep_prefix)
        } else {
            // Path completion (existing)
            let result = completion::complete(&self.buffer(), self.pos, ctx);
            (result.candidates, result.common_prefix, result.dir_prefix)
        };
```

以降(`if candidates.is_empty()` から先)は変更なし — `type = "none"` の空 candidates はここで自然に「何もしない」になる。

- [ ] **Step 5: mod.rs を配線**

`src/interactive/mod.rs`:

1. use 追加(`use completion::CompletionContext;` の下):

```rust
use spec_completion::SpecStore;
```

2. `InteractiveShell` 構造体(37 行付近)にフィールド追加:

```rust
    spec_store: SpecStore,
```

3. `Self { ... }` 構築(131-138 行付近)に追加:

```rust
        let home_dir = executor.env.vars.get("HOME").unwrap_or("").to_string();
        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
            scanner: HighlightScanner::new(),
            command_completer: CommandCompleter::new(),
            spec_store: SpecStore::from_home(&home_dir),
        }
```

(`executor` を move する前に `home_dir` を取り出すこと。)

4. `read_line_with_completion` 呼び出し(205-215 行付近)の `&mut cmd_ctx,` の直後に追加:

```rust
                &mut self.spec_store,
```

- [ ] **Step 6: テストが通ることを確認**

Run: `cargo test --lib 2>&1 | tail -5 && cargo test --test interactive 2>&1 | tail -5`
Expected: 両方 PASS(interactive は既存 9 テスト + 新規 3 テストを含む)

- [ ] **Step 7: fmt + clippy してコミット**

```bash
cargo fmt
cargo clippy --lib --tests 2>&1 | tail -5   # 警告ゼロを確認
git add src/interactive/line_editor.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(completion): wire spec-based completion into tab handling

Task 6 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

### Task 7: PTY E2E テスト + 仕上げ

**Files:**
- Modify: `tests/pty_interactive.rs`(テスト 1 件追加)
- Modify: `completion.md`(ステータス行の更新)
- Modify: `TODO.md`(既知の制限の記録)

**Interfaces:**
- Consumes: Task 6 までの全機能(実バイナリ経由)

- [ ] **Step 1: PTY E2E テストを書く**

`tests/pty_interactive.rs` の末尾に追加。`spawn_yosh` は `HOME` を一時ディレクトリに向けるので、そこにスペックを書き込めばよい(ロードは初回 Tab 時なので起動後の書き込みで間に合う):

```rust
// ── Spec-based completion ──────────────────────────────────────────────

#[test]
fn tab_spec_completion_inserts_candidate() {
    let (mut session, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    // Specs load lazily on first Tab, so writing after startup is safe.
    let dir = tmpdir.path().join(".config/yosh/completions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("echo.toml"),
        "[[args]]\nvalues = [\"alpha\", \"omega\"]\n",
    )
    .unwrap();

    session.send("echo al").unwrap();
    // Allow the editor to render before sending Tab (matches the other
    // tab-completion PTY tests; see TODO.md on fixed waits).
    std::thread::sleep(Duration::from_millis(100));
    session.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\r").unwrap();

    // Tab completed "al" → "alpha", so the command ran as `echo alpha`.
    expect_output(&mut session, "alpha", "spec completion did not insert candidate");
    exit_shell(&mut session);
}
```

- [ ] **Step 2: PTY テストを実行して通ることを確認**

Run: `cargo test --test pty_interactive tab_spec_completion_inserts_candidate 2>&1 | tail -5`
Expected: 1 test passed(ビルド込みで数分かかることがある。タイムアウトは 600000 ms に設定)

- [ ] **Step 3: completion.md のステータスを更新**

`completion.md` の 3 行目:

```markdown
ステータス: **ドラフト(設計承認済み、未実装)**
```

を以下に変更:

```markdown
ステータス: **実装済み(2026-07-14)**
```

- [ ] **Step 4: TODO.md に既知の制限を追記**

`TODO.md` の対話モード関連セクション(429 行付近の並び)に追加:

```markdown
- [ ] Spec completion: `--flag=path/sub` 形式でフラグ値が `directory`/`file` ソースのとき、パス補完がワード全体(`--flag=path/`)をディレクトリ部として解釈するため候補が出ない(パス補完へのフォールバックも同様に失敗し、無害だが未補完)。`=` 以降のみを切り出したパス補完が必要(`src/interactive/spec_completion.rs`)
```

- [ ] **Step 5: フルテストを実行**

Run(バックグラウンド推奨、タイムアウト 600000 ms):
```bash
cargo test 2>&1 | tail -15
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: すべて PASS(既存の XFAIL を除く)

- [ ] **Step 6: コミット**

```bash
git add tests/pty_interactive.rs completion.md TODO.md
git commit -m "test(completion): add PTY e2e for spec completion; mark design implemented

Task 7 of docs/superpowers/plans/2026-07-14-spec-completion.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T24r2gxzhNjScuYxqj2aFE"
```

---

## セルフレビュー結果

- **仕様カバレッジ**: completion.md の各節 → ファイル配置/遅延ロード/警告 = Task 2、候補ソース 3 形態 + 組み込み 4 タイプ = Task 1・5、マッチング(ネスト降下、フラグ消費、`--flag=`、位置引数の繰り返し)= Task 3、`exec`(sh -c、500ms、行分割)= Task 4、フォールバック規則と `none` 抑止 = Task 5・6、統合 = Task 6、PTY E2E = Task 7。ギャップなし。
- **設計との差分(意図的)**: `type = "file"` 単独(サブコマンド併存なし)は自前で候補生成せず `None` を返して既存パス補完に委譲する。挙動は設計の「file = 既存パス補完ロジックの再利用」と同一で、実装がより単純。
- **型整合**: `SpecStore::get` → `Option<&CompletionSpec>`、`resolve` → `(Resolution, String)`、`try_complete` → `Option<SpecCompletion>` で Task 5/6 の使用箇所と一致。テスト内のヘルパ名(`store_with`, `spec_complete`, `as_strings`)は各タスクで定義済みのものだけを参照。
