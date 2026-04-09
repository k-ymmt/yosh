use std::ffi::CString;

use nix::unistd::execvp;

use crate::env::{FlowControl, ShellEnv, TrapAction};
use crate::exec::Executor;

pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    match name {
        ":" => 0,
        "exit" => builtin_exit(args, executor),
        "export" => builtin_export(args, &mut executor.env),
        "unset" => builtin_unset(args, &mut executor.env),
        "readonly" => builtin_readonly(args, &mut executor.env),
        "return" => builtin_return(args, &mut executor.env),
        "break" => builtin_break(args, &mut executor.env),
        "continue" => builtin_continue(args, &mut executor.env),
        "set" => builtin_set(args, &mut executor.env),
        "eval" => builtin_eval(args, executor),
        "exec" => builtin_exec(args, &mut executor.env),
        "trap" => builtin_trap(args, &mut executor.env),
        "." => builtin_source(args, executor),
        "shift" => builtin_shift(args, &mut executor.env),
        "times" => builtin_times(),
        _ => {
            eprintln!("kish: {}: not a special builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Existing implementations (moved from mod.rs)
// ---------------------------------------------------------------------------

fn builtin_exit(args: &[String], executor: &mut Executor) -> i32 {
    let code = if args.is_empty() {
        executor.env.last_exit_status
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    executor.process_pending_signals();
    executor.execute_exit_trap();
    std::process::exit(code);
}

fn builtin_export(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() || args[0] == "-p" {
        // Print all exported variables in POSIX re-input format
        let mut exported: Vec<(String, String)> = env.vars.to_environ();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}=\"{}\"", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            // Just mark as exported (or create empty exported var)
            env.vars.export(arg);
        }
    }
    status
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> i32 {
    let mut status = 0;
    for name in args {
        if let Err(e) = env.vars.unset(name) {
            eprintln!("kish: unset: {}", e);
            status = 1;
        }
    }
    status
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all readonly variables
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            println!("readonly {}={}", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(arg);
        }
    }
    status
}

fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    let code = if args.is_empty() {
        env.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: return: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    env.flow_control = Some(FlowControl::Return(code));
    code
}

fn builtin_break(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: break: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: break: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(FlowControl::Break(n));
    0
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: continue: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: continue: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(FlowControl::Continue(n));
    0
}

// ---------------------------------------------------------------------------
// Implementations for new builtins
// ---------------------------------------------------------------------------

fn builtin_set(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Display all variables sorted
        let mut vars: Vec<(String, String)> = env.vars.vars_iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in vars {
            println!("{}={}", name, value);
        }
        return 0;
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            env.positional_params = args[i + 1..].to_vec();
            return 0;
        }
        if arg == "-" {
            env.options.xtrace = false;
            env.options.verbose = false;
            if i + 1 < args.len() {
                env.positional_params = args[i + 1..].to_vec();
            }
            return 0;
        }
        if arg == "-o" || arg == "+o" {
            let on = arg.starts_with('-');
            i += 1;
            if i >= args.len() {
                if on { env.options.display_all(); } else { env.options.display_restorable(); }
                return 0;
            }
            if let Err(e) = env.options.set_by_name(&args[i], on) {
                eprintln!("kish: {}", e);
                return 1;
            }
            i += 1;
            continue;
        }
        if arg.starts_with('-') || arg.starts_with('+') {
            let on = arg.starts_with('-');
            for c in arg[1..].chars() {
                if let Err(e) = env.options.set_by_char(c, on) {
                    eprintln!("kish: {}", e);
                    return 1;
                }
            }
            i += 1;
            continue;
        }
        // Remaining args are positional params
        env.positional_params = args[i..].to_vec();
        return 0;
    }
    0
}

fn builtin_eval(args: &[String], executor: &mut Executor) -> i32 {
    if args.is_empty() {
        return 0;
    }
    let input = args.join(" ");
    match crate::parser::Parser::new_with_aliases(&input, &executor.env.aliases).parse_program() {
        Ok(program) => executor.exec_program(&program),
        Err(e) => {
            eprintln!("kish: eval: {}", e);
            2
        }
    }
}

fn builtin_exec(args: &[String], _env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        return 0;
    }
    let cmd = &args[0];
    let c_cmd = match CString::new(cmd.as_str()) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("kish: exec: {}: invalid command name", cmd);
            return 126;
        }
    };
    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(a.as_str()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                eprintln!("kish: exec: {}: invalid argument", a);
                return 126;
            }
        }
    }
    let err = execvp(&c_cmd, &c_args).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => { eprintln!("kish: exec: {}: not found", cmd); 127 }
        Errno::EACCES => { eprintln!("kish: exec: {}: permission denied", cmd); 126 }
        _ => { eprintln!("kish: exec: {}: {}", cmd, err); 126 }
    }
}

fn builtin_trap(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        env.traps.display_all();
        return 0;
    }
    if args[0] == "-p" {
        env.traps.display_all();
        return 0;
    }
    if args.len() == 1 {
        env.traps.remove_trap(&args[0]);
        return 0;
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
            eprintln!("kish: {}", e);
            status = 1;
        }
    }
    status
}

fn builtin_source(args: &[String], executor: &mut Executor) -> i32 {
    if args.is_empty() {
        eprintln!("kish: .: filename argument required");
        return 2;
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
                None => { eprintln!("kish: .: {}: not found", filename); return 1; }
            }
        } else {
            std::path::PathBuf::from(filename)
        }
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("kish: .: {}: {}", path.display(), e); return 1; }
    };
    match crate::parser::Parser::new_with_aliases(&content, &executor.env.aliases).parse_program() {
        Ok(program) => executor.exec_program(&program),
        Err(e) => { eprintln!("kish: .: {}", e); 2 }
    }
}

fn builtin_shift(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1usize
    } else {
        match args[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: shift: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    if n > env.positional_params.len() {
        eprintln!("kish: shift: shift count out of range");
        return 1;
    }
    env.positional_params = env.positional_params[n..].to_vec();
    0
}

fn builtin_times() -> i32 {
    let mut tms: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if unsafe { libc::times(&mut tms) } == u64::MAX {
        eprintln!("kish: times: failed");
        return 1;
    }
    let fmt = |t: libc::clock_t| -> String {
        let secs = t as f64 / ticks;
        let m = (secs / 60.0) as u64;
        let s = secs - (m as f64 * 60.0);
        format!("{}m{:.3}s", m, s)
    };
    println!("{} {}", fmt(tms.tms_utime), fmt(tms.tms_stime));
    println!("{} {}", fmt(tms.tms_cutime), fmt(tms.tms_cstime));
    0
}
