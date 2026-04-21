//! interactive_smoke — runnable binary that drives yosh through a short
//! interactive scenario via expectrl. Not a Criterion bench; declared as
//! `harness = false` so that `cargo bench --bench interactive_smoke`
//! produces a plain binary that samply can profile directly.
//!
//! Scenario:
//!   1. spawn yosh on a PTY
//!   2. wait for the prompt
//!   3. send "echo hello\n", expect "hello" back
//!   4. send Tab (one completion attempt)
//!   5. send Up arrow (history recall)
//!   6. send "exit\n", expect EOF

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use expectrl::{Eof, Expect, Regex, Session};
// `Expect` brings the `expect` / `send` / `send_line` methods into scope —
// same import pattern used by tests/pty_interactive.rs.

const PROMPT_TIMEOUT: Duration = Duration::from_secs(10);

fn main() {
    let yosh_bin: PathBuf = option_env!("CARGO_BIN_EXE_yosh")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./target/profiling/yosh"));

    let tmpdir = tempfile::tempdir().expect("tempdir");

    let mut cmd = Command::new(&yosh_bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("spawn yosh");
    session.set_expect_timeout(Some(PROMPT_TIMEOUT));

    // 1. Prompt
    session.expect("$ ").expect("initial prompt");

    // 2. echo hello
    session.send_line("echo hello").expect("send echo");
    session.expect(Regex("hello")).expect("echo output");
    session.expect("$ ").expect("prompt after echo");

    // 3. Tab completion (send Tab, give yosh ~200ms, then clear the line)
    session.send("\t").expect("send tab");
    std::thread::sleep(Duration::from_millis(200));
    // Ctrl-U clears the line regardless of what tab inserted.
    session.send("\x15").expect("send ctrl-u");

    // 4. History recall: Up arrow recalls "echo hello", then Ctrl-U clears.
    session.send("\x1b[A").expect("send up arrow");
    std::thread::sleep(Duration::from_millis(200));
    session.send("\x15").expect("send ctrl-u");

    // 5. Exit
    session.send_line("exit").expect("send exit");
    session.expect(Eof).expect("eof after exit");
}
