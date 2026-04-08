#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Nested subshells maintain isolation at each level
# EXPECT_OUTPUT<<END
# inner
# outer
# original
# END
x=original
(x=outer; (x=inner; echo "$x"); echo "$x")
echo "$x"
