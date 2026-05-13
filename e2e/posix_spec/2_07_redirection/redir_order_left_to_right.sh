#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirections are processed left-to-right; 2>&1 before >f sends stderr to current stdout
# XFAIL: not yet implemented (TODO: redirection left-to-right ordering; 2>&1 before >f should dup to original stdout not the post-redir target)
# EXPECT_OUTPUT: out
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo out; echo err >&2' 2>&1 >f
cat f
