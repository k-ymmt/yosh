#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: A failed write to stdout makes echo and other output builtins return 1
# EXPECT_OUTPUT<<END
# 1
# 1
# 1
# 1
# 1
# END
# EXPECT_STDERR: echo: write error
# EXPECT_EXIT: 0
echo hi >&-; echo $?
umask >&- 2>/dev/null; echo $?
kill -l >&- 2>/dev/null; echo $?
type sh >&- 2>/dev/null; echo $?
alias a=b; alias >&- 2>/dev/null; echo $?
