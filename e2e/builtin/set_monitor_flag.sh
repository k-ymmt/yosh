#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: monitor starts off in scripts; set +m keeps m out of $-
# EXPECT_OUTPUT<<END
# has_m=no
# has_m=no
# END
# EXPECT_EXIT: 0

# Scripts start with monitor off. Whether `set -m` then enables it
# depends on controlling-terminal ownership (yosh gates the runtime
# transition like invocation -m; bash/dash set the flag regardless),
# so this test only asserts the terminal-independent states: the
# initial default and the set -m / set +m round-trip ending off.
# The tty-owning `set -m` path is covered by the PTY tests
# (tests/pty_interactive.rs) and the detached path by
# tests/parser_integration.rs::test_set_monitor_gated_without_terminal.
case "$-" in *m*) echo "has_m=yes" ;; *) echo "has_m=no" ;; esac

set -m
set +m
case "$-" in *m*) echo "has_m=yes" ;; *) echo "has_m=no" ;; esac
