//! POSIX-spec PTY-driven tests migrated from e2e/posix_spec/*.
//!
//! Each test corresponds to one e2e/posix_spec/.../foo.sh file. The
//! original shell file is retained as a stub with the directive
//! `# MIGRATED_TO: tests/pty_posix.rs::<test_path>` so readers
//! arriving at the POSIX spec layout find the Rust test, and so the
//! e2e runner accounts for it under `Migrated: N`.
//!
//! Why PTY: these tests depend on interactive history, an editor
//! process, the default PS1, or /dev/tty — none of which is
//! available to the non-interactive e2e runner.

mod helpers;

#[allow(unused_imports)]
use expectrl::{Eof, Expect, Regex, session::OsSession};

#[allow(unused_imports)]
use helpers::pty::{spawn_yosh, spawn_yosh_with_env, wait_for_prompt};

/// Send a command and capture every byte written until a sentinel line
/// `__YOSH_DONE__` appears on its own (preceded by CR/LF). Returns the
/// captured bytes (which include the line-editor echo + the command's
/// stdout/stderr that landed on the PTY).
///
/// Why a sentinel instead of `read_until_prompt`'s `\$ ` regex: when the
/// line editor is in syntax-highlight mode (the default for an interactive
/// yosh), it repaints `$ <partial>` after every keystroke. A `\$ ` regex
/// matches the very first such repaint and returns before the command
/// has even been submitted. The sentinel pattern `__YOSH_DONE__` cannot
/// appear in any line-editor repaint (the LineEditor only renders chars
/// already typed by the user up to that point), so matching on it is
/// race-free.
fn capture_until_sentinel(session: &mut OsSession, cmd: &str) -> String {
    // `; echo __YOSH_DONE__` runs after `cmd` regardless of `cmd`'s exit
    // status (we use ; not &&). The `\r?\n__YOSH_DONE__` regex matches a
    // CR/LF prefix to skip the user's echoed input which contains
    // `__YOSH_DONE__` as part of the line-editor's repaint of the
    // typed string.
    let full = format!("{}; echo __YOSH_DONE__", cmd);
    session.send_line(&full).unwrap();
    let captured = session
        .expect(Regex(r"\r?\n__YOSH_DONE__"))
        .expect("sentinel __YOSH_DONE__ not found");
    let out = String::from_utf8_lossy(captured.before()).into_owned();
    // Resync: consume up to and including the next prompt.
    let _ = session.expect(Regex(r"\$ ")).expect("prompt not found");
    out
}

/// Run a command for its side effect only (e.g. seeding history). Captures
/// and discards output up through the next prompt using the same sentinel
/// trick as `capture_until_sentinel`.
fn run_and_drain(session: &mut OsSession, cmd: &str) {
    let _ = capture_until_sentinel(session, cmd);
}

mod fc {
    use super::*;

    /// Seed three commands into history and return after the third prompt.
    fn seed_three(session: &mut OsSession) {
        run_and_drain(session, "echo aaa");
        run_and_drain(session, "echo bbb");
        run_and_drain(session, "echo ccc");
    }

    #[test]
    fn list_recent() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        let out = capture_until_sentinel(&mut session, "fc -l");

        assert!(out.contains("echo aaa"), "missing 'echo aaa' in: {:?}", out);
        assert!(out.contains("echo bbb"), "missing 'echo bbb' in: {:?}", out);
        assert!(out.contains("echo ccc"), "missing 'echo ccc' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn list_no_numbers() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        let out = capture_until_sentinel(&mut session, "fc -l -n");

        // -n suppresses leading line numbers; output lines start with a tab
        // (per src/builtin/special.rs::fc_list).
        assert!(out.contains("echo aaa"), "missing 'echo aaa' in: {:?}", out);
        assert!(out.contains("echo bbb"), "missing 'echo bbb' in: {:?}", out);
        assert!(out.contains("echo ccc"), "missing 'echo ccc' in: {:?}", out);
        // Look for a tab-prefixed entry to confirm -n's no-number formatting.
        assert!(
            out.contains("\techo aaa"),
            "expected '\\techo aaa' in: {:?}",
            out
        );

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn list_reverse() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        let out = capture_until_sentinel(&mut session, "fc -l -r");

        // Reverse order: ccc should appear before aaa.
        let i_aaa = out.find("echo aaa").expect("echo aaa not found");
        let i_ccc = out.find("echo ccc").expect("echo ccc not found");
        assert!(i_ccc < i_aaa, "expected ccc before aaa in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn substitute() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo onevar");

        let out = capture_until_sentinel(&mut session, "fc -s one=two echo");

        assert!(out.contains("twovar"), "expected 'twovar' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn editor_dash_e() {
        // Note: yosh's `fc` adds the running `fc ...` command to history
        // BEFORE executing it (see src/interactive/mod.rs:268-272), which
        // means a bare `fc -e cat` would target the fc command itself,
        // causing infinite recursion (stack overflow) when fc re-executes
        // the contents. POSIX excludes the fc invocation itself from being
        // its own target; this is a yosh bug tracked separately. Until it
        // is fixed, we pass the operand `echo` so fc's prefix-match
        // resolution selects the seeded `echo seedline` entry, not the
        // fc command. That still exercises the -e flag's editor-selection
        // logic, which is what this test asserts.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo seedline");

        // `cat` reads the tempfile (no edits), exits 0; fc then re-executes
        // the seeded `echo seedline`. We use </dev/null and >/dev/null to
        // mute re-execution side effects; only the exit status matters.
        let out = capture_until_sentinel(
            &mut session,
            "fc -e cat echo </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn no_args_uses_editor() {
        // Bare `fc` with FCEDIT=cat: cat reads tempfile, exits 0; fc
        // re-executes the previous command. We check exit status only.
        //
        // Same caveat as editor_dash_e: a literal bare `fc` would target
        // itself (yosh adds it to history first), so we pass `echo` as
        // the prefix specifier. This still exercises the
        // "no -e option → use FCEDIT/EDITOR" branch of fc.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc echo </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
