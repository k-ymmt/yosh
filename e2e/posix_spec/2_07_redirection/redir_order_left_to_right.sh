#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirections processed L-to-R: 2>&1 dupes fd2 to current fd1 before >f redirects fd1 to file; only stdout goes to file
# EXPECT_OUTPUT: out
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
# L-to-R: (1) 2>&1 dupes stderr to current stdout, (2) >f redirects stdout to file,
# (3) 2>/dev/null discards the now-duplicated stderr so it does not appear in captured output.
# With correct L-to-R only "out" reaches the file; with wrong R-to-L both "out" and "err"
# reach the file and cat f produces more than one line.
sh -c 'echo out; echo err >&2' 2>&1 >f 2>/dev/null
cat f
