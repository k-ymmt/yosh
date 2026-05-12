#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times rejects operands (POSIX says times takes no operands)
# EXPECT_EXIT: 2
# EXPECT_STDERR: times: unexpected operand
times foo
