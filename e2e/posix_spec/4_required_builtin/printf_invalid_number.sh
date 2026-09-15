#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf reports a malformed numeric operand, uses the parsed prefix, exits 1
# EXPECT_OUTPUT<<END
# 12
# 0
# rc=1
# END
# EXPECT_STDERR: not completely converted
printf '%d\n' 12abc
printf '%d\n' abc
echo "rc=$?"
