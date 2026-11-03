use super::ExpandedField;
use crate::env::ShellEnv;

// ─── IFS helper ─────────────────────────────────────────────────────────────

/// The POSIX default IFS value used when the variable is unset.
const DEFAULT_IFS: &str = " \t\n";

/// The whitespace-delimiter byte set for `DEFAULT_IFS` (all three bytes are
/// IFS-whitespace; there are no non-whitespace delimiters).
const DEFAULT_WS: &[u8] = b" \t\n";

/// Return the IFS value by reference — no allocation:
///   - If IFS is set (even empty), return that string.
///   - If IFS is unset, return the POSIX default `" \t\n"`.
fn get_ifs(env: &ShellEnv) -> &str {
    env.vars.get("IFS").unwrap_or(DEFAULT_IFS)
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
            .filter(|f| f.byte_len() != 0 || f.was_quoted)
            .collect();
    }

    // Fast path: when IFS is the POSIX default `" \t\n"`, the whitespace/
    // non-whitespace partition is known statically (all-whitespace, no
    // non-whitespace delimiters) — skip building the `ifs_ws`/`ifs_nws`
    // Vecs entirely and check directly against the default set. This is
    // the overwhelmingly common case (unset or default IFS) and avoids
    // two small heap allocations per word expansion even before reaching
    // the "does any field need splitting" check below.
    if ifs == DEFAULT_IFS {
        let delims = Delims::new(DEFAULT_WS, &[], Vec::new());
        if fields.iter().all(|f| !needs_splitting(f, &delims)) {
            return fields;
        }
        let mut result = Vec::new();
        for field in fields {
            split_field(&field, &delims, &mut result);
        }
        return result;
    }

    // Partition IFS characters.
    let ifs_ws: Vec<u8> = ifs
        .bytes()
        .filter(|b| matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();
    // ASCII non-whitespace delimiters go in the byte table. Non-ASCII IFS
    // characters (UTF-8 characters, and raw bytes held as byteenc escape
    // characters) are matched as whole characters, never byte by byte:
    // a single byte would match inside other multi-byte characters and
    // break split_field's "i is a char boundary" invariant (spec
    // 2026-04-21-field-split-utf8-panic-fix §4.1 / §5.2).
    let ifs_nws: Vec<u8> = ifs
        .bytes()
        .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
        .filter(|b| *b < 0x80)
        .collect();
    let mut ifs_multi: Vec<String> = Vec::new();
    for c in ifs.chars().filter(|c| !c.is_ascii()) {
        ifs_multi.push(String::from(c));
        // A valid character can also reach a field as its bytes held one
        // by one in byteenc escapes, when they were produced separately
        // (`$(printf 'a\303')$(printf '\251b')`): match that spelling
        // too. Escapes and raw UTF-8 never mix within one character (a
        // raw continuation byte cannot occur alone in a `String`), so
        // these two spellings are the only ones.
        if crate::byteenc::unescape_char(c).is_none() {
            let mut buf = [0u8; 4];
            let escaped: String = c
                .encode_utf8(&mut buf)
                .bytes()
                .map(crate::byteenc::escape_char)
                .collect();
            ifs_multi.push(escaped);
        }
    }

    // Fast path: if no field contains an unquoted IFS byte, the state
    // machine would emit each input field unchanged. Return without
    // allocating a new Vec or rebuilding ExpandedFields via `emit`.
    let delims = Delims::new(&ifs_ws, &ifs_nws, ifs_multi);
    if fields.iter().all(|f| !needs_splitting(f, &delims)) {
        return fields;
    }

    let mut result = Vec::new();
    for field in fields {
        split_field(&field, &delims, &mut result);
    }
    result
}

/// Byte classification table for one IFS value: IFS whitespace, IFS
/// non-whitespace, not a delimiter, or the lead byte of a non-ASCII IFS
/// character (`multi`).
struct Delims {
    table: [u8; 256],
    /// The non-ASCII IFS characters, as UTF-8. They are IFS non-whitespace
    /// (IFS white space is space, tab and newline).
    multi: Vec<String>,
}

const NOT_DELIM: u8 = 0;
const WS_DELIM: u8 = 1;
const NWS_DELIM: u8 = 2;
/// Lead byte of a non-ASCII IFS character: a delimiter only where the
/// whole character follows.
const MB_LEAD: u8 = 3;

impl Delims {
    fn new(ifs_ws: &[u8], ifs_nws: &[u8], multi: Vec<String>) -> Self {
        let mut t = [NOT_DELIM; 256];
        for &b in ifs_nws {
            t[b as usize] = NWS_DELIM;
        }
        for &b in ifs_ws {
            t[b as usize] = WS_DELIM;
        }
        for d in &multi {
            t[d.as_bytes()[0] as usize] = MB_LEAD;
        }
        Delims { table: t, multi }
    }

    /// Class of the text at byte `i` of `field` (a char boundary) and the
    /// delimiter's length in bytes: quoted bytes are never delimiters.
    #[inline]
    fn class(&self, field: &ExpandedField, i: usize) -> (u8, usize) {
        let c = self.table[field.as_bytes()[i] as usize];
        if c == NOT_DELIM || field.is_split_protected(i) {
            return (NOT_DELIM, 1);
        }
        if c == MB_LEAD {
            let rest = &field.as_bytes()[i..];
            // The whole delimiter must be unquoted: an escaped-byte
            // spelling spans several characters, which quoting can split.
            return match self.multi.iter().find(|d| {
                rest.starts_with(d.as_bytes()) && !field.is_split_protected(i + d.len() - 1)
            }) {
                Some(d) => (NWS_DELIM, d.len()),
                None => (NOT_DELIM, 1),
            };
        }
        (c, 1)
    }
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
fn split_field(field: &ExpandedField, delims: &Delims, out: &mut Vec<ExpandedField>) {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Start,
        InField,
        AfterWs,
        AfterNws,
    }

    let len = field.byte_len();

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
        let (class, dlen) = delims.class(field, i);
        let is_ws = class == WS_DELIM;
        let is_nws = class == NWS_DELIM;

        match state {
            State::Start | State::AfterNws => {
                if is_ws {
                    // Skip leading / trailing whitespace around a delimiter.
                    i += dlen;
                    // Stay in Start/AfterNws to keep skipping whitespace.
                } else if is_nws {
                    // An IFS non-whitespace delimiter immediately after
                    // Start/AfterNws → emit an empty field.
                    out.push(ExpandedField {
                        was_quoted: true,
                        ..ExpandedField::new()
                    });
                    state = State::AfterNws;
                    i += dlen;
                } else {
                    // Normal byte: start accumulating a field.
                    i = append_run(&mut current, field, i, delims);
                    state = State::InField;
                }
            }

            State::InField => {
                if is_ws {
                    // IFS whitespace: end current field, enter AfterWs.
                    emit(&mut current, out);
                    state = State::AfterWs;
                    i += dlen;
                } else if is_nws {
                    // IFS non-whitespace: end current field, enter AfterNws.
                    emit(&mut current, out);
                    state = State::AfterNws;
                    i += dlen;
                } else {
                    i = append_run(&mut current, field, i, delims);
                }
            }

            State::AfterWs => {
                if is_ws {
                    // Consecutive whitespace — skip.
                    i += dlen;
                } else if is_nws {
                    // Whitespace followed by non-whitespace delimiter:
                    // the whitespace we already consumed was the leading ws
                    // of this nws delimiter.  Do NOT emit an extra empty
                    // field; the field before the ws was already emitted.
                    state = State::AfterNws;
                    i += dlen;
                } else {
                    // Normal byte after whitespace: start a new field.
                    i = append_run(&mut current, field, i, delims);
                    state = State::InField;
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
fn needs_splitting(field: &ExpandedField, delims: &Delims) -> bool {
    (0..field.byte_len()).any(|i| delims.class(field, i).0 != NOT_DELIM)
}

/// Append to `dest` the run of non-delimiter bytes starting at `i` that
/// share byte `i`'s `(split_protected, glob_protected)` attribute pair,
/// via the push method matching that classification; returns the run's
/// end. Runs end at a delimiter byte (ASCII, or the lead byte of a
/// non-ASCII IFS character) or where the attributes change — all char
/// boundaries, since lead bytes are and every push covers whole strings.
///
/// Caller must ensure `i` is on a UTF-8 character boundary and is not a
/// delimiter.
#[inline]
fn append_run(
    dest: &mut ExpandedField,
    source: &ExpandedField,
    i: usize,
    delims: &Delims,
) -> usize {
    let split = source.is_split_protected(i);
    let glob = source.is_glob_protected(i);
    let len = source.byte_len();
    let mut j = i + 1;
    while j < len
        && source.is_split_protected(j) == split
        && source.is_glob_protected(j) == glob
        && (split || delims.table[source.as_bytes()[j] as usize] == NOT_DELIM)
    {
        j += 1;
    }
    debug_assert!(source.value.is_char_boundary(j));
    let slice = &source.value[i..j];
    match (split, glob) {
        (true, true) => dest.push_quoted(slice),
        (true, false) => dest.push_literal(slice),
        (false, false) => dest.push_expanded(slice),
        // Reserved for a future per-byte attribute that decouples
        // glob-protection from split-protection (e.g. an unquoted brace-
        // expansion region whose glob status is overridden separately).
        // Currently unreachable through `push_quoted` / `push_literal` /
        // `push_expanded`. We route as expanded rather than panicking so
        // an isolated mask-mutation bug elsewhere can't crash expansion.
        (false, true) => dest.push_expanded(slice),
    }
    j
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
        empty.push_expanded("");
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
        f.push_expanded("foo ");
        f.push_quoted("bar baz");
        f.push_expanded(" qux");
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

    // ── Default-IFS static fast path (Task 2 PERF item 8) ──

    #[test]
    fn test_default_ifs_static_path_unset_no_split_needed() {
        // Unset IFS resolves to DEFAULT_IFS and must take the static
        // whitespace-only branch without allocating ifs_ws/ifs_nws Vecs.
        let env = env_no_ifs();
        let input = vec![unquoted("hello")];
        assert_eq!(values(split(&env, input)), vec!["hello"]);
    }

    #[test]
    fn test_default_ifs_static_path_unset_splits_on_whitespace() {
        let env = env_no_ifs();
        let input = vec![unquoted("  hello   world  ")];
        assert_eq!(values(split(&env, input)), vec!["hello", "world"]);
    }

    #[test]
    fn test_default_ifs_static_path_explicit_default_string() {
        // IFS explicitly set to the literal default string " \t\n" must
        // behave identically to unset IFS via the same static branch.
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("a\tb\nc")];
        assert_eq!(values(split(&env, input)), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_default_ifs_static_path_quoted_whitespace_preserved() {
        let env = env_no_ifs();
        let mut f = ExpandedField::new();
        f.push_quoted("a b");
        assert_eq!(values(split(&env, vec![f])), vec!["a b"]);
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
        f.push_expanded("foo");
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
        empty.push_expanded("");
        let result = split(&env, vec![empty, unquoted("hello")]);
        assert_eq!(result.len(), 2);
        assert!(result[0].value.is_empty());
        assert!(!result[0].was_quoted);
        assert_eq!(result[1].value, "hello");
    }

    #[test]
    fn test_fast_path_utf8_no_false_positive() {
        // Deferred from spec 2026-04-21-field-split-fast-path-design §9.1 until
        // the slow-path UTF-8 panic (2026-04-21-field-split-utf8-panic-fix) was
        // resolved. UTF-8 continuation bytes (0x80-0xBF) cannot collide with
        // ASCII IFS bytes, so the fast path must engage for multi-byte-only
        // input with no unquoted IFS byte.
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("日本語")];
        assert_eq!(values(split(&env, input)), vec!["日本語"]);
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

    #[test]
    fn test_utf8_content_colon_delimiter() {
        // Non-whitespace ASCII IFS surrounding multi-byte content.
        let env = env_with_ifs(":");
        let input = vec![unquoted("a:日:b")];
        assert_eq!(values(split(&env, input)), vec!["a", "日", "b"]);
    }

    #[test]
    fn test_utf8_quoted_not_split() {
        // Quoted multi-byte content including an IFS byte must stay intact.
        let env = env_with_ifs(" ");
        let input = vec![quoted_field("日 本")];
        assert_eq!(values(split(&env, input)), vec!["日 本"]);
    }

    #[test]
    fn test_utf8_leading_trailing_whitespace_around_multibyte() {
        // Trailing/leading IFS-whitespace collapse around multi-byte content.
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("  日本語  ")];
        assert_eq!(values(split(&env, input)), vec!["日本語"]);
    }

    #[test]
    fn test_non_ascii_ifs_char_splits_as_whole_char() {
        // A non-ASCII IFS character is a delimiter as a whole character
        // (dash/bash split on it); 'À' (0xC3 0x80) and 'é' (0xC3 0xA9)
        // share a lead byte, so 'é' must not match.
        let env = env_with_ifs("\u{00c0}");
        let input = vec![unquoted("aÀbéÀ日"), unquoted("À")];
        assert_eq!(values(split(&env, input)), vec!["a", "bé", "日", ""]);
    }

    #[test]
    fn test_non_ascii_ifs_char_quoted_not_split() {
        let env = env_with_ifs("é");
        let input = vec![quoted_field("aéb")];
        assert_eq!(values(split(&env, input)), vec!["aéb"]);
    }

    #[test]
    fn test_raw_byte_ifs_splits_on_that_byte() {
        // A raw (non-UTF-8) byte in IFS and in the value are both byteenc
        // escape characters; they match each other and nothing else.
        let ff = crate::byteenc::encode_bytes(b"\xff").into_owned();
        let fe = crate::byteenc::encode_bytes(b"\xfe").into_owned();
        let env = env_with_ifs(&ff);
        let input = vec![unquoted(&format!("a{ff}b{fe}c"))];
        assert_eq!(
            values(split(&env, input)),
            vec!["a".to_string(), format!("b{fe}c")]
        );
    }

    #[test]
    fn test_utf8_ifs_matches_escaped_byte_spelling() {
        // `$(printf 'a\303')$(printf '\251b')`: each half is decoded on
        // its own, so `é` arrives as two byteenc escapes. It is still the
        // IFS character `é`, as in dash and bash.
        let c3 = crate::byteenc::encode_bytes(b"\xc3").into_owned();
        let a9 = crate::byteenc::encode_bytes(b"\xa9").into_owned();
        let env = env_with_ifs("é");
        let input = vec![unquoted(&format!("a{c3}{a9}b"))];
        assert_eq!(values(split(&env, input)), vec!["a", "b"]);
        // Non-ASCII IFS characters match whole (unlike dash's byte-wise
        // IFS): a lone escaped lead byte is not the character.
        let input = vec![unquoted(&format!("a{c3}b"))];
        assert_eq!(values(split(&env, input)), vec![format!("a{c3}b")]);
        // Quoting the second byte keeps the character from splitting.
        let mut f = ExpandedField::new();
        f.push_expanded(&format!("a{c3}"));
        f.push_quoted(&format!("{a9}b"));
        assert_eq!(values(split(&env, vec![f])), vec![format!("a{c3}{a9}b")]);
    }

    // ── Literal bytes are not split (POSIX XCU §2.6.5) ──

    #[test]
    fn test_literal_colon_not_split() {
        // SP3 follow-up #1: literal text must not be field-split.
        // Pins the field_split predicate behavior directly, independent
        // of the push_literal wiring in pipeline.rs (landed in addff32).
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("a::b");
        assert_eq!(values(split(&env, vec![f])), vec!["a::b"]);
    }

    #[test]
    fn test_literal_then_expansion_split_only_in_expansion() {
        // Mixed origin: literal "a::" (protected) + expansion "b:c" (subject).
        // POSIX result: ["a::b", "c"].
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("a::");
        f.push_expanded("b:c");
        assert_eq!(values(split(&env, vec![f])), vec!["a::b", "c"]);
    }

    #[test]
    fn test_literal_then_expansion_then_literal_round_trip() {
        // L "x:" + E "a:b" + L ":y" with IFS=":"
        // Expected fields: ["x:a", "b:y"]
        //   - literal "x:" — both bytes split-protected, stay together
        //   - expansion "a:b" — only the unquoted ':' splits → ends field 1 ("x:a"), starts field 2 ("b")
        //   - literal ":y" — both bytes split-protected, attach to current field → "b:y"
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("x:");
        f.push_expanded("a:b");
        f.push_literal(":y");
        assert_eq!(values(split(&env, vec![f])), vec!["x:a", "b:y"]);
    }

    #[test]
    fn split_preserves_quoted_multibyte_masks() {
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_quoted("日:本");
        f.push_expanded(":x");

        let result = split(&env, vec![f]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, "日:本");
        for i in 0..result[0].byte_len() {
            assert!(result[0].is_split_protected(i), "byte {i} split-protected");
            assert!(result[0].is_glob_protected(i), "byte {i} glob-protected");
        }
        assert_eq!(result[1].value, "x");
    }

    #[test]
    fn split_preserves_literal_multibyte_as_split_protected_glob_subject() {
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("日:本*");
        f.push_expanded(":x");

        let result = split(&env, vec![f]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, "日:本*");
        for i in 0..result[0].byte_len() {
            assert!(result[0].is_split_protected(i), "byte {i} split-protected");
            assert!(!result[0].is_glob_protected(i), "byte {i} glob-subject");
        }
        assert_eq!(result[1].value, "x");
    }

    #[test]
    fn split_literal_glob_metachar_stays_glob_subject() {
        // append_char (split=true, glob=false) → push_literal arm: the glob
        // mask must survive field splitting so downstream pathname expansion
        // still globs the literal `*`. A regression routing this byte through
        // push_expanded would keep the same value but change the mask.
        let env = env_with_ifs(" ");
        let mut f = ExpandedField::new();
        f.push_literal("a*");
        f.push_expanded(" b");

        let result = split(&env, vec![f]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, "a*");
        for i in 0..result[0].byte_len() {
            assert!(result[0].is_split_protected(i), "byte {i} split-protected");
            assert!(!result[0].is_glob_protected(i), "byte {i} glob-subject");
        }
        assert_eq!(result[1].value, "b");
    }

    #[test]
    fn split_expanded_multibyte_remains_split_subject() {
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_expanded("日:本");

        let result = split(&env, vec![f]);
        assert_eq!(values(result), vec!["日", "本"]);
    }
}
