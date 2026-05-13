#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $(...) substitutes the standard output of the enclosed command
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo $(echo hi)
