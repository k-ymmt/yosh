//! `ulimit` regular builtin (POSIX.1-2024 XCU `ulimit`).

use crate::builtin::options::{invalid_option, scan_flag_words};
use crate::builtin::output::write_stdout_or_report;
use crate::error::ShellError;

/// `resource` argument type of `getrlimit`/`setrlimit` (glibc uses an
/// enum-backed unsigned type, everyone else `c_int`).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimResource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimResource = libc::c_int;

/// One resource `ulimit` can query/set (POSIX.1-2024: `-c -d -f -n -s
/// -t -v`).
#[derive(Debug, PartialEq)]
struct UlimitResource {
    opt: char,
    resource: RlimResource,
    /// Bytes (or other native units) per reported unit.
    unit: libc::rlim_t,
    /// `ulimit -a` description and unit label (bash layout).
    desc: &'static str,
    unit_name: &'static str,
}

/// Reporting order of `ulimit -a` (alphabetical by option, as bash).
const ULIMIT_RESOURCES: &[UlimitResource] = &[
    UlimitResource {
        opt: 'c',
        resource: libc::RLIMIT_CORE,
        unit: 512,
        desc: "core file size",
        unit_name: "blocks",
    },
    UlimitResource {
        opt: 'd',
        resource: libc::RLIMIT_DATA,
        unit: 1024,
        desc: "data seg size",
        unit_name: "kbytes",
    },
    UlimitResource {
        opt: 'f',
        resource: libc::RLIMIT_FSIZE,
        unit: 512,
        desc: "file size",
        unit_name: "blocks",
    },
    UlimitResource {
        opt: 'n',
        resource: libc::RLIMIT_NOFILE,
        unit: 1,
        desc: "open files",
        unit_name: "",
    },
    UlimitResource {
        opt: 's',
        resource: libc::RLIMIT_STACK,
        unit: 1024,
        desc: "stack size",
        unit_name: "kbytes",
    },
    UlimitResource {
        opt: 't',
        resource: libc::RLIMIT_CPU,
        unit: 1,
        desc: "cpu time",
        unit_name: "seconds",
    },
    UlimitResource {
        opt: 'v',
        resource: libc::RLIMIT_AS,
        unit: 1024,
        desc: "virtual memory",
        unit_name: "kbytes",
    },
];

/// The new limit given as the `ulimit` operand.
#[derive(Debug, PartialEq)]
enum UlimitValue {
    /// A count of the resource's reporting units.
    Units(libc::rlim_t),
    Unlimited,
}

/// Parsed `ulimit` invocation.
#[derive(Debug, PartialEq)]
struct UlimitRequest {
    hard: bool,
    soft: bool,
    /// Selected resources, in option order; empty means `-f`.
    resources: Vec<char>,
    all: bool,
    value: Option<UlimitValue>,
}

/// Argument-parse failures for `ulimit`. All map to exit status 1.
#[derive(Debug, PartialEq)]
enum UlimitArgError {
    UnknownOption(String),
    InvalidNumber(String),
    TooManyArgs,
}

/// Parse `ulimit [-H|-S] [-a | -c|-d|-f|-n|-s|-t|-v ...] [limit]`.
/// Pure: no syscalls. Options may be clustered (`-Hn`); `--` ends them;
/// a word after the options that starts with `-` is an operand (so
/// `-f -5` is an invalid number, not an option). The resource count is
/// a count of units (`unlimited` for no limit).
fn parse_ulimit(args: &[String]) -> Result<UlimitRequest, UlimitArgError> {
    let mut req = UlimitRequest {
        hard: false,
        soft: false,
        resources: Vec::new(),
        all: false,
        value: None,
    };
    let i = scan_flag_words(args, |c| {
        match c {
            'H' => req.hard = true,
            'S' => req.soft = true,
            'a' => req.all = true,
            c if ULIMIT_RESOURCES.iter().any(|r| r.opt == c) => req.resources.push(c),
            _ => return false,
        }
        true
    })
    .map_err(|e| UlimitArgError::UnknownOption(e.word.to_string()))?;
    match &args[i..] {
        [] => {}
        [op] => {
            req.value = Some(if op == "unlimited" {
                UlimitValue::Unlimited
            } else {
                op.parse::<libc::rlim_t>()
                    .map(UlimitValue::Units)
                    .map_err(|_| UlimitArgError::InvalidNumber(op.clone()))?
            });
        }
        _ => return Err(UlimitArgError::TooManyArgs),
    }
    Ok(req)
}

/// Render a limit in the resource's units, or `"unlimited"`.
fn format_limit(rlim: libc::rlim_t, unit: libc::rlim_t) -> String {
    if rlim == libc::RLIM_INFINITY {
        "unlimited".to_string()
    } else {
        (rlim / unit).to_string()
    }
}

/// One `ulimit -a` line in bash's layout: description, then the
/// `(unit, -X)` tag right-aligned to column 36, then the value.
fn ulimit_all_line(r: &UlimitResource, value: &str) -> String {
    let tag = if r.unit_name.is_empty() {
        format!("(-{})", r.opt)
    } else {
        format!("({}, -{})", r.unit_name, r.opt)
    };
    let pad = 36usize.saturating_sub(r.desc.len() + tag.len()).max(1);
    format!("{}{}{} {}", r.desc, " ".repeat(pad), tag, value)
}

fn get_rlimit(resource: RlimResource) -> std::io::Result<libc::rlimit> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rl` is a valid, aligned rlimit; getrlimit only writes it.
    if unsafe { libc::getrlimit(resource, &mut rl) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(rl)
}

/// `ulimit` — query or set resource limits (POSIX.1-2024 `-H -S -a -c
/// -d -f -n -s -t -v`). Without `-H`/`-S`, a query reports the soft
/// limit and a set changes both limits (bash, dash, ksh).
pub fn builtin_ulimit(args: &[String]) -> Result<i32, ShellError> {
    let req = match parse_ulimit(args) {
        Ok(r) => r,
        Err(UlimitArgError::UnknownOption(o)) => {
            eprintln!("yosh: {}", invalid_option("ulimit", &o));
            return Ok(1);
        }
        Err(UlimitArgError::InvalidNumber(n)) => {
            eprintln!("yosh: ulimit: {n}: invalid number");
            return Ok(1);
        }
        Err(UlimitArgError::TooManyArgs) => {
            eprintln!("yosh: ulimit: too many arguments");
            return Ok(1);
        }
    };

    let selected: Vec<&UlimitResource> = if req.all {
        ULIMIT_RESOURCES.iter().collect()
    } else if req.resources.is_empty() {
        ULIMIT_RESOURCES.iter().filter(|r| r.opt == 'f').collect()
    } else {
        req.resources
            .iter()
            .filter_map(|&c| ULIMIT_RESOURCES.iter().find(|r| r.opt == c))
            .collect()
    };

    let Some(value) = req.value else {
        // Query. More than one resource (or -a) uses the labelled layout.
        let labelled = req.all || selected.len() > 1;
        let mut out = String::new();
        for r in &selected {
            let rl = match get_rlimit(r.resource) {
                Ok(rl) => rl,
                Err(e) => {
                    eprintln!("yosh: ulimit: {e}");
                    return Ok(1);
                }
            };
            // -H alone reports the hard limit; otherwise the soft one.
            let lim = if req.hard && !req.soft {
                rl.rlim_max
            } else {
                rl.rlim_cur
            };
            let v = format_limit(lim, r.unit);
            if labelled {
                out.push_str(&ulimit_all_line(r, &v));
            } else {
                out.push_str(&v);
            }
            out.push('\n');
        }
        return Ok(write_stdout_or_report("ulimit", &out, false));
    };

    if req.all {
        eprintln!("yosh: ulimit: -a: cannot be combined with a limit");
        return Ok(1);
    }
    // Neither -H nor -S: set both (as bash/dash/ksh do).
    let (set_hard, set_soft) = if req.hard || req.soft {
        (req.hard, req.soft)
    } else {
        (true, true)
    };
    for r in selected {
        let new = match value {
            UlimitValue::Unlimited => libc::RLIM_INFINITY,
            UlimitValue::Units(n) => match n.checked_mul(r.unit) {
                Some(v) if v != libc::RLIM_INFINITY => v,
                _ => {
                    eprintln!("yosh: ulimit: {n}: limit out of range");
                    return Ok(1);
                }
            },
        };
        let mut rl = match get_rlimit(r.resource) {
            Ok(rl) => rl,
            Err(e) => {
                eprintln!("yosh: ulimit: {e}");
                return Ok(1);
            }
        };
        if set_hard {
            rl.rlim_max = new;
        }
        if set_soft {
            rl.rlim_cur = new;
        }
        // SAFETY: `rl` is a valid rlimit; setrlimit only reads it.
        if unsafe { libc::setrlimit(r.resource, &rl) } != 0 {
            eprintln!(
                "yosh: ulimit: {}: cannot modify limit: {}",
                r.desc,
                std::io::Error::last_os_error()
            );
            return Ok(1);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // ── parse_ulimit ─────────────────────────────────────────────────

    fn req(
        hard: bool,
        soft: bool,
        res: &str,
        all: bool,
        value: Option<UlimitValue>,
    ) -> UlimitRequest {
        UlimitRequest {
            hard,
            soft,
            resources: res.chars().collect(),
            all,
            value,
        }
    }

    #[test]
    fn test_parse_ulimit_show() {
        assert_eq!(
            parse_ulimit(&s(&[])),
            Ok(req(false, false, "", false, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-f"])),
            Ok(req(false, false, "f", false, None))
        );
    }

    #[test]
    fn test_parse_ulimit_set_blocks() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "100"])),
            Ok(req(false, false, "f", false, Some(UlimitValue::Units(100))))
        );
        assert_eq!(
            parse_ulimit(&s(&["100"])),
            Ok(req(false, false, "", false, Some(UlimitValue::Units(100))))
        );
    }

    #[test]
    fn test_parse_ulimit_unlimited() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "unlimited"])),
            Ok(req(false, false, "f", false, Some(UlimitValue::Unlimited)))
        );
    }

    #[test]
    fn test_parse_ulimit_posix_2024_options() {
        assert_eq!(
            parse_ulimit(&s(&["-a"])),
            Ok(req(false, false, "", true, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-Hn"])),
            Ok(req(true, false, "n", false, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-S", "-c", "--", "0"])),
            Ok(req(false, true, "c", false, Some(UlimitValue::Units(0))))
        );
        assert_eq!(
            parse_ulimit(&s(&["-n", "-s", "-t", "-v", "-d"])),
            Ok(req(false, false, "nstvd", false, None))
        );
    }

    #[test]
    fn test_parse_ulimit_unknown_option() {
        assert_eq!(
            parse_ulimit(&s(&["-Z"])),
            Err(UlimitArgError::UnknownOption("-Z".to_string()))
        );
        assert_eq!(
            parse_ulimit(&s(&["-nZ"])),
            Err(UlimitArgError::UnknownOption("-nZ".to_string()))
        );
    }

    #[test]
    fn test_parse_ulimit_invalid_number() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "abc"])),
            Err(UlimitArgError::InvalidNumber("abc".to_string()))
        );
        assert_eq!(
            parse_ulimit(&s(&["-f", "--", "-5"])),
            Err(UlimitArgError::InvalidNumber("-5".to_string()))
        );
    }

    #[test]
    fn test_parse_ulimit_too_many_args() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "1", "2"])),
            Err(UlimitArgError::TooManyArgs)
        );
    }

    // ── formatting ───────────────────────────────────────────────────

    #[test]
    fn test_format_limit() {
        assert_eq!(format_limit(libc::RLIM_INFINITY, 512), "unlimited");
        assert_eq!(format_limit(51200, 512), "100"); // 51200 / 512
        assert_eq!(format_limit(8192 * 1024, 1024), "8192");
        assert_eq!(format_limit(0, 1), "0");
    }

    #[test]
    fn test_ulimit_all_line_matches_bash_layout() {
        let find = |c| ULIMIT_RESOURCES.iter().find(|r| r.opt == c).unwrap();
        assert_eq!(
            ulimit_all_line(find('c'), "0"),
            "core file size          (blocks, -c) 0"
        );
        assert_eq!(
            ulimit_all_line(find('n'), "256"),
            "open files                      (-n) 256"
        );
        assert_eq!(
            ulimit_all_line(find('t'), "unlimited"),
            "cpu time               (seconds, -t) unlimited"
        );
    }

    // ── builtin_ulimit ───────────────────────────────────────────────

    #[test]
    fn test_ulimit_show_returns_ok() {
        // Read-only paths: succeed without altering any limit.
        assert_eq!(builtin_ulimit(&s(&["-f"])), Ok(0));
        assert_eq!(builtin_ulimit(&s(&["-a"])), Ok(0));
        assert_eq!(builtin_ulimit(&s(&["-Hn"])), Ok(0));
        assert_eq!(
            builtin_ulimit(&s(&["-S", "-c", "-s", "-t", "-v", "-d"])),
            Ok(0)
        );
    }

    #[test]
    fn test_ulimit_unknown_option_returns_one() {
        assert_eq!(builtin_ulimit(&s(&["-Z"])), Ok(1));
    }

    #[test]
    fn test_ulimit_invalid_number_returns_one() {
        assert_eq!(builtin_ulimit(&s(&["-f", "abc"])), Ok(1));
    }

    #[test]
    fn test_ulimit_set_to_current_hard_is_safe() {
        // Setting the limit to the current HARD value is safe inside the shared
        // test process: soft only ever rises toward hard (never causes SIGXFSZ),
        // and it never fails with EPERM. In normal environments RLIMIT_FSIZE is
        // RLIM_INFINITY, so this is a true no-op; with a finite, non-512-aligned
        // hard limit it could round the hard limit down by <512 bytes, which is
        // harmless. This exercises the setrlimit success path.
        let rl = get_rlimit(libc::RLIMIT_FSIZE).unwrap();
        let operand = if rl.rlim_max == libc::RLIM_INFINITY {
            "unlimited".to_string()
        } else {
            (rl.rlim_max / 512).to_string()
        };
        assert_eq!(builtin_ulimit(&s(&["-f", &operand])), Ok(0));
    }
}
