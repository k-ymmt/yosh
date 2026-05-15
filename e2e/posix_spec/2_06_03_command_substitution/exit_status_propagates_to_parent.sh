#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $? after standalone $(...) reflects the substituted command's status
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
$(false)
echo $?
