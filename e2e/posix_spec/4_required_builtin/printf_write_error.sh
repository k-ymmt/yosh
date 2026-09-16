#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf reports a failed write (closed stdout) with status 1
# EXPECT_OUTPUT: status=1
# EXPECT_STDERR: write error
printf '%s\n' hello >&-
echo "status=$?"
