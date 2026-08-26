use super::{ExpandedField, pattern};
use crate::env::ShellEnv;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Perform pathname expansion (glob) on each field.
///
/// Rules (POSIX 2.6.6):
/// 1. If a field contains an unquoted `*`, `?`, or `[` opening a closable
///    bracket expression, attempt glob expansion. (A `[` with no later
///    `]` is literal and does not trigger a filesystem walk.)
/// 2. If one or more filesystem paths match, replace the field with those
///    matches (sorted, each marked fully-quoted so they are not re-split).
/// 3. If no match is found, keep the original field unchanged.
/// 4. `*` and `?` do NOT match a leading `.` unless the pattern starts with `.`.
/// 5. `*` and `?` never match `/`.
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    // Fast path: if no field contains unquoted glob metachars, return input as-is.
    // Avoids the output-Vec allocation that is the #1 dhat site by bytes in W2
    // (~2.94 MB / 14k calls). See docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md.
    if !fields.iter().any(has_unquoted_glob_chars) {
        return fields;
    }

    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
                // No match — keep original field unchanged.
                result.push(field);
            } else {
                for m in matches {
                    result.push(ExpandedField::all_quoted(m));
                }
            }
        } else {
            result.push(field);
        }
    }
    result
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Return `true` if the field contains at least one unquoted glob metachar:
/// `*`, `?`, or a `[` that opens a closable bracket expression.
///
/// A lone `[` with no later `]` is a literal (the pattern parser emits it
/// as `Literal('[')`), so it must NOT trigger a glob attempt — otherwise
/// every `[ ... ]` test command pays a `read_dir` of the cwd plus a
/// per-entry pattern match (measured 43s for a 50k-iteration `[ $i -lt N ]`
/// loop in a 2000-entry directory). Mirrors bash's `glob_pattern_p`.
fn has_unquoted_glob_chars(field: &ExpandedField) -> bool {
    let bytes = field.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if field.is_glob_protected(i) {
            continue;
        }
        match b {
            b'*' | b'?' => return true,
            b'[' if bracket_has_close(bytes, i, &|j| field.is_glob_protected(j)) => return true,
            _ => {}
        }
    }
    false
}

/// Given a `[` at byte offset `open` in `bytes`, return `true` if a `]`
/// that can close the bracket expression follows. Follows the same
/// convention as `pattern::parse_bracket`: a `]` immediately after the
/// opening `[` (or after `[!`) is a literal member of the class and does
/// not close it. Conservative in the safe direction — a `]` later
/// swallowed by a `[:class:]` member may yield a false positive (a wasted
/// glob attempt, which is exactly today's behavior for every `[`), never
/// a false negative.
///
/// `is_protected(j)` reports whether the byte at `j` is glob-protected
/// (quoted). A protected `]` can never CLOSE the expression — POSIX
/// §2.13.1: a quoted `]` is literal, so `sr[c"]"` has no closing bracket
/// and the `[` stays literal (matches bash/dash). It still counts as an
/// immediate literal member (`["]"x]`), like an unquoted one in that
/// position.
///
/// Byte-wise scanning is UTF-8 safe: `[`/`]`/`!` are ASCII and never
/// appear inside multibyte sequences.
fn bracket_has_close(bytes: &[u8], open: usize, is_protected: &dyn Fn(usize) -> bool) -> bool {
    let mut j = open + 1;
    if bytes.get(j) == Some(&b'!') {
        j += 1;
    }
    if bytes.get(j) == Some(&b']') {
        j += 1;
    }
    (j..bytes.len()).any(|k| bytes[k] == b']' && !is_protected(k))
}

/// `has_unquoted_glob_chars` for a plain pattern string (no quote mask —
/// nothing is protected): used by the per-component test in
/// `expand_components`, whose input is a raw `field.value` slice.
fn str_has_glob_chars(s: &str) -> bool {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'*' | b'?' => return true,
            b'[' if bracket_has_close(bytes, i, &|_| false) => return true,
            _ => {}
        }
    }
    false
}

/// Expand a glob pattern against the filesystem, returning a sorted list of
/// matching paths.
///
/// If the pattern contains no `/`, glob is performed in the current directory
/// and results are returned as bare file names.
///
/// If the pattern contains `/`, it is split on the first `/`-delimited
/// component, the directory portion is resolved (possibly recursively), and
/// the final component pattern is matched against entries in that directory.
pub(crate) fn glob_match(pattern: &str) -> Vec<String> {
    if !pattern.contains('/') {
        // Simple case: glob in the current directory.
        let mut matches = glob_in_dir(".", pattern);
        matches.sort();
        return matches;
    }

    // Patterns containing slashes: split into directory + filename parts.
    // Walk the directory tree component by component.
    let mut matches = glob_path(pattern);
    matches.sort();
    matches
}

/// Expand a slash-containing pattern into filesystem paths.
///
/// Strategy: split the pattern on `/`, then expand each component.
/// A component may itself contain glob chars (e.g., `src/*/mod.rs`).
fn glob_path(pattern: &str) -> Vec<String> {
    // Split the pattern into a leading absolute/relative prefix and components.
    // e.g., "src/*.rs"  → base="",  components=["src", "*.rs"]
    //       "/usr/*/bin" → base="/", components=["usr", "*", "bin"]
    let (base, components) = if let Some(stripped) = pattern.strip_prefix('/') {
        ("/".to_string(), stripped.split('/').collect::<Vec<_>>())
    } else {
        (String::new(), pattern.split('/').collect::<Vec<_>>())
    };

    expand_components(base, &components)
}

/// Recursively expand each path component, returning matching paths.
fn expand_components(dir: String, components: &[&str]) -> Vec<String> {
    if components.is_empty() {
        return if dir.is_empty() { vec![] } else { vec![dir] };
    }

    let component = components[0];
    let rest = &components[1..];

    // Determine whether the component has glob chars. Like the field-level
    // predicate, a `[` counts only when a closable `]` follows.
    let is_glob = str_has_glob_chars(component);

    if is_glob {
        let search_dir = if dir.is_empty() { "." } else { &dir };
        let entries = glob_in_dir(search_dir, component);

        let mut result = Vec::new();
        for entry in entries {
            // Build the full path so far.
            let full = join_path(&dir, &entry);
            if rest.is_empty() {
                result.push(full);
            } else {
                // Only recurse into directories.
                if std::fs::metadata(fs_path(&full))
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    result.extend(expand_components(full, rest));
                }
            }
        }
        result
    } else {
        // Literal component: just append and recurse.
        let full = join_path(&dir, component);
        if rest.is_empty() {
            // Verify the path exists.
            if fs_path(&full).exists() {
                vec![full]
            } else {
                vec![]
            }
        } else {
            expand_components(full, rest)
        }
    }
}

/// Convert a byteenc-encoded shell path into a `PathBuf` carrying the
/// original raw bytes, for filesystem checks during glob walking.
fn fs_path(s: &str) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&crate::byteenc::decode_bytes(
        s,
    )))
}

/// Join a directory and a filename into a path string.
/// Handles the special case where `dir` is empty (relative, current dir).
fn join_path(dir: &str, name: &str) -> String {
    match dir {
        "" => name.to_string(),
        "." => name.to_string(),
        "/" => format!("/{}", name),
        d => format!("{}/{}", d, name),
    }
}

/// List entries in `dir` that match `pattern`.
///
/// POSIX rules applied here:
/// - Entries starting with `.` are skipped unless `pattern` starts with `.`.
/// - `*` and `?` do not match `/` (enforced by pattern::matches since we only
///   test against entry names, never full paths).
fn glob_in_dir(dir: &str, pattern: &str) -> Vec<String> {
    use std::os::unix::ffi::OsStrExt;
    // `dir` is a byteenc-encoded shell string; decode it so non-UTF-8
    // directory components resolve to the real on-disk path.
    let dir_bytes = crate::byteenc::decode_bytes(dir);
    let read_dir = match std::fs::read_dir(std::ffi::OsStr::from_bytes(&dir_bytes)) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let skip_hidden = !pattern.starts_with('.');

    // Compile the pattern once per directory walk instead of re-tokenizing
    // it for every entry (2000-entry dir == 2000 full pattern parses).
    let compiled = pattern::compile(pattern);

    let mut matches = Vec::new();

    // `read_dir` never yields the `.` and `..` entries, but POSIX's
    // leading-<period> rule makes them matchable when the pattern begins
    // with an explicit `.` (bash/dash: `echo .*` lists `. ..` first;
    // `.[!.]*` matches neither). Offer them as candidates explicitly.
    if !skip_hidden {
        for special in [".", ".."] {
            if compiled.matches(special) {
                matches.push(special.to_string());
            }
        }
    }
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        // Encode the raw entry bytes so names that are not valid UTF-8
        // become matchable fields and round-trip losslessly back to the
        // on-disk bytes at exec/open boundaries. An escaped byte is one
        // `char`, so `?` matches a single invalid byte.
        let name_str = crate::byteenc::encode_bytes(name.as_bytes());

        // POSIX: `*` and `?` do not match a leading dot.
        if skip_hidden && name_str.starts_with('.') {
            continue;
        }

        if compiled.matches(&name_str) {
            matches.push(name_str.into_owned());
        }
    }

    matches
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    /// Directory entries whose names are not valid UTF-8 are byteenc-encoded
    /// by `glob_in_dir`; each escaped byte is one `char`, so `?`/`*`/literal
    /// patterns match them the way byte-oriented shells do. Real files with
    /// such names cannot be created on macOS APFS (EILSEQ), so the matching
    /// semantics are locked in here against the encoded form directly.
    #[test]
    fn encoded_invalid_byte_name_matches_patterns() {
        let name = crate::byteenc::encode_bytes(b"f\xe9g").into_owned();
        assert!(pattern::matches("f?g", &name));
        assert!(pattern::matches("f*g", &name));
        assert!(pattern::matches("*", &name));
        assert!(!pattern::matches("f?", &name));
        // A pattern containing the same escaped byte matches literally.
        assert!(pattern::matches(&name, &name));
        // Round trip back to the on-disk bytes is lossless.
        assert_eq!(crate::byteenc::decode_bytes(&name).as_ref(), b"f\xe9g");
    }

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    fn unquoted(s: &str) -> ExpandedField {
        let mut f = ExpandedField::new();
        f.push_expanded(s);
        f
    }

    fn quoted_field(s: &str) -> ExpandedField {
        let mut f = ExpandedField::new();
        f.push_quoted(s);
        f
    }

    fn values(fields: Vec<ExpandedField>) -> Vec<String> {
        fields.into_iter().map(|f| f.value).collect()
    }

    // ── No glob chars: pass-through ──

    #[test]
    fn test_no_glob_passthrough() {
        let env = make_env();
        let input = vec![unquoted("hello")];
        assert_eq!(values(expand(&env, input)), vec!["hello"]);
    }

    // ── Quoted glob: not expanded ──

    #[test]
    fn test_quoted_glob_not_expanded() {
        let env = make_env();
        let input = vec![quoted_field("*.rs")];
        let result = expand(&env, input);
        // Should remain unchanged since the mask is all-quoted.
        assert_eq!(values(result), vec!["*.rs"]);
    }

    // ── Actual filesystem glob ──

    #[test]
    fn test_glob_src_files() {
        // Change to the project root so "src/*.rs" makes sense.
        // We can't change cwd in tests easily, so use an absolute pattern
        // that we know exists.  We'll test that main.rs shows up.
        let shell_env = make_env();

        // Construct a pattern pointing at the src directory of this crate.
        // The crate root is two levels up from src/expand/pathname.rs.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pattern = std::path::Path::new(manifest_dir)
            .join("src")
            .join("*.rs")
            .to_string_lossy()
            .into_owned();

        let input = vec![unquoted(&pattern)];
        let result = values(expand(&shell_env, input));

        // At least main.rs and error.rs should be in src/
        assert!(
            result.iter().any(|p| p.ends_with("main.rs")),
            "expected main.rs in {:?}",
            result
        );
    }

    // ── No match: keep original pattern ──

    #[test]
    fn test_no_match_keeps_pattern() {
        let env = make_env();
        let input = vec![unquoted("nonexistent_*.xyz")];
        let result = values(expand(&env, input));
        assert_eq!(result, vec!["nonexistent_*.xyz"]);
    }

    // ── Hidden files not matched by * ──

    #[test]
    fn test_star_does_not_match_dotfiles() {
        let _env = make_env();
        // In any directory, "*" should not return dotfiles.
        let matches = glob_in_dir(".", "*");
        for m in &matches {
            assert!(
                !m.starts_with('.'),
                "glob '*' should not match dotfile: {}",
                m
            );
        }
    }

    // ── Leading-dot rule: `.` / `..` candidates (audit L7) ──

    #[test]
    fn dot_star_matches_dot_and_dotdot() {
        let matches = glob_in_dir(".", ".*");
        assert!(matches.contains(&".".to_string()), "got: {:?}", matches);
        assert!(matches.contains(&"..".to_string()), "got: {:?}", matches);
    }

    #[test]
    fn dot_bang_dot_star_excludes_dot_and_dotdot() {
        let matches = glob_in_dir(".", ".[!.]*");
        assert!(!matches.contains(&".".to_string()), "got: {:?}", matches);
        assert!(!matches.contains(&"..".to_string()), "got: {:?}", matches);
    }

    #[test]
    fn star_still_excludes_dot_and_dotdot() {
        let matches = glob_in_dir(".", "*");
        assert!(!matches.contains(&".".to_string()));
        assert!(!matches.contains(&"..".to_string()));
    }

    // ── has_unquoted_glob_chars ──

    #[test]
    fn test_has_unquoted_glob_chars_true() {
        assert!(has_unquoted_glob_chars(&unquoted("*.rs")));
        assert!(has_unquoted_glob_chars(&unquoted("file?.txt")));
        assert!(has_unquoted_glob_chars(&unquoted("[abc]")));
    }

    // ── Lone `[` must not trigger globbing (the `[ ... ]` test-command
    //    hot path: every `while [ $i -lt N ]` iteration hits this) ──

    #[test]
    fn lone_open_bracket_is_not_glob() {
        assert!(!has_unquoted_glob_chars(&unquoted("[")));
        assert!(!has_unquoted_glob_chars(&unquoted("a[")));
        assert!(!has_unquoted_glob_chars(&unquoted("a[b")));
        assert!(!has_unquoted_glob_chars(&unquoted("[!")));
    }

    #[test]
    fn closable_bracket_still_globs() {
        assert!(has_unquoted_glob_chars(&unquoted("[abc]")));
        assert!(has_unquoted_glob_chars(&unquoted("a[bc]d")));
        assert!(has_unquoted_glob_chars(&unquoted("[!abc]")));
        // `[]a]` — the first `]` is a literal member, the second closes.
        assert!(has_unquoted_glob_chars(&unquoted("[]a]")));
        // POSIX class form.
        assert!(has_unquoted_glob_chars(&unquoted("[[:alpha:]]")));
    }

    #[test]
    fn unclosable_bracket_per_literal_member_rule_is_not_glob() {
        // `[]` — the `]` right after `[` is a literal member, so nothing
        // closes the class; parse_bracket agrees (Literal('[') token).
        assert!(!has_unquoted_glob_chars(&unquoted("[]")));
        // `[!]` — same, after the negation marker.
        assert!(!has_unquoted_glob_chars(&unquoted("[!]")));
    }

    #[test]
    fn quoted_bracket_pair_is_not_glob() {
        assert!(!has_unquoted_glob_chars(&quoted_field("[abc]")));
        assert!(!has_unquoted_glob_chars(&quoted_field("[")));
    }

    #[test]
    fn unquoted_open_bracket_with_quoted_close_is_literal() {
        // POSIX §2.13.1: a quoted `]` is literal and cannot close the
        // bracket expression, so an unquoted `[` with only a quoted `]`
        // after it has no closer and stays literal (bash: `sr[c"]"`
        // prints `sr[c]`, it does not glob-match `src`).
        let mut f = ExpandedField::new();
        f.push_expanded("[a");
        f.push_quoted("]");
        assert!(!has_unquoted_glob_chars(&f));
    }

    #[test]
    fn quoted_close_bracket_repro_sr_c() {
        // `sr[c"]"` — `[` and `c` unquoted, `]` quoted → literal.
        let mut f = ExpandedField::new();
        f.push_expanded("sr[c");
        f.push_quoted("]");
        assert!(!has_unquoted_glob_chars(&f));

        // `sr["c]"` — `[` unquoted, `c]` quoted → no unprotected closer,
        // literal.
        let mut f = ExpandedField::new();
        f.push_expanded("sr[");
        f.push_quoted("c]");
        assert!(!has_unquoted_glob_chars(&f));
    }

    #[test]
    fn quoted_close_followed_by_unquoted_close_still_globs() {
        // `[a"]"]` — the quoted `]` is a literal member; the later
        // unquoted `]` closes the expression, so this is a glob.
        let mut f = ExpandedField::new();
        f.push_expanded("[a");
        f.push_quoted("]");
        f.push_expanded("]");
        assert!(has_unquoted_glob_chars(&f));
    }

    #[test]
    fn fully_unquoted_bracket_still_globs_after_mask_fix() {
        // Regression guard: fully unquoted patterns keep globbing.
        let mut f = ExpandedField::new();
        f.push_expanded("src[c]x");
        assert!(has_unquoted_glob_chars(&f));
    }

    #[test]
    fn str_has_glob_chars_component_rules() {
        assert!(str_has_glob_chars("*.rs"));
        assert!(str_has_glob_chars("file?"));
        assert!(str_has_glob_chars("[abc]"));
        assert!(!str_has_glob_chars("["));
        assert!(!str_has_glob_chars("a[b"));
        assert!(!str_has_glob_chars("plain"));
    }

    #[test]
    fn test_has_unquoted_glob_chars_false_quoted() {
        assert!(!has_unquoted_glob_chars(&quoted_field("*.rs")));
    }

    #[test]
    fn test_has_unquoted_glob_chars_false_no_meta() {
        assert!(!has_unquoted_glob_chars(&unquoted("hello.rs")));
    }

    // ── Fast-path: multi-field non-glob passthrough ──

    #[test]
    fn test_fast_path_preserves_multiple_non_glob_fields() {
        let env = make_env();
        let input = vec![unquoted("hello"), unquoted("world"), quoted_field("*.rs")];
        let result = expand(&env, input);
        // All three fields must survive intact — "hello" and "world" have no
        // glob chars; "*.rs" is fully-quoted so `has_unquoted_glob_chars`
        // returns false. The multi-field non-glob path is the dominant W2
        // case and this regression guard protects it across the fast-path
        // refactor.
        assert_eq!(values(result), vec!["hello", "world", "*.rs"]);
    }

    #[test]
    fn literal_asterisk_still_globs() {
        // POSIX: literal `*` triggers pathname expansion. push_literal
        // marks bytes split-protected but glob-subject — glob still fires.
        let mut f = ExpandedField::new();
        f.push_literal("*.rs");
        assert!(
            has_unquoted_glob_chars(&f),
            "literal '*' must be glob-subject"
        );
    }

    #[test]
    fn quoted_asterisk_does_not_glob() {
        // Regression pin: push_quoted marks bytes both split- and
        // glob-protected, so `*` is treated as literal.
        let mut f = ExpandedField::new();
        f.push_quoted("*.rs");
        assert!(
            !has_unquoted_glob_chars(&f),
            "quoted '*' must NOT be glob-subject"
        );
    }

    #[test]
    fn expanded_asterisk_globs() {
        // Sanity: expansion result with `*` is also glob-subject.
        let mut f = ExpandedField::new();
        f.push_expanded("*.rs");
        assert!(
            has_unquoted_glob_chars(&f),
            "expanded '*' must be glob-subject"
        );
    }

    #[test]
    fn multibyte_literal_star_remains_glob_subject() {
        let mut f = ExpandedField::new();
        f.push_literal("日*");

        assert_eq!(f.byte_len(), "日*".len());
        assert!(has_unquoted_glob_chars(&f));
    }

    #[test]
    fn multibyte_quoted_star_is_glob_protected() {
        let mut f = ExpandedField::new();
        f.push_quoted("日*");

        assert_eq!(f.byte_len(), "日*".len());
        assert!(!has_unquoted_glob_chars(&f));
    }

    #[test]
    fn multibyte_expanded_question_remains_glob_subject() {
        let mut f = ExpandedField::new();
        f.push_expanded("本?");

        assert_eq!(f.byte_len(), "本?".len());
        assert!(has_unquoted_glob_chars(&f));
    }
}
