//! Path completion for interactive tab-completion.
//!
//! This module provides the core logic for completing file and directory
//! paths when the user presses Tab in interactive mode.

use std::fs;
use std::io;
use std::path::PathBuf;

use super::selector::{ItemStyle, SelectorOptions, SelectorUI, colors_enabled};
use super::terminal::Terminal;

/// Scan leftward from `cursor` to find the start of the completion word.
///
/// Delimiters that break a word: whitespace (space, tab, newline — newlines
/// separate logical lines of a multiline buffer), `|`, `;`, `&`, `<`, `>`,
/// `(`, `)`. Inside quotes (single or double), whitespace does not act as
/// a delimiter, but the quote character itself is included in the returned
/// word.
///
/// Returns `(word_start_index, word_slice)`.
pub fn extract_completion_word(buf: &str, cursor: usize) -> (usize, &str) {
    let bytes = buf.as_bytes();
    let end = cursor.min(buf.len());

    // Scan left-to-right from the beginning up to `end`, tracking the last
    // unquoted delimiter. The completion word starts right after that delimiter.
    let mut word_start: usize = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let mut i = 0;
    while i < end {
        let ch = bytes[i];
        match ch {
            b'\'' if !in_double_quote => {
                if !in_single_quote {
                    // Opening quote — this is the start of a new word
                    // only if preceded by a delimiter (or at start).
                    // We treat the quote as part of the word, so update
                    // word_start to here.
                    if i == 0 || is_unquoted_delimiter(bytes[i - 1]) {
                        word_start = i;
                    }
                }
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote => {
                if !in_double_quote && (i == 0 || is_unquoted_delimiter(bytes[i - 1])) {
                    word_start = i;
                }
                in_double_quote = !in_double_quote;
            }
            ch if is_unquoted_delimiter(ch) && !in_single_quote && !in_double_quote => {
                word_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    (word_start, &buf[word_start..end])
}

/// Unquoted bytes that delimit a completion word: whitespace plus the
/// shell operator characters. The single source of truth for the
/// completion-side word/segment splitting (see also
/// [`is_segment_delimiter`] and [`segment_start`]).
pub fn is_unquoted_delimiter(ch: u8) -> bool {
    matches!(
        ch,
        b' ' | b'\t' | b'\n' | b'|' | b';' | b'&' | b'<' | b'>' | b'(' | b')'
    )
}

/// Unquoted operator bytes that end a pipeline segment for command-word
/// resolution — the non-whitespace subset of [`is_unquoted_delimiter`]
/// (whitespace separates words *within* a segment).
pub fn is_segment_delimiter(ch: u8) -> bool {
    matches!(ch, b'|' | b';' | b'&' | b'<' | b'>' | b'(' | b')')
}

/// Byte index just after the last unquoted segment delimiter in
/// `buf[..end]` (0 when there is none): the start of the pipeline
/// segment containing `end`. Tracks single/double quotes so delimiters
/// inside quotes do not split.
///
/// A balanced `$(...)` / `$((...))` or `` `...` `` substitution closed
/// before `end` is skipped whole: its closing `)` (or the operators
/// inside it) belongs to the substitution, not to the pipeline — so
/// `git -C $(pwd) ch<Tab>` still resolves `git` as the command word.
/// When the cursor is INSIDE an unterminated substitution, the segment
/// starts just after its opener (completing `echo $(git ch<Tab>`
/// resolves `git`), matching the previous `(`-as-delimiter behavior.
pub fn segment_start(buf: &str, end: usize) -> usize {
    let bytes = &buf.as_bytes()[..end.min(buf.len())];
    let mut seg_start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b'\'' if !in_double => {
                in_single = !in_single;
                i += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                i += 1;
            }
            b'$' if !in_single && bytes.get(i + 1) == Some(&b'(') => {
                // skip_balanced_parens is quote/escape-aware and treats
                // the extra parens of `$((...))` as ordinary nesting.
                let j = crate::expand::scan::skip_balanced_parens(bytes, i + 2);
                if j < bytes.len() {
                    i = j + 1; // past the closing `)`
                } else {
                    // Unterminated before the cursor: cursor is inside
                    // the substitution — it opens a fresh segment.
                    seg_start = i + 2;
                    i += 2;
                }
            }
            b'`' if !in_single => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'`' {
                    if bytes[j] == b'\\' && j + 1 < bytes.len() {
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                if j < bytes.len() {
                    i = j + 1; // past the closing backtick
                } else {
                    seg_start = i + 1;
                    i += 1;
                }
            }
            ch if is_segment_delimiter(ch) && !in_single && !in_double => {
                seg_start = i + 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    seg_start
}

/// Returns `true` if `word_start` is at command position in `buf`.
///
/// Command position means the word is the first token after:
/// - line start (nothing before it)
/// - a newline (first word of a continuation line in a multiline buffer)
/// - `|`, `;`, `&`, `(`, `!`
///
/// Scans backward from `word_start`, skipping whitespace, and checks
/// the last non-whitespace character.
///
/// The character set here is deliberately *not* [`is_segment_delimiter`]:
/// `!` precedes a command without ending a segment, while after `)`,
/// `<`, or `>` the next word is a filename/operand, not a command.
pub fn is_command_position(buf: &str, word_start: usize) -> bool {
    let before_raw = &buf[..word_start];
    let before = before_raw.trim_end();
    if before.is_empty() {
        return true;
    }
    // A newline in the trailing whitespace separates commands like `;`.
    if before_raw[before.len()..].contains('\n') {
        return true;
    }
    matches!(
        before.as_bytes().last(),
        Some(b'|' | b';' | b'&' | b'(' | b'!')
    )
}

/// Split a completion word at the last `/` into (directory_part, prefix).
///
/// - If the word starts with `~`, the tilde is expanded to `home`.
/// - A leading quote character (`'` or `"`) is stripped before processing.
/// - The directory part retains its trailing `/`.
///
/// Returns `(directory_string, prefix_slice)`.
pub fn split_path<'a>(word: &'a str, home: &str) -> (String, &'a str) {
    // Strip leading quote character
    let stripped = if word.starts_with('\'') || word.starts_with('"') {
        &word[1..]
    } else {
        word
    };

    match stripped.rfind('/') {
        Some(pos) => {
            let dir_part = &stripped[..=pos]; // includes the '/'
            let prefix = &stripped[pos + 1..];
            // Expand tilde
            let dir_expanded = if let Some(rest) = dir_part.strip_prefix('~') {
                format!("{}{}", home, rest)
            } else {
                dir_part.to_string()
            };

            // Map slice back to the original word's lifetime
            // prefix is a slice of `stripped`, which is a sub-slice of `word`
            (dir_expanded, prefix)
        }
        None => {
            // No slash: expand lone tilde prefix
            if stripped == "~" {
                (format!("{}/", home), "")
            } else {
                (String::new(), stripped)
            }
        }
    }
}

/// Compute the longest common prefix of all candidate strings.
///
/// Returns an empty string if the list is empty or there is no common
/// prefix. Compares whole characters, never bytes: two candidates can
/// share a UTF-8 leading byte without sharing a character (e.g.
/// `日本.txt` / `本日.txt` both start with 0xE6), and a byte-indexed
/// prefix would slice mid-character and panic.
pub fn longest_common_prefix(candidates: &[String]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for item in &candidates[1..] {
        let mut common = 0;
        for (a, b) in prefix.iter().zip(item.chars()) {
            if *a != b {
                break;
            }
            common += 1;
        }
        prefix.truncate(common);
    }
    prefix.into_iter().collect()
}

/// Resolve the directory to scan for a completion word's `dir_part`
/// (from [`split_path`]): empty means the CWD itself, an absolute path
/// is kept as-is, and a relative path is joined onto `cwd`.
pub fn resolve_dir(dir_part: &str, cwd: &str) -> String {
    if dir_part.is_empty() {
        cwd.to_string()
    } else if dir_part.starts_with('/') {
        dir_part.to_string()
    } else {
        let mut path = PathBuf::from(cwd);
        path.push(dir_part);
        path.to_string_lossy().into_owned()
    }
}

/// The directory prefix of `word` exactly as the user typed it (up to
/// and including the last `/`, no tilde expansion) — mirroring
/// [`split_path`] so quoted words (`'sub/xy`, or the value part of
/// `--file='sub/xy`) reconstruct their replacement text consistently.
///
/// A single leading quote character is KEPT in the returned prefix:
/// the completion word starts at the quote, so the replacement text
/// must re-insert it or a space-containing match would be inserted
/// unquoted (`cd "/tmp/My D<Tab>` must complete to `"/tmp/My Dir/`,
/// not `/tmp/My Dir/`).
pub fn dir_prefix_of(word: &str) -> String {
    let (quote, stripped) = match word.as_bytes().first() {
        Some(&q @ (b'\'' | b'"')) => (Some(q as char), &word[1..]),
        _ => (None, word),
    };
    let dir = match stripped.rfind('/') {
        Some(pos) => &stripped[..=pos],
        None => "",
    };
    match quote {
        Some(q) => format!("{q}{dir}"),
        None => dir.to_string(),
    }
}

/// If `prefix` (the verbatim re-inserted part of a completion
/// replacement, e.g. `"/tmp/My ` or `--file='sub/`) leaves a quote
/// open, return that quote character so the caller can close it after
/// a completed filename — bash-like: `cat "My D<Tab>` completes to
/// `cat "My Doc.txt" ` (directories stay open for further completion).
pub fn unclosed_quote(prefix: &str) -> Option<char> {
    let mut in_single = false;
    let mut in_double = false;
    for ch in prefix.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ => {}
        }
    }
    if in_single {
        Some('\'')
    } else if in_double {
        Some('"')
    } else {
        None
    }
}

/// Scan a directory and return sorted completion candidates matching `prefix`.
///
/// - Hidden files (starting with `.`) are excluded unless `prefix` starts
///   with `.` or `show_dotfiles` is true.
/// - Directories have a trailing `/` appended.
/// - Returns an empty `Vec` if `dir` does not exist or cannot be read.
pub fn generate_candidates(dir: &str, prefix: &str, show_dotfiles: bool) -> Vec<String> {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let include_hidden = show_dotfiles || prefix.starts_with('.');

    let mut results: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // Filter hidden files
            if name.starts_with('.') && !include_hidden {
                return None;
            }
            // Filter by prefix
            if !name.starts_with(prefix) {
                return None;
            }
            // Append trailing slash for directories
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            if is_dir {
                Some(format!("{}/", name))
            } else {
                Some(name)
            }
        })
        .collect();

    results.sort();
    results
}

/// Settings for path completion.
pub struct CompletionContext {
    /// Current working directory.
    pub cwd: String,
    /// User's home directory (for tilde expansion).
    pub home: String,
    /// Whether to show dotfiles even when prefix does not start with `.`.
    pub show_dotfiles: bool,
}

/// Result of a tab-completion attempt.
pub struct CompletionResult {
    /// All matching candidate names (file/dir names, not full paths).
    pub candidates: Vec<String>,
    /// Longest common prefix among all candidates.
    pub common_prefix: String,
    /// Byte offset in the input buffer where the completion word starts.
    #[allow(dead_code)]
    // public completion-result field; held for callers that need the offset
    pub word_start: usize,
    /// The directory prefix string (as the user typed it, before expansion),
    /// used to reconstruct the replacement text.
    pub dir_prefix: String,
}

/// Perform path completion on the current input buffer at the given cursor
/// position.
///
/// Combines `extract_completion_word`, `split_path`, directory resolution,
/// `generate_candidates`, and `longest_common_prefix` into a single call.
pub fn complete(buf: &str, cursor: usize, ctx: &CompletionContext) -> CompletionResult {
    let (word_start, word) = extract_completion_word(buf, cursor);
    let (dir_part, prefix) = split_path(word, &ctx.home);

    let resolved_dir = resolve_dir(&dir_part, &ctx.cwd);
    let candidates = generate_candidates(&resolved_dir, prefix, ctx.show_dotfiles);
    let common_prefix = longest_common_prefix(&candidates);

    CompletionResult {
        candidates,
        common_prefix,
        word_start,
        // The dir_prefix as the user typed it (before tilde expansion),
        // so the caller can reconstruct the replacement text.
        dir_prefix: dir_prefix_of(word),
    }
}

// ---------------------------------------------------------------------------
// Completion UI (interactive candidate selection)
// ---------------------------------------------------------------------------

/// Interactive fuzzy-filter UI for selecting a completion candidate.
/// Thin wrapper over the shared [`SelectorUI`].
pub struct CompletionUI;

impl CompletionUI {
    /// Returns `Some(selected)` or `None` on cancel.
    pub fn run<T: Terminal>(candidates: &[String], term: &mut T) -> io::Result<Option<String>> {
        SelectorUI::run(
            candidates,
            SelectorOptions {
                item_style: ItemStyle::Path,
                colors: colors_enabled(),
            },
            term,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    // ── extract_completion_word ──────────────────────────────────────

    #[test]
    fn test_extract_simple_word() {
        let (start, word) = extract_completion_word("ls foo", 6);
        assert_eq!(start, 3);
        assert_eq!(word, "foo");
    }

    #[test]
    fn test_extract_at_start() {
        let (start, word) = extract_completion_word("foo", 3);
        assert_eq!(start, 0);
        assert_eq!(word, "foo");
    }

    #[test]
    fn test_extract_after_pipe() {
        let (start, word) = extract_completion_word("cat foo | grep b", 16);
        assert_eq!(start, 15);
        assert_eq!(word, "b");
    }

    #[test]
    fn test_extract_after_semicolon() {
        let (start, word) = extract_completion_word("echo a; ls sr", 13);
        assert_eq!(start, 11);
        assert_eq!(word, "sr");
    }

    #[test]
    fn test_extract_empty_at_space() {
        let (start, word) = extract_completion_word("ls ", 3);
        assert_eq!(start, 3);
        assert_eq!(word, "");
    }

    #[test]
    fn test_extract_path_with_slash() {
        let (start, word) = extract_completion_word("ls src/int", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "src/int");
    }

    #[test]
    fn test_extract_with_double_quote() {
        let (start, word) = extract_completion_word("ls \"My Doc", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "\"My Doc");
    }

    #[test]
    fn test_extract_with_single_quote() {
        let (start, word) = extract_completion_word("ls 'My Doc", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "'My Doc");
    }

    #[test]
    fn test_extract_newline_delimits_word() {
        // Continuation line of a multiline buffer: the word must not span
        // the newline.
        let buf = "if true\nth";
        let (start, word) = extract_completion_word(buf, buf.len());
        assert_eq!(start, 8);
        assert_eq!(word, "th");
    }

    #[test]
    fn test_extract_tab_delimits_word() {
        let buf = "ls\tsr";
        let (start, word) = extract_completion_word(buf, buf.len());
        assert_eq!(start, 3);
        assert_eq!(word, "sr");
    }

    #[test]
    fn test_extract_newline_inside_quotes_not_delimiter() {
        let buf = "echo 'a\nb";
        let (start, word) = extract_completion_word(buf, buf.len());
        assert_eq!(start, 5);
        assert_eq!(word, "'a\nb");
    }

    // ── segment_start ───────────────────────────────────────────────

    #[test]
    fn test_segment_start_skips_closed_command_sub() {
        // The `)` of a closed $() is not a segment boundary.
        assert_eq!(segment_start("git -C $(pwd) ch", 16), 0);
        // Arithmetic form too.
        assert_eq!(segment_start("echo $((1+2)) x", 15), 0);
    }

    #[test]
    fn test_segment_start_skips_closed_backticks() {
        assert_eq!(segment_start("git -C `pwd` ch", 15), 0);
    }

    #[test]
    fn test_segment_start_open_command_sub_starts_segment() {
        // Cursor inside `$(`: the substitution opens its own segment.
        assert_eq!(segment_start("echo $(git ch", 13), 7);
        assert_eq!(segment_start("echo `git ch", 12), 6);
    }

    #[test]
    fn test_segment_start_plain_delimiters_still_split() {
        assert_eq!(segment_start("cat f | grep x", 14), 7);
        assert_eq!(segment_start("(cd /tmp) git ch", 16), 9);
        assert_eq!(segment_start("echo 'a | b' x", 14), 0);
    }

    #[test]
    fn test_segment_start_command_sub_inside_double_quotes() {
        // `"$(pwd)"` — skipped whole; the trailing `|` still splits.
        assert_eq!(segment_start("git -C \"$(pwd)\" st | wc", 23), 20);
    }

    // ── dir_prefix_of / unclosed_quote ──────────────────────────────

    #[test]
    fn test_dir_prefix_keeps_leading_double_quote() {
        // `cd "/tmp/My D<Tab>` must re-insert the opening quote, or a
        // space-containing match replaces the word unquoted.
        assert_eq!(dir_prefix_of("\"/tmp/My D"), "\"/tmp/");
        assert_eq!(dir_prefix_of("'/tmp/My D"), "'/tmp/");
    }

    #[test]
    fn test_dir_prefix_quote_without_slash() {
        assert_eq!(dir_prefix_of("\"My D"), "\"");
        assert_eq!(dir_prefix_of("'My D"), "'");
    }

    #[test]
    fn test_dir_prefix_unquoted_unchanged() {
        assert_eq!(dir_prefix_of("src/int"), "src/");
        assert_eq!(dir_prefix_of("foo"), "");
        assert_eq!(dir_prefix_of("~/Doc/x"), "~/Doc/");
    }

    #[test]
    fn test_unclosed_quote() {
        assert_eq!(unclosed_quote("\"/tmp/My "), Some('"'));
        assert_eq!(unclosed_quote("'sub/"), Some('\''));
        assert_eq!(unclosed_quote("--file='sub/"), Some('\''));
        assert_eq!(unclosed_quote("src/"), None);
        assert_eq!(unclosed_quote("\"done\" "), None);
        // A double quote inside single quotes does not open a string.
        assert_eq!(unclosed_quote("'a\"b' "), None);
    }

    // ── split_path ──────────────────────────────────────────────────

    #[test]
    fn test_split_relative_path() {
        let (dir, prefix) = split_path("src/int", "/home/user");
        assert_eq!(dir, "src/");
        assert_eq!(prefix, "int");
    }

    #[test]
    fn test_split_no_directory() {
        let (dir, prefix) = split_path("foo", "/home/user");
        assert_eq!(dir, "");
        assert_eq!(prefix, "foo");
    }

    #[test]
    fn test_split_absolute_path() {
        let (dir, prefix) = split_path("/usr/lo", "/home/user");
        assert_eq!(dir, "/usr/");
        assert_eq!(prefix, "lo");
    }

    #[test]
    fn test_split_tilde_path() {
        let (dir, prefix) = split_path("~/Doc", "/home/user");
        assert_eq!(dir, "/home/user/");
        assert_eq!(prefix, "Doc");
    }

    #[test]
    fn test_split_trailing_slash() {
        let (dir, prefix) = split_path("src/", "/home/user");
        assert_eq!(dir, "src/");
        assert_eq!(prefix, "");
    }

    // ── longest_common_prefix ───────────────────────────────────────

    #[test]
    fn test_lcp_multiple_candidates() {
        let candidates = vec![
            "src/".to_string(),
            "src_util".to_string(),
            "src_main".to_string(),
        ];
        assert_eq!(longest_common_prefix(&candidates), "src");
    }

    #[test]
    fn test_lcp_single_candidate() {
        let candidates = vec!["foobar".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "foobar");
    }

    #[test]
    fn test_lcp_empty_list() {
        let candidates: Vec<String> = vec![];
        assert_eq!(longest_common_prefix(&candidates), "");
    }

    #[test]
    fn test_lcp_no_common() {
        let candidates = vec!["abc".to_string(), "xyz".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "");
    }

    #[test]
    fn test_lcp_all_same() {
        let candidates = vec![
            "hello".to_string(),
            "hello".to_string(),
            "hello".to_string(),
        ];
        assert_eq!(longest_common_prefix(&candidates), "hello");
    }

    #[test]
    fn test_lcp_multibyte_shared_leading_byte_only() {
        // 日 and 本 share the UTF-8 leading byte 0xE6 but are different
        // characters: the common prefix must be empty, not a panic on a
        // non-char-boundary slice.
        let candidates = vec!["日本.txt".to_string(), "本日.txt".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "");
    }

    #[test]
    fn test_lcp_multibyte_common_prefix() {
        let candidates = vec!["日本語A.txt".to_string(), "日本語B.txt".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "日本語");
    }

    #[test]
    fn test_lcp_mixed_ascii_and_multibyte() {
        let candidates = vec!["fooあ".to_string(), "fooい".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "foo");
    }

    // ── generate_candidates ─────────────────────────────────────────

    fn setup_temp_dir() -> TempDir {
        let tmp = TempDir::new().unwrap();
        // Create files and directories
        File::create(tmp.path().join("alpha.txt")).unwrap();
        File::create(tmp.path().join("beta.rs")).unwrap();
        File::create(tmp.path().join("alpha_two.txt")).unwrap();
        File::create(tmp.path().join(".hidden")).unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::create_dir(tmp.path().join("alpha_dir")).unwrap();
        tmp
    }

    #[test]
    fn test_generate_basic_listing() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let mut candidates = generate_candidates(dir, "", false);
        candidates.sort();
        // Should not include hidden files, should include directories with /
        assert!(candidates.contains(&"alpha.txt".to_string()));
        assert!(candidates.contains(&"beta.rs".to_string()));
        assert!(candidates.contains(&"alpha_two.txt".to_string()));
        assert!(candidates.contains(&"subdir/".to_string()));
        assert!(candidates.contains(&"alpha_dir/".to_string()));
        assert!(!candidates.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_generate_prefix_filter() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let candidates = generate_candidates(dir, "alpha", false);
        assert!(candidates.contains(&"alpha.txt".to_string()));
        assert!(candidates.contains(&"alpha_two.txt".to_string()));
        assert!(candidates.contains(&"alpha_dir/".to_string()));
        assert!(!candidates.contains(&"beta.rs".to_string()));
        assert!(!candidates.contains(&"subdir/".to_string()));
    }

    #[test]
    fn test_generate_hidden_files_default() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let candidates = generate_candidates(dir, "", false);
        assert!(!candidates.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_generate_dotfiles_with_dot_prefix() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let candidates = generate_candidates(dir, ".", false);
        assert!(candidates.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_generate_dotfiles_with_env() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let candidates = generate_candidates(dir, "", true);
        assert!(candidates.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_generate_nonexistent_dir() {
        let candidates = generate_candidates("/nonexistent_dir_12345", "", false);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_generate_directory_gets_slash() {
        let tmp = setup_temp_dir();
        let dir = tmp.path().to_str().unwrap();
        let candidates = generate_candidates(dir, "sub", false);
        assert_eq!(candidates, vec!["subdir/"]);
    }

    // ── complete ────────────────────────────────────────────────────

    #[test]
    fn test_complete_single_candidate() {
        let tmp = setup_temp_dir();
        let cwd = tmp.path().to_str().unwrap().to_string();
        let ctx = CompletionContext {
            cwd: cwd.clone(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("ls bet", 6, &ctx);
        assert_eq!(result.candidates, vec!["beta.rs"]);
        assert_eq!(result.common_prefix, "beta.rs");
        assert_eq!(result.word_start, 3);
        assert_eq!(result.dir_prefix, "");
    }

    #[test]
    fn test_complete_multiple_candidates() {
        let tmp = setup_temp_dir();
        let cwd = tmp.path().to_str().unwrap().to_string();
        let ctx = CompletionContext {
            cwd: cwd.clone(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("ls alpha", 8, &ctx);
        assert_eq!(result.candidates.len(), 3);
        assert!(result.candidates.contains(&"alpha.txt".to_string()));
        assert!(result.candidates.contains(&"alpha_two.txt".to_string()));
        assert!(result.candidates.contains(&"alpha_dir/".to_string()));
        assert_eq!(result.common_prefix, "alpha");
        assert_eq!(result.word_start, 3);
    }

    #[test]
    fn test_complete_with_directory_prefix() {
        let tmp = setup_temp_dir();
        let cwd = tmp.path().to_str().unwrap().to_string();
        // Create a nested file
        fs::create_dir_all(tmp.path().join("subdir")).ok();
        File::create(tmp.path().join("subdir").join("nested.txt")).unwrap();

        let ctx = CompletionContext {
            cwd: cwd.clone(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("cat subdir/nes", 14, &ctx);
        assert_eq!(result.candidates, vec!["nested.txt"]);
        assert_eq!(result.common_prefix, "nested.txt");
        assert_eq!(result.word_start, 4);
        assert_eq!(result.dir_prefix, "subdir/");
    }

    #[test]
    fn test_complete_quoted_word_keeps_quote_in_dir_prefix() {
        // `cat "sub dir/nes<Tab>` — the replacement text is
        // dir_prefix + candidate; the opening quote must survive so the
        // space-containing path stays a single argument.
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("sub dir")).unwrap();
        File::create(tmp.path().join("sub dir").join("nested.txt")).unwrap();
        let ctx = CompletionContext {
            cwd: tmp.path().to_str().unwrap().to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let buf = "cat \"sub dir/nes";
        let result = complete(buf, buf.len(), &ctx);
        assert_eq!(result.candidates, vec!["nested.txt"]);
        assert_eq!(result.word_start, 4); // at the opening quote
        assert_eq!(result.dir_prefix, "\"sub dir/");
        // Reconstructed replacement (what handle_tab_complete inserts).
        let replacement = format!(
            "{}{}{} ",
            result.dir_prefix,
            &result.candidates[0],
            unclosed_quote(&result.dir_prefix).unwrap()
        );
        assert_eq!(replacement, "\"sub dir/nested.txt\" ");
    }

    #[test]
    fn test_complete_no_matches() {
        let tmp = setup_temp_dir();
        let cwd = tmp.path().to_str().unwrap().to_string();
        let ctx = CompletionContext {
            cwd: cwd.clone(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("ls zzz", 6, &ctx);
        assert!(result.candidates.is_empty());
        assert_eq!(result.common_prefix, "");
        assert_eq!(result.word_start, 3);
    }

    // ── is_command_position ────────────────────────────────────────

    #[test]
    fn test_command_position_line_start() {
        assert!(is_command_position("", 0));
        assert!(is_command_position("gi", 0));
    }

    #[test]
    fn test_command_position_after_pipe() {
        // "ls | gr" — word_start=5
        assert!(is_command_position("ls | gr", 5));
    }

    #[test]
    fn test_command_position_after_semicolon() {
        // "echo a; ls" — word_start=8
        assert!(is_command_position("echo a; ls", 8));
    }

    #[test]
    fn test_command_position_after_and_and() {
        // "true && ec" — word_start=8
        assert!(is_command_position("true && ec", 8));
    }

    #[test]
    fn test_command_position_after_or_or() {
        // "false || ec" — word_start=9
        assert!(is_command_position("false || ec", 9));
    }

    #[test]
    fn test_command_position_after_open_paren() {
        // "(ls" — word_start=1
        assert!(is_command_position("(ls", 1));
    }

    #[test]
    fn test_command_position_after_bang() {
        // "! cmd" — word_start=2
        assert!(is_command_position("! cmd", 2));
    }

    #[test]
    fn test_not_command_position_argument() {
        // "ls fo" — word_start=3
        assert!(!is_command_position("ls fo", 3));
    }

    #[test]
    fn test_not_command_position_second_arg() {
        // "echo hello wor" — word_start=11
        assert!(!is_command_position("echo hello wor", 11));
    }

    #[test]
    fn test_command_position_after_newline() {
        // "if true\nth" — word_start=8: first word of a continuation line
        assert!(is_command_position("if true\nth", 8));
    }

    #[test]
    fn test_command_position_after_newline_with_indent() {
        // "while true\n  ec" — word_start=13
        assert!(is_command_position("while true\n  ec", 13));
    }
}
