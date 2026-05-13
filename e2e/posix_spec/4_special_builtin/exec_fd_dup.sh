#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec 3>file then echo >&3 writes to file via the shell fd
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
exec 3>"$TEST_TMPDIR/out"
echo hello >&3
exec 3>&-
cat "$TEST_TMPDIR/out"
