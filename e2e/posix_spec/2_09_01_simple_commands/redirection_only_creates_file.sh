#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A command consisting only of redirections still applies the redirections
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
# XFAIL: not yet implemented (TODO: redirect-only command (no command word) should still apply redirections and create/truncate the file)
cd "$TEST_TMPDIR"
>f
test -f f && echo ok
