//! Interactive shell startup: shell options and terminal ownership,
//! history and prompt defaults, plugins, the startup files and the
//! saved history.

use crate::exec::Executor;
use crate::signal;

use super::Repl;
use super::command_completion::CommandCompleter;
use super::highlight::HighlightScanner;
use super::history;
use super::line_editor::LineEditor;
use super::spec_completion::SpecStore;
use super::terminal::CrosstermTerminal;

impl Repl {
    pub fn new(
        shell_name: String,
        positional: Vec<String>,
        explicit_s: bool,
        login: bool,
        invocation_ops: &[crate::env::InvocationOp],
    ) -> Self {
        signal::init_signal_handling();
        signal::set_interactive_shell(true);
        let mut executor = Executor::new(shell_name, positional);
        set_interactive_options(&mut executor, explicit_s, login, invocation_ops);
        set_up_job_control(&mut executor);

        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        set_history_defaults(&mut executor, &home);

        set_default_prompts(&mut executor);

        // Load plugins
        executor.load_plugins(true);

        // Login shell: /etc/profile and ~/.profile first, then the
        // interactive files (zsh: zprofile before zshrc; dash: profile
        // before $ENV). `exit` in a profile ends the shell before the
        // first prompt (`run` checks `exit_requested`), so the
        // interactive files are skipped then.
        if login {
            source_login_files(&mut executor);
        }
        if executor.exit_requested.is_none() {
            source_startup_files(&mut executor);
        }

        load_history(&mut executor);

        let spec_store = SpecStore::from_home(&home);

        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
            scanner: HighlightScanner::new(),
            command_completer: CommandCompleter::new(),
            spec_store,
        }
    }
}

/// The interactive shell's environment and option defaults, then the
/// invocation-time `set` options on top of them.
fn set_interactive_options(
    executor: &mut Executor,
    explicit_s: bool,
    login: bool,
    invocation_ops: &[crate::env::InvocationOp],
) {
    crate::env::default_path::ensure_default_path(&mut executor.env);
    // POSIX §2.5.3: adopt the inherited PWD only if it names the
    // current directory (same check run_string performs).
    crate::env::startup_pwd::ensure_pwd(&mut executor.env);
    executor.env.mode.is_interactive = true;
    executor.env.mode.options.monitor = true;
    // Interactive default editing mode is emacs (bash parity); an
    // invocation `-o vi` below or a later `set -o vi` switches it.
    executor.env.mode.options.emacs = true;
    // `$-` reports `s` only for an explicit `yosh -s` (bash agrees:
    // an interactive bash without -s has no `s` in $-; dash differs
    // and always reports it when reading stdin).
    executor.env.mode.options.stdin_reads = explicit_s;
    // `$-` reports `l` for a login shell (`-l`, or argv[0] starting
    // with `-`), set before any startup file runs so the profile
    // files see it.
    executor.env.mode.options.login = login;
    // Invocation-time set options, applied after the interactive
    // monitor default so an explicit `+m` can turn it off (POSIX:
    // -m is on by default for interactive shells). Validated at
    // parse time in main.
    executor
        .env
        .mode
        .options
        .apply_invocation_ops(invocation_ops);
}

/// Job control setup: wait to be in the foreground, take the terminal
/// and snapshot its modes (see the comments for the `+m` and
/// no-terminal cases).
fn set_up_job_control(executor: &mut Executor) {
    if executor.env.mode.options.monitor {
        // A REPL launched in the background of a job-controlling
        // parent (`yosh &`) stops inside wait_until_foreground
        // (SIGTTIN) until the user foregrounds it, instead of
        // letting the take_terminal below steal the terminal from
        // the parent. With no controlling terminal at all, job
        // control is disabled, matching run_string's invocation
        // `-m` ownership gate.
        if signal::wait_until_foreground() {
            signal::init_job_control_signals();
            // Ensure shell has terminal
            crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();
        } else {
            executor.env.mode.options.monitor = false;
        }
    }
    // With invocation `+m` the monitor block above is skipped: leaving
    // SIGTSTP/SIGTTIN/SIGTTOU at SIG_DFL matches the state the
    // runtime `set +m` path produces via reset_job_control_signals,
    // so Ctrl-Z suspends the whole shell normally and children do
    // not inherit SIG_IGN for the job-control signals.

    // Snapshot the terminal's termios so we can restore it after every
    // foreground job completes. capture_tty_termios returns Ok(None)
    // silently if stdin is not a TTY. Captured even under invocation
    // `+m` so a later `set -m` still has a snapshot to restore from;
    // the symmetric `is_interactive && monitor` guard lives in
    // `restore_shell_termios_if_interactive`, which runs after each
    // foreground wait and re-checks the live monitor flag.
    if executor.env.mode.is_interactive
        && let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios()
    {
        executor.env.process.jobs.set_shell_tmodes(t);
    }
}

/// History variable defaults. Only fill in what is unset: values
/// inherited from the environment (`HISTFILE=/tmp/h yosh`) win, as in
/// bash.
fn set_history_defaults(executor: &mut Executor, home: &str) {
    let default_histfile = format!("{}/.yosh_history", home);
    for (name, default) in [
        ("HISTFILE", default_histfile.as_str()),
        ("HISTSIZE", "500"),
        ("HISTFILESIZE", "500"),
        ("HISTCONTROL", "ignoreboth"),
    ] {
        if executor.env.vars.get(name).is_none() {
            let _ = executor.env.vars.set(name, default);
        }
    }
}

/// Load history from file. After the startup files, as bash does, so a
/// HISTFILE / HISTSIZE set in ~/.yoshrc or $ENV governs which file is
/// read and how much of it is kept, not only where the history is saved
/// at exit.
fn load_history(executor: &mut Executor) {
    let histfile = executor.env.vars.get("HISTFILE").unwrap_or("").to_string();
    if !histfile.is_empty() {
        executor.env.history.load(std::path::Path::new(&histfile));
        // HISTSIZE bounds the loaded list too (bash: HISTSIZE=0
        // starts with an empty history).
        let (histsize, _) = history::history_settings(&executor.env.vars);
        executor.env.history.trim_to(histsize);
    }
}

/// POSIX XCU §2.5.3: PS1 and PS2 have default values for interactive
/// shells (the REPL and `sh -i` reading a non-terminal).
pub fn set_default_prompts(executor: &mut Executor) {
    // Set as real variables so observers like `[ -n "${PS1+x}" ]` see
    // them (dash and bash do). Defer to an inherited value if already
    // present.
    if executor.env.vars.get("PS1").is_none() {
        // SAFETY: getuid() is always safe to call.
        let default = if unsafe { libc::getuid() } == 0 {
            "# "
        } else {
            "$ "
        };
        let _ = executor.env.vars.set("PS1", default);
    }
    if executor.env.vars.get("PS2").is_none() {
        let _ = executor.env.vars.set("PS2", "> ");
    }
}

/// Run the login-shell startup files: `/etc/profile`, then
/// `$HOME/.profile` (the dash set; a yosh extension, POSIX defines no
/// login shell). Read by every login shell, interactive or not, before
/// the interactive files. A missing file is skipped silently; `exit`
/// in `/etc/profile` ends the shell without reading `~/.profile`.
pub fn source_login_files(executor: &mut Executor) {
    executor.source_file(std::path::Path::new("/etc/profile"));
    if executor.exit_requested.is_some() {
        return;
    }
    let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
    if !home.is_empty() {
        let profile = std::path::PathBuf::from(&home).join(".profile");
        executor.source_file(&profile);
    }
}

/// Run the interactive startup files: `~/.yoshrc`, then the file named
/// by `$ENV` (POSIX: only for interactive shells).
pub fn source_startup_files(executor: &mut Executor) {
    let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
    // Source ~/.yoshrc (yosh-specific startup file)
    if !home.is_empty() {
        let rc_path = std::path::PathBuf::from(&home).join(".yoshrc");
        executor.source_file(&rc_path); // Silent skip if absent
    }

    // Source $ENV (POSIX: parameter-expanded path for interactive shells).
    // Skipped when the rc file already ran `exit`: the shell is about
    // to terminate (`run` checks `exit_requested` before the first
    // prompt), so nothing further may run.
    if executor.exit_requested.is_none()
        && let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string())
        && !env_val.is_empty()
    {
        // POSIX 2.6.1: tilde expansion occurs before parameter expansion
        let home = executor.env.vars.get("HOME").map(|s| s.to_string());
        let after_tilde = crate::expand::expand_tilde_prefix(home.as_deref(), &env_val);

        // Parse as double-quoted word for parameter expansion
        let input = format!("\"{}\"", after_tilde);
        let expanded = match crate::lexer::Lexer::new(&input).next_token() {
            Ok(tok) => {
                if let crate::lexer::token::Token::Word(word) = tok.token {
                    crate::expand::expand_word_to_string(&mut executor.env, &word)
                        .ok()
                        .or_else(|| Some(after_tilde.clone()))
                } else {
                    Some(after_tilde.clone())
                }
            }
            Err(_) => Some(after_tilde.clone()),
        };
        if let Some(path) = expanded
            && executor.source_file(std::path::Path::new(&path)).is_none()
        {
            eprintln!("yosh: {}: No such file or directory", path);
        }
    }
}
