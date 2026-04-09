#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution strips all trailing newlines
# EXPECT_OUTPUT: a-end
x=$(printf 'a\n\n\n')
echo "${x}-end"
