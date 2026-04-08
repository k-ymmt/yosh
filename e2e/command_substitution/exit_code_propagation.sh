#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Exit code of command substitution is propagated
# EXPECT_OUTPUT: 1
x=$(false)
echo "$?"
