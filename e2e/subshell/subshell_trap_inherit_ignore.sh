#!/bin/sh
# POSIX_REF: 2.13 Shell Execution Environment
# DESCRIPTION: Ignored signals are inherited by subshells
# EXPECT_OUTPUT: survived
trap '' USR1
(
  kill -USR1 $$
  echo survived
)
