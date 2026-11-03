#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: An expansion error in a redirection word is an expansion error: the command does not run and the shell exits
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: m
cd "$TEST_TMPDIR" || exit 1
/bin/echo ran > ${zz:?m}.log
echo after
