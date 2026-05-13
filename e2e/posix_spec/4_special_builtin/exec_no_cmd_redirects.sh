#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies redirections to the current shell
# XFAIL: harness limitation (/dev/tty unavailable in non-interactive test environment)
# EXPECT_OUTPUT: persistent
# EXPECT_EXIT: 0
exec >"$TEST_TMPDIR/out"
echo persistent
exec >/dev/tty 2>/dev/null || exec >&-
cat "$TEST_TMPDIR/out"
