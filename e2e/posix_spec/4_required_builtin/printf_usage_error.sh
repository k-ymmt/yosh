#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf without a format operand is a usage error
# EXPECT_EXIT: 2
# EXPECT_STDERR: usage
printf
