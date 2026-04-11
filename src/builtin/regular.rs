use crate::env::ShellEnv;
use nix::unistd::Pid;

pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let is_dash = !args.is_empty() && args[0] == "-";

    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                eprintln!("kish: cd: HOME not set");
                return 1;
            }
        }
    } else if args[0] == "-" {
        match env.vars.get("OLDPWD") {
            Some(old) => old.to_string(),
            None => {
                eprintln!("kish: cd: OLDPWD not set");
                return 1;
            }
        }
    } else {
        args[0].clone()
    };

    // Save current directory BEFORE attempting chdir
    let old_pwd = std::env::current_dir().ok();

    match std::env::set_current_dir(&target) {
        Ok(_) => {
            // Only update OLDPWD after successful chdir
            if let Some(old) = old_pwd {
                let _ = env.vars.set("OLDPWD", old.to_string_lossy().to_string());
            }
            if is_dash {
                println!("{}", target);
            }
            // Update $PWD
            match std::env::current_dir() {
                Ok(cwd) => {
                    let cwd_str = cwd.to_string_lossy().into_owned();
                    let _ = env.vars.set("PWD", cwd_str);
                }
                Err(e) => {
                    eprintln!("kish: cd: could not determine new directory: {}", e);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("kish: cd: {}: {}", target, e);
            1
        }
    }
}

pub fn builtin_echo(args: &[String]) -> i32 {
    if args.first().map(|a| a.as_str()) == Some("-n") {
        print!("{}", args[1..].join(" "));
    } else {
        println!("{}", args.join(" "));
    }
    0
}

pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        for (name, value) in env.aliases.sorted_iter() {
            println!("alias {}='{}'", name, value);
        }
        return 0;
    }
    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            env.aliases.set(name, value);
        } else {
            match env.aliases.get(arg) {
                Some(value) => println!("alias {}='{}'", arg, value),
                None => {
                    eprintln!("kish: alias: {}: not found", arg);
                    status = 1;
                }
            }
        }
    }
    status
}

pub fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        eprintln!("kish: unalias: usage: unalias name [name ...]");
        return 2;
    }
    let mut status = 0;
    for arg in args {
        if arg == "-a" {
            env.aliases.clear();
        } else if !env.aliases.remove(arg) {
            eprintln!("kish: unalias: {}: not found", arg);
            status = 1;
        }
    }
    status
}

pub fn builtin_kill(args: &[String], shell_pgid: Pid) -> i32 {
    if args.is_empty() {
        eprintln!("kish: kill: usage: kill [-s sigspec | -signum] pid...");
        return 2;
    }

    if args[0] == "-l" {
        return kill_list(&args[1..]);
    }

    let (sig_num, pid_args) = match parse_kill_signal(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("kish: kill: {}", msg);
            return 2;
        }
    };

    let mut status = 0;
    for pid_str in pid_args {
        let pid: i32 = match pid_str.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: kill: {}: invalid pid", pid_str);
                status = 1;
                continue;
            }
        };
        // PID 0 means "caller's process group" to the kernel, but in a pipeline
        // subshell the caller's group is the pipeline's group. Substitute the
        // shell's original process group so kill 0 behaves as POSIX expects.
        let target = if pid == 0 {
            let gpid = shell_pgid.as_raw();
            if gpid <= 1 {
                eprintln!("kish: kill: invalid shell process group");
                status = 1;
                continue;
            }
            Pid::from_raw(-gpid)
        } else {
            Pid::from_raw(pid)
        };
        if let Err(e) = nix::sys::signal::kill(
            target,
            nix::sys::signal::Signal::try_from(sig_num).ok(),
        ) {
            eprintln!("kish: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    status
}

fn parse_kill_signal(args: &[String]) -> Result<(i32, &[String]), String> {
    if args[0] == "-s" {
        if args.len() < 3 {
            return Err("option requires an argument -- s".to_string());
        }
        let sig = crate::signal::signal_name_to_number(&args[1])?;
        Ok((sig, &args[2..]))
    } else if args[0] == "--" {
        Ok((libc::SIGTERM, &args[1..]))
    } else if args[0].starts_with('-') && args[0].len() > 1 {
        let spec = &args[0][1..];
        if let Ok(num) = spec.parse::<i32>() {
            Ok((num, &args[1..]))
        } else {
            let sig = crate::signal::signal_name_to_number(spec)?;
            Ok((sig, &args[1..]))
        }
    } else {
        Ok((libc::SIGTERM, args))
    }
}

fn kill_list(args: &[String]) -> i32 {
    if args.is_empty() {
        let names: Vec<&str> = crate::signal::SIGNAL_TABLE
            .iter()
            .map(|&(_, name)| name)
            .collect();
        println!("{}", names.join(" "));
        return 0;
    }
    for arg in args {
        if let Ok(num) = arg.parse::<i32>() {
            let sig = if num > 128 { num - 128 } else { num };
            match crate::signal::signal_number_to_name(sig) {
                Some(name) => println!("{}", name),
                None => {
                    eprintln!("kish: kill: {}: invalid signal number", arg);
                    return 1;
                }
            }
        } else {
            match crate::signal::signal_name_to_number(arg) {
                Ok(num) => println!("{}", num),
                Err(e) => {
                    eprintln!("kish: kill: {}", e);
                    return 1;
                }
            }
        }
    }
    0
}

pub fn builtin_umask(args: &[String]) -> i32 {
    if args.is_empty() {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{:04o}", current);
        return 0;
    }

    if args[0] == "-S" {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{}", umask_to_symbolic(current));
        return 0;
    }

    // Try octal parse first
    if args[0].chars().all(|c| c.is_ascii_digit()) {
        return umask_set_octal(&args[0]);
    }

    // Try symbolic parse
    umask_set_symbolic(&args[0])
}

fn umask_to_symbolic(mask: libc::mode_t) -> String {
    let perms = 0o777 & !mask;
    let fmt = |bits: libc::mode_t| -> String {
        let mut s = String::new();
        if bits & 4 != 0 { s.push('r'); }
        if bits & 2 != 0 { s.push('w'); }
        if bits & 1 != 0 { s.push('x'); }
        s
    };
    format!(
        "u={},g={},o={}",
        fmt((perms >> 6) & 7),
        fmt((perms >> 3) & 7),
        fmt(perms & 7),
    )
}

fn umask_set_octal(s: &str) -> i32 {
    for c in s.chars() {
        if !('0'..='7').contains(&c) {
            eprintln!("kish: umask: {}: invalid octal number", s);
            return 1;
        }
    }
    match libc::mode_t::from_str_radix(s, 8) {
        Ok(mode) => {
            unsafe { libc::umask(mode) };
            0
        }
        Err(_) => {
            eprintln!("kish: umask: {}: invalid octal number", s);
            1
        }
    }
}

fn umask_set_symbolic(s: &str) -> i32 {
    let current = unsafe { libc::umask(0) };
    unsafe { libc::umask(current) };

    let mut mask = current;

    for clause in s.split(',') {
        let bytes = clause.as_bytes();
        if bytes.is_empty() {
            eprintln!("kish: umask: {}: invalid symbolic mode", s);
            return 1;
        }

        let mut i = 0;
        let mut who_mask: libc::mode_t = 0;

        // Parse who (u/g/o/a)
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            match bytes[i] {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => unreachable!(),
            }
            i += 1;
        }

        // Default to 'a' if no who specified
        if who_mask == 0 {
            who_mask = 0o777;
        }

        // Parse operator (=, +, -)
        if i >= bytes.len() || !matches!(bytes[i], b'=' | b'+' | b'-') {
            eprintln!("kish: umask: {}: invalid symbolic mode", s);
            return 1;
        }
        let op = bytes[i] as char;
        i += 1;

        // Parse permissions (r/w/x)
        let mut perm_bits: libc::mode_t = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                _ => {
                    eprintln!("kish: umask: {}: invalid symbolic mode", s);
                    return 1;
                }
            }
            i += 1;
        }

        // Apply within the who mask
        let effective_perms = perm_bits & who_mask;

        match op {
            '=' => {
                // Clear who bits, then set umask to deny everything NOT in perm
                mask = (mask & !who_mask) | (who_mask & !effective_perms);
            }
            '+' => {
                // Adding permissions = clearing umask bits
                mask &= !effective_perms;
            }
            '-' => {
                // Removing permissions = setting umask bits
                mask |= effective_perms;
            }
            _ => unreachable!(),
        }
    }

    unsafe { libc::umask(mask) };
    0
}
