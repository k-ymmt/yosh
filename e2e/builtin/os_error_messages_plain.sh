#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Builtin diagnostics carry the C library message alone, without Rust's "(os error N)" suffix or an errno name prefix
# EXPECT_OUTPUT<<END
# yosh: cd: file1: Not a directory
# yosh: echo: write error: Bad file descriptor
# yosh: kill: (999999999) - No such process
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
: > file1
cd file1 2>&1
echo hi 2>&1 >&-
kill 999999999 2>&1 || :
