#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $(...) supports nesting
# EXPECT_OUTPUT: inner
# EXPECT_EXIT: 0
echo $(echo $(echo inner))
