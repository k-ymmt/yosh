#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Subshell exit status is propagated to parent
# EXPECT_OUTPUT<<END
# 0
# 1
# END
(true)
echo "$?"
(false)
echo "$?"
