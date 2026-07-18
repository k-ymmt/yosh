#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: exec with no command applies redirections to the current shell
# MIGRATED_TO: tests/pty_posix.rs::exec_redirect::no_cmd_redirects
# EXPECT_OUTPUT: persistent
# EXPECT_EXIT: 0
exec >"$TEST_TMPDIR/out"
echo persistent
exec >/dev/tty 2>/dev/null || exec >&-
cat "$TEST_TMPDIR/out"
