#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Variable changes in subshell do not affect parent
# EXPECT_OUTPUT<<END
# after
# before
# END
x=before
(x=after; echo "$x")
echo "$x"
