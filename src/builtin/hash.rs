//! POSIX `hash` builtin.
//!
//! `hash [-r] [name...]`
//!
//! - No args: print the utility-hash cache (bash-style `hits\tcommand`
//!   table, sorted by name; nothing is printed when the cache is empty).
//!   POSIX leaves the format implementation-defined.
//! - `-r`: clear the cache.
//! - `name...`: for each name, record its location. If `name` contains
//!   `/`, the path is taken as-is and validated. Otherwise it is
//!   searched via PATH and inserted into the cache.

use std::path::PathBuf;

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::exec::command::{find_in_path, is_executable_file};

pub fn builtin_hash(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Parse leading -X flags.
    let mut clear = false;
    let mut idx = 0;
    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'r' => clear = true,
                other => {
                    eprintln!("yosh: hash: -{}: invalid option", other);
                    return Ok(1);
                }
            }
        }
        idx += 1;
    }

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
            let path = PathBuf::from(name);
            if !is_executable_file(&path) {
                eprintln!("yosh: hash: {}: not found", name);
                exit_status = 1;
                continue;
            }
            // POSIX: cache the basename; the operand path becomes the value.
            let basename = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            env.utility_hash
                .insert(basename, crate::env::HashEntry::new(path));
        } else {
            let path_var = env
                .vars
                .get("PATH")
                .map(|s| s.to_string())
                .unwrap_or_default();
            match find_in_path(name, &path_var, &mut env.utility_hash) {
                Some(_) => {
                    // find_in_path already inserted into the cache.
                }
                None => {
                    eprintln!("yosh: hash: {}: not found", name);
                    exit_status = 1;
                }
            }
        }
    }
    Ok(exit_status)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn slash_path_to_nonexistent_returns_error() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["/no/such/cmd_definitely_missing_12345".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
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
