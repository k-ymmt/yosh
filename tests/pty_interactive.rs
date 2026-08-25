mod helpers;

use std::time::Duration;

use expectrl::{Eof, Expect, Regex, session::OsSession};

use helpers::pty::{
    TIMEOUT, read_until_prompt, spawn_yosh, spawn_yosh_with_args, strip_ansi, wait_for_prompt,
    wait_for_ps2, wait_for_raw_mode,
};

/// Wait for command output (a line following a newline, not the input echo).
/// Uses a regex that matches the pattern preceded by a newline. The \r before
/// \n is optional because crossterm raw mode suppresses PTY ONLCR output
/// processing when active — so output may arrive as either \r\n or just \n.
fn expect_output(session: &mut OsSession, text: &str, msg: &str) {
    // \r? makes the carriage return optional to handle both ONLCR and raw mode
    let pattern = format!("\r?\n{}", text);
    session
        .expect(Regex(&pattern))
        .unwrap_or_else(|e| panic!("{}: {}", msg, e));
}

/// Send Ctrl+D and wait for the shell to exit cleanly.
fn exit_shell(session: &mut OsSession) {
    session.send("\x04").unwrap();
    // Wait for EOF to ensure the yosh process has fully exited before the
    // next test starts — avoids PTY resource contention between tests.
    let _ = session.expect(Eof);
}

/// RAII guard that restores the expectrl session's timeout on drop.
///
/// Use when temporarily shrinking the timeout for fast-failing expects
/// (e.g. buffer drains). Restores the original timeout even if a panic
/// aborts the test, preventing a leaked short timeout from cascading
/// into later assertions.
struct TimeoutGuard<'a> {
    session: &'a mut OsSession,
    saved: Duration,
}

impl<'a> TimeoutGuard<'a> {
    /// Install `temporary` as the session's expect timeout, restoring
    /// `prior` on drop. expectrl 0.8 has no getter for the current
    /// timeout, so the caller must pass it explicitly — this prevents
    /// a future test from silently restoring the wrong value if it has
    /// already changed the timeout away from `TIMEOUT`.
    fn new(session: &'a mut OsSession, temporary: Duration, prior: Duration) -> Self {
        session.set_expect_timeout(Some(temporary));
        Self {
            session,
            saved: prior,
        }
    }
}

impl<'a> Drop for TimeoutGuard<'a> {
    fn drop(&mut self) {
        self.session.set_expect_timeout(Some(self.saved));
    }
}

/// Consume whatever is currently in expectrl's internal buffer so the
/// next `expect` sees only fresh bytes.
///
/// The line editor repaints each typed character with syntax-highlight
/// ANSI escape sequences; by the time a user-visible command has been
/// echoed, expectrl's buffer holds ~2KB of stale `$ ` + color codes.
/// Without draining, subsequent `expect` calls can match those stale
/// prompts and race past the real post-command output.
///
/// The regex lower-bound `0,` is intentional: we want "up to 8KB or
/// whatever is there," not "at least one character." Changing it to
/// `1,` reintroduces a hang when the buffer is already empty.
///
/// `PTY_DRAIN_MAX_BYTES` is the regex's *upper bound* on a single
/// expect call — the actual drain depth is bounded by the 300 ms
/// timeout window in `drain_pty_buffer`, not by this constant.
const PTY_DRAIN_MAX_BYTES: usize = 8192;
fn drain_pty_buffer(session: &mut OsSession) {
    let guard = TimeoutGuard::new(session, Duration::from_millis(300), TIMEOUT);
    // Two back-to-back reads: the first consumes what's currently
    // buffered; the second catches bytes that arrived during the first
    // read's brief timeout window.
    let _ = guard
        .session
        .expect(Regex(&format!(r".{{0,{}}}", PTY_DRAIN_MAX_BYTES)));
    let _ = guard
        .session
        .expect(Regex(&format!(r".{{0,{}}}", PTY_DRAIN_MAX_BYTES)));
}

/// Send Ctrl-Z and wait for the foreground job's "Stopped" notification.
///
/// Drains prior line-editor echo before sending so the later `expect`
/// does not race on stale prompt bytes.
fn suspend_fg_job(session: &mut OsSession) {
    drain_pty_buffer(session);
    session.send("\x1a").unwrap();
    session
        .expect("Stopped")
        .expect("job did not stop after Ctrl-Z");
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn test_pty_echo_command() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("echo hello\r").unwrap();
    expect_output(&mut s, "hello", "echo output not found");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_ctrl_d_exits() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
    s.expect(Eof).expect("shell did not exit on Ctrl+D");
}

#[test]
fn test_pty_ctrl_c_interrupts_input() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Type something, then Ctrl+C
    s.send("partial input").unwrap();
    s.send("\x03").unwrap();

    // Should get a new prompt
    wait_for_prompt(&mut s);

    // Can still run commands
    s.send("echo ok\r").unwrap();
    expect_output(&mut s, "ok", "command after Ctrl+C failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_history_up_re_executes() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("echo first_cmd\r").unwrap();
    expect_output(&mut s, "first_cmd", "first command output not found");
    wait_for_prompt(&mut s);

    // Press Up then Enter to re-execute
    s.send("\x1b[A").unwrap(); // Up arrow (ANSI escape)
    s.send("\r").unwrap();
    expect_output(&mut s, "first_cmd", "history re-execution failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_fc_not_added_to_history() {
    // POSIX-strict fc rationale: "the fc command shall not be entered
    // into the history list". After running an fc invocation, up-arrow
    // must recall the command *before* fc, not the fc invocation.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("echo fc_probe\r").unwrap();
    expect_output(&mut s, "fc_probe", "seed command output not found");
    wait_for_prompt(&mut s);

    // Run an fc invocation with its listing muted; the trailing marker
    // gives us a reliable output anchor. The whole line's first word is
    // `fc`, so it is skipped from history as a unit.
    s.send("fc -l >/dev/null 2>&1; echo fc_done\r").unwrap();
    expect_output(&mut s, "fc_done", "fc marker output not found");
    wait_for_prompt(&mut s);

    // Up then Enter: must re-execute `echo fc_probe`, not the fc line.
    // If fc had been recorded, the recall would print fc_done instead
    // and the fc_probe expect below would time out.
    s.send("\x1b[A").unwrap(); // Up arrow (ANSI escape)
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "fc_probe",
        "up-arrow should recall the pre-fc command",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_read_until_prompt_ignores_transient_repaints() {
    // read_until_prompt historically bound to the transient `$ <partial>`
    // repaint the line editor emits after every keystroke. The idle-prompt
    // implementation must ride out those repaints: type the command
    // byte-by-byte (forcing a repaint per keystroke), then confirm the
    // helper captures the real command output and leaves the session
    // synchronized at the true post-command prompt.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    let cmd = "echo repaint_probe";
    for i in 0..cmd.len() {
        s.send(&cmd[i..i + 1]).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    s.send("\r").unwrap();

    let out = read_until_prompt(&mut s);
    assert!(
        out.contains("\nrepaint_probe") || out.contains("\r\nrepaint_probe"),
        "read_until_prompt returned before the command output: {:?}",
        out
    );

    // The session must be usable immediately after — proves we stopped
    // at the idle prompt, not mid-repaint.
    s.send("echo after_probe\r").unwrap();
    expect_output(
        &mut s,
        "after_probe",
        "session out of sync after read_until_prompt",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_backspace_editing() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Type "echoo", backspace, " works"
    s.send("echoo").unwrap();
    s.send("\x7f").unwrap(); // Backspace
    s.send(" works\r").unwrap();
    expect_output(&mut s, "works", "line editing with backspace failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_ps2_continuation() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Incomplete command: if true; then
    s.send("if true; then\r").unwrap();
    wait_for_ps2(&mut s);

    // Body — still incomplete (needs fi)
    s.send("echo continued\r").unwrap();
    wait_for_ps2(&mut s);

    s.send("fi\r").unwrap();
    expect_output(&mut s, "continued", "if-then-fi output not found");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_multiline_edit_previous_line() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Unclosed quote: Enter starts an in-editor continuation line.
    s.send("echo 'A\r").unwrap();
    wait_for_ps2(&mut s);

    // Second line, then move the cursor back INTO the first line (Up no
    // longer recalls history mid-buffer), jump to its end (Ctrl+E) and
    // append X before the newline. Submitting executes the edited buffer
    // `echo 'AX\nZ' QQ`, which prints "AX\nZ QQ".
    //
    // The quote placed mid-line ("Z' QQ") keeps the executed output
    // ("Z QQ") distinguishable from every echoed/repainted input frame,
    // which always contains the quote or a "> " prefix.
    s.send("Z' QQ").unwrap();
    s.send("\x1b[A").unwrap(); // Up — cursor to first line
    s.send("\x05").unwrap(); // Ctrl+E — end of first line
    s.send("X").unwrap();
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "AX\r\nZ QQ",
        "cross-line edit not reflected in output",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_multiline_taller_than_terminal() {
    let (mut s, _tmpdir) = spawn_yosh();
    // Shrink the window to 5 rows before editing so the construct below
    // (8 logical lines) exceeds the terminal height. The viewport-clamped
    // renderer must keep the buffer editable and submit it intact; the
    // pre-clamp renderer moved the cursor up past the top of the screen,
    // which a real terminal clamps — corrupting all later bookkeeping.
    s.get_process_mut()
        .set_window_size(60, 5)
        .expect("failed to shrink PTY window");
    wait_for_prompt(&mut s);

    // Queue the whole construct plus a cursor walk across off-screen rows
    // in one write; the editor processes queued input sequentially.
    let mut input = String::from("for i in a b c\rdo\r");
    for n in 1..=5 {
        input.push_str(&format!("echo body{}$i\r", n));
    }
    input.push_str(&"\x1b[A".repeat(4)); // Up into off-screen rows
    input.push_str(&"\x1b[B".repeat(4)); // and back down
    input.push_str("done\r");
    s.send(&input).unwrap();

    // Capture the renderer's raw output (escapes included) until the
    // executed loop output and the idle prompt appear, so the assertion
    // below can inspect the escape stream itself.
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        match s.try_read(&mut chunk) {
            Ok(0) => panic!(
                "EOF during multiline viewport capture: {:?}",
                strip_ansi(&raw)
            ),
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let text = strip_ansi(&raw);
                if text.contains("body5c") && text.ends_with("$ ") {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for loop output + idle prompt; captured: {:?}",
                    text
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("PTY read error: {}", e),
        }
    }

    // Executed output proves the buffer survived the off-screen cursor
    // walk: the echoed input frames always contain `$i`, so `body1a` /
    // `body5c` can only come from execution.
    let text = strip_ansi(&raw);
    for needle in ["body1a", "body5c"] {
        assert!(
            text.contains(needle),
            "loop output {:?} missing after submit: {:?}",
            needle,
            text
        );
    }

    // The corruption trigger itself: on a 5-row terminal, no repaint may
    // ever move the cursor up by more than 4 rows (`ESC [ n A`).
    let mut worst: u32 = 0;
    let mut i = 0;
    while i + 2 < raw.len() {
        if raw[i] == 0x1b && raw[i + 1] == b'[' {
            let mut j = i + 2;
            let mut n: u32 = 0;
            let mut has_digits = false;
            while j < raw.len() && raw[j].is_ascii_digit() {
                n = n * 10 + u32::from(raw[j] - b'0');
                has_digits = true;
                j += 1;
            }
            if j < raw.len() && raw[j] == b'A' {
                worst = worst.max(if has_digits { n } else { 1 });
            }
            i = j;
        } else {
            i += 1;
        }
    }
    assert!(
        worst <= 4,
        "renderer moved the cursor up {} rows on a 5-row terminal",
        worst
    );

    exit_shell(&mut s);
}

#[test]
fn test_pty_ctrl_r_history_search() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Build up history
    s.send("echo alpha\r").unwrap();
    expect_output(&mut s, "alpha", "first echo alpha failed");
    wait_for_prompt(&mut s);

    s.send("echo beta\r").unwrap();
    expect_output(&mut s, "beta", "echo beta failed");
    wait_for_prompt(&mut s);

    // Ctrl+R to search - wait for search UI, type query, then select and execute
    s.send("\x12").unwrap(); // Ctrl+R
    // Wait for the search UI query line to appear
    s.expect("2/2").expect("Ctrl+R search UI did not appear");
    // FuzzySearchUI::run() draws UI then enables raw mode — wait for transition
    wait_for_raw_mode(&s);

    // Type "echo alpha" to uniquely select it
    s.send("echo alpha").unwrap();
    // Wait for filter to narrow down to unique match
    s.expect("1/2")
        .expect("search query did not filter to unique match");

    s.send("\r").unwrap(); // Select from search
    // After selection, FuzzySearchUI exits and LineEditor re-enables raw mode
    wait_for_raw_mode(&s);
    s.send("\r").unwrap(); // Execute
    expect_output(&mut s, "alpha", "Ctrl+R history search failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_autosuggest_accept_with_right_arrow() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Execute a command to populate history
    s.send("echo autosuggest_test_value\r").unwrap();
    expect_output(&mut s, "autosuggest_test_value", "initial echo failed");
    wait_for_prompt(&mut s);

    // Type prefix "echo auto" — suggestion should appear
    s.send("echo auto").unwrap();
    // Brief pause for suggestion to render
    std::thread::sleep(Duration::from_millis(50));

    // Press Right arrow to accept the suggestion
    s.send("\x1b[C").unwrap(); // Right arrow (ANSI escape)
    // Brief pause for acceptance
    std::thread::sleep(Duration::from_millis(50));

    // Press Enter to execute
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "autosuggest_test_value",
        "autosuggest acceptance failed",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_tab_completion() {
    let (mut s, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Create a uniquely named file in the temp HOME directory
    let test_file = tmpdir.path().join("yosh_tab_test_unique.txt");
    std::fs::write(&test_file, "hello").unwrap();

    // cd to HOME (which is tmpdir)
    s.send("cd\r").unwrap();
    wait_for_prompt(&mut s);

    // Type "echo yosh_tab" then Tab to complete the filename
    s.send("echo yosh_tab").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap(); // Tab
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter to execute — echo will print the completed filename
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "yosh_tab_test_unique.txt",
        "Tab completion failed to complete and execute",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_command_completion() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // "ech" + Tab should complete to "echo" (builtin)
    s.send("ech").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Add " hello" and press Enter to execute "echo hello"
    s.send(" hello\r").unwrap();
    expect_output(&mut s, "hello", "Command completion for 'echo' failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_command_completion_after_pipe() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // "echo hello | ca" + Tab inserts the common prefix of ca* commands.
    // Then send "t" to ensure we have "cat", then Enter.
    s.send("echo hello | ca").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    // Send "t\r" in case Tab only completed up to the common prefix "ca"
    s.send("t\r").unwrap();
    expect_output(&mut s, "hello", "Command completion after pipe failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_path_completion_in_argument_position() {
    let (mut s, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Create a uniquely named file
    let test_file = tmpdir.path().join("yosh_argcomp_unique.txt");
    std::fs::write(&test_file, "content").unwrap();

    // cd to HOME
    s.send("cd\r").unwrap();
    wait_for_prompt(&mut s);

    // "cat yosh_argcomp" + Tab should path-complete to "yosh_argcomp_unique.txt"
    s.send("cat yosh_argcomp").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter — should print the file content
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "content",
        "Path completion in argument position failed",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_keyword() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Type "if" — should be highlighted as Keyword (Bold + Magenta)
    s.send("if").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Cancel with Ctrl+C
    s.send("\x03").unwrap(); // Ctrl+C to cancel
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_valid_command() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Type "echo hi" — echo should be highlighted as CommandValid (Bold + Green)
    s.send("echo hi\r").unwrap();
    expect_output(&mut s, "hi", "echo with highlighting failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_pipe() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Execute a pipe command with highlighting active
    s.send("echo pipe_ok | cat\r").unwrap();
    expect_output(&mut s, "pipe_ok", "pipe with highlighting failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn ansi_colored_prompt() {
    let (mut session, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    // Set PS1 to an ANSI-colored prompt using command substitution with printf
    session
        .send("PS1=$(printf '\\033[32m$ \\033[0m')\r")
        .unwrap();
    wait_for_raw_mode(&session);

    // The prompt should render and accept input
    session.expect("$").expect("colored prompt not found");
    wait_for_raw_mode(&session);

    session.send("echo hello\r").unwrap();
    expect_output(&mut session, "hello", "echo after colored prompt");

    exit_shell(&mut session);
}

#[test]
fn multi_line_prompt() {
    let (mut session, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    // Set a two-line PS1: info line + prompt char
    // Use printf to get the newline in the prompt
    session.send("PS1=$(printf 'info line\\n> ')\r").unwrap();
    wait_for_raw_mode(&session);

    session
        .expect(">")
        .expect("multi-line prompt char not found");
    wait_for_raw_mode(&session);

    session.send("echo works\r").unwrap();
    expect_output(&mut session, "works", "echo after multi-line prompt");

    exit_shell(&mut session);
}

#[test]
fn test_pty_sighup_saves_history() {
    let (mut s, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Execute a command so it gets added to history
    s.send("echo sighup_test_marker\r").unwrap();
    expect_output(&mut s, "sighup_test_marker", "echo output");
    wait_for_prompt(&mut s);

    // Send SIGHUP to the yosh process
    // get_process() returns &UnixProcess which Derefs to PtyProcess; pid() returns nix::unistd::Pid
    let pid = s.get_process().pid();
    unsafe {
        libc::kill(pid.as_raw(), libc::SIGHUP);
    }

    // Wait for yosh to exit
    let _ = s.expect(Eof);

    // Verify history file was written
    let histfile = tmpdir.path().join(".yosh_history");
    let contents =
        std::fs::read_to_string(&histfile).expect("history file should exist after SIGHUP");
    assert!(
        contents.contains("echo sighup_test_marker"),
        "history file should contain the command, got: {:?}",
        contents
    );
}

#[test]
fn test_pty_external_sigterm_ignored_at_prompt() {
    // POSIX sh: an interactive shell ignores untrapped SIGTERM. The
    // external-delivery variant exercises the read-loop gating: before
    // the fix, the signal handler set PENDING_EXIT_SIGNAL, the terminal
    // read loop returned Interrupted, and the REPL exited.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    let pid = s.get_process().pid();
    unsafe {
        libc::kill(pid.as_raw(), libc::SIGTERM);
    }
    // Give the async delivery a moment so the shell demonstrably
    // survives the signal (not just wins a race against it).
    std::thread::sleep(Duration::from_millis(200));

    s.send("echo alive-$?\r").unwrap();
    expect_output(&mut s, "alive-0", "shell should survive external SIGTERM");
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_untrapped_term_quit_int_ignored() {
    // Self-delivered TERM/QUIT/INT via the kill builtin: drained by
    // process_pending_signals after the command, then ignored at the
    // dispatch level. bash/dash survive all three silently.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    for sig in ["TERM", "QUIT", "INT"] {
        s.send(&format!("kill -{} $$\r", sig)).unwrap();
        wait_for_prompt(&mut s);
        s.send(&format!("echo alive-{}-$?\r", sig)).unwrap();
        expect_output(
            &mut s,
            &format!("alive-{}-0", sig),
            "shell should survive self-delivered signal",
        );
        wait_for_prompt(&mut s);
    }

    exit_shell(&mut s);
}

#[test]
fn test_pty_term_trap_fires_interactively() {
    // A user trap on TERM still runs — only the untrapped default is
    // ignored. The trap body prints trap-$((40+2)) so the expected
    // output "trap-42" cannot be matched against the echoed input.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("trap 'echo trap-$((40+2))' TERM\r").unwrap();
    wait_for_prompt(&mut s);

    s.send("kill -TERM $$\r").unwrap();
    expect_output(&mut s, "trap-42", "TERM trap should fire");
    wait_for_prompt(&mut s);

    s.send("echo still-alive-$?\r").unwrap();
    expect_output(&mut s, "still-alive-0", "shell should survive trapped TERM");
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_dollar_dash_contains_i() {
    // POSIX XCU 2.5.2: $- contains `i` for interactive shells. The
    // interactive REPL also enables monitor mode, so expect "im".
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("echo flags-$-\r").unwrap();
    expect_output(&mut s, "flags-im", "$- should contain i and m");
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_dollar_dash_keeps_i_in_command_sub() {
    // The command-sub child env runs with is_interactive=false (so it
    // stays killable at the dispatch level), but its $- must still
    // report `i` via the ShellMode::flag_i snapshot — bash/dash agree.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("echo sub-$(echo $-)\r").unwrap();
    expect_output(&mut s, "sub-im", "command-sub $- should contain i and m");
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_set_plus_m_disables_job_control() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Interactive shell starts with monitor=on; disable it
    s.send("set +m\r").unwrap();
    wait_for_prompt(&mut s);

    // fg should fail with "no job control"
    s.send("fg\r").unwrap();
    s.expect("no job control")
        .expect("fg should report 'no job control' after set +m");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_set_minus_m_reenables_job_control() {
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Disable then re-enable monitor mode
    s.send("set +m\r").unwrap();
    wait_for_prompt(&mut s);
    s.send("set -m\r").unwrap();
    wait_for_prompt(&mut s);

    // Start a foreground job
    s.send("sleep 100\r").unwrap();
    // Brief pause to let sleep start
    std::thread::sleep(Duration::from_millis(200));

    // Ctrl+Z to suspend
    s.send("\x1a").unwrap();

    // Shell should regain control and show prompt
    wait_for_prompt(&mut s);

    // jobs should show the stopped job
    s.send("jobs\r").unwrap();
    s.expect("Stopped")
        .expect("jobs should show Stopped after Ctrl+Z suspend");
    wait_for_prompt(&mut s);

    // Cleanup: kill the stopped job
    s.send("kill %1\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_wait_reaps_background_job() {
    // Regression: in monitor mode SIGCHLD is registered on the
    // self-pipe, and `wait` used to misread the drained SIGCHLD (the
    // child-exit notification itself) as an interrupting signal,
    // returning 128+SIGCHLD (148 on macOS) instead of the job's real
    // exit status.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("/bin/sleep 0 & wait $!; echo RC-$?\r").unwrap();
    expect_output(&mut s, "RC-0", "wait must return the job's exit status");
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_plus_m_repl_starts_without_job_control() {
    // Invocation `+m` on an interactive shell: monitor is off from
    // startup — `$-` has `i` but no `m`, and children keep SIG_DFL for
    // SIGTSTP because init_job_control_signals never ran (so there is
    // no SIG_IGN disposition to leak across fork+exec).
    let (mut s, _tmpdir) = spawn_yosh_with_args(&["+m"]);
    wait_for_prompt(&mut s);

    s.send("echo flags-$-\r").unwrap();
    // Trailing-newline anchor so `flags-i` cannot match as a prefix of
    // a wrong `flags-im`.
    s.expect(Regex(r"\r?\nflags-i\r?\n"))
        .expect("+m REPL must report i without m in $-");
    wait_for_prompt(&mut s);

    // Child SIGTSTP disposition: a background /bin/sh that sends
    // itself SIGTSTP must actually stop (SIG_DFL) and never reach its
    // echo. The marker is quote-split in the typed command so the
    // contiguous string can only appear as real (buggy) output.
    s.send("/bin/sh -c 'kill -TSTP $$; echo TSTP-\"IGNORED\"' &\r")
        .unwrap();
    wait_for_prompt(&mut s);
    s.send("sleep 0.4; echo probe-\"done\"\r").unwrap();
    let m = s
        .expect(Regex(r"probe-done"))
        .expect("probe command did not complete");
    let before = String::from_utf8_lossy(m.before()).to_string();
    assert!(
        !before.contains("TSTP-IGNORED"),
        "+m child inherited SIG_IGN for SIGTSTP: {:?}",
        before
    );
    wait_for_prompt(&mut s);
    // SIGKILL removes the stopped child without needing SIGCONT first.
    s.send("kill -9 $!\r").unwrap();
    wait_for_prompt(&mut s);

    // A later `set -m` re-enables job control using the termios
    // snapshot captured at startup (captured even under +m): after a
    // foreground job that switched the terminal to raw mode is
    // suspended, the shell must restore cooked mode from it.
    s.send("set -m\r").unwrap();
    wait_for_prompt(&mut s);
    s.send("stty raw; sleep 30\r").unwrap();
    wait_for_raw_mode(&s);
    suspend_fg_job(&mut s);
    wait_for_prompt(&mut s);
    s.send("stty -a\r").unwrap();
    s.expect(Regex(r"[^\-]icanon"))
        .expect("terminal was not restored from the +m-captured termios snapshot");
    wait_for_prompt(&mut s);

    s.send("kill -9 %1\r").unwrap();
    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_dash_m_c_foreground_job_control_setup() {
    // Foreground `yosh -m -c ...` on a terminal: the ownership gate
    // passes, so `m` lands in $- and the job-control terminal handoffs
    // (tcsetpgrp around external foreground children and pipelines,
    // background forks into their own process groups) must complete
    // without the shell or its children being stopped by SIGTTOU.
    let (mut s, _tmpdir) = spawn_yosh_with_args(&[
        "-m",
        "-c",
        "echo flags-$-; /bin/echo pipe-ok | /bin/cat; /bin/sleep 0 & wait $!; echo rc=$?",
    ]);
    s.expect(Regex(r"flags-[a-z]*m"))
        .expect("-m at a terminal must keep m in $-");
    s.expect("pipe-ok")
        .expect("external pipeline must run under -m -c");
    s.expect("rc=0")
        .expect("background job + wait must complete under -m -c");
    let _ = s.expect(Eof);
}

#[test]
fn test_pty_dash_m_with_redirected_stdin_keeps_job_control() {
    // Regression: the -m ownership gate used to probe stdin, so a
    // foreground `yosh -m ... <file` silently lost job control. The
    // gate now probes the controlling terminal (stderr / /dev/tty):
    // with stdin redirected to /dev/null but the shell foreground on
    // its tty, `m` must stay in $-.
    let bin = env!("CARGO_BIN_EXE_yosh");
    let tmpdir = helpers::TempDir::new();
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(format!("exec '{}' -m -c 'echo flags-$-' </dev/null", bin));
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());
    let mut s = expectrl::Session::spawn(cmd).expect("failed to spawn sh wrapper");
    s.set_expect_timeout(Some(TIMEOUT));
    s.expect(Regex(r"flags-[a-z]*m"))
        .expect("redirected stdin must not disable job control for a foreground -m shell");
    let _ = s.expect(Eof);
}

#[test]
fn test_pty_plus_i_forces_non_interactive_on_tty() {
    // `yosh +i` at a terminal must NOT start the REPL: the stream is
    // read to EOF and run as a script (bash agrees). The kernel's
    // cooked-mode echo shows the literal typed text (`:$-:`); the
    // executed output line carries the expanded flags, which must
    // contain neither `i` nor `m`. The character class below excludes
    // exactly those two letters, so a wrongly-interactive shell can
    // never satisfy the expect.
    let (mut s, _tmpdir) = spawn_yosh_with_args(&["+i"]);
    // No prompt/raw-mode sync: the non-interactive path leaves the
    // terminal in canonical mode. Send one script line, then EOF.
    s.send("echo :$-:\r").unwrap();
    s.send("\x04").unwrap();
    s.expect(Regex(r":[a-hj-ln-z]*:"))
        .expect("+i must run the stream as a non-interactive script");
    let _ = s.expect(Eof);
}

#[test]
fn test_pty_external_pipeline_not_stopped_by_sigttou() {
    // Regression test: a pipeline of two EXTERNAL commands (e.g.
    // `cat file | pbcopy`) was reported as "Stopped(SIGTTOU)" instead of
    // running to completion. Pipeline elements inherited monitor=on, so
    // each element forked its external command into a new process group
    // and called tcsetpgrp around it; whichever element called tcsetpgrp
    // while its (pipeline) process group was no longer the terminal's
    // foreground group received SIGTTOU, stopping the whole pipeline.
    // Two external commands make the collision deterministic — a builtin
    // first element does not reproduce it.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("/bin/echo pipe_ok | /bin/cat\r").unwrap();
    expect_output(&mut s, "pipe_ok", "external pipeline produced no output");
    wait_for_prompt(&mut s);

    // The pipeline must have completed: jobs reports nothing for it.
    drain_pty_buffer(&mut s);
    s.send("jobs; echo jobs_done\r").unwrap();
    let m = s
        .expect(Regex(r"\r?\njobs_done"))
        .expect("jobs did not complete");
    let before = String::from_utf8_lossy(m.before()).to_string();
    assert!(
        !before.contains("Stopped"),
        "pipeline was stopped by SIGTTOU: {:?}",
        before
    );

    exit_shell(&mut s);
}

#[test]
fn test_pty_pipeline_exit_status_is_last_element() {
    // Regression test: in monitor mode the pipeline's exit status was
    // taken from the last-REAPED process instead of the last pipeline
    // element. Here sleep (element 1) exits 0 AFTER false (element 2)
    // exits 1, so the buggy version reported $? = 0.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("/bin/sleep 0.2 | /usr/bin/false; echo RC=$?\r")
        .unwrap();
    expect_output(
        &mut s,
        "RC=1",
        "pipeline exit status should be the last element's",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_background_external_not_stopped_by_sigttou() {
    // Regression test: `sleep 0.3 &` with an external command was stopped
    // with SIGTTOU. The background subshell kept monitor=on, forked the
    // external command into its own new process group, and called
    // tcsetpgrp from a background process group — raising SIGTTOU.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("/bin/sleep 0.2 &\r").unwrap();
    wait_for_prompt(&mut s);
    std::thread::sleep(Duration::from_millis(600));

    drain_pty_buffer(&mut s);
    s.send("jobs; echo jobs_done\r").unwrap();
    let m = s
        .expect(Regex(r"\r?\njobs_done"))
        .expect("jobs did not complete");
    let before = String::from_utf8_lossy(m.before()).to_string();
    assert!(
        !before.contains("Stopped"),
        "background job was stopped by SIGTTOU: {:?}",
        before
    );
    assert!(
        before.contains("Done"),
        "background job should have completed with Done: {:?}",
        before
    );

    exit_shell(&mut s);
}

#[test]
fn test_pty_shell_termios_restored_after_stopped_job() {
    // Regression test for: a foreground job that modifies termios (here,
    // via `stty raw`) must not leave the shell stuck in raw mode after
    // Ctrl-Z. After suspension, the shell must be back in cooked/icanon.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Run `stty raw` then `sleep` in the same sequential job list. stty
    // modifies the terminal; sleep inherits raw mode. Sync on the PTY
    // actually being in raw mode (ICANON cleared) before sending Ctrl-Z
    // — this is more deterministic than a fixed sleep and matches what
    // the existing wait_for_raw_mode helper does for the line-editor
    // startup case.
    s.send("stty raw; sleep 30\r").unwrap();
    wait_for_raw_mode(&s);

    suspend_fg_job(&mut s);

    // After the stop notification, yosh should reach the next prompt in
    // cooked mode. Assert by running `stty -a` and looking for "icanon"
    // in its output — this only works if the terminal is truly in
    // canonical mode.
    wait_for_prompt(&mut s);
    s.send("stty -a\r").unwrap();
    // stty -a output includes flag names; "icanon" (without leading "-")
    // indicates canonical mode is ON. "-icanon" would indicate raw mode.
    s.expect(Regex(r"[^\-]icanon"))
        .expect("terminal was not restored to canonical mode after Ctrl-Z");

    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}

#[test]
fn test_pty_termios_preserved_across_suspend_fg() {
    // Regression test for: `stty -echo; cat` followed by Ctrl-Z then `fg`
    // must resume with echo still OFF, because job.saved_tmodes captured
    // "-echo" at suspend and restored it on fg.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Disable echo, then start cat (a foreground reader). The cat inherits
    // the -echo setting.
    s.send("stty -echo; cat\r").unwrap();

    // Let cat start reading, then suspend. suspend_fg_job drains the line
    // editor echo, sends Ctrl-Z, and waits for the "Stopped" notification.
    suspend_fg_job(&mut s);
    wait_for_prompt(&mut s);

    // Resume cat in the foreground.
    s.send("fg\r").unwrap();
    // DEVIATION from the task spec: the spec sent `\x04` (Ctrl-D) to EOF
    // cat, then ran `stty -a`. On macOS/BSD, `cat`'s read() returns EINTR
    // when SIGCONT is delivered (cat inherits SIG_DFL for SIGCONT, and
    // BSD does not auto-restart read() without SA_RESTART). /bin/cat does
    // not retry on EINTR, so it exits with "Interrupted system call"
    // immediately after fg. On Linux, read() on terminals auto-restarts
    // for SIG_DFL signals, so cat would keep running there. This is
    // platform behavior, not a yosh bug — yosh correctly leaves SIGCONT
    // as the kernel default for children, and SA_RESTART is a child-side
    // decision. Sending `\x04` after cat already died caused the shell
    // itself to receive Ctrl-D and exit, which produced a spurious EOF
    // on the later `stty -a` expect.
    //
    // Fix: wait for the post-cat prompt (cat self-terminates after fg)
    // and then check stty -a, skipping the explicit Ctrl-D.
    wait_for_prompt(&mut s);
    drain_pty_buffer(&mut s);

    // `cat` has exited: we hit the Task 6 restore path, which puts us back
    // in shell_tmodes (echo ON). That confirms the restore ran — but to
    // prove the DURING-fg state had echo OFF we would need a mid-resume
    // snapshot.
    //
    // This test is therefore an END-STATE test: after the full cycle,
    // echo is ON (shell_tmodes restored). Combined with Task 10's bg→fg
    // variant, we have coverage of both transitions.
    s.send("stty -a\r").unwrap();
    s.expect(Regex(r"[^\-]echo"))
        .expect("terminal echo should be restored after fg cycle completes");

    wait_for_prompt(&mut s);

    // Reset echo explicitly in case the test leaves the PTY in a weird state.
    s.send("stty echo\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_bg_then_fg_preserves_shell_termios_restoration() {
    // Variant of test_pty_termios_preserved_across_suspend_fg that exercises
    // the Ctrl-Z -> bg -> fg path. The `bg` builtin does not touch termios,
    // so all termios transitions happen in fg. End-state check: after the
    // full cycle, echo is restored (shell_tmodes applied by Task 6).
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("stty -echo; cat\r").unwrap();

    // Suspend cat. suspend_fg_job handles drain + Ctrl-Z + "Stopped" sync.
    suspend_fg_job(&mut s);
    wait_for_prompt(&mut s);

    s.send("bg\r").unwrap();
    wait_for_prompt(&mut s);

    s.send("fg\r").unwrap();
    // On macOS/BSD, cat resumed by fg exits immediately with EINTR (same
    // mechanism as test_pty_termios_preserved_across_suspend_fg). Sending
    // \x04 after cat is already dead hits the shell and causes spurious
    // exit, producing Eof on the later `stty -a` expect. Instead, rely on
    // cat self-terminating and wait for the next prompt.
    wait_for_prompt(&mut s);
    drain_pty_buffer(&mut s);

    s.send("stty -a\r").unwrap();
    s.expect(Regex(r"[^\-]echo"))
        .expect("terminal echo should be restored after bg-then-fg cycle");

    wait_for_prompt(&mut s);
    s.send("stty echo\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

// ── Spec-based completion ──────────────────────────────────────────────

#[test]
fn tab_spec_completion_inserts_candidate() {
    let (mut session, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    // Specs load lazily on first Tab, so writing after startup is safe.
    let dir = tmpdir.path().join(".config/yosh/completions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("echo.toml"),
        "[[args]]\nvalues = [\"alpha\", \"omega\"]\n",
    )
    .unwrap();

    session.send("echo al").unwrap();
    // Allow the editor to render before sending Tab (matches the other
    // tab-completion PTY tests; see TODO.md on fixed waits).
    std::thread::sleep(Duration::from_millis(100));
    session.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\r").unwrap();

    // Tab completed "al" → "alpha", so the command ran as `echo alpha`.
    expect_output(
        &mut session,
        "alpha",
        "spec completion did not insert candidate",
    );
    exit_shell(&mut session);
}

#[test]
fn tab_spec_completion_completes_flag_names() {
    let (mut session, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    let dir = tmpdir.path().join(".config/yosh/completions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("echo.toml"),
        "[[flags]]\nnames = [\"-C\"]\n\n[[flags]]\nnames = [\"--no-pager\"]\n",
    )
    .unwrap();

    // A dash-initial word completes from the level's flag spellings:
    // `--n` has the unique match `--no-pager`, inserted by one Tab.
    session.send("echo --n").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\r").unwrap();

    expect_output(
        &mut session,
        "--no-pager",
        "spec completion did not complete flag name",
    );
    exit_shell(&mut session);
}

#[test]
fn tab_spec_completion_offers_flags_on_empty_word() {
    let (mut session, tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    let dir = tmpdir.path().join(".config/yosh/completions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("echo.toml"),
        "[[flags]]\nnames = [\"--no-pager\"]\n",
    )
    .unwrap();

    // Flags are offered without typing `-`: on the empty word after
    // the command, the sole flag is the only candidate and one Tab
    // inserts it.
    session.send("echo ").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send("\r").unwrap();

    expect_output(
        &mut session,
        "--no-pager",
        "spec completion did not offer flag on empty word",
    );
    exit_shell(&mut session);
}
