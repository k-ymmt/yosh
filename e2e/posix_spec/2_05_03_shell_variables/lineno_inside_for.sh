#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO inside a for loop is the same each iteration (body stays on same line)
# EXPECT_OUTPUT<<END
# 10
# 10
# 10
# END
# EXPECT_EXIT: 0
for i in 1 2 3; do echo $LINENO; done
