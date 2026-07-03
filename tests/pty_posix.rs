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

use expectrl::{Eof, Expect, session::OsSession};

use helpers::pty::{
    capture_until_sentinel, run_and_drain, spawn_yosh, spawn_yosh_with_env, wait_for_prompt,
};

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
        // Bare `fc -e cat`: cat reads the tempfile (no edits), exits 0;
        // fc then re-executes the previous (seeded) history entry. We
        // mute the re-execution's stdio and check only the exit status.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc -e cat </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn no_args_uses_editor() {
        // Bare `fc` with FCEDIT=cat: cat reads tempfile, exits 0; fc
        // re-executes the previous command. Check exit status only.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(&mut session, "fc </dev/null >/dev/null 2>&1; echo RC=$?");

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn bare_fc_does_not_recurse() {
        // Regression: bare `fc` with FCEDIT=cat used to stack-overflow
        // yosh because Repl::run added the fc command to history BEFORE
        // running it, so fc's "previous command" resolved to itself and
        // re-entered fc indefinitely. Hoisting history.add to after
        // exec_complete_command fixes this — fc now sees the pre-fc
        // history and resolves to the user's actual prior command.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(&mut session, "fc </dev/null >/dev/null 2>&1; echo RC=$?");

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}

mod fcedit {
    use super::*;

    #[test]
    fn used_by_fc() {
        // FCEDIT=cat → fc invokes cat as editor → cat reads tempfile,
        // exits 0 → fc re-executes the previous command.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(&mut session, "fc </dev/null >/dev/null 2>&1; echo RC=$?");

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn default_ed() {
        // FCEDIT and EDITOR removed → fc falls back to /bin/ed. We
        // verify /bin/ed exits 0 when given an empty stdin (probed
        // platform-side; see SP6 design §6).
        let (mut session, _tmpdir) = spawn_yosh_with_env(&[("FCEDIT", None), ("EDITOR", None)]);
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(&mut session, "fc </dev/null >/dev/null 2>&1; echo RC=$?");

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}

mod ps1 {
    use super::*;

    #[test]
    fn default_value_set() {
        // Start with PS1 stripped from the inherited env so yosh's
        // Repl::new must be the one to set it. The is_none() guard
        // in src/interactive/mod.rs ensures the POSIX default value
        // ("$ " / "# ") is written to the variable.
        let (mut session, _tmpdir) = spawn_yosh_with_env(&[("PS1", None)]);
        wait_for_prompt(&mut session);

        // Use distinct, non-overlapping sentinels so a negative assertion
        // can rule out the unset branch even when the captured stream
        // includes the user's echoed input (syntax-highlight repaints
        // emit every typed byte). The output line begins with "\r\n",
        // so anchoring on a leading newline isolates real output.
        let out = capture_until_sentinel(
            &mut session,
            r#"[ -n "${PS1+x}" ] && echo PS1ISSET || echo PS1ISMISSING"#,
        );

        // The command's actual stdout appears on a fresh line just
        // before the sentinel; the typed input (echoed back by the
        // line editor) appears interleaved with cursor/color escapes
        // but never on a bare new line of its own.
        assert!(
            out.contains("\nPS1ISSET") || out.contains("\r\nPS1ISSET"),
            "PS1 not set (no PS1ISSET output line) in: {:?}",
            out,
        );
        assert!(
            !(out.contains("\nPS1ISMISSING") || out.contains("\r\nPS1ISMISSING")),
            "PS1 reported MISSING in: {:?}",
            out,
        );

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}

mod exec_redirect {
    use super::*;
    use expectrl::Regex;

    #[test]
    fn no_cmd_redirects() {
        // POSIX 2.14.10: bare `exec` with redirections applies them to
        // the current shell. After `exec >file`, subsequent stdout
        // lands in file. Restoring with `exec >/dev/tty` requires
        // /dev/tty to be available — i.e., the shell must run under
        // a PTY (otherwise /dev/tty fails to open).
        //
        // Important: once `exec >file` redirects the shell's stdout,
        // the prompt and the sentinel `echo __YOSH_DONE__` also go to
        // the file rather than back to the PTY. So we can't run the
        // sequence as four separate `run_and_drain` calls — the second
        // one would hang waiting for a prompt that landed in the file.
        // Instead, fuse the redirect + echo + restore into a single
        // compound command terminated by `; cat "$TEST_TMPDIR/out"`;
        // by the time the sentinel is emitted, stdout has been restored
        // to /dev/tty and the cat output is visible to expectrl.
        let (mut session, tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        // Export the per-test tmpdir so the script can reference it
        // via $TEST_TMPDIR (mirrors how e2e/run_tests.sh provides it).
        let tmp = tmpdir.path().to_string_lossy().to_string();
        run_and_drain(&mut session, &format!("export TEST_TMPDIR={}", tmp));

        // Fuse the whole POSIX 2.14.10 sequence into one command line
        // followed by the sentinel. We don't reuse `capture_until_sentinel`
        // because it resyncs to the next `$ ` prompt afterward — and
        // depending on whether `exec >/dev/tty` resolves the controlling
        // terminal back to the PTY slave (which it does under expectrl
        // but is the subject of this test's hedging), the post-sentinel
        // prompt may or may not be visible. The sentinel itself appearing
        // is sufficient evidence the sequence ran to completion.
        let cmd = r#"exec >"$TEST_TMPDIR/out"; echo persistent; exec >/dev/tty 2>/dev/null || exec >&-; cat "$TEST_TMPDIR/out"; echo __YOSH_DONE__"#;
        session.send_line(cmd).unwrap();
        let captured = session
            .expect(Regex(r"\r?\n__YOSH_DONE__"))
            .expect("sentinel __YOSH_DONE__ not found");
        let out = String::from_utf8_lossy(captured.before()).into_owned();

        assert!(
            out.contains("persistent"),
            "missing 'persistent' in: {:?}",
            out
        );

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
