use crate::env::ShellEnv;
use super::{ExpandedField, pattern};

// ─── Public API ─────────────────────────────────────────────────────────────

/// Perform pathname expansion (glob) on each field.
///
/// Rules (POSIX 2.6.6):
/// 1. If a field contains an unquoted `*`, `?`, or `[`, attempt glob expansion.
/// 2. If one or more filesystem paths match, replace the field with those
///    matches (sorted, each marked fully-quoted so they are not re-split).
/// 3. If no match is found, keep the original field unchanged.
/// 4. `*` and `?` do NOT match a leading `.` unless the pattern starts with `.`.
/// 5. `*` and `?` never match `/`.
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
                // No match — keep original field unchanged.
                result.push(field);
            } else {
                for m in matches {
                    result.push(ExpandedField {
                        quoted_mask: vec![true; m.len()],
                        value: m,
                    });
                }
            }
        } else {
            result.push(field);
        }
    }
    result
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Return `true` if the field contains at least one unquoted glob metachar
/// (`*`, `?`, `[`).
fn has_unquoted_glob_chars(field: &ExpandedField) -> bool {
    let bytes = field.value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if !field.quoted_mask[i] && matches!(b, b'*' | b'?' | b'[') {
            return true;
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
fn glob_match(pattern: &str) -> Vec<String> {
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

    // Determine whether the component has glob chars.
    let is_glob = component.contains(['*', '?', '[']);

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
                if std::fs::metadata(&full)
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
            if std::path::Path::new(&full).exists() {
                vec![full]
            } else {
                vec![]
            }
        } else {
            expand_components(full, rest)
        }
    }
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
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let skip_hidden = !pattern.starts_with('.');

    let mut matches = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // POSIX: `*` and `?` do not match a leading dot.
        if skip_hidden && name_str.starts_with('.') {
            continue;
        }

        if pattern::matches(pattern, &name_str) {
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

    fn make_env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    fn unquoted(s: &str) -> ExpandedField {
        let mut f = ExpandedField::new();
        f.push_unquoted(s);
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
        let env = make_env();
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

    // ── has_unquoted_glob_chars ──

    #[test]
    fn test_has_unquoted_glob_chars_true() {
        assert!(has_unquoted_glob_chars(&unquoted("*.rs")));
        assert!(has_unquoted_glob_chars(&unquoted("file?.txt")));
        assert!(has_unquoted_glob_chars(&unquoted("[abc]")));
    }

    #[test]
    fn test_has_unquoted_glob_chars_false_quoted() {
        assert!(!has_unquoted_glob_chars(&quoted_field("*.rs")));
    }

    #[test]
    fn test_has_unquoted_glob_chars_false_no_meta() {
        assert!(!has_unquoted_glob_chars(&unquoted("hello.rs")));
    }
}
