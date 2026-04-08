#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap with no arguments displays current traps
# XFAIL: trap output not captured by command substitution in kish
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
output=$(trap)
case "$output" in
  *"echo bye"*) exit 0 ;;
  *) exit 1 ;;
esac
