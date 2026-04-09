#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Subshell exit code is reflected in $?
# EXPECT_OUTPUT<<END
# 0
# 42
# END
(exit 0)
echo "$?"
(exit 42)
echo "$?"
