#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap with no operands writes currently-set traps to stdout
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
out=$(trap)
printf '%s\n' "$out" | grep -q "EXIT" || exit 1
