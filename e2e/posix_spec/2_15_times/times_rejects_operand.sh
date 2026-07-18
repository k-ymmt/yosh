#!/bin/sh
# POSIX_REF: 2.15 times
# DESCRIPTION: times rejects operands (POSIX says times takes no operands)
# EXPECT_EXIT: 2
# EXPECT_STDERR: times: unexpected operand
times foo
