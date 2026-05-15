#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A command consisting only of redirections still applies the redirections
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
>f
test -f f && echo ok
