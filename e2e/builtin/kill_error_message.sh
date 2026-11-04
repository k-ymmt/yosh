#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill reports a missing process with the C library message, not the errno symbol
# EXPECT_OUTPUT<<END
# yosh: kill: (999999999) - No such process
# 1
# END
# EXPECT_EXIT: 0
kill 999999999 2>&1; echo $?
