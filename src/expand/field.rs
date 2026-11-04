//! `ExpandedField`: an expanded word with per-byte quoting attributes,
//! carried from expansion through field splitting and pathname
//! expansion to quote removal.

/// A word that has been through parameter/command/arithmetic expansion.
/// Each byte has two independent attribute bits:
///   - `split_protected_mask`: byte must NOT be split on an IFS character
///     (set for quoted bytes and for `Literal`-origin bytes).
///   - `glob_protected_mask`: byte must NOT be treated as a glob metachar
///     (set for quoted bytes only; `Literal`-origin bytes are glob-subject).
///
/// The two masks are independent. The POSIX byte classification is:
///   - `push_quoted`   → split-protected, glob-protected, was_quoted=true
///   - `push_literal`  → split-protected, glob-subject  (POSIX: literal text)
///   - `push_expanded` → split-subject,   glob-subject  (POSIX: $var, $(...), $((...)))
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// Packed bitset: 1 bit per byte. Bit set = protected from IFS splitting.
    pub(super) split_protected_mask: Vec<u64>,
    /// Packed bitset: 1 bit per byte. Bit set = protected from glob expansion.
    pub(super) glob_protected_mask: Vec<u64>,
    /// True if any quoting context applied to this field (even if value is empty).
    /// POSIX requires that quoted empty strings like `''` and `""` produce a
    /// zero-length field rather than being removed.
    pub was_quoted: bool,
    /// Separator placed *before* this field when a scalar (non-splitting)
    /// context flattens multiple fields into one string. Unquoted `$*`
    /// records IFS[0] here on its second and later fields (POSIX §2.5.2:
    /// in contexts where field splitting is not performed, `$*` joins with
    /// the first IFS character); `None` joins with the historical space.
    pub scalar_join_sep: Option<String>,
    /// True once quoted *content* was pushed into this field (`push_quoted`,
    /// including a quoted empty string like `''`). Distinct from
    /// `was_quoted`, which the `DoubleQuoted` wrapper also sets before its
    /// inner parts expand: a zero-parameter `"$@"` must drop the field only
    /// when nothing quoted actually contributed to it.
    pub(crate) quoted_content: bool,
    /// Byte offsets (ascending) where a quoted empty string (`""`, `''`,
    /// `"$empty"`) sits in `value`. It adds no bytes but is still field
    /// content: field splitting keeps a field there even when IFS
    /// delimiters surround it (`x='a '; set -- $x""` is two fields, the
    /// second empty — dash and bash agree).
    pub(crate) empty_quotes: Vec<usize>,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            split_protected_mask: Vec::new(),
            glob_protected_mask: Vec::new(),
            was_quoted: false,
            scalar_join_sep: None,
            quoted_content: false,
            empty_quotes: Vec::new(),
        }
    }

    /// Record a quoted empty string at the current end of the value.
    fn mark_empty_quote(&mut self) {
        self.push_empty_quote(self.value.len());
    }

    /// Record a quoted empty string at byte offset `at`, which is at or
    /// past every offset recorded so far.
    fn push_empty_quote(&mut self, at: usize) {
        if self.empty_quotes.last() != Some(&at) {
            self.empty_quotes.push(at);
        }
    }

    /// True iff byte `i` must not be split on an IFS character.
    pub fn is_split_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.split_protected_mask, byte_index)
    }

    /// True iff byte `i` must not be treated as a glob metacharacter.
    pub fn is_glob_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.glob_protected_mask, byte_index)
    }

    /// Append `s` from a quoted context (single quotes, double quotes,
    /// escaped chars, tilde expansion). Bytes are protected from both
    /// field splitting and glob expansion; `was_quoted` becomes true.
    pub fn push_quoted(&mut self, s: &str) {
        if s.is_empty() {
            self.mark_empty_quote();
        }
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        set_mask_range(&mut self.glob_protected_mask, start, s.len());
        self.was_quoted = true;
        self.quoted_content = true;
    }

    /// Append `s` from a literal (un-quoted, non-expansion) text part.
    /// Bytes are protected from field splitting (POSIX XCU §2.6.5 restricts
    /// splitting to expansion results only) but remain glob-subject so that
    /// a literal `*.rs` still triggers pathname expansion.
    pub fn push_literal(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        // glob_protected_mask intentionally not touched: literal bytes remain
        // glob-subject.
    }

    /// Append `s` from an expansion (parameter, command sub, arithmetic).
    /// Bytes are subject to both field splitting and glob expansion
    /// (POSIX XCU §2.6.5: only expansion results are split).
    ///
    /// Neither mask is updated: the predicates fall back to `false` (subject)
    /// when reading past the mask end, so leaving the bits implicit is both
    /// correct and avoids per-push allocation.
    pub fn push_expanded(&mut self, s: &str) {
        self.value.push_str(s);
    }

    /// Append another field's bytes, carrying over each byte's protection
    /// bits and the `was_quoted` flag (used when a nested expansion produced
    /// its own fields that must merge into the current word).
    pub fn append_field(&mut self, other: &ExpandedField) {
        let start = self.value.len();
        self.value.push_str(&other.value);
        let len = other.byte_len();
        or_mask_shifted(
            &mut self.split_protected_mask,
            &other.split_protected_mask,
            len,
            start,
        );
        or_mask_shifted(
            &mut self.glob_protected_mask,
            &other.glob_protected_mask,
            len,
            start,
        );
        self.was_quoted |= other.was_quoted;
        self.quoted_content |= other.quoted_content;
        for &at in &other.empty_quotes {
            self.push_empty_quote(start + at);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// The separator a scalar (non-splitting) context puts before this
    /// field when it flattens several fields into one string: its
    /// `scalar_join_sep`, else a space.
    pub(crate) fn scalar_separator(&self) -> &str {
        self.scalar_join_sep.as_deref().unwrap_or(" ")
    }

    /// Return the field value as bytes.
    ///
    /// Stage 1 still stores valid UTF-8 in `String`; callers that reason about
    /// split/glob masks should go through this byte-facing API so later raw
    /// byte storage has one obvious migration point.
    pub fn as_bytes(&self) -> &[u8] {
        self.value.as_bytes()
    }

    /// Return the field length in bytes, matching mask indexing.
    pub fn byte_len(&self) -> usize {
        self.value.len()
    }

    /// Consume the field and return its current UTF-8 value.
    pub fn into_string(self) -> String {
        self.value
    }

    /// Create a field with all bytes marked protected from both splitting
    /// and glob expansion (used for glob-match results that must not be
    /// re-split or re-globbed).
    pub fn all_quoted(value: String) -> Self {
        // The two masks are independent state; build each one directly
        // rather than cloning one into the other.
        let needed_words = value.len().div_ceil(64);
        Self {
            split_protected_mask: vec![u64::MAX; needed_words],
            glob_protected_mask: vec![u64::MAX; needed_words],
            value,
            was_quoted: false,
            scalar_join_sep: None,
            quoted_content: false,
            empty_quotes: Vec::new(),
        }
    }
}

#[inline]
fn bit_set(mask: &[u64], byte_index: usize) -> bool {
    let word = byte_index / 64;
    let bit = byte_index % 64;
    mask.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
}

#[inline]
fn set_mask_range(mask: &mut Vec<u64>, start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let end = start + len;
    let needed_words = end.div_ceil(64);
    if mask.len() < needed_words {
        mask.resize(needed_words, 0);
    }
    // Word-aligned fill: head word, full middle words, tail word. Long
    // quoted runs (e.g. `"$big"` re-pushed in a concatenation loop) used
    // to pay one shift per byte, which made `s="$s$x"` loops quadratic.
    let first = start / 64;
    let last = (end - 1) / 64;
    let lo = start % 64;
    let hi = end - last * 64; // 1..=64
    let tail_bits = if hi == 64 { u64::MAX } else { (1u64 << hi) - 1 };
    if first == last {
        mask[first] |= tail_bits & (u64::MAX << lo);
    } else {
        mask[first] |= u64::MAX << lo;
        for w in &mut mask[first + 1..last] {
            *w = u64::MAX;
        }
        mask[last] |= tail_bits;
    }
}

/// OR `src` (a mask covering `src_len` bytes) into `dst` at byte offset
/// `start`, shifting whole words instead of copying bit by bit.
fn or_mask_shifted(dst: &mut Vec<u64>, src: &[u64], src_len: usize, start: usize) {
    if src_len == 0 || src.is_empty() {
        return;
    }
    let needed_words = (start + src_len).div_ceil(64);
    if dst.len() < needed_words {
        dst.resize(needed_words, 0);
    }
    let shift = start % 64;
    let base = start / 64;
    let src_words = src_len.div_ceil(64).min(src.len());
    for (k, &w) in src[..src_words].iter().enumerate() {
        // Clear bits past `src_len` (all_quoted fills whole words).
        let valid = src_len - k * 64;
        let w = if valid >= 64 {
            w
        } else {
            w & ((1u64 << valid) - 1)
        };
        if w == 0 {
            continue;
        }
        dst[base + k] |= w << shift;
        if shift != 0 && base + k + 1 < dst.len() {
            dst[base + k + 1] |= w >> (64 - shift);
        }
    }
}

impl Default for ExpandedField {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_mask_range_matches_per_bit_reference() {
        for &(start, len) in &[
            (0, 1),
            (0, 64),
            (0, 65),
            (3, 60),
            (3, 61),
            (63, 1),
            (63, 2),
            (64, 64),
            (5, 200),
            (130, 7),
        ] {
            let mut fast = Vec::new();
            set_mask_range(&mut fast, start, len);
            let mut slow: Vec<u64> = vec![0; (start + len).div_ceil(64)];
            for i in start..start + len {
                slow[i / 64] |= 1u64 << (i % 64);
            }
            assert_eq!(fast, slow, "start={start} len={len}");
        }
    }

    #[test]
    fn set_mask_range_preserves_existing_bits() {
        let mut m = Vec::new();
        set_mask_range(&mut m, 1, 1);
        set_mask_range(&mut m, 70, 3);
        assert!(bit_set(&m, 1) && !bit_set(&m, 0) && !bit_set(&m, 2));
        assert!(bit_set(&m, 70) && bit_set(&m, 72) && !bit_set(&m, 73) && !bit_set(&m, 69));
    }

    #[test]
    fn append_field_carries_masks_across_word_boundaries() {
        for &(prefix_len, quoted_len, plain_len) in
            &[(0, 3, 2), (61, 5, 70), (64, 64, 1), (100, 130, 3)]
        {
            let mut f = ExpandedField::new();
            f.push_expanded(&"p".repeat(prefix_len));
            let mut other = ExpandedField::new();
            other.push_quoted(&"q".repeat(quoted_len));
            other.push_expanded(&"e".repeat(plain_len));
            f.append_field(&other);
            for i in 0..f.byte_len() {
                let expect = i >= prefix_len && i < prefix_len + quoted_len;
                assert_eq!(
                    f.is_split_protected(i),
                    expect,
                    "split bit {i} ({prefix_len},{quoted_len},{plain_len})"
                );
                assert_eq!(f.is_glob_protected(i), expect, "glob bit {i}");
            }
        }
    }

    #[test]
    fn append_field_all_quoted_does_not_leak_past_length() {
        let mut f = ExpandedField::new();
        f.push_expanded("ab");
        f.append_field(&ExpandedField::all_quoted("xyz".to_string()));
        f.push_expanded("tail");
        for i in 0..f.byte_len() {
            assert_eq!(f.is_split_protected(i), (2..5).contains(&i), "bit {i}");
        }
    }

    #[test]
    fn push_literal_marks_split_protected_only() {
        let mut f = ExpandedField::new();
        f.push_literal("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn push_quoted_marks_both_and_was_quoted() {
        let mut f = ExpandedField::new();
        f.push_quoted("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(f.is_glob_protected(0));
        assert!(f.is_glob_protected(1));
        assert!(f.was_quoted);
    }

    #[test]
    fn push_expanded_marks_neither() {
        let mut f = ExpandedField::new();
        f.push_expanded("ab");
        assert!(!f.is_split_protected(0));
        assert!(!f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn expanded_field_byte_access_reports_utf8_bytes() {
        let mut f = ExpandedField::new();
        f.push_literal("日*");

        assert_eq!(f.byte_len(), "日*".len());
        assert_eq!(f.as_bytes(), "日*".as_bytes());
        assert_eq!(f.into_string(), "日*");
    }

    #[test]
    fn expanded_field_masks_are_indexed_by_byte() {
        let mut f = ExpandedField::new();
        f.push_quoted("日");
        f.push_literal("*");
        f.push_expanded(" 本");

        assert_eq!(f.byte_len(), "日* 本".len());

        for i in 0.."日".len() {
            assert!(f.is_split_protected(i), "byte {i} split-protected");
            assert!(f.is_glob_protected(i), "byte {i} glob-protected");
        }

        let star = "日".len();
        assert!(f.is_split_protected(star));
        assert!(!f.is_glob_protected(star));

        let expanded_start = star + 1;
        for i in expanded_start..f.byte_len() {
            assert!(!f.is_split_protected(i), "byte {i} split-subject");
            assert!(!f.is_glob_protected(i), "byte {i} glob-subject");
        }
    }

    #[test]
    fn mixed_push_per_byte_independence() {
        // L|E|Q sequence — verify each byte keeps its own attributes
        let mut f = ExpandedField::new();
        f.push_literal("a");
        f.push_expanded("b");
        f.push_quoted("c");
        assert_eq!(f.value, "abc");
        assert!(f.is_split_protected(0) && !f.is_glob_protected(0)); // a: literal
        assert!(!f.is_split_protected(1) && !f.is_glob_protected(1)); // b: expanded
        assert!(f.is_split_protected(2) && f.is_glob_protected(2)); // c: quoted
        assert!(f.was_quoted);
    }

    #[test]
    fn all_quoted_marks_both() {
        let f = ExpandedField::all_quoted("abc".to_string());
        for i in 0..3 {
            assert!(f.is_split_protected(i), "byte {i} split-protected");
            assert!(f.is_glob_protected(i), "byte {i} glob-protected");
        }
        assert!(!f.was_quoted);
    }
}
