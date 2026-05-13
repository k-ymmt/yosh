#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $? after standalone $(...) reflects the substituted command's status
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
# XFAIL: non-POSIX deviation (yosh sets $? to 0 after standalone command substitution; exit status of substituted command is not propagated)
$(false)
echo $?
