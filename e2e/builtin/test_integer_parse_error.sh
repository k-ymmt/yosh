#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: non-integer operand to -eq reports exit 2
# EXPECT_EXIT: 2
[ abc -eq 0 ]
