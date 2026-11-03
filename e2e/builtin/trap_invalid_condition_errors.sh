#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: Unknown signal numbers or names are errors (status 1), including for reset
# EXPECT_OUTPUT<<END
# 1
# 1
# 1
# listing:
# END
# EXPECT_EXIT: 0
trap x 999 2>/dev/null; echo $?
trap x -5 2>/dev/null; echo $?
trap - FOO 2>/dev/null; echo $?
echo "listing:$(trap)"
