#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: $? is restored to its pre-trap value when a signal trap action completes
# EXPECT_OUTPUT<<END
# 0
# t
# 0
# END
# EXPECT_EXIT: 0
trap 'false' USR1
kill -USR1 $$
echo $?
trap 'echo t' USR1
kill -USR1 $$
echo $?
