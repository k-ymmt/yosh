#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: a trap set inside a subshell fires on subshell exit and does not affect the parent's EXIT trap
# EXPECT_OUTPUT<<END
# subshell
# parent
# END
# EXPECT_EXIT: 0
( trap 'echo subshell' EXIT; true )
trap 'echo parent' EXIT
