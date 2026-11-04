//! `exec` candidate runner: a bounded `sh -c` child whose stdout lines
//! become candidates.

/// Budget for `exec` candidate commands. Chosen to keep Tab latency
/// bounded; see completion.md.
pub const EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Run `sh -c <cmd>` and return its stdout lines (empty lines dropped).
/// A timeout kills the child; timeouts and non-zero exits both yield an
/// empty Vec. Runs as a child process so completion can never mutate
/// shell state.
///
/// `env`: `None` inherits the process environment (used by unit tests);
/// `Some(pairs)` runs the child with exactly `pairs` as its environment
/// (the shell's exported variables, snapshotted per-prompt).
pub fn run_exec(
    cmd: &str,
    timeout: std::time::Duration,
    env: Option<&[(String, String)]>,
) -> Vec<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(pairs) = env {
        command
            .env_clear()
            .envs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Vec::new(),
    };

    // Drain stdout on a helper thread so a pipe-buffer-filling child can
    // never deadlock the timeout loop below. Send the result over a
    // channel rather than joining: a backgrounded grandchild that
    // inherits the write end of the pipe can keep it open long after
    // this child exits, and joining unconditionally would then hang the
    // line editor.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Timed out, or the child could not be polled.
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let Some(status) = status else {
        // Timed out: the reader thread detaches and dies with the pipe.
        return Vec::new();
    };
    if !status.success() {
        return Vec::new();
    }
    // The child exited, but a backgrounded grandchild may still hold the
    // pipe's write end and delay EOF forever — bound the wait by the
    // remaining budget instead of joining unconditionally.
    let remaining = deadline.saturating_duration_since(std::time::Instant::now())
        + std::time::Duration::from_millis(100);
    let out = match rx.recv_timeout(remaining) {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };
    out.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}
