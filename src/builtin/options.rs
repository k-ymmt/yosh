//! Option parsing shared by builtins (XBD §12.2 Utility Syntax
//! Guidelines) and the matching "invalid option" diagnostics.

/// Drop a leading `--` end-of-options marker (Guideline 10).
pub(crate) fn strip_end_of_options(args: &[String]) -> &[String] {
    match args.first().map(String::as_str) {
        Some("--") => &args[1..],
        _ => args,
    }
}

/// A flag letter rejected by the [`scan_flag_words`] callback.
#[derive(Debug, PartialEq)]
pub(crate) struct UnknownFlag<'a> {
    /// The whole option word containing the flag (`-xv`).
    pub word: &'a str,
    pub flag: char,
}

/// Scan the leading option words of `args`. Each `-abc` word is a
/// cluster of single-letter flags handed to `accept` in order; `accept`
/// returns false for a letter the utility does not know. Scanning stops
/// at the first operand (a word not starting with `-`, or a lone `-`)
/// or just after `--`. Returns the index of the first operand.
pub(crate) fn scan_flag_words<'a>(
    args: &'a [String],
    mut accept: impl FnMut(char) -> bool,
) -> Result<usize, UnknownFlag<'a>> {
    let mut idx = 0;
    while let Some(word) = args.get(idx) {
        if word == "--" {
            return Ok(idx + 1);
        }
        let Some(cluster) = word.strip_prefix('-').filter(|c| !c.is_empty()) else {
            break;
        };
        for flag in cluster.chars() {
            if !accept(flag) {
                return Err(UnknownFlag { word, flag });
            }
        }
        idx += 1;
    }
    Ok(idx)
}

/// `utility: option: invalid option` diagnostic body (callers add the
/// `yosh: ` prefix), for utilities that report the whole option word.
pub(crate) fn invalid_option(utility: &str, option: &str) -> String {
    format!("{}: {}: invalid option", utility, option)
}

/// `utility: -x: invalid option` diagnostic body for one flag letter.
pub(crate) fn invalid_flag(utility: &str, flag: char) -> String {
    format!("{}: -{}: invalid option", utility, flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn strip_end_of_options_drops_only_a_leading_double_dash() {
        assert_eq!(strip_end_of_options(&s(&["--", "a"])), s(&["a"]));
        assert_eq!(strip_end_of_options(&s(&["a", "--"])), s(&["a", "--"]));
        assert_eq!(strip_end_of_options(&s(&["--", "--"])), s(&["--"]));
        assert!(strip_end_of_options(&[]).is_empty());
    }

    #[test]
    fn scan_flag_words_stops_at_operand_dash_and_after_double_dash() {
        let mut seen = String::new();
        let mut accept = |c| {
            seen.push(c);
            true
        };
        assert_eq!(
            scan_flag_words(&s(&["-ab", "-c", "x", "-d"]), &mut accept),
            Ok(2)
        );
        assert_eq!(scan_flag_words(&s(&["-a", "-", "-b"]), &mut accept), Ok(1));
        assert_eq!(scan_flag_words(&s(&["-a", "--", "-b"]), &mut accept), Ok(2));
        assert_eq!(scan_flag_words(&s(&[]), &mut accept), Ok(0));
        assert_eq!(seen, "abcaa");
    }

    #[test]
    fn scan_flag_words_reports_the_rejected_flag_and_word() {
        let args = s(&["-a", "-bx"]);
        assert_eq!(
            scan_flag_words(&args, |c| c != 'x'),
            Err(UnknownFlag {
                word: "-bx",
                flag: 'x'
            })
        );
    }

    #[test]
    fn invalid_option_messages() {
        assert_eq!(invalid_option("umask", "-x"), "umask: -x: invalid option");
        assert_eq!(invalid_flag("hash", 'x'), "hash: -x: invalid option");
    }
}
