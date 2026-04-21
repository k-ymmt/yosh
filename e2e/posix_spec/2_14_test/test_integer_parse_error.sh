#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: non-integer operand to -eq reports exit 2
# EXPECT_EXIT: 2
[ abc -eq 0 ]
