//! POSIX `hash` builtin.
//!
//! `hash [-r] [name...]`
//!
//! - No args: print the utility-hash cache (bash-style `hits\tcommand`
//!   table, sorted by name; nothing is printed when the cache is empty).
//!   POSIX leaves the format implementation-defined.
//! - `-r`: clear the cache.
//! - `name...`: for each name, search PATH and record its location in
//!   the cache. A `name` containing `/` is never searched for (POSIX
//!   command search skips PATH for such names), so — like dash and
//!   bash — it is silently ignored rather than cached under its
//!   basename, which would make a later bare `ls` run `./d2/ls`.

use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::exec::command::find_in_path;

pub fn builtin_hash(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Parse leading -X flags.
    let mut clear = false;
    let flags = scan_flag_words(args, |ch| match ch {
        'r' => {
            clear = true;
            true
        }
        _ => false,
    });
    let idx = match flags {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("yosh: {}", invalid_flag("hash", e.flag));
            return Ok(1);
        }
    };

    let operands = &args[idx..];

    if clear {
        env.utility_hash.clear();
    }

    if operands.is_empty() {
        if clear {
            return Ok(0);
        }
        // List the cache, sorted by name for determinism. bash-style
        // table: hit count = times the cached entry satisfied a lookup
        // without a fresh PATH walk.
        let mut names: Vec<&String> = env.utility_hash.keys().collect();
        names.sort();
        if !names.is_empty() {
            println!("hits\tcommand");
        }
        for name in names {
            if let Some(entry) = env.utility_hash.get(name) {
                println!("{:4}\t{}", entry.hits, entry.path.display());
            }
        }
        return Ok(0);
    }

    let mut exit_status = 0;
    for name in operands {
        if name.contains('/') {
            // Not subject to PATH search: nothing to remember.
            continue;
        }
        let path_var = env
            .vars
            .get("PATH")
            .map(|s| s.to_string())
            .unwrap_or_default();
        // On success find_in_path has already inserted into the cache.
        if find_in_path(name, &path_var, &mut env.utility_hash).is_none() {
            eprintln!("yosh: hash: {}: not found", name);
            exit_status = 1;
        }
    }
    Ok(exit_status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn r_flag_clears_cache() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(PathBuf::from("/bin/foo")),
        );
        let args = vec!["-r".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn slash_operand_is_ignored_not_cached() {
        let mut env = env_with_path("/bin:/usr/bin");
        // dash/bash: names with `/` are skipped silently (rc 0), whether
        // or not they exist, and never seed the cache under a basename.
        let args = vec![
            "/no/such/cmd_definitely_missing_12345".to_string(),
            "/bin/sh".to_string(),
        ];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn name_lookup_succeeds_for_sh() {
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut env = env_with_path(&path_var);
        let args = vec!["sh".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(env.utility_hash.contains_key("sh"));
    }

    #[test]
    fn no_args_empty_cache_returns_zero() {
        let mut env = env_with_path("/bin:/usr/bin");
        let r = builtin_hash(&[], &mut env).unwrap();
        assert_eq!(r, 0);
    }

    #[test]
    fn invalid_option_returns_one() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["-x".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn nonexistent_name_returns_one() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["definitely_no_such_cmd_98765".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn cache_hit_increments_hits_count() {
        use crate::exec::command::find_in_path;
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut env = env_with_path(&path_var);
        // `hash sh` inserts via find_in_path's PATH-walk (miss → 0 hits).
        let r = builtin_hash(&[String::from("sh")], &mut env).unwrap();
        assert_eq!(r, 0);
        assert_eq!(env.utility_hash.get("sh").map(|e| e.hits), Some(0));
        // A later lookup served from the cache counts as a hit.
        let _ = find_in_path("sh", &path_var, &mut env.utility_hash);
        assert_eq!(env.utility_hash.get("sh").map(|e| e.hits), Some(1));
    }

    #[test]
    fn r_with_operand_clears_then_lookups() {
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut env = env_with_path(&path_var);
        env.utility_hash.insert(
            "stale".to_string(),
            crate::env::HashEntry::new(PathBuf::from("/old/stale")),
        );
        let args = vec!["-r".to_string(), "sh".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(!env.utility_hash.contains_key("stale"));
        assert!(env.utility_hash.contains_key("sh"));
    }
}
