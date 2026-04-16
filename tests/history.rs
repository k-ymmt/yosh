use std::process::Command;

fn yosh_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_yosh"))
        .args(["-c", input])
        .output()
        .expect("failed to execute yosh")
}

#[test]
fn test_fc_empty_history_error() {
    let out = yosh_exec("fc -l");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("history is empty"));
}

#[test]
fn test_fc_is_special_builtin() {
    let out = yosh_exec("fc -l 2>/dev/null; echo $?");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with('1'));
}
