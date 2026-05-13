#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Backtick form is equivalent to $(...) for simple cases
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo `echo hi`
