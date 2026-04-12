use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use expectrl::{session::OsSession, Eof, Expect, Regex, Session};

const TIMEOUT: Duration = Duration::from_secs(15);

// ── TempDir (inline, avoids pulling in mock_terminal via mod helpers) ─────

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("kish-pty-test-{}-{}", id, seq));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Returns (session, tmpdir). The tmpdir must be kept alive for the
/// duration of the test so that kish's HOME directory is not deleted.
fn spawn_kish() -> (OsSession, TempDir) {
    let bin = env!("CARGO_BIN_EXE_kish");
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("failed to spawn kish");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

fn wait_for_prompt(session: &mut OsSession) {
    session.expect("$ ").expect("prompt not found");
    wait_for_raw_mode();
}

fn wait_for_ps2(session: &mut OsSession) {
    session.expect("> ").expect("PS2 prompt not found");
    wait_for_raw_mode();
}

/// Brief pause to let kish finish enable_raw_mode() before the next send.
/// Without this, input sent immediately after the prompt may arrive while
/// kish is still transitioning from canonical to raw mode, causing the PTY
/// line discipline to buffer or transform input unexpectedly.
fn wait_for_raw_mode() {
    std::thread::sleep(Duration::from_millis(50));
}

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
    // Wait for EOF to ensure the kish process has fully exited before the
    // next test starts — avoids PTY resource contention between tests.
    let _ = session.expect(Eof);
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn test_pty_echo_command() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    s.send("echo hello\r").unwrap();
    expect_output(&mut s, "hello", "echo output not found");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_ctrl_d_exits() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
    s.expect(Eof).expect("shell did not exit on Ctrl+D");
}

#[test]
fn test_pty_ctrl_c_interrupts_input() {
    let (mut s, _tmpdir) = spawn_kish();
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
    let (mut s, _tmpdir) = spawn_kish();
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
fn test_pty_backspace_editing() {
    let (mut s, _tmpdir) = spawn_kish();
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
    let (mut s, _tmpdir) = spawn_kish();
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
fn test_pty_ctrl_r_history_search() {
    let (mut s, _tmpdir) = spawn_kish();
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
    s.expect("2/2 > ").expect("Ctrl+R search UI did not appear");
    // FuzzySearchUI::run() draws UI then enables raw mode — wait for transition
    wait_for_raw_mode();

    // Type "echo alpha" to uniquely select it
    s.send("echo alpha").unwrap();
    // Wait for filter to narrow down to unique match
    s.expect("1/1 > ").expect("search query did not filter to unique match");

    s.send("\r").unwrap(); // Select from search
    // After selection, FuzzySearchUI exits and LineEditor re-enables raw mode
    wait_for_raw_mode();
    s.send("\r").unwrap(); // Execute
    expect_output(&mut s, "alpha", "Ctrl+R history search failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_autosuggest_accept_with_right_arrow() {
    let (mut s, _tmpdir) = spawn_kish();
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
    expect_output(&mut s, "autosuggest_test_value", "autosuggest acceptance failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_tab_completion() {
    let (mut s, tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Create a uniquely named file in the temp HOME directory
    let test_file = tmpdir.path().join("kish_tab_test_unique.txt");
    std::fs::write(&test_file, "hello").unwrap();

    // cd to HOME (which is tmpdir)
    s.send("cd\r").unwrap();
    wait_for_prompt(&mut s);

    // Type "echo kish_tab" then Tab to complete the filename
    s.send("echo kish_tab").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap(); // Tab
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter to execute — echo will print the completed filename
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "kish_tab_test_unique.txt",
        "Tab completion failed to complete and execute",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_keyword() {
    let (mut s, _tmpdir) = spawn_kish();
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
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type "echo hi" — echo should be highlighted as CommandValid (Bold + Green)
    s.send("echo hi\r").unwrap();
    expect_output(&mut s, "hi", "echo with highlighting failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_pipe() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type a pipe expression — verify the highlighter handles the pipe operator
    // without crashing. Cancel with Ctrl+C instead of executing, since PTY
    // pipe execution is covered by other integration tests.
    s.send("echo hello | cat").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Cancel with Ctrl+C
    s.send("\x03").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
