#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Trailing newlines are removed from command substitution output
# EXPECT_OUTPUT: [foo]
# EXPECT_EXIT: 0
x=$(printf 'foo\n\n\n')
echo "[$x]"
