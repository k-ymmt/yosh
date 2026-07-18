#!/bin/sh
# POSIX_REF: 2.15 exit
# DESCRIPTION: exit with non-numeric operand is an error
# EXPECT_STDERR: exit
# EXPECT_EXIT: 2
exit abc
