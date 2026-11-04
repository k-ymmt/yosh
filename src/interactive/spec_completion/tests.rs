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
    let err = CompletionSpec::parse("[[args]]\ntype = \"file\"\nvalues = [\"a\"]\n").unwrap_err();
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
    assert!(
        err.contains("duplicate subcommand"),
        "unexpected error: {err}"
    );
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
    assert!(
        err.contains("duplicate subcommand"),
        "unexpected error: {err}"
    );
}

#[test]
fn bundled_completion_specs_parse() {
    // Every spec shipped in <repo>/completions/ must stay valid.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("completions");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).unwrap();
            CompletionSpec::parse(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            checked += 1;
        }
    }
    assert!(checked > 0, "no bundled specs found in {}", dir.display());
}

#[test]
fn embedded_specs_parse_and_include_git() {
    assert!(!EMBEDDED_SPECS.is_empty());
    assert!(EMBEDDED_SPECS.iter().any(|(name, _)| *name == "git"));
    for (name, text) in EMBEDDED_SPECS {
        CompletionSpec::parse(text).unwrap_or_else(|err| panic!("embedded {name}: {err}"));
    }
}

#[test]
fn embedded_specs_match_repo_dir() {
    // build.rs must pick up every spec file — a missing entry here
    // means the generator silently skipped one.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("completions");
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
        .collect();
    on_disk.sort();
    let embedded: Vec<String> = EMBEDDED_SPECS
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    assert_eq!(
        embedded, on_disk,
        "EMBEDDED_SPECS out of sync with completions/"
    );
}

// ── SpecStore ────────────────────────────────────────────────────

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

#[test]
fn store_with_embedded_falls_back_when_no_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
    let spec = store.get("git").expect("embedded git spec");
    assert!(spec.subcommands.iter().any(|s| s.name == "log"));
}

#[test]
fn store_disk_file_overrides_embedded() {
    let (_tmp, dir) = {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(
            dir.join("git.toml"),
            "[[subcommands]]\nname = \"only-mine\"\n",
        )
        .unwrap();
        (tmp, dir)
    };
    let mut store = SpecStore::with_embedded(dir);
    let spec = store.get("git").unwrap();
    assert_eq!(spec.subcommands.len(), 1);
    assert_eq!(spec.subcommands[0].name, "only-mine");
}

#[test]
fn store_broken_disk_file_does_not_fall_back() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("git.toml"), "not [ valid toml").unwrap();
    let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
    assert!(store.get("git").is_none());
}

#[test]
fn store_unreadable_disk_file_does_not_fall_back() {
    // A non-UTF-8 file makes read_to_string fail with InvalidData:
    // the user's file exists, so the embedded spec must not be served.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("git.toml"), [0xFF, 0xFE, 0xFD]).unwrap();
    let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
    assert!(store.get("git").is_none());
}

#[test]
fn store_empty_disk_file_overrides_embedded() {
    // README documents an empty file as the way to disable a
    // bundled spec: it must win over the embedded git spec.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("git.toml"), "").unwrap();
    let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
    let spec = store.get("git").expect("empty file is a valid spec");
    assert!(spec.subcommands.is_empty());
}

#[test]
fn store_plain_new_has_no_embedded_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut store = SpecStore::new(tmp.path().to_path_buf());
    assert!(store.get("git").is_none());
}

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

#[test]
fn words_after_subshell_close() {
    // "(cd /tmp) git ch|" — word_start = 14: the closed subshell is a
    // finished segment; the command word is git, not cd.
    let (cmd, args) = command_words("(cd /tmp) git ch", 14).unwrap();
    assert_eq!(cmd, "git");
    assert!(args.is_empty());
}

#[test]
fn words_after_redirect_is_none() {
    // "git add > /tmp/fi|" — word_start = 10: the redirect target is
    // not a git argument; with no command in its segment the caller
    // falls back to plain path completion.
    assert!(command_words("git add > /tmp/fi", 10).is_none());
}

#[test]
fn words_segment_delimiters_inside_quotes_do_not_split() {
    // "git commit -m 'a > b' |" — word_start = 22
    let (cmd, args) = command_words("git commit -m 'a > b' ", 22).unwrap();
    assert_eq!(cmd, "git");
    assert_eq!(args, vec!["commit", "-m", "a > b"]);
}

#[test]
fn words_closed_command_sub_does_not_split_segment() {
    // "git -C $(pwd) ch|" — word_start = 14: the `)` closing the
    // command substitution is not a segment boundary, so the
    // command word is still git.
    let buf = "git -C $(pwd) ch";
    let (cmd, args) = command_words(buf, 14).unwrap();
    assert_eq!(cmd, "git");
    assert_eq!(args, vec!["-C", "$(pwd)"]);
}

#[test]
fn words_closed_backtick_does_not_split_segment() {
    // "git -C `pwd` ch|" — word_start = 13.
    let buf = "git -C `pwd` ch";
    let (cmd, args) = command_words(buf, 13).unwrap();
    assert_eq!(cmd, "git");
    assert_eq!(args, vec!["-C", "`pwd`"]);
}

#[test]
fn words_inside_open_command_sub_start_fresh_segment() {
    // "echo $(git ch|" — the cursor is inside the substitution:
    // its own command word (git) resolves, not echo.
    let buf = "echo $(git ch";
    let (cmd, args) = command_words(buf, 11).unwrap();
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
        Resolution::Positional {
            subcommands,
            flags,
            source,
        } => {
            assert_eq!(subcommands, vec!["checkout", "remote"]);
            assert_eq!(flags, vec!["-C", "--no-pager"]);
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
        Resolution::Positional {
            subcommands,
            flags,
            source,
        } => {
            assert!(subcommands.is_empty());
            assert_eq!(flags, vec!["-b"]);
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
        Resolution::Positional {
            subcommands,
            flags,
            source,
        } => {
            assert_eq!(subcommands, vec!["sub"]);
            assert!(flags.is_empty());
            assert_eq!(source, None);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn resolve_positional_includes_level_flags() {
    let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
    let (res, _) = resolve(&spec, &[], "");
    match res {
        Resolution::Positional { flags, .. } => {
            assert_eq!(flags, vec!["-C", "--no-pager"]);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// ── run_exec ─────────────────────────────────────────────────────

use std::time::Duration;

#[test]
fn exec_splits_stdout_lines_and_drops_empties() {
    let lines = run_exec("printf 'alpha\\n\\nbeta\\n'", Duration::from_secs(5), None);
    assert_eq!(lines, vec!["alpha", "beta"]);
}

#[test]
fn exec_nonzero_exit_yields_no_candidates() {
    let lines = run_exec("echo out; exit 1", Duration::from_secs(5), None);
    assert!(lines.is_empty());
}

#[test]
fn exec_timeout_kills_child_and_yields_no_candidates() {
    let start = std::time::Instant::now();
    let lines = run_exec("sleep 5", Duration::from_millis(100), None);
    assert!(lines.is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "timeout did not fire: {:?}",
        start.elapsed()
    );
}

#[test]
fn exec_inherits_cwd() {
    let _guard = crate::test_sync::lock_cwd();
    // The child runs in the shell's cwd, so `pwd` output is non-empty
    // and matches the current dir.
    let cwd = std::env::current_dir().unwrap();
    let lines = run_exec("pwd", Duration::from_secs(5), None);
    assert_eq!(lines, vec![cwd.to_string_lossy().to_string()]);
}

#[test]
fn exec_backgrounded_grandchild_does_not_hang() {
    let start = std::time::Instant::now();
    let lines = run_exec("sleep 5 & echo hi", Duration::from_millis(100), None);
    assert!(lines.is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "reader join must be bounded: {:?}",
        start.elapsed()
    );
}

#[test]
fn exec_uses_provided_env() {
    let env = vec![
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("YOSH_TEST_MARKER".to_string(), "marker42".to_string()),
    ];
    let lines = run_exec(
        "printf '%s\\n' \"$YOSH_TEST_MARKER\"",
        Duration::from_secs(5),
        Some(&env),
    );
    assert_eq!(lines, vec!["marker42"]);
}

// ── try_complete ─────────────────────────────────────────────────

use crate::env::aliases::AliasStore;
use crate::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
use crate::interactive::completion::CompletionContext;

/// Run try_complete over `line` (cursor at end) against a single spec
/// named `cmd`, using a scratch cwd.
fn spec_complete(cmd: &str, spec_text: &str, line: &str, cwd: &str) -> Option<SpecCompletion> {
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
    try_complete(line, word_start, &word, &mut store, &ctx, &mut cmd_ctx)
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
    let result = spec_complete("mytool", "[[args]]\ntype = \"none\"\n", "mytool x", "/").unwrap();
    assert!(result.candidates.is_empty());
}

#[test]
fn complete_none_source_still_offers_subcommands() {
    let text = "[[args]]\ntype = \"none\"\n[[subcommands]]\nname = \"deploy\"\n";
    let result = spec_complete("mytool", text, "mytool dep", "/").unwrap();
    assert_eq!(result.candidates, vec!["deploy"]);
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
    // Subcommand names sort before source candidates.
    assert_eq!(result.candidates, vec!["deploy", "deep"]);
}

#[test]
fn complete_empty_word_orders_subcommands_flags_then_source() {
    let text = "\
[[flags]]
names = [\"--verbose\", \"-v\"]
[[args]]
values = [\"alpha\"]
[[subcommands]]
name = \"deploy\"
";
    let result = spec_complete("mytool", text, "mytool ", "/").unwrap();
    assert_eq!(
        result.candidates,
        vec!["deploy", "--verbose", "-v", "alpha"]
    );
    assert_eq!(result.common_prefix, "");
}

#[test]
fn complete_none_source_still_offers_flags() {
    let text = "[[flags]]\nnames = [\"--force\"]\n\n[[args]]\ntype = \"none\"\n";
    let result = spec_complete("mytool", text, "mytool ", "/").unwrap();
    assert_eq!(result.candidates, vec!["--force"]);
}

#[test]
fn complete_file_source_with_flags_lists_flags_before_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::File::create(tmp.path().join("afile.txt")).unwrap();
    let text = "[[flags]]\nnames = [\"--all\"]\n\n[[args]]\ntype = \"file\"\n";
    let result = spec_complete("mytool", text, "mytool ", tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(result.candidates, vec!["--all", "afile.txt"]);
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
fn complete_flag_eq_directory_value() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("subdir")).unwrap();
    std::fs::File::create(tmp.path().join("subfile.txt")).unwrap();
    let text = "[[flags]]\nnames = [\"--dest\"]\nvalue = { type = \"directory\" }\n";
    let result = spec_complete(
        "mytool",
        text,
        "mytool --dest=su",
        tmp.path().to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(result.candidates, vec!["subdir/"]);
    assert_eq!(result.keep_prefix, "--dest=");
}

#[test]
fn complete_flag_eq_file_value_in_subdir() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::File::create(tmp.path().join("sub").join("nested.txt")).unwrap();
    let text = "[[flags]]\nnames = [\"--config\"]\nvalue = { type = \"file\" }\n";
    let result = spec_complete(
        "mytool",
        text,
        "mytool --config=sub/ne",
        tmp.path().to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(result.candidates, vec!["nested.txt"]);
    assert_eq!(result.keep_prefix, "--config=sub/");
}

#[test]
fn complete_command_source_uses_command_completer() {
    let result =
        spec_complete("mytool", "[[args]]\ntype = \"command\"\n", "mytool e", "/").unwrap();
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
