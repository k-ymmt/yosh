use std::ffi::CString;
use std::io::Write;

use crate::env::{FlowControl, ShellEnv, TrapAction};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;
use crate::parser::word::is_valid_name;

pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    let result = match name {
        ":" => Ok(0),
        "exit" => builtin_exit(args, executor),
        "export" => builtin_export(args, &mut executor.env),
        "unset" => builtin_unset(args, &mut executor.env),
        "readonly" => builtin_readonly(args, &mut executor.env),
        "return" => builtin_return(args, executor),
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
                // Runtime `set -m` shares the invocation `-m`
                // terminal-ownership gate: without the controlling
                // terminal, monitor stays off (and `m` stays out of
                // `$-`), matching `yosh -m ...` and bash. `set`
                // itself still succeeds.
                if !crate::signal::try_enable_monitor_mode() {
                    executor.env.mode.options.monitor = false;
                }
            }
            return ret;
        }
        "eval" => builtin_eval(args, executor),
        "exec" => builtin_exec(args, executor),
        "trap" => builtin_trap(args, &mut executor.env),
        "." => builtin_source(args, executor),
        "shift" => builtin_shift(args, &mut executor.env),
        "times" => builtin_times(args),
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
        // POSIX §2.12: inside a trap action, `exit` without an operand
        // uses the value $? had when the trap action started, not the
        // status of the last command in the trap action.
        executor
            .env
            .exec
            .trap_context_status
            .unwrap_or(executor.env.exec.last_exit_status)
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                // POSIX §2.8.1: a special-builtin error still terminates the
                // shell — diagnose and fall through to the normal exit
                // sequence with status 2 instead of returning to the caller.
                eprintln!("yosh: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    // POSIX §2.12: the EXIT trap action starts with `$?` equal to the
    // value the shell is exiting with (`trap 'echo $?' EXIT; exit 7`
    // prints 7). Store it before running any trap actions; the final
    // exit status stays `code` regardless of what the trap body runs.
    executor.env.exec.last_exit_status = code;
    executor.process_pending_signals();
    executor.execute_exit_trap();
    if executor.env.mode.is_interactive {
        executor.exit_requested = Some(code);
        Ok(code)
    } else {
        crate::exec::shell_exit(code);
    }
}

// POSIX XBD §12.2 Utility Syntax Guideline 10: `--` marks end of options.
// Shared by export / readonly / unset to keep operand validation consistent.
fn consume_end_of_options(args: &[String], idx: usize) -> usize {
    if args.get(idx).map(String::as_str) == Some("--") {
        idx + 1
    } else {
        idx
    }
}

fn builtin_export(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.15: `export -p` writes exported variables in re-input
    // format. Operands after `-p` are NOT dropped (POSIX Issue 8 /
    // bash behavior; dash ignores them, bash processes them):
    //   - `export -p name=value` performs the assignment + export
    //     exactly like `export name=value` (matches bash).
    //   - `export -p name` prints just that variable, if exported.
    let print_mode = args.first().map(String::as_str) == Some("-p");
    let opts_end = if print_mode { 1 } else { 0 };
    let start = consume_end_of_options(args, opts_end);
    let operands = &args[start..];

    if args.is_empty() || (print_mode && operands.is_empty()) {
        // Print all exported variables in POSIX re-input format
        let mut exported: Vec<(String, String)> = env.vars.environ().to_vec();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            crate::builtin::regular::write_stdout_decoded(
                &format!("export {}=\"{}\"", name, value),
                true,
            );
        }
        return Ok(0);
    }

    let mut status = 0;
    for arg in operands {
        let name = match arg.find('=') {
            Some(pos) => &arg[..pos],
            None => arg.as_str(),
        };
        if !is_valid_name(name) {
            eprintln!("yosh: export: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if let Some(pos) = arg.find('=') {
            let raw_value = &arg[pos + 1..];
            if let Err(e) = env.assign_var(name, raw_value) {
                eprintln!("yosh: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else if print_mode {
            // `export -p name`: print only this variable (if exported).
            if let Some((n, value)) = env
                .vars
                .environ()
                .iter()
                .find(|(n, _)| n == name)
                .map(|(n, v)| (n.clone(), v.clone()))
            {
                crate::builtin::regular::write_stdout_decoded(
                    &format!("export {}=\"{}\"", n, value),
                    true,
                );
            }
        } else {
            env.vars.export(name);
        }
    }
    Ok(status)
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.15 unset: unset [-fv] name...
    // -f removes function definitions; -v (default) removes variables.
    // Combining -f and -v is rejected with status 2.
    let mut mode_f = false;
    let mut mode_v = false;
    let mut idx = 0;
    while idx < args.len() {
        let arg = args[idx].as_str();
        if arg == "--" {
            idx = consume_end_of_options(args, idx);
            break;
        }
        if arg == "-" || !arg.starts_with('-') || arg.len() == 1 {
            break;
        }
        for ch in arg[1..].chars() {
            match ch {
                'f' => mode_f = true,
                'v' => mode_v = true,
                _ => {
                    eprintln!("yosh: unset: -{}: invalid option", ch);
                    return Ok(2);
                }
            }
        }
        idx += 1;
    }
    if mode_f && mode_v {
        eprintln!("yosh: unset: cannot simultaneously unset a function and a variable");
        return Ok(2);
    }
    let unset_functions = mode_f;

    let mut status = 0;
    for name in &args[idx..] {
        if !is_valid_name(name) {
            eprintln!("yosh: unset: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if unset_functions {
            env.functions.remove(name);
        } else if let Err(e) = env.unset_var(name) {
            eprintln!("yosh: unset: {}", e);
            status = 1;
        }
    }
    Ok(status)
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.15 set: "When invoked with no arguments or with the -p
    // option, readonly shall write...". Only `-p` in the first position
    // triggers listing; `-p` after operands or after `--` (end of
    // options, XBD §12.2 Guideline 10) is validated as a bad identifier.
    // Operands after `-p` are NOT dropped (POSIX Issue 8 / bash
    // behavior; dash ignores them): assignments are performed + marked
    // readonly, name-only operands print just that variable.
    // Mirrors builtin_export.
    let print_mode = args.first().map(String::as_str) == Some("-p");
    let opts_end = if print_mode { 1 } else { 0 };
    let start = consume_end_of_options(args, opts_end);
    let operands = &args[start..];

    if args.is_empty() || (print_mode && operands.is_empty()) {
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.to_string(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            crate::builtin::regular::write_stdout_decoded(
                &format!("readonly {}={}", name, value),
                true,
            );
        }
        return Ok(0);
    }

    let mut status = 0;
    for arg in operands {
        let name = match arg.find('=') {
            Some(pos) => &arg[..pos],
            None => arg.as_str(),
        };
        if !is_valid_name(name) {
            eprintln!("yosh: readonly: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if let Some(pos) = arg.find('=') {
            let raw_value = &arg[pos + 1..];
            // assign_var (not vars.set): `readonly PATH=…` must
            // invalidate the utility hash like any other PATH write.
            if let Err(e) = env.assign_var(name, raw_value) {
                eprintln!("yosh: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else if print_mode {
            // `readonly -p name`: print only this variable (if readonly).
            if let Some(v) = env.vars.get_var(name).filter(|v| v.readonly) {
                let line = format!("readonly {}={}", name, v.value);
                crate::builtin::regular::write_stdout_decoded(&line, true);
            }
        } else {
            env.vars.set_readonly(name);
        }
    }
    Ok(status)
}

fn builtin_return(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let env = &mut executor.env;
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
                // POSIX §2.8.1: a special-builtin error terminates a
                // non-interactive shell with status 2 (matches dash).
                eprintln!("yosh: return: {}: numeric argument required", args[0]);
                env.exec.last_exit_status = 2;
                if !env.mode.is_interactive {
                    executor.exit_requested = Some(2);
                }
                return Ok(2);
            }
        }
    };
    env.exec.flow_control = Some(FlowControl::Return(code));
    Ok(code)
}

fn builtin_break(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if env.exec.loop_depth == 0 {
        eprintln!("yosh: break: only meaningful in a `for', `while', or `until' loop");
        return Ok(1);
    }
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
    let clamped = n.min(env.exec.loop_depth);
    env.exec.flow_control = Some(FlowControl::Break(clamped));
    Ok(0)
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if env.exec.loop_depth == 0 {
        eprintln!("yosh: continue: only meaningful in a `for', `while', or `until' loop");
        return Ok(1);
    }
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
    let clamped = n.min(env.exec.loop_depth);
    env.exec.flow_control = Some(FlowControl::Continue(clamped));
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
            crate::builtin::regular::write_stdout_decoded(&format!("{}={}", name, value), true);
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

fn builtin_exec(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }

    // POSIX §2.14 exec: when the utility cannot be executed, a
    // NON-interactive shell exits — 127 if not found, 126 if found but
    // not executable (matches bash/dash errno semantics). An interactive
    // shell prints the diagnostic and continues.
    fn exec_failure(executor: &mut Executor, msg: String, code: i32) -> Result<i32, ShellError> {
        eprintln!("yosh: {}", msg);
        executor.env.exec.last_exit_status = code;
        if !executor.env.mode.is_interactive {
            executor.exit_shell(code);
        }
        Ok(code)
    }

    let cmd = args[0].clone();

    // Resolve the executable path. If the command contains `/`, treat as
    // a relative or absolute path (byteenc-decoded so non-UTF-8 paths
    // reach the OS as their original bytes). Otherwise walk $PATH.
    let resolved_path: std::path::PathBuf = if cmd.contains('/') {
        use std::os::unix::ffi::OsStrExt;
        let bytes = crate::byteenc::decode_bytes(&cmd);
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
    } else {
        let env = &mut executor.env;
        let path_var = env
            .vars
            .get("PATH")
            .map(|s| s.to_string())
            .unwrap_or_default();
        match crate::exec::command::find_in_path(&cmd, &path_var, &mut env.utility_hash) {
            Some(p) => p,
            None => {
                return exec_failure(executor, format!("exec: {}: not found", cmd), 127);
            }
        }
    };

    let c_path = match CString::new(resolved_path.as_os_str().as_encoded_bytes()) {
        Ok(s) => s,
        Err(_) => {
            return exec_failure(executor, format!("exec: {}: invalid path", cmd), 126);
        }
    };

    // argv crosses execve as raw bytes: decode byteenc escapes (interior
    // NUL is still rejected — it cannot cross the boundary).
    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(crate::byteenc::decode_bytes(a.as_str()).into_owned()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                return exec_failure(executor, format!("exec: {}: invalid argument", a), 126);
            }
        }
    }

    // Build envp from currently-exported variables (byteenc-decoded).
    let envp: Vec<CString> = executor
        .env
        .vars
        .environ()
        .iter()
        .filter_map(|(k, v)| {
            let mut bytes = crate::byteenc::decode_bytes(k).into_owned();
            bytes.push(b'=');
            bytes.extend_from_slice(&crate::byteenc::decode_bytes(v));
            CString::new(bytes).ok()
        })
        .collect();

    let err = nix::unistd::execve(&c_path, &c_args, &envp).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => exec_failure(executor, format!("exec: {}: not found", cmd), 127),
        Errno::EACCES => {
            exec_failure(executor, format!("exec: {}: permission denied", cmd), 126)
        }
        Errno::ENOEXEC => {
            // POSIX execvp fallback (mirrors exec_external_with_redirects):
            // an executable file the kernel refuses to exec is re-run as a
            // shell script via /bin/sh, passing the resolved file path.
            let sh = CString::new("/bin/sh").expect("/bin/sh has no NUL");
            let mut sh_args = Vec::with_capacity(c_args.len() + 1);
            sh_args.push(sh.clone());
            sh_args.push(c_path.clone());
            sh_args.extend_from_slice(&c_args[1..]);
            let err2 = nix::unistd::execve(&sh, &sh_args, &envp).unwrap_err();
            exec_failure(executor, format!("exec: {}: {}", cmd, err2), 126)
        }
        _ => exec_failure(executor, format!("exec: {}: {}", cmd, err), 126),
    }
}

/// Install the OS disposition matching a trap-store change (POSIX: the
/// trap must take effect even for signals the shell has no standing
/// handler for, e.g. `trap 'cmd' ABRT`). No-ops for EXIT (0), unknown
/// conditions, and ignored-on-entry signals (§2.12 — the store already
/// no-oped those).
fn apply_trap_os_disposition(env: &ShellEnv, condition: &str, action: &TrapAction) {
    let Some(num) = crate::env::TrapStore::signal_name_to_number(condition) else {
        return;
    };
    if num == 0 || crate::signal::is_ignored_on_entry(num) {
        return;
    }
    let disposition = match action {
        TrapAction::Command(_) => crate::signal::TrapDisposition::Command,
        TrapAction::Ignore => crate::signal::TrapDisposition::Ignore,
        TrapAction::Default => crate::signal::TrapDisposition::Default,
    };
    crate::signal::apply_trap_disposition(num, disposition, env.mode.options.monitor);
}

fn builtin_trap(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX XBD §12.2 Guideline 10: a leading `--` marks the end of
    // options — required for the save/restore idiom `t=$(trap); eval
    // "$t"` to work against our own `trap -- '…' SIGNAME` output.
    let (args, saw_end_of_options) = if args.first().map(String::as_str) == Some("--") {
        (&args[1..], true)
    } else {
        (args, false)
    };
    if args.is_empty() {
        env.traps.display_all();
        return Ok(0);
    }
    if !saw_end_of_options && args[0] == "-p" {
        // POSIX 2024: with condition operands, print only the named
        // conditions; with none, print all traps. `--` may still follow.
        let conds = &args[1..];
        let conds = if conds.first().map(String::as_str) == Some("--") {
            &conds[1..]
        } else {
            conds
        };
        if conds.is_empty() {
            env.traps.display_all();
        } else {
            env.traps.display_conditions(conds);
        }
        return Ok(0);
    }
    if args.len() == 1 {
        env.traps.remove_trap(&args[0]);
        apply_trap_os_disposition(env, &args[0], &TrapAction::Default);
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
            apply_trap_os_disposition(env, sig, &action);
        } else if let Err(e) = env.traps.set_trap(sig, action.clone()) {
            eprintln!("yosh: {}", e);
            status = 1;
        } else {
            apply_trap_os_disposition(env, sig, &action);
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
    // Decode byteenc-escaped bytes so non-UTF-8 script paths resolve to
    // the real on-disk names.
    let decode_path = |s: &str| {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&crate::byteenc::decode_bytes(
            s,
        )))
    };
    let path = if filename.contains('/') {
        decode_path(filename)
    } else {
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.split(':') {
                let candidate = decode_path(dir).join(decode_path(filename));
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
            decode_path(filename)
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

fn builtin_times(args: &[String]) -> Result<i32, ShellError> {
    if !args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("times: unexpected operand: {}", args[0]),
        ));
    }
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

    let content = match std::fs::read(&tmp_path) {
        Ok(c) => crate::byteenc::encode_bytes(&c).into_owned(),
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

    #[test]
    fn unset_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("unset", &["1foo".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn export_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn unset_f_removes_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        assert!(executor.env.functions.contains_key("foo"));
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_f_keeps_variable_of_same_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        executor.env.vars.set("foo", "bar").unwrap();
        exec_special_builtin(
            "unset",
            &["-f".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(executor.env.vars.get("foo"), Some("bar"));
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_rejects_combined_f_v() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "-v".to_string(), "x".to_string()],
            &mut executor,
        );
        assert_eq!(status, 2);
    }

    #[test]
    fn unset_rejects_clustered_fv_flag() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-fv".to_string(), "x".to_string()],
            &mut executor,
        );
        assert_eq!(status, 2);
    }

    #[test]
    fn readonly_p_lists_readonly_var() {
        let mut executor = Executor::new("yosh", vec![]);
        exec_special_builtin("readonly", &["myvar=v".to_string()], &mut executor);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
        // The actual listing is on stdout (println!) which we don't capture here;
        // smoke-test via the e2e suite for output content.
    }

    #[test]
    fn export_p_with_assignment_assigns_and_exports() {
        // `export -p foo=v` must not silently drop the operand: the
        // assignment is performed and the variable exported (bash
        // behavior; POSIX Issue 8 permits operands with -p).
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "expvar=val1".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        let var = executor.env.vars.get_var("expvar").expect("must be set");
        assert_eq!(var.value, "val1");
        assert!(var.exported, "export -p name=value must export the var");
    }

    #[test]
    fn export_p_with_name_only_prints_without_exporting() {
        // `export -p name` prints that variable (stdout not captured
        // here) but must NOT export it — unlike plain `export name`.
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("plainvar", "v").unwrap();
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "plainvar".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        let var = executor.env.vars.get_var("plainvar").expect("must be set");
        assert!(
            !var.exported,
            "export -p name is print-only; it must not export"
        );
    }

    #[test]
    fn export_p_rejects_invalid_identifier_operand() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "1bad=v".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_p_with_assignment_assigns_and_marks_readonly() {
        // Same fix as export: `readonly -p foo=v` performs the
        // assignment and marks it readonly instead of dropping it.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "rovar=rv".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("rovar"), Some("rv"));
        assert!(executor.env.vars.is_readonly("rovar"));
    }

    #[test]
    fn readonly_p_with_name_only_prints_without_marking_readonly() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("notro", "v").unwrap();
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "notro".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(
            !executor.env.vars.is_readonly("notro"),
            "readonly -p name is print-only; it must not set the readonly flag"
        );
    }

    #[test]
    fn break_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("break", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("continue", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 1;
        let status = exec_special_builtin("continue", &["5".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(
            executor.env.exec.flow_control,
            Some(FlowControl::Continue(1))
        );
    }

    #[test]
    fn break_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 2;
        let status = exec_special_builtin("break", &["7".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(executor.env.exec.flow_control, Some(FlowControl::Break(2)));
    }

    #[test]
    fn consume_end_of_options_skips_double_dash() {
        let args = vec!["--".to_string(), "foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 0), 1);
    }

    #[test]
    fn consume_end_of_options_leaves_idx_when_not_double_dash() {
        let args = vec!["foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 0), 0);
    }

    #[test]
    fn consume_end_of_options_handles_empty_args() {
        let args: Vec<String> = vec![];
        assert_eq!(consume_end_of_options(&args, 0), 0);
    }

    #[test]
    fn consume_end_of_options_handles_idx_at_double_dash_mid_array() {
        let args = vec!["-f".to_string(), "--".to_string(), "x".to_string()];
        assert_eq!(consume_end_of_options(&args, 1), 2);
    }

    #[test]
    fn consume_end_of_options_handles_idx_out_of_range() {
        let args = vec!["foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 5), 5);
    }

    #[test]
    fn export_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "foo=hi".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("hi"));
    }

    #[test]
    fn export_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let before = executor.env.vars.environ().len();
        let status = exec_special_builtin("export", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
        let after = executor.env.vars.environ().len();
        assert_eq!(before, after);
    }

    #[test]
    fn export_double_dash_then_dash_p_is_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn export_dash_p_alone_remains_listing() {
        // Regression guard: -p as the only arg still triggers listing rc=0.
        // The listing branch returns early before the helper is reached.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn export_p_then_double_dash_remains_listing() {
        // Regression guard: `export -p --` triggers listing because
        // `args[0] == "-p"` matches first; helper is never reached.
        // The `--` is harmless: the listing branch returns Ok(0) before
        // consume_end_of_options is called, so the operand is silently
        // ignored.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "--".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn export_operand_then_dash_p_is_invalid_identifier() {
        // `export foo -p`: `-p` is not matched anywhere (export already
        // uses `args[0] == "-p"`), so foo is exported and `-p` is
        // rejected as a bad identifier (rc=1). Symmetric counterpart to
        // readonly_operand_then_dash_p_is_invalid_identifier.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["foo".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.exported)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "foo=ok".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("ok"));
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.readonly)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let before = executor
            .env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .count();
        let status = exec_special_builtin("readonly", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
        let after = executor
            .env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .count();
        assert_eq!(before, after);
    }

    #[test]
    fn readonly_double_dash_then_dash_p_is_invalid_identifier() {
        // `--` ends options (XBD §12.2 Guideline 10), so the trailing
        // `-p` is validated as an operand and rejected as a bad
        // identifier — mirrors
        // export_double_dash_then_dash_p_is_invalid_identifier.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_p_then_double_dash_remains_listing() {
        // Regression guard: `readonly -p --` triggers listing because
        // `args[0] == "-p"` matches first; helper is never reached.
        // The `--` is harmless: the listing branch returns Ok(0) before
        // consume_end_of_options is called, so the operand is silently
        // ignored. Mirrors export_p_then_double_dash_remains_listing.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "--".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn readonly_operand_then_dash_p_is_invalid_identifier() {
        // `readonly foo -p`: `-p` is no longer matched anywhere in args,
        // so foo is set readonly and `-p` is rejected as a bad
        // identifier (rc=1). Symmetric with export's operand-then-option
        // handling.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["foo".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.readonly)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_dash_p_alone_remains_listing() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn unset_double_dash_unsets_following_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("foo", "v").unwrap();
        let status = exec_special_builtin(
            "unset",
            &["--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), None);
    }

    #[test]
    fn unset_f_then_double_dash_unsets_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_v_then_double_dash_invalid_operand() {
        // After `-v --`, `-f` is an operand and must fail identifier validation.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-v".to_string(), "--".to_string(), "-f".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }
}
