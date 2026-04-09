#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Non-ignored traps are reset in subshells
# EXPECT_EXIT: 0
trap 'echo main_trap' USR1
(
  output=$(trap)
  case "$output" in
    *USR1*) exit 1 ;;
    *) exit 0 ;;
  esac
)
