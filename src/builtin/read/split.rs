//! IFS field splitting of the logical line `read` took in.

use super::LineByte;

/// Assemble a field's bytes into a `String`, decoding as UTF-8 so
/// multi-byte input (e.g. `café`) is preserved. Invalid sequences are
/// preserved losslessly via the byteenc escape encoding.
fn field_to_string(slice: &[LineByte]) -> String {
    let bytes: Vec<u8> = slice.iter().map(|b| b.value).collect();
    crate::byteenc::encode_bytes(&bytes).into_owned()
}

/// One character of the logical line: a valid UTF-8 sequence, or a
/// single byte that does not start one. IFS characters match whole
/// units, so a multibyte IFS character is one delimiter (as in field
/// splitting) and a raw IFS byte matches only that raw input byte.
#[derive(Debug, Clone, Copy)]
struct Unit {
    start: usize,
    end: usize,
    /// The unit's first byte came through a backslash escape.
    escaped: bool,
}

fn line_units(line: &[LineByte]) -> Vec<Unit> {
    let mut units = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        // Only unescaped continuation bytes can extend a sequence.
        let mut buf = [0u8; 4];
        let mut n = 0;
        for (k, b) in line[i..].iter().take(4).enumerate() {
            if k > 0 && b.escaped {
                break;
            }
            buf[k] = b.value;
            n = k + 1;
        }
        let end = i + crate::byteenc::utf8_unit_len(&buf[..n]);
        units.push(Unit {
            start: i,
            end,
            escaped: line[i].escaped,
        });
        i = end;
    }
    units
}

/// Byte classes of `split_fields`'s IFS table.
const NOT_IFS: u8 = 0;
const WS_IFS: u8 = 1;
const SEP_IFS: u8 = 2;

/// POSIX §2.6.5 field splitting for `read`. Returns exactly `n_vars`
/// strings: var[0..n-1] are split fields, var[n-1] is the trailing
/// remainder (with only trailing whitespace-IFS trimmed). When IFS is
/// empty, no splitting occurs.
pub(super) fn split_fields(ifs: &str, line: &[LineByte], n_vars: usize) -> Vec<String> {
    // The only caller (builtin_read) guarantees at least one var name
    // via ArgError::NoVarName.
    debug_assert!(n_vars >= 1);

    // Classify IFS characters. IFS is segmented exactly like the input
    // line (`line_units`), so a character held as several escape
    // characters is still one delimiter and a lone raw byte matches only
    // that raw input byte. One-byte characters (all IFS whitespace, and
    // every character of an ASCII IFS) go in a byte table; the rare
    // multi-byte ones are compared unit by unit.
    let mut class = [NOT_IFS; 256];
    let mut sep_multi: Vec<Vec<u8>> = Vec::new();
    let mut classify = |c: &[u8]| match c {
        [b' ' | b'\t' | b'\n'] => class[c[0] as usize] = WS_IFS,
        [b] => class[*b as usize] = SEP_IFS,
        _ => sep_multi.push(c.to_vec()),
    };
    if ifs.is_ascii() {
        for b in ifs.bytes() {
            classify(&[b]);
        }
    } else {
        for c in crate::byteenc::raw_char_units(ifs) {
            classify(&c);
        }
    }

    // Empty IFS → no splitting at all.
    if ifs.is_empty() {
        let whole = field_to_string(line);
        let mut out = vec![whole];
        out.extend((1..n_vars).map(|_| String::new()));
        return out;
    }

    let units = line_units(line);
    let unit_class = |u: &Unit| -> u8 {
        if u.escaped {
            NOT_IFS
        } else if u.end - u.start == 1 {
            class[line[u.start].value as usize]
        } else if sep_multi.iter().any(|c| {
            c.len() == u.end - u.start
                && line[u.start..u.end]
                    .iter()
                    .zip(c)
                    .all(|(b, x)| b.value == *x)
        }) {
            SEP_IFS
        } else {
            NOT_IFS
        }
    };
    // Helper: is unit `u` an unescaped IFS character of the given class?
    let is_ws = |u: &Unit| unit_class(u) == WS_IFS;
    let is_sep = |u: &Unit| unit_class(u) == SEP_IFS;
    let is_any_ifs = |u: &Unit| unit_class(u) != NOT_IFS;
    // Skip one field terminator starting at `k` and return the index
    // after it. Per XCU §2.6.5 a delimiter is a non-whitespace IFS
    // character together with any adjacent IFS whitespace (on BOTH
    // sides), or a run of IFS whitespace alone: `ws* sep? ws*`.
    let skip_terminator = |units: &[Unit], mut k: usize| -> usize {
        while k < units.len() && is_ws(&units[k]) {
            k += 1;
        }
        if k < units.len() && is_sep(&units[k]) {
            k += 1;
            while k < units.len() && is_ws(&units[k]) {
                k += 1;
            }
        }
        k
    };
    // The text of units[s..e].
    let span = |units: &[Unit], s: usize, e: usize| -> String {
        if s >= e {
            String::new()
        } else {
            field_to_string(&line[units[s].start..units[e - 1].end])
        }
    };

    // Trim leading ws_ifs.
    let mut i = 0;
    while i < units.len() && is_ws(&units[i]) {
        i += 1;
    }

    let mut result: Vec<String> = Vec::with_capacity(n_vars);

    // Emit fields 0..n_vars-2 (each terminated by IFS).
    for _ in 0..(n_vars - 1) {
        if i >= units.len() {
            result.push(String::new());
            continue;
        }
        // Field characters: until the next IFS or end-of-line.
        let start = i;
        while i < units.len() && !is_any_ifs(&units[i]) {
            i += 1;
        }
        result.push(span(&units, start, i));

        // Consume one terminator (IFS whitespace on either side of an
        // optional non-whitespace delimiter collapses into it).
        i = skip_terminator(&units, i);
    }

    // Remainder for var[n_vars-1]: trim trailing ws_ifs.
    let mut j = units.len();
    while j > i && is_ws(&units[j - 1]) {
        j -= 1;
    }
    let rest = &units[i..j];
    // If the remainder is exactly one field plus one terminator (`b:` in
    // `a:b:`), that delimiter only ends the last field — field splitting
    // yields no extra field for it — so the var gets the bare field, as
    // in dash/bash. With more fields left over (`b::`, `b:c:`), the
    // whole remainder, delimiters included, is assigned.
    let mut field_end = 0;
    while field_end < rest.len() && !is_any_ifs(&rest[field_end]) {
        field_end += 1;
    }
    let remainder = if skip_terminator(rest, field_end) == rest.len() {
        span(rest, 0, field_end)
    } else {
        span(rest, 0, rest.len())
    };
    result.push(remainder);

    debug_assert_eq!(result.len(), n_vars);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lb(value: u8, escaped: bool) -> LineByte {
        LineByte { value, escaped }
    }

    fn split_for(ifs: &str, line: Vec<LineByte>, n_vars: usize) -> Vec<String> {
        split_fields(ifs, &line, n_vars)
    }

    fn to_line(s: &str) -> Vec<LineByte> {
        s.bytes().map(|b| lb(b, false)).collect()
    }

    #[test]
    fn split_n_eq_1_trims_both_sides() {
        let out = split_for(" \t\n", to_line("   hello   "), 1);
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn split_n_eq_1_empty_input_yields_empty_string() {
        let out = split_for(" \t\n", to_line(""), 1);
        assert_eq!(out, vec!["".to_string()]);
    }

    #[test]
    fn split_n_gt_1_first_fields_then_remainder() {
        let out = split_for(" \t\n", to_line("a b c"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_remainder_keeps_internal_ifs() {
        let out = split_for(" \t\n", to_line("a b c d"), 2);
        assert_eq!(out, vec!["a".to_string(), "b c d".to_string()]);
    }

    #[test]
    fn split_leading_ifs_is_stripped() {
        let out = split_for(" \t\n", to_line("   a b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_trailing_ws_ifs_stripped_from_remainder() {
        let out = split_for(" \t\n", to_line("a b c   "), 2);
        assert_eq!(out, vec!["a".to_string(), "b c".to_string()]);
    }

    #[test]
    fn split_more_vars_than_fields_yields_empty_strings() {
        let out = split_for(" \t\n", to_line("a"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "".to_string()]);
    }

    #[test]
    fn split_empty_ifs_no_split() {
        let out = split_for("", to_line("a b c"), 2);
        // No splitting at all → entire line in var[0], var[1] empty.
        assert_eq!(out, vec!["a b c".to_string(), "".to_string()]);
    }

    #[test]
    fn split_sep_ifs_treated_as_single_separator() {
        // `IFS=:`, input "a::b" → x=a, y="" (one colon separator), z=b
        let out = split_for(":", to_line("a::b"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_mixed_sep_and_ws_ifs() {
        // IFS=":\t ", input "a: b" → ":" consumes one separator, then
        // " " is also IFS and is consumed greedily as adjacent ws.
        let out = split_for(": \t", to_line("a: b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_n_eq_1_sep_only_ifs_leading_sep_not_trimmed() {
        // IFS=":" has no whitespace-IFS bytes, so the N=1 path trims
        // nothing: the leading ':' stays (bash/dash: `IFS=: read x`
        // with input ":a" gives x=":a").
        let out = split_for(":", to_line(":a"), 1);
        assert_eq!(out, vec![":a".to_string()]);
    }

    #[test]
    fn split_n_eq_3_collapses_multi_space_runs() {
        // Runs of whitespace-IFS collapse into a single separator.
        let out = split_for(" \t\n", to_line("a   b   c"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_escaped_leading_sep_ifs_byte_stays_in_field_one() {
        // Non-raw input "\:a" with IFS=":": the ':' arrived escaped, so
        // it is not an IFS separator and stays in field 1.
        let line = vec![
            lb(b':', true), // escaped ':'
            lb(b'a', false),
        ];
        let out = split_fields(":", &line, 2);
        assert_eq!(out, vec![":a".to_string(), "".to_string()]);
    }

    #[test]
    fn split_escaped_byte_not_treated_as_ifs() {
        // Non-raw "a\ b": split would normally see " " as IFS, but the
        // space is escaped (came from `\<space>`), so it stays in field 1.
        let line = vec![
            lb(b'a', false),
            lb(b' ', true), // escaped space
            lb(b'b', false),
        ];
        let out = split_fields(" \t\n", &line, 2);
        assert_eq!(out, vec!["a b".to_string(), "".to_string()]);
    }

    #[test]
    fn split_ws_before_sep_collapses_into_terminator() {
        // `IFS=" :"`, "a :b": " :" is ONE delimiter (dash/bash [a][b][]).
        let out = split_for(" :", to_line("a :b"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "".to_string()]);
        let out = split_for(" :", to_line("a : b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
        // Two separators are still two delimiters.
        let out = split_for(" :", to_line("a: :b"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_last_var_drops_single_trailing_delimiter() {
        let t = |ifs: &str, line: &str, n: usize| split_for(ifs, to_line(line), n);
        assert_eq!(t(":", "a:b:", 2), vec!["a", "b"]);
        assert_eq!(t(": ", "a:b : ", 2), vec!["a", "b"]);
        assert_eq!(t(": ", "a::", 2), vec!["a", ""]);
        assert_eq!(t(": ", "a : :", 2), vec!["a", ""]);
        assert_eq!(t(": ", "a:", 1), vec!["a"]);
        assert_eq!(t(": ", " a :", 1), vec!["a"]);
        // More than one field left: delimiters are kept verbatim.
        assert_eq!(t(":", "a:b::", 2), vec!["a", "b::"]);
        assert_eq!(t(":", "a:b:c:", 2), vec!["a", "b:c:"]);
        assert_eq!(t(": ", "a b c:", 2), vec!["a", "b c:"]);
        assert_eq!(t(": ", "a:b:", 1), vec!["a:b:"]);
    }

    #[test]
    fn split_on_raw_byte_ifs() {
        // IFS holds the byteenc escape for byte 0xFF.
        let ifs = crate::byteenc::escape_char(0xFF).to_string();
        let line: Vec<LineByte> = b"a\xFFb".iter().map(|&b| lb(b, false)).collect();
        let out = split_for(&ifs, line, 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_multibyte_ifs_is_one_delimiter() {
        // `é` (C3 A9) splits only on the whole character, never on a
        // lone byte of it: `\xC3\xA8` (è) shares the lead byte.
        let out = split_for("é", to_line("aébèc"), 3);
        assert_eq!(
            out,
            vec!["a".to_string(), "bèc".to_string(), "".to_string()]
        );
        let out = split_for("é", to_line("aébéc"), 2);
        assert_eq!(out, vec!["a".to_string(), "béc".to_string()]);
    }

    #[test]
    fn split_escaped_multibyte_ifs_is_literal() {
        let mut line = to_line("aéb");
        line[1].escaped = true;
        let out = split_for("é", line, 2);
        assert_eq!(out, vec!["aéb".to_string(), "".to_string()]);
    }
}
