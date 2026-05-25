#!/bin/sh
# POSIX_REF: 2.14.11 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
readonly -- foo=ok || exit 99
echo "$foo"
