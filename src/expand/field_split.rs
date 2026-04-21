use super::ExpandedField;
use crate::env::ShellEnv;

// ─── IFS helper ─────────────────────────────────────────────────────────────

/// Return the IFS value:
///   - If IFS is set (even empty), return that string.
///   - If IFS is unset, return the POSIX default `" \t\n"`.
fn get_ifs(env: &ShellEnv) -> String {
    match env.vars.get("IFS") {
        Some(ifs) => ifs.to_string(),
        None => " \t\n".to_string(),
    }
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Split fields according to IFS.
///
/// POSIX rules (XBD Field Splitting):
/// 1. IFS unset → treat as `" \t\n"`.
/// 2. IFS empty → no field splitting; keep non-empty fields as-is.
/// 3. IFS set and non-empty → split on unquoted IFS characters.
pub fn split(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    let ifs = get_ifs(env);

    // IFS empty: no splitting; drop fully-empty unquoted fields.
    if ifs.is_empty() {
        return fields
            .into_iter()
            .filter(|f| !f.value.is_empty() || f.was_quoted)
            .collect();
    }

    // Partition IFS characters.
    let ifs_ws: Vec<u8> = ifs
        .bytes()
        .filter(|b| matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();
    let ifs_nws: Vec<u8> = ifs
        .bytes()
        .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();

    // Fast path: if no field contains an unquoted IFS byte, the state
    // machine would emit each input field unchanged. Return without
    // allocating a new Vec or rebuilding ExpandedFields via `emit`.
    if fields.iter().all(|f| !needs_splitting(f, &ifs_ws, &ifs_nws)) {
        return fields;
    }

    let mut result = Vec::new();
    for field in fields {
        split_field(&field, &ifs_ws, &ifs_nws, &mut result);
    }
    result
}

// ─── Core splitter ───────────────────────────────────────────────────────────

/// POSIX IFS splitting state machine.
///
/// States:
///   Start       – at the very beginning (or right after field start); skip
///                 leading IFS-whitespace.
///   InField     – accumulating bytes for the current sub-field.
///   AfterWs     – just consumed one-or-more IFS-whitespace chars.
///   AfterNws    – just consumed an IFS non-whitespace delimiter; leading
///                 whitespace of the next token should be skipped.
fn split_field(field: &ExpandedField, ifs_ws: &[u8], ifs_nws: &[u8], out: &mut Vec<ExpandedField>) {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Start,
        InField,
        AfterWs,
        AfterNws,
    }

    let bytes = field.value.as_bytes();
    let len = bytes.len();

    // A quoted empty field (e.g. '' or "") should be preserved as-is.
    if len == 0 && field.was_quoted {
        out.push(ExpandedField {
            was_quoted: true,
            ..ExpandedField::new()
        });
        return;
    }

    let mut current = ExpandedField::new();
    let mut state = State::Start;

    let mut i = 0;
    while i < len {
        let b = bytes[i];
        let quoted = field.is_quoted(i);

        let is_ws = !quoted && ifs_ws.contains(&b);
        let is_nws = !quoted && ifs_nws.contains(&b);

        match state {
            State::Start | State::AfterNws => {
                if is_ws {
                    // Skip leading / trailing whitespace around a delimiter.
                    i += 1;
                    // Stay in Start/AfterNws to keep skipping whitespace.
                } else if is_nws {
                    // An IFS non-whitespace delimiter immediately after
                    // Start/AfterNws → emit an empty field.
                    out.push(ExpandedField {
                        was_quoted: true,
                        ..ExpandedField::new()
                    });
                    state = State::AfterNws;
                    i += 1;
                } else {
                    // Normal byte: start accumulating.
                    append_byte(&mut current, field, i);
                    state = State::InField;
                    i += 1;
                }
            }

            State::InField => {
                if is_ws {
                    // IFS whitespace: end current field, enter AfterWs.
                    emit(&mut current, out);
                    state = State::AfterWs;
                    i += 1;
                } else if is_nws {
                    // IFS non-whitespace: end current field, enter AfterNws.
                    emit(&mut current, out);
                    state = State::AfterNws;
                    i += 1;
                } else {
                    append_byte(&mut current, field, i);
                    i += 1;
                }
            }

            State::AfterWs => {
                if is_ws {
                    // Consecutive whitespace — skip.
                    i += 1;
                } else if is_nws {
                    // Whitespace followed by non-whitespace delimiter:
                    // the whitespace we already consumed was the leading ws
                    // of this nws delimiter.  Do NOT emit an extra empty
                    // field; the field before the ws was already emitted.
                    state = State::AfterNws;
                    i += 1;
                } else {
                    // Normal byte after whitespace: start a new field.
                    append_byte(&mut current, field, i);
                    state = State::InField;
                    i += 1;
                }
            }
        }
    }

    // Flush the remaining field.
    // POSIX: trailing IFS-whitespace does NOT produce an extra empty field,
    // but trailing IFS non-whitespace DOES (handled via AfterNws emitting
    // empty above — here we only flush non-empty leftovers from InField).
    if !current.is_empty() {
        emit(&mut current, out);
    }
}

/// Return true iff `field` contains at least one unquoted byte that is
/// an IFS delimiter (whitespace or non-whitespace).
///
/// Used by `split()` as a pre-check: if every input field returns false,
/// the slow-path state machine would emit each input field unchanged,
/// so we can return the input Vec as-is without any allocation.
fn needs_splitting(field: &ExpandedField, ifs_ws: &[u8], ifs_nws: &[u8]) -> bool {
    field.value.bytes().enumerate().any(|(i, b)| {
        !field.is_quoted(i) && (ifs_ws.contains(&b) || ifs_nws.contains(&b))
    })
}

/// Append the byte at position `i` in `source` to `dest`, preserving quoting.
#[inline]
fn append_byte(dest: &mut ExpandedField, source: &ExpandedField, i: usize) {
    let ch = &source.value[i..i + 1];
    if source.is_quoted(i) {
        dest.push_quoted(ch);
    } else {
        dest.push_unquoted(ch);
    }
}

/// Push `current` into `out`, replacing `current` with a fresh empty field.
#[inline]
fn emit(current: &mut ExpandedField, out: &mut Vec<ExpandedField>) {
    let done = std::mem::take(current);
    out.push(done);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn env_with_ifs(ifs: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("IFS", ifs).unwrap();
        env
    }

    fn env_no_ifs() -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.unset("IFS").ok();
        env
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

    // ── Basic whitespace split ──

    #[test]
    fn test_split_spaces() {
        let env = env_with_ifs(" ");
        let input = vec![unquoted("hello world foo")];
        assert_eq!(values(split(&env, input)), vec!["hello", "world", "foo"]);
    }

    #[test]
    fn test_consecutive_whitespace() {
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("  hello   world  ")];
        assert_eq!(values(split(&env, input)), vec!["hello", "world"]);
    }

    // ── Quoted bytes are not split ──

    #[test]
    fn test_split_quoted_not_split() {
        let env = env_with_ifs(" ");
        let input = vec![quoted_field("hello world")];
        assert_eq!(values(split(&env, input)), vec!["hello world"]);
    }

    // ── Non-whitespace IFS ──

    #[test]
    fn test_split_colon_delimiter() {
        let env = env_with_ifs(":");
        let input = vec![unquoted("a:b:c")];
        assert_eq!(values(split(&env, input)), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_colon_with_surrounding_whitespace_absorbed() {
        // IFS=" :" — whitespace around `:` should be absorbed.
        let env = env_with_ifs(" :");
        let input = vec![unquoted("a : b : c")];
        assert_eq!(values(split(&env, input)), vec!["a", "b", "c"]);
    }

    // ── Empty IFS ──

    #[test]
    fn test_empty_ifs_no_split() {
        let env = env_with_ifs("");
        let input = vec![unquoted("hello world")];
        assert_eq!(values(split(&env, input)), vec!["hello world"]);
    }

    #[test]
    fn test_empty_ifs_drops_empty_fields() {
        let env = env_with_ifs("");
        let mut empty = ExpandedField::new();
        empty.push_unquoted("");
        let input = vec![empty, unquoted("hello")];
        assert_eq!(values(split(&env, input)), vec!["hello"]);
    }

    // ── Unset IFS ──

    #[test]
    fn test_unset_ifs_default() {
        let env = env_no_ifs();
        let input = vec![unquoted("hello\tworld\nfoo")];
        assert_eq!(values(split(&env, input)), vec!["hello", "world", "foo"]);
    }

    // ── Mixed quoted/unquoted ──

    #[test]
    fn test_mixed_quoted_unquoted() {
        let env = env_with_ifs(" ");
        let mut f = ExpandedField::new();
        f.push_unquoted("foo ");
        f.push_quoted("bar baz");
        f.push_unquoted(" qux");
        let result = split(&env, vec![f]);
        assert_eq!(values(result), vec!["foo", "bar baz", "qux"]);
    }

    // ── Double-colon produces empty field ──

    #[test]
    fn test_double_colon_empty_field() {
        let env = env_with_ifs(":");
        let input = vec![unquoted("a::b")];
        assert_eq!(values(split(&env, input)), vec!["a", "", "b"]);
    }

    // ── Fast-path coverage (§9.1 of fast-path spec) ──

    #[test]
    fn test_fast_path_single_field_no_ifs_chars() {
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("hello")];
        assert_eq!(values(split(&env, input)), vec!["hello"]);
    }

    #[test]
    fn test_fast_path_multiple_fields_no_ifs_chars() {
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("hello"), unquoted("world")];
        assert_eq!(values(split(&env, input)), vec!["hello", "world"]);
    }

    #[test]
    fn test_fast_path_mixed_quoted_unquoted_no_ifs() {
        let env = env_with_ifs(" ");
        let mut f = ExpandedField::new();
        f.push_unquoted("foo");
        f.push_quoted("bar");
        assert_eq!(values(split(&env, vec![f])), vec!["foobar"]);
    }

    #[test]
    fn test_slow_path_triggered_by_one_splittable_field() {
        let env = env_with_ifs(" ");
        let input = vec![unquoted("hello"), unquoted("a b")];
        assert_eq!(values(split(&env, input)), vec!["hello", "a", "b"]);
    }

    #[test]
    fn test_fast_path_quoted_ifs_byte_stays_fast() {
        // IFS byte inside quoted context does not trigger slow path.
        let env = env_with_ifs(" ");
        let mut f = ExpandedField::new();
        f.push_quoted("a b c");
        assert_eq!(values(split(&env, vec![f])), vec!["a b c"]);
    }

    #[test]
    fn test_fast_path_empty_unquoted_field_preserved() {
        // Spec §5.1 documents: slow path drops unquoted empty fields;
        // fast path preserves them. expand_word's final filter
        // (!f.is_empty() || f.was_quoted) drops them before callers see them,
        // so the external contract is identical. This test pins the
        // fast-path-internal behavior so the spec-documented divergence
        // cannot silently widen if a future caller bypasses that filter.
        let env = env_with_ifs(" \t\n");
        let mut empty = ExpandedField::new();
        empty.push_unquoted("");
        let result = split(&env, vec![empty, unquoted("hello")]);
        assert_eq!(result.len(), 2);
        assert!(result[0].value.is_empty());
        assert!(!result[0].was_quoted);
        assert_eq!(result[1].value, "hello");
    }

    // ── UTF-8 multi-byte content (spec 2026-04-21-field-split-utf8-panic-fix §9.1) ──

    #[test]
    fn test_utf8_content_ascii_ifs_splits() {
        // Slow path: ASCII IFS byte present, multi-byte content elsewhere.
        // Pre-fix: panics in append_byte on '日' lead byte.
        let env = env_with_ifs(" ");
        let input = vec![unquoted("日本 語")];
        assert_eq!(values(split(&env, input)), vec!["日本", "語"]);
    }
}
