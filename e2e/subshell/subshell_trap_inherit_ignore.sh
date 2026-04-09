#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Ignored signals are inherited by subshells
# EXPECT_OUTPUT: survived
# XFAIL: USR1 signal kills subshell process even with trap '' (shell limitation)
trap '' USR1
(
  kill -USR1 $$
  echo survived
)
