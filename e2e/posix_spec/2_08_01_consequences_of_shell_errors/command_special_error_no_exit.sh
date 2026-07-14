#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A special built-in error invoked via command does not exit the shell
# EXPECT_OUTPUT: alive
# EXPECT_EXIT: 0
command . "$TEST_TMPDIR/no_such_file" 2>/dev/null
echo alive
