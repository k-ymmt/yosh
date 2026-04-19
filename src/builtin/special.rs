use std::ffi::CString;
use std::io::Write;

use nix::unistd::execvp;

use crate::env::{FlowControl, ShellEnv, TrapAction};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;
use crate::expand::expand_tilde_in_assignment_value;

pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    let result = match name {
        ":" => Ok(0),
        "exit" => builtin_exit(args, executor),
        "export" => builtin_export(args, &mut executor.env),
        "unset" => builtin_unset(args, &mut executor.env),
        "readonly" => builtin_readonly(args, &mut executor.env),
        "return" => builtin_return(args, &mut executor.env),
        "break" => builtin_break(args, &mut executor.env),
        "continue" => builtin_continue(args, &mut executor.env),
        "set" => {
            let was_monitor = executor.env.mode.options.monitor;
            let ret = match builtin_set(args, &mut executor.env) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
                    return e.exit_code();
                }
            };
            let is_monitor = executor.env.mode.options.monitor;
            if was_monitor && !is_monitor {
                crate::signal::reset_job_control_signals();
            } else if !was_monitor && is_monitor {
                crate::signal::init_job_control_signals();
            }
            return ret;
        }
        "eval" => builtin_eval(args, executor),
        "exec" => builtin_exec(args, &mut executor.env),
        "trap" => builtin_trap(args, &mut executor.env),
        "." => builtin_source(args, executor),
        "shift" => builtin_shift(args, &mut executor.env),
        "times" => builtin_times(),
        "fc" => builtin_fc(args, executor),
        _ => Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("{}: not a special builtin", name),
        )),
    };
    match result {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}

// ---------------------------------------------------------------------------
// Existing implementations (moved from mod.rs)
// ---------------------------------------------------------------------------

fn builtin_exit(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let code = if args.is_empty() {
        executor.env.exec.last_exit_status
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("exit: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    executor.process_pending_signals();
    executor.execute_exit_trap();
    if executor.env.mode.is_interactive {
        executor.exit_requested = Some(code);
        Ok(code)
    } else {
        std::process::exit(code);
    }
}

fn builtin_export(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() || args[0] == "-p" {
        // Print all exported variables in POSIX re-input format
        let mut exported: Vec<(String, String)> = env.vars.environ().to_vec();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}=\"{}\"", name, value);
        }
        return Ok(0);
    }

    let home = env.vars.get("HOME").map(|s| s.to_string());
    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let raw_value = &arg[pos + 1..];
            let value =
                expand_tilde_in_assignment_value(home.as_deref(), raw_value);
            if let Err(e) = env.vars.set(name, &value) {
                eprintln!("yosh: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            // Just mark as exported (or create empty exported var)
            env.vars.export(arg);
        }
    }
    Ok(status)
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let mut status = 0;
    for name in args {
        if let Err(e) = env.vars.unset(name) {
            eprintln!("yosh: unset: {}", e);
            status = 1;
        }
    }
    Ok(status)
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        // Print all readonly variables
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.to_string(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            println!("readonly {}={}", name, value);
        }
        return Ok(0);
    }

    let home = env.vars.get("HOME").map(|s| s.to_string());
    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let raw_value = &arg[pos + 1..];
            let value =
                expand_tilde_in_assignment_value(home.as_deref(), raw_value);
            if let Err(e) = env.vars.set(name, &value) {
                eprintln!("yosh: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(arg);
        }
    }
    Ok(status)
}

fn builtin_return(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if env.vars.scope_depth() <= 1 && !env.mode.in_dot_script {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "return: can only return from a function or sourced script".to_string(),
        ));
    }
    let code = if args.is_empty() {
        env.exec.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("return: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    env.exec.flow_control = Some(FlowControl::Return(code));
    Ok(code)
}

fn builtin_break(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "break: loop count must be > 0".to_string(),
                ));
            }
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("break: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    env.exec.flow_control = Some(FlowControl::Break(n));
    Ok(0)
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "continue: loop count must be > 0".to_string(),
                ));
            }
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("continue: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    env.exec.flow_control = Some(FlowControl::Continue(n));
    Ok(0)
}

// ---------------------------------------------------------------------------
// Implementations for new builtins
// ---------------------------------------------------------------------------

fn builtin_set(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        // Display all variables sorted
        let mut vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .map(|(k, v)| (k.to_string(), v.value.clone()))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in vars {
            println!("{}={}", name, value);
        }
        return Ok(0);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            env.vars.set_positional_params(args[i + 1..].to_vec());
            return Ok(0);
        }
        if arg == "-" {
            env.mode.options.xtrace = false;
            env.mode.options.verbose = false;
            if i + 1 < args.len() {
                env.vars.set_positional_params(args[i + 1..].to_vec());
            }
            return Ok(0);
        }
        if arg == "-o" || arg == "+o" {
            let on = arg.starts_with('-');
            i += 1;
            if i >= args.len() {
                if on {
                    env.mode.options.display_all();
                } else {
                    env.mode.options.display_restorable();
                }
                return Ok(0);
            }
            if let Err(e) = env.mode.options.set_by_name(&args[i], on) {
                return Err(ShellError::runtime(RuntimeErrorKind::InvalidOption, e));
            }
            i += 1;
            continue;
        }
        if arg.starts_with('-') || arg.starts_with('+') {
            let on = arg.starts_with('-');
            for c in arg[1..].chars() {
                if let Err(e) = env.mode.options.set_by_char(c, on) {
                    return Err(ShellError::runtime(RuntimeErrorKind::InvalidOption, e));
                }
            }
            i += 1;
            continue;
        }
        // Remaining args are positional params
        env.vars.set_positional_params(args[i..].to_vec());
        return Ok(0);
    }
    Ok(0)
}

fn builtin_eval(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }
    let input = args.join(" ");
    match crate::parser::Parser::new_with_aliases(&input, &executor.env.aliases).parse_program() {
        Ok(program) => Ok(executor.exec_program(&program)),
        Err(e) => {
            eprintln!("yosh: eval: {}", e);
            Ok(2)
        }
    }
}

fn builtin_exec(args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }
    let cmd = &args[0];
    let c_cmd = match CString::new(cmd.as_str()) {
        Ok(s) => s,
        Err(_) => {
            return Err(ShellError::runtime(
                RuntimeErrorKind::ExecFailed,
                format!("exec: {}: invalid command name", cmd),
            ));
        }
    };
    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(a.as_str()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::ExecFailed,
                    format!("exec: {}: invalid argument", a),
                ));
            }
        }
    }
    let err = execvp(&c_cmd, &c_args).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => Err(ShellError::runtime(
            RuntimeErrorKind::CommandNotFound,
            format!("exec: {}: not found", cmd),
        )),
        Errno::EACCES => Err(ShellError::runtime(
            RuntimeErrorKind::PermissionDenied,
            format!("exec: {}: permission denied", cmd),
        )),
        _ => Err(ShellError::runtime(
            RuntimeErrorKind::ExecFailed,
            format!("exec: {}: {}", cmd, err),
        )),
    }
}

fn builtin_trap(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        env.traps.display_all();
        return Ok(0);
    }
    if args[0] == "-p" {
        env.traps.display_all();
        return Ok(0);
    }
    if args.len() == 1 {
        env.traps.remove_trap(&args[0]);
        return Ok(0);
    }
    let action_str = &args[0];
    let signals = &args[1..];
    let action = if action_str == "-" {
        TrapAction::Default
    } else if action_str.is_empty() {
        TrapAction::Ignore
    } else {
        TrapAction::Command(action_str.to_string())
    };
    let mut status = 0;
    for sig in signals {
        if matches!(action, TrapAction::Default) {
            env.traps.remove_trap(sig);
        } else if let Err(e) = env.traps.set_trap(sig, action.clone()) {
            eprintln!("yosh: {}", e);
            status = 1;
        }
    }
    Ok(status)
}

fn builtin_source(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            ".: filename argument required".to_string(),
        ));
    }
    let filename = &args[0];
    let path = if filename.contains('/') {
        std::path::PathBuf::from(filename)
    } else {
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.split(':') {
                let candidate = std::path::PathBuf::from(dir).join(filename);
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!(".: {}: not found", filename),
                    ));
                }
            }
        } else {
            std::path::PathBuf::from(filename)
        }
    };
    match executor.source_file(&path) {
        Some(status) => Ok(status),
        None => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            format!(".: {}: No such file or directory", path.display()),
        )),
    }
}

fn builtin_shift(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let n = if args.is_empty() {
        1usize
    } else {
        match args[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("shift: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    if n > env.vars.positional_params().len() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "shift: shift count out of range".to_string(),
        ));
    }
    env.vars
        .set_positional_params(env.vars.positional_params()[n..].to_vec());
    Ok(0)
}

fn builtin_times() -> Result<i32, ShellError> {
    let mut tms: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if unsafe { libc::times(&mut tms) } == u64::MAX {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "times: failed".to_string(),
        ));
    }
    let fmt = |t: libc::clock_t| -> String {
        let secs = t as f64 / ticks;
        let m = (secs / 60.0) as u64;
        let s = secs - (m as f64 * 60.0);
        format!("{}m{:.3}s", m, s)
    };
    println!("{} {}", fmt(tms.tms_utime), fmt(tms.tms_stime));
    println!("{} {}", fmt(tms.tms_cutime), fmt(tms.tms_cstime));
    Ok(0)
}

// ---------------------------------------------------------------------------
// fc built-in
// ---------------------------------------------------------------------------

fn builtin_fc(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if executor.env.history.entries().is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "fc: history is empty".to_string(),
        ));
    }

    let mut list_mode = false;
    let mut suppress_numbers = false;
    let mut reverse = false;
    let mut substitute_mode = false;
    let mut editor: Option<String> = None;
    let mut operands: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-e" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    "fc: -e: option requires an argument".to_string(),
                ));
            }
            editor = Some(args[i].clone());
        } else if arg.starts_with('-')
            && arg.len() > 1
            && arg.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic())
        {
            for ch in arg[1..].chars() {
                match ch {
                    'l' => list_mode = true,
                    'n' => suppress_numbers = true,
                    'r' => reverse = true,
                    's' => substitute_mode = true,
                    _ => {
                        return Err(ShellError::runtime(
                            RuntimeErrorKind::InvalidArgument,
                            format!("fc: -{}: invalid option", ch),
                        ));
                    }
                }
            }
        } else {
            operands.push(arg.clone());
        }
        i += 1;
    }

    if substitute_mode {
        return fc_substitute(&operands, executor);
    }

    // Clone history entries to release the immutable borrow on executor,
    // allowing fc_edit to take &mut Executor.
    let entries: Vec<String> = executor.env.history.entries().to_vec();
    let hist_len = entries.len();
    let (start, end) = fc_resolve_range(&operands, hist_len, list_mode, &entries);

    if list_mode {
        fc_list(&entries, start, end, suppress_numbers, reverse);
        Ok(0)
    } else {
        fc_edit(&entries, start, end, reverse, editor, executor)
    }
}

fn fc_resolve_one(spec: &str, default: usize, entries: &[String]) -> usize {
    if let Ok(n) = spec.parse::<i64>() {
        if n > 0 {
            ((n - 1) as usize).min(entries.len().saturating_sub(1))
        } else {
            entries.len().saturating_sub((-n) as usize)
        }
    } else {
        (0..entries.len())
            .rev()
            .find(|&i| entries[i].starts_with(spec))
            .unwrap_or(default)
    }
}

fn fc_resolve_range(
    operands: &[String],
    hist_len: usize,
    is_list: bool,
    entries: &[String],
) -> (usize, usize) {
    match operands.len() {
        0 => {
            if is_list {
                (hist_len.saturating_sub(16), hist_len.saturating_sub(1))
            } else {
                let last = hist_len.saturating_sub(1);
                (last, last)
            }
        }
        1 => {
            let idx = fc_resolve_one(&operands[0], hist_len.saturating_sub(1), entries);
            if is_list {
                (idx, hist_len.saturating_sub(1))
            } else {
                (idx, idx)
            }
        }
        _ => {
            let s = fc_resolve_one(&operands[0], hist_len.saturating_sub(1), entries);
            let e = fc_resolve_one(&operands[1], hist_len.saturating_sub(1), entries);
            (s, e)
        }
    }
}

fn fc_list(entries: &[String], start: usize, end: usize, suppress_numbers: bool, reverse: bool) {
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let range: Vec<usize> = if reverse ^ (start > end) {
        (lo..=hi).rev().collect()
    } else {
        (lo..=hi).collect()
    };
    for i in range {
        if suppress_numbers {
            println!("\t{}", entries[i]);
        } else {
            println!("{}\t{}", i + 1, entries[i]);
        }
    }
}

fn fc_edit(
    entries: &[String],
    start: usize,
    end: usize,
    reverse: bool,
    editor: Option<String>,
    executor: &mut Executor,
) -> Result<i32, ShellError> {
    let editor_cmd = editor
        .or_else(|| executor.env.vars.get("FCEDIT").map(|s| s.to_string()))
        .or_else(|| executor.env.vars.get("EDITOR").map(|s| s.to_string()))
        .unwrap_or_else(|| "/bin/ed".to_string());

    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let mut commands: Vec<&str> = (lo..=hi).map(|i| entries[i].as_str()).collect();
    if reverse {
        commands.reverse();
    }

    let tmp_path = match create_secure_tempfile("yosh_fc") {
        Ok(path) => path,
        Err(e) => {
            return Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fc: {}", e),
            ));
        }
    };
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = match OpenOptions::new().write(true).mode(0o600).open(&tmp_path) {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("fc: cannot open temp file: {}", e),
                ));
            }
        };
        for cmd in &commands {
            let _ = writeln!(file, "{}", cmd);
        }
    }

    use std::process::Command;
    let status = Command::new(&editor_cmd).arg(&tmp_path).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(s.code().unwrap_or(1));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ShellError::runtime(
                RuntimeErrorKind::CommandNotFound,
                format!("fc: {}: {}", editor_cmd, e),
            ));
        }
    }

    let content = match std::fs::read_to_string(&tmp_path) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fc: cannot read temp file: {}", e),
            ));
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if content.trim().is_empty() {
        return Ok(0);
    }

    executor.eval_string(&content);
    Ok(executor.env.exec.last_exit_status)
}

fn fc_substitute(operands: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let entries = executor.env.history.entries();
    if entries.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "fc: history is empty".to_string(),
        ));
    }

    let mut replacement: Option<(&str, &str)> = None;
    let mut target_spec: Option<&str> = None;

    for op in operands {
        if let Some(eq_pos) = op.find('=') {
            replacement = Some((&op[..eq_pos], &op[eq_pos + 1..]));
        } else {
            target_spec = Some(op.as_str());
        }
    }

    let idx = if let Some(spec) = target_spec {
        fc_resolve_one(spec, entries.len().saturating_sub(1), entries)
    } else {
        entries.len().saturating_sub(1)
    };

    let mut cmd = entries[idx].clone();
    if let Some((old, new)) = replacement {
        cmd = cmd.replacen(old, new, 1);
    }

    // Informational output — not an error
    eprintln!("{}", cmd);

    let histsize: usize = executor
        .env
        .vars
        .get("HISTSIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let histcontrol = executor
        .env
        .vars
        .get("HISTCONTROL")
        .unwrap_or("ignoreboth")
        .to_string();
    executor.env.history.add(&cmd, histsize, &histcontrol);

    executor.eval_string(&cmd);
    Ok(executor.env.exec.last_exit_status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::Executor;

    #[test]
    fn exit_builtin_sets_exit_requested_in_interactive_mode() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.mode.is_interactive = true;
        let status = exec_special_builtin("exit", &["42".to_string()], &mut executor);
        assert_eq!(status, 42);
        assert_eq!(executor.exit_requested, Some(42));
    }

    #[test]
    fn exit_builtin_uses_last_status_when_no_args() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.mode.is_interactive = true;
        executor.env.exec.last_exit_status = 7;
        exec_special_builtin("exit", &[], &mut executor);
        assert_eq!(executor.exit_requested, Some(7));
    }
}

/// Create a temporary file with a random name and restrictive permissions (0o600).
/// Uses `O_CREAT | O_EXCL` to atomically create the file, preventing TOCTOU races.
fn create_secure_tempfile(prefix: &str) -> Result<String, String> {
    use std::collections::hash_map::RandomState;
    use std::fs::OpenOptions;
    use std::hash::{BuildHasher, Hasher};
    use std::os::unix::fs::OpenOptionsExt;

    let tmp_dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());

    for _ in 0..16 {
        let s = RandomState::new();
        let mut hasher = s.build_hasher();
        hasher.write_u64(std::process::id() as u64);
        let rand_hex = format!("{:016x}", hasher.finish());
        let path = format!("{}/{}_{}", tmp_dir, prefix, rand_hex);

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot create temp file: {}", e)),
        }
    }

    Err("cannot create temp file: too many collisions".to_string())
}
